//! The seal every datagram on a network transport travels under:
//! XChaCha20-Poly1305 with nonces derived rather than drawn, keyed by
//! HKDF-SHA256 over an X25519 output.
//!
//! The encryption rule in `docs/notes/simulation.md` puts every packet on a
//! network transport under AEAD from the first packet after the hello, with no
//! plaintext mode and no switch to turn it off. This module is that AEAD, as
//! pure logic like [`crate::reliable`] beneath it: bytes in, bytes out, no
//! socket, and no randomness — key material comes from the caller.
//!
//! ```text
//! Endpoint::poll_outgoing ── Sealer::seal ── socket ── Opener::open ── Endpoint::receive_datagram
//! ```
//!
//! # Datagram
//!
//! ```text
//! tag:        u8 = SEALED_TAG
//! counter:    u64 LE   (this direction's, from FIRST_COUNTER, never wrapping)
//! ciphertext: the endpoint's packet, header included
//! mac:        16 bytes (Poly1305)
//! ```
//!
//! **The associated data is every clear byte** — the tag byte and the counter
//! — so neither can be altered without the datagram failing to open, and the
//! endpoint's header (sequence, acks, channel) is inside the ciphertext, read
//! and forged by nobody without the key. The tag byte tells a sealed datagram
//! from the plaintext hello the same socket carries before any key exists; it
//! is distinct from every [`crate::codec`] message tag and from
//! [`crate::auth::AUTH_TAG`]. [`SEAL_OVERHEAD`] is what this adds to a packet,
//! and [`crate::reliable::SEAL_RESERVE`] is that number, held back from the
//! datagram budget.
//!
//! # Nonces and replay
//!
//! **The nonce cannot be the endpoint's 16-bit packet sequence**, which wraps.
//! It is the direction plus this layer's own 64-bit counter (see `nonce`), the
//! netcode.io layering: a 64-bit sequence outside, reliable.io's 16-bit one
//! inside. A [`Sealer`] refuses with [`SealError::CounterExhausted`] rather
//! than wrap. An [`Opener`] checks the counter against a
//! [`crate::auth::ReplayWindow`] only after the tag verifies, so a duplicate or
//! a too-old datagram is refused and a forged one cannot move the window.
//!
//! # Keys
//!
//! [`derive_channel`] turns an X25519 output into this side's [`Sealer`] and
//! [`Opener`], one key per direction (see `keys` for the schedule). **Its
//! trust is honest, not strong**: an unauthenticated X25519 exchange defeats a
//! passive observer and is open to anyone who can sit in the middle of the
//! handshake and substitute keys both ways. Nothing here authenticates a
//! public key; a token minted by a trusted source, or an operator-configured
//! pre-shared key, is what would. No competitive-integrity claim rests on it.
//!
//! **The X25519 call itself is not built.** `x25519-dalek` names a `rand_core`
//! the workspace's duplicate-version ban refuses; `docs/backlog.md`, _There is
//! no UDP transport_, has the options. [`derive_channel`] takes its output as
//! bytes until then.
//!
//! # Submodules
//!
//! * `nonce` — the direction-and-counter nonce, unique by construction.
//! * `kdf` — HKDF-SHA256 per RFC 5869, the form Noise §4.3 names.
//! * `keys` — the key schedule and [`derive_channel`].
//! * `channel` — [`Sealer`] and [`Opener`].

mod channel;
pub(crate) mod kdf;
mod keys;
mod nonce;

#[cfg(test)]
pub(crate) mod tests;

use chacha20poly1305::aead::array::typenum::Unsigned;
use chacha20poly1305::{AeadCore, XChaCha20Poly1305};

pub use channel::{OpenError, Opener, SealError, Sealer};
pub use keys::{KeyAgreementError, PROTOCOL_NAME, Role, X25519_BYTES, derive_channel};

/// First byte of a sealed datagram. Distinct from every message tag and from
/// [`crate::auth::AUTH_TAG`], so one socket can carry the plaintext hello and
/// sealed traffic and tell them apart by it.
pub const SEALED_TAG: u8 = 0x60;

/// Bytes of the per-direction counter the nonce derives from.
pub const COUNTER_BYTES: usize = size_of::<u64>();

/// Bytes sent in clear ahead of the ciphertext — the tag byte and the counter
/// — all of them the AEAD's associated data.
pub const SEAL_PREFIX_BYTES: usize = 1 + COUNTER_BYTES;

/// Bytes of the Poly1305 tag XChaCha20-Poly1305 appends.
pub const TAG_BYTES: usize = <XChaCha20Poly1305 as AeadCore>::TagSize::USIZE;

/// Bytes a sealed datagram adds to the packet it carries.
pub const SEAL_OVERHEAD: usize = SEAL_PREFIX_BYTES + TAG_BYTES;

/// The counter a fresh [`Sealer`] seals its first datagram under. Not zero,
/// because [`crate::auth::ReplayWindow`] treats zero as never valid, which is
/// what lets an empty window start genuinely empty.
pub const FIRST_COUNTER: u64 = 1;

const _: () = assert!(
    crate::reliable::MAX_PACKET_BYTES + SEAL_OVERHEAD == crate::reliable::MAX_DATAGRAM_BYTES
);
