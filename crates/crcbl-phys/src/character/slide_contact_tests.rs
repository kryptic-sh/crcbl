use std::f64::consts::FRAC_PI_6;

use glam::DVec3;

use crate::collider::{BoxCollider, Capsule};
use crate::integrator::rotation_from_scaled_axis;
use crate::world::{ColliderId, PhysicsWorld};

use super::{CharacterConfig, CharacterController, MoveOutcome, SlideContact};

/// How closely a fraction or position worked out by hand has to agree with
/// the sweep's: the sweeps here are closed-form, so only rounding separates
/// them.
const EXACT: f64 = 1e-12;

/// The layer `a_masked_collider_is_neither_moved_against_nor_reported` puts
/// its item on.
const ITEMS: u32 = 1 << 3;

/// A floor whose top is `y = 0`.
fn floor(world: &mut PhysicsWorld) -> ColliderId {
    world.add_box(BoxCollider::new(
        DVec3::new(0.0, -1.0, 0.0),
        DVec3::new(50.0, 1.0, 50.0),
    ))
}

/// A wall filling `x >= near_x`, tall and wide enough that nothing here gets
/// past its ends.
fn wall_from(world: &mut PhysicsWorld, near_x: f64) -> ColliderId {
    world.add_box(BoxCollider::new(
        DVec3::new(near_x + 1.0, 0.0, 0.0),
        DVec3::new(1.0, 5.0, 10.0),
    ))
}

/// The centre a capsule needs so its feet are at `feet`.
fn centre_for_feet(config: &CharacterConfig, feet: f64) -> f64 {
    feet + config.radius + config.half_height
}

/// One move, with its contacts.
fn recorded(
    character: &mut CharacterController,
    world: &mut PhysicsWorld,
    motion: DVec3,
) -> (MoveOutcome, Vec<SlideContact>) {
    let mut contacts = Vec::new();
    let outcome = character.move_and_slide_into(world, motion, &mut contacts);
    assert_eq!(
        contacts.len(),
        outcome.slides as usize,
        "every slide is one contact: {contacts:?}"
    );
    (outcome, contacts)
}

/// The relations [`SlideContact`] documents, for a contact the capsule
/// approached: it stops a skin width short of where the sweep touched.
fn assert_stops_a_skin_short(contact: &SlideContact, config: &CharacterConfig) {
    assert!(!contact.started_inside && !contact.stepped_up);
    assert!((0.0..=1.0).contains(&contact.fraction));
    let distance = contact.requested.length();
    let travel = (contact.fraction * distance - config.skin_width).max(0.0);
    let expected = contact.requested / distance * travel;
    assert!(
        (contact.applied - expected).length() < EXACT,
        "applied {:?}, expected {expected:?} from fraction {}",
        contact.applied,
        contact.fraction,
    );
}

/// **A floor met on the way down is reported, and so is the wall after it.**
/// A falling character lands a fifth of the way along its move and slides
/// along the floor into a wall; `MoveOutcome` alone keeps neither normal.
#[test]
fn a_floor_then_a_wall_are_both_reported_in_order() {
    let config = CharacterConfig::default();
    let mut world = PhysicsWorld::new();
    let floor = floor(&mut world);
    let wall = wall_from(&mut world, 2.0);
    // Feet a tenth up and falling half a metre: the floor is met when the
    // centre has dropped that tenth, a fifth of the way along.
    let mut character =
        CharacterController::new(config, DVec3::new(0.0, centre_for_feet(&config, 0.1), 0.0));
    let motion = DVec3::new(3.0, -0.5, 0.0);

    let (outcome, contacts) = recorded(&mut character, &mut world, motion);

    let [landing, blocked] = contacts[..] else {
        panic!("a floor and a wall, not {contacts:?}");
    };
    assert_eq!(
        (landing.collider, landing.normal),
        (floor, DVec3::Y),
        "the floor first"
    );
    assert_eq!(landing.requested, motion);
    assert!((landing.fraction - 0.2).abs() < EXACT, "{landing:?}");
    assert_stops_a_skin_short(&landing, &config);
    assert_eq!(
        landing.remaining.y, 0.0,
        "the floor took the fall off what is left"
    );
    assert!(landing.remaining.x > 0.0);

    assert_eq!(
        (blocked.collider, blocked.normal),
        (wall, DVec3::NEG_X),
        "then the wall"
    );
    assert_eq!(blocked.requested, landing.remaining, "the slide went on");
    // The capsule's flank meets the wall's face at x = 2.
    let start_x = landing.applied.x;
    let expected = (2.0 - config.radius - start_x) / landing.remaining.x;
    assert!((blocked.fraction - expected).abs() < EXACT, "{blocked:?}");
    assert_stops_a_skin_short(&blocked, &config);
    assert_eq!(blocked.remaining, DVec3::ZERO, "the wall stopped it dead");

    assert!(outcome.hit_wall && outcome.grounded);
    assert!(
        (character.position().x - (2.0 - config.radius - config.skin_width)).abs() < EXACT,
        "a skin short of the wall: x = {}",
        character.position().x
    );
}

