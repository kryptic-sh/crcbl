//! The simulation: one capsule walking [`crate::map`], one hitscan pistol
//! shooting down it, and the server that owns both.
//!
//! ```text
//!  Stage ──▶ BreachModule ──▶ Server ──┐                     ┌──▶ Client
//!  (this file)                         └── InMemoryTransport ┘
//!                     │
//!                     └──▶ RenderState ──▶ crate::app, crate::page
//! ```
//!
//! # Nothing here does collision or intersection, and that is rule 9
//!
//! Every metre the player moves goes through
//! [`CharacterController::move_and_slide`], which sweeps the capsule against
//! [`crate::map::world`] and slides it along what it hits. Every shot goes
//! through [`PhysicsWorld::cast_ray`], which is the same world answering a
//! different question. This file decides **where from** and **which way**; the
//! world decides what is there.
//!
//! # How input becomes a world-space displacement, and a shot
//!
//! ```text
//!   keys ──▶ Controls ──▶ Intent { yaw, pitch, forward, strafe, fire } ──wire──▶ Intent
//!                                                                                 │
//!                                        crate::camera::walk_direction(yaw, …) ───┤
//!                                                    │                            │
//!                                        × WALK_SPEED × dt ──▶ move_and_slide     │
//!                                                                                 │
//!                                        crate::camera::forward(yaw, pitch) ──────┘
//!                                                    │
//!                                        Ray::new(eye, …) ──▶ cast_ray
//! ```
//!
//! **Both angles cross the wire and both conversions happen on the server.**
//! The client sends what the player did — which keys are down, whether the
//! trigger was pulled, and where they were looking when they pulled it — rather
//! than a vector or a hit it worked out for itself, because that is what a
//! shooter's move-and-fire command carries and what a lag-compensated one will
//! have to. The conversions are [`crate::camera`]'s, which is this sample's and
//! not the controller's; see that module for why the seam is drawn there.
//!
//! # A hitscan pistol is the crosshair, resolved once
//!
//! There is exactly one [`PhysicsWorld::cast_ray`] per tick and both readings
//! come out of it: what the crosshair is on, and — on a tick the trigger was
//! pulled — what the shot hit. That is not an optimisation, it is the model: a
//! hitscan weapon hits whatever is under the crosshair at the instant of the
//! pull, and computing the two separately would let them disagree while the
//! picture said they could not.
//!
//! # The range runs itself until somebody steps up to the line
//!
//! A page that has just loaded has had no input, and a player standing still in
//! an empty room is the same frame a stopped loop would draw. So the range runs
//! a demonstration from the first tick — it swings the aim onto each lane in
//! turn and fires — and the first movement key or trigger pull ends it for
//! good and **resets the range**: every plate back up, the score back to zero,
//! the view squared up down the near lane. A firing range resetting when a
//! shooter steps up is what one does, and it is what makes the first string a
//! visitor shoots their own.
//!
//! The camera follows that demonstration while it runs — see
//! [`RenderState::imposed_aim`] — because a first-person camera that did not
//! would be showing a different room from the one being shot at.

use std::sync::{Arc, Mutex, MutexGuard};
use std::time::Duration;

use crcbl::ecs::{ClientInputs, GameModule, World};
use crcbl::math::DVec3;
use crcbl::net::ProtocolCompatibility;
use crcbl::phys::{
    CharacterConfig, CharacterController, ColliderId, MoveOutcome, PhysicsWorld, Ray,
};
use crcbl::session::Loopback;

use crate::camera::{EYE_HEIGHT, forward, walk_direction};
use crate::map::{self, LANE_LIST, LANES};

/// Distinct from every other sample's, because they are distinct protocols: a
/// client built for one must not hand-shake with a server running another. The
/// low half spells `BRE`.
const COMPATIBILITY: ProtocolCompatibility = ProtocolCompatibility {
    protocol_version: 1,
    engine_build_id: 0x0043_5243_424C,
    schema_hash: 0x0000_0042_5245,
};

/// The default simulation rate. Reaches the server, the client and the stage, so
/// there is exactly one rate in the process.
pub const DEFAULT_TICK_HZ: u32 = 60;

/// How fast the player walks, in metres a second.
///
/// A little brisker than `apps/puppet`'s, because this room is bigger than that
/// blockout and the walk from the spawn to the firing line is the only walk
/// there is. Slow enough that the kerb stops the player rather than being
/// vaulted.
pub const WALK_SPEED: f64 = 3.4;

/// Gravity, in metres per second squared.
///
/// Integrated into a fall speed rather than applied as a fixed displacement, so
/// the player settles onto the floor at the rate a body falls at rather than at
/// whatever one tick's constant happened to be.
pub const GRAVITY: f64 = -9.81;

/// How far the pistol reaches, in metres.
///
/// Longer than the room is, so nothing inside it is out of range and a shot
/// that finds nothing has genuinely found nothing — which on this map means the
/// player is looking through a gap that should not exist. `RANGE_M` is what
/// makes [`Aim::Nothing`] a **report** rather than a state the geometry can
/// produce.
pub const RANGE_M: f64 = 64.0;

/// How long a knocked-down plate stays down, in seconds.
///
/// Long enough to be read as a hit from across the room and short enough that a
/// player working down the lanes finds the near one back up by the time they
/// come back to it.
pub const PLATE_RESET_S: f64 = 2.5;

/// How often the `[HUD]` heartbeat is logged, in ticks: a second of simulated
/// time at [`DEFAULT_TICK_HZ`], the cadence every sample's heartbeat is spaced
/// at.
pub const HEARTBEAT_TICKS: u64 = 60;

/// How long the warm-up spends on each lane, in seconds.
pub const WARMUP_LANE_S: f64 = 2.4;

