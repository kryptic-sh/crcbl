//! `crcbl bench` — one fixed scenario, warmed up, timed, and reported as a
//! distribution.
//!
//! `docs/plan/40-profiling.md` schedules "`crcbl bench` with fixed scenarios,
//! warm-up, percentiles, JSON output" and notes against it that "the job system
//! is the first thing that needs proving". This is the subcommand and its two
//! scenarios: `jobs`, which times [`crcbl::jobs::Pool`] in isolation — see
//! [`measure`] — and `phys`, which times `crcbl-phys`'s broadphase on one
//! thread — see [`phys`]. Both are headless and neither opens a device.
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
//! delivery table and are not started here, so nothing below reads or writes a
//! stored baseline: a run is compared against another run by a person holding
//! two `--json` outputs.
//!
//! The remaining scenarios the plan names live with the samples that own them,
//! and none of those samples has a fixed scenario yet.

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
/// [`Failure`] if the pool cannot be built, if a phase did not run the chunks or
/// the queries it was asked for, or if the workload did not compute what a
/// serial pass over the same seeds computes.
pub fn run(args: &BenchArgs) -> Result<Outcome, Failure> {
    match args.scenario {
        BenchScenario::Jobs => Ok(report(args, &measure(args)?)),
        BenchScenario::Phys => Ok(phys::report(args, &phys::measure(args)?)),
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

/// The percentiles this reports, in the order they are printed.
///
/// **No mean**, per the plan's decision — and the list is a constant so that a
/// mean cannot be added to one rendering and forgotten in the other.
const PERCENTILES: &[(usize, &str)] = &[(50, "p50"), (95, "p95"), (99, "p99")];

/// One timed phase, as a line a person reads and the fields a script reads.
///
/// `label` opens the human line and is the phase's name — `per call` for the
/// one distribution `jobs` has, and one of [`phys`]'s three otherwise. The
/// fields are the same shape either way, so a consumer reads three phases the
/// way it reads one.
///
/// Below [`MIN_PERCENTILE_SAMPLES`] samples the percentiles are omitted from
/// both renderings and the reason is stated, because a nearest-rank p95 over
/// fewer than that is the maximum wearing a percentile's name.
fn timing(label: &str, sorted: &[u64]) -> (String, Vec<(&'static str, Json)>) {
    let mut fields = vec![
        ("unit", Json::string("ns")),
        ("iterations", Json::Number(sorted.len() as i64)),
        (
            "min_percentile_samples",
            Json::Number(MIN_PERCENTILE_SAMPLES as i64),
        ),
    ];

    let Some(&max) = sorted.last() else {
        return (format!("{label}: nothing was timed"), fields);
    };
    if sorted.len() < MIN_PERCENTILE_SAMPLES {
        fields.push(("max", Json::Number(max as i64)));
        return (
            format!(
                "{label}: max {}, and no percentile — {} samples is below the \
                 {MIN_PERCENTILE_SAMPLES} a nearest-rank p95 needs to be one",
                micros(max),
                sorted.len()
            ),
            fields,
        );
    }

    let mut line = format!("{label}:");
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

/// The environment fields every scenario reports, in the order they print.
///
/// Shared because the plan's "a number without those is not comparable to
/// another number" is a rule about every benchmark, not about one of them —
/// and because two copies of a four-field list is where the second scenario
/// quietly stops recording the profile. A scenario with more to pin appends to
/// this; see [`report`], which adds the pool's counts.
fn base_environment() -> Vec<(&'static str, Json)> {
    vec![
        ("arch", Json::string(std::env::consts::ARCH)),
        ("os", Json::string(std::env::consts::OS)),
        ("family", Json::string(std::env::consts::FAMILY)),
        ("profile", Json::string(profile())),
    ]
}

/// [`base_environment`] as the line a person reads, with no trailing newline.
fn environment_line() -> String {
    format!(
        "environment: {} {} ({}), {}",
        std::env::consts::ARCH,
        std::env::consts::OS,
        std::env::consts::FAMILY,
        profile(),
    )
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

// ---------------------------------------------------------------------------
// The `phys` scenario
// ---------------------------------------------------------------------------

/// `crcbl-phys`'s broadphase at scale: build, refit and query, timed apart.
///
/// # Why this scenario exists, and what it refuses to report
///
/// `docs/plan/ROADMAP.md`'s P8 proposes adopting the physics broadphase onto
/// `crcbl-jobs`, and `docs/backlog.md` records that nothing has ever timed it:
/// the allocations `overlap_sphere_into` removed "are justified by the count,
/// not by a measurement, and anybody quoting it as a speed-up is quoting
/// something nobody ran". This is the harness that stops that being true, and
/// the numbers it produces are what P8 gets to design against.
///
/// The same backlog entry fixes the shape of the fixture: **"a broadphase query
/// costs what its *answer* costs, so the tick's cost tracks local density rather
/// than entity count"** — the same ten thousand `apps/horde` enemies cost
/// 14.66 ms a tick spread over the arena and 84.09 ms converged on the player.
/// So a run that printed one duration for "N bodies" would be printing the
/// misleading half of a two-number fact. Two things follow, and they are the
/// design of this module:
///
/// 1. **Density is a parameter**, spelled `--extent`: the side of the square
///    arena the crowd is placed in, in whole world units. Not "neighbours per
///    query", which is what a reader wants to know — because an extent is what
///    the fixture *uses*, with no arithmetic between the flag and the placement,
///    while a neighbour target would be a request the placement could silently
///    miss while still printing the number it was asked for. The neighbour count
///    is the run's *output*, measured, and it is what makes the two runs at one
///    body count comparable.
/// 2. **Every run carries its answer size.** `answers` reports the results the
///    query phase returned and the mean per query, beside the query timing, so a
///    reader cannot take a duration away without the density that produced it.
///
/// # Three phases, three distributions
///
/// One iteration is one build, one refit and one query pass, timed separately
/// and reported as three distributions rather than one total:
///
/// * **build** — `bodies` [`PhysicsWorld::add_sphere`] calls into a fresh world,
///   then the [`PhysicsWorld::overlap_queries`] that makes the tree current.
///   Adds before the first query accumulate, so this is one bulk `Bvh::build`.
/// * **refit** — every body moved one tick's worth with
///   [`PhysicsWorld::set_sphere`], then the tree made current again. `set_sphere`
///   refits in place when it can and drops the tree when it cannot, in which
///   case the rebuild lands in this sample — which is the honest answer, because
///   it is what a caller pays.
/// * **query** — `bodies` sphere overlaps, one per body, at its own position:
///   the shape `apps/horde`'s separation pass runs, through
///   [`OverlapQueries::overlap_sphere_into`] with a [`QueryScratch`], which is
///   the form P8's parallel adoption will use.
///
/// **There is no threading here and that is deliberate**: a pool in the middle
/// of the pass would measure the pool. `jobs` is the scenario that measures the
/// pool, and P8 is what puts the two together.
///
/// # The answers are checked, and the check is in two halves
///
/// [`serial_answers`] runs the same pass with no tree in it at all — every body
/// against every body, `O(bodies²)`, through the same
/// [`sphere_overlaps_sphere`] predicate the broadphase's exact test calls. That
/// is the answer everything else is held to.
///
/// [`full_answers`] then runs one untimed pass through the broadphase and folds
/// **which** bodies answered which query, and the run fails unless it reproduces
/// the serial fold exactly. That fold needs a `ColliderId` → body-index map and
/// therefore a hash lookup per result, which is why it is untimed and run once:
/// a `HashMap` inside the query phase would be reported as part of the
/// broadphase's cost, in the one number this scenario exists to give away.
///
/// Every timed pass — and every warm-up pass — is then held to that pass's
/// [`Tally`]: the total results, and a fold of each query's result count in
/// query order. So a pass that answered nothing, answered short, or ran its
/// queries out of order fails the run instead of reporting a fast number.
///
/// # Deterministic, and portable rather than merely reproducible
///
/// Placement and movement come from [`hash_unit`], whose value is the top 53
/// bits of [`hash_u64`] over a power-of-two divisor and therefore exact. Every
/// operation after it is a multiply, an add or a compare, and
/// [`sphere_overlaps_sphere`] is the same — **nothing here calls a
/// transcendental**, whose results differ between glibc, Apple's libm and MSVC.
/// So two runs with the same arguments place the same crowd and fold the same
/// checksum on every target, not just twice on this one. A heading angle would
/// have been the natural way to move a body and is not used for exactly that
/// reason; the drift is two independent values in `[-1, 1]` instead, which is
/// not uniform over a circle and does not need to be.
mod phys {
    use std::collections::HashMap;

    use std::time::Instant;

    use crcbl::core::rand::{hash_u64, hash_unit};
    use crcbl::math::DVec3;
    use crcbl::phys::{
        BroadphaseStats, ColliderId, PhysicsWorld, QueryScratch, Sphere, sphere_overlaps_sphere,
    };

    use crate::args::BenchArgs;
    use crate::json::Json;
    use crate::report::{Failure, Outcome};

    use super::{base_environment, environment_line, nanos, timing};

    /// The seed every `phys` run places its crowd from.
    ///
    /// Fixed, for `JOBS_SEED`'s reason: two runs of this scenario build
    /// the same world, so a difference between their timings is the crate and
    /// the machine and nothing else.
    const SEED: u64 = 0x6372_6362_6c5f_7068;

    /// The seed the one-tick drift comes from.
    ///
    /// A second seed rather than a second index range into [`SEED`], so that
    /// changing how far bodies move cannot move where they started.
    const DRIFT_SEED: u64 = 0x6372_6362_6c5f_6472;

    /// Every body's collider radius, in world units.
    ///
    /// One radius for the whole crowd, where `apps/horde` has one per enemy
    /// kind: a benchmark whose neighbour count moved with the mix of kinds would
    /// need the mix reported beside it to mean anything.
    const BODY_RADIUS: f64 = 0.5;

    /// Clear space the query looks for beyond a body's own surface.
    ///
    /// `apps/horde`'s `SEPARATION_SLACK`, because the query phase stands in for
    /// its steering pass and the slack is that query's whole tuning knob.
    const SLACK: f64 = 0.35;

    /// The radius every overlap query is run at: `r_self + slack`.
    ///
    /// The neighbour's own radius is deliberately absent — `overlap_sphere_into`
    /// tests the query sphere against each collider's *shape*, so this returns
    /// every body whose centre is within `QUERY_RADIUS + BODY_RADIUS`. That is
    /// `apps/horde`'s `separation_query_radius`, and getting it wrong here would
    /// change the density the run reports without changing anything it prints.
    const QUERY_RADIUS: f64 = BODY_RADIUS + SLACK;

    /// How fast a body travels, in world units a second.
    ///
    /// A grunt's speed in `apps/horde`, which is the crowd this fixture stands
    /// in for.
    const SPEED: f64 = 3.2;

    /// One tick of the simulation the refit phase moves the crowd through.
    ///
    /// `apps/horde` runs at 60 Hz, and the refit phase is one of its ticks.
    const TICK: f64 = 1.0 / 60.0;

    /// How far a body drifts between the build phase and the refit phase.
    ///
    /// Small on purpose: a refit's cost is a function of how much a leaf's box
    /// has to grow, and a body teleporting across the arena would measure a tree
    /// rebuild wearing a refit's name.
    const DRIFT: f64 = SPEED * TICK;

    /// The salt the per-result fold mixes a body index under.
    const RESULT_SALT: u64 = 0x6e65_6967_6862_6f75;

    /// What one `phys` run measured, before it is rendered.
    #[derive(Clone, Debug)]
    pub(super) struct Run {
        /// Nanoseconds per timed build pass, ascending.
        build: Vec<u64>,
        /// Nanoseconds per timed refit pass, ascending.
        refit: Vec<u64>,
        /// Nanoseconds per timed query pass, ascending.
        query: Vec<u64>,
        /// What one query pass answers, verified against [`serial_answers`].
        answers: Answers,
        /// Results the warm-up's query passes returned, summed.
        ///
        /// A sum of what those passes *answered* rather than a count of how many
        /// ran, which is what makes the warm-up observable at all: a warm-up
        /// that skipped its queries cannot reach this number, where an iteration
        /// counter would report it having run.
        warmup_results: u64,
        /// Results the timed query passes returned, summed.
        timed_results: u64,
        /// The tree the query phase ran against.
        broadphase: BroadphaseStats,
    }

    /// How much one query pass answered, and in what shape.
    ///
    /// Cheap enough to compute on every pass, warm-up included: one add and one
    /// mix per query, and nothing per result. `shape` folds each query's result
    /// count **in query order**, so a pass that answered the right total across
    /// the wrong queries does not match.
    #[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
    struct Tally {
        /// Results returned, summed over every query in the pass.
        results: u64,
        /// Each query's result count, folded in query order.
        shape: u64,
    }

    /// A [`Tally`] plus the fold over *which* bodies answered.
    ///
    /// The per-query part of `checksum` is an order-independent sum, because the
    /// order a query returns its neighbours in is the BVH's traversal order and
    /// `docs/backlog.md` records that `apps/horde` chose to live with it — a
    /// checksum that depended on it would be pinning the tree's shape rather
    /// than its answer. Across queries the fold *is* ordered, because the query
    /// order is the body order and nothing about the tree may change it.
    #[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
    struct Answers {
        /// The cheap summary, so a timed pass can be held to it.
        tally: Tally,
        /// The identity fold. See the type's own docs.
        checksum: u64,
    }

    /// Where body `index` starts, on the arena floor.
    ///
    /// `extent` is the arena's side in whole world units, so the crowd fills
    /// `[0, extent)` on both axes and halving it quadruples the density. The
    /// floor is XZ because `+Y` is up — the engine's axes, so the crowd is laid
    /// out the way `apps/horde`'s is rather than in a cube nothing plays in.
    fn place(index: usize, extent: usize) -> DVec3 {
        let extent = extent as f64;
        let index = index as u64;
        DVec3::new(
            hash_unit(SEED, 2 * index) * extent,
            0.0,
            hash_unit(SEED, 2 * index + 1) * extent,
        )
    }

    /// Body `index` after one tick's worth of movement from `resting`.
    fn drifted(index: usize, resting: DVec3) -> DVec3 {
        let index = index as u64;
        let offset = DVec3::new(
            hash_unit(DRIFT_SEED, 2 * index) * 2.0 - 1.0,
            0.0,
            hash_unit(DRIFT_SEED, 2 * index + 1) * 2.0 - 1.0,
        );
        resting + offset * DRIFT
    }

    /// What a body index contributes to the query it answers.
    fn mark(index: u64) -> u64 {
        hash_u64(RESULT_SALT, index)
    }

    /// The whole pass with no tree in it at all: every body against every body.
    ///
    /// `O(bodies²)` and deliberately dumb — it is what the broadphase is checked
    /// against, so it must not share the broadphase's structure. It calls
    /// [`sphere_overlaps_sphere`], which is the same predicate
    /// `overlap_sphere_into`'s exact test calls, so the two agree at the
    /// boundary by construction rather than by a transcription that has to be
    /// right.
    ///
    /// A body overlaps itself, so a correct pass answers at least one result per
    /// query and `results` is never below `centres.len()`.
    fn serial_answers(centres: &[DVec3]) -> Answers {
        let mut answers = Answers::default();
        for &centre in centres {
            let query = Sphere::new(centre, QUERY_RADIUS);
            let mut results = 0u64;
            let mut neighbours = 0u64;
            for (index, &other) in centres.iter().enumerate() {
                if sphere_overlaps_sphere(&query, &Sphere::new(other, BODY_RADIUS)) {
                    results += 1;
                    neighbours = neighbours.wrapping_add(mark(index as u64));
                }
            }
            answers.tally.results += results;
            answers.tally.shape = hash_u64(answers.tally.shape, results);
            answers.checksum = hash_u64(answers.checksum, neighbours);
        }
        answers
    }

    /// One untimed pass through the broadphase, folding *which* bodies answered.
    ///
    /// # Errors
    ///
    /// [`Failure`] if a query answers with a collider this run never added,
    /// which would mean the tree no longer describes the world it was built
    /// from — a ghost, and not something to fold into a checksum.
    fn full_answers(
        world: &mut PhysicsWorld,
        ids: &[ColliderId],
        centres: &[DVec3],
    ) -> Result<Answers, Failure> {
        let index_of: HashMap<ColliderId, u64> = ids
            .iter()
            .enumerate()
            .map(|(index, &id)| (id, index as u64))
            .collect();

        let mut scratch = QueryScratch::new();
        let mut out = Vec::new();
        let mut answers = Answers::default();

        let queries = world.overlap_queries();
        for &centre in centres {
            queries.overlap_sphere_into(centre, QUERY_RADIUS, &mut scratch, &mut out);
            let mut neighbours = 0u64;
            for id in &out {
                let Some(&index) = index_of.get(id) else {
                    return Err(Failure::new(
                        "a query answered with a collider this run never added: the tree no \
                         longer describes the world it was built from",
                    ));
                };
                neighbours = neighbours.wrapping_add(mark(index));
            }
            answers.tally.results += out.len() as u64;
            answers.tally.shape = hash_u64(answers.tally.shape, out.len() as u64);
            answers.checksum = hash_u64(answers.checksum, neighbours);
        }
        Ok(answers)
    }

    /// Refuses a query pass that did not answer what the serial scan answers.
    ///
    /// Every pass this scenario runs goes through here, warm-up included, which
    /// is what makes "the queries ran" a checked claim rather than a comment.
    ///
    /// # Errors
    ///
    /// [`Failure`], with the empty pass named separately: a pass that returned
    /// nothing at all is the failure most likely to be read as a fast number,
    /// so it says so rather than arriving as a checksum mismatch.
    fn expect_answers(phase: &str, produced: Tally, expected: Tally) -> Result<(), Failure> {
        if produced.results == 0 {
            return Err(Failure::new(format!(
                "{phase} answered nothing at all, where a scan with no tree in it answers {} \
                 results: a pass that found nothing is a fast number and not a measurement",
                expected.results
            )));
        }
        if produced.results != expected.results {
            return Err(Failure::new(format!(
                "{phase} answered {} results where a scan with no tree in it answers {}: the \
                 broadphase and the world it was built from have parted company",
                produced.results, expected.results
            )));
        }
        if produced.shape == expected.shape {
            return Ok(());
        }
        Err(Failure::new(format!(
            "{phase} answered the right {} results across the wrong queries: its per-query \
             counts fold to {} where the scan's fold to {}",
            produced.results, produced.shape, expected.shape
        )))
    }

    /// Times the broadphase's build, refit and query phases over one fixture.
    ///
    /// # Errors
    ///
    /// [`Failure`] if a body cannot be moved, if the tree does not hold every
    /// body, or if any pass — warm-up or timed — does not answer what a serial
    /// scan over the same crowd answers.
    pub(super) fn measure(args: &BenchArgs) -> Result<Run, Failure> {
        let resting: Vec<DVec3> = (0..args.bodies)
            .map(|index| place(index, args.extent))
            .collect();
        let moved: Vec<DVec3> = resting
            .iter()
            .enumerate()
            .map(|(index, &centre)| drifted(index, centre))
            .collect();

        let (answers, broadphase) = verify(&resting, &moved)?;

        let mut ids: Vec<ColliderId> = Vec::with_capacity(args.bodies);
        let mut build = Vec::with_capacity(args.iterations);
        let mut refit = Vec::with_capacity(args.iterations);
        let mut query = Vec::with_capacity(args.iterations);
        let mut warmup_results = 0u64;
        let mut timed_results = 0u64;
        let mut scratch = QueryScratch::new();
        let mut out = Vec::new();

        for iteration in 0..args.warmup.saturating_add(args.iterations) {
            let mut world = PhysicsWorld::new();

            // ── build ──────────────────────────────────────────────────────
            // `ids` is cleared rather than replaced, and it was built with the
            // crowd's capacity, so the sample is the crate's work and not a
            // reallocation. Collecting the ids is inside the timer because a
            // caller keeps them too — `apps/horde` stores one per entity.
            ids.clear();
            let started = Instant::now();
            for &centre in &resting {
                ids.push(world.add_sphere(Sphere::new(centre, BODY_RADIUS)));
            }
            // The view is not wanted, only the build it forces: this is what
            // "and get the tree current" costs.
            world.overlap_queries();
            let built = nanos(started.elapsed());

            // ── refit ──────────────────────────────────────────────────────
            let started = Instant::now();
            let moved_all = set_all(&mut world, &ids, &moved);
            world.overlap_queries();
            let refitted = nanos(started.elapsed());
            if !moved_all {
                return Err(stale_id_failure());
            }

            // ── query ──────────────────────────────────────────────────────
            let mut tally = Tally::default();
            let started = Instant::now();
            {
                let queries = world.overlap_queries();
                for &centre in &moved {
                    queries.overlap_sphere_into(centre, QUERY_RADIUS, &mut scratch, &mut out);
                    let results = out.len() as u64;
                    tally.results += results;
                    tally.shape = hash_u64(tally.shape, results);
                }
            }
            let queried = nanos(started.elapsed());

            if iteration < args.warmup {
                expect_answers("a warm-up query pass", tally, answers.tally)?;
                warmup_results += tally.results;
            } else {
                expect_answers("a timed query pass", tally, answers.tally)?;
                timed_results += tally.results;
                build.push(built);
                refit.push(refitted);
                query.push(queried);
            }
        }

        build.sort_unstable();
        refit.sort_unstable();
        query.sort_unstable();
        Ok(Run {
            build,
            refit,
            query,
            answers,
            warmup_results,
            timed_results,
            broadphase,
        })
    }

    /// The untimed pass everything the timed loop does is held to.
    ///
    /// Builds the world once, moves every body once, and runs one query pass
    /// whose fold says *which* bodies answered — then checks that fold, and the
    /// tally beside it, against [`serial_answers`]. The tree's own shape is
    /// checked too: a tree holding fewer elements than the crowd would answer
    /// quickly and consistently for a crowd that is not the one placed.
    ///
    /// # Errors
    ///
    /// [`Failure`] if a body cannot be moved, if the pass does not answer what
    /// the serial scan answers, or if the tree does not hold every body.
    fn verify(resting: &[DVec3], moved: &[DVec3]) -> Result<(Answers, BroadphaseStats), Failure> {
        let expected = serial_answers(moved);

        let mut world = PhysicsWorld::new();
        let ids: Vec<ColliderId> = resting
            .iter()
            .map(|&centre| world.add_sphere(Sphere::new(centre, BODY_RADIUS)))
            .collect();
        if !set_all(&mut world, &ids, moved) {
            return Err(stale_id_failure());
        }

        let produced = full_answers(&mut world, &ids, moved)?;
        expect_answers(
            "the broadphase's query pass",
            produced.tally,
            expected.tally,
        )?;
        if produced.checksum != expected.checksum {
            return Err(Failure::new(format!(
                "the broadphase folded {} where a scan with no tree in it folds {}: the two \
                 answered the same number of results and not the same ones",
                produced.checksum, expected.checksum
            )));
        }

        let broadphase = world.broadphase_stats();
        if broadphase.elements != ids.len() {
            return Err(Failure::new(format!(
                "the tree holds {} elements where {} bodies were added: a query against it \
                 would be answering for a crowd that is not the one placed",
                broadphase.elements,
                ids.len()
            )));
        }
        Ok((produced, broadphase))
    }

    /// Every body moved, answering whether all of them resolved.
    ///
    /// Separate from the refusal because the timed loop needs the *loop* without
    /// the branch that turns it into an error — the check is what a caller does
    /// after the tick, not inside it.
    fn set_all(world: &mut PhysicsWorld, ids: &[ColliderId], moved: &[DVec3]) -> bool {
        let mut all = true;
        for (&id, &centre) in ids.iter().zip(moved) {
            all &= world.set_sphere(id, Sphere::new(centre, BODY_RADIUS));
        }
        all
    }

    /// The one message both callers of [`set_all`] refuse with.
    fn stale_id_failure() -> Failure {
        Failure::new(
            "a collider id issued by this run stopped resolving before it could be moved: part \
             of the crowd never left the position the build put it at",
        )
    }

    // -----------------------------------------------------------------------
    // Rendering
    // -----------------------------------------------------------------------

    /// The two renderings of one finished run.
    pub(super) fn report(args: &BenchArgs, run: &Run) -> Outcome {
        let (build_line, build_fields) = timing("build", &run.build);
        let (refit_line, refit_fields) = timing("refit", &run.refit);
        let (query_line, query_fields) = timing("query", &run.query);

        let queries = args.bodies as u64;
        let per_query = run.answers.tally.results as f64 / queries as f64;

        let human = format!(
            "{}: {} bodies of radius {} in a {} x {} arena, queried at radius {}, {} timed \
             iterations of all three phases, {} warm-up\n\
             {}\n\
             {build_line}\n\
             {refit_line}\n\
             {query_line}\n\
             answers: {} results over {queries} queries, {per_query:.2} per query — read the \
             query line against this, not against the body count\n\
             broadphase: {} elements, {} nodes, depth {}\n\
             warm-up: {} iterations, {} results answered and then discarded\n\
             checksum: {}",
            args.scenario.name(),
            args.bodies,
            BODY_RADIUS,
            args.extent,
            args.extent,
            QUERY_RADIUS,
            args.iterations,
            args.warmup,
            environment_line(),
            run.answers.tally.results,
            run.broadphase.elements,
            run.broadphase.nodes,
            run.broadphase.depth,
            args.warmup,
            run.warmup_results,
            run.answers.checksum,
        );

        Outcome {
            human,
            json: vec![
                ("scenario", Json::string(args.scenario.name())),
                ("environment", Json::Object(base_environment())),
                (
                    "parameters",
                    Json::Object(vec![
                        ("bodies", Json::Number(args.bodies as i64)),
                        ("extent", Json::Number(args.extent as i64)),
                        ("body_radius", Json::Float(BODY_RADIUS as f32)),
                        ("query_radius", Json::Float(QUERY_RADIUS as f32)),
                        ("iterations", Json::Number(args.iterations as i64)),
                        ("warmup", Json::Number(args.warmup as i64)),
                    ]),
                ),
                (
                    "timing",
                    Json::Object(vec![
                        ("build", Json::Object(build_fields)),
                        ("refit", Json::Object(refit_fields)),
                        ("query", Json::Object(query_fields)),
                    ]),
                ),
                (
                    "answers",
                    Json::Object(vec![
                        ("queries", Json::Number(queries as i64)),
                        ("results", Json::Number(run.answers.tally.results as i64)),
                        ("results_per_query", Json::Float(per_query as f32)),
                    ]),
                ),
                (
                    "broadphase",
                    Json::Object(vec![
                        ("elements", Json::Number(run.broadphase.elements as i64)),
                        ("nodes", Json::Number(run.broadphase.nodes as i64)),
                        ("depth", Json::Number(run.broadphase.depth as i64)),
                    ]),
                ),
                ("warmup_results", Json::Number(run.warmup_results as i64)),
                ("timed_results", Json::Number(run.timed_results as i64)),
                // A string, not a number, for [`super::report`]'s reason: a JSON
                // number is read back as a double by most consumers, which would
                // round the low bits of the one field whose whole job is to
                // compare exactly.
                ("checksum", Json::string(run.answers.checksum.to_string())),
            ],
        }
    }

    #[cfg(test)]
    mod tests {
        use super::*;
        use crcbl::core::stats::MIN_PERCENTILE_SAMPLES;

        use crate::bench::profile;

        /// Bodies every test in this module places. Small enough that the
        /// `O(bodies²)` serial scan is free in the ordinary suite, and still a
        /// tree deep enough that a query descends rather than scanning a root.
        const BODIES: usize = 200;
        /// The dense arena: a handful of neighbours per query.
        const DENSE: usize = 12;
        /// The sparse one, sixty-four times the area of [`DENSE`].
        const SPARSE: usize = 96;

        fn args(bodies: usize, extent: usize, iterations: usize, warmup: usize) -> BenchArgs {
            BenchArgs {
                scenario: crate::args::BenchScenario::Phys,
                workers: None,
                items: 1,
                chunk: 1,
                bodies,
                extent,
                iterations,
                warmup,
                json: false,
            }
        }

        /// **An empty query pass is refused, and it is refused by that name.**
        ///
        /// The failure this scenario is most likely to report as a fast number:
        /// a pass that traversed nothing and answered nothing takes almost no
        /// time, and would otherwise arrive as a plausible distribution. The
        /// guard is called directly, because a broadphase that behaves this way
        /// is not something a test can arrange from the outside.
        #[test]
        fn a_query_pass_that_answered_nothing_is_refused_rather_than_reported() {
            let expected = Tally {
                results: 1_190,
                shape: 0xabcd,
            };
            let Err(failure) = expect_answers("a timed query pass", Tally::default(), expected)
            else {
                panic!("a pass that answered nothing has to fail the run");
            };
            assert!(
                failure.message.contains("answered nothing at all"),
                "{}",
                failure.message
            );
            assert!(
                failure.message.contains("a timed query pass"),
                "{}",
                failure.message
            );
            assert!(failure.message.contains("1190"), "{}", failure.message);

            // And the same tally is accepted, so the guard is not simply
            // refusing everything.
            assert!(expect_answers("a timed query pass", expected, expected).is_ok());
        }

        /// **The right total across the wrong queries is refused too.**
        ///
        /// `shape` folds each query's result count in query order, so a pass
        /// that answered the same number of results in a different arrangement
        /// — a chunk run out of place, a query skipped and another double
        /// counted — does not match. Without this the tally would be a sum, and
        /// a sum cannot tell those apart.
        #[test]
        fn a_pass_with_the_right_total_in_the_wrong_shape_is_refused() {
            let expected = Tally {
                results: 1_190,
                shape: 0xabcd,
            };
            let reordered = Tally {
                results: expected.results,
                shape: 0x1234,
            };
            let Err(failure) = expect_answers("a timed query pass", reordered, expected) else {
                panic!("the right total across the wrong queries has to fail the run");
            };
            assert!(
                failure.message.contains("across the wrong queries"),
                "{}",
                failure.message
            );

            // And a pass one result short, which the totals catch on their own.
            let short = Tally {
                results: expected.results - 1,
                shape: expected.shape,
            };
            let Err(failure) = expect_answers("a timed query pass", short, expected) else {
                panic!("a short pass has to fail the run");
            };
            assert!(failure.message.contains("1189"), "{}", failure.message);
        }

        /// **The broadphase answers exactly what a scan with no tree answers.**
        ///
        /// Both halves of the claim: the totals match, and the identity fold
        /// matches — which is what says the two found the same *bodies* and not
        /// merely the same number of them. The fixture is asserted to be worth
        /// checking at all: a crowd where every query answered only itself would
        /// pass an identity fold trivially.
        #[test]
        fn the_broadphase_finds_the_same_bodies_a_serial_scan_finds() {
            let resting: Vec<DVec3> = (0..BODIES).map(|index| place(index, DENSE)).collect();
            let moved: Vec<DVec3> = resting
                .iter()
                .enumerate()
                .map(|(index, &centre)| drifted(index, centre))
                .collect();

            let expected = serial_answers(&moved);
            assert!(
                expected.tally.results > BODIES as u64,
                "every body answers its own query, so a fixture worth checking has to answer \
                 more than {BODIES} results; it answered {}",
                expected.tally.results
            );

            let (produced, broadphase) = verify(&resting, &moved).expect("a verified pass");
            assert_eq!(produced, expected);
            assert_eq!(broadphase.elements, BODIES);
        }

        /// **The density control changes the answers, and the timing follows.**
        ///
        /// The one fact `docs/backlog.md` says a scale number is meaningless
        /// without: the same body count in a smaller arena answers more per
        /// query. Asserted on the *counts* rather than on a duration, because a
        /// timing assertion is not a test — what is pinned is that the two runs
        /// are genuinely different measurements, and that each is still exactly
        /// what a serial scan of its own crowd answers.
        #[test]
        fn the_same_crowd_in_a_smaller_arena_answers_more_per_query() {
            let dense = measure(&args(BODIES, DENSE, 2, 0)).expect("a run");
            let sparse = measure(&args(BODIES, SPARSE, 2, 0)).expect("a run");

            assert!(
                dense.answers.tally.results > sparse.answers.tally.results,
                "{} results in a {DENSE} x {DENSE} arena is not more than {} in a \
                 {SPARSE} x {SPARSE} one",
                dense.answers.tally.results,
                sparse.answers.tally.results
            );
            // Every body still answers its own query at any density, so the
            // sparse run is a real pass and not an empty one.
            assert!(sparse.answers.tally.results >= BODIES as u64);
            assert_ne!(
                dense.answers.checksum, sparse.answers.checksum,
                "two densities that fold the same checksum are not two densities"
            );
            // The same crowd either way: the extent moves where the bodies are,
            // never how many there are.
            assert_eq!(dense.broadphase.elements, sparse.broadphase.elements);
        }

        /// **The warm-up runs, and it is excluded from everything reported.**
        ///
        /// Both halves, for the reason the `jobs` scenario gives: a warm-up that
        /// did nothing leaves the timed counters right, and a warm-up that was
        /// never excluded leaves its own count right. `warmup_results` is a sum
        /// of query answers rather than a bare iteration counter, so a warm-up
        /// pass that skipped its queries cannot reach it.
        #[test]
        fn the_warm_up_runs_and_is_excluded_from_the_reported_totals() {
            let iterations = 4;
            let warmup = 3;
            let warmed = measure(&args(BODIES, DENSE, iterations, warmup)).expect("a run");
            let per_pass = warmed.answers.tally.results;

            assert_eq!(warmed.warmup_results, warmup as u64 * per_pass);
            assert_eq!(warmed.timed_results, iterations as u64 * per_pass);
            assert_eq!(warmed.build.len(), iterations);
            assert_eq!(warmed.refit.len(), iterations);
            assert_eq!(warmed.query.len(), iterations);

            // No warm-up at all is a legal request, and then there is nothing to
            // exclude: the timed totals are the same and the warm-up's are zero.
            let cold = measure(&args(BODIES, DENSE, iterations, 0)).expect("a run");
            assert_eq!(cold.warmup_results, 0);
            assert_eq!(cold.timed_results, warmed.timed_results);
            assert_eq!(
                cold.answers, warmed.answers,
                "the warm-up changes the timings and nothing else"
            );
        }

        /// **The fixture is a pure function of its arguments.**
        ///
        /// Two runs of the same invocation place the same crowd and fold the
        /// same checksum. Nothing here reads a clock or a thread id, so this is
        /// a claim about the code rather than about the run that happened to
        /// come out the same twice — but the claim is worth failing on, because
        /// it is what makes two `--json` outputs comparable at all.
        #[test]
        fn two_runs_of_one_invocation_place_the_same_crowd() {
            let first = measure(&args(BODIES, DENSE, 2, 1)).expect("a run");
            let second = measure(&args(BODIES, DENSE, 2, 1)).expect("a run");
            assert_eq!(first.answers, second.answers);
            assert_eq!(first.warmup_results, second.warmup_results);
            assert_eq!(first.broadphase, second.broadphase);

            // And the crowd itself, so a placement that had drifted would fail
            // here rather than only through the checksum.
            for index in 0..BODIES {
                let centre = place(index, DENSE);
                assert_eq!(centre, place(index, DENSE));
                assert_ne!(
                    drifted(index, centre),
                    centre,
                    "body {index} did not move, so the refit phase refits nothing"
                );
            }
        }

        /// **All three phases reach both renderings, each as its own
        /// distribution**, with the answer size beside them and no mean
        /// anywhere.
        #[test]
        fn the_output_carries_three_distributions_and_the_answers_that_explain_them() {
            let args = args(BODIES, DENSE, MIN_PERCENTILE_SAMPLES, 1);
            let run = measure(&args).expect("a run");
            let outcome = report(&args, &run);

            let Some((_, Json::Object(timing))) = outcome
                .json
                .iter()
                .find(|(key, _)| *key == "timing")
                .cloned()
            else {
                panic!("no timing block in {:?}", outcome.json);
            };
            let phases: Vec<&str> = timing.iter().map(|(key, _)| *key).collect();
            assert_eq!(phases, ["build", "refit", "query"]);
            for (phase, fields) in &timing {
                let Json::Object(fields) = fields else {
                    panic!("`{phase}` is not an object");
                };
                for key in ["p50", "p95", "p99", "max"] {
                    assert!(
                        fields.iter().any(|(name, _)| *name == key),
                        "`{phase}` has no {key}: {fields:?}"
                    );
                }
            }

            // The answer size, which is what the query line has to be read
            // against — and which a run may not report as zero.
            let Some((_, Json::Object(answers))) = outcome
                .json
                .iter()
                .find(|(key, _)| *key == "answers")
                .cloned()
            else {
                panic!("no answers block in {:?}", outcome.json);
            };
            assert_eq!(
                answers.iter().map(|(key, _)| *key).collect::<Vec<_>>(),
                ["queries", "results", "results_per_query"]
            );
            assert_eq!(
                answers[0].1,
                Json::Number(BODIES as i64),
                "one query per body"
            );
            assert_eq!(answers[1].1, Json::Number(run.answers.tally.results as i64));

            for phase in ["build:", "refit:", "query:", "answers:", "broadphase:"] {
                assert!(outcome.human.contains(phase), "{}", outcome.human);
            }
            assert!(
                !outcome.human.contains("mean"),
                "the plan's decision is percentiles and never a mean: {}",
                outcome.human
            );
            // The environment block is mandatory here for the same reason it is
            // mandatory for `jobs`, and it says which build the numbers are from.
            assert!(outcome.human.contains(profile()), "{}", outcome.human);
            assert!(
                outcome.human.contains(std::env::consts::ARCH),
                "{}",
                outcome.human
            );
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::args::{DEFAULT_BENCH_BODIES, DEFAULT_BENCH_EXTENT};

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
            // The `phys` scenario reads neither, and `--bodies`/`--extent` are
            // refused on a `jobs` invocation; the parser's own defaults, so that
            // a `BenchArgs` built here is one the parser could have made.
            bodies: DEFAULT_BENCH_BODIES,
            extent: DEFAULT_BENCH_EXTENT,
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
        let (line, fields) = timing("per call", &spread(MIN_PERCENTILE_SAMPLES));
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
        let (line, fields) = timing("per call", &sorted);
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
