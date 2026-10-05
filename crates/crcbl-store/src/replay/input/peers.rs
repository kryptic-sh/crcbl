//! The input section's peer track: per tick, the changes to the recorded
//! host's roster — each join with the player it names — and the input frames
//! each peer's module was handed. The layout is in `replay`'s module docs.

use crcbl_core::{PlayerId, TickId};
use crcbl_net::MAX_CLIENT_INPUTS_PER_TICK;
use crcbl_net::codec::MAX_FIELD_BYTES;

use super::{InputSectionError, Reader, count};

/// The smallest `PeerTickEntry`: a tick and two zero counts.
const MIN_PEER_TICK_BYTES: usize = 8 + 4 + 4;

/// The smallest `RosterEntry`: a kind byte and a peer. A join that names its
/// player has the player after them.
const MIN_ROSTER_ENTRY_BYTES: usize = 1 + 8;

/// The smallest `PeerFramesEntry`: a peer, a dropped count and a zero frame
/// count.
const MIN_PEER_FRAMES_BYTES: usize = 8 + 4 + 4;

/// The smallest `FrameEntry`: a tick and a zero length.
const MIN_FRAME_BYTES: usize = 8 + 4;

/// How a peer's place in the recorded host's roster changed — the kind of a
/// [`RecordedRosterChange`].
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum RosterChangeKind {
    /// A new session was admitted, last in admission order, for the player
    /// its hello named — `None` when the recording does not say: a file
    /// written before format version 4, which recorded no players, migrates
    /// with every join `None`.
    Joined(Option<PlayerId>),
    /// The peer's link dropped; it keeps its place, with no input.
    Lost,
    /// A lost peer came back to the same session.
    Resumed,
    /// The host ended the session: a grace period ran out, or a client never
    /// took the session up.
    Left,
    /// The game ended the session — a kick, or a shutdown ending every one.
    Ended,
}

/// The byte of a join that names no player: every join before format
/// version 4.
const JOINED_CODE: u8 = 1;

/// The byte of a join that names its player, which follows the peer.
const JOINED_AS_PLAYER_CODE: u8 = 6;

impl RosterChangeKind {
    /// Every kind whose code is followed by nothing past the peer, in
    /// [`code`](Self::code) order.
    const BARE: [Self; 5] = [
        Self::Joined(None),
        Self::Lost,
        Self::Resumed,
        Self::Left,
        Self::Ended,
    ];

    /// Its byte in the file. Written out rather than derived from the
    /// declaration order, so reordering the variants cannot change what an
    /// older file means. A join that names its player took a byte of its own
    /// rather than changing [`JOINED_CODE`]'s layout, so a version 3 track is
    /// a version 4 track whose joins name nobody, unchanged.
    const fn code(self) -> u8 {
        match self {
            Self::Joined(None) => JOINED_CODE,
            Self::Joined(Some(_)) => JOINED_AS_PLAYER_CODE,
            Self::Lost => 2,
            Self::Resumed => 3,
            Self::Left => 4,
            Self::Ended => 5,
        }
    }
}

/// One change to the recorded host's roster.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct RecordedRosterChange {
    /// What changed.
    pub kind: RosterChangeKind,
    /// The peer it changed for, as the host numbered it.
    pub peer: u64,
}

/// The input frames one peer's module was handed for one tick.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct RecordedPeerFrames {
    /// The peer, as the host numbered it.
    pub peer: u64,
    /// How many further frames the host's per-tick cap refused.
    pub dropped: u32,
    /// The frames, in the order the module read them: each the tick its
    /// client stamped on it and its bytes. At most
    /// [`MAX_CLIENT_INPUTS_PER_TICK`], each at most
    /// [`MAX_FIELD_BYTES`] long.
    pub frames: Vec<(TickId, Vec<u8>)>,
}

/// One tick's entry in the peer track: what changed in the roster before the
/// module ran, in the order the host applied it, and the frames of every peer
/// that had any. A peer in the roster with no entry here was handed nothing.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct RecordedPeerTick {
    /// The tick whose module read it.
    pub tick: TickId,
    /// The roster's changes, in the order applied.
    pub roster: Vec<RecordedRosterChange>,
    /// Each peer's frames, at most one entry a peer.
    pub peers: Vec<RecordedPeerFrames>,
}

/// Append the track to `buf`. `validate` has held every count and length to
/// limits far below `u32::MAX`, so the conversions refuse nothing it let
/// through.
pub(super) fn encode(
    ticks: &[RecordedPeerTick],
    buf: &mut Vec<u8>,
) -> Result<(), InputSectionError> {
    buf.extend_from_slice(&count(ticks.len(), "peer ticks")?.to_le_bytes());
    for entry in ticks {
        encode_tick(entry, buf)?;
    }
    Ok(())
}

