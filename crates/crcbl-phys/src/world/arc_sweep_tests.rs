//! [`PhysicsWorld::sweep_capsule_arc`]: contact times on a constant-
//! acceleration path, against the closed forms, the straight sweeps and EW's
//! braking fixture.
//!
//! Every expected time is solved here from the path's own equation, never
//! pasted: `gap = speed t + ½ acceleration t²` along the axis the surface
//! faces, by [`reach_time`].

use glam::{DQuat, DVec3};

use crate::broadphase::Segment;
use crate::character::CharacterConfig;
use crate::collider::{Aabb, BoxCollider, Capsule, Sphere};
use crate::components::Transform;
use crate::mesh::TriangleMesh;
use crate::query::{ARC_TIME_TOLERANCE, AcceleratedPath, ArcHit};

use super::{ColliderId, PhysicsWorld, QueryFilter};

/// How closely a value worked out by hand has to agree with the query's when
/// only rounding separates them: a flat face is met in one exact step.
const EXACT: f64 = 1e-12;

/// How closely two answers for the same contact have to agree, in time and
/// in normal, when either may stop short of it by the search's tolerance:
/// coarse against fine updates, and the arc against a closed form.
const AGREEMENT: f64 = ARC_TIME_TOLERANCE;

/// The capsule every fixture sweeps: [`CharacterConfig`]'s default shape.
const RADIUS: f64 = 0.3;
const HALF_HEIGHT: f64 = 0.6;

/// Standard gravity, which every falling fixture accelerates by.
const GRAVITY: f64 = 9.81;

/// The layer the masked fixtures put their wall on.
const ITEMS: u32 = 1 << 2;

/// Adds a fixture's collider to a world, naming it.
type Build = fn(&mut PhysicsWorld) -> ColliderId;

fn arc(world: &mut PhysicsWorld, path: &AcceleratedPath) -> Option<(ColliderId, ArcHit)> {
    world.sweep_capsule_arc(path, RADIUS, HALF_HEIGHT, QueryFilter::ALL)
}

/// The least `t > 0` at which a point moving at `speed` toward a surface
/// `gap` away, accelerating toward it at `acceleration` (negative when it
/// brakes), covers the gap: the smaller root of
/// `gap = speed t + ½ acceleration t²`.
fn reach_time(gap: f64, speed: f64, acceleration: f64) -> f64 {
    2.0 * gap / (speed + (speed * speed + 2.0 * acceleration * gap).sqrt())
}

/// Asserts `hit` is at the contact `expected`: never later, and short of it
/// by no more than the search's tolerance.
fn assert_on_time(hit: &ArcHit, expected: f64) {
    assert!(
        hit.time <= expected + EXACT && hit.time >= expected - ARC_TIME_TOLERANCE,
        "met at {} against the analytic {expected}",
        hit.time
    );
}

/// A wall filling `x >= near_x`, tall and wide enough that nothing here gets
/// past its ends.
fn wall_from(world: &mut PhysicsWorld, near_x: f64) -> ColliderId {
    world.add_box(BoxCollider::new(
        DVec3::new(near_x + 1.0, 0.0, 0.0),
        DVec3::new(1.0, 10.0, 10.0),
    ))
}

/// A ceiling whose underside is at `y = underside`.
fn ceiling_at(world: &mut PhysicsWorld, underside: f64) -> ColliderId {
    world.add_box(BoxCollider::new(
        DVec3::new(0.0, underside + 1.0, 0.0),
        DVec3::new(10.0, 1.0, 10.0),
    ))
}

/// A square of two triangles centred on `centre`, spanning `across` and
/// `up` either way.
fn square(centre: DVec3, across: DVec3, up: DVec3) -> TriangleMesh {
    TriangleMesh::new(
        &[
            centre - across - up,
            centre + across - up,
            centre + across + up,
            centre - across + up,
        ],
        &[[0, 1, 2], [0, 2, 3]],
    )
    .unwrap()
}

