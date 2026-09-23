//! A file kept in a cloud, with conflicts surfaced to the game rather than
//! settled behind its back.
//!
//! A cloud that syncs whole files between devices — Steam Cloud is the one
//! this was written for — can hold only one version of a file. When two
//! devices each change it before either sees the other's change, one change
//! is lost unless something notices. This module notices, over any pair of
//! [`StorageSource`]s: the **cloud**, which every device shares, and a
//! **shadow**, which only this device sees (under
//! [`NativeStorage::data`](crate::NativeStorage::data), never in the cloud).
//!
//! # The file
//!
//! Each version in the cloud is a fixed header and then the payload, which is
//! opaque bytes to this module:
//!
//! | Bytes | Field             | Meaning                                         |
//! | ----- | ----------------- | ----------------------------------------------- |
//! | 4     | magic             | `CRSF`                                          |
//! | 4     | format            | [`FORMAT`], little-endian like every field      |
//! | 8     | `generation`      | one more than the highest version it replaced   |
//! | 8     | `base_generation` | the version it was written on top of (0: none)  |
//! | 4     | `base_crc`        | that version's CRC, so the base is unambiguous  |
//! | 8     | payload length    |                                                 |
//! | 4     | CRC               | [`crc32`] over every byte before it and the payload |
//!
//! A version is named by its generation and CRC together: generations alone
//! repeat, since two devices that start from the same version both write its
//! generation plus one. There is no writer id: two devices that write the
//! same payload on top of the same version wrote the same version, and one
//! that told them apart would turn that into a conflict with nothing in it.
//!
//! # The shadow
//!
//! Beside each synced file the shadow keeps the version this device last saw
//! in the cloud, and — until the cloud is seen to hold it — a copy of this
//! device's own latest write. A save writes that copy first and the cloud
//! second, so a write the cloud never received is still on the device.
//!
//! # Load
//!
//! | The cloud holds                            | This device's unconfirmed write | Outcome          |
//! | ------------------------------------------ | ------------------------------- | ---------------- |
//! | nothing                                    | none                            | `Missing`        |
//! | nothing, or the version the write replaced | some                            | `Clean`, written again |
//! | that write                                 | some                            | `Clean`          |
//! | a version written on top of it             | some                            | `FastForwarded`  |
//! | anything else                              | some                            | `Conflict`       |
//! | the version last seen                      | none                            | `Clean`          |
//! | any other version                          | none                            | `FastForwarded`  |
//!
//! A conflict is handed to the game with both payloads, and
//! [`SyncedFile::resolve`] writes the game's choice as a version above both.
//! Nothing is ever merged or partly applied. A cloud file that does not parse
//! is [`SyncError::Corrupt`] — never read as empty, which would let the next
//! save overwrite a file that may be recoverable.

use std::path::{Path, PathBuf};

use crate::crc32::{crc32, crc32_continue};
use crate::{StorageError, StorageSource};

/// The synced-file format this build writes and reads.
pub const FORMAT: u32 = 1;

/// A synced file's first four bytes.
const MAGIC: [u8; 4] = *b"CRSF";
/// The header's size: every field in the table above.
const HEADER_BYTES: usize = 4 + 4 + 8 + 8 + 4 + 8 + 4;
/// Where the CRC field starts: it covers everything before it.
const CRC_AT: usize = HEADER_BYTES - 4;

/// A shadow record's first four bytes.
const SHADOW_MAGIC: [u8; 4] = *b"CRSS";
/// A shadow record: magic, format, the last-seen version's generation and
/// CRC.
const SHADOW_BYTES: usize = 4 + 4 + 8 + 4;

/// What [`SyncedFile::load`] found.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SyncOutcome {
    /// The cloud holds what this device last saw or wrote.
    Clean(Vec<u8>),
    /// The cloud moved on while this device changed nothing: take its
    /// payload.
    FastForwarded(Vec<u8>),
    /// Neither the cloud nor this device has the file.
    Missing,
    /// This device and another each changed the file without seeing the
    /// other's change. The game picks one with [`SyncedFile::resolve`];
    /// until it does, [`SyncedFile::save`] refuses.
    Conflict {
        /// This device's unsynced payload.
        local: Vec<u8>,
        /// The payload the cloud holds.
        remote: Vec<u8>,
    },
}

/// Which side of a [`SyncOutcome::Conflict`] survives.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Resolution {
    /// This device's payload.
    KeepLocal,
    /// The cloud's payload.
    KeepRemote,
}

