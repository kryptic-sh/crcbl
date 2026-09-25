//! Collider shapes and bounding volumes for physics queries.
//!
//! Every collider shape can produce an axis-aligned bounding box ([`Aabb`]) for
//! broadphase culling. Shapes are parametric (no mesh data) — L0 covers spheres,
//! boxes, and capsules, which together handle ~90% of the engines collision
//! needs (projectiles, characters, vehicles, pickups).
//!
//! All spatial types use `f64` so queries are consistent with the server's
//! deterministic tick loop. Downcasting to `f32` happens only at the render
//! boundary (camera-relative transforms).

use crcbl_core::bounds::{max_lanes_d, min_lanes_d};
use glam::DVec3;

// ---------------------------------------------------------------------------
// Aabb
// ---------------------------------------------------------------------------

/// An axis-aligned bounding box.
///
/// Defined by a minimum and maximum corner. An empty / degenerate box has
/// `min > max` on at least one axis; [`Aabb::EMPTY`] is the canonical empty
/// value and [`Aabb::is_empty`] checks for it.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Aabb {
    /// Minimum corner (inclusive).
    pub min: DVec3,
    /// Maximum corner (inclusive).
    pub max: DVec3,
}

impl Aabb {
    /// The empty AABB: `min = +inf, max = -inf`. Union with any real AABB
    /// produces that real AABB.
    pub const EMPTY: Self = Self {
        min: DVec3::splat(f64::INFINITY),
        max: DVec3::splat(f64::NEG_INFINITY),
    };

    /// Create an AABB from explicit min / max bounds.
    ///
    /// The caller is responsible for ensuring `min ≤ max` per axis; a box that
    /// violates it reads as empty (see [`Aabb::is_empty`]).
    #[inline]
    #[must_use]
    pub fn new(min: DVec3, max: DVec3) -> Self {
        Self { min, max }
    }

    /// Build an AABB from a centre and half-extents.
    #[inline]
    #[must_use]
    pub fn from_centre_half(centre: DVec3, half: DVec3) -> Self {
        Self {
            min: centre - half,
            max: centre + half,
        }
    }

    /// The centre of this AABB.
    #[inline]
    #[must_use]
    pub fn centre(&self) -> DVec3 {
        (self.min + self.max) * 0.5
    }

    /// Extents on each axis (full width / height / depth).
    #[inline]
    #[must_use]
    pub fn extents(&self) -> DVec3 {
        self.max - self.min
    }

