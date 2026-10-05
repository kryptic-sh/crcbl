//! The spool a [`ReplayStream`](super::ReplayStream) writes while a session
//! runs, and recovering a `.crpl` file from one a session left unfinished.
//!
//! # Why the spool holds everything
//!
//! The file puts each list's count before its entries — the sets, then the
//! hashes, then the peer track — so a writer cannot put any of it in the file
//! in order until the session ends. Until then the spool is all that is on
//! disk, and if the process is killed it is all there is. So **every record a
//! recording makes goes to the spool as it happens**: each set, each hash and
//! each tick of the peer track, appended in the order they came, each framed
//! so a reader tells a whole record from one the process died writing. A
//! record's payload is its entry exactly as the file's input section encodes
//! it, so writing the file is copying payloads, kind by kind, after the counts.
//!
//! # Format
//!
//! ```text
//! [0..8)    magic          b"CRBLSPOL"
//! [8..10)   spool_version  u16 little-endian
//! [10..14)  tick_rate      u32 little-endian
//! [14..18)  header_crc     u32 little-endian, crc32 of [0..14)
//! then      SpoolRecord, until the end of the spool
//! ```
//!
//! Each `SpoolRecord`:
//!
//! ```text
//! kind     u8: 1 set, 2 state hash, 3 peer tick
//! len      u32 little-endian
//! payload  `len` bytes: a SimSetEntry, StateHashEntry or PeerTickEntry, as
//!          the input section lays it out (`replay`'s module docs)
//! crc      u32 little-endian, crc32 of kind, len and payload
//! ```
//!
//! The CRC is [`crc32`](crate::crc32): corruption detection, never security.
//!
//! Spool version 2 holds peer ticks in format version 4's layout, whose joins
//! may name their players; version 1 held them in version 3's. A version 3
//! track is a version 4 track whose joins name nobody (`replay`'s `migrate`
//! module), so a version 1 spool's records are version 2 records unchanged
//! and the one reader reads both: one still recovers, every join naming
//! nobody, and only its header's version says it is older. The version was bumped all the same so
//! a build from before refuses a spool whose joins it cannot read by its
//! version, rather than stopping at its first join.
//!
//! # Reading one back
//!
//! [`recover_spool`] trusts nothing in a spool. It keeps every record from the
//! start that is whole — inside the spool, its CRC matching, its payload
//! decoding to exactly its length, and holding the reader's rules after the
//! records before it — and stops at the first that is not, dropping it and
//! everything after it: a record cut short is what a process killed mid-write
//! leaves, and one that fails its CRC or a rule is damage nothing after it can
//! be trusted past. What it dropped and why is in its [`SpoolRecovery`]. A
//! length is bounded by the bytes that follow it before anything is read for
//! it, so a hostile length reserves nothing.

use std::io::{self, BufReader, Read, Seek, SeekFrom, Write};

use crcbl_core::TickId;

use super::input::{
    self, InputSectionError, Reader, RecordedPeerTick, RecordedSimSet, RecordedStateHash,
};
use crate::StorageError;
use crate::crc32::{crc32, crc32_continue};

/// Magic bytes identifying a replay spool.
pub const SPOOL_MAGIC: &[u8; 8] = b"CRBLSPOL";

/// The spool format version this build writes.
pub const SPOOL_FORMAT_VERSION: u16 = 2;

/// The oldest spool format version this build reads. Its records are laid
/// out as the current version's ([module docs](self)).
const OLDEST_READABLE_SPOOL_VERSION: u16 = 1;

/// Where the header's CRC starts: it covers everything before it.
const HEADER_CRC_AT: usize = 8 + 2 + 4;

/// The header's size.
pub(super) const SPOOL_HEADER_BYTES: usize = HEADER_CRC_AT + 4;

/// A record's kind and length, before its payload.
const RECORD_HEAD_BYTES: usize = 1 + 4;

/// A record's CRC, after its payload.
const RECORD_CRC_BYTES: usize = 4;

/// Why a spool was refused before any record was read — its header — or why
/// one could not be read the same way twice.
#[derive(Clone, Debug, PartialEq, Eq, thiserror::Error)]
#[non_exhaustive]
pub enum SpoolError {
    /// Shorter than the header.
    #[error("a replay spool of {0} bytes is shorter than its header")]
    TooShort(u64),
    /// Not a spool.
    #[error("not a replay spool: its magic is wrong")]
    Magic,
    /// A spool version this build does not read.
    #[error(
        "unsupported replay spool version {0} (this build reads versions \
         {OLDEST_READABLE_SPOOL_VERSION} to {SPOOL_FORMAT_VERSION})"
    )]
    Version(u16),
    /// The header's CRC does not match it.
    #[error("the replay spool's header is damaged: its CRC does not match")]
    HeaderChecksum,
    /// The spool did not read back as the records it held when it was
    /// scanned, or as the ones written to it: something else wrote to it.
    #[error("the replay spool changed while it was read")]
    Changed,
}

