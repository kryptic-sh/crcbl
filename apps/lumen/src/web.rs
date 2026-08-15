//! The browser entry point: what the JS shim in `web/` calls.
//!
//! `apps/lumen` is a `cdylib` on `wasm32-unknown-unknown`, and this module is
//! the only thing in it a browser can reach. Everything here is an `extern "C"`
//! export with `#[unsafe(no_mangle)]`; there are **no imports**.
//!
//! # Only the symbol names are this sample's
//!
//! The state machine behind these exports, the log queue and the five-call
//! protocol are [`crcbl::web`], and [`crcbl::web_exports!`] writes the ten
//! symbols in the table below. That module is also where the reasons live: why
//! start-up is polled rather than blocking, why the clock is the browser's, and
//! why a sample's wasm module imports nothing of its own.
//!
//! What is left here is what is genuinely lumen's: the [`WebPending`] impl,
//! which opens the fixture with its own [`Options`]. The symbol names stay here
//! too, written out one per line — two demos can be open in one browser and the
//! exports must not collide, so the macro takes each name as an argument rather
//! than building it from a prefix.
//!
//! # What the page shows, and what decides it
//!
//! **The browser draws the room through [`crcbl::hal::LightingPath::Rasterised`]
//! by construction.** WebGPU exposes no ray query, so the selector cannot
//! resolve to anything else here — shadow maps, screen-space reflections and the
//! probe volume [`crate::bounce`] bakes are the whole of the lighting, and the
//! debug panel's `unbuilt` section says so on screen. That is why this sample is
//! worth publishing: it is the one place the rasterised path can be looked at
//! without building anything.
//!
//! `Options::default()` is also what fixes the rest of the page: the fixed
//! camera the goldens are taken from, and every effect asked for. A page has no
//! argv, so `--force-*` and the `--no-*` flags have no source here — see this
//! sample's `crcbl::engine::PolledGpu` impl, which says the same thing about the
//! device request.
//!
//! **What this sample does not add to the macro.** There is no `asset_source`
//! accessor here, because lumen has nothing to read out of it:
//! it keeps no score — there is no score — and every byte it draws with
//! (the room's geometry, the shaders, the glyph atlas) is compiled into the
//! module. The two backends are still installed by the macro's `prepare`,
//! because the shared shim's boot sequence drives both ABIs before it boots the
//! demo and both must answer.
//!
//! # The ABIs a page has to drive
//!
//! | Prefix | Owner | What it is |
//! | --- | --- | --- |
//! | `__crcbl_web_` (input/frame) | [`crcbl::shell`]'s `web` backend | canvas size, focus, keys, pointer |
//! | `__crcbl_web_audio_` | [`crcbl::audio::web`] | the AudioWorklet pull |
//! | `__crcbl_web_fetch_` | [`crcbl::store::web::fetch`] | assets over `fetch()` |
//! | `__crcbl_web_opfs_` | [`crcbl::store::web::opfs`] | saves in the Origin Private File System |
//! | `__crcbl_lumen_` | this module | boot, one rAF frame, teardown, logs |
//!
//! ## Exports
//!
//! | Symbol | Signature (wasm) | Meaning |
//! | --- | --- | --- |
//! | [`__crcbl_lumen_prepare`] | `() -> i32` | Install the log sink and the browser storage backends. **First call**, before any `__crcbl_web_fetch_*` or `__crcbl_web_opfs_*`. `1`, or `0` if it was already called. |
//! | [`__crcbl_lumen_log_level`] | `(i32) -> i32` | Set the log filter: `0` off … `5` trace. `1`/`0`. |
//! | [`__crcbl_lumen_boot`] | `() -> i32` | Open the shell on the canvas `__crcbl_web_canvas` announced, create the window, start the polled device request. `1`/`0`. |
//! | [`__crcbl_lumen_frame`] | `(f64) -> i32` | One `requestAnimationFrame`, given `performance.now()`. Returns the new status. |
//! | [`__crcbl_lumen_status`] | `() -> i32` | The status, without advancing anything. |
//! | [`__crcbl_lumen_shutdown`] | `() -> i32` | Tear the loop down. `1` if there was one. |
//! | [`__crcbl_lumen_error_ptr`] | `() -> i32` | Address of the last error message (UTF-8, not NUL-terminated), or `0`. |
//! | [`__crcbl_lumen_error_len`] | `() -> i32` | Its length in bytes. |
//! | [`__crcbl_lumen_log_take`] | `() -> i32` | Pop one log line into the scratch buffer and return its length; `0` when the queue is empty. |
//! | [`__crcbl_lumen_log_ptr`] | `() -> i32` | Address of that scratch buffer. Read it **after** `log_take`. |
//!
//! ## Status codes
//!
//! [`STATUS_IDLE`] `0`, [`STATUS_PREPARED`] `1`, [`STATUS_BOOTING`] `2`,
//! [`STATUS_RUNNING`] `3`, [`STATUS_STOPPED`] `4`, [`STATUS_FAILED`] `5`,
//! [`STATUS_PAUSED`] `6`.
//!
//! The shim drives `requestAnimationFrame` while the status is `BOOTING`,
//! `RUNNING` **or** `PAUSED` and stops on anything else — a paused page is still
//! drawing, and it is a keystroke away from ticking again. `FAILED` is the only
//! one that sets an error message.
//!
//! ## Call ordering
//!
//! ```text
//! __crcbl_lumen_prepare()                  // storage backends exist
//!   → fetch pre-load       (__crcbl_web_fetch_*)
//!   → OPFS restore + ready (__crcbl_web_opfs_*)
//! __crcbl_web_canvas(id)                   // which canvas this instance drives
//! __crcbl_lumen_boot()                     // shell + window; no size yet
//! rAF loop, every frame:
//!   __crcbl_web_resize(id, w, h, dpr)      // from ResizeObserver, when it changes
//!   __crcbl_web_frame(performance.now())   // the shell's clock reference
//!   __crcbl_lumen_frame(performance.now()) // boot poll, or a frame
//!   __crcbl_lumen_log_take() … while non-zero
//!   __crcbl_web_opfs_take() …              // drain queued saves
//! ```
//!
//! **The first `__crcbl_web_resize` is what starts the device request.** A
//! canvas has no size until the document gives it one, and a swapchain needs
//! one; a shim that never calls `resize` leaves the status at `BOOTING` forever.