/// What fraction of a lane's slot the warm-up spends swinging onto it.
///
/// The rest is spent held on the plate, which is what puts [`WARMUP_FIRE_AT`]
/// safely after the swing has finished — a shot let off mid-swing would miss,
/// and a demonstration that misses is one a visitor reads as a broken demo.
const WARMUP_SWING: f64 = 0.45;

/// Where in a lane's slot the warm-up pulls the trigger, as a fraction.
const WARMUP_FIRE_AT: f64 = 0.8;

/// The pose the range squares a shooter up at when they step to the line:
/// level, straight down the near lane.
///
/// A firing range hands everyone the same first shot, and a demo is better for
/// it too — a visitor who takes the controls mid-demonstration would otherwise
/// inherit whatever bearing the warm-up happened to be swinging through, and
/// their first trigger pull would go into a wall for no reason they could see.
pub const SQUARE_UP: (f32, f32) = (0.0, 0.0);

// ---------------------------------------------------------------------------
// What a shot found
// ---------------------------------------------------------------------------

/// What the crosshair is on — and therefore what a trigger pull would hit.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum Aim {
    /// A standing plate, and the lane it is in. **The only one that scores.**
    Plate(usize),
    /// A plate that has already been knocked down and is lying flat. Hitting it
    /// is a miss: the target has been taken, and taking it twice is not two
    /// hits.
    Downed,
    /// Some other surface of the range — a wall, the floor, the ceiling, a
    /// plate's post or the kerb.
    Range,
    /// Nothing inside [`RANGE_M`]. See that constant: on a closed room this is
    /// a report about the map rather than a thing a player can aim at.
    #[default]
    Nothing,
}

impl Aim {
    /// What the panel and the `[HUD]` line call it.
    #[must_use]
    pub const fn label(self) -> &'static str {
        match self {
            Self::Plate(lane) => LANE_LIST[lane].label,
            Self::Downed => "down",
            Self::Range => "range",
            Self::Nothing => "none",
        }
    }

    /// Whether a shot along this line scores.
    #[must_use]
    pub const fn scores(self) -> bool {
        matches!(self, Self::Plate(_))
    }
}

// ---------------------------------------------------------------------------
// Controls and the wire
// ---------------------------------------------------------------------------

/// What the input is asking for this tick, before it is sealed.
///
/// The four movement keys, the trigger, and the two angles the view is at. The
/// **look** keys are not here: looking is presentation, it turns on the frame's
/// clock in [`crate::app`], and what the simulation needs of it is the pair of
/// angles below.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct Controls {
    pub forward: bool,
    pub back: bool,
    pub left: bool,
    pub right: bool,
    /// The trigger, as an **edge**: one press is one shot. A held trigger is
    /// not an automatic weapon, and slice 1 has one pistol.
    pub fire: bool,
    /// Where the view is pointing, in [`crate::camera::Eye::yaw`]'s measure.
    pub yaw: f32,
    /// …and [`crate::camera::Eye::pitch`]'s.
    pub pitch: f32,
}

/// One client's move-and-fire command.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
struct Intent {
    forward: bool,
    back: bool,
    left: bool,
    right: bool,
    fire: bool,
    yaw: f32,
    pitch: f32,
}

const INTENT_FORWARD: u8 = 1 << 0;
const INTENT_BACK: u8 = 1 << 1;
const INTENT_LEFT: u8 = 1 << 2;
const INTENT_RIGHT: u8 = 1 << 3;
const INTENT_FIRE: u8 = 1 << 4;

/// Every bit the flag byte defines. One set outside this mask is a frame
/// something other than [`Intent::to_wire`] wrote.
const INTENT_FLAGS: u8 = INTENT_FORWARD | INTENT_BACK | INTENT_LEFT | INTENT_RIGHT | INTENT_FIRE;

/// How many bytes one sealed intent is: a flag byte and two IEEE-754 binary32
/// angles, little-endian.
const INTENT_BYTES: usize = 1 + 2 * core::mem::size_of::<f32>();

impl Intent {
    /// Whether the player did anything the range should treat as stepping up to
    /// the line.
    ///
    /// The angles are deliberately not part of the question: a view always has
    /// a pair of them, so an intent that counted them would be "anything" on
    /// the first frame and the warm-up would never run at all. Looking around
    /// does not take the controls; walking or shooting does.
    const fn is_active(self) -> bool {
        self.forward || self.back || self.left || self.right || self.fire
    }

    /// The forward axis, in `-1..=1`. Both keys held is neither, which is what
    /// makes releasing one of them do the obvious thing.
    fn ahead(self) -> f64 {
        f64::from(i8::from(self.forward) - i8::from(self.back))
    }

