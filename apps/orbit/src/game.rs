//! The flight: this sample's whole simulation, and the state a frame draws.
//!
//! ```text
//!  Flight ──▶ OrbitModule ──▶ Server ──┐                     ┌──▶ Client
//!  (this file)                         └── InMemoryTransport ┘
//!                     │
//!                     └──▶ RenderState ──▶ crate::page
//! ```
//!
//! # It is the physics pillar's acceptance test
//!
//! `docs/plan/sample/06-orbit.md` calls itself "a physics acceptance test
//! wearing a rocket costume", and nothing here reimplements physics: gravity is
//! [`crcbl::phys::PointGravity`], the air is [`crcbl::phys::Atmosphere`] and its
//! [`crcbl::phys::AtmosphericDrag`], live integration is
//! [`crcbl::phys::SemiImplicitEuler`], the frames and their sphere-of-influence
//! crossings are [`crcbl::phys::Frames`], and coasting under timewarp is
//! [`crcbl::phys::propagate`]. What is this sample's is the vehicle, the
//! controls and the flight plan.
//!
//! # The bubble, and what drops out of it
//!
//! Timewarp is on-rails: above the atmosphere with the engine shut down, a tick
//! is one analytic step of whatever conic the ship is on, however many seconds
//! long. That is only valid while gravity is the only force, so the warp drops
//! to ×1 the moment either stops being true — a burn, or the top of the
//! atmosphere on the way down. The drop is not a courtesy; a warped tick under
//! thrust would integrate nothing and the ship would coast through its own
//! burn.
//!
//! # A planet sized for a game, not for Earth
//!
//! Orbital velocity at Earth is 7.8 km/s and a real ascent is eight minutes of
//! burn. This planet is the size Kerbal Space Program picked for the same
//! reason — 600 km of radius at one Earth gravity — so orbit is about 2.2 km/s
//! and a minute of climbing, which is a demo rather than a commute. Every
//! constant below says which real quantity it stands in for.

use std::sync::{Arc, Mutex, MutexGuard};
use std::time::Duration;

use crcbl::core::WorldPos;
use crcbl::core::rand::hash_u64;
use crcbl::ecs::{ClientInputs, GameModule, World};
use crcbl::net::ProtocolCompatibility;
use crcbl::phys::{
    Atmosphere, AtmosphericDrag, FrameId, Frames, Integrator, Orbit, PointGravity, RigidBody,
    SemiImplicitEuler, State, Transform, propagate, sphere_of_influence,
};
use crcbl::session::Loopback;
use glam::{DQuat, DVec3};

/// Distinct from every other sample's, because they are distinct protocols: a
/// client built for one must not hand-shake with a server running another. The
/// low half spells `ORB`.
const COMPATIBILITY: ProtocolCompatibility = ProtocolCompatibility {
    protocol_version: 3,
    engine_build_id: 0x0043_5243_424C,
    schema_hash: 0x0000_004F_5242,
};

/// The default simulation rate. Reaches the server, the client and the flight,
/// so there is exactly one rate in the process.
pub const DEFAULT_TICK_HZ: u32 = 60;

/// Physics substeps inside one tick.
///
/// Four at [`DEFAULT_TICK_HZ`] is 240 Hz, the top of the band
/// `docs/plan/05-physics.md` asks for. A rocket at full throttle changes
/// velocity by about 25 m/s a second, and the drag it feels goes as the square
/// of that, so the substep is what keeps the ascent from over-shooting its own
/// deceleration low down.
pub const SUBSTEPS: u32 = 4;

// ---- the planet, the moon and the air ----------------------------------------

/// The planet's radius, in metres. Altitude is measured from it.
pub const PLANET_RADIUS: f64 = 600_000.0;

/// The planet's gravitational parameter `GM`, in m³/s².
///
/// Chosen so that `mu / r²` is 9.81 m/s² at the surface: the planet is small
/// and dense rather than small and weak, which is what keeps a rocket's
/// thrust-to-weight ratio meaningful while orbit stays a minute away.
pub const PLANET_MU: f64 = 3.531_6e12;

/// The planet's air.
///
/// Sea-level density is Earth's, so a drag coefficient means what it means in a
/// wind tunnel. The scale height is not Earth's: at 8.5 km over a 600 km planet
/// the atmosphere would be a skin, and the ascent would have no air to fight.
pub const AIR: Atmosphere = Atmosphere {
    sea_level_density: 1.225,
    scale_height: 5_600.0,
    ceiling: 70_000.0,
};

/// The moon's distance from the planet, its gravitational parameter and its
/// radius, in metres and m³/s².
pub const MOON_ORBIT: f64 = 12_000_000.0;
/// See [`MOON_ORBIT`].
pub const MOON_MU: f64 = 6.513_8e10;
/// See [`MOON_ORBIT`].
pub const MOON_RADIUS: f64 = 200_000.0;

// ---- the rocket --------------------------------------------------------------

/// The rocket with no fuel in it, in kilograms.
pub const DRY_MASS: f64 = 2_000.0;

/// A full tank, in kilograms.
pub const FUEL_MASS: f64 = 8_000.0;

/// Thrust at full throttle, in newtons.
///
/// Against a full ship at this planet's surface gravity that is a
/// thrust-to-weight ratio of about 2.4 — enough to leave the pad briskly
/// without the drag losses of a ratio nearer 4.
pub const MAX_THRUST: f64 = 240_000.0;

/// Specific impulse in seconds, the engine's efficiency: how many seconds a
/// kilogram of propellant produces a kilogram-force of thrust for.
pub const SPECIFIC_IMPULSE: f64 = 300.0;

/// Standard gravity, in m/s². Not this planet's surface gravity: it is the
/// defined constant `9.80665` that turns a specific impulse in seconds into an
/// exhaust velocity, and it is a unit conversion wherever it appears.
pub const STANDARD_GRAVITY: f64 = 9.806_65;

/// The rocket's drag coefficient and the area it presents, dimensionless and
/// in m².
pub const DRAG_COEFFICIENT: f64 = 0.3;
/// See [`DRAG_COEFFICIENT`].
pub const REFERENCE_AREA: f64 = 4.0;

/// How fast the throttle moves under a held key, in throttle per second.
pub const THROTTLE_RATE: f64 = 1.0;

/// How fast the ship turns under a held key, in radians per second.
pub const PITCH_RATE: f64 = 0.6;

/// The fastest a landing can be and still be a landing, in m/s.
pub const LANDING_SPEED: f64 = 12.0;

// ---- timewarp ----------------------------------------------------------------

/// The timewarp rates, in the order the controls step through them.
///
/// `06-orbit.md`'s scope: "×1–×1000 (on-rails only; auto-drop on
/// burn/atmosphere)".
pub const WARP_RATES: [u32; 4] = [1, 10, 100, 1000];

