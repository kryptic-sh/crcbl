//! The structs Steam hands over, laid out as the SDK lays them out.
//!
//! # Packing is per struct, and per operating system
//!
//! `steamclientpublic.h` defines `VALVE_CALLBACK_PACK_SMALL` on Linux, macOS
//! and FreeBSD and `VALVE_CALLBACK_PACK_LARGE` elsewhere, and each header that
//! declares callback structs wraps them in `#pragma pack( push, 4 )` or
//! `pack( push, 8 )` to match. So a struct with an 8-byte field at an offset
//! that is 4 mod 8, or whose unpadded size is 4 mod 8 and which holds an
//! 8-byte field, has a different layout on Windows. `callback_packed!` is
//! that pragma: `repr(C, packed(8))` on Windows, `repr(C, packed(4))`
//! elsewhere. On x86-64 Windows `packed(8)` changes nothing from natural
//! `repr(C)`, but spelling it keeps the rule below uniform on every OS.
//!
//! Some structs sit under a different pragma — `pack( push, 1 )` for
//! `SteamNetworkingIdentity` and Steam Input's action data, none at all for
//! `SteamNetworkingMessage_t` — so the pragma is read per struct, never assumed
//! from the file; `Pack` records which one each declaration here asserts,
//! and the drift gate compares it with the header.
//!
//! # Reading them
//!
//! **No field is ever referenced**, only copied: a packed struct's fields may
//! be unaligned. **Every field is an integer or a raw pointer**, never `bool`
//! or a Rust enum — C's `bool` becomes `u8` here — so any bytes Steam hands
//! over are a valid value, and a payload is decoded with one `read_unaligned`
//! after its size is checked.
//!
//! # Where the layout numbers come from
//!
//! The tables in this module's tests are the output of a C++ program,
//! compiled with MinGW-w64 GCC 16.2.0 for x86-64, that includes the SDK 1.65
//! headers — the Steamworks.NET mirror's copy (`docs/plan/42-steam.md`,
//! "Conventions"), not a Valve zip — and prints `sizeof` and every field's
//! `offsetof` and `sizeof`. It is compiled twice: as is, which selects
//! `VALVE_CALLBACK_PACK_LARGE` (Windows), and against a copy of
//! `steamclientpublic.h` whose platform test is forced true, which selects
//! `VALVE_CALLBACK_PACK_SMALL` (Linux and macOS). The field types are
//! fixed-width integers, `double`, enums and pointers, whose layout under an
//! explicit pack is the same on the SysV, AArch64 and Windows x64 ABIs.
//! Slice 1's tables were first computed over this crate's own transcription
//! of the fields and re-derived from the headers without a change.
//! `ValvePackingSentinel_t` is the independent check: the header itself
//! asserts it is 24 bytes under `VALVE_CALLBACK_PACK_SMALL` and 32 under
//! `VALVE_CALLBACK_PACK_LARGE`, so if `callback_packed!` picked the wrong
//! arm on some target, that one table says so before any real struct is read.

use super::{HSteamUser, SteamApiCall};

/// Declares a struct under Valve's callback packing: `pack(8)` on Windows,
/// `pack(4)` on Linux and macOS.
macro_rules! callback_packed {
    ($(#[$meta:meta])* pub(crate) struct $name:ident { $($body:tt)* }) => {
        $(#[$meta])*
        #[derive(Debug, Clone, Copy)]
        #[cfg_attr(windows, repr(C, packed(8)))]
        #[cfg_attr(not(windows), repr(C, packed(4)))]
        pub(crate) struct $name { $($body)* }
    };
}

/// The `#pragma pack` a declaration here asserts is in force at the struct.
#[cfg(test)]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Pack {
    /// Valve's callback packing: 4 on Linux and macOS, 8 on Windows.
    Callback,
    /// `#pragma pack(push,1)`, the same on every OS.
    One,
    /// No pragma: the compiler's natural layout.
    Natural,
}

/// One bound struct as the drift gate compares it with its header: the C
/// name, the pragma, and the field declarations in order.
#[cfg(test)]
#[derive(Debug, Clone, Copy)]
pub(crate) struct StructDecl {
    /// The C struct name.
    pub(crate) name: &'static str,
    /// The pragma in force at the struct.
    pub(crate) pack: Pack,
    /// Each field declaration without its `;`, as the header spells it.
    pub(crate) fields: &'static [&'static str],
}

