//! The browser entry point: what the JS shim in `web/` calls.
//!
//! `apps/flappy` is a `cdylib` on `wasm32-unknown-unknown`, and this module is
//! the only thing in it a browser can reach. Everything here is an `extern "C"`
//! export with `#[unsafe(no_mangle)]`; there are **no imports**.
//!
//! # This is `apps/breakout/src/web.rs` with a different prefix
//!
//! Which is the finding, and the reason it is written out rather than shared.
//! The five-ABI protocol below — prepare, boot, one frame per
//! `requestAnimationFrame`, status, shutdown, a log queue the page drains — is
//! not breakout's protocol. It is *the engine's* browser protocol, and every
//! wasm sample will need exactly it. The only genuinely per-game parts are the
//! symbol prefix and the two types named in [`Stage`].
//!
//! Nothing in the engine offers it: `crcbl-shell`, `crcbl-audio` and
//! `crcbl-store` each export their own `__crcbl_web_*` ABI, and the sixth —
//! the one that owns the loop — is left to the sample. A second sample is what
//! turns that from a reasonable division into a gap; see the S1B findings in
//! `docs/plan/ROADMAP.md`. Growing the engine to close it is deliberately not
//! this sample's job.
//!
//! # The ABIs a page has to drive
//!
//! | Prefix | Owner | What it is |
//! | --- | --- | --- |
//! | `__crcbl_web_` (input/frame) | [`crcbl::shell`]'s `web` backend | canvas size, focus, keys, pointer |
//! | `__crcbl_web_audio_` | [`crcbl::audio::web`] | the AudioWorklet pull |
//! | `__crcbl_web_fetch_` | [`crcbl::store::web::fetch`] | assets over `fetch()` |
//! | `__crcbl_web_opfs_` | [`crcbl::store::web::opfs`] | saves in the Origin Private File System |
//! | `__crcbl_flappy_` | this module | boot, one rAF frame, teardown, logs |
//!
//! ## Exports
//!
//! | Symbol | Signature (wasm) | Meaning |
//! | --- | --- | --- |
//! | [`__crcbl_flappy_prepare`] | `() -> i32` | Install the log sink and the browser storage backends. **First call**, before any `__crcbl_web_fetch_*` or `__crcbl_web_opfs_*`. `1`, or `0` if it was already called. |
//! | [`__crcbl_flappy_log_level`] | `(i32) -> i32` | Set the log filter: `0` off … `5` trace. `1`/`0`. |
//! | [`__crcbl_flappy_boot`] | `() -> i32` | Open the shell on the canvas `__crcbl_web_canvas` announced, create the window, start the polled device request. `1`/`0`. |
//! | [`__crcbl_flappy_frame`] | `(f64) -> i32` | One `requestAnimationFrame`, given `performance.now()`. Returns the new status. |
//! | [`__crcbl_flappy_status`] | `() -> i32` | The status, without advancing anything. |
//! | [`__crcbl_flappy_shutdown`] | `() -> i32` | Tear the loop down. `1` if there was one. |
//! | [`__crcbl_flappy_error_ptr`] | `() -> i32` | Address of the last error message (UTF-8, not NUL-terminated), or `0`. |
//! | [`__crcbl_flappy_error_len`] | `() -> i32` | Its length in bytes. |
//! | [`__crcbl_flappy_log_take`] | `() -> i32` | Pop one log line into the scratch buffer and return its length; `0` when the queue is empty. |
//! | [`__crcbl_flappy_log_ptr`] | `() -> i32` | Address of that scratch buffer. Read it **after** `log_take`. |
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
//! __crcbl_flappy_prepare()                    // storage backends exist
//!   → fetch pre-load       (__crcbl_web_fetch_*)
//!   → OPFS restore + ready (__crcbl_web_opfs_*)
//! __crcbl_web_canvas(id)                      // which canvas this instance drives
//! __crcbl_flappy_boot()                       // shell + window; no size yet
//! rAF loop, every frame:
//!   __crcbl_web_resize(id, w, h, dpr)         // from ResizeObserver, when it changes
//!   __crcbl_web_frame(performance.now())      // the shell's clock reference
//!   __crcbl_flappy_frame(performance.now())   // boot poll, or a frame
//!   __crcbl_flappy_log_take() … while non-zero
//!   __crcbl_web_opfs_take() …                 // drain queued saves
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
//! blocking wait would be sitting inside. [`__crcbl_flappy_frame`] therefore
//! does one of two different things depending on the status: while `BOOTING` it
//! polls [`PendingLoop`](crate::app::PendingLoop), and once that yields it runs
//! frames.
//!
//! # The clock is the browser's
//!
//! `std::time::Instant::now()` **panics** on `wasm32-unknown-unknown`, so
//! `Clock::Real` cannot be used here and neither can
//! `crcbl::core::log::init_logging`, which stamps its logger with an `Instant`.
//! The loop is built on `Clock::manual` and told how far to step from the
//! `performance.now()` the shim passes in; the logger is [`WebLogger`], which
//! has no clock at all.
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
use crcbl::store::web::{FetchSource, OpfsStorage};

use crate::app::{Loop, PendingLoop};
use crate::args::Options;

