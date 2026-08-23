//! The `jobs` scenario: [`Pool::par_for`] timed over a fixed synthetic
//! workload.
//!
//! [`measure`] is the fixture and the reason it is shaped the way it is;
//! [`super`] owns the subcommand, its dispatch, and the reporting every
//! scenario shares.

use std::time::Instant;

use crcbl::core::rand::hash_u64;
use crcbl::jobs::{Pool, PoolStats, default_spawner};

use crate::args::BenchArgs;
use crate::json::Json;
use crate::report::{Failure, Outcome};

use super::{base_environment, environment_line, nanos, timing};

/// The seed every `jobs` run builds its items from.
///
/// Fixed, because the plan requires a benchmark to pin everything it can: two
/// runs of this scenario mix identical bits, so a difference between their
/// timings is the pool and the machine and nothing else.
const JOBS_SEED: u64 = 0x6372_6362_6c5f_6a6f;

/// Mixing rounds each item goes through, per call.
///
/// Chosen so one chunk of the default `--chunk` costs a few microseconds — the
/// scale horde's steering pass works at, which is the pass this scenario stands
/// in for. Too little work per item and the measurement is the pool's overhead
/// alone; too much and the overhead it exists to expose disappears into it.
const JOBS_ROUNDS: u64 = 64;

/// What one run of the `jobs` scenario measured, before it is rendered.
#[derive(Clone, Debug)]
pub(super) struct JobsRun {
    /// What the spawner said the machine has.
    parallelism: usize,
    /// Workers the pool actually got, which is not the request when the spawner
    /// has no threads to give.
    workers: usize,
    /// Chunks one `par_for` call splits the workload into.
    chunks_per_call: usize,
    /// Chunks the warm-up ran, read before the counters were reset.
    warmup_chunks: u64,
    /// Nanoseconds per timed call, ascending.
    sorted: Vec<u64>,
    /// The pool's counters over the timed calls alone.
    stats: PoolStats,
    /// The finished workload's fold. See [`checksum`].
    checksum: u64,
}

/// Times [`Pool::par_for`] over a fixed synthetic workload.
///
/// The workload is deliberately dull: `--items` `u64`s, each mixed through
/// [`JOBS_ROUNDS`] rounds of splitmix64's finaliser by [`work`]. It is CPU-only,
/// allocates nothing in the timed region, reads no clock inside a chunk and
/// touches nothing outside its own slice, so what varies between two runs is how
/// the pool split and distributed the work.
///
/// **The result is used, or the loop is not there.** Each item's new value is
/// written back, the finished array is folded into a checksum the output
/// carries, and that checksum is compared against a serial pass over the same
/// seeds — so an optimiser that deleted the work, a chunk that ran twice and a
/// chunk that never ran are each a failure rather than a fast number.
pub(super) fn measure(args: &BenchArgs) -> Result<JobsRun, Failure> {
    let spawner = default_spawner();
    let parallelism = spawner.parallelism().get();
    // `Pool::new`'s own rule, spelled out here because the count is reported:
    // one fewer worker than the machine's parallelism, since the thread calling
    // `par_for` runs chunks too. `--workers 0` is the serial baseline.
    let requested = args
        .workers
        .unwrap_or_else(|| parallelism.saturating_sub(1));
    let mut pool = Pool::with_workers(spawner.as_ref(), requested)
        .map_err(|error| Failure::new(format!("could not build the pool: {error}")))?;

    let chunks_per_call = args.items.div_ceil(args.chunk);
    let mut items = vec![0u64; args.items];

    // The answer, computed on this thread with no pool in it at all.
    seed(&mut items);
    for item in &mut items {
        *item = work(*item);
    }
    let expected = checksum(&items);

    for _ in 0..args.warmup {
        seed(&mut items);
        pool.par_for(&mut items, args.chunk, pass);
    }
    // Read *before* the reset, which is the only moment the warm-up is
    // observable at all: a warm-up that silently ran nothing would otherwise
    // leave a run that still prints a plausible distribution.
    let warmup_chunks = ran_chunks(pool.stats());
    expect_chunks(warmup_chunks, args.warmup, chunks_per_call, "the warm-up")?;
    pool.reset_stats();

    let mut sorted = Vec::with_capacity(args.iterations);
    for _ in 0..args.iterations {
        // Outside the timer: re-seeding is the harness's cost and not the
        // pool's, and it is what makes every timed call do identical work.
        seed(&mut items);
        let started = Instant::now();
        pool.par_for(&mut items, args.chunk, pass);
        sorted.push(nanos(started.elapsed()));
    }
    let stats = pool.stats();
    // The other half of the same claim: the counters now describe the timed
    // calls alone, so the warm-up's chunks were excluded rather than merely
    // unreported.
    expect_chunks(
        ran_chunks(stats),
        args.iterations,
        chunks_per_call,
        "the timed run",
    )?;

    let produced = checksum(&items);
    if produced != expected {
        return Err(Failure::new(format!(
            "the pool computed {produced} where a serial pass over the same seeds computes \
             {expected}: the workload was not run the way it was written"
        )));
    }

    sorted.sort_unstable();
    Ok(JobsRun {
        parallelism,
        workers: pool.workers(),
        chunks_per_call,
        warmup_chunks,
        sorted,
        stats,
        checksum: produced,
    })
}

