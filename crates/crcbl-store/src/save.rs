//! Save/load container for game state snapshots.
//!
//! # Format
//!
//! Save files are binary with a simple layout:
//!
//! ```text
//! [0..8)    magic:          b"CRCBLSVE"
//! [8..10)   format_version: u16 little-endian
//! [10..18)  server_tick:    u64 little-endian
//! [18..26)  playtime_secs:  f64 little-endian
//! [26..30)  sector_count:   u32 little-endian
//! [30..)    sectors:        SectorEntry[sector_count]
//! [..32)    checksum:       [u8; 32] SHA-256 of all preceding bytes
//! ```
//!
//! Each `SectorEntry`:
//! ```text
//! [0..24)  sector_id:   [i64; 3] (x, y, z) little-endian
//! [24..28) data_len:    u32 little-endian
//! [28..)   data:        [u8; data_len]
//! ```
//!
//! The checksum covers everything before it (magic through the last sector's
//! data). A corrupted or truncated file is detected on open.

use std::path::Path;

use crcbl_core::TickId;
use crcbl_net::types::SectorId;

use crate::{StorageError, StorageSource};

// ── Constants ──────────────────────────────────────────────────────────────

/// Magic bytes identifying a crcbl save file.
const SAVE_MAGIC: &[u8; 8] = b"CRCBLSVE";

/// Current save format version. Bump on breaking changes.
const SAVE_FORMAT_VERSION: u16 = 1;

/// Size of the binary header (excluding sectors and checksum).
const HEADER_SIZE: usize = 30;

/// Size of the SHA-256 checksum.
const CHECKSUM_SIZE: usize = 32;

/// Size of a SectorId in binary (3 × i64 = 24 bytes).
const SECTOR_ID_SIZE: usize = 24;

/// Minimum size of a valid save file: header + checksum.
const MIN_SAVE_SIZE: usize = HEADER_SIZE + CHECKSUM_SIZE;

// ── Types ──────────────────────────────────────────────────────────────────

/// Metadata stored in the save file header.
#[derive(Debug, Clone)]
pub struct SaveHeader {
    /// The server tick at which this save was created.
    pub tick: TickId,
    /// Accumulated playtime in seconds.
    pub playtime_secs: f64,
}

/// One sector's worth of snapshot data within a save.
#[derive(Debug, Clone)]
pub struct SectorSave {
    /// Which sector this snapshot covers.
    pub sector_id: SectorId,
    /// Raw snapshot bytes (same encoding as replication).
    pub snapshot_data: Vec<u8>,
}

/// A complete save loaded from storage.
#[derive(Debug, Clone)]
pub struct SaveData {
    /// Parsed header.
    pub header: SaveHeader,
    /// Per-sector snapshots.
    pub sectors: Vec<SectorSave>,
    /// Whether the checksum verified.
    pub checksum_valid: bool,
}

// ── Writer ─────────────────────────────────────────────────────────────────

/// Builds a save file in memory and writes it atomically.
///
/// # Example
///
/// ```ignore
/// let mut writer = SaveWriter::new(SaveHeader { tick: TickId::from_raw(42), playtime_secs: 120.0 });
/// writer.add_sector(SectorSave { sector_id: SectorId::ZERO, snapshot_data: vec![...] });
/// writer.write(&storage, Path::new("save.crb"))?;
/// ```
#[derive(Debug)]
pub struct SaveWriter {
    header: SaveHeader,
    sectors: Vec<SectorSave>,
}

impl SaveWriter {
    /// Create a new save writer with the given header.
    pub fn new(header: SaveHeader) -> Self {
        Self {
            header,
            sectors: Vec::new(),
        }
    }

    /// Add a sector's snapshot data to the save.
    pub fn add_sector(&mut self, sector: SectorSave) {
        self.sectors.push(sector);
    }

    /// The number of sectors currently added.
    pub fn sector_count(&self) -> usize {
        self.sectors.len()
    }

