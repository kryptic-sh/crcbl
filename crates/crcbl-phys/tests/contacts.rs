//! The contact pipeline, run whole: rung 1 of
//! `docs/plan/36-contact-solver.md`, measured against what physics says a
//! ball, a capsule and a pair of balls should do.
//!
//! Every bound below was measured before it was written down, and each says
//! what it was measured at. They are properties of a long run of the assembled
//! pipeline — broadphase, manifolds, solver — which is why they are a target of
//! their own rather than unit tests beside any one stage.

use crcbl_ecs::Entity;
use crcbl_phys::{
    ColliderComponent, ContactBody, ContactSettings, GravityForce, KineticSource, MassProperties,
    PhysicsSystem, RigidBody, SurfaceMaterial, Transform,
};
use glam::{DQuat, DVec3};

/// The tick every test steps by: the engine's default 60 Hz.
const DT: f64 = 1.0 / 60.0;

/// Standard gravity.
const G: f64 = 9.81;

fn entity(index: u32) -> Entity {
    Entity::from_bits((1u64 << 32) | u64::from(index)).expect("generation 1 is never zero")
}

/// A system with contacts and, if asked, Earth gravity.
fn system(gravity: bool) -> PhysicsSystem {
    let mut phys = PhysicsSystem::with_contacts(ContactSettings::DEFAULT);
    if gravity {
        phys.add_force_provider(Box::new(GravityForce::EARTH));
    }
    phys
}

/// A solid ball.
fn ball(
    phys: &mut PhysicsSystem,
    index: u32,
    at: DVec3,
    radius: f64,
    mass: f64,
    material: SurfaceMaterial,
) -> Entity {
    let e = entity(index);
    let inertia = MassProperties::sphere(mass, radius, DVec3::ZERO).inertia;
    phys.set_body(e, RigidBody::new_dynamic(mass).with_inertia(inertia));
    let transform = Transform::from_position(at);
    phys.set_transform(e, transform);
    phys.set_collider(
        e,
        &ColliderComponent::Sphere {
            offset: DVec3::ZERO,
            radius,
            is_trigger: false,
        },
        &transform,
    );
    phys.set_material(e, material);
    e
}

/// A static box.
fn wall(phys: &mut PhysicsSystem, index: u32, centre: DVec3, half: DVec3) -> Entity {
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
    e
}

fn position(phys: &PhysicsSystem, e: Entity) -> DVec3 {
    phys.transform(e).expect("registered").position
}

fn velocity(phys: &PhysicsSystem, e: Entity) -> DVec3 {
    phys.body(e).expect("a body").velocity
}

/// A frictionless, bounceless surface, so a test measures one thing.
const PLAIN: SurfaceMaterial = SurfaceMaterial::new(0.0, 0.0);

// ---------------------------------------------------------------------------
// Resting
// ---------------------------------------------------------------------------

/// **A ball dropped on a plane comes to rest on it, sunk by under a tenth of
/// a millimetre.**
///
/// A soft contact is a spring, so a resting ball is sunk by its weight over the
/// spring's stiffness: a contact with a static body is stepped at twice
/// [`ContactSettings::contact_hertz`], which for 1 kg is `(2π · 60 Hz)² ≈
/// 142 kN/m`, and 9.81 N sinks it 0.069 mm. Measured on 2026-09-17: a 10 cm,
/// 1 kg ball dropped a metre is sunk by exactly that from its second second
/// on, and not moving at all.
///
/// Sleep is off: the counter is checked against the ball on the last tick,
/// and a sleeping ball's contact is not collided, so counts nothing.
#[test]
fn a_ball_dropped_on_a_plane_comes_to_rest_sunk_under_a_tenth_of_a_millimetre() {
    const RADIUS: f64 = 0.1;
    let mut phys = PhysicsSystem::with_contacts(ContactSettings {
        sleep: false,
        ..ContactSettings::DEFAULT
    });
    phys.add_force_provider(Box::new(GravityForce::EARTH));
    phys.add_plane(DVec3::Y, 0.0, PLAIN);
    let b = ball(
        &mut phys,
        0,
        DVec3::new(0.0, 1.0 + RADIUS, 0.0),
        RADIUS,
        1.0,
        PLAIN,
    );

    let mut worst = 0.0f64;
    for tick in 0..180 {
        phys.step(DT);
        if tick >= 60 {
            worst = worst.max(RADIUS - position(&phys, b).y);
        }
    }
    const BOUND: f64 = 1e-4;
    let rest = RADIUS - position(&phys, b).y;
    let speed = velocity(&phys, b).length();
    assert!(
        worst <= BOUND,
        "sunk {worst} m into the plane after landing"
    );
    assert!(rest > 0.0, "floating {} m above the plane", -rest);
    assert!(speed < 1e-6, "still moving at {speed} m/s");
    let counted = phys.contact_counters().worst_penetration;
    assert!(
        (counted - rest).abs() < 1e-9,
        "the counter says {counted} m where the ball is sunk {rest} m"
    );
}

