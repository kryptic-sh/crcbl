//! A [`ColliderComponent`] on a body at a [`Transform`], put into the query
//! world: [`PhysicsWorld::add_collider`] and [`PhysicsWorld::place_collider`].
//!
//! [`crate::PhysicsSystem`] places every body's collider through these, and a
//! client's read-only copy of the world places the colliders it rebuilds from
//! snapshots through the same two, so a server's query and a client's query
//! over the same component at the same pose meet the same shape, to the bit.

use glam::{DQuat, DVec3};

use crate::collider::{BoxCollider, Capsule, Sphere};
use crate::components::{ColliderComponent, Transform};

use super::{ColliderId, PhysicsWorld};

impl PhysicsWorld {
    /// Register `component` on a body at `transform`: its offset and its
    /// shape turned by the body's rotation, as the contact pipeline places
    /// it, and flagged a trigger when the component is one.
    ///
    /// The collider is on every layer, as an `add_*` collider is; a caller
    /// that tags it does so with [`set_layers`](Self::set_layers).
    pub fn add_collider(
        &mut self,
        component: &ColliderComponent,
        transform: &Transform,
    ) -> ColliderId {
        let (collider, is_trigger) = match component {
            ColliderComponent::Sphere {
                offset,
                radius,
                is_trigger,
            } => {
                let centre = placed_centre(*offset, transform);
                (self.add_sphere(Sphere::new(centre, *radius)), is_trigger)
            }
            ColliderComponent::Box {
                offset,
                half_extents,
                is_trigger,
            } => (
                self.add_box(query_box(*offset, *half_extents, transform)),
                is_trigger,
            ),
            ColliderComponent::Capsule {
                offset,
                radius,
                half_height,
                is_trigger,
            } => {
                let centre = placed_centre(*offset, transform);
                (
                    self.add_turned_capsule(
                        Capsule::new(centre, *radius, *half_height),
                        transform.rotation,
                    ),
                    is_trigger,
                )
            }
            ColliderComponent::Compound {
                offset,
                shape,
                is_trigger,
            } => (self.add_compound(shape, *offset, transform), is_trigger),
            ColliderComponent::Mesh { mesh, is_trigger } => {
                (self.add_mesh(mesh.clone(), *transform), is_trigger)
            }
        };
        self.set_trigger(collider, *is_trigger);
        collider
    }

    /// Move `collider` to where `component` sits on a body at `transform`,
    /// placed as [`add_collider`](Self::add_collider) places it. Its trigger
    /// flag and its layers stay as they were. Returns `true` if the id was
    /// valid.
    pub fn place_collider(
        &mut self,
        collider: ColliderId,
        component: &ColliderComponent,
        transform: &Transform,
    ) -> bool {
        match component {
            ColliderComponent::Sphere { offset, radius, .. } => self.set_sphere(
                collider,
                Sphere::new(placed_centre(*offset, transform), *radius),
            ),
            ColliderComponent::Box {
                offset,
                half_extents,
                ..
            } => self.set_box(collider, query_box(*offset, *half_extents, transform)),
            ColliderComponent::Capsule {
                offset,
                radius,
                half_height,
                ..
            } => self.set_turned_capsule(
                collider,
                Capsule::new(placed_centre(*offset, transform), *radius, *half_height),
                transform.rotation,
            ),
            ColliderComponent::Compound { offset, shape, .. } => {
                self.set_compound(collider, shape, *offset, transform)
            }
            ColliderComponent::Mesh { mesh, .. } => {
                self.set_mesh(collider, mesh.clone(), *transform)
            }
        }
    }
}

/// Where a sphere or a capsule `offset` from a body at `transform` is
/// centred: the offset turned by the body's rotation, as the contact pipeline
/// places it ([`crate::contact::shape::ContactShape::placed`]).
///
/// An unturned body, or no offset, adds the offset as it is: the product
/// with the identity can change a zero's sign, and the colliders of unturned
/// bodies keep the centres they had before offsets turned, to the bit.
fn placed_centre(offset: DVec3, transform: &Transform) -> DVec3 {
    if transform.rotation == DQuat::IDENTITY || offset == DVec3::ZERO {
        transform.position + offset
    } else {
        transform.position + transform.rotation * offset
    }
}

/// What the query world holds for a box collider `offset` from a body at
/// `transform`: the box turned with the body, its offset turned too, as the
/// contact pipeline places it ([`crate::contact::shape::ContactShape::placed`]).
fn query_box(offset: DVec3, half_extents: DVec3, transform: &Transform) -> BoxCollider {
    BoxCollider::new(
        transform.position + transform.rotation * offset,
        half_extents,
    )
    .with_rotation(transform.rotation)
}
