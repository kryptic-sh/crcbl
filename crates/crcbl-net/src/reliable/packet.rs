//! The packet wire format, and a decoder that is total on arbitrary bytes.
//!
//! ```text
//! protocol_id:  u32 LE
//! kind:         u8     (low bits: the body kind; ACKS_PRESENT flag above them)
//! sequence:     u16 LE
//! ack:          u16 LE (zero unless ACKS_PRESENT)
//! ack_bits:     u64 LE (zero unless ACKS_PRESENT)
//! body:         per kind
//!   Ack, Keepalive, Disconnect: nothing
//!   Reliable:     message_id u16 LE, fragment_index u8, fragment_count u8,
//!                 payload (the rest)
//!   Unreliable:   payload (the rest)
//! ```
//!
//! Every packet carries the ack field, so acknowledgement costs no traffic of
//! its own. The unreliable body has no counter because it needs none: the
//! packet sequence already orders it, and latest-wins compares that.
//!
//! **The protocol id is a filter, not a defence.** It drops a stray datagram
//! from another program or another build before any state is touched; it
//! proves nothing about who sent it. That is the AEAD layer's job — see the
//! [`super`] module docs.

use crate::codec::{ByteReader, DecodeError};

/// The datagram budget the whole stack is sized against: small enough to pass
/// unfragmented over any path a LAN session will meet, which is why the
/// backlog's design names it as the conservative MTU. Path-MTU discovery would
/// raise it; nothing does yet.
pub const MAX_DATAGRAM_BYTES: usize = 1200;

/// Bytes kept free in every datagram for what [`crate::seal`] adds around a
/// packet: its clear prefix and the Poly1305 tag. The nonce is not sent, since
/// it derives from direction and the prefix's counter. The seal owns the
/// number; this is where the packet budget reads it.
pub const SEAL_RESERVE: usize = crate::seal::SEAL_OVERHEAD;

/// The largest packet this layer emits or accepts: the datagram budget less the
/// seal's reserve.
pub const MAX_PACKET_BYTES: usize = MAX_DATAGRAM_BYTES - SEAL_RESERVE;

/// Bytes of the header every packet starts with: protocol id, kind, sequence,
/// ack and ack bitfield.
pub const HEADER_BYTES: usize = 4 + 1 + 2 + 2 + 8;

/// Bytes a reliable body spends before its payload: message id, fragment index
/// and fragment count.
const RELIABLE_PREFIX_BYTES: usize = 2 + 1 + 1;

/// The most payload one reliable packet carries, and so the size of every
/// fragment but the last.
pub const MAX_FRAGMENT_BYTES: usize = MAX_PACKET_BYTES - HEADER_BYTES - RELIABLE_PREFIX_BYTES;

/// The most payload one unreliable-sequenced packet carries. That channel never
/// fragments, so this is also the largest message it accepts: the one-datagram
/// rule.
pub const MAX_UNRELIABLE_PAYLOAD: usize = MAX_PACKET_BYTES - HEADER_BYTES;

/// Fragments one reliable message may be split into.
///
/// Sized so a message clears the in-memory transport's
/// [`crate::MAX_IN_MEMORY_MESSAGE_BYTES`] with room over — a join-in-progress
/// snapshot that fits the seam must fit here too — while one message's
/// reassembly stays a bounded, modest allocation. It fits the wire's `u8`
/// fragment count.
pub const MAX_FRAGMENTS_PER_MESSAGE: usize = 128;

/// The largest reliable message: every fragment full.
pub const MAX_RELIABLE_MESSAGE_BYTES: usize = MAX_FRAGMENTS_PER_MESSAGE * MAX_FRAGMENT_BYTES;

const _: () = assert!(MAX_FRAGMENTS_PER_MESSAGE <= u8::MAX as usize);
const _: () = assert!(MAX_RELIABLE_MESSAGE_BYTES > crate::MAX_IN_MEMORY_MESSAGE_BYTES);

