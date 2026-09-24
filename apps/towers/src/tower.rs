//! `TowerSystem` and `ProjectileSystem`: three kinds of tower, acquisition by
//! sphere overlap, a swept bolt against a creep that is moving in the same
//! tick, and a burst at the point it lands.
//!
//! ```text
//!   muzzle ──▶ PhysicsWorld::overlap_sphere ──▶ ids ──filter──▶ the creep
//!                                                     nearest the exit
//!                                                          │
//!   Bolt ── Segment(at, at + heading × speed × dt) ──▶ sweep_sphere ──▶ damage
//!                                                          │
//!                   impact point ──▶ overlap_sphere ──▶ every creep in the burst
//!
//!   a Slow tower: muzzle ──▶ overlap_sphere ──▶ Creep::slow_to, every tick
//! ```
//!
//! # Three kinds, and [`TOWERS`] is every number any of them has
//!
//! | | [`Kind::Bolt`] | [`Kind::Splash`] | [`Kind::Slow`] |
//! | --- | --- | --- | --- |
//! | does | one creep at a time | everything in a burst | no damage at all |
//! | costs | least | most | middling |
//! | answers | a tanky creep | a swarm | both, by buying the others time |
//!
//! A kind at a tier is a **row of a table** rather than a match arm in each of
//! a dozen methods: [`TOWERS`] is indexed by [`Kind::index`] and
//! [`Tier::index`], and [`Kind::spec`] is the only way anything in this crate
//! reads a tower's reach, damage, reload or price. A new kind is a row and a
//! variant, which is the extensibility `docs/plan/sample/07-towers.md`'s exit
//! criteria ask for.
//!
//! **A kind either fires or holds, never both**, and the table says which:
//! [`TowerSpec::fires`] is a damage above zero and [`TowerSpec::slows`] is a
//! speed fraction below one. `crate::game`'s tick puts a tower in exactly one of
//! its two passes on that reading, and
//! `every_row_either_fires_or_holds_and_never_both` is what stops a row falling
//! into neither — which would be a tower a player can buy and that does nothing
//! at all.
//!
//! # Both halves of a shot are `crcbl-phys` queries, and rule 9 has no exemption
//!
//! Nothing in this file works out whether two things are touching. Acquisition
//! is [`PhysicsWorld::overlap_sphere`], a hit is
//! [`PhysicsWorld::sweep_sphere`], a splash burst is the overlap again at the
//! impact point, and a slow tower's reach is the overlap once a tick; what this
//! module decides is **where from**, **which way** and **what an answer means**.
//!
//! # An overlap answers with everything, which is the point of the filter
//!
//! [`PhysicsWorld::overlap_sphere`] reports triggers as well as solids — that is
//! the query they exist for — and the ground slab is inside every tower's
//! range. So [`acquire`] hands back a creep index or nothing, and a build that
//! took the query's first answer would have every tower shooting at the floor.
//! `apps/horde` carries the same note about the same query, and
//! `a_tower_ignores_everything_in_range_that_is_not_a_creep` asserts the
//! overlap really does return the other things. [`hold`] and [`burst_into`]
//! filter the same way and for the same reason.
//!
//! # The sweep is the whole reason a bolt is a bolt
//!
//! [`BOLT_SPEED`] is fast enough that one tick's travel is longer than a creep
//! is wide: a test that asked "is the bolt inside a creep?" at the start of the
//! tick and again at the end would answer no both times, on a tick the bolt
//! passed clean through one. Time of impact against moving targets is the
//! physics slice towers drives (`docs/notes/simulation.md`), and
//! `a_bolt_hits_a_creep_that_a_test_at_either_end_of_the_tick_would_miss` is
//! that claim made against a creep that is **also moving** — the creep's sphere
//! is written by [`crate::creep::Creep::advance`] earlier in the same tick, so
//! the sweep is against where the target is now rather than where it was.
//!
//! One speed for every kind, and that is deliberate: what makes a
//! [`Kind::Splash`] tower slower is its reload, not a bolt that loiters. The
//! single speed is also what keeps [`crate::map::MAX_BOLTS`]' argument one
//! sentence long — see [`SHORTEST_RELOAD_S`].
//!
//! # A bolt homes, and that is a design decision rather than a shortcut
//!
//! A tower defense's single-target tower hits what it shot at; leading a target
//! is a skill a player has and a tower does not. So [`Bolt::step`] turns toward
//! its target's current centre every tick.
//!
//! **A bolt whose target died before it landed keeps its last heading and stops
//! in the ground**, which is [`BoltOutcome::Spent`]. There is no separate
//! lifetime on a bolt and it does not need one: [`MUZZLE_Y`] stands above a
//! creep's centre, so every bolt this sample fires is descending and the ground
//! slab is always in front of it. A timer beside that would be a guard nothing
//! could ever trip.
//!
//! **A splash bolt bursts wherever it stops**, a creep or the ground, because
//! both are an impact and the burst is what the kind is for. `crate::game`'s
//! `Stage::splash` is the caller, and it is what keeps the directly struck creep
//! from being wounded twice.

use crcbl::math::DVec3;
use crcbl::phys::{ColliderId, PhysicsWorld, Segment};

use crate::creep::Creep;
use crate::map::{BOLT_RADIUS, MUZZLE_Y, PLOTS};

// ---------------------------------------------------------------------------
// The table
// ---------------------------------------------------------------------------

/// How many kinds of tower there are.
pub const KINDS: usize = 3;

/// How many tiers a kind has: what it is built as, and the one upgrade
/// `docs/plan/sample/07-towers.md` asks for.
pub const TIERS: usize = 2;