/// **A capsule thrown at a wall while falling and braking meets it when its
/// flank reaches the face**, solved from the path's `x` alone.
#[test]
fn a_falling_braking_capsule_meets_a_wall_at_the_analytic_time() {
    let mut world = PhysicsWorld::new();
    let face = 0.8;
    let wall = wall_from(&mut world, face);
    let (speed, braking) = (2.0, -1.5);
    let path = AcceleratedPath::new(
        DVec3::ZERO,
        DVec3::new(speed, 1.0, 0.25),
        DVec3::new(braking, -GRAVITY, 0.0),
        1.0,
    );

    let (id, hit) = arc(&mut world, &path).expect("the wall is reached");

    assert_eq!(id, wall);
    assert_on_time(&hit, reach_time(face - RADIUS, speed, braking));
    assert_eq!(hit.normal, DVec3::NEG_X);
    assert!((hit.point.x - face).abs() < EXACT, "{hit:?}");
    assert!(!hit.started_inside);
}

/// **A jumping capsule meets a ceiling when its top reaches the underside**,
/// solved from the path's `y` alone; a mesh ceiling is met at the same time.
#[test]
fn a_rising_capsule_meets_a_ceiling_at_the_analytic_time() {
    let underside = 1.3;
    let rise = 3.5;
    let path = AcceleratedPath::new(
        DVec3::ZERO,
        DVec3::new(1.0, rise, -0.5),
        DVec3::new(0.0, -GRAVITY, 0.0),
        0.5,
    );
    let expected = reach_time(underside - HALF_HEIGHT - RADIUS, rise, -GRAVITY);

    let mut world = PhysicsWorld::new();
    let ceiling = ceiling_at(&mut world, underside);
    let (id, hit) = arc(&mut world, &path).expect("the ceiling is reached");
    assert_eq!(id, ceiling);
    assert_on_time(&hit, expected);
    assert_eq!(hit.normal, DVec3::NEG_Y);
    assert!((hit.point.y - underside).abs() < EXACT, "{hit:?}");

    let mut world = PhysicsWorld::new();
    let mesh = world.add_mesh(
        square(DVec3::Y * underside, DVec3::X * 10.0, DVec3::Z * 10.0),
        Transform::IDENTITY,
    );
    let (id, hit) = arc(&mut world, &path).expect("the mesh ceiling is reached");
    assert_eq!(id, mesh);
    assert_on_time(&hit, expected);
    assert!((hit.normal - DVec3::NEG_Y).length() < EXACT, "{hit:?}");
}

/// **A capsule dropped from rest lands when its bottom reaches the floor**,
/// though it starts with no speed toward it: the approach is gravity's alone.
#[test]
fn a_capsule_dropped_from_rest_lands_at_the_analytic_time() {
    let mut world = PhysicsWorld::new();
    let floor = world.add_box(BoxCollider::new(
        DVec3::new(0.0, -0.5, 0.0),
        DVec3::new(10.0, 0.5, 10.0),
    ));
    let drop = 1.2;
    let path = AcceleratedPath::new(
        DVec3::new(0.0, HALF_HEIGHT + RADIUS + drop, 0.0),
        DVec3::new(0.5, 0.0, 0.0),
        DVec3::new(0.0, -GRAVITY, 0.0),
        1.0,
    );
    let (id, hit) = arc(&mut world, &path).expect("the floor is reached");
    assert_eq!(id, floor);
    assert_on_time(&hit, reach_time(drop, 0.0, GRAVITY));
    assert_eq!(hit.normal, DVec3::Y);
    assert!(hit.point.y.abs() < EXACT, "{hit:?}");
}

