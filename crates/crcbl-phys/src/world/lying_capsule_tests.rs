use std::f64::consts::{FRAC_PI_2, PI};

use glam::DVec3;

use crate::collider::{BoxCollider, Capsule, LyingCapsule, Sphere};
use crate::components::Transform;
use crate::integrator::rotation_from_scaled_axis;
use crate::mesh::TriangleMesh;

use super::{ALL_LAYERS, ColliderId, PhysicsWorld, QueryFilter};

/// The fixtures' capsule radius. A power of two, like every coordinate the
/// resting-height tests place, so "exactly a radius above the floor" is exact
/// in `f64` and not a rounding either side of it.
const RADIUS: f64 = 0.5;
/// How far behind the head the fixtures' feet lie.
const LENGTH: f64 = 1.5;

/// A capsule lying with its underside on `y = 0` and its head over the origin.
/// At a yaw of zero the head faces `-Z`, so the feet are at `z = LENGTH` and
/// the body reaches `z = LENGTH + RADIUS` behind.
fn lying(yaw: f64) -> LyingCapsule {
    LyingCapsule::new(DVec3::new(0.0, RADIUS, 0.0), yaw, RADIUS, LENGTH)
}

fn blocker(world: &mut PhysicsWorld, capsule: &LyingCapsule) -> Option<ColliderId> {
    world.lying_capsule_blocker(capsule, QueryFilter::ALL)
}

/// A square of two triangles `2 · half` across, flat in its own frame's
/// `y = 0` plane and centred on its origin.
fn square(half: f64) -> TriangleMesh {
    TriangleMesh::new(
        &[
            DVec3::new(-half, 0.0, -half),
            DVec3::new(-half, 0.0, half),
            DVec3::new(half, 0.0, half),
            DVec3::new(half, 0.0, -half),
        ],
        &[[0, 1, 2], [0, 2, 3]],
    )
    .unwrap()
}

/// A wall across the feet end of `lying(0.0)`, reaching `z = near`: a square
/// mesh stood up by a quarter turn about `+X`, so the query has to turn the
/// capsule into the mesh's frame to meet it.
fn mesh_wall(world: &mut PhysicsWorld, near: f64) -> ColliderId {
    world.add_mesh(
        square(1.0),
        Transform::new(
            DVec3::new(0.0, 1.0, near),
            rotation_from_scaled_axis(DVec3::X * FRAC_PI_2),
        ),
    )
}

#[test]
fn a_lying_capsule_in_open_space_is_blocked_by_nothing() {
    let mut world = PhysicsWorld::new();
    world.add_box(BoxCollider::new(
        DVec3::new(10.0, 1.0, 10.0),
        DVec3::splat(1.0),
    ));
    for yaw in [0.0, FRAC_PI_2, PI, 3.0 * FRAC_PI_2, 0.3] {
        assert_eq!(blocker(&mut world, &lying(yaw)), None, "yaw {yaw}");
    }
}

/// **The case the prone query exists for.** The head is clear, and a wall a
/// body length behind it is inside the legs — which a sphere at the head, the
/// prone shape before this query, cannot see. Turned so the body points away
/// from the wall, or across it, the same pose fits.
#[test]
fn a_wall_behind_the_head_blocks_the_legs_until_the_body_turns_away() {
    let mut world = PhysicsWorld::new();
    let wall = world.add_box(BoxCollider::new(
        DVec3::new(0.0, 1.0, 1.5),
        DVec3::new(2.0, 1.0, 0.5),
    ));

    assert_eq!(
        world.lying_capsule_blocker(
            &LyingCapsule::new(DVec3::new(0.0, RADIUS, 0.0), 0.0, RADIUS, 0.0),
            QueryFilter::ALL,
        ),
        None,
        "the head alone is clear of the wall, so a sphere there fits"
    );
    assert_eq!(blocker(&mut world, &lying(0.0)), Some(wall));
    assert_eq!(blocker(&mut world, &lying(PI)), None, "feet toward -Z");
    assert_eq!(
        blocker(&mut world, &lying(FRAC_PI_2)),
        None,
        "feet toward +X"
    );
}

