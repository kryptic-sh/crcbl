//! [`CharacterController::preview_upright`]: the move's answer, to the bit,
//! from a call that changes nothing.

use glam::DVec3;

use crate::broadphase::Segment;
use crate::collider::{Aabb, BoxCollider, Capsule};
use crate::query::{Penetration, ShapeHit};
use crate::world::{BroadphaseStats, ColliderId, PhysicsWorld, QueryFilter, QueryScratch};

use super::{CharacterConfig, CharacterController, UprightPreview};

/// How closely a position or fraction worked out by hand has to agree with
/// the move's: every sweep here is closed-form against a box, so only
/// rounding separates them.
const EXACT: f64 = 1e-12;

/// The layer `a_masked_collider_is_previewed_as_the_move_ignores_it` puts its
/// item on.
const ITEMS: u32 = 1 << 3;

/// The vertical displacement a walking tick adds for gravity, enough to keep
/// the ground probe honest without being a fall.
const GRAVITY_TICK: f64 = -0.01;

/// How many previews the no-touch test runs before it looks again.
const REPEATED_PREVIEWS: usize = 16;

/// A floor whose top is `y = 0`.
fn floor(world: &mut PhysicsWorld) -> ColliderId {
    world.add_box(BoxCollider::new(
        DVec3::new(0.0, -1.0, 0.0),
        DVec3::new(50.0, 1.0, 50.0),
    ))
}

/// The centre a capsule needs so its feet are at `feet`.
fn centre_for_feet(config: &CharacterConfig, feet: f64) -> f64 {
    feet + config.radius + config.half_height
}

/// [`standing`], with its ground forgotten: where a jump leaves a character
/// on the tick it takes off.
fn airborne(world: &mut PhysicsWorld) -> CharacterController {
    let mut character = standing(world);
    character.set_position(character.position());
    character
}

/// A character at the origin with its feet on `y = 0`, settled onto the
/// floor by a zero move.
fn standing(world: &mut PhysicsWorld) -> CharacterController {
    let config = CharacterConfig::default();
    let mut character =
        CharacterController::new(config, DVec3::new(0.0, centre_for_feet(&config, 0.0), 0.0));
    character.move_and_slide(world, DVec3::ZERO);
    assert!(character.is_grounded(), "the fixture starts on the floor");
    character
}

/// Register `character`'s capsule in `world` and bind the controller to it,
/// as a character other bodies collide with is.
fn bind(world: &mut PhysicsWorld, character: CharacterController) -> CharacterController {
    let collider = world.add_capsule(character.capsule());
    character.with_self_collider(collider)
}

/// The preview, through a view of `world` and buffers of its own.
fn preview(
    character: &CharacterController,
    world: &mut PhysicsWorld,
    motion: DVec3,
) -> UprightPreview {
    let mut scratch = QueryScratch::new();
    character.preview_upright(world.overlap_queries(), &mut scratch, motion)
}

/// Preview `motion`, then make it, and require the two to agree exactly:
/// the outcome, every contact, the capsule, the ground, and — for a bound
/// controller — the collider the move wrote. Returns the preview.
fn previewed_then_moved(
    character: &mut CharacterController,
    world: &mut PhysicsWorld,
    motion: DVec3,
) -> UprightPreview {
    let predicted = preview(character, world, motion);
    let mut contacts = Vec::new();
    let outcome = character.move_and_slide_into(world, motion, &mut contacts);

    assert_eq!(predicted.outcome, outcome, "the outcome of {motion:?}");
    assert_eq!(predicted.contacts, contacts, "the contacts of {motion:?}");
    assert_eq!(
        predicted.capsule,
        character.capsule(),
        "where {motion:?} left the capsule"
    );
    assert_eq!(
        predicted.ground.as_ref(),
        character.ground(),
        "the ground after {motion:?}"
    );
    assert_eq!(
        predicted.contacts.len(),
        predicted.outcome.slides as usize,
        "every slide is one contact"
    );
    if let Some(collider) = character.self_collider {
        assert_eq!(
            world.aabb_of(collider),
            Some(predicted.capsule.aabb()),
            "the move wrote the capsule the preview predicted"
        );
    }
    predicted
}

