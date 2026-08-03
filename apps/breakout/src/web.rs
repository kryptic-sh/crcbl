//! The browser entry point (P5.8): what the JS shim in `web/` calls.
//!
//! `apps/breakout` is a `cdylib` on `wasm32-unknown-unknown`, and this module
//! is the only thing in it a browser can reach. Everything here is an
//! `extern "C"` export with `#[unsafe(no_mangle)]`; there are **no imports**.
//! That is a deliberate property and not an accident of scope — see
//! [below](#the-module-imports-nothing-of-its-own).
//!
//! # The four ABIs a page has to drive, and how they fit together
//!
//! This module is the fifth and smallest. The other four are specified, symbol
//! by symbol, by the crates that own them, and the shim calls them directly:
//!
//! | Prefix | Owner | What it is |
//! | --- | --- | --- |
//! | `__crcbl_web_` (input/frame) | [`crcbl::shell`]'s `web` backend | canvas size, focus, keys, pointer |
//! | `__crcbl_web_audio_` | [`crcbl::audio::web`] | the AudioWorklet pull |
//! | `__crcbl_web_fetch_` | [`crcbl::store::web::fetch`] | assets over `fetch()` |
//! | `__crcbl_web_opfs_` | [`crcbl::store::web::opfs`] | saves in the Origin Private File System |
//! | `__crcbl_breakout_` | this module | boot, one rAF frame, teardown, logs |
//!
//! ## Exports
//!
//! | Symbol | Signature (wasm) | Meaning |
//! | --- | --- | --- |
//! | [`__crcbl_breakout_prepare`] | `() -> i32` | Install the log sink and the browser storage backends. **First call**, before any `__crcbl_web_fetch_*` or `__crcbl_web_opfs_*`. `1`, or `0` if it was already called. |
//! | [`__crcbl_breakout_log_level`] | `(i32) -> i32` | Set the log filter: `0` off … `5` trace. `1`/`0`. |
//! | [`__crcbl_breakout_boot`] | `() -> i32` | Open the shell on the canvas `__crcbl_web_canvas` announced, create the window, start the polled device request. `1`/`0`. |
//! | [`__crcbl_breakout_frame`] | `(f64) -> i32` | One `requestAnimationFrame`, given `performance.now()`. Returns the new status. |
//! | [`__crcbl_breakout_status`] | `() -> i32` | The status, without advancing anything. |
//! | [`__crcbl_breakout_shutdown`] | `() -> i32` | Tear the loop down. `1` if there was one. |
//! | [`__crcbl_breakout_error_ptr`] | `() -> i32` | Address of the last error message (UTF-8, not NUL-terminated), or `0`. |
//! | [`__crcbl_breakout_error_len`] | `() -> i32` | Its length in bytes. |
//! | [`__crcbl_breakout_log_take`] | `() -> i32` | Pop one log line into the scratch buffer and return its length; `0` when the queue is empty. |
//! | [`__crcbl_breakout_log_ptr`] | `() -> i32` | Address of that scratch buffer. Read it **after** `log_take`. |
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
//! __crcbl_breakout_prepare()                    // storage backends exist
//!   → fetch pre-load       (__crcbl_web_fetch_*)
//!   → OPFS restore + ready (__crcbl_web_opfs_*)
//! __crcbl_web_canvas(id)                        // which canvas this instance drives
//! __crcbl_breakout_boot()                       // shell + window; no size yet
//! rAF loop, every frame:
//!   __crcbl_web_resize(id, w, h, dpr)           // from ResizeObserver, when it changes
//!   __crcbl_web_frame(performance.now())        // the shell's clock reference
//!   __crcbl_breakout_frame(performance.now())   // boot poll, or a frame
//!   __crcbl_breakout_log_take() … while non-zero
//!   __crcbl_web_opfs_take() …                   // drain queued saves
//! ```
//!
//! **The first `__crcbl_web_resize` is what starts the device request.** A
//! canvas has no size until the document gives it one, and a swapchain needs
//! one; a shim that never calls `resize` leaves the status at `BOOTING`
//! forever, which is the same handshake Wayland forces and the same symptom it
//! produces.
//!
//! # Start-up cannot block, so it is a state machine
//!
//! P5.4 made device creation polled across the whole HAL seam:
//! `Instance::request_device` → `PendingDevice::poll`, with `create_device`
//! `cfg`'d out on `wasm32` entirely. The reason is that the promise behind
//! `requestDevice` is resolved by the page's event loop — the very loop a
//! blocking wait would be sitting inside — so a browser that blocked on it
//! would deadlock against itself and the tab would simply stop.
//!
//! [`__crcbl_breakout_frame`] therefore does one of two different things
//! depending on the status: while `BOOTING` it polls
//! [`PendingLoop`], and once that yields it runs
//! frames. Several rAF ticks pass before the first frame is drawn, and that is
//! the design rather than a delay to be optimised away.
//!
//! # The clock is the browser's
//!
//! `std::time::Instant::now()` **panics** on `wasm32-unknown-unknown` — the
//! target has no time implementation at all — so `Clock::Real` cannot be used
//! here, and neither can `crcbl::core::log::init_logging`, which stamps its
//! logger with an `Instant`. The loop is built on `Clock::manual` and told how
//! far to step from the `performance.now()` the shim passes in; the logger is
//! `crcbl::web`'s own logger, which has no clock at all and lets the console
//! timestamp the line.
//!
//! # The module imports nothing of its own
//!
//! Every one of the five ABIs is exports-plus-polling: JS calls in, reads a
//! buffer wasm owns, and never passes a pointer or a callback the other way.
//! Logging is the case that would most naturally have been an import —
//! `console.log` is right there — and it is a queue the shim drains instead
//! ([`__crcbl_breakout_log_take`]).
//!
//! That is worth the small awkwardness because of what it buys: the wasm
//! module's *only* imports are the ones `wasm-bindgen` generates for `wgpu`'s
//! `web-sys` calls, which means the import list is a thing CI can assert about.
//! `web/tools/check-exports.mjs` does exactly that — every import must be in
//! the `wbg` module, so an accidental `extern "C" { fn … }` somewhere in the
//! engine turns into a failed check rather than a `LinkError` in someone's
//! browser.

