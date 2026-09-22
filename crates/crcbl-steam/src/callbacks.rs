//! The callback ids this crate claims, and what each payload decodes to.
//!
//! One row per bound callback: its id, written as Valve writes it — a base
//! plus an offset (`k_iSteamFriendsCallbacks + 31`) — its C struct name, the
//! size the payload must have, and the decode. The size is `size_of` the
//! declared struct, which `ffi::structs`'s layout tables pin per operating
//! system. The drift gate compares each row's base and offset with the
//! header's own `k_iCallback` expression.
//!
//! Ids not in the table are skipped by the pump, by design: the pipe carries
//! dozens nobody bound, and every SDK adds more.

use crate::ffi::structs::{GameOverlayActivated, SteamApiCallCompleted};

/// The callback id bases from `steam_api_common.h`'s `enum { k_iSteam…Callbacks = … }`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(i32)]
pub(crate) enum Base {
    /// `k_iSteamFriendsCallbacks`.
    Friends = 300,
    /// `k_iSteamUtilsCallbacks`.
    Utils = 700,
}

impl Base {
    /// Valve's name for the base, as the headers spell it.
    #[cfg(test)]
    pub(crate) const fn valve_name(self) -> &'static str {
        match self {
            Self::Friends => "k_iSteamFriendsCallbacks",
            Self::Utils => "k_iSteamUtilsCallbacks",
        }
    }
}

/// Something that happened in Steam, drained from [`Steam::events`](crate::Steam::events).
///
/// One variant per bound callback, added by the slice that binds it.
#[derive(Debug, Clone, PartialEq, Eq)]
#[non_exhaustive]
pub enum SteamEvent {
    /// The Steam overlay opened (`active`) or closed. An open overlay should
    /// pause the game and release held input exactly as focus loss does.
    OverlayActivated {
        /// Whether it has just opened.
        active: bool,
    },
}

/// What a claimed payload decodes to.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum Decoded {
    /// A game-visible event.
    Event(SteamEvent),
    /// `SteamAPICallCompleted_t`: an asynchronous call's answer is ready. No
    /// slice has registered an asynchronous call yet, so the pump counts every
    /// one as unclaimed.
    CallCompleted(SteamApiCallCompleted),
}

impl PartialEq for SteamApiCallCompleted {
    fn eq(&self, other: &Self) -> bool {
        // Copies, never references: the struct is packed.
        let (a, b) = (*self, *other);
        (a.async_call, a.callback, a.param_size) == (b.async_call, b.callback, b.param_size)
    }
}

impl Eq for SteamApiCallCompleted {}

/// One claimed callback id.
#[derive(Debug, Clone, Copy)]
pub(crate) struct Row {
    /// Valve's base for the id.
    pub(crate) base: Base,
    /// The offset from [`base`](Self::base).
    pub(crate) offset: i32,
    /// The C struct name, for the drift gate.
    #[cfg(test)]
    pub(crate) name: &'static str,
    /// The size the payload must be: `size_of` the declared struct.
    pub(crate) size: usize,
    /// Decodes a payload of exactly [`size`](Self::size) bytes; `None` for any
    /// other length.
    pub(crate) decode: fn(&[u8]) -> Option<Decoded>,
}

impl Row {
    /// The callback id: `base + offset`.
    pub(crate) const fn id(&self) -> i32 {
        self.base as i32 + self.offset
    }
}

/// Every claimed callback.
pub(crate) const ROWS: &[Row] = &[
    Row {
        base: Base::Utils,
        offset: 3,
        #[cfg(test)]
        name: "SteamAPICallCompleted_t",
        size: size_of::<SteamApiCallCompleted>(),
        decode: |bytes| read::<SteamApiCallCompleted>(bytes).map(Decoded::CallCompleted),
    },
    Row {
        base: Base::Friends,
        offset: 31,
        #[cfg(test)]
        name: "GameOverlayActivated_t",
        size: size_of::<GameOverlayActivated>(),
        decode: |bytes| {
            read::<GameOverlayActivated>(bytes).map(|payload| {
                Decoded::Event(SteamEvent::OverlayActivated {
                    active: payload.active != 0,
                })
            })
        },
    },
];

