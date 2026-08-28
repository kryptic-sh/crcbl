//! The browser entry point: what the JS shim in `web/` calls.
//!
//! `apps/options` is a `cdylib` on `wasm32-unknown-unknown`, and this module is
//! the only thing in it a browser can reach. Everything here is an `extern "C"`
//! export with `#[unsafe(no_mangle)]`; there are **no imports**.
//!
//! # Only the symbol names are this sample's
//!
//! The state machine behind these exports, the log queue and the five-call
//! protocol are [`crcbl::web`], and [`crcbl::web_exports!`] writes the ten
//! symbols in the table below. That module is where the reasons live: why
//! start-up is polled rather than blocking, why the clock is the browser's, and
//! why a sample's wasm module imports nothing of its own. What is left here is
//! the [`WebPending`](crcbl::web::WebPending) impl, which opens the sample with
//! its own [`Options`], and the symbol names — written out one per line, since
//! two demos can be open in one browser and their exports must not collide.
//!
//! # The half of this sample that only exists here
//!
//! The point of a settings screen is that a setting outlives the run, and in a
//! browser tab that is a claim about the Origin Private File System and nothing
//! else. `__crcbl_options_prepare` installs the OPFS backend before anything
//! reads a key — that is what the ordering below is for — and
//! [`SettingsStack::with_platform_storage`](crcbl::store::settings::SettingsStack::with_platform_storage)
//! resolves to it on `wasm32`, so [`Screen::opened`](crate::app::Screen::opened)
//! reads the player's file and `SAVE` writes it back with no arm of its own.
//! Where no store is installed the reader answers an empty stack and
//! [`SaveState::Nowhere`](crate::app::SaveState) is what the screen shows: a
//! settings screen that silently forgets is the worst version of this bug.
//!
//! # The ABIs a page has to drive
//!
//! | Prefix | Owner | What it is |
//! | --- | --- | --- |
//! | `__crcbl_web_` (input/frame) | [`crcbl::shell`]'s `web` backend | canvas size, focus, keys, pointer |
//! | `__crcbl_web_audio_` | [`crcbl::audio::web`] | the AudioWorklet pull |
//! | `__crcbl_web_fetch_` | [`crcbl::store::web::fetch`] | assets over `fetch()` |
//! | `__crcbl_web_opfs_` | [`crcbl::store::web::opfs`] | saves in the Origin Private File System |
//! | `__crcbl_options_` | this module | boot, one rAF frame, teardown, logs |
//!
//! ## Exports
//!
//! | Symbol | Signature (wasm) | Meaning |
//! | --- | --- | --- |
//! | [`__crcbl_options_prepare`] | `() -> i32` | Install the log sink and the browser storage backends. **First call**, before any `__crcbl_web_fetch_*` or `__crcbl_web_opfs_*`. `1`, or `0` if it was already called. |
//! | [`__crcbl_options_log_level`] | `(i32) -> i32` | Set the log filter: `0` off … `5` trace. `1`/`0`. |
//! | [`__crcbl_options_boot`] | `() -> i32` | Open the shell on the canvas `__crcbl_web_canvas` announced, create the window, start the polled device request. `1`/`0`. |
//! | [`__crcbl_options_frame`] | `(f64) -> i32` | One `requestAnimationFrame`, given `performance.now()`. Returns the new status. |
//! | [`__crcbl_options_status`] | `() -> i32` | The status, without advancing anything. |
//! | [`__crcbl_options_shutdown`] | `() -> i32` | Tear the loop down. `1` if there was one. |
//! | [`__crcbl_options_error_ptr`] | `() -> i32` | Address of the last error message (UTF-8, not NUL-terminated), or `0`. |
//! | [`__crcbl_options_error_len`] | `() -> i32` | Its length in bytes. |
//! | [`__crcbl_options_log_take`] | `() -> i32` | Pop one log line into the scratch buffer and return its length; `0` when the queue is empty. |
//! | [`__crcbl_options_log_ptr`] | `() -> i32` | Address of that scratch buffer. Read it **after** `log_take`. |
//!
//! ## Status codes
//!
//! [`STATUS_IDLE`] `0`, [`STATUS_PREPARED`] `1`, [`STATUS_BOOTING`] `2`,
//! [`STATUS_RUNNING`] `3`, [`STATUS_STOPPED`] `4`, [`STATUS_FAILED`] `5`,
//! [`STATUS_PAUSED`] `6`.
//!
//! ## Call ordering
//!
//! ```text
//! __crcbl_options_prepare()                // storage backends exist
//!   → OPFS restore + ready (__crcbl_web_opfs_*)
//! __crcbl_web_canvas(id)                   // which canvas this instance drives
//! __crcbl_options_boot()                   // shell + window; no size yet
//! rAF loop, every frame:
//!   __crcbl_web_resize(id, w, h, dpr)      // from ResizeObserver, when it changes
//!   __crcbl_web_frame(performance.now())   // the shell's clock reference
//!   __crcbl_options_frame(performance.now())
//!   __crcbl_options_log_take() … while non-zero
//!   __crcbl_web_opfs_take() …              // drain queued saves
//! ```
//!
//! **The OPFS drain is not optional for this demo.** A save returns when it is
//! *queued*; the bytes reach the file when the shim takes them. A page that
//! never drains shows a player `SAVED` and keeps nothing.
//!
//! **The first `__crcbl_web_resize` is what starts the device request.** A
//! canvas has no size until the document gives it one, and a swapchain needs
//! one; a shim that never calls `resize` leaves the status at `BOOTING` forever.

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
// This sample's half of the lifecycle
// ---------------------------------------------------------------------------

// **There is no `WebLoop` impl here.** `crcbl::web` blanket-implements it for
// every `crcbl::engine::Loop`, and the two halves that were ever this sample's
// — its name and the log line a finished run is worth — are `HostedGame::NAME`
// and `HostedGame::log_summary` in `app.rs`.
//
// **`WebPending` is deliberately not imported.** The macro's guard against a
// missing inherent method resolves `PendingLoop::poll` by path, and an import
// would let that resolve to the trait method instead — which is the infinite
// recursion the guard exists to catch. See `crcbl::impl_web_pending`.
crcbl::impl_web_pending!(PendingLoop, Loop, Options, crate::app::OptionsError);

// ---------------------------------------------------------------------------
// Exports
// ---------------------------------------------------------------------------

crcbl::web_exports! {
    pending: PendingLoop<dyn crcbl::shell::Shell>,
    prepare: __crcbl_options_prepare,
    log_level: __crcbl_options_log_level,
    boot: __crcbl_options_boot,
    frame: __crcbl_options_frame,
    status: __crcbl_options_status,
    shutdown: __crcbl_options_shutdown,
    error_ptr: __crcbl_options_error_ptr,
    error_len: __crcbl_options_error_len,
    log_take: __crcbl_options_log_take,
    log_ptr: __crcbl_options_log_ptr,
}