/// Every bound struct, for the drift gate.
#[cfg(test)]
pub(crate) const DECLS: &[StructDecl] = &[
    StructDecl {
        name: "ValvePackingSentinel_t",
        pack: Pack::Callback,
        fields: &["uint32 m_u32", "uint64 m_u64", "uint16 m_u16", "double m_d"],
    },
    StructDecl {
        name: "CallbackMsg_t",
        pack: Pack::Callback,
        fields: &[
            "HSteamUser m_hSteamUser",
            "int m_iCallback",
            "uint8 *m_pubParam",
            "int m_cubParam",
        ],
    },
    StructDecl {
        name: "SteamAPICallCompleted_t",
        pack: Pack::Callback,
        fields: &[
            "SteamAPICall_t m_hAsyncCall",
            "int m_iCallback",
            "uint32 m_cubParam",
        ],
    },
    StructDecl {
        name: "GameOverlayActivated_t",
        pack: Pack::Callback,
        fields: &[
            "uint8 m_bActive",
            "bool m_bUserInitiated",
            "AppId_t m_nAppID",
            "uint32 m_dwOverlayPID",
        ],
    },
    StructDecl {
        name: "GameLobbyJoinRequested_t",
        pack: Pack::Callback,
        fields: &["CSteamID m_steamIDLobby", "CSteamID m_steamIDFriend"],
    },
    StructDecl {
        name: "GameRichPresenceJoinRequested_t",
        pack: Pack::Callback,
        fields: &[
            "CSteamID m_steamIDFriend",
            "char m_rgchConnect[k_cchMaxRichPresenceValueLength]",
        ],
    },
    StructDecl {
        name: "NewUrlLaunchParameters_t",
        pack: Pack::Callback,
        fields: &[],
    },
    StructDecl {
        name: "UserStatsReceived_t",
        pack: Pack::Callback,
        fields: &[
            "uint64 m_nGameID",
            "EResult m_eResult",
            "CSteamID m_steamIDUser",
        ],
    },
    StructDecl {
        name: "UserStatsStored_t",
        pack: Pack::Callback,
        fields: &["uint64 m_nGameID", "EResult m_eResult"],
    },
    StructDecl {
        name: "UserAchievementStored_t",
        pack: Pack::Callback,
        fields: &[
            "uint64 m_nGameID",
            "bool m_bGroupAchievement",
            "char m_rgchAchievementName[k_cchStatNameMax]",
            "uint32 m_nCurProgress",
            "uint32 m_nMaxProgress",
        ],
    },
    StructDecl {
        name: "LeaderboardFindResult_t",
        pack: Pack::Callback,
        fields: &[
            "SteamLeaderboard_t m_hSteamLeaderboard",
            "uint8 m_bLeaderboardFound",
        ],
    },
    StructDecl {
        name: "LeaderboardScoresDownloaded_t",
        pack: Pack::Callback,
        fields: &[
            "SteamLeaderboard_t m_hSteamLeaderboard",
            "SteamLeaderboardEntries_t m_hSteamLeaderboardEntries",
            "int m_cEntryCount",
        ],
    },
    StructDecl {
        name: "LeaderboardScoreUploaded_t",
        pack: Pack::Callback,
        fields: &[
            "uint8 m_bSuccess",
            "SteamLeaderboard_t m_hSteamLeaderboard",
            "int32 m_nScore",
            "uint8 m_bScoreChanged",
            "int m_nGlobalRankNew",
            "int m_nGlobalRankPrevious",
        ],
    },
    StructDecl {
        name: "LeaderboardEntry_t",
        pack: Pack::Callback,
        fields: &[
            "CSteamID m_steamIDUser",
            "int32 m_nGlobalRank",
            "int32 m_nScore",
            "int32 m_cDetails",
            "UGCHandle_t m_hUGC",
        ],
    },
    StructDecl {
        name: "RemoteStorageLocalFileChange_t",
        pack: Pack::Callback,
        fields: &[],
    },
    StructDecl {
        name: "PersonaStateChange_t",
        pack: Pack::Callback,
        fields: &["uint64 m_ulSteamID", "int m_nChangeFlags"],
    },
    StructDecl {
        name: "AvatarImageLoaded_t",
        pack: Pack::Callback,
        fields: &[
            "CSteamID m_steamID",
            "int m_iImage",
            "int m_iWide",
            "int m_iTall",
        ],
    },
    StructDecl {
        name: "FriendRichPresenceUpdate_t",
        pack: Pack::Callback,
        fields: &["CSteamID m_steamIDFriend", "AppId_t m_nAppID"],
    },
    StructDecl {
        name: "SteamNetworkingIPAddr",
        pack: Pack::One,
        // The `union { uint8 m_ipv6[ 16 ]; … }` before it is not a member
        // line the gate reads; the layout table pins its 16 bytes.
        fields: &["uint16 m_port"],
    },
    StructDecl {
        name: "SteamNetworkingIdentity",
        pack: Pack::One,
        // As above: the 128-byte union after `m_cbSize` is pinned by size.
        fields: &["ESteamNetworkingIdentityType m_eType", "int m_cbSize"],
    },
    StructDecl {
        name: "SteamNetConnectionInfo_t",
        pack: Pack::Callback,
        fields: &[
            "SteamNetworkingIdentity m_identityRemote",
            "int64 m_nUserData",
            "HSteamListenSocket m_hListenSocket",
            "SteamNetworkingIPAddr m_addrRemote",
            "uint16 m__pad1",
            "SteamNetworkingPOPID m_idPOPRemote",
            "SteamNetworkingPOPID m_idPOPRelay",
            "ESteamNetworkingConnectionState m_eState",
            "int m_eEndReason",
            "char m_szEndDebug[ k_cchSteamNetworkingMaxConnectionCloseReason ]",
            "char m_szConnectionDescription[ k_cchSteamNetworkingMaxConnectionDescription ]",
            "int m_nFlags",
            "uint32 reserved[63]",
        ],
    },
    StructDecl {
        name: "SteamNetConnectionStatusChangedCallback_t",
        pack: Pack::Callback,
        fields: &[
            "HSteamNetConnection m_hConn",
            "SteamNetConnectionInfo_t m_info",
            "ESteamNetworkingConnectionState m_eOldState",
        ],
    },
    StructDecl {
        name: "SteamRelayNetworkStatus_t",
        pack: Pack::Natural,
        fields: &[
            "ESteamNetworkingAvailability m_eAvail",
            "int m_bPingMeasurementInProgress",
            "ESteamNetworkingAvailability m_eAvailNetworkConfig",
            "ESteamNetworkingAvailability m_eAvailAnyRelay",
            "char m_debugMsg[ 256 ]",
        ],
    },
    StructDecl {
        name: "SteamNetworkingMessage_t",
        pack: Pack::Natural,
        fields: &[
            "void *m_pData",
            "int m_cbSize",
            "HSteamNetConnection m_conn",
            "SteamNetworkingIdentity m_identityPeer",
            "int64 m_nConnUserData",
            "SteamNetworkingMicroseconds m_usecTimeReceived",
            "int64 m_nMessageNumber",
            "void (*m_pfnFreeData)( SteamNetworkingMessage_t *pMsg )",
            "void (*m_pfnRelease)( SteamNetworkingMessage_t *pMsg )",
            "int m_nChannel",
            "int m_nFlags",
            "int64 m_nUserData",
            "uint16 m_idxLane",
            "uint16 _pad1__",
        ],
    },
    StructDecl {
        name: "InputAnalogActionData_t",
        pack: Pack::One,
        fields: &["EInputSourceMode eMode", "float x, y", "bool bActive"],
    },
    StructDecl {
        name: "InputDigitalActionData_t",
        pack: Pack::One,
        fields: &["bool bState", "bool bActive"],
    },
    StructDecl {
        name: "SteamInputDeviceConnected_t",
        pack: Pack::Callback,
        fields: &["InputHandle_t m_ulConnectedDeviceHandle"],
    },
    StructDecl {
        name: "SteamInputDeviceDisconnected_t",
        pack: Pack::Callback,
        fields: &["InputHandle_t m_ulDisconnectedDeviceHandle"],
    },
    StructDecl {
        name: "LobbyCreated_t",
        pack: Pack::Callback,
        fields: &["EResult m_eResult", "uint64 m_ulSteamIDLobby"],
    },
    StructDecl {
        name: "LobbyEnter_t",
        pack: Pack::Callback,
        fields: &[
            "uint64 m_ulSteamIDLobby",
            "uint32 m_rgfChatPermissions",
            "bool m_bLocked",
            "uint32 m_EChatRoomEnterResponse",
        ],
    },
    StructDecl {
        name: "LobbyDataUpdate_t",
        pack: Pack::Callback,
        fields: &[
            "uint64 m_ulSteamIDLobby",
            "uint64 m_ulSteamIDMember",
            "uint8 m_bSuccess",
        ],
    },
    StructDecl {
        name: "LobbyChatUpdate_t",
        pack: Pack::Callback,
        fields: &[
            "uint64 m_ulSteamIDLobby",
            "uint64 m_ulSteamIDUserChanged",
            "uint64 m_ulSteamIDMakingChange",
            "uint32 m_rgfChatMemberStateChange",
        ],
    },
    StructDecl {
        name: "LobbyChatMsg_t",
        pack: Pack::Callback,
        fields: &[
            "uint64 m_ulSteamIDLobby",
            "uint64 m_ulSteamIDUser",
            "uint8 m_eChatEntryType",
            "uint32 m_iChatID",
        ],
    },
];

