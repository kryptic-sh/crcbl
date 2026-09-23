//! Boxes, run whole: rung 2 of `docs/plan/36-contact-solver.md`, measured on
//! the scenes its row names — a column, a pyramid, dominoes — and on what a
//! box's friction should do.
//!
//! As in `contacts.rs`, every bound was measured before it was written down,
//! and each says what it was measured at. These are properties of the whole
//! pipeline over a long run, not of the box manifold alone, whose known values
//! are unit tests beside it.

use crcbl_ecs::{Entity, SystemTrait as _};
use crcbl_phys::{
    ColliderComponent, ContactSettings, GravityForce, MassProperties, PhysicsSystem, RigidBody,
    SurfaceMaterial, Transform,
};
use glam::{DQuat, DVec3};

/// The tick every test steps by: the engine's default 60 Hz.
const DT: f64 = 1.0 / 60.0;

/// Standard gravity.
const G: f64 = 9.81;

/// Box2D's default friction, and no bounce: what stacking is measured with.
const CRATE: SurfaceMaterial = SurfaceMaterial::new(0.6, 0.0);

fn entity(index: u32) -> Entity {
    Entity::from_bits((1u64 << 32) | u64::from(index)).expect("generation 1 is never zero")
}

/// The default settings with sleep off, for a test that measures the solver
/// holding a stack up over a long run: asleep, a stack holds by not being
/// solved at all.
const AWAKE: ContactSettings = ContactSettings {
    sleep: false,
    ..ContactSettings::DEFAULT
};

/// A system with contacts at `settings`, Earth gravity and a floor at `y = 0`
/// of `floor`.
fn system_with(settings: ContactSettings, floor: SurfaceMaterial) -> PhysicsSystem {
    let mut phys = PhysicsSystem::with_contacts(settings);
    phys.add_force_provider(Box::new(GravityForce::EARTH));
    phys.add_plane(DVec3::Y, 0.0, floor);
    phys
}

/// [`system_with`] at the default settings.
fn system(floor: SurfaceMaterial) -> PhysicsSystem {
    system_with(ContactSettings::DEFAULT, floor)
}

/// A dynamic box of `mass` and half-extents `half`, at `at` turned by
/// `rotation`.
fn crate_(
    phys: &mut PhysicsSystem,
    index: u32,
    at: DVec3,
    rotation: DQuat,
    half: DVec3,
    mass: f64,
    material: SurfaceMaterial,
) -> Entity {
    let e = entity(index);
    let inertia = MassProperties::cuboid(mass, half, DVec3::ZERO).inertia;
    phys.set_body(e, RigidBody::new_dynamic(mass).with_inertia(inertia));
    let transform = Transform::new(at, rotation);
    phys.set_transform(e, transform);
    phys.set_collider(
        e,
        &ColliderComponent::Box {
            offset: DVec3::ZERO,
            half_extents: half,
            is_trigger: false,
        },
        &transform,
    );
    phys.set_material(e, material);
    e
}

/// A static box.
fn slab(phys: &mut PhysicsSystem, index: u32, centre: DVec3, half: DVec3) -> Entity {
    let e = entity(index);
    let transform = Transform::from_position(centre);
    phys.set_transform(e, transform);
    phys.set_collider(
        e,
        &ColliderComponent::Box {
            offset: DVec3::ZERO,
            half_extents: half,
            is_trigger: false,
        },
        &transform,
    );
    phys.set_material(e, CRATE);
    e
}

fn position(phys: &PhysicsSystem, e: Entity) -> DVec3 {
    phys.transform(e).expect("registered").position
}

// ---------------------------------------------------------------------------
// Feature ids
// ---------------------------------------------------------------------------

