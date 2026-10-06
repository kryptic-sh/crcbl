use std::collections::BTreeSet;
use std::fmt;
use std::hash::Hasher;
use std::ops::Range;
use std::sync::Arc;

use crcbl_core::time::TimeSource;
use crcbl_jobs::Pool;

use crate::access::{AccessError, Conflict, Declared, conflicts_between};
use crate::entity::Entity;
use crate::shared::Shared;
use crate::system::{DebugCtx, SystemTrait};
use crate::tick_time::{TickTime, TickWindow};

/// A [`Hasher`] that keeps the bytes instead of mixing them.
///
/// [`Schedule::hash_state`] needs each system's contribution as a value it can
/// sort, which a one-way hash cannot give it. `finish` is never meaningful and
/// returns zero.
#[derive(Default)]
struct ByteSink(Vec<u8>);

impl Hasher for ByteSink {
    fn write(&mut self, bytes: &[u8]) {
        self.0.extend_from_slice(bytes);
    }

    fn finish(&self) -> u64 {
        0
    }
}

/// A clock a schedule times its systems on: shared with the pool's workers
/// when the systems tick there, hence `Sync`, and `Send` so a world that owns
/// one can move to the thread that runs it.
pub type ScheduleClock = Box<dyn TimeSource + Send + Sync>;

/// One registered system and what the schedule keeps beside it.
///
/// Kept together so a stage of the schedule is one contiguous `&mut` slice,
/// which is the shape [`Pool::par_for`] splits.
struct Slot {
    system: Box<dyn SystemTrait>,
    /// The system's tick times.
    times: TickWindow,
    /// The system's name and declaration as registered; reference-counted so
    /// a running tick's debug check can hold one.
    declared: Arc<Declared>,
}

impl Slot {
    /// Runs the system's tick, timed on `clock` when there is one, with its
    /// declaration as this thread's running one in debug builds.
    fn tick(&mut self, clock: Option<&(dyn TimeSource + Send + Sync)>, dt: f64) {
        #[cfg(debug_assertions)]
        let _running = crate::access::Running::enter(&self.declared);
        match clock {
            None => self.system.tick(dt),
            Some(clock) => {
                let started = clock.elapsed();
                self.system.tick(dt);
                self.times.record(clock.elapsed().saturating_sub(started));
            }
        }
    }
}

/// An ordered sequence of systems run each tick.
///
/// Without a pool — the default — systems are executed in insertion order, one
/// after another, on the calling thread.
///
/// # Declared access and the conflict graph
///
/// Each system declares, at registration, the [`Shared`] resources its tick
/// reads and writes ([`SystemTrait::access`]); a resource is registered first,
/// with [`Schedule::share`]. From those declarations the schedule derives
/// [`Schedule::conflicts`] as each system is added: a write against a write,
/// or a read against a write, of one resource means the later-registered
/// system waits for the earlier. In debug builds a tick that touches a
/// resource its system did not declare panics, which is what keeps the graph
/// honest — the [`Access`](crate::Access) docs describe the seam and what lies
/// outside it.
///
/// # Stages, and running them on a pool
///
/// The systems are also grouped, at registration, into
/// [`stages`](Schedule::stages): runs of consecutive systems no two of which
/// conflict. A system starts a new stage when it conflicts with any system in
/// the stage before it; otherwise it joins that stage. So every conflict's
/// earlier system is in an earlier stage than its later one, and running the
/// stages in order — each one finished before the next starts — keeps every
/// order a conflict protects.
///
/// Handed a [`Pool`] ([`Schedule::set_pool`]), `run` ticks each stage's
/// systems across the pool's workers and the calling thread, with the end of
/// [`Pool::par_for`] as the barrier between stages. The answer is the serial
/// one bit for bit: systems in one stage touch nothing in common except
/// resources they all only read, so the order they ran in cannot show. Only
/// ticks run there. [`sweep`](Schedule::sweep),
/// [`debug_draw`](Schedule::debug_draw), [`hash_state`](Schedule::hash_state)
/// and everything [`iter`](Schedule::iter) hands out run on the calling
/// thread in schedule order, pool or not.
///
/// **Two things do differ under a pool, and neither is state.** A system that
/// logs from its tick logs in completion order rather than schedule order. And
/// a tick that panics no longer stops the systems after it in its stage: they
/// run, and the lowest-placed panic is re-raised once the stage is done —
/// [`Pool::par_for`]'s rule — where serially the systems after it would not
/// have ticked.
///
/// # Tick times
///
/// Given a clock ([`Schedule::set_clock`]), `run` reads it either side of every
/// system's tick and keeps a [`TickTime`] per system, which
/// [`Inspector::collect`](crate::Inspector::collect) reports. Without one —
/// the default — nothing is measured and no clock is read. Under a pool the
/// clock is read on whichever thread ran the system, so each time is that
/// system's own tick and not its stage's. The times are never part of
/// [`Schedule::hash_state`]; [`TickTime`]'s docs say why that is the whole of
/// their licence to exist.
pub struct Schedule {
    slots: Vec<Slot>,
    /// The names [`Schedule::share`] registered.
    resources: BTreeSet<String>,
    /// Grouped by `after`, then by `before`, then by resource name.
    conflicts: Vec<Conflict>,
    /// Consecutive, ascending and covering every slot, in schedule order.
    stages: Vec<Range<usize>>,
    clock: Option<ScheduleClock>,
    pool: Option<Pool>,
}

