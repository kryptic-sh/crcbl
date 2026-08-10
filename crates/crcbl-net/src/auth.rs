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
mod tests {
    use super::*;

    fn key() -> SessionKey {
        SessionKey::derive(&ResumeToken::from_bytes([0xA5; 32]))
    }

    #[test]
    fn hmac_matches_rfc4231_test_case_1() {
        // RFC 4231 §4.2: key = 0x0b × 20, data = "Hi There".
        let mac = hmac_sha256(&[0x0b; 20], b"Hi There");
        let expected: [u8; 32] = [
            0xb0, 0x34, 0x4c, 0x61, 0xd8, 0xdb, 0x38, 0x53, 0x5c, 0xa8, 0xaf, 0xce, 0xaf, 0x0b,
            0xf1, 0x2b, 0x88, 0x1d, 0xc2, 0x00, 0xc9, 0x83, 0x3d, 0xa7, 0x26, 0xe9, 0x37, 0x6c,
            0x2e, 0x32, 0xcf, 0xf7,
        ];
        assert_eq!(mac, expected);
    }

    #[test]
    fn hmac_matches_rfc4231_test_case_2() {
        // RFC 4231 §4.3: key = "Jefe", data = "what do ya want for nothing?".
        let mac = hmac_sha256(b"Jefe", b"what do ya want for nothing?");
        let expected: [u8; 32] = [
            0x5b, 0xdc, 0xc1, 0x46, 0xbf, 0x60, 0x75, 0x4e, 0x6a, 0x04, 0x24, 0x26, 0x08, 0x95,
            0x75, 0xc7, 0x5a, 0x00, 0x3f, 0x08, 0x9d, 0x27, 0x39, 0x83, 0x9d, 0xec, 0x58, 0xb9,
            0x64, 0xec, 0x38, 0x43,
        ];
        assert_eq!(mac, expected);
    }

    #[test]
    fn hmac_hashes_oversized_keys() {
        // RFC 4231 §4.6: key = 0xaa × 131 (longer than the block), so the key
        // is hashed first.
        let mac = hmac_sha256(
            &[0xaa; 131],
            b"Test Using Larger Than Block-Size Key - Hash Key First",
        );
        let expected: [u8; 32] = [
            0x60, 0xe4, 0x31, 0x59, 0x1e, 0xe0, 0xb6, 0x7f, 0x0d, 0x8a, 0x26, 0xaa, 0xcb, 0xf5,
            0xb7, 0x7f, 0x8e, 0x0b, 0xc6, 0x21, 0x37, 0x28, 0xc5, 0x14, 0x05, 0x46, 0x04, 0x0f,
            0x0e, 0xe3, 0x7f, 0x54,
        ];
        assert_eq!(mac, expected);
    }

    #[test]
    fn an_envelope_opens_back_to_the_counter_and_payload_it_sealed() {
        let key = key();
        let sealed = seal(&key, 7, b"payload");
        let (counter, payload) = open(&key, &sealed).expect("own envelope verifies");
        assert_eq!(counter, 7);
        assert_eq!(payload, b"payload");
    }

    #[test]
    fn envelope_is_tagged_and_sized() {
        let sealed = seal(&key(), 1, b"abc");
        assert_eq!(sealed[0], AUTH_TAG);
        assert_eq!(sealed.len(), AUTH_OVERHEAD + 3);
    }

    #[test]
    fn a_different_key_does_not_verify() {
        let sealed = seal(&key(), 1, b"payload");
        let other = SessionKey::derive(&ResumeToken::from_bytes([0x5A; 32]));
        assert_eq!(open(&other, &sealed), Err(AuthError::BadMac));
    }

    #[test]
    fn tampering_with_any_field_fails() {
        let key = key();
        let sealed = seal(&key, 3, b"payload");
        for index in 0..sealed.len() {
            let mut forged = sealed.clone();
            forged[index] ^= 0x01;
            if index == 0 {
                assert_eq!(open(&key, &forged), Err(AuthError::NotSealed));
            } else {
                assert_eq!(open(&key, &forged), Err(AuthError::BadMac));
            }
        }
    }

    #[test]
    fn unsealed_and_truncated_payloads_are_rejected() {
        let key = key();
        assert_eq!(open(&key, &[]), Err(AuthError::NotSealed));
        assert_eq!(open(&key, &[0x30, 0x00]), Err(AuthError::NotSealed));
        assert_eq!(open(&key, &[AUTH_TAG; 4]), Err(AuthError::TooShort));
        // An envelope of exactly the overhead carries an empty payload.
        let empty = seal(&key, 1, &[]);
        assert_eq!(empty.len(), AUTH_OVERHEAD);
        assert_eq!(open(&key, &empty).expect("verifies").1, b"");
    }

