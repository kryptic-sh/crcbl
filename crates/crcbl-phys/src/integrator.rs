//! Numerical integrators for the physics dynamics pipeline.
//!
//! Each integrator advances a rigid body's position, orientation and velocities
//! over a fixed timestep `dt`. The default is [`SemiImplicitEuler`] (symplectic
//! Euler), which is energy-stable enough for game physics and simple enough
//! to be deterministic across all targets.

use crcbl_core::trig;
use glam::{DMat3, DQuat, DVec3};

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

/// Symplectic (semi-implicit) Euler integrator, with rotation.
///
/// Update order, each velocity before the position it moves:
///   1. `velocity += (force / mass) * dt`
///   2. `position += velocity * dt`
///   3. `angular_velocity += I_world⁻¹ · torque · dt`, then the gyroscopic
///      step below, for a body with rotational inertia
///   4. the orientation turned by the angular velocity, below
///
/// The linear order is symplectic: it conserves a perturbed energy and avoids
/// the unbounded energy growth of explicit Euler on oscillatory systems.
///
/// # The gyroscopic step
///
/// A body whose three principal moments differ does not keep spinning about a
/// fixed axis: its angular momentum is what is conserved, and `ω = I⁻¹ L`
/// wanders as it turns. That is Euler's rotation equation, `I ω̇ + ω × I ω = τ`,
/// and the `ω × I ω` term is what flips a T-handle spun about its middle axis.
///
/// It is integrated **implicitly**, in the body's frame, by the **implicit
/// midpoint rule**: [`gyroscopic_step`] solves
/// `I (ω₂ − ω₁) + dt · ω̄ × I ω̄ = 0`, with `ω̄ = (ω₁ + ω₂) / 2`, by
/// [`GYROSCOPIC_ITERATIONS`] Newton-Raphson iterations.
///
/// The engines this crate's plan surveyed integrate the same equation three
/// ways, and this is the fourth on purpose:
///
/// * **Explicit Euler**, `ω₂ = ω₁ − dt I⁻¹ (ω₁ × I ω₁)`, gains energy. Jolt
///   uses it and rescales the momentum's magnitude back after every step, which
///   holds the magnitude and not the energy.
/// * **Implicit Euler**, `I (ω₂ − ω₁) + dt · ω₂ × I ω₂ = 0` with one Newton
///   iteration, is Box3D's `b3IntegrateVelocitiesTask` (its source credits the
///   Jacobian to Erin Catto) and Bullet's
///   `btRigidBody::computeGyroscopicImpulseImplicit_Body`. It never gains
///   energy, because it loses it: measured on 2026-09-17, a T-handle spinning
///   at 8 rad/s at 240 Hz lost half its energy and 30% of its angular momentum
///   in two minutes. That is a stable choice for a crate that has contacts to
///   damp anyway, and the wrong one for rung 0's scene, whose whole point is a
///   free body keeping its momentum.
/// * **The implicit midpoint rule** conserves every quadratic invariant of the
///   equation exactly — Hairer, Lubich and Wanner, *Geometric Numerical
///   Integration*, theorem IV.2.1 — and a free body's kinetic energy
///   `½ ωᵀ I ω` and its momentum's length `|I ω|` are both quadratic. So a
///   tumble here drifts only by the Newton iterations' residual and rounding;
///   `a_zero_g_tumble_conserves_momentum_and_energy` measures by how much.
///   Where the iterations have not converged, the answer is scaled back onto
///   the energy the step started with, so an unconverged step can lose energy
///   but never gain it.
///
/// # The orientation update
///
/// With ω̄ held for the step, the midpoint rule turns the body-frame momentum by
/// the **Cayley transform** of `dt · ω̄`, a rotation by `2 atan(dt |ω̄| / 2)`.
/// Turning the orientation by exactly that rotation, the other way, is what
/// makes the world-frame momentum `R I ω` come out unchanged — and that rotation
/// as a quaternion is `(dt ω̄ / 2, 1)`, normalised: [`cayley_rotation`]. It is
/// the same quaternion Box3D's `b3IntegrateRotation` builds, which Box3D reaches
/// as a first-order update and which here is exact for the momentum it
/// conserves. It needs no sine and no cosine.
///
/// A body with **no rotational inertia** keeps its angular velocity, so nothing
/// conserves anything and the rotation that matters is the one the game asked
/// for: `|ω| dt` exactly, through the exponential map
/// [`rotation_from_scaled_axis`], whose sine and cosine are
/// [`crcbl_core::trig`]'s so the result is the same on every target.
///
/// Either way the quaternion is renormalised, which keeps its length within
/// [`MAX_ROTATION_LENGTH_ERROR`] of one however long it runs, and a body with
/// zero angular velocity is not touched at all, so an orientation a game sets
/// by hand survives every step bit for bit.
#[derive(Debug, Clone, Copy, Default)]
pub struct SemiImplicitEuler;

