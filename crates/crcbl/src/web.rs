//! The browser entry point, written once for every sample.
//!
//! A sample's `web.rs` is what the JS shim in `web/` calls: ten
//! `#[unsafe(no_mangle)] extern "C"` exports named after the demo, a status
//! code the page polls, and a log queue the page drains once a frame. The
//! *symbols* have to stay per-demo — two demos can be open in one browser, and
//! the shim looks each up by name — but nothing behind them does, and all of
//! that is here. [`web_exports!`] writes a sample's symbols; what is left in the
//! sample is its [`WebPending`] impl, because the options a game boots with and
//! the error it fails with are the game's own.
//!
//! What this module owns:
//!
//! * **The status codes.** They are a wire format: the shim in `web/` switches
//!   on the numbers, so a sample that renumbered them would break the page
//!   rather than fail to compile. One definition is the only way that stays
//!   true.
//! * **The log queue.** `crcbl::log` has no `console.log` on this target — an
//!   import would be the only one in the module — so lines are queued in wasm
//!   memory and the shim pulls them across one at a time. Bounded, because a
//!   page that stops draining must not grow the heap without limit.
//! * **The five-call protocol** — prepare, boot, one frame per
//!   `requestAnimationFrame`, status, shutdown — as [`App`], which is the state
//!   machine each sample used to write out for itself.
//!
//! # Start-up cannot block, so it is a state machine
//!
//! Device creation is polled across the whole HAL seam, because the promise
//! behind `requestDevice` is resolved by the page's event loop — the very loop a
//! blocking wait would be sitting inside, so a browser that blocked on it would
//! deadlock against itself and the tab would simply stop. [`App::frame`]
//! therefore does one of two different things depending on the stage: while
//! booting it polls [`WebPending::poll`], and once that yields a loop it runs
//! frames. Several `requestAnimationFrame` ticks pass before the first frame is
//! drawn, and that is the design rather than a delay to be optimised away.
//!
//! # The clock is the browser's
//!
//! [`std::time::Instant::now`] **panics** on `wasm32-unknown-unknown` — the
//! target has no time implementation at all — so [`crate::engine::Clock::new`]
//! cannot be used here, and neither can [`crate::core::log::init_logging`],
//! which stamps its logger with an `Instant`. [`App::boot`] builds the loop on
//! [`crate::engine::Clock::manual`] and tells it how far to step from the
//! `performance.now()` the shim passes to [`App::frame`]. The logger is
//! [`install_logger`]'s, and it reads that same `performance.now()` — kept by
//! [`App::frame`] — so a queued line carries the seconds-since-start prefix the
//! native sink writes, from the one clock this target has.
//!
//! # …and so is the frame limit
//!
//! A manual clock cannot wait, so a `[engine.video] frame_limit` or a `--fps`
//! reaches the page as a number to obey rather than as a sleep: [`App::frame`]
//! runs the same [`crate::engine::FramePacer`] the desktop limiter runs, and
//! skips the `requestAnimationFrame` ticks that fall inside a slot the last
//! frame already claimed. **The best a browser can do is choose which vsync
//! ticks to draw on.** `setTimeout` is coarser than a frame and is not aligned
//! to the display at all, so a cap that is not a divisor of the refresh rate
//! lands its frames on an uneven pattern of ticks and judders. That is the
//! platform's floor rather than this module's — and the only alternative on
//! offer is what was here before, which was no limit applied at all.
//!
//! # A sample's wasm module imports nothing of its own
//!
//! Every one of the ABIs a page drives is exports-plus-polling: JS calls in,
//! reads a buffer wasm owns, and never passes a pointer or a callback the other
//! way. Logging is the case that would most naturally have been an import —
//! `console.log` is right there — and it is the queue [`log_take`] drains
//! instead.
//!
//! That is worth the small awkwardness because of what it buys: **a sample's
//! wasm imports nothing**, which means the import list is a thing CI can assert
//! about. `web/tools/check-exports.mjs` does exactly that, so an accidental
//! `extern "C" { fn … }` somewhere in the engine turns into a failed check
//! rather than a `LinkError` in someone's browser. It used to have to tolerate
//! the ~340 imports `wasm-bindgen` generated for `wgpu`'s `web-sys` calls;
//! `crcbl-wgpu` is not a wasm dependency of the umbrella any more, so the
//! honest count is zero.

use core::cell::{Cell, RefCell};

use crate::engine::GameLoop;

// ---------------------------------------------------------------------------
// Status
// ---------------------------------------------------------------------------

/// Nothing has been prepared; the demo's `prepare` export has not run.
pub const STATUS_IDLE: u32 = 0;
/// Storage is installed and the shim may pre-load; no shell yet.
pub const STATUS_PREPARED: u32 = 1;
/// Waiting for the canvas's first size, or for the device promise.
pub const STATUS_BOOTING: u32 = 2;
/// Playing. Every `frame` export draws.
pub const STATUS_RUNNING: u32 = 3;
/// The loop ended on its own terms — the page asked it to close, or the window
/// went away. Not an error.
pub const STATUS_STOPPED: u32 = 4;
/// Something failed; the demo's `error_ptr` export says what.
pub const STATUS_FAILED: u32 = 5;
/// Running, but the simulation is stopped: the player pressed Escape, or the
/// canvas lost focus.
///
/// A separate code rather than a flag beside [`STATUS_RUNNING`], because the
/// page's status line is a *status* — it said "Playing." for as long as the demo
/// was alive, including while the canvas sat unfocused behind something else.
/// Numbered after [`STATUS_FAILED`] so the codes already published to the shim
/// keep their values.
pub const STATUS_PAUSED: u32 = 6;

/// The base URL a demo's `FetchSource` resolves asset keys against.
///
/// Relative to the *document*, so a demo served from `/crcbl/demos/breakout/`
/// fetches `/crcbl/demos/breakout/assets/<key>`. The trailing slash is required
/// by `FetchSource::new`, which refuses a base without one so that a key can
/// never be read as a scheme.
pub const ASSET_BASE: &str = "assets/";

// ---------------------------------------------------------------------------
// Logging
// ---------------------------------------------------------------------------

/// The most log lines held for the shim before the oldest is dropped.
///
/// A page that never drains must not grow wasm memory without bound; a page that
/// drains once per frame will never see this.
const MAX_LOG_LINES: usize = 512;

/// The longest log line handed to the shim, in bytes.
const MAX_LOG_LINE: usize = 1024;

/// The queue [`log_take`] drains.
#[derive(Default)]
struct LogQueue {
    lines: std::collections::VecDeque<String>,
    /// The line the shim is currently reading. Kept at a fixed capacity so its
    /// address does not move between a `take` and the [`log_ptr`] that follows.
    current: String,
    /// Lines dropped because the shim was not draining. Reported once, on the
    /// next line that fits, rather than silently.
    dropped: u64,
}

thread_local! {
    static LOG: RefCell<LogQueue> = RefCell::new(LogQueue::default());
}

thread_local! {
    /// `performance.now()` at the first frame, and the most recent one.
    ///
    /// The browser's clock, kept here so a queued line can carry the same
    /// seconds-since-start prefix the native sink writes. [`std::time::Instant`]
    /// is not available on this target and `console.log` is not imported — but
    /// the shim already hands `performance.now()` to [`App::frame`] every
    /// `requestAnimationFrame`, so the number is in the module already and only
    /// had to be kept.
    static CLOCK: Cell<Option<(f64, f64)>> = const { Cell::new(None) };
}

/// Records `now_ms` as the browser's current time, and the run's start if this
/// is the first one seen.
fn note_frame_time(now_ms: f64) {
    CLOCK.with(|clock| {
        let started = clock.get().map_or(now_ms, |(started, _)| started);
        clock.set(Some((started, now_ms)));
    });
}

/// Seconds since the first frame, or zero before there has been one.
///
/// Zero is honest rather than a placeholder: a line logged before the first
/// `requestAnimationFrame` — the whole of `prepare` and `boot` — really is at
/// the start of the run.
fn elapsed_seconds() -> f64 {
    CLOCK
        .with(Cell::get)
        .map_or(0.0, |(started, now)| (now - started).max(0.0) / 1000.0)
}