    /// The strafe axis, positive toward the player's right. See
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
            (self.fire, INTENT_FIRE),
        ] {
            if set {
                flags |= bit;
            }
        }
        let mut bytes = Vec::with_capacity(INTENT_BYTES);
        bytes.push(flags);
        bytes.extend_from_slice(&self.yaw.to_le_bytes());
        bytes.extend_from_slice(&self.pitch.to_le_bytes());
        bytes
    }

    /// The intent a client sealed, read back on the server's side of the wire.
    ///
    /// `None` for anything this build did not write: a payload of the wrong
    /// length, a flag outside [`INTENT_FLAGS`], or an angle that is not a finite
    /// number. **Validated rather than trusted**, because these are the only
    /// bytes in this sample a peer chooses — and a `NaN` angle would reach
    /// [`forward`] and put the shot, the walk and the player at a place nothing
    /// can recover from.
    fn from_wire(bytes: &[u8]) -> Option<Self> {
        if bytes.len() != INTENT_BYTES {
            return None;
        }
        let flags = bytes[0];
        if flags & !INTENT_FLAGS != 0 {
            return None;
        }
        let yaw = f32::from_le_bytes(bytes[1..5].try_into().ok()?);
        let pitch = f32::from_le_bytes(bytes[5..].try_into().ok()?);
        if !yaw.is_finite() || !pitch.is_finite() {
            return None;
        }
        Some(Self {
            forward: flags & INTENT_FORWARD != 0,
            back: flags & INTENT_BACK != 0,
            left: flags & INTENT_LEFT != 0,
            right: flags & INTENT_RIGHT != 0,
            fire: flags & INTENT_FIRE != 0,
            yaw,
            pitch,
        })
    }

    /// Everything that arrived for this tick, folded into one.
    ///
    /// Normally one frame per tick and this is a decode. Several is a client
    /// whose clock ran ahead of the server's: the buttons are OR-ed, because
    /// each is a thing the player asked for and a later frame that says nothing
    /// is not a retraction — **the trigger included**, so a pull that landed in
    /// the same tick as a release is still a shot and never two. The **angles
    /// are the last ones**, because a view angle is a state rather than a
    /// request and the average of two angles either side of `π` points the
    /// wrong way.
    ///
    /// `held` is the aim the last tick ran at, and is what a tick with no
    /// readable frame keeps. **The buttons and the angles default in opposite
    /// directions on purpose**: a button is an edge and letting go is the safe
    /// reading of silence, while an aim is a pose with no "off" — defaulting it
    /// would swing the view to due north for one tick and send that tick's shot
    /// somewhere nobody pointed.
    fn from_inputs(inputs: ClientInputs<'_>, held: (f32, f32)) -> Self {
        let mut merged = Self {
            yaw: held.0,
            pitch: held.1,
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
            merged.fire |= frame.fire;
            merged.yaw = frame.yaw;
            merged.pitch = frame.pitch;
        }
        merged
    }
}

// ---------------------------------------------------------------------------
// The stage
// ---------------------------------------------------------------------------

/// Everything this sample simulates.
///
/// Behind an `Arc<Mutex<_>>` shared with [`BreachModule`], for the reason
/// `apps/orbit` gives: the module is what the server ticks and the frame is what
/// reads the result, and the two are not the same call stack.
struct Stage {
    world: PhysicsWorld,
    /// The plates' colliders, near lane first — what a ray's answer is compared
    /// against, and what a knock-down moves.
    plates: [ColliderId; LANES],
    player: CharacterController,
    /// How fast the player is falling, in metres a second, negative downward.
    /// Zeroed the moment they are grounded.
    fall_speed: f64,
    ticks: u64,
    /// Seconds of **simulated** time, accumulated a tick at a time. What the
    /// plates' timers are measured against, so a paused demo's plates stay
    /// where they are.
    elapsed: f64,
    /// When each plate stands back up, in [`Stage::elapsed`] seconds, or `None`
    /// for a plate that is already standing.
    down_until: [Option<f64>; LANES],
    shots: u64,
    hits: u64,
    /// Whether the range is still running its own demonstration.
    warming_up: bool,
    /// The angles the last tick actually walked and shot along.
    aim: (f32, f32),
    /// What the crosshair was on at the end of the last tick.
    crosshair: Aim,
    /// What the last move came back with, kept so the frame and the heartbeat
    /// report the tick that happened rather than the one being asked for.
    outcome: MoveOutcome,
    /// How many ticks the move was stopped by something too steep to stand on —
    /// [`MoveOutcome::hit_wall`], counted. On this map that is the firing line
    /// and the walls, so it is the number that says the kerb is doing its job.
    blocked: u64,
}

/// Where the player's feet are, given where their capsule's centre is.
fn feet_of(player: &CharacterController) -> f64 {
    let config = player.config();
    player.position().y - (config.radius + config.half_height)
}

/// Where the player's **eye** is — the origin of every shot.
fn eye_of(player: &CharacterController) -> DVec3 {
    let position = player.position();
    DVec3::new(position.x, feet_of(player) + EYE_HEIGHT, position.z)
}

impl Stage {
    /// The player on the spawn, ungrounded until the first move finds the floor,
    /// with every plate standing.
    fn new() -> Self {
        let config = CharacterConfig::default();
        let centre = map::SPAWN + DVec3::Y * (config.radius + config.half_height);
        let (world, plates) = map::world();
        Self {
            world,
            plates,
            player: CharacterController::new(config, centre),
            fall_speed: 0.0,
            ticks: 0,
            elapsed: 0.0,
            down_until: [None; LANES],
            shots: 0,
            hits: 0,
            warming_up: true,
            aim: (0.0, 0.0),
            crosshair: Aim::Nothing,
            outcome: MoveOutcome::default(),
            blocked: 0,
        }
    }

    /// Which lane a collider is the plate of, if it is one.
    fn lane_of(&self, id: ColliderId) -> Option<usize> {
        self.plates.iter().position(|&plate| plate == id)
    }

    /// Writes a plate's collider where it is now: standing or knocked flat, at
    /// wherever [`map::plate_x`] puts it at this instant.
    ///
    /// **The collider and nothing else**, since the mesh is the frame's
    /// business — [`RenderState`] is what carries the same two facts across.
    fn set_plate(&mut self, lane: usize, down: bool) {
        let at = map::plate_collider(LANE_LIST[lane], map::plate_x(lane, self.elapsed), down);
        self.world.set_box(self.plates[lane], at);
    }
}

/// The angles that point from `eye` at `target`, in [`forward`]'s measure.
///
/// The inverse of that function, and the only place in this sample that runs
/// it backwards: the warm-up knows where it wants to shoot and needs the pair
/// of angles that gets there. `an_aim_computed_at_a_plate_points_at_it` holds
/// the two together.
#[must_use]
pub fn aim_at(eye: DVec3, target: DVec3) -> (f64, f64) {
    let to = (target - eye).normalize_or_zero();
    (to.x.atan2(-to.z), to.y.clamp(-1.0, 1.0).asin())
}

