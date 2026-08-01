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
//! [`FileTransport`] reads a `.crpl` file and emits entries as if they were
//! arriving from a live network transport.

use std::path::Path;

use crcbl_core::TickId;
use crcbl_net::transport::{Message, MessageKind, Transport, TransportError};

use crate::{StorageError, StorageSource};

// ── Constants ──────────────────────────────────────────────────────────────

/// Magic bytes identifying a `.crpl` replay file.
pub const REPLAY_MAGIC: &[u8; 8] = b"CRBLREPL";

/// Current replay format version.
pub const REPLAY_FORMAT_VERSION: u16 = 1;

/// Minimum size of a valid replay file: header only (empty replay is valid).
pub const REPLAY_MIN_SIZE: usize = 30;

/// Smallest possible `TickEntry`: `tick_id` + `msg_len`, with no payload.
const MIN_ENTRY_SIZE: usize = 12;

// ── ReplayWriter ───────────────────────────────────────────────────────────

/// Records server output messages into a `.crpl` replay buffer.
///
/// Call [`push_tick`](Self::push_tick) for each tick's server message, then
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

    /// Encode the replay into a byte buffer.
    fn encode(&self) -> Vec<u8> {
        let total_entries = self.entries.len() as u64;
        let mut buf = Vec::with_capacity(
            REPLAY_MIN_SIZE + self.entries.iter().map(|(_, d)| d.len()).sum::<usize>(),
        );

        buf.extend_from_slice(REPLAY_MAGIC);
        buf.extend_from_slice(&REPLAY_FORMAT_VERSION.to_le_bytes());
        buf.extend_from_slice(&total_entries.to_le_bytes());
        buf.extend_from_slice(&self.tick_rate.to_le_bytes());

        let start_tick = self.entries.first().map(|(t, _)| t.get()).unwrap_or(0);
        buf.extend_from_slice(&start_tick.to_le_bytes());

        for (tick, data) in &self.entries {
            buf.extend_from_slice(&tick.get().to_le_bytes());
            let len = data.len() as u32;
            buf.extend_from_slice(&len.to_le_bytes());
            buf.extend_from_slice(data);
        }

        buf
    }

    /// Write the replay file atomically through `storage` at `path`.
    pub fn write(&self, storage: &dyn StorageSource, path: &Path) -> Result<(), StorageError> {
        let data = self.encode();
        storage.write(path, &data)
    }
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
}

impl FileTransport {
    /// Open a `.crpl` replay file from `path` in `storage`.
    ///
    /// Validates the magic and format version. Returns an error if the file is
    /// too short, has an invalid magic, or contains an unsupported format
    /// version.
    pub fn open(storage: &dyn StorageSource, path: &Path) -> Result<Self, StorageError> {
        let bytes = storage.read(path)?;

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
        if format_version != REPLAY_FORMAT_VERSION {
            return Err(StorageError::Other(format!(
                "unsupported replay format version: {format_version} (expected {REPLAY_FORMAT_VERSION})"
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

        Ok(Self {
            entries,
            cursor: 0,
            connected: true,
            tick_rate,
        })
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
    fn replay_empty_writer() {
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
    fn replay_tick_at() {
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
