//! The Bullets room, rung 4's proving scenes: a cannon at a thin plate and at
//! a brick wall, and a fast spinning plank beside a pillar.
//!
//! ```text
//!    ▣ ○→      ┃ plate: a centimetre, static        sensor: behind it
//!
//!    ▣ ●→   ▤▤▤▤▤ brick wall: dynamic bricks        sensor: behind it
//!           ▤▤▤▤▤ (shots here are bullets)
//!
//!        ╲ ▮ pillar                                 a crossing is a tunnel
//!    ────╳──── plank, spinning at 60 rad/s on ice
//! ```
//!
//! **The cannon fires at point blank**, its charge a force held for one tick
//! that sends a shot from rest to [`SHOT_SPEED`] with the target less than a
//! tick's travel away. The tick's speculative contacts are sized from the
//! speed the shot began the tick with, which is none, so nothing but rung 4's
//! sweep stands between the shot and the far side: with `continuous` off in
//! [`ContactSettings`] every shot goes through. At the plate the shot is an
//! ordinary fast body, swept against static bodies; at the brick wall it is a
//! **bullet**, because the bricks are dynamic and only a bullet is swept
//! against dynamic bodies.
//!
//! **The plank** is two metres long and spins at [`PLANK_SPIN`] on a
//! frictionless floor, turning a radian a tick towards a two-centimetre
//! pillar; every [`PLANK_EVERY_TICKS`] it is set spinning again. Its manifold
//! with the pillar is built from where it is when the tick begins, so the part
//! of it that meets the pillar is one the manifold did not measure from: that
//! is the rotation the sweep's turning bound is for.
//!
//! The counters are the rung's: bodies swept, sweep candidates, hits and the
//! time dropped, and the tunnels each sensor counted — a shot's centre behind
//! the plate or the wall, and the pillar crossing the plank's length.

use std::collections::VecDeque;

use crcbl::core::trig;
use crcbl::ecs::{Entity, SystemTrait as _};
use crcbl::math::DVec3;
use crcbl::phys::{
    ColliderComponent, ContactSettings, GravityForce, MassProperties, PhysicsSystem, RigidBody,
    SurfaceMaterial, Transform,
};

use crate::scene::{Room, Shape, Tally, Tint, entity};

/// Where the room stands: the middle of its floor.
pub const BULLETS_AT: DVec3 = DVec3::new(66.0, 0.0, 0.0);

/// How fast the cannon sends a shot, in m/s, within the tick it fires.
pub const SHOT_SPEED: f64 = 80.0;
/// A shot's radius.
pub const SHOT_RADIUS: f64 = 0.05;
/// A shot's mass, in kilograms.
const SHOT_MASS: f64 = 0.2;
/// How often the cannon fires, in ticks, turn about at the plate and at the
/// wall: long enough for a stopped shot to fall out of the next one's way.
pub const FIRE_EVERY_TICKS: u64 = 30;
/// The most shots in the room at once.
pub const MAX_SHOTS: usize = 24;

/// The plate's half-extents: a centimetre thick.
pub const PLATE_HALF: DVec3 = DVec3::new(0.005, 0.75, 0.75);
/// The plate's lane, from the room's middle.
const PLATE_LANE: DVec3 = DVec3::new(0.0, 0.75, -2.5);

/// A brick's half-extents.
pub const BRICK_HALF: DVec3 = DVec3::new(0.1, 0.1, 0.2);
/// A brick's mass, in kilograms: heavy, so a shot does not open the wall.
const BRICK_MASS: f64 = 50.0;
/// Bricks along a row.
pub const BRICKS_ACROSS: u32 = 5;
/// Rows of bricks.
pub const BRICK_ROWS: u32 = 6;
/// The wall's lane, from the room's middle, on the floor.
const WALL_LANE: DVec3 = DVec3::new(0.0, 0.0, 0.0);
/// How high the cannon aims at the wall.
const WALL_AIM: f64 = 0.5;
/// How often the wall is built again, in ticks.
pub const WALL_EVERY_TICKS: u64 = 600;

/// How far in front of its target the cannon's muzzle is: less than a tick's
/// travel at [`SHOT_SPEED`].
const MUZZLE: f64 = 0.6;

