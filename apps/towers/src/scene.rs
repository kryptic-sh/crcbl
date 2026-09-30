//! The field's layout, as a `.scn/` scene directory.
//!
//! `assets/scenes/field.scn/` is towers' map: a header, an environment and two
//! chunk files, one holding a [`Waypoint`] per corner of the path and one a
//! [`Plot`] per place a tower can be built. It is read through
//! [`crcbl::scene::scn`], the engine's own scene format — the directory of chunk
//! files `docs/notes/tooling.md` records under _What the deleted
//! 06-assets-scenes plan left behind_ — and it is what
//! `docs/plan/sample/07-towers.md`'s milestone 2 means by a map the editor
//! authors: `apps/editor` registers these two components and opens this
//! directory.
//!
//! ```text
//! assets/scenes/field.scn/
//!   scene.ron          format version, name, the system manifest
//!   env.ron            the camera the field is viewed from, and its ambient
//!   sys/waypoints.ron  one Waypoint(order, position) per corner of the path
//!   sys/plots.ron      one Plot(label, position) per build plot
//! ```
//!
//! # Two sources, one loader
//!
//! The committed files are `include_str!`ed and read back through a
//! [`MemorySource`], for the reason `apps/breakout/src/scene.rs` gives about its
//! board: a browser has no filesystem, and a binary that could fail to find its
//! own map is one whose run depends on the working directory it was started
//! from. `--scene <DIR>` is the run-time door onto a *different* directory,
//! opened with a [`DirSource`] — the same [`Map::load`] call either way, because
//! that is what an [`AssetSource`] is for.
//!
//! # The file is the layout, and `crate::map` is the rules
//!
//! [`Map::load`] spawns the chunks' rows into a [`World`] of its own, reads them
//! straight back out, and hands them to [`Map::new`], which refuses a layout the
//! field cannot hold — a diagonal leg, a plot on the lane, more plots than a
//! frame draws — by name. The world is the parse and nothing more; the game
//! plays on the [`Map`], and the creeps and towers it spawns are never the
//! file's entities.
//!
//! # What keeps the committed files honest
//!
//! Two tests, because they answer different questions.
//! `the_committed_field_is_what_the_writer_writes` builds the scene from the
//! milestone 1 table `crate::map` used to hold, writes it with [`Scene::save`],
//! and asserts the result is byte-for-byte the four committed files — so the
//! file is that table and not a retyping of it.
//! `saving_the_loaded_field_reproduces_the_committed_files` loads the committed
//! directory and saves it again, which is the round trip the editor makes every
//! time it opens and saves this map. A hand-typed `7.8000000000000005` is
//! written back as `7.8`, so either test goes red on a file edited by anything
//! but the writer.

use std::path::Path;

use crcbl::assets::{AssetSource, DirSource, MemorySource};
use crcbl::ecs::{ComponentHash, System, World};
use crcbl::math::DVec3;
use crcbl::reflect::Reflect;
use crcbl::registry::{Placement, Registry};
use crcbl::scene::scn::{IdMap, Scene};
use crcbl::serde::{Deserialize, Serialize};

use crate::map::{LANE_HEIGHT, LANE_WIDTH, Map, MapError, PAD_EDGE, PAD_HEIGHT};

/// The system every corner of the path is a row of: the manifest entry, the
/// chunk file's stem, and the name [`Waypoint`]'s codec is registered under.
const WAYPOINTS: &str = "waypoints";

/// The system every build plot is a row of.
const PLOTS: &str = "plots";

/// The directory `--scene` defaults to, relative to nothing: the committed
/// field is compiled in, and this is the name its keys are spelled under.
///
/// Public because [`built_in_source`] is, and a source whose keys nobody can
/// spell is a source nobody can read.
pub const FIELD: &str = "field.scn";