/// Where a spool's records stopped being trustworthy, and why.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum SpoolEnd {
    /// Every record was whole: nothing was dropped.
    Whole,
    /// The last record is cut short — what a process killed while writing it
    /// leaves — or its length was damaged past the spool's end.
    CutShort,
    /// A record's CRC does not match it.
    BadChecksum,
    /// A record of a kind no build writes.
    UnknownRecord(u8),
    /// A record whole on disk whose entry does not decode to exactly its
    /// length, or breaks a rule of the input section after the records before
    /// it.
    Refused(InputSectionError),
}

impl std::fmt::Display for SpoolEnd {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Whole => f.write_str("every record was whole"),
            Self::CutShort => f.write_str("the last record was cut short"),
            Self::BadChecksum => f.write_str("a record failed its CRC"),
            Self::UnknownRecord(kind) => {
                write!(f, "a record has kind {kind}, which no build writes")
            }
            Self::Refused(error) => write!(f, "a record was refused: {error}"),
        }
    }
}

/// What [`recover_spool`] wrote, and what it dropped.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SpoolRecovery {
    /// The tick rate the spool's header holds, which the file's does too.
    pub tick_rate: u32,
    /// How many sets the file holds.
    pub sim_sets: usize,
    /// How many state hashes the file holds.
    pub state_hashes: usize,
    /// The tick of the first state hash it holds and of the last: the ticks a
    /// re-simulation of it checks.
    pub hashed_ticks: Option<(TickId, TickId)>,
    /// How many ticks of the peer track the file holds.
    pub peer_ticks: usize,
    /// The bytes of the spool kept: the header and every whole record.
    pub kept_bytes: u64,
    /// The bytes of the spool dropped after them.
    pub dropped_bytes: u64,
    /// Why it stopped where it did.
    pub end: SpoolEnd,
}

/// Write a `.crpl` file into `out` from a spool a [`ReplayStream`] wrote —
/// one whose recording never finished — keeping every whole record and
/// dropping the first that is not and everything after it ([module
/// docs](self)). The file is at
/// [`REPLAY_FORMAT_VERSION`](super::REPLAY_FORMAT_VERSION), whatever the
/// spool's version, with no output entries, as the stream's own would have
/// been for the records it holds, and reads back through
/// [`FileTransport`](super::FileTransport). `out` is written in order and
/// flushed, never read or seeked; `spool` is read from its start once to
/// find the records to keep and then once for each list, holding no more
/// than one record in memory at once.
///
/// # Errors
///
/// [`StorageError::ReplaySpool`] for a spool whose header is missing or
/// damaged, or that changed between its reads, and [`StorageError::Io`] when
/// it could not be read or `out` refused a write — after which `out` holds a
/// file cut short, which the reader refuses.
///
/// [`ReplayStream`]: super::ReplayStream
pub fn recover_spool<S: Read + Seek>(
    mut spool: S,
    out: &mut impl Write,
) -> Result<SpoolRecovery, StorageError> {
    let len = spool.seek(SeekFrom::End(0))?;
    let scan = scan(&mut spool, len)?;
    write_file(&mut spool, &scan, out)?;
    Ok(SpoolRecovery {
        tick_rate: scan.tick_rate,
        sim_sets: scan.tally.count(RecordKind::SimSet),
        state_hashes: scan.tally.count(RecordKind::StateHash),
        hashed_ticks: scan.tally.hashed,
        peer_ticks: scan.tally.count(RecordKind::PeerTick),
        kept_bytes: scan.end,
        dropped_bytes: len - scan.end,
        end: scan.stopped,
    })
}

/// What a spool record holds.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum RecordKind {
    SimSet,
    StateHash,
    PeerTick,
}

impl RecordKind {
    /// Every kind, in the order the file lays its lists out.
    const ALL: [Self; 3] = [Self::SimSet, Self::StateHash, Self::PeerTick];

    /// Its byte in the spool. Written out rather than derived from the
    /// declaration order, so reordering the variants cannot change what an
    /// older spool means.
    const fn code(self) -> u8 {
        match self {
            Self::SimSet => 1,
            Self::StateHash => 2,
            Self::PeerTick => 3,
        }
    }

