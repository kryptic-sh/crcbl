//! The registry's tests: names resolved to types, scenes loaded through it,
//! and the accessors answering for the entity they name.

use std::hash::Hasher;

use crcbl_assets::MemorySource;
use crcbl_reflect::{Value, get_path};
use crcbl_scene::scn::{Scene, SceneEntityId, ScnError};
use serde::Deserialize;

use super::*;

/// A component in the shape a real one has: a position, an extent, and a
/// placement built from both.
#[derive(Clone, Copy, Debug, PartialEq, Reflect, Serialize, Deserialize)]
#[reflect(crate = "crcbl_reflect")]
struct Block {
    position: [f64; 3],
    half_extents: [f64; 3],
}

impl ComponentHash for Block {
    fn hash_component(&self, hasher: &mut dyn Hasher) {
        for value in self.position.iter().chain(&self.half_extents) {
            hasher.write(&value.to_bits().to_le_bytes());
        }
    }
}

impl Placement for Block {
    fn placement(&self) -> Option<(DVec3, DVec3)> {
        Some((
            DVec3::from_array(self.position),
            DVec3::from_array(self.half_extents),
        ))
    }
}

/// A second vocabulary, and one that is **not** a thing in space — the
/// `apps/puppet` `Sun` shape, so the tests below are about two component
/// types rather than one registered twice.
#[derive(Clone, Copy, Debug, PartialEq, Reflect, Serialize, Deserialize)]
#[reflect(crate = "crcbl_reflect")]
struct Beacon {
    intensity: f32,
}

impl ComponentHash for Beacon {
    fn hash_component(&self, hasher: &mut dyn Hasher) {
        hasher.write(&self.intensity.to_bits().to_le_bytes());
    }
}

impl Placement for Beacon {
    fn placement(&self) -> Option<(DVec3, DVec3)> {
        None
    }
}

/// Both components, under the names the scene below spells.
fn registry() -> Registry {
    let mut registry = Registry::new();
    registry.register::<Block>("blocks");
    registry.register::<Beacon>("beacons");
    registry
}

/// A two-system scene, as text, keyed the way a source rooted at the scene
/// directory reads it.
fn scene_source() -> MemorySource {
    let mut source = MemorySource::new();
    for (key, text) in [
        (
            "scene.ron",
            "(format: 0, name: \"two\", systems: [\"blocks\", \"beacons\"])",
        ),
        (
            "env.ron",
            "(camera: (position: (0.0, 0.0, 8.0), look_at: (0.0, 0.0, 0.0)), \
             ambient: (0.1, 0.1, 0.1))",
        ),
        (
            "sys/blocks.ron",
            "(system: \"blocks\", entities: [\
             (0, (position: (1.0, 2.0, 3.0), half_extents: (0.5, 0.25, 0.5))), \
             (1, (position: (4.0, 0.0, 0.0), half_extents: (1.0, 1.0, 1.0)))])",
        ),
        (
            "sys/beacons.ron",
            "(system: \"beacons\", entities: [(2, (intensity: 3.0))])",
        ),
    ] {
        source
            .insert(std::path::Path::new(key), text.as_bytes().to_vec())
            .expect("a scene key is a legal asset key");
    }
    source
}

/// **A chunk resolves to the type its rows are of**, which is the whole
/// question a tool asks the registry — and it is asked by name, because the
/// name is what a manifest carries.
#[test]
fn the_registry_resolves_a_system_name_to_the_component_it_holds() {
    let registry = registry();
    assert_eq!(registry.len(), 2);
    assert!(registry.contains("blocks"));
    assert!(!registry.contains("bricks"));
    assert!(
        registry
            .component_type("blocks")
            .expect("blocks is registered")
            .ends_with("Block"),
        "{:?}",
        registry.component_type("blocks"),
    );
    assert!(
        registry
            .component_type("beacons")
            .expect("beacons is registered")
            .ends_with("Beacon"),
    );
    assert_eq!(registry.component_type("bricks"), None);
}