use std::cell::RefCell;
use std::rc::Rc;

use crcbl::engine::Clock;
use crcbl::log;
use crcbl::store::web::{FetchSource, OpfsStorage};
use crcbl::web::{App, WebPending};

use crate::app::{Loop, PendingLoop};
use crate::args::Options;

// The status codes and the asset base are the shim's wire format, so they have
// exactly one definition; see [`crcbl::web`]. Re-exported rather than reached
// through the path, because this module's own docs name them.
pub use crcbl::web::{
    ASSET_BASE, STATUS_BOOTING, STATUS_FAILED, STATUS_IDLE, STATUS_PAUSED, STATUS_PREPARED,
    STATUS_RUNNING, STATUS_STOPPED,
};

// ---------------------------------------------------------------------------
// This game's half of the lifecycle
// ---------------------------------------------------------------------------

/// **There is no `WebLoop` impl here.** `crcbl::web` blanket-implements it for
/// every `crcbl::engine::Loop`, and the two halves that were ever this game's —
/// its name and the log line a finished run is worth — are
/// `HostedGame::NAME` and `HostedGame::log_summary` in `app.rs`. What is left
/// below is start-up, which stays here because the options it opens with are
/// breakout's.
impl WebPending for PendingLoop<dyn crcbl::shell::Shell> {
    type Loop = Loop<dyn crcbl::shell::Shell>;

    fn request(
        shell: Box<dyn crcbl::shell::Shell>,
        clock: Clock,
    ) -> Result<Self, crate::app::BreakoutError> {
        Self::request(shell, &Options::default(), clock)
    }

    fn poll(&mut self) -> Result<Option<Self::Loop>, crate::app::BreakoutError> {
        Self::poll(self)
    }
}

thread_local! {
    static APP: RefCell<App<PendingLoop<dyn crcbl::shell::Shell>>> =
        const { RefCell::new(App::new()) };

    /// The storage handles, held for the life of the page.
    ///
    /// Both crates' `install` keeps only a [`std::rc::Weak`]: dropping the `Rc`
    /// would silently turn every `__crcbl_web_opfs_*` call into a `0`, and the
    /// first symptom would be a high score that never saves.
    static STORAGE: RefCell<Option<(Rc<OpfsStorage>, Rc<FetchSource>)>> =
        const { RefCell::new(None) };
}

/// Runs `f` against the page's state.
///
/// `absent` is returned when the cell is already borrowed, which can only happen
/// if an export were called re-entrantly from another export — the shim never
/// does, and answering rather than panicking keeps a shim bug from aborting the
/// wasm instance.
fn with_app<R>(
    absent: R,
    f: impl FnOnce(&mut App<PendingLoop<dyn crcbl::shell::Shell>>) -> R,
) -> R {
    APP.with(|slot| match slot.try_borrow_mut() {
        Ok(mut app) => f(&mut app),
        Err(_) => absent,
    })
}

/// The OPFS store the shim restored into, if `prepare` ran.
///
/// `crate::high_score`'s browser arm. Returns `None` on a page that never
/// prepared, which is a shim that started the game before the storage existed.
#[must_use]
pub fn opfs_store() -> Option<Rc<OpfsStorage>> {
    STORAGE.with(|slot| slot.borrow().as_ref().map(|(saves, _)| Rc::clone(saves)))
}

