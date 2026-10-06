//! A host whose world's schedule ticks on a `crcbl_jobs::Pool`: the state
//! hash after every tick is the one the same host reaches with no pool, at any
//! worker count — the server half of `docs/plan/21-jobs.md`'s determinism rule.

use std::hash::Hasher;

use crcbl_ecs::{Access, DebugCtx, Entity, Shared, System, SystemTrait};
use crcbl_jobs::{Pool, default_spawner};

use super::tests::{COMPATIBILITY, TICK, TICK_HZ};
use super::*;
use crate::sim_hash::hash_world;

/// How many ticks each host runs: long enough for the module's despawns to
/// have swept part of the crowd.
const TICKS: u32 = 240;

/// The module despawns one of the crowd every this many ticks.
const DESPAWN_EVERY: u64 = 16;

/// Entities in the crowd system: more than [`TICKS`] / [`DESPAWN_EVERY`], so
/// the despawns never run out of members.
const CROWD: u32 = 32;

const _: () = assert!(CROWD as u64 > TICKS as u64 / DESPAWN_EVERY);

/// One value stepped each tick, reading one resource and writing another as
/// built — readers after a writer, two writers of one resource, and systems
/// touching nothing, so the schedule has both stages of several systems and
/// conflicts between stages.
struct Lane {
    name: &'static str,
    value: f64,
    reads: Option<Shared<f64>>,
    writes: Option<Shared<f64>>,
}

impl Lane {
    /// Where a value wraps: bounded, and never settling, so the order a
    /// conflict protects keeps showing in the hash however long the run.
    const WRAP: f64 = 997.0;
}

impl SystemTrait for Lane {
    fn name(&self) -> &str {
        self.name
    }

    fn access(&self) -> Access {
        let mut access = Access::none();
        if let Some(read) = &self.reads {
            access = access.reads(read.name());
        }
        if let Some(written) = &self.writes {
            access = access.writes(written.name());
        }
        access
    }

    fn tick(&mut self, dt: f64) {
        let input = self.reads.as_ref().map_or(0.0, |read| *read.read());
        self.value = (1.5 * self.value + input + dt) % Self::WRAP;
        if let Some(written) = &self.writes {
            let mut value = written.write();
            *value = (0.5 * *value + self.value) % Self::WRAP;
        }
    }

    fn entity_count(&self) -> usize {
        1
    }

    fn sweep(&mut self, _dead: &[Entity]) {}

    fn debug_draw(&mut self, _ctx: &DebugCtx) {}

    fn hash_state(&self, hasher: &mut dyn Hasher) {
        hasher.write_u64(self.value.to_bits());
    }

    fn contributes_to_hash(&self) -> bool {
        true
    }

    fn as_any_mut(&mut self) -> &mut dyn std::any::Any {
        self
    }
}

/// Despawns the crowd's oldest member every [`DESPAWN_EVERY`] ticks, so the
/// host's sweep — which stays on the calling thread — has work in it.
struct Thinning {
    crowd: Vec<Entity>,
    ticks: u64,
}

impl HostModule for Thinning {
    fn tick(&mut self, world: &mut World, _inputs: PeerInputs<'_>) {
        self.ticks += 1;
        if self.ticks.is_multiple_of(DESPAWN_EVERY) {
            world.despawn(self.crowd.remove(0));
        }
    }
}

/// A host over the lanes and a crowd of [`CROWD`] entities, its schedule on
/// `pool`.
fn host(pool: Option<Pool>) -> Host {
    let wind = Shared::new("wind", 0.0);
    let score = Shared::new("score", 0.0);
    let mut world = World::new();
    world.share(&wind).expect("distinct names");
    world.share(&score).expect("distinct names");
    let lane = |name, reads: Option<&Shared<f64>>, writes: Option<&Shared<f64>>| {
        Box::new(Lane {
            name,
            value: 1.0,
            reads: reads.cloned(),
            writes: writes.cloned(),
        })
    };
    world.register_system(lane("gust", None, Some(&wind)));
    world.register_system(lane("drift", Some(&wind), None));
    world.register_system(lane("spin", None, None));
    world.register_system(lane("tally", Some(&wind), Some(&score)));
    world.register_system(lane("audit", None, Some(&score)));
    world.register_system(lane("pulse", Some(&score), None));
    let mut crowd = System::<u32>::new("crowd");
    let mut members = Vec::new();
    for index in 0..CROWD {
        let entity = world.spawn();
        crowd.attach(entity, index);
        members.push(entity);
    }
    world.register_system(Box::new(crowd));
    world.set_pool(pool);

    let mut host = Host::new(
        world,
        HostConfig {
            max_peers: 2,
            tick_hz: TICK_HZ,
            compatibility: COMPATIBILITY,
        },
    );
    host.set_module(Box::new(Thinning {
        crowd: members,
        ticks: 0,
    }));
    host
}

/// The world's hash after each of [`TICKS`] host ticks.
fn hashes(mut host: Host) -> Vec<u64> {
    let mut now = Duration::ZERO;
    host.update(now);
    (1..=TICKS)
        .map(|tick| {
            now += TICK;
            assert_eq!(host.update(now), 1, "one period is one tick");
            hash_world(host.world(), TickId::from_raw(u64::from(tick)))
        })
        .collect()
}

/// **A host's world hashes the same after every tick on a pool as without
/// one**, at no, one and seven workers, with despawns swept along the way.
#[test]
fn a_host_on_a_pool_hashes_as_it_does_serially_after_every_tick() {
    let serial = hashes(host(None));
    assert!(serial.windows(2).all(|pair| pair[0] != pair[1]));
    for workers in [0, 1, 7] {
        let pool = Pool::with_workers(default_spawner().as_ref(), workers).expect("a pool");
        assert_eq!(hashes(host(Some(pool))), serial, "{workers} workers");
    }
    // Checked after the hashes rather than before, so a schedule that put
    // conflicting systems in one stage is caught by the hash first.
    let schedule_len = host(None).world().schedule().len();
    let stages = host(None).world().schedule().stages().len();
    assert!(
        stages > 1 && stages < schedule_len,
        "the world needs stages of several systems and conflicts between them"
    );
}
