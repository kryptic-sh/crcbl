//! The simulation: one capsule walking [`crate::map`], and the server that owns
//! it.
//!
//! ```text
//!  Stage ──▶ PuppetModule ──▶ Server ──┐                     ┌──▶ Client
//!  (this file)                         └── InMemoryTransport ┘
//!                     │
//!                     └──▶ RenderState ──▶ crate::app, crate::page
//! ```
//!
//! # Nothing here does collision, and that is rule 9
//!
//! Every metre the character moves goes through
//! [`CharacterController::move_and_slide`], which sweeps the capsule against
//! [`crate::map::world`] and slides it along what it hits. This file decides
//! **how far** to ask for and **which way**; the world decides what is left of
//! the request, and [`MoveOutcome`] is what it says about it.
//!
//! # How input becomes a world-space displacement
//!
//! ```text
//!   keys ──▶ Controls ──▶ Intent { yaw, forward, strafe } ──wire──▶ Intent
//!                                                                    │
//!                            crate::camera::walk_direction(yaw, …) ──┘
//!                                     │
//!                        × WALK_SPEED × dt  ──▶ move_and_slide
//! ```
//!
//! **The yaw crosses the wire and the direction is derived on the server.** The
//! client sends what the player did — which movement keys are down, and where
//! the camera was pointing when they pressed them — rather than a vector it
//! worked out for itself, because that is what a third-person game's move
//! command carries and what a predicted one will have to. The conversion is
//! [`crate::camera::walk_direction`], which is this sample's and not the
//! controller's; see that module for why the seam is drawn there.
//!
//! # The body turns toward where it went, not toward where the camera is
//!
//! [`CharacterController`] stores no orientation, so there is no yaw here to
//! fight over: the body's own facing is turned toward
//! [`MoveOutcome::motion`] — the displacement the world actually allowed, not
//! the one that was asked for — at [`TURN_RATE`]. A character sliding along a
//! wall therefore faces along the wall, which is where it is going.
//!
//! # It walks a circuit until somebody takes the controls
//!
//! A page that has just loaded has had no input, and a character standing still
//! is the same frame a stopped loop would draw. So a scripted circuit walks a
//! slow circle on the spawn pad from the first tick, and the first movement key
//! ends it for good — the arrangement `apps/orbit` and `apps/viewer` both use,
//! and what `web/tools/browser-e2e.mjs` reads to tell a running demo from a
//! stalled one before it presses anything itself.

use std::sync::{Arc, Mutex, MutexGuard};
use std::time::Duration;

use crcbl::ecs::{ClientInputs, GameModule, World};
use crcbl::math::DVec3;
use crcbl::net::ProtocolCompatibility;
use crcbl::phys::{CharacterConfig, CharacterController, MoveOutcome, PhysicsWorld};
use crcbl::session::Loopback;

use crate::camera::{facing_of, walk_direction};
use crate::map;

/// Distinct from every other sample's, because they are distinct protocols: a
/// client built for one must not hand-shake with a server running another. The
/// low half spells `PUP`.
const COMPATIBILITY: ProtocolCompatibility = ProtocolCompatibility {
    protocol_version: 1,
    engine_build_id: 0x0043_5243_424C,
    schema_hash: 0x0000_0050_5550,
};

/// The default simulation rate. Reaches the server, the client and the stage, so
/// there is exactly one rate in the process.
pub const DEFAULT_TICK_HZ: u32 = 60;

/// How fast the character walks, in metres a second.
///
/// A brisk walk rather than a run: milestone 1 has one gait, and this is the
/// speed the [`crate::map`] lane's steps are read at — fast enough to cross the
/// map without waiting, slow enough that a step is climbed rather than vaulted.
pub const WALK_SPEED: f64 = 3.2;

/// Gravity, in metres per second squared.
///
/// Integrated into a fall speed rather than applied as a fixed displacement:
/// the drop off the far side of a step is short, and a character that fell at a
/// constant rate would leave it at the wrong moment.
pub const GRAVITY: f64 = -9.81;

/// How fast the body turns toward the direction it is moving, in radians a
/// second.
///
/// Fast enough that the turn is over well inside a step's worth of walking, slow
/// enough to be a turn rather than a snap — which is the whole of what makes the
/// facing readable as animation-free motion.
pub const TURN_RATE: f64 = 9.0;