/// A [`log::Log`] that queues lines for the shim instead of writing them.
///
/// There is no `console.log` import: it would be the only one in the module.
/// The line is shaped exactly like the native sink's, so a browser log and a
/// terminal log read the same and the gate's matchers — which look at the
/// message, never the prefix — are indifferent to which produced them.
///
/// **It feeds the console's log ring as well**, through the one
/// [`crcbl_core::log::console::push`] the native sink also calls and before its
/// own filter, so the debug console draws the same lines in a browser as in a
/// terminal. The queue below stays the page's: that one is destructive — the
/// shim takes each line — and a panel reading it would take the lines away from
/// the browser console.
///
/// **No wall-clock banner here, unlike the native side.** `SystemTime::now`
/// panics on this target as surely as `Instant::now` does, and getting a date
/// would mean importing one from JS. The browser console stamps each line as
/// the shim prints it, which is the same information from a source that already
/// exists.
struct WebLogger;

impl log::Log for WebLogger {
    /// The engine's live filter, per target, which is what
    /// [`install_logger`] registers this sink for.
    ///
    /// Not `metadata.level() <= log::max_level()`, which is what it was: that
    /// one reading is the facade's single global maximum, so
    /// `log warn,crcbl_vk=trace` typed at the console in a browser could only
    /// ever have meant "trace, everywhere". The directives are the terminal's
    /// now — `crcbl_core::log::sink_permits` is the same predicate
    /// `StderrLogger::permits` uses — and the maximum stays in front of it as
    /// the facade's cheap gate.
    fn enabled(&self, metadata: &log::Metadata<'_>) -> bool {
        crcbl_core::log::sink_permits(metadata.level(), metadata.target())
    }

    fn log(&self, record: &log::Record<'_>) {
        // Before the filter, and the same push the native sink makes: the
        // console's ring holds what the browser console did not print, which is
        // `docs/plan/52-debug-console.md` decision 4. `try_from_secs_f64` rather
        // than `from_secs_f64` because that one panics on a non-finite number,
        // and a logger must not be the thing that ends a frame.
        crcbl_core::log::console::push(
            record.level(),
            record.target(),
            core::time::Duration::try_from_secs_f64(elapsed_seconds())
                .unwrap_or(core::time::Duration::ZERO),
            *record.args(),
        );
        if !self.enabled(record.metadata()) {
            return;
        }
        LOG.with(|slot| {
            // `try_borrow_mut` because a `Drop` running inside `log_take` could
            // in principle log; dropping the line is better than a panic in a
            // logger.
            let Ok(mut queue) = slot.try_borrow_mut() else {
                return;
            };

            if queue.lines.len() >= MAX_LOG_LINES {
                queue.lines.pop_front();
                queue.dropped = queue.dropped.saturating_add(1);
            }
            let mut line = format!(
                "[{elapsed:9.4}s {level:<5} {target}] {args}",
                elapsed = elapsed_seconds(),
                level = record.level(),
                target = record.target(),
                args = record.args(),
            );
            // Byte truncation would split a UTF-8 sequence; `char_indices` finds
            // the last boundary at or before the limit.
            if line.len() > MAX_LOG_LINE {
                let end = line
                    .char_indices()
                    .map(|(i, _)| i)
                    .take_while(|i| *i <= MAX_LOG_LINE)
                    .last()
                    .unwrap_or(0);
                line.truncate(end);
            }
            queue.lines.push_back(line);
        });
    }

    fn flush(&self) {}
}

static LOGGER: WebLogger = WebLogger;

/// Installs the queueing logger, unless a logger is already installed.
///
/// **The registration is what makes `log` answer on this tier.**
/// `crcbl_core::log`'s filter is the engine's rather than a sink's, and
/// [`crcbl_core::log::register_sink`] is how it learns that the sink applying
/// it is this one: without it `crcbl_core::log::filter` answers `None` and
/// `set_filter` refuses, because `StderrLogger` is never installed in a browser
/// — which is exactly what the console command reported here until this line
/// existed.
///
/// **Only on the winning branch**, because a sink that lost the slot honours
/// nothing: registering there would put a filter in force for a logger that is
/// not running.
///
/// [`Filter::from_env`](crcbl_core::log::Filter::from_env) rather than a level
/// written here, so a native caller of this gets `CRCBL_LOG` like every other
/// entry point. On `wasm32` there is no environment to read and it is
/// `crcbl_core::log::DEFAULT_FILTER`, which is the `Info` this used to set.
pub fn install_logger() {
    if log::set_logger(&LOGGER).is_ok() {
        crcbl_core::log::register_sink(crcbl_core::log::Filter::from_env());
    }
}

/// Sets the log filter: `0` off, `1` error, `2` warn, `3` info, `4` debug,
/// `5` trace.
///
/// Returns `1`, or `0` for a level outside that range — which leaves the filter
/// **unchanged**. Refusing rather than clamping is the point: a shim that sent
/// nonsense would otherwise get a quiet default and a log at a level nobody
/// asked for.
#[must_use]
pub fn set_log_level(level: u32) -> u32 {
    let directive = match level {
        0 => "off",
        1 => "error",
        2 => "warn",
        3 => "info",
        4 => "debug",
        5 => "trace",
        _ => return 0,
    };
    // Through the engine's filter rather than straight at the facade's maximum,
    // because that filter is what this page's sink now consults: a shim moving
    // only the maximum would leave the directives in force and change nothing a
    // record is actually decided by. A bare level has no per-target overrides,
    // so its `max_level` *is* the level — which is what the fallback needs on a
    // page whose logger somebody else installed and this sink never registered.
    let filter = crcbl_core::log::Filter::parse(directive);
    let maximum = filter.max_level();
    if !crcbl_core::log::set_filter(filter) {
        log::set_max_level(maximum);
    }
    1
}

/// Moves the next queued line into the scratch buffer and returns its length.
///
/// `0` means there was nothing to take. The line itself is read through
/// [`log_ptr`], which stays valid until the next call to this.
#[must_use]
pub fn log_take() -> u32 {
    LOG.with(|slot| {
        let Ok(mut queue) = slot.try_borrow_mut() else {
            return 0;
        };
        queue.current.clear();
        match queue.lines.pop_front() {
            Some(line) => queue.current.push_str(&line),
            // The overflow notice is emitted only once the queue has actually
            // drained — synthesising it while lines are still queued would
            // itself be a line, and the counter is cleared as it is reported so
            // a page that keeps up never sees it twice.
            None => {
                let dropped = core::mem::take(&mut queue.dropped);
                if dropped == 0 {
                    return 0;
                }
                use core::fmt::Write as _;
                let _ = write!(
                    queue.current,
                    "[WARN] crcbl::web: {dropped} log lines dropped; the shim is not draining"
                );
            }
        }
        u32::try_from(queue.current.len()).unwrap_or(u32::MAX)
    })
}

/// Address of the log scratch buffer, or null when nothing has been taken.
#[must_use]
pub fn log_ptr() -> *const u8 {
    LOG.with(|slot| match slot.try_borrow() {
        Ok(queue) if !queue.current.is_empty() => queue.current.as_ptr(),
        _ => core::ptr::null(),
    })
}

// ---------------------------------------------------------------------------
// The demo's lifecycle
// ---------------------------------------------------------------------------

/// A game's assembled loop, as the browser lifecycle needs to see it.
///
/// Every method is one a sample's `Loop` already has; the trait exists so this
/// module can own the state machine that was written out in all four `web.rs`
/// files, which past the naming were the same file.
pub trait WebLoop: crate::engine::GameLoop {
    /// The demo's own name, for this module's log lines.
    const NAME: &'static str;

    /// The swapchain's current size, for the "running at" line.
    fn extent(&self) -> (u32, u32);

    /// Whether the simulation is stopped — see [`STATUS_PAUSED`].
    fn is_paused(&self) -> bool;