/// The plank's half-extents: two metres long, ten centimetres wide.
pub const PLANK_HALF: DVec3 = DVec3::new(1.0, 0.02, 0.05);
/// The plank's mass, in kilograms.
const PLANK_MASS: f64 = 4.0;
/// How fast the plank is set spinning, in rad/s.
pub const PLANK_SPIN: f64 = 60.0;
/// How often the plank is set spinning again, in ticks.
pub const PLANK_EVERY_TICKS: u64 = 30;
/// Where the plank spins, from the room's middle.
const PLANK_AT: DVec3 = DVec3::new(0.0, 0.0, 3.5);
/// How far out the pillar stands from the plank's middle.
const PILLAR_RADIUS: f64 = 0.85;
/// How far round the plank's turn the pillar stands, in radians.
const PILLAR_ANGLE: f64 = 1.0;
/// The pillar's half-extents: two centimetres square.
pub const PILLAR_HALF: DVec3 = DVec3::new(0.01, 0.5, 0.01);
/// The half-extents of the patch of ice the plank spins on.
const ICE_HALF: DVec3 = DVec3::new(1.3, 0.05, 1.3);
/// How far the ice's top stands above the floor.
const ICE_TOP: f64 = 0.01;

/// Shots, plate and bricks: no bounce, so a stopped shot drops.
const DEAD: SurfaceMaterial = SurfaceMaterial::new(0.6, 0.0);
/// The plank and the patch of floor it spins on: ice.
const ICE: SurfaceMaterial = SurfaceMaterial::new(0.0, 0.0);

/// The first entity index a brick gets; fixtures count up from zero.
const FIRST_BRICK: u32 = 100;
/// The plank's entity index.
const PLANK: u32 = 90;
/// The first entity index a shot gets.
const FIRST_SHOT: u32 = 10_000;
/// How many entity indices the shots cycle through.
const SHOT_IDS: u64 = 1_000_000;

/// Which target a shot was fired at.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Lane {
    Plate,
    Wall,
}

/// A fixture: a static box, its surface and how it is drawn.
#[derive(Clone, Copy, Debug)]
struct Fixture {
    centre: DVec3,
    half: DVec3,
    material: SurfaceMaterial,
    tint: Tint,
}

/// What the Bullets room's counters read.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct BulletsReading {
    /// Shots fired so far.
    pub shots: u64,
    /// Shots whose centre got behind the plate.
    pub plate_tunnels: u64,
    /// Shots whose centre got behind the brick wall.
    pub wall_tunnels: u64,
    /// Times the pillar crossed the plank's length.
    pub plank_tunnels: u64,
    /// The contact counters.
    pub contacts: Tally,
}

/// The Bullets room.
#[derive(Debug)]
pub struct Bullets {
    phys: PhysicsSystem,
    fixtures: Vec<Fixture>,
    bricks: Vec<Entity>,
    plank: Entity,
    /// The shots in the room, oldest first: each, its lane and whether its
    /// sensor has counted it.
    shots: VecDeque<(Entity, Lane, bool)>,
    fired: u64,
    plate_tunnels: u64,
    wall_tunnels: u64,
    plank_tunnels: u64,
    /// Which side of the plank's length the pillar was on last tick, while it
    /// was within reach of it.
    pillar_side: Option<f64>,
    tick: u64,
    tally: Tally,
}

impl Default for Bullets {
    fn default() -> Self {
        Self::new()
    }
}

/// Where the pillar stands.
fn pillar_at() -> DVec3 {
    BULLETS_AT
        + PLANK_AT
        + DVec3::new(
            PILLAR_RADIUS * trig::cos(PILLAR_ANGLE),
            PILLAR_HALF.y,
            PILLAR_RADIUS * trig::sin(PILLAR_ANGLE),
        )
}

/// Where brick `k` of the wall stands.
fn brick_home(k: u32) -> DVec3 {
    let (row, column) = (k / BRICKS_ACROSS, k % BRICKS_ACROSS);
    let across = (f64::from(column) - 0.5 * f64::from(BRICKS_ACROSS - 1)) * 2.0 * BRICK_HALF.z;
    BULLETS_AT + WALL_LANE + DVec3::new(0.0, BRICK_HALF.y * (1.0 + 2.0 * f64::from(row)), across)
}