#[cfg(test)]
callback_packed! {
    /// `ValvePackingSentinel_t` (`steamclientpublic.h`): Valve's own canary
    /// for the callback packing. Never read from Steam; it exists for its
    /// layout test.
    pub(crate) struct ValvePackingSentinel {
        /// `uint32 m_u32`.
        pub(crate) m_u32: u32,
        /// `uint64 m_u64`.
        pub(crate) m_u64: u64,
        /// `uint16 m_u16`.
        pub(crate) m_u16: u16,
        /// `double m_d`.
        pub(crate) m_d: f64,
    }
}

callback_packed! {
    /// `CallbackMsg_t` (`steam_api_common.h`): one message drained from the
    /// pipe. The payload `param` points at is Steam's, valid until
    /// `SteamAPI_ManualDispatch_FreeLastCallback`.
    pub(crate) struct CallbackMsg {
        /// `HSteamUser m_hSteamUser`.
        pub(crate) steam_user: HSteamUser,
        /// `int m_iCallback` — the payload struct's `k_iCallback`.
        pub(crate) callback: i32,
        /// `uint8 *m_pubParam`.
        pub(crate) param: *mut u8,
        /// `int m_cubParam` — the payload's size in bytes.
        pub(crate) param_size: i32,
    }
}

impl CallbackMsg {
    /// An empty message for `GetNextCallback` to fill.
    pub(crate) const EMPTY: Self = Self {
        steam_user: 0,
        callback: 0,
        param: core::ptr::null_mut(),
        param_size: 0,
    };
}

callback_packed! {
    /// `SteamAPICallCompleted_t` (`isteamutils.h`, `k_iSteamUtilsCallbacks +
    /// 3`): an asynchronous call has an answer waiting in
    /// `SteamAPI_ManualDispatch_GetAPICallResult`.
    pub(crate) struct SteamApiCallCompleted {
        /// `SteamAPICall_t m_hAsyncCall`.
        pub(crate) async_call: SteamApiCall,
        /// `int m_iCallback` — the answer's callback id.
        pub(crate) callback: i32,
        /// `uint32 m_cubParam` — the answer's size.
        pub(crate) param_size: u32,
    }
}

callback_packed! {
    /// `GameOverlayActivated_t` (`isteamfriends.h`, `k_iSteamFriendsCallbacks
    /// + 31`): the overlay opened or closed.
    pub(crate) struct GameOverlayActivated {
        /// `uint8 m_bActive` — non-zero when it has just opened.
        pub(crate) active: u8,
        /// `bool m_bUserInitiated`.
        pub(crate) user_initiated: u8,
        /// `AppId_t m_nAppID`.
        pub(crate) app_id: u32,
        /// `uint32 m_dwOverlayPID`.
        pub(crate) overlay_pid: u32,
    }
}

/// `CSteamID` as it sits in a struct: a class under `#pragma pack( push, 1 )`
/// holding one 64-bit union, so 8 bytes **aligned to 1**. Declared as bytes,
/// not `u64`, because the alignment changes layouts: `AvatarImageLoaded_t`
/// (a `CSteamID` and three `int`s) is 20 bytes under either packing in C,
/// and would be 24 under `pack(8)` with a `u64` in its place. Read it with
/// [`steam_id`].
pub(crate) type CSteamId = [u8; 8];

/// A [`CSteamId`]'s 64-bit value. The union is a `uint64` in the target's own
/// byte order, which is little-endian on every supported target.
pub(crate) const fn steam_id(raw: CSteamId) -> u64 {
    u64::from_ne_bytes(raw)
}

callback_packed! {
    /// `GameLobbyJoinRequested_t` (`isteamfriends.h`,
    /// `k_iSteamFriendsCallbacks + 33`): the player accepted a lobby invite,
    /// or chose "Join game" on a friend, while the game was running.
    pub(crate) struct GameLobbyJoinRequested {
        /// `CSteamID m_steamIDLobby`.
        pub(crate) lobby: CSteamId,
        /// `CSteamID m_steamIDFriend` — invalid when not joined through a
        /// friend.
        pub(crate) friend: CSteamId,
    }
}

