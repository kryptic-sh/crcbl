//! Where a document lives: a new scene with no directory yet, and save-as
//! giving it one.
//!
//! # A new scene
//!
//! [`Document::new_scene`] puts an empty scene in place of the one being
//! edited — no entity, a manifest listing no system, the compiled-in scene's
//! light and camera ([`crate::scene::empty_source`]) — with a fresh history
//! and no [`origin`](Document::origin). It keeps the vocabulary and the asset
//! source, so the asset browser goes on listing what it listed: a new scene
//! is started in the game being worked on. Each system is listed as the first
//! thing of its kind is put in it, through the
//! [`EditCommand::ListSystem`](crate::command::EditCommand::ListSystem) a mesh
//! drop and an attach already batch in front of themselves.
//!
//! # Save-as moves the document (decided 2026-10-03)
//!
//! [`Document::save_as`] writes the scene into a directory and **makes that
//! directory the document's origin**: the next [`save`](Document::save)
//! writes there, and the files it owns are the ones save-as wrote — the
//! meaning every editor gives the command. Before this, the only way to name
//! a directory was [`save_to`](Document::save_to), a copy that adopts
//! nothing, so a scene with no origin saved to one directory twice was
//! refused the second time.
//!
//! The safety of a copy is kept whole. A directory already holding a file the
//! scene would write is refused before anything is written
//! ([`EditError::Occupied`]), so save-as never merges into another scene; and
//! nothing in the old directory is removed or touched — the old files stay a
//! scene of their own, owned by nobody, which a person deletes if they want
//! it gone.
//!
//! **The asset root follows the new origin** unless it was named: a source
//! derived from where the scene was ([`super::asset_root`] of its old
//! origin), or none at all for a scene that never had one, becomes the
//! [`asset_root`] of the new directory, measured afresh. A
//! source named by [`set_assets`](Document::set_assets) — `--assets` on the
//! command line — stays, because a person chose it for the run.
//!
//! **A new scene takes its directory's name** (decided 2026-10-03): a save-as
//! of a scene still called [`UNTITLED`] names it after the directory — its
//! last component, less an extension, so `levels/first.scn` is `first` — and
//! writes that name into the header it saves. A scene with a name of its own
//! keeps it wherever it is saved, so a committed scene's files are written
//! byte for byte as they were; and nothing else renames a scene. A refused
//! save-as puts the old name back.
//!
//! # Typed directories
//!
//! The shell has no file dialog, so a directory is typed; [`save_target`] and
//! [`open_target`] check the text at that boundary, before anything is
//! written or read.

use std::path::{Path, PathBuf};

use crcbl::assets::DirSource;

use super::{Document, EditError, asset_root, load, ownership};
use crate::command::UndoLog;
use crate::scene::UNTITLED;

/// The file that makes a directory a scene: the header
/// [`Scene::load`](crcbl::scene::scn::Scene::load) reads first.
const HEADER: &str = "scene.ron";

/// Where a document's asset source came from, which decides whether a
/// [`Document::save_as`] moves it — see the module docs.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(super) enum AssetRoot {
    /// Nowhere yet: the empty source a scene opened out of a compiled-in
    /// source starts with. The first origin the document gets brings its own.
    Unset,
    /// [`asset_root`] of the document's origin, the directory named.
    Derived(PathBuf),
    /// Named by [`Document::set_assets`], and kept.
    Named,
}

impl Document {
    /// Puts a new, empty scene in place of this one — see the module docs:
    /// no entity, no system listed, no origin, nothing to undo, and clean.
    ///
    /// **Unsaved edits are dropped**, as closing the window drops them: the
    /// editor has no prompt for either, and a caller that wants to say so
    /// reads [`is_dirty`](Self::is_dirty) first.
    ///
    /// # Errors
    ///
    /// [`EditError::Playing`] in play mode, leaving the playing scene as it
    /// was: what play changed is thrown away by a stop, and a new scene in
    /// between would throw the authored one away with it.
    /// [`EditError::Scene`] if the empty scene will not load into the
    /// vocabulary, which is a tree whose `crate::scene` tests are red too.
    pub fn new_scene(&mut self) -> Result<(), EditError> {
        self.refuse_in_play()?;
        let (world, scene, ids) =
            load(&crate::scene::empty_source(), Path::new(""), &self.registry)?;
        self.world = world;
        self.scene = scene;
        self.ids = ids;
        self.selection.clear();
        self.log = UndoLog::new();
        self.saved_at = Some(0);
        self.recovered = None;
        self.origin = None;
        self.owned.clear();
        // Moved rather than reset: a view that read the old scene at some
        // count must see this one as a change, whatever that count was.
        self.membership += 1;
        self.naming += 1;
        self.resolve_meshes();
        Ok(())
    }

