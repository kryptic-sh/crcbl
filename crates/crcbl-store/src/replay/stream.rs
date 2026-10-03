//! Writing a `.crpl` file while a session runs, without holding the session in
//! memory.
//!
//! [`ReplayWriter`](super::ReplayWriter) keeps everything until it writes, so
//! a recorder built on it grows by every peer's frames every tick for as long
//! as it records. The peer track is that growth, and it is also the last thing
//! in the file — but the format puts each list's count before its entries and
//! the sets and hashes before the track, so the track cannot go straight into
//! the file as it arrives. [`ReplayStream`] encodes each tick of it into a
//! spool as it comes instead, keeps the sets and hashes (rare, and a tick
//! and a value each), and writes the file in order when it finishes: the header,
//! the sets, the hashes, the track's count and then the spool, copied.
//!
//! Every entry is checked against the rules the reader holds as it is pushed,
//! so a refusal names the entry that broke one at the tick it happened, and
//! the file it finishes is one [`FileTransport`](super::FileTransport) reads.

use std::io::{self, Read, Seek, SeekFrom, Write};

use crcbl_core::TickId;
use crcbl_net::ConsoleSet;

use super::input::{self, InputSection, InputSectionError};
use super::{RecordedPeerTick, RecordedSimSet, RecordedStateHash, encode_header};
use crate::StorageError;

/// Writes a `.crpl` file — no output entries, and an input section — as a
/// session runs, without holding its peer track: the format puts each list's
/// count before its entries and the track last, so each tick of the track is
/// encoded into a spool as it comes and copied into the file when it
/// finishes, after the sets and hashes it does hold.
///
/// Push each applied set ([`push_sim_set`](Self::push_sim_set)), each hashed
/// tick ([`push_state_hash`](Self::push_state_hash)) and each tick of the
/// peer track ([`push_peer_tick`](Self::push_peer_tick)) as they happen, then
/// [`finish`](Self::finish) into the file. What it holds in memory is the sets
/// and the hashes; the track goes to the spool it was given, which is the
/// caller's to create and, once finished, to remove.
///
/// It writes no output entries — the messages a viewer plays back — so its
/// file's header counts no ticks and starts at tick zero: what it records is
/// what a re-simulation needs.
#[derive(Debug)]
pub struct ReplayStream<S> {
    tick_rate: u32,
    /// The sets and the hashes so far; its peer track stays empty, since the
    /// track is in the spool.
    section: InputSection,
    spool: S,
    /// The bytes of the spool that hold whole entries. A write that fails
    /// part-way leaves bytes past it, which the next write overwrites and
    /// `finish` never copies.
    spool_len: u64,
    /// Whether the spool's position may be past `spool_len`.
    spool_dirty: bool,
    peer_ticks: u32,
    last_peer_tick: Option<TickId>,
    /// One tick's entry, encoded; kept so its buffer is reused.
    scratch: Vec<u8>,
}

impl<S: Read + Write + Seek> ReplayStream<S> {
    /// A stream of a session at `tick_rate` Hz — stored in the header, as
    /// [`ReplayWriter::new`](super::ReplayWriter::new) stores it — spooling
    /// its peer track to `spool`, which must be empty.
    pub fn new(tick_rate: u32, spool: S) -> Self {
        Self {
            tick_rate,
            section: InputSection::default(),
            spool,
            spool_len: 0,
            spool_dirty: false,
            peer_ticks: 0,
            last_peer_tick: None,
            scratch: Vec::new(),
        }
    }

    /// Record a `Flags::SIM` console set the host applied at the start of
    /// `tick`, as [`ReplayWriter::push_sim_set`](super::ReplayWriter::push_sim_set)
    /// does.
    ///
    /// # Errors
    ///
    /// [`StorageError::ReplayInput`] for a set for an earlier tick than the
    /// one before it, or a name or value longer than a console set carries;
    /// nothing is recorded.
    pub fn push_sim_set(&mut self, tick: TickId, set: ConsoleSet) -> Result<(), StorageError> {
        let recorded = RecordedSimSet { tick, set };
        recorded.check_after(self.section.sim_sets.last().map(|last| last.tick))?;
        self.section.sim_sets.push(recorded);
        Ok(())
    }

