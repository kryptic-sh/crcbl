//! Physics world: collider storage, BVH management, and query dispatch.
//!
//! [`PhysicsWorld`] is the main entry point for physics queries during
//! simulation. It stores collider shapes indexed by a generic `ColliderId`,
//! maintains a lazily-rebuilt BVH over their AABBs, and dispatches rays and
//! sweeps to the shape-level intersection functions.

use glam::DVec3;

use crate::broadphase::{Bvh, BvhHit, Ray, Segment};
use crate::collider::{Aabb, BoxCollider, Capsule, Sphere};
use crate::query::{self, ShapeHit};

/// Opaque identifier for a registered collider.
///
/// Created by [`PhysicsWorld::add_sphere`], [`PhysicsWorld::add_box`], or
/// [`PhysicsWorld::add_capsule`]. Use it to remove or update the collider.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct ColliderId(pub(crate) u32);

// ---------------------------------------------------------------------------
// Collider shape storage
// ---------------------------------------------------------------------------

/// A collider stored in the [`PhysicsWorld`], with an optional trigger flag.
#[derive(Debug, Clone)]
struct ColliderSlot {
    entry: ColliderEntry,
    /// When true, this collider is a trigger (generates overlap events rather
    /// than collision response).
    is_trigger: bool,
}

/// A collider instance stored in the [`PhysicsWorld`].
#[derive(Debug, Clone)]
enum ColliderEntry {
    Sphere(Sphere),
    Box(BoxCollider),
    Capsule(Capsule),
}

impl ColliderEntry {
    fn aabb(&self) -> Aabb {
        match self {
            ColliderEntry::Sphere(s) => s.aabb(),
            ColliderEntry::Box(b) => b.aabb(),
            ColliderEntry::Capsule(c) => c.aabb(),
        }
    }
}

// ---------------------------------------------------------------------------
// PhysicsWorld
// ---------------------------------------------------------------------------

/// A spatial world that stores colliders and supports ray/sweep queries.
///
/// Colliders are added with `add_sphere`, `add_box`, or `add_capsule` and
/// removed with `remove`. The internal BVH is rebuilt lazily — call
/// [`PhysicsWorld::rebuild`] after batch mutations, or it auto-rebuilds on
/// the next query.
pub struct PhysicsWorld {
    /// Dense storage of collider entries.
    colliders: Vec<Option<ColliderSlot>>,
    /// Free slots for reuse (indices into `colliders`).
    free_slots: Vec<u32>,
    /// Next id to assign.
    next_id: u32,
    /// Lazily-rebuilt BVH. `None` when dirty.
    bvh: Option<Bvh>,
    /// Maps collider slot index → BVH element position. Populated during
    /// [`PhysicsWorld::rebuild`]. Used by update methods for O(log n) refit.
    bvh_slot_to_elem: Vec<u32>,
    /// Incremented on every mutation; used to detect staleness.
    generation: u64,
}

impl PhysicsWorld {
    /// Create an empty physics world.
    #[must_use]
    pub fn new() -> Self {
        Self {
            colliders: Vec::new(),
            free_slots: Vec::new(),
            next_id: 0,
            bvh: None,
            bvh_slot_to_elem: Vec::new(),
            generation: 0,
        }
    }

    /// Register a sphere collider. Returns a [`ColliderId`] that can be used
    /// to remove or update the collider later.
    pub fn add_sphere(&mut self, sphere: Sphere) -> ColliderId {
        self.add(ColliderEntry::Sphere(sphere))
    }

    /// Register a box collider.
    pub fn add_box(&mut self, box_collider: BoxCollider) -> ColliderId {
        self.add(ColliderEntry::Box(box_collider))
    }

    /// Register a capsule collider.
    pub fn add_capsule(&mut self, capsule: Capsule) -> ColliderId {
        self.add(ColliderEntry::Capsule(capsule))
    }

    /// Update an existing sphere collider. Returns `true` if the id was valid.
    ///
    /// If the BVH is built, this refits the tree in O(log n). Otherwise the
    /// BVH is simply marked dirty for the next query.
    pub fn set_sphere(&mut self, id: ColliderId, sphere: Sphere) -> bool {
        self.set(id, ColliderEntry::Sphere(sphere))
    }

    /// Update an existing box collider.
    pub fn set_box(&mut self, id: ColliderId, box_collider: BoxCollider) -> bool {
        self.set(id, ColliderEntry::Box(box_collider))
    }