/// `assets/scenes/field.scn/scene.ron`, as it is committed.
const FIELD_SCENE_RON: &str = include_str!("../assets/scenes/field.scn/scene.ron");
/// `assets/scenes/field.scn/env.ron`, as it is committed.
const FIELD_ENV_RON: &str = include_str!("../assets/scenes/field.scn/env.ron");
/// `assets/scenes/field.scn/sys/waypoints.ron`, as it is committed.
const FIELD_WAYPOINTS_RON: &str = include_str!("../assets/scenes/field.scn/sys/waypoints.ron");
/// `assets/scenes/field.scn/sys/plots.ron`, as it is committed.
const FIELD_PLOTS_RON: &str = include_str!("../assets/scenes/field.scn/sys/plots.ron");

/// One corner of the path the creeps walk: where it is, and where it comes in
/// the walk.
///
/// **The order is a field rather than the file's entity order**, because the
/// order is the path and an id is not: an editor that adds a corner gives it
/// the next free id, and a path read in id order would put the new corner at
/// the end of the walk wherever it had been placed. [`Map::load`] sorts by this
/// and refuses two corners that claim the same place.
///
/// In `f64` like `apps/breakout`'s `Brick`, because that is what the physics
/// world a creep's sphere is written into is spelled in, and a path written as
/// `f32` would round on the way through the file and move the lane.
#[derive(Clone, Copy, Debug, PartialEq, Reflect, Serialize, Deserialize)]
#[reflect(crate = "crcbl::reflect")]
#[serde(crate = "crcbl::serde")]
pub struct Waypoint {
    /// Where it comes in the walk: the spawn is the lowest, the exit the
    /// highest. Gaps are allowed, which is what lets a corner be inserted
    /// between two others without renumbering the rest.
    #[reflect(name = "Order")]
    pub order: u32,
    /// Where it stands, in metres. On the ground: [`Map::new`] refuses a `y`
    /// that is not zero.
    #[reflect(name = "Position")]
    pub position: [f64; 3],
}

impl ComponentHash for Waypoint {
    fn hash_component(&self, hasher: &mut dyn std::hash::Hasher) {
        hasher.write_u32(self.order);
        for value in self.position {
            hasher.write(&value.to_bits().to_le_bytes());
        }
    }
}

/// The square of lane a corner of the path stands in: as wide as the lane, and
/// as proud of the ground as it is drawn — the square the two legs meeting there
/// both cover, so a tool picks the corner where the picture shows one.
impl Placement for Waypoint {
    fn placement(&self) -> Option<(DVec3, DVec3)> {
        let feet = DVec3::from_array(self.position);
        Some((
            feet + DVec3::new(0.0, 0.5 * LANE_HEIGHT, 0.0),
            DVec3::new(0.5 * LANE_WIDTH, 0.5 * LANE_HEIGHT, 0.5 * LANE_WIDTH),
        ))
    }
}

/// One place a tower can be built.
#[derive(Clone, Debug, PartialEq, Reflect, Serialize, Deserialize)]
#[reflect(crate = "crcbl::reflect")]
#[serde(crate = "crcbl::serde")]
pub struct Plot {
    /// What the overlay, the debug panel and a failing test call it.
    #[reflect(name = "Label")]
    pub label: String,
    /// Where a tower on it has its feet, in metres. On the ground: [`Map::new`]
    /// refuses a `y` that is not zero.
    #[reflect(name = "Position")]
    pub position: [f64; 3],
}

impl Plot {
    /// Where a tower on this plot has its feet, in metres.
    #[must_use]
    pub fn at(&self) -> DVec3 {
        DVec3::from_array(self.position)
    }
}

impl ComponentHash for Plot {
    fn hash_component(&self, hasher: &mut dyn std::hash::Hasher) {
        // The label's length before its bytes, the way `apps/puppet`'s
        // `Surface` hashes its own, so the stream says where the text stops
        // rather than leaving that to the fixed width of what follows it.
        hasher.write_usize(self.label.len());
        hasher.write(self.label.as_bytes());
        for value in self.position {
            hasher.write(&value.to_bits().to_le_bytes());
        }
    }
}

