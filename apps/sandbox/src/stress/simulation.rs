//! Dense deterministic broadphase work for the `combined` stress fixture.
//!
//! The renderer half of [`Stress::Combined`] is the `lights-fit` grid; this is
//! the other half, a crowd of spheres that move, refit the broadphase and run
//! one overlap query each on every fixed tick. It competes with the frame for
//! the CPU, which is what the fixture exists to measure.
//!
//! The queries are independent of one another, so they run on a
//! [`crcbl::jobs::Pool`] through the one shared
//! [`OverlapQueries`](crcbl::phys::OverlapQueries) view, each worker thread
//! querying into scratch of its own — the shape `apps/horde`'s steering pass
//! has.

use std::cell::RefCell;
use std::time::{Duration, Instant};

use crcbl::core::stats::Window;
use crcbl::jobs::{Pool, SpawnError, default_spawner};
use crcbl::math::DVec3;
use crcbl::phys::{ColliderId, PhysicsWorld, QueryScratch, Sphere};

use super::Stress;

/// Bodies in the crowd.
const BODY_COUNT: usize = 5_000;
/// Side of the square the crowd moves in, centred on the origin.
const EXTENT: f64 = 48.0;
const BODY_RADIUS: f64 = 0.2;
/// Radius of the overlap query each body runs around itself.
const QUERY_RADIUS: f64 = 1.0;
/// The largest speed either horizontal velocity component starts at.
const SPEED: f64 = 1.5;
/// Ticks the rolling timing windows hold.
const TIMING_SAMPLES: usize = 120;
/// Bodies one pool chunk queries for.
///
/// A constant rather than a function of the worker count, so every pool runs
/// the same chunks and the counts come out the same at any worker count.
const QUERY_CHUNK: usize = 64;

thread_local! {
    /// The buffers one thread's query chunks work in, reused for the life of
    /// the thread: a `par_for` closure cannot own mutable state, and a set per
    /// chunk would allocate on every tick. Every query clears both before it
    /// writes them, so what a chunk finds here never changes an answer.
    static QUERY_SCRATCH: RefCell<(QueryScratch, Vec<ColliderId>)> =
        RefCell::new((QueryScratch::new(), Vec::new()));
}

/// Recent simulation-tick latency, as nearest-rank p50 and p95 pairs.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct SimulationTimings {
    /// Ticks currently in the rolling window.
    pub samples: usize,
    /// Worker threads the query pool has, not counting the ticking thread.
    pub workers: usize,
    /// Query chunks the ticking thread ran and chunks a worker ran, over the
    /// whole run.
    pub query_chunks: (u64, u64),
    /// The complete tick, in nanoseconds.
    pub complete_ns: (u64, u64),
    /// Moving the crowd and refitting the broadphase, in nanoseconds.
    pub refit_ns: (u64, u64),
    /// The overlap queries, in nanoseconds.
    pub query_ns: (u64, u64),
}

/// The rolling windows [`SimulationTimings`] is read from.
#[derive(Debug, Default)]
struct Profiler {
    complete: Window<TIMING_SAMPLES>,
    refit: Window<TIMING_SAMPLES>,
    query: Window<TIMING_SAMPLES>,
}

/// The simulation half of [`Stress::Combined`].
#[derive(Debug)]
pub struct Simulation {
    world: PhysicsWorld,
    ids: Vec<ColliderId>,
    positions: Vec<DVec3>,
    velocities: Vec<DVec3>,
    pool: Pool,
    /// Overlaps each body's query found in the last step, itself included.
    neighbour_counts: Vec<usize>,
    /// Overlaps the last step's queries found, every body counting itself.
    last_results: usize,
    /// Present only when the run reads the real clock: a deterministic run
    /// never reads it, here or anywhere else.
    profiler: Option<Profiler>,
}

impl Simulation {
    /// The crowd `stress` runs, or `None` for a render-only fixture. `measure`
    /// times every step, for [`timings`](Self::timings).
    ///
    /// The queries run on a pool sized to the machine.
    ///
    /// # Errors
    ///
    /// [`SpawnError`] if the platform refused a worker thread.
    pub fn for_stress(stress: Stress, measure: bool) -> Result<Option<Self>, SpawnError> {
        if !stress.simulates() {
            return Ok(None);
        }
        let pool = Pool::new(default_spawner().as_ref())?;
        Ok(Some(Self::with_pool(BODY_COUNT, measure, pool)))
    }

    /// [`with_pool`](Self::with_pool) on a pool with no workers, which runs
    /// every chunk on the calling thread in order.
    #[cfg(test)]
    fn with_count(count: usize, measure: bool) -> Self {
        let pool = Pool::new(&crcbl::jobs::Inline).expect("an inline pool starts no thread");
        Self::with_pool(count, measure, pool)
    }

