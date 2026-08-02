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
//! `docs/plan/sample/03-horde.md` is the plan. This sub-slice is the core loop
//! only — arena, player, enemies, damage, death. The art, the XP and the
//! level-up screen are the next one; the scale push, the measurement and the
//! browser demo are the one after.
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
//! [`crcbl_phys::PhysicsWorld::overlap_sphere`] tests the query sphere against
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

use crcbl_client::Client;
use crcbl_core::FrameClock;
use crcbl_core::input::KeyCode;
use crcbl_ecs::{Entity, GameModule, World};
use crcbl_input::{ActionDecl, ActionKind, ActionMap, Binding};
use crcbl_net::{InMemoryTransport, ProtocolCompatibility};
use crcbl_phys::{ColliderComponent, PhysicsSystem, RigidBody, Segment, Transform};
use crcbl_server::Server;
use glam::DVec3;

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

/// How far in front of the player's centre a bolt appears.
///
/// Outside [`PLAYER_RADIUS`], so a shot never begins inside the thing that fired
/// it — which matters more here than in asteroids, because the enemy the bolt is
/// aimed at is very often already touching the player.
const MUZZLE_OFFSET: f64 = PLAYER_RADIUS + BOLT_RADIUS + 0.05;

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
/// trick.** [`crcbl_phys::PhysicsWorld::overlap_sphere`] tests the query sphere
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
/// **A pure function, not a running generator**, for the reason
/// `apps/asteroids/src/game.rs` gives at length: a generator whose position
/// depends on how many enemies have been spawned so far is hidden state, and
/// hidden state is what makes two runs of one script diverge. Hashing the index
/// instead means the four-hundredth enemy of a run enters in the same place
/// however the run reached it.
///
/// splitmix64's finaliser, taken as 53 bits of mantissa — an `f64` has exactly
/// 53, so this is uniform with nothing thrown away twice.
#[must_use]
pub fn hash_unit(seed: u64, index: u64) -> f64 {
    let mut z = seed.wrapping_add(index.wrapping_mul(0x9E37_79B9_7F4A_7C15));
    z = (z ^ (z >> 30)).wrapping_mul(0xBF58_476D_1CE4_E5B9);
    z = (z ^ (z >> 27)).wrapping_mul(0x94D0_49BB_1331_11EB);
    z ^= z >> 31;
    (z >> 11) as f64 / (1u64 << 53) as f64
}

/// The seed the `runs`-th run of a game seeded with `seed` is dealt from.
///
/// A restart deals a different run, because a game that dealt the same one every
/// time would be memorised rather than played. It changes it *deterministically*
/// — the run counter is simulation state like any other — so a recorded script
/// replayed from a fresh game meets the same horde.
#[must_use]
fn run_seed(seed: u64, runs: u32) -> u64 {
    seed ^ u64::from(runs).wrapping_mul(0xD1B5_4A32_D192_ED03)
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
const ACTION_RESTART: &str = "restart";

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
    /// The restart key, on the tick it went down. An *edge*: held, it would
    /// restart the run sixty times a second.
    restart: bool,
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
    fn to_wire(self) -> u8 {
        u8::from(self.up)
            | (u8::from(self.down) << 1)
            | (u8::from(self.left) << 2)
            | (u8::from(self.right) << 3)
            | (u8::from(self.restart) << 4)
    }
}

// ---------------------------------------------------------------------------
// State
// ---------------------------------------------------------------------------

/// Where a run is.
///
/// **Two states, where breakout, flappy and asteroids each have three.** Those
/// games open on a title screen because they open on a *board* — something to
/// look at while the player decides. This one's board is empty at `t = 0` and
/// fills up because time passes, so a "waiting to start" state would be a blank
/// screen with a prompt on it, and the first thing the player would do is press
/// the key. It starts playing.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum GameState {
    /// Running. The clock is going up and the horde is arriving.
    Playing,
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

