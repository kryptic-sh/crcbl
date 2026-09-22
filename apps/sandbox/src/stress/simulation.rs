//! Dense deterministic broadphase work for the combined stress scene.

use std::time::Instant;

use crcbl::core::stats::Window;
#[cfg(test)]
use crcbl::jobs::Inline;
use crcbl::jobs::{Pool, SpawnError, default_spawner};
use crcbl::math::DVec3;
#[cfg(test)]
use crcbl::phys::sphere_overlaps_sphere;
use crcbl::phys::{ColliderId, PhysicsWorld, QueryScratch, Sphere};

use super::Scene;

const BODY_COUNT: usize = 5_000;
const EXTENT: f64 = 48.0;
const BODY_RADIUS: f64 = 0.2;
const QUERY_RADIUS: f64 = 1.0;
const SPEED: f64 = 1.5;
const TIMING_SAMPLES: usize = 120;
const QUERY_CHUNK: usize = 64;

thread_local! {
    static QUERY_SCRATCH: std::cell::RefCell<(QueryScratch, Vec<ColliderId>)> =
        std::cell::RefCell::new((QueryScratch::new(), Vec::new()));
}

/// Recent complete simulation-tick latency.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct SimulationTimings {
    /// Samples currently in the rolling window.
    pub samples: usize,
    /// Worker threads available to the overlap-query pass.
    pub workers: usize,
    /// Query chunks completed by the calling thread and worker threads.
    pub query_chunks: (u64, u64),
    /// Complete-tick nearest-rank median and p95, in nanoseconds.
    pub complete_ns: (u64, u64),
    /// Movement and broadphase-refit median and p95, in nanoseconds.
    pub refit_ns: (u64, u64),
    /// Overlap-query median and p95, in nanoseconds.
    pub query_ns: (u64, u64),
}

#[derive(Debug)]
struct SimulationProfiler {
    complete: Window<TIMING_SAMPLES>,
    refit: Window<TIMING_SAMPLES>,
    query: Window<TIMING_SAMPLES>,
}

impl SimulationProfiler {
    fn new() -> Self {
        Self {
            complete: Window::new(),
            refit: Window::new(),
            query: Window::new(),
        }
    }
}

/// The deterministic simulation half of [`Scene::Combined`].
#[derive(Debug)]
pub struct Simulation {
    world: PhysicsWorld,
    ids: Vec<ColliderId>,
    positions: Vec<DVec3>,
    velocities: Vec<DVec3>,
    query_pool: Pool,
    query_results: Vec<usize>,
    last_results: usize,
    profiler: Option<SimulationProfiler>,
}

impl Simulation {
    /// Build the fixed dense crowd, or no simulation for a render-only scene.
    pub fn for_scene(scene: Scene, measure: bool) -> Result<Option<Self>, SpawnError> {
        if !scene.simulates() {
            return Ok(None);
        }
        let spawner = default_spawner();
        Pool::new(spawner.as_ref()).map(|pool| Some(Self::with_pool(BODY_COUNT, measure, pool)))
    }

    #[cfg(test)]
    fn with_count(count: usize, measure: bool) -> Self {
        let query_pool = Pool::new(&Inline).expect("the inline pool spawns no workers");
        Self::with_pool(count, measure, query_pool)
    }

