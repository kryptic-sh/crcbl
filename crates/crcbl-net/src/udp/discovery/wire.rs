//! The two discovery datagrams: a host's announce and a client's query.
//!
//! ```text
//! announce                    ANNOUNCE_BYTES, always
//!   tag:              u8      ANNOUNCE_TAG
//!   version:          u8      DISCOVERY_VERSION
//!   protocol_id:      u32 LE  the endpoint protocol id the host's listener speaks
//!   game_port:        u16 LE  the listener's port; never zero
//!   players:          u16 LE
//!   max_players:      u16 LE
//!   protocol_version: u32 LE  ProtocolCompatibility, as the session handshake
//!   engine_build_id:  u64 LE  gates on it
//!   schema_hash:      u64 LE
//!   name_len:         u8      at most MAX_NAME_BYTES
//!   name:             [u8; MAX_NAME_BYTES]  UTF-8, name_len bytes, then zeros
//!
//! query                       QUERY_BYTES, always
//!   tag:              u8      QUERY_TAG
//!   version:          u8      DISCOVERY_VERSION
//!   protocol_id:      u32 LE  the protocol id the client wants hosts for
//!   padding:          zeros up to QUERY_BYTES
//! ```
//!
//! Both have one fixed length, and the query is padded to at least the
//! announce it asks for — see [`super`]'s docs for why. **No address travels
//! in either**: the address a client connects to is the announce's source IP,
//! which is what keeps a forged announce from pointing clients anywhere but
//! at the forger itself.

use std::num::NonZeroU16;

use crate::ProtocolCompatibility;

/// First byte of a host's announce. Distinct from the hello's tags, from
/// [`crate::seal::SEALED_TAG`] and from every [`crate::codec`] message tag, so
/// a stray announce reaching a game socket is dropped as unknown there.
pub const ANNOUNCE_TAG: u8 = 0x63;

/// First byte of a client's query.
pub const QUERY_TAG: u8 = 0x64;

/// The discovery format's own version. An announce or query in another is
/// dropped and counted, not parsed.
pub const DISCOVERY_VERSION: u8 = 1;

/// The most bytes of UTF-8 a host's name takes on the wire. Room for a short
/// label in a browser's row, and it keeps the announce fixed-size.
pub const MAX_NAME_BYTES: usize = 32;

/// Tag, version and protocol id: the part both datagrams share.
const HEADER_BYTES: usize = 1 + 1 + size_of::<u32>();

/// Bytes of every announce.
pub const ANNOUNCE_BYTES: usize = HEADER_BYTES
    + 3 * size_of::<u16>()
    + size_of::<u32>()
    + 2 * size_of::<u64>()
    + 1
    + MAX_NAME_BYTES;

/// Bytes of every query: as long as the announce it asks for, so a reply is
/// never larger than the query that provoked it.
pub const QUERY_BYTES: usize = ANNOUNCE_BYTES;

const _: () = assert!(QUERY_BYTES >= ANNOUNCE_BYTES);
const _: () = assert!(MAX_NAME_BYTES <= u8::MAX as usize);
const _: () = assert!(ANNOUNCE_TAG != QUERY_TAG);
const _: () = assert!(ANNOUNCE_TAG != crate::seal::SEALED_TAG);
const _: () = assert!(QUERY_TAG != crate::seal::SEALED_TAG);
const _: () = assert!(ANNOUNCE_TAG != super::super::HELLO_TAG);
const _: () = assert!(ANNOUNCE_TAG != super::super::HELLO_REPLY_TAG);
const _: () = assert!(QUERY_TAG != super::super::HELLO_TAG);
const _: () = assert!(QUERY_TAG != super::super::HELLO_REPLY_TAG);

// Where each announce field starts.
const GAME_PORT_AT: usize = HEADER_BYTES;
const PLAYERS_AT: usize = GAME_PORT_AT + size_of::<u16>();
const MAX_PLAYERS_AT: usize = PLAYERS_AT + size_of::<u16>();
const PROTOCOL_VERSION_AT: usize = MAX_PLAYERS_AT + size_of::<u16>();
const ENGINE_BUILD_AT: usize = PROTOCOL_VERSION_AT + size_of::<u32>();
const SCHEMA_HASH_AT: usize = ENGINE_BUILD_AT + size_of::<u64>();
const NAME_LEN_AT: usize = SCHEMA_HASH_AT + size_of::<u64>();
const NAME_AT: usize = NAME_LEN_AT + 1;

