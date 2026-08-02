//! The browser entry point: what the JS shim in `web/` calls.
//!
//! `apps/horde` is a `cdylib` on `wasm32-unknown-unknown`, and this module
//! is the only thing in it a browser can reach. Everything here is an
//! `extern "C"` export with `#[unsafe(no_mangle)]`; there are **no imports**.
//!
//! # This is `apps/asteroids/src/web.rs` with a different prefix, for the fourth time
//!
//! S1B **finding 2** said the browser entry point "should be written once,
//! before S2, which will otherwise write it a third time". S2 wrote it a third
//! time and re-owed it **before S3**. This is S3, and this is the fourth copy:
//! the deadline has now been missed twice, by the same argument each time, and
//! the argument is worth restating because it is not "we ran out of room".
//!
//! Sharing this file means putting it somewhere all four samples can reach:
//! a new engine crate or a `crcbl` module owning the `Stage` state machine, the
//! log queue and the five-call protocol, plus a `macro_rules!` wrapper emitting
//! the ten literal `#[unsafe(no_mangle)]` symbols per sample — the names have to
//! stay per-sample, because two demos can be open in one browser and the symbols
//! must not collide, and `concat_idents!` is not stable, so the macro takes them
//! as arguments. That is a change to `crates/`, to `apps/breakout`, to
//! `apps/flappy` and to `apps/asteroids`: an engine-API slice wearing a sample
//! slice's clothes, landing in the same commit as this game's audio, its save
//! file, its demo page and its scale measurement. The JS half was done as its
//! own piece of work for exactly that reason, and the Rust half deserves the
//! same.
//!
//! **What the fourth copy now costs**, which is the part `docs/backlog.md` has
//! been updated to carry: the fix is no longer "write it once and adopt it in
//! two places". It is one new shared implementation plus **four** call sites to
//! migrate, four sets of `STATUS_*` constants to delete, four prefixes to
//! thread through a macro, and four browser gates to re-run. Every further
//! sample adds one more; the cost has grown by a third since the last time this
//! paragraph was written. The JS side settles the shape question in the
//! affirmative — `web/engine/demo.js` is one boot sequence for every demo and
//! each `main.js` is 33 lines of literal symbol names — so the remaining
//! question is only who does the Rust half, and when.
//!
//! # The ABIs a page has to drive
//!
//! | Prefix | Owner | What it is |
//! | --- | --- | --- |
//! | `__crcbl_web_` (input/frame) | [`crcbl::shell`]'s `web` backend | canvas size, focus, keys, pointer |
//! | `__crcbl_web_audio_` | [`crcbl_audio::web`] | the AudioWorklet pull |
//! | `__crcbl_web_fetch_` | [`crcbl_store::web::fetch`] | assets over `fetch()` |
//! | `__crcbl_web_opfs_` | [`crcbl_store::web::opfs`] | saves in the Origin Private File System |
//! | `__crcbl_horde_` | this module | boot, one rAF frame, teardown, logs |
//!
//! ## Exports
//!
//! | Symbol | Signature (wasm) | Meaning |
//! | --- | --- | --- |
//! | [`__crcbl_horde_prepare`] | `() -> i32` | Install the log sink and the browser storage backends. **First call**, before any `__crcbl_web_fetch_*` or `__crcbl_web_opfs_*`. `1`, or `0` if it was already called. |
//! | [`__crcbl_horde_log_level`] | `(i32) -> i32` | Set the log filter: `0` off … `5` trace. `1`/`0`. |
//! | [`__crcbl_horde_boot`] | `() -> i32` | Open the shell on the canvas `__crcbl_web_canvas` announced, create the window, start the polled device request. `1`/`0`. |
//! | [`__crcbl_horde_frame`] | `(f64) -> i32` | One `requestAnimationFrame`, given `performance.now()`. Returns the new status. |
//! | [`__crcbl_horde_status`] | `() -> i32` | The status, without advancing anything. |
//! | [`__crcbl_horde_shutdown`] | `() -> i32` | Tear the loop down. `1` if there was one. |
//! | [`__crcbl_horde_error_ptr`] | `() -> i32` | Address of the last error message (UTF-8, not NUL-terminated), or `0`. |
//! | [`__crcbl_horde_error_len`] | `() -> i32` | Its length in bytes. |
//! | [`__crcbl_horde_log_take`] | `() -> i32` | Pop one log line into the scratch buffer and return its length; `0` when the queue is empty. |
//! | [`__crcbl_horde_log_ptr`] | `() -> i32` | Address of that scratch buffer. Read it **after** `log_take`. |
//!
//! ## Status codes
//!
//! [`STATUS_IDLE`] `0`, [`STATUS_PREPARED`] `1`, [`STATUS_BOOTING`] `2`,
//! [`STATUS_RUNNING`] `3`, [`STATUS_STOPPED`] `4`, [`STATUS_FAILED`] `5`,
//! [`STATUS_PAUSED`] `6`.
//!
//! The shim drives `requestAnimationFrame` while the status is `BOOTING`,
//! `RUNNING` **or** `PAUSED` and stops on anything else — a paused demo is
//! still drawing, and it is a keystroke away from playing again. `FAILED` is
//! the only one that sets an error message.
//!
//! ## Call ordering
//!
//! ```text
//! __crcbl_horde_prepare()                    // storage backends exist
//!   → fetch pre-load       (__crcbl_web_fetch_*)
//!   → OPFS restore + ready (__crcbl_web_opfs_*)
//! __crcbl_web_canvas(id)                         // which canvas this instance drives
//! __crcbl_horde_boot()                       // shell + window; no size yet
//! rAF loop, every frame:
//!   __crcbl_web_resize(id, w, h, dpr)            // from ResizeObserver, when it changes
//!   __crcbl_web_frame(performance.now())         // the shell's clock reference
//!   __crcbl_horde_frame(performance.now())   // boot poll, or a frame
//!   __crcbl_horde_log_take() … while non-zero
//!   __crcbl_web_opfs_take() …                    // drain queued saves
//! ```
//!
//! **The first `__crcbl_web_resize` is what starts the device request.** A
//! canvas has no size until the document gives it one, and a swapchain needs
//! one; a shim that never calls `resize` leaves the status at `BOOTING` forever.
//!
//! # Start-up cannot block, so it is a state machine
//!
//! Device creation is polled across the whole HAL seam, because the promise
//! behind `requestDevice` is resolved by the page's event loop — the very loop a
//! blocking wait would be sitting inside. [`__crcbl_horde_frame`] therefore
//! does one of two different things depending on the status: while `BOOTING` it
//! polls [`PendingLoop`], and once that yields it runs
//! frames.
//!
//! # The clock is the browser's
//!
//! `std::time::Instant::now()` **panics** on `wasm32-unknown-unknown`, so
//! `Clock::Real` cannot be used here and neither can
//! `crcbl_core::log::init_logging`, which stamps its logger with an `Instant`.
//! The loop is built on `Clock::manual` and told how far to step from the
//! `performance.now()` the shim passes in; the logger is `WebLogger` below,
//! which has no clock at all.
//!
//! # The module imports nothing of its own
//!
//! That is worth the small awkwardness because of what it buys: the wasm
//! module's *only* imports are the ones `wasm-bindgen` generates for `wgpu`'s
//! `web-sys` calls, which means the import list is a thing CI can assert about.
//! `web/tools/check-exports.mjs` does exactly that — every import must be in
//! the `wbg` module, so an accidental `extern "C" { fn … }` somewhere in the
//! engine turns into a failed check rather than a `LinkError` in someone's
//! browser.

