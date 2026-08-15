//! The browser entry point: what the JS shim in `web/` calls.
//!
//! `apps/horde` is a `cdylib` on `wasm32-unknown-unknown`, and this module
//! is the only thing in it a browser can reach. Everything here is an
//! `extern "C"` export with `#[unsafe(no_mangle)]`; there are **no imports**.
//!
//! # Only the symbol names are this game's
//!
//! The state machine behind these exports, the log queue and the five-call
//! protocol are [`crcbl::web`], and [`crcbl::web_exports!`] writes the ten
//! symbols in the table below. That module is also where the reasons live: why
//! start-up is polled rather than blocking, why the clock is the browser's, and
//! why a sample's wasm module imports nothing of its own.
//!
//! What is left here is what is genuinely horde's: the [`WebPending`] impl,
//! which opens the game with its own [`Options`], and the two accessors
//! `crate::best` reads a score through. The symbol names stay here too, written
//! out one per line — two demos can be open in one browser and the exports must
//! not collide, so the macro takes each name as an argument rather than building
//! it from a prefix.
//!
//! # The ABIs a page has to drive
//!
//! | Prefix | Owner | What it is |
//! | --- | --- | --- |
//! | `__crcbl_web_` (input/frame) | [`crcbl::shell`]'s `web` backend | canvas size, focus, keys, pointer |
//! | `__crcbl_web_audio_` | [`crcbl::audio::web`] | the AudioWorklet pull |
//! | `__crcbl_web_fetch_` | [`crcbl::store::web::fetch`] | assets over `fetch()` |
//! | `__crcbl_web_opfs_` | [`crcbl::store::web::opfs`] | saves in the Origin Private File System |
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
//! __crcbl_web_canvas(id)                     // which canvas this instance drives
//! __crcbl_horde_boot()                       // shell + window; no size yet
//! rAF loop, every frame:
//!   __crcbl_web_resize(id, w, h, dpr)        // from ResizeObserver, when it changes
//!   __crcbl_web_frame(performance.now())     // the shell's clock reference
//!   __crcbl_horde_frame(performance.now())   // boot poll, or a frame
//!   __crcbl_horde_log_take() … while non-zero
//!   __crcbl_web_opfs_take() …                // drain queued saves
//! ```
//!
//! **The first `__crcbl_web_resize` is what starts the device request.** A
//! canvas has no size until the document gives it one, and a swapchain needs
//! one; a shim that never calls `resize` leaves the status at `BOOTING` forever.

use std::rc::Rc;

use crcbl::engine::Clock;
use crcbl::store::web::{FetchSource, OpfsStorage};
use crcbl::web::WebPending;

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
/// its name and the log line a finished run is worth — are `HostedGame::NAME`
/// and `HostedGame::log_summary` in `app.rs`. What is left below is start-up,
/// which stays here because the options it opens with are horde's.
impl WebPending for PendingLoop<dyn crcbl::shell::Shell> {
    type Loop = Loop<dyn crcbl::shell::Shell>;

    fn request(
        shell: Box<dyn crcbl::shell::Shell>,
        clock: Clock,
    ) -> Result<Self, crate::app::HordeError> {
        Self::request(shell, &Options::default(), clock)
    }

    fn poll(&mut self) -> Result<Option<Self::Loop>, crate::app::HordeError> {
        Self::poll(self)
    }
}

// ---------------------------------------------------------------------------
// Exports
// ---------------------------------------------------------------------------

crcbl::web_exports! {
    pending: PendingLoop<dyn crcbl::shell::Shell>,
    prepare: __crcbl_horde_prepare,
    log_level: __crcbl_horde_log_level,
    boot: __crcbl_horde_boot,
    frame: __crcbl_horde_frame,
    status: __crcbl_horde_status,
    shutdown: __crcbl_horde_shutdown,
    error_ptr: __crcbl_horde_error_ptr,
    error_len: __crcbl_horde_error_len,
    log_take: __crcbl_horde_log_take,
    log_ptr: __crcbl_horde_log_ptr,
}

/// The OPFS store the shim restored into, if `prepare` ran.
///
/// `crate::best`'s browser arm. Returns `None` on a page that never
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
