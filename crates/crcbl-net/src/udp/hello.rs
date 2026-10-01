//! The plaintext handshake datagrams: the only unsealed ones a
//! [`super::UdpTransport`] sends or reads.
//!
//! Each starts with one prefix:
//!
//! ```text
//! tag:          u8      HELLO_TAG, CHALLENGE_TAG or HELLO_REPLY_TAG
//! version:      u8      TRANSPORT_VERSION
//! protocol_id:  u32 LE  the endpoint protocol id both sides must share
//! nonce:        [u8; HELLO_NONCE_BYTES]  the client's; the answers echo it
//! ```
//!
//! and goes on by kind:
//!
//! ```text
//! hello      public_key [u8; X25519_BYTES]   the client's
//!            token      [u8; TOKEN_BYTES]    NO_TOKEN until a challenge gave one
//! challenge  token      [u8; TOKEN_BYTES]    for the next hello to present
//! reply      public_key [u8; X25519_BYTES]   the server's
//! ```
//!
//! Every kind has one fixed length, and **the hello is the longest**, so
//! neither answer is ever longer than the hello that asked for it — the
//! token field is in every hello, empty or not, for exactly that. Anything
//! of another length, tag, version or protocol id is not the datagram asked
//! for, and the decoders say so without looking further. The decoders are
//! public so the fuzz target in `crates/crcbl-net/fuzz` reaches them.

use crate::seal::X25519_BYTES;

use super::token::TOKEN_BYTES;

/// First byte of a client's hello. Distinct from [`crate::seal::SEALED_TAG`]
/// and from every [`crate::codec`] message tag, as the seal's own is.
pub const HELLO_TAG: u8 = 0x61;

/// First byte of the server's reply to a hello that presented a valid token.
pub const HELLO_REPLY_TAG: u8 = 0x62;

/// First byte of the server's challenge to a hello that presented none, or
/// one that did not verify.
pub const CHALLENGE_TAG: u8 = 0x65;

/// The handshake's own format version. A peer speaking another is not
/// answered: its datagrams do not decode, and its connect times out.
pub const TRANSPORT_VERSION: u8 = 2;

/// Bytes of the client's random nonce, which every answer echoes. Sixteen
/// are far past guessing for an attacker who did not see the hello.
pub const HELLO_NONCE_BYTES: usize = 16;

/// Bytes of the prefix every handshake datagram shares: tag, version,
/// protocol id and nonce.
const PREFIX_BYTES: usize = 1 + 1 + size_of::<u32>() + HELLO_NONCE_BYTES;

/// Bytes of a hello: the prefix, the client's public key and the token
/// field.
pub const HELLO_BYTES: usize = PREFIX_BYTES + X25519_BYTES + TOKEN_BYTES;

/// Bytes of a challenge: the prefix and the token.
pub const CHALLENGE_BYTES: usize = PREFIX_BYTES + TOKEN_BYTES;

/// Bytes of a reply: the prefix and the server's public key.
pub const REPLY_BYTES: usize = PREFIX_BYTES + X25519_BYTES;

const _: () = assert!(CHALLENGE_BYTES <= HELLO_BYTES);
const _: () = assert!(REPLY_BYTES <= HELLO_BYTES);
const _: () = assert!(HELLO_TAG != crate::seal::SEALED_TAG);
const _: () = assert!(HELLO_REPLY_TAG != crate::seal::SEALED_TAG);
const _: () = assert!(CHALLENGE_TAG != crate::seal::SEALED_TAG);
const _: () = assert!(HELLO_TAG != HELLO_REPLY_TAG);
const _: () = assert!(CHALLENGE_TAG != HELLO_TAG && CHALLENGE_TAG != HELLO_REPLY_TAG);
const _: () = assert!(CHALLENGE_TAG != super::discovery::ANNOUNCE_TAG);
const _: () = assert!(CHALLENGE_TAG != super::discovery::QUERY_TAG);

/// Where the nonce starts: after the tag, version and protocol id.
const NONCE_AT: usize = 1 + 1 + size_of::<u32>();

/// A datagram of `N` bytes starting with the prefix for `tag`.
fn with_prefix<const N: usize>(
    tag: u8,
    protocol_id: u32,
    nonce: &[u8; HELLO_NONCE_BYTES],
) -> [u8; N] {
    let mut out = [0; N];
    out[0] = tag;
    out[1] = TRANSPORT_VERSION;
    out[2..NONCE_AT].copy_from_slice(&protocol_id.to_le_bytes());
    out[NONCE_AT..PREFIX_BYTES].copy_from_slice(nonce);
    out
}

