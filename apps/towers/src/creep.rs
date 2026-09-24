//! `CreepSystem`: three archetypes of creep, walking [`crate::map::PATH`] as
//! kinematic bodies in the broadphase.
//!
//! ```text
//!   along += speed × slow × dt ──▶ path::point_at ──▶ PhysicsWorld::set_sphere
//!                                                            │
//!                                    exit trigger ◀── overlap_sphere ──▶ a life
//! ```
//!
//! # A creep is a sphere the game moves, not a body physics moves
//!
//! There is no integrator and no controller here. A creep's whole state is the
//! metres it has walked; every tick that number grows and the sphere is written
//! to wherever [`crate::path`] puts it. That is what
//! `docs/plan/sample/07-towers.md` means by "kinematic spline-followers in the
//! broadphase" — the body is in the world so the towers' queries can find it and
//! the bolts' sweeps can hit it, and nothing in `crcbl-phys` decides where it
//! goes.
//!
//! # Reaching the exit is an overlap and not a distance
//!
//! [`has_reached_the_exit`] asks the world whether the creep's sphere touches
//! [`crate::map::exit_collider`], rather than comparing `along` against
//! [`crate::path::length`]. The two answers are genuinely different — the
//! volume is two metres across, so it catches a creep about a metre and a half
//! before the last waypoint — and the overlap is the one the sample is for:
//! a **trigger volume** is the L0 feature (`docs/notes/simulation.md`) this map
//! exists to drive, and a distance check would be the game doing the physics'
//! job. `a_creep_is_taken_by_the_volume_before_the_path_runs_out` is what holds
//! the two apart.
//!
//! # Three archetypes, and the kind owns every number
//!
//! | | [`Kind::Fast`] | [`Kind::Tanky`] | [`Kind::Swarm`] |
//! | --- | --- | --- | --- |
//! | has | a little | a great deal | almost nothing |
//! | walks at | briskly | slowly | flat out |
//! | pays | a fair bounty | the best one | a pittance |
//! | arrives | evenly spaced | further apart | in a press |
//!
//! [`CREEPS`] is that table, one row per kind, and it is the **only** place a
//! creep's health, speed or bounty is written. A [`crate::wave::Wave`] row says
//! how many of each kind it releases and how fast it releases them, and nothing
//! else: there is no per-wave toughness multiplier, so the tenth wave's fast
//! creeps are the first wave's fast creeps and what escalates is **what a wave
//! sends**. A reviewer reading [`CREEPS`] has read every creep in the game.
//!
//! # Being slowed is recomputed every tick, not latched
//!
//! A [`crate::tower::Kind::Slow`] tower holds every creep inside its reach at a
//! fraction of its speed, and the hold **ends when the creep leaves**. So
//! [`Creep::slow`] is written from scratch once a tick: [`Creep::release`] puts
//! every creep back to full speed and the slow pass then calls
//! [`Creep::slow_to`] for each creep an overlap found, strongest hold winning.
//!
//! That is deliberately not the other two encodings. A membership **count**
//! kept by entering and leaving needs edge detection, and a decrement missed
//! once leaves a creep slowed for the rest of its life — a leak nothing would
//! ever clear. A "slowed until" **stamp** needs a timeout standing in for "now",
//! and the only honest timeout is one tick, which is this recomputation with a
//! clock bolted to it. Recomputing cannot leak and cannot go stale:
//! `a_creep_that_walks_out_of_a_slow_field_gets_its_speed_back` is the claim,
//! and `crate::game`'s own slow tests are it end to end.

use crcbl::math::DVec3;
use crcbl::phys::{ColliderId, PhysicsWorld, Sphere};

use crate::map::CREEP_RADIUS;
use crate::path;

// ---------------------------------------------------------------------------
// The table
// ---------------------------------------------------------------------------

/// How many kinds of creep there are.
pub const KINDS: usize = 3;

/// One row of [`CREEPS`]: everything one archetype is.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct CreepSpec {
    /// What the overlay, the `[HUD]` line and a failing test call it.
    pub label: &'static str,
    /// What it has, in hit points.
    pub health: u32,
    /// How fast it walks, in metres a second.
    pub speed: f64,
    /// What killing it pays, in gold.
    pub bounty: u32,
    /// What this kind does to the gap a wave releases at.
    ///
    /// The wave's [`crate::wave::Wave::spacing_s`] times this is how long after
    /// one of these is released before the next creep is. Under one for a
    /// [`Kind::Swarm`], which is what "released in bulk" means; over one for a
    /// [`Kind::Tanky`], which lumbers in on its own.
    pub spacing_scale: f64,
}