callback_packed! {
    /// `GameRichPresenceJoinRequested_t` (`isteamfriends.h`,
    /// `k_iSteamFriendsCallbacks + 37`): the player accepted a rich-presence
    /// invite while the game was running.
    pub(crate) struct GameRichPresenceJoinRequested {
        /// `CSteamID m_steamIDFriend`.
        pub(crate) friend: CSteamId,
        /// `char m_rgchConnect[k_cchMaxRichPresenceValueLength]` — the
        /// connect string, NUL-terminated inside the array.
        pub(crate) connect: [u8; 256],
    }
}

callback_packed! {
    /// `PersonaStateChange_t` (`isteamfriends.h`, `k_iSteamFriendsCallbacks +
    /// 4`): something about a user changed — name, status, avatar, rich
    /// presence; `m_nChangeFlags` says what.
    pub(crate) struct PersonaStateChange {
        /// `uint64 m_ulSteamID`.
        pub(crate) user: u64,
        /// `int m_nChangeFlags` — `EPersonaChange` bits.
        pub(crate) change: i32,
    }
}

callback_packed! {
    /// `AvatarImageLoaded_t` (`isteamfriends.h`, `k_iSteamFriendsCallbacks +
    /// 34`): an avatar that was still downloading has arrived.
    pub(crate) struct AvatarImageLoaded {
        /// `CSteamID m_steamID`.
        pub(crate) user: CSteamId,
        /// `int m_iImage` — the image handle.
        pub(crate) image: i32,
        /// `int m_iWide`.
        pub(crate) width: i32,
        /// `int m_iTall`.
        pub(crate) height: i32,
    }
}

callback_packed! {
    /// `FriendRichPresenceUpdate_t` (`isteamfriends.h`,
    /// `k_iSteamFriendsCallbacks + 36`): a friend's rich presence changed.
    pub(crate) struct FriendRichPresenceUpdate {
        /// `CSteamID m_steamIDFriend`.
        pub(crate) friend: CSteamId,
        /// `AppId_t m_nAppID`.
        pub(crate) app: u32,
    }
}

callback_packed! {
    /// `NewUrlLaunchParameters_t` (`isteamapps.h`, `k_iSteamAppsCallbacks +
    /// 14`): the game was launched again through a Steam URL while running.
    /// The C++ struct has no members; a C++ struct is never empty, so it is
    /// one byte, which is this field.
    pub(crate) struct NewUrlLaunchParameters {
        /// The one byte an empty C++ struct occupies; never meaningful.
        pub(crate) unused: u8,
    }
}

callback_packed! {
    /// `RemoteStorageLocalFileChange_t` (`isteamremotestorage.h`,
    /// `k_iSteamRemoteStorageCallbacks + 33`, declared with
    /// `STEAM_CALLBACK_BEGIN`): a cloud file changed during the session; the
    /// changes themselves are read with `GetLocalFileChange`. No members, so
    /// one byte, as [`NewUrlLaunchParameters`].
    pub(crate) struct RemoteStorageLocalFileChange {
        /// The one byte an empty C++ struct occupies; never meaningful.
        pub(crate) unused: u8,
    }
}

callback_packed! {
    /// `LobbyCreated_t` (`isteammatchmaking.h`, `k_iSteamMatchmakingCallbacks
    /// + 13`): the call result of `CreateLobby`.
    pub(crate) struct LobbyCreated {
        /// `EResult m_eResult`.
        pub(crate) result: i32,
        /// `uint64 m_ulSteamIDLobby` — zero on failure.
        pub(crate) lobby: u64,
    }
}

callback_packed! {
    /// `LobbyEnter_t` (`isteammatchmaking.h`, `k_iSteamMatchmakingCallbacks +
    /// 4`): the call result of `JoinLobby`, also broadcast on every entry.
    pub(crate) struct LobbyEnter {
        /// `uint64 m_ulSteamIDLobby`.
        pub(crate) lobby: u64,
        /// `uint32 m_rgfChatPermissions`.
        pub(crate) chat_permissions: u32,
        /// `bool m_bLocked` — only invitees may join.
        pub(crate) locked: u8,
        /// `uint32 m_EChatRoomEnterResponse`.
        pub(crate) response: u32,
    }
}

callback_packed! {
    /// `LobbyDataUpdate_t` (`isteammatchmaking.h`,
    /// `k_iSteamMatchmakingCallbacks + 5`): a lobby's or a member's data
    /// changed.
    pub(crate) struct LobbyDataUpdate {
        /// `uint64 m_ulSteamIDLobby`.
        pub(crate) lobby: u64,
        /// `uint64 m_ulSteamIDMember` — the lobby itself for lobby data.
        pub(crate) member: u64,
        /// `uint8 m_bSuccess`.
        pub(crate) success: u8,
    }
}

callback_packed! {
    /// `LobbyChatUpdate_t` (`isteammatchmaking.h`,
    /// `k_iSteamMatchmakingCallbacks + 6`): a member entered, left, dropped or
    /// was removed.
    pub(crate) struct LobbyChatUpdate {
        /// `uint64 m_ulSteamIDLobby`.
        pub(crate) lobby: u64,
        /// `uint64 m_ulSteamIDUserChanged`.
        pub(crate) changed: u64,
        /// `uint64 m_ulSteamIDMakingChange`.
        pub(crate) making_change: u64,
        /// `uint32 m_rgfChatMemberStateChange` — `EChatMemberStateChange` bits.
        pub(crate) state_change: u32,
    }
}

callback_packed! {
    /// `LobbyChatMsg_t` (`isteammatchmaking.h`, `k_iSteamMatchmakingCallbacks
    /// + 7`): a lobby chat message arrived; `GetLobbyChatEntry` reads it.
    pub(crate) struct LobbyChatMsg {
        /// `uint64 m_ulSteamIDLobby`.
        pub(crate) lobby: u64,
        /// `uint64 m_ulSteamIDUser`.
        pub(crate) user: u64,
        /// `uint8 m_eChatEntryType`.
        pub(crate) entry_type: u8,
        /// `uint32 m_iChatID`.
        pub(crate) chat_id: u32,
    }
}