    /// Record the state hash at the end of `tick`.
    ///
    /// # Errors
    ///
    /// [`StorageError::ReplayInput`] for a hash for a tick not after the one
    /// before it; nothing is recorded.
    pub fn push_state_hash(&mut self, tick: TickId, hash: u64) -> Result<(), StorageError> {
        let recorded = RecordedStateHash { tick, hash };
        recorded.check_after(self.section.state_hashes.last().map(|last| last.tick))?;
        self.section.state_hashes.push(recorded);
        Ok(())
    }

    /// Record what the host's module was handed of its peers at one tick,
    /// encoded into the spool at once.
    ///
    /// # Errors
    ///
    /// [`StorageError::ReplayInput`] for an entry for a tick not after the one
    /// before it, one peer's frames twice in a tick, more frames or longer
    /// ones than one host tick holds, or more entries than the track's count
    /// can say; [`StorageError::Io`] when the spool refused the write. Either
    /// way nothing is recorded, and the stream takes the next entry as if
    /// this one had not been pushed.
    pub fn push_peer_tick(&mut self, tick: &RecordedPeerTick) -> Result<(), StorageError> {
        input::check_tick(self.last_peer_tick, tick)?;
        if self.peer_ticks == u32::MAX {
            return Err(InputSectionError::TooMany {
                what: "peer ticks",
                count: usize::try_from(self.peer_ticks).map_or(usize::MAX, |n| n.saturating_add(1)),
            }
            .into());
        }
        self.scratch.clear();
        input::encode_tick(tick, &mut self.scratch)?;
        if self.spool_dirty {
            self.spool.seek(SeekFrom::Start(self.spool_len))?;
            self.spool_dirty = false;
        }
        if let Err(error) = self.spool.write_all(&self.scratch) {
            self.spool_dirty = true;
            return Err(error.into());
        }
        self.spool_len += self.scratch.len() as u64;
        self.peer_ticks += 1;
        self.last_peer_tick = Some(tick.tick);
        Ok(())
    }

    /// How many sets are recorded.
    pub fn sim_set_count(&self) -> usize {
        self.section.sim_sets.len()
    }

    /// How many state hashes are recorded.
    pub fn state_hash_count(&self) -> usize {
        self.section.state_hashes.len()
    }

    /// The tick of the first state hash recorded, and of the last.
    pub fn hashed_ticks(&self) -> Option<(TickId, TickId)> {
        let hashes = &self.section.state_hashes;
        Some((hashes.first()?.tick, hashes.last()?.tick))
    }

    /// How many ticks of the peer track are recorded.
    pub fn peer_tick_count(&self) -> usize {
        usize::try_from(self.peer_ticks).unwrap_or(usize::MAX)
    }

    /// Write the file into `out` — the header, the sets, the hashes, then the
    /// peer track copied from the spool — and hand the spool back, for the
    /// caller to remove. `out` is written in order and flushed, never read or
    /// seeked.
    ///
    /// # Errors
    ///
    /// [`StorageError::ReplayInput`] for more sets or hashes than a count can
    /// say, and [`StorageError::Io`] when the spool could not be read back or
    /// `out` refused a write — after which `out` holds a file cut short,
    /// which the reader refuses.
    pub fn finish(mut self, out: &mut impl Write) -> Result<S, StorageError> {
        let mut head = Vec::new();
        encode_header(&mut head, 0, self.tick_rate, 0);
        self.section.encode_sets_and_hashes(&mut head)?;
        head.extend_from_slice(&self.peer_ticks.to_le_bytes());
        out.write_all(&head)?;
        self.spool.seek(SeekFrom::Start(0))?;
        let copied = io::copy(&mut (&mut self.spool).take(self.spool_len), out)?;
        if copied != self.spool_len {
            return Err(io::Error::new(
                io::ErrorKind::UnexpectedEof,
                format!(
                    "the replay's spool held {copied} of the {} bytes written to it",
                    self.spool_len
                ),
            )
            .into());
        }
        out.flush()?;
        Ok(self.spool)
    }
}