/// The build pad a plot is drawn as, which is the box a tool draws and picks it
/// by.
impl Placement for Plot {
    fn placement(&self) -> Option<(DVec3, DVec3)> {
        Some((
            self.at() + DVec3::new(0.0, 0.5 * PAD_HEIGHT, 0.0),
            DVec3::new(0.5 * PAD_EDGE, 0.5 * PAD_HEIGHT, 0.5 * PAD_EDGE),
        ))
    }
}

/// This game's scene vocabulary: two components, under the names their chunk
/// files are spelled with, and the rule a scene holding them is held to.
///
/// **The one place `waypoints` and `plots` are joined to their types.**
/// [`Map::load`] uses it and so does any tool that opens this game's field, so
/// the vocabulary the game ships and the vocabulary an editor sees are the same
/// list rather than two that agree today — `docs/plan/08-editor.md`'s component
/// registry.
pub fn register_components(registry: &mut Registry) {
    registry.register::<Waypoint>(WAYPOINTS);
    registry.register::<Plot>(PLOTS);
    registry.check(WAYPOINTS, check_field);
}

/// This game's rule over a scene that holds its waypoints: that
/// [`Map::load`] would take it — the check an editor saving this game's field
/// runs, so a diagonal leg or a plot on the lane is reported where it was made
/// rather than at the next `--scene`.
fn check_field(source: &dyn AssetSource, dir: &Path) -> Result<(), String> {
    Map::load(source, dir)
        .map(drop)
        .map_err(|error| error.to_string())
}

impl Map {
    /// The committed `assets/scenes/field.scn/`, parsed.
    ///
    /// # Panics
    ///
    /// If the committed directory is not a map, naming the key, the line and the
    /// column — or the rule it breaks. It is compiled into this binary, so that
    /// is a tree in which `the_committed_field_is_what_the_writer_writes` is also
    /// red: the panic is what keeps a run from starting on a map nobody could
    /// read.
    #[must_use]
    pub fn built_in() -> Self {
        Self::load(&built_in_source(), Path::new(FIELD))
            .unwrap_or_else(|error| panic!("apps/towers/assets/scenes/{FIELD}: {error}"))
    }

    /// The map `dir` holds, read through `source`.
    ///
    /// # Errors
    ///
    /// [`MapError`]: a key that is not there, text that is not this format, a
    /// header this build does not read, a manifest naming a system with no
    /// codec or leaving out one a towers map is made of, two corners of the path
    /// with one order — or a layout [`Map::new`] refuses.
    pub fn load(source: &dyn AssetSource, dir: &Path) -> Result<Self, MapError> {
        let mut registry = Registry::new();
        register_components(&mut registry);
        let mut world = World::new();
        registry.register_systems(&mut world);
        let (scene, ids) =
            Scene::load(source, dir, &registry.codecs(), &mut world).map_err(MapError::Scene)?;

        // A manifest that names *fewer* systems is not an error to the format
        // at all, and this is where it becomes one: a map with no plots chunk
        // would otherwise read as a map with no plots.
        for system in [WAYPOINTS, PLOTS] {
            if !scene.systems().iter().any(|named| named == system) {
                return Err(MapError::Missing(system));
            }
        }

        let mut waypoints = rows::<Waypoint>(&mut world, &ids);
        waypoints.sort_by_key(|waypoint| waypoint.order);
        if let Some(pair) = waypoints
            .windows(2)
            .find(|pair| pair[0].order == pair[1].order)
        {
            return Err(MapError::RepeatedOrder {
                order: pair[0].order,
            });
        }
        Self::new(
            waypoints
                .iter()
                .map(|waypoint| DVec3::from_array(waypoint.position))
                .collect(),
            rows::<Plot>(&mut world, &ids),
        )
    }

