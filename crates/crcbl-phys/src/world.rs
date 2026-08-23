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
    /// The storage slot this id names.
    ///
    /// A small integer to subscript a caller's own per-collider array with,
    /// which is what a hot loop wants where a `HashMap<ColliderId, _>` would
    /// otherwise stand: turning a query result back into "which of my bodies
    /// is this" costs an array index rather than a hash.
    ///
    /// What the number does and does not promise:
    ///
    /// - **Bounded** by the number of slots the world has ever allocated. That
    ///   equals [`PhysicsWorld::len`] only while nothing has been removed —
    ///   afterwards the slot count stays where it was and `len` drops below it,
    ///   so an array sized from `len` is too short.
    /// - **Stable** for as long as the collider lives. Nothing compacts the
    ///   slot array, so a collider keeps its slot however many others are added
    ///   or removed around it.
    /// - **Not dense** once anything has been removed: the freed slot is a hole
    ///   until a later `add` recycles it.
    /// - **Not an identity.** A recycled slot names a different collider, and
    ///   only the generation the id also carries tells the two apart. Comparing
    ///   slots is not comparing colliders; compare the ids themselves.
    #[inline]
    #[must_use]
    pub const fn index(self) -> u32 {
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

/// A reading of the broadphase, from [`PhysicsWorld::broadphase_stats`].
///
/// Two kinds of number, read two different ways.
///
/// **The shape, as it is right now:** [`elements`](Self::elements),
/// [`nodes`](Self::nodes) and [`depth`](Self::depth). These are what let a
/// structural policy (a teleport rule, a rebuild cadence) be argued from
/// measurement — depth is the query-cost bound, node count the footprint
/// including recycled slots.
///
/// **The totals, counting up from the moment the world was created and never
/// reset:** [`refits`](Self::refits),
/// [`updates_without_refit`](Self::updates_without_refit) and
/// [`rebuilds`](Self::rebuilds). There is no reset call, deliberately: take a
/// reading either side of a phase and subtract, and the phase's own numbers
/// come out exactly, with no second observer able to zero the counters halfway
/// through somebody else's measurement.
///
/// Taking the reading is itself an event the totals see.
/// [`PhysicsWorld::broadphase_stats`] forces the lazy rebuild, so a reading
/// taken while the tree is dirty reports a `rebuilds` that includes the one it
/// just caused — which is the honest number, since that rebuild happened.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct BroadphaseStats {
    /// Live colliders (leaves) in the tree.
    pub elements: usize,
    /// Allocated node slots, including those a `remove` recycled.
    pub nodes: usize,
    /// Longest root-to-leaf path, counted in nodes ([`Bvh::depth`]'s units).
    pub depth: usize,
    /// Collider updates ([`PhysicsWorld::set_sphere`] and its siblings) that a
    /// built tree absorbed in place, refitting one root-to-leaf path.
    ///
    /// How far the collider moved does not enter into it. [`Bvh::update_aabb`]
    /// never refuses a new box for being far from the old one — it grows the
    /// ancestors it walks back through — so a body that teleports across the
    /// world is counted here too, and pays for the trip in a looser tree rather
    /// than in a rebuild.
    pub refits: u64,
    /// Collider updates that did not refit, leaving the BVH absent so that the
    /// next query rebuilds it.
    ///
    /// An update refits whenever the id resolves *and* the tree both exists and
    /// still holds an element for that slot, so what lands here is an update
    /// made with no tree to refit: before the first query, or after something
    /// else already dropped the tree.
    ///
    /// Counted per update, not per rebuild — several of these before the next
    /// query cost one rebuild between them. With [`refits`](Self::refits) it
    /// accounts for every `set_*` call that resolved to a live collider; a call
    /// on a stale id is counted by neither.
    pub updates_without_refit: u64,
    /// Full [`Bvh::build`]s performed, whether asked for by
    /// [`PhysicsWorld::rebuild`] or forced by a query finding the tree absent.
    ///
    /// The number that says which of the two a phase's timing actually
    /// measured, and the reason the other two totals are here at all.
    pub rebuilds: u64,
}

/// The running totals [`BroadphaseStats`] hands out.
///
/// Separate from the stats struct because these are the world's own state,
/// while `BroadphaseStats` is a reading of it taken alongside the tree's shape.
#[derive(Debug, Clone, Copy, Default)]
struct BroadphaseCounters {
    refits: u64,
    updates_without_refit: u64,
    rebuilds: u64,
}

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
    /// Refit and rebuild totals, handed out by
    /// [`PhysicsWorld::broadphase_stats`]. Never reset.
    counters: BroadphaseCounters,
    /// Maps collider slot index → BVH element position (`u32::MAX` for slots
    /// with no element). Populated during [`PhysicsWorld::rebuild`]. Used by
    /// update methods for O(log n) refit.
    bvh_slot_to_elem: Vec<u32>,
    /// The buffers an overlap query works in, kept between calls.
    ///
    /// None of them outlives the call, so allocating them per call is a cost a
    /// caller running one query per body per tick pays for nothing. They live
    /// here rather than being passed in because `&mut self` query methods
    /// already take the exclusive borrow that makes one shared set sound —
    /// [`OverlapQueries`] is the shape for callers who cannot, and it asks for
    /// a [`QueryScratch`] of their own instead.
    scratch: QueryScratch,
}

/// The buffers one overlap query works in, owned by whoever runs the query.
///
/// [`OverlapQueries::overlap_sphere_into`] takes `&self`, so it cannot reach
/// the world's own buffers — several threads running queries at once would be
/// writing the same ones. One of these per thread is what replaces them, and it
/// is reused across calls exactly as the world's are: the contents are cleared
/// on entry and mean nothing between calls, so a scratch buffer never changes
/// an answer, only what a query has to allocate to reach it.
#[derive(Debug, Default)]
pub struct QueryScratch {
    /// The BVH descent stack.
    stack: Vec<u32>,
    /// The elements the descent turned up, before the exact shape test.
    candidates: Vec<u32>,
    /// The leaves a ray descent turned up. Separate from `candidates` because
    /// a ray leaf carries the AABB entry distance alongside its element id.
    ray_hits: Vec<BvhHit>,
    /// The collider ids the exact test kept, for a caller mapping them onto
    /// something of its own — [`crate::system::EntityOverlapQueries`] maps them
    /// to entities, which is why this is reachable from that module and from
    /// nowhere outside the crate.
    pub(crate) ids: Vec<ColliderId>,
}

impl QueryScratch {
    /// Empty buffers, which the first query grows.
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }
}

/// Read-only overlap queries against a world whose broadphase is already built.
///
/// **The type is the proof, not a comment.** A query needs the BVH to be
/// current, and building it needs `&mut PhysicsWorld` — so
/// [`PhysicsWorld::overlap_queries`] takes that exclusive borrow, builds the
/// tree, and hands back a shared borrow of the world in its place. A caller
/// cannot reach these queries without the tree being current and cannot mutate
/// the world while one is outstanding, which is what makes the query itself
/// `&self` and therefore `Sync`.
///
/// That is the whole point of it: a data-parallel pass over a crowd hands the
/// same view to every chunk and gives each chunk a [`QueryScratch`] of its own.
/// A caller that is not doing that wants
/// [`PhysicsWorld::overlap_sphere_into`], which keeps the scratch itself.
#[derive(Clone, Copy, Debug)]
pub struct OverlapQueries<'a> {
    colliders: &'a [Option<ColliderSlot>],
    generations: &'a [u32],
    bvh: &'a Bvh,
}

