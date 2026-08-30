//! Asteroids' simulation: a ship that turns and thrusts, bullets that sweep,
//! rocks that split, and a playfield with no edges.
//!
//! # What is different about this one
//!
//! Breakout spawns its world once and only ever removes from it. Flappy runs a
//! treadmill at a couple of entities a second. This game creates and destroys
//! entities *constantly* — a bullet every sixth of a second, two rocks for every
//! one shot, a whole wave at a time — which is the reason
//! `docs/plan/sample/02-asteroids.md` picked it: generational ids, deferred
//! destruction and pool slot recycling all get hammered, and a leak shows up as
//! a number that climbs.
//!
//! # Three seams into `crcbl-phys`, and each is a different query
//!
//! * **The ship** is a dynamic body driven by the L1 force pipeline —
//!   [`ThrustForce`] for the engine, a matching damping term for the coast. Its
//!   *rotation* is not: `crcbl-phys` has no angular velocity and no torque (see
//!   `docs/backlog.md`), so [`SHIP_TURN_RATE`] is integrated by hand here and
//!   written into the [`Transform`] the thrust then reads.
//! * **Bullets** are segment CCD. A bullet has no collider at all: every tick it
//!   sweeps `prev → cur` with [`PhysicsSystem::sweep_body`], so the fastest
//!   bullet the game can produce cannot pass through the thinnest rock. See
//!   [`sweep_bullets`].
//! * **The ship against the rocks** is a sphere overlap against the broadphase,
//!   [`PhysicsSystem::overlap_sphere`], centred on the ship. The ship itself is
//!   not *in* the broadphase — every collider in this world is a rock — which
//!   falls out of that query taking a free centre rather than an entity. See
//!   [`check_ship`].
//!
//! # A wrap is a teleport, and a teleport is a remove-and-re-insert
//!
//! This is the rule this sample had to choose, because nothing in `crcbl-phys`
//! chooses it — see [`teleport`] for the argument and for what was measured.
//!
//! # Where the simulation runs
//!
//! Inside the server's tick, in [`AsteroidsModule::tick`] — the hook `crcbl-ecs`
//! documents as running every server tick *after* the ECS schedule, which means
//! after [`PhysicsSystem::step`] has integrated everything. [`Game`] is the
//! client-side facade: it resolves input into an [`Intent`], puts the intent on
//! the wire, advances the server and the client by exactly one tick period, and
//! reads back what to draw.
//!
//! # The input path is the wire and nothing else
//!
//! [`Intent::to_wire`] is handed to `Client::set_input`, sealed with the session
//! key, and read back by [`Intent::from_wire`] inside [`AsteroidsModule::tick`],
//! out of the [`ClientInputs`] the server hands every module. The shared cell
//! this game and its module both hold is output-only: the module writes what it
//! simulated and the facade reads it back to draw, and nothing travels the other
//! way. A dropped input frame is a lost tick of intent, which is what playing
//! over a real transport means — `InMemoryTransport` drops nothing, so a
//! single-player session never sees one.
//!
//! The order in [`Game::tick`] is what keeps that free of lag: the client is
//! updated **before** the server, because `Client::update` is the only thing
//! that puts input on the wire and the server drains the wire at the top of its
//! tick. Updating it afterwards would post this tick's intent to the next tick.
//! [`Game::with_seed`] spends one tick on the handshake for the same reason — an
//! unkeyed client drops what it is asked to send — and deals the board *after*
//! that tick, because unlike breakout's this game's field moves on every tick
//! there is one.

use std::sync::{Arc, Mutex, MutexGuard};
use std::time::Duration;

use crcbl::core::input::KeyCode;
use crcbl::ecs::{ClientInputs, Entity, GameModule, World};
use crcbl::input::{ActionDecl, ActionKind, ActionMap, Binding};
use crcbl::math::{DQuat, DVec3};
use crcbl::net::ProtocolCompatibility;
use crcbl::phys::{
    ColliderComponent, DampingForce, PhysicsSystem, RigidBody, ThrustForce, Transform,
};
use crcbl::session::Loopback;

/// Distinct from breakout's and flappy's, because they are distinct protocols:
/// a client built for one must not hand-shake with a server running another.
const COMPATIBILITY: ProtocolCompatibility = ProtocolCompatibility {
    protocol_version: 3,
    engine_build_id: 0x0043_5243_424C,
    schema_hash: 0x0041_5354_524F,
};

/// The default simulation rate. The value reaches the server, the client, the
/// ECS `tick_dt` and the integrator, so there is exactly one rate in the
/// process.
pub const DEFAULT_TICK_HZ: u32 = 60;

// ---------------------------------------------------------------------------
// The playfield
// ---------------------------------------------------------------------------

/// Half the width of the playfield, in world units.
///
/// 4:3 against [`WORLD_HALF_HEIGHT`], which is the shape of the window
/// `app.rs` opens and of the canvas the browser demo will use. A field that did
/// not match the viewport would either hide rocks or show empty margin, and in a
/// wrapping game both read as a bug rather than as a border.
pub const WORLD_HALF_WIDTH: f64 = 16.0;

/// Half the height of the playfield, in world units.
pub const WORLD_HALF_HEIGHT: f64 = 12.0;

// ---------------------------------------------------------------------------
// The ship
// ---------------------------------------------------------------------------

/// How close a rock has to come to the ship's centre to kill it.
///
/// Not a collider — the ship has none, see this module's header — but the
/// radius of the sphere it queries the broadphase with, which comes to the same
/// thing.
/// Smaller than the hull will look once there is art: a ship whose kill radius
/// matches its longest spine dies to near misses, and asteroids is a game of
/// near misses.
pub const SHIP_RADIUS: f64 = 0.55;

/// How fast the ship turns, in radians per second.
///
/// 3.4 rad/s is a full revolution in 1.85 s. Faster than that and a tap of the
/// key overshoots the rock; slower and the far side of the screen takes longer
/// to aim at than to fly to.
pub const SHIP_TURN_RATE: f64 = 3.4;

/// The engine's thrust, in newtons, against a ship of one kilogram.
pub const SHIP_THRUST: f64 = 14.0;

/// The coast, as a damping coefficient in kg/s.
///
/// Two constants in one, which is why they are tuned together:
///
/// * **Terminal speed is `SHIP_THRUST / SHIP_DAMPING`** — 14 units/s, which
///   crosses the 32-unit field in 2.3 s. There is deliberately no separate speed
///   clamp: a clamp is a second mechanism that can disagree with this one, and a
///   guard that the damping makes unreachable is not a guard.
/// * **The coast's time constant is `mass / SHIP_DAMPING`** — 1 s to fall to
///   `1/e` of a speed. Long enough that the ship drifts (which is the game),
///   short enough that a panicked burst is recoverable inside a wave.
pub const SHIP_DAMPING: f64 = 1.0;

/// The ship's mass, in kilograms. One, so thrust in newtons reads as
/// acceleration in units per second squared.
const SHIP_MASS: f64 = 1.0;

/// How long a destroyed ship stays gone, in seconds, before it may return.
pub const RESPAWN_DELAY: f64 = 1.0;

/// How long it may wait for a clear centre beyond that, in seconds.
///
/// The ship returns to the middle of the field, and returning it into a rock
/// would spend a life on nothing the player did. So it waits for
/// [`RESPAWN_CLEAR_RADIUS`] to be empty — but not forever: rocks drift, and a
/// slow one crossing the centre must not stall the game, so after this the ship
/// comes back regardless.
pub const RESPAWN_MAX_WAIT: f64 = 5.0;

/// How much room around the centre must be clear before the ship returns.
pub const RESPAWN_CLEAR_RADIUS: f64 = 3.5;

/// Lives a game starts with.
pub const STARTING_LIVES: u32 = 3;

// ---------------------------------------------------------------------------
// Bullets
// ---------------------------------------------------------------------------

/// Muzzle speed, in world units per second, added to the ship's own velocity.
///
/// Added, because a bullet is a thing that leaves a moving ship. 24 against a
/// terminal [`SHIP_THRUST`] / [`SHIP_DAMPING`] of 14 keeps the shot ahead of the
/// ship even at full speed, which is the property that makes it a weapon.
pub const BULLET_SPEED: f64 = 24.0;

/// The radius the bullet's sweep uses. A bullet has no collider — see this
/// module's header — so this is only ever the radius of the swept sphere.
pub const BULLET_RADIUS: f64 = 0.12;

/// How long a bullet lives, in seconds.
///
/// At [`BULLET_SPEED`] that is 19.2 units of travel, which is deliberately
/// **less than the field is tall** (24) and well short of how wide it is (32).
/// The playfield wraps, so a longer-lived shot comes back round and arrives
/// behind the ship that fired it — which is not a rule anybody would guess and
/// reads as the game cheating. At one second it did exactly that: 24 units up a
/// 24-unit field is one lap. `the_reach_of_a_shot_is_less_than_one_lap` asserts
/// the relation rather than the number.
pub const BULLET_LIFE: f64 = 0.8;

/// The gap between shots, in seconds.
pub const FIRE_COOLDOWN: f64 = 0.16;

/// How many of the player's bullets may be in the air at once.
///
/// Four, the arcade original's number, and the reason firing has a rhythm rather
/// than being a held key.
pub const MAX_BULLETS: usize = 4;

/// How far in front of the ship's centre a bullet appears.
///
/// Outside [`SHIP_RADIUS`], so a shot never begins inside the hull that fired
/// it.
const MUZZLE_OFFSET: f64 = SHIP_RADIUS + BULLET_RADIUS + 0.05;

// ---------------------------------------------------------------------------
// Hit flashes
// ---------------------------------------------------------------------------

/// How long a hit flash lives, in seconds.
///
/// 0.15 s is nine ticks at 60 Hz: long enough to read, short enough that two
/// hits near each other do not smear into one.
pub const FLASH_LIFE: f64 = 0.15;

// ---------------------------------------------------------------------------
// Rocks
// ---------------------------------------------------------------------------

/// How big a rock is, which decides everything else about it.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum RockSize {
    Large,
    Medium,
    Small,
}

impl RockSize {
    /// The collider radius.
    #[must_use]
    pub const fn radius(self) -> f64 {
        match self {
            Self::Large => 1.7,
            Self::Medium => 1.0,
            Self::Small => 0.55,
        }
    }

    /// How fast a rock of this size drifts, in world units per second.
    ///
    /// Smaller is faster, which is the whole difficulty curve: shooting a large
    /// rock does not clear the screen, it fills it with quicker ones.
    #[must_use]
    pub const fn speed(self) -> f64 {
        match self {
            Self::Large => 2.2,
            Self::Medium => 3.2,
            Self::Small => 4.4,
        }
    }

    /// What destroying one is worth. The arcade original's values.
    #[must_use]
    pub const fn score(self) -> u32 {
        match self {
            Self::Large => 20,
            Self::Medium => 50,
            Self::Small => 100,
        }
    }

    /// What it breaks into, or `None` for the size that simply dies.
    #[must_use]
    pub const fn child(self) -> Option<Self> {
        match self {
            Self::Large => Some(Self::Medium),
            Self::Medium => Some(Self::Small),
            Self::Small => None,
        }
    }

    /// How fast a rock of this size tumbles, in radians per second — the
    /// magnitude; `spawn_rock` draws the sign from the board's seed.
    ///
    /// **Smaller is faster, as [`Self::speed`] is**, and for the same reason: a
    /// small rock is a chip off a bigger one and reads as lighter. The numbers
    /// are bounded by what the *art* can show rather than by physics — a rock is
    /// 34, 20 or 11 texels across, so a turn faster than about half a revolution
    /// a second stops reading as a tumble and starts reading as a flicker.
    ///
    /// This is game state and not physics state, exactly as
    /// [`SHIP_TURN_RATE`] is: `crcbl-phys` has no angular velocity, so
    /// `tumble_rocks` integrates it by hand. It never reaches a collider —
    /// every rock's collider is a sphere, and a sphere that turns is the same
    /// sphere — so a tumble is a pure presentation term and cannot change what
    /// the simulation does.
    #[must_use]
    pub const fn spin(self) -> f64 {
        match self {
            Self::Large => 0.55,
            Self::Medium => 0.9,
            Self::Small => 1.5,
        }
    }

    /// The collider a rock of this size carries.
    #[must_use]
    pub const fn collider(self) -> ColliderComponent {
        ColliderComponent::Sphere {
            offset: DVec3::ZERO,
            radius: self.radius(),
            is_trigger: false,
        }
    }
}

/// How many children a split produces. Two, and the count is named because the
/// entity-lifecycle tests assert against it rather than against a literal.
pub const SPLIT_CHILDREN: usize = 2;

/// The narrowest angle, in radians, between a child's course and its parent's.
const SPLIT_ANGLE_MIN: f64 = 0.35;

/// How much wider than [`SPLIT_ANGLE_MIN`] a child's course may be.
const SPLIT_ANGLE_RANGE: f64 = 0.6;

// ---------------------------------------------------------------------------
// Waves
// ---------------------------------------------------------------------------

/// How many rocks the first wave puts on the field.
pub const FIRST_WAVE_ROCKS: u32 = 4;

/// The ceiling on that count.
///
/// Without one the twentieth wave is unplayable *and* slow, and neither is the
/// difficulty curve the game wants. Eleven large rocks split to 44, which is
/// already more than the screen holds comfortably.
pub const MAX_WAVE_ROCKS: u32 = 11;

/// How many rocks wave `wave` opens with. Wave 0 is the first.
#[must_use]
pub const fn wave_rocks(wave: u32) -> u32 {
    let count = FIRST_WAVE_ROCKS + wave;
    if count > MAX_WAVE_ROCKS {
        MAX_WAVE_ROCKS
    } else {
        count
    }
}

// ---------------------------------------------------------------------------
// Determinism: every random-looking number is a pure function of a seed
// ---------------------------------------------------------------------------

/// The board every game is dealt unless a caller picks another.
pub const DEFAULT_SEED: u64 = 0x4153_5452_4F49_4453;

/// A uniform value in `[0, 1)` from `seed` and `index`.
///
/// The engine's, re-exported so this game's three index spaces stay beside the
/// draws that use them. [`crcbl::core::rand`] is where the argument for hashing
/// an index rather than stepping a generator is written down — every sample
/// reached it independently, which is why it is the engine's shape now.
pub use crcbl::core::rand::hash_unit;

/// The seed the `runs`-th board of a game seeded with `seed` is dealt from.
///
/// A restart deals a different board, because a game that dealt the same hand
/// every time would be memorised rather than played. It changes it
/// *deterministically* — the run counter is simulation state like any other — so
/// a recorded script replayed from a fresh game meets the same rocks.
#[must_use]
fn board_seed(seed: u64, runs: u32) -> u64 {
    crcbl::core::rand::salt(seed, u64::from(runs))
}

/// Index space for wave spawns. Disjoint from the split space below, so a wave
/// and a split never draw the same number.
const fn wave_index(wave: u32, rock: u32, which: u64) -> u64 {
    (wave as u64) << 20 | (rock as u64) << 4 | which
}

/// Index space for splits: the top bit, plus the spawn counter.
const fn split_index(counter: u64) -> u64 {
    0x8000_0000_0000_0000 | counter
}

/// Index space for a rock's tumble: bit 62, the spawn counter, and which of the
/// two draws — the starting angle or the direction of spin.
///
/// Disjoint from both of the above. [`wave_index`] shifts a `u32` wave left by
/// 20 and so cannot reach bit 62, and [`split_index`] sets bit 63.
const fn tumble_index(spawned: u64, which: u64) -> u64 {
    0x4000_0000_0000_0000 | (spawned << 1) | which
}

/// Where rock `rock` of wave `wave` enters the field.
///
/// **On the border, never in the middle.** The ship respawns at the centre and
/// a wave arrives while the player is standing there, so a position drawn freely
/// from the field would sooner or later put a rock on top of the ship — a life
/// lost to the dealer rather than to the player. Walking the perimeter is a pure
/// function *and* is provably far from the centre, where a rejection loop would
/// be neither.
#[must_use]
pub fn wave_rock_position(seed: u64, wave: u32, rock: u32) -> DVec3 {
    let t = hash_unit(seed, wave_index(wave, rock, 0));
    perimeter_point(t)
}

/// Which way rock `rock` of wave `wave` drifts.
#[must_use]
pub fn wave_rock_velocity(seed: u64, wave: u32, rock: u32) -> DVec3 {
    let angle = hash_unit(seed, wave_index(wave, rock, 1)) * std::f64::consts::TAU;
    DVec3::new(angle.cos(), angle.sin(), 0.0) * RockSize::Large.speed()
}

/// The point `t` of the way around the playfield's border, `t` in `[0, 1)`.
fn perimeter_point(t: f64) -> DVec3 {
    let (w, h) = (WORLD_HALF_WIDTH, WORLD_HALF_HEIGHT);
    // Four sides, each parameterised over a quarter of `t`. Uneven in length —
    // a wide field gets more of its rocks per unit of edge on the short sides —
    // which nothing here cares about and which keeps this arithmetic exact.
    let side = (t * 4.0) as u32;
    let along = t.mul_add(4.0, -f64::from(side));
    match side {
        0 => DVec3::new(-w + along * 2.0 * w, h, 0.0),
        1 => DVec3::new(w, h - along * 2.0 * h, 0.0),
        2 => DVec3::new(w - along * 2.0 * w, -h, 0.0),
        // `t` is in [0, 1) so `side` is in 0..=3; the catch-all is the fourth
        // side rather than a panic, because a caller is not obliged to know
        // that.
        _ => DVec3::new(-w, -h + along * 2.0 * h, 0.0),
    }
}

/// The course of split child `child` of a rock destroyed at spawn counter
/// `counter`, given the parent's direction.
fn split_velocity(
    seed: u64,
    counter: u64,
    child: usize,
    parent_dir: DVec3,
    size: RockSize,
) -> DVec3 {
    let jitter = hash_unit(seed, split_index(counter).wrapping_add(child as u64));
    let magnitude = SPLIT_ANGLE_MIN + jitter * SPLIT_ANGLE_RANGE;
    // The children go opposite ways around the parent's course, so a split
    // always opens rather than sending both halves the same way.
    let angle = if child.is_multiple_of(2) {
        magnitude
    } else {
        -magnitude
    };
    (DQuat::from_rotation_z(angle) * parent_dir) * size.speed()
}

// ---------------------------------------------------------------------------
// Screen wrap
// ---------------------------------------------------------------------------

/// Brings `v` back inside `[-half, half)`.
///
/// Exact for a value already inside, which matters: the tick decides whether to
/// teleport a body by comparing this against the position it was given, and a
/// round trip through `rem_euclid` that returned a value one ulp away would
/// teleport every body every tick.
#[must_use]
pub fn wrap_axis(v: f64, half: f64) -> f64 {
    if v >= -half && v < half {
        return v;
    }
    let span = 2.0 * half;
    (v + half).rem_euclid(span) - half
}

/// Brings a position back inside the playfield.
#[must_use]
pub fn wrap_position(position: DVec3) -> DVec3 {
    DVec3::new(
        wrap_axis(position.x, WORLD_HALF_WIDTH),
        wrap_axis(position.y, WORLD_HALF_HEIGHT),
        position.z,
    )
}

// ---------------------------------------------------------------------------
// Input
// ---------------------------------------------------------------------------

const ACTION_LEFT: &str = "left";
const ACTION_RIGHT: &str = "right";
const ACTION_THRUST: &str = "thrust";
const ACTION_FIRE: &str = "fire";
const ACTION_RESTART: &str = "restart";

/// One tick of player intent.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
struct Intent {
    left: bool,
    right: bool,
    thrust: bool,
    /// The trigger, on the tick it went down. An *edge*: a held key would empty
    /// the magazine into one rock at whatever the tick rate happens to be.
    fire: bool,
    restart: bool,
}

const INTENT_LEFT: u8 = 1 << 0;
const INTENT_RIGHT: u8 = 1 << 1;
const INTENT_THRUST: u8 = 1 << 2;
const INTENT_FIRE: u8 = 1 << 3;
const INTENT_RESTART: u8 = 1 << 4;
/// Every bit the flag byte defines. One set outside this mask is a frame
/// something other than [`Intent::to_wire`] wrote.
const INTENT_FLAGS: u8 = INTENT_LEFT | INTENT_RIGHT | INTENT_THRUST | INTENT_FIRE | INTENT_RESTART;

impl Intent {
    /// The wire form handed to `Client::set_input`: one byte of flags.
    fn to_wire(self) -> u8 {
        let mut flags = 0;
        if self.left {
            flags |= INTENT_LEFT;
        }
        if self.right {
            flags |= INTENT_RIGHT;
        }
        if self.thrust {
            flags |= INTENT_THRUST;
        }
        if self.fire {
            flags |= INTENT_FIRE;
        }
        if self.restart {
            flags |= INTENT_RESTART;
        }
        flags
    }

    /// The intent a client sealed, read back on the server's side of the wire.
    ///
    /// `None` for anything this build did not write: a payload that is not one
    /// byte, or a flag outside [`INTENT_FLAGS`]. **Validated rather than
    /// trusted**, because these are the only bytes in this game a peer chooses
    /// — every field below is a flag, so the mask is the whole of the check,
    /// and a frame that fails it is one no wire form of this game produces.
    fn from_wire(bytes: &[u8]) -> Option<Self> {
        let &[flags] = bytes else {
            return None;
        };
        if flags & !INTENT_FLAGS != 0 {
            return None;
        }
        Some(Self {
            left: flags & INTENT_LEFT != 0,
            right: flags & INTENT_RIGHT != 0,
            thrust: flags & INTENT_THRUST != 0,
            fire: flags & INTENT_FIRE != 0,
            restart: flags & INTENT_RESTART != 0,
        })
    }

    /// Folds one server tick's worth of input frames into the intent to run it
    /// with.
    ///
    /// Normally one frame arrives per tick and this is a decode. Several is a
    /// client whose clock ran ahead of the server's, and then each field takes
    /// the rule its own meaning asks for:
    ///
    /// * `left`, `right` and `thrust` are **held**, so the latest frame wins: a
    ///   hand is wherever it is now, and a key released in the second frame was
    ///   released however hard the first one was holding it.
    /// * `fire` and `restart` are **edges**, so they are OR-ed: a trigger the
    ///   player pulled in any of these frames is one they pulled, and a later
    ///   frame that says nothing about it is not a release. Taking the latest
    ///   here would silently swallow shots at exactly the moment the client is
    ///   sending more of them than the server is consuming.
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
            merged.left = frame.left;
            merged.right = frame.right;
            merged.thrust = frame.thrust;
            merged.fire |= frame.fire;
            merged.restart |= frame.restart;
        }
        merged
    }
}

// ---------------------------------------------------------------------------
// State
// ---------------------------------------------------------------------------

/// Where a game is.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum GameState {
    /// Rocks drift, the ship is inert. The first shot begins the game.
    WaitingToStart,
    /// Playing.
    Playing,
    /// Out of lives. Fire or restart begins a new game.
    GameOver,
}