/// **A path that reaches a surface and turns back before its end is a
/// contact, though the chord between its ends misses the surface.** Braking
/// toward a wall while moving along it, and a jump into a ceiling that lands
/// back at its starting height: both chords run parallel to the surface,
/// clear of it, and the straight sweep along each finds nothing.
#[test]
fn a_path_that_touches_and_leaves_a_surface_is_met_though_its_chord_misses() {
    // Toward the wall at 1 m/s, braking at 4 m/s²: it turns at t = 0.25 a
    // span of 0.125 out, and is back where it started at t = 0.5.
    let (speed, braking, duration) = (1.0, -4.0, 0.5);
    let reach = 0.1;
    let mut world = PhysicsWorld::new();
    let wall = wall_from(&mut world, RADIUS + reach);
    let path = AcceleratedPath::new(
        DVec3::ZERO,
        DVec3::new(speed, 0.0, 1.0),
        DVec3::new(braking, 0.0, 0.0),
        duration,
    );
    let chord = Segment::new(path.start, path.position_at(duration));
    assert!(chord.dir().x.abs() < EXACT, "the chord runs along the wall");
    assert_eq!(
        world.sweep_capsule(&chord, RADIUS, HALF_HEIGHT),
        None,
        "the chord misses the wall"
    );
    let (id, hit) = arc(&mut world, &path).expect("the path reaches the wall");
    assert_eq!(id, wall);
    assert_on_time(&hit, reach_time(reach, speed, braking));
    assert_eq!(hit.normal, DVec3::NEG_X);

    // A jump whose apex is above the ceiling's underside, landing back at its
    // starting height.
    let rise = 3.0;
    let mut world = PhysicsWorld::new();
    let headroom = 0.2;
    let ceiling = ceiling_at(&mut world, HALF_HEIGHT + RADIUS + headroom);
    let path = AcceleratedPath::new(
        DVec3::ZERO,
        DVec3::new(0.7, rise, 0.0),
        DVec3::new(0.0, -GRAVITY, 0.0),
        2.0 * rise / GRAVITY,
    );
    assert!(
        rise * rise / (2.0 * GRAVITY) > headroom,
        "the apex is above"
    );
    let chord = Segment::new(path.start, path.position_at(path.duration));
    assert!(chord.dir().y.abs() < EXACT, "the chord is level");
    assert_eq!(world.sweep_capsule(&chord, RADIUS, HALF_HEIGHT), None);
    let (id, hit) = arc(&mut world, &path).expect("the jump reaches the ceiling");
    assert_eq!(id, ceiling);
    assert_on_time(&hit, reach_time(headroom, rise, -GRAVITY));
}

/// **A path that turns back just short of a surface does not meet it**: the
/// braking path above, its wall moved a hair past the turning point, and the
/// jump's ceiling a hair above the apex.
#[test]
fn a_path_that_turns_back_just_short_of_a_surface_misses_it() {
    let hair = 1e-6;
    let (speed, braking) = (1.0, -4.0);
    let turn = speed * speed / (2.0 * -braking);
    let mut world = PhysicsWorld::new();
    wall_from(&mut world, RADIUS + turn + hair);
    let path = AcceleratedPath::new(
        DVec3::ZERO,
        DVec3::new(speed, 0.0, 1.0),
        DVec3::new(braking, 0.0, 0.0),
        0.5,
    );
    assert_eq!(arc(&mut world, &path), None);

    let rise = 3.0;
    let apex = rise * rise / (2.0 * GRAVITY);
    let mut world = PhysicsWorld::new();
    ceiling_at(&mut world, HALF_HEIGHT + RADIUS + apex + hair);
    let path = AcceleratedPath::new(
        DVec3::ZERO,
        DVec3::new(0.7, rise, 0.0),
        DVec3::new(0.0, -GRAVITY, 0.0),
        2.0 * rise / GRAVITY,
    );
    assert_eq!(arc(&mut world, &path), None);
}

