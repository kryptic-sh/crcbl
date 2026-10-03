//! A recovery copy: the scene's files written into a directory of their own
//! when the editor is going away and cannot ask about unsaved edits.
//!
//! The editor asks before unsaved edits are lost wherever it can — the
//! unsaved bar, the editor app's `unsaved` module — and that includes a close
//! request, which every backend holds open until it is answered. What it
//! cannot ask about is a window taken away without a request, or a run that
//! fails: then [`Document::write_recovery`] writes the scene as it was
//! authored somewhere nothing else writes, and the caller logs where.
//!
//! **It never overwrites.** Each copy is a directory this module makes itself,
//! named for the moment and the scene, and a name already taken — two copies
//! in one millisecond, or one a person left — is passed over for the next free
//! one. The document is not touched: it is not marked saved and does not
//! adopt the directory, because a recovery copy is not where the scene lives.
//! The editor's autosave is the same writer on a timer.
//!
//! **A copy is read back unowned** ([`Document::open_recovery`]): no origin,
//! so a save asks for a directory and a copy is never written back over
//! itself, and dirty, so what it holds is asked about before it is lost.
//! It remembers the copy ([`Document::take_recovered`]) so the caller can
//! remove it once a save-as has put the scene somewhere of its own. Listing,
//! pruning, removing copies and marking a live session's autosave in use is
//! `copies`' — each by a path it checks is a copy, never by a pattern.
//!
//! **A copy remembers where its scene lived**: beside the scene's files it
//! records the scene's directory and its asset root (`sidecar`'s module docs
//! say how, and why nothing it names is trusted). Read back, a copy whose
//! asset root is still a directory reads its meshes from there, and the
//! scene's old directory is what a save-as offers first
//! ([`Document::recorded_origin`]); a copy without the record, or with a
//! stale one, reads its meshes from no asset root until a save-as gives it a
//! directory, and says why ([`Document::take_recovery_notes`]).

use std::path::{Path, PathBuf};

use crate::registry::Registry;
use crate::store::{NativeStorage, StorageSource};

use super::origin::AssetRoot;
use super::{Document, EditError};
use crate::scene::edit::UndoLog;

mod copies;
mod sidecar;

pub use copies::{
    IN_USE_SUFFIX, InUse, KEEP_NEWEST, MAX_AGE, Pruned, RecoveryCopy, list_copies, mark_in_use,
    prune_copies, remove_copy,
};
pub use sidecar::SIDECAR;

/// The directory under the recovery base every copy is made in — a name of
/// its own, so a person who finds it knows what wrote it.
pub const RECOVERY_DIR: &str = "crcbl-editor-recovery";

/// How many names a copy tries before giving up: the stamp alone, then the
/// stamp with a counter.
const ATTEMPTS: u32 = 100;

/// The longest the scene's name may make a copy's directory name: a header
/// carries any string, and a directory name has a length limit everywhere.
const NAME_MAX: usize = 64;

/// What a copy is called when the scene's name holds no character a
/// directory name keeps.
const UNNAMED: &str = "scene";

impl Document {
    /// Writes the scene as it was authored ([`authored_files`]) into a new
    /// directory under `base` named `<stamp>-<scene name>`, with [`SIDECAR`]
    /// beside it recording where the scene lived, and hands back the
    /// directory — see the module docs.
    ///
    /// `stamp` is the caller's, so the name is the moment the copy was asked
    /// for: milliseconds since the Unix epoch is what the editor passes.
    ///
    /// [`authored_files`]: Self::authored_files
    ///
    /// # Errors
    ///
    /// [`EditError::Recovery`] if `base` or the copy's directory would not be
    /// made, [`EditError::Write`] naming the key that would not write — the
    /// sidecar last, so the scene's files are all there when it fails — or
    /// as [`authored_files`](Self::authored_files).
    pub fn write_recovery(&mut self, base: &Path, stamp: u128) -> Result<PathBuf, EditError> {
        let files = self.authored_files()?;
        std::fs::create_dir_all(base).map_err(|source| EditError::Recovery {
            dir: base.to_path_buf(),
            source,
        })?;
        let dir = claim(base, &format!("{stamp}-{}", dir_name(self.name())))?;
        let storage = NativeStorage::at(dir.clone());
        for (key, text) in &files {
            storage
                .write(Path::new(key), text.as_bytes())
                .map_err(|source| EditError::Write {
                    key: key.clone(),
                    source,
                })?;
        }
        if let Some(text) = sidecar::text(&self.home()) {
            storage
                .write(Path::new(SIDECAR), text.as_bytes())
                .map_err(|source| EditError::Write {
                    key: SIDECAR.to_owned(),
                    source,
                })?;
        }
        Ok(dir)
    }

