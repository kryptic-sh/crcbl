//! The thing being edited: a scene, the world it was loaded into, what is
//! selected, and the history of what has been done to it.
//!
//! Everything in this module runs **without a device and without a window**,
//! which is `docs/plan/08-editor.md`'s "nothing editor-side may be implemented
//! GUI-only" applied to the first slice: the pick, the command, its inverse and
//! the save are all methods here, and [`crate::app`] is a loop that calls them.
//! The gates hold this half; the windowed harness holds the other.
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
//!   [`Document::save_to`] the same text written to a directory.
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

use std::collections::{BTreeMap, HashSet};
use std::fmt;
use std::path::{Path, PathBuf};

use crcbl::assets::{AssetSource, MemorySource};
use crcbl::ecs::{Entity, World};
use crcbl::math::{DVec3, Vec3};
use crcbl::phys::{ColliderComponent, PhysicsSystem, Ray, RigidBody, Transform};
use crcbl::reflect::{PathError, Reflect, Value, get_path, set_path};
use crcbl::registry::Registry;
use crcbl::render::ViewRay;
use crcbl::scene::scn::{EntityName, IdMap, NameError, Scene, SceneEntityId, ScnError};
use crcbl::store::{NativeStorage, StorageError, StorageSource};

use crate::command::{EditCommand, Gesture, SystemRow, UndoLog, set_property};

mod field;
mod naming;
mod play;
mod systems;