/// The asset source the shim pre-loads into, if `prepare` ran.
#[must_use]
pub fn asset_source() -> Option<Rc<FetchSource>> {
    STORAGE.with(|slot| slot.borrow().as_ref().map(|(_, assets)| Rc::clone(assets)))
}

// ---------------------------------------------------------------------------
// Exports
// ---------------------------------------------------------------------------

/// Install the log sink and the browser storage backends.
///
/// The first call the shim makes, and the one that has to happen before any
/// `__crcbl_web_fetch_*` or `__crcbl_web_opfs_*` call — both of those answer `0`
/// until something is installed, which is the documented "a shim that started
/// before the engine did" case rather than a failure.
///
/// Returns `1`, or `0` if it had already run.
#[unsafe(no_mangle)]
pub extern "C" fn __crcbl_breakout_prepare() -> u32 {
    crcbl::web::install_logger();
    with_app(0, |app| {
        if !app.is_idle() {
            return 0;
        }

        let saves = Rc::new(OpfsStorage::new());
        if !crcbl::store::web::opfs::install(&saves) {
            app.fail("an OPFS store was already installed");
            return 0;
        }

        let assets = match FetchSource::new(ASSET_BASE) {
            Ok(source) => Rc::new(source),
            Err(error) => {
                app.fail(error);
                return 0;
            }
        };
        if !crcbl::store::web::fetch::install(&assets) {
            app.fail("a fetch source was already installed");
            return 0;
        }
        STORAGE.with(|slot| *slot.borrow_mut() = Some((saves, assets)));

        log::info!("breakout: prepared; assets from {ASSET_BASE}");
        app.prepared();
        1
    })
}

/// Set the log filter: `0` off, `1` error, `2` warn, `3` info, `4` debug,
/// `5` trace.
///
/// Returns `1`, or `0` for a level outside that range.
#[unsafe(no_mangle)]
pub extern "C" fn __crcbl_breakout_log_level(level: u32) -> u32 {
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
pub extern "C" fn __crcbl_breakout_boot() -> u32 {
    with_app(0, App::boot)
}

/// One `requestAnimationFrame`.
///
/// `now_ms` is `performance.now()`. Call `__crcbl_web_frame(now_ms)` first, so
/// the shell's event-clock reference is this frame's and not the previous one's.
///
/// Returns the status afterwards; the shim keeps scheduling frames while it is
/// [`STATUS_BOOTING`] or [`STATUS_RUNNING`].
#[unsafe(no_mangle)]
pub extern "C" fn __crcbl_breakout_frame(now_ms: f64) -> u32 {
    with_app(STATUS_FAILED, |app| app.frame(now_ms))
}

/// The status, without advancing anything.
#[unsafe(no_mangle)]
pub extern "C" fn __crcbl_breakout_status() -> u32 {
    with_app(STATUS_FAILED, |app| app.status())
}

/// Tear the loop down: release the swapchain, the device and the window.
///
/// Returns `1` if there was something to tear down. Safe to call from
/// `beforeunload`; the page's OPFS drain should happen *before* it, because the
/// game's last write is queued during its last frame.
#[unsafe(no_mangle)]
pub extern "C" fn __crcbl_breakout_shutdown() -> u32 {
    with_app(0, App::shutdown)
}

/// Address of the last error message (UTF-8, not NUL-terminated), or `0`.
///
/// Valid until the next export call. Read [`__crcbl_breakout_error_len`] first and
/// decode immediately.
#[unsafe(no_mangle)]
pub extern "C" fn __crcbl_breakout_error_ptr() -> *const u8 {
    with_app(core::ptr::null(), |app| app.error_ptr())
}

/// The length of that message in bytes.
#[unsafe(no_mangle)]
pub extern "C" fn __crcbl_breakout_error_len() -> u32 {
    with_app(0, |app| app.error_len())
}

/// Pop one log line into the scratch buffer and return its length in bytes.
///
/// `0` means the queue is empty. Call [`__crcbl_breakout_log_ptr`] **after** this,
/// not before: the two together are one read, and the buffer's contents belong
/// to the most recent `take`.
#[unsafe(no_mangle)]
pub extern "C" fn __crcbl_breakout_log_take() -> u32 {
    crcbl::web::log_take()
}

/// Address of the log scratch buffer, or `0` when nothing has been taken.
#[unsafe(no_mangle)]
pub extern "C" fn __crcbl_breakout_log_ptr() -> *const u8 {
    crcbl::web::log_ptr()
}