const _: () = assert!(NAME_AT + MAX_NAME_BYTES == ANNOUNCE_BYTES);

/// What a host tells the network about itself.
///
/// The name is held capped: [`new`](Self::new) and
/// [`set_name`](Self::set_name) drop control characters and cut it at the
/// last whole character within [`MAX_NAME_BYTES`], so every announcement
/// encodes, and decodes back to itself.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Announcement {
    /// The endpoint protocol id the host's [`crate::udp::UdpListener`]
    /// speaks. A browser lists only hosts on its own.
    pub protocol_id: u32,
    /// The port the host's listener is bound to: where a client connects.
    pub game_port: NonZeroU16,
    /// Players in the session now.
    pub players: u16,
    /// Players the session takes.
    pub max_players: u16,
    /// What the session handshake will gate on, so a browser can show a host
    /// it cannot join as such before anyone tries.
    pub compatibility: ProtocolCompatibility,
    name: String,
}

impl Announcement {
    /// An announcement of a session with no players and room for none,
    /// named `name` after capping it as the type's docs say. Set
    /// [`players`](Self::players) and [`max_players`](Self::max_players) to
    /// taste.
    #[must_use]
    pub fn new(
        protocol_id: u32,
        game_port: NonZeroU16,
        compatibility: ProtocolCompatibility,
        name: &str,
    ) -> Self {
        Self {
            protocol_id,
            game_port,
            players: 0,
            max_players: 0,
            compatibility,
            name: cap_name(name),
        }
    }

    /// The host's name, as it goes on the wire.
    #[must_use]
    pub fn name(&self) -> &str {
        &self.name
    }

    /// Renames the host, capping `name` as the type's docs say.
    pub fn set_name(&mut self, name: &str) {
        self.name = cap_name(name);
    }

    /// The announce datagram.
    pub(crate) fn encode(&self) -> [u8; ANNOUNCE_BYTES] {
        let mut out = [0; ANNOUNCE_BYTES];
        encode_header(&mut out, ANNOUNCE_TAG, self.protocol_id);
        out[GAME_PORT_AT..PLAYERS_AT].copy_from_slice(&self.game_port.get().to_le_bytes());
        out[PLAYERS_AT..MAX_PLAYERS_AT].copy_from_slice(&self.players.to_le_bytes());
        out[MAX_PLAYERS_AT..PROTOCOL_VERSION_AT].copy_from_slice(&self.max_players.to_le_bytes());
        let compatibility = &self.compatibility;
        out[PROTOCOL_VERSION_AT..ENGINE_BUILD_AT]
            .copy_from_slice(&compatibility.protocol_version.to_le_bytes());
        out[ENGINE_BUILD_AT..SCHEMA_HASH_AT]
            .copy_from_slice(&compatibility.engine_build_id.to_le_bytes());
        out[SCHEMA_HASH_AT..NAME_LEN_AT].copy_from_slice(&compatibility.schema_hash.to_le_bytes());
        let name = self.name.as_bytes();
        // `cap_name` holds the name within `MAX_NAME_BYTES`, which the
        // assertion above holds within a `u8`.
        out[NAME_LEN_AT] = name.len() as u8;
        out[NAME_AT..NAME_AT + name.len()].copy_from_slice(name);
        out
    }