#[cfg(test)]
mod tests {
    use std::io::Cursor;

    use super::*;
    use crate::replay::{
        FileTransport, RecordedPeerFrames, RecordedRosterChange, ReplayWriter, RosterChangeKind,
    };

    fn spin_rate(value: &str) -> ConsoleSet {
        ConsoleSet {
            name: "sv_spin_rate".to_owned(),
            value: value.to_owned(),
        }
    }

    fn peer_tick(tick: u64, peer: u64, frame: &[u8]) -> RecordedPeerTick {
        RecordedPeerTick {
            tick: TickId::from_raw(tick),
            roster: vec![RecordedRosterChange {
                kind: RosterChangeKind::Joined,
                peer,
            }],
            peers: vec![RecordedPeerFrames {
                peer,
                dropped: 2,
                frames: vec![(TickId::from_raw(tick - 1), frame.to_vec())],
            }],
        }
    }

    fn finished(stream: ReplayStream<Cursor<Vec<u8>>>) -> Vec<u8> {
        let mut out = Vec::new();
        stream.finish(&mut out).expect("a valid stream finishes");
        out
    }

    /// **A streamed file is the file `ReplayWriter` writes** from the same
    /// sets, hashes and track, byte for byte, and it reads back as them.
    #[test]
    fn a_streamed_file_is_byte_for_byte_the_writers() {
        let mut stream = ReplayStream::new(30, Cursor::new(Vec::new()));
        let mut writer = ReplayWriter::new(30);
        stream
            .push_sim_set(TickId::from_raw(2), spin_rate("2.5"))
            .unwrap();
        writer.push_sim_set(TickId::from_raw(2), spin_rate("2.5"));
        for tick in 1..=4 {
            stream
                .push_state_hash(TickId::from_raw(tick), tick * 7)
                .unwrap();
            writer.push_state_hash(TickId::from_raw(tick), tick * 7);
        }
        for (tick, peer) in [(2, 1), (4, 2)] {
            let entry = peer_tick(tick, peer, &[1, 2, 3]);
            stream.push_peer_tick(&entry).unwrap();
            writer.push_peer_tick(entry);
        }
        assert_eq!(stream.sim_set_count(), 1);
        assert_eq!(stream.state_hash_count(), 4);
        assert_eq!(stream.peer_tick_count(), 2);
        assert_eq!(
            stream.hashed_ticks(),
            Some((TickId::from_raw(1), TickId::from_raw(4)))
        );

        let bytes = finished(stream);
        assert_eq!(bytes, writer.encode().unwrap());
        let file = FileTransport::decode(&bytes).unwrap();
        assert_eq!(file.tick_rate(), 30);
        assert_eq!(file.peer_ticks(), writer.input.peer_ticks);
        assert_eq!(file.state_hashes(), writer.input.state_hashes);
        assert_eq!(file.sim_sets(), writer.input.sim_sets);
    }

    /// **An entry that breaks a rule is refused as it is pushed**, by name,
    /// and the stream goes on as if it had not been: the file it finishes
    /// holds the rest and reads back.
    #[test]
    fn an_entry_breaking_a_rule_is_refused_and_leaves_the_rest() {
        let mut stream = ReplayStream::new(60, Cursor::new(Vec::new()));
        stream.push_peer_tick(&peer_tick(5, 1, b"a")).unwrap();
        let err = stream.push_peer_tick(&peer_tick(5, 2, b"b")).unwrap_err();
        assert!(
            matches!(
                err,
                StorageError::ReplayInput(InputSectionError::PeerTickOutOfOrder { .. })
            ),
            "{err}"
        );
        stream.push_state_hash(TickId::from_raw(3), 1).unwrap();
        let err = stream.push_state_hash(TickId::from_raw(3), 1).unwrap_err();
        assert!(
            matches!(
                err,
                StorageError::ReplayInput(InputSectionError::HashOutOfOrder { .. })
            ),
            "{err}"
        );
        stream
            .push_sim_set(TickId::from_raw(4), spin_rate("1"))
            .unwrap();
        let err = stream
            .push_sim_set(TickId::from_raw(3), spin_rate("2"))
            .unwrap_err();
        assert!(
            matches!(
                err,
                StorageError::ReplayInput(InputSectionError::SetOutOfOrder { .. })
            ),
            "{err}"
        );
        stream.push_peer_tick(&peer_tick(6, 2, b"c")).unwrap();

        let file = FileTransport::decode(&finished(stream)).unwrap();
        let ticks: Vec<u64> = file.peer_ticks().iter().map(|t| t.tick.get()).collect();
        assert_eq!(ticks, [5, 6]);
        assert_eq!(file.state_hashes().len(), 1);
        assert_eq!(file.sim_sets().len(), 1);
    }

