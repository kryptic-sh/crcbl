//! The browser entry point: what the JS shim in `web/` calls.
//!
//! `apps/breach` is a `cdylib` on `wasm32-unknown-unknown`, and this module is
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
//! What is left here is what is genuinely breach's: the
//! [`WebPending`](crcbl::web::WebPending) impl, which opens the sample with its
//! own [`Options`]. The symbol names stay here too, written out one per line —
//! two demos can be open in one browser and the exports must not collide, so the
//! macro takes each name as an argument rather than building it from a prefix.
//!
//! # What the page shows, and what decides it
//!
//! `Options::default()` is the whole of the page's configuration: a page has no
//! argv, so the range, the tick rate and the pistol are the ones the binary
//! opens with. What a visitor sees is the range shooting itself within a second
//! of arriving, and their first step or trigger pull taking it over —
//! [`crate::game`] carries that argument.
//!
//! # This page is `docs/plan/sample/11-breach.md`'s milestone 0, and no more
//!
//! That doc's milestone 1 onward is a competitive shooter, and it says in as
//! many words that a browser build of it would be a claim the platform cannot
//! back: no anti-cheat, no unreliable channel, no measurable latency, and **no
//! raw mouse input**. The last of those is visible right here — the web shell
//! reports no `RAW_POINTER_MOTION` because `movementX`/`movementY` under
//! Pointer Lock are accelerated by the same OS layer the capability exists to
//! bypass — so this page is looked around with the arrow keys, and
//! [`crate::app`] says so where the binding is made.
//!
//! What milestone 0 *is* about is the fallback paths, and a browser is where
//! they are not hypothetical: there is no mesh stage and no ray query, so the
//! frame goes through `IndirectPerBatch` and `LightingPath::Rasterised` by
//! construction. [`crate::gpu::Paths`] is what names them, and the `[HUD]`
//! heartbeat is where a browser can read them.
//!
//! **What this sample does not add to the macro.** There is no `asset_source`
//! accessor here, because breach has nothing to read out of it: the range is
//! built in code, it keeps no score across runs and no save, and every byte it
//! draws with (the geometry, the shaders, the glyph atlas) is compiled into the
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
//! | `__crcbl_breach_` | this module | boot, one rAF frame, teardown, logs |
//!
//! ## Exports
//!
//! | Symbol | Signature (wasm) | Meaning |
//! | --- | --- | --- |
//! | [`__crcbl_breach_prepare`] | `() -> i32` | Install the log sink and the browser storage backends. **First call**, before any `__crcbl_web_fetch_*` or `__crcbl_web_opfs_*`. `1`, or `0` if it was already called. |
//! | [`__crcbl_breach_log_level`] | `(i32) -> i32` | Set the log filter: `0` off … `5` trace. `1`/`0`. |
//! | [`__crcbl_breach_boot`] | `() -> i32` | Open the shell on the canvas `__crcbl_web_canvas` announced, create the window, start the polled device request. `1`/`0`. |
//! | [`__crcbl_breach_frame`] | `(f64) -> i32` | One `requestAnimationFrame`, given `performance.now()`. Returns the new status. |
//! | [`__crcbl_breach_status`] | `() -> i32` | The status, without advancing anything. |
//! | [`__crcbl_breach_shutdown`] | `() -> i32` | Tear the loop down. `1` if there was one. |
//! | [`__crcbl_breach_error_ptr`] | `() -> i32` | Address of the last error message (UTF-8, not NUL-terminated), or `0`. |
//! | [`__crcbl_breach_error_len`] | `() -> i32` | Its length in bytes. |
//! | [`__crcbl_breach_log_take`] | `() -> i32` | Pop one log line into the scratch buffer and return its length; `0` when the queue is empty. |
//! | [`__crcbl_breach_log_ptr`] | `() -> i32` | Address of that scratch buffer. Read it **after** `log_take`. |
//!
//! ## Status codes
//!
//! [`STATUS_IDLE`] `0`, [`STATUS_PREPARED`] `1`, [`STATUS_BOOTING`] `2`,
//! [`STATUS_RUNNING`] `3`, [`STATUS_STOPPED`] `4`, [`STATUS_FAILED`] `5`,
//! [`STATUS_PAUSED`] `6`.
//!
//! The shim drives `requestAnimationFrame` while the status is `BOOTING`,
//! `RUNNING` **or** `PAUSED` and stops on anything else — a paused page is still
//! drawing, and it is a keystroke away from being played again. `FAILED` is the
//! only one that sets an error message.
//!
//! ## Call ordering
//!
//! ```text
//! __crcbl_breach_prepare()                  // storage backends exist
//!   → fetch pre-load       (__crcbl_web_fetch_*)
//!   → OPFS restore + ready (__crcbl_web_opfs_*)
//! __crcbl_web_canvas(id)                     // which canvas this instance drives
//! __crcbl_breach_boot()                     // shell + window; no size yet
//! rAF loop, every frame:
//!   __crcbl_web_resize(id, w, h, dpr)        // from ResizeObserver, when it changes
//!   __crcbl_web_frame(performance.now())     // the shell's clock reference
//!   __crcbl_breach_frame(performance.now()) // boot poll, or a frame
//!   __crcbl_breach_log_take() … while non-zero
//!   __crcbl_web_opfs_take() …                // drain queued saves
//! ```
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
// and `HostedGame::log_summary` in `app.rs`. What is left is start-up, and the
// macro below writes it.
//
// Breach's `Options` is the shared set and nothing else, so the browser wants
// it exactly as the binary's default builds it.
//
// **`WebPending` is deliberately not imported.** The macro's guard against a
// missing inherent method resolves `PendingLoop::poll` by path, and an import
// would let that resolve to the trait method instead — which is the infinite
// recursion the guard exists to catch. See `crcbl::impl_web_pending`.
crcbl::impl_web_pending!(PendingLoop, Loop, Options, crate::app::BreachError);

// ---------------------------------------------------------------------------
// Exports
// ---------------------------------------------------------------------------

crcbl::web_exports! {
    pending: PendingLoop<dyn crcbl::shell::Shell>,
    prepare: __crcbl_breach_prepare,
    log_level: __crcbl_breach_log_level,
    boot: __crcbl_breach_boot,
    frame: __crcbl_breach_frame,
    status: __crcbl_breach_status,
    shutdown: __crcbl_breach_shutdown,
    error_ptr: __crcbl_breach_error_ptr,
    error_len: __crcbl_breach_error_len,
    log_take: __crcbl_breach_log_take,
    log_ptr: __crcbl_breach_log_ptr,
}
