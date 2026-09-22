//! `crcbl-steam` — Steamworks for the engine, over the SDK's flat C API.
//!
//! ```text
//! Steam::init(AppId) ──▶ once per frame: pump() ──▶ events() ──▶ act
//!        │
//!        └── user().steam_id(), utils().app_id(), utils().steam_hardware()
//! ```
//!
//! `docs/plan/42-steam.md` is the design; this crate is its slices as they
//! land. What exists now is slice 1: the library is found and opened at
//! runtime, Steam is initialised with a version handshake, the callback pipe is
//! drained by manual dispatch into a queue of `SteamEvent`s, the local
//! `SteamId` is read, and the API is shut down exactly once, when the last
//! owner of it is gone.
//!
//! # No SDK in the repository, no link-time dependency
//!
//! Valve's licence lets a game redistribute `redistributable_bin` beside its
//! executable and nothing else, so no header, no `steam_api.json` and no
//! library is committed. The function prototypes and struct layouts here are
//! this crate's own declarations of the C ABI, each carrying the C declaration
//! it was copied from (`ffi::manifest`). The library is opened by absolute
//! path when `Steam::init` runs — beside the executable first, then under
//! `$CRCBL_STEAM_SDK/redistributable_bin/<platform>/` for development — so a
//! machine without Steam gets an ordinary `Err` and the game runs on, and the
//! crate builds and tests on a CI that has never seen the SDK.
//!
//! # Where it compiles
//!
//! 64-bit Linux, Windows and macOS. Everywhere else — `wasm32`, Android, any
//! 32-bit target — the crate is this documentation and no items, so nothing
//! above it ever asks `cfg(target_os)` about Steam. Items that exist on only
//! some targets are named in backticks rather than linked, because `cargo doc`
//! is a `-D warnings` gate on the targets where they do not exist.
//!
//! # What is checked where
//!
//! The pump, init and loader run in CI against a fake library — a struct of
//! Rust function pointers with the exact types the real one is called through
//! (`testing`, test builds only). The struct layouts are asserted per
//! operating system, because Valve packs callback structs to 4 bytes on Linux
//! and macOS and 8 on Windows. What no CI job can check — that these
//! declarations match a real SDK, and that a real client accepts them — is the
//! `#[ignore]`d drift gate (`ffi::drift`) and `tests/smoke.rs`, run by hand
//! with the SDK and a Steam client present.

#![warn(missing_docs)]

/// Applies the one target gate to every item it wraps: 64-bit Linux, Windows
/// and macOS, the targets Valve ships a 64-bit `steam_api` for.
macro_rules! supported {
    ($($item:item)*) => {
        $(
            #[cfg(all(
                target_pointer_width = "64",
                any(target_os = "linux", target_os = "windows", target_os = "macos")
            ))]
            $item
        )*
    };
}

supported! {
    mod callbacks;
    mod client;
    mod error;
    mod ffi;
    mod pump;
    #[cfg(test)]
    mod testing;
    mod user;
    mod utils;

    pub use crate::{
        callbacks::SteamEvent,
        client::{AppId, Steam},
        error::InitError,
        pump::PumpDiagnostics,
        user::{SteamId, User},
        utils::{SteamHardware, Utils},
    };
}
