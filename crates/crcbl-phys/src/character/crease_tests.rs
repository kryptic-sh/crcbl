use std::f64::consts::FRAC_PI_6;

use glam::DVec3;

use crate::collider::BoxCollider;
use crate::integrator::rotation_from_scaled_axis;
use crate::world::PhysicsWorld;

use super::{CharacterConfig, CharacterController};

/// **A character falling into the corner of a wall and a turned box keeps
/// falling down the crease between them.**
///
/// The wall fills `x >= 5`; the box is a slab turned 30° whose far end
/// meets it, so its `-X` face and the wall make a vertical V the move drives
/// into. Their crease is vertical, so once wedged the clip keeps the fall and
/// nothing else. The hang this guards: the second wall is met within
/// rounding of the skin width, the advance is about 1e-17, and that once
/// emptied the plane set, so the clip ran against the box alone, turned the
/// move against itself and stopped it dead — every tick, with gravity
/// applied, in mid-air.
#[test]
fn a_character_falling_into_a_corner_keeps_falling_down_the_crease() {
    let config = CharacterConfig::default();
    let mut world = PhysicsWorld::new();
    let wall = world.add_box(BoxCollider::new(
        DVec3::new(6.0, 0.0, 0.0),
        DVec3::new(1.0, 5.0, 10.0),
    ));
    let slab = world.add_box(
        BoxCollider::new(DVec3::new(4.6, 0.0, -2.0), DVec3::new(0.25, 5.0, 1.0))
            .with_rotation(rotation_from_scaled_axis(DVec3::Y * FRAC_PI_6)),
    );
    let mut character = CharacterController::new(config, DVec3::new(3.0, 2.0, -1.0));
    let fall = -0.01;
    let motion = DVec3::new(0.08, fall, -0.03);
    let mut contacts = Vec::new();
    let mut wedged = false;
    let mut last = DVec3::ZERO;

    for tick in 0..200 {
        let outcome = character.move_and_slide_into(&mut world, motion, &mut contacts);
        assert!(
            outcome.motion.y < 0.0,
            "tick {tick} stopped falling at {:?}, met {contacts:?}",
            character.position(),
        );
        let met = |collider| contacts.iter().any(|contact| contact.collider == collider);
        wedged |= met(wall) && met(slab);
        last = outcome.motion;
    }
    assert!(
        wedged,
        "the move never met the wall and the box together, so the crease this \
         is about never ran"
    );
    assert!(
        (last - DVec3::Y * fall).length() < 1e-12,
        "wedged in a vertical crease, the whole fall survives and nothing else \
         does: {last:?}",
    );
}
