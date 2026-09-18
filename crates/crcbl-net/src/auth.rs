//! Per-session message authentication.
//!
//! Nothing below the handshake authenticates a packet on its own: tick ids,
//! sector ids and session ids all travel in cleartext, so an off-path attacker
//! who can see one snapshot can forge an ack, an input, or a delta that the
//! peer would otherwise accept. This module closes that hole by keying a MAC
//! with the 32-byte [`ResumeToken`] the handshake already exchanges.
//!
//! # Envelope
//!
//! ```text
//! tag:      u8 = 0x40
//! counter:  u64 LE   (replay counter, starts at 1)
//! payload:  bytes    (the inner message, tag byte and all)
//! mac:      16 bytes (HMAC-SHA256(session_key, tag || counter || payload)[..16])
//! ```
//!
//! The counter is inside the MAC input, so a captured packet cannot be
//! replayed under a different counter, and [`ReplayWindow`] rejects a replay
//! of the counter it was captured with.
//!
//! # What this is and is not
//!
//! This authenticates *and* orders; it does not encrypt. Payloads stay
//! readable on the wire — snapshot confidentiality is a separate decision that
//! needs a key exchange the handshake does not have (the resume token travels
//! in the clear inside `Accept`, so an on-path observer of the handshake
//! learns the key). Against the threat this protocol actually names — a
//! spoofer who can send packets but did not see the handshake — a shared-secret
//! MAC is the right primitive.
//!
//! The MAC is HMAC-SHA256 truncated to 128 bits, built on the workspace's own
//! [`crcbl_shaders::sha256`] so no third-party crypto dependency enters the
//! build graph.

use crate::types::ResumeToken;
use crcbl_shaders::sha256::sha256;

/// First byte of an authenticated envelope. Distinct from every message tag.
pub const AUTH_TAG: u8 = 0x40;
/// Truncated HMAC length carried by an envelope.
pub const MAC_BYTES: usize = 16;
/// Bytes an envelope adds to the payload it wraps.
pub const AUTH_OVERHEAD: usize = 1 + 8 + MAC_BYTES;

const HMAC_BLOCK_BYTES: usize = 64;
/// Domain separator so the session key is not the resume token itself: a
/// server that leaked a MAC key would not thereby leak the reconnect
/// credential.
const SESSION_KEY_INFO: &[u8] = b"crcbl session key v1";

// ── Errors ────────────────────────────────────────────────────────────────────

/// Why an authenticated envelope was rejected.
#[derive(Debug, Clone, Copy, PartialEq, Eq, thiserror::Error)]
pub enum AuthError {
    /// The payload is not an authenticated envelope at all.
    #[error("message is not authenticated")]
    NotSealed,
    /// The envelope is shorter than its own framing.
    #[error("authenticated envelope is truncated")]
    TooShort,
    /// The MAC does not match — forged, corrupted, or keyed for another session.
    #[error("message authentication code does not match")]
    BadMac,
    /// The counter was already used, or has fallen out of the replay window.
    #[error("replayed or out-of-window counter: {0}")]
    Replayed(u64),
    /// The 2^64 counter space for this key is exhausted; rekey by reconnecting.
    #[error("replay counter space exhausted")]
    CounterExhausted,
}

// ── SessionKey ────────────────────────────────────────────────────────────────

/// A per-session MAC key derived from the session's [`ResumeToken`].
///
/// `Debug` redacts the secret, as [`ResumeToken`]'s does.
#[derive(Clone, Copy)]
pub struct SessionKey([u8; 32]);

impl SessionKey {
    /// Derive the MAC key for the session identified by `token`.
    ///
    /// Rotating the resume token rotates the key, which is what makes a
    /// reconnect start a fresh counter space.
    #[must_use]
    pub fn derive(token: &ResumeToken) -> Self {
        Self(hmac_sha256(token.as_bytes(), SESSION_KEY_INFO))
    }
}

impl std::fmt::Debug for SessionKey {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("SessionKey([REDACTED])")
    }
}

// ── Sealing ───────────────────────────────────────────────────────────────────

/// Wrap `payload` in an authenticated envelope carrying `counter`.
#[must_use]
pub fn seal(key: &SessionKey, counter: u64, payload: &[u8]) -> Vec<u8> {
    let mut out = Vec::with_capacity(AUTH_OVERHEAD + payload.len());
    out.push(AUTH_TAG);
    out.extend_from_slice(&counter.to_le_bytes());
    out.extend_from_slice(payload);
    let mac = hmac_sha256(&key.0, &out);
    out.extend_from_slice(&mac[..MAC_BYTES]);
    out
}

/// Verify an envelope and return `(counter, payload)`.
///
/// Verification is total on arbitrary bytes and never panics. It does **not**
/// consult a [`ReplayWindow`] — use [`SessionCrypto::open`] for that.
pub fn open<'a>(key: &SessionKey, envelope: &'a [u8]) -> Result<(u64, &'a [u8]), AuthError> {
    if envelope.first() != Some(&AUTH_TAG) {
        return Err(AuthError::NotSealed);
    }
    if envelope.len() < AUTH_OVERHEAD {
        return Err(AuthError::TooShort);
    }
    let (signed, mac) = envelope.split_at(envelope.len() - MAC_BYTES);
    let expected = hmac_sha256(&key.0, signed);
    if !constant_time_eq(&expected[..MAC_BYTES], mac) {
        return Err(AuthError::BadMac);
    }
    let counter = u64::from_le_bytes(signed[1..9].try_into().expect("9 bytes of framing"));
    Ok((counter, &signed[9..]))
}