    /// The announcement `datagram` carries. Total on arbitrary bytes.
    ///
    /// # Errors
    ///
    /// [`Refusal::OtherVersion`] for an announce in another
    /// [`DISCOVERY_VERSION`], and [`Refusal::Malformed`] for anything else
    /// that is not exactly an announce: another tag or length, a zero game
    /// port, a name length past [`MAX_NAME_BYTES`], bytes after the name that
    /// are not zero, or a name that is not UTF-8 or holds a control
    /// character.
    pub(crate) fn decode(datagram: &[u8]) -> Result<Self, Refusal> {
        let protocol_id = decode_header(datagram, ANNOUNCE_TAG, ANNOUNCE_BYTES)?;
        let game_port = NonZeroU16::new(u16::from_le_bytes(field(datagram, GAME_PORT_AT)))
            .ok_or(Refusal::Malformed)?;
        let name_len = usize::from(datagram[NAME_LEN_AT]);
        if name_len > MAX_NAME_BYTES {
            return Err(Refusal::Malformed);
        }
        let (name, rest) = datagram[NAME_AT..].split_at(name_len);
        if rest.iter().any(|&byte| byte != 0) {
            return Err(Refusal::Malformed);
        }
        let name = std::str::from_utf8(name).map_err(|_| Refusal::Malformed)?;
        if name.chars().any(char::is_control) {
            return Err(Refusal::Malformed);
        }
        Ok(Self {
            protocol_id,
            game_port,
            players: u16::from_le_bytes(field(datagram, PLAYERS_AT)),
            max_players: u16::from_le_bytes(field(datagram, MAX_PLAYERS_AT)),
            compatibility: ProtocolCompatibility {
                protocol_version: u32::from_le_bytes(field(datagram, PROTOCOL_VERSION_AT)),
                engine_build_id: u64::from_le_bytes(field(datagram, ENGINE_BUILD_AT)),
                schema_hash: u64::from_le_bytes(field(datagram, SCHEMA_HASH_AT)),
            },
            name: name.to_owned(),
        })
    }
}

/// Why a datagram on a discovery socket was dropped unread.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Refusal {
    /// Its tag and length are right and its version is not ours: a newer or
    /// older build's discovery, not garbage.
    OtherVersion,
    /// Not the datagram it was read as.
    Malformed,
}

/// The query datagram asking hosts on `protocol_id` to announce.
pub(crate) fn encode_query(protocol_id: u32) -> [u8; QUERY_BYTES] {
    let mut out = [0; QUERY_BYTES];
    encode_header(&mut out, QUERY_TAG, protocol_id);
    out
}

/// The protocol id the query `datagram` asks for. Total on arbitrary bytes.
///
/// # Errors
///
/// [`Refusal::OtherVersion`] for a query in another [`DISCOVERY_VERSION`],
/// and [`Refusal::Malformed`] for another tag or length — a short, unpadded
/// query among them — or padding that is not zero.
pub(crate) fn decode_query(datagram: &[u8]) -> Result<u32, Refusal> {
    let protocol_id = decode_header(datagram, QUERY_TAG, QUERY_BYTES)?;
    if datagram[HEADER_BYTES..].iter().any(|&byte| byte != 0) {
        return Err(Refusal::Malformed);
    }
    Ok(protocol_id)
}

/// `name` without control characters, cut at the last whole character that
/// fits [`MAX_NAME_BYTES`].
fn cap_name(name: &str) -> String {
    let mut capped = String::new();
    for c in name.chars().filter(|c| !c.is_control()) {
        if capped.len() + c.len_utf8() > MAX_NAME_BYTES {
            break;
        }
        capped.push(c);
    }
    capped
}

fn encode_header(out: &mut [u8], tag: u8, protocol_id: u32) {
    out[0] = tag;
    out[1] = DISCOVERY_VERSION;
    out[2..HEADER_BYTES].copy_from_slice(&protocol_id.to_le_bytes());
}

/// The protocol id of a datagram exactly `len` bytes long under `tag`.
fn decode_header(datagram: &[u8], tag: u8, len: usize) -> Result<u32, Refusal> {
    if datagram.len() != len || datagram[0] != tag {
        return Err(Refusal::Malformed);
    }
    if datagram[1] != DISCOVERY_VERSION {
        return Err(Refusal::OtherVersion);
    }
    Ok(u32::from_le_bytes(field(datagram, 2)))
}

/// The `N` bytes of `datagram` from `at`; the caller has checked the length.
fn field<const N: usize>(datagram: &[u8], at: usize) -> [u8; N] {
    let mut bytes = [0; N];
    bytes.copy_from_slice(&datagram[at..at + N]);
    bytes
}

#[cfg(test)]
mod tests {
    use super::*;

    const PROTOCOL: u32 = 0x4352_4342;

