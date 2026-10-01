//! The body component through the scene format, its refusals, and the module
//! simulating it as a tool's play ticks it.

use std::hash::Hasher;

use crcbl_assets::MemorySource;
use crcbl_scene::scn::{IdMap, ScnError};

use super::*;

/// A placing component whose `position` is its centre — a greybox block.
#[derive(Clone, Copy, Debug, Default, PartialEq, Reflect, Serialize, Deserialize)]
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
    fn placement(&self) -> Option<OrientedBox> {
        Some(OrientedBox::axis_aligned(
            DVec3::from_array(self.position),
            DVec3::from_array(self.half_extents),
        ))
    }
}

/// A placing component that **stands on** its `position`: its centre is
/// [`PAD_HALF`] above it, so a pad at rest on the ground has `position.1` at
/// the ground, where a block's centre would be half its height up.
#[derive(Clone, Copy, Debug, Default, PartialEq, Reflect, Serialize, Deserialize)]
#[reflect(crate = "crcbl_reflect")]
struct Pad {
    position: [f64; 3],
}

/// A pad's half extent on every axis.
const PAD_HALF: f64 = 0.25;

impl ComponentHash for Pad {
    fn hash_component(&self, hasher: &mut dyn Hasher) {
        for value in self.position {
            hasher.write(&value.to_bits().to_le_bytes());
        }
    }
}

impl Placement for Pad {
    fn placement(&self) -> Option<OrientedBox> {
        Some(OrientedBox::axis_aligned(
            DVec3::from_array(self.position) + DVec3::new(0.0, PAD_HALF, 0.0),
            DVec3::splat(PAD_HALF),
        ))
    }
}

/// A placing component with no `position`: its centre is spelled otherwise.
#[derive(Clone, Copy, Debug, Default, PartialEq, Reflect, Serialize, Deserialize)]
#[reflect(crate = "crcbl_reflect")]
struct Marker {
    centre: [f64; 3],
}

impl ComponentHash for Marker {
    fn hash_component(&self, hasher: &mut dyn Hasher) {
        for value in self.centre {
            hasher.write(&value.to_bits().to_le_bytes());
        }
    }
}

impl Placement for Marker {
    fn placement(&self) -> Option<OrientedBox> {
        Some(OrientedBox::axis_aligned(
            DVec3::from_array(self.centre),
            DVec3::splat(0.5),
        ))
    }
}

/// The three placing components and the bodies.
fn registry() -> Registry {
    let mut registry = Registry::new();
    registry.register::<Block>("blocks");
    registry.register::<Pad>("pads");
    registry.register::<Marker>("markers");
    register(&mut registry);
    registry
}

/// A scene of `systems`, with `chunks` as `(system, rows)` — each row an
/// `(id, component)` pair as a chunk file spells it.
fn scene(systems: &[&str], chunks: &[(&str, &str)]) -> MemorySource {
    let listed: Vec<String> = systems.iter().map(|system| format!("{system:?}")).collect();
    let mut source = MemorySource::new();
    let mut files = vec![
        (
            "scene.ron".to_owned(),
            format!(
                "(format: 0, name: \"bodies\", systems: [{}])",
                listed.join(", ")
            ),
        ),
        (
            "env.ron".to_owned(),
            "(camera: (position: (0.0, 2.0, 12.0), look_at: (0.0, 0.0, 0.0)), \
             ambient: (0.1, 0.1, 0.1))"
                .to_owned(),
        ),
    ];
    for (system, rows) in chunks {
        files.push((
            format!("sys/{system}.ron"),
            format!("(system: {system:?}, entities: [{rows}])"),
        ));
    }
    for (key, text) in files {
        source
            .insert(Path::new(&key), text.into_bytes())
            .expect("a scene key is a legal asset key");
    }
    source
}

/// The ground: a slab whose top is `y = 0`.
const SLAB: &str = "(0, (position: (0.0, -0.5, 0.0), half_extents: (8.0, 0.5, 8.0)))";

/// How high the falling block starts, in metres: its centre.
const DROP_Y: f64 = 3.0;

/// The falling block's half extent on every axis.
const HALF: f64 = 0.5;

