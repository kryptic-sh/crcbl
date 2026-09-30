//! Nonces derived from direction and counter, unique by construction.
//!
//! XChaCha20-Poly1305 takes a 24-byte nonce (draft-irtf-cfrg-xchacha-03 §2).
//! That size is what makes a *random* nonce safe for it, but nothing here draws
//! one: `crcbl-net` generates no randomness at all, and a derived nonce is
//! unique by arithmetic rather than by probability.
//!
//! ```text
//! byte 0:      direction
//! bytes 1..9:  counter, u64 LE
//! bytes 9..24: zero
//! ```
//!
//! Two nonces are equal only when their direction and counter both are, and a
//! [`super::Sealer`] never seals twice under one counter — it refuses at
//! exhaustion rather than wrap — so no nonce repeats under one key. Each
//! direction also has its own key; the direction byte is a second, independent
//! guarantee on top of that, so a datagram reflected back at its sender fails
//! to open even if the two keys were ever equal.

/// Which way a datagram travels. Each has its own key and its own nonce space.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Direction {
    ClientToServer,
    ServerToClient,
}

impl Direction {
    /// The nonce's first byte. Nonzero both ways, so an all-zero nonce is never
    /// produced — not a requirement of the cipher, but a zeroed buffer
    /// mistaken for a nonce then cannot collide with a real one.
    const fn byte(self) -> u8 {
        match self {
            Self::ClientToServer => 0x01,
            Self::ServerToClient => 0x02,
        }
    }
}

/// XChaCha20-Poly1305's nonce length (draft-irtf-cfrg-xchacha-03 §2).
pub(crate) const NONCE_BYTES: usize = 24;

const DIRECTION_OFFSET: usize = 0;
const COUNTER_OFFSET: usize = DIRECTION_OFFSET + 1;
const COUNTER_END: usize = COUNTER_OFFSET + size_of::<u64>();

const _: () = assert!(COUNTER_END <= NONCE_BYTES);

/// The nonce for the datagram `direction` seals under `counter`.
pub(crate) fn nonce(direction: Direction, counter: u64) -> [u8; NONCE_BYTES] {
    let mut nonce = [0u8; NONCE_BYTES];
    nonce[DIRECTION_OFFSET] = direction.byte();
    nonce[COUNTER_OFFSET..COUNTER_END].copy_from_slice(&counter.to_le_bytes());
    nonce
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashSet;

    /// Counters from both ends of the space and around every byte boundary,
    /// in both directions: every (direction, counter) pair gets its own nonce.
    #[test]
    fn every_direction_and_counter_pair_has_its_own_nonce() {
        let mut counters: Vec<u64> = (0..4_096).collect();
        counters.extend((0..4_096).map(|n| u64::MAX - n));
        for shift in (8..64).step_by(8) {
            let boundary = 1u64 << shift;
            counters.extend(boundary - 2..boundary + 2);
        }
        counters.sort_unstable();
        counters.dedup();
        let mut seen = HashSet::new();
        let mut pairs = 0;
        for direction in [Direction::ClientToServer, Direction::ServerToClient] {
            for &counter in &counters {
                pairs += 1;
                assert!(
                    seen.insert(nonce(direction, counter)),
                    "{direction:?} counter {counter} repeats a nonce"
                );
            }
        }
        assert_eq!(seen.len(), pairs);
    }

    /// The layout is the documented one, so the peer — any implementation of
    /// it — derives the same nonce.
    #[test]
    fn the_nonce_is_the_direction_byte_then_the_counter_then_zeros() {
        let nonce = nonce(Direction::ServerToClient, 0x0807_0605_0403_0201);
        assert_eq!(nonce[0], 0x02);
        assert_eq!(nonce[1..9], [1, 2, 3, 4, 5, 6, 7, 8]);
        assert!(nonce[9..].iter().all(|&byte| byte == 0));
    }
}