    fn from_code(code: u8) -> Option<Self> {
        Self::ALL.into_iter().find(|kind| kind.code() == code)
    }

    /// What the file's count of this kind counts, as its errors name it.
    const fn what(self) -> &'static str {
        match self {
            Self::SimSet => "sets",
            Self::StateHash => "state hashes",
            Self::PeerTick => "peer ticks",
        }
    }
}

/// The spool's header, for a session at `tick_rate` Hz.
pub(super) fn encode_header(tick_rate: u32) -> [u8; SPOOL_HEADER_BYTES] {
    let mut header = [0; SPOOL_HEADER_BYTES];
    header[..8].copy_from_slice(SPOOL_MAGIC);
    header[8..10].copy_from_slice(&SPOOL_FORMAT_VERSION.to_le_bytes());
    header[10..HEADER_CRC_AT].copy_from_slice(&tick_rate.to_le_bytes());
    let crc = crc32(&header[..HEADER_CRC_AT]);
    header[HEADER_CRC_AT..].copy_from_slice(&crc.to_le_bytes());
    header
}

/// The tick rate a spool's header holds.
fn decode_header(header: &[u8; SPOOL_HEADER_BYTES]) -> Result<u32, SpoolError> {
    if &header[..8] != SPOOL_MAGIC {
        return Err(SpoolError::Magic);
    }
    let version = u16::from_le_bytes([header[8], header[9]]);
    if !(OLDEST_READABLE_SPOOL_VERSION..=SPOOL_FORMAT_VERSION).contains(&version) {
        return Err(SpoolError::Version(version));
    }
    let crc = u32::from_le_bytes([header[14], header[15], header[16], header[17]]);
    if crc != crc32(&header[..HEADER_CRC_AT]) {
        return Err(SpoolError::HeaderChecksum);
    }
    Ok(u32::from_le_bytes([
        header[10], header[11], header[12], header[13],
    ]))
}

/// Append one record of `kind` to `buf`: its head, the payload `encode`
/// appends, and its CRC. On an error `buf` is left as it was.
pub(super) fn frame(
    kind: RecordKind,
    buf: &mut Vec<u8>,
    encode: impl FnOnce(&mut Vec<u8>) -> Result<(), InputSectionError>,
) -> Result<(), InputSectionError> {
    let start = buf.len();
    buf.push(kind.code());
    buf.extend_from_slice(&[0; RECORD_HEAD_BYTES - 1]);
    let encoded = encode(buf).and_then(|()| {
        let len = buf.len() - start - RECORD_HEAD_BYTES;
        u32::try_from(len).map_err(|_| InputSectionError::TooMany {
            what: "bytes in one spool record",
            count: len,
        })
    });
    let len = match encoded {
        Ok(len) => len,
        Err(error) => {
            buf.truncate(start);
            return Err(error);
        }
    };
    buf[start + 1..start + RECORD_HEAD_BYTES].copy_from_slice(&len.to_le_bytes());
    let crc = crc32(&buf[start..]);
    buf.extend_from_slice(&crc.to_le_bytes());
    Ok(())
}

/// How many records of each kind came before, and the ticks the next of each
/// must come after: the rules a spool's records hold, checked as a stream
/// pushes them and again as a spool is read back.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub(super) struct Tally {
    counts: [u32; 3],
    last_set: Option<TickId>,
    /// The first hash's tick and the last's.
    pub(super) hashed: Option<(TickId, TickId)>,
    last_peer_tick: Option<TickId>,
}

impl Tally {
    /// How many records of `kind` there are.
    pub(super) fn count(&self, kind: RecordKind) -> usize {
        usize::try_from(self.counts[kind as usize]).unwrap_or(usize::MAX)
    }

    /// Refuses one more record of `kind` than the file's `u32` count can say.
    fn check_room(&self, kind: RecordKind) -> Result<(), InputSectionError> {
        let count = self.counts[kind as usize];
        if count == u32::MAX {
            return Err(InputSectionError::TooMany {
                what: kind.what(),
                count: usize::try_from(count).map_or(usize::MAX, |n| n.saturating_add(1)),
            });
        }
        Ok(())
    }

    /// The rules `set` holds after the records before it.
    pub(super) fn check_set(&self, set: &RecordedSimSet) -> Result<(), InputSectionError> {
        self.check_room(RecordKind::SimSet)?;
        set.check_after(self.last_set)
    }

    /// The rules `hash` holds after the records before it.
    pub(super) fn check_hash(&self, hash: &RecordedStateHash) -> Result<(), InputSectionError> {
        self.check_room(RecordKind::StateHash)?;
        hash.check_after(self.hashed.map(|(_, last)| last))
    }

