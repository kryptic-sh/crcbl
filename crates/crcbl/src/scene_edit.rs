//! The thing being edited: a scene, the world it was loaded into, what is
//! selected, and the history of what has been done to it — and the server
//! that lets several clients edit one ([`EditServer`]).
//!
//! Everything in this module runs **without a device and without a window**,
//! which is `docs/plan/08-editor.md`'s "nothing editor-side may be implemented
//! GUI-only" applied to the first slice: the pick, the command, its inverse and
//! the save are all methods here, and the editor's app (`apps/editor`) is a
//! loop that calls them. The gates hold this half; the windowed harness holds
//! the other.
//!
//! # Why it lives in the umbrella (decided 2026-10-04)
//!
//! It was `apps/editor`'s `document` module, and moved here so that the
//! editor, the `crcbl` CLI and the edit server share one implementation: a
//! crate under `crates/` cannot depend on an app, and a second document
//! written for the CLI would be a second set of rules. The editor re-exports
//! it from its old paths. **It names no component type**: the vocabulary a
//! document opens with is a [`Registry`] its caller builds, so the editor's
//! own greybox block and the games' components stay with the caller.
//!
//! # The five verbs
//!
//! * [`Document::open`] — a `.scn/` directory through any [`AssetSource`],
//!   into a [`World`] of this document's own.
//! * [`Document::pick`] — a ray, through [`PhysicsSystem::cast_ray`], to the
//!   [`SceneEntityId`] under it.
//! * [`Document::apply`] — one [`EditCommand`], recorded with the inverse it
//!   produced.
//! * [`Document::undo`] / [`Document::redo`] — the log, walked.
//! * [`Document::files`] — the scene as text, byte-stably, and
//!   [`Document::save_to`] the same text written to a directory;
//!   [`Document::save_as`] makes that directory the document's own, and
//!   [`Document::new_scene`] starts from nothing (`scene_edit::origin`).
//!
//! And a sixth that is not an edit: [`Document::play`] runs the scene with its
//! games' modules until [`Document::stop`] puts it back exactly as it was —
//! see `play`'s own module docs, and why every edit is refused in between.
//!
//! # What "dirty" means here
//!
//! The position the log stands at, against the position it stood at when the
//! document was last saved — not a flag. Undoing back to a saved state is
//! **clean**, which a flag could not say and which this module's
//! `the_dirty_marker_follows_the_logs_position` holds.

use std::collections::{BTreeMap, BTreeSet, HashSet};
use std::fmt;
use std::path::{Path, PathBuf};

use crate::assets::{AssetSource, MemorySource};
use crate::ecs::{Entity, World};
use crate::math::{DVec3, Vec3};
use crate::phys::{ColliderComponent, PhysicsSystem, Ray, RigidBody, Transform};
use crate::reflect::{PathError, Reflect, Value, get_path, restore_path, set_path};
use crate::registry::{FieldError, OrientedBox, Registry};
use crate::render::ViewRay;
use crate::scene::scn::{EntityName, IdMap, NameError, Scene, SceneEntityId, ScnError};
use crate::scene_mesh::{MeshLibrary, MeshProblem};
use crate::store::{NativeStorage, StorageError, StorageSource};
use crate::ui::tree::{FieldEdit, VariantEdit};

use crate::scene::edit::{EditCommand, Gesture, SystemRow, UndoLog, set_property, set_variant};

pub mod clipboard;
mod environment;
mod field;
mod history;
mod meshes;
mod naming;
mod origin;
mod ownership;
mod play;
mod recovery;
mod selection;
pub mod serve;
mod systems;
mod validation;

pub use environment::Environment;
pub use field::text_of;
pub use history::{HISTORY, HistoryError, MAX_HISTORY_BYTES, MAX_HISTORY_ENTRIES};
pub use origin::{NEW_SCENE_ENV_RON, UNTITLED, empty_source, open_target, save_target};
pub use play::{Hit, PlayState};
pub use recovery::{
    IN_USE_SUFFIX, InUse, KEEP_NEWEST, MAX_AGE, Pruned, RECOVERY_DIR, RecoveryCopy, SIDECAR,
    list_copies, mark_in_use, prune_copies, remove_copy,
};
pub use serve::{EditServer, refusal_of};
pub use systems::{IN_SCENE, SystemGroup, UNGROUPED};

/// A loaded scene and everything the editor knows about it.
#[derive(Debug)]
pub struct Document {
    world: World,
    scene: Scene,
    ids: IdMap,
    /// The vocabulary this document was opened with: the codec, the system, the
    /// `&mut dyn Reflect` and the placement, per chunk name.
    ///
    /// Held rather than borrowed, so a `Document` has no lifetime and two of them
    /// can be open on different vocabularies. It is a handful of function
    /// pointers per component — see [`crate::registry::Registry`].
    registry: Registry,
    /// Which [`SceneEntityId`]s are selected, in the order they joined, the
    /// primary last — see `scene_edit::selection`. Ids rather than
    /// [`Entity`]s for [`EditCommand`]'s reason: they are what survives a
    /// reload.
    selection: Vec<SceneEntityId>,
    /// The history, which also remembers the entry the document was last
    /// written at ([`UndoLog::mark_saved`]): its empty state for one loaded
    /// and not saved, so a document nobody has edited opens clean, and nothing
    /// for a recovery copy read back, whose edits are saved nowhere until it
    /// is written somewhere ([`Document::open_recovery`]).
    log: UndoLog,
    /// The recovery copy this document was read back from, until the caller
    /// takes it to remove once a save-as has put the scene somewhere of its
    /// own — see [`Document::take_recovered`]. [`None`] for anything else.
    recovered: Option<PathBuf>,
    /// Where the recovery copy this document was read from said its scene
    /// lived — see [`Document::recorded_origin`]. [`None`] for anything else.
    recorded_origin: Option<PathBuf>,
    /// What [`Document::open_recovery`] passed over in the copy's record,
    /// until the caller takes it — see [`Document::take_recovery_notes`].
    recovery_notes: Vec<String>,
    /// Why [`Document::open_with_history_or_fresh`] read no history from
    /// beside the scene, until the caller takes it — see
    /// [`Document::take_history_refusal`]. [`None`] for anything else.
    history_refusal: Option<HistoryError>,
    /// How many times an entity has entered or left this document — see
    /// [`Document::membership`].
    membership: u64,
    /// How many times an entity's name has changed — see
    /// [`Document::naming`].
    naming: u64,
    /// The last [`Gesture`] [`Document::begin_gesture`] handed out.
    gestures: u64,
    /// Where [`Document::save_to`] writes when it is not told otherwise: the
    /// directory this document was opened from, or [`None`] for one opened out
    /// of a compiled-in source.
    origin: Option<PathBuf>,
    /// The files of [`origin`](Self::origin) this document stands behind: the
    /// ones its manifest named when it was opened, and every one a save has
    /// written there since and not removed — so the only files a save there
    /// may remove. Empty for a document with no origin. See
    /// `scene_edit::ownership`.
    owned: BTreeSet<String>,
    /// The scene as it stood when play began, and what is running it — or
    /// [`None`] while editing. See [`Document::play`].
    play: Option<play::Session>,
    /// Where a mesh's asset key is read from — see [`Document::set_assets`].
    assets: Box<dyn AssetSource>,
    /// Where [`assets`](Self::assets) came from, which says whether a save-as
    /// moves it — see `scene_edit::origin`.
    asset_root: origin::AssetRoot,
    /// Every asset a mesh has been measured from, each imported once.
    meshes: MeshLibrary,
    /// What the last resolve could not measure — see
    /// [`Document::mesh_problems`].
    mesh_problems: Vec<MeshProblem>,
    /// How many resolves have moved a mesh's box — see
    /// [`Document::measures`].
    measures: u64,
}

/// Why a document would not open, edit or save.
///
/// [`fmt::Display`] hand-written rather than derived, which is every app in
/// this workspace's shape: `apps/viewer`'s `LoadError` is the nearest one, and
/// a sample's dependency list is the engine and the standard library.
#[derive(Debug)]
pub enum EditError {
    /// The scene would not load, or would not be written back.
    Scene(ScnError),