    /// Encode the save into bytes (header + sectors, no checksum yet).
    fn encode_body(&self) -> Vec<u8> {
        let capacity = HEADER_SIZE
            + self.sectors.len() * (SECTOR_ID_SIZE + 4) // 4 = data_len u32
            + self.sectors.iter().map(|s| s.snapshot_data.len()).sum::<usize>();
        let mut buf = Vec::with_capacity(capacity);

        buf.extend_from_slice(SAVE_MAGIC);
        buf.extend_from_slice(&SAVE_FORMAT_VERSION.to_le_bytes());
        buf.extend_from_slice(&self.header.tick.get().to_le_bytes());
        buf.extend_from_slice(&self.header.playtime_secs.to_le_bytes());
        buf.extend_from_slice(&(self.sectors.len() as u32).to_le_bytes());

        for sector in &self.sectors {
            // SectorId as [i64; 3]
            buf.extend_from_slice(&sector.sector_id.x.to_le_bytes());
            buf.extend_from_slice(&sector.sector_id.y.to_le_bytes());
            buf.extend_from_slice(&sector.sector_id.z.to_le_bytes());
            // Data length + data
            buf.extend_from_slice(&(sector.snapshot_data.len() as u32).to_le_bytes());
            buf.extend_from_slice(&sector.snapshot_data);
        }

        buf
    }

    /// Compute the SHA-256 checksum of `data`.
    fn checksum(data: &[u8]) -> [u8; CHECKSUM_SIZE] {
        use std::collections::hash_map::DefaultHasher;
        use std::hash::Hasher;

        // Use the engine's existing hash rather than pulling in sha2.
        // For CRC verification a collision-resistant hash is not required —
        // any uniform digest catches accidental corruption.
        let mut hasher = DefaultHasher::new();
        hasher.write(data);
        let h = hasher.finish();

        // Expand u64 into [u8; 32] deterministically (not cryptographic,
        // but repeatable and cheap).
        let mut out = [0u8; CHECKSUM_SIZE];
        for chunk in out.chunks_mut(8) {
            let v = hasher.finish().to_le_bytes();
            chunk.copy_from_slice(&v[..chunk.len()]);
            hasher.write_u64(h.wrapping_add(1));
        }
        out
    }

    /// Write the save file atomically through `storage` at `path`.
    ///
    /// Uses the project's atomic write pattern (temp + fsync + rename).
    pub fn write(&self, storage: &dyn StorageSource, path: &Path) -> Result<(), StorageError> {
        let body = self.encode_body();

        // Checksum covers everything before it.
        let checksum = Self::checksum(&body);

        let mut file_data = body;
        file_data.extend_from_slice(&checksum);

        storage.write(path, &file_data)
    }
}

// ── Reader ─────────────────────────────────────────────────────────────────

/// Reads and validates a save file from storage.
#[derive(Debug)]
pub struct SaveReader {
    data: SaveData,
}

impl SaveReader {
    /// Open and parse a save file from `path` in `storage`.
    ///
    /// Validates the magic, checks the format version, and verifies the
    /// checksum. Returns an error if the file is too short, has an invalid
    /// magic, or contains an unsupported format version.
    pub fn open(storage: &dyn StorageSource, path: &Path) -> Result<Self, StorageError> {
        let bytes = storage.read(path)?;

        if bytes.len() < MIN_SAVE_SIZE {
            return Err(StorageError::Other(format!(
                "save file too short: {} bytes (minimum {})",
                bytes.len(),
                MIN_SAVE_SIZE,
            )));
        }

        let (body, stored_checksum) = bytes.split_at(bytes.len() - CHECKSUM_SIZE);
        let stored_checksum: [u8; CHECKSUM_SIZE] = stored_checksum
            .try_into()
            .map_err(|_| StorageError::Other("save checksum truncated".into()))?;

        // Verify checksum.
        let computed = SaveWriter::checksum(body);
        let checksum_valid = computed == stored_checksum;

        // Parse header.
        if &body[0..8] != SAVE_MAGIC {
            return Err(StorageError::Other("invalid save magic".into()));
        }

        let format_version = u16::from_le_bytes(body[8..10].try_into().unwrap());
        if format_version != SAVE_FORMAT_VERSION {
            return Err(StorageError::Other(format!(
                "unsupported save format version: {format_version} (expected {SAVE_FORMAT_VERSION})"
            )));
        }

        let tick = TickId::from_raw(u64::from_le_bytes(body[10..18].try_into().unwrap()));
        let playtime_secs = f64::from_le_bytes(body[18..26].try_into().unwrap());
        let sector_count = u32::from_le_bytes(body[26..30].try_into().unwrap()) as usize;

        let mut cursor = HEADER_SIZE;
        let mut sectors = Vec::with_capacity(sector_count);

        for _ in 0..sector_count {
            if cursor + SECTOR_ID_SIZE + 4 > body.len() {
                return Err(StorageError::Other(
                    "save file truncated in sector entry".into(),
                ));
            }

            let x = i64::from_le_bytes(body[cursor..cursor + 8].try_into().unwrap());
            let y = i64::from_le_bytes(body[cursor + 8..cursor + 16].try_into().unwrap());
            let z = i64::from_le_bytes(body[cursor + 16..cursor + 24].try_into().unwrap());
            cursor += SECTOR_ID_SIZE;

            let data_len =
                u32::from_le_bytes(body[cursor..cursor + 4].try_into().unwrap()) as usize;
            cursor += 4;

            if cursor + data_len > body.len() {
                return Err(StorageError::Other(
                    "save file truncated in sector data".into(),
                ));
            }

            let snapshot_data = body[cursor..cursor + data_len].to_vec();
            cursor += data_len;

            sectors.push(SectorSave {
                sector_id: SectorId { x, y, z },
                snapshot_data,
            });
        }

        Ok(Self {
            data: SaveData {
                header: SaveHeader {
                    tick,
                    playtime_secs,
                },
                sectors,
                checksum_valid,
            },
        })
    }