/// One `par_for` chunk of the workload.
///
/// A named function rather than a closure so the warm-up and the timed loop run
/// the same code by construction — two copies of a body this small is exactly
/// where a warm-up quietly stops warming up the thing being measured.
///
/// The leading index is the chunk's first item, which a real caller uses to
/// address the *other* SoA arrays it reads. This workload reads only its own
/// chunk, so it ignores it.
fn pass(_start: usize, chunk: &mut [u64]) {
    for item in chunk {
        *item = work(*item);
    }
}

/// One item's work: [`JOBS_ROUNDS`] passes of splitmix64's finaliser.
///
/// [`crcbl::core::rand::hash_u64`] is the engine's own, and reaching for it
/// rather than writing a second copy of the same constants is the point: it is
/// integer arithmetic with no floating point and no dependence on word order, so
/// this workload computes the same values on every target and its checksum is a
/// portable claim rather than a local one.
fn work(mut value: u64) -> u64 {
    for round in 0..JOBS_ROUNDS {
        value = hash_u64(value, round);
    }
    value
}

/// Fills `items` with the run's fixed starting values.
fn seed(items: &mut [u64]) {
    for (index, item) in items.iter_mut().enumerate() {
        *item = hash_u64(JOBS_SEED, index as u64);
    }
}

/// Folds a finished workload into one value, in index order.
///
/// Order-dependent on purpose: a chunk that ran out of place, twice, or not at
/// all changes this, where a sum over the items would not.
fn checksum(items: &[u64]) -> u64 {
    items
        .iter()
        .fold(0, |folded, &value| hash_u64(folded, value))
}

/// Chunks a [`PoolStats`] accounts for, the driver's and the workers' together.
///
/// Read between submissions, where [`PoolStats`] states this is exactly the
/// number of chunks every completed `par_for` split into.
const fn ran_chunks(stats: PoolStats) -> u64 {
    stats
        .chunks_run_by_driver
        .saturating_add(stats.chunks_run_by_workers)
}

/// Refuses a phase that did not run the chunks it was asked for.
///
/// Both readings this scenario takes go through here, and between them they are
/// what makes the warm-up's exclusion a checked claim rather than a comment.
fn expect_chunks(
    ran: u64,
    calls: usize,
    chunks_per_call: usize,
    phase: &str,
) -> Result<(), Failure> {
    let expected = calls as u64 * chunks_per_call as u64;
    if ran == expected {
        return Ok(());
    }
    Err(Failure::new(format!(
        "{phase} ran {ran} chunks where {calls} calls of {chunks_per_call} chunks each is \
         {expected}: the pool's counters and its work have parted company"
    )))
}

// ---------------------------------------------------------------------------
// Rendering
// ---------------------------------------------------------------------------