pub use play::PlayState;

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
    /// pointers per component — see [`crcbl::registry::Registry`].
    registry: Registry,
    /// Which [`SceneEntityId`] is selected, if any. An id rather than an
    /// [`Entity`] for [`EditCommand`]'s reason: it is what survives a reload.
    selected: Option<SceneEntityId>,
    log: UndoLog,
    /// The log position the document was last written at. `0` for one that has
    /// been loaded and not saved, which is also where a fresh log stands — so a
    /// document nobody has edited opens clean.
    saved_at: usize,
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
    /// The scene as it stood when play began, and what is running it — or
    /// [`None`] while editing. See [`Document::play`].
    play: Option<play::Session>,
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

    /// A spawn or an attach named a system this scene's manifest does not
    /// list, or one the document's vocabulary cannot read a row of.
    ///
    /// Refused rather than attached: a save writes the manifest's chunks and no
    /// others, so the row would be dropped by the next one.
    NoSystem(String),

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
    Paste(crcbl::ron::error::SpannedError),

    /// A rename, or a clipping's name, is not a name: empty where one was
    /// wanted, too long, or holding a control character.
    Name(NameError),

    /// A field paste's text is not a value of the field's kind, read the way
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
    /// which a module's [`register`](crcbl::ecs::GameModule::register) set.
    TickRate(f64),

    /// A game whose system the scene lists refused to play it, saying why —
    /// towers' path with a diagonal leg is the case. Play does not start.
    Unplayable(String),
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
                write!(f, "the clipboard holds no value for `{path}`: {message}")
            }
            Self::Write { key, source } => write!(f, "writing `{key}`: {source}"),
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
        Ok(Self {
            world,
            scene,
            ids,
            registry,
            selected: None,
            log: UndoLog::new(),
            saved_at: 0,
            membership: 0,
            naming: 0,
            gestures: 0,
            origin: None,
            play: None,
        })
    }

    /// Opens the scene directory at `path` on this machine's filesystem.
    ///
    /// The document remembers where it came from, so [`Document::save`] writes
    /// back over it.
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
        let source = crcbl::assets::DirSource::at(path.clone());
        let mut document = Self::open(&source, Path::new(""), registry)?;
        document.origin = Some(path);
        Ok(document)
    }

    /// Opens this build's compiled-in scene — the document the editor starts on
    /// when it is not given one.
    ///
    /// [`crate::scene`]'s greybox blocks, through that module's own vocabulary,
    /// out of a compiled-in source rather than off disk: a tool that had to find
    /// its own default document would be one whose behaviour depended on the
    /// directory it was started from. It has no [`origin`](Document::save), so
    /// saving it means naming a directory.
    ///
    /// # Errors
    ///
    /// [`EditError::Scene`] if the compiled-in scene is not readable, which is a
    /// tree in which `crate::scene`'s own tests are red too.
    pub fn built_in() -> Result<Self, EditError> {
        Self::open(
            &crate::scene::built_in_source(),
            Path::new(crate::scene::GREYBOX),
            crate::scene::vocabulary(),
        )
    }

    /// The vocabulary this document was opened with.
    #[must_use]
    pub const fn registry(&self) -> &Registry {
        &self.registry
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

    /// What is selected.
    #[must_use]
    pub const fn selected(&self) -> Option<SceneEntityId> {
        self.selected
    }

    /// Selects `id`, or clears the selection with [`None`].
    ///
    /// An id the document does not hold selects nothing, so a stale id from
    /// before a reload cannot leave a selection pointing at a hole.
    pub fn select(&mut self, id: Option<SceneEntityId>) {
        self.selected = id.filter(|id| self.ids.entity(*id).is_some());
    }

    /// The nearest entity `ray` hits, or [`None`] where it hits nothing.
    ///
    /// **Only entities with a collider pick**, which is
    /// `docs/plan/08-editor.md`'s missing piece 8 and is why
    /// this module's `sync_colliders` runs after a load and after every edit
    /// that moves something.
    #[must_use]
    pub fn pick(&mut self, ray: &Ray) -> Option<SceneEntityId> {
        let entity = self.world.system_mut::<PhysicsSystem>()?.cast_ray(ray)?.0;
        self.ids.id(entity)
    }

    /// [`pick`](Self::pick), taking the ray a viewport pixel unprojects to.
    ///
    /// The join between [`crcbl::render::Camera::ray_through`], which answers
    /// in render space's `f32`, and [`PhysicsSystem::cast_ray`], which asks in
    /// simulation space's `f64`. One place, because a widening written twice is
    /// a widening that can disagree with itself.
    #[must_use]
    pub fn pick_ray(&mut self, ray: &ViewRay) -> Option<SceneEntityId> {
        self.pick(&Ray::new(widen(ray.origin), widen(ray.direction)))
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
    /// a [runtime](crcbl::registry::Registry::register_runtime) component's,
    /// towers' creeps — in the order they are drawn. Empty while editing.
    ///
    /// **Drawn, and nothing else.** They have no [`SceneEntityId`], so the
    /// [`outline`](Self::outline) does not list them, no command can name
    /// them, and a save has no row to write them as; a click does not select
    /// one, because only the scene's systems are given colliders; and
    /// [`stop`](Self::stop) throws away the world they are in.
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

    /// `entity`'s placement as a render-space box: what [`bounds`](Self::bounds)
    /// and [`spawned_bounds`](Self::spawned_bounds) both answer with.
    fn placed(&mut self, entity: Entity) -> Option<(Vec3, Vec3)> {
        let (centre, half) = self.registry.placement(&mut self.world, entity)?;
        let (centre, half) = (narrow(centre), narrow(half));
        Some((centre - half, centre + half))
    }

    /// What the leaf `path` names inside `id`'s component in `system` currently
    /// holds.
    ///
    /// The read half of an [`EditCommand`], through the same
    /// [`crcbl::reflect`] path resolution the write goes through: a caller
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
    /// value a panel draws: [`crcbl::registry`]'s `&mut dyn Reflect`, which is
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

    /// Turns an edit a panel **has already made** into an [`EditCommand`], so
    /// that it lands in the log like every other edit.
    ///
    /// # Why this rewinds first
    ///
    /// [`crcbl::ui::tree::Ui::inspector`] edits the `&mut dyn Reflect` it is
    /// handed and then *reports* what it did, as a `FieldEdit` of a path, the
    /// value the field held and the value it holds now. By the time a caller
    /// sees that report the write has happened, so handing the new value
    /// straight to [`apply`](Self::apply) would produce an inverse reading
    /// "set it to what it already is" — an undo that undoes nothing, and the
    /// one failure that would pass every test asserting the command was
    /// recorded.
    ///
    /// So `before` is written back first, putting the component where the panel
    /// found it, and the command is then applied over the top. **This is the
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
    /// — so the panel's write does not stand either.
    ///
    /// # A drag
    ///
    /// With `gesture`, the command is recorded through
    /// [`apply_in`](Self::apply_in), so a field dragged over many frames — one
    /// report a frame — is one entry whose undo goes back to where the drag
    /// began.
    pub fn record_edit(
        &mut self,
        id: SceneEntityId,
        system: &str,
        path: &str,
        before: &Value,
        after: &Value,
        gesture: Option<Gesture>,
    ) -> Result<(), EditError> {
        set_path(self.component_of(id, system)?, path, before)?;
        // After the rewind, so a panel's write into a playing scene is taken
        // back rather than left standing beside the refusal.
        self.refuse_in_play()?;
        let command = EditCommand::SetProperty {
            entity: id,
            system: system.to_owned(),
            path: path.to_owned(),
            value: after.clone(),
        };
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
    pub fn apply(&mut self, command: EditCommand) -> Result<(), EditError> {
        self.refuse_in_play()?;
        let undo = self.perform(&command)?;
        self.log.record(command, undo);
        Ok(())
    }

    /// A new [`Gesture`], distinct from every one before it: what a drag passes
    /// to [`apply_in`](Self::apply_in) for each write it makes.
    pub fn begin_gesture(&mut self) -> Gesture {
        self.gestures += 1;
        Gesture(self.gestures)
    }

    /// [`apply`](Self::apply), as one write of `gesture`: a property set of the
    /// leaf the gesture's last write set folds into that write's entry, so the
    /// whole drag is one undo. See [`UndoLog`].
    ///
    /// # Errors
    ///
    /// As [`apply`](Self::apply).
    pub fn apply_in(&mut self, command: EditCommand, gesture: Gesture) -> Result<(), EditError> {
        self.refuse_in_play()?;
        let undo = self.perform(&command)?;
        self.log.record_in(command, undo, gesture);
        Ok(())
    }

    /// Removes `id` from the scene, as an [`EditCommand::Delete`] — so an undo
    /// brings it back under the same id, component and all.
    ///
    /// # Errors
    ///
    /// [`EditError::NoEntity`] for an id this document does not hold, or
    /// [`EditError::Playing`] in play mode, in which case nothing is recorded.
    pub fn delete(&mut self, id: SceneEntityId) -> Result<(), EditError> {
        self.apply(EditCommand::Delete { entity: id })
    }

    /// Copies `id` into a new entity in the same systems, and returns the new
    /// entity's id.
    ///
    /// An [`EditCommand::Spawn`] of every one of the original's rows under the
    /// next id the
    /// document would hand out, so the copy is the original's component to the
    /// bit and its undo is the spawn's own inverse — there is no duplicate
    /// variant whose inverse could be wrong on its own. The copy stands where
    /// the original does, which is what a caller moving it next expects.
    ///
    /// **The copy is unnamed.** A name says which entity this is, and two
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
    pub fn duplicate(&mut self, id: SceneEntityId) -> Result<SceneEntityId, EditError> {
        let rows = self.rows(id)?;
        let copy = self.ids.next_id();
        self.apply(EditCommand::Spawn {
            entity: copy,
            rows,
            name: None,
        })?;
        Ok(copy)
    }

    /// The clipboard text for `id`: every system's row and its name, in
    /// [`crate::clipboard`]'s format.
    ///
    /// # Errors
    ///
    /// As [`duplicate`](Self::duplicate).
    pub fn copy(&mut self, id: SceneEntityId) -> Result<String, EditError> {
        let rows = self.rows(id)?;
        let name = self
            .scene
            .entity_name(id)
            .map(|name| name.as_str().to_owned());
        let clipped = crate::clipboard::Clipped::of(rows, name).ok_or(EditError::NoEntity(id))?;
        Ok(crate::clipboard::encode(vec![clipped]))
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
        let entities = crate::clipboard::decode(text).map_err(EditError::Paste)?;
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
    /// The log's position against the position of the last save — so undoing
    /// back to a saved state is clean, and redoing away from it is dirty again.
    #[must_use]
    pub const fn is_dirty(&self) -> bool {
        self.log.position() != self.saved_at
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
    /// [`crcbl::scene::scn::Scene::save`]'s text: byte-identical for equal
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
    /// Through [`crcbl::store::NativeStorage`] rather than `std::fs` directly:
    /// it refuses a key that would escape the root, creates the `sys/`
    /// subdirectory, and writes through
    /// [`crcbl::store::write_atomic`] — so an interrupted save leaves the
    /// previous chunk rather than half of a new one.
    ///
    /// # Errors
    ///
    /// [`EditError::Scene`] as [`files`](Self::files), or [`EditError::Write`]
    /// naming the key that would not write. **The document is marked saved only
    /// when every file landed**, so a partial write leaves the dirty marker up.
    /// [`EditError::Playing`] in play mode, writing nothing: a played state is
    /// not the scene that was authored.
    pub fn save_to(&mut self, dir: impl AsRef<Path>) -> Result<(), EditError> {
        self.refuse_in_play()?;
        let files = self.files()?;
        let storage = NativeStorage::at(dir.as_ref().to_path_buf());
        for (key, text) in &files {
            storage
                .write(Path::new(key), text.as_bytes())
                .map_err(|source| EditError::Write {
                    key: key.clone(),
                    source,
                })?;
        }
        self.saved_at = self.log.position();
        // A drag carried on past the save writes an entry of its own, so the
        // document is dirty again rather than folding the change into the
        // entry the save stands on.
        self.log.seal();
        Ok(())
    }

    /// What the games whose systems this scene holds would refuse in it as it
    /// stands — each registered [`crcbl::registry::SceneCheck`] run over the
    /// scene's own saved text — or nothing for a scene they would all play.
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
        Ok(self
            .registry
            .problems(self.scene.systems(), &source, Path::new("")))
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
                let undo = set_property(self.component_of(*id, system)?, *id, system, path, value)?;
                let entity = self.ids.entity(*id).ok_or(EditError::NoEntity(*id))?;
                sync_colliders(&self.registry, &mut self.world, [entity]);
                Ok(undo)
            }
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
        if self.selected == Some(id) {
            self.selected = None;
        }
        Ok(EditCommand::Spawn {
            entity: id,
            rows,
            name,
        })
    }

    /// Every system holding `id`, in manifest order, with its component as one
    /// row's text — what a delete's undo, a duplicate and a copy are built
    /// from. Never empty: an id no system holds is [`EditError::NoEntity`].
    fn rows(&mut self, id: SceneEntityId) -> Result<Vec<SystemRow>, EditError> {
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
/// it already had, from the [`crcbl::registry::Placement`]
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
fn sync_colliders(
    registry: &Registry,
    world: &mut World,
    entities: impl IntoIterator<Item = Entity>,
) {
    let placements: Vec<(Entity, Option<(DVec3, DVec3)>)> = entities
        .into_iter()
        .map(|entity| (entity, registry.placement(world, entity)))
        .collect();
    let Some(phys) = world.system_mut::<PhysicsSystem>() else {
        return;
    };
    for (entity, placement) in placements {
        let Some((centre, half_extents)) = placement else {
            phys.remove_entity(entity);
            continue;
        };
        let transform = Transform::from_position(centre);
        phys.set_body(entity, RigidBody::new_kinematic());
        phys.set_transform(entity, transform);
        phys.set_collider(
            entity,
            &ColliderComponent::Box {
                offset: DVec3::ZERO,
                half_extents,
                is_trigger: false,
            },
            &transform,
        );
    }
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

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod entity_tests;

#[cfg(test)]
mod field_tests;

#[cfg(test)]
mod naming_tests;

#[cfg(test)]
pub(crate) mod play_tests;

#[cfg(test)]
pub(crate) mod systems_tests;

#[cfg(test)]
mod towers_play_tests;

#[cfg(test)]
mod tests {
    use super::*;

    /// The document every test here opens: this build's compiled-in greybox
    /// scene, so nothing below depends on a working directory **or on a game** —
    /// `apps/editor/tests/vocabularies.rs` is where the samples' own scenes are
    /// opened through this module.
    fn document() -> Document {
        Document::built_in().expect("the compiled-in scene is a scene")
    }

    /// The id of the step a test picks, and the `(x, y)` a ray down `-Z` hits it
    /// at: `crate::scene`'s three steps, whose centres are these and whose gaps
    /// are between them.
    const STEPS: [(u32, f64, f64); 3] = [(1, -3.0, 0.25), (2, 0.0, 0.75), (3, 3.0, 1.25)];

    /// `(x, y)` in the gap between the first two steps, at the first one's
    /// height: outside both, and above the ground slab whose top is `y = 0`.
    const GAP: (f64, f64) = (-1.5, 0.25);

    /// A command that moves `id` along `axis` by `delta`, read off the
    /// document so the *replaced* value is the one actually in the component.
    fn nudge(document: &mut Document, id: SceneEntityId, axis: usize, delta: f64) -> EditCommand {
        let (min, max) = document.bounds(id).expect("a block in this document");
        let centre = (min + max) * 0.5;
        let was = f64::from([centre.x, centre.y, centre.z][axis]);
        EditCommand::SetProperty {
            entity: id,
            system: crate::scene::BLOCKS.to_owned(),
            path: format!("position.{axis}"),
            value: Value::Float(was + delta),
        }
    }

    /// A ray that comes down `-Z` at `(x, y)`, which is how the greybox scene is
    /// looked at — every step's `position.2` is 0 and its half extent on that
    /// axis is 1.5, so a ray from `z = 20` meets every one of them.
    fn ray_at(x: f64, y: f64) -> Ray {
        Ray::new(DVec3::new(x, y, 20.0), DVec3::NEG_Z)
    }

    /// **The compiled-in scene loads, grouped by the system its chunk came out
    /// of, in file order.**
    #[test]
    fn the_built_in_scene_opens_with_every_block_the_file_names() {
        let mut document = document();
        assert_eq!(document.name(), "greybox");
        let outline = document.outline();
        assert_eq!(outline.len(), 1, "the manifest names one system");
        assert_eq!(outline[0].0, crate::scene::BLOCKS);
        assert_eq!(
            outline[0].1,
            (0..4).map(SceneEntityId).collect::<Vec<_>>(),
            "the outline is not the file's own ids in the file's own order",
        );
        assert_eq!(document.entity_count(), outline[0].1.len());
        assert!(!document.is_dirty(), "a document nobody edited opens clean");
    }

    /// **The ray hits the entity under the cursor and not its neighbour.**
    ///
    /// Every step is aimed at by name rather than "something was hit": they are
    /// 2.4 m wide on a 3.0 m pitch, so a ray 3 m along the row has to come back
    /// with the *next* id — and a pick that quietly answered "the nearest entity
    /// in the world" would pass a test that only asserted a hit.
    #[test]
    fn a_ray_picks_the_step_it_points_at_and_not_the_one_beside_it() {
        let mut document = document();
        for (id, x, y) in STEPS {
            // The constant is checked against the file before it is used with
            // it, so a step that moved in `crate::scene` is red here rather than
            // a ray that quietly aims at nothing in particular.
            let (min, max) = document
                .bounds(SceneEntityId(id))
                .unwrap_or_else(|| panic!("step {id} is in the document"));
            let centre = (min + max) * 0.5;
            assert!(
                (f64::from(centre.x) - x).abs() < 1e-5 && (f64::from(centre.y) - y).abs() < 1e-5,
                "step {id} is at {centre:?}, not at ({x}, {y})",
            );
            assert_eq!(
                document.pick(&ray_at(x, y)),
                Some(SceneEntityId(id)),
                "a ray down step {id} picked something else",
            );
        }
    }

    /// A ray down the **gap between two steps** picks nothing, which is the half
    /// that says the pick is a ray and not a nearest-entity query: the same ray
    /// 1.5 m either way hits a step.
    #[test]
    fn a_ray_down_the_gap_between_two_steps_picks_nothing() {
        let mut document = document();
        let (x, y) = GAP;
        assert_eq!(document.pick(&ray_at(x, y)), None);
        assert!(document.pick(&ray_at(x - 1.5, y)).is_some());
        assert!(document.pick(&ray_at(x + 1.5, y)).is_some());
    }

    /// A ray that points at nothing picks nothing, rather than the nearest
    /// thing anywhere.
    #[test]
    fn a_ray_that_misses_the_scene_picks_nothing() {
        let mut document = document();
        assert!(document.pick(&ray_at(0.0, 400.0)).is_none());
    }

    /// **The collider follows the edit**, which is what makes a second pick
    /// find the entity where it now is rather than where it was.
    ///
    /// The observable is a *pick*, not a field: an edit that wrote the
    /// component and skipped [`sync_colliders`] would satisfy every
    /// assertion about `position` and still hand the wrong entity back to the
    /// next click.
    #[test]
    fn a_moved_block_is_picked_where_it_now_is() {
        let mut document = document();
        let (_, x, y) = STEPS[0];
        let id = document.pick(&ray_at(x, y)).expect("the first step");

        // Straight up, far enough to clear its own half extent and land
        // somewhere the scene has no other row.
        const LIFT: f64 = 40.0;
        let command = nudge(&mut document, id, 1, LIFT);
        document.apply(command).expect("a block has a y");

        assert!(
            document.pick(&ray_at(x, y)).is_none(),
            "the block still picks where it used to be",
        );
        assert_eq!(
            document.pick(&ray_at(x, y + LIFT)),
            Some(id),
            "and does not pick where it now is",
        );
    }

    /// **Load → save with no edits is byte-identical.**
    ///
    /// Against the source's own files rather than against a second save: a
    /// writer that was merely self-consistent would pass that, and the claim is
    /// that opening a scene and saving it changes nothing on disk.
    #[test]
    fn a_load_and_a_save_with_no_edits_is_byte_identical() {
        let mut document = document();
        let written = document.files().expect("every block has an id");
        let source = crate::scene::built_in_source();
        for (key, text) in &written {
            let committed = source
                .read(Path::new(&format!("{}/{key}", crate::scene::GREYBOX)))
                .unwrap_or_else(|error| panic!("the compiled-in {key}: {error}"));
            assert_eq!(
                text.as_bytes(),
                committed.as_slice(),
                "a load-save round trip changed {key}",
            );
        }
        assert_eq!(
            written.keys().collect::<Vec<_>>(),
            ["env.ron", "scene.ron", "sys/blocks.ron"]
                .iter()
                .collect::<Vec<_>>(),
            "the save wrote a different set of files",
        );
    }

    /// **Load → edit → save changes exactly the edited bytes.**
    ///
    /// Line by line against the untouched save, and then **component by
    /// component within the one line that moved** — so the assertion is not
    /// "something changed" but "the `y` of that row changed to that value, and
    /// its `x` and `z` did not". A command that wrote a neighbouring axis
    /// rewrites the same single line and passes every weaker form of this.
    #[test]
    fn a_load_edit_and_save_changes_exactly_the_edited_field() {
        let mut before = document();
        let before = before.files().expect("every block has an id");

        let mut document = document();
        let id = SceneEntityId(3);
        document
            .apply(EditCommand::SetProperty {
                entity: id,
                system: crate::scene::BLOCKS.to_owned(),
                path: "position.1".to_owned(),
                value: Value::Float(-3.5),
            })
            .expect("a block has a y");
        let after = document.files().expect("every block has an id");

        assert_eq!(
            before.keys().collect::<Vec<_>>(),
            after.keys().collect::<Vec<_>>()
        );
        for key in before.keys() {
            if key != "sys/blocks.ron" {
                assert_eq!(before[key], after[key], "{key} moved and nothing edited it");
            }
        }

        let changed: Vec<(usize, &str, &str)> = before["sys/blocks.ron"]
            .lines()
            .zip(after["sys/blocks.ron"].lines())
            .enumerate()
            .filter(|(_, (was, now))| was != now)
            .map(|(line, (was, now))| (line, was, now))
            .collect();
        assert_eq!(
            changed.len(),
            1,
            "one field was edited and {} lines moved: {changed:?}",
            changed.len(),
        );
        let (_, was, now) = changed[0];
        assert!(
            was.trim_start().starts_with("position: ("),
            "the changed line is not a position: {was:?}",
        );
        let (was, now) = (tuple_of(was), tuple_of(now));
        assert_eq!(was.len(), 3, "a position has three components: {was:?}");
        assert_eq!(now[1], "-3.5", "the y is not what the command set it to");
        assert_eq!(
            (was[0].as_str(), was[2].as_str()),
            (now[0].as_str(), now[2].as_str()),
            "editing the y moved the x or the z as well",
        );
    }

    /// The comma-separated components of the one `(…)` group in a RON line.
    fn tuple_of(line: &str) -> Vec<String> {
        let open = line.find('(').expect("a tuple line has an open paren");
        let close = line.rfind(')').expect("and a close one");
        line[open + 1..close]
            .split(',')
            .map(|part| part.trim().to_owned())
            .filter(|part| !part.is_empty())
            .collect()
    }

    /// **An undo restores the file byte for byte**, which is the strongest
    /// form of "the inverse restores the value": the RON writer prints a float
    /// through Rust's shortest round trip, so a value that came back as a
    /// different float prints as a different string.
    #[test]
    fn undoing_every_edit_restores_the_files_byte_for_byte() {
        let mut document = document();
        let before = document.files().expect("every block has an id");

        for (id, axis, delta) in [(0_u32, 0_usize, 0.35_f64), (1, 1, -0.4), (2, 2, 1.25)] {
            let id = SceneEntityId(id);
            let command = nudge(&mut document, id, axis, delta);
            document.apply(command).expect("a block has that axis");
        }
        assert_ne!(document.files().expect("ids"), before, "nothing was edited");

        while document.undo().expect("every entry names a live entity") {}
        assert_eq!(
            document.files().expect("every block has an id"),
            before,
            "walking the whole log back did not restore the file",
        );
    }

    /// Redo puts the edits back, and the file matches the edited one exactly.
    #[test]
    fn redoing_every_undone_edit_restores_the_edited_files() {
        let mut document = document();
        for (id, axis, delta) in [(0_u32, 0_usize, 0.35_f64), (3, 1, -0.4)] {
            let command = nudge(&mut document, SceneEntityId(id), axis, delta);
            document.apply(command).expect("a block has that axis");
        }
        let edited = document.files().expect("every block has an id");

        while document.undo().expect("live entities") {}
        while document.redo().expect("live entities") {}

        assert_eq!(document.files().expect("ids"), edited);
        assert_eq!(document.log().position(), 2);
    }

    /// **A drag carried on past a save is dirty again**: the save seals the
    /// entry it stands on, so the drag's next write starts an entry of its own
    /// rather than folding into one the file already holds.
    #[test]
    fn a_drag_carried_past_a_save_is_dirty_again() {
        let dir = tempfile::tempdir().expect("a temporary directory");
        let mut document = document();
        let drag = document.begin_gesture();
        for x in [0.5, 1.0] {
            let command = nudge(&mut document, SceneEntityId(1), 0, x);
            document.apply_in(command, drag).expect("a block has an x");
        }
        assert_eq!(document.log().len(), 1, "one drag, one entry");
        document
            .save_to(dir.path())
            .expect("the directory is writable");
        assert!(!document.is_dirty());

        let command = nudge(&mut document, SceneEntityId(1), 0, 0.5);
        document.apply_in(command, drag).expect("a block has an x");
        assert!(
            document.is_dirty(),
            "the write after the save was folded into it"
        );
        assert_eq!(document.log().len(), 2);
    }

    /// **The dirty marker follows the log's position**, in both directions.
    ///
    /// The case a flag gets wrong is the last one: undoing back to where the
    /// document was saved is clean again.
    #[test]
    fn the_dirty_marker_follows_the_logs_position() {
        let dir = tempfile::tempdir().expect("a temporary directory");
        let mut document = document();
        assert!(!document.is_dirty());
        assert!(!document.title().starts_with('*'), "{}", document.title());

        let command = nudge(&mut document, SceneEntityId(0), 0, 0.5);
        document.apply(command).expect("a block has an x");
        assert!(document.is_dirty());
        assert!(
            document.title().starts_with("*greybox"),
            "{}",
            document.title()
        );

        document.save_to(dir.path()).expect("a writable directory");
        assert!(!document.is_dirty(), "a save clears the marker");

        let command = nudge(&mut document, SceneEntityId(0), 0, 0.5);
        document.apply(command).expect("a block has an x");
        assert!(document.is_dirty());
        assert!(document.undo().expect("one entry"));
        assert!(
            !document.is_dirty(),
            "undoing back to the saved position must be clean again",
        );
        assert!(document.redo().expect("one entry"));
        assert!(document.is_dirty(), "and redoing away from it dirty again");
    }

    /// A save writes the files the scene is, and they read back as the same
    /// scene — the round trip through a real directory rather than through
    /// text.
    #[test]
    fn a_saved_directory_opens_again_as_the_same_document() {
        let dir = tempfile::tempdir().expect("a temporary directory");
        let mut document = document();
        let command = nudge(&mut document, SceneEntityId(2), 1, -1.5);
        document.apply(command).expect("a block has a y");
        let expected = document.files().expect("every block has an id");
        document.save_to(dir.path()).expect("a writable directory");

        let mut reopened = Document::open_dir(dir.path(), crate::scene::vocabulary())
            .expect("what we just wrote is a scene");
        assert_eq!(reopened.files().expect("ids"), expected);
        assert_eq!(reopened.origin(), Some(dir.path()));
        assert!(!reopened.is_dirty());
    }

    /// A command naming an id the document does not hold is refused by that id,
    /// and nothing is recorded.
    #[test]
    fn a_command_for_an_absent_entity_is_refused_and_not_recorded() {
        let mut document = document();
        let error = document
            .apply(EditCommand::SetProperty {
                entity: SceneEntityId(9_999),
                system: crate::scene::BLOCKS.to_owned(),
                path: "position.0".to_owned(),
                value: Value::Float(0.0),
            })
            .expect_err("that id is not in this scene");
        assert!(
            matches!(error, EditError::NoEntity(SceneEntityId(9_999))),
            "{error}",
        );
        assert!(document.log().is_empty());
        assert!(!document.is_dirty());
    }

    /// A command whose path names nothing is refused, nothing is written, and
    /// nothing is recorded — so a typo cannot leave a hole in the history.
    #[test]
    fn a_command_with_a_bad_path_is_refused_and_not_recorded() {
        let mut document = document();
        let before = document.files().expect("ids");
        let error = document
            .apply(EditCommand::SetProperty {
                entity: SceneEntityId(0),
                system: crate::scene::BLOCKS.to_owned(),
                path: "rotation.0".to_owned(),
                value: Value::Float(1.0),
            })
            .expect_err("a block has no rotation");
        assert!(matches!(error, EditError::Path(_)), "{error}");
        assert!(document.log().is_empty());
        assert_eq!(document.files().expect("ids"), before);
    }

    /// Selecting an id the document does not hold selects nothing.
    #[test]
    fn selecting_an_absent_id_selects_nothing() {
        let mut document = document();
        document.select(Some(SceneEntityId(0)));
        assert_eq!(document.selected(), Some(SceneEntityId(0)));
        document.select(Some(SceneEntityId(9_999)));
        assert_eq!(document.selected(), None);
    }

    /// **The bounds a selection is drawn with come from the component's own
    /// `Placement`**, and they are that component's box.
    ///
    /// Checked against the numbers the chunk file spells rather than against
    /// whatever the registry answered, so a placement that read the wrong field
    /// — or the right field of the wrong row — is red here.
    #[test]
    fn the_bounds_of_a_block_are_its_centre_plus_and_minus_its_half_extents() {
        let mut document = document();
        let (min, max) = document.bounds(SceneEntityId(3)).expect("the third step");
        let centre = (min + max) * 0.5;
        let half = (max - min) * 0.5;
        assert!((f64::from(centre.x) - 3.0).abs() < 1e-5, "{centre:?}");
        assert!((f64::from(centre.y) - 1.25).abs() < 1e-5, "{centre:?}");
        assert!((f64::from(half.x) - 1.2).abs() < 1e-5, "{half:?}");
        assert!((f64::from(half.y) - 1.25).abs() < 1e-5, "{half:?}");
        assert!(document.bounds(SceneEntityId(9_999)).is_none());
    }
}
