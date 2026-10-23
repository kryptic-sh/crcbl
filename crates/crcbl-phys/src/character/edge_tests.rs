//! A character at the edge of a box or a mesh step: EW's report that a
//! capsule a radius past a box's top edge stood on air (2026-10-23), and the
//! steps a round capsule still has to climb now that edges are round.

use glam::DVec3;

use crate::collider::BoxCollider;
use crate::world::PhysicsWorld;

use super::{CharacterConfig, CharacterController};

/// One tick of a character under gravity, moving `walk` sideways.
fn tick(
    character: &mut CharacterController,
    world: &mut PhysicsWorld,
    fall: &mut f64,
    walk: DVec3,
) -> bool {
    const DT: f64 = 1.0 / 60.0;
    *fall -= 9.81 * DT;
    let outcome = character.move_and_slide(world, walk + DVec3::Y * (*fall * DT));
    if outcome.grounded {
        *fall = 0.0;
    }
    outcome.grounded
}

/// EW's platform: a box whose top is `y = 0` and whose edge is at `x = 0.5`,
/// with nothing below it, and a character standing with its axis at `x`.
fn on_the_platform(x: f64) -> (PhysicsWorld, CharacterController) {
    let mut world = PhysicsWorld::new();
    world.add_box(BoxCollider::new(
        DVec3::new(0.0, -0.5, 0.0),
        DVec3::new(0.5, 0.5, 2.0),
    ));
    let config = CharacterConfig::default();
    let feet = config.skin_width;
    let character = CharacterController::new(
        config,
        DVec3::new(x, feet + config.radius + config.half_height, 0.0),
    );
    (world, character)
}

/// **A character 0.29 past a box's edge does not stand on it** (EW's repro):
/// its radius is 0.3, so it touches only the edge's rounding, at a normal
/// far steeper than it can stand on, and it slides off and falls, where the
/// square edge an inflated box gave it held it up for good.
#[test]
fn a_character_a_radius_past_a_box_edge_falls_off_it() {
    let (mut world, mut character) = on_the_platform(0.79);
    let start = character.position().y;
    let mut fall = 0.0;
    let mut grounded = 0;
    for _ in 0..60 {
        if tick(&mut character, &mut world, &mut fall, DVec3::ZERO) {
            grounded += 1;
        }
    }
    assert!(grounded <= 1, "stood on the edge for {grounded} ticks");
    assert!(
        character.position().y < start - 1.0,
        "fell from {start} only to {:?}",
        character.position()
    );
}

/// **One standing 0.1 past the edge still stands**: the edge's rounding
/// there faces up within the slope limit, so the edge holds it.
#[test]
fn a_character_slightly_past_a_box_edge_still_stands() {
    let (mut world, mut character) = on_the_platform(0.6);
    let mut fall = 0.0;
    // Spawned a skin above the box's top, it is further than that above the
    // rounded edge it rests on there, and settles onto it first.
    for _ in 0..10 {
        tick(&mut character, &mut world, &mut fall, DVec3::ZERO);
    }
    for tick_index in 10..60 {
        assert!(
            tick(&mut character, &mut world, &mut fall, DVec3::ZERO),
            "fell on tick {tick_index} at {:?}",
            character.position()
        );
    }
    assert!(
        character.position().y > 0.5,
        "it rests on the edge: {:?}",
        character.position()
    );
}

/// **A mesh step is climbed like a box step**: a mesh's edges were always
/// round, so this is the case that showed the controller needed to step onto
/// a rounded edge at all.
#[test]
fn a_mesh_step_is_climbed() {
    use crate::{Transform, TriangleMesh};

    let height = 0.3;
    let mesh = TriangleMesh::new(
        &[
            DVec3::new(-5.0, 0.0, -5.0),
            DVec3::new(0.0, 0.0, -5.0),
            DVec3::new(0.0, 0.0, 5.0),
            DVec3::new(-5.0, 0.0, 5.0),
            DVec3::new(0.0, height, -5.0),
            DVec3::new(5.0, height, -5.0),
            DVec3::new(5.0, height, 5.0),
            DVec3::new(0.0, height, 5.0),
        ],
        &[
            [0, 2, 1],
            [0, 3, 2],
            [1, 2, 7],
            [1, 7, 4],
            [4, 7, 6],
            [4, 6, 5],
        ],
    )
    .expect("a floor, a riser and a step");
    let mut world = PhysicsWorld::new();
    world.add_mesh(mesh, Transform::IDENTITY);
    let config = CharacterConfig::default();
    let mut character = CharacterController::new(
        config,
        DVec3::new(
            -1.5,
            config.skin_width + config.radius + config.half_height,
            0.0,
        ),
    );
    let mut fall = 0.0;
    for _ in 0..80 {
        tick(&mut character, &mut world, &mut fall, DVec3::X * 0.05);
    }
    let feet = character.position().y - config.radius - config.half_height;
    assert!(
        character.position().x > 1.0,
        "stuck at {:?}",
        character.position()
    );
    assert!(
        (feet - height - config.skin_width).abs() < 0.02,
        "feet at {feet}"
    );
}
