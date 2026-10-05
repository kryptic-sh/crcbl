//! Writing a `.crpl` file while a session runs, without holding the session in
//! memory, so that a session that never finishes still leaves a file to
//! recover.
//!
//! [`ReplayWriter`](super::ReplayWriter) keeps everything until it writes, so
//! a recorder built on it grows by every peer's frames every tick for as long
//! as it records, and a process killed mid-session leaves nothing. The format
//! puts each list's count before its entries and the sets and hashes before
//! the peer track, so nothing can go straight into the file as it arrives.
//! [`ReplayStream`] appends each set, hash and tick of the peer track to a
//! spool as it comes instead — each record framed and checksummed, the layout
//! in [`spool`]'s module docs — and writes the file in order when it
//! finishes, from the spool: the header, then each list's count and its
//! records. A spool whose stream never finished is turned into the same
//! file by [`recover_spool`](super::recover_spool).
//!
//! Every entry is checked against the rules the reader holds as it is pushed,
//! so a refusal names the entry that broke one at the tick it happened, and
//! the file it finishes is one [`FileTransport`](super::FileTransport) reads.

use std::io::{Read, Seek, SeekFrom, Write};

use crcbl_core::TickId;
use crcbl_net::ConsoleSet;

use super::RecordedPeerTick;
use super::input::{self, RecordedSimSet, RecordedStateHash};
use super::spool::{self, RecordKind, SpoolEnd, SpoolError, Tally};
use crate::StorageError;

/// Writes a `.crpl` file — no output entries, and an input section — as a
/// session runs, holding none of it: each set, hash and tick of the peer
/// track is appended to a spool, framed so a reader can tell a whole record
/// from a torn one, and the file is written from the spool when it finishes.
///
/// Push each applied set ([`push_sim_set`](Self::push_sim_set)), each hashed
/// tick ([`push_state_hash`](Self::push_state_hash)) and each tick of the
/// peer track ([`push_peer_tick`](Self::push_peer_tick)) as they happen,
/// [`flush`](Self::flush) them to the spool — in one write, however many
/// there are, so a recorder flushing once a tick pays for one write a tick —
/// and [`finish`](Self::finish) into the file. The spool is the caller's to
/// create and, once finished, to remove; a process killed before it finishes
/// leaves the spool holding every record flushed whole, which
/// [`recover_spool`](super::recover_spool) turns into the file. Nothing is
/// synced to the disk: a record flushed is in the system's hands, which
/// survives the process but not the machine.
///
/// It writes no output entries — the messages a viewer plays back — so its
/// file's header counts no ticks and starts at tick zero: what it records is
/// what a re-simulation needs.
#[derive(Debug)]
pub struct ReplayStream<S> {
    spool: S,
    /// The bytes of the spool that hold its header and whole records. A
    /// write that fails part-way leaves bytes past it, which the next write
    /// overwrites and `finish` never reads.
    spool_len: u64,
    /// Whether the spool's position may be past `spool_len`.
    spool_dirty: bool,
    /// What the spool holds, and the ticks the next of each must come after.
    flushed: Tally,
    /// The same, with the records pushed since the last flush.
    pushed: Tally,
    /// The records pushed since the last flush, framed.
    pending: Vec<u8>,
}

impl<S: Read + Write + Seek> ReplayStream<S> {
    /// A stream of a session at `tick_rate` Hz — stored in the header, as
    /// [`ReplayWriter::new`](super::ReplayWriter::new) stores it — spooling
    /// to `spool`, which must be empty, and whose header it writes now.
    ///
    /// # Errors
    ///
    /// [`StorageError::Io`] when the spool refused the header.
    pub fn new(tick_rate: u32, mut spool: S) -> Result<Self, StorageError> {
        let header = spool::encode_header(tick_rate);
        spool.write_all(&header)?;
        Ok(Self {
            spool,
            spool_len: header.len() as u64,
            spool_dirty: false,
            flushed: Tally::default(),
            pushed: Tally::default(),
            pending: Vec::new(),
        })
    }

    /// Record a `Flags::SIM` console set the host applied at the start of
    /// `tick`, as [`ReplayWriter::push_sim_set`](super::ReplayWriter::push_sim_set)
    /// does.
    ///
    /// # Errors
    ///
    /// [`StorageError::ReplayInput`] for a set for an earlier tick than the
    /// one before it, a name or value longer than a console set carries, or
    /// more sets than the file's count can say. Nothing is recorded, and the
    /// stream takes the next entry as if this one had not been pushed.
    pub fn push_sim_set(&mut self, tick: TickId, set: ConsoleSet) -> Result<(), StorageError> {
        let recorded = RecordedSimSet { tick, set };
        self.pushed.check_set(&recorded)?;
        spool::frame(RecordKind::SimSet, &mut self.pending, |buf| {
            input::encode_sim_set(&recorded, buf)
        })?;
        self.pushed.add_set(tick);
        Ok(())
    }

