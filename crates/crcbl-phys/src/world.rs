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
///
/// This is a *generational* id — a storage slot plus the generation that slot
/// was issued with — for the same reason [`crcbl_ecs::Entity`] is: removing a
/// collider recycles its slot, and a bare index kept across the removal would
/// silently start addressing whatever collider landed there next. A stale id
/// resolves to nothing instead.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct ColliderId {
    index: u32,
    generation: u32,
}

impl ColliderId {
    /// The storage slot this id names. Only meaningful together with the
    /// generation, so this stays crate-internal.
    #[inline]
    pub(crate) const fn index(self) -> u32 {
        self.index
    }

    /// Build an id from its parts. Crate-internal: only [`PhysicsWorld`] knows
    /// which generation a slot is currently on.
    #[inline]
    const fn new(index: u32, generation: u32) -> Self {
        Self { index, generation }
    }
}

// ---------------------------------------------------------------------------
// Collider shape storage
// ---------------------------------------------------------------------------

/// `bvh_slot_to_elem` entry for a collider slot with no element in the tree.
const NO_ELEMENT: u32 = u32::MAX;

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
    /// Generation counter per slot, parallel to `colliders`. Bumped when a
    /// slot is freed, so ids issued before the removal stop resolving.
    generations: Vec<u32>,
    /// Free slots for reuse (indices into `colliders`).
    free_slots: Vec<u32>,
    /// Live collider count, so `len` does not scan the slot array.
    live_count: usize,
    /// Lazily-rebuilt BVH. `None` when dirty.
    bvh: Option<Bvh>,
    /// Maps collider slot index → BVH element position (`u32::MAX` for slots
    /// with no element). Populated during [`PhysicsWorld::rebuild`]. Used by
    /// update methods for O(log n) refit.
    bvh_slot_to_elem: Vec<u32>,
}

impl PhysicsWorld {
    /// Create an empty physics world.
    #[must_use]
    pub fn new() -> Self {
        Self {
            colliders: Vec::new(),
            generations: Vec::new(),
            free_slots: Vec::new(),
            live_count: 0,
            bvh: None,
            bvh_slot_to_elem: Vec::new(),
        }
    }

    /// Register a sphere collider. Returns a [`ColliderId`] that can be used
    /// to remove or update the collider later.
    ///
    /// # Adding does not cost a rebuild
    ///
    /// Once the BVH exists, a new collider is *inserted* into it
    /// ([`Bvh::insert`]) rather than the tree being dropped and rebuilt on the
    /// next query. A game that spawns and kills colliders every tick — a
    /// bullet per shot, two rocks per split — would otherwise pay `O(n log n)`
    /// per frame for a tree it had already built, and pay it again for the
    /// removal.
    ///
    /// Before the first query there is no tree, and adds simply accumulate: a
    /// world populated in one batch still gets one bulk [`Bvh::build`], which
    /// produces a better tree than the same elements inserted one at a time.
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

    /// Mark a collider as a trigger.
    ///
    /// A trigger is *non-solid*: [`PhysicsWorld::cast_ray`] and
    /// [`PhysicsWorld::sweep_sphere`] pass straight through it, while
    /// [`PhysicsWorld::overlap_sphere`] and [`PhysicsWorld::overlap_aabb`]
    /// still report it — that is what makes it an overlap-only volume.
    ///
    /// Returns `false` if the id is invalid.
    pub fn set_trigger(&mut self, id: ColliderId, is_trigger: bool) -> bool {
        let Some(slot) = self.slot_of(id) else {
            return false;
        };
        if let Some(slot_data) = self.colliders[slot].as_mut() {
            slot_data.is_trigger = is_trigger;
        }
        true
    }

    /// Whether a collider is a trigger.
    pub fn is_trigger(&self, id: ColliderId) -> bool {
        self.slot_of(id)
            .is_some_and(|slot| self.colliders[slot].as_ref().is_some_and(|s| s.is_trigger))
    }

