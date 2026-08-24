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
/// drag (atmospheric), see [`crate::AtmosphericDrag`].
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

impl DragForce {
    /// The world-space force this provider would apply to a body moving at
    /// `velocity`.
    ///
    /// Public for the same reason [`ThrustForce::world_force`] is: a provider
    /// is **global**, so a game that drags one entity among a field of others
    /// cannot use the pipeline and would otherwise write `-k·v` out again.
    #[inline]
    #[must_use]
    pub fn world_force(&self, velocity: DVec3) -> DVec3 {
        -velocity * self.coefficient
    }
}

impl ForceProvider for DragForce {
    fn apply(&self, body: &mut RigidBody, _transform: &Transform, _dt: f64) {
        if body.is_dynamic() {
            body.apply_force(self.world_force(body.velocity));
        }
    }
}

// ---------------------------------------------------------------------------
// Damping (drag that cannot overshoot)
// ---------------------------------------------------------------------------

/// Velocity damping that is stable at any timestep.
///
/// Like [`DragForce`] this applies `F = -k·v`, but it caps `k` at `m/dt` — the
/// exact coefficient that brings the body to a standstill in one step:
///
/// ```text
/// F = -min(k, m/dt) · v
/// ```
///
/// # Why the cap
///
/// Semi-implicit Euler applies the force before moving, so one step of plain
/// `-k·v` leaves `v' = v·(1 - k·dt/m)`. That factor is a sensible decay only
/// while `k·dt/m < 1`. At `k·dt/m = 2` the body's velocity is *negated* every
/// step, and past that it grows without bound — a body asked to slow down
/// harder flies off backwards instead. Nothing about the drag coefficient
/// warns of it: the same `k` that is well behaved at a 240 Hz substep explodes
/// at a 10 Hz one, so the failure shows up as a frame-rate-dependent
/// instability, which is the hardest kind to find.
///
/// With the cap, `k·dt/m ≥ 1` clamps to a factor of exactly zero: the velocity
/// reaches zero and stops there, never reverses.
///
/// # Which one to use
///
/// [`DragForce`] models a real fluid, where `F = -k·v` is the physics and the
/// caller is responsible for a timestep it is stable at. [`DampingForce`] is
/// the game-feel control — "a ship that coasts to a halt" — where the intent is
/// the decay and no physical claim is being made, so trading exactness at large
/// `dt` for never blowing up is the right trade.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct DampingForce {
    /// Damping coefficient (kg/s). Higher = faster decay.
    ///
    /// The time constant is `τ = m/k`: velocity falls to `1/e` of its value
    /// every `τ` seconds.
    pub coefficient: f64,
}

impl DampingForce {
    /// Create a damping force provider.
    #[inline]
    #[must_use]
    pub fn new(coefficient: f64) -> Self {
        debug_assert!(
            coefficient >= 0.0,
            "damping coefficient must be non-negative"
        );
        Self { coefficient }
    }
}

impl DampingForce {
    /// The world-space force this provider would apply to a body of `mass`
    /// moving at `velocity` over a step of `dt`, **including the cap**.
    ///
    /// Public for the same reason [`ThrustForce::world_force`] is: a provider
    /// is global, so a game damping one entity — a ship coasting to a halt
    /// among a field of rocks — cannot use the pipeline. `apps/asteroids` wrote
    /// this out by hand rather than go without, and the copy it kept was the
    /// arithmetic *and* the cap: the second is the part that is easy to leave
    /// out and impossible to notice until someone runs at a coarse tick rate
    /// and the ship flies backwards.
    ///
    /// `dt` is the step the force will be integrated over, which is what the
    /// cap is a function of; passing a different one silently changes the model.
    #[inline]
    #[must_use]
    pub fn world_force(&self, velocity: DVec3, mass: f64, dt: f64) -> DVec3 {
        // `m/dt` zeroes the velocity exactly; anything stronger reverses it.
        let critical = mass / dt;
        let effective = if self.coefficient < critical {
            self.coefficient
        } else {
            critical
        };
        -velocity * effective
    }
}

