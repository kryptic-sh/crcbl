//! [`PlayerId`]: who a player is, across sessions, on the servers they join.
//!
//! # What it is
//!
//! A random 128-bit value a client draws once, keeps in its config directory
//! (`crcbl_store::identity`), and presents in every hello. The server keys
//! what must outlive a session on it — its denylist today; a stash, a voice
//! mute and a replay's attribution are the plans that wait on it — and keeps
//! it apart from the per-session numbers the transport and the host hand out
//! (`crcbl_net::SessionId`, `crcbl_server::PeerId`), which name one session
//! and are never reused.
//!
//! **128 bits, not the 64 `docs/plan/27-auth.md` sketches**, because nothing
//! coordinates who draws which: every client draws its own, so the width is
//! what keeps two players from ever drawing the same one, and what keeps one
//! player's id from being guessed. A backend-minted id of any narrower width
//! fits inside it when an authenticated tier exists.
//!
//! # How far it can be trusted: not at all, yet
//!
//! **The id is self-asserted.** On an open server — the LAN tiers, which are
//! the only ones built — a client says who it is and the server believes it:
//! anyone who learns another player's id can present it, and be them. What
//! the id buys at this tier is continuity for honest clients — the same
//! player recognised after a restart — not identity anyone has proved. A ban
//! by id holds back a player who does not know to change it, and nobody else.
//! An authenticated tier — a server-minted or backend-minted id bound into
//! the handshake — is what would make it a claim the server can rely on; it
//! is not built, and the id's type does not need to change when it is.
//!
//! The id is no secret from the server, which logs it and lists it so an
//! operator can ban it, and it is never sent to other players.

use std::fmt;
use std::str::FromStr;

/// A player's identity, stable across sessions. See the [module docs](self),
/// including **how little it can be trusted** on an open server.
#[derive(Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct PlayerId([u8; PlayerId::BYTES]);

impl PlayerId {
    /// The id's length on the wire and on disk.
    pub const BYTES: usize = 16;

    /// The id these bytes are — its wire and file form.
    #[must_use]
    pub const fn from_bytes(bytes: [u8; Self::BYTES]) -> Self {
        Self(bytes)
    }

    /// The id's bytes, as [`from_bytes`](Self::from_bytes) takes them.
    #[must_use]
    pub const fn to_bytes(self) -> [u8; Self::BYTES] {
        self.0
    }

    /// An id from `seed`, the same on every run and every platform: for
    /// tests, headless runs and in-process sessions, which must not depend on
    /// what a machine's config directory holds. Distinct seeds give distinct
    /// ids for any count of seeds a test will use, but nothing stops a seed
    /// colliding with an id some client drew.
    #[must_use]
    pub fn from_seed(seed: u64) -> Self {
        let mut bytes = [0u8; Self::BYTES];
        let (high, low) = bytes.split_at_mut(Self::BYTES / 2);
        high.copy_from_slice(&crate::rand::hash_u64(seed, 0).to_be_bytes());
        low.copy_from_slice(&crate::rand::hash_u64(seed, 1).to_be_bytes());
        Self(bytes)
    }
}

/// Lowercase hexadecimal, every byte: what a server prints and what
/// [`FromStr`] reads back.
impl fmt::Display for PlayerId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        for byte in self.0 {
            write!(f, "{byte:02x}")?;
        }
        Ok(())
    }
}

impl fmt::Debug for PlayerId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "PlayerId({self})")
    }
}

/// Why a string is no [`PlayerId`].
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ParsePlayerIdError {
    /// Not twice [`PlayerId::BYTES`] characters long.
    Length(usize),
    /// A character that is not a hexadecimal digit.
    NotHex(char),
}

impl fmt::Display for ParsePlayerIdError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Length(len) => write!(
                f,
                "a player id is {} hex digits, not {len}",
                PlayerId::BYTES * 2
            ),
            Self::NotHex(c) => write!(f, "{c:?} is not a hex digit"),
        }
    }
}

impl std::error::Error for ParsePlayerIdError {}

/// Reads [`Display`](fmt::Display)'s form back, either case.
impl FromStr for PlayerId {
    type Err = ParsePlayerIdError;

    fn from_str(text: &str) -> Result<Self, Self::Err> {
        let digits: Vec<char> = text.chars().collect();
        if digits.len() != Self::BYTES * 2 {
            return Err(ParsePlayerIdError::Length(digits.len()));
        }
        let digit = |c: char| {
            c.to_digit(16)
                .and_then(|value| u8::try_from(value).ok())
                .ok_or(ParsePlayerIdError::NotHex(c))
        };
        let mut bytes = [0u8; Self::BYTES];
        let (pairs, _) = digits.as_chunks::<2>();
        for (byte, &[high, low]) in bytes.iter_mut().zip(pairs) {
            *byte = (digit(high)? << 4) | digit(low)?;
        }
        Ok(Self(bytes))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// **The printed form reads back to the same id**, upper case included,
    /// and every byte is in it — so an id an operator copies off a status line
    /// is the id that was there.
    #[test]
    fn the_printed_form_reads_back_to_the_same_id() {
        let id = PlayerId::from_bytes(std::array::from_fn(|i| (i as u8) * 17));
        let text = id.to_string();
        assert_eq!(text, "00112233445566778899aabbccddeeff");
        assert_eq!(text.parse::<PlayerId>(), Ok(id));
        assert_eq!(text.to_uppercase().parse::<PlayerId>(), Ok(id));
        assert_eq!(format!("{id:?}"), format!("PlayerId({text})"));
    }

    /// **A string that is not exactly an id is refused**, naming why — one
    /// digit short, one long, and one that is not hex.
    #[test]
    fn a_string_that_is_not_an_id_is_refused() {
        let short = "0".repeat(31);
        assert_eq!(
            short.parse::<PlayerId>(),
            Err(ParsePlayerIdError::Length(31))
        );
        assert_eq!(
            "0".repeat(33).parse::<PlayerId>(),
            Err(ParsePlayerIdError::Length(33))
        );
        let not_hex = format!("{}g", "0".repeat(31));
        assert_eq!(
            not_hex.parse::<PlayerId>(),
            Err(ParsePlayerIdError::NotHex('g'))
        );
    }

    /// **A seed gives the same id every time, and another seed another id** —
    /// both halves, so an id built from one half of the hash and zeros would
    /// fail.
    #[test]
    fn a_seed_gives_the_same_id_every_time_and_another_seed_another() {
        assert_eq!(PlayerId::from_seed(7), PlayerId::from_seed(7));
        let ids: std::collections::HashSet<PlayerId> = (0..64).map(PlayerId::from_seed).collect();
        assert_eq!(ids.len(), 64, "two seeds gave one id");
        let bytes = PlayerId::from_seed(7).to_bytes();
        let (high, low) = bytes.split_at(PlayerId::BYTES / 2);
        assert_ne!(high, low, "both halves came from one hash");
        assert!(low.iter().any(|&byte| byte != 0), "the low half is empty");
    }
}