    /// Whether this AABB is degenerate: `min` is past `max` on at least one
    /// axis. A zero-extent (flat or point) box is *not* empty — it still
    /// intersects and bounds correctly.
    #[inline]
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.min.x > self.max.x || self.min.y > self.max.y || self.min.z > self.max.z
    }

    /// Half the true surface area of this box — `wh + hd + dw`.
    ///
    /// This is the cost term of the surface area heuristic the dynamic BVH
    /// picks insertion sites with. The factor of two the real surface area
    /// carries is dropped because every comparison the heuristic makes is
    /// between two of these, and a constant factor common to both sides cannot
    /// change which one is smaller.
    ///
    /// An empty box has no surface, so it costs `0` rather than the `NaN` that
    /// `(-inf) - (+inf)` arithmetic on [`Aabb::EMPTY`] would otherwise produce
    /// and propagate into every subsequent comparison.
    #[inline]
    #[must_use]
    pub(crate) fn half_surface_area(&self) -> f64 {
        if self.is_empty() {
            return 0.0;
        }
        let e = self.extents();
        e.x * e.y + e.y * e.z + e.z * e.x
    }

    /// Widen this AABB by a uniform margin on every side.
    #[inline]
    #[must_use]
    pub fn inflated(self, margin: f64) -> Self {
        Self {
            min: self.min - DVec3::splat(margin),
            max: self.max + DVec3::splat(margin),
        }
    }

    // -- set operations -------------------------------------------------------

    /// Union with another AABB, expanding to contain both.
    #[inline]
    #[must_use]
    pub fn union(self, other: Self) -> Self {
        if other.is_empty() {
            return self;
        }
        if self.is_empty() {
            return other;
        }
        // Not `DVec3::min`/`DVec3::max`: glam writes those as a bare `<` per
        // lane, which is false against a `NaN` and so yields the *incoming*
        // box, discarding the accumulator. `Bvh::build_rec` folds this over a
        // range, so one `NaN` body position followed by one finite body left a
        // parent node that came back finite and no longer contained its
        // children — and overlap and ray queries silently miss those pairs.
        // The `is_empty` guards above do not catch it either: `is_empty` is
        // `min.x > max.x`, and `>` is false on a `NaN`, so a poisoned box reads
        // as non-empty. See `crcbl_core::bounds`.
        Self {
            min: min_lanes_d(self.min, other.min),
            max: max_lanes_d(self.max, other.max),
        }
    }

    /// Whether `other` overlaps this AABB (touching counts as overlapping).
    #[inline]
    #[must_use]
    pub fn intersects(&self, other: &Self) -> bool {
        self.min.x <= other.max.x
            && self.max.x >= other.min.x
            && self.min.y <= other.max.y
            && self.max.y >= other.min.y
            && self.min.z <= other.max.z
            && self.max.z >= other.min.z
    }

    /// Whether `other` lies wholly inside this AABB (a shared face counts as
    /// inside).
    ///
    /// The contact broadphase's test for a proxy that has left its fattened
    /// bounds: a `NaN` anywhere in `other` answers `false`, so a poisoned body
    /// is re-inserted rather than kept on bounds that no longer describe it.
    #[inline]
    #[must_use]
    pub fn contains(&self, other: &Self) -> bool {
        self.min.x <= other.min.x
            && self.min.y <= other.min.y
            && self.min.z <= other.min.z
            && self.max.x >= other.max.x
            && self.max.y >= other.max.y
            && self.max.z >= other.max.z
    }

    /// The point of this box (a solid, so its interior counts) nearest to
    /// `point`: `point` itself when it is inside or on the surface, otherwise
    /// the point on the surface it is closest to.
    ///
    /// `None` when there is no answer to give: the box is empty (`min` past
    /// `max` on an axis), or `point` or either corner holds a `NaN`. Clamping
    /// against such a box would either panic in `f64::clamp` or hand back a
    /// `NaN` that reads as a position everywhere downstream.
    #[inline]
    #[must_use]
    pub fn closest_point(&self, point: DVec3) -> Option<DVec3> {
        // `cmple` is false on a `NaN` lane, so one test refuses both an
        // inverted box and a poisoned one — `is_empty` alone passes a `NaN`.
        if !self.min.cmple(self.max).all() || point.is_nan() {
            return None;
        }
        Some(point.clamp(self.min, self.max))
    }

    // -- ray intersection (slab method)---------------------------------------

    /// The parametric interval `(t_near, t_far)` over which a ray is inside
    /// this AABB, or `None` if the ray misses it entirely.
    ///
    /// This is the one slab test in the crate — the boolean overlap check, the
    /// BVH's entry-distance probe and the shape-level box queries all go
    /// through it. `inv_dir` is the component-wise reciprocal of the ray
    /// direction and `dir_is_neg` its per-axis sign; both are precomputed by
    /// the caller because a traversal reuses them across many boxes. Axes with
    /// a zero direction component (infinite reciprocal) are handled without
    /// producing `NaN` from `0.0 * INF`.
    ///
    /// The interval is unclamped: `t_near` is negative when the ray origin is
    /// inside the box. Choosing which end to report is the caller's job — see
    /// `crate::query`'s shared root-selection rule.
    #[must_use]
    pub(crate) fn ray_slab(
        &self,
        origin: DVec3,
        inv_dir: DVec3,
        dir_is_neg: [bool; 3],
    ) -> Option<(f64, f64)> {
        let origin = origin.to_array();
        let inv_dir = inv_dir.to_array();
        let min = self.min.to_array();
        let max = self.max.to_array();

        let mut t_near = f64::NEG_INFINITY;
        let mut t_far = f64::INFINITY;
        for axis in 0..3 {
            let (lo, hi) = if dir_is_neg[axis] {
                (max[axis], min[axis])
            } else {
                (min[axis], max[axis])
            };
            if inv_dir[axis].is_finite() {
                t_near = t_near.max((lo - origin[axis]) * inv_dir[axis]);
                t_far = t_far.min((hi - origin[axis]) * inv_dir[axis]);
            } else if origin[axis] < min[axis] || origin[axis] > max[axis] {
                // Parallel to this slab and outside it: no intersection.
                return None;
            }
        }

        if t_near > t_far {
            return None;
        }
        Some((t_near, t_far))
    }

    /// Test whether a ray intersects this AABB, using the pre-computed
    /// inverse direction and sign flags for efficiency.
    ///
    /// `t_min` and `t_max` bound the ray segment (typically `(0, +inf)` for
    /// unbounded rays, or `(0, length)` for segments).
    #[inline]
    #[must_use]
    pub fn intersect_ray(
        &self,
        origin: DVec3,
        inv_dir: DVec3,
        dir_is_neg: [bool; 3],
        t_min: f64,
        t_max: f64,
    ) -> bool {
        self.ray_slab(origin, inv_dir, dir_is_neg)
            .is_some_and(|(t_near, t_far)| t_near <= t_max && t_far >= t_min)
    }
}

