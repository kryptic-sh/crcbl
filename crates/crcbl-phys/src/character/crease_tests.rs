use std::f64::consts::FRAC_PI_6;

use glam::DVec3;

use crate::collider::BoxCollider;
use crate::integrator::rotation_from_scaled_axis;
use crate::world::{ColliderId, PhysicsWorld};

use super::{CharacterConfig, CharacterController};

/// The corner both tests fall into: an axis wall filling `x >= 5`, and a slab
/// turned 30° whose far end meets it, so its `-X` face and the wall make a
/// vertical V. Returns the wall, then the slab.
fn corner(world: &mut PhysicsWorld) -> (ColliderId, ColliderId) {
    let wall = world.add_box(BoxCollider::new(
        DVec3::new(6.0, 0.0, 0.0),
        DVec3::new(1.0, 5.0, 10.0),
    ));
    let slab = world.add_box(
        BoxCollider::new(DVec3::new(4.6, 0.0, -2.0), DVec3::new(0.25, 5.0, 1.0))
            .with_rotation(rotation_from_scaled_axis(DVec3::Y * FRAC_PI_6)),
    );
    (wall, slab)
}

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
    let (wall, slab) = corner(&mut world);
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

/// **No start of a scan falling into the same corner hangs on the turned box's
/// face.**
///
/// The slab is swept by conservative advancement, which reports a hit once
/// the gap is within its tolerance. A move that ends just short of touching
/// the face is no hit, so the capsule can finish a tick 1e-15 off it, nearer
/// than the skin width; the slide then has to back it off. When it backed off
/// only a sweep that started inside, the capsule stayed that near, crept
/// nearer by rounding as it slid down the face, and once the gap was about
/// 5e-17 the remainder clipped to run along the face met it again: the
/// segment's `end - start` rounds the clipped motion by about 3e-16, which
/// against that gap is a closing speed, and the advancement stopped at 2/9 of
/// the sweep with nothing applied — every sweep of every tick, in mid-air.
///
/// The slide now backs off every hit nearer than the skin width, not only one
/// that started inside. Before it did, 25 of these starts hung, all on one
/// diagonal of the grid. A fall that stops, with nothing below to stand on,
/// is the hang, as in the crease test above.
#[test]
fn no_start_falling_into_the_corner_hangs_on_the_turned_face() {
    const COLUMNS: u32 = 60;
    const ROWS: u32 = 80;
    const TICKS: u32 = 100;
    let (x_from, x_to) = (1.0, 4.0);
    let (z_from, z_to) = (-2.0, 2.0);
    let motion = DVec3::new(0.05, -0.01, -0.05);

    let mut world = PhysicsWorld::new();
    let (_, slab) = corner(&mut world);
    let mut contacts = Vec::new();
    let mut hung = Vec::new();
    let mut met_slab = 0;
    for column in 0..COLUMNS {
        for row in 0..ROWS {
            let start = DVec3::new(
                x_from + (x_to - x_from) * f64::from(column) / f64::from(COLUMNS),
                2.0,
                z_from + (z_to - z_from) * f64::from(row) / f64::from(ROWS),
            );
            let mut character = CharacterController::new(CharacterConfig::default(), start);
            let mut met = false;
            for tick in 0..TICKS {
                let outcome = character.move_and_slide_into(&mut world, motion, &mut contacts);
                met |= contacts.iter().any(|contact| contact.collider == slab);
                if outcome.motion.y >= 0.0 {
                    hung.push((start, tick, character.position()));
                    break;
                }
            }
            met_slab += u32::from(met);
        }
    }
    assert!(
        met_slab > COLUMNS * ROWS / 2,
        "only {met_slab} starts met the slab, so the scan mostly fell past the \
         face this is about"
    );
    assert!(
        hung.is_empty(),
        "{} of {} starts stopped falling in mid-air (start, tick, position): \
         {hung:?}",
        hung.len(),
        COLUMNS * ROWS,
    );
}