/// How far the character has to have moved in a tick for that motion to be worth
/// turning toward, in metres.
///
/// Below this the horizontal displacement is numerical noise — a grounded
/// character settling against its skin width — and a facing derived from it
/// would spin.
const FACING_EPSILON: f64 = 1e-4;

/// How often the `[HUD]` heartbeat is logged, in ticks: a second of simulated
/// time at [`DEFAULT_TICK_HZ`], the cadence every sample's heartbeat is spaced
/// at.
pub const HEARTBEAT_TICKS: u64 = 60;

/// How far from the spawn the unattended circuit walks, in metres.
///
/// Derived rather than chosen: the circuit is a constant walk speed under a yaw
/// that turns at a constant rate, which is a circle of
/// `WALK_SPEED · PATROL_PERIOD / 2π`. Named because this module's own tests
/// hold it against the map: everywhere the circuit can reach has to be flat
/// ground clear of both mounds, or the browser gate's run-up starts somewhere
/// it cannot walk a straight line from.
pub const PATROL_RADIUS: f64 = WALK_SPEED * PATROL_PERIOD / core::f64::consts::TAU;

/// How long the unattended circuit takes, in seconds.
///
/// Short, because it is [`PATROL_RADIUS`] that matters and this is what sets
/// it: a longer circuit is a wider one, and a wide enough one reaches the steep
/// mound.
pub const PATROL_PERIOD: f64 = 3.5;

// ---------------------------------------------------------------------------
// Controls and the wire
// ---------------------------------------------------------------------------

/// What the keyboard is asking for this tick, before it is sealed.
///
/// The four movement keys and the camera's azimuth. The camera keys are **not**
/// here: the camera is presentation, it turns on the frame's clock in
/// [`crate::app`], and what the simulation needs of it is the one angle below.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct Controls {
    pub forward: bool,
    pub back: bool,
    pub left: bool,
    pub right: bool,
    /// Where the view is pointing, in [`crate::camera::Follow::yaw`]'s measure.
    pub yaw: f32,
}

/// One client's move command.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
struct Intent {
    forward: bool,
    back: bool,
    left: bool,
    right: bool,
    yaw: f32,
}

const INTENT_FORWARD: u8 = 1 << 0;
const INTENT_BACK: u8 = 1 << 1;
const INTENT_LEFT: u8 = 1 << 2;
const INTENT_RIGHT: u8 = 1 << 3;

/// Every bit the flag byte defines. One set outside this mask is a frame
/// something other than [`Intent::to_wire`] wrote.
const INTENT_FLAGS: u8 = INTENT_FORWARD | INTENT_BACK | INTENT_LEFT | INTENT_RIGHT;

/// How many bytes one sealed intent is: a flag byte and the yaw as an IEEE-754
/// binary32, little-endian.
const INTENT_BYTES: usize = 1 + core::mem::size_of::<f32>();

impl Intent {
    /// Whether the player asked to **move**.
    ///
    /// The yaw is deliberately not part of the question: a camera always has an
    /// angle, so an intent that counted it would be "anything" on the first
    /// frame and the circuit would never run at all.
    const fn is_moving(self) -> bool {
        self.forward || self.back || self.left || self.right
    }

    /// The forward axis, in `-1..=1`. Both keys held is neither, which is what
    /// makes releasing one of them do the obvious thing.
    fn ahead(self) -> f64 {
        f64::from(i8::from(self.forward) - i8::from(self.back))
    }

    /// The strafe axis, positive toward the camera's right. See
    /// [`ahead`](Self::ahead).
    fn across(self) -> f64 {
        f64::from(i8::from(self.right) - i8::from(self.left))
    }

    /// The wire form handed to `Client::set_input`.
    fn to_wire(self) -> Vec<u8> {
        let mut flags = 0;
        if self.forward {
            flags |= INTENT_FORWARD;
        }
        if self.back {
            flags |= INTENT_BACK;
        }
        if self.left {
            flags |= INTENT_LEFT;
        }
        if self.right {
            flags |= INTENT_RIGHT;
        }
        let mut bytes = Vec::with_capacity(INTENT_BYTES);
        bytes.push(flags);
        bytes.extend_from_slice(&self.yaw.to_le_bytes());
        bytes
    }