/// **A ceiling met on the way up is reported, and so is the wall after it.**
#[test]
fn a_ceiling_then_a_wall_are_both_reported_in_order() {
    let config = CharacterConfig::default();
    let mut world = PhysicsWorld::new();
    // The capsule's top is at 0.9; the ceiling's underside a tenth above it.
    let ceiling = world.add_box(BoxCollider::new(
        DVec3::new(0.0, 2.0, 0.0),
        DVec3::new(50.0, 1.0, 50.0),
    ));
    let wall = wall_from(&mut world, 2.0);
    let mut character = CharacterController::new(config, DVec3::ZERO);
    let motion = DVec3::new(3.0, 0.5, 0.0);

    let (outcome, contacts) = recorded(&mut character, &mut world, motion);

    let [bump, blocked] = contacts[..] else {
        panic!("a ceiling and a wall, not {contacts:?}");
    };
    assert_eq!((bump.collider, bump.normal), (ceiling, DVec3::NEG_Y));
    assert!(character.is_ceiling(bump.normal));
    assert!((bump.fraction - 0.2).abs() < EXACT, "{bump:?}");
    assert_stops_a_skin_short(&bump, &config);
    assert_eq!(bump.remaining.y, 0.0, "the ceiling took the rise");

    assert_eq!((blocked.collider, blocked.normal), (wall, DVec3::NEG_X));
    assert_eq!(blocked.requested, bump.remaining);
    let expected = (2.0 - config.radius - bump.applied.x) / bump.remaining.x;
    assert!((blocked.fraction - expected).abs() < EXACT, "{blocked:?}");
    assert_stops_a_skin_short(&blocked, &config);

    assert!(outcome.hit_ceiling && outcome.hit_wall && !outcome.grounded);
}

/// **A wall the character is touching and moving away from is a contact**,
/// and the approaching wall after it is a second one. The first stops
/// nothing — it starts inside, backs the capsule off by a skin width and
/// clips the drift off the motion — but it costs a sweep, which is exactly
/// what hid the later wall from a caller reading `MoveOutcome`.
#[test]
fn a_wall_moved_away_from_then_one_approached_are_both_reported() {
    let config = CharacterConfig::default();
    let mut world = PhysicsWorld::new();
    let beside = wall_from(&mut world, 1.0);
    // Across the path at z in [2, 4], ending where `beside` begins.
    let ahead = world.add_box(BoxCollider::new(
        DVec3::new(-2.0, 0.0, 3.0),
        DVec3::new(3.0, 5.0, 1.0),
    ));
    // The flank exactly on `beside`'s face: touching, not inside.
    let mut character = CharacterController::new(config, DVec3::new(1.0 - config.radius, 0.0, 0.0));
    let motion = DVec3::new(-0.01, 0.0, 3.0);

    let (outcome, contacts) = recorded(&mut character, &mut world, motion);

    assert_eq!(outcome.depenetration, DVec3::ZERO, "touching is no overlap");
    let [touching, blocked] = contacts[..] else {
        panic!("the wall beside and the wall ahead, not {contacts:?}");
    };
    assert_eq!((touching.collider, touching.normal), (beside, DVec3::NEG_X));
    assert!(touching.started_inside);
    assert_eq!(touching.fraction, 0.0);
    assert_eq!(touching.applied, DVec3::NEG_X * config.skin_width);
    assert_eq!(
        touching.remaining,
        DVec3::new(0.0, 0.0, 3.0),
        "the drift away from the wall was clipped off and the rest kept"
    );

    assert_eq!((blocked.collider, blocked.normal), (ahead, DVec3::NEG_Z));
    assert_eq!(blocked.requested, touching.remaining);
    let expected = (2.0 - config.radius) / 3.0;
    assert!((blocked.fraction - expected).abs() < EXACT, "{blocked:?}");
    assert_stops_a_skin_short(&blocked, &config);
}