/// `SteamNetworkingIdentity` (`steamnetworkingtypes.h`, under
/// `#pragma pack(push,1)`): who is at the other end of a connection. Only
/// the Steam-id form is built or read here.
#[derive(Debug, Clone, Copy)]
#[repr(C, packed)]
pub(crate) struct SteamNetworkingIdentity {
    /// `ESteamNetworkingIdentityType m_eType`.
    pub(crate) kind: i32,
    /// `int m_cbSize` — the bytes of `data` in use.
    pub(crate) size: i32,
    /// The union: `uint64 m_steamID64` in its first 8 bytes for a Steam id,
    /// `uint32 m_reserved[ 32 ]` giving its 128-byte size.
    pub(crate) data: [u8; 128],
}

callback_packed! {
    /// `SteamNetConnectionInfo_t` (`steamnetworkingtypes.h`): a connection's
    /// state, its remote identity and why it ended.
    pub(crate) struct SteamNetConnectionInfo {
        /// `SteamNetworkingIdentity m_identityRemote`.
        pub(crate) identity: SteamNetworkingIdentity,
        /// `int64 m_nUserData`.
        pub(crate) user_data: i64,
        /// `HSteamListenSocket m_hListenSocket` — the socket it arrived on, or
        /// zero for one this end opened.
        pub(crate) listen_socket: u32,
        /// `SteamNetworkingIPAddr m_addrRemote` — 1-aligned, 18 bytes.
        pub(crate) address: [u8; 18],
        /// `uint16 m__pad1`.
        pub(crate) pad1: u16,
        /// `SteamNetworkingPOPID m_idPOPRemote`.
        pub(crate) pop_remote: u32,
        /// `SteamNetworkingPOPID m_idPOPRelay`.
        pub(crate) pop_relay: u32,
        /// `ESteamNetworkingConnectionState m_eState`.
        pub(crate) state: i32,
        /// `int m_eEndReason`.
        pub(crate) end_reason: i32,
        /// `char m_szEndDebug[ k_cchSteamNetworkingMaxConnectionCloseReason ]`.
        pub(crate) end_debug: [u8; 128],
        /// `char m_szConnectionDescription[ k_cchSteamNetworkingMaxConnectionDescription ]`.
        pub(crate) description: [u8; 128],
        /// `int m_nFlags`.
        pub(crate) flags: i32,
        /// `uint32 reserved[63]`.
        pub(crate) reserved: [u32; 63],
    }
}

callback_packed! {
    /// `UserStatsReceived_t` (`isteamuserstats.h`, `k_iSteamUserStatsCallbacks
    /// + 1`): a user's stats and achievements arrived — for the local user,
    /// on their own at start-up since SDK 1.61.
    pub(crate) struct UserStatsReceived {
        /// `uint64 m_nGameID`.
        pub(crate) game_id: u64,
        /// `EResult m_eResult`.
        pub(crate) result: i32,
        /// `CSteamID m_steamIDUser`.
        pub(crate) user: CSteamId,
    }
}

callback_packed! {
    /// `UserStatsStored_t` (`isteamuserstats.h`, `k_iSteamUserStatsCallbacks
    /// + 2`): the answer to `StoreStats`.
    pub(crate) struct UserStatsStored {
        /// `uint64 m_nGameID`.
        pub(crate) game_id: u64,
        /// `EResult m_eResult`.
        pub(crate) result: i32,
    }
}

callback_packed! {
    /// `UserAchievementStored_t` (`isteamuserstats.h`,
    /// `k_iSteamUserStatsCallbacks + 3`): an achievement was stored, or its
    /// progress shown; zero progress of zero means unlocked.
    pub(crate) struct UserAchievementStored {
        /// `uint64 m_nGameID`.
        pub(crate) game_id: u64,
        /// `bool m_bGroupAchievement` — unused, per the header.
        pub(crate) group_achievement: u8,
        /// `char m_rgchAchievementName[k_cchStatNameMax]`.
        pub(crate) name: [u8; 128],
        /// `uint32 m_nCurProgress`.
        pub(crate) current: u32,
        /// `uint32 m_nMaxProgress`.
        pub(crate) max: u32,
    }
}

callback_packed! {
    /// `LeaderboardFindResult_t` (`isteamuserstats.h`,
    /// `k_iSteamUserStatsCallbacks + 4`): the call result of
    /// `FindOrCreateLeaderboard` and `FindLeaderboard`.
    pub(crate) struct LeaderboardFindResult {
        /// `SteamLeaderboard_t m_hSteamLeaderboard` — zero when not found.
        pub(crate) leaderboard: u64,
        /// `uint8 m_bLeaderboardFound`.
        pub(crate) found: u8,
    }
}

callback_packed! {
    /// `LeaderboardScoresDownloaded_t` (`isteamuserstats.h`,
    /// `k_iSteamUserStatsCallbacks + 5`): the call result of
    /// `DownloadLeaderboardEntries`.
    pub(crate) struct LeaderboardScoresDownloaded {
        /// `SteamLeaderboard_t m_hSteamLeaderboard`.
        pub(crate) leaderboard: u64,
        /// `SteamLeaderboardEntries_t m_hSteamLeaderboardEntries`.
        pub(crate) entries: u64,
        /// `int m_cEntryCount`.
        pub(crate) count: i32,
    }
}

callback_packed! {
    /// `LeaderboardScoreUploaded_t` (`isteamuserstats.h`,
    /// `k_iSteamUserStatsCallbacks + 6`): the call result of
    /// `UploadLeaderboardScore`.
    pub(crate) struct LeaderboardScoreUploaded {
        /// `uint8 m_bSuccess`.
        pub(crate) success: u8,
        /// `SteamLeaderboard_t m_hSteamLeaderboard`.
        pub(crate) leaderboard: u64,
        /// `int32 m_nScore`.
        pub(crate) score: i32,
        /// `uint8 m_bScoreChanged`.
        pub(crate) changed: u8,
        /// `int m_nGlobalRankNew`.
        pub(crate) rank_new: i32,
        /// `int m_nGlobalRankPrevious` — zero for a first entry.
        pub(crate) rank_previous: i32,
    }
}

