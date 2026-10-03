//! The input section of a `.crpl` file, from format version 2: its records,
//! its codec and the rules both directions hold — the peer track, from
//! version 3, in `peers`. The layout is in the parent module's docs.

use crcbl_core::TickId;
use crcbl_net::ConsoleSet;
use crcbl_net::command::{MAX_CONSOLE_NAME_BYTES, MAX_CONSOLE_VALUE_BYTES};

mod peers;

pub use peers::{RecordedPeerFrames, RecordedPeerTick, RecordedRosterChange, RosterChangeKind};
pub(super) use peers::{check_tick, encode_tick};

/// The smallest `SimSetEntry`: a tick and two empty texts.
const MIN_SIM_SET_BYTES: usize = 8 + 2 + 2;

/// The size of every `StateHashEntry`: a tick and a hash.
const STATE_HASH_BYTES: usize = 8 + 8;

/// One `Flags::SIM` console set a host applied, as a replay records it.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct RecordedSimSet {
    /// The tick whose start it was applied at — the first tick that read it.
    pub tick: TickId,
    /// What was set, as text: the name as declared and the value as the
    /// console prints it.
    pub set: ConsoleSet,
}

/// The recorder's state hash at the end of one tick.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct RecordedStateHash {
    /// The tick it was taken at the end of.
    pub tick: TickId,
    /// The hash.
    pub hash: u64,
}

/// Why a replay's input section was refused, by the writer or the reader.
#[derive(Clone, Debug, PartialEq, Eq, thiserror::Error)]
#[non_exhaustive]
pub enum InputSectionError {
    /// The file ends inside the section.
    #[error("replay input section truncated in {0}")]
    Truncated(&'static str),
    /// A count the bytes after it cannot hold, refused before anything is
    /// reserved for it.
    #[error("replay input section declares {declared} {what} but only {remaining} bytes follow")]
    CountBeyondFile {
        /// What was counted.
        what: &'static str,
        /// The count the file declares.
        declared: u32,
        /// The bytes left after the count.
        remaining: usize,
    },
    /// More entries than the section's `u32` count can say.
    #[error("replay input section holds {count} {what}, more than its u32 count")]
    TooMany {
        /// What was counted.
        what: &'static str,
        /// How many there are.
        count: usize,
    },
    /// A set's name or value longer than a console set carries.
    #[error("a recorded set's {field} is {len} bytes, past the {limit} a set carries")]
    TextTooLong {
        /// `"name"` or `"value"`.
        field: &'static str,
        /// Its length, in bytes.
        len: usize,
        /// The most a set carries, in bytes.
        limit: usize,
    },
    /// A set's name or value that is not UTF-8.
    #[error("a recorded set's {0} is not UTF-8")]
    NotUtf8(&'static str),
    /// A set recorded for an earlier tick than the one before it.
    #[error(
        "a set recorded for tick {} follows one for tick {}, and sets are recorded in tick order",
        tick.get(),
        previous.get()
    )]
    SetOutOfOrder {
        /// The tick of the set before it.
        previous: TickId,
        /// Its own tick.
        tick: TickId,
    },
    /// A state hash for a tick not after the one before it.
    #[error(
        "a state hash for tick {} follows one for tick {}, and hashes are one a tick, in tick order",
        tick.get(),
        previous.get()
    )]
    HashOutOfOrder {
        /// The tick of the hash before it.
        previous: TickId,
        /// Its own tick.
        tick: TickId,
    },
    /// A roster change whose kind byte no build writes.
    #[error("a recorded roster change has kind {0}, which no build writes")]
    UnknownRosterChange(u8),
    /// A peer tick for a tick not after the one before it.
    #[error(
        "a peer tick for tick {} follows one for tick {}, and peer ticks are one a tick, in \
         tick order",
        tick.get(),
        previous.get()
    )]
    PeerTickOutOfOrder {
        /// The tick of the peer tick before it.
        previous: TickId,
        /// Its own tick.
        tick: TickId,
    },
    /// Two frame entries for one peer in one tick.
    #[error("tick {} records peer {peer}'s frames twice", tick.get())]
    PeerFramesTwice {
        /// The tick.
        tick: TickId,
        /// The peer.
        peer: u64,
    },
    /// More frames for one peer in one tick than a host's tick holds.
    #[error(
        "tick {} records {count} frames for peer {peer}, past the {limit} a tick holds",
        tick.get()
    )]
    TooManyFrames {
        /// The tick.
        tick: TickId,
        /// The peer.
        peer: u64,
        /// How many frames it records.
        count: usize,
        /// The most a tick holds.
        limit: usize,
    },
    /// A frame longer than the wire carries one.
    #[error(
        "tick {} records a frame of {len} bytes for peer {peer}, past the {limit} the wire \
         carries",
        tick.get()
    )]
    FrameTooLong {
        /// The tick.
        tick: TickId,
        /// The peer.
        peer: u64,
        /// Its length, in bytes.
        len: usize,
        /// The most the wire carries, in bytes.
        limit: usize,
    },
    /// Bytes after the section, which ends the file.
    #[error("{0} bytes follow the replay input section")]
    TrailingBytes(usize),
}

