//! The spherical joint, a ball and socket with a swing and a twist limit:
//! Box3D's `src/spherical_joint.c` (`b3PrepareSphericalJoint`,
//! `b3WarmStartSphericalJoint`, `b3SolveSphericalJoint`), without its spring.
//!
//! The swing is about frame A's z-axis and bounds how far frame B's z-axis
//! tips from it; the twist is frame B's turn about its own z-axis. The twist
//! axis and a circular swing's are taken once a tick, as Box3D takes them;
//! an elliptical swing's, whose normal turns as the swing moves round it,
//! afresh each solve.
//!
//! A circular swing is Box3D's cone, solved as Box3D solves it. An
//! elliptical one is Jolt's (github.com/jrouwe/JoltPhysics,
//! `Jolt/Physics/Constraints/ConstraintPart/SwingTwistConstraintPart.h`,
//! `ESwingType::Cone`): the swing's quaternion x and y parts kept inside the
//! ellipse whose radii are the sines of the half-angles' halves, pushed back
//! from the closest point on it — see [`math::ellipse_gap`]. Each limit's
//! soft zone ([`crate::joint`]'s soft zones) is a row of its own, before the
//! hard one, along the same axis.

use glam::{DQuat, DVec3};

use super::math;
use super::{Common, Impulses, Prepared as Joint, Velocities, limit_bias, motion, soft_zone};
use crate::components::RigidBody;
use crate::contact::solver::SolverBody;
use crate::joint::SphericalJoint;

/// A swing limit: Box3D's circular cone, or Jolt's ellipse.
#[derive(Clone, Copy, Debug)]
enum Swing {
    /// The cone's half-angle.
    Cone(f64),
    /// The ellipse's radii in the swing quaternion's x and y parts: the
    /// sines of its half-angles' halves.
    Ellipse(f64, f64),
}

impl Swing {
    /// The limit of half-angles `x` and `y`. An ellipse's radii are both
    /// above zero, since `JointKind::invalid` refuses a swing with one zero
    /// half-angle and the other not, and the half-angles lie in `[0, π]`.
    fn new(x: f64, y: f64) -> Self {
        if x == y {
            Self::Cone(x)
        } else {
            Self::Ellipse(
                crcbl_core::trig::sin(0.5 * x),
                crcbl_core::trig::sin(0.5 * y),
            )
        }
    }

    /// How far `rel` is inside it, as an angle, negative past it; and for an
    /// ellipse, the world axis about which frame B turning takes the gap
    /// away, frame A standing at `frame_a`. A cone's axis is frame A's
    /// z-axis crossed with frame B's, taken once a tick as Box3D takes it;
    /// an ellipse's normal turns as the swing moves round it, so it is
    /// taken afresh each solve, as the hinge's collinearity axes are.
    fn gap(self, rel: DQuat, frame_a: DQuat) -> (f64, Option<DVec3>) {
        match self {
            Self::Cone(angle) => (angle - math::swing_angle(rel), None),
            Self::Ellipse(a, b) => {
                let (gap, axis) = math::ellipse_gap(a, b, rel);
                (gap, Some(frame_a * axis))
            }
        }
    }
}

/// The effective mass of a turn about `axis` for bodies whose inverse
/// inertias sum to `sum`, or zero for none.
fn axis_mass(sum: glam::DMat3, axis: DVec3) -> f64 {
    let k = axis.dot(sum * axis);
    if k > 0.0 { 1.0 / k } else { 0.0 }
}

/// How far `rel` is inside joint `s`'s swing limit, as an angle, negative
/// past it.
pub(super) fn swing_gap(s: &SphericalJoint, rel: DQuat) -> f64 {
    Swing::new(s.swing_x_angle, s.swing_y_angle)
        .gap(rel, DQuat::IDENTITY)
        .0
}

#[derive(Clone, Copy, Debug)]
pub(in crate::contact) struct Prepared {
    settings: SphericalJoint,
    swing: Swing,
    swing_axis: DVec3,
    swing_mass: f64,
    soft_swing: Swing,
    soft_swing_axis: DVec3,
    soft_swing_mass: f64,
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
        let radial_axis = cone_axis.cross(twist_axis).normalize_or_zero();
        let mass = |axis: DVec3| axis_mass(sum, axis);

        let rel = math::inv_mul(c.frame_a.q, c.frame_b.q);
        let swing = Swing::new(settings.swing_x_angle, settings.swing_y_angle);
        let swing_axis = if settings.enable_swing_limit {
            swing.gap(rel, c.frame_a.q).1.unwrap_or(radial_axis)
        } else {
            radial_axis
        };
        let soft_swing = Swing::new(settings.soft_swing_x_angle, settings.soft_swing_y_angle);
        let soft_swing_axis = if settings.enable_soft_swing {
            soft_swing.gap(rel, c.frame_a.q).1.unwrap_or(radial_axis)
        } else {
            DVec3::ZERO
        };
        let across = rel.z * rel.z + rel.w * rel.w;
        // A swing of π leaves the twist undefined; the cone axis stands in.
        let tan_half = if across > 0.0 {
            ((rel.x * rel.x + rel.y * rel.y) / across).sqrt()
        } else {
            0.0
        };
        // Box3D's twist Jacobian is built on the radial swing axis, square to
        // both z-axes — not on an ellipse's limit axis, which points
        // elsewhere off the ellipse's own axes.
        let perp_axis = radial_axis.cross(cone_axis);
        let twist_jacobian = cone_axis + tan_half * perp_axis;

