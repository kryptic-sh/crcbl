//! The arithmetic Box3D's joint solvers share, transcribed from its
//! `include/box3d/math_functions.h`, `src/math_internal.h` and
//! `src/math_functions.c` (github.com/erincatto/box3d, commit `9e5a4cde`),
//! in `f64`.

use glam::{DMat3, DQuat, DVec3};

use crate::contact::solver::Softness;

/// The smallest determinant a solve divides by: Box3D's `1000.0f * FLT_MIN`,
/// at `f64`'s smallest normal.
const TINY_DETERMINANT: f64 = 1000.0 * f64::MIN_POSITIVE;

/// A soft constraint's coefficients — Box2D v3's and Box3D's `b3MakeSoft`,
/// **including its zero**: a spring of zero hertz has no mass and no bias,
/// so it does nothing, where [`Softness::new`]'s contact form makes it rigid.
pub(super) fn make_soft(hertz: f64, zeta: f64, h: f64) -> Softness {
    if hertz == 0.0 {
        return Softness {
            bias_rate: 0.0,
            mass_scale: 0.0,
            impulse_scale: 0.0,
        };
    }
    Softness::new(hertz, zeta, h)
}

/// `b3Atan2`: a minimax polynomial for `atan` on `[0, 1]`, mapped to the whole
/// circle, and zero at the origin as `atan2f` is.
///
/// `crcbl-phys` calls no platform transcendental (see its `clippy.toml`), so
/// the joint angles use Box3D's own. Its error is measured by
/// `the_arc_tangent_is_within_its_bound`: under 3e-5 rad, a float's worth,
/// which is far inside anything a joint limit is set to.
pub(crate) fn atan2(y: f64, x: f64) -> f64 {
    if x == 0.0 && y == 0.0 {
        return 0.0;
    }
    let ax = x.abs();
    let ay = y.abs();
    let mx = ay.max(ax);
    let mn = ay.min(ax);
    let a = mn / mx;

    let s = a * a;
    let c = s * a;
    let q = s * s;
    let mut r = 0.024_840_285 * q + 0.186_814_18;
    let t = -0.094_097_948 * q - 0.332_130_72;
    r = r * s + t;
    r = r * c + a;

    if ay > ax {
        r = core::f64::consts::FRAC_PI_2 - r;
    }
    if x < 0.0 {
        r = core::f64::consts::PI - r;
    }
    if y < 0.0 {
        r = -r;
    }
    r
}

/// `b3GetTwistAngle`: the turn of `q` about its z-axis, in `[-π, π]`, with
/// the quaternion's sign taken so the angle stays in range.
pub(crate) fn twist_angle(q: DQuat) -> f64 {
    let twist = if q.w < 0.0 {
        atan2(-q.z, -q.w)
    } else {
        atan2(q.z, q.w)
    };
    2.0 * twist
}

/// `b3GetSwingAngle`: how far `q` tips its z-axis, in `[0, π]` — the angle
/// a cone limit bounds.
pub(crate) fn swing_angle(q: DQuat) -> f64 {
    let x = (q.z * q.z + q.w * q.w).sqrt();
    let y = (q.x * q.x + q.y * q.y).sqrt();
    2.0 * atan2(y, x)
}

/// `b3InvMulQuat`: `q1⁻¹ q2` for unit quaternions.
pub(super) fn inv_mul(q1: DQuat, q2: DQuat) -> DQuat {
    q1.conjugate() * q2
}

/// `b3DeltaQuatToRotation`: the rotation vector, to first order, that turns
/// `q` onto `target`, with `q`'s sign taken to face `target`.
pub(super) fn delta_rotation(q: DQuat, target: DQuat) -> DVec3 {
    let s = if q.dot(target) < 0.0 { -q } else { q };
    let diff = DQuat::from_xyzw(
        target.x - s.x,
        target.y - s.y,
        target.z - s.z,
        target.w - s.w,
    );
    let product = diff * s.conjugate();
    2.0 * DVec3::new(product.x, product.y, product.z)
}

/// `b3Skew`: the matrix that crosses `v` with what it multiplies.
pub(super) fn skew(v: DVec3) -> DMat3 {
    DMat3::from_cols(
        DVec3::new(0.0, v.z, -v.y),
        DVec3::new(-v.z, 0.0, v.x),
        DVec3::new(v.y, -v.x, 0.0),
    )
}

/// `b3Solve3`: `m⁻¹ a` by Cramer's rule, or zero for a singular `m`.
pub(super) fn solve3(m: DMat3, a: DVec3) -> DVec3 {
    let det = m.determinant();
    if det.abs() > TINY_DETERMINANT {
        let inv = 1.0 / det;
        let sx = m.y_axis.cross(m.z_axis);
        let sy = m.z_axis.cross(m.x_axis);
        let sz = m.x_axis.cross(m.y_axis);
        DVec3::new(inv * sx.dot(a), inv * sy.dot(a), inv * sz.dot(a))
    } else {
        DVec3::ZERO
    }
}

/// `b3InvertMatrix`: `m⁻¹`, or zero for a singular `m`.
pub(super) fn invert(m: DMat3) -> DMat3 {
    let det = m.determinant();
    if det.abs() > TINY_DETERMINANT {
        let inv = 1.0 / det;
        DMat3::from_cols(
            m.y_axis.cross(m.z_axis) * inv,
            m.z_axis.cross(m.x_axis) * inv,
            m.x_axis.cross(m.y_axis) * inv,
        )
        .transpose()
    } else {
        DMat3::ZERO
    }
}