    /// Update an existing capsule collider.
    pub fn set_capsule(&mut self, id: ColliderId, capsule: Capsule) -> bool {
        self.set(id, ColliderEntry::Capsule(capsule))
    }

    /// Mark a collider as a trigger (non-solid, generates overlap events).
    /// Returns `false` if the id is invalid.
    pub fn set_trigger(&mut self, id: ColliderId, is_trigger: bool) -> bool {
        let slot = id.0 as usize;
        let Some(slot_data) = self.colliders.get_mut(slot).and_then(|s| s.as_mut()) else {
            return false;
        };
        slot_data.is_trigger = is_trigger;
        true
    }

    /// Whether a collider is a trigger.
    pub fn is_trigger(&self, id: ColliderId) -> bool {
        let slot = id.0 as usize;
        self.colliders
            .get(slot)
            .and_then(|s| s.as_ref())
            .is_some_and(|s| s.is_trigger)
    }

    /// Remove a collider by its id. Returns `true` if the id was valid.
    pub fn remove(&mut self, id: ColliderId) -> bool {
        let idx = id.0 as usize;
        if idx >= self.colliders.len() || self.colliders[idx].is_none() {
            return false;
        }
        self.colliders[idx] = None;
        self.free_slots.push(id.0);
        self.bvh = None;
        self.bvh_slot_to_elem.clear();
        self.generation = self.generation.wrapping_add(1);
        true
    }

    /// Rebuild the BVH from the current set of colliders.
    ///
    /// Called automatically on the first query after a mutation. Explicitly
    /// calling this is only needed when you want to control the timing of the
    /// rebuild (e.g. once per frame rather than once per query batch).
    pub fn rebuild(&mut self) {
        let elements: Vec<_> = self
            .colliders
            .iter()
            .enumerate()
            .filter_map(|(idx, slot)| slot.as_ref().map(|s| (s.entry.aabb(), idx as u32)))
            .collect();
        // Build slot→elem reverse mapping.
        self.bvh_slot_to_elem = vec![u32::MAX; self.colliders.len()];
        for (elem_idx, (_, slot)) in elements.iter().enumerate() {
            self.bvh_slot_to_elem[*slot as usize] = elem_idx as u32;
        }
        self.bvh = Some(Bvh::build(elements));
    }

    /// Number of colliders currently registered.
    #[must_use]
    pub fn len(&self) -> usize {
        self.colliders.iter().filter(|s| s.is_some()).count()
    }

    /// Whether the world has no colliders.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }

    // ── Queries ────────────────────────────────────────────────────────

    /// Return all collider ids whose shape overlaps the query sphere.
    ///
    /// Uses the BVH for broadphase culling, then tests exact shape overlap
    /// (sphere-vs-sphere, sphere-vs-AABB, sphere-vs-capsule).
    #[must_use]
    pub fn overlap_sphere(&mut self, centre: DVec3, radius: f64) -> Vec<ColliderId> {
        self.ensure_bvh();
        let bvh = self.bvh.as_ref().unwrap();
        let query_aabb = Aabb::from_centre_half(centre, DVec3::splat(radius));
        let candidates = bvh.traverse_aabb(&query_aabb);
        let query_sphere = Sphere::new(centre, radius);

        candidates
            .into_iter()
            .filter(|&idx| {
                let idx = idx as usize;
                self.colliders
                    .get(idx)
                    .and_then(|s| s.as_ref())
                    .is_some_and(|slot| match &slot.entry {
                        ColliderEntry::Sphere(s) => query::sphere_overlaps_sphere(&query_sphere, s),
                        ColliderEntry::Box(b) => {
                            query::sphere_overlaps_aabb(&query_sphere, &b.aabb())
                        }
                        ColliderEntry::Capsule(c) => {
                            query::sphere_overlaps_capsule(&query_sphere, c)
                        }
                    })
            })
            .map(ColliderId)
            .collect()
    }

    /// Return all collider ids whose AABB intersects the query AABB.
    ///
    /// This is a broadphase-only query — it tests AABB-vs-AABB without
    /// exact shape overlap. Use [`PhysicsWorld::overlap_sphere`] for exact
    /// shape-aware overlap.
    #[must_use]
    pub fn overlap_aabb(&mut self, aabb: &Aabb) -> Vec<ColliderId> {
        self.ensure_bvh();
        let bvh = self.bvh.as_ref().unwrap();
        bvh.traverse_aabb(aabb)
            .into_iter()
            .map(ColliderId)
            .collect()
    }