/// Why a synced file could not be read.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Corruption {
    /// Shorter than its header.
    ShortHeader,
    /// Not a synced file at all: the magic is wrong.
    NotSynced,
    /// A format this build does not read.
    Format(u32),
    /// The header's payload length and the bytes present disagree — a
    /// truncated or extended file.
    Length {
        /// The length the header declares.
        declared: u64,
        /// The bytes after the header.
        present: u64,
    },
    /// The CRC does not match: the header or the payload was damaged.
    Checksum,
}

/// A synced-file failure.
#[derive(Debug, thiserror::Error)]
#[non_exhaustive]
pub enum SyncError {
    /// The cloud or the shadow failed.
    #[error(transparent)]
    Storage(#[from] StorageError),
    /// A file that must parse did not.
    #[error("{path}: corrupt synced file: {corruption:?}")]
    Corrupt {
        /// Which file, in the storage it was read from.
        path: PathBuf,
        /// What was wrong with it.
        corruption: Corruption,
    },
    /// [`SyncedFile::save`] before any [`SyncedFile::load`], or after one
    /// that failed: a save must know what the cloud holds, or it overwrites a
    /// version it never saw.
    #[error("a synced file must be loaded before it is saved")]
    NotLoaded,
    /// [`SyncedFile::save`] while a conflict from the last load is
    /// unresolved.
    #[error("the last load found a conflict; resolve it before saving")]
    Unresolved,
    /// [`SyncedFile::resolve`] with no conflict to resolve.
    #[error("there is no conflict to resolve")]
    NoConflict,
}

/// A version's identity: generation and CRC together.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct Version {
    generation: u64,
    crc: u32,
}

/// One parsed synced file.
#[derive(Debug, Clone)]
struct Parsed {
    version: Version,
    /// `None` for a first version.
    base: Option<Version>,
    /// The whole file, header included, as it was read.
    bytes: Vec<u8>,
}

impl Parsed {
    fn payload(&self) -> &[u8] {
        &self.bytes[HEADER_BYTES..]
    }

    /// Builds a version of `payload`.
    fn build(generation: u64, base: Option<Version>, payload: &[u8]) -> Self {
        let base_version = base.unwrap_or(Version {
            generation: 0,
            crc: 0,
        });
        let mut bytes = Vec::with_capacity(HEADER_BYTES + payload.len());
        bytes.extend_from_slice(&MAGIC);
        bytes.extend_from_slice(&FORMAT.to_le_bytes());
        bytes.extend_from_slice(&generation.to_le_bytes());
        bytes.extend_from_slice(&base_version.generation.to_le_bytes());
        bytes.extend_from_slice(&base_version.crc.to_le_bytes());
        bytes.extend_from_slice(&(payload.len() as u64).to_le_bytes());
        let crc = checksum(&bytes, payload);
        bytes.extend_from_slice(&crc.to_le_bytes());
        bytes.extend_from_slice(payload);
        Self {
            version: Version { generation, crc },
            base,
            bytes,
        }
    }

    /// Parses a whole file.
    fn parse(bytes: Vec<u8>) -> Result<Self, Corruption> {
        let header = bytes.get(..HEADER_BYTES).ok_or(Corruption::ShortHeader)?;
        if header[..4] != MAGIC {
            return Err(Corruption::NotSynced);
        }
        let format = u32::from_le_bytes(field(header, 4));
        if format != FORMAT {
            return Err(Corruption::Format(format));
        }
        let generation = u64::from_le_bytes(field(header, 8));
        let base_generation = u64::from_le_bytes(field(header, 16));
        let base_crc = u32::from_le_bytes(field(header, 24));
        let declared = u64::from_le_bytes(field(header, 28));
        let crc = u32::from_le_bytes(field(header, CRC_AT));
        let present = (bytes.len() - HEADER_BYTES) as u64;
        if declared != present {
            return Err(Corruption::Length { declared, present });
        }
        let computed = checksum(&header[..CRC_AT], &bytes[HEADER_BYTES..]);
        if computed != crc {
            return Err(Corruption::Checksum);
        }
        Ok(Self {
            version: Version { generation, crc },
            base: (base_generation != 0).then_some(Version {
                generation: base_generation,
                crc: base_crc,
            }),
            bytes,
        })
    }
}

/// A version's CRC: over the header fields before the CRC, then the payload
/// — so a damaged generation or base is caught as surely as a damaged
/// payload.
fn checksum(header: &[u8], payload: &[u8]) -> u32 {
    crc32_continue(crc32(header), payload)
}

/// `N` bytes of `bytes` from `at`, which the caller has bounds-checked.
fn field<const N: usize>(bytes: &[u8], at: usize) -> [u8; N] {
    let mut out = [0; N];
    out.copy_from_slice(&bytes[at..at + N]);
    out
}

/// A conflict the last load found, kept for [`SyncedFile::resolve`].
#[derive(Debug, Clone)]
struct Conflict {
    local: Parsed,
    remote: Parsed,
}