/// How often the flight logs its `[HUD]` line, in ticks.
///
/// A second of simulated time at [`DEFAULT_TICK_HZ`], and the same cadence
/// every other sample uses — `web/tools/browser-e2e.mjs` watches for that
/// heartbeat to tell a paused demo from a running one.
pub const HEARTBEAT_TICKS: u64 = 60;

/// How many points the orbit path is drawn from.
pub const PATH_SAMPLES: usize = 96;

// ---- the autopilot -----------------------------------------------------------

/// The tick the autopilot releases the clamp on.
///
/// A second on the pad before it goes, so a page that has just loaded shows the
/// rocket standing on the planet rather than already gone.
pub const AUTOPILOT_LAUNCH_TICK: u64 = 60;

/// The altitude the autopilot flies straight up to before it starts to lean
/// over, and the one it is fully horizontal at, in metres.
///
/// A gravity turn: the ship pitches over gradually and lets its own momentum
/// carry the turn, which is what a real ascent does and why the profile is two
/// altitudes rather than a schedule of angles.
pub const TURN_START: f64 = 3_000.0;
/// See [`TURN_START`].
pub const TURN_END: f64 = 45_000.0;

/// The apoapsis the autopilot aims for before it cuts the engine, in metres of
/// altitude.
pub const TARGET_APOAPSIS: f64 = 100_000.0;

/// How slowly the ship must be climbing for the autopilot to call it apoapsis,
/// in m/s.
///
/// A stand-in for a time-to-apoapsis the flight does not compute: the climb
/// rate falls through zero at the top of the arc. Compared **signed** rather
/// than as a magnitude, so a ship that has fallen past its apoapsis with the
/// burn unfinished keeps burning instead of watching itself come down.
pub const COAST_CLIMB_RATE: f64 = 20.0;

// ---- what the player asked for -----------------------------------------------

/// One tick's worth of controls.
///
/// Every field is a flag rather than an axis, because that is what a keyboard
/// produces; the rates they move things at are [`THROTTLE_RATE`] and
/// [`PITCH_RATE`], applied per tick on the server.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
struct Intent {
    throttle_up: bool,
    throttle_down: bool,
    pitch_left: bool,
    pitch_right: bool,
    warp_up: bool,
    warp_down: bool,
    /// Release the launch clamp, or start again after a landing or a crash.
    launch: bool,
}

const INTENT_THROTTLE_UP: u8 = 1 << 0;
const INTENT_THROTTLE_DOWN: u8 = 1 << 1;
const INTENT_PITCH_LEFT: u8 = 1 << 2;
const INTENT_PITCH_RIGHT: u8 = 1 << 3;
const INTENT_WARP_UP: u8 = 1 << 4;
const INTENT_WARP_DOWN: u8 = 1 << 5;
const INTENT_LAUNCH: u8 = 1 << 6;

/// Every bit the flag byte defines. One set outside this mask is a frame
/// something other than [`Intent::to_wire`] wrote.
const INTENT_FLAGS: u8 = INTENT_THROTTLE_UP
    | INTENT_THROTTLE_DOWN
    | INTENT_PITCH_LEFT
    | INTENT_PITCH_RIGHT
    | INTENT_WARP_UP
    | INTENT_WARP_DOWN
    | INTENT_LAUNCH;

impl Intent {
    /// Whether the player asked for anything at all.
    ///
    /// What hands the flight over from the autopilot: the first frame that says
    /// anything is the last one the script flies.
    const fn is_anything(self) -> bool {
        self.throttle_up
            || self.throttle_down
            || self.pitch_left
            || self.pitch_right
            || self.warp_up
            || self.warp_down
            || self.launch
    }

    /// The wire form handed to `Client::set_input`: one byte of flags.
    fn to_wire(self) -> u8 {
        let mut flags = 0;
        if self.throttle_up {
            flags |= INTENT_THROTTLE_UP;
        }
        if self.throttle_down {
            flags |= INTENT_THROTTLE_DOWN;
        }
        if self.pitch_left {
            flags |= INTENT_PITCH_LEFT;
        }
        if self.pitch_right {
            flags |= INTENT_PITCH_RIGHT;
        }
        if self.warp_up {
            flags |= INTENT_WARP_UP;
        }
        if self.warp_down {
            flags |= INTENT_WARP_DOWN;
        }
        if self.launch {
            flags |= INTENT_LAUNCH;
        }
        flags
    }

    /// The intent a client sealed, read back on the server's side of the wire.
    ///
    /// `None` for anything this build did not write: a payload that is not one
    /// byte, or a flag outside [`INTENT_FLAGS`]. **Validated rather than
    /// trusted**, because these are the only bytes in this game a peer chooses.
    fn from_wire(bytes: &[u8]) -> Option<Self> {
        let &[flags] = bytes else {
            return None;
        };
        if flags & !INTENT_FLAGS != 0 {
            return None;
        }
        Some(Self {
            throttle_up: flags & INTENT_THROTTLE_UP != 0,
            throttle_down: flags & INTENT_THROTTLE_DOWN != 0,
            pitch_left: flags & INTENT_PITCH_LEFT != 0,
            pitch_right: flags & INTENT_PITCH_RIGHT != 0,
            warp_up: flags & INTENT_WARP_UP != 0,
            warp_down: flags & INTENT_WARP_DOWN != 0,
            launch: flags & INTENT_LAUNCH != 0,
        })
    }

    /// Everything that arrived for this tick, folded into one.
    ///
    /// Normally one frame per tick and this is a decode. Several is a client
    /// whose clock ran ahead of the server's, and every field is OR-ed: each is
    /// a thing the player asked for, and a later frame that says nothing is not
    /// a retraction. A held key that produced two frames in one tick moves the
    /// throttle by one tick's worth either way, because the rate is applied
    /// once per tick and not once per frame.
    fn from_inputs(inputs: ClientInputs<'_>) -> Self {
        let mut merged = Self::default();
        for (_tick, data) in inputs.iter() {
            // A frame this build cannot read is skipped rather than taken as an
            // empty intent, which would read as the player letting go.
            let Some(frame) = Self::from_wire(data) else {
                continue;
            };
            merged.throttle_up |= frame.throttle_up;
            merged.throttle_down |= frame.throttle_down;
            merged.pitch_left |= frame.pitch_left;
            merged.pitch_right |= frame.pitch_right;
            merged.warp_up |= frame.warp_up;
            merged.warp_down |= frame.warp_down;
            merged.launch |= frame.launch;
        }
        merged
    }
}

// ---- the flight --------------------------------------------------------------

