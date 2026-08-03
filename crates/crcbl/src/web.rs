//! The half of a browser entry point that is not about any one game.
//!
//! A sample's `web.rs` is what the JS shim in `web/` calls: a dozen
//! `#[unsafe(no_mangle)] extern "C"` exports named after the demo, a status
//! code the page polls, and a log queue the page drains once a frame. The
//! exports have to stay per-game — the shim looks them up by name — and so does
//! the state machine that holds a game's own `Loop`. Everything else was
//! identical in all four samples, and it is here.
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
//!
//! Deliberately **not** here: anything that touches a game's `Loop`. See each
//! sample's `web.rs` for the state machine, which is the part that genuinely
//! differs.

use core::cell::RefCell;

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

/// A [`log::Log`] that queues lines for the shim instead of writing them.
///
/// There is no `console.log` import and no timestamp: an import would be the
/// only one in the module, and a timestamp would need [`std::time::Instant`],
/// which this target does not have. The browser's console stamps the line when
/// the shim prints it, which is within a frame of when it was written.
struct WebLogger;

impl log::Log for WebLogger {
    fn enabled(&self, metadata: &log::Metadata<'_>) -> bool {
        metadata.level() <= log::max_level()
    }

    fn log(&self, record: &log::Record<'_>) {
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
                "[{}] {}: {}",
                record.level(),
                record.target(),
                record.args()
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
pub fn install_logger() {
    if log::set_logger(&LOGGER).is_ok() {
        log::set_max_level(log::LevelFilter::Info);
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
    let filter = match level {
        0 => log::LevelFilter::Off,
        1 => log::LevelFilter::Error,
        2 => log::LevelFilter::Warn,
        3 => log::LevelFilter::Info,
        4 => log::LevelFilter::Debug,
        5 => log::LevelFilter::Trace,
        _ => return 0,
    };
    log::set_max_level(filter);
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

    fn log_summary(summary: &Self::Summary) {
        G::log_summary(summary);
    }
}

/// A game's start-up, as the browser lifecycle needs to see it.
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
    error: String,
}

impl<P: WebPending> core::fmt::Debug for App<P> {
    /// Hand-written because a game's `Loop` need not be `Debug`; what a reader
    /// wants from this is which stage the page is in.
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        f.debug_struct("App")
            .field("status", &self.stage.status())
            .field("last_ms", &self.last_ms)
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
    pub fn frame(&mut self, now_ms: f64) -> u32 {
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

/// How far the clock should step for a frame that arrived at `now_ms`.
///
/// `performance.now()` is monotonic, but a shim that passed the wrong number —
/// or a first frame with no predecessor — must not produce a negative or
/// non-finite `Duration`, which would panic in `from_secs_f64`. The upper bound
/// is [`crate::engine::MAX_FRAME_STEP`]'s job, applied by `set_frame_step`,
/// because a native manual clock needs it too.
fn step_from(previous: Option<f64>, now_ms: f64) -> core::time::Duration {
    let Some(previous) = previous else {
        return core::time::Duration::ZERO;
    };
    let delta = now_ms - previous;
    if delta.is_finite() && delta > 0.0 {
        core::time::Duration::from_secs_f64(delta / 1000.0)
    } else {
        core::time::Duration::ZERO
    }
}

#[cfg(test)]
mod tests {
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
        LOGGER.log(
            &log::Record::builder()
                .level(log::Level::Info)
                .target(target)
                .args(format_args!("{message}"))
                .build(),
        );
    }

    /// **A page that stops draining bounds the queue and says how much it lost.**
    ///
    /// Both halves matter and each fails silently on its own: an unbounded queue
    /// grows wasm memory until the tab dies, and a bounded one that drops
    /// quietly turns "the log is missing the interesting part" into a mystery.
    #[test]
    fn a_queue_nobody_drains_is_bounded_and_reports_what_it_dropped() {
        log::set_max_level(log::LevelFilter::Info);
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
        log::set_max_level(log::LevelFilter::Info);
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

    // ---- the lifecycle ------------------------------------------------------

    /// A loop that runs for a set number of frames and then stops.
    struct FakeLoop {
        frames_left: u32,
        paused: bool,
        fail_frame: bool,
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

        fn set_frame_step(&mut self, _dt: core::time::Duration) {}

        fn log_summary(summary: &Self::Summary) {
            log::info!("fake: {summary} frames left");
        }
    }

    struct FakePending {
        polls_left: u32,
        frames: u32,
        paused: bool,
        fail_frame: bool,
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
            frames: 10,
            paused: false,
            fail_frame: false,
        });
        assert_eq!(app.status(), STATUS_BOOTING);
        assert_eq!(app.frame(0.0), STATUS_BOOTING);
        assert_eq!(app.frame(16.0), STATUS_BOOTING);
        assert_eq!(app.frame(32.0), STATUS_RUNNING, "the device had arrived");
        assert_eq!(app.status(), STATUS_RUNNING);

        let mut app = booting(FakePending {
            polls_left: 0,
            frames: 10,
            paused: true,
            fail_frame: false,
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
            polls_left: 0,
            frames: 1,
            paused: false,
            fail_frame: false,
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
            polls_left: 0,
            frames: 10,
            paused: false,
            fail_frame: true,
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
        let mut app = booting(FakePending {
            polls_left: 0,
            frames: 10,
            paused: false,
            fail_frame: false,
        });
        assert_eq!(app.frame(0.0), STATUS_RUNNING);
        assert_eq!(app.shutdown(), 1);
        assert_eq!(app.status(), STATUS_STOPPED);
        assert_eq!(app.shutdown(), 0, "there was nothing left to tear down");
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
}
