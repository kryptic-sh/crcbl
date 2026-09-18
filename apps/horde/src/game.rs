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
//! client-side facade: it resolves input into an `Intent`, puts the intent on
//! the wire, advances the server and the client by exactly one tick period, and
//! reads back what to draw.
//!
//! # The input path is the wire and nothing else
//!
//! `Intent::to_wire` is handed to `Client::set_input`, sealed with the session
//! key, and read back by `Intent::from_wire` inside `HordeModule::tick`, out of
//! the [`ClientInputs`] the server hands every module. The shared cell this game
//! and its module both hold is output-only: the module writes what it simulated
//! and the facade reads it back to draw, and nothing travels the other way. A
//! dropped input frame is a lost tick of intent, which is what playing over a
//! real transport means — `InMemoryTransport` drops nothing, so a single-player
//! session never sees one.
//!
//! The order in [`Game::tick`] is what keeps that free of lag: the client is
//! updated **before** the server, because `Client::update` is the only thing
//! that puts input on the wire and the server drains the wire at the top of its
//! tick. Updating it afterwards would post this tick's intent to the next tick.
//! [`Game::with_setup`] spends one tick on the handshake for the same reason —
//! an unkeyed client drops what it is asked to send, so without it the Space
//! that starts the run on the opening frame would be one the simulation never
//! heard about.

use std::cell::{Cell, RefCell};
use std::collections::HashMap;
use std::sync::atomic::{AtomicU32, Ordering};
use std::sync::{Arc, Mutex, MutexGuard};
use std::time::Duration;

use crcbl::core::input::KeyCode;
use crcbl::ecs::{ClientInputs, Entity, GameModule, World};
use crcbl::input::{ActionDecl, ActionKind, ActionMap, Binding};
use crcbl::jobs::{Inline, Pool, Spawn, default_spawner};
use crcbl::math::DVec3;
use crcbl::net::ProtocolCompatibility;
use crcbl::phys::{ColliderComponent, PhysicsSystem, QueryScratch, RigidBody, Transform};
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

/// What one thing lying on the ground is worth to the player who walks over it.
///
/// **One population, two payloads**, rather than a second list of entities
/// beside the gems. Everything a pickup does is the same for both — it is
/// dropped where something died, it is a trigger collider in the same
/// broadphase, it is refused by the same [`MAX_PICKUPS`] ceiling, it is filtered
/// back out of the same aiming and separation queries, and it is collected by
/// the same overlap in `collect_pickups`. A second `Vec` and a second
/// `HashMap` would be that whole apparatus written twice, and would put a second
/// term into both of the leak test's exact equalities for a difference that is
/// one `match` arm wide.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum PickupKind {
    /// Experience, worth what the kind that dropped it is. See
    /// [`EnemyKind::xp`].
    ///
    /// The amount rides on the variant because it genuinely varies with what
    /// died; a potion's does not, which is why the other arm carries nothing.
    Xp(u64),
    /// Hit points, worth [`POTION_HEAL`].
    Health,
}

impl PickupKind {
    /// Every kind, in a fixed order, for the tests and the art.
    ///
    /// The experience arm carries the smallest payload there is, because this
    /// list exists to enumerate the two *pictures* and nothing reads the value
    /// off it.
    pub const ALL: [Self; 2] = [Self::Xp(0), Self::Health];
}

/// The radius of a dropped pickup's collider, in world units.
///
/// 0.7 units across, which at `art::TEXELS_PER_UNIT` is a whole 14 texels, and
/// a little larger than a runner — loot the player cannot see is loot the
/// player does not walk to. Both [`PickupKind`]s use it, so the two are drawn
/// at one size and the collection query is one radius rather than two.
///
/// **A trigger, not a solid.** `crcbl::phys` skips triggers in
/// [`PhysicsSystem::sweep_sphere`], so a bolt flies through a gem instead of
/// being spent on it; `overlap_sphere` does *not* skip them, which is exactly
/// what `collect_pickups` wants and what the separation and aiming queries have
/// to filter back out. Both filters are the `by_entity` lookups those passes
/// already did.
pub const LOOT_RADIUS: f64 = 0.35;

/// The most pickups of every kind together that may be lying on the field at
/// once.
///
/// A ceiling rather than a lifetime: loot does not rot, so a player who never
/// picks one up would otherwise accumulate one collider per kill forever, and
/// the broadphase this sample exists to measure would be measuring litter. When
/// it is full a kill drops nothing, which is a pressure to go and collect
/// rather than a silent loss — `pickups_dropped` counts what was skipped.
///
/// It is one ceiling over both kinds because they are one list; that is also
/// what keeps it the whole growth bound the soak test's entity ceiling needs.
pub const MAX_PICKUPS: usize = 512;

/// How many hit points a potion puts back.
///
/// A quarter of the bar a run starts with — enough to be worth crossing ground
/// for, nowhere near enough to undo a mistake. Against [`EnemyKind`]'s
/// `contact_dps` it buys a couple of seconds inside the mass and under a second
/// inside a brute, which is the scale the whole genre's pressure is set at;
/// `a_potion_is_worth_seconds_of_contact_and_not_a_run` states both bounds.
///
/// Derived from [`PLAYER_MAX_HP`] rather than spelled out, so the two cannot
/// drift into a potion that is most of a bar or a rounding of it. What it is
/// actually clamped against is [`Stats::max_hp`], which an upgrade moves.
///
/// It happens to equal [`VITALITY_HP`] and is deliberately not defined in terms
/// of it: a quarter of the bar is the natural unit of healing in this game, and
/// tuning the drop is not a reason to re-tune an upgrade.
pub const POTION_HEAL: f64 = PLAYER_MAX_HP / 4.0;

/// The share of dead brutes that leave a potion.
///
/// # Brutes, and only some of them
///
/// [`EnemyKind::Brute`] is the kind that actually takes hit points off a run:
/// its `contact_dps` is more than the other two kinds' put together, and it is
/// the slowest thing on the field, so it is also the one a player can simply
/// walk away from — killing it is a decision rather than something that happens
/// while the player kites. Paying the heal out exactly there means a potion is
/// earned by the fight that cost the hit points, and it follows a rule this game
/// already has: [`EnemyKind::xp`] pays a brute five times a grunt for the same
/// reason.
///
/// [`EnemyKind::from_roll`] deals a brute a tenth of the time, so gating on the
/// kind alone would already make a potion an event. The roll on top of it is
/// what stops a brute being a vending machine — a player who could count on the
/// drop would plan the run around brutes, and a healing item that can be
/// *farmed* is one that deletes contact damage from the game, which is the only
/// pressure the genre has.
///
/// # The number came off a measurement, not off a feel
///
/// It was tried at a third of brutes first, and `a_long_run_leaks_nothing` — the
/// soak, which plays a kiting autopilot and asserts the run *ends* — stopped
/// reaching a death at all. That is the failure this constant has to avoid
/// stated as a test: the healing had grown to most of the damage coming in, and
/// contact damage had stopped being what the run is about. At a twentieth the
/// same soak dies and restarts, and one rate above it — a tenth — survives the
/// whole soak on single-figure hit points, which is the edge this sits under
/// deliberately rather than near.
///
/// `potions_drop_from_brutes_at_the_rate_the_constant_says` is where the rate a
/// player actually sees is read off a seeded run, because it is a property of
/// this constant *and* the spawn table together and neither says it alone.
pub const POTION_DROP_CHANCE: f64 = 1.0 / 20.0;

/// The hand the loot is dealt from, as a salt on the run's seed.
///
/// `"LOOT"` in ASCII, in the spirit of [`PROP_HAND`] and for the same reason:
/// [`spawn_index`] packs an unbounded counter into the whole of a `u64`, so
/// there is no bit pattern left for a second index space in the run's own hand
/// and the two have to be told apart by the seed instead.
const LOOT_HAND: u64 = 0x0000_0000_4C4F_4F54;

/// The index space for one kill's loot draws.
///
/// The same packing [`spawn_index`] and [`prop_index`] use, with the same three
/// bits of headroom. The index is the run's kill counter, which is simulation
/// state like any other: it advances once per death, it is reset by `restart`
/// alongside the run seed, and it is the same on the server, on the client and
/// in a replay.
const fn loot_index(kill: u64, which: u64) -> u64 {
    (kill << 3) | which
}

/// Which draw of a kill's loot is which.
const DRAW_POTION_PRESENCE: u64 = 0;
const DRAW_POTION_BEARING: u64 = 1;

/// Whether the `kill`-th kill of run `seed` leaves a potion, given what died.
///
/// See [`POTION_DROP_CHANCE`] for the design. The draw is salted onto the seed
/// rather than packed into the spawner's index space because `spawn_index`
/// already owns the whole of a `u64`, leaving no bit pattern for a second index
/// space in the run's own hand.
#[must_use]
pub fn drops_potion(seed: u64, kill: u64, kind: EnemyKind) -> bool {
    kind == EnemyKind::Brute
        && hash_unit(
            crcbl::core::rand::salt(seed, LOOT_HAND),
            loot_index(kill, DRAW_POTION_PRESENCE),
        ) < POTION_DROP_CHANCE
}

