//! A turned box let go over a static floor, run whole: it lands on a corner,
//! tips onto a face and rests there, never gaining energy — and a box with its
//! rotation locked lands and keeps the orientation it started in.
//!
//! The floor is a static box, so every contact here is the box pair's
//! separating axis test (`contact::manifold`'s `box_box`), whose known values
//! are unit tests beside it. As in `stacking.rs`, every bound was measured
//! before it was written down, and each says what it was measured at.

use crcbl_ecs::{Entity, SystemTrait as _};
use crcbl_phys::{
    ColliderComponent, ContactSettings, GravityForce, MassProperties, PhysicsSystem, RigidBody,
    SurfaceMaterial, Transform, rotation_from_scaled_axis,
};
use glam::{DQuat, DVec3};

/// The tick every test steps by: the engine's default 60 Hz.
const DT: f64 = 1.0 / 60.0;

/// Standard gravity, as [`GravityForce::EARTH`] pulls.
const G: f64 = 9.81;

/// Friction enough that a landing corner grips and tips the box rather than
/// skating, and no bounce.
const SURFACE: SurfaceMaterial = SurfaceMaterial::new(0.6, 0.0);

/// The dropped cube's half extent and mass.
const HALF: f64 = 0.5;
const MASS: f64 = 2.0;

/// How the cube is turned when it is let go: about no axis of its own, so a
/// corner is lowest and lands first.
fn corner_down() -> DQuat {
    rotation_from_scaled_axis(DVec3::new(0.5, 0.3, 0.7))
}

/// How high the cube's centre starts: its lowest corner half a metre over the
/// floor's top at `y = 0`.
fn start_height(rotation: DQuat) -> f64 {
    let reach = (0..8)
        .map(|corner| {
            let sign = |bit: u32| if corner & (1 << bit) == 0 { -1.0 } else { 1.0 };
            (rotation * DVec3::new(sign(0), sign(1), sign(2)) * HALF).y
        })
        .fold(f64::INFINITY, f64::min);
    0.5 - reach
}

fn entity(index: u32) -> Entity {
    Entity::from_bits((1u64 << 32) | u64::from(index)).expect("generation 1 is never zero")
}

/// A system with contacts and Earth gravity over a static slab whose top is
/// `y = 0`, and the cube over it at `rotation` — tumbling if `turns`, its
/// rotation locked if not. Returns the system and the cube.
fn drop(rotation: DQuat, turns: bool) -> (PhysicsSystem, Entity) {
    let mut phys = PhysicsSystem::with_contacts(ContactSettings::DEFAULT);
    phys.add_force_provider(Box::new(GravityForce::EARTH));

    let floor = entity(0);
    let at = Transform::from_position(DVec3::new(0.0, -0.5, 0.0));
    phys.set_transform(floor, at);
    phys.set_collider(floor, &box_of(DVec3::new(8.0, 0.5, 8.0)), &at);
    phys.set_material(floor, SURFACE);

    let cube = entity(1);
    let body = RigidBody::new_dynamic(MASS);
    let body = if turns {
        body.with_inertia(MassProperties::of_collider(&box_of(DVec3::splat(HALF)), MASS).inertia)
    } else {
        body
    };
    let at = Transform::new(DVec3::new(0.0, start_height(rotation), 0.0), rotation);
    phys.set_body(cube, body);
    phys.set_transform(cube, at);
    phys.set_collider(cube, &box_of(DVec3::splat(HALF)), &at);
    phys.set_material(cube, SURFACE);
    (phys, cube)
}

fn box_of(half_extents: DVec3) -> ColliderComponent {
    ColliderComponent::Box {
        offset: DVec3::ZERO,
        half_extents,
        is_trigger: false,
    }
}

/// The cube's kinetic energy and its potential energy above the floor.
fn energy(phys: &PhysicsSystem, cube: Entity) -> f64 {
    let body = phys.body(cube).expect("a body");
    let at = phys.transform(cube).expect("a transform");
    body.kinetic_energy(at.rotation) + MASS * G * at.position.y
}

/// How far from lying on a face the cube is: one less the largest `|y|` of
/// its own axes, zero exactly when one of them stands upright.
fn off_face(rotation: DQuat) -> f64 {
    let up = [DVec3::X, DVec3::Y, DVec3::Z]
        .map(|axis| (rotation * axis).y.abs())
        .into_iter()
        .fold(0.0, f64::max);
    1.0 - up
}

/// How long a drop runs: long enough to land, tip and settle.
const SECONDS: f64 = 4.0;

#[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)]
fn ticks() -> u32 {
    (SECONDS / DT).round() as u32
}