// ---------------------------------------------------------------------------
// Restitution
// ---------------------------------------------------------------------------

/// The highest a ball dropped from `height` climbs after its first bounce.
fn bounce_height(restitution: f64, height: f64) -> f64 {
    const RADIUS: f64 = 0.1;
    let mut phys = system(true);
    let material = SurfaceMaterial::new(0.0, restitution);
    phys.add_plane(DVec3::Y, 0.0, material);
    let b = ball(
        &mut phys,
        0,
        DVec3::new(0.0, height + RADIUS, 0.0),
        RADIUS,
        1.0,
        material,
    );
    let mut landed = false;
    let mut highest = 0.0f64;
    for _ in 0..240 {
        phys.step(DT);
        let v = velocity(&phys, b).y;
        if !landed && v > 0.0 {
            landed = true;
        }
        if landed {
            highest = highest.max(position(&phys, b).y - RADIUS);
            if v < 0.0 && position(&phys, b).y - RADIUS > 0.05 {
                break;
            }
        }
    }
    highest
}

/// **Restitution 1 bounces back to nearly the height it fell from, and
/// restitution 0 does not bounce at all.**
///
/// Measured on 2026-09-17 from a 2 m drop: restitution 1 climbs back to
/// 1.95 m, restitution 0.5 to 0.49 m — a quarter of the drop, as `e²` says —
/// and restitution 0 not at all.
#[test]
fn restitution_one_bounces_near_the_drop_height_and_zero_not_at_all() {
    const DROP: f64 = 2.0;
    let elastic = bounce_height(1.0, DROP);
    let half = bounce_height(0.5, DROP);
    let dead = bounce_height(0.0, DROP);
    assert!(
        elastic > 0.9 * DROP && elastic <= 1.0 * DROP,
        "restitution 1 climbed to {elastic} m of {DROP}"
    );
    assert!(
        (half - 0.25 * DROP).abs() < 0.05 * DROP,
        "restitution 0.5 climbed to {half} m, not a quarter of {DROP}"
    );
    assert!(dead < 1e-4, "restitution 0 climbed to {dead} m");
}

// ---------------------------------------------------------------------------
// Warm starting
// ---------------------------------------------------------------------------

/// A column of balls stacked exactly on a plane, and how far the bottom of it
/// is sunk once it has had `ticks` to settle.
fn column_sink(warm_starting: bool, ticks: u32) -> f64 {
    const RADIUS: f64 = 0.1;
    const BALLS: u32 = 10;
    let mut phys = PhysicsSystem::with_contacts(ContactSettings {
        warm_starting,
        ..ContactSettings::DEFAULT
    });
    phys.add_force_provider(Box::new(GravityForce::EARTH));
    phys.add_plane(DVec3::Y, 0.0, PLAIN);
    let balls: Vec<Entity> = (0..BALLS)
        .map(|i| {
            let y = RADIUS + 2.0 * RADIUS * f64::from(i);
            ball(&mut phys, i, DVec3::new(0.0, y, 0.0), RADIUS, 1.0, PLAIN)
        })
        .collect();
    for _ in 0..ticks {
        phys.step(DT);
    }
    let top = position(&phys, balls[BALLS as usize - 1]).y;
    RADIUS + 2.0 * RADIUS * f64::from(BALLS - 1) - top
}