use core::time::Duration;
use std::cell::RefCell;
use std::rc::Rc;

use crcbl::engine::{Clock, ExitReason, Flow};
use crcbl::shell::{ShellBackend, open_backend};
use crcbl_store::web::{FetchSource, OpfsStorage};

use crate::app::{Loop, PendingLoop};
use crate::args::Options;

// ---------------------------------------------------------------------------
// Status
// ---------------------------------------------------------------------------

/// Nothing has been prepared; [`__crcbl_horde_prepare`] has not run.
pub const STATUS_IDLE: u32 = 0;
/// Storage is installed and the shim may pre-load; no shell yet.
pub const STATUS_PREPARED: u32 = 1;
/// Waiting for the canvas's first size, or for the device promise.
pub const STATUS_BOOTING: u32 = 2;
/// Playing. Every [`__crcbl_horde_frame`] draws.
pub const STATUS_RUNNING: u32 = 3;
/// The loop ended on its own terms — the page asked it to close, or the window
/// went away. Not an error.
pub const STATUS_STOPPED: u32 = 4;
/// Something failed; [`__crcbl_horde_error_ptr`] says what.
pub const STATUS_FAILED: u32 = 5;
/// Running, but the simulation is stopped: the player pressed Escape, or the
/// canvas lost focus.
///
/// A separate code rather than a flag beside `RUNNING`, because the page's
/// status line is a *status*: it must not read "Playing." while the canvas sits
/// unfocused behind something else. Numbered after `FAILED` so the codes already
/// published to the shim keep their values.
pub const STATUS_PAUSED: u32 = 6;