/// **A box resting on a static box keeps its four feature ids, tick after
/// tick** — the persisted-id ratio the rung's row counts is one — and no
/// contact begins or ends once it has landed.
///
/// A flickering id is the bug decision 2's ids exist to prevent: each flicker
/// throws away that point's warm start, and a stack shivers. Measured on
/// 2026-09-23: from the second tick on, 4 points and 4 persisted every tick.
///
/// Sleep is off, since the ids are only rebuilt while the box is awake.
#[test]
fn a_box_resting_on_a_box_keeps_its_feature_ids() {
    let mut phys = PhysicsSystem::with_contacts(AWAKE);
    phys.add_force_provider(Box::new(GravityForce::EARTH));
    slab(
        &mut phys,
        100,
        DVec3::new(0.0, -0.5, 0.0),
        DVec3::new(2.0, 0.5, 2.0),
    );
    let half = DVec3::new(0.3, 0.2, 0.4);
    crate_(
        &mut phys,
        0,
        DVec3::new(0.1, half.y, -0.2),
        DQuat::IDENTITY,
        half,
        5.0,
        CRATE,
    );
    let (mut points, mut persisted) = (0, 0);
    for tick in 0..240 {
        phys.step(DT);
        let counters = phys.contact_counters();
        if tick >= 1 {
            assert_eq!(counters.touching, 1, "tick {tick}: {counters:?}");
            assert_eq!(counters.points, 4, "tick {tick}: {counters:?}");
            assert_eq!(
                counters.begun + counters.ended,
                0,
                "tick {tick}: {counters:?}"
            );
            points += counters.points;
            persisted += counters.persisted;
        }
    }
    assert_eq!(
        persisted, points,
        "{persisted} of {points} points persisted"
    );
    assert_eq!(
        phys.contact_counters().persisted_ratio(),
        Some(1.0),
        "the counter's own ratio"
    );
}

// ---------------------------------------------------------------------------
// Stacking
// ---------------------------------------------------------------------------

/// A column of `count` cubes of half-extent `half` stood on the floor, and
/// the entities bottom first.
fn column(phys: &mut PhysicsSystem, count: u32, half: f64) -> Vec<Entity> {
    (0..count)
        .map(|i| {
            let y = half + 2.0 * half * f64::from(i);
            crate_(
                phys,
                i,
                DVec3::new(0.0, y, 0.0),
                DQuat::IDENTITY,
                DVec3::splat(half),
                10.0,
                CRATE,
            )
        })
        .collect()
}

/// Where a column stands after one tick: how far its top box has sunk and
/// moved sideways, the most any box in it has tipped, as the sine of the
/// angle, and whether every box in it sleeps.
#[derive(Clone, Copy, Debug)]
struct ColumnSample {
    sunk: f64,
    sideways: f64,
    tipped: f64,
    asleep: bool,
}

/// A column of `count` one-metre cubes, each asking for `substeps`, in a
/// system at `settings`, sampled after each of `ticks` ticks: the sample after
/// tick `n` is at index `n - 1`.
fn column_trace(
    settings: ContactSettings,
    count: u32,
    substeps: u32,
    ticks: u32,
) -> Vec<ColumnSample> {
    let mut phys = system_with(settings, CRATE);
    let boxes = column(&mut phys, count, 0.5);
    for &e in &boxes {
        phys.set_substeps(e, substeps);
    }
    let top = *boxes.last().expect("a column has a top");
    let start = position(&phys, top);
    (0..ticks)
        .map(|_| {
            phys.step(DT);
            let drift = position(&phys, top) - start;
            let tipped = boxes
                .iter()
                .map(|&e| {
                    let up = phys.transform(e).expect("a box").rotation * DVec3::Y;
                    (up.x * up.x + up.z * up.z).sqrt()
                })
                .fold(0.0, f64::max);
            ColumnSample {
                sunk: -drift.y,
                sideways: DVec3::new(drift.x, 0.0, drift.z).length(),
                tipped,
                asleep: boxes.iter().all(|&e| phys.is_sleeping(e)),
            }
        })
        .collect()
}