    /// How far the clock advances for this frame.
    fn set_frame_step(&mut self, dt: core::time::Duration);

    /// The frame limit the run is holding, which on this target the page is
    /// what obeys.
    ///
    /// Read every frame rather than once at boot, because a settings screen can
    /// change it mid-run — see [`crate::engine::Loop::frame_limit`].
    fn frame_limit(&self) -> crate::engine::FrameLimit;

    /// Logs the one line that is genuinely per-game.
    ///
    /// The **only** thing that differed between the four samples' copies of this
    /// whole file: which numbers a run is worth reporting. Breakout has a score,
    /// horde has a time survived and a kill count, and no shared shape covers
    /// both without inventing a summary type that neither wanted.
    fn log_summary(summary: &Self::Summary);
}

/// An engine-owned loop is a browser loop, with nothing to forward.
///
/// Both halves that are genuinely per-game — the name and the summary line —
/// are already [`HostedGame`](crate::engine::HostedGame)'s, so a game that lets
/// the engine own its frame gets the browser lifecycle for free. A game that
/// keeps its own frame implements [`WebLoop`] itself, which is what the trait
/// is for.
impl<S: crate::shell::Shell + ?Sized, G: crate::engine::HostedGame> WebLoop
    for crate::engine::Loop<S, G>
{
    const NAME: &'static str = G::NAME;

    fn extent(&self) -> (u32, u32) {
        Self::extent(self)
    }

    fn is_paused(&self) -> bool {
        Self::is_paused(self)
    }

    fn set_frame_step(&mut self, dt: core::time::Duration) {
        Self::set_frame_step(self, dt);
    }

    fn frame_limit(&self) -> crate::engine::FrameLimit {
        Self::frame_limit(self)
    }

    fn log_summary(summary: &Self::Summary) {
        G::log_summary(summary);
    }
}

/// A game's start-up, as the browser lifecycle needs to see it.
/// Writes a sample's [`WebPending`] impl, which is pure forwarding.
///
/// Six samples carried the same block, identical once the sample's name is
/// normalised away: `request` fills in the game's own `Options::default()` and
/// `poll` delegates, because the browser entry point has neither to hand.
///
/// # The recursion this had to be made safe against first
///
/// Both forwards are `Self::method(..)`, and if the type has no inherent method
/// of that name the call resolves to the **trait** method — so it calls itself
/// for ever rather than failing to compile. Written out by hand that is caught
/// by rustc's `unconditional_recursion`, which this workspace denies; but
/// **rustc suppresses its lints inside an external macro's expansion**, so
/// collapsing the block into a macro removes the only thing catching it. This is
/// the same trap [`impl_game_gpu`](crate::impl_game_gpu) documents, and it is
/// the reason this macro opens with a `const _` block coercing both inherent
/// methods to function pointers: path syntax only considers a trait's methods
/// when the trait is in scope, and `WebPending` is not imported inside the
/// expansion, so each coercion can only resolve to the inherent method. A type
/// missing one gets `E0599` naming it.
///
/// # Examples
///
/// ```ignore
/// crcbl::impl_web_pending!(PendingLoop, Loop, Options, crate::app::HudError);
/// ```
///
/// The example is `ignore` because it needs the sample's own `PendingLoop`,
/// `Loop` and `Options`. Every sample under `apps/` exercises the expansion.
#[macro_export]
macro_rules! impl_web_pending {
    ($pending:ident, $running:ident, $options:ty, $error:ty) => {
        // Load-bearing, not decoration: see the macro's docs. Without it a
        // sample that loses its inherent `request` or `poll` compiles into an
        // infinite recursion instead of an error.
        const _: () = {
            let _: fn(
                ::std::boxed::Box<dyn $crate::shell::Shell>,
                &$options,
                $crate::engine::Clock,
            ) -> ::core::result::Result<$pending<dyn $crate::shell::Shell>, $error> =
                <$pending<dyn $crate::shell::Shell>>::request;
            let _: fn(
                &mut $pending<dyn $crate::shell::Shell>,
            ) -> ::core::result::Result<
                ::core::option::Option<$running<dyn $crate::shell::Shell>>,
                $error,
            > = <$pending<dyn $crate::shell::Shell>>::poll;
        };

        impl $crate::web::WebPending for $pending<dyn $crate::shell::Shell> {
            type Loop = $running<dyn $crate::shell::Shell>;

            fn request(
                shell: ::std::boxed::Box<dyn $crate::shell::Shell>,
                clock: $crate::engine::Clock,
            ) -> ::core::result::Result<Self, $error> {
                Self::request(shell, &<$options>::default(), clock)
            }

            fn poll(
                &mut self,
            ) -> ::core::result::Result<::core::option::Option<Self::Loop>, $error> {
                Self::poll(self)
            }
        }
    };
}

pub trait WebPending: Sized {
    /// The loop this becomes.
    type Loop: WebLoop;

    /// Opens the window and starts the device request, without blocking.
    ///
    /// The game supplies its own options; the shell and the clock are the
    /// page's.
    ///
    /// # Errors
    ///
    /// The game's error if the shell refused the window.
    fn request(
        shell: Box<dyn crate::shell::Shell>,
        clock: crate::engine::Clock,
    ) -> Result<Self, <Self::Loop as crate::engine::GameLoop>::Error>;

    /// `Ok(None)` means "not yet, poll again next frame".
    ///
    /// # Errors
    ///
    /// The game's error if start-up failed.
    fn poll(
        &mut self,
    ) -> Result<Option<Self::Loop>, <Self::Loop as crate::engine::GameLoop>::Error>;
}

/// Where the demo has got to. One value, so no two of these can be true at once.
///
/// Both live variants are boxed. A `Loop` is the whole engine — swapchain ring,
/// render graph pool, ECS world — and an unboxed variant would make every
/// assignment in [`App::frame`] a multi-kilobyte move, once per frame, for no
/// reason.
enum Stage<P: WebPending> {
    Idle,
    Prepared,
    Booting(Box<P>),
    Running(Box<P::Loop>),
    Stopped,
    Failed,
}

impl<P: WebPending> Stage<P> {
    fn status(&self) -> u32 {
        match self {
            Self::Idle => STATUS_IDLE,
            Self::Prepared => STATUS_PREPARED,
            Self::Booting(_) => STATUS_BOOTING,
            Self::Running(engine) => running_status(engine.as_ref()),
            Self::Stopped => STATUS_STOPPED,
            Self::Failed => STATUS_FAILED,
        }
    }
}

/// [`STATUS_PAUSED`] or [`STATUS_RUNNING`], from the loop itself.
///
/// Asked of the loop rather than tracked here: the page has no way to know that
/// a `blur` paused the game, and a second copy of the answer would be the thing
/// that drifts.
fn running_status<L: WebLoop>(engine: &L) -> u32 {
    if engine.is_paused() {
        STATUS_PAUSED
    } else {
        STATUS_RUNNING
    }
}

/// Everything a demo owns for the life of the page.
///
/// A sample holds one of these in its own `thread_local!` — which is why this is
/// a type with methods rather than free functions over a static here: a
/// `thread_local!` cannot hold a generic.
pub struct App<P: WebPending> {
    stage: Stage<P>,
    /// `performance.now()` at the last frame, for the delta the clock is stepped
    /// by. `None` until the loop starts running.
    last_ms: Option<f64>,
    /// The deadline grid the frame limit is applied on — see [`App::frame`].
    ///
    /// Starts unlimited and takes the loop's limit every frame, because there is
    /// no loop to ask until one has booted.
    pacer: crate::engine::FramePacer,
    error: String,
}

impl<P: WebPending> core::fmt::Debug for App<P> {
    /// Hand-written because a game's `Loop` need not be `Debug`; what a reader
    /// wants from this is which stage the page is in.
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        f.debug_struct("App")
            .field("status", &self.stage.status())
            .field("last_ms", &self.last_ms)
            .field("pacer", &self.pacer)
            .field("error", &self.error)
            .finish()
    }
}

impl<P: WebPending> Default for App<P> {
    fn default() -> Self {
        Self::new()
    }
}