/// Where the warm-up is aiming and whether it is firing, `seconds` into the run.
///
/// A pure function of the elapsed time, so the same tick is the same request on
/// every machine. It swings onto each lane in turn over [`WARMUP_SWING`] of the
/// slot, holds there, and lets one shot off at [`WARMUP_FIRE_AT`] — after the
/// swing has finished, which is what makes the demonstration hit.
fn warmup(seconds: f64, dt: f64) -> Intent {
    let (yaw, pitch) = warmup_aim(seconds);
    #[allow(clippy::cast_possible_truncation)]
    Intent {
        fire: warmup_fires(seconds, dt),
        yaw: yaw as f32,
        pitch: pitch as f32,
        ..Intent::default()
    }
}

/// The bearing from the firing point to one lane's plate, `seconds` into the
/// run.
///
/// The time is in it because [`map::MOVER_LANE`]'s plate travels: a warm-up
/// that aimed at where the mover *used* to be would miss it, and a
/// demonstration that misses is one a visitor reads as a broken demo.
fn lane_bearing(lane: usize, seconds: f64) -> (f64, f64) {
    let at = LANE_LIST[lane];
    aim_at(
        DVec3::new(map::SPAWN.x, EYE_HEIGHT, map::SPAWN.z),
        DVec3::new(map::plate_x(lane, seconds), map::PLATE_CENTRE_Y, at.z),
    )
}

/// Where the warm-up is looking, `seconds` in.
fn warmup_aim(seconds: f64) -> (f64, f64) {
    let phase = seconds / WARMUP_LANE_S;
    let slot = phase.floor();
    let frac = phase - slot;
    #[allow(clippy::cast_possible_truncation)]
    let lane = (slot as i64).rem_euclid(LANES as i64) as usize;
    let from = lane_bearing((lane + LANES - 1) % LANES, seconds);
    let to = lane_bearing(lane, seconds);
    // Eased rather than linear, so the swing starts and stops rather than
    // snapping into motion — the same reason a real shooter's transitions are
    // not steps.
    let t = (frac / WARMUP_SWING).clamp(0.0, 1.0);
    let eased = t * t * (3.0 - 2.0 * t);
    (
        from.0 + (to.0 - from.0) * eased,
        from.1 + (to.1 - from.1) * eased,
    )
}

/// Whether the warm-up's trigger comes down on the tick that ends at `seconds`.
///
/// One shot per slot: the fraction has to have **crossed** [`WARMUP_FIRE_AT`]
/// this tick, so a slower tick rate fires once and a faster one does too.
fn warmup_fires(seconds: f64, dt: f64) -> bool {
    let frac = |at: f64| (at / WARMUP_LANE_S).rem_euclid(1.0);
    frac(seconds) >= WARMUP_FIRE_AT && frac(seconds - dt) < WARMUP_FIRE_AT
}

/// Stands every plate up and clears the score.
///
/// What stepping up to the line does. The plates are put back through the same
/// [`Stage::set_plate`] a hit uses, so a reset range is the range a fresh run
/// starts on rather than one that merely looks like it.
fn reset_range(stage: &mut Stage) {
    for lane in 0..LANES {
        if stage.down_until[lane].take().is_some() {
            stage.set_plate(lane, false);
        }
    }
    stage.shots = 0;
    stage.hits = 0;
}

/// One tick of the simulation: an intent in, a displacement and a ray through
/// the world.
fn run_tick(stage: &mut Stage, player: Intent, dt: f64) {
    // The first thing the player asks for ends the warm-up and resets the
    // range. The same tick is the first one they drive, so the handover costs
    // nothing here — squaring the shooter up is the client's to do, because
    // the view is the client's: see [`RenderState::imposed_aim`].
    if stage.warming_up && player.is_active() {
        stage.warming_up = false;
        reset_range(stage);
    }
    let intent = if stage.warming_up {
        warmup(stage.elapsed, dt)
    } else {
        player
    };
    stage.aim = (intent.yaw, intent.pitch);

    // **The walk conversion**: a view angle and two axes become a direction in
    // the world. Everything below this line is metres.
    let direction = walk_direction(f64::from(intent.yaw), intent.ahead(), intent.across());
    let horizontal = direction * WALK_SPEED * dt;

    // Gravity is integrated while the player is off the floor and reset the
    // moment they are on it. A grounded `move_and_slide` discards the vertical
    // it is asked for anyway, so this is about what the *next* tick falls at.
    stage.fall_speed += GRAVITY * dt;
    let motion = horizontal + DVec3::Y * stage.fall_speed * dt;

    let outcome = stage.player.move_and_slide(&mut stage.world, motion);
    if outcome.grounded {
        stage.fall_speed = 0.0;
    } else if outcome.hit_ceiling {
        stage.fall_speed = stage.fall_speed.min(0.0);
    }
    stage.blocked += u64::from(outcome.hit_wall);
    stage.outcome = outcome;

    // Plates come back up **before** the shot is resolved, so a plate whose
    // delay expires on this tick can be taken again on it. The alternative
    // reads as a plate that is visibly standing and cannot be hit.
    for lane in 0..LANES {
        if stage.down_until[lane].is_some_and(|until| stage.elapsed >= until) {
            stage.down_until[lane] = None;
            stage.set_plate(lane, false);
        }
    }
    // And the travelling plate is moved to where this instant puts it, for the
    // same reason and in the same window: a mover whose collider lagged its
    // mesh is a target that cannot be hit where it is drawn.
    stage.set_plate(map::MOVER_LANE, stage.down_until[map::MOVER_LANE].is_some());

    // **The shot conversion**, and the one ray this tick casts — see the module
    // docs for why the crosshair and the trigger share it.
    let eye = eye_of(&stage.player);
    let along = forward(f64::from(intent.yaw), f64::from(intent.pitch));
    stage.crosshair = trace(stage, eye, along);

    if intent.fire {
        stage.shots += 1;
        if let Aim::Plate(lane) = stage.crosshair {
            stage.hits += 1;
            stage.down_until[lane] = Some(stage.elapsed + PLATE_RESET_S);
            stage.set_plate(lane, true);
        }
    }

    stage.ticks += 1;
    stage.elapsed += dt;
}

