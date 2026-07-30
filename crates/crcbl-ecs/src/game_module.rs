//! Game module API — the seam between engine and game logic.
//!
//! A [`GameModule`] implements game-specific logic: it declares systems and
//! component schemas at init, then drives per-tick gameplay decisions on the
//! [`World`].
//!
//! # Two bindings (static + wasm)
//!
//! This trait is the **static binding**: game logic compiled directly into the
//! binary. Every sample (breakout, asteroids, …) implements it. The **wasm
//! binding** (P6A) exposes the same semantics over an FFI ABI, and the
//! determinism harness proves `hash(static) == hash(wasm)`.
//!
//! # Lifecycle
//!
//! 1. [`GameModule::register`] — called once during server init to register
//!    systems on the world.
//! 2. [`GameModule::tick`] — called every server tick, after the ECS schedule
//!    runs, for game-specific per-tick logic (spawn/despawn decisions, scoring,
//!    win/lose checks).

use crate::World;

/// Game-specific logic compiled into the binary (static binding).
///
/// A module is the unit of game code: it owns the systems, rules, and
/// per-tick logic that distinguish one game from another. The engine hosts
/// the module; the module never owns engine resources.
pub trait GameModule: Send {
    /// Human-readable name for logging and debug UIs.
    fn name(&self) -> &str;

    /// Register systems on `world`.
    ///
    /// Called once during server initialisation. The module should call
    /// [`World::register_system`] for each of its systems here.
    fn register(&self, world: &mut World);

    /// Per-tick game logic.
    ///
    /// Called every server tick **after** the ECS schedule has run and after
    /// dead entities have been swept. Use this to:
    ///
    /// - Spawn or despawn entities based on game state
    /// - Check win/lose conditions
    /// - Update meta-state that lives outside component arrays
    ///
    /// The default implementation does nothing.
    fn tick(&mut self, _world: &mut World) {}
}

// ── Tests ──────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;
    use crate::System;

    struct CounterModule {
        name: String,
        tick_count: u32,
    }

    impl CounterModule {
        fn new(name: &str) -> Self {
            Self {
                name: name.to_owned(),
                tick_count: 0,
            }
        }
    }

    impl GameModule for CounterModule {
        fn name(&self) -> &str {
            &self.name
        }

        fn register(&self, world: &mut World) {
            let mut system = System::<f32>::new("counter");
            let e = world.spawn();
            system.attach(e, 0.0);
            world.register_system(Box::new(system));
        }

        fn tick(&mut self, world: &mut World) {
            self.tick_count += 1;
            // Increment every entity's f32 component by 1.0 each tick.
            // This requires iterating through the World's systems.
            // For now, just verify we can access the world.
            let _ = world.entity_count();
        }
    }

    #[test]
    fn module_has_name() {
        let module = CounterModule::new("test_mod");
        assert_eq!(module.name(), "test_mod");
    }

    #[test]
    fn module_registers_systems() {
        let mut world = World::new();
        let module = CounterModule::new("test_mod");
        module.register(&mut world);
        assert_eq!(world.entity_count(), 1);
    }

    #[test]
    fn module_tick_increments_counter() {
        let mut module = CounterModule::new("test_mod");
        let mut world = World::new();
        module.register(&mut world);

        assert_eq!(module.tick_count, 0);
        module.tick(&mut world);
        assert_eq!(module.tick_count, 1);
        module.tick(&mut world);
        assert_eq!(module.tick_count, 2);
    }

    /// A module with a default tick implementation (no-op).
    struct NoopModule;

    impl GameModule for NoopModule {
        fn name(&self) -> &str {
            "noop"
        }

        fn register(&self, _world: &mut World) {}
    }

    #[test]
    fn default_tick_does_not_panic() {
        let mut module = NoopModule;
        let mut world = World::new();
        module.tick(&mut world); // should be a no-op
    }

    #[test]
    fn module_tick_after_schedule() {
        let mut world = World::new();
        let mut module = CounterModule::new("counter");

        // Register a custom system that ticks
        module.register(&mut world);

        // Run the schedule (systems tick)
        world.tick();

        // Module tick after schedule
        module.tick(&mut world);
        assert_eq!(module.tick_count, 1);

        // Another full frame
        world.tick();
        module.tick(&mut world);
        assert_eq!(module.tick_count, 2);
    }
}
