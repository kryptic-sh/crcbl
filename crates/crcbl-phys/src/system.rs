//! ECS physics system: bridges `PhysicsWorld` with entity-component data.
//!
//! [`PhysicsSystem`] implements [`crcbl_ecs::SystemTrait`] so it can be
//! registered in a schedule. It owns a [`crate::PhysicsWorld`], stores
//! per-entity rigid body and transform data, and drives the integration
//! loop with configurable force providers.

use std::collections::HashMap;

use crcbl_ecs::{DebugCtx, Entity, SystemTrait};
use glam::DVec3;

use crate::collider::{Aabb, BoxCollider, Capsule, Sphere};
use crate::components::{ColliderComponent, RigidBody, Transform};
use crate::forces::ForceProvider;
use crate::integrator::{Integrator as _, SemiImplicitEuler};
use crate::query::ShapeHit;
use crate::world::{ColliderId, PhysicsWorld};
use crate::{Ray, Segment};

// ---------------------------------------------------------------------------
// PhysicsSystem
// ---------------------------------------------------------------------------

/// ECS system that owns a [`PhysicsWorld`], stores rigid body dynamics data,
/// and runs the integration loop with force providers.
///
/// # Integration loop
///
/// Each `tick(dt)` calls `step(dt)` once with the schedule's real tick period.
/// The caller is responsible for fixed-timestep accumulation (substepping);
/// call `step(substep_dt)` multiple times per tick to advance physics at a
/// higher rate than the game tick.
pub struct PhysicsSystem {
    world: PhysicsWorld,

    /// Entity → ColliderId mapping.
    entity_to_collider: HashMap<Entity, ColliderId>,
    /// ColliderId → Entity reverse mapping.
    collider_to_entity: Vec<Option<Entity>>,

    /// Per-entity rigid body data.
    bodies: HashMap<Entity, RigidBody>,
    /// Per-entity world-space transform.
    transforms: HashMap<Entity, Transform>,
    /// Per-entity collider component (cached for transform updates).
    collider_comps: HashMap<Entity, ColliderComponent>,

    /// Force providers applied in order each substep before integration.
    force_providers: Vec<Box<dyn ForceProvider>>,
}

impl PhysicsSystem {
    /// Create an empty physics system.
    ///
    /// Integration is [`SemiImplicitEuler`]; see [`PhysicsSystem::step`].
    #[must_use]
    pub fn new() -> Self {
        Self {
            world: PhysicsWorld::new(),
            entity_to_collider: HashMap::new(),
            collider_to_entity: Vec::new(),
            bodies: HashMap::new(),
            transforms: HashMap::new(),
            collider_comps: HashMap::new(),
            force_providers: Vec::new(),
        }
    }

    /// Number of entities with colliders registered.
    #[must_use]
    pub fn collider_count(&self) -> usize {
        self.entity_to_collider.len()
    }

    /// Number of entities with rigid bodies registered.
    #[must_use]
    pub fn body_count(&self) -> usize {
        self.bodies.len()
    }

    // ── Dynamics setup ───────────────────────────────────────────────────

    /// Register a rigid body for `entity`.
    ///
    /// Replaces any existing body. The entity will participate in the
    /// integration loop.
    pub fn set_body(&mut self, entity: Entity, body: RigidBody) {
        self.bodies.insert(entity, body);
        self.transforms.entry(entity).or_insert(Transform::IDENTITY);
    }

    /// Set the world-space transform for `entity`.
    ///
    /// If the entity has a collider, it is repositioned immediately.
    pub fn set_transform(&mut self, entity: Entity, transform: Transform) {
        self.transforms.insert(entity, transform);
        self.sync_collider_from_cached(entity);
    }

    /// Get a reference to an entity's rigid body.
    #[must_use]
    pub fn body(&self, entity: Entity) -> Option<&RigidBody> {
        self.bodies.get(&entity)
    }

    /// Get a reference to an entity's transform.
    #[must_use]
    pub fn transform(&self, entity: Entity) -> Option<&Transform> {
        self.transforms.get(&entity)
    }

    /// Add a force provider. Providers are applied in order before
    /// integration.
    pub fn add_force_provider(&mut self, provider: Box<dyn ForceProvider>) {
        self.force_providers.push(provider);
    }