    /// A command named an entity this document does not hold.
    ///
    /// Not a panic: a command is a value that can be recorded now and applied
    /// later, so an id that has gone away is a condition rather than a bug —
    /// and it is the reason code `docs/plan/08-editor.md`'s 2026-07-27
    /// correction asks a stale operation to fail with.
    NoEntity(SceneEntityId),

    /// A command named a field the entity's component does not have, or handed
    /// a leaf a value it refused.
    Path(PathError),

    /// A spawn named an id an entity already holds.
    ///
    /// Filing a second entity under it would drop one of the two out of the
    /// scene's id map, and it would come back as a save that lost a row.
    IdInUse(SceneEntityId),

    /// A spawn or a row-level attach named a system this scene's manifest does
    /// not list, or an edit named one the document's vocabulary cannot read a
    /// row of.
    ///
    /// Refused rather than attached: a save writes the manifest's chunks and no
    /// others, so the row would be dropped by the next one.
    /// [`Document::attach`] lists the system first instead.
    NoSystem(String),

    /// A listing named a system the manifest already lists.
    Listed(String),

    /// A listing named a place past the manifest's end.
    PastManifest {
        /// The system it would have listed.
        system: String,
        /// The place it named.
        at: usize,
        /// How many systems the manifest lists.
        len: usize,
    },

    /// An unlisting named a system still holding entities, whose rows the
    /// next save would drop.
    Populated(String),

    /// A mesh was asked for of a key no mesh may name.
    Asset(crate::scene_mesh::MeshPathError),

    /// A property write would leave `entity`'s component in `system` holding
    /// a value its rule refuses — a body's mass of zero, one of a rotation's
    /// four leaves written alone — which the next load would refuse.
    ///
    /// Refused, and the command put back, rather than repaired: see the
    /// module docs of `scene_edit::validation`.
    Invalid {
        /// Whose.
        entity: SceneEntityId,
        /// The system holding the component.
        system: String,
        /// The field, and why — [`FieldError::field`] is its dotted path.
        error: FieldError,
    },

    /// A drop's ray met neither the scene nor the ground plane in front of
    /// the camera, so there is nowhere to put what was dropped.
    NoGround,

    /// A command named a component of `entity` in a system that does not hold
    /// it — an edit, a read or a detach.
    NotAttached {
        /// Whose.
        entity: SceneEntityId,
        /// The system named.
        system: String,
    },

    /// An attach, or a spawn's second row, named a system that already holds
    /// `entity`: a system holds one component per entity, so the row would
    /// replace the one there and its undo would lose it.
    Attached {
        /// Whose.
        entity: SceneEntityId,
        /// The system named.
        system: String,
    },

    /// An edit would leave `entity` in no system: a spawn of no rows, or a
    /// detach of its last component. An entity in no system is in no chunk
    /// file, so a save would lose it; [`Document::delete`] is how an entity
    /// goes.
    NoComponent(SceneEntityId),

    /// A paste's text is not a clipping of entities — the ordinary case of
    /// pasting something copied from anywhere else.
    Paste(crate::ron::error::SpannedError),

    /// A rename, or a clipping's name, is not a name: empty where one was
    /// wanted, too long, or holding a control character.
    Name(NameError),

    /// A field paste's text — from the clipboard, or typed on the `crcbl`
    /// CLI's command line — is not a value of the field's kind, read the way
    /// the scene's loader reads that kind.
    FieldPaste {
        /// The field pasted into.
        path: String,
        /// What ron said about the text.
        message: String,
    },

    /// A file would not be written.
    Write {
        /// The scene-relative key that failed.
        key: String,
        /// What the storage said.
        source: StorageError,
    },

    /// A save to a directory other than the document's own found a file it
    /// would write already there, and wrote nothing.
    ///
    /// Refused rather than overwritten: the file is some other scene's, or
    /// something a person put there, and a save that replaced another scene's
    /// `scene.ron` would orphan that scene's chunks beside it.
    Occupied {
        /// The directory the save was asked to write into.
        dir: PathBuf,
        /// The scene-relative key already there.
        key: String,
    },

    /// A save-as was handed text that names no directory it could write
    /// into — see [`save_target`].
    Target {
        /// What was typed.
        text: String,
        /// Why it is not a directory to save into.
        reason: String,
    },

    /// An open was handed text that names no scene directory — see
    /// [`open_target`].
    OpenTarget {
        /// What was typed.
        text: String,
        /// Why it is not a scene to open.
        reason: String,
    },

    /// A recovery copy's directory would not be made, or the recovery
    /// directory would not be read — see [`Document::write_recovery`] and
    /// [`list_copies`].
    Recovery {
        /// The directory that was being made or read.
        dir: PathBuf,
        /// What the filesystem said.
        source: std::io::Error,
    },

    /// A removal was asked for of a path that is not a recovery copy directly
    /// under the recovery directory — see [`remove_copy`]. Nothing was
    /// removed.
    NotACopy(PathBuf),

    /// A removal was asked for of another editor's live autosave — see
    /// [`mark_in_use`]. Nothing was removed.
    CopyInUse(PathBuf),

    /// A recovery copy would not be removed.
    RemoveCopy {
        /// The copy's directory.
        dir: PathBuf,
        /// What the filesystem said.
        source: std::io::Error,
    },

    /// A save wrote every file, then could not remove one its scene no longer
    /// names.
    ///
    /// The document stays dirty and still owns the file, so the next save
    /// tries again; until then the file is inert, because the loader reads only
    /// what the manifest names.
    Remove {
        /// The scene-relative key that would not go.
        key: String,
        /// What the storage said.
        source: StorageError,
    },

    /// [`Document::save`] was asked to write a document that was never opened
    /// from a directory.
    NoOrigin,

    /// An edit, an undo or a save was asked for while the scene is in play
    /// mode.
    ///
    /// Refused rather than applied: what play changes is thrown away when it
    /// stops, so an edit made into it would be lost with it, and a save would
    /// write a played state over the authored one. See [`Document::play`].
    Playing,

    /// Play was asked to step a world whose tick period, in seconds, is not
    /// one a fixed step can be taken at — zero, negative or not a number,
    /// which a module's [`register`](crate::ecs::GameModule::register) set.
    TickRate(f64),

    /// A game whose system the scene lists refused to play it, saying why —
    /// towers' path with a diagonal leg is the case. Play does not start.
    Unplayable(String),

    /// A play action was asked for while the scene is being edited: there is
    /// no game running to send it to.
    NotPlaying,

    /// A play action that could not be sent, saying why: no running module
    /// under that system, or an action or arguments its game's controls
    /// would not encode. Nothing was sent.
    PlayCommand(String),

    /// The edit history beside the scene was refused, or would not be
    /// written — see [`Document::open_with_history`] and
    /// [`Document::save_with_history`]. Nothing was replayed from it.
    History(HistoryError),
}