/// One rock.
#[derive(Clone, Copy, Debug)]
struct Rock {
    entity: Entity,
    size: RockSize,
    /// How far it has tumbled, in radians, kept in `[0, τ)`.
    ///
    /// Presentation state that lives in the simulation, for the same reason the
    /// ship's heading does: it is integrated per tick from a seed, so it is
    /// deterministic, it replays, and the renderer never has to own a clock.
    angle: f64,
    /// The same at the end of the previous tick, so the renderer can interpolate
    /// across a frame that lands between two ticks. See [`lerp_angle`].
    prev_angle: f64,
    /// Radians per second, signed. [`RockSize::spin`] is the magnitude.
    spin: f64,
    /// Where it was at the end of the previous tick. The position half of the
    /// angle pair above: positions live in physics, so this is advanced by
    /// [`refresh_views`] at the end of each tick rather than captured at the
    /// top of [`run_tick`].
    prev_position: DVec3,
    /// Whether the wrap teleported it this tick. A teleported body must be
    /// drawn snapped to its current position, not interpolated — the previous
    /// one is a whole field away. Cleared by [`refresh_views`].
    teleported: bool,
}

/// One bullet. No collider: see [`sweep_bullets`].
#[derive(Clone, Copy, Debug)]
struct Bullet {
    entity: Entity,
    /// Seconds left before it expires.
    life: f64,
    /// Where it was at the end of the previous tick, advanced by
    /// [`refresh_views`] like [`Rock::prev_position`].
    prev_position: DVec3,
    /// Whether the wrap teleported it this tick. See [`Rock::teleported`].
    teleported: bool,
}

/// What the renderer needs for one rock.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct RockView {
    pub position: DVec3,
    pub size: RockSize,
    /// How far it has tumbled at the end of this tick, in radians.
    pub angle: f64,
    /// The same at the end of the previous tick. `angle` and this are what
    /// [`lerp_angle`] takes; a rock that has just spawned has them equal.
    pub prev_angle: f64,
    /// Where it was at the end of the previous tick. `position` and this are
    /// what the renderer lerps between; a rock that has just spawned has them
    /// equal.
    pub prev_position: DVec3,
    /// Whether the wrap teleported it this tick. When set, the renderer draws
    /// it at `position` rather than interpolating — the previous position is a
    /// whole field away on the tick a rock crosses an edge.
    pub teleported: bool,
}

/// What the renderer needs for one bullet.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct BulletView {
    pub position: DVec3,
    /// Where it was at the end of the previous tick, for the same lerp.
    pub prev_position: DVec3,
    /// Whether the wrap teleported it this tick; the renderer snaps when set.
    pub teleported: bool,
}

/// A hit flash: where a rock died, how big it was, and how long it has left.
///
/// The visual twin of the explosion cue — `shatter` pushes both, and the
/// renderer draws this for [`FLASH_LIFE`] seconds at the dead rock's position
/// and radius, swapping frames halfway through. In the simulation rather than
/// on the facade because a replay must reproduce the picture as well as the
/// score.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Flash {
    /// Where the rock was when it shattered.
    pub position: DVec3,
    /// The size of the rock that died, so the flash covers it.
    pub size: RockSize,
    /// Seconds left before the flash goes out.
    pub life: f64,
}

/// The mutable game state the server-side module owns.
///
/// **Output-only, as far as [`Game`] is concerned.** The facade reads the
/// results out of here after each server tick; what the player asked for goes
/// the other way over the wire, not through this cell. `AsteroidsModule` is the
/// only thing that mutates any of it, and it only ever does so from inside a
/// server tick.
#[derive(Debug)]
struct GameLogic {
    ship: Entity,
    state: GameState,

    /// The ship's heading, in radians, with 0 pointing along +Y.
    ///
    /// **Game state, not physics state.** `crcbl-phys` has no angular velocity
    /// and no torque; this is integrated by hand in [`turn_ship`] and written
    /// into the [`Transform`] that [`ThrustForce::world_force`] then reads.
    heading: f64,
    /// The heading at the end of the previous tick, for [`lerp_angle`].
    ///
    /// Reset alongside [`Self::heading`] wherever the heading is *assigned*
    /// rather than integrated — [`place_ship`] — because a respawn that put the
    /// ship back facing up while this still held the heading it died on would
    /// make the new ship spin through the difference over one tick.
    prev_heading: f64,
    ship_pos: DVec3,
    /// Where the ship was at the end of the previous tick. The position half
    /// of [`Self::prev_heading`]: unlike the angles it is captured at the top
    /// of [`run_tick`] rather than per-writer, but from the *mirror* — physics
    /// does not touch `ship_pos`, so at tick entry it still holds last tick's
    /// position, which is exactly what `prev` must be.
    ///
    /// Reset alongside [`Self::ship_pos`] wherever the position is *assigned*
    /// rather than integrated — [`place_ship`] — for the same reason
    /// [`Self::prev_heading`] is: a respawn must not be drawn sweeping in from
    /// where the old ship died.
    ship_prev_pos: DVec3,
    /// Whether the wrap teleported the ship this tick. The renderer snaps to
    /// `ship_pos` when set, rather than lerping across the field. Set by
    /// [`wrap_everything`], cleared at the top of [`run_tick`].
    ship_teleported: bool,
    ship_vel: DVec3,
    /// Whether the ship is on the field. `false` between a death and a respawn.
    ship_alive: bool,
    /// Seconds since the ship was destroyed, while it is waiting to return.
    respawn_timer: f64,
    /// Seconds until the next shot is allowed.
    fire_timer: f64,
    /// Whether the ship is under power this tick.
    ///
    /// A fact about the ship, not about the speakers: it is `intent.thrust`
    /// narrowed to the ticks where the engine can actually fire. The facade
    /// reads it and hands it to `crate::audio::Audio::set_thrust`, which is
    /// where every decision about *how a held sound is made* now lives.
    ///
    /// This replaced a countdown that re-fired a one-shot cue every
    /// `THRUST_CUE_PERIOD` seconds, back when the crate had no looping voice.
    /// That timer was accumulating tick state, so an audio detail was part of
    /// what a replay had to reproduce; this is a bool derived from the same
    /// tick's input and carries no history.
    thrusting: bool,

    rocks: Vec<Rock>,
    bullets: Vec<Bullet>,

    score: u32,
    lives: u32,
    /// Which wave is on the field. Zero-based; the HUD adds one.
    wave: u32,

    /// The seed the whole game was started with. The board actually in play is
    /// [`board_seed`] of this and [`Self::runs`].
    seed: u64,
    /// How many games have been started. Simulation state, so a replay meets
    /// the same boards in the same order.
    runs: u32,
    /// Every rock ever spawned by a split, counted so [`split_velocity`] has an
    /// index. Never reset within a game, so two splits never draw the same
    /// number.
    spawn_counter: u64,

    /// How many rocks have ever been put on the field, and how many bullets
    /// have ever left the gun.
    ///
    /// **Instrumentation, not mechanism** — nothing reads them to decide
    /// anything. They exist because the leak test's whole claim is "this ran a
    /// lot of churn and leaked nothing", and without a count of the churn the
    /// second half is true of a game that did nothing at all.
    rocks_spawned: u64,
    bullets_fired: u64,

    /// Live views for the renderer, refilled rather than rebuilt so a
    /// steady-state tick does not allocate.
    rock_views: Vec<RockView>,
    bullet_views: Vec<BulletView>,

    /// Scratch space for the per-tick sweeps, kept here for the same reason.
    scratch_entities: Vec<Entity>,

    /// Scratch for the bullet sweep's hit list, kept here for the same reason.
    scratch_hits: Vec<(usize, Entity)>,

    /// Cues raised this tick: `(sound id, world x, world y)`. Drained by the
    /// facade, which owns the output stream — audio does not belong on the
    /// simulation's side of the seam, and a frame that ran two ticks must play
    /// both of their cues rather than only the last one's.
    cues: Vec<(u32, f64, f64)>,

    /// Hit flashes waiting to be drawn, drained by the renderer and aged in
    /// [`run_tick`]. The visual twin of [`Self::cues`]: `shatter` pushes both,
    /// and both live in the simulation so a replay reproduces the picture as
    /// well as the score.
    flashes: Vec<Flash>,

    /// The engine, as a force model rather than as a number. See
    /// [`ThrustForce::world_force`], which exists for exactly this case: a game
    /// that thrusts one entity among a field of rocks cannot use a pipeline
    /// provider, because a provider applies to every body.
    thrust: ThrustForce,

    /// The coast, likewise. [`DampingForce::world_force`] carries the `mass/dt`
    /// cap with it — the thing that stops a coarse tick rate from over-damping
    /// past zero and flying the ship backwards — so this game no longer keeps
    /// its own copy of that arithmetic.
    ///
    /// [`DampingForce::world_force`]: crcbl::phys::DampingForce::world_force
    damping: DampingForce,

    /// Ticks the module has actually run. The facade asserts this advances by
    /// exactly one per [`Game::tick`].
    ticks: u64,
}

impl GameLogic {
    /// The seed of the board in play.
    fn board(&self) -> u64 {
        board_seed(self.seed, self.runs)
    }
}

// ---------------------------------------------------------------------------
// The module
// ---------------------------------------------------------------------------

/// Per-tick game logic, run by the server after the ECS physics schedule.
///
/// `register` is empty for the same reason breakout's and flappy's are:
/// `Server::set_module` does not call it, and the physics system is registered
/// on the world in [`Game::new`] before the server is built.
struct AsteroidsModule {
    shared: Arc<Mutex<GameLogic>>,
}

impl std::fmt::Debug for AsteroidsModule {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("AsteroidsModule").finish_non_exhaustive()
    }
}

impl GameModule for AsteroidsModule {
    fn name(&self) -> &str {
        "asteroids"
    }

    fn register(&self, _world: &mut World) {}

    fn tick(&mut self, world: &mut World, inputs: ClientInputs<'_>) {
        let mut logic = lock(&self.shared);
        run_tick(&mut logic, world, Intent::from_inputs(inputs));
    }
}

/// A poisoned mutex here means a previous tick panicked. The game state is plain
/// data with no invariant a panic could have half-broken, so recovering the
/// guard is strictly better than taking the process down a second time.
fn lock(shared: &Mutex<GameLogic>) -> MutexGuard<'_, GameLogic> {
    shared.lock().unwrap_or_else(|e| e.into_inner())
}

/// One tick of asteroids, inside the server's tick, after physics has stepped.
///
/// **The order is load-bearing**, and the two places it is are:
///
/// * The bullet sweeps run **before** the wrap. A sweep is `prev → cur`, and a
///   bullet wrapped first would be swept across the whole field from the edge it
///   left to the edge it arrived at, hitting everything on the way.
/// * The wrap runs **before** the overlap test, so the ship and the rocks are
///   compared in one frame rather than one of each.
///
/// `intent` is what the client sent for this tick, decoded from the bytes that
/// arrived over the transport — see [`Intent::from_inputs`]. A tick nothing
/// arrived for runs on [`Intent::default`], which is a player holding nothing.
fn run_tick(logic: &mut GameLogic, world: &mut World, intent: Intent) {
    logic.ticks += 1;
    let dt = world.tick_dt();

    // Every angle the renderer interpolates is `(previous, current)`, and the
    // previous one is fixed here — **before** anything this tick can change a
    // heading or a tumble. Doing it per-writer instead would mean every future
    // caller of `turn_ship` remembering to, which is the shape of bug that only
    // shows up as a one-frame flick at a frame rate nobody tests at.
    logic.prev_heading = logic.heading;
    for rock in &mut logic.rocks {
        rock.prev_angle = rock.angle;
    }

    // The ship's position half of the same capture, from the mirror: physics
    // stepped *before* this function, so the ship's physics transform already
    // holds this tick's position — but `logic.ship_pos` is only refreshed by
    // `read_ship` below, so it still holds last tick's, which is exactly what
    // `prev` must be. The flag starts cleared; the wrap may set it.
    logic.ship_prev_pos = logic.ship_pos;
    logic.ship_teleported = false;

    // The integrator has just moved the ship, and everything below places
    // things relative to where it is *now* — the muzzle a shot leaves from, the
    // sphere the overlap query is centred on. Reading it at the end of the last
    // tick instead would put both a tick behind, which at terminal speed is a
    // quarter of a unit of lag on the gun.
    read_ship(logic, world);

    match logic.state {
        _ if intent.restart => restart(logic, world),
        GameState::WaitingToStart if intent.fire => logic.state = GameState::Playing,
        GameState::GameOver if intent.fire => restart(logic, world),
        _ => {}
    }

    if logic.state == GameState::Playing && logic.ship_alive {
        turn_ship(logic, world, intent, dt);
        drive_ship(logic, world, intent, dt);
    } else {
        // The cooldown still runs down while the ship is gone, so a respawned
        // ship can shoot immediately rather than inheriting a stale timer.
        logic.fire_timer = (logic.fire_timer - dt).max(0.0);
        // Nothing is burning on the title screen or behind the game-over panel,
        // however hard the key is being held. `drive_ship` sets the other half.
        logic.thrusting = false;
    }

    // Before the sweeps, so a rock destroyed this tick is never advanced past
    // the frame it was drawn at, and unconditional on the game's state: a rock
    // keeps turning on the title screen and behind the game-over panel, which is
    // what makes both look like a game rather than a screenshot.
    tumble_rocks(logic, dt);

    // The bullets are the other way round — gated, because a bullet is a score
    // carrier and the score must freeze the moment the game does. A leftover
    // bullet kept sweeping behind the game-over panel would shatter rocks and
    // add score that `best.raise`, which runs only on the edge into `GameOver`,
    // never saw. `shatter` doubles the guard on its own score line.
    if logic.state == GameState::Playing {
        sweep_bullets(logic, world, dt);
    }

    // **After the sweep, which is the whole point.** A sweep reconstructs where
    // a bullet was as `position - velocity * dt`, and for one created *this*
    // tick that point is a whole step behind the muzzle — through the ship's own
    // hull and out the other side, so a shot could hit a rock sitting behind the
    // ship on the tick it left the gun. Firing here makes a bullet's first sweep
    // its first real step, with `start` exactly at the muzzle. `apps/horde`
    // already did it this way round; this game did not, and
    // `a_bullet_is_never_swept_over_ground_it_did_not_travel` is what holds it.
    if logic.state == GameState::Playing && logic.ship_alive {
        fire(logic, world, intent, dt);
    }

    wrap_everything(logic, world);
    // Again, because the wrap is a teleport: a ship that just crossed an edge is
    // a whole field away from where it was three lines ago, and `check_ship`
    // below centres its overlap query on this value.
    read_ship(logic, world);
    expire_bullets(logic, world, dt);
    // The flash is the cue's visual twin, aged here the way `expire_bullets`
    // ages a bullet — and it is the whole pass, because a flash has no entity
    // to despawn with.
    logic.flashes.retain_mut(|flash| {
        flash.life -= dt;
        flash.life > 0.0
    });

    if logic.state == GameState::Playing {
        if logic.ship_alive {
            check_ship(logic, world);
        } else {
            respawn(logic, world, dt);
        }
        advance_wave(logic, world);
    }

    refresh_views(logic, world);
}

// ---------------------------------------------------------------------------
// The ship
// ---------------------------------------------------------------------------

/// Integrates the ship's heading and writes it into the transform.
///
/// By hand, because `crcbl-phys` has no rotational dynamics: `RigidBody` carries
/// `velocity` and `force_accum` and nothing angular, and `SemiImplicitEuler`
/// steps position only. That is the right answer for this game — a turn rate is
/// a constant here, not a physical response — and it is recorded in
/// `docs/backlog.md` as what the crate still owes a game where it is not.
fn turn_ship(logic: &mut GameLogic, world: &mut World, intent: Intent, dt: f64) {
    let turn = f64::from(i8::from(intent.left) - i8::from(intent.right));
    if turn == 0.0 {
        return;
    }
    logic.heading = (logic.heading + turn * SHIP_TURN_RATE * dt).rem_euclid(std::f64::consts::TAU);
    let ship = logic.ship;
    let rotation = DQuat::from_rotation_z(logic.heading);
    with_physics(world, |phys| {
        if let Some(transform) = phys.transform(ship).copied() {
            phys.set_transform(ship, Transform::new(transform.position, rotation));
        }
    });
}

/// Applies the engine and the coast.
///
/// Both go through [`PhysicsSystem::apply_force`] rather than through a force
/// provider, because a provider is global and would push every rock on the
/// field. [`ThrustForce::world_force`] exists for this; **the damping has no
/// equivalent**, so the `-k·v` below is a hand copy of [`DampingForce`]'s model,
/// clamp and all. That asymmetry is a finding, recorded in `docs/backlog.md`.
///
/// [`DampingForce`]: crcbl::phys::DampingForce
fn drive_ship(logic: &mut GameLogic, world: &mut World, intent: Intent, dt: f64) {
    let ship = logic.ship;
    let thrust = logic.thrust;
    let damping = logic.damping;
    with_physics(world, |phys| {
        let Some(transform) = phys.transform(ship).copied() else {
            return;
        };
        if intent.thrust {
            phys.apply_force(ship, thrust.world_force(&transform));
        }
        if let Some(body) = phys.body(ship).copied() {
            phys.apply_force(ship, damping.world_force(body.velocity, body.mass, dt));
        }
    });
    // Recorded, not sounded. The engine is a held cue and the facade owns it;
    // see `GameLogic::thrusting`.
    logic.thrusting = intent.thrust;
}

/// Fires, if the trigger went down, the magazine has room and the cooldown has
/// run out.
fn fire(logic: &mut GameLogic, world: &mut World, intent: Intent, dt: f64) {
    logic.fire_timer = (logic.fire_timer - dt).max(0.0);
    if !intent.fire || logic.fire_timer > 0.0 || logic.bullets.len() >= MAX_BULLETS {
        return;
    }
    logic.fire_timer = FIRE_COOLDOWN;

    let direction = heading_vector(logic.heading);
    let position = wrap_position(logic.ship_pos + direction * MUZZLE_OFFSET);
    let velocity = logic.ship_vel + direction * BULLET_SPEED;

    let entity = world.spawn();
    let transform = Transform::from_position(position);
    with_physics(world, |phys| {
        let mut body = RigidBody::new_dynamic(1.0);
        body.velocity = velocity;
        phys.set_body(entity, body);
        phys.set_transform(entity, transform);
    });
    logic.bullets.push(Bullet {
        entity,
        life: BULLET_LIFE,
        // Equal, the same convention as `prev_angle` in `spawn_rock`: the
        // first frame a bullet is on screen interpolates to itself.
        prev_position: position,
        teleported: false,
    });
    logic.bullets_fired += 1;
    // At the muzzle, not at the ship's centre: the two are a fifth of a unit
    // apart on a 32-unit field, which is inaudible — the point is that the cue
    // is raised where the *event* is, and the event is a bullet appearing.
    logic
        .cues
        .push((crate::audio::SOUND_SHOT, position.x, position.y));
}

/// The unit vector a heading of `radians` points along. Zero is +Y.
#[must_use]
pub fn heading_vector(radians: f64) -> DVec3 {
    DVec3::new(-radians.sin(), radians.cos(), 0.0)
}

/// Brings an angle into `(-π, π]`.
///
/// Public because it is half of [`lerp_angle`] and because the tests' autopilot
/// needs the same arithmetic to steer with — it had its own copy, which is one
/// place for the two to disagree about what "the short way" means.
#[must_use]
pub fn wrap_to_pi(angle: f64) -> f64 {
    let wrapped = angle.rem_euclid(std::f64::consts::TAU);
    if wrapped > std::f64::consts::PI {
        wrapped - std::f64::consts::TAU
    } else {
        wrapped
    }
}

/// Interpolates from angle `from` to angle `to`, **the short way round**, and
/// returns the result in `(-π, π]`.
///
/// # Why a game with no angular velocity still needs this
///
/// Every angle this game renders — the ship's heading, every rock's tumble — is
/// integrated once per *tick*, at [`DEFAULT_TICK_HZ`]. A window runs at whatever
/// rate the compositor gives it, and on any rate that is not the tick rate a
/// frame lands between two ticks. Drawing the newer of the two angles on both
/// frames is a stutter: the ship turns in sixtieths of a revolution at 144 Hz
/// instead of turning smoothly, and it is worst on exactly the thing this sample
/// exists to show. So the renderer lerps, with the frame clock's alpha — the
/// same shape `Client::interpolate` uses for positions, which this game does not
/// go through because its positions **wrap**. See [`RenderState`].
///
/// # Angles wrap, and that is the whole of the difficulty
///
/// `lerp(350°, 10°, 0.5)` is 180° — the ship spins right round the wrong way,
/// once, on the frame after it crosses the seam, and the seam is crossed
/// constantly because `turn_ship` keeps the heading in `[0, τ)`. Interpolating
/// the *difference* brought into `(-π, π]` takes the 20° arc instead of the 340°
/// one, so the value at 0.5 is 0° and not 180°.
///
/// A position wrap is the opposite case and is why this is not applied to
/// positions: a rock crossing the field's edge really is somewhere else, and
/// "the short way" would slide it back across the whole field. An angle wrap is
/// not a discontinuity at all — τ is the identity — which is what makes the
/// short way *correct* here rather than merely nicer.
///
/// `alpha` is clamped, so a caller that hands over an out-of-range value gets an
/// endpoint rather than an extrapolation.
#[must_use]
pub fn lerp_angle(from: f64, to: f64, alpha: f64) -> f64 {
    wrap_to_pi(from + wrap_to_pi(to - from) * alpha.clamp(0.0, 1.0))
}

/// Whether the ship is touching a rock, and what happens if it is.
///
/// A sphere overlap against the broadphase, which is what the plan asks for.
/// The [`crcbl::phys::ShapeHit`] each result carries is **discarded**: it is
/// fabricated (`t: 0.0`, normal `+Y`, `started_inside: true` for every result,
/// recorded in `docs/backlog.md`), and all this query is asked is *whether*
/// anything is there. `PhysicsWorld::overlap_sphere` underneath is honest but
/// returns `ColliderId`s, and the entity mapping lives only on the system.
///
/// # The ship is not in the broadphase, and this is why
///
/// The ship is the *subject* of every overlap test in this game and the
/// *target* of none: bullets are excluded from it deliberately, and the only
/// other query — [`respawn`]'s clear-centre check — runs while the ship is
/// destroyed. A collider for it would therefore be a leaf in the tree that no
/// query is allowed to return, and every one of them would have needed
/// filtering back out by entity id.
///
/// That falls out of the shape of the API rather than of this game:
/// [`PhysicsSystem::overlap_sphere`] takes a free centre and radius, so an
/// entity that only ever *asks* has no reason to be in the tree. A crate-level
/// "what does entity E overlap, excluding these" would change the answer.
/// Recorded in `docs/backlog.md`.
fn check_ship(logic: &mut GameLogic, world: &mut World) {
    let centre = logic.ship_pos;
    let touching = with_physics(world, |phys| {
        !phys.overlap_sphere(centre, SHIP_RADIUS).is_empty()
    })
    .unwrap_or(false);
    if touching {
        destroy_ship(logic, world);
    }
}

