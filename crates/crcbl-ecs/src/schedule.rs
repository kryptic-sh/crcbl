use std::fmt;
use std::hash::Hasher;

use crate::entity::Entity;
use crate::system::{DebugCtx, SystemTrait};

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
/// Systems are executed in insertion order. No automatic dependency inference
/// is done — ordering is declared by the caller and conflicts are asserted in
/// debug builds.
pub struct Schedule {
    systems: Vec<Box<dyn SystemTrait>>,
}

impl Schedule {
    /// Creates an empty schedule.
    #[must_use]
    pub fn new() -> Self {
        Self {
            systems: Vec::new(),
        }
    }

    /// Appends a system to the end of the schedule.
    pub fn add_system(&mut self, system: Box<dyn SystemTrait>) {
        self.systems.push(system);
    }

    /// Runs every system's [`SystemTrait::tick`] in order, passing the
    /// schedule's fixed timestep `dt` (seconds) through to each.
    pub fn run(&mut self, dt: f64) {
        for system in &mut self.systems {
            system.tick(dt);
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
    use crate::system::System;

    #[test]
    fn runs_systems_in_insertion_order() {
        let mut schedule = Schedule::new();
        let order = std::rc::Rc::new(std::cell::RefCell::new(Vec::new()));

        struct Probe {
            id: usize,
            order: std::rc::Rc<std::cell::RefCell<Vec<usize>>>,
        }
        impl SystemTrait for Probe {
            fn name(&self) -> &str {
                "probe"
            }
            fn tick(&mut self, _dt: f64) {
                self.order.borrow_mut().push(self.id);
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
        assert_eq!(*order.borrow(), vec![0, 1, 2]);
    }

    #[test]
    fn run_passes_the_schedule_dt_to_every_system() {
        struct Recorder(std::rc::Rc<std::cell::RefCell<Vec<f64>>>);
        impl SystemTrait for Recorder {
            fn name(&self) -> &str {
                "recorder"
            }
            fn tick(&mut self, dt: f64) {
                self.0.borrow_mut().push(dt);
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

        let seen = std::rc::Rc::new(std::cell::RefCell::new(Vec::new()));
        let mut schedule = Schedule::new();
        schedule.add_system(Box::new(Recorder(seen.clone())));
        schedule.add_system(Box::new(Recorder(seen.clone())));

        schedule.run(1.0 / 30.0);
        assert_eq!(*seen.borrow(), vec![1.0 / 30.0, 1.0 / 30.0]);
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

    #[test]
    fn empty_schedule_runs_without_panic() {
        let mut schedule = Schedule::new();
        schedule.run(1.0 / 60.0);
        schedule.sweep(&[]);
        schedule.debug_draw(&DebugCtx);
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
