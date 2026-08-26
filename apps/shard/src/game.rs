//! The simulation: one capsule walking [`crate::zone`], and the server that owns
//! it.
//!
//! ```text
//!  Stage ──▶ ShardModule ──▶ Server ──┐                     ┌──▶ Client
//!  (this file)                        └── InMemoryTransport ┘
//!                     │
//!                     └──▶ RenderState ──▶ crate::app, crate::page, crate::gpu
//! ```
//!
//! # Two verbs, and they are *explore* and *fight*
//!
//! `docs/plan/sample/15-shard.md`'s milestone 1 is "explore, fight, loot, level,
//! save, resume". This file is the first two of those and nothing else: there is
//! no item, no rarity, no experience and no inventory. What there is is a
//! character, a zone with stone in it, gravity, three archetypes of foe with one
//! ability each, and a blow that answers them. `docs/backlog.md` carries the
//! rest with what each would take.
//!
//! Save and resume are the other two verbs this sample now has, and neither is
//! in here: [`crate::save`] owns the format and the platform, and what this file
//! contributes is `Stage::restore` and `Stage::snapshot` — the two functions
//! that turn the stage into a save's payload and back, under the same lock every
//! other reader of the stage takes.
//!
//! # Nothing here does collision, and that is rule 9
//!
//! Every metre the character moves goes through
//! [`CharacterController::move_and_slide`], which sweeps the capsule against
//! [`zone::world`] and slides it along what it hits. Every foe moves through the
//! *same* call against the *same* world — see [`crate::foe`] — and every
//! sighting and every blow is one [`crcbl::phys::PhysicsWorld::cast_ray`]. This
//! file decides **where from**, **which way** and **what a hit costs**; the
//! world decides what is there.
//!
//! # The character carries no collider, and that is deliberate
//!
//! A foe's sighting ray and the character's own cleave both leave the
//! character's capsule centre, and a collider there would be the first thing
//! either of them hit. `apps/breach/src/game.rs` makes the same choice for the
//! same reason, and `docs/backlog.md` records what closes it: a `cast_ray` that
//! can exclude one collider, which is an engine change and now has two callers
//! that would use it. The visible cost is the same one breach records — a foe
//! walks through the character rather than being stopped by them, while the
//! character *is* stopped by a foe, because the foes' bodies are in the world
//! and the character's is not.
//!
//! # It is a real client/server sample
//!
//! `docs/plan/sample/00-samples-overview.md` rule 2 has no exemption for a
//! single-player slice: the walk is a [`crcbl::ecs::GameModule`] the
//! authoritative server owns, stepped on the fixed timestep, with a client on
//! the other end of an `InMemoryTransport`. The camera is the one thing that is
//! **not** on that side, because it is presentation — and what crosses the wire
//! from it is the bearing the player was looking along when they walked.
//!
//! Note that milestone 1 ships **no networking at all** beyond this loopback,
//! and the plan says so in as many words: the shared world is milestone 2's job,
//! on native. The loopback is rule 2, not a network.
//!
//! # The lights are not in here
//!
//! [`crate::light`] is what decides how bright a torch is, and it is a function
//! of the simulated seconds `Stage::elapsed` accumulates a tick
//! at a time. So a paused zone's flames hold still, and two runs of the same
//! length are lit identically; but nothing about a light crosses the wire,
//! because a light is not something the server owns.

use std::sync::{Arc, Mutex, MutexGuard};
use std::time::Duration;

use crcbl::ecs::{ClientInputs, GameModule, World};
use crcbl::math::DVec3;
use crcbl::net::ProtocolCompatibility;
use crcbl::phys::{CharacterConfig, CharacterController, MoveOutcome, PhysicsWorld};
use crcbl::session::Loopback;

use crate::camera::walk_direction;
use crate::foe::{self, Foe, FoeView, Kind};
use crate::zone;

/// Distinct from every other sample's, because they are distinct protocols: a
/// client built for one must not hand-shake with a server running another. The
/// low half spells `SHD`.
const COMPATIBILITY: ProtocolCompatibility = ProtocolCompatibility {
    protocol_version: 1,
    engine_build_id: 0x0043_5243_424C,
    schema_hash: 0x0000_0053_4844,
};

/// The default simulation rate. Reaches the server, the client and the stage, so
/// there is exactly one rate in the process.
pub const DEFAULT_TICK_HZ: u32 = 60;

/// How fast the character walks, in metres a second.
///
/// The genre's pace: brisk enough that crossing the zone is not a chore and slow
/// enough that a visitor can read the room they are crossing, which is what this
/// sample is for.
pub const WALK_SPEED: f64 = 4.2;

/// Gravity, in metres per second squared.
///
/// Integrated into a fall speed rather than applied as a fixed displacement, so
/// the character settles onto the floor at the rate a body falls at rather than
/// at whatever one tick's constant happened to be.
pub const GRAVITY: f64 = -9.81;

/// How often the `[HUD]` heartbeat is logged, in ticks: **a quarter** of a
/// second of simulated time at [`DEFAULT_TICK_HZ`].
///
/// Four times as often as most samples' and twice as often as
/// `apps/breach/src/game.rs`'s, and that is this demo's browser gate paying for
/// itself. Every wait `web/tools/browser-e2e.mjs` makes of this page is a whole
/// number of heartbeats, and this is the heaviest scene on the site: on the
/// software rasteriser that gate runs on, a simulated second costs about five
/// wall seconds here, so the heartbeat period *is* what each step of the gate
/// costs. Measured: taking it from half a second to a quarter took the browser
/// gate from 102 s to under the 90 s that step is budgeted. The driver is told
/// the period through its `beatMs` row, so the slowdown it scales every other
/// budget by stays a true reading.
pub const HEARTBEAT_TICKS: u64 = 15;

// ---------------------------------------------------------------------------
// Controls and the wire
// ---------------------------------------------------------------------------