// The status codes and the asset base are the shim's wire format, so they
// have exactly one definition; see [`crcbl::web`]. Re-exported rather than
// referenced through the path, because the page's own docs name them.
pub use crcbl::web::{
    ASSET_BASE, STATUS_BOOTING, STATUS_FAILED, STATUS_IDLE, STATUS_PAUSED, STATUS_PREPARED,
    STATUS_RUNNING, STATUS_STOPPED,
};

// ---------------------------------------------------------------------------
// State
// ---------------------------------------------------------------------------

/// Where the demo has got to. One value, so no two of these can be true at once.
///
/// Both live variants are boxed. `Loop` is the whole engine — swapchain ring,
/// render graph pool, ECS world — and an unboxed variant would make every
/// `Stage` assignment in [`__crcbl_flappy_frame`] a multi-kilobyte move, once
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
/// `__crcbl_web_opfs_*` call into a `0`, and the first symptom would be a high
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
        crcbl::log::error!("flappy: {}", self.error);
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

/// The OPFS store the shim restored into, if [`__crcbl_flappy_prepare`] ran.
///
/// [`crate::best`]'s browser arm. Returns `None` on a page that never
/// prepared, which is a shim that started the game before the storage existed.
#[must_use]
pub fn opfs_store() -> Option<Rc<OpfsStorage>> {
    with_app(None, |app| app.saves.clone())
}

/// The asset source the shim pre-loads into, if [`__crcbl_flappy_prepare`] ran.
#[must_use]
pub fn asset_source() -> Option<Rc<FetchSource>> {
    with_app(None, |app| app.assets.clone())
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
pub extern "C" fn __crcbl_flappy_prepare() -> u32 {
    crcbl::web::install_logger();
    with_app(0, |app| {
        if !matches!(app.stage, Stage::Idle) {
            return 0;
        }

        let saves = Rc::new(OpfsStorage::new());
        if !crcbl::store::web::opfs::install(&saves) {
            app.fail("an OPFS store was already installed");
            return 0;
        }
        app.saves = Some(saves);

        match FetchSource::new(ASSET_BASE) {
            Ok(source) => {
                let source = Rc::new(source);
                if !crcbl::store::web::fetch::install(&source) {
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

        crcbl::log::info!("flappy: prepared; assets from {ASSET_BASE}");
        app.stage = Stage::Prepared;
        1
    })
}

/// Set the log filter: `0` off, `1` error, `2` warn, `3` info, `4` debug,
/// `5` trace.
///
/// Returns `1`, or `0` for a level outside that range.
#[unsafe(no_mangle)]
pub extern "C" fn __crcbl_flappy_log_level(level: u32) -> u32 {
    crcbl::web::set_log_level(level)
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
pub extern "C" fn __crcbl_flappy_boot() -> u32 {
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
                crcbl::log::info!("flappy: booting; waiting for the canvas to report a size");
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
/// [`STATUS_BOOTING`] or [`STATUS_RUNNING`].
#[unsafe(no_mangle)]
pub extern "C" fn __crcbl_flappy_frame(now_ms: f64) -> u32 {
    with_app(STATUS_FAILED, |app| {
        match core::mem::replace(&mut app.stage, Stage::Failed) {
            Stage::Booting(mut pending) => match pending.poll() {
                Ok(Some(engine)) => {
                    crcbl::log::info!("flappy: running at {:?}", engine.extent());
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
                            crcbl::log::error!(
                                "teardown after a failed frame also failed: {teardown}"
                            );
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
            crcbl::log::info!(
                "flappy: {} frames, {} ticks, score {} ({:?}, {:?})",
                summary.frames,
                summary.ticks,
                summary.score,
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
pub extern "C" fn __crcbl_flappy_status() -> u32 {
    with_app(STATUS_FAILED, |app| app.stage.status())
}

/// Tear the loop down: release the swapchain, the device and the window.
///
/// Returns `1` if there was something to tear down. Safe to call from
/// `beforeunload`; the page's OPFS drain should happen *before* it, because the
/// game's last write is queued during its last frame.
#[unsafe(no_mangle)]
pub extern "C" fn __crcbl_flappy_shutdown() -> u32 {
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
/// Valid until the next export call. Read [`__crcbl_flappy_error_len`] first
/// and decode immediately.
#[unsafe(no_mangle)]
pub extern "C" fn __crcbl_flappy_error_ptr() -> *const u8 {
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
pub extern "C" fn __crcbl_flappy_error_len() -> u32 {
    with_app(0, |app| u32::try_from(app.error.len()).unwrap_or(u32::MAX))
}

/// Pop one log line into the scratch buffer and return its length in bytes.
///
/// `0` means the queue is empty. Call [`__crcbl_flappy_log_ptr`] **after**
/// this, not before: the two together are one read, and the buffer's contents
/// belong to the most recent `take`.
#[unsafe(no_mangle)]
pub extern "C" fn __crcbl_flappy_log_take() -> u32 {
    crcbl::web::log_take()
}

/// Address of the log scratch buffer, or `0` when nothing has been taken.
#[unsafe(no_mangle)]
pub extern "C" fn __crcbl_flappy_log_ptr() -> *const u8 {
    crcbl::web::log_ptr()
}
