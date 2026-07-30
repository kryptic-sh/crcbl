//! Shape-level intersection queries: ray-vs-shape and swept-sphere TOI.
//!
//! These operate on the collider shapes directly (sphere, box, capsule) rather
//! than on the BVH. They compute exact hit points, normals, and times of impact.

use crate::broadphase::Ray;
use crate::collider::{Aabb, Capsule, Sphere};
use glam::DVec3;

// ---------------------------------------------------------------------------
// Overlap queries
// ---------------------------------------------------------------------------

/// Test whether two spheres overlap (touching counts as overlapping).
#[inline]
#[must_use]
pub fn sphere_overlaps_sphere(a: &Sphere, b: &Sphere) -> bool {
    let combined = a.radius + b.radius;
    (a.centre - b.centre).length_squared() <= combined * combined
}

/// Test whether a sphere overlaps an AABB.
#[inline]
#[must_use]
pub fn sphere_overlaps_aabb(sphere: &Sphere, aabb: &Aabb) -> bool {
    let closest = DVec3::new(
        sphere.centre.x.clamp(aabb.min.x, aabb.max.x),
        sphere.centre.y.clamp(aabb.min.y, aabb.max.y),
        sphere.centre.z.clamp(aabb.min.z, aabb.max.z),
    );
    (sphere.centre - closest).length_squared() <= sphere.radius * sphere.radius
}

/// Test whether a sphere overlaps a Y-aligned capsule.
#[inline]
#[must_use]
pub fn sphere_overlaps_capsule(sphere: &Sphere, capsule: &Capsule) -> bool {
    // Clamp the sphere centre to the capsule segment.
    let bot = capsule.bottom();
    let top = capsule.top();
    let seg_dir = top - bot;
    let seg_len_sq = seg_dir.length_squared();

    let t = if seg_len_sq < f64::EPSILON {
        0.5
    } else {
        ((sphere.centre - bot).dot(seg_dir) / seg_len_sq).clamp(0.0, 1.0)
    };
    let closest = bot + seg_dir * t;
    let combined = sphere.radius + capsule.radius;
    (sphere.centre - closest).length_squared() <= combined * combined
}

// ---------------------------------------------------------------------------
// Hit result
// ---------------------------------------------------------------------------

/// The result of a ray or sweep query against a single shape.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct ShapeHit {
    /// Parametric distance along the ray at the hit point.
    pub t: f64,
    /// World-space position of the hit.
    pub point: DVec3,
    /// Unit normal at the hit point, pointing **away** from the surface the ray
    /// struck.  For swept-sphere queries this is the contact normal.
    pub normal: DVec3,
    /// Whether the ray origin started inside the shape.
    pub started_inside: bool,
}

// ---------------------------------------------------------------------------
// Ray vs sphere
// ---------------------------------------------------------------------------

/// Intersect a ray with a sphere. Returns the closest hit, or `None` if the
/// ray misses entirely (including if the sphere is behind the ray origin).
#[must_use]
pub fn ray_vs_sphere(ray: &Ray, sphere: &Sphere) -> Option<ShapeHit> {
    let oc = ray.origin - sphere.centre;
    let a = ray.dir.dot(ray.dir);
    if a <= f64::EPSILON {
        return None;
    }
    let b = 2.0 * oc.dot(ray.dir);
    let c = oc.dot(oc) - sphere.radius * sphere.radius;
    let disc = b * b - 4.0 * a * c;
    if disc < 0.0 {
        return None;
    }
    let sqrt_disc = disc.sqrt();
    let t1 = (-b - sqrt_disc) / (2.0 * a);
    let t2 = (-b + sqrt_disc) / (2.0 * a);
    let (t, started_inside) = if t1 >= 0.0 {
        (t1, false)
    } else if t2 >= 0.0 {
        (t2, true)
    } else {
        return None;
    };
    if t < ray.t_min || t > ray.t_max {
        return None;
    }
    let point = ray.origin + ray.dir * t;
    let to_centre = point - sphere.centre;
    let normal = if to_centre.length_squared() > 0.0 {
        to_centre.normalize()
    } else {
        DVec3::Y
    };
    Some(ShapeHit {
        t,
        point,
        normal,
        started_inside,
    })
}

// ---------------------------------------------------------------------------
// Ray vs AABB (axis-aligned box)
// ---------------------------------------------------------------------------