/// Run `motions` from `character` in a world `build` makes, unbound and then
/// bound to a collider, previewing each before making it. Returns each run's
/// previews, unbound first.
fn both_bindings(
    build: impl Fn() -> (PhysicsWorld, CharacterController),
    motions: &[DVec3],
) -> [Vec<UprightPreview>; 2] {
    [false, true].map(|bound| {
        let (mut world, character) = build();
        let mut character = if bound {
            bind(&mut world, character)
        } else {
            character
        };
        motions
            .iter()
            .map(|&motion| previewed_then_moved(&mut character, &mut world, motion))
            .collect()
    })
}

// ── Preview equals move ────────────────────────────────────────────────

#[test]
fn a_walk_into_a_wall_is_previewed_as_the_move_makes_it() {
    let runs = both_bindings(
        || {
            let mut world = PhysicsWorld::new();
            floor(&mut world);
            // Its -X face is x = 0.5.
            world.add_box(BoxCollider::new(
                DVec3::new(1.5, 2.0, 0.0),
                DVec3::new(1.0, 2.0, 10.0),
            ));
            let character = standing(&mut world);
            (world, character)
        },
        &[DVec3::new(0.1, GRAVITY_TICK, 0.05); 10],
    );
    for previews in runs {
        assert!(
            previews.iter().any(|p| p.outcome.hit_wall),
            "the walk reached the wall"
        );
        assert!(
            previews.iter().all(|p| p.outcome.grounded),
            "it walked, never fell"
        );
    }
}

#[test]
fn a_start_inside_geometry_is_previewed_with_the_same_push_out() {
    let runs = both_bindings(
        || {
            let mut world = PhysicsWorld::new();
            floor(&mut world);
            // Its -X face is x = 0.25, inside the capsule's 0.3 radius.
            world.add_box(BoxCollider::new(
                DVec3::new(0.35, 1.0, 0.0),
                DVec3::new(0.1, 1.0, 1.0),
            ));
            let config = CharacterConfig::default();
            let character = CharacterController::new(
                config,
                DVec3::new(0.0, centre_for_feet(&config, 0.0), 0.0),
            );
            (world, character)
        },
        &[
            DVec3::ZERO,
            DVec3::new(-0.05, GRAVITY_TICK, 0.0),
            DVec3::new(0.1, GRAVITY_TICK, 0.0),
        ],
    );
    for previews in runs {
        assert!(
            previews[0].outcome.depenetration.x < 0.0,
            "the first move started inside the box and was pushed out along -X: {:?}",
            previews[0].outcome
        );
    }
}

#[test]
fn a_jump_into_a_ceiling_is_previewed_as_the_move_makes_it() {
    let runs = both_bindings(
        || {
            let mut world = PhysicsWorld::new();
            floor(&mut world);
            // Its underside is y = 1.9, 0.09 above a settled capsule's top.
            world.add_box(BoxCollider::new(
                DVec3::new(0.0, 2.0, 0.0),
                DVec3::new(3.0, 0.1, 3.0),
            ));
            let character = standing(&mut world);
            (world, character)
        },
        &[
            DVec3::new(0.02, 0.3, 0.0),
            DVec3::new(0.02, -0.05, 0.0),
            DVec3::new(0.02, -0.2, 0.0),
        ],
    );
    for previews in runs {
        assert!(previews[0].outcome.hit_ceiling, "the jump met the ceiling");
        assert!(previews[0].ground.is_none(), "and left the ground");
        assert!(previews[2].outcome.grounded, "the fall landed again");
    }
}

#[test]
fn a_step_is_previewed_with_the_same_climb() {
    let runs = both_bindings(
        || {
            let mut world = PhysicsWorld::new();
            // Two floors: `y = 0` up to x = 0.5, and a step 0.3 high past it.
            world.add_box(BoxCollider::new(
                DVec3::new(-24.5, -1.0, 0.0),
                DVec3::new(25.0, 1.0, 5.0),
            ));
            world.add_box(BoxCollider::new(
                DVec3::new(25.5, -0.85, 0.0),
                DVec3::new(25.0, 1.15, 5.0),
            ));
            let character = standing(&mut world);
            (world, character)
        },
        &[DVec3::new(0.1, GRAVITY_TICK, 0.0); 10],
    );
    for previews in runs {
        assert!(
            previews.iter().any(|p| p.outcome.stepped_up),
            "the walk climbed the step"
        );
        assert!(
            previews.iter().all(|p| p.outcome.grounded),
            "without leaving the ground"
        );
    }
}

