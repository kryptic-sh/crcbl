//! The key schedule: one X25519 output in, two directional keys out.
//!
//! ```text
//! transcript = SHA-256(PROTOCOL_NAME || protocol_id (u32 LE)
//!                      || client_public || server_public)
//! k_c2s, k_s2c = HKDF(transcript, shared_secret, 2)     (Noise §4.3)
//! ```
//!
//! `HKDF` is Noise's two-output form, which is RFC 5869 extract-then-expand
//! with an empty `info` (see [`super::kdf`]). The transcript takes the place of
//! Noise's chaining key and binds everything both peers must agree on: the
//! cipher suite by name, the build's protocol id, and both public keys in a
//! fixed client-then-server order. A peer that saw a different key — a
//! substituted one, or the two swapped — derives different session keys, and
//! its first datagram fails to open rather than being read. **This is a key
//! schedule, not a Noise handshake pattern**: there is one DH, no static keys
//! are authenticated, and nothing here decides what goes on the wire when.
//!
//! The X25519 output comes from [`super::agreement`], which has already
//! refused the all-zero one RFC 7748 §6.1 says to check for; this module only
//! ever sees a contributory secret.

use std::fmt;

use chacha20poly1305::{KeyInit, XChaCha20Poly1305};
use hmac::digest::zeroize::Zeroizing;
use sha2::{Digest, Sha256};

use super::kdf::{HASH_BYTES, hkdf_expand, hkdf_extract};
use super::nonce::Direction;
use super::{Opener, Sealer};

/// The cipher suite by name, the first thing the transcript hashes, in the
/// spirit of a Noise protocol name (§8): a peer running another construction
/// under the same keys derives other session keys.
pub const PROTOCOL_NAME: &[u8] = b"crcbl-seal/1_X25519_HKDF-SHA256_XChaCha20-Poly1305";

/// Bytes of an X25519 public key, and of the shared secret it agrees (RFC
/// 7748 §5).
pub const X25519_BYTES: usize = 32;

/// Bytes of one direction's XChaCha20-Poly1305 key.
const SESSION_KEY_BYTES: usize = 32;

const _: () = assert!(SESSION_KEY_BYTES == HASH_BYTES);

/// Which end of the link this side is. It decides which derived key seals and
/// which opens.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Role {
    /// The side that connected.
    Client,
    /// The side that accepted.
    Server,
}

impl Role {
    fn sends(self) -> Direction {
        match self {
            Self::Client => Direction::ClientToServer,
            Self::Server => Direction::ServerToClient,
        }
    }

    fn receives(self) -> Direction {
        match self {
            Self::Client => Direction::ServerToClient,
            Self::Server => Direction::ClientToServer,
        }
    }
}

/// The two directional keys. Wiped on drop, and never printed.
pub(crate) struct SessionKeys {
    client_to_server: Zeroizing<[u8; SESSION_KEY_BYTES]>,
    server_to_client: Zeroizing<[u8; SESSION_KEY_BYTES]>,
}

impl fmt::Debug for SessionKeys {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("SessionKeys([REDACTED])")
    }
}

impl SessionKeys {
    /// Run the key schedule in the module docs.
    pub(crate) fn derive(
        shared_secret: &[u8; X25519_BYTES],
        client_public: &[u8; X25519_BYTES],
        server_public: &[u8; X25519_BYTES],
        protocol_id: u32,
    ) -> Self {
        let transcript: [u8; HASH_BYTES] = Sha256::new()
            .chain_update(PROTOCOL_NAME)
            .chain_update(protocol_id.to_le_bytes())
            .chain_update(client_public)
            .chain_update(server_public)
            .finalize()
            .into();
        let prk = hkdf_extract(&transcript, shared_secret);
        let mut okm = Zeroizing::new([0u8; 2 * SESSION_KEY_BYTES]);
        hkdf_expand(&prk, &[], &mut okm[..]);
        let (first, second) = okm.split_at(SESSION_KEY_BYTES);
        let mut keys = Self {
            client_to_server: Zeroizing::new([0; SESSION_KEY_BYTES]),
            server_to_client: Zeroizing::new([0; SESSION_KEY_BYTES]),
        };
        keys.client_to_server.copy_from_slice(first);
        keys.server_to_client.copy_from_slice(second);
        keys
    }

    fn key(&self, direction: Direction) -> &[u8; SESSION_KEY_BYTES] {
        match direction {
            Direction::ClientToServer => &self.client_to_server,
            Direction::ServerToClient => &self.server_to_client,
        }
    }

    fn cipher(&self, direction: Direction) -> XChaCha20Poly1305 {
        XChaCha20Poly1305::new_from_slice(self.key(direction))
            .expect("a session key is the cipher's key length")
    }