/// **A column of twenty one-metre cubes stands for ten seconds** in a system
/// at the default settings, its cubes asking for twelve substeps — their
/// contacts at 90 Hz — its top box sunk by the contact springs' squeeze and
/// hardly moved sideways.
///
/// Twenty soft contacts in series each give under the weight above them, so
/// the top settles lower by their sum and then stays: what the test bounds is
/// that sum and any sideways creep. Twenty cubes are past the height the
/// default settings hold up — see Greenhill's height in the solver's groups
/// (`src/contact/group.rs`) and the next test. Measured on 2026-09-23 over
/// 600 ticks: the top box sank 1.18 cm and moved 1.5 mm sideways, and no box
/// tipped more than 0.11 mrad; the whole system at eight substeps and 90 Hz,
/// before groups, sank it the same 1.18 cm, 3.6 mm sideways, 0.28 mrad.
/// Asking for eight substeps (60 Hz), it sank 2.66 cm; for none, 10.6 cm.
#[test]
fn a_column_of_twenty_boxes_stands() {
    let ColumnSample {
        sunk,
        sideways,
        tipped,
        ..
    } = *column_trace(ContactSettings::DEFAULT, 20, 12, 600)
        .last()
        .expect("600 ticks");
    assert!(sideways < 5e-3, "the top box moved {sideways} m sideways");
    assert!(sunk < 0.02, "the top box sank {sunk} m under the column");
    assert!(tipped < 2e-3, "a box tipped by {tipped}");
}

/// **At the default settings a column shorter than Greenhill's height
/// stands, and falls asleep standing**: fourteen one-metre cubes, where the
/// soft contacts' arithmetic in the solver's groups (`src/contact/group.rs`)
/// puts the limit at fifteen.
///
/// With sleep on, what a late reading measures is where the column fell
/// asleep, not where the solver holds it: the column sways slowly — under the
/// 5 cm/s of [`ContactSettings::sleep_speed`] — so the island goes still half
/// a second after it settles and freezes mid-swing, and every later tick reads
/// that one frozen pose. So this bounds that pose, and that it is the pose the
/// column keeps. How the solver holds the column up while it keeps swinging is
/// `a_column_under_greenhills_height_damps_its_sway`'s, with sleep off.
///
/// Measured on 2026-09-23 at rung 5: every box asleep from tick 49, the top
/// box frozen 5.03 cm sunk and 8.52 mm sideways, no box tipped more than
/// 0.95 mrad, and the same to the bit at tick 600. At rung 2, before sleep,
/// the same test read 1.8 mm sideways at tick 600: where the swing happened
/// to be then. Seventeen cubes, over the limit, leaned 0.54 m by tick 600 and
/// lay 6.2 m away by tick 900.
#[test]
fn a_column_under_greenhills_height_stands_at_the_defaults() {
    let trace = column_trace(ContactSettings::DEFAULT, 14, 0, 600);
    let asleep_from = trace
        .iter()
        .position(|s| s.asleep)
        .expect("the column falls asleep");
    let tick = asleep_from + 1;
    assert!(tick <= 60, "the column fell asleep at tick {tick}");
    assert!(
        trace[asleep_from..].iter().all(|s| s.asleep),
        "the column woke after tick {tick}"
    );
    let frozen = trace[asleep_from];
    let last = trace.last().expect("600 ticks");
    assert_eq!(
        (last.sunk, last.sideways, last.tipped),
        (frozen.sunk, frozen.sideways, frozen.tipped),
        "the column moved in its sleep"
    );
    let ColumnSample {
        sunk,
        sideways,
        tipped,
        ..
    } = frozen;
    assert!(sideways < 1e-2, "the top box slept {sideways} m sideways");
    assert!(sunk < 0.06, "the top box slept {sunk} m sunk");
    assert!(tipped < 2e-3, "a box slept tipped by {tipped}");
}

