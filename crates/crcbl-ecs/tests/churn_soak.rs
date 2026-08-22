//! Spawn/despawn churn over many ticks, then assert nothing leaked.
//!
//! `docs/plan/12-testing.md` asks `crcbl-ecs` for a "churn soak with leak
//! assert" by name. The loop is the easy half; what "leaked" means here is the
//! substance, so each place this ECS can lose something is named below together
//! with the observable that would differ if it did.
//!
//! # What can leak, and what says so
//!
//! * **The entity id space.** [`World::spawn`] hands out handles from
//!   `crcbl_core::Pool`, which recycles a removed slot through a free list. Its
//!   slot vector therefore grows to the high-water mark of *simultaneously*
//!   live entities and never to the total number of spawns. Nothing exposes the
//!   slot count, but the index a fresh handle carries does: a pool that stopped
//!   recycling would issue index `n` for the `n`th spawn ever. So the largest
//!   index ever issued, plus one, must equal the peak population — and the peak
//!   must be far below the spawn total, or the loop never recycled anything to
//!   begin with.
//! * **Component rows.** A [`System<T>`] keeps a dense `Vec<T>` beside a sparse
//!   entity→index map, and [`World::sweep`] detaches every dead entity from
//!   every system before removing it from the pool. A row must never outlive
//!   its entity: after every tick each system must hold exactly one row per
//!   live entity it was given, that row must read back the value the entity was
//!   spawned with, and the dense arrays walked by
//!   [`System::iter_entities`] must agree with the sparse map that
//!   [`System::get`] answers from.
//! * **The deferred-destruction queue.** [`World::despawn`] pushes onto a `Vec`
//!   that the sweep drains. It must be empty after every tick.
//! * **Stale handles.** A despawned handle must resolve nowhere afterwards —
//!   not through [`World::is_alive`], not through [`System::get`] — and no
//!   handle issued during the whole run may ever equal another, which is what
//!   makes a recycled slot a *new* entity rather than an alias for the old one.
//!
//! # What is not asserted, and why
//!
//! * **Allocator capacity.** `Vec` and `HashMap` keep the capacity they grew
//!   to. Holding the peak's worth of spare capacity is how they are meant to
//!   behave and is not a leak; nothing exposes it either.
//! * **Generation exhaustion.** `Pool` retires a slot whose 32-bit generation
//!   is spent rather than wrapping it. Reaching that needs about four billion
//!   reuses of one slot, which no soak that fits in a `cargo test` run can
//!   approach, and `Pool::retired_slots` is not reachable through `World`
//!   anyway. An assertion here could not fail, so there is none.
//!
//! # Replaying a failure
//!
//! Every draw is [`crcbl_core::rand::hash_u64`] of an index, not a generator,
//! so what a tick does depends on [`SEED`] and that tick's index alone —
//! nothing is carried from the ticks before it. Failure messages name the tick
//! and the seed; re-running the test replays exactly the same sequence.

use std::collections::HashSet;

use crcbl_core::rand::{hash_u64 as mix, salt};
use crcbl_ecs::{Entity, System, SystemTrait, World};

// ---------------------------------------------------------------------------
// The soak's shape
// ---------------------------------------------------------------------------

/// The seed every draw in this file hashes against.
///
/// Named in each failure message, because re-running with it is the whole
/// reproduction.
const SEED: u64 = 0x5EED_C0FF_EE00_1234;

/// Ticks the full soak runs.
///
/// Deliberately not a whole number of [`PURGE_PERIOD`]s: a run that ended on a
/// purge would end with nothing alive, and every end-of-run assertion about
/// surviving rows would then be a walk over an empty set.
const TICKS: u64 = 4_900;

/// Ticks the replay check runs — enough to spawn, despawn, purge and recycle,
/// short enough that running it twice costs nothing. Not a whole number of
/// [`PURGE_PERIOD`]s either, for the reason [`TICKS`] gives.
const REPLAY_TICKS: u64 = 600;

