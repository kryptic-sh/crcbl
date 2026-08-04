//! Horde's simulation: one arena, one player, an auto-aiming weapon, and as
//! many dumb seeking agents as the machine will carry.
//!
//! # What is different about this one
//!
//! Breakout spawns its world once. Flappy runs a treadmill. Asteroids churns
//! hard but never holds more than about fifty bodies at a time. This game's
//! whole question is **what happens when the same tick has to steer a thousand
//! agents and then ten thousand**, so the interesting number is not how often
//! something spawns but how much work one tick does per live body.
//!
//! `docs/plan/sample/03-horde.md` is the plan. What is here is the core loop —
//! arena, player, enemies, damage, death — plus the progression the art
//! sub-slice added: XP that drops where an enemy died, and a "pick 1 of 3"
//! level-up from a fixed pool of six upgrades. The scale push, the measurement
//! and the browser demo are the sub-slice after.
//!
//! # A level-up freezes the field, and the freeze is simulation state
//!
//! [`GameState::LevelUp`] is a state of the *simulation*, not of the loop —
//! unlike pause, which the window owns. It has to be, because the choice the
//! player makes changes what the simulation does, and a seeded script has to
//! replay it. While it is up, nothing is steered, spawned, swept or damaged and
//! the run clock is stopped.
//!
//! The freeze is **one pass, on the tick it starts**, not a check on the hot
//! path: `freeze_field` writes a zero velocity to the player, every enemy and
//! every bolt once, and the integrator then moves nothing for as long as the
//! screen is up. A bolt keeps its velocity in [`Bolt::velocity`] so it can be
//! given back the moment the screen closes; an enemy needs no such thing,
//! because `steer_enemies` writes it a fresh velocity on the first tick after.
//!
//! # Three seams into `crcbl-phys`, and each is a different query
//!
//! * **Separation is `N` overlap queries a tick**, one per enemy, each centred
//!   on that enemy — see `steer_enemies`. It is the workload the sample
//!   exists to produce, and it goes through the broadphase rather than an `N²`
//!   loop over the enemy list.
//! * **Contact damage is exactly one overlap query a tick**, centred on the
//!   player — see `contact_damage`. The player is not in the broadphase, so
//!   what comes back is enemies and only enemies.
//! * **The weapon is segment CCD**, `prev → cur` through
//!   [`PhysicsSystem::sweep_sphere`], so the fastest bolt the game can fire
//!   cannot step over the thinnest enemy. See `sweep_bolts`. Aiming is a
//!   *fourth* use of the same broadphase: one overlap query at
//!   [`WEAPON_RANGE`] rather than a scan of the enemy list.
//!
//! # Both query radii are exact, and that is a property of the shape query
//!
//! [`crcbl::phys::PhysicsWorld::overlap_sphere`] tests the query sphere against
//! the collider's *shape*, so a query of radius `R` centred on `a` returns every
//! collider `b` whose centre is within `R + r_b`. Both of this game's overlap
//! queries exploit that rather than working around it:
//!
//! * separation wants every pair closer than `r_a + r_b + slack`, so it queries
//!   with `r_a + slack` — no filtering, no over-fetch;
//! * contact damage wants every enemy touching the player, `d < r_player + r_b`,
//!   so it queries with `PLAYER_RADIUS` and every result is a hit.
//!
//! `the_separation_query_radius_is_exactly_the_neighbourhood` pins the first of
//! those against a hand-computed set, because it is an assumption about a
//! *different crate* and nothing else in this file would notice if it changed.
//!
//! # Nothing here is force-driven
//!
//! Asteroids was the L1 force pipeline's consumer. This game is not: a survivors
//! agent has a velocity it *chooses*, not one a force integrates it towards, and
//! putting a mass and a drag term between the two would only add a lag nobody
//! asked for. So every body in this world is
//! [`RigidBody::new_kinematic`] — no forces, no providers, an infinite mass —
//! and the game writes `velocity` each tick while `SemiImplicitEuler` does the
//! `position += velocity * dt` and nothing else.
//!
//! # Where the simulation runs
//!
//! Inside the server's tick, in `HordeModule::tick` — the hook `crcbl-ecs`
//! documents as running every server tick *after* the ECS schedule, which means
//! after [`PhysicsSystem::step`] has integrated everything. [`Game`] is the
//! client-side facade: it resolves input into an `Intent`, advances the server
//! and the client by exactly one tick period, and reads back what to draw.

use std::collections::HashMap;
use std::sync::{Arc, Mutex, MutexGuard};
use std::time::Duration;

use crcbl::core::input::KeyCode;
use crcbl::ecs::{Entity, GameModule, World};
use crcbl::input::{ActionDecl, ActionKind, ActionMap, Binding};
use crcbl::math::DVec3;
use crcbl::net::ProtocolCompatibility;
use crcbl::phys::{ColliderComponent, PhysicsSystem, RigidBody, Segment, Transform};
use crcbl::session::Loopback;

/// Distinct from breakout's, flappy's and asteroids', because they are distinct
/// protocols: a client built for one must not hand-shake with a server running
/// another. The version is the wire format's, which is shared; the schema hash
/// is this game's.
const COMPATIBILITY: ProtocolCompatibility = ProtocolCompatibility {
    protocol_version: 3,
    engine_build_id: 0x0043_5243_424C,
    schema_hash: 0x0048_4F52_4445,
};

/// The default simulation rate. The value reaches the server, the client, the
/// ECS `tick_dt` and the integrator, so there is exactly one rate in the
/// process.
pub const DEFAULT_TICK_HZ: u32 = 60;

// ---------------------------------------------------------------------------
// The arena
// ---------------------------------------------------------------------------

/// Half the width of the arena, in world units.
///
/// 4:3 against [`ARENA_HALF_HEIGHT`], and **much larger than the view** — see
/// [`VIEW_HALF_HEIGHT`]. Asteroids' field is exactly the viewport because its
/// defining move is crossing an edge; this game's is running away, and a player
/// who can reach the wall in two seconds has nowhere to run to.
///
/// 96 units across at [`PLAYER_SPEED`] is fourteen seconds corner to corner,
/// which is long enough that retreating is a real option and short enough that
/// the horde catches up.
pub const ARENA_HALF_WIDTH: f64 = 48.0;

/// Half the height of the arena, in world units.
pub const ARENA_HALF_HEIGHT: f64 = 36.0;

/// Half the vertical extent the camera shows, in world units.
///
/// The camera follows the player rather than framing the whole arena, which is
/// the genre's rule and this game's: an arena sized to fit on screen is an arena
/// with no room to retreat into. 14 units of half-height puts about 37 × 28
/// units on a 4:3 window, so [`SPAWN_RING`] can sit just outside the corner.
pub const VIEW_HALF_HEIGHT: f64 = 14.0;

// ---------------------------------------------------------------------------
// The player
// ---------------------------------------------------------------------------

/// The player's radius, in world units.
///
/// A real radius, not a query convenience: it is the sphere
/// `contact_damage` tests with, and the margin the arena clamp keeps the
/// player off the wall by.
pub const PLAYER_RADIUS: f64 = 0.5;

/// How fast the player moves, in world units per second.
///
/// Faster than a [`EnemyKind::Grunt`] (3.2) and slower than a
/// [`EnemyKind::Runner`] (5.6), which is the whole of the movement game: walking
/// away from the mass works, and outrunning the fast ones does not.
pub const PLAYER_SPEED: f64 = 7.0;

/// The player's hit points at the start of a run.
///
/// A hundred, so the HUD's number is a percentage and the damage figures below
/// read as "how many seconds of contact is this".
pub const PLAYER_MAX_HP: f64 = 100.0;

/// Which way the wizard is turned.
///
/// **Set by the input**, in `drive_player`, and by nothing else. Not by the aim,
/// which is the gun's business and would spin the figure round every time the
/// nearest enemy changed; and not by the velocity, which is the input after the
/// arena clamp has had it and would leave a wizard pressed against a wall facing
/// whichever way the wall let it slide.
///
/// Only the horizontal keys move it. Pressing up, down, both horizontals or
/// nothing at all leaves it exactly where it was — a wizard that snapped back to
/// a default on key-up would flicker every time the player stopped, and stopping
/// is most of what a player does.
///
/// [`Facing::Right`] is the default because that is the way
/// `assets/actors.crpix` draws the figure; the other one is the same art with
/// its `u` range reversed.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub enum Facing {
    #[default]
    Right,
    Left,
}

// ---------------------------------------------------------------------------
// The weapon
// ---------------------------------------------------------------------------

/// How far the auto-aim looks for a target, in world units.
///
/// Inside [`VIEW_HALF_HEIGHT`], so the gun never fires at something the player
/// cannot see — a weapon that kills off screen makes the horde's arrival
/// unreadable.
pub const WEAPON_RANGE: f64 = 13.0;

/// A bolt's speed, in world units per second.
pub const BOLT_SPEED: f64 = 30.0;

/// The radius a bolt's sweep uses. A bolt has no collider — see
/// `sweep_bolts` — so this is only ever the radius of the swept sphere.
pub const BOLT_RADIUS: f64 = 0.15;

/// How much damage one bolt does.
///
/// Four against a grunt's six means two bolts a kill; against a brute's
/// twenty-four it means six, which is what makes a brute something to run from
/// rather than something to shoot.
pub const BOLT_DAMAGE: f64 = 4.0;

/// How long a bolt lives, in seconds.
///
/// `BOLT_SPEED * BOLT_LIFE` is 18 units against a [`WEAPON_RANGE`] of 13, so a
/// bolt always outlives the reach it was fired at — a shot that expired short of
/// its target would make the weapon's range a lie, and
/// `the_reach_of_a_bolt_covers_the_weapons_range` asserts the relation rather
/// than the number.
pub const BOLT_LIFE: f64 = 0.6;

/// The gap between shots, in seconds. Four a second.
pub const FIRE_COOLDOWN: f64 = 0.25;

/// Where a bolt appears, relative to the player's centre, for a wizard facing
/// right. In world units.
///
/// **The head of the staff, to the texel.** `assets/actors.crpix` draws the orb
/// at exactly this offset — this constant times `art::TEXELS_PER_UNIT`, from the
/// centre of the frame — and `art::tests::the_staff_head_is_where_the_muzzle_says_it_is`
/// measures the baked bytes against it, so the picture and the shot cannot drift
/// apart. It is a *point* rather than a distance because the staff is held out
/// to one side and up: there is no direction it is "in front" along.
///
/// It sits **inside** [`PLAYER_RADIUS`] plus [`BOLT_RADIUS`], which the old
/// straight-ahead muzzle did not, and that is a consequence of drawing the
/// wizard to its collider rather than an oversight: the whole figure, staff
/// included, is 2 × [`PLAYER_RADIUS`] across, so nothing on it can be further
/// out than that. Nothing depended on the clearance — a bolt has no collider
/// against the player, and the reason it is not drawn *through* the wizard is
/// that `art::Scene` puts the shots on a layer above the hero.
pub const STAFF_MUZZLE: DVec3 = DVec3::new(0.45, 0.45, 0.0);

/// The muzzle for a wizard turned this way.
///
/// The sprite is one drawing with its `u` range reversed, so the staff mirrors
/// with the figure and the muzzle mirrors on X with the staff.
///
/// # It does not follow the target
///
/// The wizard faces where the input pointed and the gun aims itself, so a
/// wizard can be walking left and firing right. When that happens the bolt still
/// starts at the drawn staff head and crosses the body — it is not flipped to
/// the firing side. The choice is that the staff head is a thing on screen: a
/// bolt appearing a body's width away from the orb because the target is behind
/// would make the picture a lie about where the magic comes from, and the cost
/// is a bolt sweeping across a 1-unit figure in a thirtieth of a second, drawn
/// over the wizard rather than under it. It also cannot make the weapon miss —
/// `sweep_bolts` sweeps from wherever the bolt starts, so a shot that begins on
/// the far side of the body sweeps *more* of the ground in front of it, not
/// less.
#[must_use]
pub const fn staff_muzzle(facing: Facing) -> DVec3 {
    match facing {
        Facing::Right => STAFF_MUZZLE,
        Facing::Left => DVec3::new(-STAFF_MUZZLE.x, STAFF_MUZZLE.y, STAFF_MUZZLE.z),
    }
}

// ---------------------------------------------------------------------------
// The enemies
// ---------------------------------------------------------------------------

/// One of the three things that come at the player.
///
/// Three, which is the top of the plan's "2–3 enemy types" and the smallest
/// number that makes the mix mean anything: something numerous, something fast
/// and something that will not die.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum EnemyKind {
    /// The mass. Slow, weak, and most of what is on the field.
    Grunt,
    /// Faster than the player. Dies to one bolt and a bit.
    Runner,
    /// Slow, huge, and takes six bolts.
    Brute,
}

impl EnemyKind {
    /// Every kind, in a fixed order, for the tests and the spawn table.
    pub const ALL: [Self; 3] = [Self::Grunt, Self::Runner, Self::Brute];

    /// The collider radius, in world units.
    #[must_use]
    pub const fn radius(self) -> f64 {
        match self {
            Self::Grunt => 0.45,
            Self::Runner => 0.32,
            Self::Brute => 0.85,
        }
    }

    /// Hit points.
    #[must_use]
    pub const fn max_hp(self) -> f64 {
        match self {
            Self::Grunt => 6.0,
            Self::Runner => 2.0,
            Self::Brute => 24.0,
        }
    }

    /// How fast it seeks, in world units per second.
    #[must_use]
    pub const fn speed(self) -> f64 {
        match self {
            Self::Grunt => 3.2,
            Self::Runner => 5.6,
            Self::Brute => 1.9,
        }
    }

    /// How much damage a second it does while it is touching the player.
    ///
    /// **Continuous, not a hit with a cooldown.** A per-hit model needs
    /// invulnerability frames to stop a stack of enemies deleting the player in
    /// one tick, and invulnerability frames are per-enemy timers — `N` more
    /// pieces of state on the hottest path in the game. A damage *rate* summed
    /// over whatever is touching costs one multiply and says the same thing:
    /// standing in a crowd is worse than standing next to one.
    #[must_use]
    pub const fn contact_dps(self) -> f64 {
        match self {
            Self::Grunt => 12.0,
            Self::Runner => 8.0,
            Self::Brute => 30.0,
        }
    }

    /// How much experience the gem it drops is worth.
    ///
    /// Flat for the two cheap kinds and five times that for a brute, which is
    /// roughly what six bolts against two is worth — so shooting the thing that
    /// takes work is paid for, and the level-up rate tracks the *effort* a run
    /// puts in rather than the number of bodies it walks past.
    #[must_use]
    pub const fn xp(self) -> u64 {
        match self {
            Self::Grunt | Self::Runner => 1,
            Self::Brute => 5,
        }
    }

    /// The collider one of these carries.
    #[must_use]
    pub const fn collider(self) -> ColliderComponent {
        ColliderComponent::Sphere {
            offset: DVec3::ZERO,
            radius: self.radius(),
            is_trigger: false,
        }
    }

    /// The kind a uniform draw in `[0, 1)` selects.
    ///
    /// A fixed table rather than a difficulty ramp: the ramp in this game is the
    /// spawn *rate* (see [`spawn_interval`]), and a second one riding on the
    /// same clock would make neither legible.
    #[must_use]
    pub fn from_roll(roll: f64) -> Self {
        if roll < 0.62 {
            Self::Grunt
        } else if roll < 0.90 {
            Self::Runner
        } else {
            Self::Brute
        }
    }
}

/// The largest [`EnemyKind::radius`] there is.
///
/// Not a fourth constant to keep in step — derived, so adding a kind cannot
/// leave it stale.
#[must_use]
pub fn max_enemy_radius() -> f64 {
    EnemyKind::ALL
        .iter()
        .map(|kind| kind.radius())
        .fold(0.0f64, f64::max)
}

// ---------------------------------------------------------------------------
// Experience, pickups and the level-up
// ---------------------------------------------------------------------------

/// The radius of a dropped gem's collider, in world units.
///
/// 0.7 units across, which at `art::TEXELS_PER_UNIT` is a whole 14 texels, and
/// a little larger than a runner — a gem the player cannot see is a gem the
/// player does not walk to.
///
/// **A trigger, not a solid.** `crcbl::phys` skips triggers in
/// [`PhysicsSystem::sweep_sphere`], so a bolt flies through a gem instead of
/// being spent on it; `overlap_sphere` does *not* skip them, which is exactly
/// what `collect_pickups` wants and what the separation and aiming queries have
/// to filter back out. Both filters are the `by_entity` lookups those passes
/// already did.
pub const XP_RADIUS: f64 = 0.35;

/// The most gems that may be lying on the field at once.
///
/// A ceiling rather than a lifetime: gems do not rot, so a player who never
/// picks one up would otherwise accumulate one collider per kill forever, and
/// the broadphase this sample exists to measure would be measuring litter. When
/// it is full a kill drops nothing, which is a pressure to go and collect
/// rather than a silent loss — `pickups_dropped` counts what was skipped.
pub const MAX_PICKUPS: usize = 512;

/// How much experience the run needs to leave `level` for the next one.
///
/// Linear, for the reason [`spawn_interval`] is: the thing the player feels is
/// the *rate* of level-ups, and a linearly growing threshold against a spawn
/// rate that is itself accelerating already slows that down.
#[must_use]
pub const fn xp_for_next_level(level: u32) -> u64 {
    8 + 4 * (level as u64).saturating_sub(1)
}

/// One thing a level-up can give the player.
///
/// **Six, fixed, and every one of them is a single number.** The plan's
/// non-goals bar meta-progression and a wide weapon table; what this is for is
/// to exercise game UI mid-session, so the pool is the smallest one where the
/// choice is a choice. Each variant is one line of `apply_upgrade` and every
/// one may be taken again.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Upgrade {
    /// Shorter gap between shots.
    RapidFire,
    /// More damage a bolt.
    HeavyBolts,
    /// A faster player.
    SwiftBoots,
    /// The auto-aim looks further.
    LongBarrel,
    /// More hit points, and that much healed on the spot.
    Vitality,
    /// Gems are collected from further away.
    Magnet,
}

impl Upgrade {
    /// Every upgrade, in a fixed order. The order is the shuffle's input, so it
    /// is part of what a seed decides.
    pub const ALL: [Self; 6] = [
        Self::RapidFire,
        Self::HeavyBolts,
        Self::SwiftBoots,
        Self::LongBarrel,
        Self::Vitality,
        Self::Magnet,
    ];

    /// What the level-up menu prints on the button.
    #[must_use]
    pub const fn label(self) -> &'static str {
        match self {
            Self::RapidFire => "RAPID FIRE",
            Self::HeavyBolts => "HEAVY BOLTS",
            Self::SwiftBoots => "SWIFT BOOTS",
            Self::LongBarrel => "LONG BARREL",
            Self::Vitality => "VITALITY",
            Self::Magnet => "MAGNET",
        }
    }
}

/// How many an offer holds.
pub const UPGRADE_CHOICES: usize = 3;

/// The floor [`Upgrade::RapidFire`] cannot take the cooldown below, in seconds.
///
/// Twenty shots a second. Without it the multiplier is unbounded and a long run
/// ends up firing once a tick, which is not a weapon, it is a stress test of
/// the bolt list wearing a weapon's name.
pub const FIRE_COOLDOWN_FLOOR: f64 = 0.05;

/// Keeps the upgrade draws out of the spawn table's index space.
///
/// [`spawn_index`] packs a spawn counter and a draw number into the whole of a
/// `u64`, so there is no room left in it for a second stream. Salting the *seed*
/// instead gives the offers an independent sequence that is still a pure
/// function of the run — a restart deals different upgrades as well as
/// different hordes.
const UPGRADE_SALT: u64 = 0x5550_4752_4144_4553;

/// The three upgrades offered on reaching `level`, in run `seed`.
///
/// **Exactly three, and always distinct**, because it is a partial
/// Fisher–Yates over [`Upgrade::ALL`] rather than three independent draws —
/// three draws would offer the same upgrade twice about one level in three, and
/// a menu with two identical buttons is not a choice.
#[must_use]
pub fn upgrade_offer(seed: u64, level: u32) -> [Upgrade; UPGRADE_CHOICES] {
    let seed = seed ^ UPGRADE_SALT;
    let mut pool = Upgrade::ALL;
    let mut offer = [Upgrade::RapidFire; UPGRADE_CHOICES];
    for (i, slot) in offer.iter_mut().enumerate() {
        let remaining = pool.len() - i;
        let roll = hash_unit(seed, u64::from(level) * 8 + i as u64);
        // `hash_unit` is in `[0, 1)`, so this is in `0..remaining`; the `min`
        // is there for the one input where a rounding of 1.0 would not be.
        let pick = i + ((roll * remaining as f64) as usize).min(remaining - 1);
        pool.swap(i, pick);
        *slot = pool[i];
    }
    offer
}

/// The numbers a run can raise, and the only mutable ones in the game.
///
/// Everything else is a `const`. These start at the constants above and are
/// reset by `restart`, so a new run is a new set — the plan's non-goals bar
/// meta-progression, and the shape of this struct is what makes that structural
/// rather than a promise.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Stats {
    /// Seconds between shots. See [`FIRE_COOLDOWN`].
    pub fire_cooldown: f64,
    /// Damage one bolt does. See [`BOLT_DAMAGE`].
    pub bolt_damage: f64,
    /// World units a second. See [`PLAYER_SPEED`].
    pub player_speed: f64,
    /// How far the auto-aim looks. See [`WEAPON_RANGE`].
    pub weapon_range: f64,
    /// The player's ceiling. See [`PLAYER_MAX_HP`].
    pub max_hp: f64,
    /// The radius `collect_pickups` queries at. Starts at [`PLAYER_RADIUS`], so
    /// a gem is picked up by walking over it and no sooner.
    pub pickup_radius: f64,
}

impl Default for Stats {
    fn default() -> Self {
        Self {
            fire_cooldown: FIRE_COOLDOWN,
            bolt_damage: BOLT_DAMAGE,
            player_speed: PLAYER_SPEED,
            weapon_range: WEAPON_RANGE,
            max_hp: PLAYER_MAX_HP,
            pickup_radius: PLAYER_RADIUS,
        }
    }
}

// ---------------------------------------------------------------------------
// Separation
// ---------------------------------------------------------------------------

/// How much clear space separation tries to keep between two enemies' surfaces,
/// in world units.
///
/// **This is the query radius' whole tuning knob.** A pair is pushed apart while
/// its centres are closer than `r_a + r_b + SEPARATION_SLACK`, and the overlap
/// query that finds those pairs is `r_a + SEPARATION_SLACK` wide — see this
/// module's header. Larger means a looser, more expensive crowd (every enemy
/// sees more neighbours); smaller means the horde stacks into a single point and
/// the sample stops testing anything.
///
/// 0.35 is a little under a grunt's radius: enough that a crowd reads as a crowd
/// rather than as one enemy, small enough that the neighbourhood of a grunt is a
/// handful of bodies and not a screenful.
pub const SEPARATION_SLACK: f64 = 0.35;

/// How hard separation pushes, in world units per second, at full overlap.
///
/// Deliberately **larger than [`EnemyKind::Grunt`]'s speed** (3.2): if the push
/// were weaker than the seek, a crowd converging on a stationary player would
/// compress until the seek won and separation would be decoration. It is applied
/// on top of the seek rather than blended with it, so a fully-overlapped enemy
/// moves away from its neighbours faster than it moves towards the player.
pub const SEPARATION_STRENGTH: f64 = 6.0;

/// The radius the separation query for an enemy of `kind` is run at.
///
/// **`r_self + slack`, and the omission of the neighbour's radius is the whole
/// trick.** [`crcbl::phys::PhysicsWorld::overlap_sphere`] tests the query sphere
/// against each collider's *shape*, so this returns every `b` with
/// `d <= r_self + slack + r_b` — which is exactly the neighbourhood
/// `separation_push` wants, with nothing over-fetched and nothing filtered.
/// A query of `r_self + max_enemy_radius() + slack` would be the conservative
/// version, and at a brute's 0.85 it would nearly triple the area a grunt
/// searches.
///
/// Named, rather than spelled out at the call site, because
/// `the_separation_query_radius_is_exactly_the_neighbourhood` runs *this*
/// function against the same broadphase — a test that re-derived the radius
/// would be checking its own arithmetic.
#[must_use]
pub fn separation_query_radius(kind: EnemyKind) -> f64 {
    kind.radius() + SEPARATION_SLACK
}

// ---------------------------------------------------------------------------
// Spawning
// ---------------------------------------------------------------------------

/// How far from the player enemies enter, in world units.
///
/// Just outside the corner of a 4:3 view — `hypot(VIEW_HALF_HEIGHT * 4 / 3,
/// VIEW_HALF_HEIGHT)` is 23.3 — so an enemy walks on screen rather than
/// appearing in it. `enemies_enter_from_beyond_the_view` asserts the relation.
pub const SPAWN_RING: f64 = 24.0;

/// The gap between spawns at the start of a run, in seconds.
pub const SPAWN_INTERVAL_START: f64 = 0.5;

/// The gap between spawns once the ramp is finished, in seconds.
pub const SPAWN_INTERVAL_MIN: f64 = 0.06;

/// How long the spawn rate takes to reach [`SPAWN_INTERVAL_MIN`], in seconds.
///
/// Four minutes, against the plan's five-minute survival target: the last minute
/// is deliberately flat-out, so surviving it is the difficulty rather than
/// watching a number keep climbing.
pub const SPAWN_RAMP_SECONDS: f64 = 240.0;

/// The most enemies one tick may put on the field.
///
/// A tick with a very coarse `dt` — `--tick-hz 1`, or a debugger breakpoint —
/// would otherwise drain the whole accumulated backlog into one frame. The cap
/// makes the worst case bounded rather than a function of how long the process
/// was stopped.
const SPAWN_BURST_CAP: u32 = 64;

/// The default ceiling on live enemies.
///
/// **1500, not the plan's 10,000, and that is a decision rather than an
/// oversight.** The exit criterion of `docs/plan/sample/03-horde.md` is 10k at
/// 60 fps and 60 Hz, and the roadmap puts that behind P7 (GPU-driven rendering)
/// and P8 (`crcbl-jobs`), neither of which exists. The sub-slice that raises
/// this and measures where it breaks is the one after next; `--max-enemies` is
/// here so raising it needs no rebuild.
pub const DEFAULT_MAX_ENEMIES: usize = 1_500;

/// The gap between spawns after `elapsed` seconds of a run.
///
/// Linear from [`SPAWN_INTERVAL_START`] to [`SPAWN_INTERVAL_MIN`] over
/// [`SPAWN_RAMP_SECONDS`], then flat. Linear rather than exponential because the
/// *rate* is what the player feels and the rate of a linearly-shrinking interval
/// already accelerates.
#[must_use]
pub fn spawn_interval(elapsed: f64) -> f64 {
    let t = (elapsed / SPAWN_RAMP_SECONDS).clamp(0.0, 1.0);
    SPAWN_INTERVAL_START + (SPAWN_INTERVAL_MIN - SPAWN_INTERVAL_START) * t
}