/// **Kept awake, a column under Greenhill's height sways and the sway dies
/// away**: the fourteen cubes of the test above with sleep off, bounded over
/// the whole run rather than at one tick, since a single tick reads wherever
/// the swing happens to be.
///
/// Measured on 2026-09-23 at rung 5, over 600 ticks: the top box swings out
/// to 13.67 mm sideways at tick 120, back to within a millimetre of the
/// column's axis, out again to 6.96 mm at tick 395 — half the first swing —
/// and is at 1.79 mm at tick 600; it sinks at most 5.12 cm, and no box tips
/// more than 1.49 mrad. The first swing is the largest over the first 300 ticks and the
/// second the largest over the last 300, and the bounds are those with room:
/// 16 mm for the first, and the second under 0.6 of it.
#[test]
fn a_column_under_greenhills_height_damps_its_sway() {
    let trace = column_trace(AWAKE, 14, 0, 600);
    let most = |samples: &[ColumnSample], of: fn(&ColumnSample) -> f64| {
        samples.iter().map(of).fold(0.0, f64::max)
    };
    let (first, second) = trace.split_at(300);
    let first_swing = most(first, |s| s.sideways);
    let second_swing = most(second, |s| s.sideways);
    assert!(
        first_swing < 0.016,
        "the top box swung {first_swing} m sideways"
    );
    assert!(
        second_swing < 0.6 * first_swing,
        "the top box swung {first_swing} m and then {second_swing} m"
    );
    let sunk = most(&trace, |s| s.sunk);
    let tipped = most(&trace, |s| s.tipped);
    assert!(sunk < 0.06, "the top box sank {sunk} m under the column");
    assert!(tipped < 2e-3, "a box tipped by {tipped}");
}

/// A pyramid with `base` cubes of half-extent `half` along its bottom row,
/// every row centred on the one below, and its top box.
fn pyramid(phys: &mut PhysicsSystem, base: u32, half: f64) -> (Vec<Entity>, Entity) {
    let mut boxes = Vec::new();
    let mut index = 0;
    for row in 0..base {
        let across = base - row;
        for k in 0..across {
            let x = (f64::from(k) - 0.5 * f64::from(across - 1)) * 2.0 * half;
            let y = half + 2.0 * half * f64::from(row);
            boxes.push(crate_(
                phys,
                index,
                DVec3::new(x, y, 0.0),
                DQuat::IDENTITY,
                DVec3::splat(half),
                10.0,
                CRATE,
            ));
            index += 1;
        }
    }
    let top = *boxes.last().expect("a pyramid has a top");
    (boxes, top)
}

/// **A base-20 pyramid of one-metre cubes — 210 boxes, Box2D's and Box3D's
/// regression pyramid — holds for ten seconds at the default settings**: no
/// box leaves its place by more than the contact springs' squeeze, the top box
/// hardly moves sideways, and the feature ids hold.
///
/// Measured on 2026-09-23 over 600 ticks: the most any box moved was
/// 2.76 cm — the top box, sinking by the squeeze of the twenty rows under it —
/// and it moved 0.05 mm sideways; 4 points a manifold, and after the first
/// second no tick persisted fewer than 99.7% of its ids.
///
/// Sleep is off: this is the solver's regression test, and a sleeping pyramid
/// is not solved. `a_pyramid_settles_to_sleep` in `settling.rs` is the same
/// pyramid with sleep on.
#[test]
fn a_base_twenty_pyramid_holds() {
    let mut phys = system_with(AWAKE, CRATE);
    let (boxes, top) = pyramid(&mut phys, 20, 0.5);
    assert_eq!(boxes.len(), 210);
    let starts: Vec<DVec3> = boxes.iter().map(|&e| position(&phys, e)).collect();
    let top_start = position(&phys, top);
    let mut least_persisted = 1.0f64;
    for tick in 0..600 {
        phys.step(DT);
        if tick >= 60 {
            let ratio = phys.contact_counters().persisted_ratio();
            least_persisted = least_persisted.min(ratio.expect("the pyramid touches"));
        }
    }
    let worst = boxes
        .iter()
        .zip(&starts)
        .map(|(&e, &start)| (position(&phys, e) - start).length())
        .fold(0.0, f64::max);
    let top_drift = position(&phys, top) - top_start;
    let sideways = DVec3::new(top_drift.x, 0.0, top_drift.z).length();
    assert!(worst < 0.04, "a box moved {worst} m");
    assert!(sideways < 1e-3, "the top box moved {sideways} m sideways");
    assert!(least_persisted > 0.99, "a tick persisted {least_persisted}");
    let counters = phys.contact_counters();
    assert_eq!(counters.points_per_manifold(), Some(4.0), "{counters:?}");
}