/// **The box the broadphase is asked for holds the path's apex.** A ceiling
/// above the capsule at both ends of a jump, and clear of the box round
/// those ends however far the capsule reaches, is still met at the apex.
#[test]
fn a_ceiling_above_both_ends_of_a_jump_is_met_at_its_apex() {
    let rise = 3.0;
    let apex = rise * rise / (2.0 * GRAVITY);
    let headroom = 0.5 * apex;
    let mut world = PhysicsWorld::new();
    let top = HALF_HEIGHT + RADIUS;
    // A slab whose underside is above the capsule's top at both ends by more
    // than the capsule's reach.
    let ceiling = world.add_box(BoxCollider::new(
        DVec3::new(0.0, top + headroom + 0.05, 0.0),
        DVec3::new(10.0, 0.05, 10.0),
    ));
    let path = AcceleratedPath::new(
        DVec3::ZERO,
        DVec3::new(0.2, rise, 0.0),
        DVec3::new(0.0, -GRAVITY, 0.0),
        2.0 * rise / GRAVITY,
    );
    let reach = DVec3::new(RADIUS, RADIUS + HALF_HEIGHT, RADIUS);
    let end = path.position_at(path.duration);
    let ends = Aabb::new(path.start.min(end) - reach, path.start.max(end) + reach);
    assert!(
        !ends.intersects(&world.aabb_of(ceiling).unwrap()),
        "the ceiling is out of the reach of the box round the ends"
    );
    let (id, hit) = arc(&mut world, &path).expect("the apex reaches the ceiling");
    assert_eq!(id, ceiling);
    assert_on_time(&hit, reach_time(headroom, rise, -GRAVITY));
}

/// **With no acceleration the arc meets what the straight sweep along the
/// same line meets, when it meets it**: a box face, a sphere, a standing
/// capsule and both sides of a mesh wall. An unturned box's edge is the
/// exception, below.
#[test]
fn with_no_acceleration_the_arc_meets_what_the_straight_sweep_meets() {
    let duration = 0.8;
    let start = DVec3::new(-2.0, 0.1, 0.3);
    let cases: [(&str, Build, DVec3); 5] = [
        (
            "a box face",
            |w| {
                w.add_box(BoxCollider::new(
                    DVec3::new(1.0, 0.0, 0.0),
                    DVec3::splat(0.5),
                ))
            },
            DVec3::new(4.0, 0.0, 0.0),
        ),
        (
            "a sphere",
            |w| w.add_sphere(Sphere::new(DVec3::new(0.5, 0.4, 0.6), 0.4)),
            DVec3::new(4.0, 0.2, 0.0),
        ),
        (
            "a standing capsule",
            |w| w.add_capsule(Capsule::new(DVec3::new(0.0, 0.0, 0.0), 0.25, 0.5)),
            DVec3::new(4.0, -0.3, -0.2),
        ),
        (
            "a mesh wall from in front",
            |w| {
                w.add_mesh(
                    square(DVec3::X, DVec3::Z * 3.0, DVec3::Y * 3.0),
                    Transform::IDENTITY,
                )
            },
            DVec3::new(4.0, 0.5, 0.5),
        ),
        (
            "a mesh wall from behind",
            |w| {
                w.add_mesh(
                    square(DVec3::X, DVec3::Y * 3.0, DVec3::Z * 3.0),
                    Transform::IDENTITY,
                )
            },
            DVec3::new(4.0, 0.5, 0.5),
        ),
    ];
    for (name, build, velocity) in cases {
        let mut world = PhysicsWorld::new();
        let target = build(&mut world);
        let path = AcceleratedPath::new(start, velocity, DVec3::ZERO, duration);
        let segment = Segment::new(start, path.position_at(duration));
        let (straight_id, straight) = world
            .sweep_capsule(&segment, RADIUS, HALF_HEIGHT)
            .unwrap_or_else(|| panic!("{name}: the straight sweep meets it"));
        let (id, hit) =
            arc(&mut world, &path).unwrap_or_else(|| panic!("{name}: the arc meets it"));
        assert_eq!((id, straight_id), (target, target), "{name}");
        let at = straight.t * duration;
        assert!(
            hit.time <= at + EXACT && at - hit.time < AGREEMENT,
            "{name}: the arc at {} and the straight sweep at {at}",
            hit.time
        );
        assert!(
            (hit.normal - straight.normal).length() < AGREEMENT,
            "{name}: {hit:?} against {straight:?}"
        );
        assert!(!hit.started_inside && !straight.started_inside, "{name}");
    }
}

