use std::f64::consts::{FRAC_PI_2, PI};

use crcbl_ecs::Entity;
use glam::DVec3;

use crate::collider::{Aabb, BoxCollider, Capsule, LyingCapsule, Sphere};
use crate::components::{ColliderComponent, Transform};
use crate::compound_shape::CompoundShape;
use crate::integrator::rotation_from_scaled_axis;
use crate::mesh::TriangleMesh;
use crate::system::PhysicsSystem;
use crate::world::{ColliderId, PhysicsWorld};

use super::{CharacterConfig, CharacterController, LyingMoveOutcome};

/// How far behind the actor's origin a prone body's feet lie.
pub(super) const BODY: f64 = 1.6;

/// Where the obstacles behind a body lying at the origin begin: the body's
/// back end reaches `BODY + radius = 1.9`, so this leaves 0.6 of room.
const NEAR: f64 = 2.5;

/// How much nearer than a skin width a sweep may leave a body: the
/// conservative advancement the parametric shapes are swept by stops within a
/// quarter of a linear slop of the contact, and this is a whole one.
const SWEEP_TOLERANCE: f64 = 5e-3;

/// A prone character as a game models one today: a controller with no
/// cylindrical section, a sphere at the actor's origin.
pub(super) fn prone_config() -> CharacterConfig {
    CharacterConfig {
        half_height: 0.0,
        ..CharacterConfig::default()
    }
}

/// The pitch sine the default slope limit clamps to.
fn limit() -> f64 {
    let cosine = prone_config().min_ground_normal_y;
    (1.0 - cosine * cosine).sqrt()
}

/// A floor whose top is `y = 0`, wide enough that nothing here reaches its
/// edge.
pub(super) fn floor() -> PhysicsWorld {
    let mut world = PhysicsWorld::new();
    world.add_box(BoxCollider::new(
        DVec3::new(0.0, -1.0, 0.0),
        DVec3::new(50.0, 1.0, 50.0),
    ));
    world
}

/// A prone character settled on `world`'s floor at `(x, 0, z)`, and its body
/// lying back from it facing `yaw`, settled by a move of nothing.
pub(super) fn lying_at(
    world: &mut PhysicsWorld,
    x: f64,
    z: f64,
    yaw: f64,
) -> (CharacterController, LyingCapsule) {
    let config = prone_config();
    let mut character = CharacterController::new(config, DVec3::new(x, config.radius, z));
    character.move_and_slide(world, DVec3::ZERO);
    assert!(character.is_grounded(), "the fixture starts on the floor");
    let body = LyingCapsule::new(character.position(), yaw, config.radius, BODY);
    let settled = character.move_lying(world, &body, DVec3::ZERO);
    assert!(settled.grounded, "the body starts on the floor");
    (character, settled.body)
}

/// Crawl `ticks` times by `step`, checking every pose on the way fits.
fn crawl(
    world: &mut PhysicsWorld,
    character: &mut CharacterController,
    mut body: LyingCapsule,
    step: DVec3,
    ticks: usize,
) -> Vec<LyingMoveOutcome> {
    let mut outcomes = Vec::with_capacity(ticks);
    for tick in 0..ticks {
        let outcome = character.move_lying(world, &body, step);
        assert_eq!(
            character.lying_blocker(world, &outcome.body),
            None,
            "tick {tick} left the body inside something: {:?}",
            outcome.body
        );
        body = outcome.body;
        outcomes.push(outcome);
    }
    outcomes
}

/// A plane through the origin rising `rise` for each unit run toward `-Z`,
/// as a mesh wide enough that nothing here reaches its edge. Its upward
/// normal is `(0, 1, rise)` normalised.
pub(super) fn slope(rise: f64) -> (PhysicsWorld, DVec3) {
    const HALF: f64 = 50.0;
    let mesh = TriangleMesh::new(
        &[
            DVec3::new(-HALF, rise * HALF, -HALF),
            DVec3::new(-HALF, -rise * HALF, HALF),
            DVec3::new(HALF, -rise * HALF, HALF),
            DVec3::new(HALF, rise * HALF, -HALF),
        ],
        &[[0, 1, 2], [0, 2, 3]],
    )
    .unwrap();
    let mut world = PhysicsWorld::new();
    world.add_mesh(mesh, Transform::IDENTITY);
    (world, DVec3::new(0.0, 1.0, rise).normalize())
}

