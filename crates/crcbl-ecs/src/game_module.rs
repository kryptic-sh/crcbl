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
//!    win/lose checks), and handed the [`ClientInputs`] that arrived since the
//!    previous tick.

use crcbl_core::TickId;

use crate::World;

/// The client input frames the server received since the previous tick.
///
/// A borrowed view over the server's per-tick queue. A module iterates it to
/// see each frame as `(TickId, &[u8])` **in arrival order**; the bytes are
/// whatever that game's client put in them, and no engine crate looks inside.
/// This is how player intent reaches game logic — the engine hands the module
/// what arrived and the module decides what it means.
///
/// # Cleared every tick
///
/// The queue behind this view is emptied at the start of each server tick, so
/// a frame is offered to exactly one [`GameModule::tick`] call: **whatever a
/// module does not read during that call is gone.** A module that needs an
/// input to survive until some later tick has to copy it out.
///
/// # What this deliberately is not
///
/// Nothing here compares a frame's [`TickId`] against the server's own clock.
/// Aligning an input to the tick it names is a *jitter buffer*, and it comes
/// with the client tick lead and the rate correction that keep such a buffer
/// fed — none of which exists yet. Until it does, the tick a frame carries is
/// the client's statement about when it sampled that input, offered as-is to a
/// module that cares.
///
/// One session's frames: the server hosts one session, so there is no session
/// id to key them by. A second session would be a second view, not a field
/// added here.
#[derive(Clone, Copy, Debug)]
pub struct ClientInputs<'a> {
    frames: &'a [(TickId, Vec<u8>)],
    dropped: u32,
}

impl<'a> ClientInputs<'a> {
    /// Views `frames` as the inputs for one tick, `dropped` of which the
    /// server's per-tick cap refused.
    #[must_use]
    pub const fn new(frames: &'a [(TickId, Vec<u8>)], dropped: u32) -> Self {
        Self { frames, dropped }
    }

    /// A tick nothing arrived for.
    #[must_use]
    pub const fn empty() -> Self {
        Self {
            frames: &[],
            dropped: 0,
        }
    }

    /// The frames, in the order they arrived.
    ///
    /// By value rather than by reference: the view is [`Copy`], and the
    /// iterator borrows the server's queue rather than the view over it, so a
    /// module can iterate twice without either call outliving the other.
    pub fn iter(self) -> impl Iterator<Item = (TickId, &'a [u8])> {
        self.frames
            .iter()
            .map(|(tick, data)| (*tick, data.as_slice()))
    }

    /// How many frames arrived.
    #[must_use]
    pub const fn len(&self) -> usize {
        self.frames.len()
    }

    /// Whether nothing arrived this tick.
    #[must_use]
    pub const fn is_empty(&self) -> bool {
        self.frames.is_empty()
    }

    /// How many further frames the server refused this tick because its
    /// per-tick cap was already full.
    ///
    /// Non-zero means the peer sent more input for one tick than the server
    /// will hold — a client running far ahead of the server's clock, or one
    /// trying to make the server allocate. The frames counted here were never
    /// stored, so they are not recoverable; the number is what stops the loss
    /// being silent.
    #[must_use]
    pub const fn dropped(&self) -> u32 {
        self.dropped
    }
}

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
    /// - Apply the player intent in `inputs`
    ///
    /// `inputs` holds the client input frames that arrived since the previous
    /// tick, in arrival order, and is empty on a tick nothing arrived for. It
    /// does not outlive the call — see [`ClientInputs`].
    ///
    /// The default implementation does nothing.
    fn tick(&mut self, _world: &mut World, _inputs: ClientInputs<'_>) {}
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

        fn tick(&mut self, world: &mut World, _inputs: ClientInputs<'_>) {
            self.tick_count += 1;
            // Increment every entity's f32 component by 1.0 each tick.
            // This requires iterating through the World's systems.
            // For now, just verify we can access the world.
            let _ = world.entity_count();
        }
    }

    #[test]
    fn a_game_module_reports_the_name_it_was_built_with() {
        let module = CounterModule::new("test_mod");
        assert_eq!(module.name(), "test_mod");
    }

    #[test]
    fn a_modules_register_call_spawns_into_the_world_it_is_handed() {
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
        module.tick(&mut world, ClientInputs::empty());
        assert_eq!(module.tick_count, 1);
        module.tick(&mut world, ClientInputs::empty());
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

    /// The default [`GameModule::tick`] is a no-op, and "no-op" is a claim
    /// about the world it was handed rather than about not panicking: a
    /// default body that ticked the schedule, or swept, would come back
    /// without panicking too.
    #[test]
    fn the_default_tick_leaves_the_world_it_is_handed_unchanged() {
        use std::collections::hash_map::DefaultHasher;
        use std::hash::Hasher;

        fn hash(world: &World) -> u64 {
            let mut hasher = DefaultHasher::new();
            world.hash_state(&mut hasher);
            hasher.finish()
        }

        let mut world = World::new();
        // A world with something in it to disturb: an empty one hashes the
        // same however badly a tick mangles it.
        CounterModule::new("counter").register(&mut world);
        let entity = world.spawn();
        world.despawn(entity);
        let (before, entities, dead) = (hash(&world), world.entity_count(), world.dead_queue_len());

        let mut module = NoopModule;
        module.tick(&mut world, ClientInputs::empty());

        assert_eq!(hash(&world), before, "the systems' state moved");
        assert_eq!(world.entity_count(), entities);
        assert_eq!(
            world.dead_queue_len(),
            dead,
            "the pending despawn was swept"
        );
    }

    /// The view hands the frames back in the order they were queued, with the
    /// tick each one carried. Two frames with *different* ticks and different
    /// bytes, because a view that returned them reversed, or that paired the
    /// wrong tick with the wrong payload, passes any test built on one frame.
    #[test]
    fn the_view_yields_every_frame_in_arrival_order_with_its_own_tick() {
        let frames = vec![
            (TickId::from_raw(7), vec![1, 2]),
            (TickId::from_raw(9), vec![3]),
        ];
        let inputs = ClientInputs::new(&frames, 0);

        assert_eq!(inputs.len(), 2);
        assert!(!inputs.is_empty());
        assert_eq!(
            inputs.iter().collect::<Vec<_>>(),
            vec![
                (TickId::from_raw(7), &[1, 2][..]),
                (TickId::from_raw(9), &[3][..]),
            ],
        );
    }

    /// A tick nothing arrived for reads as empty, and the drop count of a view
    /// that refused frames is the number it refused — not a flag, because a
    /// module logging "some input was dropped" cannot tell one from a hundred.
    #[test]
    fn an_empty_view_is_empty_and_a_capped_one_says_how_many_it_refused() {
        let empty = ClientInputs::empty();
        assert!(empty.is_empty());
        assert_eq!(empty.len(), 0);
        assert_eq!(empty.iter().count(), 0);
        assert_eq!(empty.dropped(), 0);

        let frames = vec![(TickId::ZERO, vec![0])];
        let capped = ClientInputs::new(&frames, 5);
        assert_eq!(capped.len(), 1);
        assert_eq!(capped.dropped(), 5);
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
        module.tick(&mut world, ClientInputs::empty());
        assert_eq!(module.tick_count, 1);

        // Another full frame
        world.tick();
        module.tick(&mut world, ClientInputs::empty());
        assert_eq!(module.tick_count, 2);
    }
}