/// The two renderings of one finished run.
pub(super) fn report(args: &BenchArgs, run: &JobsRun) -> Outcome {
    let (timing_line, timing_fields) = timing("per call", &run.sorted);
    let human = format!(
        "{}: {} items, chunk {} ({} chunks per call), {} timed calls, {} warm-up\n\
         {}, parallelism {}, workers {}\n\
         {timing_line}\n\
         {}\n\
         warm-up: {} chunks, run and then discarded\n\
         checksum: {}",
        args.scenario.name(),
        args.items,
        args.chunk,
        run.chunks_per_call,
        args.iterations,
        args.warmup,
        environment_line(),
        run.parallelism,
        run.workers,
        pool_line(run.stats),
        run.warmup_chunks,
        run.checksum,
    );

    let mut environment = base_environment();
    environment.push(("parallelism", Json::Number(run.parallelism as i64)));
    environment.push(("workers", Json::Number(run.workers as i64)));

    Outcome {
        human,
        json: vec![
            ("scenario", Json::string(args.scenario.name())),
            ("environment", Json::Object(environment)),
            (
                "parameters",
                Json::Object(vec![
                    ("items", Json::Number(args.items as i64)),
                    ("chunk", Json::Number(args.chunk as i64)),
                    ("chunks_per_call", Json::Number(run.chunks_per_call as i64)),
                    ("iterations", Json::Number(args.iterations as i64)),
                    ("warmup", Json::Number(args.warmup as i64)),
                ]),
            ),
            ("timing", Json::Object(timing_fields)),
            ("warmup_chunks", Json::Number(run.warmup_chunks as i64)),
            ("pool", Json::Object(pool_fields(run.stats))),
            // A string, not a number: this is a full 64-bit fold, and a JSON
            // number is read back as a double by most consumers, which would
            // round the low bits of the one field whose whole job is to compare
            // exactly.
            ("checksum", Json::string(run.checksum.to_string())),
        ],
    }
}

/// The pool's counters, over the timed calls alone.
fn pool_line(stats: PoolStats) -> String {
    format!(
        "pool over the timed calls: {} chunks — {} on the driver, {} on workers; {} steals, \
         {} empty searches, {} lost exchanges, {} parks, longest queue {}, {} submissions",
        ran_chunks(stats),
        stats.chunks_run_by_driver,
        stats.chunks_run_by_workers,
        stats.steals,
        stats.steal_failures,
        stats.steal_retries,
        stats.parks,
        stats.longest_queue,
        stats.submissions,
    )
}