/// The three archetypes, in the order [`CREEPS`] rows them and a
/// [`crate::wave::Wave`]'s mix counts them.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Kind {
    /// The one slice 1 shipped: brisk, thin, and what a bolt tower is priced
    /// against.
    Fast,
    /// Slow and very hard to chew through. The kind a field with no splash and
    /// no hold has to out-damage one shot at a time.
    Tanky,
    /// Almost no health, the quickest thing on the field, and released in a
    /// press — the kind a single-target tower cannot keep up with.
    Swarm,
}

/// Every archetype, in the order [`Kind`] declares them.
pub const ALL: [Kind; KINDS] = [Kind::Fast, Kind::Tanky, Kind::Swarm];

/// What each archetype is. Indexed by [`Kind::index`].
///
/// `the_kinds_and_their_rows_are_in_the_same_order` is what holds the enum and
/// this table together, and `every_kind_is_distinguishable_from_the_others`
/// asserts that the three rows are genuinely three archetypes rather than three
/// labels on one.
pub const CREEPS: [CreepSpec; KINDS] = [
    CreepSpec {
        label: "fast",
        health: 40,
        speed: 6.0,
        bounty: 12,
        spacing_scale: 1.0,
    },
    CreepSpec {
        label: "tanky",
        health: 600,
        speed: 3.6,
        bounty: 60,
        spacing_scale: 1.5,
    },
    CreepSpec {
        label: "swarm",
        health: 18,
        speed: 7.5,
        bounty: 5,
        spacing_scale: 0.35,
    },
];

impl Kind {
    /// Which row of [`CREEPS`] this kind is.
    #[must_use]
    pub const fn index(self) -> usize {
        self as usize
    }

    /// What this kind is.
    #[must_use]
    pub const fn spec(self) -> &'static CreepSpec {
        &CREEPS[self.index()]
    }

    /// What the overlay, the `[HUD]` line and a failing test call it.
    #[must_use]
    pub const fn label(self) -> &'static str {
        self.spec().label
    }
}

// ---------------------------------------------------------------------------
// The creep
// ---------------------------------------------------------------------------

/// One creep.
#[derive(Debug)]
pub struct Creep {
    /// Its sphere in the world — what a tower's overlap finds and what a bolt's
    /// sweep hits.
    body: ColliderId,
    /// How far it has walked, in metres along [`crate::map::PATH`]. **The whole
    /// of its position**: see [`crate::path`].
    along: f64,
    /// Which archetype it is, and therefore every number it walks and dies on.
    kind: Kind,
    /// What it has left, and what it started with.
    health: u32,
    max_health: u32,
    /// What fraction of its speed it is walking at this tick: one for a free
    /// creep, less for one inside a [`crate::tower::Kind::Slow`] tower's reach.
    ///
    /// Rewritten every tick from the towers' own overlaps — see the module docs
    /// for why it is recomputed rather than latched.
    slow: f64,
}

/// Where a creep with `along` metres behind it has its centre.
///
/// A free function because the frame wants it for a creep it does not hold and
/// [`Creep::centre`] wants it for one it does.
#[must_use]
pub fn centre_at(along: f64) -> DVec3 {
    path::point_at(along) + DVec3::Y * CREEP_RADIUS
}

impl Creep {
    /// Puts a creep of `kind` on the first waypoint, with its sphere in `world`.
    #[must_use]
    pub fn spawn(world: &mut PhysicsWorld, kind: Kind) -> Self {
        let body = world.add_sphere(Sphere::new(centre_at(0.0), CREEP_RADIUS));
        Self {
            body,
            along: 0.0,
            kind,
            health: kind.spec().health,
            max_health: kind.spec().health,
            slow: 1.0,
        }
    }

    /// Its sphere, for a query's answer to be matched against.
    #[must_use]
    pub const fn body(&self) -> ColliderId {
        self.body
    }

    /// Which archetype it is.
    #[must_use]
    pub const fn kind(&self) -> Kind {
        self.kind
    }

    /// How far it has walked, in metres.
    ///
    /// **What a tower picks its target by**: the creep nearest the exit is the
    /// one with the most to lose, which is the rule every tower defense uses.
    #[must_use]
    pub const fn along(&self) -> f64 {
        self.along
    }

    /// What killing it pays.
    #[must_use]
    pub const fn bounty(&self) -> u32 {
        self.kind.spec().bounty
    }

    /// What it has left.
    #[must_use]
    pub const fn health(&self) -> u32 {
        self.health
    }

    /// What fraction of its speed it is walking at this tick.
    #[must_use]
    pub const fn slow(&self) -> f64 {
        self.slow
    }

