//! State recording and playback — replays and `FileTransport`.
//!
//! # Format
//!
//! `.crpl` files are binary recordings of the server's output stream:
//!
//! ```text
//! [0..8)    magic:          b"CRBLREPL"
//! [8..10)   format_version: u16 little-endian
//! [10..18)  tick_count:     u64 little-endian
//! [18..22)  tick_rate:      u32 little-endian
//! [22..30)  start_tick:     u64 little-endian
//! [30..)    entries:        TickEntry[tick_count]
//! then      input section   (format version 2 on)
//! ```
//!
//! Each `TickEntry`:
//! ```text
//! [0..8)   tick_id:     u64 little-endian
//! [8..12)  msg_len:     u32 little-endian
//! [12..)   msg_data:    `msg_len` bytes (an encoded `ServerToClient` message)
//! ```
//!
//! For MVP, each entry stores the full server message bytes. A future format
//! bump may add delta compression against the previous tick's entry.
//!
//! The input section, which ends the file, carries what a re-simulation needs
//! and the output does not. The entries are what the server sent, and a viewer
//! plays them back without simulating anything; a re-simulation runs a fresh
//! host through the same ticks instead, so it needs the tick inputs the output
//! was computed from and something to compare its own state against:
//!
//! ```text
//! sim_set_count   u32 little-endian
//! SimSetEntry[sim_set_count]:
//!   tick          u64 little-endian
//!   name_len      u16 little-endian, then `name_len` bytes of UTF-8
//!   value_len     u16 little-endian, then `value_len` bytes of UTF-8
//! hash_count      u32 little-endian
//! StateHashEntry[hash_count]:
//!   tick          u64 little-endian
//!   hash          u64 little-endian
//! peer_tick_count u32 little-endian (format version 3 on)
//! PeerTickEntry[peer_tick_count]:
//!   tick          u64 little-endian
//!   change_count  u32 little-endian
//!   RosterEntry[change_count]:
//!     kind        u8: 1 joined, 2 lost, 3 resumed, 4 left, 5 ended
//!     peer        u64 little-endian
//!   peer_count    u32 little-endian
//!   PeerFramesEntry[peer_count]:
//!     peer        u64 little-endian
//!     dropped     u32 little-endian
//!     frame_count u32 little-endian
//!     FrameEntry[frame_count]:
//!       tick      u64 little-endian
//!       len       u32 little-endian, then `len` bytes
//! ```
//!
//! A set ([`RecordedSimSet`]) is a `Flags::SIM` console set as the host applied
//! it — the variable's name and the value as the text the console prints, which
//! `crcbl_console::Registry::sim_set` parses back to the same value — at the
//! tick whose start it was applied at, each text within a console set's limits.
//! Sets are in the order the host applied them, so their ticks never decrease.
//! A state hash ([`RecordedStateHash`]) is the recorder's state hash at the end
//! of a tick, at most one a tick and in tick order; a recorder may hash every
//! tick or only some.
//!
//! The peer track ([`RecordedPeerTick`]) is what the recorded host's module
//! was handed of its peers, one entry for each tick that had any: the changes
//! to the roster before the module ran — which peer joined, was lost, resumed,
//! left or was ended by the game, by the number the host gave it, in the order
//! the host applied them — and the input frames of every peer that had any,
//! each with the tick its client stamped on it, as the module read them after
//! the host's own checks and its per-tick cap, with the count that cap
//! refused. A peer in the roster with no entry for a tick was handed nothing.
//! Entries are one a tick in tick order, a peer has at most one in a tick, and
//! its frames are no more than
//! [`MAX_CLIENT_INPUTS_PER_TICK`](crcbl_net::MAX_CLIENT_INPUTS_PER_TICK), each
//! no longer than
//! [`MAX_FIELD_BYTES`](crcbl_net::codec::MAX_FIELD_BYTES) — what one host tick
//! holds. Whether the roster's changes make sense one after another is the
//! re-simulating host's to check, since only it knows what a roster is.
//!
//! Both directions refuse a section that breaks a rule, by name
//! ([`InputSectionError`]).
//!
//! A version 1 file has no section and reads as one with no sets and no
//! hashes, so it plays back as it always did; a version 2 file's section ends
//! after its hashes and reads with no peer track. [`ReplayWriter`] writes
//! version 3, holding everything until it writes; so does [`ReplayStream`],
//! which writes a file with no entries while a session runs, spooling its
//! peer track rather than holding it.
//!
//! [`FileTransport`] reads a `.crpl` file and emits entries as if they were
//! arriving from a live network transport.