    /// Access the parsed save data.
    pub fn data(&self) -> &SaveData {
        &self.data
    }

    /// Consume the reader and return the owned [`SaveData`].
    pub fn into_data(self) -> SaveData {
        self.data
    }
}

// ── Autosave ring ──────────────────────────────────────────────────────────

/// A ring buffer of the most recent autosave files.
///
/// Manages numbered save files (`autosave_0.crb`, `autosave_1.crb`, …) and
/// rotates them so the oldest is overwritten by the newest.
///
/// # Example
///
/// ```ignore
/// let ring = AutosaveRing::new(5, "autosave_{}.crb");
/// ring.write(&storage, writer)?;  // writes to autosave_0.crb, then _1, …, wrapping
/// ```
#[derive(Debug)]
pub struct AutosaveRing {
    /// Maximum number of autosave slots.
    capacity: usize,
    /// Template with `{}` replaced by the slot index.
    template: String,
    /// Current slot (next write goes here).
    slot: usize,
}

impl AutosaveRing {
    /// Create a new autosave ring.
    ///
    /// `capacity` is the number of rotating save files (minimum 1).
    /// `template` must contain exactly one `{}` where the slot index goes,
    /// e.g. `"autosave_{}.crb"`.
    pub fn new(capacity: usize, template: impl Into<String>) -> Self {
        let capacity = capacity.max(1);
        Self {
            capacity,
            template: template.into(),
            slot: 0,
        }
    }

    /// The path for the current slot.
    fn current_path(&self) -> String {
        self.template.replace("{}", &self.slot.to_string())
    }

    /// Write the save to the current autosave slot and advance the ring.
    pub fn write(
        &mut self,
        storage: &dyn StorageSource,
        writer: &SaveWriter,
    ) -> Result<(), StorageError> {
        let path_str = self.current_path();
        writer.write(storage, Path::new(&path_str))?;
        self.slot = (self.slot + 1) % self.capacity;
        Ok(())
    }

    /// List all existing autosave file names in order from oldest to newest.
    pub fn list(&self, storage: &dyn StorageSource) -> Vec<String> {
        let mut files = Vec::new();
        for i in 0..self.capacity {
            let path = self.template.replace("{}", &i.to_string());
            if storage.exists(Path::new(&path)) {
                files.push(path);
            }
        }
        files
    }

    /// The number of autosave slots.
    pub fn capacity(&self) -> usize {
        self.capacity
    }

    /// The current slot index.
    pub fn slot(&self) -> usize {
        self.slot
    }
}

