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
    // An inverted box (e.g. `Aabb::EMPTY`) overlaps nothing; clamping against
    // it would assert on `min > max`.
    if aabb.is_empty() {
        return false;
    }
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
    let closest = closest_point_on_capsule_axis(capsule, sphere.centre);
    let combined = sphere.radius + capsule.radius;
    (sphere.centre - closest).length_squared() <= combined * combined
}

/// The point on a capsule's axis segment (bottom cap centre → top cap centre)
/// closest to `point`.
///
/// The capsule surface is exactly the set of points at distance `radius` from
/// this segment, so this is the anchor for every capsule normal and distance
/// in this module. A degenerate (zero-height) capsule returns its centre.
#[inline]
#[must_use]
fn closest_point_on_capsule_axis(capsule: &Capsule, point: DVec3) -> DVec3 {
    let bot = capsule.bottom();
    let seg = capsule.top() - bot;
    let len_sq = seg.length_squared();
    if len_sq <= 0.0 {
        return capsule.centre;
    }
    bot + seg * ((point - bot).dot(seg) / len_sq).clamp(0.0, 1.0)
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
    /// Whether the query's accepted interval *begins* inside the shape — for
    /// a ray that is `origin + dir * t_min`, for a sweep the sweep's start
    /// position. A hit with this set is a resolution case (push out), not an
    /// approach.
    pub started_inside: bool,
}

// ---------------------------------------------------------------------------
// Shared root selection
// ---------------------------------------------------------------------------

/// What a query reports when its accepted interval starts inside the shape.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum InsideRule {
    /// Report the far root: the surface the ray leaves through. Ray casts use
    /// this — a ray fired from inside a volume strikes its far wall.
    Exit,
    /// Report the start of the interval: the shapes are already touching, so
    /// the time of impact is now. Sweeps use this.
    Contact,
}

/// Solve `a·t² + b·t + c = 0`, returning the roots in ascending order.
///
/// `None` means no real intersection: either the quadratic is degenerate
/// (`a` at or below the noise floor — a ray with no length along the axis
/// being solved) or the discriminant is negative (a miss).
#[inline]
fn solve_quadratic(a: f64, b: f64, c: f64) -> Option<(f64, f64)> {
    if a <= f64::EPSILON {
        return None;
    }
    let disc = b * b - 4.0 * a * c;
    if disc < 0.0 {
        return None;
    }
    let sqrt_disc = disc.sqrt();
    let inv_2a = 0.5 / a;
    Some(((-b - sqrt_disc) * inv_2a, (-b + sqrt_disc) * inv_2a))
}

/// The one root-selection rule in this module: every quadratic and every AABB
/// slab test resolves its `(near, far)` interval through here.
///
/// `near..=far` is the interval over which the ray is inside the shape and
/// `lo..=hi` the interval the caller accepts. The result is the reported `t`
/// and whether the accepted interval began inside the shape.
///
/// * Disjoint intervals are a miss.
/// * A `near` inside `lo..=hi` is the hit, from outside the shape.
/// * Otherwise `lo` is already inside the shape and `rule` decides what to
///   report. Note this covers a caller that merely *advanced* `lo` past the
///   entry point (`Ray::with_bounds`) as well as one that genuinely starts
///   within the shape — both must still produce a hit.
#[inline]
fn select_hit(near: f64, far: f64, lo: f64, hi: f64, rule: InsideRule) -> Option<(f64, bool)> {
    if near > far || far < lo || near > hi {
        return None;
    }
    if near >= lo {
        return Some((near, false));
    }
    match rule {
        InsideRule::Exit if far <= hi => Some((far, true)),
        InsideRule::Exit => None,
        InsideRule::Contact => Some((lo, true)),
    }
}

/// The outward unit normal of the box face nearest to `point`, and how far
/// `point` is from that face.
///
/// Doubles as the surface normal for a hit lying on a face (distance zero) and
/// as the minimum-penetration escape axis for a point inside the box, so
/// callers never have to fabricate a normal when a query starts overlapping.
/// The distance is what a caller pushing that point back out has to travel.
#[must_use]
fn aabb_escape(aabb: &Aabb, point: DVec3) -> (DVec3, f64) {
    let faces = [
        ((point.x - aabb.min.x).abs(), DVec3::NEG_X),
        ((aabb.max.x - point.x).abs(), DVec3::X),
        ((point.y - aabb.min.y).abs(), DVec3::NEG_Y),
        ((aabb.max.y - point.y).abs(), DVec3::Y),
        ((point.z - aabb.min.z).abs(), DVec3::NEG_Z),
        ((aabb.max.z - point.z).abs(), DVec3::Z),
    ];
    let mut best = faces[0];
    for &face in &faces[1..] {
        if face.0 < best.0 {
            best = face;
        }
    }
    (best.1, best.0)
}

// ---------------------------------------------------------------------------
// Ray vs sphere
// ---------------------------------------------------------------------------