/// **A post the move passes clean through at its end is still met.** An
/// overlap test at the end of the move finds nothing; the sweep does.
#[test]
fn a_finite_wall_missed_by_the_endpoint_is_met_by_the_sweep() {
    let config = CharacterConfig::default();
    let mut world = PhysicsWorld::new();
    let post = world.add_box(BoxCollider::new(
        DVec3::new(1.0, 0.0, 0.0),
        DVec3::new(0.05, 0.5, 0.05),
    ));
    let motion = DVec3::new(5.0, 0.0, 0.0);
    let mut overlaps = Vec::new();
    world.capsule_penetrations_into(
        &Capsule::new(motion, config.radius, config.half_height),
        None,
        &mut overlaps,
    );
    assert!(
        overlaps.is_empty(),
        "the end of the move is clear of the post"
    );
    let mut character = CharacterController::new(config, DVec3::ZERO);

    let (_, contacts) = recorded(&mut character, &mut world, motion);

    let [met] = contacts[..] else {
        panic!("the post, not {contacts:?}");
    };
    assert_eq!((met.collider, met.normal), (post, DVec3::NEG_X));
    let expected = (0.95 - config.radius) / 5.0;
    assert!((met.fraction - expected).abs() < EXACT, "{met:?}");
    assert_stops_a_skin_short(&met, &config);
    assert_eq!(met.remaining, DVec3::ZERO);
}

/// **A collider off the query mask is neither moved against nor reported**,
/// and the one behind it is.
#[test]
fn a_masked_collider_is_neither_moved_against_nor_reported() {
    let config = CharacterConfig::default();
    let mut world = PhysicsWorld::new();
    let item = world.add_box(BoxCollider::new(
        DVec3::new(1.25, 0.0, 0.0),
        DVec3::new(0.25, 2.0, 5.0),
    ));
    assert!(world.set_layers(item, ITEMS));
    let wall = wall_from(&mut world, 3.0);
    let motion = DVec3::new(5.0, 0.0, 0.0);

    let mut blind = CharacterController::new(config, DVec3::ZERO).with_query_mask(!ITEMS);
    let (_, contacts) = recorded(&mut blind, &mut world, motion);
    let [met] = contacts[..] else {
        panic!("only the wall, not {contacts:?}");
    };
    assert_eq!(met.collider, wall);
    let expected = (3.0 - config.radius) / 5.0;
    assert!((met.fraction - expected).abs() < EXACT, "{met:?}");

    let mut sees = CharacterController::new(config, DVec3::ZERO);
    let (_, contacts) = recorded(&mut sees, &mut world, motion);
    assert_eq!(
        contacts.first().map(|contact| contact.collider),
        Some(item),
        "with the default mask the item is met first"
    );
}

/// **The character's own collider is never a contact**, and the world's copy
/// of it still follows the move.
#[test]
fn the_self_collider_is_never_a_contact_and_still_follows() {
    let config = CharacterConfig::default();
    let mut world = PhysicsWorld::new();
    let wall = wall_from(&mut world, 1.0);
    let body = world.add_capsule(Capsule::new(DVec3::ZERO, config.radius, config.half_height));
    let mut character = CharacterController::new(config, DVec3::ZERO).with_self_collider(body);

    let (_, contacts) = recorded(&mut character, &mut world, DVec3::new(2.0, 0.0, 0.0));

    let [met] = contacts[..] else {
        panic!("only the wall, not {contacts:?}");
    };
    assert_eq!(met.collider, wall);
    let aabb = world.aabb_of(body).expect("the body is still registered");
    assert!(
        (aabb.centre() - character.position()).length() < EXACT,
        "the world's copy is at {:?} and the character at {:?}",
        aabb.centre(),
        character.position(),
    );
}

