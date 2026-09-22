//! Dense deterministic broadphase work for the combined stress scene.

use crcbl::math::DVec3;
use crcbl::phys::{ColliderId, PhysicsWorld, QueryScratch, Sphere};

use super::Scene;

const BODY_COUNT: usize = 5_000;
const EXTENT: f64 = 48.0;
const BODY_RADIUS: f64 = 0.2;
const QUERY_RADIUS: f64 = 1.0;
const SPEED: f64 = 1.5;

/// The deterministic simulation half of [`Scene::Combined`].
#[derive(Debug)]
pub struct Simulation {
    world: PhysicsWorld,
    ids: Vec<ColliderId>,
    positions: Vec<DVec3>,
    velocities: Vec<DVec3>,
    scratch: QueryScratch,
    out: Vec<ColliderId>,
    last_results: usize,
}

impl Simulation {
    /// Build the fixed dense crowd, or no simulation for a render-only scene.
    #[must_use]
    pub fn for_scene(scene: Scene) -> Option<Self> {
        scene.simulates().then(|| Self::with_count(BODY_COUNT))
    }

    #[allow(clippy::cast_precision_loss)]
    fn with_count(count: usize) -> Self {
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
            scratch: QueryScratch::new(),
            out: Vec::new(),
            last_results: 0,
        }
    }

    /// Move, refit and query the whole crowd once.
    pub fn step(&mut self, dt: f64) {
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

        let queries = self.world.overlap_queries();
        let mut results = 0usize;
        for &position in &self.positions {
            queries.overlap_sphere_into(position, QUERY_RADIUS, &mut self.scratch, &mut self.out);
            results = results.saturating_add(self.out.len());
        }
        self.last_results = std::hint::black_box(results);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_combined_fixture_moves_refits_and_queries_every_body() {
        let mut simulation = Simulation::with_count(64);
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
    }
}