impl fmt::Display for EditError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Scene(error) => write!(f, "{error}"),
            Self::NoEntity(id) => write!(f, "the scene holds no entity {id}"),
            Self::Path(error) => write!(f, "{error}"),
            Self::IdInUse(id) => write!(f, "the scene already holds an entity {id}"),
            Self::NoSystem(system) => {
                write!(f, "the scene has no system `{system}` to put an entity in")
            }
            Self::Listed(system) => write!(f, "the scene already lists `{system}`"),
            Self::PastManifest { system, at, len } => write!(
                f,
                "the scene lists {len} systems, so `{system}` cannot be listed at place {at}"
            ),
            Self::Populated(system) => write!(
                f,
                "`{system}` still holds entities, which a save without it would drop"
            ),
            Self::Asset(error) => write!(f, "{error}"),
            Self::Invalid {
                entity,
                system,
                error,
            } => write!(
                f,
                "entity {entity}'s `{}` in `{system}` is refused: {error}",
                error.field
            ),
            Self::NoGround => f.write_str(
                "the drop meets nothing in the scene and the ground is not in front of the \
                 camera, so there is nowhere to put it",
            ),
            Self::NotAttached { entity, system } => {
                write!(f, "entity {entity} has no component in `{system}`")
            }
            Self::Attached { entity, system } => {
                write!(f, "entity {entity} already has a component in `{system}`")
            }
            Self::NoComponent(entity) => write!(
                f,
                "entity {entity} would be left with no component; delete it instead"
            ),
            Self::Paste(error) => write!(f, "the clipboard holds no entities: {error}"),
            Self::Name(error) => write!(f, "{error}"),
            Self::FieldPaste { path, message } => {
                write!(f, "the text pasted is no value for `{path}`: {message}")
            }
            Self::Write { key, source } => write!(f, "writing `{key}`: {source}"),
            Self::Occupied { dir, key } => write!(
                f,
                "`{}` already holds `{key}`, and a save into a directory other than the \
                 document's own does not overwrite; name an empty directory",
                dir.display()
            ),
            Self::Target { text, reason } => {
                write!(f, "the scene cannot be saved into `{text}`: {reason}")
            }
            Self::OpenTarget { text, reason } => {
                write!(f, "`{text}` cannot be opened: {reason}")
            }
            Self::Recovery { dir, source } => write!(
                f,
                "making or reading the recovery directory `{}`: {source}",
                dir.display()
            ),
            Self::NotACopy(dir) => write!(
                f,
                "`{}` is not a recovery copy in the recovery directory, so it was not removed",
                dir.display()
            ),
            Self::CopyInUse(dir) => write!(
                f,
                "`{}` is the autosave of an editor still running, so it was not removed",
                dir.display()
            ),
            Self::RemoveCopy { dir, source } => write!(
                f,
                "removing the recovery copy `{}`: {source}",
                dir.display()
            ),
            Self::Remove { key, source } => write!(
                f,
                "removing `{key}`, which the scene no longer names: {source}"
            ),
            Self::NoOrigin => f.write_str(
                "this document was not opened from a directory, so there is nowhere to save \
                 it back to; name a directory",
            ),
            Self::Playing => f.write_str(
                "the scene is in play mode, which refuses edits and saves; stop play mode \
                 first",
            ),
            Self::TickRate(dt) => write!(
                f,
                "the scene's world ticks every {dt} s, which is not a period play mode can \
                 step at"
            ),
            Self::Unplayable(reason) => write!(f, "the scene's game will not play it: {reason}"),
            Self::NotPlaying => {
                f.write_str("the scene is not playing, so there is no game to send that to")
            }
            Self::PlayCommand(reason) => write!(f, "the command was not sent: {reason}"),
            Self::History(error) => write!(
                f,
                "the edit history `{HISTORY}` beside the scene is refused: {error}; remove it to \
                 start a new one"
            ),
        }
    }
}

impl std::error::Error for EditError {}

impl From<ScnError> for EditError {
    fn from(error: ScnError) -> Self {
        Self::Scene(error)
    }
}

impl From<PathError> for EditError {
    fn from(error: PathError) -> Self {
        Self::Path(error)
    }
}

impl Document {
    /// Opens the `.scn/` directory at `dir`, read through `source`, with the
    /// components `registry` knows.
    ///
    /// The world is this document's: the systems are registered here, the
    /// entities are spawned here, and nothing else holds a handle into it. That
    /// is what makes opening again the whole of "revert" and the whole of
    /// [`Document::stop`] — play's restore is this load, run over the text the
    /// scene was saved as when play began, rather than a snapshot of the world.
    ///
    /// **The registry is the vocabulary and there is no other.** The systems a
    /// scene loads into, the codecs its chunks are read with, the component an
    /// edit is applied to and the box a selection is drawn as all come out of the
    /// same entries, so a component registered for one of them cannot be missing
    /// from another.
    ///
    /// # Errors
    ///
    /// [`EditError::Scene`], which names the key or the system it is about: a
    /// file that is not there, text that is not this format, a header this build
    /// does not read, or a manifest naming a system `registry` has no entry for
    /// — which is [`ScnError::NoCodec`], and is a refusal naming that system
    /// rather than a scene opened with a third of its entities missing.
    pub fn open(
        source: &dyn AssetSource,
        dir: &Path,
        registry: Registry,
    ) -> Result<Self, EditError> {
        let (world, scene, ids) = load(source, dir, &registry)?;
        let mut document = Self {
            world,
            scene,
            ids,
            registry,
            selection: Vec::new(),
            log: UndoLog::new(),
            recovered: None,
            recorded_origin: None,
            recovery_notes: Vec::new(),
            history_refusal: None,
            membership: 0,
            naming: 0,
            gestures: 0,
            origin: None,
            owned: BTreeSet::new(),
            play: None,
            assets: Box::new(MemorySource::new()),
            asset_root: origin::AssetRoot::Unset,
            meshes: MeshLibrary::new(),
            mesh_problems: Vec::new(),
            measures: 0,
        };
        document.log.mark_saved();
        document.resolve_meshes();
        Ok(document)
    }

    /// Opens the scene directory at `path` on this machine's filesystem.
    ///
    /// The document remembers where it came from, so [`Document::save`] writes
    /// back over it, and reads its meshes' assets from [`asset_root`] of it
    /// until [`Document::set_assets`] says otherwise. It owns the files the
    /// manifest there names, so a save that stops writing one of them removes
    /// it — and owns nothing else in the directory.
    ///
    /// # Errors
    ///
    /// As [`Document::open`].
    pub fn open_dir(path: impl Into<PathBuf>, registry: Registry) -> Result<Self, EditError> {
        let path = path.into();
        // Rooted at the scene directory itself and read with an empty prefix,
        // which is what `apps/breakout`'s `Board::read_dir` does and for the
        // same reason: `DirSource` refuses an absolute key and a `..`, so the
        // root is how a caller says where the scene is.
        let source = crate::assets::DirSource::at(path.clone());
        let mut document = Self::open(&source, Path::new(""), registry)?;
        document.follow_asset_root(&path);
        // The files the scene as loaded would write are exactly the files its
        // manifest names, spelled the way the writer spells them.
        document.owned = document.files()?.into_keys().collect();
        document.origin = Some(path);
        Ok(document)
    }

    /// The vocabulary this document was opened with.
    #[must_use]
    pub const fn registry(&self) -> &Registry {
        &self.registry
    }

    /// The systems the scene's manifest lists, in the order their chunks are
    /// saved in — the [`outline`](Self::outline)'s groups, empty ones too.
    #[must_use]
    pub fn manifest(&self) -> &[String] {
        self.scene.systems()
    }

    /// The map between the scene's ids and this document's entities.
    #[must_use]
    pub const fn ids(&self) -> &IdMap {
        &self.ids
    }

    /// The world the scene is loaded into: the registry's systems, this
    /// tool's picking physics, and while playing whatever the games' modules
    /// spawned.
    #[must_use]
    pub const fn world(&self) -> &World {
        &self.world
    }

    /// [`world`](Self::world), to write. **A write through it bypasses the
    /// history**, as a write through [`component`](Self::component) does: it
    /// is for reading what a [`Registry`] answers only through a `&mut World`,
    /// and for a caller that adds a system of its own, as a test does.
    pub const fn world_mut(&mut self) -> &mut World {
        &mut self.world
    }

    /// The scene's name, as its header spells it.
    #[must_use]
    pub fn name(&self) -> &str {
        self.scene.name()
    }

    /// Where this document was opened from, if it was opened from a directory.
    #[must_use]
    pub fn origin(&self) -> Option<&Path> {
        self.origin.as_deref()
    }

    /// The entities this document holds, grouped by the system whose chunk file
    /// they came out of and in the order that file spells them — **each entity
    /// once**, under the first system in manifest order that holds it.
    ///
    /// The outliner's rows — `docs/plan/08-editor.md` feature 2's "grouped by
    /// system (the natural shape of scene files)". Once rather than under every
    /// system holding it, because a row is the entity: selecting it selects
    /// the entity, and the inspector shows each of its systems
    /// ([`systems_of`](Self::systems_of)).
    #[must_use]
    pub fn outline(&mut self) -> Vec<(String, Vec<SceneEntityId>)> {
        let systems = self.scene.systems().to_vec();
        // Disjoint field borrows, spelled as two bindings so the closure below
        // does not capture the whole of `self`.
        let ids = &self.ids;
        let world = &mut self.world;
        let registry = &self.registry;
        let mut listed = HashSet::new();
        systems
            .into_iter()
            .map(|system| {
                let entities = registry
                    .entities(world, ids, &system)
                    .into_iter()
                    .filter_map(|entity| ids.id(entity))
                    .filter(|id| listed.insert(*id))
                    .collect();
                (system, entities)
            })
            .collect()
    }

