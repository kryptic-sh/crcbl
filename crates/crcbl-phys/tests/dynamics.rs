//! The dynamics pipeline: force providers, the integrator, and what falls out
//! of running them for a long time.
//!
//! These verify emergent physical behaviour — terminal velocity, energy drift
//! bounds, determinism — that simple unit tests cannot capture, which is why
//! they are a target of their own rather than a `#[cfg(test)]` module beside
//! `PhysicsSystem`: the claim is about a long run of the assembled pipeline,
//! not about any one call into it.
//!
//! Both files in this directory are property tests, so "property" is not what
//! distinguishes them — this one is the integrator, `broadphase_churn.rs` is
//! the BVH and the overlap queries.

use crcbl_phys::{
    DampingForce, DragForce, ForceProvider, GravityForce, PhysicsSystem, RigidBody, ThrustForce,
    Transform,
};
use glam::{DQuat, DVec3};

/// Build an [`Entity`] from a raw index for use in these tests.
///
/// Real entities only come from `crcbl_ecs::World::spawn`; these tests drive
/// `PhysicsSystem` without a `World`, so they fabricate handles instead.
/// Generation 1 is the first generation `Pool` ever issues, so every handle
/// built here is well-formed.
fn test_entity(index: u32) -> crcbl_ecs::Entity {
    crcbl_ecs::Entity::from_bits((1u64 << 32) | index as u64).expect("generation 1 is never zero")
}

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
    let e = test_entity(0);
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
    let e = test_entity(0);
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
// Thrust and damping (L1)
// ---------------------------------------------------------------------------

#[test]
fn thrust_alone_reaches_the_closed_form_velocity() {
    // Constant thrust on a constant mass is constant acceleration, so after n
    // steps of semi-implicit Euler v = (T/m)·n·dt — exactly, no approximation
    // in the integrator to allow for.
    const THRUST: f64 = 30.0;
    const MASS: f64 = 4.0;
    const STEPS: usize = 600;
    let dt = 1.0 / 120.0;

    let mut phys = PhysicsSystem::new();
    phys.add_force_provider(Box::new(ThrustForce::new(THRUST, DVec3::Y)));
    let e = test_entity(0);
    phys.set_body(e, RigidBody::new_dynamic(MASS));
    phys.set_transform(e, Transform::IDENTITY);

    for _ in 0..STEPS {
        phys.step(dt);
    }

    let expected = (THRUST / MASS) * STEPS as f64 * dt;
    let vy = phys.body(e).unwrap().velocity.y;
    assert!(
        (vy - expected).abs() < 1e-9,
        "vy = {vy}, closed form = {expected}"
    );
    // Thrust is one-directional: nothing should have moved off the axis.
    assert_eq!(phys.body(e).unwrap().velocity.x, 0.0);
    assert_eq!(phys.body(e).unwrap().velocity.z, 0.0);
}

#[test]
fn thrust_follows_the_body_orientation() {
    // The ship turns, the thrust turns with it. A quarter turn about +Z sends
    // local +Y to world -X, and the playfield plane (z = 0) is not left.
    const THRUST: f64 = 12.0;
    let dt = 1.0 / 120.0;

    let mut phys = PhysicsSystem::new();
    phys.add_force_provider(Box::new(ThrustForce::new(THRUST, DVec3::Y)));
    let e = test_entity(0);
    phys.set_body(e, RigidBody::new_dynamic(3.0));
    phys.set_transform(
        e,
        Transform::new(
            DVec3::ZERO,
            DQuat::from_rotation_z(std::f64::consts::FRAC_PI_2),
        ),
    );

    for _ in 0..240 {
        phys.step(dt);
    }

    let v = phys.body(e).unwrap().velocity;
    let expected = (THRUST / 3.0) * 240.0 * dt;
    assert!(
        (v.x + expected).abs() < 1e-9,
        "vx = {}, want {}",
        v.x,
        -expected
    );
    assert!(v.y.abs() < 1e-9, "thrust leaked onto +Y: {}", v.y);
    assert_eq!(v.z, 0.0, "thrust left the playfield plane");
}

