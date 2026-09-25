//! Islands and sleep: contact-solver rung 3 and decision 4
//! (`docs/notes/simulation.md`).
//!
//! ```text
//!   a contact begins touching between two dynamic bodies ─▶ their islands merge
//!   a touching contact between two of them ends ─────────▶ `removed` += 1
//!   end of the step:
//!     every awake dynamic body slower than both sleep thresholds adds the
//!     tick to its timer; any other has it reset
//!     the island needing a split whose sleepiest body has slept longest is
//!     split into its connected parts — one a tick, and only once some part
//!     of it could sleep
//!     every island not needing a split, all of whose bodies have been slow
//!     for `time_to_sleep`, sleeps: its bodies leave the awake set
//! ```
//!
//! An **island** is a set of dynamic bodies joined by touching contacts, kept
//! from tick to tick rather than rebuilt: it is **merged** the moment a
//! contact between two of its bodies begins, and **split lazily**, only when
//! it has lost a contact and some of it wants to sleep — Box2D v3's scheme,
//! where islands exist for sleeping and nothing else. Static bodies, planes and
//! kinematic bodies join no island, so a floor does not tie everything on it
//! into one.
//!
//! A **sleeping island holds its own bodies**: their transforms and bodies
//! move out of the awake set into the island, so the step, the solver and the
//! broadphase update never see them — they cost nothing and are not
//! integrated. Their contacts stay in the pool with their manifolds and
//! impulses, so a woken stack is warm-started exactly as it was left.
//!
//! # Determinism
//!
//! Nothing here iterates a hash map: islands live in slots and are visited in
//! slot order, an island's bodies in the order they joined it, and the split
//! finds its parts in that order too. The map [`Islands::split`] builds is
//! used only to look up an index.

use std::collections::HashMap;

use crcbl_core::Pool;

use crate::components::{RigidBody, Transform};
use crate::system::{AwakeSet, BodyId, BodyRecord, BodySet};

/// An island's slot.
pub(crate) type IslandId = u32;

/// A sleeping island's bodies, indexed as the island's `bodies` are.
#[derive(Debug, Default)]
pub(crate) struct Sleepers {
    transforms: Vec<Transform>,
    bodies: Vec<RigidBody>,
}

/// One island.
#[derive(Debug)]
struct Island {
    /// Its bodies, in the order they joined.
    bodies: Vec<BodyId>,
    /// Touching contacts between two of its bodies that ended since it was
    /// last split, and bodies taken out of it: while this is not zero the
    /// island may be in pieces, and it cannot sleep until it is split.
    removed: u32,
    /// Its bodies' state while it sleeps.
    asleep: Option<Sleepers>,
}

/// Every island of a system with contacts.
#[derive(Debug, Default)]
pub(crate) struct Islands {
    slots: Vec<Option<Island>>,
    free: Vec<IslandId>,
    sleeping_bodies: usize,
    sleeping_islands: usize,
    live: usize,
}

/// How long body `id` has been slow enough to sleep, or zero for one that is
/// not in the awake set.
fn sleep_time(records: &Pool<BodyRecord>, awake: &AwakeSet, id: BodyId) -> f64 {
    match records.get(id) {
        Some(record) if record.set == BodySet::Awake => awake.sleep_times[record.index],
        _ => 0.0,
    }
}

impl Islands {
    /// Bodies whose island sleeps.
    pub(crate) fn sleeping_bodies(&self) -> usize {
        self.sleeping_bodies
    }

    /// Islands that sleep.
    pub(crate) fn sleeping_islands(&self) -> usize {
        self.sleeping_islands
    }

    /// Islands that are awake.
    pub(crate) fn awake_islands(&self) -> usize {
        self.live - self.sleeping_islands
    }

    /// The transform of the body at `index` of sleeping island `island`.
    ///
    /// # Panics
    ///
    /// Panics if the island does not sleep: a record says `Sleeping` only
    /// while it does.
    pub(crate) fn transform(&self, island: Option<IslandId>, index: usize) -> &Transform {
        &self.sleepers(island).transforms[index]
    }

    /// The body at `index` of sleeping island `island`, on
    /// [`transform`](Self::transform)'s terms.
    pub(crate) fn body(&self, island: Option<IslandId>, index: usize) -> &RigidBody {
        &self.sleepers(island).bodies[index]
    }