callback_packed! {
    /// `LeaderboardEntry_t` (`isteamuserstats.h`): one downloaded entry, as
    /// `GetDownloadedLeaderboardEntry` fills it. Under the callback packing
    /// though it is no callback.
    pub(crate) struct LeaderboardEntry {
        /// `CSteamID m_steamIDUser`.
        pub(crate) user: CSteamId,
        /// `int32 m_nGlobalRank`.
        pub(crate) rank: i32,
        /// `int32 m_nScore`.
        pub(crate) score: i32,
        /// `int32 m_cDetails` — how many details the entry holds.
        pub(crate) details: i32,
        /// `UGCHandle_t m_hUGC`.
        pub(crate) ugc: u64,
    }
}

callback_packed! {
    /// `SteamNetConnectionStatusChangedCallback_t`
    /// (`isteamnetworkingsockets.h`, `k_iSteamNetworkingSocketsCallbacks + 1`):
    /// a connection changed state. The OS difference is here, not in the info:
    /// `m_info` starts at 4 under `pack(4)` and 8 under `pack(8)`.
    pub(crate) struct SteamNetConnectionStatusChanged {
        /// `HSteamNetConnection m_hConn`.
        pub(crate) connection: u32,
        /// `SteamNetConnectionInfo_t m_info`.
        pub(crate) info: SteamNetConnectionInfo,
        /// `ESteamNetworkingConnectionState m_eOldState`.
        pub(crate) old_state: i32,
    }
}

/// `InputAnalogActionData_t` (`isteaminput.h`, under `#pragma pack( push, 1
/// )`): one analog action's state, as `GetAnalogActionData` **returns it by
/// value**. 13 bytes, aligned to 1; the fields happen to sit at their natural
/// offsets.
#[derive(Debug, Clone, Copy, Default)]
#[repr(C, packed)]
pub(crate) struct InputAnalogActionData {
    /// `EInputSourceMode eMode` — `k_EInputSourceMode_JoystickMove` for a
    /// stick action, `…_Trigger` for a trigger one. Not read: the manifest
    /// fixes each action's mode.
    pub(crate) mode: i32,
    /// `float x` — a stick's X, or a trigger's pull.
    pub(crate) x: f32,
    /// `float y` — a stick's Y; unused for a trigger.
    pub(crate) y: f32,
    /// `bool bActive` — whether the action is bound in the active set.
    pub(crate) active: u8,
}

/// `InputDigitalActionData_t` (`isteaminput.h`, under `#pragma pack( push, 1
/// )`): one digital action's state, as `GetDigitalActionData` **returns it
/// by value**.
#[derive(Debug, Clone, Copy, Default)]
#[repr(C, packed)]
pub(crate) struct InputDigitalActionData {
    /// `bool bState` — held.
    pub(crate) state: u8,
    /// `bool bActive` — whether the action is bound in the active set.
    pub(crate) active: u8,
}

callback_packed! {
    /// `SteamInputDeviceConnected_t` (`isteaminput.h`,
    /// `k_iSteamControllerCallbacks + 1`): a Steam Input controller
    /// connected — or was already connected when device callbacks were
    /// enabled.
    pub(crate) struct SteamInputDeviceConnected {
        /// `InputHandle_t m_ulConnectedDeviceHandle`.
        pub(crate) handle: u64,
    }
}

callback_packed! {
    /// `SteamInputDeviceDisconnected_t` (`isteaminput.h`,
    /// `k_iSteamControllerCallbacks + 2`): a Steam Input controller
    /// disconnected.
    pub(crate) struct SteamInputDeviceDisconnected {
        /// `InputHandle_t m_ulDisconnectedDeviceHandle`.
        pub(crate) handle: u64,
    }
}

/// `SteamRelayNetworkStatus_t` (`isteamnetworkingutils.h`,
/// `k_iSteamNetworkingUtilsCallbacks + 1`): relay availability, both as
/// `GetRelayNetworkStatus` fills it and as a callback. Declared under no
/// pragma, unlike the other callbacks — the drift gate's first run over it
/// said so — so natural `repr(C)`; being `int`s and bytes, the layout is the
/// same either way.
#[derive(Debug, Clone, Copy)]
#[repr(C)]
pub(crate) struct SteamRelayNetworkStatus {
    /// `ESteamNetworkingAvailability m_eAvail`.
    pub(crate) availability: i32,
    /// `int m_bPingMeasurementInProgress`.
    pub(crate) ping_measurement_in_progress: i32,
    /// `ESteamNetworkingAvailability m_eAvailNetworkConfig`.
    pub(crate) network_config: i32,
    /// `ESteamNetworkingAvailability m_eAvailAnyRelay`.
    pub(crate) any_relay: i32,
    /// `char m_debugMsg[ 256 ]`.
    pub(crate) debug: [u8; 256],
}

/// `SteamNetworkingMessage_t` (`steamnetworkingtypes.h`, after the header
/// pops its packing, so natural `repr(C)` on every OS): a received message,
/// owned by Steam until `SteamAPI_SteamNetworkingMessage_t_Release`.
#[derive(Debug, Clone, Copy)]
#[repr(C)]
pub(crate) struct SteamNetworkingMessage {
    /// `void *m_pData`.
    pub(crate) data: *mut core::ffi::c_void,
    /// `int m_cbSize`.
    pub(crate) size: i32,
    /// `HSteamNetConnection m_conn`.
    pub(crate) connection: u32,
    /// `SteamNetworkingIdentity m_identityPeer`.
    pub(crate) identity_peer: SteamNetworkingIdentity,
    /// `int64 m_nConnUserData`.
    pub(crate) connection_user_data: i64,
    /// `SteamNetworkingMicroseconds m_usecTimeReceived`.
    pub(crate) time_received: i64,
    /// `int64 m_nMessageNumber`.
    pub(crate) message_number: i64,
    /// `void (*m_pfnFreeData)( SteamNetworkingMessage_t *pMsg )` — never
    /// called here, so held as an opaque pointer.
    pub(crate) free_data: *const core::ffi::c_void,
    /// `void (*m_pfnRelease)( SteamNetworkingMessage_t *pMsg )` — never
    /// called here; `SteamAPI_SteamNetworkingMessage_t_Release` is.
    pub(crate) release: *const core::ffi::c_void,
    /// `int m_nChannel`.
    pub(crate) channel: i32,
    /// `int m_nFlags` — on a received message only
    /// `k_nSteamNetworkingSend_Reliable` is meaningful.
    pub(crate) flags: i32,
    /// `int64 m_nUserData`.
    pub(crate) user_data: i64,
    /// `uint16 m_idxLane`.
    pub(crate) lane: u16,
    /// `uint16 _pad1__`.
    pub(crate) pad1: u16,
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The width of the field `select` picks out, read from its type alone —
    /// the function is never called.
    fn field_size<T, F>(_select: fn(&T) -> F) -> usize {
        size_of::<F>()
    }