use std::path::Path;

use crcbl_core::TickId;
use crcbl_net::ConsoleSet;
use crcbl_net::transport::{Message, MessageKind, Transport, TransportError};

use crate::{StorageError, StorageSource};

mod input;
mod stream;

use input::InputSection;
pub use input::{
    InputSectionError, RecordedPeerFrames, RecordedPeerTick, RecordedRosterChange, RecordedSimSet,
    RecordedStateHash, RosterChangeKind,
};
pub use stream::ReplayStream;

// ── Constants ──────────────────────────────────────────────────────────────

/// Magic bytes identifying a `.crpl` replay file.
pub const REPLAY_MAGIC: &[u8; 8] = b"CRBLREPL";

/// Current replay format version: the one [`ReplayWriter`] writes.
///
/// Version 2 added the input section and version 3 its peer track; versions 1
/// and 2, without them, still read.
pub const REPLAY_FORMAT_VERSION: u16 = 3;

/// The oldest format version [`FileTransport`] reads.
const OLDEST_READABLE_VERSION: u16 = 1;

/// The first format version with an input section.
const INPUT_SECTION_VERSION: u16 = 2;

/// The first format version whose input section has a peer track.
const PEER_TRACK_VERSION: u16 = 3;

/// The header's size, which is the smallest a file of any version can be: a
/// version 1 file with no entries. A later version adds at least its input
/// section's counts.
pub const REPLAY_MIN_SIZE: usize = 30;

/// Smallest possible `TickEntry`: `tick_id` + `msg_len`, with no payload.
const MIN_ENTRY_SIZE: usize = 12;

// ── ReplayWriter ───────────────────────────────────────────────────────────

/// Records server output messages into a `.crpl` replay buffer.
///
/// Call [`push_tick`](Self::push_tick) for each tick's server message — and,
/// for a file a re-simulation can check, [`push_sim_set`](Self::push_sim_set)
/// for each applied `Flags::SIM` set and
/// [`push_state_hash`](Self::push_state_hash) for the ticks it hashed, and
/// [`push_peer_tick`](Self::push_peer_tick) for what its module was handed of
/// its peers — then
/// [`write`](Self::write) to persist the replay through a [`StorageSource`].
///
/// # Example
///
/// ```ignore
/// let mut writer = ReplayWriter::new(60);
/// for msg in server_messages {
///     writer.push_tick(tick_id, &msg);
/// }
/// writer.write(&storage, Path::new("game.crpl"))?;
/// ```
#[derive(Debug)]
pub struct ReplayWriter {
    tick_rate: u32,
    entries: Vec<(TickId, Vec<u8>)>,
    input: InputSection,
}

impl ReplayWriter {
    /// Create a new replay writer.
    ///
    /// `tick_rate` is the server's tick rate in Hz, stored in the file header
    /// so the player can replay at the correct speed.
    pub fn new(tick_rate: u32) -> Self {
        Self {
            tick_rate,
            entries: Vec::new(),
            input: InputSection::default(),
        }
    }

    /// Record one tick's server output message.
    ///
    /// `msg` should be an encoded `ServerToClient` message as produced by
    /// the server's snapshot emission.
    pub fn push_tick(&mut self, tick: TickId, msg: &[u8]) {
        self.entries.push((tick, msg.to_vec()));
    }

    /// The number of ticks recorded.
    pub fn tick_count(&self) -> usize {
        self.entries.len()
    }

    /// Record a `Flags::SIM` console set the host applied at the start of
    /// `tick`, in the order it applied them — `crcbl_server::Host::sim_record`
    /// as text, the value as the console prints it.
    ///
    /// [`write`](Self::write) refuses a set for an earlier tick than the one
    /// before it, or a name or value longer than a console set carries.
    pub fn push_sim_set(&mut self, tick: TickId, set: ConsoleSet) {
        self.input.sim_sets.push(RecordedSimSet { tick, set });
    }

    /// Record the state hash at the end of `tick`, for a re-simulation to
    /// compare its own against.
    ///
    /// [`write`](Self::write) refuses a hash for a tick not after the one
    /// before it.
    pub fn push_state_hash(&mut self, tick: TickId, hash: u64) {
        self.input
            .state_hashes
            .push(RecordedStateHash { tick, hash });
    }

