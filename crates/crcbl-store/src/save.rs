//! Save/load container for game state snapshots.
//!
//! # Format
//!
//! Save files are binary with a simple layout (format version 3):
//!
//! ```text
//! [0..8)    magic:          b"CRCBLSVE"
//! [8..10)   format_version: u16 little-endian
//! [10..18)  server_tick:    u64 little-endian
//! [18..26)  playtime_secs:  f64 little-endian
//! [26..27)  engine_len:     u8, 0 when the engine version was not recorded
//! [..)      engine_version: [u8; engine_len] UTF-8
//! [..+1)    scene_marker:   u8, 0 for no scene, 1 for a scene reference
//!           — and when it is 1:
//! [..+2)    scene_len:      u16 little-endian
//! [..)      scene_name:     [u8; scene_len] UTF-8
//! [..+32)   scene_hash:     [u8; 32]
//! [..+4)    sector_count:   u32 little-endian
//! [..)      sectors:        SectorEntry[sector_count]
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
//! data) and is a real SHA-256 ([`crcbl_shaders::sha256`]). A corrupted or
//! truncated file is detected on open: [`SaveReader::open`] fails on a checksum
//! mismatch, and [`SaveReader::open_ignoring_checksum`] is the explicit
//! salvage path for a damaged file.
//!
//! # Older and newer files
//!
//! A file at an older format version is **migrated on open**, one version at a
//! time, by the pure steps in the `migrate` module, and then read as a current
//! one; [`SaveData::format_version`] says which version it was written at. The
//! checksum is verified against the bytes as they are on disk, before any step
//! runs. Migration happens in memory only: the file keeps its old version until
//! the game next saves, because [`SaveWriter`] always writes
//! [`SAVE_FORMAT_VERSION`]. A file from a newer engine is refused as
//! [`FormatError::Newer`] rather than misread, and a step that cannot migrate
//! its input as [`FormatError::Migration`].

use std::path::Path;

use crcbl_core::TickId;
use crcbl_net::types::SectorId;

use crate::{StorageError, StorageSource};

mod migrate;

// ── Constants ──────────────────────────────────────────────────────────────

/// Magic bytes identifying a crcbl save file.
const SAVE_MAGIC: &[u8; 8] = b"CRCBLSVE";

/// The save format version [`SaveWriter`] writes. Bump on breaking changes,
/// and register the step from the previous version in the `migrate` module.
///
/// Version 2 replaced the "SHA-256" field — which was actually a
/// `DefaultHasher` digest, and so not stable across Rust releases — with a real
/// SHA-256. The layout is otherwise identical to version 1.
///
/// Version 3 added the engine version and the optional scene reference between
/// the playtime and the sector count.
pub const SAVE_FORMAT_VERSION: u16 = 3;

/// The version of the engine this build is, as [`SaveHeader::new`] records it.
///
/// `crcbl-store`'s own package version, which is the workspace's: every engine
/// crate takes `version.workspace = true`, so it is the umbrella crate's too.
pub const ENGINE_VERSION: &str = env!("CARGO_PKG_VERSION");

/// Where the format version sits: after the magic.
const VERSION_AT: usize = SAVE_MAGIC.len();

/// The magic and the format version: what every version of the file starts
/// with, and what has to be read before anything else can be.
const PREAMBLE_SIZE: usize = VERSION_AT + 2;

/// The magic, version, tick and playtime: the fixed fields every version from
/// 2 on starts with, before the fields that differ.
const FIXED_HEADER_SIZE: usize = PREAMBLE_SIZE + 8 + 8;

/// The longest engine version a header holds, in bytes: what its `u8` length
/// can say.
pub const ENGINE_VERSION_MAX: usize = u8::MAX as usize;

/// The longest scene name a header holds, in bytes.
///
/// A scene is named by its path, and this is Linux's `PATH_MAX`, the longest
/// path any platform the engine runs on takes.
pub const SCENE_NAME_MAX: usize = 4096;

// The scene name's length is written as a `u16`.
const _: () = assert!(SCENE_NAME_MAX <= u16::MAX as usize);

/// The engine version length for a header that does not record one.
const NO_ENGINE_VERSION: u8 = 0;

/// The scene marker for a header with no scene reference.
const NO_SCENE: u8 = 0;

/// The scene marker for a header with a scene reference after it.
const SCENE: u8 = 1;

/// Size of a scene's content hash: a SHA-256.
pub const SCENE_HASH_SIZE: usize = 32;

/// Size of the SHA-256 checksum.
const CHECKSUM_SIZE: usize = 32;

/// Size of a SectorId in binary (3 × i64 = 24 bytes).
const SECTOR_ID_SIZE: usize = 24;

/// Smallest possible `SectorEntry`: id + `data_len`, with no data.
const MIN_SECTOR_ENTRY_SIZE: usize = SECTOR_ID_SIZE + 4;

/// The shortest file the reader looks inside: a preamble and a checksum. Each
/// version's own header is then held to its own length, by name.
const MIN_SAVE_SIZE: usize = PREAMBLE_SIZE + CHECKSUM_SIZE;

// ── Types ──────────────────────────────────────────────────────────────────

