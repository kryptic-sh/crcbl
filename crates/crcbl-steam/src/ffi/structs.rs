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
    }
}
