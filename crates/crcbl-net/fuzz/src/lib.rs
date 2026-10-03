//! What `fuzz_targets/decoder.rs` and `tests/corpus.rs` both run.
//!
//! Most of the target's calls are one line on the bytes, and the corpus test
//! makes that line again itself. A save file is not: `crcbl_store::save`'s
//! reader reads through a [`StorageSource`] and a path, so the bytes have to be
//! put somewhere first. That scaffolding lives here once, so the seeds the
//! test names go through exactly the calls the fuzzer makes — take the reader
//! out of [`open_save`] and the seed test fails, rather than a copy of it in
//! the test passing on.

use std::path::Path;

use crcbl_store::save::SaveReader;
use crcbl_store::{MemoryStorage, StorageError, StorageSource};

/// Where [`open_save`] puts the bytes. Any name does; a [`MemoryStorage`]
/// reads nothing else.
const SAVE_PATH: &str = "fuzz.crb";

/// One save file read both ways a game can read it.
#[derive(Debug)]
pub struct OpenedSave {
    /// [`SaveReader::open`]: every check, the checksum included — the read a
    /// game resumes from.
    pub checked: Result<SaveReader, StorageError>,
    /// [`SaveReader::open_ignoring_checksum`]: the salvage read of a damaged
    /// file. The fuzzer cannot forge a SHA-256, so this is the call that
    /// carries its bytes through the header and every sector entry whatever
    /// the checksum says.
    pub salvaged: Result<SaveReader, StorageError>,
}

/// `bytes` as a save file someone handed over, read through `crcbl-store`'s
/// container reader.
///
/// # Panics
///
/// Never on the bytes: a [`MemoryStorage`] takes every write, and a panic in
/// the reader is the finding this exists to make.
#[must_use]
pub fn open_save(bytes: &[u8]) -> OpenedSave {
    let storage = MemoryStorage::new();
    let path = Path::new(SAVE_PATH);
    storage
        .write(path, bytes)
        .expect("a memory storage takes every write");
    OpenedSave {
        checked: SaveReader::open(&storage, path),
        salvaged: SaveReader::open_ignoring_checksum(&storage, path),
    }
}
