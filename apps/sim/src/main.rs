//! Headless server simulation — the determinism harness.
//!
//! Runs N ticks of a deterministic world and prints the state hash. Same input
//! → same hash → provably deterministic tick loop.
//!
//! # Usage
//!
//! ```text
//! crcbl-sim --ticks 1000 --tick-rate 60 --seed 42
//! ```
//!
//! Outputs one line: `hash:<hex> ticks:<n> final_tick:<n>`

use std::env;
use std::hash::Hasher;
use std::process;

use crcbl_core::FrameClock;
use crcbl_core::time::{ManualTime, TimeSource};
use crcbl_ecs::{ComponentHash, DebugCtx, Entity, System, SystemTrait, World};
use crcbl_server::sim_hash::hash_world;

/// The highest tick rate with a period of at least one nanosecond.
///
/// `1_000_000_000 / tick_rate` is integer division: above this it truncates to
/// zero, and `FrameClock::with_period` refuses a zero period. Rejecting it here
/// makes it exit 2 rather than assert.
const MAX_TICK_RATE: u32 = 1_000_000_000;

fn main() {
    let mut ticks: u64 = 1_000;
    let mut tick_rate: u32 = 60;
    let mut seed: u64 = 0;

    let mut args = env::args().skip(1);
    while let Some(arg) = args.next() {
        match arg.as_str() {
            "--ticks" | "-n" => {
                ticks = args.next().and_then(|v| v.parse().ok()).unwrap_or_else(|| {
                    eprintln!("sim: --ticks needs a number");
                    process::exit(2);
                });
            }
            // A zero tick rate used to parse cleanly and then divide by zero
            // computing the period, aborting with exit 101 instead of the
            // documented exit 2. Both other apps reject `--tick-hz 0`.
            "--tick-rate" | "-r" => {
                tick_rate = args
                    .next()
                    .and_then(|v| v.parse::<u32>().ok())
                    .filter(|&rate| (1..=MAX_TICK_RATE).contains(&rate))
                    .unwrap_or_else(|| {
                        eprintln!("sim: --tick-rate needs a number in 1..={MAX_TICK_RATE}");
                        process::exit(2);
                    });
            }
            "--seed" | "-s" => {
                seed = args.next().and_then(|v| v.parse().ok()).unwrap_or_else(|| {
                    eprintln!("sim: --seed needs a number");
                    process::exit(2);
                });
            }
            "-h" | "--help" => {
                print_help();
                process::exit(0);
            }
            other => {
                eprintln!("sim: unrecognized argument `{other}`");
                process::exit(2);
            }
        }
    }

    // Build a deterministic world.  Same seed → same entity layout.
    let mut world = build_world(seed);
    let period = std::time::Duration::from_nanos(1_000_000_000u64 / tick_rate as u64);
    let mut time = ManualTime::new();

    // Wire up a serverless tick loop using ManualTime.
    let mut clock = FrameClock::with_period(period);
    clock.update(time.elapsed());

    // One period per iteration and the accumulator drained to empty each time,
    // so the loop leaves no whole tick unconsumed. It used to `break` out of
    // the inner drain the moment `ran` reached the budget, which left the
    // accumulator holding ticks the clock had already counted — so `ticks` and
    // `final_tick`, the two numbers this harness's only output contract is made
    // of, could disagree for reasons that had nothing to do with determinism.
    let mut ran = 0u64;
    while ran < ticks {
        time.advance(period);
        clock.update(time.elapsed());
        while clock.consume_tick() {
            world.tick();
            ran += 1;
        }
    }

    let final_tick = clock.tick();
    assert_eq!(
        final_tick.get(),
        ran,
        "the clock counted a different number of ticks than the world ran",
    );
    let hash = hash_world(&world, final_tick);

    // Warn about any systems that do not contribute to the determinism hash
    // (custom SystemTrait implementations using the default no-op hash_state).
    let non_contrib = world.non_contributing_systems();
    if !non_contrib.is_empty() {
        eprintln!(
            "sim: WARNING — {} system(s) do not contribute to the determinism hash: {}",
            non_contrib.len(),
            non_contrib.join(", ")
        );
    }

    // `.get()`, not the `Display` impl: `TickId` renders as "tick 100", and the
    // module docs and `--help` both state the contract as `final_tick:<n>`.
    println!(
        "hash:{hash:016x} ticks:{ran} final_tick:{}",
        final_tick.get()
    );
}

