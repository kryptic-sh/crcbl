//! Determinism harness: hash the server world state after N ticks.
//!
//! The hash covers every system's entity count and the total entity count, so
//! two runs with identical inputs produce identical hashes. When per-entity
//! component data lands (P3), the hash grows to cover that too.
//!
//! Uses [`std::collections::hash_map::DefaultHasher`] — SipHash, which is
//! stable for the lifetime of a Rust version and deterministic within a process.

use std::collections::hash_map::DefaultHasher;
use std::hash::{Hash, Hasher};

use crcbl_core::TickId;
use crcbl_ecs::{Inspector, World};

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
/// The hash is computed from every system's name, entity count, and the total
/// entity count — deterministic as long as the schedule is populated the same
/// way and the same ticks have run.
pub fn hash_world(world: &World, tick: TickId) -> u64 {
    let mut hasher = DefaultHasher::new();
    tick.hash(&mut hasher);
    let stats = Inspector::collect(world);
    // Sort by name so insertion order doesn't affect the hash.
    let mut sorted: Vec<_> = stats.iter().collect();
    sorted.sort_by(|a, b| a.name.cmp(&b.name));
    for stat in &sorted {
        stat.name.hash(&mut hasher);
        stat.entity_count.hash(&mut hasher);
    }
    world.entity_count().hash(&mut hasher);
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
}
