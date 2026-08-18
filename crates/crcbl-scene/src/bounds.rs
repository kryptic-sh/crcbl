//! Lane-wise `min`/`max` that a `NaN` cannot corrupt.
//!
//! # Why this exists rather than `Vec3::min`
//!
//! glam implements `Vec3::min`/`Vec3::max` (and the `DVec3` pair) as a bare
//! comparison per lane:
//!
//! ```text
//! x: if self.x < rhs.x { self.x } else { rhs.x }
//! ```
//!
//! Every comparison against `NaN` is false, so the expression yields **`rhs`**
//! whenever either side is unusable. In an accumulating fold — `acc =
//! acc.min(point)`, which is what a bounding box is — one `NaN` replaces the
//! whole accumulator, and the next finite point replaces the `NaN`. The extent
//! gathered before the bad value is silently discarded and the result comes back
//! **finite**, so nothing downstream can tell.
//!
//! `f32::min`/`f32::max` return the other operand instead, which is the
//! behaviour a bounding box wants: an unusable coordinate is skipped and every
//! finite point stays enclosed. A lane with no finite value at all still comes
//! out `NaN`, so wholly degenerate geometry is still visible as degenerate.
//!
//! # Why it matters here specifically
//!
//! Nothing validates a `POSITION` accessor for finiteness. `gltf_check` never
//! looks at a float's value, and [`crate::meshlet::build_meshlets`] lists a
//! partial triangle and an out-of-range index as its preconditions. A downloaded
//! document with one `NaN` vertex reaches the cluster builder directly, and a
//! cluster sphere that does not contain its cluster culls geometry that is on
//! screen — the direction `crcbl_render::cull` documents a cull must never err
//! in.
//!
//! `crcbl_render::cull::Aabb::from_points` had the same defect and folds the
//! same way now; it is a separate crate rather than a shared call because the
//! two sit on opposite sides of the host/render split this workspace keeps.

use glam::{DVec3, Vec3};

/// The lane-wise minimum, skipping a lane where either side is `NaN`.
pub(crate) fn min_lanes(accumulator: Vec3, point: Vec3) -> Vec3 {
    Vec3::new(
        accumulator.x.min(point.x),
        accumulator.y.min(point.y),
        accumulator.z.min(point.z),
    )
}

/// The lane-wise maximum, skipping a lane where either side is `NaN`.
pub(crate) fn max_lanes(accumulator: Vec3, point: Vec3) -> Vec3 {
    Vec3::new(
        accumulator.x.max(point.x),
        accumulator.y.max(point.y),
        accumulator.z.max(point.z),
    )
}

/// [`min_lanes`] at double width.
pub(crate) fn min_lanes_d(accumulator: DVec3, point: DVec3) -> DVec3 {
    DVec3::new(
        accumulator.x.min(point.x),
        accumulator.y.min(point.y),
        accumulator.z.min(point.z),
    )
}

/// [`max_lanes`] at double width.
pub(crate) fn max_lanes_d(accumulator: DVec3, point: DVec3) -> DVec3 {
    DVec3::new(
        accumulator.x.max(point.x),
        accumulator.y.max(point.y),
        accumulator.z.max(point.z),
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The property the whole module exists for, at every position a `NaN` can
    /// take in a fold — because the defect it replaces was positional.
    ///
    /// Folding with glam's operators instead fails this: the `middle` case
    /// returns a box that no longer contains `high`.
    #[test]
    fn a_nan_is_skipped_from_any_position_in_a_fold() {
        let nan = Vec3::new(f32::NAN, 0.0, 0.0);
        let low = Vec3::new(-1.0, -1.0, -1.0);
        let high = Vec3::new(1.0, 1.0, 1.0);

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

    /// An infinity has a defined ordering, so it is data rather than a hole and
    /// is kept — the distinction that makes an infinite corner still reportable.
    #[test]
    fn an_infinity_is_kept_where_a_nan_is_skipped() {
        let seed = Vec3::splat(f32::INFINITY);
        let infinite = Vec3::new(f32::INFINITY, 0.0, 0.0);
        assert_eq!(max_lanes(Vec3::ZERO, infinite).x, f32::INFINITY);
        assert_eq!(
            min_lanes(seed, Vec3::new(f32::NAN, 2.0, 2.0)),
            Vec3::new(f32::INFINITY, 2.0, 2.0),
            "the NaN lane keeps the seed, which is what leaves a wholly NaN lane detectable"
        );
    }

    /// The double-width pair is the same rule, and is folded by
    /// [`crate::cluster_dag`] over group spheres.
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