/// How far a sphere of `radius` at `centre` is off the plane through the
/// origin with `normal`: negative inside it.
pub(super) fn off_plane(centre: DVec3, radius: f64, normal: DVec3) -> f64 {
    centre.dot(normal) - radius
}

// ── Open ground ────────────────────────────────────────────────────────

/// **Crawling in open space moves the whole body with the head**: every tick
/// covers exactly what was asked, the body stays level on the floor, and the
/// feet stay a body length behind.
#[test]
fn crawling_forward_in_open_space_moves_the_whole_body() {
    let mut world = floor();
    let (mut character, body) = lying_at(&mut world, 0.0, 0.0, 0.0);
    let resting = prone_config().radius + prone_config().skin_width;

    let step = DVec3::new(0.0, 0.0, -0.1);
    for outcome in crawl(&mut world, &mut character, body, step, 10) {
        assert!(
            (outcome.motion - step).length() < 1e-12,
            "asked for {step:?} and got {:?}",
            outcome.motion
        );
        assert_eq!(outcome.slides, 0, "nothing was in the way");
        assert!(outcome.grounded);
        assert_eq!(outcome.body.pitch_sine, 0.0);
        assert!((outcome.body.head.y - resting).abs() < 1e-12, "{outcome:?}");
        assert!((outcome.body.feet() - (outcome.body.head + DVec3::Z * BODY)).length() < 1e-12);
    }
    assert!((character.position().z + 1.0).abs() < 1e-12);
}

// ── Walls ──────────────────────────────────────────────────────────────

/// **Crawling backward toward a wall stops the feet at it**, where the sphere
/// at the head — the prone shape before this — would have gone the whole way:
/// the legs no longer pass through.
#[test]
fn crawling_backward_into_a_wall_stops_the_feet_where_the_head_alone_would_not() {
    let mut world = floor();
    world.add_box(BoxCollider::new(
        DVec3::new(0.0, 1.0, NEAR + 0.5),
        DVec3::new(5.0, 1.0, 0.5),
    ));
    let (mut character, body) = lying_at(&mut world, 0.0, 0.0, 0.0);
    let mut sphere_only = character.clone();

    let back = DVec3::new(0.0, 0.0, 1.0);
    let outcome = character.move_lying(&mut world, &body, back);
    let heels = outcome.body.feet().z + body.radius;
    assert!(outcome.hit_wall);
    assert!(
        heels <= NEAR && heels >= NEAR - prone_config().skin_width - SWEEP_TOLERANCE,
        "the heels stopped at {heels}, and the wall is at {NEAR}"
    );

    let alone = sphere_only.move_and_slide(&mut world, back);
    assert!(
        (alone.motion - back).length() < 1e-12,
        "the head's sphere alone went {:?}",
        alone.motion
    );
    assert!(outcome.motion.z < alone.motion.z - 0.3);
}

/// **Crawling into a wall at an angle slides along it**: the part of the move
/// along the wall survives whole, the part into it does not, and the body
/// lying beside the wall along its whole length stays out of it.
#[test]
fn crawling_into_a_wall_at_an_angle_slides_along_it() {
    let mut world = floor();
    // Its -X face is x = 0.35, 0.05 beside the body's flank.
    world.add_box(BoxCollider::new(
        DVec3::new(5.35, 1.0, 0.0),
        DVec3::new(5.0, 1.0, 10.0),
    ));
    let (mut character, body) = lying_at(&mut world, 0.0, 0.0, 0.0);

    let asked = DVec3::new(0.1, 0.0, -0.1);
    let outcome = character.move_lying(&mut world, &body, asked);
    assert!(outcome.hit_wall, "the wall is 0.05 away");
    assert!(
        outcome.motion.x < 0.05 && outcome.motion.x > 0.05 - 2.0 * SWEEP_TOLERANCE,
        "x moved {}",
        outcome.motion.x
    );
    assert!(
        (outcome.motion.z - asked.z).abs() < 1e-12,
        "the move along the wall is {} and was asked to be {}",
        outcome.motion.z,
        asked.z
    );
    assert_eq!(character.lying_blocker(&mut world, &outcome.body), None);
}