/// The base URL the [`FetchSource`] resolves asset keys against.
///
/// Relative to the *document*, so a demo served from `/demos/horde/`
/// fetches `/demos/horde/assets/<key>`. The trailing slash is required by
/// [`FetchSource::new`], which refuses a base without one so that a key can
/// never be read as a scheme.
pub const ASSET_BASE: &str = "assets/";

/// The most log lines held for the shim before the oldest is dropped.
///
/// A page that never drains must not grow wasm memory without bound; a page
/// that drains once per frame will never see this.
const MAX_LOG_LINES: usize = 512;

/// The longest log line handed to the shim, in bytes.
const MAX_LOG_LINE: usize = 1024;

// ---------------------------------------------------------------------------
// State
// ---------------------------------------------------------------------------

/// Where the demo has got to. One value, so no two of these can be true at once.
///
/// Both live variants are boxed. `Loop` is the whole engine — swapchain ring,
/// render graph pool, ECS world — and an unboxed variant would make every
/// `Stage` assignment in [`__crcbl_horde_frame`] a multi-kilobyte move, once
/// per frame, for no reason.
enum Stage {
    Idle,
    Prepared,
    Booting(Box<PendingLoop<dyn crcbl::shell::Shell>>),
    Running(Box<Loop<dyn crcbl::shell::Shell>>),
    Stopped,
    Failed,
}

impl Stage {
    fn status(&self) -> u32 {
        match self {
            Self::Idle => STATUS_IDLE,
            Self::Prepared => STATUS_PREPARED,
            Self::Booting(_) => STATUS_BOOTING,
            Self::Running(engine) => running_status(engine),
            Self::Stopped => STATUS_STOPPED,
            Self::Failed => STATUS_FAILED,
        }
    }
}

/// Everything this module owns for the life of the page.
///
/// The storage handles are held here because both crates' `install` keeps only
/// a [`std::rc::Weak`]: dropping the `Rc` would silently turn every
/// `__crcbl_web_opfs_*` call into a `0`, and the first symptom would be a best
/// score that never saves.
struct App {
    stage: Stage,
    assets: Option<Rc<FetchSource>>,
    saves: Option<Rc<OpfsStorage>>,
    /// `performance.now()` at the last frame, for the delta the clock is
    /// stepped by. `None` until the loop starts running.
    last_ms: Option<f64>,
    error: String,
}

impl App {
    const fn new() -> Self {
        Self {
            stage: Stage::Idle,
            assets: None,
            saves: None,
            last_ms: None,
            error: String::new(),
        }
    }

