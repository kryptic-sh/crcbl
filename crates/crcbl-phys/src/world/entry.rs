//! What the query world holds for one collider, and the shapes a query's
//! narrow phase tests in it.
//!
//! Every query family asks an entry for its [`primitives`] and tests each,
//! rather than matching on the entry itself, so a compound — a collider of
//! several boxes — is answered part by part by the same code that answers a
//! collider of one shape.
//!
//! [`primitives`]: ColliderEntry::primitives

use glam::DVec3;

use crate::collider::{Aabb, BoxCollider, Capsule, Sphere};
use crate::components::Transform;
use crate::compound_shape::CompoundShape;
use crate::mesh::PlacedMesh;

/// A collider instance stored in the [`PhysicsWorld`](super::PhysicsWorld).
#[derive(Debug, Clone)]
pub(super) enum ColliderEntry {
    Sphere(Sphere),
    Box(BoxCollider),
    Capsule(Capsule),
    /// A triangle mesh at a transform: one entry, descending its own tree.
    Mesh(PlacedMesh),
    /// A compound's parts, placed: one entry, each part tested on its own.
    Compound(PlacedCompound),
}

/// One shape a query's narrow phase tests: the whole of a collider, or one
/// part of a compound.
#[derive(Debug, Clone, Copy)]
pub(super) enum Primitive<'a> {
    Sphere(&'a Sphere),
    Box(&'a BoxCollider),
    Capsule(&'a Capsule),
    Mesh(&'a PlacedMesh),
}

/// A [`CompoundShape`] placed on a body: each part a turned box, in the
/// shape's order, and the world-axis box around them all for the broadphase.
#[derive(Debug, Clone)]
pub(super) struct PlacedCompound {
    parts: Vec<BoxCollider>,
    bounds: Aabb,
}

impl PlacedCompound {
    /// `shape` `offset` from a body at `transform`, its parts placed as the
    /// contact pipeline places them ([`crate::compound_shape::CompoundPart`]'s
    /// placement), written into `parts`, which is cleared first: a moving
    /// compound is placed again every tick, and handing it the old entry's
    /// buffer saves a new one each time.
    pub(super) fn place(
        mut parts: Vec<BoxCollider>,
        shape: &CompoundShape,
        offset: DVec3,
        transform: &Transform,
    ) -> Self {
        parts.clear();
        parts.extend(
            shape
                .parts()
                .iter()
                .map(|part| part.placed(offset, transform)),
        );
        let bounds = parts
            .iter()
            .fold(Aabb::EMPTY, |bounds, part| bounds.union(part.aabb()));
        Self { parts, bounds }
    }

    /// The parts' buffer, taken for the next placement to reuse: what is
    /// left holds no parts, and is replaced by that placement at once.
    pub(super) fn take_parts(&mut self) -> Vec<BoxCollider> {
        core::mem::take(&mut self.parts)
    }

    /// The placed parts, in the shape's order.
    #[cfg(test)]
    pub(super) fn parts(&self) -> &[BoxCollider] {
        &self.parts
    }
}

impl ColliderEntry {
    pub(super) fn aabb(&self) -> Aabb {
        match self {
            ColliderEntry::Sphere(s) => s.aabb(),
            ColliderEntry::Box(b) => b.aabb(),
            ColliderEntry::Capsule(c) => c.aabb(),
            ColliderEntry::Mesh(m) => m.bounds,
            ColliderEntry::Compound(c) => c.bounds,
        }
    }

    /// Every shape a query tests in this collider, each with its part index —
    /// the index into a compound's [`CompoundShape::parts`], and `0` for a
    /// collider of one shape — in part order.
    ///
    /// The order is fixed by the shape, so a query that keeps the first of
    /// several equally near parts keeps the same one on every run.
    pub(super) fn primitives(&self) -> impl Iterator<Item = (usize, Primitive<'_>)> {
        let (single, parts): (Option<Primitive<'_>>, &[BoxCollider]) = match self {
            ColliderEntry::Sphere(s) => (Some(Primitive::Sphere(s)), &[]),
            ColliderEntry::Box(b) => (Some(Primitive::Box(b)), &[]),
            ColliderEntry::Capsule(c) => (Some(Primitive::Capsule(c)), &[]),
            ColliderEntry::Mesh(m) => (Some(Primitive::Mesh(m)), &[]),
            ColliderEntry::Compound(c) => (None, &c.parts),
        };
        single.map(|shape| (0, shape)).into_iter().chain(
            parts
                .iter()
                .enumerate()
                .map(|(part, shape)| (part, Primitive::Box(shape))),
        )
    }
}