// ---------------------------------------------------------------------------
// Sphere
// ---------------------------------------------------------------------------

/// A sphere collider.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Sphere {
    /// Centre in local simulation space.
    pub centre: DVec3,
    /// Radius, in metres.
    pub radius: f64,
}

impl Sphere {
    /// Create a sphere.
    #[inline]
    #[must_use]
    pub fn new(centre: DVec3, radius: f64) -> Self {
        debug_assert!(radius >= 0.0, "sphere radius must be non-negative");
        Self { centre, radius }
    }

    /// AABB of this sphere.
    #[inline]
    #[must_use]
    pub fn aabb(&self) -> Aabb {
        let r = DVec3::splat(self.radius);
        Aabb {
            min: self.centre - r,
            max: self.centre + r,
        }
    }
}

// ---------------------------------------------------------------------------
// BoxCollider
// ---------------------------------------------------------------------------

/// An axis-aligned box collider (not oriented — orientation is a rotation of
/// the ECS transform system, not of the collider shape itself).
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct BoxCollider {
    /// Centre in local simulation space.
    pub centre: DVec3,
    /// Half-width on each axis, in metres.
    pub half_extents: DVec3,
}

impl BoxCollider {
    /// Create a box collider.
    ///
    /// # Panics
    ///
    /// Panics in debug builds if any half-extent is negative.
    #[inline]
    #[must_use]
    pub fn new(centre: DVec3, half_extents: DVec3) -> Self {
        debug_assert!(
            half_extents.x >= 0.0 && half_extents.y >= 0.0 && half_extents.z >= 0.0,
            "box half-extents must be non-negative"
        );
        Self {
            centre,
            half_extents,
        }
    }

    /// AABB of this box (same as the box itself for an axis-aligned collider).
    #[inline]
    #[must_use]
    pub fn aabb(&self) -> Aabb {
        Aabb {
            min: self.centre - self.half_extents,
            max: self.centre + self.half_extents,
        }
    }
}

// ---------------------------------------------------------------------------
// Capsule
// ---------------------------------------------------------------------------