    /// A crowd of `count` bodies on a square grid, each with its own fixed
    /// velocity, and the broadphase already built over them; `pool` runs the
    /// queries.
    #[allow(clippy::cast_precision_loss)]
    fn with_pool(count: usize, measure: bool, pool: Pool) -> Self {
        assert!(count > 0, "a stress crowd must contain a body");
        let columns = (count as f64).sqrt().ceil() as usize;
        let mut world = PhysicsWorld::new();
        let mut ids = Vec::with_capacity(count);
        let mut positions = Vec::with_capacity(count);
        let mut velocities = Vec::with_capacity(count);
        for index in 0..count {
            let column = index % columns;
            let row = index / columns;
            let x = (column as f64 + 0.5) / columns as f64 * EXTENT - EXTENT * 0.5;
            let z = (row as f64 + 0.5) / columns as f64 * EXTENT - EXTENT * 0.5;
            let position = DVec3::new(x, 0.0, z);
            // Strides coprime to their moduli scatter the headings without a
            // random source.
            let vx = ((index * 17 % 101) as f64 / 50.0 - 1.0) * SPEED;
            let vz = ((index * 43 % 103) as f64 / 51.0 - 1.0) * SPEED;
            positions.push(position);
            velocities.push(DVec3::new(vx, 0.0, vz));
            ids.push(world.add_sphere(Sphere::new(position, BODY_RADIUS)));
        }
        // Built here, so the first step refits a tree rather than building one.
        world.overlap_queries();
        Self {
            world,
            ids,
            positions,
            velocities,
            pool,
            neighbour_counts: vec![0; count],
            last_results: 0,
            profiler: measure.then(Profiler::default),
        }
    }

    /// Moves, refits and queries the whole crowd once, `dt` seconds on.
    ///
    /// # Panics
    ///
    /// If a body's collider has gone, which nothing here removes.
    pub fn step(&mut self, dt: f64) {
        let started = self.profiler.as_ref().map(|_| Instant::now());
        let boundary = EXTENT * 0.5;
        for ((position, velocity), &id) in self
            .positions
            .iter_mut()
            .zip(&mut self.velocities)
            .zip(&self.ids)
        {
            *position += *velocity * dt;
            for (coordinate, speed) in [
                (&mut position.x, &mut velocity.x),
                (&mut position.z, &mut velocity.z),
            ] {
                if coordinate.abs() > boundary {
                    *coordinate = coordinate.clamp(-boundary, boundary);
                    *speed = -*speed;
                }
            }
            let moved = self
                .world
                .set_sphere(id, Sphere::new(*position, BODY_RADIUS));
            assert!(moved, "the fixed crowd keeps every collider live");
        }
        let refitted = started.map(|_| Instant::now());

        // The exclusive borrow is spent once, on building the tree, and buys a
        // view every chunk can query at the same time.
        let queries = self.world.overlap_queries();
        let positions = &self.positions;
        self.pool
            .par_for(&mut self.neighbour_counts, QUERY_CHUNK, |start, counts| {
                QUERY_SCRATCH.with_borrow_mut(|(scratch, neighbours)| {
                    for (offset, count) in counts.iter_mut().enumerate() {
                        queries.overlap_sphere_ids_into(
                            positions[start + offset],
                            QUERY_RADIUS,
                            scratch,
                            neighbours,
                        );
                        *count = neighbours.len();
                    }
                });
            });
        let results = self
            .neighbour_counts
            .iter()
            .copied()
            .fold(0usize, usize::saturating_add);
        // Kept, and kept opaque, so the queries are work the optimiser must do.
        self.last_results = std::hint::black_box(results);

        if let (Some(started), Some(refitted), Some(profiler)) =
            (started, refitted, self.profiler.as_mut())
        {
            let finished = Instant::now();
            let nanos = |duration: Duration| u64::try_from(duration.as_nanos()).unwrap_or(u64::MAX);
            profiler.complete.record(nanos(finished - started));
            profiler.refit.record(nanos(refitted - started));
            profiler.query.record(nanos(finished - refitted));
        }
    }

    /// Percentiles over the most recent steps, or `None` when the run is not
    /// measuring or has too few steps for a p95 that is not just the maximum.
    #[must_use]
    pub fn timings(&self) -> Option<SimulationTimings> {
        let profiler = self.profiler.as_ref()?;
        let pool = self.pool.stats();
        Some(SimulationTimings {
            samples: profiler.complete.len(),
            workers: self.pool.workers(),
            query_chunks: (pool.chunks_run_by_driver, pool.chunks_run_by_workers),
            complete_ns: profiler.complete.percentiles()?,
            refit_ns: profiler.refit.percentiles()?,
            query_ns: profiler.query.percentiles()?,
        })
    }

