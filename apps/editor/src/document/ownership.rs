//! Which files in a scene directory a save may remove, and where it may write.
//!
//! # A save removes exactly what the scene stopped owning
//!
//! A document opened from a directory owns the files the manifest there named
//! — `scene.ron`, `env.ron`, each listed system's `sys/<name>.ron`, and
//! `names.ron` when the header declares it — and each save into that directory
//! owns what it wrote. A save there then removes the owned files it did not
//! write this time: a system unlisted, or the last name cleared. That set is
//! computed from the scene's own writer on both sides, never from a listing of
//! the directory, so nothing else in it is ever a candidate: a person's notes
//! beside the scene, an asset, a `sys/` chunk no manifest of this document
//! named. `scene.ron` and `env.ron` are written by every save, so only chunks
//! ever fall out.
//!
//! The removal runs **after every file has been written**. A save that fails
//! part way has removed nothing, and still owns what it did write, so the next
//! save that succeeds removes whatever that one left behind.
//!
//! # A save anywhere else is a copy, and overwrites nothing
//!
//! Into a directory other than the document's own, a save removes nothing —
//! it owns nothing there — and **refuses before writing anything if a file it
//! would write is already there** ([`EditError::Occupied`]). Merging would
//! overwrite another scene's `scene.ron` and leave that scene's chunks orphaned
//! beside it, with no owner to ever remove them: a scene lost, silently. A
//! refusal costs a person one more choice of directory and loses nothing. A
//! directory holding only files the scene never writes takes the copy beside
//! them.

use std::collections::{BTreeMap, BTreeSet};
use std::io;
use std::path::Path;

use crcbl::store::{NativeStorage, StorageError, StorageSource};

use super::EditError;

/// Whether `a` and `b` name the same directory: spelled the same, or the same
/// once each is resolved — so a document opened by a relative path cleans up
/// on a save naming it absolutely.
///
/// Two paths that do not resolve, because one is not there yet, are not the
/// same: a save into a directory that does not exist has nothing of its own
/// to remove, and treating it as a copy is the side that cannot delete.
pub(super) fn same_dir(a: &Path, b: &Path) -> bool {
    a == b
        || matches!(
            (std::fs::canonicalize(a), std::fs::canonicalize(b)),
            (Ok(a), Ok(b)) if a == b
        )
}

/// [`EditError::Occupied`] for the first of `files` already in `storage`, so
/// a copy writes nothing over a file that is not its own.
pub(super) fn refuse_occupied(
    storage: &NativeStorage,
    files: &BTreeMap<String, String>,
) -> Result<(), EditError> {
    match files.keys().find(|key| storage.exists(Path::new(key))) {
        Some(key) => Err(EditError::Occupied {
            dir: storage.root().to_path_buf(),
            key: key.clone(),
        }),
        None => Ok(()),
    }
}

/// Removes from `storage` every key in `owned` that `written` does not hold,
/// and takes each out of `owned` as it goes — so `owned` is `written`'s keys
/// once it returns `Ok`, and still holds the ones not yet removed when it
/// does not.
///
/// # Errors
///
/// [`EditError::Remove`] for the first key that would not go.
pub(super) fn remove_unwritten(
    storage: &NativeStorage,
    owned: &mut BTreeSet<String>,
    written: &BTreeMap<String, String>,
) -> Result<(), EditError> {
    let unwritten: Vec<String> = owned
        .iter()
        .filter(|key| !written.contains_key(*key))
        .cloned()
        .collect();
    for key in unwritten {
        remove_file(storage, &key).map_err(|source| EditError::Remove {
            key: key.clone(),
            source,
        })?;
        owned.remove(&key);
    }
    Ok(())
}

/// Removes the regular file at `key`, if one is there.
///
/// Gone already is done: whoever removed it did this save's work. Anything
/// but a regular file — a directory or a link where the save wrote a file — is
/// something put there since, not the file the document owned, and is left
/// alone; [`NativeStorage::delete`] would empty a directory outright.
fn remove_file(storage: &NativeStorage, key: &str) -> Result<(), StorageError> {
    let path = storage.root().join(key);
    match std::fs::symlink_metadata(&path) {
        Err(error) if error.kind() == io::ErrorKind::NotFound => Ok(()),
        Err(error) => Err(StorageError::Io(error)),
        Ok(metadata) if !metadata.is_file() => Ok(()),
        Ok(_) => storage.delete(Path::new(key)),
    }
}
