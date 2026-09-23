//! The Tower room, rung 2's proving scenes: a column of twenty cubes, a
//! pyramid twenty cubes across, and a run of dominoes.
//!
//! ```text
//!    ▢                 ▢
//!    ▢                ▢▢
//!    ▢               ▢▢▢          20 rows, 210 cubes
//!    ▢     ...      ▢▢▢▢
//!    ▢             ▢▢▢▢▢▢
//!    ▢  20 cubes  ▢▢▢▢▢▢▢▢▢▢
//!  ──┴──────────────────────────── the floor: a plane
//!      ▌▌▌▌▌▌▌▌▌▌▌▌▌▌▌             dominoes, in front, toppled again every
//!                                  few seconds
//! ```
//!
//! Every pair here is box against box or box against the floor: the cached
//! separating axis test, the clipping, the four-point reduction and the
//! feature ids of rung 2, with friction at each manifold's centroid and a
//! twist term. Its counters are the rung's: points per manifold, the share of
//! points whose feature id persisted from the tick before, and how far the
//! column's and the pyramid's top boxes have drifted from where they started.
//!
//! # Two physics systems, and why
//!
//! **The pyramid runs at the default settings** — four substeps, 30 Hz
//! contacts — alone in its system, so the room's solver time is the
//! pyramid's, comparable with Box3D's benchmark at the same settings.
//!
//! **The column runs at [`ContactSettings::TALL_STACK`]**, with the dominoes:
//! at 30 Hz a soft contact is too soft a joint for twenty cubes of this size,
//! which buckle under their own weight past about twelve — Greenhill's height,
//! worked out beside that constant. Decision 1 gives a long chain more
//! substeps for its group; groups are not built, so the column's system takes
//! them whole.
//!
//! # Sleep
//!
//! Rung 3's claim is that every scene here settles to zero awake bodies: the
//! pyramid and the column sleep once they have stopped squeezing, and the
//! dominoes once they have all fallen, until the next flick teleports them
//! upright and wakes them.

use crcbl::ecs::{Entity, SystemTrait as _};
use crcbl::math::DVec3;
use crcbl::phys::{
    ColliderComponent, ContactSettings, GravityForce, MassProperties, PhysicsSystem, RigidBody,
    SurfaceMaterial, Transform,
};

use crate::scene::{Room, Shape, Tally, Tint, entity};

/// Where the room stands: the middle of its floor.
pub const TOWER_AT: DVec3 = DVec3::new(44.0, 0.0, 0.0);
/// A cube's half-extent, in metres.
pub const CUBE_HALF: f64 = 0.25;
/// A cube's mass, in kilograms.
const CUBE_MASS: f64 = 10.0;
/// Cubes in the column.
pub const COLUMN: u32 = 20;
/// Where the column stands, from the room's middle.
const COLUMN_AT: DVec3 = DVec3::new(-7.5, 0.0, 0.0);
/// Cubes along the pyramid's bottom row.
pub const PYRAMID_BASE: u32 = 20;

/// Dominoes in the run.
pub const DOMINOES: u32 = 15;
/// A domino's half-extents: thin along the run, a metre tall.
pub const DOMINO_HALF: DVec3 = DVec3::new(0.05, 0.5, 0.25);
/// A domino's mass, in kilograms.
const DOMINO_MASS: f64 = 2.0;
/// The distance between dominoes along the run.
const DOMINO_SPACING: f64 = 0.6;
/// Where the first domino stands, from the room's middle.
const DOMINO_START: DVec3 = DVec3::new(-4.2, 0.0, 3.0);
/// How hard the first domino is flicked over, in rad/s.
const FLICK: f64 = 2.0;
/// How often the dominoes are stood back up and flicked again, in ticks.
pub const DOMINO_EVERY_TICKS: u64 = 480;
/// How far from upright a domino must lean to count as down, as the cosine of
/// the angle: 60°.
const DOWN: f64 = 0.5;

/// Box2D's default friction, and no bounce.
const SURFACE: SurfaceMaterial = SurfaceMaterial::new(0.6, 0.0);

/// The first entity index a domino gets; the column's cubes count up from zero.
const FIRST_DOMINO: u32 = 1_000;