    /// Every entity `system` holds, in the order its chunk file spells them —
    /// those the [`outline`](Self::outline) files under another system too.
    /// Empty for a system the scene does not list.
    #[must_use]
    pub fn entities_in(&mut self, system: &str) -> Vec<SceneEntityId> {
        self.registry
            .entities(&mut self.world, &self.ids, system)
            .into_iter()
            .filter_map(|entity| self.ids.id(entity))
            .collect()
    }

    /// How many entities the document holds.
    #[must_use]
    pub fn entity_count(&self) -> usize {
        self.ids.len()
    }

    /// A number that moves every time an entity enters or leaves the document,
    /// or enters or leaves one of its systems — a spawn, a delete, an attach, a
    /// detach, and any of them undone or redone.
    ///
    /// What a view of the entity list re-reads on. Not
    /// [`entity_count`](Self::entity_count): a delete followed by a duplicate
    /// leaves the count where it was and the list different, and an attach or a
    /// detach can move an entity to another system's group in the
    /// [`outline`](Self::outline) or take away what places it.
    #[must_use]
    pub const fn membership(&self) -> u64 {
        self.membership
    }

    /// The nearest entity `ray` hits, or [`None`] where it hits nothing — or
    /// where the nearest thing it hits is a [`Hit::Spawned`] entity standing
    /// in front, which [`hit`](Self::hit) tells apart.
    ///
    /// **Only entities with a collider pick**, which is
    /// `docs/plan/08-editor.md`'s missing piece 8 and is why
    /// this module's `sync_colliders` runs after a load and after every edit
    /// that moves something.
    #[must_use]
    pub fn pick(&mut self, ray: &Ray) -> Option<SceneEntityId> {
        self.hit(ray)?.scene()
    }

    /// The nearest thing `ray` hits: one of the scene's entities, or one a
    /// playing module spawned into a runtime system a play action picks
    /// from — the only spawned entities given a collider, see
    /// [`picked_runtime`](Self::picked_runtime).
    #[must_use]
    pub fn hit(&mut self, ray: &Ray) -> Option<Hit> {
        let entity = self.world.system_mut::<PhysicsSystem>()?.cast_ray(ray)?.0;
        Some(self.ids.id(entity).map_or(Hit::Spawned(entity), Hit::Scene))
    }

    /// [`hit`](Self::hit), taking the ray a viewport pixel unprojects to.
    ///
    /// The join between [`crate::render::Camera::ray_through`], which answers
    /// in render space's `f32`, and [`PhysicsSystem::cast_ray`], which asks in
    /// simulation space's `f64`. One place, because a widening written twice is
    /// a widening that can disagree with itself.
    #[must_use]
    pub fn hit_ray(&mut self, ray: &ViewRay) -> Option<Hit> {
        self.hit(&Ray::new(widen(ray.origin), widen(ray.direction)))
    }

    /// [`pick`](Self::pick), taking the ray a viewport pixel unprojects to,
    /// through [`hit_ray`](Self::hit_ray).
    #[must_use]
    pub fn pick_ray(&mut self, ray: &ViewRay) -> Option<SceneEntityId> {
        self.hit_ray(ray)?.scene()
    }

    /// Where `id` stands and how far it reaches, in render space — what a
    /// selection's bounds are drawn from.
    ///
    /// [`None`] for an id this document does not hold.
    #[must_use]
    pub fn bounds(&mut self, id: SceneEntityId) -> Option<(Vec3, Vec3)> {
        let entity = self.ids.entity(id)?;
        self.placed(entity)
    }

    /// The entities a playing module spawned that the vocabulary can place —
    /// a [runtime](crate::registry::Registry::register_runtime) component's,
    /// towers' creeps — in the order they are drawn. Empty while editing.
    ///
    /// **Drawn, and nothing else.** They have no [`SceneEntityId`], so the
    /// [`outline`](Self::outline) does not list them, no command can name
    /// them, and a save has no row to write them as; a click does not select
    /// one — only the scene's systems, and a runtime system a running game's
    /// play action picks from, are given colliders, and a click on one of the
    /// latter is a play pick ([`set_runtime_pick`](Self::set_runtime_pick))
    /// rather than a selection; and [`stop`](Self::stop) throws away the world
    /// they are in.
    #[must_use]
    pub fn spawned(&mut self) -> Vec<Entity> {
        self.registry.runtime_entities(&mut self.world)
    }

    /// Where a [`spawned`](Self::spawned) entity stands and how far it
    /// reaches, in render space.
    ///
    /// [`None`] for an entity no longer in the world, and for one of the
    /// scene's — [`bounds`](Self::bounds) answers for those, by id.
    #[must_use]
    pub fn spawned_bounds(&mut self, entity: Entity) -> Option<(Vec3, Vec3)> {
        if self.ids.id(entity).is_some() {
            return None;
        }
        self.placed(entity)
    }

    /// `id`'s placement, turned as its placing component says: the box a
    /// greybox instance is drawn as and the selection outlined by, in
    /// simulation space. [`bounds`](Self::bounds) is the world-axis box
    /// around it.
    ///
    /// [`None`] for an id this document does not hold, and for an entity
    /// nothing places.
    #[must_use]
    pub fn placement(&mut self, id: SceneEntityId) -> Option<OrientedBox> {
        let entity = self.ids.entity(id)?;
        self.registry.placement(&mut self.world, entity)
    }

    /// [`placement`](Self::placement), for a [`spawned`](Self::spawned)
    /// entity: [`None`] for one no longer in the world, and for one of the
    /// scene's.
    #[must_use]
    pub fn spawned_placement(&mut self, entity: Entity) -> Option<OrientedBox> {
        if self.ids.id(entity).is_some() {
            return None;
        }
        self.registry.placement(&mut self.world, entity)
    }

    /// `entity`'s placement as the render-space box with the world's axes
    /// around it: what [`bounds`](Self::bounds) and
    /// [`spawned_bounds`](Self::spawned_bounds) both answer with.
    ///
    /// The centre and the reach are narrowed apart, as they were before a
    /// placement could turn, so an unturned box's corners are the same `f32`s
    /// they always were.
    fn placed(&mut self, entity: Entity) -> Option<(Vec3, Vec3)> {
        let placement = self.registry.placement(&mut self.world, entity)?;
        let (centre, reach) = (narrow(placement.centre), narrow(placement.reach()));
        Some((centre - reach, centre + reach))
    }

    /// What the leaf `path` names inside `id`'s component in `system` currently
    /// holds.
    ///
    /// The read half of an [`EditCommand`], through the same
    /// [`crate::reflect`] path resolution the write goes through: a caller
    /// building a *relative* edit — a nudge — reads the value here and sends an
    /// absolute command, so the command stays exact and its inverse stays
    /// exact with it.
    ///
    /// # Errors
    ///
    /// [`EditError::NoEntity`] for an id this document does not hold,
    /// [`EditError::NotAttached`] for a system that does not hold it, or
    /// [`EditError::Path`] if the path names nothing or stops short of a leaf.
    pub fn read(
        &mut self,
        id: SceneEntityId,
        system: &str,
        path: &str,
    ) -> Result<Value, EditError> {
        Ok(get_path(self.component_of(id, system)?, path)?)
    }

    /// The component the entity `id` names has in `system`, as the editable
    /// value a panel draws: [`crate::registry`]'s `&mut dyn Reflect`, which is
    /// the same one an [`EditCommand`] is applied to.
    ///
    /// [`None`] for an id this document does not hold, and for a system that
    /// does not hold it.
    pub fn component(&mut self, id: SceneEntityId, system: &str) -> Option<&mut dyn Reflect> {
        self.component_of(id, system).ok()
    }