/// Where the kinematic block hangs, in metres: well clear of the ground, so a
/// kinematic body that fell would be seen to.
const HANG_Y: f64 = 2.0;

/// The ground, a block over it, and a block hanging to one side.
fn drop_blocks() -> String {
    format!(
        "{SLAB}, (1, (position: (0.0, {DROP_Y:?}, 0.0), half_extents: ({HALF:?}, {HALF:?}, \
         {HALF:?}))), (2, (position: (4.0, {HANG_Y:?}, 0.0), half_extents: (0.5, 0.5, 0.5)))"
    )
}

/// The slab static, the block over it dynamic, the hanging block kinematic.
const DROP_BODIES: &str = "(0, (kind: Static, mass: 1.0, friction: 0.6, restitution: 0.0)), \
     (1, (kind: Dynamic, mass: 2.0, friction: 0.6, restitution: 0.0)), \
     (2, (kind: Kinematic, mass: 1.0, friction: 0.6, restitution: 0.0))";

/// [`drop_blocks`] with [`DROP_BODIES`] beside them.
fn drop_scene() -> MemorySource {
    let blocks = drop_blocks();
    scene(
        &["blocks", "bodies"],
        &[("blocks", &blocks), ("bodies", DROP_BODIES)],
    )
}

/// The scene at `source` loaded through `registry`, as a tool's world.
fn load(registry: &Registry, source: &MemorySource) -> (World, IdMap) {
    let mut world = World::new();
    registry.register_systems(&mut world);
    let (_, ids) = Scene::load(source, Path::new(""), &registry.codecs(), &mut world)
        .expect("the scene loads");
    (world, ids)
}

/// A world playing `source`: loaded, with the bodies' module built from the
/// same files and registered on it, as a tool's play does.
fn playing(source: &MemorySource) -> (World, IdMap, Box<dyn GameModule>) {
    let registry = registry();
    let mut modules = registry
        .modules(&[BODIES.to_owned()], source, Path::new(""))
        .expect("every body here can be simulated");
    assert_eq!(modules.len(), 1, "the bodies' module is registered once");
    let module = modules.remove(0);
    let (mut world, ids) = load(&registry, source);
    module.register(&mut world);
    (world, ids, module)
}

/// `ticks` ticks in a tool's order: the schedule, the module, a sweep.
fn run(world: &mut World, module: &mut dyn GameModule, ticks: u32) {
    for _ in 0..ticks {
        world.tick();
        module.tick(world, ClientInputs::empty());
        world.sweep();
    }
}

/// The ticks in `seconds` of play at the world's default rate.
fn ticks_in(seconds: f64) -> u32 {
    #[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)]
    let ticks = (seconds / World::DEFAULT_TICK_DT).round() as u32;
    ticks
}

/// The `system` component of the entity filed under `id`.
fn component<T: ComponentHash + Copy + 'static>(
    world: &mut World,
    ids: &IdMap,
    system: &str,
    id: u32,
) -> T {
    let entity = ids
        .entity(SceneEntityId(id))
        .expect("the id is in the scene");
    world
        .schedule_mut()
        .iter_mut()
        .find(|each| each.name() == system)
        .and_then(|each| each.as_any_mut().downcast_mut::<System<T>>())
        .and_then(|each| each.get(entity).copied())
        .expect("the entity is in that system")
}

/// The block filed under `id`.
fn block(world: &mut World, ids: &IdMap, id: u32) -> Block {
    component(world, ids, "blocks", id)
}

/// How far a resting box may sit off the surface it rests on, in metres: the
/// contact solver's soft contacts settle within a few millimetres.
const REST_TOLERANCE: f64 = 0.01;