/// Where the mission is.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum Phase {
    /// On the pad, clamped, engine ignitable but going nowhere.
    #[default]
    Prelaunch,
    /// Off the pad and under its own control.
    Flying,
    /// Down in one piece, slower than [`LANDING_SPEED`].
    Landed,
    /// Down harder than that.
    Crashed,
}

impl Phase {
    /// Whether the flight is over and a launch would start a new one.
    #[must_use]
    pub const fn is_finished(self) -> bool {
        matches!(self, Self::Landed | Self::Crashed)
    }

    /// The word the HUD prints.
    #[must_use]
    pub const fn label(self) -> &'static str {
        match self {
            Self::Prelaunch => "PRELAUNCH",
            Self::Flying => "FLYING",
            Self::Landed => "LANDED",
            Self::Crashed => "CRASHED",
        }
    }
}

/// Everything this sample simulates.
///
/// Lives behind an `Arc<Mutex<_>>` shared with [`OrbitModule`], for the reason
/// every sample's does: [`crcbl::server::Server::set_module`] takes ownership of
/// the module and never hands it back, so the only way to read what a tick
/// produced is a cell both sides hold.
struct Flight {
    /// The planet, the moon, and where the moon is. Rebuilt on a restart only
    /// because the moon moves; the shape never changes.
    frames: Frames,
    planet: FrameId,
    moon: FrameId,
    /// Which frame the ship is being simulated in.
    frame: FrameId,
    /// Where the ship is and how fast, in that frame.
    ship: State,
    /// Which way the engine points. A unit vector, in the same frame.
    attitude: DVec3,
    /// Fuel remaining, in kilograms.
    fuel: f64,
    /// Throttle, in `0..=1`.
    throttle: f64,
    /// Index into [`WARP_RATES`].
    warp: usize,
    phase: Phase,
    ticks: u64,
    /// Seconds of simulated time the flight has run, which timewarp advances
    /// faster than the tick clock.
    elapsed: f64,
    /// Whether the script is still flying. Cleared for good by the first input
    /// the player sends.
    autopilot: bool,
    /// How many sphere-of-influence crossings this flight has made. Simulation
    /// state rather than a statistic: it is what a replay comparison would
    /// diverge on first if a crossing were missed.
    crossings: u64,
}

impl Flight {
    fn new() -> Self {
        let mut frames = Frames::new(PLANET_MU);
        let planet = frames.root();
        let moon = frames.add(
            planet,
            MOON_MU,
            sphere_of_influence(MOON_ORBIT, MOON_MU, PLANET_MU),
            WorldPos::from_offset(DVec3::new(MOON_ORBIT, 0.0, 0.0)),
            // Circular, so the moon is still there next orbit.
            DVec3::new(0.0, (PLANET_MU / MOON_ORBIT).sqrt(), 0.0),
        );
        let mut flight = Self {
            frames,
            planet,
            moon,
            frame: planet,
            ship: State::AT_ORIGIN,
            attitude: DVec3::Y,
            fuel: FUEL_MASS,
            throttle: 0.0,
            warp: 0,
            phase: Phase::Prelaunch,
            ticks: 0,
            elapsed: 0.0,
            autopilot: true,
            crossings: 0,
        };
        flight.park_on_the_pad();
        flight
    }

    /// Puts the ship back on the pad with a full tank, which is also what a
    /// restart does.
    fn park_on_the_pad(&mut self) {
        self.frame = self.planet;
        self.ship = State::new(
            WorldPos::from_offset(DVec3::new(0.0, PLANET_RADIUS, 0.0)),
            DVec3::ZERO,
        );
        self.attitude = DVec3::Y;
        self.fuel = FUEL_MASS;
        self.throttle = 0.0;
        self.warp = 0;
        self.phase = Phase::Prelaunch;
    }

    /// The ship's mass right now, in kilograms.
    fn mass(&self) -> f64 {
        DRY_MASS + self.fuel
    }

    /// The ship's offset from the origin of the frame it is in, in metres.
    fn offset(&self) -> DVec3 {
        self.ship.position.delta(WorldPos::ORIGIN)
    }

    /// The radius of the body whose frame the ship is in.
    fn body_radius(&self) -> f64 {
        if self.frame == self.moon {
            MOON_RADIUS
        } else {
            PLANET_RADIUS
        }
    }

    /// Height above that body's surface, in metres.
    fn altitude(&self) -> f64 {
        self.offset().length() - self.body_radius()
    }

    /// The orbit the ship is on, about whatever it is orbiting.
    fn orbit(&self) -> Orbit {
        Orbit::from_state(self.frames.mu(self.frame), self.ship)
    }

    /// The thrust the engine is producing, in newtons. Zero with the tank dry,
    /// whatever the throttle says.
    fn thrust(&self) -> f64 {
        if self.fuel > 0.0 {
            MAX_THRUST * self.throttle
        } else {
            0.0
        }
    }

    /// Whether the ship may be propagated rather than integrated.
    ///
    /// Gravity has to be the only force: the engine off, and the ship out of
    /// the air. Also only in flight — a ship on the pad is held by a clamp,
    /// which is not a conic either.
    fn may_warp(&self) -> bool {
        self.phase == Phase::Flying
            && self.thrust() == 0.0
            && (self.frame == self.moon || self.altitude() >= AIR.ceiling)
    }

    /// The air this ship is flying through, or `None` in vacuum.
    ///
    /// The moon has none, which is `06-orbit.md`'s scope: "one moon (vacuum)".
    fn air(&self) -> Option<AtmosphericDrag> {
        (self.frame == self.planet).then_some(AtmosphericDrag {
            atmosphere: AIR,
            centre: DVec3::ZERO,
            radius: PLANET_RADIUS,
            drag_coefficient: DRAG_COEFFICIENT,
            reference_area: REFERENCE_AREA,
        })
    }
}

/// A poisoned mutex here means a previous tick panicked. The flight is plain
/// numbers with no invariant a panic could have half-broken, so recovering the
/// guard is strictly better than taking the process down a second time.
fn lock(shared: &Mutex<Flight>) -> MutexGuard<'_, Flight> {
    shared
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner())
}

// ---- the tick ----------------------------------------------------------------

