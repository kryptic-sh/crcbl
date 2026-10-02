//! The query world answers a compound by its parts, not by the world-axis box
//! around them.
//!
//! The fixture is a U: two columns, `x ∈ [-2, -1]` and `x ∈ [1, 2]`, standing
//! on a base whose top is at `y = -1`, all `z ∈ [-1, 1]`. The box around it
//! fills the gap between the columns, so each query here is one the old
//! single box answered differently.

use core::f64::consts::FRAC_PI_2;

use glam::{DQuat, DVec3};

use crate::broadphase::{Ray, Segment};
use crate::collider::{Aabb, Capsule, LyingCapsule};
use crate::components::{ColliderComponent, RigidBody, Transform};
use crate::compound_shape::{CompoundPart, CompoundShape};
use crate::query::ShapeHit;
use crate::system::PhysicsSystem;

use super::{ColliderId, PhysicsWorld, QueryFilter};

/// How closely a hit worked out by hand has to agree with a closed-form one.
const EXACT: f64 = 1e-12;

/// The U's parts, in order: the column on `-X`, the column on `+X`, the base.
const LEFT: usize = 0;
const RIGHT: usize = 1;
const BASE: usize = 2;

fn u_shape() -> CompoundShape {
    let part = |centre: DVec3, half: DVec3| CompoundPart::new(centre, DQuat::IDENTITY, half);
    CompoundShape::new(vec![
        part(DVec3::new(-1.5, 0.0, 0.0), DVec3::new(0.5, 1.0, 1.0)),
        part(DVec3::new(1.5, 0.0, 0.0), DVec3::new(0.5, 1.0, 1.0)),
        part(DVec3::new(0.0, -1.5, 0.0), DVec3::new(2.0, 0.5, 1.0)),
    ])
    .expect("a valid shape")
}

/// A world holding the U at the origin, unturned.
fn world() -> (PhysicsWorld, ColliderId) {
    let mut world = PhysicsWorld::new();
    let id = world.add_compound(&u_shape(), DVec3::ZERO, &Transform::IDENTITY);
    (world, id)
}

fn close(a: DVec3, b: DVec3) -> bool {
    (a - b).length() < EXACT
}

/// **A ray down the gap hits the base, not the box's top**: the old box was
/// met at `y = 1`, the base's top is at `y = -1`.
#[test]
fn a_ray_down_the_gap_hits_the_base_below_it() {
    let (mut world, id) = world();
    let (hit, at) = world
        .cast_ray(&Ray::new(DVec3::new(0.0, 5.0, 0.0), DVec3::NEG_Y))
        .expect("the base");
    assert_eq!((hit, at.part), (id, BASE));
    assert!((at.t - 6.0).abs() < EXACT, "{at:?}");
    assert!(close(at.point, DVec3::new(0.0, -1.0, 0.0)), "{at:?}");
    assert_eq!(at.normal, DVec3::Y);
}

/// **A ray, a sphere sweep and a capsule sweep through the gap meet
/// nothing.**
#[test]
fn rays_and_sweeps_through_the_gap_miss() {
    let (mut world, _) = world();
    let along = Segment::new(DVec3::new(0.0, 0.0, -5.0), DVec3::new(0.0, 0.0, 5.0));
    assert_eq!(
        world.cast_ray(&Ray::new(along.start, along.end - along.start)),
        None
    );
    assert_eq!(world.sweep_sphere(&along, 0.5), None);
    assert_eq!(world.sweep_capsule(&along, 0.3, 0.4), None);
    let mut hits = Vec::new();
    world.sweep_sphere_all(&along, 0.5, QueryFilter::ALL, &mut hits);
    assert!(hits.is_empty(), "{hits:?}");
}

