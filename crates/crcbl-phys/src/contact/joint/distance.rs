//! The distance joint: Box3D's `src/distance_joint.c`
//! (`b3PrepareDistanceJoint`, `b3WarmStartDistanceJoint`,
//! `b3SolveDistanceJoint`), which is Box2D v3's `distance_joint.c` in three
//! dimensions. Box3D's spring-force range is not carried: its default is
//! unbounded, which is what this is.

use glam::DVec3;

use super::math::make_soft;
use super::{Common, Prepared as Joint, Velocities, limit_bias, motion};
use crate::components::RigidBody;
use crate::contact::solver::{Softness, SolverBody};
use crate::joint::DistanceJoint;

#[derive(Clone, Copy, Debug)]
pub(in crate::contact) struct Prepared {
    settings: DistanceJoint,
    axial_mass: f64,
    distance_softness: Softness,
}

impl Prepared {
    pub(super) fn new(settings: DistanceJoint, c: &Common) -> Self {
        let (ra, rb) = (c.frame_a.p, c.frame_b.p);
        let axis = (rb - ra + c.delta_center).normalize_or_zero();
        let cra = ra.cross(axis);
        let crb = rb.cross(axis);
        let k = c.ma + c.mb + cra.dot(c.ia * cra) + crb.dot(c.ib * crb);
        Self {
            settings,
            axial_mass: if k > 0.0 { 1.0 / k } else { 0.0 },
            distance_softness: make_soft(0.0, 0.0, 1.0),
        }
    }

    pub(super) fn soften(&mut self, h: f64) {
        self.distance_softness = make_soft(
            self.settings.spring.hertz,
            self.settings.spring.damping_ratio,
            h,
        );
    }

    /// The anchors, and the separation between them now.
    fn geometry(joint: &Joint, solver: &[SolverBody]) -> (DVec3, DVec3, DVec3) {
        let (dpa, dqa) = motion(solver, joint.a);
        let (dpb, dqb) = motion(solver, joint.b);
        let ra = dqa * joint.frame_a.p;
        let rb = dqb * joint.frame_b.p;
        let separation = joint.delta_center + (dpb - dpa) + (rb - ra);
        (ra, rb, separation)
    }

    pub(super) fn warm_start(
        &self,
        joint: &Joint,
        solver: &[SolverBody],
        bodies: &mut [RigidBody],
    ) {
        let (ra, rb, separation) = Self::geometry(joint, solver);
        let axis = separation.normalize_or_zero();
        let i = &joint.impulses;
        let mut v = Velocities::read(joint, bodies);
        v.push(
            joint,
            (i.axial + i.lower - i.upper + i.motor) * axis,
            ra,
            rb,
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
        let mut v = Velocities::read(joint, bodies);
        let (ra, rb, separation) = Self::geometry(joint, solver);
        let length = separation.length();
        let axis = separation.normalize_or_zero();
        let inverse_h = 1.0 / h;
        let soft = joint.softness;

        // Soft while its spring is on, unless its limits pin it to one length.
        if s.enable_spring && (s.min_length < s.max_length || !s.enable_limit) {
            if s.spring.hertz > 0.0 {
                let cdot = axis.dot(v.relative(ra, rb));
                let c = length - s.length;
                let bias = self.distance_softness.bias_rate * c;
                let m = self.distance_softness.mass_scale * self.axial_mass;
                let old = joint.impulses.axial;
                let impulse = -m * (cdot + bias) - self.distance_softness.impulse_scale * old;
                joint.impulses.axial = old + impulse;
                v.push(joint, impulse * axis, ra, rb);
            }

            if s.enable_motor {
                let cdot = axis.dot(v.relative(ra, rb));
                let impulse = self.axial_mass * (s.motor_speed - cdot);
                let old = joint.impulses.motor;
                let max = h * s.max_motor_force;
                joint.impulses.motor = (old + impulse).clamp(-max, max);
                v.push(joint, (joint.impulses.motor - old) * axis, ra, rb);
            }

            if s.enable_limit {
                // Lower.
                let cdot = axis.dot(v.relative(ra, rb));
                let (bias, mass_scale, impulse_scale) =
                    limit_bias(length - s.min_length, inverse_h, soft, use_bias);
                let old = joint.impulses.lower;
                let impulse = -mass_scale * self.axial_mass * (cdot + bias) - impulse_scale * old;
                joint.impulses.lower = (old + impulse).max(0.0);
                v.push(joint, (joint.impulses.lower - old) * axis, ra, rb);

                // Upper, its signs flipped to keep the gap and the impulse
                // positive.
                let cdot = -axis.dot(v.relative(ra, rb));
                let (bias, mass_scale, impulse_scale) =
                    limit_bias(s.max_length - length, inverse_h, soft, use_bias);
                let old = joint.impulses.upper;
                let impulse = -mass_scale * self.axial_mass * (cdot + bias) - impulse_scale * old;
                joint.impulses.upper = (old + impulse).max(0.0);
                v.push(joint, -(joint.impulses.upper - old) * axis, ra, rb);
            }
        } else {
            // Rigid.
            let cdot = axis.dot(v.relative(ra, rb));
            let c = length - s.length;
            let (bias, mass_scale, impulse_scale) = if use_bias {
                (soft.bias_rate * c, soft.mass_scale, soft.impulse_scale)
            } else {
                (0.0, 1.0, 0.0)
            };
            let impulse = -mass_scale * self.axial_mass * (cdot + bias)
                - impulse_scale * joint.impulses.axial;
            joint.impulses.axial += impulse;
            v.push(joint, impulse * axis, ra, rb);
        }

        v.write(joint, bodies);
    }
}