impl<P: WebPending> App<P> {
    /// A page that has not prepared yet.
    #[must_use]
    pub const fn new() -> Self {
        Self {
            stage: Stage::Idle,
            last_ms: None,
            pacer: crate::engine::FramePacer::new(crate::engine::FrameLimit::unlimited()),
            error: String::new(),
        }
    }

    /// Records `error`, fails the stage, and returns [`STATUS_FAILED`].
    ///
    /// A failure is terminal: nothing here retries, because every failure this
    /// can see (no WebGPU adapter, a device that refused the features the UI
    /// pass needs) is a property of the browser rather than a transient.
    pub fn fail(&mut self, error: impl core::fmt::Display) -> u32 {
        use core::fmt::Write as _;
        self.error.clear();
        let _ = write!(self.error, "{error}");
        log::error!("{}: {}", P::Loop::NAME, self.error);
        self.stage = Stage::Failed;
        STATUS_FAILED
    }

    /// Whether the page is still [`STATUS_IDLE`], for the `prepare` export.
    #[must_use]
    pub const fn is_idle(&self) -> bool {
        matches!(self.stage, Stage::Idle)
    }

    /// Moves to [`STATUS_PREPARED`]. The caller has installed its storage.
    pub fn prepared(&mut self) {
        self.stage = Stage::Prepared;
    }

    /// Opens the shell and starts the polled device request.
    ///
    /// Returns `1`, or `0` if the page had not prepared or the shell refused.
    pub fn boot(&mut self) -> u32 {
        if !matches!(self.stage, Stage::Prepared) {
            return 0;
        }
        let shell = match crate::shell::open_backend(crate::shell::ShellBackend::Web) {
            Ok(shell) => shell,
            Err(error) => return self.fail(error),
        };
        // `Clock::manual` rather than `Clock::new(false)`: `Instant::now` panics
        // on this target, so the rAF delta is the only clock there is. The step
        // is replaced every frame with what the browser reported.
        match P::request(
            shell,
            crate::engine::Clock::manual(core::time::Duration::ZERO),
        ) {
            Ok(pending) => {
                self.stage = Stage::Booting(Box::new(pending));
                log::info!(
                    "{}: booting; waiting for the canvas to report a size",
                    P::Loop::NAME
                );
                1
            }
            Err(error) => {
                self.fail(error);
                0
            }
        }
    }

    /// One `requestAnimationFrame`, and the status afterwards.
    ///
    /// `now_ms` is `performance.now()`.
    ///
    /// # A tick inside a claimed slot is skipped, not shortened
    ///
    /// Once the loop is running this is where the [`crate::engine::FrameLimit`]
    /// is applied, because the clock underneath cannot wait — see the module
    /// docs. The tick's timestamp goes to the same [`crate::engine::FramePacer`]
    /// the desktop limiter waits on; a tick the pacer is still holding a
    /// deadline for returns the current status and does nothing else. Nothing
    /// is stepped and nothing is drawn, and `last_ms` is deliberately left
    /// alone, so the tick that *does* run steps the clock by the whole gap
    /// rather than by the last slice of it — a game's motion is then the same
    /// whichever ticks it landed on.
    ///
    /// A cap at or above the display's rate runs every tick, after at most one
    /// skipped tick while the grid settles behind them. A cap below it drops
    /// the ticks that fall inside a slot. What the browser cannot offer is a
    /// slot boundary that is not a vsync tick, which is why an unfriendly ratio
    /// judders — the module docs make that argument.
    pub fn frame(&mut self, now_ms: f64) -> u32 {
        // Before anything below can log, so a line written this frame is
        // stamped with this frame's time and not the previous one's.
        note_frame_time(now_ms);
        match core::mem::replace(&mut self.stage, Stage::Failed) {
            Stage::Booting(mut pending) => match pending.poll() {
                Ok(Some(engine)) => {
                    log::info!("{}: running at {:?}", P::Loop::NAME, engine.extent());
                    let status = running_status(&engine);
                    self.stage = Stage::Running(Box::new(engine));
                    self.last_ms = Some(now_ms);
                    status
                }
                Ok(None) => {
                    self.stage = Stage::Booting(pending);
                    STATUS_BOOTING
                }
                Err(error) => self.fail(error),
            },
            Stage::Running(mut engine) => {
                // Every frame, not once at boot: a settings screen can change
                // the cap mid-run, and reading it back is a field access.
                self.pacer.set_limit(engine.frame_limit());
                let now = millis_from(now_ms);
                if self.pacer.wait(now).is_some() {
                    let status = running_status(engine.as_ref());
                    self.stage = Stage::Running(engine);
                    return status;
                }
                self.pacer.start(now);
                engine.set_frame_step(step_from(self.last_ms.replace(now_ms), now_ms));
                match engine.frame() {
                    Ok(crate::engine::Flow::Continue) => {
                        let status = running_status(engine.as_ref());
                        self.stage = Stage::Running(engine);
                        status
                    }
                    Ok(crate::engine::Flow::Stop(reason)) => self.teardown(*engine, reason),
                    Err(error) => {
                        // The frame error is the one worth reporting; a teardown
                        // failure on top of it is logged, exactly as the native
                        // `run` does.
                        if let Err(teardown) = engine.finish(crate::engine::ExitReason::Failed) {
                            log::error!("teardown after a failed frame also failed: {teardown}");
                        }
                        self.fail(error)
                    }
                }
            }
            other => {
                let status = other.status();
                self.stage = other;
                status
            }
        }
    }

    /// Tears the loop down and records how it ended.
    fn teardown(&mut self, engine: P::Loop, reason: crate::engine::ExitReason) -> u32 {
        match engine.finish(reason) {
            Ok(summary) => {
                P::Loop::log_summary(&summary);
                self.stage = Stage::Stopped;
                STATUS_STOPPED
            }
            Err(error) => self.fail(error),
        }
    }

    /// The status, without advancing anything.
    #[must_use]
    pub fn status(&self) -> u32 {
        self.stage.status()
    }

    /// Releases the swapchain, the device and the window.
    ///
    /// Returns `1` if there was something to tear down.
    pub fn shutdown(&mut self) -> u32 {
        match core::mem::replace(&mut self.stage, Stage::Stopped) {
            Stage::Running(engine) => {
                self.teardown(*engine, crate::engine::ExitReason::CloseRequested);
                1
            }
            Stage::Booting(_) => 1,
            other => {
                self.stage = other;
                0
            }
        }
    }

    /// Address of the last error message (UTF-8, not NUL-terminated), or null.
    ///
    /// Valid until the next export call.
    #[must_use]
    pub fn error_ptr(&self) -> *const u8 {
        if self.error.is_empty() {
            core::ptr::null()
        } else {
            self.error.as_ptr()
        }
    }

    /// The length of that message in bytes.
    #[must_use]
    pub fn error_len(&self) -> u32 {
        u32::try_from(self.error.len()).unwrap_or(u32::MAX)
    }
}

/// Milliseconds from the page's clock as a [`core::time::Duration`], with the
/// guard the conversion needs.
///
/// `Duration::from_secs_f64` **panics** on a negative or non-finite value and a
/// panic here takes the whole wasm instance with it, so anything that is not a
/// time is zero. `performance.now()` is monotonic and non-negative, so this is
/// a guard against a shim bug rather than against physics — but it is the same
/// guard for the timestamp [`App::frame`] paces on and for the delta
/// [`step_from`] derives, and one of the two having it would be the one that
/// drifts.
fn millis_from(ms: f64) -> core::time::Duration {
    if ms.is_finite() && ms > 0.0 {
        core::time::Duration::from_secs_f64(ms / 1000.0)
    } else {
        core::time::Duration::ZERO
    }
}

/// How far the clock should step for a frame that arrived at `now_ms`.
///
/// Zero for a first frame with no predecessor, and for a `previous` that is not
/// behind `now_ms` — see [`millis_from`] for the guard. The upper bound is
/// [`crate::engine::MAX_FRAME_STEP`]'s job, applied by `set_frame_step`,
/// because a native manual clock needs it too.
fn step_from(previous: Option<f64>, now_ms: f64) -> core::time::Duration {
    let Some(previous) = previous else {
        return core::time::Duration::ZERO;
    };
    millis_from(now_ms - previous)
}