    /// The intent a client sealed, read back on the server's side of the wire.
    ///
    /// `None` for anything this build did not write: a payload of the wrong
    /// length, a flag outside [`INTENT_FLAGS`], or a yaw that is not a finite
    /// number. **Validated rather than trusted**, because these are the only
    /// bytes in this sample a peer chooses — and a `NaN` yaw would reach
    /// [`walk_direction`] and put the character at a position nothing can
    /// recover from.
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
            yaw,
        })
    }

    /// Everything that arrived for this tick, folded into one.
    ///
    /// Normally one frame per tick and this is a decode. Several is a client
    /// whose clock ran ahead of the server's: the buttons are OR-ed, because
    /// each is a thing the player asked for and a later frame that says nothing
    /// is not a retraction — but the **yaw is the last one**, because a view
    /// angle is a state rather than a request and the average of two angles
    /// either side of `π` points the wrong way.
    fn from_inputs(inputs: ClientInputs<'_>) -> Self {
        let mut merged = Self::default();
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
/// Behind an `Arc<Mutex<_>>` shared with [`PuppetModule`], for the reason
/// `apps/orbit` gives: the module is what the server ticks and the frame is what
/// reads the result, and the two are not the same call stack.
struct Stage {
    world: PhysicsWorld,
    character: CharacterController,
    /// How fast the character is falling, in metres a second, negative
    /// downward. Zeroed the moment it is grounded.
    fall_speed: f64,
    /// Which way the body is turned, in [`crate::camera::Follow::yaw`]'s
    /// measure. **The demo's, not the controller's** — see the module docs.
    facing: f64,
    ticks: u64,
    /// Whether the circuit is still walking it. Ends at the first movement key.
    patrolling: bool,
    /// Seconds of **simulated** time, accumulated a tick at a time.
    ///
    /// What [`crate::map::sun`] is drawn from, so the light is a pure function
    /// of the tick rather than of a wall clock: two runs of the same length draw
    /// the same frame, and a paused demo's shadows stop where they are.
    elapsed: f64,
    /// What the last move came back with, kept so the frame and the heartbeat
    /// report the tick that happened rather than the one being asked for.
    outcome: MoveOutcome,
    /// How many times the controller has climbed a step —
    /// [`MoveOutcome::stepped_up`], counted.
    ///
    /// **A number no other part of this demo can move.** A step-up is the one
    /// thing in the move that is neither a slide nor a fall, so a run that
    /// reports one has been through `step_up`'s rise, advance and validated
    /// landing.
    climbed: u64,
    /// How many ticks the move was stopped by something too steep to stand on —
    /// [`MoveOutcome::hit_wall`], counted. The other half of the pair above: it
    /// is what says the character was *pushing* against the thing it did not
    /// climb rather than standing next to it.
    blocked: u64,
    /// The highest the character's feet have been, in metres above the ground.
    ///
    /// A record rather than a reading, because that is the shape the claim
    /// wants: "it got onto the low step and never onto the high one" is a
    /// question about the whole run, and a reading taken at the wrong instant
    /// answers neither half.
    highest: f64,
}

/// Where the character's feet are, given where its capsule's centre is.
fn feet_of(character: &CharacterController) -> f64 {
    let config = character.config();
    character.position().y - (config.radius + config.half_height)
}

impl Stage {
    /// The character on the spawn pad, ungrounded until the first move finds the
    /// floor.
    fn new() -> Self {
        let config = CharacterConfig::default();
        let centre = map::SPAWN + DVec3::Y * (config.radius + config.half_height);
        Self {
            world: map::world(),
            character: CharacterController::new(config, centre),
            fall_speed: 0.0,
            facing: 0.0,
            ticks: 0,
            patrolling: true,
            elapsed: 0.0,
            outcome: MoveOutcome::default(),
            climbed: 0,
            blocked: 0,
            highest: map::SPAWN.y,
        }
    }
}

/// What the circuit is asking for at `ticks` into the run.
///
/// A constant walk under a yaw that turns once every [`PATROL_PERIOD`], which is
/// a circle of [`PATROL_RADIUS`] about wherever it started. A pure function of
/// the tick, so the same tick is the same request on every machine.
fn patrol(ticks: u64, dt: f64) -> Intent {
    let seconds = ticks as f64 * dt;
    // Narrowed to the `f32` the wire carries, so the circuit drives the tick
    // through the same precision a real client's camera would have.
    let yaw = (core::f64::consts::TAU * seconds / PATROL_PERIOD) as f32;
    Intent {
        forward: true,
        yaw,
        ..Intent::default()
    }
}

/// Turns `from` toward `to` by at most `most`, the short way round.
fn turn_toward(from: f64, to: f64, most: f64) -> f64 {
    let mut delta = (to - from) % core::f64::consts::TAU;
    if delta > core::f64::consts::PI {
        delta -= core::f64::consts::TAU;
    } else if delta < -core::f64::consts::PI {
        delta += core::f64::consts::TAU;
    }
    from + delta.clamp(-most, most)
}

/// One tick of the simulation: an intent in, a displacement through the world.
fn run_tick(stage: &mut Stage, player: Intent, dt: f64) {
    // The first thing the player asks for ends the circuit for good, and the
    // same tick is the first one they drive — so the handover costs nothing.
    if player.is_moving() {
        stage.patrolling = false;
    }
    let intent = if stage.patrolling {
        patrol(stage.ticks, dt)
    } else {
        player
    };

    // **The conversion**: a view angle and two axes become a direction in the
    // world. Everything below this line is metres.
    let direction = walk_direction(f64::from(intent.yaw), intent.ahead(), intent.across());
    let horizontal = direction * WALK_SPEED * dt;

    // Gravity is integrated while the character is off the ground and reset the
    // moment it is on it. A grounded `move_and_slide` discards the vertical it
    // is asked for anyway — it takes its rise from the ramp instead — so this is
    // about what the *next* tick falls at, not about this one.
    stage.fall_speed += GRAVITY * dt;
    let motion = horizontal + DVec3::Y * stage.fall_speed * dt;

    let outcome = stage.character.move_and_slide(&mut stage.world, motion);
    if outcome.grounded {
        stage.fall_speed = 0.0;
    } else if outcome.hit_ceiling {
        stage.fall_speed = stage.fall_speed.min(0.0);
    }

    // The body turns toward what the world allowed, which is why this reads
    // `outcome.motion` rather than `horizontal`.
    let walked = DVec3::new(outcome.motion.x, 0.0, outcome.motion.z);
    if walked.length() > FACING_EPSILON
        && let Some(want) = facing_of(walked)
    {
        stage.facing = turn_toward(stage.facing, want, TURN_RATE * dt);
    }

    stage.climbed += u64::from(outcome.stepped_up);
    stage.blocked += u64::from(outcome.hit_wall);
    stage.highest = stage.highest.max(feet_of(&stage.character));
    stage.outcome = outcome;
    stage.ticks += 1;
    stage.elapsed += dt;
}

// ---------------------------------------------------------------------------
// The module
// ---------------------------------------------------------------------------

/// The stage, as the server hosts it.
///
/// `register` is empty for the same reason `apps/orbit`'s is: the whole
/// simulation is the [`Stage`] behind the shared cell, and there is no ECS
/// system to register.
struct PuppetModule {
    shared: Arc<Mutex<Stage>>,
}

impl std::fmt::Debug for PuppetModule {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("PuppetModule").finish_non_exhaustive()
    }
}