/// The flag in the kind byte saying the ack field is live.
const ACKS_PRESENT: u8 = 0x80;

const KIND_ACK: u8 = 0;
const KIND_KEEPALIVE: u8 = 1;
const KIND_RELIABLE: u8 = 2;
const KIND_UNRELIABLE: u8 = 3;
const KIND_DISCONNECT: u8 = 4;

/// Why a datagram is not a packet.
#[derive(Debug, thiserror::Error)]
pub enum PacketDecodeError {
    /// Shorter than its own framing, or longer than a bodiless kind allows.
    #[error("packet framing: {0}")]
    Framing(#[from] DecodeError),
    /// Longer than [`MAX_PACKET_BYTES`]; no honest peer sends it.
    #[error("packet of {size} bytes exceeds the {limit}-byte limit")]
    Oversized { size: usize, limit: usize },
    /// Another program's datagram, or another build's.
    #[error("packet protocol id {found:#010x} is not ours")]
    WrongProtocol { found: u32 },
    /// A kind byte this build does not know.
    #[error("unknown packet kind {0:#04x}")]
    UnknownKind(u8),
    /// The ack field is marked absent and is not zero.
    #[error("ack field marked absent carries a value")]
    AbsentAcksNotZero,
    /// A fragment count of zero, or above [`MAX_FRAGMENTS_PER_MESSAGE`].
    #[error("fragment count {0} is outside 1..={MAX_FRAGMENTS_PER_MESSAGE}")]
    FragmentCount(u8),
    /// A fragment index at or past its own count.
    #[error("fragment index {index} is not below its count {count}")]
    FragmentIndex { index: u8, count: u8 },
    /// A fragment of a split message whose size the split could not produce:
    /// every fragment but the last is exactly [`MAX_FRAGMENT_BYTES`], and the
    /// last is not empty.
    #[error("fragment {index} of {count} has impossible size {size}")]
    FragmentSize { index: u8, count: u8, size: usize },
}

/// A packet's acknowledgement of what it has received from the peer.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct PacketAcks {
    /// The newest sequence received.
    pub latest: u16,
    /// Bit `i` set acknowledges `latest - 1 - i`.
    pub bits: u64,
}

/// The fields every packet carries ahead of its body.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct PacketHeader {
    /// This packet's own sequence, one past the previous packet's, wrapping.
    pub sequence: u16,
    /// What the sender has received, or `None` before it has received
    /// anything.
    pub acks: Option<PacketAcks>,
}

/// One piece of a reliable message; an unsplit message is its own single
/// fragment.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Fragment<'a> {
    /// The message's position in the reliable channel's order.
    pub message_id: u16,
    /// Which piece this is, below [`Self::count`].
    pub index: u8,
    /// How many pieces the message was split into.
    pub count: u8,
    /// This piece's bytes.
    pub payload: &'a [u8],
}