/// **A turned wall reports its turned face**, not the world axis nearest it.
#[test]
fn a_turned_wall_reports_its_turned_face_normal() {
    let config = CharacterConfig::default();
    let mut world = PhysicsWorld::new();
    let turn = rotation_from_scaled_axis(DVec3::Y * FRAC_PI_6);
    let slab = world.add_box(
        BoxCollider::new(DVec3::new(2.0, 0.0, 0.0), DVec3::new(0.25, 2.0, 5.0)).with_rotation(turn),
    );
    let mut character = CharacterController::new(config, DVec3::ZERO);

    let (_, contacts) = recorded(&mut character, &mut world, DVec3::new(3.0, 0.0, 0.0));

    let met = contacts.first().expect("the slab is in the way");
    assert_eq!(met.collider, slab);
    let face = turn * DVec3::NEG_X;
    assert!(
        (met.normal - face).length() < 1e-9,
        "met {:?}, the turned face is {face:?}",
        met.normal
    );
    assert!(!met.started_inside && met.fraction > 0.0 && met.fraction < 1.0);
}

/// **A short move keeps its exact contact fraction**, down to moves the
/// box sweep once mistook for standing still. Placed about the origin, with a
/// radius binary fractions hold exactly, so the inflated face is at exactly
/// `x = 0` and only the move's own scale is in the arithmetic.
#[test]
fn a_short_move_keeps_its_exact_contact_fraction() {
    for scale in [1e-3, 1e-6, 1e-8] {
        let config = CharacterConfig {
            radius: 0.25,
            skin_width: scale * 0.1,
            ..CharacterConfig::default()
        };
        let mut world = PhysicsWorld::new();
        let wall = wall_from(&mut world, config.radius);
        let mut character = CharacterController::new(config, DVec3::new(-0.5 * scale, 0.0, 0.0));

        let (_, contacts) = recorded(&mut character, &mut world, DVec3::new(scale, 0.0, 0.0));

        let [met] = contacts[..] else {
            panic!("the wall at scale {scale}, not {contacts:?}");
        };
        assert_eq!((met.collider, met.normal), (wall, DVec3::NEG_X));
        assert!(
            (met.fraction - 0.5).abs() < EXACT,
            "met at {} of the way at scale {scale}",
            met.fraction
        );
        assert_stops_a_skin_short(&met, &config);
    }
}

/// **A step climbed is a contact marked stepped up**, and its displacement
/// carries the climb.
#[test]
fn a_step_climbed_is_a_contact_marked_stepped_up() {
    let config = CharacterConfig::default();
    let mut world = PhysicsWorld::new();
    floor(&mut world);
    let height = 0.2;
    let step = world.add_box(BoxCollider::new(
        DVec3::new(5.0, height * 0.5, 0.0),
        DVec3::new(4.0, height * 0.5, 5.0),
    ));
    let mut character =
        CharacterController::new(config, DVec3::new(0.0, centre_for_feet(&config, 0.0), 0.0));
    character.move_and_slide(&mut world, DVec3::ZERO);
    assert!(character.is_grounded());

    let (outcome, contacts) = recorded(&mut character, &mut world, DVec3::new(1.0, 0.0, 0.0));

    assert!(outcome.stepped_up);
    let climbed = contacts
        .iter()
        .find(|contact| contact.stepped_up)
        .expect("a contact stepped up");
    assert_eq!((climbed.collider, climbed.normal), (step, DVec3::NEG_X));
    assert!(
        (climbed.applied.y - height).abs() < EXACT,
        "the climb is in the displacement: {climbed:?}"
    );
    assert!(climbed.remaining.length() < climbed.requested.length());
}