// ---------------------------------------------------------------------------
// Determinism: every random-looking number is a pure function of a seed
// ---------------------------------------------------------------------------

/// The run every game is dealt unless a caller picks another.
pub const DEFAULT_SEED: u64 = 0x484F_5244_4553_4545;

/// A uniform value in `[0, 1)` from `seed` and `index`.
///
/// The engine's, re-exported so this game's own index spaces stay beside the
/// draws that use them. [`crcbl::core::rand`] is where the argument for hashing
/// an index rather than stepping a generator is written down — every sample
/// reached it independently.
pub use crcbl::core::rand::hash_unit;

/// The seed the `runs`-th run of a game seeded with `seed` is dealt from.
///
/// A restart deals a different run, because a game that dealt the same one every
/// time would be memorised rather than played. It changes it *deterministically*
/// — the run counter is simulation state like any other — so a recorded script
/// replayed from a fresh game meets the same horde.
#[must_use]
fn run_seed(seed: u64, runs: u32) -> u64 {
    crcbl::core::rand::salt(seed, u64::from(runs))
}

/// The index space for one spawn's draws.
///
/// The **only** index space in this game, so there is nothing for it to collide
/// with. Three bits of `which` leaves room for five more draws per spawn before
/// the counter has to move.
const fn spawn_index(counter: u64, which: u64) -> u64 {
    (counter << 3) | which
}

/// Which draw of a spawn is which.
const DRAW_RING_ANGLE: u64 = 0;
const DRAW_KIND: u64 = 1;
const DRAW_JITTER: u64 = 2;

/// Where the `counter`-th enemy of run `seed` enters, relative to the player.
///
/// **On a ring, never in the view.** An enemy that appeared inside the screen
/// would be indistinguishable from a rendering bug, and one that appeared on top
/// of the player would be damage the player had no chance to avoid. A ring is a
/// pure function *and* is provably outside the view, where a rejection loop
/// would be neither.
#[must_use]
pub fn spawn_offset(seed: u64, counter: u64) -> DVec3 {
    let angle = hash_unit(seed, spawn_index(counter, DRAW_RING_ANGLE)) * std::f64::consts::TAU;
    DVec3::new(angle.cos(), angle.sin(), 0.0) * SPAWN_RING
}

/// Which kind the `counter`-th enemy of run `seed` is.
#[must_use]
pub fn spawn_kind(seed: u64, counter: u64) -> EnemyKind {
    EnemyKind::from_roll(hash_unit(seed, spawn_index(counter, DRAW_KIND)))
}

/// The unit vector the `counter`-th enemy pushes along when it finds a
/// neighbour exactly on top of it.
///
/// **The tie-break separation cannot do without.** Two coincident bodies have no
/// direction between them, so the `away` vector is zero and the pair would sit
/// there forever — which is precisely the state the separation test asserts is
/// unreachable. Drawing it from the same seed as everything else keeps it out of
/// the determinism story.
#[must_use]
pub fn spawn_jitter(seed: u64, counter: u64) -> DVec3 {
    let angle = hash_unit(seed, spawn_index(counter, DRAW_JITTER)) * std::f64::consts::TAU;
    DVec3::new(angle.cos(), angle.sin(), 0.0)
}

// ---------------------------------------------------------------------------
// The arena's walls
// ---------------------------------------------------------------------------

/// Brings `v` inside `[-half, half]`, and leaves it **bit-exact** if it is
/// already there.
///
/// Exactness matters for the same reason asteroids' `wrap_axis` needed it:
/// `clamp_bodies` decides whether to write a transform back by comparing this
/// against the position it was given, and a round trip that returned a value one
/// ulp away would re-place every body in the broadphase on every tick.
///
/// `half` may be negative — an arena narrower than the body in it — in which
/// case the only point inside is the middle.
#[must_use]
pub fn clamp_axis(v: f64, half: f64) -> f64 {
    if half <= 0.0 {
        return 0.0;
    }
    v.clamp(-half, half)
}

/// Brings a body of `radius` fully inside the arena.
#[must_use]
pub fn clamp_to_arena(position: DVec3, radius: f64) -> DVec3 {
    DVec3::new(
        clamp_axis(position.x, ARENA_HALF_WIDTH - radius),
        clamp_axis(position.y, ARENA_HALF_HEIGHT - radius),
        position.z,
    )
}

// ---------------------------------------------------------------------------
// Input
// ---------------------------------------------------------------------------

const ACTION_UP: &str = "up";
const ACTION_DOWN: &str = "down";
const ACTION_LEFT: &str = "left";
const ACTION_RIGHT: &str = "right";
/// The one edge that both **starts** a waiting run and **restarts** a live one.
///
/// Two jobs on one action, the way asteroids' `fire` both begins a game and
/// deals a new one from the death screen. `R` and `Space` are both bound to it:
/// `Space` because that is the key the other three samples' start screens print,
/// and `R` because it is the one this game's death screen has always printed.
const ACTION_RESTART: &str = "restart";
/// The three level-up buttons, in offer order.
///
/// Bound to the digit row, and pressed by the level-up menu as **real key
/// events** rather than by calling into the simulation — the argument asteroids
/// makes for its `FLY` button, and it matters more here: which upgrade a run
/// took is simulation state a seeded script has to be able to replay.
const ACTION_CHOOSE: [&str; UPGRADE_CHOICES] = ["choose1", "choose2", "choose3"];

/// One tick of player intent.
///
/// **No fire button.** The weapon aims and fires itself; that is the genre and
/// it is also what makes the sample's workload honest, because the shots go out
/// at a fixed rate rather than at whatever rate a test harness taps a key.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
struct Intent {
    up: bool,
    down: bool,
    left: bool,
    right: bool,
    /// The start/restart key, on the tick it went down. An *edge*: held, it
    /// would restart the run sixty times a second.
    ///
    /// On [`GameState::WaitingToStart`] it begins the run rather than clearing
    /// it — see `run_tick`.
    restart: bool,
    /// Which level-up button was pressed this tick, one-based, or zero for
    /// none. An edge for the same reason `restart` is.
    choose: u8,
}

impl Intent {
    /// The direction these keys ask for, normalised so a diagonal is not faster
    /// than a straight line.
    fn direction(self) -> DVec3 {
        let x = f64::from(i8::from(self.right) - i8::from(self.left));
        let y = f64::from(i8::from(self.up) - i8::from(self.down));
        DVec3::new(x, y, 0.0).normalize_or_zero()
    }

    /// The wire form handed to `Client::set_input`.
    ///
    /// The choice takes the top two bits, which is enough for
    /// [`UPGRADE_CHOICES`] plus "none" and is asserted to be by
    /// `the_wire_form_carries_every_bit_of_intent`.
    fn to_wire(self) -> u8 {
        u8::from(self.up)
            | (u8::from(self.down) << 1)
            | (u8::from(self.left) << 2)
            | (u8::from(self.right) << 3)
            | (u8::from(self.restart) << 4)
            | ((self.choose & 0b11) << 5)
    }
}

// ---------------------------------------------------------------------------
// State
// ---------------------------------------------------------------------------

/// Where a run is.
///
/// **There is a "waiting to start", and it was argued against before it was
/// built.** The first cut of this game had none: breakout, flappy and asteroids
/// each open on a title screen because they open on a *board*, and this one's
/// board is empty at `t = 0`, so a waiting state is a blank arena with a prompt
/// on it. The user played it and asked for the screen anyway, which settles it —
/// a demo that starts taking hit points off the player before the window has
/// been looked at is worse than a blank arena, and four samples that open the
/// same way is worth more than one clever exception.
///
/// So the field a player looks at here is **empty but for the player**, not
/// frozen: there is nothing to freeze at `t = 0`, because everything on this
/// field is spawned by time passing.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum GameState {
    /// The title screen is up. The arena is empty, the clock is stopped and
    /// nothing spawns; `run_tick` short-circuits before it can move anything.
    /// The start edge — `R` or `Space` — begins the run.
    WaitingToStart,
    /// Running. The clock is going up and the horde is arriving.
    Playing,
    /// The level-up screen is up and the player is picking one of three. The
    /// whole field is frozen — see this module's header — and the run clock is
    /// stopped.
    LevelUp,
    /// The player's hit points reached zero. The clock is stopped and the kill
    /// count is frozen; the horde keeps moving, so the screen is a game and not
    /// a screenshot. Restart begins a new run.
    Dead,
}

/// One enemy.
#[derive(Clone, Copy, Debug)]
struct Enemy {
    entity: Entity,
    kind: EnemyKind,
    hp: f64,
    /// Where it was at the end of the last steering pass.
    ///
    /// Cached rather than read from physics twice: `steer_enemies` needs its
    /// own position *and* every neighbour's, and a neighbour reached through
    /// `PhysicsSystem::transform` is a second hash lookup on the hottest path in
    /// the game.
    position: DVec3,
    /// The direction it pushes when a neighbour is exactly on top of it. See
    /// [`spawn_jitter`].
    jitter: DVec3,
}

/// One bolt. No collider: see `sweep_bolts`.
#[derive(Clone, Copy, Debug)]
struct Bolt {
    entity: Entity,
    /// Seconds left before it expires.
    life: f64,
    /// The velocity it was fired at.
    ///
    /// Kept because `freeze_field` zeroes it for the level-up screen and
    /// nothing else could put it back: a bolt's direction is not recoverable
    /// from its position, and an enemy's is (`steer_enemies` recomputes one
    /// every tick).
    velocity: DVec3,
}

/// One dropped gem.
#[derive(Clone, Copy, Debug)]
struct Pickup {
    entity: Entity,
    position: DVec3,
    /// How much experience collecting it is worth. See [`EnemyKind::xp`].
    xp: u64,
}

/// What the renderer needs for one enemy.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct EnemyView {
    pub position: DVec3,
    pub kind: EnemyKind,
    /// What is left of its hit points, as a fraction in `[0, 1]`.
    pub health: f64,
}

/// What the renderer needs for one bolt.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct BoltView {
    pub position: DVec3,
}

/// What the renderer needs for one gem.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct PickupView {
    pub position: DVec3,
}

/// The mutable game state the server-side module owns.
#[derive(Debug)]
struct GameLogic {
    player: Entity,
    intent: Intent,
    state: GameState,

    player_pos: DVec3,
    player_hp: f64,
    /// Which way the wizard is turned, and whether it is being driven. Both are
    /// written by `drive_player` from the *intent*, so both replicate and a
    /// replay animates the same way the run it replays did. See [`Facing`].
    player_facing: Facing,
    player_moving: bool,

    /// Seconds until the next shot is allowed.
    fire_timer: f64,
    /// Seconds since the last spawn.
    spawn_timer: f64,
    /// How long this run has lasted, in simulated seconds. Stopped by death.
    elapsed: f64,
    /// How many enemies this run has killed.
    kills: u64,

    enemies: Vec<Enemy>,
    /// Where each live enemy is in [`Self::enemies`].
    ///
    /// **A map rather than a scan.** Both the bolt sweep and the contact query
    /// hand back entity ids, and resolving one by walking the enemy list is
    /// `O(N)` per hit — at the counts this sample exists to reach that is the
    /// difference between a tick and a stall. Maintained by `push_enemy` and
    /// `remove_enemy`, which are the only two places the list changes shape.
    by_entity: HashMap<Entity, usize>,
    bolts: Vec<Bolt>,

    /// The gems on the ground, and where each is in the list — the same pair
    /// [`Self::enemies`] and [`Self::by_entity`] are, for the same reason: both
    /// the collection query and the separation query hand back entity ids.
    pickups: Vec<Pickup>,
    pickup_by_entity: HashMap<Entity, usize>,

    /// Experience banked towards the next level, and which level the run is on.
    /// The run starts at level 1.
    xp: u64,
    level: u32,
    /// The three upgrades the level-up screen is offering, or `None` when it is
    /// not up. Refreshed by `enter_level_up` and consumed by `apply_choice`.
    offer: Option<[Upgrade; UPGRADE_CHOICES]>,
    /// The numbers this run has raised. See [`Stats`].
    stats: Stats,

    /// The seed the whole game was started with. The run actually in play is
    /// `run_seed` of this and [`Self::runs`].
    seed: u64,
    /// How many runs have been started. Simulation state, so a replay meets the
    /// same hordes in the same order.
    runs: u32,
    /// Every enemy ever spawned, counted so [`spawn_offset`] has an index.
    /// Never reset within a game, so two spawns never draw the same number.
    spawn_counter: u64,

    /// The ceiling on live enemies. See [`DEFAULT_MAX_ENEMIES`].
    max_enemies: usize,

    /// How many enemies have ever been put on the field, and how many bolts
    /// have ever left the gun.
    ///
    /// **Instrumentation, not mechanism** — nothing reads them to decide
    /// anything. They exist because the leak test's whole claim is "this ran a
    /// lot of churn and leaked nothing", and without a count of the churn the
    /// second half is true of a game that did nothing at all.
    enemies_spawned: u64,
    bolts_fired: u64,
    /// How many gems a full field refused to drop. See [`MAX_PICKUPS`].
    pickups_dropped: u64,

    /// Live views for the renderer, refilled rather than rebuilt so a
    /// steady-state tick does not allocate.
    enemy_views: Vec<EnemyView>,
    bolt_views: Vec<BoltView>,
    pickup_views: Vec<PickupView>,

    /// Scratch space for the per-tick passes, kept here for the same reason.
    scratch_entities: Vec<Entity>,

    /// Cues raised this tick, as `(sound id, where it happened)`.
    ///
    /// **Filled inside the tick and drained outside it**, by [`Game::tick`],
    /// which is what keeps an audio device out of a module that has to stay a
    /// pure function of its inputs. Nothing in the simulation ever reads this
    /// back, so a build with no sound is the same game as one with sound —
    /// which asteroids cannot say, because its thrust pulse is on a timer the
    /// tick owns. See `crate::audio`'s header.
    cues: Vec<(u32, DVec3)>,

    /// Ticks the module has actually run. The facade asserts this advances by
    /// exactly one per [`Game::tick`].
    ticks: u64,
}

impl GameLogic {
    /// The seed of the run in play.
    fn run(&self) -> u64 {
        run_seed(self.seed, self.runs)
    }
}

/// Adds an enemy to the list and the index in one place.
fn push_enemy(logic: &mut GameLogic, enemy: Enemy) {
    logic.by_entity.insert(enemy.entity, logic.enemies.len());
    logic.enemies.push(enemy);
}

/// Takes enemy `index` out of the list and the index in one place.
///
/// `swap_remove`, so the last enemy moves into the hole — and the map entry that
/// pointed at the end has to follow it, which is exactly the step that is silent
/// when it is forgotten. `an_enemy_index_survives_a_swap_remove` is the test.
fn remove_enemy(logic: &mut GameLogic, index: usize) -> Enemy {
    let removed = logic.enemies.swap_remove(index);
    logic.by_entity.remove(&removed.entity);
    if let Some(moved) = logic.enemies.get(index) {
        logic.by_entity.insert(moved.entity, index);
    }
    removed
}

/// The same pair for the gems.
fn push_pickup(logic: &mut GameLogic, pickup: Pickup) {
    logic
        .pickup_by_entity
        .insert(pickup.entity, logic.pickups.len());
    logic.pickups.push(pickup);
}

/// The same `swap_remove` and the same follow-up write. See [`remove_enemy`].
fn remove_pickup(logic: &mut GameLogic, index: usize) -> Pickup {
    let removed = logic.pickups.swap_remove(index);
    logic.pickup_by_entity.remove(&removed.entity);
    if let Some(moved) = logic.pickups.get(index) {
        logic.pickup_by_entity.insert(moved.entity, index);
    }
    removed
}

// ---------------------------------------------------------------------------
// The module
// ---------------------------------------------------------------------------

/// Per-tick game logic, run by the server after the ECS physics schedule.
///
/// `register` is empty for the same reason breakout's, flappy's and asteroids'
/// are: `Server::set_module` does not call it, and the physics system is
/// registered on the world in [`Game::with_setup`] before the server is built.
struct HordeModule {
    shared: Arc<Mutex<GameLogic>>,
}

impl std::fmt::Debug for HordeModule {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("HordeModule").finish_non_exhaustive()
    }
}

impl GameModule for HordeModule {
    fn name(&self) -> &str {
        "horde"
    }

    fn register(&self, _world: &mut World) {}

    fn tick(&mut self, world: &mut World) {
        let mut logic = lock(&self.shared);
        run_tick(&mut logic, world);
    }
}

/// A poisoned mutex here means a previous tick panicked. The game state is plain
/// data with no invariant a panic could have half-broken, so recovering the
/// guard is strictly better than taking the process down a second time.
fn lock(shared: &Mutex<GameLogic>) -> MutexGuard<'_, GameLogic> {
    shared.lock().unwrap_or_else(|e| e.into_inner())
}

/// One tick of horde, inside the server's tick, after physics has stepped.
///
/// **The order is load-bearing**, and the four places it is are:
///
/// * The **clamp runs first**, so everything below sees one consistent frame of
///   positions. Asteroids interleaves its wrap with its queries and pays for it
///   with a second `read_ship`; putting the whole of the "bodies are where they
///   are allowed to be" step at the top means nothing after it has to ask
///   whether it is looking at pre- or post-wall positions.
/// * The **gun fires after the sweep, not before it**, which is where this
///   diverges from asteroids. A sweep is `prev → cur` *reconstructed* from the
///   body's velocity, and a bolt that was created this tick has not moved yet —
///   so sweeping it produces the segment it would have travelled to *arrive* at
///   the muzzle, which points backwards through the thing that fired it. Firing
///   after the sweep means a bolt's first sweep is its first real step, and
///   `start` is exactly the muzzle. Asteroids has the same order the other way
///   round and the same latent segment; recorded in `docs/backlog.md`.
/// * **Steering caches every enemy's position**, and everything after it —
///   contact damage, the views — is entitled to use that cache. Nothing between
///   `steer_enemies` and `refresh_views` may move a body.
/// * **Spawning is last**, so an enemy that arrives this tick is not steered,
///   not swept and not asked for contact damage until the tick after. An enemy
///   spawned into the middle of the pass would be steered from a position
///   nothing else in the pass knows about.
///
/// Two states short-circuit the whole of it. [`GameState::WaitingToStart`] does
/// it first and hardest — the title screen is up, so nothing below moves, spends
/// or spawns anything and the run clock does not start. [`GameState::LevelUp`]
/// does it after the restart edge: the field stays where `freeze_field` left it.
/// See this module's header for why that is one pass on entry rather than a
/// branch on the hot path.
fn run_tick(logic: &mut GameLogic, world: &mut World) {
    logic.ticks += 1;
    let dt = world.tick_dt();
    let intent = std::mem::take(&mut logic.intent);

    clamp_bodies(logic, world);
    read_player(logic, world);

    if logic.state == GameState::WaitingToStart {
        // **Nothing below runs while the title screen is up**, which is the
        // whole of "the game does not play itself before it is started": no
        // clock, no spawner, no gun, no contact damage. The views are still
        // refreshed, because the renderer draws this frame like any other.
        //
        // The start edge is *not* a `restart`: there is nothing to clear, and a
        // `restart` here would bump the run counter and deal the first run of
        // the session the second run's seed.
        if !intent.restart {
            refresh_views(logic, world);
            return;
        }
        logic.state = GameState::Playing;
    } else if intent.restart {
        restart(logic, world);
    }

    if logic.state == GameState::LevelUp {
        if intent.choose > 0 {
            apply_choice(logic, world, usize::from(intent.choose - 1));
        }
        refresh_views(logic, world);
        return;
    }

    if logic.state == GameState::Playing {
        drive_player(logic, world, intent);
    } else {
        // Nothing is driving the wizard, so it is standing still whatever keys
        // are down. Without this a player who dies holding a direction gets a
        // corpse walking on the spot behind the death screen, because the last
        // tick that ran `drive_player` left the flag set.
        logic.player_moving = false;
    }

    sweep_bolts(logic, world, dt);
    expire_bolts(logic, world, dt);

    if logic.state == GameState::Playing {
        fire(logic, world, dt);
    } else {
        // The cooldown still runs down, so a restart never inherits a stale
        // timer from the run before it.
        logic.fire_timer = (logic.fire_timer - dt).max(0.0);
    }

    // Unconditional on the state: the horde keeps converging behind the death
    // screen, which is what makes it look like a game rather than a screenshot.
    steer_enemies(logic, world);

    if logic.state == GameState::Playing {
        contact_damage(logic, world, dt);
        collect_pickups(logic, world);
        spawn_enemies(logic, world, dt);
        logic.elapsed += dt;
        // Last, and guarded again: a tick that both banked the level and ran
        // the player out of hit points is a death, not a level-up. The screen
        // it would otherwise open has no way back.
        if logic.state == GameState::Playing {
            maybe_level_up(logic, world);
        }
    }

    refresh_views(logic, world);
}

// ---------------------------------------------------------------------------
// The arena's walls
// ---------------------------------------------------------------------------

/// Brings the player and every enemy back inside the arena.
///
/// # A clamp is not a teleport, and this is the difference from asteroids
///
/// Asteroids' wrap moves a body the whole width of the field, which is why that
/// sample takes its colliders out of the broadphase and puts them back — a leaf
/// dragged 32 units leaves every ancestor on the path to the root stretched
/// across the whole world. A clamp moves a body by **at most one tick of
/// travel** past the wall: at [`EnemyKind::Runner`]'s 5.6 units a second and 60
/// Hz that is 0.093 of a unit, which is smaller than the body. So this is a
/// continuation, not a discontinuity, and `PhysicsSystem::set_transform` — a
/// refit along one root-to-leaf path — is the right call rather than a
/// remove-and-re-insert.
///
/// Only bodies that are actually outside are written, which is what makes
/// [`clamp_axis`]' bit-exactness load-bearing: an inexact round trip would make
/// this an unconditional `N` transform writes a tick.
fn clamp_bodies(logic: &mut GameLogic, world: &mut World) {
    let player = logic.player;
    let mut enemies = std::mem::take(&mut logic.enemies);
    with_physics(world, |phys| {
        clamp_one(phys, player, PLAYER_RADIUS);
        for enemy in &mut enemies {
            if let Some(position) = clamp_one(phys, enemy.entity, enemy.kind.radius()) {
                enemy.position = position;
            }
        }
    });
    logic.enemies = enemies;
}

/// Clamps one body, and reports where it ended up if it moved.
fn clamp_one(phys: &mut PhysicsSystem, entity: Entity, radius: f64) -> Option<DVec3> {
    let transform = phys.transform(entity).copied()?;
    let clamped = clamp_to_arena(transform.position, radius);
    if clamped == transform.position {
        return None;
    }
    phys.set_transform(entity, Transform::new(clamped, transform.rotation));
    Some(clamped)
}

// ---------------------------------------------------------------------------
// The player
// ---------------------------------------------------------------------------

/// Writes the player's velocity for the coming integration step.
///
/// Straight to `velocity`, not through a force: see this module's header. The
/// body is kinematic, so [`PhysicsSystem::apply_force`] would be a no-op on it
/// and there is nothing to be gained by pretending otherwise.
///
/// It is also where the wizard's *drawing* is decided, because both halves of
/// that are properties of the intent rather than of anything physics gives back.
/// [`Facing`] says why it is the intent and not the velocity; the walk cycle
/// plays exactly when there is a direction being asked for, so a wizard held
/// against a wall keeps walking on the spot — which is what a player pushing
/// into a wall is doing.
fn drive_player(logic: &mut GameLogic, world: &mut World, intent: Intent) {
    let player = logic.player;
    let direction = intent.direction();
    // Only a horizontal key turns the figure, and only one of them: with both
    // down, or neither, there is nothing being asked for and the wizard keeps
    // the facing it had.
    if intent.left != intent.right {
        logic.player_facing = if intent.right {
            Facing::Right
        } else {
            Facing::Left
        };
    }
    logic.player_moving = direction != DVec3::ZERO;
    let velocity = direction * logic.stats.player_speed;
    with_physics(world, |phys| {
        if let Some(mut body) = phys.body(player).copied() {
            body.velocity = velocity;
            phys.set_body(player, body);
        }
    });
}

/// Fires at the nearest enemy in range, if the cooldown has run out.
///
/// # Aiming is a broadphase query, not a scan
///
/// The obvious auto-aim walks the enemy list and keeps the closest, which is
/// `O(N)` a tick — small beside the `N` queries `steer_enemies` already runs,
/// and still `N` work to find one thing inside a 13-unit circle on a 96-unit
/// arena. One [`PhysicsSystem::overlap_sphere`] at [`WEAPON_RANGE`] hands back
/// only what is in that circle.
///
/// The [`crcbl::phys::ShapeHit`] each result carries is **discarded**: it is
/// fabricated (`t: 0.0`, normal `+Y`, `started_inside: true` for every result,
/// recorded in `docs/backlog.md`), and all this query is asked is *what* is
/// there.
///
/// Ties are broken by entity id, not left to the order the broadphase happens to
/// return: two enemies at exactly the same distance are common in this game —
/// separation pushes pairs into symmetric positions constantly — and a target
/// chosen by tree order is a target that changes when the tree is rebalanced.
fn fire(logic: &mut GameLogic, world: &mut World, dt: f64) {
    logic.fire_timer = (logic.fire_timer - dt).max(0.0);
    if logic.fire_timer > 0.0 {
        return;
    }

    let origin = logic.player_pos;
    let range = logic.stats.weapon_range;
    // **The filter that stops the gun aiming at the loot.** A dropped gem is a
    // trigger collider and `overlap_sphere` does not skip triggers, so without
    // this a player standing over a gem in an empty field would fire at their
    // own XP forever. Taken out and put back rather than borrowed, because the
    // closure below holds the physics system for the whole query.
    let by_entity = std::mem::take(&mut logic.by_entity);
    let target = with_physics(world, |phys| {
        phys.overlap_sphere(origin, range)
            .into_iter()
            .filter(|(entity, _hit)| by_entity.contains_key(entity))
            .filter_map(|(entity, _hit)| {
                let position = phys.transform(entity)?.position;
                Some((entity, position))
            })
            .min_by(|(a_entity, a), (b_entity, b)| {
                (*a - origin)
                    .length_squared()
                    .total_cmp(&(*b - origin).length_squared())
                    .then_with(|| a_entity.to_bits().cmp(&b_entity.to_bits()))
            })
    })
    .flatten();
    logic.by_entity = by_entity;

    // No cooldown is spent on an empty field: the gun is ready the instant
    // something walks into range, which is what stops the first enemy of a wave
    // living a quarter of a second longer than the rest.
    let Some((_, aim)) = target else {
        return;
    };
    // **Chosen from the player's centre, aimed from the staff.** The range that
    // decides what is shootable is a property of the *player*, and a query
    // centred on a muzzle that moves with the facing would put a different set
    // of enemies in reach depending on which way the wizard happened to be
    // turned. Where the bolt actually goes is another matter: it leaves the head
    // of the staff, which is up and off to one side, so a direction taken from
    // the centre would send it along a line parallel to the one that hits and
    // half a unit beside it — far enough to miss a runner outright. The staff
    // points at the target; see [`staff_muzzle`] for what that looks like when
    // the wizard is facing the other way.
    let position = origin + staff_muzzle(logic.player_facing);
    let Some(direction) = (aim - position).try_normalize() else {
        // The target is exactly on the staff head. There is no direction to fire
        // in, and contact damage is already dealing with it.
        return;
    };
    logic.fire_timer = logic.stats.fire_cooldown;

    let velocity = direction * BOLT_SPEED;

    let entity = world.spawn();
    with_physics(world, |phys| {
        let mut body = RigidBody::new_kinematic();
        body.velocity = velocity;
        phys.set_body(entity, body);
        phys.set_transform(entity, Transform::from_position(position));
    });
    logic.bolts.push(Bolt {
        entity,
        life: BOLT_LIFE,
        velocity,
    });
    logic.bolts_fired += 1;
    // At the muzzle rather than at the player's centre: the two are half a unit
    // apart and inaudible, and the point is that a cue is raised where the
    // *event* is — a bolt appearing.
    logic.cues.push((crate::audio::SOUND_SHOT, position));
}

