//! What a towers host tells one player besides the snapshots: its map, at
//! join, and that a command the player sent was refused, and why.
//!
//! Both go through `crcbl_server::Host::send_event`, sealed on the reliable
//! channel, and arrive through `crcbl_client::Client::events` as opaque
//! bytes. So each carries a small envelope saying which it is:
//!
//! ```text
//!  byte 0    VERSION
//!  byte 1    MAP_TAG      | REFUSAL_TAG
//!  bytes 2…  Map::to_wire | one Refusal::code byte
//! ```
//!
//! **The version is the envelope's own**, apart from the handshake's
//! protocol version: a player of a build whose envelope differs refuses
//! every event it cannot read rather than reading one layout as another.
//!
//! # The bytes are a stranger's
//!
//! [`decode`] trusts none of them. A map's payload goes to
//! [`Map::from_wire`], which holds it to every rule a scene file is; a
//! refusal's is exactly one byte naming a [`Refusal`] this build knows. An
//! envelope of another version or with a tag this build does not know is an
//! [`EventError`] like any other — the player counts it and plays on, since
//! a host of a newer build may tell it things it cannot act on.

use crate::game::Refusal;
use crate::map::{Map, MapWireError};

/// The envelope's layout, which every event starts with.
pub const VERSION: u8 = 1;

/// The tag of the host's map: the payload is [`Map::to_wire`].
pub const MAP_TAG: u8 = 1;

/// The tag of a refused command: the payload is one [`Refusal::code`].
pub const REFUSAL_TAG: u8 = 2;

/// How many bytes the envelope puts ahead of the payload: the version and
/// the tag.
pub const HEADER_BYTES: usize = 2;

/// An event, read back.
#[derive(Clone, Debug, PartialEq)]
pub enum Event {
    /// The host's map, which a joiner builds its game on.
    Map(Map),
    /// A command this player sent was turned down, for this reason.
    Refused(Refusal),
}

/// Why [`decode`] read no event.
#[derive(Debug)]
pub enum EventError {
    /// Fewer bytes than the envelope's header.
    Truncated,
    /// An envelope of a layout this build does not read.
    UnknownVersion(u8),
    /// A tag this build does not know.
    UnknownTag(u8),
    /// A map this build refuses.
    Map(MapWireError),
    /// A refusal's payload is not exactly one byte.
    RefusalLength(usize),
    /// A refusal's byte names no [`Refusal`] this build knows.
    UnknownRefusal(u8),
}

impl std::fmt::Display for EventError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Truncated => write!(f, "shorter than an event's {HEADER_BYTES}-byte header"),
            Self::UnknownVersion(version) => {
                write!(
                    f,
                    "an event of version {version}, where this build reads {VERSION}"
                )
            }
            Self::UnknownTag(tag) => {
                write!(f, "an event tagged {tag}, which this build does not know")
            }
            Self::Map(error) => write!(f, "a map this build refuses: {error}"),
            Self::RefusalLength(len) => {
                write!(f, "a refusal of {len} bytes, where one is one byte")
            }
            Self::UnknownRefusal(code) => {
                write!(f, "a refusal coded {code}, which this build does not know")
            }
        }
    }
}

impl std::error::Error for EventError {}

/// The event carrying `wire` — a [`Map::to_wire`].
#[must_use]
pub fn map(wire: &[u8]) -> Vec<u8> {
    let mut bytes = Vec::with_capacity(HEADER_BYTES + wire.len());
    bytes.extend_from_slice(&[VERSION, MAP_TAG]);
    bytes.extend_from_slice(wire);
    bytes
}

/// The event telling a player their command was refused, for `refusal`.
#[must_use]
pub fn refusal(refusal: Refusal) -> Vec<u8> {
    vec![VERSION, REFUSAL_TAG, refusal.code()]
}

/// The event `bytes` carry — see the module docs for what is refused.
///
/// # Errors
///
/// [`EventError`], naming what was wrong.
pub fn decode(bytes: &[u8]) -> Result<Event, EventError> {
    let [version, tag, payload @ ..] = bytes else {
        return Err(EventError::Truncated);
    };
    if *version != VERSION {
        return Err(EventError::UnknownVersion(*version));
    }
    match *tag {
        MAP_TAG => Map::from_wire(payload)
            .map(Event::Map)
            .map_err(EventError::Map),
        REFUSAL_TAG => {
            let [code] = payload else {
                return Err(EventError::RefusalLength(payload.len()));
            };
            Refusal::from_code(*code)
                .map(Event::Refused)
                .ok_or(EventError::UnknownRefusal(*code))
        }
        other => Err(EventError::UnknownTag(other)),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// **Both events survive the envelope**: the map exactly, and every
    /// refusal this build knows.
    #[test]
    fn a_map_and_every_refusal_survive_the_envelope() {
        let map = crate::lan::tests::another_map();
        assert_eq!(
            decode(&super::map(&map.to_wire())).ok(),
            Some(Event::Map(map))
        );
        for reason in Refusal::ALL {
            assert_eq!(decode(&refusal(reason)).ok(), Some(Event::Refused(reason)));
        }
    }

    /// **Every byte of the envelope is checked**: too short, another
    /// version, an unknown tag, a map that is not one, a refusal of the
    /// wrong length or naming nothing — each refused by name, none a panic.
    #[test]
    fn an_event_this_build_cannot_read_is_refused_by_name() {
        let refused = |bytes: &[u8]| decode(bytes).expect_err("an event this build cannot read");
        assert!(matches!(refused(&[]), EventError::Truncated));
        assert!(matches!(refused(&[VERSION]), EventError::Truncated));
        assert!(matches!(
            refused(&[VERSION + 1, REFUSAL_TAG, Refusal::ALL[0].code()]),
            EventError::UnknownVersion(version) if version == VERSION + 1
        ));
        assert!(matches!(
            refused(&[VERSION, 0xee, 1]),
            EventError::UnknownTag(0xee)
        ));
        assert!(matches!(
            refused(&super::map(b"not a towers map, nor anything like one")),
            EventError::Map(MapWireError::NotAMap)
        ));
        assert!(matches!(
            refused(&[VERSION, REFUSAL_TAG]),
            EventError::RefusalLength(0)
        ));
        assert!(matches!(
            refused(&[VERSION, REFUSAL_TAG, 1, 1]),
            EventError::RefusalLength(2)
        ));
        assert!(matches!(
            refused(&[VERSION, REFUSAL_TAG, 0]),
            EventError::UnknownRefusal(0)
        ));
        assert!(matches!(
            refused(&[VERSION, REFUSAL_TAG, u8::MAX]),
            EventError::UnknownRefusal(u8::MAX)
        ));
    }
}