    /// Remove a collider by its id. Returns `true` if the id was valid.
    ///
    /// The slot is recycled, but its generation is bumped first, so `id` (and
    /// any copy of it) stops resolving even once a new collider lands there.
    ///
    /// If a BVH is built, the element is taken out of it incrementally rather
    /// than the tree being thrown away — see [`PhysicsWorld::add_sphere`] for
    /// why that matters.
    pub fn remove(&mut self, id: ColliderId) -> bool {
        let Some(slot) = self.slot_of(id) else {
            return false;
        };
        self.colliders[slot] = None;
        self.generations[slot] = self.generations[slot].wrapping_add(1);
        self.free_slots.push(slot as u32);
        self.live_count -= 1;

        if let Some(bvh) = self.bvh.as_mut() {
            match self.bvh_slot_to_elem.get(slot).copied() {
                Some(elem) if elem != NO_ELEMENT && bvh.remove(elem as usize) => {
                    self.bvh_slot_to_elem[slot] = NO_ELEMENT;
                }
                // A live collider with no element in a built tree should not
                // happen; if it ever does, the tree no longer describes the
                // collider set and querying it would report a ghost.
                _ => self.invalidate_bvh(),
            }
        }
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
        // Build slot→elem reverse mapping.  `elem_idx` is the element's
        // position in the array handed to `Bvh::build` — its *build order* —
        // which is exactly what `Bvh::update_aabb` is indexed by.
        self.bvh_slot_to_elem = vec![NO_ELEMENT; self.colliders.len()];
        for (elem_idx, (_, slot)) in elements.iter().enumerate() {
            self.bvh_slot_to_elem[*slot as usize] = elem_idx as u32;
        }
        self.bvh = Some(Bvh::build(elements));
    }

    /// Number of colliders currently registered.
    #[must_use]
    pub fn len(&self) -> usize {
        self.live_count
    }