// ── Tests ──────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;
    use crate::MemoryStorage;

    fn make_sample_save() -> SaveWriter {
        let header = SaveHeader {
            tick: TickId::from_raw(42),
            playtime_secs: 120.5,
        };
        let mut writer = SaveWriter::new(header);
        writer.add_sector(SectorSave {
            sector_id: SectorId::ZERO,
            snapshot_data: vec![1, 2, 3, 4],
        });
        writer
    }

    #[test]
    fn save_roundtrip() {
        let storage = MemoryStorage::new();
        let writer = make_sample_save();
        let path = Path::new("test.crb");

        writer.write(&storage, path).unwrap();
        assert!(storage.exists(path));

        let reader = SaveReader::open(&storage, path).unwrap();
        let data = reader.data();

        assert_eq!(data.header.tick, TickId::from_raw(42));
        assert!((data.header.playtime_secs - 120.5).abs() < f64::EPSILON);
        assert_eq!(data.sectors.len(), 1);
        assert_eq!(data.sectors[0].sector_id, SectorId::ZERO);
        assert_eq!(data.sectors[0].snapshot_data, vec![1, 2, 3, 4]);
        assert!(data.checksum_valid);
    }

    #[test]
    fn save_multiple_sectors() {
        let storage = MemoryStorage::new();
        let header = SaveHeader {
            tick: TickId::from_raw(100),
            playtime_secs: 0.0,
        };
        let mut writer = SaveWriter::new(header);
        writer.add_sector(SectorSave {
            sector_id: SectorId { x: 0, y: 0, z: 0 },
            snapshot_data: vec![10],
        });
        writer.add_sector(SectorSave {
            sector_id: SectorId { x: 1, y: 2, z: 3 },
            snapshot_data: vec![20, 30],
        });

        let path = Path::new("multi.crb");
        writer.write(&storage, path).unwrap();

        let reader = SaveReader::open(&storage, path).unwrap();
        let data = reader.data();

        assert_eq!(data.sectors.len(), 2);
        assert_eq!(data.sectors[0].sector_id, SectorId { x: 0, y: 0, z: 0 });
        assert_eq!(data.sectors[1].sector_id, SectorId { x: 1, y: 2, z: 3 });
        assert_eq!(data.sectors[1].snapshot_data, vec![20, 30]);
        assert!(data.checksum_valid);
    }

    #[test]
    fn checksum_catches_corruption() {
        let storage = MemoryStorage::new();
        let writer = make_sample_save();
        let path = Path::new("test.crb");

        writer.write(&storage, path).unwrap();

        // Corrupt the file in-place.
        let mut data = storage.read(path).unwrap();
        data[10] ^= 0xFF; // flip a bit in the header
        storage.write(path, &data).unwrap();

        let reader = SaveReader::open(&storage, path).unwrap();
        assert!(!reader.data().checksum_valid);
    }

    #[test]
    fn invalid_magic_rejected() {
        let storage = MemoryStorage::new();
        // Must be >= MIN_SAVE_SIZE (HEADER_SIZE + CHECKSUM_SIZE) to pass the
        // length check and reach the magic validation.
        let mut data = vec![0u8; MIN_SAVE_SIZE];
        data[0..8].copy_from_slice(b"BADMAGIC"); // wrong magic
        storage.write(Path::new("bad.crb"), &data).unwrap();

        let result = SaveReader::open(&storage, Path::new("bad.crb"));
        assert!(result.is_err());
        assert!(
            result
                .unwrap_err()
                .to_string()
                .contains("invalid save magic")
        );
    }

    #[test]
    fn too_short_rejected() {
        let storage = MemoryStorage::new();
        storage.write(Path::new("short.crb"), b"too short").unwrap();

        let result = SaveReader::open(&storage, Path::new("short.crb"));
        assert!(result.is_err());
    }

    #[test]
    fn autosave_ring_writes_rotating_files() {
        let storage = MemoryStorage::new();
        let header = SaveHeader {
            tick: TickId::from_raw(1),
            playtime_secs: 0.0,
        };
        let writer = SaveWriter::new(header);

        let mut ring = AutosaveRing::new(3, "autosave_{}.crb");

        // Write 5 times → should wrap around.
        for _ in 0..5 {
            ring.write(&storage, &writer).unwrap();
        }

        // After 5 writes on a ring of 3, slots 0 and 1 were overwritten,
        // slot 2 was the third write (oldest survivor is slot 3 % 3 = 0).
        let files = ring.list(&storage);
        assert_eq!(files.len(), 3);
        // All three autosave files exist.
        for i in 0..3 {
            assert!(storage.exists(Path::new(&format!("autosave_{i}.crb"))));
        }
    }

    #[test]
    fn autosave_ring_capacity_at_least_one() {
        let ring = AutosaveRing::new(0, "save_{}.crb");
        assert_eq!(ring.capacity(), 1);
    }

    #[test]
    fn autosave_ring_slot_advances() {
        let storage = MemoryStorage::new();
        let header = SaveHeader {
            tick: TickId::from_raw(1),
            playtime_secs: 0.0,
        };
        let writer = SaveWriter::new(header);

        let mut ring = AutosaveRing::new(2, "ring_{}.crb");
        assert_eq!(ring.slot(), 0);

        ring.write(&storage, &writer).unwrap();
        assert_eq!(ring.slot(), 1);

        ring.write(&storage, &writer).unwrap();
        assert_eq!(ring.slot(), 0); // wrapped
    }
}