/// A capsule collider: a cylinder with hemispherical caps, aligned to the Y
/// axis in local space.
///
/// A capsule is defined by a centre point, a radius, and a half-height
/// (half the length of the cylindrical section). The total length is
/// `2 * half_height + 2 * radius`.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Capsule {
    /// Centre of the cylindrical section (midpoint between the two hemisphere
    /// centres).
    pub centre: DVec3,
    /// Radius of the capsule (both the cylinder and the hemispherical caps).
    pub radius: f64,
    /// Half the length of the cylindrical section, not including caps.
    pub half_height: f64,
}

impl Capsule {
    /// Create a capsule.
    ///
    /// # Panics
    ///
    /// Panics in debug builds if radius or half-height is negative.
    #[inline]
    #[must_use]
    pub fn new(centre: DVec3, radius: f64, half_height: f64) -> Self {
        debug_assert!(radius >= 0.0, "capsule radius must be non-negative");
        debug_assert!(
            half_height >= 0.0,
            "capsule half-height must be non-negative"
        );
        Self {
            centre,
            radius,
            half_height,
        }
    }

    /// AABB of this capsule (the capsule is always Y-aligned).
    #[inline]
    #[must_use]
    pub fn aabb(&self) -> Aabb {
        let total_half = DVec3::new(self.radius, self.half_height + self.radius, self.radius);
        Aabb {
            min: self.centre - total_half,
            max: self.centre + total_half,
        }
    }

    /// The top hemisphere centre.
    #[inline]
    #[must_use]
    pub fn top(&self) -> DVec3 {
        DVec3::new(
            self.centre.x,
            self.centre.y + self.half_height,
            self.centre.z,
        )
    }

    /// The bottom hemisphere centre.
    #[inline]
    #[must_use]
    pub fn bottom(&self) -> DVec3 {
        DVec3::new(
            self.centre.x,
            self.centre.y - self.half_height,
            self.centre.z,
        )
    }
}

// ---------------------------------------------------------------------------
// LyingCapsule
// ---------------------------------------------------------------------------

/// A capsule lying on its side: a prone body, its core running horizontally
/// from the head back to the feet.
///
/// A query shape, not a collider — the world's own capsules stand up the Y
/// axis — for asking whether a body lying at a pose would be inside anything:
/// see [`crate::PhysicsWorld::lying_capsule_blocker`].
///
/// # The pose
///
/// - [`head`](Self::head) is the centre of the hemisphere at the head end.
/// - [`yaw`](Self::yaw) is the way the head faces, in radians: a right-handed
///   turn about `+Y`, which is up, from `-Z`. Zero faces `-Z`, a quarter turn
///   faces `-X`, a half turn `+Z` and three quarters `+X` — `-Z` turned by the
///   rotation [`crate::rotation_from_scaled_axis`] builds from `+Y · yaw`, and
///   the `ahead` of `OrbitCamera::walk_direction` in `crcbl-render`.
/// - [`length`](Self::length) is how far behind the head the feet end's
///   hemisphere centre lies, so the body reaches a further
///   [`radius`](Self::radius) past each of the two ends.
///
/// A length of zero is a sphere at `head`, which is what a character
/// controller with a `half_height` of zero already is.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct LyingCapsule {
    /// Centre of the head end's hemisphere.
    pub head: DVec3,
    /// The way the head faces, in radians about `+Y` from `-Z`.
    pub yaw: f64,
    /// Radius of the capsule.
    pub radius: f64,
    /// Distance from the head end's hemisphere centre back to the feet end's,
    /// not including the caps.
    pub length: f64,
}

impl LyingCapsule {
    /// A capsule lying at a pose; see the type for what each part means.
    ///
    /// # Panics
    ///
    /// Panics in debug builds if radius or length is negative.
    #[inline]
    #[must_use]
    pub fn new(head: DVec3, yaw: f64, radius: f64, length: f64) -> Self {
        debug_assert!(radius >= 0.0, "capsule radius must be non-negative");
        debug_assert!(length >= 0.0, "capsule length must be non-negative");
        Self {
            head,
            yaw,
            radius,
            length,
        }
    }