/// Applies one tick of contact damage, and kills the player if it runs them out.
///
/// # One query, and every result is a hit
///
/// [`crcbl::phys::PhysicsWorld::overlap_sphere`] tests the query sphere against
/// each collider's *shape*, so a query of [`PLAYER_RADIUS`] returns exactly the
/// enemies whose centres are within `PLAYER_RADIUS + r_enemy` — which is the
/// definition of touching. There is no second distance test here because there
/// is nothing left to reject.
///
/// # The player is not in the broadphase, and this is why
///
/// The player is the *subject* of every overlap test in this game and the
/// *target* of none: bolts are aimed away from it and enemies test nothing.
/// A collider for it would be a leaf that this query would return every single
/// tick and that `sweep_bolts` would have to filter back out. That falls out
/// of the shape of the API rather than of this game —
/// `PhysicsSystem::overlap_sphere` takes a free centre, so an entity that only
/// ever *asks* has no reason to be in the tree, and there is no entity-shaped
/// overlap with an exclusion list. Recorded in `docs/backlog.md`.
///
/// **Every collider in this world is an enemy**, which is what makes the leak
/// test's collider count an equality rather than a bound.
fn contact_damage(logic: &mut GameLogic, world: &mut World, dt: f64) {
    let centre = logic.player_pos;
    let by_entity = std::mem::take(&mut logic.by_entity);
    let enemies = std::mem::take(&mut logic.enemies);
    let dps = with_physics(world, |phys| {
        phys.overlap_sphere(centre, PLAYER_RADIUS)
            .into_iter()
            .filter_map(|(entity, _hit)| by_entity.get(&entity).copied())
            .filter_map(|index| enemies.get(index))
            .map(|enemy| enemy.kind.contact_dps())
            .sum::<f64>()
    })
    .unwrap_or(0.0);
    logic.by_entity = by_entity;
    logic.enemies = enemies;

    if dps <= 0.0 {
        return;
    }
    logic.player_hp -= dps * dt;
    if logic.player_hp <= 0.0 {
        logic.player_hp = 0.0;
        logic.state = GameState::Dead;
        logic
            .cues
            .push((crate::audio::SOUND_DEATH, logic.player_pos));
        crcbl::log::info!(
            "died after {:.1}s with {} kills, {} enemies on the field",
            logic.elapsed,
            logic.kills,
            logic.enemies.len(),
        );
    }
}

// ---------------------------------------------------------------------------
// Bolts
// ---------------------------------------------------------------------------

/// Sweeps every bolt along the path it took this tick and resolves what it hit.
///
/// **This is the "never miss at any speed" half of the plan, and it is written
/// by hand.** `crcbl-phys` has the machinery — [`PhysicsSystem::sweep_sphere`]
/// takes a [`Segment`] and a radius — and no bullet-shaped entry point, so every
/// game that fires anything writes this same "from where it was to where it is"
/// itself. `docs/backlog.md` records that; this is the third consumer to decide
/// it is still worth writing rather than worked around.
///
/// A bolt therefore has **no collider**. It is a query, not a body in the
/// broadphase: giving it one would put the bolt's own shape at the far end of
/// its own sweep, where it reports hitting itself at `t = 0`, and would need a
/// remove-and-re-insert per bolt per tick to avoid.
fn sweep_bolts(logic: &mut GameLogic, world: &mut World, dt: f64) {
    if logic.bolts.is_empty() {
        return;
    }

    // `(bolt index, enemy entity)` for everything that connected this tick.
    let mut hits: Vec<(usize, Entity)> = Vec::new();
    let bolts = logic.bolts.clone();
    with_physics(world, |phys| {
        for (index, bolt) in bolts.iter().enumerate() {
            let Some((body, transform)) = phys
                .body(bolt.entity)
                .copied()
                .zip(phys.transform(bolt.entity).copied())
            else {
                continue;
            };
            let segment = Segment {
                start: transform.position - body.velocity * dt,
                end: transform.position,
            };
            if let Some((entity, _hit)) = phys.sweep_sphere(&segment, BOLT_RADIUS) {
                hits.push((index, entity));
            }
        }
    });

    // Highest index first, so removing one bolt does not move the next.
    for &(index, hit) in hits.iter().rev() {
        let Some(bolt) = logic.bolts.get(index).copied() else {
            continue;
        };
        despawn_bolt(world, bolt.entity);
        logic.bolts.remove(index);
        // Two bolts can reach the same enemy on the same tick. The first may
        // kill it; the second finds an entity that is no longer an enemy and is
        // spent without scoring, which is not a double kill.
        if let Some(&target) = logic.by_entity.get(&hit) {
            damage_enemy(logic, world, target, logic.stats.bolt_damage);
        }
    }
}

/// Ages every bolt and destroys the ones that have run out.
fn expire_bolts(logic: &mut GameLogic, world: &mut World, dt: f64) {
    let mut dead = std::mem::take(&mut logic.scratch_entities);
    dead.clear();
    logic.bolts.retain_mut(|bolt| {
        bolt.life -= dt;
        if bolt.life > 0.0 {
            return true;
        }
        dead.push(bolt.entity);
        false
    });
    for entity in dead.drain(..) {
        despawn_bolt(world, entity);
    }
    logic.scratch_entities = dead;
}

/// Destroys a bolt, in the physics world and in the ECS.
///
/// Both, and in that order — the failure mode a game with this much churn would
/// produce a hundred times a minute is a body left behind when its entity goes.
fn despawn_bolt(world: &mut World, entity: Entity) {
    with_physics(world, |phys| phys.remove_entity(entity));
    world.despawn(entity);
}

// ---------------------------------------------------------------------------
// Enemies
// ---------------------------------------------------------------------------

/// Takes `amount` off enemy `index`, and kills it if that empties it.
fn damage_enemy(logic: &mut GameLogic, world: &mut World, index: usize, amount: f64) {
    let Some(enemy) = logic.enemies.get_mut(index) else {
        return;
    };
    enemy.hp -= amount;
    if enemy.hp > 0.0 {
        return;
    }
    let dead = remove_enemy(logic, index);
    with_physics(world, |phys| phys.remove_entity(dead.entity));
    world.despawn(dead.entity);
    logic.kills += 1;
    logic.cues.push((crate::audio::SOUND_KILL, dead.position));
    drop_pickup(logic, world, dead.position, dead.kind.xp());
}

// ---------------------------------------------------------------------------
// Experience
// ---------------------------------------------------------------------------

/// Leaves a gem where an enemy died, if the field has room for one.
///
/// The collider is a **trigger**, which is the whole of how a gem stays out of
/// the game's other three queries: `crcbl::phys` skips triggers in the sweep, so
/// a bolt flies through it; `fire` and `steer_enemies` filter theirs back out
/// through the enemy index they already consult. See [`XP_RADIUS`].
fn drop_pickup(logic: &mut GameLogic, world: &mut World, position: DVec3, xp: u64) {
    if logic.pickups.len() >= MAX_PICKUPS {
        logic.pickups_dropped += 1;
        return;
    }
    let position = clamp_to_arena(position, XP_RADIUS);
    let entity = world.spawn();
    let transform = Transform::from_position(position);
    with_physics(world, |phys| {
        phys.set_collider(
            entity,
            &ColliderComponent::Sphere {
                offset: DVec3::ZERO,
                radius: XP_RADIUS,
                is_trigger: true,
            },
            &transform,
        );
    });
    push_pickup(
        logic,
        Pickup {
            entity,
            position,
            xp,
        },
    );
}

/// Banks every gem the player is standing on.
///
/// **One query, and the radius is exact for the same reason contact damage's
/// is**: a shape-aware overlap of radius `R` returns every collider whose centre
/// is within `R + r_b`, so querying at `stats.pickup_radius` picks up exactly
/// the gems whose surface the player is touching. [`Upgrade::Magnet`] raises
/// that radius and nothing else changes.
///
/// The query also returns enemies — they are colliders too — and the
/// `pickup_by_entity` lookup is what rejects them.
fn collect_pickups(logic: &mut GameLogic, world: &mut World) {
    if logic.pickups.is_empty() {
        return;
    }
    let centre = logic.player_pos;
    let radius = logic.stats.pickup_radius;
    let pickup_by_entity = std::mem::take(&mut logic.pickup_by_entity);
    let mut taken = std::mem::take(&mut logic.scratch_entities);
    taken.clear();
    with_physics(world, |phys| {
        for (entity, _hit) in phys.overlap_sphere(centre, radius) {
            if pickup_by_entity.contains_key(&entity) {
                taken.push(entity);
            }
        }
    });
    logic.pickup_by_entity = pickup_by_entity;

    for entity in taken.drain(..) {
        let Some(&index) = logic.pickup_by_entity.get(&entity) else {
            continue;
        };
        let gem = remove_pickup(logic, index);
        with_physics(world, |phys| phys.remove_entity(gem.entity));
        world.despawn(gem.entity);
        logic.xp += gem.xp;
        logic.cues.push((crate::audio::SOUND_PICKUP, gem.position));
    }
    logic.scratch_entities = taken;
}

/// Opens the level-up screen if the run has banked enough experience.
fn maybe_level_up(logic: &mut GameLogic, world: &mut World) {
    if logic.xp < xp_for_next_level(logic.level) {
        return;
    }
    logic.xp -= xp_for_next_level(logic.level);
    logic.level += 1;
    logic.offer = Some(upgrade_offer(logic.run(), logic.level));
    logic.state = GameState::LevelUp;
    // On the player, not out in the field: this is the one cue in the game that
    // is about the *run* rather than about something that happened somewhere,
    // so it is heard dead centre at full volume.
    logic
        .cues
        .push((crate::audio::SOUND_LEVEL, logic.player_pos));
    freeze_field(logic, world);
    crcbl::log::info!(
        "level {} at {:.1}s, offering {:?}",
        logic.level,
        logic.elapsed,
        logic.offer,
    );
}

/// Takes the `index`-th upgrade of the offer and puts the field back in motion.
///
/// A choice out of range is ignored rather than clamped: it can only come from a
/// caller that made one up, and silently taking a different upgrade from the one
/// asked for is worse than doing nothing.
///
/// **One more threshold may already be crossed** — a brute's gem is five
/// experience against a step of four — so this re-checks and opens the next
/// screen rather than banking a level the player never chose for.
fn apply_choice(logic: &mut GameLogic, world: &mut World, index: usize) {
    let Some(upgrade) = logic.offer.and_then(|offer| offer.get(index).copied()) else {
        return;
    };
    apply_upgrade(logic, upgrade);
    crcbl::log::info!("took {} at level {}", upgrade.label(), logic.level);
    logic.offer = None;
    logic.state = GameState::Playing;
    thaw_field(logic, world);
    maybe_level_up(logic, world);
}

/// What one upgrade does. One line each, which is the point of the pool.
fn apply_upgrade(logic: &mut GameLogic, upgrade: Upgrade) {
    let stats = &mut logic.stats;
    match upgrade {
        Upgrade::RapidFire => {
            stats.fire_cooldown = (stats.fire_cooldown * 0.85).max(FIRE_COOLDOWN_FLOOR);
        }
        Upgrade::HeavyBolts => stats.bolt_damage += 2.0,
        Upgrade::SwiftBoots => stats.player_speed += 0.6,
        Upgrade::LongBarrel => stats.weapon_range += 2.0,
        Upgrade::Vitality => {
            stats.max_hp += 25.0;
            // Healed on the spot as well, or the upgrade is a promise that only
            // pays off after the next twenty-five points of damage.
            logic.player_hp = (logic.player_hp + 25.0).min(stats.max_hp);
        }
        Upgrade::Magnet => stats.pickup_radius += 1.0,
    }
}

/// Stops everything that is moving, once, for the level-up screen.
///
/// See this module's header: the integrator runs before the game module every
/// tick, so a frozen field is one whose velocities are all zero rather than one
/// the module keeps stepping over.
fn freeze_field(logic: &mut GameLogic, world: &mut World) {
    let player = logic.player;
    let entities: Vec<Entity> = std::iter::once(player)
        .chain(logic.enemies.iter().map(|enemy| enemy.entity))
        .chain(logic.bolts.iter().map(|bolt| bolt.entity))
        .collect();
    with_physics(world, |phys| {
        for entity in entities {
            if let Some(mut body) = phys.body(entity).copied() {
                body.velocity = DVec3::ZERO;
                phys.set_body(entity, body);
            }
        }
    });
}

/// Hands the bolts their velocities back.
///
/// Only the bolts: `drive_player` and `steer_enemies` both write a fresh
/// velocity on the first tick the game is playing again, and a bolt has nothing
/// that would.
fn thaw_field(logic: &mut GameLogic, world: &mut World) {
    let bolts: Vec<(Entity, DVec3)> = logic
        .bolts
        .iter()
        .map(|bolt| (bolt.entity, bolt.velocity))
        .collect();
    with_physics(world, |phys| {
        for (entity, velocity) in bolts {
            if let Some(mut body) = phys.body(entity).copied() {
                body.velocity = velocity;
                phys.set_body(entity, body);
            }
        }
    });
}

/// Seeks the player, and pushes off the neighbours, for every enemy on the
/// field.
///
/// # This is the workload the sample exists to produce
///
/// The pattern is **one [`PhysicsSystem::overlap_sphere`] per enemy per tick**,
/// centred on that enemy, of radius `r_self + `[`SEPARATION_SLACK`]. Not an `N²`
/// loop over the enemy list, and not a hand-rolled grid: the plan's claim is
/// that the engine's broadphase carries this, so the sample has to ask it to.
///
/// The cost shape, per tick, is therefore:
///
/// * `N` BVH descents, each `O(log N)` in the tree's depth — for the AVL-bounded
///   tree `crcbl-phys` landed in slice 16 that is the whole of the query's
///   *search* cost;
/// * plus one exact sphere-versus-sphere test per candidate the descent turns
///   up, so the total is `O(N log N + Σk)` where `k` is a neighbourhood size —
///   which [`SEPARATION_SLACK`] is the tuning knob for, and which is bounded by
///   how densely bodies of a given radius can be packed rather than by `N`;
/// * and **no allocations at all**, once the one `neighbours` buffer below has
///   grown. `PhysicsSystem::overlap_sphere_into` clears and refills a buffer
///   the caller owns, the collider ids land in a scratch buffer of the
///   system's, and the BVH's descent stack and candidate list are the world's
///   own. The owned `overlap_sphere` this used to call cost three `Vec`s per
///   enemy per tick — 1.8 million a second at the plan's ten thousand, every
///   one of them dropped immediately.
/// * plus `N` hash **lookups** to write the velocities, through
///   `PhysicsSystem::body_mut`. This used to be `N` `set_body` calls, which is
///   an insert into the body map plus a touch of the transform map — two hash
///   operations per enemy per tick to change one `DVec3`.
///
/// # It is order-independent, and that is a property rather than an accident
///
/// Nothing in this pass moves a body. `body_mut` writes a velocity, and a
/// velocity is not read by the broadphase — so every enemy's query sees the same
/// world whatever order the loop visits them in, and the result does not depend
/// on the enemy list's ordering. That is why the positions can be cached once at
/// the top and why `remove_enemy`'s `swap_remove` is free to shuffle the list.
///
/// The sum over neighbours **is** floating-point order-dependent, and the order
/// is the BVH's traversal order. That is deterministic — the tree is a pure
/// function of the sequence of inserts and removes, which is itself a pure
/// function of the seed and the script — so two runs of one script agree, which
/// is what `the_same_script_replays_bit_identically` checks. Sorting the
/// neighbourhood would make it independent of the *tree* as well, at the price
/// of a sort per enemy per tick; it is not worth it and the decision is recorded
/// in `docs/backlog.md`.
fn steer_enemies(logic: &mut GameLogic, world: &mut World) {
    if logic.enemies.is_empty() {
        return;
    }
    let mut enemies = std::mem::take(&mut logic.enemies);
    let by_entity = std::mem::take(&mut logic.by_entity);
    let player = logic.player_pos;

    with_physics(world, |phys| {
        // One read of the authoritative positions, so the loop below never has
        // to go back for a neighbour's.
        for enemy in &mut enemies {
            if let Some(transform) = phys.transform(enemy.entity) {
                enemy.position = transform.position;
            }
        }

        // One buffer for the whole pass: `overlap_sphere_into` clears and
        // refills it, and nothing below it allocates either, so `N` queries a
        // tick cost no allocations at all once it has grown.
        let mut neighbours = Vec::new();
        for me in &enemies {
            let mut push = DVec3::ZERO;
            phys.overlap_sphere_into(
                me.position,
                separation_query_radius(me.kind),
                &mut neighbours,
            );
            for &(other, _hit) in neighbours.iter() {
                if other == me.entity {
                    continue;
                }
                let Some(them) = by_entity.get(&other).and_then(|index| enemies.get(*index)) else {
                    continue;
                };
                push += separation_push(me, them);
            }

            let seek = (player - me.position).normalize_or_zero() * me.kind.speed();
            let velocity = seek + clamp_length(push, 1.0) * SEPARATION_STRENGTH;
            if let Some(body) = phys.body_mut(me.entity) {
                body.velocity = velocity;
            }
        }
    });

    logic.enemies = enemies;
    logic.by_entity = by_entity;
}

/// How hard `me` is pushed away from `them`, as a weight in `[0, 1]` along the
/// line between them.
///
/// One at full overlap, zero at the edge of the neighbourhood, linear between.
/// The coincident case — two bodies at exactly the same point, which a horde
/// converging on one player produces constantly — has no line between them, so
/// it falls back to `Enemy::jitter`, a per-enemy direction drawn from the
/// seed. Without it a coincident pair is a fixed point of this function and the
/// two never come apart.
fn separation_push(me: &Enemy, them: &Enemy) -> DVec3 {
    let away = me.position - them.position;
    let distance_squared = away.length_squared();
    if distance_squared <= f64::EPSILON {
        return me.jitter;
    }
    let distance = distance_squared.sqrt();
    let desired = me.kind.radius() + them.kind.radius() + SEPARATION_SLACK;
    let weight = ((desired - distance) / desired).clamp(0.0, 1.0);
    away / distance * weight
}

/// `v`, shortened to `max` if it is longer than that.
#[must_use]
fn clamp_length(v: DVec3, max: f64) -> DVec3 {
    let length_squared = v.length_squared();
    if length_squared <= max * max {
        return v;
    }
    v / length_squared.sqrt() * max
}

/// Puts one enemy on the field.
fn spawn_enemy(
    logic: &mut GameLogic,
    world: &mut World,
    kind: EnemyKind,
    position: DVec3,
    jitter: DVec3,
) -> Entity {
    let position = clamp_to_arena(position, kind.radius());
    let entity = world.spawn();
    let transform = Transform::from_position(position);
    with_physics(world, |phys| {
        // Kinematic: this body's velocity is chosen, not integrated. See the
        // module header.
        phys.set_body(entity, RigidBody::new_kinematic());
        phys.set_collider(entity, &kind.collider(), &transform);
    });
    push_enemy(
        logic,
        Enemy {
            entity,
            kind,
            hp: kind.max_hp(),
            position,
            jitter,
        },
    );
    logic.enemies_spawned += 1;
    entity
}

/// Puts however many enemies this tick is owed on the field.
///
/// The timer is consumed whether or not there is room under
/// `GameLogic::max_enemies`, so a field that has been full for a minute does
/// not release a minute's worth of spawns the instant something dies.
fn spawn_enemies(logic: &mut GameLogic, world: &mut World, dt: f64) {
    logic.spawn_timer += dt;
    let mut spawned = 0;
    while spawned < SPAWN_BURST_CAP {
        let interval = spawn_interval(logic.elapsed);
        if logic.spawn_timer < interval {
            break;
        }
        logic.spawn_timer -= interval;
        spawned += 1;

        let counter = logic.spawn_counter;
        logic.spawn_counter += 1;
        if logic.enemies.len() >= logic.max_enemies {
            continue;
        }
        let seed = logic.run();
        spawn_enemy(
            logic,
            world,
            spawn_kind(seed, counter),
            logic.player_pos + spawn_offset(seed, counter),
            spawn_jitter(seed, counter),
        );
    }
    // A burst that hit the cap must not leave a backlog that bursts again next
    // tick: whatever is left over is dropped rather than owed.
    if spawned >= SPAWN_BURST_CAP {
        logic.spawn_timer = 0.0;
    }
}

// ---------------------------------------------------------------------------
// Restart and read-back
// ---------------------------------------------------------------------------

/// Clears the field and deals a run that is not the one just played.
///
/// **It lands on the title screen, not in play** — the same as asteroids'
/// `restart` and flappy's `reset`. `TRY AGAIN` therefore takes two presses to
/// get back into a run, and that is the point: a run that begins on the frame a
/// player is still mashing the key on the death screen is a run they die in
/// again immediately.
fn restart(logic: &mut GameLogic, world: &mut World) {
    for enemy in std::mem::take(&mut logic.enemies) {
        with_physics(world, |phys| phys.remove_entity(enemy.entity));
        world.despawn(enemy.entity);
    }
    logic.by_entity.clear();
    for bolt in std::mem::take(&mut logic.bolts) {
        despawn_bolt(world, bolt.entity);
    }
    for gem in std::mem::take(&mut logic.pickups) {
        with_physics(world, |phys| phys.remove_entity(gem.entity));
        world.despawn(gem.entity);
    }
    logic.pickup_by_entity.clear();
    logic.runs = logic.runs.wrapping_add(1);
    logic.state = GameState::WaitingToStart;
    // **Every upgrade comes off.** The plan's non-goals bar meta-progression,
    // and a `Stats::default()` here is what makes that a property of the code
    // rather than of nobody having written the carry-over yet.
    logic.stats = Stats::default();
    logic.player_hp = logic.stats.max_hp;
    logic.xp = 0;
    logic.level = 1;
    logic.offer = None;
    logic.elapsed = 0.0;
    logic.kills = 0;
    logic.fire_timer = 0.0;
    logic.spawn_timer = 0.0;
    logic.spawn_counter = 0;
    place_player(logic, world, DVec3::ZERO);
}

/// Puts the player in the middle of the arena, stationary.
///
/// Stationary in the drawing as well as in the physics: the walk cycle and the
/// facing both go back to where a fresh wizard starts, so a run that ended
/// mid-stride does not deal the next one a figure already turned and walking.
fn place_player(logic: &mut GameLogic, world: &mut World, position: DVec3) {
    let player = logic.player;
    logic.player_pos = position;
    logic.player_facing = Facing::default();
    logic.player_moving = false;
    with_physics(world, |phys| {
        phys.set_body(player, RigidBody::new_kinematic());
        phys.set_transform(player, Transform::from_position(position));
    });
}

/// Copies the authoritative state the renderer needs out of the simulation.
///
/// The enemy positions come from [`Enemy::position`] rather than from a fresh
/// pass over `PhysicsSystem::transform`: `steer_enemies` has just refreshed
/// them and nothing since has moved a body, so a second `N` hash lookups a tick
/// would buy nothing. See `run_tick`'s note on the order.
fn refresh_views(logic: &mut GameLogic, world: &mut World) {
    let mut enemy_views = std::mem::take(&mut logic.enemy_views);
    enemy_views.clear();
    enemy_views.extend(logic.enemies.iter().map(|enemy| EnemyView {
        position: enemy.position,
        kind: enemy.kind,
        health: (enemy.hp / enemy.kind.max_hp()).clamp(0.0, 1.0),
    }));
    logic.enemy_views = enemy_views;

    // Straight off `Pickup::position`, which never changes: a gem is dropped
    // where an enemy died and stays there until it is walked over.
    let mut pickup_views = std::mem::take(&mut logic.pickup_views);
    pickup_views.clear();
    pickup_views.extend(logic.pickups.iter().map(|gem| PickupView {
        position: gem.position,
    }));
    logic.pickup_views = pickup_views;

    let bolts: Vec<Entity> = logic.bolts.iter().map(|bolt| bolt.entity).collect();
    let mut bolt_views = std::mem::take(&mut logic.bolt_views);
    bolt_views.clear();
    let player = logic.player;
    let position = with_physics(world, |phys| {
        for entity in bolts {
            if let Some(transform) = phys.transform(entity) {
                bolt_views.push(BoltView {
                    position: transform.position,
                });
            }
        }
        phys.transform(player).map(|t| t.position)
    })
    .flatten();
    logic.bolt_views = bolt_views;
    if let Some(position) = position {
        logic.player_pos = position;
    }
}

/// Copies the player's authoritative position out of the physics world.
fn read_player(logic: &mut GameLogic, world: &mut World) {
    let player = logic.player;
    if let Some(Some(position)) =
        with_physics(world, |phys| phys.transform(player).map(|t| t.position))
    {
        logic.player_pos = position;
    }
}

/// Runs `f` against the world's physics system, if it has one.
fn with_physics<R>(world: &mut World, f: impl FnOnce(&mut PhysicsSystem) -> R) -> Option<R> {
    world.system_mut::<PhysicsSystem>().map(f)
}

// ---------------------------------------------------------------------------
// Game — the client-side facade
// ---------------------------------------------------------------------------