    fn sleepers(&self, island: Option<IslandId>) -> &Sleepers {
        island
            .and_then(|island| self.slots.get(island as usize))
            .and_then(Option::as_ref)
            .and_then(|island| island.asleep.as_ref())
            .expect("a sleeping body's island sleeps")
    }

    /// Gives every awake dynamic body without an island one of its own, and
    /// takes out of its island every body that is no longer dynamic — the
    /// bodies registered or changed since the last step.
    pub(crate) fn reconcile(&mut self, records: &mut Pool<BodyRecord>, awake: &AwakeSet) {
        for (index, &id) in awake.ids.iter().enumerate() {
            let dynamic = awake.bodies[index].is_dynamic();
            let Some(record) = records.get_mut(id) else {
                continue;
            };
            match (dynamic, record.island) {
                (true, None) => record.island = Some(self.create(id)),
                (false, Some(island)) => {
                    record.island = None;
                    self.remove_member(island, id);
                }
                _ => {}
            }
        }
    }

    /// Takes body `id` out of awake island `island`, which may leave it in
    /// pieces, and frees the island if it is left empty.
    pub(crate) fn remove_member(&mut self, island: IslandId, id: BodyId) {
        let Some(slot) = self.slots.get_mut(island as usize) else {
            return;
        };
        let Some(entry) = slot.as_mut() else {
            return;
        };
        debug_assert!(entry.asleep.is_none(), "a body leaves only an awake island");
        entry.bodies.retain(|&body| body != id);
        entry.removed += 1;
        if entry.bodies.is_empty() {
            *slot = None;
            self.free.push(island);
            self.live -= 1;
        }
    }

    /// Notes that island `island` may be in pieces: a touching contact inside
    /// it ended.
    pub(crate) fn mark_removed(&mut self, island: IslandId) {
        if let Some(Some(entry)) = self.slots.get_mut(island as usize) {
            entry.removed += 1;
        }
    }

    /// Merges the islands of `a` and `b`, the smaller into the larger, waking
    /// either first if it sleeps.
    pub(crate) fn link(
        &mut self,
        records: &mut Pool<BodyRecord>,
        awake: &mut AwakeSet,
        a: BodyId,
        b: BodyId,
    ) {
        let island = |records: &Pool<BodyRecord>, id| records.get(id).and_then(|r| r.island);
        let (Some(ia), Some(ib)) = (island(records, a), island(records, b)) else {
            return;
        };
        if ia == ib {
            return;
        }
        self.wake(records, awake, ia);
        self.wake(records, awake, ib);
        let len = |this: &Self, id: IslandId| {
            this.slots[id as usize]
                .as_ref()
                .map_or(0, |island| island.bodies.len())
        };
        let (keep, gone) = if len(self, ia) >= len(self, ib) {
            (ia, ib)
        } else {
            (ib, ia)
        };
        let Some(gone_island) = self.slots[gone as usize].take() else {
            return;
        };
        self.free.push(gone);
        self.live -= 1;
        for &id in &gone_island.bodies {
            if let Some(record) = records.get_mut(id) {
                record.island = Some(keep);
            }
        }
        if let Some(kept) = self.slots[keep as usize].as_mut() {
            kept.bodies.extend_from_slice(&gone_island.bodies);
            kept.removed += gone_island.removed;
        }
    }

    /// Wakes the island of body `id`, if it sleeps.
    pub(crate) fn wake_body(
        &mut self,
        records: &mut Pool<BodyRecord>,
        awake: &mut AwakeSet,
        id: BodyId,
    ) {
        if let Some(record) = records.get(id)
            && record.set == BodySet::Sleeping
            && let Some(island) = record.island
        {
            self.wake(records, awake, island);
        }
    }

    /// Wakes every sleeping island.
    pub(crate) fn wake_all(&mut self, records: &mut Pool<BodyRecord>, awake: &mut AwakeSet) {
        for island in 0..self.slots.len() {
            self.wake(records, awake, island as IslandId);
        }
    }