/// **Dominoes topple in order**: the first, tipped, knocks down each of the
/// others one after the next, and all of them fall.
///
/// Measured on 2026-09-23: ten dominoes a metre tall and 0.6 m apart passed
/// 30° at ticks 44, 68, 84, 98, 111, 123, 135, 147, 159 and 171 — the wave
/// quickening to a steady 12 ticks a domino — and the last lay down.
#[test]
fn dominoes_topple_in_order() {
    const COUNT: u32 = 10;
    const SPACING: f64 = 0.6;
    let half = DVec3::new(0.05, 0.5, 0.25);
    let mut phys = system(CRATE);
    let dominoes: Vec<Entity> = (0..COUNT)
        .map(|i| {
            crate_(
                &mut phys,
                i,
                DVec3::new(SPACING * f64::from(i), half.y, 0.0),
                DQuat::IDENTITY,
                half,
                2.0,
                CRATE,
            )
        })
        .collect();
    // A flick about its bottom leading edge, towards the others.
    phys.body_mut(dominoes[0]).expect("a body").angular_velocity = DVec3::new(0.0, 0.0, -2.0);

    let mut fell_at = vec![None; dominoes.len()];
    for tick in 0..240u32 {
        phys.step(DT);
        for (k, &e) in dominoes.iter().enumerate() {
            let up = phys.transform(e).expect("a domino").rotation * DVec3::Y;
            // Past 30° from upright.
            if fell_at[k].is_none() && up.y < 0.866 {
                fell_at[k] = Some(tick);
            }
        }
    }
    let ticks: Vec<u32> = fell_at
        .iter()
        .enumerate()
        .map(|(k, t)| t.unwrap_or_else(|| panic!("domino {k} never fell: {fell_at:?}")))
        .collect();
    assert!(
        ticks.windows(2).all(|w| w[0] < w[1]),
        "out of order: {ticks:?}"
    );
    let last = phys
        .transform(dominoes[COUNT as usize - 1])
        .expect("a domino");
    assert!(
        (last.rotation * DVec3::Y).y < 0.5,
        "the last domino is still up: {last:?}"
    );
}

// ---------------------------------------------------------------------------
// Friction
// ---------------------------------------------------------------------------

/// **A box sliding on the floor stops in the distance `v² / 2μg` says.**
///
/// Measured on 2026-09-23: a box slid at 3 m/s with μ = 0.5 stopped 0.911 m
/// on, against 0.917 m.
#[test]
fn a_sliding_box_stops_where_friction_says() {
    const MU: f64 = 0.5;
    const SPEED: f64 = 3.0;
    let material = SurfaceMaterial::new(MU, 0.0);
    let mut phys = system(material);
    let half = DVec3::new(0.3, 0.1, 0.2);
    let b = crate_(
        &mut phys,
        0,
        DVec3::new(0.0, half.y, 0.0),
        DQuat::IDENTITY,
        half,
        4.0,
        material,
    );
    phys.step(DT);
    phys.body_mut(b).expect("a body").velocity = DVec3::new(SPEED, 0.0, 0.0);
    let start = position(&phys, b).x;
    for _ in 0..120 {
        phys.step(DT);
    }
    let slid = position(&phys, b).x - start;
    let analytic = SPEED * SPEED / (2.0 * MU * G);
    assert!(
        (slid - analytic).abs() < 0.03 * analytic,
        "slid {slid} m against {analytic} m"
    );
    assert!(
        phys.body(b).expect("a body").velocity.length() < 1e-3,
        "still moving"
    );
}