/// Most entities a single tick spawns; the count is drawn from `0..=` this.
const MAX_SPAWNS_PER_TICK: u64 = 6;

/// Most entities an ordinary tick despawns, before the population caps it.
///
/// Below [`MAX_SPAWNS_PER_TICK`] on purpose: the population climbs between
/// purges, so the pool keeps being asked for slots it has not handed out yet.
const MAX_DESPAWNS_PER_TICK: u64 = 5;

/// Every this many ticks the soak despawns the entire population, so the free
/// list is refilled wholesale and the next stretch runs entirely on recycled
/// slots.
const PURGE_PERIOD: u64 = 250;

/// The component every spawned entity carries: a grid cell.
type Cell = (i32, i32, i32);

/// The component only some entities carry, so the two systems hold different
/// row sets and neither can stand in for the other.
type Health = i32;

// ---------------------------------------------------------------------------
// Deterministic draws
// ---------------------------------------------------------------------------

/// Each kind of decision draws from its own salted seed, so the number of
/// spawns in a tick cannot correlate with which entity that tick kills.
const STREAM_SPAWN_COUNT: u64 = 0;
const STREAM_DESPAWN_COUNT: u64 = 1;
const STREAM_VICTIM: u64 = 2;
const STREAM_CELL: u64 = 3;
const STREAM_HEALTH: u64 = 4;

/// A 64-bit draw from one stream at one index.
fn draw(stream: u64, index: u64) -> u64 {
    mix(salt(SEED, stream), index)
}

/// A uniform index into `len` items.
///
/// The modulo is taken in 64 bits so a 32-bit `usize` picks the same item as a
/// 64-bit one; the result is below `len`, so narrowing it is exact.
fn pick(stream: u64, index: u64, len: usize) -> usize {
    assert!(len > 0, "nothing to pick from (seed {SEED:#x})");
    (draw(stream, index) % len as u64) as usize
}

// ---------------------------------------------------------------------------
// The soak
// ---------------------------------------------------------------------------

/// One live entity and the component values it was spawned with, as the test
/// remembers them — the oracle every row assertion compares against.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct Spawned {
    entity: Entity,
    cell: Cell,
    health: Option<Health>,
}

/// What a run of the soak did, for the caller to assert on.
#[derive(Debug, PartialEq, Eq)]
struct Outcome {
    spawns: u64,
    despawns: u64,
    /// Largest `World::entity_count()` ever reached — sampled after each tick's
    /// spawns, which is where the pool is fullest, since a despawn frees
    /// nothing until the sweep.
    peak_pool_len: usize,
    /// Largest slot index any handle was ever issued with.
    max_index: u32,
    /// Every entity still alive at the end with its components, by handle.
    final_rows: Vec<(u64, Cell, Option<Health>)>,
}