/// Everything the renderer draws, in world space.
///
/// Filled through [`Game::render_state`], which reuses the caller's allocations
/// — this game hands over a fresh enemy list every frame forever, and at the
/// counts the plan asks for that list is the largest thing in the process.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct RenderState {
    pub player: DVec3,
    /// Which way the wizard is turned. See [`Facing`].
    pub player_facing: Facing,
    /// Whether the wizard is being walked, which is what `art::Scene::build`
    /// chooses between the walk cycle and the standing frame on.
    ///
    /// The *intent's* answer, not the velocity's: see `drive_player`.
    pub player_walking: bool,
    /// What is left of the player's hit points, and the ceiling they are
    /// against — the ceiling moves, so the HUD cannot read it off a constant.
    pub player_hp: f64,
    pub player_max_hp: f64,
    pub enemies: Vec<EnemyView>,
    pub bolts: Vec<BoltView>,
    pub pickups: Vec<PickupView>,
    /// How long this run has lasted, in simulated seconds.
    pub elapsed: f64,
    pub kills: u64,
    /// Experience banked towards the next level, and how much that needs.
    pub xp: u64,
    pub xp_needed: u64,
    pub level: u32,
    /// The three upgrades on the level-up screen, or `None` when it is not up.
    pub offer: Option<[Upgrade; UPGRADE_CHOICES]>,
    pub state: Option<GameState>,
    /// The longest run this player has survived, in whole seconds.
    ///
    /// **The facade's, not the simulation's** — see [`Game::render_state`].
    pub best: u32,
}

/// How a [`Game`] is built.
///
/// A struct rather than four positional arguments, because `max_enemies` is a
/// knob the scale sub-slice will drive from the command line and from the
/// browser, and a fifth positional `u64` would be one transposition away from a
/// silent bug.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Setup {
    pub headless: bool,
    pub tick_hz: u32,
    pub seed: u64,
    pub max_enemies: usize,
}

impl Default for Setup {
    fn default() -> Self {
        Self {
            headless: false,
            tick_hz: DEFAULT_TICK_HZ,
            seed: DEFAULT_SEED,
            max_enemies: DEFAULT_MAX_ENEMIES,
        }
    }
}

pub struct Game {
    pub player_entity: Entity,
    action_map: ActionMap,
    /// The server, its client and the transport between them.
    ///
    /// One field rather than two: the tick rate, the compatibility and the
    /// transport pair are what both halves must agree on, and
    /// [`Loopback::new`] is where they are made to.
    session: Loopback,
    shared: Arc<Mutex<GameLogic>>,
    /// Exactly one tick period per [`Game::tick`], so the server's accumulator
    /// yields exactly one tick per call.
    tick_period: Duration,
    sim_time: Duration,
    ticks_run: u64,
    /// Queued key events from the shell pump, replayed after `begin_tick`.
    pending_keys: Vec<(KeyCode, bool)>,
    /// The output stream and the five cues. On the facade rather than in the
    /// simulation: the module runs inside the server's tick and must stay a pure
    /// function of its inputs, and an audio device is neither.
    pub audio: crate::audio::Audio,
    /// The longest run, and where it is kept.
    pub best: crate::best::Best,
    /// Mirrors of the shared state, refreshed after each tick so the render and
    /// HUD paths never take the lock.
    pub state: GameState,
    pub player: DVec3,
    pub player_hp: f64,
    pub elapsed: f64,
    pub kills: u64,
    pub level: u32,
    /// Which run is in play, counted from 1. Mirrors `GameLogic::runs + 1`.
    pub run: u32,
    prev_log_state: GameState,
    /// `elapsed` at the end of the previous tick.
    ///
    /// The only way the facade can see a **restart**: `run_tick` resets
    /// `elapsed` to zero, so a clock that went backwards is a run that ended
    /// without a death screen — and a four-minute run abandoned by pressing R
    /// is still the record. See [`Game::tick`].
    prev_elapsed: f64,
}

impl std::fmt::Debug for Game {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Game")
            .field("player_entity", &self.player_entity)
            .field("state", &self.state)
            .field("elapsed", &self.elapsed)
            .field("kills", &self.kills)
            .finish_non_exhaustive()
    }
}

#[derive(Debug)]
pub enum GameError {
    Server(String),
}

impl std::fmt::Display for GameError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Server(msg) => write!(f, "server creation failed: {msg}"),
        }
    }
}

impl std::error::Error for GameError {}

impl Game {
    /// Builds the world, the physics system, the server and the client on the
    /// published run.
    ///
    /// # Errors
    ///
    /// [`GameError::Server`] if the operating system would not give the server
    /// the entropy for a resume credential.
    ///
    /// # Panics
    ///
    /// If `tick_hz` is zero.
    pub fn new(headless: bool, tick_hz: u32) -> Result<Self, GameError> {
        Self::with_setup(&Setup {
            headless,
            tick_hz,
            ..Setup::default()
        })
    }

    /// The same, spelled out.
    ///
    /// `seed` decides every enemy of every run of this game — see [`hash_unit`]
    /// — so two games built with the same setup and fed the same input are the
    /// same game, which is what the determinism tests rest on.
    ///
    /// # Errors
    ///
    /// [`GameError::Server`] if the operating system would not give the server
    /// the entropy for a resume credential.
    ///
    /// # Panics
    ///
    /// If `tick_hz` is zero.
    pub fn with_setup(setup: &Setup) -> Result<Self, GameError> {
        assert!(setup.tick_hz > 0, "tick rate must be positive");
        let mut world = World::new();

        // **No force providers at all**, and no force-driven bodies either: every
        // body in this world is kinematic and carries a velocity the game wrote.
        // See the module header.
        world.register_system(Box::new(PhysicsSystem::new()));

        let player_entity = world.spawn();

        let mut action_map = ActionMap::new();
        for (name, keys) in [
            (ACTION_UP, vec![KeyCode::ArrowUp, KeyCode::KeyW]),
            (ACTION_DOWN, vec![KeyCode::ArrowDown, KeyCode::KeyS]),
            (ACTION_LEFT, vec![KeyCode::ArrowLeft, KeyCode::KeyA]),
            (ACTION_RIGHT, vec![KeyCode::ArrowRight, KeyCode::KeyD]),
            (ACTION_RESTART, vec![KeyCode::KeyR, KeyCode::Space]),
            (ACTION_CHOOSE[0], vec![KeyCode::Digit1]),
            (ACTION_CHOOSE[1], vec![KeyCode::Digit2]),
            (ACTION_CHOOSE[2], vec![KeyCode::Digit3]),
        ] {
            action_map.declare(ActionDecl {
                name: name.into(),
                kind: ActionKind::Button,
                bindings: keys.into_iter().map(Binding::Key).collect(),
            });
        }

        let shared = Arc::new(Mutex::new(GameLogic {
            player: player_entity,
            intent: Intent::default(),
            state: GameState::WaitingToStart,
            player_pos: DVec3::ZERO,
            player_hp: PLAYER_MAX_HP,
            player_facing: Facing::default(),
            player_moving: false,
            fire_timer: 0.0,
            spawn_timer: 0.0,
            elapsed: 0.0,
            kills: 0,
            enemies: Vec::new(),
            by_entity: HashMap::new(),
            bolts: Vec::new(),
            pickups: Vec::new(),
            pickup_by_entity: HashMap::new(),
            xp: 0,
            level: 1,
            offer: None,
            stats: Stats::default(),
            seed: setup.seed,
            runs: 0,
            spawn_counter: 0,
            max_enemies: setup.max_enemies,
            enemies_spawned: 0,
            bolts_fired: 0,
            pickups_dropped: 0,
            enemy_views: Vec::new(),
            bolt_views: Vec::new(),
            pickup_views: Vec::new(),
            scratch_entities: Vec::new(),
            cues: Vec::new(),
            ticks: 0,
        }));

        {
            let mut logic = lock(&shared);
            place_player(&mut logic, &mut world, DVec3::ZERO);
        }

        let mut session = Loopback::new(
            world,
            Box::new(HordeModule {
                shared: Arc::clone(&shared),
            }),
            setup.tick_hz,
            COMPATIBILITY,
        )
        .map_err(|e| GameError::Server(e.to_string()))?;

        let tick_period = session.tick_period();

        {
            let mut logic = lock(&shared);
            refresh_views(&mut logic, session.server_mut().world_mut());
        }

        let game = Self {
            player_entity,
            action_map,
            session,
            shared,
            tick_period,
            sim_time: Duration::ZERO,
            ticks_run: 0,
            pending_keys: Vec::new(),
            audio: crate::audio::Audio::new(setup.headless),
            best: crate::best::Best::load(setup.headless),
            state: GameState::WaitingToStart,
            player: DVec3::ZERO,
            player_hp: PLAYER_MAX_HP,
            elapsed: 0.0,
            kills: 0,
            level: 1,
            run: 1,
            prev_log_state: GameState::WaitingToStart,
            prev_elapsed: 0.0,
        };
        crcbl::log::info!(
            "sim: {} Hz, {:.3} ms per tick, up to {} enemies",
            setup.tick_hz,
            game.tick_dt_secs() * 1e3,
            setup.max_enemies,
        );
        Ok(game)
    }

    /// The fixed simulation step, in seconds.
    #[must_use]
    pub fn tick_dt_secs(&self) -> f64 {
        self.tick_period.as_secs_f64()
    }

    /// Queue a key event for replay at the start of the next tick.
    ///
    /// The shell pumps events once per **frame** while the action map's edge
    /// flags are per **tick**, and `ActionMap::begin_tick` resets those flags —
    /// so an event fed before `begin_tick` has its press edge erased by it.
    /// Queueing here and replaying after `begin_tick` is the order the action
    /// map asks for, and it is what makes a frame that runs no ticks lossless.
    pub fn key_event(&mut self, key: KeyCode, pressed: bool) {
        self.pending_keys.push((key, pressed));
    }

    /// Advances the simulation by exactly one fixed tick.
    ///
    /// Call it from the loop's fixed-timestep accumulator — once per tick, not
    /// once per frame. Nothing in here reads a wall clock.
    pub fn tick(&mut self) {
        let dt = self.tick_period.as_secs_f64();
        self.action_map.begin_tick(dt as f32);
        for (key, pressed) in std::mem::take(&mut self.pending_keys) {
            self.action_map.key_event(key, pressed);
        }

        // First match wins, so two digits in one frame take the earlier button
        // rather than the later one — the same rule an edge follows everywhere
        // else here.
        let choose = ACTION_CHOOSE
            .iter()
            .position(|name| self.action_map.just_pressed(name))
            .map_or(0, |index| index as u8 + 1);
        let intent = Intent {
            up: self.action_map.button_held(ACTION_UP),
            down: self.action_map.button_held(ACTION_DOWN),
            left: self.action_map.button_held(ACTION_LEFT),
            right: self.action_map.button_held(ACTION_RIGHT),
            restart: self.action_map.just_pressed(ACTION_RESTART),
            choose,
        };

        let ticks_before = {
            let mut logic = lock(&self.shared);
            // Held flags are assigned; the edge is `|=`, because an edge raised
            // on a frame that ran no ticks must survive until a tick consumes
            // it.
            logic.intent.up = intent.up;
            logic.intent.down = intent.down;
            logic.intent.left = intent.left;
            logic.intent.right = intent.right;
            logic.intent.restart |= intent.restart;
            if logic.intent.choose == 0 {
                logic.intent.choose = intent.choose;
            }
            logic.ticks
        };

        self.session.client_mut().set_input(vec![intent.to_wire()]);

        self.sim_time += self.tick_period;
        let server_ticks = self.session.server_mut().update(self.sim_time);
        debug_assert_eq!(
            server_ticks, 1,
            "one tick period in must be exactly one server tick out",
        );
        let _alpha = self.session.client_mut().update(self.sim_time);
        self.ticks_run += 1;

        // Drained under the same lock the tick filled it under, and *before* the
        // mirrors are read, so a frame that ran two ticks plays both of their
        // cues rather than only the last one's. The listener is the player,
        // which is the position read back below; taken here so a cue raised on
        // this tick is heard from where the player is on this tick.
        let (cues, listener) = {
            let mut logic = lock(&self.shared);
            let listener = logic.player_pos;
            (logic.cues.drain(..).collect::<Vec<_>>(), listener)
        };
        for (id, at) in cues {
            self.audio.play_at(id, listener, at);
        }

        let was = self.state;
        let ticks_after = {
            let logic = lock(&self.shared);
            self.state = logic.state;
            self.player = logic.player_pos;
            self.player_hp = logic.player_hp;
            self.elapsed = logic.elapsed;
            self.kills = logic.kills;
            self.level = logic.level;
            self.run = logic.runs.saturating_add(1);
            logic.ticks
        };
        debug_assert_eq!(
            ticks_after,
            ticks_before + u64::from(server_ticks),
            "game logic must run exactly once per physics tick",
        );

        // **Two edges bank a record, and the second is this game's own.** Death
        // is the obvious one, on the edge rather than every tick, because the
        // clock is frozen by then and an `update` per tick would write the file
        // sixty times a second for as long as the panel is up. The other is a
        // **restart**: `run_tick` puts `elapsed` back to zero, so a clock that
        // went backwards means a run ended without a death screen, and the run
        // it ended is worth exactly what it lasted. Without this, a player who
        // pressed R at four minutes would have that run count for nothing.
        let died = self.state == GameState::Dead && was != GameState::Dead;
        let restarted = self.elapsed < self.prev_elapsed;
        if died {
            self.best.update(self.elapsed);
        } else if restarted {
            self.best.update(self.prev_elapsed);
        }
        self.prev_elapsed = self.elapsed;

        let state_changed = self.state != self.prev_log_state;
        self.prev_log_state = self.state;
        // **Every sixty ticks, which is a second of simulated time, and the same
        // cadence breakout, flappy and asteroids use.** `web/tools/browser-e2e.mjs`
        // watches for this heartbeat to tell a paused demo from a running one.
        //
        // The state is in the line because that is what the gate reads: "the
        // input reached the simulation" is `WaitingToStart` before the key and
        // `Playing` after it, the same claim the other three samples make. `run`
        // is beside it for a bug report, and because it is what tells a restart
        // from a start — only a real restart edge advances it.
        if state_changed || self.ticks_run.is_multiple_of(60) {
            crcbl::log::info!(
                "[HUD] {:?}  run: {}  time: {:.1}  best: {}  kills: {}  hp: {:.0}  lvl: {}  \
                 enemies: {}  bolts: {}  gems: {}",
                self.state,
                self.run,
                self.elapsed,
                self.best.get(),
                self.kills,
                self.player_hp,
                self.level,
                self.enemy_count(),
                self.bolt_count(),
                self.pickup_count(),
            );
        }
    }

    /// Everything the renderer draws, in world space.
    ///
    /// `out` is reused across frames so a steady-state frame does not allocate.
    pub fn render_state(&self, out: &mut RenderState) {
        let logic = lock(&self.shared);
        out.player = logic.player_pos;
        out.player_facing = logic.player_facing;
        out.player_walking = logic.player_moving;
        out.player_hp = logic.player_hp;
        out.player_max_hp = logic.stats.max_hp;
        out.enemies.clear();
        out.enemies.extend_from_slice(&logic.enemy_views);
        out.bolts.clear();
        out.bolts.extend_from_slice(&logic.bolt_views);
        out.pickups.clear();
        out.pickups.extend_from_slice(&logic.pickup_views);
        out.elapsed = logic.elapsed;
        out.kills = logic.kills;
        out.xp = logic.xp;
        out.xp_needed = xp_for_next_level(logic.level);
        out.level = logic.level;
        out.offer = logic.offer;
        out.state = Some(logic.state);
        drop(logic);
        // Outside the lock: the record is the facade's, not the simulation's —
        // a replay of the same script must not depend on how long some earlier
        // session happened to survive.
        out.best = self.best.get();
    }

    /// The run in play, for a caller that wants to name it — a bug report, a
    /// replay header, or a test.
    #[must_use]
    pub fn run_seed(&self) -> u64 {
        lock(&self.shared).run()
    }

    /// The enemies on the field right now.
    #[must_use]
    pub fn enemies(&self) -> Vec<EnemyView> {
        lock(&self.shared).enemy_views.clone()
    }

    /// The bolts in the air right now.
    #[must_use]
    pub fn bolts(&self) -> Vec<BoltView> {
        lock(&self.shared).bolt_views.clone()
    }

    /// How many enemies are on the field.
    #[must_use]
    pub fn enemy_count(&self) -> usize {
        lock(&self.shared).enemies.len()
    }

    /// How many bolts are in the air.
    #[must_use]
    pub fn bolt_count(&self) -> usize {
        lock(&self.shared).bolts.len()
    }

    /// How many gems are on the ground.
    #[must_use]
    pub fn pickup_count(&self) -> usize {
        lock(&self.shared).pickups.len()
    }

    /// Experience banked towards the next level.
    #[must_use]
    pub fn xp(&self) -> u64 {
        lock(&self.shared).xp
    }

    /// The three upgrades on offer, or `None` when the screen is not up.
    #[must_use]
    pub fn offer(&self) -> Option<[Upgrade; UPGRADE_CHOICES]> {
        lock(&self.shared).offer
    }

    /// The numbers this run has raised. See [`Stats`].
    #[must_use]
    pub fn stats(&self) -> Stats {
        lock(&self.shared).stats
    }

    /// How many enemies this game has ever put on the field, across every run.
    ///
    /// The denominator of the leak test: "nothing leaked" is a claim about a run
    /// that churned, and this is what says it churned.
    #[must_use]
    pub fn enemies_spawned(&self) -> u64 {
        lock(&self.shared).enemies_spawned
    }

    /// How many bolts this game has ever fired.
    #[must_use]
    pub fn bolts_fired(&self) -> u64 {
        lock(&self.shared).bolts_fired
    }

    /// How many gems a full field refused to drop. See [`MAX_PICKUPS`].
    #[must_use]
    pub fn pickups_dropped(&self) -> u64 {
        lock(&self.shared).pickups_dropped
    }

    /// The ceiling on live enemies this game was built with.
    #[must_use]
    pub fn max_enemies(&self) -> usize {
        lock(&self.shared).max_enemies
    }

    /// How many entities the simulation is holding.
    #[must_use]
    pub fn entity_count(&mut self) -> usize {
        self.session.server_mut().world_mut().entity_count()
    }

    /// How many entities are queued for destruction and not yet swept.
    ///
    /// **Not an implementation detail — a number the counts above cannot be read
    /// without.** `crcbl::ecs::World::sweep` runs at the end of `World::tick`,
    /// and `crcbl-server` calls `GameModule::tick` *after* that, so everything
    /// this game destroys — and it destroys a great deal — waits one tick before
    /// the pool lets go of it. Recorded in `docs/backlog.md` as a finding
    /// against the module hook's placement.
    #[must_use]
    pub fn pending_despawns(&mut self) -> usize {
        self.session.server_mut().world_mut().dead_queue_len()
    }

    /// How many colliders the physics world is holding.
    ///
    /// A second, independent count, because the two leak separately: a collider
    /// left behind when its entity goes is an invisible wall, and nothing about
    /// the entity count would notice.
    #[must_use]
    pub fn collider_count(&mut self) -> usize {
        with_physics(self.session.server_mut().world_mut(), |phys| {
            phys.collider_count()
        })
        .unwrap_or(0)
    }

    /// Fills the arena with `count` enemies before the first tick.
    ///
    /// **The scale sub-slice's fixture, and the only reason it is not
    /// `#[cfg(test)]`.** The spawner ramps from one enemy every half second to
    /// one every sixteenth (see [`spawn_interval`]), so a field of ten thousand
    /// is somewhere over ten minutes of play that nothing survives — there is no
    /// way to *measure* the plan's target by playing to it. `--prefill` puts the
    /// field there on frame zero instead, and the numbers in
    /// `docs/plan/sample/03-horde.md` are all taken through it.
    ///
    /// The layout is a grid over the **whole arena**, sized so `count` fits:
    /// staging them at the 1.25 units separation settles at would need 125 × 125
    /// units for ten thousand and the arena is 96 × 72, so a fixture written
    /// that way would pile most of the field onto the walls under
    /// [`clamp_to_arena`] and measure a crowd nothing produces. Spreading them
    /// evenly is what ten thousand in this arena actually looks like: about 0.83
    /// units apart, which is denser than separation wants and is the point.
    ///
    /// The kinds follow the same [`spawn_kind`] table the spawner draws from, so
    /// the mix is the game's rather than a field of grunts, and the counter is
    /// the run's own — a prefilled run and a played one never draw the same
    /// number twice.
    ///
    /// Refuses to go past `max_enemies`, and reports how many it actually
    /// staged.
    pub fn stage_field(&mut self, count: usize) -> usize {
        let mut logic = lock(&self.shared);
        let world = self.session.server_mut().world_mut();
        let room = logic.max_enemies.saturating_sub(logic.enemies.len());
        let wanted = count.min(room);
        if wanted == 0 {
            return 0;
        }

        // A grid with the arena's own aspect, so the spacing is the same on both
        // axes and the crowd is isotropic. `+ 1` on the divisor keeps every
        // enemy strictly inside the walls rather than on them.
        let aspect = ARENA_HALF_WIDTH / ARENA_HALF_HEIGHT;
        #[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)]
        let cols = ((wanted as f64 * aspect).sqrt().ceil() as usize).max(1);
        let rows = wanted.div_ceil(cols).max(1);
        let step_x = (2.0 * ARENA_HALF_WIDTH) / (cols + 1) as f64;
        let step_y = (2.0 * ARENA_HALF_HEIGHT) / (rows + 1) as f64;

        for index in 0..wanted {
            let (col, row) = (index % cols, index / cols);
            let position = DVec3::new(
                -ARENA_HALF_WIDTH + step_x * (col + 1) as f64,
                -ARENA_HALF_HEIGHT + step_y * (row + 1) as f64,
                0.0,
            );
            let counter = logic.spawn_counter;
            logic.spawn_counter += 1;
            let seed = logic.run();
            spawn_enemy(
                &mut logic,
                world,
                spawn_kind(seed, counter),
                position,
                spawn_jitter(seed, counter),
            );
        }
        // The views are what the renderer reads and they were built when the
        // field was empty; without this the first frame draws nothing and the
        // measurement's first frame is the wrong one.
        refresh_views(&mut logic, world);
        crcbl::log::info!(
            "prefill: staged {wanted} enemies on a {cols}x{rows} grid, \
             {:.2} x {:.2} units apart",
            step_x,
            step_y,
        );
        wanted
    }

    /// Puts the player somewhere specific, for a test that needs a known board.
    #[cfg(test)]
    pub fn stage_player(&mut self, position: DVec3) {
        let mut logic = lock(&self.shared);
        let world = self.session.server_mut().world_mut();
        place_player(&mut logic, world, position);
    }

    /// Clears the field, for the same reason.
    #[cfg(test)]
    pub fn clear_enemies(&mut self) {
        let mut logic = lock(&self.shared);
        let world = self.session.server_mut().world_mut();
        for enemy in std::mem::take(&mut logic.enemies) {
            with_physics(world, |phys| phys.remove_entity(enemy.entity));
            world.despawn(enemy.entity);
        }
        logic.by_entity.clear();
    }

    /// Puts one enemy at a named place, and returns its entity.
    #[cfg(test)]
    pub fn stage_enemy(&mut self, kind: EnemyKind, position: DVec3) -> Entity {
        let mut logic = lock(&self.shared);
        let world = self.session.server_mut().world_mut();
        let jitter = spawn_jitter(logic.run(), logic.spawn_counter);
        logic.spawn_counter += 1;
        spawn_enemy(&mut logic, world, kind, position, jitter)
    }

    /// What is left of one enemy's hit points.
    #[cfg(test)]
    #[must_use]
    pub fn enemy_hp(&self, entity: Entity) -> Option<f64> {
        let logic = lock(&self.shared);
        logic
            .by_entity
            .get(&entity)
            .and_then(|index| logic.enemies.get(*index))
            .map(|enemy| enemy.hp)
    }

    /// Stops the spawner, so a test can stage a board and keep it.
    #[cfg(test)]
    pub fn freeze_spawns(&mut self) {
        lock(&self.shared).max_enemies = 0;
    }

    /// Takes the player's hit points down to `hp`, so a test does not have to
    /// stand in a crowd for eight seconds to reach a death screen.
    #[cfg(test)]
    pub fn set_player_hp(&mut self, hp: f64) {
        lock(&self.shared).player_hp = hp;
    }

    /// Banks experience directly, so a test reaches a level-up screen without
    /// killing eight grunts first.
    #[cfg(test)]
    pub fn bank_xp(&mut self, xp: u64) {
        lock(&self.shared).xp += xp;
    }

    /// Drops a gem at a named place, and returns its entity.
    #[cfg(test)]
    pub fn stage_pickup(&mut self, position: DVec3, xp: u64) -> Entity {
        let mut logic = lock(&self.shared);
        let world = self.session.server_mut().world_mut();
        drop_pickup(&mut logic, world, position, xp);
        logic.pickups.last().expect("just dropped one").entity
    }

    /// Where every gem on the ground is.
    #[cfg(test)]
    #[must_use]
    pub fn pickup_positions(&self) -> Vec<DVec3> {
        lock(&self.shared)
            .pickups
            .iter()
            .map(|gem| gem.position)
            .collect()
    }

    /// Where every bolt in the air is, straight off the physics world.
    #[cfg(test)]
    #[must_use]
    pub fn bolt_positions(&mut self) -> Vec<DVec3> {
        let bolts: Vec<Entity> = lock(&self.shared).bolts.iter().map(|b| b.entity).collect();
        with_physics(self.session.server_mut().world_mut(), |phys| {
            bolts
                .into_iter()
                .filter_map(|entity| phys.transform(entity).map(|t| t.position))
                .collect()
        })
        .unwrap_or_default()
    }

    /// The neighbourhood `steer_enemies` would see for `entity`, through the
    /// same [`separation_query_radius`] and the same broadphase.
    ///
    /// The test seam for the one assumption this file makes about a *different*
    /// crate — that a shape-aware overlap of radius `R` returns every collider
    /// within `R + r_b`. Sorted, because the broadphase's own order is a
    /// traversal order and a test that depended on it would be testing the tree.
    #[cfg(test)]
    #[must_use]
    pub fn separation_neighbours(&mut self, entity: Entity) -> Vec<Entity> {
        let found = {
            let logic = lock(&self.shared);
            logic
                .by_entity
                .get(&entity)
                .and_then(|index| logic.enemies.get(*index))
                .map(|enemy| (enemy.position, enemy.kind))
        };
        let Some((position, kind)) = found else {
            return Vec::new();
        };
        let mut found = with_physics(self.session.server_mut().world_mut(), |phys| {
            phys.overlap_sphere(position, separation_query_radius(kind))
                .into_iter()
                .map(|(entity, _hit)| entity)
                .collect::<Vec<_>>()
        })
        .unwrap_or_default();
        found.sort_unstable_by_key(|entity| entity.to_bits());
        found
    }

