//! Lane-wise `min`/`max` that a `NaN` cannot corrupt.
//!
//! # Why this exists rather than `Vec3::min`
//!
//! glam implements `Vec3::min`/`Vec3::max` — and the `DVec3` pair — as a bare
//! comparison per lane:
//!
//! ```text
//! x: if self.x < rhs.x { self.x } else { rhs.x }
//! ```
//!
//! Every comparison against `NaN` is false, so the expression yields **`rhs`**
//! whenever either side is unusable. In an accumulating fold — `acc =
//! acc.min(point)`, which is what a bounding box is — one `NaN` replaces the
//! whole accumulator and the next finite point replaces the `NaN`. The extent
//! gathered before the bad value is silently discarded and the result comes back
//! **finite**, so nothing downstream can tell.
//!
//! `f32::min`/`f32::max` return the other operand instead, which is the
//! behaviour a bounding box wants: an unusable coordinate is skipped and every
//! finite point stays enclosed. A lane with no finite value at all still comes
//! out `NaN`, so wholly degenerate input is still visible as degenerate.
//!
//! # This is one piece of knowledge, and it was wrong in three places
//!
//! It lives here because three crates fold boxes and each had the defect
//! independently: `crcbl_render::cull::Aabb::from_points`, which culled geometry
//! that was on screen; `crcbl_scene::meshlet::cluster_bounds`, which put a
//! cluster's sphere off its own geometry; and `crcbl_phys`'s BVH, whose parent
//! nodes stopped containing their children. Three copies of a rule is three
//! chances to fix two of them.

use glam::{DVec3, Vec3};

/// The lane-wise minimum, skipping a lane where either side is `NaN`.
#[must_use]
pub fn min_lanes(accumulator: Vec3, point: Vec3) -> Vec3 {
    Vec3::new(
        accumulator.x.min(point.x),
        accumulator.y.min(point.y),
        accumulator.z.min(point.z),
    )
}

/// The lane-wise maximum, skipping a lane where either side is `NaN`.
#[must_use]
pub fn max_lanes(accumulator: Vec3, point: Vec3) -> Vec3 {
    Vec3::new(
        accumulator.x.max(point.x),
        accumulator.y.max(point.y),
        accumulator.z.max(point.z),
    )
}

/// [`min_lanes`] at double width.
#[must_use]
pub fn min_lanes_d(accumulator: DVec3, point: DVec3) -> DVec3 {
    DVec3::new(
        accumulator.x.min(point.x),
        accumulator.y.min(point.y),
        accumulator.z.min(point.z),
    )
}

/// [`max_lanes`] at double width.
#[must_use]
pub fn max_lanes_d(accumulator: DVec3, point: DVec3) -> DVec3 {
    DVec3::new(
        accumulator.x.max(point.x),
        accumulator.y.max(point.y),
        accumulator.z.max(point.z),
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The property the module exists for, at every position a `NaN` can take.
    ///
    /// Every position, because the defect it replaces was positional: the bare
    /// comparison yields the incoming point, so a `NaN` anywhere but last threw
    /// away the extent gathered before it and still came back finite. Folding
    /// with glam's operators instead fails the `middle` case.
    #[test]
    fn a_nan_is_skipped_from_any_position_in_a_fold() {
        let nan = Vec3::new(f32::NAN, 0.0, 0.0);
        let low = Vec3::splat(-1.0);
        let high = Vec3::splat(1.0);

        for (label, points) in [
            ("leading", vec![nan, low, high]),
            ("middle", vec![low, nan, high]),
            ("trailing", vec![low, high, nan]),
        ] {
            let mut min = Vec3::splat(f32::INFINITY);
            let mut max = Vec3::splat(f32::NEG_INFINITY);
            for point in points {
                min = min_lanes(min, point);
                max = max_lanes(max, point);
            }
            assert_eq!(min, low, "a NaN {label} moved the minimum");
            assert_eq!(max, high, "a NaN {label} moved the maximum");
        }
    }

    /// An infinity orders normally, so it is data and is kept — the distinction
    /// that leaves an infinite corner reportable while a `NaN` is skipped.
    #[test]
    fn an_infinity_is_kept_where_a_nan_is_skipped() {
        let infinite = Vec3::new(f32::INFINITY, 0.0, 0.0);
        assert_eq!(max_lanes(Vec3::ZERO, infinite).x, f32::INFINITY);
        assert_eq!(
            min_lanes(Vec3::splat(f32::INFINITY), Vec3::new(f32::NAN, 2.0, 2.0)),
            Vec3::new(f32::INFINITY, 2.0, 2.0),
            "the NaN lane keeps the seed, which is what leaves a wholly NaN lane detectable"
        );
    }

    /// The double-width pair is the same rule, and is what `crcbl-phys` folds.
    #[test]
    fn the_double_width_pair_skips_a_nan_the_same_way() {
        let nan = DVec3::new(f64::NAN, 0.0, 0.0);
        let low = DVec3::splat(-1.0);
        let high = DVec3::splat(1.0);

        let mut min = DVec3::splat(f64::INFINITY);
        let mut max = DVec3::splat(f64::NEG_INFINITY);
        for point in [low, nan, high] {
            min = min_lanes_d(min, point);
            max = max_lanes_d(max, point);
        }
        assert_eq!(min, low);
        assert_eq!(max, high);
    }
}