    /// The unit horizontal direction the head faces: from the feet toward the
    /// head.
    #[inline]
    #[must_use]
    pub fn facing(&self) -> DVec3 {
        DVec3::new(
            -crcbl_core::trig::sin(self.yaw),
            0.0,
            -crcbl_core::trig::cos(self.yaw),
        )
    }

    /// Centre of the feet end's hemisphere: [`length`](Self::length) behind
    /// the head.
    #[inline]
    #[must_use]
    pub fn feet(&self) -> DVec3 {
        self.head - self.facing() * self.length
    }

    /// AABB of this capsule.
    #[inline]
    #[must_use]
    pub fn aabb(&self) -> Aabb {
        let feet = self.feet();
        let r = DVec3::splat(self.radius);
        Aabb {
            min: self.head.min(feet) - r,
            max: self.head.max(feet) + r,
        }
    }
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    // -- Aabb ----------------------------------------------------------------

    /// **A `NaN` box does not eat the union it is folded into.**
    ///
    /// `Bvh::build_rec` folds `union` over a range of items, so this is the
    /// shape that matters rather than the pairwise call. glam's `DVec3::min` is
    /// a bare `<` per lane, false against a `NaN`, so it used to yield the
    /// incoming box and throw the accumulator away — and then the *next* finite
    /// box threw the `NaN` away, leaving a node that was finite and no longer
    /// contained its children. A parent that does not contain its children is a
    /// BVH that silently misses overlaps.
    ///
    /// The `is_empty` early-outs do not save it: `is_empty` is `min.x > max.x`,
    /// and every comparison against a `NaN` is false, so a poisoned box reads as
    /// non-empty and goes down the folding path.
    #[test]
    fn a_nan_box_does_not_shrink_the_union_it_is_folded_into() {
        let low = Aabb::from_centre_half(DVec3::splat(-2.0), DVec3::splat(1.0));
        let high = Aabb::from_centre_half(DVec3::splat(2.0), DVec3::splat(1.0));
        let nan = Aabb {
            min: DVec3::new(f64::NAN, 0.0, 0.0),
            max: DVec3::new(f64::NAN, 0.0, 0.0),
        };

        assert!(
            !nan.is_empty(),
            "a NaN box reads as non-empty, which is why the early-outs above do not catch it"
        );

        // The damaging order: a finite box after the NaN restores finiteness
        // while the extent gathered before it would be gone.
        let folded = [low, high, nan, low]
            .into_iter()
            .fold(Aabb::EMPTY, Aabb::union);

        assert!(
            folded.min.x <= low.min.x && folded.max.x >= high.max.x,
            "the union came back as {:?}..{:?}, which does not contain the boxes it was folded \
             from — a BVH node like this misses every pair in the part it dropped",
            folded.min,
            folded.max
        );
    }

    #[test]
    fn empty_aabb_is_empty() {
        assert!(Aabb::EMPTY.is_empty());
    }

    #[test]
    fn point_aabb_is_not_empty() {
        let aabb = Aabb::new(DVec3::splat(1.0), DVec3::splat(1.0));
        assert!(!aabb.is_empty());
        assert_eq!(aabb.min, DVec3::splat(1.0));
        assert_eq!(aabb.max, DVec3::splat(1.0));
    }

    #[test]
    fn union_with_empty_is_identity() {
        let a = Aabb::from_centre_half(DVec3::ZERO, DVec3::splat(1.0));
        assert_eq!(a.union(Aabb::EMPTY), a);
        assert_eq!(Aabb::EMPTY.union(a), a);
    }

    #[test]
    fn union_expands_to_contain_both() {
        let a = Aabb::from_centre_half(DVec3::new(-2.0, 0.0, 0.0), DVec3::splat(1.0));
        let b = Aabb::from_centre_half(DVec3::new(2.0, 0.0, 0.0), DVec3::splat(1.0));
        let u = a.union(b);
        assert_eq!(u.min.x, -3.0);
        assert_eq!(u.max.x, 3.0);
    }