#[test]
fn rotated_targets_are_met_at_the_analytic_time() {
    for turned_capsule in [false, true] {
        for acceleration in [0.0, 2.0] {
            let mut world = PhysicsWorld::new();
            let (target, normal, extent) = if turned_capsule {
                let rotation = DQuat::from_rotation_arc(DVec3::Y, DVec3::X);
                let target =
                    world.add_turned_capsule(Capsule::new(DVec3::ZERO, 0.5, 1.0), rotation);
                (target, DVec3::X, 1.5)
            } else {
                let rotation = crate::rotation_from_scaled_axis(DVec3::Y * 0.4);
                let target = world.add_box(
                    BoxCollider::new(DVec3::ZERO, DVec3::new(0.5, 2.0, 2.0))
                        .with_rotation(rotation),
                );
                (target, rotation * DVec3::X, 0.5)
            };
            let start_distance = 3.0;
            let speed = 4.0;
            let path = AcceleratedPath::new(
                normal * start_distance,
                -normal * speed,
                -normal * acceleration,
                1.0,
            );
            let (id, hit) = arc(&mut world, &path).expect("the rotated target");
            let expected = reach_time(start_distance - extent - RADIUS, speed, acceleration);
            assert_eq!(id, target);
            assert_on_time(&hit, expected);
            assert!((hit.normal - normal).length() < AGREEMENT, "{hit:?}");
            assert!(!hit.started_inside);
        }
    }
}

/// **An unturned box's edge is met where the round capsule touches it**,
/// solved from the distance between the capsule's core and the edge. The
/// straight sweep inflates the box by the radius rather than rounding it, so
/// it meets the inflated corner first: earlier, and never later.
#[test]
fn an_unturned_box_edge_is_met_where_the_round_capsule_touches_it() {
    let mut world = PhysicsWorld::new();
    // Its vertical edge nearest the capsule runs through x = 0, z = 0.7.
    let edge = DVec3::new(0.0, 0.0, 0.7);
    let target = world.add_box(BoxCollider::new(
        DVec3::new(0.5, 0.0, 1.2),
        DVec3::splat(0.5),
    ));
    let start = DVec3::new(-2.0, 0.1, 0.3);
    let velocity = DVec3::new(4.0, 0.0, 0.9);
    let duration = 0.8;
    let path = AcceleratedPath::new(start, velocity, DVec3::ZERO, duration);

    // |offset + velocity t| = RADIUS across the edge: the smaller root.
    let offset = start - edge;
    let flat = |v: DVec3| DVec3::new(v.x, 0.0, v.z);
    let (a, b, c) = (
        flat(velocity).length_squared(),
        2.0 * flat(offset).dot(flat(velocity)),
        flat(offset).length_squared() - RADIUS * RADIUS,
    );
    let touch = (-b - (b * b - 4.0 * a * c).sqrt()) / (2.0 * a);
    let centre = path.position_at(touch);
    assert!(
        centre.x < edge.x && centre.z < edge.z,
        "the edge is nearest"
    );

    let (id, hit) = arc(&mut world, &path).expect("the edge is met");
    assert_eq!(id, target);
    assert!(
        hit.time <= touch + EXACT && touch - hit.time < AGREEMENT,
        "met at {} against the analytic {touch}",
        hit.time
    );
    let expected = flat(centre - edge).normalize();
    assert!((hit.normal - expected).length() < AGREEMENT, "{hit:?}");

    let segment = Segment::new(start, path.position_at(duration));
    let (_, straight) = world
        .sweep_capsule(&segment, RADIUS, HALF_HEIGHT)
        .expect("the straight sweep meets it");
    assert!(
        straight.t * duration < hit.time,
        "{straight:?} against {hit:?}"
    );
}

/// **A capsule that begins inside a collider meets it at time zero, flagged,
/// whichever way it heads** — as the straight sweep does — and so a wall
/// further along is not reported.
#[test]
fn a_capsule_that_starts_inside_is_met_at_time_zero() {
    let mut world = PhysicsWorld::new();
    let inside = wall_from(&mut world, RADIUS - 0.01);
    world.add_box(BoxCollider::new(
        DVec3::new(-3.0, 0.0, 0.0),
        DVec3::new(1.0, 10.0, 10.0),
    ));
    let path = AcceleratedPath::new(
        DVec3::ZERO,
        DVec3::new(-1.0, 0.5, 0.0),
        DVec3::new(-2.0, -GRAVITY, 0.0),
        1.0,
    );
    let (id, hit) = arc(&mut world, &path).expect("it starts inside");
    assert_eq!(id, inside);
    assert_eq!(hit.time, 0.0);
    assert!(hit.started_inside);
    assert_eq!(hit.normal, DVec3::NEG_X);
    let segment = Segment::new(path.start, path.position_at(1.0));
    let (straight_id, straight) = world
        .sweep_capsule(&segment, RADIUS, HALF_HEIGHT)
        .expect("it starts inside");
    assert_eq!(straight_id, inside);
    assert!(straight.started_inside && straight.t == 0.0);
}