/// **Warm starting holds a stack up: a column of ten balls sinks far less with
/// it than without.**
///
/// The column is the case warm starting exists for: each substep's single
/// biased pass carries an impulse only one ball up the column, so without last
/// tick's impulses to start from the column's weight never reaches the bottom
/// contact within a tick and the column squashes. Measured on 2026-09-17 after
/// two seconds: the top ball sat 2.6 cm low with warm starting — the ten
/// contact springs compressed under the weight above each — and 9.4 cm low
/// without.
#[test]
fn warm_starting_holds_a_column_of_balls_up() {
    let warm = column_sink(true, 120);
    let cold = column_sink(false, 120);
    assert!(
        warm * 3.0 < cold,
        "the column sank {warm} m warm and {cold} m cold"
    );
}

// ---------------------------------------------------------------------------
// Speculative contacts
// ---------------------------------------------------------------------------

/// **A ball faster than its own diameter a tick does not pass through a thin
/// static plate, whatever the phase it meets it at.**
///
/// Nothing sweeps at rung 1, so what stops it is the speculative contact: the
/// pair's speculative distance grows with its speed, so the contact exists the
/// tick before the ball would cross. The plate is 2 cm thick and the ball 5 cm
/// in radius; at 30 m/s it moves 50 cm a tick, ten times its diameter. The
/// wall scene's fastest ball is under 12 m/s.
#[test]
fn a_fast_ball_does_not_tunnel_through_a_thin_plate() {
    const RADIUS: f64 = 0.05;
    const SPEED: f64 = 30.0;
    for phase in 0..10 {
        let mut phys = system(false);
        wall(
            &mut phys,
            100,
            DVec3::new(2.0, 0.0, 0.0),
            DVec3::new(0.01, 1.0, 1.0),
        );
        let start = DVec3::new(f64::from(phase) * 0.05, 0.0, 0.0);
        let b = ball(&mut phys, 0, start, RADIUS, 1.0, PLAIN);
        phys.body_mut(b).expect("a body").velocity = DVec3::new(SPEED, 0.0, 0.0);
        let mut furthest = f64::NEG_INFINITY;
        for _ in 0..30 {
            phys.step(DT);
            furthest = furthest.max(position(&phys, b).x);
        }
        assert!(
            furthest <= 2.0 - 0.01 - RADIUS + 1e-3,
            "phase {phase}: the ball reached x = {furthest} through a plate at 2 m"
        );
    }
}

// ---------------------------------------------------------------------------
// Friction
// ---------------------------------------------------------------------------

/// A plane through the origin rising at slope `rise` (a tangent), falling away
/// along +X, and the unit vector down it.
fn slope(rise: f64) -> (DVec3, DVec3) {
    let length = (1.0 + rise * rise).sqrt();
    (
        DVec3::new(rise, 1.0, 0.0) / length,
        DVec3::new(1.0, -rise, 0.0) / length,
    )
}

/// How far a capsule laid down a slope of tangent `rise`, pointing downhill,
/// slides in two seconds with friction `mu` on both surfaces.
fn capsule_slide(mu: f64, rise: f64) -> f64 {
    const RADIUS: f64 = 0.1;
    const HALF: f64 = 0.3;
    let mut phys = system(true);
    let material = SurfaceMaterial::new(mu, 0.0);
    let (normal, down) = slope(rise);
    phys.add_plane(normal, 0.0, material);

    let e = entity(0);
    let mass = 2.0;
    let inertia = MassProperties::capsule(mass, RADIUS, HALF, DVec3::ZERO).inertia;
    phys.set_body(e, RigidBody::new_dynamic(mass).with_inertia(inertia));
    // The capsule's own Y axis turned onto the downhill direction.
    let turn =
        DQuat::from_xyzw(0.0, 0.0, DVec3::Y.cross(down).z, 1.0 + DVec3::Y.dot(down)).normalize();
    let start = normal * RADIUS;
    let transform = Transform::new(start, turn);
    phys.set_transform(e, transform);
    phys.set_collider(
        e,
        &ColliderComponent::Capsule {
            offset: DVec3::ZERO,
            radius: RADIUS,
            half_height: HALF,
            is_trigger: false,
        },
        &transform,
    );
    phys.set_material(e, material);
    for _ in 0..120 {
        phys.step(DT);
    }
    (position(&phys, e) - start).dot(down)
}