/// Append one `PeerTickEntry` to `buf` — what the track's count is followed
/// by, one entry at a time, so a writer streaming the track encodes each tick
/// as it comes. [`check_tick`] holds the counts and lengths this converts.
pub(in crate::replay) fn encode_tick(
    entry: &RecordedPeerTick,
    buf: &mut Vec<u8>,
) -> Result<(), InputSectionError> {
    buf.extend_from_slice(&entry.tick.get().to_le_bytes());
    buf.extend_from_slice(&count(entry.roster.len(), "roster changes")?.to_le_bytes());
    for change in &entry.roster {
        buf.push(change.kind.code());
        buf.extend_from_slice(&change.peer.to_le_bytes());
        if let RosterChangeKind::Joined(Some(player)) = change.kind {
            buf.extend_from_slice(&player.to_bytes());
        }
    }
    buf.extend_from_slice(&count(entry.peers.len(), "peers' frames")?.to_le_bytes());
    for peer in &entry.peers {
        buf.extend_from_slice(&peer.peer.to_le_bytes());
        buf.extend_from_slice(&peer.dropped.to_le_bytes());
        buf.extend_from_slice(&count(peer.frames.len(), "frames")?.to_le_bytes());
        for (tick, data) in &peer.frames {
            buf.extend_from_slice(&tick.get().to_le_bytes());
            buf.extend_from_slice(&count(data.len(), "frame bytes")?.to_le_bytes());
            buf.extend_from_slice(data);
        }
    }
    Ok(())
}

/// Read the track from `reader`, leaving what follows it unread.
pub(super) fn decode(reader: &mut Reader<'_>) -> Result<Vec<RecordedPeerTick>, InputSectionError> {
    let tick_count = reader.count("peer ticks", MIN_PEER_TICK_BYTES)?;
    let mut ticks = Vec::with_capacity(tick_count);
    for _ in 0..tick_count {
        ticks.push(decode_tick(reader)?);
    }
    Ok(ticks)
}

/// Read one `PeerTickEntry` from `reader` — what [`encode_tick`] wrote. Its
/// rules are the caller's to check ([`check_tick`]).
pub(in crate::replay) fn decode_tick(
    reader: &mut Reader<'_>,
) -> Result<RecordedPeerTick, InputSectionError> {
    let tick = TickId::from_raw(reader.u64("a peer tick's tick")?);

    let change_count = reader.count("roster changes", MIN_ROSTER_ENTRY_BYTES)?;
    let mut roster = Vec::with_capacity(change_count);
    for _ in 0..change_count {
        let [code] = reader.array("a roster change's kind")?;
        let peer = reader.u64("a roster change's peer")?;
        let kind = if code == JOINED_AS_PLAYER_CODE {
            RosterChangeKind::Joined(Some(PlayerId::from_bytes(reader.array("a join's player")?)))
        } else {
            RosterChangeKind::BARE
                .into_iter()
                .find(|kind| kind.code() == code)
                .ok_or(InputSectionError::UnknownRosterChange(code))?
        };
        roster.push(RecordedRosterChange { kind, peer });
    }

    let peer_count = reader.count("peers' frames", MIN_PEER_FRAMES_BYTES)?;
    let mut peers = Vec::with_capacity(peer_count);
    for _ in 0..peer_count {
        let peer = reader.u64("a peer's id")?;
        let dropped = u32::from_le_bytes(reader.array("a peer's dropped count")?);
        let frame_count = reader.count("frames", MIN_FRAME_BYTES)?;
        let mut frames = Vec::with_capacity(frame_count);
        for _ in 0..frame_count {
            let frame_tick = TickId::from_raw(reader.u64("a frame's tick")?);
            let len = u32::from_le_bytes(reader.array("a frame's length")?);
            // A length past this target's address space cannot fit in the
            // bytes that follow either.
            let len = usize::try_from(len).map_err(|_| InputSectionError::Truncated("a frame"))?;
            frames.push((frame_tick, reader.take(len, "a frame")?.to_vec()));
        }
        peers.push(RecordedPeerFrames {
            peer,
            dropped,
            frames,
        });
    }
    Ok(RecordedPeerTick {
        tick,
        roster,
        peers,
    })
}

/// The rules both directions hold: ticks in order, one entry a peer a tick,
/// and no more frames, nor longer ones, than a host's tick holds.
pub(super) fn validate(ticks: &[RecordedPeerTick]) -> Result<(), InputSectionError> {
    let mut previous = None;
    for entry in ticks {
        check_tick(previous, entry)?;
        previous = Some(entry.tick);
    }
    Ok(())
}