    fn port(port: u16) -> NonZeroU16 {
        NonZeroU16::new(port).expect("a test port is not zero")
    }

    fn sample() -> Announcement {
        let mut announcement = Announcement::new(
            PROTOCOL,
            port(0xBEEF),
            ProtocolCompatibility {
                protocol_version: 0x0102_0304,
                engine_build_id: 0x1122_3344_5566_7788,
                schema_hash: 0x99AA_BBCC_DDEE_FF00,
            },
            "Den",
        );
        announcement.players = 3;
        announcement.max_players = 8;
        announcement
    }

    /// The layout in the module docs, rebuilt by hand.
    #[test]
    fn the_announce_is_laid_out_as_documented() {
        let encoded = sample().encode();
        let mut expected = vec![ANNOUNCE_TAG, DISCOVERY_VERSION];
        expected.extend_from_slice(&PROTOCOL.to_le_bytes());
        expected.extend_from_slice(&0xBEEF_u16.to_le_bytes());
        expected.extend_from_slice(&3_u16.to_le_bytes());
        expected.extend_from_slice(&8_u16.to_le_bytes());
        expected.extend_from_slice(&0x0102_0304_u32.to_le_bytes());
        expected.extend_from_slice(&0x1122_3344_5566_7788_u64.to_le_bytes());
        expected.extend_from_slice(&0x99AA_BBCC_DDEE_FF00_u64.to_le_bytes());
        expected.push(3);
        expected.extend_from_slice(b"Den");
        expected.resize(ANNOUNCE_BYTES, 0);
        assert_eq!(encoded.as_slice(), expected.as_slice());
    }

    #[test]
    fn an_announce_decodes_to_what_was_encoded() {
        for name in ["", "Den", "Łódź — ü", &"x".repeat(MAX_NAME_BYTES)] {
            let mut announcement = sample();
            announcement.set_name(name);
            assert_eq!(announcement.name(), name);
            assert_eq!(
                Announcement::decode(&announcement.encode()),
                Ok(announcement)
            );
        }
    }

    #[test]
    fn the_query_is_laid_out_as_documented_and_decodes() {
        let encoded = encode_query(PROTOCOL);
        let mut expected = vec![QUERY_TAG, DISCOVERY_VERSION];
        expected.extend_from_slice(&PROTOCOL.to_le_bytes());
        expected.resize(QUERY_BYTES, 0);
        assert_eq!(encoded.as_slice(), expected.as_slice());
        assert_eq!(decode_query(&encoded), Ok(PROTOCOL));
    }

    /// A name past the cap is cut at the last whole character that fits —
    /// here a two-byte one straddling the limit is left out whole — and
    /// control characters never reach the wire.
    #[test]
    fn a_long_name_is_cut_at_a_character_within_the_cap() {
        let fits = "a".repeat(MAX_NAME_BYTES - 1);
        let announcement = Announcement::new(
            PROTOCOL,
            port(1),
            ProtocolCompatibility::DEFAULT,
            &format!("{fits}é and more"),
        );
        assert_eq!(announcement.name(), fits);

        let exact = "b".repeat(MAX_NAME_BYTES);
        let mut announcement = sample();
        announcement.set_name(&format!("{exact}c"));
        assert_eq!(announcement.name(), exact);
        assert_eq!(
            Announcement::decode(&announcement.encode()),
            Ok(announcement)
        );

        let announcement = Announcement::new(
            PROTOCOL,
            port(1),
            ProtocolCompatibility::DEFAULT,
            "two\nlines\u{7}",
        );
        assert_eq!(announcement.name(), "twolines");
    }