#[test]
fn walking_off_a_ledge_is_previewed_with_the_same_loss_of_support() {
    let runs = both_bindings(
        || {
            let mut world = PhysicsWorld::new();
            // A floor ending at x = 1, and another two metres below it: a
            // drop far past the step offset, so nothing snaps down to it.
            world.add_box(BoxCollider::new(
                DVec3::new(-24.0, -1.0, 0.0),
                DVec3::new(25.0, 1.0, 5.0),
            ));
            world.add_box(BoxCollider::new(
                DVec3::new(0.0, -3.0, 0.0),
                DVec3::new(50.0, 1.0, 50.0),
            ));
            let character = standing(&mut world);
            (world, character)
        },
        &[DVec3::new(0.1, GRAVITY_TICK, 0.0); 16],
    );
    for previews in runs {
        assert!(previews[0].outcome.grounded, "the walk starts supported");
        assert!(
            !previews
                .last()
                .expect("the walk has ticks")
                .outcome
                .grounded,
            "and ends past the edge, unsupported"
        );
    }
}

#[test]
fn a_masked_collider_is_previewed_as_the_move_ignores_it() {
    let runs = both_bindings(
        || {
            let mut world = PhysicsWorld::new();
            floor(&mut world);
            let item = world.add_box(BoxCollider::new(
                DVec3::new(0.6, 0.5, 0.0),
                DVec3::new(0.1, 0.5, 2.0),
            ));
            assert!(world.set_layers(item, ITEMS));
            let character = standing(&mut world);
            (world, character.with_query_mask(!ITEMS))
        },
        &[DVec3::new(0.1, GRAVITY_TICK, 0.0); 12],
    );
    for previews in runs {
        assert!(
            previews.iter().all(|p| !p.outcome.hit_wall),
            "the item is off the mask"
        );
        assert!(
            previews
                .last()
                .expect("the walk has ticks")
                .capsule
                .centre
                .x
                > 1.0,
            "and the walk went through where it stands"
        );
    }
}

// ── Touches nothing ────────────────────────────────────────────────────

/// Everything the world would answer that a move or a preview could have
/// changed: each collider's box, the broadphase's counters, and a fixed set
/// of queries that see the character's own collider as well as the scene.
#[derive(Debug, PartialEq)]
struct WorldReading {
    boxes: Vec<Option<Aabb>>,
    broadphase: BroadphaseStats,
    sweeps: Vec<Option<(ColliderId, ShapeHit)>>,
    penetrations: Vec<(ColliderId, Penetration)>,
}

fn read_world(world: &mut PhysicsWorld, colliders: &[ColliderId], at: &Capsule) -> WorldReading {
    let sweeps = [
        (DVec3::new(3.0, 1.0, 0.0), DVec3::new(-3.0, 1.0, 0.0)),
        (DVec3::new(0.0, 4.0, 0.0), DVec3::new(0.0, -1.0, 0.0)),
        (DVec3::new(0.0, 1.0, 3.0), DVec3::new(0.0, 1.0, -3.0)),
    ]
    .map(|(from, to)| {
        world.sweep_capsule_filtered(&Segment::new(from, to), 0.2, 0.3, QueryFilter::ALL)
    })
    .to_vec();
    let mut penetrations = Vec::new();
    world.capsule_penetrations_filtered_into(at, QueryFilter::ALL, &mut penetrations);
    WorldReading {
        boxes: colliders.iter().map(|&id| world.aabb_of(id)).collect(),
        broadphase: world.broadphase_stats(),
        sweeps,
        penetrations,
    }
}