/// **Friction holds a capsule on a slope below the friction angle and lets it
/// slide above it.**
///
/// A capsule laid pointing downhill cannot roll, so it stays put while
/// `tan θ ≤ μ` and slides at `g (sin θ − μ cos θ)` beyond. Measured on
/// 2026-09-17 with μ = 0.5, over two seconds: at a slope of 0.4 it crept
/// 0.03 mm, and at 0.6 it slid 1.688 m, where the analytic answer is 1.682 m.
#[test]
fn friction_holds_a_capsule_below_the_friction_angle_and_not_above() {
    const MU: f64 = 0.5;
    let below = capsule_slide(MU, 0.8 * MU);
    let above = capsule_slide(MU, 1.2 * MU);
    let rise: f64 = 1.2 * MU;
    let cos = 1.0 / (1.0 + rise * rise).sqrt();
    let analytic = 0.5 * G * (rise * cos - MU * cos) * 2.0 * 2.0;
    assert!(below.abs() < 0.005, "below the angle it slid {below} m");
    assert!(
        (above - analytic).abs() < 0.1 * analytic,
        "above the angle it slid {above} m, against {analytic} m"
    );
}

/// How fast a ball's contact point is slipping after rolling two seconds down
/// a slope of tangent `rise`, and how far down it went.
fn ball_roll(mu: f64, rise: f64) -> (f64, f64) {
    const RADIUS: f64 = 0.1;
    let mut phys = system(true);
    let material = SurfaceMaterial::new(mu, 0.0);
    let (normal, down) = slope(rise);
    phys.add_plane(normal, 0.0, material);
    let start = normal * RADIUS;
    let b = ball(&mut phys, 0, start, RADIUS, 1.0, material);
    for _ in 0..120 {
        phys.step(DT);
    }
    let body = phys.body(b).expect("a body");
    let slip = (body.velocity + body.angular_velocity.cross(-normal * RADIUS)).length();
    (slip, (position(&phys, b) - start).dot(down))
}

/// **A ball on a slope rolls whatever the friction — and friction decides only
/// whether it rolls without slipping.**
///
/// A solid ball's rolling needs a friction force of `(2/7) m g sin θ`, which
/// friction can supply while `tan θ ≤ (7/2) μ`. So a ball is never held on a
/// slope by friction, and the test of friction on a ball is the slip: none
/// below that angle, and a growing slip above it. Measured on 2026-09-17 with
/// μ = 0.1: at a slope of 0.28 the contact point slipped at 1.3 mm/s and the
/// ball travelled `(5/7) g sin θ t² / 2` to within 0.2%, and at 0.42 it
/// slipped at 1.27 m/s.
#[test]
fn a_ball_rolls_without_slipping_only_below_its_rolling_angle() {
    const MU: f64 = 0.1;
    let rise = 0.8 * 3.5 * MU;
    let (slip_below, travel) = ball_roll(MU, rise);
    let (slip_above, _) = ball_roll(MU, 1.2 * 3.5 * MU);
    let sin = rise / (1.0 + rise * rise).sqrt();
    let rolling = 0.5 * (5.0 / 7.0) * G * sin * 2.0 * 2.0;
    assert!(
        slip_below < 0.01,
        "below the angle it slipped at {slip_below} m/s"
    );
    assert!(
        (travel - rolling).abs() < 0.02 * rolling,
        "it rolled {travel} m, against {rolling} m"
    );
    assert!(
        slip_above > 0.5,
        "above the angle it slipped at only {slip_above} m/s"
    );
}

// ---------------------------------------------------------------------------
// Persistence
// ---------------------------------------------------------------------------