/// Intersect a ray with an axis-aligned bounding box (treated as a solid box).
/// Returns the closest hit, or `None` if the ray misses entirely.
#[must_use]
pub fn ray_vs_aabb(ray: &Ray, aabb: &Aabb) -> Option<ShapeHit> {
    let inv_dir = ray.dir.recip();
    let dir_is_neg = [ray.dir.x < 0.0, ray.dir.y < 0.0, ray.dir.z < 0.0];

    let (lox, hix) = if dir_is_neg[0] {
        (aabb.max.x, aabb.min.x)
    } else {
        (aabb.min.x, aabb.max.x)
    };
    let (loy, hiy) = if dir_is_neg[1] {
        (aabb.max.y, aabb.min.y)
    } else {
        (aabb.min.y, aabb.max.y)
    };
    let (loz, hiz) = if dir_is_neg[2] {
        (aabb.max.z, aabb.min.z)
    } else {
        (aabb.min.z, aabb.max.z)
    };

    // Handle zero-direction axes to avoid NaN from 0.0 * INF.
    let (tmin_x, tmax_x) = if inv_dir.x.is_finite() {
        (
            (lox - ray.origin.x) * inv_dir.x,
            (hix - ray.origin.x) * inv_dir.x,
        )
    } else if ray.origin.x < aabb.min.x || ray.origin.x > aabb.max.x {
        return None;
    } else {
        (f64::NEG_INFINITY, f64::INFINITY)
    };
    let (tmin_y, tmax_y) = if inv_dir.y.is_finite() {
        (
            (loy - ray.origin.y) * inv_dir.y,
            (hiy - ray.origin.y) * inv_dir.y,
        )
    } else if ray.origin.y < aabb.min.y || ray.origin.y > aabb.max.y {
        return None;
    } else {
        (f64::NEG_INFINITY, f64::INFINITY)
    };
    let (tmin_z, tmax_z) = if inv_dir.z.is_finite() {
        (
            (loz - ray.origin.z) * inv_dir.z,
            (hiz - ray.origin.z) * inv_dir.z,
        )
    } else if ray.origin.z < aabb.min.z || ray.origin.z > aabb.max.z {
        return None;
    } else {
        (f64::NEG_INFINITY, f64::INFINITY)
    };

    let tmin = tmin_x.max(tmin_y).max(tmin_z);
    let tmax = tmax_x.min(tmax_y).min(tmax_z);

    if tmin > tmax || tmax < 0.0 {
        return None;
    }
    let (t, started_inside) = if tmin >= 0.0 {
        (tmin, false)
    } else {
        (tmax, true)
    };
    if t < ray.t_min || t > ray.t_max {
        return None;
    }

    let point = ray.origin + ray.dir * t;

    // Compute the face normal of the box at the hit point.
    let eps = 1e-9;
    let normal = if (point.x - aabb.min.x).abs() < eps {
        DVec3::NEG_X
    } else if (point.x - aabb.max.x).abs() < eps {
        DVec3::X
    } else if (point.y - aabb.min.y).abs() < eps {
        DVec3::NEG_Y
    } else if (point.y - aabb.max.y).abs() < eps {
        DVec3::Y
    } else if (point.z - aabb.min.z).abs() < eps {
        DVec3::NEG_Z
    } else {
        DVec3::Z
    };

    Some(ShapeHit {
        t,
        point,
        normal,
        started_inside,
    })
}

// ---------------------------------------------------------------------------
// Ray vs capsule
// ---------------------------------------------------------------------------

/// Intersect a ray with a Y-aligned capsule. Returns the closest hit, or `None`.
#[must_use]
pub fn ray_vs_capsule(ray: &Ray, capsule: &Capsule) -> Option<ShapeHit> {
    // Treat the capsule as a segment (bottom→top) inflated by radius.
    let bot = capsule.bottom();
    let top = capsule.top();
    let seg_dir = top - bot; // always (0, 2*half_height, 0)

    // Find the closest point on the capsule segment to the ray.
    // Solve for ray parameter t and segment parameter s.

    // Ray: P(t) = ray.origin + ray.dir * t
    // Segment: Q(s) = bot + seg_dir * s, s in [0, 1]
    // Minimise distance squared: f(t, s) = |P(t) - Q(s)|^2

    let d = ray.origin - bot;
    let a = ray.dir.dot(ray.dir);
    let b = ray.dir.dot(seg_dir);
    let c = seg_dir.dot(seg_dir);
    let d_dot_dir = d.dot(ray.dir);
    let d_dot_seg = d.dot(seg_dir);

    let denom = a * c - b * b;
    let (t, s) = if denom.abs() <= f64::EPSILON {
        // Ray and segment are parallel — choose midpoint on segment, solve for t.
        let s = 0.5;
        let t_val = if a > 0.0 {
            (0.5 * b - d_dot_dir) / a
        } else {
            0.0
        };
        (t_val, s)
    } else {
        let s = (a * d_dot_seg - b * d_dot_dir) / denom;
        let s = s.clamp(0.0, 1.0);
        // Re-solve for t with clamped s: t = (s*b - d_dot_dir) / |D|²
        let t = (s * b - d_dot_dir) / a;
        (t, s)
    };

    let closest_on_seg = bot + seg_dir * s;
    let diff = ray.origin + ray.dir * t - closest_on_seg;
    let dist_sq = diff.length_squared();

    if dist_sq > capsule.radius * capsule.radius {
        return None; // Missed the capsule entirely.
    }

    // Compute the exact ray-capsule intersection.
    // We approach it as a cylinder + two hemispheres.
    // For simplicity, treat it as ray-vs-infinite-cylinder capped by ray-vs-sphere.

    // First try the cylindrical section.
    if let Some(hit) = ray_vs_capsule_cylinder(ray, capsule)
        && (ray.t_min..=ray.t_max).contains(&hit.t)
    {
        return Some(hit);
    }

    // Then try the hemispherical caps.
    if let Some(hit) = ray_vs_sphere(ray, &Sphere::new(top, capsule.radius))
        && (ray.t_min..=ray.t_max).contains(&hit.t)
    {
        return Some(hit);
    }
    if let Some(hit) = ray_vs_sphere(ray, &Sphere::new(bot, capsule.radius))
        && (ray.t_min..=ray.t_max).contains(&hit.t)
    {
        return Some(hit);
    }

    None
}