    /// Cast a ray against all colliders, returning the closest hit (if any).
    ///
    /// Results are tested against the exact shape geometry (not just the AABB).
    /// The returned `ColliderId` is the handle you got from `add_*`.
    #[must_use]
    pub fn cast_ray(&mut self, ray: &Ray) -> Option<(ColliderId, ShapeHit)> {
        self.ensure_bvh();
        let bvh = self.bvh.as_ref().unwrap();
        let hits = bvh.traverse_ray(ray);
        self.closest_hit(ray, &hits)
    }

    /// Sweep a sphere along a segment, returning the closest hit (if any).
    ///
    /// Uses the shape-level swept-sphere TOI functions for exact results.
    #[must_use]
    pub fn sweep_sphere(
        &mut self,
        segment: &Segment,
        radius: f64,
    ) -> Option<(ColliderId, ShapeHit)> {
        self.ensure_bvh();
        let bvh = self.bvh.as_ref().unwrap();
        let hits = bvh.traverse_segment(segment);
        self.closest_swept(segment, radius, &hits)
    }

    /// Get the AABB of a collider by id.
    #[must_use]
    pub fn aabb_of(&self, id: ColliderId) -> Option<Aabb> {
        let idx = id.0 as usize;
        self.colliders
            .get(idx)
            .and_then(|s| s.as_ref().map(|s| s.entry.aabb()))
    }

    /// The current generation counter (incremented on every mutation).
    /// Useful for change detection in ECS systems.
    #[must_use]
    pub fn generation(&self) -> u64 {
        self.generation
    }

    // ── Internal helpers ───────────────────────────────────────────────

    fn add(&mut self, entry: ColliderEntry) -> ColliderId {
        let slot_data = ColliderSlot {
            entry,
            is_trigger: false,
        };
        let id = if let Some(slot) = self.free_slots.pop() {
            self.colliders[slot as usize] = Some(slot_data);
            slot
        } else {
            let idx = self.next_id;
            self.next_id = idx.wrapping_add(1);
            self.colliders.push(Some(slot_data));
            idx
        };
        self.bvh = None;
        self.bvh_slot_to_elem.clear();
        self.generation = self.generation.wrapping_add(1);
        ColliderId(id)
    }

    /// Update an existing collider entry, refitting the BVH if built.
    fn set(&mut self, id: ColliderId, entry: ColliderEntry) -> bool {
        let slot = id.0 as usize;
        let Some(existing) = self.colliders.get(slot).and_then(|s| s.as_ref()) else {
            return false;
        };
        let is_trigger = existing.is_trigger;
        let new_aabb = entry.aabb();
        self.colliders[slot] = Some(ColliderSlot { entry, is_trigger });
        self.generation = self.generation.wrapping_add(1);

        // Try incremental refit.
        if let Some(ref mut bvh) = self.bvh
            && slot < self.bvh_slot_to_elem.len()
        {
            let elem_idx = self.bvh_slot_to_elem[slot] as usize;
            bvh.update_aabb(elem_idx, new_aabb);
        } else {
            // BVH not built or mapping stale — mark dirty.
            self.bvh = None;
            self.bvh_slot_to_elem.clear();
        }
        true
    }

    fn ensure_bvh(&mut self) {
        if self.bvh.is_none() {
            self.rebuild();
        }
    }

    /// Given BVH hits (AABB-level), find the closest exact hit using
    /// shape-level intersection.
    fn closest_hit(&self, ray: &Ray, bvh_hits: &[BvhHit]) -> Option<(ColliderId, ShapeHit)> {
        let mut best: Option<(f64, ColliderId, ShapeHit)> = None;
        for bvh_hit in bvh_hits {
            let idx = bvh_hit.element_id as usize;
            let Some(Some(slot)) = self.colliders.get(idx) else {
                continue;
            };
            let hit = match &slot.entry {
                ColliderEntry::Sphere(s) => query::ray_vs_sphere(ray, s),
                ColliderEntry::Box(b) => query::ray_vs_aabb(ray, &b.aabb()),
                ColliderEntry::Capsule(c) => query::ray_vs_capsule(ray, c),
            };
            if let Some(hit) = hit
                && hit.t < best.as_ref().map_or(f64::INFINITY, |&(t, _, _)| t)
            {
                best = Some((hit.t, ColliderId(bvh_hit.element_id), hit));
            }
        }
        best.map(|(_, id, hit)| (id, hit))
    }

