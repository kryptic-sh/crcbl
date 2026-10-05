//! The `ecs` scenario: one [`crcbl::ecs::World`] ticked over a fixed schedule,
//! timed a tick at a time.
//!
//! # Why this scenario exists
//!
//! `docs/backlog.md` records the 2026-09-06 decision that put an ECS bench
//! ahead of any parallel schedule: a runner that overlaps systems is only worth
//! its complexity if a tick of a realistic schedule gets cheaper, and nothing
//! had ever timed one. This is that baseline, and the number a parallel runner
//! is compared against.
//!
//! # The schedule, and why its declarations are mixed
//!
//! [`LANES`] is the whole schedule: every system owns one [`Row`] per entity
//! and runs the same damped-spring [`step`] over its rows, so the systems cost
//! the same and what differs between them is only what they declare. Some
//! touch nothing shared, some read a [`Shared`] resource, and some write one —
//! two of them the same one — so the conflict graph the schedule derives has
//! the shape a game's has: a run of independent systems between writers that
//! have to keep their order. A schedule of independent systems alone would
//! flatter any runner, and one where every system conflicts would give it
//! nothing to do.
//!
//! A writer folds its rows into one value and blends it into its resource
//! ([`BLEND`]), so two writers of one resource give an answer that depends on
//! which wrote first — the order a conflict exists to protect is one the
//! checksum can see.
//!
//! # The result is used, and it is portable
//!
//! The run ends by hashing the world through `crcbl-server`'s
//! [`hash_world`], the hash `crcbl sim` prints, and fails if the ticks left it
//! where they found it: a schedule the optimiser emptied, or one that ran no
//! system, is a failure rather than a fast number. [`step`] is multiplies,
//! adds and a square root, which IEEE 754 rounds identically everywhere, and
//! the rows are seeded from [`hash_unit`] — nothing here calls a
//! transcendental — so the checksum is the same on every target, not merely
//! twice on this one.
//!
//! **No entity is ever despawned**, so the rows are kept without their
//! [`Entity`] handles and `sweep` has nothing to do: the scenario times the
//! schedule, and a sweep over an empty dead list is what every tick of a world
//! that loses nobody pays anyway.

use std::hash::Hasher;
use std::time::Instant;

use crcbl::core::TickId;
use crcbl::core::rand::{hash_unit, salt};
use crcbl::ecs::{Access, DebugCtx, Entity, Shared, SystemTrait, World};
use crcbl::server::sim_hash::hash_world;

use crate::args::BenchArgs;
use crate::json::Json;
use crate::report::{Failure, Outcome};

use super::{base_environment, environment_line, nanos, timing};

/// The seed every `ecs` run fills its rows from, before a lane's index is
/// salted into it.
///
/// Fixed, for `JOBS_SEED`'s reason: two runs of this scenario build the same
/// world, so a difference between their timings is the schedule and the
/// machine and nothing else.
const SEED: u64 = 0x6372_6362_6c5f_6563;

/// The tick every run steps at: the server's own 60 Hz.
const TICK: f64 = 1.0 / 60.0;

/// Spring passes [`step`] runs over each row, per tick.
///
/// More than one, so a system's tick is enough work that the per-tick number
/// measures the systems rather than the schedule's loop over them; few enough
/// that what the schedule itself costs is still visible beside it at small
/// `--entities`.
const ROW_PASSES: usize = 8;

/// How hard a row's spring pulls it back towards zero.
const STIFFNESS: f64 = 4.0;

/// How much of a row's velocity its spring loses, per second.
const DAMPING: f64 = 0.5;

/// What a writer keeps of its resource's old value when it writes the new one.
///
/// Above zero, so a write depends on the write before it and the order two
/// writers ran in shows in the checksum — at zero each writer would store its
/// own mean whichever ran first. Below one, so the value settles at a bound
/// rather than growing with the length of the run.
const BLEND: f64 = 0.5;

/// The resource written by one system and read by two.
const WIND: &str = "wind";

/// The resource written by two systems and read by a third.
const SCORE: &str = "score";

/// One system of the schedule: its name, and the resource it reads or writes.
struct LaneSpec {
    name: &'static str,
    reads: Option<&'static str>,
    writes: Option<&'static str>,
}