fn box_collider(half: DVec3) -> ColliderComponent {
    ColliderComponent::Box {
        offset: DVec3::ZERO,
        half_extents: half,
        is_trigger: false,
    }
}

impl Bullets {
    /// The room with its wall built, its plank spinning and its cannon about
    /// to fire.
    #[must_use]
    pub fn new() -> Self {
        let mut phys = PhysicsSystem::with_contacts(ContactSettings::DEFAULT);
        phys.add_force_provider(Box::new(GravityForce::EARTH));
        phys.add_plane(DVec3::Y, 0.0, DEAD);

        let cannon = |lane: DVec3| Fixture {
            centre: BULLETS_AT + lane + DVec3::new(-MUZZLE - 0.4, 0.0, 0.0),
            half: DVec3::new(0.25, 0.12, 0.12),
            material: DEAD,
            tint: Tint::Peg,
        };
        let fixtures = vec![
            Fixture {
                centre: BULLETS_AT + PLATE_LANE,
                half: PLATE_HALF,
                material: DEAD,
                tint: Tint::Board,
            },
            Fixture {
                centre: pillar_at(),
                half: PILLAR_HALF,
                material: DEAD,
                tint: Tint::Peg,
            },
            // The ice stands a little proud of the floor, so the plank on it
            // never touches the floor's grip.
            Fixture {
                centre: BULLETS_AT + PLANK_AT + DVec3::Y * (ICE_TOP - ICE_HALF.y),
                half: ICE_HALF,
                material: ICE,
                tint: Tint::Board,
            },
            cannon(PLATE_LANE),
            cannon(WALL_LANE + DVec3::Y * WALL_AIM),
        ];
        for (index, fixture) in (0u32..).zip(&fixtures) {
            let e = entity(index);
            let transform = Transform::from_position(fixture.centre);
            phys.set_transform(e, transform);
            phys.set_collider(e, &box_collider(fixture.half), &transform);
            phys.set_material(e, fixture.material);
        }

        let brick_inertia = MassProperties::cuboid(BRICK_MASS, BRICK_HALF, DVec3::ZERO).inertia;
        let bricks = (0..BRICKS_ACROSS * BRICK_ROWS)
            .map(|k| {
                let e = entity(FIRST_BRICK + k);
                phys.set_body(
                    e,
                    RigidBody::new_dynamic(BRICK_MASS).with_inertia(brick_inertia),
                );
                let transform = Transform::from_position(brick_home(k));
                phys.set_transform(e, transform);
                phys.set_collider(e, &box_collider(BRICK_HALF), &transform);
                phys.set_material(e, DEAD);
                e
            })
            .collect();

        let plank = entity(PLANK);
        let inertia = MassProperties::cuboid(PLANK_MASS, PLANK_HALF, DVec3::ZERO).inertia;
        phys.set_body(
            plank,
            RigidBody::new_dynamic(PLANK_MASS).with_inertia(inertia),
        );
        let transform =
            Transform::from_position(BULLETS_AT + PLANK_AT + DVec3::Y * (ICE_TOP + PLANK_HALF.y));
        phys.set_transform(plank, transform);
        phys.set_collider(plank, &box_collider(PLANK_HALF), &transform);
        phys.set_material(plank, ICE);

        let mut room = Self {
            phys,
            fixtures,
            bricks,
            plank,
            shots: VecDeque::new(),
            fired: 0,
            plate_tunnels: 0,
            wall_tunnels: 0,
            plank_tunnels: 0,
            pillar_side: None,
            tick: 0,
            tally: Tally::default(),
        };
        room.spin_plank();
        room
    }

    /// Lays the plank flat where it spins, its `+X` end towards the pillar's
    /// side, and sets it spinning.
    fn spin_plank(&mut self) {
        self.phys.set_transform(
            self.plank,
            Transform::from_position(BULLETS_AT + PLANK_AT + DVec3::Y * (ICE_TOP + PLANK_HALF.y)),
        );
        if let Some(body) = self.phys.body_mut(self.plank) {
            body.velocity = DVec3::ZERO;
            // Turning +X towards +Z, where the pillar stands.
            body.angular_velocity = DVec3::NEG_Y * PLANK_SPIN;
        }
        self.pillar_side = None;
    }

