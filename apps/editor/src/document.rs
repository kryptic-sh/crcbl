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
//! # What "dirty" means here
//!
//! The position the log stands at, against the position it stood at when the
//! document was last saved — not a flag. Undoing back to a saved state is
//! **clean**, which a flag could not say and which this module's
//! `the_dirty_marker_follows_the_logs_position` holds.

use std::collections::BTreeMap;
use std::fmt;
use std::path::{Path, PathBuf};

use crcbl::assets::AssetSource;
use crcbl::ecs::{Entity, World};
use crcbl::math::{DVec3, Vec3};
use crcbl::phys::{PhysicsSystem, Ray};
use crcbl::reflect::{PathError, Value, get_path};
use crcbl::render::ViewRay;
use crcbl::scene::scn::{IdMap, Scene, SceneEntityId, ScnError};
use crcbl::store::{NativeStorage, StorageError, StorageSource};

use crate::board;
use crate::command::{EditCommand, UndoLog};

/// A loaded scene and everything the editor knows about it.
#[derive(Debug)]
pub struct Document {
    world: World,
    scene: Scene,
    ids: IdMap,
    /// Which [`SceneEntityId`] is selected, if any. An id rather than an
    /// [`Entity`] for [`EditCommand`]'s reason: it is what survives a reload.
    selected: Option<SceneEntityId>,
    log: UndoLog,
    /// The log position the document was last written at. `0` for one that has
    /// been loaded and not saved, which is also where a fresh log stands — so a
    /// document nobody has edited opens clean.
    saved_at: usize,
    /// Where [`Document::save_to`] writes when it is not told otherwise: the
    /// directory this document was opened from, or [`None`] for one opened out
    /// of a compiled-in source.
    origin: Option<PathBuf>,
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
}