/// What the input is asking for this tick, before it is sealed.
///
/// The four movement keys and the bearing the view is at. The **rotate** keys
/// are not here: turning the camera is presentation, it swings on the frame's
/// clock in [`crate::app`], and what the simulation needs of it is the bearing
/// below.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct Controls {
    pub forward: bool,
    pub back: bool,
    pub left: bool,
    pub right: bool,
    /// Whether the character is swinging this tick.
    ///
    /// A **request** rather than an event: the cadence is the server's, so a
    /// held key swings once per [`foe::STRIKE_PERIOD_S`] rather than once a
    /// tick.
    pub strike: bool,
    /// Where the view is pointing, in [`crate::camera::Iso::yaw`]'s measure.
    pub yaw: f32,
}

/// One client's move command.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
struct Intent {
    forward: bool,
    back: bool,
    left: bool,
    right: bool,
    strike: bool,
    yaw: f32,
}

const INTENT_FORWARD: u8 = 1 << 0;
const INTENT_BACK: u8 = 1 << 1;
const INTENT_LEFT: u8 = 1 << 2;
const INTENT_RIGHT: u8 = 1 << 3;
/// The blow. A **request** rather than an event: the server owns the cadence, so
/// a client that sent this every tick still swings once per
/// [`foe::STRIKE_PERIOD_S`].
const INTENT_STRIKE: u8 = 1 << 4;

/// Every bit the flag byte defines. One set outside this mask is a frame
/// something other than [`Intent::to_wire`] wrote.
const INTENT_FLAGS: u8 = INTENT_FORWARD | INTENT_BACK | INTENT_LEFT | INTENT_RIGHT | INTENT_STRIKE;

/// How many bytes one sealed intent is: a flag byte and one IEEE-754 binary32
/// bearing, little-endian.
const INTENT_BYTES: usize = 1 + core::mem::size_of::<f32>();

impl Intent {
    /// The forward axis, in `-1..=1`. Both keys held is neither, which is what
    /// makes releasing one of them do the obvious thing.
    fn ahead(self) -> f64 {
        f64::from(i8::from(self.forward) - i8::from(self.back))
    }

    /// The strafe axis, positive toward the character's right. See
    /// [`ahead`](Self::ahead).
    fn across(self) -> f64 {
        f64::from(i8::from(self.right) - i8::from(self.left))
    }

    /// The wire form handed to `Client::set_input`.
    fn to_wire(self) -> Vec<u8> {
        let mut flags = 0;
        for (set, bit) in [
            (self.forward, INTENT_FORWARD),
            (self.back, INTENT_BACK),
            (self.left, INTENT_LEFT),
            (self.right, INTENT_RIGHT),
            (self.strike, INTENT_STRIKE),
        ] {
            if set {
                flags |= bit;
            }
        }
        let mut bytes = Vec::with_capacity(INTENT_BYTES);
        bytes.push(flags);
        bytes.extend_from_slice(&self.yaw.to_le_bytes());
        bytes
    }

    /// The intent a client sealed, read back on the server's side of the wire.
    ///
    /// `None` for anything this build did not write: a payload of the wrong
    /// length, a flag outside [`INTENT_FLAGS`], or a bearing that is not a finite
    /// number. **Validated rather than trusted**, because these are the only
    /// bytes in this sample a peer chooses — and a `NaN` bearing would reach
    /// [`walk_direction`] and put the character somewhere nothing can recover
    /// from.
    fn from_wire(bytes: &[u8]) -> Option<Self> {
        if bytes.len() != INTENT_BYTES {
            return None;
        }
        let flags = bytes[0];
        if flags & !INTENT_FLAGS != 0 {
            return None;
        }
        let yaw = f32::from_le_bytes(bytes[1..].try_into().ok()?);
        if !yaw.is_finite() {
            return None;
        }
        Some(Self {
            forward: flags & INTENT_FORWARD != 0,
            back: flags & INTENT_BACK != 0,
            left: flags & INTENT_LEFT != 0,
            right: flags & INTENT_RIGHT != 0,
            strike: flags & INTENT_STRIKE != 0,
            yaw,
        })
    }

    /// Everything that arrived for this tick, folded into one.
    ///
    /// Normally one frame per tick and this is a decode. Several is a client
    /// whose clock ran ahead of the server's: the buttons are OR-ed, because each
    /// is a thing the player asked for and a later frame that says nothing is not
    /// a retraction. The **bearing is the last one**, because a view angle is a
    /// state rather than a request and the average of two angles either side of
    /// `π` points the wrong way.
    ///
    /// `held` is the bearing the last tick ran at, and is what a tick with no
    /// readable frame keeps. **The buttons and the bearing default in opposite
    /// directions on purpose**: a button is an edge and letting go is the safe
    /// reading of silence, while a bearing is a pose with no "off" — defaulting
    /// it would swing the walk to due north for one tick.
    fn from_inputs(inputs: ClientInputs<'_>, held: f32) -> Self {
        let mut merged = Self {
            yaw: held,
            ..Self::default()
        };
        for (_tick, data) in inputs.iter() {
            // A frame this build cannot read is skipped rather than taken as an
            // empty intent, which would read as the player letting go.
            let Some(frame) = Self::from_wire(data) else {
                continue;
            };
            merged.forward |= frame.forward;
            merged.back |= frame.back;
            merged.left |= frame.left;
            merged.right |= frame.right;
            merged.strike |= frame.strike;
            merged.yaw = frame.yaw;
        }
        merged
    }
}

// ---------------------------------------------------------------------------
// The stage
// ---------------------------------------------------------------------------

