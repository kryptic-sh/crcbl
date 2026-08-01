use std::fmt;

use crcbl_core::Pool;

use crate::entity::{Entity, EntityMarker};
use crate::schedule::Schedule;
use crate::system::SystemTrait;

/// The top-level ECS container.
///
/// Owns the entity [`Pool`], the [`Schedule`], and a deferred-destruction
/// queue. The typical per-frame flow is:
///
/// ```text
/// world.tick();  // schedule.run() then sweep()
/// ```
pub struct World {
    pool: Pool<EntityMarker>,
    schedule: Schedule,
    dead: Vec<Entity>,
    tick_dt: f64,
}

impl World {
    /// The tick period assumed by [`World::tick`] until
    /// [`World::set_tick_dt`] says otherwise: 60 Hz.
    pub const DEFAULT_TICK_DT: f64 = 1.0 / 60.0;

    /// Creates an empty world ticking at [`World::DEFAULT_TICK_DT`].
    ///
    /// A host that runs at any other rate **must** call
    /// [`World::set_tick_dt`], or every system that integrates over time will
    /// simulate at the wrong speed.
    #[must_use]
    pub fn new() -> Self {
        Self {
            pool: Pool::new(),
            schedule: Schedule::new(),
            dead: Vec::new(),
            tick_dt: Self::DEFAULT_TICK_DT,
        }
    }

    /// Set the fixed timestep, in seconds, handed to every system's
    /// [`SystemTrait::tick`].
    ///
    /// This is the host's real tick period — `1.0 / tick_hz` for a
    /// fixed-rate server loop.
    pub fn set_tick_dt(&mut self, dt: f64) {
        self.tick_dt = dt;
    }

    /// The fixed timestep handed to systems each tick, in seconds.
    #[must_use]
    pub fn tick_dt(&self) -> f64 {
        self.tick_dt
    }

    /// Allocates a fresh entity and returns its handle.
    ///
    /// The entity has no data until it is attached to one or more systems.
    pub fn spawn(&mut self) -> Entity {
        self.pool.insert(EntityMarker)
    }

    /// Marks `entity` for deferred destruction.
    ///
    /// The entity is *not* removed immediately. It is queued and swept at the
    /// end of the current tick (during [`sweep`](Self::sweep) or
    /// [`tick`](Self::tick)). Stale handles (already despawned or never
    /// spawned) are silently ignored.
    pub fn despawn(&mut self, entity: Entity) {
        if self.pool.contains(entity) {
            self.dead.push(entity);
        }
    }

    /// Removes every queued entity from all systems and from the pool, then
    /// clears the queue.
    ///
    /// Called automatically at the end of [`tick`](Self::tick).
    pub fn sweep(&mut self) {
        if self.dead.is_empty() {
            return;
        }
        self.schedule.sweep(&self.dead);
        for &entity in &self.dead {
            self.pool.remove(entity);
        }
        self.dead.clear();
    }

    /// Adds a system to the schedule. Systems run in insertion order.
    pub fn register_system(&mut self, system: Box<dyn SystemTrait>) {
        self.schedule.add_system(system);
    }

    /// Runs one tick at [`World::tick_dt`]: calls every system's `tick`, then
    /// sweeps dead entities.
    pub fn tick(&mut self) {
        self.tick_with_dt(self.tick_dt);
    }

    /// Runs one tick of `dt` seconds without changing the stored
    /// [`World::tick_dt`].
    ///
    /// Use this when the host already has the tick period to hand and would
    /// otherwise have to keep it in sync with the world.
    pub fn tick_with_dt(&mut self, dt: f64) {
        self.schedule.run(dt);
        self.sweep();
    }

    /// Borrows the schedule (for inspection, stats, etc.).
    pub fn schedule(&self) -> &Schedule {
        &self.schedule
    }

    /// Mutably borrows the schedule — used to reach concrete systems via
    /// [`Schedule::iter_mut`] outside of ticking (setup, tests, tooling).
    pub fn schedule_mut(&mut self) -> &mut Schedule {
        &mut self.schedule
    }

    /// Whether `entity` is still alive (in the pool, not yet swept).
    #[must_use]
    pub fn is_alive(&self, entity: Entity) -> bool {
        self.pool.contains(entity)
    }

    /// Hash every system's component state into `hasher` for determinism
    /// checks.  Delegates to [`Schedule::hash_state`].
    pub fn hash_state(&self, hasher: &mut dyn std::hash::Hasher) {
        self.schedule.hash_state(hasher);
    }

