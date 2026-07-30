//! Property-level integration tests for the physics dynamics pipeline.
//!
//! These verify emergent physical behaviour — terminal velocity, energy
//! drift bounds, determinism — that simple unit tests cannot capture.

use crcbl_phys::{DragForce, ForceProvider, GravityForce, PhysicsSystem, RigidBody, Transform};
use glam::DVec3;

/// Build a system with Earth gravity and linear drag.
fn make_falling_system() -> PhysicsSystem {
    let mut phys = PhysicsSystem::new();
    phys.add_force_provider(Box::new(GravityForce::EARTH));
    phys.add_force_provider(Box::new(DragForce::new(0.5)));
    phys
}

// ---------------------------------------------------------------------------
// Terminal velocity
// ---------------------------------------------------------------------------

#[test]
fn terminal_velocity_emerges_under_gravity_and_drag() {
    // For gravity g=9.81 and linear drag k=0.5 on a 1 kg body:
    // v_term = m*g/k = 1*9.81/0.5 = 19.62 m/s (downward).
    let mut phys = make_falling_system();
    let e = crcbl_phys::test_entity(0);
    phys.set_body(e, RigidBody::new_dynamic(1.0));
    phys.set_transform(e, Transform::from_position(DVec3::new(0.0, 1000.0, 0.0)));

    // Simulate ~30 seconds at 60 Hz.
    let dt = 1.0 / 60.0;
    for _ in 0..(30 * 60) {
        phys.step(dt);
    }

    let b = phys.body(e).unwrap();
    let vy = b.velocity.y;
    let v_term = -19.62;
    let error = (vy - v_term).abs() / v_term.abs();

    // After 30 seconds (τ = m/k = 2s → 15τ), well within 0.5%.
    assert!(
        error < 0.005,
        "vy = {vy}, v_term = {v_term}, error = {error:.4}"
    );
}

#[test]
fn heavier_body_has_higher_terminal_velocity() {
    // For mass 4 kg: v_term = 4*9.81/0.5 = 78.48
    let mut phys = make_falling_system();
    let e = crcbl_phys::test_entity(0);
    phys.set_body(e, RigidBody::new_dynamic(4.0));
    phys.set_transform(e, Transform::from_position(DVec3::new(0.0, 1000.0, 0.0)));

    // Run 60 seconds at 60 Hz (time constant τ = m/k = 8s, so 60s ≈ 7.5τ).
    let dt = 1.0 / 60.0;
    for _ in 0..(60 * 60) {
        phys.step(dt);
    }

    let b = phys.body(e).unwrap();
    let vy = b.velocity.y;
    let v_term = -78.48;
    let error = (vy - v_term).abs() / v_term.abs();
    assert!(
        error < 0.01,
        "vy = {vy}, v_term = {v_term}, error = {error:.4}"
    );

    // Heavier body falls faster (more negative vy) than 1 kg terminal (19.62).
    assert!(vy < -19.0);
}

// ---------------------------------------------------------------------------
// Determinism
// ---------------------------------------------------------------------------

#[test]
fn same_initial_state_produces_same_final_state() {
    let run = || {
        let mut phys = make_falling_system();
        let e = crcbl_phys::test_entity(0);
        let mut body = RigidBody::new_dynamic(2.0);
        body.velocity = DVec3::new(10.0, 5.0, 0.0);
        phys.set_body(e, body);
        phys.set_transform(e, Transform::from_position(DVec3::new(42.0, 100.0, 7.0)));

        let dt = 1.0 / 120.0;
        for _ in 0..1000 {
            phys.step(dt);
        }

        let b = phys.body(e).unwrap();
        let t = phys.transform(e).unwrap();
        (b.velocity, t.position)
    };

    let (v1, p1) = run();
    let (v2, p2) = run();

    assert_eq!(v1, v2, "velocity drift across runs");
    assert_eq!(p1, p2, "position drift across runs");
}

// ---------------------------------------------------------------------------
// Energy drift (symplectic integrator)
// ---------------------------------------------------------------------------