/// One row of [`TOWERS`]: everything one kind is at one tier.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct TowerSpec {
    /// How far it reaches, in metres, measured from its muzzle.
    pub range_m: f64,
    /// What one shot takes off a creep, in hit points. **Zero for a kind with
    /// no projectile** — see [`TowerSpec::fires`].
    pub damage: u32,
    /// How long between one shot and the next, in seconds. Zero for a kind that
    /// does not shoot: a hold is continuous and there is nothing to reload.
    pub reload_s: f64,
    /// What this row costs in gold: **building** it, on the [`Tier::Base`] row,
    /// and **stepping up to** it on the [`Tier::Upgraded`] one. A fully
    /// upgraded tower has therefore cost the two added together.
    pub cost: u32,
    /// How far a burst at the impact point wounds, in metres. Zero for a kind
    /// whose shot wounds only what it struck.
    pub burst_m: f64,
    /// What fraction of its speed a creep inside this tower's reach walks at.
    /// One for a kind that holds nothing.
    pub slow_factor: f64,
}

impl TowerSpec {
    /// Whether this row shoots at all.
    ///
    /// What puts a tower in `crate::game`'s projectile pass. A row with no
    /// damage is a [`Kind::Slow`] tower, whose whole effect is [`hold`].
    #[must_use]
    pub const fn fires(&self) -> bool {
        self.damage > 0
    }

    /// Whether this row's shot bursts at the point it lands.
    #[must_use]
    pub const fn bursts(&self) -> bool {
        self.burst_m > 0.0
    }

    /// Whether this row holds what is inside its reach.
    ///
    /// What puts a tower in `crate::game`'s slow pass.
    #[must_use]
    pub const fn slows(&self) -> bool {
        self.slow_factor < 1.0
    }
}

/// Which kind of tower, in the order [`TOWERS`] rows them and the `1`/`2`/`3`
/// keys pick them.
///
/// [`Kind::Bolt`] is the default because it is what the build list opens on: the
/// cheapest kind, the only one slice 1 had, and the one a player who has pressed
/// no kind key yet is asking for. A `Controls` frame carries a kind on every
/// tick — see [`crate::game::Controls::kind`] — so there has to be one.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum Kind {
    /// The one slice 1 shipped: a swept bolt at one creep, cheap and quick.
    #[default]
    Bolt,
    /// A slower, dearer bolt that bursts where it lands.
    Splash,
    /// No projectile at all: everything inside its reach walks slower while it
    /// is inside.
    Slow,
}

/// Every kind, in the order [`Kind`] declares them.
pub const ALL: [Kind; KINDS] = [Kind::Bolt, Kind::Splash, Kind::Slow];

/// Which tier a built tower is at.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum Tier {
    /// What `PlaceTower` builds.
    #[default]
    Base,
    /// What `UpgradeTower` steps it up to, once.
    Upgraded,
}

impl Tier {
    /// Which column of [`TOWERS`] this tier is.
    #[must_use]
    pub const fn index(self) -> usize {
        self as usize
    }

    /// What the overlay and the `[HUD]` line call it.
    #[must_use]
    pub const fn label(self) -> &'static str {
        match self {
            Self::Base => "base",
            Self::Upgraded => "upgraded",
        }
    }
}

/// What each kind is, at each tier: `TOWERS[kind][tier]`.
///
/// The second column is the upgrade, and every number in it is the first
/// column's made better — `an_upgrade_is_dearer_and_better_than_what_it_replaces`
/// is that read as an assertion, so a row typed in the wrong order is a red test
/// rather than a purchase that makes a tower worse.
pub const TOWERS: [[TowerSpec; TIERS]; KINDS] = [
    [
        TowerSpec {
            range_m: 7.0,
            damage: 18,
            reload_s: 0.35,
            cost: 40,
            burst_m: 0.0,
            slow_factor: 1.0,
        },
        TowerSpec {
            range_m: 8.0,
            damage: 30,
            reload_s: 0.30,
            cost: 50,
            burst_m: 0.0,
            slow_factor: 1.0,
        },
    ],
    [
        TowerSpec {
            range_m: 6.5,
            damage: 20,
            reload_s: 0.85,
            cost: 70,
            burst_m: 2.5,
            slow_factor: 1.0,
        },
        TowerSpec {
            range_m: 7.5,
            damage: 32,
            reload_s: 0.70,
            cost: 80,
            burst_m: 3.5,
            slow_factor: 1.0,
        },
    ],
    [
        TowerSpec {
            range_m: 8.0,
            damage: 0,
            reload_s: 0.0,
            cost: 50,
            burst_m: 0.0,
            slow_factor: 0.50,
        },
        TowerSpec {
            range_m: 10.0,
            damage: 0,
            reload_s: 0.0,
            cost: 60,
            burst_m: 0.0,
            slow_factor: 0.30,
        },
    ],
];

impl Kind {
    /// Which row of [`TOWERS`] this kind is.
    #[must_use]
    pub const fn index(self) -> usize {
        self as usize
    }

    /// The kind a `PlaceTower` command's kind byte names, or `None` for a byte
    /// no kind has.
    ///
    /// **A format question rather than a rules one**, which is why it is here
    /// and not in `crate::game`'s validation: a plot number outside [`PLOTS`]
    /// is a thing the rules turn down, and a kind byte outside this table is a
    /// frame no build of this game wrote.
    #[must_use]
    pub const fn from_index(index: u8) -> Option<Self> {
        if (index as usize) < KINDS {
            Some(ALL[index as usize])
        } else {
            None
        }
    }

    /// What this kind is at `tier`.
    #[must_use]
    pub const fn spec(self, tier: Tier) -> &'static TowerSpec {
        &TOWERS[self.index()][tier.index()]
    }

    /// What the overlay, the `[HUD]` line and a failing test call it.
    #[must_use]
    pub const fn label(self) -> &'static str {
        match self {
            Self::Bolt => "bolt",
            Self::Splash => "splash",
            Self::Slow => "slow",
        }
    }
}