/// What the Tower room's counters read.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct TowerReading {
    /// How far the column's top cube is from where it started, in metres.
    pub column_drift: f64,
    /// The sideways part of that.
    pub column_sway: f64,
    /// How far the pyramid's top cube is from where it started, in metres.
    pub pyramid_drift: f64,
    /// The sideways part of that.
    pub pyramid_sway: f64,
    /// Dominoes leaning past 60° in the current run.
    pub dominoes_down: u32,
    /// Domino runs so far, the current one included.
    pub runs: u64,
    /// The pyramid's contact counters.
    pub pyramid: Tally,
    /// The column's and the dominoes' contact counters.
    pub column: Tally,
}

/// The Tower room.
#[derive(Debug)]
pub struct Tower {
    /// The pyramid, at the default settings.
    stack: PhysicsSystem,
    /// The column and the dominoes, at [`ContactSettings::TALL_STACK`].
    tall: PhysicsSystem,
    pyramid: Vec<Entity>,
    column: Vec<Entity>,
    dominoes: Vec<Entity>,
    pyramid_top_start: DVec3,
    column_top_start: DVec3,
    runs: u64,
    tick: u64,
    stack_tally: Tally,
    tall_tally: Tally,
}

impl Default for Tower {
    fn default() -> Self {
        Self::new()
    }
}

/// A system with contacts, Earth gravity and the floor.
fn system(settings: ContactSettings) -> PhysicsSystem {
    let mut phys = PhysicsSystem::with_contacts(settings);
    phys.add_force_provider(Box::new(GravityForce::EARTH));
    phys.add_plane(DVec3::Y, 0.0, SURFACE);
    phys
}

/// A dynamic box of `half` and `mass` at `at`.
fn place(phys: &mut PhysicsSystem, e: Entity, at: DVec3, half: DVec3, mass: f64) {
    let inertia = MassProperties::cuboid(mass, half, DVec3::ZERO).inertia;
    phys.set_body(e, RigidBody::new_dynamic(mass).with_inertia(inertia));
    let transform = Transform::from_position(at);
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
    phys.set_material(e, SURFACE);
}

/// Where domino `k` stands upright.
fn domino_home(k: u32) -> DVec3 {
    TOWER_AT + DOMINO_START + DVec3::new(DOMINO_SPACING * f64::from(k), DOMINO_HALF.y, 0.0)
}

impl Tower {
    /// Every scene standing, the first domino just flicked.
    #[must_use]
    pub fn new() -> Self {
        let cube = DVec3::splat(CUBE_HALF);
        let mut stack = system(ContactSettings::DEFAULT);
        let mut pyramid = Vec::new();
        for row in 0..PYRAMID_BASE {
            let across = PYRAMID_BASE - row;
            for k in 0..across {
                let x = (f64::from(k) - 0.5 * f64::from(across - 1)) * 2.0 * CUBE_HALF;
                let y = CUBE_HALF + 2.0 * CUBE_HALF * f64::from(row);
                let e = entity(u32::try_from(pyramid.len()).expect("a few hundred cubes"));
                place(
                    &mut stack,
                    e,
                    TOWER_AT + DVec3::new(x, y, 0.0),
                    cube,
                    CUBE_MASS,
                );
                pyramid.push(e);
            }
        }

        let mut tall = system(ContactSettings::TALL_STACK);
        let column: Vec<Entity> = (0..COLUMN)
            .map(|i| {
                let e = entity(i);
                let y = CUBE_HALF + 2.0 * CUBE_HALF * f64::from(i);
                place(
                    &mut tall,
                    e,
                    TOWER_AT + COLUMN_AT + DVec3::Y * y,
                    cube,
                    CUBE_MASS,
                );
                e
            })
            .collect();
        let dominoes: Vec<Entity> = (0..DOMINOES)
            .map(|k| {
                let e = entity(FIRST_DOMINO + k);
                place(&mut tall, e, domino_home(k), DOMINO_HALF, DOMINO_MASS);
                e
            })
            .collect();

        let top = |phys: &PhysicsSystem, boxes: &[Entity]| {
            let e = *boxes.last().expect("a stack has a top");
            phys.transform(e).expect("placed").position
        };
        let mut tower = Self {
            pyramid_top_start: top(&stack, &pyramid),
            column_top_start: top(&tall, &column),
            stack,
            tall,
            pyramid,
            column,
            dominoes,
            runs: 0,
            tick: 0,
            stack_tally: Tally::default(),
            tall_tally: Tally::default(),
        };
        tower.flick();
        tower
    }