    /// Record what the host's module was handed of its peers at one tick —
    /// an entry of `crcbl_server::Host::peer_input_record`, each peer as its
    /// number.
    ///
    /// [`write`](Self::write) refuses an entry for a tick not after the one
    /// before it, one peer's frames twice in a tick, or more frames, or longer
    /// ones, than one host tick holds.
    pub fn push_peer_tick(&mut self, tick: RecordedPeerTick) {
        self.input.peer_ticks.push(tick);
    }

    /// Encode the replay into a byte buffer.
    ///
    /// Fails rather than truncating when an entry's data length does not fit
    /// the `u32` the format reserves for it, and with an
    /// [`InputSectionError`] for an input section the reader would refuse.
    fn encode(&self) -> Result<Vec<u8>, StorageError> {
        let total_entries = self.entries.len() as u64;
        let mut buf = Vec::with_capacity(
            REPLAY_MIN_SIZE + self.entries.iter().map(|(_, d)| d.len()).sum::<usize>(),
        );

        let start_tick = self.entries.first().map(|(t, _)| t.get()).unwrap_or(0);
        encode_header(&mut buf, total_entries, self.tick_rate, start_tick);

        for (tick, data) in &self.entries {
            buf.extend_from_slice(&tick.get().to_le_bytes());
            let len = u32::try_from(data.len()).map_err(|_| {
                StorageError::Other(format!(
                    "entry at tick {} is {} bytes, more than the format's u32 length",
                    tick.get(),
                    data.len()
                ))
            })?;
            buf.extend_from_slice(&len.to_le_bytes());
            buf.extend_from_slice(data);
        }

        self.input.encode(&mut buf)?;
        Ok(buf)
    }

    /// Write the replay file atomically through `storage` at `path`.
    pub fn write(&self, storage: &dyn StorageSource, path: &Path) -> Result<(), StorageError> {
        let data = self.encode()?;
        storage.write(path, &data)
    }
}

/// Append the header a version [`REPLAY_FORMAT_VERSION`] file opens with.
fn encode_header(buf: &mut Vec<u8>, tick_count: u64, tick_rate: u32, start_tick: u64) {
    buf.extend_from_slice(REPLAY_MAGIC);
    buf.extend_from_slice(&REPLAY_FORMAT_VERSION.to_le_bytes());
    buf.extend_from_slice(&tick_count.to_le_bytes());
    buf.extend_from_slice(&tick_rate.to_le_bytes());
    buf.extend_from_slice(&start_tick.to_le_bytes());
}

// ── FileTransport ──────────────────────────────────────────────────────────

/// A [`Transport`] that reads a `.crpl` replay file and emits its entries as
/// messages.
///
/// Replay playback works exactly like a client connected to a server: the
/// transport provides the same sequence of messages the client would have
/// received live.
///
/// # Example
///
/// ```ignore
/// let mut transport = FileTransport::open(&storage, Path::new("game.crpl"))?;
/// while transport.is_connected() {
///     if let Ok(Some(msg)) = transport.recv() {
///         // process as a live server message
///     }
/// }
/// ```
#[derive(Debug)]
pub struct FileTransport {
    entries: Vec<(TickId, Vec<u8>)>,
    cursor: usize,
    connected: bool,
    tick_rate: u32,
    format_version: u16,
    input: InputSection,
}

impl FileTransport {
    /// Open a `.crpl` replay file from `path` in `storage`.
    ///
    /// See [`decode`](Self::decode) for what it refuses.
    pub fn open(storage: &dyn StorageSource, path: &Path) -> Result<Self, StorageError> {
        Self::decode(&storage.read(path)?)
    }