    // ── Collider management ────────────────────────────────────────────

    /// Add or replace a collider for `entity`.
    ///
    /// The world-space position is `transform.position + component.offset`.
    /// The component is cached so [`PhysicsSystem::step`] can reposition the
    /// collider after integration.
    pub fn set_collider(
        &mut self,
        entity: Entity,
        component: &ColliderComponent,
        transform: &Transform,
    ) {
        // Store transform, caching the component.
        self.transforms.insert(entity, *transform);
        self.collider_comps.insert(entity, component.clone());
        self.add_collider_to_world(entity, component, transform);
    }

    /// Remove the collider and dynamics data for `entity`.
    pub fn remove_entity(&mut self, entity: Entity) {
        self.remove_collider(entity);
        self.bodies.remove(&entity);
        self.transforms.remove(&entity);
        self.collider_comps.remove(&entity);
    }

    /// Remove only the collider (no-op if none).
    pub fn remove_collider(&mut self, entity: Entity) {
        if let Some(id) = self.entity_to_collider.remove(&entity) {
            self.world.remove(id);
            let slot = id.index() as usize;
            if slot < self.collider_to_entity.len() {
                self.collider_to_entity[slot] = None;
            }
        }
    }

    // ── Integration ────────────────────────────────────────────────────

    /// Advance dynamics by one substep of `dt` seconds.
    ///
    /// Applies all force providers, then integrates every dynamic body with
    /// [`SemiImplicitEuler`]. Collider positions are synced to the new
    /// transforms.
    ///
    /// Bodies are visited in ascending entity order. `bodies` is a `HashMap`
    /// with a randomly-seeded hasher, so its iteration order differs between
    /// processes; floating-point work in a different order is a different
    /// result, and this crate promises determinism.
    pub fn step(&mut self, dt: f64) {
        // Collect entity list to avoid borrow conflicts, in a canonical order
        // so the arithmetic does not depend on the map's seed.
        let mut entities: Vec<Entity> = self.bodies.keys().copied().collect();
        entities.sort_unstable_by_key(|entity| entity.to_bits());

        // Apply forces.
        for &entity in &entities {
            let Some(body) = self.bodies.get_mut(&entity) else {
                continue;
            };
            let transform = self.transforms.get(&entity).copied().unwrap_or_default();
            for provider in &self.force_providers {
                provider.apply(body, &transform, dt);
            }
        }

        // Integrate.
        for &entity in &entities {
            let Some(body) = self.bodies.get_mut(&entity) else {
                continue;
            };
            let Some(transform) = self.transforms.get_mut(&entity) else {
                continue;
            };
            SemiImplicitEuler.step(body, transform, dt);
        }

        // Sync colliders to new transforms.
        for entity in entities {
            self.sync_collider_from_cached(entity);
        }
    }

    // ── Queries ────────────────────────────────────────────────────────

    /// Cast a ray, returning the closest hit entity and details.
    #[must_use]
    pub fn cast_ray(&mut self, ray: &Ray) -> Option<(Entity, ShapeHit)> {
        self.world.cast_ray(ray).and_then(|(id, hit)| {
            let slot = id.index() as usize;
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
                let slot = id.index() as usize;
                self.collider_to_entity
                    .get(slot)
                    .and_then(|e| e.map(|entity| (entity, hit)))
            })
    }

