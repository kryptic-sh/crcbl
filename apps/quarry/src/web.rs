//! The browser entry point: what the JS shim in `web/` calls.
//!
//! `apps/quarry` is a `cdylib` on `wasm32-unknown-unknown`, and this module is
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
//! What is left here is what is genuinely quarry's: the
//! [`WebPending`](crcbl::web::WebPending) impl, and the options a page opens
//! with. The symbol names stay here too, written out one per line — two demos
//! can be open in one browser and the exports must not collide, so the macro
//! takes each name as an argument rather than building it from a prefix.
//!
//! # What the page shows, and what decides it
//!
//! **The browser draws the face through
//! [`crcbl::hal::GeometryPath::IndirectPerBatch`] by construction.** WebGPU
//! exposes neither a mesh stage nor a GPU-side draw count, so the selector
//! cannot resolve to either better arm here: the cut is chosen once per
//! *instance* rather than once per cluster, and one flat level is drawn for the
//! whole face. That is the honest picture of what a browser visitor gets, which
//! is exactly what `docs/plan/sample/14-quarry.md`'s Scope asks this demo to be
//! — and it is the same shape of claim `apps/lumen`'s page makes about
//! [`crcbl::hal::LightingPath::Rasterised`]. `Quarry::log_heartbeat` names the
//! arm it resolved to once a second, so the claim is checkable from the console
//! rather than taken on trust.
//!
//! # A page has no argv, so it opens on [`PageOptions`]
//!
//! Every other setting is [`Options`]'s own default — the device's own paths,
//! the renderer's own error budget, the shaded picture rather than the LOD tint
//! — because `--force-*`, `--lod-budget` and `--lod-view` have no source in a
//! browser. This sample's `crcbl::engine::PolledGpu` impl says the same thing
//! about the device request.
//!
//! The camera is the one exception, and it is the reason there is a type here at
//! all. [`Options::default`] is [`CameraMode::Fixed`] — the pose `tests/golden/`
//! was blessed from, held still — and it is the right default for a run whose
//! screenshot is going to be compared against a checked-in reference. **It is
//! the wrong thing to publish.** A held frame shows a cut and says nothing about
//! where the cut came from; the sample's whole claim is that detail arrives as
//! the camera closes *without the boundary popping*, and nobody can see an
//! absence of popping in a still picture. So the page opens on
//! [`CameraMode::Dolly`], which is that run with a window in front of it.
//!
//! # The ABIs a page has to drive
//!
//! | Prefix | Owner | What it is |
//! | --- | --- | --- |
//! | `__crcbl_web_` (input/frame) | [`crcbl::shell`]'s `web` backend | canvas size, focus, keys, pointer |
//! | `__crcbl_web_audio_` | [`crcbl::audio::web`] | the AudioWorklet pull |
//! | `__crcbl_web_fetch_` | [`crcbl::store::web::fetch`] | assets over `fetch()` |
//! | `__crcbl_web_opfs_` | [`crcbl::store::web::opfs`] | saves in the Origin Private File System |
//! | `__crcbl_quarry_` | this module | boot, one rAF frame, teardown, logs |
//!
//! ## Exports
//!
//! | Symbol | Signature (wasm) | Meaning |
//! | --- | --- | --- |
//! | [`__crcbl_quarry_prepare`] | `() -> i32` | Install the log sink and the browser storage backends. **First call**, before any `__crcbl_web_fetch_*` or `__crcbl_web_opfs_*`. `1`, or `0` if it was already called. |
//! | [`__crcbl_quarry_log_level`] | `(i32) -> i32` | Set the log filter: `0` off … `5` trace. `1`/`0`. |
//! | [`__crcbl_quarry_boot`] | `() -> i32` | Open the shell on the canvas `__crcbl_web_canvas` announced, create the window, start the polled device request. `1`/`0`. |
//! | [`__crcbl_quarry_frame`] | `(f64) -> i32` | One `requestAnimationFrame`, given `performance.now()`. Returns the new status. |
//! | [`__crcbl_quarry_status`] | `() -> i32` | The status, without advancing anything. |
//! | [`__crcbl_quarry_shutdown`] | `() -> i32` | Tear the loop down. `1` if there was one. |
//! | [`__crcbl_quarry_error_ptr`] | `() -> i32` | Address of the last error message (UTF-8, not NUL-terminated), or `0`. |
//! | [`__crcbl_quarry_error_len`] | `() -> i32` | Its length in bytes. |
//! | [`__crcbl_quarry_log_take`] | `() -> i32` | Pop one log line into the scratch buffer and return its length; `0` when the queue is empty. |
//! | [`__crcbl_quarry_log_ptr`] | `() -> i32` | Address of that scratch buffer. Read it **after** `log_take`. |
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
//! __crcbl_quarry_prepare()                  // storage backends exist
//!   → fetch pre-load       (__crcbl_web_fetch_*)
//!   → OPFS restore + ready (__crcbl_web_opfs_*)
//! __crcbl_web_canvas(id)                    // which canvas this instance drives
//! __crcbl_quarry_boot()                     // shell + window; no size yet
//! rAF loop, every frame:
//!   __crcbl_web_resize(id, w, h, dpr)       // from ResizeObserver, when it changes
//!   __crcbl_web_frame(performance.now())    // the shell's clock reference
//!   __crcbl_quarry_frame(performance.now()) // boot poll, or a frame
//!   __crcbl_quarry_log_take() … while non-zero
//!   __crcbl_web_opfs_take() …               // drain queued saves
//! ```
//!
//! **The first `__crcbl_web_resize` is what starts the device request.** A
//! canvas has no size until the document gives it one, and a swapchain needs
//! one; a shim that never calls `resize` leaves the status at `BOOTING` forever.

