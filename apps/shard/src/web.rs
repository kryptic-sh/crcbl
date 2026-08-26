//! The browser entry point: what the JS shim in `web/` calls.
//!
//! `apps/shard` is a `cdylib` on `wasm32-unknown-unknown`, and this module is the
//! only thing in it a browser can reach. Everything here is an `extern "C"`
//! export with `#[unsafe(no_mangle)]`; there are **no imports**.
//!
//! # Only the symbol names are this sample's
//!
//! The state machine behind these exports, the log queue and the five-call
//! protocol are [`crcbl::web`], and [`crcbl::web_exports!`] writes the ten symbols
//! in the table below. That module is also where the reasons live: why start-up is
//! polled rather than blocking, why the clock is the browser's, and why a sample's
//! wasm module imports nothing of its own.
//!
//! What is left here is what is genuinely shard's: the
//! [`WebPending`](crcbl::web::WebPending) impl, which opens the sample with its
//! own [`Options`]. The symbol names stay here too, written out one per line — two
//! demos can be open in one browser and the exports must not collide, so the macro
//! takes each name as an argument rather than building it from a prefix.
//!
//! # This page is `docs/plan/sample/15-shard.md`'s milestone 1, four verbs in
//!
//! That doc's milestone 1 is "a complete play session — explore, fight, loot,
//! level, save, resume — in a browser, from the same build that runs natively".
//! **Four of those six are here.** A visitor walks a torch-lit zone, puts its
//! torches out, fights what is standing in it, and comes back to a character
//! where they left them: the save goes into the Origin Private File System
//! through [`crate::save`], on the same build that writes it to the platform
//! data directory natively. There is no item and no level, and
//! `docs/backlog.md` carries the rest with what each would take.
//!
//! # And it is the sample the fallback paths were built for
//!
//! `docs/plan/sample/15-shard.md` says why the web slice comes before the native
//! world: "the fallback paths are what every browser visitor and every Apple
//! machine runs, and a fallback proven after the fact is a fallback nobody
//! proved." A browser has no ray query, no mesh stage and no bindless, so the
//! frame goes through `IndirectPerBatch`, `ArrayPages` and
//! `LightingPath::Rasterised` **by construction** — with real content on top of
//! them: a zone of modular tiles, a torch over every brazier, shadow
//! tiles, screen-space occlusion and reflections, and a baked irradiance volume.
//! [`crate::gpu::Paths`] is what names them, and the `[HUD]` heartbeat is where a
//! browser can read them.
//!
//! **What this sample does not add to the macro.** There is no `asset_source`
//! accessor here, because shard has nothing to read out of one: the zone is built
//! in code and every byte it draws with (the geometry, the shaders, the glyph
//! atlas, the probe volume) is compiled into the module. The **OPFS** half is a
//! different matter and needs no accessor either — [`crate::save`] reaches the
//! store the macro's `prepare` installed through
//! [`crcbl::store::web::opfs::installed`], which is the same handle
//! [`crcbl::store::record::Backing::platform`] uses for a high score. Both
//! backends are installed by `prepare` because the shared shim's boot sequence
//! drives both ABIs before it boots the demo and both must answer.
//!
//! # The ABIs a page has to drive
//!
//! | Prefix | Owner | What it is |
//! | --- | --- | --- |
//! | `__crcbl_web_` (input/frame) | [`crcbl::shell`]'s `web` backend | canvas size, focus, keys, pointer |
//! | `__crcbl_web_audio_` | [`crcbl::audio::web`] | the AudioWorklet pull |
//! | `__crcbl_web_fetch_` | [`crcbl::store::web::fetch`] | assets over `fetch()` |
//! | `__crcbl_web_opfs_` | [`crcbl::store::web::opfs`] | saves in the Origin Private File System |
//! | `__crcbl_shard_` | this module | boot, one rAF frame, teardown, logs |
//!
//! ## Exports
//!
//! | Symbol | Signature (wasm) | Meaning |
//! | --- | --- | --- |
//! | [`__crcbl_shard_prepare`] | `() -> i32` | Install the log sink and the browser storage backends. **First call**, before any `__crcbl_web_fetch_*` or `__crcbl_web_opfs_*`. `1`, or `0` if it was already called. |
//! | [`__crcbl_shard_log_level`] | `(i32) -> i32` | Set the log filter: `0` off … `5` trace. `1`/`0`. |
//! | [`__crcbl_shard_boot`] | `() -> i32` | Open the shell on the canvas `__crcbl_web_canvas` announced, create the window, start the polled device request. `1`/`0`. |
//! | [`__crcbl_shard_frame`] | `(f64) -> i32` | One `requestAnimationFrame`, given `performance.now()`. Returns the new status. |
//! | [`__crcbl_shard_status`] | `() -> i32` | The status, without advancing anything. |
//! | [`__crcbl_shard_shutdown`] | `() -> i32` | Tear the loop down. `1` if there was one. |
//! | [`__crcbl_shard_error_ptr`] | `() -> i32` | Address of the last error message (UTF-8, not NUL-terminated), or `0`. |
//! | [`__crcbl_shard_error_len`] | `() -> i32` | Its length in bytes. |
//! | [`__crcbl_shard_log_take`] | `() -> i32` | Pop one log line into the scratch buffer and return its length; `0` when the queue is empty. |
//! | [`__crcbl_shard_log_ptr`] | `() -> i32` | Address of that scratch buffer. Read it **after** `log_take`. |
//!
//! ## Status codes
//!
//! [`STATUS_IDLE`] `0`, [`STATUS_PREPARED`] `1`, [`STATUS_BOOTING`] `2`,
//! [`STATUS_RUNNING`] `3`, [`STATUS_STOPPED`] `4`, [`STATUS_FAILED`] `5`,
//! [`STATUS_PAUSED`] `6`.
//!
//! The shim drives `requestAnimationFrame` while the status is `BOOTING`,
//! `RUNNING` **or** `PAUSED` and stops on anything else — a paused page is still
//! drawing, and here that matters more than usual: `L` still works on a paused
//! frame, so a visitor can stop the zone and switch its lighting while looking at
//! one still picture of it. `FAILED` is the only one that sets an error message.
//!
//! ## Call ordering
//!
//! ```text
//! __crcbl_shard_prepare()                   // storage backends exist
//!   → fetch pre-load       (__crcbl_web_fetch_*)
//!   → OPFS restore + ready (__crcbl_web_opfs_*)
//! __crcbl_web_canvas(id)                     // which canvas this instance drives
//! __crcbl_shard_boot()                      // shell + window; no size yet
//! rAF loop, every frame:
//!   __crcbl_web_resize(id, w, h, dpr)        // from ResizeObserver, when it changes
//!   __crcbl_web_frame(performance.now())     // the shell's clock reference
//!   __crcbl_shard_frame(performance.now())  // boot poll, or a frame
//!   __crcbl_shard_log_take() … while non-zero
//!   __crcbl_web_opfs_take() …                // drain queued saves
//! ```
//!
//! **The first `__crcbl_web_resize` is what starts the device request.** A canvas
//! has no size until the document gives it one, and a swapchain needs one; a shim
//! that never calls `resize` leaves the status at `BOOTING` forever.

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
// every `crcbl::engine::Loop`, and the two halves that were ever this sample's —
// its name and the log line a finished run is worth — are `HostedGame::NAME` and
// `HostedGame::log_summary` in `app.rs`. What is left is start-up, and the macro
// below writes it.
//
// Shard's `Options` is the shared set and nothing else, so the browser wants it
// exactly as the binary's default builds it.
//
// **`WebPending` is deliberately not imported.** The macro's guard against a
// missing inherent method resolves `PendingLoop::poll` by path, and an import
// would let that resolve to the trait method instead — which is the infinite
// recursion the guard exists to catch. See `crcbl::impl_web_pending`.
crcbl::impl_web_pending!(PendingLoop, Loop, Options, crate::app::ShardError);

// ---------------------------------------------------------------------------
// Exports
// ---------------------------------------------------------------------------

crcbl::web_exports! {
    pending: PendingLoop<dyn crcbl::shell::Shell>,
    prepare: __crcbl_shard_prepare,
    log_level: __crcbl_shard_log_level,
    boot: __crcbl_shard_boot,
    frame: __crcbl_shard_frame,
    status: __crcbl_shard_status,
    shutdown: __crcbl_shard_shutdown,
    error_ptr: __crcbl_shard_error_ptr,
    error_len: __crcbl_shard_error_len,
    log_take: __crcbl_shard_log_take,
    log_ptr: __crcbl_shard_log_ptr,
}