    #[test]
    fn two_overlapping_aabbs_report_that_they_intersect() {
        let a = Aabb::from_centre_half(DVec3::ZERO, DVec3::splat(2.0));
        let b = Aabb::from_centre_half(DVec3::new(1.0, 0.0, 0.0), DVec3::splat(2.0));
        assert!(a.intersects(&b));
    }

    #[test]
    fn aabb_does_not_intersect_separated() {
        let a = Aabb::from_centre_half(DVec3::ZERO, DVec3::splat(1.0));
        let b = Aabb::from_centre_half(DVec3::new(5.0, 0.0, 0.0), DVec3::splat(1.0));
        assert!(!a.intersects(&b));
    }

    #[test]
    fn a_ray_aimed_at_a_box_passes_the_slab_test() {
        let aabb = Aabb::from_centre_half(DVec3::ZERO, DVec3::splat(1.0));
        let origin = DVec3::new(-5.0, 0.0, 0.0);
        let dir = DVec3::new(1.0, 0.0, 0.0);
        let inv_dir = dir.recip();
        let neg = [dir.x < 0.0, dir.y < 0.0, dir.z < 0.0];
        assert!(aabb.intersect_ray(origin, inv_dir, neg, 0.0, f64::INFINITY));
    }

    #[test]
    fn a_ray_passing_above_a_box_fails_the_slab_test() {
        let aabb = Aabb::from_centre_half(DVec3::ZERO, DVec3::splat(1.0));
        let origin = DVec3::new(-5.0, 10.0, 0.0);
        let dir = DVec3::new(1.0, 0.0, 0.0);
        let inv_dir = dir.recip();
        let neg = [dir.x < 0.0, dir.y < 0.0, dir.z < 0.0];
        assert!(!aabb.intersect_ray(origin, inv_dir, neg, 0.0, f64::INFINITY));
    }

    #[test]
    fn ray_hits_from_inside() {
        let aabb = Aabb::from_centre_half(DVec3::ZERO, DVec3::splat(1.0));
        let origin = DVec3::ZERO;
        let dir = DVec3::new(1.0, 0.0, 0.0);
        let inv_dir = dir.recip();
        let neg = [dir.x < 0.0, dir.y < 0.0, dir.z < 0.0];
        assert!(aabb.intersect_ray(origin, inv_dir, neg, 0.0, f64::INFINITY));
    }

    #[test]
    fn ray_on_face_with_zero_dir_component_hits() {
        // Ray starts on +Y face with dir having zero Y component.
        // Old code: (loy - origin.y) * inv_dir.y = (1.0 - 1.0) * INF = 0.0 * INF = NaN.
        let aabb = Aabb::from_centre_half(DVec3::ZERO, DVec3::splat(1.0));
        let origin = DVec3::new(0.0, 1.0, 0.0);
        let dir = DVec3::new(1.0, 0.0, 0.0);
        let inv_dir = dir.recip();
        let neg = [dir.x < 0.0, dir.y < 0.0, dir.z < 0.0];
        assert!(aabb.intersect_ray(origin, inv_dir, neg, 0.0, f64::INFINITY));
    }

    #[test]
    fn ray_on_face_with_zero_dir_component_parallel_miss() {
        // Ray outside AABB on Y, with zero Y direction → parallel miss.
        let aabb = Aabb::from_centre_half(DVec3::ZERO, DVec3::splat(1.0));
        let origin = DVec3::new(0.0, 5.0, 0.0);
        let dir = DVec3::new(1.0, 0.0, 0.0);
        let inv_dir = dir.recip();
        let neg = [dir.x < 0.0, dir.y < 0.0, dir.z < 0.0];
        assert!(!aabb.intersect_ray(origin, inv_dir, neg, 0.0, f64::INFINITY));
    }