/// Metadata stored in the save file header.
#[derive(Debug, Clone, PartialEq)]
pub struct SaveHeader {
    /// The server tick at which this save was created.
    pub tick: TickId,
    /// Accumulated playtime in seconds.
    pub playtime_secs: f64,
    /// The engine version that wrote the save — [`ENGINE_VERSION`] for one
    /// [`SaveHeader::new`] made. `None` when it was not recorded, which is
    /// every save written before format version 3.
    pub engine_version: Option<String>,
    /// The scene the save was taken in, when the game names one.
    pub scene: Option<SceneRef>,
}

impl SaveHeader {
    /// A header for a save taken now, at `tick` after `playtime_secs` of play,
    /// recording this build's [`ENGINE_VERSION`] and no scene.
    #[must_use]
    pub fn new(tick: TickId, playtime_secs: f64) -> Self {
        Self {
            tick,
            playtime_secs,
            engine_version: Some(ENGINE_VERSION.to_owned()),
            scene: None,
        }
    }
}

/// The scene a save was taken in: its name and a hash of its content, so a
/// save loaded against another scene, or another revision of the same one, can
/// be told apart from one loaded against its own.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SceneRef {
    /// The scene's name — its path, as the game resolves it. Never empty, and
    /// at most [`SCENE_NAME_MAX`] bytes.
    pub name: String,
    /// A SHA-256 of the scene's content, computed by the game that names it.
    pub content_hash: [u8; SCENE_HASH_SIZE],
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
    /// The format version the file was written at: [`SAVE_FORMAT_VERSION`],
    /// or an older one it was migrated from on open.
    pub format_version: u16,
}