/// The schedule, in registration order.
///
/// `gust` writes [`WIND`]; `drift` and `drag` read it, with two systems that
/// touch nothing between them; `tally` and `audit` both write [`SCORE`], and
/// `pulse` reads it. So the five systems after `gust` conflict with nothing
/// among themselves, `audit` must follow `tally`, and `pulse` must follow
/// both.
const LANES: &[LaneSpec] = &[
    LaneSpec {
        name: "gust",
        reads: None,
        writes: Some(WIND),
    },
    LaneSpec {
        name: "drift",
        reads: Some(WIND),
        writes: None,
    },
    LaneSpec {
        name: "spin",
        reads: None,
        writes: None,
    },
    LaneSpec {
        name: "decay",
        reads: None,
        writes: None,
    },
    LaneSpec {
        name: "drag",
        reads: Some(WIND),
        writes: None,
    },
    LaneSpec {
        name: "tally",
        reads: None,
        writes: Some(SCORE),
    },
    LaneSpec {
        name: "audit",
        reads: None,
        writes: Some(SCORE),
    },
    LaneSpec {
        name: "pulse",
        reads: Some(SCORE),
        writes: None,
    },
];

/// One entity's state in one system.
#[derive(Clone, Copy, Debug)]
struct Row {
    position: f64,
    velocity: f64,
    /// The spring's energy after the last pass, kept so the square root's
    /// result is part of the state rather than a value the optimiser may drop.
    energy: f64,
}

/// A system of the schedule: [`Row`]s it owns, and the resources its tick
/// reads and writes.
struct Lane {
    name: &'static str,
    rows: Vec<Row>,
    input: Option<Shared<f64>>,
    output: Option<Shared<f64>>,
    /// What this lane last wrote to its output, kept so the value lands in the
    /// hash: a resource belongs to no system, so nothing else would hash it.
    written: f64,
}

impl SystemTrait for Lane {
    fn name(&self) -> &str {
        self.name
    }

    fn access(&self) -> Access {
        let mut access = Access::none();
        if let Some(input) = &self.input {
            access = access.reads(input.name());
        }
        if let Some(output) = &self.output {
            access = access.writes(output.name());
        }
        access
    }

    fn tick(&mut self, dt: f64) {
        let input = self.input.as_ref().map_or(0.0, |input| *input.read());
        for row in &mut self.rows {
            step(row, input, dt);
        }
        if let Some(output) = &self.output {
            let mean =
                self.rows.iter().map(|row| row.position).sum::<f64>() / self.rows.len() as f64;
            let mut value = output.write();
            *value = BLEND * *value + mean;
            self.written = *value;
        }
    }

    fn entity_count(&self) -> usize {
        self.rows.len()
    }

    fn sweep(&mut self, _dead: &[Entity]) {}

    fn debug_draw(&mut self, _ctx: &DebugCtx) {}

    fn hash_state(&self, hasher: &mut dyn Hasher) {
        for row in &self.rows {
            hasher.write_u64(row.position.to_bits());
            hasher.write_u64(row.velocity.to_bits());
            hasher.write_u64(row.energy.to_bits());
        }
        hasher.write_u64(self.written.to_bits());
    }

    fn contributes_to_hash(&self) -> bool {
        true
    }

    fn as_any_mut(&mut self) -> &mut dyn std::any::Any {
        self
    }
}

/// One tick of one row: [`ROW_PASSES`] semi-implicit Euler steps of a damped
/// spring driven by `input`.
///
/// Damped and driven by a bounded input, so the rows settle rather than grow
/// however long a run is, and a long run's checksum is as meaningful as a short
/// one's.
fn step(row: &mut Row, input: f64, dt: f64) {
    for _ in 0..ROW_PASSES {
        let accel = input - STIFFNESS * row.position - DAMPING * row.velocity;
        row.velocity += accel * dt;
        row.position += row.velocity * dt;
        row.energy = (row.velocity * row.velocity + STIFFNESS * row.position * row.position).sqrt();
    }
}

/// The world every run ticks: [`LANES`] registered in order, each with
/// `entities` rows, over the two resources they share.
fn build(entities: usize) -> World {
    let mut world = World::new();
    world.set_tick_dt(TICK);
    let resources = [Shared::new(WIND, 0.0_f64), Shared::new(SCORE, 0.0_f64)];
    for resource in &resources {
        // The names are this module's constants and distinct, so no
        // registration can be refused.
        world
            .share(resource)
            .expect("each resource name is registered once");
    }
    let handle = |name: &str| {
        resources
            .iter()
            .find(|resource| resource.name() == name)
            .expect("every name a lane declares is one of the resources above")
            .clone()
    };

    for (index, spec) in LANES.iter().enumerate() {
        let lane_seed = salt(SEED, index as u64);
        let rows = (0..entities as u64)
            .map(|entity| Row {
                position: hash_unit(lane_seed, 2 * entity) * 2.0 - 1.0,
                velocity: hash_unit(lane_seed, 2 * entity + 1) * 2.0 - 1.0,
                energy: 0.0,
            })
            .collect();
        world.register_system(Box::new(Lane {
            name: spec.name,
            rows,
            input: spec.reads.map(handle),
            output: spec.writes.map(handle),
            written: 0.0,
        }));
    }
    world
}