/// Runs the churn loop, asserting the per-tick invariants as it goes.
fn run_soak(ticks: u64) -> Outcome {
    let mut world = World::new();
    world.register_system(Box::new(System::<Cell>::new("position")));
    world.register_system(Box::new(System::<Health>::new("health")));

    let mut live: Vec<Spawned> = Vec::new();
    let mut retired: Vec<Entity> = Vec::new();
    let mut issued: HashSet<u64> = HashSet::new();
    let mut spawns: u64 = 0;
    let mut despawns: u64 = 0;
    let mut peak_pool_len: usize = 0;
    let mut max_index: u32 = 0;

    for tick in 0..ticks {
        for _ in 0..draw(STREAM_SPAWN_COUNT, tick) % (MAX_SPAWNS_PER_TICK + 1) {
            let entity = world.spawn();
            assert!(
                issued.insert(entity.to_bits()),
                "tick {tick}: {entity:?} was handed out twice, so a recycled \
                 slot aliases the entity that used to live in it \
                 (seed {SEED:#x})"
            );
            max_index = max_index.max(entity.index());

            let cell = (
                draw(STREAM_CELL, spawns * 3) as i32,
                draw(STREAM_CELL, spawns * 3 + 1) as i32,
                draw(STREAM_CELL, spawns * 3 + 2) as i32,
            );
            let health = draw(STREAM_HEALTH, spawns * 2)
                .is_multiple_of(2)
                .then(|| draw(STREAM_HEALTH, spawns * 2 + 1) as i32);

            world
                .system_mut::<System<Cell>>()
                .expect("the position system was registered above")
                .attach(entity, cell);
            if let Some(points) = health {
                world
                    .system_mut::<System<Health>>()
                    .expect("the health system was registered above")
                    .attach(entity, points);
            }

            live.push(Spawned {
                entity,
                cell,
                health,
            });
            spawns += 1;
        }
        peak_pool_len = peak_pool_len.max(world.entity_count());

        let kills = if tick % PURGE_PERIOD == PURGE_PERIOD - 1 {
            live.len() as u64
        } else {
            (draw(STREAM_DESPAWN_COUNT, tick) % (MAX_DESPAWNS_PER_TICK + 1)).min(live.len() as u64)
        };
        for _ in 0..kills {
            let victim = live.swap_remove(pick(STREAM_VICTIM, despawns, live.len()));
            world.despawn(victim.entity);
            retired.push(victim.entity);
            despawns += 1;
        }

        world.tick();

        assert_eq!(
            world.dead_queue_len(),
            0,
            "tick {tick}: the deferred-destruction queue was not drained by the \
             sweep (seed {SEED:#x})"
        );
        assert_eq!(
            world.entity_count(),
            live.len(),
            "tick {tick}: the pool holds a different number of entities than \
             the soak believes are alive (seed {SEED:#x})"
        );
        assert_rows_match_live(&mut world, &live, tick);
    }

    for &stale in &retired {
        assert!(
            !world.is_alive(stale),
            "{stale:?} was despawned and swept but still resolves (seed {SEED:#x})"
        );
    }
    let positions = world
        .system_mut::<System<Cell>>()
        .expect("the position system was registered above");
    for &stale in &retired {
        assert!(
            positions.get(stale).is_none(),
            "the position system still answers for despawned {stale:?}, so its \
             row outlived its entity (seed {SEED:#x})"
        );
    }
    let health = world
        .system_mut::<System<Health>>()
        .expect("the health system was registered above");
    for &stale in &retired {
        assert!(
            health.get(stale).is_none(),
            "the health system still answers for despawned {stale:?}, so its \
             row outlived its entity (seed {SEED:#x})"
        );
    }

    let mut final_rows: Vec<(u64, Cell, Option<Health>)> = live
        .iter()
        .map(|s| (s.entity.to_bits(), s.cell, s.health))
        .collect();
    final_rows.sort_unstable();

    Outcome {
        spawns,
        despawns,
        peak_pool_len,
        max_index,
        final_rows,
    }
}