/// **A cube dropped on its corner tips over and comes to rest on a face**,
/// its centre a half extent over the floor, and the landing and the tipping
/// put no energy into it: its energy never climbs back above the lowest it
/// has been by more than the soft contact's settling.
///
/// Measured on 2026-10-01: the cube started `3.2e-1` from a face (one less
/// its most upright axis), ended `5.4e-12` from one with its centre at
/// `0.49993`, and its energy rose at most `2.5e-4` J above its lowest so far.
/// A solver that kept the soft contact's push-out velocity (no relax pass)
/// rose `1.4e-3` J, and one whose contact impulses were half again too strong
/// `1.5e-3` J.
#[test]
fn a_cube_dropped_on_its_corner_tips_over_and_rests_on_a_face() {
    /// How far from a face the rested cube may be: a few hundredths of a
    /// degree.
    const FACE_BOUND: f64 = 1e-4;
    /// How far off a half extent over the floor its centre may rest.
    const REST_BOUND: f64 = 0.01;
    /// How far, in joules, the energy may climb back above its lowest so far:
    /// twice the settling measured.
    const RISE_BOUND: f64 = 5e-4;

    let (mut phys, cube) = drop(corner_down(), true);
    assert!(off_face(corner_down()) > 0.1, "it starts on a face");
    let mut lowest = energy(&phys, cube);
    let mut rise = 0.0f64;
    for _ in 0..ticks() {
        phys.step(DT);
        let now = energy(&phys, cube);
        lowest = lowest.min(now);
        rise = rise.max(now - lowest);
    }
    let rested = *phys.transform(cube).expect("a transform");
    assert!(
        off_face(rested.rotation) < FACE_BOUND,
        "rested {} off a face: {rested:?}",
        off_face(rested.rotation),
    );
    assert!(
        (rested.position.y - HALF).abs() < REST_BOUND,
        "rested at {}, not a half extent up",
        rested.position.y,
    );
    assert!(rise <= RISE_BOUND, "its energy climbed {rise} J");
}

/// **A cube whose rotation is locked lands on its corner and stays turned**:
/// with no inertia, nothing the contacts do turns it, so its orientation is
/// the one it was let go at, to the bit, at every tick.
#[test]
fn a_locked_cube_keeps_its_orientation_through_the_landing() {
    let (mut phys, cube) = drop(corner_down(), false);
    for tick in 0..ticks() {
        phys.step(DT);
        let at = phys.transform(cube).expect("a transform");
        assert_eq!(at.rotation, corner_down(), "turned at tick {tick}");
    }
    let body = phys.body(cube).expect("a body");
    assert_eq!(body.angular_velocity, DVec3::ZERO);
    let landed = phys.transform(cube).expect("a transform").position.y;
    assert!(
        landed < start_height(corner_down()) - 0.4,
        "it never landed: {landed}"
    );
}

/// **A box tumbling free in a system with contacts keeps its angular
/// momentum**: touching nothing, the contact pipeline's substeps and rotation
/// cap leave the gyroscopic integration alone.
///
/// Measured on 2026-10-01: a ten-second tumble drifted `5.2e-13` in momentum,
/// relatively.
#[test]
fn a_box_tumbling_free_among_contacts_keeps_its_angular_momentum() {
    const MOMENTUM_BOUND: f64 = 1e-10;
    let half = DVec3::new(0.2, 0.5, 0.8);
    let mut phys = PhysicsSystem::with_contacts(ContactSettings::DEFAULT);
    let e = entity(0);
    let mut body = RigidBody::new_dynamic(MASS)
        .with_inertia(MassProperties::of_collider(&box_of(half), MASS).inertia);
    // Mostly about the intermediate axis, so it flips as it tumbles.
    body.angular_velocity = DVec3::new(0.05, 4.0, 0.03);
    phys.set_body(e, body);
    phys.set_transform(e, Transform::IDENTITY);
    phys.set_collider(e, &box_of(half), &Transform::IDENTITY);

    let momentum = body.angular_momentum(DQuat::IDENTITY);
    let mut drift = 0.0f64;
    let mut flipped = false;
    #[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)]
    for _ in 0..(10.0 / DT).round() as u32 {
        phys.step(DT);
        let body = phys.body(e).expect("a body");
        let at = phys.transform(e).expect("a transform");
        let now = body.angular_momentum(at.rotation);
        drift = drift.max((now - momentum).length() / momentum.length());
        flipped |= (at.rotation * DVec3::Y).y < -0.9;
    }
    assert!(flipped, "it never tumbled");
    assert!(
        drift <= MOMENTUM_BOUND,
        "angular momentum drifted {drift:e}"
    );
}

/// The tipping drop after `ticks`, hashed.
fn tipping_hash(ticks: u32) -> u64 {
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
    let (mut phys, _) = drop(corner_down(), true);
    for _ in 0..ticks {
        phys.step(DT);
    }
    let mut hasher = Fnv(0xcbf2_9ce4_8422_2325);
    phys.hash_state(&mut hasher);
    hasher.finish()
}

/// **The same tipping drop hashes the same on two runs**, and the hash moves
/// while the cube is still tipping.
#[test]
fn a_tipping_drop_hashes_the_same_on_two_runs() {
    assert_eq!(tipping_hash(90), tipping_hash(90));
    assert_ne!(tipping_hash(90), tipping_hash(91));
}