/// How long a tower is drawn hot after firing — or, for a [`Kind::Slow`] tower,
/// after holding something — in seconds.
///
/// Long enough to be seen at sixty frames a second and short enough that a
/// tower between shots is plainly between shots — the picture is how a reviewer
/// tells a tower that is working from one that has nothing in range.
pub const FLASH_S: f64 = 0.12;

/// How long a splash burst is drawn, in seconds.
///
/// Shorter than [`FLASH_S`], because the burst is the loudest thing on the field
/// and a lingering one would read as a permanent feature of the map. Under every
/// bursting row's reload, which is what makes [`crate::map::MAX_BURSTS`] one
/// slot per plot — `a_burst_is_gone_before_its_tower_can_raise_another` asserts
/// it.
pub const BURST_S: f64 = 0.09;

/// How fast a bolt travels, in metres a second.
///
/// **Chosen so that one tick's travel is longer than a creep is wide**, which
/// is what makes the sweep load-bearing rather than decorative — see the module
/// docs, and `a_tick_of_a_bolt_is_longer_than_a_creep_is_wide`, which asserts
/// the inequality against the simulation rate.
pub const BOLT_SPEED: f64 = 120.0;

/// The shortest reach anything in [`TOWERS`] has, in metres.
///
/// What `crate::map`'s `every_plot_stands_clear_of_the_lane_and_still_covers_it`
/// holds the plots to: a plot the *shortest-reaching* kind cannot cover from is
/// a plot that kind may not be built on, and the build list offers every kind on
/// every plot.
pub const SHORTEST_RANGE_M: f64 = {
    let mut shortest = f64::INFINITY;
    let mut kind = 0;
    while kind < KINDS {
        let mut tier = 0;
        while tier < TIERS {
            if TOWERS[kind][tier].range_m < shortest {
                shortest = TOWERS[kind][tier].range_m;
            }
            tier += 1;
        }
        kind += 1;
    }
    shortest
};

/// The shortest reload any shooting row in [`TOWERS`] has, in seconds.
///
/// **What [`crate::map::MAX_BOLTS`] rests on.** A bolt lands inside the reload
/// of the quickest tower that could have fired it, so no tower ever has two out
/// at once and a full field has one each —
/// `a_bolt_lands_long_before_its_tower_reloads` measures the longest flight
/// there is against this number.
pub const SHORTEST_RELOAD_S: f64 = {
    let mut shortest = f64::INFINITY;
    let mut kind = 0;
    while kind < KINDS {
        let mut tier = 0;
        while tier < TIERS {
            let spec = &TOWERS[kind][tier];
            if spec.fires() && spec.reload_s < shortest {
                shortest = spec.reload_s;
            }
            tier += 1;
        }
        kind += 1;
    }
    shortest
};

// ---------------------------------------------------------------------------
// The tower
// ---------------------------------------------------------------------------

/// One built tower.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Tower {
    /// Which of [`PLOTS`] it stands on.
    plot: usize,
    /// Which kind it is — and therefore which row of [`TOWERS`] it reads.
    kind: Kind,
    /// Which column of that row: what it was built as, or the one upgrade.
    tier: Tier,
    /// When it may fire again, in the stage's elapsed seconds.
    ready_at: f64,
    /// When it last did something, in the same seconds — a shot for a kind that
    /// fires, a creep held for one that does not. Read by the frame and by
    /// nothing in the simulation.
    fired_at: f64,
}

impl Tower {
    /// Builds a `kind` tower on `plot`, able to fire at once.
    #[must_use]
    pub const fn new(plot: usize, kind: Kind) -> Self {
        Self {
            plot,
            kind,
            tier: Tier::Base,
            ready_at: 0.0,
            fired_at: f64::NEG_INFINITY,
        }
    }

    /// Which plot it stands on.
    #[must_use]
    pub const fn plot(&self) -> usize {
        self.plot
    }

    /// Which kind it is.
    #[must_use]
    pub const fn kind(&self) -> Kind {
        self.kind
    }

    /// Which tier it is at.
    #[must_use]
    pub const fn tier(&self) -> Tier {
        self.tier
    }

    /// What it is, this tier: the row of [`TOWERS`] every number comes off.
    #[must_use]
    pub const fn spec(&self) -> &'static TowerSpec {
        self.kind.spec(self.tier)
    }

    /// Steps it up a tier. Answers whether there was a tier to step up to.
    ///
    /// **The rule, and nothing else**: the purse is `crate::game`'s to check and
    /// the plot is its to find. What this owns is that there is exactly one
    /// upgrade — a second `UpgradeTower` on the same plot is refused here, which
    /// is the refusal `an_upgrade_is_taken_once_and_then_turned_down` holds.
    pub const fn upgrade(&mut self) -> bool {
        match self.tier {
            Tier::Base => {
                self.tier = Tier::Upgraded;
                true
            }
            Tier::Upgraded => false,
        }
    }

    /// What stepping it up would cost, or `None` for a tower already at the top.
    #[must_use]
    pub const fn upgrade_cost(&self) -> Option<u32> {
        match self.tier {
            Tier::Base => Some(self.kind.spec(Tier::Upgraded).cost),
            Tier::Upgraded => None,
        }
    }

    /// Where its bolts start, in metres — and where a [`Kind::Slow`] tower's
    /// reach is measured from.
    ///
    /// # Panics
    ///
    /// If its plot is not a plot, which `crate::game`'s validation makes
    /// unreachable: a `PlaceTower` naming a plot outside [`PLOTS`] is refused
    /// before a tower is built.
    #[must_use]
    pub fn muzzle(&self) -> DVec3 {
        let at = PLOTS[self.plot];
        DVec3::new(at.x, MUZZLE_Y, at.z)
    }

    /// Whether its reload has finished. Meaningless for a kind that does not
    /// fire, and `crate::game` never asks one.
    #[must_use]
    pub fn is_ready(&self, now: f64) -> bool {
        now >= self.ready_at
    }

    /// Records a shot — or, for a [`Kind::Slow`] tower, a tick on which it held
    /// something — and starts the reload.
    pub fn fired(&mut self, now: f64) {
        self.ready_at = now + self.spec().reload_s;
        self.fired_at = now;
    }

    /// Whether the frame draws it hot.
    #[must_use]
    pub fn is_firing(&self, now: f64) -> bool {
        now - self.fired_at < FLASH_S
    }
}

