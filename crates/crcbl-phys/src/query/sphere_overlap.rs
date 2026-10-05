//! A sphere's overlap with one shape, measured: how deep, which way out, and
//! where.
//!
//! [`OverlapHit`] is what the overlap queries answer per collider. Every
//! function here is the push-out the capsule queries already compute
//! ([`Penetration`]), asked for a capsule of no height — which is a sphere —
//! with the contact point added.
//!
//! A turned box and a turned capsule are measured in their own frames, as
//! [`super::sphere_overlaps_box`] tests one, by the forms beside it in
//! [`super::boxes`] and [`super::capsules`]: a sphere is the same in any frame
//! and a rotation keeps lengths, so only the point and the normal are turned
//! back out. A mesh is measured by its own capsule push-out, against the
//! triangle the sphere is deepest in.
//!
//! # A hit is a positive depth, exactly
//!
//! A sphere that only touches a shape is not inside it, and is not a hit: the
//! depth has to be strictly positive in `f64`, with no tolerance either way,
//! the rule [`Penetration`] keeps. Two shapes apart by less than rounding can
//! therefore read either side of the line; nothing here pads it.
//!
//! # The normal where there is no direction to escape along
//!
//! A centre exactly on a sphere's centre, or on a capsule's core, is equally
//! far from every point of the surface. The normal there is `+Y` — the shape's
//! own `+Y` for a turned capsule, its core — which is the fallback the sweeps
//! and the capsule push-out already take for the same case. A centre inside a
//! box leaves through the nearest face, and of faces equally near through the
//! first of `-X`, `+X`, `-Y`, `+Y`, `-Z`, `+Z` in the box's own frame, which is
//! the minimum-penetration rule the capsule push-out uses.
//!
//! "Exactly" is as the arithmetic has it. A centre that is on a capsule's core
//! in exact numbers can land a rounding off it in `f64` — the nearest core
//! point is worked out along the core, and need not come back to the bit —
//! and then leaves along that rounding's direction: deterministic, the same
//! on every run and target, but no more meaningful than which way it rounded.
//! A centre on a mesh's triangle leaves along the triangle's own normal, the
//! side its winding faces.

use glam::DVec3;

use super::{
    Penetration, capsule_penetration_vs_aabb, capsule_penetration_vs_capsule,
    capsule_penetration_vs_sphere,
};
use crate::collider::{Aabb, Capsule, Sphere};

/// Where, how deep and which way a query sphere overlaps one collider.
///
/// The overlap queries' answer per collider —
/// [`PhysicsWorld::overlap_sphere`](crate::PhysicsWorld::overlap_sphere) and
/// every form of it. Not a [`ShapeHit`](super::ShapeHit): an overlap has no
/// time of impact, so a ray parameter would be a field with no meaning, and
/// the depth a sweep cannot give is the measurement an overlap exists for.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct OverlapHit {
    /// The point of the collider's surface the query sphere is pushed out
    /// from: moved `depth` along `normal`, the sphere touches the collider
    /// here.
    pub point: DVec3,
    /// Unit direction to move the query sphere along to leave the collider,
    /// pointing out of the collider.
    pub normal: DVec3,
    /// How far along `normal` the sphere has to move. Always positive — see
    /// the module docs.
    pub depth: f64,
    /// Which part of the collider the sphere is deepest in: for a compound
    /// ([`crate::PhysicsWorld::add_compound`]) the index of the part in its
    /// [`CompoundShape::parts`](crate::CompoundShape::parts), and `0` for
    /// every other collider and from every shape-level function here.
    pub part: usize,
}

impl OverlapHit {
    /// The hit for `sphere` pushed out by `penetration`: the surface point is
    /// where the sphere's deepest point lands once it has moved out, which for
    /// a centre outside the shape is the shape's point nearest the centre.
    pub(crate) fn from_penetration(sphere: &Sphere, penetration: Penetration) -> Self {
        let Penetration { normal, depth } = penetration;
        Self {
            point: sphere.centre - normal * (sphere.radius - depth),
            normal,
            depth,
            part: 0,
        }
    }
}

/// `sphere` as the capsule queries take one: a capsule of no height.
fn as_capsule(sphere: &Sphere) -> Capsule {
    Capsule::new(sphere.centre, sphere.radius, 0.0)
}

/// How `sphere` overlaps another sphere, or `None` if they are apart or only
/// touch.
#[must_use]
pub fn sphere_overlap_vs_sphere(sphere: &Sphere, target: &Sphere) -> Option<OverlapHit> {
    let penetration = capsule_penetration_vs_sphere(&as_capsule(sphere), target)?;
    Some(OverlapHit::from_penetration(sphere, penetration))
}

/// How `sphere` overlaps an AABB, or `None` if they are apart or only touch.
#[must_use]
pub fn sphere_overlap_vs_aabb(sphere: &Sphere, target: &Aabb) -> Option<OverlapHit> {
    let penetration = capsule_penetration_vs_aabb(&as_capsule(sphere), target)?;
    Some(OverlapHit::from_penetration(sphere, penetration))
}

/// How `sphere` overlaps a Y-aligned capsule, or `None` if they are apart or
/// only touch.
#[must_use]
pub fn sphere_overlap_vs_capsule(sphere: &Sphere, target: &Capsule) -> Option<OverlapHit> {
    let penetration = capsule_penetration_vs_capsule(&as_capsule(sphere), target)?;
    Some(OverlapHit::from_penetration(sphere, penetration))
}

#[cfg(test)]
mod tests;