/// What a packet carries after its header.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PacketBody<'a> {
    /// Nothing but the header's acks. Never acknowledged in return, so two
    /// peers acking each other cannot ping-pong.
    Ack,
    /// A liveness probe for an idle link. The peer acks it promptly, which is
    /// what keeps the round-trip estimate fed when no reliable traffic flows.
    Keepalive,
    /// A fragment on the reliable-ordered channel.
    Reliable(Fragment<'a>),
    /// A whole payload on the unreliable-sequenced channel.
    Unreliable(&'a [u8]),
    /// The sender is going away on purpose — distinct from falling silent.
    Disconnect,
}

/// Write one packet.
///
/// The endpoint only ever builds bodies that fit; a caller handing this a
/// payload past [`MAX_UNRELIABLE_PAYLOAD`] or a fragment past
/// [`MAX_FRAGMENT_BYTES`] gets a packet [`decode_packet`] refuses.
#[must_use]
pub fn encode_packet(protocol_id: u32, header: &PacketHeader, body: &PacketBody<'_>) -> Vec<u8> {
    let (kind, payload_len) = match body {
        PacketBody::Ack => (KIND_ACK, 0),
        PacketBody::Keepalive => (KIND_KEEPALIVE, 0),
        PacketBody::Reliable(fragment) => (
            KIND_RELIABLE,
            RELIABLE_PREFIX_BYTES + fragment.payload.len(),
        ),
        PacketBody::Unreliable(payload) => (KIND_UNRELIABLE, payload.len()),
        PacketBody::Disconnect => (KIND_DISCONNECT, 0),
    };
    let mut out = Vec::with_capacity(HEADER_BYTES + payload_len);
    out.extend_from_slice(&protocol_id.to_le_bytes());
    let acks = header.acks.unwrap_or(PacketAcks { latest: 0, bits: 0 });
    let flag = if header.acks.is_some() {
        ACKS_PRESENT
    } else {
        0
    };
    out.push(kind | flag);
    out.extend_from_slice(&header.sequence.to_le_bytes());
    out.extend_from_slice(&acks.latest.to_le_bytes());
    out.extend_from_slice(&acks.bits.to_le_bytes());
    match body {
        PacketBody::Reliable(fragment) => {
            out.extend_from_slice(&fragment.message_id.to_le_bytes());
            out.push(fragment.index);
            out.push(fragment.count);
            out.extend_from_slice(fragment.payload);
        }
        PacketBody::Unreliable(payload) => out.extend_from_slice(payload),
        PacketBody::Ack | PacketBody::Keepalive | PacketBody::Disconnect => {}
    }
    out
}

/// Read one packet, refusing anything not addressed to `protocol_id`.
///
/// Total on arbitrary bytes: every malformed input is a
/// [`PacketDecodeError`], never a panic, and the size is checked before
/// anything else is read.
pub fn decode_packet(
    datagram: &[u8],
    protocol_id: u32,
) -> Result<(PacketHeader, PacketBody<'_>), PacketDecodeError> {
    if datagram.len() > MAX_PACKET_BYTES {
        return Err(PacketDecodeError::Oversized {
            size: datagram.len(),
            limit: MAX_PACKET_BYTES,
        });
    }
    let mut r = ByteReader::new(datagram);
    let found = r.read_u32()?;
    if found != protocol_id {
        return Err(PacketDecodeError::WrongProtocol { found });
    }
    let kind_byte = r.read_u8()?;
    let sequence = r.read_u16()?;
    let latest = r.read_u16()?;
    let bits = r.read_u64()?;
    let acks = if kind_byte & ACKS_PRESENT != 0 {
        Some(PacketAcks { latest, bits })
    } else if latest != 0 || bits != 0 {
        return Err(PacketDecodeError::AbsentAcksNotZero);
    } else {
        None
    };
    let body = match kind_byte & !ACKS_PRESENT {
        KIND_ACK => PacketBody::Ack,
        KIND_KEEPALIVE => PacketBody::Keepalive,
        KIND_DISCONNECT => PacketBody::Disconnect,
        KIND_UNRELIABLE => PacketBody::Unreliable(r.read_bytes(r.remaining())?),
        KIND_RELIABLE => PacketBody::Reliable(decode_fragment(&mut r)?),
        _ => return Err(PacketDecodeError::UnknownKind(kind_byte)),
    };
    r.assert_empty()?;
    Ok((PacketHeader { sequence, acks }, body))
}

fn decode_fragment<'a>(r: &mut ByteReader<'a>) -> Result<Fragment<'a>, PacketDecodeError> {
    let message_id = r.read_u16()?;
    let index = r.read_u8()?;
    let count = r.read_u8()?;
    if count == 0 || usize::from(count) > MAX_FRAGMENTS_PER_MESSAGE {
        return Err(PacketDecodeError::FragmentCount(count));
    }
    if index >= count {
        return Err(PacketDecodeError::FragmentIndex { index, count });
    }
    let payload = r.read_bytes(r.remaining())?;
    // An unsplit message may be anything up to a full fragment, empty
    // included. A split one was cut by `fragment::piece`, so its sizes are
    // fixed, and holding a hostile peer to them is what caps one message's
    // reassembly at `count` full fragments.
    let impossible = count > 1
        && if index + 1 < count {
            payload.len() != MAX_FRAGMENT_BYTES
        } else {
            payload.is_empty()
        };
    if impossible {
        return Err(PacketDecodeError::FragmentSize {
            index,
            count,
            size: payload.len(),
        });
    }
    Ok(Fragment {
        message_id,
        index,
        count,
        payload,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    const PROTOCOL: u32 = 0xC0DE_0001;

    fn header(acks: Option<PacketAcks>) -> PacketHeader {
        PacketHeader {
            sequence: 65_535,
            acks,
        }
    }

    fn roundtrip(header: PacketHeader, body: PacketBody<'_>) {
        let bytes = encode_packet(PROTOCOL, &header, &body);
        let (decoded_header, decoded_body) = decode_packet(&bytes, PROTOCOL).unwrap();
        assert_eq!(decoded_header, header);
        assert_eq!(decoded_body, body);
    }

    #[test]
    fn every_packet_kind_survives_an_encode_decode_round_trip() {
        let acks = Some(PacketAcks {
            latest: 3,
            bits: 0xDEAD_BEEF_0000_0001,
        });
        let full = vec![0xAB; MAX_FRAGMENT_BYTES];
        for header in [header(None), header(acks)] {
            roundtrip(header, PacketBody::Ack);
            roundtrip(header, PacketBody::Keepalive);
            roundtrip(header, PacketBody::Disconnect);
            roundtrip(header, PacketBody::Unreliable(b"snapshot"));
            roundtrip(header, PacketBody::Unreliable(&[0; MAX_UNRELIABLE_PAYLOAD]));
            roundtrip(
                header,
                PacketBody::Reliable(Fragment {
                    message_id: 65_535,
                    index: 0,
                    count: 1,
                    payload: b"",
                }),
            );
            roundtrip(
                header,
                PacketBody::Reliable(Fragment {
                    message_id: 7,
                    index: 0,
                    count: 3,
                    payload: &full,
                }),
            );
        }
    }

    #[test]
    fn a_full_packet_is_exactly_the_packet_budget() {
        let bytes = encode_packet(
            PROTOCOL,
            &header(None),
            &PacketBody::Unreliable(&[0; MAX_UNRELIABLE_PAYLOAD]),
        );
        assert_eq!(bytes.len(), MAX_PACKET_BYTES);
        let bytes = encode_packet(
            PROTOCOL,
            &header(None),
            &PacketBody::Reliable(Fragment {
                message_id: 0,
                index: 0,
                count: 2,
                payload: &[0; MAX_FRAGMENT_BYTES],
            }),
        );
        assert_eq!(bytes.len(), MAX_PACKET_BYTES);
    }

    /// Every prefix of every valid packet shorter than its framing is refused
    /// as framing, never read past.
    #[test]
    fn every_truncation_of_a_packet_is_refused() {
        let reliable = encode_packet(
            PROTOCOL,
            &header(None),
            &PacketBody::Reliable(Fragment {
                message_id: 1,
                index: 0,
                count: 1,
                payload: b"x",
            }),
        );
        for len in 0..HEADER_BYTES + RELIABLE_PREFIX_BYTES {
            assert!(
                matches!(
                    decode_packet(&reliable[..len], PROTOCOL),
                    Err(PacketDecodeError::Framing(DecodeError::TooShort { .. }))
                ),
                "a {len}-byte prefix must be refused as too short",
            );
        }
    }

    #[test]
    fn malformed_packets_are_refused_with_the_reason() {
        let keepalive = encode_packet(PROTOCOL, &header(None), &PacketBody::Keepalive);

        assert!(matches!(
            decode_packet(&keepalive, PROTOCOL + 1),
            Err(PacketDecodeError::WrongProtocol { found: PROTOCOL })
        ));

        let mut trailing = keepalive.clone();
        trailing.push(0);
        assert!(matches!(
            decode_packet(&trailing, PROTOCOL),
            Err(PacketDecodeError::Framing(DecodeError::TrailingBytes(1)))
        ));

        let mut unknown = keepalive.clone();
        unknown[4] = 0x7F;
        assert!(matches!(
            decode_packet(&unknown, PROTOCOL),
            Err(PacketDecodeError::UnknownKind(0x7F))
        ));

        let mut stray_acks = keepalive.clone();
        stray_acks[7] = 1;
        assert!(matches!(
            decode_packet(&stray_acks, PROTOCOL),
            Err(PacketDecodeError::AbsentAcksNotZero)
        ));

        assert!(matches!(
            decode_packet(&vec![0; MAX_PACKET_BYTES + 1], PROTOCOL),
            Err(PacketDecodeError::Oversized { .. })
        ));

        let fragment = |index: u8, count: u8, payload: &[u8]| {
            let mut bytes = encode_packet(
                PROTOCOL,
                &header(None),
                &PacketBody::Reliable(Fragment {
                    message_id: 0,
                    index: 0,
                    count: 1,
                    payload,
                }),
            );
            bytes[HEADER_BYTES + 2] = index;
            bytes[HEADER_BYTES + 3] = count;
            decode_packet(&bytes, PROTOCOL).map(|_| ())
        };
        assert!(matches!(
            fragment(0, 0, b"x"),
            Err(PacketDecodeError::FragmentCount(0))
        ));
        let too_many = u8::try_from(MAX_FRAGMENTS_PER_MESSAGE + 1).unwrap();
        assert!(matches!(
            fragment(0, too_many, b"x"),
            Err(PacketDecodeError::FragmentCount(_))
        ));
        assert!(matches!(
            fragment(2, 2, b"x"),
            Err(PacketDecodeError::FragmentIndex { index: 2, count: 2 })
        ));
        // A short fragment that is not the last, and an empty last one.
        assert!(matches!(
            fragment(0, 2, b"x"),
            Err(PacketDecodeError::FragmentSize { index: 0, .. })
        ));
        assert!(matches!(
            fragment(1, 2, b""),
            Err(PacketDecodeError::FragmentSize { index: 1, .. })
        ));
        assert!(fragment(1, 2, b"x").is_ok());
    }

    /// Seeded arbitrary bytes, and seeded corruptions of valid packets — the
    /// latter carry the right protocol id, so they reach the body parsers
    /// rather than stopping at the first check. None may panic.
    #[test]
    fn arbitrary_and_corrupted_bytes_never_panic_the_decoder() {
        let mut seed = 0x5EED_u64;
        let mut next = || {
            seed = seed.wrapping_mul(6364136223846793005).wrapping_add(1);
            seed >> 32
        };
        let valid = encode_packet(
            PROTOCOL,
            &header(None),
            &PacketBody::Reliable(Fragment {
                message_id: 9,
                index: 1,
                count: 2,
                payload: b"tail",
            }),
        );
        for _ in 0..20_000 {
            let len = (next() % (MAX_PACKET_BYTES as u64 + 8)) as usize;
            let bytes: Vec<u8> = (0..len).map(|_| next() as u8).collect();
            let _ = decode_packet(&bytes, PROTOCOL);

            let mut corrupt = valid.clone();
            for _ in 0..=next() % 4 {
                let at = (next() as usize) % corrupt.len();
                corrupt[at] = next() as u8;
            }
            corrupt.truncate(1 + (next() as usize) % corrupt.len());
            let _ = decode_packet(&corrupt, PROTOCOL);
        }
    }
}
