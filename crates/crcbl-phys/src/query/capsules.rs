//! Queries against a capsule in the query world, standing or turned.
//!
//! [`Capsule`] stands up the world's `Y`, and the query world's own capsule —
//! a character's — stays that way. A body's capsule turns with the body, as
//! the contact pipeline places it ([`ContactShape::placed`]), so the query
//! world holds it as a [`TurnedCapsule`]: the capsule and the rotation that
//! turns it about its centre.
//!
//! A capsule whose turned axis has no horizontal part still stands, and is
//! answered by the parent module's upright functions exactly as before
//! capsules could turn, to the bit: a rotation about `Y` alone, such as a
//! character's facing, leaves it standing. A turned one is answered as
//! [`super::boxes`] answers a turned box:
//!
//! - **A ray, a swept sphere and a sphere overlap** are moved into the
//!   capsule's frame — the point relative to the centre, turned back by the
//!   rotation — tested against the upright capsule there, and the hit's point
//!   and normal turned back out. A rotation keeps lengths, so `t` needs no
//!   change.
//! - **An upright capsule** pushed out of or swept against a turned one is the
//!   contact pipeline's distance between the two cores, less the radii
//!   ([`gap`]): exact for the push-out, and for the sweep the conservative
//!   advancement over that distance ([`time_of_contact`]) that a turned box is
//!   swept by, which stops a little short of the contact rather than on it.
//!
//! [`ContactShape::placed`]: crate::contact::shape::ContactShape::placed

use glam::{DQuat, DVec3};

use super::{
    Penetration, ShapeHit, capsule_penetration_vs_capsule, ray_vs_capsule, sphere_overlaps_capsule,
    swept_capsule_vs_capsule, swept_sphere_vs_capsule,
};
use crate::broadphase::{Ray, Segment};
use crate::collider::{Aabb, Capsule, Sphere};
use crate::contact::manifold::{closest_between_segments, gap};
use crate::contact::shape::ContactShape;
use crate::contact::sweep::time_of_contact;

/// A capsule turned about its centre: its core runs along `rotation * Y`.
#[derive(Debug, Clone, Copy, PartialEq)]
pub(crate) struct TurnedCapsule {
    /// The capsule before the turn: its centre, radius and half-height.
    pub(crate) capsule: Capsule,
    /// The unit quaternion turning it about its centre.
    pub(crate) rotation: DQuat,
}

impl TurnedCapsule {
    /// `capsule` standing up the world's `Y`.
    pub(crate) const fn upright(capsule: Capsule) -> Self {
        Self {
            capsule,
            rotation: DQuat::IDENTITY,
        }
    }

    /// `capsule` turned by `rotation` about its centre.
    pub(crate) const fn new(capsule: Capsule, rotation: DQuat) -> Self {
        Self { capsule, rotation }
    }

    /// Whether the turn leaves the core along the world's `Y` — no turn at
    /// all, a turn about `Y` alone, or a half turn end over end — so the
    /// upright functions answer for it.
    fn stands(&self) -> bool {
        let axis = self.rotation * DVec3::Y;
        axis.x == 0.0 && axis.z == 0.0
    }

    /// The core's ends, as the contact pipeline places them: the centre less
    /// and plus the half-height turned.
    fn core(&self) -> (DVec3, DVec3) {
        if self.stands() {
            return (self.capsule.bottom(), self.capsule.top());
        }
        let axis = self.rotation * DVec3::new(0.0, self.capsule.half_height, 0.0);
        (self.capsule.centre - axis, self.capsule.centre + axis)
    }

    /// The smallest world-axis box holding the capsule: its core's ends,
    /// grown by the radius.
    pub(crate) fn aabb(&self) -> Aabb {
        if self.stands() {
            return self.capsule.aabb();
        }
        let (a, b) = self.core();
        let radius = DVec3::splat(self.capsule.radius);
        Aabb::new(a.min(b) - radius, a.max(b) + radius)
    }

    /// The capsule as the contact pipeline holds one.
    pub(crate) fn contact_shape(&self) -> ContactShape {
        let (a, b) = self.core();
        ContactShape::Capsule {
            a,
            b,
            radius: self.capsule.radius,
        }
    }

    /// `point` in the capsule's frame: relative to the centre, turned back by
    /// the rotation, and put back at the centre, where the capsule stands.
    fn local_point(&self, point: DVec3) -> DVec3 {
        self.capsule.centre + self.rotation.inverse() * (point - self.capsule.centre)
    }

    /// A point of the capsule's frame back in the world.
    fn world_point(&self, point: DVec3) -> DVec3 {
        self.capsule.centre + self.rotation * (point - self.capsule.centre)
    }
}

/// Test whether a sphere overlaps a capsule, standing or turned (touching
/// counts as overlapping).
pub(crate) fn sphere_overlaps_turned_capsule(sphere: &Sphere, target: &TurnedCapsule) -> bool {
    if target.stands() {
        return sphere_overlaps_capsule(sphere, &target.capsule);
    }
    let local = Sphere::new(target.local_point(sphere.centre), sphere.radius);
    sphere_overlaps_capsule(&local, &target.capsule)
}