    /// Whether anything is holding it this tick.
    #[must_use]
    pub fn is_slowed(&self) -> bool {
        self.slow < 1.0
    }

    /// Lets go of whatever was holding it, ready for this tick's slow pass.
    ///
    /// Called on every creep before the pass rather than on the ones that left
    /// a tower's reach, because "left" is the thing nothing can observe: see
    /// the module docs.
    pub const fn release(&mut self) {
        self.slow = 1.0;
    }

    /// Holds it at `factor` of its speed, if nothing is holding it harder.
    ///
    /// The strongest hold wins, so two slow towers covering one stretch do not
    /// multiply into a creep that has stopped.
    pub const fn slow_to(&mut self, factor: f64) {
        if factor < self.slow {
            self.slow = factor;
        }
    }

    /// Where its centre is, in metres.
    #[must_use]
    pub fn centre(&self) -> DVec3 {
        centre_at(self.along)
    }

    /// Walks one tick and writes the sphere where the walk left it.
    ///
    /// The write is not optional and not deferred: the towers' queries and the
    /// bolts' sweeps run later in the same tick against this world, and a body
    /// left at last tick's place is a target that cannot be hit where it is
    /// drawn.
    pub fn advance(&mut self, world: &mut PhysicsWorld, dt: f64) {
        self.along += self.kind.spec().speed * self.slow * dt;
        world.set_sphere(self.body, Sphere::new(self.centre(), CREEP_RADIUS));
    }

    /// Takes `damage` off it. Answers whether that killed it.
    pub fn wounded(&mut self, damage: u32) -> bool {
        self.health = self.health.saturating_sub(damage);
        self.health == 0
    }

    /// What the frame draws of it.
    #[must_use]
    pub fn view(&self) -> CreepView {
        CreepView {
            kind: self.kind,
            centre: self.centre(),
            facing: {
                #[allow(clippy::cast_possible_truncation)]
                let facing = path::heading_at(self.along) as f32;
                facing
            },
            hurt: 2 * self.health <= self.max_health,
            slowed: self.is_slowed(),
        }
    }

    /// Takes its sphere out of the world.
    ///
    /// Consuming, because a creep without a body is not a creep: the id would
    /// go on resolving to whatever collider recycled its slot, which is the one
    /// mistake `ColliderId`'s generation exists to make impossible.
    pub fn despawn(self, world: &mut PhysicsWorld) {
        world.remove(self.body);
    }
}

/// Where one creep is drawn, and how.
///
/// The frame's copy of a [`Creep`], snapshotted with the rest of
/// [`crate::game::RenderState`] so a draw never reads through the tick's lock.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct CreepView {
    /// Which archetype it is — what picks its material. See
    /// [`crate::map::creep_material`].
    pub kind: Kind,
    /// Where its centre is, in metres.
    pub centre: DVec3,
    /// Which way it is walking, in [`crate::path::heading_at`]'s measure.
    pub facing: f32,
    /// Whether it is down to half its health or less.
    pub hurt: bool,
    /// Whether a slow tower is holding it this tick.
    pub slowed: bool,
}

impl Default for CreepView {
    /// A parked slot: the first archetype, at the origin, whole and free.
    ///
    /// Written out rather than derived because [`Kind`] has no default and
    /// should not have one — a creep is one of three archetypes by construction,
    /// and a default kind would be a fourth state the table has no row for.
    fn default() -> Self {
        Self {
            kind: Kind::Fast,
            centre: DVec3::ZERO,
            facing: 0.0,
            hurt: false,
            slowed: false,
        }
    }
}