/// **The pairs, the contacts' slots and their feature ids survive a frame in
/// which nothing moved, and nothing begins or ends.**
///
/// Three balls in zero gravity touching in a row and a fourth touching a
/// static box, none of them moving.
#[test]
fn pairs_and_feature_ids_survive_a_frame_where_nothing_moved() {
    const RADIUS: f64 = 0.1;
    let mut phys = system(false);
    for i in 0..3 {
        ball(
            &mut phys,
            i,
            DVec3::new(2.0 * RADIUS * f64::from(i), 0.0, 0.0),
            RADIUS,
            1.0,
            PLAIN,
        );
    }
    ball(&mut phys, 3, DVec3::new(0.0, 1.0, 0.0), RADIUS, 1.0, PLAIN);
    wall(
        &mut phys,
        10,
        DVec3::new(0.0, 1.0 + RADIUS + 0.5, 0.0),
        DVec3::splat(0.5),
    );

    phys.step(DT);
    let first = phys.contacts();
    let counters = phys.contact_counters();
    assert_eq!(counters.touching, 3, "{counters:?}");
    assert_eq!(counters.begun, 3, "{counters:?}");

    phys.step(DT);
    let second = phys.contacts();
    let counters = phys.contact_counters();
    let key = |reports: &[crcbl_phys::ContactReport]| {
        reports
            .iter()
            .map(|r| {
                (
                    r.slot,
                    r.a,
                    r.b,
                    r.manifold.points().iter().map(|p| p.id).collect::<Vec<_>>(),
                )
            })
            .collect::<Vec<_>>()
    };
    assert_eq!(key(&first), key(&second));
    assert_eq!(counters.begun, 0, "{counters:?}");
    assert_eq!(counters.ended, 0, "{counters:?}");
    assert_eq!(counters.persisted, counters.points, "{counters:?}");
    assert!(
        first
            .iter()
            .any(|r| matches!(r.b, ContactBody::Entity(e) if e == entity(10))),
        "the static box's contact is among them: {first:?}"
    );
    for (a, b) in first.iter().zip(&second) {
        for point in 0..a.manifold.points().len() {
            assert_eq!(
                a.manifold.points()[point].point,
                b.manifold.points()[point].point,
                "a point moved in a frame where nothing did"
            );
        }
    }
}

/// **Taking a body out ends its contacts, and the next step counts them as
/// ended** — once, though the body went between steps.
#[test]
fn removing_a_body_ends_its_contacts_and_counts_them() {
    const RADIUS: f64 = 0.1;
    let mut phys = system(false);
    let a = ball(&mut phys, 0, DVec3::ZERO, RADIUS, 1.0, PLAIN);
    ball(&mut phys, 1, DVec3::new(0.2, 0.0, 0.0), RADIUS, 1.0, PLAIN);
    phys.step(DT);
    assert_eq!(phys.contact_counters().touching, 1);
    phys.remove_entity(a);
    phys.step(DT);
    let counters = phys.contact_counters();
    assert_eq!(counters.pairs, 0, "{counters:?}");
    assert_eq!(counters.ended, 1, "{counters:?}");
    assert!(phys.contacts().is_empty());
    phys.step(DT);
    assert_eq!(phys.contact_counters().ended, 0, "counted twice");
}

// ---------------------------------------------------------------------------
// Momentum
// ---------------------------------------------------------------------------

/// **Momentum is conserved through a head-on collision of two unequal balls**,
/// to within 1e-12 of its size, bouncy or not.
#[test]
fn a_head_on_collision_conserves_momentum() {
    const RADIUS: f64 = 0.1;
    for restitution in [0.0, 0.5, 1.0] {
        let material = SurfaceMaterial::new(0.3, restitution);
        let mut phys = system(false);
        let a = ball(
            &mut phys,
            0,
            DVec3::new(-1.0, 0.0, 0.0),
            RADIUS,
            1.0,
            material,
        );
        let b = ball(
            &mut phys,
            1,
            DVec3::new(1.0, 0.0, 0.0),
            RADIUS,
            3.0,
            material,
        );
        phys.body_mut(a).expect("a").velocity = DVec3::new(4.0, 0.0, 0.0);
        phys.body_mut(b).expect("b").velocity = DVec3::new(-2.0, 0.0, 0.0);
        let momentum = |phys: &PhysicsSystem| velocity(phys, a) * 1.0 + velocity(phys, b) * 3.0;
        let before = momentum(&phys);
        let mut met = false;
        for _ in 0..60 {
            phys.step(DT);
            met |= phys.contact_counters().touching > 0;
        }
        let after = momentum(&phys);
        let (va, vb) = (velocity(&phys, a).x, velocity(&phys, b).x);
        assert!(met, "the balls never met");
        assert!(
            (after - before).length() <= 1e-12 * before.length(),
            "restitution {restitution}: momentum went {before:?} → {after:?}"
        );
        // The analytic separation speed, and the balls actually parted.
        let parted = vb - va;
        assert!(
            (parted - restitution * 6.0).abs() < 0.02 * 6.0,
            "restitution {restitution}: they part at {parted} m/s, not {}",
            restitution * 6.0
        );
    }
}

