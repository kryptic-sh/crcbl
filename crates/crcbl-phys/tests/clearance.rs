//! [`PhysicsSystem::farthest_clear_point`]: a sphere stopped short of a wall,
//! by its skin, measured along the path.

use crcbl_ecs::Entity;
use crcbl_phys::{ColliderComponent, PhysicsSystem, QueryFilter, Transform};
use glam::DVec3;

const RADIUS: f64 = 0.2;
const SKIN: f64 = 0.01;
const TOLERANCE: f64 = 1e-9;

fn entity(index: u32) -> Entity {
    Entity::from_bits((1u64 << 32) | u64::from(index)).expect("generation 1 is never zero")
}

/// A static wall, one metre thick, whose near face is the plane `x = 2`, and
/// a character's sphere standing at the origin.
fn room() -> (PhysicsSystem, Entity) {
    let mut physics = PhysicsSystem::new();
    let wall = entity(1);
    let at = Transform::from_position(DVec3::new(2.5, 0.0, 0.0));
    physics.set_transform(wall, at);
    physics.set_collider(
        wall,
        &ColliderComponent::Box {
            offset: DVec3::ZERO,
            half_extents: DVec3::new(0.5, 5.0, 5.0),
            is_trigger: false,
        },
        &at,
    );
    let character = entity(2);
    let here = Transform::IDENTITY;
    physics.set_transform(character, here);
    physics.set_collider(
        character,
        &ColliderComponent::Sphere {
            offset: DVec3::ZERO,
            radius: 0.4,
            is_trigger: false,
        },
        &here,
    );
    (physics, character)
}

fn excluding(physics: &PhysicsSystem, character: Entity) -> QueryFilter {
    QueryFilter::excluding(physics.collider_of(character))
}

/// **The sphere stops its radius and its skin in front of the wall.** Its
/// centre would touch at `x = 1.8`; the skin takes it to `1.79`.
#[test]
fn a_wall_stops_the_sphere_its_skin_short() {
    let (mut physics, character) = room();
    let filter = excluding(&physics, character);
    let eye = physics.farthest_clear_point(DVec3::ZERO, DVec3::X * 4.0, RADIUS, SKIN, filter);
    assert!(
        eye.distance(DVec3::X * (2.0 - RADIUS - SKIN)) < TOLERANCE,
        "{eye}"
    );
}

/// **Nothing in the way, or no way at all, gives back `to`.** The path stops
/// short of the wall, or has no length.
#[test]
fn a_clear_or_empty_path_reaches_its_end() {
    let (mut physics, character) = room();
    let filter = excluding(&physics, character);
    let to = DVec3::new(1.0, 0.5, 0.0);
    assert_eq!(
        physics.farthest_clear_point(DVec3::ZERO, to, RADIUS, SKIN, filter),
        to
    );
    let at = DVec3::new(0.3, 0.0, 0.0);
    assert_eq!(
        physics.farthest_clear_point(at, at, RADIUS, SKIN, filter),
        at
    );
}

/// **The filter is honoured.** Unfiltered, the character's own sphere stops
/// the sweep where it starts; left out, the wall does.
#[test]
fn the_filter_leaves_the_character_out() {
    let (mut physics, character) = room();
    let from = DVec3::ZERO;
    let to = DVec3::X * 4.0;
    assert_eq!(
        physics.farthest_clear_point(from, to, RADIUS, SKIN, QueryFilter::ALL),
        from,
        "starting inside the character, the sweep goes nowhere"
    );
    let filter = excluding(&physics, character);
    assert!(
        physics
            .farthest_clear_point(from, to, RADIUS, SKIN, filter)
            .x
            > 1.7
    );
}

/// **A skin longer than the way to the wall leaves the sphere where it
/// started**, never behind it.
#[test]
fn a_long_skin_clamps_to_the_start() {
    let (mut physics, character) = room();
    let filter = excluding(&physics, character);
    let from = DVec3::X * 1.7;
    assert_eq!(
        physics.farthest_clear_point(from, DVec3::X * 4.0, RADIUS, 0.5, filter),
        from
    );
}