use crcbl::engine::Clock;
use crcbl::shell::Shell;

use crate::app::{Loop, QuarryError};
use crate::args::Options;
use crate::menu::CameraMode;

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

/// The [`Options`] a page opens with: [`Options::default`], on the dolly.
///
/// A type of its own rather than a call to a constructor, because
/// [`crcbl::impl_web_pending!`] reaches the sample's settings through
/// [`Default`] and nothing else — a page has no argv to read them from. The five
/// other samples' pages want their `Options::default()` verbatim and hand the
/// macro that type directly; this one wants a single field changed, and the
/// module docs say why.
///
/// The wrapper is what keeps `Options::default()` meaning "a bare `quarry`
/// invocation" on every other target. Making it mean the dolly instead would
/// change what `quarry` with no arguments draws, and with it the pose every
/// golden in `tests/golden/` was blessed from.
#[derive(Clone, Debug, PartialEq)]
pub struct PageOptions(Options);

impl Default for PageOptions {
    fn default() -> Self {
        Self(Options {
            camera: CameraMode::Dolly,
            ..Options::default()
        })
    }
}

/// [`crate::PendingLoop`], started from [`PageOptions`].
///
/// The whole of the difference between a page's start-up and a window's is which
/// options `request` is handed, so this forwards both halves and holds nothing
/// of its own. It exists because the macro's guard pins the inherent `request`
/// to `&PageOptions` — see [`PageOptions`] for why that is the type.
#[derive(Debug)]
pub struct PendingPage<S: Shell + ?Sized = dyn Shell>(crate::app::PendingLoop<S>);

impl<S: Shell + ?Sized> PendingPage<S> {
    /// Creates the window and starts the wait, without blocking on either half.
    ///
    /// # Errors
    ///
    /// [`QuarryError`] if the shell refused the window.
    pub fn request(
        shell: Box<S>,
        options: &PageOptions,
        clock: Clock,
    ) -> Result<Self, QuarryError> {
        Ok(Self(crate::app::PendingLoop::request(
            shell, &options.0, clock,
        )?))
    }

    /// Advances start-up. `Ok(None)` means "not yet, poll again next frame".
    ///
    /// # Errors
    ///
    /// [`QuarryError`] if the window went away before it had a size, or if the
    /// device request failed.
    pub fn poll(&mut self) -> Result<Option<Loop<S>>, QuarryError> {
        self.0.poll()
    }
}

// **There is no `WebLoop` impl here.** `crcbl::web` blanket-implements it for
// every `crcbl::engine::Loop`, and the two halves that were ever this sample's
// — its name and the log line a finished run is worth — are `HostedGame::NAME`
// and `HostedGame::log_summary` in `app.rs`. What is left below is start-up,
// which stays here because the options it opens with are the page's.
// **`WebPending` is deliberately not imported.** The macro's guard against a
// missing inherent method resolves `PendingPage::poll` by path, and an import
// would let that resolve to the trait method instead — which is the infinite
// recursion the guard exists to catch. See `crcbl::impl_web_pending`.
crcbl::impl_web_pending!(PendingPage, Loop, PageOptions, crate::app::QuarryError);

// ---------------------------------------------------------------------------
// Exports
// ---------------------------------------------------------------------------

crcbl::web_exports! {
    pending: PendingPage<dyn crcbl::shell::Shell>,
    prepare: __crcbl_quarry_prepare,
    log_level: __crcbl_quarry_log_level,
    boot: __crcbl_quarry_boot,
    frame: __crcbl_quarry_frame,
    status: __crcbl_quarry_status,
    shutdown: __crcbl_quarry_shutdown,
    error_ptr: __crcbl_quarry_error_ptr,
    error_len: __crcbl_quarry_error_len,
    log_take: __crcbl_quarry_log_take,
    log_ptr: __crcbl_quarry_log_ptr,
}
