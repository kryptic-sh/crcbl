//! The host's "players" system: an entity per peer in the session, holding
//! the number the host gave that peer.
//!
//! **Kept by the host's module, from the roster it is handed.** Each tick
//! [`seat`] reads who is in through [`PeerInputs::iter`] — every admitted
//! peer in admission order, a lost one included — and spawns or despawns to
//! match. A re-simulating host
//! ([`Host::resimulate`](crcbl::server::Host::resimulate)) hands its module
//! the recorded roster through the same step, so it seats the same players
//! at the same ticks; a world changed on the host's session events, outside
//! the module, would not be.
//!
//! **The peer's number is the component**, so the world itself says which
//! entity is whose: the state hash tells two players admitted in the other
//! order apart, and the module keeps nothing of its own that the hash cannot
//! see. Only the count reaches a client — `System<T>` replicates no rows.

use crcbl::ecs::{Entity, System, World};
use crcbl::server::PeerInputs;

/// The replicated system holding one entity per admitted peer.
pub const PLAYERS: &str = "players";

/// Registers an empty players system in `world`.
pub fn install(world: &mut World) {
    world.register_system(Box::new(System::<u64>::new(PLAYERS)));
}

/// Seats the peers `inputs` lists: despawns the entity of every peer it no
/// longer lists, then spawns one for each it lists without one, in admission
/// order. A tick whose roster did not change spawns and despawns nothing.
pub fn seat(world: &mut World, inputs: PeerInputs<'_>) {
    let system = players(world);
    let gone: Vec<Entity> = system
        .iter_entities()
        .filter(|&(_, &number)| !inputs.iter().any(|(peer, _)| peer.get() == number))
        .map(|(entity, _)| entity)
        .collect();
    let new: Vec<u64> = inputs
        .iter()
        .map(|(peer, _)| peer.get())
        .filter(|&peer| !system.iter().any(|&number| number == peer))
        .collect();
    for entity in gone {
        world.despawn(entity);
    }
    for peer in new {
        let entity = world.spawn();
        players(world).attach(entity, peer);
    }
}

/// The number of every seated peer, in no particular order.
#[cfg(test)]
pub fn seated(world: &mut World) -> Vec<u64> {
    players(world).iter().copied().collect()
}

/// The players system, which [`install`] registers.
fn players(world: &mut World) -> &mut System<u64> {
    world
        .system_mut::<System<u64>>()
        .expect("the host's world registers the players system")
}
