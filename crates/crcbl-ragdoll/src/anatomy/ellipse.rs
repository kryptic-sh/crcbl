//! A ball's swing ellipse fitted through its four anatomical stops, in the
//! coordinates `crcbl-phys` bounds a swing in.
//!
//! The physics joint tips its frame by the neutral swing and bounds the
//! swing part of the child's turn from there: the quaternion's `x` and `y`,
//! inside the ellipse of semi-axes `sin(a/2)` and `sin(b/2)` (Jolt's
//! `SwingTwistConstraint` form). Centring that ellipse on the middle of each
//! range meets the range only along its own axes, because turns do not
//! add: a shoulder centred between 50° of extension and 180° of flexion and
//! tipped sideways by its abduction's middle reaches neither stop. So the
//! centre and both half-angles are solved together, so that the swing from
//! the centre to each stop — pure flexion, extension, abduction and
//! adduction from the anatomical zero — lies on the ellipse.
//!
//! Given a centre, the stops' swings from it make two of the four
//! conditions linear in `1/sin²(a/2)` and `1/sin²(b/2)`; the other two are
//! the residuals Newton's method drives to zero over the centre, its
//! Jacobian by forward differences. Every step is IEEE-754 arithmetic or
//! `crcbl_core::trig`, so the fit is the same bits on every target.

use crcbl_core::trig::atan2;
use crcbl_phys::rotation_from_scaled_axis;
use glam::{DQuat, DVec3};

/// The most Newton steps a fit takes; the profile's balls converge in five.
const ITERATIONS: usize = 32;

/// The residual, in the ellipse's own units, a fit accepts.
const TOLERANCE: f64 = 1e-12;

/// The forward-difference step over the centre, in radians.
const STEP: f64 = 1e-7;

/// A swing ellipse about a centre, in the joint frame's x–y plane.
#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) struct Ellipse {
    /// The swing from the anatomical zero to the centre, as a rotation
    /// vector's x and y.
    pub(crate) centre: [f64; 2],
    /// The half-angles about the centred frame's x and y, in radians.
    pub(crate) half: [f64; 2],
    /// Their half-angle sines, the semi-axes the physics bounds.
    sines: [f64; 2],
}

/// The turn by the rotation vector `(x, y, 0)`.
fn turn([x, y]: [f64; 2]) -> DQuat {
    rotation_from_scaled_axis(DVec3::new(x, y, 0.0))
}

/// The swing part of `q` about z — `q = swing twist` — as its x and y:
/// `crcbl-phys`'s split, its sign taken so the real part is not negative.
fn swing_part(q: DQuat) -> [f64; 2] {
    let q = if q.w < 0.0 { -q } else { q };
    let s = (q.z * q.z + q.w * q.w).sqrt();
    if s > 0.0 {
        [(q.w * q.x - q.z * q.y) / s, (q.w * q.y + q.z * q.x) / s]
    } else {
        [q.x, q.y]
    }
}

/// `2·asin(s)`, from the deterministic arc tangent.
fn twice_asin(s: f64) -> f64 {
    2.0 * atan2(s, (1.0 - s * s).sqrt())
}

impl Ellipse {
    /// The ellipse through `stops` — the rotation vectors from the
    /// anatomical zero of the two stops on x, then the two on y — or `None`
    /// where there is none: two stops at one swing (a shoulder's flexion
    /// and abduction both overhead), a half-angle past a half turn, or a
    /// fit that does not converge.
    pub(crate) fn through(stops: [[f64; 2]; 4]) -> Option<Self> {
        // Each range's middle on its own axis: exact for a range on one axis
        // alone, and where Newton starts.
        let mut centre = [
            0.5 * (stops[0][0] + stops[1][0]),
            0.5 * (stops[2][1] + stops[3][1]),
        ];
        let range = [[stops[0][0], stops[1][0]], [stops[2][1], stops[3][1]]]
            .map(|[a, b]| [a.min(b), a.max(b)]);
        let stops = stops.map(turn);
        // Given a centre: the semi-axes' inverse squares from the first stop
        // on each axis, and how far the other two miss.
        let solve = |centre: [f64; 2]| -> Option<([f64; 2], [f64; 2])> {
            let from = turn(centre).inverse();
            let [p, q, r, s] = stops.map(|stop| swing_part(from * stop));
            let det = p[0] * p[0] * r[1] * r[1] - p[1] * p[1] * r[0] * r[0];
            // Success checks, so a NaN fails them.
            (det.abs() >= TOLERANCE).then_some(())?;
            let u = (r[1] * r[1] - p[1] * p[1]) / det;
            let v = (p[0] * p[0] - r[0] * r[0]) / det;
            let miss = |point: [f64; 2]| point[0] * point[0] * u + point[1] * point[1] * v - 1.0;
            Some(([u, v], [miss(q), miss(s)]))
        };
        for _ in 0..ITERATIONS {
            let (_, miss) = solve(centre)?;
            if miss[0].abs() + miss[1].abs() < TOLERANCE {
                break;
            }
            let (_, dx) = solve([centre[0] + STEP, centre[1]])?;
            let (_, dy) = solve([centre[0], centre[1] + STEP])?;
            let j = [
                [(dx[0] - miss[0]) / STEP, (dy[0] - miss[0]) / STEP],
                [(dx[1] - miss[1]) / STEP, (dy[1] - miss[1]) / STEP],
            ];
            let det = j[0][0] * j[1][1] - j[0][1] * j[1][0];
            (det.abs() >= TOLERANCE).then_some(())?;
            centre = [
                centre[0] - (j[1][1] * miss[0] - j[0][1] * miss[1]) / det,
                centre[1] - (j[0][0] * miss[1] - j[1][0] * miss[0]) / det,
            ];
        }
        let ([u, v], miss) = solve(centre)?;
        if !(miss[0].abs() + miss[1].abs() < TOLERANCE && u > 1.0 && v > 1.0) {
            return None;
        }
        let sines = [1.0 / u.sqrt(), 1.0 / v.sqrt()];
        Self {
            centre,
            half: sines.map(twice_asin),
            sines,
        }
        .if_plausible(range)
    }