/// **Overlaps, push-outs and a lying capsule in the gap find nothing**, and
/// beside a column the push-out is that column's.
#[test]
fn overlaps_and_push_outs_in_the_gap_find_nothing() {
    let (mut world, id) = world();
    assert!(world.overlap_sphere(DVec3::ZERO, 0.5).is_empty());
    let small = Aabb::from_centre_half(DVec3::ZERO, DVec3::splat(0.25));
    assert!(world.overlap_aabb(&small).is_empty());

    let mut out = Vec::new();
    world.capsule_penetrations_into(&Capsule::new(DVec3::ZERO, 0.3, 0.4), None, &mut out);
    assert!(out.is_empty(), "{out:?}");
    let lying = LyingCapsule::new(DVec3::new(0.0, 0.0, -0.5), 0.0, 0.3, 1.0);
    assert_eq!(world.lying_capsule_blocker(&lying, QueryFilter::ALL), None);

    // 0.2 from the right column's face at x = 1, with a radius of 0.3.
    world.capsule_penetrations_into(
        &Capsule::new(DVec3::new(0.8, 0.0, 0.0), 0.3, 0.4),
        None,
        &mut out,
    );
    let [(hit, push)] = out[..] else {
        panic!("one push-out, not {out:?}");
    };
    assert_eq!(hit, id);
    assert!((push.depth - 0.1).abs() < EXACT, "{push:?}");
    assert!(close(push.normal, DVec3::NEG_X), "{push:?}");
}

/// **The candidate sweeps report each part they meet**, nearest first, each
/// with its part index, and the first is the closest sweep's answer to the
/// bit.
#[test]
fn the_candidate_sweeps_report_each_part() {
    let (mut world, id) = world();
    let path = Segment::new(DVec3::new(-5.0, 0.0, 0.0), DVec3::new(5.0, 0.0, 0.0));
    let radius = 0.25;
    for capsule in [false, true] {
        let mut hits = Vec::new();
        let closest = if capsule {
            world.sweep_capsule_all(&path, radius, 0.5, QueryFilter::ALL, &mut hits);
            world.sweep_capsule(&path, radius, 0.5)
        } else {
            world.sweep_sphere_all(&path, radius, QueryFilter::ALL, &mut hits);
            world.sweep_sphere(&path, radius)
        };
        let [(first, left), (second, right)] = hits[..] else {
            panic!("both columns, not {hits:?} (capsule {capsule})");
        };
        assert_eq!((first, left.part), (id, LEFT));
        assert_eq!((second, right.part), (id, RIGHT));
        // The left column's face is at x = -2, the right one's at x = 1.
        assert!((left.t - (3.0 - radius) / 10.0).abs() < EXACT, "{left:?}");
        assert!((right.t - (6.0 - radius) / 10.0).abs() < EXACT, "{right:?}");
        assert_eq!(left.normal, DVec3::NEG_X);
        assert_eq!(right.normal, DVec3::NEG_X);
        assert_eq!(closest, Some(hits[0]), "capsule {capsule}");
    }
}

/// **Two parts met at the same fraction come in part order**, and the
/// closest sweep and ray pick the first.
#[test]
fn parts_met_together_come_in_part_order() {
    let mut world = PhysicsWorld::new();
    let part = |y: f64| CompoundPart::new(DVec3::new(4.0, y, 0.0), DQuat::IDENTITY, DVec3::ONE);
    let shape = CompoundShape::new(vec![part(1.0), part(-1.0)]).expect("a valid shape");
    let id = world.add_compound(&shape, DVec3::ZERO, &Transform::IDENTITY);
    let path = Segment::new(DVec3::ZERO, DVec3::new(5.0, 0.0, 0.0));

    let mut hits: Vec<(ColliderId, ShapeHit)> = Vec::new();
    world.sweep_sphere_all(&path, 0.5, QueryFilter::ALL, &mut hits);
    let parts: Vec<_> = hits.iter().map(|&(hit, at)| (hit, at.part)).collect();
    assert_eq!(parts, [(id, 0), (id, 1)]);
    assert_eq!(hits[0].1.t, hits[1].1.t, "a tie");
    assert_eq!(world.sweep_sphere(&path, 0.5), Some(hits[0]));
    let ray = Ray::new(DVec3::ZERO, DVec3::X);
    assert_eq!(world.cast_ray(&ray).map(|(_, at)| at.part), Some(0));
}