    /// Writes the scene into the directory at `dir` and makes it the
    /// document's origin, so later saves go there — see the module docs.
    /// Returns whether the asset source moved with it, which a caller listing
    /// or drawing the assets reads again.
    ///
    /// Into the document's own origin it is [`save`](Self::save), removals
    /// and all, and moves nothing.
    ///
    /// # Errors
    ///
    /// As [`save_to`](Self::save_to) into another directory: in particular
    /// [`EditError::Occupied`], writing nothing, for a directory that already
    /// holds a file the scene would write, and [`EditError::Playing`] in play
    /// mode. **The origin moves only when every file landed**, so a refused
    /// or failed save-as leaves the document where it was, still dirty.
    pub fn save_as(&mut self, dir: impl Into<PathBuf>) -> Result<bool, EditError> {
        self.refuse_in_play()?;
        let dir = dir.into();
        if self
            .origin
            .as_deref()
            .is_some_and(|origin| ownership::same_dir(origin, &dir))
        {
            self.save()?;
            return Ok(false);
        }
        let was = self.name_after(&dir);
        match self.write(&dir) {
            Ok(owned) => self.owned = owned,
            Err(error) => {
                if let Some(was) = was {
                    self.scene.set_name(was);
                }
                return Err(error);
            }
        }
        let moved = self.follow_asset_root(&dir);
        self.origin = Some(dir);
        Ok(moved)
    }

    /// Names a scene still called [`UNTITLED`] after the directory `dir` —
    /// its last component, less an extension — and hands back the name it
    /// had; or changes nothing, handing back [`None`], for a scene with a
    /// name of its own or a directory whose name is not text.
    fn name_after(&mut self, dir: &Path) -> Option<String> {
        if self.scene.name() != UNTITLED {
            return None;
        }
        let name = dir.file_stem()?.to_str()?;
        Some(self.scene.set_name(name))
    }

    /// Reads assets from [`asset_root`] of the scene at `scene` from now on,
    /// unless a source was named — and says whether that moved the source.
    pub(super) fn follow_asset_root(&mut self, scene: &Path) -> bool {
        if self.asset_root == AssetRoot::Named {
            return false;
        }
        let root = asset_root(scene);
        if self.asset_root == AssetRoot::Derived(root.clone()) {
            return false;
        }
        self.replace_assets(Box::new(DirSource::at(root.clone())));
        self.asset_root = AssetRoot::Derived(root);
        true
    }
}

/// The directory a person typed for a save-as, checked before anything is
/// written: surrounding whitespace dropped, made absolute against the working
/// directory — where `editor <SCENE_DIR>` resolves a relative path too — and
/// refused when it is empty, holds a control character, or names something
/// other than a directory.
///
/// What is in the directory is [`Document::save_as`]'s to check, since the
/// files it would write are the scene's.
///
/// # Errors
///
/// [`EditError::Target`] naming the text and why.
pub fn save_target(text: &str) -> Result<PathBuf, EditError> {
    let refuse = |reason: &str| EditError::Target {
        text: text.to_owned(),
        reason: reason.to_owned(),
    };
    let path = typed_dir(text, refuse)?;
    if path.exists() && !path.is_dir() {
        return Err(refuse("something other than a directory is there"));
    }
    Ok(path)
}

/// The directory a person typed to open, checked before anything is read: as
/// [`save_target`] reads the text, and refused unless a directory holding a
/// `scene.ron` is there — so a directory that is not a scene is refused by
/// its name rather than by the key the loader missed.
///
/// What the scene holds is [`Document::open_dir`]'s to check.
///
/// # Errors
///
/// [`EditError::OpenTarget`] naming the text and why.
pub fn open_target(text: &str) -> Result<PathBuf, EditError> {
    let refuse = |reason: &str| EditError::OpenTarget {
        text: text.to_owned(),
        reason: reason.to_owned(),
    };
    let path = typed_dir(text, refuse)?;
    if !path.is_dir() {
        return Err(refuse("no directory is there"));
    }
    if !path.join(HEADER).is_file() {
        return Err(refuse(&format!(
            "it holds no `{HEADER}`, so it is not a scene"
        )));
    }
    Ok(path)
}

/// A typed directory with its surrounding whitespace dropped, made absolute
/// against the working directory — where `editor <SCENE_DIR>` resolves a
/// relative path too — or `refuse` of why not: it is empty, or holds a control
/// character.
fn typed_dir(text: &str, refuse: impl Fn(&str) -> EditError) -> Result<PathBuf, EditError> {
    let text = text.trim();
    if text.is_empty() {
        return Err(refuse("it names no directory"));
    }
    if text.chars().any(char::is_control) {
        return Err(refuse("it holds a control character"));
    }
    std::path::absolute(text).map_err(|error| refuse(&error.to_string()))
}