    /// A spool that refuses some writes, part-way through: the first write
    /// after `fail_after` bytes writes up to them and fails.
    #[derive(Debug)]
    struct FlakySpool {
        inner: Cursor<Vec<u8>>,
        fail_after: Option<u64>,
    }

    impl Write for FlakySpool {
        fn write(&mut self, buf: &[u8]) -> io::Result<usize> {
            if let Some(limit) = self.fail_after.take() {
                let room = usize::try_from(limit.saturating_sub(self.inner.position()))
                    .unwrap_or(usize::MAX);
                if room < buf.len() {
                    self.inner.write_all(&buf[..room])?;
                    return Err(io::Error::other("the disk is full"));
                }
                self.fail_after = Some(limit);
            }
            self.inner.write(buf)
        }

        fn flush(&mut self) -> io::Result<()> {
            self.inner.flush()
        }
    }

    impl Read for FlakySpool {
        fn read(&mut self, buf: &mut [u8]) -> io::Result<usize> {
            self.inner.read(buf)
        }
    }

    impl Seek for FlakySpool {
        fn seek(&mut self, pos: SeekFrom) -> io::Result<u64> {
            self.inner.seek(pos)
        }
    }

    /// **A spool write that fails part-way loses that entry and nothing
    /// else**: its torn bytes are overwritten by the next entry and never
    /// reach the file.
    #[test]
    fn a_spool_write_failing_part_way_leaves_a_whole_file() {
        let first = peer_tick(1, 1, &[9; 40]);
        let mut first_bytes = Vec::new();
        input::encode_tick(&first, &mut first_bytes).unwrap();
        let spool = FlakySpool {
            inner: Cursor::new(Vec::new()),
            // Room for the first entry and a few bytes of the second.
            fail_after: Some(first_bytes.len() as u64 + 5),
        };
        let mut stream = ReplayStream::new(60, spool);
        stream.push_peer_tick(&first).unwrap();
        let err = stream
            .push_peer_tick(&peer_tick(2, 2, &[8; 40]))
            .unwrap_err();
        assert!(matches!(err, StorageError::Io(_)), "{err}");
        assert_eq!(
            stream.peer_tick_count(),
            1,
            "the failed tick is not counted"
        );
        // The failed tick may be pushed again, or the next one.
        stream.push_peer_tick(&peer_tick(3, 2, &[7; 3])).unwrap();

        let mut out = Vec::new();
        stream.finish(&mut out).unwrap();
        let file = FileTransport::decode(&out).unwrap();
        let ticks: Vec<u64> = file.peer_ticks().iter().map(|t| t.tick.get()).collect();
        assert_eq!(ticks, [1, 3]);
        assert_eq!(file.peer_ticks()[1].peers[0].frames[0].1, [7; 3]);
    }

    /// **The empty stream finishes into a file that reads**: a header counting
    /// no ticks, and an empty section.
    #[test]
    fn an_empty_stream_finishes_into_an_empty_file() {
        let stream = ReplayStream::new(60, Cursor::new(Vec::new()));
        assert_eq!(stream.hashed_ticks(), None);
        let bytes = finished(stream);
        assert_eq!(bytes, ReplayWriter::new(60).encode().unwrap());
        let file = FileTransport::decode(&bytes).unwrap();
        assert!(file.is_empty());
        assert!(file.peer_ticks().is_empty());
    }
}
