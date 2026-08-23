//! `crcbl bench` — one fixed scenario, warmed up, timed, and reported as a
//! distribution.
//!
//! `docs/plan/40-profiling.md` schedules "`crcbl bench` with fixed scenarios,
//! warm-up, percentiles, JSON output" and notes against it that "the job system
//! is the first thing that needs proving". This is that first slice: the
//! subcommand, and the one scenario that times [`crcbl::jobs::Pool`] in
//! isolation. It is headless and opens no device — see [`measure`] for what it
//! actually runs.
//!
//! # Human output by default, `--json` on request
//!
//! The plan says "output is JSON by default". This subcommand does the
//! opposite, and that is a decision rather than an oversight: [`crate`]'s own
//! rule table says "`--json` on every subcommand, human output otherwise", and
//! [`crate::report::emit`] is what implements it for every other subcommand.
//! The global contract is the older and the wider rule, and a subcommand that
//! inverted it would be the one surprise in the tool — a script that reads
//! `crcbl lod` and `crcbl bench` the same way should not have to know which of
//! the two flipped the default. Nothing is lost: `--json` is one flag away, and
//! what it prints is the shape a stored baseline will be.
//!
//! # No mean, and the percentiles are refused when they would be a lie
//!
//! The plan's decision, in its own words: "a benchmark reports p50, p95, p99 and
//! max. Frame time is a tail problem — a mean hides exactly the stutter a player
//! notices." So there is no mean here and none is computed. Below
//! [`MIN_PERCENTILE_SAMPLES`] iterations a nearest-rank p95 *is* the maximum, so
//! the run reports its maximum and says why the percentiles are missing rather
//! than printing one number three times under three names.
//!
//! # The environment block is mandatory
//!
//! Also the plan's: "a benchmark pins everything it can and records everything
//! it cannot … a number without those is not comparable to another number." What
//! this scenario can pin, it pins — a fixed seed, a fixed item count, a fixed
//! chunk length, a fixed round count, integer arithmetic throughout. What it
//! cannot, it reports: the machine's architecture and OS, the build profile, the
//! parallelism the spawner offered, and the worker count the pool actually got.
//!
//! **There is no adapter, backend or driver version**, because nothing here
//! opens a device; inventing those fields so the block resembles the plan's
//! GPU-scenario list would be reporting something that was never measured.
//!
//! **And there is no target triple.** A binary cannot read one: Cargo hands
//! `TARGET` to build scripts and to nothing else, and `std` exposes only
//! [`ARCH`](std::env::consts::ARCH), [`OS`](std::env::consts::OS) and
//! [`FAMILY`](std::env::consts::FAMILY) — no vendor, no environment. Those three
//! are reported under their own names. A triple reassembled from a hand-written
//! `cfg!` chain would be right on the targets somebody remembered and quietly
//! wrong on every other one, which is worse than three honest fields.
//!
//! # What this slice deliberately does not have
//!
//! `--compare <baseline>` and `--trace <path>` are separate rows of the same
//! delivery table and are not started here. Neither is a second scenario: the
//! others the plan names live with the samples that own them, and none of those
//! samples has a fixed scenario yet.

use std::fmt::Write as _;
use std::time::{Duration, Instant};

use crcbl::core::rand::hash_u64;
use crcbl::core::stats::{MIN_PERCENTILE_SAMPLES, percentile_of};
use crcbl::jobs::{Pool, PoolStats, default_spawner};

use crate::args::{BenchArgs, BenchScenario};
use crate::json::Json;
use crate::report::{Failure, Outcome};

/// Runs `crcbl bench`.
///
/// # Errors
///
/// [`Failure`] if the pool cannot be built, if a phase did not run the chunks it
/// was asked for, or if the workload did not compute what a serial pass over the
/// same seeds computes.
pub fn run(args: &BenchArgs) -> Result<Outcome, Failure> {
    match args.scenario {
        BenchScenario::Jobs => Ok(report(args, &measure(args)?)),
    }
}