/// A bound character on a floor, inside a box at its side, under a ceiling
/// and a step away from a wall: every phase of the move has something to do.
fn cluttered() -> (PhysicsWorld, CharacterController, Vec<ColliderId>) {
    let mut world = PhysicsWorld::new();
    let mut colliders = vec![floor(&mut world)];
    for (centre, half) in [
        // Overlapping the capsule from +X.
        (DVec3::new(0.35, 1.0, 0.0), DVec3::new(0.1, 1.0, 1.0)),
        // A ceiling with its underside at y = 2.1.
        (DVec3::new(0.0, 2.2, 0.0), DVec3::new(3.0, 0.1, 3.0)),
        // A wall with its +Z face at z = -0.5.
        (DVec3::new(0.0, 2.0, -0.6), DVec3::new(3.0, 2.0, 0.1)),
    ] {
        colliders.push(world.add_box(BoxCollider::new(centre, half)));
    }
    let config = CharacterConfig::default();
    let character =
        CharacterController::new(config, DVec3::new(0.0, centre_for_feet(&config, 0.0), 0.0));
    let character = bind(&mut world, character);
    colliders.push(character.self_collider.expect("bound"));
    (world, character, colliders)
}

/// **Repeated previews change no query and no controller field**, and so no
/// later move: the world reads the same to the bit, the controller's every
/// field is what it was, and a move made after them is the move made by a
/// twin that never previewed.
#[test]
fn repeated_previews_change_no_query_no_controller_field_and_no_later_move() {
    let motions = [
        DVec3::new(-0.1, 0.0, -0.4),
        DVec3::new(0.0, 0.6, 0.0),
        DVec3::new(0.2, -0.3, 0.1),
        DVec3::ZERO,
    ];

    let (mut world, mut character, colliders) = cluttered();
    let before = read_world(&mut world, &colliders, &character.capsule());
    let fields = format!("{character:?}");
    let mut first_round = Vec::new();
    for i in 0..REPEATED_PREVIEWS {
        let previewed = preview(&character, &mut world, motions[i % motions.len()]);
        assert!(
            previewed.outcome.depenetration != DVec3::ZERO,
            "each preview starts inside the box, so it would push the capsule out"
        );
        assert_ne!(previewed.capsule, character.capsule(), "and move it");
        match first_round.get(i % motions.len()) {
            Some(first) => assert_eq!(&previewed, first, "the same preview, asked again"),
            None => first_round.push(previewed),
        }
    }
    assert_eq!(
        read_world(&mut world, &colliders, &character.capsule()),
        before
    );
    assert_eq!(format!("{character:?}"), fields, "every controller field");

    let (mut twin_world, mut twin, _) = cluttered();
    let twin_before = read_world(&mut twin_world, &colliders, &twin.capsule());
    assert_eq!(
        twin_before, before,
        "the twin starts where the original did"
    );
    for &motion in &motions {
        let mut contacts = Vec::new();
        let mut twin_contacts = Vec::new();
        let outcome = character.move_and_slide_into(&mut world, motion, &mut contacts);
        let twin_outcome = twin.move_and_slide_into(&mut twin_world, motion, &mut twin_contacts);
        assert_eq!(outcome, twin_outcome);
        assert_eq!(contacts, twin_contacts);
        assert_eq!(format!("{character:?}"), format!("{twin:?}"));
        assert_eq!(
            read_world(&mut world, &colliders, &character.capsule()),
            read_world(&mut twin_world, &colliders, &twin.capsule()),
        );
    }
}

// ── Engine semantics EW's airborne forecasts read ──────────────────────

/// The share of `motion` a capsule centred at `centre` covers before its top
/// reaches a ceiling whose underside is `ceiling`.
fn ceiling_fraction(config: &CharacterConfig, centre: DVec3, ceiling: f64, motion: DVec3) -> f64 {
    (ceiling - (centre.y + config.half_height + config.radius)) / motion.y
}

/// The share of `motion` a capsule centred at `centre` covers before its
/// `-Z` side reaches a wall whose face is `wall_z`.
fn wall_fraction(config: &CharacterConfig, centre: DVec3, wall_z: f64, motion: DVec3) -> f64 {
    (centre.z - config.radius - wall_z) / -motion.z
}

