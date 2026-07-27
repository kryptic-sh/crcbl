//! Determinism harness: hash the server world state after N ticks.
//!
//! The hash covers the tick id, every system's name, and every system's
//! component data (raw bytes).  Same input → same hash → provably
//! deterministic tick loop.
//!
//! Uses [`std::collections::hash_map::DefaultHasher`] — SipHash, which is
//! stable for the lifetime of a Rust version and deterministic within a process.

use std::collections::hash_map::DefaultHasher;
use std::hash::{Hash, Hasher};

use crcbl_core::TickId;
use crcbl_ecs::World;

/// The result of a determinism check: world state hash + tick count.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SimHash {
    /// Number of ticks that ran.
    pub ticks: u64,
    /// The final tick id.
    pub final_tick: TickId,
    /// Hash of world state after all ticks.
    pub hash: u64,
}

/// Hash the current state of `world` at `tick`.
///
/// The hash covers the tick id and every system's name + component data.
/// Systems are sorted by name so registration order does not affect the
/// hash.
pub fn hash_world(world: &World, tick: TickId) -> u64 {
    let mut hasher = DefaultHasher::new();
    tick.hash(&mut hasher);
    world.hash_state(&mut hasher);
    hasher.finish()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crcbl_ecs::System;

    #[test]
    fn empty_world_hashes() {
        let world = World::new();
        let h1 = hash_world(&world, TickId::ZERO);
        let h2 = hash_world(&world, TickId::ZERO);
        assert_eq!(h1, h2);
    }

    #[test]
    fn hash_differs_per_tick() {
        let world = World::new();
        let h1 = hash_world(&world, TickId::ZERO);
        let h2 = hash_world(&world, TickId::from_raw(42));
        assert_ne!(h1, h2);
    }

    #[test]
    fn hash_is_deterministic() {
        let mut world = World::new();
        let e = world.spawn();
        let mut sys = System::<f32>::new("pos");
        sys.attach(e, 1.0);
        world.register_system(Box::new(sys));
        world.tick();

        let h1 = hash_world(&world, TickId::from_raw(1));
        let h2 = hash_world(&world, TickId::from_raw(1));
        assert_eq!(h1, h2);

        // Different tick gives different hash.
        let h3 = hash_world(&world, TickId::from_raw(2));
        assert_ne!(h1, h3);
    }

    #[test]
    fn hash_changes_when_entity_count_changes() {
        let mut world = World::new();
        let e = world.spawn();
        let mut sys = System::<f32>::new("data");
        sys.attach(e, 0.0);
        world.register_system(Box::new(sys));

        let h_before = hash_world(&world, TickId::from_raw(0));
        world.despawn(e);
        world.tick();
        let h_after = hash_world(&world, TickId::from_raw(1));
        assert_ne!(h_before, h_after);
    }

    #[test]
    fn hash_is_stable_across_system_registration_order() {
        let mut world_a = World::new();
        let e = world_a.spawn();
        let mut sa = System::<f32>::new("a");
        sa.attach(e, 0.0);
        let mut sb = System::<f32>::new("b");
        sb.attach(e, 0.0);
        world_a.register_system(Box::new(sa));
        world_a.register_system(Box::new(sb));

        let mut world_b = World::new();
        let e2 = world_b.spawn();
        let mut sb2 = System::<f32>::new("b");
        sb2.attach(e2, 0.0);
        let mut sa2 = System::<f32>::new("a");
        sa2.attach(e2, 0.0);
        world_b.register_system(Box::new(sb2));
        world_b.register_system(Box::new(sa2));

        // Hashes must match despite different registration order.
        assert_eq!(
            hash_world(&world_a, TickId::ZERO),
            hash_world(&world_b, TickId::ZERO)
        );
    }

    #[test]
    fn different_component_values_produce_different_hash() {
        // Same entity counts, different component values — hash must differ.
        // This is the probe from docs/audit.md turned into a regression test.
        let mut world_a = World::new();
        let ea = world_a.spawn();
        let mut sa = System::<f32>::new("pos");
        sa.attach(ea, 1.0);
        world_a.register_system(Box::new(sa));

        let mut world_b = World::new();
        let eb = world_b.spawn();
        let mut sb = System::<f32>::new("pos");
        sb.attach(eb, 999_999.0);
        world_b.register_system(Box::new(sb));

        assert_ne!(
            hash_world(&world_a, TickId::ZERO),
            hash_world(&world_b, TickId::ZERO),
            "two worlds whose component values differ must hash differently"
        );
    }
}
