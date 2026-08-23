//! `crcbl-phys`'s broadphase at scale: build, refit and query, timed apart.
//!
//! # Why this scenario exists, and what it refuses to report
//!
//! `docs/plan/ROADMAP.md`'s P8 proposes adopting the physics broadphase onto
//! `crcbl-jobs`, and `docs/backlog.md` records that nothing has ever timed it:
//! the allocations `overlap_sphere_into` removed "are justified by the count,
//! not by a measurement, and anybody quoting it as a speed-up is quoting
//! something nobody ran". This is the harness that stops that being true, and
//! the numbers it produces are what P8 gets to design against.
//!
//! The same backlog entry fixes the shape of the fixture: **"a broadphase query
//! costs what its *answer* costs, so the tick's cost tracks local density rather
//! than entity count"** — the same ten thousand `apps/horde` enemies cost
//! 14.66 ms a tick spread over the arena and 84.09 ms converged on the player.
//! So a run that printed one duration for "N bodies" would be printing the
//! misleading half of a two-number fact. Two things follow, and they are the
//! design of this module:
//!
//! 1. **Density is a parameter**, spelled `--extent`: the side of the square
//!    arena the crowd is placed in, in whole world units. Not "neighbours per
//!    query", which is what a reader wants to know — because an extent is what
//!    the fixture *uses*, with no arithmetic between the flag and the placement,
//!    while a neighbour target would be a request the placement could silently
//!    miss while still printing the number it was asked for. The neighbour count
//!    is the run's *output*, measured, and it is what makes the two runs at one
//!    body count comparable.
//! 2. **Every run carries its answer size.** `answers` reports the results the
//!    query phase returned and the mean per query, beside the query timing, so a
//!    reader cannot take a duration away without the density that produced it.
//!
//! # Three phases, three distributions
//!
//! One iteration is one build, one refit and one query pass, timed separately
//! and reported as three distributions rather than one total:
//!
//! * **build** — `bodies` [`PhysicsWorld::add_sphere`] calls into a fresh world,
//!   then the [`PhysicsWorld::overlap_queries`] that makes the tree current.
//!   Adds before the first query accumulate, so this is one bulk `Bvh::build`.
//! * **refit** — every body moved one tick's worth with
//!   [`PhysicsWorld::set_sphere`], then the tree made current again. `set_sphere`
//!   refits in place when it can and drops the tree when it cannot, in which
//!   case the rebuild lands in this sample — which is the honest answer, because
//!   it is what a caller pays.
//! * **query** — `bodies` sphere overlaps, one per body, at its own position:
//!   the shape `apps/horde`'s separation pass runs, through
//!   `OverlapQueries::overlap_sphere_into` with a [`QueryScratch`], which is
//!   the form P8's parallel adoption will use.
//!
//! **There is no threading here and that is deliberate**: a pool in the middle
//! of the pass would measure the pool. `jobs` is the scenario that measures the
//! pool, and P8 is what puts the two together.
//!
//! # The answers are checked, and the check is in two halves
//!
//! [`serial_answers`] runs the same pass with no tree in it at all — every body
//! against every body, `O(bodies²)`, through the same
//! [`sphere_overlaps_sphere`] predicate the broadphase's exact test calls. That
//! is the answer everything else is held to.
//!
//! [`full_answers`] then runs one untimed pass through the broadphase and folds
//! **which** bodies answered which query, and the run fails unless it reproduces
//! the serial fold exactly. That fold needs a `ColliderId` → body-index map and
//! therefore a hash lookup per result, which is why it is untimed and run once:
//! a `HashMap` inside the query phase would be reported as part of the
//! broadphase's cost, in the one number this scenario exists to give away.
//!
//! Every timed pass — and every warm-up pass — is then held to that pass's
//! [`Tally`]: the total results, and a fold of each query's result count in
//! query order. So a pass that answered nothing, answered short, or ran its
//! queries out of order fails the run instead of reporting a fast number.
//!
//! # Deterministic, and portable rather than merely reproducible
//!
//! Placement and movement come from [`hash_unit`], whose value is the top 53
//! bits of [`hash_u64`] over a power-of-two divisor and therefore exact. Every
//! operation after it is a multiply, an add or a compare, and
//! [`sphere_overlaps_sphere`] is the same — **nothing here calls a
//! transcendental**, whose results differ between glibc, Apple's libm and MSVC.
//! So two runs with the same arguments place the same crowd and fold the same
//! checksum on every target, not just twice on this one. A heading angle would
//! have been the natural way to move a body and is not used for exactly that
//! reason; the drift is two independent values in `[-1, 1]` instead, which is
//! not uniform over a circle and does not need to be.

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
    answers: Tally,
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