// ── Slopes ─────────────────────────────────────────────────────────────

/// **On a slope within the walkable limit the body pitches to follow it and
/// both ends rest on it**: a skin width of vertical travel above it, never in
/// it, crawling up it and lying across it.
///
/// The slope rises 3 in 4, so its normal is `(0, 0.8, 0.6)` and a body lying
/// straight up it has a pitch sine of 0.6. It starts lying level, its head on
/// the slope and its feet in the air, and the first settle turns it down onto
/// it.
#[test]
fn on_a_walkable_slope_the_body_pitches_to_follow_it_and_both_ends_rest() {
    let config = prone_config();
    let resting_gap = config.skin_width * 0.8;
    for (yaw, pitch) in [(0.0, 0.6), (FRAC_PI_2, 0.0)] {
        let (mut world, normal) = slope(0.75);
        let head = normal * (config.radius + config.skin_width);
        let mut character = CharacterController::new(config, head);
        let level = LyingCapsule::new(head, yaw, config.radius, BODY);

        let first = character.move_lying(&mut world, &level, DVec3::ZERO);
        let settled = character.move_lying(&mut world, &first.body, DVec3::ZERO);
        let crawled = crawl(
            &mut world,
            &mut character,
            settled.body,
            DVec3::new(0.0, 0.0, -0.1),
            10,
        );
        for outcome in std::iter::once(&settled).chain(&crawled) {
            let body = outcome.body;
            assert!(outcome.grounded, "yaw {yaw}");
            assert!(
                (body.pitch_sine - pitch).abs() < 1e-9,
                "yaw {yaw}: pitch sine {} on a slope of {pitch}",
                body.pitch_sine
            );
            for (end, at) in [("head", body.head), ("feet", body.feet())] {
                let gap = off_plane(at, body.radius, normal);
                assert!(
                    (gap - resting_gap).abs() < 1e-9,
                    "yaw {yaw}: the {end} is {gap} off the slope"
                );
            }
        }
        assert!(
            crawled.last().unwrap().body.head.y > settled.body.head.y + 0.5,
            "yaw {yaw}: crawling up the slope climbs it"
        );
    }
}

/// **On a slope steeper than walkable the pitch clamps** to the limit, and
/// the uphill end rests while the downhill end is held off the slope — facing
/// up it and facing down it. Nothing is walkable, so nothing is grounded.
///
/// The slope rises 4 in 3, normal `(0, 0.6, 0.8)`: a pitch sine of 0.8 against
/// a limit of `sqrt(1/2)`.
#[test]
fn on_a_slope_steeper_than_walkable_the_pitch_clamps() {
    let config = prone_config();
    let (mut world, normal) = slope(4.0 / 3.0);
    let resting = normal * (config.radius + config.skin_width);

    // Facing up it, the head rests and the feet hang below the limit's line.
    let mut character = CharacterController::new(config, resting);
    let up = LyingCapsule::new(resting, 0.0, config.radius, BODY);
    let outcome = character.move_lying(&mut world, &up, DVec3::ZERO);
    let body = outcome.body;
    assert!((body.pitch_sine - limit()).abs() < 1e-12, "{body:?}");
    assert!((off_plane(body.head, body.radius, normal) - config.skin_width * 0.6).abs() < 1e-9);
    assert!(off_plane(body.feet(), body.radius, normal) > config.skin_width);
    assert!(!outcome.grounded);

    // Facing down it, the feet rest and the head hangs.
    let head = resting + DVec3::Z * BODY;
    let mut character = CharacterController::new(config, head);
    let down = LyingCapsule::new(head, PI, config.radius, BODY);
    let outcome = character.move_lying(&mut world, &down, DVec3::ZERO);
    let body = outcome.body;
    assert!((body.pitch_sine + limit()).abs() < 1e-12, "{body:?}");
    assert!((off_plane(body.feet(), body.radius, normal) - config.skin_width * 0.6).abs() < 1e-9);
    assert!(off_plane(body.head, body.radius, normal) > config.skin_width);
    assert!(!outcome.grounded);
}

