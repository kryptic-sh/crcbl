use std::f64::consts::{FRAC_PI_2, PI, TAU};

use crcbl_ecs::Entity;
use glam::DVec3;

use crate::collider::{Aabb, BoxCollider, Capsule, LyingCapsule, Sphere};
use crate::components::{ColliderComponent, Transform};
use crate::compound_shape::CompoundShape;
use crate::integrator::rotation_from_scaled_axis;
use crate::mesh::TriangleMesh;
use crate::system::PhysicsSystem;
use crate::world::{ColliderId, PhysicsWorld};

use super::lying_move_tests::{BODY, floor, lying_at, off_plane, prone_config, slope};
use super::{CharacterController, LyingTurnOutcome};

/// Where the wall beside a body lying at the origin facing `-Z` begins: its
/// `-X` face. The feet end swinging toward `+X` meets it at a yaw whose sine
/// is `(WALL - radius) / BODY`.
const WALL: f64 = 1.2;

/// Turn `body` toward `yaw`, checking what every turn must: the head is
/// exactly where it was, the shape is the same, and the pose reached fits.
fn turn(
    world: &mut PhysicsWorld,
    character: &CharacterController,
    body: &LyingCapsule,
    yaw: f64,
) -> LyingTurnOutcome {
    let outcome = character.turn_lying(world, body, yaw);
    let turned = outcome.body;
    assert_eq!(turned.head, body.head, "the head moved: {outcome:?}");
    assert_eq!((turned.radius, turned.length), (body.radius, body.length));
    assert!((0.0..=1.0).contains(&outcome.fraction), "{outcome:?}");
    outcome
}

/// A wall whose `-X` face is [`WALL`], standing beside a body lying at the
/// origin along `+Z`.
fn wall_beside(world: &mut PhysicsWorld) -> ColliderId {
    world.add_box(BoxCollider::new(
        DVec3::new(WALL + 5.0, 1.0, 0.0),
        DVec3::new(5.0, 1.0, 10.0),
    ))
}

/// Whether `outcome` is a turn that `blocker` stopped part of the way.
fn stopped_by(outcome: &LyingTurnOutcome, blocker: ColliderId) -> bool {
    outcome.blocker == Some(blocker) && outcome.fraction > 0.0 && outcome.fraction < 1.0
}

// ── Open ground ────────────────────────────────────────────────────────

/// **A turn in open space reaches the yaw asked for, as given**, either way
/// round, and a yaw a whole turn further on is the same turn: the body lies
/// level along the new facing, its feet a body length behind the head.
#[test]
fn a_turn_in_open_space_reaches_the_target_yaw() {
    let mut world = floor();
    let (character, body) = lying_at(&mut world, 0.0, 0.0, 0.0);
    for (yaw, turned) in [(2.0, 2.0), (-2.0, -2.0), (2.0 + TAU, 2.0)] {
        let outcome = turn(&mut world, &character, &body, yaw);
        assert_eq!(outcome.body.yaw, yaw, "{outcome:?}");
        assert!((outcome.turned - turned).abs() < 1e-12, "{outcome:?}");
        assert_eq!(outcome.fraction, 1.0);
        assert_eq!(outcome.blocker, None);
        assert_eq!(outcome.body.pitch_sine, 0.0);
        let feet = body.head - outcome.body.facing() * BODY;
        assert!((outcome.body.feet() - feet).length() < 1e-12);
    }
}

// ── Walls ──────────────────────────────────────────────────────────────

/// **A turn that swings the legs into a wall beside the body stops with the
/// feet touching it**: blocked by that wall, the feet end within half a skin
/// width of it and not in it, and the pose reached fits.
#[test]
fn a_turn_that_swings_the_legs_into_a_wall_stops_with_the_feet_touching_it() {
    let mut world = floor();
    let wall = wall_beside(&mut world);
    let (character, body) = lying_at(&mut world, 0.0, 0.0, 0.0);

    let outcome = turn(&mut world, &character, &body, FRAC_PI_2);
    assert!(stopped_by(&outcome, wall), "{outcome:?}");
    assert_eq!(outcome.body.yaw, body.yaw + outcome.turned);
    assert!((outcome.fraction - outcome.turned / FRAC_PI_2).abs() < 1e-12);
    let gap = WALL - (outcome.body.feet().x + body.radius);
    assert!(
        gap >= 0.0 && gap <= 0.5 * prone_config().skin_width,
        "the feet stopped {gap} short of the wall"
    );
    assert_eq!(character.lying_blocker(&mut world, &outcome.body), None);
}

