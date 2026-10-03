use glam::DVec3;

use crate::collider::{BoxCollider, Capsule};
use crate::world::PhysicsWorld;

use super::{CharacterConfig, CharacterController};

/// How far ahead, in `-z`, the other character stands.
const OTHER_AT: f64 = -4.0;

/// The sprint's first stride, in `z`, which carries the character from
/// standing still to resting against the other one.
const SPRINT_STRIDE: f64 = -7.7;

/// Each held tick's stride, in `z`, into the other character.
const HELD_STRIDE: f64 = -6.0;

/// How many ticks the character keeps walking into the other one once it
/// rests against it.
const PRESSED_TICKS: usize = 120;

/// **A character resting a skin off another character it keeps walking into
/// stays put, to the bit.**
///
/// Each tick's sweep meets the other capsule within rounding of the skin
/// width, so the slide takes the back-off branch, and the capsule-against-
/// capsule sweep's time is not exact: the rest is about 1e-15 nearer than the
/// skin. Backing off that rounding-sized gap moved the capsule an ulp along
/// the normal every tick, and a game asserting that a character pressed into
/// someone does not move saw its feet go from `-3.390000000000001` to `-3.39`
/// while standing still. The moves are the game's: a sprint's long first
/// stride into the other character, then a held key.
#[test]
fn a_character_pressed_into_another_does_not_move() {
    let config = CharacterConfig::default();
    let mut world = PhysicsWorld::new();
    world.add_box(BoxCollider::new(
        DVec3::new(0.0, -0.5, 0.0),
        DVec3::new(100.0, 0.5, 100.0),
    ));
    let standing = config.radius + config.half_height + config.skin_width;
    let other = world.add_capsule(Capsule {
        centre: DVec3::new(0.0, standing, OTHER_AT),
        radius: config.radius,
        half_height: config.half_height,
    });
    let mut character = CharacterController::new(config, DVec3::new(0.0, standing, 0.0));
    let mut contacts = Vec::new();

    character.move_and_slide_into(&mut world, DVec3::NEG_Y, &mut contacts);
    character.move_and_slide_into(&mut world, DVec3::Z * SPRINT_STRIDE, &mut contacts);
    let rest = character.position();
    assert!(
        (rest.z - (OTHER_AT + 2.0 * config.radius + config.skin_width)).abs() < 1e-9,
        "the walk did not end against the other character: {rest:?}"
    );

    for tick in 0..PRESSED_TICKS {
        character.move_and_slide_into(&mut world, DVec3::Z * HELD_STRIDE, &mut contacts);
        assert!(
            contacts.iter().any(|contact| contact.collider == other),
            "tick {tick} did not meet the other character, so the rest this is \
             about never ran: {contacts:?}"
        );
        assert_eq!(
            character.position().to_array().map(f64::to_bits),
            rest.to_array().map(f64::to_bits),
            "tick {tick} moved a character pressed into another from {rest:?} to \
             {:?}",
            character.position(),
        );
    }
}
