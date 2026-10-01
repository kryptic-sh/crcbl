use super::*;

/// Compute the TOI for a sphere moving along a segment against a static AABB.
/// Uses the Minkowski sum: inflate the AABB by the sphere radius and test the
/// segment against the inflated box.
///
/// A sweep that starts already overlapping reports `t = 0`,
/// [`ShapeHit::started_inside`], and the minimum-penetration face normal.
#[must_use]
pub fn swept_sphere_vs_aabb(
    segment: &crate::broadphase::Segment,
    swept_radius: f64,
    target: &Aabb,
) -> Option<ShapeHit> {
    let dir = segment.end - segment.start;
    // An inverted box (e.g. `Aabb::EMPTY`) overlaps nothing; both the clamp in
    // the stationary branch and the inflated slab would misbehave on it.
    if target.is_empty() {
        return None;
    }
    let distance_scale = dir.abs().max_element();
    if distance_scale == 0.0 {
        // Stationary: test sphere vs AABB.
        let closest = DVec3::new(
            segment.start.x.clamp(target.min.x, target.max.x),
            segment.start.y.clamp(target.min.y, target.max.y),
            segment.start.z.clamp(target.min.z, target.max.z),
        );
        let diff = segment.start - closest;
        let overlap_scale = diff.abs().max_element();
        let dist = if overlap_scale > 0.0 {
            (diff / overlap_scale).length() * overlap_scale
        } else {
            0.0
        };
        if dist <= swept_radius {
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

    // Inflate the AABB by swept_radius: the sphere centre path against the
    // inflated box is the same intersection problem as the sphere against the
    // original box.
    let inflated = target.inflated(swept_radius);
    // Scale the direction before taking its reciprocal so tiny nonzero
    // motion retains a finite slab interval instead of becoming stationary.
    let inv_dir = (dir / distance_scale).recip();
    let dir_is_neg = [dir.x < 0.0, dir.y < 0.0, dir.z < 0.0];

    let (near, far) = inflated.ray_slab(segment.start, inv_dir, dir_is_neg)?;
    let (distance, started_inside) =
        select_hit(near, far, 0.0, distance_scale, InsideRule::Contact)?;
    let t = distance / distance_scale;

    // Compute the hit point in world-space.  Inside the inflated box (a sweep
    // that starts overlapping) the nearest face is the minimum-penetration
    // escape axis, which is the normal a caller needs to push the sphere out.
    let point_on_inflated = segment.start + dir * t;
    let normal = aabb_escape(&inflated, point_on_inflated).0;

    // Contact point on the original AABB surface.
    let contact_point = DVec3::new(
        point_on_inflated.x.clamp(target.min.x, target.max.x),
        point_on_inflated.y.clamp(target.min.y, target.max.y),
        point_on_inflated.z.clamp(target.min.z, target.max.z),
    );

    Some(ShapeHit {
        t,
        point: contact_point,
        normal,
        started_inside,
    })
}

#[cfg(test)]
#[path = "aabb_sweep_tests.rs"]
mod tests;