    /// Read a `.crpl` replay from its bytes.
    ///
    /// Validates the magic and format version. Returns an error if the file is
    /// too short, has an invalid magic, contains an unsupported format
    /// version, or — from version 2 — has an input section that breaks one of
    /// its rules ([`StorageError::ReplayInput`]), its peer track's from
    /// version 3.
    pub fn decode(bytes: &[u8]) -> Result<Self, StorageError> {
        if bytes.len() < REPLAY_MIN_SIZE {
            return Err(StorageError::Other(format!(
                "replay file too short: {} bytes (minimum {REPLAY_MIN_SIZE})",
                bytes.len(),
            )));
        }

        if &bytes[0..8] != REPLAY_MAGIC {
            return Err(StorageError::Other("invalid replay magic".into()));
        }

        let format_version = u16::from_le_bytes(bytes[8..10].try_into().unwrap());
        if !(OLDEST_READABLE_VERSION..=REPLAY_FORMAT_VERSION).contains(&format_version) {
            return Err(StorageError::Other(format!(
                "unsupported replay format version: {format_version} (this build reads \
                 {OLDEST_READABLE_VERSION} to {REPLAY_FORMAT_VERSION})"
            )));
        }

        let declared_ticks = u64::from_le_bytes(bytes[10..18].try_into().unwrap());
        let tick_rate = u32::from_le_bytes(bytes[18..22].try_into().unwrap());
        let _start_tick = u64::from_le_bytes(bytes[22..30].try_into().unwrap());

        let mut cursor = REPLAY_MIN_SIZE;

        // Reject a count the remaining bytes cannot possibly hold *before*
        // reserving for it: `tick_count` comes from the file, and a 30-byte
        // file declaring u64::MAX entries would otherwise abort the process in
        // `Vec::with_capacity`.
        let remaining = bytes.len() - cursor;
        let tick_count = usize::try_from(declared_ticks).unwrap_or(usize::MAX);
        if declared_ticks > (remaining / MIN_ENTRY_SIZE) as u64 {
            return Err(StorageError::Other(format!(
                "replay declares {declared_ticks} ticks but only {remaining} bytes follow the header"
            )));
        }

        let mut entries = Vec::with_capacity(tick_count);

        for _ in 0..tick_count {
            if cursor + MIN_ENTRY_SIZE > bytes.len() {
                return Err(StorageError::Other(
                    "replay file truncated in entry header".into(),
                ));
            }

            let tick_id = TickId::from_raw(u64::from_le_bytes(
                bytes[cursor..cursor + 8].try_into().unwrap(),
            ));
            cursor += 8;

            let msg_len =
                u32::from_le_bytes(bytes[cursor..cursor + 4].try_into().unwrap()) as usize;
            cursor += 4;

            // `checked_add`: on a 32-bit target `cursor + msg_len` can wrap,
            // which would pass the bounds check and then panic on the slice.
            let end = cursor
                .checked_add(msg_len)
                .filter(|end| *end <= bytes.len())
                .ok_or_else(|| StorageError::Other("replay file truncated in entry data".into()))?;

            let msg_data = bytes[cursor..end].to_vec();
            cursor = end;

            entries.push((tick_id, msg_data));
        }

        // A version 1 file ends at its entries, and its reader never looked
        // past them, so whatever follows is left unread as it always was.
        let input = if format_version >= INPUT_SECTION_VERSION {
            InputSection::decode(&bytes[cursor..], format_version >= PEER_TRACK_VERSION)?
        } else {
            InputSection::default()
        };

        Ok(Self {
            entries,
            cursor: 0,
            connected: true,
            tick_rate,
            format_version,
            input,
        })
    }

    /// The format version the file was written in.
    pub fn format_version(&self) -> u16 {
        self.format_version
    }

    /// The `Flags::SIM` console sets the recorded host applied, in the order
    /// it applied them — none for a version 1 file.
    pub fn sim_sets(&self) -> &[RecordedSimSet] {
        &self.input.sim_sets
    }

    /// The recorder's state hashes, in tick order — none for a version 1
    /// file.
    pub fn state_hashes(&self) -> &[RecordedStateHash] {
        &self.input.state_hashes
    }

    /// What the recorded host's module was handed of its peers, one entry for
    /// each tick that had any, in tick order — none before version 3.
    pub fn peer_ticks(&self) -> &[RecordedPeerTick] {
        &self.input.peer_ticks
    }

    /// The server tick rate recorded in the replay file.
    pub fn tick_rate(&self) -> u32 {
        self.tick_rate
    }

    /// The total number of entries in the replay.
    pub fn len(&self) -> usize {
        self.entries.len()
    }

    /// Whether the replay has no entries.
    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }

    /// The current playback position (index into entries).
    pub fn position(&self) -> usize {
        self.cursor
    }

    /// The tick id at the given logical entry index, or `TickId::ZERO` if out
    /// of range.
    pub fn tick_at(&self, index: usize) -> TickId {
        self.entries
            .get(index)
            .map(|(t, _)| *t)
            .unwrap_or(TickId::ZERO)
    }
}

impl Transport for FileTransport {
    fn send_reliable(&mut self, _msg: Message) -> Result<(), TransportError> {
        // Replay transport is read-only.
        Err(TransportError::Disconnected)
    }

    fn send_unreliable(&mut self, _msg: Message) -> Result<(), TransportError> {
        Err(TransportError::Disconnected)
    }