    /// Moves a sleeping island's bodies back into the awake set, each with
    /// its sleep timer at zero. An island that is awake is left alone.
    pub(crate) fn wake(
        &mut self,
        records: &mut Pool<BodyRecord>,
        awake: &mut AwakeSet,
        island: IslandId,
    ) {
        let Some(Some(entry)) = self.slots.get_mut(island as usize) else {
            return;
        };
        let Some(sleepers) = entry.asleep.take() else {
            return;
        };
        for ((&id, transform), body) in entry
            .bodies
            .iter()
            .zip(sleepers.transforms)
            .zip(sleepers.bodies)
        {
            let index = awake.push(id, transform, body);
            if let Some(record) = records.get_mut(id) {
                record.set = BodySet::Awake;
                record.index = index;
            }
        }
        self.sleeping_bodies -= entry.bodies.len();
        self.sleeping_islands -= 1;
    }

    /// Puts an awake island to sleep: each body leaves the awake set with its
    /// velocities zeroed and its forces cleared, as Box2D's does.
    pub(crate) fn sleep(
        &mut self,
        records: &mut Pool<BodyRecord>,
        awake: &mut AwakeSet,
        island: IslandId,
    ) {
        let Some(Some(entry)) = self.slots.get_mut(island as usize) else {
            return;
        };
        if entry.asleep.is_some() {
            return;
        }
        let mut sleepers = Sleepers {
            transforms: Vec::with_capacity(entry.bodies.len()),
            bodies: Vec::with_capacity(entry.bodies.len()),
        };
        for (k, &id) in entry.bodies.iter().enumerate() {
            let Some(record) = records.get(id) else {
                continue;
            };
            debug_assert_eq!(record.set, BodySet::Awake, "an awake island's body");
            let index = record.index;
            let (transform, mut body, moved) = awake.swap_remove(index);
            if let Some(record) = moved.and_then(|moved| records.get_mut(moved)) {
                record.index = index;
            }
            body.velocity = glam::DVec3::ZERO;
            body.angular_velocity = glam::DVec3::ZERO;
            body.clear_forces();
            sleepers.transforms.push(transform);
            sleepers.bodies.push(body);
            if let Some(record) = records.get_mut(id) {
                record.set = BodySet::Sleeping;
                record.index = k;
            }
        }
        self.sleeping_bodies += entry.bodies.len();
        self.sleeping_islands += 1;
        entry.asleep = Some(sleepers);
    }

    /// The island to split this tick, if any: of the awake islands that may
    /// be in pieces, the one whose sleepiest body has been slow longest,
    /// provided that is at least `time_to_sleep` — splitting an island none of
    /// which could sleep yet would buy nothing.
    pub(crate) fn split_candidate(
        &self,
        records: &Pool<BodyRecord>,
        awake: &AwakeSet,
        time_to_sleep: f64,
    ) -> Option<IslandId> {
        let mut best: Option<(f64, IslandId)> = None;
        for (slot, island) in self.slots.iter().enumerate() {
            let Some(island) = island else {
                continue;
            };
            if island.asleep.is_some() || island.removed == 0 {
                continue;
            }
            let sleepiest = island
                .bodies
                .iter()
                .map(|&id| sleep_time(records, awake, id))
                .fold(0.0, f64::max);
            if sleepiest >= time_to_sleep && best.is_none_or(|(time, _)| sleepiest > time) {
                best = Some((sleepiest, slot as IslandId));
            }
        }
        best.map(|(_, island)| island)
    }