/// The same counters, under the names [`PoolStats`] gives its fields.
fn pool_fields(stats: PoolStats) -> Vec<(&'static str, Json)> {
    vec![
        (
            "chunks_run_by_driver",
            Json::Number(stats.chunks_run_by_driver as i64),
        ),
        (
            "chunks_run_by_workers",
            Json::Number(stats.chunks_run_by_workers as i64),
        ),
        ("steals", Json::Number(stats.steals as i64)),
        ("steal_failures", Json::Number(stats.steal_failures as i64)),
        ("steal_retries", Json::Number(stats.steal_retries as i64)),
        ("parks", Json::Number(stats.parks as i64)),
        ("longest_queue", Json::Number(stats.longest_queue as i64)),
        ("submissions", Json::Number(stats.submissions as i64)),
    ]
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::args::{
        BenchScenario, DEFAULT_BENCH_BODIES, DEFAULT_BENCH_EXTENT, DEFAULT_BENCH_TICKS,
    };
    use crate::bench::profile;

    /// Items every test in this module runs over, and the chunk it splits them
    /// into. Small enough for the ordinary suite and still more chunks than any
    /// desktop has cores, so the parallel path is the one being exercised.
    const ITEMS: usize = 512;
    const CHUNK: usize = 16;
    /// What [`ITEMS`] and [`CHUNK`] make one call cost, in chunks.
    const CHUNKS_PER_CALL: u64 = (ITEMS / CHUNK) as u64;

    fn args(workers: usize, iterations: usize, warmup: usize) -> BenchArgs {
        BenchArgs {
            scenario: BenchScenario::Jobs,
            workers: Some(workers),
            items: ITEMS,
            chunk: CHUNK,
            // This scenario reads none of them, and `--bodies`/`--extent`/
            // `--ticks` are refused on a `jobs` invocation; the parser's own
            // defaults, so that a `BenchArgs` built here is one the parser
            // could have made.
            bodies: DEFAULT_BENCH_BODIES,
            extent: DEFAULT_BENCH_EXTENT,
            ticks: DEFAULT_BENCH_TICKS,
            iterations,
            warmup,
            json: false,
        }
    }

    /// **The warm-up runs, and it is excluded from everything reported.**
    ///
    /// Both halves are checked, because either alone passes on a broken run: a
    /// warm-up that did nothing leaves the measured counters right, and a reset
    /// that never happened leaves the warm-up's count right.
    #[test]
    fn the_warm_up_runs_and_is_excluded_from_the_measured_counters() {
        let iterations = 5;
        let warmed = measure(&args(2, iterations, 3)).expect("a run");
        assert_eq!(warmed.chunks_per_call as u64, CHUNKS_PER_CALL);
        assert_eq!(warmed.warmup_chunks, 3 * CHUNKS_PER_CALL);
        assert_eq!(
            ran_chunks(warmed.stats),
            iterations as u64 * CHUNKS_PER_CALL
        );
        assert_eq!(warmed.sorted.len(), iterations);

        // No warm-up at all is a legal request, and then there is nothing to
        // exclude — the counters read the same as the warmed run's.
        let cold = measure(&args(2, iterations, 0)).expect("a run");
        assert_eq!(cold.warmup_chunks, 0);
        assert_eq!(ran_chunks(cold.stats), iterations as u64 * CHUNKS_PER_CALL);
        assert_eq!(
            cold.checksum, warmed.checksum,
            "the warm-up changes the timings and nothing else"
        );
    }

    /// **`--workers 0` puts every chunk on the driver**, which is what makes it
    /// the serial baseline rather than a pool that happens to be small.
    #[test]
    fn no_workers_puts_every_chunk_on_the_driver() {
        let iterations = 5;
        let serial = measure(&args(0, iterations, 2)).expect("a run");
        assert_eq!(serial.workers, 0);
        assert_eq!(serial.stats.chunks_run_by_workers, 0);
        assert_eq!(
            serial.stats.chunks_run_by_driver,
            iterations as u64 * CHUNKS_PER_CALL
        );
        assert_eq!(serial.stats.steals, 0);
        assert_eq!(
            serial.stats.submissions, 0,
            "an inline call queues nothing, so it is not a submission"
        );

        // And the baseline reaches the same answer as the parallel run, which
        // is the whole reason a baseline is worth comparing against.
        //
        // **What proves the second run took the other path is `submissions`,
        // not the workers' chunk count.** Whether a worker gets a chunk is the
        // scheduler's to decide: the driver pops from its own end of the deque
        // while the thieves are still waking, and on a loaded macOS runner it
        // took all of them, which failed this test on a claim it does not own.
        // `submissions` is incremented once by the parallel path and never by
        // the inline one, so it separates the two arms by construction.
        let parallel = measure(&args(3, iterations, 2)).expect("a run");
        assert_eq!(serial.checksum, parallel.checksum);
        assert_eq!(
            parallel.stats.submissions, iterations as u64,
            "a pool with workers did not take the parallel path"
        );
        assert_eq!(
            parallel.stats.chunks_run_by_driver + parallel.stats.chunks_run_by_workers,
            iterations as u64 * CHUNKS_PER_CALL,
            "the parallel arm lost or repeated a chunk"
        );
    }

    /// **The work actually happens.**
    ///
    /// The checksum is compared against a serial fold computed here — not a
    /// literal, which would only pin whatever the workload does today — and
    /// against the fold of the untouched seeds, which is what a `par_for` the
    /// optimiser had emptied would produce.
    #[test]
    fn the_workload_computes_the_serial_answer_rather_than_nothing() {
        let run = measure(&args(0, 2, 0)).expect("a run");

        let mut expected = vec![0u64; ITEMS];
        seed(&mut expected);
        let untouched = checksum(&expected);
        for item in &mut expected {
            *item = work(*item);
        }
        assert_eq!(run.checksum, checksum(&expected));
        assert_ne!(
            run.checksum, untouched,
            "a pass that did nothing would fold the seeds it started from"
        );
    }

    /// The environment block is mandatory, so it is asserted rather than
    /// assumed: every field the module docs promise is in both renderings.
    #[test]
    fn the_output_carries_the_environment_the_numbers_came_from() {
        let args = args(0, 2, 0);
        let outcome = report(&args, &measure(&args).expect("a run"));
        let Some((_, Json::Object(environment))) = outcome
            .json
            .iter()
            .find(|(key, _)| *key == "environment")
            .cloned()
        else {
            panic!("no environment block in {:?}", outcome.json);
        };
        let keys: Vec<&str> = environment.iter().map(|(key, _)| *key).collect();
        assert_eq!(
            keys,
            ["arch", "os", "family", "profile", "parallelism", "workers"]
        );
        assert!(outcome.human.contains(profile()), "{}", outcome.human);
        assert!(
            outcome.human.contains(std::env::consts::ARCH),
            "{}",
            outcome.human
        );
        assert!(
            !outcome.human.contains("mean"),
            "the plan's decision is percentiles and never a mean: {}",
            outcome.human
        );
    }
}