    /// Overlaps the last step's queries found.
    #[cfg(test)]
    pub const fn last_results(&self) -> usize {
        self.last_results
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    use crcbl::core::stats::MIN_PERCENTILE_SAMPLES;
    use crcbl::phys::sphere_overlaps_sphere;

    const DT: f64 = 1.0 / 60.0;

    #[test]
    fn a_step_moves_refits_and_queries_every_body() {
        let mut simulation = Simulation::with_count(64, false);
        let before = simulation.positions.clone();
        let refits = simulation.world.broadphase_stats().refits;

        simulation.step(DT);

        assert!(
            simulation
                .positions
                .iter()
                .zip(&before)
                .all(|(after, before)| after != before),
            "every body must move",
        );
        assert_eq!(
            simulation.world.broadphase_stats().refits - refits,
            u64::try_from(simulation.ids.len()).expect("the test crowd fits u64"),
            "every body must reach the built broadphase",
        );
        assert!(
            simulation.last_results >= simulation.ids.len(),
            "every sphere query must at least find its own body",
        );
        assert!(
            simulation.profiler.is_none(),
            "a run that did not ask for timings must not read the clock",
        );
    }

    #[test]
    fn timings_split_complete_ticks_into_their_phases() {
        let mut simulation = Simulation::with_count(64, true);
        for _ in 0..MIN_PERCENTILE_SAMPLES - 1 {
            simulation.step(DT);
        }
        assert_eq!(simulation.timings(), None, "too few ticks for a p95");

        simulation.step(DT);

        let timings = simulation.timings().expect("enough ticks for a p95");
        assert_eq!(timings.samples, MIN_PERCENTILE_SAMPLES);
        // One chunk a step, on an inline pool, so every one on this thread.
        assert_eq!(timings.workers, 0);
        assert_eq!(
            timings.query_chunks,
            (
                u64::try_from(MIN_PERCENTILE_SAMPLES).expect("the step count fits u64"),
                0
            ),
        );
        for (p50_ns, p95_ns) in [timings.complete_ns, timings.refit_ns, timings.query_ns] {
            assert!(p50_ns > 0);
            assert!(p95_ns >= p50_ns);
        }
        assert!(timings.complete_ns.0 >= timings.refit_ns.0);
        assert!(timings.complete_ns.0 >= timings.query_ns.0);
    }

    #[test]
    fn the_crowd_bounces_off_the_edges_of_its_square() {
        let mut simulation = Simulation::with_count(64, false);
        // Long enough for the fastest body to cross the square twice.
        let steps = (2.0 * EXTENT / SPEED / DT).ceil() as usize;
        let boundary = EXTENT * 0.5;
        for step in 0..steps {
            simulation.step(DT);
            assert!(
                simulation
                    .positions
                    .iter()
                    .all(|position| position.x.abs() <= boundary && position.z.abs() <= boundary),
                "a body left the square on step {step}",
            );
        }
        let pinned = simulation
            .positions
            .iter()
            .filter(|position| position.x.abs() == boundary || position.z.abs() == boundary)
            .count();
        assert!(
            pinned * 4 < simulation.positions.len(),
            "{pinned} bodies sit on an edge: the crowd is clamped, not bouncing",
        );
    }

    /// **A threaded pool finds exactly what a scan of every body finds**, and
    /// it really is threaded: steps repeat until a worker has run a chunk,
    /// checking every one, so a pool that ran everything on the ticking thread
    /// goes red rather than passing as a serial run.
    #[test]
    #[cfg(not(target_arch = "wasm32"))]
    fn threaded_queries_count_exactly_what_a_scan_finds() {
        let pool = Pool::with_workers(default_spawner().as_ref(), 3).expect("native query workers");
        let mut simulation = Simulation::with_pool(BODY_COUNT, false, pool);
        assert!(
            BODY_COUNT.div_ceil(QUERY_CHUNK) > 1,
            "one chunk runs on the calling thread by construction",
        );
        let deadline = Instant::now() + Duration::from_secs(10);
        loop {
            simulation.step(DT);

            let expected: Vec<_> = simulation
                .positions
                .iter()
                .map(|centre| {
                    let query = Sphere::new(*centre, QUERY_RADIUS);
                    simulation
                        .positions
                        .iter()
                        .filter(|other| {
                            sphere_overlaps_sphere(&query, &Sphere::new(**other, BODY_RADIUS))
                        })
                        .count()
                })
                .collect();
            assert!(
                expected.iter().any(|&count| count > 1),
                "every body is alone, so a query that found only itself would match",
            );
            assert_eq!(simulation.neighbour_counts, expected);
            assert_eq!(simulation.last_results, expected.iter().sum::<usize>());

            if simulation.pool.stats().chunks_run_by_workers > 0 {
                break;
            }
            assert!(
                Instant::now() < deadline,
                "no worker ran a query chunk, so this compared a serial run",
            );
        }
    }
}