/// **A scene of two systems loads through the registry alone**, and every row
/// of both arrives — the claim a registry with one entry could not make.
#[test]
fn a_scene_of_two_systems_loads_through_the_registry() {
    let registry = registry();
    let mut world = World::new();
    registry.register_systems(&mut world);
    let (scene, ids) = Scene::load(
        &scene_source(),
        std::path::Path::new(""),
        &registry.codecs(),
        &mut world,
    )
    .expect("two registered systems is a scene this registry opens");

    assert_eq!(scene.systems(), ["blocks", "beacons"]);
    assert_eq!(ids.len(), 3, "two blocks and a beacon");
    assert_eq!(registry.entities(&mut world, &ids, "blocks").len(), 2);
    assert_eq!(registry.entities(&mut world, &ids, "beacons").len(), 1);
    assert!(
        registry.entities(&mut world, &ids, "bricks").is_empty(),
        "a name this registry does not know holds nothing",
    );
}

/// **An entity resolves to the system holding it, and that system's codec
/// reads its row** — what a delete records so its undo can rebuild the
/// entity.
#[test]
fn an_entity_resolves_to_its_system_and_that_systems_row() {
    let registry = registry();
    let mut world = World::new();
    registry.register_systems(&mut world);
    let (_, ids) = Scene::load(
        &scene_source(),
        std::path::Path::new(""),
        &registry.codecs(),
        &mut world,
    )
    .expect("the scene loads");

    let block = ids.entity(SceneEntityId(1)).expect("the second block");
    let beacon = ids.entity(SceneEntityId(2)).expect("the beacon");
    assert_eq!(
        registry.system_of(&mut world, block).as_deref(),
        Some("blocks")
    );
    assert_eq!(
        registry.system_of(&mut world, beacon).as_deref(),
        Some("beacons")
    );
    let stranger = world.spawn();
    assert_eq!(registry.system_of(&mut world, stranger), None);

    let row = registry
        .codec("beacons")
        .expect("beacons is registered")
        .row(&mut world, beacon)
        .expect("the system is in the world")
        .expect("it holds the beacon");
    assert_eq!(row, "(intensity:3.0)");
    assert!(registry.codec("bricks").is_none());
}

/// **A game's check runs on a scene that lists its system and on no other**,
/// and says what it refuses.
#[test]
fn a_check_runs_only_on_scenes_listing_its_system() {
    fn refuse_beacons(_: &dyn AssetSource, _: &Path) -> Result<(), String> {
        Err("a beacon needs a block to stand on".to_owned())
    }
    fn refuse_bricks(_: &dyn AssetSource, _: &Path) -> Result<(), String> {
        Err("bricks were checked".to_owned())
    }
    fn pass(_: &dyn AssetSource, _: &Path) -> Result<(), String> {
        Ok(())
    }
    let mut registry = registry();
    registry.check("beacons", refuse_beacons);
    registry.check("bricks", refuse_bricks);
    registry.check("blocks", pass);

    let source = scene_source();
    let systems = ["blocks".to_owned(), "beacons".to_owned()];
    assert_eq!(
        registry.problems(&systems, &source, std::path::Path::new("")),
        ["a beacon needs a block to stand on"],
    );
    assert!(
        registry
            .problems(&["blocks".to_owned()], &source, std::path::Path::new(""))
            .is_empty(),
        "a check ran on a scene that does not list its system",
    );
}

