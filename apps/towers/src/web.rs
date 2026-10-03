//! The browser entry point: what the JS shim in `web/` calls.
//!
//! `apps/towers` is a `cdylib` on `wasm32-unknown-unknown`, and this module is
//! the only thing in it a browser can reach. Everything here is an `extern "C"`
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
//! What is left here is what is genuinely towers': the
//! [`WebPending`](crcbl::web::WebPending) impl, which opens the sample with its
//! own [`Options`]. The symbol names stay here too, written out one per line —
//! two demos can be open in one browser and the exports must not collide, so the
//! macro takes each name as an argument rather than building it from a prefix.
//!
//! # This page is the sample's web exit criterion, and nothing more than it
//!
//! `docs/plan/sample/07-towers.md` asks for a web build that "ships and is
//! single player, like every other sample's — same game over
//! `InMemoryTransport`, so the wasm target cannot rot". That is exactly what
//! this is: the page runs [`crate::game`]'s client and server in the one wasm
//! module, `PlaceTower` and `StartWave` are sealed into bytes and validated on
//! the way through, and a refused command is counted on the `[HUD]` line where
//! a browser gate can read it. **A browser cannot be the other half of a co-op
//! session** — `crcbl-net` ships no transport but the loopback, and that
//! document's milestone 3 is where a wire arrives.
//!
//! **What this sample does not add to the macro.** There is no `asset_source`
//! accessor here, because towers has nothing to read out of one: the map is the
//! committed `.scn/` directory compiled in by [`crate::scene`], the field is
//! `crcbl::greybox` primitives, and every byte it draws with — the geometry, the
//! materials, the shaders, the font atlas — is compiled into the module. The
//! OPFS store the `prepare` installs is the run's save (`crate::save`): `S`
//! and each wave's end write it, and with no lobby to offer *Continue* from,
//! the page opens on the saved run whenever there is one, as `apps/shard`'s
//! opens on its character. Both backends are installed there because the
//! shared shim's boot sequence drives both ABIs before it boots the demo and
//! both must answer.
//!
//! # The symbols this module exports
//!
//! `__crcbl_towers_` is this module's prefix. **What each of the ten lifecycle
//! symbols means, the four other ABI prefixes a page drives, the status codes
//! and the order a page calls them all in are [`crcbl::web`]'s module docs**,
//! written once rather than once a sample.
//!
//! [`__crcbl_towers_prepare`], [`__crcbl_towers_log_level`],
//! [`__crcbl_towers_boot`], [`__crcbl_towers_frame`],
//! [`__crcbl_towers_status`], [`__crcbl_towers_shutdown`],
//! [`__crcbl_towers_error_ptr`], [`__crcbl_towers_error_len`],
//! [`__crcbl_towers_log_take`], [`__crcbl_towers_log_ptr`].

use crate::app::{Loop, PendingLoop};
use crate::args::Options;

// ---------------------------------------------------------------------------
// This sample's half of the lifecycle
// ---------------------------------------------------------------------------

// **There is no `WebLoop` impl here.** `crcbl::web` blanket-implements it for
// every `crcbl::engine::Loop`, and the two halves that were ever this sample's
// — its name and the log line a finished run is worth — are `HostedGame::NAME`
// and `HostedGame::log_summary` in `app.rs`. What is left is start-up, and the
// macro below writes it.
//
// Towers' `Options` is the shared set and the committed map, so the browser
// wants it exactly as the binary's default builds it: a page has no directory
// for `--scene` to name.
//
// **`WebPending` is deliberately not imported.** The macro's guard against a
// missing inherent method resolves `PendingLoop::poll` by path, and an import
// would let that resolve to the trait method instead — which is the infinite
// recursion the guard exists to catch. See `crcbl::impl_web_pending`.
crcbl::impl_web_pending!(PendingLoop, Loop, Options, crate::app::TowersError);

// ---------------------------------------------------------------------------
// Exports
// ---------------------------------------------------------------------------

crcbl::web_exports! {
    pending: PendingLoop<dyn crcbl::shell::Shell>,
    prepare: __crcbl_towers_prepare,
    log_level: __crcbl_towers_log_level,
    boot: __crcbl_towers_boot,
    frame: __crcbl_towers_frame,
    status: __crcbl_towers_status,
    shutdown: __crcbl_towers_shutdown,
    error_ptr: __crcbl_towers_error_ptr,
    error_len: __crcbl_towers_error_len,
    log_take: __crcbl_towers_log_take,
    log_ptr: __crcbl_towers_log_ptr,
}