    /// The rules `tick` holds after the records before it.
    pub(super) fn check_peer_tick(&self, tick: &RecordedPeerTick) -> Result<(), InputSectionError> {
        self.check_room(RecordKind::PeerTick)?;
        input::check_tick(self.last_peer_tick, tick)
    }

    /// Counts a set for `tick`, which [`check_set`](Self::check_set) passed.
    pub(super) fn add_set(&mut self, tick: TickId) {
        self.counts[RecordKind::SimSet as usize] += 1;
        self.last_set = Some(tick);
    }

    /// Counts a hash for `tick`, which [`check_hash`](Self::check_hash)
    /// passed.
    pub(super) fn add_hash(&mut self, tick: TickId) {
        self.counts[RecordKind::StateHash as usize] += 1;
        self.hashed = Some((self.hashed.map_or(tick, |(first, _)| first), tick));
    }

    /// Counts a peer tick for `tick`, which
    /// [`check_peer_tick`](Self::check_peer_tick) passed.
    pub(super) fn add_peer_tick(&mut self, tick: TickId) {
        self.counts[RecordKind::PeerTick as usize] += 1;
        self.last_peer_tick = Some(tick);
    }

    /// Decodes a record of `kind` from its payload, checks it and counts it.
    fn take(&mut self, kind: RecordKind, payload: &[u8]) -> Result<(), InputSectionError> {
        let mut reader = Reader { bytes: payload };
        match kind {
            RecordKind::SimSet => {
                let set = input::decode_sim_set(&mut reader)?;
                whole(&reader)?;
                self.check_set(&set)?;
                self.add_set(set.tick);
            }
            RecordKind::StateHash => {
                let hash = input::decode_state_hash(&mut reader)?;
                whole(&reader)?;
                self.check_hash(&hash)?;
                self.add_hash(hash.tick);
            }
            RecordKind::PeerTick => {
                let tick = input::decode_tick(&mut reader)?;
                whole(&reader)?;
                self.check_peer_tick(&tick)?;
                self.add_peer_tick(tick.tick);
            }
        }
        Ok(())
    }
}

/// Refuses a payload with bytes its entry did not read.
fn whole(reader: &Reader<'_>) -> Result<(), InputSectionError> {
    if reader.bytes.is_empty() {
        Ok(())
    } else {
        Err(InputSectionError::TrailingBytes(reader.bytes.len()))
    }
}

/// The records of a spool to keep, found by [`scan`].
#[derive(Debug)]
pub(super) struct Scan {
    tick_rate: u32,
    pub(super) tally: Tally,
    /// The offset of the first byte not kept: the end of the last whole
    /// record.
    end: u64,
    pub(super) stopped: SpoolEnd,
}

/// Reads the spool's first `len` bytes and finds the whole records among
/// them ([module docs](self)).
pub(super) fn scan<S: Read + Seek>(spool: &mut S, len: u64) -> Result<Scan, StorageError> {
    let (tick_rate, mut records) = Records::open(spool, len)?;
    let mut tally = Tally::default();
    loop {
        let at = records.pos;
        let stopped = match records.head()? {
            Head::End(end) => end,
            Head::Record { kind, len, head } => match records.payload(head, len)? {
                None => SpoolEnd::BadChecksum,
                Some(payload) => match tally.take(kind, payload) {
                    Ok(()) => continue,
                    Err(error) => SpoolEnd::Refused(error),
                },
            },
        };
        return Ok(Scan {
            tick_rate,
            tally,
            end: at,
            stopped,
        });
    }
}

/// Writes the file from the records `scan` kept: the header, then each list's
/// count and the payloads of its records, in the order the spool holds them.
pub(super) fn write_file<S: Read + Seek>(
    spool: &mut S,
    scan: &Scan,
    out: &mut impl Write,
) -> Result<(), StorageError> {
    let mut head = Vec::new();
    super::encode_header(&mut head, 0, scan.tick_rate, 0);
    out.write_all(&head)?;
    let (tick_rate, mut records) = Records::open(spool, scan.end)?;
    if tick_rate != scan.tick_rate {
        return Err(SpoolError::Changed.into());
    }
    for kind in RecordKind::ALL {
        records.rewind()?;
        let expected = scan.tally.counts[kind as usize];
        out.write_all(&expected.to_le_bytes())?;
        let mut copied = 0u32;
        loop {
            match records.head()? {
                Head::End(SpoolEnd::Whole) => break,
                Head::End(_) => return Err(SpoolError::Changed.into()),
                Head::Record {
                    kind: found, len, ..
                } if found != kind => records.skip(len)?,
                Head::Record { len, head, .. } => {
                    let payload = records.payload(head, len)?.ok_or(SpoolError::Changed)?;
                    out.write_all(payload)?;
                    copied = copied.checked_add(1).ok_or(SpoolError::Changed)?;
                }
            }
        }
        if copied != expected {
            return Err(SpoolError::Changed.into());
        }
    }
    out.flush()?;
    Ok(())
}