// ---------------------------------------------------------------------------
// Events
// ---------------------------------------------------------------------------

/// **A ball landing hard raises one `KineticContact`** naming it as struck and
/// the plane as the impactor, with the impulse and the energy a dead landing
/// takes out of it.
#[test]
fn a_hard_landing_raises_a_kinetic_contact_with_its_impulse_and_energy() {
    const RADIUS: f64 = 0.1;
    const SPEED: f64 = 5.0;
    let mut phys = system(false);
    phys.add_plane(DVec3::Y, 0.0, PLAIN);
    let b = ball(
        &mut phys,
        0,
        DVec3::new(0.0, RADIUS + 0.05, 0.0),
        RADIUS,
        2.0,
        PLAIN,
    );
    phys.body_mut(b).expect("a body").velocity = DVec3::new(0.0, -SPEED, 0.0);
    let mut events = Vec::new();
    for _ in 0..10 {
        phys.step(DT);
        events.extend_from_slice(phys.kinetic_contacts());
    }
    assert_eq!(events.len(), 1, "{events:?}");
    let event = events[0];
    assert_eq!(event.source, KineticSource::Contact);
    assert_eq!(event.struck, b);
    assert_eq!(event.impactor, None);
    assert!(event.impactor_mass.is_infinite());
    // From the impactor, the plane, up into the ball it struck.
    assert!((event.normal - DVec3::Y).length() < 1e-12, "{event:?}");
    assert!(
        (event.impulse - 2.0 * SPEED).abs() < 0.05 * 2.0 * SPEED,
        "{event:?}"
    );
    assert!(
        (event.energy_deposited - 0.5 * 2.0 * SPEED * SPEED).abs() < 0.05 * 25.0,
        "{event:?}"
    );
}

// ---------------------------------------------------------------------------
// Determinism and the untouched path
// ---------------------------------------------------------------------------

/// A pile of balls poured into a box, hashed.
fn pour(ticks: u32) -> u64 {
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
    use crcbl_ecs::SystemTrait as _;

    let mut phys = system(true);
    let material = SurfaceMaterial::new(0.4, 0.3);
    phys.add_plane(DVec3::Y, 0.0, material);
    for (i, x) in [-1.1, 1.1].into_iter().enumerate() {
        wall(
            &mut phys,
            1000 + i as u32,
            DVec3::new(x, 1.0, 0.0),
            DVec3::new(0.1, 1.0, 1.2),
        );
        wall(
            &mut phys,
            1010 + i as u32,
            DVec3::new(0.0, 1.0, x),
            DVec3::new(1.2, 1.0, 0.1),
        );
    }
    let mut next = 0;
    for tick in 0..ticks {
        if tick % 3 == 0 && next < 60 {
            let x = f64::from(next % 7) * 0.13 - 0.4;
            let z = f64::from(next % 5) * 0.11 - 0.2;
            ball(&mut phys, next, DVec3::new(x, 2.5, z), 0.1, 1.0, material);
            next += 1;
        }
        phys.step(DT);
    }
    let mut hasher = Fnv(0xcbf2_9ce4_8422_2325);
    phys.hash_state(&mut hasher);
    hasher.finish()
}

/// **The same pour hashes the same twice**, and the hash moves as the pile
/// does.
#[test]
fn a_pour_hashes_the_same_on_two_runs() {
    assert_eq!(pour(240), pour(240));
    assert_ne!(pour(240), pour(241));
}

