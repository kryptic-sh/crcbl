//! `crcbl-steam` — Steamworks for the engine, over the SDK's flat C API.
//!
//! ```text
//! Steam::relaunch_via_steam(AppId)?  ── true: quit, Steam relaunches the game
//! Steam::init(AppId) ──▶ once per frame: pump() ──▶ events() ──▶ act
//!        │
//!        ├── user(): steam_id(), logged_on(), steam_level()
//!        ├── friends(): persona_name(), set_rich_presence(), open_invite_dialog()
//!        ├── apps(): subscribed(), game_language(), launch_command_line()
//!        ├── utils(): app_id(), steam_hardware(), overlay_enabled(), …
//!        └── matchmaking(): create_lobby() / join_lobby() ──▶ SteamCall<T>
//!                                  └──▶ a later frame: steam.take(call) ──▶ Lobby
//! ```
//!
//! `docs/plan/42-steam.md` is the design; this crate is its slices as they
//! land. What exists now is slices 1, 1b and 3a: the library is found and
//! opened at runtime, Steam is initialised with a version handshake, the
//! callback pipe is drained by manual dispatch into a queue of `SteamEvent`s,
//! the local player's identity and the machine's basics are read,
//! asynchronous calls are typed tokens redeemed after the pump, lobbies are
//! created, joined, invited to and left, and the API is shut down exactly
//! once, when the last owner of it is gone. Every string Steam returns is
//! copied before the call that got it returns.
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

// The one target gate — 64-bit Linux, Windows and macOS, the targets Valve
// ships a 64-bit `steam_api` for — written on each item, as `crcbl-dx12`
// writes its own. Not a macro wrapping the list: rustfmt does not look inside
// macro invocations, so a module declared in one is never formatted.
#[cfg(all(
    target_pointer_width = "64",
    any(target_os = "linux", target_os = "windows", target_os = "macos")
))]
mod apps;
#[cfg(all(
    target_pointer_width = "64",
    any(target_os = "linux", target_os = "windows", target_os = "macos")
))]
mod call;
#[cfg(all(
    target_pointer_width = "64",
    any(target_os = "linux", target_os = "windows", target_os = "macos")
))]
mod callbacks;
#[cfg(all(
    target_pointer_width = "64",
    any(target_os = "linux", target_os = "windows", target_os = "macos")
))]
mod client;
#[cfg(all(
    target_pointer_width = "64",
    any(target_os = "linux", target_os = "windows", target_os = "macos")
))]
mod error;
#[cfg(all(
    target_pointer_width = "64",
    any(target_os = "linux", target_os = "windows", target_os = "macos")
))]
mod ffi;
#[cfg(all(
    target_pointer_width = "64",
    any(target_os = "linux", target_os = "windows", target_os = "macos")
))]
mod friends;
#[cfg(all(
    target_pointer_width = "64",
    any(target_os = "linux", target_os = "windows", target_os = "macos")
))]
mod matchmaking;
#[cfg(all(
    target_pointer_width = "64",
    any(target_os = "linux", target_os = "windows", target_os = "macos")
))]
mod presence;
#[cfg(all(
    target_pointer_width = "64",
    any(target_os = "linux", target_os = "windows", target_os = "macos")
))]
mod pump;
#[cfg(all(
    target_pointer_width = "64",
    any(target_os = "linux", target_os = "windows", target_os = "macos")
))]
mod strings;
#[cfg(all(
    test,
    target_pointer_width = "64",
    any(target_os = "linux", target_os = "windows", target_os = "macos")
))]
mod testing;
#[cfg(all(
    target_pointer_width = "64",
    any(target_os = "linux", target_os = "windows", target_os = "macos")
))]
mod user;
#[cfg(all(
    target_pointer_width = "64",
    any(target_os = "linux", target_os = "windows", target_os = "macos")
))]
mod utils;

#[cfg(all(
    target_pointer_width = "64",
    any(target_os = "linux", target_os = "windows", target_os = "macos")
))]
pub use crate::{
    apps::{Apps, CONNECT_LOBBY, connect_lobby},
    call::{CallError, CallResult, CallState, SteamCall},
    callbacks::SteamEvent,
    client::{AppId, Steam},
    error::{EResult, InitError, SteamError},
    friends::Friends,
    matchmaking::{
        EnterResponse, Lobby, LobbyCreated, LobbyEntered, LobbyId, LobbyKind,
        MAX_LOBBY_CHAT_MESSAGE, MAX_LOBBY_KEY_LENGTH, Matchmaking, MemberChange,
    },
    presence::{
        MAX_RICH_PRESENCE_KEY_LENGTH, MAX_RICH_PRESENCE_KEYS, MAX_RICH_PRESENCE_VALUE_LENGTH,
    },
    pump::PumpDiagnostics,
    user::{SteamId, User},
    utils::{HardwareDefaultConfig, NotificationCorner, SteamHardware, Utils},
};
