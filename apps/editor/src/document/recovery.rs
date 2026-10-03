//! A recovery copy: the scene's files written into a directory of their own
//! when the editor is going away and cannot ask about unsaved edits.
//!
//! The editor asks before unsaved edits are lost wherever it can — the
//! unsaved bar, `crate::app`'s `unsaved` module — and that includes a close
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
//! Listing, pruning and removing copies is `copies`' — each by a path it
//! checks is a copy, never by a pattern.

use std::path::{Path, PathBuf};

use crcbl::registry::Registry;
use crcbl::store::{NativeStorage, StorageSource};

use super::{Document, EditError};

mod copies;

pub use copies::{
    KEEP_NEWEST, MAX_AGE, Pruned, RecoveryCopy, list_copies, prune_copies, remove_copy,
};

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
    /// directory under `base` named `<stamp>-<scene name>`, and hands back
    /// the directory — see the module docs.
    ///
    /// `stamp` is the caller's, so the name is the moment the copy was asked
    /// for: milliseconds since the Unix epoch is what the editor passes.
    ///
    /// [`authored_files`]: Self::authored_files
    ///
    /// # Errors
    ///
    /// [`EditError::Recovery`] if `base` or the copy's directory would not be
    /// made, [`EditError::Write`] naming the key that would not write, or as
    /// [`authored_files`](Self::authored_files).
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
        Ok(dir)
    }

    /// Opens the recovery copy at `dir` with the components `registry`
    /// knows, **unowned and dirty** — see the module docs. Its meshes read
    /// from no asset root until a save-as gives it a directory, whose root it
    /// takes then.
    ///
    /// # Errors
    ///
    /// As [`Document::open`].
    pub fn open_recovery(dir: &Path, registry: Registry) -> Result<Self, EditError> {
        let source = crcbl::assets::DirSource::at(dir.to_path_buf());
        let mut document = Self::open(&source, Path::new(""), registry)?;
        document.saved_at = None;
        Ok(document)
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

    use crate::document::origin_tests::tree;
    use crate::scene::BLOCKS;
    use crcbl::scene::scn::SceneEntityId;

    /// **A recovery copy is the scene's files, in a new directory named for
    /// the moment and the scene**, and the document is left as it was: dirty,
    /// with no origin.
    #[test]
    fn a_recovery_copy_writes_the_scene_and_leaves_the_document_alone() {
        let base = tempfile::tempdir().expect("a temporary directory");
        let mut document = Document::built_in().expect("the compiled-in scene");
        document.select(Some(SceneEntityId(0)));
        document
            .apply(crate::command::EditCommand::SetProperty {
                entity: SceneEntityId(0),
                system: BLOCKS.to_owned(),
                path: "position.0".to_owned(),
                value: crcbl::reflect::Value::Float(4.0),
            })
            .expect("a block moves");

        let dir = document
            .write_recovery(base.path(), 1_234)
            .expect("a fresh base");
        assert_eq!(dir, base.path().join("1234-greybox"));
        let files = document.files().expect("ids");
        let written = tree(&dir);
        assert_eq!(written.len(), files.len());
        for (key, text) in &files {
            assert_eq!(
                std::fs::read_to_string(dir.join(key)).expect("written"),
                *text,
                "`{key}` is not the scene's"
            );
        }
        assert!(
            document.is_dirty(),
            "a recovery copy marked the scene saved"
        );
        assert_eq!(document.origin(), None, "the copy became the scene's own");
    }

    /// **A taken name is passed over, never written into**: a second copy in
    /// the same millisecond gets the next name, and what was there is left
    /// byte for byte.
    #[test]
    fn a_recovery_copy_never_overwrites() {
        let base = tempfile::tempdir().expect("a temporary directory");
        let taken = base.path().join("7-greybox");
        std::fs::create_dir(&taken).expect("a fresh base");
        std::fs::write(taken.join("scene.ron"), "a person's own").expect("written");
        let mut document = Document::built_in().expect("the compiled-in scene");

        let dir = document
            .write_recovery(base.path(), 7)
            .expect("a free name");
        assert_eq!(dir, base.path().join("7-greybox-1"));
        assert_eq!(
            std::fs::read_to_string(taken.join("scene.ron")).expect("still there"),
            "a person's own"
        );
        let again = document
            .write_recovery(base.path(), 7)
            .expect("a free name");
        assert_eq!(again, base.path().join("7-greybox-2"));
    }

    /// **A copy made while the scene plays is the scene as authored**, not the
    /// played state.
    #[test]
    fn a_recovery_copy_in_play_is_the_authored_scene() {
        let base = tempfile::tempdir().expect("a temporary directory");
        let mut document = crate::document::play_tests::drifting_document();
        let authored = document.files().expect("ids");
        document.play().expect("the greybox scene plays");
        document.advance(crate::document::play_tests::TICK * 2);
        assert_ne!(
            document.files().expect("ids"),
            authored,
            "nothing moved in play, so this cannot tell the two apart"
        );
        document
            .write_recovery(base.path(), 1)
            .expect("a fresh base");
        let dir = base.path().join("1-greybox");
        for (key, text) in &authored {
            assert_eq!(
                std::fs::read_to_string(dir.join(key)).expect("written"),
                *text
            );
        }
    }

    /// **A copy is read back unowned and dirty**: the scene it holds, no
    /// origin, so a save asks for a directory, and the dirty marker up until
    /// a save elsewhere lands — and the copy left as it was.
    #[test]
    fn a_copy_opens_unowned_and_dirty() {
        let base = tempfile::tempdir().expect("a temporary directory");
        let mut document = crate::document::play_tests::drifting_document();
        let files = document.files().expect("ids");
        let dir = document
            .write_recovery(base.path(), 5)
            .expect("a fresh base");

        let mut opened =
            Document::open_recovery(&dir, crate::scene::vocabulary()).expect("a copy opens");
        assert_eq!(opened.files().expect("ids"), files);
        assert_eq!(opened.origin(), None, "the copy became the scene's home");
        assert!(opened.is_dirty(), "a recovered scene opened clean");
        assert!(matches!(opened.save(), Err(EditError::NoOrigin)));
        let saved = tempfile::tempdir().expect("a temporary directory");
        opened
            .save_as(saved.path().join("kept.scn"))
            .expect("an empty directory");
        assert!(!opened.is_dirty(), "a save-as left it dirty");
        assert_eq!(tree(&dir).len(), files.len(), "the copy was touched");
    }

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
