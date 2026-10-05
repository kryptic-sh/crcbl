use std::collections::BTreeSet;
use std::fmt;
use std::hash::Hasher;
use std::sync::Arc;

use crcbl_core::time::TimeSource;

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

/// An ordered sequence of systems run each tick.
///
/// Systems are executed in insertion order, one after another, on the calling
/// thread.
///
/// # Declared access and the conflict graph
///
/// Each system declares, at registration, the [`Shared`] resources its tick
/// reads and writes ([`SystemTrait::access`]); a resource is registered first,
/// with [`Schedule::share`]. From those declarations the schedule derives
/// [`Schedule::conflicts`] as each system is added: a write against a write,
/// or a read against a write, of one resource means the later-registered
/// system waits for the earlier. Running in registration order respects every
/// such edge by construction, so the graph changes nothing today; it is what a
/// schedule running independent systems at once would be built on. In debug
/// builds a tick that touches a resource its system did not declare panics,
/// which is what keeps the graph honest — the [`Access`](crate::Access) docs
/// describe the seam and what lies outside it.
///
/// # Tick times
///
/// Given a clock ([`Schedule::set_clock`]), `run` reads it either side of every
/// system's tick and keeps a [`TickTime`] per system, which
/// [`Inspector::collect`](crate::Inspector::collect) reports. Without one —
/// the default — nothing is measured and no clock is read. The times are
/// never part of [`Schedule::hash_state`]; [`TickTime`]'s docs say why that
/// is the whole of their licence to exist.
pub struct Schedule {
    systems: Vec<Box<dyn SystemTrait>>,
    /// Each system's tick times, index for index with `systems`.
    times: Vec<TickWindow>,
    /// Each system's name and declaration as registered, index for index with
    /// `systems`; reference-counted so a running tick's debug check can hold
    /// one.
    declared: Vec<Arc<Declared>>,
    /// The names [`Schedule::share`] registered.
    resources: BTreeSet<String>,
    /// Grouped by `after`, then by `before`, then by resource name.
    conflicts: Vec<Conflict>,
    clock: Option<Box<dyn TimeSource>>,
}

impl Schedule {
    /// Creates an empty schedule with no clock.
    #[must_use]
    pub fn new() -> Self {
        Self {
            systems: Vec::new(),
            times: Vec::new(),
            declared: Vec::new(),
            resources: BTreeSet::new(),
            conflicts: Vec::new(),
            clock: None,
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
    /// [`SystemTrait::access`] and adding its conflicts with every system
    /// before it.
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
        let after = self.systems.len();
        for (before, earlier) in self.declared.iter().enumerate() {
            conflicts_between(before, &earlier.access, after, &access, &mut self.conflicts);
        }
        self.declared.push(Arc::new(Declared {
            system: system.name().to_owned(),
            access,
        }));
        self.systems.push(system);
        self.times.push(TickWindow::default());
        Ok(())
    }

    /// Every pair of systems whose declared access conflicts, one entry per
    /// resource they collide on: the edges of the graph a concurrent schedule
    /// would have to respect.
    ///
    /// Ordered by the later system, then the earlier, then the resource's
    /// name, so the same registrations give the same list on every run.
    #[must_use]
    pub fn conflicts(&self) -> &[Conflict] {
        &self.conflicts
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
    pub fn set_clock(&mut self, clock: Option<Box<dyn TimeSource>>) {
        self.clock = clock;
        for window in &mut self.times {
            window.clear();
        }
    }

    /// Whether [`run`](Self::run) times the systems it ticks.
    #[must_use]
    pub fn is_timed(&self) -> bool {
        self.clock.is_some()
    }

    /// Runs every system's [`SystemTrait::tick`] in order, passing the
    /// schedule's fixed timestep `dt` (seconds) through to each, and with a
    /// clock, timing each one.
    ///
    /// In debug builds each tick runs with its system's declaration as the
    /// thread's running one, which every [`Shared`] access checks.
    pub fn run(&mut self, dt: f64) {
        for (index, system) in self.systems.iter_mut().enumerate() {
            #[cfg(debug_assertions)]
            let _running = crate::access::Running::enter(&self.declared[index]);
            match &self.clock {
                None => system.tick(dt),
                Some(clock) => {
                    let started = clock.elapsed();
                    system.tick(dt);
                    self.times[index].record(clock.elapsed().saturating_sub(started));
                }
            }
        }
    }

    /// Calls [`SystemTrait::sweep`] on every system with the given dead
    /// entities.
    pub fn sweep(&mut self, dead: &[Entity]) {
        for system in &mut self.systems {
            system.sweep(dead);
        }
    }

    /// Calls [`SystemTrait::debug_draw`] on every system.
    pub fn debug_draw(&mut self, ctx: &DebugCtx) {
        for system in &mut self.systems {
            system.debug_draw(ctx);
        }
    }

    /// Number of systems in the schedule.
    #[must_use]
    pub fn len(&self) -> usize {
        self.systems.len()
    }

    /// Whether the schedule is empty.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.systems.is_empty()
    }

    /// Returns `(name, entity_count)` for every system — used by
    /// [`Inspector`](crate::Inspector).
    pub(crate) fn stats(&self) -> impl Iterator<Item = (String, usize)> + '_ {
        self.systems
            .iter()
            .map(|s| (s.name().to_string(), s.entity_count()))
    }

    /// Every system's [`TickTime`], in schedule order: `None` for a system
    /// not timed since the clock was set, and for all of them without one.
    pub(crate) fn tick_times(&self) -> impl Iterator<Item = Option<TickTime>> + '_ {
        self.times.iter().map(TickWindow::read)
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
            .systems
            .iter()
            .map(|system| {
                let mut bytes = ByteSink::default();
                system.hash_state(&mut bytes);
                (system.name(), bytes.0)
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
        self.systems
            .iter()
            .filter(|s| !s.contributes_to_hash())
            .map(|s| s.name())
            .collect()
    }

    /// Iterates the systems in schedule order — used by the server's
    /// snapshot emission to call [`SystemTrait::replicate`] on each.
    pub fn iter(&self) -> impl Iterator<Item = &dyn SystemTrait> {
        self.systems.iter().map(AsRef::as_ref)
    }

    /// Mutably iterates the systems in schedule order — used by game code
    /// (and tests) to reach a concrete system via [`SystemTrait::as_any_mut`].
    pub fn iter_mut(&mut self) -> impl Iterator<Item = &mut (dyn SystemTrait + '_)> + '_ {
        self.systems
            .iter_mut()
            .map(|system| &mut **system as &mut (dyn SystemTrait + '_))
    }
}

impl fmt::Debug for Schedule {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("Schedule")
            .field("system_count", &self.systems.len())
            .field("timed", &self.is_timed())
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
}