    /// Every way a datagram can fail to be an announce, and the refusal it
    /// earns.
    #[test]
    fn anything_but_an_exact_announce_is_refused() {
        let valid = sample().encode();
        let with = |at: usize, byte: u8| {
            let mut datagram = valid;
            datagram[at] = byte;
            datagram.to_vec()
        };
        // A name field holding exactly `name`, zeros after it, so only the
        // name's own content can be what is refused.
        let named = |name: &[u8]| {
            let mut datagram = valid;
            datagram[NAME_LEN_AT] = name.len() as u8;
            datagram[NAME_AT..].fill(0);
            datagram[NAME_AT..NAME_AT + name.len()].copy_from_slice(name);
            datagram.to_vec()
        };
        let invalid_utf8 = named(&[b'o', 0xC3, 0x28]);
        let control = named(b"a\nb");
        let mut zero_port = valid.to_vec();
        zero_port[GAME_PORT_AT..PLAYERS_AT].copy_from_slice(&[0, 0]);
        let cases: [(&str, Vec<u8>, Refusal); 11] = [
            ("empty", Vec::new(), Refusal::Malformed),
            (
                "one byte short",
                valid[..ANNOUNCE_BYTES - 1].to_vec(),
                Refusal::Malformed,
            ),
            (
                "one byte long",
                [valid.as_slice(), &[0]].concat(),
                Refusal::Malformed,
            ),
            ("the query's tag", with(0, QUERY_TAG), Refusal::Malformed),
            (
                "another version",
                with(1, DISCOVERY_VERSION + 1),
                Refusal::OtherVersion,
            ),
            ("a zero game port", zero_port, Refusal::Malformed),
            (
                "a name length past the cap",
                with(NAME_LEN_AT, MAX_NAME_BYTES as u8 + 1),
                Refusal::Malformed,
            ),
            (
                "a byte after the name",
                with(ANNOUNCE_BYTES - 1, b'x'),
                Refusal::Malformed,
            ),
            ("a name that is not UTF-8", invalid_utf8, Refusal::Malformed),
            ("a control character", control, Refusal::Malformed),
            (
                "a query",
                encode_query(PROTOCOL).to_vec(),
                Refusal::Malformed,
            ),
        ];
        for (what, datagram, refusal) in cases {
            assert_eq!(Announcement::decode(&datagram), Err(refusal), "{what}");
        }
    }

    /// A short query is the amplification attempt the padding exists to
    /// stop, and padding that is not zero is not a query either.
    #[test]
    fn anything_but_an_exact_query_is_refused() {
        let valid = encode_query(PROTOCOL);
        let mut dirty = valid;
        dirty[QUERY_BYTES - 1] = 1;
        let mut other_version = valid;
        other_version[1] = DISCOVERY_VERSION + 1;
        assert_eq!(
            decode_query(&valid[..HEADER_BYTES]),
            Err(Refusal::Malformed)
        );
        assert_eq!(
            decode_query(&valid[..QUERY_BYTES - 1]),
            Err(Refusal::Malformed)
        );
        assert_eq!(
            decode_query(&[valid.as_slice(), &[0]].concat()),
            Err(Refusal::Malformed)
        );
        assert_eq!(decode_query(&dirty), Err(Refusal::Malformed));
        assert_eq!(decode_query(&other_version), Err(Refusal::OtherVersion));
        assert_eq!(decode_query(&sample().encode()), Err(Refusal::Malformed));
        assert_eq!(decode_query(&[]), Err(Refusal::Malformed));
    }

    /// Seeded arbitrary bytes, and seeded corruptions of a valid announce
    /// and query — the latter pass the length and tag checks and reach the
    /// field parsers. None may panic.
    #[test]
    fn arbitrary_and_corrupted_bytes_never_panic_the_decoders() {
        let mut seed = 0x5EED_u64;
        let mut next = || {
            seed = seed.wrapping_mul(6364136223846793005).wrapping_add(1);
            seed >> 32
        };
        let announce = sample().encode();
        let query = encode_query(PROTOCOL);
        for _ in 0..20_000 {
            let len = (next() % (ANNOUNCE_BYTES as u64 + 8)) as usize;
            let bytes: Vec<u8> = (0..len).map(|_| next() as u8).collect();
            let _ = Announcement::decode(&bytes);
            let _ = decode_query(&bytes);

            for valid in [announce.as_slice(), query.as_slice()] {
                let mut corrupt = valid.to_vec();
                for _ in 0..=next() % 4 {
                    let at = (next() as usize) % corrupt.len();
                    corrupt[at] = next() as u8;
                }
                let _ = Announcement::decode(&corrupt);
                let _ = decode_query(&corrupt);
            }
        }
    }
}