/// What the frame draws of one built tower.
///
/// The frame's copy of a [`Tower`], snapshotted with the rest of
/// [`crate::game::RenderState`] so a draw never reads through the tick's lock.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct TowerView {
    /// Which kind it is — what picks its material. See
    /// [`crate::map::tower_material`].
    pub kind: Kind,
    /// Which tier it is at — what makes it stand taller. See
    /// [`crate::map::UPGRADED_SCALE`].
    pub tier: Tier,
    /// Whether it did something this instant: fired, for a kind that fires, or
    /// held a creep, for one that does not. See [`Tower::is_firing`].
    pub working: bool,
}

/// What the frame draws of one splash burst.
///
/// A sphere the size of the burst that raised it, drawn for [`BURST_S`] and then
/// parked — see [`crate::map::MAX_BURSTS`].
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct BurstView {
    /// Where the bolt stopped, in metres.
    pub centre: DVec3,
    /// How far the burst reached, in metres — the radius the overlap was run at,
    /// so the picture is the query.
    pub radius_m: f64,
}

/// Which creep a tower at `from` reaching `range_m` shoots, or `None` for a
/// tower with nothing in range.
///
/// **The creep nearest the exit**, which is every tower defense's rule: the one
/// with the least path left is the one about to cost a life.
/// [`Creep::along`] is that ordering, and it is a total one over the creeps on
/// the field, so two runs pick the same target.
///
/// `scratch` is the caller's so the query allocates nothing — this runs once
/// per tower per tick.
#[must_use]
pub fn acquire(
    world: &mut PhysicsWorld,
    creeps: &[Creep],
    from: DVec3,
    range_m: f64,
    scratch: &mut Vec<ColliderId>,
) -> Option<usize> {
    world.overlap_sphere_into(from, range_m, scratch);
    let mut best: Option<(usize, f64)> = None;
    for id in scratch.iter() {
        // The ground and the exit volume are in range of every tower, and both
        // come back from the overlap. See the module docs.
        let Some(index) = creeps.iter().position(|creep| creep.body() == *id) else {
            continue;
        };
        let along = creeps[index].along();
        if best.is_none_or(|(_, furthest)| along > furthest) {
            best = Some((index, along));
        }
    }
    best.map(|(index, _)| index)
}

/// Holds every creep inside `range_m` of `from` at `factor` of its speed, and
/// answers how many it held.
///
/// **A [`Kind::Slow`] tower's whole effect, run once a tick.** There is no
/// projectile and no state on the tower: the hold is written onto the creeps the
/// overlap found right now, and a creep the overlap no longer finds was released
/// before this ran — see [`crate::creep`]'s module docs for why the hold is
/// recomputed rather than latched.
///
/// The count is what the frame draws a slow tower hot on, so a tower with
/// nothing in reach is plainly a tower with nothing in reach.
pub fn hold(
    world: &mut PhysicsWorld,
    creeps: &mut [Creep],
    from: DVec3,
    range_m: f64,
    factor: f64,
    scratch: &mut Vec<ColliderId>,
) -> usize {
    world.overlap_sphere_into(from, range_m, scratch);
    let mut held = 0;
    for id in scratch.iter() {
        // The same filter every query in this file needs: the slab and the exit
        // volume are in reach of every plot.
        let Some(creep) = creeps.iter_mut().find(|creep| creep.body() == *id) else {
            continue;
        };
        creep.slow_to(factor);
        held += 1;
    }
    held
}

/// Every collider a burst of `radius_m` metres at `at` reaches, into `scratch`.
///
/// The splash half of [`Kind::Splash`]: one [`PhysicsWorld::overlap_sphere`] at
/// the point a bolt stopped. It answers with the slab and the exit volume too —
/// `crate::game::Stage::splash` is what turns the ids into creeps, and what
/// keeps the creep the bolt struck directly from being wounded twice.
pub fn burst_into(
    world: &mut PhysicsWorld,
    at: DVec3,
    radius_m: f64,
    scratch: &mut Vec<ColliderId>,
) {
    world.overlap_sphere_into(at, radius_m, scratch);
}

// ---------------------------------------------------------------------------
// The bolt
// ---------------------------------------------------------------------------

/// What became of a bolt over one tick.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum BoltOutcome {
    /// Still in the air.
    Flying,
    /// It struck the creep whose body this is.
    Hit(ColliderId),
    /// It struck the map — in practice the ground, which every bolt is
    /// descending toward. It is gone and nothing was damaged **directly**; a
    /// splash bolt still bursts where it stopped.
    Spent,
}

/// One bolt in the air.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Bolt {
    /// Where its centre is, in metres.
    at: DVec3,
    /// The unit direction it is travelling in, kept so a bolt whose target died
    /// still has somewhere to go.
    heading: DVec3,
    /// The body it was fired at. A [`ColliderId`] rather than an index into the
    /// creep list, because that list is swap-removed as creeps die: an index
    /// would silently come to mean a different creep, and a stale
    /// [`ColliderId`] resolves to nothing instead.
    target: ColliderId,
    /// What it takes off whatever it hits, and off everything in its burst.
    damage: u32,
    /// How far its burst reaches, in metres, or zero for a bolt that wounds
    /// only what it struck. Carried on the bolt rather than looked up from the
    /// tower, because the tower may have been upgraded — or the run restarted —
    /// between the shot and the landing.
    burst_m: f64,
}

