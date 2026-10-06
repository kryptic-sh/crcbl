//! The towers server's world on a job pool: a host over the committed map,
//! its schedule handed a `crcbl::jobs::Pool`, hashes after every tick as the
//! same host with no pool does.
//!
//! The server's world holds one system, [`FieldReplica`](super::FieldReplica),
//! and the stage is ticked by the module between schedule runs — so this pins
//! that a pool leaves a one-system schedule and everything around it exactly
//! as it was, not that towers gains anything from one.

use std::time::Duration;

use crcbl::core::{FrameClock, TickId};
use crcbl::jobs::{Pool, default_spawner};
use crcbl::server::sim_hash::hash_world;
use crcbl::server::{Host, HostConfig};

use super::{COMPATIBILITY, DEFAULT_TICK_HZ, Field};
use crate::map::Map;

/// Long enough for the first wave to leave the gate with nobody playing.
const TICKS: u64 = 1200;

/// The stage's hash after each of [`TICKS`] host ticks, its world on `pool`.
fn hashes(pool: Option<Pool>) -> Vec<u64> {
    let (_field, mut world, module) = Field::open(&Map::built_in(), DEFAULT_TICK_HZ);
    world.set_pool(pool);
    let mut host = Host::new(
        world,
        HostConfig {
            max_peers: 2,
            tick_hz: DEFAULT_TICK_HZ,
            compatibility: COMPATIBILITY,
        },
    );
    host.set_module(Box::new(module));
    let period = FrameClock::new(DEFAULT_TICK_HZ).tick_dt();
    let mut now = Duration::ZERO;
    host.update(now);
    (1..=TICKS)
        .map(|tick| {
            now += period;
            host.update(now);
            hash_world(host.world(), TickId::from_raw(tick))
        })
        .collect()
}

#[test]
fn the_towers_server_hashes_alike_on_a_pool_and_without_one() {
    let serial = hashes(None);
    assert_ne!(
        serial.first(),
        serial.last(),
        "the stage must move during the run, or equal hashes prove nothing"
    );
    for workers in [1, 7] {
        let pool = Pool::with_workers(default_spawner().as_ref(), workers).expect("a pool");
        assert_eq!(hashes(Some(pool)), serial, "{workers} workers");
    }
}
