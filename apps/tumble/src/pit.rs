//! The ball pit — the user's second scene, at rung 1: a thousand balls poured
//! into a walled pit, and the benchmark of what they cost.
//!
//! ```text
//!              ○ ○ ○ ○ ○        a wave of balls every few ticks,
//!               ○ ○ ○ ○          until there are a thousand
//!      ┃                   ┃
//!      ┃ ○○○○○○○○○○○○○○○○○ ┃
//!      ┃○○○○○○○○○○○○○○○○○○○┃   four static walls on a plane
//!      ┗━━━━━━━━━━━━━━━━━━━┛
//! ```
//!
//! Its counters are the rung's cost: pairs, contacts begun and ended, and the
//! broadphase, narrow-phase and solver time of every tick. And rung 3's: once
//! the last wave has landed and the pile is still, it **sleeps** — awake
//! bodies fall to zero, and the solver's time at rest is what a settled
//! thousand balls cost.
//!
//! # What it cannot show yet
//!
//! `docs/plan/sample/24-tumble.md`'s pit spawns **without end**, overflows and
//! despawns what rolls past a radius; that is rung 6's, with the most bodies
//! held inside a tick. This one stops at [`BALLS`].

use crcbl::core::rand::hash_unit;
use crcbl::ecs::{Entity, SystemTrait as _};
use crcbl::math::DVec3;
use crcbl::phys::{
    ColliderComponent, ContactSettings, GravityForce, MassProperties, PhysicsSystem, RigidBody,
    SurfaceMaterial, Transform,
};

use crate::scene::{Room, Shape, Tally, Tint, entity};

/// Where the pit stands: the middle of its floor.
pub const PIT_AT: DVec3 = DVec3::new(26.0, 0.0, 0.0);
/// Half the pit's inside width, both ways.
pub const HALF_WIDTH: f64 = 1.5;
/// Half the walls' height.
const WALL_HALF_HEIGHT: f64 = 0.6;
/// Half the walls' thickness.
const WALL_HALF_THICKNESS: f64 = 0.05;

/// How many balls the pit fills to.
pub const BALLS: u32 = 1000;
/// A ball's radius.
pub const BALL_RADIUS: f64 = 0.1;
/// A ball's mass.
const BALL_MASS: f64 = 0.3;
/// Balls per wave.
const WAVE: u32 = 5;
/// Ticks between waves.
pub const WAVE_EVERY_TICKS: u64 = 4;
/// How high a wave starts.
const WAVE_HEIGHT: f64 = 2.5;
/// How fast a wave is thrown down, so the next one has room.
const WAVE_SPEED: f64 = 5.0;
/// How far from the middle a ball may start, both ways.
const SPREAD: f64 = 0.9;

/// The balls: a little grip, a little bounce.
const BALL_SURFACE: SurfaceMaterial = SurfaceMaterial::new(0.4, 0.2);
/// The walls and floor.
const PIT_SURFACE: SurfaceMaterial = SurfaceMaterial::new(0.5, 0.1);

/// The spawner's seed.
const SEED: u64 = 0x0b17_5eed;
/// The first entity index a ball gets; the walls count up from zero.
const FIRST_BALL: u32 = 100;

/// What the pit's counters read beyond its contacts.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct PitReading {
    /// Balls in the pit.
    pub balls: u32,
    /// The contact counters.
    pub contacts: Tally,
}

/// The ball pit.
#[derive(Debug)]
pub struct Pit {
    phys: PhysicsSystem,
    walls: Vec<(DVec3, DVec3)>,
    balls: Vec<Entity>,
    tick: u64,
    tally: Tally,
}

impl Default for Pit {
    fn default() -> Self {
        Self::new()
    }
}

impl Pit {
    /// The empty pit.
    #[must_use]
    pub fn new() -> Self {
        let mut phys = PhysicsSystem::with_contacts(ContactSettings::DEFAULT);
        phys.add_force_provider(Box::new(GravityForce::EARTH));
        phys.add_plane(DVec3::Y, 0.0, PIT_SURFACE);
        let reach = HALF_WIDTH + WALL_HALF_THICKNESS;
        let walls = vec![
            (
                PIT_AT + DVec3::new(-reach, WALL_HALF_HEIGHT, 0.0),
                DVec3::new(WALL_HALF_THICKNESS, WALL_HALF_HEIGHT, reach),
            ),
            (
                PIT_AT + DVec3::new(reach, WALL_HALF_HEIGHT, 0.0),
                DVec3::new(WALL_HALF_THICKNESS, WALL_HALF_HEIGHT, reach),
            ),
            (
                PIT_AT + DVec3::new(0.0, WALL_HALF_HEIGHT, -reach),
                DVec3::new(reach, WALL_HALF_HEIGHT, WALL_HALF_THICKNESS),
            ),
            (
                PIT_AT + DVec3::new(0.0, WALL_HALF_HEIGHT, reach),
                DVec3::new(reach, WALL_HALF_HEIGHT, WALL_HALF_THICKNESS),
            ),
        ];
        for (index, &(centre, half)) in (0u32..).zip(&walls) {
            let e = entity(index);
            let transform = Transform::from_position(centre);
            phys.set_transform(e, transform);
            phys.set_collider(
                e,
                &ColliderComponent::Box {
                    offset: DVec3::ZERO,
                    half_extents: half,
                    is_trigger: false,
                },
                &transform,
            );
            phys.set_material(e, PIT_SURFACE);
        }
        Self {
            phys,
            walls,
            balls: Vec::new(),
            tick: 0,
            tally: Tally::default(),
        }
    }

