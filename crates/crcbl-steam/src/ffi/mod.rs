//! Hand-written declarations of the Steamworks flat C API.
//!
//! Nothing here is generated from, copied out of, or checked against a header
//! at build time: the SDK cannot enter the repository (see the crate docs), so
//! every prototype is this crate's own declaration, carrying the C text it was
//! read from and the SDK version (1.65). The `#[ignore]`d drift gate
//! (`drift`) is what compares them with a real SDK.
//!
//! - `manifest` — every bound function: its symbol, its C declaration, and
//!   its Rust type, written once. The function-pointer groups in [`Lib`], the
//!   `prototype` aliases, the resolver and the drift gate's `BINDINGS` table
//!   all come from that one list.
//! - `structs` — the structs Steam hands over, packed as the SDK packs them.
//! - `versions` — accessor names and interface-version strings, the single
//!   source for both the accessor lookup and the init handshake.
//! - `load` — the per-OS loader.
//!
//! Every function is `unsafe extern "C"`: `S_CALLTYPE` is `__cdecl`, and on
//! every 64-bit target this crate compiles for there is one C convention.

pub(crate) mod load;
pub(crate) mod manifest;
pub(crate) mod structs;
pub(crate) mod versions;

#[cfg(test)]
mod drift;

use core::sync::atomic::AtomicBool;

pub(crate) use manifest::Fns;

/// `HSteamPipe` — `typedef int32 HSteamPipe;`.
pub(crate) type HSteamPipe = i32;
/// `HSteamUser` — `typedef int32 HSteamUser;`.
pub(crate) type HSteamUser = i32;
/// `SteamAPICall_t` — `typedef uint64 SteamAPICall_t;`, `0` reserved as
/// `k_uAPICallInvalid`.
pub(crate) type SteamApiCall = u64;
/// `HSteamNetConnection` — `typedef uint32 HSteamNetConnection;`, `0` being
/// `k_HSteamNetConnection_Invalid`.
pub(crate) type HSteamNetConnection = u32;
/// `HSteamListenSocket` — `typedef uint32 HSteamListenSocket;`, `0` being
/// `k_HSteamListenSocket_Invalid`.
pub(crate) type HSteamListenSocket = u32;
/// `SteamErrMsg` — `typedef char SteamErrMsg[ 1024 ];`, the English message
/// `SteamInternal_SteamAPI_Init` fills in on failure.
pub(crate) type SteamErrMsg = [core::ffi::c_char; 1024];

/// `ESteamAPIInitResult`'s values, from `steam_api.h`.
pub(crate) mod init_result {
    /// `k_ESteamAPIInitResult_OK`.
    pub(crate) const OK: i32 = 0;
    /// `k_ESteamAPIInitResult_FailedGeneric`.
    pub(crate) const FAILED_GENERIC: i32 = 1;
    /// `k_ESteamAPIInitResult_NoSteamClient`.
    pub(crate) const NO_STEAM_CLIENT: i32 = 2;
    /// `k_ESteamAPIInitResult_VersionMismatch`.
    pub(crate) const VERSION_MISMATCH: i32 = 3;
}

/// An opaque interface object; only ever held by pointer.
macro_rules! opaque {
    ($($(#[$meta:meta])* $name:ident;)+) => {
        $(
            $(#[$meta])*
            #[repr(C)]
            pub(crate) struct $name {
                _opaque: [u8; 0],
            }
        )+
    };
}

opaque! {
    /// `class ISteamApps`.
    ISteamApps;
    /// `class ISteamFriends`.
    ISteamFriends;
    /// `class ISteamMatchmaking`.
    ISteamMatchmaking;
    /// `class ISteamNetworkingSockets`.
    ISteamNetworkingSockets;
    /// `class ISteamNetworkingUtils`.
    ISteamNetworkingUtils;
    /// `class ISteamRemoteStorage`.
    ISteamRemoteStorage;
    /// `class ISteamUser`.
    ISteamUser;
    /// `class ISteamUtils`.
    ISteamUtils;
}

/// A loaded library: its resolved functions, and the guard that allows one
/// live `Steam` at a time over it.
///
/// Leaked, never freed: interface pointers and callback buffers point into the
/// module, so unloading it while anything might hold one would be a
/// use-after-free. The real library is loaded once per process (`load::real`);
/// each fake-library test leaks its own.
#[derive(Debug)]
pub(crate) struct Lib {
    /// Every bound function.
    pub(crate) fns: Fns,
    /// Set while a `Steam` built over this library is alive. A field of the
    /// library rather than a process global, so fake-library tests — threads
    /// of one process under `cargo test` — each get their own.
    pub(crate) live: AtomicBool,
}

impl Lib {
    /// A library over resolved functions, with no `Steam` alive.
    pub(crate) const fn new(fns: Fns) -> Self {
        Self {
            fns,
            live: AtomicBool::new(false),
        }
    }
}
