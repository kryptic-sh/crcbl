//! The obstacle wall — the user's first scene, and rung 1's proving scene, with rung
//! 2's cubes.
//!
//! ```text
//!           ○  ◯  ▪        spawner: a ball; every seventh a pill and every
//!                             fifth otherwise a cube
//!        ╲▁▁▁▁▁▁      ▁▁▁▁▁▁╱   deflector bars, tilted capsules
//!    │  •   •   •   •   •   •  │
//!    │    •   •   ◇   •   •    │  pegs: capsules end-on through the board
//!    │  •   •   •   •   •   •  │  wedges: turned boxes
//!    │    •   ◇   •   •   •    │
//!    │  •   •   •   •   •   •  │
//!    ├──┬──┬──┬──┬──┬──┬──┬──┤  bins: static boxes
//!    └──┴──┴──┴──┴──┴──┴──┴──┘  the floor: a plane
//! ```
//!
//! A board of static pegs, bars and wedges stands between a back board and an
//! invisible front pane a ball's width apart, and balls, pills and cubes drop
//! onto it and bounce down into the bins. Every rung 1 pair is in it: balls
//! against pegs and bars (sphere against capsule), against wedges and bins
//! (sphere against a turned box and an axis-aligned one), against the floor (a
//! plane) and against each other; pills against all of those as capsules.
//! And rung 2's: a cube against a wedge, a bin, the boards and another cube is
//! box against box.
//!
//! The wall holds [`MAX_LIVE`] bodies, and the oldest is taken out as each new
//! one drops — which is the despawn half of the counters, contacts ended as
//! well as begun.

use std::collections::VecDeque;

use crcbl::core::rand::hash_unit;
use crcbl::ecs::{Entity, SystemTrait as _};
use crcbl::math::{DQuat, DVec3};
use crcbl::phys::{
    ColliderComponent, ContactSettings, GravityForce, MassProperties, PhysicsSystem, RigidBody,
    SurfaceMaterial, Transform, rotation_from_scaled_axis,
};

use crate::scene::{Room, Shape, Tally, Tint, entity};

/// Where the wall stands: the middle of its floor.
pub const WALL_AT: DVec3 = DVec3::new(12.0, 0.0, 0.0);
/// Half the wall's width, inside the side walls.
pub const HALF_WIDTH: f64 = 2.0;
/// The height of the board.
pub const HEIGHT: f64 = 5.5;
/// Half the gap between the back board and the front pane.
pub const SLAB: f64 = 0.1;

/// A ball's radius.
pub const BALL_RADIUS: f64 = 0.07;
/// A ball's mass.
const BALL_MASS: f64 = 0.2;
/// A pill's radius and half the length of its core.
pub const PILL_RADIUS: f64 = 0.06;
/// Half a pill's core.
pub const PILL_HALF: f64 = 0.08;
/// A pill's mass.
const PILL_MASS: f64 = 0.25;
/// Every how many drops is a pill.
const PILL_EVERY: u64 = 7;
/// A cube's half-extent: small enough that turned any way it fits between the
/// boards, whose gap is twice [`SLAB`].
pub const CUBE_HALF: f64 = 0.05;
/// A cube's mass.
const CUBE_MASS: f64 = 0.3;
/// Every how many drops is a cube, where it is not a pill.
const CUBE_EVERY: u64 = 5;

const _: () = assert!(
    3.0 * CUBE_HALF * CUBE_HALF < SLAB * SLAB,
    "a cube's long diagonal fits between the boards"
);

/// A peg's radius.
pub const PEG_RADIUS: f64 = 0.05;

/// How often something drops, in ticks.
pub const DROP_EVERY_TICKS: u64 = 6;
/// How clear of every live body the drop point must be for a drop to happen:
/// a ball bouncing back up off the bars waits out a drop rather than having
/// one land inside it.
const DROP_CLEARANCE: f64 = 0.3;
/// How high it drops from.
const DROP_HEIGHT: f64 = 5.2;
/// The most bodies on the wall at once.
pub const MAX_LIVE: usize = 120;

/// The spawner's seed.
const SEED: u64 = 0x7a11_b0a2;

