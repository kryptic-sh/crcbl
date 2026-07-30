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
    /// The caller is responsible for ensuring `min ≤ max` per axis; use
    /// [`Aabb::from_points`] when the relationship is unknown.
    #[inline]
    #[must_use]
    pub fn new(min: DVec3, max: DVec3) -> Self {
        Self { min, max }
    }

    /// Build an AABB that contains every point in the iterator.
    #[inline]
    #[must_use]
    pub fn from_points(points: impl IntoIterator<Item = DVec3>) -> Self {
        let mut aabb = Self::EMPTY;
        for p in points {
            aabb = aabb.union_point(p);
        }
        aabb
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

    /// Half-extents on each axis.
    #[inline]
    #[must_use]
    pub fn half_extents(&self) -> DVec3 {
        (self.max - self.min) * 0.5
    }

    /// Extents on each axis (full width / height / depth).
    #[inline]
    #[must_use]
    pub fn extents(&self) -> DVec3 {
        self.max - self.min
    }

    /// Surface area. Returns `0.0` for an empty AABB.
    #[inline]
    #[must_use]
    pub fn surface_area(&self) -> f64 {
        let d = self.extents();
        if d.x <= 0.0 || d.y <= 0.0 || d.z <= 0.0 {
            return 0.0;
        }
        2.0 * (d.x * d.y + d.x * d.z + d.y * d.z)
    }

    /// Whether this AABB contains no volume (any axis has zero or negative
    /// extent).
    #[inline]
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.min.x > self.max.x || self.min.y > self.max.y || self.min.z > self.max.z
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
        Self {
            min: self.min.min(other.min),
            max: self.max.max(other.max),
        }
    }

    /// Expand to contain a single point.
    #[inline]
    #[must_use]
    pub fn union_point(self, point: DVec3) -> Self {
        Self {
            min: self.min.min(point),
            max: self.max.max(point),
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

    /// Whether `point` is inside (or on the boundary of) this AABB.
    #[inline]
    #[must_use]
    pub fn contains(&self, point: DVec3) -> bool {
        point.x >= self.min.x
            && point.x <= self.max.x
            && point.y >= self.min.y
            && point.y <= self.max.y
            && point.z >= self.min.z
            && point.z <= self.max.z
    }

    // -- ray intersection (slab method) ---------------------------------------

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
        let (lox, hix) = if dir_is_neg[0] {
            (self.max.x, self.min.x)
        } else {
            (self.min.x, self.max.x)
        };
        let (loy, hiy) = if dir_is_neg[1] {
            (self.max.y, self.min.y)
        } else {
            (self.min.y, self.max.y)
        };
        let (loz, hiz) = if dir_is_neg[2] {
            (self.max.z, self.min.z)
        } else {
            (self.min.z, self.max.z)
        };

        let tmin = (lox - origin.x) * inv_dir.x;
        let tmax = (hix - origin.x) * inv_dir.x;
        let tymin = (loy - origin.y) * inv_dir.y;
        let tymax = (hiy - origin.y) * inv_dir.y;

        if tmin > tymax || tymin > tmax {
            return false;
        }
        let tmin = tmin.max(tymin);
        let tmax = tmax.min(tymax);

        let tzmin = (loz - origin.z) * inv_dir.z;
        let tzmax = (hiz - origin.z) * inv_dir.z;

        if tmin > tzmax || tzmin > tmax {
            return false;
        }
        let tmin = tmin.max(tzmin);
        let tmax = tmax.min(tzmax);

        tmin <= t_max && tmax >= t_min
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
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    // -- Aabb ----------------------------------------------------------------

    #[test]
    fn empty_aabb_is_empty() {
        assert!(Aabb::EMPTY.is_empty());
    }

    #[test]
    fn point_aabb_is_not_empty() {
        let aabb = Aabb::from_points([DVec3::splat(1.0)]);
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
    fn aabb_intersects_overlapping() {
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
    fn aabb_contains_point() {
        let aabb = Aabb::from_centre_half(DVec3::ZERO, DVec3::splat(2.0));
        assert!(aabb.contains(DVec3::new(1.0, 1.0, 1.0)));
        assert!(aabb.contains(DVec3::ZERO));
        assert!(aabb.contains(DVec3::new(2.0, 2.0, 2.0))); // boundary
        assert!(!aabb.contains(DVec3::new(2.1, 0.0, 0.0)));
    }

    #[test]
    fn ray_intersects_aabb() {
        let aabb = Aabb::from_centre_half(DVec3::ZERO, DVec3::splat(1.0));
        let origin = DVec3::new(-5.0, 0.0, 0.0);
        let dir = DVec3::new(1.0, 0.0, 0.0);
        let inv_dir = dir.recip();
        let neg = [dir.x < 0.0, dir.y < 0.0, dir.z < 0.0];
        assert!(aabb.intersect_ray(origin, inv_dir, neg, 0.0, f64::INFINITY));
    }

    #[test]
    fn ray_misses_aabb() {
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
    fn surface_area_of_unit_box() {
        let aabb = Aabb::from_centre_half(DVec3::ZERO, DVec3::splat(0.5));
        assert!((aabb.surface_area() - 6.0).abs() < 1e-12);
    }

    #[test]
    fn surface_area_of_empty_is_zero() {
        assert_eq!(Aabb::EMPTY.surface_area(), 0.0);
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
        assert_eq!(aabb.half_extents(), DVec3::new(0.5, 1.0, 1.5));
    }

    // -- Sphere --------------------------------------------------------------

    #[test]
    fn sphere_aabb() {
        let s = Sphere::new(DVec3::new(1.0, 2.0, 3.0), 0.5);
        let aabb = s.aabb();
        assert_eq!(aabb.min, DVec3::new(0.5, 1.5, 2.5));
        assert_eq!(aabb.max, DVec3::new(1.5, 2.5, 3.5));
    }

    // -- BoxCollider ---------------------------------------------------------

    #[test]
    fn box_aabb() {
        let b = BoxCollider::new(DVec3::ZERO, DVec3::splat(2.0));
        let aabb = b.aabb();
        assert_eq!(aabb.min, DVec3::splat(-2.0));
        assert_eq!(aabb.max, DVec3::splat(2.0));
    }

    // -- Capsule -------------------------------------------------------------

    #[test]
    fn capsule_aabb() {
        let c = Capsule::new(DVec3::ZERO, 0.5, 1.0);
        let aabb = c.aabb();
        assert_eq!(aabb.min, DVec3::new(-0.5, -1.5, -0.5));
        assert_eq!(aabb.max, DVec3::new(0.5, 1.5, 0.5));
    }

    #[test]
    fn capsule_top_bottom() {
        let c = Capsule::new(DVec3::new(0.0, 5.0, 0.0), 0.5, 2.0);
        assert_eq!(c.top(), DVec3::new(0.0, 7.0, 0.0));
        assert_eq!(c.bottom(), DVec3::new(0.0, 3.0, 0.0));
    }
}
