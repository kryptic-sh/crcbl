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

use crcbl::registry::Registry;
use crcbl::store::{NativeStorage, StorageSource};

use super::origin::AssetRoot;
use super::{Document, EditError};
use crate::command::UndoLog;

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
        let source = crcbl::assets::DirSource::at(dir.to_path_buf());
        let mut document = Self::open(&source, Path::new(""), registry)?;
        // A log no save has marked, so the copy reads dirty from the start.
        document.log = UndoLog::new();
        document.recovered = Some(dir.to_path_buf());
        let (home, notes) = sidecar::read(dir);
        if let Some(root) = home.assets {
            document.replace_assets(Box::new(crcbl::assets::DirSource::at(root.clone())));
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

    use crate::document::origin_tests::{props_in_a_game, tree};
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

    /// **A recovered document remembers its copy until it is taken**, once,
    /// and a new scene in its place forgets it; a scene opened from a
    /// directory has none.
    #[test]
    fn a_recovered_document_remembers_its_copy_once() {
        let base = tempfile::tempdir().expect("a temporary directory");
        let mut document = Document::built_in().expect("the compiled-in scene");
        assert_eq!(document.take_recovered(), None);
        let dir = document
            .write_recovery(base.path(), 5)
            .expect("a fresh base");

        let mut opened =
            Document::open_recovery(&dir, crate::scene::vocabulary()).expect("a copy opens");
        assert_eq!(opened.take_recovered(), Some(dir.clone()));
        assert_eq!(opened.take_recovered(), None, "taken twice");
        let mut replaced =
            Document::open_recovery(&dir, crate::scene::vocabulary()).expect("a copy opens");
        replaced.new_scene().expect("an empty scene");
        assert_eq!(replaced.take_recovered(), None, "a new scene kept the copy");
    }

    /// **A copy records where its scene lived, and reads its meshes from
    /// there**: the sidecar names the scene's directory and its game's root,
    /// the loader passes over it, and the scene read back measures the
    /// game's triangle and offers its old directory — with nothing to note.
    #[test]
    fn a_copy_records_where_its_scene_lived_and_reads_its_meshes_from_there() {
        let base = tempfile::tempdir().expect("a temporary directory");
        let (game, scene, mut document) = props_in_a_game();
        let files = document.files().expect("ids");
        let dir = document
            .write_recovery(base.path(), 3)
            .expect("a fresh base");
        assert_eq!(
            std::fs::read_to_string(dir.join(SIDECAR)).expect("a sidecar"),
            format!(
                "origin={}\nassets={}\n",
                scene.display(),
                game.path().display()
            )
        );
        assert_eq!(tree(&dir).len(), files.len() + 1);

        let mut opened =
            Document::open_recovery(&dir, crate::scene::vocabulary()).expect("a copy opens");
        assert_eq!(opened.files().expect("ids"), files, "the sidecar was read");
        assert_eq!(
            opened.mesh_problems().len(),
            1,
            "the recovered scene's meshes are not the game's: {:?}",
            opened.mesh_problems()
        );
        assert_eq!(opened.recorded_origin(), Some(scene.as_path()));
        assert_eq!(opened.origin(), None, "the old directory became its home");
        assert_eq!(opened.take_recovery_notes(), Vec::<String>::new());

        // A copy of the recovered scene still knows where it lived.
        let again = opened.write_recovery(base.path(), 4).expect("a fresh name");
        assert_eq!(
            std::fs::read_to_string(again.join(SIDECAR)).expect("a sidecar"),
            std::fs::read_to_string(dir.join(SIDECAR)).expect("a sidecar"),
        );
    }

    /// **A copy without a sidecar opens as before**: its meshes read from no
    /// asset root and nothing is offered or noted — and a scene that lived
    /// nowhere writes none.
    #[test]
    fn a_copy_without_a_sidecar_opens_as_before() {
        let base = tempfile::tempdir().expect("a temporary directory");
        let (_game, _scene, mut document) = props_in_a_game();
        let dir = document
            .write_recovery(base.path(), 3)
            .expect("a fresh base");
        std::fs::remove_file(dir.join(SIDECAR)).expect("a sidecar");

        let mut opened =
            Document::open_recovery(&dir, crate::scene::vocabulary()).expect("a copy opens");
        assert_eq!(opened.mesh_problems().len(), 2, "a mesh was measured");
        assert_eq!(opened.recorded_origin(), None);
        assert_eq!(opened.take_recovery_notes(), Vec::<String>::new());

        let lived_nowhere = Document::built_in()
            .expect("the compiled-in scene")
            .write_recovery(base.path(), 5)
            .expect("a fresh name");
        assert!(
            !lived_nowhere.join(SIDECAR).exists(),
            "a sidecar of nothing"
        );
    }

    /// **A stale sidecar is passed over, never trusted**: a scene directory
    /// gone since and an asset root that is now a file are neither offered
    /// nor read from, each is noted, and neither is made.
    #[test]
    fn a_stale_sidecar_is_passed_over_with_notes() {
        let base = tempfile::tempdir().expect("a temporary directory");
        let (_game, _scene, mut document) = props_in_a_game();
        let dir = document
            .write_recovery(base.path(), 3)
            .expect("a fresh base");
        let elsewhere = tempfile::tempdir().expect("a temporary directory");
        let gone = elsewhere.path().join("gone.scn");
        let file = elsewhere.path().join("assets");
        std::fs::write(&file, "not a directory").expect("written");
        std::fs::write(
            dir.join(SIDECAR),
            format!("origin={}\nassets={}\n", gone.display(), file.display()),
        )
        .expect("written");

        let mut opened =
            Document::open_recovery(&dir, crate::scene::vocabulary()).expect("a copy opens");
        assert_eq!(opened.recorded_origin(), None, "a gone directory offered");
        assert_eq!(opened.mesh_problems().len(), 2, "read through a file");
        let notes = opened.take_recovery_notes();
        assert_eq!(notes.len(), 2, "{notes:?}");
        assert!(notes.iter().all(|note| note.contains("passed over")));
        assert!(!gone.exists(), "the gone directory was made");
        assert!(file.is_file(), "the file was touched");
        assert_eq!(opened.take_recovery_notes(), Vec::<String>::new(), "twice");
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
