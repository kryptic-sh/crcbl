//! What the query world holds for one collider.

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

impl ColliderEntry {
    pub(super) fn aabb(&self) -> Aabb {
        match self {
            ColliderEntry::Sphere(s) => s.aabb(),
            ColliderEntry::Box(b) => b.aabb(),
            ColliderEntry::Capsule(c) => c.aabb(),
            ColliderEntry::Mesh(m) => m.bounds,
        }
    }
}