    /// Stands every domino back up and flicks the first towards the rest.
    fn flick(&mut self) {
        for (k, &e) in (0u32..).zip(&self.dominoes) {
            self.tall
                .set_transform(e, Transform::from_position(domino_home(k)));
            if let Some(body) = self.tall.body_mut(e) {
                body.velocity = DVec3::ZERO;
                body.angular_velocity = DVec3::ZERO;
            }
        }
        if let Some(body) = self.tall.body_mut(self.dominoes[0]) {
            body.angular_velocity = DVec3::new(0.0, 0.0, -FLICK);
        }
        self.runs += 1;
    }

    fn position(phys: &PhysicsSystem, e: Entity) -> DVec3 {
        phys.transform(e).expect("a tower body").position
    }

    /// Every counter, at this instant.
    #[must_use]
    pub fn reading(&self) -> TowerReading {
        let drift = |phys: &PhysicsSystem, boxes: &[Entity], start: DVec3| {
            let d = Self::position(phys, *boxes.last().expect("a top")) - start;
            (d.length(), DVec3::new(d.x, 0.0, d.z).length())
        };
        let (column_drift, column_sway) = drift(&self.tall, &self.column, self.column_top_start);
        let (pyramid_drift, pyramid_sway) =
            drift(&self.stack, &self.pyramid, self.pyramid_top_start);
        let dominoes_down = self
            .dominoes
            .iter()
            .filter(|&&e| {
                let up = self.tall.transform(e).expect("a domino").rotation * DVec3::Y;
                up.y < DOWN
            })
            .count();
        TowerReading {
            column_drift,
            column_sway,
            pyramid_drift,
            pyramid_sway,
            dominoes_down: u32::try_from(dominoes_down).expect("fifteen dominoes"),
            runs: self.runs,
            pyramid: self.stack_tally,
            column: self.tall_tally,
        }
    }

    /// Where every pyramid cube is.
    #[cfg(test)]
    pub(crate) fn pyramid_positions(&self) -> Vec<DVec3> {
        self.pyramid
            .iter()
            .map(|&e| Self::position(&self.stack, e))
            .collect()
    }
}

impl Room for Tower {
    fn step(&mut self, dt: f64, mut clock: Option<&mut dyn FnMut() -> f64>) {
        match clock.as_deref_mut() {
            Some(clock) => self.stack.step_timed(dt, clock),
            None => self.stack.step(dt),
        }
        match clock {
            Some(clock) => self.tall.step_timed(dt, clock),
            None => self.tall.step(dt),
        }
        self.tick += 1;
        self.stack_tally.add(&self.stack.contact_counters());
        self.tall_tally.add(&self.tall.contact_counters());
        if self.tick.is_multiple_of(DOMINO_EVERY_TICKS) {
            self.flick();
        }
    }

    fn hash(&self, hasher: &mut dyn std::hash::Hasher) {
        self.stack.hash_state(hasher);
        self.tall.hash_state(hasher);
    }

    fn bodies(&self, out: &mut Vec<Shape>) {
        // The two systems name their bodies from the same indices, so the tall
        // system's keys have their top bit set to keep them apart.
        const TALL_KEY: u64 = 1 << 63;
        let mut push = |phys: &PhysicsSystem, salt: u64, e: Entity, half: DVec3, tint: Tint| {
            if let Some(t) = phys.transform(e) {
                out.push(Shape::Box {
                    key: e.to_bits() | salt,
                    centre: t.position,
                    rotation: t.rotation,
                    half,
                    tint,
                });
            }
        };
        let cube = DVec3::splat(CUBE_HALF);
        for &e in &self.pyramid {
            push(&self.stack, 0, e, cube, Tint::Box);
        }
        for &e in &self.column {
            push(&self.tall, TALL_KEY, e, cube, Tint::Box);
        }
        for &e in &self.dominoes {
            push(&self.tall, TALL_KEY, e, DOMINO_HALF, Tint::Pill);
        }
    }

    fn fixtures(&self, _out: &mut Vec<Shape>) {}
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::scene::tests::tick_dt;