// ---------------------------------------------------------------------------
// The exports
// ---------------------------------------------------------------------------

/// Writes a sample's browser entry point: the page's state, and the ten
/// `#[unsafe(no_mangle)] extern "C"` symbols the JS shim calls.
///
/// Invoke it at the top level of a sample's `web.rs`, beside that sample's
/// [`WebPending`] impl.
///
/// # Why the symbol names are arguments
///
/// Two demos can be open in one browser, so the exports carry the demo's name
/// or they would collide — and `concat_idents!` is not stable, so the macro
/// cannot build `__crcbl_asteroids_prepare` out of `asteroids`. Each name is
/// therefore spelled out at the call site, which is what
/// `web/tools/check-exports.mjs` wants anyway: it learns the contract by
/// scanning the JS shim for literal `__crcbl_…` names, and a symbol assembled
/// from pieces would be invisible to the same search on the Rust side.
///
/// The keys are named rather than positional because the failure mode is
/// silent. A symbol emitted under the wrong name still compiles and still
/// links; the shim's lookup returns `undefined` at run time, and the page shows
/// a blank canvas.
///
/// # What it expands to
///
/// * `APP`, this page's [`App`], and `STORAGE`, the two browser storage handles
///   held for the life of the page. Both crates' `install` keeps only a
///   [`std::rc::Weak`], so dropping the `Rc`s would silently turn every
///   `__crcbl_web_opfs_*` and `__crcbl_web_fetch_*` call into a `0`.
/// * `STORAGE` is left reachable from the invoking module, but **a game that
///   only wants to read its own save file back no longer needs it**: the `Rc`
///   it holds is what keeps the installed store alive, and
///   [`Backing::platform`](crate::store::record::Backing::platform) reaches
///   that store through `crcbl_store::web::opfs::installed` rather than through
///   this cell. The samples used to spell out a two-line `opfs_store` accessor
///   here for that and no longer do.
/// * The ten exports, each forwarding to [`App`], [`set_log_level`],
///   [`log_take`] or [`log_ptr`].
///
/// What it does **not** expand to is the [`WebPending`] impl: the options a game
/// boots with, and the error type it fails with, are the game's own.
///
/// # Examples
///
/// ```ignore
/// crcbl::web_exports! {
///     pending: PendingLoop<dyn crcbl::shell::Shell>,
///     prepare: __crcbl_asteroids_prepare,
///     log_level: __crcbl_asteroids_log_level,
///     boot: __crcbl_asteroids_boot,
///     frame: __crcbl_asteroids_frame,
///     status: __crcbl_asteroids_status,
///     shutdown: __crcbl_asteroids_shutdown,
///     error_ptr: __crcbl_asteroids_error_ptr,
///     error_len: __crcbl_asteroids_error_len,
///     log_take: __crcbl_asteroids_log_take,
///     log_ptr: __crcbl_asteroids_log_ptr,
/// }
/// ```
///
/// The example is `ignore` because two expansions in one binary would define
/// the same `APP`; the expansion is exercised instead by this module's own
/// tests, which invoke the macro over a fixture loop and drive the result.
#[macro_export]
macro_rules! web_exports {
    (
        pending: $pending:ty,
        prepare: $prepare:ident,
        log_level: $log_level:ident,
        boot: $boot:ident,
        frame: $frame:ident,
        status: $status:ident,
        shutdown: $shutdown:ident,
        error_ptr: $error_ptr:ident,
        error_len: $error_len:ident,
        log_take: $log_take:ident,
        log_ptr: $log_ptr:ident $(,)?
    ) => {
        ::std::thread_local! {
            /// Everything this demo owns for the life of the page.
            static APP: ::std::cell::RefCell<$crate::web::App<$pending>> =
                const { ::std::cell::RefCell::new($crate::web::App::new()) };

            /// The storage handles, held for the life of the page.
            ///
            /// Both crates' `install` keeps only a [`std::rc::Weak`]: dropping
            /// the `Rc` would silently turn every `__crcbl_web_opfs_*` and
            /// `__crcbl_web_fetch_*` call into a `0`, and the shim's boot
            /// sequence drives both ABIs before the demo starts.
            static STORAGE: ::std::cell::RefCell<Option<(
                ::std::rc::Rc<$crate::store::web::OpfsStorage>,
                ::std::rc::Rc<$crate::store::web::FetchSource>,
            )>> = const { ::std::cell::RefCell::new(None) };
        }

        /// Runs `f` against the page's state.
        ///
        /// `absent` is returned when the cell is already borrowed, which can
        /// only happen if an export were called re-entrantly from another
        /// export — the shim never does, and answering rather than panicking
        /// keeps a shim bug from aborting the wasm instance.
        fn with_app<R>(absent: R, f: impl FnOnce(&mut $crate::web::App<$pending>) -> R) -> R {
            APP.with(|slot| match slot.try_borrow_mut() {
                Ok(mut app) => f(&mut app),
                Err(_) => absent,
            })
        }

        /// Install the log sink and the browser storage backends.
        ///
        /// The first call the shim makes, and the one that has to happen before
        /// any `__crcbl_web_fetch_*` or `__crcbl_web_opfs_*` call — both of
        /// those answer `0` until something is installed, which is the
        /// documented "a shim that started before the engine did" case rather
        /// than a failure.
        ///
        /// Returns `1`, or `0` if it had already run.
        #[unsafe(no_mangle)]
        pub extern "C" fn $prepare() -> u32 {
            $crate::web::install_logger();
            with_app(0, |app| {
                if !app.is_idle() {
                    return 0;
                }

                let saves = ::std::rc::Rc::new($crate::store::web::OpfsStorage::new());
                if !$crate::store::web::opfs::install(&saves) {
                    app.fail("an OPFS store was already installed");
                    return 0;
                }

                let assets = match $crate::store::web::FetchSource::new($crate::web::ASSET_BASE) {
                    Ok(source) => ::std::rc::Rc::new(source),
                    Err(error) => {
                        app.fail(error);
                        return 0;
                    }
                };
                if !$crate::store::web::fetch::install(&assets) {
                    app.fail("a fetch source was already installed");
                    return 0;
                }
                STORAGE.with(|slot| *slot.borrow_mut() = Some((saves, assets)));

                // The name is `HostedGame::NAME`, reached through the loop this
                // page runs, rather than a string the call site repeats.
                $crate::log::info!(
                    "{}: prepared; assets from {}",
                    <<$pending as $crate::web::WebPending>::Loop as $crate::web::WebLoop>::NAME,
                    $crate::web::ASSET_BASE,
                );
                app.prepared();
                1
            })
        }

        /// Set the log filter: `0` off, `1` error, `2` warn, `3` info,
        /// `4` debug, `5` trace.
        ///
        /// Returns `1`, or `0` for a level outside that range.
        #[unsafe(no_mangle)]
        pub extern "C" fn $log_level(level: u32) -> u32 {
            $crate::web::set_log_level(level)
        }

        /// Open the shell and the window, and start the polled device request.
        ///
        /// The canvas is the one `__crcbl_web_canvas` announced, so that call
        /// must come first; there is deliberately no argument here, because a
        /// canvas id that entered wasm through two doors could disagree with
        /// itself and the shell's event routing would silently drop everything.
        ///
        /// Returns `1`, or `0` if the page had not prepared or the shell
        /// refused.
        #[unsafe(no_mangle)]
        pub extern "C" fn $boot() -> u32 {
            with_app(0, $crate::web::App::boot)
        }

        /// One `requestAnimationFrame`.
        ///
        /// `now_ms` is `performance.now()`. Call `__crcbl_web_frame(now_ms)`
        /// first, so the shell's event-clock reference is this frame's and not
        /// the previous one's.
        ///
        /// Returns the status afterwards; the shim keeps scheduling frames
        /// while it is `STATUS_BOOTING`, `STATUS_RUNNING` or `STATUS_PAUSED`.
        #[unsafe(no_mangle)]
        pub extern "C" fn $frame(now_ms: f64) -> u32 {
            with_app($crate::web::STATUS_FAILED, |app| app.frame(now_ms))
        }

        /// The status, without advancing anything.
        #[unsafe(no_mangle)]
        pub extern "C" fn $status() -> u32 {
            with_app($crate::web::STATUS_FAILED, |app| app.status())
        }

        /// Tear the loop down: release the swapchain, the device and the
        /// window.
        ///
        /// Returns `1` if there was something to tear down. Safe to call from
        /// `beforeunload`; a page's OPFS drain should happen *before* it,
        /// because a game's last write is queued during its last frame.
        #[unsafe(no_mangle)]
        pub extern "C" fn $shutdown() -> u32 {
            with_app(0, $crate::web::App::shutdown)
        }

        /// Address of the last error message (UTF-8, not NUL-terminated), or
        /// `0`.
        ///
        /// Valid until the next export call. Read the length first and decode
        /// immediately.
        #[unsafe(no_mangle)]
        pub extern "C" fn $error_ptr() -> *const u8 {
            with_app(::std::ptr::null(), |app| app.error_ptr())
        }

        /// The length of that message in bytes.
        #[unsafe(no_mangle)]
        pub extern "C" fn $error_len() -> u32 {
            with_app(0, |app| app.error_len())
        }

        /// Pop one log line into the scratch buffer and return its length in
        /// bytes.
        ///
        /// `0` means the queue is empty. Read the pointer **after** this, not
        /// before: the two together are one read, and the buffer's contents
        /// belong to the most recent take.
        #[unsafe(no_mangle)]
        pub extern "C" fn $log_take() -> u32 {
            $crate::web::log_take()
        }

        /// Address of the log scratch buffer, or `0` when nothing has been
        /// taken.
        #[unsafe(no_mangle)]
        pub extern "C" fn $log_ptr() -> *const u8 {
            $crate::web::log_ptr()
        }
    };
}

