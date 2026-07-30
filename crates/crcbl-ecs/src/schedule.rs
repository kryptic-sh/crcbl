use std::fmt;
use std::hash::Hasher;

use crate::entity::Entity;
use crate::system::{DebugCtx, SystemTrait};

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

    /// Runs every system's [`SystemTrait::tick`] in order.
    pub fn run(&mut self) {
        for system in &mut self.systems {
            system.tick();
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
    /// sorted by name so insertion order does not affect the hash.
    pub fn hash_state(&self, hasher: &mut dyn Hasher) {
        let mut systems: Vec<(&str, &dyn SystemTrait)> = self
            .systems
            .iter()
            .map(|s| (s.name(), s.as_ref()))
            .collect();
        systems.sort_by_key(|(name, _)| *name);
        for (name, system) in &systems {
            hasher.write(name.as_bytes());
            system.hash_state(hasher);
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
            fn tick(&mut self) {
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

        schedule.run();
        assert_eq!(*order.borrow(), vec![0, 1, 2]);
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
        schedule.run();
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