/// `datagram` as `N` bytes under the prefix for `tag` and `protocol_id`,
/// with its nonce, or `None` when it is anything else.
fn split_prefix<const N: usize>(
    datagram: &[u8],
    tag: u8,
    protocol_id: u32,
) -> Option<(&[u8; N], [u8; HELLO_NONCE_BYTES])> {
    let datagram: &[u8; N] = datagram.try_into().ok()?;
    if datagram[0] != tag
        || datagram[1] != TRANSPORT_VERSION
        || datagram[2..NONCE_AT] != protocol_id.to_le_bytes()
    {
        return None;
    }
    let mut nonce = [0; HELLO_NONCE_BYTES];
    nonce.copy_from_slice(&datagram[NONCE_AT..PREFIX_BYTES]);
    Some((datagram, nonce))
}

/// A client's hello.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Hello {
    /// The client's nonce, which every answer echoes.
    pub nonce: [u8; HELLO_NONCE_BYTES],
    /// The client's X25519 public key.
    pub public_key: [u8; X25519_BYTES],
    /// The token the last challenge carried, or
    /// [`NO_TOKEN`](super::NO_TOKEN). Opaque to the client.
    pub token: [u8; TOKEN_BYTES],
}

impl Hello {
    const KEY_AT: usize = PREFIX_BYTES;
    const TOKEN_AT: usize = Self::KEY_AT + X25519_BYTES;

    /// The datagram carrying this hello for `protocol_id`.
    #[must_use]
    pub fn encode(&self, protocol_id: u32) -> [u8; HELLO_BYTES] {
        let mut out = with_prefix(HELLO_TAG, protocol_id, &self.nonce);
        out[Self::KEY_AT..Self::TOKEN_AT].copy_from_slice(&self.public_key);
        out[Self::TOKEN_AT..].copy_from_slice(&self.token);
        out
    }

    /// The hello `datagram` carries for `protocol_id`, or `None` when it is
    /// anything else. Total on arbitrary bytes.
    #[must_use]
    pub fn decode(datagram: &[u8], protocol_id: u32) -> Option<Self> {
        let (datagram, nonce) = split_prefix::<HELLO_BYTES>(datagram, HELLO_TAG, protocol_id)?;
        let mut hello = Self {
            nonce,
            public_key: [0; X25519_BYTES],
            token: [0; TOKEN_BYTES],
        };
        hello
            .public_key
            .copy_from_slice(&datagram[Self::KEY_AT..Self::TOKEN_AT]);
        hello.token.copy_from_slice(&datagram[Self::TOKEN_AT..]);
        Some(hello)
    }
}

/// The server's challenge: a token for the client's next hello to present.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Challenge {
    /// The nonce of the hello this answers.
    pub nonce: [u8; HELLO_NONCE_BYTES],
    /// The token, opaque to the client.
    pub token: [u8; TOKEN_BYTES],
}

impl Challenge {
    /// The datagram carrying this challenge for `protocol_id`.
    #[must_use]
    pub fn encode(&self, protocol_id: u32) -> [u8; CHALLENGE_BYTES] {
        let mut out = with_prefix(CHALLENGE_TAG, protocol_id, &self.nonce);
        out[PREFIX_BYTES..].copy_from_slice(&self.token);
        out
    }

    /// The challenge `datagram` carries for `protocol_id`, or `None` when it
    /// is anything else. Total on arbitrary bytes.
    #[must_use]
    pub fn decode(datagram: &[u8], protocol_id: u32) -> Option<Self> {
        let (datagram, nonce) =
            split_prefix::<CHALLENGE_BYTES>(datagram, CHALLENGE_TAG, protocol_id)?;
        let mut token = [0; TOKEN_BYTES];
        token.copy_from_slice(&datagram[PREFIX_BYTES..]);
        Some(Self { nonce, token })
    }
}

/// The server's reply to a hello whose token verified.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Reply {
    /// The nonce of the hello this answers.
    pub nonce: [u8; HELLO_NONCE_BYTES],
    /// The server's X25519 public key.
    pub public_key: [u8; X25519_BYTES],
}

impl Reply {
    /// The datagram carrying this reply for `protocol_id`.
    #[must_use]
    pub fn encode(&self, protocol_id: u32) -> [u8; REPLY_BYTES] {
        let mut out = with_prefix(HELLO_REPLY_TAG, protocol_id, &self.nonce);
        out[PREFIX_BYTES..].copy_from_slice(&self.public_key);
        out
    }