/// Takes the ship off the field and spends a life.
fn destroy_ship(logic: &mut GameLogic, world: &mut World) {
    let ship = logic.ship;
    logic.ship_alive = false;
    logic.respawn_timer = 0.0;
    logic.lives = logic.lives.saturating_sub(1);
    // The engine dies with the ship, in the same tick. `run_tick`'s else branch
    // only runs on the *next* one, which is a tick of engine over the wreck.
    logic.thrusting = false;
    // **Kinematic, not merely stopped.** Zeroing the velocity leaves the body
    // dynamic, so the next tick's damping and thrust act on a wreck — and the
    // wreck keeps drifting across the field behind the game-over screen.
    with_physics(world, |phys| {
        phys.set_body(ship, RigidBody::new_kinematic());
    });
    // The same cue a rock gets, where the ship was. Deliberately not a fourth
    // sound: the arcade original does not have one either, and the wreck is the
    // player's own — hearing it at their own position is the information.
    logic.cues.push((
        crate::audio::SOUND_EXPLOSION,
        logic.ship_pos.x,
        logic.ship_pos.y,
    ));
    if logic.lives == 0 {
        logic.state = GameState::GameOver;
        crcbl::log::info!(
            "game over on wave {} with {} points",
            logic.wave + 1,
            logic.score
        );
    }
}

/// Counts down to the ship's return, and returns it when the middle is clear.
fn respawn(logic: &mut GameLogic, world: &mut World, dt: f64) {
    logic.respawn_timer += dt;
    if logic.respawn_timer < RESPAWN_DELAY {
        return;
    }
    let clear = with_physics(world, |phys| {
        phys.overlap_sphere(DVec3::ZERO, RESPAWN_CLEAR_RADIUS)
            .is_empty()
    })
    .unwrap_or(true);
    if !clear && logic.respawn_timer < RESPAWN_MAX_WAIT {
        return;
    }
    place_ship(logic, world, DVec3::ZERO, 0.0, DVec3::ZERO);
}

/// Puts the ship on the field, stationary and facing up.
fn place_ship(
    logic: &mut GameLogic,
    world: &mut World,
    position: DVec3,
    heading: f64,
    velocity: DVec3,
) {
    let ship = logic.ship;
    logic.heading = heading;
    // See `GameLogic::prev_heading`: an assignment, not an integration, so the
    // renderer must not interpolate through it.
    logic.prev_heading = heading;
    logic.ship_pos = position;
    // And the position half of the same rule: a respawn that left `ship_prev_pos`
    // holding where the old ship died would be drawn sweeping the new one in
    // from there over one tick.
    logic.ship_prev_pos = position;
    logic.ship_teleported = false;
    logic.ship_vel = velocity;
    logic.ship_alive = true;
    logic.respawn_timer = 0.0;
    let transform = Transform::new(position, DQuat::from_rotation_z(heading));
    with_physics(world, |phys| {
        let mut body = RigidBody::new_dynamic(SHIP_MASS);
        body.velocity = velocity;
        phys.set_body(ship, body);
        phys.set_transform(ship, transform);
    });
}

// ---------------------------------------------------------------------------
// Bullets
// ---------------------------------------------------------------------------

/// Sweeps every bullet along the path it took this tick and resolves what it
/// hit.
///
/// **This is the "never miss at any speed" half of the plan, and it has a name
/// now.** [`PhysicsSystem::sweep_body`] reads the bullet's own body and
/// reconstructs the segment — from where it was (`position − velocity·dt`) to
/// where it is (`position`) — and this game is its first consumer; the
/// hand-written `Segment` this function used to build is gone.
///
/// A bullet therefore has **no collider**. It is a query, not a body in the
/// broadphase: nothing in this game ever asks what a bullet is touching, so a
/// collider would be an insert and a remove per bullet per tick buying nothing
/// — and bullets would start stopping each other. Hitting itself is no longer
/// part of that argument: [`PhysicsSystem::sweep_body`] holds the swept
/// entity's own collider out of the candidate set.
///
/// Nor does the ship carry one — [`check_ship`] overlaps a free centre — so a
/// shot cannot report hitting the hull it left. **Every collider in the world
/// is a rock**, which is what makes the leak test's collider count an equality
/// rather than a bound.
fn sweep_bullets(logic: &mut GameLogic, world: &mut World, dt: f64) {
    if logic.bullets.is_empty() {
        return;
    }

    // `(bullet index, rock entity)` for everything that connected this tick.
    // Hoisted into the game state and taken out for the round trip, the way
    // `rock_views` are, so a steady-state tick allocates nothing — the buffer
    // grows back into the capacity it already has. The bullets are borrowed,
    // not cloned, for the same reason: the closure only reads them.
    let mut hits = std::mem::take(&mut logic.scratch_hits);
    hits.clear();
    with_physics(world, |phys| {
        for (index, bullet) in logic.bullets.iter().enumerate() {
            if let Some((entity, _hit)) = phys.sweep_body(bullet.entity, dt, BULLET_RADIUS) {
                hits.push((index, entity));
            }
        }
    });

    // Highest index first, so removing one bullet does not move the next.
    for &(index, hit) in hits.iter().rev() {
        let Some(bullet) = logic.bullets.get(index).copied() else {
            continue;
        };
        // Two bullets can reach the same rock on the same tick. The first
        // splits it; the second finds an entity that is no longer a rock and is
        // spent without scoring, which is the arcade's behaviour and, more to
        // the point, is not a double score.
        let rock = logic.rocks.iter().position(|rock| rock.entity == hit);
        despawn_bullet(world, bullet);
        logic.bullets.remove(index);
        if let Some(rock) = rock {
            shatter(logic, world, rock);
        }
    }
    logic.scratch_hits = hits;
}

/// Ages every bullet and destroys the ones that have run out.
fn expire_bullets(logic: &mut GameLogic, world: &mut World, dt: f64) {
    let mut dead = std::mem::take(&mut logic.scratch_entities);
    dead.clear();
    logic.bullets.retain_mut(|bullet| {
        bullet.life -= dt;
        if bullet.life > 0.0 {
            return true;
        }
        dead.push(bullet.entity);
        false
    });
    for entity in dead.drain(..) {
        // The position pair is dead along with it; a zero is as good as any.
        despawn_bullet(
            world,
            Bullet {
                entity,
                life: 0.0,
                prev_position: DVec3::ZERO,
                teleported: false,
            },
        );
    }
    logic.scratch_entities = dead;
}

/// Destroys a bullet, in the physics world and in the ECS.
///
/// Both, and in that order — the failure mode a game with this much churn would
/// produce a hundred times a minute is a body left behind when its entity goes.
fn despawn_bullet(world: &mut World, bullet: Bullet) {
    with_physics(world, |phys| phys.remove_entity(bullet.entity));
    world.despawn(bullet.entity);
}

// ---------------------------------------------------------------------------
// Rocks
// ---------------------------------------------------------------------------

/// Breaks rock `index` into its children, scores it and destroys it.
fn shatter(logic: &mut GameLogic, world: &mut World, index: usize) {
    let rock = logic.rocks.swap_remove(index);
    // **State-gated, not merely sweep-gated.** The only caller sweeps only
    // while `Playing`, but the invariant "no score after game over" belongs on
    // the score line itself, so a future caller that shatters behind the panel
    // breaks the rock and plays the cue but scores nothing.
    if logic.state == GameState::Playing {
        logic.score += rock.size.score();
    }

    let (position, direction) = with_physics(world, |phys| {
        let position = phys
            .transform(rock.entity)
            .map_or(DVec3::ZERO, |t| t.position);
        let direction = phys
            .body(rock.entity)
            .map_or(DVec3::Y, |b| b.velocity.normalize_or(DVec3::Y));
        (position, direction)
    })
    .unwrap_or((DVec3::ZERO, DVec3::Y));

    with_physics(world, |phys| phys.remove_entity(rock.entity));
    world.despawn(rock.entity);
    logic
        .cues
        .push((crate::audio::SOUND_EXPLOSION, position.x, position.y));
    // The flash is the cue's visual twin: pushed on the same tick, at the
    // same place, drawn for [`FLASH_LIFE`] at the size of the rock that died.
    logic.flashes.push(Flash {
        position,
        size: rock.size,
        life: FLASH_LIFE,
    });

    let Some(child) = rock.size.child() else {
        return;
    };
    let seed = logic.board();
    for which in 0..SPLIT_CHILDREN {
        let counter = logic.spawn_counter;
        logic.spawn_counter += 1;
        let velocity = split_velocity(seed, counter, which, direction, child);
        spawn_rock(logic, world, child, position, velocity);
    }
}

/// Turns every rock on the field by one tick's worth of its own spin.
///
/// Kept in `[0, τ)`, as [`turn_ship`] keeps the heading, so the value cannot
/// drift into the range where an `f64`'s spacing is coarser than the turn — and
/// so the wrap the renderer's [`lerp_angle`] has to survive is crossed by every
/// rock, several times a minute, rather than being a case only a test reaches.
fn tumble_rocks(logic: &mut GameLogic, dt: f64) {
    for rock in &mut logic.rocks {
        rock.angle = (rock.angle + rock.spin * dt).rem_euclid(std::f64::consts::TAU);
    }
}

/// Puts one rock on the field.
fn spawn_rock(
    logic: &mut GameLogic,
    world: &mut World,
    size: RockSize,
    position: DVec3,
    velocity: DVec3,
) -> Entity {
    let entity = world.spawn();
    let transform = Transform::from_position(wrap_position(position));
    with_physics(world, |phys| {
        let mut body = RigidBody::new_dynamic(1.0);
        body.velocity = velocity;
        phys.set_body(entity, body);
        phys.set_collider(entity, &size.collider(), &transform);
    });
    // Where it starts and which way it turns, from the board's seed and this
    // rock's place in the spawn order — a pure function of the same two things
    // every other random-looking number in this game comes from, so a replay
    // tumbles the same way. A fixed starting angle would have every rock of a
    // fresh wave in the same attitude, which reads as a stamp rather than a
    // field.
    let spawned = logic.rocks_spawned;
    let seed = logic.board();
    let angle = hash_unit(seed, tumble_index(spawned, 0)) * std::f64::consts::TAU;
    let spin = if hash_unit(seed, tumble_index(spawned, 1)) < 0.5 {
        -size.spin()
    } else {
        size.spin()
    };
    logic.rocks.push(Rock {
        entity,
        size,
        angle,
        // Equal, so the first frame a rock is on screen interpolates to itself
        // rather than sweeping in from wherever the previous tick's value was.
        prev_angle: angle,
        spin,
        // Same convention for the position half of the pair, and a fresh rock
        // has not teleported. The position is the wrapped one, matching the
        // transform above: a wave deals rocks onto the field's *border*, and a
        // border point wraps to the opposite edge, so the unwrapped spawn
        // point would not be where the rock is.
        prev_position: wrap_position(position),
        teleported: false,
    });
    logic.rocks_spawned += 1;
    entity
}

/// Deals the next wave once the field is clear.
fn advance_wave(logic: &mut GameLogic, world: &mut World) {
    if !logic.rocks.is_empty() {
        return;
    }
    logic.wave += 1;
    deal_wave(logic, world);
    crcbl::log::info!(
        "wave {} — {} rocks, {} points",
        logic.wave + 1,
        logic.rocks.len(),
        logic.score
    );
}

/// Puts the current wave's rocks on the field.
fn deal_wave(logic: &mut GameLogic, world: &mut World) {
    let seed = logic.board();
    let wave = logic.wave;
    for rock in 0..wave_rocks(wave) {
        let position = wave_rock_position(seed, wave, rock);
        let velocity = wave_rock_velocity(seed, wave, rock);
        spawn_rock(logic, world, RockSize::Large, position, velocity);
    }
}

// ---------------------------------------------------------------------------
// The wrap
// ---------------------------------------------------------------------------

/// Brings everything that has left the field back in on the other side, and
/// flags every body the wrap actually moved, so the renderer snaps rather than
/// interpolates across the field on that tick.
fn wrap_everything(logic: &mut GameLogic, world: &mut World) {
    let ship = logic.ship;
    let ship_alive = logic.ship_alive;

    // Taken out and put back rather than borrowed through the guard: the
    // closure below needs the physics system *and* the two lists, and both
    // live behind the same `&mut logic`. The allocations survive the round
    // trip, which is the point of reusing them. Nothing spawns during the wrap
    // — `shatter` runs earlier in the tick — so putting the lists back
    // restores exactly what was taken.
    let mut rocks = std::mem::take(&mut logic.rocks);
    let mut bullets = std::mem::take(&mut logic.bullets);
    with_physics(world, |phys| {
        if ship_alive {
            // `None`: the ship carries no collider (see `check_ship`), so its
            // wrap has nothing to re-place in the tree. The rocks below are
            // where the teleport rule actually bites.
            if teleport_if_outside(phys, ship, None) {
                logic.ship_teleported = true;
            }
        }
        for rock in &mut rocks {
            if teleport_if_outside(phys, rock.entity, Some(&rock.size.collider())) {
                rock.teleported = true;
            }
        }
        for bullet in &mut bullets {
            if teleport_if_outside(phys, bullet.entity, None) {
                bullet.teleported = true;
            }
        }
    });
    logic.rocks = rocks;
    logic.bullets = bullets;
}

/// Wraps `entity` if it has left the field, and does nothing at all if it has
/// not. Returns whether it teleported.
#[must_use]
fn teleport_if_outside(
    phys: &mut PhysicsSystem,
    entity: Entity,
    collider: Option<&ColliderComponent>,
) -> bool {
    let Some(transform) = phys.transform(entity).copied() else {
        return false;
    };
    let wrapped = wrap_position(transform.position);
    if wrapped == transform.position {
        return false;
    }
    teleport(
        phys,
        entity,
        Transform::new(wrapped, transform.rotation),
        collider,
    );
    true
}

/// Moves `entity` to `transform` as a **teleport**: its collider comes out of
/// the broadphase and goes back in, rather than being refitted where it stood.
///
/// # The rule this sample had to pick
///
/// `docs/backlog.md` left the choice to whoever wrote the wrap, so: **a wrap is
/// a teleport, and a teleport is a remove-and-re-insert.** Uniformly, for
/// everything that is in the broadphase at all — which here is every rock, and
/// nothing else — with no distance threshold, because "did the position change
/// discontinuously" is exactly what a wrap knows and a threshold would only be
/// a guess at it.
///
/// [`PhysicsSystem::set_collider`] *is* that remove-and-re-insert: it drops the
/// old collider (bumping its generation and recycling its slot) and inserts a
/// fresh one, so the leaf is re-placed by the surface area heuristic instead of
/// staying where it was.
///
/// # What was and was not verified
///
/// The backlog's entry implies a wrapped body's *collisions* break. **They do
/// not**, and this was checked: `Bvh::update_aabb` refits every ancestor on the
/// way to the root, so a leaf dragged across the field leaves its ancestors as
/// conservative supersets — bigger than they should be, never smaller. A
/// superset prunes nothing, so `set_transform` still finds every overlap.
/// Swapping this call for `set_transform` leaves the whole of this crate's suite
/// green, which is the honest report of it.
///
/// What it costs instead is **tree quality**: ancestors stretched across a
/// 32 × 24 field make every query traverse subtrees it has no business in.
/// That is a cost this sample **cannot observe** — `PhysicsWorld` exposes no
/// `depth()`, no node count and no `&Bvh`, though `Bvh::depth` itself is public
/// — so the rule is chosen on the argument and not on a measurement. Recorded
/// in `docs/backlog.md` as the gap it is.
fn teleport(
    phys: &mut PhysicsSystem,
    entity: Entity,
    transform: Transform,
    collider: Option<&ColliderComponent>,
) {
    match collider {
        Some(component) => phys.set_collider(entity, component, &transform),
        // Nothing in the tree to re-place. `set_transform` is the whole of the
        // move for a body that is only ever a query.
        None => phys.set_transform(entity, transform),
    }
}

// ---------------------------------------------------------------------------
// Restart and read-back
// ---------------------------------------------------------------------------

/// Clears the field and deals a new game on a board that is not the one just
/// played.
fn restart(logic: &mut GameLogic, world: &mut World) {
    for rock in std::mem::take(&mut logic.rocks) {
        with_physics(world, |phys| phys.remove_entity(rock.entity));
        world.despawn(rock.entity);
    }
    for bullet in std::mem::take(&mut logic.bullets) {
        despawn_bullet(world, bullet);
    }
    // A flash is a fact about the field it happened on; one still burning from
    // the last game would smear over the new board.
    logic.flashes.clear();
    logic.runs = logic.runs.wrapping_add(1);
    logic.state = GameState::WaitingToStart;
    logic.score = 0;
    logic.lives = STARTING_LIVES;
    logic.wave = 0;
    logic.fire_timer = 0.0;
    logic.thrusting = false;
    logic.spawn_counter = 0;
    place_ship(logic, world, DVec3::ZERO, 0.0, DVec3::ZERO);
    deal_wave(logic, world);
}

/// Copies the authoritative state the renderer needs out of the physics world,
/// and advances the position half of each body's `(prev, cur)` pair.
fn refresh_views(logic: &mut GameLogic, world: &mut World) {
    let ship = logic.ship;

    // Taken out and put back rather than borrowed through the guard: the
    // closure below needs the physics system *and* these four lists, and all
    // live behind the same `&mut logic`. The allocations survive the round
    // trip, which is the point of reusing them. A steady-state tick allocates
    // nothing here.
    let mut rock_views = std::mem::take(&mut logic.rock_views);
    let mut bullet_views = std::mem::take(&mut logic.bullet_views);
    let mut rocks = std::mem::take(&mut logic.rocks);
    let mut bullets = std::mem::take(&mut logic.bullets);
    rock_views.clear();
    bullet_views.clear();
    let ship_state = with_physics(world, |phys| {
        for rock in &mut rocks {
            if let Some(transform) = phys.transform(rock.entity) {
                rock_views.push(RockView {
                    position: transform.position,
                    size: rock.size,
                    angle: rock.angle,
                    prev_angle: rock.prev_angle,
                    prev_position: rock.prev_position,
                    teleported: rock.teleported,
                });
                // Physics holds the positions, so the pair is advanced at the
                // *end* of the tick: the view just published carries last
                // tick's position, and the stored `prev` becomes this one.
                // A wrap is a one-tick fact, so the flag clears here too.
                rock.prev_position = transform.position;
                rock.teleported = false;
            }
        }
        for bullet in &mut bullets {
            if let Some(transform) = phys.transform(bullet.entity) {
                bullet_views.push(BulletView {
                    position: transform.position,
                    prev_position: bullet.prev_position,
                    teleported: bullet.teleported,
                });
                bullet.prev_position = transform.position;
                bullet.teleported = false;
            }
        }
        let position = phys.transform(ship).map(|t| t.position);
        let velocity = phys.body(ship).map(|b| b.velocity);
        position.zip(velocity)
    })
    .flatten();
    logic.rock_views = rock_views;
    logic.bullet_views = bullet_views;
    logic.rocks = rocks;
    logic.bullets = bullets;

    if let Some((position, velocity)) = ship_state {
        logic.ship_pos = position;
        logic.ship_vel = velocity;
        // `ship_prev_pos` is deliberately not touched: it was captured at the
        // top of `run_tick`, when the mirror still held last tick's position.
    }
}

/// Copies the ship's authoritative position and velocity out of the physics
/// world.
fn read_ship(logic: &mut GameLogic, world: &mut World) {
    let ship = logic.ship;
    let state = with_physics(world, |phys| {
        let position = phys.transform(ship).map(|t| t.position);
        let velocity = phys.body(ship).map(|b| b.velocity);
        position.zip(velocity)
    })
    .flatten();
    if let Some((position, velocity)) = state {
        logic.ship_pos = position;
        logic.ship_vel = velocity;
    }
}

/// Runs `f` against the world's physics system, if it has one.
fn with_physics<R>(world: &mut World, f: impl FnOnce(&mut PhysicsSystem) -> R) -> Option<R> {
    world.system_mut::<PhysicsSystem>().map(f)
}

// ---------------------------------------------------------------------------
// Game — the client-side facade
// ---------------------------------------------------------------------------

/// Everything the renderer needs for one frame, in world space.
///
/// Filled through [`Game::render_state`], which reuses the caller's allocations
/// — this game hands over a fresh rock and bullet list every frame forever.
///
/// # Angles and positions both come in pairs — and positions carry a teleport flag
///
/// Every rotation here is `(previous tick, this tick)`, and
/// `art::Scene::build` takes the frame clock's alpha and [`lerp_angle`]s
/// between them. Every position is the same pair, because this playfield
/// **wraps**: a rock's previous position and its current one are on opposite
/// sides of the field on the tick it crosses an edge, and interpolating between
/// them would fly it back across the whole screen. The pair alone cannot
/// express that, so each body carries `teleported`, set on the tick the wrap
/// moved it; the renderer draws a teleported body at its current position
/// rather than between the two. The flag is why positions could be
/// interpolated at all — without it, "lerp them too" would be a bug, not a
/// fix. An angle has no such case: a wrap of τ is the identity, which is
/// exactly why the angles could be interpolated first.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct RenderState {
    pub ship: DVec3,
    /// Where the ship was at the end of the previous tick, for the same lerp.
    pub ship_prev_pos: DVec3,
    /// Whether the wrap teleported the ship this tick; the renderer snaps when
    /// set.
    pub ship_teleported: bool,
    pub ship_heading: f64,
    /// The ship's heading at the end of the previous tick.
    pub ship_heading_prev: f64,
    pub ship_alive: bool,
    /// Whether the ship was under power on the tick just run.
    ///
    /// The renderer's half of `game::Game::thrusting`: picks the ship's flame
    /// frame in `art::Scene::build`. Kept off `RenderState` for as long as it
    /// was because the flame frame did not exist; the audio cue was already
    /// driven by the same bool on the game side.
    pub thrusting: bool,
    pub rocks: Vec<RockView>,
    pub bullets: Vec<BulletView>,
    /// The hit flashes still burning this frame.
    pub flashes: Vec<Flash>,
    pub score: u32,
    /// The best score this player has ever reached, across sessions.
    pub best: u32,
    pub lives: u32,
    /// Zero-based. The HUD shows `wave + 1`.
    pub wave: u32,
    pub state: Option<GameState>,
}