    fn wave(&mut self) {
        let inertia = MassProperties::sphere(BALL_MASS, BALL_RADIUS, DVec3::ZERO).inertia;
        for slot in 0..WAVE {
            let Ok(count) = u32::try_from(self.balls.len()) else {
                return;
            };
            if count >= BALLS {
                return;
            }
            let n = u64::from(count);
            // One ball per column of the wave, jittered within its column, so
            // no two in a wave overlap.
            let column = (f64::from(slot) + 0.5) / f64::from(WAVE);
            let x = (column * 2.0 - 1.0) * SPREAD + (hash_unit(SEED, 2 * n) - 0.5) * 0.1;
            let z = (hash_unit(SEED, 2 * n + 1) * 2.0 - 1.0) * SPREAD;
            let at = PIT_AT + DVec3::new(x, WAVE_HEIGHT, z);
            let e = entity(FIRST_BALL + count);
            let mut body = RigidBody::new_dynamic(BALL_MASS).with_inertia(inertia);
            body.velocity = DVec3::new(0.0, -WAVE_SPEED, 0.0);
            let transform = Transform::from_position(at);
            self.phys.set_body(e, body);
            self.phys.set_transform(e, transform);
            self.phys.set_collider(
                e,
                &ColliderComponent::Sphere {
                    offset: DVec3::ZERO,
                    radius: BALL_RADIUS,
                    is_trigger: false,
                },
                &transform,
            );
            self.phys.set_material(e, BALL_SURFACE);
            self.balls.push(e);
        }
    }

    /// Every counter, at this instant.
    #[must_use]
    pub fn reading(&self) -> PitReading {
        PitReading {
            balls: u32::try_from(self.balls.len()).unwrap_or(u32::MAX),
            contacts: self.tally,
        }
    }

    /// The deepest any ball has got outside the pit, in metres.
    #[cfg(test)]
    pub(crate) fn worst_escape(&self) -> f64 {
        self.balls
            .iter()
            .filter_map(|e| self.phys.transform(*e))
            .map(|t| {
                let local = t.position - PIT_AT;
                (BALL_RADIUS - local.y)
                    .max(local.x.abs() + BALL_RADIUS - HALF_WIDTH)
                    .max(local.z.abs() + BALL_RADIUS - HALF_WIDTH)
                    .max(0.0)
            })
            .fold(0.0, f64::max)
    }
}

impl Room for Pit {
    fn step(&mut self, dt: f64, clock: Option<&mut dyn FnMut() -> f64>) {
        if self.tick.is_multiple_of(WAVE_EVERY_TICKS) {
            self.wave();
        }
        match clock {
            Some(clock) => self.phys.step_timed(dt, clock),
            None => self.phys.step(dt),
        }
        self.tick += 1;
        self.tally.add(&self.phys.contact_counters());
    }

    fn hash(&self, hasher: &mut dyn std::hash::Hasher) {
        self.phys.hash_state(hasher);
    }

    fn bodies(&self, out: &mut Vec<Shape>) {
        for &e in &self.balls {
            if let Some(transform) = self.phys.transform(e) {
                out.push(Shape::Sphere {
                    key: e.to_bits(),
                    centre: transform.position,
                    radius: BALL_RADIUS,
                    tint: Tint::Ball,
                });
            }
        }
    }

