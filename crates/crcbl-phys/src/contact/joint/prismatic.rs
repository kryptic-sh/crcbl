//! The prismatic joint, a slider along frame A's x-axis: Box3D's
//! `src/prismatic_joint.c` (`b3PreparePrismaticJoint`,
//! `b3WarmStartPrismaticJoint`, `b3SolvePrismaticJoint`).
//!
//! Five rows: the rotation lock (three) and the point held on the axis
//! (two), plus the spring, the motor and the two limits along it. As in
//! Box3D, a joint whose bodies cannot turn has no spring, motor or limits.

use glam::{DMat3, DQuat, DVec3};

use super::math::{self, make_soft};
use super::{Common, Prepared as Joint, Velocities, limit_bias, motion};
use crate::components::RigidBody;
use crate::contact::solver::{Softness, SolverBody};
use crate::joint::PrismaticJoint;

#[derive(Clone, Copy, Debug)]
pub(in crate::contact) struct Prepared {
    settings: PrismaticJoint,
    rotation_mass: DMat3,
    /// Frame A's axes at the tick's start: the slide, and the two across it.
    joint_axis: DVec3,
    perp_y: DVec3,
    perp_z: DVec3,
    spring_softness: Softness,
}

/// Where the joint stands now: the anchors, the vector between them, and
/// frame A's three axes turned with body A.
struct Geometry {
    ra: DVec3,
    rb: DVec3,
    d: DVec3,
    axis: DVec3,
    perp_y: DVec3,
    perp_z: DVec3,
}

