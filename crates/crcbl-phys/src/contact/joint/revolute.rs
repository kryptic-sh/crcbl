//! The revolute joint, a hinge about the frames' z-axes: Box3D's
//! `src/revolute_joint.c` (`b3PrepareRevoluteJoint`,
//! `b3WarmStartRevoluteJoint`, `b3SolveRevoluteJoint`).
//!
//! Five rows: the point constraint (three), and two collinearity rows keeping
//! frame B's z-axis on frame A's, taken from the relative quaternion's x and
//! y parts — plus the spring, the motor and the two limits about the axis.

use glam::{DQuat, DVec3};

use super::math::{self, make_soft};
use super::{Common, Prepared as Joint, Velocities, limit_bias, motion};
use crate::components::RigidBody;
use crate::contact::solver::{Softness, SolverBody};
use crate::joint::RevoluteJoint;

#[derive(Clone, Copy, Debug)]
pub(in crate::contact) struct Prepared {
    settings: RevoluteJoint,
    /// Frame A's z-axis at the tick's start.
    axis_z: DVec3,
    axial_mass: f64,
    /// The collinearity rows' axes, kept from the last solve for the warm
    /// start.
    perp_x: DVec3,
    perp_y: DVec3,
    spring_softness: Softness,
}

/// `½ q (s e + v × e)`: the direction a collinearity row of the relative
/// rotation `rel` acts along, for axis `e` of frame A at `q`.
fn perp_axis(q: DQuat, rel: DQuat, e: DVec3) -> DVec3 {
    let v = DVec3::new(rel.x, rel.y, rel.z);
    0.5 * (q * (rel.w * e + v.cross(e)))
}

impl Prepared {
    pub(super) fn new(settings: RevoluteJoint, c: &Common) -> Self {
        let sum = c.ia + c.ib;
        let axis_z = c.frame_a.q * DVec3::Z;
        let k = axis_z.dot(sum * axis_z);
        let rel = math::inv_mul(c.frame_a.q, c.frame_b.q);
        Self {
            settings,
            axis_z,
            axial_mass: if k > 0.0 { 1.0 / k } else { 0.0 },
            perp_x: perp_axis(c.frame_a.q, rel, DVec3::X),
            perp_y: perp_axis(c.frame_a.q, rel, DVec3::Y),
            spring_softness: make_soft(0.0, 0.0, 1.0),
        }
    }

    pub(super) fn soften(&mut self, h: f64) {
        self.spring_softness = make_soft(
            self.settings.spring.hertz,
            self.settings.spring.damping_ratio,
            h,
        );
    }

    pub(super) fn warm_start(
        &self,
        joint: &Joint,
        solver: &[SolverBody],
        bodies: &mut [RigidBody],
    ) {
        let (_, dqa) = motion(solver, joint.a);
        let (_, dqb) = motion(solver, joint.b);
        let ra = dqa * joint.frame_a.p;
        let rb = dqb * joint.frame_b.p;
        let i = &joint.impulses;
        let axial = i.spring + i.motor + i.lower - i.upper;
        let angular = i.perp[0] * self.perp_x + i.perp[1] * self.perp_y + axial * self.axis_z;
        let mut v = Velocities::read(joint, bodies);
        v.push_turning(
            joint,
            i.linear,
            ra.cross(i.linear) + angular,
            rb.cross(i.linear) + angular,
        );
        v.write(joint, bodies);
    }