// ── ReplayWindow ──────────────────────────────────────────────────────────────

/// Sliding 64-slot window over accepted replay counters.
///
/// Snapshots and acks travel unreliably, so counters legitimately arrive out
/// of order; the window accepts any counter newer than `highest - 64` that has
/// not been seen, and rejects everything else.
#[derive(Debug, Default, Clone, Copy)]
pub struct ReplayWindow {
    highest: u64,
    /// Bit `n` is set when counter `highest - n` has been accepted.
    seen: u64,
}

impl ReplayWindow {
    /// Width of the out-of-order tolerance, in counters.
    pub const WIDTH: u64 = 64;

    /// Accept `counter` if it is fresh, marking it used. Zero is never valid,
    /// so a default window starts genuinely empty.
    #[must_use]
    pub fn accept(&mut self, counter: u64) -> bool {
        if counter == 0 {
            return false;
        }
        if counter > self.highest {
            let shift = counter - self.highest;
            self.seen = if shift >= Self::WIDTH {
                0
            } else {
                self.seen << shift
            };
            self.seen |= 1;
            self.highest = counter;
            return true;
        }
        let behind = self.highest - counter;
        if behind >= Self::WIDTH {
            return false;
        }
        let mask = 1u64 << behind;
        if self.seen & mask != 0 {
            return false;
        }
        self.seen |= mask;
        true
    }
}

// ── SessionCrypto ─────────────────────────────────────────────────────────────

/// One direction-agnostic authenticated channel: a key, an outbound counter,
/// and the inbound replay window.
///
/// Both peers derive the same key from the same resume token and keep
/// independent counters, so a packet the peer sent can never be reflected back
/// as one this side sent — the counters are per-sender and the window only
/// ever sees the peer's.
#[derive(Debug)]
pub struct SessionCrypto {
    key: SessionKey,
    next_counter: u64,
    replay: ReplayWindow,
}

impl SessionCrypto {
    /// Start a channel keyed by `key` with an empty counter space.
    #[must_use]
    pub fn new(key: SessionKey) -> Self {
        Self {
            key,
            next_counter: 1,
            replay: ReplayWindow::default(),
        }
    }

    /// Start a channel keyed by the session's resume token.
    #[must_use]
    pub fn from_token(token: &ResumeToken) -> Self {
        Self::new(SessionKey::derive(token))
    }

    /// Seal `payload` under the next outbound counter.
    ///
    /// Counter zero is not a valid counter, so wrapping past `u64::MAX` lands
    /// on a value that doubles as the "this key is spent" marker.
    pub fn seal(&mut self, payload: &[u8]) -> Result<Vec<u8>, AuthError> {
        if self.next_counter == 0 {
            return Err(AuthError::CounterExhausted);
        }
        let counter = self.next_counter;
        self.next_counter = counter.wrapping_add(1);
        Ok(seal(&self.key, counter, payload))
    }

    /// Verify `envelope` and reject replays.
    pub fn open<'a>(&mut self, envelope: &'a [u8]) -> Result<&'a [u8], AuthError> {
        let (counter, payload) = open(&self.key, envelope)?;
        if !self.replay.accept(counter) {
            return Err(AuthError::Replayed(counter));
        }
        Ok(payload)
    }
}

// ── HMAC-SHA256 ───────────────────────────────────────────────────────────────

/// HMAC-SHA256 (RFC 2104) over the workspace's own SHA-256.
fn hmac_sha256(key: &[u8], data: &[u8]) -> [u8; 32] {
    let mut block = [0u8; HMAC_BLOCK_BYTES];
    if key.len() > HMAC_BLOCK_BYTES {
        block[..32].copy_from_slice(&sha256(key));
    } else {
        block[..key.len()].copy_from_slice(key);
    }

    let mut inner = Vec::with_capacity(HMAC_BLOCK_BYTES + data.len());
    for byte in block {
        inner.push(byte ^ 0x36);
    }
    inner.extend_from_slice(data);
    let inner_digest = sha256(&inner);

    let mut outer = Vec::with_capacity(HMAC_BLOCK_BYTES + 32);
    for byte in block {
        outer.push(byte ^ 0x5c);
    }
    outer.extend_from_slice(&inner_digest);
    sha256(&outer)
}

/// Compare two equal-length byte slices without an early return, so a
/// mismatch does not reveal how many leading bytes matched.
fn constant_time_eq(left: &[u8], right: &[u8]) -> bool {
    if left.len() != right.len() {
        return false;
    }
    let mut difference = 0u8;
    for (l, r) in left.iter().zip(right) {
        difference |= l ^ r;
    }
    difference == 0
}

// ── Tests ─────────────────────────────────────────────────────────────────────

#[cfg(test)]
#[path = "auth/tests.rs"]
mod tests;