// ---------------------------------------------------------------------------
// The `jobs` scenario
// ---------------------------------------------------------------------------

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
struct JobsRun {
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
fn measure(args: &BenchArgs) -> Result<JobsRun, Failure> {
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
fn report(args: &BenchArgs, run: &JobsRun) -> Outcome {
    let (timing_line, timing_fields) = timing(&run.sorted);
    let human = format!(
        "{}: {} items, chunk {} ({} chunks per call), {} timed calls, {} warm-up\n\
         environment: {} {} ({}), {}, parallelism {}, workers {}\n\
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
        std::env::consts::ARCH,
        std::env::consts::OS,
        std::env::consts::FAMILY,
        profile(),
        run.parallelism,
        run.workers,
        pool_line(run.stats),
        run.warmup_chunks,
        run.checksum,
    );

    Outcome {
        human,
        json: vec![
            ("scenario", Json::string(args.scenario.name())),
            (
                "environment",
                Json::Object(vec![
                    ("arch", Json::string(std::env::consts::ARCH)),
                    ("os", Json::string(std::env::consts::OS)),
                    ("family", Json::string(std::env::consts::FAMILY)),
                    ("profile", Json::string(profile())),
                    ("parallelism", Json::Number(run.parallelism as i64)),
                    ("workers", Json::Number(run.workers as i64)),
                ]),
            ),
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

/// The percentiles this reports, in the order they are printed.
///
/// **No mean**, per the plan's decision — and the list is a constant so that a
/// mean cannot be added to one rendering and forgotten in the other.
const PERCENTILES: &[(usize, &str)] = &[(50, "p50"), (95, "p95"), (99, "p99")];

/// The timed calls, as a line a person reads and the fields a script reads.
///
/// Below [`MIN_PERCENTILE_SAMPLES`] samples the percentiles are omitted from
/// both renderings and the reason is stated, because a nearest-rank p95 over
/// fewer than that is the maximum wearing a percentile's name.
fn timing(sorted: &[u64]) -> (String, Vec<(&'static str, Json)>) {
    let mut fields = vec![
        ("unit", Json::string("ns")),
        ("iterations", Json::Number(sorted.len() as i64)),
        (
            "min_percentile_samples",
            Json::Number(MIN_PERCENTILE_SAMPLES as i64),
        ),
    ];

    let Some(&max) = sorted.last() else {
        return ("per call: nothing was timed".to_string(), fields);
    };
    if sorted.len() < MIN_PERCENTILE_SAMPLES {
        fields.push(("max", Json::Number(max as i64)));
        return (
            format!(
                "per call: max {}, and no percentile — {} samples is below the \
                 {MIN_PERCENTILE_SAMPLES} a nearest-rank p95 needs to be one",
                micros(max),
                sorted.len()
            ),
            fields,
        );
    }

    let mut line = String::from("per call:");
    for &(percent, key) in PERCENTILES {
        // Infallible here: `last()` above answered, so the slice is not empty
        // and every rank in `1..=len` is a sample it holds.
        let value = percentile_of(sorted, percent).expect("a non-empty slice has every rank");
        let _ = write!(line, " {key} {},", micros(value));
        fields.push((key, Json::Number(value as i64)));
    }
    let _ = write!(line, " max {}", micros(max));
    fields.push(("max", Json::Number(max as i64)));
    (line, fields)
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

/// The build profile this binary was compiled at.
///
/// `debug_assertions` is what a binary can read — Cargo hands the profile name
/// to build scripts and to nothing else — and it is the distinction that
/// matters: a checked build and an optimised one differ by an order of
/// magnitude, and a timing that does not say which is not comparable to another.
fn profile() -> &'static str {
    if cfg!(debug_assertions) {
        "debug"
    } else {
        "release"
    }
}

/// Nanoseconds as the microseconds one `par_for` call lands at.
fn micros(nanos: u64) -> String {
    format!("{:.3} µs", nanos as f64 / 1.0e3)
}

/// A duration as whole nanoseconds, saturating rather than wrapping.
fn nanos(duration: Duration) -> u64 {
    u64::try_from(duration.as_nanos()).unwrap_or(u64::MAX)
}

#[cfg(test)]
mod tests {
    use super::*;

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
            iterations,
            warmup,
            json: false,
        }
    }

    /// One `"key":<integer>` out of a rendered field list, or `None` when the
    /// key is absent — which is how the run says it has no percentile.
    fn number(fields: &[(&'static str, Json)], key: &str) -> Option<i64> {
        fields
            .iter()
            .find(|(name, _)| *name == key)
            .map(|(_, value)| match value {
                Json::Number(number) => *number,
                other => panic!("`{key}` is not a number: {other}"),
            })
    }

    /// Ascending samples spaced a microsecond apart, so every rank is a
    /// different number and an off-by-one lands on a value this test names.
    fn spread(count: usize) -> Vec<u64> {
        (1..=count as u64).map(|value| value * 1_000).collect()
    }

    /// **The percentiles are the samples at their ranks, and they ascend.**
    ///
    /// Hand-computed rather than taken off a run: twenty samples of 1..=20 µs
    /// put p50 at the tenth, p95 at the nineteenth, and p99 and the max at the
    /// twentieth. The whole line is pinned, so a percentile printed under
    /// another's name — or a mean appearing beside them — fails here.
    #[test]
    fn the_reported_percentiles_are_the_samples_at_their_ranks_and_ascend() {
        let (line, fields) = timing(&spread(MIN_PERCENTILE_SAMPLES));
        assert_eq!(number(&fields, "p50"), Some(10_000));
        assert_eq!(number(&fields, "p95"), Some(19_000));
        assert_eq!(number(&fields, "p99"), Some(20_000));
        assert_eq!(number(&fields, "max"), Some(20_000));
        assert_eq!(
            line,
            "per call: p50 10.000 µs, p95 19.000 µs, p99 20.000 µs, max 20.000 µs"
        );
        assert_eq!(
            number(&fields, "min_percentile_samples"),
            Some(MIN_PERCENTILE_SAMPLES as i64)
        );
    }

    /// **One sample short of the minimum there is no percentile**, in either
    /// rendering, and the line says which threshold it fell under rather than
    /// printing the maximum three times over.
    #[test]
    fn a_short_run_reports_its_maximum_and_says_why_it_has_no_percentile() {
        let sorted = spread(MIN_PERCENTILE_SAMPLES - 1);
        let (line, fields) = timing(&sorted);
        assert_eq!(number(&fields, "max"), Some(19_000));
        for key in PERCENTILES {
            assert_eq!(number(&fields, key.1), None, "{} survived", key.1);
        }
        assert!(line.contains("max 19.000 µs"), "{line}");
        assert!(line.contains("no percentile"), "{line}");
        assert!(
            line.contains(&MIN_PERCENTILE_SAMPLES.to_string()),
            "the reason has to name the threshold: {line}"
        );
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
