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
                part: 0,
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
        part: 0,
    })
}

/// The time of impact of a sphere moving along `segment` against a static
/// AABB, **exactly**: against the box rounded by the radius — its faces moved
/// out, its edges cylinders, its corners spheres — where
/// [`swept_sphere_vs_aabb`] sweeps against the box inflated into a larger box
/// with square edges and corners.
///
/// Ericson, *Real-Time Collision Detection*, §5.5.7: the sweep enters the
/// inflated box first. Where it enters beyond the box on at most one axis, a
/// face's region, that is the hit. Where it enters beyond it on two, an edge's
/// region, the hit is the segment against that edge's capsule of the radius,
/// and on three, a corner's, the first of the three edges' capsules meeting
/// there; a segment that misses them misses the rounded box, which is convex.
/// A sweep starting within the radius of the box meets it at `t = 0` with
/// [`ShapeHit::started_inside`].
pub(crate) fn swept_sphere_vs_rounded_aabb(
    segment: &crate::broadphase::Segment,
    swept_radius: f64,
    target: &Aabb,
) -> Option<ShapeHit> {
    let hit = swept_sphere_vs_aabb(segment, swept_radius, target)?;
    let dir = segment.end - segment.start;
    let entry = segment.start + dir * hit.t;
    let below = [0, 1, 2].map(|axis| entry[axis] < target.min[axis]);
    let above = [0, 1, 2].map(|axis| entry[axis] > target.max[axis]);
    let beyond = (0..3).filter(|&axis| below[axis] || above[axis]).count();
    if beyond <= 1 {
        return Some(hit);
    }
    // The corner the entry is nearest, and the edges through it to test: the
    // one along the axis the entry is within, or all three at a corner.
    let corner = DVec3::from_array([0, 1, 2].map(|axis| {
        if below[axis] {
            target.min[axis]
        } else {
            target.max[axis]
        }
    }));
    let along = (0..3).filter(|&axis| beyond == 3 || !(below[axis] || above[axis]));
    along
        .filter_map(|axis| {
            let mut centre = corner;
            centre[axis] = (target.min[axis] + target.max[axis]) * 0.5;
            let half = (target.max[axis] - target.min[axis]) * 0.5;
            // The edge turned onto Y, which `ray_vs_capsule` takes, by
            // swapping two coordinates: exact, unlike a rotation.
            let edge = Capsule::new(swap_with_y(centre, axis), swept_radius, half);
            let ray = Ray::new(swap_with_y(segment.start, axis), swap_with_y(dir, axis))
                .with_bounds(0.0, 1.0);
            let found = ray_vs_capsule(&ray, &edge)?;
            let normal = swap_with_y(found.normal, axis);
            Some(ShapeHit {
                t: found.t,
                point: segment.start + dir * found.t - normal * swept_radius,
                normal,
                started_inside: found.started_inside,
                part: 0,
            })
        })
        .min_by(|a, b| a.t.total_cmp(&b.t))
}

/// `v` with its `axis` and Y coordinates swapped.
fn swap_with_y(v: DVec3, axis: usize) -> DVec3 {
    let mut swapped = v;
    swapped[1] = v[axis];
    swapped[axis] = v[1];
    swapped
}

#[cfg(test)]
#[path = "aabb_sweep_tests.rs"]
mod tests;