/// **Turning away from a wall is not stopped by it**: the same wall, the same
/// quarter turn the other way, reaches its yaw.
#[test]
fn turning_away_from_a_wall_reaches_the_target_yaw() {
    let mut world = floor();
    wall_beside(&mut world);
    let (character, body) = lying_at(&mut world, 0.0, 0.0, 0.0);

    let outcome = turn(&mut world, &character, &body, -FRAC_PI_2);
    assert_eq!(outcome.body.yaw, -FRAC_PI_2);
    assert_eq!(outcome.blocker, None);
    assert_eq!(outcome.fraction, 1.0);
}

/// **The turn goes the shorter way round, across the `±π` wrap**: from `3.0`
/// to `-3.0` is about `+0.28`, not `-6.0`, and back again the other way. Walls
/// on both sides stand where the long way would swing the legs; neither is
/// met. A turn of exactly half a circle goes the positive way, into the wall
/// on that side.
#[test]
fn a_turn_goes_the_shorter_way_round_across_the_wrap() {
    let mut world = floor();
    let right = wall_beside(&mut world);
    world.add_box(BoxCollider::new(
        DVec3::new(-WALL - 5.0, 1.0, 0.0),
        DVec3::new(5.0, 1.0, 10.0),
    ));
    let short = TAU - 6.0;
    for (from, to, turned) in [(3.0, -3.0, short), (-3.0, 3.0, -short)] {
        let (character, body) = lying_at(&mut world, 0.0, 0.0, from);
        let outcome = turn(&mut world, &character, &body, to);
        assert_eq!(outcome.blocker, None, "{from} to {to}: {outcome:?}");
        assert_eq!(outcome.body.yaw, to);
        assert!(
            (outcome.turned - turned).abs() < 1e-12,
            "{from} to {to} turned {}",
            outcome.turned
        );
    }

    let (character, body) = lying_at(&mut world, 0.0, 0.0, 0.0);
    for half in [PI, -PI] {
        let outcome = turn(&mut world, &character, &body, half);
        assert!(stopped_by(&outcome, right), "to {half}: {outcome:?}");
        assert!(outcome.turned > 0.0);
    }
}

// ── From a blocked pose ────────────────────────────────────────────────

/// **A body whose legs start in a wall may turn them out of it**, and once
/// out, the next wall stops it as any turn is stopped: the legs start in a
/// wall behind, a half turn swings them out past a wall beside, and a block
/// the far side of that is where the turn ends. Without the block, the same
/// turn goes the whole way.
#[test]
fn a_turn_from_a_blocked_pose_passes_what_it_starts_in_and_stops_at_what_follows() {
    let mut world = floor();
    let (character, body) = lying_at(&mut world, 0.0, 0.0, 0.0);
    let behind = world.add_box(BoxCollider::new(
        DVec3::new(0.0, 1.0, 2.0),
        DVec3::new(2.0, 1.0, 0.25),
    ));
    assert_eq!(character.lying_blocker(&mut world, &body), Some(behind));

    let free = turn(&mut world, &character, &body, PI);
    assert_eq!(free.blocker, None, "{free:?}");
    assert_eq!(free.body.yaw, PI);

    let block = world.add_box(BoxCollider::new(
        DVec3::new(1.5, 1.0, -1.5),
        DVec3::new(0.5, 1.0, 0.5),
    ));
    let outcome = turn(&mut world, &character, &body, PI);
    assert!(stopped_by(&outcome, block), "{outcome:?}");
    assert!(outcome.turned > FRAC_PI_2, "{outcome:?}");
    assert_eq!(character.lying_blocker(&mut world, &outcome.body), None);
}