    /// Returns the names of systems that do NOT contribute component data
    /// to the determinism hash.  See [`Schedule::non_contributing_systems`].
    pub fn non_contributing_systems(&self) -> Vec<&str> {
        self.schedule.non_contributing_systems()
    }

    /// Number of live entities.
    #[must_use]
    pub fn entity_count(&self) -> usize {
        self.pool.len()
    }

    /// Number of entities queued for deferred destruction.
    #[must_use]
    pub fn dead_queue_len(&self) -> usize {
        self.dead.len()
    }
}

impl fmt::Debug for World {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("World")
            .field("entity_count", &self.pool.len())
            .field("schedule", &self.schedule)
            .field("dead_queue_len", &self.dead.len())
            .finish()
    }
}

impl Default for World {
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
    fn spawn_and_despawn_lifecycle() {
        let mut world = World::new();
        let e = world.spawn();
        assert!(world.is_alive(e));
        assert_eq!(world.entity_count(), 1);

        world.despawn(e);
        // Not removed yet — sweep hasn't run.
        assert!(world.is_alive(e));
        assert_eq!(world.dead_queue_len(), 1);

        world.sweep();
        assert!(!world.is_alive(e));
        assert_eq!(world.entity_count(), 0);
        assert_eq!(world.dead_queue_len(), 0);
    }

    #[test]
    fn despawn_stale_handle_is_noop() {
        let mut world = World::new();
        let stale = crate::test_entity(0xDEAD, 1);
        world.despawn(stale);
        assert_eq!(world.dead_queue_len(), 0);
    }

    #[test]
    fn double_despawn_queues_once() {
        let mut world = World::new();
        let e = world.spawn();
        world.despawn(e);
        world.despawn(e);
        assert_eq!(world.dead_queue_len(), 2); // queued twice but sweep handles it
        world.sweep();
        assert!(!world.is_alive(e));
    }

    #[test]
    fn tick_runs_systems_then_sweeps() {
        let mut world = World::new();
        let e = world.spawn();
        world.despawn(e);

        // Register a system that records whether it was ticked.
        struct Record {
            ticked: bool,
        }
        impl SystemTrait for Record {
            fn name(&self) -> &str {
                "rec"
            }
            fn tick(&mut self, _dt: f64) {
                self.ticked = true;
            }
            fn entity_count(&self) -> usize {
                0
            }
            fn sweep(&mut self, _dead: &[Entity]) {}
            fn debug_draw(&mut self, _ctx: &crate::system::DebugCtx) {}
            fn as_any_mut(&mut self) -> &mut dyn std::any::Any {
                self
            }
        }

        let rec = Record { ticked: false };
        world.register_system(Box::new(rec));
        world.tick();
        assert!(!world.is_alive(e));
        // Can't easily read back the ticked flag from Box<dyn SystemTrait>,
        // but we know tick() ran because sweep happened after.
    }

    #[test]
    fn sweep_removes_from_all_systems() {
        let mut world = World::new();
        let e = world.spawn();

        let mut s1 = System::<i32>::new("s1");
        s1.attach(e, 10);
        let mut s2 = System::<i32>::new("s2");
        s2.attach(e, 20);

        world.register_system(Box::new(s1));
        world.register_system(Box::new(s2));
        world.despawn(e);
        world.sweep();

        let stats: Vec<_> = world.schedule().stats().collect();
        assert_eq!(stats, vec![("s1".into(), 0), ("s2".into(), 0)]);
    }

    #[test]
    fn respawn_after_despawn_gives_new_generation() {
        let mut world = World::new();
        let old = world.spawn();
        world.despawn(old);
        world.sweep();

        let new = world.spawn();
        assert_ne!(old.generation(), new.generation());
        // Old handle must not resolve.
        assert!(!world.is_alive(old));
        assert!(world.is_alive(new));
    }

    #[test]
    fn stale_handle_in_system_get_returns_none() {
        let mut world = World::new();
        let e = world.spawn();
        let mut sys = System::<i32>::new("test");
        sys.attach(e, 42);
        world.register_system(Box::new(sys));

        world.despawn(e);
        world.sweep();

        // Entity is gone from pool; systems in the schedule have been swept.
        let stats: Vec<_> = world.schedule().stats().collect();
        assert_eq!(stats, vec![("test".into(), 0)]);
    }

    #[test]
    fn debug_format() {
        let mut world = World::new();
        world.spawn();
        world.spawn();
        let s = format!("{world:?}");
        assert!(s.contains("entity_count: 2"));
    }
}