impl Schedule {
    /// Creates an empty schedule with no clock and no pool.
    #[must_use]
    pub fn new() -> Self {
        Self {
            slots: Vec::new(),
            resources: BTreeSet::new(),
            conflicts: Vec::new(),
            stages: Vec::new(),
            clock: None,
            pool: None,
        }
    }

    /// Registers `shared` under its name, so a system's
    /// [`SystemTrait::access`] may name it.
    ///
    /// # Errors
    ///
    /// [`AccessError::DuplicateResource`] when a resource of that name is
    /// already registered: names are how declarations tell resources apart.
    pub fn share<T>(&mut self, shared: &Shared<T>) -> Result<(), AccessError> {
        if self.resources.insert(shared.name().to_owned()) {
            Ok(())
        } else {
            Err(AccessError::DuplicateResource {
                resource: shared.name().to_owned(),
            })
        }
    }

    /// Appends a system to the end of the schedule.
    ///
    /// # Panics
    ///
    /// When [`Schedule::try_add_system`] refuses it — its declaration names a
    /// resource not registered here — with the refusal's message, which names
    /// the system and the resource. A declaration is code, so a bad one is a
    /// bug the first run finds.
    pub fn add_system(&mut self, system: Box<dyn SystemTrait>) {
        if let Err(error) = self.try_add_system(system) {
            panic!("{error}");
        }
    }

    /// Appends a system to the end of the schedule, asking it its
    /// [`SystemTrait::access`], adding its conflicts with every system before
    /// it, and placing it in the last stage or a new one.
    ///
    /// # Errors
    ///
    /// [`AccessError::UnknownResource`] when the declaration names a resource
    /// no [`Schedule::share`] registered — the first such in name order. The
    /// schedule is left as it was.
    pub fn try_add_system(&mut self, system: Box<dyn SystemTrait>) -> Result<(), AccessError> {
        let access = system.access();
        if let Some(unknown) = access
            .touched()
            .find(|resource| !self.resources.contains(*resource))
        {
            return Err(AccessError::UnknownResource {
                system: system.name().to_owned(),
                resource: unknown.to_owned(),
            });
        }
        let after = self.slots.len();
        let first_new = self.conflicts.len();
        for (before, earlier) in self.slots.iter().enumerate() {
            conflicts_between(
                before,
                &earlier.declared.access,
                after,
                &access,
                &mut self.conflicts,
            );
        }
        match self.stages.last_mut() {
            Some(stage)
                if !self.conflicts[first_new..]
                    .iter()
                    .any(|conflict| stage.contains(&conflict.before)) =>
            {
                stage.end = after + 1;
            }
            _ => self.stages.push(after..after + 1),
        }
        self.slots.push(Slot {
            declared: Arc::new(Declared {
                #[cfg(debug_assertions)]
                system: system.name().to_owned(),
                access,
            }),
            system,
            times: TickWindow::default(),
        });
        Ok(())
    }