/// Where the `kill`-th kill's potion lands, relative to the gem beside it.
///
/// One pickup diameter, so the two discs touch and neither hides the other: a
/// brute drops both, and a potion painted exactly over its own gem would leave
/// the gem invisible every time the rare drop happened. The bearing is drawn
/// from the same hand as the drop rather than fixed, so a field of loot does
/// not grow a visible convention.
#[must_use]
fn potion_offset(seed: u64, kill: u64) -> DVec3 {
    let angle = hash_unit(
        crcbl::core::rand::salt(seed, LOOT_HAND),
        loot_index(kill, DRAW_POTION_BEARING),
    ) * std::f64::consts::TAU;
    DVec3::new(angle.cos(), angle.sin(), 0.0) * (2.0 * LOOT_RADIUS)
}

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

/// How much [`Upgrade::Vitality`] raises the ceiling by, and heals on the spot.
///
/// Named rather than written twice into `apply_upgrade`, because the two uses
/// are the same decision: the upgrade is worth a quarter of the starting bar
/// whether the run needs it now or later.
pub const VITALITY_HP: f64 = PLAYER_MAX_HP / 4.0;

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

/// The largest enemy radius, in world units.
///
/// The spawn ring's arc is inset by this much: [`spawn_arc`] draws only the
/// angles whose ring point is at least this far from every wall, so even the
/// biggest body that can be spawned there is fully inside the arena and the
/// clamp in `spawn_enemy` never moves a spawn.
const MAX_ENEMY_RADIUS: f64 = {
    let mut max = 0.0;
    let mut i = 0;
    while i < EnemyKind::ALL.len() {
        let radius = EnemyKind::ALL[i].radius();
        if radius > max {
            max = radius;
        }
        i += 1;
    }
    max
};

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
/// 60 fps and 60 Hz. Both phases the roadmap put that behind have since moved:
/// P7's GPU-driven half landed, and `crcbl-jobs` ships — this file's own
/// `steer_enemies` runs on its pool. What has not happened is the sub-slice
/// that raises this number and measures where it breaks, so the ceiling stays
/// where it was last measured rather than where the plan wants it.
/// `--max-enemies` is here so raising it needs no rebuild.
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
/// Three bits of `which` leaves room for five more draws per spawn before the
/// counter has to move. The counter itself is unbounded, so this space is the
/// whole of `u64` and there is no room in it for a second one — which is why
/// the props are dealt from a **salted** seed rather than from a range of this.
/// See [`PROP_HAND`].
const fn spawn_index(counter: u64, which: u64) -> u64 {
    (counter << 3) | which
}

/// Which draw of a spawn is which.
const DRAW_RING_ANGLE: u64 = 0;
const DRAW_KIND: u64 = 1;
const DRAW_JITTER: u64 = 2;

/// The arc of [`SPAWN_RING`] around `player` that stays fully inside the
/// arena, as `(start, span)` with `start ∈ [0, 2π)` and `span ∈ (0, 2π]`.
///
/// The ring crosses each of the four inset box lines — `x = ±(half width −
/// largest enemy radius)`, `y = ±(half height − largest enemy radius)` — at
/// most twice, and between two consecutive crossings it is wholly inside or
/// wholly outside, so one midpoint test per interval decides them all. The
/// intervals are the segments between the sorted crossing angles, plus the one
/// that wraps past 0. Each axis's constraint permits at most one arc of
/// angles, so the inside intervals join into a single arc; with no crossings
/// at all — the player at the origin — the whole circle is inside and the
/// answer is `(0, 2π)` exactly.
#[must_use]
fn spawn_arc(player: DVec3) -> (f64, f64) {
    let half_x = ARENA_HALF_WIDTH - MAX_ENEMY_RADIUS;
    let half_y = ARENA_HALF_HEIGHT - MAX_ENEMY_RADIUS;
    let tau = std::f64::consts::TAU;

    // Every angle at which the ring sits on one of the four box lines.
    let mut crossings: Vec<f64> = Vec::new();
    // x = ±half_x: the ring crosses where cos θ = (±half_x − player.x) / R.
    for c in [
        (half_x - player.x) / SPAWN_RING,
        (-half_x - player.x) / SPAWN_RING,
    ] {
        if (-1.0..=1.0).contains(&c) {
            let a = c.acos();
            crossings.push(a);
            crossings.push(-a);
        }
    }
    // y = ±half_y: the ring crosses where sin θ = (±half_y − player.y) / R.
    for c in [
        (half_y - player.y) / SPAWN_RING,
        (-half_y - player.y) / SPAWN_RING,
    ] {
        if (-1.0..=1.0).contains(&c) {
            let a = c.asin();
            crossings.push(a);
            crossings.push(std::f64::consts::PI - a);
        }
    }
    for angle in &mut crossings {
        *angle = angle.rem_euclid(tau);
    }
    crossings.sort_by(f64::total_cmp);

    let mut inside: Vec<(f64, f64)> = Vec::new();
    for i in 0..crossings.len() {
        let start = crossings[i];
        let end = if i + 1 < crossings.len() {
            crossings[i + 1]
        } else {
            crossings[0] + tau // the interval that wraps past 0
        };
        if end - start <= 0.0 {
            continue; // degenerate: the ring touches a line rather than crossing it
        }
        let mid = (start + end) * 0.5;
        let point = DVec3::new(
            player.x + SPAWN_RING * mid.cos(),
            player.y + SPAWN_RING * mid.sin(),
            0.0,
        );
        if point.x.abs() <= half_x && point.y.abs() <= half_y {
            inside.push((start, end));
        }
    }

    match inside.as_slice() {
        // No crossings means the whole ring is inside the box — the player at
        // the origin is exactly this, and the answer is the full circle. An
        // out-of-arena player also lands here (defensively, since the draw must
        // stay defined): the full ring is the fallback, at the cost of the
        // inside promise — the alternative to returning something silently.
        [] => (0.0, tau),
        [one] => {
            let span = one.1 - one.0;
            if span > 0.0 && span <= tau {
                (one.0, span)
            } else {
                (0.0, tau)
            }
        }
        _ => {
            // Unreachable in this geometry — each axis allows at most one arc,
            // so their intersection is one — but a near-tangent sliver misjudged
            // by rounding would read as a second, tiny interval. The widest one
            // is the real arc, and it alone keeps the inside-the-arena promise.
            let widest = inside
                .iter()
                .max_by(|a, b| (a.1 - a.0).total_cmp(&(b.1 - b.0)))
                .expect("the arm above guarantees a non-empty slice");
            let span = widest.1 - widest.0;
            if span > 0.0 && span <= tau {
                (widest.0, span)
            } else {
                (0.0, tau)
            }
        }
    }
}

/// Where the `counter`-th enemy of run `seed` enters, relative to the player.
///
/// **On a ring, never in the view.** An enemy that appeared inside the screen
/// would be indistinguishable from a rendering bug, and one that appeared on top
/// of the player would be damage the player had no chance to avoid. A ring is a
/// pure function *and* is provably outside the view, where a rejection loop
/// would be neither.
///
/// The angle is drawn within the arc of the ring that stays inside the arena,
/// inset by the largest enemy radius — see `spawn_arc` — so the clamp in
/// `spawn_enemy` never moves a spawn, however close to a wall the player
/// stands. Still a pure function of its three arguments: `(seed, counter,
/// player)`.
#[must_use]
pub fn spawn_offset(seed: u64, counter: u64, player: DVec3) -> DVec3 {
    let (start, span) = spawn_arc(player);
    let angle = start + hash_unit(seed, spawn_index(counter, DRAW_RING_ANGLE)) * span;
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
// The props
// ---------------------------------------------------------------------------

/// One of the two things scattered over the arena that the player cannot walk
/// through.
///
/// # The player collides with these and nothing else does
///
/// **Enemies walk through them and bolts fly through them**, and that is the
/// decision rather than an unfinished half of one.
/// `docs/plan/sample/03-horde.md`'s hard cap bars pathfinding, and a prop the
/// horde had to route around is pathfinding wearing a tree costume: the seek
/// loop is the hottest path in this game and the sample exists to show that its
/// cost does not move with the size of the field. An obstacle query per enemy
/// per tick would be exactly the term that breaks that, for scenery.
///
/// The same reasoning keeps them out of the ECS and out of the broadphase.
/// A prop is [`PropView`] in a plain `Vec` — no entity, no collider — so
/// `steer_enemies`' `N` overlap queries return what they returned before,
/// `sweep_bolts` sweeps what it swept before, and the leak test's two exact
/// equalities (`entity_count` and `collider_count` in `Harness`) mean what they
/// meant before. There is nothing here for either of them to count.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum PropKind {
    /// The large one. A canopy seen from above.
    Tree,
    /// The small one.
    Bush,
}

impl PropKind {
    /// Every kind, in a fixed order, for the tests and the scatter.
    pub const ALL: [Self; 2] = [Self::Tree, Self::Bush];

    /// The collider radius, in world units.
    ///
    /// The tree is a little larger than [`EnemyKind::Brute`], so the biggest
    /// thing on the field is scenery; the bush is exactly [`PLAYER_RADIUS`],
    /// because "can I get round this" is judged against the shape the player
    /// already knows the size of. `assets/props.crpix` draws each to its own
    /// collider, to the texel, and `art.rs` asserts it.
    #[must_use]
    pub const fn radius(self) -> f64 {
        match self {
            Self::Tree => 0.9,
            Self::Bush => 0.5,
        }
    }
}

