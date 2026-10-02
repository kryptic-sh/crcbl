//! Queries against a [`BoxCollider`], turned or not.
//!
//! An unturned box is answered by the world-axis functions of the parent
//! module against [`BoxCollider::aabb`], exactly as before boxes could turn,
//! so its answers are the same to the bit. A turned box is answered in its own
//! frame, where it is a world-axis box at the origin:
//!
//! - **A ray, a swept sphere and a sphere overlap** are moved into the box's
//!   frame — the point relative to the centre, turned back by the rotation —
//!   tested against the box there, and the hit's point and normal turned back
//!   out. A sphere is the same in any frame and a rotation keeps lengths, so
//!   the hit's `t` needs no change. This is the local-frame test of Ericson,
//!   _Real-Time Collision Detection_ §5.3.3, for a ray against an OBB.
//! - **An upright capsule** stops being upright in a turned box's frame, so
//!   the world-axis capsule functions' Y-growth does not apply. Its push-out
//!   is the contact pipeline's signed distance between the capsule's core and
//!   the box ([`gap`]), and its sweep is that pipeline's conservative
//!   advancement over the same distance ([`time_of_contact`]) — the method
//!   the query world's lying capsule is swept by — which stops a little short
//!   of the contact, within the advancement's tolerance, rather than on it.

use glam::DVec3;

use super::{
    Penetration, ShapeHit, capsule_penetration_vs_aabb, ray_vs_aabb, sphere_overlaps_aabb,
    swept_capsule_vs_aabb, swept_sphere_vs_aabb,
};
use crate::broadphase::{Ray, Segment};
use crate::collider::{BoxCollider, Capsule, Sphere};
use crate::contact::manifold::{closest_on_segment_to_box, gap};
use crate::contact::shape::ContactShape;
use crate::contact::sweep::time_of_contact;

/// Test whether a sphere overlaps a box collider, turned or not (touching
/// counts as overlapping).
#[must_use]
pub fn sphere_overlaps_box(sphere: &Sphere, target: &BoxCollider) -> bool {
    if !target.is_turned() {
        return sphere_overlaps_aabb(sphere, &target.aabb());
    }
    let local = Sphere::new(target.local_point(sphere.centre), sphere.radius);
    sphere_overlaps_aabb(&local, &target.local_aabb())
}

/// Intersect a ray with a box collider, turned or not, treated as solid: the
/// nearest hit within the ray's bounds, or `None`.
///
/// As [`ray_vs_aabb`], a ray whose accepted interval starts inside the box
/// hits the far face and reports [`ShapeHit::started_inside`]; a turned box's
/// normal is its own face's, turned with it.
#[must_use]
pub fn ray_vs_box(ray: &Ray, target: &BoxCollider) -> Option<ShapeHit> {
    if !target.is_turned() {
        return ray_vs_aabb(ray, &target.aabb());
    }
    let local = Ray {
        origin: target.local_point(ray.origin),
        dir: target.rotation.inverse() * ray.dir,
        ..*ray
    };
    let hit = ray_vs_aabb(&local, &target.local_aabb())?;
    Some(ShapeHit {
        point: ray.origin + ray.dir * hit.t,
        normal: target.rotation * hit.normal,
        ..hit
    })
}

/// The time of impact of a sphere moving along `segment` against a static box
/// collider, turned or not — see [`swept_sphere_vs_aabb`], whose answers a
/// turned box gives in its own frame.
#[must_use]
pub fn swept_sphere_vs_box(
    segment: &Segment,
    swept_radius: f64,
    target: &BoxCollider,
) -> Option<ShapeHit> {
    if !target.is_turned() {
        return swept_sphere_vs_aabb(segment, swept_radius, &target.aabb());
    }
    let local = Segment::new(
        target.local_point(segment.start),
        target.local_point(segment.end),
    );
    let hit = swept_sphere_vs_aabb(&local, swept_radius, &target.local_aabb())?;
    Some(ShapeHit {
        point: target.world_point(hit.point),
        normal: target.rotation * hit.normal,
        ..hit
    })
}

/// The time of impact of a Y-aligned capsule whose centre moves along
/// `segment` against a static box collider, turned or not.
///
/// An unturned box is [`swept_capsule_vs_aabb`]. A turned one is swept by
/// conservative advancement over the contact pipeline's signed distance from
/// the box to the capsule's core, stopping a little short of the contact; a
/// capsule that begins
/// touching or inside the box meets it at `t = 0` with
/// [`ShapeHit::started_inside`], whichever way it is moving, and one that does
/// not move meets nothing it is clear of.
#[must_use]
pub fn swept_capsule_vs_box(
    segment: &Segment,
    swept_radius: f64,
    swept_half_height: f64,
    target: &BoxCollider,
) -> Option<ShapeHit> {
    if !target.is_turned() {
        return swept_capsule_vs_aabb(segment, swept_radius, swept_half_height, &target.aabb());
    }
    let obstacle = contact_box(target);
    let motion = segment.end - segment.start;
    let up = DVec3::Y * swept_half_height;
    let at = |t: f64| {
        let centre = segment.start + motion * t;
        ContactShape::Capsule {
            a: centre - up,
            b: centre + up,
            radius: swept_radius,
        }
    };
    let start = gap(&obstacle, &at(0.0));
    let (t, normal, started_inside) = if start.0 <= 0.0 {
        (0.0, start.1, true)
    } else {
        let t = time_of_contact(&obstacle, at, motion, start)?;
        (t, gap(&obstacle, &at(t)).1, false)
    };
    let centre = segment.start + motion * t;
    Some(ShapeHit {
        t,
        point: nearest_on_box(target, centre - up, centre + up),
        normal,
        started_inside,
        part: 0,
    })
}

/// The push-out for a Y-aligned capsule overlapping a box collider, turned or
/// not, or `None` if they are clear of each other or only touch.
///
/// An unturned box is [`capsule_penetration_vs_aabb`]. A turned one is the
/// contact pipeline's signed distance from the box to the capsule, whose
/// normal points out of the box towards the capsule: exact while the capsule's
/// core is outside the box, and for a core that has entered it the depth of
/// the core point nearest the box's surface.
#[must_use]
pub fn capsule_penetration_vs_box(capsule: &Capsule, target: &BoxCollider) -> Option<Penetration> {
    if !target.is_turned() {
        return capsule_penetration_vs_aabb(capsule, &target.aabb());
    }
    let core = ContactShape::Capsule {
        a: capsule.bottom(),
        b: capsule.top(),
        radius: capsule.radius,
    };
    let (distance, normal) = gap(&contact_box(target), &core);
    (distance < 0.0).then_some(Penetration {
        normal,
        depth: -distance,
    })
}

/// `target` as the contact pipeline places a box.
pub(crate) fn contact_box(target: &BoxCollider) -> ContactShape {
    ContactShape::Box {
        centre: target.centre,
        rotation: target.rotation,
        half: target.half_extents,
    }
}

/// The point of `target` nearest the segment `a`–`b`: the segment's nearest
/// point clamped into the box, in the box's own frame.
fn nearest_on_box(target: &BoxCollider, a: DVec3, b: DVec3) -> DVec3 {
    let p0 = target.local_point(a);
    let along = target.local_point(b) - p0;
    let t = closest_on_segment_to_box(p0, along, target.half_extents);
    let half = target.half_extents;
    target.world_point((p0 + along * t).clamp(-half, half))
}

#[cfg(test)]
mod tests;