impl GameModule for PuppetModule {
    fn name(&self) -> &str {
        "puppet"
    }

    fn register(&self, _world: &mut World) {}

    fn tick(&mut self, world: &mut World, inputs: ClientInputs<'_>) {
        let dt = world.tick_dt();
        run_tick(&mut lock(&self.shared), Intent::from_inputs(inputs), dt);
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
    /// The **centre** of the character's capsule, which is what
    /// [`crate::map::Character::place_at`] takes.
    pub position: DVec3,
    /// Where the feet are, in metres above the ground.
    pub feet: f64,
    /// Which way the body is turned.
    pub facing: f64,
    /// Whether it is standing on walkable ground.
    pub grounded: bool,
    /// Whether the last move was stopped by something too steep to stand on.
    pub blocked: bool,
    /// Whether the circuit is still walking it.
    pub patrolling: bool,
    /// Seconds of simulated time — what [`crate::map::sun`] takes.
    pub elapsed: f64,
}

/// The stage's numbers, for the debug overlay.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct Stats {
    pub ticks: u64,
    pub position: DVec3,
    pub feet: f64,
    pub grounded: bool,
    pub climbed: u64,
    pub blocked: u64,
    pub highest: f64,
    pub slides: u32,
    pub patrolling: bool,
}

impl crcbl::ui::DebugModule for Stats {
    fn debug_section(&self, section: &mut crcbl::ui::DebugSection) {
        section.set_title("puppet");
        section.row("tick", format_args!("{}", self.ticks));
        section.row(
            "pos",
            format_args!(
                "{:.2} {:.2} {:.2}",
                self.position.x, self.feet, self.position.z
            ),
        );
        section.row(
            "ground",
            format_args!("{}", if self.grounded { "yes" } else { "no" }),
        );
        section.row("climbed", format_args!("{}", self.climbed));
        section.row("blocked", format_args!("{}", self.blocked));
        section.row("top", format_args!("{:.2} m", self.highest));
        section.row("slides", format_args!("{}", self.slides));
        section.row(
            "pilot",
            format_args!("{}", if self.patrolling { "circuit" } else { "player" }),
        );
    }
}