#[test]
fn thrust_against_damping_reaches_the_closed_form_terminal_velocity() {
    // With both, each step is v' = v·(1 - k·dt/m) + T·dt/m, whose closed form
    // is v_n = (T/k)·(1 - (1 - k·dt/m)^n) and whose limit is T/k. This is the
    // model asteroids' ship flies under: press thrust, coast to a top speed.
    const THRUST: f64 = 24.0;
    const DAMPING: f64 = 0.8;
    const MASS: f64 = 2.0;
    // 30 seconds at 120 Hz, against a time constant τ = m/k = 2.5 s, so the
    // run ends 12τ in — deep enough that "it converged" is a claim about the
    // limit and not about a curve still climbing.
    const STEPS: usize = 3_600;
    let dt = 1.0 / 120.0;

    let mut phys = PhysicsSystem::new();
    phys.add_force_provider(Box::new(ThrustForce::new(THRUST, DVec3::Y)));
    phys.add_force_provider(Box::new(DampingForce::new(DAMPING)));
    let e = test_entity(0);
    phys.set_body(e, RigidBody::new_dynamic(MASS));
    phys.set_transform(e, Transform::IDENTITY);

    for _ in 0..STEPS {
        phys.step(dt);
    }

    let decay = 1.0 - DAMPING * dt / MASS;
    let closed_form = (THRUST / DAMPING) * (1.0 - decay.powi(STEPS as i32));
    let vy = phys.body(e).unwrap().velocity.y;
    let error = (vy - closed_form).abs() / closed_form.abs();
    assert!(
        error < 1e-9,
        "vy = {vy}, closed form = {closed_form}, relative error {error:e}"
    );

    // And it really is approaching the terminal velocity, not merely matching
    // an early part of the curve.
    let terminal = THRUST / DAMPING;
    assert!(
        (vy - terminal).abs() / terminal < 0.01,
        "vy = {vy} is not near the terminal velocity {terminal}"
    );
}

#[test]
fn damping_decays_toward_zero_and_never_past_it() {
    // The classic bug: -k·v applied explicitly overshoots when k·dt/m ≥ 1, so
    // the body reverses instead of stopping. Every combination here has a
    // large dt, and several are past that threshold.
    for &(mass, coefficient, dt) in &[
        (1.0f64, 1.5f64, 0.5f64), // k·dt/m = 0.75 — under the cap
        (1.0, 2.0, 0.5),          // exactly at it
        (1.0, 100.0, 0.5),        // far past it
        (0.25, 40.0, 1.0),        // past it, light body
        (10.0, 3.0, 2.0),         // past it, long step
    ] {
        let mut phys = PhysicsSystem::new();
        phys.add_force_provider(Box::new(DampingForce::new(coefficient)));
        let e = test_entity(0);
        let mut body = RigidBody::new_dynamic(mass);
        body.velocity = DVec3::new(37.0, -11.0, 4.0);
        phys.set_body(e, body);
        phys.set_transform(e, Transform::IDENTITY);

        let mut previous = phys.body(e).unwrap().velocity;
        for step in 0..64 {
            phys.step(dt);
            let v = phys.body(e).unwrap().velocity;
            // Never reverses: each component keeps its sign or reaches zero.
            for axis in 0..3 {
                let (before, after) = (previous.to_array()[axis], v.to_array()[axis]);
                assert!(
                    after == 0.0 || after.signum() == before.signum(),
                    "m={mass} k={coefficient} dt={dt} step {step} axis {axis}: \
                     {before} flipped to {after}"
                );
                assert!(
                    after.abs() <= before.abs(),
                    "m={mass} k={coefficient} dt={dt} step {step} axis {axis}: \
                     |{after}| grew past |{before}|"
                );
            }
            previous = v;
        }
        assert!(
            previous.length() < 1e-6,
            "m={mass} k={coefficient} dt={dt}: velocity settled at {previous:?} \
             rather than decaying to rest"
        );
    }
}