/// Intersect a ray with the infinite cylinder of a Y-aligned capsule.
fn ray_vs_capsule_cylinder(ray: &Ray, capsule: &Capsule) -> Option<ShapeHit> {
    // Cylinder along Y: x² + z² = r², clamped to [bottom.y, top.y]
    let bot = capsule.bottom();
    let top = capsule.top();

    // Project ray onto XZ plane.
    let oxz = DVec3::new(ray.origin.x, 0.0, ray.origin.z);
    let dxz = DVec3::new(ray.dir.x, 0.0, ray.dir.z);
    let cxz = DVec3::new(capsule.centre.x, 0.0, capsule.centre.z);

    let oc = oxz - cxz;
    let a = dxz.dot(dxz);
    if a <= f64::EPSILON {
        return None; // Ray is parallel to the cylinder axis.
    }
    let b = 2.0 * oc.dot(dxz);
    let c = oc.dot(oc) - capsule.radius * capsule.radius;
    let disc = b * b - 4.0 * a * c;
    if disc < 0.0 {
        return None;
    }

    let sqrt_disc = disc.sqrt();
    let t1 = (-b - sqrt_disc) / (2.0 * a);
    let t2 = (-b + sqrt_disc) / (2.0 * a);
    let mut t = t1.min(t2);
    if t < 0.0 {
        t = t1.max(t2);
        if t < 0.0 {
            return None;
        }
    }
    if t < ray.t_min || t > ray.t_max {
        return None;
    }

    // Check if the hit point is within the capsule's Y range.
    let point = ray.origin + ray.dir * t;
    if point.y < bot.y || point.y > top.y {
        return None;
    }

    // Normal is radial from the capsule axis.
    let radial = DVec3::new(point.x - capsule.centre.x, 0.0, point.z - capsule.centre.z);
    let normal = if radial.length_squared() > 0.0 {
        radial.normalize()
    } else {
        DVec3::Y
    };

    Some(ShapeHit {
        t,
        point,
        normal,
        started_inside: false,
    })
}

// ---------------------------------------------------------------------------
// Swept sphere vs sphere
// ---------------------------------------------------------------------------

/// Compute the time of impact (TOI) for a sphere moving along a segment against
/// a static target sphere. Returns the earliest time in `[0, 1]` and contact
/// details, or `None` if no collision occurs.
///
/// The `swept` sphere moves from `segment.start` to `segment.end`; the `target`
/// sphere is static at its position.
#[must_use]
pub fn swept_sphere_vs_sphere(
    segment: &crate::broadphase::Segment,
    swept_radius: f64,
    target: &Sphere,
) -> Option<ShapeHit> {
    let dir = segment.end - segment.start;
    let len = dir.length();
    if len <= 0.0 {
        // Stationary sweep: just test sphere-sphere overlap.
        let dist = (segment.start - target.centre).length();
        let combined = swept_radius + target.radius;
        if dist <= combined {
            let overlap = combined - dist;
            let normal = if dist > 0.0 {
                (segment.start - target.centre).normalize()
            } else {
                DVec3::Y
            };
            return Some(ShapeHit {
                t: 0.0,
                point: segment.start - normal * (swept_radius - overlap * 0.5),
                normal,
                started_inside: true,
            });
        }
        return None;
    }

    let combined_r = swept_radius + target.radius;
    let oc = segment.start - target.centre;

    // Solve: |oc + t * dir| = combined_r
    // a = dir·dir, b = 2 * oc·dir, c = oc·oc - combined_r²
    let a = dir.dot(dir);
    let b = 2.0 * oc.dot(dir);
    let c = oc.dot(oc) - combined_r * combined_r;
    let disc = b * b - 4.0 * a * c;
    if disc < 0.0 {
        return None;
    }
    let sqrt_disc = disc.sqrt();
    let t1 = (-b - sqrt_disc) / (2.0 * a);
    let t2 = (-b + sqrt_disc) / (2.0 * a);

    let t = if (0.0..=1.0).contains(&t1) {
        t1
    } else if (0.0..=1.0).contains(&t2) {
        t2
    } else {
        return None;
    };

    let point_on_swept = segment.start + dir * t;
    let to_target = point_on_swept - target.centre;
    let normal = if to_target.length_squared() > 0.0 {
        to_target.normalize()
    } else {
        DVec3::Y
    };
    let contact_point = target.centre + normal * target.radius;

    Some(ShapeHit {
        t,
        point: contact_point,
        normal,
        started_inside: false,
    })
}