    /// [`component`](Self::component), saying why there is none.
    fn component_of(
        &mut self,
        id: SceneEntityId,
        system: &str,
    ) -> Result<&mut dyn Reflect, EditError> {
        let entity = self.ids.entity(id).ok_or(EditError::NoEntity(id))?;
        self.registry
            .component(&mut self.world, system, entity)
            .ok_or_else(|| EditError::NotAttached {
                entity: id,
                system: system.to_owned(),
            })
    }

    /// Turns the edits a panel **has already made** to `id`'s component in
    /// `system` in one frame into one [`EditCommand`], so that they land in
    /// the log like every other edit: a [`EditCommand::SetProperty`] for one
    /// leaf, and an [`EditCommand::Batch`] of them for several — what a row
    /// writing several leaves at once makes, as the rotation row writes all
    /// four of a quaternion, so one undo puts them all back together. Each of
    /// `switches` — an enum's variant picked in the same frame, which the
    /// panel made after every edit — is an [`EditCommand::SetVariant`] after
    /// them.
    ///
    /// # Why this rewinds first
    ///
    /// [`crate::ui::tree::Ui::inspector`] edits the `&mut dyn Reflect` it is
    /// handed and then *reports* what it did, as a `FieldEdit` of a path, the
    /// value the field held and the value it holds now. By the time a caller
    /// sees that report the write has happened, so handing the new value
    /// straight to [`apply`](Self::apply) would produce an inverse reading
    /// "set it to what it already is" — an undo that undoes nothing, and the
    /// one failure that would pass every test asserting the command was
    /// recorded.
    ///
    /// So each `before` is written back first — the switches newest first,
    /// then the edits newest first, the reverse of the order the panel made
    /// them in — putting the component where the panel found it, and the
    /// command is then applied over the top. **This is the
    /// only field write in the crate**, and it exists to make sure the command
    /// is the thing that does the editing: the value it restores came out of
    /// the same leaf a moment earlier, so nothing is invented and the inverse
    /// the log records is exact.
    ///
    /// # Errors
    ///
    /// [`EditError::NoEntity`] for an id this document does not hold,
    /// [`EditError::NotAttached`] for a system that does not hold it, or
    /// [`EditError::Path`] if the path names nothing in that component or the
    /// leaf refuses either value. A refusal on the way back in leaves the
    /// rewind standing, which is the panel's own `before` and so still a value
    /// the document held. [`EditError::Playing`] in play mode, after the rewind
    /// — so the panel's write does not stand either. [`EditError::Invalid`]
    /// for a value the component's rule refuses, which leaves the rewind
    /// standing too.
    ///
    /// # A drag
    ///
    /// With `gesture`, the command is recorded through
    /// [`apply_in`](Self::apply_in), so a field dragged over many frames — one
    /// report a frame — is one entry whose undo goes back to where the drag
    /// began.
    pub fn record_edits(
        &mut self,
        id: SceneEntityId,
        system: &str,
        edits: &[FieldEdit],
        switches: &[VariantEdit],
        gesture: Option<Gesture>,
    ) -> Result<(), EditError> {
        if edits.is_empty() && switches.is_empty() {
            return Ok(());
        }
        let component = self.component_of(id, system)?;
        for switch in switches.iter().rev() {
            restore_path(component, &switch.path, &switch.before)?;
        }
        for edit in edits.iter().rev() {
            set_path(component, &edit.path, &edit.before)?;
        }
        // After the rewind, so a panel's write into a playing scene is taken
        // back rather than left standing beside the refusal.
        self.refuse_in_play()?;
        let sets = edits.iter().map(|edit| EditCommand::SetProperty {
            entity: id,
            system: system.to_owned(),
            path: edit.path.clone(),
            value: edit.after.clone(),
        });
        let variants = switches.iter().map(|switch| EditCommand::SetVariant {
            entity: id,
            system: system.to_owned(),
            path: switch.path.clone(),
            value: switch.after.clone(),
        });
        let command = EditCommand::one_or_batch(sets.chain(variants).collect());
        match gesture {
            Some(gesture) => self.apply_in(command, gesture),
            None => self.apply(command),
        }
    }

    /// Applies `command` and records it with the inverse it produced.
    ///
    /// The collider of whatever it touched is rebuilt afterwards, because a
    /// brick that moved and a collider that did not is a scene that draws in
    /// one place and picks in another.
    ///
    /// # Errors
    ///
    /// [`EditError::NoEntity`] for an id this document does not hold, or
    /// [`EditError::Path`] carrying the component's own refusal — in which case
    /// nothing was written and nothing was recorded — or [`EditError::Playing`]
    /// in play mode, which writes and records nothing either.
    /// [`EditError::Invalid`] for a property write that would leave a value
    /// its component's rule refuses, which is put back and not recorded.
    pub fn apply(&mut self, command: EditCommand) -> Result<(), EditError> {
        self.refuse_in_play()?;
        let undo = self.perform_valid(&command)?;
        self.resolve_meshes();
        self.log.record(command, undo);
        Ok(())
    }

    /// A new [`Gesture`], distinct from every one before it: what a drag passes
    /// to [`apply_in`](Self::apply_in) for each write it makes.
    pub fn begin_gesture(&mut self) -> Gesture {
        self.gestures += 1;
        Gesture(self.gestures)
    }

    /// [`apply`](Self::apply), as one write of `gesture`: a property set, or a
    /// batch of them, folds into the gesture's entry whatever leaves it names,
    /// so the whole drag is one undo. See [`UndoLog`].
    ///
    /// # Errors
    ///
    /// As [`apply`](Self::apply).
    pub fn apply_in(&mut self, command: EditCommand, gesture: Gesture) -> Result<(), EditError> {
        self.refuse_in_play()?;
        let undo = self.perform_valid(&command)?;
        self.resolve_meshes();
        self.log.record_in(command, undo, gesture);
        Ok(())
    }

    /// Removes every entity of `ids` from the scene, as one entry of an
    /// [`EditCommand::Delete`] each — so one undo brings them all back under
    /// the same ids, components and all. No ids, no entry.
    ///
    /// # Errors
    ///
    /// [`EditError::NoEntity`] for an id this document does not hold, or
    /// [`EditError::Playing`] in play mode, in which case nothing is deleted
    /// and nothing is recorded.
    pub fn delete(&mut self, ids: &[SceneEntityId]) -> Result<(), EditError> {
        if ids.is_empty() {
            return Ok(());
        }
        self.apply(EditCommand::one_or_batch(
            ids.iter()
                .map(|&entity| EditCommand::Delete { entity })
                .collect(),
        ))
    }

    /// Copies every entity of `ids` into a new entity in the same systems,
    /// and returns the new entities' ids in the same order. No ids, no entry.
    ///
    /// An [`EditCommand::Spawn`] per original, of every one of its rows,
    /// under the next ids the document would hand out — one entry, so one
    /// undo takes every copy back. Each copy is its original's component to
    /// the bit and its undo is the spawn's own inverse: there is no duplicate
    /// variant whose inverse could be wrong on its own. A copy stands where
    /// its original does, which is what a caller moving it next expects.
    ///
    /// **A copy is unnamed.** A name says which entity this is, and two
    /// entities answering to "Gate" is the confusion a name exists to end;
    /// numbering it `Gate (2)` would invent a name nobody chose, which a
    /// person then renames anyway. [`paste`](Self::paste) follows the same
    /// rule: a clipping's name comes along only while nothing else bears it.
    ///
    /// # Errors
    ///
    /// [`EditError::NoEntity`] for an id this document does not hold, or
    /// [`EditError::Scene`] if the component would not serialise, or
    /// [`EditError::Playing`] in play mode.
    pub fn duplicate(&mut self, ids: &[SceneEntityId]) -> Result<Vec<SceneEntityId>, EditError> {
        if ids.is_empty() {
            return Ok(Vec::new());
        }
        let first = self.ids.next_id().0;
        let mut copies = Vec::with_capacity(ids.len());
        let mut spawns = Vec::with_capacity(ids.len());
        for (copy, &id) in (first..).map(SceneEntityId).zip(ids) {
            copies.push(copy);
            spawns.push(EditCommand::Spawn {
                entity: copy,
                rows: self.rows(id)?,
                name: None,
            });
        }
        self.apply(EditCommand::one_or_batch(spawns))?;
        Ok(copies)
    }