/// How far a body's quaternion may drift from unit length.
///
/// Each step renormalises, and one division by a square root lands within a
/// few units in the last place of one; `a_spinning_quaternion_stays_unit_length`
/// measures the worst over a million steps against this.
pub const MAX_ROTATION_LENGTH_ERROR: f64 = 4.0 * f64::EPSILON;

/// Newton-Raphson iterations [`gyroscopic_step`] takes.
///
/// Fixed rather than a tolerance, so every run does the same arithmetic.
/// Measured on 2026-09-17 over ten simulated minutes. One iteration let a
/// T-handle at 8 rad/s and 240 Hz drift `2·10^-5` in momentum and gain energy.
/// For a thin bar spun off-axis at 60 Hz, two iterations drifted `5·10^-7` at
/// half a radian a step and `0.15` at two; four held the drift under `10^-11`
/// up to two radians a step — past the quarter turn per substep
/// `docs/plan/36-contact-solver.md` caps rotation at — and to `2·10^-6` at
/// five.
pub const GYROSCOPIC_ITERATIONS: usize = 4;

impl Integrator for SemiImplicitEuler {
    fn step(&self, body: &mut RigidBody, transform: &mut Transform, dt: f64) {
        debug_assert!(dt > 0.0, "integration step dt must be positive");
        debug_assert!(body.inverse_mass.is_finite(), "inverse mass must be finite");
        let spin = Self::integrate_velocity(body, transform.rotation, dt);
        Self::integrate_position(body, transform, spin, dt);
        body.clear_forces();
    }
}

/// The angular half of one [`SemiImplicitEuler`] step, carried from
/// [`SemiImplicitEuler::integrate_velocity`] to
/// [`SemiImplicitEuler::integrate_position`].
///
/// The implicit midpoint rule turns the orientation by the *mean* of the
/// body-frame angular velocity before and after the gyroscopic step, so the
/// position half needs both, and not only the world-frame velocity the contact
/// solver sees between the two halves.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct SpinStep {
    /// The body-frame angular velocity with the torque applied, before the
    /// gyroscopic step.
    before: DVec3,
    /// The body-frame angular velocity after it.
    after: DVec3,
    /// `after` in the world, through the orientation at the start of the step:
    /// what [`SemiImplicitEuler::integrate_velocity`] left in
    /// [`RigidBody::angular_velocity`]. The position half compares against it
    /// to learn whether anything — a contact impulse — changed the spin since.
    world: DVec3,
}

impl SemiImplicitEuler {
    /// The velocity half of [`Integrator::step`]: the force and the torque
    /// applied, and the gyroscopic step taken, for a body oriented at
    /// `rotation`. The accumulators are left alone.
    ///
    /// Between this and [`integrate_position`](Self::integrate_position) the
    /// body's velocities are the ones a constraint solver works on; the two
    /// halves back to back are exactly [`Integrator::step`], bit for bit.
    pub fn integrate_velocity(body: &mut RigidBody, rotation: DQuat, dt: f64) -> SpinStep {
        // a = F / m = F * (1/m)
        let acceleration = body.force_accum * body.inverse_mass;
        body.velocity += acceleration * dt;

        if !body.has_rotational_inertia() {
            return SpinStep::default();
        }
        let local_torque = rotation.inverse() * body.torque_accum;
        let before = rotation.inverse() * body.angular_velocity
            + body.inverse_local_inertia * local_torque * dt;
        let after = gyroscopic_step(body.local_inertia, before, dt);
        let world = rotation * after;
        body.angular_velocity = world;
        SpinStep {
            before,
            after,
            world,
        }
    }

