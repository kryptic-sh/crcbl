//! The spherical joint, a ball and socket with a cone and a twist limit:
//! Box3D's `src/spherical_joint.c` (`b3PrepareSphericalJoint`,
//! `b3WarmStartSphericalJoint`, `b3SolveSphericalJoint`), without its spring.
//!
//! The cone is about frame A's z-axis and bounds how far frame B's z-axis
//! tips from it; the twist is frame B's turn about its own z-axis. Both axes
//! are taken once a tick, as Box3D takes them.

use glam::DVec3;

use super::math;
use super::{Common, Impulses, Prepared as Joint, Velocities, limit_bias, motion};
use crate::components::RigidBody;
use crate::contact::solver::SolverBody;
use crate::joint::SphericalJoint;

#[derive(Clone, Copy, Debug)]
pub(in crate::contact) struct Prepared {
    settings: SphericalJoint,
    swing_axis: DVec3,
    swing_mass: f64,
    twist_jacobian: DVec3,
    twist_mass: f64,
    rotation_mass: glam::DMat3,
}

impl Prepared {
    pub(super) fn new(settings: SphericalJoint, c: &Common) -> Self {
        let sum = c.ia + c.ib;
        let cone_axis = c.frame_a.q * DVec3::Z;
        let twist_axis = c.frame_b.q * DVec3::Z;
        // Zero while the axes agree.
        let swing_axis = cone_axis.cross(twist_axis).normalize_or_zero();
        let mass = |axis: DVec3| {
            let k = axis.dot(sum * axis);
            if k > 0.0 { 1.0 / k } else { 0.0 }
        };

        let rel = math::inv_mul(c.frame_a.q, c.frame_b.q);
        let across = rel.z * rel.z + rel.w * rel.w;
        // A swing of π leaves the twist undefined; the cone axis stands in.
        let tan_half = if across > 0.0 {
            ((rel.x * rel.x + rel.y * rel.y) / across).sqrt()
        } else {
            0.0
        };
        let perp_axis = swing_axis.cross(cone_axis);
        let twist_jacobian = cone_axis + tan_half * perp_axis;

        Self {
            settings,
            swing_axis,
            swing_mass: mass(swing_axis),
            twist_jacobian,
            twist_mass: mass(twist_jacobian),
            rotation_mass: if math::is_fixed_rotation(sum) {
                glam::DMat3::ZERO
            } else {
                math::invert(sum)
            },
        }
    }

    /// The angular impulse the joint's rotational rows carry, as its warm
    /// start applies it.
    pub(super) fn angular_impulse(&self, i: &Impulses) -> DVec3 {
        i.motor_vector - i.swing * self.swing_axis + (i.lower - i.upper) * self.twist_jacobian
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
        let angular = self.angular_impulse(i);
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
        let rel = math::inv_mul(dqa * joint.frame_a.q, dqb * joint.frame_b.q);

        if s.enable_motor && !joint.fixed_rotation {
            let cdot = v.wb - v.wa;
            let lambda = -(self.rotation_mass * (cdot - s.motor_velocity));
            let mut new = joint.impulses.motor_vector + lambda;
            let length = new.length();
            let max = s.max_motor_torque * h;
            if length > max {
                new *= max / length;
            }
            let lambda = new - joint.impulses.motor_vector;
            joint.impulses.motor_vector = new;
            v.turn(joint, lambda);
        }

        if s.enable_twist_limit && !joint.fixed_rotation {
            let angle = math::twist_angle(rel);
            let jacobian = self.twist_jacobian;

            let (bias, mass_scale, impulse_scale) =
                limit_bias(angle - s.lower_twist_angle, inverse_h, soft, use_bias);
            let cdot = (v.wb - v.wa).dot(jacobian);
            let old = joint.impulses.lower;
            let impulse = -mass_scale * self.twist_mass * (cdot + bias) - impulse_scale * old;
            joint.impulses.lower = (old + impulse).max(0.0);
            v.turn(joint, (joint.impulses.lower - old) * jacobian);

            // Signs flipped on the rate and the impulse.
            let (bias, mass_scale, impulse_scale) =
                limit_bias(s.upper_twist_angle - angle, inverse_h, soft, use_bias);
            let cdot = (v.wa - v.wb).dot(jacobian);
            let old = joint.impulses.upper;
            let impulse = -mass_scale * self.twist_mass * (cdot + bias) - impulse_scale * old;
            joint.impulses.upper = (old + impulse).max(0.0);
            v.turn(joint, -(joint.impulses.upper - old) * jacobian);
        }

        if s.enable_cone_limit && !joint.fixed_rotation {
            let angle = math::swing_angle(rel);
            let axis = self.swing_axis;
            // Signs flipped on the rate and the impulse.
            let (bias, mass_scale, impulse_scale) =
                limit_bias(s.cone_angle - angle, inverse_h, soft, use_bias);
            let cdot = (v.wa - v.wb).dot(axis);
            let old = joint.impulses.swing;
            let impulse = -mass_scale * self.swing_mass * (cdot + bias) - impulse_scale * old;
            joint.impulses.swing = (old + impulse).max(0.0);
            v.turn(joint, -(joint.impulses.swing - old) * axis);
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