impl OverlapQueries<'_> {
    /// [`PhysicsWorld::overlap_sphere_into`] under a shared borrow, working in
    /// `scratch` instead of the world's own buffers.
    ///
    /// `out` is cleared and then filled. The results are identical to the
    /// `&mut self` form's — same tree, same traversal order, same exact tests —
    /// because the two are one implementation and not two that agree;
    /// `crcbl_phys::world::tests::the_shared_view_finds_exactly_what_a_scan_would`
    /// is what checks that implementation against a scan.
    pub fn overlap_sphere_into(
        &self,
        centre: DVec3,
        radius: f64,
        scratch: &mut QueryScratch,
        out: &mut Vec<ColliderId>,
    ) {
        overlap_sphere_core(
            self.bvh,
            self.colliders,
            self.generations,
            centre,
            radius,
            scratch,
            out,
        );
    }

    /// [`PhysicsWorld::overlap_aabb`] under a shared borrow, working in
    /// `scratch` and writing into a buffer the caller owns.
    ///
    /// `out` is cleared and then filled, with the same ids in the same tree
    /// order the `&mut self` form produces, because the two are one
    /// implementation and not two that agree.
    pub fn overlap_aabb_into(
        &self,
        aabb: &Aabb,
        scratch: &mut QueryScratch,
        out: &mut Vec<ColliderId>,
    ) {
        overlap_aabb_core(self.bvh, self.generations, aabb, scratch, out);
    }

    /// [`PhysicsWorld::cast_ray`] under a shared borrow, working in `scratch`
    /// instead of the world's own buffers.
    ///
    /// Triggers are skipped and the closest exact hit wins, exactly as in the
    /// `&mut self` form, because the two are one implementation.
    #[must_use]
    pub fn cast_ray(
        &self,
        ray: &Ray,
        scratch: &mut QueryScratch,
    ) -> Option<(ColliderId, ShapeHit)> {
        cast_ray_core(self.bvh, self.colliders, self.generations, ray, scratch)
    }

    /// [`PhysicsWorld::sweep_sphere`] under a shared borrow, working in
    /// `scratch` instead of the world's own buffers.
    #[must_use]
    pub fn sweep_sphere(
        &self,
        segment: &Segment,
        radius: f64,
        scratch: &mut QueryScratch,
    ) -> Option<(ColliderId, ShapeHit)> {
        self.sweep_sphere_excluding(segment, radius, None, scratch)
    }

    /// [`PhysicsWorld::sweep_sphere_excluding`] under a shared borrow, working
    /// in `scratch` instead of the world's own buffers.
    ///
    /// This is the form a body sweeping its own path needs — see the `&mut
    /// self` method for why the exclusion happens in the narrow phase and not
    /// after the fact. The two forms are one implementation.
    #[must_use]
    pub fn sweep_sphere_excluding(
        &self,
        segment: &Segment,
        radius: f64,
        exclude: Option<ColliderId>,
        scratch: &mut QueryScratch,
    ) -> Option<(ColliderId, ShapeHit)> {
        sweep_sphere_core(
            self.bvh,
            self.colliders,
            self.generations,
            segment,
            radius,
            exclude,
            scratch,
        )
    }
}

/// The one implementation of "which colliders overlap this sphere".
///
/// Both the `&mut self` form and [`OverlapQueries`] come through here, so there
/// is no second copy of the traversal for the two to disagree in — which
/// matters more than usual, because a caller mixing the two in one pass is
/// exactly what a `par_for` adoption looks like mid-migration.
///
/// # Shape-aware: `radius` is expanded by each collider's own shape
///
/// The query sphere is tested against every collider's *shape* (see
/// `query::sphere_overlaps_*`), not against its centre. A sphere collider of
/// radius `r_b` is therefore returned iff its centre is within
/// `radius + r_b` of `centre`, which is what makes `apps/horde`'s
/// `separation_query_radius` correct without adding the neighbour's radius to
/// the query. `tests::a_sphere_overlap_is_expanded_by_the_colliders_own_radius`
/// pins the boundary.
fn overlap_sphere_core(
    bvh: &Bvh,
    colliders: &[Option<ColliderSlot>],
    generations: &[u32],
    centre: DVec3,
    radius: f64,
    scratch: &mut QueryScratch,
    out: &mut Vec<ColliderId>,
) {
    out.clear();
    let query_aabb = Aabb::from_centre_half(centre, DVec3::splat(radius));
    let query_sphere = Sphere::new(centre, radius);

    bvh.traverse_aabb_into(&query_aabb, &mut scratch.stack, &mut scratch.candidates);

    for &idx in scratch.candidates.iter() {
        let slot = idx as usize;
        let hit = colliders
            .get(slot)
            .and_then(|s| s.as_ref())
            .is_some_and(|slot| match &slot.entry {
                ColliderEntry::Sphere(s) => query::sphere_overlaps_sphere(&query_sphere, s),
                ColliderEntry::Box(b) => query::sphere_overlaps_aabb(&query_sphere, &b.aabb()),
                ColliderEntry::Capsule(c) => query::sphere_overlaps_capsule(&query_sphere, c),
            });
        if hit {
            out.push(ColliderId::new(idx, generations[slot]));
        }
    }
}

/// The one implementation of "which colliders' AABBs meet this AABB".
///
/// Broadphase-only by design — the BVH's leaves *are* the collider AABBs, so
/// there is nothing to refine. Both [`PhysicsWorld::overlap_aabb`] and
/// [`OverlapQueries::overlap_aabb_into`] come through here.
fn overlap_aabb_core(
    bvh: &Bvh,
    generations: &[u32],
    aabb: &Aabb,
    scratch: &mut QueryScratch,
    out: &mut Vec<ColliderId>,
) {
    bvh.traverse_aabb_into(aabb, &mut scratch.stack, &mut scratch.candidates);
    out.clear();
    out.extend(
        scratch
            .candidates
            .iter()
            .map(|&slot| id_for_slot_in(generations, slot)),
    );
}

/// The one implementation of "what does this ray hit first".
///
/// Both [`PhysicsWorld::cast_ray`] and [`OverlapQueries::cast_ray`] come
/// through here. Triggers are non-solid and are skipped by `closest_hit_core`.
fn cast_ray_core(
    bvh: &Bvh,
    colliders: &[Option<ColliderSlot>],
    generations: &[u32],
    ray: &Ray,
    scratch: &mut QueryScratch,
) -> Option<(ColliderId, ShapeHit)> {
    // Out of the scratch and back into it, because the descent borrows the
    // stack at the same time and the two are fields of one struct.
    let mut hits = core::mem::take(&mut scratch.ray_hits);
    bvh.traverse_ray_into(ray, &mut scratch.stack, &mut hits);
    let best = closest_hit_core(colliders, generations, ray, &hits);
    scratch.ray_hits = hits;
    best
}

/// The one implementation of "what does this swept sphere hit first".
///
/// Both [`PhysicsWorld::sweep_sphere_excluding`] and
/// [`OverlapQueries::sweep_sphere_excluding`] come through here — and so do
/// the two `sweep_sphere` forms, which are this with no exclusion.
///
/// The broadphase query is the swept *volume* and not the centre line; see
/// [`PhysicsWorld::sweep_sphere`] for what that costs and what it fixes.
fn sweep_sphere_core(
    bvh: &Bvh,
    colliders: &[Option<ColliderSlot>],
    generations: &[u32],
    segment: &Segment,
    radius: f64,
    exclude: Option<ColliderId>,
    scratch: &mut QueryScratch,
) -> Option<(ColliderId, ShapeHit)> {
    let skip = exclude.and_then(|id| slot_of_in(colliders, generations, id));
    let bounds = Aabb::new(
        segment.start.min(segment.end),
        segment.start.max(segment.end),
    )
    .inflated(radius);
    bvh.traverse_aabb_into(&bounds, &mut scratch.stack, &mut scratch.candidates);
    closest_swept_core(
        colliders,
        generations,
        segment,
        radius,
        &scratch.candidates,
        skip,
    )
}