/// The row claiming `id`, if any.
pub(crate) fn find(id: i32) -> Option<&'static Row> {
    ROWS.iter().find(|row| row.id() == id)
}

/// Types every bit pattern of which is a valid value: `ffi::structs`'s
/// payload structs, whose fields are all integers.
///
/// # Safety
///
/// Implement only for `Copy` types with no padding-sensitive invariants, no
/// references, no `bool`, no enums — any `size_of::<Self>()` bytes must be a
/// valid `Self`.
pub(crate) unsafe trait Pod: Copy {}

// SAFETY: every field is an integer (see `ffi::structs`).
unsafe impl Pod for SteamApiCallCompleted {}
// SAFETY: every field is an integer, `bool` included as `u8`.
unsafe impl Pod for GameOverlayActivated {}

/// Copies a `T` out of exactly `size_of::<T>()` bytes; `None` for any other
/// length.
fn read<T: Pod>(bytes: &[u8]) -> Option<T> {
    if bytes.len() != size_of::<T>() {
        return None;
    }
    // SAFETY: `bytes` holds exactly `size_of::<T>()` readable bytes, the read
    // is unaligned, and `T: Pod` makes any bytes a valid value.
    Some(unsafe { bytes.as_ptr().cast::<T>().read_unaligned() })
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A `GameOverlayActivated_t` payload, byte by byte in little-endian
    /// (every supported target is): active, user-initiated, two bytes of
    /// padding, the app id, the overlay's pid.
    fn overlay_bytes(active: u8) -> Vec<u8> {
        let mut bytes = vec![active, 1, 0, 0];
        bytes.extend_from_slice(&480u32.to_le_bytes());
        bytes.extend_from_slice(&1234u32.to_le_bytes());
        bytes
    }

    #[test]
    fn the_ids_are_valves() {
        assert_eq!(find(331).map(|row| row.name), Some("GameOverlayActivated_t"));
        assert_eq!(find(703).map(|row| row.name), Some("SteamAPICallCompleted_t"));
        assert!(find(332).is_none());
    }

    #[test]
    fn no_two_rows_claim_one_id() {
        for (i, a) in ROWS.iter().enumerate() {
            for b in &ROWS[i + 1..] {
                assert_ne!(a.id(), b.id(), "{} and {}", a.name, b.name);
            }
        }
    }

    #[test]
    fn overlay_activated_decodes_m_b_active() {
        let row = find(331).unwrap();
        assert_eq!(
            (row.decode)(&overlay_bytes(1)),
            Some(Decoded::Event(SteamEvent::OverlayActivated { active: true }))
        );
        assert_eq!(
            (row.decode)(&overlay_bytes(0)),
            Some(Decoded::Event(SteamEvent::OverlayActivated { active: false }))
        );
    }

    #[test]
    fn call_completed_decodes_every_field() {
        let mut bytes = 0x1122_3344_5566_7788u64.to_le_bytes().to_vec();
        bytes.extend_from_slice(&513i32.to_le_bytes());
        bytes.extend_from_slice(&24u32.to_le_bytes());
        let Some(Decoded::CallCompleted(done)) = (find(703).unwrap().decode)(&bytes) else {
            panic!("did not decode");
        };
        assert_eq!({ done.async_call }, 0x1122_3344_5566_7788);
        assert_eq!({ done.callback }, 513);
        assert_eq!({ done.param_size }, 24);
    }

    #[test]
    fn a_payload_of_the_wrong_length_does_not_decode() {
        let row = find(331).unwrap();
        let mut long = overlay_bytes(1);
        long.push(0);
        assert_eq!((row.decode)(&long), None);
        assert_eq!((row.decode)(&overlay_bytes(1)[..11]), None);
        assert_eq!((row.decode)(&[]), None);
    }
}
