//! The weld joint: Box3D's `src/weld_joint.c` (`b3PrepareWeldJoint`,
//! `b3WarmStartWeldJoint`, `b3SolveWeldJoint`) — the angular lock, then the
//! point constraint, each rigid at the joint's own stiffness or a spring of
//! its own.

use glam::{DMat3, DQuat, DVec3};

use super::math::{self, make_soft};
use super::{Common, Prepared as Joint, Velocities, motion};
use crate::components::RigidBody;
use crate::contact::solver::{Softness, SolverBody};
use crate::joint::WeldJoint;

#[derive(Clone, Copy, Debug)]
pub(in crate::contact) struct Prepared {
    settings: WeldJoint,
    angular_mass: DMat3,
    linear_spring: Softness,
    angular_spring: Softness,
}

impl Prepared {
    pub(super) fn new(settings: WeldJoint, c: &Common) -> Self {
        Self {
            settings,
            angular_mass: math::invert(c.ia + c.ib),
            linear_spring: make_soft(0.0, 0.0, 1.0),
            angular_spring: make_soft(0.0, 0.0, 1.0),
        }
    }

    /// A spring of zero hertz is the joint's own rigid softness.
    pub(super) fn soften(&mut self, h: f64, constraint: Softness) {
        let spring = |hertz: f64, zeta: f64| {
            if hertz == 0.0 {
                constraint
            } else {
                make_soft(hertz, zeta, h)
            }
        };
        let s = self.settings;
        self.linear_spring = spring(s.linear.hertz, s.linear.damping_ratio);
        self.angular_spring = spring(s.angular.hertz, s.angular.damping_ratio);
    }

    pub(super) fn solve(
        &mut self,
        joint: &mut Joint,
        solver: &[SolverBody],
        bodies: &mut [RigidBody],
        use_bias: bool,
    ) {
        let s = self.settings;
        let mut v = Velocities::read(joint, bodies);
        let (dpa, dqa) = motion(solver, joint.a);
        let (dpb, dqb) = motion(solver, joint.b);
        let quat_a = dqa * joint.frame_a.q;
        let mut quat_b = dqb * joint.frame_b.q;
        if quat_a.dot(quat_b) < 0.0 {
            quat_b = -quat_b;
        }
        let rel = math::inv_mul(quat_a, quat_b);

        if !joint.fixed_rotation {
            let (bias, mass_scale, impulse_scale) = if use_bias || s.angular.hertz > 0.0 {
                let c = -(quat_a * math::delta_rotation(rel, DQuat::IDENTITY));
                let spring = self.angular_spring;
                (
                    spring.bias_rate * c,
                    spring.mass_scale,
                    spring.impulse_scale,
                )
            } else {
                (DVec3::ZERO, 1.0, 0.0)
            };
            let cdot = v.wb - v.wa;
            let impulse = -mass_scale * (self.angular_mass * (cdot + bias))
                - impulse_scale * joint.impulses.angular;
            joint.impulses.angular += impulse;
            v.turn(joint, impulse);
        }

        {
            let ra = dqa * joint.frame_a.p;
            let rb = dqb * joint.frame_b.p;
            let cdot = v.relative(ra, rb);
            let (bias, mass_scale, impulse_scale) = if use_bias || s.linear.hertz > 0.0 {
                let separation = (dpb - dpa) + (rb - ra) + joint.delta_center;
                let spring = self.linear_spring;
                (
                    spring.bias_rate * separation,
                    spring.mass_scale,
                    spring.impulse_scale,
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

pub(super) fn warm_start(joint: &Joint, solver: &[SolverBody], bodies: &mut [RigidBody]) {
    let (_, dqa) = motion(solver, joint.a);
    let (_, dqb) = motion(solver, joint.b);
    let ra = dqa * joint.frame_a.p;
    let rb = dqb * joint.frame_b.p;
    let i = &joint.impulses;
    let mut v = Velocities::read(joint, bodies);
    v.push_turning(
        joint,
        i.linear,
        ra.cross(i.linear) + i.angular,
        rb.cross(i.linear) + i.angular,
    );
    v.write(joint, bodies);
}