// ---------------------------------------------------------------------------
// Swept sphere vs AABB
// ---------------------------------------------------------------------------

/// Compute the TOI for a sphere moving along a segment against a static AABB.
/// Uses the Minkowski sum: inflate the AABB by the sphere radius and test the
/// segment against the inflated box.
#[must_use]
pub fn swept_sphere_vs_aabb(
    segment: &crate::broadphase::Segment,
    swept_radius: f64,
    target: &Aabb,
) -> Option<ShapeHit> {
    let dir = segment.end - segment.start;
    let len = dir.length();
    if len <= 0.0 {
        // Stationary: test sphere vs AABB.
        let closest = DVec3::new(
            segment.start.x.clamp(target.min.x, target.max.x),
            segment.start.y.clamp(target.min.y, target.max.y),
            segment.start.z.clamp(target.min.z, target.max.z),
        );
        let diff = segment.start - closest;
        let dist_sq = diff.length_squared();
        if dist_sq <= swept_radius * swept_radius {
            let dist = dist_sq.sqrt();
            let normal = if dist > 0.0 { diff / dist } else { DVec3::Y };
            return Some(ShapeHit {
                t: 0.0,
                point: closest,
                normal,
                started_inside: true,
            });
        }
        return None;
    }

    // Inflate the AABB by swept_radius.
    let inflated = target.inflated(swept_radius);
    let inv_dir = dir.recip();
    let dir_is_neg = [dir.x < 0.0, dir.y < 0.0, dir.z < 0.0];
    let origin = segment.start;

    let (lox, hix) = if dir_is_neg[0] {
        (inflated.max.x, inflated.min.x)
    } else {
        (inflated.min.x, inflated.max.x)
    };
    let (loy, hiy) = if dir_is_neg[1] {
        (inflated.max.y, inflated.min.y)
    } else {
        (inflated.min.y, inflated.max.y)
    };
    let (loz, hiz) = if dir_is_neg[2] {
        (inflated.max.z, inflated.min.z)
    } else {
        (inflated.min.z, inflated.max.z)
    };

    // Handle zero-direction axes to avoid NaN from 0.0 * INF.
    let (tmin_x, tmax_x) = if inv_dir.x.is_finite() {
        ((lox - origin.x) * inv_dir.x, (hix - origin.x) * inv_dir.x)
    } else if origin.x < inflated.min.x || origin.x > inflated.max.x {
        return None;
    } else {
        (f64::NEG_INFINITY, f64::INFINITY)
    };
    let (tmin_y, tmax_y) = if inv_dir.y.is_finite() {
        ((loy - origin.y) * inv_dir.y, (hiy - origin.y) * inv_dir.y)
    } else if origin.y < inflated.min.y || origin.y > inflated.max.y {
        return None;
    } else {
        (f64::NEG_INFINITY, f64::INFINITY)
    };
    let (tmin_z, tmax_z) = if inv_dir.z.is_finite() {
        ((loz - origin.z) * inv_dir.z, (hiz - origin.z) * inv_dir.z)
    } else if origin.z < inflated.min.z || origin.z > inflated.max.z {
        return None;
    } else {
        (f64::NEG_INFINITY, f64::INFINITY)
    };

    let tmin = tmin_x.max(tmin_y).max(tmin_z);
    let tmax = tmax_x.min(tmax_y).min(tmax_z);

    // No intersection if the entry is past the exit or the exit is before the ray starts.
    if tmin > tmax || tmax < 0.0 {
        return None;
    }

    // tmin/tmax are the ray parameter such that P = O + t*D.
    let t_dirlen = tmin.max(0.0);
    let t_normalised = t_dirlen;

    if !(0.0..=1.0).contains(&t_normalised) {
        return None;
    }

    // Compute the hit point in world-space.
    let point_on_inflated = segment.start + dir * t_dirlen;

    // Compute the face normal of the *original* AABB that was struck.
    let eps = 1e-9;
    let normal = if (point_on_inflated.x - inflated.min.x).abs() < eps {
        DVec3::NEG_X
    } else if (point_on_inflated.x - inflated.max.x).abs() < eps {
        DVec3::X
    } else if (point_on_inflated.y - inflated.min.y).abs() < eps {
        DVec3::NEG_Y
    } else if (point_on_inflated.y - inflated.max.y).abs() < eps {
        DVec3::Y
    } else if (point_on_inflated.z - inflated.min.z).abs() < eps {
        DVec3::NEG_Z
    } else {
        DVec3::Z
    };

    // Contact point on the original AABB surface.
    let contact_point = DVec3::new(
        point_on_inflated.x.clamp(target.min.x, target.max.x),
        point_on_inflated.y.clamp(target.min.y, target.max.y),
        point_on_inflated.z.clamp(target.min.z, target.max.z),
    );

    Some(ShapeHit {
        t: t_normalised,
        point: contact_point,
        normal,
        started_inside: false,
    })
}