/// **A dynamic body falls under gravity, and its placing component moves
/// with it** — every tick of the fall, not only at the end — **and comes to
/// rest on the static slab below**: its bottom on the slab's top.
#[test]
fn a_dynamic_body_falls_and_comes_to_rest_on_the_static_one_below() {
    let source = drop_scene();
    let (mut world, ids, mut module) = playing(&source);

    run(&mut world, module.as_mut(), ticks_in(0.25));
    let falling = block(&mut world, &ids, 1);
    let expected = DROP_Y - 0.5 * 9.81 * 0.25 * 0.25;
    assert!(
        (falling.position[1] - expected).abs() < 0.05,
        "a quarter second in, the block is at {:?}, not near y = {expected}",
        falling.position,
    );

    run(&mut world, module.as_mut(), ticks_in(3.0));
    let rested = block(&mut world, &ids, 1);
    assert!(
        (rested.position[1] - HALF).abs() < REST_TOLERANCE,
        "the block came to rest at {:?}, not on the slab's top",
        rested.position,
    );
    assert_eq!(
        (rested.position[0], rested.position[2]),
        (0.0, 0.0),
        "a straight fall moved the block sideways",
    );
    assert_eq!(rested.half_extents, [HALF; 3], "the fall resized the block");
}

/// **A static body never moves, and nor does a kinematic one with no
/// velocity** — the kinematic block hangs in the air, so if it were stepped
/// as dynamic it would fall. Read from the simulation as well as from the
/// blocks: only a dynamic body's pose is written back, so a block standing
/// still says nothing about a body that fell.
#[test]
fn static_and_kinematic_bodies_never_move() {
    let source = drop_scene();
    let (mut world, ids, mut module) = playing(&source);
    let slab = block(&mut world, &ids, 0);
    let hanging = block(&mut world, &ids, 2);
    run(&mut world, module.as_mut(), ticks_in(2.0));
    assert_eq!(block(&mut world, &ids, 0), slab, "the static slab moved");
    assert_eq!(
        block(&mut world, &ids, 2),
        hanging,
        "the kinematic block moved"
    );
    let simulation = world
        .system_mut::<Simulation>()
        .expect("the module registered its simulation");
    for (id, placed) in [(0, slab.position), (2, hanging.position)] {
        let entity = ids.entity(SceneEntityId(id)).expect("in the scene");
        let simulated = simulation
            .physics()
            .transform(entity)
            .expect("every body is simulated")
            .position;
        assert_eq!(
            simulated,
            DVec3::from_array(placed),
            "the simulated body of #{id} moved",
        );
    }
    assert!(
        block(&mut world, &ids, 1).position[1] < DROP_Y - 1.0,
        "the dynamic block did not fall, so nothing here was stepped",
    );
}

/// **No tunnelling at the tick rate**: a block dropped from high enough to
/// cross a thin slab in less than a tick still comes to rest on it.
#[test]
fn a_fast_body_does_not_tunnel_through_a_thin_static_one() {
    const HIGH: f64 = 40.0;
    const THIN: f64 = 0.05;
    let blocks = format!(
        "(0, (position: (0.0, {:?}, 0.0), half_extents: (8.0, {THIN:?}, 8.0))), \
         (1, (position: (0.0, {HIGH:?}, 0.0), half_extents: (0.5, 0.5, 0.5)))",
        -THIN
    );
    let bodies = "(0, (kind: Static, mass: 1.0, friction: 0.6, restitution: 0.0)), \
                  (1, (kind: Dynamic, mass: 1.0, friction: 0.6, restitution: 0.0))";
    let source = scene(
        &["blocks", "bodies"],
        &[("blocks", &blocks), ("bodies", bodies)],
    );
    let (mut world, ids, mut module) = playing(&source);
    // Its speed at the slab, against how far one tick carries it: the slab is
    // thinner than that, so a body moved only by integration would pass it.
    let speed = (2.0 * 9.81 * HIGH).sqrt();
    assert!(speed * World::DEFAULT_TICK_DT > 2.0 * THIN);
    run(&mut world, module.as_mut(), ticks_in(6.0));
    let rested = block(&mut world, &ids, 1);
    assert!(
        (rested.position[1] - HALF).abs() < REST_TOLERANCE,
        "the block ended at {:?}, not resting on the thin slab",
        rested.position,
    );
}