    /// Overlap query: return all entities whose collider overlaps the sphere.
    #[must_use]
    pub fn overlap_sphere(&mut self, centre: DVec3, radius: f64) -> Vec<(Entity, ShapeHit)> {
        self.world
            .overlap_sphere(centre, radius)
            .into_iter()
            .filter_map(|id| {
                let slot = id.index() as usize;
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
                let slot = id.index() as usize;
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

    // ── Internals ──────────────────────────────────────────────────────

    fn add_collider_to_world(
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
        let slot = id.index() as usize;
        if slot >= self.collider_to_entity.len() {
            self.collider_to_entity.resize(slot + 1, None);
        }
        self.collider_to_entity[slot] = Some(entity);
    }

    /// Reposition the collider for `entity` from the cached component and
    /// current transform. No-op if entity has no collider.
    fn sync_collider_from_cached(&mut self, entity: Entity) {
        let Some(&id) = self.entity_to_collider.get(&entity) else {
            return;
        };
        let Some(comp) = self.collider_comps.get(&entity) else {
            return;
        };
        let Some(transform) = self.transforms.get(&entity) else {
            return;
        };

        let centre = transform.position;
        match comp {
            ColliderComponent::Sphere {
                offset,
                radius,
                is_trigger: _,
            } => {
                let c = centre + *offset;
                self.world.set_sphere(id, Sphere::new(c, *radius));
            }
            ColliderComponent::Box {
                offset,
                half_extents,
                is_trigger: _,
            } => {
                let c = centre + *offset;
                self.world.set_box(id, BoxCollider::new(c, *half_extents));
            }
            ColliderComponent::Capsule {
                offset,
                radius,
                half_height,
                is_trigger: _,
            } => {
                let c = centre + *offset;
                self.world
                    .set_capsule(id, Capsule::new(c, *radius, *half_height));
            }
        }
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
            .field("body_count", &self.bodies.len())
            .field("force_provider_count", &self.force_providers.len())
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

    fn tick(&mut self, dt: f64) {
        // One substep at the schedule's real tick period.  Assuming a rate
        // here instead would make simulated speed a function of the host's
        // tick rate.  Callers wanting sub-stepping call `step(dt / n)` n times
        // directly instead of relying on the schedule.
        self.step(dt);
    }

    fn entity_count(&self) -> usize {
        self.entity_to_collider.len()
    }

    fn sweep(&mut self, dead: &[Entity]) {
        for &entity in dead {
            self.remove_entity(entity);
        }
    }

    fn debug_draw(&mut self, _ctx: &DebugCtx) {
        // Future: draw collider AABBs, contacts, swept paths.
    }

    /// Feed transform and rigid-body state into the determinism hash.
    ///
    /// Entities are visited in ascending `to_bits()` order (the `HashMap`s
    /// this reads are seeded per process) and every float goes in as
    /// `to_bits()`, so the hash is a bit-exact function of the simulation
    /// state and nothing else.
    fn hash_state(&self, hasher: &mut dyn std::hash::Hasher) {
        let mut entities: Vec<Entity> = self
            .transforms
            .keys()
            .chain(self.bodies.keys())
            .copied()
            .collect();
        entities.sort_unstable_by_key(|entity| entity.to_bits());
        entities.dedup();

        for entity in entities {
            hasher.write(&entity.to_bits().to_le_bytes());

            match self.transforms.get(&entity) {
                Some(transform) => {
                    hasher.write(&[1]);
                    for value in [
                        transform.position.x,
                        transform.position.y,
                        transform.position.z,
                        transform.rotation.x,
                        transform.rotation.y,
                        transform.rotation.z,
                        transform.rotation.w,
                    ] {
                        hasher.write(&value.to_bits().to_le_bytes());
                    }
                }
                None => hasher.write(&[0]),
            }

            match self.bodies.get(&entity) {
                Some(body) => {
                    hasher.write(&[1]);
                    for value in [
                        body.mass,
                        body.inverse_mass,
                        body.velocity.x,
                        body.velocity.y,
                        body.velocity.z,
                        body.force_accum.x,
                        body.force_accum.y,
                        body.force_accum.z,
                    ] {
                        hasher.write(&value.to_bits().to_le_bytes());
                    }
                }
                None => hasher.write(&[0]),
            }
        }
    }

    fn contributes_to_hash(&self) -> bool {
        true
    }

    fn replicate(&self, out: &mut Vec<u8>) -> bool {
        // Per-entity Transform: position (3 × f64 LE) then rotation
        // quaternion (4 × f64 LE, x/y/z/w). Entities are sorted by bits so
        // the wire encoding is deterministic across runs and platforms.
        let mut entries: Vec<_> = self.transforms.iter().collect();
        entries.sort_by_key(|(entity, _)| entity.to_bits());
        for (entity, transform) in entries {
            out.extend_from_slice(&entity.to_bits().to_le_bytes());
            out.extend_from_slice(&(transform.encoded_len() as u32).to_le_bytes());
            transform.encode(out);
        }
        !self.transforms.is_empty()
    }

    fn as_any_mut(&mut self) -> &mut dyn std::any::Any {
        self
    }
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use crate::forces::GravityForce;
    use glam::DVec3;

    fn test_entity(idx: u32) -> Entity {
        Entity::from_bits(((1u64) << 32) | idx as u64).expect("test entity")
    }

    // ── Existing collider tests (preserved) ────────────────────────────

    #[test]
    fn empty_system_has_no_colliders() {
        let phys = PhysicsSystem::new();
        assert_eq!(phys.collider_count(), 0);
    }

    #[test]
    fn replicate_encodes_sorted_transform_blobs() {
        let mut phys = PhysicsSystem::new();
        let e1 = test_entity(9);
        let e2 = test_entity(2);
        // Insert out of order — the encoding must still come out sorted.
        phys.set_transform(e1, Transform::from_position(DVec3::new(1.0, 2.0, 3.0)));
        phys.set_transform(
            e2,
            Transform::new(
                DVec3::new(-4.0, 0.5, 8.0),
                glam::DQuat::from_xyzw(0.0, 0.0, 0.0, 1.0),
            ),
        );

        let mut out = Vec::new();
        assert!(SystemTrait::replicate(&phys, &mut out));

        let entity_len = 8 + 4 + Transform::IDENTITY.encoded_len();
        assert_eq!(out.len(), 2 * entity_len);

        let mut cursor = 0usize;
        let mut decoded = Vec::new();
        while cursor < out.len() {
            let bits = u64::from_le_bytes(out[cursor..cursor + 8].try_into().unwrap());
            cursor += 8;
            let len = u32::from_le_bytes(out[cursor..cursor + 4].try_into().unwrap()) as usize;
            cursor += 4;
            decoded.push((bits, Transform::decode(&out[cursor..cursor + len]).unwrap()));
            cursor += len;
        }
        assert_eq!(decoded[0].0, e2.to_bits(), "sorted by entity bits");
        assert_eq!(decoded[1].0, e1.to_bits());
        assert_eq!(decoded[0].1.position, DVec3::new(-4.0, 0.5, 8.0));
        assert_eq!(decoded[1].1.position, DVec3::new(1.0, 2.0, 3.0));

        // An empty system writes nothing and reports as non-replicated.
        let empty = PhysicsSystem::new();
        let mut out = Vec::new();
        assert!(!SystemTrait::replicate(&empty, &mut out));
        assert!(out.is_empty());
    }

    #[test]
    fn transform_encode_decode_roundtrips_and_lerp_midpoint() {
        let t = Transform::new(
            DVec3::new(1.25, -2.5, 3.75),
            glam::DQuat::from_rotation_y(1.0),
        );
        let mut buf = Vec::new();
        t.encode(&mut buf);
        assert_eq!(buf.len(), t.encoded_len());
        let decoded = Transform::decode(&buf).unwrap();
        assert_eq!(decoded.position, t.position);
        assert!((decoded.rotation.dot(t.rotation) - 1.0).abs() < 1e-12);

        // Wrong-length payloads decode to None.
        assert!(Transform::decode(&buf[..buf.len() - 1]).is_none());
        assert!(Transform::decode(&[]).is_none());

        let a = Transform::from_position(DVec3::new(0.0, 0.0, 0.0));
        let b = Transform::from_position(DVec3::new(10.0, -4.0, 2.0));
        let mid = a.lerp(&b, 0.5);
        assert_eq!(mid.position, DVec3::new(5.0, -2.0, 1.0));
        let at_a = a.lerp(&b, 0.0);
        let at_b = a.lerp(&b, 1.0);
        assert_eq!(at_a.position, a.position);
        assert_eq!(at_b.position, b.position);
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

    // ── Dynamics tests ──────────────────────────────────────────────────

    #[test]
    fn body_falls_under_gravity() {
        let mut phys = PhysicsSystem::new();
        phys.add_force_provider(Box::new(GravityForce::EARTH));
        let e = test_entity(0);
        phys.set_body(e, RigidBody::new_dynamic(1.0));
        phys.set_transform(e, Transform::from_position(DVec3::new(0.0, 100.0, 0.0)));

        // Step for ~1 second at 60 Hz.
        let dt = 1.0 / 60.0;
        for _ in 0..60 {
            phys.step(dt);
        }

        let t = phys.transform(e).unwrap();
        // After 1s: pos.y ≈ 100 - 0.5 * 9.81 * 1² = 100 - 4.905 ≈ 95.095
        assert!(
            t.position.y < 96.0 && t.position.y > 94.0,
            "expected pos.y ~95.1, got {}",
            t.position.y
        );

        let b = phys.body(e).unwrap();
        // vy ≈ -9.81
        assert!(
            b.velocity.y < -9.0 && b.velocity.y > -10.5,
            "expected vy ~-9.81, got {}",
            b.velocity.y
        );
    }

    #[test]
    fn kinematic_body_stays_put() {
        let mut phys = PhysicsSystem::new();
        phys.add_force_provider(Box::new(GravityForce::EARTH));
        let e = test_entity(0);
        phys.set_body(e, RigidBody::new_kinematic());
        phys.set_transform(e, Transform::from_position(DVec3::new(5.0, 5.0, 5.0)));

        phys.step(1.0 / 60.0);
        let t = phys.transform(e).unwrap();
        assert_eq!(t.position, DVec3::new(5.0, 5.0, 5.0));
    }

    #[test]
    fn collider_moves_with_body_after_integration() {
        let mut phys = PhysicsSystem::new();
        let e = test_entity(0);
        let body = {
            let mut b = RigidBody::new_dynamic(1.0);
            b.velocity = DVec3::new(10.0, 0.0, 0.0);
            b
        };
        let transform = Transform::from_position(DVec3::new(0.0, 0.0, 0.0));
        let comp = ColliderComponent::Sphere {
            offset: DVec3::ZERO,
            radius: 1.0,
            is_trigger: false,
        };
        phys.set_body(e, body);
        phys.set_collider(e, &comp, &transform);

        // Step: pos moves by vel * dt.
        phys.step(0.5);
        // pos = (5, 0, 0)
        let t = phys.transform(e).unwrap();
        assert!((t.position.x - 5.0).abs() < 1e-12);

        // Collider should have moved to the new position.
        let ray = Ray::new(DVec3::new(-5.0, 0.0, 0.0), DVec3::X);
        let result = phys.cast_ray(&ray);
        assert!(result.is_some());
        let (hit_entity, hit) = result.unwrap();
        assert_eq!(hit_entity, e);
        // Sphere at (5,0,0) radius 1: hit at t = (5 - 1) = 4 from origin at x=-5
        // Actually: ray origin is -5, sphere centre is 5, radius 1 → hit at t ≈ 9
        // Direction is (1,0,0) so t is distance from -5 to 4 = 9.
        assert!((hit.t - 9.0).abs() < 0.01, "t = {}", hit.t);
    }

    #[test]
    fn remove_entity_cleans_up_all_data() {
        let mut phys = PhysicsSystem::new();
        let e = test_entity(0);
        phys.set_body(e, RigidBody::new_dynamic(1.0));
        phys.set_collider(
            e,
            &ColliderComponent::Sphere {
                offset: DVec3::ZERO,
                radius: 1.0,
                is_trigger: false,
            },
            &Transform::IDENTITY,
        );
        assert_eq!(phys.body_count(), 1);
        assert_eq!(phys.collider_count(), 1);

        phys.remove_entity(e);
        assert_eq!(phys.body_count(), 0);
        assert_eq!(phys.collider_count(), 0);
        assert!(phys.body(e).is_none());
        assert!(phys.transform(e).is_none());
    }

    #[test]
    fn multiple_bodies_integrate_independently() {
        let mut phys = PhysicsSystem::new();
        phys.add_force_provider(Box::new(GravityForce::EARTH));

        let e1 = test_entity(0);
        let e2 = test_entity(1);

        phys.set_body(e1, RigidBody::new_dynamic(1.0));
        phys.set_transform(e1, Transform::from_position(DVec3::new(0.0, 0.0, 0.0)));

        // e2: kinematic, should not move.
        phys.set_body(e2, RigidBody::new_kinematic());
        phys.set_transform(e2, Transform::from_position(DVec3::new(10.0, 10.0, 0.0)));

        phys.step(1.0 / 60.0);

        let t1 = phys.transform(e1).unwrap();
        let t2 = phys.transform(e2).unwrap();
        assert!(t1.position.y < 0.0, "dynamic body should fall");
        assert_eq!(
            t2.position,
            DVec3::new(10.0, 10.0, 0.0),
            "kinematic should not move"
        );
    }

    #[test]
    fn step_with_no_bodies_does_not_panic() {
        let mut phys = PhysicsSystem::new();
        phys.add_force_provider(Box::new(GravityForce::EARTH));
        phys.step(1.0 / 60.0);
        // Just shouldn't panic.
    }

    // ── Schedule integration ────────────────────────────────────────────

    #[test]
    fn tick_uses_the_schedule_dt_rather_than_an_assumed_rate() {
        // Same simulated second, two tick rates.  A hardcoded step would make
        // the 30 Hz run cover half the ground of the 60 Hz one.
        let simulate = |hz: u32| {
            let mut phys = PhysicsSystem::new();
            let e = test_entity(0);
            let mut body = RigidBody::new_dynamic(1.0);
            body.velocity = DVec3::new(10.0, 0.0, 0.0);
            phys.set_body(e, body);
            phys.set_transform(e, Transform::IDENTITY);

            let dt = 1.0 / f64::from(hz);
            for _ in 0..hz {
                SystemTrait::tick(&mut phys, dt);
            }
            phys.transform(e).unwrap().position.x
        };

        let at_60 = simulate(60);
        let at_30 = simulate(30);
        assert!((at_60 - 10.0).abs() < 1e-9, "x = {at_60}");
        assert!((at_30 - 10.0).abs() < 1e-9, "x = {at_30}");
    }

    #[test]
    fn step_visits_bodies_in_canonical_order() {
        // Insertion order must not reach the arithmetic: the `bodies` map is
        // randomly seeded, so anything that depended on it would differ per
        // process.  Two systems built in opposite orders must agree bit for
        // bit.
        let build = |reverse: bool| {
            let mut phys = PhysicsSystem::new();
            phys.add_force_provider(Box::new(GravityForce::EARTH));
            let mut indices: Vec<u32> = (0..16).collect();
            if reverse {
                indices.reverse();
            }
            for i in indices {
                let e = test_entity(i);
                let mut body = RigidBody::new_dynamic(1.0 + f64::from(i) * 0.25);
                body.velocity = DVec3::new(f64::from(i) * 0.1, 0.0, 0.0);
                phys.set_body(e, body);
                phys.set_transform(e, Transform::from_position(DVec3::splat(f64::from(i))));
            }
            for _ in 0..120 {
                phys.step(1.0 / 120.0);
            }
            phys
        };

        let forward = build(false);
        let backward = build(true);
        for i in 0..16 {
            let e = test_entity(i);
            assert_eq!(
                forward.transform(e).unwrap().position,
                backward.transform(e).unwrap().position,
                "entity {i} diverged"
            );
        }
    }

    #[test]
    fn hash_state_covers_physics_state_in_canonical_order() {
        use std::collections::hash_map::DefaultHasher;
        use std::hash::Hasher as _;

        fn hash(phys: &PhysicsSystem) -> u64 {
            let mut h = DefaultHasher::new();
            SystemTrait::hash_state(phys, &mut h);
            h.finish()
        }

        let build = |reverse: bool, y: f64| {
            let mut phys = PhysicsSystem::new();
            let mut indices: Vec<u32> = (0..8).collect();
            if reverse {
                indices.reverse();
            }
            for i in indices {
                let e = test_entity(i);
                phys.set_body(e, RigidBody::new_dynamic(1.0 + f64::from(i)));
                phys.set_transform(
                    e,
                    Transform::from_position(DVec3::new(f64::from(i), y, 0.0)),
                );
            }
            phys
        };

        // The system must announce that it contributes, or the server's
        // determinism harness silently skips all of physics.
        assert!(SystemTrait::contributes_to_hash(&build(false, 0.0)));

        // Insertion order is not state; position is.
        assert_eq!(hash(&build(false, 0.0)), hash(&build(true, 0.0)));
        assert_ne!(hash(&build(false, 0.0)), hash(&build(false, 1.0)));

        // A velocity change alone must move the hash.
        let mut moving = build(false, 0.0);
        let e = test_entity(3);
        let mut body = *moving.body(e).unwrap();
        body.velocity = DVec3::new(0.0, -1.0, 0.0);
        moving.set_body(e, body);
        assert_ne!(hash(&build(false, 0.0)), hash(&moving));
    }
}
