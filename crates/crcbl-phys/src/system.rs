//! ECS physics system: bridges `PhysicsWorld` with entity-component data.
//!
//! [`PhysicsSystem`] implements [`crcbl_ecs::SystemTrait`] so it can be
//! registered in a schedule. It owns a [`crate::PhysicsWorld`] and syncs
//! collider components each tick, providing ray/overlap/sweep queries keyed
//! by entity.

use std::collections::HashMap;

use crcbl_ecs::{DebugCtx, Entity, SystemTrait};
use glam::DVec3;

use crate::collider::{Aabb, BoxCollider, Capsule, Sphere};
use crate::components::{ColliderComponent, Transform};
use crate::query::ShapeHit;
use crate::world::{ColliderId, PhysicsWorld};
use crate::{Ray, Segment};

// ---------------------------------------------------------------------------
// PhysicsSystem
// ---------------------------------------------------------------------------

/// ECS system that owns a [`PhysicsWorld`] and maps entities to colliders.
///
/// # Usage
///
/// ```ignore
/// let mut phys = PhysicsSystem::new();
/// phys.set_collider(player_entity, &collider_comp, &transform);
/// let hits = phys.overlap_sphere(origin, 10.0);
/// ```
pub struct PhysicsSystem {
    world: PhysicsWorld,
    /// Entity → ColliderId mapping.
    entity_to_collider: HashMap<Entity, ColliderId>,
    /// ColliderId → Entity reverse mapping (sparse, index by ColliderId.0).
    collider_to_entity: Vec<Option<Entity>>,
}

impl PhysicsSystem {
    /// Create an empty physics system.
    #[must_use]
    pub fn new() -> Self {
        Self {
            world: PhysicsWorld::new(),
            entity_to_collider: HashMap::new(),
            collider_to_entity: Vec::new(),
        }
    }

    /// Number of colliders registered.
    #[must_use]
    pub fn collider_count(&self) -> usize {
        self.entity_to_collider.len()
    }

    // ── Collider management ────────────────────────────────────────────

    /// Add or replace a collider for `entity`.
    ///
    /// The collider's world-space position is `transform.position + component.offset`.
    /// If the entity already has a collider it is replaced.
    pub fn set_collider(
        &mut self,
        entity: Entity,
        component: &ColliderComponent,
        transform: &Transform,
    ) {
        // Remove existing if present.
        self.remove_collider(entity);

        let world_centre = transform.position;
        let id = match component {
            ColliderComponent::Sphere {
                offset,
                radius,
                is_trigger,
            } => {
                let centre = world_centre + *offset;
                let id = self.world.add_sphere(Sphere::new(centre, *radius));
                self.world.set_trigger(id, *is_trigger);
                id
            }
            ColliderComponent::Box {
                offset,
                half_extents,
                is_trigger,
            } => {
                let centre = world_centre + *offset;
                let id = self.world.add_box(BoxCollider::new(centre, *half_extents));
                self.world.set_trigger(id, *is_trigger);
                id
            }
            ColliderComponent::Capsule {
                offset,
                radius,
                half_height,
                is_trigger,
            } => {
                let centre = world_centre + *offset;
                let id = self
                    .world
                    .add_capsule(Capsule::new(centre, *radius, *half_height));
                self.world.set_trigger(id, *is_trigger);
                id
            }
        };

        self.entity_to_collider.insert(entity, id);
        // Grow reverse mapping if needed.
        let slot = id.0 as usize;
        if slot >= self.collider_to_entity.len() {
            self.collider_to_entity.resize(slot + 1, None);
        }
        self.collider_to_entity[slot] = Some(entity);
    }

    /// Remove the collider for `entity` (no-op if none).
    pub fn remove_collider(&mut self, entity: Entity) {
        if let Some(id) = self.entity_to_collider.remove(&entity) {
            self.world.remove(id);
            let slot = id.0 as usize;
            if slot < self.collider_to_entity.len() {
                self.collider_to_entity[slot] = None;
            }
        }
    }