/// The next record's head, or where the records end.
enum Head {
    Record {
        kind: RecordKind,
        len: u32,
        head: [u8; RECORD_HEAD_BYTES],
    },
    End(SpoolEnd),
}

/// A spool's records, read in order up to `end`.
struct Records<'s, S> {
    reader: BufReader<&'s mut S>,
    /// The offset of the next record.
    pos: u64,
    /// The offset reading stops at.
    end: u64,
    /// The last payload read, with its CRC; kept so its buffer is reused.
    payload: Vec<u8>,
}

impl<'s, S: Read + Seek> Records<'s, S> {
    /// Reads and checks the header of a spool whose first `end` bytes are
    /// read, and answers its tick rate and its records.
    fn open(spool: &'s mut S, end: u64) -> Result<(u32, Self), StorageError> {
        if end < SPOOL_HEADER_BYTES as u64 {
            return Err(SpoolError::TooShort(end).into());
        }
        spool.seek(SeekFrom::Start(0))?;
        let mut reader = BufReader::new(spool);
        let mut header = [0; SPOOL_HEADER_BYTES];
        reader.read_exact(&mut header)?;
        let tick_rate = decode_header(&header)?;
        Ok((
            tick_rate,
            Self {
                reader,
                pos: SPOOL_HEADER_BYTES as u64,
                end,
                payload: Vec::new(),
            },
        ))
    }

    /// Back to the first record.
    fn rewind(&mut self) -> io::Result<()> {
        self.reader
            .seek(SeekFrom::Start(SPOOL_HEADER_BYTES as u64))?;
        self.pos = SPOOL_HEADER_BYTES as u64;
        Ok(())
    }

    /// Reads the next record's head, which [`payload`](Self::payload) or
    /// [`skip`](Self::skip) must follow; a record whose length runs past the
    /// end is where the records end, cut short.
    fn head(&mut self) -> io::Result<Head> {
        let left = self.end - self.pos;
        if left == 0 {
            return Ok(Head::End(SpoolEnd::Whole));
        }
        if left < (RECORD_HEAD_BYTES + RECORD_CRC_BYTES) as u64 {
            return Ok(Head::End(SpoolEnd::CutShort));
        }
        let mut head = [0; RECORD_HEAD_BYTES];
        self.reader.read_exact(&mut head)?;
        let Some(kind) = RecordKind::from_code(head[0]) else {
            return Ok(Head::End(SpoolEnd::UnknownRecord(head[0])));
        };
        let len = u32::from_le_bytes([head[1], head[2], head[3], head[4]]);
        if record_bytes(len) > left {
            return Ok(Head::End(SpoolEnd::CutShort));
        }
        Ok(Head::Record { kind, len, head })
    }

    /// Reads the payload of the record whose head was `head`, answering it if
    /// its CRC matches and `None` if not.
    fn payload(&mut self, head: [u8; RECORD_HEAD_BYTES], len: u32) -> io::Result<Option<&[u8]>> {
        // `head` bounded the length by the bytes left, so this reserves no
        // more than the spool holds.
        let bytes = usize::try_from(len).map_err(io::Error::other)?;
        self.payload.resize(bytes + RECORD_CRC_BYTES, 0);
        self.reader.read_exact(&mut self.payload)?;
        let (payload, crc) = self.payload.split_at(bytes);
        let stored = u32::from_le_bytes([crc[0], crc[1], crc[2], crc[3]]);
        if stored != crc32_continue(crc32(&head), payload) {
            return Ok(None);
        }
        self.pos += record_bytes(len);
        Ok(Some(payload))
    }

    /// Steps over the payload and CRC of the record whose head was read: one
    /// [`scan`] already checked.
    fn skip(&mut self, len: u32) -> io::Result<()> {
        let rest = i64::from(len) + RECORD_CRC_BYTES as i64;
        self.reader.seek_relative(rest)?;
        self.pos += record_bytes(len);
        Ok(())
    }
}

/// A whole record's size, for a payload of `len` bytes.
fn record_bytes(len: u32) -> u64 {
    (RECORD_HEAD_BYTES + RECORD_CRC_BYTES) as u64 + u64::from(len)
}

#[cfg(test)]
mod tests;