use crcbl::engine::Clock;
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
// This sample's half of the lifecycle
// ---------------------------------------------------------------------------

/// **There is no `WebLoop` impl here.** `crcbl::web` blanket-implements it for
/// every `crcbl::engine::Loop`, and the two halves that were ever this sample's
/// — its name and the log line a finished run is worth — are `HostedGame::NAME`
/// and `HostedGame::log_summary` in `app.rs`. What is left below is start-up,
/// which stays here because the options it opens with are lumen's.
impl WebPending for PendingLoop<dyn crcbl::shell::Shell> {
    type Loop = Loop<dyn crcbl::shell::Shell>;

    fn request(
        shell: Box<dyn crcbl::shell::Shell>,
        clock: Clock,
    ) -> Result<Self, crate::app::LumenError> {
        Self::request(shell, &Options::default(), clock)
    }

    fn poll(&mut self) -> Result<Option<Self::Loop>, crate::app::LumenError> {
        Self::poll(self)
    }
}

// ---------------------------------------------------------------------------
// Exports
// ---------------------------------------------------------------------------

crcbl::web_exports! {
    pending: PendingLoop<dyn crcbl::shell::Shell>,
    prepare: __crcbl_lumen_prepare,
    log_level: __crcbl_lumen_log_level,
    boot: __crcbl_lumen_boot,
    frame: __crcbl_lumen_frame,
    status: __crcbl_lumen_status,
    shutdown: __crcbl_lumen_shutdown,
    error_ptr: __crcbl_lumen_error_ptr,
    error_len: __crcbl_lumen_error_len,
    log_take: __crcbl_lumen_log_take,
    log_ptr: __crcbl_lumen_log_ptr,
}