    #[test]
    fn open_never_panics_on_arbitrary_bytes() {
        let key = key();
        let mut state: u64 = 0x4352_4342_4c41_5554;
        for _ in 0..2_000 {
            state ^= state << 13;
            state ^= state >> 7;
            state ^= state << 17;
            let len = (state % 96) as usize;
            let mut payload = Vec::with_capacity(len + 1);
            payload.push(AUTH_TAG);
            let mut h = state;
            for _ in 0..len {
                h ^= h << 13;
                h ^= h >> 7;
                h ^= h << 17;
                payload.push(h as u8);
            }
            let _ = open(&key, &payload);
        }
    }

    #[test]
    fn replay_window_rejects_repeats_and_accepts_reorder() {
        let mut window = ReplayWindow::default();
        assert!(!window.accept(0));
        assert!(window.accept(1));
        assert!(!window.accept(1));
        assert!(window.accept(5));
        assert!(window.accept(3));
        assert!(!window.accept(3));
        assert!(window.accept(4));
        assert!(window.accept(2));
    }

    #[test]
    fn replay_window_drops_counters_past_its_width() {
        let mut window = ReplayWindow::default();
        assert!(window.accept(1));
        assert!(window.accept(1 + ReplayWindow::WIDTH));
        // 1 is now exactly `WIDTH` behind the head and no longer representable.
        assert!(!window.accept(1));
        assert!(window.accept(2));
        // A jump clears the window entirely rather than leaving stale bits.
        assert!(window.accept(10_000));
        assert!(!window.accept(2));
        assert!(window.accept(9_999));
    }

    #[test]
    fn session_crypto_rejects_a_replayed_packet() {
        let token = ResumeToken::from_bytes([7; 32]);
        let mut sender = SessionCrypto::from_token(&token);
        let mut receiver = SessionCrypto::from_token(&token);

        let first = sender.seal(b"ack-1").expect("counter available");
        let second = sender.seal(b"ack-2").expect("counter available");
        assert_eq!(receiver.open(&first).expect("verifies"), b"ack-1");
        assert_eq!(receiver.open(&second).expect("verifies"), b"ack-2");
        assert_eq!(receiver.open(&first), Err(AuthError::Replayed(1)));
        assert_eq!(receiver.open(&second), Err(AuthError::Replayed(2)));
    }

    #[test]
    fn session_crypto_counters_start_at_one_and_increment() {
        let mut crypto = SessionCrypto::from_token(&ResumeToken::from_bytes([1; 32]));
        let key = SessionKey::derive(&ResumeToken::from_bytes([1; 32]));
        for expected in 1..=3u64 {
            let sealed = crypto.seal(b"x").expect("counter available");
            assert_eq!(open(&key, &sealed).expect("verifies").0, expected);
        }
    }

    #[test]
    fn exhausted_counter_space_refuses_to_seal() {
        let mut crypto = SessionCrypto::from_token(&ResumeToken::from_bytes([2; 32]));
        crypto.next_counter = u64::MAX;
        assert!(crypto.seal(b"last").is_ok());
        assert_eq!(crypto.seal(b"over"), Err(AuthError::CounterExhausted));
    }

    #[test]
    fn key_derivation_is_token_specific_and_not_the_token() {
        let token = ResumeToken::from_bytes([0x11; 32]);
        let a = SessionKey::derive(&token);
        let b = SessionKey::derive(&token);
        let c = SessionKey::derive(&ResumeToken::from_bytes([0x12; 32]));
        assert_eq!(a.0, b.0);
        assert_ne!(a.0, c.0);
        assert_ne!(a.0, [0x11; 32]);
    }

    #[test]
    fn debug_redacts_the_key() {
        let debug = format!("{:?}", key());
        assert!(debug.contains("REDACTED"));
        assert!(!debug.contains("165"));
    }

    #[test]
    fn constant_time_eq_compares_contents_and_length() {
        assert!(constant_time_eq(b"abc", b"abc"));
        assert!(!constant_time_eq(b"abc", b"abd"));
        assert!(!constant_time_eq(b"abc", b"ab"));
    }

    #[test]
    fn an_auth_error_names_the_check_that_failed_and_the_offending_counter() {
        assert_eq!(
            AuthError::Replayed(9).to_string(),
            "replayed or out-of-window counter: 9"
        );
        assert_eq!(
            AuthError::BadMac.to_string(),
            "message authentication code does not match"
        );
    }
}