/// Balls and pills: bouncy, a little grip.
const BALL_SURFACE: SurfaceMaterial = SurfaceMaterial::new(0.2, 0.55);
/// Pegs, bars and wedges: as bouncy.
const PEG_SURFACE: SurfaceMaterial = SurfaceMaterial::new(0.2, 0.55);
/// The boards either side of the slab: slick and dead, so a ball rubbing one
/// is not held by it.
const PANE_SURFACE: SurfaceMaterial = SurfaceMaterial::new(0.0, 0.0);
/// The bins and the floor: grippy and dead, so what lands there stays.
const BIN_SURFACE: SurfaceMaterial = SurfaceMaterial::new(0.6, 0.1);

/// The first entity index a dropped body gets; fixtures count up from zero.
const FIRST_DROP: u32 = 10_000;
/// How many entity indices the drops cycle through.
const DROP_IDS: u64 = 1_000_000;

/// A fixture: a static shape and how it is drawn.
#[derive(Clone, Debug)]
struct Fixture {
    component: ColliderComponent,
    transform: Transform,
    material: SurfaceMaterial,
    /// How the page draws it, or `None` for the front pane, which is
    /// invisible.
    tint: Option<Tint>,
}

/// What the wall's counters read beyond its contacts.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct WallReading {
    /// Bodies dropped so far.
    pub dropped: u64,
    /// Bodies on the wall now.
    pub live: usize,
    /// The contact counters.
    pub contacts: Tally,
}

/// What a dropped body is.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Drop {
    Ball,
    Pill,
    Cube,
}

impl Drop {
    /// What drop `n` is.
    const fn nth(n: u64) -> Self {
        if n % PILL_EVERY == PILL_EVERY - 1 {
            Self::Pill
        } else if n % CUBE_EVERY == CUBE_EVERY - 1 {
            Self::Cube
        } else {
            Self::Ball
        }
    }
}

/// The obstacle wall.
#[derive(Debug)]
pub struct Wall {
    phys: PhysicsSystem,
    fixtures: Vec<Fixture>,
    /// The bodies on the wall, oldest first, and what each is.
    live: VecDeque<(Entity, Drop)>,
    dropped: u64,
    /// Whether a drop is due and waiting for its point to clear.
    due: bool,
    tick: u64,
    tally: Tally,
}

impl Default for Wall {
    fn default() -> Self {
        Self::new()
    }
}

/// A static capsule lying along `axis` (a unit vector), centred at `centre`.
fn bar(centre: DVec3, axis: DVec3, radius: f64, half: f64) -> (ColliderComponent, Transform) {
    // The capsule's own Y turned onto the axis: the shortest arc, as a
    // quaternion built from the two vectors without a sine.
    let turn = DQuat::from_xyzw(
        DVec3::Y.cross(axis).x,
        DVec3::Y.cross(axis).y,
        DVec3::Y.cross(axis).z,
        1.0 + DVec3::Y.dot(axis),
    )
    .normalize();
    (
        ColliderComponent::Capsule {
            offset: DVec3::ZERO,
            radius,
            half_height: half,
            is_trigger: false,
        },
        Transform::new(centre, turn),
    )
}

/// A static box.
fn slab(centre: DVec3, rotation: DQuat, half: DVec3) -> (ColliderComponent, Transform) {
    (
        ColliderComponent::Box {
            offset: DVec3::ZERO,
            half_extents: half,
            is_trigger: false,
        },
        Transform::new(centre, rotation),
    )
}