/// EW's `ceiling_contact_does_not_delay_wall_steering`, as far as the engine
/// answers it: a rising chord toward a wall under a ceiling reports both
/// contacts in the order the geometry puts them, and the ceiling takes
/// nothing from the travel toward and along the wall. The capsule ends a skin
/// width off the wall with the whole strafe kept, exactly where the same
/// chord leaves it without a ceiling, and a bound collider follows the move
/// and never the preview.
#[test]
fn ceiling_contact_does_not_take_travel_from_the_wall_contact() {
    const CEILING: f64 = 2.1;
    let config = CharacterConfig::default();
    for wall_distance in [0.4, 0.65] {
        for strafe in [0.0, 0.3] {
            for bound in [false, true] {
                for pieces in [1u32, 8] {
                    let build = |ceiling: bool| {
                        let mut world = PhysicsWorld::new();
                        floor(&mut world);
                        if ceiling {
                            world.add_box(BoxCollider::new(
                                DVec3::new(0.0, CEILING + 0.1, 0.0),
                                DVec3::new(3.0, 0.1, 3.0),
                            ));
                        }
                        world.add_box(BoxCollider::new(
                            DVec3::new(0.0, 2.0, -wall_distance - 0.1),
                            DVec3::new(3.0, 2.0, 0.1),
                        ));
                        let character = airborne(&mut world);
                        let character = if bound {
                            bind(&mut world, character)
                        } else {
                            character
                        };
                        (world, character)
                    };
                    let chord = DVec3::new(strafe, 0.9, -0.8);
                    let (mut world, mut character) = build(true);
                    let start = character.position();
                    let ceiling_first = ceiling_fraction(&config, start, CEILING, chord)
                        < wall_fraction(&config, start, -wall_distance, chord);
                    assert_eq!(
                        ceiling_first,
                        wall_distance > 0.5,
                        "the two distances put the contacts in both orders"
                    );

                    let mut met = Vec::new();
                    for _ in 0..pieces {
                        let previewed = previewed_then_moved(
                            &mut character,
                            &mut world,
                            chord / f64::from(pieces),
                        );
                        met.extend(previewed.contacts);
                    }
                    let case = format!(
                        "wall_distance={wall_distance} strafe={strafe} bound={bound} pieces={pieces}"
                    );
                    let ceiling = met.iter().position(|c| character.is_ceiling(c.normal));
                    let wall = met.iter().position(|c| {
                        !character.is_ceiling(c.normal) && !character.is_walkable(c.normal)
                    });
                    let (Some(ceiling), Some(wall)) = (ceiling, wall) else {
                        panic!("both contacts are reported: {case} {met:?}");
                    };
                    assert_eq!(
                        ceiling < wall,
                        ceiling_first,
                        "in the geometry's order: {case}"
                    );
                    assert!(!character.is_grounded(), "still airborne: {case}");

                    if pieces == 1 {
                        let first = &met[0];
                        let expected = if ceiling_first {
                            ceiling_fraction(&config, start, CEILING, chord)
                        } else {
                            wall_fraction(&config, start, -wall_distance, chord)
                        };
                        assert!(
                            (first.fraction - expected).abs() < EXACT,
                            "the first contact's share of the chord: {case} {first:?}"
                        );
                        let at_ceiling = &met[ceiling];
                        assert!(
                            at_ceiling.remaining.y.abs() < EXACT,
                            "the ceiling removes only the rise: {case} {at_ceiling:?}"
                        );
                        let horizontal = |v: DVec3| DVec3::new(v.x, 0.0, v.z);
                        assert!(
                            (horizontal(at_ceiling.remaining)
                                - horizontal(at_ceiling.requested - at_ceiling.applied))
                            .length()
                                < EXACT,
                            "and keeps every horizontal part it did not apply: {case} {at_ceiling:?}"
                        );
                    }

                    let end = character.position();
                    let expected = DVec3::new(
                        start.x + strafe,
                        CEILING - config.skin_width - config.half_height - config.radius,
                        -wall_distance + config.radius + config.skin_width,
                    );
                    assert!(
                        (end - expected).length() < EXACT,
                        "a skin off the wall and the ceiling, the strafe kept: {case} {end:?}"
                    );

                    let (mut open_world, mut open) = build(false);
                    for _ in 0..pieces {
                        open.move_and_slide(&mut open_world, chord / f64::from(pieces));
                    }
                    let flat = |v: DVec3| DVec3::new(v.x, 0.0, v.z);
                    assert!(
                        (flat(end) - flat(open.position())).length() < EXACT,
                        "the ceiling moved nothing horizontally: {case}"
                    );
                }
            }
        }
    }
}