    /// Whether the world has no colliders.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.live_count == 0
    }

    // ── Queries ────────────────────────────────────────────────────────

    /// Return all collider ids whose shape overlaps the query sphere.
    ///
    /// Uses the BVH for broadphase culling, then tests exact shape overlap
    /// (sphere-vs-sphere, sphere-vs-AABB, sphere-vs-capsule). Triggers are
    /// included — overlap is the query they exist for.
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
            .map(|slot| self.id_for_slot(slot))
            .collect()
    }

    /// Return all collider ids whose AABB intersects the query AABB.
    ///
    /// This is a broadphase-only query — it tests AABB-vs-AABB without
    /// exact shape overlap. Use [`PhysicsWorld::overlap_sphere`] for exact
    /// shape-aware overlap. Triggers are included.
    #[must_use]
    pub fn overlap_aabb(&mut self, aabb: &Aabb) -> Vec<ColliderId> {
        self.ensure_bvh();
        let bvh = self.bvh.as_ref().unwrap();
        let slots = bvh.traverse_aabb(aabb);
        slots
            .into_iter()
            .map(|slot| self.id_for_slot(slot))
            .collect()
    }

    /// Cast a ray against all colliders, returning the closest hit (if any).
    ///
    /// Results are tested against the exact shape geometry (not just the AABB).
    /// The returned `ColliderId` is the handle you got from `add_*`.
    ///
    /// Triggers are non-solid and are skipped — use
    /// [`PhysicsWorld::overlap_sphere`] to detect them.
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
    /// Triggers are non-solid and are skipped.
    ///
    /// # The broadphase query is the swept *volume*, not the centre line
    ///
    /// A sphere is `radius` wide, so the colliders it can touch are the ones
    /// within `radius` of the path its centre takes. Traversing the BVH with the
    /// centre line as a ray — which this did — offers the narrow phase only the
    /// colliders that line runs through, and every shape the sphere merely
    /// grazes is dropped before the exact test that would have caught it. The
    /// visible form of that: a ball rolling along a wall never touches it, and a
    /// ball approaching one is reported as hitting only once its centre is level
    /// with the surface, half a diameter inside.
    ///
    /// Traversing the segment's bounds inflated by `radius` offers a superset
    /// instead. The extra candidates cost one exact test each and are rejected
    /// by the same narrow phase as before.
    #[must_use]
    pub fn sweep_sphere(
        &mut self,
        segment: &Segment,
        radius: f64,
    ) -> Option<(ColliderId, ShapeHit)> {
        self.ensure_bvh();
        let bvh = self.bvh.as_ref().unwrap();
        let bounds = Aabb::new(
            segment.start.min(segment.end),
            segment.start.max(segment.end),
        )
        .inflated(radius);
        let candidates = bvh.traverse_aabb(&bounds);
        self.closest_swept(segment, radius, &candidates)
    }

    /// Get the AABB of a collider by id.
    #[must_use]
    pub fn aabb_of(&self, id: ColliderId) -> Option<Aabb> {
        let slot = self.slot_of(id)?;
        self.colliders[slot].as_ref().map(|s| s.entry.aabb())
    }

    // ── Internal helpers ───────────────────────────────────────────────

    /// Resolve an id to a live storage slot, or `None` if the id is stale
    /// (generation mismatch) or names an empty slot.
    fn slot_of(&self, id: ColliderId) -> Option<usize> {
        let slot = id.index as usize;
        if self.generations.get(slot).copied() != Some(id.generation) {
            return None;
        }
        self.colliders
            .get(slot)
            .and_then(|s| s.as_ref())
            .map(|_| slot)
    }

    /// The id currently naming `slot`. Only call with a slot the BVH just
    /// reported, i.e. one that is live.
    fn id_for_slot(&self, slot: u32) -> ColliderId {
        ColliderId::new(slot, self.generations[slot as usize])
    }

    fn add(&mut self, entry: ColliderEntry) -> ColliderId {
        let aabb = entry.aabb();
        let slot_data = ColliderSlot {
            entry,
            is_trigger: false,
        };
        let index = if let Some(slot) = self.free_slots.pop() {
            self.colliders[slot as usize] = Some(slot_data);
            slot
        } else {
            let idx = self.colliders.len() as u32;
            self.colliders.push(Some(slot_data));
            self.generations.push(0);
            idx
        };
        self.live_count += 1;

        // A tree that exists absorbs the new collider; one that does not stays
        // absent, so a batch of adds before the first query still costs one
        // bulk build rather than n insertions.
        if let Some(bvh) = self.bvh.as_mut() {
            let elem = bvh.insert(aabb, index);
            if self.bvh_slot_to_elem.len() <= index as usize {
                self.bvh_slot_to_elem.resize(index as usize + 1, NO_ELEMENT);
            }
            self.bvh_slot_to_elem[index as usize] = elem as u32;
        }
        self.id_for_slot(index)
    }

    /// Drop the BVH so the next query rebuilds it from the collider set.
    fn invalidate_bvh(&mut self) {
        self.bvh = None;
        self.bvh_slot_to_elem.clear();
    }

    /// Update an existing collider entry, refitting the BVH if built.
    fn set(&mut self, id: ColliderId, entry: ColliderEntry) -> bool {
        let Some(slot) = self.slot_of(id) else {
            return false;
        };
        let is_trigger = self.colliders[slot].as_ref().is_some_and(|s| s.is_trigger);
        let new_aabb = entry.aabb();
        self.colliders[slot] = Some(ColliderSlot { entry, is_trigger });

        // Try incremental refit.  A refit that reports failure leaves the tree
        // holding the old bounds, so the BVH must be dropped rather than
        // queried against stale geometry.
        let refit = match self.bvh {
            Some(ref mut bvh) => match self.bvh_slot_to_elem.get(slot).copied() {
                Some(elem) if elem != NO_ELEMENT => bvh.update_aabb(elem as usize, new_aabb),
                _ => false,
            },
            None => false,
        };
        if !refit {
            self.invalidate_bvh();
        }
        true
    }

    fn ensure_bvh(&mut self) {
        if self.bvh.is_none() {
            self.rebuild();
        }
    }

    /// Given BVH hits (AABB-level), find the closest exact hit using
    /// shape-level intersection. Triggers are non-solid and are skipped.
    fn closest_hit(&self, ray: &Ray, bvh_hits: &[BvhHit]) -> Option<(ColliderId, ShapeHit)> {
        let mut best: Option<(f64, ColliderId, ShapeHit)> = None;
        for bvh_hit in bvh_hits {
            let idx = bvh_hit.element_id as usize;
            let Some(Some(slot)) = self.colliders.get(idx) else {
                continue;
            };
            if slot.is_trigger {
                continue;
            }
            let hit = match &slot.entry {
                ColliderEntry::Sphere(s) => query::ray_vs_sphere(ray, s),
                ColliderEntry::Box(b) => query::ray_vs_aabb(ray, &b.aabb()),
                ColliderEntry::Capsule(c) => query::ray_vs_capsule(ray, c),
            };
            if let Some(hit) = hit
                && hit.t < best.as_ref().map_or(f64::INFINITY, |&(t, _, _)| t)
            {
                best = Some((hit.t, self.id_for_slot(bvh_hit.element_id), hit));
            }
        }
        best.map(|(_, id, hit)| (id, hit))
    }

    /// Given broadphase candidates, find the closest exact swept-sphere hit.
    /// Triggers are non-solid and are skipped.
    fn closest_swept(
        &self,
        segment: &Segment,
        radius: f64,
        candidates: &[u32],
    ) -> Option<(ColliderId, ShapeHit)> {
        let mut best: Option<(f64, ColliderId, ShapeHit)> = None;
        for &element in candidates {
            let idx = element as usize;
            let Some(Some(slot)) = self.colliders.get(idx) else {
                continue;
            };
            if slot.is_trigger {
                continue;
            }
            let hit = match &slot.entry {
                ColliderEntry::Sphere(s) => query::swept_sphere_vs_sphere(segment, radius, s),
                ColliderEntry::Box(b) => query::swept_sphere_vs_aabb(segment, radius, &b.aabb()),
                ColliderEntry::Capsule(c) => query::swept_sphere_vs_capsule(segment, radius, c),
            };
            if let Some(hit) = hit
                && hit.t < best.as_ref().map_or(f64::INFINITY, |&(t, _, _)| t)
            {
                best = Some((hit.t, self.id_for_slot(element), hit));
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
        assert!(!world.remove(ColliderId::new(999, 0)));
        // A double remove is the same stale-id case.
        let id = world.add_sphere(Sphere::new(DVec3::ZERO, 1.0));
        assert!(world.remove(id));
        assert!(!world.remove(id));
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

    /// A sphere whose *centre* never enters the box still touches it, and the
    /// sweep has to say so.
    ///
    /// The broadphase used to traverse the centre line as a ray, so a box the
    /// sphere overlaps by anything less than its radius was dropped before the
    /// exact test ran. Every case below is a real contact: the centre line
    /// passes 0.3 above a box whose top is at y = 0, with a sphere of radius
    /// 0.5, so the sphere is 0.2 deep at its closest.
    #[test]
    fn sweep_sphere_hits_a_box_its_centre_line_misses() {
        let mut world = PhysicsWorld::new();
        world.add_box(BoxCollider::new(
            DVec3::new(0.0, -1.0, 0.0),
            DVec3::splat(1.0),
        ));

        // Along the top face, never crossing it.
        let along = Segment::new(DVec3::new(-5.0, 0.3, 0.0), DVec3::new(5.0, 0.3, 0.0));
        let (_, hit) = world
            .sweep_sphere(&along, 0.5)
            .expect("a sphere grazing the top face touches it");
        assert!(hit.point.y <= 0.0 + 1e-9, "contact on the box: {hit:?}");

        // Coming down onto it and stopping short: the surfaces meet, the
        // centres do not.
        let onto = Segment::new(DVec3::new(0.0, 1.0, 0.0), DVec3::new(0.0, 0.3, 0.0));
        let (_, hit) = world
            .sweep_sphere(&onto, 0.5)
            .expect("a sphere landing on the face touches it");
        assert!(hit.normal.y > 0.5, "the top face's normal: {hit:?}");

        // And a genuine miss is still a miss: 0.6 clear of a 0.5 sphere.
        let clear = Segment::new(DVec3::new(-5.0, 0.6, 0.0), DVec3::new(5.0, 0.6, 0.0));
        assert!(
            world.sweep_sphere(&clear, 0.5).is_none(),
            "a sphere that does not reach the box must not report a hit",
        );
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
    fn churn_after_the_first_query_keeps_the_tree() {
        // The behaviour the asteroids sample turns on: once the tree exists,
        // spawning and killing colliders updates it in place. If either path
        // dropped the tree instead, `bvh_cached` would read false right after
        // the mutation and every frame would pay a rebuild.
        let mut world = PhysicsWorld::new();
        let keep = world.add_sphere(Sphere::new(DVec3::new(20.0, 0.0, 0.0), 1.0));
        assert!(world.cast_ray(&Ray::new(DVec3::ZERO, DVec3::X)).is_some());
        assert!(format!("{world:?}").contains("bvh_cached: true"));

        let bullet = world.add_sphere(Sphere::new(DVec3::new(5.0, 0.0, 0.0), 0.5));
        assert!(
            format!("{world:?}").contains("bvh_cached: true"),
            "adding a collider threw the tree away"
        );
        // The new collider is in the tree, not merely in the slot array: the
        // ray now stops at it rather than at the one it used to reach.
        let (hit_id, _) = world.cast_ray(&Ray::new(DVec3::ZERO, DVec3::X)).unwrap();
        assert_eq!(hit_id, bullet);

        assert!(world.remove(bullet));
        assert!(
            format!("{world:?}").contains("bvh_cached: true"),
            "removing a collider threw the tree away"
        );
        let (hit_id, _) = world.cast_ray(&Ray::new(DVec3::ZERO, DVec3::X)).unwrap();
        assert_eq!(hit_id, keep, "the removed collider still answers queries");
        assert_eq!(world.len(), 1);
    }

    #[test]
    fn churn_through_a_recycled_slot_tracks_the_right_element() {
        // Collider slots and BVH element indices are two independent recycling
        // schemes. If the mapping between them went stale, the new occupant of
        // a slot would refit the *old* occupant's leaf — a collider that moves
        // when a different one is moved.
        let mut world = PhysicsWorld::new();
        let far = world.add_sphere(Sphere::new(DVec3::new(100.0, 0.0, 0.0), 1.0));
        let doomed = world.add_sphere(Sphere::new(DVec3::new(10.0, 0.0, 0.0), 1.0));
        assert!(world.cast_ray(&Ray::new(DVec3::ZERO, DVec3::X)).is_some());

        assert!(world.remove(doomed));
        let reused = world.add_sphere(Sphere::new(DVec3::new(30.0, 0.0, 0.0), 1.0));
        assert_eq!(reused.index(), doomed.index(), "the slot must be recycled");

        // Move the new occupant, then ask a *local* question about where it
        // landed. A local query is the one that can tell: it only descends
        // into leaves whose bounds reach the query, so a refit applied to some
        // other collider's leaf leaves this one still sitting at its old
        // bounds and out of reach. A long ray would not catch it — the ray
        // would pass through the stale bounds anyway and the narrow phase
        // would re-read the correct shape and paper over the mistake.
        assert!(world.set_sphere(reused, Sphere::new(DVec3::new(3.0, 0.0, 0.0), 1.0)));
        assert_eq!(
            world.overlap_sphere(DVec3::new(3.0, 0.0, 0.0), 0.5),
            vec![reused],
            "the refit did not move the collider that was asked to move"
        );
        // And nothing else moved with it.
        assert!(
            world
                .overlap_sphere(DVec3::new(30.0, 0.0, 0.0), 0.5)
                .is_empty()
        );
        assert_eq!(
            world.aabb_of(far).unwrap().centre(),
            DVec3::new(100.0, 0.0, 0.0)
        );
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
        assert!(world.aabb_of(ColliderId::new(42, 0)).is_none());
    }

    #[test]
    fn stale_id_does_not_address_the_slot_s_new_occupant() {
        // ABA: remove a collider, add another that recycles the same slot, and
        // check the retained id resolves to nothing rather than to the new
        // occupant.
        let mut world = PhysicsWorld::new();
        let old = world.add_sphere(Sphere::new(DVec3::ZERO, 1.0));
        assert!(world.remove(old));

        let new = world.add_sphere(Sphere::new(DVec3::new(9.0, 0.0, 0.0), 2.0));
        assert_eq!(new.index(), old.index(), "the slot must be recycled");
        assert_ne!(new, old, "but the id must not be");

        assert!(world.aabb_of(old).is_none());
        assert!(!world.remove(old));
        assert!(!world.set_trigger(old, true));
        assert!(!world.is_trigger(old));
        assert!(!world.set_sphere(old, Sphere::new(DVec3::new(-50.0, 0.0, 0.0), 1.0)));

        // The new occupant is untouched by any of the above.
        let aabb = world.aabb_of(new).unwrap();
        assert_eq!(aabb.centre(), DVec3::new(9.0, 0.0, 0.0));
        assert_eq!(world.len(), 1);
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
        assert!(!world.set_sphere(ColliderId::new(999, 0), Sphere::new(DVec3::ZERO, 1.0)));
    }

    #[test]
    fn set_sphere_refits_the_right_leaf_when_colliders_are_not_centroid_ordered() {
        // The BVH sorts each range by centroid while building, so the leaf
        // order differs from the order colliders were added.  Moving slot 0
        // must move the collider at x = 20, not the one at x = 5.
        let mut world = PhysicsWorld::new();
        let far = world.add_sphere(Sphere::new(DVec3::new(20.0, 0.0, 0.0), 1.0));
        let near = world.add_sphere(Sphere::new(DVec3::new(5.0, 0.0, 0.0), 1.0));

        let ray = Ray::new(DVec3::ZERO, DVec3::X);
        let (id, hit) = world
            .cast_ray(&ray)
            .expect("the near sphere is ahead of the ray");
        assert_eq!(id, near);
        assert!((hit.t - 4.0).abs() < 1e-9, "t = {}", hit.t);

        // Move the *far* sphere behind the ray origin.  The near one must
        // still be found, at the same distance.
        assert!(world.set_sphere(far, Sphere::new(DVec3::new(-50.0, 0.0, 0.0), 1.0)));
        let (id, hit) = world
            .cast_ray(&ray)
            .expect("the untouched sphere must not vanish");
        assert_eq!(id, near);
        assert!((hit.t - 4.0).abs() < 1e-9, "t = {}", hit.t);
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
        assert!(!world.set_trigger(ColliderId::new(42, 0), true));
    }

    #[test]
    fn trigger_is_transparent_to_rays_and_sweeps() {
        let mut world = PhysicsWorld::new();
        let trigger = world.add_sphere(Sphere::new(DVec3::new(5.0, 0.0, 0.0), 1.0));
        assert!(world.set_trigger(trigger, true));

        let ray = Ray::new(DVec3::ZERO, DVec3::X);
        assert!(
            world.cast_ray(&ray).is_none(),
            "a trigger must not stop a bullet"
        );

        let seg = Segment::new(DVec3::ZERO, DVec3::new(10.0, 0.0, 0.0));
        assert!(world.sweep_sphere(&seg, 0.5).is_none());
    }

    #[test]
    fn trigger_still_shows_up_in_overlap_queries() {
        let mut world = PhysicsWorld::new();
        let trigger = world.add_sphere(Sphere::new(DVec3::new(2.0, 0.0, 0.0), 1.0));
        assert!(world.set_trigger(trigger, true));

        assert_eq!(world.overlap_sphere(DVec3::ZERO, 4.0), vec![trigger]);
        let query = Aabb::from_centre_half(DVec3::ZERO, DVec3::splat(4.0));
        assert_eq!(world.overlap_aabb(&query), vec![trigger]);
    }

    #[test]
    fn solid_collider_behind_a_trigger_is_still_hit() {
        let mut world = PhysicsWorld::new();
        let trigger = world.add_sphere(Sphere::new(DVec3::new(5.0, 0.0, 0.0), 1.0));
        world.set_trigger(trigger, true);
        let solid = world.add_sphere(Sphere::new(DVec3::new(20.0, 0.0, 0.0), 1.0));

        let ray = Ray::new(DVec3::ZERO, DVec3::X);
        let (id, hit) = world.cast_ray(&ray).expect("the solid sphere is behind it");
        assert_eq!(id, solid);
        assert!((hit.t - 19.0).abs() < 1e-9, "t = {}", hit.t);
    }
}