    /// Record the state hash at the end of `tick`.
    ///
    /// # Errors
    ///
    /// [`StorageError::ReplayInput`] for a hash for a tick not after the one
    /// before it, or more hashes than the file's count can say; nothing is
    /// recorded.
    pub fn push_state_hash(&mut self, tick: TickId, hash: u64) -> Result<(), StorageError> {
        let recorded = RecordedStateHash { tick, hash };
        self.pushed.check_hash(&recorded)?;
        spool::frame(RecordKind::StateHash, &mut self.pending, |buf| {
            input::encode_state_hash(&recorded, buf);
            Ok(())
        })?;
        self.pushed.add_hash(tick);
        Ok(())
    }

    /// Record what the host's module was handed of its peers at one tick.
    ///
    /// # Errors
    ///
    /// [`StorageError::ReplayInput`] for an entry for a tick not after the one
    /// before it, one peer's frames twice in a tick, more frames or longer
    /// ones than one host tick holds, or more entries than the track's count
    /// can say; nothing is recorded.
    pub fn push_peer_tick(&mut self, tick: &RecordedPeerTick) -> Result<(), StorageError> {
        self.pushed.check_peer_tick(tick)?;
        spool::frame(RecordKind::PeerTick, &mut self.pending, |buf| {
            input::encode_tick(tick, buf)
        })?;
        self.pushed.add_peer_tick(tick.tick);
        Ok(())
    }

    /// Write the records pushed since the last flush to the spool, in one
    /// write after the last whole record.
    ///
    /// # Errors
    ///
    /// [`StorageError::Io`] when the spool refused the write. The records it
    /// held are not recorded, and the stream takes the next entry as if they
    /// had not been pushed.
    pub fn flush(&mut self) -> Result<(), StorageError> {
        if self.pending.is_empty() {
            return Ok(());
        }
        let written = self.write_pending();
        if written.is_ok() {
            self.spool_len += self.pending.len() as u64;
            self.flushed = self.pushed.clone();
        } else {
            self.spool_dirty = true;
            self.pushed = self.flushed.clone();
        }
        self.pending.clear();
        written.map_err(StorageError::from)
    }

    fn write_pending(&mut self) -> std::io::Result<()> {
        if self.spool_dirty {
            self.spool.seek(SeekFrom::Start(self.spool_len))?;
            self.spool_dirty = false;
        }
        self.spool.write_all(&self.pending)
    }

    /// How many sets are recorded.
    pub fn sim_set_count(&self) -> usize {
        self.pushed.count(RecordKind::SimSet)
    }

    /// How many state hashes are recorded.
    pub fn state_hash_count(&self) -> usize {
        self.pushed.count(RecordKind::StateHash)
    }

    /// The tick of the first state hash recorded, and of the last.
    pub fn hashed_ticks(&self) -> Option<(TickId, TickId)> {
        self.pushed.hashed
    }

    /// How many ticks of the peer track are recorded.
    pub fn peer_tick_count(&self) -> usize {
        self.pushed.count(RecordKind::PeerTick)
    }

    /// [`flush`](Self::flush) what was pushed since the last flush, then
    /// write the file into `out` — the header, the sets, the hashes, then the
    /// peer track, each read back from the spool — and hand the spool back,
    /// for the caller to remove. `out` is written in order and flushed, never
    /// read or seeked.
    ///
    /// # Errors
    ///
    /// As [`flush`](Self::flush); [`StorageError::ReplaySpool`] when the
    /// spool does not read back as the records written to it, and
    /// [`StorageError::Io`] when it could not be read back or `out` refused a
    /// write — after which `out` holds a file cut short, which the reader
    /// refuses.
    pub fn finish(mut self, out: &mut impl Write) -> Result<S, StorageError> {
        self.flush()?;
        let scan = spool::scan(&mut self.spool, self.spool_len)?;
        if scan.stopped != SpoolEnd::Whole || scan.tally != self.flushed {
            return Err(SpoolError::Changed.into());
        }
        spool::write_file(&mut self.spool, &scan, out)?;
        Ok(self.spool)
    }
}

#[cfg(test)]
mod tests {
    use std::io::{self, Cursor};