/// A replay's input section: the sets, the state hashes, then the peer
/// track.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub(super) struct InputSection {
    pub(super) sim_sets: Vec<RecordedSimSet>,
    pub(super) state_hashes: Vec<RecordedStateHash>,
    pub(super) peer_ticks: Vec<RecordedPeerTick>,
}

impl InputSection {
    /// Append the section to `buf`, peer track included, refusing what the
    /// reader would refuse.
    pub(super) fn encode(&self, buf: &mut Vec<u8>) -> Result<(), InputSectionError> {
        self.validate()?;
        self.encode_sets_and_hashes(buf)?;
        peers::encode(&self.peer_ticks, buf)
    }

    /// Append the sets and the hashes to `buf` — the section up to its peer
    /// track, which a streaming writer appends after them from its spool.
    /// Their rules are the caller's to have checked.
    pub(super) fn encode_sets_and_hashes(
        &self,
        buf: &mut Vec<u8>,
    ) -> Result<(), InputSectionError> {
        buf.extend_from_slice(&count(self.sim_sets.len(), "sets")?.to_le_bytes());
        for recorded in &self.sim_sets {
            buf.extend_from_slice(&recorded.tick.get().to_le_bytes());
            // `validate` held both texts to limits far below `u16::MAX`, so
            // this refuses nothing it let through.
            for (field, text) in [("name", &recorded.set.name), ("value", &recorded.set.value)] {
                let len =
                    u16::try_from(text.len()).map_err(|_| InputSectionError::TextTooLong {
                        field,
                        len: text.len(),
                        limit: usize::from(u16::MAX),
                    })?;
                buf.extend_from_slice(&len.to_le_bytes());
                buf.extend_from_slice(text.as_bytes());
            }
        }
        buf.extend_from_slice(&count(self.state_hashes.len(), "state hashes")?.to_le_bytes());
        for recorded in &self.state_hashes {
            buf.extend_from_slice(&recorded.tick.get().to_le_bytes());
            buf.extend_from_slice(&recorded.hash.to_le_bytes());
        }
        Ok(())
    }

    /// Read the section from `bytes`, which must be the rest of the file —
    /// with its peer track when `has_peer_track`, which a version 2 file's
    /// section ends without.
    pub(super) fn decode(bytes: &[u8], has_peer_track: bool) -> Result<Self, InputSectionError> {
        let mut reader = Reader { bytes };

        let set_count = reader.count("sets", MIN_SIM_SET_BYTES)?;
        let mut sim_sets = Vec::with_capacity(set_count);
        for _ in 0..set_count {
            let tick = TickId::from_raw(reader.u64("a set's tick")?);
            let name = reader.text("name")?;
            let value = reader.text("value")?;
            sim_sets.push(RecordedSimSet {
                tick,
                set: ConsoleSet { name, value },
            });
        }

        let hash_count = reader.count("state hashes", STATE_HASH_BYTES)?;
        let mut state_hashes = Vec::with_capacity(hash_count);
        for _ in 0..hash_count {
            let tick = TickId::from_raw(reader.u64("a state hash's tick")?);
            let hash = reader.u64("a state hash")?;
            state_hashes.push(RecordedStateHash { tick, hash });
        }

        let peer_ticks = if has_peer_track {
            peers::decode(&mut reader)?
        } else {
            Vec::new()
        };

        if !reader.bytes.is_empty() {
            return Err(InputSectionError::TrailingBytes(reader.bytes.len()));
        }
        let section = Self {
            sim_sets,
            state_hashes,
            peer_ticks,
        };
        section.validate()?;
        Ok(section)
    }