/// **A character's own registered capsule is left out, and the wall past it
/// is met**; without the exclusion the capsule starts inside itself.
#[test]
fn the_capsule_s_own_collider_is_excluded_and_the_wall_is_met() {
    let mut world = PhysicsWorld::new();
    let face = 1.0;
    let wall = wall_from(&mut world, face);
    let own = world.add_capsule(Capsule::new(DVec3::ZERO, RADIUS, HALF_HEIGHT));
    let speed = 2.0;
    let path = AcceleratedPath::new(
        DVec3::ZERO,
        DVec3::new(speed, 2.0, 0.0),
        DVec3::new(0.0, -GRAVITY, 0.0),
        1.0,
    );

    let (id, hit) = world
        .sweep_capsule_arc(
            &path,
            RADIUS,
            HALF_HEIGHT,
            QueryFilter::excluding(Some(own)),
        )
        .expect("the wall is met");
    assert_eq!(id, wall);
    assert_on_time(&hit, (face - RADIUS) / speed);

    let (id, hit) = arc(&mut world, &path).expect("it starts inside itself");
    assert_eq!(id, own);
    assert!(hit.started_inside);
}

/// **The mask and triggers apply**: a wall on a layer the query does not look
/// at is not met and the one past it is, and a trigger is passed through.
#[test]
fn masked_layers_and_triggers_are_passed_through() {
    let mut world = PhysicsWorld::new();
    let item = wall_from(&mut world, 1.0);
    world.set_layers(item, ITEMS);
    let trigger = world.add_box(BoxCollider::new(
        DVec3::new(2.5, 0.0, 0.0),
        DVec3::new(0.2, 10.0, 10.0),
    ));
    world.set_trigger(trigger, true);
    let far = wall_from(&mut world, 3.5);
    let speed = 4.0;
    let path = AcceleratedPath::new(
        DVec3::ZERO,
        DVec3::new(speed, 0.0, 0.0),
        DVec3::new(0.0, -GRAVITY, 0.0),
        1.0,
    );

    let looking_past = world
        .sweep_capsule_arc(&path, RADIUS, HALF_HEIGHT, QueryFilter::masked(!ITEMS))
        .expect("the far wall is met");
    assert_eq!(looking_past.0, far);
    assert_on_time(&looking_past.1, (3.5 - RADIUS) / speed);
    let only_items = world
        .sweep_capsule_arc(&path, RADIUS, HALF_HEIGHT, QueryFilter::masked(ITEMS))
        .expect("the item is met");
    assert_eq!(only_items.0, item);
    assert_eq!(
        world.sweep_capsule_arc(&path, RADIUS, HALF_HEIGHT, QueryFilter::masked(0)),
        None
    );
}

