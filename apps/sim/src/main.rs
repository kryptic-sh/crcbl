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
use std::process;

use crcbl_core::FrameClock;
use crcbl_core::time::{ManualTime, TimeSource};
use crcbl_ecs::{System, World};
use crcbl_server::sim_hash::hash_world;

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
            "--tick-rate" | "-r" => {
                tick_rate = args.next().and_then(|v| v.parse().ok()).unwrap_or_else(|| {
                    eprintln!("sim: --tick-rate needs a number");
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

    let mut ran = 0u64;
    while ran < ticks {
        time.advance(period);
        clock.update(time.elapsed());
        while clock.consume_tick() {
            world.tick();
            ran += 1;
            if ran >= ticks {
                break;
            }
        }
    }

    let final_tick = clock.tick();
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

    println!("hash:{hash:016x} ticks:{ran} final_tick:{final_tick}");
}

/// Builds a deterministic world from a seed.
///
/// The world layout is simple but varied: a few systems with different entity
/// counts, derived from the seed so different seeds test different shapes.
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

    world
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
    -r, --tick-rate <HZ>   Server tick rate [default: 60]
    -s, --seed <SEED>      World-generation seed [default: 0]
    -h, --help             Print this text

OUTPUT:
    hash:<hex> ticks:<n> final_tick:<n>

EXIT CODES:
    0   ok       2   bad invocation"
    );
}