#[doc(inline)]
pub use crate::web_exports;

#[cfg(test)]
mod tests {
    use std::rc::Rc;
    use std::sync::{Mutex, MutexGuard, PoisonError};

    use log::Log as _;

    use super::*;

    /// Drains everything the queue is holding, so one test cannot see another's
    /// lines — `LOG` is a thread-local and the test harness reuses threads.
    fn drain() -> Vec<String> {
        let mut taken = Vec::new();
        while log_take() > 0 {
            LOG.with(|slot| taken.push(slot.borrow().current.clone()));
        }
        taken
    }

    fn write_line(target: &str, message: &str) {
        write_at(log::Level::Info, target, message);
    }

    fn write_at(level: log::Level, target: &str, message: &str) {
        LOGGER.log(
            &log::Record::builder()
                .level(level)
                .target(target)
                .args(format_args!("{message}"))
                .build(),
        );
    }

    /// Puts `directives` in force as the filter this sink reads, registering it
    /// as the sink that reads them.
    ///
    /// Replaces the bare `log::set_max_level(Info)` these tests used to open
    /// with. [`WebLogger::enabled`] asks [`crcbl_core::log::sink_permits`] now,
    /// so pinning the facade's maximum no longer says what the sink will accept.
    /// It still moves that maximum — `register_sink` does — so the reason the old
    /// line was here holds unchanged: a `capture` running in another of this
    /// binary's tests raises it to `Trace`, and a fixture that read it would be
    /// logging a line the filter admits.
    ///
    /// **`init_logging` first, and it is not decoration.** The filter these
    /// write is the process's, and `crcbl_core::log::try_init_logging` stores
    /// the caller's over it — once, on whichever thread wins the logger slot. A
    /// `capture` in another of this binary's tests is what calls it, so a
    /// fixture that did not take the slot first could have its directives
    /// replaced by `CRCBL_LOG`'s between writing them and logging the record
    /// they are about. Calling it here means the slot is gone before any
    /// directive is written, and a later `try_init_logging` returns `Err`
    /// without touching the filter.
    ///
    /// **The guard is the caller's to hold for the rest of its test**, because
    /// the filter is one value and these tests each assert on the one they just
    /// wrote: without it they overwrite each other's directives, which was a
    /// failure of this file about one `cargo test -p crcbl --lib` run in ten.
    /// (Under `cargo nextest`, which CI runs, each test is its own process and
    /// neither hazard can arise.)
    fn filter_at(directives: &str) -> MutexGuard<'static, ()> {
        let order = FILTER_ORDER.lock().unwrap_or_else(PoisonError::into_inner);
        let _ = crcbl_core::log::init_logging();
        crcbl_core::log::register_sink(crcbl_core::log::Filter::parse(directives));
        order
    }

    /// Serialises the tests that write the process-wide log filter. See
    /// [`filter_at`].
    static FILTER_ORDER: Mutex<()> = Mutex::new(());

    /// **A page that stops draining bounds the queue and says how much it lost.**
    ///
    /// Both halves matter and each fails silently on its own: an unbounded queue
    /// grows wasm memory until the tab dies, and a bounded one that drops
    /// quietly turns "the log is missing the interesting part" into a mystery.
    #[test]
    fn a_queue_nobody_drains_is_bounded_and_reports_what_it_dropped() {
        let _order = filter_at("info");
        drain();

        let overflow = 10;
        for i in 0..MAX_LOG_LINES + overflow {
            write_line("test", &format!("line {i}"));
        }

        let taken = drain();
        assert_eq!(
            taken.len(),
            MAX_LOG_LINES + 1,
            "the cap did not hold, or the overflow notice is missing",
        );
        // The oldest went, not the newest: a log that dropped the most recent
        // lines would lose whatever just went wrong.
        assert!(
            taken[0].contains(&format!("line {overflow}")),
            "{}",
            taken[0]
        );
        let notice = taken.last().expect("the queue was not empty");
        assert!(
            notice.contains(&overflow.to_string()) && notice.contains("not draining"),
            "the drop count must be reported, not swallowed: {notice}",
        );

        // Reported once. A counter that never cleared would append the notice to
        // every later drain of a page that had long since caught up.
        write_line("test", "after");
        let taken = drain();
        assert_eq!(taken.len(), 1, "the notice was repeated: {taken:?}");
    }

    /// **A line longer than the cap is cut on a character boundary.**
    ///
    /// `String::truncate` panics on a byte index inside a UTF-8 sequence, and a
    /// logger that panics takes the frame with it. Asserted with a multi-byte
    /// character straddling the limit, which is the only case that can fail.
    #[test]
    fn an_over_long_line_is_cut_without_splitting_a_character() {
        let _order = filter_at("info");
        drain();

        // 'é' is two bytes, and the prefix is chosen so that MAX_LOG_LINE lands
        // *between* the two — the only arrangement a byte truncate panics on.
        // With an even-length prefix the limit falls on a boundary and a wrong
        // implementation passes, which is what the first version of this test
        // did.
        let target = "tt";
        let prefix = format!("[{}] {target}: ", log::Level::Info).len();
        assert!(
            (MAX_LOG_LINE - prefix) % 2 == 1,
            "the fixture must put the limit inside a character, not on a boundary",
        );
        let message = "é".repeat(MAX_LOG_LINE);
        write_line(target, &message);

        let taken = drain();
        let line = taken.first().expect("the line was queued");
        assert!(line.len() <= MAX_LOG_LINE, "not truncated: {}", line.len());
        assert!(
            line.chars().last().is_some_and(|c| c == 'é'),
            "the cut landed inside a character",
        );
    }

    /// **The browser sink feeds the console ring, and does it before its own
    /// filter.**
    ///
    /// Both halves, and the level is what separates them: a debug line under an
    /// `Info` maximum is one the browser console never prints, so finding it in
    /// the ring is the whole claim. A push moved below the `enabled` check
    /// leaves the ring empty here and every other test in this file green.
    ///
    /// The maximum is set to `Info` rather than read, exactly as the two tests
    /// above set it, because a capture running in another of this binary's
    /// tests raises it to `Trace` and the fixture would then be logging a line
    /// the filter admits.
    #[test]
    fn the_web_sink_rings_a_line_its_filter_refused() {
        let _order = filter_at("info");
        drain();

        let target = "crcbl::web::tests::the_web_sink_rings_a_line_its_filter_refused";
        let message = "queued nowhere, ringed anyway";
        LOGGER.log(
            &log::Record::builder()
                .level(log::Level::Debug)
                .target(target)
                .args(format_args!("{message}"))
                .build(),
        );

        assert!(
            drain().is_empty(),
            "a debug line is under the filter and must not reach the shim queue",
        );
        let ringed: Vec<_> = crcbl_core::log::console::snapshot()
            .into_iter()
            .filter(|record| record.target == target)
            .collect();
        assert_eq!(ringed.len(), 1, "the ring did not get it: {ringed:?}");
        assert_eq!(ringed[0].message, message);
        assert_eq!(ringed[0].level, log::Level::Debug);
    }

    /// **A filter set at runtime decides what this sink queues, target by
    /// target.**
    ///
    /// `docs/backlog.md`'s "`log` answers with a fault in a browser", at the
    /// sink end of it: [`WebLogger::enabled`] was `level <= log::max_level()`,
    /// one global number, so no per-target directive could reach this logger and
    /// the console command that installs them had nothing here to move.
    ///
    /// **The pair of targets is what makes it a check.** Both records are
    /// `Debug` and one filter decides them: a sink back on the facade's maximum
    /// queues both, because a filter carrying a `debug` directive raises that
    /// maximum to `Debug`; a sink that took the directives and not the maximum
    /// queues neither.
    ///
    /// Native rather than in a browser, for
    /// [`the_web_sink_rings_a_line_its_filter_refused`]'s reason — this drives
    /// the real [`WebLogger::log`]. That `install_logger` performs the
    /// registration on a real page is the browser gate's `log` line in
    /// `web/tools/browser-e2e.mjs`.
    #[test]
    fn a_filter_set_at_runtime_decides_what_the_browser_sink_queues() {
        const LOUD: &str = "crcbl::web::tests::queued";
        const QUIET: &str = "crcbl::web::tests::unqueued";

        let _order = filter_at(&format!("info,{LOUD}=debug"));
        drain();

        write_at(log::Level::Debug, LOUD, "the directive admits this one");
        write_at(log::Level::Debug, QUIET, "and refuses this one");

        let taken = drain();
        assert_eq!(
            taken.len(),
            1,
            "one directive named one target, and the queue took {}: {taken:?}",
            taken.len(),
        );
        assert!(
            taken[0].contains(LOUD) && taken[0].contains("admits this one"),
            "the queued line is not the one the directive named: {}",
            taken[0],
        );
    }

    // ---- the lifecycle ------------------------------------------------------

    /// A loop that runs for a set number of frames and then stops.
    struct FakeLoop {
        frames_left: u32,
        paused: bool,
        fail_frame: bool,
        limit: crate::engine::FrameLimit,
        /// Every `dt` [`App::frame`] handed over, in order.
        ///
        /// The observable the pacing tests turn on: a skipped tick never
        /// reaches `set_frame_step`, so the length of this is how many frames
        /// ran and the values are what the clock was moved by. Shared with the
        /// fixture that built the loop, because [`App`] hands out no way to
        /// reach the loop it owns.
        steps: Rc<RefCell<Vec<core::time::Duration>>>,
    }

    #[derive(Debug)]
    struct FakeError;

    impl core::fmt::Display for FakeError {
        fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
            write!(f, "the fixture refused")
        }
    }

    impl crate::engine::GameLoop for FakeLoop {
        type Error = FakeError;
        type Summary = u32;

        fn frame(&mut self) -> Result<crate::engine::Flow, Self::Error> {
            if self.fail_frame {
                return Err(FakeError);
            }
            if self.frames_left == 0 {
                return Ok(crate::engine::Flow::Stop(
                    crate::engine::ExitReason::FrameBudget,
                ));
            }
            self.frames_left -= 1;
            Ok(crate::engine::Flow::Continue)
        }

        fn finish(self, _exit: crate::engine::ExitReason) -> Result<Self::Summary, Self::Error> {
            Ok(self.frames_left)
        }
    }

    impl WebLoop for FakeLoop {
        const NAME: &'static str = "fake";

        fn extent(&self) -> (u32, u32) {
            (320, 240)
        }

        fn is_paused(&self) -> bool {
            self.paused
        }

        fn set_frame_step(&mut self, dt: core::time::Duration) {
            self.steps.borrow_mut().push(dt);
        }

        fn frame_limit(&self) -> crate::engine::FrameLimit {
            self.limit
        }

        fn log_summary(summary: &Self::Summary) {
            log::info!("fake: {summary} frames left");
        }
    }

    struct FakePending {
        polls_left: u32,
        frames: u32,
        paused: bool,
        fail_frame: bool,
        limit: crate::engine::FrameLimit,
        steps: Rc<RefCell<Vec<core::time::Duration>>>,
    }

    /// A loop that boots on the first frame, runs plenty, and is not capped.
    ///
    /// Every field a test does not name is one it is not about — which is what
    /// keeps a pacing fixture from reading like a pause fixture.
    impl Default for FakePending {
        fn default() -> Self {
            Self {
                polls_left: 0,
                frames: 10,
                paused: false,
                fail_frame: false,
                limit: crate::engine::FrameLimit::unlimited(),
                steps: Rc::default(),
            }
        }
    }

    impl WebPending for FakePending {
        type Loop = FakeLoop;

        fn request(
            _shell: Box<dyn crate::shell::Shell>,
            _clock: crate::engine::Clock,
        ) -> Result<Self, FakeError> {
            unreachable!("boot() opens a Web shell, which only exists on wasm32")
        }

        fn poll(&mut self) -> Result<Option<Self::Loop>, FakeError> {
            if self.polls_left > 0 {
                self.polls_left -= 1;
                return Ok(None);
            }
            Ok(Some(FakeLoop {
                frames_left: self.frames,
                paused: self.paused,
                fail_frame: self.fail_frame,
                limit: self.limit,
                steps: Rc::clone(&self.steps),
            }))
        }
    }

    fn booting(pending: FakePending) -> App<FakePending> {
        let mut app = App::new();
        app.stage = Stage::Booting(Box::new(pending));
        app
    }

    /// **The status the page polls tracks the stage, including pause.**
    ///
    /// The shim drives everything off this number, and the paused case is the
    /// one a second copy of the answer would get wrong — which is why
    /// [`running_status`] asks the loop instead of tracking a flag.
    #[test]
    fn the_status_reports_booting_then_running_then_paused() {
        let mut app = booting(FakePending {
            polls_left: 2,
            ..FakePending::default()
        });
        assert_eq!(app.status(), STATUS_BOOTING);
        assert_eq!(app.frame(0.0), STATUS_BOOTING);
        assert_eq!(app.frame(16.0), STATUS_BOOTING);
        assert_eq!(app.frame(32.0), STATUS_RUNNING, "the device had arrived");
        assert_eq!(app.status(), STATUS_RUNNING);

        let mut app = booting(FakePending {
            paused: true,
            ..FakePending::default()
        });
        assert_eq!(app.frame(0.0), STATUS_PAUSED);
        assert_eq!(app.status(), STATUS_PAUSED, "a paused demo is not RUNNING");
    }

    /// **A loop that stops on its own terms is torn down, not failed.**
    ///
    /// `Flow::Stop` is the window closing or the frame budget running out. A
    /// page that reported those as `STATUS_FAILED` would show an error for a
    /// demo that ended correctly.
    #[test]
    fn a_loop_that_stops_reaches_stopped_and_stays_there() {
        let mut app = booting(FakePending {
            frames: 1,
            ..FakePending::default()
        });
        assert_eq!(app.frame(0.0), STATUS_RUNNING);
        assert_eq!(app.frame(16.0), STATUS_RUNNING, "one frame of budget");
        assert_eq!(app.frame(32.0), STATUS_STOPPED);
        assert_eq!(app.frame(48.0), STATUS_STOPPED, "a stopped demo stays put");
        assert_eq!(app.error_len(), 0, "stopping is not an error");
    }

    /// **A failed frame is terminal, and says why.**
    ///
    /// Nothing retries: every failure this can see is a property of the browser
    /// rather than a transient, so a page that kept scheduling frames would spin
    /// against a device that has already refused.
    #[test]
    fn a_failed_frame_is_terminal_and_reports_its_reason() {
        let mut app = booting(FakePending {
            fail_frame: true,
            ..FakePending::default()
        });
        assert_eq!(app.frame(0.0), STATUS_RUNNING);
        assert_eq!(app.frame(16.0), STATUS_FAILED);
        assert!(app.error_len() > 0, "a failure with no message");
        assert!(!app.error_ptr().is_null());
        assert_eq!(app.frame(32.0), STATUS_FAILED, "a failure is not retried");
    }

    /// **Shutting down a running demo tears it down; shutting down twice does
    /// not.**
    ///
    /// The page calls this from `beforeunload`, which can fire after the loop
    /// already stopped.
    #[test]
    fn shutdown_tears_down_once() {
        let mut app = booting(FakePending::default());
        assert_eq!(app.frame(0.0), STATUS_RUNNING);
        assert_eq!(app.shutdown(), 1);
        assert_eq!(app.status(), STATUS_STOPPED);
        assert_eq!(app.shutdown(), 0, "there was nothing left to tear down");
    }

    /// **A cap under the display's rate skips ticks, and the frame that runs
    /// is stepped by the whole gap.**
    ///
    /// The three claims are one behaviour and each fails silently on its own: a
    /// page that ran every tick would ignore the limit, a page that stepped the
    /// clock on a skipped tick would lose that tick's time out of the
    /// simulation, and a page that stepped the running frame by one tick rather
    /// than by the gap would run the game at the fraction of real time the cap
    /// is of the refresh rate.
    ///
    /// A hundred ticks a second against a fifty a second cap, because both are
    /// exact in binary floating point *and* in whole nanoseconds — so what is
    /// asserted here is the pacer and not the rounding on the way in. The real
    /// browser's numbers are neither, which is
    /// `crate::engine::FramePacer`'s own tests' business.
    #[test]
    fn a_capped_page_draws_on_the_ticks_its_grid_claims() {
        /// A 100 Hz display.
        const TICK_MS: f64 = 10.0;
        /// One frame of the cap, as `set_frame_step` should see it.
        const SLOT: core::time::Duration = core::time::Duration::from_millis(20);

        let steps = Rc::new(RefCell::new(Vec::new()));
        let mut app = booting(FakePending {
            frames: 100,
            limit: crate::engine::FrameLimit::fps(50),
            steps: Rc::clone(&steps),
            ..FakePending::default()
        });

        // Tick zero is the boot poll: it starts the loop and draws nothing.
        assert_eq!(app.frame(0.0), STATUS_RUNNING);
        for tick in 1..=11 {
            assert_eq!(
                app.frame(f64::from(tick) * TICK_MS),
                STATUS_RUNNING,
                "a skipped tick reports the status it had, not a new one",
            );
        }

        let steps = steps.borrow();
        assert_eq!(
            steps.len(),
            6,
            "eleven ticks at twice the cap must draw on six of them: {steps:?}",
        );
        assert_eq!(
            steps[0],
            core::time::Duration::from_millis(10),
            "the first frame after boot is one tick after it",
        );
        assert!(
            steps[1..].iter().all(|step| *step == SLOT),
            "a frame after a skipped tick must be stepped by the whole gap, or \
             the simulation runs slow: {steps:?}",
        );
    }

    // ---- the frame step -----------------------------------------------------

    /// **A nonsense `performance.now()` produces no step rather than a panic.**
    ///
    /// `Duration::from_secs_f64` panics on a negative or non-finite value, and a
    /// panic here takes the whole wasm instance with it. The shim controls this
    /// number, so the guard is against a caller bug rather than against physics.
    #[test]
    fn a_frame_step_from_nonsense_is_zero_rather_than_a_panic() {
        assert_eq!(step_from(None, 16.0), core::time::Duration::ZERO);
        assert_eq!(step_from(Some(32.0), 16.0), core::time::Duration::ZERO);
        assert_eq!(step_from(Some(16.0), 16.0), core::time::Duration::ZERO);
        assert_eq!(step_from(Some(f64::NAN), 16.0), core::time::Duration::ZERO);
        assert_eq!(
            step_from(Some(0.0), f64::INFINITY),
            core::time::Duration::ZERO
        );
        // …and a sane delta is carried through in seconds.
        assert_eq!(
            step_from(Some(1_000.0), 1_016.0),
            core::time::Duration::from_secs_f64(0.016),
        );
    }

    // ---- the generated exports ----------------------------------------------

    web_exports! {
        pending: FakePending,
        prepare: __crcbl_fixture_prepare,
        log_level: __crcbl_fixture_log_level,
        boot: __crcbl_fixture_boot,
        frame: __crcbl_fixture_frame,
        status: __crcbl_fixture_status,
        shutdown: __crcbl_fixture_shutdown,
        error_ptr: __crcbl_fixture_error_ptr,
        error_len: __crcbl_fixture_error_len,
        log_take: __crcbl_fixture_log_take,
        log_ptr: __crcbl_fixture_log_ptr,
    }

    /// **Every symbol the macro writes is wired to the thing it names.**
    ///
    /// This is the failure the macro exists to make impossible and is also the
    /// one it could introduce: an export that forwards to the wrong call, or
    /// that the expansion named something else, still compiles and still links.
    /// The shim's lookup is by name at run time, so the symptom is a blank
    /// canvas rather than a build error — which is why this drives the generated
    /// functions and not [`App`].
    ///
    /// `boot` is the one export not driven here: it opens a `Web` shell, which
    /// only exists on `wasm32`. The browser gate covers it, and so does
    /// `web/tools/smoke.mjs`, which drives the real artifact's ten symbols for
    /// every demo `web/build.sh` builds.
    #[test]
    fn the_generated_exports_drive_the_page() {
        drain();

        assert_eq!(__crcbl_fixture_status(), STATUS_IDLE);
        assert_eq!(__crcbl_fixture_error_len(), 0, "nothing has failed yet");
        assert!(__crcbl_fixture_error_ptr().is_null());

        assert_eq!(__crcbl_fixture_prepare(), 1);
        assert_eq!(__crcbl_fixture_status(), STATUS_PREPARED);
        assert_eq!(__crcbl_fixture_prepare(), 0, "preparing twice");
        assert_eq!(
            __crcbl_fixture_frame(0.0),
            STATUS_PREPARED,
            "a page that never booted must not advance",
        );
        assert_eq!(__crcbl_fixture_shutdown(), 0, "there was no loop");

        assert_eq!(__crcbl_fixture_log_level(3), 1);
        assert_eq!(__crcbl_fixture_log_level(99), 0, "out of range, so refused");

        // The line is pushed onto the queue directly rather than logged, and
        // the queue is emptied first: whether `prepare`'s own line reached it
        // depends on which test won the process-wide `log::set_logger` race,
        // and the claim here is only that the two generated log exports are
        // this module's queue. `web/tools/smoke.mjs` is what asserts that
        // `prepare` logs, against the real artifact, for every demo.
        drain();
        let line = "a line the shim has not taken yet";
        LOG.with(|slot| slot.borrow_mut().lines.push_back(line.to_owned()));
        assert_eq!(__crcbl_fixture_log_take(), line.len() as u32);
        let buffer = LOG.with(|slot| slot.borrow().current.as_ptr());
        assert_eq!(__crcbl_fixture_log_ptr(), buffer);
        assert_eq!(__crcbl_fixture_log_take(), 0, "the queue had one line");
    }
}