    /// Records `error`, fails the stage, and returns [`STATUS_FAILED`].
    ///
    /// A failure is terminal: nothing here retries, because every failure this
    /// can see (no WebGPU adapter, a device that refused the features the UI
    /// pass needs) is a property of the browser rather than a transient.
    fn fail(&mut self, error: impl core::fmt::Display) -> u32 {
        use core::fmt::Write as _;
        self.error.clear();
        let _ = write!(self.error, "{error}");
        log::error!("horde: {}", self.error);
        self.stage = Stage::Failed;
        STATUS_FAILED
    }
}

thread_local! {
    static APP: RefCell<App> = const { RefCell::new(App::new()) };
}

/// Runs `f` against the page's state.
///
/// `absent` is returned when the cell is already borrowed, which can only
/// happen if an export were called re-entrantly from another export — the shim
/// never does, and answering rather than panicking keeps a shim bug from
/// aborting the wasm instance.
fn with_app<R>(absent: R, f: impl FnOnce(&mut App) -> R) -> R {
    APP.with(|slot| match slot.try_borrow_mut() {
        Ok(mut app) => f(&mut app),
        Err(_) => absent,
    })
}

/// The OPFS store the shim restored into, if [`__crcbl_horde_prepare`] ran.
///
/// `crate::best`'s browser arm. Returns `None` on a page that never prepared,
/// which is a shim that started the game before the storage existed.
#[must_use]
pub fn opfs_store() -> Option<Rc<OpfsStorage>> {
    with_app(None, |app| app.saves.clone())
}

/// The asset source the shim pre-loads into, if [`__crcbl_horde_prepare`]
/// ran.
#[must_use]
pub fn asset_source() -> Option<Rc<FetchSource>> {
    with_app(None, |app| app.assets.clone())
}

// ---------------------------------------------------------------------------
// Logging
// ---------------------------------------------------------------------------