/// Whether `creep` is standing in the exit volume.
///
/// `scratch` is the caller's so the query allocates nothing: this runs once per
/// creep per tick, which is `apps/horde`'s reason for hoisting the same buffer
/// out of the same loop.
#[must_use]
pub fn has_reached_the_exit(
    world: &mut PhysicsWorld,
    creep: &Creep,
    exit: ColliderId,
    scratch: &mut Vec<ColliderId>,
) -> bool {
    world.overlap_sphere_into(creep.centre(), CREEP_RADIUS, scratch);
    scratch.contains(&exit)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::map;

    /// One tick at sixty a second.
    const DT: f64 = 1.0 / 60.0;

    /// **The enum and the table are in the same order**, which is what makes
    /// [`Kind::index`] a row number rather than a hope.
    ///
    /// Without it a reordered enum would hand every creep another archetype's
    /// health and bounty, and every test below would go on passing because each
    /// of them asks the kind what it is.
    #[test]
    fn the_kinds_and_their_rows_are_in_the_same_order() {
        for (row, kind) in ALL.iter().enumerate() {
            assert_eq!(kind.index(), row, "{} is not row {row}", kind.label());
            assert_eq!(
                kind.spec(),
                &CREEPS[row],
                "{} does not read its own row",
                kind.label(),
            );
        }
        assert_eq!(
            ALL.len(),
            CREEPS.len(),
            "a kind has no row, or a row no kind"
        );
    }

    /// **Each archetype is actually a different archetype.** Three rows that
    /// happened to carry the same numbers would be one creep with three labels,
    /// and every mix in `crate::wave::WAVES` would be decoration.
    ///
    /// The ordering is the design, stated as the assertion: a tanky creep has
    /// the most health, walks slowest and pays best; a swarm creep has the
    /// least, walks fastest and pays least and arrives in the tightest press.
    #[test]
    fn every_kind_is_distinguishable_from_the_others() {
        let (fast, tanky, swarm) = (Kind::Fast.spec(), Kind::Tanky.spec(), Kind::Swarm.spec());
        assert!(
            tanky.health > fast.health && fast.health > swarm.health,
            "the health ordering is not tanky > fast > swarm",
        );
        assert!(
            swarm.speed > fast.speed && fast.speed > tanky.speed,
            "the speed ordering is not swarm > fast > tanky",
        );
        assert!(
            tanky.bounty > fast.bounty && fast.bounty > swarm.bounty,
            "the bounty ordering is not tanky > fast > swarm",
        );
        assert!(
            swarm.spacing_scale < fast.spacing_scale && fast.spacing_scale < tanky.spacing_scale,
            "a swarm does not arrive in a tighter press than a tanky creep",
        );
        // Every label distinct, because the `[HUD]` line and the overlay name a
        // creep by it and two kinds called the same thing are unreadable.
        for (at, kind) in ALL.iter().enumerate() {
            for other in &ALL[at + 1..] {
                assert_ne!(kind.label(), other.label(), "two kinds share a label");
            }
        }
    }

    /// **A creep walks at its kind's speed and drags its collider with it.**
    /// The second half is the one worth asserting: a follower that moved the
    /// number and not the sphere leaves every tower shooting at where the creep
    /// was on the tick it spawned.
    ///
    /// All three kinds, because the speed is the kind's and a build that read
    /// one row for all of them would pass on the first.
    #[test]
    fn a_creep_walks_its_speed_and_its_body_follows() {
        for kind in ALL {
            let (mut world, _) = map::world();
            let mut creep = Creep::spawn(&mut world, kind);
            let start = creep.centre();

            for _ in 0..60 {
                creep.advance(&mut world, DT);
            }
            let walked = (creep.centre() - start).length();
            assert!(
                (walked - kind.spec().speed).abs() < 0.05,
                "a second of walking covered {walked:.2} m at {} m/s for a {}",
                kind.spec().speed,
                kind.label(),
            );
            assert!(
                world
                    .overlap_sphere(creep.centre(), 0.01)
                    .contains(&creep.body()),
                "the collider was left behind at the spawn",
            );
            assert!(
                !world.overlap_sphere(start, 0.01).contains(&creep.body()),
                "the collider is in two places, so `set_sphere` added rather than moved",
            );
        }
    }

    /// **A held creep walks slower, and gets its speed back the moment nothing
    /// is holding it.**
    ///
    /// The second half is the claim the whole encoding exists for — see the
    /// module docs. A latched "slowed until" or a leaked membership count passes
    /// the first assertion and fails this one, which is exactly the failure a
    /// slow tower a creep has walked out of would produce.
    #[test]
    fn a_creep_that_walks_out_of_a_slow_field_gets_its_speed_back() {
        const FACTOR: f64 = 0.5;
        let (mut world, _) = map::world();
        let mut creep = Creep::spawn(&mut world, Kind::Fast);

        // A second held, then a second free, measured over the same stretch of
        // the first leg so the path itself cannot be what differs.
        creep.slow_to(FACTOR);
        assert!(creep.is_slowed(), "a creep told to slow is not slowed");
        let start = creep.along();
        for _ in 0..60 {
            creep.advance(&mut world, DT);
        }
        let held = creep.along() - start;

        creep.release();
        assert!(!creep.is_slowed(), "a released creep is still held");
        let from = creep.along();
        for _ in 0..60 {
            creep.advance(&mut world, DT);
        }
        let free = creep.along() - from;

        assert!(
            (held - FACTOR * Kind::Fast.spec().speed).abs() < 0.05,
            "a held creep covered {held:.2} m at {FACTOR} of {} m/s",
            Kind::Fast.spec().speed,
        );
        assert!(
            (free - Kind::Fast.spec().speed).abs() < 0.05,
            "a released creep covered {free:.2} m rather than its full {} m",
            Kind::Fast.spec().speed,
        );
        assert!(
            free > held * 1.5,
            "being held cost {held:.2} m against {free:.2} m free, which is no hold at all",
        );
    }

    /// **The strongest hold wins and nothing multiplies.** Two slow towers over
    /// one stretch of lane must not stop a creep dead, which is what compounding
    /// the factors would do.
    #[test]
    fn two_holds_on_one_creep_do_not_multiply() {
        let (mut world, _) = map::world();
        let mut creep = Creep::spawn(&mut world, Kind::Fast);
        creep.slow_to(0.6);
        creep.slow_to(0.4);
        creep.slow_to(0.8);
        assert!(
            (creep.slow() - 0.4).abs() < f64::EPSILON,
            "three holds left it at {}, not the strongest of them",
            creep.slow(),
        );
    }

    /// **The exit volume is what takes a creep, and it takes it before the path
    /// runs out.** The margin is the whole point: a build that compared `along`
    /// against `path::length()` would agree with this test at the last waypoint
    /// and disagree everywhere the volume reaches, which is a metre and a half
    /// of the last leg.
    #[test]
    fn a_creep_is_taken_by_the_volume_before_the_path_runs_out() {
        let (mut world, exit) = map::world();
        let mut creep = Creep::spawn(&mut world, Kind::Fast);
        let mut scratch = Vec::new();

        assert!(
            !has_reached_the_exit(&mut world, &creep, exit, &mut scratch),
            "a creep on the spawn is already in the exit",
        );

        let mut caught = None;
        for _ in 0..(60 * 60) {
            creep.advance(&mut world, DT);
            if has_reached_the_exit(&mut world, &creep, exit, &mut scratch) {
                caught = Some(creep.along());
                break;
            }
        }
        let caught = caught.expect("a creep walking the whole path never reached the exit");
        let left = path::length() - caught;
        assert!(
            left > 0.5,
            "the volume caught it {left:.2} m from the end, which is the end rather than the \
             volume",
        );
        assert!(
            left < map::EXIT_HALF.x + CREEP_RADIUS + 0.2,
            "it was caught {left:.2} m out, further than the volume reaches",
        );
    }

    /// **Damage runs out at zero and not below it**, and a creep is drawn hurt
    /// once it is down to half. Per kind, because the pool is the kind's.
    #[test]
    fn a_creep_dies_when_its_health_runs_out() {
        for kind in ALL {
            let (mut world, _) = map::world();
            let mut creep = Creep::spawn(&mut world, kind);
            let half = kind.spec().health / 2;

            assert!(!creep.view().hurt, "a {} spawns hurt", kind.label());
            assert!(
                !creep.wounded(half),
                "half a {}'s health killed it",
                kind.label(),
            );
            assert!(
                creep.view().hurt,
                "half a {}'s health is not drawn hurt",
                kind.label(),
            );
            assert!(
                creep.wounded(kind.spec().health * 4),
                "an overkill did not kill a {}",
                kind.label(),
            );
            assert_eq!(creep.health(), 0, "health went below zero");
        }
    }

    /// **The picture says which kind a creep is, and whether it is held.** The
    /// view is the only thing the frame sees, so a field that never reached it
    /// is a state a reviewer cannot check the readout against.
    #[test]
    fn the_view_carries_the_kind_and_the_hold() {
        let (mut world, _) = map::world();
        let mut creep = Creep::spawn(&mut world, Kind::Tanky);
        let free = creep.view();
        assert_eq!(free.kind, Kind::Tanky, "the view forgot the archetype");
        assert!(!free.slowed, "a free creep is drawn held");

        creep.slow_to(0.5);
        assert!(creep.view().slowed, "a held creep is not drawn held");
        assert_eq!(creep.view().kind, Kind::Tanky);
    }

    /// **A despawned creep's body leaves the world**, which is what stops a
    /// dead creep going on being a thing towers acquire and bolts stop at.
    #[test]
    fn a_despawned_creep_is_no_longer_in_the_world() {
        let (mut world, _) = map::world();
        let creep = Creep::spawn(&mut world, Kind::Fast);
        let (body, centre) = (creep.body(), creep.centre());
        assert!(world.overlap_sphere(centre, 0.01).contains(&body));

        creep.despawn(&mut world);
        assert!(
            !world.overlap_sphere(centre, 0.01).contains(&body),
            "the collider outlived the creep",
        );
    }
}