    /// Stands every brick back where the wall has it.
    fn build_wall(&mut self) {
        for (k, &e) in (0u32..).zip(&self.bricks) {
            self.phys
                .set_transform(e, Transform::from_position(brick_home(k)));
            if let Some(body) = self.phys.body_mut(e) {
                body.velocity = DVec3::ZERO;
                body.angular_velocity = DVec3::ZERO;
            }
        }
    }

    /// Loads a shot at a lane's muzzle and fires it: a force held for this
    /// tick, taking it from rest to [`SHOT_SPEED`].
    fn fire(&mut self, dt: f64) {
        let n = self.fired;
        let lane = if n.is_multiple_of(2) {
            Lane::Plate
        } else {
            Lane::Wall
        };
        let target = match lane {
            Lane::Plate => BULLETS_AT + PLATE_LANE + DVec3::X * -PLATE_HALF.x,
            Lane::Wall => BULLETS_AT + WALL_LANE + DVec3::new(-BRICK_HALF.x, WALL_AIM, 0.0),
        };
        let e = entity(FIRST_SHOT + u32::try_from(n % SHOT_IDS).expect("under SHOT_IDS"));
        let inertia = MassProperties::sphere(SHOT_MASS, SHOT_RADIUS, DVec3::ZERO).inertia;
        let body = RigidBody::new_dynamic(SHOT_MASS)
            .with_inertia(inertia)
            .with_bullet(lane == Lane::Wall);
        let transform = Transform::from_position(target - DVec3::X * MUZZLE);
        self.phys.set_body(e, body);
        self.phys.set_transform(e, transform);
        self.phys.set_collider(
            e,
            &ColliderComponent::Sphere {
                offset: DVec3::ZERO,
                radius: SHOT_RADIUS,
                is_trigger: false,
            },
            &transform,
        );
        self.phys.set_material(e, DEAD);
        self.phys
            .apply_force(e, DVec3::X * (SHOT_MASS * SHOT_SPEED / dt));
        self.shots.push_back((e, lane, false));
        self.fired += 1;
        if self.shots.len() > MAX_SHOTS
            && let Some((oldest, _, _)) = self.shots.pop_front()
        {
            self.phys.remove_entity(oldest);
        }
    }

    /// Counts every shot that has got behind its target, once each, and the
    /// pillar crossing the plank's length.
    fn read_sensors(&mut self) {
        for (e, lane, counted) in &mut self.shots {
            let Some(at) = self.phys.transform(*e).map(|t| t.position) else {
                continue;
            };
            let (behind, within) = match lane {
                Lane::Plate => {
                    let local = at - (BULLETS_AT + PLATE_LANE);
                    (
                        local.x > PLATE_HALF.x,
                        local.y.abs() < PLATE_HALF.y && local.z.abs() < PLATE_HALF.z,
                    )
                }
                Lane::Wall => {
                    let local = at - (BULLETS_AT + WALL_LANE);
                    (
                        local.x > BRICK_HALF.x,
                        local.y < 2.0 * BRICK_HALF.y * f64::from(BRICK_ROWS)
                            && local.z.abs() < BRICK_HALF.z * f64::from(BRICKS_ACROSS),
                    )
                }
            };
            if behind && within && !*counted {
                *counted = true;
                match lane {
                    Lane::Plate => self.plate_tunnels += 1,
                    Lane::Wall => self.wall_tunnels += 1,
                }
            }
        }

        let Some(t) = self.phys.transform(self.plank) else {
            return;
        };
        let along = t.rotation * DVec3::X;
        let to = pillar_at() - t.position;
        let within = along.x * to.x + along.z * to.z > 0.0
            && to.x * to.x + to.z * to.z < PLANK_HALF.x * PLANK_HALF.x
            && along.x * along.x + along.z * along.z > 0.25;
        let side = along.x * to.z - along.z * to.x;
        if within {
            if self
                .pillar_side
                .is_some_and(|before| before.signum() != side.signum())
            {
                self.plank_tunnels += 1;
            }
            self.pillar_side = Some(side);
        } else {
            self.pillar_side = None;
        }
    }