    /// **The room's claims over ten seconds**: the column and the pyramid
    /// stand, their top boxes sunk by the contact springs' squeeze and hardly
    /// moved sideways; the dominoes all went down on the first run; and the
    /// feature ids persisted.
    ///
    /// Measured on 2026-09-23 at tick 600, before rung 3: the column's top
    /// cube 1.19 cm from where it started, 1.7 mm of it sideways; the
    /// pyramid's 2.76 cm, 0.39 mm sideways, and no pyramid cube further; all
    /// fifteen dominoes down by tick 238; the pyramid's 590 contacts at 4
    /// points each, every one of them persisted. With rung 3's sleep, the
    /// column's top cube 1.24 cm off, 3.8 mm of it sideways, and the
    /// pyramid's 2.73 cm, 2.40 mm sideways.
    ///
    /// The points are read on every tick the pyramid is awake, and the
    /// persisted ids on the last of them, where it settled: asleep, its
    /// contacts are not collided and count nothing.
    ///
    /// **Since rung 3 the pyramid sleeps at tick 58, with its top cube 2.40 mm
    /// aside**, and stays there. Awake, the same cube had crept back to
    /// 0.39 mm by tick 600, at a fraction of a millimetre a second — far
    /// slower than the sleep threshold — so the sideways bound is now the
    /// sway it sleeps with. `stacking.rs`'s `a_base_twenty_pyramid_holds`,
    /// with sleep off, is where the solver's own sway is held to a millimetre.
    #[test]
    fn the_column_and_the_pyramid_stand_and_the_dominoes_fall() {
        let mut tower = Tower::new();
        let mut all_down_by = None;
        let mut last_persisted = None;
        for tick in 0..600u64 {
            tower.step(tick_dt(), None);
            if all_down_by.is_none()
                && tick < DOMINO_EVERY_TICKS
                && tower.reading().dominoes_down == DOMINOES
            {
                all_down_by = Some(tick);
            }
            let pyramid = tower.reading().pyramid;
            if let Some(ratio) = pyramid.persisted_ratio() {
                last_persisted = Some(ratio);
                assert_eq!(pyramid.points_per_manifold(), Some(4.0), "{pyramid:?}");
            }
        }
        let r = tower.reading();
        assert!(
            all_down_by.is_some(),
            "the dominoes did not all fall: {r:?}"
        );
        assert!(r.column_sway < 5e-3, "{r:?}");
        assert!(r.column_drift < 0.02, "{r:?}");
        assert!(r.pyramid_sway < 3e-3, "{r:?}");
        let worst = Tower::new()
            .pyramid_positions()
            .iter()
            .zip(tower.pyramid_positions())
            .map(|(start, now)| (now - *start).length())
            .fold(0.0, f64::max);
        assert!(worst < 0.04, "a pyramid cube moved {worst} m");
        let persisted = last_persisted.expect("the pyramid was never awake");
        assert!(persisted > 0.99, "{persisted}: {r:?}");
        assert_eq!(r.runs, 2, "{r:?}");
    }

    /// **Every scene in the room settles to zero awake bodies** — rung 3's
    /// claim for the Tower room: the pyramid, and the column with the
    /// dominoes once they have all fallen, well before the next flick.
    ///
    /// Measured on 2026-09-23: the pyramid asleep from tick 58, the column
    /// and the dominoes from tick 276; the next flick is at tick 480.
    #[test]
    fn every_scene_in_the_room_settles_to_zero_awake_bodies() {
        let mut tower = Tower::new();
        let (mut pyramid_at, mut tall_at) = (None, None);
        for tick in 0..DOMINO_EVERY_TICKS - 1 {
            tower.step(tick_dt(), None);
            let r = tower.reading();
            if pyramid_at.is_none() && r.pyramid.bodies == 0 {
                pyramid_at = Some(tick);
            }
            if tall_at.is_none() && r.column.bodies == 0 {
                tall_at = Some(tick);
            }
        }
        let r = tower.reading();
        let pyramid_at = pyramid_at.unwrap_or_else(|| panic!("the pyramid never slept: {r:?}"));
        let tall_at = tall_at.unwrap_or_else(|| panic!("the column never slept: {r:?}"));
        assert_eq!(
            (r.pyramid.bodies, r.pyramid.sleeping),
            (0, 210),
            "the pyramid woke: {r:?}"
        );
        assert_eq!(
            (r.column.bodies, r.column.sleeping),
            (0, (COLUMN + DOMINOES) as usize),
            "the column or a domino woke: {r:?}"
        );
        assert_eq!(r.dominoes_down, DOMINOES, "{r:?}");
        assert!(pyramid_at < 120, "the pyramid slept at tick {pyramid_at}");
        assert!(
            tall_at < 400,
            "the column and dominoes slept at tick {tall_at}"
        );
        assert!(
            r.pyramid.solver_at_rest.is_none(),
            "untimed steps have no time"
        );
    }
}