/// One tick of the flight.
///
/// The order is the order a flight reads in: the controls are taken, the warp
/// is checked against what the ship is actually doing, the ship moves — on
/// rails or under its forces — and then where it ended up is looked at, for a
/// sphere-of-influence crossing and for the ground.
fn run_tick(flight: &mut Flight, intent: Intent, dt: f64) {
    flight.ticks += 1;

    if intent.is_anything() {
        flight.autopilot = false;
    }
    let intent = if flight.autopilot {
        autopilot(flight)
    } else {
        intent
    };

    apply_controls(flight, intent, dt);

    // **The auto-drop.** Checked after the controls, so a throttle nudged up
    // this tick drops the warp on the same tick it starts to burn rather than
    // one tick into a burn nothing integrated.
    if !flight.may_warp() {
        flight.warp = 0;
    }
    let warp = f64::from(WARP_RATES[flight.warp]);

    if flight.phase == Phase::Prelaunch {
        // Held by the clamp. The throttle can be set here and the tank does not
        // drain for it: the engine ignites on release, so a launch at full
        // throttle leaves the pad with a full tank rather than one already a
        // second down.
        flight.elapsed += dt;
        return;
    }

    if flight.phase.is_finished() {
        flight.elapsed += dt;
        return;
    }

    let step = dt * warp;
    flight.elapsed += step;
    if flight.warp > 0 {
        flight.ship = propagate(flight.frames.mu(flight.frame), flight.ship, step);
    } else {
        integrate(flight, step);
    }

    cross_boundaries(flight);
    touch_down(flight);
}

/// Takes one tick of controls: throttle, attitude, warp and the clamp.
fn apply_controls(flight: &mut Flight, intent: Intent, dt: f64) {
    if intent.throttle_up {
        flight.throttle = (flight.throttle + THROTTLE_RATE * dt).min(1.0);
    }
    if intent.throttle_down {
        flight.throttle = (flight.throttle - THROTTLE_RATE * dt).max(0.0);
    }

    // Turning is about the orbital plane's normal, so the ship stays in the
    // plane it launched into — `06-orbit.md` has one plane and a map view drawn
    // in it, and a yaw axis would put the ship somewhere the map cannot show.
    let turn = f64::from(i32::from(intent.pitch_left) - i32::from(intent.pitch_right));
    if turn != 0.0 {
        let rotation = DQuat::from_rotation_z(turn * PITCH_RATE * dt);
        flight.attitude = (rotation * flight.attitude).normalize();
    }

    if intent.warp_up && flight.warp + 1 < WARP_RATES.len() {
        flight.warp += 1;
    }
    if intent.warp_down && flight.warp > 0 {
        flight.warp -= 1;
    }

    if intent.launch {
        match flight.phase {
            Phase::Prelaunch => flight.phase = Phase::Flying,
            Phase::Landed | Phase::Crashed => flight.park_on_the_pad(),
            Phase::Flying => {}
        }
    }
}

/// Live integration: gravity, air and thrust, on the substep clock.
fn integrate(flight: &mut Flight, dt: f64) {
    let gravity = PointGravity::new(flight.frames.mu(flight.frame), DVec3::ZERO);
    let air = flight.air();
    let integrator = SemiImplicitEuler;
    let h = dt / f64::from(SUBSTEPS);

    let mut transform = Transform {
        position: flight.offset(),
        rotation: DQuat::IDENTITY,
    };
    let mut velocity = flight.ship.velocity;

    for _ in 0..SUBSTEPS {
        // Rebuilt each substep because the mass falls as the tank empties, and
        // a rocket that kept its launch mass would arrive with the wrong
        // velocity by the end of the burn.
        let mut body = RigidBody::new_dynamic(flight.mass());
        body.velocity = velocity;

        body.apply_force(gravity.acceleration_at(transform.position) * body.mass);
        if let Some(air) = air {
            body.apply_force(air.world_force(transform.position, body.velocity));
        }
        let thrust = flight.thrust();
        if thrust > 0.0 {
            body.apply_force(flight.attitude * thrust);
            // `F = m_dot · Isp · g0` inverted: what the engine weighs in
            // propellant per second at this throttle.
            flight.fuel =
                (flight.fuel - thrust / (SPECIFIC_IMPULSE * STANDARD_GRAVITY) * h).max(0.0);
        }

        integrator.step(&mut body, &mut transform, h);
        velocity = body.velocity;
    }

    flight.ship = State::new(WorldPos::from_offset(transform.position), velocity);
}

/// Hands the ship to whichever body now dominates it, if that changed.
///
/// One step per tick and no loop: a tick that crossed two boundaries would be
/// a tick long enough to cross a whole sphere of influence, which at ×1000 over
/// this moon is still minutes away.
fn cross_boundaries(flight: &mut Flight) {
    if let Some(next) = flight.frames.transition(flight.ship, flight.frame) {
        flight.ship = flight.frames.convert(flight.ship, flight.frame, next);
        flight.frame = next;
        flight.crossings += 1;
        crcbl::log::info!(
            "[ORBIT] frame change to {} at {:.0} m",
            if next == flight.moon {
                "moon"
            } else {
                "planet"
            },
            flight.altitude(),
        );
    }
}

/// Stops the ship at the surface, one way or the other.
fn touch_down(flight: &mut Flight) {
    if flight.altitude() > 0.0 {
        return;
    }
    let speed = flight.ship.velocity.length();
    let up = flight.offset().normalize_or(DVec3::Y);
    flight.ship = State::new(
        WorldPos::from_offset(up * flight.body_radius()),
        DVec3::ZERO,
    );
    flight.throttle = 0.0;
    flight.warp = 0;
    flight.phase = if speed <= LANDING_SPEED {
        Phase::Landed
    } else {
        Phase::Crashed
    };
    crcbl::log::info!(
        "[ORBIT] {} at {speed:.1} m/s",
        flight.phase.label().to_lowercase(),
    );
}

// ---- the autopilot -----------------------------------------------------------

