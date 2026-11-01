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

/// The swing part of `q` — `q = swing twist`, the twist about z — as its x
/// and y parts and its w, which is never negative: Jolt's
/// `Quat::GetSwingTwist` (github.com/jrouwe/JoltPhysics,
/// `Jolt/Math/Quat.inl`) with its twist axis moved from x to z. Its z part
/// is zero. The same for `q` and `−q`; a swing of π, which leaves the twist
/// undefined, is `q` itself.
pub(crate) fn swing_part(q: DQuat) -> (f64, f64, f64) {
    let s = (q.z * q.z + q.w * q.w).sqrt();
    if s > 0.0 {
        ((q.w * q.x - q.z * q.y) / s, (q.w * q.y + q.z * q.x) / s, s)
    } else {
        (q.x, q.y, 0.0)
    }
}

/// The z-axis turned by the swing whose x, y and w parts these are: frame
/// B's z-axis in frame A's.
pub(super) fn swung_z(x: f64, y: f64, w: f64) -> DVec3 {
    DVec3::new(2.0 * w * y, -2.0 * w * x, 1.0 - 2.0 * (x * x + y * y))
}

/// The most halvings [`closest_on_ellipse`] takes: more than enough to reach
/// adjacent `f64`s on its bracket, where it stops first.
const ELLIPSE_BISECTIONS: u32 = 200;

/// The point of the ellipse `(x/a)² + (y/b)² = 1` closest to `(x, y)`, and
/// whether `(x, y)` is inside it or on it: David Eberly's robust bisection,
/// "Distance from a Point to an Ellipse, an Ellipsoid, or a
/// Hyperellipsoid" (Geometric Tools, 2013), `DistancePointEllipse` and its
/// `GetRoot`. Both radii are above zero: a spherical joint whose swing has
/// one zero half-angle is refused (see `JointKind::invalid`).
pub(super) fn closest_on_ellipse(a: f64, b: f64, x: f64, y: f64) -> ((f64, f64), bool) {
    // Eberly's form: the first quadrant, the longer axis first.
    let swap = a < b;
    let (e0, e1, y0, y1) = if swap {
        (b, a, y.abs(), x.abs())
    } else {
        (a, b, x.abs(), y.abs())
    };
    let z0 = y0 / e0;
    let z1 = y1 / e1;
    let g = z0 * z0 + z1 * z1 - 1.0;
    let (x0, x1) = if y1 > 0.0 {
        if y0 > 0.0 {
            if g == 0.0 {
                (y0, y1)
            } else {
                let r0 = (e0 / e1) * (e0 / e1);
                let s = ellipse_root(r0, z0, z1, g);
                (r0 * y0 / (s + r0), y1 / (s + 1.0))
            }
        } else {
            (0.0, e1)
        }
    } else {
        let numer0 = e0 * y0;
        let denom0 = e0 * e0 - e1 * e1;
        if numer0 < denom0 {
            let xde0 = numer0 / denom0;
            (e0 * xde0, e1 * (1.0 - xde0 * xde0).sqrt())
        } else {
            (e0, 0.0)
        }
    };
    let (x0, x1) = if swap { (x1, x0) } else { (x0, x1) };
    ((x0.copysign(x), x1.copysign(y)), g <= 0.0)
}

/// Eberly's `GetRoot`: the root `s` of `(r0 z0 / (s + r0))² + (z1 / (s +
/// 1))² = 1` by bisection, `g` its left side less one at `s = 0`.
fn ellipse_root(r0: f64, z0: f64, z1: f64, g: f64) -> f64 {
    let n0 = r0 * z0;
    let mut s0 = z1 - 1.0;
    let mut s1 = if g < 0.0 {
        0.0
    } else {
        (n0 * n0 + z1 * z1).sqrt() - 1.0
    };
    let mut s = 0.0;
    for _ in 0..ELLIPSE_BISECTIONS {
        s = 0.5 * (s0 + s1);
        if s == s0 || s == s1 {
            break;
        }
        let ratio0 = n0 / (s + r0);
        let ratio1 = z1 / (s + 1.0);
        let g = ratio0 * ratio0 + ratio1 * ratio1 - 1.0;
        if g > 0.0 {
            s0 = s;
        } else if g < 0.0 {
            s1 = s;
        } else {
            break;
        }
    }
    s
}