/// **One query over an interval and successive queries over its pieces agree
/// on the first contact**, each piece starting where and how fast the path is
/// at its start: against a wall met face on and a sphere met on its curve.
#[test]
fn coarse_and_fine_updates_agree_on_the_first_contact() {
    let duration = 0.9;
    let path = AcceleratedPath::new(
        DVec3::new(0.0, 0.2, 0.0),
        DVec3::new(2.5, 2.0, 0.4),
        DVec3::new(-1.0, -GRAVITY, 0.3),
        duration,
    );
    let worlds: [(&str, Build); 2] = [
        ("a wall", |w| wall_from(w, 1.2)),
        ("a sphere", |w| {
            w.add_sphere(Sphere::new(DVec3::new(1.4, 0.4, 0.4), 0.3))
        }),
    ];
    for (name, build) in worlds {
        let mut world = PhysicsWorld::new();
        let target = build(&mut world);
        let (id, coarse) = arc(&mut world, &path).unwrap_or_else(|| panic!("{name}: met"));
        assert_eq!(id, target, "{name}");
        for pieces in [2_u32, 7, 64] {
            let piece = duration / f64::from(pieces);
            let fine = (0..pieces).find_map(|k| {
                let from = piece * f64::from(k);
                let step = AcceleratedPath::new(
                    path.position_at(from),
                    path.velocity_at(from),
                    path.acceleration,
                    piece,
                );
                arc(&mut world, &step).map(|(id, hit)| (id, from + hit.time, hit))
            });
            let (fine_id, time, hit) =
                fine.unwrap_or_else(|| panic!("{name}: {pieces} pieces meet it"));
            assert_eq!(fine_id, target, "{name}");
            assert!(
                (time - coarse.time).abs() < AGREEMENT,
                "{name}: {pieces} pieces meet it at {time}, one at {}",
                coarse.time
            );
            assert!(
                (hit.normal - coarse.normal).length() < AGREEMENT,
                "{name}: {hit:?} against {coarse:?}"
            );
        }
    }
}

/// **The same query gives the same answer, to the bit, every time.**
#[test]
fn the_answer_is_the_same_on_every_run() {
    let build = || {
        let mut world = PhysicsWorld::new();
        world.add_sphere(Sphere::new(DVec3::new(1.4, 0.4, 0.4), 0.3));
        wall_from(&mut world, 1.6);
        world
    };
    let path = AcceleratedPath::new(
        DVec3::new(0.0, 0.2, 0.0),
        DVec3::new(2.5, 2.0, 0.4),
        DVec3::new(-1.0, -GRAVITY, 0.3),
        0.9,
    );
    let mut world = build();
    let first = arc(&mut world, &path).expect("met");
    assert_eq!(arc(&mut world, &path), Some(first));
    assert_eq!(arc(&mut build(), &path), Some(first));
}

