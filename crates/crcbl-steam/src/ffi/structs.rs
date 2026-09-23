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
        // Made only of 1-aligned `CSteamID`s and bytes, so 1-aligned in C too;
        // a `u64` in place of `CSteamId` would make these 4 or 8.
        assert_eq!(align_of::<GameLobbyJoinRequested>(), 1);
        assert_eq!(align_of::<GameRichPresenceJoinRequested>(), 1);
        assert_layout!(NewUrlLaunchParameters, 1, {
            unused: 0, 1;
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
    }
}