/// The largest [`PropKind::radius`] there is.
///
/// A constant rather than a fold over [`PropKind::ALL`], because the scatter's
/// spacing guarantees are `const` assertions and a fold is not a constant
/// expression. `the_largest_prop_radius_is_the_one_the_spacing_was_checked
/// _against` is what stops a third kind leaving it stale.
const PROP_MAX_RADIUS: f64 = PropKind::Tree.radius();

/// One prop: where it stands and what it is.
///
/// **One type, where the enemies and the gems each have two.** `Enemy` carries
/// hit points and an entity that `EnemyView` has no use for; a prop is a
/// position and a kind to the simulation and the same position and kind to the
/// renderer, so a second struct would be a copy of this one with the same two
/// fields in it.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct PropView {
    pub position: DVec3,
    pub kind: PropKind,
}

/// The side of one scatter cell, in world units.
///
/// **The pitch of the whole layout**: at most one prop is dealt per cell, so
/// this is how far apart props can be and how often one can turn up in a
/// straight walk. At [`PLAYER_SPEED`] a cell takes a little over a second to
/// cross, which is the interval a player meets scenery at — often enough to be
/// something you steer around, rare enough that steering is not what the game
/// is about.
///
/// It is also both spacing guarantees. See [`scatter_props`].
pub const PROP_CELL: f64 = 8.0;

/// How far from its cell's centre a prop may be dealt, in world units.
///
/// The lattice is what bounds the layout and the jitter is what stops it
/// *looking* like a lattice — the same split `assets/terrain.crpix` makes for
/// the ground, arrived at the other way round: the grass hides its grid by
/// having no tile-scale feature, and the scatter hides its grid by moving the
/// feature. Half the cell would erase the grid entirely and take both spacing
/// guarantees with it.
pub const PROP_JITTER: f64 = 2.0;

/// The share of cells that hold a prop at all.
///
/// A share rather than a count, so the layout follows the arena instead of
/// having to be re-tuned beside it. The number is bounded on both sides by how
/// a survivors run reads: too many and a 96 × 72 arena is a maze the horde
/// ignores while the player is penned in by scenery the horde walks through;
/// too few and props are a surprise rather than a feature of the ground.
/// A little under half the cells puts a handful in a view of about 37 × 28
/// units and leaves the great majority of the arena open —
/// `the_scatter_is_sparse_and_never_pens_the_player_in` measures both.
const PROP_DENSITY: f64 = 0.45;

/// The share of props that are trees rather than bushes.
///
/// The minority, because the tree is the one that actually blocks a route: at
/// 0.9 units it is wider than any monster, and a field where most of the
/// scenery is that size stops being scenery.
const PROP_TREE_SHARE: f64 = 0.4;

/// How much clear ground the player is dealt around the spawn, in world units.
///
/// **The player must not begin a run inside a tree**, and this is what makes
/// that true by construction rather than by a rejection loop: any prop whose
/// disc reaches inside this radius of the origin — where `place_player` puts
/// the wizard, on a fresh game and on every restart — is simply not dealt.
///
/// Three player-diameters of clear ground on every side, so at [`PLAYER_SPEED`]
/// there is most of a second of travel in any direction before the first prop
/// can be reached. `a_run_never_starts_inside_a_prop` asserts the clearance
/// over a spread of seeds, having first asserted the scatter is not empty.
pub const PROP_SPAWN_CLEARANCE: f64 = 4.0;

/// The hand the props are dealt from, as a salt on the game's seed.
///
/// `"PROPS"` in ASCII, in the spirit of `COMPATIBILITY` — an arbitrary constant
/// that is at least readable in a hex dump. [`crcbl::core::rand::salt`] is the
/// operation for *another seed from this one*, which is what keeps the scatter's
/// index space clear of the spawner's: `spawn_index` packs a counter that is
/// unbounded, so no bit pattern is free for a second index space to use and the
/// two have to be told apart by the seed instead.
const PROP_HAND: u64 = 0x0000_0050_524F_5053;

/// The index space for one scatter cell's draws.
///
/// The same packing [`spawn_index`] uses, in the salted hand [`PROP_HAND`]
/// names, and with the same three bits of headroom.
const fn prop_index(cell: u64, which: u64) -> u64 {
    (cell << 3) | which
}

/// Which draw of a cell is which.
const DRAW_PROP_PRESENCE: u64 = 0;
const DRAW_PROP_X: u64 = 1;
const DRAW_PROP_Y: u64 = 2;
const DRAW_PROP_KIND: u64 = 3;

/// The props of a game seeded with `seed`.
///
/// # It is dealt from the game's seed, not the run's
///
/// The horde is re-dealt by a restart, which advances the run's own seed, and
/// the scenery is not.
/// A prop is a feature of the *place*: the walls do not move when a player
/// presses `R` and neither does the ground, and a player who learns where the
/// cover is should keep that between attempts at the same seed. Two games built
/// from the same [`Setup`] therefore stand in the same arena, on every machine
/// and in a replay, which is what makes a recorded script meet the layout it
/// recorded against.
///
/// # A jittered lattice, so the spacing is a property rather than a hope
///
/// The arena is cut into whole cells of at least [`PROP_CELL`] — `floor`, so a
/// cell is never *smaller* than that — and each cell deals at most one prop,
/// displaced from its own centre by up to [`PROP_JITTER`] on each axis. Two
/// consequences follow from arithmetic rather than from tuning, and both are
/// asserted below at compile time:
///
/// * **No two props can overlap the player at once.** Two props are at least
///   `cell - 2 * jitter` apart, which is wider than two of the largest discs
///   the player can be inside of — so `push_out_of_props` resolving them one at
///   a time in a single pass is exact and not an approximation.
/// * **The arena clamp can never push the player into a prop.** The outermost
///   cell centre sits half a cell from the wall, so a prop is at least
///   `cell / 2 - jitter` from it; a player pressed against that wall has its
///   centre [`PLAYER_RADIUS`] inside, so the clearance the layout needs is the
///   prop's radius plus *two* of the player's.
///
/// A rejection loop over random points would give neither, and would not be a
/// pure function of the seed without a bound nobody could state.
#[must_use]
#[allow(
    clippy::cast_possible_truncation,
    clippy::cast_sign_loss,
    reason = "the lattice is a dozen cells across an arena fixed at compile time"
)]
pub fn scatter_props(seed: u64) -> Vec<PropView> {
    const {
        assert!(
            PROP_CELL - 2.0 * PROP_JITTER >= 2.0 * (PROP_MAX_RADIUS + PLAYER_RADIUS),
            "two props can be close enough to overlap the player at once, so \
             pushing out of one at a time is not exact",
        );
        assert!(
            PROP_CELL / 2.0 - PROP_JITTER >= PROP_MAX_RADIUS + 2.0 * PLAYER_RADIUS,
            "a prop can be dealt close enough to a wall that the arena clamp \
             pushes the player into it",
        );
        assert!(
            PROP_CELL <= 2.0 * ARENA_HALF_HEIGHT && PROP_CELL <= 2.0 * ARENA_HALF_WIDTH,
            "the arena does not hold one whole scatter cell",
        );
    }

    let hand = crcbl::core::rand::salt(seed, PROP_HAND);
    let cols = (2.0 * ARENA_HALF_WIDTH / PROP_CELL).floor() as u32;
    let rows = (2.0 * ARENA_HALF_HEIGHT / PROP_CELL).floor() as u32;
    let step_x = 2.0 * ARENA_HALF_WIDTH / f64::from(cols);
    let step_y = 2.0 * ARENA_HALF_HEIGHT / f64::from(rows);

    let mut props = Vec::new();
    for row in 0..rows {
        for col in 0..cols {
            let cell = u64::from(row) * u64::from(cols) + u64::from(col);
            if hash_unit(hand, prop_index(cell, DRAW_PROP_PRESENCE)) >= PROP_DENSITY {
                continue;
            }
            let kind = if hash_unit(hand, prop_index(cell, DRAW_PROP_KIND)) < PROP_TREE_SHARE {
                PropKind::Tree
            } else {
                PropKind::Bush
            };
            let offset =
                |which| (hash_unit(hand, prop_index(cell, which)) * 2.0 - 1.0) * PROP_JITTER;
            let position = DVec3::new(
                -ARENA_HALF_WIDTH + (f64::from(col) + 0.5) * step_x + offset(DRAW_PROP_X),
                -ARENA_HALF_HEIGHT + (f64::from(row) + 0.5) * step_y + offset(DRAW_PROP_Y),
                0.0,
            );
            // The glade the player starts in. See `PROP_SPAWN_CLEARANCE`.
            if position.length() < PROP_SPAWN_CLEARANCE + kind.radius() {
                continue;
            }
            props.push(PropView { position, kind });
        }
    }
    props
}