/// **A body's compound turns with the body in the query world, part by
/// part**: turned a quarter about `+Y`, the right column, `x ∈ [1, 2]`,
/// stands at `z ∈ [-2, -1]`, and nothing is left at its unturned place.
#[test]
fn a_bodys_compound_turns_with_the_body_part_by_part() {
    let entity =
        crcbl_ecs::Entity::from_bits((1u64 << 32) | 5).expect("generation 1 is never zero");
    let transform = Transform::new(
        DVec3::ZERO,
        crate::rotation_from_scaled_axis(DVec3::Y * FRAC_PI_2),
    );
    let mut phys = PhysicsSystem::new();
    phys.set_body(entity, RigidBody::new_kinematic());
    phys.set_transform(entity, transform);
    phys.set_collider(
        entity,
        &ColliderComponent::Compound {
            offset: DVec3::ZERO,
            shape: u_shape(),
            is_trigger: false,
        },
        &transform,
    );
    let id = phys.collider_of(entity).expect("a collider");

    let onto = Ray::new(DVec3::new(0.0, 5.0, -1.5), DVec3::NEG_Y);
    let (hit, at) = phys.world_mut().cast_ray(&onto).expect("the column");
    assert_eq!((hit, at.part), (id, RIGHT));
    assert!((at.t - 4.0).abs() < 1e-9, "{at:?}");

    let unturned = Ray::new(DVec3::new(1.5, 5.0, 0.0), DVec3::NEG_Y);
    assert_eq!(phys.cast_ray(&unturned), None);

    // A step places it again, as the body's other colliders are placed.
    phys.step(1.0 / 60.0);
    assert_eq!(phys.cast_ray(&onto).map(|(hit, _)| hit), Some(entity));

    // And moving the body moves its parts: unturned, 10 along `+X`, the
    // right column stands at `x ∈ [11, 12]` and the gap at `x = 10`.
    phys.set_transform(entity, Transform::from_position(DVec3::new(10.0, 0.0, 0.0)));
    let down = |x: f64| Ray::new(DVec3::new(x, 5.0, 0.0), DVec3::NEG_Y);
    let (hit, at) = phys.world_mut().cast_ray(&down(11.5)).expect("the column");
    assert_eq!((hit, at.part), (id, RIGHT));
    let (hit, at) = phys.world_mut().cast_ray(&down(10.0)).expect("the base");
    assert_eq!((hit, at.part), (id, BASE));
    assert_eq!(phys.cast_ray(&onto), None, "nothing left where it stood");
}

/// **Placing a compound again moves its parts and reuses their buffer**: a
/// ray finds the U where it now stands, and the parts are written into the
/// same allocation the last placement used.
#[test]
fn placing_a_compound_again_reuses_its_parts_buffer() {
    let (mut world, id) = world();
    let parts = |world: &PhysicsWorld| {
        let slot = world.slot_of(id).expect("a live collider");
        match &world.colliders[slot].as_ref().expect("a filled slot").entry {
            super::ColliderEntry::Compound(placed) => placed.parts().as_ptr(),
            other => panic!("not a compound: {other:?}"),
        }
    };
    let before = parts(&world);
    let moved = Transform::from_position(DVec3::new(10.0, 0.0, 0.0));
    assert!(world.set_compound(id, &u_shape(), DVec3::ZERO, &moved));
    assert_eq!(parts(&world), before, "a new buffer");

    let down = |x: f64| Ray::new(DVec3::new(x, 5.0, 0.0), DVec3::NEG_Y);
    assert_eq!(world.cast_ray(&down(1.5)), None, "the U has moved away");
    let (hit, at) = world.cast_ray(&down(11.5)).expect("the right column");
    assert_eq!((hit, at.part), (id, RIGHT));
}