/// **The pose written back keeps the component's own offset**: a pad stands
/// on its `position`, so at rest on the ground its `position.1` is the
/// ground's height — where writing the simulated centre would have put it
/// half a pad up.
#[test]
fn a_component_whose_centre_is_not_its_position_moves_by_its_offset() {
    let pads = "(1, (position: (0.0, 3.0, 0.0)))";
    let bodies = "(0, (kind: Static, mass: 1.0, friction: 0.6, restitution: 0.0)), \
                  (1, (kind: Dynamic, mass: 1.0, friction: 0.6, restitution: 0.0))";
    let source = scene(
        &["blocks", "pads", "bodies"],
        &[("blocks", SLAB), ("pads", pads), ("bodies", bodies)],
    );
    let (mut world, ids, mut module) = playing(&source);
    run(&mut world, module.as_mut(), ticks_in(3.0));
    let pad: Pad = component(&mut world, &ids, "pads", 1);
    assert!(
        pad.position[1].abs() < REST_TOLERANCE,
        "the pad came to rest with its position at {:?}, not on the ground",
        pad.position,
    );
}

/// **Two plays of the same scene step the same bodies to the same poses**, to
/// the bit: a stack of three, so the order bodies are solved in shows.
#[test]
fn two_plays_of_one_scene_end_in_identical_poses() {
    let blocks = format!(
        "{SLAB}, (1, (position: (0.0, 1.0, 0.0), half_extents: (0.5, 0.5, 0.5))), \
         (2, (position: (0.2, 2.5, 0.0), half_extents: (0.5, 0.5, 0.5))), \
         (3, (position: (-0.2, 4.0, 0.1), half_extents: (0.5, 0.5, 0.5)))"
    );
    let bodies = "(0, (kind: Static, mass: 1.0, friction: 0.6, restitution: 0.0)), \
                  (1, (kind: Dynamic, mass: 1.0, friction: 0.6, restitution: 0.2)), \
                  (2, (kind: Dynamic, mass: 3.0, friction: 0.4, restitution: 0.0)), \
                  (3, (kind: Dynamic, mass: 0.5, friction: 0.9, restitution: 0.5))";
    let source = scene(
        &["blocks", "bodies"],
        &[("blocks", &blocks), ("bodies", bodies)],
    );
    let play = || {
        let (mut world, ids, mut module) = playing(&source);
        run(&mut world, module.as_mut(), ticks_in(2.0));
        (1..=3)
            .map(|id| block(&mut world, &ids, id).position)
            .collect::<Vec<_>>()
    };
    let first = play();
    assert_ne!(
        first[2][1], 4.0,
        "the top of the stack never moved, so nothing was compared"
    );
    let bits = |poses: &[[f64; 3]]| -> Vec<u64> {
        poses
            .iter()
            .flatten()
            .map(|value| value.to_bits())
            .collect()
    };
    assert_eq!(bits(&play()), bits(&first));
}

/// **A body round-trips through the scene format**, and an entity with a
/// block and a body is one entity placed by its block.
#[test]
fn a_body_round_trips_and_a_block_with_a_body_is_one_entity() {
    let registry = registry();
    let source = drop_scene();
    let mut world = World::new();
    registry.register_systems(&mut world);
    let (scene, ids) = Scene::load(&source, Path::new(""), &registry.codecs(), &mut world)
        .expect("the scene loads");
    assert_eq!(ids.len(), 3, "a block and its body are one entity, not two");
    let entity = ids.entity(SceneEntityId(1)).expect("the falling block");
    assert_eq!(registry.systems_of(&mut world, entity), ["blocks", BODIES]);
    assert_eq!(
        registry.placing_system(&mut world, entity).as_deref(),
        Some("blocks"),
        "the body, which places nothing, placed the entity",
    );
    let body: Body = component(&mut world, &ids, BODIES, 1);
    assert_eq!(
        body,
        Body {
            kind: BodyKind::Dynamic,
            mass: 2.0,
            friction: 0.6,
            restitution: 0.0,
        }
    );
    let hanging: Body = component(&mut world, &ids, BODIES, 2);
    assert_eq!(hanging.kind, BodyKind::Kinematic);

    let written = scene
        .save(&mut world, &ids, &registry.codecs())
        .expect("the scene saves");
    assert!(
        written["sys/bodies.ron"].contains("kind: Kinematic"),
        "{}",
        written["sys/bodies.ron"],
    );
    let mut again = MemorySource::new();
    for (key, text) in &written {
        again
            .insert(Path::new(key), text.clone().into_bytes())
            .expect("a scene key");
    }
    let mut reloaded = World::new();
    registry.register_systems(&mut reloaded);
    let (scene, ids) = Scene::load(&again, Path::new(""), &registry.codecs(), &mut reloaded)
        .expect("what the writer wrote loads");
    assert_eq!(
        scene
            .save(&mut reloaded, &ids, &registry.codecs())
            .expect("saves"),
        written,
        "a body did not survive the round trip byte for byte",
    );
}