/// How far the swing of `q` is inside an elliptical limit of radii `a` and
/// `b` — the sines of its half-angles' halves, both above zero — as an angle,
/// negative past it; and the axis in frame A, unit or zero, about which
/// frame B turning takes that gap away.
///
/// Jolt's `SwingTwistConstraintPart` (`ESwingType::Cone`) bounds the swing's
/// x and y parts by this ellipse and, past it, pushes back from the closest
/// point on it. Here the gap is the angle between frame B's z-axis and the
/// closest point's, inside the ellipse too — where Jolt does nothing — so the
/// limit is speculative as every other one in the solver is, and never
/// reads more room than the nearest boundary leaves. The axis is the one
/// about which a turn grows the ellipse's function fastest, taken through
/// frame B's z-axis, so it is square to that axis — a turn about it does not
/// tip B — and counts the slide round an ellipse that a turn about frame A's
/// z-axis makes, which a circle does not have; on a circle it is the
/// swing's own axis, as Box3D's cone takes it. At zero swing there is no
/// direction and no axis, as Box3D's cone has none.
pub(super) fn ellipse_gap(a: f64, b: f64, q: DQuat) -> (f64, DVec3) {
    let (x, y, w) = swing_part(q);
    let ((cx, cy), inside) = closest_on_ellipse(a, b, x, y);
    let cw = (1.0 - cx * cx - cy * cy).max(0.0).sqrt();
    let now = swung_z(x, y, w);
    let limit = swung_z(cx, cy, cw);
    let angle = atan2(now.cross(limit).length(), now.dot(limit));
    // The swing's x and y parts through frame B's z-axis `d` are
    // `(−d.y, d.x) / √(2 (1 + d.z))`, so the ellipse's function is
    // `g = (d.y²/a² + d.x²/b²) / (2 (1 + d.z))`, and a turn `ω` changes it at
    // `ω · (d × ∇g)`: that axis, from the gradient scaled by `(1 + d.z)²` so
    // it stays finite near a swing of π. Zero at zero swing.
    let along = 1.0 + now.z;
    let n = now.y * now.y / (a * a) + now.x * now.x / (b * b);
    let gradient = DVec3::new(now.x * along / (b * b), now.y * along / (a * a), -0.5 * n);
    (
        if inside { angle } else { -angle },
        now.cross(gradient).normalize_or_zero(),
    )
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

    /// The swing part times the twist about z is the rotation it came from,
    /// its z part is zero and its w not negative, for either sign of the
    /// quaternion — and at a swing of π, where the twist is undefined, it is
    /// the rotation itself.
    #[test]
    fn the_swing_part_and_the_twist_make_the_rotation() {
        for (swing, twist) in [
            (DVec3::new(0.3, -0.5, 0.0), 0.7),
            (DVec3::new(-1.2, 0.4, 0.0), -2.9),
            (DVec3::new(0.0, 3.1, 0.0), 1.0),
        ] {
            let q = rotation_from_scaled_axis(swing) * rotation_from_scaled_axis(DVec3::Z * twist);
            for q in [q, -q] {
                let (x, y, w) = swing_part(q);
                assert!(w >= 0.0);
                let s = DQuat::from_xyzw(x, y, 0.0, w);
                let t = s.conjugate() * q;
                assert!(t.x.abs() < 1.0e-12 && t.y.abs() < 1.0e-12, "{t:?}");
                assert!((swung_z(x, y, w) - q * DVec3::Z).length() < 1.0e-12);
            }
        }
        let half_turn =
            rotation_from_scaled_axis(DVec3::new(0.6, 0.8, 0.0) * core::f64::consts::PI);
        let (x, y, w) = swing_part(half_turn);
        assert!(
            (swung_z(x, y, w) + DVec3::Z).length() < 1.0e-12,
            "{x} {y} {w}"
        );
    }

    /// The closest point on an ellipse lies on it and is the nearest of a
    /// dense sampling of it, inside and outside, in every quadrant and with
    /// either axis the longer; and on an axis it is the known one.
    #[test]
    fn the_closest_point_on_an_ellipse_is_the_nearest() {
        assert_eq!(closest_on_ellipse(2.0, 1.0, 3.0, 0.0), ((2.0, 0.0), false));
        assert_eq!(
            closest_on_ellipse(2.0, 1.0, 0.0, -3.0),
            ((0.0, -1.0), false)
        );
        assert_eq!(closest_on_ellipse(2.0, 1.0, 0.0, 0.5), ((0.0, 1.0), true));
        // Inside on the long axis, the nearest point is off it.
        let ((x, y), inside) = closest_on_ellipse(2.0, 1.0, 0.5, 0.0);
        assert!(inside && (x - 2.0 / 3.0).abs() < 1.0e-12, "{x}");
        assert!((y - (8.0f64 / 9.0).sqrt()).abs() < 1.0e-12, "{y}");

        let samples = 200_000;
        for (a, b) in [(0.7, 0.3), (0.2, 0.9)] {
            let ring: Vec<(f64, f64)> = (0..samples)
                .map(|k| {
                    let t = core::f64::consts::TAU * f64::from(k) / f64::from(samples);
                    (a * crcbl_core::trig::cos(t), b * crcbl_core::trig::sin(t))
                })
                .collect();
            for (x, y) in [
                (0.1, 0.05),
                (-0.5, 0.2),
                (0.9, -0.8),
                (-0.05, -0.6),
                (0.3, 0.0),
            ] {
                let ((cx, cy), inside) = closest_on_ellipse(a, b, x, y);
                let on = (cx / a) * (cx / a) + (cy / b) * (cy / b);
                assert!((on - 1.0).abs() < 1.0e-12, "({x}, {y}) on {on}");
                assert_eq!(inside, (x / a) * (x / a) + (y / b) * (y / b) <= 1.0);
                let found = ((cx - x) * (cx - x) + (cy - y) * (cy - y)).sqrt();
                let nearest = ring
                    .iter()
                    .map(|&(rx, ry)| ((rx - x) * (rx - x) + (ry - y) * (ry - y)).sqrt())
                    .fold(f64::INFINITY, f64::min);
                assert!(
                    found <= nearest + 1.0e-12,
                    "({x}, {y}): {found} > {nearest}"
                );
                assert!(nearest - found < 1.0e-9, "({x}, {y}): {found} < {nearest}");
            }
        }
    }

    /// An elliptical swing limit reads its gap as an angle to the nearest of
    /// it: equal radii are the cone's gap; near either axis's end a swing
    /// meets that axis's own half-angle, and inside on the long axis short of
    /// its end, the nearer side; and a swing of nearly π past a limit with a
    /// half-angle of π on the other axis reads finite.
    #[test]
    fn the_elliptical_gap_is_the_angle_to_the_limit() {
        let radius = |angle: f64| crcbl_core::trig::sin(0.5 * angle);
        for direction in [DVec3::X, DVec3::new(0.6, -0.8, 0.0), DVec3::Y] {
            for angle in [0.2, 0.5, 0.9] {
                let q = rotation_from_scaled_axis(direction * angle)
                    * rotation_from_scaled_axis(DVec3::Z * 0.4);
                let (gap, axis) = ellipse_gap(radius(0.5), radius(0.5), q);
                assert!((gap - (0.5 - swing_angle(q))).abs() < 1.0e-4, "{gap}");
                // About the swing's own axis, which closes the gap on either
                // side of the limit, as the cone's does.
                assert!((axis - direction).length() < 1.0e-9, "{axis:?}");
            }
        }
        let (x, y) = (0.3, 1.1);
        let gap = |q: DQuat| ellipse_gap(radius(x), radius(y), q).0;
        assert!((gap(rotation_from_scaled_axis(DVec3::X * 0.1)) - (x - 0.1)).abs() < 1.0e-4);
        assert!((gap(rotation_from_scaled_axis(DVec3::Y * 1.05)) - (y - 1.05)).abs() < 1.0e-4);
        assert!((gap(rotation_from_scaled_axis(-DVec3::Y * 1.3)) - (y - 1.3)).abs() < 1.0e-4);
        // Halfway out along the long axis, the short side is nearer than its
        // end.
        let halfway = gap(rotation_from_scaled_axis(DVec3::Y * 0.5));
        assert!(halfway > 0.0 && halfway < y - 0.5, "{halfway}");
        let near_half_turn = rotation_from_scaled_axis(DVec3::new(0.0, 1.0, 0.0) * 3.1);
        let (wide, axis) = ellipse_gap(radius(core::f64::consts::PI), radius(0.4), near_half_turn);
        assert!(
            wide.is_finite() && wide < 0.0 && axis.is_finite(),
            "{wide} {axis:?}"
        );
        let (open, _) = ellipse_gap(
            radius(core::f64::consts::PI),
            radius(0.4),
            rotation_from_scaled_axis(DVec3::X * 3.1),
        );
        assert!(open > 0.0 && open < 0.1, "{open}");
        assert_eq!(
            ellipse_gap(radius(x), radius(y), DQuat::IDENTITY).1,
            DVec3::ZERO
        );
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