    /// The rules both directions hold: texts within a console set's limits,
    /// sets in tick order, hashes one a tick in tick order, and the peer
    /// track's own.
    fn validate(&self) -> Result<(), InputSectionError> {
        let mut previous = None;
        for recorded in &self.sim_sets {
            recorded.check_after(previous)?;
            previous = Some(recorded.tick);
        }
        let mut previous = None;
        for recorded in &self.state_hashes {
            recorded.check_after(previous)?;
            previous = Some(recorded.tick);
        }
        peers::validate(&self.peer_ticks)
    }
}

impl RecordedSimSet {
    /// The rules this set holds after one recorded for tick `previous`: texts
    /// within a console set's limits, and a tick no earlier.
    pub(super) fn check_after(&self, previous: Option<TickId>) -> Result<(), InputSectionError> {
        check_len("name", &self.set.name, MAX_CONSOLE_NAME_BYTES)?;
        check_len("value", &self.set.value, MAX_CONSOLE_VALUE_BYTES)?;
        match previous {
            Some(previous) if self.tick < previous => Err(InputSectionError::SetOutOfOrder {
                previous,
                tick: self.tick,
            }),
            _ => Ok(()),
        }
    }
}

impl RecordedStateHash {
    /// The rule this hash holds after one for tick `previous`: a later tick,
    /// since a tick has one end.
    pub(super) fn check_after(&self, previous: Option<TickId>) -> Result<(), InputSectionError> {
        match previous {
            Some(previous) if self.tick <= previous => Err(InputSectionError::HashOutOfOrder {
                previous,
                tick: self.tick,
            }),
            _ => Ok(()),
        }
    }
}

fn count(len: usize, what: &'static str) -> Result<u32, InputSectionError> {
    u32::try_from(len).map_err(|_| InputSectionError::TooMany { what, count: len })
}

fn check_len(field: &'static str, text: &str, limit: usize) -> Result<(), InputSectionError> {
    if text.len() > limit {
        return Err(InputSectionError::TextTooLong {
            field,
            len: text.len(),
            limit,
        });
    }
    Ok(())
}

/// The unread rest of the section.
struct Reader<'a> {
    bytes: &'a [u8],
}