    /// Where this scene lives, as a copy records it: its origin — or, for a
    /// scene read back from a copy, where that copy said it lived — and the
    /// asset root it follows. A root named by [`set_assets`](Self::set_assets)
    /// is a source with no path, and is not recorded.
    fn home(&self) -> sidecar::Home {
        sidecar::Home {
            origin: self.origin.clone().or_else(|| self.recorded_origin.clone()),
            assets: match &self.asset_root {
                AssetRoot::Derived(root) => Some(root.clone()),
                AssetRoot::Unset | AssetRoot::Named => None,
            },
        }
    }

    /// Opens the recovery copy at `dir` with the components `registry`
    /// knows, **unowned and dirty**, remembering `dir` — see the module docs.
    /// Its meshes read from the asset root the copy recorded, if that is
    /// still a directory, and otherwise from none until a save-as gives it a
    /// directory, whose root it takes then. What the record held that was
    /// passed over is in [`take_recovery_notes`](Self::take_recovery_notes).
    ///
    /// # Errors
    ///
    /// As [`Document::open`].
    pub fn open_recovery(dir: &Path, registry: Registry) -> Result<Self, EditError> {
        let source = crate::assets::DirSource::at(dir.to_path_buf());
        let mut document = Self::open(&source, Path::new(""), registry)?;
        // A log no save has marked, so the copy reads dirty from the start.
        document.log = UndoLog::new();
        document.recovered = Some(dir.to_path_buf());
        let (home, notes) = sidecar::read(dir);
        if let Some(root) = home.assets {
            document.replace_assets(Box::new(crate::assets::DirSource::at(root.clone())));
            document.asset_root = AssetRoot::Derived(root);
        }
        document.recorded_origin = home.origin;
        document.recovery_notes = notes;
        Ok(document)
    }

    /// The directory the recovery copy this document was read from recorded
    /// as where its scene lived, if that was still a directory when it was
    /// read — what a save-as offers first. [`None`] for anything else, and
    /// once a save-as or a new scene has given the document a home of its
    /// own.
    #[must_use]
    pub fn recorded_origin(&self) -> Option<&Path> {
        self.recorded_origin.as_deref()
    }

    /// Hands back, once, a note for each thing the recovery copy this
    /// document was read from recorded and [`open_recovery`] passed over,
    /// for the caller to say. Empty for anything else.
    ///
    /// [`open_recovery`]: Self::open_recovery
    pub fn take_recovery_notes(&mut self) -> Vec<String> {
        std::mem::take(&mut self.recovery_notes)
    }

    /// Hands back the recovery copy this document was read back from, once:
    /// a caller takes it after the first save-as that lands, when the copy
    /// holds nothing the scene's own directory does not (decided
    /// 2026-10-03). [`None`] for a document not read from a copy, or one
    /// whose copy was taken, and after [`Document::new_scene`].
    pub fn take_recovered(&mut self) -> Option<PathBuf> {
        self.recovered.take()
    }
}

/// Makes a directory under `base` named `stem`, or `stem-1`, `stem-2` and so
/// on if that is taken, and hands it back.
///
/// [`std::fs::create_dir`] rather than a check and then a make: it fails on a
/// name that is there, so a directory is never claimed twice, even by two
/// copies racing.
fn claim(base: &Path, stem: &str) -> Result<PathBuf, EditError> {
    for attempt in 0..ATTEMPTS {
        let dir = match attempt {
            0 => base.join(stem),
            _ => base.join(format!("{stem}-{attempt}")),
        };
        match std::fs::create_dir(&dir) {
            Ok(()) => return Ok(dir),
            Err(error) if error.kind() == std::io::ErrorKind::AlreadyExists => {}
            Err(source) => return Err(EditError::Recovery { dir, source }),
        }
    }
    Err(EditError::Recovery {
        dir: base.join(stem),
        source: std::io::Error::new(
            std::io::ErrorKind::AlreadyExists,
            format!("every one of {ATTEMPTS} names for the copy is taken"),
        ),
    })
}

/// `name` as a directory name on every platform: ASCII letters, digits, `-`
/// and `_` kept, anything else `_`, at most [`NAME_MAX`] characters — or
/// [`UNNAMED`] for a name with nothing to keep.
fn dir_name(name: &str) -> String {
    let kept: String = name
        .chars()
        .take(NAME_MAX)
        .map(|c| {
            if c.is_ascii_alphanumeric() || c == '-' || c == '_' {
                c
            } else {
                '_'
            }
        })
        .collect();
    if kept.chars().all(|c| c == '_') {
        UNNAMED.to_owned()
    } else {
        kept
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// **A scene's name is made a directory name**: what a path would read as
    /// a separator or worse is replaced, a long name is cut, and a name with
    /// nothing to keep is called something.
    #[test]
    fn a_scene_name_is_made_a_directory_name() {
        assert_eq!(dir_name("field"), "field");
        assert_eq!(dir_name("../a b"), "___a_b");
        assert_eq!(dir_name(""), UNNAMED);
        assert_eq!(dir_name("//"), UNNAMED);
        assert_eq!(dir_name(&"x".repeat(200)).len(), NAME_MAX);
    }
}