impl fmt::Display for EditError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Scene(error) => write!(f, "{error}"),
            Self::NoEntity(id) => write!(f, "the scene holds no entity {id}"),
            Self::Path(error) => write!(f, "{error}"),
            Self::Write { key, source } => write!(f, "writing `{key}`: {source}"),
            Self::NoOrigin => f.write_str(
                "this document was not opened from a directory, so there is nowhere to save \
                 it back to; name a directory",
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
    /// Opens the `.scn/` directory at `dir`, read through `source`.
    ///
    /// The world is this document's: the systems are registered here, the
    /// entities are spawned here, and nothing else holds a handle into it. That
    /// is what makes [`Document::open`] the whole of "revert" and the whole of
    /// `docs/plan/08-editor.md`'s decided play/stop — reloading is opening
    /// again, not restoring a snapshot.
    ///
    /// # Errors
    ///
    /// [`EditError::Scene`], which names the key it is about: a file that is
    /// not there, text that is not this format, a header this build does not
    /// read, or a manifest naming a system this editor has no codec for.
    pub fn open(source: &dyn AssetSource, dir: &Path) -> Result<Self, EditError> {
        let mut world = World::new();
        board::register(&mut world);
        let (scene, ids) = Scene::load(source, dir, &board::codecs(), &mut world)?;
        for system in scene.systems() {
            let entities = board::entities(&mut world, &ids, system);
            board::sync_colliders(&mut world, entities);
        }
        Ok(Self {
            world,
            scene,
            ids,
            selected: None,
            log: UndoLog::new(),
            saved_at: 0,
            origin: None,
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
    pub fn open_dir(path: impl Into<PathBuf>) -> Result<Self, EditError> {
        let path = path.into();
        // Rooted at the scene directory itself and read with an empty prefix,
        // which is what `apps/breakout`'s `Board::read_dir` does and for the
        // same reason: `DirSource` refuses an absolute key and a `..`, so the
        // root is how a caller says where the scene is.
        let source = crcbl::assets::DirSource::at(path.clone());
        let mut document = Self::open(&source, Path::new(""))?;
        document.origin = Some(path);
        Ok(document)
    }

    /// Opens `apps/breakout`'s committed board — the document the editor starts
    /// on when it is not given one.
    ///
    /// Out of the game's own compiled-in source rather than off disk, for the
    /// reason that crate gives about its own loader: a tool that had to find
    /// `apps/breakout/assets/` would be one whose behaviour depended on the
    /// directory it was started from. It has no [`origin`](Document::save), so
    /// saving it means naming a directory.
    ///
    /// # Errors
    ///
    /// [`EditError::Scene`] if the committed board is not readable, which is a
    /// tree in which `apps/breakout`'s own tests are red too.
    pub fn built_in() -> Result<Self, EditError> {
        Self::open(
            &crcbl_breakout::built_in_source(),
            Path::new(crcbl_breakout::BOARD),
        )
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
    /// they came out of and in the order that file spells them.
    ///
    /// The outliner's rows — `docs/plan/08-editor.md` feature 2's "grouped by
    /// system (the natural shape of scene files)".
    #[must_use]
    pub fn outline(&mut self) -> Vec<(String, Vec<SceneEntityId>)> {
        let systems = self.scene.systems().to_vec();
        // Disjoint field borrows, spelled as two bindings so the closure below
        // does not capture the whole of `self`.
        let ids = &self.ids;
        let world = &mut self.world;
        systems
            .into_iter()
            .map(|system| {
                let entities = board::entities(world, ids, &system)
                    .into_iter()
                    .filter_map(|entity| ids.id(entity))
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
    /// [`board::sync_colliders`] runs after a load and after every edit that
    /// moves something.
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
        let (centre, half) = board::placement(&mut self.world, entity)?;
        let (centre, half) = (narrow(centre), narrow(half));
        Some((centre - half, centre + half))
    }

    /// What the leaf `path` names inside `id`'s component currently holds.
    ///
    /// The read half of an [`EditCommand`], through the same
    /// [`crcbl::reflect`] path resolution the write goes through: a caller
    /// building a *relative* edit — a nudge — reads the value here and sends an
    /// absolute command, so the command stays exact and its inverse stays
    /// exact with it.
    ///
    /// # Errors
    ///
    /// [`EditError::NoEntity`] for an id this document does not hold, or
    /// [`EditError::Path`] if the path names nothing or stops short of a leaf.
    pub fn read(&mut self, id: SceneEntityId, path: &str) -> Result<Value, EditError> {
        let entity = self.ids.entity(id).ok_or(EditError::NoEntity(id))?;
        let component = board::component(&mut self.world, entity).ok_or(EditError::NoEntity(id))?;
        Ok(get_path(component, path)?)
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
    /// nothing was written and nothing was recorded.
    pub fn apply(&mut self, command: EditCommand) -> Result<(), EditError> {
        let id = command.entity();
        let entity = self.ids.entity(id).ok_or(EditError::NoEntity(id))?;
        let undo = self.apply_to(entity, &command)?;
        self.log.record(command, undo);
        Ok(())
    }

    /// Steps back over the most recent applied command.
    ///
    /// Returns whether there was one. The inverse is applied and **not**
    /// recorded: the entry it came from already holds both halves, so the log
    /// is walked rather than grown.
    ///
    /// # Errors
    ///
    /// [`EditError`] if the entity the entry names has gone away, which in this
    /// slice cannot happen — nothing despawns — and which is still an error
    /// rather than a panic because a later slice's delete command will make it
    /// possible.
    pub fn undo(&mut self) -> Result<bool, EditError> {
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
            .save(&mut self.world, &self.ids, &board::codecs())?)
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
    pub fn save_to(&mut self, dir: impl AsRef<Path>) -> Result<(), EditError> {
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
        Ok(())
    }

    /// [`save_to`](Self::save_to) the directory this document was opened from.
    ///
    /// # Errors
    ///
    /// [`EditError::NoOrigin`] for a document that was not opened from one —
    /// the compiled-in board is the case — and otherwise as
    /// [`save_to`](Self::save_to).
    pub fn save(&mut self) -> Result<(), EditError> {
        let dir = self.origin.clone().ok_or(EditError::NoOrigin)?;
        self.save_to(dir)
    }

    /// Applies `command` to the component of the entity it names, without
    /// touching the log — the body [`apply`](Self::apply) and
    /// [`replay`](Self::replay) share.
    fn apply_to(
        &mut self,
        entity: Entity,
        command: &EditCommand,
    ) -> Result<EditCommand, EditError> {
        let component = board::component(&mut self.world, entity)
            .ok_or_else(|| EditError::NoEntity(command.entity()))?;
        let undo = command.apply(component)?;
        board::sync_colliders(&mut self.world, [entity]);
        Ok(undo)
    }

    /// Applies a command the log handed back, discarding the inverse: the entry
    /// it came from is already holding the other half.
    fn replay(&mut self, command: &EditCommand) -> Result<(), EditError> {
        let id = command.entity();
        let entity = self.ids.entity(id).ok_or(EditError::NoEntity(id))?;
        self.apply_to(entity, command)?;
        Ok(())
    }
}

/// Simulation space's `f64`, from render space's `f32`.
fn widen(value: Vec3) -> DVec3 {
    DVec3::new(f64::from(value.x), f64::from(value.y), f64::from(value.z))
}

/// Render space's `f32`, from simulation space's `f64`.
///
/// The lossy direction, and the safe one to be lossy in: this is what a picture
/// is drawn from, and `docs/plan/01-foundations.md` §1.2's rule is that plain
/// `Vec3` is only ever camera-relative render space.
#[allow(clippy::cast_possible_truncation)]
fn narrow(value: DVec3) -> Vec3 {
    Vec3::new(value.x as f32, value.y as f32, value.z as f32)
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    /// The board every test here opens: `apps/breakout`'s committed one,
    /// through its own compiled-in source, so nothing below depends on a
    /// working directory.
    fn document() -> Document {
        Document::built_in().expect("the committed board is a scene")
    }

    /// A command that moves `id` along `axis` by `delta`, read off the
    /// document so the *replaced* value is the one actually in the component.
    fn nudge(document: &mut Document, id: SceneEntityId, axis: usize, delta: f64) -> EditCommand {
        let (min, max) = document.bounds(id).expect("a brick in this document");
        let centre = (min + max) * 0.5;
        let was = f64::from([centre.x, centre.y, centre.z][axis]);
        EditCommand::SetProperty {
            entity: id,
            path: format!("position.{axis}"),
            value: Value::Float(was + delta),
        }
    }

    /// A ray that comes down `-Z` at `(x, y)`, which is how the board is
    /// looked at — every brick's `position.2` is 0 and its half extent on that
    /// axis is 0.5.
    fn ray_at(x: f64, y: f64) -> Ray {
        Ray::new(DVec3::new(x, y, 20.0), DVec3::NEG_Z)
    }

    /// **The board loads, and it is the board.**
    #[test]
    fn the_committed_board_opens_with_every_brick_the_file_names() {
        let mut document = document();
        assert_eq!(document.name(), "board");
        let outline = document.outline();
        assert_eq!(outline.len(), 1, "the manifest names one system");
        assert_eq!(outline[0].0, "bricks");
        assert_eq!(
            outline[0].1.len(),
            crcbl_breakout::Board::built_in().bricks().len(),
            "the editor and the game read a different number of bricks out of one file",
        );
        assert_eq!(document.entity_count(), outline[0].1.len());
        assert!(!document.is_dirty(), "a document nobody edited opens clean");
    }

    /// **The ray hits the entity under the cursor and not its neighbour.**
    ///
    /// The neighbour is named rather than merely "something else": the bricks
    /// are 2.4 m wide on a 2.6 m pitch, so a ray aimed 2.6 m along the row has
    /// to come back with the next id, and a pick that quietly answered "the
    /// nearest entity in the world" would pass a test that only asserted a hit.
    #[test]
    fn a_ray_picks_the_brick_it_points_at_and_not_the_one_beside_it() {
        let mut document = document();
        let bricks = crcbl_breakout::Board::built_in();
        let first = bricks.bricks()[0];
        let second = bricks.bricks()[1];
        assert!(
            (second.position[0] - first.position[0]).abs() > 2.0,
            "the fixture assumes the first two bricks are neighbours along x",
        );

        let hit = document
            .pick(&ray_at(first.position[0], first.position[1]))
            .expect("a ray down the first brick hits it");
        let next = document
            .pick(&ray_at(second.position[0], second.position[1]))
            .expect("and one down its neighbour hits that");
        assert_ne!(hit, next, "both rays picked the same brick");
        assert_eq!(hit, SceneEntityId(0), "file order is id order");
        assert_eq!(next, SceneEntityId(1));
    }

    /// A ray that points at nothing picks nothing, rather than the nearest
    /// thing anywhere.
    #[test]
    fn a_ray_that_misses_the_board_picks_nothing() {
        let mut document = document();
        assert!(document.pick(&ray_at(0.0, 400.0)).is_none());
    }

    /// **The collider follows the edit**, which is what makes a second pick
    /// find the brick where it now is rather than where it was.
    ///
    /// The observable is a *pick*, not a field: an edit that wrote the
    /// component and skipped [`board::sync_colliders`] would satisfy every
    /// assertion about `position` and still hand the wrong entity back to the
    /// next click.
    #[test]
    fn a_moved_brick_is_picked_where_it_now_is() {
        let mut document = document();
        let first = crcbl_breakout::Board::built_in().bricks()[0];
        let id = document
            .pick(&ray_at(first.position[0], first.position[1]))
            .expect("the first brick");

        // Straight up, far enough to clear its own half extent and land
        // somewhere the board has no other row.
        const LIFT: f64 = 40.0;
        let command = nudge(&mut document, id, 1, LIFT);
        document.apply(command).expect("a brick has a y");

        assert!(
            document
                .pick(&ray_at(first.position[0], first.position[1]))
                .is_none(),
            "the brick still picks where it used to be",
        );
        assert_eq!(
            document.pick(&ray_at(first.position[0], first.position[1] + LIFT)),
            Some(id),
            "and does not pick where it now is",
        );
    }

    /// **Load → save with no edits is byte-identical.**
    ///
    /// Against the committed files themselves rather than against a second
    /// save: a writer that was merely self-consistent would pass that, and the
    /// claim is that opening this board and saving it changes nothing on disk.
    #[test]
    fn a_load_and_a_save_with_no_edits_is_byte_identical() {
        let mut document = document();
        let written = document.files().expect("every brick has an id");
        let source = crcbl_breakout::built_in_source();
        for (key, text) in &written {
            let committed = source
                .read(Path::new(&format!("{}/{key}", crcbl_breakout::BOARD)))
                .unwrap_or_else(|error| panic!("the committed {key}: {error}"));
            assert_eq!(
                text.as_bytes(),
                committed.as_slice(),
                "a load-save round trip changed {key}",
            );
        }
        assert_eq!(
            written.keys().collect::<Vec<_>>(),
            ["env.ron", "scene.ron", "sys/bricks.ron"]
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
        let before = before.files().expect("every brick has an id");

        let mut document = document();
        let id = SceneEntityId(4);
        document
            .apply(EditCommand::SetProperty {
                entity: id,
                path: "position.1".to_owned(),
                value: Value::Float(-3.5),
            })
            .expect("a brick has a y");
        let after = document.files().expect("every brick has an id");

        assert_eq!(
            before.keys().collect::<Vec<_>>(),
            after.keys().collect::<Vec<_>>()
        );
        for key in before.keys() {
            if key != "sys/bricks.ron" {
                assert_eq!(before[key], after[key], "{key} moved and nothing edited it");
            }
        }

        let changed: Vec<(usize, &str, &str)> = before["sys/bricks.ron"]
            .lines()
            .zip(after["sys/bricks.ron"].lines())
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
        let before = document.files().expect("every brick has an id");

        for (id, axis, delta) in [(0_u32, 0_usize, 0.35_f64), (1, 1, -0.4), (2, 2, 1.25)] {
            let id = SceneEntityId(id);
            let command = nudge(&mut document, id, axis, delta);
            document.apply(command).expect("a brick has that axis");
        }
        assert_ne!(document.files().expect("ids"), before, "nothing was edited");

        while document.undo().expect("every entry names a live entity") {}
        assert_eq!(
            document.files().expect("every brick has an id"),
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
            document.apply(command).expect("a brick has that axis");
        }
        let edited = document.files().expect("every brick has an id");

        while document.undo().expect("live entities") {}
        while document.redo().expect("live entities") {}

        assert_eq!(document.files().expect("ids"), edited);
        assert_eq!(document.log().position(), 2);
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
        document.apply(command).expect("a brick has an x");
        assert!(document.is_dirty());
        assert!(
            document.title().starts_with("*board"),
            "{}",
            document.title()
        );

        document.save_to(dir.path()).expect("a writable directory");
        assert!(!document.is_dirty(), "a save clears the marker");

        let command = nudge(&mut document, SceneEntityId(0), 0, 0.5);
        document.apply(command).expect("a brick has an x");
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
        document.apply(command).expect("a brick has a y");
        let expected = document.files().expect("every brick has an id");
        document.save_to(dir.path()).expect("a writable directory");

        let mut reopened = Document::open_dir(dir.path()).expect("what we just wrote is a scene");
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
                path: "position.0".to_owned(),
                value: Value::Float(0.0),
            })
            .expect_err("that id is not on this board");
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
                path: "rotation.0".to_owned(),
                value: Value::Float(1.0),
            })
            .expect_err("a brick has no rotation");
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

    /// The bounds a selection is drawn with are the brick's own box.
    #[test]
    fn the_bounds_of_a_brick_are_its_centre_plus_and_minus_its_half_extents() {
        let mut document = document();
        let brick = crcbl_breakout::Board::built_in().bricks()[0];
        let (min, max) = document.bounds(SceneEntityId(0)).expect("the first brick");
        let centre = (min + max) * 0.5;
        let half = (max - min) * 0.5;
        assert!((f64::from(centre.x) - brick.position[0]).abs() < 1e-5);
        assert!((f64::from(half.x) - brick.half_extents[0]).abs() < 1e-5);
        assert!(document.bounds(SceneEntityId(9_999)).is_none());
    }
}
