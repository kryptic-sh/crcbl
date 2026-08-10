//! Numerical integrators for the physics dynamics pipeline.
//!
//! Each integrator advances a rigid body's position and velocity over a
//! fixed timestep `dt`. The default is [`SemiImplicitEuler`] (symplectic
//! Euler), which is energy-stable enough for game physics and simple enough
//! to be deterministic across all targets.

use crate::components::{RigidBody, Transform};

// ---------------------------------------------------------------------------
// Integrator trait
// ---------------------------------------------------------------------------

/// Advances a [`RigidBody`]'s position (in [`Transform`]) and velocity
/// over one fixed timestep.
///
/// Implementations must be deterministic: given the same `body`, `transform`,
/// `dt`, and accumulated force, they must produce the same result every time
/// on the same binary. No platform-specific fast-math or FMA allowed.
pub trait Integrator: std::fmt::Debug {
    /// Advance the body by `dt` seconds.
    ///
    /// * `body` — mutable ref to the rigid body (velocity + force accumulator).
    /// * `transform` — mutable ref to the body's transform (position is updated).
    /// * `dt` — fixed timestep in seconds.
    ///
    /// After this call the force accumulator should be cleared so the next
    /// substep starts fresh.
    fn step(&self, body: &mut RigidBody, transform: &mut Transform, dt: f64);
}

// ---------------------------------------------------------------------------
// Semi-implicit Euler
// ---------------------------------------------------------------------------

/// Symplectic (semi-implicit) Euler integrator.
///
/// Update order:
///   1. `velocity += (force / mass) * dt`
///   2. `position += velocity * dt`
///
/// This order is symplectic: it conserves a perturbed energy and avoids the
/// unbounded energy growth of explicit Euler on oscillatory systems.
#[derive(Debug, Clone, Copy, Default)]
pub struct SemiImplicitEuler;

impl Integrator for SemiImplicitEuler {
    fn step(&self, body: &mut RigidBody, transform: &mut Transform, dt: f64) {
        debug_assert!(dt > 0.0, "integration step dt must be positive");
        debug_assert!(body.inverse_mass.is_finite(), "inverse mass must be finite");

        // a = F / m = F * (1/m)
        let acceleration = body.force_accum * body.inverse_mass;
        body.velocity += acceleration * dt;
        transform.position += body.velocity * dt;
        body.clear_forces();
    }
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use glam::DVec3;

    #[test]
    fn euler_moves_stationary_body_under_gravity() {
        let integrator = SemiImplicitEuler;
        let mut body = RigidBody::new_dynamic(1.0); // 1 kg
        body.apply_force(DVec3::new(0.0, -9.81, 0.0)); // gravity
        let mut transform = Transform::from_position(DVec3::new(10.0, 20.0, 0.0));
        let dt = 1.0 / 60.0;

        integrator.step(&mut body, &mut transform, dt);

        // v = 0 + (-9.81)/1 * dt = -0.1635
        let expected_vy = -9.81 * dt;
        assert!(
            (body.velocity.y - expected_vy).abs() < 1e-12,
            "vy = {}, expected ~{}",
            body.velocity.y,
            expected_vy
        );
        // pos = (10, 20, 0) + (0, vy, 0) * dt
        assert!((transform.position.y - 20.0 + 9.81 * dt * dt).abs() < 1e-12);
    }

    #[test]
    fn euler_clears_forces_after_step() {
        let integrator = SemiImplicitEuler;
        let mut body = RigidBody::new_dynamic(2.0);
        body.apply_force(DVec3::new(10.0, 0.0, 0.0));
        let mut transform = Transform::IDENTITY;
        integrator.step(&mut body, &mut transform, 0.1);
        assert_eq!(body.force_accum, DVec3::ZERO);
    }

    #[test]
    fn kinematic_body_does_not_move() {
        let integrator = SemiImplicitEuler;
        let mut body = RigidBody::new_kinematic();
        body.apply_force(DVec3::new(100.0, 0.0, 0.0));
        let mut transform = Transform::from_position(DVec3::new(1.0, 2.0, 3.0));
        integrator.step(&mut body, &mut transform, 0.1);
        // acceleration = 100 * 0 = 0, velocity stays 0, position unchanged
        assert_eq!(body.velocity, DVec3::ZERO);
        assert_eq!(transform.position, DVec3::new(1.0, 2.0, 3.0));
    }

    #[test]
    fn two_identical_euler_runs_end_at_bit_identical_velocity_and_position() {
        let integrator = SemiImplicitEuler;
        let dt = 1.0 / 120.0;

        let run = || {
            let mut body = RigidBody::new_dynamic(5.0);
            body.velocity = DVec3::new(1.0, 2.0, 0.0);
            body.apply_force(DVec3::new(0.0, -9.81 * 5.0, 0.0));
            let mut transform = Transform::from_position(DVec3::new(0.0, 100.0, 0.0));
            for _ in 0..10 {
                integrator.step(&mut body, &mut transform, dt);
                body.apply_force(DVec3::new(0.0, -9.81 * 5.0, 0.0));
            }
            (body.velocity, transform.position)
        };

        let (v1, p1) = run();
        let (v2, p2) = run();
        assert_eq!(v1, v2);
        assert_eq!(p1, p2);
    }
}