// ── Edges ──────────────────────────────────────────────────────────────

/// A floor whose top is `y = 0` behind `z = 0`, and one whose top is
/// `y = step` ahead of it: a curb up for a positive step, down for a negative
/// one, which a body facing `-Z` meets head first.
fn curb(step: f64) -> PhysicsWorld {
    let mut world = PhysicsWorld::new();
    world.add_box(BoxCollider::new(
        DVec3::new(0.0, -1.0, 10.0),
        DVec3::new(5.0, 1.0, 10.0),
    ));
    world.add_box(BoxCollider::new(
        DVec3::new(0.0, step - 1.0, -10.0),
        DVec3::new(5.0, 1.0, 10.0),
    ));
    world
}

/// **Crawling head first off a curb, the body rides level until tipping is
/// the smaller turn, then tips onto the lower floor**: the edge is never cut
/// through, the head never rises, the head is never above the feet, and the
/// body ends lying level on the lower floor.
///
/// A plank pushed over a curb does the same. The two poses the test pins are
/// the two sides of the tip: the head out past the edge by more than a radius
/// with the body still near level on the upper floor, and later the head on
/// the lower floor with the feet still up on the upper one.
#[test]
fn crawling_head_first_off_a_curb_rides_level_then_tips_onto_the_lower_floor() {
    const DROP: f64 = 0.3;
    let mut world = curb(-DROP);
    let (mut character, body) = lying_at(&mut world, 0.0, 0.5, 0.0);
    let config = prone_config();
    let (upper, lower) = (
        config.radius + config.skin_width,
        config.radius + config.skin_width - DROP,
    );

    let outcomes = crawl(
        &mut world,
        &mut character,
        body,
        DVec3::new(0.0, 0.0, -0.05),
        70,
    );
    let mut height = body.head.y;
    for outcome in &outcomes {
        let body = outcome.body;
        assert!(outcome.grounded, "{body:?}");
        assert!(body.head.y <= height + 1e-12, "the head rose: {body:?}");
        assert!(
            body.pitch_sine <= 1e-12,
            "the head rose above the feet: {body:?}"
        );
        height = body.head.y;
    }
    assert!(
        outcomes.iter().any(|o| o.body.head.z < -config.radius
            && o.body.head.y > upper - 0.05
            && o.body.pitch_sine > -0.05),
        "the body rides level with its head out over the drop"
    );
    assert!(
        outcomes
            .iter()
            .any(|o| (o.body.head.y - lower).abs() < 1e-9 && o.body.feet().y > upper),
        "the body tips onto its head with its feet still up on the upper floor"
    );
    let last = outcomes.last().unwrap().body;
    assert_eq!(last.pitch_sine, 0.0, "{last:?}");
    assert!((last.head.y - lower).abs() < 1e-12 && (last.feet().y - lower).abs() < 1e-12);
}

