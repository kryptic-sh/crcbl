//! Spread: a direction scattered within a cone about an aim, from two
//! uniform random numbers the caller draws — a shot group, a shotgun's
//! pellets, a spray of sparks.
//!
//! The randoms come in as numbers rather than through a generator, so a
//! caller keeps its own stream (a seeded `crcbl_rand::Rng`, a replay's
//! recorded draws) and this stays a pure function.

use core::f64::consts::{FRAC_PI_2, TAU};

use crcbl_core::trig::{cos, sin};
use glam::DVec3;

use crate::contact::manifold::orthonormal_basis;

/// A direction within `half_angle` radians of `aim`, uniform over the disc
/// the cone cuts on any plane square to `aim` — so hits on a target face are
/// spread evenly, as a shot group is, rather than evenly over the sphere.
///
/// `radial` and `azimuth` are uniform draws in `[0, 1)`: `radial` picks the
/// distance from the centre (its square root, for an even disc) and
/// `azimuth` the angle around it. A `half_angle` of zero is `aim` itself.
///
/// `None` for an `aim` with no direction, a draw outside `[0, 1)`, or a
/// half-angle that is not finite in `[0, π/2)`.
#[must_use]
pub fn direction_in_cone(aim: DVec3, half_angle: f64, radial: f64, azimuth: f64) -> Option<DVec3> {
    let unit = |value: f64| (0.0..1.0).contains(&value);
    if !unit(radial) || !unit(azimuth) || !(0.0..FRAC_PI_2).contains(&half_angle) {
        return None;
    }
    let aim = aim.try_normalize()?;
    if half_angle == 0.0 {
        return Some(aim);
    }
    let (right, up) = orthonormal_basis(aim);
    // The engine's own sine and cosine, as the simulation's determinism rule
    // asks (`crates/crcbl-phys/clippy.toml`): a spread drawn on two targets
    // from the same numbers lands in the same place on both.
    let radius = radial.sqrt() * sin(half_angle) / cos(half_angle);
    let turn = TAU * azimuth;
    (aim + right * (radius * cos(turn)) + up * (radius * sin(turn))).try_normalize()
}

#[cfg(test)]
mod tests {
    use super::*;

    const HALF_ANGLE: f64 = 0.01;

    /// The platform's tangent: a reference the engine's own trig is checked
    /// against, never a value the simulation uses.
    #[expect(
        clippy::disallowed_methods,
        reason = "an independent reference for the engine's sine over cosine"
    )]
    fn platform_tan(angle: f64) -> f64 {
        angle.tan()
    }

    /// Where `direction` meets the plane one metre along `aim`, from its
    /// centre.
    fn on_target(direction: DVec3, aim: DVec3) -> DVec3 {
        let aim = aim.normalize();
        direction / direction.dot(aim) - aim
    }

    /// **The edge of the draw is the edge of the cone**, at every azimuth and
    /// for an aim along any axis, straight up included.
    #[test]
    fn the_largest_draw_reaches_the_cone_and_no_further() {
        let edge = platform_tan(HALF_ANGLE);
        for aim in [DVec3::Z, DVec3::NEG_Y, DVec3::new(0.3, 0.9, -0.2), DVec3::Y] {
            for step in 0..16 {
                let azimuth = f64::from(step) / 16.0;
                let largest = 1.0 - f64::EPSILON;
                let direction = direction_in_cone(aim, HALF_ANGLE, largest, azimuth).unwrap();
                assert!(direction.is_normalized());
                let off = on_target(direction, aim).length();
                assert!(
                    (off - edge).abs() < 1e-9,
                    "{aim} at {azimuth}: {off} vs {edge}"
                );
                let centre = direction_in_cone(aim, HALF_ANGLE, 0.0, azimuth).unwrap();
                assert!(centre.abs_diff_eq(aim.normalize(), 1e-15));
            }
        }
    }

    /// **The disc is even**: the share of draws inside half the radius is a
    /// quarter, as area says, and the azimuth turns the point all the way
    /// round.
    #[test]
    fn the_disc_is_covered_evenly() {
        let aim = DVec3::X;
        let edge = platform_tan(HALF_ANGLE);
        let n = 64_u32;
        let mut inner = 0_u32;
        for i in 0..n {
            for j in 0..n {
                let radial = (f64::from(i) + 0.5) / f64::from(n);
                let azimuth = (f64::from(j) + 0.5) / f64::from(n);
                let off = on_target(
                    direction_in_cone(aim, HALF_ANGLE, radial, azimuth).unwrap(),
                    aim,
                );
                if off.length() < 0.5 * edge {
                    inner += 1;
                }
            }
        }
        let share = f64::from(inner) / f64::from(n * n);
        assert!((share - 0.25).abs() < 0.01, "{share}");
        let a = on_target(direction_in_cone(aim, HALF_ANGLE, 0.5, 0.0).unwrap(), aim);
        let b = on_target(direction_in_cone(aim, HALF_ANGLE, 0.5, 0.5).unwrap(), aim);
        assert!((a + b).length() < 1e-12, "half a turn is the opposite side");
    }

    /// Zero spread is the aim; bad inputs are refused.
    #[test]
    fn zero_spread_and_bad_inputs() {
        let aim = DVec3::new(0.0, 0.0, 2.0);
        assert_eq!(direction_in_cone(aim, 0.0, 0.7, 0.2), Some(DVec3::Z));
        assert_eq!(direction_in_cone(DVec3::ZERO, HALF_ANGLE, 0.5, 0.5), None);
        assert_eq!(direction_in_cone(aim, HALF_ANGLE, 1.0, 0.5), None);
        assert_eq!(direction_in_cone(aim, HALF_ANGLE, 0.5, -0.1), None);
        assert_eq!(direction_in_cone(aim, FRAC_PI_2, 0.5, 0.5), None);
        assert_eq!(direction_in_cone(aim, f64::NAN, 0.5, 0.5), None);
    }
}