/// Everything this sample simulates.
///
/// Behind an `Arc<Mutex<_>>` shared with [`ShardModule`], for the reason
/// `apps/orbit` gives: the module is what the server ticks and the frame is what
/// reads the result, and the two are not the same call stack.
struct Stage {
    world: PhysicsWorld,
    player: CharacterController,
    /// The zone's foes, one per [`foe::POSTS`] row, in that order.
    foes: Vec<Foe>,
    /// What the character has left, out of [`foe::HEALTH_MAX`].
    health: u32,
    /// How many times they have been put down and returned to the spawn.
    downs: u64,
    /// How many of the foes had the character engaged at the end of the last
    /// tick.
    ///
    /// Kept rather than recomputed when the readout asks, because the answer is
    /// one ray per foe and the tick has already paid for them.
    engaged: usize,
    /// How many blows the character has swung — **trigger pulls**, whether or
    /// not anything was in reach.
    swings: u64,
    /// How many of those landed on a foe. One swing can land on several: the
    /// cleave answers everything within [`foe::STRIKE_REACH_M`] that has a clear
    /// line, so this counts *bodies struck* rather than swings that connected.
    hits: u64,
    /// How much health those blows took off, summed.
    dealt: u64,
    /// How much the foes' abilities have taken off the character, summed.
    ///
    /// **Monotone**, which is the half a readout needs that
    /// [`Stage::health`] cannot give: health comes back when the character is
    /// put down, so a reader that missed the dip would see a full bar and no
    /// evidence.
    taken: u64,
    /// When the next blow may be swung, in [`Stage::elapsed`] seconds.
    next_strike_at: f64,
    /// Which foe the cleave would answer, as an index into [`Stage::foes`], at
    /// the end of the last tick.
    target: Option<usize>,
    /// How fast the character is falling, in metres a second, negative downward.
    /// Zeroed the moment they are grounded.
    fall_speed: f64,
    ticks: u64,
    /// Seconds of **simulated** time, accumulated a tick at a time. What
    /// [`crate::light::flame`] is a function of, so a paused zone's flames hold
    /// still.
    elapsed: f64,
    /// Seconds of simulated time across **every** session, including the ones a
    /// save was resumed from.
    ///
    /// Separate from [`Stage::elapsed`] on purpose: elapsed is this session's
    /// clock and the torches are a function of it, so seeding it from a save
    /// would have a resumed zone open mid-flicker and the `[HUD]` heartbeat
    /// open at a tick nothing on the page expects. This is the number
    /// [`SaveHeader::playtime_secs`](crcbl::store::save::SaveHeader) means.
    playtime: f64,
    /// The bearing the last tick actually walked along.
    yaw: f32,
    /// What the last move came back with, kept so the frame and the heartbeat
    /// report the tick that happened rather than the one being asked for.
    outcome: MoveOutcome,
    /// How many ticks the move was stopped by something too steep to stand on —
    /// [`MoveOutcome::hit_wall`], counted. In this zone that is the stonework, so
    /// it is the number that says the walls are doing their job.
    blocked: u64,
    /// How many ticks it stepped up onto something —
    /// [`MoveOutcome::stepped_up`], counted. In this zone that is the dais, and
    /// it is what says the vertical variety is variety the character can use.
    climbed: u64,
}

/// Where the character's feet are, given where their capsule's centre is.
fn feet_of(player: &CharacterController) -> f64 {
    let config = player.config();
    player.position().y - (config.radius + config.half_height)
}

impl Stage {
    /// The character on the zone's spawn, ungrounded until the first move finds
    /// the floor.
    fn new() -> Self {
        let config = CharacterConfig::default();
        let lift = DVec3::Y * (config.radius + config.half_height);
        let mut world = zone::world();
        let foes = foe::stand_all(&mut world);
        Self {
            world,
            player: CharacterController::new(config, zone::spawn() + lift),
            foes,
            health: foe::HEALTH_MAX,
            downs: 0,
            engaged: 0,
            swings: 0,
            hits: 0,
            dealt: 0,
            taken: 0,
            next_strike_at: 0.0,
            target: None,
            fall_speed: 0.0,
            ticks: 0,
            elapsed: 0.0,
            playtime: 0.0,
            yaw: 0.0,
            outcome: MoveOutcome::default(),
            blocked: 0,
            climbed: 0,
        }
    }

    /// How many foes are still on their feet.
    fn alive(&self) -> usize {
        self.foes.iter().filter(|foe| foe.is_alive()).count()
    }

    /// Puts the stage into the state a previous session left.
    ///
    /// **Every field here is one [`crate::save`]'s own decoder has already
    /// validated**, so nothing is clamped or second-guessed on the way in: a
    /// position that was not a finite number inside the zone, or a health above
    /// an archetype's own ceiling, never reaches this function — it reads as no
    /// save at all and the zone opens fresh.
    ///
    /// The fall speed is zeroed rather than saved. A restored character is
    /// standing wherever the save left them and the next
    /// [`CharacterController::move_and_slide`] is what finds the floor under
    /// them, exactly as the first tick of a fresh zone does.
    fn restore(&mut self, character: &crate::save::Character) {
        self.player.set_position(character.centre);
        self.fall_speed = 0.0;
        self.health = character.health;
        self.downs = character.downs;
        self.playtime = character.playtime_secs;
        for (foe, health) in self.foes.iter_mut().zip(character.foes) {
            foe.restore(&mut self.world, health);
        }
    }

    /// What this session would leave for the next.
    fn snapshot(&self) -> crate::save::Character {
        let mut foes = [0; foe::FOES];
        for (slot, foe) in foes.iter_mut().zip(&self.foes) {
            *slot = if foe.is_alive() { foe.health() } else { 0 };
        }
        crate::save::Character {
            centre: self.player.position(),
            health: self.health,
            downs: self.downs,
            foes,
            playtime_secs: self.playtime,
            tick: self.ticks,
        }
    }
}

/// The nearest living foe the cleave would answer, or `None`.
///
/// Nearest rather than first, so the readout names the body a player would
/// expect to be answering — and the same query the trigger resolves with, so
/// what the panel says is what the blow does. A foe behind a pillar is not in
/// the answer, because [`foe::can_see`] is what decides.
fn cleave_target(world: &mut PhysicsWorld, centre: DVec3, foes: &[Foe]) -> Option<usize> {
    let mut nearest: Option<(usize, f64)> = None;
    for (index, target) in foes.iter().enumerate() {
        if !foe::can_see(world, centre, target, foe::STRIKE_REACH_M) {
            continue;
        }
        let gap = (target.centre() - centre).length();
        if nearest.is_none_or(|(_, best)| gap < best) {
            nearest = Some((index, gap));
        }
    }
    nearest.map(|(index, _)| index)
}