/// One file kept in a cloud; see the [module docs](self).
#[derive(Debug)]
pub struct SyncedFile {
    cloud: Box<dyn StorageSource>,
    shadow: Box<dyn StorageSource>,
    path: PathBuf,
    loaded: bool,
    conflict: Option<Conflict>,
}

impl SyncedFile {
    /// The file at `path` in `cloud`, tracked through `shadow` — which must
    /// be storage only this device sees, never the cloud's own.
    pub fn new(
        cloud: Box<dyn StorageSource>,
        shadow: Box<dyn StorageSource>,
        path: impl Into<PathBuf>,
    ) -> Self {
        Self {
            cloud,
            shadow,
            path: path.into(),
            loaded: false,
            conflict: None,
        }
    }

    /// Reads the cloud's version and classifies it against what this device
    /// last saw and wrote (the table in the [module docs](self)).
    ///
    /// # Errors
    ///
    /// [`SyncError::Corrupt`] for a cloud file, a shadow record or a kept
    /// write that does not parse; [`SyncError::Storage`] when either storage
    /// fails — a cloud read that is [`StorageError::NotFound`] is `Missing`
    /// or a re-sent write, not an error.
    pub fn load(&mut self) -> Result<SyncOutcome, SyncError> {
        // Until this load succeeds, what the cloud holds is unknown: neither
        // an earlier load's conflict nor its permission to save still stands.
        self.conflict = None;
        self.loaded = false;
        let pending = self.read_pending()?;
        let seen = self.read_seen()?;
        let cloud = match self.cloud.read(&self.path) {
            Ok(bytes) => Some(parse(&self.path, bytes)?),
            Err(StorageError::NotFound(_)) => None,
            Err(error) => return Err(error.into()),
        };
        let outcome = match (cloud, pending) {
            (None, None) => SyncOutcome::Missing,
            // The cloud never received this device's write, and nobody else
            // wrote since: send it again.
            (None, Some(mine)) => self.resend(&mine)?,
            (Some(cloud), Some(mine)) if Some(cloud.version) == mine.base => self.resend(&mine)?,
            (Some(cloud), Some(mine)) if cloud.version == mine.version => {
                self.confirm(cloud.version)?;
                SyncOutcome::Clean(cloud.payload().to_vec())
            }
            (Some(cloud), Some(mine)) if cloud.base == Some(mine.version) => {
                self.confirm(cloud.version)?;
                SyncOutcome::FastForwarded(cloud.payload().to_vec())
            }
            (Some(cloud), Some(mine)) => {
                let outcome = SyncOutcome::Conflict {
                    local: mine.payload().to_vec(),
                    remote: cloud.payload().to_vec(),
                };
                self.conflict = Some(Conflict {
                    local: mine,
                    remote: cloud,
                });
                outcome
            }
            (Some(cloud), None) => {
                let payload = cloud.payload().to_vec();
                if seen == Some(cloud.version) {
                    SyncOutcome::Clean(payload)
                } else {
                    self.confirm(cloud.version)?;
                    SyncOutcome::FastForwarded(payload)
                }
            }
        };
        self.loaded = true;
        Ok(outcome)
    }

    /// Writes `payload` as a new version on top of this device's latest —
    /// its unconfirmed write if it has one, else the version it last saw in
    /// the cloud: kept in the shadow first, then written to the cloud.
    ///
    /// # Errors
    ///
    /// [`SyncError::NotLoaded`] unless the last [`load`](Self::load)
    /// succeeded;
    /// [`SyncError::Unresolved`] while the last load's conflict stands;
    /// otherwise as [`load`](Self::load).
    pub fn save(&mut self, payload: &[u8]) -> Result<(), SyncError> {
        if !self.loaded {
            return Err(SyncError::NotLoaded);
        }
        if self.conflict.is_some() {
            return Err(SyncError::Unresolved);
        }
        // An unconfirmed write is always above the version it was built on,
        // so the base is also the highest version this device knows.
        let base = match self.read_pending()? {
            Some(mine) => Some(mine.version),
            None => self.read_seen()?,
        };
        let generation = base.map_or(0, |base| base.generation) + 1;
        let version = Parsed::build(generation, base, payload);
        self.write_version(&version)
    }