/// Builds a deterministic world from a seed.
///
/// The world layout is simple but varied: a few systems with different entity
/// counts, derived from the seed so different seeds test different shapes.
///
/// One system (`CounterSystem`) has real per-tick behaviour — it increments
/// each entity's f32 component by 1.0 every tick, so the state hash genuinely
/// depends on tick count, not just on `(seed, ticks)`.
fn build_world(seed: u64) -> World {
    let mut world = World::new();

    // Simple LCG for repeatable entity count variation.
    let mut state = seed.wrapping_mul(6364136223846793005).wrapping_add(1);
    state = state.wrapping_mul(6364136223846793005).wrapping_add(1);

    let position_count = (state % 20) + 1;
    let velocity_count = (state.wrapping_mul(6364136223846793005).wrapping_add(1) % 20) + 1;

    let mut positions = System::<f32>::new("position");
    for i in 0..position_count {
        let e = world.spawn();
        positions.attach(e, i as f32);
    }
    world.register_system(Box::new(positions));

    let mut velocities = System::<f32>::new("velocity");
    for i in 0..velocity_count {
        let e = world.spawn();
        velocities.attach(e, (i as f32) * 0.5);
    }
    world.register_system(Box::new(velocities));

    // A system with real per-tick behaviour: each tick increments every
    // entity's f32 by 1.0.  This ensures the determinism hash actually
    // changes with simulation state, not just with (seed, tick_count).
    let mut counters = CounterSystem::new("counter");
    for i in 0..5 {
        let e = world.spawn();
        counters.attach(e, (i as f32) * 10.0);
    }
    world.register_system(Box::new(counters));

    world
}

// ---------------------------------------------------------------------------
// CounterSystem — a custom SystemTrait impl with real per-tick mutation
// ---------------------------------------------------------------------------

/// A simple system that increments every entity's `f32` component by 1.0
/// each tick.  Exists so the determinism harness has real mutable state to
/// hash, not just inert storage.
struct CounterSystem {
    name: String,
    data: Vec<(Entity, f32)>,
}

impl CounterSystem {
    fn new(name: &str) -> Self {
        Self {
            name: name.to_owned(),
            data: Vec::new(),
        }
    }

    fn attach(&mut self, entity: Entity, value: f32) {
        self.data.push((entity, value));
    }
}

impl SystemTrait for CounterSystem {
    fn name(&self) -> &str {
        &self.name
    }

    fn tick(&mut self, _dt: f64) {
        for (_, val) in &mut self.data {
            *val += 1.0;
        }
    }

    fn entity_count(&self) -> usize {
        self.data.len()
    }

    fn sweep(&mut self, dead: &[Entity]) {
        self.data.retain(|(e, _)| !dead.contains(e));
    }

    fn debug_draw(&mut self, _ctx: &DebugCtx) {
        // No debug visuals.
    }

    fn hash_state(&self, hasher: &mut dyn Hasher) {
        for (_, val) in &self.data {
            val.hash_component(hasher);
        }
    }

    fn contributes_to_hash(&self) -> bool {
        true
    }

    fn as_any_mut(&mut self) -> &mut dyn std::any::Any {
        self
    }
}

fn print_help() {
    println!(
        "\
crcbl-sim — determinism harness

USAGE:
    crcbl-sim [OPTIONS]

Runs a headless server simulation for N ticks and prints the state hash.
Same input → same hash → deterministic loop.

OPTIONS:
    -n, --ticks <N>        Number of ticks to simulate [default: 1000]
    -r, --tick-rate <HZ>   Server tick rate, 1..=1000000000 [default: 60]
    -s, --seed <SEED>      World-generation seed [default: 0]
    -h, --help             Print this text

OUTPUT:
    hash:<hex> ticks:<n> final_tick:<n>

EXIT CODES:
    0   ok       2   bad invocation"
    );
}