    use super::*;
    use crate::replay::{
        FileTransport, InputSectionError, RecordedPeerFrames, RecordedRosterChange, ReplayWriter,
        RosterChangeKind,
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
                kind: RosterChangeKind::Joined(Some(crcbl_core::PlayerId::from_seed(peer))),
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
        let mut stream = ReplayStream::new(30, Cursor::new(Vec::new())).unwrap();
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
        let mut stream = ReplayStream::new(60, Cursor::new(Vec::new())).unwrap();
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
    /// after `fail_after` bytes writes up to them and fails. It counts the
    /// writes it is handed.
    #[derive(Debug)]
    struct FlakySpool {
        inner: Cursor<Vec<u8>>,
        fail_after: Option<u64>,
        writes: usize,
    }

    impl Write for FlakySpool {
        fn write(&mut self, buf: &[u8]) -> io::Result<usize> {
            self.writes += 1;
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

    /// **A flush is one write, and one that fails part-way loses what it
    /// held and nothing else**: its torn bytes are overwritten by the next
    /// flush and never reach the file, and the stream takes the next entries
    /// as if the lost ones had not been pushed.
    #[test]
    fn a_spool_write_failing_part_way_leaves_a_whole_file() {
        let first = peer_tick(1, 1, &[9; 40]);
        let mut first_bytes = Vec::new();
        spool::frame(RecordKind::PeerTick, &mut first_bytes, |buf| {
            input::encode_tick(&first, buf)
        })
        .unwrap();
        let spool = FlakySpool {
            inner: Cursor::new(Vec::new()),
            // Room for the header, the first record and a few bytes of the
            // second flush.
            fail_after: Some((spool::SPOOL_HEADER_BYTES + first_bytes.len()) as u64 + 5),
            writes: 0,
        };
        let mut stream = ReplayStream::new(60, spool).unwrap();
        stream.push_peer_tick(&first).unwrap();
        stream.flush().unwrap();
        stream.push_peer_tick(&peer_tick(2, 2, &[8; 40])).unwrap();
        stream.push_state_hash(TickId::from_raw(2), 5).unwrap();
        assert_eq!(stream.peer_tick_count(), 2, "pushed, not yet flushed");
        let err = stream.flush().unwrap_err();
        assert!(matches!(err, StorageError::Io(_)), "{err}");
        assert_eq!(
            stream.peer_tick_count(),
            1,
            "the failed tick is not counted"
        );
        assert_eq!(stream.hashed_ticks(), None, "nor the hash it held");
        // The failed tick may be pushed again, or the next one; a flush with
        // nothing pushed writes nothing.
        stream.push_peer_tick(&peer_tick(2, 2, &[7; 3])).unwrap();
        stream.push_state_hash(TickId::from_raw(2), 6).unwrap();
        stream.flush().unwrap();
        stream.flush().unwrap();

        let mut out = Vec::new();
        let spool = stream.finish(&mut out).unwrap();
        assert_eq!(spool.writes, 4, "the header, then one write a flush");
        let file = FileTransport::decode(&out).unwrap();
        let ticks: Vec<u64> = file.peer_ticks().iter().map(|t| t.tick.get()).collect();
        assert_eq!(ticks, [1, 2]);
        assert_eq!(file.peer_ticks()[1].peers[0].frames[0].1, [7; 3]);
        assert_eq!(file.state_hashes().len(), 1);
        assert_eq!(file.state_hashes()[0].hash, 6);
    }

    /// A spool whose reads come back with one byte flipped: a disk that does
    /// not hand back what was written to it.
    #[derive(Debug)]
    struct DamagedSpool {
        inner: Cursor<Vec<u8>>,
        flip_at: u64,
    }

    impl Write for DamagedSpool {
        fn write(&mut self, buf: &[u8]) -> io::Result<usize> {
            self.inner.write(buf)
        }

        fn flush(&mut self) -> io::Result<()> {
            self.inner.flush()
        }
    }

    impl Read for DamagedSpool {
        fn read(&mut self, buf: &mut [u8]) -> io::Result<usize> {
            let start = self.inner.position();
            let read = self.inner.read(buf)?;
            if let Some(byte) = self
                .flip_at
                .checked_sub(start)
                .and_then(|at| usize::try_from(at).ok())
                .and_then(|at| buf[..read].get_mut(at))
            {
                *byte ^= 1;
            }
            Ok(read)
        }
    }

    impl Seek for DamagedSpool {
        fn seek(&mut self, pos: SeekFrom) -> io::Result<u64> {
            self.inner.seek(pos)
        }
    }

    /// **A spool that does not read back as it was written is refused at
    /// the finish**, rather than written into a file short of what was
    /// recorded.
    #[test]
    fn a_spool_reading_back_otherwise_is_refused_at_the_finish() {
        let spool = DamagedSpool {
            inner: Cursor::new(Vec::new()),
            // Inside the second record, which starts after the header and
            // the first.
            flip_at: 130,
        };
        let mut stream = ReplayStream::new(60, spool).unwrap();
        stream.push_peer_tick(&peer_tick(1, 1, &[1; 40])).unwrap();
        stream.push_peer_tick(&peer_tick(2, 1, &[2; 40])).unwrap();
        match stream.finish(&mut Vec::new()) {
            Err(StorageError::ReplaySpool(SpoolError::Changed)) => {}
            other => panic!("not refused as changed: {other:?}"),
        }
    }

    /// **The empty stream finishes into a file that reads**: a header counting
    /// no ticks, and an empty section.
    #[test]
    fn an_empty_stream_finishes_into_an_empty_file() {
        let stream = ReplayStream::new(60, Cursor::new(Vec::new())).unwrap();
        assert_eq!(stream.hashed_ticks(), None);
        let bytes = finished(stream);
        assert_eq!(bytes, ReplayWriter::new(60).encode().unwrap());
        let file = FileTransport::decode(&bytes).unwrap();
        assert!(file.is_empty());
        assert!(file.peer_ticks().is_empty());
    }
}
