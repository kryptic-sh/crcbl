//! `crcbl sim` — the determinism harness.
//!
//! Runs N ticks of a deterministic world and prints the state hash. Same input
//! → same hash → provably deterministic tick loop.
//!
//! `docs/plan/11-cli-headless.md` puts the determinism half of its exit
//! criterion behind this verb: "all sample CI determinism + golden-image checks
//! run through the CLI". It used to be a binary of its own, `crcbl-sim`, which
//! meant the one pillar the topic exists to defend had a hole in it — a
//! capability reachable only by building a second thing.
//!
//! # What this verb does not take
//!
//! The topic sketches `crcbl sim <scene> --ticks N [--input script.ron]
//! [--hash]`. Neither the scene argument nor the input script is built, and
//! inventing them here would mean inventing a scene file format and a RON
//! reader, both of which are open questions in `docs/backlog.md` rather than
//! things this tree has. So the world is generated from `--seed`, the parser
//! refuses a positional argument by name, and `--hash` is not a flag because
//! the hash is the output. [`crate::args::SIM_USAGE`] says all of that where a
//! user reads it.
//!
//! # The world lives here, not in the engine
//!
//! [`build_world`] and [`CounterSystem`] are a *harness* world: a fixed, varied,
//! seed-derived shape whose only job is to give [`hash_world`] something with
//! real per-tick mutation to hash. Nothing a game links should be able to reach
//! them, so they are private to this binary rather than surface on
//! `crcbl-server` or `crcbl-ecs`.

use std::hash::Hasher;
use std::time::Duration;

use crcbl::core::FrameClock;
use crcbl::core::time::{ManualTime, TimeSource};
use crcbl::ecs::{ComponentHash, DebugCtx, Entity, System, SystemTrait, World};
use crcbl::server::sim_hash::hash_world;

use crate::args::SimArgs;
use crate::json::Json;
use crate::report::{Failure, Outcome};

/// Runs `crcbl sim`.
///
/// # Errors
///
/// [`Failure`] if the clock and the world disagree about how many ticks ran,
/// which is the one condition that would make the two numbers in the output
/// contract mean different things.
pub fn run(args: &SimArgs) -> Result<Outcome, Failure> {
    // Build a deterministic world. Same seed → same entity layout.
    let mut world = build_world(args.seed);
    // The parser holds `tick_rate` to `1..=MAX_TICK_RATE`, so this division
    // neither divides by zero nor truncates to a zero period.
    let period = Duration::from_nanos(1_000_000_000u64 / u64::from(args.tick_rate));
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
    while ran < args.ticks {
        time.advance(period);
        clock.update(time.elapsed());
        while clock.consume_tick() {
            world.tick();
            ran += 1;
        }
    }

    let final_tick = clock.tick();
    if final_tick.get() != ran {
        return Err(Failure::new(format!(
            "the clock counted {} ticks and the world ran {ran}",
            final_tick.get()
        )));
    }
    let hash = hash_world(&world, final_tick);

    // Warn about any systems that do not contribute to the determinism hash
    // (custom SystemTrait implementations using the default no-op hash_state).
    let non_contrib = world.non_contributing_systems();
    if !non_contrib.is_empty() {
        eprintln!(
            "crcbl: WARNING — {} system(s) do not contribute to the determinism hash: {}",
            non_contrib.len(),
            non_contrib.join(", ")
        );
    }

    // `.get()`, not the `Display` impl: `TickId` renders as "tick 100", and the
    // module docs and `--help` both state the contract as `final_tick:<n>`.
    let human = format!(
        "hash:{hash:016x} ticks:{ran} final_tick:{}",
        final_tick.get()
    );

    Ok(Outcome {
        human,
        // `hash` and `seed` are strings and the other two are numbers, which is
        // not an inconsistency: both are `u64`, [`Json::Number`] holds an `i64`,
        // and a JSON number above 2^53 is not a value a consumer can read back
        // anyway. The hash is hex because that is what the human line says and a
        // consumer should be able to compare the two by eye; the seed is decimal
        // because that is how it was typed. `ticks` and `final_tick` are numbers
        // because a run that reached `i64::MAX` would have had to execute that
        // many ticks first.
        json: vec![
            ("hash", Json::string(format!("{hash:016x}"))),
            ("ticks", Json::Number(ran as i64)),
            ("final_tick", Json::Number(final_tick.get() as i64)),
            ("seed", Json::string(args.seed.to_string())),
            ("tick_rate", Json::Number(i64::from(args.tick_rate))),
        ],
    })
}