/// Every system holds exactly one row per live entity that was attached to it,
/// each row still reads back its entity's value, and the dense arrays agree
/// with the sparse map.
fn assert_rows_match_live(world: &mut World, live: &[Spawned], tick: u64) {
    let mut expected_cells: Vec<(u64, Cell)> =
        live.iter().map(|s| (s.entity.to_bits(), s.cell)).collect();
    expected_cells.sort_unstable();
    let mut expected_health: Vec<(u64, Health)> = live
        .iter()
        .filter_map(|s| s.health.map(|points| (s.entity.to_bits(), points)))
        .collect();
    expected_health.sort_unstable();

    let positions = world
        .system_mut::<System<Cell>>()
        .expect("the position system was registered in run_soak");
    assert_eq!(
        positions.entity_count(),
        expected_cells.len(),
        "tick {tick}: the position system holds a row count that is not the \
         number of live entities attached to it (seed {SEED:#x})"
    );
    for entry in live {
        assert_eq!(
            positions.get(entry.entity),
            Some(&entry.cell),
            "tick {tick}: the position system lost or corrupted the row for \
             {:?} (seed {SEED:#x})",
            entry.entity
        );
    }
    let mut rows: Vec<(u64, Cell)> = positions
        .iter_entities()
        .map(|(entity, cell)| (entity.to_bits(), *cell))
        .collect();
    rows.sort_unstable();
    assert_eq!(
        rows, expected_cells,
        "tick {tick}: the position system's dense arrays disagree with the \
         live set (seed {SEED:#x})"
    );

    let health = world
        .system_mut::<System<Health>>()
        .expect("the health system was registered in run_soak");
    assert_eq!(
        health.entity_count(),
        expected_health.len(),
        "tick {tick}: the health system holds a row count that is not the \
         number of live entities attached to it (seed {SEED:#x})"
    );
    for entry in live {
        assert_eq!(
            health.get(entry.entity),
            entry.health.as_ref(),
            "tick {tick}: the health system's answer for {:?} is not what that \
             entity was spawned with (seed {SEED:#x})",
            entry.entity
        );
    }
    let mut rows: Vec<(u64, Health)> = health
        .iter_entities()
        .map(|(entity, points)| (entity.to_bits(), *points))
        .collect();
    rows.sort_unstable();
    assert_eq!(
        rows, expected_health,
        "tick {tick}: the health system's dense arrays disagree with the live \
         set (seed {SEED:#x})"
    );
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

/// The spawns [`TICKS`] of this seed's sequence performs.
///
/// Pinned rather than bounded: a loop whose churn silently went to zero passes
/// every leak assertion in this file perfectly, and a generator that quietly
/// changed shape would replay nothing.
const EXPECTED_SPAWNS: u64 = 14_925;

/// The despawns the same sequence performs. Short of [`EXPECTED_SPAWNS`] by
/// whatever is still alive at the last tick.
const EXPECTED_DESPAWNS: u64 = 14_859;

/// The peak population, which is also the number of pool slots the run ever
/// needed. Far below [`EXPECTED_SPAWNS`] is the whole point.
const EXPECTED_PEAK: usize = 229;

/// How many times over the run must reuse the slots it peaked at before the
/// id-space bound is saying anything.
///
/// The bound is an equality between the peak population and the highest slot
/// index issued, and a soak whose population only ever grew would satisfy it
/// trivially. This is what makes the loop prove recycling rather than assume
/// it.
const MIN_SPAWNS_PER_PEAK_SLOT: u64 = 10;

#[test]
fn churn_over_many_ticks_leaks_nothing() {
    let outcome = run_soak(TICKS);

    assert_eq!(
        (outcome.spawns, outcome.despawns),
        (EXPECTED_SPAWNS, EXPECTED_DESPAWNS),
        "the soak did not perform the churn it is supposed to (seed {SEED:#x})"
    );
    assert_eq!(
        outcome.peak_pool_len, EXPECTED_PEAK,
        "the peak population changed, so the sequence is not the one the id-space \
         bound below was established against (seed {SEED:#x})"
    );
    assert_eq!(
        outcome.max_index as usize + 1,
        outcome.peak_pool_len,
        "the pool handed out {} slot indices for a population that never \
         exceeded {}, so removed slots are not being recycled (seed {SEED:#x})",
        outcome.max_index as usize + 1,
        outcome.peak_pool_len
    );
    assert!(
        (outcome.peak_pool_len as u64) < outcome.spawns / MIN_SPAWNS_PER_PEAK_SLOT,
        "the peak population {} is not far enough below the {} spawns for the \
         bound above to say anything about recycling (seed {SEED:#x})",
        outcome.peak_pool_len,
        outcome.spawns
    );
}

/// The sequence is a function of the seed and nothing else.
///
/// Without this a red soak reports that something is wrong and stops there:
/// re-running would not reproduce it, and the seed in the failure message would
/// be decoration.
#[test]
fn the_soak_replays_from_its_seed() {
    assert_eq!(
        run_soak(REPLAY_TICKS),
        run_soak(REPLAY_TICKS),
        "two runs of the same seed churned differently (seed {SEED:#x})"
    );
}