/// Intersect a ray with a capsule, standing or turned: [`ray_vs_capsule`],
/// whose answers a turned capsule gives in its own frame.
pub(crate) fn ray_vs_turned_capsule(ray: &Ray, target: &TurnedCapsule) -> Option<ShapeHit> {
    if target.stands() {
        return ray_vs_capsule(ray, &target.capsule);
    }
    let local = Ray {
        origin: target.local_point(ray.origin),
        dir: target.rotation.inverse() * ray.dir,
        ..*ray
    };
    let hit = ray_vs_capsule(&local, &target.capsule)?;
    Some(ShapeHit {
        point: ray.origin + ray.dir * hit.t,
        normal: target.rotation * hit.normal,
        ..hit
    })
}

/// The time of impact of a sphere moving along `segment` against a static
/// capsule, standing or turned: [`swept_sphere_vs_capsule`], whose answers a
/// turned capsule gives in its own frame.
pub(crate) fn swept_sphere_vs_turned_capsule(
    segment: &Segment,
    swept_radius: f64,
    target: &TurnedCapsule,
) -> Option<ShapeHit> {
    if target.stands() {
        return swept_sphere_vs_capsule(segment, swept_radius, &target.capsule);
    }
    let local = Segment::new(
        target.local_point(segment.start),
        target.local_point(segment.end),
    );
    let hit = swept_sphere_vs_capsule(&local, swept_radius, &target.capsule)?;
    Some(ShapeHit {
        point: target.world_point(hit.point),
        normal: target.rotation * hit.normal,
        ..hit
    })
}

/// The time of impact of a Y-aligned capsule whose centre moves along
/// `segment` against a static capsule, standing or turned.
///
/// A standing one is [`swept_capsule_vs_capsule`]. A turned one is swept by
/// conservative advancement over the distance between the two cores, less
/// the radii, stopping a little short of the contact; a capsule that begins
/// touching or inside it meets it at `t = 0` with
/// [`ShapeHit::started_inside`], whichever way it is moving, and one that
/// does not move meets nothing it is clear of.
pub(crate) fn swept_capsule_vs_turned_capsule(
    segment: &Segment,
    swept_radius: f64,
    swept_half_height: f64,
    target: &TurnedCapsule,
) -> Option<ShapeHit> {
    if target.stands() {
        return swept_capsule_vs_capsule(segment, swept_radius, swept_half_height, &target.capsule);
    }
    let (t, normal, started_inside) = advance_upright_capsule(
        &target.contact_shape(),
        segment,
        swept_radius,
        swept_half_height,
    )?;
    // The contact is on the target's surface, along the normal out of it from
    // the point of its core nearest the swept capsule's.
    let centre = segment.start + (segment.end - segment.start) * t;
    let up = DVec3::Y * swept_half_height;
    let (a, b) = target.core();
    let (core_point, _) = closest_between_segments(a, b, centre - up, centre + up);
    Some(ShapeHit {
        t,
        point: core_point + normal * target.capsule.radius,
        normal,
        started_inside,
        part: 0,
    })
}

/// How far along `segment` a Y-aligned capsule whose centre moves along it
/// first touches `obstacle`, by conservative advancement over the contact
/// pipeline's [`gap`]: the fraction, the normal out of the obstacle there,
/// and whether the capsule began touching or inside it, at `t = 0` whichever
/// way it is moving. A capsule that does not move meets nothing it is clear
/// of. A turned box and a turned capsule are swept by it.
pub(super) fn advance_upright_capsule(
    obstacle: &ContactShape,
    segment: &Segment,
    radius: f64,
    half_height: f64,
) -> Option<(f64, DVec3, bool)> {
    let motion = segment.end - segment.start;
    let up = DVec3::Y * half_height;
    let at = |t: f64| {
        let centre = segment.start + motion * t;
        ContactShape::Capsule {
            a: centre - up,
            b: centre + up,
            radius,
        }
    };
    let start = gap(obstacle, &at(0.0));
    if start.0 <= 0.0 {
        return Some((0.0, start.1, true));
    }
    let t = time_of_contact(obstacle, at, motion, start)?;
    Some((t, gap(obstacle, &at(t)).1, false))
}

/// The push-out for a Y-aligned capsule overlapping a capsule, standing or
/// turned, or `None` if they are clear of each other or only touch.
///
/// A standing one is [`capsule_penetration_vs_capsule`]. A turned one is the
/// distance between the two cores, less the radii, whose normal points out of
/// the target towards the capsule.
pub(crate) fn capsule_penetration_vs_turned_capsule(
    capsule: &Capsule,
    target: &TurnedCapsule,
) -> Option<Penetration> {
    if target.stands() {
        return capsule_penetration_vs_capsule(capsule, &target.capsule);
    }
    let core = ContactShape::Capsule {
        a: capsule.bottom(),
        b: capsule.top(),
        radius: capsule.radius,
    };
    let (distance, normal) = gap(&target.contact_shape(), &core);
    (distance < 0.0).then_some(Penetration {
        normal,
        depth: -distance,
    })
}