/// **A module is built for a scene that lists its system and for no
/// other**, in the order the modules were added — and every call builds
/// new instances, so what one play left in a module is not where the next
/// starts.
#[test]
fn modules_are_built_fresh_for_scenes_listing_their_system() {
    use std::sync::atomic::{AtomicUsize, Ordering};

    /// How many modules the factories below have built.
    static BUILT: AtomicUsize = AtomicUsize::new(0);

    struct Named(&'static str);

    impl GameModule for Named {
        fn name(&self) -> &str {
            self.0
        }
        fn register(&self, _world: &mut World) {}
    }

    fn names(modules: &[Box<dyn GameModule>]) -> Vec<&str> {
        modules.iter().map(|module| module.name()).collect()
    }

    let mut registry = registry();
    registry.module("beacons", |_, _| {
        BUILT.fetch_add(1, Ordering::Relaxed);
        Ok(Box::new(Named("first")))
    });
    registry.module("bricks", |_, _| {
        BUILT.fetch_add(1, Ordering::Relaxed);
        Ok(Box::new(Named("elsewhere")))
    });
    registry.module("blocks", |_, _| {
        BUILT.fetch_add(1, Ordering::Relaxed);
        Ok(Box::new(Named("second")))
    });

    let source = scene_source();
    let build = |systems: &[String]| {
        registry
            .modules(systems, &source, Path::new(""))
            .expect("no factory here refuses")
    };
    let systems = ["blocks".to_owned(), "beacons".to_owned()];
    let first = build(&systems);
    assert_eq!(names(&first), ["first", "second"]);
    assert_eq!(
        BUILT.load(Ordering::Relaxed),
        2,
        "a module nothing listed was built"
    );
    let again = build(&systems);
    assert_eq!(names(&again), ["first", "second"]);
    assert_eq!(
        BUILT.load(Ordering::Relaxed),
        4,
        "a second call handed back the modules it built before",
    );
    assert!(build(&[]).is_empty());
}

/// **A factory reads the scene it is handed and may refuse it**, and the
/// refusal is what `modules` answers — no module is handed back beside it.
#[test]
fn a_factory_that_refuses_the_scene_refuses_the_play() {
    struct Rules;

    impl GameModule for Rules {
        fn name(&self) -> &str {
            "rules"
        }
        fn register(&self, _world: &mut World) {}
    }

    /// Plays a scene with a header, and refuses one without.
    fn start(source: &dyn AssetSource, dir: &Path) -> Result<Box<dyn GameModule>, String> {
        source
            .read(&dir.join("scene.ron"))
            .map_err(|error| format!("no header: {error}"))?;
        Ok(Box::new(Rules))
    }

    let mut registry = registry();
    registry.module("blocks", start);
    let systems = ["blocks".to_owned()];

    let played = registry
        .modules(&systems, &scene_source(), Path::new(""))
        .expect("the scene has a header");
    assert_eq!(played.len(), 1);
    let refused = registry
        .modules(&systems, &MemorySource::new(), Path::new(""))
        .err()
        .expect("an empty source has no header");
    assert!(refused.starts_with("no header"), "{refused}");
}

/// A runtime component: where a thing a module spawned stands.
#[derive(Clone, Copy, Debug, PartialEq)]
struct Ball {
    centre: [f64; 3],
}

impl ComponentHash for Ball {
    fn hash_component(&self, hasher: &mut dyn Hasher) {
        for value in self.centre {
            hasher.write(&value.to_bits().to_le_bytes());
        }
    }
}

impl Placement for Ball {
    fn placement(&self) -> Option<(DVec3, DVec3)> {
        Some((DVec3::from_array(self.centre), DVec3::splat(0.25)))
    }
}

/// **A runtime component is placed and listed for drawing, and it is no
/// part of the scene vocabulary**: not a system a scene can name, no
/// codec, no editable row, no system a load registers.
#[test]
fn a_runtime_component_is_placed_and_never_part_of_the_scene() {
    let mut registry = registry();
    registry.register_runtime::<Ball>("balls");

    assert_eq!(
        registry.systems().collect::<Vec<_>>(),
        ["beacons", "blocks"]
    );
    assert!(!registry.contains("balls"));
    assert!(registry.codec("balls").is_none());
    assert_eq!(registry.codecs().len(), 2);
    assert_eq!(registry.component_type("balls"), None);

    let mut world = World::new();
    registry.register_systems(&mut world);
    assert_eq!(
        world.schedule().len(),
        2,
        "a load registered the runtime system"
    );
    assert!(
        registry.runtime_entities(&mut world).is_empty(),
        "a world nothing plays has runtime entities",
    );

    // What the module that owns it does in its own `register` and `tick`.
    let mut balls = System::<Ball>::new("balls");
    let ball = world.spawn();
    balls.attach(
        ball,
        Ball {
            centre: [1.0, 2.0, 3.0],
        },
    );
    world.register_system(Box::new(balls));

    assert_eq!(registry.runtime_entities(&mut world), [ball]);
    assert_eq!(
        registry.placement(&mut world, ball),
        Some((DVec3::new(1.0, 2.0, 3.0), DVec3::splat(0.25))),
    );
    assert!(
        registry.component(&mut world, ball).is_none(),
        "a ball is editable"
    );
    assert_eq!(registry.system_of(&mut world, ball), None);
}

/// **A scene naming a runtime system is refused by name**, because there
/// is no codec to read it with — the half of "never saved" a load holds.
#[test]
fn a_scene_naming_a_runtime_system_is_refused_by_name() {
    let mut registry = Registry::new();
    registry.register::<Block>("blocks");
    registry.register_runtime::<Beacon>("beacons");
    let mut world = World::new();
    registry.register_systems(&mut world);

    let error = Scene::load(
        &scene_source(),
        std::path::Path::new(""),
        &registry.codecs(),
        &mut world,
    )
    .expect_err("`beacons` is runtime only");
    assert!(
        matches!(&error, ScnError::NoCodec { system } if system == "beacons"),
        "{error}",
    );
}

/// A runtime component under a scene system's name is refused like two
/// scene components under one.
#[test]
#[should_panic(expected = "is already registered")]
fn a_runtime_component_under_a_scene_systems_name_is_refused() {
    let mut registry = registry();
    registry.register_runtime::<Ball>("blocks");
}

/// …and the other way round.
#[test]
#[should_panic(expected = "is already registered")]
fn a_scene_component_under_a_runtime_systems_name_is_refused() {
    let mut registry = Registry::new();
    registry.register_runtime::<Ball>("blocks");
    registry.register::<Block>("blocks");
}

/// **An unregistered system fails loudly, naming itself.** The failure this
/// module exists to remove is the other one: a load that quietly skipped the
/// chunk and handed back a scene with a third of its entities missing.
#[test]
fn a_scene_naming_an_unregistered_system_is_refused_by_name() {
    let mut registry = Registry::new();
    registry.register::<Block>("blocks");
    let mut world = World::new();
    registry.register_systems(&mut world);

    let error = Scene::load(
        &scene_source(),
        std::path::Path::new(""),
        &registry.codecs(),
        &mut world,
    )
    .expect_err("`beacons` is not registered");
    assert!(
        matches!(&error, ScnError::NoCodec { system } if system == "beacons"),
        "{error}",
    );
    assert!(
        error.to_string().contains("beacons"),
        "the refusal does not name the system: {error}",
    );
}

/// **The codec list and the registered systems are the same list**, which is
/// what makes "registered for the scene but not for the tool" impossible
/// rather than merely unlikely.
#[test]
fn every_codec_has_a_system_of_the_same_name_and_the_reverse() {
    let registry = registry();
    let mut world = World::new();
    registry.register_systems(&mut world);

    let codecs: Vec<String> = registry
        .codecs()
        .iter()
        .map(|codec| codec.name().to_owned())
        .collect();
    let systems: Vec<String> = world
        .schedule_mut()
        .iter_mut()
        .map(|system| system.name().to_owned())
        .collect();
    assert_eq!(codecs, vec!["beacons".to_owned(), "blocks".to_owned()]);
    assert_eq!(systems, codecs);
}

/// The `&mut dyn Reflect` a registry hands back is **that entity's**
/// component, reachable by the same path an edit command carries — and
/// writing through it changes that row and no other.
#[test]
fn the_component_accessor_reads_and_writes_the_entity_it_names() {
    let registry = registry();
    let mut world = World::new();
    registry.register_systems(&mut world);
    let (_, ids) = Scene::load(
        &scene_source(),
        std::path::Path::new(""),
        &registry.codecs(),
        &mut world,
    )
    .expect("the scene loads");

    let first = ids
        .entity(crcbl_scene::scn::SceneEntityId(0))
        .expect("id 0");
    let second = ids
        .entity(crcbl_scene::scn::SceneEntityId(1))
        .expect("id 1");

    let component = registry
        .component(&mut world, first)
        .expect("a block is a registered component");
    assert_eq!(get_path(component, "position.1"), Ok(Value::Float(2.0)));
    crcbl_reflect::set_path(component, "position.1", &Value::Float(9.0)).expect("a block has a y");

    assert_eq!(
        registry
            .placement(&mut world, first)
            .expect("a block is a thing in space")
            .0,
        DVec3::new(1.0, 9.0, 3.0),
        "the write did not reach the component the placement reads",
    );
    assert_eq!(
        registry
            .placement(&mut world, second)
            .expect("so is its neighbour")
            .0,
        DVec3::new(4.0, 0.0, 0.0),
        "editing one row moved another",
    );
}

/// A component that is not a thing in space has **no** placement, and that is
/// different from being unregistered: the accessor still reaches it.
#[test]
fn a_component_that_is_not_in_space_has_a_reflect_row_and_no_placement() {
    let registry = registry();
    let mut world = World::new();
    registry.register_systems(&mut world);
    let (_, ids) = Scene::load(
        &scene_source(),
        std::path::Path::new(""),
        &registry.codecs(),
        &mut world,
    )
    .expect("the scene loads");
    let beacon = ids
        .entity(crcbl_scene::scn::SceneEntityId(2))
        .expect("id 2");

    assert!(
        registry.component(&mut world, beacon).is_some(),
        "a beacon is editable",
    );
    assert_eq!(registry.placement(&mut world, beacon), None);
}

/// An entity no registered system holds answers [`None`] to both halves,
/// rather than panicking or reaching some other entity's row.
#[test]
fn an_entity_with_no_registered_component_has_neither_half() {
    let registry = registry();
    let mut world = World::new();
    registry.register_systems(&mut world);
    let stranger = world.spawn();

    assert!(registry.component(&mut world, stranger).is_none());
    assert_eq!(registry.placement(&mut world, stranger), None);
}

/// An empty registry opens nothing and says so, rather than opening a scene
/// with no entities in it.
#[test]
fn an_empty_registry_holds_nothing_and_refuses_every_scene() {
    let registry = Registry::new();
    assert!(registry.is_empty());
    assert_eq!(registry.len(), 0);
    assert!(registry.codecs().is_empty());

    let mut world = World::new();
    registry.register_systems(&mut world);
    assert_eq!(world.schedule().len(), 0);
    assert!(
        Scene::load(
            &scene_source(),
            std::path::Path::new(""),
            &registry.codecs(),
            &mut world,
        )
        .is_err(),
    );
}

/// Registering two components under one name is a panic that names the
/// system and both types, rather than a silent replacement of one by the
/// other.
#[test]
#[should_panic(expected = "is already registered")]
fn two_components_under_one_system_name_is_refused() {
    let mut registry = Registry::new();
    registry.register::<Block>("blocks");
    registry.register::<Beacon>("blocks");
}

/// The `Debug` form is the vocabulary: every name with the type it resolves
/// to, which is what a tool logs when it says what it can open.
#[test]
fn the_debug_form_names_every_system_and_its_component() {
    let text = format!("{:?}", registry());
    assert!(text.contains("blocks"), "{text}");
    assert!(text.contains("beacons"), "{text}");
    assert!(text.contains("Block"), "{text}");
    assert!(text.contains("Beacon"), "{text}");
}