    /// Given BVH hits, find the closest exact swept-sphere hit.
    fn closest_swept(
        &self,
        segment: &Segment,
        radius: f64,
        bvh_hits: &[BvhHit],
    ) -> Option<(ColliderId, ShapeHit)> {
        let mut best: Option<(f64, ColliderId, ShapeHit)> = None;
        for bvh_hit in bvh_hits {
            let idx = bvh_hit.element_id as usize;
            let Some(Some(slot)) = self.colliders.get(idx) else {
                continue;
            };
            let hit = match &slot.entry {
                ColliderEntry::Sphere(s) => query::swept_sphere_vs_sphere(segment, radius, s),
                ColliderEntry::Box(b) => query::swept_sphere_vs_aabb(segment, radius, &b.aabb()),
                ColliderEntry::Capsule(c) => query::swept_sphere_vs_capsule(segment, radius, c),
            };
            if let Some(hit) = hit
                && hit.t < best.as_ref().map_or(f64::INFINITY, |&(t, _, _)| t)
            {
                best = Some((hit.t, ColliderId(bvh_hit.element_id), hit));
            }
        }
        best.map(|(_, id, hit)| (id, hit))
    }
}

impl Default for PhysicsWorld {
    fn default() -> Self {
        Self::new()
    }
}

impl std::fmt::Debug for PhysicsWorld {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("PhysicsWorld")
            .field("collider_count", &self.len())
            .field("generation", &self.generation)
            .field("bvh_cached", &self.bvh.is_some())
            .finish()
    }
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn empty_world_is_empty() {
        let world = PhysicsWorld::new();
        assert!(world.is_empty());
        assert_eq!(world.len(), 0);
    }

    #[test]
    fn add_and_remove() {
        let mut world = PhysicsWorld::new();
        let id = world.add_sphere(Sphere::new(DVec3::ZERO, 1.0));
        assert_eq!(world.len(), 1);
        assert!(!world.is_empty());
        assert!(world.remove(id));
        assert!(world.is_empty());
    }

    #[test]
    fn remove_invalid_id_returns_false() {
        let mut world = PhysicsWorld::new();
        assert!(!world.remove(ColliderId(999)));
    }

    #[test]
    fn add_sphere_then_cast_ray_hits() {
        let mut world = PhysicsWorld::new();
        world.add_sphere(Sphere::new(DVec3::new(5.0, 0.0, 0.0), 1.0));
        let ray = Ray::new(DVec3::ZERO, DVec3::X);
        let result = world.cast_ray(&ray);
        assert!(result.is_some());
        let (_id, hit) = result.unwrap();
        assert!((hit.t - 4.0).abs() < 0.001);
    }

    #[test]
    fn cast_ray_misses_when_no_colliders() {
        let mut world = PhysicsWorld::new();
        let ray = Ray::new(DVec3::ZERO, DVec3::X);
        assert!(world.cast_ray(&ray).is_none());
    }

    #[test]
    fn add_multiple_and_cast_ray_finds_closest() {
        let mut world = PhysicsWorld::new();
        world.add_sphere(Sphere::new(DVec3::new(20.0, 0.0, 0.0), 1.0));
        let close = world.add_sphere(Sphere::new(DVec3::new(5.0, 0.0, 0.0), 1.0));
        let ray = Ray::new(DVec3::ZERO, DVec3::X);
        let (id, hit) = world.cast_ray(&ray).unwrap();
        assert_eq!(id, close);
        assert!((hit.t - 4.0).abs() < 0.001);
    }

    #[test]
    fn sweep_sphere_hits_box() {
        let mut world = PhysicsWorld::new();
        world.add_box(BoxCollider::new(DVec3::ZERO, DVec3::splat(1.0)));
        let seg = Segment::new(DVec3::new(-5.0, 0.0, 0.0), DVec3::new(5.0, 0.0, 0.0));
        let result = world.sweep_sphere(&seg, 0.5);
        assert!(result.is_some());
        let (_id, hit) = result.unwrap();
        // t should be (distance from start to inflated box near plane) / segment length
        // start=-5, sphere_r=0.5, box half=1.0, near=-1.5, distance=3.5, segment=10
        assert!(
            (hit.t - 0.35).abs() < 0.001,
            "expected ~0.35, got {}",
            hit.t
        );
        assert_eq!(hit.normal, DVec3::NEG_X);
    }

    #[test]
    fn rebuild_after_removal() {
        let mut world = PhysicsWorld::new();
        let id = world.add_sphere(Sphere::new(DVec3::new(5.0, 0.0, 0.0), 1.0));
        world.remove(id);
        world.rebuild(); // no panics
        let ray = Ray::new(DVec3::ZERO, DVec3::X);
        assert!(world.cast_ray(&ray).is_none());
    }

    #[test]
    fn aabb_of_returns_correct_bounds() {
        let mut world = PhysicsWorld::new();
        let id = world.add_sphere(Sphere::new(DVec3::new(1.0, 2.0, 3.0), 0.5));
        let aabb = world.aabb_of(id).unwrap();
        assert_eq!(aabb.min, DVec3::new(0.5, 1.5, 2.5));
        assert_eq!(aabb.max, DVec3::new(1.5, 2.5, 3.5));
    }

    #[test]
    fn aabb_of_unknown_id_returns_none() {
        let world = PhysicsWorld::new();
        assert!(world.aabb_of(ColliderId(42)).is_none());
    }

    #[test]
    fn generation_increments_on_mutation() {
        let mut world = PhysicsWorld::new();
        let gen0 = world.generation();
        let id = world.add_sphere(Sphere::new(DVec3::ZERO, 1.0));
        assert_ne!(world.generation(), gen0);
        world.remove(id);
        assert_ne!(world.generation(), gen0);
    }

    #[test]
    fn debug_format() {
        let mut world = PhysicsWorld::new();
        world.add_sphere(Sphere::new(DVec3::ZERO, 1.0));
        let s = format!("{world:?}");
        assert!(s.contains("collider_count: 1"));
    }

    // ── Overlap tests ─────────────────────────────────────────────────

    #[test]
    fn overlap_sphere_finds_sphere() {
        let mut world = PhysicsWorld::new();
        world.add_sphere(Sphere::new(DVec3::new(2.0, 0.0, 0.0), 1.0));
        let results = world.overlap_sphere(DVec3::ZERO, 4.0);
        assert_eq!(results.len(), 1);
    }

    #[test]
    fn overlap_sphere_misses_distant() {
        let mut world = PhysicsWorld::new();
        world.add_sphere(Sphere::new(DVec3::new(10.0, 0.0, 0.0), 1.0));
        let results = world.overlap_sphere(DVec3::ZERO, 2.0);
        assert!(results.is_empty());
    }

    #[test]
    fn overlap_sphere_finds_box() {
        let mut world = PhysicsWorld::new();
        world.add_box(BoxCollider::new(DVec3::ZERO, DVec3::splat(1.0)));
        let results = world.overlap_sphere(DVec3::new(2.0, 0.0, 0.0), 1.5);
        assert_eq!(results.len(), 1);
    }

    #[test]
    fn overlap_sphere_finds_capsule() {
        let mut world = PhysicsWorld::new();
        world.add_capsule(Capsule::new(DVec3::ZERO, 0.5, 2.0));
        let results = world.overlap_sphere(DVec3::new(0.0, 0.0, 0.0), 3.0);
        assert_eq!(results.len(), 1);
    }

    #[test]
    fn overlap_sphere_finds_multiple() {
        let mut world = PhysicsWorld::new();
        world.add_sphere(Sphere::new(DVec3::new(2.0, 0.0, 0.0), 1.0));
        world.add_sphere(Sphere::new(DVec3::new(-2.0, 0.0, 0.0), 1.0));
        world.add_sphere(Sphere::new(DVec3::new(0.0, 6.0, 0.0), 1.0));
        let results = world.overlap_sphere(DVec3::ZERO, 4.0);
        assert_eq!(results.len(), 2); // only the two on x-axis
    }

    #[test]
    fn overlap_aabb_finds_all_intersecting() {
        let mut world = PhysicsWorld::new();
        world.add_sphere(Sphere::new(DVec3::new(2.0, 0.0, 0.0), 1.0));
        world.add_sphere(Sphere::new(DVec3::new(-2.0, 0.0, 0.0), 1.0));
        world.add_sphere(Sphere::new(DVec3::new(0.0, 10.0, 0.0), 1.0));
        let query = Aabb::from_centre_half(DVec3::ZERO, DVec3::splat(3.0));
        let results = world.overlap_aabb(&query);
        assert_eq!(results.len(), 2);
    }

    #[test]
    fn overlap_aabb_empty_world() {
        let mut world = PhysicsWorld::new();
        let query = Aabb::from_centre_half(DVec3::ZERO, DVec3::splat(10.0));
        assert!(world.overlap_aabb(&query).is_empty());
    }

    // ── Dynamic update (refit) tests ────────────────────────────────────

    #[test]
    fn set_sphere_moves_collider_in_ray_cast() {
        let mut world = PhysicsWorld::new();
        let id = world.add_sphere(Sphere::new(DVec3::new(5.0, 0.0, 0.0), 1.0));
        // Initially hits.
        let ray = Ray::new(DVec3::ZERO, DVec3::X);
        assert!(world.cast_ray(&ray).is_some());

        // Move the sphere far away via set_sphere.
        assert!(world.set_sphere(id, Sphere::new(DVec3::new(-50.0, 0.0, 0.0), 1.0)));

        // Should no longer hit with +X ray.
        assert!(world.cast_ray(&ray).is_none());
    }

    #[test]
    fn set_box_updates_overlap_query() {
        let mut world = PhysicsWorld::new();
        let id = world.add_box(BoxCollider::new(
            DVec3::new(5.0, 0.0, 0.0),
            DVec3::splat(1.0),
        ));

        // Overlap near origin should not find it.
        let results = world.overlap_sphere(DVec3::ZERO, 2.0);
        assert!(results.is_empty());

        // Move it to origin.
        assert!(world.set_box(
            id,
            BoxCollider::new(DVec3::new(1.0, 0.0, 0.0), DVec3::splat(1.0))
        ));

        // Now overlap should find it.
        let results = world.overlap_sphere(DVec3::ZERO, 3.0);
        assert_eq!(results.len(), 1);
        assert_eq!(results[0], id);
    }

    #[test]
    fn set_sphere_on_invalid_id_returns_false() {
        let mut world = PhysicsWorld::new();
        assert!(!world.set_sphere(ColliderId(999), Sphere::new(DVec3::ZERO, 1.0)));
    }

    #[test]
    fn set_capsule_refits_bvh() {
        let mut world = PhysicsWorld::new();
        let id = world.add_capsule(Capsule::new(DVec3::new(5.0, 0.0, 0.0), 0.5, 1.0));
        let ray = Ray::new(DVec3::ZERO, DVec3::X);

        // Initially hits.
        assert!(world.cast_ray(&ray).is_some());

        // Move it.
        assert!(world.set_capsule(id, Capsule::new(DVec3::new(-30.0, 30.0, 0.0), 0.5, 1.0)));

        // Should miss with +X ray.
        assert!(world.cast_ray(&ray).is_none());
    }

    #[test]
    fn set_sphere_after_removal_fails() {
        let mut world = PhysicsWorld::new();
        let id = world.add_sphere(Sphere::new(DVec3::ZERO, 1.0));
        world.remove(id);
        assert!(!world.set_sphere(id, Sphere::new(DVec3::new(1.0, 0.0, 0.0), 1.0)));
    }

    #[test]
    fn sweep_sphere_hits_capsule_exact() {
        let mut world = PhysicsWorld::new();
        world.add_capsule(Capsule::new(DVec3::ZERO, 0.5, 2.0));
        let seg = Segment::new(DVec3::new(-5.0, 0.0, 0.0), DVec3::new(5.0, 0.0, 0.0));
        let result = world.sweep_sphere(&seg, 0.5);
        assert!(result.is_some());
    }

    #[test]
    fn sweep_sphere_misses_distant_capsule() {
        let mut world = PhysicsWorld::new();
        world.add_capsule(Capsule::new(DVec3::new(100.0, 0.0, 0.0), 0.5, 2.0));
        let seg = Segment::new(DVec3::new(-5.0, 0.0, 0.0), DVec3::new(5.0, 0.0, 0.0));
        assert!(world.sweep_sphere(&seg, 0.5).is_none());
    }

    // ── Trigger tests ──────────────────────────────────────────────────

    #[test]
    fn trigger_defaults_to_false() {
        let mut world = PhysicsWorld::new();
        let id = world.add_sphere(Sphere::new(DVec3::ZERO, 1.0));
        assert!(!world.is_trigger(id));
    }

    #[test]
    fn set_trigger_toggles_flag() {
        let mut world = PhysicsWorld::new();
        let id = world.add_sphere(Sphere::new(DVec3::ZERO, 1.0));
        assert!(world.set_trigger(id, true));
        assert!(world.is_trigger(id));
        assert!(world.set_trigger(id, false));
        assert!(!world.is_trigger(id));
    }

    #[test]
    fn set_trigger_on_invalid_id_returns_false() {
        let mut world = PhysicsWorld::new();
        assert!(!world.set_trigger(ColliderId(42), true));
    }
}