impl ForceProvider for DampingForce {
    fn apply(&self, body: &mut RigidBody, _transform: &Transform, dt: f64) {
        if !body.is_dynamic() {
            return;
        }
        body.apply_force(self.world_force(body.velocity, body.mass, dt));
    }
}

// ---------------------------------------------------------------------------
// Thrust
// ---------------------------------------------------------------------------

/// A constant force along a body-local direction, rotated by the body's
/// orientation.
///
/// This is the first force in the pipeline that reads the [`Transform`]'s
/// *rotation* rather than its position: thrust points where the body is
/// facing, so turning the body turns the force without the caller recomputing
/// anything.
///
/// ```text
/// F = magnitude · (rotation × local_direction)
/// ```
///
/// # Why the direction is body-local and configurable
///
/// [`Transform::forward`] is `-Z`, which is the facing of a 3D craft. A
/// top-down 2D game lays its playfield on XY and turns its ship about Z, where
/// `-Z` points at the camera and thrusting along it would drive the ship out of
/// the plane. Naming the local axis makes both work: `-Z` for the 3D case,
/// `+Y` for the 2D one.
///
/// # Closed form
///
/// Under thrust alone, acceleration is constant at `a = magnitude/m`, so after
/// `n` steps of semi-implicit Euler `v = a·n·dt` exactly. Under thrust and
/// [`DampingForce`] together the velocity converges to `v = magnitude/k`, and
/// the whole approach is closed-form too:
///
/// ```text
/// v_n = (T/k)·(1 - (1 - k·dt/m)^n)
/// ```
///
/// which is what `crates/crcbl-phys/tests/dynamics.rs` checks it against.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct ThrustForce {
    /// Thrust magnitude in newtons.
    pub magnitude: f64,
    /// Unit direction in the body's local frame.
    pub local_direction: DVec3,
}

impl ThrustForce {
    /// Create a thrust provider along a body-local direction.
    ///
    /// `local_direction` is normalised; a zero-length direction yields no
    /// thrust rather than a `NaN` force.
    #[inline]
    #[must_use]
    pub fn new(magnitude: f64, local_direction: DVec3) -> Self {
        Self {
            magnitude,
            local_direction: local_direction.normalize_or_zero(),
        }
    }

    /// Thrust along the body's forward axis ([`Transform::forward`], local
    /// `-Z`) — the 3D craft case.
    #[inline]
    #[must_use]
    pub fn forward(magnitude: f64) -> Self {
        Self {
            magnitude,
            local_direction: DVec3::NEG_Z,
        }
    }

    /// The world-space force this provider would apply to a body with the
    /// given orientation.
    ///
    /// Public because a game that thrusts *one* entity — a player's ship among
    /// a field of rocks — cannot use a pipeline provider, which applies to
    /// every body. It reads the same model, so the ship and any
    /// pipeline-thrusted body agree.
    #[inline]
    #[must_use]
    pub fn world_force(&self, transform: &Transform) -> DVec3 {
        (transform.rotation * self.local_direction) * self.magnitude
    }
}