/// Why the container refused a save, by the writer or the reader.
#[derive(Clone, Debug, PartialEq, Eq, thiserror::Error)]
#[non_exhaustive]
pub enum FormatError {
    /// Too short to hold a magic, a version and a checksum.
    #[error("save file too short: {len} bytes (minimum {min})")]
    TooShort {
        /// The file's length.
        len: usize,
        /// The least the reader looks inside.
        min: usize,
    },
    /// The file does not start with the save magic, `CRCBLSVE`.
    #[error("invalid save magic")]
    BadMagic,
    /// Written by a later engine at a version this build does not know.
    #[error("save format version {found} is newer than this build reads ({current})")]
    Newer {
        /// The version the file says it is.
        found: u16,
        /// [`SAVE_FORMAT_VERSION`].
        current: u16,
    },
    /// Older than the oldest version this build migrates.
    #[error("save format version {found} is older than any this build migrates (oldest {oldest})")]
    Unmigratable {
        /// The version the file says it is.
        found: u16,
        /// The oldest version this build migrates.
        oldest: u16,
    },
    /// A migration step refused its input.
    #[error("migrating a save from format version {from} to {to} failed: {reason}")]
    Migration {
        /// The version the step migrates from.
        from: u16,
        /// The version it migrates to.
        to: u16,
        /// What was wrong with its input.
        reason: &'static str,
    },
    /// The file ends inside a field.
    #[error("save file truncated in {0}")]
    Truncated(&'static str),
    /// A sector count the bytes after it cannot hold, refused before anything
    /// is reserved for it.
    #[error("save declares {declared} sectors but only {remaining} bytes follow the header")]
    CountBeyondFile {
        /// The count the file declares.
        declared: u32,
        /// The bytes left after the count.
        remaining: usize,
    },
    /// A header text longer than its field holds.
    #[error("save header's {field} is {len} bytes, past the {limit} it holds")]
    FieldTooLong {
        /// Which field.
        field: &'static str,
        /// Its length, in bytes.
        len: usize,
        /// The most it holds, in bytes.
        limit: usize,
    },
    /// A header text that must say something and is empty.
    #[error("save header's {0} is empty")]
    FieldEmpty(&'static str),
    /// A header text that is not UTF-8.
    #[error("save header's {0} is not UTF-8")]
    NotUtf8(&'static str),
    /// A scene marker that is neither "no scene" nor "a scene".
    #[error("save header's scene marker is {0}, neither the no-scene marker nor the scene one")]
    SceneMarker(u8),
    /// Bytes after the last sector, before the checksum.
    #[error("{0} bytes follow the last sector")]
    TrailingBytes(usize),
}

// ── Writer ─────────────────────────────────────────────────────────────────

/// Builds a save file in memory and writes it atomically.
///
/// # Example
///
/// ```ignore
/// let mut writer = SaveWriter::new(SaveHeader::new(TickId::from_raw(42), 120.0));
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

    /// Encode the save into bytes (header + sectors, no checksum yet), always
    /// at [`SAVE_FORMAT_VERSION`].
    ///
    /// Fails rather than truncating when a count or a sector's data length does
    /// not fit the `u32` the format reserves for it, and rather than writing a
    /// header the reader would refuse or read back differently.
    fn encode_body(&self) -> Result<Vec<u8>, StorageError> {
        let capacity = FIXED_HEADER_SIZE
            + self.sectors.len() * MIN_SECTOR_ENTRY_SIZE
            + self
                .sectors
                .iter()
                .map(|s| s.snapshot_data.len())
                .sum::<usize>();
        let mut buf = Vec::with_capacity(capacity);

        let sector_count = u32::try_from(self.sectors.len()).map_err(|_| {
            StorageError::Other(format!(
                "save has {} sectors, more than the format's u32 count",
                self.sectors.len()
            ))
        })?;

        buf.extend_from_slice(SAVE_MAGIC);
        buf.extend_from_slice(&SAVE_FORMAT_VERSION.to_le_bytes());
        buf.extend_from_slice(&self.header.tick.get().to_le_bytes());
        buf.extend_from_slice(&self.header.playtime_secs.to_le_bytes());

        match &self.header.engine_version {
            None => buf.push(NO_ENGINE_VERSION),
            Some(version) => {
                buf.push(text_len("engine version", version, ENGINE_VERSION_MAX)?);
                buf.extend_from_slice(version.as_bytes());
            }
        }

        match &self.header.scene {
            None => buf.push(NO_SCENE),
            Some(scene) => {
                let len: u16 = text_len("scene name", &scene.name, SCENE_NAME_MAX)?;
                buf.push(SCENE);
                buf.extend_from_slice(&len.to_le_bytes());
                buf.extend_from_slice(scene.name.as_bytes());
                buf.extend_from_slice(&scene.content_hash);
            }
        }

        buf.extend_from_slice(&sector_count.to_le_bytes());

        for sector in &self.sectors {
            let data_len = u32::try_from(sector.snapshot_data.len()).map_err(|_| {
                StorageError::Other(format!(
                    "sector {:?} snapshot is {} bytes, more than the format's u32 length",
                    sector.sector_id,
                    sector.snapshot_data.len()
                ))
            })?;
            // SectorId as [i64; 3]
            buf.extend_from_slice(&sector.sector_id.x.to_le_bytes());
            buf.extend_from_slice(&sector.sector_id.y.to_le_bytes());
            buf.extend_from_slice(&sector.sector_id.z.to_le_bytes());
            // Data length + data
            buf.extend_from_slice(&data_len.to_le_bytes());
            buf.extend_from_slice(&sector.snapshot_data);
        }

        Ok(buf)
    }

    /// Compute the SHA-256 checksum of `data`.
    ///
    /// This is the workspace's own SHA-256, verified against the NIST vectors
    /// in `crcbl-shaders`. It must stay a real, specified digest: the previous
    /// `DefaultHasher` expansion changed with the Rust release, so every save
    /// written by one toolchain failed its checksum under the next.
    fn checksum(data: &[u8]) -> [u8; CHECKSUM_SIZE] {
        crcbl_shaders::sha256::sha256(data)
    }

    /// Write the save file atomically through `storage` at `path`.
    ///
    /// Uses the project's atomic write pattern (temp + fsync + rename).
    pub fn write(&self, storage: &dyn StorageSource, path: &Path) -> Result<(), StorageError> {
        let body = self.encode_body()?;

        // Checksum covers everything before it.
        let checksum = Self::checksum(&body);

        let mut file_data = body;
        file_data.extend_from_slice(&checksum);

        storage.write(path, &file_data)
    }
}

/// `text`'s length as the `L` its field stores it in, refusing a text the
/// reader would refuse or read back differently: an empty one, which reads as
/// no text at all, or one longer than `limit` bytes.
fn text_len<L: TryFrom<usize>>(
    field: &'static str,
    text: &str,
    limit: usize,
) -> Result<L, FormatError> {
    if text.is_empty() {
        return Err(FormatError::FieldEmpty(field));
    }
    let too_long = FormatError::FieldTooLong {
        field,
        len: text.len(),
        limit,
    };
    if text.len() > limit {
        return Err(too_long);
    }
    L::try_from(text.len()).map_err(|_| too_long)
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
    /// Validates the magic, migrates an older format version to the current
    /// one, and verifies the checksum. Returns an error if the file is too
    /// short, has an invalid magic, is at a format version this build cannot
    /// read or migrate, or fails its checksum.
    ///
    /// Use [`open_ignoring_checksum`](Self::open_ignoring_checksum) to salvage
    /// what is readable from a file whose checksum does not match.
    pub fn open(storage: &dyn StorageSource, path: &Path) -> Result<Self, StorageError> {
        let reader = Self::open_ignoring_checksum(storage, path)?;
        if !reader.data.checksum_valid {
            return Err(StorageError::Other(format!(
                "save checksum mismatch: {} is corrupt",
                path.display()
            )));
        }
        Ok(reader)
    }

    /// Open and parse a save file without failing on a checksum mismatch.
    ///
    /// Every other validation still applies; the result's
    /// [`SaveData::checksum_valid`] reports whether the contents can be
    /// trusted.
    pub fn open_ignoring_checksum(
        storage: &dyn StorageSource,
        path: &Path,
    ) -> Result<Self, StorageError> {
        let bytes = storage.read(path)?;
        Ok(Self {
            data: decode(&bytes)?,
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

/// A whole save file, checksum and all, read at whatever version it was
/// written at.
fn decode(bytes: &[u8]) -> Result<SaveData, FormatError> {
    if bytes.len() < MIN_SAVE_SIZE {
        return Err(FormatError::TooShort {
            len: bytes.len(),
            min: MIN_SAVE_SIZE,
        });
    }

    let (body, stored_checksum) = bytes.split_at(bytes.len() - CHECKSUM_SIZE);
    // The checksum is of the bytes as written, so it is verified before any
    // migration step rewrites them.
    let checksum_valid = SaveWriter::checksum(body) == stored_checksum;

    if !body.starts_with(SAVE_MAGIC) {
        return Err(FormatError::BadMagic);
    }
    let format_version = version_of(body).ok_or(FormatError::Truncated("format version"))?;
    let current = migrate::to_current(format_version, body)?;
    let (header, sectors) = parse(&current)?;

    Ok(SaveData {
        header,
        sectors,
        checksum_valid,
        format_version,
    })
}

/// The format version a body says it is, or `None` for one too short to say.
fn version_of(body: &[u8]) -> Option<u16> {
    let bytes = body.get(VERSION_AT..PREAMBLE_SIZE)?;
    Some(u16::from_le_bytes(bytes.try_into().ok()?))
}

/// The header and sectors of a body at [`SAVE_FORMAT_VERSION`].
fn parse(body: &[u8]) -> Result<(SaveHeader, Vec<SectorSave>), FormatError> {
    let mut reader = Reader { bytes: body };
    reader.take(PREAMBLE_SIZE, "preamble")?;
    let tick = TickId::from_raw(reader.u64("tick")?);
    let playtime_secs = f64::from_bits(reader.u64("playtime")?);

    let engine_version = match reader.u8("engine version")? {
        NO_ENGINE_VERSION => None,
        len => Some(reader.text(usize::from(len), "engine version")?),
    };

    let scene = match reader.u8("scene marker")? {
        NO_SCENE => None,
        SCENE => {
            let len = usize::from(u16::from_le_bytes(reader.array("scene name")?));
            if len == 0 {
                return Err(FormatError::FieldEmpty("scene name"));
            }
            if len > SCENE_NAME_MAX {
                return Err(FormatError::FieldTooLong {
                    field: "scene name",
                    len,
                    limit: SCENE_NAME_MAX,
                });
            }
            Some(SceneRef {
                name: reader.text(len, "scene name")?,
                content_hash: reader.array("scene hash")?,
            })
        }
        other => return Err(FormatError::SceneMarker(other)),
    };

    // Reject a count the remaining bytes cannot possibly hold *before*
    // reserving for it: `sector_count` comes from the file, and a minimum-size
    // file declaring u32::MAX sectors would otherwise abort the process in
    // `Vec::with_capacity`.
    let declared = u32::from_le_bytes(reader.array("sector count")?);
    let remaining = reader.bytes.len();
    let sector_count = usize::try_from(declared)
        .ok()
        .filter(|count| *count <= remaining / MIN_SECTOR_ENTRY_SIZE)
        .ok_or(FormatError::CountBeyondFile {
            declared,
            remaining,
        })?;

    let mut sectors = Vec::with_capacity(sector_count);
    for _ in 0..sector_count {
        let x = i64::from_le_bytes(reader.array("sector entry")?);
        let y = i64::from_le_bytes(reader.array("sector entry")?);
        let z = i64::from_le_bytes(reader.array("sector entry")?);
        let data_len = u32::from_le_bytes(reader.array("sector entry")?);
        // A `u32` that does not fit a `usize` (a 16-bit target) cannot fit
        // the file either.
        let data_len = usize::try_from(data_len).unwrap_or(usize::MAX);
        let snapshot_data = reader.take(data_len, "sector data")?.to_vec();
        sectors.push(SectorSave {
            sector_id: SectorId { x, y, z },
            snapshot_data,
        });
    }

    if !reader.bytes.is_empty() {
        return Err(FormatError::TrailingBytes(reader.bytes.len()));
    }

    Ok((
        SaveHeader {
            tick,
            playtime_secs,
            engine_version,
            scene,
        },
        sectors,
    ))
}

/// The unread rest of a save's body.
struct Reader<'a> {
    bytes: &'a [u8],
}

impl<'a> Reader<'a> {
    fn take(&mut self, len: usize, what: &'static str) -> Result<&'a [u8], FormatError> {
        if self.bytes.len() < len {
            return Err(FormatError::Truncated(what));
        }
        let (taken, rest) = self.bytes.split_at(len);
        self.bytes = rest;
        Ok(taken)
    }

    fn array<const N: usize>(&mut self, what: &'static str) -> Result<[u8; N], FormatError> {
        let mut out = [0; N];
        out.copy_from_slice(self.take(N, what)?);
        Ok(out)
    }

    fn u8(&mut self, what: &'static str) -> Result<u8, FormatError> {
        self.array::<1>(what).map(|[byte]| byte)
    }

    fn u64(&mut self, what: &'static str) -> Result<u64, FormatError> {
        self.array(what).map(u64::from_le_bytes)
    }

    fn text(&mut self, len: usize, field: &'static str) -> Result<String, FormatError> {
        let bytes = self.take(len, field)?;
        let text = std::str::from_utf8(bytes).map_err(|_| FormatError::NotUtf8(field))?;
        Ok(text.to_owned())
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
/// let ring = AutosaveRing::new(5, "autosave_{}.crb")?;
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
    /// `template` must contain a `{}` where the slot index goes, e.g.
    /// `"autosave_{}.crb"`.
    ///
    /// Returns an error if `template` has no `{}` — without it every slot
    /// resolves to the same path and the ring silently keeps one save.
    pub fn new(capacity: usize, template: impl Into<String>) -> Result<Self, StorageError> {
        let capacity = capacity.max(1);
        let template = template.into();
        if !template.contains("{}") {
            return Err(StorageError::Other(format!(
                "autosave template {template:?} has no `{{}}` slot placeholder"
            )));
        }
        Ok(Self {
            capacity,
            template,
            slot: 0,
        })
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
    ///
    /// The next write goes to `slot`, so once the ring has wrapped that slot
    /// holds the *oldest* save — iterating `0..capacity` would report the
    /// reverse of the documented order.
    pub fn list(&self, storage: &dyn StorageSource) -> Vec<String> {
        let mut files = Vec::new();
        for i in 0..self.capacity {
            let idx = (self.slot + i) % self.capacity;
            let path = self.template.replace("{}", &idx.to_string());
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

// ── Where saves live ───────────────────────────────────────────────────────

/// Where a game's saves are kept: the platform's **data** directory natively,
/// the installed Origin Private File System store in a browser, or nowhere.
///
/// The persistence rules in `docs/notes/simulation.md` put saves in the data
/// directory and records in the config directory, so this is not
/// [`Backing::platform`](crate::record::Backing::platform), which answers with
/// the config directory and hands out a path. This hands out the
/// [`StorageSource`] a [`SaveWriter`] writes through and a [`SaveReader`]
/// reads from. `apps/shard` wrote the arm first and `apps/towers` is the
/// second game that needed it, which is why it is here rather than written
/// out again.
///
/// [`SaveBacking::None`] is a state a caller *chooses* rather than a failure:
/// a headless run must leave nothing behind, so a test suite and CI never
/// write into whoever's data directory.
#[derive(Debug)]
pub enum SaveBacking {
    /// Kept nowhere: every write is refused and every read finds nothing.
    None,
    /// A directory on a real filesystem.
    #[cfg(not(target_arch = "wasm32"))]
    Native(crate::NativeStorage),
    /// The store the page's shim restored the Origin Private File System into.
    #[cfg(target_arch = "wasm32")]
    Browser(std::rc::Rc<crate::web::OpfsStorage>),
}

impl SaveBacking {
    /// The platform's own place for `app_name`'s saves, or [`SaveBacking::None`]
    /// with a warning when the platform will not give one — no data directory,
    /// no OPFS store installed. That is the ordinary no-shim case rather than
    /// something a caller can act on.
    ///
    /// `app_name` names the directory natively and means nothing in a
    /// browser, where the origin already is the namespace; it stays in the
    /// signature so no caller writes a `#[cfg]` of its own.
    #[must_use]
    pub fn platform(app_name: &str) -> Self {
        #[cfg(not(target_arch = "wasm32"))]
        {
            match crate::NativeStorage::data(app_name) {
                Ok(store) => Self::Native(store),
                Err(error) => {
                    crcbl_core::log::warn!(
                        "save: no data dir ({error}); {app_name}'s saves will not persist"
                    );
                    Self::None
                }
            }
        }
        #[cfg(target_arch = "wasm32")]
        {
            match crate::web::opfs::installed() {
                Some(store) => Self::Browser(store),
                None => {
                    crcbl_core::log::warn!(
                        "save: no OPFS store installed; {app_name}'s saves will not persist"
                    );
                    Self::None
                }
            }
        }
    }

    /// The backend to write and read through, or `None` for saves kept
    /// nowhere.
    #[must_use]
    pub fn source(&self) -> Option<&dyn StorageSource> {
        match self {
            Self::None => None,
            #[cfg(not(target_arch = "wasm32"))]
            Self::Native(store) => Some(store),
            #[cfg(target_arch = "wasm32")]
            Self::Browser(store) => Some(&**store),
        }
    }

    /// Where saves go, in the words a debug panel uses.
    #[must_use]
    pub const fn label(&self) -> &'static str {
        match self {
            Self::None => "nowhere",
            #[cfg(not(target_arch = "wasm32"))]
            Self::Native(_) => "data dir",
            #[cfg(target_arch = "wasm32")]
            Self::Browser(_) => "opfs",
        }
    }
}

// ── Tests ──────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;
    use crate::MemoryStorage;

    fn make_sample_save() -> SaveWriter {
        let header = SaveHeader::new(TickId::from_raw(42), 120.5);
        let mut writer = SaveWriter::new(header);
        writer.add_sector(SectorSave {
            sector_id: SectorId::ZERO,
            snapshot_data: vec![1, 2, 3, 4],
        });
        writer
    }

    #[test]
    fn a_save_reads_back_with_its_header_its_sector_and_a_valid_checksum() {
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
    fn a_save_with_several_sectors_reads_them_back_in_order_with_their_own_data() {
        let storage = MemoryStorage::new();
        let header = SaveHeader::new(TickId::from_raw(100), 0.0);
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
    fn a_corrupt_save_is_refused_by_open_and_flagged_by_the_salvage_path() {
        let storage = MemoryStorage::new();
        let writer = make_sample_save();
        let path = Path::new("test.crb");

        writer.write(&storage, path).unwrap();

        // Corrupt the file in-place.
        let mut data = storage.read(path).unwrap();
        data[10] ^= 0xFF; // flip a bit in the header
        storage.write(path, &data).unwrap();

        // `open` refuses a corrupt file — the module doc promises detection.
        let err = SaveReader::open(&storage, path).unwrap_err();
        assert!(err.to_string().contains("checksum mismatch"), "{err}");

        // The salvage path still parses it, and reports the mismatch.
        let reader = SaveReader::open_ignoring_checksum(&storage, path).unwrap();
        assert!(!reader.data().checksum_valid);
    }

    #[test]
    fn checksum_is_real_sha256_of_the_body() {
        let storage = MemoryStorage::new();
        let writer = make_sample_save();
        let path = Path::new("sha.crb");
        writer.write(&storage, path).unwrap();

        let bytes = storage.read(path).unwrap();
        let (body, checksum) = bytes.split_at(bytes.len() - CHECKSUM_SIZE);
        assert_eq!(checksum, crcbl_shaders::sha256::sha256(body));
    }

    #[test]
    fn absurd_sector_count_rejected_without_allocating() {
        let storage = MemoryStorage::new();
        // A minimum-size file claiming u32::MAX sectors: the old code fed that
        // straight into `Vec::with_capacity` and aborted the process.
        let data = sealed(body_with(&[0], &[NO_SCENE], u32::MAX, &[]));
        storage.write(Path::new("huge.crb"), &data).unwrap();

        let err = SaveReader::open(&storage, Path::new("huge.crb")).unwrap_err();
        assert!(err.to_string().contains("bytes follow the header"), "{err}");
    }

    #[test]
    fn sector_data_length_beyond_file_rejected() {
        let storage = MemoryStorage::new();
        // One sector whose data_len is u32::MAX.
        let mut entry = vec![0u8; MIN_SECTOR_ENTRY_SIZE];
        entry[SECTOR_ID_SIZE..].copy_from_slice(&u32::MAX.to_le_bytes());
        let data = sealed(body_with(&[0], &[NO_SCENE], 1, &entry));
        storage.write(Path::new("len.crb"), &data).unwrap();

        let err = SaveReader::open(&storage, Path::new("len.crb")).unwrap_err();
        assert!(
            err.to_string().contains("truncated in sector data"),
            "{err}"
        );
    }

    #[test]
    fn a_file_with_the_wrong_magic_is_refused_as_an_invalid_save() {
        let storage = MemoryStorage::new();
        // Must be >= MIN_SAVE_SIZE (a preamble and a checksum) to pass the
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
    fn a_file_shorter_than_the_save_header_is_refused() {
        let storage = MemoryStorage::new();
        storage.write(Path::new("short.crb"), b"too short").unwrap();

        let result = SaveReader::open(&storage, Path::new("short.crb"));
        assert!(result.is_err());
    }

    /// The body of a current-version save at tick zero: `engine` (its length
    /// byte and its text), `scene` (its marker and what follows it),
    /// `sector_count`, then `tail`.
    pub(super) fn body_with(
        engine: &[u8],
        scene: &[u8],
        sector_count: u32,
        tail: &[u8],
    ) -> Vec<u8> {
        let mut body = Vec::new();
        body.extend_from_slice(SAVE_MAGIC);
        body.extend_from_slice(&SAVE_FORMAT_VERSION.to_le_bytes());
        body.resize(FIXED_HEADER_SIZE, 0);
        body.extend_from_slice(engine);
        body.extend_from_slice(scene);
        body.extend_from_slice(&sector_count.to_le_bytes());
        body.extend_from_slice(tail);
        body
    }

    /// `body` with its checksum after it: a whole file.
    pub(super) fn sealed(mut body: Vec<u8>) -> Vec<u8> {
        let checksum = SaveWriter::checksum(&body);
        body.extend_from_slice(&checksum);
        body
    }

    /// `writer`'s file, as it writes it.
    pub(super) fn written(writer: &SaveWriter) -> Result<Vec<u8>, StorageError> {
        let storage = MemoryStorage::new();
        let path = Path::new("written.crb");
        writer.write(&storage, path)?;
        Ok(storage.read(path).expect("the file just written"))
    }

    /// A scene reference with a hash no two bytes of which are alike, so a
    /// hash read back shifted or reversed is not the one written.
    fn arena() -> SceneRef {
        SceneRef {
            name: "scenes/arena.scn".to_owned(),
            content_hash: std::array::from_fn(|i| i as u8 + 1),
        }
    }

    /// **A header with an engine version and a scene reads back exactly**,
    /// at the current format version, with its sectors after it.
    #[test]
    fn a_header_with_an_engine_version_and_a_scene_reads_back_exactly() {
        let header = SaveHeader {
            tick: TickId::from_raw(77),
            playtime_secs: 3.5,
            engine_version: Some("9.8.7-test".to_owned()),
            scene: Some(arena()),
        };
        let mut writer = SaveWriter::new(header.clone());
        writer.add_sector(SectorSave {
            sector_id: SectorId { x: 5, y: -6, z: 7 },
            snapshot_data: vec![9, 8, 7],
        });
        let bytes = written(&writer).expect("a valid header");
        assert_eq!(version_of(&bytes), Some(SAVE_FORMAT_VERSION));

        let read = decode(&bytes).expect("the save this build just wrote");
        assert_eq!(read.header, header);
        assert_eq!(read.format_version, SAVE_FORMAT_VERSION);
        assert!(read.checksum_valid);
        assert_eq!(read.sectors.len(), 1);
        assert_eq!(read.sectors[0].sector_id, SectorId { x: 5, y: -6, z: 7 });
        assert_eq!(read.sectors[0].snapshot_data, [9, 8, 7]);
    }

    /// **A new header records this engine's version and no scene**, and a
    /// header that records no version reads back as recording none.
    #[test]
    fn a_new_header_records_this_engine_and_a_bare_one_reads_back_bare() {
        let header = SaveHeader::new(TickId::from_raw(1), 0.0);
        assert_eq!(
            header.engine_version.as_deref(),
            Some(env!("CARGO_PKG_VERSION"))
        );
        assert_eq!(header.scene, None);
        let read = decode(&written(&SaveWriter::new(header.clone())).expect("a valid header"))
            .expect("the save this build just wrote");
        assert_eq!(read.header, header);

        let bare = SaveHeader {
            engine_version: None,
            ..header
        };
        let read = decode(&written(&SaveWriter::new(bare.clone())).expect("a valid header"))
            .expect("the save this build just wrote");
        assert_eq!(read.header, bare);
    }

    /// **The writer refuses a header the reader would refuse or read back
    /// differently**: an empty or overlong engine version, an empty or
    /// overlong scene name. Each field at its limit is written.
    #[test]
    fn the_writer_refuses_a_header_the_reader_would_not_read_back() {
        let refused = |header: SaveHeader| match written(&SaveWriter::new(header)) {
            Err(StorageError::Save(error)) => error,
            other => panic!("the header was not refused by name: {other:?}"),
        };
        let with_engine = |text: String| SaveHeader {
            engine_version: Some(text),
            ..SaveHeader::new(TickId::from_raw(1), 0.0)
        };
        let with_scene = |name: String| SaveHeader {
            scene: Some(SceneRef { name, ..arena() }),
            ..SaveHeader::new(TickId::from_raw(1), 0.0)
        };

        assert_eq!(
            refused(with_engine(String::new())),
            FormatError::FieldEmpty("engine version")
        );
        assert_eq!(
            refused(with_engine("v".repeat(ENGINE_VERSION_MAX + 1))),
            FormatError::FieldTooLong {
                field: "engine version",
                len: ENGINE_VERSION_MAX + 1,
                limit: ENGINE_VERSION_MAX,
            }
        );
        assert_eq!(
            refused(with_scene(String::new())),
            FormatError::FieldEmpty("scene name")
        );
        assert_eq!(
            refused(with_scene("s".repeat(SCENE_NAME_MAX + 1))),
            FormatError::FieldTooLong {
                field: "scene name",
                len: SCENE_NAME_MAX + 1,
                limit: SCENE_NAME_MAX,
            }
        );

        for header in [
            with_engine("v".repeat(ENGINE_VERSION_MAX)),
            with_scene("s".repeat(SCENE_NAME_MAX)),
        ] {
            let bytes = written(&SaveWriter::new(header.clone())).expect("a field at its limit");
            assert_eq!(decode(&bytes).expect("and read back").header, header);
        }
    }

    /// **The reader holds the header to its bounds**: a scene marker that is
    /// neither, a scene name empty, past its limit or past the file, an engine
    /// version past the file or not UTF-8, and a byte after the last sector
    /// are each refused by name.
    #[test]
    fn the_reader_holds_the_header_to_its_bounds() {
        let refused =
            |body: Vec<u8>| decode(&sealed(body)).expect_err("a malformed header was read");
        let scene_named = |len: u16, name: &[u8]| {
            let mut scene = vec![SCENE];
            scene.extend_from_slice(&len.to_le_bytes());
            scene.extend_from_slice(name);
            scene.extend_from_slice(&arena().content_hash);
            scene
        };
        let too_long = u16::try_from(SCENE_NAME_MAX + 1).expect("the limit fits the field");

        assert_eq!(
            refused(body_with(&[0], &[2], 0, &[])),
            FormatError::SceneMarker(2)
        );
        assert_eq!(
            refused(body_with(&[0], &scene_named(0, &[]), 0, &[])),
            FormatError::FieldEmpty("scene name")
        );
        assert_eq!(
            refused(body_with(
                &[0],
                &scene_named(too_long, &vec![b's'; SCENE_NAME_MAX + 1]),
                0,
                &[]
            )),
            FormatError::FieldTooLong {
                field: "scene name",
                len: SCENE_NAME_MAX + 1,
                limit: SCENE_NAME_MAX,
            }
        );
        // A name length the bytes before the checksum cannot hold.
        let mut cut = body_with(&[0], &[SCENE], 0, &[]);
        cut.truncate(FIXED_HEADER_SIZE + 2);
        cut.extend_from_slice(&64u16.to_le_bytes());
        cut.extend_from_slice(b"scenes");
        assert_eq!(refused(cut), FormatError::Truncated("scene name"));
        // An engine version longer than everything after it.
        let mut cut = body_with(&[], &[], 0, &[]);
        cut.truncate(FIXED_HEADER_SIZE);
        cut.extend_from_slice(&[200, b'1', b'.']);
        assert_eq!(refused(cut), FormatError::Truncated("engine version"));
        assert_eq!(
            refused(body_with(&[2, 0xFF, 0xFE], &[NO_SCENE], 0, &[])),
            FormatError::NotUtf8("engine version")
        );
        assert_eq!(
            refused(body_with(&[0], &[NO_SCENE], 0, &[0])),
            FormatError::TrailingBytes(1)
        );

        // …and the same shapes, well formed, are read.
        let read = decode(&sealed(body_with(
            &[3, b'1', b'.', b'2'],
            &scene_named(6, b"scenes"),
            0,
            &[],
        )))
        .expect("a well-formed header");
        assert_eq!(read.header.engine_version.as_deref(), Some("1.2"));
        assert_eq!(
            read.header.scene.map(|scene| scene.name).as_deref(),
            Some("scenes")
        );
    }

    #[test]
    fn autosave_ring_writes_rotating_files() {
        let storage = MemoryStorage::new();
        let header = SaveHeader::new(TickId::from_raw(1), 0.0);
        let writer = SaveWriter::new(header);

        let mut ring = AutosaveRing::new(3, "autosave_{}.crb").unwrap();

        // Write 5 times → should wrap around.
        for _ in 0..5 {
            ring.write(&storage, &writer).unwrap();
        }

        // After 5 writes on a ring of 3 the next slot is 2, so slot 2 holds
        // the oldest surviving save and slot 1 the newest.
        let files = ring.list(&storage);
        assert_eq!(
            files,
            vec![
                "autosave_2.crb".to_string(),
                "autosave_0.crb".to_string(),
                "autosave_1.crb".to_string(),
            ]
        );
    }

    #[test]
    fn autosave_ring_lists_oldest_first_before_wrapping() {
        let storage = MemoryStorage::new();
        let writer = SaveWriter::new(SaveHeader::new(TickId::from_raw(1), 0.0));

        let mut ring = AutosaveRing::new(4, "part_{}.crb").unwrap();
        for _ in 0..2 {
            ring.write(&storage, &writer).unwrap();
        }

        assert_eq!(
            ring.list(&storage),
            vec!["part_0.crb".to_string(), "part_1.crb".to_string()]
        );
    }

    #[test]
    fn autosave_ring_rejects_template_without_placeholder() {
        let err = AutosaveRing::new(3, "autosave.crb").unwrap_err();
        assert!(err.to_string().contains("slot placeholder"), "{err}");
    }

    #[test]
    fn autosave_ring_capacity_at_least_one() {
        let ring = AutosaveRing::new(0, "save_{}.crb").unwrap();
        assert_eq!(ring.capacity(), 1);
    }

    /// **Saves kept nowhere are refused and read as nothing**, and a native
    /// backing writes into the directory it was handed — so a caller choosing
    /// [`SaveBacking::None`] for a headless run really leaves no trace.
    #[cfg(not(target_arch = "wasm32"))]
    #[test]
    fn a_save_backing_writes_where_it_says_and_nowhere_writes_nothing() {
        assert!(SaveBacking::None.source().is_none());
        assert_eq!(SaveBacking::None.label(), "nowhere");

        let dir = tempfile::tempdir().expect("a scratch directory");
        let backing = SaveBacking::Native(crate::NativeStorage::at(dir.path().to_path_buf()));
        assert_eq!(backing.label(), "data dir");
        let source = backing.source().expect("a native backing has a source");
        make_sample_save()
            .write(source, Path::new("game.crb"))
            .expect("the scratch directory is writable");
        assert!(dir.path().join("game.crb").is_file());
        assert!(SaveReader::open(source, Path::new("game.crb")).is_ok());
    }

    #[test]
    fn autosave_ring_slot_advances() {
        let storage = MemoryStorage::new();
        let header = SaveHeader::new(TickId::from_raw(1), 0.0);
        let writer = SaveWriter::new(header);

        let mut ring = AutosaveRing::new(2, "ring_{}.crb").unwrap();
        assert_eq!(ring.slot(), 0);

        ring.write(&storage, &writer).unwrap();
        assert_eq!(ring.slot(), 1);

        ring.write(&storage, &writer).unwrap();
        assert_eq!(ring.slot(), 0); // wrapped
    }
}