/// What the script asks for this tick.
///
/// A gravity turn to [`TARGET_APOAPSIS`], then a circularisation burn at
/// apoapsis, then nothing. It exists because a page that has just loaded takes
/// no input and a rocket standing on a pad is indistinguishable from a stopped
/// loop — the same reason `apps/viewer` turns its model. The first thing the
/// player asks for ends it for good.
fn autopilot(flight: &Flight) -> Intent {
    let mut intent = Intent::default();
    if flight.phase == Phase::Prelaunch {
        // Spool the engine up on the pad, then release.
        intent.throttle_up = true;
        intent.launch = flight.ticks >= AUTOPILOT_LAUNCH_TICK;
        return intent;
    }
    if flight.phase.is_finished() || flight.frame != flight.planet {
        return intent;
    }

    let orbit = flight.orbit();
    let apoapsis = orbit
        .apoapsis()
        .map_or(f64::INFINITY, |apoapsis| apoapsis - PLANET_RADIUS);
    let periapsis = orbit.periapsis() - PLANET_RADIUS;
    let altitude = flight.altitude();

    let up = flight.offset().normalize_or(DVec3::Y);
    let climb_rate = flight.ship.velocity.dot(up);

    // Ascent: full throttle until the apoapsis is where it should be.
    let climbing = apoapsis < TARGET_APOAPSIS;
    // Circularisation: raise the periapsis out of the air, and **at the top**,
    // where a prograde burn adds to the speed without adding to the height.
    // Burning on the way up would spend most of itself raising an apoapsis that
    // is already where it was asked to be. Near-zero climb rate is what "at the
    // top" means without a time-to-apoapsis to compute.
    let circularising = !climbing
        && periapsis < AIR.ceiling
        && altitude > AIR.ceiling
        && climb_rate < COAST_CLIMB_RATE;
    intent.throttle_up = climbing || circularising;
    intent.throttle_down = !intent.throttle_up;

    // With the orbit made, warp along it — the page's only chance to show the
    // on-rails path to a visitor who has not touched a key, and the auto-drop
    // with it: the request is refused for as long as the throttle is still
    // closing.
    intent.warp_up = !climbing && !circularising && periapsis >= AIR.ceiling;

    // The heading it wants: a pitch program on the way up, prograde once the
    // program has run out.
    //
    // **Scheduled against altitude, not steered off the velocity.** An earlier
    // version blended toward prograde as it leaned, on the reasoning that a
    // gravity turn holds prograde — which is true, and useless as a way to
    // *start* one: prograde low down is still straight up, so the blend's
    // target was the heading it already had and the ascent locked vertical
    // after a single tick of turn. A launch vehicle's early pitch program is
    // open-loop for the same reason, and closes the loop only once there is a
    // velocity vector worth following.
    let wanted = if circularising || altitude >= TURN_END {
        flight.ship.velocity.normalize_or(up)
    } else {
        let lean = ((altitude - TURN_START) / (TURN_END - TURN_START)).clamp(0.0, 1.0);
        let angle = lean * std::f64::consts::FRAC_PI_2;
        // `up` turned a quarter turn, in the same sense the moon goes round, so
        // a transfer later is not retrograde.
        let downrange = DVec3::new(-up.y, up.x, 0.0);
        (up * angle.cos() + downrange * angle.sin()).normalize_or(up)
    };

    // Steer toward it, one tick's worth. The cross product's `z` is the signed
    // angle's sense in the orbital plane.
    let sense = flight.attitude.cross(wanted).z;
    intent.pitch_left = sense > 0.0;
    intent.pitch_right = sense < 0.0;
    intent
}

// ---- the module --------------------------------------------------------------

/// The flight, as the server hosts it.
///
/// `register` is empty for the same reason hud's and flappy's are: the whole
/// simulation is the [`Flight`] behind the shared cell, and there is no ECS
/// system to register.
struct OrbitModule {
    shared: Arc<Mutex<Flight>>,
}

impl std::fmt::Debug for OrbitModule {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("OrbitModule").finish_non_exhaustive()
    }
}

impl GameModule for OrbitModule {
    fn name(&self) -> &str {
        "orbit"
    }

    fn register(&self, _world: &mut World) {}

    fn tick(&mut self, world: &mut World, inputs: ClientInputs<'_>) {
        let dt = world.tick_dt();
        run_tick(&mut lock(&self.shared), Intent::from_inputs(inputs), dt);
    }
}

// ---- what a frame draws ------------------------------------------------------

/// Everything [`crate::page`] draws, snapshotted once a frame.
///
/// A plain struct rather than a borrow of the flight: the page runs on the
/// frame's thread and the flight is behind a mutex the server's tick holds, and
/// a page that read through the lock would be holding it for the length of a
/// draw.
///
/// Every distance here is in metres and every speed in m/s, in the frame of
/// whatever the ship is orbiting — the HUD says which.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct RenderState {
    /// Ticks the simulation has run.
    pub tick: u64,
    /// Seconds of simulated time, which runs ahead of the tick clock under
    /// timewarp.
    pub elapsed: f64,
    pub phase: Phase,
    /// The name of the body the ship is orbiting.
    pub body: &'static str,
    /// That body's radius, so the map knows how big to draw it.
    pub body_radius: f64,
    /// Height above its surface.
    pub altitude: f64,
    /// Speed relative to that body's frame.
    pub speed: f64,
    /// How fast the ship is climbing, positive outward.
    pub vertical_speed: f64,
    /// Apoapsis and periapsis as altitudes above the surface. Apoapsis is
    /// `None` on an escape trajectory, which has none.
    pub apoapsis: Option<f64>,
    /// See [`apoapsis`](Self::apoapsis).
    pub periapsis: f64,
    /// The orbital period in seconds, or `None` for an orbit that does not come
    /// back.
    pub period: Option<f64>,
    /// Fuel remaining, in `0..=1`.
    pub fuel: f64,
    /// Throttle, in `0..=1`.
    pub throttle: f64,
    /// The current timewarp rate.
    pub warp: u32,
    /// Whether the warp controls would do anything right now.
    pub warp_allowed: bool,
    /// Whether the script is still flying.
    pub autopilot: bool,
    /// Where the ship is, in its frame's plane.
    pub ship: [f64; 2],
    /// Which way the engine points, in the same plane.
    pub attitude: [f64; 2],
    /// Whether [`path`](Self::path) comes back round to its own first point,
    /// so the map can stroke the closing arc as well. False on a trajectory
    /// that escapes, whose samples stop where the map stops.
    pub path_closed: bool,
    /// The trajectory ahead, in the same plane. Refilled rather than
    /// reallocated.
    ///
    /// **Sampled from the propagator, not drawn from elements.** An ellipse
    /// needs an orientation to draw and [`Orbit`] deliberately does not carry
    /// one — the angles it would come from are undefined for the circular orbit
    /// this demo aims at. Asking [`propagate`] where the ship will be at a
    /// spread of times needs no angle and is right for an escape trajectory
    /// too.
    pub path: Vec<[f64; 2]>,
}

// ---- the debug panel's section -----------------------------------------------

/// The flight's numbers, for the debug overlay.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct FlightStats {
    /// Ticks the simulation has run.
    pub ticks: u64,
    /// Seconds of simulated time.
    pub elapsed: f64,
    pub phase: Phase,
    /// Height above the surface, in metres.
    pub altitude: f64,
    /// Speed in the current frame, in m/s.
    pub speed: f64,
    /// Fuel remaining, in kilograms.
    pub fuel: f64,
    /// The current timewarp rate.
    pub warp: u32,
    /// How many sphere-of-influence crossings this flight has made.
    pub crossings: u64,
    /// Whether the script is still flying.
    pub autopilot: bool,
}