/// **A turn that never comes clear goes the whole way**: a ball the head is
/// inside at every yaw does not hold the body still.
#[test]
fn a_turn_that_never_comes_clear_goes_the_whole_way() {
    let mut world = floor();
    let (character, body) = lying_at(&mut world, 0.0, 0.0, 0.0);
    let ball = world.add_sphere(Sphere::new(body.head, 0.2));
    assert_eq!(character.lying_blocker(&mut world, &body), Some(ball));

    let outcome = turn(&mut world, &character, &body, 2.0);
    assert_eq!(outcome.blocker, None, "{outcome:?}");
    assert_eq!(outcome.body.yaw, 2.0);
    assert_eq!(outcome.fraction, 1.0);
}

// ── Slopes ─────────────────────────────────────────────────────────────

/// **On a slope the turn follows it**: a body lying straight up a slope that
/// rises 3 in 4 turns across it and then down it without the slope stopping
/// it, its pitch following the slope along each facing — a sine of 0.6 up it,
/// 0 across it and -0.6 down it — and both ends resting as the settle left
/// them, the head where it was.
#[test]
fn a_turn_on_a_slope_follows_it() {
    let config = prone_config();
    let (mut world, normal) = slope(0.75);
    let head = normal * (config.radius + config.skin_width);
    let mut character = CharacterController::new(config, head);
    let level = LyingCapsule::new(head, 0.0, config.radius, BODY);
    let first = character.move_lying(&mut world, &level, DVec3::ZERO);
    let body = character
        .move_lying(&mut world, &first.body, DVec3::ZERO)
        .body;
    assert!((body.pitch_sine - 0.6).abs() < 1e-9, "{body:?}");
    let resting_gap = config.skin_width * 0.8;

    for (yaw, pitch) in [(FRAC_PI_2, 0.0), (PI, -0.6), (-FRAC_PI_2, 0.0)] {
        let outcome = turn(&mut world, &character, &body, yaw);
        assert_eq!(outcome.blocker, None, "to {yaw}: {outcome:?}");
        let turned = outcome.body;
        assert!(
            (turned.pitch_sine - pitch).abs() < 1e-9,
            "to {yaw}: pitch sine {}",
            turned.pitch_sine
        );
        for (end, at) in [("head", turned.head), ("feet", turned.feet())] {
            let gap = off_plane(at, turned.radius, normal);
            assert!(
                (gap - resting_gap).abs() < 1e-9,
                "to {yaw}: the {end} is {gap} off the slope"
            );
        }
    }
}

// ── What the turn sees ─────────────────────────────────────────────────

/// **Every collider kind the query world holds stops the turn**: a sphere, a
/// box, an upright capsule and a triangle mesh, each where a quarter turn
/// swings the feet.
#[test]
fn every_collider_kind_stops_a_turn_into_it() {
    type Place = fn(&mut PhysicsWorld) -> ColliderId;
    let kinds: [(&str, Place); 4] = [
        ("sphere", |world| {
            world.add_sphere(Sphere::new(DVec3::new(BODY, 0.31, 0.0), 0.3))
        }),
        ("box", |world| {
            world.add_box(BoxCollider::new(
                DVec3::new(BODY, 1.0, 0.0),
                DVec3::new(0.3, 1.0, 0.3),
            ))
        }),
        ("capsule", |world| {
            world.add_capsule(Capsule::new(DVec3::new(BODY, 1.0, 0.0), 0.3, 0.7))
        }),
        ("mesh", |world| {
            // A square facing `-X`, toward the body: a quarter turn about
            // `+Z` stands its `+Y` face up that way.
            let square = TriangleMesh::new(
                &[
                    DVec3::new(-2.0, 0.0, -2.0),
                    DVec3::new(-2.0, 0.0, 2.0),
                    DVec3::new(2.0, 0.0, 2.0),
                    DVec3::new(2.0, 0.0, -2.0),
                ],
                &[[0, 1, 2], [0, 2, 3]],
            )
            .unwrap();
            world.add_mesh(
                square,
                Transform::new(
                    DVec3::new(WALL, 1.0, 0.0),
                    rotation_from_scaled_axis(DVec3::Z * FRAC_PI_2),
                ),
            )
        }),
    ];
    for (kind, place) in kinds {
        let mut world = floor();
        let placed = place(&mut world);
        let (character, body) = lying_at(&mut world, 0.0, 0.0, 0.0);
        let outcome = turn(&mut world, &character, &body, FRAC_PI_2);
        assert!(stopped_by(&outcome, placed), "a {kind}: {outcome:?}");
        assert_eq!(character.lying_blocker(&mut world, &outcome.body), None);
    }
}