/// One tick of the foes: each one looks, moves, and takes its ability if one is
/// due.
///
/// **Nothing here decides how a foe behaves** — see [`crate::foe`]. What this
/// function owns is the order the three happen in and what an ability that
/// lands costs the character.
fn step_foes(stage: &mut Stage, dt: f64) {
    let now = stage.elapsed;
    let Stage {
        world,
        player,
        foes,
        health,
        taken,
        engaged,
        ..
    } = stage;
    // The character has already moved this tick, so a foe reacts to where they
    // are now rather than to where they were.
    let centre = player.position();
    for foe in foes.iter_mut() {
        foe.advance(world, centre, now, dt);
    }
    // Counted after the walk, so a foe that stepped out from behind a doorpost
    // this tick is engaged on it rather than on the next one.
    *engaged = foes.iter().filter(|foe| foe.is_engaged(now)).count();
    for foe in foes.iter_mut() {
        if let Some(damage) = foe.strikes(world, centre, now) {
            *taken += u64::from(damage);
            *health = health.saturating_sub(damage);
        }
    }
}

/// The character's cleave: everything within [`foe::STRIKE_REACH_M`] with a
/// clear line takes [`foe::STRIKE_DAMAGE`].
///
/// Resolved **after** the foes have moved, for the reason breach's plates give:
/// a body that had not been moved yet is one that cannot be hit where it is
/// drawn.
fn swing(stage: &mut Stage) {
    stage.next_strike_at = stage.elapsed + foe::STRIKE_PERIOD_S;
    stage.swings += 1;
    let centre = stage.player.position();
    let Stage {
        world,
        foes,
        hits,
        dealt,
        ..
    } = stage;
    for foe in foes.iter_mut() {
        if !foe::can_see(world, centre, foe, foe::STRIKE_REACH_M) {
            continue;
        }
        *hits += 1;
        *dealt += u64::from(foe::STRIKE_DAMAGE.min(foe.health()));
        foe.wounded(world, foe::STRIKE_DAMAGE);
    }
}

/// One tick of the simulation: an intent in, a displacement through the world.
fn run_tick(stage: &mut Stage, intent: Intent, dt: f64) {
    stage.yaw = intent.yaw;

    // **The walk conversion**: a bearing and two axes become a direction in the
    // world. Everything below this line is metres.
    let direction = walk_direction(f64::from(intent.yaw), intent.ahead(), intent.across());
    let horizontal = direction * WALK_SPEED * dt;

    // Gravity is integrated while the character is off the floor and reset the
    // moment they are on it. A grounded `move_and_slide` discards the vertical it
    // is asked for anyway, so this is about what the *next* tick falls at.
    stage.fall_speed += GRAVITY * dt;
    let motion = horizontal + DVec3::Y * stage.fall_speed * dt;

    let outcome = stage.player.move_and_slide(&mut stage.world, motion);
    if outcome.grounded {
        stage.fall_speed = 0.0;
    } else if outcome.hit_ceiling {
        stage.fall_speed = stage.fall_speed.min(0.0);
    }
    stage.blocked += u64::from(outcome.hit_wall);
    stage.climbed += u64::from(outcome.stepped_up);
    stage.outcome = outcome;

    // **The foes move before the blow is resolved**, and the readout is taken
    // between the two — see [`step_foes`] and [`swing`].
    step_foes(stage, dt);
    let centre = stage.player.position();
    stage.target = {
        let Stage { world, foes, .. } = &mut *stage;
        cleave_target(world, centre, foes)
    };
    // The cadence is the **server's**: a client holding the key down still
    // swings once per period, and one that sent the flag every tick gains
    // nothing by it.
    if intent.strike && stage.elapsed >= stage.next_strike_at {
        swing(stage);
    }

    // **The character can lose.** Running out returns them to the spawn with
    // full health and one more down against their name — which is what makes
    // the health a pool rather than a number that only falls.
    if stage.health == 0 {
        stage.health = foe::HEALTH_MAX;
        stage.downs += 1;
        let config = *stage.player.config();
        stage
            .player
            .set_position(zone::spawn() + DVec3::Y * (config.radius + config.half_height));
        stage.fall_speed = 0.0;
    }

    stage.ticks += 1;
    stage.elapsed += dt;
    stage.playtime += dt;
}

// ---------------------------------------------------------------------------
// The module
// ---------------------------------------------------------------------------

/// The stage, as the server hosts it.
///
/// `register` is empty for the same reason `apps/puppet`'s is: the whole
/// simulation is the [`Stage`] behind the shared cell, and there is no ECS system
/// to register.
struct ShardModule {
    shared: Arc<Mutex<Stage>>,
}

impl std::fmt::Debug for ShardModule {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("ShardModule").finish_non_exhaustive()
    }
}

impl GameModule for ShardModule {
    fn name(&self) -> &str {
        "shard"
    }

    fn register(&self, _world: &mut World) {}

    fn tick(&mut self, world: &mut World, inputs: ClientInputs<'_>) {
        let dt = world.tick_dt();
        let mut stage = lock(&self.shared);
        let held = stage.yaw;
        run_tick(&mut stage, Intent::from_inputs(inputs, held), dt);
    }
}

/// The shared stage, with a poisoned lock treated as the stage it was left in.
///
/// A panic inside the tick is a bug this sample would rather report through its
/// own numbers than through a second panic in the frame that reads them.
fn lock(shared: &Arc<Mutex<Stage>>) -> MutexGuard<'_, Stage> {
    shared
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner)
}

// ---------------------------------------------------------------------------
// What a frame reads
// ---------------------------------------------------------------------------

/// Everything the frame draws, snapshotted once per draw.
///
/// A plain struct rather than a borrow of the stage: the frame runs on the
/// frame's thread and the stage is behind a mutex the tick holds, and a frame
/// that read through the lock would be holding it for the length of a draw.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct RenderState {
    /// The **centre** of the character's capsule.
    pub position: DVec3,
    /// Where the feet are, in metres above the floor plane — what the figure is
    /// drawn at and what the camera pivots on.
    pub feet: DVec3,
    /// Whether the character is standing on something.
    pub grounded: bool,
    /// Whether the last move was stopped by something too steep to stand on.
    pub blocked: bool,
    /// Seconds of simulated time — what [`crate::light`] is a function of.
    pub elapsed: f64,
    /// One view per [`foe::POSTS`] row, in that order.
    pub foes: [FoeView; foe::FOES],
    /// What the character has left, out of [`foe::HEALTH_MAX`].
    pub health: u32,
    /// How many foes are still on their feet.
    pub alive: usize,
}