    /// The map the directory at `path` holds, or the message to refuse the run
    /// with.
    ///
    /// Every failure reads the same way — the path, then what went wrong with
    /// it — because to a person fixing it "no such file", "line 3, column 5" and
    /// "leg 2 runs along neither X nor Z" are the same kind of answer about the
    /// same argument. The shape `apps/breakout`'s `Board::read_dir` has.
    ///
    /// # Errors
    ///
    /// The refusal message, ready to print.
    pub fn read_dir(path: &str) -> Result<Self, String> {
        // Rooted at the scene directory itself and read with an empty prefix:
        // `DirSource` refuses an absolute key and a `..`, so the root is how a
        // caller says where the scene is.
        let source = DirSource::at(std::path::PathBuf::from(path));
        Self::load(&source, Path::new("")).map_err(|error| format!("{path}: {error}"))
    }
}

/// One system's rows, in the file's own id order.
///
/// Sorted by [`SceneEntityId`](crcbl::scene::scn::SceneEntityId) rather than
/// taken in storage order, so the plots are numbered in the order the chunk
/// spells them however the ECS happened to lay the rows out — which is the
/// order `PlaceTower` numbers them by. `apps/puppet`'s map reads its chunks the
/// same way.
fn rows<T>(world: &mut World, ids: &IdMap) -> Vec<T>
where
    T: Clone + ComponentHash + 'static,
{
    let system = world
        .system_mut::<System<T>>()
        .expect("the system `Map::load` registered is in the world it registered it in");
    let mut rows: Vec<_> = system
        .iter_entities()
        .map(|(entity, row)| (ids.id(entity), row.clone()))
        .collect();
    rows.sort_by_key(|(id, _)| *id);
    rows.into_iter().map(|(_, row)| row).collect()
}