/// A new body is a dynamic one of one kilogram on the default surface.
#[test]
fn a_new_body_is_dynamic_with_unit_mass() {
    assert_eq!(
        registry()
            .default_row(BODIES)
            .expect("bodies is registered"),
        "(kind:Dynamic,mass:1.0,friction:0.6,restitution:0.0)",
    );
    assert_eq!(Body::default().check(), Ok(()));
}

/// **A body no simulation takes is refused on load, naming the field and the
/// file** — and the same values are what the bodies' check reports, so an
/// editor saving them is told where it made them.
#[test]
fn invalid_body_values_are_refused_on_load_by_name() {
    let registry = registry();
    for (row, field) in [
        (
            "kind: Dynamic, mass: -1.0, friction: 0.6, restitution: 0.0",
            "`mass`",
        ),
        (
            "kind: Static, mass: 0.0, friction: 0.6, restitution: 0.0",
            "`mass`",
        ),
        (
            "kind: Dynamic, mass: inf, friction: 0.6, restitution: 0.0",
            "`mass`",
        ),
        (
            "kind: Dynamic, mass: 1.0, friction: -0.1, restitution: 0.0",
            "`friction`",
        ),
        (
            "kind: Dynamic, mass: 1.0, friction: NaN, restitution: 0.0",
            "`friction`",
        ),
        (
            "kind: Dynamic, mass: 1.0, friction: 0.6, restitution: 1.5",
            "`restitution`",
        ),
        (
            "kind: Dynamic, mass: 1.0, friction: 0.6, restitution: -0.5",
            "`restitution`",
        ),
    ] {
        let bodies = format!("(0, ({row}))");
        let source = scene(
            &["blocks", "bodies"],
            &[("blocks", SLAB), ("bodies", &bodies)],
        );
        let mut world = World::new();
        registry.register_systems(&mut world);
        let error = Scene::load(&source, Path::new(""), &registry.codecs(), &mut world)
            .err()
            .unwrap_or_else(|| panic!("`{row}` loaded"));
        assert!(
            matches!(&error, ScnError::Parse { key, message, .. }
                if key == "sys/bodies.ron" && message.contains(field)),
            "`{row}` was refused without naming {field} in its file: {error}",
        );
        let problems = registry.problems(&[BODIES.to_owned()], &source, Path::new(""));
        assert!(
            problems.len() == 1 && problems[0].contains(field),
            "the check did not report {field} for `{row}`: {problems:?}",
        );
    }
    assert!(
        registry
            .problems(&[BODIES.to_owned()], &drop_scene(), Path::new(""))
            .is_empty(),
        "the check refused bodies a simulation takes",
    );
}

/// The refusal [`playing`] would have been handed for `source`.
fn refusal(source: &MemorySource) -> String {
    registry()
        .modules(&[BODIES.to_owned()], source, Path::new(""))
        .err()
        .expect("the scene is refused")
}

/// **A body with nothing placing it refuses play, naming the entity.**
#[test]
fn a_body_with_no_placement_refuses_play_by_name() {
    let bodies = "(0, (kind: Static, mass: 1.0, friction: 0.6, restitution: 0.0)), \
                  (5, (kind: Dynamic, mass: 1.0, friction: 0.6, restitution: 0.0))";
    let source = scene(
        &["blocks", "bodies"],
        &[("blocks", SLAB), ("bodies", bodies)],
    );
    let refused = refusal(&source);
    assert!(
        refused.starts_with("entity #5 ") && refused.contains("nothing that places it"),
        "{refused}",
    );
}