/// **A box spun flat on the floor is stopped by twist friction, in the time
/// its four corners' friction says.**
///
/// The manifold's friction acts at its centroid, and a box spinning about its
/// own vertical axis has no velocity there at all: without the twist term it
/// would spin forever. The twist term resists turning about the normal with
/// the torque of each point's friction at its distance from the centroid —
/// here `μ m g · h√2` for a cube of half-extent `h` on its four corners — so
/// it decelerates at `μ m g h√2 / I`, with `I = ⅔ m h²`.
///
/// Measured on 2026-09-23: spun at 10 rad/s with μ = 0.5 it stopped after
/// 0.300 s against 0.288 s, and its centre moved 0.05 mm. With the twist
/// term taken out it was still spinning after two seconds.
#[test]
fn a_box_spun_flat_is_stopped_by_twist_friction() {
    const MU: f64 = 0.5;
    const SPIN: f64 = 10.0;
    const HALF: f64 = 0.3;
    let material = SurfaceMaterial::new(MU, 0.0);
    let mut phys = system(material);
    let b = crate_(
        &mut phys,
        0,
        DVec3::new(0.0, HALF, 0.0),
        DQuat::IDENTITY,
        DVec3::splat(HALF),
        8.0,
        material,
    );
    phys.step(DT);
    phys.body_mut(b).expect("a body").angular_velocity = DVec3::new(0.0, SPIN, 0.0);
    let start = position(&phys, b);
    let mut stopped = None;
    for tick in 1..=120u32 {
        phys.step(DT);
        if stopped.is_none() && phys.body(b).expect("a body").angular_velocity.y.abs() < 0.05 {
            stopped = Some(f64::from(tick) * DT);
        }
    }
    let alpha = MU * G * HALF * core::f64::consts::SQRT_2 / (2.0 / 3.0 * HALF * HALF);
    let analytic = SPIN / alpha;
    let stopped = stopped.expect("it never stopped spinning");
    assert!(
        (stopped - analytic).abs() < 0.1 * analytic,
        "stopped after {stopped} s against {analytic} s"
    );
    let moved = (position(&phys, b) - start).length();
    assert!(moved < 1e-3, "its centre moved {moved} m");
}

// ---------------------------------------------------------------------------
// Determinism
// ---------------------------------------------------------------------------

/// A base-20 pyramid after `ticks`, hashed, with sleep off so it is still
/// being solved at every tick asked about.
fn pyramid_hash(ticks: u32) -> u64 {
    use std::hash::Hasher;
    struct Fnv(u64);
    impl Hasher for Fnv {
        fn finish(&self) -> u64 {
            self.0
        }
        fn write(&mut self, bytes: &[u8]) {
            for byte in bytes {
                self.0 = (self.0 ^ u64::from(*byte)).wrapping_mul(0x0000_0100_0000_01b3);
            }
        }
    }
    let mut phys = system_with(AWAKE, CRATE);
    pyramid(&mut phys, 20, 0.5);
    for _ in 0..ticks {
        phys.step(DT);
    }
    let mut hasher = Fnv(0xcbf2_9ce4_8422_2325);
    phys.hash_state(&mut hasher);
    hasher.finish()
}

/// **The same pyramid hashes the same twice**, and the hash moves as the
/// pyramid settles.
#[test]
fn a_pyramid_hashes_the_same_on_two_runs() {
    assert_eq!(pyramid_hash(120), pyramid_hash(120));
    assert_ne!(pyramid_hash(120), pyramid_hash(121));
}