    /// Asserts a structure's size and every field's offset and width, one
    /// `field: offset, width;` row per field — `crcbl-shell`'s Win32 macro,
    /// with the field read by copy because these structs are packed and a
    /// reference to a packed field is an error.
    ///
    /// The destructuring pattern names every row's field and has no `..`, so a
    /// field added to the declaration without a row here fails to compile
    /// rather than going unchecked.
    macro_rules! assert_layout {
        ($ty:ident, $size:literal, { $($field:ident: $offset:literal, $width:literal;)+ }) => {{
            let _every_field_has_a_row: fn($ty) = |value| {
                let $ty { $($field: _),+ } = value;
            };
            assert_eq!(size_of::<$ty>(), $size, concat!("size of ", stringify!($ty)));
            $(
                assert_eq!(
                    core::mem::offset_of!($ty, $field),
                    $offset,
                    concat!("offset of ", stringify!($ty), "::", stringify!($field))
                );
                assert_eq!(
                    field_size(|value: &$ty| value.$field),
                    $width,
                    concat!("width of ", stringify!($ty), "::", stringify!($field))
                );
            )+
        }};
    }

    /// The structs whose layout is the same under both packings.
    #[test]
    fn the_pack_independent_structures_match_the_c_layout() {
        assert_layout!(SteamApiCallCompleted, 16, {
            async_call: 0, 8;
            callback: 8, 4;
            param_size: 12, 4;
        });
        assert_layout!(GameOverlayActivated, 12, {
            active: 0, 1;
            user_initiated: 1, 1;
            app_id: 4, 4;
            overlay_pid: 8, 4;
        });
        assert_layout!(GameLobbyJoinRequested, 16, {
            lobby: 0, 8;
            friend: 8, 8;
        });
        assert_layout!(GameRichPresenceJoinRequested, 264, {
            friend: 0, 8;
            connect: 8, 256;
        });
        // 20 under both packings only because `CSteamID` is 1-aligned.
        assert_layout!(AvatarImageLoaded, 20, {
            user: 0, 8;
            image: 8, 4;
            width: 12, 4;
            height: 16, 4;
        });
        assert_layout!(FriendRichPresenceUpdate, 12, {
            friend: 0, 8;
            app: 8, 4;
        });
        assert_layout!(SteamNetworkingIdentity, 136, {
            kind: 0, 4;
            size: 4, 4;
            data: 8, 128;
        });
        assert_eq!(align_of::<SteamNetworkingIdentity>(), 1);
        // 696 under both; its alignment (8 or 4) is what moves the callback.
        assert_layout!(SteamNetConnectionInfo, 696, {
            identity: 0, 136;
            user_data: 136, 8;
            listen_socket: 144, 4;
            address: 148, 18;
            pad1: 166, 2;
            pop_remote: 168, 4;
            pop_relay: 172, 4;
            state: 176, 4;
            end_reason: 180, 4;
            end_debug: 184, 128;
            description: 312, 128;
            flags: 440, 4;
            reserved: 444, 252;
        });
        assert_layout!(SteamRelayNetworkStatus, 272, {
            availability: 0, 4;
            ping_measurement_in_progress: 4, 4;
            network_config: 8, 4;
            any_relay: 12, 4;
            debug: 16, 256;
        });
        // Natural `repr(C)`, 64-bit pointers on every supported target.
        assert_layout!(SteamNetworkingMessage, 216, {
            data: 0, 8;
            size: 8, 4;
            connection: 12, 4;
            identity_peer: 16, 136;
            connection_user_data: 152, 8;
            time_received: 160, 8;
            message_number: 168, 8;
            free_data: 176, 8;
            release: 184, 8;
            channel: 192, 4;
            flags: 196, 4;
            user_data: 200, 8;
            lane: 208, 2;
            pad1: 210, 2;
        });
        // Made only of 1-aligned `CSteamID`s and bytes, so 1-aligned in C too;
        // a `u64` in place of `CSteamId` would make these 4 or 8.
        assert_eq!(align_of::<GameLobbyJoinRequested>(), 1);
        assert_eq!(align_of::<GameRichPresenceJoinRequested>(), 1);
        assert_layout!(NewUrlLaunchParameters, 1, {
            unused: 0, 1;
        });
        assert_layout!(RemoteStorageLocalFileChange, 1, {
            unused: 0, 1;
        });
        // `pack(1)`, and returned by value: the size and alignment are what
        // the by-value return depends on.
        assert_layout!(InputAnalogActionData, 13, {
            mode: 0, 4;
            x: 4, 4;
            y: 8, 4;
            active: 12, 1;
        });
        assert_eq!(align_of::<InputAnalogActionData>(), 1);
        assert_layout!(InputDigitalActionData, 2, {
            state: 0, 1;
            active: 1, 1;
        });
        assert_eq!(align_of::<InputDigitalActionData>(), 1);
        // One `uint64`: 8 bytes under either packing, aligned 4 or 8.
        assert_layout!(SteamInputDeviceConnected, 8, {
            handle: 0, 8;
        });
        assert_layout!(SteamInputDeviceDisconnected, 8, {
            handle: 0, 8;
        });
        // The `uint32` after the `uint8` is 4-aligned under either packing.
        assert_layout!(LobbyChatMsg, 24, {
            lobby: 0, 8;
            user: 8, 8;
            entry_type: 16, 1;
            chat_id: 20, 4;
        });
    }