/// What a ray from `eye` along `along` finds, as the readout the crosshair and
/// the trigger share.
fn trace(stage: &mut Stage, eye: DVec3, along: DVec3) -> Aim {
    let ray = Ray::new(eye, along).with_bounds(0.0, RANGE_M);
    let Some((id, _)) = stage.world.cast_ray(&ray) else {
        return Aim::Nothing;
    };
    match stage.lane_of(id) {
        // A plate whose collider is in its lying-down pose can still be hit —
        // by a shot aimed at the floor — and that is a miss, not a second hit
        // on a target already taken.
        Some(lane) if stage.down_until[lane].is_some() => Aim::Downed,
        Some(lane) => Aim::Plate(lane),
        None => Aim::Range,
    }
}

// ---------------------------------------------------------------------------
// The module
// ---------------------------------------------------------------------------

/// The stage, as the server hosts it.
///
/// `register` is empty for the same reason `apps/puppet`'s is: the whole
/// simulation is the [`Stage`] behind the shared cell, and there is no ECS
/// system to register.
struct BreachModule {
    shared: Arc<Mutex<Stage>>,
}

impl std::fmt::Debug for BreachModule {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("BreachModule").finish_non_exhaustive()
    }
}

impl GameModule for BreachModule {
    fn name(&self) -> &str {
        "breach"
    }

    fn register(&self, _world: &mut World) {}

    fn tick(&mut self, world: &mut World, inputs: ClientInputs<'_>) {
        let dt = world.tick_dt();
        let mut stage = lock(&self.shared);
        let held = stage.aim;
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
    /// The **centre** of the player's capsule.
    pub position: DVec3,
    /// Where the feet are, in metres above the floor.
    pub feet: f64,
    /// Where the eye is — what the frame is drawn from.
    pub eye: DVec3,
    /// Whether the player is standing on the floor.
    pub grounded: bool,
    /// Whether the last move was stopped by something too steep to stand on —
    /// the firing line, or a wall.
    pub blocked: bool,
    /// Which plates are lying down, near lane first.
    pub plates_down: [bool; LANES],
    /// Where each plate is across the range, near lane first — the same
    /// [`map::plate_x`] the colliders were written at, so the picture and the
    /// physics are one instant rather than two.
    pub plates_x: [f64; LANES],
    /// What the crosshair is on.
    pub crosshair: Aim,
    pub shots: u64,
    pub hits: u64,
    /// Whether the range is still running its own demonstration.
    pub warming_up: bool,
    /// The angles the simulation is imposing on the view this tick, or `None`
    /// once the player owns it.
    ///
    /// **Some for exactly as long as the warm-up runs.** The range is aiming
    /// during the demonstration and a first-person camera that ignored that
    /// would be showing a different room from the one being shot at.
    /// [`crate::app`] is what writes it into [`crate::camera::Eye`].
    ///
    /// **The frame this goes back to `None` is the frame the client squares
    /// the shooter up at [`SQUARE_UP`]**, and it is the client that does it,
    /// off the edge in this field rather than off a pose the simulation
    /// imposes for a tick. A frame is not a tick: a browser drawing at ten
    /// frames a second runs six ticks between two draws, and anything offered
    /// on one of those six is offered to nobody. An edge the client remembers
    /// across its own frames cannot be missed that way, and it costs at most
    /// one frame of walking at the bearing the warm-up left.
    pub imposed_aim: Option<(f32, f32)>,
}

/// The stage's numbers, for the debug overlay.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct Stats {
    pub ticks: u64,
    pub position: DVec3,
    pub feet: f64,
    pub grounded: bool,
    pub blocked: u64,
    pub shots: u64,
    pub hits: u64,
    pub crosshair: Aim,
    pub plates_down: [bool; LANES],
    /// Where the travelling plate is across the range, in metres —
    /// [`map::plate_x`] at [`map::MOVER_LANE`].
    ///
    /// **The one number on this demo's heartbeat that nothing a player does can
    /// move**, which is what `web/tools/browser-e2e.mjs` reads to tell a
    /// running demo from a stalled one.
    pub mover_x: f64,
    pub warming_up: bool,
    /// Where the last tick was looking, in radians.
    pub aim: (f32, f32),
}

impl Stats {
    /// Hits as a percentage of shots — [`accuracy`] over this reading.
    #[must_use]
    pub fn accuracy(&self) -> Option<f64> {
        accuracy(self.shots, self.hits)
    }
}

/// Hits as a percentage of shots, or `None` before the first shot.
///
/// `None` rather than zero, which is what a run that has fired once and missed
/// reports — and the difference between "nothing yet" and "nothing hit" is the
/// whole of what an accuracy readout is for.
///
/// A free function because two readouts want it: the debug panel through
/// [`Stats`] and the overlay through [`RenderState`], and a second copy of one
/// division is a second copy that can round differently.
#[must_use]
#[allow(clippy::cast_precision_loss)]
pub fn accuracy(shots: u64, hits: u64) -> Option<f64> {
    (shots > 0).then(|| 100.0 * hits as f64 / shots as f64)
}