/// **The last sweep the slide budget allows keeps a remainder that is not
/// applied**, as [`SlideContact::remaining`] warns.
#[test]
fn the_last_sweep_of_the_budget_keeps_an_unapplied_remainder() {
    let config = CharacterConfig {
        max_slides: 1,
        ..CharacterConfig::default()
    };
    let mut world = PhysicsWorld::new();
    floor(&mut world);
    let mut character =
        CharacterController::new(config, DVec3::new(0.0, centre_for_feet(&config, 0.1), 0.0));

    let (outcome, contacts) = recorded(&mut character, &mut world, DVec3::new(3.0, -0.5, 0.0));

    let [landing] = contacts[..] else {
        panic!("the floor alone, not {contacts:?}");
    };
    assert!(landing.remaining.x > 0.0, "{landing:?}");
    assert!(
        (outcome.motion.x - landing.applied.x).abs() < EXACT,
        "only the landing moved it: {:?}",
        outcome.motion
    );
}

/// **Recording contacts does not change the move, to the bit.** Two
/// controllers take the same script — walking, stepping, sliding along and
/// into walls, a turned slab, jumps into a ceiling and moves too small to
/// leave the floor's snap — one recording and one not, and must agree on
/// every position and outcome exactly.
#[test]
fn recording_contacts_moves_exactly_as_not_recording() {
    let config = CharacterConfig::default();
    let mut world = PhysicsWorld::new();
    floor(&mut world);
    // A step a fifth of a metre high over x in [1, 3].
    world.add_box(BoxCollider::new(
        DVec3::new(2.0, 0.1, 0.0),
        DVec3::new(1.0, 0.1, 5.0),
    ));
    wall_from(&mut world, 5.0);
    let slab = world.add_box(
        BoxCollider::new(DVec3::new(4.0, 1.0, -2.0), DVec3::new(0.25, 1.0, 1.0))
            .with_rotation(rotation_from_scaled_axis(DVec3::Y * FRAC_PI_6)),
    );
    // A ceiling 2.1 m up from past the step to the wall: a standing capsule
    // is 1.8 m tall, so it walks under and a jump reaches it.
    world.add_box(BoxCollider::new(
        DVec3::new(4.3, 2.6, 0.0),
        DVec3::new(0.7, 0.5, 5.0),
    ));
    let start = DVec3::new(0.0, centre_for_feet(&config, 0.0), 0.0);
    let mut plain = CharacterController::new(config, start);
    let mut recording = CharacterController::new(config, start);
    let mut contacts = Vec::new();
    let mut seen = Vec::new();

    let gravity = DVec3::new(0.0, -0.05, 0.0);
    for tick in 0..240 {
        let motion = match tick % 40 {
            39 => DVec3::new(0.05, 0.4, 0.02),
            7 => DVec3::new(0.005, config.skin_width * 0.1, 0.0),
            _ => DVec3::new(0.08, 0.0, if tick < 120 { 0.02 } else { -0.04 }) + gravity,
        };
        let expected = plain.move_and_slide(&mut world, motion);
        let outcome = recording.move_and_slide_into(&mut world, motion, &mut contacts);
        assert_eq!(outcome, expected, "tick {tick}");
        assert_eq!(
            recording.position().to_array().map(f64::to_bits),
            plain.position().to_array().map(f64::to_bits),
            "tick {tick}"
        );
        assert_eq!(recording.ground(), plain.ground(), "tick {tick}");
        assert_eq!(contacts.len(), outcome.slides as usize, "tick {tick}");
        seen.extend_from_slice(&contacts);
    }
    assert!(
        seen.iter().any(|contact| contact.stepped_up),
        "the script climbed the step"
    );
    assert!(
        seen.iter()
            .any(|contact| recording.is_ceiling(contact.normal)),
        "the script hit the ceiling"
    );
    assert!(
        seen.iter().any(|contact| contact.normal == DVec3::NEG_X),
        "the script walked into the wall"
    );
    assert!(
        seen.iter().any(|contact| contact.collider == slab),
        "the script slid along the turned slab"
    );
}