    /// The reply `datagram` carries for `protocol_id`, or `None` when it is
    /// anything else. Total on arbitrary bytes.
    #[must_use]
    pub fn decode(datagram: &[u8], protocol_id: u32) -> Option<Self> {
        let (datagram, nonce) =
            split_prefix::<REPLY_BYTES>(datagram, HELLO_REPLY_TAG, protocol_id)?;
        let mut public_key = [0; X25519_BYTES];
        public_key.copy_from_slice(&datagram[PREFIX_BYTES..]);
        Some(Self { nonce, public_key })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const PROTOCOL: u32 = 0x4352_4342;

    fn prefix(tag: u8) -> Vec<u8> {
        let mut expected = vec![tag, TRANSPORT_VERSION];
        expected.extend_from_slice(&PROTOCOL.to_le_bytes());
        expected.extend_from_slice(&[0xA5; HELLO_NONCE_BYTES]);
        expected
    }

    fn hello() -> Hello {
        Hello {
            nonce: [0xA5; HELLO_NONCE_BYTES],
            public_key: [0x5A; X25519_BYTES],
            token: [0x3C; TOKEN_BYTES],
        }
    }

    fn challenge() -> Challenge {
        Challenge {
            nonce: [0xA5; HELLO_NONCE_BYTES],
            token: [0x3C; TOKEN_BYTES],
        }
    }

    fn reply() -> Reply {
        Reply {
            nonce: [0xA5; HELLO_NONCE_BYTES],
            public_key: [0x5A; X25519_BYTES],
        }
    }

    /// The layouts in the module docs, rebuilt by hand.
    #[test]
    fn every_kind_is_laid_out_as_documented() {
        let mut expected = prefix(HELLO_TAG);
        expected.extend_from_slice(&[0x5A; X25519_BYTES]);
        expected.extend_from_slice(&[0x3C; TOKEN_BYTES]);
        assert_eq!(hello().encode(PROTOCOL).as_slice(), expected.as_slice());
        assert_eq!(Hello::decode(&expected, PROTOCOL), Some(hello()));

        let mut expected = prefix(CHALLENGE_TAG);
        expected.extend_from_slice(&[0x3C; TOKEN_BYTES]);
        assert_eq!(challenge().encode(PROTOCOL).as_slice(), expected.as_slice());
        assert_eq!(Challenge::decode(&expected, PROTOCOL), Some(challenge()));

        let mut expected = prefix(HELLO_REPLY_TAG);
        expected.extend_from_slice(&[0x5A; X25519_BYTES]);
        assert_eq!(reply().encode(PROTOCOL).as_slice(), expected.as_slice());
        assert_eq!(Reply::decode(&expected, PROTOCOL), Some(reply()));
    }

    /// Another tag, version or protocol id, or any other length, is not the
    /// datagram asked for — the previous version's hello and reply among them.
    #[test]
    fn anything_but_the_exact_datagram_does_not_decode() {
        fn refused<T: PartialEq + std::fmt::Debug>(
            encoded: &[u8],
            decode: impl Fn(&[u8], u32) -> Option<T>,
        ) {
            assert_eq!(decode(encoded, PROTOCOL + 1), None);
            let mut other_version = encoded.to_vec();
            other_version[1] = TRANSPORT_VERSION - 1;
            assert_eq!(decode(&other_version, PROTOCOL), None);
            assert_eq!(decode(&encoded[..encoded.len() - 1], PROTOCOL), None);
            assert_eq!(decode(&[encoded, &[0]].concat(), PROTOCOL), None);
            assert_eq!(decode(&[], PROTOCOL), None);
            assert!(decode(encoded, PROTOCOL).is_some());
        }
        let hello = hello().encode(PROTOCOL);
        let challenge = challenge().encode(PROTOCOL);
        let reply = reply().encode(PROTOCOL);
        refused(&hello, Hello::decode);
        refused(&challenge, Challenge::decode);
        refused(&reply, Reply::decode);
        // Each kind's bytes are none of the others'.
        assert_eq!(Challenge::decode(&hello, PROTOCOL), None);
        assert_eq!(Reply::decode(&hello, PROTOCOL), None);
        assert_eq!(Hello::decode(&reply, PROTOCOL), None);
        assert_eq!(Challenge::decode(&reply, PROTOCOL), None);
        assert_eq!(Hello::decode(&challenge, PROTOCOL), None);
        assert_eq!(Reply::decode(&challenge, PROTOCOL), None);
    }
}