    /// Every pair of systems whose declared access conflicts, one entry per
    /// resource they collide on: the edges the [`stages`](Self::stages)
    /// respect.
    ///
    /// Ordered by the later system, then the earlier, then the resource's
    /// name, so the same registrations give the same list on every run.
    #[must_use]
    pub fn conflicts(&self) -> &[Conflict] {
        &self.conflicts
    }

    /// The schedule's stages, as ranges of schedule positions in order: each a
    /// run of consecutive systems no two of which conflict, and every system
    /// in exactly one. See the [type docs](Self).
    #[must_use]
    pub fn stages(&self) -> &[Range<usize>] {
        &self.stages
    }

    /// The systems the one at `index` must run after — each registered before
    /// it with a declaration that conflicts with its own — once each, in
    /// schedule order.
    pub(crate) fn runs_after(&self, index: usize) -> Vec<usize> {
        let mut before: Vec<usize> = self
            .conflicts
            .iter()
            .filter(|conflict| conflict.after == index)
            .map(|conflict| conflict.before)
            .collect();
        // Grouped by `before` within one `after`, so repeats are adjacent.
        before.dedup();
        before
    }

    /// Ticks each stage's systems on `pool` from the next [`run`](Self::run),
    /// or on the calling thread alone with `None`, handing back the pool it
    /// had.
    ///
    /// The schedule owns the pool because [`Pool::par_for`] takes `&mut self`:
    /// one thread drives a pool at a time, and while the schedule runs, that
    /// thread is the one running it. A pool with no workers — built on
    /// [`Inline`](crcbl_jobs::Inline), as in a browser without threads — runs
    /// every system on the calling thread in schedule order, as no pool does.
    pub fn set_pool(&mut self, pool: Option<Pool>) -> Option<Pool> {
        std::mem::replace(&mut self.pool, pool)
    }

    /// Times every system's tick on `clock` from the next [`run`](Self::run),
    /// or stops timing them with `None`. Either way the times measured so far
    /// are dropped, since they were taken on a clock that is no longer this
    /// schedule's.
    ///
    /// Natively the clock is a
    /// [`MonotonicTime`](crcbl_core::time::MonotonicTime); a test hands a
    /// clock it advances itself. **A browser build has no clock to hand**:
    /// `MonotonicTime` reads [`std::time::Instant`], which panics on
    /// `wasm32-unknown-unknown`, and the page's only clock is the
    /// `performance.now()` its shim passes in once a frame, which cannot
    /// time anything inside one. Such a schedule stays untimed and its
    /// systems report no [`TickTime`] rather than a zero.
    pub fn set_clock(&mut self, clock: Option<ScheduleClock>) {
        self.clock = clock;
        for slot in &mut self.slots {
            slot.times.clear();
        }
    }

    /// Whether [`run`](Self::run) times the systems it ticks.
    #[must_use]
    pub fn is_timed(&self) -> bool {
        self.clock.is_some()
    }

    /// Runs every system's [`SystemTrait::tick`], passing the schedule's fixed
    /// timestep `dt` (seconds) through to each, and with a clock, timing each
    /// one: in schedule order on the calling thread, or a stage at a time
    /// across the pool given to [`set_pool`](Self::set_pool).
    ///
    /// In debug builds each tick runs with its system's declaration as its
    /// thread's running one, which every [`Shared`] access checks.
    ///
    /// # Panics
    ///
    /// When a system's tick does — under a pool, after the rest of its stage
    /// has run; see the [type docs](Self).
    pub fn run(&mut self, dt: f64) {
        let clock = self.clock.as_deref();
        match &mut self.pool {
            None => {
                for slot in &mut self.slots {
                    slot.tick(clock, dt);
                }
            }
            Some(pool) => {
                for stage in &self.stages {
                    // One system per chunk: the split is the stage's own and
                    // never the worker count's.
                    pool.par_for(&mut self.slots[stage.clone()], 1, |_, slots| {
                        for slot in slots {
                            slot.tick(clock, dt);
                        }
                    });
                }
            }
        }
    }