/// Builds a deterministic world from a seed.
///
/// The world layout is simple but varied: a few systems with different entity
/// counts, derived from the seed so different seeds test different shapes.
///
/// One system ([`CounterSystem`]) has real per-tick behaviour — it increments
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
#[derive(Debug)]
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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::args::{DEFAULT_SIM_TICK_RATE, MAX_TICK_RATE};

    fn args(ticks: u64, seed: u64) -> SimArgs {
        SimArgs {
            ticks,
            tick_rate: DEFAULT_SIM_TICK_RATE,
            seed,
            json: false,
        }
    }

    /// Every system this harness builds contributes to the hash, so the warning
    /// path stays a warning about somebody else's world and never fires on this
    /// one. Asserted because a `CounterSystem` that quietly stopped
    /// contributing would leave the hash covering nothing but inert storage —
    /// still stable, still "deterministic", and blind to the only state in the
    /// world that moves.
    #[test]
    fn every_system_in_the_harness_world_contributes_to_the_hash() {
        let world = build_world(7);
        assert_eq!(world.non_contributing_systems(), Vec::<String>::new());
    }

    /// The counter really mutates: two runs of different lengths over one seed
    /// hash differently, which is what makes the hash a function of simulation
    /// state and not of `(seed, ticks)` alone.
    #[test]
    fn the_hash_tracks_how_long_the_world_ran() {
        let short = run(&args(10, 3)).expect("a short run").human;
        let long = run(&args(11, 3)).expect("a longer run").human;
        assert_ne!(short, long);
    }

    /// The tick rate sets the clock's period and never the tick count: the loop
    /// advances by exactly one period per iteration at any rate, so the hash of
    /// a run is the same at 1 Hz and at the cap.
    #[test]
    fn the_tick_rate_changes_neither_the_tick_count_nor_the_hash() {
        let mut lines = Vec::new();
        for rate in [1, DEFAULT_SIM_TICK_RATE, 240, MAX_TICK_RATE] {
            let outcome = run(&SimArgs {
                ticks: 32,
                tick_rate: rate,
                seed: 5,
                json: false,
            })
            .expect("a run at every legal rate");
            assert!(
                outcome.human.ends_with("ticks:32 final_tick:32"),
                "{rate} Hz: {}",
                outcome.human
            );
            lines.push(outcome.human);
        }
        assert!(
            lines.windows(2).all(|pair| pair[0] == pair[1]),
            "the rate moved the hash: {lines:?}"
        );
    }

    /// The `--json` mirror carries the same three fields the human line does,
    /// with the hash spelled identically so the two can be compared by eye.
    #[test]
    fn the_json_fields_mirror_the_human_line() {
        let outcome = run(&SimArgs {
            ticks: 4,
            tick_rate: DEFAULT_SIM_TICK_RATE,
            seed: u64::MAX,
            json: true,
        })
        .expect("a run");
        let hash = outcome
            .human
            .strip_prefix("hash:")
            .and_then(|rest| rest.split_whitespace().next())
            .expect("the human line opens with the hash");
        assert_eq!(outcome.json[0], ("hash", Json::string(hash)));
        assert_eq!(outcome.json[1], ("ticks", Json::Number(4)));
        assert_eq!(outcome.json[2], ("final_tick", Json::Number(4)));
        // A seed no `i64` can hold, spelled back exactly.
        assert_eq!(
            outcome.json[3],
            ("seed", Json::string(u64::MAX.to_string()))
        );
    }
}