/// Brings a body of `radius` out of any prop it is standing in, and leaves it
/// **bit-exact** when it is standing in none.
///
/// # It slides rather than sticking
///
/// The body is moved to the nearest point on the prop's surface — straight out
/// along the line between the two centres — so only the component of the
/// approach that was *into* the disc is taken away and the component along it
/// survives. Walking diagonally into a trunk therefore slides round it, which is
/// exactly what [`clamp_axis`] does at a wall: it takes one axis and leaves the
/// other alone. Stopping the body dead instead would need this to know what
/// velocity brought it here, which is a second thing to keep in step for a worse
/// feel.
///
/// Bit-exactness is load-bearing for the same reason it is on [`clamp_axis`]:
/// `confine_player` writes a transform back only when the position it computed
/// differs from the one it was handed.
///
/// **One pass.** [`scatter_props`] spaces props so that no two can contain one
/// body at once, so resolving them in sequence cannot push out of one and into
/// another and there is no iteration to converge.
#[must_use]
pub fn push_out_of_props(position: DVec3, radius: f64, props: &[PropView]) -> DVec3 {
    let mut out = position;
    for prop in props {
        let clear = prop.kind.radius() + radius;
        let (dx, dy) = (out.x - prop.position.x, out.y - prop.position.y);
        let distance = dx.hypot(dy);
        if distance >= clear {
            continue;
        }
        // Two exactly coincident centres have no direction between them — the
        // same tie `spawn_jitter` exists to break for two coincident enemies.
        // A fixed axis is enough here where a drawn one was needed there: there
        // is one player, so there is no pair to push apart symmetrically.
        let (nx, ny) = if distance > 0.0 {
            (dx / distance, dy / distance)
        } else {
            (1.0, 0.0)
        };
        out = DVec3::new(
            prop.position.x + nx * clear,
            prop.position.y + ny * clear,
            out.z,
        );
    }
    out
}

// ---------------------------------------------------------------------------
// Input
// ---------------------------------------------------------------------------

/// Where the player is walking, as **one** action with a value rather than four
/// with a flag each.
///
/// It was four — `up`, `down`, `left`, `right`, a `Binding::Key` apiece — and
/// four buttons is a shape only a keyboard has. A stick reports a *direction*,
/// and an action that could only be pressed or not had nowhere to put one:
/// binding a stick to four buttons would mean the widget deciding, before the
/// binding layer ever saw it, which of eight directions the thumb meant. So the
/// keyboard's four keys are a [`Binding::Wasd`] composite now — the same
/// normalised vector, from the same action — and the on-screen stick is a
/// [`Binding::Virtual`] beside it. Nothing downstream can tell them apart, which
/// is the whole of `docs/plan/19-input.md`'s claim.
const ACTION_MOVE: &str = "move";

/// The id the on-screen stick reports under — `docs/plan/19-input.md`'s
/// `Virtual("stick_move")`.
///
/// Private on purpose: `crate::app` owns the widget and hands its deflection to
/// [`Game::stick_moved`], so nothing outside this file has to know that the
/// binding layer calls it this.
const STICK_MOVE: &str = "stick_move";

/// How far the stick has to be pushed before it asks for anything, as a
/// fraction of its throw.
///
/// A floating stick centres itself under the finger that landed, so this is not
/// there to swallow a resting offset — there is none. It is there because the
/// wizard walks at one speed: a thumb a few pixels off centre would otherwise
/// commit to a direction at full speed, and the eight-way split below would pick
/// that direction out of noise.
const MOVE_DEAD_ZONE: f32 = 0.25;

/// Where one of the eight directions ends and the next begins: `sin(π/8)`, the
/// component a unit vector has at 22.5° off an axis.
///
/// Applied to the **normalised** direction, so the eight sectors are 45° wide
/// each and a diagonal is no harder to hold than a cardinal.
/// `the_eight_sectors_are_the_angle_they_claim` checks this against `f32::sin`,
/// because a transcribed constant is a transcription until something computes
/// it.
const MOVE_SECTOR: f32 = 0.382_683_43;
/// The one edge that both **starts** a waiting run and **restarts** a live one.
///
/// Two jobs on one action, the way asteroids' `fire` both begins a game and
/// deals a new one from the death screen. `R` and `Space` are both bound to it:
/// `Space` because that is the key the other games' start screens print,
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

/// The four digital directions a stick deflection asks for, as
/// `(up, down, left, right)`.
///
/// **This game walks at one speed**, and it always has: [`Intent::direction`]
/// normalises, so a wizard leaning on the stick and a wizard leaning on `W`
/// cover the same ground. That is what makes an eight-way reduction of the
/// analog value the honest one rather than a loss — the wire form carries four
/// bits, the simulation reads a normalised direction, and an analog magnitude
/// would have nowhere to go on either side.
///
/// Two thresholds, and they measure different things: [`MOVE_DEAD_ZONE`] is
/// about *how far* the thumb has moved and rejects the middle of the pad,
/// [`MOVE_SECTOR`] is about *which way* it points and splits the rest into eight
/// equal sectors. Folding them into one would make a stick pushed gently
/// north-east ask for nothing while the same push due north asked for a walk.
fn eight_way(x: f32, y: f32) -> (bool, bool, bool, bool) {
    let length = x.hypot(y);
    if length < MOVE_DEAD_ZONE {
        return (false, false, false, false);
    }
    let (x, y) = (x / length, y / length);
    (
        y >= MOVE_SECTOR,
        y <= -MOVE_SECTOR,
        x <= -MOVE_SECTOR,
        x >= MOVE_SECTOR,
    )
}

const INTENT_UP: u8 = 1 << 0;
const INTENT_DOWN: u8 = 1 << 1;
const INTENT_LEFT: u8 = 1 << 2;
const INTENT_RIGHT: u8 = 1 << 3;
const INTENT_RESTART: u8 = 1 << 4;
/// Where the choice sits in the flag byte.
const INTENT_CHOOSE_SHIFT: u32 = 5;
/// The bits the choice occupies: two, above the five flags.
const INTENT_CHOOSE: u8 = 0b11 << INTENT_CHOOSE_SHIFT;
/// Every bit the flag byte defines. One set outside this mask is a frame
/// something other than [`Intent::to_wire`] wrote.
const INTENT_FLAGS: u8 =
    INTENT_UP | INTENT_DOWN | INTENT_LEFT | INTENT_RIGHT | INTENT_RESTART | INTENT_CHOOSE;

/// **The choice's field is exactly as wide as the offer**, which is what makes
/// the mask in [`Intent::from_wire`] the whole of that field's validation: the
/// largest value two bits can carry is the last button, so no byte a peer can
/// send decodes to a choice outside the offer and there is no range check to
/// forget. A fourth upgrade on the level-up screen would fail this and want a
/// third bit.
const _: () = assert!(UPGRADE_CHOICES as u8 == INTENT_CHOOSE >> INTENT_CHOOSE_SHIFT);

impl Intent {
    /// The direction these keys ask for, normalised so a diagonal is not faster
    /// than a straight line.
    fn direction(self) -> DVec3 {
        let x = f64::from(i8::from(self.right) - i8::from(self.left));
        let y = f64::from(i8::from(self.up) - i8::from(self.down));
        DVec3::new(x, y, 0.0).normalize_or_zero()
    }

    /// The wire form handed to `Client::set_input`: one byte of flags with the
    /// choice in its top two bits.
    fn to_wire(self) -> u8 {
        debug_assert!(
            self.choose <= UPGRADE_CHOICES as u8,
            "a choice outside the offer has no wire form",
        );
        let mut flags = 0;
        if self.up {
            flags |= INTENT_UP;
        }
        if self.down {
            flags |= INTENT_DOWN;
        }
        if self.left {
            flags |= INTENT_LEFT;
        }
        if self.right {
            flags |= INTENT_RIGHT;
        }
        if self.restart {
            flags |= INTENT_RESTART;
        }
        flags | ((self.choose << INTENT_CHOOSE_SHIFT) & INTENT_CHOOSE)
    }

    /// The intent a client sealed, read back on the server's side of the wire.
    ///
    /// `None` for anything this build did not write: a payload that is not one
    /// byte, or a bit outside [`INTENT_FLAGS`]. **Validated rather than
    /// trusted**, because these are the only bytes in this game a peer chooses.
    /// The choice is the field that would bite if it were not — it indexes the
    /// offer — and the mask is what holds it: see the assertion above
    /// [`INTENT_CHOOSE`], which is why nothing here has a range check that a
    /// wider field would silently outgrow.
    fn from_wire(bytes: &[u8]) -> Option<Self> {
        let &[flags] = bytes else {
            return None;
        };
        if flags & !INTENT_FLAGS != 0 {
            return None;
        }
        Some(Self {
            up: flags & INTENT_UP != 0,
            down: flags & INTENT_DOWN != 0,
            left: flags & INTENT_LEFT != 0,
            right: flags & INTENT_RIGHT != 0,
            restart: flags & INTENT_RESTART != 0,
            choose: (flags & INTENT_CHOOSE) >> INTENT_CHOOSE_SHIFT,
        })
    }