    /// This side's sealer and opener.
    pub(crate) fn into_channel(self, role: Role) -> (Sealer, Opener) {
        (
            Sealer::new(self.cipher(role.sends()), role.sends()),
            Opener::new(self.cipher(role.receives()), role.receives()),
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::seal::kdf::hmac_sha256;

    const SHARED: [u8; X25519_BYTES] = [0x11; X25519_BYTES];
    const CLIENT: [u8; X25519_BYTES] = [0xC1; X25519_BYTES];
    const SERVER: [u8; X25519_BYTES] = [0x5E; X25519_BYTES];
    const PROTOCOL: u32 = 0x4352_4342;

    fn keys(client: &[u8; 32], server: &[u8; 32], protocol_id: u32) -> SessionKeys {
        SessionKeys::derive(&SHARED, client, server, protocol_id)
    }

    /// The schedule written out as Noise §4.3 states `HKDF`, without the RFC
    /// 5869 loop in between: `temp_key = HMAC(ck, ikm)`, `output1 =
    /// HMAC(temp_key, 0x01)`, `output2 = HMAC(temp_key, output1 || 0x02)`.
    #[test]
    fn the_session_keys_are_noise_hkdf_over_the_transcript() {
        let mut transcript = PROTOCOL_NAME.to_vec();
        transcript.extend_from_slice(&PROTOCOL.to_le_bytes());
        transcript.extend_from_slice(&CLIENT);
        transcript.extend_from_slice(&SERVER);
        let chaining_key: [u8; 32] = Sha256::digest(&transcript).into();
        let temp_key = hmac_sha256(&chaining_key, &[&SHARED]);
        let output1 = hmac_sha256(&temp_key, &[&[0x01]]);
        let output2 = hmac_sha256(&temp_key, &[&output1, &[0x02]]);

        let keys = keys(&CLIENT, &SERVER, PROTOCOL);
        assert_eq!(*keys.client_to_server, output1);
        assert_eq!(*keys.server_to_client, output2);
    }

    #[test]
    fn the_two_directions_get_different_keys() {
        let keys = keys(&CLIENT, &SERVER, PROTOCOL);
        assert_ne!(*keys.client_to_server, *keys.server_to_client);
    }

    /// Each direction's cipher is keyed with that direction's key: with the
    /// direction byte held equal, a datagram sealed under one direction's key
    /// does not open under the other's.
    #[test]
    fn each_direction_seals_under_its_own_key() {
        let keys = keys(&CLIENT, &SERVER, PROTOCOL);
        let direction = Direction::ClientToServer;
        let mut sealer = Sealer::new(keys.cipher(Direction::ClientToServer), direction);
        let mut wrong_key = Opener::new(keys.cipher(Direction::ServerToClient), direction);
        let mut right_key = Opener::new(keys.cipher(Direction::ClientToServer), direction);
        let sealed = sealer.seal(b"packet").unwrap();
        assert_eq!(wrong_key.open(&sealed), Err(crate::seal::OpenError::Forged));
        assert_eq!(right_key.open(&sealed).unwrap(), b"packet");
    }

    /// Each input the transcript binds moves both keys: a swapped pair of
    /// public keys, a substituted one, another build's protocol id.
    #[test]
    fn every_bound_input_changes_the_keys() {
        let reference = keys(&CLIENT, &SERVER, PROTOCOL);
        let flip = |key: [u8; 32]| {
            let mut key = key;
            key[31] ^= 0x01;
            key
        };
        for (what, other) in [
            ("swapped public keys", keys(&SERVER, &CLIENT, PROTOCOL)),
            (
                "a substituted client key",
                keys(&flip(CLIENT), &SERVER, PROTOCOL),
            ),
            (
                "a substituted server key",
                keys(&CLIENT, &flip(SERVER), PROTOCOL),
            ),
            ("another protocol id", keys(&CLIENT, &SERVER, PROTOCOL + 1)),
        ] {
            assert_ne!(
                *other.client_to_server, *reference.client_to_server,
                "{what}"
            );
            assert_ne!(
                *other.server_to_client, *reference.server_to_client,
                "{what}"
            );
        }
    }

    /// Neither the key schedule's output nor the sealer and opener built from
    /// it print key bytes, in hex or in `Debug`'s decimal array form.
    #[test]
    fn debug_never_prints_a_key() {
        let keys = keys(&CLIENT, &SERVER, PROTOCOL);
        assert_eq!(format!("{keys:?}"), "SessionKeys([REDACTED])");
        let mut forms = Vec::new();
        for key in [&*keys.client_to_server, &*keys.server_to_client] {
            forms.push(
                key.iter()
                    .map(|byte| format!("{byte:02x}"))
                    .collect::<String>(),
            );
            let decimal = format!("{:?}", &key[..4]);
            forms.push(decimal.trim_matches(['[', ']']).to_owned());
        }
        let (sealer, opener) = keys.into_channel(Role::Client);
        let printed = format!("{sealer:?} {opener:?}");
        for form in forms {
            assert!(!printed.contains(&form), "{printed} contains {form}");
        }
    }
}