/// A named entity's refusal names it as an outliner row reads: its name and
/// its id.
#[test]
fn a_named_bodys_refusal_names_it() {
    let bodies = "(5, (kind: Dynamic, mass: 1.0, friction: 0.6, restitution: 0.0))";
    let mut source = scene(
        &["blocks", "bodies"],
        &[("blocks", SLAB), ("bodies", bodies)],
    );
    for (key, text) in [
        (
            "scene.ron",
            "(format: 0, name: \"bodies\", systems: [\"blocks\", \"bodies\"], names: true)",
        ),
        ("names.ron", "(names: [(5, \"Crate\")])"),
    ] {
        source
            .insert(Path::new(key), text.as_bytes().to_vec())
            .expect("a scene key is a legal asset key");
    }
    let refused = refusal(&source);
    assert!(refused.starts_with("entity `Crate` #5 "), "{refused}");
}

/// A body whose box has no extent on an axis, or whose placing component has
/// no `position` to write into, refuses play naming the entity and why.
#[test]
fn a_body_that_cannot_be_a_box_or_written_back_refuses_play() {
    let flat = "(1, (position: (0.0, 1.0, 0.0), half_extents: (0.5, 0.0, 0.5)))";
    let bodies = "(1, (kind: Dynamic, mass: 1.0, friction: 0.6, restitution: 0.0))";
    let refused = refusal(&scene(
        &["blocks", "bodies"],
        &[("blocks", flat), ("bodies", bodies)],
    ));
    assert!(
        refused.starts_with("entity #1 ") && refused.contains("half extents"),
        "{refused}",
    );

    let marker = "(1, (centre: (0.0, 1.0, 0.0)))";
    let refused = refusal(&scene(
        &["markers", "bodies"],
        &[("markers", marker), ("bodies", bodies)],
    ));
    assert!(
        refused.starts_with("entity #1 ")
            && refused.contains("`markers`")
            && refused.contains("no `position`"),
        "{refused}",
    );
}

/// **The simulated bodies are in a system of their own**: the world's
/// `PhysicsSystem` — what a tool picks through — is not where they are, and
/// the module registers exactly one body per row.
#[test]
fn the_simulation_is_a_system_of_its_own() {
    let source = drop_scene();
    let (mut world, ids, _module) = playing(&source);
    assert!(
        world.system_mut::<PhysicsSystem>().is_none(),
        "the module registered a plain PhysicsSystem a tool's picking would find",
    );
    let simulation = world
        .system_mut::<Simulation>()
        .expect("the module registered its simulation");
    assert_eq!(simulation.physics().collider_count(), 3);
    assert_eq!(
        simulation.physics().body_count(),
        2,
        "the dynamic and the kinematic block have bodies; the static slab none",
    );
    let falling = ids.entity(SceneEntityId(1)).expect("the falling block");
    assert_eq!(
        simulation.poses(),
        [(falling, DVec3::new(0.0, DROP_Y, 0.0))]
    );
}

/// **Rotation is locked**: a block dropped half over the edge of a narrow
/// pillar would tip off it if it could turn. It cannot, so it comes to rest
/// level on the pillar's top, unrotated, exactly as it is drawn.
#[test]
fn a_body_does_not_rotate_even_landing_off_centre() {
    const PILLAR_HALF: f64 = 0.2;
    const PILLAR_TOP: f64 = 2.0;
    let blocks = format!(
        "{SLAB}, (1, (position: (0.0, {:?}, 0.0), half_extents: ({PILLAR_HALF:?}, {:?},          {PILLAR_HALF:?}))), (2, (position: (0.45, 3.0, 0.0), half_extents: (0.5, 0.5, 0.5)))",
        0.5 * PILLAR_TOP,
        0.5 * PILLAR_TOP,
    );
    let bodies = "(0, (kind: Static, mass: 1.0, friction: 0.6, restitution: 0.0)),                   (1, (kind: Static, mass: 1.0, friction: 0.6, restitution: 0.0)),                   (2, (kind: Dynamic, mass: 1.0, friction: 0.6, restitution: 0.0))";
    let source = scene(
        &["blocks", "bodies"],
        &[("blocks", &blocks), ("bodies", bodies)],
    );
    let (mut world, ids, mut module) = playing(&source);
    run(&mut world, module.as_mut(), ticks_in(3.0));
    let rested = block(&mut world, &ids, 2);
    assert!(
        (rested.position[1] - (PILLAR_TOP + HALF)).abs() < REST_TOLERANCE,
        "the block ended at {:?}, not level on the pillar",
        rested.position,
    );
    let entity = ids.entity(SceneEntityId(2)).expect("the dropped block");
    let rotation = world
        .system_mut::<Simulation>()
        .and_then(|simulation| simulation.physics().transform(entity).copied())
        .expect("the dropped block is simulated")
        .rotation;
    assert_eq!(rotation, glam::DQuat::IDENTITY, "the block turned");
}