/// **A lying body does not step up.** A riser low enough that its round head
/// meets the edge on a walkable slope is slid up and over; one a little over
/// that is a wall it stops at, flat against it, and does not creep up however
/// long it pushes.
///
/// The cut-off is where the edge's normal on the head leans `acos 0.7071` from
/// vertical: an edge `radius · (1 - 0.7071) + skin_width ≈ 0.098` below the
/// head's centre.
#[test]
fn a_low_riser_is_crawled_over_and_a_taller_one_is_a_wall() {
    let config = prone_config();
    let step = DVec3::new(0.0, 0.0, -0.05);

    let mut world = curb(0.08);
    let (mut character, body) = lying_at(&mut world, 0.0, 0.5, 0.0);
    let last = crawl(&mut world, &mut character, body, step, 70)
        .pop()
        .unwrap();
    assert!(
        last.body.feet().z < 0.0,
        "the whole body is up: {:?}",
        last.body
    );
    assert!((last.body.head.y - (0.08 + config.radius + config.skin_width)).abs() < 1e-9);

    let mut world = curb(0.12);
    let (mut character, body) = lying_at(&mut world, 0.0, 0.5, 0.0);
    let outcomes = crawl(&mut world, &mut character, body, step, 70);
    let last = outcomes.last().unwrap();
    assert!(last.hit_wall);
    assert_eq!(last.body.head.y, body.head.y, "it crept up the riser");
    let edge = DVec3::new(0.0, 0.12, 0.0);
    let gap = (last.body.head - edge).length() - config.radius;
    assert!(
        gap > 0.0 && gap <= config.skin_width + SWEEP_TOLERANCE,
        "the head stopped {gap} short of the riser's edge"
    );
}

// ── What the move sees ─────────────────────────────────────────────────

/// Crawl the body lying at the origin backward toward `+Z` by a metre, where
/// whatever `world` has behind it begins at [`NEAR`].
fn crawl_back(
    world: &mut PhysicsWorld,
    character: &mut CharacterController,
    body: &LyingCapsule,
) -> LyingMoveOutcome {
    character.move_lying(world, body, DVec3::new(0.0, 0.0, 1.0))
}

/// Whether `outcome`'s heels were stopped a skin width short of [`NEAR`] by
/// a wall, the body still lying level on the floor.
fn heels_at_near(outcome: &LyingMoveOutcome) -> bool {
    let heels = outcome.body.feet().z + outcome.body.radius;
    outcome.hit_wall
        && outcome.body.pitch_sine == 0.0
        && heels <= NEAR
        && heels >= NEAR - prone_config().skin_width - SWEEP_TOLERANCE
}

/// **Every collider kind the query world holds stops the feet**: a sphere, a
/// box, an upright capsule and a triangle mesh, each reaching [`NEAR`] on the
/// body's axis behind it.
#[test]
fn every_collider_kind_stops_a_body_crawling_back_into_it() {
    type Place = fn(&mut PhysicsWorld, f64) -> ColliderId;
    let core = prone_config().radius + prone_config().skin_width;
    let kinds: [(&str, Place); 4] = [
        ("sphere", |world, core| {
            world.add_sphere(Sphere::new(DVec3::new(0.0, core, NEAR + 0.4), 0.4))
        }),
        ("box", |world, _| {
            world.add_box(BoxCollider::new(
                DVec3::new(0.0, 1.0, NEAR + 0.5),
                DVec3::new(1.0, 1.0, 0.5),
            ))
        }),
        ("capsule", |world, _| {
            world.add_capsule(Capsule::new(DVec3::new(0.0, 1.0, NEAR + 0.4), 0.4, 1.0))
        }),
        ("mesh", |world, _| {
            // A square facing `-Z`, toward the body: a quarter turn about
            // `-X` stands its `+Y` face up that way.
            let square = TriangleMesh::new(
                &[
                    DVec3::new(-1.0, 0.0, -1.0),
                    DVec3::new(-1.0, 0.0, 1.0),
                    DVec3::new(1.0, 0.0, 1.0),
                    DVec3::new(1.0, 0.0, -1.0),
                ],
                &[[0, 1, 2], [0, 2, 3]],
            )
            .unwrap();
            world.add_mesh(
                square,
                Transform::new(
                    DVec3::new(0.0, 1.0, NEAR),
                    rotation_from_scaled_axis(DVec3::X * -FRAC_PI_2),
                ),
            )
        }),
    ];
    for (kind, place) in kinds {
        let mut world = floor();
        place(&mut world, core);
        let (mut character, body) = lying_at(&mut world, 0.0, 0.0, 0.0);
        let outcome = crawl_back(&mut world, &mut character, &body);
        assert!(
            heels_at_near(&outcome),
            "a {kind}: the heels stopped at {}",
            outcome.body.feet().z + body.radius
        );
        assert_eq!(character.lying_blocker(&mut world, &outcome.body), None);
    }
}