/// The stage's numbers, for the debug overlay and the `[HUD]` line.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct Stats {
    pub ticks: u64,
    pub position: DVec3,
    pub feet: f64,
    pub grounded: bool,
    /// How many ticks the walk was refused by stone.
    pub blocked: u64,
    /// How many ticks it stepped up onto the dais.
    pub climbed: u64,
    /// Seconds of simulated time.
    pub elapsed: f64,
    /// Seconds of simulated time across every session, including the ones this
    /// one resumed from. What `crate::save::SaveStats` reports and what the
    /// save's header carries.
    pub playtime: f64,
    /// The bearing the last tick walked along, in radians.
    pub yaw: f32,
    /// How many foes are still on their feet.
    pub alive: usize,
    /// How many of them have the character engaged.
    ///
    /// **The control for every claim about the fight**, on the line itself: a
    /// build that engaged unconditionally reports this at its ceiling from the
    /// first tick, and one that never noticed anything leaves it at zero for the
    /// whole run.
    pub engaged: usize,
    /// What the character has left, out of [`foe::HEALTH_MAX`].
    pub health: u32,
    /// How many times they have been put down and returned to the spawn.
    pub downs: u64,
    /// Blows swung, and the bodies those blows landed on. `swings` above `hits`
    /// is a swing that reached nothing, which is the control for the cleave
    /// resolving against the world rather than counting key presses.
    pub swings: u64,
    pub hits: u64,
    /// How much health the character has taken off the foes, and how much the
    /// foes have taken off them.
    pub dealt: u64,
    pub taken: u64,
    /// Which archetype the cleave would answer, or `None` for a swing that would
    /// reach nothing.
    pub target: Option<Kind>,
}

impl crcbl::ui::DebugModule for Stats {
    fn debug_section(&self, section: &mut crcbl::ui::DebugSection) {
        section.set_title("shard");
        section.row("tick", format_args!("{}", self.ticks));
        section.row(
            "pos",
            format_args!(
                "{:.2} {:.2} {:.2}",
                self.position.x, self.feet, self.position.z
            ),
        );
        section.row("bearing", format_args!("{:.2}", self.yaw));
        section.row(
            "ground",
            format_args!("{}", if self.grounded { "yes" } else { "no" }),
        );
        section.row("blocked", format_args!("{}", self.blocked));
        section.row("climbed", format_args!("{}", self.climbed));
        section.row(
            "health",
            format_args!("{}/{}", self.health, foe::HEALTH_MAX),
        );
        section.row("downs", format_args!("{}", self.downs));
        section.row("foes", format_args!("{}/{}", self.alive, foe::FOES));
        section.row("engaged", format_args!("{}", self.engaged));
        section.row_str("target", self.target_label());
        section.row("swings", format_args!("{}/{}", self.hits, self.swings));
        section.row("damage", format_args!("{} / {}", self.dealt, self.taken));
        section.row("elapsed", format_args!("{:.1} s", self.elapsed));
    }
}

impl Stats {
    /// What the cleave would answer, as one word.
    ///
    /// `"none"` rather than an empty string, so a heartbeat that names it cannot
    /// be read as a missing field.
    #[must_use]
    pub fn target_label(&self) -> &'static str {
        self.target.map_or("none", Kind::label)
    }
}

// ---------------------------------------------------------------------------
// The facade
// ---------------------------------------------------------------------------

/// What can stop shard before it starts.
#[derive(Debug)]
pub enum GameError {
    /// The operating system would not seed the server's resume credential.
    Server(String),
}

impl std::fmt::Display for GameError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Server(message) => write!(f, "server creation failed: {message}"),
        }
    }
}

impl std::error::Error for GameError {}

/// The stage, its server, its client, and the clock that drives all three.
pub struct Game {
    session: Loopback,
    shared: Arc<Mutex<Stage>>,
    /// Exactly one tick period per [`Game::tick`], so the server's accumulator
    /// yields exactly one tick per call.
    tick_period: Duration,
    sim_time: Duration,
    ticks_run: u64,
    /// What the player is holding down, sent on the next tick.
    pending: Intent,
}

impl std::fmt::Debug for Game {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Game")
            .field("ticks_run", &self.ticks_run)
            .finish_non_exhaustive()
    }
}

impl Game {
    /// Builds the server, its client and the stage between them.
    ///
    /// `restore` is what a previous session left, or `None` for a zone that
    /// opens fresh. It is applied to the stage **before** the server is built
    /// and therefore before any tick has run, so the first tick a resumed
    /// session takes is one from the state that was saved rather than one from
    /// the spawn — see `Stage::restore`.
    ///
    /// # Errors
    ///
    /// [`GameError::Server`] if the operating system would not give the server
    /// the entropy for a resume credential, or if the loopback session did not
    /// come up.
    ///
    /// # Panics
    ///
    /// If `tick_hz` is zero.
    pub fn new(tick_hz: u32, restore: Option<crate::save::Character>) -> Result<Self, GameError> {
        assert!(tick_hz > 0, "tick rate must be positive");
        let mut stage = Stage::new();
        if let Some(character) = &restore {
            stage.restore(character);
        }
        let shared = Arc::new(Mutex::new(stage));

        // An empty world, and that is the honest shape: this sample has no entity
        // and no ECS system. What the server hosts is the module, and what the
        // module owns is the stage.
        let session = Loopback::new(
            World::new(),
            Box::new(ShardModule {
                shared: Arc::clone(&shared),
            }),
            tick_hz,
            COMPATIBILITY,
        )
        .map_err(|error| GameError::Server(error.to_string()))?;

        let tick_period = session.tick_period();
        let mut game = Self {
            session,
            shared,
            tick_period,
            sim_time: Duration::ZERO,
            ticks_run: 0,
            pending: Intent::default(),
        };

        // **One tick spent on the handshake, before the character moves.**
        // `Server::update` drains the transport inside `tick`, so the client's
        // hello is not read until a tick runs, and until the session is up the
        // client drops every input frame it is asked to send. Spending it here is
        // what makes the player's first key the first the simulation sees.
        game.sim_time = tick_period;
        game.session.client_mut().update(game.sim_time);
        game.session.server_mut().update(game.sim_time);
        game.session.client_mut().update(game.sim_time);
        if game.session.server().session_state() != crcbl::net::SessionState::Connected {
            return Err(GameError::Server(
                "the loopback session did not come up in its first tick".into(),
            ));
        }

        crcbl::log::info!(
            "sim: {tick_hz} Hz, {:.3} ms per tick, walking at {WALK_SPEED} m/s across a \
             {}x{} tile zone",
            tick_period.as_secs_f64() * 1e3,
            zone::COLS,
            zone::ROWS,
        );
        Ok(game)
    }