    /// Update the transform-derived position of an existing collider.
    ///
    /// Returns `false` if the entity has no collider registered.
    pub fn update_transform(&mut self, entity: Entity, transform: &Transform) -> bool {
        let Some(&id) = self.entity_to_collider.get(&entity) else {
            return false;
        };
        let slot = id.0 as usize;
        let Some(Some(entry_entity)) = self.collider_to_entity.get(slot) else {
            return false;
        };
        debug_assert_eq!(*entry_entity, entity);

        // Read current collider from world, offset from old transform, apply
        // new transform. We don't store the local offset separately, so we
        // approximate: just move the collider's centre to the new transform
        // position.  For now this is a centre-only update — proper local-offset
        // tracking is future work.
        //
        // Since we control ColliderEntry (but it's private), we use the public
        // set_* API with the current shape data. We reconstruct from the
        // component that was last set — but we don't cache the component.
        // For now, this is a best-effort centre shift.
        _ = transform;
        false // stub: full implementation needs component caching
    }

    // ── Queries ────────────────────────────────────────────────────────

    /// Cast a ray, returning the closest hit entity and details.
    #[must_use]
    pub fn cast_ray(&mut self, ray: &Ray) -> Option<(Entity, ShapeHit)> {
        self.world.cast_ray(ray).and_then(|(id, hit)| {
            let slot = id.0 as usize;
            self.collider_to_entity
                .get(slot)
                .and_then(|e| e.map(|entity| (entity, hit)))
        })
    }

    /// Sweep a sphere, returning the closest hit entity and details.
    #[must_use]
    pub fn sweep_sphere(&mut self, segment: &Segment, radius: f64) -> Option<(Entity, ShapeHit)> {
        self.world
            .sweep_sphere(segment, radius)
            .and_then(|(id, hit)| {
                let slot = id.0 as usize;
                self.collider_to_entity
                    .get(slot)
                    .and_then(|e| e.map(|entity| (entity, hit)))
            })
    }

    /// Overlap query: return all entities whose collider overlaps the sphere.
    #[must_use]
    pub fn overlap_sphere(&mut self, centre: DVec3, radius: f64) -> Vec<(Entity, ShapeHit)> {
        // The world returns ColliderIds; map back to entities.
        // For now we don't have a hit-struct from overlap, so return empty hits.
        self.world
            .overlap_sphere(centre, radius)
            .into_iter()
            .filter_map(|id| {
                let slot = id.0 as usize;
                let entity = self.collider_to_entity.get(slot).and_then(|e| *e)?;
                Some((
                    entity,
                    ShapeHit {
                        t: 0.0,
                        point: centre,
                        normal: DVec3::Y,
                        started_inside: true,
                    },
                ))
            })
            .collect()
    }

    /// Overlap query: return all entities whose AABB intersects `aabb`.
    #[must_use]
    pub fn overlap_aabb(&mut self, aabb: &Aabb) -> Vec<Entity> {
        self.world
            .overlap_aabb(aabb)
            .into_iter()
            .filter_map(|id| {
                let slot = id.0 as usize;
                self.collider_to_entity.get(slot).and_then(|e| *e)
            })
            .collect()
    }

    /// Direct access to the underlying [`PhysicsWorld`] for advanced use.
    #[must_use]
    pub fn world(&self) -> &PhysicsWorld {
        &self.world
    }

    /// Mutable access to the underlying [`PhysicsWorld`] for advanced use.
    pub fn world_mut(&mut self) -> &mut PhysicsWorld {
        &mut self.world
    }
}

impl Default for PhysicsSystem {
    fn default() -> Self {
        Self::new()
    }
}

impl std::fmt::Debug for PhysicsSystem {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("PhysicsSystem")
            .field("collider_count", &self.entity_to_collider.len())
            .finish()
    }
}

// ---------------------------------------------------------------------------
// SystemTrait impl
// ---------------------------------------------------------------------------

impl SystemTrait for PhysicsSystem {
    fn name(&self) -> &str {
        "physics"
    }

    fn tick(&mut self) {
        // Physics substeps happen here once forces + integration are wired.
        // For now this is a no-op — the system's main role is query dispatch.
    }

    fn entity_count(&self) -> usize {
        self.entity_to_collider.len()
    }

    fn sweep(&mut self, dead: &[Entity]) {
        for &entity in dead {
            self.remove_collider(entity);
        }
    }

    fn debug_draw(&mut self, _ctx: &DebugCtx) {
        // Future: draw collider AABBs, contacts, swept paths.
    }
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use glam::DVec3;

    fn test_entity(idx: u32) -> Entity {
        Entity::from_bits(((1u64) << 32) | idx as u64).expect("test entity")
    }

    #[test]
    fn empty_system_has_no_colliders() {
        let phys = PhysicsSystem::new();
        assert_eq!(phys.collider_count(), 0);
    }