    fn fixtures(&self, out: &mut Vec<Shape>) {
        for (key, &(centre, half)) in (0u64..).zip(&self.walls) {
            out.push(Shape::Box {
                key,
                centre,
                rotation: crcbl::math::DQuat::IDENTITY,
                half,
                tint: Tint::Board,
            });
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::scene::tests::tick_dt;

    /// **The pit fills to a thousand balls and holds them, and the pile comes
    /// to a stop**: no ball gets through a wall or the floor, and the settled
    /// pile's overlaps stay under a centimetre.
    ///
    /// Measured on 2026-09-17: the last wave lands at tick 800 and the pile is
    /// still by tick 1000. At tick 1200 the fastest ball moves under a
    /// micrometre a second and the deepest overlap is 7.0 mm, between two balls
    /// at the bottom of the pile — each soft contact is a spring, and the
    /// bottom ones carry the most weight. The furthest any ball got past a wall
    /// or into the floor, in any tick, was 1.2 cm, landing from a wave.
    #[test]
    fn the_pit_fills_to_a_thousand_holds_them_and_comes_to_rest() {
        let mut pit = Pit::new();
        let mut escape = 0.0f64;
        for _ in 0..1200 {
            pit.step(tick_dt(), None);
            escape = escape.max(pit.worst_escape());
        }
        let reading = pit.reading();
        assert_eq!(reading.balls, BALLS);
        assert!(escape < 0.02, "a ball got {escape} m out of the pit");
        assert!(
            reading.contacts.worst_penetration < 0.01,
            "{:?}",
            reading.contacts
        );
        let fastest = pit
            .balls
            .iter()
            .filter_map(|e| pit.phys.body(*e))
            .map(|body| body.velocity.length())
            .fold(0.0, f64::max);
        assert!(fastest < 1e-3, "the pile is still moving at {fastest} m/s");
    }

    /// **The full pit settles to zero awake bodies**, rung 3's claim for the
    /// ball pit: every ball asleep within a bound of the last wave landing,
    /// and the pile still in the pit.
    ///
    /// Measured on 2026-09-23: the last wave spawns at tick 796, and every
    /// ball is asleep at tick 1000, the whole pile one island. Since one-point
    /// contacts twist against their patch, measured the same day, at tick 966.
    #[test]
    fn the_full_pit_settles_to_zero_awake_bodies() {
        let mut pit = Pit::new();
        let mut asleep_at = None;
        for tick in 0..1400u32 {
            pit.step(tick_dt(), None);
            let reading = pit.reading();
            if reading.balls == BALLS && reading.contacts.bodies == 0 {
                asleep_at = Some(tick);
                break;
            }
        }
        let reading = pit.reading();
        let tick = asleep_at.unwrap_or_else(|| panic!("never settled: {:?}", reading.contacts));
        assert_eq!(reading.contacts.sleeping, BALLS as usize);
        assert!(pit.worst_escape() < 0.02);
        assert!(tick < 1200, "the pit settled only at tick {tick}");
    }

    /// **The pit's cost, stage by stage, once it is full and at rest** — the
    /// benchmark `docs/plan/36-contact-solver.md` rung 1 asks for. Ignored,
    /// because a timing means nothing in a debug build or on a loaded machine:
    ///
    /// ```text
    /// cargo test -p tumble --release --lib -- --ignored pit_benchmark --nocapture
    /// ```
    ///
    /// It fills the pit, lets it settle, then prints the mean and the worst of
    /// each stage over the next ten seconds of ticks. Since rung 3 a settled
    /// pit sleeps, so these are its costs at rest.
    #[test]
    #[ignore = "a timing: run it in a release build"]
    fn pit_benchmark() {
        const SETTLE: u32 = 1000;
        const MEASURE: u32 = 600;
        let mut pit = Pit::new();
        for _ in 0..SETTLE {
            pit.step(tick_dt(), None);
        }
        let epoch = std::time::Instant::now();
        let mut clock = || epoch.elapsed().as_secs_f64();
        let mut sum = [0.0f64; 3];
        let mut worst = [0.0f64; 3];
        for _ in 0..MEASURE {
            pit.step(tick_dt(), Some(&mut clock));
            let stages = pit.reading().contacts.stages.expect("timed");
            for (k, value) in [stages.broadphase, stages.narrow_phase, stages.solver]
                .into_iter()
                .enumerate()
            {
                sum[k] += value;
                worst[k] = worst[k].max(value);
            }
        }
        let tally = pit.reading().contacts;
        println!(
            "pit: {} balls, {} pairs, {} contacts, {} points-bearing ticks measured",
            pit.reading().balls,
            tally.pairs,
            tally.touching,
            MEASURE
        );
        for (k, name) in ["broadphase", "narrow phase", "solver"]
            .into_iter()
            .enumerate()
        {
            println!(
                "pit {name}: mean {:.3} ms, worst {:.3} ms",
                sum[k] / f64::from(MEASURE) * 1e3,
                worst[k] * 1e3
            );
        }
        println!(
            "pit step: mean {:.3} ms",
            sum.iter().sum::<f64>() / f64::from(MEASURE) * 1e3
        );
    }
}
