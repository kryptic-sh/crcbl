//! The query world answers a turned box by its turned faces, in every query,
//! and a [`PhysicsSystem`] puts a body's box there turned with the body.
//!
//! The box is the unit cube turned 45° about `+Y` — the diamond
//! `|x| + |z| ≤ √2` — which `query::boxes`' own tests measure exactly; here
//! each query is the one place a hit or a miss differs from the unturned
//! cube's.

use core::f64::consts::FRAC_PI_4;

use glam::DVec3;

use crate::broadphase::{Ray, Segment};
use crate::collider::{BoxCollider, Capsule, LyingCapsule};
use crate::components::{ColliderComponent, RigidBody, Transform};
use crate::system::PhysicsSystem;

use super::{PhysicsWorld, QueryFilter};

/// A quarter of a half turn about `+Y`.
fn eighth_turn() -> glam::DQuat {
    crate::rotation_from_scaled_axis(DVec3::Y * FRAC_PI_4)
}

/// A world holding the unit cube, turned by `turned` or not.
fn world(turned: bool) -> PhysicsWorld {
    let cube = BoxCollider::new(DVec3::ZERO, DVec3::ONE);
    let mut world = PhysicsWorld::new();
    world.add_box(if turned {
        cube.with_rotation(eighth_turn())
    } else {
        cube
    });
    world
}

/// Whether `turned` and the unturned cube answer a query differently, and the
/// turned one answers `expected`.
fn turned_answers(mut query: impl FnMut(&mut PhysicsWorld) -> bool, expected: bool) {
    assert_eq!(query(&mut world(true)), expected, "the turned box");
    assert_eq!(query(&mut world(false)), !expected, "the unturned box");
}

#[test]
fn a_ray_hits_the_turned_box_where_the_unturned_one_is_not() {
    turned_answers(
        |world| {
            world
                .cast_ray(&Ray::new(DVec3::new(-5.0, 0.0, 1.2), DVec3::X))
                .is_some()
        },
        true,
    );
}

#[test]
fn a_sphere_sweep_and_overlap_meet_the_turned_box_only() {
    let path = Segment::new(DVec3::new(-5.0, 0.0, 1.25), DVec3::new(5.0, 0.0, 1.25));
    turned_answers(|world| world.sweep_sphere(&path, 0.1).is_some(), true);
    turned_answers(
        |world| {
            !world
                .overlap_sphere(DVec3::new(1.2, 0.0, 0.0), 0.1)
                .is_empty()
        },
        true,
    );
    turned_answers(
        |world| {
            !world
                .overlap_sphere(DVec3::new(0.85, 0.0, 0.85), 0.05)
                .is_empty()
        },
        false,
    );
}

#[test]
fn a_capsule_sweep_and_push_out_meet_the_turned_box_only() {
    let path = Segment::new(DVec3::new(-5.0, 0.0, 1.25), DVec3::new(5.0, 0.0, 1.25));
    turned_answers(|world| world.sweep_capsule(&path, 0.1, 0.5).is_some(), true);
    let mut out = Vec::new();
    let beside = Capsule::new(DVec3::new(1.15, 0.0, 0.0), 0.1, 0.5);
    turned_answers(
        |world| {
            world.capsule_penetrations_into(&beside, None, &mut out);
            !out.is_empty()
        },
        true,
    );
}

/// **A lying capsule is blocked by the turned box's corner**: lying along `Z`
/// at `x = 1.25`, it is clear of the unturned cube's face at 1 and inside the
/// diamond's corner, which reaches `√2`.
#[test]
fn a_lying_capsule_is_blocked_by_the_turned_corner() {
    let lying = LyingCapsule::new(DVec3::new(1.25, 0.0, -1.0), 0.0, 0.1, 2.0);
    turned_answers(
        |world| {
            world
                .lying_capsule_blocker(&lying, QueryFilter::ALL)
                .is_some()
        },
        true,
    );
}

/// **A body's box is put in the query world turned with the body, its offset
/// turned too**: a cube offset 2 along the body's `X`, on a body turned 45°
/// about `Y`, sits at `(√2, 0, -√2)` as a diamond. A ray down through that
/// centre's diamond corner region, outside the unturned cube there, hits.
#[test]
fn a_bodys_box_turns_with_the_body_in_the_query_world() {
    let entity =
        crcbl_ecs::Entity::from_bits((1u64 << 32) | 7).expect("generation 1 is never zero");
    let transform = Transform::new(DVec3::ZERO, eighth_turn());
    let mut phys = PhysicsSystem::new();
    phys.set_body(entity, RigidBody::new_kinematic());
    phys.set_transform(entity, transform);
    phys.set_collider(
        entity,
        &ColliderComponent::Box {
            offset: DVec3::new(2.0, 0.0, 0.0),
            half_extents: DVec3::ONE,
            is_trigger: false,
        },
        &transform,
    );
    let centre = transform.rotation * DVec3::new(2.0, 0.0, 0.0);
    let bounds = phys
        .world()
        .aabb_of(phys.collider_of(entity).expect("a collider"))
        .expect("in the world");
    assert!((bounds.centre() - centre).length() < 1e-12, "{bounds:?}");

    // Straight down at the diamond's corner on +X: 1.3 out from the centre,
    // past the unturned cube's face and inside the turned corner at √2.
    let down = Ray::new(centre + DVec3::new(1.3, 5.0, 0.0), DVec3::NEG_Y);
    let (hit, _) = phys.cast_ray(&down).expect("the turned corner");
    assert_eq!(hit, entity);
}
