use glam::DVec3;

use crate::collider::{BoxCollider, LyingCapsule};
use crate::world::{ColliderId, PhysicsWorld};

use super::lying_move_tests::{floor, lying_at, prone_config};
use super::{CharacterController, LyingMoveOutcome, SlideContact};

/// Where the wall behind a body lying at the origin facing `-Z` begins, with
/// room between it and the body's back end for most of a one-metre move.
const BEHIND: f64 = 2.5;

/// How far a lying sweep's fraction may be from the one worked out by hand:
/// the conservative advancement the lying capsule is swept by stops within a
/// quarter of a linear slop of the contact, and this is a whole one over the
/// one-metre moves here.
const SWEEP_TOLERANCE: f64 = 5e-3;

/// How closely relations the slide computes exactly have to hold: only
/// rounding separates them.
const EXACT: f64 = 1e-12;

/// A wall across the floor filling `z >= near_z`, wide and tall enough that
/// nothing here gets past its ends.
fn wall_behind(world: &mut PhysicsWorld, near_z: f64) -> ColliderId {
    world.add_box(BoxCollider::new(
        DVec3::new(0.0, 1.0, near_z + 0.5),
        DVec3::new(5.0, 1.0, 0.5),
    ))
}

/// One lying move, with its contacts, checking there is one per slide.
fn recorded(
    character: &mut CharacterController,
    world: &mut PhysicsWorld,
    body: &LyingCapsule,
    motion: DVec3,
) -> (LyingMoveOutcome, Vec<SlideContact>) {
    let mut contacts = Vec::new();
    let outcome = character.move_lying_into(world, body, motion, &mut contacts);
    assert_eq!(
        contacts.len(),
        outcome.slides as usize,
        "every slide is one contact: {contacts:?}"
    );
    (outcome, contacts)
}

/// **Crawling backward into a wall records the wall the feet met**: the
/// collider, its face's normal, how far along the move the heels touched it,
/// and the advance to a skin width short of it, which is the whole of the
/// move's motion.
#[test]
fn crawling_backward_into_a_wall_records_the_wall_the_feet_met() {
    let config = prone_config();
    let mut world = floor();
    let wall = wall_behind(&mut world, BEHIND);
    let (mut character, body) = lying_at(&mut world, 0.0, 0.0, 0.0);
    let back = DVec3::new(0.0, 0.0, 1.0);

    let (outcome, contacts) = recorded(&mut character, &mut world, &body, back);

    let [met] = contacts[..] else {
        panic!("the wall behind, not {contacts:?}");
    };
    assert_eq!(met.collider, wall);
    assert!(
        (met.normal - DVec3::NEG_Z).length() < EXACT,
        "the wall's face, not {:?}",
        met.normal
    );
    assert_eq!(met.requested, back, "the flat move on flat ground");
    let heels = body.feet().z + body.radius;
    let expected = BEHIND - heels;
    assert!(
        met.fraction <= expected + EXACT && met.fraction > expected - SWEEP_TOLERANCE,
        "met at {} of the way, and the wall is {expected} off",
        met.fraction
    );
    assert!(!met.started_inside && !met.stepped_up);
    let advance = met.fraction - config.skin_width;
    assert!(
        (met.applied - back * advance).length() < EXACT,
        "applied {:?}, a skin short of {}",
        met.applied,
        met.fraction
    );
    assert!(
        met.remaining.length() < EXACT,
        "the wall stopped it dead: {:?}",
        met.remaining
    );

    assert!(outcome.hit_wall && outcome.grounded);
    assert!(
        (outcome.motion - met.applied).length() < EXACT,
        "the move is the advance: {:?}",
        outcome.motion
    );
}

/// **A move nothing gets in the way of records nothing.**
#[test]
fn a_lying_move_with_no_contact_records_nothing() {
    let mut world = floor();
    let (mut character, body) = lying_at(&mut world, 0.0, 0.0, 0.0);

    let (outcome, contacts) = recorded(
        &mut character,
        &mut world,
        &body,
        DVec3::new(0.1, 0.0, -0.3),
    );

    assert!(contacts.is_empty(), "{contacts:?}");
    assert_eq!(outcome.slides, 0);
    assert!(outcome.grounded, "the settle is not a contact");
}