    fn recv(&mut self) -> Result<Option<Message>, TransportError> {
        if !self.connected {
            return Err(TransportError::Disconnected);
        }
        if self.cursor >= self.entries.len() {
            self.connected = false;
            return Ok(None);
        }
        let (_tick, data) = &self.entries[self.cursor];
        self.cursor += 1;
        Ok(Some(Message {
            kind: MessageKind::Reliable,
            payload: data.clone(),
        }))
    }

    fn is_connected(&self) -> bool {
        self.connected
    }
}

// ── Save→load→hash roundtrip ──────────────────────────────────────────────
//
// The exit criterion for P2c is:
//   save→load→hash green; a recorded session replays identically.
//
// The full integration test (record from a real Server → write → FileTransport
// → replay → hash match) requires the server and client crates. The unit tests
// below verify the replay format itself: write → read → same messages.

// ── Tests ──────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;
    use crate::MemoryStorage;

    fn sample_replay_data() -> ReplayWriter {
        let mut writer = ReplayWriter::new(60);
        writer.push_tick(TickId::from_raw(1), b"snapshot_1");
        writer.push_tick(TickId::from_raw(2), b"snapshot_2");
        writer.push_tick(TickId::from_raw(3), b"snapshot_3");
        writer
    }

    #[test]
    fn replay_write_read_roundtrip() {
        let storage = MemoryStorage::new();
        let writer = sample_replay_data();
        let path = Path::new("test.crpl");

        writer.write(&storage, path).unwrap();
        assert!(storage.exists(path));

        let transport = FileTransport::open(&storage, path).unwrap();
        assert_eq!(transport.tick_rate(), 60);
        assert_eq!(transport.len(), 3);
    }

    #[test]
    fn replay_playback_emits_messages_in_order() {
        let storage = MemoryStorage::new();
        let writer = sample_replay_data();
        let path = Path::new("test.crpl");

        writer.write(&storage, path).unwrap();

        let mut transport = FileTransport::open(&storage, path).unwrap();
        assert!(transport.is_connected());

        let msg1 = transport.recv().unwrap().unwrap();
        assert_eq!(msg1.payload, b"snapshot_1");

        let msg2 = transport.recv().unwrap().unwrap();
        assert_eq!(msg2.payload, b"snapshot_2");

        let msg3 = transport.recv().unwrap().unwrap();
        assert_eq!(msg3.payload, b"snapshot_3");

        // No more messages.
        let none = transport.recv().unwrap();
        assert!(none.is_none());
        assert!(!transport.is_connected());
    }

    #[test]
    fn an_empty_replay_keeps_its_tick_rate_and_reports_no_ticks_to_replay() {
        let storage = MemoryStorage::new();
        let writer = ReplayWriter::new(30);
        let path = Path::new("empty.crpl");

        writer.write(&storage, path).unwrap();

        let mut transport = FileTransport::open(&storage, path).unwrap();
        assert_eq!(transport.tick_rate(), 30);
        assert_eq!(transport.len(), 0);
        assert!(transport.recv().unwrap().is_none());
        assert!(!transport.is_connected());
    }

    #[test]
    fn replay_tick_rate_preserved() {
        let storage = MemoryStorage::new();
        let mut writer = ReplayWriter::new(120);
        let path = Path::new("rate.crpl");

        writer.push_tick(TickId::from_raw(1), b"x");
        writer.write(&storage, path).unwrap();

        let transport = FileTransport::open(&storage, path).unwrap();
        assert_eq!(transport.tick_rate(), 120);
    }

    #[test]
    fn tick_at_answers_the_recorded_tick_and_zero_past_the_end() {
        let storage = MemoryStorage::new();
        let mut writer = ReplayWriter::new(60);
        writer.push_tick(TickId::from_raw(10), b"msg");
        writer.push_tick(TickId::from_raw(20), b"msg");
        let path = Path::new("ticks.crpl");
        writer.write(&storage, path).unwrap();

        let transport = FileTransport::open(&storage, path).unwrap();
        assert_eq!(transport.tick_at(0), TickId::from_raw(10));
        assert_eq!(transport.tick_at(1), TickId::from_raw(20));
        assert_eq!(transport.tick_at(2), TickId::from_raw(0)); // out of range
    }

    #[test]
    fn replay_invalid_magic_rejected() {
        let storage = MemoryStorage::new();
        let mut data = vec![0u8; REPLAY_MIN_SIZE];
        data[0..8].copy_from_slice(b"BADREPLY");
        storage.write(Path::new("bad.crpl"), &data).unwrap();

        let result = FileTransport::open(&storage, Path::new("bad.crpl"));
        assert!(result.is_err());
        assert!(
            result
                .unwrap_err()
                .to_string()
                .contains("invalid replay magic")
        );
    }

    #[test]
    fn replay_absurd_tick_count_rejected_without_allocating() {
        let storage = MemoryStorage::new();
        // A 30-byte header claiming u64::MAX entries: the old code fed that
        // straight into `Vec::with_capacity` and aborted the process.
        let mut data = vec![0u8; REPLAY_MIN_SIZE];
        data[0..8].copy_from_slice(REPLAY_MAGIC);
        data[8..10].copy_from_slice(&REPLAY_FORMAT_VERSION.to_le_bytes());
        data[10..18].copy_from_slice(&u64::MAX.to_le_bytes());
        storage.write(Path::new("huge.crpl"), &data).unwrap();

        let err = FileTransport::open(&storage, Path::new("huge.crpl")).unwrap_err();
        assert!(err.to_string().contains("bytes follow the header"));
    }

    #[test]
    fn replay_tick_count_beyond_remaining_bytes_rejected() {
        let storage = MemoryStorage::new();
        // Room for exactly one minimum-size entry, but two are declared.
        let mut data = vec![0u8; REPLAY_MIN_SIZE + 12];
        data[0..8].copy_from_slice(REPLAY_MAGIC);
        data[8..10].copy_from_slice(&REPLAY_FORMAT_VERSION.to_le_bytes());
        data[10..18].copy_from_slice(&2u64.to_le_bytes());
        storage.write(Path::new("over.crpl"), &data).unwrap();

        assert!(FileTransport::open(&storage, Path::new("over.crpl")).is_err());
    }

    #[test]
    fn replay_entry_length_beyond_file_rejected() {
        let storage = MemoryStorage::new();
        let mut data = vec![0u8; REPLAY_MIN_SIZE + 12];
        data[0..8].copy_from_slice(REPLAY_MAGIC);
        data[8..10].copy_from_slice(&REPLAY_FORMAT_VERSION.to_le_bytes());
        data[10..18].copy_from_slice(&1u64.to_le_bytes());
        // msg_len = u32::MAX for the single entry.
        data[REPLAY_MIN_SIZE + 8..REPLAY_MIN_SIZE + 12].copy_from_slice(&u32::MAX.to_le_bytes());
        storage.write(Path::new("len.crpl"), &data).unwrap();

        let err = FileTransport::open(&storage, Path::new("len.crpl")).unwrap_err();
        assert!(err.to_string().contains("truncated in entry data"));
    }

    #[test]
    fn replay_too_short_rejected() {
        let storage = MemoryStorage::new();
        storage.write(Path::new("short.crpl"), b"tooshrt").unwrap();

        let result = FileTransport::open(&storage, Path::new("short.crpl"));
        assert!(result.is_err());
    }

    #[test]
    fn send_fails_on_replay_transport() {
        let storage = MemoryStorage::new();
        let writer = sample_replay_data();
        let path = Path::new("test.crpl");
        writer.write(&storage, path).unwrap();

        let mut transport = FileTransport::open(&storage, path).unwrap();
        let msg = Message {
            kind: MessageKind::Reliable,
            payload: vec![0],
        };
        assert!(transport.send_reliable(msg).is_err());
    }

    fn spin_rate(value: &str) -> ConsoleSet {
        ConsoleSet {
            name: "sv_spin_rate".to_owned(),
            value: value.to_owned(),
        }
    }

    #[test]
    fn sets_and_state_hashes_read_back_as_written() {
        let storage = MemoryStorage::new();
        let mut writer = sample_replay_data();
        writer.push_sim_set(TickId::from_raw(2), spin_rate("2.5"));
        writer.push_sim_set(TickId::from_raw(2), spin_rate("0.1"));
        writer.push_state_hash(TickId::from_raw(1), 0x1111);
        writer.push_state_hash(TickId::from_raw(3), u64::MAX);
        let path = Path::new("inputs.crpl");
        writer.write(&storage, path).unwrap();

        let mut transport = FileTransport::open(&storage, path).unwrap();
        assert_eq!(transport.format_version(), REPLAY_FORMAT_VERSION);
        assert_eq!(
            transport.sim_sets(),
            [
                RecordedSimSet {
                    tick: TickId::from_raw(2),
                    set: spin_rate("2.5"),
                },
                RecordedSimSet {
                    tick: TickId::from_raw(2),
                    set: spin_rate("0.1"),
                },
            ]
        );
        assert_eq!(
            transport.state_hashes(),
            [
                RecordedStateHash {
                    tick: TickId::from_raw(1),
                    hash: 0x1111,
                },
                RecordedStateHash {
                    tick: TickId::from_raw(3),
                    hash: u64::MAX,
                },
            ]
        );
        // The section follows the entries and leaves them as they were.
        assert_eq!(transport.len(), 3);
        assert_eq!(transport.recv().unwrap().unwrap().payload, b"snapshot_1");
    }

    fn joined_and_sent(tick: u64) -> RecordedPeerTick {
        RecordedPeerTick {
            tick: TickId::from_raw(tick),
            roster: vec![RecordedRosterChange {
                kind: RosterChangeKind::Joined,
                peer: 7,
            }],
            peers: vec![RecordedPeerFrames {
                peer: 7,
                dropped: 1,
                frames: vec![(TickId::from_raw(tick - 1), vec![1, 2, 3])],
            }],
        }
    }

    #[test]
    fn the_peer_track_reads_back_as_written() {
        let storage = MemoryStorage::new();
        let mut writer = sample_replay_data();
        writer.push_sim_set(TickId::from_raw(2), spin_rate("2.5"));
        writer.push_state_hash(TickId::from_raw(3), 9);
        writer.push_peer_tick(joined_and_sent(2));
        writer.push_peer_tick(RecordedPeerTick {
            tick: TickId::from_raw(3),
            roster: vec![RecordedRosterChange {
                kind: RosterChangeKind::Ended,
                peer: 7,
            }],
            peers: Vec::new(),
        });
        let path = Path::new("peers.crpl");
        writer.write(&storage, path).unwrap();

        let mut transport = FileTransport::open(&storage, path).unwrap();
        assert_eq!(transport.format_version(), 3);
        assert_eq!(transport.peer_ticks(), writer.input.peer_ticks);
        assert_eq!(transport.peer_ticks().len(), 2);
        // The track ends the section and leaves the rest as it was.
        assert_eq!(transport.sim_sets().len(), 1);
        assert_eq!(transport.state_hashes().len(), 1);
        assert_eq!(transport.recv().unwrap().unwrap().payload, b"snapshot_1");
    }

    #[test]
    fn a_version_2_file_reads_its_sets_and_hashes_with_no_peer_track() {
        let mut writer = sample_replay_data();
        writer.push_sim_set(TickId::from_raw(2), spin_rate("2.5"));
        writer.push_state_hash(TickId::from_raw(3), 9);
        // As the version 2 writer wrote it: the same, without the track's
        // count, which ends a version 3 file with no peer ticks.
        let mut bytes = writer.encode().unwrap();
        assert_eq!(bytes.split_off(bytes.len() - 4), [0; 4]);
        bytes[8..10].copy_from_slice(&2u16.to_le_bytes());

        let mut transport = FileTransport::decode(&bytes).unwrap();
        assert_eq!(transport.format_version(), 2);
        assert_eq!(transport.sim_sets(), writer.input.sim_sets);
        assert_eq!(transport.state_hashes(), writer.input.state_hashes);
        assert!(transport.peer_ticks().is_empty());
        assert_eq!(transport.len(), 3);
        assert_eq!(transport.recv().unwrap().unwrap().payload, b"snapshot_1");
    }

    #[test]
    fn a_malformed_peer_track_is_refused_through_the_file() {
        let mut writer = ReplayWriter::new(60);
        writer.push_peer_tick(joined_and_sent(5));
        writer.push_peer_tick(joined_and_sent(5));
        let storage = MemoryStorage::new();
        let err = writer.write(&storage, Path::new("bad.crpl")).unwrap_err();
        assert!(
            matches!(
                err,
                StorageError::ReplayInput(InputSectionError::PeerTickOutOfOrder { .. })
            ),
            "{err}"
        );
        assert!(!storage.exists(Path::new("bad.crpl")), "nothing written");

        // A version 3 header over a version 2 body: the track is missing.
        let mut bytes = ReplayWriter::new(60).encode().unwrap();
        bytes.truncate(bytes.len() - 4);
        let err = FileTransport::decode(&bytes).unwrap_err();
        assert!(
            matches!(
                err,
                StorageError::ReplayInput(InputSectionError::Truncated("peer ticks"))
            ),
            "{err}"
        );
    }

    /// A file as the version 1 writer wrote it: the header and the entries,
    /// and nothing after them.
    fn version_1_bytes(tick_rate: u32, entries: &[(u64, &[u8])]) -> Vec<u8> {
        let mut buf = REPLAY_MAGIC.to_vec();
        buf.extend_from_slice(&1u16.to_le_bytes());
        buf.extend_from_slice(&(entries.len() as u64).to_le_bytes());
        buf.extend_from_slice(&tick_rate.to_le_bytes());
        buf.extend_from_slice(&entries.first().map_or(0, |(tick, _)| *tick).to_le_bytes());
        for (tick, data) in entries {
            buf.extend_from_slice(&tick.to_le_bytes());
            buf.extend_from_slice(&(data.len() as u32).to_le_bytes());
            buf.extend_from_slice(data);
        }
        buf
    }

    #[test]
    fn a_version_1_file_reads_as_before_with_no_sets_and_no_hashes() {
        let bytes = version_1_bytes(60, &[(7, b"first"), (8, b"second")]);
        let mut transport = FileTransport::decode(&bytes).unwrap();
        assert_eq!(transport.format_version(), 1);
        assert_eq!(transport.tick_rate(), 60);
        assert_eq!(transport.len(), 2);
        assert_eq!(transport.tick_at(1), TickId::from_raw(8));
        assert!(transport.sim_sets().is_empty());
        assert!(transport.state_hashes().is_empty());
        assert_eq!(transport.recv().unwrap().unwrap().payload, b"first");
        assert_eq!(transport.recv().unwrap().unwrap().payload, b"second");
        assert!(transport.recv().unwrap().is_none());

        // Its reader never looked past the entries, so a byte there is left
        // unread rather than taken for a section it cannot have.
        let mut trailing = version_1_bytes(60, &[(7, b"first")]);
        trailing.push(0xFF);
        assert!(FileTransport::decode(&trailing).is_ok());
    }

    #[test]
    fn a_version_2_file_without_its_section_is_refused_by_name() {
        // A version 2 header over a version 1 body: the counts are missing.
        let mut bytes = version_1_bytes(60, &[(1, b"x")]);
        bytes[8..10].copy_from_slice(&2u16.to_le_bytes());
        let err = FileTransport::decode(&bytes).unwrap_err();
        assert!(
            matches!(
                err,
                StorageError::ReplayInput(InputSectionError::Truncated("sets"))
            ),
            "{err}"
        );
    }

    #[test]
    fn a_malformed_section_is_refused_through_the_file() {
        let mut writer = ReplayWriter::new(60);
        writer.push_sim_set(TickId::from_raw(4), spin_rate("1"));
        writer.push_sim_set(TickId::from_raw(3), spin_rate("2"));
        let storage = MemoryStorage::new();
        let err = writer.write(&storage, Path::new("bad.crpl")).unwrap_err();
        assert!(
            matches!(
                err,
                StorageError::ReplayInput(InputSectionError::SetOutOfOrder { .. })
            ),
            "{err}"
        );
        assert!(!storage.exists(Path::new("bad.crpl")), "nothing written");

        let mut bytes = ReplayWriter::new(60).encode().unwrap();
        bytes.push(0);
        let err = FileTransport::decode(&bytes).unwrap_err();
        assert!(
            matches!(
                err,
                StorageError::ReplayInput(InputSectionError::TrailingBytes(1))
            ),
            "{err}"
        );
    }

    #[test]
    fn a_version_newer_than_this_build_is_refused() {
        let mut bytes = ReplayWriter::new(60).encode().unwrap();
        bytes[8..10].copy_from_slice(&(REPLAY_FORMAT_VERSION + 1).to_le_bytes());
        let err = FileTransport::decode(&bytes).unwrap_err();
        assert!(
            err.to_string()
                .contains("unsupported replay format version"),
            "{err}"
        );
        bytes[8..10].copy_from_slice(&0u16.to_le_bytes());
        assert!(FileTransport::decode(&bytes).is_err(), "nor version 0");
    }

    #[test]
    fn replay_tick_count_matches() {
        let storage = MemoryStorage::new();
        let mut writer = ReplayWriter::new(60);
        for i in 0..5 {
            writer.push_tick(TickId::from_raw(i), b"data");
        }
        let path = Path::new("five.crpl");
        writer.write(&storage, path).unwrap();

        let transport = FileTransport::open(&storage, path).unwrap();
        assert_eq!(transport.len(), 5);
        assert!(!transport.is_empty());
    }
}