/// Intersect a ray with a sphere. Returns the nearest hit within the ray's
/// bounds, or `None` if the ray misses entirely (including if the sphere is
/// behind the ray origin).
///
/// A ray whose accepted interval starts inside the sphere hits the far surface
/// and reports [`ShapeHit::started_inside`].
#[must_use]
pub fn ray_vs_sphere(ray: &Ray, sphere: &Sphere) -> Option<ShapeHit> {
    let oc = ray.origin - sphere.centre;
    let a = ray.dir.dot(ray.dir);
    let b = 2.0 * oc.dot(ray.dir);
    let c = oc.dot(oc) - sphere.radius * sphere.radius;
    let (near, far) = solve_quadratic(a, b, c)?;
    let (t, started_inside) = select_hit(near, far, ray.t_min, ray.t_max, InsideRule::Exit)?;
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
/// Returns the nearest hit within the ray's bounds, or `None` if the ray
/// misses entirely.
///
/// A ray whose accepted interval starts inside the box hits the far face and
/// reports [`ShapeHit::started_inside`].
#[must_use]
pub fn ray_vs_aabb(ray: &Ray, aabb: &Aabb) -> Option<ShapeHit> {
    let inv_dir = ray.dir.recip();
    let dir_is_neg = [ray.dir.x < 0.0, ray.dir.y < 0.0, ray.dir.z < 0.0];

    let (near, far) = aabb.ray_slab(ray.origin, inv_dir, dir_is_neg)?;
    let (t, started_inside) = select_hit(near, far, ray.t_min, ray.t_max, InsideRule::Exit)?;

    let point = ray.origin + ray.dir * t;
    let normal = aabb_escape(aabb, point).0;

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

/// Intersect a ray with a Y-aligned capsule. Returns the nearest hit within
/// the ray's bounds, or `None`.
///
/// The capsule surface is the union of three pieces — the cylindrical band
/// between the cap centres, the top hemisphere above the top cap centre, and
/// the bottom hemisphere below the bottom one. Every root of all three is
/// tested, each is kept only if it lands on the piece that owns it (the rest
/// of each cap sphere lies *inside* the capsule), and the nearest survivor
/// wins. Testing the pieces in sequence instead would return a far-side
/// cylinder exit in preference to a nearer cap hit.
#[must_use]
pub fn ray_vs_capsule(ray: &Ray, capsule: &Capsule) -> Option<ShapeHit> {
    let bot = capsule.bottom();
    let top = capsule.top();
    let radius_sq = capsule.radius * capsule.radius;

    let mut best = f64::INFINITY;
    let mut consider = |t: f64| {
        if t >= ray.t_min && t <= ray.t_max && t < best {
            best = t;
        }
    };

    // Cylindrical band: solve in the XZ plane, keep roots between the caps.
    let oc_xz = DVec3::new(
        ray.origin.x - capsule.centre.x,
        0.0,
        ray.origin.z - capsule.centre.z,
    );
    let dir_xz = DVec3::new(ray.dir.x, 0.0, ray.dir.z);
    if let Some(roots) = solve_quadratic(
        dir_xz.dot(dir_xz),
        2.0 * oc_xz.dot(dir_xz),
        oc_xz.dot(oc_xz) - radius_sq,
    ) {
        for t in [roots.0, roots.1] {
            let y = ray.origin.y + ray.dir.y * t;
            if y >= bot.y && y <= top.y {
                consider(t);
            }
        }
    }

    // Hemispherical caps: keep only the roots beyond the cap centre's plane.
    let a = ray.dir.dot(ray.dir);
    for (cap, is_top) in [(top, true), (bot, false)] {
        let oc = ray.origin - cap;
        if let Some(roots) = solve_quadratic(a, 2.0 * oc.dot(ray.dir), oc.dot(oc) - radius_sq) {
            for t in [roots.0, roots.1] {
                let y = ray.origin.y + ray.dir.y * t;
                if (is_top && y >= top.y) || (!is_top && y <= bot.y) {
                    consider(t);
                }
            }
        }
    }

    if !best.is_finite() {
        return None;
    }

    let point = ray.origin + ray.dir * best;
    let axis_point = closest_point_on_capsule_axis(capsule, point);
    let radial = point - axis_point;
    let normal = if radial.length_squared() > 0.0 {
        radial.normalize()
    } else {
        DVec3::Y
    };

    // The interval starts inside the capsule when the point at `t_min` is
    // within `radius` of the axis segment.
    let entry = ray.origin + ray.dir * ray.t_min;
    let started_inside =
        (entry - closest_point_on_capsule_axis(capsule, entry)).length_squared() <= radius_sq;

    Some(ShapeHit {
        t: best,
        point,
        normal,
        started_inside,
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
///
/// A sweep that is already overlapping at `segment.start` reports `t = 0` and
/// [`ShapeHit::started_inside`] — including when it is overlapping for the
/// whole sweep — so a body resting inside another still produces a contact.
#[must_use]
pub fn swept_sphere_vs_sphere(
    segment: &crate::broadphase::Segment,
    swept_radius: f64,
    target: &Sphere,
) -> Option<ShapeHit> {
    let dir = segment.end - segment.start;
    let combined_r = swept_radius + target.radius;
    let oc = segment.start - target.centre;

    // A zero or near-stationary sweep (dir·dir at or below the quadratic
    // floor) takes the overlap branch: `solve_quadratic` rejects `a <= EPSILON`
    // outright, so anything shorter would be reported as a miss instead of the
    // documented resting contact.
    if dir.length_squared() <= f64::EPSILON {
        // Stationary sweep: just test sphere-sphere overlap.
        let dist = oc.length();
        if dist <= combined_r {
            let overlap = combined_r - dist;
            let normal = if dist > 0.0 { oc / dist } else { DVec3::Y };
            return Some(ShapeHit {
                t: 0.0,
                point: segment.start - normal * (swept_radius - overlap * 0.5),
                normal,
                started_inside: true,
            });
        }
        return None;
    }

    // Solve: |oc + t * dir| = combined_r
    // a = dir·dir, b = 2 * oc·dir, c = oc·oc - combined_r²
    let (near, far) = solve_quadratic(
        dir.dot(dir),
        2.0 * oc.dot(dir),
        oc.dot(oc) - combined_r * combined_r,
    )?;
    let (t, started_inside) = select_hit(near, far, 0.0, 1.0, InsideRule::Contact)?;

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
        started_inside,
    })
}

// ---------------------------------------------------------------------------
// Swept sphere vs AABB
// ---------------------------------------------------------------------------

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
    // A zero or near-stationary sweep (dir·dir at or below the quadratic
    // floor) takes the overlap branch — with a tiny `dir` the slab t-values
    // blow up, and the sphere sweep's quadratic rejects the same range.
    if dir.length_squared() <= f64::EPSILON {
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

    // Inflate the AABB by swept_radius: the sphere centre path against the
    // inflated box is the same intersection problem as the sphere against the
    // original box.
    let inflated = target.inflated(swept_radius);
    let inv_dir = dir.recip();
    let dir_is_neg = [dir.x < 0.0, dir.y < 0.0, dir.z < 0.0];

    let (near, far) = inflated.ray_slab(segment.start, inv_dir, dir_is_neg)?;
    let (t, started_inside) = select_hit(near, far, 0.0, 1.0, InsideRule::Contact)?;

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

// ---------------------------------------------------------------------------
// Swept sphere vs capsule
// ---------------------------------------------------------------------------

/// Compute the TOI for a sphere moving along a segment against a static
/// Y-aligned capsule.
///
/// The capsule is inflated by `swept_radius` and the segment (sphere centre
/// path) is tested against the inflated shape via ray-vs-capsule.
///
/// As with the sphere and AABB sweeps, a sweep that starts already overlapping
/// reports `t = 0`, [`ShapeHit::started_inside`], and the normal pointing out
/// of the capsule along its nearest axis point — not out of its centre, which
/// would push a character along a tall capsule instead of away from it.
#[must_use]
pub fn swept_sphere_vs_capsule(
    segment: &crate::broadphase::Segment,
    swept_radius: f64,
    capsule: &Capsule,
) -> Option<ShapeHit> {
    let combined = swept_radius + capsule.radius;

    // Already overlapping at the start of the sweep: contact is now.
    let axis_point = closest_point_on_capsule_axis(capsule, segment.start);
    let radial = segment.start - axis_point;
    let dist_sq = radial.length_squared();
    if dist_sq <= combined * combined {
        let dist = dist_sq.sqrt();
        let normal = if dist > 0.0 { radial / dist } else { DVec3::Y };
        return Some(ShapeHit {
            t: 0.0,
            point: axis_point + normal * capsule.radius,
            normal,
            started_inside: true,
        });
    }

    let dir = segment.end - segment.start;
    if dir.length_squared() <= 0.0 {
        return None; // Stationary and not overlapping.
    }

    // Inflate the capsule by the swept sphere radius.  The segment (sphere
    // centre path) vs the inflated capsule is a ray-vs-capsule test.
    let inflated = Capsule::new(capsule.centre, combined, capsule.half_height);
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
// Swept capsule
// ---------------------------------------------------------------------------

// The swept-capsule tests below are the swept-*sphere* tests against a target
// that has already absorbed the capsule's axis.
//
// Sweeping shape `A` against a static `B` asks whether `A`'s origin path enters
// the Minkowski sum `B ⊕ (−A)`. A capsule is `axis ⊕ ball(r)` with the axis
// symmetric about the capsule centre, so `−A = A` and the sum is
// `(B ⊕ axis) ⊕ ball(r)`: a *ball* of the capsule's radius following the capsule
// centre, against a target grown along Y by the capsule's half-height. Growing
// along Y is closed over the shapes this crate has — a sphere becomes a capsule,
// a capsule a taller capsule, an AABB a taller AABB — which is what makes the
// reduction exact rather than an approximation, and it is exact only because
// every capsule here is Y-aligned.
//
// The time of impact, the contact normal and `started_inside` therefore come
// straight back from the swept-sphere call. Only [`ShapeHit::point`] has to be
// undone: the sphere test reports a point on the *grown* target, and each
// function below maps it back onto the real one.

/// Compute the TOI for a Y-aligned capsule moving along a segment against a
/// static sphere. `segment` is the path of the capsule's **centre**.
///
/// A sphere grown along Y is a capsule, so this is [`swept_sphere_vs_capsule`]
/// against that proxy — see the note above.
///
/// As with the sphere sweeps, a sweep that starts already overlapping reports
/// `t = 0` and [`ShapeHit::started_inside`].
#[must_use]
pub fn swept_capsule_vs_sphere(
    segment: &crate::broadphase::Segment,
    swept_radius: f64,
    swept_half_height: f64,
    target: &Sphere,
) -> Option<ShapeHit> {
    let proxy = Capsule::new(target.centre, target.radius, swept_half_height);
    let hit = swept_sphere_vs_capsule(segment, swept_radius, &proxy)?;
    // The proxy's surface is a capsule's and the contact is on the real sphere,
    // which the normal already points out of.
    Some(ShapeHit {
        point: target.centre + hit.normal * target.radius,
        ..hit
    })
}

/// Compute the TOI for a Y-aligned capsule moving along a segment against a
/// static AABB. `segment` is the path of the capsule's **centre**.
///
/// An AABB grown along Y is an AABB, so this is [`swept_sphere_vs_aabb`]
/// against that proxy — see the note above. It inherits that function's
/// treatment of edges and corners: the box is inflated by the radius rather
/// than rounded, so a hit arriving diagonally at a corner reports the corner of
/// the inflated box and its face normal.
///
/// As with the sphere sweeps, a sweep that starts already overlapping reports
/// `t = 0`, [`ShapeHit::started_inside`] and the minimum-penetration face
/// normal.
#[must_use]
pub fn swept_capsule_vs_aabb(
    segment: &crate::broadphase::Segment,
    swept_radius: f64,
    swept_half_height: f64,
    target: &Aabb,
) -> Option<ShapeHit> {
    // Checked before the growth and not after: a box inverted only on Y reads
    // as non-empty once Y has been grown, and would then be swept against as
    // though it were a real box.
    if target.is_empty() {
        return None;
    }
    let grown = Aabb::new(
        target.min - DVec3::Y * swept_half_height,
        target.max + DVec3::Y * swept_half_height,
    );
    let hit = swept_sphere_vs_aabb(segment, swept_radius, &grown)?;
    // The sphere test clamped the contact onto `grown`, so only Y can be off
    // the real box; clamping again puts it back on the face that was grown.
    Some(ShapeHit {
        point: hit.point.clamp(target.min, target.max),
        ..hit
    })
}

/// Compute the TOI for a Y-aligned capsule moving along a segment against a
/// static Y-aligned capsule. `segment` is the path of the moving capsule's
/// **centre**.
///
/// A capsule grown along Y is a capsule with the half-heights summed, so this
/// is [`swept_sphere_vs_capsule`] against that proxy — see the note above.
///
/// As with the sphere sweeps, a sweep that starts already overlapping reports
/// `t = 0` and [`ShapeHit::started_inside`].
#[must_use]
pub fn swept_capsule_vs_capsule(
    segment: &crate::broadphase::Segment,
    swept_radius: f64,
    swept_half_height: f64,
    target: &Capsule,
) -> Option<ShapeHit> {
    let proxy = Capsule::new(
        target.centre,
        target.radius,
        target.half_height + swept_half_height,
    );
    let hit = swept_sphere_vs_capsule(segment, swept_radius, &proxy)?;
    // The proxy is taller than the target, so the axis point the sphere test
    // anchored on is not one of the target's. Re-anchor on the real axis, which
    // the normal points out of.
    let centre_at_hit = segment.start + (segment.end - segment.start) * hit.t;
    let axis_point = closest_point_on_capsule_axis(target, centre_at_hit);
    Some(ShapeHit {
        point: axis_point + hit.normal * target.radius,
        ..hit
    })
}

// ---------------------------------------------------------------------------
// Capsule penetration
// ---------------------------------------------------------------------------

/// How far an overlapping shape has to move, and which way, to stop
/// overlapping.
///
/// This is what a sweep cannot answer. A sweep that starts already overlapping
/// reports the contact at `t = 0` and stops there, because there is no
/// *earlier* time to report; the depth is a different measurement, and a
/// character that woke up inside the world needs it to get out. PhysX exposes
/// the same pair separately (`PxSweep*` against `PxComputePenetration`), and so
/// does Unity (`CapsuleCast` against `Physics.ComputePenetration`).
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Penetration {
    /// Unit direction to translate the overlapping shape along, pointing out of
    /// the shape it is inside.
    pub normal: DVec3,
    /// How far along `normal` the translation has to go. Always positive — a
    /// pair that merely touches is not a penetration and is reported as `None`.
    pub depth: f64,
}

/// The push-out for a Y-aligned capsule overlapping a sphere, or `None` if they
/// are clear of each other.
///
/// The same Y-growth reduction the swept-capsule tests use: a sphere grown
/// along Y by the capsule's half-height is a capsule, and the capsule's centre
/// is inside it exactly when the two shapes overlap.
#[must_use]
pub fn capsule_penetration_vs_sphere(capsule: &Capsule, target: &Sphere) -> Option<Penetration> {
    let grown = Capsule::new(target.centre, target.radius, capsule.half_height);
    penetration_in_capsule(capsule.centre, capsule.radius, &grown)
}

/// The push-out for a Y-aligned capsule overlapping a Y-aligned capsule, or
/// `None` if they are clear of each other.
#[must_use]
pub fn capsule_penetration_vs_capsule(capsule: &Capsule, target: &Capsule) -> Option<Penetration> {
    let grown = Capsule::new(
        target.centre,
        target.radius,
        target.half_height + capsule.half_height,
    );
    penetration_in_capsule(capsule.centre, capsule.radius, &grown)
}

/// The push-out for a Y-aligned capsule overlapping an AABB, or `None` if they
/// are clear of each other.
///
/// A centre *inside* the grown box has no direction to escape along, so the
/// answer there is the minimum-penetration face — the same choice the swept
/// forms make, and the reason a character that spawned inside a wall leaves
/// through the nearest one rather than travelling the length of the room.
#[must_use]
pub fn capsule_penetration_vs_aabb(capsule: &Capsule, target: &Aabb) -> Option<Penetration> {
    // Checked before the growth, for the reason `swept_capsule_vs_aabb` gives.
    if target.is_empty() {
        return None;
    }
    let grown = Aabb::new(
        target.min - DVec3::Y * capsule.half_height,
        target.max + DVec3::Y * capsule.half_height,
    );
    let closest = capsule.centre.clamp(grown.min, grown.max);
    let out = capsule.centre - closest;
    let dist_sq = out.length_squared();
    if dist_sq > capsule.radius * capsule.radius {
        return None;
    }
    let (normal, depth) = if dist_sq > 0.0 {
        let dist = dist_sq.sqrt();
        (out / dist, capsule.radius - dist)
    } else {
        let (normal, to_face) = aabb_escape(&grown, capsule.centre);
        (normal, capsule.radius + to_face)
    };
    (depth > 0.0).then_some(Penetration { normal, depth })
}

/// The push-out for a point of the given radius sitting inside `grown`, which
/// is whichever capsule the caller's Y-growth produced.
///
/// A point exactly on the axis has no direction to escape along and takes the
/// same `+Y` fallback the swept forms use for the same degenerate case.
fn penetration_in_capsule(point: DVec3, radius: f64, grown: &Capsule) -> Option<Penetration> {
    let combined = radius + grown.radius;
    let out = point - closest_point_on_capsule_axis(grown, point);
    let dist_sq = out.length_squared();
    if dist_sq > combined * combined {
        return None;
    }
    let dist = dist_sq.sqrt();
    let depth = combined - dist;
    if depth <= 0.0 {
        return None;
    }
    Some(Penetration {
        normal: if dist > 0.0 { out / dist } else { DVec3::Y },
        depth,
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
    fn a_ray_passing_beside_a_sphere_reports_no_hit() {
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

    #[test]
    fn zero_radius_sphere_normal_is_finite_when_hit_at_centre() {
        // Bypass Sphere::new's debug_assert to test the query directly
        // with a degenerate radius=0 sphere.  Before the guard,
        // `.normalize()` on the zero-length centre-to-point vector
        // produced NaN.
        let sphere = Sphere {
            centre: DVec3::ZERO,
            radius: 0.0,
        };
        let ray = Ray::new(DVec3::new(-5.0, 0.0, 0.0), DVec3::X);
        let hit =
            ray_vs_sphere(&ray, &sphere).expect("ray through centre must hit zero-radius sphere");
        assert!(
            !hit.normal.x.is_nan() && !hit.normal.y.is_nan() && !hit.normal.z.is_nan(),
            "normal must be finite for zero-radius sphere hit at centre; got {:?}",
            hit.normal
        );
    }

    #[test]
    fn ray_hits_sphere_it_already_entered_before_t_min() {
        // Near root at t=4, far at t=6, but the caller advanced t_min past the
        // near root.  The far root is still in bounds, so this is a hit.
        let ray = Ray::new(DVec3::new(-5.0, 0.0, 0.0), DVec3::X).with_bounds(4.5, 100.0);
        let sphere = Sphere::new(DVec3::ZERO, 1.0);
        let hit = ray_vs_sphere(&ray, &sphere).expect("far root is within bounds");
        assert!((hit.t - 6.0).abs() < 1e-9, "t = {}", hit.t);
        assert!(hit.started_inside);
    }

    #[test]
    fn ray_misses_sphere_when_both_roots_are_out_of_bounds() {
        let ray = Ray::new(DVec3::new(-5.0, 0.0, 0.0), DVec3::X).with_bounds(6.5, 100.0);
        let sphere = Sphere::new(DVec3::ZERO, 1.0);
        assert!(ray_vs_sphere(&ray, &sphere).is_none());
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
    fn a_ray_passing_above_a_box_reports_no_hit() {
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

    #[test]
    fn ray_hits_aabb_it_already_entered_before_t_min() {
        let ray = Ray::new(DVec3::new(-5.0, 0.0, 0.0), DVec3::X).with_bounds(4.5, 100.0);
        let aabb = Aabb::from_centre_half(DVec3::ZERO, DVec3::splat(1.0));
        let hit = ray_vs_aabb(&ray, &aabb).expect("far face is within bounds");
        assert!((hit.t - 6.0).abs() < 1e-9, "t = {}", hit.t);
        assert!(hit.started_inside);
        assert_eq!(hit.normal, DVec3::X);
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
    fn a_ray_passing_beside_a_capsule_reports_no_hit() {
        let capsule = Capsule::new(DVec3::ZERO, 0.5, 1.0);
        let ray = Ray::new(DVec3::new(-5.0, 3.0, 0.0), DVec3::X);
        assert!(ray_vs_capsule(&ray, &capsule).is_none());
    }

    #[test]
    fn ray_inside_capsule_cylinder_radius_hits_the_cap_not_the_far_side() {
        // Origin is radially inside the infinite cylinder (|x| = 0.9 < r = 1)
        // but far above the capsule, aimed almost straight down.  The first
        // real surface is the top hemisphere at t ≈ 4; the cylinder's exit at
        // t ≈ 5 is a backface and must not win.
        let capsule = Capsule::new(DVec3::ZERO, 1.0, 1.0);
        let ray = Ray::new(DVec3::new(0.9, 5.0, 0.0), DVec3::new(0.0001, -1.0, 0.0));
        let hit = ray_vs_capsule(&ray, &capsule).expect("ray must hit the capsule");
        assert!(
            hit.t < 4.5,
            "expected the top cap at t ≈ 4, got t = {} at {:?}",
            hit.t,
            hit.point
        );
        assert!(
            hit.point.y > 1.0,
            "hit must be on the top cap: {:?}",
            hit.point
        );
        assert!(
            hit.normal.y > 0.0,
            "top-cap normal must point up: {:?}",
            hit.normal
        );
        assert!(!hit.started_inside);
    }

    #[test]
    fn ray_started_inside_capsule_is_reported() {
        let capsule = Capsule::new(DVec3::ZERO, 1.0, 2.0);
        let ray = Ray::new(DVec3::ZERO, DVec3::X);
        let hit = ray_vs_capsule(&ray, &capsule).expect("ray from the axis must exit the capsule");
        assert!(hit.started_inside);
        assert!((hit.t - 1.0).abs() < 1e-9, "t = {}", hit.t);
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
        let hit = swept_sphere_vs_sphere(&seg, 0.5, &target).expect("overlap at t=0 is a contact");
        // Contact is now, not when the sphere would leave the target.
        assert_eq!(hit.t, 0.0);
        assert!(hit.started_inside);
        assert!((hit.normal - DVec3::X).length() < 1e-9, "{:?}", hit.normal);
    }

    #[test]
    fn swept_sphere_overlapping_for_the_whole_sweep_still_contacts() {
        // Deep overlap: both roots lie outside [0, 1] (t1 < 0 < 1 < t2), which
        // is a resting contact, not a miss.
        let seg = crate::broadphase::Segment::new(DVec3::ZERO, DVec3::new(0.1, 0.0, 0.0));
        let target = Sphere::new(DVec3::ZERO, 5.0);
        let hit = swept_sphere_vs_sphere(&seg, 0.5, &target).expect("resting contact");
        assert_eq!(hit.t, 0.0);
        assert!(hit.started_inside);
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
        let hit = swept_sphere_vs_aabb(&seg, 0.5, &aabb).expect("overlap at t=0 is a contact");
        assert_eq!(hit.t, 0.0);
        assert!(hit.started_inside);
        // Nearest inflated face from (0.5, 0, 0) is +X (0.5 vs 1.5 elsewhere),
        // not the fabricated +Z the eps-chain used to fall through to.
        assert_eq!(hit.normal, DVec3::X);
    }

    #[test]
    fn tiny_sweep_starting_inside_sphere_is_a_contact() {
        // A sweep of length ~1e-10 m (dir·dir = 1e-20) is a real, non-stationary
        // sweep but sits below `solve_quadratic`'s EPSILON floor. It must take
        // the stationary branch and report a contact, not be rejected as a miss.
        let seg = crate::broadphase::Segment::new(DVec3::ZERO, DVec3::new(1e-10, 0.0, 0.0));
        let target = Sphere::new(DVec3::ZERO, 1.0);
        let hit = swept_sphere_vs_sphere(&seg, 0.5, &target)
            .expect("a sweep inside the target is a contact, not a miss");
        assert_eq!(hit.t, 0.0);
        assert!(hit.started_inside);
    }

    #[test]
    fn tiny_sweep_starting_inside_aabb_is_a_contact() {
        let seg = crate::broadphase::Segment::new(DVec3::ZERO, DVec3::new(1e-10, 0.0, 0.0));
        let aabb = Aabb::from_centre_half(DVec3::ZERO, DVec3::splat(1.0));
        let hit = swept_sphere_vs_aabb(&seg, 0.5, &aabb)
            .expect("a sweep inside the box is a contact, not a miss");
        assert_eq!(hit.t, 0.0);
        assert!(hit.started_inside);
    }

    #[test]
    fn empty_aabb_overlaps_nothing() {
        let sphere = Sphere::new(DVec3::ZERO, 1.0);
        assert!(!sphere_overlaps_aabb(&sphere, &Aabb::EMPTY));
    }

    #[test]
    fn swept_sphere_vs_empty_aabb_is_a_miss() {
        let seg = crate::broadphase::Segment::new(DVec3::ZERO, DVec3::ZERO);
        assert!(swept_sphere_vs_aabb(&seg, 1.0, &Aabb::EMPTY).is_none());
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
    fn swept_sphere_overlapping_tall_capsule_pushes_out_sideways() {
        // A sphere overlapping the side of a tall capsule must be pushed along
        // the shortest way out — laterally — not away from the capsule centre.
        let capsule = Capsule::new(DVec3::ZERO, 1.0, 5.0);
        let seg =
            crate::broadphase::Segment::new(DVec3::new(1.2, 5.0, 0.0), DVec3::new(1.2, 5.0, 0.0));
        let hit = swept_sphere_vs_capsule(&seg, 0.5, &capsule).expect("resting contact");
        assert_eq!(hit.t, 0.0);
        assert!(hit.started_inside);
        assert!(
            (hit.normal - DVec3::X).length() < 1e-9,
            "expected a lateral normal, got {:?}",
            hit.normal
        );
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

    // ── Swept capsule ──────────────────────────────────────────────────

    /// A capsule sweep is the query a character needs and a sphere sweep is
    /// not: the curb is below the sphere and inside the capsule.
    #[test]
    fn a_capsule_sweep_strikes_a_curb_the_sphere_at_its_waist_rides_over() {
        // Feet on y = 0, so the capsule spans y ∈ [0, 1.8] and the sphere of
        // the same radius, at the same centre, spans y ∈ [0.5, 1.3].
        let (radius, half_height) = (0.4, 0.5);
        let centre_y = radius + half_height;
        let seg = crate::broadphase::Segment::new(
            DVec3::new(-3.0, centre_y, 0.0),
            DVec3::new(3.0, centre_y, 0.0),
        );
        let curb = Aabb::new(DVec3::new(1.0, 0.0, -2.0), DVec3::new(3.0, 0.3, 2.0));

        assert!(
            swept_sphere_vs_aabb(&seg, radius, &curb).is_none(),
            "the sphere must miss the curb, or the capsule hitting it proves nothing",
        );
        let hit = swept_capsule_vs_aabb(&seg, radius, half_height, &curb)
            .expect("the capsule reaches down to the curb");
        assert_eq!(hit.normal, DVec3::NEG_X, "struck the curb's -X face");
    }

    /// The contact is reported on the real box, not on the Y-grown proxy the
    /// reduction sweeps against — a controller placing a foot from this point
    /// would otherwise stand `half_height` above the floor.
    #[test]
    fn a_capsule_landing_reports_the_contact_on_the_floor_not_the_grown_box() {
        let (radius, half_height) = (0.4, 0.5);
        let floor = Aabb::new(DVec3::new(-5.0, -1.0, -5.0), DVec3::new(5.0, 0.0, 5.0));
        let seg =
            crate::broadphase::Segment::new(DVec3::new(0.0, 3.0, 0.0), DVec3::new(0.0, 0.5, 0.0));
        let hit = swept_capsule_vs_aabb(&seg, radius, half_height, &floor).expect("lands");
        assert_eq!(hit.normal, DVec3::Y);
        // Bottom cap touches y = 0 when the centre is at radius + half_height.
        let touch_y = radius + half_height;
        assert!(
            (hit.t - (3.0 - touch_y) / 2.5).abs() < 1e-12,
            "expected the TOI where the bottom cap meets the floor, got t = {}",
            hit.t,
        );
        assert!(
            (hit.point.y - floor.max.y).abs() < 1e-12,
            "contact reported at y = {}, and the floor's top is y = {}",
            hit.point.y,
            floor.max.y,
        );
    }

    /// A box inverted only on Y reads as non-empty once Y has been grown, so
    /// the emptiness check has to happen before the growth.
    #[test]
    fn a_capsule_sweep_misses_a_box_that_is_empty_only_on_y() {
        let inverted = Aabb::new(DVec3::new(-1.0, 1.0, -1.0), DVec3::new(1.0, -1.0, 1.0));
        assert!(inverted.is_empty());
        let seg =
            crate::broadphase::Segment::new(DVec3::new(-5.0, 0.0, 0.0), DVec3::new(5.0, 0.0, 0.0));
        assert!(swept_capsule_vs_aabb(&seg, 0.5, 2.0, &inverted).is_none());
    }

    /// The capsule's flank reaches a sphere its centre line passes well below,
    /// and the contact comes back on the sphere's own surface.
    #[test]
    fn a_capsule_flank_hits_a_sphere_above_its_centre_line() {
        let (radius, half_height) = (0.5, 2.0);
        let target = Sphere::new(DVec3::ZERO, 1.0);
        // The path is 1.6 above the target's centre: further than the 1.5 the
        // two radii cover, so the sphere sweep misses, and well inside the
        // capsule's 2.0 half-height, so the flank does not.
        let seg =
            crate::broadphase::Segment::new(DVec3::new(-5.0, 1.6, 0.0), DVec3::new(5.0, 1.6, 0.0));
        assert!(
            swept_sphere_vs_sphere(&seg, radius, &target).is_none(),
            "the sphere must miss, or the capsule hitting it proves nothing",
        );

        let hit = swept_capsule_vs_sphere(&seg, radius, half_height, &target).expect("flank hit");
        // Contact at |x| = radius + target.radius along a 10 m path from -5.
        assert!(
            (hit.t - (5.0 - 1.5) / 10.0).abs() < 1e-12,
            "expected the TOI where the flank meets the sphere, got t = {}",
            hit.t,
        );
        assert!(
            (hit.normal - DVec3::NEG_X).length() < 1e-12,
            "got {:?}",
            hit.normal
        );
        assert!(
            (hit.point - DVec3::new(-1.0, 0.0, 0.0)).length() < 1e-12,
            "the contact belongs on the target sphere's surface, and came back at {:?}",
            hit.point,
        );
    }

    /// Two Y-aligned capsules meet when the horizontal gap closes to the sum of
    /// their radii, and the contact lands on the static one's surface rather
    /// than on the taller proxy the reduction sweeps against.
    #[test]
    fn a_capsule_sweep_re_anchors_its_contact_on_the_static_capsules_axis() {
        let (radius, half_height) = (0.5, 3.0);
        let target = Capsule::new(DVec3::ZERO, 0.5, 1.0);
        // The mover's centre rides above the target's top cap; only the flanks
        // can meet, and the overlap of the two axes tops out at y = 1.0.
        let seg =
            crate::broadphase::Segment::new(DVec3::new(-5.0, 2.0, 0.0), DVec3::new(5.0, 2.0, 0.0));
        let hit =
            swept_capsule_vs_capsule(&seg, radius, half_height, &target).expect("flanks meet");
        assert!(
            (hit.t - (5.0 - 1.0) / 10.0).abs() < 1e-12,
            "expected the TOI where the two flanks touch, got t = {}",
            hit.t,
        );
        assert!(
            (hit.normal - DVec3::NEG_X).length() < 1e-12,
            "got {:?}",
            hit.normal
        );
        let off_axis = hit.point - closest_point_on_capsule_axis(&target, hit.point);
        assert!(
            (off_axis.length() - target.radius).abs() < 1e-12,
            "the contact is {} from the target's axis and its surface is {} out — \
             the proxy's axis is taller than the target's, so this is what an \
             un-re-anchored point fails",
            off_axis.length(),
            target.radius,
        );
    }

    /// A capsule that starts overlapping is a resting contact at `t = 0`, the
    /// same answer the sphere sweeps give and the one depenetration reads.
    #[test]
    fn a_capsule_sweep_that_starts_overlapping_is_a_contact_at_zero() {
        let (radius, half_height) = (0.4, 0.9);
        let wall = Aabb::new(DVec3::new(0.0, -5.0, -5.0), DVec3::new(5.0, 5.0, 5.0));
        // Centre 0.2 inside the wall's -X face, so the whole flank overlaps.
        let seg =
            crate::broadphase::Segment::new(DVec3::new(0.2, 0.0, 0.0), DVec3::new(0.2, 0.0, 1.0));
        let hit = swept_capsule_vs_aabb(&seg, radius, half_height, &wall).expect("resting contact");
        assert_eq!(hit.t, 0.0);
        assert!(hit.started_inside);
        assert_eq!(
            hit.normal,
            DVec3::NEG_X,
            "the shortest way out is back through the face it entered",
        );
    }
}