/// The queue [`__crcbl_horde_log_take`] drains.
#[derive(Default)]
struct LogQueue {
    lines: std::collections::VecDeque<String>,
    /// The line the shim is currently reading. Kept at a fixed capacity so its
    /// address does not move between a `take` and the `log_ptr` that follows.
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
/// only one in the module (see the module docs) and a timestamp would need
/// [`std::time::Instant`], which this target does not have. The browser's
/// console stamps the line when the shim prints it, which is within a frame of
/// when it was written.
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
            // Byte truncation would split a UTF-8 sequence; `char_indices`
            // finds the last boundary at or before the limit.
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

/// Installs the queueing logger, unless one is already installed.
fn install_logger() {
    if log::set_logger(&LOGGER).is_ok() {
        log::set_max_level(log::LevelFilter::Info);
    }
}

// ---------------------------------------------------------------------------
// Exports
// ---------------------------------------------------------------------------

/// Install the log sink and the browser storage backends.
///
/// The first call the shim makes, and the one that has to happen before any
/// `__crcbl_web_fetch_*` or `__crcbl_web_opfs_*` call — both of those answer
/// `0` until something is installed, which is the documented "a shim that
/// started before the engine did" case rather than a failure.
///
/// Returns `1`, or `0` if it had already run.
#[unsafe(no_mangle)]
pub extern "C" fn __crcbl_horde_prepare() -> u32 {
    install_logger();
    with_app(0, |app| {
        if !matches!(app.stage, Stage::Idle) {
            return 0;
        }

        let saves = Rc::new(OpfsStorage::new());
        if !crcbl_store::web::opfs::install(&saves) {
            app.fail("an OPFS store was already installed");
            return 0;
        }
        app.saves = Some(saves);

        match FetchSource::new(ASSET_BASE) {
            Ok(source) => {
                let source = Rc::new(source);
                if !crcbl_store::web::fetch::install(&source) {
                    app.fail("a fetch source was already installed");
                    return 0;
                }
                app.assets = Some(source);
            }
            Err(error) => {
                app.fail(error);
                return 0;
            }
        }

        log::info!("horde: prepared; assets from {ASSET_BASE}");
        app.stage = Stage::Prepared;
        1
    })
}

/// Set the log filter: `0` off, `1` error, `2` warn, `3` info, `4` debug,
/// `5` trace.
///
/// Returns `1`, or `0` for a level outside that range.
#[unsafe(no_mangle)]
pub extern "C" fn __crcbl_horde_log_level(level: u32) -> u32 {
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

/// Open the shell and the window, and start the polled device request.
///
/// The canvas is the one `__crcbl_web_canvas` announced, so that call must come
/// first; there is deliberately no argument here, because a canvas id that
/// entered wasm through two doors could disagree with itself and the shell's
/// event routing would silently drop everything.
///
/// Returns `1`, or `0` if the page had not prepared or the shell refused.
#[unsafe(no_mangle)]
pub extern "C" fn __crcbl_horde_boot() -> u32 {
    with_app(0, |app| {
        if !matches!(app.stage, Stage::Prepared) {
            return 0;
        }
        let shell = match open_backend(ShellBackend::Web) {
            Ok(shell) => shell,
            Err(error) => {
                app.fail(error);
                return 0;
            }
        };
        let options = Options::default();
        // `Clock::manual` rather than `Clock::new(false)`: see the module docs.
        // The step is replaced every frame with what the browser reported.
        match PendingLoop::request(shell, &options, Clock::manual(Duration::ZERO)) {
            Ok(pending) => {
                app.stage = Stage::Booting(Box::new(pending));
                log::info!("horde: booting; waiting for the canvas to report a size");
                1
            }
            Err(error) => {
                app.fail(error);
                0
            }
        }
    })
}

/// One `requestAnimationFrame`.
///
/// `now_ms` is `performance.now()`. Call `__crcbl_web_frame(now_ms)` first, so
/// the shell's event-clock reference is this frame's and not the previous one's.
///
/// Returns the status afterwards; the shim keeps scheduling frames while it is
/// [`STATUS_BOOTING`], [`STATUS_RUNNING`] or [`STATUS_PAUSED`].
#[unsafe(no_mangle)]
pub extern "C" fn __crcbl_horde_frame(now_ms: f64) -> u32 {
    with_app(STATUS_FAILED, |app| {
        match core::mem::replace(&mut app.stage, Stage::Failed) {
            Stage::Booting(mut pending) => match pending.poll() {
                Ok(Some(engine)) => {
                    log::info!("horde: running at {:?}", engine.extent());
                    let status = running_status(&engine);
                    app.stage = Stage::Running(Box::new(engine));
                    app.last_ms = Some(now_ms);
                    status
                }
                Ok(None) => {
                    app.stage = Stage::Booting(pending);
                    STATUS_BOOTING
                }
                Err(error) => app.fail(error),
            },
            Stage::Running(mut engine) => {
                engine.set_frame_step(step_from(app.last_ms.replace(now_ms), now_ms));
                match engine.frame() {
                    Ok(Flow::Continue) => {
                        let status = running_status(&engine);
                        app.stage = Stage::Running(engine);
                        status
                    }
                    Ok(Flow::Stop(reason)) => finish(app, engine, reason),
                    Err(error) => {
                        // The frame error is the one worth reporting; a
                        // teardown failure on top of it is logged, exactly as
                        // the native `run` does.
                        if let Err(teardown) = engine.finish(ExitReason::Failed) {
                            log::error!("teardown after a failed frame also failed: {teardown}");
                        }
                        app.fail(error)
                    }
                }
            }
            other => {
                let status = other.status();
                app.stage = other;
                status
            }
        }
    })
}

/// [`STATUS_PAUSED`] or [`STATUS_RUNNING`], from the loop itself.
///
/// Asked of the loop rather than tracked here: the page has no way to know that
/// a `blur` paused the game, and a second copy of the answer would be the thing
/// that drifts.
fn running_status(engine: &Loop<dyn crcbl::shell::Shell>) -> u32 {
    if engine.is_paused() {
        STATUS_PAUSED
    } else {
        STATUS_RUNNING
    }
}

/// Tears `engine` down and records how it ended.
fn finish(app: &mut App, engine: Box<Loop<dyn crcbl::shell::Shell>>, reason: ExitReason) -> u32 {
    match engine.finish(reason) {
        Ok(summary) => {
            log::info!(
                "horde: {} frames, {} ticks, survived {:.1}s with {} kills at level {} \
                 ({} enemies left, {:?}, {:?})",
                summary.frames,
                summary.ticks,
                summary.elapsed,
                summary.kills,
                summary.level,
                summary.enemies,
                summary.state,
                summary.exit,
            );
            app.stage = Stage::Stopped;
            STATUS_STOPPED
        }
        Err(error) => app.fail(error),
    }
}

/// How far the clock should step for a frame that arrived at `now_ms`.
///
/// `performance.now()` is monotonic, but a shim that passed the wrong number —
/// or a first frame with no predecessor — must not produce a negative or
/// non-finite `Duration`, which would panic in `from_secs_f64`. The upper bound
/// is `Loop::set_frame_step`'s job, because a native manual clock needs it too.
fn step_from(previous: Option<f64>, now_ms: f64) -> Duration {
    let Some(previous) = previous else {
        return Duration::ZERO;
    };
    let delta = now_ms - previous;
    if delta.is_finite() && delta > 0.0 {
        Duration::from_secs_f64(delta / 1000.0)
    } else {
        Duration::ZERO
    }
}

/// The status, without advancing anything.
#[unsafe(no_mangle)]
pub extern "C" fn __crcbl_horde_status() -> u32 {
    with_app(STATUS_FAILED, |app| app.stage.status())
}

/// Tear the loop down: release the swapchain, the device and the window.
///
/// Returns `1` if there was something to tear down. Safe to call from
/// `beforeunload`; the page's OPFS drain should happen *before* it, because the
/// game's last write is queued during its last frame.
#[unsafe(no_mangle)]
pub extern "C" fn __crcbl_horde_shutdown() -> u32 {
    with_app(0, |app| {
        match core::mem::replace(&mut app.stage, Stage::Stopped) {
            Stage::Running(engine) => {
                finish(app, engine, ExitReason::CloseRequested);
                1
            }
            Stage::Booting(_) => 1,
            other => {
                app.stage = other;
                0
            }
        }
    })
}

/// Address of the last error message (UTF-8, not NUL-terminated), or `0`.
///
/// Valid until the next export call. Read [`__crcbl_horde_error_len`] first
/// and decode immediately.
#[unsafe(no_mangle)]
pub extern "C" fn __crcbl_horde_error_ptr() -> *const u8 {
    with_app(core::ptr::null(), |app| {
        if app.error.is_empty() {
            core::ptr::null()
        } else {
            app.error.as_ptr()
        }
    })
}

/// The length of that message in bytes.
#[unsafe(no_mangle)]
pub extern "C" fn __crcbl_horde_error_len() -> u32 {
    with_app(0, |app| u32::try_from(app.error.len()).unwrap_or(u32::MAX))
}

/// Pop one log line into the scratch buffer and return its length in bytes.
///
/// `0` means the queue is empty. Call [`__crcbl_horde_log_ptr`] **after**
/// this, not before: the two together are one read, and the buffer's contents
/// belong to the most recent `take`.
#[unsafe(no_mangle)]
pub extern "C" fn __crcbl_horde_log_take() -> u32 {
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
                    "[WARN] horde::web: {dropped} log lines dropped; the shim is not draining"
                );
            }
        }
        u32::try_from(queue.current.len()).unwrap_or(u32::MAX)
    })
}

/// Address of the log scratch buffer, or `0` when nothing has been taken.
#[unsafe(no_mangle)]
pub extern "C" fn __crcbl_horde_log_ptr() -> *const u8 {
    LOG.with(|slot| match slot.try_borrow() {
        Ok(queue) if !queue.current.is_empty() => queue.current.as_ptr(),
        _ => core::ptr::null(),
    })
}