/// Every fixture on the wall, in the order they are registered.
fn fixtures() -> Vec<Fixture> {
    let mut out = Vec::new();
    let mut push = |(component, transform): (ColliderComponent, Transform),
                    material: SurfaceMaterial,
                    tint: Option<Tint>| {
        out.push(Fixture {
            component,
            transform,
            material,
            tint,
        });
    };
    let at = |x: f64, y: f64| WALL_AT + DVec3::new(x, y, 0.0);
    let half_height = 0.5 * HEIGHT;

    // The back board, the front pane and the two sides.
    push(
        slab(
            at(0.0, half_height) + DVec3::new(0.0, 0.0, -SLAB - 0.05),
            DQuat::IDENTITY,
            DVec3::new(HALF_WIDTH + 0.1, half_height, 0.05),
        ),
        PANE_SURFACE,
        Some(Tint::Board),
    );
    push(
        slab(
            at(0.0, half_height) + DVec3::new(0.0, 0.0, SLAB + 0.05),
            DQuat::IDENTITY,
            DVec3::new(HALF_WIDTH + 0.1, half_height, 0.05),
        ),
        PANE_SURFACE,
        None,
    );
    for side in [-1.0, 1.0] {
        push(
            slab(
                at(side * (HALF_WIDTH + 0.05), half_height),
                DQuat::IDENTITY,
                DVec3::new(0.05, half_height, SLAB + 0.1),
            ),
            PANE_SURFACE,
            Some(Tint::Board),
        );
    }

    // The bins' dividers.
    let mut x = -HALF_WIDTH + 0.5;
    while x < HALF_WIDTH - 0.25 {
        push(
            slab(at(x, 0.3), DQuat::IDENTITY, DVec3::new(0.02, 0.3, SLAB)),
            BIN_SURFACE,
            Some(Tint::Board),
        );
        x += 0.5;
    }

    // The pegs, end-on through the slab, in staggered rows.
    for row in 0..8 {
        let y = 1.1 + 0.42 * f64::from(row);
        let shift = if row % 2 == 0 { 0.0 } else { 0.2 };
        for column in 0..10 {
            let x = -1.8 + shift + 0.4 * f64::from(column);
            if x.abs() > HALF_WIDTH - 0.15 {
                continue;
            }
            // Two wedges stand in for pegs, one on each side.
            if (row == 3 && column == 1) || (row == 4 && column == 7) {
                push(
                    slab(
                        at(x, y),
                        rotation_from_scaled_axis(DVec3::Z * core::f64::consts::FRAC_PI_4),
                        DVec3::new(0.12, 0.12, SLAB),
                    ),
                    PEG_SURFACE,
                    Some(Tint::Peg),
                );
                continue;
            }
            push(
                bar(at(x, y), DVec3::Z, PEG_RADIUS, SLAB),
                PEG_SURFACE,
                Some(Tint::Peg),
            );
        }
    }

    // The deflector bars over the pegs, tilted in towards the middle.
    for side in [-1.0f64, 1.0] {
        let axis = DVec3::new(side, -0.35, 0.0).normalize();
        push(
            bar(at(side * 1.2, 4.75), axis, 0.04, 0.55),
            PEG_SURFACE,
            Some(Tint::Peg),
        );
    }
    out
}

impl Wall {
    /// The wall, empty, with its spawner about to start.
    #[must_use]
    pub fn new() -> Self {
        let mut phys = PhysicsSystem::with_contacts(ContactSettings::DEFAULT);
        phys.add_force_provider(Box::new(GravityForce::EARTH));
        phys.add_plane(DVec3::Y, 0.0, BIN_SURFACE);
        let fixtures = fixtures();
        for (index, fixture) in (0u32..).zip(&fixtures) {
            let e = entity(index);
            phys.set_transform(e, fixture.transform);
            phys.set_collider(e, &fixture.component, &fixture.transform);
            phys.set_material(e, fixture.material);
        }
        Self {
            phys,
            fixtures,
            live: VecDeque::new(),
            dropped: 0,
            due: false,
            tick: 0,
            tally: Tally::default(),
        }
    }

    /// Drops the next body if its drop point is clear, and says whether it
    /// did.
    fn drop_one(&mut self) -> bool {
        let n = self.dropped;
        // Ids come round again only long after the body that had one was
        // taken off the wall.
        let e = entity(FIRST_DROP + u32::try_from(n % DROP_IDS).expect("under DROP_IDS"));
        let x = (hash_unit(SEED, 2 * n) * 2.0 - 1.0) * (HALF_WIDTH - 0.3);
        let at = WALL_AT + DVec3::new(x, DROP_HEIGHT, 0.0);
        let blocked = self.live.iter().any(|(e, _)| {
            self.phys
                .transform(*e)
                .is_some_and(|t| t.position.distance(at) < DROP_CLEARANCE)
        });
        if blocked {
            return false;
        }
        let kind = Drop::nth(n);
        let turn = || rotation_from_scaled_axis(DVec3::Z * (hash_unit(SEED, 2 * n + 1) * 3.0));
        let (body, component, rotation) = match kind {
            Drop::Pill => {
                let inertia =
                    MassProperties::capsule(PILL_MASS, PILL_RADIUS, PILL_HALF, DVec3::ZERO).inertia;
                (
                    RigidBody::new_dynamic(PILL_MASS).with_inertia(inertia),
                    ColliderComponent::Capsule {
                        offset: DVec3::ZERO,
                        radius: PILL_RADIUS,
                        half_height: PILL_HALF,
                        is_trigger: false,
                    },
                    turn(),
                )
            }
            Drop::Cube => {
                let half = DVec3::splat(CUBE_HALF);
                let inertia = MassProperties::cuboid(CUBE_MASS, half, DVec3::ZERO).inertia;
                (
                    RigidBody::new_dynamic(CUBE_MASS).with_inertia(inertia),
                    ColliderComponent::Box {
                        offset: DVec3::ZERO,
                        half_extents: half,
                        is_trigger: false,
                    },
                    turn(),
                )
            }
            Drop::Ball => {
                let inertia = MassProperties::sphere(BALL_MASS, BALL_RADIUS, DVec3::ZERO).inertia;
                (
                    RigidBody::new_dynamic(BALL_MASS).with_inertia(inertia),
                    ColliderComponent::Sphere {
                        offset: DVec3::ZERO,
                        radius: BALL_RADIUS,
                        is_trigger: false,
                    },
                    DQuat::IDENTITY,
                )
            }
        };
        let transform = Transform::new(at, rotation);
        self.phys.set_body(e, body);
        self.phys.set_transform(e, transform);
        self.phys.set_collider(e, &component, &transform);
        self.phys.set_material(e, BALL_SURFACE);
        self.live.push_back((e, kind));
        self.dropped += 1;

        if self.live.len() > MAX_LIVE
            && let Some((oldest, _)) = self.live.pop_front()
        {
            self.phys.remove_entity(oldest);
        }
        true
    }