/// EW's `braking_near_a_wall_preserves_contact_before_reversal`
/// (`src/controller_wall_braking_tests.rs`, EW `17b62e5f`), ported to the
/// query: a jumping character steers against its own horizontal motion
/// toward a wall, and the braking arc reaches the wall before it turns back,
/// while the straight sweep between the arc's ends misses the wall. Both
/// halves are asserted, for each axis, a straight and a turning brake,
/// attached and unattached colliders, a wall reached and one out of reach,
/// two update lengths and coarse and fine updates.
///
/// The character's numbers are EW's: its air-control acceleration and the
/// most air control may change the takeoff velocity by. Its horizontal speed
/// toward the wall is slow enough for the brake to turn it within both
/// update lengths, as a character a tick after takeoff is.
#[test]
fn braking_near_a_wall_preserves_contact_before_reversal() {
    // EW's `profile::AIR_CONTROL_ACCELERATION_MPS2` and
    // `profile::MAX_AIR_CONTROL_SPEED_CHANGE_MPS`.
    const AIR_CONTROL_ACCELERATION: f64 = 1.0;
    const MAX_AIR_CONTROL_SPEED_CHANGE: f64 = 0.5;
    const SPEED_TOWARD_WALL: f64 = 0.01;
    // A takeoff's vertical speed a tick after the jump.
    const RISE: f64 = 3.3;
    let margin = RADIUS + CharacterConfig::default().skin_width;
    let start = DVec3::Y * (HALF_HEIGHT + margin + 0.05);
    let mut chord_misses = 0;
    let mut reached = 0;
    for axis in [DVec3::X, DVec3::NEG_Z, DVec3::new(0.6, 0.0, 0.8)] {
        for turning in [false, true] {
            for bound in [false, true] {
                for reaches_wall in [false, true] {
                    for elapsed in [0.03_f64, 0.06] {
                        let mut world = PhysicsWorld::new();
                        world.add_box(BoxCollider::new(
                            DVec3::new(0.0, -0.5, 0.0),
                            DVec3::new(200.0, 0.5, 200.0),
                        ));
                        let filter = if bound {
                            let own = world.add_capsule(Capsule::new(start, RADIUS, HALF_HEIGHT));
                            QueryFilter::excluding(Some(own))
                        } else {
                            QueryFilter::ALL
                        };
                        let speed = SPEED_TOWARD_WALL;
                        let tangent = axis.cross(DVec3::Y);
                        let direction = if turning {
                            -axis * 0.8 + tangent * 0.6
                        } else {
                            -axis
                        };
                        let target = direction * MAX_AIR_CONTROL_SPEED_CHANGE;
                        let initial_velocity = axis * speed;
                        let steering =
                            (target - initial_velocity).normalize() * AIR_CONTROL_ACCELERATION;
                        let braking = -steering.dot(axis);
                        let stopping_distance = speed * speed / (2.0 * braking);
                        let distance = stopping_distance * if reaches_wall { 0.75 } else { 1.25 };
                        let centre = start + axis * (distance + margin) - DVec3::Y;
                        let wall = world.add_mesh(
                            TriangleMesh::new(
                                &[
                                    centre - tangent * 4.0,
                                    centre + tangent * 4.0,
                                    centre + tangent * 4.0 + DVec3::Y * 4.0,
                                    centre - tangent * 4.0 + DVec3::Y * 4.0,
                                ],
                                &[[0, 1, 2], [0, 2, 3]],
                            )
                            .unwrap(),
                            Transform::IDENTITY,
                        );
                        let path = AcceleratedPath::new(
                            start,
                            initial_velocity + DVec3::Y * RISE,
                            steering + DVec3::NEG_Y * GRAVITY,
                            elapsed,
                        );
                        let free_motion = path.position_at(elapsed) - start;
                        let case = format!(
                            "axis={axis:?} turning={turning} bound={bound} \
                             reaches_wall={reaches_wall} elapsed={elapsed}"
                        );
                        if reaches_wall && free_motion.dot(axis) < 0.0 {
                            assert!(stopping_distance > distance);
                            assert!(speed / braking < elapsed);
                            assert_eq!(
                                world.sweep_capsule_filtered(
                                    &Segment::new(start, start + free_motion),
                                    margin,
                                    HALF_HEIGHT,
                                    filter,
                                ),
                                None,
                                "the endpoint chord must miss the wall reached by the arc: {case}"
                            );
                            chord_misses += 1;
                        }
                        let impact = 2.0 * distance
                            / (speed + (speed * speed - 2.0 * braking * distance).sqrt());
                        for pieces in [1_u32, (elapsed / 0.001).round() as u32] {
                            let piece = elapsed / f64::from(pieces);
                            let hit = (0..pieces).find_map(|k| {
                                let from = piece * f64::from(k);
                                let step = AcceleratedPath::new(
                                    path.position_at(from),
                                    path.velocity_at(from),
                                    path.acceleration,
                                    piece,
                                );
                                world
                                    .sweep_capsule_arc(&step, margin, HALF_HEIGHT, filter)
                                    .map(|(id, hit)| (id, from + hit.time, hit))
                            });
                            if !reaches_wall {
                                assert_eq!(hit, None, "{case} pieces={pieces}");
                                continue;
                            }
                            assert!(impact < speed / braking && speed / braking < elapsed);
                            let (id, time, hit) =
                                hit.unwrap_or_else(|| panic!("{case} pieces={pieces}"));
                            assert_eq!(id, wall, "{case} pieces={pieces}");
                            assert!(
                                time <= impact + EXACT && impact - time < AGREEMENT,
                                "{case} pieces={pieces}: met at {time}, reached at {impact}"
                            );
                            assert!(
                                (hit.normal + axis).length() < AGREEMENT,
                                "{case} pieces={pieces}: {hit:?}"
                            );
                            assert!(!hit.started_inside, "{case} pieces={pieces}");
                            reached += 1;
                        }
                    }
                }
            }
        }
    }
    assert!(chord_misses > 0);
    assert_eq!(reached, 2 * 24, "every reached case, coarse and fine");
}