/// Given BVH hits (AABB-level), find the closest exact hit using shape-level
/// intersection. Triggers are non-solid and are skipped.
fn closest_hit_core(
    colliders: &[Option<ColliderSlot>],
    generations: &[u32],
    ray: &Ray,
    bvh_hits: &[BvhHit],
) -> Option<(ColliderId, ShapeHit)> {
    let mut best: Option<(f64, ColliderId, ShapeHit)> = None;
    for bvh_hit in bvh_hits {
        let idx = bvh_hit.element_id as usize;
        let Some(Some(slot)) = colliders.get(idx) else {
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
            best = Some((hit.t, id_for_slot_in(generations, bvh_hit.element_id), hit));
        }
    }
    best.map(|(_, id, hit)| (id, hit))
}

/// Given broadphase candidates, find the closest exact swept-sphere hit.
/// Triggers are non-solid and are skipped, and so is `skip` — the storage slot
/// of the collider the caller excluded, if any.
fn closest_swept_core(
    colliders: &[Option<ColliderSlot>],
    generations: &[u32],
    segment: &Segment,
    radius: f64,
    candidates: &[u32],
    skip: Option<usize>,
) -> Option<(ColliderId, ShapeHit)> {
    let mut best: Option<(f64, ColliderId, ShapeHit)> = None;
    for &element in candidates {
        let idx = element as usize;
        if Some(idx) == skip {
            continue;
        }
        let Some(Some(slot)) = colliders.get(idx) else {
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
            best = Some((hit.t, id_for_slot_in(generations, element), hit));
        }
    }
    best.map(|(_, id, hit)| (id, hit))
}

/// Resolve an id to a live storage slot, or `None` if the id is stale
/// (generation mismatch) or names an empty slot.
///
/// The free form of [`PhysicsWorld::slot_of`], which is what
/// `sweep_sphere_core` needs: a shared view has the two arrays but not the
/// world.
fn slot_of_in(
    colliders: &[Option<ColliderSlot>],
    generations: &[u32],
    id: ColliderId,
) -> Option<usize> {
    let slot = id.index as usize;
    if generations.get(slot).copied() != Some(id.generation) {
        return None;
    }
    colliders.get(slot).and_then(|s| s.as_ref()).map(|_| slot)
}