/// **A capsule between two parts is pushed out of the deeper one**, and of
/// two parts equally deep, out of the lower: the compound is one entry, one
/// push-out.
#[test]
fn a_push_out_is_the_deepest_part_and_ties_go_to_the_lower() {
    let column = |x: f64| {
        CompoundPart::new(
            DVec3::new(x, 0.0, 0.0),
            DQuat::IDENTITY,
            DVec3::new(0.5, 1.0, 1.0),
        )
    };
    let shape = CompoundShape::new(vec![column(-1.5), column(1.5)]).expect("a valid shape");
    let mut world = PhysicsWorld::new();
    let id = world.add_compound(&shape, DVec3::ZERO, &Transform::IDENTITY);

    // Midway, 0.1 into each column's face at x = ∓1.
    let mut out = Vec::new();
    world.capsule_penetrations_into(&Capsule::new(DVec3::ZERO, 1.1, 0.0), None, &mut out);
    let [(hit, push)] = out[..] else {
        panic!("one push-out, not {out:?}");
    };
    assert_eq!(hit, id);
    assert!((push.depth - 0.1).abs() < EXACT, "{push:?}");
    assert!(
        close(push.normal, DVec3::X),
        "out of the lower part: {push:?}"
    );

    // Nearer the right column, 0.15 into it and 0.05 into the left.
    world.capsule_penetrations_into(
        &Capsule::new(DVec3::new(0.05, 0.0, 0.0), 1.1, 0.0),
        None,
        &mut out,
    );
    let [(_, push)] = out[..] else {
        panic!("one push-out, not {out:?}");
    };
    assert!((push.depth - 0.15).abs() < EXACT, "{push:?}");
    assert!(
        close(push.normal, DVec3::NEG_X),
        "out of the deeper part: {push:?}"
    );
}

/// **Many parts met at a few fractions still come in part order within each
/// fraction**: each compound's parts are met far, middle, near, far, middle,
/// near, …, and three compounds stand in the same place, so the candidate
/// list is sorted out of the order it was found in, and the sort alone — not
/// the order of finding — keeps each compound's tied parts in order.
#[test]
fn many_tied_parts_are_sorted_into_part_order() {
    let parts: Vec<_> = (0..CompoundShape::MAX_PARTS)
        .map(|i| {
            // Faces at x = 5, 4 and 3 in turn, every part standing on the path.
            let face = 5.0 - (i % 3) as f64;
            CompoundPart::new(
                DVec3::new(face + 0.5, 0.0, 0.0),
                DQuat::IDENTITY,
                DVec3::splat(0.5),
            )
        })
        .collect();
    let shape = CompoundShape::new(parts).expect("a valid shape");
    let mut world = PhysicsWorld::new();
    for _ in 0..3 {
        world.add_compound(&shape, DVec3::ZERO, &Transform::IDENTITY);
    }

    let path = Segment::new(DVec3::ZERO, DVec3::new(10.0, 0.0, 0.0));
    for capsule in [false, true] {
        let mut hits: Vec<(ColliderId, ShapeHit)> = Vec::new();
        if capsule {
            world.sweep_capsule_all(&path, 0.25, 0.1, QueryFilter::ALL, &mut hits);
        } else {
            world.sweep_sphere_all(&path, 0.25, QueryFilter::ALL, &mut hits);
        }
        assert_eq!(
            hits.len(),
            3 * CompoundShape::MAX_PARTS,
            "capsule {capsule}"
        );
        // Every `t` here is positive, so its bits sort as the number does.
        let order: Vec<_> = hits
            .iter()
            .map(|&(id, at)| (at.t.to_bits(), id.index(), at.part))
            .collect();
        let mut sorted = order.clone();
        sorted.sort_unstable();
        assert_eq!(order, sorted, "capsule {capsule}");
    }
}