/// Whether `m` — a sum of inverse inertias — is singular enough that the
/// joint cannot turn either body: Box3D's `fixedRotation`.
pub(super) fn is_fixed_rotation(m: DMat3) -> bool {
    m.determinant() < TINY_DETERMINANT
}

/// `b3Solve2` on the symmetric `[[xx, xy], [xy, yy]]`: its inverse times
/// `b`, or zero where the determinant is not positive.
pub(super) fn solve2(xx: f64, xy: f64, yy: f64, b: [f64; 2]) -> [f64; 2] {
    let det = xx * yy - xy * xy;
    if det > TINY_DETERMINANT {
        let inv = 1.0 / det;
        [
            inv * yy * b[0] - inv * xy * b[1],
            -inv * xy * b[0] + inv * xx * b[1],
        ]
    } else {
        [0.0, 0.0]
    }
}

/// The point constraint's effective mass matrix, `(mA + mB) E − [rA]× IA⁻¹
/// [rA]× − [rB]× IB⁻¹ [rB]×`, as Box3D builds it in each joint's
/// point-to-point block.
pub(super) fn point_mass(ma: f64, mb: f64, ia: DMat3, ib: DMat3, ra: DVec3, rb: DVec3) -> DMat3 {
    let sa = skew(ra);
    let sb = skew(rb);
    let k = -(sa * ia * sa + sb * ib * sb);
    k + DMat3::from_diagonal(DVec3::splat(ma + mb))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::integrator::rotation_from_scaled_axis;

    /// **Box3D's arc tangent is good to 3e-5 rad** over the whole circle —
    /// measured against the platform's, which is fine to read in a test —
    /// and exact at the axes.
    #[test]
    #[expect(
        clippy::disallowed_methods,
        reason = "the platform's atan2 is the reference the transcription is measured against"
    )]
    fn the_arc_tangent_is_within_its_bound() {
        let mut worst: f64 = 0.0;
        for k in 0..3600 {
            let angle = -core::f64::consts::PI + f64::from(k) * core::f64::consts::TAU / 3600.0;
            let (y, x) = (crcbl_core::trig::sin(angle), crcbl_core::trig::cos(angle));
            for scale in [1.0e-3, 1.0, 7.0e2] {
                worst =
                    worst.max((atan2(y * scale, x * scale) - (y * scale).atan2(x * scale)).abs());
            }
        }
        // Measured on 2026-09-23: 2.76e-5 rad at worst.
        assert!(worst < 3.0e-5, "worst error {worst:e} rad");
        assert!(worst > 0.0, "an exact arc tangent would not need a bound");
        assert_eq!(atan2(0.0, 0.0), 0.0);
        assert_eq!(atan2(0.0, 1.0), 0.0);
    }

    /// A turn about z reads back as its twist, with no swing; a tip of the
    /// z-axis reads back as its swing, with no twist — each to twice the arc
    /// tangent's bound, since both are twice an arc tangent.
    #[test]
    fn twist_and_swing_read_back_what_made_them() {
        for angle in [-2.5, -0.7, 0.0, 0.3, 1.9, 3.0] {
            let twist = rotation_from_scaled_axis(DVec3::Z * angle);
            assert!((twist_angle(twist) - angle).abs() < 6.0e-5, "{angle}");
            assert!(swing_angle(twist).abs() < 1.0e-12, "{angle}");
            let swing = rotation_from_scaled_axis(DVec3::new(0.6, 0.8, 0.0) * angle.abs());
            assert!((swing_angle(swing) - angle.abs()).abs() < 6.0e-5, "{angle}");
            assert!(twist_angle(swing).abs() < 1.0e-12, "{angle}");
        }
    }

    /// The first-order rotation error is the small rotation that takes a
    /// quaternion back to its target.
    #[test]
    fn the_delta_rotation_undoes_a_small_turn() {
        let turn = DVec3::new(0.01, -0.02, 0.015);
        let q = rotation_from_scaled_axis(turn);
        let back = delta_rotation(q, DQuat::IDENTITY);
        assert!((back + turn).length() < 1.0e-5, "{back:?}");
    }

    /// Cramer's rule inverts, and refuses a singular matrix.
    #[test]
    fn the_solves_invert_and_refuse_singular_matrices() {
        let m = DMat3::from_cols(
            DVec3::new(4.0, 1.0, 0.5),
            DVec3::new(1.0, 3.0, 0.2),
            DVec3::new(0.5, 0.2, 2.0),
        );
        let a = DVec3::new(1.0, -2.0, 0.5);
        assert!((m * solve3(m, a) - a).length() < 1.0e-12);
        assert!((m * invert(m) - DMat3::IDENTITY).abs_diff_eq(DMat3::ZERO, 1.0e-12));
        assert_eq!(solve3(DMat3::ZERO, a), DVec3::ZERO);
        let x = solve2(4.0, 1.0, 3.0, [1.0, 2.0]);
        assert!((4.0 * x[0] + x[1] - 1.0).abs() < 1.0e-12);
        assert!((x[0] + 3.0 * x[1] - 2.0).abs() < 1.0e-12);
        assert_eq!(solve2(0.0, 0.0, 0.0, [1.0, 1.0]), [0.0, 0.0]);
    }
}