/// **A compound stops the turn at its bounds**, as the query world holds it:
/// one box around its parts, here reaching the feet's quarter-turn pose only
/// through a part overhead.
#[test]
fn a_compound_stops_a_turn_at_its_bounds() {
    let mut phys = PhysicsSystem::new();
    let floor_entity = Entity::from_bits((1u64 << 32) | 1).expect("generation 1 is never zero");
    let floor_at = Transform::from_position(DVec3::new(0.0, -1.0, 0.0));
    phys.set_transform(floor_entity, floor_at);
    phys.set_collider(
        floor_entity,
        &ColliderComponent::Box {
            offset: DVec3::ZERO,
            half_extents: DVec3::new(50.0, 1.0, 50.0),
            is_trigger: false,
        },
        &floor_at,
    );
    let arch = Entity::from_bits((1u64 << 32) | 2).expect("generation 1 is never zero");
    let arch_at = Transform::from_position(DVec3::new(WALL, 0.0, 0.0));
    phys.set_transform(arch, arch_at);
    phys.set_collider(
        arch,
        &ColliderComponent::Compound {
            offset: DVec3::ZERO,
            shape: CompoundShape::from_aabbs(&[
                Aabb::new(DVec3::new(0.0, 1.5, -2.0), DVec3::new(1.0, 2.0, 2.0)),
                Aabb::new(DVec3::new(1.0, 0.0, -2.0), DVec3::new(1.5, 2.0, 2.0)),
            ])
            .expect("valid parts"),
            is_trigger: false,
        },
        &arch_at,
    );

    let world = phys.world_mut();
    let (character, body) = lying_at(world, 0.0, 0.0, 0.0);
    let outcome = turn(world, &character, &body, FRAC_PI_2);
    assert!(
        outcome.blocker.is_some() && outcome.fraction < 1.0,
        "{outcome:?}"
    );
    let gap = WALL - (outcome.body.feet().x + body.radius);
    assert!(
        gap >= 0.0 && gap <= 0.5 * prone_config().skin_width,
        "the feet stopped {gap} short of the compound's bounds"
    );
}

/// **The character's own collider does not stop its turn**: a sphere where
/// the feet swing stops the turn until it is bound as the controller's own.
#[test]
fn the_characters_own_collider_does_not_stop_its_turn() {
    let mut world = floor();
    let own = world.add_sphere(Sphere::new(DVec3::new(BODY, 0.31, 0.0), 0.3));
    let (character, body) = lying_at(&mut world, 0.0, 0.0, 0.0);

    let unbound = turn(&mut world, &character, &body, FRAC_PI_2);
    assert!(stopped_by(&unbound, own), "{unbound:?}");

    let bound = character.with_self_collider(own);
    let outcome = turn(&mut world, &bound, &body, FRAC_PI_2);
    assert_eq!(outcome.blocker, None, "{outcome:?}");
    assert_eq!(outcome.body.yaw, FRAC_PI_2);
}

/// **The query mask applies to the turn**: an item where the feet swing
/// stops it only while the mask sees it.
#[test]
fn a_collider_off_the_query_mask_does_not_stop_a_turn() {
    const ITEMS: u32 = 1 << 3;
    let mut world = floor();
    let item = world.add_sphere(Sphere::new(DVec3::new(BODY, 0.31, 0.0), 0.3));
    assert!(world.set_layers(item, ITEMS));
    let (character, body) = lying_at(&mut world, 0.0, 0.0, 0.0);

    let seeing = turn(&mut world, &character, &body, FRAC_PI_2);
    assert!(stopped_by(&seeing, item), "{seeing:?}");

    let blind = character.with_query_mask(!ITEMS);
    let outcome = turn(&mut world, &blind, &body, FRAC_PI_2);
    assert_eq!(outcome.blocker, None, "{outcome:?}");
}