impl<'a> Reader<'a> {
    fn take(&mut self, len: usize, what: &'static str) -> Result<&'a [u8], InputSectionError> {
        if self.bytes.len() < len {
            return Err(InputSectionError::Truncated(what));
        }
        let (taken, rest) = self.bytes.split_at(len);
        self.bytes = rest;
        Ok(taken)
    }

    fn array<const N: usize>(&mut self, what: &'static str) -> Result<[u8; N], InputSectionError> {
        let mut out = [0; N];
        out.copy_from_slice(self.take(N, what)?);
        Ok(out)
    }

    fn u64(&mut self, what: &'static str) -> Result<u64, InputSectionError> {
        self.array(what).map(u64::from_le_bytes)
    }

    /// A count of entries at least `min_entry` bytes each, refused when the
    /// rest of the section cannot hold that many — before the caller reserves
    /// for it, since the count comes from the file.
    fn count(&mut self, what: &'static str, min_entry: usize) -> Result<usize, InputSectionError> {
        let declared = u32::from_le_bytes(self.array(what)?);
        let remaining = self.bytes.len();
        let fits = usize::try_from(declared)
            .ok()
            .filter(|declared| *declared <= remaining / min_entry);
        fits.ok_or(InputSectionError::CountBeyondFile {
            what,
            declared,
            remaining,
        })
    }

    fn text(&mut self, field: &'static str) -> Result<String, InputSectionError> {
        let len = usize::from(u16::from_le_bytes(self.array(field)?));
        let bytes = self.take(len, field)?;
        let text = std::str::from_utf8(bytes).map_err(|_| InputSectionError::NotUtf8(field))?;
        Ok(text.to_owned())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn set(tick: u64, name: &str, value: &str) -> RecordedSimSet {
        RecordedSimSet {
            tick: TickId::from_raw(tick),
            set: ConsoleSet {
                name: name.to_owned(),
                value: value.to_owned(),
            },
        }
    }

    fn hash(tick: u64, hash: u64) -> RecordedStateHash {
        RecordedStateHash {
            tick: TickId::from_raw(tick),
            hash,
        }
    }

    fn sample() -> InputSection {
        InputSection {
            sim_sets: vec![
                set(30, "sv_spin_rate", "2.5"),
                set(30, "sv_spin_rate", "3"),
                set(61, "sv_spin_rate", "0.1"),
            ],
            state_hashes: vec![hash(1, 0xA1), hash(2, 0xB2), hash(90, u64::MAX)],
            peer_ticks: Vec::new(),
        }
    }

    fn encoded(section: &InputSection) -> Vec<u8> {
        let mut buf = Vec::new();
        section.encode(&mut buf).expect("a valid section");
        buf
    }

    /// `section` as a version 2 file holds it: without the peer track, which
    /// for a section with no peer ticks is its last four bytes, a zero count.
    fn encoded_v2(section: &InputSection) -> Vec<u8> {
        assert!(section.peer_ticks.is_empty(), "version 2 has no peer track");
        let mut buf = encoded(section);
        assert_eq!(buf.split_off(buf.len() - 4), [0; 4]);
        buf
    }

    #[test]
    fn a_section_reads_back_as_written() {
        let section = sample();
        assert_eq!(InputSection::decode(&encoded(&section), true), Ok(section));
        let empty = InputSection::default();
        assert_eq!(encoded(&empty), [0; 12], "three zero counts");
        assert_eq!(InputSection::decode(&encoded(&empty), true), Ok(empty));
    }

    #[test]
    fn a_version_2_section_reads_with_no_peer_track() {
        let section = sample();
        assert_eq!(
            InputSection::decode(&encoded_v2(&section), false),
            Ok(section)
        );
        assert_eq!(encoded_v2(&InputSection::default()), [0; 8]);
    }

    #[test]
    fn a_section_read_as_the_wrong_version_is_refused() {
        // A version 3 section read as version 2: its track's count is four
        // bytes the older layout ends before.
        assert_eq!(
            InputSection::decode(&encoded(&sample()), false),
            Err(InputSectionError::TrailingBytes(4))
        );
        // And a version 2 section read as version 3 ends where its track
        // should start.
        assert_eq!(
            InputSection::decode(&encoded_v2(&sample()), true),
            Err(InputSectionError::Truncated("peer ticks"))
        );
    }

    #[test]
    fn a_truncated_section_is_refused_by_where_it_ends() {
        let bytes = encoded_v2(&sample());
        assert_eq!(
            InputSection::decode(&[], false),
            Err(InputSectionError::Truncated("sets"))
        );
        // One set whose name is cut short: its tick, a length of 20 and two
        // bytes of name — as many bytes as the smallest set, so the count
        // passes and the name is where it ends.
        let mut cut = 1u32.to_le_bytes().to_vec();
        cut.extend_from_slice(&1u64.to_le_bytes());
        cut.extend_from_slice(&20u16.to_le_bytes());
        cut.extend_from_slice(b"sv");
        assert_eq!(
            InputSection::decode(&cut, false),
            Err(InputSectionError::Truncated("name"))
        );
        // Hashes are all one size, so a file cut inside one is caught by its
        // count.
        assert_eq!(
            InputSection::decode(&bytes[..bytes.len() - 1], false),
            Err(InputSectionError::CountBeyondFile {
                what: "state hashes",
                declared: 3,
                remaining: 3 * STATE_HASH_BYTES - 1,
            })
        );
    }

    #[test]
    fn a_count_past_the_file_is_refused_before_reserving() {
        let mut bytes = u32::MAX.to_le_bytes().to_vec();
        bytes.extend_from_slice(&[0; MIN_SIM_SET_BYTES]);
        assert_eq!(
            InputSection::decode(&bytes, false),
            Err(InputSectionError::CountBeyondFile {
                what: "sets",
                declared: u32::MAX,
                remaining: MIN_SIM_SET_BYTES,
            })
        );
        // No sets, then two hashes declared where one fits.
        let mut bytes = vec![0; 4];
        bytes.extend_from_slice(&2u32.to_le_bytes());
        bytes.extend_from_slice(&[0; STATE_HASH_BYTES]);
        assert_eq!(
            InputSection::decode(&bytes, false),
            Err(InputSectionError::CountBeyondFile {
                what: "state hashes",
                declared: 2,
                remaining: STATE_HASH_BYTES,
            })
        );
    }

    #[test]
    fn text_past_a_console_sets_limit_is_refused_both_ways() {
        let long_name = "n".repeat(MAX_CONSOLE_NAME_BYTES + 1);
        let section = InputSection {
            sim_sets: vec![set(1, &long_name, "1")],
            state_hashes: Vec::new(),
            peer_ticks: Vec::new(),
        };
        let refusal = InputSectionError::TextTooLong {
            field: "name",
            len: MAX_CONSOLE_NAME_BYTES + 1,
            limit: MAX_CONSOLE_NAME_BYTES,
        };
        assert_eq!(section.encode(&mut Vec::new()), Err(refusal.clone()));

        // The same set written by hand, as a hostile file would hold it.
        let mut bytes = 1u32.to_le_bytes().to_vec();
        bytes.extend_from_slice(&1u64.to_le_bytes());
        for text in [long_name.as_str(), "1"] {
            bytes.extend_from_slice(&(text.len() as u16).to_le_bytes());
            bytes.extend_from_slice(text.as_bytes());
        }
        bytes.extend_from_slice(&0u32.to_le_bytes());
        assert_eq!(InputSection::decode(&bytes, false), Err(refusal));

        let long_value = InputSection {
            sim_sets: vec![set(
                1,
                "sv_spin_rate",
                &"9".repeat(MAX_CONSOLE_VALUE_BYTES + 1),
            )],
            state_hashes: Vec::new(),
            peer_ticks: Vec::new(),
        };
        assert!(matches!(
            long_value.encode(&mut Vec::new()),
            Err(InputSectionError::TextTooLong { field: "value", .. })
        ));
    }

    #[test]
    fn text_that_is_not_utf8_is_refused_by_field() {
        let mut bytes = encoded_v2(&InputSection {
            sim_sets: vec![set(1, "ab", "cd")],
            state_hashes: Vec::new(),
            peer_ticks: Vec::new(),
        });
        // The value's first byte: count, tick, name length, name, value length.
        bytes[4 + 8 + 2 + 2 + 2] = 0xFF;
        assert_eq!(
            InputSection::decode(&bytes, false),
            Err(InputSectionError::NotUtf8("value"))
        );
    }

    #[test]
    fn ticks_out_of_order_are_refused_both_ways() {
        let backwards = InputSection {
            sim_sets: vec![set(5, "a", "1"), set(4, "a", "2")],
            state_hashes: Vec::new(),
            peer_ticks: Vec::new(),
        };
        let refusal = InputSectionError::SetOutOfOrder {
            previous: TickId::from_raw(5),
            tick: TickId::from_raw(4),
        };
        assert_eq!(backwards.encode(&mut Vec::new()), Err(refusal.clone()));
        let mut bytes = encoded_v2(&InputSection {
            sim_sets: vec![set(5, "a", "1"), set(6, "a", "2")],
            state_hashes: Vec::new(),
            peer_ticks: Vec::new(),
        });
        // The second set's tick: count, then the first set's 14 bytes.
        let second = 4 + 8 + 2 + 1 + 2 + 1;
        bytes[second..second + 8].copy_from_slice(&4u64.to_le_bytes());
        assert_eq!(InputSection::decode(&bytes, false), Err(refusal));

        // Two hashes for one tick: a hash is the state at a tick's end, and a
        // tick has one end.
        let twice = InputSection {
            sim_sets: Vec::new(),
            state_hashes: vec![hash(3, 1), hash(3, 1)],
            peer_ticks: Vec::new(),
        };
        let refusal = InputSectionError::HashOutOfOrder {
            previous: TickId::from_raw(3),
            tick: TickId::from_raw(3),
        };
        assert_eq!(twice.encode(&mut Vec::new()), Err(refusal.clone()));
        let mut bytes = encoded_v2(&InputSection {
            sim_sets: Vec::new(),
            state_hashes: vec![hash(3, 1), hash(4, 1)],
            peer_ticks: Vec::new(),
        });
        let second = 4 + 4 + STATE_HASH_BYTES;
        bytes[second..second + 8].copy_from_slice(&3u64.to_le_bytes());
        assert_eq!(InputSection::decode(&bytes, false), Err(refusal));
    }

    #[test]
    fn bytes_after_the_section_are_refused() {
        let mut bytes = encoded_v2(&sample());
        bytes.push(0);
        assert_eq!(
            InputSection::decode(&bytes, false),
            Err(InputSectionError::TrailingBytes(1))
        );
    }
}