    /// Calls [`SystemTrait::sweep`] on every system with the given dead
    /// entities.
    pub fn sweep(&mut self, dead: &[Entity]) {
        for slot in &mut self.slots {
            slot.system.sweep(dead);
        }
    }

    /// Calls [`SystemTrait::debug_draw`] on every system.
    pub fn debug_draw(&mut self, ctx: &DebugCtx) {
        for slot in &mut self.slots {
            slot.system.debug_draw(ctx);
        }
    }

    /// Number of systems in the schedule.
    #[must_use]
    pub fn len(&self) -> usize {
        self.slots.len()
    }

    /// Whether the schedule is empty.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.slots.is_empty()
    }

    /// Returns `(name, entity_count)` for every system — used by
    /// [`Inspector`](crate::Inspector).
    pub(crate) fn stats(&self) -> impl Iterator<Item = (String, usize)> + '_ {
        self.slots
            .iter()
            .map(|slot| (slot.system.name().to_string(), slot.system.entity_count()))
    }

    /// Every system's [`TickTime`], in schedule order: `None` for a system
    /// not timed since the clock was set, and for all of them without one.
    pub(crate) fn tick_times(&self) -> impl Iterator<Item = Option<TickTime>> + '_ {
        self.slots.iter().map(|slot| slot.times.read())
    }

    /// Hash every system's state (name + component data) into `hasher`,
    /// independently of the order the systems were registered in.
    ///
    /// Each system's state is hashed into its own byte buffer first, then the
    /// `(name, bytes)` pairs are sorted together and length-delimited into
    /// `hasher`. Sorting on the name alone would not be enough: names are not
    /// required to be unique, and ties would fall back on registration order.
    /// The length prefixes stop concatenation ambiguity — without them a
    /// system called `"ab"` with no data is indistinguishable from one called
    /// `"a"` whose first data byte is `b'b'`.
    pub fn hash_state(&self, hasher: &mut dyn Hasher) {
        let mut entries: Vec<(&str, Vec<u8>)> = self
            .slots
            .iter()
            .map(|slot| {
                let mut bytes = ByteSink::default();
                slot.system.hash_state(&mut bytes);
                (slot.system.name(), bytes.0)
            })
            .collect();
        entries.sort_unstable();

        for (name, bytes) in &entries {
            hasher.write(&(name.len() as u64).to_le_bytes());
            hasher.write(name.as_bytes());
            hasher.write(&(bytes.len() as u64).to_le_bytes());
            hasher.write(bytes);
        }
    }

    /// Returns the names of systems that do NOT contribute component data
    /// to the determinism hash (their [`SystemTrait::contributes_to_hash`]
    /// returns `false`).
    ///
    /// These systems are likely custom `SystemTrait` implementations that
    /// forgot to override [`SystemTrait::hash_state`]; the determinism harness
    /// should warn about them.
    pub fn non_contributing_systems(&self) -> Vec<&str> {
        self.slots
            .iter()
            .filter(|slot| !slot.system.contributes_to_hash())
            .map(|slot| slot.system.name())
            .collect()
    }

    /// Iterates the systems in schedule order — used by the server's
    /// snapshot emission to call [`SystemTrait::replicate`] on each.
    pub fn iter(&self) -> impl Iterator<Item = &dyn SystemTrait> {
        self.slots.iter().map(|slot| slot.system.as_ref())
    }

    /// Mutably iterates the systems in schedule order — used by game code
    /// (and tests) to reach a concrete system via [`SystemTrait::as_any_mut`].
    pub fn iter_mut(&mut self) -> impl Iterator<Item = &mut (dyn SystemTrait + '_)> + '_ {
        self.slots
            .iter_mut()
            .map(|slot| &mut *slot.system as &mut (dyn SystemTrait + '_))
    }
}

impl fmt::Debug for Schedule {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("Schedule")
            .field("system_count", &self.slots.len())
            .field("stages", &self.stages.len())
            .field("timed", &self.is_timed())
            .field("pool_workers", &self.pool.as_ref().map(Pool::workers))
            .finish()
    }
}

impl Default for Schedule {
    fn default() -> Self {
        Self::new()
    }
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use crate::access::Access;
    use crate::system::System;

    #[test]
    fn runs_systems_in_insertion_order() {
        let mut schedule = Schedule::new();
        let order = Shared::new("order", Vec::new());
        schedule.share(&order).expect("the only resource");

        struct Probe {
            id: usize,
            order: Shared<Vec<usize>>,
        }
        impl SystemTrait for Probe {
            fn name(&self) -> &str {
                "probe"
            }
            fn access(&self) -> Access {
                Access::none().writes("order")
            }
            fn tick(&mut self, _dt: f64) {
                self.order.write().push(self.id);
            }
            fn entity_count(&self) -> usize {
                0
            }
            fn sweep(&mut self, _dead: &[Entity]) {}
            fn debug_draw(&mut self, _ctx: &DebugCtx) {}
            fn as_any_mut(&mut self) -> &mut dyn std::any::Any {
                self
            }
        }

        schedule.add_system(Box::new(Probe {
            id: 0,
            order: order.clone(),
        }));
        schedule.add_system(Box::new(Probe {
            id: 1,
            order: order.clone(),
        }));
        schedule.add_system(Box::new(Probe {
            id: 2,
            order: order.clone(),
        }));

        schedule.run(1.0 / 60.0);
        assert_eq!(*order.read(), vec![0, 1, 2]);
    }

    #[test]
    fn run_passes_the_schedule_dt_to_every_system() {
        struct Recorder(Shared<Vec<f64>>);
        impl SystemTrait for Recorder {
            fn name(&self) -> &str {
                "recorder"
            }
            fn access(&self) -> Access {
                Access::none().writes("seen")
            }
            fn tick(&mut self, dt: f64) {
                self.0.write().push(dt);
            }
            fn entity_count(&self) -> usize {
                0
            }
            fn sweep(&mut self, _dead: &[Entity]) {}
            fn debug_draw(&mut self, _ctx: &DebugCtx) {}
            fn as_any_mut(&mut self) -> &mut dyn std::any::Any {
                self
            }
        }

        let seen = Shared::new("seen", Vec::new());
        let mut schedule = Schedule::new();
        schedule.share(&seen).expect("the only resource");
        schedule.add_system(Box::new(Recorder(seen.clone())));
        schedule.add_system(Box::new(Recorder(seen.clone())));

        schedule.run(1.0 / 30.0);
        assert_eq!(*seen.read(), vec![1.0 / 30.0, 1.0 / 30.0]);
    }

    #[test]
    fn hash_state_is_unambiguous_across_name_and_data_boundaries() {
        use std::collections::hash_map::DefaultHasher;

        fn hash(schedule: &Schedule) -> u64 {
            let mut h = DefaultHasher::new();
            schedule.hash_state(&mut h);
            h.finish()
        }

        // System "ab" with no data must not collide with system "a" whose
        // first data byte is b'b'.
        let mut ab = Schedule::new();
        ab.add_system(Box::new(System::<u8>::new("ab")));

        let mut a_with_b = Schedule::new();
        let mut sys = System::<u8>::new("a");
        sys.attach(crate::test_entity(1, 1), b'b');
        a_with_b.add_system(Box::new(sys));

        assert_ne!(hash(&ab), hash(&a_with_b));
    }

    #[test]
    fn hash_state_ignores_registration_order_of_same_named_systems() {
        use std::collections::hash_map::DefaultHasher;

        fn build(first: i32, second: i32) -> Schedule {
            let mut schedule = Schedule::new();
            for value in [first, second] {
                let mut sys = System::<i32>::new("dup");
                sys.attach(crate::test_entity(1, 1), value);
                schedule.add_system(Box::new(sys));
            }
            schedule
        }

        fn hash(schedule: &Schedule) -> u64 {
            let mut h = DefaultHasher::new();
            schedule.hash_state(&mut h);
            h.finish()
        }

        // Sorting on the name alone would leave these two at the mercy of
        // registration order.
        assert_eq!(hash(&build(1, 2)), hash(&build(2, 1)));
        assert_ne!(hash(&build(1, 2)), hash(&build(1, 3)));
    }

    #[test]
    fn sweep_propagates_to_all_systems() {
        let mut schedule = Schedule::new();
        let e = crate::test_entity(1, 1);

        let mut s1 = System::<i32>::new("s1");
        s1.attach(e, 42);
        let mut s2 = System::<i32>::new("s2");
        s2.attach(e, 99);

        schedule.add_system(Box::new(s1));
        schedule.add_system(Box::new(s2));

        schedule.sweep(&[e]);
        // Both systems should have removed the entity.
        assert_eq!(
            schedule.stats().map(|(_, c)| c).collect::<Vec<_>>(),
            vec![0, 0]
        );
    }

    /// A schedule with nothing in it stays that way through a full frame's
    /// worth of calls.
    ///
    /// Each of the three used to be asserted by "did not panic", which is also
    /// what a `run` that registered a system of its own would have managed.
    #[test]
    fn a_frame_against_an_empty_schedule_leaves_it_empty() {
        let mut schedule = Schedule::new();
        schedule.run(1.0 / 60.0);
        schedule.sweep(&[]);
        schedule.debug_draw(&DebugCtx);

        assert!(schedule.is_empty());
        assert_eq!(schedule.len(), 0);
        assert_eq!(schedule.stats().collect::<Vec<_>>(), Vec::new());
        assert_eq!(schedule.iter().count(), 0);
    }

    #[test]
    fn stats_returns_correct_counts() {
        let mut schedule = Schedule::new();
        let mut s1 = System::<i32>::new("a");
        s1.attach(crate::test_entity(1, 1), 1);
        s1.attach(crate::test_entity(2, 1), 2);

        let mut s2 = System::<i32>::new("b");
        s2.attach(crate::test_entity(3, 1), 3);

        schedule.add_system(Box::new(s1));
        schedule.add_system(Box::new(s2));

        let stats: Vec<_> = schedule.stats().collect();
        assert_eq!(stats, vec![("a".into(), 2), ("b".into(), 1)]);
    }

    // -- stages and the pool --------------------------------------------------

    /// A system that steps one value, reading one resource and writing
    /// another as it is told — enough to build any shape of conflict graph.
    ///
    /// A writer blends its value into what the resource held, so two writers'
    /// order shows in what a later reader sees; a reader folds what it read
    /// into its value, so whether it ran before or after a writer shows in its
    /// own hash. The step grows the value and wraps it at [`Lane::WRAP`], so it
    /// never settles: a schedule whose values converged would hash alike
    /// whatever order its systems had run in.
    struct Lane {
        name: &'static str,
        value: f64,
        reads: Option<Shared<f64>>,
        writes: Option<Shared<f64>>,
    }

    impl Lane {
        /// Where a value wraps, keeping it bounded without letting it settle.
        const WRAP: f64 = 997.0;

        fn new(
            name: &'static str,
            reads: Option<&Shared<f64>>,
            writes: Option<&Shared<f64>>,
        ) -> Self {
            Self {
                name,
                value: name.len() as f64,
                reads: reads.cloned(),
                writes: writes.cloned(),
            }
        }
    }

    impl SystemTrait for Lane {
        fn name(&self) -> &str {
            self.name
        }
        fn access(&self) -> Access {
            let mut access = Access::none();
            if let Some(read) = &self.reads {
                access = access.reads(read.name());
            }
            if let Some(written) = &self.writes {
                access = access.writes(written.name());
            }
            access
        }
        fn tick(&mut self, dt: f64) {
            let input = self.reads.as_ref().map_or(0.0, |read| *read.read());
            self.value = (1.5 * self.value + input + dt) % Self::WRAP;
            if let Some(written) = &self.writes {
                let mut value = written.write();
                *value = (0.5 * *value + self.value) % Self::WRAP;
            }
        }
        fn entity_count(&self) -> usize {
            1
        }
        fn sweep(&mut self, _dead: &[Entity]) {}
        fn debug_draw(&mut self, _ctx: &DebugCtx) {}
        fn hash_state(&self, hasher: &mut dyn Hasher) {
            hasher.write_u64(self.value.to_bits());
        }
        fn contributes_to_hash(&self) -> bool {
            true
        }
        fn as_any_mut(&mut self) -> &mut dyn std::any::Any {
            self
        }
    }

    /// A schedule of [`Lane`]s with every kind of edge in it: readers after a
    /// writer, two writers of one resource, a reader of one resource that
    /// writes another, and systems that touch nothing between them.
    fn mixed() -> Schedule {
        let wind = Shared::new("wind", 0.0);
        let score = Shared::new("score", 0.0);
        let mut schedule = Schedule::new();
        schedule.share(&wind).expect("distinct names");
        schedule.share(&score).expect("distinct names");
        for lane in [
            Lane::new("gust", None, Some(&wind)),
            Lane::new("drift", Some(&wind), None),
            Lane::new("spin", None, None),
            Lane::new("drag", Some(&wind), None),
            Lane::new("tally", Some(&wind), Some(&score)),
            Lane::new("decay", None, None),
            Lane::new("audit", None, Some(&score)),
            Lane::new("pulse", Some(&score), None),
            Lane::new("calm", None, Some(&wind)),
            Lane::new("rest", None, None),
        ] {
            schedule.add_system(Box::new(lane));
        }
        schedule
    }

    fn stage_of(schedule: &Schedule, index: usize) -> usize {
        schedule
            .stages()
            .iter()
            .position(|stage| stage.contains(&index))
            .unwrap_or_else(|| panic!("system {index} is in no stage"))
    }

    /// **No stage holds two systems that conflict**: every conflict's earlier
    /// system sits in a stage strictly before its later one's.
    #[test]
    fn conflicting_systems_never_share_a_stage() {
        let schedule = mixed();
        assert!(!schedule.conflicts().is_empty());
        for conflict in schedule.conflicts() {
            assert!(
                stage_of(&schedule, conflict.before) < stage_of(&schedule, conflict.after),
                "{conflict:?} shares a stage: {:?}",
                schedule.stages()
            );
        }
        // And the stages are the greedy ones, not merely legal: a system joins
        // the stage before it unless it conflicts with something in it.
        assert_eq!(schedule.stages(), [0..1, 1..6, 6..7, 7..10]);
    }

    /// **The stages keep registration order**: consecutive ranges, ascending,
    /// covering every system once — so running them in order runs every
    /// conflicting pair in the order it was registered.
    #[test]
    fn the_stages_cover_the_schedule_in_registration_order() {
        let schedule = mixed();
        let mut next = 0;
        for stage in schedule.stages() {
            assert_eq!(stage.start, next, "{:?}", schedule.stages());
            assert!(stage.end > stage.start, "{:?}", schedule.stages());
            next = stage.end;
        }
        assert_eq!(next, schedule.len());

        // A schedule of systems that touch nothing is one stage.
        let mut free = Schedule::new();
        for name in ["a", "b", "c"] {
            free.add_system(Box::new(Lane::new(name, None, None)));
        }
        assert_eq!(free.stages().len(), 1);
        assert_eq!(free.stages()[0], 0..3);
    }

    fn hash(schedule: &Schedule) -> u64 {
        let mut hasher = std::collections::hash_map::DefaultHasher::new();
        schedule.hash_state(&mut hasher);
        hasher.finish()
    }

    fn pool(workers: usize) -> Pool {
        Pool::with_workers(crcbl_jobs::default_spawner().as_ref(), workers).expect("a pool")
    }

    /// **A pool changes nothing about the answer**: [`mixed`] ticked serially,
    /// on a pool with no workers, and on pools of one and seven workers hashes
    /// the same after every tick.
    #[test]
    fn a_schedule_on_a_pool_hashes_as_it_does_serially_after_every_tick() {
        const TICKS: usize = 200;
        let run = |pool: Option<Pool>| {
            let mut schedule = mixed();
            schedule.set_pool(pool);
            (0..TICKS)
                .map(|_| {
                    schedule.run(1.0 / 60.0);
                    hash(&schedule)
                })
                .collect::<Vec<_>>()
        };
        let serial = run(None);
        assert!(serial.windows(2).all(|pair| pair[0] != pair[1]));
        for workers in [0, 1, 7] {
            assert_eq!(run(Some(pool(workers))), serial, "{workers} workers");
        }
    }

    /// Two systems that each wait for the other to arrive, or give up after
    /// [`Meet::PATIENCE`]: they can only both meet if they tick at once.
    struct Meet {
        name: &'static str,
        arrived: Arc<std::sync::atomic::AtomicUsize>,
        met: bool,
    }

    impl Meet {
        /// Far longer than a worker takes to wake, and short enough that the
        /// serial schedule this test exists to tell apart fails promptly.
        const PATIENCE: std::time::Duration = std::time::Duration::from_secs(5);
    }

    impl SystemTrait for Meet {
        fn name(&self) -> &str {
            self.name
        }
        fn access(&self) -> Access {
            Access::none()
        }
        fn tick(&mut self, _dt: f64) {
            use std::sync::atomic::Ordering;
            self.arrived.fetch_add(1, Ordering::SeqCst);
            let deadline = std::time::Instant::now() + Self::PATIENCE;
            while self.arrived.load(Ordering::SeqCst) < 2 && std::time::Instant::now() < deadline {
                std::thread::yield_now();
            }
            self.met = self.arrived.load(Ordering::SeqCst) >= 2;
        }
        fn entity_count(&self) -> usize {
            0
        }
        fn sweep(&mut self, _dead: &[Entity]) {}
        fn debug_draw(&mut self, _ctx: &DebugCtx) {}
        fn as_any_mut(&mut self) -> &mut dyn std::any::Any {
            self
        }
    }

    /// **A stage really runs at once on a pool**: two systems that can only
    /// finish together do — which a schedule that kept ticking one at a time
    /// with a pool in hand could not manage.
    #[test]
    fn a_pool_ticks_the_systems_of_one_stage_at_once() {
        let arrived = Arc::new(std::sync::atomic::AtomicUsize::new(0));
        let mut schedule = Schedule::new();
        for name in ["left", "right"] {
            schedule.add_system(Box::new(Meet {
                name,
                arrived: Arc::clone(&arrived),
                met: false,
            }));
        }
        assert_eq!(schedule.stages().len(), 1, "both meets share one stage");
        schedule.set_pool(Some(pool(1)));
        schedule.run(1.0 / 60.0);
        let met: Vec<bool> = schedule
            .iter_mut()
            .map(|system| {
                system
                    .as_any_mut()
                    .downcast_mut::<Meet>()
                    .expect("both are meets")
                    .met
            })
            .collect();
        assert_eq!(met, [true, true]);
    }

    /// A system whose tick takes at least [`Slow::COST`].
    struct Slow(&'static str);

    impl Slow {
        const COST: std::time::Duration = std::time::Duration::from_millis(2);
    }

    impl SystemTrait for Slow {
        fn name(&self) -> &str {
            self.0
        }
        fn access(&self) -> Access {
            Access::none()
        }
        fn tick(&mut self, _dt: f64) {
            std::thread::sleep(Self::COST);
        }
        fn entity_count(&self) -> usize {
            0
        }
        fn sweep(&mut self, _dead: &[Entity]) {}
        fn debug_draw(&mut self, _ctx: &DebugCtx) {}
        fn as_any_mut(&mut self) -> &mut dyn std::any::Any {
            self
        }
    }

    /// **The clock still times every system on a pool**, each with its own
    /// tick's length, whichever thread ran it.
    #[test]
    fn a_timed_schedule_on_a_pool_records_every_systems_tick() {
        let mut schedule = Schedule::new();
        for name in ["a", "b", "c", "d"] {
            schedule.add_system(Box::new(Slow(name)));
        }
        schedule.set_clock(Some(Box::new(crcbl_core::time::MonotonicTime::new())));
        schedule.set_pool(Some(pool(3)));
        schedule.run(1.0 / 60.0);
        let times: Vec<Option<TickTime>> = schedule.tick_times().collect();
        assert_eq!(times.len(), 4);
        for (index, time) in times.into_iter().enumerate() {
            let time = time.unwrap_or_else(|| panic!("system {index} was not timed"));
            assert!(time.last >= Slow::COST, "system {index}: {time:?}");
        }
    }
}