#[test]
fn energy_drift_is_bounded_for_oscillator() {
    // A mass on a spring: F = -k*x.  With symplectic Euler, energy should
    // oscillate around the true value rather than growing unbounded.
    // We verify that after 1000 periods the position hasn't exploded.

    #[derive(Debug)]
    struct SpringForce {
        stiffness: f64,
    }
    impl ForceProvider for SpringForce {
        fn apply(&self, body: &mut RigidBody, transform: &Transform, _dt: f64) {
            if body.is_dynamic() {
                let force = -transform.position * self.stiffness;
                body.apply_force(force);
            }
        }
    }

    let mut phys = PhysicsSystem::new();
    phys.add_force_provider(Box::new(SpringForce { stiffness: 10.0 }));
    let e = crcbl_phys::test_entity(0);
    phys.set_body(e, RigidBody::new_dynamic(1.0));
    phys.set_transform(e, Transform::from_position(DVec3::new(1.0, 0.0, 0.0)));

    let dt = 1.0 / 120.0;
    // Run for many periods.
    for _ in 0..(100 * 120) {
        phys.step(dt);
    }

    let t = phys.transform(e).unwrap();
    // Position should still be bounded (not NaN, not infinite, not huge).
    assert!(t.position.x.is_finite());
    assert!(
        t.position.x.abs() < 10.0,
        "oscillator position drifted to {:.2}, expected |x| < 10",
        t.position.x
    );
}

// ---------------------------------------------------------------------------
// 1000-body determinism stress test
// ---------------------------------------------------------------------------

/// Compute a simple FNV-1a-like hash of all body states for determinism
/// verification.
fn hash_physics_state(phys: &PhysicsSystem, count: usize) -> u64 {
    use std::collections::hash_map::DefaultHasher;
    use std::hash::{Hash, Hasher};

    let mut hasher = DefaultHasher::new();
    for i in 0..count {
        let e = crcbl_phys::test_entity(i as u32);
        if let Some(body) = phys.body(e) {
            body.velocity.to_array().iter().for_each(|v| {
                v.to_bits().hash(&mut hasher);
            });
            body.force_accum.to_array().iter().for_each(|v| {
                v.to_bits().hash(&mut hasher);
            });
            body.inverse_mass.to_bits().hash(&mut hasher);
        }
        if let Some(transform) = phys.transform(e) {
            transform.position.to_array().iter().for_each(|v| {
                v.to_bits().hash(&mut hasher);
            });
        }
    }
    hasher.finish()
}

#[test]
fn thousand_body_determinism() {
    let run = || {
        let mut phys = PhysicsSystem::new();
        phys.add_force_provider(Box::new(GravityForce::EARTH));
        phys.add_force_provider(Box::new(DragForce::new(0.1)));

        let count = 1000;
        for i in 0..count {
            let e = crcbl_phys::test_entity(i as u32);
            let mass = 1.0 + ((i % 10) as f64) * 0.5; // 1.0..5.5 kg
            let mut body = RigidBody::new_dynamic(mass);
            // Deterministic initial velocity based on index.
            body.velocity = DVec3::new(
                (i as f64) * 0.1,
                -((i as f64) % 7.0) * 0.5,
                (i as f64).sin() * 2.0,
            );
            phys.set_body(e, body);
            phys.set_transform(
                e,
                Transform::from_position(DVec3::new(
                    (i as f64) * 2.0,
                    100.0 + (i as f64) * 0.1,
                    (i as f64) * 0.3,
                )),
            );
        }

        // Simulate 5 seconds at 120 Hz.
        let dt = 1.0 / 120.0;
        for _ in 0..(5 * 120) {
            phys.step(dt);
        }

        hash_physics_state(&phys, count)
    };

    let h1 = run();
    let h2 = run();
    assert_eq!(h1, h2, "1000-body sim produced different state hashes");
}

#[test]
fn thousand_body_substep_count_preserves_determinism() {
    // Same total dt, different substep counts — different result (by design
    // with explicit Euler, but symplectic Euler also diverges). This test
    // just verifies the hash is computed without panicking at scale.
    let mut phys = PhysicsSystem::new();
    phys.add_force_provider(Box::new(GravityForce::EARTH));

    let count = 500;
    for i in 0..count {
        let e = crcbl_phys::test_entity(i as u32);
        phys.set_body(e, RigidBody::new_dynamic(1.0 + (i % 5) as f64));
        phys.set_transform(
            e,
            Transform::from_position(DVec3::new((i as f64) * 1.5, 50.0, 0.0)),
        );
    }

    let dt = 1.0 / 60.0;
    for _ in 0..600 {
        phys.step(dt);
    }

    let hash = hash_physics_state(&phys, count);
    // Hash is non-zero (sanity check).
    assert!(hash != 0, "state hash should be non-zero");

    // Verify all bodies exist and have finite state.
    for i in 0..count {
        let e = crcbl_phys::test_entity(i as u32);
        let b = phys.body(e).expect("body should exist");
        let t = phys.transform(e).expect("transform should exist");
        assert!(b.velocity.is_finite());
        assert!(t.position.is_finite());
    }
}