    /// Every counter, at this instant.
    #[must_use]
    pub fn reading(&self) -> WallReading {
        WallReading {
            dropped: self.dropped,
            live: self.live.len(),
            contacts: self.tally,
        }
    }

    /// The fastest any body on the wall is moving, in m/s.
    #[cfg(test)]
    pub(crate) fn fastest(&self) -> f64 {
        self.live
            .iter()
            .filter_map(|(e, _)| self.phys.body(*e))
            .map(|body| body.velocity.length())
            .fold(0.0, f64::max)
    }

    /// The deepest any live body is below the floor or outside the slab, in
    /// metres: zero if everything is where the wall should hold it.
    #[cfg(test)]
    pub(crate) fn worst_escape(&self) -> f64 {
        self.live
            .iter()
            .filter_map(|(e, _)| self.phys.transform(*e))
            .map(|t| {
                let local = t.position - WALL_AT;
                (-local.y)
                    .max(local.z.abs() - SLAB)
                    .max(local.x.abs() - HALF_WIDTH)
                    .max(0.0)
            })
            .fold(0.0, f64::max)
    }
}

impl Room for Wall {
    fn step(&mut self, dt: f64, clock: Option<&mut dyn FnMut() -> f64>) {
        self.due |= self.tick.is_multiple_of(DROP_EVERY_TICKS);
        if self.due {
            self.due = !self.drop_one();
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
        for &(e, kind) in &self.live {
            let Some(transform) = self.phys.transform(e) else {
                continue;
            };
            let key = e.to_bits();
            out.push(match kind {
                Drop::Pill => {
                    let axis = transform.rotation * DVec3::new(0.0, PILL_HALF, 0.0);
                    Shape::Capsule {
                        key,
                        a: transform.position - axis,
                        b: transform.position + axis,
                        radius: PILL_RADIUS,
                        tint: Tint::Pill,
                    }
                }
                Drop::Cube => Shape::Box {
                    key,
                    centre: transform.position,
                    rotation: transform.rotation,
                    half: DVec3::splat(CUBE_HALF),
                    tint: Tint::Box,
                },
                Drop::Ball => Shape::Sphere {
                    key,
                    centre: transform.position,
                    radius: BALL_RADIUS,
                    tint: Tint::Ball,
                },
            });
        }
    }

    fn fixtures(&self, out: &mut Vec<Shape>) {
        for (index, fixture) in (0u64..).zip(&self.fixtures) {
            let Some(tint) = fixture.tint else {
                continue;
            };
            let t = fixture.transform;
            match fixture.component {
                ColliderComponent::Box { half_extents, .. } => out.push(Shape::Box {
                    key: index,
                    centre: t.position,
                    rotation: t.rotation,
                    half: half_extents,
                    tint,
                }),
                ColliderComponent::Capsule {
                    radius,
                    half_height,
                    ..
                } => {
                    let axis = t.rotation * DVec3::new(0.0, half_height, 0.0);
                    out.push(Shape::Capsule {
                        key: index,
                        a: t.position - axis,
                        b: t.position + axis,
                        radius,
                        tint,
                    });
                }
                ColliderComponent::Sphere { radius, .. } => out.push(Shape::Sphere {
                    key: index,
                    centre: t.position,
                    radius,
                    tint,
                }),
                // Every part is a piece of the one fixture, so each carries
                // its key.
                ColliderComponent::Compound {
                    offset, ref shape, ..
                } => out.extend(shape.parts().iter().map(|part| Shape::Box {
                    key: index,
                    centre: t.position + t.rotation * (offset + part.centre),
                    rotation: t.rotation * part.rotation,
                    half: part.half_extents,
                    tint,
                })),
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::scene::tests::tick_dt;

    /// **The wall's claims over twenty seconds**: it fills to its cap and
    /// turns over, contacts begin and end, balls bounce at about the
    /// restitution they were given, nothing leaves the board, and nothing
    /// sinks into anything by more than a few centimetres even for a tick.
    ///
    /// Measured on 2026-09-23, with cubes, at tick 1200: 120 live of 155
    /// dropped, the fastest body at 6.5 m/s, nothing out of the wall, and the
    /// mean bounce 0.490 against 0.487 asked for. The worst overlap was 1.4 cm
    /// in the last tick and 6.9 cm in any tick: a cube spinning at up to
    /// 80 rad/s — over a radian a tick — turning a corner into a peg that was
    /// not its nearest feature when the tick's manifold was built. That is
    /// rotation outrunning a once-a-tick manifold, which rung 4's sweeps for
    /// fast bodies are for; on 2026-09-17, with balls and pills only, the same
    /// effect from a spinning pill peaked at 2.6 cm. Since rung 3 the bodies
    /// in the bins sleep, which changes the rest of the run, and on
    /// 2026-09-23 another cube, spinning at 43 rad/s and never asleep, turned
    /// a corner 8.2 cm into peg 68 at tick 912: the same defect, a different
    /// history, and the bound below is raised to hold it.
    #[test]
    fn the_wall_fills_turns_over_bounces_and_holds_everything() {
        let mut wall = Wall::new();
        let mut fastest = 0.0f64;
        let mut worst_escape = 0.0f64;
        for _ in 0..1200 {
            wall.step(tick_dt(), None);
            fastest = fastest.max(wall.fastest());
            worst_escape = worst_escape.max(wall.worst_escape());
        }
        let reading = wall.reading();
        let tally = reading.contacts;
        assert_eq!(reading.live, MAX_LIVE, "{reading:?}");
        assert!(reading.dropped > MAX_LIVE as u64, "{reading:?}");
        assert!(tally.begun > 1000 && tally.ended > 1000, "{tally:?}");
        let bounce = tally.bounce_ratio().expect("something bounced");
        let asked = tally.restitution().expect("something bounced");
        assert!(
            (bounce - asked).abs() < 0.05,
            "bounced {bounce} against {asked}"
        );
        assert!(
            worst_escape < 0.01,
            "a body got {worst_escape} m out of the wall"
        );
        assert!(fastest < 12.0, "a body reached {fastest} m/s");
        assert!(tally.worst_penetration < 0.02, "{tally:?}");
        assert!(tally.peak_penetration < 0.1, "{tally:?}");
    }

    /// **The wall settles to zero awake bodies once the spawner stops** —
    /// rung 3's claim for the obstacle wall, whose spawner otherwise never
    /// lets it rest: filled to its cap and left alone, everything on it comes
    /// to rest in the bins and sleeps, and none of it has left the wall.
    ///
    /// Measured on 2026-09-23: after twenty seconds of drops, the 120 bodies
    /// left alone were all asleep 326 ticks later, in eleven islands.
    #[test]
    fn the_wall_settles_to_zero_awake_bodies_once_the_spawner_stops() {
        let mut wall = Wall::new();
        for _ in 0..1200 {
            wall.step(tick_dt(), None);
        }
        let mut asleep_at = None;
        for tick in 0..1200u32 {
            wall.phys.step(tick_dt());
            if wall.phys.contact_counters().bodies == 0 {
                asleep_at = Some(tick);
                break;
            }
        }
        let counters = wall.phys.contact_counters();
        let tick = asleep_at.unwrap_or_else(|| panic!("never settled: {counters:?}"));
        assert_eq!(counters.sleeping, MAX_LIVE, "{counters:?}");
        assert!(wall.worst_escape() < 0.01);
        assert!(tick < 900, "the wall settled only after {tick} ticks");
    }
}