impl Prepared {
    pub(super) fn new(settings: PrismaticJoint, c: &Common) -> Self {
        let sum = c.ia + c.ib;
        let axes = DMat3::from_quat(c.frame_a.q);
        Self {
            settings,
            rotation_mass: math::invert(sum),
            joint_axis: axes.x_axis,
            perp_y: axes.y_axis,
            perp_z: axes.z_axis,
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

    fn geometry(&self, joint: &Joint, solver: &[SolverBody]) -> Geometry {
        let (dpa, dqa) = motion(solver, joint.a);
        let (dpb, dqb) = motion(solver, joint.b);
        let ra = dqa * joint.frame_a.p;
        let rb = dqb * joint.frame_b.p;
        Geometry {
            ra,
            rb,
            d: (dpb - dpa) + joint.delta_center + (rb - ra),
            axis: dqa * self.joint_axis,
            perp_y: dqa * self.perp_y,
            perp_z: dqa * self.perp_z,
        }
    }

    pub(super) fn warm_start(
        &self,
        joint: &Joint,
        solver: &[SolverBody],
        bodies: &mut [RigidBody],
    ) {
        let g = self.geometry(joint, solver);
        let lever = g.ra + g.d;
        let i = &joint.impulses;
        let axial = i.spring + i.motor + i.lower - i.upper;
        let p = axial * g.axis + i.perp[0] * g.perp_y + i.perp[1] * g.perp_z;
        let la = axial * lever.cross(g.axis)
            + i.perp[0] * lever.cross(g.perp_y)
            + i.perp[1] * lever.cross(g.perp_z)
            + i.angular;
        let lb = axial * g.rb.cross(g.axis)
            + i.perp[0] * g.rb.cross(g.perp_y)
            + i.perp[1] * g.rb.cross(g.perp_z)
            + i.angular;
        let mut v = Velocities::read(joint, bodies);
        v.push_turning(joint, p, la, lb);
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
        let g = self.geometry(joint, solver);
        let lever = g.ra + g.d;
        let sax = lever.cross(g.axis);
        let sbx = g.rb.cross(g.axis);
        let translation = g.d.dot(g.axis);
        // Fresh every solve, as Box3D's is, so a stressed joint does not
        // diverge.
        let ka = joint.ma + joint.mb + sax.dot(joint.ia * sax) + sbx.dot(joint.ib * sbx);
        let axial_mass = if ka > 0.0 { 1.0 / ka } else { 0.0 };
        let along = |v: &Velocities| v.relative(lever, g.rb).dot(g.axis);

        if s.enable_spring && !joint.fixed_rotation {
            let c = translation - s.target_translation;
            let spring = self.spring_softness;
            let impulse = -spring.mass_scale * axial_mass * (along(&v) + spring.bias_rate * c)
                - spring.impulse_scale * joint.impulses.spring;
            joint.impulses.spring += impulse;
            v.push_turning(joint, impulse * g.axis, impulse * sax, impulse * sbx);
        }

        if s.enable_motor && !joint.fixed_rotation {
            let cdot = along(&v) - s.motor_speed;
            let old = joint.impulses.motor;
            let max = s.max_motor_force * h;
            joint.impulses.motor = (old - axial_mass * cdot).clamp(-max, max);
            let impulse = joint.impulses.motor - old;
            v.push_turning(joint, impulse * g.axis, impulse * sax, impulse * sbx);
        }

        if s.enable_limit && !joint.fixed_rotation {
            let speculative = 0.25 * (s.upper_translation - s.lower_translation);

            let c = translation - s.lower_translation;
            if c < speculative {
                let (bias, mass_scale, impulse_scale) = limit_bias(c, inverse_h, soft, use_bias);
                let old = joint.impulses.lower;
                let impulse = -mass_scale * axial_mass * (along(&v) + bias) - impulse_scale * old;
                joint.impulses.lower = (old + impulse).max(0.0);
                let impulse = joint.impulses.lower - old;
                v.push_turning(joint, impulse * g.axis, impulse * sax, impulse * sbx);
            } else {
                joint.impulses.lower = 0.0;
            }

            // Signs flipped on the rate and the impulse.
            let c = s.upper_translation - translation;
            if c < speculative {
                let (bias, mass_scale, impulse_scale) = limit_bias(c, inverse_h, soft, use_bias);
                let old = joint.impulses.upper;
                let impulse = -mass_scale * axial_mass * (-along(&v) + bias) - impulse_scale * old;
                joint.impulses.upper = (old + impulse).max(0.0);
                let impulse = old - joint.impulses.upper;
                v.push_turning(joint, impulse * g.axis, impulse * sax, impulse * sbx);
            } else {
                joint.impulses.upper = 0.0;
            }
        }

        // The rotation lock.
        if !joint.fixed_rotation {
            let (bias, mass_scale, impulse_scale) = if use_bias {
                let (_, dqa) = motion(solver, joint.a);
                let (_, dqb) = motion(solver, joint.b);
                let quat_a = dqa * joint.frame_a.q;
                let quat_b = dqb * joint.frame_b.q;
                let rel = math::inv_mul(quat_a, quat_b);
                let c = -(quat_a * math::delta_rotation(rel, DQuat::IDENTITY));
                (soft.bias_rate * c, soft.mass_scale, soft.impulse_scale)
            } else {
                (DVec3::ZERO, 1.0, 0.0)
            };
            let cdot = v.wb - v.wa;
            let impulse = -mass_scale * (self.rotation_mass * (cdot + bias))
                - impulse_scale * joint.impulses.angular;
            joint.impulses.angular += impulse;
            v.turn(joint, impulse);
        }

        // The point on the axis, as a 2×2 block.
        {
            let (bias, mass_scale, impulse_scale) = if use_bias {
                (
                    [
                        soft.bias_rate * g.perp_y.dot(g.d),
                        soft.bias_rate * g.perp_z.dot(g.d),
                    ],
                    soft.mass_scale,
                    soft.impulse_scale,
                )
            } else {
                ([0.0, 0.0], 1.0, 0.0)
            };
            let relative = v.relative(lever, g.rb);
            let cdot = [
                g.perp_y.dot(relative) + bias[0],
                g.perp_z.dot(relative) + bias[1],
            ];
            let say = lever.cross(g.perp_y);
            let sby = g.rb.cross(g.perp_y);
            let saz = lever.cross(g.perp_z);
            let sbz = g.rb.cross(g.perp_z);
            let (ma, mb, ia, ib) = (joint.ma, joint.mb, joint.ia, joint.ib);
            let kyy = ma + mb + say.dot(ia * say) + sby.dot(ib * sby);
            let kyz = say.dot(ia * saz) + sby.dot(ib * sbz);
            let kzz = ma + mb + saz.dot(ia * saz) + sbz.dot(ib * sbz);
            let old = joint.impulses.perp;
            let solution = math::solve2(kyy, kyz, kzz, cdot);
            let delta = [
                -mass_scale * solution[0] - impulse_scale * old[0],
                -mass_scale * solution[1] - impulse_scale * old[1],
            ];
            joint.impulses.perp = [old[0] + delta[0], old[1] + delta[1]];
            v.push_turning(
                joint,
                delta[0] * g.perp_y + delta[1] * g.perp_z,
                delta[0] * say + delta[1] * saz,
                delta[0] * sby + delta[1] * sbz,
            );
        }

        v.write(joint, bodies);
    }
}
