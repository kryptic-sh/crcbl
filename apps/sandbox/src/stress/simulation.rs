//! Dense deterministic broadphase work for the `combined` stress fixture.
//!
//! The renderer half of [`Stress::Combined`] is the `lights-fit` grid; this is
//! the other half, a crowd of spheres that move, refit the broadphase and run
//! one overlap query each on every fixed tick. It competes with the frame for
//! the CPU, which is what the fixture exists to measure.

use std::time::{Duration, Instant};

use crcbl::core::stats::Window;
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

/// Recent simulation-tick latency, as nearest-rank p50 and p95 pairs.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct SimulationTimings {
    /// Ticks currently in the rolling window.
    pub samples: usize,
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
    scratch: QueryScratch,
    neighbours: Vec<ColliderId>,
    /// Overlaps the last step's queries found, every body counting itself.
    last_results: usize,
    /// Present only when the run reads the real clock: a deterministic run
    /// never reads it, here or anywhere else.
    profiler: Option<Profiler>,
}

impl Simulation {
    /// The crowd `stress` runs, or `None` for a render-only fixture. `measure`
    /// times every step, for [`timings`](Self::timings).
    #[must_use]
    pub fn for_stress(stress: Stress, measure: bool) -> Option<Self> {
        stress
            .simulates()
            .then(|| Self::with_count(BODY_COUNT, measure))
    }

    /// A crowd of `count` bodies on a square grid, each with its own fixed
    /// velocity, and the broadphase already built over them.
    #[allow(clippy::cast_precision_loss)]
    fn with_count(count: usize, measure: bool) -> Self {
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
            scratch: QueryScratch::new(),
            neighbours: Vec::new(),
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

        let queries = self.world.overlap_queries();
        let mut results = 0usize;
        for &position in &self.positions {
            queries.overlap_sphere_ids_into(
                position,
                QUERY_RADIUS,
                &mut self.scratch,
                &mut self.neighbours,
            );
            results = results.saturating_add(self.neighbours.len());
        }
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
        Some(SimulationTimings {
            samples: profiler.complete.len(),
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
}