    /// Splits awake island `island` into its connected parts, given every
    /// touching contact between two of its bodies as `edges`. The first part —
    /// the one holding the island's first body — keeps the slot; each other
    /// part is a new island, in the order its first body appears.
    pub(crate) fn split(
        &mut self,
        records: &mut Pool<BodyRecord>,
        island: IslandId,
        edges: &[(BodyId, BodyId)],
    ) {
        let Some(Some(entry)) = self.slots.get_mut(island as usize) else {
            return;
        };
        let bodies = std::mem::take(&mut entry.bodies);
        entry.removed = 0;
        let local: HashMap<BodyId, usize> =
            bodies.iter().enumerate().map(|(k, &id)| (id, k)).collect();

        // Union-find with path halving.
        let mut parent: Vec<usize> = (0..bodies.len()).collect();
        let find = |parent: &mut Vec<usize>, mut k: usize| {
            while parent[k] != k {
                parent[k] = parent[parent[k]];
                k = parent[k];
            }
            k
        };
        for (a, b) in edges {
            let (Some(&a), Some(&b)) = (local.get(a), local.get(b)) else {
                continue;
            };
            let (ra, rb) = (find(&mut parent, a), find(&mut parent, b));
            if ra != rb {
                parent[ra.max(rb)] = ra.min(rb);
            }
        }

        let mut part_of_root = vec![usize::MAX; bodies.len()];
        let mut parts: Vec<Vec<BodyId>> = Vec::new();
        for (k, &id) in bodies.iter().enumerate() {
            let root = find(&mut parent, k);
            if part_of_root[root] == usize::MAX {
                part_of_root[root] = parts.len();
                parts.push(Vec::new());
            }
            parts[part_of_root[root]].push(id);
        }

        let mut parts = parts.into_iter();
        if let Some(first) = parts.next()
            && let Some(Some(entry)) = self.slots.get_mut(island as usize)
        {
            entry.bodies = first;
        }
        for part in parts {
            let id = self.allocate(Island {
                bodies: Vec::new(),
                removed: 0,
                asleep: None,
            });
            for &body in &part {
                if let Some(record) = records.get_mut(body) {
                    record.island = Some(id);
                }
            }
            if let Some(Some(entry)) = self.slots.get_mut(id as usize) {
                entry.bodies = part;
            }
        }
    }

    /// Puts to sleep every awake island that needs no split and all of whose
    /// bodies have been slow for `time_to_sleep`, in slot order.
    pub(crate) fn sleep_ready(
        &mut self,
        records: &mut Pool<BodyRecord>,
        awake: &mut AwakeSet,
        time_to_sleep: f64,
    ) {
        for slot in 0..self.slots.len() {
            let ready = self.slots[slot].as_ref().is_some_and(|island| {
                island.asleep.is_none()
                    && island.removed == 0
                    && island
                        .bodies
                        .iter()
                        .all(|&id| sleep_time(records, awake, id) >= time_to_sleep)
            });
            if ready {
                self.sleep(records, awake, slot as IslandId);
            }
        }
    }

    /// A new island holding only `id`.
    fn create(&mut self, id: BodyId) -> IslandId {
        self.allocate(Island {
            bodies: vec![id],
            removed: 0,
            asleep: None,
        })
    }

    fn allocate(&mut self, island: Island) -> IslandId {
        self.live += 1;
        match self.free.pop() {
            Some(slot) => {
                self.slots[slot as usize] = Some(island);
                slot
            }
            None => {
                self.slots.push(Some(island));
                (self.slots.len() - 1) as IslandId
            }
        }
    }
}

/// Adds `dt` to the sleep timer of every awake dynamic body moving slower than
/// `sleep_speed`, turning slower than `sleep_angular_speed`, and whose
/// farthest point — `extent_of` its id, times its angular speed, on top of its
/// linear speed — moves slower than `sleep_speed` too, and resets every other
/// body's.
///
/// The last is Box2D v3's test (`b2FinalizeBodies`): it bounds the fastest
/// point of a body whatever its size, where angular speed alone lets a body
/// reaching well past a metre sleep while its rim still moves visibly. The
/// angular threshold stays, so a body with no collider, which has no extent,
/// cannot sleep while it spins.
pub(crate) fn update_timers(
    awake: &mut AwakeSet,
    sleep_speed: f64,
    sleep_angular_speed: f64,
    extent_of: impl Fn(BodyId) -> f64,
    dt: f64,
) {
    let speed = sleep_speed * sleep_speed;
    let turn = sleep_angular_speed * sleep_angular_speed;
    for ((body, time), &id) in awake
        .bodies
        .iter()
        .zip(awake.sleep_times.iter_mut())
        .zip(&awake.ids)
    {
        let slow = body.is_dynamic()
            && body.velocity.length_squared() < speed
            && body.angular_velocity.length_squared() < turn
            && body.velocity.length() + extent_of(id) * body.angular_velocity.length()
                < sleep_speed;
        *time = if slow { *time + dt } else { 0.0 };
    }
}