    /// Where everything on the field is, straight off the simulation's own
    /// list.
    ///
    /// Not [`Game::enemies`], which reads the *views* — and the views are
    /// refilled by `refresh_views` at the end of a tick, so a board that was
    /// staged and not yet ticked has none. A test that measured a staged board
    /// through the views would be measuring an empty vector.
    #[cfg(test)]
    #[must_use]
    pub fn enemy_positions(&self) -> Vec<DVec3> {
        lock(&self.shared)
            .enemies
            .iter()
            .map(|enemy| enemy.position)
            .collect()
    }

    /// Where one enemy is, for a test that staged it and wants to watch it.
    #[cfg(test)]
    #[must_use]
    pub fn enemy_position(&self, entity: Entity) -> Option<DVec3> {
        let logic = lock(&self.shared);
        let index = *logic.by_entity.get(&entity)?;
        logic.enemies.get(index).map(|enemy| enemy.position)
    }
}

// ---- tests ------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use crcbl::core::FrameClock;

    use super::*;
    use crcbl::core::time::{ManualTime, TimeSource as _};

    /// One entry of a script: `(tick index, key, pressed)`.
    type Script = [(u64, KeyCode, bool)];

    /// How many ticks one lap of the autopilot's kite takes.
    ///
    /// Ten seconds at [`DEFAULT_TICK_HZ`], which at [`PLAYER_SPEED`] is a circle
    /// of about eleven units' radius — comfortably inside the arena, and long
    /// enough that the grunts left behind at the middle of it catch up.
    const KITE_PERIOD: u64 = 600;

    /// Drives a `Game` the way the app loop will — a frame clock at `frame_hz`,
    /// a fixed-timestep accumulator at `tick_hz`, and events pumped once per
    /// frame.
    ///
    /// The frame rate and the tick rate are independent knobs on purpose. Every
    /// property asserted below is a property of *simulated* time, and a loop
    /// that leaked the frame rate into the simulation is what makes them
    /// disagree.
    struct Harness {
        game: Game,
        clock: FrameClock,
        time: ManualTime,
        frame_step: Duration,
        ticks: u64,
        /// What the autopilot currently has held down, in
        /// `[up, down, left, right]` order. Movement is a *held* action, so a
        /// controller that pressed and released every tick would be testing the
        /// edge detector rather than the player.
        held: [bool; 4],
        /// How many times `Harness::play_ticks` has restarted a finished run.
        ///
        /// A restart is the largest single piece of churn this game has — it
        /// wipes the whole field — so a soak that never reached one has not
        /// tested the path.
        restarts: u32,
        /// How many level-up screens `Harness::play_ticks` has answered, for the
        /// same reason: a soak that never opened one never froze the field.
        levels: u32,
    }

    /// Indices into [`Harness::held`], and the key each one drives.
    const HELD_KEYS: [KeyCode; 4] = [KeyCode::KeyW, KeyCode::KeyS, KeyCode::KeyA, KeyCode::KeyD];

    impl Harness {
        fn new(frame_hz: u32, tick_hz: u32) -> Self {
            Self::with_setup(
                frame_hz,
                &Setup {
                    headless: true,
                    tick_hz,
                    ..Setup::default()
                },
            )
        }

        fn with_setup(frame_hz: u32, setup: &Setup) -> Self {
            let mut harness = Self::at_the_title_screen(frame_hz, setup);
            // **Every harness below starts on tick 0 rather than on the title
            // screen**, so the tick indices its scripts are keyed on still mean
            // what they meant before the start screen existed. The edge is
            // *queued*, not poked: it is replayed into the action map by the
            // first `Game::tick`, which consumes it and plays the whole of that
            // tick — so tick 0 is a playing tick, exactly as it was.
            //
            // That also makes this the suite's widest check on the start path.
            // A start edge the simulation stopped honouring leaves every test
            // below looking at a frozen arena.
            harness.game.key_event(KeyCode::Space, true);
            harness.game.key_event(KeyCode::Space, false);
            harness
        }

        /// The same, left on the title screen — for the handful of tests that
        /// are *about* the title screen.
        fn at_the_title_screen(frame_hz: u32, setup: &Setup) -> Self {
            Self {
                game: Game::with_setup(setup).expect("a headless game always starts"),
                clock: FrameClock::new(setup.tick_hz),
                time: ManualTime::new(),
                frame_step: FrameClock::new(frame_hz).tick_dt(),
                ticks: 0,
                held: [false; 4],
                restarts: 0,
                levels: 0,
            }
        }

        /// A game left on the title screen, with its spawner live — the state a
        /// player's window opens in.
        fn waiting(frame_hz: u32, tick_hz: u32) -> Self {
            Self::at_the_title_screen(
                frame_hz,
                &Setup {
                    headless: true,
                    tick_hz,
                    ..Setup::default()
                },
            )
        }

        /// A staged board: no spawner, no enemies, the player where it is asked
        /// for. Every mechanism test starts from this, so a change to the spawn
        /// ramp cannot silently move one of them.
        fn staged(frame_hz: u32, tick_hz: u32, player: DVec3) -> Self {
            let mut harness = Self::new(frame_hz, tick_hz);
            harness.game.freeze_spawns();
            harness.game.clear_enemies();
            harness.game.stage_player(player);
            harness
        }

        /// One frame: advance the clock, drain whole ticks, exactly as the app
        /// loop does — stopping at `limit` so a caller counting ticks is not at
        /// the mercy of how many a single frame happened to release.
        ///
        /// The script is keyed on the **tick** index and fed immediately before
        /// that tick runs, so the input a given tick sees is the same at every
        /// frame rate.
        fn frame(&mut self, script: &Script, limit: u64) {
            self.time.advance(self.frame_step);
            self.clock.update(self.time.elapsed());
            while self.ticks < limit && self.clock.consume_tick() {
                for &(at, key, pressed) in script {
                    if at == self.ticks {
                        self.game.key_event(key, pressed);
                    }
                }
                self.game.tick();
                self.ticks += 1;
            }
        }

        /// Runs frames until the simulation has run exactly `ticks` of them.
        fn run_ticks(&mut self, ticks: u64, script: &Script) {
            while self.ticks < ticks {
                self.frame(script, ticks);
            }
        }

        /// Presses or releases a held key only when its state has to change.
        fn hold(&mut self, slot: usize, want: bool) {
            if self.held[slot] != want {
                self.game.key_event(HELD_KEYS[slot], want);
                self.held[slot] = want;
            }
        }

        /// Runs to `ticks` under the autopilot — a player who kites.
        fn play_ticks(&mut self, ticks: u64) {
            while self.ticks < ticks {
                self.time.advance(self.frame_step);
                self.clock.update(self.time.elapsed());
                while self.ticks < ticks && self.clock.consume_tick() {
                    // Start the next run — **two edges, one tick apart**, because
                    // a restart lands on the title screen and the title screen
                    // is left by the same key. Only the first is counted: the
                    // second is a start, not a restart. The edge is pressed and
                    // released inside the one tick, and it is the harness that
                    // does it rather than `autopilot`, because the plan is a set
                    // of held keys and this is not one.
                    if self.game.state == GameState::Dead {
                        self.restarts += 1;
                    }
                    if matches!(self.game.state, GameState::Dead | GameState::WaitingToStart) {
                        self.game.key_event(KeyCode::KeyR, true);
                        self.game.key_event(KeyCode::KeyR, false);
                    }
                    // **A level-up screen has to be answered or the soak
                    // stops.** The field freezes while it is up and the spawner
                    // does not run, so an autopilot that walked past it would
                    // measure a frozen field for the rest of the run — which is
                    // exactly the shape of a soak that silently tests nothing.
                    if self.game.state == GameState::LevelUp {
                        self.levels += 1;
                        self.game.key_event(KeyCode::Digit1, true);
                        self.game.key_event(KeyCode::Digit1, false);
                    }
                    let plan = autopilot(&self.game, self.ticks);
                    for (slot, want) in plan.iter().copied().enumerate() {
                        self.hold(slot, want);
                    }
                    self.game.tick();
                    self.ticks += 1;
                }
            }
        }

        /// A restart, all the way back into play.
        ///
        /// **Two edges and two ticks**, because `restart` lands on the title
        /// screen: the first clears the run, the second leaves the screen. A
        /// test that wants to *see* the title screen taps once instead.
        fn restart_run(&mut self) {
            self.tap(KeyCode::KeyR);
            assert_eq!(
                self.game.state,
                GameState::WaitingToStart,
                "a restart did not land on the title screen",
            );
            self.tap(KeyCode::KeyR);
        }

        /// Presses and releases a key on the next tick, and runs it.
        fn tap(&mut self, key: KeyCode) {
            self.game.key_event(key, true);
            self.game.key_event(key, false);
            self.game.tick();
            self.ticks += 1;
        }

        /// The invariant that has to hold on **every** tick of every test that
        /// churns: the ECS holds exactly the player, the enemies and the bolts,
        /// and the broadphase holds exactly the enemies.
        ///
        /// This is the leak detector. An entity or a collider that outlived what
        /// it belonged to shows up here on the tick it happened.
        ///
        /// `pending` is in the entity sum because destruction is *deferred*: the
        /// ECS sweeps at the end of `World::tick` and the game module runs after
        /// that, so everything destroyed this tick is still in the pool until
        /// the next one. See [`Game::pending_despawns`].
        fn assert_nothing_leaked(&mut self) {
            let enemies = self.game.enemy_count();
            let bolts = self.game.bolt_count();
            let gems = self.game.pickup_count();
            let pending = self.game.pending_despawns();
            assert_eq!(
                self.game.entity_count(),
                1 + enemies + bolts + gems + pending,
                "tick {}: {enemies} enemies, {bolts} bolts, {gems} gems and {pending} \
                 awaiting the sweep do not account for the world",
                self.ticks,
            );
            // An equality, not a bound: every collider in the world is an enemy
            // or a gem. The player is not in the broadphase and neither is a
            // bolt — see `contact_damage` and `sweep_bolts`.
            assert_eq!(
                self.game.collider_count(),
                enemies + gems,
                "tick {}: {enemies} enemies and {gems} gems do not account for the \
                 broadphase",
                self.ticks,
            );
        }
    }

    /// What the autopilot holds this tick, in [`HELD_KEYS`] order.
    type Plan = [bool; 4];

    /// A player who kites: walks a circle, which is what a survivors player
    /// actually does and what keeps a run going long enough to churn.
    ///
    /// Deliberately a **function of the tick index alone**, not of the enemy
    /// list. Two reasons, and the second is the load-bearing one:
    ///
    /// * reading `Game::enemies()` clones the whole view vector, which at the
    ///   counts this sample reaches would make the soak a test of `memcpy`;
    /// * the input a given tick sees is then the same at 20 fps as at 240, so
    ///   the frame-rate test compares two runs of the same script rather than
    ///   two runs of two scripts.
    ///
    /// A finished run is restarted by `Harness::play_ticks`, not here.
    fn autopilot(game: &Game, tick: u64) -> Plan {
        if game.state == GameState::Dead {
            return [false; 4];
        }
        let phase = (tick % KITE_PERIOD) as f64 / KITE_PERIOD as f64 * std::f64::consts::TAU;
        let (x, y) = (phase.cos(), phase.sin());
        // A dead zone, so a lap is eight distinct directions rather than a
        // continuum — which keeps a held key held for a stretch of ticks instead
        // of chattering on and off at the axis crossings.
        [y > 0.4, y < -0.4, x < -0.4, x > 0.4]
    }

    /// The smallest gap between any two of `positions`, and the largest.
    fn extremes(positions: &[DVec3]) -> (f64, f64) {
        let mut min = f64::INFINITY;
        let mut max: f64 = 0.0;
        for (i, a) in positions.iter().enumerate() {
            for b in &positions[i + 1..] {
                let d = (*a - *b).length();
                min = min.min(d);
                max = max.max(d);
            }
        }
        (min, max)
    }

    // ---- the arena -----------------------------------------------------------

    /// `clamp_axis` is **bit-exact** inside the arena, and saturating outside
    /// it.
    ///
    /// Exactness is not fussiness. `clamp_bodies` decides whether to write a
    /// transform back — and so whether to touch the broadphase — by comparing
    /// this against the position it was given, so a round trip that returned a
    /// value one ulp away would re-place every body on every tick, forever.
    #[test]
    fn the_clamp_is_exact_inside_the_arena_and_saturates_outside_it() {
        for v in [
            -47.999_9,
            -0.5,
            0.0,
            7.25,
            47.999_9,
            0.1,
            -0.3,
            1.0 / 3.0,
            12.345_678_901_234,
        ] {
            assert_eq!(clamp_axis(v, ARENA_HALF_WIDTH), v, "{v} was moved");
        }
        for point in [
            DVec3::new(0.1, -0.3, 0.0),
            DVec3::new(1.0 / 3.0, -11.345_678_901_234, 0.0),
        ] {
            assert_eq!(
                clamp_to_arena(point, PLAYER_RADIUS),
                point,
                "{point:?} moved"
            );
        }
        assert_eq!(clamp_axis(100.0, 48.0), 48.0);
        assert_eq!(clamp_axis(-100.0, 48.0), -48.0);
        // A body wider than the space it is in has exactly one legal position.
        assert_eq!(clamp_axis(3.0, -1.0), 0.0);
        // The radius is taken off both sides, so a body is *inside* the wall.
        let corner = clamp_to_arena(DVec3::new(1e6, -1e6, 0.0), EnemyKind::Brute.radius());
        assert_eq!(corner.x, ARENA_HALF_WIDTH - EnemyKind::Brute.radius());
        assert_eq!(corner.y, -(ARENA_HALF_HEIGHT - EnemyKind::Brute.radius()));
    }

    /// **The arena holds the player in**, however long they walk at a wall.
    ///
    /// Asserted at the wall rather than merely "inside the arena": a clamp that
    /// stopped a unit short would pass the weaker version and would still be
    /// wrong.
    #[test]
    fn a_player_walking_at_a_wall_stops_at_it() {
        let mut harness = Harness::staged(60, 60, DVec3::ZERO);
        // Long enough to cross the whole arena twice over at PLAYER_SPEED.
        harness.run_ticks(1_200, &[(0, KeyCode::KeyD, true), (0, KeyCode::KeyW, true)]);
        let player = harness.game.player;
        assert!(
            (player.x - (ARENA_HALF_WIDTH - PLAYER_RADIUS)).abs() < 1e-9,
            "the player did not stop at the right wall: {player:?}",
        );
        assert!(
            (player.y - (ARENA_HALF_HEIGHT - PLAYER_RADIUS)).abs() < 1e-9,
            "the player did not stop at the top wall: {player:?}",
        );
        harness.assert_nothing_leaked();
    }

    /// **Enemies are held in too**, which is where separation and the walls
    /// meet: a crowd jammed into a corner is pushed outward by exactly the term
    /// that has no idea the arena has edges.
    ///
    /// The player is parked in the opposite corner, far outside
    /// [`WEAPON_RANGE`], so nothing is shot and the crowd is intact at the end.
    /// Both halves are asserted: nothing escaped, **and** the clamp actually
    /// ran — a crowd that never reached a wall would satisfy the first on its
    /// own.
    #[test]
    fn a_crowd_squeezed_into_a_corner_stays_inside_the_arena() {
        let far = DVec3::new(
            -(ARENA_HALF_WIDTH - PLAYER_RADIUS),
            -(ARENA_HALF_HEIGHT - PLAYER_RADIUS),
            0.0,
        );
        let mut harness = Harness::staged(60, 60, far);
        let corner = DVec3::new(ARENA_HALF_WIDTH, ARENA_HALF_HEIGHT, 0.0);
        for i in 0..30 {
            // All staged on top of each other in the corner, so separation has
            // nowhere to send them but into the two walls.
            // Planar, not `DVec3::splat`: the arena is a plane and everything
            // the game itself produces sits at `z = 0`, so a fixture with a
            // depth component would be separating in a dimension the clamp
            // deliberately leaves alone. See `docs/backlog.md`.
            let t = i as f64 * 0.01;
            harness
                .game
                .stage_enemy(EnemyKind::Grunt, corner - DVec3::new(t, t, 0.0));
        }

        let limit = DVec3::new(
            ARENA_HALF_WIDTH - EnemyKind::Grunt.radius(),
            ARENA_HALF_HEIGHT - EnemyKind::Grunt.radius(),
            0.0,
        );
        // Checked on **every** tick rather than at the end: a body that left the
        // arena and was dragged back by its own seek would pass the end-state
        // version, and the failure names the tick it escaped on.
        let mut at_a_wall = 0;
        while harness.ticks < 180 {
            harness.run_ticks(harness.ticks + 1, &[]);
            for position in harness.game.enemy_positions() {
                assert!(
                    position.x.abs() <= limit.x + 1e-9 && position.y.abs() <= limit.y + 1e-9,
                    "tick {}: an enemy is outside the arena at {position:?}, \
                     against a limit of {limit:?}",
                    harness.ticks,
                );
                if (position.x - limit.x).abs() < 1e-12 || (position.y - limit.y).abs() < 1e-12 {
                    at_a_wall += 1;
                }
            }
        }
        assert_eq!(harness.game.enemy_count(), 30, "something killed the crowd");
        assert!(
            at_a_wall > 0,
            "nothing ever reached a wall, so the clamp was never exercised",
        );
        harness.assert_nothing_leaked();
    }

    /// Enemies enter from beyond the view, so the horde walks on screen rather
    /// than appearing in it.
    ///
    /// The relation is asserted, not the number, so a later tuning pass that
    /// changes either constant is told rather than left to find out.
    #[test]
    fn enemies_enter_from_beyond_the_view() {
        // The corner of the widest window the demo pages ever open: a 4:3
        // canvas is the reference, and a wider one shows more of the arena.
        let view_corner = (VIEW_HALF_HEIGHT * 4.0 / 3.0).hypot(VIEW_HALF_HEIGHT);
        assert!(
            SPAWN_RING > view_corner,
            "spawns at {SPAWN_RING} land inside a view whose corner is {view_corner}",
        );
        // …and not so far that the horde never arrives: half a lap of the
        // autopilot's kite.
        const { assert!(SPAWN_RING < ARENA_HALF_HEIGHT) };
        for counter in 0..500 {
            let offset = spawn_offset(DEFAULT_SEED, counter);
            assert!(
                (offset.length() - SPAWN_RING).abs() < 1e-9,
                "spawn {counter} was dealt at {offset:?}, off the ring",
            );
        }
    }

    // ---- the player and the arena's rules -------------------------------------

    /// The player moves at the stated speed, and a diagonal is not faster than a
    /// straight line.
    ///
    /// The classic bug this closes is one line of arithmetic — an unnormalised
    /// input vector — and it is invisible in play until somebody notices that
    /// running north-east outruns the runners and running north does not.
    #[test]
    fn the_player_moves_at_the_stated_speed_and_a_diagonal_is_no_faster() {
        let straight = {
            let mut harness = Harness::staged(60, 60, DVec3::ZERO);
            harness.run_ticks(60, &[(0, KeyCode::KeyD, true)]);
            harness.game.player
        };
        let diagonal = {
            let mut harness = Harness::staged(60, 60, DVec3::ZERO);
            harness.run_ticks(60, &[(0, KeyCode::KeyD, true), (0, KeyCode::KeyW, true)]);
            harness.game.player
        };
        assert!(
            (straight.length() - PLAYER_SPEED).abs() < 0.15,
            "a second of walking covered {}, not {PLAYER_SPEED}",
            straight.length(),
        );
        assert!(
            (diagonal.length() - straight.length()).abs() < 1e-9,
            "a diagonal covered {} against a straight line's {}",
            diagonal.length(),
            straight.length(),
        );
        assert!(
            (diagonal.x - diagonal.y).abs() < 1e-9,
            "a diagonal is not diagonal: {diagonal:?}",
        );
    }

    /// Opposite keys cancel rather than picking a winner.
    #[test]
    fn opposite_keys_stand_still() {
        let mut harness = Harness::staged(60, 60, DVec3::ZERO);
        harness.run_ticks(60, &[(0, KeyCode::KeyD, true), (0, KeyCode::KeyA, true)]);
        assert_eq!(harness.game.player, DVec3::ZERO);
    }

    // ---- seeking and separation, which is what this sample is for -------------

    /// **Enemies seek the player.**
    ///
    /// The distance is asserted against the *speed they are supposed to travel
    /// at*, not against "it went down": an enemy that drifted a hundredth of a
    /// unit a second would satisfy the weaker version.
    #[test]
    fn every_kind_of_enemy_seeks_the_player_at_its_own_speed() {
        for kind in EnemyKind::ALL {
            let mut harness = Harness::staged(60, 60, DVec3::ZERO);
            let start = DVec3::new(0.0, -30.0, 0.0);
            let enemy = harness.game.stage_enemy(kind, start);
            harness.run_ticks(120, &[]);
            let position = harness
                .game
                .enemy_position(enemy)
                .unwrap_or_else(|| panic!("{kind:?} died: nothing in this test can kill it"));

            let travelled = (position - start).length();
            let expected = kind.speed() * 2.0;
            assert!(
                (travelled - expected).abs() < 0.1,
                "{kind:?} covered {travelled} in two seconds, not {expected}",
            );
            assert!(
                position.length() < start.length(),
                "{kind:?} moved away from the player: {position:?}",
            );
            // …and straight at the player, not merely nearer.
            assert!(
                position.x.abs() < 1e-9,
                "{kind:?} wandered off the line to the player: {position:?}",
            );
        }
    }

    /// **A crowd does not end up co-located.**
    ///
    /// The property separation exists for, and the one a broken implementation
    /// satisfies vacuously if everything simply sits on the player — so the
    /// player is parked 36 units away and out of weapon range, and what is
    /// measured is the crowd's *internal* spacing while it travels.
    ///
    /// Both halves are asserted:
    ///
    /// * the knot starts closer than the neighbourhood, so the mechanism is
    ///   genuinely engaged rather than untouched;
    /// * every pair ends at least `r_a + r_b` apart, which is the physical claim
    ///   — no two bodies are inside one another — and the crowd's spread grows
    ///   from a point to something a screen can see.
    ///
    /// With `SEPARATION_STRENGTH` at zero the whole crowd rides on top of itself
    /// and the minimum gap stays at its starting value.
    #[test]
    fn a_crowd_seeking_one_player_comes_apart_rather_than_stacking() {
        let mut harness = Harness::staged(60, 60, DVec3::new(0.0, 30.0, 0.0));
        const CROWD: usize = 20;
        for i in 0..CROWD {
            // A knot 0.19 units across, which is inside two grunts' radii.
            let t = i as f64 * 0.01;
            harness
                .game
                .stage_enemy(EnemyKind::Grunt, DVec3::new(t, -6.0 - t, 0.0));
        }

        let (min_before, max_before) = extremes(&harness.game.enemy_positions());
        let touching = 2.0 * EnemyKind::Grunt.radius();
        assert!(
            min_before < touching,
            "the crowd did not start interpenetrating, so this proves nothing: \
             {min_before} against {touching}",
        );

        harness.run_ticks(180, &[]);
        assert_eq!(
            harness.game.enemy_count(),
            CROWD,
            "something killed part of the crowd, so the spacing below is of \
             fewer bodies than were staged",
        );
        let positions = harness.game.enemy_positions();
        let (min_after, max_after) = extremes(&positions);

        assert!(
            min_after >= touching,
            "two grunts are still inside each other: {min_after} against {touching}",
        );
        assert!(
            max_after > max_before + 3.0,
            "the crowd never spread: {max_before} to {max_after}",
        );
        // And it is still a crowd going somewhere, not an explosion: the
        // centroid has to have travelled towards the player.
        let centroid = positions.iter().copied().sum::<DVec3>() / CROWD as f64;
        assert!(
            centroid.y > -6.0 + 5.0,
            "the crowd stopped seeking while it separated: {centroid:?}",
        );
        harness.assert_nothing_leaked();
    }

    /// **Two enemies at exactly the same point come apart**, which is the case
    /// with no direction between them and the one [`spawn_jitter`] exists for.
    ///
    /// A horde converging on one player produces this constantly. Without the
    /// tie-break a coincident pair is a fixed point of `separation_push` and
    /// the two ride on top of each other forever — which the test above cannot
    /// see, because twenty bodies staged a hundredth of a unit apart are never
    /// exactly coincident.
    #[test]
    fn two_enemies_at_exactly_one_point_still_come_apart() {
        let mut harness = Harness::staged(60, 60, DVec3::new(0.0, 30.0, 0.0));
        let spot = DVec3::new(0.0, -6.0, 0.0);
        let a = harness.game.stage_enemy(EnemyKind::Grunt, spot);
        let b = harness.game.stage_enemy(EnemyKind::Grunt, spot);
        assert_eq!(
            harness.game.enemy_position(a),
            harness.game.enemy_position(b),
            "the two were not staged on the same point",
        );

        harness.run_ticks(120, &[]);
        let (Some(a), Some(b)) = (
            harness.game.enemy_position(a),
            harness.game.enemy_position(b),
        ) else {
            panic!("one of them died: nothing in this test can kill it");
        };
        let gap = (a - b).length();
        assert!(
            gap >= 2.0 * EnemyKind::Grunt.radius(),
            "a coincident pair is still coincident after two seconds: {gap}",
        );
    }

    /// **The separation query radius is exactly the neighbourhood**, which is an
    /// assumption about `crcbl-phys` and not about this file.
    ///
    /// [`separation_query_radius`] omits the *neighbour's* radius on purpose —
    /// see its docs — which is only correct because `overlap_sphere` is
    /// shape-aware. If that ever became an AABB-versus-point test the horde
    /// would quietly stop seeing half its neighbours and nothing else here would
    /// notice. The board below has a body just inside the boundary and one just
    /// outside it, for each pair of kinds, and the expected set is computed from
    /// the distances rather than read off a passing run.
    #[test]
    fn the_separation_query_radius_is_exactly_the_neighbourhood() {
        for subject in EnemyKind::ALL {
            for neighbour in EnemyKind::ALL {
                let mut harness = Harness::staged(60, 60, DVec3::new(0.0, 30.0, 0.0));
                let me = harness.game.stage_enemy(subject, DVec3::ZERO);
                let desired = subject.radius() + neighbour.radius() + SEPARATION_SLACK;
                let inside = harness
                    .game
                    .stage_enemy(neighbour, DVec3::new(desired - 0.01, 0.0, 0.0));
                let outside = harness
                    .game
                    .stage_enemy(neighbour, DVec3::new(0.0, -(desired + 0.01), 0.0));

                let found = harness.game.separation_neighbours(me);
                let mut expected = vec![me, inside];
                expected.sort_unstable_by_key(|entity| entity.to_bits());
                assert_eq!(
                    found,
                    expected,
                    "{subject:?} against {neighbour:?} at a desired gap of {desired}: \
                     the body at {} should be in and the one at {} should be out",
                    desired - 0.01,
                    desired + 0.01,
                );
                assert!(
                    !found.contains(&outside),
                    "{subject:?} saw a {neighbour:?} past its neighbourhood",
                );
            }
        }
    }

    /// The query returns the subject itself, which is why `steer_enemies`
    /// filters it out.
    ///
    /// A regression guard rather than a wish: the day `crcbl-phys` grows an
    /// entity-shaped overlap with an exclusion list (`docs/backlog.md`), this
    /// goes red and the filter can go.
    #[test]
    fn an_enemy_finds_itself_in_its_own_neighbourhood() {
        let mut harness = Harness::staged(60, 60, DVec3::new(0.0, 30.0, 0.0));
        let alone = harness.game.stage_enemy(EnemyKind::Grunt, DVec3::ZERO);
        assert_eq!(harness.game.separation_neighbours(alone), vec![alone]);
    }

    // ---- the weapon ----------------------------------------------------------

    /// **A bolt that crosses an enemy inside one tick still hits it.**
    ///
    /// The whole point of sweeping `prev → cur` rather than testing where the
    /// bolt ended up. The tick rate is turned down until one tick of travel is
    /// wider than the enemy, and the test then *proves the discrete test would
    /// have missed*: it reads the bolt's real position on one side, computes
    /// where the next tick puts it, asserts both are clear of the target, and
    /// only then asserts the kill.
    /// **The target is moving too**, which is why both positions are read from
    /// the simulation rather than assumed. A brute closing at 1.9 units a second
    /// covers half a unit per tick at this rate, which is more than its own
    /// radius — a version of this test that pinned the enemy at the origin
    /// "passed" while the bolt was in fact landing on a body that had walked
    /// into it, which is not the property being claimed.
    #[test]
    fn a_bolt_that_crosses_an_enemy_within_one_tick_still_hits_it() {
        // 4 Hz: a quarter-second step, so a bolt covers 7.5 units while the
        // brute it must not skip is 2 units across including the bolt.
        let mut harness = Harness::staged(4, 4, DVec3::new(0.0, -13.0, 0.0));
        let brute = harness.game.stage_enemy(EnemyKind::Brute, DVec3::ZERO);
        let dt = harness.game.tick_dt_secs();
        let reach = EnemyKind::Brute.radius() + BOLT_RADIUS;
        assert!(
            BOLT_SPEED * dt > 2.0 * reach,
            "this tick rate does not make the bolt skip the enemy at all: \
             {} against {}",
            BOLT_SPEED * dt,
            2.0 * reach,
        );

        // Tick one: the gun acquires and fires. Nothing has moved yet.
        harness.run_ticks(1, &[]);
        assert_eq!(harness.game.bolt_count(), 1, "nothing was fired");
        assert_eq!(
            harness.game.enemy_hp(brute),
            Some(EnemyKind::Brute.max_hp()),
            "something hit it before it was fired at",
        );

        // Tick two: one step of flight, which lands the bolt short. The oldest
        // bolt is the one fired first, because `bolts` is appended to.
        harness.run_ticks(2, &[]);
        let Some(bolt_before) = harness.game.bolts().first().map(|bolt| bolt.position) else {
            panic!("the bolt vanished before it reached anything");
        };
        let enemy_before = harness.game.enemy_position(brute).expect("the brute");
        assert!(
            (bolt_before - enemy_before).length() > reach,
            "a point test would already have hit: bolt {bolt_before:?}, \
             enemy {enemy_before:?}",
        );

        // Tick three: the step that goes straight over it.
        harness.run_ticks(3, &[]);
        let enemy_after = harness
            .game
            .enemy_position(brute)
            .expect("a brute survives one bolt");
        let bolt_after = bolt_before + DVec3::new(0.0, BOLT_SPEED * dt, 0.0);
        assert!(
            (bolt_after - enemy_after).length() > reach,
            "a point test would have hit on the far side: bolt {bolt_after:?}, \
             enemy {enemy_after:?}",
        );
        // Both ends of the step are clear of the enemy, at the enemy's real
        // position on each of those ticks — so nothing but the sweep can
        // account for the damage.
        let hp = harness.game.enemy_hp(brute).expect("still alive");
        assert!(
            (hp - (EnemyKind::Brute.max_hp() - BOLT_DAMAGE)).abs() < 1e-9,
            "the bolt stepped over the enemy: it went from {bolt_before:?} to \
             {bolt_after:?} past a body at {enemy_before:?} then {enemy_after:?}, \
             and the brute is on {hp}",
        );
        harness.assert_nothing_leaked();
    }

    /// **A bolt is never swept over ground it did not travel.**
    ///
    /// The reason the gun fires *after* the sweep — see `run_tick`. A sweep
    /// reconstructs `prev` as `position - velocity * dt`, so a bolt swept on the
    /// tick it was created is swept from a point one whole step *behind the
    /// muzzle*, through the thing that fired it, to the muzzle. At 60 Hz that
    /// segment is half a unit and hides inside the player; at 4 Hz it is 7.5
    /// units of arena behind them.
    ///
    /// The decoy below sits in exactly that stretch and is **further from the
    /// player than the target**, so the gun has no reason to aim at it — the
    /// only thing that can touch it is a sweep over ground the bolt never
    /// covered.
    #[test]
    fn a_bolt_is_never_swept_over_ground_it_did_not_travel() {
        let mut harness = Harness::staged(4, 4, DVec3::ZERO);
        let dt = harness.game.tick_dt_secs();
        let target = harness
            .game
            .stage_enemy(EnemyKind::Brute, DVec3::new(0.0, 3.0, 0.0));
        let behind = -(BOLT_SPEED * dt) / 2.0;
        let decoy = harness
            .game
            .stage_enemy(EnemyKind::Brute, DVec3::new(0.0, behind, 0.0));
        assert!(
            behind.abs() > 3.0,
            "the decoy at {behind} is nearer than the target, so the gun would \
             legitimately shoot it",
        );

        harness.run_ticks(2, &[]);
        assert_eq!(
            harness.game.enemy_hp(decoy),
            Some(EnemyKind::Brute.max_hp()),
            "the bolt was swept backwards through the player on the tick it \
             was fired, and hit something {behind} units behind them",
        );
        // Second, because a gun that fired nothing at all would leave the decoy
        // untouched too and satisfy the assertion above for the wrong reason.
        assert!(
            harness
                .game
                .enemy_hp(target)
                .is_some_and(|hp| hp < EnemyKind::Brute.max_hp()),
            "the gun never hit the target, so the decoy's health proves nothing",
        );
    }

    /// A bolt fired at an enemy at the ordinary tick rate hits it too — the case
    /// the CCD test above deliberately makes impossible for a point test,
    /// asserted here for the case that is not.
    ///
    /// And it takes exactly the number of bolts the damage table says, which is
    /// what makes [`BOLT_DAMAGE`] a number rather than a decoration.
    #[test]
    fn an_enemy_dies_to_exactly_the_number_of_bolts_its_hit_points_say() {
        for kind in EnemyKind::ALL {
            let mut harness = Harness::staged(60, 60, DVec3::new(0.0, -6.0, 0.0));
            let enemy = harness.game.stage_enemy(kind, DVec3::ZERO);
            let expected = (kind.max_hp() / BOLT_DAMAGE).ceil() as u64;

            // Long enough for `expected` shots plus their flight, and not so
            // long that the enemy reaches the player.
            let limit = harness.ticks + 60 * (expected + 2);
            let mut fired_at_death = None;
            while harness.ticks < limit && fired_at_death.is_none() {
                harness.run_ticks(harness.ticks + 1, &[]);
                if harness.game.kills == 1 {
                    fired_at_death = Some(harness.game.bolts_fired());
                }
            }

            let fired =
                fired_at_death.unwrap_or_else(|| panic!("{kind:?} never died in {limit} ticks"));
            assert_eq!(
                fired,
                expected,
                "{kind:?} took {fired} bolts against {} hit points at {BOLT_DAMAGE} each",
                kind.max_hp(),
            );
            assert!(
                harness.game.enemy_hp(enemy).is_none(),
                "{kind:?} was killed and is still on the list",
            );
            assert_eq!(harness.game.enemy_count(), 0);
            harness.assert_nothing_leaked();
        }
    }

    /// **Damage lands before death.** The intermediate state is what says the
    /// hit points are being subtracted rather than the enemy being deleted on
    /// the first touch.
    #[test]
    fn a_bolt_takes_hit_points_off_before_it_takes_the_enemy_off() {
        let mut harness = Harness::staged(60, 60, DVec3::new(0.0, -6.0, 0.0));
        let brute = harness.game.stage_enemy(EnemyKind::Brute, DVec3::ZERO);
        assert_eq!(
            harness.game.enemy_hp(brute),
            Some(EnemyKind::Brute.max_hp())
        );

        // One shot's flight: 6 units at 30 units a second is a fifth of a
        // second, and the cooldown is a quarter, so exactly one bolt has landed.
        harness.run_ticks(20, &[]);
        let hp = harness
            .game
            .enemy_hp(brute)
            .expect("a brute survives one bolt");
        assert!(
            (hp - (EnemyKind::Brute.max_hp() - BOLT_DAMAGE)).abs() < 1e-9,
            "one bolt took the brute to {hp}",
        );
        assert_eq!(harness.game.kills, 0, "it died to one bolt");
    }

    /// The gun aims at the **nearest** enemy, not at whichever the broadphase
    /// happened to hand back first.
    ///
    /// Four targets rather than two, and the assertion is the **order they die
    /// in**. With two it is a coin toss whether a gun that took the first result
    /// off the tree happened to pick the right one — that version of this test
    /// passed while `min_by` was replaced by `next()`. Four one-shot runners at
    /// even spacing, staged in a scrambled order, is a one-in-twenty-four
    /// coincidence instead.
    #[test]
    fn the_gun_shoots_the_nearest_enemy_first() {
        let mut harness = Harness::staged(60, 60, DVec3::ZERO);
        // Distances 3, 6, 9, 12, staged 12, 3, 9, 6 — so neither the list order
        // nor its reverse is the answer — and each at a different bearing, so
        // the broadphase's own traversal order is a spatial partition rather
        // than a distance ranking. Four in a line does not distinguish the two:
        // the tree visits a colinear board nearest-first anyway, and that
        // version of this test passed under `next()`.
        let mut staged: Vec<(f64, Entity)> = Vec::new();
        for (distance, bearing) in [(12.0, 290.0), (3.0, 200.0), (9.0, 110.0), (6.0, 20.0)] {
            let angle: f64 = f64::to_radians(bearing);
            let entity = harness.game.stage_enemy(
                EnemyKind::Runner,
                DVec3::new(angle.cos(), angle.sin(), 0.0) * distance,
            );
            staged.push((distance, entity));
        }
        assert!(
            staged.iter().all(|(d, _)| *d < WEAPON_RANGE),
            "a target out of range would never be shot at all",
        );

        // Every runner dies to one bolt, so the order they leave the field in is
        // the order the gun chose them in.
        let mut order = Vec::new();
        while order.len() < staged.len() && harness.ticks < 300 {
            harness.run_ticks(harness.ticks + 1, &[]);
            for (distance, entity) in &staged {
                if harness.game.enemy_hp(*entity).is_none() && !order.contains(distance) {
                    order.push(*distance);
                }
            }
        }
        assert_eq!(
            order,
            vec![3.0, 6.0, 9.0, 12.0],
            "the gun did not work outwards from the player",
        );
        assert_eq!(harness.game.kills, 4);
        harness.assert_nothing_leaked();
    }

    /// Nothing in range is nothing fired, and the cooldown is not spent on it.
    #[test]
    fn the_gun_holds_its_fire_when_nothing_is_in_range() {
        let mut harness = Harness::staged(60, 60, DVec3::ZERO);
        // Comfortably outside the weapon's reach, and stationary because it is
        // only there to prove the gun can see nothing rather than that the field
        // is empty.
        harness
            .game
            .stage_enemy(EnemyKind::Grunt, DVec3::new(WEAPON_RANGE + 20.0, 0.0, 0.0));
        harness.run_ticks(30, &[]);
        assert_eq!(harness.game.bolts_fired(), 0, "it shot at nothing");

        // Now put something in reach: the gun must fire on the very next tick,
        // not a cooldown later.
        harness
            .game
            .stage_enemy(EnemyKind::Grunt, DVec3::new(3.0, 0.0, 0.0));
        harness.run_ticks(harness.ticks + 1, &[]);
        assert_eq!(
            harness.game.bolts_fired(),
            1,
            "the gun was not ready when a target arrived",
        );
    }

    /// **The wizard faces the way the input last pointed**, and nothing else
    /// turns it.
    ///
    /// Every clause here is a way of getting it wrong that would look fine in a
    /// screenshot: a facing taken from the velocity turns the wrong way against
    /// a wall, one taken from the aim spins with the crowd, one that resets on
    /// key-up flickers every time the player stops, and one driven by any key
    /// rather than the horizontal pair turns on `W`.
    #[test]
    fn the_wizard_faces_the_way_the_input_last_pointed() {
        let mut harness = Harness::staged(60, 60, DVec3::ZERO);
        let facing = |harness: &Harness| lock(&harness.game.shared).player_facing;
        let walking = |harness: &Harness| lock(&harness.game.shared).player_moving;

        assert_eq!(facing(&harness), Facing::Right, "the way it is drawn");
        assert!(!walking(&harness), "nobody has pressed anything");

        let step = |harness: &mut Harness, key, down| {
            harness.game.key_event(key, down);
            harness.run_ticks(harness.ticks + 1, &[]);
        };

        step(&mut harness, KeyCode::KeyA, true);
        assert_eq!(facing(&harness), Facing::Left);
        assert!(walking(&harness));

        // …and both reach the renderer. `art::Scene::build` takes a
        // `RenderState` and nothing else, so a `render_state` that forgot either
        // field would leave the wizard permanently facing right and standing
        // still, with every assertion in this test still passing.
        let mut out = RenderState::default();
        harness.game.render_state(&mut out);
        assert_eq!(out.player_facing, Facing::Left);
        assert!(out.player_walking);

        // Released, and it keeps the facing it had. This is the flicker.
        step(&mut harness, KeyCode::KeyA, false);
        assert_eq!(facing(&harness), Facing::Left, "it snapped back on key-up");
        assert!(!walking(&harness));

        // Straight up: walking, and still facing left, because facing is a
        // left/right property and `W` says nothing about it.
        step(&mut harness, KeyCode::KeyW, true);
        assert_eq!(facing(&harness), Facing::Left, "`W` turned the wizard");
        assert!(walking(&harness));
        step(&mut harness, KeyCode::KeyW, false);

        step(&mut harness, KeyCode::KeyD, true);
        assert_eq!(facing(&harness), Facing::Right);

        // Both horizontals: nothing is being asked for, so the facing stands and
        // the wizard is not walking either.
        step(&mut harness, KeyCode::KeyA, true);
        assert_eq!(
            facing(&harness),
            Facing::Right,
            "a cancelled input turned it"
        );
        assert!(!walking(&harness), "a cancelled input walked it");
        step(&mut harness, KeyCode::KeyD, false);
        assert_eq!(facing(&harness), Facing::Left, "and `A` is still down");

        // Dead is not walking, whatever is held. Without this the death screen
        // shows a corpse walking on the spot.
        harness.game.set_player_hp(0.000_1);
        harness
            .game
            .stage_enemy(EnemyKind::Brute, harness.game.player);
        harness.run_ticks(harness.ticks + 8, &[]);
        assert_eq!(harness.game.state, GameState::Dead);
        assert!(!walking(&harness), "the dead wizard kept walking");
    }

    /// **A bolt leaves the head of the staff, on the side the wizard is
    /// facing — even when that is the side the target is not on.**
    ///
    /// The decision [`staff_muzzle`] records, asserted rather than described.
    /// The wizard is turned left and the only enemy is due east, so a muzzle
    /// mirrored to the *firing* side and a muzzle mirrored to the *facing* side
    /// are a body's width apart and this can tell them apart.
    #[test]
    fn a_bolt_leaves_the_staff_on_the_side_the_wizard_faces() {
        let mut harness = Harness::staged(60, 60, DVec3::ZERO);
        // One tick of `A`, so the wizard is turned and then standing still: the
        // bolt's position is read against where the player actually is, and a
        // player still moving would make that a moving target.
        harness.game.key_event(KeyCode::KeyA, true);
        harness.run_ticks(harness.ticks + 1, &[]);
        harness.game.key_event(KeyCode::KeyA, false);
        harness.run_ticks(harness.ticks + 2, &[]);
        assert_eq!(lock(&harness.game.shared).player_facing, Facing::Left);
        assert_eq!(harness.game.bolts_fired(), 0, "there was nothing to shoot");

        // Now give it a target on the other side, and let it fire exactly once.
        let target = DVec3::new(4.0, 0.0, 0.0);
        harness.game.stage_enemy(EnemyKind::Grunt, target);
        harness.run_ticks(harness.ticks + 1, &[]);
        assert_eq!(harness.game.bolts_fired(), 1);

        let bolt = harness.game.bolts()[0].position;
        let player = harness.game.player;
        let want = player + staff_muzzle(Facing::Left);
        assert!(
            (bolt - want).length() < 1e-9,
            "the bolt started at {bolt:?}, and the staff head is at {want:?}",
        );
        // …and the two candidates really are far apart, so the assertion above
        // is not satisfied by both of them.
        let mirrored = player + staff_muzzle(Facing::Right);
        assert!(
            (want - mirrored).length() > 2.0 * STAFF_MUZZLE.x - 1e-9,
            "the two muzzles are the same point, so this test cannot fail",
        );
        // The bolt starts behind the aim and flies through the wizard, which is
        // the documented consequence and not an accident.
        assert!(
            bolt.x < player.x,
            "the bolt did not start on the staff side"
        );
        assert!(
            harness.game.bolts()[0].position.x < target.x,
            "the bolt did not start short of its target",
        );
    }

    /// **A bolt outlives the range it was fired at.** A shot that expired short
    /// of its target would make [`WEAPON_RANGE`] a lie.
    ///
    /// The relation is asserted, not the number, so a later tuning pass that
    /// changes either constant is told rather than left to find out.
    #[test]
    fn the_reach_of_a_bolt_covers_the_weapons_range() {
        let reach = BOLT_SPEED * BOLT_LIFE;
        // The staff head is the third term: the range is measured from the
        // player's centre and the bolt starts at the muzzle, which can be on the
        // far side of the wizard from the target.
        assert!(
            reach > WEAPON_RANGE + max_enemy_radius() + STAFF_MUZZLE.length(),
            "a bolt reaches {reach}, short of a target at {WEAPON_RANGE}",
        );
        // …and does not cross the whole arena, or the range is decoration.
        assert!(reach < ARENA_HALF_WIDTH);
    }

    /// A bolt that hits nothing expires, and takes its entity with it.
    #[test]
    fn a_bolt_that_hits_nothing_expires() {
        let mut harness = Harness::staged(60, 60, DVec3::ZERO);
        // A runner well off to one side: in range, so the gun fires, and fast
        // enough that it has left the bolt's line by the time the bolt arrives.
        harness
            .game
            .stage_enemy(EnemyKind::Runner, DVec3::new(0.0, 12.0, 0.0));
        harness.run_ticks(1, &[]);
        assert_eq!(harness.game.bolt_count(), 1, "nothing was fired");

        // The bolt outlives its target, so take the target away and let the
        // bolt fly on into an empty arena. Removing it is not a kill — nothing
        // shot it — which is what the count below says.
        harness.game.clear_enemies();
        let life_ticks = (BOLT_LIFE / harness.game.tick_dt_secs()).ceil() as u64;
        harness.run_ticks(harness.ticks + life_ticks + 3, &[]);

        assert_eq!(harness.game.bolt_count(), 0, "the bolt outlived its life");
        assert_eq!(harness.game.kills, 0, "it hit something it should not have");
        assert_eq!(
            harness.game.entity_count(),
            1,
            "the expired bolt left its entity behind: the player should be all \
             that is left",
        );
        assert_eq!(harness.game.pending_despawns(), 0, "the sweep never ran");
        harness.assert_nothing_leaked();
    }

    // ---- contact damage, death and the clock ---------------------------------

    /// **Contact damage applies at the stated rate, and stops when it stops.**
    ///
    /// The rate is asserted against the table rather than "hit points went
    /// down": a game that took one point per touching enemy per tick would pass
    /// the weaker version and would make the tick rate the difficulty.
    #[test]
    fn contact_damage_runs_at_the_stated_rate_and_stops_when_it_does() {
        let mut harness = Harness::staged(60, 60, DVec3::ZERO);
        // A brute, because it survives long enough under the gun to keep
        // touching, and because it is the loudest number in the table.
        harness.game.stage_enemy(
            EnemyKind::Brute,
            DVec3::new(0.0, PLAYER_RADIUS + EnemyKind::Brute.radius() - 0.1, 0.0),
        );

        let ticks = 30;
        harness.run_ticks(ticks, &[]);
        let taken = PLAYER_MAX_HP - harness.game.player_hp;
        let expected = EnemyKind::Brute.contact_dps() * ticks as f64 * harness.game.tick_dt_secs();
        assert!(
            (taken - expected).abs() < 0.5,
            "half a second against a brute took {taken} hit points, not {expected}",
        );

        // Walk away: nothing is touching, so nothing more is taken.
        harness.game.clear_enemies();
        let after = harness.game.player_hp;
        harness.run_ticks(harness.ticks + 60, &[]);
        assert_eq!(
            harness.game.player_hp, after,
            "hit points kept draining with an empty arena",
        );
    }

    /// **A crowd is worse than one enemy**, which is the whole argument for a
    /// damage *rate* summed over what is touching.
    #[test]
    fn standing_in_a_crowd_hurts_more_than_standing_next_to_one() {
        let taken = |count: usize| {
            let mut harness = Harness::staged(60, 60, DVec3::ZERO);
            let angle = std::f64::consts::TAU / count as f64;
            for i in 0..count {
                let a = angle * i as f64;
                let r = PLAYER_RADIUS + EnemyKind::Grunt.radius() - 0.1;
                harness
                    .game
                    .stage_enemy(EnemyKind::Grunt, DVec3::new(a.cos(), a.sin(), 0.0) * r);
            }
            harness.run_ticks(6, &[]);
            PLAYER_MAX_HP - harness.game.player_hp
        };
        let one = taken(1);
        let six = taken(6);
        assert!(one > 0.0, "one grunt did no damage at all");
        assert!(
            six > 4.0 * one,
            "six grunts did {six} against one grunt's {one}",
        );
    }

    /// **Hit points reach zero, and that is the death screen.**
    ///
    /// The clock stops and the kill count freezes, which is what makes the
    /// screen a report of the run rather than a live HUD with a caption.
    #[test]
    fn hit_points_reach_zero_and_the_run_ends_with_its_numbers_frozen() {
        let mut harness = Harness::staged(60, 60, DVec3::ZERO);
        harness.game.stage_enemy(
            EnemyKind::Brute,
            DVec3::new(0.0, PLAYER_RADIUS + EnemyKind::Brute.radius() - 0.1, 0.0),
        );
        // Two ticks' worth of a brute, so the death arrives inside this test
        // rather than four seconds into it.
        harness
            .game
            .set_player_hp(EnemyKind::Brute.contact_dps() * 2.0 * harness.game.tick_dt_secs());

        harness.run_ticks(30, &[]);
        assert_eq!(harness.game.state, GameState::Dead, "the run did not end");
        assert_eq!(harness.game.player_hp, 0.0, "hit points went negative");
        let elapsed = harness.game.elapsed;
        let kills = harness.game.kills;
        assert!(elapsed > 0.0, "the run ended before it started");

        harness.run_ticks(harness.ticks + 120, &[]);
        assert_eq!(harness.game.state, GameState::Dead);
        assert_eq!(
            harness.game.elapsed, elapsed,
            "the clock kept running after death",
        );
        assert_eq!(harness.game.kills, kills, "kills kept counting after death");
        assert_eq!(
            harness.game.player_hp, 0.0,
            "a dead player kept taking damage",
        );
        harness.assert_nothing_leaked();
    }

    /// **The horde keeps moving behind the death screen**, which is what makes
    /// it a game over rather than a screenshot.
    #[test]
    fn the_horde_keeps_converging_after_the_player_dies() {
        let mut harness = Harness::staged(60, 60, DVec3::ZERO);
        let far = harness
            .game
            .stage_enemy(EnemyKind::Grunt, DVec3::new(0.0, -30.0, 0.0));
        harness.game.stage_enemy(
            EnemyKind::Brute,
            DVec3::new(0.0, PLAYER_RADIUS + EnemyKind::Brute.radius() - 0.1, 0.0),
        );
        harness.game.set_player_hp(0.5);

        harness.run_ticks(2, &[]);
        assert_eq!(harness.game.state, GameState::Dead);
        let before = harness.game.enemy_position(far).expect("the far grunt");

        harness.run_ticks(harness.ticks + 60, &[]);
        let after = harness.game.enemy_position(far).expect("the far grunt");
        assert!(
            after.y > before.y + 2.0,
            "the horde froze with the clock: {before:?} to {after:?}",
        );
    }

    /// **The clock counts simulated seconds**, not frames and not ticks.
    #[test]
    fn the_clock_counts_simulated_seconds() {
        for tick_hz in [20, 60, 144] {
            let mut harness = Harness::staged(60, tick_hz, DVec3::ZERO);
            harness.run_ticks(u64::from(tick_hz) * 3, &[]);
            assert!(
                (harness.game.elapsed - 3.0).abs() < 0.05,
                "{tick_hz} Hz reported {} seconds for three",
                harness.game.elapsed,
            );
        }
    }

    // ---- the title screen ----------------------------------------------------

    /// **The title screen does not play the game.**
    ///
    /// Ten simulated seconds of it leave the world bit-identical. Asserted on
    /// the whole of [`RenderState`] — the struct the renderer draws from, and
    /// the only thing a player can actually see — rather than on the state enum,
    /// because an enum comparison passes just as happily on a simulation that
    /// ran every line of its tick and merely mislabelled itself.
    ///
    /// The tick count is asserted too: a run that never ticked would satisfy
    /// "nothing changed" without testing anything at all.
    #[test]
    fn the_title_screen_does_not_advance_the_simulation() {
        let mut harness = Harness::waiting(60, 60);
        assert_eq!(harness.game.state, GameState::WaitingToStart);
        let mut before = RenderState::default();
        harness.game.render_state(&mut before);

        harness.run_ticks(600, &[]);

        let mut after = RenderState::default();
        harness.game.render_state(&mut after);
        assert_eq!(harness.ticks, 600, "the frames ran no ticks");
        assert_eq!(
            before, after,
            "ten seconds of title screen changed the world"
        );
        // …and the same again through the facade's own mirrors, which are what
        // the HUD and the browser gate read. `SPAWN_INTERVAL_START` is half a
        // second, so a spawner that ran for ten of them owed nineteen enemies.
        assert_eq!(harness.game.state, GameState::WaitingToStart);
        assert_eq!(harness.game.enemies_spawned(), 0, "the spawner ran");
        assert_eq!(harness.game.bolts_fired(), 0, "the gun fired");
        assert_eq!(harness.game.elapsed, 0.0, "the run clock ran");
        assert_eq!(harness.game.enemy_count(), 0);
        assert_eq!(harness.game.player, DVec3::ZERO);
    }

    /// **Either key that starts a run starts it**, and what follows is a run:
    /// the clock moves and the spawner deals.
    ///
    /// The run counter is asserted *not* to move, which is the difference
    /// between starting and restarting — a start implemented as a restart would
    /// hand the session's first run the second run's horde.
    #[test]
    fn the_start_key_begins_the_run() {
        for key in [KeyCode::Space, KeyCode::KeyR] {
            let mut harness = Harness::waiting(60, 60);
            harness.run_ticks(60, &[]);
            assert_eq!(harness.game.state, GameState::WaitingToStart, "{key:?}");
            let seed = harness.game.run_seed();

            harness.tap(key);
            assert_eq!(
                harness.game.state,
                GameState::Playing,
                "{key:?} did not start the run",
            );
            assert_eq!(harness.game.run, 1, "{key:?} restarted instead of starting");
            assert_eq!(harness.game.run_seed(), seed, "{key:?} re-dealt the run");

            harness.run_ticks(harness.ticks + 120, &[]);
            assert!(
                harness.game.elapsed > 1.5,
                "{key:?}: the clock never started: {}",
                harness.game.elapsed,
            );
            assert!(
                harness.game.enemies_spawned() > 0,
                "{key:?}: the spawner never dealt",
            );
        }
    }

    /// A restart puts the clock, the hit points, the kills and the player back,
    /// and deals a horde that is not the one just played.
    ///
    /// **It lands on the title screen**, which is the one thing here that
    /// changed when the start screen arrived: the board it puts back is the
    /// same, and it takes a second edge to be playing on it.
    #[test]
    fn a_restart_puts_everything_back_and_deals_a_new_horde() {
        let mut harness = Harness::new(60, 60);
        harness.play_ticks(600);
        let first_seed = harness.game.run_seed();
        assert!(
            harness.game.enemy_count() > 0,
            "the run has to have dealt something first",
        );
        harness.game.stage_player(DVec3::new(11.0, -7.0, 0.0));

        harness.tap(KeyCode::KeyR);
        assert_eq!(harness.game.state, GameState::WaitingToStart);
        assert_eq!(harness.game.kills, 0);
        assert_eq!(harness.game.player_hp, PLAYER_MAX_HP);
        assert_eq!(
            harness.game.player,
            DVec3::ZERO,
            "the player was not moved back"
        );
        assert_eq!(harness.game.enemy_count(), 0, "the field was not cleared");
        assert_eq!(harness.game.bolt_count(), 0, "a bolt survived the restart");
        assert!(
            harness.game.elapsed < harness.game.tick_dt_secs() * 2.0,
            "the clock was not reset: {}",
            harness.game.elapsed,
        );
        assert_ne!(
            harness.game.run_seed(),
            first_seed,
            "a restart re-dealt the identical run, so the seed advance did nothing",
        );
        harness.assert_nothing_leaked();

        // And the second edge is what plays it, on the board the first one
        // dealt: the seed does not move again.
        let dealt = harness.game.run_seed();
        harness.tap(KeyCode::KeyR);
        assert_eq!(harness.game.state, GameState::Playing);
        assert_eq!(
            harness.game.run_seed(),
            dealt,
            "leaving the title screen re-dealt the run",
        );
    }

    /// A dead run restarts, which is the only way out of the death screen —
    /// **onto the title screen**, and a second press from there into play.
    #[test]
    fn restarting_after_a_death_starts_a_new_run() {
        let mut harness = Harness::staged(60, 60, DVec3::ZERO);
        harness.game.stage_enemy(
            EnemyKind::Brute,
            DVec3::new(0.0, PLAYER_RADIUS + EnemyKind::Brute.radius() - 0.1, 0.0),
        );
        harness.game.set_player_hp(0.5);
        harness.run_ticks(2, &[]);
        assert_eq!(harness.game.state, GameState::Dead);

        harness.tap(KeyCode::Space);
        assert_eq!(harness.game.state, GameState::WaitingToStart);
        assert_eq!(harness.game.player_hp, PLAYER_MAX_HP);
        assert_eq!(harness.game.enemy_count(), 0);

        harness.tap(KeyCode::Space);
        assert_eq!(harness.game.state, GameState::Playing);
    }

    // ---- spawning ------------------------------------------------------------

    /// The spawn rate ramps and then stops ramping, and never runs backwards.
    #[test]
    fn the_spawn_rate_ramps_to_a_floor_and_no_further() {
        assert_eq!(spawn_interval(0.0), SPAWN_INTERVAL_START);
        assert_eq!(spawn_interval(SPAWN_RAMP_SECONDS), SPAWN_INTERVAL_MIN);
        assert_eq!(
            spawn_interval(SPAWN_RAMP_SECONDS * 10.0),
            SPAWN_INTERVAL_MIN
        );
        assert_eq!(spawn_interval(-5.0), SPAWN_INTERVAL_START, "no time travel");
        let mut previous = f64::INFINITY;
        for step in 0..600 {
            let interval = spawn_interval(f64::from(step));
            assert!(interval <= previous, "the rate went backwards at {step}s");
            assert!(interval >= SPAWN_INTERVAL_MIN);
            previous = interval;
        }
        assert!(
            spawn_interval(SPAWN_RAMP_SECONDS / 2.0) < SPAWN_INTERVAL_START,
            "the ramp does nothing in the middle",
        );
    }

    /// The spawner deals all three kinds, in something like the table's
    /// proportions.
    ///
    /// A share, not a count, and asserted loosely — the point is that no kind is
    /// unreachable, which is exactly what a mistyped comparison in
    /// [`EnemyKind::from_roll`] would produce and what no other test would see.
    #[test]
    fn the_spawner_deals_every_kind() {
        let mut counts = [0u32; 3];
        for counter in 0..10_000 {
            let index = match spawn_kind(DEFAULT_SEED, counter) {
                EnemyKind::Grunt => 0,
                EnemyKind::Runner => 1,
                EnemyKind::Brute => 2,
            };
            counts[index] += 1;
        }
        for (index, kind) in EnemyKind::ALL.iter().enumerate() {
            assert!(
                counts[index] > 500,
                "{kind:?} came up {} times in ten thousand",
                counts[index],
            );
        }
        assert!(
            counts[0] > counts[1] && counts[1] > counts[2],
            "the mix is not the table's: {counts:?}",
        );
    }

    /// **The field never exceeds its cap**, and the cap is genuinely reached
    /// rather than being a number nothing gets near.
    #[test]
    fn the_field_never_exceeds_the_enemy_cap() {
        const CAP: usize = 25;
        let mut harness = Harness::with_setup(
            60,
            &Setup {
                headless: true,
                max_enemies: CAP,
                ..Setup::default()
            },
        );
        // **The player is kept alive by hand**, because the spawner only runs
        // while the run is: a stationary player is dead inside ten seconds, and
        // what would then be under test is the death screen rather than the cap.
        let mut peak = 0;
        while harness.ticks < 7_200 {
            harness.game.set_player_hp(PLAYER_MAX_HP);
            // The spawner does not run while the level-up screen is up, so a
            // run that walked past one would stop spawning and the cap would
            // never be reached — see `Harness::play_ticks`.
            if harness.game.state == GameState::LevelUp {
                harness.game.key_event(KeyCode::Digit1, true);
                harness.game.key_event(KeyCode::Digit1, false);
            }
            harness.run_ticks(harness.ticks + 1, &[]);
            peak = peak.max(harness.game.enemy_count());
            assert!(
                harness.game.enemy_count() <= CAP,
                "tick {}: {} enemies against a cap of {CAP}",
                harness.ticks,
                harness.game.enemy_count(),
            );
        }
        assert_eq!(peak, CAP, "the cap was never reached, so it is untested");
        assert!(
            harness.game.enemies_spawned() > CAP as u64,
            "only {} were ever spawned, so the cap did nothing",
            harness.game.enemies_spawned(),
        );
        harness.assert_nothing_leaked();
    }

    /// The list and the entity index stay in step across a `swap_remove`.
    ///
    /// The failure this guards is silent and specific: `swap_remove` moves the
    /// *last* enemy into the hole, and the map entry that named its old slot has
    /// to follow it. Forget that and every later lookup of the moved enemy
    /// resolves to the wrong body — a bolt damages a stranger, and nothing
    /// panics.
    #[test]
    fn an_enemy_index_survives_a_swap_remove() {
        let mut harness = Harness::staged(60, 60, DVec3::new(0.0, -6.0, 0.0));
        // Three in a row up the gun's line, so the nearest dies first and the
        // last one in the list is moved into its slot.
        let near = harness
            .game
            .stage_enemy(EnemyKind::Runner, DVec3::new(0.0, 0.0, 0.0));
        let mid = harness
            .game
            .stage_enemy(EnemyKind::Brute, DVec3::new(0.0, 4.0, 0.0));
        let far = harness
            .game
            .stage_enemy(EnemyKind::Brute, DVec3::new(0.0, 6.0, 0.0));

        harness.run_ticks(30, &[]);
        assert_eq!(harness.game.kills, 1, "the nearest should have died");
        assert!(harness.game.enemy_hp(near).is_none());
        // Both survivors still resolve, and to themselves: a broken index would
        // hand back one of them for the other, or nothing at all.
        assert!(
            harness.game.enemy_hp(mid).is_some(),
            "the middle one is lost"
        );
        assert!(harness.game.enemy_hp(far).is_some(), "the far one is lost");
        assert!(
            harness.game.enemy_position(far).expect("the far one").y
                > harness.game.enemy_position(mid).expect("the middle one").y,
            "the two swapped identities",
        );
        harness.assert_nothing_leaked();
    }

    // ---- the entity lifecycle, under pressure --------------------------------

    /// **Thousands of bodies come and go, and nothing is left behind.**
    ///
    /// Staged rather than played, because the point is the *count*: three
    /// thousand colliders inserted into the broadphase, indexed, and taken out
    /// again, with the entity count, the collider count and the destruction
    /// queue all accounted for exactly at each stage. A run that reached three
    /// thousand through the spawner would take a simulated hour.
    #[test]
    fn thousands_of_bodies_come_and_go_without_leaking() {
        const BODIES: usize = 3_000;
        let mut harness = Harness::staged(60, 60, DVec3::ZERO);
        let baseline = harness.game.entity_count();
        assert_eq!(baseline, 1, "a staged board is the player and nothing else");

        // A grid well clear of the player, so nothing is shot and nothing does
        // contact damage while it is being counted.
        let mut staged = Vec::with_capacity(BODIES);
        for i in 0..BODIES {
            let x = -40.0 + (i % 60) as f64 * 1.3;
            let y = 20.0 + (i / 60) as f64 * 0.25;
            staged.push(
                harness
                    .game
                    .stage_enemy(EnemyKind::Grunt, DVec3::new(x, y, 0.0)),
            );
        }
        assert_eq!(harness.game.enemy_count(), BODIES);
        assert_eq!(
            harness.game.collider_count(),
            BODIES,
            "colliders went missing"
        );
        assert_eq!(harness.game.entity_count(), 1 + BODIES);

        // One tick, so every one of them is steered, queried and clamped at
        // least once — a leak that only happens on the hot path would otherwise
        // never be reached.
        harness.run_ticks(1, &[]);
        harness.assert_nothing_leaked();

        // Cleared through the **restart** the game itself runs, not through a
        // test helper: a bulk teardown is the thing under test here, and a
        // helper that removed the colliders would prove only that the helper
        // does.
        harness.tap(KeyCode::KeyR);
        assert_eq!(harness.game.enemy_count(), 0);
        assert_eq!(
            harness.game.collider_count(),
            0,
            "the broadphase kept {BODIES} invisible walls",
        );

        // The ECS sweeps at the end of `World::tick` and the module runs after
        // it, so the pool needs a tick to let go.
        harness.run_ticks(harness.ticks + 3, &[]);
        assert_eq!(
            harness.game.pending_despawns(),
            0,
            "the queue never emptied"
        );
        assert_eq!(
            harness.game.entity_count(),
            baseline,
            "{BODIES} bodies left something behind",
        );
        harness.assert_nothing_leaked();
        // And the run genuinely churned, which is what stops all of the above
        // being true of a game that did nothing.
        assert!(harness.game.enemies_spawned() >= BODIES as u64);
        assert_eq!(staged.len(), BODIES);
    }

    /// **A long run leaks nothing**, checked on every tick rather than at the
    /// end — a leak that is cleaned up before the last tick is still a leak
    /// while it is happening, and the failure names the tick it started on.
    ///
    /// Smaller than `thousands_of_bodies_come_and_go_without_leaking` in bodies
    /// and much larger in ticks, because the two catch different things: that
    /// one catches a collider left behind by a bulk removal, this one catches a
    /// body leaked by the spawn/kill/expire paths a thousand ticks in.
    #[test]
    fn a_long_run_leaks_nothing() {
        let mut harness = Harness::with_setup(
            60,
            &Setup {
                headless: true,
                max_enemies: 120,
                ..Setup::default()
            },
        );
        let mut peak = 0;
        while harness.ticks < 9_000 {
            harness.play_ticks(harness.ticks + 1);
            harness.assert_nothing_leaked();
            peak = peak.max(harness.game.entity_count());
        }

        let spawned = harness.game.enemies_spawned();
        let fired = harness.game.bolts_fired();
        assert!(
            spawned >= 250,
            "only {spawned} enemies were ever spawned, which is not enough churn",
        );
        assert!(
            fired >= 400,
            "only {fired} bolts were ever fired, which is not enough churn",
        );
        assert!(
            harness.game.kills > 0,
            "the run killed nothing, so the enemy death path was never exercised",
        );
        assert!(
            harness.restarts > 0,
            "the soak never finished a run, so the restart — which wipes the \
             whole field and is the largest single piece of churn in the game — \
             was never exercised",
        );
        // The most the world can legitimately hold: the player, the enemy cap,
        // every bolt that can be in the air at once and every gem the ground
        // will keep — plus as many again waiting for the deferred sweep. Derived
        // rather than measured: a number taken from a passing run breaks on the
        // next tuning change for a reason that is not a leak.
        //
        // **The gems were missing from this and it passed anyway**, because a
        // soak that killed a little less left fewer of them lying about than the
        // enemy cap did. They are the largest population in the run by a wide
        // margin, so leaving them out was a bound on the wrong thing; the
        // per-tick equality in `assert_nothing_leaked` is what carries the exact
        // claim, and this is the growth bound over it.
        let bolts_in_flight = (BOLT_LIFE / FIRE_COOLDOWN).ceil() as usize + 1;
        let ceiling = 2 * (1 + 120 + bolts_in_flight + MAX_PICKUPS);
        assert!(
            peak <= ceiling,
            "the world peaked at {peak} entities against a ceiling of {ceiling}",
        );
        assert!(
            peak > 1,
            "the world never grew, so the ceiling proves nothing"
        );

        harness.run_ticks(harness.ticks + 3, &[]);
        assert_eq!(
            harness.game.pending_despawns(),
            0,
            "the destruction queue never emptied",
        );
        harness.assert_nothing_leaked();
        crcbl::log::info!(
            "soak: {spawned} spawned, {fired} bolts, {} kills, {} restarts, \
             peak {peak} entities",
            harness.game.kills,
            harness.restarts,
        );
    }

    // ---- determinism ---------------------------------------------------------

    /// **The determinism criterion.** The same script replays to the same
    /// outcome, twice, bit-identically.
    ///
    /// Everything observable is compared, not just the kill count: a run that
    /// agreed about the number and disagreed about where the horde was would be
    /// a coincidence, not determinism. That includes every enemy position, which
    /// is what makes it a test of the separation sum's order as well — see
    /// `steer_enemies`.
    #[test]
    fn the_same_script_replays_bit_identically() {
        let run = || {
            let mut harness = Harness::with_setup(
                60,
                &Setup {
                    headless: true,
                    max_enemies: 120,
                    ..Setup::default()
                },
            );
            harness.play_ticks(2_400);
            (
                harness.game.elapsed,
                harness.game.kills,
                harness.game.player_hp,
                harness.game.player,
                harness.game.enemies(),
                harness.game.bolts(),
                harness.game.enemies_spawned(),
                harness.game.bolts_fired(),
            )
        };
        let first = run();
        assert!(
            first.6 > 50,
            "the reference run spawned {}, which is not enough to compare",
            first.6,
        );
        assert!(
            !first.4.is_empty(),
            "the reference run ended with an empty field"
        );
        assert!(first.7 > 0, "the reference run fired nothing");
        assert!(first.1 > 0, "the reference run killed nothing");
        assert_eq!(first, run());
    }

    /// **The frame rate is not the tick rate.** The same script reaches the same
    /// place at 20, 60 and 240 frames a second, because the simulation runs on
    /// its own fixed step and the frame loop only decides how often it is asked
    /// to.
    #[test]
    fn the_same_run_plays_out_the_same_at_every_frame_rate() {
        type Observed = (f64, u64, f64, DVec3, Vec<EnemyView>);
        let mut reference: Option<Observed> = None;
        for frame_hz in [20, 60, 240] {
            let mut harness = Harness::with_setup(
                frame_hz,
                &Setup {
                    headless: true,
                    max_enemies: 80,
                    ..Setup::default()
                },
            );
            harness.play_ticks(900);
            assert_eq!(harness.ticks, 900);
            let observed = (
                harness.game.elapsed,
                harness.game.kills,
                harness.game.player_hp,
                harness.game.player,
                harness.game.enemies(),
            );
            match &reference {
                None => {
                    assert!(!observed.4.is_empty(), "the reference run spawned nothing");
                    reference = Some(observed);
                }
                Some(expected) => assert_eq!(
                    &observed, expected,
                    "{frame_hz} fps played a different game",
                ),
            }
        }
    }

    /// Two games given the same seed play the same game, and two given different
    /// seeds do not — without the second half the seed would be decoration.
    #[test]
    fn two_games_on_one_seed_play_the_same_game() {
        let run = |seed: u64| {
            let mut harness = Harness::with_setup(
                60,
                &Setup {
                    headless: true,
                    seed,
                    max_enemies: 80,
                    ..Setup::default()
                },
            );
            harness.play_ticks(600);
            harness.game.enemies()
        };
        for seed in [1, DEFAULT_SEED] {
            assert_eq!(run(seed), run(seed), "seed {seed} was not reproducible");
        }
        assert_ne!(run(1), run(2), "two seeds played the same game");
    }

    /// …and a *restart* is predictable too, so a recorded script replayed from a
    /// fresh game meets the same horde on its second run as the recording did on
    /// its second run.
    #[test]
    fn two_games_with_one_seed_agree_about_every_run() {
        let seeds = |restarts: u32| {
            let mut harness = Harness::staged(60, 60, DVec3::ZERO);
            // One tick to spend the harness's queued start edge, or the first
            // `restart_run` below would find a game still on the title screen
            // and merely start it.
            harness.run_ticks(1, &[]);
            let mut seen = vec![harness.game.run_seed()];
            for _ in 0..restarts {
                // The whole restart, not just its first edge: a second `R` on
                // the title screen *starts* rather than restarts, so a single
                // tap per iteration would deal the same run twice.
                harness.restart_run();
                seen.push(harness.game.run_seed());
            }
            seen
        };
        for restarts in 0..4 {
            assert_eq!(
                seeds(restarts),
                seeds(restarts),
                "run {restarts} was not reproducible",
            );
        }
        let four = seeds(3);
        assert_eq!(four.len(), 4);
        for (index, seed) in four.iter().enumerate() {
            assert!(
                !four[..index].contains(seed),
                "restart {index} re-dealt an earlier run's seed",
            );
        }
    }

    // ---- experience, pickups and the level-up --------------------------------

    /// Every field of an [`Intent`] survives the wire form.
    ///
    /// The choice is the one that could silently not: it is two bits at the top
    /// of a `u8` that already carried five flags, and a shift one place out
    /// would take a button with it.
    #[test]
    fn the_wire_form_carries_every_bit_of_intent() {
        let mut seen = Vec::new();
        for choose in 0..=UPGRADE_CHOICES as u8 {
            for flags in 0..32u8 {
                let intent = Intent {
                    up: flags & 1 != 0,
                    down: flags & 2 != 0,
                    left: flags & 4 != 0,
                    right: flags & 8 != 0,
                    restart: flags & 16 != 0,
                    choose,
                };
                let wire = intent.to_wire();
                assert!(
                    !seen.contains(&wire),
                    "{intent:?} shares a wire form with an earlier intent",
                );
                seen.push(wire);
            }
        }
        assert_eq!(seen.len(), 4 * 32, "the loop did not cover what it claims");
    }

    /// **XP drops where an enemy died, is collected on contact, and nothing is
    /// left behind.**
    ///
    /// All three halves, because each fails silently on its own: a gem that
    /// never dropped, a gem that could not be picked up, and a gem picked up
    /// whose collider stayed in the broadphase as an invisible obstacle.
    #[test]
    fn an_enemy_that_dies_drops_a_gem_and_walking_over_it_banks_the_experience() {
        let mut harness = Harness::staged(60, 60, DVec3::ZERO);
        let at = DVec3::new(4.0, 0.0, 0.0);
        harness.game.stage_enemy(EnemyKind::Grunt, at);
        assert_eq!(harness.game.pickup_count(), 0);

        // Shot until it dies. Six bolts' worth of ticks is plenty for a grunt,
        // which takes two.
        harness.run_ticks(harness.ticks + 90, &[]);
        assert_eq!(harness.game.enemy_count(), 0, "the grunt never died");
        assert_eq!(harness.game.kills, 1);
        assert_eq!(harness.game.pickup_count(), 1, "no gem was dropped");
        let gem = harness.game.pickup_positions()[0];
        // Where it *died*, which is not quite where it was staged: a grunt walks
        // towards the player while it is being shot. The bound is one step of
        // its own travel over the ticks it survived, not a shrug.
        assert!(
            (gem - at).length() < 2.0 && gem.x > 1.0,
            "the gem landed at {gem:?}, not on the path the grunt walked from {at:?}",
        );
        assert_eq!(harness.game.xp(), 0, "the gem banked itself");
        harness.assert_nothing_leaked();

        // Walk onto it. The player is at the origin and the gem is four units
        // away; PLAYER_SPEED covers that in well under a second.
        harness.run_ticks(harness.ticks + 120, &[(harness.ticks, KeyCode::KeyD, true)]);
        assert_eq!(
            harness.game.xp(),
            EnemyKind::Grunt.xp(),
            "walking over the gem banked nothing",
        );
        assert_eq!(harness.game.pickup_count(), 0, "the gem was not removed");
        harness.assert_nothing_leaked();
    }

    /// A brute's gem is worth more than a grunt's, which is what makes the
    /// level-up rate track effort rather than bodies.
    #[test]
    fn a_brutes_gem_is_worth_more_than_a_grunts() {
        assert!(EnemyKind::Brute.xp() > EnemyKind::Grunt.xp());
        let mut harness = Harness::staged(60, 60, DVec3::ZERO);
        let entity = harness.game.stage_pickup(DVec3::new(0.2, 0.0, 0.0), 7);
        assert!(harness.game.pickup_positions().len() == 1, "{entity:?}");
        harness.run_ticks(harness.ticks + 2, &[]);
        assert_eq!(harness.game.xp(), 7, "the gem's own value was not banked");
    }

    /// **A gem is a trigger, so a bolt flies through it.**
    ///
    /// The property the whole pickup design rests on: gems are in the same
    /// broadphase the weapon sweeps, and a solid one would eat every shot fired
    /// across a battlefield covered in loot.
    #[test]
    fn a_bolt_flies_through_a_gem_and_kills_what_is_behind_it() {
        let mut harness = Harness::staged(60, 60, DVec3::ZERO);
        let enemy = harness
            .game
            .stage_enemy(EnemyKind::Brute, DVec3::new(6.0, 0.0, 0.0));
        // Directly on the line of fire, and closer than the target.
        harness.game.stage_pickup(DVec3::new(3.0, 0.0, 0.0), 1);
        let before = harness.game.enemy_hp(enemy).expect("a live brute");
        harness.run_ticks(harness.ticks + 60, &[]);
        let after = harness.game.enemy_hp(enemy).expect("still alive");
        assert!(
            after < before,
            "the gem in the way absorbed every bolt: {before} -> {after}",
        );
        assert_eq!(harness.game.pickup_count(), 1, "a bolt destroyed the gem");
    }

    /// **The gun does not aim at the loot.**
    ///
    /// `overlap_sphere` does not skip triggers, so `fire`'s target query has to
    /// reject gems itself — and a gun that locked onto one would stop shooting
    /// the moment the field had anything to pick up.
    #[test]
    fn the_gun_ignores_gems_when_there_is_nothing_to_shoot() {
        let mut harness = Harness::staged(60, 60, DVec3::ZERO);
        harness.game.stage_pickup(DVec3::new(4.0, 3.0, 0.0), 1);
        harness.run_ticks(harness.ticks + 120, &[]);
        assert_eq!(
            harness.game.bolts_fired(),
            0,
            "the gun fired at a gem it cannot hurt",
        );
    }

    /// **A full field stops dropping gems** rather than growing a collider per
    /// kill forever, and it says how many it refused.
    #[test]
    fn a_field_full_of_gems_drops_no_more() {
        let mut harness = Harness::staged(60, 60, DVec3::new(40.0, 30.0, 0.0));
        for i in 0..MAX_PICKUPS {
            harness
                .game
                .stage_pickup(DVec3::new(-40.0 + (i % 60) as f64 * 0.1, -30.0, 0.0), 1);
        }
        assert_eq!(harness.game.pickup_count(), MAX_PICKUPS);
        assert_eq!(harness.game.pickups_dropped(), 0);
        harness.game.stage_pickup(DVec3::ZERO, 1);
        assert_eq!(
            harness.game.pickup_count(),
            MAX_PICKUPS,
            "the cap did not hold"
        );
        assert_eq!(
            harness.game.pickups_dropped(),
            1,
            "the refusal was not counted"
        );
    }

    /// **Banking the threshold opens the level-up screen**, takes the threshold
    /// out of the bank and leaves the remainder.
    #[test]
    fn banking_the_threshold_opens_the_level_up_screen() {
        let mut harness = Harness::staged(60, 60, DVec3::ZERO);
        assert_eq!(harness.game.level, 1);
        assert_eq!(harness.game.offer(), None);

        // One short of the threshold: nothing happens, which is what makes the
        // assertion below about the threshold and not about any XP at all.
        harness.game.bank_xp(xp_for_next_level(1) - 1);
        harness.run_ticks(harness.ticks + 2, &[]);
        assert_eq!(harness.game.state, GameState::Playing);
        assert_eq!(harness.game.level, 1);

        harness.game.bank_xp(3);
        harness.run_ticks(harness.ticks + 1, &[]);
        assert_eq!(harness.game.state, GameState::LevelUp);
        assert_eq!(harness.game.level, 2);
        assert_eq!(
            harness.game.xp(),
            2,
            "the threshold was not taken out of the bank",
        );
        assert!(harness.game.offer().is_some(), "no offer was rolled");
    }

    /// **An offer is exactly three distinct upgrades from the pool**, at every
    /// level of every run — and across enough of them the whole pool appears,
    /// which is what says the shuffle is a shuffle and not a fixed prefix.
    #[test]
    fn an_offer_is_three_distinct_upgrades_and_the_pool_is_used() {
        let mut seen: Vec<Upgrade> = Vec::new();
        for seed in 0..64u64 {
            for level in 1..40u32 {
                let offer = upgrade_offer(seed, level);
                assert_eq!(offer.len(), UPGRADE_CHOICES);
                for (index, upgrade) in offer.iter().enumerate() {
                    assert!(
                        Upgrade::ALL.contains(upgrade),
                        "{upgrade:?} is not in the pool",
                    );
                    assert!(
                        !offer[..index].contains(upgrade),
                        "seed {seed} level {level} offered {upgrade:?} twice: {offer:?}",
                    );
                    if !seen.contains(upgrade) {
                        seen.push(*upgrade);
                    }
                }
            }
        }
        assert_eq!(
            seen.len(),
            Upgrade::ALL.len(),
            "only {seen:?} of the pool is ever offered",
        );
        // Deterministic: the same seed and level deal the same three.
        assert_eq!(upgrade_offer(7, 3), upgrade_offer(7, 3));
        assert_ne!(
            upgrade_offer(7, 3),
            upgrade_offer(8, 3),
            "the offer does not depend on the seed",
        );
    }

    /// The digit keys the level-up screen is driven by, in offer order.
    const CHOICE_KEYS: [KeyCode; UPGRADE_CHOICES] =
        [KeyCode::Digit1, KeyCode::Digit2, KeyCode::Digit3];

    /// A run seed whose **first** level-up offers `upgrade`, and where in the
    /// offer it sits.
    ///
    /// Searched rather than constructed, because `upgrade_offer` is a pure
    /// function of the run seed and the level and there is no way to ask it for
    /// a particular answer. The first level-up takes the run to level 2, so that
    /// is the level to search.
    fn seed_offering(upgrade: Upgrade) -> (u64, usize) {
        (0..4096u64)
            .find_map(|seed| {
                let offer = upgrade_offer(run_seed(seed, 0), 2);
                offer
                    .iter()
                    .position(|found| *found == upgrade)
                    .map(|index| (seed, index))
            })
            .unwrap_or_else(|| panic!("no seed under 4096 offers {upgrade:?} at level 2"))
    }

    /// A staged run that has taken `upgrade` and nothing else, or — for `None` —
    /// the same run with no upgrade at all.
    ///
    /// The two are the same seed and the same board, so anything that differs
    /// between them is the upgrade.
    fn with_upgrade(upgrade: Option<Upgrade>) -> Harness {
        let (seed, index) = upgrade.map_or((DEFAULT_SEED, 0), seed_offering);
        let mut harness = Harness::with_setup(
            60,
            &Setup {
                headless: true,
                seed,
                ..Setup::default()
            },
        );
        harness.game.freeze_spawns();
        harness.game.clear_enemies();
        harness.game.stage_player(DVec3::ZERO);
        let Some(upgrade) = upgrade else {
            return harness;
        };
        harness.game.bank_xp(xp_for_next_level(1));
        harness.run_ticks(harness.ticks + 1, &[]);
        assert_eq!(harness.game.state, GameState::LevelUp, "no screen opened");
        assert_eq!(
            harness.game.offer().expect("an offer")[index],
            upgrade,
            "the search found the wrong seed",
        );
        harness.tap(CHOICE_KEYS[index]);
        assert_eq!(
            harness.game.state,
            GameState::Playing,
            "the screen stayed up"
        );
        harness
    }

    /// **Taking an upgrade changes what the simulation does** — every one of
    /// them, measured as behaviour rather than as a field that moved.
    ///
    /// Each arm names an *observable*: how far the player walked, how many bolts
    /// left the gun, how much damage landed, whether a shot was taken at all,
    /// how much of the bar came back, whether a gem out of reach was collected.
    /// A test that read `Game::stats()` back would pass on an `apply_upgrade`
    /// that wrote the number and on nothing that read it.
    #[test]
    fn every_upgrade_in_the_pool_changes_what_the_simulation_does() {
        for upgrade in Upgrade::ALL {
            let (mut base, mut up) = (with_upgrade(None), with_upgrade(Some(upgrade)));
            match upgrade {
                Upgrade::SwiftBoots => {
                    let walk = |h: &mut Harness| {
                        let from = h.game.player;
                        h.run_ticks(h.ticks + 60, &[(h.ticks, KeyCode::KeyD, true)]);
                        h.game.player.x - from.x
                    };
                    let (slow, fast) = (walk(&mut base), walk(&mut up));
                    assert!(fast > slow + 0.4, "walked {slow} then {fast}");
                }
                Upgrade::RapidFire => {
                    let shots = |h: &mut Harness| {
                        // A dozen brutes, so the gun never runs out of targets:
                        // one dies to six bolts and the count would then be
                        // measuring how long a brute lasts.
                        for i in 0..12 {
                            h.game.stage_enemy(
                                EnemyKind::Brute,
                                DVec3::new(5.0, -6.0 + i as f64, 0.0),
                            );
                        }
                        h.run_ticks(h.ticks + 180, &[]);
                        h.game.bolts_fired()
                    };
                    let (slow, fast) = (shots(&mut base), shots(&mut up));
                    assert!(fast > slow, "fired {slow} then {fast}");
                }
                Upgrade::HeavyBolts => {
                    let hurt = |h: &mut Harness| {
                        let brute = h
                            .game
                            .stage_enemy(EnemyKind::Brute, DVec3::new(5.0, 0.0, 0.0));
                        h.run_ticks(h.ticks + 40, &[]);
                        h.game.enemy_hp(brute).expect("a live brute")
                    };
                    let (light, heavy) = (hurt(&mut base), hurt(&mut up));
                    assert!(heavy < light, "left {light} hp then {heavy}");
                }
                Upgrade::LongBarrel => {
                    let fired = |h: &mut Harness| {
                        // Outside the base reach and inside the extended one, so
                        // this is a shot that could not otherwise be taken. A
                        // brute because it is the slowest thing in the game: the
                        // target walks in, and the window below has to be short
                        // enough that it has not walked into the *base* reach by
                        // the end of it — 1.65 units at 1.9 a second is 52 ticks.
                        h.game.stage_enemy(
                            EnemyKind::Brute,
                            DVec3::new(WEAPON_RANGE + 2.5, 0.0, 0.0),
                        );
                        h.run_ticks(h.ticks + 20, &[]);
                        h.game.bolts_fired()
                    };
                    assert_eq!(fired(&mut base), 0, "the base reach already covers it");
                    assert!(fired(&mut up) > 0, "the longer barrel did not reach");
                }
                Upgrade::Vitality => {
                    // The heal is the observable a `max_hp += 25` alone would
                    // not produce: the run took the upgrade at full health, so
                    // the ceiling moved and the bar has to follow it.
                    assert!(up.game.player_hp > base.game.player_hp);
                    let survive = |h: &mut Harness| {
                        h.game.stage_enemy(EnemyKind::Brute, DVec3::ZERO);
                        h.run_ticks(h.ticks + 120, &[]);
                        h.game.player_hp
                    };
                    let (weak, tough) = (survive(&mut base), survive(&mut up));
                    assert!(tough > weak, "left {weak} hp then {tough}");
                }
                Upgrade::Magnet => {
                    let collect = |h: &mut Harness| {
                        // Out of reach of a bare player — `PLAYER_RADIUS +
                        // XP_RADIUS` is 0.85 — and inside a magnet's.
                        h.game.stage_pickup(DVec3::new(1.1, 0.0, 0.0), 1);
                        h.run_ticks(h.ticks + 10, &[]);
                        h.game.xp()
                    };
                    assert_eq!(collect(&mut base), 0, "it was already in reach");
                    assert_eq!(collect(&mut up), 1, "the magnet did not reach");
                }
            }
        }
    }

    /// **A restart takes every upgrade back off.**
    ///
    /// The plan's non-goals bar meta-progression, and this is what makes that a
    /// property of the code rather than of nobody having written the carry-over.
    #[test]
    fn a_restart_takes_every_upgrade_back_off() {
        let mut harness = with_upgrade(Some(Upgrade::HeavyBolts));
        assert_ne!(harness.game.stats(), Stats::default());
        assert!(harness.game.level > 1);
        harness.tap(KeyCode::KeyR);
        assert_eq!(harness.game.stats(), Stats::default());
        assert_eq!(harness.game.level, 1);
        assert_eq!(harness.game.xp(), 0);
        assert_eq!(harness.game.offer(), None);
    }

    /// **The field does not advance while the level-up screen is up.**
    ///
    /// Every moving thing, and the run clock with them: the enemies stay where
    /// they were, the bolts hang in the air, nothing spawns, nothing takes
    /// damage. Asserted as bit equality rather than as a tolerance, because the
    /// mechanism is a zeroed velocity and `position += 0 * dt` is exact.
    #[test]
    fn the_field_does_not_advance_while_the_level_up_screen_is_up() {
        let mut harness = Harness::staged(60, 60, DVec3::ZERO);
        for i in 0..8 {
            harness
                .game
                .stage_enemy(EnemyKind::Grunt, DVec3::new(6.0 + i as f64, 2.0, 0.0));
        }
        // Run until there is actually a bolt in flight rather than for a
        // number of ticks that happens to leave one: a bolt lives 0.6 s and the
        // gun fires every 0.25, so "some ticks later" lands on an empty sky
        // about as often as not.
        for _ in 0..120 {
            harness.run_ticks(harness.ticks + 1, &[]);
            if harness.game.bolt_count() > 0 {
                break;
            }
        }
        assert!(harness.game.bolt_count() > 0, "no bolt to freeze");

        harness.game.bank_xp(xp_for_next_level(1));
        harness.run_ticks(harness.ticks + 1, &[]);
        assert_eq!(harness.game.state, GameState::LevelUp);
        // One more tick, so the zeroed velocities have been through the
        // integrator once and the field is at rest rather than mid-step.
        harness.run_ticks(harness.ticks + 1, &[]);

        let enemies = harness.game.enemy_positions();
        let bolts = harness.game.bolt_positions();
        let (elapsed, kills, hp) = (
            harness.game.elapsed,
            harness.game.kills,
            harness.game.player_hp,
        );
        let (live, shots) = (harness.game.enemy_count(), harness.game.bolt_count());

        // Held movement keys too: a frozen field that the *player* could still
        // walk across would be half a freeze.
        harness.run_ticks(harness.ticks + 120, &[(harness.ticks, KeyCode::KeyD, true)]);

        assert_eq!(harness.game.state, GameState::LevelUp, "it closed itself");
        assert_eq!(harness.game.enemy_positions(), enemies, "the crowd moved");
        assert_eq!(harness.game.bolt_positions(), bolts, "the bolts moved");
        assert_eq!(harness.game.player, DVec3::ZERO, "the player walked away");
        assert_eq!(harness.game.elapsed, elapsed, "the run clock advanced");
        assert_eq!(harness.game.kills, kills);
        assert_eq!(harness.game.player_hp, hp, "damage was dealt");
        assert_eq!(harness.game.enemy_count(), live, "the spawner ran");
        assert_eq!(harness.game.bolt_count(), shots, "a bolt expired");
        harness.assert_nothing_leaked();
    }

    /// **And it starts again when the screen closes** — the freeze is a pause,
    /// not a stop. The bolts are the half that could not recover on their own:
    /// an enemy is given a fresh velocity every tick and a bolt is not.
    #[test]
    fn taking_an_upgrade_puts_the_field_back_in_motion() {
        let mut harness = Harness::staged(60, 60, DVec3::ZERO);
        for i in 0..4 {
            harness
                .game
                .stage_enemy(EnemyKind::Grunt, DVec3::new(6.0 + i as f64, 2.0, 0.0));
        }
        for _ in 0..120 {
            harness.run_ticks(harness.ticks + 1, &[]);
            if harness.game.bolt_count() > 0 {
                break;
            }
        }
        harness.game.bank_xp(xp_for_next_level(1));
        harness.run_ticks(harness.ticks + 2, &[]);
        assert_eq!(harness.game.state, GameState::LevelUp);
        assert!(harness.game.bolt_count() > 0, "no bolt to thaw");

        let frozen_enemies = harness.game.enemy_positions();
        let frozen_bolts = harness.game.bolt_positions();
        harness.tap(KeyCode::Digit1);
        assert_eq!(harness.game.state, GameState::Playing);
        harness.run_ticks(harness.ticks + 4, &[]);

        assert_ne!(
            harness.game.enemy_positions(),
            frozen_enemies,
            "the crowd never started moving again",
        );
        assert_ne!(
            harness.game.bolt_positions(),
            frozen_bolts,
            "the bolts never got their velocity back",
        );
        assert!(harness.game.elapsed > 0.0);
    }

    /// A choice that names no button is ignored, and the screen stays up.
    #[test]
    fn a_choice_outside_the_offer_takes_nothing() {
        let mut harness = Harness::staged(60, 60, DVec3::ZERO);
        harness.game.bank_xp(xp_for_next_level(1));
        harness.run_ticks(harness.ticks + 1, &[]);
        assert_eq!(harness.game.state, GameState::LevelUp);
        let before = harness.game.stats();
        {
            let mut logic = lock(&harness.game.shared);
            logic.intent.choose = 9;
        }
        harness.run_ticks(harness.ticks + 1, &[]);
        assert_eq!(harness.game.state, GameState::LevelUp, "it closed anyway");
        assert_eq!(harness.game.stats(), before, "something was applied");
    }

    // -----------------------------------------------------------------------
    // Audio
    // -----------------------------------------------------------------------

    /// **All five cues fire, and each one is heard where its event happened.**
    ///
    /// Counted with [`crate::audio::Audio::plays`] rather than `voices()`: a
    /// voice is reaped by the audio thread on a clock nothing here controls, and
    /// this game's cap refuses a voice outright on a busy frame — so a test
    /// written against the live voice count would be a race *and* would report a
    /// cue that happened as one that did not. That is flappy's trap, and it is
    /// worse here.
    #[test]
    fn every_cue_fires_and_carries_the_position_of_what_raised_it() {
        use crate::audio::{SOUND_DEATH, SOUND_KILL, SOUND_LEVEL, SOUND_PICKUP, SOUND_SHOT};

        let mut harness = Harness::staged(60, 60, DVec3::ZERO);
        let plays = |h: &Harness, id| h.game.audio.plays(id);
        for id in [
            SOUND_SHOT,
            SOUND_KILL,
            SOUND_PICKUP,
            SOUND_LEVEL,
            SOUND_DEATH,
        ] {
            assert_eq!(plays(&harness, id), 0, "cue {id} fired before anything did");
        }

        // A grunt in range: the gun fires at it, and it dies.
        let at = DVec3::new(4.0, 0.0, 0.0);
        harness.game.stage_enemy(EnemyKind::Grunt, at);
        harness.run_ticks(harness.ticks + 90, &[]);
        assert!(plays(&harness, SOUND_SHOT) > 0, "the gun was silent");
        // Read here, before the fixture walks the player east: the muzzle
        // asserted at the bottom is the one this facing had when the gun fired.
        let facing = lock(&harness.game.shared).player_facing;
        assert_eq!(plays(&harness, SOUND_KILL), 1, "the kill was silent");
        assert_eq!(plays(&harness, SOUND_PICKUP), 0, "the gem banked itself");

        // …and the gem it left, walked onto.
        harness.run_ticks(harness.ticks + 120, &[(harness.ticks, KeyCode::KeyD, true)]);
        assert_eq!(plays(&harness, SOUND_PICKUP), 1, "the gem was silent");

        // A level, which is the one cue that is about the run rather than about
        // a place.
        harness.game.key_event(KeyCode::KeyD, false);
        harness.game.bank_xp(xp_for_next_level(harness.game.level));
        harness.run_ticks(harness.ticks + 2, &[]);
        assert_eq!(harness.game.state, GameState::LevelUp);
        assert_eq!(plays(&harness, SOUND_LEVEL), 1, "the level was silent");

        // And the end of the run. The player walked east to reach the gem, so
        // the brute goes where the player *is* — a fixture that staged it at the
        // origin would touch nothing and this would report a silent death that
        // never happened.
        harness.tap(KeyCode::Digit1);
        harness.game.set_player_hp(0.000_1);
        let player = harness.game.player;
        harness.game.stage_enemy(EnemyKind::Brute, player);
        harness.run_ticks(harness.ticks + 8, &[]);
        assert_eq!(harness.game.state, GameState::Dead);
        assert_eq!(plays(&harness, SOUND_DEATH), 1, "the death was silent");

        // **Where**, not just whether. Every cue carries a world position, and
        // a `play_at` handed a constant would satisfy every count above.
        let played = harness.game.audio.played().to_vec();
        let position_of = |want: u32| {
            played
                .iter()
                .find(|(id, _, _)| *id == want)
                .map(|(_, x, y)| DVec3::new(*x, *y, 0.0))
                .unwrap_or_else(|| panic!("cue {want} was counted, so it was played"))
        };

        // The shot leaves the **head of the staff**, not the player's centre and
        // not a point along the aim: the wizard has never pressed a horizontal
        // key in this fixture, so it is still facing the way it was drawn, and
        // the first bolt starts at that facing's muzzle whatever direction the
        // grunt is in.
        assert_eq!(
            facing,
            Facing::Right,
            "nothing in this fixture turns the wizard before it fires",
        );
        let shot = position_of(SOUND_SHOT);
        assert!(
            (shot - staff_muzzle(facing)).length() < 1e-9,
            "the shot was heard at {shot:?}, not at the staff head",
        );
        let killed_at = position_of(SOUND_KILL);
        assert!(
            (killed_at - at).length() < 2.0,
            "the kill was heard at {killed_at:?}, not near the grunt at {at:?}",
        );
        // The level is heard on the player, who by now has walked east to the
        // gem — so it is nowhere near either of the two above.
        let level = position_of(SOUND_LEVEL);
        assert!(
            level.x > 3.0 && (level - killed_at).length() > 1.0,
            "the level was heard at {level:?}, not on the player",
        );
        let distinct: std::collections::HashSet<(u64, u64)> = played
            .iter()
            .map(|(_, x, y)| (x.to_bits(), y.to_bits()))
            .collect();
        assert!(
            distinct.len() >= 3,
            "every cue was heard in the same place: {distinct:?}",
        );
    }

    /// The cue queue is drained every tick, so it cannot grow without bound and
    /// a tick's cues never leak into the next one's.
    ///
    /// The failure this guards is the one a queue filled inside the tick and
    /// read outside it invites: a drain that missed a path — a frame that ran
    /// two ticks, a level-up's early return — leaves cues sitting in simulation
    /// state, which at this game's kill rate is an unbounded `Vec` on the hot
    /// path.
    #[test]
    fn the_cue_queue_never_survives_the_tick_that_filled_it() {
        let mut harness = Harness::new(60, 60);
        harness.play_ticks(1_200);
        assert!(harness.game.kills > 0, "the soak killed nothing");
        assert_eq!(
            lock(&harness.game.shared).cues.len(),
            0,
            "cues were left in the simulation",
        );
        // A frame that runs several ticks at once must drain all of them, not
        // the last one's: a frame clock at 10 Hz over a 60 Hz tick runs six.
        let mut slow = Harness::new(10, 60);
        slow.play_ticks(600);
        assert_eq!(lock(&slow.game.shared).cues.len(), 0);
        assert!(
            slow.game.audio.plays(crate::audio::SOUND_SHOT) > 0,
            "a six-tick frame played none of its cues",
        );
    }

    // -----------------------------------------------------------------------
    // The record
    // -----------------------------------------------------------------------

    /// **Both edges bank a record**: dying, and pressing restart on a live run.
    ///
    /// The second is this game's own and the one a copy of asteroids' would
    /// miss — asteroids' game is over when the ship runs out and the score is
    /// frozen, while a horde run can be abandoned at any moment and is still
    /// worth what it lasted.
    ///
    /// The file itself is asserted in `crate::best`'s own suite; this is the
    /// wiring.
    #[test]
    fn the_longest_run_is_banked_by_a_death_and_by_a_restart() {
        let mut harness = Harness::staged(60, 60, DVec3::ZERO);
        assert_eq!(harness.game.best.get(), 0);

        // Three seconds of survival, then a restart. The record is the run that
        // ended, not the one that started.
        //
        // `elapsed as u32` rather than a literal 3: `elapsed` is 180 additions
        // of 1/60 and lands a few ulps under three, which the record truncates
        // to 2 — and the HUD's clock truncates the same way, so the two agree
        // and a literal here would be asserting arithmetic rather than
        // behaviour.
        harness.run_ticks(harness.ticks + 180, &[]);
        let abandoned = harness.game.elapsed;
        assert!(abandoned > 2.9, "the run was only {abandoned}s long");
        harness.restart_run();
        // One tick's worth, not zero: the tick that leaves the title screen
        // counts itself.
        assert!(
            harness.game.elapsed < 0.02,
            "the restart did not reset the clock: {}",
            harness.game.elapsed,
        );
        assert_eq!(
            harness.game.best.get(),
            abandoned as u32,
            "the abandoned run was worth nothing",
        );

        // A shorter run that ends in a death does not beat it…
        harness.game.freeze_spawns();
        harness.game.clear_enemies();
        harness.run_ticks(harness.ticks + 60, &[]);
        harness.game.set_player_hp(0.000_1);
        harness
            .game
            .stage_enemy(EnemyKind::Brute, harness.game.player);
        harness.run_ticks(harness.ticks + 8, &[]);
        assert_eq!(harness.game.state, GameState::Dead);
        assert!(
            harness.game.elapsed < abandoned,
            "the short run was not short"
        );
        assert_eq!(
            harness.game.best.get(),
            abandoned as u32,
            "a shorter run took the record",
        );

        // …and a longer one does.
        harness.restart_run();
        harness.game.freeze_spawns();
        harness.game.clear_enemies();
        harness.run_ticks(harness.ticks + 300, &[]);
        let survived = harness.game.elapsed;
        harness.game.set_player_hp(0.000_1);
        harness
            .game
            .stage_enemy(EnemyKind::Brute, harness.game.player);
        harness.run_ticks(harness.ticks + 8, &[]);
        assert_eq!(harness.game.state, GameState::Dead);
        assert!(survived > abandoned, "the long run was not longer");
        assert_eq!(
            harness.game.best.get(),
            harness.game.elapsed as u32,
            "the longest run did not take the record",
        );
    }

    /// The record reaches the renderer, and it is the **facade's** number: a
    /// replay of the same script must not depend on what an earlier session
    /// survived, so it is read outside the simulation's lock.
    #[test]
    fn the_record_reaches_the_render_state_without_entering_the_simulation() {
        let mut harness = Harness::staged(60, 60, DVec3::ZERO);
        let mut render = RenderState::default();
        harness.game.render_state(&mut render);
        assert_eq!(render.best, 0);

        harness.game.best.update(212.0);
        harness.game.render_state(&mut render);
        assert_eq!(render.best, 212);

        // Nothing the simulation hashes changed: the same seed and script still
        // produce the same field.
        let mut fresh = Harness::staged(60, 60, DVec3::ZERO);
        fresh.run_ticks(120, &[]);
        harness.run_ticks(120, &[]);
        assert_eq!(harness.game.enemy_positions(), fresh.game.enemy_positions());
    }

    // -----------------------------------------------------------------------
    // The scale fixture
    // -----------------------------------------------------------------------

    /// **A prefilled field is the size it was asked for, inside the arena, and
    /// made of the game's own mix of kinds.**
    ///
    /// The fixture every number in `docs/plan/sample/03-horde.md` is taken
    /// through, so a fixture that quietly staged a tenth of what it was asked
    /// for — or piled the whole field onto one wall, which is what a 1.25-unit
    /// grid does at ten thousand — would make every one of those numbers a
    /// measurement of something else.
    #[test]
    fn a_prefilled_field_is_the_size_and_shape_it_was_asked_for() {
        let mut game = Game::with_setup(&Setup {
            headless: true,
            max_enemies: 10_000,
            ..Setup::default()
        })
        .expect("a headless game always starts");
        assert_eq!(game.stage_field(10_000), 10_000);
        assert_eq!(game.enemy_count(), 10_000);

        let positions = game.enemy_positions();
        for position in &positions {
            assert!(
                position.x.abs() <= ARENA_HALF_WIDTH && position.y.abs() <= ARENA_HALF_HEIGHT,
                "{position:?} is outside the arena",
            );
        }
        // Spread, not stacked: a fixture that put them all in one place would
        // pass every count above and measure a crowd that does not exist.
        let distinct: std::collections::HashSet<(i64, i64)> = positions
            .iter()
            .map(|p| ((p.x * 100.0) as i64, (p.y * 100.0) as i64))
            .collect();
        assert_eq!(
            distinct.len(),
            10_000,
            "the grid put two enemies in one spot"
        );

        // …and the view holds a real crowd rather than the whole field, which is
        // the number the render measurement turns on.
        let mut render = RenderState::default();
        game.render_state(&mut render);
        assert_eq!(render.enemies.len(), 10_000);

        // Every kind, from the spawner's own table.
        let mut kinds: Vec<EnemyKind> = render.enemies.iter().map(|e| e.kind).collect();
        kinds.sort_unstable_by_key(|k| format!("{k:?}"));
        kinds.dedup();
        assert_eq!(kinds.len(), 3, "the prefill deals one kind: {kinds:?}");

        // The cap is honoured rather than ignored.
        assert_eq!(game.stage_field(500), 0, "the prefill went past the cap");
    }
}