    /// Folds one server tick's worth of input frames into the intent to run it
    /// with.
    ///
    /// Normally one frame arrives per tick and this is a decode. Several is a
    /// client whose clock ran ahead of the server's, and then each field takes
    /// the rule its own meaning asks for:
    ///
    /// * the four directions are **held**, so the latest frame wins: a thumb is
    ///   wherever it is now, and a key released in the second frame was released
    ///   however hard the first one was holding it.
    /// * `restart` is an **edge**, so it is OR-ed: a key the player pressed in
    ///   any of these frames is one they pressed, and a later frame that says
    ///   nothing about it is not a release.
    /// * `choose` is an edge that also carries *which* button, so the **first
    ///   frame that names one wins** and the rest are ignored — the same rule
    ///   [`Game::tick`] applies to two digits pressed in one frame. Taking the
    ///   latest instead would let a stray second press overwrite the upgrade the
    ///   player actually chose, and there is no way back to a level-up screen.
    ///
    /// The [`TickId`](crcbl::core::TickId) each frame carries is deliberately
    /// unread. Lining an input up with the tick it names is a jitter buffer,
    /// and the server has none — see [`ClientInputs`].
    fn from_inputs(inputs: ClientInputs<'_>) -> Self {
        let mut merged = Self::default();
        for (_tick, data) in inputs.iter() {
            // A frame this build cannot read is skipped rather than taken as an
            // empty intent, which would read as the player letting go.
            let Some(frame) = Self::from_wire(data) else {
                continue;
            };
            merged.up = frame.up;
            merged.down = frame.down;
            merged.left = frame.left;
            merged.right = frame.right;
            merged.restart |= frame.restart;
            if merged.choose == 0 {
                merged.choose = frame.choose;
            }
        }
        merged
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
/// been looked at is worse than a blank arena, and every game opening the same
/// way is worth more than one clever exception.
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

/// One dropped thing on the ground.
#[derive(Clone, Copy, Debug)]
struct Pickup {
    entity: Entity,
    position: DVec3,
    /// What walking over it does. See [`PickupKind`].
    kind: PickupKind,
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

/// What the renderer needs for one pickup.
///
/// The kind is here because it chooses the frame — `art::pickup_frame` is the
/// one place a payload becomes a picture. The experience payload rides along
/// unread; splitting it out would be a second enum whose only job is to be the
/// first one with its value removed.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct PickupView {
    pub position: DVec3,
    pub kind: PickupKind,
}

/// The mutable game state the server-side module owns.
///
/// **Output-only, as far as [`Game`] is concerned.** The facade reads the
/// results out of here after each server tick; what the player asked for goes
/// the other way over the wire, not through this cell. `HordeModule` is the
/// only thing that mutates any of it, and it only ever does so from inside a
/// server tick.
#[derive(Debug)]
struct GameLogic {
    player: Entity,
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

    /// Everything lying on the ground, of either [`PickupKind`], and where each
    /// is in the list — the same pair [`Self::enemies`] and [`Self::by_entity`]
    /// are, for the same reason: both the collection query and the separation
    /// query hand back entity ids.
    pickups: Vec<Pickup>,
    pickup_by_entity: HashMap<Entity, usize>,

    /// The trees and bushes the player cannot walk through.
    ///
    /// **Dealt once, from the game's seed, and never touched again** — see
    /// [`scatter_props`] for why a restart does not re-deal them and
    /// [`PropKind`] for why they are a plain `Vec` rather than entities with
    /// colliders. `clamp_bodies` reads it and nothing writes it.
    props: Vec<PropView>,

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
    /// How many pickups a full field refused to drop. See [`MAX_PICKUPS`].
    pickups_dropped: u64,
    /// How many potions this game's kills have actually left on the ground.
    ///
    /// Instrumentation like the two above, and the only way the drop *rate* can
    /// be read off a run at all: a potion is picked up and gone, so a count of
    /// what is lying about says nothing, and
    /// `potions_drop_from_brutes_at_the_rate_the_constant_says` is what turns
    /// this into the measured figure [`POTION_DROP_CHANCE`] is justified
    /// against. Never reset — it is a count of the whole game, like
    /// [`Self::enemies_spawned`], so a restart does not hide the churn.
    potions_dropped: u64,

    /// Live views for the renderer, refilled rather than rebuilt so a
    /// steady-state tick does not allocate.
    enemy_views: Vec<EnemyView>,
    bolt_views: Vec<BoltView>,
    pickup_views: Vec<PickupView>,

    /// Scratch space for the per-tick passes, kept here for the same reason.
    scratch_entities: Vec<Entity>,

    /// The velocity [`steer_enemies`] decided for each enemy, parallel to
    /// [`Self::enemies`].
    ///
    /// It is a field rather than a local because the pass runs every tick and
    /// this is one `Vec` per tick otherwise; it carries nothing between ticks
    /// and `steer_enemies` refills it whole before reading a single slot.
    steer_velocities: Vec<DVec3>,

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
    /// The pool [`steer_enemies`] splits the crowd across.
    ///
    /// **It lives on the module, not in [`GameLogic`]**, for the reason
    /// `crcbl_jobs::Pool`'s own docs give: `par_for` takes `&mut self`, so one
    /// thread drives a pool at a time, and the module is the thing that is
    /// already single-threaded per tick. The shared state behind the mutex is
    /// read by the render path on another thread, and a pool in there would be
    /// a pool two threads could reach.
    pool: Pool,
}

impl std::fmt::Debug for HordeModule {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("HordeModule")
            .field("workers", &self.pool.workers())
            .finish_non_exhaustive()
    }
}

impl GameModule for HordeModule {
    fn name(&self) -> &str {
        "horde"
    }

    fn register(&self, _world: &mut World) {}