impl crcbl::ui::DebugModule for FlightStats {
    fn debug_section(&self, section: &mut crcbl::ui::DebugSection) {
        section.set_title("orbit");
        section.row("tick", format_args!("{}", self.ticks));
        section.row("t", format_args!("{:.1} s", self.elapsed));
        section.row("phase", format_args!("{}", self.phase.label()));
        section.row("alt", format_args!("{:.0} m", self.altitude));
        section.row("vel", format_args!("{:.1} m/s", self.speed));
        section.row("fuel", format_args!("{:.0} kg", self.fuel));
        section.row("warp", format_args!("x{}", self.warp));
        section.row("soi", format_args!("{}", self.crossings));
        section.row(
            "pilot",
            format_args!("{}", if self.autopilot { "auto" } else { "player" }),
        );
    }
}

// ---- the facade --------------------------------------------------------------

/// What can stop orbit before it starts.
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

/// The flight, its server, its client, and the clock that drives all three.
pub struct Game {
    session: Loopback,
    shared: Arc<Mutex<Flight>>,
    /// Exactly one tick period per [`Game::tick`], so the server's accumulator
    /// yields exactly one tick per call.
    tick_period: Duration,
    sim_time: Duration,
    ticks_run: u64,
    /// What the player is holding down, sent on the next tick.
    pending: Intent,
    /// The phase the last heartbeat reported, so a landing gets a line of its
    /// own rather than waiting for the next second.
    logged_phase: Option<Phase>,
}

impl std::fmt::Debug for Game {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Game")
            .field("ticks_run", &self.ticks_run)
            .finish_non_exhaustive()
    }
}

impl Game {
    /// Builds the server, its client and the flight between them.
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
        let shared = Arc::new(Mutex::new(Flight::new()));

        // An empty world, and that is the honest shape: this sample has no
        // entity and no ECS system. What the server hosts is the module, and
        // what the module owns is the flight.
        let session = Loopback::new(
            World::new(),
            Box::new(OrbitModule {
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
            logged_phase: None,
        };

        // **One tick spent on the handshake, before the flight starts.**
        // `Server::update` drains the transport inside `tick`, so the client's
        // hello is not read until a tick runs, and until the session is up the
        // client drops every input frame it is asked to send. Spending it here
        // is what makes the player's first key the first the simulation sees.
        // It costs nothing: the ship is clamped to the pad for the first
        // second either way.
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
            "sim: {tick_hz} Hz, {:.3} ms per tick, {SUBSTEPS} substeps",
            tick_period.as_secs_f64() * 1e3,
        );
        Ok(game)
    }

    /// Records what the player is asking for, to be sent on the next tick.
    pub fn set_controls(&mut self, controls: Controls) {
        self.pending = Intent {
            throttle_up: controls.throttle_up,
            throttle_down: controls.throttle_down,
            pitch_left: controls.pitch_left,
            pitch_right: controls.pitch_right,
            warp_up: controls.warp_up,
            warp_down: controls.warp_down,
            launch: controls.launch,
        };
    }