/// **Every collider kind the query world holds blocks the feet end**, and
/// each one moved a tenth of a metre further off does not — so the verdict is
/// the shape's and not its bounds'. The capsule's feet end reaches
/// `z = LENGTH + RADIUS = 2`; each obstacle is placed to reach `z = 1.9`, then
/// `z = 2.1`.
///
/// A compound reaches the query world as one box and a plane not at all; the
/// compound is checked through a whole system in the character's tests.
#[test]
fn every_collider_kind_blocks_a_capsule_lying_into_it() {
    type Place = fn(&mut PhysicsWorld, f64) -> ColliderId;
    let kinds: [(&str, Place); 4] = [
        ("sphere", |world, near| {
            world.add_sphere(Sphere::new(DVec3::new(0.0, RADIUS, near + 0.4), 0.4))
        }),
        ("box", |world, near| {
            world.add_box(BoxCollider::new(
                DVec3::new(0.0, 1.0, near + 0.5),
                DVec3::new(1.0, 1.0, 0.5),
            ))
        }),
        ("capsule", |world, near| {
            world.add_capsule(Capsule::new(DVec3::new(0.0, 1.0, near + 0.4), 0.4, 1.0))
        }),
        ("mesh", mesh_wall),
    ];
    for (kind, place) in kinds {
        let mut world = PhysicsWorld::new();
        let obstacle = place(&mut world, 1.9);
        assert_eq!(
            blocker(&mut world, &lying(0.0)),
            Some(obstacle),
            "a {kind} a tenth of a metre inside the feet end"
        );

        let mut world = PhysicsWorld::new();
        place(&mut world, 2.1);
        assert_eq!(
            blocker(&mut world, &lying(0.0)),
            None,
            "a {kind} a tenth of a metre past the feet end"
        );
    }
}

/// **Lying on the floor is not being inside it.** Resting exactly a radius
/// above a box floor or a mesh floor, at any yaw, the capsule touches along its
/// whole length and fits; a millionth of a metre lower it is inside, and does
/// not.
#[test]
fn a_floor_touched_at_resting_height_does_not_block() {
    let mut boxed = PhysicsWorld::new();
    let box_floor = boxed.add_box(BoxCollider::new(
        DVec3::new(0.0, -1.0, 0.0),
        DVec3::new(50.0, 1.0, 50.0),
    ));
    let mut meshed = PhysicsWorld::new();
    let mesh_floor = meshed.add_mesh(square(50.0), Transform::from_position(DVec3::ZERO));

    for (world, floor) in [(&mut boxed, box_floor), (&mut meshed, mesh_floor)] {
        for yaw in [0.0, FRAC_PI_2, PI, 3.0 * FRAC_PI_2, 0.3] {
            assert_eq!(blocker(world, &lying(yaw)), None, "resting, yaw {yaw}");
            let sunk = LyingCapsule {
                head: DVec3::new(0.0, RADIUS - 1e-6, 0.0),
                ..lying(yaw)
            };
            assert_eq!(blocker(world, &sunk), Some(floor), "sunk, yaw {yaw}");
        }
    }
}

/// **The filter decides what can block**, as it does for the penetration
/// query: the excluded collider, one off the mask, and a trigger all sit in
/// the legs and none of them blocks — and the wall behind them still does.
#[test]
fn an_excluded_masked_out_or_trigger_collider_does_not_block() {
    const LEVEL: u32 = 1 << 0;
    const ITEMS: u32 = 1 << 1;
    let mut world = PhysicsWorld::new();
    let own = world.add_sphere(Sphere::new(DVec3::new(0.0, RADIUS, 0.75), RADIUS));
    let item = world.add_sphere(Sphere::new(DVec3::new(0.0, RADIUS, 1.0), 0.25));
    assert!(world.set_layers(item, ITEMS));
    let trigger = world.add_sphere(Sphere::new(DVec3::new(0.0, RADIUS, 1.25), 0.25));
    assert!(world.set_trigger(trigger, true));
    for id in [own, trigger] {
        assert!(world.set_layers(id, LEVEL));
    }

    let filter = QueryFilter::excluding(Some(own)).with_mask(LEVEL);
    assert_eq!(world.lying_capsule_blocker(&lying(0.0), filter), None);
    assert_eq!(
        world.lying_capsule_blocker(&lying(0.0), filter.with_mask(ALL_LAYERS)),
        Some(item),
        "the item blocks once the mask includes it"
    );

    let wall = mesh_wall(&mut world, 1.9);
    assert!(world.set_layers(wall, LEVEL));
    assert_eq!(world.lying_capsule_blocker(&lying(0.0), filter), Some(wall));
}
