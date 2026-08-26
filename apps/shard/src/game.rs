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
//! # Slice 1 is one verb, and the verb is *explore*
//!
//! `docs/plan/sample/15-shard.md`'s milestone 1 is "explore, fight, loot, level,
//! save, resume". This file is the first of those and nothing else: there is no
//! enemy, no ability, no item, no experience and no save. What there is is a
//! character, a zone with stone in it, and gravity. `docs/backlog.md` carries the
//! rest with what each would take.
//!
//! # Nothing here does collision, and that is rule 9
//!
//! Every metre the character moves goes through
//! [`CharacterController::move_and_slide`], which sweeps the capsule against
//! [`zone::world`] and slides it along what it hits. This
//! file decides **where from** and **which way**; the world decides what is
//! there.
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
    yaw: f32,
}

const INTENT_FORWARD: u8 = 1 << 0;
const INTENT_BACK: u8 = 1 << 1;
const INTENT_LEFT: u8 = 1 << 2;
const INTENT_RIGHT: u8 = 1 << 3;

/// Every bit the flag byte defines. One set outside this mask is a frame
/// something other than [`Intent::to_wire`] wrote.
const INTENT_FLAGS: u8 = INTENT_FORWARD | INTENT_BACK | INTENT_LEFT | INTENT_RIGHT;

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
    /// How fast the character is falling, in metres a second, negative downward.
    /// Zeroed the moment they are grounded.
    fall_speed: f64,
    ticks: u64,
    /// Seconds of **simulated** time, accumulated a tick at a time. What
    /// [`crate::light::flame`] is a function of, so a paused zone's flames hold
    /// still.
    elapsed: f64,
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
        Self {
            world: zone::world(),
            player: CharacterController::new(config, zone::spawn() + lift),
            fall_speed: 0.0,
            ticks: 0,
            elapsed: 0.0,
            yaw: 0.0,
            outcome: MoveOutcome::default(),
            blocked: 0,
            climbed: 0,
        }
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

    stage.ticks += 1;
    stage.elapsed += dt;
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
    /// The bearing the last tick walked along, in radians.
    pub yaw: f32,
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
        section.row("elapsed", format_args!("{:.1} s", self.elapsed));
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
        }
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
            yaw: stage.yaw,
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
        yaw: 0.0,
    };

    /// And walking towards it, which from the spawn is where the dais is.
    const BACK: Intent = Intent {
        forward: false,
        back: true,
        left: false,
        right: false,
        yaw: 0.0,
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
                yaw: -2.5,
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

    /// **A run of the whole game walks the character and reports it**, which is
    /// the one check that says the server, the client, the transport and the
    /// stage are all joined up.
    #[test]
    fn the_loopback_carries_a_held_key_to_the_controller() {
        let mut game = Game::new(DEFAULT_TICK_HZ).expect("the loopback always comes up");
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
