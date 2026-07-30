//! Force providers for the dynamics pipeline.
//!
//! Force providers are callbacks that append to each rigid body's
//! [`RigidBody::force_accum`] before integration. The physics system
//! iterates providers in order, so a body accumulates forces from
//! gravity, drag, thrust, etc. in a single pass.

use glam::DVec3;

use crate::components::{RigidBody, Transform};

// ---------------------------------------------------------------------------
// ForceProvider trait
// ---------------------------------------------------------------------------

/// Applies a force to a rigid body each substep.
///
/// Implementations read the body's state (velocity, position) and write
/// into `body.force_accum`. They must **not** clear the accumulator —
/// that is the integrator's job after the step.
pub trait ForceProvider: std::fmt::Debug {
    /// Apply force for this substep.
    ///
    /// `dt` is the integration timestep so providers can compute
    /// time-dependent forces (e.g. impulse-based thrust).
    fn apply(&self, body: &mut RigidBody, transform: &Transform, dt: f64);
}

// ---------------------------------------------------------------------------
// Gravity
// ---------------------------------------------------------------------------

/// Uniform gravitational acceleration.
///
/// Applies `F = m * g` where `g` is a world-space acceleration vector
/// (e.g. `(0, -9.81, 0)` for Earth surface gravity).
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct GravityForce {
    /// Gravitational acceleration vector (m/s²), e.g. `DVec3::NEG_Y * 9.81`.
    pub acceleration: DVec3,
}

impl GravityForce {
    /// Create a gravity force provider.
    #[inline]
    #[must_use]
    pub fn new(acceleration: DVec3) -> Self {
        Self { acceleration }
    }

    /// Standard Earth surface gravity (9.81 m/s² downward).
    pub const EARTH: Self = Self {
        acceleration: DVec3::new(0.0, -9.81, 0.0),
    };
}

impl ForceProvider for GravityForce {
    fn apply(&self, body: &mut RigidBody, _transform: &Transform, _dt: f64) {
        if body.is_dynamic() {
            body.apply_force(self.acceleration * body.mass);
        }
    }
}

// ---------------------------------------------------------------------------
// Drag (linear damping)
// ---------------------------------------------------------------------------

/// Velocity-proportional drag (linear damping).
///
/// Applies `F = -k * v` where `k` is the drag coefficient. For quadratic
/// drag (atmospheric), see the upcoming `AtmosphericDrag` provider.
///
/// When combined with [`GravityForce`], this causes terminal velocity to
/// emerge: `v_term = m * g / k`.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct DragForce {
    /// Drag coefficient (N·s/m, or kg/s). Higher = faster deceleration.
    pub coefficient: f64,
}

impl DragForce {
    /// Create a drag force provider.
    #[inline]
    #[must_use]
    pub fn new(coefficient: f64) -> Self {
        debug_assert!(coefficient >= 0.0, "drag coefficient must be non-negative");
        Self { coefficient }
    }
}

impl ForceProvider for DragForce {
    fn apply(&self, body: &mut RigidBody, _transform: &Transform, _dt: f64) {
        if body.is_dynamic() {
            body.apply_force(-body.velocity * self.coefficient);
        }
    }
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    // ── Gravity ──────────────────────────────────────────────────────────

    #[test]
    fn gravity_applies_mass_proportional_force() {
        let g = GravityForce::EARTH;
        let mut body = RigidBody::new_dynamic(2.0); // 2 kg
        g.apply(&mut body, &Transform::IDENTITY, 0.0);
        // F = m * g = 2.0 * -9.81 = -19.62
        assert_eq!(body.force_accum, DVec3::new(0.0, -19.62, 0.0));
    }

    #[test]
    fn gravity_ignores_kinematic_bodies() {
        let g = GravityForce::EARTH;
        let mut body = RigidBody::new_kinematic();
        g.apply(&mut body, &Transform::IDENTITY, 0.0);
        assert_eq!(body.force_accum, DVec3::ZERO);
    }

    // ── Drag ─────────────────────────────────────────────────────────────

    #[test]
    fn drag_opposes_velocity() {
        let drag = DragForce::new(0.5);
        let mut body = RigidBody::new_dynamic(1.0);
        body.velocity = DVec3::new(10.0, 0.0, 0.0);
        drag.apply(&mut body, &Transform::IDENTITY, 0.0);
        // F = -k * v = -0.5 * 10 = -5.0
        assert_eq!(body.force_accum, DVec3::new(-5.0, 0.0, 0.0));
    }

    #[test]
    fn drag_ignores_kinematic_bodies() {
        let drag = DragForce::new(1.0);
        let mut body = RigidBody::new_kinematic();
        body.velocity = DVec3::new(5.0, 0.0, 0.0);
        drag.apply(&mut body, &Transform::IDENTITY, 0.0);
        assert_eq!(body.force_accum, DVec3::ZERO);
    }

    #[test]
    fn multiple_providers_accumulate() {
        let g = GravityForce::EARTH;
        let drag = DragForce::new(0.3);
        let mut body = RigidBody::new_dynamic(1.0);
        body.velocity = DVec3::new(0.0, -5.0, 0.0); // falling
        g.apply(&mut body, &Transform::IDENTITY, 0.0);
        // gravity: (0, -9.81, 0)
        drag.apply(&mut body, &Transform::IDENTITY, 0.0);
        // drag: -0.3 * (0, -5, 0) = (0, 1.5, 0)
        // sum: (0, -8.31, 0)
        assert!(
            (body.force_accum.y + 8.31).abs() < 1e-10,
            "expected ~-8.31, got {}",
            body.force_accum.y
        );
    }
}