/// A placing component that may be turned — the editor's greybox block with
/// its rotation.
#[derive(Clone, Copy, Debug, Default, PartialEq, Reflect, Serialize, Deserialize)]
#[reflect(crate = "crcbl_reflect")]
struct Tilt {
    position: [f64; 3],
    half_extents: [f64; 3],
    #[serde(
        default,
        skip_serializing_if = "crate::registry::Rotation::is_identity"
    )]
    rotation: crate::registry::Rotation,
}

impl ComponentHash for Tilt {
    fn hash_component(&self, hasher: &mut dyn Hasher) {
        let numbers = self.position.into_iter().chain(self.half_extents);
        for value in numbers.chain(self.rotation.to_array()) {
            hasher.write(&value.to_bits().to_le_bytes());
        }
    }
}

impl Placement for Tilt {
    fn placement(&self) -> Option<OrientedBox> {
        Some(OrientedBox::new(
            DVec3::from_array(self.position),
            DVec3::from_array(self.half_extents),
            self.rotation.quat(),
        ))
    }
}

/// **A turned body starts at its rotation and keeps it**: a cube tipped a
/// twelfth of a turn about `+Z` falls onto the slab, lands on its edge and
/// rests there, still tipped — its collider was made turned, so it rests
/// with its lowest edge on the slab rather than its face, and its rotation is
/// locked, so it never falls over onto that face.
#[test]
fn a_turned_body_starts_at_its_rotation_and_keeps_it() {
    let mut registry = registry();
    registry.register::<Tilt>("tilts");
    let tip = std::f64::consts::FRAC_PI_6;
    let rotation = glam::DQuat::from_rotation_z(tip);
    let [x, y, z, w] = rotation.to_array();
    let tilts = format!(
        "(1, (position: (0.0, {DROP_Y:?}, 0.0), half_extents: ({HALF:?}, {HALF:?}, {HALF:?}), \
         rotation: ({x:?}, {y:?}, {z:?}, {w:?})))"
    );
    let source = scene(
        &["blocks", "tilts", "bodies"],
        &[
            ("blocks", SLAB),
            ("tilts", &tilts),
            (
                "bodies",
                "(0, (kind: Static, mass: 1.0, friction: 0.6, restitution: 0.0)), \
                 (1, (kind: Dynamic, mass: 1.0, friction: 0.6, restitution: 0.0))",
            ),
        ],
    );
    let mut modules = registry
        .modules(&[BODIES.to_owned()], &source, Path::new(""))
        .expect("every body here can be simulated");
    let mut module = modules.remove(0);
    let (mut world, ids) = load(&registry, &source);
    module.register(&mut world);
    let entity = ids.entity(SceneEntityId(1)).expect("in the scene");
    let simulated = |world: &mut World| {
        world
            .system_mut::<Simulation>()
            .and_then(|simulation| simulation.physics().transform(entity).copied())
            .expect("the tipped cube is simulated")
    };
    assert_eq!(
        simulated(&mut world).rotation,
        rotation,
        "it started unturned"
    );

    run(&mut world, module.as_mut(), ticks_in(3.0));
    let rested = simulated(&mut world);
    assert_eq!(rested.rotation, rotation, "a locked rotation turned");
    let tipped: Tilt = component(&mut world, &ids, "tilts", 1);
    assert_eq!(tipped.rotation.quat(), rotation, "the row's rotation moved");
    // Resting on its lowest edge: the centre stands as high as the turned
    // cube reaches down, which is more than its half extent.
    let reach = HALF * (tip.cos() + tip.sin());
    assert!(
        (tipped.position[1] - reach).abs() < REST_TOLERANCE,
        "the tipped cube came to rest at {:?}, not on its edge at y = {reach}",
        tipped.position,
    );
}