    /// Settles the last load's conflict with the game's choice, written as a
    /// version above both sides and on top of the cloud's. Returns the
    /// payload kept; the next load reads it as `Clean`.
    ///
    /// # Errors
    ///
    /// [`SyncError::NoConflict`] unless the last load was a conflict;
    /// otherwise as [`save`](Self::save).
    pub fn resolve(&mut self, choice: Resolution) -> Result<Vec<u8>, SyncError> {
        let Some(conflict) = self.conflict.take() else {
            return Err(SyncError::NoConflict);
        };
        let kept = match choice {
            Resolution::KeepLocal => &conflict.local,
            Resolution::KeepRemote => &conflict.remote,
        };
        let payload = kept.payload().to_vec();
        let generation = conflict
            .local
            .version
            .generation
            .max(conflict.remote.version.generation)
            + 1;
        let version = Parsed::build(generation, Some(conflict.remote.version), &payload);
        // Written on top of the cloud's version, which this device has now
        // seen.
        if let Err(error) = self
            .write_seen(conflict.remote.version)
            .and_then(|()| self.write_version(&version))
        {
            self.conflict = Some(conflict);
            return Err(error);
        }
        Ok(payload)
    }

    /// Writes this device's kept write to the cloud again.
    fn resend(&self, mine: &Parsed) -> Result<SyncOutcome, SyncError> {
        self.cloud.write(&self.path, &mine.bytes)?;
        Ok(SyncOutcome::Clean(mine.payload().to_vec()))
    }

    /// Keeps `version` in the shadow, then writes it to the cloud.
    fn write_version(&self, version: &Parsed) -> Result<(), SyncError> {
        self.shadow.write(&self.pending_path(), &version.bytes)?;
        self.cloud.write(&self.path, &version.bytes)?;
        Ok(())
    }

    /// Records `version` as seen in the cloud, and drops this device's kept
    /// write, which it contains or supersedes.
    fn confirm(&self, version: Version) -> Result<(), SyncError> {
        self.write_seen(version)?;
        match self.shadow.delete(&self.pending_path()) {
            Ok(()) | Err(StorageError::NotFound(_)) => Ok(()),
            Err(error) => Err(error.into()),
        }
    }

    fn write_seen(&self, version: Version) -> Result<(), SyncError> {
        let mut record = Vec::with_capacity(SHADOW_BYTES);
        record.extend_from_slice(&SHADOW_MAGIC);
        record.extend_from_slice(&FORMAT.to_le_bytes());
        record.extend_from_slice(&version.generation.to_le_bytes());
        record.extend_from_slice(&version.crc.to_le_bytes());
        self.shadow.write(&self.seen_path(), &record)?;
        Ok(())
    }

    /// The version this device last saw in the cloud, if any.
    fn read_seen(&self) -> Result<Option<Version>, SyncError> {
        let path = self.seen_path();
        let Some(record) = read_optional(&*self.shadow, &path)? else {
            return Ok(None);
        };
        let corrupt = |corruption| SyncError::Corrupt {
            path: path.clone(),
            corruption,
        };
        if record.len() != SHADOW_BYTES {
            return Err(corrupt(Corruption::Length {
                declared: SHADOW_BYTES as u64,
                present: record.len() as u64,
            }));
        }
        if record[..4] != SHADOW_MAGIC {
            return Err(corrupt(Corruption::NotSynced));
        }
        let format = u32::from_le_bytes(field(&record, 4));
        if format != FORMAT {
            return Err(corrupt(Corruption::Format(format)));
        }
        Ok(Some(Version {
            generation: u64::from_le_bytes(field(&record, 8)),
            crc: u32::from_le_bytes(field(&record, 16)),
        }))
    }

    /// This device's write the cloud has not been seen to hold, if any.
    fn read_pending(&self) -> Result<Option<Parsed>, SyncError> {
        let path = self.pending_path();
        read_optional(&*self.shadow, &path)?
            .map(|bytes| parse(&path, bytes))
            .transpose()
    }

    fn seen_path(&self) -> PathBuf {
        sibling(&self.path, "seen")
    }

    fn pending_path(&self) -> PathBuf {
        sibling(&self.path, "pending")
    }
}

/// `path` with `.suffix` appended to its file name.
fn sibling(path: &Path, suffix: &str) -> PathBuf {
    let mut name = path.as_os_str().to_owned();
    name.push(".");
    name.push(suffix);
    PathBuf::from(name)
}

/// A read where a missing file is `None`.
fn read_optional(storage: &dyn StorageSource, path: &Path) -> Result<Option<Vec<u8>>, SyncError> {
    match storage.read(path) {
        Ok(bytes) => Ok(Some(bytes)),
        Err(StorageError::NotFound(_)) => Ok(None),
        Err(error) => Err(error.into()),
    }
}

fn parse(path: &Path, bytes: Vec<u8>) -> Result<Parsed, SyncError> {
    Parsed::parse(bytes).map_err(|corruption| SyncError::Corrupt {
        path: path.to_path_buf(),
        corruption,
    })
}

#[cfg(test)]
mod tests;