impl ForceProvider for ThrustForce {
    fn apply(&self, body: &mut RigidBody, transform: &Transform, _dt: f64) {
        if body.is_dynamic() {
            body.apply_force(self.world_force(transform));
        }
    }
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use crate::integrator::{Integrator as _, SemiImplicitEuler};
    use glam::DQuat;

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
    fn drag_produces_a_force_opposite_the_velocity_and_proportional_to_it() {
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

    // ── Damping ──────────────────────────────────────────────────────────

    #[test]
    fn damping_matches_drag_at_a_sane_timestep() {
        // Below the cap the two are the same force, and that has to stay true
        // or "damping" would quietly be a different model from the one its
        // coefficient is tuned against.
        let mut damped = RigidBody::new_dynamic(2.0);
        let mut dragged = RigidBody::new_dynamic(2.0);
        damped.velocity = DVec3::new(3.0, -4.0, 1.0);
        dragged.velocity = damped.velocity;

        let dt = 1.0 / 120.0;
        DampingForce::new(0.5).apply(&mut damped, &Transform::IDENTITY, dt);
        DragForce::new(0.5).apply(&mut dragged, &Transform::IDENTITY, dt);
        assert_eq!(damped.force_accum, dragged.force_accum);
    }

    #[test]
    fn damping_is_capped_at_the_force_that_stops_the_body() {
        // k = 1000 against m/dt = 2/0.5 = 4: the cap binds, and the force is
        // exactly the one that zeroes this velocity in one step.
        let dt = 0.5;
        let mut body = RigidBody::new_dynamic(2.0);
        body.velocity = DVec3::new(10.0, 0.0, 0.0);
        DampingForce::new(1000.0).apply(&mut body, &Transform::IDENTITY, dt);
        assert_eq!(body.force_accum, DVec3::new(-40.0, 0.0, 0.0));

        // Confirm it is the stopping force by taking the step: a = F/m = -20,
        // v' = 10 + (-20)(0.5) = 0.
        let mut transform = Transform::IDENTITY;
        SemiImplicitEuler.step(&mut body, &mut transform, dt);
        assert_eq!(body.velocity, DVec3::ZERO);
    }

    /// The per-entity route is the pipeline's own model, cap included.
    ///
    /// `world_force` exists so a game damping one body among many does not have
    /// to write `-k·v` and the `m/dt` cap out again — `apps/asteroids` did, and
    /// its copy is deleted. A route that agreed with the provider everywhere
    /// except the capped regime would be worse than the copy was: the game
    /// would look like it was using the engine's model and would not be.
    #[test]
    fn the_per_entity_damping_is_the_pipelines_own_including_the_cap() {
        let provider = DampingForce::new(3.0);
        let mass = 2.0;
        let mut capped = 0;
        for dt in [1.0 / 240.0, 1.0 / 60.0, 0.5, 1.0, 4.0] {
            for velocity in [
                DVec3::ZERO,
                DVec3::new(3.0, -4.0, 0.0),
                DVec3::new(-14.0, 0.0, 0.0),
            ] {
                let mut body = RigidBody::new_dynamic(mass);
                body.velocity = velocity;
                provider.apply(&mut body, &Transform::IDENTITY, dt);
                // Against the model written out, **not** against the
                // provider: `apply` delegates to `world_force`, so comparing
                // the two would be comparing a value with itself and would
                // stay green with the cap deleted. Measured: it does.
                let effective = provider.coefficient.min(mass / dt);
                let expected = -velocity * effective;
                assert_eq!(
                    provider.world_force(velocity, mass, dt),
                    expected,
                    "the per-entity route at dt {dt}, {velocity:?}",
                );
                assert_eq!(
                    body.force_accum, expected,
                    "the pipeline at dt {dt}, {velocity:?}",
                );
                if provider.coefficient > mass / dt {
                    capped += 1;
                    // The cap's whole claim: this force zeroes the velocity in
                    // one step and does not reverse it.
                    let mut transform = Transform::IDENTITY;
                    SemiImplicitEuler.step(&mut body, &mut transform, dt);
                    assert_eq!(
                        body.velocity,
                        DVec3::ZERO,
                        "the capped force did not stop the body at dt {dt}",
                    );
                }
            }
        }
        assert!(
            capped > 0,
            "no `dt` in this grid reaches the cap, so the case that matters is untested",
        );
    }

    /// `DragForce`'s route likewise, which has no cap to get wrong and is here
    /// so the pair cannot drift apart.
    #[test]
    fn the_per_entity_drag_is_the_pipelines_own() {
        let provider = DragForce::new(0.75);
        let velocity = DVec3::new(3.0, -4.0, 12.0);
        let mut body = RigidBody::new_dynamic(2.0);
        body.velocity = velocity;
        provider.apply(&mut body, &Transform::IDENTITY, 1.0 / 60.0);
        assert_eq!(body.force_accum, provider.world_force(velocity));
    }

    #[test]
    fn damping_ignores_kinematic_bodies() {
        let mut body = RigidBody::new_kinematic();
        body.velocity = DVec3::new(5.0, 0.0, 0.0);
        DampingForce::new(1.0).apply(&mut body, &Transform::IDENTITY, 0.1);
        assert_eq!(body.force_accum, DVec3::ZERO);
    }

    // ── Thrust ───────────────────────────────────────────────────────────

    #[test]
    fn thrust_points_along_the_body_facing() {
        // Unrotated, `forward` thrust is local -Z.
        let thrust = ThrustForce::forward(50.0);
        let mut body = RigidBody::new_dynamic(1.0);
        thrust.apply(&mut body, &Transform::IDENTITY, 0.1);
        assert_eq!(body.force_accum, DVec3::new(0.0, 0.0, -50.0));

        // Yawed a quarter turn about +Y, local -Z points along world -X.
        let mut body = RigidBody::new_dynamic(1.0);
        let transform = Transform::new(
            DVec3::ZERO,
            DQuat::from_rotation_y(std::f64::consts::FRAC_PI_2),
        );
        thrust.apply(&mut body, &transform, 0.1);
        assert!(
            (body.force_accum - DVec3::new(-50.0, 0.0, 0.0)).length() < 1e-12,
            "got {:?}",
            body.force_accum
        );
    }

    #[test]
    fn thrust_along_a_chosen_local_axis_stays_in_the_plane() {
        // The 2D case: playfield on XY, ship turns about Z, thrust along local
        // +Y. A quarter turn about +Z takes local +Y to world -X, and z stays
        // zero — which is what `forward()`'s -Z would have broken.
        let thrust = ThrustForce::new(10.0, DVec3::Y);
        let mut body = RigidBody::new_dynamic(1.0);
        let transform = Transform::new(
            DVec3::ZERO,
            DQuat::from_rotation_z(std::f64::consts::FRAC_PI_2),
        );
        thrust.apply(&mut body, &transform, 0.1);
        assert!(
            (body.force_accum - DVec3::new(-10.0, 0.0, 0.0)).length() < 1e-12,
            "got {:?}",
            body.force_accum
        );
        assert_eq!(body.force_accum.z, 0.0);
    }

    #[test]
    fn thrust_normalises_its_direction() {
        // A caller passing an unnormalised axis gets the magnitude they asked
        // for, not that magnitude times the axis length.
        let thrust = ThrustForce::new(7.0, DVec3::new(0.0, 3.0, 0.0));
        let mut body = RigidBody::new_dynamic(1.0);
        thrust.apply(&mut body, &Transform::IDENTITY, 0.1);
        assert_eq!(body.force_accum, DVec3::new(0.0, 7.0, 0.0));
    }

    #[test]
    fn zero_thrust_direction_produces_no_force_rather_than_nan() {
        let thrust = ThrustForce::new(7.0, DVec3::ZERO);
        let mut body = RigidBody::new_dynamic(1.0);
        thrust.apply(&mut body, &Transform::IDENTITY, 0.1);
        assert_eq!(body.force_accum, DVec3::ZERO);
    }

    #[test]
    fn thrust_ignores_kinematic_bodies() {
        let mut body = RigidBody::new_kinematic();
        ThrustForce::forward(100.0).apply(&mut body, &Transform::IDENTITY, 0.1);
        assert_eq!(body.force_accum, DVec3::ZERO);
    }

    #[test]
    fn two_force_providers_add_into_the_same_accumulator_rather_than_replacing_it() {
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