    /// The clipboard text for every entity of `ids`, in that order: each
    /// one's every system's row and its name, in [`clipboard`]'s
    /// format.
    ///
    /// # Errors
    ///
    /// As [`duplicate`](Self::duplicate).
    pub fn copy(&mut self, ids: &[SceneEntityId]) -> Result<String, EditError> {
        let mut clipped = Vec::with_capacity(ids.len());
        for &id in ids {
            let rows = self.rows(id)?;
            let name = self
                .scene
                .entity_name(id)
                .map(|name| name.as_str().to_owned());
            clipped.push(
                crate::scene_edit::clipboard::Clipped::of(rows, name)
                    .ok_or(EditError::NoEntity(id))?,
            );
        }
        Ok(crate::scene_edit::clipboard::encode(clipped))
    }

    /// Spawns every entity the clipboard text `text` names, under ids this
    /// document hands out fresh, and returns them in the text's order.
    ///
    /// One [`EditCommand::Batch`], so one undo takes the whole paste back — and
    /// a paste with one entity the scene cannot hold spawns none of them. A
    /// clipping's name comes along **only while no entity in the scene bears
    /// it**, nor an earlier one in the same paste — the rule a
    /// [`duplicate`](Self::duplicate) follows, so a copy pasted back into the
    /// scene it came from is unnamed and one pasted into another keeps its name.
    ///
    /// # Errors
    ///
    /// [`EditError::Paste`] if the text is not a clipping, [`EditError::Name`]
    /// if a clipping's name is not a name, and otherwise as a spawn:
    /// [`EditError::NoSystem`] for a system this scene does not list,
    /// [`EditError::Attached`] for a clipping naming one system twice, or
    /// [`EditError::Scene`] for a row that is not that system's component.
    /// [`EditError::Playing`] in play mode, before the text is read.
    pub fn paste(&mut self, text: &str) -> Result<Vec<SceneEntityId>, EditError> {
        // Before the text is read, so a paste into a playing scene says why it
        // was refused rather than what was wrong with the clipboard.
        self.refuse_in_play()?;
        let entities = crate::scene_edit::clipboard::decode(text).map_err(EditError::Paste)?;
        let mut taken: Vec<EntityName> = self.scene.entity_names().values().cloned().collect();
        let first = self.ids.next_id().0;
        let mut ids = Vec::with_capacity(entities.len());
        let mut spawns = Vec::with_capacity(entities.len());
        for (id, clipped) in (first..).map(SceneEntityId).zip(entities) {
            let name = clipped
                .name
                .as_deref()
                .map(EntityName::new)
                .transpose()
                .map_err(EditError::Name)?
                .filter(|name| !taken.contains(name));
            taken.extend(name.clone());
            ids.push(id);
            spawns.push(EditCommand::Spawn {
                entity: id,
                rows: clipped.rows(),
                name,
            });
        }
        if !spawns.is_empty() {
            self.apply(EditCommand::Batch(spawns))?;
        }
        Ok(ids)
    }

    /// Steps back over the most recent applied command.
    ///
    /// Returns whether there was one. The inverse is applied and **not**
    /// recorded: the entry it came from already holds both halves, so the log
    /// is walked rather than grown.
    ///
    /// # Errors
    ///
    /// [`EditError`] if the entity the entry names is not where the log left
    /// it. Every edit goes through the log, so that is a document whose history
    /// was walked out of order — a condition a caller can report rather than a
    /// panic. [`EditError::Playing`] in play mode, leaving the log where it was.
    pub fn undo(&mut self) -> Result<bool, EditError> {
        self.refuse_in_play()?;
        let Some(command) = self.log.undo() else {
            return Ok(false);
        };
        self.replay(&command)?;
        Ok(true)
    }

    /// Steps forward over the entry just above the log's position, if there is
    /// one. Returns whether there was.
    ///
    /// # Errors
    ///
    /// As [`Document::undo`].
    pub fn redo(&mut self) -> Result<bool, EditError> {
        self.refuse_in_play()?;
        let Some(command) = self.log.redo() else {
            return Ok(false);
        };
        self.replay(&command)?;
        Ok(true)
    }

    /// The history.
    #[must_use]
    pub const fn log(&self) -> &UndoLog {
        &self.log
    }

    /// Whether there are edits the document has not been saved at.
    ///
    /// Whether the log stands off the entry the last save was taken at
    /// ([`UndoLog::is_saved`]) — so undoing back to a saved state is clean,
    /// redoing away from it is dirty again, and an edit that drops the saved
    /// entry is dirty until the next save, wherever the log then stands.
    #[must_use]
    pub fn is_dirty(&self) -> bool {
        !self.log.is_saved()
    }

    /// The title bar text: the scene's name with a marker while
    /// [`is_dirty`](Self::is_dirty).
    ///
    /// A leading `*`, which is what every editor that has ever had a title bar
    /// uses and what a person already reads without being told.
    #[must_use]
    pub fn title(&self) -> String {
        let marker = if self.is_dirty() { "*" } else { "" };
        format!("{marker}{} — crcbl editor", self.scene.name())
    }

    /// The files this scene is, keyed relative to the scene directory.
    ///
    /// [`crate::scene::scn::Scene::save`]'s text: byte-identical for equal
    /// scenes on every platform, carrying no timestamp and no editor-session
    /// state. Taking it as text rather than writing it is what lets a test
    /// compare a round trip without a directory.
    ///
    /// # Errors
    ///
    /// [`EditError::Scene`] if an entity attached to a system was never given a
    /// [`SceneEntityId`] — a save that silently dropped such a row is the
    /// failure that refusal exists to prevent.
    pub fn files(&mut self) -> Result<BTreeMap<String, String>, EditError> {
        Ok(self
            .scene
            .save(&mut self.world, &self.ids, &self.registry.codecs())?)
    }

    /// Writes [`files`](Self::files) into the directory at `dir`, and marks the
    /// document saved.
    ///
    /// Through [`crate::store::NativeStorage`] rather than `std::fs` directly:
    /// it refuses a key that would escape the root, creates the `sys/`
    /// subdirectory, and writes through
    /// [`crate::store::write_atomic`] — so an interrupted save leaves the
    /// previous chunk rather than half of a new one.
    ///
    /// **Into the document's own directory** ([`origin`](Self::origin)), a save
    /// then removes every chunk the document owned there and no longer writes
    /// — a system unlisted, the last name cleared — and nothing else: not a file
    /// a person put beside the scene, not a chunk the manifest never named.
    /// The removal comes only after every file landed, so a save that fails
    /// part way loses no chunk the old manifest still names.
    ///
    /// **Into any other directory** it is a copy: it writes nothing if a file it
    /// would write is already there, and it removes nothing. See
    /// `scene_edit::ownership` for both rules.
    ///
    /// # Errors
    ///
    /// [`EditError::Scene`] as [`files`](Self::files), [`EditError::Occupied`]
    /// for a copy that would overwrite, [`EditError::Write`] naming the key that
    /// would not write, or [`EditError::Remove`] naming the one that would not
    /// go. **The document is marked saved only when every file landed and every
    /// file it stopped writing is gone**, so a partial save leaves the dirty
    /// marker up. [`EditError::Playing`] in play mode, writing nothing: a played
    /// state is not the scene that was authored.
    pub fn save_to(&mut self, dir: impl AsRef<Path>) -> Result<(), EditError> {
        self.write(dir.as_ref()).map(|_| ())
    }

