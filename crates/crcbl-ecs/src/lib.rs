//! Entity/component storage built on system-owned arrays.
//!
//! # Model
//!
//! * **Entity** — a generational id ([`crcbl_core::Handle<EntityMarker>`]) from
//!   [`crcbl_core::Pool`]. An entity carries no data of its own.
//! * **System** — owns a dense `Vec<T>` of component data plus a sparse
//!   entity→index map. Systems iterate their own arrays linearly (SoA,
//!   cache-friendly). Cross-system access uses sparse lookups for cold paths.
//! * **Schedule** — an ordered sequence of systems run each tick.
//! * **World** — the container: entity pool, schedule, deferred-destruction
//!   queue. `World::tick()` runs the schedule then sweeps dead entities.
//!
//! # Example
//!
//! ```rust
//! use crcbl_ecs::*;
//!
//! let mut world = World::new();
//!
//! // Create entities.
//! let player = world.spawn();
//! let npc    = world.spawn();
//!
//! // Register a position system and attach data.
//! let mut positions = System::<(f32, f32, f32)>::new("position");
//! positions.attach(player, (0.0, 0.0, 0.0));
//! positions.attach(npc,    (10.0, 0.0, 5.0));
//! world.register_system(Box::new(positions));
//!
//! // Run one tick.
//! world.tick();
//!
//! // Inspect.
//! let stats = Inspector::collect(&world);
//! assert_eq!(stats[0].name, "position");
//! assert_eq!(stats[0].entity_count, 2);
//!
//! // Despawn an entity — deferred until sweep.
//! world.despawn(npc);
//! world.tick();
//! assert_eq!(world.entity_count(), 1);
//! ```

mod component_hash;
mod entity;
mod game_module;
mod inspector;
mod schedule;
mod system;
mod world;

pub use component_hash::ComponentHash;
pub use entity::{Entity, EntityMarker};
pub use game_module::GameModule;
pub use inspector::{Inspector, SystemStats};
pub use schedule::Schedule;
pub use system::{DebugCtx, DebugDrawFn, System, SystemTrait};
pub use world::World;

/// Builds an [`Entity`] from raw index and generation for use in tests.
///
/// Real entities are only produced by [`World::spawn`]; this exists so unit
/// tests can fabricate handles without a full `World`.
#[cfg(test)]
pub(crate) fn test_entity(index: u32, generation: u32) -> Entity {
    Entity::from_bits(((generation as u64) << 32) | index as u64)
        .expect("test entity must have non-zero generation")
}