    /// This ellipse, if it is the one the stops ask for: finite, its centre
    /// between each axis's stops (`range`, least then most, x then y), and
    /// the anatomical zero inside it. Newton's method has other roots — an
    /// ellipse through the four stops centred outside them, turned through
    /// the long way round — that are no joint's range.
    fn if_plausible(self, range: [[f64; 2]; 2]) -> Option<Self> {
        let finite = self
            .centre
            .iter()
            .chain(&self.half)
            .chain(&self.sines)
            .all(|v| v.is_finite());
        let between = |value: f64, [least, most]: [f64; 2]| least <= value && value <= most;
        let plausible = finite
            && between(self.centre[0], range[0])
            && between(self.centre[1], range[1])
            && self.radius([0.0, 0.0]) < 1.0;
        plausible.then_some(self)
    }

    /// How far out `point`, a rotation vector from the anatomical zero,
    /// lies: 1 on the ellipse, 0 at its centre.
    pub(crate) fn radius(&self, point: [f64; 2]) -> f64 {
        let [x, y] = swing_part(turn(self.centre).inverse() * turn(point));
        let (u, v) = (x / self.sines[0], y / self.sines[1]);
        (u * u + v * v).sqrt()
    }

    /// The half-angles of this ellipse scaled by `ratio` in its semi-axes:
    /// a soft ellipse about the same centre whose radius, as
    /// [`radius`](Self::radius) measures it, is `ratio`.
    pub(crate) fn scaled_half(&self, ratio: f64) -> [f64; 2] {
        self.sines.map(|sine| twice_asin(ratio * sine))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const DEGREE: f64 = core::f64::consts::PI / 180.0;

    /// **The fitted ellipse passes through all four stops**, where a
    /// symmetric one centred on each range's middle does not; a range
    /// symmetric on both axes is that symmetric ellipse.
    #[test]
    fn the_ellipse_passes_through_every_stop() {
        let hip = [[125.0, 0.0], [-30.0, 0.0], [0.0, 45.0], [0.0, -20.0]]
            .map(|[x, y]| [x * DEGREE, y * DEGREE]);
        let fit = Ellipse::through(hip).expect("a fit");
        for stop in hip {
            assert!(
                (fit.radius(stop) - 1.0).abs() < 1e-9,
                "{stop:?}: {}",
                fit.radius(stop)
            );
        }
        let neck = [[22.5, 0.0], [-22.5, 0.0], [0.0, 22.5], [0.0, -22.5]]
            .map(|[x, y]| [x * DEGREE, y * DEGREE]);
        let fit = Ellipse::through(neck).expect("a fit");
        assert!(fit.centre[0].abs() < 1e-12 && fit.centre[1].abs() < 1e-12);
        assert!((fit.half[0] - 22.5 * DEGREE).abs() < 1e-9);
    }

    /// **Stops with no sensible ellipse get none**: a NaN anywhere, and
    /// stops at the zero, whose Jacobian is singular.
    #[test]
    fn a_nan_or_a_singular_fit_is_refused() {
        let nan = [[f64::NAN, 0.0], [-0.3, 0.0], [0.0, 0.4], [0.0, -0.2]];
        assert_eq!(Ellipse::through(nan), None);
        let nan_y = [[1.0, 0.0], [-0.3, 0.0], [0.0, f64::NAN], [0.0, -0.2]];
        assert_eq!(Ellipse::through(nan_y), None);
        assert_eq!(Ellipse::through([[0.0, 0.0]; 4]), None);
    }

    /// **A root of the fit that is no range is refused**: an ellipse
    /// centred outside the stops, or one the anatomical zero lies outside.
    #[test]
    fn an_implausible_root_is_refused() {
        let hip = [[125.0, 0.0], [-30.0, 0.0], [0.0, 45.0], [0.0, -20.0]]
            .map(|[x, y]| [x * DEGREE, y * DEGREE]);
        let fit = Ellipse::through(hip).expect("a fit");
        let range = [
            [-30.0 * DEGREE, 125.0 * DEGREE],
            [-20.0 * DEGREE, 45.0 * DEGREE],
        ];
        assert_eq!(fit.if_plausible(range), Some(fit));
        let outside = Ellipse {
            centre: [130.0 * DEGREE, 0.0],
            ..fit
        };
        assert_eq!(outside.if_plausible(range), None);
        // Centred between the stops but too small to hold the zero.
        let small = Ellipse {
            sines: [0.1, 0.1],
            half: [0.1, 0.1].map(twice_asin),
            ..fit
        };
        assert_eq!(small.if_plausible(range), None);
        let infinite = Ellipse {
            half: [f64::INFINITY, 0.1],
            ..fit
        };
        assert_eq!(infinite.if_plausible(range), None);
    }

    /// Stops that meet — a shoulder's flexion and abduction both straight
    /// overhead — have no ellipse through them.
    #[test]
    fn coincident_stops_have_no_ellipse() {
        let shoulder = [[180.0, 0.0], [-50.0, 0.0], [0.0, 180.0], [0.0, -30.0]]
            .map(|[x, y]| [x * DEGREE, y * DEGREE]);
        assert_eq!(Ellipse::through(shoulder), None);
    }
}
