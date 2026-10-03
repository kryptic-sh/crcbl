//! The browser entry point: what the JS shim in `web/` calls.
//!
//! `apps/hud` is a `cdylib` on `wasm32-unknown-unknown`, and this module is the
//! only thing in it a browser can reach. Everything here is an `extern "C"`
//! export with `#[unsafe(no_mangle)]`; there are **no imports**.
//!
//! # Only the symbol names are this sample's
//!
//! The state machine behind these exports, the log queue and the five-call
//! protocol are [`crcbl::web`], and [`crcbl::web_exports!`] writes the ten
//! symbols listed below. That module is also where the reasons live: why
//! start-up is polled rather than blocking, why the clock is the browser's, and
//! why a sample's wasm module imports nothing of its own.
//!
//! What is left here is what is genuinely hud's: the [`WebPending`](crcbl::web::WebPending) impl, which
//! opens the sample with its own [`Options`]. The symbol names stay here too,
//! written out one per line — two demos can be open in one browser and the
//! exports must not collide, so the macro takes each name as an argument rather
//! than building it from a prefix.
//!
//! **What this sample does not add to that.** There is no `asset_source`
//! accessor here, because hud has nothing to read out of it:
//! it keeps no high score — there is no score — and every byte it draws with
//! (the glyph atlas, the shaders, the stylesheet) is compiled into the module. The two backends
//! are still installed by the macro's `prepare`, because the shared shim's boot
//! sequence drives both ABIs before it boots the demo and both must answer.
//!
//! # The symbols this module exports
//!
//! `__crcbl_hud_` is this module's prefix. **What each of the ten lifecycle
//! symbols means, the four other ABI prefixes a page drives, the status codes
//! and the order a page calls them all in are [`crcbl::web`]'s module docs**,
//! written once rather than once a sample.
//!
//! [`__crcbl_hud_prepare`], [`__crcbl_hud_log_level`], [`__crcbl_hud_boot`],
//! [`__crcbl_hud_frame`], [`__crcbl_hud_status`], [`__crcbl_hud_shutdown`],
//! [`__crcbl_hud_error_ptr`], [`__crcbl_hud_error_len`],
//! [`__crcbl_hud_log_take`], [`__crcbl_hud_log_ptr`].

use crate::app::{Loop, PendingLoop};
use crate::args::Options;

// ---------------------------------------------------------------------------
// This sample's half of the lifecycle
// ---------------------------------------------------------------------------

// **There is no `WebLoop` impl here.** `crcbl::web` blanket-implements it for
// every `crcbl::engine::Loop`, and the two halves that were ever this sample's
// — its name and the log line a finished run is worth — are `HostedGame::NAME`
// and `HostedGame::log_summary` in `app.rs`. What is left is start-up, and the
// macro below writes it: the impl was identical in all six samples once the
// sample's name was normalised away.
//
// **`WebPending` is deliberately not imported.** The macro's guard against a
// missing inherent method resolves `PendingLoop::poll` by path, and an import
// would let that resolve to the trait method instead — which is the infinite
// recursion the guard exists to catch. See `crcbl::impl_web_pending`.
crcbl::impl_web_pending!(PendingLoop, Loop, Options, crate::app::HudError);

// ---------------------------------------------------------------------------
// Exports
// ---------------------------------------------------------------------------

crcbl::web_exports! {
    pending: PendingLoop<dyn crcbl::shell::Shell>,
    prepare: __crcbl_hud_prepare,
    log_level: __crcbl_hud_log_level,
    boot: __crcbl_hud_boot,
    frame: __crcbl_hud_frame,
    status: __crcbl_hud_status,
    shutdown: __crcbl_hud_shutdown,
    error_ptr: __crcbl_hud_error_ptr,
    error_len: __crcbl_hud_error_len,
    log_take: __crcbl_hud_log_take,
    log_ptr: __crcbl_hud_log_ptr,
}