    /// The position half of [`Integrator::step`]: the body moved by its
    /// velocity and turned by its spin.
    ///
    /// `spin` is what [`integrate_velocity`](Self::integrate_velocity) returned
    /// for this body. If its angular velocity has changed since, the changed
    /// one is read back into the body's frame and the rotation uses that; if
    /// not, the step is the undivided one's to the bit.
    pub fn integrate_position(
        body: &mut RigidBody,
        transform: &mut Transform,
        spin: SpinStep,
        dt: f64,
    ) {
        transform.position += body.velocity * dt;

        if body.has_rotational_inertia() {
            let rotation = transform.rotation;
            let after = if body.angular_velocity == spin.world {
                spin.after
            } else {
                rotation.inverse() * body.angular_velocity
            };
            let midpoint = rotation * ((spin.before + after) * 0.5);
            if midpoint != DVec3::ZERO {
                transform.rotation = (cayley_rotation(midpoint, dt) * rotation).normalize();
            }
            // Back to the world through the orientation the body has *now*: the
            // body-frame velocity is the one at the end of the step, and reading
            // it through the start-of-step orientation instead turns it by a
            // whole step's rotation — measured, that cost a tumbling T-handle a
            // fifth of its energy in two minutes.
            body.angular_velocity = transform.rotation * after;
        } else if body.angular_velocity != DVec3::ZERO {
            transform.rotation = integrate_rotation(transform.rotation, body.angular_velocity, dt);
        }
    }
}

/// One implicit-midpoint step of `I ω̇ = −ω × I ω` in the body's frame, from
/// `omega` over `dt`; see [`SemiImplicitEuler`].
///
/// Newton-Raphson on `g(ω₂) = I (ω₂ − ω₁) + dt · ω̄ × I ω̄`, whose Jacobian is
/// `I + (dt / 2) (ω̄× I − (I ω̄)×)`, started from `ω₂ = ω₁`.
#[must_use]
pub fn gyroscopic_step(inertia: DMat3, omega: DVec3, dt: f64) -> DVec3 {
    let mut next = omega;
    for _ in 0..GYROSCOPIC_ITERATIONS {
        let midpoint = (omega + next) * 0.5;
        let momentum = inertia * midpoint;
        let residual = inertia * (next - omega) + midpoint.cross(momentum) * dt;
        let jacobian = inertia + (skew(midpoint) * inertia - skew(momentum)) * (0.5 * dt);
        next -= jacobian.inverse() * residual;
    }
    // The midpoint rule conserves the energy only once Newton has converged,
    // and from a far enough first guess — a long thin body spun at several
    // radians a substep — it has not. Scaling the answer back onto the energy
    // it started with keeps an unconverged step from pumping energy in: the
    // one direction a free body's error must never go.
    let energy_before = omega.dot(inertia * omega);
    let energy_after = next.dot(inertia * next);
    if energy_after > energy_before {
        next *= (energy_before / energy_after).sqrt();
    }
    next
}

/// The matrix `[v]×` with `[v]× u = v × u`.
fn skew(v: DVec3) -> DMat3 {
    DMat3::from_cols(
        DVec3::new(0.0, v.z, -v.y),
        DVec3::new(-v.z, 0.0, v.x),
        DVec3::new(v.y, -v.x, 0.0),
    )
}

/// The rotation the Cayley transform of `dt · angular_velocity` describes, as a
/// unit quaternion: `(dt ω / 2, 1)` normalised, which turns by
/// `2 atan(dt |ω| / 2)` about `ω`; see [`SemiImplicitEuler`].
#[must_use]
pub fn cayley_rotation(angular_velocity: DVec3, dt: f64) -> DQuat {
    let half = angular_velocity * (0.5 * dt);
    DQuat::from_xyzw(half.x, half.y, half.z, 1.0).normalize()
}

/// `rotation` turned by `angular_velocity` held for `dt` — by `|ω| dt` exactly,
/// through the exponential map — and renormalised.
#[must_use]
pub fn integrate_rotation(rotation: DQuat, angular_velocity: DVec3, dt: f64) -> DQuat {
    (rotation_from_scaled_axis(angular_velocity * dt) * rotation).normalize()
}

/// The rotation by `|v|` radians about `v`'s direction, built with
/// [`crcbl_core::trig`] rather than the platform's sine and cosine.
///
/// `glam::DQuat::from_scaled_axis` and `from_axis_angle` are the same rotation
/// through the platform's `sin` and `cos`, which is why this crate's lint
/// configuration refuses them; this is the one to build an orientation with
/// anywhere the result reaches simulation state.
#[must_use]
pub fn rotation_from_scaled_axis(v: DVec3) -> DQuat {
    let angle = v.length();
    if angle == 0.0 {
        return DQuat::IDENTITY;
    }
    let half = 0.5 * angle;
    let axis = v * (trig::sin(half) / angle);
    DQuat::from_xyzw(axis.x, axis.y, axis.z, trig::cos(half))
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

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