    #[test]
    fn set_and_remove_collider() {
        let mut phys = PhysicsSystem::new();
        let e = test_entity(0);
        let transform = Transform::from_position(DVec3::new(1.0, 2.0, 3.0));
        let comp = ColliderComponent::Sphere {
            offset: DVec3::ZERO,
            radius: 1.0,
            is_trigger: false,
        };
        phys.set_collider(e, &comp, &transform);
        assert_eq!(phys.collider_count(), 1);
        phys.remove_collider(e);
        assert_eq!(phys.collider_count(), 0);
    }

    #[test]
    fn ray_cast_hits_entity() {
        let mut phys = PhysicsSystem::new();
        let e = test_entity(0);
        let transform = Transform::from_position(DVec3::new(5.0, 0.0, 0.0));
        let comp = ColliderComponent::Sphere {
            offset: DVec3::ZERO,
            radius: 1.0,
            is_trigger: false,
        };
        phys.set_collider(e, &comp, &transform);
        let ray = Ray::new(DVec3::ZERO, DVec3::X);
        let result = phys.cast_ray(&ray);
        assert!(result.is_some());
        let (hit_entity, _) = result.unwrap();
        assert_eq!(hit_entity, e);
    }

    #[test]
    fn ray_cast_misses_when_no_collider() {
        let mut phys = PhysicsSystem::new();
        let ray = Ray::new(DVec3::ZERO, DVec3::X);
        assert!(phys.cast_ray(&ray).is_none());
    }

    #[test]
    fn overlap_aabb_finds_entity() {
        let mut phys = PhysicsSystem::new();
        let e = test_entity(0);
        let transform = Transform::from_position(DVec3::new(2.0, 0.0, 0.0));
        let comp = ColliderComponent::Sphere {
            offset: DVec3::ZERO,
            radius: 1.0,
            is_trigger: false,
        };
        phys.set_collider(e, &comp, &transform);
        let query = Aabb::from_centre_half(DVec3::ZERO, DVec3::splat(3.0));
        let hits = phys.overlap_aabb(&query);
        assert_eq!(hits.len(), 1);
        assert_eq!(hits[0], e);
    }

    #[test]
    fn sweep_removes_dead_entities() {
        let mut phys = PhysicsSystem::new();
        let e = test_entity(0);
        let transform = Transform::IDENTITY;
        let comp = ColliderComponent::Sphere {
            offset: DVec3::ZERO,
            radius: 1.0,
            is_trigger: false,
        };
        phys.set_collider(e, &comp, &transform);
        assert_eq!(phys.collider_count(), 1);
        phys.sweep(&[e]);
        assert_eq!(phys.collider_count(), 0);
    }

    #[test]
    fn entity_count_matches() {
        let mut phys = PhysicsSystem::new();
        assert_eq!(phys.entity_count(), 0);
        let e = test_entity(0);
        phys.set_collider(
            e,
            &ColliderComponent::Sphere {
                offset: DVec3::ZERO,
                radius: 1.0,
                is_trigger: false,
            },
            &Transform::IDENTITY,
        );
        assert_eq!(phys.entity_count(), 1);
    }

    #[test]
    fn replacing_collider_keeps_count_stable() {
        let mut phys = PhysicsSystem::new();
        let e = test_entity(0);
        let transform = Transform::IDENTITY;
        let comp1 = ColliderComponent::Sphere {
            offset: DVec3::ZERO,
            radius: 1.0,
            is_trigger: false,
        };
        let comp2 = ColliderComponent::Box {
            offset: DVec3::ZERO,
            half_extents: DVec3::splat(2.0),
            is_trigger: true,
        };
        phys.set_collider(e, &comp1, &transform);
        phys.set_collider(e, &comp2, &transform);
        assert_eq!(phys.collider_count(), 1);
    }

    #[test]
    fn sweep_sphere_hits_entity() {
        let mut phys = PhysicsSystem::new();
        let e = test_entity(0);
        let transform = Transform::from_position(DVec3::new(5.0, 0.0, 0.0));
        let comp = ColliderComponent::Sphere {
            offset: DVec3::ZERO,
            radius: 1.0,
            is_trigger: false,
        };
        phys.set_collider(e, &comp, &transform);
        let seg = Segment::new(DVec3::new(-5.0, 0.0, 0.0), DVec3::new(5.0, 0.0, 0.0));
        let result = phys.sweep_sphere(&seg, 0.5);
        assert!(result.is_some());
        let (hit_entity, _) = result.unwrap();
        assert_eq!(hit_entity, e);
    }
}