    /// [`save_to`](Self::save_to)'s body, handing back the keys it wrote — what
    /// a save-as adopts as the files it owns in its new directory.
    fn write(&mut self, dir: &Path) -> Result<BTreeSet<String>, EditError> {
        self.refuse_in_play()?;
        let files = self.files()?;
        let storage = NativeStorage::at(dir.to_path_buf());
        let own = self
            .origin
            .as_deref()
            .is_some_and(|origin| ownership::same_dir(origin, dir));
        if !own {
            ownership::refuse_occupied(&storage, &files)?;
        }
        for (key, text) in &files {
            storage
                .write(Path::new(key), text.as_bytes())
                .map_err(|source| EditError::Write {
                    key: key.clone(),
                    source,
                })?;
            if own {
                // As each lands rather than after the loop: a save that fails
                // on a later file has still put this one there, and the
                // manifest naming it may be the one on disk now.
                self.owned.insert(key.clone());
            }
        }
        if own {
            ownership::remove_unwritten(&storage, &mut self.owned, &files)?;
        }
        self.log.mark_saved();
        Ok(files.into_keys().collect())
    }

    /// What the scene as it stands would be refused for, read from its own
    /// saved text by [`Registry::problems`]: every row a component's rule
    /// refuses — a value written past [`apply`](Self::apply)'s check, by file,
    /// line and column — and what each registered
    /// [`crate::registry::SceneCheck`] of a game whose systems the scene holds
    /// refuses; or nothing for a scene that reloads and that they would all
    /// play. After them, every mesh whose asset could not be measured
    /// ([`mesh_problems`](Self::mesh_problems)).
    ///
    /// Not a gate on [`save`](Self::save): authoring passes through layouts
    /// no game would load, a corner added before the leg it breaks is
    /// straightened, and a save that refused them would lose the work in
    /// between. A caller reports these instead.
    ///
    /// # Errors
    ///
    /// As [`files`](Self::files).
    pub fn problems(&mut self) -> Result<Vec<String>, EditError> {
        let source = memory_source(self.files()?)?;
        let mut problems = self
            .registry
            .problems(self.scene.systems(), &source, Path::new(""));
        problems.extend(self.mesh_problems());
        Ok(problems)
    }

    /// [`save_to`](Self::save_to) the directory this document was opened from.
    ///
    /// # Errors
    ///
    /// [`EditError::Playing`] in play mode, whatever the document was opened
    /// from; [`EditError::NoOrigin`] for a document that was not opened from a
    /// directory — the compiled-in scene is the case — and otherwise as
    /// [`save_to`](Self::save_to).
    pub fn save(&mut self) -> Result<(), EditError> {
        // Before the origin, so a save in play mode says why whatever the
        // document was opened from.
        self.refuse_in_play()?;
        let dir = self.origin.clone().ok_or(EditError::NoOrigin)?;
        self.save_to(dir)
    }

    /// [`perform`](Self::perform), then puts it back and refuses it if a
    /// property write left a value its component's rule refuses — the body
    /// [`apply`](Self::apply) and [`apply_in`](Self::apply_in) share. See the
    /// module docs of `scene_edit::validation`.
    fn perform_valid(&mut self, command: &EditCommand) -> Result<EditCommand, EditError> {
        let undo = self.perform(command)?;
        if let Err(error) = self.validated(command) {
            self.perform(&undo)
                .expect("an inverse produced a moment ago applies");
            return Err(error);
        }
        self.sync_written(command);
        Ok(undo)
    }

    /// `text` about the entity `id`, as a problem names it: its name and id,
    /// or its id alone for an unnamed one.
    fn about(&self, id: SceneEntityId, text: &str) -> String {
        match self.scene.entity_name(id) {
            Some(name) => format!("entity `{}` #{id}: {text}", name.as_str()),
            None => format!("entity #{id}: {text}"),
        }
    }

    /// Performs `command` without touching the log, and hands back the command
    /// that undoes it — the body [`apply`](Self::apply) and
    /// [`replay`](Self::replay) share.
    fn perform(&mut self, command: &EditCommand) -> Result<EditCommand, EditError> {
        match command {
            EditCommand::SetProperty {
                entity: id,
                system,
                path,
                value,
            } => {
                // The collider is rebuilt by the caller once the whole
                // command stands (`sync_written`): a write the rule refuses,
                // or one leaf of a batch's several, is no box to build.
                Ok(set_property(
                    self.component_of(*id, system)?,
                    *id,
                    system,
                    path,
                    value,
                )?)
            }
            EditCommand::SetVariant {
                entity: id,
                system,
                path,
                value,
            } => Ok(set_variant(
                self.component_of(*id, system)?,
                *id,
                system,
                path,
                value,
            )?),
            EditCommand::SetEnvironment { path, value } => self.set_environment(path, value),
            EditCommand::Spawn {
                entity: id,
                rows,
                name,
            } => self.spawn(*id, rows, name.as_ref()),
            EditCommand::Delete { entity: id } => self.remove(*id),
            EditCommand::Attach {
                entity: id,
                system,
                row,
            } => self.attach_row(*id, system, row),
            EditCommand::Detach { entity: id, system } => self.detach_row(*id, system),
            EditCommand::Rename { entity: id, name } => self.set_name(*id, name.clone()),
            EditCommand::ListSystem { system, at } => self.list_system(system, *at),
            EditCommand::UnlistSystem { system } => self.unlist_system(system),
            EditCommand::Batch(commands) => {
                let mut undo = Vec::with_capacity(commands.len());
                for command in commands {
                    match self.perform(command) {
                        Ok(inverse) => undo.push(inverse),
                        Err(error) => {
                            for inverse in undo.iter().rev() {
                                self.perform(inverse)
                                    .expect("an inverse produced a moment ago applies");
                            }
                            return Err(error);
                        }
                    }
                }
                undo.reverse();
                Ok(EditCommand::Batch(undo))
            }
        }
    }

    /// Creates `id` with a component per row of `rows`, called `name`, and
    /// hands back the delete that undoes it.
    ///
    /// Every row's system is checked before anything is spawned, and a row
    /// that will not read takes the whole entity back out — every system it
    /// had joined with it — so a refused spawn leaves nothing.
    fn spawn(
        &mut self,
        id: SceneEntityId,
        rows: &[SystemRow],
        name: Option<&EntityName>,
    ) -> Result<EditCommand, EditError> {
        if self.ids.entity(id).is_some() {
            return Err(EditError::IdInUse(id));
        }
        if rows.is_empty() {
            return Err(EditError::NoComponent(id));
        }
        let mut codecs = Vec::with_capacity(rows.len());
        for (index, row) in rows.iter().enumerate() {
            if rows[..index]
                .iter()
                .any(|earlier| earlier.system == row.system)
            {
                return Err(EditError::Attached {
                    entity: id,
                    system: row.system.clone(),
                });
            }
            codecs.push(self.listed_codec(&row.system)?);
        }
        let entity = self.world.spawn();
        for (codec, row) in codecs.iter().zip(rows) {
            if let Err(error) = codec.attach_row(&mut self.world, entity, &row.row) {
                self.world.despawn(entity);
                self.world.sweep();
                return Err(error.into());
            }
        }
        // The id was free a moment ago and the entity is this call's own, so
        // both halves of the map are empty for them.
        assert!(
            self.ids.restore(id, entity),
            "a fresh entity under a free id is always filed",
        );
        // A free id holds no name — a delete took it with the entity — so
        // this names the entity or leaves it unnamed, and replaces nothing.
        self.scene.set_entity_name(id, name.cloned());
        sync_colliders(&self.registry, &mut self.world, [entity]);
        self.membership += 1;
        Ok(EditCommand::Delete { entity: id })
    }

    /// Removes `id` from the scene, and hands back the spawn that undoes it: the
    /// same id, every system's row read immediately before it went, and its
    /// name.
    ///
    /// The name goes with the entity: a scene holding a name for an id it does
    /// not hold is one [`Scene::save`] refuses to write.
    fn remove(&mut self, id: SceneEntityId) -> Result<EditCommand, EditError> {
        let rows = self.rows(id)?;
        let entity = self.ids.entity(id).ok_or(EditError::NoEntity(id))?;
        self.world.despawn(entity);
        // Swept now rather than at the end of a tick, because nothing ticks
        // while the scene is edited: a despawned entity stays in every system
        // until a sweep, drawn, picked and saved.
        self.world.sweep();
        self.ids.remove(id);
        let name = self.scene.set_entity_name(id, None);
        self.membership += 1;
        self.prune_selection();
        Ok(EditCommand::Spawn {
            entity: id,
            rows,
            name,
        })
    }