/// The mutable game state the server-side module owns.
#[derive(Debug)]
struct GameLogic {
    player: Entity,
    intent: Intent,
    state: GameState,

    player_pos: DVec3,
    player_hp: f64,

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

    /// Live views for the renderer, refilled rather than rebuilt so a
    /// steady-state tick does not allocate.
    enemy_views: Vec<EnemyView>,
    bolt_views: Vec<BoltView>,

    /// Scratch space for the per-tick passes, kept here for the same reason.
    scratch_entities: Vec<Entity>,

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
fn run_tick(logic: &mut GameLogic, world: &mut World) {
    logic.ticks += 1;
    let dt = world.tick_dt();
    let intent = std::mem::take(&mut logic.intent);

    clamp_bodies(logic, world);
    read_player(logic, world);

    if intent.restart {
        restart(logic, world);
    }

    if logic.state == GameState::Playing {
        drive_player(logic, world, intent);
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
        spawn_enemies(logic, world, dt);
        logic.elapsed += dt;
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
fn drive_player(logic: &mut GameLogic, world: &mut World, intent: Intent) {
    let player = logic.player;
    let velocity = intent.direction() * PLAYER_SPEED;
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
/// The [`crcbl_phys::ShapeHit`] each result carries is **discarded**: it is
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
    let target = with_physics(world, |phys| {
        phys.overlap_sphere(origin, WEAPON_RANGE)
            .into_iter()
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

    // No cooldown is spent on an empty field: the gun is ready the instant
    // something walks into range, which is what stops the first enemy of a wave
    // living a quarter of a second longer than the rest.
    let Some((_, aim)) = target else {
        return;
    };
    let Some(direction) = (aim - origin).try_normalize() else {
        // The target is exactly on top of the player. There is no direction to
        // fire in, and contact damage is already dealing with it.
        return;
    };
    logic.fire_timer = FIRE_COOLDOWN;

    let position = origin + direction * MUZZLE_OFFSET;
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
    });
    logic.bolts_fired += 1;
}

/// Applies one tick of contact damage, and kills the player if it runs them out.
///
/// # One query, and every result is a hit
///
/// [`crcbl_phys::PhysicsWorld::overlap_sphere`] tests the query sphere against
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
        log::info!(
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
            damage_enemy(logic, world, target, BOLT_DAMAGE);
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
/// * plus `N` `Vec` allocations, because `overlap_sphere` returns an owned
///   `Vec` and there is no `_into`/callback form of it. That is the single
///   biggest thing standing between this pass and the plan's 10k, and it is a
///   finding against the crate rather than against the sample. Recorded in
///   `docs/backlog.md`.
/// * plus `N` `HashMap` inserts, because `PhysicsSystem` has no `body_mut` and
///   writing a velocity means `set_body`, which is a full map insert. Also a
///   finding, also recorded.
///
/// # It is order-independent, and that is a property rather than an accident
///
/// Nothing in this pass moves a body. `set_body` writes a velocity, and a
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

        for me in &enemies {
            let mut push = DVec3::ZERO;
            for (other, _hit) in phys.overlap_sphere(me.position, separation_query_radius(me.kind))
            {
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
            if let Some(mut body) = phys.body(me.entity).copied() {
                body.velocity = velocity;
                phys.set_body(me.entity, body);
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

/// Clears the field and starts a run that is not the one just played.
fn restart(logic: &mut GameLogic, world: &mut World) {
    for enemy in std::mem::take(&mut logic.enemies) {
        with_physics(world, |phys| phys.remove_entity(enemy.entity));
        world.despawn(enemy.entity);
    }
    logic.by_entity.clear();
    for bolt in std::mem::take(&mut logic.bolts) {
        despawn_bolt(world, bolt.entity);
    }
    logic.runs = logic.runs.wrapping_add(1);
    logic.state = GameState::Playing;
    logic.player_hp = PLAYER_MAX_HP;
    logic.elapsed = 0.0;
    logic.kills = 0;
    logic.fire_timer = 0.0;
    logic.spawn_timer = 0.0;
    logic.spawn_counter = 0;
    place_player(logic, world, DVec3::ZERO);
}

/// Puts the player in the middle of the arena, stationary.
fn place_player(logic: &mut GameLogic, world: &mut World, position: DVec3) {
    let player = logic.player;
    logic.player_pos = position;
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
    for sys in world.schedule_mut().iter_mut() {
        if sys.name() == "physics"
            && let Some(phys) = sys.as_any_mut().downcast_mut::<PhysicsSystem>()
        {
            return Some(f(phys));
        }
    }
    None
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
    /// What is left of the player's hit points.
    pub player_hp: f64,
    pub enemies: Vec<EnemyView>,
    pub bolts: Vec<BoltView>,
    /// How long this run has lasted, in simulated seconds.
    pub elapsed: f64,
    pub kills: u64,
    pub state: Option<GameState>,
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
    server: Server<InMemoryTransport>,
    client: Client<InMemoryTransport>,
    shared: Arc<Mutex<GameLogic>>,
    /// Exactly one tick period per [`Game::tick`], so the server's accumulator
    /// yields exactly one tick per call.
    tick_period: Duration,
    sim_time: Duration,
    ticks_run: u64,
    /// Queued key events from the shell pump, replayed after `begin_tick`.
    pending_keys: Vec<(KeyCode, bool)>,
    /// Mirrors of the shared state, refreshed after each tick so the render and
    /// HUD paths never take the lock.
    pub state: GameState,
    pub player: DVec3,
    pub player_hp: f64,
    pub elapsed: f64,
    pub kills: u64,
    prev_log_state: GameState,
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
            state: GameState::Playing,
            player_pos: DVec3::ZERO,
            player_hp: PLAYER_MAX_HP,
            fire_timer: 0.0,
            spawn_timer: 0.0,
            elapsed: 0.0,
            kills: 0,
            enemies: Vec::new(),
            by_entity: HashMap::new(),
            bolts: Vec::new(),
            seed: setup.seed,
            runs: 0,
            spawn_counter: 0,
            max_enemies: setup.max_enemies,
            enemies_spawned: 0,
            bolts_fired: 0,
            enemy_views: Vec::new(),
            bolt_views: Vec::new(),
            scratch_entities: Vec::new(),
            ticks: 0,
        }));

        {
            let mut logic = lock(&shared);
            place_player(&mut logic, &mut world, DVec3::ZERO);
        }

        let (server_transport, client_transport) = InMemoryTransport::pair();
        let mut server = Server::try_new_with_compatibility(
            world,
            server_transport,
            setup.tick_hz,
            COMPATIBILITY,
        )
        .map_err(|e| GameError::Server(e.to_string()))?;
        server.set_module(Box::new(HordeModule {
            shared: Arc::clone(&shared),
        }));

        let mut client = Client::new_with_compatibility(
            World::new(),
            client_transport,
            setup.tick_hz,
            COMPATIBILITY,
        );

        let tick_period = FrameClock::new(setup.tick_hz).tick_dt();

        // Both clocks establish their baseline from the first `update`, which
        // therefore runs no ticks. Spending it here, at time zero, is what lets
        // `tick` promise that every later call runs exactly one.
        server.update(Duration::ZERO);
        client.update(Duration::ZERO);

        {
            let mut logic = lock(&shared);
            refresh_views(&mut logic, server.world_mut());
        }

        let game = Self {
            player_entity,
            action_map,
            server,
            client,
            shared,
            tick_period,
            sim_time: Duration::ZERO,
            ticks_run: 0,
            pending_keys: Vec::new(),
            state: GameState::Playing,
            player: DVec3::ZERO,
            player_hp: PLAYER_MAX_HP,
            elapsed: 0.0,
            kills: 0,
            prev_log_state: GameState::Playing,
        };
        log::info!(
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

        let intent = Intent {
            up: action_held(&self.action_map, ACTION_UP),
            down: action_held(&self.action_map, ACTION_DOWN),
            left: action_held(&self.action_map, ACTION_LEFT),
            right: action_held(&self.action_map, ACTION_RIGHT),
            restart: action_just_pressed(&self.action_map, ACTION_RESTART),
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
            logic.ticks
        };

        self.client.set_input(vec![intent.to_wire()]);

        self.sim_time += self.tick_period;
        let server_ticks = self.server.update(self.sim_time);
        debug_assert_eq!(
            server_ticks, 1,
            "one tick period in must be exactly one server tick out",
        );
        let _alpha = self.client.update(self.sim_time);
        self.ticks_run += 1;

        let ticks_after = {
            let logic = lock(&self.shared);
            self.state = logic.state;
            self.player = logic.player_pos;
            self.player_hp = logic.player_hp;
            self.elapsed = logic.elapsed;
            self.kills = logic.kills;
            logic.ticks
        };
        debug_assert_eq!(
            ticks_after,
            ticks_before + u64::from(server_ticks),
            "game logic must run exactly once per physics tick",
        );

        let state_changed = self.state != self.prev_log_state;
        self.prev_log_state = self.state;
        // **Every sixty ticks, which is a second of simulated time, and the same
        // cadence breakout, flappy and asteroids use.** `web/tools/browser-e2e.mjs`
        // watches for this heartbeat to tell a paused demo from a running one.
        if state_changed || self.ticks_run.is_multiple_of(60) {
            log::info!(
                "[HUD] {:?}  time: {:.1}  kills: {}  hp: {:.0}  enemies: {}  bolts: {}",
                self.state,
                self.elapsed,
                self.kills,
                self.player_hp,
                self.enemy_count(),
                self.bolt_count(),
            );
        }
    }

    /// Everything the renderer draws, in world space.
    ///
    /// `out` is reused across frames so a steady-state frame does not allocate.
    pub fn render_state(&self, out: &mut RenderState) {
        let logic = lock(&self.shared);
        out.player = logic.player_pos;
        out.player_hp = logic.player_hp;
        out.enemies.clear();
        out.enemies.extend_from_slice(&logic.enemy_views);
        out.bolts.clear();
        out.bolts.extend_from_slice(&logic.bolt_views);
        out.elapsed = logic.elapsed;
        out.kills = logic.kills;
        out.state = Some(logic.state);
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

    /// The ceiling on live enemies this game was built with.
    #[must_use]
    pub fn max_enemies(&self) -> usize {
        lock(&self.shared).max_enemies
    }

    /// How many entities the simulation is holding.
    #[must_use]
    pub fn entity_count(&mut self) -> usize {
        self.server.world_mut().entity_count()
    }

    /// How many entities are queued for destruction and not yet swept.
    ///
    /// **Not an implementation detail — a number the counts above cannot be read
    /// without.** `crcbl_ecs::World::sweep` runs at the end of `World::tick`,
    /// and `crcbl-server` calls `GameModule::tick` *after* that, so everything
    /// this game destroys — and it destroys a great deal — waits one tick before
    /// the pool lets go of it. Recorded in `docs/backlog.md` as a finding
    /// against the module hook's placement.
    #[must_use]
    pub fn pending_despawns(&mut self) -> usize {
        self.server.world_mut().dead_queue_len()
    }

    /// How many colliders the physics world is holding.
    ///
    /// A second, independent count, because the two leak separately: a collider
    /// left behind when its entity goes is an invisible wall, and nothing about
    /// the entity count would notice.
    #[must_use]
    pub fn collider_count(&mut self) -> usize {
        with_physics(self.server.world_mut(), |phys| phys.collider_count()).unwrap_or(0)
    }

    /// Puts the player somewhere specific, for a test that needs a known board.
    #[cfg(test)]
    pub fn stage_player(&mut self, position: DVec3) {
        let mut logic = lock(&self.shared);
        let world = self.server.world_mut();
        place_player(&mut logic, world, position);
    }

    /// Clears the field, for the same reason.
    #[cfg(test)]
    pub fn clear_enemies(&mut self) {
        let mut logic = lock(&self.shared);
        let world = self.server.world_mut();
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
        let world = self.server.world_mut();
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
        let mut found = with_physics(self.server.world_mut(), |phys| {
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

// ---- input helpers ----------------------------------------------------------

fn action_held(map: &ActionMap, name: &str) -> bool {
    map.action(name).is_some_and(|v| match v {
        crcbl_input::ActionValue::Button(b) => matches!(
            b.state,
            crcbl_input::ButtonState::Pressed | crcbl_input::ButtonState::Held { .. }
        ),
        _ => false,
    })
}

fn action_just_pressed(map: &ActionMap, name: &str) -> bool {
    map.action(name).is_some_and(|v| match v {
        crcbl_input::ActionValue::Button(b) => b.just_pressed,
        _ => false,
    })
}

// ---- tests ------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use crcbl_core::time::{ManualTime, TimeSource as _};

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
            Self {
                game: Game::with_setup(setup).expect("a headless game always starts"),
                clock: FrameClock::new(setup.tick_hz),
                time: ManualTime::new(),
                frame_step: FrameClock::new(frame_hz).tick_dt(),
                ticks: 0,
                held: [false; 4],
                restarts: 0,
            }
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
                    if self.game.state == GameState::Dead {
                        // Start the next run. The restart is an *edge*, so it
                        // is pressed and released inside the one tick — and it
                        // is the harness that does it rather than `autopilot`,
                        // because the plan is a set of held keys and this is
                        // not one.
                        self.restarts += 1;
                        self.game.key_event(KeyCode::KeyR, true);
                        self.game.key_event(KeyCode::KeyR, false);
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
            let pending = self.game.pending_despawns();
            assert_eq!(
                self.game.entity_count(),
                1 + enemies + bolts + pending,
                "tick {}: {enemies} enemies, {bolts} bolts and {pending} awaiting the \
                 sweep do not account for the world",
                self.ticks,
            );
            // An equality, not a bound: every collider in the world is an
            // enemy. The player is not in the broadphase and neither is a bolt
            // — see `contact_damage` and `sweep_bolts`.
            assert_eq!(
                self.game.collider_count(),
                enemies,
                "tick {}: {enemies} enemies do not account for the broadphase",
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

    /// **A bolt outlives the range it was fired at.** A shot that expired short
    /// of its target would make [`WEAPON_RANGE`] a lie.
    ///
    /// The relation is asserted, not the number, so a later tuning pass that
    /// changes either constant is told rather than left to find out.
    #[test]
    fn the_reach_of_a_bolt_covers_the_weapons_range() {
        let reach = BOLT_SPEED * BOLT_LIFE;
        assert!(
            reach > WEAPON_RANGE + max_enemy_radius(),
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

    /// A restart puts the clock, the hit points, the kills and the player back,
    /// and deals a horde that is not the one just played.
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
        assert_eq!(harness.game.state, GameState::Playing);
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
    }

    /// A dead run restarts, which is the only way out of the death screen.
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
        assert_eq!(harness.game.state, GameState::Playing);
        assert_eq!(harness.game.player_hp, PLAYER_MAX_HP);
        assert_eq!(harness.game.enemy_count(), 0);
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
        while harness.ticks < 3_600 {
            harness.game.set_player_hp(PLAYER_MAX_HP);
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
        // and every bolt that can be in the air at once — plus as many again
        // waiting for the deferred sweep. Derived rather than measured: a number
        // taken from a passing run breaks on the next tuning change for a reason
        // that is not a leak.
        let bolts_in_flight = (BOLT_LIFE / FIRE_COOLDOWN).ceil() as usize + 1;
        let ceiling = 2 * (1 + 120 + bolts_in_flight);
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
        log::info!(
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
            let mut seen = vec![harness.game.run_seed()];
            for _ in 0..restarts {
                harness.tap(KeyCode::KeyR);
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
}
