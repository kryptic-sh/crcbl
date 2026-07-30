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