    /// Records what the player is asking for, to be sent on the next tick.
    pub fn set_controls(&mut self, controls: Controls) {
        self.pending = Intent {
            forward: controls.forward,
            back: controls.back,
            left: controls.left,
            right: controls.right,
            strike: controls.strike,
            yaw: controls.yaw,
        };
    }

    /// Advances the server, and with it the stage, by exactly one tick.
    pub fn tick(&mut self) {
        self.sim_time += self.tick_period;
        let (server, client) = self.session.both_mut();

        // The bytes are the whole input path: the client seals them, the
        // transport carries them and the module decodes them, exactly as a remote
        // client's would be.
        client.set_input(self.pending.to_wire());

        // Send, simulate, then receive — and the send has to come first.
        // `Client::update` is the only thing that puts input on the wire and the
        // server drains the wire at the top of its tick, so a client updated only
        // after the server posts this tick's controls to the next one.
        client.update(self.sim_time);
        let server_ticks = server.update(self.sim_time);
        debug_assert_eq!(
            server_ticks, 1,
            "one tick period in must be exactly one server tick out",
        );
        // Consumes no tick — the clock has not moved between the two — and is
        // there to take the snapshot this tick produced.
        client.update(self.sim_time);
        self.ticks_run += 1;
    }

    /// How many times [`Game::tick`] has been called.
    #[must_use]
    pub const fn ticks_run(&self) -> u64 {
        self.ticks_run
    }

    /// What the frame should draw.
    #[must_use]
    pub fn render_state(&self) -> RenderState {
        let stage = lock(&self.shared);
        let position = stage.player.position();
        RenderState {
            position,
            feet: DVec3::new(position.x, feet_of(&stage.player), position.z),
            grounded: stage.outcome.grounded,
            blocked: stage.outcome.hit_wall,
            elapsed: stage.elapsed,
            foes: foe::views(&stage.foes, stage.elapsed),
            health: stage.health,
            alive: stage.alive(),
        }
    }

    /// What this session would leave for the next, read off the stage the
    /// server owns.
    ///
    /// Under the same lock every other reader takes, so a snapshot is one
    /// tick's state rather than a mixture of two — which for a save is the
    /// difference between a character standing where their health says they
    /// were and one who is not.
    #[must_use]
    pub fn snapshot(&self) -> crate::save::Character {
        lock(&self.shared).snapshot()
    }