    /// Every system holding `id`, in manifest order, with its component as one
    /// row's text — what a delete's undo, a duplicate and a copy are built
    /// from. Never empty: an id no system holds is [`EditError::NoEntity`].
    ///
    /// # Errors
    ///
    /// [`EditError::NoEntity`] for an id this document does not hold, and
    /// [`EditError::Scene`] for a component that would not serialise.
    pub fn rows(&mut self, id: SceneEntityId) -> Result<Vec<SystemRow>, EditError> {
        let entity = self.ids.entity(id).ok_or(EditError::NoEntity(id))?;
        let mut rows = Vec::new();
        for system in self.systems_of(id) {
            let codec = self.listed_codec(&system)?;
            let row =
                codec
                    .row(&mut self.world, entity)?
                    .ok_or_else(|| EditError::NotAttached {
                        entity: id,
                        system: system.clone(),
                    })?;
            rows.push(SystemRow { system, row });
        }
        // An id still in the map whose entity no system holds — one a playing
        // module despawned — has nothing to copy or bring back.
        if rows.is_empty() {
            return Err(EditError::NoEntity(id));
        }
        Ok(rows)
    }

    /// Applies a command the log handed back, discarding the inverse: the entry
    /// it came from is already holding the other half.
    fn replay(&mut self, command: &EditCommand) -> Result<(), EditError> {
        self.perform(command)?;
        self.sync_written(command);
        self.resolve_meshes();
        Ok(())
    }
}

/// A world of `registry`'s systems and this tool's picking physics, with the
/// scene at `dir`, read through `source`, loaded into it and every entity given
/// its collider — what [`Document::open`] and play's restore both build.
fn load(
    source: &dyn AssetSource,
    dir: &Path,
    registry: &Registry,
) -> Result<(World, Scene, IdMap), EditError> {
    let mut world = World::new();
    registry.register_systems(&mut world);
    // Every pick goes through it, and an entity with no collider does not
    // pick. Registered here rather than by the registry: a scene's chunks are
    // the registry's business and the way this tool selects things is not.
    world.register_system(Box::new(PhysicsSystem::new()));

    let (scene, ids) = Scene::load(source, dir, &registry.codecs(), &mut world)?;
    sync_scene_colliders(registry, &mut world, &scene, &ids);
    Ok((world, scene, ids))
}

/// [`sync_colliders`] over every entity the scene's systems hold: after a load,
/// and after a tick of play has moved whatever its modules moved.
fn sync_scene_colliders(registry: &Registry, world: &mut World, scene: &Scene, ids: &IdMap) {
    for system in scene.systems() {
        let entities = registry.entities(world, ids, system);
        sync_colliders(registry, world, entities);
    }
}

/// `files`, keyed the way a source rooted at the scene directory reads them —
/// a scene in memory, for a load or a check that wants no directory.
fn memory_source(files: BTreeMap<String, String>) -> Result<MemorySource, EditError> {
    let mut source = MemorySource::new();
    for (key, text) in files {
        source
            .insert(Path::new(&key), text.into_bytes())
            .map_err(|source| EditError::Write { key, source })?;
    }
    Ok(source)
}

/// Gives every entity in `entities` the collider a ray picks it by, replacing any
/// it already had, from the [`crate::registry::Placement`]
/// [`Registry::placement`] answers for it — and takes the collider of one it
/// answers none for.
///
/// Called once after a load and again after any edit that moved or resized the
/// thing edited — **not** only after a load. A collider left where the entity
/// used to be is the failure this exists to prevent, and it is one a picture
/// would not show: the thing draws in its new place and picks in its old one.
///
/// An entity none of whose components is a thing in space — `apps/puppet`'s
/// `Sun` is the case — has no collider and so cannot be picked, which is the
/// honest answer rather than a box at the origin. That includes one whose
/// placing component was just detached: a collider left behind would pick a
/// thing nothing draws.
///
/// Kinematic bodies: the body is what the broadphase tracks, and nothing moves
/// it but this — no velocity is ever given to one, so a tick of play leaves it
/// where the last sync put it.
///
/// **These are picking boxes, never simulated bodies.** A scene that plays
/// with [`crate::scene_physics`] simulates its bodies in that module's own
/// [`Simulation`](crate::scene_physics::Simulation) system, a type of its own,
/// so the [`PhysicsSystem`] found here is still this tool's and setting a
/// kinematic box cannot overwrite a body being simulated. The simulation
/// writes each body's pose into its placing component, and the sync after a
/// tick reads it from there like any other move.
fn sync_colliders(
    registry: &Registry,
    world: &mut World,
    entities: impl IntoIterator<Item = Entity>,
) {
    let placements: Vec<(Entity, Option<OrientedBox>)> = entities
        .into_iter()
        .map(|entity| (entity, registry.placement(world, entity)))
        .collect();
    let Some(phys) = world.system_mut::<PhysicsSystem>() else {
        return;
    };
    for (entity, placement) in placements {
        let Some(placement) = placement.filter(|placement| {
            let buildable = can_pick_by(placement);
            if !buildable {
                crate::log::warn!(
                    "editor: {entity:?} is placed as a box no collider can be — \
                     {placement:?}; it cannot be picked until its component is fixed"
                );
            }
            buildable
        }) else {
            phys.remove_entity(entity);
            continue;
        };
        let transform = Transform::new(placement.centre, placement.rotation);
        phys.set_body(entity, RigidBody::new_kinematic());
        phys.set_transform(entity, transform);
        // A box on the turned transform turns with it in the query world, so
        // a ray picks the turned box itself.
        phys.set_collider(
            entity,
            &ColliderComponent::Box {
                offset: DVec3::ZERO,
                half_extents: placement.half_extents,
                is_trigger: false,
            },
            &transform,
        );
    }
}

/// Whether `placement` is a box the physics can build a collider for: every
/// number finite and no half extent below zero.
///
/// A registered component's rule ([`crate::registry::Validate`]) should keep
/// any other placement out of a document, but a component without one — or a
/// value written past the commands — would otherwise reach `BoxCollider::new`'s
/// assertion and take the editor down. Such an entity is left unpickable
/// instead, and the warning names it.
fn can_pick_by(placement: &OrientedBox) -> bool {
    placement.centre.is_finite()
        && placement.half_extents.is_finite()
        && placement.half_extents.min_element() >= 0.0
        && placement.rotation.is_finite()
}

/// Simulation space's `f64`, from render space's `f32`.
fn widen(value: Vec3) -> DVec3 {
    DVec3::new(f64::from(value.x), f64::from(value.y), f64::from(value.z))
}

/// Render space's `f32`, from simulation space's `f64`.
///
/// The lossy direction, and the safe one to be lossy in: this is what a picture
/// is drawn from, and the engine's `WorldPos` rule (`docs/notes/backends.md`) is
/// that plain `Vec3` is only ever camera-relative render space.
#[allow(clippy::cast_possible_truncation)]
fn narrow(value: DVec3) -> Vec3 {
    Vec3::new(value.x as f32, value.y as f32, value.z as f32)
}

/// The marker of a game's root: the manifest `crcbl new` writes and every
/// sample has.
const PROJECT_MARKER: &str = "Cargo.toml";

/// Where a scene at `scene` reads its meshes' assets from when the command
/// line names no `--assets`: the nearest directory above it holding a
/// `Cargo.toml` — the game's own root — or, outside any project, the
/// directory holding the scene.
///
/// The game's root rather than the scene's directory so an asset key keeps
/// naming the same file when a scene moves between the game's folders: a key
/// relative to the scene would break on every such move (decided 2026-10-01).
pub fn asset_root(scene: &Path) -> PathBuf {
    let scene = std::path::absolute(scene).unwrap_or_else(|_| scene.to_path_buf());
    let holding = scene.parent().map(Path::to_path_buf).unwrap_or_default();
    holding
        .ancestors()
        .find(|dir| dir.join(PROJECT_MARKER).is_file())
        .map_or(holding.clone(), Path::to_path_buf)
}

#[cfg(test)]
mod tests;

#[cfg(test)]
mod history_tests;