        Self {
            settings,
            swing,
            swing_axis,
            swing_mass: mass(swing_axis),
            soft_swing,
            soft_swing_axis,
            soft_swing_mass: mass(soft_swing_axis),
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
        let mut angular =
            i.motor_vector - i.swing * self.swing_axis + (i.lower - i.upper) * self.twist_jacobian;
        if self.settings.enable_soft_twist {
            angular += (i.soft_lower - i.soft_upper) * self.twist_jacobian;
        }
        if self.settings.enable_soft_swing {
            angular -= i.soft_swing * self.soft_swing_axis;
        }
        angular
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

        if s.enable_soft_twist && !joint.fixed_rotation {
            let angle = math::twist_angle(rel);
            let jacobian = self.twist_jacobian;
            let zone = s.soft_twist;

            let old = joint.impulses.soft_lower;
            joint.impulses.soft_lower = soft_zone(
                angle - s.soft_lower_twist_angle,
                angle - s.lower_twist_angle,
                (v.wb - v.wa).dot(jacobian),
                self.twist_mass,
                old,
                zone,
                h,
            );
            v.turn(joint, (joint.impulses.soft_lower - old) * jacobian);

            let old = joint.impulses.soft_upper;
            joint.impulses.soft_upper = soft_zone(
                s.soft_upper_twist_angle - angle,
                s.upper_twist_angle - angle,
                (v.wa - v.wb).dot(jacobian),
                self.twist_mass,
                old,
                zone,
                h,
            );
            v.turn(joint, -(joint.impulses.soft_upper - old) * jacobian);
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

        // The soft swing needs the hard one, so this is read once for both.
        let quat_a = dqa * joint.frame_a.q;
        let sum = joint.ia + joint.ib;
        let swing_gap = if s.enable_swing_limit && !joint.fixed_rotation {
            let (gap, axis) = self.swing.gap(rel, quat_a);
            if let Some(axis) = axis {
                self.swing_axis = axis;
                self.swing_mass = axis_mass(sum, axis);
            }
            gap
        } else {
            0.0
        };

        if s.enable_soft_swing && !joint.fixed_rotation {
            let (soft_gap, axis) = self.soft_swing.gap(rel, quat_a);
            if let Some(axis) = axis {
                self.soft_swing_axis = axis;
                self.soft_swing_mass = axis_mass(sum, axis);
            }
            let axis = self.soft_swing_axis;
            let old = joint.impulses.soft_swing;
            joint.impulses.soft_swing = soft_zone(
                soft_gap,
                swing_gap,
                (v.wa - v.wb).dot(axis),
                self.soft_swing_mass,
                old,
                s.soft_swing,
                h,
            );
            v.turn(joint, -(joint.impulses.soft_swing - old) * axis);
        }

        if s.enable_swing_limit && !joint.fixed_rotation {
            let axis = self.swing_axis;
            // Signs flipped on the rate and the impulse.
            let (bias, mass_scale, impulse_scale) =
                limit_bias(swing_gap, inverse_h, soft, use_bias);
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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::contact::joint::Frame;
    use crate::integrator::rotation_from_scaled_axis;
    use glam::DMat3;

    /// **The twist rows read the twist's own rate, an ellipse on or not.** At
    /// a diagonal swing inside an elliptical limit, frame B turned at `ω` for
    /// a moment changes its twist at the Jacobian's dot `ω`, to second order —
    /// for turns about every axis. Built on the ellipse's limit axis instead of
    /// the radial one, it read 0.167 rad/s of twist from a turn of 1 rad/s
    /// about frame A's x-axis that twists it at 0.418 (measured on
    /// 2026-11-01).
    #[test]
    fn the_twist_jacobian_is_the_twists_rate() {
        let swing = rotation_from_scaled_axis(DVec3::new(0.15, 1.0, 0.0).normalize() * 0.8);
        let twist = rotation_from_scaled_axis(DVec3::Z * 0.1);
        let frame_b = swing * twist;
        let common = Common {
            ma: 1.0,
            mb: 1.0,
            ia: DMat3::IDENTITY,
            ib: DMat3::IDENTITY,
            frame_a: Frame {
                p: DVec3::ZERO,
                q: DQuat::IDENTITY,
            },
            frame_b: Frame {
                p: DVec3::ZERO,
                q: frame_b,
            },
            delta_center: DVec3::ZERO,
        };
        for settings in [
            SphericalJoint::ball()
                .with_swing(0.3, 1.2)
                .with_twist(-0.2, 0.2),
            SphericalJoint::ball().with_cone(1.2).with_twist(-0.2, 0.2),
        ] {
            let prepared = Prepared::new(settings, &common);
            let dt = 1.0e-6;
            for omega in [
                DVec3::X,
                DVec3::Y,
                DVec3::Z,
                frame_b * DVec3::Z,
                DVec3::new(0.3, -0.7, 0.5),
            ] {
                let turned = rotation_from_scaled_axis(omega * dt) * frame_b;
                let rate = (math::twist_angle(turned) - math::twist_angle(frame_b)) / dt;
                let read = prepared.twist_jacobian.dot(omega);
                assert!(
                    (rate - read).abs() < 1.0e-4,
                    "{settings:?} at {omega:?}: twist turns at {rate}, read {read}"
                );
            }
        }
    }
}