/// The rules `entry` holds after a track whose last tick is `previous`: a
/// later tick, one entry a peer, and no more frames, nor longer ones, than a
/// host's tick holds — [`validate`]'s, one entry at a time.
pub(in crate::replay) fn check_tick(
    previous: Option<TickId>,
    entry: &RecordedPeerTick,
) -> Result<(), InputSectionError> {
    if let Some(previous) = previous
        && entry.tick <= previous
    {
        return Err(InputSectionError::PeerTickOutOfOrder {
            previous,
            tick: entry.tick,
        });
    }
    for (index, peer) in entry.peers.iter().enumerate() {
        if entry.peers[..index]
            .iter()
            .any(|seen| seen.peer == peer.peer)
        {
            return Err(InputSectionError::PeerFramesTwice {
                tick: entry.tick,
                peer: peer.peer,
            });
        }
        if peer.frames.len() > MAX_CLIENT_INPUTS_PER_TICK {
            return Err(InputSectionError::TooManyFrames {
                tick: entry.tick,
                peer: peer.peer,
                count: peer.frames.len(),
                limit: MAX_CLIENT_INPUTS_PER_TICK,
            });
        }
        if let Some((_, data)) = peer
            .frames
            .iter()
            .find(|(_, data)| data.len() > MAX_FIELD_BYTES)
        {
            return Err(InputSectionError::FrameTooLong {
                tick: entry.tick,
                peer: peer.peer,
                len: data.len(),
                limit: MAX_FIELD_BYTES,
            });
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::super::InputSection;
    use super::*;

    fn frames(peer: u64, frames: &[(u64, &[u8])]) -> RecordedPeerFrames {
        RecordedPeerFrames {
            peer,
            dropped: 0,
            frames: frames
                .iter()
                .map(|(tick, data)| (TickId::from_raw(*tick), data.to_vec()))
                .collect(),
        }
    }

    fn change(kind: RosterChangeKind, peer: u64) -> RecordedRosterChange {
        RecordedRosterChange { kind, peer }
    }

    /// The player `sample`'s first join names.
    fn player() -> PlayerId {
        PlayerId::from_seed(11)
    }

    /// A track whose first tick holds a join naming its player and one that
    /// names nobody, as a re-recorded migrated file's would.
    fn sample() -> Vec<RecordedPeerTick> {
        vec![
            RecordedPeerTick {
                tick: TickId::from_raw(1),
                roster: vec![
                    change(RosterChangeKind::Joined(Some(player())), 1),
                    change(RosterChangeKind::Joined(None), 2),
                ],
                peers: Vec::new(),
            },
            RecordedPeerTick {
                tick: TickId::from_raw(2),
                roster: Vec::new(),
                peers: vec![
                    frames(2, &[(7, b"\x01\xFF\x00\xFF")]),
                    RecordedPeerFrames {
                        dropped: 3,
                        ..frames(1, &[(6, b"ab"), (7, b"")])
                    },
                ],
            },
            RecordedPeerTick {
                tick: TickId::from_raw(9),
                roster: vec![
                    change(RosterChangeKind::Lost, 1),
                    change(RosterChangeKind::Resumed, 1),
                    change(RosterChangeKind::Left, 2),
                    change(RosterChangeKind::Ended, 1),
                ],
                peers: Vec::new(),
            },
        ]
    }

    fn section(peer_ticks: Vec<RecordedPeerTick>) -> InputSection {
        InputSection {
            peer_ticks,
            ..InputSection::default()
        }
    }

    fn encoded(section: &InputSection) -> Vec<u8> {
        let mut buf = Vec::new();
        section.encode(&mut buf).expect("a valid section");
        buf
    }

    /// Where the track starts in an encoded section with no sets and no
    /// hashes: after their two counts.
    const TRACK: usize = 4 + 4;

    #[test]
    fn the_track_reads_back_as_written() {
        let written = section(sample());
        assert_eq!(InputSection::decode(&encoded(&written)), Ok(written));
    }

    #[test]
    fn every_roster_kind_keeps_its_code() {
        // The codes are the file's: a kind that read back as another would
        // swap a join for a leave in every recording made before the change.
        let codes: Vec<u8> = RosterChangeKind::BARE
            .iter()
            .map(|kind| kind.code())
            .collect();
        assert_eq!(codes, [1, 2, 3, 4, 5]);
        assert_eq!(RosterChangeKind::Joined(Some(player())).code(), 6);
    }

    /// **A join's player is the sixteen bytes after its peer**, under a code
    /// of its own, and reads back as the same player; the join beside it
    /// that names nobody keeps version 3's layout.
    #[test]
    fn a_join_carries_its_player_after_its_peer_and_one_without_keeps_the_old_layout() {
        let bytes = encoded(&section(sample()[..1].to_vec()));
        // The track's count, the tick and its change count.
        let first = TRACK + 4 + 8 + 4;
        assert_eq!(bytes[first], JOINED_AS_PLAYER_CODE);
        assert_eq!(bytes[first + 1..first + 9], 1u64.to_le_bytes());
        let player_at = first + MIN_ROSTER_ENTRY_BYTES;
        assert_eq!(
            bytes[player_at..player_at + PlayerId::BYTES],
            player().to_bytes()
        );
        let second = player_at + PlayerId::BYTES;
        assert_eq!(bytes[second], JOINED_CODE);
        assert_eq!(bytes[second + 1..second + 9], 2u64.to_le_bytes());

        let read = InputSection::decode(&bytes).expect("it reads");
        assert_eq!(
            read.peer_ticks[0].roster,
            [
                change(RosterChangeKind::Joined(Some(player())), 1),
                change(RosterChangeKind::Joined(None), 2),
            ]
        );
    }

    /// **A join cut inside its player is refused by where it ends**: the
    /// change count is bounded by the smallest entry, so the player is what
    /// runs out.
    #[test]
    fn a_join_cut_inside_its_player_is_refused_by_where_it_ends() {
        let joined = section(vec![RecordedPeerTick {
            tick: TickId::from_raw(1),
            roster: vec![change(RosterChangeKind::Joined(Some(player())), 1)],
            peers: Vec::new(),
        }]);
        let bytes = encoded(&joined);
        // Cut the player short, and the peer count after it with it.
        let cut = &bytes[..bytes.len() - 4 - 1];
        assert_eq!(
            InputSection::decode(cut),
            Err(InputSectionError::Truncated("a join's player"))
        );
    }

    #[test]
    fn an_unknown_roster_kind_is_refused_by_its_byte() {
        let mut bytes = encoded(&section(sample()));
        // The first change's kind: the track's count, the first tick and its
        // change count.
        bytes[TRACK + 4 + 8 + 4] = 0x7F;
        assert_eq!(
            InputSection::decode(&bytes),
            Err(InputSectionError::UnknownRosterChange(0x7F))
        );
    }

    #[test]
    fn peer_ticks_out_of_order_are_refused_both_ways() {
        let mut backwards = sample();
        backwards[2].tick = TickId::from_raw(2);
        let refusal = InputSectionError::PeerTickOutOfOrder {
            previous: TickId::from_raw(2),
            tick: TickId::from_raw(2),
        };
        assert_eq!(
            section(backwards).encode(&mut Vec::new()),
            Err(refusal.clone())
        );

        let mut bytes = encoded(&section(sample()[..2].to_vec()));
        // The second tick's tick: count, then the first tick's entry — its
        // tick, its two changes, one with its player, and its zero peers.
        let second = TRACK + 4 + 8 + 4 + 2 * MIN_ROSTER_ENTRY_BYTES + PlayerId::BYTES + 4;
        bytes[second..second + 8].copy_from_slice(&1u64.to_le_bytes());
        assert_eq!(
            InputSection::decode(&bytes),
            Err(InputSectionError::PeerTickOutOfOrder {
                previous: TickId::from_raw(1),
                tick: TickId::from_raw(1),
            })
        );
    }

    #[test]
    fn one_peer_twice_in_a_tick_is_refused_both_ways() {
        let twice = vec![RecordedPeerTick {
            tick: TickId::from_raw(4),
            roster: Vec::new(),
            peers: vec![frames(3, &[(1, b"a")]), frames(3, &[(2, b"b")])],
        }];
        let refusal = InputSectionError::PeerFramesTwice {
            tick: TickId::from_raw(4),
            peer: 3,
        };
        assert_eq!(
            section(twice.clone()).encode(&mut Vec::new()),
            Err(refusal.clone())
        );
        let mut distinct = twice;
        distinct[0].peers[1].peer = 5;
        let mut bytes = encoded(&section(distinct));
        // The second peer's id: count, tick, zero changes, peer count, then
        // the first peer's entry with its one frame of one byte.
        let second = TRACK + 4 + 8 + 4 + 4 + MIN_PEER_FRAMES_BYTES + MIN_FRAME_BYTES + 1;
        bytes[second..second + 8].copy_from_slice(&3u64.to_le_bytes());
        assert_eq!(InputSection::decode(&bytes), Err(refusal));
    }

    #[test]
    fn more_frames_than_a_tick_holds_are_refused_both_ways() {
        let frame: &[u8] = b"f";
        let too_many: Vec<(u64, &[u8])> = (0..=MAX_CLIENT_INPUTS_PER_TICK as u64)
            .map(|tick| (tick, frame))
            .collect();
        let crowded = section(vec![RecordedPeerTick {
            tick: TickId::from_raw(1),
            roster: Vec::new(),
            peers: vec![frames(1, &too_many)],
        }]);
        let refusal = InputSectionError::TooManyFrames {
            tick: TickId::from_raw(1),
            peer: 1,
            count: MAX_CLIENT_INPUTS_PER_TICK + 1,
            limit: MAX_CLIENT_INPUTS_PER_TICK,
        };
        assert_eq!(crowded.encode(&mut Vec::new()), Err(refusal.clone()));

        // As many as a tick holds is written, and the same bytes with one
        // more frame spliced in are what a hostile file would hold.
        let full = section(vec![RecordedPeerTick {
            tick: TickId::from_raw(1),
            roster: Vec::new(),
            peers: vec![frames(1, &too_many[1..])],
        }]);
        let mut bytes = encoded(&full);
        let frame_count = TRACK + 4 + 8 + 4 + 4 + 8 + 4;
        bytes[frame_count..frame_count + 4]
            .copy_from_slice(&((MAX_CLIENT_INPUTS_PER_TICK + 1) as u32).to_le_bytes());
        let mut spliced = 0u64.to_le_bytes().to_vec();
        spliced.extend_from_slice(&1u32.to_le_bytes());
        spliced.extend_from_slice(frame);
        bytes.splice(frame_count + 4..frame_count + 4, spliced);
        assert_eq!(InputSection::decode(&bytes), Err(refusal));
    }

    #[test]
    fn a_frame_longer_than_the_wire_carries_is_refused_both_ways() {
        let long = vec![0xAB; MAX_FIELD_BYTES + 1];
        let oversized = section(vec![RecordedPeerTick {
            tick: TickId::from_raw(2),
            roster: Vec::new(),
            peers: vec![frames(4, &[(1, &long)])],
        }]);
        let refusal = InputSectionError::FrameTooLong {
            tick: TickId::from_raw(2),
            peer: 4,
            len: MAX_FIELD_BYTES + 1,
            limit: MAX_FIELD_BYTES,
        };
        assert_eq!(oversized.encode(&mut Vec::new()), Err(refusal.clone()));

        let fits = section(vec![RecordedPeerTick {
            tick: TickId::from_raw(2),
            roster: Vec::new(),
            peers: vec![frames(4, &[(1, &long[1..])])],
        }]);
        let mut bytes = encoded(&fits);
        let len_at = TRACK + 4 + 8 + 4 + 4 + MIN_PEER_FRAMES_BYTES + 8;
        bytes[len_at..len_at + 4].copy_from_slice(&((MAX_FIELD_BYTES + 1) as u32).to_le_bytes());
        bytes.push(0xAB);
        assert_eq!(InputSection::decode(&bytes), Err(refusal));
    }

    #[test]
    fn a_truncated_track_is_refused_by_where_it_ends() {
        let bytes = encoded(&section(sample()));
        assert_eq!(
            InputSection::decode(&bytes[..TRACK]),
            Err(InputSectionError::Truncated("peer ticks"))
        );
        // A frame cut short: its length says four bytes, and two follow — as
        // many as the smallest entries after it, so the counts pass and the
        // frame is where it ends.
        let one = section(vec![RecordedPeerTick {
            tick: TickId::from_raw(1),
            roster: Vec::new(),
            peers: vec![frames(1, &[(1, b"abcd")])],
        }]);
        let whole = encoded(&one);
        assert_eq!(
            InputSection::decode(&whole[..whole.len() - 2]),
            Err(InputSectionError::Truncated("a frame"))
        );
    }

    #[test]
    fn a_count_past_the_file_is_refused_before_reserving() {
        let mut bytes = vec![0; TRACK];
        bytes.extend_from_slice(&u32::MAX.to_le_bytes());
        bytes.extend_from_slice(&[0; MIN_PEER_TICK_BYTES]);
        assert_eq!(
            InputSection::decode(&bytes),
            Err(InputSectionError::CountBeyondFile {
                what: "peer ticks",
                declared: u32::MAX,
                remaining: MIN_PEER_TICK_BYTES,
            })
        );
    }
}