impl crcbl::ui::DebugModule for Stats {
    fn debug_section(&self, section: &mut crcbl::ui::DebugSection) {
        section.set_title("breach");
        section.row("tick", format_args!("{}", self.ticks));
        section.row(
            "pos",
            format_args!(
                "{:.2} {:.2} {:.2}",
                self.position.x, self.feet, self.position.z
            ),
        );
        section.row("look", format_args!("{:.2} {:.2}", self.aim.0, self.aim.1));
        section.row(
            "ground",
            format_args!("{}", if self.grounded { "yes" } else { "no" }),
        );
        section.row("blocked", format_args!("{}", self.blocked));
        section.row("shots", format_args!("{}", self.shots));
        section.row("hits", format_args!("{}", self.hits));
        match self.accuracy() {
            // A dash and not `0%`, which is what a run that has fired and
            // missed reports — see [`Stats::accuracy`].
            Some(percent) => section.row("accuracy", format_args!("{percent:.0}%")),
            None => section.row_str("accuracy", "--"),
        }
        section.row("aim", format_args!("{}", self.crosshair.label()));
        for (lane, at) in LANE_LIST.iter().enumerate() {
            section.row(
                at.label,
                format_args!(
                    "{:.0} m  {}",
                    at.distance(),
                    if self.plates_down[lane] { "down" } else { "up" },
                ),
            );
        }
        section.row("mover", format_args!("{:.2} m", self.mover_x));
        section.row(
            "pilot",
            format_args!("{}", if self.warming_up { "range" } else { "player" }),
        );
    }
}

// ---------------------------------------------------------------------------
// The facade
// ---------------------------------------------------------------------------