    fn tick(&mut self, world: &mut World, inputs: ClientInputs<'_>) {
        let mut logic = lock(&self.shared);
        run_tick(
            &mut logic,
            world,
            Intent::from_inputs(inputs),
            &mut self.pool,
        );
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
///
/// `intent` is what the client sent for this tick, decoded from the bytes that
/// arrived over the transport — see [`Intent::from_inputs`]. A tick nothing
/// arrived for runs on [`Intent::default`], which is a player holding nothing.
fn run_tick(logic: &mut GameLogic, world: &mut World, intent: Intent, pool: &mut Pool) {
    logic.ticks += 1;
    let dt = world.tick_dt();

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

    // The sweep is gated on the state, because a bolt is a kill carrier and the
    // death screen freezes the kill count (`GameState::Dead` says so). A bolt
    // still in flight when the player dies would otherwise keep sweeping behind
    // the panel, killing enemies, playing the kill sound and dropping gems for
    // up to `BOLT_LIFE`. The bolts themselves still age and expire below, which
    // is what keeps the field honest after the sweep stops.
    if logic.state == GameState::Playing {
        sweep_bolts(logic, world, dt);
    }
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
    steer_enemies(logic, world, pool);

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
///
/// **The props are the player's alone**, so the enemies go through the plain
/// clamp and the player goes through [`confine_player`]. [`PropKind`] carries
/// the reasoning; the shape of it here is that the horde's per-body work is
/// unchanged by scenery existing.
fn clamp_bodies(logic: &mut GameLogic, world: &mut World) {
    let player = logic.player;
    let mut enemies = std::mem::take(&mut logic.enemies);
    let props = std::mem::take(&mut logic.props);
    with_physics(world, |phys| {
        confine_player(phys, player, &props);
        for enemy in &mut enemies {
            if let Some(position) = clamp_one(phys, enemy.entity, enemy.kind.radius()) {
                enemy.position = position;
            }
        }
    });
    logic.enemies = enemies;
    logic.props = props;
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

/// The same for the player, which is also pushed out of the props.
///
/// **Props first, then the wall**, and the order is the one that terminates.
/// The other way round, a player pushed out of a prop could finish outside the
/// arena and spend the next tick being clamped back into it — a body oscillating
/// between two resolutions that each undo the other. This way the wall has the
/// last word, and [`scatter_props`] guarantees the wall never hands the player
/// back to a prop: no prop is dealt close enough to one for a clamped player to
/// be inside it.
fn confine_player(phys: &mut PhysicsSystem, entity: Entity, props: &[PropView]) -> Option<DVec3> {
    let transform = phys.transform(entity).copied()?;
    let confined = clamp_to_arena(
        push_out_of_props(transform.position, PLAYER_RADIUS, props),
        PLAYER_RADIUS,
    );
    if confined == transform.position {
        return None;
    }
    phys.set_transform(entity, Transform::new(confined, transform.rotation));
    Some(confined)
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
            .filter(|entity| by_entity.contains_key(entity))
            .filter_map(|entity| {
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
            .filter_map(|entity| by_entity.get(&entity).copied())
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
/// **This is the "never miss at any speed" half of the plan.**
/// [`PhysicsSystem::sweep_body`] reads the bolt's own body and reconstructs the
/// segment — from where it was (`position − velocity·dt`) to where it is
/// (`position`) — so the "from where it was to where it is" that this function
/// used to write by hand is gone, and the one copy asteroids also calls is the
/// only one.
///
/// A bolt therefore has **no collider**. It is a query, not a body in the
/// broadphase: nothing here ever asks what a bolt is touching, so a collider
/// would be an insert and a remove per bolt per tick buying nothing, and bolts
/// would start stopping each other.
fn sweep_bolts(logic: &mut GameLogic, world: &mut World, dt: f64) {
    if logic.bolts.is_empty() {
        return;
    }

    // `(bolt index, enemy entity)` for everything that connected this tick.
    let mut hits: Vec<(usize, Entity)> = Vec::new();
    let bolts = logic.bolts.clone();
    with_physics(world, |phys| {
        for (index, bolt) in bolts.iter().enumerate() {
            if let Some((entity, _hit)) = phys.sweep_body(bolt.entity, dt, BOLT_RADIUS) {
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
    // Read before it is advanced, so the kill that is being resolved is the one
    // the loot is dealt for: `drops_potion` indexes on the kill's own number,
    // and taking the counter after the increment would deal every kill the next
    // one's hand.
    let kill = logic.kills;
    logic.kills += 1;
    logic.cues.push((crate::audio::SOUND_KILL, dead.position));
    drop_pickup(logic, world, dead.position, PickupKind::Xp(dead.kind.xp()));
    // **Beside the experience, not instead of it.** A brute's gem is what pays
    // for the bolts it took; the potion is what pays for the hit points it cost,
    // and the two are separate rewards for separate things.
    let seed = logic.run();
    if drops_potion(seed, kill, dead.kind) {
        let at = dead.position + potion_offset(seed, kill);
        drop_pickup(logic, world, at, PickupKind::Health);
    }
}

// ---------------------------------------------------------------------------
// Experience and health
// ---------------------------------------------------------------------------

/// Leaves one pickup where something died, if the field has room for it.
///
/// The collider is a **trigger**, which is the whole of how loot stays out of
/// the game's other three queries: `crcbl::phys` skips triggers in the sweep, so
/// a bolt flies through it; `fire` and `steer_enemies` filter theirs back out
/// through the enemy index they already consult. See [`LOOT_RADIUS`].
///
/// The [`MAX_PICKUPS`] refusal is over the whole list rather than per kind, so a
/// field littered with gems can refuse a potion. That is the honest behaviour —
/// the ceiling exists to bound the broadphase, and a kind that could jump the
/// queue would be an unbounded population wearing a bound.
fn drop_pickup(logic: &mut GameLogic, world: &mut World, position: DVec3, kind: PickupKind) {
    if logic.pickups.len() >= MAX_PICKUPS {
        logic.pickups_dropped += 1;
        return;
    }
    let position = clamp_to_arena(position, LOOT_RADIUS);
    let entity = world.spawn();
    let transform = Transform::from_position(position);
    with_physics(world, |phys| {
        phys.set_collider(
            entity,
            &ColliderComponent::Sphere {
                offset: DVec3::ZERO,
                radius: LOOT_RADIUS,
                is_trigger: true,
            },
            &transform,
        );
    });
    if kind == PickupKind::Health {
        logic.potions_dropped += 1;
    }
    push_pickup(
        logic,
        Pickup {
            entity,
            position,
            kind,
        },
    );
}

/// Puts `amount` hit points back, up to the ceiling the run has reached.
///
/// **[`Stats::max_hp`], never [`PLAYER_MAX_HP`].** [`Upgrade::Vitality`] raises
/// that ceiling, so a heal clamped to the constant would silently stop paying
/// out the moment a run took the upgrade — a bug no screenshot shows, because
/// the bar is drawn against the same moving maximum.
///
/// One function for both the potion and the upgrade, because both are the same
/// piece of knowledge: what "full" means to this run.
/// `a_potion_never_heals_past_the_ceiling_the_run_has_reached` is what holds it.
fn heal_player(logic: &mut GameLogic, amount: f64) {
    logic.player_hp = (logic.player_hp + amount).min(logic.stats.max_hp);
}

/// Takes every pickup the player is standing on.
///
/// **One query, and the radius is exact for the same reason contact damage's
/// is**: a shape-aware overlap of radius `R` returns every collider whose centre
/// is within `R + r_b`, so querying at `stats.pickup_radius` picks up exactly
/// the loot whose surface the player is touching. [`Upgrade::Magnet`] raises
/// that radius and nothing else changes.
///
/// The query also returns enemies — they are colliders too — and the
/// `pickup_by_entity` lookup is what rejects them.
///
/// **The kind decides only what is paid out**, and it is the last thing that
/// happens: a potion and a gem are found, removed and destroyed by identical
/// code, so neither can be the one that leaves a collider behind.
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
        for entity in phys.overlap_sphere(centre, radius) {
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
        let taken = remove_pickup(logic, index);
        with_physics(world, |phys| phys.remove_entity(taken.entity));
        world.despawn(taken.entity);
        let cue = match taken.kind {
            PickupKind::Xp(xp) => {
                logic.xp += xp;
                crate::audio::SOUND_PICKUP
            }
            PickupKind::Health => {
                heal_player(logic, POTION_HEAL);
                crate::audio::SOUND_HEAL
            }
        };
        logic.cues.push((cue, taken.position));
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
            stats.max_hp += VITALITY_HP;
            // Healed on the spot as well, or the upgrade is a promise that only
            // pays off after the next `VITALITY_HP` points of damage. Through
            // the same clamp a potion goes through, and against the ceiling this
            // line has just raised.
            heal_player(logic, VITALITY_HP);
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
///
/// # And that is why it is the pass that runs on the job pool
///
/// Order-independence is exactly the precondition [`Pool::par_for`] asks for,
/// so this is the sample's adoption of the job system. The shape is three
/// steps, and the middle one is the only parallel one:
///
/// 1. **Cache the positions**, serially, because it reads the transform map.
/// 2. **Decide every velocity**, in chunks of [`STEER_CHUNK`], through
///    [`PhysicsSystem::overlap_queries`]. Chunk `i` writes
///    `steer_velocities[i * STEER_CHUNK ..]` and reads nothing any other chunk
///    writes, so the value in a slot is a pure function of that enemy, the
///    crowd, and a broadphase nothing can touch while the view is alive.
/// 3. **Write them back**, serially, because `body_mut` is `&mut`.
///
/// **The results are bit-identical to the serial version's, at any worker
/// count**, which is the property the whole adoption stands on. Two things make
/// it true rather than likely: the chunk boundaries come from
/// [`STEER_CHUNK`] and the crowd's length and never from the worker count, so
/// every mode calls the closure with exactly the same `(start, slice)` pairs;
/// and each slot's arithmetic — including the neighbour sum, whose `f64` order
/// is the tree's and not the scheduler's — is untouched by which thread ran it.
/// `steering_is_bit_identical_however_many_workers_run_it` and
/// `the_same_script_replays_bit_identically_on_a_threaded_pool` are what hold
/// both halves, and the second one is the design's `--threads 1` versus
/// `--threads N` gate that `--workers` exists to drive.
///
/// A pool with no workers runs every chunk on the calling thread in ascending
/// order, which is the serial loop this replaced with one extra `Vec` write per
/// enemy. That is what a browser gets from a page which cannot start workers —
/// every published demo — and what `--workers 0` asks for on a machine that
/// can. A page that *can* start them announces itself and this pass splits
/// there too; `crate::web`'s `__crcbl_horde_sim_threads` is how a browser says
/// which happened. There is no `cfg` here and there is none in `crate::web`
/// either; `crcbl_jobs::default_spawner` answers it.
fn steer_enemies(logic: &mut GameLogic, world: &mut World, pool: &mut Pool) {
    // Before the early return, so the count describes the pool this game built
    // rather than only the ticks that had a crowd to split.
    STEER_WORKERS.store(
        u32::try_from(pool.workers()).unwrap_or(u32::MAX),
        Ordering::Relaxed,
    );
    if logic.enemies.is_empty() {
        return;
    }
    let mut enemies = std::mem::take(&mut logic.enemies);
    let by_entity = std::mem::take(&mut logic.by_entity);
    let mut velocities = std::mem::take(&mut logic.steer_velocities);
    let player = logic.player_pos;

    with_physics(world, |phys| {
        // One read of the authoritative positions, so the pass below never has
        // to go back for a neighbour's.
        for enemy in &mut enemies {
            if let Some(transform) = phys.transform(enemy.entity) {
                enemy.position = transform.position;
            }
        }

        // Sized to the crowd before anything is queued, so every chunk has its
        // own slots and the closure never has to grow the buffer.
        velocities.clear();
        velocities.resize(enemies.len(), DVec3::ZERO);

        {
            // The exclusive borrow is spent once, here, and buys a view several
            // threads can query at the same time. It is dropped at the end of
            // this block, which is what lets `body_mut` below take `phys` back.
            let queries = phys.overlap_queries();
            let crowd = &enemies;
            let by_entity = &by_entity;
            pool.par_for(&mut velocities, STEER_CHUNK, |start, out| {
                count_steer_thread();
                STEER_SCRATCH.with_borrow_mut(|(scratch, neighbours)| {
                    for (offset, velocity) in out.iter_mut().enumerate() {
                        let me = &crowd[start + offset];
                        let mut push = DVec3::ZERO;
                        queries.overlap_sphere_into(
                            me.position,
                            separation_query_radius(me.kind),
                            scratch,
                            neighbours,
                        );
                        for &other in neighbours.iter() {
                            if other == me.entity {
                                continue;
                            }
                            let Some(them) =
                                by_entity.get(&other).and_then(|index| crowd.get(*index))
                            else {
                                continue;
                            };
                            push += separation_push(me, them);
                        }

                        let seek = (player - me.position).normalize_or_zero() * me.kind.speed();
                        *velocity = seek + clamp_length(push, 1.0) * SEPARATION_STRENGTH;
                    }
                });
            });
        }

        for (enemy, velocity) in enemies.iter().zip(velocities.iter()) {
            if let Some(body) = phys.body_mut(enemy.entity) {
                body.velocity = *velocity;
            }
        }
    });

    logic.enemies = enemies;
    logic.by_entity = by_entity;
    logic.steer_velocities = velocities;
}

/// How many enemies one [`steer_enemies`] chunk decides a velocity for.
///
/// **A constant, not a function of the worker count**, which is what keeps the
/// closure calls identical between a pool with workers and one without — see
/// [`Pool::par_for`], whose docs are explicit that the boundaries are the
/// caller's. Sizing it to the pool would make the split a function of the
/// machine, and the `--workers` determinism gate compares runs on the same
/// machine at different worker counts.
///
/// Sixty-four enemies is a few microseconds of BVH descents, which is far more
/// than the per-chunk overhead and small enough that
/// [`DEFAULT_MAX_ENEMIES`] still splits into more chunks than any desktop has
/// cores. The pool's queue holds 1024 chunks, so this stays under it up to
/// 65 536 enemies and degrades to running the excess on the driving thread
/// above that rather than dropping it.
const STEER_CHUNK: usize = 64;

thread_local! {
    /// The buffers one thread's [`steer_enemies`] chunks query into.
    ///
    /// **Thread-local rather than per-chunk**, because a `par_for` closure is
    /// `Fn` and cannot own mutable state: allocating inside it would be three
    /// `Vec`s per chunk per tick, and the pass's whole documented claim above is
    /// that it allocates nothing in the steady state. One set per worker,
    /// reused for the life of the thread, keeps that true.
    ///
    /// It cannot affect an answer: both halves are cleared by the query before
    /// it writes them, so what a chunk finds in here is whatever the last chunk
    /// on this thread left, and nothing reads it.
    static STEER_SCRATCH: RefCell<(QueryScratch, Vec<Entity>)> =
        RefCell::new((QueryScratch::new(), Vec::new()));

    /// Whether this thread has already been added to [`STEER_THREADS`].
    ///
    /// One flag per thread rather than a set of thread ids, because the only
    /// question asked of it is "have I been counted", and a `HashSet` behind a
    /// lock would be an allocation and a contended lock inside the pass whose
    /// documented claim is that it allocates nothing.
    static STEER_COUNTED: Cell<bool> = const { Cell::new(false) };
}

/// Distinct threads that have run a [`steer_enemies`] chunk since the process
/// started.
///
/// **The observable behind `__crcbl_horde_sim_threads`**, and the only thing in
/// this sample that can tell a browser run driving worker threads from one
/// running every chunk inline — the two produce identical frames by
/// construction, which is [`steer_enemies`]'s determinism claim rather than an
/// accident. It is the same evidence
/// `steering_is_bit_identical_however_many_workers_run_it` takes from its probe
/// pass: more than one thread ran a chunk.
///
/// It only ever grows, and it is never reset: a pool that is dropped and rebuilt
/// contributes its threads to the total, which is what a "has this ever run off
/// the calling thread" question wants.
///
/// **A thread that cannot read its own thread-local is missed rather than
/// double-counted.** [`STEER_COUNTED`] is what stops a thread being added
/// twice, so a build whose TLS was set up wrong — every thread aliasing one
/// block — reports *one* thread however many ran. That is the safe direction:
/// this number can be too small and never too large, so nothing built on it
/// passes because the evidence broke.
static STEER_THREADS: AtomicU32 = AtomicU32::new(0);

/// How many workers the pool that last ran [`steer_enemies`] has.
///
/// The observable behind `__crcbl_horde_sim_workers`, and the other half of the
/// browser's question: zero says the run degraded onto the inline path, which is
/// what every published demo gets, and a non-zero count with
/// [`STEER_THREADS`] still at one says the workers exist and took nothing.
static STEER_WORKERS: AtomicU32 = AtomicU32::new(0);

/// Counts the calling thread in [`STEER_THREADS`], once.
///
/// Called at the top of every [`steer_enemies`] chunk: one thread-local read and
/// a branch, and an atomic increment on each thread's first chunk only.
fn count_steer_thread() {
    STEER_COUNTED.with(|counted| {
        if !counted.replace(true) {
            STEER_THREADS.fetch_add(1, Ordering::Relaxed);
        }
    });
}

/// How many distinct threads have run a steering chunk.
///
/// The counter `STEER_THREADS` holds, which is where the reasoning is. Public
/// because `crate::web` exports it as `__crcbl_horde_sim_threads`, which is the
/// only way a browser can be asked the question at all.
#[must_use]
pub fn steer_threads() -> u32 {
    STEER_THREADS.load(Ordering::Relaxed)
}

/// How many workers the pool that last ran the steering pass has.
///
/// The counter `STEER_WORKERS` holds. Public for [`steer_threads`]'s reason:
/// `crate::web` exports it as `__crcbl_horde_sim_workers`.
#[must_use]
pub fn steer_workers() -> u32 {
    STEER_WORKERS.load(Ordering::Relaxed)
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
            logic.player_pos + spawn_offset(seed, counter, logic.player_pos),
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
    for pickup in std::mem::take(&mut logic.pickups) {
        with_physics(world, |phys| phys.remove_entity(pickup.entity));
        world.despawn(pickup.entity);
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

    // Straight off `Pickup::position`, which never changes: loot is dropped
    // where an enemy died and stays there until it is walked over.
    let mut pickup_views = std::mem::take(&mut logic.pickup_views);
    pickup_views.clear();
    pickup_views.extend(logic.pickups.iter().map(|pickup| PickupView {
        position: pickup.position,
        kind: pickup.kind,
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
    /// The trees and bushes, which never move and never change.
    ///
    /// Copied per frame like everything else here rather than handed over once:
    /// the list is tens of items where the crowd is thousands, and a renderer
    /// that held its own copy would be a second place the layout lives.
    pub props: Vec<PropView>,
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
    /// Whether this run opens no audio output and keeps its own high score.
    ///
    /// **Defaults to the isolated side**, because a `Setup` that took the
    /// default is a test's: the other side hands `Audio::new` the machine's
    /// real output device and `Best::load` the player's saved record, and a
    /// test may do neither to the person running it. The game itself never
    /// reads this default — `Options::setup` names the flag's value — so the
    /// default costs a real run nothing and costs a forgetful test nothing
    /// either.
    pub headless: bool,
    pub tick_hz: u32,
    pub seed: u64,
    pub max_enemies: usize,
    /// How many worker threads the steering pass's pool gets, or `None` for as
    /// many as the machine has to spare.
    ///
    /// **`Some(0)` is the browser's answer and the gate's**: a pool with no
    /// workers runs every chunk on the calling thread, which is what
    /// `wasm32` gets from `crcbl_jobs::default_spawner` whether this asks for
    /// workers or not. Setting it here is how a native run reproduces that,
    /// which is the whole of the design's `--threads 1` versus `--threads N`
    /// determinism comparison.
    pub workers: Option<usize>,
}

impl Default for Setup {
    fn default() -> Self {
        Self {
            headless: true,
            tick_hz: DEFAULT_TICK_HZ,
            seed: DEFAULT_SEED,
            max_enemies: DEFAULT_MAX_ENEMIES,
            workers: None,
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
    /// Ticks this `Game` has actually run, which is **not** the loop's tick
    /// count: the loop counts the times it called [`Game::tick`], and a build
    /// whose call does nothing still raises that. This only moves when the
    /// simulation did, which is what lets a caller outside the binary tell a
    /// frozen run from a live one — see `tests/golden.rs`.
    pub ticks_run: u64,
    /// Queued key events from the shell pump, replayed after `begin_tick`.
    pending_keys: Vec<(KeyCode, bool)>,
    /// The on-screen stick's deflection since the last tick — see
    /// [`Game::stick_moved`]. `None` on a tick nothing reported one.
    pending_stick: Option<(f32, f32)>,
    /// The output stream and the six cues. On the facade rather than in the
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
    /// A runtime that said it had threads then refused one for the steering
    /// pool. Not the browser, which says it has none and gets a pool with no
    /// workers — see this module's `steer_pool`.
    Pool(String),
}

impl std::fmt::Display for GameError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Server(msg) => write!(f, "server creation failed: {msg}"),
            Self::Pool(msg) => write!(f, "the steering pool would not start: {msg}"),
        }
    }
}

impl std::error::Error for GameError {}

/// The pool [`steer_enemies`] splits the crowd across.
///
/// **The one place the sample asks about threads at all.** `default_spawner`
/// answers native versus browser — `Threads` on a desktop, `Inline` in a
/// browser, which is where the worker backend will land without this file
/// changing — and `Pool::with_workers` treats the count as a request a spawner
/// with no threads answers with zero. So `wasm32` reaches a pool of zero
/// workers however `workers` was set, and there is no `cfg(target_arch)` here.
///
/// `Some(0)` asks for that same pool on a machine that *does* have threads,
/// which is what `--workers 0` is for: the serial half of the determinism
/// comparison, run on the same hardware as the parallel half. It takes
/// [`Inline`] rather than a `Threads` spawner asked for zero workers, so the
/// pool is the browser's own shape and not merely one that behaves like it.
fn steer_pool(workers: Option<usize>) -> Result<Pool, GameError> {
    let spawner: Box<dyn Spawn> = match workers {
        Some(0) => Box::new(Inline),
        _ => default_spawner(),
    };
    match workers {
        None => Pool::new(spawner.as_ref()),
        Some(count) => Pool::with_workers(spawner.as_ref(), count),
    }
    .map_err(|e| GameError::Pool(e.to_string()))
}

impl Game {
    /// Builds the world, the physics system, the server and the client on the
    /// published run.
    ///
    /// # Errors
    ///
    /// [`GameError::Server`] if the operating system would not give the server
    /// the entropy for a resume credential, or if the loopback session did not
    /// come up in the handshake tick.
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
    /// the entropy for a resume credential, or if the loopback session did not
    /// come up in the handshake tick.
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
        // **Three bindings, three devices, one action.** Two keyboard
        // composites because this game has always taken WASD *and* the arrows,
        // and one on-screen stick because a phone has neither. They sum inside
        // the unit disc, so a player pressing a key while pushing the stick asks
        // for one direction rather than for twice the speed.
        action_map.declare(ActionDecl {
            name: ACTION_MOVE.into(),
            kind: ActionKind::Axis2,
            bindings: vec![
                Binding::Wasd {
                    up: KeyCode::KeyW,
                    down: KeyCode::KeyS,
                    left: KeyCode::KeyA,
                    right: KeyCode::KeyD,
                },
                Binding::Wasd {
                    up: KeyCode::ArrowUp,
                    down: KeyCode::ArrowDown,
                    left: KeyCode::ArrowLeft,
                    right: KeyCode::ArrowRight,
                },
                Binding::Virtual(STICK_MOVE.into()),
            ],
        });
        for (name, keys) in [
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
            props: scatter_props(setup.seed),
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
            potions_dropped: 0,
            enemy_views: Vec::new(),
            bolt_views: Vec::new(),
            pickup_views: Vec::new(),
            scratch_entities: Vec::new(),
            steer_velocities: Vec::new(),
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
                pool: steer_pool(setup.workers)?,
            }),
            setup.tick_hz,
            COMPATIBILITY,
        )
        .map_err(|e| GameError::Server(e.to_string()))?;

        let tick_period = session.tick_period();

        // **One tick spent on the handshake, before the game starts.**
        //
        // `Server::update` drains the transport inside `tick`, so the client's
        // hello is not even read until a tick runs — and until the session is
        // established the client has no key and drops every input frame it is
        // asked to send. Spending that tick here is what makes the player's
        // first input the first one the simulation sees: press Space on the
        // opening frame and the run starts on the opening tick.
        //
        // It costs the tick and nothing else. `GameState::WaitingToStart`
        // short-circuits `run_tick` before anything can move, spend or spawn,
        // and every body in this world carries a velocity the game wrote — so
        // the arena this leaves is the arena it was built with.
        let sim_time = tick_period;
        session.client_mut().update(sim_time);
        session.server_mut().update(sim_time);
        session.client_mut().update(sim_time);
        if session.server().session_state() != crcbl::net::SessionState::Connected {
            return Err(GameError::Server(
                "the loopback session did not come up in its first tick".into(),
            ));
        }

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
            sim_time,
            ticks_run: 0,
            pending_keys: Vec::new(),
            pending_stick: None,
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

    /// The action map, for the debug console's `bind` and `unbind`.
    ///
    /// The map is private and this game's `HostedGame` impl is a sibling module,
    /// so `crcbl::engine::HostedGame::actions` has no other way to reach it.
    /// Nothing else writes bindings: the console's rebind is the one caller.
    pub const fn action_map_mut(&mut self) -> &mut ActionMap {
        &mut self.action_map
    }

    /// Where the on-screen stick is deflected to, +X right and +Y up.
    ///
    /// Queued like a key and for the same reason — the widget is driven from the
    /// shell's pump, once per frame, and the action map's bookkeeping is per
    /// tick. Unlike a key it is a **level**, so a second report before the next
    /// tick replaces the first rather than queueing behind it: what the tick
    /// wants is where the thumb is now, and where it was two milliseconds ago is
    /// not an edge anybody can lose.
    pub fn stick_moved(&mut self, x: f32, y: f32) {
        self.pending_stick = Some((x, y));
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
        // Only when the widget reported one. A finger resting on the stick
        // moves nothing and so reports nothing, and the map holds the last
        // deflection across ticks — a value pushed in every tick regardless
        // would be the same value, and a value *cleared* every tick would
        // centre the stick under a thumb that never let go.
        if let Some((x, y)) = self.pending_stick.take() {
            self.action_map.virtual_stick(STICK_MOVE, x, y);
        }

        // First match wins, so two digits in one frame take the earlier button
        // rather than the later one — the same rule an edge follows everywhere
        // else here.
        let choose = ACTION_CHOOSE
            .iter()
            .position(|name| self.action_map.just_pressed(name))
            .map_or(0, |index| index as u8 + 1);
        let (x, y) = self.action_map.axis2(ACTION_MOVE);
        let (up, down, left, right) = eight_way(x, y);
        let intent = Intent {
            up,
            down,
            left,
            right,
            restart: self.action_map.just_pressed(ACTION_RESTART),
            choose,
        };

        let ticks_before = lock(&self.shared).ticks;

        self.sim_time += self.tick_period;
        let (server, client) = self.session.both_mut();

        // The bytes are the whole input path. `Client::set_input` takes the
        // *input data*, wrapping it in `ClientToServer::Input` itself, so this
        // hands over the intent's own wire form rather than a whole message
        // nested inside the data field of another.
        client.set_input(vec![intent.to_wire()]);

        // Send, simulate, then receive — and the send has to come first.
        // `Client::update` is the only thing that puts input on the wire, and
        // the server drains the wire at the top of its tick, so a client
        // updated after the server would be posting this tick's intent to the
        // next one. The second call consumes no tick (the clock has not moved
        // between them); it is there to take the snapshot this tick produced.
        client.update(self.sim_time);
        let server_ticks = server.update(self.sim_time);
        debug_assert_eq!(
            server_ticks, 1,
            "one tick period in must be exactly one server tick out",
        );
        let _alpha = client.update(self.sim_time);
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
        self.audio.set_listener(listener);
        for (id, at) in cues {
            self.audio.play_at(id, at);
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
        // `Playing` after it, the same claim the other games make. `run`
        // is beside it for a bug report, and because it is what tells a restart
        // from a start — only a real restart edge advances it.
        //
        // **Where the wizard is standing is in the line for the same reason**,
        // and it is the only thing here a *movement* input changes: nothing else
        // in this game moves the player, so a position that differs between two
        // heartbeats is the player having walked and cannot be anything else.
        // `web/tools/browser-e2e.mjs` reads it to see a finger drive the stick.
        if state_changed || self.ticks_run.is_multiple_of(60) {
            crcbl::log::info!(
                "[HUD] {:?}  run: {}  time: {:.1}  x: {:.2}  y: {:.2}  best: {}  kills: {}  \
                 hp: {:.0}  lvl: {}  enemies: {}  bolts: {}  gems: {}",
                self.state,
                self.run,
                self.elapsed,
                self.player.x,
                self.player.y,
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
        out.props.clear();
        out.props.extend_from_slice(&logic.props);
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

    /// How many pickups of both kinds are on the ground.
    #[must_use]
    pub fn pickup_count(&self) -> usize {
        lock(&self.shared).pickups.len()
    }

    /// The trees and bushes this game's arena was dealt.
    ///
    /// Fixed for the life of the game, restarts included — see
    /// [`scatter_props`].
    #[must_use]
    pub fn props(&self) -> Vec<PropView> {
        lock(&self.shared).props.clone()
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

    /// How many pickups a full field refused to drop. See [`MAX_PICKUPS`].
    #[must_use]
    pub fn pickups_dropped(&self) -> u64 {
        lock(&self.shared).pickups_dropped
    }

    /// How many potions this game's kills have left on the ground, over every
    /// run. The numerator of the measured drop rate — see
    /// [`POTION_DROP_CHANCE`].
    #[must_use]
    pub fn potions_dropped(&self) -> u64 {
        lock(&self.shared).potions_dropped
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
    /// **Zero between ticks, and the tests here assert that rather than
    /// compensating for it.** `crcbl_server::Server::tick` sweeps between the
    /// game module and the snapshot, so a destruction this game takes is
    /// applied on the tick it happens. This used to be a term in
    /// `Harness::assert_nothing_leaked`'s entity sum, back when the sweep ran
    /// before the module and everything destroyed waited a tick; that sum is an
    /// exact equality now and this is what proves the queue drains.
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

    /// Drops one pickup at a named place, and returns its entity.
    #[cfg(test)]
    pub fn stage_pickup(&mut self, position: DVec3, kind: PickupKind) -> Entity {
        let mut logic = lock(&self.shared);
        let world = self.session.server_mut().world_mut();
        drop_pickup(&mut logic, world, position, kind);
        logic.pickups.last().expect("just dropped one").entity
    }

    /// Where every pickup on the ground is, and what it is.
    #[cfg(test)]
    #[must_use]
    pub fn pickups_on_the_ground(&self) -> Vec<(DVec3, PickupKind)> {
        lock(&self.shared)
            .pickups
            .iter()
            .map(|pickup| (pickup.position, pickup.kind))
            .collect()
    }

    /// Where every pickup on the ground is.
    #[cfg(test)]
    #[must_use]
    pub fn pickup_positions(&self) -> Vec<DVec3> {
        lock(&self.shared)
            .pickups
            .iter()
            .map(|pickup| pickup.position)
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
#[path = "game/tests.rs"]
mod tests;