    /// Every counter, at this instant.
    #[must_use]
    pub fn reading(&self) -> BulletsReading {
        BulletsReading {
            shots: self.fired,
            plate_tunnels: self.plate_tunnels,
            wall_tunnels: self.wall_tunnels,
            plank_tunnels: self.plank_tunnels,
            contacts: self.tally,
        }
    }
}

impl Room for Bullets {
    fn step(&mut self, dt: f64, clock: Option<&mut dyn FnMut() -> f64>) {
        if self.tick.is_multiple_of(FIRE_EVERY_TICKS) {
            self.fire(dt);
        }
        match clock {
            Some(clock) => self.phys.step_timed(dt, clock),
            None => self.phys.step(dt),
        }
        self.tick += 1;
        self.tally.add(&self.phys.contact_counters());
        self.read_sensors();
        if self.tick.is_multiple_of(PLANK_EVERY_TICKS) {
            self.spin_plank();
        }
        if self.tick.is_multiple_of(WALL_EVERY_TICKS) {
            self.build_wall();
        }
    }

    fn hash(&self, hasher: &mut dyn std::hash::Hasher) {
        self.phys.hash_state(hasher);
    }

    fn bodies(&self, out: &mut Vec<Shape>) {
        let mut push_box = |e: Entity, half: DVec3, tint: Tint| {
            if let Some(t) = self.phys.transform(e) {
                out.push(Shape::Box {
                    key: e.to_bits(),
                    centre: t.position,
                    rotation: t.rotation,
                    half,
                    tint,
                });
            }
        };
        for &e in &self.bricks {
            push_box(e, BRICK_HALF, Tint::Box);
        }
        push_box(self.plank, PLANK_HALF, Tint::Pill);
        for &(e, _, _) in &self.shots {
            if let Some(t) = self.phys.transform(e) {
                out.push(Shape::Sphere {
                    key: e.to_bits(),
                    centre: t.position,
                    radius: SHOT_RADIUS,
                    tint: Tint::Ball,
                });
            }
        }
    }

    fn fixtures(&self, out: &mut Vec<Shape>) {
        for (index, fixture) in (0u64..).zip(&self.fixtures) {
            out.push(Shape::Box {
                key: index,
                centre: fixture.centre,
                rotation: crcbl::math::DQuat::IDENTITY,
                half: fixture.half,
                tint: fixture.tint,
            });
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::scene::tests::tick_dt;

    /// Twenty seconds of the room, with the sweeps on or off.
    fn run(continuous: bool) -> Bullets {
        let mut room = Bullets::new();
        if let Some(settings) = room.phys.contact_settings_mut() {
            settings.continuous = continuous;
        }
        for _ in 0..1200 {
            room.step(tick_dt(), None);
        }
        room
    }

    /// **Nothing tunnels through the room's sensors**: in twenty seconds the
    /// cannon fires forty shots at point blank, half at the plate and half,
    /// as bullets, at the brick wall, and the plank is set spinning at the
    /// pillar forty times, and no shot gets behind either target and the
    /// pillar never crosses the plank.
    ///
    /// Measured on 2026-09-23: without the sweeps every shot and every spin of
    /// the plank tunnels — 20, 20 and 40 — and the deepest overlap in any tick
    /// is 5.02 cm; with them none does, 364 swept bodies are stopped with
    /// 2.92 s of their motion dropped, and the deepest overlap is 0.77 cm.
    #[test]
    fn nothing_tunnels_through_the_rooms_sensors() {
        let off = run(false).reading();
        assert_eq!(off.shots, 40, "{off:?}");
        assert_eq!(
            (off.plate_tunnels, off.wall_tunnels),
            (20, 20),
            "without sweeps every shot should go through: {off:?}"
        );
        assert!(
            off.plank_tunnels > 0,
            "without sweeps the plank should pass the pillar: {off:?}"
        );

        let on = run(true).reading();
        assert_eq!(on.shots, 40, "{on:?}");
        assert_eq!(
            (on.plate_tunnels, on.wall_tunnels, on.plank_tunnels),
            (0, 0, 0),
            "{on:?}"
        );
        let tally = on.contacts;
        assert!(tally.sweep_hits >= 40, "{tally:?}");
        assert!(tally.dropped_time > 0.0, "{tally:?}");
        assert!(tally.peak_penetration < 0.02, "{tally:?}");
    }
}