    /// Advances the server, and with it the flight, by exactly one tick.
    pub fn tick(&mut self) {
        self.sim_time += self.tick_period;
        let (server, client) = self.session.both_mut();

        // The bytes are the whole input path: the client seals them, the
        // transport carries them and the module decodes them, exactly as a
        // remote client's would be.
        client.set_input(vec![self.pending.to_wire()]);
        // Edges are consumed by the tick that sends them; held flags are
        // re-set by the next frame's `set_controls`.
        self.pending = Intent::default();

        // Send, simulate, then receive — and the send has to come first.
        // `Client::update` is the only thing that puts input on the wire and
        // the server drains the wire at the top of its tick, so a client
        // updated only after the server posts this tick's controls to the next
        // one. Left that way, nothing the player pressed reached the tick they
        // pressed it on, and the handover away from the autopilot never
        // happened at all.
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

    /// The `[HUD]` line, on the same cadence and in the same shape every other
    /// sample uses, plus the tick a phase changes on so the change is not
    /// swallowed by the gap.
    ///
    /// `web/tools/browser-e2e.mjs` reads two claims out of it. One is the
    /// heartbeat itself — it exists while the demo runs and stops while it is
    /// paused. The other is that the simulation is *moving*, and here that is
    /// `alt`: the ship is flown by a script from the moment the page loads, so
    /// its altitude climbs without anyone touching a key, and a flight that had
    /// stalled would leave the number standing still.
    fn log_heartbeat(&mut self) {
        let flight = lock(&self.shared);
        let changed = self.logged_phase != Some(flight.phase);
        if !changed && !flight.ticks.is_multiple_of(HEARTBEAT_TICKS) {
            return;
        }
        let orbit = flight.orbit();
        let radius = flight.body_radius();
        crcbl::log::info!(
            "[HUD] tick: {}  phase: {}  alt: {:.0}  vel: {:.0}  apo: {:.0}  peri: {:.0}  \
             fuel: {:.0}  warp: x{}",
            flight.ticks,
            flight.phase.label(),
            flight.altitude(),
            flight.ship.velocity.length(),
            orbit.apoapsis().unwrap_or(f64::INFINITY) - radius,
            orbit.periapsis() - radius,
            flight.fuel,
            WARP_RATES[flight.warp],
        );
        self.logged_phase = Some(flight.phase);
    }

    /// Refills `out` with what the page should draw this frame.
    pub fn render_state(&self, out: &mut RenderState) {
        let flight = lock(&self.shared);
        let orbit = flight.orbit();
        let radius = flight.body_radius();
        let offset = flight.offset();
        let up = offset.normalize_or(DVec3::Y);

        out.tick = flight.ticks;
        out.elapsed = flight.elapsed;
        out.phase = flight.phase;
        out.body = if flight.frame == flight.moon {
            "MOON"
        } else {
            "PLANET"
        };
        out.body_radius = radius;
        out.altitude = flight.altitude();
        out.speed = flight.ship.velocity.length();
        out.vertical_speed = flight.ship.velocity.dot(up);
        out.apoapsis = orbit.apoapsis().map(|apoapsis| apoapsis - radius);
        out.periapsis = orbit.periapsis() - radius;
        out.period = orbit.period(flight.frames.mu(flight.frame));
        out.fuel = flight.fuel / FUEL_MASS;
        out.throttle = flight.throttle;
        out.warp = WARP_RATES[flight.warp];
        out.warp_allowed = flight.may_warp();
        out.autopilot = flight.autopilot;
        out.ship = [offset.x, offset.y];
        out.attitude = [flight.attitude.x, flight.attitude.y];

        out.path.clear();
        out.path_closed = false;
        // **Only where there is a trajectory to draw.** A ship going straight
        // up from a standstill has exactly zero angular momentum — this planet
        // does not rotate, so the launch is radial — and the conic through that
        // state passes through the centre of attraction, where [`propagate`]
        // refuses to work and is right to. `semi_latus_rectum` is that angular
        // momentum: above zero the periapsis is above zero too, and no sample
        // can reach the focus. So the path appears as the rocket leans over,
        // which is also when it first means anything.
        if orbit.semi_latus_rectum > 0.0 && flight.phase == Phase::Flying {
            // One period for a closed orbit; for one that does not come back,
            // an hour of it, which is as far ahead as a map at this scale can
            // show.
            let span = out.period.unwrap_or(3_600.0);
            // The samples span one whole period without repeating the first
            // point, so the arc from the last back to it is the map's to draw.
            out.path_closed = out.period.is_some();
            let mu = flight.frames.mu(flight.frame);
            for sample in 0..PATH_SAMPLES {
                let ahead = span * sample as f64 / PATH_SAMPLES as f64;
                let at = propagate(mu, flight.ship, ahead);
                let point = at.position.delta(WorldPos::ORIGIN);
                // A non-finite sample is pushed rather than dropped.
                // `DrawList::polyline` breaks its run at one instead of
                // joining across it, so a propagator that diverged shows as
                // the gap it is; dropping the sample here would hand the map a
                // shorter but unbroken curve and hide it.
                out.path.push([point.x, point.y]);
            }
        }
    }

    /// The flight's numbers for the debug panel.
    #[must_use]
    pub fn stats(&self) -> FlightStats {
        let flight = lock(&self.shared);
        FlightStats {
            ticks: flight.ticks,
            elapsed: flight.elapsed,
            phase: flight.phase,
            altitude: flight.altitude(),
            speed: flight.ship.velocity.length(),
            fuel: flight.fuel,
            warp: WARP_RATES[flight.warp],
            crossings: flight.crossings,
            autopilot: flight.autopilot,
        }
    }

    /// The whole flight folded to a 64-bit hash, over its **logical** values.
    ///
    /// Field by field in a fixed order through [`hash_u64`] — never over the
    /// bytes the struct happens to occupy, which would fold in padding and the
    /// `Frames` heap pointers. The floats go in through `to_bits`, so two runs
    /// that produced the same numbers hash the same and two that produced
    /// numbers a bit apart do not: this is a *where did they diverge*
    /// instrument, and the replay test compares the values themselves.
    #[must_use]
    pub fn state_hash(&self) -> u64 {
        let flight = lock(&self.shared);
        let mut h: u64 = 0x4F52_4249_5420_4658;
        let mut fold = |value: u64| h = hash_u64(h, value);
        fold(flight.ticks);
        fold(flight.crossings);
        fold(flight.phase as u64);
        fold(flight.warp as u64);
        fold(flight.elapsed.to_bits());
        fold(flight.fuel.to_bits());
        fold(flight.throttle.to_bits());
        for axis in [flight.attitude.x, flight.attitude.y, flight.attitude.z] {
            fold(axis.to_bits());
        }
        let offset = flight.offset();
        for axis in [offset.x, offset.y, offset.z] {
            fold(axis.to_bits());
        }
        for axis in [
            flight.ship.velocity.x,
            flight.ship.velocity.y,
            flight.ship.velocity.z,
        ] {
            fold(axis.to_bits());
        }
        h
    }
}

/// What the player is holding down, as the front end reads it off the keyboard.
///
/// The same seven flags the private `Intent` carries, in a public shape:
/// `app.rs` fills
/// this in from its action map and hands it to [`Game::set_controls`], and the
/// wire form stays private because nothing outside this module writes bytes.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Controls {
    /// Open the throttle.
    pub throttle_up: bool,
    /// Close it.
    pub throttle_down: bool,
    /// Turn anticlockwise in the orbital plane.
    pub pitch_left: bool,
    /// Turn clockwise.
    pub pitch_right: bool,
    /// One step up the timewarp ladder. An edge, not a held key.
    pub warp_up: bool,
    /// One step down. Also an edge.
    pub warp_down: bool,
    /// Release the clamp, or start again after a landing or a crash.
    pub launch: bool,
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    /// Long enough for the pad hold, the ascent, the coast to apoapsis and the
    /// circularisation burn, at [`DEFAULT_TICK_HZ`].
    ///
    /// Five minutes of simulated time. The ascent is about a hundred seconds of
    /// burn and the coast to apoapsis a little over another hundred, so this
    /// leaves the autopilot room to be slower than expected without leaving it
    /// room to have failed.
    const ASCENT_TICKS: u64 = 300 * DEFAULT_TICK_HZ as u64;

    fn flown(ticks: u64) -> Game {
        let mut game = Game::new(DEFAULT_TICK_HZ).expect("the loopback session comes up");
        for _ in 0..ticks {
            game.tick();
        }
        game
    }

    fn seen(game: &Game) -> RenderState {
        let mut state = RenderState::default();
        game.render_state(&mut state);
        state
    }

    /// **The sample's own exit criterion**: left alone, it reaches an orbit
    /// that stays up.
    ///
    /// "Stays up" is the periapsis above the atmosphere, which is the only
    /// definition that survives timewarp — an orbit whose low point is in the
    /// air is one the auto-drop will interrupt and drag will eventually bring
    /// down.
    #[test]
    fn the_autopilot_reaches_an_orbit_that_stays_up() {
        let game = flown(ASCENT_TICKS);
        let state = seen(&game);
        assert_eq!(
            state.phase,
            Phase::Flying,
            "the flight should still be up, and it is {} at {:.0} m",
            state.phase.label(),
            state.altitude
        );
        assert!(
            state.periapsis > AIR.ceiling,
            "the periapsis is {:.0} m, inside an atmosphere that reaches {:.0} m",
            state.periapsis,
            AIR.ceiling
        );
        let apoapsis = state.apoapsis.expect("a closed orbit has an apoapsis");
        assert!(apoapsis > AIR.ceiling, "the apoapsis is {apoapsis:.0} m");
        assert!(
            state.fuel > 0.0,
            "the ascent should not need the whole tank"
        );
    }

    /// The ship leaves the pad when the script says so, and not before.
    #[test]
    fn nothing_moves_until_the_clamp_is_released() {
        let before = seen(&flown(AUTOPILOT_LAUNCH_TICK - 2));
        assert_eq!(before.phase, Phase::Prelaunch);
        assert_eq!(before.altitude, 0.0, "clamped means clamped");
        assert!(
            before.throttle > 0.0,
            "the engine spools up on the pad, and it had reached {}",
            before.throttle
        );

        let after = seen(&flown(AUTOPILOT_LAUNCH_TICK + DEFAULT_TICK_HZ as u64));
        assert_eq!(after.phase, Phase::Flying);
        assert!(
            after.altitude > 0.0,
            "a second after release it should be off the ground, and it is at {:.1} m",
            after.altitude
        );
        assert!(
            after.fuel < 1.0,
            "a second of full throttle should have cost fuel"
        );
    }