    /// The stage's numbers for the debug panel and the `[HUD]` line.
    #[must_use]
    pub fn stats(&self) -> Stats {
        let stage = lock(&self.shared);
        Stats {
            ticks: stage.ticks,
            position: stage.player.position(),
            feet: feet_of(&stage.player),
            grounded: stage.outcome.grounded,
            blocked: stage.blocked,
            climbed: stage.climbed,
            elapsed: stage.elapsed,
            playtime: stage.playtime,
            yaw: stage.yaw,
            alive: stage.alive(),
            engaged: stage.engaged,
            health: stage.health,
            downs: stage.downs,
            swings: stage.swings,
            hits: stage.hits,
            dealt: stage.dealt,
            taken: stage.taken,
            target: stage.target.map(|index| stage.foes[index].kind()),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// One tick at the default rate.
    const DT: f64 = 1.0 / DEFAULT_TICK_HZ as f64;

    /// A stage that has already found the floor.
    fn ready() -> Stage {
        let mut stage = Stage::new();
        run_tick(&mut stage, Intent::default(), DT);
        assert!(stage.outcome.grounded, "the spawn has no floor under it");
        stage
    }

    /// Holds `intent` for `seconds`.
    fn hold(stage: &mut Stage, intent: Intent, seconds: f64) {
        #[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)]
        let ticks = (seconds / DT).round() as u64;
        for _ in 0..ticks {
            run_tick(stage, intent, DT);
        }
    }

    /// Every key held, walking away from the camera.
    const AHEAD: Intent = Intent {
        forward: true,
        back: false,
        left: false,
        right: false,
        strike: false,
        yaw: 0.0,
    };

    /// And walking towards it, which from the spawn is where the dais is.
    const BACK: Intent = Intent {
        forward: false,
        back: true,
        left: false,
        right: false,
        strike: false,
        yaw: 0.0,
    };

    /// Swinging, standing still.
    const SWING: Intent = Intent {
        forward: false,
        back: false,
        left: false,
        right: false,
        strike: true,
        yaw: 0.0,
    };

    /// Walking into the zone with the blow held down.
    const CHARGE: Intent = Intent {
        strike: true,
        ..AHEAD
    };

    /// **Held input walks the character and released input stops them**, which is
    /// the same pair the browser gate asserts and the reason it can be asserted
    /// there: if it were true only in a headless test, the browser check would be
    /// a check of the shim rather than of the controller.
    #[test]
    fn the_character_walks_while_asked_to_and_stops_when_they_are_not() {
        let mut stage = ready();
        let start = stage.player.position();
        hold(&mut stage, AHEAD, 1.0);
        let walked = stage.player.position();
        let covered = (walked - start).length();
        assert!(
            covered > 0.5 * WALK_SPEED,
            "a second of walking covered {covered:.2} m at {WALK_SPEED} m/s",
        );
        // A bearing of zero walks down -Z, which is where the zone reaches.
        assert!(walked.z < start.z - 0.5 * WALK_SPEED);

        hold(&mut stage, Intent::default(), 1.0);
        let stopped = stage.player.position();
        assert!(
            (stopped - walked).length() < 1e-6,
            "it kept moving after the key came up: {walked:?} then {stopped:?}",
        );
    }

    /// **The walk goes where the bearing says**, which is the seam this whole
    /// sample turns on: the camera is presentation and the only thing the
    /// simulation is ever told about it is this one angle.
    ///
    /// A quarter turn anticlockwise about `+Y` puts "away from the camera" along
    /// `−X`, which is [`crate::camera::walk_direction`]'s measure and not a sign
    /// this test is free to choose — `a_zero_bearing_walks_into_the_zone` in that
    /// module is what pins the convention, and this is it reaching the wire.
    #[test]
    fn the_bearing_on_the_wire_is_what_the_walk_follows() {
        let turned = Intent {
            yaw: core::f32::consts::FRAC_PI_2,
            ..AHEAD
        };
        let mut stage = ready();
        let start = stage.player.position();
        hold(&mut stage, turned, 0.6);
        let walked = stage.player.position();
        assert!(
            walked.x < start.x - 1.0,
            "a turned bearing walked to {walked:?} from {start:?}",
        );
        assert!(
            (walked.z - start.z).abs() < 0.3,
            "it drifted down the old bearing to {walked:?}",
        );
    }

    /// **The character walks onto the dais and stays on it**, which is what makes
    /// the zone's vertical variety something the controller uses rather than
    /// scenery it walks round.
    ///
    /// The wall is the control: the same walk into stone is refused, so a
    /// controller that climbed everything would pass the step and fail this.
    #[test]
    fn the_dais_is_stepped_onto_and_the_wall_is_not() {
        let mut stage = ready();
        // Straight down +Z from the spawn — towards the camera — is the dais,
        // and a second and a half at `WALK_SPEED` reaches its middle rather
        // than crossing it.
        hold(&mut stage, BACK, 1.5);
        assert!(
            stage.climbed > 0,
            "nothing was stepped onto on the way to the dais",
        );
        let feet = feet_of(&stage.player);
        assert!(
            (feet - zone::DAIS_HEIGHT).abs() < 0.05,
            "the character's feet are at {feet:.3} m, and the dais is at {:.3} m",
            zone::DAIS_HEIGHT,
        );

        // …and the stone at the far end of the zone refuses them.
        hold(&mut stage, BACK, 20.0);
        assert!(stage.blocked > 0, "nothing ever stopped the walk");
        let stopped = stage.player.position();
        hold(&mut stage, BACK, 2.0);
        assert!(
            (stage.player.position() - stopped).length() < 0.05,
            "the character walked through the far wall",
        );
    }

    /// **A sealed intent survives the wire, and nothing else does.** These are
    /// the only bytes in this sample a peer chooses, and a `NaN` bearing reaching
    /// [`walk_direction`] is unrecoverable.
    #[test]
    fn only_an_intent_this_build_sealed_reads_back() {
        for intent in [
            Intent::default(),
            AHEAD,
            Intent {
                back: true,
                left: true,
                right: true,
                strike: true,
                yaw: -2.5,
                ..Intent::default()
            },
            Intent {
                strike: true,
                ..Intent::default()
            },
        ] {
            let wire = intent.to_wire();
            assert_eq!(wire.len(), INTENT_BYTES);
            assert_eq!(Intent::from_wire(&wire), Some(intent));
        }

        assert_eq!(Intent::from_wire(&[]), None, "an empty frame");
        assert_eq!(Intent::from_wire(&[0; INTENT_BYTES + 1]), None, "too long");
        let mut spurious = Intent::default().to_wire();
        spurious[0] = 0xF0;
        assert_eq!(Intent::from_wire(&spurious), None, "a flag we never write");
        let mut nan = Intent::default().to_wire();
        nan[1..].copy_from_slice(&f32::NAN.to_le_bytes());
        assert_eq!(Intent::from_wire(&nan), None, "a bearing that is not one");
    }

    /// **A blow that reaches nothing kills nothing, and one that reaches a foe
    /// does.** The pair the browser gate makes in a browser, made here where a
    /// failure names the step.
    ///
    /// The first half is the control, and it is the whole claim that the cleave
    /// is resolved against the world: a build that counted key presses passes
    /// the kill and fails this, because on the spawn there is nothing within
    /// [`foe::STRIKE_REACH_M`] of the character at all.
    #[test]
    fn a_blow_that_reaches_nothing_kills_nothing_and_one_that_reaches_a_foe_does() {
        let mut stage = ready();
        assert_eq!(stage.alive(), foe::FOES, "the zone opened with foes down");

        // Swinging at the empty spawn, for long enough that the cadence lets
        // several blows through.
        hold(&mut stage, SWING, 2.0);
        assert!(stage.swings > 1, "the cadence let one blow through in 2 s");
        assert_eq!(stage.hits, 0, "a blow at an empty room landed on something");
        assert_eq!(stage.dealt, 0);
        assert_eq!(stage.alive(), foe::FOES);
        assert_eq!(stage.target, None, "something was in reach on the spawn");

        // …and then walking up the corridor into the husk on the doorway, with
        // the blow held down.
        let swung = stage.swings;
        hold(&mut stage, CHARGE, 8.0);
        assert!(
            stage.swings > swung,
            "the walk swallowed the blow: {swung} then {}",
            stage.swings,
        );
        assert!(
            stage.hits > 0,
            "nothing was ever within reach of the cleave"
        );
        assert!(stage.dealt > 0);
        assert!(
            stage.alive() < foe::FOES,
            "{} foes still standing after {} blow(s) landed",
            stage.alive(),
            stage.hits,
        );
    }

    /// **A foe notices the character when they come at it, and not before.**
    ///
    /// The "not before" is the control, and it is what the browser gate's
    /// engagement check depends on: every post is out of [`foe::NOTICE_M`] of
    /// the spawn, so a build that engaged unconditionally fails here rather than
    /// making that gate meaningless.
    #[test]
    fn a_foe_notices_the_character_only_once_they_come_at_it() {
        let mut stage = ready();
        hold(&mut stage, Intent::default(), 3.0);
        assert_eq!(
            stage.engaged, 0,
            "a foe engaged a character standing on the spawn",
        );
        assert_eq!(stage.taken, 0, "something reached them on the spawn");
        assert_eq!(stage.health, foe::HEALTH_MAX);

        hold(&mut stage, AHEAD, 6.0);
        assert!(
            stage.engaged > 0,
            "nothing noticed the character walking up the corridor at it",
        );
    }

    /// **A foe's ability costs the character health**, which is what makes the
    /// zone something a player can lose in.
    ///
    /// The control is the run above it: `taken` sat at zero for three seconds
    /// with the character on the spawn, so this is not a counter that was always
    /// climbing.
    #[test]
    fn a_foe_that_reaches_the_character_costs_them_health() {
        let mut stage = ready();
        hold(&mut stage, Intent::default(), 2.0);
        let untouched = stage.taken;
        assert_eq!(untouched, 0);

        // Walking into the husk and standing there, without swinging back.
        hold(&mut stage, AHEAD, 10.0);
        assert!(
            stage.taken > 0,
            "{} foe(s) engaged and none of them ever landed anything",
            stage.engaged,
        );
        assert!(
            stage.health < foe::HEALTH_MAX || stage.downs > 0,
            "the character took {} damage and still has all {} of their health",
            stage.taken,
            stage.health,
        );
    }

    /// **A session that resumes a snapshot opens where the last one stopped**,
    /// and a session handed nothing opens on the spawn with the zone intact.
    ///
    /// The second half is the control, and it is the one that matters: every
    /// reading the first half asserts — the position, the health, the downs, the
    /// standing count — is one a *fresh* zone also has a value for, so without
    /// the pair "it resumed" would pass for a build that ignored the argument
    /// entirely and always opened the same way.
    ///
    /// The snapshot is taken from a stage that was actually played rather than
    /// written by hand, so what is asserted is a round trip through the two
    /// functions a save goes through and not a struct this test filled in.
    #[test]
    fn a_resumed_session_opens_where_the_last_one_stopped_and_a_fresh_one_does_not() {
        // A stage walked away from the spawn, wounded, with its husk felled and
        // its warden hurt — a state nothing about opening a zone can produce.
        let mut played = ready();
        hold(&mut played, AHEAD, 1.0);
        played.health = 37;
        played.downs = 4;
        played.foes[0].wounded(&mut played.world, foe::HEALTH_MAX);
        played.foes[2].wounded(&mut played.world, 40);
        let saved = played.snapshot();
        assert_eq!(saved.foes[0], 0, "the husk was not felled");
        assert!(saved.centre.z < zone::spawn().z - 1.0, "it never walked");

        // ---- the stage, where the comparison can be exact --------------------
        let mut restored = Stage::new();
        restored.restore(&saved);
        assert_eq!(
            restored.snapshot(),
            crate::save::Character {
                // The one field that is provenance rather than state: it says
                // which tick wrote the save, and a session that resumes one
                // counts its own ticks from zero.
                tick: 0,
                ..saved
            },
            "a field did not survive the round trip",
        );
        // …and this session's own clock starts again, which is what keeps the
        // torches opening at the start of their cycle and the heartbeat at the
        // tick every reader expects. `playtime` is the one that carries over.
        assert!(saved.tick > 0, "the played stage never ticked");
        assert_eq!(restored.ticks, 0);
        assert_eq!(restored.elapsed, 0.0);
        assert_eq!(restored.playtime, saved.playtime_secs);

        // ---- and through the facade, which spends one tick on the handshake --
        let resumed = Game::new(DEFAULT_TICK_HZ, Some(saved)).expect("the loopback comes up");
        let stats = resumed.stats();
        assert!(
            (stats.position.x - saved.centre.x).abs() < 1e-9
                && (stats.position.z - saved.centre.z).abs() < 1e-9,
            "it opened at {:?} rather than at {:?}",
            stats.position,
            saved.centre,
        );
        assert_eq!(stats.health, saved.health);
        assert_eq!(stats.downs, saved.downs);
        assert_eq!(stats.alive, foe::FOES - 1, "the felled foe was back up");

        let fresh = Game::new(DEFAULT_TICK_HZ, None).expect("the loopback comes up");
        let opened = fresh.stats();
        assert_eq!(
            opened.alive,
            foe::FOES,
            "a fresh zone opened already cleared"
        );
        assert_eq!(opened.health, foe::HEALTH_MAX);
        assert_eq!(opened.downs, 0);
        assert!(
            (opened.position.z - zone::spawn().z).abs() < 1e-9,
            "a fresh zone opened at {:?} rather than on the spawn",
            opened.position,
        );
        assert!(
            opened.playtime < stats.playtime,
            "a fresh zone opened with {:.2} s of play behind it, against the \
             resumed session's {:.2} s",
            opened.playtime,
            stats.playtime,
        );
    }

    /// **A run of the whole game walks the character and reports it**, which is
    /// the one check that says the server, the client, the transport and the
    /// stage are all joined up.
    #[test]
    fn the_loopback_carries_a_held_key_to_the_controller() {
        let mut game = Game::new(DEFAULT_TICK_HZ, None).expect("the loopback always comes up");
        for _ in 0..30 {
            game.tick();
        }
        let start = game.render_state();
        assert!(start.grounded, "the character never found the floor");

        game.set_controls(Controls {
            forward: true,
            ..Controls::default()
        });
        for _ in 0..60 {
            game.tick();
        }
        let walked = game.render_state();
        assert!(
            walked.position.z < start.position.z - 1.0,
            "a second of walking got from {:?} to {:?}",
            start.position,
            walked.position,
        );
        assert!(walked.elapsed > start.elapsed, "the clock stood still");

        game.set_controls(Controls::default());
        for _ in 0..60 {
            game.tick();
        }
        let stopped = game.render_state();
        assert!(
            (stopped.position - walked.position).length() < 0.01,
            "it kept moving with nothing held",
        );
        // One more tick than `Game::tick` ran, and exactly one: `Game::new`
        // spends a tick bringing the loopback session up before the player can
        // move, which the module sees and the caller's counter does not.
        assert_eq!(game.stats().ticks, game.ticks_run() + 1);
    }
}
