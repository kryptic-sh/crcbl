//! What the query world holds for one collider, and the shapes a query's
//! narrow phase tests in it.
//!
//! Every query family asks an entry for its [`primitives`] and tests each,
//! rather than matching on the entry itself, so a collider holding several
//! shapes is answered by the same code as one holding a single shape.
//!
//! [`primitives`]: ColliderEntry::primitives

use crate::collider::{Aabb, BoxCollider, Capsule, Sphere};
use crate::mesh::PlacedMesh;

/// A collider instance stored in the [`PhysicsWorld`](super::PhysicsWorld).
#[derive(Debug, Clone)]
pub(super) enum ColliderEntry {
    Sphere(Sphere),
    Box(BoxCollider),
    Capsule(Capsule),
    /// A triangle mesh at a transform: one entry, descending its own tree.
    Mesh(PlacedMesh),
}

/// One shape a query's narrow phase tests.
#[derive(Debug, Clone, Copy)]
pub(super) enum Primitive<'a> {
    Sphere(&'a Sphere),
    Box(&'a BoxCollider),
    Capsule(&'a Capsule),
    Mesh(&'a PlacedMesh),
}

impl ColliderEntry {
    pub(super) fn aabb(&self) -> Aabb {
        match self {
            ColliderEntry::Sphere(s) => s.aabb(),
            ColliderEntry::Box(b) => b.aabb(),
            ColliderEntry::Capsule(c) => c.aabb(),
            ColliderEntry::Mesh(m) => m.bounds,
        }
    }

    /// Every shape a query tests in this collider.
    pub(super) fn primitives(&self) -> impl Iterator<Item = Primitive<'_>> {
        let shape = match self {
            ColliderEntry::Sphere(s) => Primitive::Sphere(s),
            ColliderEntry::Box(b) => Primitive::Box(b),
            ColliderEntry::Capsule(c) => Primitive::Capsule(c),
            ColliderEntry::Mesh(m) => Primitive::Mesh(m),
        };
        core::iter::once(shape)
    }
}