    #[test]
    fn ray_slab_reports_entry_and_exit() {
        let aabb = Aabb::from_centre_half(DVec3::ZERO, DVec3::splat(1.0));
        let dir = DVec3::X;
        let neg = [dir.x < 0.0, dir.y < 0.0, dir.z < 0.0];
        // Origin inside: entry is behind the origin, exit ahead of it.
        let (near, far) = aabb.ray_slab(DVec3::ZERO, dir.recip(), neg).unwrap();
        assert!((near + 1.0).abs() < 1e-12);
        assert!((far - 1.0).abs() < 1e-12);
    }

    #[test]
    fn the_closest_point_of_a_box_is_on_its_surface_from_outside() {
        let aabb = Aabb::new(DVec3::ZERO, DVec3::new(1.0, 2.0, 3.0));
        // Past a face: only that axis moves.
        assert_eq!(
            aabb.closest_point(DVec3::new(0.5, 5.0, 1.0)),
            Some(DVec3::new(0.5, 2.0, 1.0))
        );
        // Past a corner: every axis clamps.
        assert_eq!(
            aabb.closest_point(DVec3::new(-4.0, -1.0, 9.0)),
            Some(DVec3::new(0.0, 0.0, 3.0))
        );
    }

    #[test]
    fn the_closest_point_of_a_box_to_a_point_inside_it_is_that_point() {
        let aabb = Aabb::new(DVec3::ZERO, DVec3::new(1.0, 2.0, 3.0));
        let inside = DVec3::new(0.25, 1.5, 2.75);
        assert_eq!(aabb.closest_point(inside), Some(inside));
        // A flat box is a real box: its face is its interior.
        let flat = Aabb::new(DVec3::ZERO, DVec3::new(1.0, 0.0, 1.0));
        assert_eq!(
            flat.closest_point(DVec3::new(0.5, 3.0, 0.5)),
            Some(DVec3::new(0.5, 0.0, 0.5))
        );
    }

    #[test]
    fn an_empty_or_nan_box_or_a_nan_point_has_no_closest_point() {
        let unit = Aabb::new(DVec3::ZERO, DVec3::ONE);
        assert_eq!(Aabb::EMPTY.closest_point(DVec3::ZERO), None);
        assert_eq!(
            Aabb::new(DVec3::ONE, DVec3::ZERO).closest_point(DVec3::ZERO),
            None
        );
        let poisoned = Aabb::new(DVec3::new(f64::NAN, 0.0, 0.0), DVec3::ONE);
        assert!(
            !poisoned.is_empty(),
            "a NaN box reads as non-empty, so is_empty alone would not refuse it"
        );
        assert_eq!(poisoned.closest_point(DVec3::ZERO), None);
        assert_eq!(unit.closest_point(DVec3::new(0.0, f64::NAN, 0.0)), None);
    }

    #[test]
    fn inflated_aabb_is_larger() {
        let aabb = Aabb::from_centre_half(DVec3::ZERO, DVec3::splat(1.0));
        let inflated = aabb.inflated(0.5);
        assert_eq!(inflated.min, DVec3::splat(-1.5));
        assert_eq!(inflated.max, DVec3::splat(1.5));
    }

    #[test]
    fn from_centre_half_roundtrips() {
        let aabb = Aabb::from_centre_half(DVec3::new(1.0, 2.0, 3.0), DVec3::new(0.5, 1.0, 1.5));
        assert_eq!(aabb.centre(), DVec3::new(1.0, 2.0, 3.0));
        assert_eq!(aabb.extents(), DVec3::new(1.0, 2.0, 3.0));
    }

    // -- Sphere --------------------------------------------------------------

    #[test]
    fn a_spheres_bounds_are_its_centre_expanded_by_its_radius() {
        let s = Sphere::new(DVec3::new(1.0, 2.0, 3.0), 0.5);
        let aabb = s.aabb();
        assert_eq!(aabb.min, DVec3::new(0.5, 1.5, 2.5));
        assert_eq!(aabb.max, DVec3::new(1.5, 2.5, 3.5));
    }