    pub(super) fn solve(
        &mut self,
        joint: &mut Joint,
        solver: &[SolverBody],
        bodies: &mut [RigidBody],
        h: f64,
        use_bias: bool,
    ) {
        let s = self.settings;
        let inverse_h = 1.0 / h;
        let soft = joint.softness;
        let mut v = Velocities::read(joint, bodies);
        let (dpa, dqa) = motion(solver, joint.a);
        let (dpb, dqb) = motion(solver, joint.b);

        let quat_a = dqa * joint.frame_a.q;
        let mut quat_b = dqb * joint.frame_b.q;
        if quat_a.dot(quat_b) < 0.0 {
            // Keeps the angle in [-π, π].
            quat_b = -quat_b;
        }
        let rel = math::inv_mul(quat_a, quat_b);
        let axis = self.axis_z;
        let turning = |v: &Velocities| (v.wb - v.wa).dot(axis);

        if s.enable_spring && !joint.fixed_rotation {
            let c = math::twist_angle(rel) - s.target_angle;
            let bias = self.spring_softness.bias_rate * c;
            let impulse = -self.spring_softness.mass_scale * self.axial_mass * (turning(&v) + bias)
                - self.spring_softness.impulse_scale * joint.impulses.spring;
            joint.impulses.spring += impulse;
            v.turn(joint, impulse * axis);
        }

        if s.enable_motor && !joint.fixed_rotation {
            let cdot = turning(&v) - s.motor_speed;
            let old = joint.impulses.motor;
            let max = s.max_motor_torque * h;
            joint.impulses.motor = (old - self.axial_mass * cdot).clamp(-max, max);
            v.turn(joint, (joint.impulses.motor - old) * axis);
        }

        if s.enable_limit && !joint.fixed_rotation {
            let angle = math::twist_angle(rel);

            let (bias, mass_scale, impulse_scale) =
                limit_bias(angle - s.lower_angle, inverse_h, soft, use_bias);
            let old = joint.impulses.lower;
            let impulse =
                -mass_scale * self.axial_mass * (turning(&v) + bias) - impulse_scale * old;
            joint.impulses.lower = (old + impulse).max(0.0);
            v.turn(joint, (joint.impulses.lower - old) * axis);

            // Signs flipped on the rate and the impulse.
            let (bias, mass_scale, impulse_scale) =
                limit_bias(s.upper_angle - angle, inverse_h, soft, use_bias);
            let old = joint.impulses.upper;
            let impulse =
                -mass_scale * self.axial_mass * (-turning(&v) + bias) - impulse_scale * old;
            joint.impulses.upper = (old + impulse).max(0.0);
            v.turn(joint, -(joint.impulses.upper - old) * axis);
        }

        // Collinearity, as a 2×2 block.
        if !joint.fixed_rotation {
            let (bias, mass_scale, impulse_scale) = if use_bias {
                (
                    [soft.bias_rate * rel.x, soft.bias_rate * rel.y],
                    soft.mass_scale,
                    soft.impulse_scale,
                )
            } else {
                ([0.0, 0.0], 1.0, 0.0)
            };
            let perp_x = perp_axis(quat_a, rel, DVec3::X);
            let perp_y = perp_axis(quat_a, rel, DVec3::Y);
            self.perp_x = perp_x;
            self.perp_y = perp_y;
            let sum = joint.ia + joint.ib;
            let kxx = perp_x.dot(sum * perp_x);
            let kyy = perp_y.dot(sum * perp_y);
            let kxy = perp_x.dot(sum * perp_y);
            let w = v.wb - v.wa;
            let cdot = [w.dot(perp_x) + bias[0], w.dot(perp_y) + bias[1]];
            let old = joint.impulses.perp;
            let solution = math::solve2(kxx, kxy, kyy, cdot);
            let delta = [
                -mass_scale * solution[0] - impulse_scale * old[0],
                -mass_scale * solution[1] - impulse_scale * old[1],
            ];
            joint.impulses.perp = [old[0] + delta[0], old[1] + delta[1]];
            v.turn(joint, delta[0] * perp_x + delta[1] * perp_y);
        }

        // The point constraint.
        {
            let ra = dqa * joint.frame_a.p;
            let rb = dqb * joint.frame_b.p;
            let cdot = v.relative(ra, rb);
            let (bias, mass_scale, impulse_scale) = if use_bias {
                let separation = (dpb - dpa) + (rb - ra) + joint.delta_center;
                (
                    soft.bias_rate * separation,
                    soft.mass_scale,
                    soft.impulse_scale,
                )
            } else {
                (DVec3::ZERO, 1.0, 0.0)
            };
            let k = math::point_mass(joint.ma, joint.mb, joint.ia, joint.ib, ra, rb);
            let b = math::solve3(k, cdot + bias);
            let impulse = -mass_scale * b - impulse_scale * joint.impulses.linear;
            joint.impulses.linear += impulse;
            v.push(joint, impulse, ra, rb);
        }

        v.write(joint, bodies);
    }
}