// ---------------------------------------------------------------------------
// The facade
// ---------------------------------------------------------------------------

/// What can stop puppet before it starts.
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
    /// # Errors
    ///
    /// [`GameError::Server`] if the operating system would not give the server
    /// the entropy for a resume credential, or if the loopback session did not
    /// come up.
    ///
    /// # Panics
    ///
    /// If `tick_hz` is zero.
    pub fn new(tick_hz: u32) -> Result<Self, GameError> {
        assert!(tick_hz > 0, "tick rate must be positive");
        let shared = Arc::new(Mutex::new(Stage::new()));

        // An empty world, and that is the honest shape: this sample has no
        // entity and no ECS system. What the server hosts is the module, and
        // what the module owns is the stage.
        let session = Loopback::new(
            World::new(),
            Box::new(PuppetModule {
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
        // client drops every input frame it is asked to send. Spending it here
        // is what makes the player's first key the first the simulation sees.
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
            "sim: {tick_hz} Hz, {:.3} ms per tick, walking at {WALK_SPEED} m/s",
            tick_period.as_secs_f64() * 1e3,
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
            yaw: controls.yaw,
        };
    }

    /// Advances the server, and with it the stage, by exactly one tick.
    pub fn tick(&mut self) {
        self.sim_time += self.tick_period;
        let (server, client) = self.session.both_mut();

        // The bytes are the whole input path: the client seals them, the
        // transport carries them and the module decodes them, exactly as a
        // remote client's would be.
        client.set_input(self.pending.to_wire());

        // Send, simulate, then receive — and the send has to come first.
        // `Client::update` is the only thing that puts input on the wire and
        // the server drains the wire at the top of its tick, so a client
        // updated only after the server posts this tick's controls to the next
        // one.
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
        self.log_heartbeat();
    }

    /// How many times [`Game::tick`] has been called.
    #[must_use]
    pub const fn ticks_run(&self) -> u64 {
        self.ticks_run
    }

    /// The `[HUD]` line, on the cadence every other sample uses.
    ///
    /// `web/tools/browser-e2e.mjs` reads five claims out of it, and each one is
    /// a number nothing but [`CharacterController`] can move:
    ///
    /// * `px`, `py`, `pz` — where the character is. The gate holds a key and
    ///   requires `pz` to advance, then releases it and requires `pz` to stop,
    ///   which is the pair a demo that merely drifts cannot pass.
    /// * `climbed` — [`MoveOutcome::stepped_up`], counted. It rises when the
    ///   character walks onto [`map::LOW_STEP_TOP`] and at no other time in the
    ///   lane.
    /// * `blocked` — [`MoveOutcome::hit_wall`], counted. It says the character
    ///   is *pushing* against the riser it did not climb rather than standing
    ///   near it.
    /// * `top` — the highest its feet have been. The control for `climbed`: it
    ///   reaches [`map::LOW_STEP_TOP`] and never [`map::HIGH_STEP_TOP`].
    fn log_heartbeat(&self) {
        let stage = lock(&self.shared);
        if !stage.ticks.is_multiple_of(HEARTBEAT_TICKS) {
            return;
        }
        let position = stage.character.position();
        crcbl::log::info!(
            "[HUD] tick: {}  px: {:.2}  py: {:.2}  pz: {:.2}  ground: {}  climbed: {}  \
             blocked: {}  top: {:.2}  pilot: {}",
            stage.ticks,
            position.x,
            feet_of(&stage.character),
            position.z,
            if stage.outcome.grounded { "yes" } else { "no" },
            stage.climbed,
            stage.blocked,
            stage.highest,
            if stage.patrolling {
                "circuit"
            } else {
                "player"
            },
        );
    }

    /// What the frame should draw.
    #[must_use]
    pub fn render_state(&self) -> RenderState {
        let stage = lock(&self.shared);
        RenderState {
            position: stage.character.position(),
            feet: feet_of(&stage.character),
            facing: stage.facing,
            grounded: stage.outcome.grounded,
            blocked: stage.outcome.hit_wall,
            patrolling: stage.patrolling,
            elapsed: stage.elapsed,
        }
    }

    /// The stage's numbers for the debug panel.
    #[must_use]
    pub fn stats(&self) -> Stats {
        let stage = lock(&self.shared);
        Stats {
            ticks: stage.ticks,
            position: stage.character.position(),
            feet: feet_of(&stage.character),
            grounded: stage.outcome.grounded,
            climbed: stage.climbed,
            blocked: stage.blocked,
            highest: stage.highest,
            slides: stage.outcome.slides,
            patrolling: stage.patrolling,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// One tick at the default rate.
    const DT: f64 = 1.0 / DEFAULT_TICK_HZ as f64;

    /// A stage that has already found the floor, with the circuit switched off
    /// so a test drives it.
    fn standing() -> Stage {
        let mut stage = Stage::new();
        stage.patrolling = false;
        run_tick(&mut stage, Intent::default(), DT);
        assert!(stage.outcome.grounded, "the spawn has no floor under it");
        stage
    }

    /// Walks for `seconds` with the given intent and hands back the stage.
    fn walk(stage: &mut Stage, intent: Intent, seconds: f64) {
        for _ in 0..(seconds / DT).round() as u64 {
            run_tick(stage, intent, DT);
        }
    }

    /// **Held input moves the character and released input stops it**, which is
    /// the same pair the browser gate asserts and the reason it can be asserted
    /// there: if it were true only in a headless test, the browser check would
    /// be a check of the shim rather than of the controller.
    #[test]
    fn the_character_walks_while_asked_to_and_stops_when_it_is_not() {
        let mut stage = standing();
        let forward = Intent {
            forward: true,
            ..Intent::default()
        };
        let start = stage.character.position();
        walk(&mut stage, forward, 1.0);
        let walked = stage.character.position();
        let covered = (walked - start).length();
        assert!(
            covered > 0.5 * WALK_SPEED,
            "a second of walking covered {covered:.2} m at {WALK_SPEED} m/s",
        );
        // A yaw of zero walks down -Z, which is where the lane is.
        assert!(walked.z < start.z - 0.5 * WALK_SPEED);

        walk(&mut stage, Intent::default(), 1.0);
        let stopped = stage.character.position();
        assert!(
            (stopped - walked).length() < 1e-6,
            "it drifted {:.6} m with nothing held",
            (stopped - walked).length(),
        );
    }

    /// **It climbs the step under the offset and refuses the one over it.**
    /// The positive and its control, out of one walk down the lane — and the
    /// same claim the browser gate makes, so a failure there is a failure here
    /// rather than a mystery about the browser.
    #[test]
    fn it_gets_onto_the_low_step_and_no_further() {
        let mut stage = standing();
        let forward = Intent {
            forward: true,
            ..Intent::default()
        };
        // Long enough to cross the run-up, the low step and reach the riser:
        // the lane is `SPAWN_Z - HIGH_STEP_FAR_Z` metres long at `WALK_SPEED`,
        // and a walk that is stopped early has simply stopped early.
        walk(&mut stage, forward, 12.0);

        assert!(stage.climbed > 0, "nothing stepped up over the whole lane",);
        assert!(
            (stage.highest - map::LOW_STEP_TOP).abs() < 0.05,
            "the highest the feet reached was {:.3} m, not the low step's {:.3} m",
            stage.highest,
            map::LOW_STEP_TOP,
        );
        assert!(
            stage.highest < map::HIGH_STEP_TOP - 0.1,
            "it climbed onto the high step, whose top is {:.2} m",
            map::HIGH_STEP_TOP,
        );
        assert!(
            stage.blocked > 0,
            "it never pushed against the riser it did not climb",
        );
        assert!(
            stage.character.position().z > map::LOW_STEP_FAR_Z - 1.0,
            "it walked past the riser at z = {:.2}",
            map::LOW_STEP_FAR_Z,
        );
    }

    /// **The mound under the walkable angle is walked up and the one over it is
    /// not**, which is the other half of what the map is for. Driven with a
    /// yaw rather than a world vector, so the conversion is in the path.
    #[test]
    fn it_walks_up_one_mound_and_is_refused_by_the_other() {
        // +X is a quarter turn right of the -Z the camera opens looking down.
        let toward_gentle = Intent {
            forward: true,
            yaw: -core::f32::consts::FRAC_PI_2,
            ..Intent::default()
        };
        let toward_steep = Intent {
            forward: true,
            yaw: core::f32::consts::FRAC_PI_2,
            ..Intent::default()
        };

        let mut up = standing();
        walk(&mut up, toward_gentle, 4.0);
        assert!(
            up.highest > 0.5,
            "the gentle mound took the character to {:.2} m",
            up.highest,
        );

        let mut refused = standing();
        walk(&mut refused, toward_steep, 4.0);
        // It slides around the flank rather than stopping dead — a sphere is
        // convex — so what "refused" means here is that it never got up it.
        // Measured against the low step rather than against a number chosen for
        // this assertion: the mound's summit is 2.2 m, and the character did not
        // even reach the height of the step in the lane.
        assert!(
            refused.highest < map::LOW_STEP_TOP,
            "the steep mound took the character to {:.2} m, which is over the {:.2} m \
             step in the lane",
            refused.highest,
            map::LOW_STEP_TOP,
        );
        assert!(
            refused.blocked > 0,
            "it never pushed against the steep mound",
        );
    }

    /// **The circuit walks the character and the first key ends it for good.**
    #[test]
    fn the_circuit_runs_until_somebody_takes_the_controls() {
        let mut stage = Stage::new();
        walk(&mut stage, Intent::default(), 1.0);
        assert!(stage.patrolling, "a page with no input keeps the circuit");
        let moved = (stage.character.position() - map::SPAWN).length();
        assert!(
            moved > 1.0,
            "the circuit only got {moved:.2} m from the spawn"
        );

        run_tick(
            &mut stage,
            Intent {
                back: true,
                ..Intent::default()
            },
            DT,
        );
        assert!(
            !stage.patrolling,
            "a movement key did not take the controls"
        );
        walk(&mut stage, Intent::default(), 1.0);
        assert!(
            !stage.patrolling,
            "the circuit came back after the player let go",
        );
    }

    /// **The circuit stays on the flat**, which is what makes the browser
    /// gate's run-up meaningful: it holds a key from wherever the circuit left
    /// the character, so everywhere the circuit can reach has to be ground.
    #[test]
    fn the_patrol_stays_on_the_flat() {
        let mut stage = Stage::new();
        walk(&mut stage, Intent::default(), 4.0 * PATROL_PERIOD);
        assert!(stage.patrolling);
        assert_eq!(stage.climbed, 0, "the circuit climbed something");
        assert!(
            stage.highest.abs() < 0.05,
            "the circuit reached {:.3} m above the ground",
            stage.highest,
        );
        let from_spawn = DVec3::new(
            stage.character.position().x - map::SPAWN.x,
            0.0,
            stage.character.position().z - map::SPAWN.z,
        );
        assert!(
            from_spawn.length() < 2.0 * PATROL_RADIUS + 0.5,
            "the circuit wandered {:.2} m from the spawn, past its own {PATROL_RADIUS:.2} m radius",
            from_spawn.length(),
        );
    }

    /// **The browser gate's script works from anywhere the circuit can leave
    /// the character**, which is the assumption the whole of that gate rests
    /// on: it holds one key from wherever the demo happens to be when it takes
    /// over, and requires the character to advance, to climb the low step and
    /// to be refused by the high one.
    ///
    /// So the circuit is handed over at eight points spread round its own
    /// period, and the same walk is run from each. A map whose lane was too
    /// narrow, or a circuit that wandered into a mound, fails here rather than
    /// in a browser on somebody else's machine.
    #[test]
    fn the_gates_walk_works_from_every_point_of_the_circuit() {
        let period_ticks = (PATROL_PERIOD / DT).round() as u64;
        for eighth in 0..8 {
            let mut stage = Stage::new();
            for _ in 0..(period_ticks * eighth / 8) {
                run_tick(&mut stage, Intent::default(), DT);
            }
            let handover = stage.character.position();
            walk(
                &mut stage,
                Intent {
                    forward: true,
                    ..Intent::default()
                },
                12.0,
            );
            assert!(
                stage.climbed > 0,
                "from {handover:?} the walk climbed nothing",
            );
            assert!(
                (stage.highest - map::LOW_STEP_TOP).abs() < 0.05,
                "from {handover:?} the feet reached {:.3} m, not the low step's {:.3} m",
                stage.highest,
                map::LOW_STEP_TOP,
            );
            assert!(
                stage.blocked > 0,
                "from {handover:?} it never pushed against the high step",
            );
            assert!(
                stage.character.position().z > map::LOW_STEP_FAR_Z - 1.0,
                "from {handover:?} it walked past the riser, to z = {:.2}",
                stage.character.position().z,
            );
        }
    }

    /// **The body turns toward where it went**, and gets there — the demo's own
    /// job, since the controller stores no orientation at all.
    #[test]
    fn the_body_turns_toward_the_direction_it_is_moving() {
        let mut stage = standing();
        let back = Intent {
            back: true,
            ..Intent::default()
        };
        assert_eq!(stage.facing, 0.0, "it starts looking down -Z");
        walk(&mut stage, back, 2.0);
        // Walking backwards from a facing of zero is a half turn.
        let half = core::f64::consts::PI;
        let error = (stage.facing.rem_euclid(core::f64::consts::TAU) - half).abs();
        assert!(
            error < 0.05,
            "it walked -Z-backwards and ended up facing {:.3} rad",
            stage.facing,
        );
    }

    /// **The wire is validated rather than trusted.** These are the only bytes
    /// a peer chooses, and a `NaN` yaw would reach the conversion.
    #[test]
    fn a_frame_this_build_did_not_write_is_refused() {
        let intent = Intent {
            forward: true,
            right: true,
            yaw: 1.25,
            ..Intent::default()
        };
        let wire = intent.to_wire();
        assert_eq!(wire.len(), INTENT_BYTES);
        assert_eq!(Intent::from_wire(&wire), Some(intent));

        assert_eq!(Intent::from_wire(&[]), None, "an empty frame");
        assert_eq!(Intent::from_wire(&wire[..1]), None, "a truncated frame");
        let mut long = wire.clone();
        long.push(0);
        assert_eq!(Intent::from_wire(&long), None, "an over-long frame");

        let mut unknown = wire.clone();
        unknown[0] |= 1 << 7;
        assert_eq!(Intent::from_wire(&unknown), None, "an undefined flag");

        let mut nan = wire.clone();
        nan[1..].copy_from_slice(&f32::NAN.to_le_bytes());
        assert_eq!(Intent::from_wire(&nan), None, "a yaw that is not a number");
        let mut infinite = wire;
        infinite[1..].copy_from_slice(&f32::INFINITY.to_le_bytes());
        assert_eq!(Intent::from_wire(&infinite), None, "a yaw at infinity");
    }

    /// Both movement keys held is neither, and the yaw is the last one that
    /// arrived rather than a blend of them.
    #[test]
    fn opposing_keys_cancel_and_the_yaw_is_the_latest() {
        let both = Intent {
            forward: true,
            back: true,
            left: true,
            right: true,
            yaw: 0.0,
        };
        assert_eq!(both.ahead(), 0.0);
        assert_eq!(both.across(), 0.0);
        assert!(both.is_moving(), "the keys are still down");
        assert!(
            !Intent {
                yaw: 3.0,
                ..Intent::default()
            }
            .is_moving(),
            "a camera angle is not a request to move",
        );
    }

    /// The short way round, including across the wrap.
    #[test]
    fn a_turn_takes_the_short_way_round() {
        let pi = core::f64::consts::PI;
        assert!((turn_toward(0.0, 0.5, 1.0) - 0.5).abs() < 1e-12);
        assert!((turn_toward(0.0, 0.5, 0.1) - 0.1).abs() < 1e-12);
        // From just under +π to just over -π is a short step forward, not a
        // long walk back through zero.
        let stepped = turn_toward(pi - 0.05, -pi + 0.05, 1.0);
        assert!(
            (stepped - (pi + 0.05)).abs() < 1e-12,
            "it went the long way: {stepped}",
        );
    }
}