/// What one `ecs` run measured, before it is rendered.
#[derive(Clone, Debug)]
pub(super) struct EcsRun {
    /// Conflicts the schedule derived from [`LANES`]' declarations.
    conflicts: usize,
    /// Nanoseconds per timed tick, ascending.
    sorted: Vec<u64>,
    /// [`hash_world`] once every tick, warm-up included, had run.
    checksum: u64,
}

/// Times [`World::tick`] over the [`build`] world.
///
/// # Errors
///
/// [`Failure`] if the ticks left the world's hash where they found it.
pub(super) fn measure(args: &BenchArgs) -> Result<EcsRun, Failure> {
    let mut world = build(args.entities);
    let untouched = hash_world(&world, TickId::from_raw(0));

    for _ in 0..args.warmup {
        world.tick();
    }
    let mut sorted = Vec::with_capacity(args.iterations);
    for _ in 0..args.iterations {
        let started = Instant::now();
        world.tick();
        sorted.push(nanos(started.elapsed()));
    }

    // The tick id is held at zero for both readings, so a difference between
    // them is the systems' state and not the id the hash folds in.
    let checksum = hash_world(&world, TickId::from_raw(0));
    expect_moved(untouched, checksum)?;

    sorted.sort_unstable();
    Ok(EcsRun {
        conflicts: world.schedule().conflicts().len(),
        sorted,
        checksum,
    })
}

/// Refuses a run whose ticks left the world's hash where they found it.
fn expect_moved(before: u64, after: u64) -> Result<(), Failure> {
    if before != after {
        return Ok(());
    }
    Err(Failure::new(
        "the world hashes the same after the run as before it: the schedule ran no system",
    ))
}

/// Every lane and what it declares, as the human line reads it.
fn schedule_line() -> String {
    LANES
        .iter()
        .map(|spec| match (spec.reads, spec.writes) {
            (Some(read), _) => format!("{} reads {read}", spec.name),
            (None, Some(written)) => format!("{} writes {written}", spec.name),
            (None, None) => spec.name.to_owned(),
        })
        .collect::<Vec<_>>()
        .join(", ")
}

// ---------------------------------------------------------------------------
// Rendering
// ---------------------------------------------------------------------------