/// Asteroids' debug-panel section: what is on the field, and what it has cost
/// the entity pool.
///
/// **This is the churn sample, and these are the numbers the churn is judged
/// by.** `docs/plan/sample/02-asteroids.md`'s whole case is that a game which
/// spawns and destroys forever must not grow forever, and the soak test asserts
/// exactly the three counts at the bottom of this struct — until this section
/// existed they were assertable and not *watchable*, so a leak in a real
/// playthrough showed up as the process getting slower and nothing else.
///
/// [`FieldStats::entities`] must be read together with
/// [`FieldStats::pending_despawns`]: `crcbl::ecs::World::sweep` runs at the end
/// of `World::tick` and `crcbl-server` calls `GameModule::tick` after it, so
/// everything destroyed on a tick is still counted for one more. A panel showing
/// only the first would report a leak every time a rock shattered. See
/// [`Game::pending_despawns`].
///
/// The wave number is **not** here: the HUD already draws it, and a second copy
/// on screen is a number a reader has to reconcile rather than one they can
/// use.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct FieldStats {
    /// Large rocks on the field.
    pub large: usize,
    /// Medium rocks — what a large one breaks into.
    pub medium: usize,
    /// Small rocks, which break into nothing.
    pub small: usize,
    /// Bullets in the air.
    pub bullets: usize,
    /// Rocks ever put on the field, across every wave, split and restart.
    pub rocks_spawned: u64,
    /// Bullets ever fired.
    pub bullets_fired: u64,
    /// Entities the simulation is holding.
    pub entities: usize,
    /// Entities destroyed and not yet swept. See the type docs.
    pub pending_despawns: usize,
    /// Colliders the physics world is holding — a second, independent count,
    /// because a collider outliving its entity is an invisible wall that the
    /// entity count would say nothing about.
    pub colliders: usize,
}

impl crcbl::ui::DebugModule for FieldStats {
    fn debug_section(&self, section: &mut crcbl::ui::DebugSection) {
        section.set_title("field");
        section.row(
            "rocks",
            format_args!("{}/{}/{}", self.large, self.medium, self.small),
        );
        section.row("bullets", format_args!("{}", self.bullets));
        section.row("spawned", format_args!("{}", self.rocks_spawned));
        section.row("fired", format_args!("{}", self.bullets_fired));
        section.row("entities", format_args!("{}", self.entities));
        // "despawns" rather than "pending": the GPU's own section renders
        // `timings: pending` while a timestamp report is still in flight, and
        // two rows a panel apart that both say "pending" are two different
        // facts a reader has to tell apart by position.
        section.row("despawns", format_args!("{}", self.pending_despawns));
        section.row("colliders", format_args!("{}", self.colliders));
    }
}

pub struct Game {
    pub ship_entity: Entity,
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
    /// The output stream and the three cues. On the facade rather than in the
    /// simulation: the module runs inside the server's tick and must stay a pure
    /// function of its inputs, and an audio device is neither.
    pub audio: crate::audio::Audio,
    /// The best score, and where it is kept.
    pub best: crcbl::store::record::Record,
    /// Mirrors of the shared state, refreshed after each tick so the render and
    /// HUD paths never take the lock.
    pub state: GameState,
    pub score: u32,
    pub lives: u32,
    pub wave: u32,
    pub ship: DVec3,
    pub ship_velocity: DVec3,
    pub ship_heading: f64,
    /// The heading at the end of the tick before last, so a caller that reads
    /// these mirrors rather than [`Game::render_state`] can interpolate too.
    pub ship_heading_prev: f64,
    pub ship_alive: bool,
    /// Whether the ship was under power on the tick just run.
    ///
    /// Mirrored for the same reason the rest are, and read by [`Game::tick`]
    /// itself: it is what drives the held engine cue. See
    /// `GameLogic::thrusting`.
    pub thrusting: bool,
    prev_log_state: GameState,
}