/// How much one query pass answered, in what shape, and from which bodies.
///
/// Cheap enough to compute on every pass, warm-up included: one add and one
/// mix per query, and one array index and one add per result. `shape` folds
/// each query's result count **in query order**, so a pass that answered the
/// right total across the wrong queries does not match; `checksum` folds the
/// bodies themselves, so a pass that answered the right counts from the wrong
/// bodies does not either.
///
/// The per-query part of `checksum` is an order-independent sum, because the
/// order a query returns its neighbours in is the BVH's traversal order and
/// `docs/backlog.md` records that `apps/horde` chose to live with it — a
/// checksum that depended on it would be pinning the tree's shape rather
/// than its answer. Across queries the fold *is* ordered, because the query
/// order is the body order and nothing about the tree may change it.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
struct Tally {
    /// Results returned, summed over every query in the pass.
    results: u64,
    /// Each query's result count, folded in query order.
    shape: u64,
    /// Which bodies answered. See the type's own docs.
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
fn serial_answers(centres: &[DVec3]) -> Tally {
    let mut tally = Tally::default();
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
        tally.results += results;
        tally.shape = hash_u64(tally.shape, results);
        tally.checksum = hash_u64(tally.checksum, neighbours);
    }
    tally
}

/// What each collider slot contributes to a query that answers with it.
///
/// Indexed by [`ColliderId::index`], which is why the timed loop can fold
/// *which* bodies answered without a `HashMap` in it: a result becomes an
/// array subscript. Sized from the largest slot the run was handed rather
/// than from the body count, because the two are equal only while nothing
/// has been removed and that is `PhysicsWorld`'s contract, not this
/// scenario's to assume.
///
/// Two live colliders never share a slot — `crcbl-phys`'
/// `collider_slots_are_stable_and_stop_being_dense_after_a_removal` is what
/// holds that — so nothing here re-checks it.
fn marks_by_slot(ids: &[ColliderId]) -> Vec<u64> {
    let slots = || ids.iter().map(|id| id.index() as usize);
    let mut marks = vec![0u64; slots().max().map_or(0, |max| max + 1)];
    for (index, slot) in slots().enumerate() {
        marks[slot] = mark(index as u64);
    }
    marks
}