    /// `pack(4)`: Linux and macOS.
    #[cfg(not(windows))]
    #[test]
    fn the_callback_packed_structures_match_the_c_layout_at_pack_4() {
        // The header asserts 24 under `VALVE_CALLBACK_PACK_SMALL`.
        assert_layout!(ValvePackingSentinel, 24, {
            m_u32: 0, 4;
            m_u64: 4, 8;
            m_u16: 12, 2;
            m_d: 16, 8;
        });
        // No tail padding after the last `int`: 20, not 24.
        assert_layout!(CallbackMsg, 20, {
            steam_user: 0, 4;
            callback: 4, 4;
            param: 8, 8;
            param_size: 16, 4;
        });
        assert_layout!(SteamNetConnectionStatusChanged, 704, {
            connection: 0, 4;
            info: 4, 696;
            old_state: 700, 4;
        });
        assert_layout!(PersonaStateChange, 12, {
            user: 0, 8;
            change: 8, 4;
        });
        // The `uint64` after a 4-byte `EResult` sits at 4, not 8.
        assert_layout!(LobbyCreated, 12, {
            result: 0, 4;
            lobby: 4, 8;
        });
        // No tail padding after the last `uint32`.
        assert_layout!(LobbyEnter, 20, {
            lobby: 0, 8;
            chat_permissions: 8, 4;
            locked: 12, 1;
            response: 16, 4;
        });
        assert_layout!(LobbyDataUpdate, 20, {
            lobby: 0, 8;
            member: 8, 8;
            success: 16, 1;
        });
        assert_layout!(LobbyChatUpdate, 28, {
            lobby: 0, 8;
            changed: 8, 8;
            making_change: 16, 8;
            state_change: 24, 4;
        });
        assert_layout!(UserStatsReceived, 20, {
            game_id: 0, 8;
            result: 8, 4;
            user: 12, 8;
        });
        assert_layout!(UserStatsStored, 12, {
            game_id: 0, 8;
            result: 8, 4;
        });
        assert_layout!(UserAchievementStored, 148, {
            game_id: 0, 8;
            group_achievement: 8, 1;
            name: 9, 128;
            current: 140, 4;
            max: 144, 4;
        });
        assert_layout!(LeaderboardFindResult, 12, {
            leaderboard: 0, 8;
            found: 8, 1;
        });
        assert_layout!(LeaderboardScoresDownloaded, 20, {
            leaderboard: 0, 8;
            entries: 8, 8;
            count: 16, 4;
        });
        // The `uint64` after a leading byte sits at 4, not 8.
        assert_layout!(LeaderboardScoreUploaded, 28, {
            success: 0, 1;
            leaderboard: 4, 8;
            score: 12, 4;
            changed: 16, 1;
            rank_new: 20, 4;
            rank_previous: 24, 4;
        });
        assert_layout!(LeaderboardEntry, 28, {
            user: 0, 8;
            rank: 8, 4;
            score: 12, 4;
            details: 16, 4;
            ugc: 20, 8;
        });
    }

    /// `pack(8)`: Windows.
    #[cfg(windows)]
    #[test]
    fn the_callback_packed_structures_match_the_c_layout_at_pack_8() {
        // The header asserts 32 under `VALVE_CALLBACK_PACK_LARGE`.
        assert_layout!(ValvePackingSentinel, 32, {
            m_u32: 0, 4;
            m_u64: 8, 8;
            m_u16: 16, 2;
            m_d: 24, 8;
        });
        assert_layout!(CallbackMsg, 24, {
            steam_user: 0, 4;
            callback: 4, 4;
            param: 8, 8;
            param_size: 16, 4;
        });
        assert_layout!(SteamNetConnectionStatusChanged, 712, {
            connection: 0, 4;
            info: 8, 696;
            old_state: 704, 4;
        });
        assert_layout!(PersonaStateChange, 16, {
            user: 0, 8;
            change: 8, 4;
        });
        assert_layout!(LobbyCreated, 16, {
            result: 0, 4;
            lobby: 8, 8;
        });
        assert_layout!(LobbyEnter, 24, {
            lobby: 0, 8;
            chat_permissions: 8, 4;
            locked: 12, 1;
            response: 16, 4;
        });
        assert_layout!(LobbyDataUpdate, 24, {
            lobby: 0, 8;
            member: 8, 8;
            success: 16, 1;
        });
        assert_layout!(LobbyChatUpdate, 32, {
            lobby: 0, 8;
            changed: 8, 8;
            making_change: 16, 8;
            state_change: 24, 4;
        });
        assert_layout!(UserStatsReceived, 24, {
            game_id: 0, 8;
            result: 8, 4;
            user: 12, 8;
        });
        assert_layout!(UserStatsStored, 16, {
            game_id: 0, 8;
            result: 8, 4;
        });
        assert_layout!(UserAchievementStored, 152, {
            game_id: 0, 8;
            group_achievement: 8, 1;
            name: 9, 128;
            current: 140, 4;
            max: 144, 4;
        });
        assert_layout!(LeaderboardFindResult, 16, {
            leaderboard: 0, 8;
            found: 8, 1;
        });
        assert_layout!(LeaderboardScoresDownloaded, 24, {
            leaderboard: 0, 8;
            entries: 8, 8;
            count: 16, 4;
        });
        assert_layout!(LeaderboardScoreUploaded, 32, {
            success: 0, 1;
            leaderboard: 8, 8;
            score: 16, 4;
            changed: 20, 1;
            rank_new: 24, 4;
            rank_previous: 28, 4;
        });
        assert_layout!(LeaderboardEntry, 32, {
            user: 0, 8;
            rank: 8, 4;
            score: 12, 4;
            details: 16, 4;
            ugc: 24, 8;
        });
    }
}