/// The committed scene directory, as a source with no filesystem under it,
/// keyed under [`FIELD`].
///
/// Public so that a **tool** can open this game's field without one: `.scn/` is
/// the engine's own scene format, `apps/editor` opens this directory through
/// the vocabulary [`register_components`] builds, and a tool that had to find
/// `apps/towers/assets/` on disk would be one whose behaviour depended on the
/// directory it was started from. The game reads it through [`Map::built_in`],
/// which is this source and the loader over it.
#[must_use]
pub fn built_in_source() -> MemorySource {
    let mut source = MemorySource::new();
    for (key, text) in [
        ("scene.ron", FIELD_SCENE_RON),
        ("env.ron", FIELD_ENV_RON),
        ("sys/waypoints.ron", FIELD_WAYPOINTS_RON),
        ("sys/plots.ron", FIELD_PLOTS_RON),
    ] {
        // `format!` and not `Path::join`: an asset key is `/`-separated on
        // every host, and a key joined on Windows would not be one anywhere
        // else.
        source
            .insert(
                Path::new(&format!("{FIELD}/{key}")),
                text.as_bytes().to_vec(),
            )
            .expect("a nested scene key is a legal asset key");
    }
    source
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    use crcbl::reflect::{Kind, Value, get_path, set_path};
    use crcbl::scene::scn::{Env, EnvCamera};

    /// The corners of the path, spawn first — the milestone 1 table
    /// `crate::map` held as a constant before the map was a file, and so what
    /// the committed `sys/waypoints.ron` was written from.
    const TABLE_PATH: [[f64; 3]; 4] = [
        [-14.0, 0.0, 8.0],
        [8.0, 0.0, 8.0],
        [8.0, 0.0, -6.0],
        [-10.0, 0.0, -6.0],
    ];

    /// The plots, in the order `PlaceTower` numbers them — the other half of
    /// the same table, and what `sys/plots.ron` was written from.
    const TABLE_PLOTS: [(&str, [f64; 3]); 5] = [
        ("entry", [-6.0, 0.0, 3.0]),
        ("bend", [3.0, 0.0, 3.0]),
        ("east", [13.0, 0.0, 1.0]),
        ("middle", [2.0, 0.0, -1.0]),
        ("gate", [-7.0, 0.0, -1.0]),
    ];

    /// The environment the committed `env.ron` holds.
    ///
    /// The camera is [`crate::camera`]'s — its eye and what it looks at — and
    /// the ambient is [`crate::map::sun`]'s, because there is one camera and
    /// one light on this field. Both are read from the code rather than
    /// retyped, so a change to either is a red test here rather than a file
    /// that quietly disagrees with the game.
    ///
    /// Neither reaches the renderer from the file: the frame is drawn through
    /// `crate::camera::camera()` and lit by `crate::map::sun()` directly.
    /// `env.ron` is a file the format requires, and this is what keeps its
    /// numbers from drifting away from the two they were taken from.
    fn env() -> Env {
        Env {
            camera: EnvCamera {
                position: crate::camera::EYE.to_array(),
                look_at: crate::camera::TARGET.to_array(),
            },
            ambient: crate::map::sun().ambient.to_array(),
        }
    }

    /// The milestone 1 table, in a world of its own, ready to be written out:
    /// the waypoints first and numbered in walking order, then the plots.
    fn generated() -> (Scene, IdMap, World) {
        let mut world = World::new();
        world.register_system(Box::new(System::<Waypoint>::new(WAYPOINTS)));
        world.register_system(Box::new(System::<Plot>::new(PLOTS)));
        let mut ids = IdMap::new();
        let mut waypoints = Vec::with_capacity(TABLE_PATH.len());
        for (order, position) in (0..).zip(TABLE_PATH) {
            let entity = world.spawn();
            ids.assign(entity);
            waypoints.push((entity, Waypoint { order, position }));
        }
        let mut plots = Vec::with_capacity(TABLE_PLOTS.len());
        for (label, position) in TABLE_PLOTS {
            let entity = world.spawn();
            ids.assign(entity);
            plots.push((
                entity,
                Plot {
                    label: label.to_string(),
                    position,
                },
            ));
        }
        let system = world
            .system_mut::<System<Waypoint>>()
            .expect("just registered");
        for (entity, waypoint) in waypoints {
            system.attach(entity, waypoint);
        }
        let system = world.system_mut::<System<Plot>>().expect("just registered");
        for (entity, plot) in plots {
            system.attach(entity, plot);
        }
        let scene = Scene::new(
            "field",
            vec![WAYPOINTS.to_string(), PLOTS.to_string()],
            env(),
        );
        (scene, ids, world)
    }

    /// Asserts a committed file is what the writer wrote, and says *where* it
    /// stopped agreeing when it is not.
    ///
    /// Line by line rather than as two strings, and byte for byte on every
    /// platform — `apps/breakout/src/scene.rs`'s helper of the same name says
    /// why for both.
    fn assert_committed(key: &str, written: &str, committed: &str) {
        if written == committed {
            return;
        }
        match written
            .lines()
            .zip(committed.lines())
            .position(|(written, committed)| written != committed)
        {
            Some(line) => panic!(
                "{key} line {}: the writer writes `{}`, the file has `{}`",
                line + 1,
                written.lines().nth(line).expect("the line just compared"),
                committed.lines().nth(line).expect("the line just compared"),
            ),
            None => panic!(
                "{key}: the writer writes {} lines, the file has {}",
                written.lines().count(),
                committed.lines().count(),
            ),
        }
    }

    /// Asserts `files` is the four committed files and nothing else.
    fn assert_all_committed(files: &std::collections::BTreeMap<String, String>) {
        assert_committed("scene.ron", &files["scene.ron"], FIELD_SCENE_RON);
        assert_committed("env.ron", &files["env.ron"], FIELD_ENV_RON);
        assert_committed(
            "sys/waypoints.ron",
            &files["sys/waypoints.ron"],
            FIELD_WAYPOINTS_RON,
        );
        assert_committed("sys/plots.ron", &files["sys/plots.ron"], FIELD_PLOTS_RON);
        assert_eq!(
            files.keys().collect::<Vec<_>>(),
            ["env.ron", "scene.ron", "sys/plots.ron", "sys/waypoints.ron"],
            "the manifest's files and nothing else"
        );
        for (key, text) in files {
            assert!(!text.contains('\r'), "the newline is pinned in {key}");
        }
    }

    /// **The committed field is exactly what the writer writes** from the
    /// milestone 1 table, which is what lets the layout live in a file without
    /// a second copy of it in the game.
    ///
    /// Generated once by [`Scene::save`] and maintained that way — see the
    /// module docs.
    #[test]
    fn the_committed_field_is_what_the_writer_writes() {
        let (scene, ids, mut world) = generated();
        // This game's own registration rather than a second list, so the writer
        // test is about the vocabulary the game ships.
        let mut registry = Registry::new();
        register_components(&mut registry);
        let files = scene
            .save(&mut world, &ids, &registry.codecs())
            .expect("a generated field is writable");
        assert_all_committed(&files);
    }

    /// **Saving the loaded field reproduces the committed files, byte for
    /// byte** — the round trip `apps/editor` makes on this map every time it is
    /// opened and saved, so an editor session that changes nothing leaves the
    /// directory exactly as it was.
    #[test]
    fn saving_the_loaded_field_reproduces_the_committed_files() {
        let mut registry = Registry::new();
        register_components(&mut registry);
        let mut world = World::new();
        registry.register_systems(&mut world);
        let (scene, ids) = Scene::load(
            &built_in_source(),
            Path::new(FIELD),
            &registry.codecs(),
            &mut world,
        )
        .expect("the committed field is a scene");
        let files = scene
            .save(&mut world, &ids, &registry.codecs())
            .expect("a loaded field is writable");
        assert_all_committed(&files);
    }

    /// The other half of the writer test: what the committed files *parse* to
    /// is the table the game used to hold in code. A writer that agreed with
    /// itself while dropping a field would pass both byte comparisons above.
    #[test]
    fn the_committed_field_parses_to_the_table_it_was_written_from() {
        let map = Map::built_in();
        assert_eq!(
            map.path().waypoints(),
            TABLE_PATH.map(DVec3::from_array),
            "the path moved"
        );
        assert_eq!(map.plots().len(), TABLE_PLOTS.len());
        for (plot, (label, position)) in map.plots().iter().zip(TABLE_PLOTS) {
            assert_eq!(plot.label, label, "a plot was renamed or reordered");
            assert_eq!(plot.at(), DVec3::from_array(position), "{label} moved");
        }
    }

    /// Writes a scene directory holding `waypoints` and `plots` as their chunk
    /// files, under a name no other test uses.
    fn scene_dir(name: &str, waypoints: &str, plots: &str) -> std::path::PathBuf {
        let dir = std::env::temp_dir().join(format!("towers-{name}-{}.scn", std::process::id()));
        std::fs::create_dir_all(dir.join("sys")).expect("the temp dir is writable");
        std::fs::write(
            dir.join("scene.ron"),
            "Scene(format: 0, name: \"test\", systems: [\"waypoints\", \"plots\"])",
        )
        .expect("the temp dir is writable");
        std::fs::write(dir.join("env.ron"), FIELD_ENV_RON).expect("the temp dir is writable");
        std::fs::write(dir.join("sys").join("waypoints.ron"), waypoints)
            .expect("the temp dir is writable");
        std::fs::write(dir.join("sys").join("plots.ron"), plots).expect("the temp dir is writable");
        dir
    }

    /// **The walk is the `order` field's, not the file's.** The chunk here
    /// spells the committed corners with their ids shuffled against their
    /// `order` — the exit first, the spawn last, and gaps between the orders —
    /// so a loader that took id order would walk the path backwards, spawning
    /// the creeps at the exit.
    #[test]
    fn the_path_is_walked_in_waypoint_order_rather_than_file_order() {
        let dir = scene_dir(
            "order",
            "Chunk(system: \"waypoints\", entities: [\n\
             (0, Waypoint(order: 30, position: (-10.0, 0.0, -6.0))),\n\
             (1, Waypoint(order: 10, position: (8.0, 0.0, 8.0))),\n\
             (2, Waypoint(order: 20, position: (8.0, 0.0, -6.0))),\n\
             (3, Waypoint(order: 0, position: (-14.0, 0.0, 8.0))),\n\
             ])",
            FIELD_PLOTS_RON,
        );
        let map = Map::read_dir(dir.to_str().expect("utf-8")).expect("the scene is a map");
        assert_eq!(
            map.path().waypoints(),
            TABLE_PATH.map(DVec3::from_array),
            "the path was not read in order"
        );
    }

    /// **Two corners claiming one place in the walk are refused by that
    /// place**, rather than taken in whichever order the sort left them.
    #[test]
    fn two_waypoints_with_one_order_are_refused() {
        let dir = scene_dir(
            "repeated",
            "Chunk(system: \"waypoints\", entities: [\n\
             (0, Waypoint(order: 3, position: (-14.0, 0.0, 8.0))),\n\
             (1, Waypoint(order: 3, position: (8.0, 0.0, 8.0))),\n\
             ])",
            FIELD_PLOTS_RON,
        );
        let message =
            Map::read_dir(dir.to_str().expect("utf-8")).expect_err("two corners share an order");
        assert!(message.contains("order 3"), "{message}");
    }

    /// **A layout the field cannot hold is refused through the loader, by the
    /// rule it breaks**, and the message names the directory it came from.
    /// `crate::map`'s tests hold each rule on its own; this is the join.
    #[test]
    fn a_layout_the_field_cannot_hold_is_refused_through_the_loader() {
        let dir = scene_dir(
            "diagonal",
            "Chunk(system: \"waypoints\", entities: [\n\
             (0, Waypoint(order: 0, position: (-14.0, 0.0, 8.0))),\n\
             (1, Waypoint(order: 1, position: (8.0, 0.0, -6.0))),\n\
             ])",
            FIELD_PLOTS_RON,
        );
        let path = dir.to_str().expect("utf-8");
        let message = Map::read_dir(path).expect_err("a diagonal leg is not a lane");
        assert!(message.starts_with(path), "{message}");
        assert!(message.contains("leg 0"), "{message}");
        assert!(message.contains("neither X nor Z"), "{message}");
    }

    /// **A manifest that leaves out a chunk is refused by the chunk**, rather
    /// than read as a map with no plots.
    #[test]
    fn a_manifest_without_the_plots_chunk_is_refused_by_name() {
        let dir = scene_dir("missing", FIELD_WAYPOINTS_RON, FIELD_PLOTS_RON);
        std::fs::write(
            dir.join("scene.ron"),
            "Scene(format: 0, name: \"test\", systems: [\"waypoints\"])",
        )
        .expect("the temp dir is writable");
        let message = Map::read_dir(dir.to_str().expect("utf-8"))
            .expect_err("a map with no plots chunk is not a map");
        assert!(message.contains("`plots`"), "{message}");
    }

    /// A directory with no `scene.ron` is refused by the key that is missing,
    /// spelled the way `--scene` points at it — the scene directory *is* the
    /// root, so the header is `scene.ron` and not `field.scn/scene.ron`.
    #[test]
    fn a_directory_with_no_header_is_refused_by_the_missing_key() {
        let dir = std::env::temp_dir().join(format!("towers-empty-{}.scn", std::process::id()));
        std::fs::create_dir_all(&dir).expect("the temp dir is writable");
        let message = Map::read_dir(dir.to_str().expect("utf-8"))
            .expect_err("a directory with no scene.ron is not a map");
        assert!(message.contains("scene.ron"), "{message}");
        assert!(message.contains("path not found"), "{message}");
        assert!(
            !message.contains(FIELD),
            "the key must be the caller's, not the built-in field's: {message}"
        );
    }

    /// A directory whose header is not RON is refused by line and column, so a
    /// person fixing it is told where to look.
    #[test]
    fn a_header_that_is_not_ron_is_refused_by_line_and_column() {
        let dir = std::env::temp_dir().join(format!("towers-bad-{}.scn", std::process::id()));
        std::fs::create_dir_all(&dir).expect("the temp dir is writable");
        std::fs::write(
            dir.join("scene.ron"),
            "Scene(\n    format: 0,\n    nome: \"field\",\n)",
        )
        .expect("the temp dir is writable");
        let message = Map::read_dir(dir.to_str().expect("utf-8"))
            .expect_err("a header that is not a scene is not a map");
        assert!(message.contains("scene.ron"), "{message}");
        assert!(message.contains("line"), "{message}");
        assert!(message.contains("column"), "{message}");
    }

    // -- the vocabulary ------------------------------------------------------

    /// **Both components describe the rows a property panel draws**, labelled
    /// as the attributes say.
    #[test]
    fn both_components_describe_the_rows_a_property_panel_draws() {
        let waypoint = Waypoint {
            order: 2,
            position: [8.0, 0.0, -6.0],
        };
        assert_eq!(Reflect::type_name(&waypoint), "Waypoint");
        assert_eq!(Reflect::kind(&waypoint), Kind::Struct);
        assert_eq!(
            Reflect::fields(&waypoint)
                .iter()
                .map(|row| row.label)
                .collect::<Vec<_>>(),
            ["Order", "Position"]
        );

        let plot = Plot {
            label: "gate".to_string(),
            position: [-7.0, 0.0, -1.0],
        };
        assert_eq!(Reflect::type_name(&plot), "Plot");
        assert_eq!(
            Reflect::fields(&plot)
                .iter()
                .map(|row| row.label)
                .collect::<Vec<_>>(),
            ["Label", "Position"]
        );
    }

    /// **An edit to a plot reads back and undoes**, which is the whole of what
    /// the editor's inspector does with a row.
    #[test]
    fn an_edit_to_a_plot_reads_back_and_undoes() {
        let mut plot = Plot {
            label: "gate".to_string(),
            position: [-7.0, 0.0, -1.0],
        };
        let before = plot.clone();

        let old = get_path(&plot, "position.0").expect("the row is an f64");
        assert_eq!(old, Value::Float(-7.0));
        set_path(&mut plot, "position.0", &Value::Float(-9.5)).expect("an f64 takes a float");
        assert_eq!(plot.at(), DVec3::new(-9.5, 0.0, -1.0));

        set_path(&mut plot, "position.0", &old).expect("the recorded value");
        assert_eq!(plot, before);
    }

    /// **A corner and a plot are picked by the boxes the field draws them as**:
    /// the square of lane a corner stands in and the pad a plot is. Centred
    /// half their height up, because both stand on the ground rather than
    /// straddling it.
    #[test]
    fn a_waypoint_and_a_plot_are_placed_where_the_field_draws_them() {
        let waypoint = Waypoint {
            order: 0,
            position: [8.0, 0.0, -6.0],
        };
        assert_eq!(
            waypoint.placement(),
            Some((
                DVec3::new(8.0, 0.5 * LANE_HEIGHT, -6.0),
                DVec3::new(0.5 * LANE_WIDTH, 0.5 * LANE_HEIGHT, 0.5 * LANE_WIDTH),
            )),
        );
        let plot = Plot {
            label: "gate".to_string(),
            position: [-7.0, 0.0, -1.0],
        };
        assert_eq!(
            plot.placement(),
            Some((
                DVec3::new(-7.0, 0.5 * PAD_HEIGHT, -1.0),
                DVec3::new(0.5 * PAD_EDGE, 0.5 * PAD_HEIGHT, 0.5 * PAD_EDGE),
            )),
        );
    }

    /// **A plot's hash covers its label and its position**: two equal rows
    /// hash alike, and a row that differs in one letter or in one coordinate
    /// does not.
    #[test]
    fn a_plots_hash_covers_its_label_and_its_position() {
        use std::hash::Hasher;

        let hash = |plot: &Plot| {
            let mut hasher = std::collections::hash_map::DefaultHasher::new();
            plot.hash_component(&mut hasher);
            hasher.finish()
        };
        let plot = |label: &str, x: f64| Plot {
            label: label.to_string(),
            position: [x, 0.0, 0.0],
        };
        assert_eq!(hash(&plot("gate", 1.0)), hash(&plot("gate", 1.0)));
        assert_ne!(hash(&plot("gate", 1.0)), hash(&plot("gate", 2.0)));
        assert_ne!(hash(&plot("gate", 1.0)), hash(&plot("gatf", 1.0)));
    }
}