/// EW's `ceiling_contact_uses_the_remaining_time_for_descent`, as far as the
/// engine answers it: a jump chord that meets a ceiling reports the share of
/// the chord covered before touching, keeps every horizontal part of the
/// chord, and leaves the capsule a skin width under the ceiling free to fall
/// at once — so the caller can spend the rest of its tick descending. One
/// coarse chord and the same chord in pieces end in the same place.
#[test]
fn a_ceiling_contact_reports_its_share_and_leaves_the_descent_free() {
    const CEILING: f64 = 1.9;
    const PIECES: u32 = 8;
    let config = CharacterConfig::default();
    for moving in [false, true] {
        for bound in [false, true] {
            let prepare = || {
                let mut world = PhysicsWorld::new();
                floor(&mut world);
                world.add_box(BoxCollider::new(
                    DVec3::new(0.0, CEILING + 0.1, 0.0),
                    DVec3::new(3.0, 0.1, 3.0),
                ));
                let character = airborne(&mut world);
                let character = if bound {
                    bind(&mut world, character)
                } else {
                    character
                };
                (world, character)
            };
            let case = format!("moving={moving} bound={bound}");
            let chord = DVec3::new(0.0, 0.5, if moving { -0.3 } else { 0.0 });

            let (mut coarse_world, mut coarse) = prepare();
            let start = coarse.position();
            let previewed = previewed_then_moved(&mut coarse, &mut coarse_world, chord);
            assert!(
                previewed.outcome.hit_ceiling,
                "the jump met the ceiling: {case}"
            );
            let touch = &previewed.contacts[0];
            assert!(
                coarse.is_ceiling(touch.normal),
                "first, the ceiling: {case}"
            );
            assert!(
                (touch.fraction - ceiling_fraction(&config, start, CEILING, chord)).abs() < EXACT,
                "its share of the chord: {case} {touch:?}"
            );
            let expected = DVec3::new(
                start.x,
                CEILING - config.skin_width - config.half_height - config.radius,
                start.z + chord.z,
            );
            assert!(
                (coarse.position() - expected).length() < EXACT,
                "a skin under the ceiling with the whole horizontal kept: {case} {:?}",
                coarse.position()
            );

            let (mut split_world, mut split) = prepare();
            for _ in 0..PIECES {
                previewed_then_moved(&mut split, &mut split_world, chord / f64::from(PIECES));
            }
            assert!(
                (coarse.position() - split.position()).length() < EXACT,
                "coarse and split chords agree: {case} {:?} {:?}",
                coarse.position(),
                split.position()
            );

            let fall = DVec3::new(0.0, -0.05, 0.0);
            let descent = previewed_then_moved(&mut coarse, &mut coarse_world, fall);
            assert_eq!(
                descent.outcome.slides, 0,
                "nothing holds it to the ceiling: {case}"
            );
            assert!(
                (descent.outcome.motion - fall).length() < EXACT,
                "it falls the whole way asked: {case} {:?}",
                descent.outcome
            );
            assert!(!descent.outcome.grounded, "still above the floor: {case}");
        }
    }
}

/// A preview lists every contact of the slide in order, which is what lets a
/// caller find a wall the slide only reaches after a ceiling turned it,
/// behind the closest hit of the chord's first sweep.
#[test]
fn a_preview_lists_a_later_wall_behind_the_first_contact() {
    let mut world = PhysicsWorld::new();
    floor(&mut world);
    // A ceiling with its underside at y = 1.9, and a wall whose -X face is
    // x = 1, which the chord only reaches after the ceiling turns it.
    world.add_box(BoxCollider::new(
        DVec3::new(0.0, 2.0, 0.0),
        DVec3::new(5.0, 0.1, 5.0),
    ));
    world.add_box(BoxCollider::new(
        DVec3::new(1.5, 1.0, 0.0),
        DVec3::new(0.5, 1.0, 5.0),
    ));
    let character = standing(&mut world);
    let previewed = preview(&character, &mut world, DVec3::new(1.0, 0.5, 0.0));
    let normals: Vec<DVec3> = previewed.contacts.iter().map(|c| c.normal).collect();
    assert_eq!(
        normals,
        [DVec3::NEG_Y, DVec3::NEG_X],
        "ceiling first, then the wall"
    );
}