    // -- BoxCollider ---------------------------------------------------------

    #[test]
    fn a_box_colliders_bounds_are_its_centre_expanded_by_its_half_extents() {
        let b = BoxCollider::new(DVec3::ZERO, DVec3::splat(2.0));
        let aabb = b.aabb();
        assert_eq!(aabb.min, DVec3::splat(-2.0));
        assert_eq!(aabb.max, DVec3::splat(2.0));
    }

    // -- Capsule -------------------------------------------------------------

    #[test]
    fn a_capsules_bounds_reach_a_radius_past_each_cap_and_a_radius_sideways() {
        let c = Capsule::new(DVec3::ZERO, 0.5, 1.0);
        let aabb = c.aabb();
        assert_eq!(aabb.min, DVec3::new(-0.5, -1.5, -0.5));
        assert_eq!(aabb.max, DVec3::new(0.5, 1.5, 0.5));
    }

    #[test]
    fn a_capsules_end_points_exclude_the_radius_its_caps_add() {
        let c = Capsule::new(DVec3::new(0.0, 5.0, 0.0), 0.5, 2.0);
        assert_eq!(c.top(), DVec3::new(0.0, 7.0, 0.0));
        assert_eq!(c.bottom(), DVec3::new(0.0, 3.0, 0.0));
    }

    // -- LyingCapsule --------------------------------------------------------

    /// **The yaw convention, written out**: zero faces `-Z`, and each quarter
    /// turn carries the facing a right-handed quarter turn about `+Y` — to
    /// `-X`, `+Z`, then `+X` — with the feet the length behind the head.
    #[test]
    fn a_lying_capsules_feet_lie_behind_its_head_at_each_quarter_turn() {
        use std::f64::consts::{FRAC_PI_2, PI};
        let head = DVec3::new(1.0, 2.0, 3.0);
        for (yaw, facing) in [
            (0.0, DVec3::NEG_Z),
            (FRAC_PI_2, DVec3::NEG_X),
            (PI, DVec3::Z),
            (3.0 * FRAC_PI_2, DVec3::X),
        ] {
            let capsule = LyingCapsule::new(head, yaw, 0.3, 1.6);
            assert!(
                (capsule.facing() - facing).length() < 1e-12,
                "yaw {yaw} faces {:?}, not {facing:?}",
                capsule.facing()
            );
            let feet = head - facing * 1.6;
            assert!(
                (capsule.feet() - feet).length() < 1e-12,
                "yaw {yaw} puts the feet at {:?}, not {feet:?}",
                capsule.feet()
            );
        }
    }

    /// **The facing is `-Z` turned by the engine's own rotation about `+Y`**,
    /// at angles that are not quarter turns too — the claim the type's
    /// documentation makes about `rotation_from_scaled_axis`.
    #[test]
    fn a_lying_capsule_faces_where_the_engines_yaw_rotation_turns_minus_z() {
        for yaw in [-2.5, -0.7, 0.3, 1.1, 4.0] {
            let turned = crate::rotation_from_scaled_axis(DVec3::Y * yaw) * DVec3::NEG_Z;
            let facing = LyingCapsule::new(DVec3::ZERO, yaw, 0.3, 1.6).facing();
            assert!(
                (facing - turned).length() < 1e-12,
                "yaw {yaw}: {facing:?} against {turned:?}"
            );
        }
    }

    #[test]
    fn a_lying_capsules_bounds_reach_a_radius_past_its_head_and_feet() {
        let c = LyingCapsule::new(DVec3::new(0.0, 0.5, 0.0), 0.0, 0.5, 1.5);
        let aabb = c.aabb();
        assert!((aabb.min - DVec3::new(-0.5, 0.0, -0.5)).length() < 1e-12);
        assert!((aabb.max - DVec3::new(0.5, 1.0, 2.0)).length() < 1e-12);
    }
}
