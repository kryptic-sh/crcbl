//! The simulation: one capsule walking [`crate::map`], one hitscan pistol
//! shooting down it, and the server that owns both.
//!
//! # Two maps, one tick
//!
//! [`MapChoice`] says which of them a run opened on, and `Arena` is the half
//! of the stage that differs: the firing range's plates and its demonstration,
//! or the practice map's bots. Everything above that line — the walk, the ray,
//! the wire, the score — is one implementation, which is the point of building
//! the second map inside this sample rather than beside it.
//!
//! The practice map has **no warm-up**: it does not need one, because three
//! bots walking their patrols is already a picture that moves, and one of them
//! is shooting at a visitor who has not touched anything. `crate::bots` owns
//! what a bot does; what this file owns is the order it happens in and what a
//! round that arrives takes off the player.
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

use crate::bots::{self, Bot};
use crate::camera::{EYE_HEIGHT, forward, walk_direction};
use crate::map::practice::{BOTS, BotView};
use crate::map::{self, LANE_LIST, LANES, MapChoice};

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

/// How often the `[HUD]` heartbeat is logged, in ticks: **half** a second of
/// simulated time at [`DEFAULT_TICK_HZ`].
///
/// Twice as often as every other sample's, and that is this demo's gate paying
/// for itself. `web/tools/browser-e2e.mjs` reads a dozen claims out of this line
/// — a walk that advances and stops, a shot that scores and one that does not, a
/// view that turns, a patrol that walks, a sighting that cover breaks — and
/// every one of them waits a whole number of heartbeats for its answer. On the
/// software rasteriser the browser gate runs on, a simulated second is nearly
/// three wall seconds, so the heartbeat period *is* what that gate costs: the
/// Pages run of 2026-08-26 lost breach's step to a ten-minute cap with every
/// assertion in it green. Halving the period halves the waiting and changes no
/// claim.
///
/// What it costs is one more log line a second. The driver is told the shorter
/// period through its `beatMs` row, so the slowdown it scales every other budget
/// by stays a true reading of how far behind real time the demo is running.
pub const HEARTBEAT_TICKS: u64 = 30;

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
    /// A bot on the practice map, and which one. **The only other thing in
    /// this sample that scores**, and the practice map's answer to a plate.
    Bot(usize),
    /// Some other surface of the map — a wall, the floor, the ceiling, a
    /// plate's post, the kerb, or a block of cover.
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
            Self::Bot(bot) => map::practice::ROUTES[bot].label,
            Self::Downed => "down",
            Self::Range => "range",
            Self::Nothing => "none",
        }
    }

    /// Whether a shot along this line scores.
    #[must_use]
    pub const fn scores(self) -> bool {
        matches!(self, Self::Plate(_) | Self::Bot(_))
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

/// The half of the stage that belongs to the map rather than to the player.
///
/// One arm per [`MapChoice`], because the two maps genuinely have nothing in
/// common below the player: the range has plates on timers and a demonstration
/// that runs itself, and the practice map has bots. A struct carrying both would
/// give every range run an empty bot list to step and every practice run three
/// plates nothing can hit.
enum Arena {
    /// The firing range.
    Range(Plates),
    /// The bot practice map.
    Practice(Squad),
}

/// The range's own state.
struct Plates {
    /// The plates' colliders, near lane first — what a ray's answer is compared
    /// against, and what a knock-down moves.
    ids: [ColliderId; LANES],
    /// When each plate stands back up, in [`Stage::elapsed`] seconds, or `None`
    /// for a plate that is already standing.
    down_until: [Option<f64>; LANES],
    /// Whether the range is still running its own demonstration.
    warming_up: bool,
}

/// The practice map's own state.
struct Squad {
    /// One bot per [`crate::map::practice::ROUTES`] row.
    bots: Vec<Bot>,
    /// What the player has left, out of [`bots::HEALTH_MAX`].
    health: u32,
    /// How many times they have been put down and respawned.
    downs: u64,
    /// How many rounds the bots have fired.
    fired: u64,
    /// How many of them reached the player. The difference between the two is
    /// cover, and nothing else — see [`crate::bots`].
    taken: u64,
    /// How many bots had the player in sight at the end of the last tick, and
    /// how many were in range and could not see them anyway.
    ///
    /// Kept rather than recomputed when the panel asks, because the answer is
    /// three ray casts and the tick has already paid for them.
    seen: usize,
    covered: usize,
}

/// Everything this sample simulates.
///
/// Behind an `Arc<Mutex<_>>` shared with [`BreachModule`], for the reason
/// `apps/orbit` gives: the module is what the server ticks and the frame is what
/// reads the result, and the two are not the same call stack.
struct Stage {
    world: PhysicsWorld,
    /// Whichever of the two maps this run opened on.
    arena: Arena,
    player: CharacterController,
    /// How fast the player is falling, in metres a second, negative downward.
    /// Zeroed the moment they are grounded.
    fall_speed: f64,
    ticks: u64,
    /// Seconds of **simulated** time, accumulated a tick at a time. What the
    /// plates' timers are measured against, so a paused demo's plates stay
    /// where they are.
    elapsed: f64,
    shots: u64,
    hits: u64,
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
    /// The player on `map`'s spawn, ungrounded until the first move finds the
    /// floor, with the map's own furniture in its opening state.
    fn new(map: MapChoice) -> Self {
        let config = CharacterConfig::default();
        let lift = DVec3::Y * (config.radius + config.half_height);
        let (world, arena, spawn) = match map {
            MapChoice::Range => {
                let (world, ids) = map::world();
                (
                    world,
                    Arena::Range(Plates {
                        ids,
                        down_until: [None; LANES],
                        warming_up: true,
                    }),
                    map::SPAWN,
                )
            }
            MapChoice::Practice => {
                let mut world = map::practice::world();
                let bots = bots::spawn_all(&mut world);
                (
                    world,
                    Arena::Practice(Squad {
                        bots,
                        health: bots::HEALTH_MAX,
                        downs: 0,
                        fired: 0,
                        taken: 0,
                        seen: 0,
                        covered: 0,
                    }),
                    map::practice::SPAWN,
                )
            }
        };
        Self {
            world,
            arena,
            player: CharacterController::new(config, spawn + lift),
            fall_speed: 0.0,
            ticks: 0,
            elapsed: 0.0,
            shots: 0,
            hits: 0,
            aim: (0.0, 0.0),
            crosshair: Aim::Nothing,
            outcome: MoveOutcome::default(),
            blocked: 0,
        }
    }

    /// Which map this run opened on.
    const fn map(&self) -> MapChoice {
        match self.arena {
            Arena::Range(_) => MapChoice::Range,
            Arena::Practice(_) => MapChoice::Practice,
        }
    }

    /// Whether the range is still running its own demonstration. Always false on
    /// the practice map, which has bots walking about instead and hands the
    /// controls over from the first tick.
    const fn warming_up(&self) -> bool {
        match &self.arena {
            Arena::Range(plates) => plates.warming_up,
            Arena::Practice(_) => false,
        }
    }

    /// What a ray's answer means: a plate, a bot, or the room.
    fn what_is(&self, id: ColliderId) -> Aim {
        match &self.arena {
            Arena::Range(plates) => match plates.ids.iter().position(|&plate| plate == id) {
                // A plate whose collider is in its lying-down pose can still be
                // hit — by a shot aimed at the floor — and that is a miss, not
                // a second hit on a target already taken.
                Some(lane) if plates.down_until[lane].is_some() => Aim::Downed,
                Some(lane) => Aim::Plate(lane),
                None => Aim::Range,
            },
            Arena::Practice(squad) => squad
                .bots
                .iter()
                .position(|bot| bot.body() == id && bot.is_alive())
                .map_or(Aim::Range, Aim::Bot),
        }
    }

    /// Writes a plate's collider where it is now: standing or knocked flat, at
    /// wherever [`map::plate_x`] puts it at this instant.
    ///
    /// **The collider and nothing else**, since the mesh is the frame's
    /// business — [`RenderState`] is what carries the same two facts across.
    ///
    /// A no-op on a map with no plates on it, which is the honest shape: the
    /// callers below are the range's own tick.
    fn set_plate(&mut self, lane: usize, down: bool) {
        let Arena::Range(plates) = &self.arena else {
            return;
        };
        let id = plates.ids[lane];
        let at = map::plate_collider(LANE_LIST[lane], map::plate_x(lane, self.elapsed), down);
        self.world.set_box(id, at);
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
        let was_down = match &mut stage.arena {
            Arena::Range(plates) => plates.down_until[lane].take().is_some(),
            Arena::Practice(_) => false,
        };
        if was_down {
            stage.set_plate(lane, false);
        }
    }
    stage.shots = 0;
    stage.hits = 0;
}

/// Stands the plates whose delay has expired back up, and moves the travelling
/// one to where this instant puts it.
///
/// Run **before** the shot is resolved, so a plate whose delay expires on this
/// tick can be taken again on it, and so a mover whose collider lagged its mesh
/// is never a target that cannot be hit where it is drawn.
fn step_plates(stage: &mut Stage) {
    for lane in 0..LANES {
        let due = match &stage.arena {
            Arena::Range(plates) => {
                plates.down_until[lane].is_some_and(|until| stage.elapsed >= until)
            }
            Arena::Practice(_) => false,
        };
        if due {
            if let Arena::Range(plates) = &mut stage.arena {
                plates.down_until[lane] = None;
            }
            stage.set_plate(lane, false);
        }
    }
    let mover_down = match &stage.arena {
        Arena::Range(plates) => plates.down_until[map::MOVER_LANE].is_some(),
        Arena::Practice(_) => return,
    };
    stage.set_plate(map::MOVER_LANE, mover_down);
}

/// One tick of the practice map: every bot walks its patrol, looks for the
/// player and takes its shot if one is due.
///
/// **Nothing here decides where a bot goes** — see [`crate::bots`], and
/// `docs/plan/24-navigation.md` for why breach is not the sample that forces a
/// navmesh. What this function owns is the order the three happen in and what a
/// round that arrives does to the player.
fn step_bots(stage: &mut Stage, dt: f64) {
    let Stage {
        world,
        arena,
        player,
        fall_speed,
        elapsed,
        ..
    } = stage;
    let Arena::Practice(squad) = arena else {
        return;
    };
    let now = *elapsed;

    for bot in &mut squad.bots {
        bot.patrol(world, now, dt);
    }

    // The sighting comes after the walk, so a bot that stepped out from behind
    // the pillar this tick is seen on it rather than on the next one.
    let eye = eye_of(player);
    squad.seen = 0;
    squad.covered = 0;
    for index in 0..squad.bots.len() {
        let in_sight = bots::has_line_of_sight(world, eye, &squad.bots[index]);
        if in_sight {
            squad.seen += 1;
            squad.bots[index].notice(now);
        } else if bots::is_within_notice(eye, &squad.bots[index]) {
            squad.covered += 1;
        }
        if squad.bots[index].wants_to_shoot(now) {
            squad.fired += 1;
            // **The round is resolved along the segment the sighting is.** A
            // bot that has just lost the player goes on shooting at where they
            // were, and cover is what those rounds land in — which is the whole
            // of the difference between `fired` and `taken`.
            if in_sight {
                squad.taken += 1;
                squad.health = squad.health.saturating_sub(bots::BOT_DAMAGE);
            }
        }
    }

    if squad.health == 0 {
        squad.health = bots::HEALTH_MAX;
        squad.downs += 1;
        let config = *player.config();
        player.set_position(map::practice::SPAWN + DVec3::Y * (config.radius + config.half_height));
        *fall_speed = 0.0;
    }
}

/// One tick of the simulation: an intent in, a displacement and a ray through
/// the world.
fn run_tick(stage: &mut Stage, player: Intent, dt: f64) {
    // The first thing the player asks for ends the warm-up and resets the
    // range. The same tick is the first one they drive, so the handover costs
    // nothing here — squaring the shooter up is the client's to do, because
    // the view is the client's: see [`RenderState::imposed_aim`].
    if stage.warming_up() && player.is_active() {
        if let Arena::Range(plates) = &mut stage.arena {
            plates.warming_up = false;
        }
        reset_range(stage);
    }
    let intent = if stage.warming_up() {
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

    // **The map's own furniture moves before the shot is resolved**: the
    // range's plates come back up and its mover travels, and the practice map's
    // bots walk, look and shoot. A target that had not been moved yet is one
    // that cannot be hit where it is drawn.
    match stage.map() {
        MapChoice::Range => step_plates(stage),
        MapChoice::Practice => step_bots(stage, dt),
    }

    // **The shot conversion**, and the one ray this tick casts — see the module
    // docs for why the crosshair and the trigger share it.
    let eye = eye_of(&stage.player);
    let along = forward(f64::from(intent.yaw), f64::from(intent.pitch));
    stage.crosshair = trace(stage, eye, along);

    if intent.fire {
        stage.shots += 1;
        match stage.crosshair {
            Aim::Plate(lane) => {
                stage.hits += 1;
                if let Arena::Range(plates) = &mut stage.arena {
                    plates.down_until[lane] = Some(stage.elapsed + PLATE_RESET_S);
                }
                stage.set_plate(lane, true);
            }
            Aim::Bot(index) => {
                stage.hits += 1;
                let now = stage.elapsed;
                let Stage { world, arena, .. } = stage;
                if let Arena::Practice(squad) = arena {
                    squad.bots[index].down(world, now);
                }
            }
            Aim::Downed | Aim::Range | Aim::Nothing => {}
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
    stage.what_is(id)
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

/// What a frame draws that belongs to the map rather than to the player.
///
/// An enum rather than a struct with both maps' fields on it, for `Arena`'s
/// reason: a range frame has no bots to draw and a practice frame has no plates,
/// and a caller that had to guess which half was live would guess wrong exactly
/// once.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Scene {
    /// The firing range's three plates.
    Range {
        /// Which are lying down, near lane first.
        plates_down: [bool; LANES],
        /// Where each is across the range, near lane first — the same
        /// [`map::plate_x`] the colliders were written at, so the picture and
        /// the physics are one instant rather than two.
        plates_x: [f64; LANES],
    },
    /// The practice map's bots, and what the player has left.
    Practice {
        /// One view per [`crate::map::practice::ROUTES`] row.
        bots: [BotView; BOTS],
        /// The player's health, out of [`bots::HEALTH_MAX`].
        health: u32,
    },
}

impl Default for Scene {
    /// The range as it stands before the first tick, because
    /// [`MapChoice::default`] is the range.
    fn default() -> Self {
        Self::Range {
            plates_down: [false; LANES],
            plates_x: core::array::from_fn(|lane| map::plate_x(lane, 0.0)),
        }
    }
}

impl Scene {
    /// Which map this is a frame of.
    #[must_use]
    pub const fn map(&self) -> MapChoice {
        match self {
            Self::Range { .. } => MapChoice::Range,
            Self::Practice { .. } => MapChoice::Practice,
        }
    }
}

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
    /// the firing line, a block of cover, or a wall.
    pub blocked: bool,
    /// What the map has in it, which is the half of the frame that differs
    /// between the two.
    pub scene: Scene,
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

/// The numbers that belong to one map or the other, for the panel and the
/// `[HUD]` line.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum ArenaStats {
    /// The firing range's.
    Range {
        /// Which plates are lying down, near lane first.
        plates_down: [bool; LANES],
        /// Where the travelling plate is across the range, in metres —
        /// [`map::plate_x`] at [`map::MOVER_LANE`].
        ///
        /// **The one number on the range's heartbeat that nothing a player does
        /// can move**, which is what `web/tools/browser-e2e.mjs` reads to tell
        /// a running demo from a stalled one.
        mover_x: f64,
    },
    /// The practice map's.
    Practice {
        /// How many bots are on their feet.
        alive: usize,
        /// How many of those have the player in sight this tick.
        seen: usize,
        /// How many are within [`bots::NOTICE_M`] and cannot see the player
        /// anyway, because something is in the way.
        ///
        /// **The control for `seen`, on the line itself.** A build that noticed
        /// unconditionally reports a `seen` that rises and a `covered` that is
        /// always zero, which is the failure a sighting check on its own cannot
        /// tell from success.
        covered: usize,
        /// What the player has left, out of [`bots::HEALTH_MAX`].
        health: u32,
        /// How many times they have been put down and respawned.
        downs: u64,
        /// How many rounds the bots have fired.
        fired: u64,
        /// How many of them reached the player. `fired` above `taken` is cover
        /// having stopped a round — see [`crate::bots`].
        taken: u64,
        /// Where the first bot's feet are, in metres.
        ///
        /// **The number that says the patrol is walking**, and it is a *bot's*
        /// rather than the player's: nothing on this map moves it but the
        /// bot's own `move_and_slide`.
        lead: DVec3,
    },
}

impl Default for ArenaStats {
    /// The range's, because [`MapChoice::default`] is the range.
    fn default() -> Self {
        Self::Range {
            plates_down: [false; LANES],
            mover_x: map::plate_x(map::MOVER_LANE, 0.0),
        }
    }
}

impl ArenaStats {
    /// Which map these are the numbers of.
    #[must_use]
    pub const fn map(&self) -> MapChoice {
        match self {
            Self::Range { .. } => MapChoice::Range,
            Self::Practice { .. } => MapChoice::Practice,
        }
    }
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
    /// Whichever map's numbers this run has.
    pub arena: ArenaStats,
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

/// Whichever map's numbers this stage has, for [`Stats`].
///
/// A free function rather than a method because it reads the arena and the
/// clock and nothing else, and because the two arms are what the panel and the
/// `[HUD]` line branch on — keeping the branch here is what stops either of them
/// growing its own.
fn arena_stats(stage: &Stage) -> ArenaStats {
    match &stage.arena {
        Arena::Range(plates) => ArenaStats::Range {
            plates_down: core::array::from_fn(|lane| plates.down_until[lane].is_some()),
            mover_x: map::plate_x(map::MOVER_LANE, stage.elapsed),
        },
        Arena::Practice(squad) => ArenaStats::Practice {
            alive: squad.bots.iter().filter(|bot| bot.is_alive()).count(),
            seen: squad.seen,
            covered: squad.covered,
            health: squad.health,
            downs: squad.downs,
            fired: squad.fired,
            taken: squad.taken,
            lead: squad.bots.first().map_or(DVec3::ZERO, Bot::feet),
        },
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
        section.row_str("map", self.arena.map().name());
        match self.arena {
            ArenaStats::Range {
                plates_down,
                mover_x,
            } => {
                for (lane, at) in LANE_LIST.iter().enumerate() {
                    section.row(
                        at.label,
                        format_args!(
                            "{:.0} m  {}",
                            at.distance(),
                            if plates_down[lane] { "down" } else { "up" },
                        ),
                    );
                }
                section.row("mover", format_args!("{mover_x:.2} m"));
            }
            ArenaStats::Practice {
                alive,
                seen,
                covered,
                health,
                downs,
                fired,
                taken,
                lead,
            } => {
                section.row("bots", format_args!("{alive}/{BOTS}"));
                section.row("seen", format_args!("{seen}"));
                section.row("covered", format_args!("{covered}"));
                section.row("health", format_args!("{health}/{}", bots::HEALTH_MAX));
                section.row("downs", format_args!("{downs}"));
                section.row("incoming", format_args!("{taken}/{fired}"));
                section.row("lead", format_args!("{:.1} {:.1}", lead.x, lead.z));
            }
        }
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
    /// Builds the server, its client and the stage between them, on `map`.
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
    pub fn new(tick_hz: u32, map: MapChoice) -> Result<Self, GameError> {
        assert!(tick_hz > 0, "tick rate must be positive");
        let shared = Arc::new(Mutex::new(Stage::new(map)));

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
            "sim: {tick_hz} Hz, {:.3} ms per tick on the {} map, walking at {WALK_SPEED} m/s, \
             pistol reaching {RANGE_M} m",
            tick_period.as_secs_f64() * 1e3,
            map.name(),
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
            scene: match &stage.arena {
                Arena::Range(plates) => Scene::Range {
                    plates_down: core::array::from_fn(|lane| plates.down_until[lane].is_some()),
                    plates_x: core::array::from_fn(|lane| map::plate_x(lane, stage.elapsed)),
                },
                Arena::Practice(squad) => Scene::Practice {
                    bots: bots::views(&squad.bots, stage.elapsed),
                    health: squad.health,
                },
            },
            crosshair: stage.crosshair,
            shots: stage.shots,
            hits: stage.hits,
            warming_up: stage.warming_up(),
            imposed_aim: stage.warming_up().then_some(stage.aim),
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
            arena: arena_stats(&stage),
            warming_up: stage.warming_up(),
            aim: stage.aim,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// One tick at the default rate.
    const DT: f64 = 1.0 / DEFAULT_TICK_HZ as f64;

    /// A range that has already found the floor, with the warm-up switched off
    /// so a test drives it, squared up down the near lane.
    fn ready() -> Stage {
        let mut stage = Stage::new(MapChoice::Range);
        if let Arena::Range(plates) = &mut stage.arena {
            plates.warming_up = false;
        }
        run_tick(&mut stage, Intent::default(), DT);
        assert!(stage.outcome.grounded, "the spawn has no floor under it");
        stage
    }

    /// A practice map that has already found the floor. It has no warm-up to
    /// switch off — see the module docs.
    fn practice() -> Stage {
        let mut stage = Stage::new(MapChoice::Practice);
        run_tick(&mut stage, Intent::default(), DT);
        assert!(stage.outcome.grounded, "the spawn has no floor under it");
        stage
    }

    /// Whether one of the range's plates is lying down.
    ///
    /// A helper so the assertions below read as claims about the range rather
    /// than as a match on which arena the stage is in.
    fn plate_down(stage: &Stage, lane: usize) -> bool {
        match &stage.arena {
            Arena::Range(plates) => plates.down_until[lane].is_some(),
            Arena::Practice(_) => panic!("this stage has no plates"),
        }
    }

    /// The practice map's numbers, whichever tick they were read on.
    fn squad(stage: &Stage) -> (usize, usize, usize, u32, u64, u64, u64) {
        match arena_stats(stage) {
            ArenaStats::Practice {
                alive,
                seen,
                covered,
                health,
                downs,
                fired,
                taken,
                ..
            } => (alive, seen, covered, health, downs, fired, taken),
            ArenaStats::Range { .. } => panic!("this stage has no bots"),
        }
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
        assert!(plate_down(&stage, 0), "the plate did not go down");

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
        assert!(plate_down(&stage, 0));

        // Well short of the delay: still down.
        hold(&mut stage, Intent::default(), PLATE_RESET_S * 0.5);
        assert!(plate_down(&stage, 0), "it stood up early");
        assert_eq!(stage.crosshair, Aim::Range, "the near lane is not clear");

        hold(&mut stage, Intent::default(), PLATE_RESET_S);
        assert!(!plate_down(&stage, 0), "it never stood back up");
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
            assert!(plate_down(&stage, lane));
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
        let mut stage = Stage::new(MapChoice::Range);
        hold(&mut stage, Intent::default(), 3.0 * WARMUP_LANE_S);
        assert!(stage.warming_up(), "a page with no input keeps the warm-up");
        assert!(stage.shots >= 3, "the warm-up fired {} shots", stage.shots);
        assert_eq!(
            stage.hits, stage.shots,
            "the warm-up missed: it aims at a plate and shoots after the swing",
        );
        assert!(
            stage.plates_down_count() > 0,
            "the warm-up never knocked anything down",
        );
        assert!(
            stage.warming_up(),
            "the camera is not following the warm-up",
        );

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
            !stage.warming_up(),
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
            !stage.warming_up(),
            "the warm-up came back after the player let go"
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

    /// **A bot in the open sees the player and a bot behind the pillar does
    /// not**, read off the same pair of counters the browser gate does.
    ///
    /// The positive and its control in one run, with nobody touching anything:
    /// a build that noticed unconditionally leaves `covered` at zero for ever,
    /// and one that never noticed leaves `seen` there. The bots' own patrols
    /// are what move them in and out of cover — see
    /// [`crate::map::practice::ROUTES`].
    #[test]
    fn a_bot_in_the_open_is_seen_and_one_behind_the_pillar_is_not() {
        let mut stage = practice();
        let mut ever_seen = 0;
        let mut ever_covered = 0;
        // A whole lap of the longest patrol, which is when every bot has been
        // on both sides of the pillar.
        for _ in 0..(30.0 / DT) as u64 {
            run_tick(&mut stage, Intent::default(), DT);
            let (_, seen, covered, ..) = squad(&stage);
            ever_seen = ever_seen.max(seen);
            ever_covered = ever_covered.max(covered);
        }
        assert!(
            ever_seen > 0,
            "no bot ever saw the player standing in the open in front of them",
        );
        assert!(
            ever_covered > 0,
            "every bot saw the player from everywhere, so the cover does nothing",
        );
    }

    /// **A bot's round takes the player's health, and a round with cover in the
    /// way does not.** The second is the control for the first: a build whose
    /// bots hit whatever they fired at reports `fired` and `taken` in step for
    /// the whole run.
    #[test]
    fn a_round_that_arrives_costs_health_and_one_that_is_blocked_does_not() {
        let mut stage = practice();
        for _ in 0..(30.0 / DT) as u64 {
            run_tick(&mut stage, Intent::default(), DT);
        }
        let (_, _, _, health, downs, fired, taken) = squad(&stage);
        assert!(taken > 0, "nothing ever hit the player in thirty seconds");
        assert!(
            health < bots::HEALTH_MAX || downs > 0,
            "the player took {taken} round(s) and still has {health} health",
        );
        assert!(
            fired > taken,
            "every one of the {fired} round(s) fired arrived, so cover stopped none of them",
        );
    }

    /// **The player is put back on the spawn with their health when the bots
    /// run them out of it**, rather than standing there at zero.
    #[test]
    fn the_player_respawns_when_the_bots_run_them_out_of_health() {
        let mut stage = practice();
        let mut downed = false;
        for _ in 0..(120.0 / DT) as u64 {
            run_tick(&mut stage, Intent::default(), DT);
            if squad(&stage).4 > 0 {
                downed = true;
                break;
            }
        }
        assert!(downed, "the bots never ran the player out of health");
        let (_, _, _, health, ..) = squad(&stage);
        assert_eq!(health, bots::HEALTH_MAX, "a respawn left the player hurt");
        let feet = feet_of(&stage.player);
        let position = stage.player.position();
        assert!(
            (position.x - map::practice::SPAWN.x).abs() < 0.1
                && (position.z - map::practice::SPAWN.z).abs() < 0.1
                && feet.abs() < 0.1,
            "they came back at {:.2} {feet:.2} {:.2}",
            position.x,
            position.z,
        );
    }

    /// **A shot at a bot puts it down, and it comes back on its own route.**
    ///
    /// Aimed at the bot the map keeps in the open, so this is a claim about
    /// what a hit does rather than about whether one was possible. The control
    /// is inside it: a downed bot is no longer something the crosshair reports,
    /// which is what makes the second shot below meaningful.
    #[test]
    fn a_shot_at_a_bot_puts_it_down_and_it_gets_back_up() {
        let mut stage = practice();
        let (target, before) = (2, squad(&stage).0);
        assert_eq!(
            before,
            map::practice::BOTS,
            "a bot was down before the shot"
        );

        let eye = eye_of(&stage.player);
        let at = match &stage.arena {
            Arena::Practice(squad) => bots::eye_of(squad.bots[target].feet()),
            Arena::Range(_) => unreachable!("this stage is the practice map"),
        };
        let (yaw, pitch) = aim_at(eye, at);
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
            Aim::Bot(target),
            "the crosshair is on {:?} rather than on the bot it was pointed at",
            stage.crosshair,
        );

        shoot(&mut stage, yaw, pitch);
        assert_eq!((stage.shots, stage.hits), (1, 1));
        assert_eq!(squad(&stage).0, before - 1, "the bot stayed on its feet");

        // One more tick along the same bearing, because the crosshair is
        // resolved *before* the trigger is on the tick that fires — so the
        // reading that says the body has stopped stopping rays is the next
        // one.
        run_tick(
            &mut stage,
            Intent {
                yaw,
                pitch,
                ..Intent::default()
            },
            DT,
        );
        assert_ne!(
            stage.crosshair,
            Aim::Bot(target),
            "a downed bot is still what the crosshair reports",
        );

        hold(&mut stage, Intent::default(), bots::BOT_RESPAWN_S + 1.0);
        assert_eq!(squad(&stage).0, before, "the bot never got back up");
    }

    /// **The practice map hands the controls over from the first tick**, and
    /// therefore never imposes an aim on the camera: it has three bots walking
    /// about in it, which is what the range needs a demonstration to stand in
    /// for.
    #[test]
    fn the_practice_map_has_no_warm_up_because_it_does_not_need_one() {
        let mut stage = Stage::new(MapChoice::Practice);
        hold(&mut stage, Intent::default(), 3.0 * WARMUP_LANE_S);
        assert!(!stage.warming_up(), "the practice map ran a demonstration");
        assert_eq!(
            stage.shots, 0,
            "something pulled the player's trigger for them",
        );
        // And the bots are what is moving instead. Read off the same field the
        // `[HUD]` line carries, which is a *bot's* position and not the
        // player's — a stage that moved the player would fail this.
        let lead = match arena_stats(&stage) {
            ArenaStats::Practice { lead, .. } => lead,
            ArenaStats::Range { .. } => unreachable!("this stage is the practice map"),
        };
        assert!(
            (lead - map::practice::ROUTES[0].waypoints[0]).length() > 1.0,
            "the first bot is still on its first waypoint at {lead}",
        );
        assert!(
            (stage.player.position()
                - (map::practice::SPAWN
                    + DVec3::Y
                        * (stage.player.config().radius + stage.player.config().half_height)))
                .length()
                < 0.05,
            "the player moved without being asked to",
        );
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
            (0..LANES).filter(|&lane| plate_down(self, lane)).count()
        }
    }
}