/// **The sink is cleared before each move**, as the upright move's is: a
/// buffer reused from a move that met a wall holds nothing after one that
/// met nothing.
#[test]
fn the_sink_is_cleared_before_each_lying_move() {
    let mut walled = floor();
    wall_behind(&mut walled, BEHIND);
    let (mut blocked, body) = lying_at(&mut walled, 0.0, 0.0, 0.0);
    let mut contacts = Vec::new();
    blocked.move_lying_into(&mut walled, &body, DVec3::new(0.0, 0.0, 1.0), &mut contacts);
    assert_eq!(contacts.len(), 1, "the wall filled the buffer");

    let mut open = floor();
    let (mut free, body) = lying_at(&mut open, 0.0, 0.0, 0.0);
    let outcome = free.move_lying_into(&mut open, &body, DVec3::new(0.0, 0.0, 1.0), &mut contacts);

    assert_eq!(outcome.slides, 0);
    assert!(
        contacts.is_empty(),
        "the wall's contact was kept: {contacts:?}"
    );
}

/// **Recording contacts does not change the lying move, to the bit.** Two
/// controllers crawl the same script — into the wall behind, along a wall
/// beside, over a riser low enough to ride up, off its far edge, and falling
/// under the caller's gravity — one recording and one not, and must agree on
/// every body, position, ground and outcome exactly.
#[test]
fn recording_contacts_moves_a_lying_body_exactly_as_not_recording() {
    let mut world = floor();
    let behind = wall_behind(&mut world, BEHIND);
    // Its -X face is x = 0.35, 0.05 beside the body's flank.
    let beside = world.add_box(BoxCollider::new(
        DVec3::new(5.35, 1.0, 0.0),
        DVec3::new(5.0, 1.0, 10.0),
    ));
    // A riser 4 cm high across the path ahead, over z in [-3, -2].
    world.add_box(BoxCollider::new(
        DVec3::new(-2.0, 0.02, -2.5),
        DVec3::new(3.0, 0.02, 0.5),
    ));
    let (plain_start, body) = lying_at(&mut world, 0.0, 0.0, 0.0);
    let mut plain = plain_start.clone();
    let mut recording = plain_start;
    let (mut plain_body, mut recording_body) = (body, body);
    let mut contacts = Vec::new();
    let mut seen = Vec::new();

    let gravity = DVec3::new(0.0, -0.05, 0.0);
    for tick in 0..120 {
        let motion = match tick {
            0..10 => DVec3::new(0.0, 0.0, 0.1),
            10..20 => DVec3::new(0.02, 0.0, -0.05),
            _ if tick % 30 == 29 => DVec3::new(0.0, 0.3, -0.05),
            _ => DVec3::new(-0.01, 0.0, -0.08) + gravity,
        };
        let expected = plain.move_lying(&mut world, &plain_body, motion);
        let outcome = recording.move_lying_into(&mut world, &recording_body, motion, &mut contacts);
        assert_eq!(outcome, expected, "tick {tick}");
        let bits = |body: &LyingCapsule| {
            [body.head.x, body.head.y, body.head.z, body.pitch_sine].map(f64::to_bits)
        };
        assert_eq!(bits(&outcome.body), bits(&expected.body), "tick {tick}");
        assert_eq!(
            recording.position().to_array().map(f64::to_bits),
            plain.position().to_array().map(f64::to_bits),
            "tick {tick}"
        );
        assert_eq!(recording.ground(), plain.ground(), "tick {tick}");
        assert_eq!(contacts.len(), outcome.slides as usize, "tick {tick}");
        seen.extend_from_slice(&contacts);
        (plain_body, recording_body) = (expected.body, outcome.body);
    }
    for (wall, name) in [(behind, "behind"), (beside, "beside")] {
        assert!(
            seen.iter().any(|contact| contact.collider == wall),
            "the script met the wall {name}: {seen:?}"
        );
    }
    assert!(
        plain.position().z < -3.0,
        "the script crawled over the riser to {:?}",
        plain.position()
    );
}