/// **A body that touches nothing steps exactly as the same substeps without
/// contacts would**, to the bit: a system with contacts changes nothing for a
/// body with no contact to make.
#[test]
fn a_body_touching_nothing_steps_as_it_would_without_contacts() {
    let spin = |phys: &mut PhysicsSystem| {
        let e = entity(0);
        let inertia = MassProperties::cuboid(3.0, DVec3::new(0.4, 0.1, 0.05), DVec3::ZERO).inertia;
        let mut body = RigidBody::new_dynamic(3.0).with_inertia(inertia);
        body.angular_velocity = DVec3::new(0.1, 5.0, 0.2);
        body.velocity = DVec3::new(1.0, 2.0, 3.0);
        phys.set_body(e, body);
        phys.set_transform(e, Transform::from_position(DVec3::new(0.0, 5.0, 0.0)));
        e
    };
    let mut with = system(true);
    let a = spin(&mut with);
    let mut without = PhysicsSystem::new();
    without.add_force_provider(Box::new(GravityForce::EARTH));
    let b = spin(&mut without);
    let substeps = ContactSettings::DEFAULT.substeps;
    for _ in 0..240 {
        with.step(DT);
        for _ in 0..substeps {
            without.step(DT / f64::from(substeps));
        }
    }
    assert_eq!(with.transform(a), without.transform(b));
    assert_eq!(with.body(a), without.body(b));
}

// ---------------------------------------------------------------------------
// Spinning on a point
// ---------------------------------------------------------------------------

/// **A ball spinning about its contact normal slows to rest at the rate its
/// contact patch predicts, and a frictionless one spins on.**
///
/// A one-point contact twists against a patch of Hertz radius `a = √(R δ)`,
/// with a torque of up to `μ m g a` for a ball resting under its weight, so a
/// solid ball's spin `ω₀` falls linearly to rest in `T = ω₀ · ⅖ R² / (μ g a)`.
/// The depth `δ` is read off the contact the solver built. Measured on
/// 2026-09-23: a 10 cm, 1 kg ball sunk 0.069 mm has a 2.6 mm patch, and at
/// `μ = 0.5` its 5 rad/s is predicted to stop in 1.552 s and stops on the tick
/// at 1.550 s; the bound, 3%, is about three ticks. Without the patch, twist
/// acted only in manifolds of two points or more, and the ball spun at
/// 5 rad/s for ever.
#[test]
fn a_ball_spinning_on_its_contact_point_stops_as_its_patch_predicts() {
    const RADIUS: f64 = 0.1;
    const SPIN: f64 = 5.0;
    let run = |friction: f64| {
        let mut phys = PhysicsSystem::with_contacts(ContactSettings {
            sleep: false,
            ..ContactSettings::DEFAULT
        });
        phys.add_force_provider(Box::new(GravityForce::EARTH));
        let surface = SurfaceMaterial::new(friction, 0.0);
        phys.add_plane(DVec3::Y, 0.0, surface);
        let e = ball(
            &mut phys,
            1,
            DVec3::new(0.0, RADIUS, 0.0),
            RADIUS,
            1.0,
            surface,
        );
        for _ in 0..120 {
            phys.step(DT);
        }
        let depth = -phys.contacts()[0].manifold.points()[0].separation;
        phys.body_mut(e).expect("a body").angular_velocity = DVec3::Y * SPIN;
        let mut stopped = None;
        for tick in 1..=600u32 {
            phys.step(DT);
            let spin = phys.body(e).expect("a body").angular_velocity.y;
            if stopped.is_none() && spin.abs() < 0.01 * SPIN {
                stopped = Some(f64::from(tick) * DT);
            }
        }
        let spin = phys.body(e).expect("a body").angular_velocity.y;
        (depth, stopped, spin)
    };

    let friction = 0.5;
    let (depth, stopped, _) = run(friction);
    let mu = SurfaceMaterial::new(friction, 0.0)
        .combine(&SurfaceMaterial::new(friction, 0.0))
        .friction;
    let patch = (RADIUS * depth).sqrt();
    assert!(
        patch > 1.0e-3,
        "the patch {patch} m is the clamp's, not Hertz's"
    );
    let predicted = SPIN * 0.4 * RADIUS * RADIUS / (mu * G * patch);
    let stopped = stopped.expect("the ball never stopped spinning");
    assert!(
        (stopped - predicted).abs() < 0.03 * predicted,
        "stopped after {stopped} s against the {predicted} s a {patch} m patch predicts"
    );

    let (_, stopped, spin) = run(0.0);
    assert_eq!(stopped, None, "a frictionless ball stopped spinning");
    assert!(
        (spin - SPIN).abs() < 1e-9,
        "a frictionless ball's spin changed to {spin}"
    );
}