/// One untimed pass through the broadphase, folding *which* bodies answered.
///
/// The same fold the timed loop runs, kept separate because this one also
/// proves every result names a collider the run added — a check that needs
/// the slot table's bounds and would be a branch per result in the timed
/// path. Once this pass has passed, the timed loop's subscript is in range
/// by construction.
///
/// # Errors
///
/// [`Failure`] if a query answers with a collider this run never added,
/// which would mean the tree no longer describes the world it was built
/// from — a ghost, and not something to fold into a checksum.
fn full_answers(
    world: &mut PhysicsWorld,
    marks: &[u64],
    centres: &[DVec3],
) -> Result<Tally, Failure> {
    let mut scratch = QueryScratch::new();
    let mut out = Vec::new();
    let mut tally = Tally::default();

    let queries = world.overlap_queries();
    for &centre in centres {
        queries.overlap_sphere_into(centre, QUERY_RADIUS, &mut scratch, &mut out);
        let mut neighbours = 0u64;
        for id in &out {
            let Some(&mark) = marks.get(id.index() as usize) else {
                return Err(Failure::new(
                    "a query answered with a collider this run never added: the tree no \
                     longer describes the world it was built from",
                ));
            };
            neighbours = neighbours.wrapping_add(mark);
        }
        tally.results += out.len() as u64;
        tally.shape = hash_u64(tally.shape, out.len() as u64);
        tally.checksum = hash_u64(tally.checksum, neighbours);
    }
    Ok(tally)
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
    if produced.shape != expected.shape {
        return Err(Failure::new(format!(
            "{phase} answered the right {} results across the wrong queries: its per-query \
             counts fold to {} where the scan's fold to {}",
            produced.results, produced.shape, expected.shape
        )));
    }
    if produced.checksum == expected.checksum {
        return Ok(());
    }
    Err(Failure::new(format!(
        "{phase} answered the right {} results in the right shape and from the wrong \
         bodies: its identity fold is {} where the scan's is {}",
        produced.results, produced.checksum, expected.checksum
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

    let answers = verify(&resting, &moved)?;

    let mut ids: Vec<ColliderId> = Vec::with_capacity(args.bodies);
    let mut build = Vec::with_capacity(args.iterations);
    let mut refit = Vec::with_capacity(args.iterations);
    let mut query = Vec::with_capacity(args.iterations);
    let mut warmup_results = 0u64;
    let mut timed_results = 0u64;
    let mut scratch = QueryScratch::new();
    let mut out = Vec::new();
    let mut broadphase: Option<BroadphaseStats> = None;

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
        // The identity fold is inside the timer: one array index and one add
        // per result, which is what `ColliderId::index` being public buys —
        // the `HashMap` this scenario refused to put here is gone.
        //
        // It is not free, and pretending otherwise would be the kind of
        // claim this file exists to avoid. Measured 2026-08-23 on an x86-64
        // release build at the default arena: the query phase's p50 rose
        // from ~504 to ~515 microseconds, about two percent, with the two
        // sets of runs not overlapping. That cost is paid identically by
        // every run, so it does not disturb a comparison between runs, and
        // it buys the one failure the totals cannot see: the right number of
        // results, in the right per-query shape, from the wrong bodies.
        let marks = marks_by_slot(&ids);
        let mut tally = Tally::default();
        let started = Instant::now();
        {
            let queries = world.overlap_queries();
            for &centre in &moved {
                queries.overlap_sphere_into(centre, QUERY_RADIUS, &mut scratch, &mut out);
                let mut neighbours = 0u64;
                for id in &out {
                    neighbours = neighbours.wrapping_add(marks[id.index() as usize]);
                }
                let results = out.len() as u64;
                tally.results += results;
                tally.shape = hash_u64(tally.shape, results);
                tally.checksum = hash_u64(tally.checksum, neighbours);
            }
        }
        let queried = nanos(started.elapsed());

        if iteration < args.warmup {
            expect_answers("a warm-up query pass", tally, answers)?;
            warmup_results += tally.results;
        } else {
            expect_answers("a timed query pass", tally, answers)?;
            timed_results += tally.results;
            build.push(built);
            refit.push(refitted);
            query.push(queried);
            // Read after the query pass, so the tree is current and this does
            // not force a rebuild of its own. Overwritten each iteration: the
            // reading wanted is one iteration's, and every iteration builds
            // the same world from the same crowd. Reading the *verify* pass's
            // world instead would report a tree the timings never touched.
            broadphase = Some(world.broadphase_stats());
        }
    }

    let Some(broadphase) = broadphase else {
        return Err(Failure::new(
            "no timed iteration ran, so there is no tree to report: `--iterations` must be \
             at least one",
        ));
    };

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
fn verify(resting: &[DVec3], moved: &[DVec3]) -> Result<Tally, Failure> {
    let expected = serial_answers(moved);

    let mut world = PhysicsWorld::new();
    let ids: Vec<ColliderId> = resting
        .iter()
        .map(|&centre| world.add_sphere(Sphere::new(centre, BODY_RADIUS)))
        .collect();
    if !set_all(&mut world, &ids, moved) {
        return Err(stale_id_failure());
    }

    let produced = full_answers(&mut world, &marks_by_slot(&ids), moved)?;
    expect_answers("the broadphase's query pass", produced, expected)?;

    let broadphase = world.broadphase_stats();
    if broadphase.elements != ids.len() {
        return Err(Failure::new(format!(
            "the tree holds {} elements where {} bodies were added: a query against it \
             would be answering for a crowd that is not the one placed",
            broadphase.elements,
            ids.len()
        )));
    }
    Ok(produced)
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
    let per_query = run.answers.results as f64 / queries as f64;

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
         one iteration: {} refits, {} updates left for a rebuild, {} tree builds — the \
         build phase is one of them, so a refit phase that rebuilt shows a second\n\
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
        run.answers.results,
        run.broadphase.elements,
        run.broadphase.nodes,
        run.broadphase.depth,
        run.broadphase.refits,
        run.broadphase.updates_without_refit,
        run.broadphase.rebuilds,
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
                    ("results", Json::Number(run.answers.results as i64)),
                    ("results_per_query", Json::Float(per_query as f32)),
                ]),
            ),
            (
                "broadphase",
                Json::Object(vec![
                    ("elements", Json::Number(run.broadphase.elements as i64)),
                    ("nodes", Json::Number(run.broadphase.nodes as i64)),
                    ("depth", Json::Number(run.broadphase.depth as i64)),
                    ("refits", Json::Number(run.broadphase.refits as i64)),
                    (
                        "updates_without_refit",
                        Json::Number(run.broadphase.updates_without_refit as i64),
                    ),
                    ("rebuilds", Json::Number(run.broadphase.rebuilds as i64)),
                ]),
            ),
            ("warmup_results", Json::Number(run.warmup_results as i64)),
            ("timed_results", Json::Number(run.timed_results as i64)),
            // A string, not a number, for [`super::jobs::report`]'s reason: a JSON
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
            checksum: 0x5eed,
        };
        let Err(failure) = expect_answers("a timed query pass", Tally::default(), expected) else {
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
            checksum: 0x5eed,
        };
        let reordered = Tally {
            shape: 0x1234,
            ..expected
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
            ..expected
        };
        let Err(failure) = expect_answers("a timed query pass", short, expected) else {
            panic!("a short pass has to fail the run");
        };
        assert!(failure.message.contains("1189"), "{}", failure.message);

        // And the right counts in the right shape from the wrong bodies,
        // which only the identity fold can see. This is the case the timed
        // loop could not catch until it folded identity itself.
        let impostors = Tally {
            checksum: expected.checksum ^ 1,
            ..expected
        };
        let Err(failure) = expect_answers("a timed query pass", impostors, expected) else {
            panic!("the right shape from the wrong bodies has to fail the run");
        };
        assert!(
            failure.message.contains("from the wrong \nbodies")
                || failure.message.contains("from the wrong bodies"),
            "{}",
            failure.message
        );
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
            expected.results > BODIES as u64,
            "every body answers its own query, so a fixture worth checking has to answer \
             more than {BODIES} results; it answered {}",
            expected.results
        );

        let produced = verify(&resting, &moved).expect("a verified pass");
        assert_eq!(produced, expected);
    }

    /// **One timed iteration refits every body and rebuilds once, and the run
    /// says so.**
    ///
    /// The reported tree is the last *timed* iteration's, not the
    /// verification pass's, so these counters describe the world the three
    /// timings came from. What they say is the answer to the question the
    /// refit phase's number cannot answer on its own — a refit phase that had
    /// been rebuilding would show it here as a second rebuild rather than as
    /// a slower microsecond figure that looks like every other slow figure.
    ///
    /// One rebuild, because the build phase's `overlap_queries` is what
    /// builds the tree; `bodies` refits, because `set_sphere` on a built tree
    /// always refits; nothing left over, because an update only skips the
    /// refit when there is no tree to refit.
    #[test]
    fn the_reported_tree_is_the_timed_iterations_and_says_it_refit() {
        let run = measure(&args(BODIES, DENSE, 2, 1)).expect("a run");
        assert_eq!(
            run.broadphase.elements, BODIES,
            "the reported tree must hold the whole crowd"
        );
        assert_eq!(
            run.broadphase.refits, BODIES as u64,
            "one iteration moves every body once, and a built tree refits every time"
        );
        assert_eq!(
            run.broadphase.updates_without_refit, 0,
            "every update had a tree to refit"
        );
        assert_eq!(
            run.broadphase.rebuilds, 1,
            "only the build phase builds; reading the stats after the query pass must not \
             force a second one"
        );
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
            dense.answers.results > sparse.answers.results,
            "{} results in a {DENSE} x {DENSE} arena is not more than {} in a \
             {SPARSE} x {SPARSE} one",
            dense.answers.results,
            sparse.answers.results
        );
        // Every body still answers its own query at any density, so the
        // sparse run is a real pass and not an empty one.
        assert!(sparse.answers.results >= BODIES as u64);
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
        let per_pass = warmed.answers.results;

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
        assert_eq!(answers[1].1, Json::Number(run.answers.results as i64));

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