/// The id currently naming `slot`. Only call with a slot the BVH just
/// reported, i.e. one that is live.
fn id_for_slot_in(generations: &[u32], slot: u32) -> ColliderId {
    ColliderId::new(slot, generations[slot as usize])
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
            counters: BroadphaseCounters::default(),
            bvh_slot_to_elem: Vec::new(),
            scratch: QueryScratch::new(),
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
        self.counters.rebuilds += 1;
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
        let mut out = Vec::new();
        self.overlap_sphere_into(centre, radius, &mut out);
        out
    }

    /// [`overlap_sphere`](Self::overlap_sphere) writing into a buffer the
    /// caller owns.
    ///
    /// `out` is cleared and then filled. The intermediate buffers — the BVH
    /// descent stack and its candidate list — are the world's own and are
    /// reused between calls, so a caller that hoists one `out` out of its loop
    /// runs the whole pass without allocating.
    pub fn overlap_sphere_into(&mut self, centre: DVec3, radius: f64, out: &mut Vec<ColliderId>) {
        // Lent to the view and put straight back, which is what lets one
        // implementation serve both borrow shapes: the view cannot reach a
        // field of the world it is only sharing.
        let mut scratch = core::mem::take(&mut self.scratch);
        self.overlap_queries()
            .overlap_sphere_into(centre, radius, &mut scratch, out);
        self.scratch = scratch;
    }

    /// Builds the broadphase and hands back a shared view that can query it.
    ///
    /// The exchange this makes is the point: it costs the caller's exclusive
    /// borrow *once*, and buys a [`Sync`] query it can run from several threads
    /// at a time — see [`OverlapQueries`] for why the type is what enforces
    /// that rather than a rule in a comment.
    pub fn overlap_queries(&mut self) -> OverlapQueries<'_> {
        self.ensure_bvh();
        OverlapQueries {
            colliders: &self.colliders,
            generations: &self.generations,
            bvh: self.bvh.as_ref().expect("ensure_bvh built it"),
        }
    }

    /// Return all collider ids whose AABB intersects the query AABB.
    ///
    /// This is a broadphase-only query — it tests AABB-vs-AABB without
    /// exact shape overlap. Use [`PhysicsWorld::overlap_sphere`] for exact
    /// shape-aware overlap. Triggers are included.
    #[must_use]
    pub fn overlap_aabb(&mut self, aabb: &Aabb) -> Vec<ColliderId> {
        let mut scratch = core::mem::take(&mut self.scratch);
        let mut out = Vec::new();
        self.overlap_queries()
            .overlap_aabb_into(aabb, &mut scratch, &mut out);
        self.scratch = scratch;
        out
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
        let mut scratch = core::mem::take(&mut self.scratch);
        let hit = self.overlap_queries().cast_ray(ray, &mut scratch);
        self.scratch = scratch;
        hit
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
        self.sweep_sphere_excluding(segment, radius, None)
    }

    /// [`sweep_sphere`](Self::sweep_sphere) with one collider left out of the
    /// answer.
    ///
    /// This is the query a *body* sweeping its own path needs: the segment ends
    /// where the body is, so the body's own shape is sitting on the far end of
    /// it and is reported as the closest hit at `t = 0`. Excluding it here — and
    /// not by discarding the result afterwards — is what keeps the *next*
    /// closest hit: the narrow phase picks a single winner, so a caller that
    /// throws away a self-hit throws away the wall behind it too.
    ///
    /// A stale or invalid `exclude` excludes nothing, which is the same answer
    /// as `None`.
    ///
    /// # One id, not a filter object
    ///
    /// The field's general form is a filter the query calls back into — PhysX's
    /// `PxQueryFilterData`/`PxQueryFilterCallback`, Jolt's `BodyFilter`,
    /// Bullet's `ClosestRayResultCallback` and its mask. Every one of them also
    /// ships the degenerate case as its own thing, because it is the case that
    /// actually comes up: Jolt has `IgnoreSingleBodyFilter`, and Bullet's
    /// character controller carries a `ClosestNotMeConvexResultCallback`. That
    /// degenerate case is the whole of what this crate's consumers ask for, so
    /// it is the whole of what this takes.
    #[must_use]
    pub fn sweep_sphere_excluding(
        &mut self,
        segment: &Segment,
        radius: f64,
        exclude: Option<ColliderId>,
    ) -> Option<(ColliderId, ShapeHit)> {
        let mut scratch = core::mem::take(&mut self.scratch);
        let hit =
            self.overlap_queries()
                .sweep_sphere_excluding(segment, radius, exclude, &mut scratch);
        self.scratch = scratch;
        hit
    }

    /// Get the AABB of a collider by id.
    #[must_use]
    pub fn aabb_of(&self, id: ColliderId) -> Option<Aabb> {
        let slot = self.slot_of(id)?;
        self.colliders[slot].as_ref().map(|s| s.entry.aabb())
    }

    /// The broadphase's shape and its refit/rebuild totals, as a reading a
    /// game can measure a policy against.
    ///
    /// **For diagnostics, not for the frame path.** It forces the lazy rebuild
    /// if the tree is dirty, and `Bvh::depth` walks the whole tree — `O(n)` —
    /// so this is what a teleport rule's cost/benefit is argued from, the way
    /// `docs/backlog.md`'s "A consumer cannot see the cost it is being asked to
    /// avoid" asked for, and nothing a hot loop should call.
    ///
    /// The forced rebuild is counted: call this on a dirty tree and the
    /// [`rebuilds`](BroadphaseStats::rebuilds) it returns includes that one.
    /// A caller subtracting two readings to measure a phase therefore sees the
    /// phase it asked about *plus* whatever the first reading itself forced,
    /// which is why the first reading is best taken with the tree already
    /// built.
    #[must_use]
    pub fn broadphase_stats(&mut self) -> BroadphaseStats {
        self.ensure_bvh();
        let bvh = self.bvh.as_ref().expect("ensure_bvh built it");
        BroadphaseStats {
            elements: bvh.len(),
            nodes: bvh.node_count(),
            depth: bvh.depth(),
            refits: self.counters.refits,
            updates_without_refit: self.counters.updates_without_refit,
            rebuilds: self.counters.rebuilds,
        }
    }

    // ── Internal helpers ───────────────────────────────────────────────

    /// Resolve an id to a live storage slot, or `None` if the id is stale
    /// (generation mismatch) or names an empty slot.
    fn slot_of(&self, id: ColliderId) -> Option<usize> {
        slot_of_in(&self.colliders, &self.generations, id)
    }

    /// The id currently naming `slot`. Only call with a slot the BVH just
    /// reported, i.e. one that is live.
    fn id_for_slot(&self, slot: u32) -> ColliderId {
        id_for_slot_in(&self.generations, slot)
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
        if refit {
            self.counters.refits += 1;
        } else {
            self.counters.updates_without_refit += 1;
            self.invalidate_bvh();
        }
        true
    }

    fn ensure_bvh(&mut self) {
        if self.bvh.is_none() {
            self.rebuild();
        }
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

    /// **The stats a teleport rule would measure with.** `broadphase_stats`
    /// forces the lazy rebuild and reports the tree's shape: one entry per
    /// live collider, at least one node per leaf, and a depth that exists for
    /// a non-empty tree. The exact node count and depth are the AVL balance's
    /// business — what is pinned is that the observable exists and agrees with
    /// the world's own count.
    #[test]
    fn broadphase_stats_report_the_trees_shape() {
        let mut world = PhysicsWorld::new();
        assert_eq!(
            world.broadphase_stats(),
            BroadphaseStats {
                elements: 0,
                nodes: 0,
                depth: 0,
                refits: 0,
                updates_without_refit: 0,
                rebuilds: 1,
            },
            "an empty world has an empty tree, built by the reading itself"
        );

        for x in 0..16 {
            world.add_sphere(Sphere::new(DVec3::new(f64::from(x), 0.0, 0.0), 0.4));
        }
        let stats = world.broadphase_stats();
        assert_eq!(stats.elements, 16, "every collider is a leaf");
        assert!(
            stats.nodes >= 16,
            "a binary tree holds at least one node per leaf; got {stats:?}"
        );
        assert!(
            stats.depth >= 1,
            "a non-empty tree has a depth; got {stats:?}"
        );
    }

    /// A line of spheres, and the ids naming them.
    ///
    /// One row rather than a lattice: the refit counters are about *when* the
    /// tree is refit rather than about what it answers, so the cheapest fixture
    /// that still gives a real tree with real ancestors to walk back through is
    /// the right one.
    fn line_of_spheres(n: u32) -> (PhysicsWorld, Vec<ColliderId>) {
        let mut world = PhysicsWorld::new();
        let ids = (0..n)
            .map(|x| world.add_sphere(Sphere::new(DVec3::new(f64::from(x), 0.0, 0.0), 0.4)))
            .collect();
        (world, ids)
    }

    /// **A small move is absorbed in place: one refit, nothing invalidated,
    /// nothing rebuilt.**
    ///
    /// The counters exist to answer "did that phase refit or did it rebuild",
    /// and this is the answer they have to give for the case a game actually
    /// runs — a body that walked a fraction of its own radius since last tick.
    #[test]
    fn a_small_update_is_counted_as_a_refit() {
        let (mut world, ids) = line_of_spheres(16);
        let before = world.broadphase_stats();
        assert_eq!(before.rebuilds, 1, "the reading itself built the tree");
        assert_eq!(before.refits, 0, "and nothing has been updated yet");

        assert!(world.set_sphere(ids[7], Sphere::new(DVec3::new(7.05, 0.0, 0.0), 0.4)));

        let after = world.broadphase_stats();
        assert_eq!(
            after.refits,
            before.refits + 1,
            "the update should have refit the tree in place; got {after:?}"
        );
        assert_eq!(
            after.updates_without_refit, before.updates_without_refit,
            "nothing should have been left for a rebuild; got {after:?}"
        );
        assert_eq!(
            after.rebuilds, before.rebuilds,
            "and no rebuild should have happened; got {after:?}"
        );
    }

    /// **A teleport refits too — a tree is never thrown away for a move being
    /// large.**
    ///
    /// [`Bvh::update_aabb`] fails only when the element index names nothing. It
    /// has no notion of the new box escaping the old one, and grows the
    /// ancestors it walks instead. Worth pinning rather than assuming, because
    /// the opposite is the intuitive guess and a `refits` read under it would be
    /// read as a rebuild that never happened. The move is checked to have
    /// *landed* as well, so a refit that counted itself without touching the
    /// tree fails here rather than passing quietly.
    #[test]
    fn a_teleport_refits_rather_than_rebuilding() {
        let (mut world, ids) = line_of_spheres(16);
        let before = world.broadphase_stats();

        assert!(world.set_sphere(ids[7], Sphere::new(DVec3::new(0.0, 900.0, 0.0), 0.4)));

        let after = world.broadphase_stats();
        assert_eq!(
            after.refits,
            before.refits + 1,
            "a teleport is still a refit; got {after:?}"
        );
        assert_eq!(
            after.rebuilds, before.rebuilds,
            "and costs no rebuild; got {after:?}"
        );
        assert_eq!(
            after.updates_without_refit, before.updates_without_refit,
            "and leaves nothing for one; got {after:?}"
        );

        assert_eq!(
            world.overlap_sphere(DVec3::new(0.0, 900.0, 0.0), 0.1),
            vec![ids[7]],
            "the refit tree must answer at the new place"
        );
        assert!(
            world
                .overlap_sphere(DVec3::new(7.0, 0.0, 0.0), 0.1)
                .is_empty(),
            "and no longer at the old one"
        );
    }

    /// **An update the tree cannot absorb drops it, and the next query pays for
    /// the rebuild.**
    ///
    /// The state that produces it is unreachable through the public API — a
    /// live collider whose built tree holds no element for its slot — so the
    /// test reaches in and builds it. That state is the whole of what
    /// `updates_without_refit` counts while a tree exists, and leaving it
    /// unexercised would leave the invalidation branch guarded by nothing.
    #[test]
    fn an_update_with_no_element_to_refit_invalidates_and_the_next_query_rebuilds() {
        let (mut world, ids) = line_of_spheres(16);
        let before = world.broadphase_stats();
        assert!(world.bvh.is_some(), "the reading built the tree");

        world.bvh_slot_to_elem[ids[3].index() as usize] = NO_ELEMENT;
        assert!(world.set_sphere(ids[3], Sphere::new(DVec3::new(3.0, 40.0, 0.0), 0.4)));
        assert!(
            world.bvh.is_none(),
            "an update that could not refit must drop the tree rather than \
             leave it holding the old bounds"
        );

        let after = world.broadphase_stats();
        assert_eq!(
            after.updates_without_refit,
            before.updates_without_refit + 1,
            "the update should be counted as one that did not refit; got {after:?}"
        );
        assert_eq!(
            after.refits, before.refits,
            "and must not also count as a refit; got {after:?}"
        );
        assert_eq!(
            after.rebuilds,
            before.rebuilds + 1,
            "the reading after it should have rebuilt; got {after:?}"
        );
        assert_eq!(
            after.elements, 16,
            "and rebuilt the whole set; got {after:?}"
        );

        assert_eq!(
            world.overlap_sphere(DVec3::new(3.0, 40.0, 0.0), 0.1),
            vec![ids[3]],
            "the rebuilt tree must answer at the new place"
        );
    }

    /// **Updates made before there is a tree are counted one by one, and cost
    /// one rebuild between them.**
    ///
    /// The counting rule in one test: the totals only climb, `refits` and
    /// `updates_without_refit` between them account for every `set_*` that
    /// resolved to a live collider, and one that resolved to nothing is counted
    /// by neither.
    #[test]
    fn updates_with_no_tree_yet_are_counted_per_update_not_per_rebuild() {
        let (mut world, ids) = line_of_spheres(16);
        assert!(world.set_sphere(ids[0], Sphere::new(DVec3::new(0.0, 1.0, 0.0), 0.4)));
        assert!(world.set_sphere(ids[1], Sphere::new(DVec3::new(1.0, 1.0, 0.0), 0.4)));

        let stats = world.broadphase_stats();
        assert_eq!(
            stats.updates_without_refit, 2,
            "one per update, with no tree to refit; got {stats:?}"
        );
        assert_eq!(stats.refits, 0, "nothing was refit; got {stats:?}");
        assert_eq!(
            stats.rebuilds, 1,
            "and one rebuild between the two of them; got {stats:?}"
        );

        assert!(world.remove(ids[5]));
        let before = world.broadphase_stats();
        assert!(!world.set_sphere(ids[5], Sphere::new(DVec3::ZERO, 0.4)));
        let after = world.broadphase_stats();
        assert_eq!(
            (after.refits, after.updates_without_refit),
            (before.refits, before.updates_without_refit),
            "a set_* on a stale id is counted by neither; got {after:?}"
        );
    }

    /// **The shared view finds exactly what overlaps, checked against a scan
    /// rather than against the other form.**
    ///
    /// Comparing [`OverlapQueries`] with [`PhysicsWorld::overlap_sphere_into`]
    /// would prove nothing: they are one implementation, and a defect in it
    /// breaks both sides of that comparison equally — measured, not assumed, by
    /// reversing the core's output and watching the comparison stay green. So
    /// the oracle here is an exhaustive scan of every collider in the fixture,
    /// which shares no code with the traversal at all.
    ///
    /// It is a set comparison, because a scan cannot reproduce the tree's
    /// order. The *order* is what a caller reducing over the neighbourhood in
    /// `f64` depends on, and it comes from having one implementation rather than
    /// from this test — which is the reason the two forms were made to share
    /// one, and the reason `apps/horde`'s
    /// `steering_is_bit_identical_however_many_workers_run_it` is where that
    /// property is actually asserted.
    #[test]
    fn the_shared_view_finds_exactly_what_a_scan_would() {
        let mut world = PhysicsWorld::new();
        // A lattice, so a query sphere lands on a neighbourhood rather than on
        // one collider or none, and a descent that pruned a subtree it should
        // not have loses something the scan still finds.
        let mut expected: Vec<(ColliderId, Sphere)> = Vec::new();
        for x in -3..=3 {
            for y in -3..=3 {
                let sphere = Sphere::new(DVec3::new(f64::from(x), f64::from(y), 0.0), 0.4);
                expected.push((world.add_sphere(sphere), sphere));
            }
        }
        // A capsule and a box as well, so every arm of the exact test is
        // exercised rather than only the sphere one.
        let capsule = Capsule::new(DVec3::ZERO, 0.3, 1.0);
        let capsule_id = world.add_capsule(capsule);
        let boxed = BoxCollider::new(DVec3::new(1.5, 1.5, 0.0), DVec3::splat(0.5));
        let box_id = world.add_box(boxed);

        let centres = [
            DVec3::ZERO,
            DVec3::new(1.2, -0.7, 0.0),
            DVec3::new(-2.5, 2.5, 0.0),
            DVec3::new(50.0, 50.0, 0.0),
        ];
        let mut scratch = QueryScratch::new();
        let mut biggest = 0usize;
        for centre in centres {
            for radius in [0.1, 0.75, 2.0] {
                let probe = Sphere::new(centre, radius);
                // The oracle: every collider, tested directly, with no tree and
                // no traversal between the shapes and the answer.
                let mut scanned: Vec<ColliderId> = expected
                    .iter()
                    .filter(|(_, s)| query::sphere_overlaps_sphere(&probe, s))
                    .map(|(id, _)| *id)
                    .collect();
                if query::sphere_overlaps_capsule(&probe, &capsule) {
                    scanned.push(capsule_id);
                }
                if query::sphere_overlaps_aabb(&probe, &boxed.aabb()) {
                    scanned.push(box_id);
                }
                scanned.sort_unstable_by_key(|id| id.index());

                let mut shared = Vec::new();
                world.overlap_queries().overlap_sphere_into(
                    centre,
                    radius,
                    &mut scratch,
                    &mut shared,
                );
                shared.sort_unstable_by_key(|id| id.index());

                assert_eq!(scanned, shared, "at {centre} r{radius}");
                biggest = biggest.max(shared.len());
            }
        }
        assert!(
            biggest > 4,
            "the widest query in the fixture found {biggest} colliders, so the \
             comparison above was mostly two empty vectors",
        );
    }

    /// Every probe's answers, from one form or the other, so a whole run
    /// compares in a single `assert_eq!`.
    #[derive(Debug, PartialEq)]
    struct ProbeAnswers {
        rays: Vec<Option<(ColliderId, ShapeHit)>>,
        sweeps: Vec<Option<(ColliderId, ShapeHit)>>,
        excluded_sweeps: Vec<Option<(ColliderId, ShapeHit)>>,
        aabbs: Vec<Vec<ColliderId>>,
    }

    /// The exact ray test for a collider, with no tree and no traversal
    /// between the shape and the answer.
    fn scan_ray_one(ray: &Ray, entry: &ColliderEntry) -> Option<ShapeHit> {
        match entry {
            ColliderEntry::Sphere(s) => query::ray_vs_sphere(ray, s),
            ColliderEntry::Box(b) => query::ray_vs_aabb(ray, &b.aabb()),
            ColliderEntry::Capsule(c) => query::ray_vs_capsule(ray, c),
        }
    }

    /// The exact swept-sphere test for a collider, likewise.
    fn scan_sweep_one(segment: &Segment, radius: f64, entry: &ColliderEntry) -> Option<ShapeHit> {
        match entry {
            ColliderEntry::Sphere(s) => query::swept_sphere_vs_sphere(segment, radius, s),
            ColliderEntry::Box(b) => query::swept_sphere_vs_aabb(segment, radius, &b.aabb()),
            ColliderEntry::Capsule(c) => query::swept_sphere_vs_capsule(segment, radius, c),
        }
    }

    /// **The ray, sweep and AABB queries answer a brute-force scan under a
    /// shared borrow, from every thread asking at once.**
    ///
    /// The companion to `the_shared_view_finds_exactly_what_a_scan_would`, and
    /// it is held to the same standard for the same reason: comparing
    /// [`OverlapQueries::cast_ray`] with [`PhysicsWorld::cast_ray`] proves
    /// nothing on its own, because they are one implementation and a defect in
    /// it moves both sides of that comparison together. So each answer is also
    /// checked against an exhaustive scan of the fixture, which shares no code
    /// with the traversal.
    ///
    /// What the scan can and cannot pin down:
    ///
    /// * For a ray or a sweep the scan gives the closest `t`, and the query's
    ///   own `t` must equal it exactly — same inputs through the same shape
    ///   functions, so this is not a tolerance question. The *identity* is then
    ///   checked by re-running the exact test on the collider that came back:
    ///   a query that returned a farther collider fails on `t`, and one that
    ///   returned a mislabelled id fails on the re-test.
    /// * For an AABB overlap the scan gives the whole set, compared sorted —
    ///   tree order is not something a scan can reproduce. The order is a
    ///   property of the two forms sharing one traversal, which is why they do.
    ///
    /// The concurrent half is what the shared form exists for: one view, a
    /// [`QueryScratch`] per thread, and every thread must reach the answers the
    /// calling thread reached alone. It guards a future regression — putting
    /// the scratch back inside the view is exactly the change that would break
    /// it — rather than a race in today's code, which holds only shared
    /// references.
    #[test]
    fn the_shared_view_casts_sweeps_and_boxes_like_a_scan_would() {
        let mut world = PhysicsWorld::new();
        // (id, shape, is_trigger): the oracle, and all the scan below reads.
        let mut fixture: Vec<(ColliderId, ColliderEntry, bool)> = Vec::new();
        // A lattice, so a probe lands on a neighbourhood rather than on one
        // collider or none, and a descent that pruned a subtree it should not
        // have loses something the scan still finds.
        let mut row_start = None;
        for x in -3..=3 {
            for y in -3..=3 {
                let sphere = Sphere::new(DVec3::new(f64::from(x), f64::from(y), 0.0), 0.4);
                let id = world.add_sphere(sphere);
                if x == -3 && y == 0 {
                    row_start = Some(id);
                }
                fixture.push((id, ColliderEntry::Sphere(sphere), false));
            }
        }
        let row_start = row_start.expect("the lattice contains (-3, 0, 0)");
        // A capsule and a box as well, so every arm of the exact test is
        // exercised rather than only the sphere one.
        let capsule = Capsule::new(DVec3::new(0.0, 0.0, 3.0), 0.3, 1.0);
        fixture.push((
            world.add_capsule(capsule),
            ColliderEntry::Capsule(capsule),
            true,
        ));
        let boxed = BoxCollider::new(DVec3::new(1.5, 1.5, 0.0), DVec3::splat(0.5));
        fixture.push((world.add_box(boxed), ColliderEntry::Box(boxed), false));
        // A trigger standing in front of the first ray's solid hits. Triggers
        // are non-solid, so a cast that reports this one has stopped skipping
        // them — and `the_skipped_trigger_was_in_the_way` below checks that
        // this collider really is in the way, so the skip is a branch the run
        // actually takes.
        let trigger = Sphere::new(DVec3::new(-5.0, 0.0, 0.0), 0.5);
        let trigger_id = world.add_sphere(trigger);
        assert!(world.set_trigger(trigger_id, true), "the trigger was added");
        fixture.push((trigger_id, ColliderEntry::Sphere(trigger), true));

        let rays = [
            // Down the lattice's middle row, through the trigger first.
            Ray::new(DVec3::new(-10.0, 0.0, 0.0), DVec3::X),
            // Another row, no trigger on it.
            Ray::new(DVec3::new(-10.0, 2.0, 0.0), DVec3::X),
            // Along +Z through a lattice sphere and the capsule behind it.
            Ray::new(DVec3::new(0.0, 0.0, -10.0), DVec3::Z),
            // Between the rows, so only the box is wide enough to be hit.
            Ray::new(DVec3::new(-10.0, 1.5, 0.0), DVec3::X),
            // Diagonally across the lattice.
            Ray::new(DVec3::new(-10.0, -10.0, 0.0), DVec3::new(1.0, 1.0, 0.0)),
            // Nowhere near anything.
            Ray::new(DVec3::new(-10.0, 20.0, 0.0), DVec3::X),
            // Bounded short of the first solid collider, so `t_max` decides.
            Ray::new(DVec3::new(-10.0, 0.0, 0.0), DVec3::X).with_bounds(0.0, 5.0),
        ];
        let sweeps = [
            (
                Segment::new(DVec3::new(-10.0, 0.0, 0.0), DVec3::new(10.0, 0.0, 0.0)),
                0.2,
            ),
            // Between two rows, close enough that the swept volume reaches
            // both — the case a centre-line traversal used to drop.
            (
                Segment::new(DVec3::new(-10.0, 0.7, 0.0), DVec3::new(10.0, 0.7, 0.0)),
                0.35,
            ),
            (
                Segment::new(DVec3::new(0.0, 3.0, -10.0), DVec3::new(0.0, 3.0, 10.0)),
                0.1,
            ),
            // Starting inside a collider.
            (
                Segment::new(DVec3::new(0.0, 3.0, 0.0), DVec3::new(0.1, 3.0, 0.0)),
                0.2,
            ),
            // Nowhere near anything.
            (
                Segment::new(DVec3::new(20.0, 20.0, 20.0), DVec3::new(30.0, 30.0, 30.0)),
                0.5,
            ),
        ];
        let aabbs = [
            Aabb::from_centre_half(DVec3::ZERO, DVec3::splat(1.2)),
            Aabb::from_centre_half(DVec3::new(1.5, 1.5, 0.0), DVec3::splat(0.6)),
            Aabb::from_centre_half(DVec3::new(50.0, 50.0, 0.0), DVec3::splat(1.0)),
            Aabb::from_centre_half(DVec3::ZERO, DVec3::splat(10.0)),
        ];

        // ── The exclusive form's answers, before the view borrows the world ──
        let plain_sweeps: Vec<Option<(ColliderId, ShapeHit)>> = sweeps
            .iter()
            .map(|(segment, radius)| world.sweep_sphere(segment, *radius))
            .collect();
        // The same sweeps with the collider each one actually hits left out —
        // the query a body sweeping its own path runs, and the only form of it
        // where the exclusion is guaranteed to change the answer. The last
        // entry is the opposite case: excluding a collider that sweep never
        // reaches, which must change nothing.
        let mut excluded_sweeps: Vec<(Segment, f64, ColliderId)> = sweeps
            .iter()
            .zip(&plain_sweeps)
            .filter_map(|((segment, radius), hit)| hit.map(|(id, _)| (*segment, *radius, id)))
            .collect();
        excluded_sweeps.push((sweeps[2].0, sweeps[2].1, row_start));

        let exclusive = ProbeAnswers {
            rays: rays.iter().map(|ray| world.cast_ray(ray)).collect(),
            sweeps: plain_sweeps,
            excluded_sweeps: excluded_sweeps
                .iter()
                .map(|(segment, radius, skip)| {
                    world.sweep_sphere_excluding(segment, *radius, Some(*skip))
                })
                .collect(),
            aabbs: aabbs
                .iter()
                .map(|aabb| {
                    let mut ids = world.overlap_aabb(aabb);
                    ids.sort_unstable_by_key(|id| id.index());
                    ids
                })
                .collect(),
        };

        // ── The scan, which shares no code with the traversal ─────────────
        let solid = |skip: Option<ColliderId>| {
            fixture
                .iter()
                .filter(move |(id, _, is_trigger)| !is_trigger && Some(*id) != skip)
        };
        let closest =
            |hits: &mut dyn Iterator<Item = ShapeHit>| hits.map(|hit| hit.t).min_by(f64::total_cmp);
        let entry_of = |wanted: ColliderId| {
            fixture
                .iter()
                .find(|(id, _, _)| *id == wanted)
                .map(|(_, entry, is_trigger)| (entry.clone(), *is_trigger))
                .expect("the query named a collider the fixture does not hold")
        };

        let mut rays_that_hit = 0usize;
        for (ray, answer) in rays.iter().zip(&exclusive.rays) {
            let scanned =
                closest(&mut solid(None).filter_map(|(_, entry, _)| scan_ray_one(ray, entry)));
            assert_eq!(
                answer.map(|(_, hit)| hit.t),
                scanned,
                "the closest solid hit along {ray:?}",
            );
            if let Some((id, hit)) = answer {
                let (entry, is_trigger) = entry_of(*id);
                assert!(!is_trigger, "a cast reported a trigger: {ray:?}");
                assert_eq!(
                    scan_ray_one(ray, &entry),
                    Some(*hit),
                    "the collider the cast named does not produce the hit it reported",
                );
                rays_that_hit += 1;
            }
        }
        assert!(
            rays_that_hit >= 4 && rays_that_hit < rays.len(),
            "{rays_that_hit} of {} ray probes hit, so the comparison above was \
             mostly empty on one side or never exercised a miss",
            rays.len(),
        );

        // The trigger is genuinely in the way of the first ray, so skipping it
        // is a branch this run takes rather than one it never reaches.
        let with_triggers = closest(
            &mut fixture
                .iter()
                .filter_map(|(_, entry, _)| scan_ray_one(&rays[0], entry)),
        );
        assert!(
            with_triggers < exclusive.rays[0].map(|(_, hit)| hit.t),
            "the trigger is not in front of the first ray's solid hits, so \
             nothing here would notice if triggers stopped being skipped",
        );

        let mut sweeps_that_hit = 0usize;
        for ((segment, radius), answer) in sweeps.iter().zip(&exclusive.sweeps) {
            let scanned = closest(
                &mut solid(None)
                    .filter_map(|(_, entry, _)| scan_sweep_one(segment, *radius, entry)),
            );
            assert_eq!(
                answer.map(|(_, hit)| hit.t),
                scanned,
                "the closest solid hit sweeping r{radius} along {segment:?}",
            );
            if let Some((id, hit)) = answer {
                let (entry, is_trigger) = entry_of(*id);
                assert!(!is_trigger, "a sweep reported a trigger: {segment:?}");
                assert_eq!(
                    scan_sweep_one(segment, *radius, &entry),
                    Some(*hit),
                    "the collider the sweep named does not produce the hit it reported",
                );
                sweeps_that_hit += 1;
            }
        }
        assert!(
            sweeps_that_hit >= 3 && sweeps_that_hit < sweeps.len(),
            "{sweeps_that_hit} of {} sweep probes hit, so the comparison above \
             was mostly empty on one side or never exercised a miss",
            sweeps.len(),
        );

        let mut exclusion_mattered = 0usize;
        let mut exclusion_was_moot = 0usize;
        for ((segment, radius, skip), answer) in
            excluded_sweeps.iter().zip(&exclusive.excluded_sweeps)
        {
            let scanned = closest(
                &mut solid(Some(*skip))
                    .filter_map(|(_, entry, _)| scan_sweep_one(segment, *radius, entry)),
            );
            assert_eq!(
                answer.map(|(_, hit)| hit.t),
                scanned,
                "the closest hit excluding {skip:?}, sweeping r{radius} along {segment:?}",
            );
            assert_ne!(
                answer.map(|(id, _)| id),
                Some(*skip),
                "the excluded collider came back anyway",
            );
            // Whether the exclusion was load-bearing is the scan's to say, not
            // the query's: it is whether the closest solid hit moved when that
            // collider left the fixture.
            let unexcluded = closest(
                &mut solid(None)
                    .filter_map(|(_, entry, _)| scan_sweep_one(segment, *radius, entry)),
            );
            if unexcluded == scanned {
                exclusion_was_moot += 1;
            } else {
                exclusion_mattered += 1;
            }
        }
        assert!(
            exclusion_mattered >= 3 && exclusion_was_moot >= 1,
            "{exclusion_mattered} exclusions changed the answer and \
             {exclusion_was_moot} did not, so one of the two cases never ran",
        );

        let mut widest = 0usize;
        for (aabb, answer) in aabbs.iter().zip(&exclusive.aabbs) {
            let mut scanned: Vec<ColliderId> = fixture
                .iter()
                .filter(|(_, entry, _)| entry.aabb().intersects(aabb))
                .map(|(id, _, _)| *id)
                .collect();
            scanned.sort_unstable_by_key(|id| id.index());
            assert_eq!(&scanned, answer, "the AABBs meeting {aabb:?}");
            widest = widest.max(answer.len());
        }
        assert!(
            widest > 4,
            "the widest AABB probe found {widest} colliders, so the comparison \
             above was mostly two empty vectors",
        );

        // ── The same answers under a shared borrow, from several threads ──
        // One scratch and one output buffer for the whole run, which is how a
        // `par_for` chunk holds them — and what makes a query that forgot to
        // clear its output visible here.
        let ask = |queries: &OverlapQueries<'_>| {
            let mut scratch = QueryScratch::new();
            let mut ids = Vec::new();
            ProbeAnswers {
                rays: rays
                    .iter()
                    .map(|ray| queries.cast_ray(ray, &mut scratch))
                    .collect(),
                sweeps: sweeps
                    .iter()
                    .map(|(segment, radius)| queries.sweep_sphere(segment, *radius, &mut scratch))
                    .collect(),
                excluded_sweeps: excluded_sweeps
                    .iter()
                    .map(|(segment, radius, skip)| {
                        queries.sweep_sphere_excluding(segment, *radius, Some(*skip), &mut scratch)
                    })
                    .collect(),
                aabbs: aabbs
                    .iter()
                    .map(|aabb| {
                        queries.overlap_aabb_into(aabb, &mut scratch, &mut ids);
                        let mut found = ids.clone();
                        found.sort_unstable_by_key(|id| id.index());
                        found
                    })
                    .collect(),
            }
        };

        let queries = world.overlap_queries();
        assert_eq!(ask(&queries), exclusive, "the calling thread's own answers");

        std::thread::scope(|scope| {
            let handles: Vec<_> = (0..4).map(|_| scope.spawn(|| ask(&queries))).collect();
            for handle in handles {
                assert_eq!(
                    handle.join().expect("a query thread panicked"),
                    exclusive,
                    "a thread sharing the view disagreed with the scan",
                );
            }
        });
    }

    /// **A sphere overlap is shape-aware: the query radius is expanded by each
    /// collider's own radius.**
    ///
    /// `overlap_sphere(centre, R)` returns every collider whose *shape* overlaps
    /// the query sphere, so a sphere collider of radius `r_b` is returned iff
    /// its centre is within `R + r_b` of the query's — on every axis by the
    /// broadphase prefilter (whose leaf AABBs carry the collider's own extent),
    /// and off-axis by the exact shape test, which trims the prefilter's square
    /// back to the circle. The contract matters to `apps/horde`: its separation
    /// query passes `r_self + slack` and expects neighbours out to
    /// `r_self + slack + r_b`, which is only right because of this expansion —
    /// the guard for that lives here rather than in the consumer.
    ///
    /// The two families of cases exercise the two layers separately: the
    /// on-axis ones are decided by the broadphase, the 45° ones (in the z = 0
    /// plane) are admitted by the broadphase and decided by the exact shape
    /// test.
    #[test]
    fn a_sphere_overlap_is_expanded_by_the_colliders_own_radius() {
        let mut world = PhysicsWorld::new();
        let body = world.add_sphere(Sphere::new(DVec3::ZERO, 0.5));
        let mut scratch = QueryScratch::new();
        let mut out = Vec::new();
        let queries = world.overlap_queries();

        let query_radius = 1.0;
        let boundary = query_radius + 0.5; // the body's radius

        // On the query's own axis, the broadphase's AABB admission decides.
        queries.overlap_sphere_into(
            DVec3::new(boundary - 0.01, 0.0, 0.0),
            query_radius,
            &mut scratch,
            &mut out,
        );
        assert_eq!(
            out,
            vec![body],
            "a body whose centre is inside R + r_b is returned"
        );

        queries.overlap_sphere_into(
            DVec3::new(boundary + 0.01, 0.0, 0.0),
            query_radius,
            &mut scratch,
            &mut out,
        );
        assert!(
            out.is_empty(),
            "a body whose centre is outside R + r_b is not returned"
        );

        // The boundary is R + r_b, not R: a query radius alone, with the body's
        // centre at R + r_b/2, must still find it.
        queries.overlap_sphere_into(
            DVec3::new(query_radius + 0.25, 0.0, 0.0),
            query_radius,
            &mut scratch,
            &mut out,
        );
        assert_eq!(
            out,
            vec![body],
            "the query radius is expanded by the collider's own radius, not used raw"
        );

        // At 45° in the z = 0 plane, the AABB prefilter still admits both
        // (axis offsets below R + r_b), so the exact shape test decides: the
        // boundary is the circle of radius R + r_b, not the square.
        // (R + r_b)/√2 ≈ 1.0607.
        queries.overlap_sphere_into(
            DVec3::new(1.05, 1.05, 0.0),
            query_radius,
            &mut scratch,
            &mut out,
        );
        assert_eq!(
            out,
            vec![body],
            "a body within the circle of radius R + r_b is returned even off-axis"
        );

        queries.overlap_sphere_into(
            DVec3::new(1.07, 1.07, 0.0),
            query_radius,
            &mut scratch,
            &mut out,
        );
        assert!(
            out.is_empty(),
            "a body outside the circle of radius R + r_b is not returned, \
             even where the broadphase AABB would admit it"
        );
    }

    /// One scratch, many queries: the buffers carry nothing between calls.
    ///
    /// The whole point of handing the caller the scratch is that it is reused,
    /// so a query that read a stale `candidates` from the call before it would
    /// be a defect this API introduced and the owning form cannot have.
    #[test]
    fn a_reused_scratch_does_not_leak_one_query_into_the_next() {
        let mut world = PhysicsWorld::new();
        world.add_sphere(Sphere::new(DVec3::ZERO, 0.5));
        world.add_sphere(Sphere::new(DVec3::new(20.0, 0.0, 0.0), 0.5));

        let mut scratch = QueryScratch::new();
        let mut out = Vec::new();
        let queries = world.overlap_queries();

        queries.overlap_sphere_into(DVec3::ZERO, 1.0, &mut scratch, &mut out);
        assert_eq!(out.len(), 1);
        queries.overlap_sphere_into(DVec3::new(0.0, 100.0, 0.0), 1.0, &mut scratch, &mut out);
        assert!(out.is_empty(), "a stale candidate survived the next query");
        queries.overlap_sphere_into(DVec3::new(20.0, 0.0, 0.0), 1.0, &mut scratch, &mut out);
        assert_eq!(out.len(), 1);
    }

    #[test]
    fn adding_and_removing_a_collider_leaves_the_world_empty_again() {
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

    /// **The excluded collider is dropped before the winner is picked, so the
    /// next-closest hit survives.**
    ///
    /// `near` sits where the swept sphere reaches it first and `far` behind it,
    /// so an implementation that ran the ordinary sweep and threw the answer
    /// away when it named `near` would report nothing at all. Excluding `near`
    /// has to return `far`, and excluding an id the world never issued has to
    /// return `near` again.
    #[test]
    fn sweep_sphere_excluding_keeps_the_next_closest_hit() {
        let mut world = PhysicsWorld::new();
        let near = world.add_sphere(Sphere::new(DVec3::new(0.0, 0.0, 0.0), 1.0));
        let far = world.add_sphere(Sphere::new(DVec3::new(3.0, 0.0, 0.0), 1.0));
        let seg = Segment::new(DVec3::new(-5.0, 0.0, 0.0), DVec3::new(5.0, 0.0, 0.0));

        let (id, _) = world.sweep_sphere(&seg, 0.5).expect("the sweep hits both");
        assert_eq!(id, near, "the unfiltered sweep answers the closest");

        let (id, hit) = world
            .sweep_sphere_excluding(&seg, 0.5, Some(near))
            .expect("the one behind it is still there");
        assert_eq!(id, far, "excluding the closest must not lose the rest");
        assert!(
            (hit.t - 0.65).abs() < 0.001,
            "expected ~0.65, got {}",
            hit.t
        );

        let (id, _) = world
            .sweep_sphere_excluding(&seg, 0.5, Some(ColliderId::new(999, 0)))
            .expect("an id the world never issued excludes nothing");
        assert_eq!(id, near);

        // **The dangerous stale id is the one whose slot is still live.** An id
        // the world never issued names no slot at all, so ignoring it is easy;
        // an id whose index was reused names a slot that *is* occupied, by
        // something else. Excluding on the index alone would take the new
        // occupant out of the sweep, which is a hit silently lost rather than a
        // filter that did nothing.
        assert!(world.remove(near), "the collider was live");
        let reused = world.add_sphere(Sphere::new(DVec3::new(0.0, 0.0, 0.0), 1.0));
        assert_eq!(
            reused.index, near.index,
            "this test only says anything while the slot is actually reused"
        );
        assert_ne!(
            reused.generation, near.generation,
            "and the id has moved on"
        );
        let (id, _) = world
            .sweep_sphere_excluding(&seg, 0.5, Some(near))
            .expect("the stale id must not hide the collider that took its slot");
        assert_eq!(
            id, reused,
            "a generation that has moved on excludes nothing, so the sweep still \
             answers the shape occupying that slot"
        );
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
    fn rebuilding_after_the_last_removal_leaves_nothing_for_a_ray_to_hit() {
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
    fn the_worlds_debug_output_reports_how_many_colliders_it_holds() {
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