/// The two renderings of one finished run.
pub(super) fn report(args: &BenchArgs, run: &EcsRun) -> Outcome {
    let (timing_line, timing_fields) = timing("per tick", &run.sorted);
    let human = format!(
        "{}: {} systems over {} entities each, {} conflicts, {} timed ticks, {} warm-up\n\
         schedule: {}\n\
         {}\n\
         {timing_line}\n\
         checksum: {:016x}",
        args.scenario.name(),
        LANES.len(),
        args.entities,
        run.conflicts,
        args.iterations,
        args.warmup,
        schedule_line(),
        environment_line(),
        run.checksum,
    );

    Outcome {
        human,
        json: vec![
            ("scenario", Json::string(args.scenario.name())),
            ("environment", Json::Object(base_environment())),
            (
                "parameters",
                Json::Object(vec![
                    ("entities", Json::Number(args.entities as i64)),
                    ("systems", Json::Number(LANES.len() as i64)),
                    ("conflicts", Json::Number(run.conflicts as i64)),
                    ("row_passes", Json::Number(ROW_PASSES as i64)),
                    ("iterations", Json::Number(args.iterations as i64)),
                    ("warmup", Json::Number(args.warmup as i64)),
                ]),
            ),
            ("timing", Json::Object(timing_fields)),
            // Hex, as `crcbl sim` prints the same hash, and a string for
            // `super::jobs::report`'s reason: a JSON number is read back as a
            // double, which would round the low bits of the one field whose
            // whole job is to compare exactly.
            ("checksum", Json::string(format!("{:016x}", run.checksum))),
        ],
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::args::{
        BenchScenario, DEFAULT_BENCH_BODIES, DEFAULT_BENCH_EXTENT, DEFAULT_BENCH_TICKS,
    };
    use crate::bench::profile;
    use crcbl::ecs::ConflictKind;

    /// Rows per system in every test here: enough that a mean is a mean, few
    /// enough for a checked build.
    const ENTITIES: usize = 64;

    fn args(iterations: usize, warmup: usize) -> BenchArgs {
        BenchArgs {
            scenario: BenchScenario::Ecs,
            workers: None,
            items: 1,
            chunk: 1,
            // This scenario reads none of them; the parser's own defaults, so
            // that a `BenchArgs` built here is one the parser could have made.
            bodies: DEFAULT_BENCH_BODIES,
            extent: DEFAULT_BENCH_EXTENT,
            ticks: DEFAULT_BENCH_TICKS,
            entities: ENTITIES,
            iterations,
            warmup,
            json: false,
        }
    }

    /// **The schedule has the conflicts the module docs describe**, by pair and
    /// kind: the readers of `wind` after its writer, `audit` after `tally`,
    /// and `pulse` after both — and nothing among the systems between.
    #[test]
    fn the_schedule_conflicts_where_its_declarations_say_and_nowhere_else() {
        let world = build(1);
        let found: Vec<(usize, usize, &str, ConflictKind)> = world
            .schedule()
            .conflicts()
            .iter()
            .map(|conflict| {
                (
                    conflict.before,
                    conflict.after,
                    conflict.resource.as_str(),
                    conflict.kind,
                )
            })
            .collect();
        assert_eq!(
            found,
            [
                (0, 1, WIND, ConflictKind::ReadWrite),
                (0, 4, WIND, ConflictKind::ReadWrite),
                (5, 6, SCORE, ConflictKind::WriteWrite),
                (5, 7, SCORE, ConflictKind::ReadWrite),
                (6, 7, SCORE, ConflictKind::ReadWrite),
            ]
        );
    }

    /// **Two writers of one resource give an answer that depends on their
    /// order**, which is what makes the conflict between them one the
    /// checksum can see rather than one it would pass either way.
    #[test]
    fn swapping_the_two_writers_of_score_changes_the_checksum() {
        fn run(world: &mut World) -> u64 {
            for _ in 0..4 {
                world.tick();
            }
            hash_world(world, TickId::from_raw(0))
        }
        let mut ordered = build(ENTITIES);
        let mut swapped = build(ENTITIES);
        // `tally` and `audit` are adjacent; swapping their rows and names
        // leaves every system holding what it held and only the order of the
        // two writes changed.
        let mut lanes: Vec<&mut Lane> = swapped
            .schedule_mut()
            .iter_mut()
            .filter_map(|system| system.as_any_mut().downcast_mut::<Lane>())
            .collect();
        let (left, right) = lanes.split_at_mut(6);
        std::mem::swap(&mut left[5].rows, &mut right[0].rows);
        std::mem::swap(&mut left[5].name, &mut right[0].name);
        assert_ne!(run(&mut ordered), run(&mut swapped));
    }

    /// **The run reports a full distribution and a checksum that moved**, and
    /// two runs of the same arguments report the same checksum.
    #[test]
    fn a_run_times_every_tick_and_hashes_the_world_it_left() {
        let first = measure(&args(20, 2)).expect("a run");
        assert_eq!(first.sorted.len(), 20);
        assert!(first.sorted.windows(2).all(|pair| pair[0] <= pair[1]));
        assert_eq!(first.conflicts, 5);
        let again = measure(&args(20, 2)).expect("a run");
        assert_eq!(first.checksum, again.checksum);
        // The warm-up's ticks are ticks: one more of them is a different world.
        let longer = measure(&args(20, 3)).expect("a run");
        assert_ne!(first.checksum, longer.checksum);
    }

    /// **A run whose ticks changed nothing is refused**, by that name, and a
    /// run whose ticks changed something is not.
    #[test]
    fn a_run_whose_ticks_change_nothing_is_refused() {
        let Err(failure) = expect_moved(0x5eed, 0x5eed) else {
            panic!("an unmoved hash has to fail the run");
        };
        assert!(failure.message.contains("no system"), "{}", failure.message);
        assert!(expect_moved(0x5eed, 0x5eee).is_ok());
    }

    /// The environment block and the per-tick distribution are in both
    /// renderings, and no mean is in either.
    #[test]
    fn the_output_carries_the_environment_and_the_per_tick_distribution() {
        let args = args(20, 0);
        let outcome = report(&args, &measure(&args).expect("a run"));
        let keys: Vec<&str> = outcome.json.iter().map(|(key, _)| *key).collect();
        assert_eq!(
            keys,
            [
                "scenario",
                "environment",
                "parameters",
                "timing",
                "checksum"
            ]
        );
        assert!(outcome.human.contains("per tick: p50"), "{}", outcome.human);
        assert!(outcome.human.contains(profile()), "{}", outcome.human);
        assert!(
            outcome.human.contains("tally writes score"),
            "{}",
            outcome.human
        );
        assert!(!outcome.human.contains("mean"), "{}", outcome.human);
    }
}