    /// **The auto-drop.** Timewarp is refused whenever the analytic solution
    /// would stop being the truth.
    #[test]
    fn timewarp_drops_the_moment_the_engine_lights() {
        let mut game = flown(ASCENT_TICKS);
        // In orbit and coasting, the script has already warped.
        let orbiting = seen(&game);
        assert!(
            orbiting.warp_allowed,
            "a coasting ship above the air may warp"
        );
        assert!(
            orbiting.warp > 1,
            "the script warps once the orbit is made, and it is at x{}",
            orbiting.warp
        );

        // One frame of throttle is enough: the drop happens on the tick the
        // burn starts, not a tick into it.
        game.set_controls(Controls {
            throttle_up: true,
            ..Controls::default()
        });
        game.tick();
        let burning = seen(&game);
        assert!(burning.throttle > 0.0, "the throttle opened");
        assert_eq!(
            burning.warp, 1,
            "a burn must drop the warp on the tick it starts"
        );
        assert!(!burning.warp_allowed);
    }

    /// The first thing the player asks for takes the flight off the script, for
    /// good.
    #[test]
    fn the_player_takes_the_controls_and_never_gives_them_back() {
        let mut game = flown(AUTOPILOT_LAUNCH_TICK + 10 * DEFAULT_TICK_HZ as u64);
        assert!(seen(&game).autopilot, "the script is flying to begin with");

        game.set_controls(Controls {
            throttle_down: true,
            ..Controls::default()
        });
        let before = seen(&game).throttle;
        game.tick();
        assert!(!seen(&game).autopilot, "one control ends the script");
        let closed_a_notch = seen(&game).throttle;
        assert!(
            closed_a_notch < before,
            "the control the player sent should have moved the throttle from {before}"
        );

        // And it stays ended through ticks that carry nothing at all, which is
        // what a player who stops pressing keys produces.
        for _ in 0..DEFAULT_TICK_HZ {
            game.tick();
        }
        let idle = seen(&game);
        assert!(!idle.autopilot, "the script must not come back");
        // A tick of `throttle_down` closes the throttle by a tick's worth, and
        // then nothing touches it again — which is the claim. A script still
        // flying would have pushed it straight back to full for the ascent.
        assert_eq!(
            idle.throttle, closed_a_notch,
            "a second of nobody pressing anything moved the throttle, so something \
             is still flying"
        );
    }

    /// Two flights of the same length are the same flight.
    ///
    /// Nothing here is seeded and nothing is random: the script is a function
    /// of the state, and the state is a function of the tick. So this compares
    /// the numbers themselves rather than a tolerance — on one target, which is
    /// what it claims. It is not a claim about `libm` agreeing across targets.
    #[test]
    fn the_same_flight_replays_identically() {
        const TICKS: u64 = 30 * DEFAULT_TICK_HZ as u64;
        let first = flown(TICKS);
        let second = flown(TICKS);
        assert_eq!(
            first.state_hash(),
            second.state_hash(),
            "two runs of {TICKS} ticks diverged"
        );
        assert_eq!(seen(&first), seen(&second));
    }

    /// A control that reached the wire reached the simulation.
    ///
    /// Sent as bytes through the transport like a remote client's, so this
    /// covers the encode, the decode and the merge rather than a direct call
    /// into the flight.
    #[test]
    fn a_control_frame_crosses_the_wire() {
        let mut game = flown(AUTOPILOT_LAUNCH_TICK + 5 * DEFAULT_TICK_HZ as u64);
        let before = seen(&game).attitude;
        for _ in 0..DEFAULT_TICK_HZ {
            game.set_controls(Controls {
                pitch_left: true,
                ..Controls::default()
            });
            game.tick();
        }
        let after = seen(&game).attitude;
        // A second of turning at the published rate is most of a radian, which
        // no two headings a second apart on the ascent are.
        let turned = (before[0] * after[1] - before[1] * after[0]).abs();
        assert!(
            turned > 0.1,
            "a second of turning left moved the heading from {before:?} to {after:?}"
        );
    }

    /// An intent that this build did not write is refused rather than read as
    /// the player letting go of everything.
    #[test]
    fn a_control_frame_this_build_did_not_write_is_refused() {
        assert_eq!(Intent::from_wire(&[]), None, "a frame of no bytes");
        assert_eq!(Intent::from_wire(&[0, 0]), None, "a frame of two bytes");
        assert_eq!(
            Intent::from_wire(&[!INTENT_FLAGS]),
            None,
            "every bit the flags do not define"
        );
        // And the round trip, which is what says the two halves agree.
        let asked = Intent {
            throttle_up: true,
            pitch_right: true,
            warp_down: true,
            ..Intent::default()
        };
        assert_eq!(Intent::from_wire(&[asked.to_wire()]), Some(asked));
    }

    /// The orbit drawn on the map is the orbit the readouts describe.
    ///
    /// The path is sampled from the propagator rather than fitted, so this is
    /// the check that the two agree: every sample must lie between the
    /// periapsis and the apoapsis the panel prints.
    #[test]
    fn every_sample_of_the_path_lies_on_the_orbit_the_readouts_report() {
        let state = seen(&flown(ASCENT_TICKS));
        assert_eq!(
            state.path.len(),
            PATH_SAMPLES,
            "a flying ship on a closed orbit has a whole path"
        );
        let apoapsis = state.apoapsis.expect("closed") + state.body_radius;
        let periapsis = state.periapsis + state.body_radius;
        for point in &state.path {
            let radius = (point[0] * point[0] + point[1] * point[1]).sqrt();
            assert!(
                radius >= periapsis - 1.0 && radius <= apoapsis + 1.0,
                "a path sample at {radius:.1} m is outside {periapsis:.1}..{apoapsis:.1}"
            );
        }
    }

    /// Straight up is a trajectory with no orbit in it, and the map says so by
    /// drawing nothing rather than by dividing by zero.
    #[test]
    fn a_vertical_ascent_has_no_path_to_draw() {
        // Just off the pad, still going straight up: the planet does not
        // rotate, so the launch is radial and the angular momentum is exactly
        // zero.
        let state = seen(&flown(AUTOPILOT_LAUNCH_TICK + 10));
        assert_eq!(state.phase, Phase::Flying);
        assert!(state.altitude > 0.0 && state.altitude < TURN_START);
        assert!(
            state.path.is_empty(),
            "a radial trajectory passes through the centre of attraction, where \
             there is nothing to propagate, and it drew {} samples",
            state.path.len()
        );
    }
}