    #[allow(clippy::cast_precision_loss)]
    fn with_pool(count: usize, measure: bool, query_pool: Pool) -> Self {
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
            let vx = ((index * 17 % 101) as f64 / 50.0 - 1.0) * SPEED;
            let vz = ((index * 43 % 103) as f64 / 51.0 - 1.0) * SPEED;
            positions.push(position);
            velocities.push(DVec3::new(vx, 0.0, vz));
            ids.push(world.add_sphere(Sphere::new(position, BODY_RADIUS)));
        }
        world.overlap_queries();
        Self {
            world,
            ids,
            positions,
            velocities,
            query_pool,
            query_results: vec![0; count],
            last_results: 0,
            profiler: measure.then(SimulationProfiler::new),
        }
    }

    /// Move, refit and query the whole crowd once.
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
        let refit_finished = started.map(|_| Instant::now());

        let queries = self.world.overlap_queries();
        let positions = &self.positions;
        self.query_pool
            .par_for(&mut self.query_results, QUERY_CHUNK, |start, counts| {
                QUERY_SCRATCH.with_borrow_mut(|(scratch, out)| {
                    for (offset, count) in counts.iter_mut().enumerate() {
                        queries.overlap_sphere_into(
                            positions[start + offset],
                            QUERY_RADIUS,
                            scratch,
                            out,
                        );
                        *count = out.len();
                    }
                });
            });
        let results = self
            .query_results
            .iter()
            .copied()
            .fold(0usize, usize::saturating_add);
        self.last_results = std::hint::black_box(results);
        if let (Some(started), Some(refit_finished), Some(profiler)) =
            (started, refit_finished, self.profiler.as_mut())
        {
            let finished = Instant::now();
            let nanos = |duration: std::time::Duration| {
                u64::try_from(duration.as_nanos()).unwrap_or(u64::MAX)
            };
            profiler.complete.record(nanos(finished - started));
            profiler.refit.record(nanos(refit_finished - started));
            profiler.query.record(nanos(finished - refit_finished));
        }
    }

    /// Percentiles over the most recent complete ticks, once the sample count
    /// is large enough that p95 is not merely the maximum under another name.
    #[must_use]
    pub fn timings(&self) -> Option<SimulationTimings> {
        let profiler = self.profiler.as_ref()?;
        let pool = self.query_pool.stats();
        Some(SimulationTimings {
            samples: profiler.complete.len(),
            workers: self.query_pool.workers(),
            query_chunks: (pool.chunks_run_by_driver, pool.chunks_run_by_workers),
            complete_ns: profiler.complete.percentiles()?,
            refit_ns: profiler.refit.percentiles()?,
            query_ns: profiler.query.percentiles()?,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_combined_fixture_moves_refits_and_queries_every_body() {
        let mut simulation = Simulation::with_count(64, false);
        let before = simulation.positions.clone();

        simulation.step(1.0 / 60.0);

        assert_ne!(simulation.positions, before, "the crowd must move");
        assert_eq!(
            simulation.world.broadphase_stats().refits,
            u64::try_from(simulation.ids.len()).expect("the test crowd fits u64"),
            "every body must reach the built broadphase",
        );
        assert!(
            simulation.last_results >= simulation.ids.len(),
            "every sphere query must at least find its own body",
        );
        assert_eq!(simulation.timings(), None, "timing is disabled");
    }

    #[test]
    fn timings_cover_complete_simulation_ticks() {
        let mut simulation = Simulation::with_count(64, true);
        for _ in 0..crcbl::core::stats::MIN_PERCENTILE_SAMPLES {
            simulation.step(1.0 / 60.0);
        }
        let timings = simulation.timings().expect("enough ticks for a p95");
        assert_eq!(timings.samples, crcbl::core::stats::MIN_PERCENTILE_SAMPLES);
        for (p50_ns, p95_ns) in [timings.complete_ns, timings.refit_ns, timings.query_ns] {
            assert!(p50_ns > 0);
            assert!(p95_ns >= p50_ns);
        }
        assert!(timings.complete_ns.0 >= timings.refit_ns.0);
        assert!(timings.complete_ns.0 >= timings.query_ns.0);
    }

    #[test]
    #[cfg(not(target_arch = "wasm32"))]
    fn parallel_queries_match_an_independent_scan_exactly() {
        let spawner = default_spawner();
        let pool = Pool::with_workers(spawner.as_ref(), 3).expect("native query workers");
        let mut parallel = Simulation::with_pool(BODY_COUNT, false, pool);

        parallel.step(1.0 / 60.0);

        let expected: Vec<_> = parallel
            .positions
            .iter()
            .map(|centre| {
                let query = Sphere::new(*centre, QUERY_RADIUS);
                parallel
                    .positions
                    .iter()
                    .filter(|candidate| {
                        sphere_overlaps_sphere(&query, &Sphere::new(**candidate, BODY_RADIUS))
                    })
                    .count()
            })
            .collect();
        assert_eq!(parallel.query_results, expected);
        assert_eq!(parallel.last_results, expected.iter().sum::<usize>());
        assert!(
            parallel.query_pool.stats().chunks_run_by_workers > 0,
            "the threaded comparison ran no query chunk on a worker",
        );
    }
}