impl std::fmt::Debug for Game {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Game")
            .field("ship_entity", &self.ship_entity)
            .field("state", &self.state)
            .field("score", &self.score)
            .field("lives", &self.lives)
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
    /// Builds the world, the physics system, the server and the client.
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
        Self::with_seed(headless, tick_hz, DEFAULT_SEED)
    }

    /// The same, on a named board.
    ///
    /// `seed` decides every rock of every wave of every run of this game — see
    /// [`hash_unit`] — so two games built with the same seed and fed the same
    /// input are the same game, which is what the determinism tests rest on.
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
    pub fn with_seed(headless: bool, tick_hz: u32, seed: u64) -> Result<Self, GameError> {
        assert!(tick_hz > 0, "tick rate must be positive");
        let mut world = World::new();

        // **No force providers at all.** A provider applies to every dynamic
        // body, and every dynamic body here except the ship is a rock or a
        // bullet, neither of which may be pushed or slowed by anything. The
        // ship's two forces are per-entity; see `drive_ship`.
        world.register_system(Box::new(PhysicsSystem::new()));

        let ship_entity = world.spawn();

        let mut action_map = ActionMap::new();
        for (name, keys) in [
            (ACTION_LEFT, vec![KeyCode::ArrowLeft, KeyCode::KeyA]),
            (ACTION_RIGHT, vec![KeyCode::ArrowRight, KeyCode::KeyD]),
            (ACTION_THRUST, vec![KeyCode::ArrowUp, KeyCode::KeyW]),
            (ACTION_FIRE, vec![KeyCode::Space]),
            (ACTION_RESTART, vec![KeyCode::KeyR]),
        ] {
            action_map.declare(ActionDecl {
                name: name.into(),
                kind: ActionKind::Button,
                bindings: keys.into_iter().map(Binding::Key).collect(),
            });
        }

        let shared = Arc::new(Mutex::new(GameLogic {
            ship: ship_entity,
            state: GameState::WaitingToStart,
            heading: 0.0,
            prev_heading: 0.0,
            ship_pos: DVec3::ZERO,
            ship_prev_pos: DVec3::ZERO,
            ship_teleported: false,
            ship_vel: DVec3::ZERO,
            ship_alive: false,
            respawn_timer: 0.0,
            fire_timer: 0.0,
            thrusting: false,
            rocks: Vec::new(),
            bullets: Vec::new(),
            score: 0,
            lives: STARTING_LIVES,
            wave: 0,
            seed,
            runs: 0,
            spawn_counter: 0,
            rocks_spawned: 0,
            bullets_fired: 0,
            rock_views: Vec::new(),
            bullet_views: Vec::new(),
            scratch_entities: Vec::new(),
            scratch_hits: Vec::new(),
            cues: Vec::new(),
            flashes: Vec::new(),
            thrust: ThrustForce::new(SHIP_THRUST, DVec3::Y),
            damping: DampingForce::new(SHIP_DAMPING),
            ticks: 0,
        }));

        let mut session = Loopback::new(
            world,
            Box::new(AsteroidsModule {
                shared: Arc::clone(&shared),
            }),
            tick_hz,
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
        // opening frame and the game starts on the opening tick.
        let sim_time = tick_period;
        session.client_mut().update(sim_time);
        session.server_mut().update(sim_time);
        session.client_mut().update(sim_time);
        if session.server().session_state() != crcbl::net::SessionState::Connected {
            return Err(GameError::Server(
                "the loopback session did not come up in its first tick".into(),
            ));
        }

        // **The board is dealt after that tick, not before it**, and this is
        // where this game differs from breakout. Breakout's handshake tick
        // costs the tick and nothing else because an unlaunched ball is pinned
        // and a paddle with no intent does not move; here a rock drifts and
        // tumbles on every tick whatever the state, so a board dealt first
        // would be a board one tick further along than the one the run is
        // supposed to open on. Dealt after, the opening frame is the wave as
        // `deal_wave` laid it out — which is what the golden pictures and what
        // `the_opening_board_is_where_the_dealer_put_it` pins.
        //
        // It still exists before the first tick of *play*: the player is
        // looking at the game while deciding whether to press anything, and an
        // empty screen that fills in on the first shot reads as a bug.
        {
            let mut logic = lock(&shared);
            let world = session.server_mut().world_mut();
            place_ship(&mut logic, world, DVec3::ZERO, 0.0, DVec3::ZERO);
            deal_wave(&mut logic, world);
            refresh_views(&mut logic, world);
        }

        let game = Self {
            ship_entity,
            action_map,
            session,
            shared,
            tick_period,
            sim_time,
            ticks_run: 0,
            pending_keys: Vec::new(),
            audio: crate::audio::Audio::new(headless),
            best: crate::best::open(headless),
            state: GameState::WaitingToStart,
            score: 0,
            lives: STARTING_LIVES,
            wave: 0,
            ship: DVec3::ZERO,
            ship_velocity: DVec3::ZERO,
            ship_heading: 0.0,
            ship_heading_prev: 0.0,
            ship_alive: true,
            thrusting: false,
            prev_log_state: GameState::WaitingToStart,
        };
        crcbl::log::info!(
            "sim: {tick_hz} Hz, {:.3} ms per tick, wave 1 of {} rocks",
            game.tick_dt_secs() * 1e3,
            wave_rocks(0),
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
            left: self.action_map.button_held(ACTION_LEFT),
            right: self.action_map.button_held(ACTION_RIGHT),
            thrust: self.action_map.button_held(ACTION_THRUST),
            fire: self.action_map.just_pressed(ACTION_FIRE),
            restart: self.action_map.just_pressed(ACTION_RESTART),
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
        // cues rather than only the last one's. The listener is the camera at
        // the origin; see `crate::audio`.
        let cues: Vec<_> = {
            let mut logic = lock(&self.shared);
            logic.cues.drain(..).collect()
        };
        for (id, x, y) in cues {
            self.audio.play_at(id, DVec3::new(x, y, 0.0));
        }

        let was = self.state;
        let ticks_after = {
            let logic = lock(&self.shared);
            self.state = logic.state;
            self.score = logic.score;
            self.lives = logic.lives;
            self.wave = logic.wave;
            self.ship = logic.ship_pos;
            self.ship_velocity = logic.ship_vel;
            self.ship_heading = logic.heading;
            self.ship_heading_prev = logic.prev_heading;
            self.ship_alive = logic.ship_alive;
            self.thrusting = logic.thrusting;
            logic.ticks
        };

        // **The engine, after the mirrors and outside the lock.** It is a held
        // cue: one looping voice that starts on the first burning tick, is
        // re-aimed at the ship on every tick after that, and is stopped the tick
        // the key comes up. `crate::audio::Audio::set_thrust` owns all of that —
        // the simulation only says whether the ship is burning.
        self.audio.set_thrust(self.thrusting, self.ship);
        debug_assert_eq!(
            ticks_after,
            ticks_before + u64::from(server_ticks),
            "game logic must run exactly once per physics tick",
        );

        // **On the edge into `GameOver`, not every tick.** The score is frozen
        // by then — the ship is gone and the last rock a bullet reached has
        // already been counted — and an `update` per tick would write the file
        // sixty times a second for as long as the panel is up.
        if self.state == GameState::GameOver && was != GameState::GameOver {
            self.best.raise(self.score);
        }

        let state_changed = self.state != self.prev_log_state;
        self.prev_log_state = self.state;
        // **Every sixty ticks, which is a second of simulated time, and the same
        // cadence breakout and flappy use.** `web/tools/browser-e2e.mjs` watches
        // for this heartbeat to tell a paused demo from a running one, and its
        // window is written against a second rather than the two this used to
        // take.
        if state_changed || self.ticks_run.is_multiple_of(60) {
            // `rock x` is here for that gate too, and it is the honest choice:
            // it is the one number in this game that moves on its own. The ship
            // is stationary unless the player thrusts, the score changes only on
            // a hit, and a HUD whose numbers can all stand still cannot show
            // that the simulation is advancing.
            let rock_x = self
                .rocks()
                .first()
                .map_or(f64::NAN, |rock| rock.position.x);
            crcbl::log::info!(
                "[HUD] {:?}  score: {}  best: {}  lives: {}  wave: {}  rocks: {}  rock x: {:.2}",
                self.state,
                self.score,
                self.best.get(),
                self.lives,
                self.wave + 1,
                self.rock_count(),
                rock_x,
            );
        }
    }

    /// Everything the renderer draws, in world space.
    ///
    /// `out` is reused across frames so a steady-state frame does not allocate.
    pub fn render_state(&self, out: &mut RenderState) {
        let logic = lock(&self.shared);
        out.ship = logic.ship_pos;
        out.ship_prev_pos = logic.ship_prev_pos;
        out.ship_teleported = logic.ship_teleported;
        out.ship_heading = logic.heading;
        out.ship_heading_prev = logic.prev_heading;
        out.ship_alive = logic.ship_alive;
        out.thrusting = self.thrusting;
        out.rocks.clear();
        out.rocks.extend_from_slice(&logic.rock_views);
        out.bullets.clear();
        out.bullets.extend_from_slice(&logic.bullet_views);
        out.flashes.clear();
        out.flashes.extend_from_slice(&logic.flashes);
        out.score = logic.score;
        out.lives = logic.lives;
        out.wave = logic.wave;
        out.state = Some(logic.state);
        drop(logic);
        // Outside the lock: the best score is the facade's, not the
        // simulation's — a replay of the same script must not depend on what
        // some earlier session happened to score.
        out.best = self.best.get();
    }

    /// The board in play, for a caller that wants to name it — a bug report, a
    /// replay header, or a test.
    #[must_use]
    pub fn board_seed(&self) -> u64 {
        lock(&self.shared).board()
    }

    /// The rocks on the field right now.
    #[must_use]
    pub fn rocks(&self) -> Vec<RockView> {
        lock(&self.shared).rock_views.clone()
    }

    /// The bullets in the air right now.
    #[must_use]
    pub fn bullets(&self) -> Vec<BulletView> {
        lock(&self.shared).bullet_views.clone()
    }

    /// How many rocks are on the field.
    #[must_use]
    pub fn rock_count(&self) -> usize {
        lock(&self.shared).rocks.len()
    }

    /// How many bullets are in the air.
    #[must_use]
    pub fn bullet_count(&self) -> usize {
        lock(&self.shared).bullets.len()
    }

    /// How many rocks this game has ever put on the field, across every wave,
    /// split and restart.
    ///
    /// The denominator of the leak test: "nothing leaked" is a claim about a
    /// run that churned, and this is what says it churned.
    #[must_use]
    pub fn rocks_spawned(&self) -> u64 {
        lock(&self.shared).rocks_spawned
    }

    /// How many bullets this game has ever fired.
    #[must_use]
    pub fn bullets_fired(&self) -> u64 {
        lock(&self.shared).bullets_fired
    }

    /// How many entities the simulation is holding.
    ///
    /// The number this whole sample exists to watch: it spawns and destroys
    /// entities forever, and one that climbs is the failure the plan named.
    #[must_use]
    pub fn entity_count(&mut self) -> usize {
        self.session.server_mut().world_mut().entity_count()
    }

    /// How many entities are queued for destruction and not yet swept.
    ///
    /// **Zero between ticks, and the panel row exists to say so.**
    /// `crcbl_server::Server::tick` sweeps after the game module has run and
    /// before it serialises the snapshot, so nothing this game destroys
    /// survives the tick that destroyed it. A non-zero reading here means a
    /// despawn happened outside the tick — `Game::stage_rock`, the test
    /// fixture, is the only caller that does that — and [`Game::entity_count`]
    /// is correspondingly high until the next tick sweeps it.
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

    /// Everything [`FieldStats`] shows, read in one pass.
    ///
    /// Takes `&mut self` because three of the nine numbers are the server's
    /// world rather than the shared cell, which is why the panel is handed a
    /// snapshot taken on the frame's own draw rather than reading the game
    /// itself — `HostedGame::debug_sections` has only `&self`. Horde's
    /// `SceneStats` is the same arrangement.
    #[must_use]
    pub fn field_stats(&mut self) -> FieldStats {
        let entities = self.entity_count();
        let pending_despawns = self.pending_despawns();
        let colliders = self.collider_count();
        let logic = lock(&self.shared);
        let count = |size| logic.rocks.iter().filter(|rock| rock.size == size).count();
        FieldStats {
            large: count(RockSize::Large),
            medium: count(RockSize::Medium),
            small: count(RockSize::Small),
            bullets: logic.bullets.len(),
            rocks_spawned: logic.rocks_spawned,
            bullets_fired: logic.bullets_fired,
            entities,
            pending_despawns,
            colliders,
        }
    }

    /// Clears the field and places one rock, for a test that needs a known
    /// board rather than a dealt one.
    #[cfg(test)]
    pub fn stage_rock(&mut self, position: DVec3, velocity: DVec3, size: RockSize) -> Entity {
        let mut logic = lock(&self.shared);
        let world = self.session.server_mut().world_mut();
        for rock in std::mem::take(&mut logic.rocks) {
            with_physics(world, |phys| phys.remove_entity(rock.entity));
            world.despawn(rock.entity);
        }
        spawn_rock(&mut logic, world, size, position, velocity)
    }

    /// Adds a rock to the board [`stage_rock`](Self::stage_rock) laid out,
    /// without clearing it — for a test that needs two rocks in known places.
    #[cfg(test)]
    pub fn stage_another_rock(
        &mut self,
        position: DVec3,
        velocity: DVec3,
        size: RockSize,
    ) -> Entity {
        let mut logic = lock(&self.shared);
        let world = self.session.server_mut().world_mut();
        spawn_rock(&mut logic, world, size, position, velocity)
    }

    /// Puts the ship somewhere specific, for the same reason.
    #[cfg(test)]
    pub fn stage_ship(&mut self, position: DVec3, heading: f64, velocity: DVec3) {
        let mut logic = lock(&self.shared);
        let world = self.session.server_mut().world_mut();
        place_ship(&mut logic, world, position, heading, velocity);
    }

    /// Forces the game into [`GameState::Playing`] without a trigger pull, so a
    /// test can set a board up and then start it.
    #[cfg(test)]
    pub fn begin(&mut self) {
        lock(&self.shared).state = GameState::Playing;
    }

    /// Whether the action map still counts the fire key as down.
    ///
    /// The *input* state, not a flag the loop keeps beside it: [`ActionMap`]
    /// raises `just_pressed` only on the transition, so a Space the map never
    /// saw released makes every later press a no-op.
    #[cfg(test)]
    #[must_use]
    pub fn fire_is_down(&self) -> bool {
        self.action_map.button_held(ACTION_FIRE)
    }
}

// ---- tests ------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use crcbl::core::FrameClock;

    use super::*;
    use crcbl::core::TickId;
    use crcbl::core::time::{ManualTime, TimeSource as _};

    /// One entry of a script: `(tick index, key, pressed)`.
    type Script = [(u64, KeyCode, bool)];

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
        /// What the autopilot currently has held down. Turn and thrust are
        /// *held* actions, so a controller that pressed and released every tick
        /// would be testing the edge detector rather than the ship.
        held: [bool; 3],
    }

    /// Indices into [`Harness::held`].
    const HELD_LEFT: usize = 0;
    const HELD_RIGHT: usize = 1;
    const HELD_THRUST: usize = 2;

    impl Harness {
        fn new(frame_hz: u32, tick_hz: u32) -> Self {
            Self::with_seed(frame_hz, tick_hz, DEFAULT_SEED)
        }

        fn with_seed(frame_hz: u32, tick_hz: u32, seed: u64) -> Self {
            Self {
                game: Game::with_seed(true, tick_hz, seed).expect("a headless game always starts"),
                clock: FrameClock::new(tick_hz),
                time: ManualTime::new(),
                frame_step: FrameClock::new(frame_hz).tick_dt(),
                ticks: 0,
                held: [false; 3],
            }
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
        fn hold(&mut self, key: KeyCode, slot: usize, want: bool) {
            if self.held[slot] != want {
                self.game.key_event(key, want);
                self.held[slot] = want;
            }
        }

        /// Runs `ticks` of them under [`autopilot`] — a player who is trying.
        fn play_ticks(&mut self, ticks: u64) {
            while self.ticks < ticks {
                self.time.advance(self.frame_step);
                self.clock.update(self.time.elapsed());
                while self.ticks < ticks && self.clock.consume_tick() {
                    let plan = autopilot(&self.game);
                    self.hold(KeyCode::ArrowLeft, HELD_LEFT, plan.left);
                    self.hold(KeyCode::ArrowRight, HELD_RIGHT, plan.right);
                    self.hold(KeyCode::ArrowUp, HELD_THRUST, plan.thrust);
                    if plan.fire {
                        self.game.key_event(KeyCode::Space, true);
                        self.game.key_event(KeyCode::Space, false);
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
        /// churns: the ECS holds exactly the ship, the rocks and the bullets,
        /// and the broadphase holds exactly the rocks plus a live ship.
        ///
        /// This is the leak detector. An entity or a collider that outlived
        /// what it belonged to shows up here on the tick it happened.
        ///
        /// **An exact equality with no allowance for the dead queue.** It used
        /// to carry a `+ pending_despawns()` term, because the ECS swept at the
        /// end of `World::tick` while the game module ran after that — so
        /// everything destroyed on a tick sat in the pool until the next one.
        /// `crcbl_server::Server::tick` now sweeps between the module and the
        /// snapshot, so the queue is empty by the time a tick returns and the
        /// compensation term was only ever hiding that ordering.
        fn assert_nothing_leaked(&mut self) {
            let rocks = self.game.rock_count();
            let bullets = self.game.bullet_count();
            assert_eq!(
                self.game.entity_count(),
                1 + rocks + bullets,
                "tick {}: {rocks} rocks and {bullets} bullets do not account for \
                 the world",
                self.ticks,
            );
            // An equality, not a bound: every collider in the world is a
            // rock. The ship is not in the broadphase and neither is a bullet
            // — see `check_ship` and `sweep_bullets`.
            assert_eq!(
                self.game.collider_count(),
                rocks,
                "tick {}: {rocks} rocks do not account for the broadphase",
                self.ticks,
            );
        }
    }

    /// What the autopilot wants this tick.
    #[derive(Clone, Copy, Debug, Default)]
    struct Plan {
        left: bool,
        right: bool,
        thrust: bool,
        fire: bool,
    }

    /// How close to aimed the autopilot has to be before it shoots, in radians.
    const AIM_TOLERANCE: f64 = 0.10;

    /// A competent-enough player: turn towards the nearest rock, and shoot when
    /// pointing at it.
    ///
    /// Deliberately a **decision per tick from simulation state**, not a
    /// recorded script — the same reason flappy's is. The state a given tick
    /// sees is the same at 20 fps as at 240, so the controller presses the same
    /// keys on the same ticks in both and the runs are comparable. It never
    /// thrusts: drifting into a rock is what kills this game, and a stationary
    /// gun is a better test subject than a good pilot.
    fn autopilot(game: &Game) -> Plan {
        if !game.ship_alive || game.state != GameState::Playing {
            // Fire is also what starts a game and what restarts a finished one.
            return Plan {
                fire: true,
                ..Plan::default()
            };
        }
        let ship = game.ship;
        let Some(target) = game.rocks().into_iter().min_by(|a, b| {
            (a.position - ship)
                .length_squared()
                .total_cmp(&(b.position - ship).length_squared())
        }) else {
            return Plan::default();
        };
        let to = target.position - ship;
        // The inverse of `heading_vector`: a heading of θ points along
        // (-sin θ, cos θ).
        let desired = (-to.x).atan2(to.y);
        let error = wrap_to_pi(desired - game.ship_heading);
        Plan {
            left: error > AIM_TOLERANCE,
            right: error < -AIM_TOLERANCE,
            thrust: false,
            fire: error.abs() <= AIM_TOLERANCE,
        }
    }

    /// Stages a stationary rock straight up the gun's line from a ship that is
    /// not moving, and returns the harness ready to fire.
    ///
    /// `distance` is how far up the line the rock sits. Every split and CCD
    /// test starts from this, so a change to the muzzle or the ship's radius
    /// moves all of them together.
    fn one_rock_in_the_sights(
        frame_hz: u32,
        tick_hz: u32,
        size: RockSize,
        distance: f64,
    ) -> Harness {
        let mut harness = Harness::new(frame_hz, tick_hz);
        harness.game.begin();
        harness
            .game
            .stage_ship(DVec3::new(0.0, -distance, 0.0), 0.0, DVec3::ZERO);
        harness.game.stage_rock(DVec3::ZERO, DVec3::ZERO, size);
        harness
    }

    // ---- the input path ------------------------------------------------------

    /// Puts one intent on the wire and runs exactly one tick of the pair.
    ///
    /// The whole input path and nothing beside it: no action map, no queued key
    /// event, no write to the shared cell. The same three calls in the same
    /// order [`Game::tick`] makes, so what these tests exercise is the path the
    /// game plays through.
    fn send_one_tick(game: &mut Game, intent: Intent) {
        game.sim_time += game.tick_period;
        let (server, client) = game.session.both_mut();
        client.set_input(vec![intent.to_wire()]);
        client.update(game.sim_time);
        assert_eq!(
            server.update(game.sim_time),
            1,
            "one tick period in must be exactly one server tick out",
        );
        client.update(game.sim_time);
    }

    /// **The ship turns on bytes that only ever existed as bytes.**
    ///
    /// The only thing that happens here is `Client::set_input` with an intent's
    /// wire form and one tick of the pair. The trigger starting the run and the
    /// heading moving are the whole input path — encode, seal, transport,
    /// unseal, decode, apply — and a server that drops what a client sends
    /// leaves the game on its title screen facing up, because there is no other
    /// way into this simulation.
    #[test]
    fn the_ship_turns_on_an_intent_that_only_ever_travelled_as_bytes() {
        let mut game = Game::new(true, DEFAULT_TICK_HZ).expect("a headless game always starts");
        assert_eq!(lock(&game.shared).state, GameState::WaitingToStart);
        assert_eq!(lock(&game.shared).heading, 0.0, "the ship opens facing up");

        send_one_tick(
            &mut game,
            Intent {
                fire: true,
                ..Intent::default()
            },
        );
        assert_eq!(
            lock(&game.shared).state,
            GameState::Playing,
            "the trigger never reached the simulation",
        );

        send_one_tick(
            &mut game,
            Intent {
                left: true,
                ..Intent::default()
            },
        );
        let step = SHIP_TURN_RATE * game.tick_dt_secs();
        assert!(
            (lock(&game.shared).heading - step).abs() < 1e-9,
            "one tick of a held turn key is {step} and the ship is at {}",
            lock(&game.shared).heading,
        );

        // And a frame that holds nothing is not the previous one repeated: the
        // server clears its queue every tick, and this one decodes to a player
        // holding no key at all.
        send_one_tick(&mut game, Intent::default());
        assert!(
            (lock(&game.shared).heading - step).abs() < 1e-9,
            "the ship kept turning to {} on a tick nobody asked for",
            lock(&game.shared).heading,
        );
        assert_eq!(
            game.session.server().dropped_input_count(),
            0,
            "a frame was refused rather than applied",
        );
    }

    /// Every intent this build can produce survives the round trip, and no two
    /// share a wire form — a decoder that read one flag back as another would
    /// still round-trip an intent with everything set.
    #[test]
    fn an_intent_survives_the_wire_in_both_directions() {
        let mut seen = Vec::new();
        for flags in 0..32u8 {
            let intent = Intent {
                left: flags & 1 != 0,
                right: flags & 2 != 0,
                thrust: flags & 4 != 0,
                fire: flags & 8 != 0,
                restart: flags & 16 != 0,
            };
            let wire = intent.to_wire();
            assert!(
                !seen.contains(&wire),
                "{intent:?} shares a wire form with an earlier intent",
            );
            seen.push(wire);
            assert_eq!(Intent::from_wire(&[wire]), Some(intent));
        }
        assert_eq!(seen.len(), 32, "the loop did not cover what it claims");
    }

    /// **A frame from something that is not this build is refused.** These are
    /// the only bytes in the game a peer chooses: the count of them and the
    /// bits inside them.
    #[test]
    fn a_frame_this_build_did_not_write_is_refused() {
        assert!(
            Intent::from_wire(&[INTENT_FLAGS]).is_some(),
            "every flag this build defines must decode",
        );

        // A bit outside the mask: a byte no `to_wire` of this game produces.
        assert_eq!(Intent::from_wire(&[!INTENT_FLAGS]), None);
        assert_eq!(Intent::from_wire(&[u8::MAX]), None);

        // And the length is the other half — `set_input` takes a `Vec`, so how
        // many bytes arrive is the peer's choice too.
        assert_eq!(Intent::from_wire(&[]), None);
        assert_eq!(Intent::from_wire(&[0, 0]), None);
    }

    /// Several frames in one tick — a client whose clock ran ahead — fold into
    /// one intent: the held keys are the latest word and the edges survive from
    /// whichever frame raised them.
    #[test]
    fn several_frames_in_one_tick_fold_into_one_intent() {
        let frames: Vec<(TickId, Vec<u8>)> = [
            Intent {
                left: true,
                fire: true,
                ..Intent::default()
            },
            Intent {
                right: true,
                thrust: true,
                ..Intent::default()
            },
            Intent {
                right: true,
                restart: true,
                ..Intent::default()
            },
        ]
        .iter()
        .enumerate()
        .map(|(i, intent)| (TickId::from_raw(i as u64), vec![intent.to_wire()]))
        .collect();

        // `left` and `thrust` were let go of by the last frame that mentioned
        // them and are gone; `fire` and `restart` were raised by one frame each
        // and both survive.
        assert_eq!(
            Intent::from_inputs(ClientInputs::new(&frames, 0)),
            Intent {
                left: false,
                right: true,
                thrust: false,
                fire: true,
                restart: true,
            },
        );

        // An unreadable frame is skipped rather than read as an empty intent,
        // which would be the player letting go of everything.
        let with_rubbish = vec![
            (TickId::ZERO, vec![u8::MAX]),
            (
                TickId::ZERO,
                vec![
                    Intent {
                        thrust: true,
                        ..Intent::default()
                    }
                    .to_wire(),
                ],
            ),
        ];
        assert!(Intent::from_inputs(ClientInputs::new(&with_rubbish, 0)).thrust);

        // A tick nothing arrived for is a player holding nothing.
        assert_eq!(
            Intent::from_inputs(ClientInputs::empty()),
            Intent::default()
        );
    }

    // ---- the board -----------------------------------------------------------

    /// A game has a board before anybody presses anything. A screen that fills
    /// in on the first shot reads as a bug.
    #[test]
    fn the_board_is_dealt_before_the_first_tick() {
        let mut harness = Harness::new(60, 60);
        let rocks = harness.game.rocks();
        assert_eq!(rocks.len(), wave_rocks(0) as usize);
        assert!(
            rocks.iter().all(|rock| rock.size == RockSize::Large),
            "a wave opens with large rocks only"
        );
        harness.assert_nothing_leaked();
    }

    /// **The board the game opens on is the board the dealer laid out**, not one
    /// a tick has already moved.
    ///
    /// [`Game::with_seed`] spends a tick on the handshake and deals *after* it
    /// for exactly this reason: a rock drifts and tumbles on every tick whatever
    /// the state, so a board dealt before that tick is a board the opening frame
    /// shows a step into. Nothing else in this suite would notice — every other
    /// board test asks the generator rather than the field — and the only other
    /// thing that would is the golden, which needs a GPU.
    #[test]
    fn the_opening_board_is_where_the_dealer_put_it() {
        let game = Game::new(true, DEFAULT_TICK_HZ).expect("a headless game always starts");
        let rocks = game.rocks();
        assert_eq!(rocks.len(), wave_rocks(0) as usize);
        for (rock, view) in rocks.iter().enumerate() {
            // Through the same wrap `spawn_rock` puts a spawn point through: a
            // wave deals onto the field's border, and a border point is the
            // opposite edge.
            assert_eq!(
                view.position,
                wrap_position(wave_rock_position(game.board_seed(), 0, rock as u32)),
                "rock {rock} had already moved when the game handed the board back",
            );
        }
        assert_eq!(
            lock(&game.shared).ship_pos,
            DVec3::ZERO,
            "the ship was not at the centre the board was built around",
        );
    }

    /// Every rock a wave deals enters on the border, never on top of the ship.
    ///
    /// Checked across seeds and waves rather than at the default, because the
    /// failure this guards against — a rock dealt onto the respawn point —
    /// would be a life lost to the dealer, and would show up once in a hundred
    /// games rather than in the first one.
    #[test]
    fn every_rock_a_wave_deals_enters_on_the_border() {
        for seed in [0, 7, DEFAULT_SEED, u64::MAX] {
            for wave in 0..40 {
                for rock in 0..wave_rocks(wave) {
                    let position = wave_rock_position(seed, wave, rock);
                    let on_edge = (position.x.abs() - WORLD_HALF_WIDTH).abs() < 1e-9
                        || (position.y.abs() - WORLD_HALF_HEIGHT).abs() < 1e-9;
                    assert!(
                        on_edge,
                        "seed {seed} wave {wave} rock {rock} was dealt at {position:?}, \
                         which is not on the border"
                    );
                    assert!(
                        position.length() > RESPAWN_CLEAR_RADIUS + RockSize::Large.radius(),
                        "seed {seed} wave {wave} rock {rock} at {position:?} \
                         is within reach of the respawn point"
                    );
                }
            }
        }
    }

    /// Two games told the same seed deal the same board, rock for rock.
    #[test]
    fn the_same_seed_deals_the_same_board() {
        for seed in [0, 1, DEFAULT_SEED, u64::MAX] {
            let deal = |()| -> Vec<(DVec3, DVec3)> {
                (0..200)
                    .map(|i| {
                        (
                            wave_rock_position(seed, i / 8, i % 8),
                            wave_rock_velocity(seed, i / 8, i % 8),
                        )
                    })
                    .collect()
            };
            assert_eq!(
                deal(()),
                deal(()),
                "seed {seed} is not a function of its index"
            );
        }
    }

    /// Different seeds are different boards. Without this the seed would be
    /// decoration and every game would be the same game.
    #[test]
    fn different_seeds_deal_different_boards() {
        let a: Vec<DVec3> = (0..100).map(|i| wave_rock_position(1, 0, i)).collect();
        let b: Vec<DVec3> = (0..100).map(|i| wave_rock_position(2, 0, i)).collect();
        let shared = a.iter().zip(&b).filter(|(x, y)| x == y).count();
        assert!(
            shared < 5,
            "{shared} of 100 rocks coincided between two seeds"
        );
    }

    /// The shipped first wave, pinned to literal positions and velocities. A
    /// change to `wave_rock_position`'s or `wave_rock_velocity`'s arithmetic —
    /// or to the hash under them — redraws every board and every replay the
    /// project ships, and the determinism suites compare one run against
    /// another, so they would all move together. These literals are the anchor
    /// that does not.
    #[test]
    fn the_shipped_first_wave_is_pinned_to_literal_values() {
        let positions = [
            DVec3::new(3.669167470067322, -12.0, 0.0),
            DVec3::new(16.0, 7.514915638166904, 0.0),
            DVec3::new(16.0, -3.819280049496051, 0.0),
        ];
        let velocities = [
            DVec3::new(-1.966823772697785, 0.9856998768138566, 0.0),
            DVec3::new(2.109494435445692, -0.624526402022895, 0.0),
            DVec3::new(1.7103816638749278, 1.3836887525308696, 0.0),
        ];
        for (rock, (position, velocity)) in positions.iter().zip(&velocities).enumerate() {
            assert_eq!(
                wave_rock_position(DEFAULT_SEED, 0, rock as u32),
                *position,
                "rock {rock}"
            );
            assert_eq!(
                wave_rock_velocity(DEFAULT_SEED, 0, rock as u32),
                *velocity,
                "rock {rock}"
            );
        }
    }

    /// Waves grow, and then stop growing.
    #[test]
    fn waves_grow_to_a_ceiling_and_no_further() {
        assert_eq!(wave_rocks(0), FIRST_WAVE_ROCKS);
        assert_eq!(wave_rocks(1), FIRST_WAVE_ROCKS + 1);
        assert!(wave_rocks(3) > wave_rocks(2));
        for wave in 0..500 {
            assert!(wave_rocks(wave) <= MAX_WAVE_ROCKS);
            assert!(
                wave_rocks(wave + 1) >= wave_rocks(wave),
                "wave {wave} shrank"
            );
        }
        assert_eq!(wave_rocks(500), MAX_WAVE_ROCKS);
    }

    // ---- the ship ------------------------------------------------------------

    /// The ship does not move until the player asks it to.
    #[test]
    fn a_ship_nobody_has_flown_stays_exactly_where_it_started() {
        let mut harness = Harness::new(60, 60);
        harness.run_ticks(120, &[]);
        assert_eq!(harness.game.state, GameState::WaitingToStart);
        assert_eq!(harness.game.ship, DVec3::ZERO);
        assert_eq!(harness.game.ship_velocity, DVec3::ZERO);
        assert_eq!(harness.game.ship_heading, 0.0);
    }

    /// A held turn key turns the ship at exactly [`SHIP_TURN_RATE`], and the
    /// two keys turn opposite ways.
    ///
    /// The rate is asserted against the constant rather than against "it
    /// moved": rotation is the one part of this ship the physics crate does not
    /// own, so it is the part with nothing else checking it.
    #[test]
    fn a_held_turn_key_turns_the_ship_at_the_stated_rate() {
        for (key, sign) in [(KeyCode::ArrowLeft, 1.0), (KeyCode::ArrowRight, -1.0f64)] {
            let mut harness = Harness::new(60, 60);
            harness.run_ticks(1, &[(0, KeyCode::Space, true), (1, KeyCode::Space, false)]);
            assert_eq!(harness.game.state, GameState::Playing);

            let turns = 30;
            let start = harness.ticks;
            harness.run_ticks(start + turns, &[(start, key, true)]);

            let dt = harness.game.tick_dt_secs();
            let expected =
                (sign * SHIP_TURN_RATE * dt * turns as f64).rem_euclid(std::f64::consts::TAU);
            assert!(
                (harness.game.ship_heading - expected).abs() < 1e-9,
                "{key:?} for {turns} ticks left the heading at {}, not {expected}",
                harness.game.ship_heading,
            );
        }
    }

    /// Thrust pushes the ship along its heading, and only along its heading.
    #[test]
    fn thrust_pushes_the_ship_where_it_is_pointing() {
        let mut harness = Harness::new(60, 60);
        harness.game.begin();
        // Pointing 90 degrees left of "up", so a bug that thrusts along a fixed
        // axis rather than the heading fails rather than coincidentally passes.
        harness
            .game
            .stage_ship(DVec3::ZERO, std::f64::consts::FRAC_PI_2, DVec3::ZERO);
        harness.game.stage_rock(
            DVec3::new(0.0, WORLD_HALF_HEIGHT - 1.0, 0.0),
            DVec3::ZERO,
            RockSize::Small,
        );

        harness.run_ticks(30, &[(0, KeyCode::ArrowUp, true)]);
        let velocity = harness.game.ship_velocity;
        assert!(
            velocity.length() > 1.0,
            "the ship never accelerated: {velocity:?}"
        );
        let heading = heading_vector(std::f64::consts::FRAC_PI_2);
        let along = velocity.normalize().dot(heading);
        assert!(
            (along - 1.0).abs() < 1e-9,
            "the ship accelerated along {:?} rather than its heading {heading:?}",
            velocity.normalize(),
        );
    }

    /// A ship under full thrust settles at `SHIP_THRUST / SHIP_DAMPING`, and a
    /// ship that lets go coasts back down.
    ///
    /// The terminal speed *is* the speed limit — there is no clamp — so a run
    /// that overshot it would mean the damping was not being applied at all.
    #[test]
    fn the_terminal_speed_is_thrust_over_damping_and_the_coast_undoes_it() {
        let mut harness = Harness::new(60, 60);
        harness.game.begin();
        harness.game.stage_ship(DVec3::ZERO, 0.0, DVec3::ZERO);
        // No rocks: this is about the force pipeline, and a run that ended on a
        // collision would prove nothing either way.
        harness.game.stage_rock(
            DVec3::new(WORLD_HALF_WIDTH - 1.0, WORLD_HALF_HEIGHT - 1.0, 0.0),
            DVec3::ZERO,
            RockSize::Small,
        );

        let terminal = SHIP_THRUST / SHIP_DAMPING;
        harness.run_ticks(900, &[(0, KeyCode::ArrowUp, true)]);
        let top = harness.game.ship_velocity.length();
        assert!(
            (top - terminal).abs() < 0.05,
            "fifteen seconds of thrust reached {top}, not the terminal {terminal}"
        );
        assert!(top <= terminal, "the ship exceeded its own terminal speed");

        // Let go: one time constant is `mass / SHIP_DAMPING` = 1 s, so a second
        // of coasting has to take about `1/e` of it off.
        let start = harness.ticks;
        harness.run_ticks(start + 60, &[(start, KeyCode::ArrowUp, false)]);
        let coasted = harness.game.ship_velocity.length();
        let expected = top * std::f64::consts::E.recip();
        assert!(
            (coasted - expected).abs() < 0.2,
            "a second of coasting left {coasted}, not about {expected}"
        );
    }

    /// The ship's coast is the engine's damping model, reached through the
    /// per-entity route rather than copied.
    ///
    /// This test used to compare a hand-rolled `damping_force` in this file
    /// against `DampingForce::apply`, because the force pipeline is global and
    /// this game damps exactly one of its bodies.
    /// [`DampingForce::world_force`] is that route now and the copy is gone, so
    /// what is left to check here is that the game reaches it with **its own**
    /// constants — a ship built with a different mass or coefficient than
    /// `drive_ship` uses would coast differently and nothing else would say so.
    ///
    /// [`DampingForce::world_force`]: crcbl::phys::DampingForce::world_force
    #[test]
    fn the_ships_coast_is_the_engines_damping_with_this_games_constants() {
        let harness = Harness::new(60, 60);
        let logic = lock(&harness.game.shared);
        let dt = 1.0 / 60.0;
        let velocity = DVec3::new(-14.0, 3.0, 0.0);
        assert_eq!(
            logic.damping.world_force(velocity, SHIP_MASS, dt),
            DampingForce::new(SHIP_DAMPING).world_force(velocity, SHIP_MASS, dt),
            "the ship is damped by something other than SHIP_DAMPING",
        );
        assert_eq!(
            logic.damping.world_force(velocity, SHIP_MASS, dt),
            -velocity * SHIP_DAMPING,
            "at 60 Hz the cap is far away, so this is plain -k·v",
        );
    }

    // ---- the wrap ------------------------------------------------------------

    /// `wrap_axis` is **bit-exact** inside the field, and periodic outside it.
    ///
    /// Exactness is not fussiness. `teleport_if_outside` decides whether to
    /// take a collider out of the broadphase and put it back by comparing this
    /// against the position it was given, so a round trip that returned a value
    /// one ulp away would re-insert every rock on every tick, forever. The
    /// values below are chosen to be the ones a bare `rem_euclid` gets wrong:
    /// `0.1 + 16.0 - 16.0` is `0.100000000000001`, not `0.1`.
    #[test]
    fn the_wrap_is_exact_inside_the_field_and_periodic_outside_it() {
        for v in [
            -15.999_9,
            -0.5,
            0.0,
            7.25,
            15.999_9,
            0.1,
            -0.3,
            1.0 / 3.0,
            12.345_678_901_234,
        ] {
            assert_eq!(wrap_axis(v, WORLD_HALF_WIDTH), v, "{v} was moved");
        }
        // And the whole position, which is what the teleport check compares.
        for point in [
            DVec3::new(0.1, -0.3, 0.0),
            DVec3::new(1.0 / 3.0, -11.345_678_901_234, 0.0),
        ] {
            assert_eq!(wrap_position(point), point, "{point:?} was moved");
        }
        assert!((wrap_axis(16.1, 16.0) - -15.9).abs() < 1e-12);
        assert!((wrap_axis(-16.1, 16.0) - 15.9).abs() < 1e-12);
        // A teleport of many field-widths lands in the same place as one.
        assert!((wrap_axis(16.1 + 32.0 * 5.0, 16.0) - -15.9).abs() < 1e-9);
        // The upper edge belongs to the lower one, so there is exactly one
        // representation of every point.
        assert_eq!(wrap_axis(16.0, 16.0), -16.0);
    }

    /// A ship leaving each of the four edges arrives at the opposite one.
    #[test]
    fn a_ship_leaving_each_edge_arrives_at_the_other() {
        let cases = [
            (
                DVec3::new(WORLD_HALF_WIDTH - 0.2, 0.0, 0.0),
                DVec3::new(8.0, 0.0, 0.0),
            ),
            (
                DVec3::new(-WORLD_HALF_WIDTH + 0.2, 0.0, 0.0),
                DVec3::new(-8.0, 0.0, 0.0),
            ),
            (
                DVec3::new(0.0, WORLD_HALF_HEIGHT - 0.2, 0.0),
                DVec3::new(0.0, 8.0, 0.0),
            ),
            (
                DVec3::new(0.0, -WORLD_HALF_HEIGHT + 0.2, 0.0),
                DVec3::new(0.0, -8.0, 0.0),
            ),
        ];
        for (start, velocity) in cases {
            let mut harness = Harness::new(60, 60);
            harness.game.begin();
            harness.game.stage_ship(start, 0.0, velocity);
            // One rock, parked in a corner well away from the ship's line, so
            // the field is not empty and the wave machinery stays quiet.
            harness.game.stage_rock(
                DVec3::new(WORLD_HALF_WIDTH - 2.0, WORLD_HALF_HEIGHT - 2.0, 0.0),
                DVec3::ZERO,
                RockSize::Small,
            );

            harness.run_ticks(30, &[]);
            let after = harness.game.ship;
            assert!(
                after.x >= -WORLD_HALF_WIDTH && after.x < WORLD_HALF_WIDTH,
                "from {start:?} at {velocity:?} the ship ended outside the field: {after:?}"
            );
            assert!(
                after.y >= -WORLD_HALF_HEIGHT && after.y < WORLD_HALF_HEIGHT,
                "from {start:?} at {velocity:?} the ship ended outside the field: {after:?}"
            );
            // And it came out of the *opposite* side, not merely stayed inside.
            let travelled = velocity.normalize();
            let displacement = after - start;
            assert!(
                displacement.dot(travelled) < 0.0,
                "from {start:?} at {velocity:?} the ship reached {after:?}, \
                 which is not on the far side"
            );
        }
    }

    /// **A wrapped ship still collides.** The wrap is a teleport, and a
    /// teleport that moved the game's idea of where the ship is without moving
    /// its collider would leave a ship that flies through everything on the far
    /// side of the field — and a test that only checked the position would pass
    /// while it happened.
    #[test]
    fn a_ship_that_has_wrapped_still_collides_on_the_far_side() {
        let mut harness = Harness::new(60, 60);
        harness.game.begin();
        harness.game.stage_ship(
            DVec3::new(WORLD_HALF_WIDTH - 0.2, 0.0, 0.0),
            0.0,
            DVec3::new(8.0, 0.0, 0.0),
        );
        // Just inside the *left* edge: the ship can only reach it by wrapping.
        let rock = DVec3::new(-WORLD_HALF_WIDTH + 1.0, 0.0, 0.0);
        harness.game.stage_rock(rock, DVec3::ZERO, RockSize::Large);

        harness.run_ticks(1, &[]);
        assert_eq!(
            harness.game.lives, STARTING_LIVES,
            "it died before wrapping"
        );
        assert!(
            harness.game.ship.x > 0.0,
            "the ship should still be on the right: {:?}",
            harness.game.ship
        );

        harness.run_ticks(40, &[]);
        assert!(
            harness.game.ship.x < 0.0 || !harness.game.ship_alive,
            "the ship never reached the left side: {:?}",
            harness.game.ship
        );
        assert_eq!(
            harness.game.lives,
            STARTING_LIVES - 1,
            "the wrapped ship flew through a rock it was on top of"
        );
        assert!(!harness.game.ship_alive);
    }

    /// **The tick a ship wraps is the tick it collides on the far side**, not
    /// the one after.
    ///
    /// The sharper form of the test above, and the one that pins the *order*
    /// inside a tick: the ship's overlap query is centred on a position read
    /// back from physics, and reading it before the wrap rather than after
    /// leaves the query a whole field away from the ship for one tick. The
    /// looser test cannot see that — it runs forty ticks and the collision
    /// merely arrives late — so this one runs exactly one.
    #[test]
    fn a_ship_collides_on_the_very_tick_it_wraps() {
        let mut harness = Harness::new(60, 60);
        harness.game.begin();
        // Placed so that one tick of travel takes it just over the right edge:
        // 8 units/s at 60 Hz is 0.133 of a unit, against 0.05 of clearance.
        harness.game.stage_ship(
            DVec3::new(WORLD_HALF_WIDTH - 0.05, 0.0, 0.0),
            0.0,
            DVec3::new(8.0, 0.0, 0.0),
        );
        // Where it will come out, near enough: the wrap lands it at about
        // -15.917 and the rock's radius is twenty times the difference.
        harness.game.stage_rock(
            DVec3::new(-WORLD_HALF_WIDTH + 0.1, 0.0, 0.0),
            DVec3::ZERO,
            RockSize::Small,
        );

        harness.run_ticks(1, &[]);
        assert!(
            harness.game.ship.x < 0.0,
            "the ship did not wrap on the first tick: {:?}",
            harness.game.ship
        );
        assert_eq!(
            harness.game.lives,
            STARTING_LIVES - 1,
            "the collision was a tick late, so the query was centred on where \
             the ship used to be"
        );
    }

    /// The same for a rock: one that wraps is still found by the ship's overlap
    /// query on the far side.
    #[test]
    fn a_rock_that_has_wrapped_still_collides_on_the_far_side() {
        let mut harness = Harness::new(60, 60);
        harness.game.begin();
        // A stationary ship a little inside the left edge, and a rock leaving
        // the right one. Only the rock moves, so only the rock's teleport can
        // bring them together.
        harness.game.stage_ship(
            DVec3::new(-WORLD_HALF_WIDTH + 1.0, 0.0, 0.0),
            0.0,
            DVec3::ZERO,
        );
        harness.game.stage_rock(
            DVec3::new(WORLD_HALF_WIDTH - 0.2, 0.0, 0.0),
            DVec3::new(6.0, 0.0, 0.0),
            RockSize::Large,
        );

        harness.run_ticks(1, &[]);
        assert_eq!(harness.game.lives, STARTING_LIVES);
        harness.run_ticks(40, &[]);
        assert_eq!(
            harness.game.lives,
            STARTING_LIVES - 1,
            "a rock that wrapped onto the ship did not touch it"
        );
    }

    // ---- bullets and CCD -----------------------------------------------------

    /// **A bullet that crosses a rock inside one tick still hits it.**
    ///
    /// The whole point of sweeping `prev → cur` rather than testing where the
    /// bullet ended up. The tick rate is turned down until one tick of travel is
    /// wider than the rock, and the test then *proves the discrete test would
    /// have missed*: it checks that no tick ever left the bullet inside the
    /// rock, and that the tick which hit it began on one side and ended on the
    /// other.
    #[test]
    fn a_bullet_that_crosses_a_rock_within_one_tick_still_hits_it() {
        // 4 Hz: a quarter-second step, so a bullet covers six units while the
        // rock it must not skip is barely one across.
        let mut harness = one_rock_in_the_sights(4, 4, RockSize::Small, 8.0);
        let dt = harness.game.tick_dt_secs();
        let reach = RockSize::Small.radius() + BULLET_RADIUS;
        assert!(
            BULLET_SPEED * dt > 2.0 * reach,
            "this tick rate does not make the bullet skip the rock at all: \
             {} against {}",
            BULLET_SPEED * dt,
            2.0 * reach,
        );

        harness.tap(KeyCode::Space);
        assert_eq!(harness.game.bullet_count(), 1, "nothing was fired");
        assert_eq!(harness.game.score, 0);

        // One tick of flight, which lands the bullet short of the rock.
        harness.run_ticks(harness.ticks + 1, &[]);
        let before = harness.game.bullets().first().copied().map(|b| b.position);
        let Some(before) = before else {
            panic!("the bullet vanished before it reached anything");
        };
        assert!(
            before.y < -reach,
            "the bullet is already at the rock: {before:?}"
        );
        let after = before + DVec3::new(0.0, BULLET_SPEED * dt, 0.0);
        assert!(
            after.y > reach,
            "the next tick does not clear the rock, so this proves nothing: {after:?}"
        );

        // The tick that steps straight over it.
        harness.run_ticks(harness.ticks + 1, &[]);
        assert_eq!(
            harness.game.score,
            RockSize::Small.score(),
            "the bullet stepped over the rock: it went from {before:?} to {after:?}"
        );
        assert_eq!(harness.game.bullet_count(), 0, "the bullet was not spent");
    }

    /// **A bullet is never swept over ground it did not travel.**
    ///
    /// The reason the gun fires *after* the sweep — see `run_tick`. A sweep
    /// reconstructs `prev` as `position - velocity * dt`, so a bullet swept on
    /// the tick it was created is swept from a point one whole step *behind the
    /// muzzle*, through the ship that fired it, to the muzzle. At 60 Hz that
    /// segment is 0.4 of a unit and hides inside the hull; at 4 Hz it is six
    /// units of field behind the ship.
    ///
    /// The decoy below sits in exactly that stretch, **behind** the ship, where
    /// nothing the bullet actually does can reach it.
    #[test]
    fn a_bullet_is_never_swept_over_ground_it_did_not_travel() {
        let mut harness = Harness::new(4, 4);
        harness.game.begin();
        harness.game.stage_ship(DVec3::ZERO, 0.0, DVec3::ZERO);
        let dt = harness.game.tick_dt_secs();
        let step = BULLET_SPEED * dt;

        // Halfway along the phantom segment, which runs from the muzzle back to
        // `muzzle - step`.
        let behind = MUZZLE_OFFSET - step / 2.0;
        assert!(
            behind < -RockSize::Small.radius() - SHIP_RADIUS,
            "the decoy at {behind} is not clear of the ship, so a hit on it \
             would be ambiguous",
        );
        harness
            .game
            .stage_rock(DVec3::new(0.0, behind, 0.0), DVec3::ZERO, RockSize::Small);
        // In front, near enough that one real step reaches it.
        harness.game.stage_another_rock(
            DVec3::new(0.0, step / 2.0, 0.0),
            DVec3::ZERO,
            RockSize::Small,
        );

        harness.tap(KeyCode::Space);
        assert_eq!(
            harness.game.bullet_count(),
            1,
            "nothing was fired — or the shot was spent on the tick it left the \
             gun, which is this bug reported one assertion early",
        );
        harness.run_ticks(harness.ticks + 2, &[]);

        // A `Small` rock shatters into nothing, so the survivors are the whole
        // answer: exactly one rock, and it is the one behind the ship.
        let alive = harness.game.rocks();
        assert_eq!(
            alive.len(),
            1,
            "expected the rock in front destroyed and the one behind untouched; \
             got {alive:?}",
        );
        assert!(
            (alive[0].position.y - behind).abs() < 1e-9,
            "the survivor is the rock in *front* of the ship, so the bullet was \
             swept backwards through the hull and killed the decoy at {behind}: \
             {alive:?}",
        );
        harness.assert_nothing_leaked();
    }

    /// A bullet fired at a rock at the ordinary tick rate hits it too — the
    /// case the CCD test above deliberately makes impossible for a point test,
    /// asserted here for the case that is not.
    #[test]
    fn a_bullet_hits_the_rock_it_was_aimed_at() {
        let mut harness = one_rock_in_the_sights(60, 60, RockSize::Large, 6.0);
        harness.tap(KeyCode::Space);
        harness.run_ticks(harness.ticks + 30, &[]);
        assert_eq!(harness.game.score, RockSize::Large.score());
    }

    /// **A hit raises a flash where the rock died, sized to it, and the flash
    /// fades out over exactly [`FLASH_LIFE`].**
    ///
    /// The visual twin of the explosion cue, and the point of the whole
    /// feature: a rock that vanishes with only a sound is a rock the game
    /// forgot to draw. The life is asserted against the tick's own `dt` — and
    /// the tick count it survives is derived from [`FLASH_LIFE`] and that
    /// `dt` rather than hardcoded — so a change to either constant updates
    /// this test.
    #[test]
    fn a_hit_raises_a_flash_at_the_dead_rock_that_fades_out() {
        let mut harness = one_rock_in_the_sights(60, 60, RockSize::Small, 6.0);
        let dt = harness.game.tick_dt_secs();
        let life_ticks = (FLASH_LIFE / dt).ceil() as u64;
        let read = |h: &Harness| {
            let mut render = RenderState::default();
            h.game.render_state(&mut render);
            render.flashes
        };

        harness.tap(KeyCode::Space);
        // Step until the shot lands, then read the flash the tick after the
        // hit: the life assertion below is about its *first* tick.
        loop {
            harness.run_ticks(harness.ticks + 1, &[]);
            assert!(harness.ticks < 40, "the shot never landed");
            if !read(&harness).is_empty() {
                break;
            }
        }
        let first = read(&harness);
        assert_eq!(first.len(), 1, "the hit raised no flash");
        assert_eq!(
            first[0].position,
            DVec3::ZERO,
            "the flash is not where the rock died",
        );
        assert_eq!(
            first[0].size,
            RockSize::Small,
            "the flash is not the rock's size"
        );
        assert!(
            (first[0].life - (FLASH_LIFE - dt)).abs() < 1e-9,
            "a fresh flash carries {}s of life, not {}",
            first[0].life,
            FLASH_LIFE - dt,
        );

        // One more tick: exactly one tick's dt has come off.
        harness.run_ticks(harness.ticks + 1, &[]);
        let aged = read(&harness);
        assert_eq!(aged.len(), 1, "the flash went out a tick early");
        assert!(
            (first[0].life - aged[0].life - dt).abs() < 1e-9,
            "the flash aged {}s in one tick, not {}",
            first[0].life - aged[0].life,
            dt,
        );

        // The tick before it would run out it is still burning; the tick
        // after, gone.
        harness.run_ticks(harness.ticks + (life_ticks - 3), &[]);
        assert_eq!(read(&harness).len(), 1, "the flash expired before its time");
        harness.run_ticks(harness.ticks + 1, &[]);
        assert!(read(&harness).is_empty(), "the flash outlived its life");
        harness.assert_nothing_leaked();
    }

    /// **Two hits raise two flashes, and they coexist.** The flash list is a
    /// list, not a single slot: a second rock dying while the first flash is
    /// still burning must show both.
    ///
    /// Both hits land on the same tick — the only way two can overlap, since a
    /// flash lives nine ticks at 60 Hz against a ten-tick reload. One shot up
    /// the +Y line at a far rock, one along +X at a near one, aimed by
    /// re-staging the ship rather than by holding a key, which would take
    /// longer than either flash's life.
    #[test]
    fn two_hits_raise_two_flashes_that_coexist() {
        let mut harness = Harness::new(60, 60);
        harness.game.begin();
        harness.game.stage_ship(DVec3::ZERO, 0.0, DVec3::ZERO);
        harness
            .game
            .stage_rock(DVec3::new(0.0, 8.0, 0.0), DVec3::ZERO, RockSize::Small);
        harness
            .game
            .stage_another_rock(DVec3::new(4.0, 0.0, 0.0), DVec3::ZERO, RockSize::Small);

        harness.tap(KeyCode::Space); // tick 0: the +Y shot
        harness.run_ticks(harness.ticks + 9, &[]); // the cooldown
        harness
            .game
            .stage_ship(DVec3::ZERO, -std::f64::consts::FRAC_PI_2, DVec3::ZERO);
        harness.tap(KeyCode::Space); // tick 10: the +X shot
        harness.run_ticks(harness.ticks + 7, &[]); // both land on tick 17

        let mut render = RenderState::default();
        harness.game.render_state(&mut render);
        assert_eq!(render.flashes.len(), 2, "two hits must raise two flashes");
        let mut where_: Vec<DVec3> = render.flashes.iter().map(|f| f.position).collect();
        where_.sort_by(|a, b| a.x.total_cmp(&b.x));
        assert_eq!(
            where_,
            vec![DVec3::new(0.0, 8.0, 0.0), DVec3::new(4.0, 0.0, 0.0)],
            "the flashes are not where the rocks died",
        );
        assert!(
            render.flashes.iter().all(|f| f.size == RockSize::Small),
            "a flash is not the size of the rock it marks",
        );
        let dt = harness.game.tick_dt_secs();
        assert!(
            render
                .flashes
                .iter()
                .all(|f| (f.life - (FLASH_LIFE - dt)).abs() < 1e-9),
            "a flash did not carry the expected first-tick life of {}s (found {:?})",
            FLASH_LIFE - dt,
            render.flashes.iter().map(|f| f.life).collect::<Vec<_>>(),
        );
        harness.assert_nothing_leaked();
    }

    /// A bullet that hits nothing expires, and takes its entity with it.
    #[test]
    fn a_bullet_that_hits_nothing_expires() {
        let mut harness = Harness::new(60, 60);
        harness.game.begin();
        harness.game.stage_ship(DVec3::ZERO, 0.0, DVec3::ZERO);
        // The one rock is off in a corner, nowhere near the column the shot
        // flies up. Deliberately *not* directly behind the ship: the field
        // wraps, so "behind" is also "a long way ahead", and a shot fired up a
        // column that had a rock at the bottom of it would arrive there.
        harness.game.stage_rock(
            DVec3::new(WORLD_HALF_WIDTH - 2.0, WORLD_HALF_HEIGHT - 2.0, 0.0),
            DVec3::ZERO,
            RockSize::Small,
        );

        harness.tap(KeyCode::Space);
        assert_eq!(harness.game.bullet_count(), 1);
        let entities = harness.game.entity_count();

        harness.run_ticks(harness.ticks + 70, &[]);
        assert_eq!(
            harness.game.bullet_count(),
            0,
            "the bullet outlived its life"
        );
        assert_eq!(harness.game.score, 0, "it hit something it should not have");
        assert_eq!(
            harness.game.entity_count(),
            entities - 1,
            "the expired bullet left its entity behind"
        );
        assert_eq!(harness.game.pending_despawns(), 0, "the sweep never ran");
        harness.assert_nothing_leaked();
    }

    /// **A shot cannot lap the field.** The playfield wraps, so a bullet that
    /// lived long enough would come round and arrive behind the ship that fired
    /// it — a rule nobody would guess, and the one that made
    /// `a_bullet_that_hits_nothing_expires` fail while [`BULLET_LIFE`] was a
    /// second against a field exactly 24 units tall.
    ///
    /// The relation is asserted, not the number, so a later tuning pass that
    /// changes either constant is told rather than left to find out.
    #[test]
    fn the_reach_of_a_shot_is_less_than_one_lap() {
        let reach = BULLET_SPEED * BULLET_LIFE;
        assert!(
            reach < 2.0 * WORLD_HALF_HEIGHT,
            "a shot reaches {reach}, which laps a field {} tall",
            2.0 * WORLD_HALF_HEIGHT
        );
        assert!(reach < 2.0 * WORLD_HALF_WIDTH);
        // And it still crosses most of the screen, or it is not a weapon.
        assert!(reach > WORLD_HALF_HEIGHT, "a shot only reaches {reach}");
    }

    /// A shot never finds the hull it left, however long the ship sits still.
    ///
    /// True today because the ship has no collider at all (see `check_ship`),
    /// which is why this reads as a regression guard rather than as a test of
    /// an exclusion list: it goes red the moment somebody puts one in the tree
    /// without also excluding it from the sweep.
    #[test]
    fn a_ship_cannot_shoot_itself() {
        let mut harness = Harness::new(60, 60);
        harness.game.begin();
        harness.game.stage_ship(DVec3::ZERO, 0.0, DVec3::ZERO);
        harness.game.stage_rock(
            DVec3::new(WORLD_HALF_WIDTH - 2.0, WORLD_HALF_HEIGHT - 2.0, 0.0),
            DVec3::ZERO,
            RockSize::Small,
        );
        let shots = 20;
        for _ in 0..shots {
            harness.tap(KeyCode::Space);
            harness.run_ticks(harness.ticks + 12, &[]);
        }
        // The premise, asserted rather than assumed: the surviving lives below
        // say nothing at all unless the shots were really taken. One tap, one
        // bullet — twelve ticks clears the cooldown and the cap is never
        // reached, so a shortfall here means firing broke, not that the ship
        // was spared.
        assert_eq!(
            harness.game.bullets_fired(),
            shots,
            "the taps did not become shots, so this test proves nothing"
        );
        assert_eq!(harness.game.lives, STARTING_LIVES, "the ship shot itself");
        assert_eq!(harness.game.score, 0);
    }

    /// The magazine holds four, and the trigger has a rhythm.
    #[test]
    fn no_more_than_four_bullets_are_ever_in_the_air() {
        let mut harness = Harness::new(60, 60);
        harness.game.begin();
        harness.game.stage_ship(DVec3::ZERO, 0.0, DVec3::ZERO);
        harness.game.stage_rock(
            DVec3::new(0.0, -WORLD_HALF_HEIGHT + 1.0, 0.0),
            DVec3::ZERO,
            RockSize::Small,
        );

        let mut worst = 0;
        for _ in 0..120 {
            harness.tap(KeyCode::Space);
            worst = worst.max(harness.game.bullet_count());
            assert!(
                harness.game.bullet_count() <= MAX_BULLETS,
                "{} bullets in the air",
                harness.game.bullet_count()
            );
        }
        assert_eq!(
            worst, MAX_BULLETS,
            "the magazine never filled, so the cap is untested"
        );
    }

    /// **The trigger has a cooldown, and it is [`FIRE_COOLDOWN`] long.**
    ///
    /// Its own test rather than a bound tacked onto the magazine one: with a
    /// four-shot cap and a bullet that lives 48 ticks, a cooldown of *zero*
    /// still only lets twelve shots out in 120 ticks, so any inequality loose
    /// enough to be safe is loose enough to pass with the cooldown deleted.
    /// The gap between the first two shots is exact and cannot be.
    #[test]
    fn the_trigger_has_a_cooldown_of_exactly_the_stated_length() {
        let mut harness = Harness::new(60, 60);
        harness.game.begin();
        harness.game.stage_ship(DVec3::ZERO, 0.0, DVec3::ZERO);
        harness.game.stage_rock(
            DVec3::new(WORLD_HALF_WIDTH - 2.0, WORLD_HALF_HEIGHT - 2.0, 0.0),
            DVec3::ZERO,
            RockSize::Small,
        );

        harness.tap(KeyCode::Space);
        assert_eq!(
            harness.game.bullet_count(),
            1,
            "the first shot did not fire"
        );
        let first = harness.ticks;

        // Pull the trigger every tick until a second shot gets out.
        let mut waited = 0;
        while harness.game.bullet_count() < 2 {
            harness.tap(KeyCode::Space);
            waited += 1;
            assert!(waited < 60, "no second shot in a whole second");
        }
        let expected = (FIRE_COOLDOWN / harness.game.tick_dt_secs()).ceil() as u64;
        assert_eq!(
            harness.ticks - first,
            expected,
            "the second shot came {} ticks after the first, not {expected}",
            harness.ticks - first,
        );
        assert!(expected > 1, "a one-tick cooldown is no cooldown");
    }

    // ---- splits --------------------------------------------------------------

    /// A large rock becomes exactly two mediums, and a medium exactly two
    /// smalls. Counts asserted exactly, by size, because "the number went up"
    /// would pass for a split that produced three.
    #[test]
    fn a_rock_splits_into_exactly_two_of_the_next_size_down() {
        for (size, child) in [
            (RockSize::Large, RockSize::Medium),
            (RockSize::Medium, RockSize::Small),
        ] {
            let mut harness = one_rock_in_the_sights(60, 60, size, 6.0);
            harness.run_ticks(1, &[]);
            assert_eq!(harness.game.rock_count(), 1);

            harness.tap(KeyCode::Space);
            harness.run_ticks(harness.ticks + 30, &[]);

            let rocks = harness.game.rocks();
            assert_eq!(rocks.len(), SPLIT_CHILDREN, "{size:?} did not split in two");
            assert!(
                rocks.iter().all(|rock| rock.size == child),
                "{size:?} produced {rocks:?} rather than two {child:?}"
            );
            assert_eq!(harness.game.score, size.score());
            assert_eq!(
                harness.game.wave, 0,
                "the wave advanced while rocks remained"
            );
            harness.assert_nothing_leaked();
        }
    }

    /// A small rock simply dies — and, because it was the last one, the next
    /// wave arrives with more rocks than the last.
    ///
    /// The two halves are one test because they are one tick: the split that
    /// produces nothing is exactly what empties the field.
    #[test]
    fn a_small_rock_dies_and_clears_the_wave_for_a_bigger_one() {
        let mut harness = one_rock_in_the_sights(60, 60, RockSize::Small, 6.0);
        harness.run_ticks(1, &[]);
        assert_eq!(harness.game.wave, 0);
        assert_eq!(harness.game.rock_count(), 1);

        harness.tap(KeyCode::Space);
        harness.run_ticks(harness.ticks + 30, &[]);

        assert_eq!(harness.game.score, RockSize::Small.score());
        assert_eq!(
            harness.game.wave, 1,
            "the field emptied and no wave followed"
        );
        let rocks = harness.game.rocks();
        assert_eq!(
            rocks.len(),
            wave_rocks(1) as usize,
            "the second wave is not the size the ramp says"
        );
        assert!(
            rocks.len() > wave_rocks(0) as usize,
            "the second wave is no bigger than the first"
        );
        assert!(rocks.iter().all(|rock| rock.size == RockSize::Large));
        harness.assert_nothing_leaked();
    }

    /// A split sends its children apart rather than both the same way, at the
    /// speed their new size drifts at.
    #[test]
    fn a_splits_children_go_different_ways_at_their_own_speed() {
        let parent = DVec3::Y;
        for counter in 0..64 {
            let a = split_velocity(DEFAULT_SEED, counter, 0, parent, RockSize::Medium);
            let b = split_velocity(DEFAULT_SEED, counter, 1, parent, RockSize::Medium);
            assert!(
                (a.length() - RockSize::Medium.speed()).abs() < 1e-9,
                "child speed was {}",
                a.length()
            );
            assert!((b.length() - RockSize::Medium.speed()).abs() < 1e-9);
            // Apart, and by at least twice the narrowest permitted angle.
            let between = a.normalize().dot(b.normalize()).clamp(-1.0, 1.0).acos();
            assert!(
                between >= 2.0 * SPLIT_ANGLE_MIN - 1e-9,
                "counter {counter}: the children left {between} radians apart"
            );
        }
    }

    // ---- lives, game over, restart -------------------------------------------

    /// Lives run out one at a time, and the third takes the game with it.
    #[test]
    fn lives_run_out_and_the_game_ends() {
        let mut harness = Harness::new(60, 60);
        harness.game.begin();
        // A rock parked on the respawn point. The ship returns into it every
        // time, which is exactly what the `RESPAWN_MAX_WAIT` escape hatch is
        // for and what makes this test finish.
        harness.game.stage_ship(DVec3::ZERO, 0.0, DVec3::ZERO);
        harness
            .game
            .stage_rock(DVec3::ZERO, DVec3::ZERO, RockSize::Large);

        harness.run_ticks(1, &[]);
        assert_eq!(
            harness.game.lives,
            STARTING_LIVES - 1,
            "the first life went"
        );
        assert!(!harness.game.ship_alive);
        assert_eq!(harness.game.state, GameState::Playing);

        harness.run_ticks(2000, &[]);
        assert_eq!(harness.game.lives, 0);
        assert_eq!(harness.game.state, GameState::GameOver);
        harness.assert_nothing_leaked();
    }

    /// A ship that has been destroyed comes back, in the middle, once the
    /// middle is clear.
    #[test]
    fn a_destroyed_ship_comes_back_to_a_clear_centre() {
        let mut harness = Harness::new(60, 60);
        harness.game.begin();
        harness
            .game
            .stage_ship(DVec3::ZERO, 1.0, DVec3::new(5.0, 5.0, 0.0));
        // A rock that hits the ship and keeps going, so the centre clears.
        harness
            .game
            .stage_rock(DVec3::ZERO, DVec3::new(9.0, 0.0, 0.0), RockSize::Small);

        harness.run_ticks(1, &[]);
        assert!(
            !harness.game.ship_alive,
            "the ship survived a rock on top of it"
        );
        assert_eq!(harness.game.lives, STARTING_LIVES - 1);

        // Not before the delay is up.
        let delay_ticks = (RESPAWN_DELAY / harness.game.tick_dt_secs()) as u64;
        harness.run_ticks(delay_ticks - 2, &[]);
        assert!(!harness.game.ship_alive, "the ship came back early");

        harness.run_ticks(delay_ticks + 60, &[]);
        assert!(harness.game.ship_alive, "the ship never came back");
        assert_eq!(
            harness.game.ship,
            DVec3::ZERO,
            "it came back somewhere else"
        );
        assert_eq!(harness.game.ship_velocity, DVec3::ZERO);
        assert_eq!(harness.game.ship_heading, 0.0);
        harness.assert_nothing_leaked();
    }

    /// A restart puts the score, the lives, the wave and the ship back, and
    /// deals a board that is not the one just played.
    #[test]
    fn a_restart_puts_everything_back_and_deals_a_new_board() {
        let mut harness = Harness::new(60, 60);
        let first: Vec<DVec3> = harness.game.rocks().iter().map(|r| r.position).collect();

        harness.play_ticks(400);
        assert!(
            harness.game.score > 0,
            "the run has to have done something first"
        );

        harness.tap(KeyCode::KeyR);
        assert_eq!(harness.game.score, 0);
        assert_eq!(harness.game.lives, STARTING_LIVES);
        assert_eq!(harness.game.wave, 0);
        assert_eq!(harness.game.state, GameState::WaitingToStart);
        assert_eq!(harness.game.ship, DVec3::ZERO);
        assert!(harness.game.ship_alive);
        assert_eq!(
            harness.game.bullet_count(),
            0,
            "a bullet survived the restart"
        );

        let second: Vec<DVec3> = harness.game.rocks().iter().map(|r| r.position).collect();
        assert_eq!(first.len(), second.len(), "the same wave should be dealt");
        assert_ne!(
            first, second,
            "a restart re-dealt the identical board, so the seed advance did nothing"
        );
        harness.assert_nothing_leaked();
    }

    /// …and it deals a *predictable* different board, so a recorded script
    /// replayed from a fresh game meets the same rocks on its second run as the
    /// recording did on its second run.
    #[test]
    fn two_games_with_one_seed_agree_about_every_board() {
        let board = |restarts: u32| {
            let mut harness = Harness::new(60, 60);
            for _ in 0..restarts {
                harness.tap(KeyCode::KeyR);
            }
            (
                harness.game.board_seed(),
                harness
                    .game
                    .rocks()
                    .iter()
                    .map(|r| r.position)
                    .collect::<Vec<_>>(),
            )
        };
        for restarts in 0..4 {
            assert_eq!(
                board(restarts),
                board(restarts),
                "run {restarts} was not reproducible"
            );
        }
        assert_ne!(board(0).0, board(1).0, "a restart did not change the board");
    }

    /// Game over is not the end: firing starts a new game.
    #[test]
    fn firing_after_a_game_over_starts_a_new_game() {
        let mut harness = Harness::new(60, 60);
        harness.game.begin();
        harness.game.stage_ship(DVec3::ZERO, 0.0, DVec3::ZERO);
        harness
            .game
            .stage_rock(DVec3::ZERO, DVec3::ZERO, RockSize::Large);
        harness.run_ticks(2000, &[]);
        assert_eq!(harness.game.state, GameState::GameOver);

        harness.tap(KeyCode::Space);
        assert_eq!(harness.game.state, GameState::WaitingToStart);
        assert_eq!(harness.game.lives, STARTING_LIVES);
        assert_eq!(harness.game.score, 0);
        assert!(harness.game.ship_alive);
    }

    // ---- determinism ---------------------------------------------------------

    /// **The determinism criterion.** The same script replays to the same
    /// score, twice, bit-identically.
    ///
    /// Everything observable is compared, not just the score: a run that agreed
    /// about the number and disagreed about where the rocks were would be a
    /// coincidence, not determinism.
    #[test]
    fn the_same_script_replays_bit_identically() {
        let run = || {
            let mut harness = Harness::new(60, 60);
            harness.play_ticks(900);
            (
                harness.game.score,
                harness.game.lives,
                harness.game.wave,
                harness.game.ship,
                harness.game.ship_velocity,
                harness.game.ship_heading,
                harness.game.rocks(),
                harness.game.bullets(),
                harness.game.rocks_spawned(),
            )
        };
        let first = run();
        assert!(first.0 > 0, "the reference run scored nothing");
        assert!(
            first.8 > u64::from(wave_rocks(0)),
            "the reference run split nothing: {} rocks ever spawned",
            first.8
        );
        assert_eq!(first, run());
    }

    /// **The frame rate is not the tick rate.** The same flight reaches the
    /// same place at 20, 60 and 240 frames a second, because the simulation
    /// runs on its own fixed step and the frame loop only decides how often it
    /// is asked to.
    #[test]
    fn the_same_flight_plays_out_the_same_at_every_frame_rate() {
        let mut reference: Option<(u32, u32, u32, DVec3, Vec<RockView>)> = None;
        for frame_hz in [20, 60, 240] {
            let mut harness = Harness::new(frame_hz, 60);
            harness.play_ticks(900);
            assert_eq!(harness.ticks, 900);
            let observed = (
                harness.game.score,
                harness.game.lives,
                harness.game.wave,
                harness.game.ship,
                harness.game.rocks(),
            );
            match &reference {
                None => {
                    assert!(observed.0 > 0, "the reference run scored nothing");
                    reference = Some(observed);
                }
                Some(expected) => {
                    assert_eq!(
                        &observed, expected,
                        "{frame_hz} fps played a different game"
                    )
                }
            }
        }
    }

    /// Two games given the same seed play the same game.
    #[test]
    fn two_games_on_one_seed_play_the_same_game() {
        for seed in [1, DEFAULT_SEED] {
            let run = || {
                let mut harness = Harness::with_seed(60, 60, seed);
                harness.play_ticks(600);
                (harness.game.score, harness.game.rocks())
            };
            assert_eq!(run(), run(), "seed {seed} was not reproducible");
        }
        assert_ne!(
            {
                let mut h = Harness::with_seed(60, 60, 1);
                h.play_ticks(600);
                h.game.rocks()
            },
            {
                let mut h = Harness::with_seed(60, 60, 2);
                h.play_ticks(600);
                h.game.rocks()
            },
            "two seeds played the same game"
        );
    }

    // ---- the cues, and the best score ----------------------------------------

    /// **Every cue this game has, fired by the thing it belongs to, counted.**
    ///
    /// Counted with [`crate::audio::Audio::plays`] rather than `voices()`, which
    /// is the trap flappy's `plays` counter was added to close: the audio thread
    /// reaps a voice as it finishes, so a queue length falls again on a clock no
    /// test controls. A headless game opens a null stream, so nothing here
    /// races.
    ///
    /// Each cue is asserted against **all three** counters, not just its own. A
    /// `fire` that pushed `SOUND_EXPLOSION` — or a `shatter` that pushed
    /// `SOUND_SHOT` — passes any version of this that only checks the count went
    /// up somewhere.
    #[test]
    fn a_shot_and_a_shatter_reach_the_audio_as_themselves() {
        use crate::audio::{SOUND_EXPLOSION, SOUND_SHOT, SOUND_THRUST};

        let mut harness = one_rock_in_the_sights(60, 60, RockSize::Small, 6.0);
        harness.run_ticks(1, &[]);
        let plays = |h: &Harness, id| h.game.audio.plays(id);
        assert_eq!(plays(&harness, SOUND_SHOT), 0, "silence before");
        assert_eq!(plays(&harness, SOUND_EXPLOSION), 0, "silence before");
        assert_eq!(plays(&harness, SOUND_THRUST), 0, "silence before");

        harness.tap(KeyCode::Space);
        assert_eq!(harness.game.bullet_count(), 1, "the shot did not fire");
        assert_eq!(plays(&harness, SOUND_SHOT), 1, "the gun was silent");
        assert_eq!(
            plays(&harness, SOUND_EXPLOSION),
            0,
            "the gun played the explosion",
        );
        assert_eq!(
            plays(&harness, SOUND_THRUST),
            0,
            "the gun played the engine"
        );

        // A small rock has no children, so it simply dies — one explosion.
        harness.run_ticks(harness.ticks + 40, &[]);
        assert_eq!(harness.game.score, RockSize::Small.score(), "no hit");
        assert_eq!(
            plays(&harness, SOUND_EXPLOSION),
            1,
            "the rock came apart in silence",
        );
        assert_eq!(
            plays(&harness, SOUND_SHOT),
            1,
            "the shatter played the gun, or the gun fired twice",
        );
        assert_eq!(
            plays(&harness, SOUND_THRUST),
            0,
            "nothing thrusted in this test",
        );
    }

    /// **The engine is a held sound, so it is one voice held**, and it stops the
    /// tick the key comes up.
    ///
    /// This used to assert a *pulse count* against `THRUST_CUE_PERIOD`, a
    /// countdown in the simulation that re-fired a one-shot, because the crate
    /// had no looping voice. It has one, the timer is gone, and what a burn
    /// leaves behind is a single sounding voice rather than a stream of plays —
    /// so both halves are checked: exactly one play, and the voice still there
    /// a whole second later. A per-tick or per-period re-fire fails the first;
    /// the old one-shot fails the second, because its buffer is a tenth of a
    /// second long.
    #[test]
    fn the_engine_is_one_held_voice_while_thrust_is_down_and_stops_when_it_is_not() {
        use crate::audio::SOUND_THRUST;

        let mut harness = Harness::new(60, 60);
        harness.game.begin();
        harness.game.stage_ship(DVec3::ZERO, 0.0, DVec3::ZERO);
        // Off the ship, so nothing collides while it flies.
        harness.game.stage_rock(
            DVec3::new(WORLD_HALF_WIDTH - 1.0, WORLD_HALF_HEIGHT - 1.0, 0.0),
            DVec3::ZERO,
            RockSize::Small,
        );
        assert!(!harness.game.audio.thrust_playing(), "silence before");

        let ticks = 60u64;
        harness.game.key_event(KeyCode::ArrowUp, true);
        harness.run_ticks(harness.ticks + ticks, &[]);
        assert!(
            harness.game.thrusting,
            "a whole second of held thrust and the ship is not burning",
        );
        assert!(
            harness.game.audio.thrust_playing(),
            "the engine is not sounding a second into the burn",
        );
        assert_eq!(
            harness.game.audio.plays(SOUND_THRUST),
            1,
            "the burn is being re-fired rather than held",
        );

        harness.game.key_event(KeyCode::ArrowUp, false);
        harness.run_ticks(harness.ticks + 1, &[]);
        assert!(!harness.game.thrusting, "the ship is still burning");
        assert!(
            !harness.game.audio.thrust_playing(),
            "the engine kept running after the key came up",
        );

        // And nothing restarts it on its own, however long the game runs.
        harness.run_ticks(harness.ticks + 60, &[]);
        assert!(!harness.game.audio.thrust_playing());
        assert_eq!(harness.game.audio.plays(SOUND_THRUST), 1);

        // **The voice is gone, not merely forgotten.** `thrust_playing` reads
        // the handle this side kept; a release that dropped the handle without
        // stopping the voice would answer "silent" above and leave a loop
        // running for the rest of the process. Nothing but a loop survives a
        // second of mixing.
        assert_eq!(
            harness.game.audio.voices_after_a_second(),
            0,
            "the engine voice is still in the mixer",
        );
    }

    /// **The engine dies with the ship**, in the tick the ship does — not on the
    /// next one, which would be a tick of engine noise over the wreck.
    #[test]
    fn the_engine_stops_the_tick_the_ship_is_destroyed() {
        let mut harness = Harness::new(60, 60);
        harness.game.begin();
        harness.game.stage_ship(DVec3::ZERO, 0.0, DVec3::ZERO);
        harness.game.key_event(KeyCode::ArrowUp, true);
        harness.run_ticks(harness.ticks + 5, &[]);
        assert!(harness.game.audio.thrust_playing(), "no engine to stop");

        // A rock right on top of the ship, with the key still held.
        harness
            .game
            .stage_rock(DVec3::ZERO, DVec3::ZERO, RockSize::Small);
        harness.run_ticks(harness.ticks + 1, &[]);
        assert!(!harness.game.ship_alive, "the ship survived the rock");
        assert!(
            !harness.game.audio.thrust_playing(),
            "the wreck is still under power",
        );
        // The explosion the death raised does not survive a second of mixing; a
        // leaked engine loop would. See `Audio::voices_after_a_second`.
        assert_eq!(
            harness.game.audio.voices_after_a_second(),
            0,
            "the engine voice outlived the ship",
        );
    }

    /// **A cue is raised where the thing happened**, not at the origin.
    ///
    /// Without this, every `play_at` call could pass `(0, 0)` and the two tests
    /// above would still pass — the sample would ship a spatial grammar fed a
    /// constant. `crate::audio`'s `where_a_cue_happens_changes_how_it_sounds`
    /// covers the other half: that the grammar does something with it.
    #[test]
    fn a_cue_carries_the_position_of_the_thing_that_raised_it() {
        use crate::audio::{SOUND_EXPLOSION, SOUND_SHOT};

        // A corner of the field, as far from the listener at the origin as this
        // game gets — so "it is where the ship is" and "it is not at (0, 0)" are
        // two different assertions rather than the same one.
        let corner = DVec3::new(-(WORLD_HALF_WIDTH - 3.0), WORLD_HALF_HEIGHT - 4.0, 0.0);
        let mut harness = Harness::new(60, 60);
        harness.game.begin();
        harness.game.stage_ship(corner, 0.0, DVec3::ZERO);
        harness.game.stage_rock(
            corner + DVec3::new(0.0, 3.0, 0.0),
            DVec3::ZERO,
            RockSize::Small,
        );
        harness.run_ticks(1, &[]);

        harness.tap(KeyCode::Space);
        harness.run_ticks(harness.ticks + 40, &[]);
        assert_eq!(harness.game.score, RockSize::Small.score(), "no hit");

        let played = harness.game.audio.played().to_vec();
        for (id, near) in [
            (SOUND_SHOT, corner),
            (SOUND_EXPLOSION, corner + DVec3::Y * 3.0),
        ] {
            let (_, x, y) = played
                .iter()
                .find(|(played_id, _, _)| *played_id == id)
                .copied()
                .unwrap_or_else(|| panic!("cue {id} was never played: {played:?}"));
            assert!(
                (x - near.x).abs() < 1.5 && (y - near.y).abs() < 1.5,
                "cue {id} was heard at ({x:.2}, {y:.2}) and happened at {near:?}",
            );
            // The listener is at the origin, so a cue that lost its position
            // would land exactly there and the check above would still pass for
            // a game played in the middle of the field. This one would not.
            assert!(
                x.abs() > 1.0 && y.abs() > 1.0,
                "cue {id} is at the origin, so nothing is carrying a position",
            );
        }
    }

    /// **The best score is taken when a game ends, once**, and a worse game does
    /// not lower it.
    ///
    /// Headless, so it is held in memory and the test writes nothing — which is
    /// itself asserted in `crate::best`'s own suite.
    #[test]
    fn the_best_score_is_taken_when_a_game_ends() {
        let mut harness = one_rock_in_the_sights(60, 60, RockSize::Small, 6.0);
        assert_eq!(harness.game.best.get(), 0);
        harness.run_ticks(1, &[]);

        // Score something, and check it is *not* yet recorded: a best score
        // taken every tick would already be there.
        harness.tap(KeyCode::Space);
        harness.run_ticks(harness.ticks + 40, &[]);
        assert_eq!(harness.game.score, RockSize::Small.score(), "no hit");
        assert_eq!(harness.game.state, GameState::Playing);
        assert_eq!(
            harness.game.best.get(),
            0,
            "a game still in progress is not a best score",
        );

        // A rock parked on the respawn point ends the game, the way
        // `lives_run_out_and_the_game_ends` does it: the ship returns into it
        // every time, and `RESPAWN_MAX_WAIT` is what keeps that finite.
        harness.game.stage_ship(DVec3::ZERO, 0.0, DVec3::ZERO);
        harness
            .game
            .stage_rock(DVec3::ZERO, DVec3::ZERO, RockSize::Large);
        harness.run_ticks(harness.ticks + 2_000, &[]);
        assert_eq!(
            harness.game.state,
            GameState::GameOver,
            "the game never ended"
        );
        let ended_with = harness.game.score;
        assert!(ended_with > 0, "the game ended on a score of nothing");
        assert_eq!(
            harness.game.best.get(),
            ended_with,
            "the run's score never reached the best",
        );

        // A restart, and a worse game must not lower it.
        harness.tap(KeyCode::KeyR);
        assert_eq!(harness.game.score, 0, "the restart kept the score");
        assert_eq!(
            harness.game.best.get(),
            ended_with,
            "the restart cleared the best score",
        );
    }

    /// **The score freezes the tick the game ends.** A bullet still in flight
    /// when the ship dies keeps flying behind the game-over panel, and before
    /// the fix the ungated sweep kept resolving it — shattering the rock it was
    /// aimed at and adding score that `best.raise`, which runs only on the edge
    /// into `GameOver`, never saw.
    #[test]
    fn score_is_frozen_from_the_tick_the_game_ends() {
        let mut harness = Harness::new(60, 60);
        harness.game.begin();
        harness.game.stage_ship(DVec3::ZERO, 0.0, DVec3::ZERO);
        // A rock that kills the ship on every respawn. Off the gun's line (the
        // ship faces up, so the line is +Y), so a shot fired up the line never
        // touches it.
        harness
            .game
            .stage_rock(DVec3::new(0.5, 0.0, 0.0), DVec3::ZERO, RockSize::Small);

        // Spend two lives: the ship returns into the killer rock, and
        // `RESPAWN_MAX_WAIT` is what keeps each cycle finite.
        while harness.game.lives > 1 {
            harness.run_ticks(harness.ticks + 1, &[]);
        }
        assert!(!harness.game.ship_alive, "the second life never went");

        // Clear the field and park the real target up the gun's line. The
        // centre is clear again, so the ship comes back.
        harness
            .game
            .stage_rock(DVec3::new(0.0, 8.0, 0.0), DVec3::ZERO, RockSize::Small);
        harness.run_ticks(harness.ticks + 300, &[]);
        assert!(harness.game.ship_alive, "the ship never came back");
        assert_eq!(harness.game.lives, 1);

        // Fire up the line: a bullet now in flight towards the rock at (0, 8),
        // which takes ~18 ticks to reach. Kill the ship before it arrives.
        harness.tap(KeyCode::Space);
        assert_eq!(harness.game.bullet_count(), 1, "nothing was fired");
        harness
            .game
            .stage_another_rock(DVec3::new(0.5, 0.0, 0.0), DVec3::ZERO, RockSize::Small);

        harness.run_ticks(harness.ticks + 1, &[]);
        assert_eq!(
            harness.game.state,
            GameState::GameOver,
            "the game never ended"
        );
        let score_at_death = harness.game.score;
        assert_eq!(score_at_death, 0, "a rock was broken before the game ended");
        assert!(
            harness.game.bullet_count() > 0,
            "no bullet was in flight at the moment of death",
        );

        // More than enough ticks for the in-flight bullet to have reached the
        // rock at (0, 8) and shattered it.
        harness.run_ticks(harness.ticks + 100, &[]);
        assert_eq!(harness.game.state, GameState::GameOver);
        assert_eq!(
            harness.game.score, score_at_death,
            "the score kept counting after the game ended",
        );
        assert!(
            harness.game.score <= harness.game.best.get(),
            "the score outgrew the best, which was recorded on the edge into \
             GameOver",
        );
        assert_eq!(
            harness.game.rock_count(),
            2,
            "the rock the in-flight bullet was aimed at was shattered after death",
        );
        harness.assert_nothing_leaked();
    }

    /// The best score reaches the renderer, so the HUD can show it.
    #[test]
    fn the_best_score_reaches_the_render_state() {
        let mut harness = Harness::new(60, 60);
        harness.game.best.raise(1_234);
        let mut render = RenderState::default();
        harness.game.render_state(&mut render);
        assert_eq!(render.best, 1_234);
    }

    // ---- the debug panel's field section --------------------------------------

    /// **A known field renders known rows.** Built by hand, with nine values no
    /// two of which are equal, so a row that reported its neighbour's number
    /// cannot pass — which counting a real board could not rule out, since a
    /// fresh one has three of these at zero.
    #[test]
    fn the_field_section_renders_the_counts_it_was_given() {
        use crcbl::ui::DebugModule as _;

        let stats = FieldStats {
            large: 2,
            medium: 3,
            small: 5,
            bullets: 4,
            rocks_spawned: 61,
            bullets_fired: 97,
            entities: 15,
            pending_despawns: 1,
            colliders: 10,
        };
        let mut section = crcbl::ui::DebugSection::new("field");
        stats.debug_section(&mut section);
        assert_eq!(section.title(), "field");
        assert_eq!(
            section.rows(),
            &[
                row("rocks", "2/3/5"),
                row("bullets", "4"),
                row("spawned", "61"),
                row("fired", "97"),
                row("entities", "15"),
                row("despawns", "1"),
                row("colliders", "10"),
            ],
            "the field section is exactly these seven rows",
        );
    }

    /// **And the rows follow the churn.** A few seconds of the autopilot fires
    /// bullets and splits rocks, so every counter here moves — and the entity
    /// row keeps agreeing with [`Harness::assert_nothing_leaked`]'s accounting,
    /// which is the point of putting these particular numbers on screen.
    #[test]
    fn playing_moves_the_field_sections_rows_and_they_still_account_for_the_world() {
        use crcbl::ui::DebugModule as _;

        let mut harness = Harness::new(60, 60);
        let opening = harness.game.field_stats();
        assert!(opening.large > 0, "the opening wave is all large rocks");
        assert_eq!((opening.medium, opening.small), (0, 0));
        assert_eq!((opening.bullets, opening.bullets_fired), (0, 0));
        assert_eq!(opening.rocks_spawned, opening.large as u64);

        harness.play_ticks(900);
        let played = harness.game.field_stats();
        assert!(
            played.bullets_fired > 0,
            "fifteen seconds and the autopilot never shot",
        );
        assert!(
            played.rocks_spawned > opening.rocks_spawned,
            "nothing ever split: {} then {}",
            opening.rocks_spawned,
            played.rocks_spawned,
        );
        assert!(
            played.medium + played.small > 0,
            "no rock was ever broken, so the size split says nothing",
        );

        // The same sum `assert_nothing_leaked` asserts, read off the rows the
        // panel draws — so a leak would be visible on screen and not only in
        // this suite.
        assert_eq!(
            played.entities,
            1 + played.large
                + played.medium
                + played.small
                + played.bullets
                + played.pending_despawns,
            "the entity row does not account for what is on the field",
        );
        assert_eq!(
            played.colliders,
            played.large + played.medium + played.small,
            "the collider row does not account for the rocks",
        );

        let mut section = crcbl::ui::DebugSection::new("field");
        played.debug_section(&mut section);
        assert_eq!(section.rows().len(), 7);
        assert_eq!(
            section.rows()[0].value,
            format!("{}/{}/{}", played.large, played.medium, played.small),
        );
        assert_eq!(section.rows()[3].value, played.bullets_fired.to_string());
        assert_ne!(
            section.rows()[3].value,
            "0",
            "the fired row never left zero",
        );
    }

    /// One expected row, spelled once.
    fn row(label: &str, value: &str) -> crcbl::ui::DebugRow {
        crcbl::ui::DebugRow {
            label: label.into(),
            value: value.into(),
        }
    }

    // ---- the entity lifecycle, which is what this sample is for ---------------

    /// **Hundreds of spawns and deaths, and nothing leaks.**
    ///
    /// `docs/plan/sample/02-asteroids.md` says this is what the sample exists to
    /// justify: bullets at four a second, two rocks for every one shot, a wave
    /// at a time, and a restart that wipes the field — all of it hammering
    /// generational ids, deferred destruction and pool slot recycling.
    ///
    /// The invariant is checked on **every** tick rather than at the end,
    /// because a leak that is cleaned up before the last tick is still a leak
    /// while it is happening, and because the failure names the tick it started
    /// on. The churn counters at the bottom are what stop it passing vacuously:
    /// a game that spawned nothing would satisfy every assertion above them.
    #[test]
    fn hundreds_of_spawns_and_deaths_leak_nothing() {
        let mut harness = Harness::new(60, 60);
        let mut restarts = 0;
        let mut peak_entities = 0;

        while harness.ticks < 18_000 {
            harness.play_ticks(harness.ticks + 1);
            harness.assert_nothing_leaked();
            peak_entities = peak_entities.max(harness.game.entity_count());
            // A finished game is restarted rather than left sitting, so the
            // soak keeps churning — and a restart is itself the largest single
            // piece of churn in the game.
            if harness.game.state == GameState::GameOver {
                harness.tap(KeyCode::KeyR);
                restarts += 1;
            }
        }

        let spawned = harness.game.rocks_spawned();
        let fired = harness.game.bullets_fired();
        assert!(
            spawned >= 300,
            "only {spawned} rocks were ever spawned, which is not enough churn to test"
        );
        assert!(
            fired >= 1_000,
            "only {fired} bullets were ever fired, which is not enough churn to test"
        );
        assert!(
            restarts > 0 || harness.game.wave >= 3,
            "the soak cleared only {} waves and finished no game, so neither the \
             wave respawn nor the restart path was exercised",
            harness.game.wave,
        );
        // The most the world can legitimately hold: the ship, a full magazine,
        // every rock of the largest wave broken all the way down to smalls, and
        // — because destruction is deferred by a tick — as many again waiting
        // for the sweep on the tick a restart wipes the field. Derived rather
        // than measured: a number taken from a passing run breaks on the next
        // tuning change for a reason that is not a leak.
        let ceiling = 2 * (1 + MAX_BULLETS + 4 * MAX_WAVE_ROCKS as usize);
        assert!(
            peak_entities <= ceiling,
            "the world peaked at {peak_entities} entities against a ceiling of {ceiling}"
        );
        assert!(
            peak_entities > 1 + wave_rocks(0) as usize,
            "the world never grew past its opening board, so the ceiling proves nothing"
        );

        // The queue drains: a few quiet ticks and the pool has let go of
        // everything, which is what "back to baseline" means for a deferred
        // sweep.
        harness.run_ticks(harness.ticks + 3, &[]);
        assert_eq!(
            harness.game.pending_despawns(),
            0,
            "the destruction queue never emptied"
        );
        // And the final state is accounted for exactly, entity for entity.
        harness.assert_nothing_leaked();
        crcbl::log::info!(
            "soak: {spawned} rocks, {fired} bullets, {restarts} restarts, \
             peak {peak_entities} entities"
        );
    }

    /// A game left alone forever does not grow.
    ///
    /// The complement of the soak above: that one churns hard and checks
    /// nothing is left behind; this one checks that a game with *nothing*
    /// happening does not accumulate either — the failure mode where a tick
    /// re-registers something it already had.
    #[test]
    fn an_idle_game_stays_exactly_the_size_it_started() {
        let mut harness = Harness::new(60, 60);
        harness.run_ticks(1, &[]);
        let entities = harness.game.entity_count();
        let colliders = harness.game.collider_count();
        assert_eq!(entities, 1 + wave_rocks(0) as usize);
        assert_eq!(colliders, wave_rocks(0) as usize);

        harness.run_ticks(600, &[]);
        assert_eq!(harness.game.entity_count(), entities);
        assert_eq!(harness.game.collider_count(), colliders);
    }

    // -----------------------------------------------------------------------
    // Angles: the wrap, and the interpolation the renderer runs on
    // -----------------------------------------------------------------------

    /// **The short way round, across the seam.**
    ///
    /// The case the renderer exists to get right: a heading that has just
    /// crossed zero has a previous value near τ and a current one near zero, and
    /// a plain `from + (to - from) * α` sends the ship the long way round —
    /// 350° to 10° through 180° instead of through 0°. The **intermediate**
    /// value is what says which route was taken; the endpoints are identical
    /// either way, so a test that only checked those would pass on the bug.
    #[test]
    fn an_angle_interpolates_the_short_way_across_the_wrap() {
        let deg = |d: f64| d.to_radians();
        let from = deg(350.0);
        let to = deg(10.0);

        // Quarter, half and three-quarters of the way. The short arc is 20°, so
        // the midpoint is 0° — the naive lerp would put it at 180°.
        for (alpha, expected) in [(0.25, 355.0), (0.5, 0.0), (0.75, 5.0)] {
            let got = lerp_angle(from, to, alpha);
            assert!(
                wrap_to_pi(got - deg(expected)).abs() < 1e-12,
                "at alpha = {alpha} the short way is {expected} degrees, this went to {}",
                got.to_degrees(),
            );
            // And explicitly: nowhere near the long way's midpoint.
            assert!(
                wrap_to_pi(got - std::f64::consts::PI).abs() > deg(90.0),
                "alpha = {alpha} took the long way round: {}",
                got.to_degrees(),
            );
        }

        // The other direction across the same seam, so the sign is not being
        // read off one example.
        let back = lerp_angle(to, from, 0.5);
        assert!(wrap_to_pi(back).abs() < 1e-12, "{}", back.to_degrees());

        // The endpoints are the endpoints, modulo tau, and alpha is clamped
        // rather than extrapolated.
        for (alpha, expected) in [(0.0, from), (1.0, to), (-3.0, from), (7.0, to)] {
            assert!(
                wrap_to_pi(lerp_angle(from, to, alpha) - expected).abs() < 1e-12,
                "alpha = {alpha}",
            );
        }
    }

    /// A plain interval interpolates plainly — the wrap handling must not bend
    /// an angle that never crosses the seam.
    #[test]
    fn an_angle_that_does_not_wrap_interpolates_straight_through() {
        for (from, to) in [(0.5, 1.5), (2.0, 1.0), (-0.4, 0.4), (3.0, 3.1)] {
            for alpha in [0.0, 0.2, 0.5, 0.9, 1.0] {
                let expected = from + (to - from) * alpha;
                let got = lerp_angle(from, to, alpha);
                assert!(
                    wrap_to_pi(got - expected).abs() < 1e-12,
                    "{from} to {to} at {alpha} gave {got}, not {expected}",
                );
            }
        }
    }

    /// **`wrap_to_pi` is the half-open interval it says it is**, which is what
    /// makes the midpoint above exact rather than nearly right.
    #[test]
    fn wrapping_lands_in_the_half_open_interval() {
        use std::f64::consts::{PI, TAU};
        for angle in [0.0, 0.3, PI, -PI, TAU, TAU + 0.3, -0.3, -TAU - 0.3, 17.0] {
            let wrapped = wrap_to_pi(angle);
            assert!(
                wrapped > -PI && wrapped <= PI,
                "{angle} wrapped to {wrapped}",
            );
            let turns = (wrapped - angle) / TAU;
            assert!(
                (turns - turns.round()).abs() < 1e-12,
                "{angle} wrapped to {wrapped}, which is a different angle",
            );
        }
        assert_eq!(wrap_to_pi(PI), PI, "pi stays pi; it is the closed end");
        assert_eq!(wrap_to_pi(-PI), PI, "minus pi is the same angle as pi");
        assert_eq!(wrap_to_pi(TAU), 0.0);
    }

    /// **Rocks tumble, each its own way, and the previous angle trails the
    /// current one by exactly one tick.**
    ///
    /// The three things the renderer's interpolation rests on: that the angle
    /// moves at all, that `prev_angle` is the value from the tick before rather
    /// than a copy of this one, and that the pair is the same on two runs of one
    /// seed — a tumble drawn from a running RNG would replay differently and
    /// take the determinism suite with it.
    #[test]
    fn every_rock_tumbles_its_own_way_and_the_same_way_on_a_replay() {
        let mut harness = Harness::new(60, 60);
        harness.game.tick();
        let first = harness.game.rocks();
        assert_eq!(first.len(), wave_rocks(0) as usize);

        harness.game.tick();
        let second = harness.game.rocks();
        let mut steps = Vec::new();
        for (before, now) in first.iter().zip(&second) {
            assert_eq!(
                now.prev_angle, before.angle,
                "prev_angle is not the previous tick's value",
            );
            let step = wrap_to_pi(now.angle - now.prev_angle);
            assert!(step.abs() > 1e-6, "a rock did not turn at all");
            let expected = before.size.spin() / f64::from(DEFAULT_TICK_HZ);
            assert!(
                (step.abs() - expected).abs() < 1e-9,
                "a {:?} rock turned {step} rather than plus or minus {expected}",
                before.size,
            );
            steps.push(step);
        }
        assert!(
            steps.iter().any(|s| *s > 0.0) && steps.iter().any(|s| *s < 0.0),
            "every rock in the wave turns the same way: {steps:?}",
        );
        assert!(
            first.iter().any(|r| r.angle != first[0].angle),
            "every rock in the wave started at the same attitude",
        );

        // And the whole of it replays: the same seed, the same tumble.
        let mut replay = Harness::new(60, 60);
        replay.game.tick();
        replay.game.tick();
        assert_eq!(
            replay.game.rocks(),
            second,
            "the tumble is not deterministic",
        );

        // A different board tumbles differently, so the seed really reaches it.
        let mut other = Harness::with_seed(60, 60, DEFAULT_SEED ^ 0x5EED);
        other.game.tick();
        other.game.tick();
        assert_ne!(
            other
                .game
                .rocks()
                .iter()
                .map(|r| r.angle)
                .collect::<Vec<_>>(),
            second.iter().map(|r| r.angle).collect::<Vec<_>>(),
        );
    }

    /// A rock crosses the seam while it tumbles, which is what makes the
    /// renderer's short-way interpolation load-bearing rather than theoretical.
    ///
    /// Caught as a step of more than a radian in one tick: no spin rate in the
    /// game is anywhere near that, so the only thing it can be is the wrap.
    #[test]
    fn a_tumbling_rock_crosses_the_seam() {
        let mut harness = Harness::new(60, 60);
        harness.game.tick();
        let mut wrapped = 0;
        // A large rock at 0.55 rad/s takes about 11.5 s for a revolution, so
        // twelve seconds is enough for every rock to have crossed zero once
        // whatever attitude it started at.
        for _ in 0..720 {
            harness.game.tick();
            for rock in harness.game.rocks() {
                if (rock.angle - rock.prev_angle).abs() > 1.0 {
                    wrapped += 1;
                }
            }
        }
        assert!(
            wrapped > 0,
            "no rock crossed zero in twelve seconds, so the wrap is untested",
        );
        for rock in harness.game.rocks() {
            assert!(
                (0.0..std::f64::consts::TAU).contains(&rock.angle),
                "a rock's angle escaped [0, tau): {}",
                rock.angle,
            );
        }
    }

    /// The ship's previous heading trails by one tick while it turns, and is
    /// **reset rather than carried** when the ship is placed back facing up — a
    /// renderer interpolating from the heading the last one died on would spin
    /// the new ship through the difference over a single frame.
    #[test]
    fn the_ships_previous_heading_trails_a_tick_and_resets_when_it_is_placed() {
        let mut harness = Harness::new(60, 60);
        harness.tap(KeyCode::Space);
        assert_eq!(harness.game.state, GameState::Playing);

        harness.game.key_event(KeyCode::ArrowLeft, true);
        let before = harness.game.ship_heading;
        harness.game.tick();
        assert_eq!(
            harness.game.ship_heading_prev, before,
            "prev is not the heading from the tick before",
        );
        assert!(
            harness.game.ship_heading > harness.game.ship_heading_prev,
            "a held turn left prev and current equal",
        );

        // The render state carries the same pair the mirrors do, which is what
        // `art::Scene::build` actually reads.
        let mut render = RenderState::default();
        harness.game.render_state(&mut render);
        assert_eq!(render.ship_heading, harness.game.ship_heading);
        assert_eq!(render.ship_heading_prev, harness.game.ship_heading_prev);

        for _ in 0..60 {
            harness.game.tick();
        }
        assert!(
            harness.game.ship_heading != 0.0,
            "the ship never turned, so the reset below proves nothing",
        );

        // A restart runs `place_ship`, which *assigns* the heading. Both halves
        // have to land on it.
        harness.game.key_event(KeyCode::ArrowLeft, false);
        harness.tap(KeyCode::KeyR);
        assert_eq!(harness.game.ship_heading, 0.0);
        assert_eq!(
            harness.game.ship_heading_prev, 0.0,
            "a fresh ship would be drawn spinning out of the heading the last \
             one was flying",
        );
    }

    /// The ship's previous position trails by one tick while it moves, and is
    /// **reset rather than carried** when the ship is placed back at the
    /// origin — a renderer interpolating from where the last ship died would
    /// drag the new one across the field over a single frame.
    #[test]
    fn the_ships_previous_position_trails_a_tick_and_resets_when_it_is_placed() {
        let mut harness = Harness::new(60, 60);
        harness.tap(KeyCode::Space);
        assert_eq!(harness.game.state, GameState::Playing);
        // A clean board: the ship at the origin and one rock parked in a far
        // corner, so the ship can thrust without the field emptying and the
        // wave machinery running.
        harness.game.stage_ship(DVec3::ZERO, 0.0, DVec3::ZERO);
        harness.game.stage_rock(
            DVec3::new(WORLD_HALF_WIDTH - 2.0, WORLD_HALF_HEIGHT - 2.0, 0.0),
            DVec3::ZERO,
            RockSize::Small,
        );

        harness.game.key_event(KeyCode::ArrowUp, true);
        // The first thrust tick applies the force; physics steps it at the
        // start of the *next* tick, so run one tick to arm the engine and then
        // watch the pair across the tick that actually moves.
        harness.game.tick();
        let mut render = RenderState::default();
        harness.game.render_state(&mut render);
        let before = render.ship;
        harness.game.tick();
        harness.game.render_state(&mut render);
        assert_eq!(
            render.ship_prev_pos, before,
            "prev is not the position from the tick before",
        );
        assert_ne!(
            render.ship, render.ship_prev_pos,
            "a held thrust left prev and current equal",
        );

        // A few more ticks of thrust, so the reset below has a distance to
        // undo rather than a rounding error.
        for _ in 0..10 {
            harness.game.tick();
        }
        harness.game.render_state(&mut render);
        assert!(
            render.ship != DVec3::ZERO,
            "the ship never moved, so the reset below proves nothing",
        );

        // A restart runs `place_ship`, which *assigns* the position. Both
        // halves have to land on it.
        harness.game.key_event(KeyCode::ArrowUp, false);
        harness.tap(KeyCode::KeyR);
        harness.game.render_state(&mut render);
        assert_eq!(render.ship, DVec3::ZERO);
        assert_eq!(
            render.ship_prev_pos,
            DVec3::ZERO,
            "a fresh ship would be drawn sweeping in from where the last one died",
        );
        assert!(
            !render.ship_teleported,
            "a restarted ship inherited a stale teleport flag",
        );
    }

    /// **The tick a rock wraps is the tick its view is flagged teleported**,
    /// and a rock that stayed inside is not — with the unwrapped one's
    /// previous position trailing by exactly one tick.
    ///
    /// The flag is the renderer's whole defence against lerping a wrapped body
    /// back across the field, so it has to be set on the wrap tick itself and
    /// clear on the next one.
    #[test]
    fn a_wrapped_rock_is_flagged_teleported_on_that_tick_and_unwrapped_ones_are_not() {
        let mut harness = Harness::new(60, 60);
        harness.game.begin();
        harness.game.stage_ship(DVec3::ZERO, 0.0, DVec3::ZERO);
        // One rock crossing the right edge on the first tick — 8 u/s at 60 Hz
        // is 0.13 of a unit of travel from 0.1 short of the edge — and one
        // moving slowly inside the field, which must never be flagged.
        harness.game.stage_rock(
            DVec3::new(WORLD_HALF_WIDTH - 0.1, 0.0, 0.0),
            DVec3::new(8.0, 0.0, 0.0),
            RockSize::Small,
        );
        harness.game.stage_another_rock(
            DVec3::new(-8.0, -6.0, 0.0),
            DVec3::new(1.5, 0.0, 0.0),
            RockSize::Large,
        );

        harness.game.tick();
        let first = harness.game.rocks();
        let wrapped = first
            .iter()
            .find(|r| r.teleported)
            .expect("the rock that crossed the edge was not flagged teleported");
        assert!(
            wrapped.position.x < 0.0,
            "the flagged rock never crossed: {:?}",
            wrapped.position,
        );
        // The pair spans the field on the wrap tick — exactly why the flag,
        // and not a lerp, is what the renderer has to see.
        assert_eq!(
            wrapped.prev_position,
            DVec3::new(WORLD_HALF_WIDTH - 0.1, 0.0, 0.0),
            "the flagged rock's previous position is not where it crossed from",
        );
        let calm = first
            .iter()
            .find(|r| !r.teleported)
            .expect("every rock on the field was flagged");
        assert!(
            calm.position.x.abs() < WORLD_HALF_WIDTH - 1.0,
            "the 'unwrapped' rock actually crossed: {:?}",
            calm.position,
        );

        harness.game.tick();
        let second = harness.game.rocks();
        for now in &second {
            assert!(
                !now.teleported,
                "the teleport flag did not clear on the tick after the wrap",
            );
        }
        // And the pair trails: each rock's previous position is the previous
        // tick's current one, the wrapped rock included.
        for (before, now) in first.iter().zip(&second) {
            assert_eq!(
                now.prev_position, before.position,
                "a rock's previous position is not the previous tick's",
            );
        }
    }

    /// **A freshly spawned rock and a freshly fired bullet interpolate to
    /// themselves on the first frame** — `prev == cur`, so the renderer draws
    /// them where they are rather than sweeping them in from somewhere.
    #[test]
    fn a_new_rock_and_bullet_interpolate_to_themselves_on_the_first_frame() {
        let mut harness = Harness::new(60, 60);
        // The wave is dealt at construction, before any tick has moved it.
        let rocks = harness.game.rocks();
        assert!(!rocks.is_empty(), "no wave was dealt");
        for rock in &rocks {
            assert_eq!(
                rock.prev_position, rock.position,
                "a fresh rock's previous position is not its own",
            );
            assert!(!rock.teleported, "a fresh rock is flagged teleported");
        }

        harness.tap(KeyCode::Space);
        let bullets = harness.game.bullets();
        assert!(!bullets.is_empty(), "the first shot did not fire");
        for bullet in &bullets {
            assert_eq!(
                bullet.prev_position, bullet.position,
                "a fresh bullet's previous position is not its own",
            );
            assert!(!bullet.teleported, "a fresh bullet is flagged teleported");
        }
    }
}