/// **A compound stops the feet at its bounds**, as the query world holds it:
/// one box around its parts. Here the part level with the body lies a metre
/// further back, and only a part overhead reaches [`NEAR`]; the feet stop at
/// [`NEAR`] all the same.
#[test]
fn a_compound_stops_a_body_crawling_back_at_its_bounds() {
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
    let arch_at = Transform::from_position(DVec3::new(0.0, 0.0, NEAR));
    phys.set_transform(arch, arch_at);
    phys.set_collider(
        arch,
        &ColliderComponent::Compound {
            offset: DVec3::ZERO,
            shape: CompoundShape::from_aabbs(&[
                Aabb::new(DVec3::new(-1.0, 1.5, 0.0), DVec3::new(1.0, 2.0, 1.0)),
                Aabb::new(DVec3::new(-1.0, 0.0, 1.0), DVec3::new(1.0, 2.0, 1.5)),
            ])
            .expect("valid parts"),
            is_trigger: false,
        },
        &arch_at,
    );

    let world = phys.world_mut();
    let (mut character, body) = lying_at(world, 0.0, 0.0, 0.0);
    let outcome = crawl_back(world, &mut character, &body);
    assert!(
        heels_at_near(&outcome),
        "the heels stopped at {}, and the compound's bounds begin at {NEAR}",
        outcome.body.feet().z + body.radius
    );
}

/// **The character's own collider is left out of the move and follows the
/// head**: bound, the body crawls through the sphere registered at its own
/// head as though it were not there, and the sphere is moved to where the
/// head ends. Unbound, the same sphere is inside the body and stops it.
#[test]
fn the_characters_own_collider_does_not_stop_its_lying_body_and_follows_its_head() {
    let mut world = floor();
    let (character, body) = lying_at(&mut world, 0.0, 0.0, 0.0);
    let own = world.add_sphere(Sphere::new(body.head, body.radius));
    let forward = DVec3::new(0.0, 0.0, -1.0);

    let mut unbound = character.clone();
    let stuck = unbound.move_lying(&mut world, &body, forward);
    assert!(stuck.slides > 0 && stuck.motion.z > forward.z, "{stuck:?}");

    let mut bound = character.with_self_collider(own);
    let moved = bound.move_lying(&mut world, &body, forward);
    assert!((moved.motion - forward).length() < 1e-12, "{moved:?}");
    let sphere = world.aabb_of(own).expect("still registered");
    assert!((sphere.centre() - moved.body.head).length() < 1e-12);
}

/// **The query mask applies to the move**: an item behind the feet stops
/// them only while the mask sees it.
#[test]
fn a_collider_off_the_query_mask_does_not_stop_a_lying_body() {
    const ITEMS: u32 = 1 << 3;
    let mut world = floor();
    let core = prone_config().radius + prone_config().skin_width;
    let item = world.add_sphere(Sphere::new(DVec3::new(0.0, core, NEAR + 0.2), 0.2));
    assert!(world.set_layers(item, ITEMS));
    let (character, body) = lying_at(&mut world, 0.0, 0.0, 0.0);

    let mut seeing = character.clone();
    assert!(heels_at_near(&crawl_back(&mut world, &mut seeing, &body)));

    let mut blind = character.with_query_mask(!ITEMS);
    let outcome = crawl_back(&mut world, &mut blind, &body);
    assert_eq!(outcome.slides, 0);
    assert!((outcome.motion.z - 1.0).abs() < 1e-12, "{outcome:?}");
}
