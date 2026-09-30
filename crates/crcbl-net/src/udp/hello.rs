//! The plaintext hello and its reply: the only unsealed datagrams a
//! [`super::UdpTransport`] sends or reads.
//!
//! ```text
//! tag:          u8      HELLO_TAG, or HELLO_REPLY_TAG on the reply
//! version:      u8      TRANSPORT_VERSION
//! protocol_id:  u32 LE  the endpoint protocol id both sides must share
//! nonce:        [u8; HELLO_NONCE_BYTES]  the client's; the reply echoes it
//! public_key:   [u8; X25519_BYTES]       the sender's
//! ```
//!
//! Both directions have one layout and one fixed length, [`HELLO_BYTES`], so
//! a reply is never longer than the hello that asked for it. Anything of
//! another length, tag, version or protocol id is not a hello, and the
//! decoder says so without looking further.

use crate::seal::X25519_BYTES;

/// First byte of a client's hello. Distinct from [`crate::seal::SEALED_TAG`]
/// and from every [`crate::codec`] message tag, as the seal's own is.
pub const HELLO_TAG: u8 = 0x61;

/// First byte of the server's reply to a hello.
pub const HELLO_REPLY_TAG: u8 = 0x62;

/// The hello's own format version. A peer speaking another is not answered:
/// its hello does not decode, and its connect times out.
pub const TRANSPORT_VERSION: u8 = 1;

/// Bytes of the client's random nonce, which the reply echoes. Sixteen are
/// far past guessing for an attacker who did not see the hello.
pub const HELLO_NONCE_BYTES: usize = 16;

/// Bytes of a hello, and of its reply: tag, version, protocol id, nonce and
/// public key.
pub const HELLO_BYTES: usize = 1 + 1 + size_of::<u32>() + HELLO_NONCE_BYTES + X25519_BYTES;

const _: () = assert!(HELLO_TAG != crate::seal::SEALED_TAG);
const _: () = assert!(HELLO_REPLY_TAG != crate::seal::SEALED_TAG);
const _: () = assert!(HELLO_TAG != HELLO_REPLY_TAG);

/// Where the nonce starts: after the tag, version and protocol id.
const NONCE_AT: usize = 1 + 1 + size_of::<u32>();
/// Where the public key starts.
const KEY_AT: usize = NONCE_AT + HELLO_NONCE_BYTES;

/// A decoded hello or reply: the fields that differ between connections.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct Hello {
    /// The client's nonce: its own on a hello, echoed on a reply.
    pub(crate) nonce: [u8; HELLO_NONCE_BYTES],
    /// The sender's X25519 public key.
    pub(crate) public_key: [u8; X25519_BYTES],
}

impl Hello {
    /// The datagram carrying this hello under `tag`.
    pub(crate) fn encode(&self, tag: u8, protocol_id: u32) -> [u8; HELLO_BYTES] {
        let mut out = [0; HELLO_BYTES];
        out[0] = tag;
        out[1] = TRANSPORT_VERSION;
        out[2..NONCE_AT].copy_from_slice(&protocol_id.to_le_bytes());
        out[NONCE_AT..KEY_AT].copy_from_slice(&self.nonce);
        out[KEY_AT..].copy_from_slice(&self.public_key);
        out
    }

    /// The hello `datagram` carries under `tag` for `protocol_id`, or `None`
    /// when it is anything else. Total on arbitrary bytes.
    pub(crate) fn decode(datagram: &[u8], tag: u8, protocol_id: u32) -> Option<Self> {
        let datagram: &[u8; HELLO_BYTES] = datagram.try_into().ok()?;
        if datagram[0] != tag
            || datagram[1] != TRANSPORT_VERSION
            || datagram[2..NONCE_AT] != protocol_id.to_le_bytes()
        {
            return None;
        }
        let mut hello = Self {
            nonce: [0; HELLO_NONCE_BYTES],
            public_key: [0; X25519_BYTES],
        };
        hello.nonce.copy_from_slice(&datagram[NONCE_AT..KEY_AT]);
        hello.public_key.copy_from_slice(&datagram[KEY_AT..]);
        Some(hello)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const PROTOCOL: u32 = 0x4352_4342;

    fn sample() -> Hello {
        Hello {
            nonce: [0xA5; HELLO_NONCE_BYTES],
            public_key: [0x5A; X25519_BYTES],
        }
    }

    /// The layout in the module docs, rebuilt by hand.
    #[test]
    fn the_hello_is_laid_out_as_documented() {
        let encoded = sample().encode(HELLO_TAG, PROTOCOL);
        let mut expected = vec![HELLO_TAG, TRANSPORT_VERSION];
        expected.extend_from_slice(&PROTOCOL.to_le_bytes());
        expected.extend_from_slice(&[0xA5; HELLO_NONCE_BYTES]);
        expected.extend_from_slice(&[0x5A; X25519_BYTES]);
        assert_eq!(encoded.as_slice(), expected.as_slice());
        assert_eq!(Hello::decode(&encoded, HELLO_TAG, PROTOCOL), Some(sample()));
    }

    /// Another tag, version or protocol id, or any other length, is not a
    /// hello.
    #[test]
    fn anything_but_an_exact_hello_does_not_decode() {
        let encoded = sample().encode(HELLO_TAG, PROTOCOL);
        assert_eq!(Hello::decode(&encoded, HELLO_REPLY_TAG, PROTOCOL), None);
        assert_eq!(Hello::decode(&encoded, HELLO_TAG, PROTOCOL + 1), None);
        let mut other_version = encoded;
        other_version[1] = TRANSPORT_VERSION + 1;
        assert_eq!(Hello::decode(&other_version, HELLO_TAG, PROTOCOL), None);
        assert_eq!(
            Hello::decode(&encoded[..HELLO_BYTES - 1], HELLO_TAG, PROTOCOL),
            None
        );
        let longer = [encoded.as_slice(), &[0]].concat();
        assert_eq!(Hello::decode(&longer, HELLO_TAG, PROTOCOL), None);
        assert_eq!(Hello::decode(&[], HELLO_TAG, PROTOCOL), None);
    }
}
