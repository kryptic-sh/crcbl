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

/// EW's V-crease (2026-10-23): one mesh of two faces sloping up at 60° on
/// either side of a crease along `z` at `x = 0`, turned by `turn`. Each face
/// alone is steeper than the default slope limit.
fn v_crease(world: &mut PhysicsWorld, turn: glam::DQuat) -> ColliderId {
    use crate::{Transform, TriangleMesh};

    // tan 60°, which is √3: `sqrt` is correctly rounded, and platform
    // trigonometry is not deterministic.
    let rise = 5.0 * 3_f64.sqrt();
    let mesh = TriangleMesh::new(
        &[
            DVec3::new(-5.0, rise, -5.0),
            DVec3::new(-5.0, rise, 5.0),
            DVec3::new(0.0, 0.0, -5.0),
            DVec3::new(0.0, 0.0, 5.0),
            DVec3::new(5.0, rise, -5.0),
            DVec3::new(5.0, rise, 5.0),
        ],
        &[[0, 1, 3], [0, 3, 2], [2, 3, 5], [2, 5, 4]],
    )
    .expect("two faces");
    world.add_mesh(
        mesh,
        Transform {
            rotation: turn,
            ..Transform::IDENTITY
        },
    )
}

/// One tick of a character falling under gravity, or walking `walk` on the
/// ground: what a game's controller does with it.
fn tick(
    character: &mut CharacterController,
    world: &mut PhysicsWorld,
    fall: &mut f64,
    walk: DVec3,
) -> bool {
    const DT: f64 = 1.0 / 60.0;
    *fall -= 9.81 * DT;
    let outcome = character.move_and_slide(world, walk + DVec3::Y * (*fall * DT));
    if outcome.grounded {
        *fall = 0.0;
    }
    outcome.grounded
}

/// **A capsule wedged in a V-crease stands in it**, though each face alone
/// is too steep: the two normals holding it up sum to straight up, which is
/// footing, and it reports that as its ground. Before, it lay in the crease
/// for five seconds ungrounded (EW's report).
#[test]
fn a_character_wedged_in_a_v_crease_is_grounded() {
    let mut world = PhysicsWorld::new();
    v_crease(&mut world, glam::DQuat::IDENTITY);
    let mut character =
        CharacterController::new(CharacterConfig::default(), DVec3::new(0.0, 1.0, 0.0));
    let mut fall = 0.0;
    let grounded = (0..300)
        .map(|_| tick(&mut character, &mut world, &mut fall, DVec3::ZERO))
        .last()
        .expect("ticks ran");
    assert!(
        grounded,
        "settled in the crease at {:?}",
        character.position()
    );
    let ground = character.ground().expect("ground");
    assert!(
        (ground.normal - DVec3::Y).length() < 1e-6,
        "the crease's footing points up: {:?}",
        ground.normal
    );
}

/// **It walks along the crease at a walk**, not at air control: a second of
/// 3 m/s along `z` covers about 3 m and never leaves the ground.
#[test]
fn a_character_walks_along_a_v_crease() {
    let mut world = PhysicsWorld::new();
    v_crease(&mut world, glam::DQuat::IDENTITY);
    let mut character =
        CharacterController::new(CharacterConfig::default(), DVec3::new(0.0, 1.0, 0.0));
    let mut fall = 0.0;
    for _ in 0..120 {
        tick(&mut character, &mut world, &mut fall, DVec3::ZERO);
    }
    let start = character.position().z;
    for step in 0..60 {
        assert!(
            tick(&mut character, &mut world, &mut fall, DVec3::Z * 0.05),
            "left the ground on step {step} at {:?}",
            character.position()
        );
    }
    let walked = character.position().z - start;
    assert!((walked - 3.0).abs() < 0.05, "walked {walked} m");
}

/// **A crease steeper along its own length than the slope limit is still no
/// footing**: tipped 50° about `x`, its faces' normals sum to a normal 50°
/// off vertical, which the default 45° limit refuses, so it slides.
#[test]
fn a_v_crease_too_steep_along_its_length_is_no_footing() {
    let mut world = PhysicsWorld::new();
    v_crease(
        &mut world,
        rotation_from_scaled_axis(DVec3::X * 50_f64.to_radians()),
    );
    let config = CharacterConfig::default();
    let character = CharacterController::new(config, DVec3::new(0.0, 1.0, 0.0));
    let probe = character
        .probe_ground_at(&mut world, DVec3::new(0.0, 1.0, 0.0), 2.0)
        .expect("the crease is below");
    assert!(!probe.walkable, "{probe:?}");
}