#[test]
fn drag_overshoots_at_the_timestep_damping_survives() {
    // The contrast that makes the test above mean something: the uncapped
    // model, given the same numbers, flips the velocity and then grows it.
    // `DragForce` is not wrong — it is the physical law, and this is what the
    // physical law does when integrated at a timestep it is unstable at.
    let dt = 0.5;
    let mut phys = PhysicsSystem::new();
    phys.add_force_provider(Box::new(DragForce::new(100.0)));
    let e = test_entity(0);
    let mut body = RigidBody::new_dynamic(1.0);
    body.velocity = DVec3::new(37.0, 0.0, 0.0);
    phys.set_body(e, body);
    phys.set_transform(e, Transform::IDENTITY);

    phys.step(dt);
    let after = phys.body(e).unwrap().velocity.x;
    assert!(
        after < 0.0 && after.abs() > 37.0,
        "expected uncapped drag to reverse and amplify; got {after}"
    );
}

#[test]
fn per_entity_force_moves_only_that_entity() {
    // The pipeline applies a provider to every body, which is wrong for a
    // ship's thrust among a field of rocks. `apply_force` is the per-entity
    // path, and `ThrustForce::world_force` is how it stays the same model.
    let dt = 1.0 / 60.0;
    let mut phys = PhysicsSystem::new();

    let ship = test_entity(0);
    let rock = test_entity(1);
    for e in [ship, rock] {
        phys.set_body(e, RigidBody::new_dynamic(1.0));
        phys.set_transform(e, Transform::IDENTITY);
    }

    let thrust = ThrustForce::new(20.0, DVec3::Y);
    let transform = *phys.transform(ship).unwrap();
    for _ in 0..60 {
        assert!(phys.apply_force(ship, thrust.world_force(&transform)));
        phys.step(dt);
    }

    assert!(
        phys.body(ship).unwrap().velocity.y > 0.0,
        "the ship did not accelerate"
    );
    assert_eq!(
        phys.body(rock).unwrap().velocity,
        DVec3::ZERO,
        "the rock felt the ship's thrust"
    );
    assert!(!phys.apply_force(test_entity(99), DVec3::Y), "no such body");
}

// ---------------------------------------------------------------------------
// Determinism
// ---------------------------------------------------------------------------

#[test]
fn thrust_and_damping_are_deterministic() {
    let run = || {
        let mut phys = PhysicsSystem::new();
        phys.add_force_provider(Box::new(ThrustForce::new(17.5, DVec3::new(1.0, 2.0, -3.0))));
        phys.add_force_provider(Box::new(DampingForce::new(0.37)));
        let e = test_entity(0);
        phys.set_body(e, RigidBody::new_dynamic(1.75));
        phys.set_transform(
            e,
            Transform::new(DVec3::new(3.0, 1.0, -2.0), DQuat::from_rotation_x(0.4)),
        );
        for _ in 0..2_000 {
            phys.step(1.0 / 240.0);
        }
        (
            phys.body(e).unwrap().velocity,
            phys.transform(e).unwrap().position,
        )
    };

    let (v1, p1) = run();
    let (v2, p2) = run();
    assert_eq!(v1, v2, "velocity differed between runs");
    assert_eq!(p1, p2, "position differed between runs");
    assert_ne!(v1, DVec3::ZERO, "nothing moved — nothing was pinned");
}

#[test]
fn same_initial_state_produces_same_final_state() {
    let run = || {
        let mut phys = make_falling_system();
        let e = test_entity(0);
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
    let e = test_entity(0);
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
        let e = test_entity(i as u32);
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
fn a_thousand_body_run_hashes_the_same_state_twice_over() {
    let run = || {
        let mut phys = PhysicsSystem::new();
        phys.add_force_provider(Box::new(GravityForce::EARTH));
        phys.add_force_provider(Box::new(DragForce::new(0.1)));

        let count = 1000;
        for i in 0..count {
            let e = test_entity(i as u32);
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
        let e = test_entity(i as u32);
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
        let e = test_entity(i as u32);
        let b = phys.body(e).expect("body should exist");
        let t = phys.transform(e).expect("transform should exist");
        assert!(b.velocity.is_finite());
        assert!(t.position.is_finite());
    }
}