/// What can stop breach before it starts.
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
            Box::new(BreachModule {
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

        // **One tick spent on the handshake, before the player moves.**
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
            "sim: {tick_hz} Hz, {:.3} ms per tick, walking at {WALK_SPEED} m/s, \
             pistol reaching {RANGE_M} m",
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
            fire: controls.fire,
            yaw: controls.yaw,
            pitch: controls.pitch,
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
        RenderState {
            position: stage.player.position(),
            feet: feet_of(&stage.player),
            eye: eye_of(&stage.player),
            grounded: stage.outcome.grounded,
            blocked: stage.outcome.hit_wall,
            plates_down: core::array::from_fn(|lane| stage.down_until[lane].is_some()),
            plates_x: core::array::from_fn(|lane| map::plate_x(lane, stage.elapsed)),
            crosshair: stage.crosshair,
            shots: stage.shots,
            hits: stage.hits,
            warming_up: stage.warming_up,
            imposed_aim: stage.warming_up.then_some(stage.aim),
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
            shots: stage.shots,
            hits: stage.hits,
            crosshair: stage.crosshair,
            plates_down: core::array::from_fn(|lane| stage.down_until[lane].is_some()),
            mover_x: map::plate_x(map::MOVER_LANE, stage.elapsed),
            warming_up: stage.warming_up,
            aim: stage.aim,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// One tick at the default rate.
    const DT: f64 = 1.0 / DEFAULT_TICK_HZ as f64;

    /// A stage that has already found the floor, with the warm-up switched off
    /// so a test drives it, squared up down the near lane.
    fn ready() -> Stage {
        let mut stage = Stage::new();
        stage.warming_up = false;
        run_tick(&mut stage, Intent::default(), DT);
        assert!(stage.outcome.grounded, "the spawn has no floor under it");
        stage
    }

    /// Holds `intent` for `seconds`.
    fn hold(stage: &mut Stage, intent: Intent, seconds: f64) {
        for _ in 0..(seconds / DT).round() as u64 {
            run_tick(stage, intent, DT);
        }
    }

    /// One trigger pull along `yaw` and `pitch`, and nothing else.
    fn shoot(stage: &mut Stage, yaw: f32, pitch: f32) {
        run_tick(
            stage,
            Intent {
                fire: true,
                yaw,
                pitch,
                ..Intent::default()
            },
            DT,
        );
    }

    /// **Held input walks the player and released input stops them**, which is
    /// the same pair the browser gate asserts and the reason it can be asserted
    /// there: if it were true only in a headless test, the browser check would
    /// be a check of the shim rather than of the controller.
    #[test]
    fn the_player_walks_while_asked_to_and_stops_when_they_are_not() {
        let mut stage = ready();
        let forward = Intent {
            forward: true,
            ..Intent::default()
        };
        let start = stage.player.position();
        hold(&mut stage, forward, 1.0);
        let walked = stage.player.position();
        let covered = (walked - start).length();
        assert!(
            covered > 0.5 * WALK_SPEED,
            "a second of walking covered {covered:.2} m at {WALK_SPEED} m/s",
        );
        // A yaw of zero walks down -Z, which is where the range is.
        assert!(walked.z < start.z - 0.5 * WALK_SPEED);

        hold(&mut stage, Intent::default(), 1.0);
        let stopped = stage.player.position();
        assert!(
            (stopped - walked).length() < 1e-6,
            "it drifted {:.6} m with nothing held",
            (stopped - walked).length(),
        );
    }

    /// **The firing line stops the player**, and it is the controller that
    /// stops them rather than anything in this file.
    #[test]
    fn the_player_cannot_walk_past_the_firing_line() {
        let mut stage = ready();
        let forward = Intent {
            forward: true,
            ..Intent::default()
        };
        // Long enough to cross the whole run-up several times over.
        hold(&mut stage, forward, 12.0);
        assert!(
            stage.blocked > 0,
            "it never pushed against anything on the way down the range",
        );
        assert!(
            stage.player.position().z > map::FIRING_LINE_Z,
            "it got down-range, to z = {:.2}",
            stage.player.position().z,
        );
        // And it did not climb the kerb on the way: the feet are still on the
        // floor rather than on top of the line.
        assert!(
            feet_of(&stage.player).abs() < 0.05,
            "the feet ended up at {:.3} m",
            feet_of(&stage.player),
        );
    }

    /// **A shot down the near lane scores, and a shot anywhere else does not.**
    /// The positive and its control, which is the same pair the browser gate
    /// makes — so a failure there is a failure here rather than a mystery about
    /// the browser.
    #[test]
    fn a_shot_at_a_plate_hits_and_a_shot_at_the_wall_misses() {
        let mut stage = ready();
        assert_eq!(stage.crosshair, Aim::Plate(0), "the near lane is not ahead");

        shoot(&mut stage, 0.0, 0.0);
        assert_eq!((stage.shots, stage.hits), (1, 1));
        assert!(stage.down_until[0].is_some(), "the plate did not go down");

        // A quarter turn puts the side wall in the crosshair, and nothing else.
        let aside = core::f32::consts::FRAC_PI_2;
        run_tick(
            &mut stage,
            Intent {
                yaw: aside,
                ..Intent::default()
            },
            DT,
        );
        assert_eq!(stage.crosshair, Aim::Range);
        shoot(&mut stage, aside, 0.0);
        assert_eq!(
            (stage.shots, stage.hits),
            (2, 1),
            "a shot at the wall scored",
        );
    }

    /// **A plate that is down cannot be hit again**, which is what makes a
    /// knock-down a knock-down rather than a colour change.
    ///
    /// Shot at from a pitch that finds the lying slab, so this is a claim about
    /// the geometry and the score rather than about the crosshair alone.
    #[test]
    fn a_plate_already_down_is_a_miss_however_it_is_shot() {
        let mut stage = ready();
        shoot(&mut stage, 0.0, 0.0);
        assert_eq!(stage.hits, 1);

        // Where the near plate is now lying, and the angles that point at it.
        let lying = map::plate_collider(LANE_LIST[0], map::plate_x(0, stage.elapsed), true);
        let (yaw, pitch) = aim_at(eye_of(&stage.player), lying.centre);
        #[allow(clippy::cast_possible_truncation)]
        let (yaw, pitch) = (yaw as f32, pitch as f32);
        run_tick(
            &mut stage,
            Intent {
                yaw,
                pitch,
                ..Intent::default()
            },
            DT,
        );
        assert_eq!(
            stage.crosshair,
            Aim::Downed,
            "the lying plate is not where the collider says it is",
        );
        shoot(&mut stage, yaw, pitch);
        assert_eq!((stage.shots, stage.hits), (2, 1), "a downed plate scored");
    }

    /// **A knocked-down plate stands back up after the delay**, and can be
    /// taken again once it has.
    #[test]
    fn a_plate_comes_back_up_and_can_be_taken_again() {
        let mut stage = ready();
        shoot(&mut stage, 0.0, 0.0);
        assert!(stage.down_until[0].is_some());

        // Well short of the delay: still down.
        hold(&mut stage, Intent::default(), PLATE_RESET_S * 0.5);
        assert!(stage.down_until[0].is_some(), "it stood up early");
        assert_eq!(stage.crosshair, Aim::Range, "the near lane is not clear");

        hold(&mut stage, Intent::default(), PLATE_RESET_S);
        assert!(stage.down_until[0].is_none(), "it never stood back up");
        assert_eq!(stage.crosshair, Aim::Plate(0));
        shoot(&mut stage, 0.0, 0.0);
        assert_eq!((stage.shots, stage.hits), (2, 2));
    }

    /// **Every lane can be hit from the firing point**, which is what the
    /// warm-up depends on and what a map edit would otherwise break silently.
    #[test]
    fn the_bearing_to_each_lane_is_a_hit() {
        for (lane, at) in LANE_LIST.iter().enumerate() {
            let mut stage = ready();
            let (yaw, pitch) = lane_bearing(lane, stage.elapsed);
            #[allow(clippy::cast_possible_truncation)]
            let (yaw, pitch) = (yaw as f32, pitch as f32);
            shoot(&mut stage, yaw, pitch);
            assert_eq!(
                (stage.shots, stage.hits),
                (1, 1),
                "the bearing to the {} lane missed it",
                at.label,
            );
            assert!(stage.down_until[lane].is_some());
        }
    }

    /// **An aim computed at a point points at it**, which is
    /// [`aim_at`] being the inverse of [`crate::camera::forward`] rather than
    /// a second guess at the same trigonometry.
    #[test]
    fn an_aim_computed_at_a_plate_points_at_it() {
        let eye = DVec3::new(0.4, EYE_HEIGHT, map::SPAWN_Z);
        for at in LANE_LIST {
            let target = DVec3::new(at.x, map::PLATE_CENTRE_Y + 0.1, at.z);
            let (yaw, pitch) = aim_at(eye, target);
            let along = forward(yaw, pitch);
            let want = (target - eye).normalize();
            assert!(
                (along - want).length() < 1e-12,
                "the {} lane's bearing points along {along} rather than {want}",
                at.label,
            );
        }
    }

    /// **The warm-up runs the range until somebody steps up, and stepping up
    /// resets it.**
    #[test]
    fn the_warmup_runs_until_somebody_takes_the_controls() {
        let mut stage = Stage::new();
        hold(&mut stage, Intent::default(), 3.0 * WARMUP_LANE_S);
        assert!(stage.warming_up, "a page with no input keeps the warm-up");
        assert!(stage.shots >= 3, "the warm-up fired {} shots", stage.shots);
        assert_eq!(
            stage.hits, stage.shots,
            "the warm-up missed: it aims at a plate and shoots after the swing",
        );
        assert!(
            stage.plates_down_count() > 0,
            "the warm-up never knocked anything down",
        );
        assert!(stage.warming_up, "the camera is not following the warm-up",);

        run_tick(
            &mut stage,
            Intent {
                back: true,
                yaw: 1.25,
                ..Intent::default()
            },
            DT,
        );
        assert!(
            !stage.warming_up,
            "a movement key did not take the controls"
        );
        assert_eq!((stage.shots, stage.hits), (0, 0), "the score did not reset");
        assert_eq!(stage.plates_down_count(), 0, "a plate stayed down");
        assert_eq!(
            stage.aim,
            (1.25, 0.0),
            "the tick that took the controls did not run at the player's aim",
        );

        hold(&mut stage, Intent::default(), 2.0 * WARMUP_LANE_S);
        assert!(
            !stage.warming_up,
            "the warm-up came back after the player let go"
        );
        assert!(
            !stage.warming_up,
            "the camera never came back to the player"
        );
    }

    /// The warm-up fires exactly once a lane, whatever the tick rate.
    #[test]
    fn the_warmup_fires_once_per_lane_at_any_tick_rate() {
        for hz in [30_u32, 60, 144] {
            let dt = 1.0 / f64::from(hz);
            let slots = 5;
            let ticks = (slots as f64 * WARMUP_LANE_S / dt).round() as u64;
            // The same elapsed values `run_tick` passes: zero on the first
            // tick, and a `dt` more on each after it.
            let fired = (0..ticks)
                .filter(|tick| warmup_fires(*tick as f64 * dt, dt))
                .count();
            assert_eq!(fired, slots, "at {hz} Hz the warm-up fired {fired} times");
        }
    }

    /// **The wire is validated rather than trusted.** These are the only bytes
    /// a peer chooses, and a `NaN` angle would reach both conversions.
    #[test]
    fn a_frame_this_build_did_not_write_is_refused() {
        let intent = Intent {
            forward: true,
            fire: true,
            yaw: 1.25,
            pitch: -0.5,
            ..Intent::default()
        };
        let wire = intent.to_wire();
        assert_eq!(wire.len(), INTENT_BYTES);
        assert_eq!(Intent::from_wire(&wire), Some(intent));

        assert_eq!(Intent::from_wire(&[]), None, "an empty frame");
        assert_eq!(Intent::from_wire(&wire[..5]), None, "a truncated frame");
        let mut long = wire.clone();
        long.push(0);
        assert_eq!(Intent::from_wire(&long), None, "an over-long frame");

        let mut unknown = wire.clone();
        unknown[0] |= 1 << 7;
        assert_eq!(Intent::from_wire(&unknown), None, "an undefined flag");

        for at in [1, 5] {
            for bad in [f32::NAN, f32::INFINITY] {
                let mut broken = wire.clone();
                broken[at..at + 4].copy_from_slice(&bad.to_le_bytes());
                assert_eq!(
                    Intent::from_wire(&broken),
                    None,
                    "an angle of {bad} at byte {at}",
                );
            }
        }
    }

    /// Both movement keys held is neither, a trigger pull is activity and a
    /// view angle is not, and the angles are the last that arrived.
    #[test]
    fn opposing_keys_cancel_and_a_look_is_not_a_step_up() {
        let both = Intent {
            forward: true,
            back: true,
            left: true,
            right: true,
            ..Intent::default()
        };
        assert_eq!(both.ahead(), 0.0);
        assert_eq!(both.across(), 0.0);
        assert!(both.is_active(), "the keys are still down");
        assert!(
            Intent {
                fire: true,
                ..Intent::default()
            }
            .is_active(),
            "a trigger pull is stepping up to the line",
        );
        assert!(
            !Intent {
                yaw: 3.0,
                pitch: -0.4,
                ..Intent::default()
            }
            .is_active(),
            "looking around is not stepping up to the line",
        );
    }

    /// **A tick nothing arrived for keeps the aim and drops the buttons.**
    ///
    /// The two halves default in opposite directions and both are asserted
    /// here, because getting either one backwards is invisible until it is
    /// not: a defaulted aim swings the view to due north for one tick and
    /// sends that tick's shot into a wall, and a held button on a dropped
    /// frame is a player who cannot stop walking.
    #[test]
    fn a_tick_nothing_arrived_for_keeps_the_aim_and_lets_the_keys_go() {
        let held = (1.25_f32, -0.4_f32);
        let starved = Intent::from_inputs(ClientInputs::empty(), held);
        assert_eq!((starved.yaw, starved.pitch), held, "the aim was not held");
        assert!(!starved.is_active(), "a dropped frame held a key down");

        // The control: a frame that did arrive is what the tick runs at.
        let sent = Intent {
            forward: true,
            yaw: 0.25,
            pitch: 0.1,
            ..Intent::default()
        };
        let frames = [(crcbl::core::TickId::ZERO, sent.to_wire())];
        let arrived = Intent::from_inputs(ClientInputs::new(&frames, 0), held);
        assert_eq!(arrived, sent, "the frame that arrived was not what ran");
    }

    /// Accuracy is a percentage of what was actually fired, and nothing before
    /// the first shot.
    #[test]
    fn accuracy_is_none_until_something_has_been_fired() {
        let mut stats = Stats::default();
        assert_eq!(stats.accuracy(), None);
        stats.shots = 4;
        stats.hits = 3;
        assert!((stats.accuracy().expect("four shots") - 75.0).abs() < 1e-12);
        stats.hits = 0;
        assert_eq!(stats.accuracy(), Some(0.0), "a run that has missed is 0%");
    }

    impl Stage {
        /// How many plates are lying down — a test helper, so the assertions
        /// above read as claims rather than as index arithmetic.
        fn plates_down_count(&self) -> usize {
            self.down_until.iter().filter(|down| down.is_some()).count()
        }
    }
}