impl Bolt {
    /// Fires a bolt from `from` at `target`, with `spec`'s damage and burst.
    #[must_use]
    pub fn fire(from: DVec3, target: &Creep, spec: &TowerSpec) -> Self {
        Self {
            at: from,
            heading: (target.centre() - from).normalize_or_zero(),
            target: target.body(),
            damage: spec.damage,
            burst_m: spec.burst_m,
        }
    }

    /// Where it is, for the frame to draw it.
    #[must_use]
    pub const fn at(&self) -> DVec3 {
        self.at
    }

    /// What it takes off what it hits.
    #[must_use]
    pub const fn damage(&self) -> u32 {
        self.damage
    }

    /// How far its burst reaches, in metres.
    #[must_use]
    pub const fn burst_m(&self) -> f64 {
        self.burst_m
    }

    /// Whether it bursts at all.
    #[must_use]
    pub const fn bursts(&self) -> bool {
        self.burst_m > 0.0
    }

    /// Flies one tick, and says what it met on the way.
    ///
    /// The sweep is the whole of the collision — see the module docs. A hit on
    /// anything that is not a creep is [`BoltOutcome::Spent`]: the map stops a
    /// bolt, it does not bounce one.
    pub fn step(&mut self, world: &mut PhysicsWorld, creeps: &[Creep], dt: f64) -> BoltOutcome {
        if let Some(creep) = creeps.iter().find(|creep| creep.body() == self.target) {
            let toward = creep.centre() - self.at;
            if toward.length_squared() > 0.0 {
                self.heading = toward.normalize();
            }
        }
        let to = self.at + self.heading * BOLT_SPEED * dt;
        if let Some((id, hit)) = world.sweep_sphere(&Segment::new(self.at, to), BOLT_RADIUS) {
            // Left where it struck rather than at the end of the segment, so
            // the last place a bolt was is a place on a surface — and so a
            // burst is raised where the impact was.
            self.at = hit.point;
            return if creeps.iter().any(|creep| creep.body() == id) {
                BoltOutcome::Hit(id)
            } else {
                BoltOutcome::Spent
            };
        }
        self.at = to;
        BoltOutcome::Flying
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::creep::{self, Creep};
    use crate::map;

    /// One tick at the sample's own rate.
    const DT: f64 = 1.0 / crate::game::DEFAULT_TICK_HZ as f64;

    /// A fast creep `along` metres into the path, in a world with the map in it.
    fn creep_at(world: &mut PhysicsWorld, along: f64) -> Creep {
        let mut creep = Creep::spawn(world, creep::Kind::Fast);
        // Walked rather than placed, because `advance` is what writes the
        // sphere: a creep whose number moved and whose body did not is exactly
        // the failure these queries would then fail to see.
        let ticks = (along / (creep::Kind::Fast.spec().speed * DT)).round() as u64;
        for _ in 0..ticks {
            creep.advance(world, DT);
        }
        creep
    }

    /// Which plot is labelled `label`.
    fn plot(label: &str) -> usize {
        PLOTS
            .iter()
            .position(|plot| plot.label == label)
            .unwrap_or_else(|| panic!("the map has no {label} plot"))
    }

    /// **The enum and the table are in the same order**, which is what makes
    /// [`Kind::index`] a row number rather than a hope — and what makes the kind
    /// byte on the wire mean what the player pressed.
    #[test]
    fn the_kinds_and_their_rows_are_in_the_same_order() {
        for (row, kind) in ALL.iter().enumerate() {
            assert_eq!(kind.index(), row, "{} is not row {row}", kind.label());
            assert_eq!(
                Kind::from_index(row as u8),
                Some(*kind),
                "byte {row} does not decode to the {} kind",
                kind.label(),
            );
            for (column, tier) in [Tier::Base, Tier::Upgraded].iter().enumerate() {
                assert_eq!(tier.index(), column);
                assert_eq!(
                    kind.spec(*tier),
                    &TOWERS[row][column],
                    "the {} kind does not read its own row at {}",
                    kind.label(),
                    tier.label(),
                );
            }
        }
        assert_eq!(
            Kind::from_index(KINDS as u8),
            None,
            "a byte past the table decoded"
        );
        assert_eq!(Kind::from_index(u8::MAX), None);
        // Every label distinct: the build list and the `[HUD]` line name a kind
        // by it, and two kinds called the same thing are unreadable.
        for (at, kind) in ALL.iter().enumerate() {
            for other in &ALL[at + 1..] {
                assert_ne!(kind.label(), other.label(), "two kinds share a label");
            }
        }
    }

    /// **Every row either fires or holds, and never both.** The tick puts a
    /// tower in exactly one of its two passes on that reading, so a row that
    /// answered `false` to both would be a tower a player can buy that does
    /// nothing at all — and one that answered `true` to both would be a tower
    /// the tick runs twice.
    ///
    /// The second half is what makes the first mean something: a shooting row
    /// must have a reload, or it fires on every tick of the simulation rather
    /// than on its own clock.
    #[test]
    fn every_row_either_fires_or_holds_and_never_both() {
        for kind in ALL {
            for tier in [Tier::Base, Tier::Upgraded] {
                let spec = kind.spec(tier);
                let (label, tier_label) = (kind.label(), tier.label());
                assert_ne!(
                    spec.fires(),
                    spec.slows(),
                    "the {tier_label} {label} row both fires and holds, or does neither",
                );
                assert!(
                    spec.range_m > 0.0,
                    "the {tier_label} {label} row reaches nothing",
                );
                if spec.fires() {
                    assert!(
                        spec.reload_s > 0.0,
                        "the {tier_label} {label} row fires with no reload",
                    );
                } else {
                    assert!(
                        spec.burst_m == 0.0,
                        "the {tier_label} {label} row bursts without firing",
                    );
                }
                assert!(spec.cost > 0, "the {tier_label} {label} row is free");
            }
        }
        // And the kinds are the three the plan asks for, each doing its own job.
        assert!(Kind::Bolt.spec(Tier::Base).fires());
        assert!(!Kind::Bolt.spec(Tier::Base).bursts());
        assert!(Kind::Splash.spec(Tier::Base).bursts());
        assert!(Kind::Slow.spec(Tier::Base).slows());
    }

    /// **An upgrade is dearer and better than what it replaces**, every kind.
    /// A row typed in the wrong order would be a purchase that makes a tower
    /// worse, and the purse would still come out of the player's pocket.
    #[test]
    fn an_upgrade_is_dearer_and_better_than_what_it_replaces() {
        for kind in ALL {
            let (base, up) = (kind.spec(Tier::Base), kind.spec(Tier::Upgraded));
            let label = kind.label();
            assert!(
                up.range_m > base.range_m,
                "an upgraded {label} reaches {} m against {} m",
                up.range_m,
                base.range_m,
            );
            assert!(
                up.cost > base.cost,
                "stepping a {label} up costs {} against the {} it cost to build",
                up.cost,
                base.cost,
            );
            if base.fires() {
                assert!(
                    up.damage > base.damage && up.reload_s < base.reload_s,
                    "an upgraded {label} does {} per {} s against {} per {} s",
                    up.damage,
                    up.reload_s,
                    base.damage,
                    base.reload_s,
                );
            }
            if base.bursts() {
                assert!(
                    up.burst_m > base.burst_m,
                    "an upgraded {label} bursts over {} m against {} m",
                    up.burst_m,
                    base.burst_m,
                );
            }
            if base.slows() {
                assert!(
                    up.slow_factor < base.slow_factor,
                    "an upgraded {label} holds at {} against {}",
                    up.slow_factor,
                    base.slow_factor,
                );
            }
        }
    }

    /// **A tower is upgraded once and then turned down.** The rule the
    /// `UpgradeTower` command's server side rests on — `crate::game` checks the
    /// plot and the purse, and this is the half that says there is exactly one
    /// tier to buy.
    #[test]
    fn an_upgrade_is_taken_once_and_then_turned_down() {
        let mut tower = Tower::new(0, Kind::Bolt);
        assert_eq!(tower.tier(), Tier::Base);
        assert_eq!(
            tower.upgrade_cost(),
            Some(Kind::Bolt.spec(Tier::Upgraded).cost),
        );
        assert!(tower.upgrade(), "the first upgrade was refused");
        assert_eq!(tower.tier(), Tier::Upgraded);
        assert_eq!(
            tower.upgrade_cost(),
            None,
            "an upgraded tower is still priced"
        );
        assert!(!tower.upgrade(), "it was upgraded twice");
        assert_eq!(tower.tier(), Tier::Upgraded);
        // And the stats it reads moved with it.
        assert_eq!(tower.spec(), Kind::Bolt.spec(Tier::Upgraded));
    }

    /// **The shortest reach and the shortest reload are the table's own.** Both
    /// are derived in a `const` block, and both are what something else rests
    /// on — the plots in `crate::map` and [`crate::map::MAX_BOLTS`] — so a
    /// derivation that silently stopped covering a row would take those two
    /// claims with it.
    #[test]
    fn the_derived_extremes_are_the_tables_own() {
        let ranges = ALL
            .iter()
            .flat_map(|kind| [Tier::Base, Tier::Upgraded].map(|tier| kind.spec(tier).range_m));
        assert_eq!(
            SHORTEST_RANGE_M,
            ranges.fold(f64::INFINITY, f64::min),
            "the shortest reach is not the table's",
        );
        let reloads = ALL
            .iter()
            .flat_map(|kind| [Tier::Base, Tier::Upgraded].map(|tier| *kind.spec(tier)))
            .filter(TowerSpec::fires)
            .map(|spec| spec.reload_s);
        assert_eq!(
            SHORTEST_RELOAD_S,
            reloads.fold(f64::INFINITY, f64::min),
            "the shortest reload is not the table's",
        );
    }

    /// **A burst is gone before its tower can raise another**, which is the
    /// claim [`crate::map::MAX_BURSTS`] rests on: one slot per plot, because a
    /// plot never has two bursts drawn at once.
    #[test]
    fn a_burst_is_gone_before_its_tower_can_raise_another() {
        for kind in ALL {
            for tier in [Tier::Base, Tier::Upgraded] {
                let spec = kind.spec(tier);
                if !spec.bursts() {
                    continue;
                }
                assert!(
                    BURST_S < spec.reload_s,
                    "a {} {} burst is drawn for {BURST_S} s against a {} s reload",
                    tier.label(),
                    kind.label(),
                    spec.reload_s,
                );
            }
        }
        const {
            assert!(
                BURST_S < FLASH_S,
                "a burst outlives the flash that raised it"
            )
        };
    }

    /// **One tick of a bolt is longer than a creep is wide.** The inequality
    /// the sweep exists for, asserted against the constants rather than assumed
    /// by the test below it: without it that test would pass on a build with no
    /// sweep in it at all.
    #[test]
    fn a_tick_of_a_bolt_is_longer_than_a_creep_is_wide() {
        let step = BOLT_SPEED * DT;
        let shadow = 2.0 * (map::CREEP_RADIUS + BOLT_RADIUS);
        assert!(
            step > shadow,
            "a bolt covers {step:.2} m a tick and a creep casts a {shadow:.2} m shadow, so a \
             test at the two ends of a tick would catch it",
        );
    }

    /// **A bolt hits a creep that a test at either end of the tick would
    /// miss** — and the creep is moving while it happens.
    ///
    /// The two `overlap_sphere` readings are the control: they are the check a
    /// build without continuous collision would be making, and both of them
    /// answer no on the very tick the bolt goes through the creep. What sees it
    /// is [`PhysicsWorld::sweep_sphere`] over the segment between them.
    #[test]
    fn a_bolt_hits_a_creep_that_a_test_at_either_end_of_the_tick_would_miss() {
        let (mut world, _) = map::world();
        let mut creep = creep_at(&mut world, 6.0);

        // Half a tick's travel short of the creep, aimed straight at it: one
        // step carries the bolt the same distance out the far side.
        let half_step = 0.5 * BOLT_SPEED * DT;
        let approach = DVec3::new(0.0, 0.0, 1.0);
        let from = creep.centre() + approach * half_step;
        let mut bolt = Bolt::fire(from, &creep, Kind::Bolt.spec(Tier::Base));

        assert!(
            !world
                .overlap_sphere(from, BOLT_RADIUS)
                .contains(&creep.body()),
            "the bolt starts the tick already inside the creep, so this proves nothing",
        );

        // The creep walks first, exactly as the tick order has it.
        creep.advance(&mut world, DT);
        let creeps = [creep];
        // Where the tick's segment ends, worked out the way `Bolt::step` works
        // it out — this is the *other* place a static test would look, and it
        // is read before the step because a hit leaves the bolt on the contact
        // point rather than at the end of its segment.
        let heading = (creeps[0].centre() - from).normalize();
        let tick_end = from + heading * BOLT_SPEED * DT;
        assert!(
            !world
                .overlap_sphere(tick_end, BOLT_RADIUS)
                .contains(&creeps[0].body()),
            "the tick ends with the bolt inside the creep, so a static test would have caught it",
        );

        let outcome = bolt.step(&mut world, &creeps, DT);
        assert_eq!(
            outcome,
            BoltOutcome::Hit(creeps[0].body()),
            "the sweep did not find the creep it flew through",
        );
    }

    /// **A tower ignores everything in range that is not a creep**, and the
    /// query really does hand it those things.
    ///
    /// The second assertion is what makes the first one mean something: if the
    /// overlap returned creeps alone, a build with no filter at all would pass.
    #[test]
    fn a_tower_ignores_everything_in_range_that_is_not_a_creep() {
        let (mut world, exit) = map::world();
        let mut scratch = Vec::new();
        // The gate plot, which is the one the exit volume stands beside.
        let tower = Tower::new(plot("gate"), Kind::Bolt);
        let muzzle = tower.muzzle();

        let in_range = world.overlap_sphere(muzzle, tower.spec().range_m);
        assert!(
            in_range.contains(&exit),
            "the gate tower does not have the exit volume in range, so this proves nothing",
        );
        assert!(
            in_range.len() > 1,
            "only one thing is in range of the gate tower",
        );
        assert_eq!(
            acquire(&mut world, &[], muzzle, tower.spec().range_m, &mut scratch),
            None,
            "a tower with no creeps on the field acquired something",
        );
    }

    /// **A tower shoots the creep nearest the exit**, and shoots nothing at all
    /// once its target walks out of range.
    #[test]
    fn a_tower_shoots_the_creep_nearest_the_exit_and_nothing_out_of_range() {
        let (mut world, _) = map::world();
        let mut scratch = Vec::new();
        let tower = Tower::new(plot("entry"), Kind::Bolt);
        let (muzzle, range) = (tower.muzzle(), tower.spec().range_m);

        // Two creeps on the first leg, one further along than the other, both
        // inside the entry tower's reach.
        let behind = creep_at(&mut world, 6.0);
        let ahead = creep_at(&mut world, 9.0);
        for creep in [&behind, &ahead] {
            assert!(
                (creep.centre() - muzzle).length() < range,
                "a creep {:.2} m out is not in the entry tower's reach",
                (creep.centre() - muzzle).length(),
            );
        }
        let creeps = [behind, ahead];
        assert_eq!(
            acquire(&mut world, &creeps, muzzle, range, &mut scratch),
            Some(1),
            "it shot the creep with more path left",
        );

        // And one that has walked away down the far leg is out of reach.
        let (mut world, _) = map::world();
        let gone = creep_at(&mut world, crate::path::length() - 2.0);
        let creeps = [gone];
        assert!(
            (creeps[0].centre() - muzzle).length() > range,
            "the far end of the path is inside the entry tower's reach",
        );
        assert_eq!(
            acquire(&mut world, &creeps, muzzle, range, &mut scratch),
            None,
            "it acquired a creep outside its range",
        );
    }

    /// **A slow tower holds what is inside its reach and nothing outside it**,
    /// and the hold it writes is its own row's.
    ///
    /// The creep out of reach is the control: a build that held every creep on
    /// the field — the shape an overlap with no radius filter, or a loop over
    /// `creeps` with no query at all, produces — passes the first half and fails
    /// this one.
    #[test]
    fn a_slow_tower_holds_what_is_in_reach_and_nothing_outside_it() {
        let (mut world, _) = map::world();
        let mut scratch = Vec::new();
        let tower = Tower::new(plot("entry"), Kind::Slow);
        let spec = tower.spec();

        let near = creep_at(&mut world, 8.0);
        let far = creep_at(&mut world, crate::path::length() - 2.0);
        assert!(
            (near.centre() - tower.muzzle()).length() < spec.range_m,
            "the near creep is not in reach, so this proves nothing",
        );
        assert!(
            (far.centre() - tower.muzzle()).length() > spec.range_m,
            "the far creep is in reach too",
        );
        let mut creeps = [near, far];

        let held = hold(
            &mut world,
            &mut creeps,
            tower.muzzle(),
            spec.range_m,
            spec.slow_factor,
            &mut scratch,
        );
        assert_eq!(
            held, 1,
            "it held {held} creeps rather than the one in reach"
        );
        assert!(
            (creeps[0].slow() - spec.slow_factor).abs() < f64::EPSILON,
            "the near creep is at {} of its speed, not the row's {}",
            creeps[0].slow(),
            spec.slow_factor,
        );
        assert!(!creeps[1].is_slowed(), "a creep out of reach was held");
    }

    /// **A bolt whose target is gone stops in the ground.** Not a hit, and not
    /// a bolt that lives for ever: a creep killed by another tower's shot
    /// leaves this one's in the air with nothing to home on.
    ///
    /// **Where it stopped is the assertion that means something.** A bolt that
    /// merely reported [`BoltOutcome::Spent`] could have expired on a timer, or
    /// have been let through the ground and stopped by the loop's own patience;
    /// a bolt lying on `y = 0` struck the slab. That the slab is always there
    /// to be struck is [`MUZZLE_Y`]'s doing, asserted first.
    #[test]
    fn a_bolt_whose_target_is_gone_stops_in_the_ground() {
        const {
            assert!(
                MUZZLE_Y > map::CREEP_RADIUS,
                "a bolt fired at a creep would not be descending",
            );
        }
        let (mut world, _) = map::world();
        let creep = creep_at(&mut world, 6.0);
        let tower = Tower::new(plot("entry"), Kind::Bolt);
        let mut bolt = Bolt::fire(tower.muzzle(), &creep, tower.spec());
        creep.despawn(&mut world);

        let mut outcome = BoltOutcome::Flying;
        for _ in 0..600 {
            outcome = bolt.step(&mut world, &[], DT);
            if outcome != BoltOutcome::Flying {
                break;
            }
        }
        assert_eq!(
            outcome,
            BoltOutcome::Spent,
            "a bolt with no target left is still flying",
        );
        assert!(
            bolt.at().y.abs() < BOLT_RADIUS,
            "it stopped at y = {:.2}, which is not the ground",
            bolt.at().y,
        );
        assert!(
            bolt.at().x.abs() < map::HALF_WIDTH && bolt.at().z.abs() < map::HALF_DEPTH,
            "it stopped at {:?}, off the slab",
            bolt.at(),
        );
    }

    /// **A bolt is gone before its tower can fire again.** The claim
    /// [`crate::map::MAX_BOLTS`] rests on: a tower with a bolt still in the
    /// air has not reloaded, so a full field has one bolt per plot and never
    /// more. Measured on the longest flight there is — a bolt fired at the
    /// edge of the *longest* reach in the table whose target then vanished,
    /// which glides on its last heading until the ground stops it instead of
    /// ending in a creep — and against the *shortest* reload in the table,
    /// which is the quickest any tower could ask for a second one.
    #[test]
    fn a_bolt_lands_long_before_its_tower_reloads() {
        let (mut world, _) = map::world();
        let tower = Tower::new(plot("entry"), Kind::Bolt);
        let muzzle = tower.muzzle();
        let longest = ALL
            .iter()
            .flat_map(|kind| [Tier::Base, Tier::Upgraded].map(|tier| kind.spec(tier).range_m))
            .fold(0.0_f64, f64::max);

        let mut creep = Creep::spawn(&mut world, creep::Kind::Fast);
        let mut walked = 0_u64;
        while (creep.centre() - muzzle).length() > longest {
            creep.advance(&mut world, DT);
            walked += 1;
            assert!(walked < 10_000, "the entry plot never had a creep in range");
        }

        let mut bolt = Bolt::fire(muzzle, &creep, tower.spec());
        creep.despawn(&mut world);
        let mut ticks = 0_u64;
        while bolt.step(&mut world, &[], DT) == BoltOutcome::Flying {
            ticks += 1;
            assert!(ticks < 10_000, "the bolt never came down");
        }
        let flight = (ticks + 1) as f64 * DT;
        assert!(
            flight < SHORTEST_RELOAD_S,
            "a bolt fired at the edge of the longest reach was up for {flight:.3} s \
             against a {SHORTEST_RELOAD_S} s reload",
        );
    }

    /// **A tower fires on its reload and not on the tick rate**, and the reload
    /// is its own row's. One shot per the row's `reload_s`, however many ticks
    /// that is.
    ///
    /// Every shooting row, because the reload moved into the table: a build that
    /// read one row for all of them would pass on the kind it read.
    #[test]
    fn a_tower_fires_on_its_reload_rather_than_every_tick() {
        let seconds = 6.0;
        for kind in ALL {
            for tier in [Tier::Base, Tier::Upgraded] {
                if !kind.spec(tier).fires() {
                    continue;
                }
                let mut tower = Tower::new(0, kind);
                if tier == Tier::Upgraded {
                    assert!(tower.upgrade());
                }
                let reload = tower.spec().reload_s;
                let mut shots = 0_u32;
                for tick in 0..(seconds / DT).round() as u64 {
                    let now = tick as f64 * DT;
                    if tower.is_ready(now) {
                        shots += 1;
                        tower.fired(now);
                        assert!(tower.is_firing(now), "a tower that fired is not drawn hot");
                    }
                }
                let expected = (seconds / reload).floor() as u32 + 1;
                assert!(
                    shots.abs_diff(expected) <= 1,
                    "a {} {} fired {shots} times over {seconds} s at one per {reload} s",
                    tier.label(),
                    kind.label(),
                );
                assert!(
                    !tower.is_firing(seconds + 1.0),
                    "a tower is still drawn hot a second after its last shot",
                );
            }
        }
    }
}