// ---------------------------------------------------------------------------
// Swept sphere vs capsule
// ---------------------------------------------------------------------------

/// Compute the TOI for a sphere moving along a segment against a static
/// Y-aligned capsule.
///
/// The capsule is inflated by `swept_radius` and the segment (sphere centre
/// path) is tested against the inflated shape via ray-vs-capsule.
#[must_use]
pub fn swept_sphere_vs_capsule(
    segment: &crate::broadphase::Segment,
    swept_radius: f64,
    capsule: &Capsule,
) -> Option<ShapeHit> {
    let dir = segment.end - segment.start;
    let len = dir.length();
    if len <= 0.0 {
        // Stationary: test sphere-vs-capsule overlap.
        let sphere = Sphere::new(segment.start, swept_radius);
        if sphere_overlaps_capsule(&sphere, capsule) {
            let to_capsule = segment.start - capsule.centre;
            let normal = if to_capsule.length_squared() > 0.0 {
                to_capsule.normalize()
            } else {
                DVec3::Y
            };
            return Some(ShapeHit {
                t: 0.0,
                point: segment.start,
                normal,
                started_inside: true,
            });
        }
        return None;
    }

    // Inflate the capsule by the swept sphere radius.  The segment (sphere
    // centre path) vs the inflated capsule is a ray-vs-capsule test.
    let inflated = Capsule::new(
        capsule.centre,
        capsule.radius + swept_radius,
        capsule.half_height,
    );
    let ray = Ray::new(segment.start, dir).with_bounds(0.0, 1.0);

    ray_vs_capsule(&ray, &inflated).map(|hit| {
        // Back out the contact point on the original capsule surface.
        let contact_point = hit.point - hit.normal * swept_radius;
        ShapeHit {
            t: hit.t, // already in [0, 1] because t_max = 1.0
            point: contact_point,
            normal: hit.normal,
            started_inside: hit.started_inside,
        }
    })
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use crate::broadphase::Ray;

    // ── Ray vs sphere ─────────────────────────────────────────────────

    #[test]
    fn ray_hits_sphere_dead_centre() {
        let ray = Ray::new(DVec3::new(-5.0, 0.0, 0.0), DVec3::X);
        let sphere = Sphere::new(DVec3::ZERO, 1.0);
        let hit = ray_vs_sphere(&ray, &sphere).unwrap();
        assert!((hit.t - 4.0).abs() < 0.001);
        assert_eq!(hit.normal, DVec3::NEG_X);
    }

    #[test]
    fn ray_misses_sphere() {
        let ray = Ray::new(DVec3::new(-5.0, 3.0, 0.0), DVec3::X);
        let sphere = Sphere::new(DVec3::ZERO, 1.0);
        assert!(ray_vs_sphere(&ray, &sphere).is_none());
    }

    #[test]
    fn ray_starts_inside_sphere() {
        let ray = Ray::new(DVec3::ZERO, DVec3::X);
        let sphere = Sphere::new(DVec3::ZERO, 2.0);
        let hit = ray_vs_sphere(&ray, &sphere).unwrap();
        assert!(hit.started_inside);
        assert!((hit.t - 2.0).abs() < 0.001);
    }

    #[test]
    fn ray_sphere_behind_origin() {
        let ray = Ray::new(DVec3::new(5.0, 0.0, 0.0), DVec3::NEG_X);
        let sphere = Sphere::new(DVec3::ZERO, 1.0);
        let hit = ray_vs_sphere(&ray, &sphere).unwrap();
        assert!((hit.t - 4.0).abs() < 0.001);
    }

    // ── Ray vs AABB ───────────────────────────────────────────────────

    #[test]
    fn ray_hits_aabb_centre() {
        let ray = Ray::new(DVec3::new(-5.0, 0.0, 0.0), DVec3::X);
        let aabb = Aabb::from_centre_half(DVec3::ZERO, DVec3::splat(1.0));
        let hit = ray_vs_aabb(&ray, &aabb).unwrap();
        assert!((hit.t - 4.0).abs() < 0.001);
        assert_eq!(hit.normal, DVec3::NEG_X);
    }

    #[test]
    fn ray_misses_aabb() {
        let ray = Ray::new(DVec3::new(-5.0, 5.0, 0.0), DVec3::X);
        let aabb = Aabb::from_centre_half(DVec3::ZERO, DVec3::splat(1.0));
        assert!(ray_vs_aabb(&ray, &aabb).is_none());
    }

    #[test]
    fn ray_hits_aabb_from_inside() {
        let ray = Ray::new(DVec3::ZERO, DVec3::X);
        let aabb = Aabb::from_centre_half(DVec3::ZERO, DVec3::splat(1.0));
        let hit = ray_vs_aabb(&ray, &aabb).unwrap();
        assert!(hit.started_inside);
        assert!((hit.t - 1.0).abs() < 0.001);
    }

    #[test]
    fn ray_hits_aabb_y_face() {
        let ray = Ray::new(DVec3::new(0.0, -5.0, 0.0), DVec3::Y);
        let aabb = Aabb::from_centre_half(DVec3::ZERO, DVec3::splat(1.0));
        let hit = ray_vs_aabb(&ray, &aabb).unwrap();
        assert_eq!(hit.normal, DVec3::NEG_Y);
        assert!((hit.t - 4.0).abs() < 0.001);
    }

    #[test]
    fn ray_hits_aabb_z_face() {
        let ray = Ray::new(DVec3::new(0.0, 0.0, -5.0), DVec3::Z);
        let aabb = Aabb::from_centre_half(DVec3::ZERO, DVec3::splat(1.0));
        let hit = ray_vs_aabb(&ray, &aabb).unwrap();
        assert_eq!(hit.normal, DVec3::NEG_Z);
    }

    #[test]
    fn ray_on_aabb_face_zero_dir_component_hits() {
        // Ray starts on +Y face with zero Y direction — would NaN before fix.
        let ray = Ray::new(DVec3::new(0.0, 1.0, 0.0), DVec3::new(1.0, 0.0, 0.0));
        let aabb = Aabb::from_centre_half(DVec3::ZERO, DVec3::splat(1.0));
        let hit = ray_vs_aabb(&ray, &aabb).unwrap();
        assert!(hit.started_inside);
        assert!((hit.t - 1.0).abs() < 0.001);
        assert_eq!(hit.normal, DVec3::X);
    }

    #[test]
    fn ray_parallel_to_axis_outside_misses() {
        // Ray outside AABB on Y, zero Y direction — should miss.
        let ray = Ray::new(DVec3::new(0.0, 5.0, 0.0), DVec3::new(1.0, 0.0, 0.0));
        let aabb = Aabb::from_centre_half(DVec3::ZERO, DVec3::splat(1.0));
        assert!(ray_vs_aabb(&ray, &aabb).is_none());
    }

    // ── Ray vs capsule ────────────────────────────────────────────────

    #[test]
    fn ray_hits_capsule_cylinder() {
        let capsule = Capsule::new(DVec3::ZERO, 0.5, 1.0);
        let ray = Ray::new(DVec3::new(-5.0, 0.0, 0.0), DVec3::X);
        let hit = ray_vs_capsule(&ray, &capsule).unwrap();
        assert!((hit.t - 4.5).abs() < 0.001);
    }

    #[test]
    fn ray_hits_capsule_top_cap() {
        let capsule = Capsule::new(DVec3::ZERO, 0.5, 1.0);
        // Aim at the top hemisphere centre.
        let ray = Ray::new(DVec3::new(0.0, 5.0, 0.0), DVec3::NEG_Y);
        let hit = ray_vs_capsule(&ray, &capsule).unwrap();
        assert!((hit.t - 3.5).abs() < 0.001);
    }

    #[test]
    fn ray_misses_capsule() {
        let capsule = Capsule::new(DVec3::ZERO, 0.5, 1.0);
        let ray = Ray::new(DVec3::new(-5.0, 3.0, 0.0), DVec3::X);
        assert!(ray_vs_capsule(&ray, &capsule).is_none());
    }

    // ── Swept sphere vs sphere ────────────────────────────────────────

    #[test]
    fn swept_sphere_hits_static_sphere() {
        let seg =
            crate::broadphase::Segment::new(DVec3::new(-5.0, 0.0, 0.0), DVec3::new(5.0, 0.0, 0.0));
        let target = Sphere::new(DVec3::ZERO, 1.0);
        let hit = swept_sphere_vs_sphere(&seg, 0.5, &target).unwrap();
        // Combined radius = 1.5, start at x=-5, hit at x=-1.5
        // t = (5 - 1.5) / 10 = 0.35
        // Actually: distance to travel = 10, distance before contact = 5 - 1.5 = 3.5
        // t = 3.5 / 10 = 0.35
        assert!((hit.t - 0.35).abs() < 0.001, "expected 0.35, got {}", hit.t);
    }

    #[test]
    fn swept_sphere_misses_static_sphere() {
        let seg =
            crate::broadphase::Segment::new(DVec3::new(-5.0, 5.0, 0.0), DVec3::new(5.0, 5.0, 0.0));
        let target = Sphere::new(DVec3::ZERO, 1.0);
        assert!(swept_sphere_vs_sphere(&seg, 0.5, &target).is_none());
    }

    #[test]
    fn swept_sphere_starts_overlapping() {
        let seg =
            crate::broadphase::Segment::new(DVec3::new(0.5, 0.0, 0.0), DVec3::new(5.0, 0.0, 0.0));
        let target = Sphere::new(DVec3::ZERO, 1.0);
        let hit = swept_sphere_vs_sphere(&seg, 0.5, &target);
        assert!(hit.is_some());
    }

    // ── Swept sphere vs AABB ──────────────────────────────────────────

    #[test]
    fn swept_sphere_hits_aabb() {
        let seg =
            crate::broadphase::Segment::new(DVec3::new(-5.0, 0.0, 0.0), DVec3::new(5.0, 0.0, 0.0));
        let aabb = Aabb::from_centre_half(DVec3::ZERO, DVec3::splat(1.0));
        let hit = swept_sphere_vs_aabb(&seg, 0.5, &aabb).unwrap();
        // Sphere radius 0.5, AABB half-extent 1.0. Inflated AABB spans [-1.5, 1.5].
        // Distance from start (-5) to inflated near plane (-1.5) = 3.5.
        // Segment length = 10. t = 3.5 / 10 = 0.35
        assert!((hit.t - 0.35).abs() < 0.001, "expected 0.35, got {}", hit.t);
        assert_eq!(hit.normal, DVec3::NEG_X);
    }

    #[test]
    fn swept_sphere_misses_aabb() {
        let seg =
            crate::broadphase::Segment::new(DVec3::new(-5.0, 5.0, 0.0), DVec3::new(5.0, 5.0, 0.0));
        let aabb = Aabb::from_centre_half(DVec3::ZERO, DVec3::splat(1.0));
        assert!(swept_sphere_vs_aabb(&seg, 0.5, &aabb).is_none());
    }

    #[test]
    fn swept_sphere_vs_aabb_starts_overlapping() {
        let seg =
            crate::broadphase::Segment::new(DVec3::new(0.5, 0.0, 0.0), DVec3::new(5.0, 0.0, 0.0));
        let aabb = Aabb::from_centre_half(DVec3::ZERO, DVec3::splat(1.0));
        let hit = swept_sphere_vs_aabb(&seg, 0.5, &aabb);
        assert!(hit.is_some());
    }

    #[test]
    fn swept_sphere_vs_aabb_touches_y_face() {
        let seg =
            crate::broadphase::Segment::new(DVec3::new(0.0, -5.0, 0.0), DVec3::new(0.0, 5.0, 0.0));
        let aabb = Aabb::from_centre_half(DVec3::ZERO, DVec3::splat(1.0));
        let hit = swept_sphere_vs_aabb(&seg, 0.5, &aabb).unwrap();
        assert_eq!(hit.normal, DVec3::NEG_Y);
    }

    #[test]
    fn swept_sphere_zero_dir_on_face_hits() {
        // Sphere starts on inflated Y face with zero Y direction.
        // AABB half-extent 1.0, radius 0.5 → inflated spans [-1.5, 1.5].
        // Origin at y=1.5 (on inflated +Y face), dir = (1,0,0) (zero Y).
        let seg =
            crate::broadphase::Segment::new(DVec3::new(-2.0, 1.5, 0.0), DVec3::new(2.0, 1.5, 0.0));
        let aabb = Aabb::from_centre_half(DVec3::ZERO, DVec3::splat(1.0));
        let hit = swept_sphere_vs_aabb(&seg, 0.5, &aabb).unwrap();
        assert!(hit.t >= 0.0 && hit.t <= 1.0);
    }

    // ── Overlap queries ────────────────────────────────────────────────

    #[test]
    fn spheres_overlap_when_close() {
        let a = Sphere::new(DVec3::ZERO, 1.0);
        let b = Sphere::new(DVec3::new(1.5, 0.0, 0.0), 1.0);
        assert!(sphere_overlaps_sphere(&a, &b));
    }

    #[test]
    fn spheres_do_not_overlap_when_far() {
        let a = Sphere::new(DVec3::ZERO, 1.0);
        let b = Sphere::new(DVec3::new(5.0, 0.0, 0.0), 1.0);
        assert!(!sphere_overlaps_sphere(&a, &b));
    }

    #[test]
    fn sphere_overlaps_aabb_when_intersecting() {
        let sphere = Sphere::new(DVec3::new(2.0, 0.0, 0.0), 1.5);
        let aabb = Aabb::from_centre_half(DVec3::ZERO, DVec3::splat(1.0));
        assert!(sphere_overlaps_aabb(&sphere, &aabb));
    }

    #[test]
    fn sphere_does_not_overlap_distant_aabb() {
        let sphere = Sphere::new(DVec3::new(5.0, 0.0, 0.0), 1.0);
        let aabb = Aabb::from_centre_half(DVec3::ZERO, DVec3::splat(1.0));
        assert!(!sphere_overlaps_aabb(&sphere, &aabb));
    }

    #[test]
    fn sphere_overlaps_capsule_body() {
        let sphere = Sphere::new(DVec3::new(1.0, 0.0, 0.0), 0.6);
        let capsule = Capsule::new(DVec3::ZERO, 0.5, 2.0);
        assert!(sphere_overlaps_capsule(&sphere, &capsule));
    }

    #[test]
    fn sphere_overlaps_capsule_cap() {
        let sphere = Sphere::new(DVec3::new(0.0, 3.0, 0.0), 0.6);
        let capsule = Capsule::new(DVec3::ZERO, 0.5, 2.0);
        // capsule top is at y=2.0, radius 0.5 → top cap spans y ∈ [1.5, 2.5]
        // sphere at y=3.0, radius 0.6, combined radius 1.1 → distance 1.0 < 1.1
        assert!(sphere_overlaps_capsule(&sphere, &capsule));
    }

    #[test]
    fn sphere_does_not_overlap_distant_capsule() {
        let sphere = Sphere::new(DVec3::new(10.0, 0.0, 0.0), 1.0);
        let capsule = Capsule::new(DVec3::ZERO, 0.5, 2.0);
        assert!(!sphere_overlaps_capsule(&sphere, &capsule));
    }

    // ── Swept sphere vs capsule ────────────────────────────────────────

    #[test]
    fn swept_sphere_hits_capsule_cylinder() {
        let seg =
            crate::broadphase::Segment::new(DVec3::new(-5.0, 0.0, 0.0), DVec3::new(5.0, 0.0, 0.0));
        let capsule = Capsule::new(DVec3::ZERO, 0.5, 2.0);
        let hit = swept_sphere_vs_capsule(&seg, 0.5, &capsule).unwrap();
        // Combined radius = 1.0, capsule at origin, swept sphere starts at -5.
        // Should hit the cylinder at approximately t = (5 - 1) / 10 = 0.4.
        assert!(hit.t > 0.0 && hit.t < 1.0);
        assert!((hit.normal.x + 1.0).abs() < 0.1); // normal is roughly -X
    }

    #[test]
    fn swept_sphere_misses_capsule() {
        let seg =
            crate::broadphase::Segment::new(DVec3::new(-5.0, 5.0, 0.0), DVec3::new(5.0, 5.0, 0.0));
        let capsule = Capsule::new(DVec3::ZERO, 0.5, 2.0);
        assert!(swept_sphere_vs_capsule(&seg, 0.5, &capsule).is_none());
    }

    #[test]
    fn swept_sphere_vs_capsule_touches_top_cap() {
        // Swept sphere moving downward toward the top cap.
        let seg =
            crate::broadphase::Segment::new(DVec3::new(0.0, 5.0, 0.0), DVec3::new(0.0, 0.0, 0.0));
        let capsule = Capsule::new(DVec3::ZERO, 0.5, 2.0);
        // Capsule top at y=2.0, radius 0.5. Swept sphere radius 0.5.
        // Combined radius = 1.0. Hit at t = (5 - 2 - 1) / 5 = 2/5 = 0.4.
        let hit = swept_sphere_vs_capsule(&seg, 0.5, &capsule).unwrap();
        assert!((hit.t - 0.4).abs() < 0.01); // near t=0.4
        assert!(hit.normal.y > 0.5); // pointing away from capsule (upward)
    }
}
