//! The gallery: six rooms, each with its own physics, all stepped every tick
//! whichever one the camera is looking at.
//!
//! ```text
//!   x = 0          x = 12              x = 26             x = 44        x = 66          x = 92
//!   Spin           Wall                Pit                Tower         Bullets         Bridge
//!   rung 0         rungs 1 and 2       rung 1             rung 2        rung 4          rung 5
//!   T-handle,      the obstacle wall   a thousand balls   a column, a   a cannon at a   a cradle, a
//!   a box landing  with balls, pills   poured into a pit  pyramid and   plate and a     plank bridge,
//!                  and cubes                              dominoes      wall; a plank   ragdolls
//! ```
//!
//! **The view never reaches the simulation.** Keys `1` to `6` move the
//! camera and change which room's counters the panel shows, and nothing else:
//! every room steps every tick from the same start, so the hash at
//! [`CHECK_TICK`] is a constant whatever was pressed. [`PINNED_HASH`] is it,
//! taken natively and asserted by `the_hash_at_the_check_tick_is_the_pinned_one`,
//! and the browser gate reads the same tick's hash off the wasm build's
//! heartbeat and holds it to the same constant.
//!
//! **Each room is its own system** because a gravity provider is global: the
//! Spin room's T-handle is in zero g, and the wall and the pit are not. It
//! also makes every counter — pairs, contacts, each stage's time — the room's
//! own rather than the gallery's.
//!
//! See [`crate::spin`], [`crate::wall`], [`crate::pit`], [`crate::tower`],
//! [`crate::bullets`] and [`crate::bridge`] for what each room shows and what
//! it cannot yet.

use std::hash::Hasher;

use crcbl::core::input::KeyCode;
use crcbl::ecs::Entity;
use crcbl::math::{DQuat, DVec3};
use crcbl::phys::{ContactCounters, StageTimes};

use crate::bridge::{Bridge, BridgeReading};
use crate::bullets::{Bullets, BulletsReading};
use crate::pit::{Pit, PitReading};
use crate::spin::{Spin, SpinReading};
use crate::tower::{Tower, TowerReading};
use crate::wall::{Wall, WallReading};

#[cfg(test)]
std::thread_local! {
    pub(crate) static HASH_CALLS: std::cell::Cell<usize> = const { std::cell::Cell::new(0) };
}

/// How fast the loop ticks, in ticks a second: the engine's own default.
pub const TICK_HZ: u32 = 60;

/// The tick whose hash [`PINNED_HASH`] is: ten simulated seconds in, far enough
/// that the handle has flipped, the box has landed three times, the wall has
/// most of its bodies, the pit three quarters of its balls, the dominoes
/// have all fallen once and are falling again, and the cannon has fired
/// twenty shots; and in the Bridge room the cradle has passed its momentum,
/// four crates have slid down the bridge and the ragdolls have been pushed
/// off the landing twice.
pub const CHECK_TICK: u64 = 600;

/// [`Scenes::hash`] at [`CHECK_TICK`], taken on x86-64 Windows on 2026-09-23,
/// after rung 5's joints added the Bridge room. The five rooms before it
/// still hash to the value pinned before it (`0x810e_2250_7c7a_fc8f`), taken
/// without the Bridge room's share: joints and solver groups left every
/// scene without them bit for bit as it was.
pub const PINNED_HASH: u64 = 0x32a0_2fb4_6966_7d55;

/// Standard gravity, in m/s².
pub const GRAVITY: f64 = 9.81;

/// A room's entity by index. There is no ECS world on this page — each room is
/// one physics system — so entities are named by hand, generation one being
/// the first a pool issues.
pub(crate) fn entity(index: u32) -> Entity {
    Entity::from_bits((1u64 << 32) | u64::from(index)).expect("generation 1 is never zero")
}

/// What a room is drawn as.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Tint {
    /// The T-handle.
    Handle,
    /// The dropped box.
    Box,
    /// A ball.
    Ball,
    /// A pill.
    Pill,
    /// A peg, a bar or a wedge.
    Peg,
    /// A board, a side or a pit wall.
    Board,
}

/// A shape to draw, where the physics has it.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Shape {
    /// A ball.
    Sphere {
        /// Names it from frame to frame, within its room.
        key: u64,
        /// Its centre.
        centre: DVec3,
        /// Its radius.
        radius: f64,
        /// How it is drawn.
        tint: Tint,
    },
    /// A capsule, between its core's two ends.
    Capsule {
        /// Names it from frame to frame, within its room.
        key: u64,
        /// One end of its core.
        a: DVec3,
        /// The other.
        b: DVec3,
        /// Its radius.
        radius: f64,
        /// How it is drawn.
        tint: Tint,
    },
    /// A box.
    Box {
        /// Names it from frame to frame, within its room.
        key: u64,
        /// Its centre.
        centre: DVec3,
        /// Its orientation.
        rotation: DQuat,
        /// Its half-extents.
        half: DVec3,
        /// How it is drawn.
        tint: Tint,
    },
}

/// One room of the gallery.
pub trait Room {
    /// One tick of `dt`, reading `clock` between the physics stages if given.
    fn step(&mut self, dt: f64, clock: Option<&mut dyn FnMut() -> f64>);
    /// The room's physics state into a determinism hash.
    fn hash(&self, hasher: &mut dyn Hasher);
    /// The bodies that move, where they are now.
    fn bodies(&self, out: &mut Vec<Shape>);
    /// The fixtures, which never move.
    fn fixtures(&self, out: &mut Vec<Shape>);
}

/// A room's contact counters, over its run: `docs/plan/36-contact-solver.md`
/// rung 1's row, rung 2's, rung 3's, rung 4's and rung 5's.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct Tally {
    /// Bodies that step: the awake ones.
    pub bodies: usize,
    /// Bodies asleep.
    pub sleeping: usize,
    /// Islands awake.
    pub islands: usize,
    /// Islands asleep.
    pub sleeping_islands: usize,
    /// The solver's time on the last timed tick with every body asleep, in
    /// seconds — rung 3's "solver time at rest" — or `None` if no timed tick
    /// has been at rest yet.
    pub solver_at_rest: Option<f64>,
    /// Pairs in the pair set.
    pub pairs: usize,
    /// Contacts with a point.
    pub touching: usize,
    /// Manifold points in the last tick.
    pub points: usize,
    /// Of those, the ones whose feature id persisted from the tick before.
    pub persisted: usize,
    /// Contacts begun, over the run.
    pub begun: u64,
    /// Contacts ended, over the run.
    pub ended: u64,
    /// The deepest overlap in the last tick, in metres.
    pub worst_penetration: f64,
    /// The deepest overlap in any tick, in metres.
    pub peak_penetration: f64,
    /// Points the restitution pass bounced, over the run.
    pub bounces: u64,
    pub(crate) bounce_ratio_sum: f64,
    pub(crate) restitution_sum: f64,
    /// Bodies swept in the last tick — rung 4's row.
    pub swept: usize,
    /// Times of impact computed in the last tick: the sweep candidates.
    pub sweep_candidates: usize,
    /// Swept bodies stopped at an impact, over the run.
    pub sweep_hits: u64,
    /// The motion those stops dropped, in seconds, over the run.
    pub dropped_time: f64,
    /// Joints solved in the last tick — rung 5's row.
    pub joints: usize,
    /// The worst positional drift of any joint in the last tick, in metres.
    pub joint_error: f64,
    /// The worst angular drift, in radians.
    pub joint_angle_error: f64,
    /// The worst positional drift of any joint in any tick, in metres.
    pub peak_joint_error: f64,
    /// Joints broken, over the run.
    pub broken_joints: u64,
    /// The last tick's stage times, where the build had a clock.
    pub stages: Option<StageTimes>,
}

impl Tally {
    /// Folds one tick's counters in.
    pub fn add(&mut self, counters: &ContactCounters) {
        self.bodies = counters.bodies;
        self.sleeping = counters.sleeping;
        self.islands = counters.islands;
        self.sleeping_islands = counters.sleeping_islands;
        if counters.bodies == 0
            && counters.sleeping > 0
            && let Some(stages) = counters.stages
        {
            self.solver_at_rest = Some(stages.solver);
        }
        self.pairs = counters.pairs;
        self.touching = counters.touching;
        self.points = counters.points;
        self.persisted = counters.persisted;
        self.begun += counters.begun;
        self.ended += counters.ended;
        self.worst_penetration = counters.worst_penetration;
        self.peak_penetration = self.peak_penetration.max(counters.worst_penetration);
        self.bounces += counters.bounces;
        self.bounce_ratio_sum += counters.bounce_ratio_sum;
        self.restitution_sum += counters.restitution_sum;
        self.swept = counters.swept;
        self.sweep_candidates = counters.sweep_candidates;
        self.sweep_hits += counters.sweep_hits as u64;
        self.dropped_time += counters.dropped_time;
        self.joints = counters.joints;
        self.joint_error = counters.joint_error;
        self.joint_angle_error = counters.joint_angle_error;
        self.peak_joint_error = self.peak_joint_error.max(counters.joint_error);
        self.broken_joints += counters.broken_joints as u64;
        self.stages = counters.stages;
    }

    /// The mean bounce over the run — separating speed over approach speed —
    /// or `None` if nothing has bounced.
    #[must_use]
    pub fn bounce_ratio(&self) -> Option<f64> {
        #[allow(clippy::cast_precision_loss)]
        (self.bounces > 0).then(|| self.bounce_ratio_sum / self.bounces as f64)
    }

    /// The mean restitution those bounces asked for.
    #[must_use]
    pub fn restitution(&self) -> Option<f64> {
        #[allow(clippy::cast_precision_loss)]
        (self.bounces > 0).then(|| self.restitution_sum / self.bounces as f64)
    }

    /// The last tick's points per touching contact, or `None` if nothing
    /// touched.
    #[must_use]
    pub fn points_per_manifold(&self) -> Option<f64> {
        #[allow(clippy::cast_precision_loss)]
        (self.touching > 0).then(|| self.points as f64 / self.touching as f64)
    }

    /// The share of the last tick's points whose feature id persisted, or
    /// `None` if there were none. Flickering ids read below one.
    #[must_use]
    pub fn persisted_ratio(&self) -> Option<f64> {
        #[allow(clippy::cast_precision_loss)]
        (self.points > 0).then(|| self.persisted as f64 / self.points as f64)
    }
}

/// Which room the camera and the panel are on.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum View {
    /// Rung 0's room.
    Spin,
    /// The obstacle wall.
    #[default]
    Wall,
    /// The ball pit.
    Pit,
    /// Rung 2's room: the column, the pyramid and the dominoes.
    Tower,
    /// Rung 4's room: the cannon, the plate, the brick wall and the plank.
    Bullets,
    /// Rung 5's room: the cradle, the bridge and the ragdolls on the stairs.
    Bridge,
}

impl View {
    /// The view a key asks for, if it asks for one.
    #[must_use]
    pub const fn for_key(key: KeyCode) -> Option<Self> {
        match key {
            KeyCode::Digit1 => Some(Self::Spin),
            KeyCode::Digit2 => Some(Self::Wall),
            KeyCode::Digit3 => Some(Self::Pit),
            KeyCode::Digit4 => Some(Self::Tower),
            KeyCode::Digit5 => Some(Self::Bullets),
            KeyCode::Digit6 => Some(Self::Bridge),
            _ => None,
        }
    }

    /// Its name, as the page and the heartbeat print it.
    #[must_use]
    pub const fn name(self) -> &'static str {
        match self {
            Self::Spin => "spin",
            Self::Wall => "wall",
            Self::Pit => "pit",
            Self::Tower => "tower",
            Self::Bullets => "bullets",
            Self::Bridge => "bridge",
        }
    }
}

/// What the page and the heartbeat both read, at one instant.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Reading {
    /// Ticks stepped.
    pub tick: u64,
    /// The room on screen.
    pub view: View,
    /// The Spin room.
    pub spin: SpinReading,
    /// The wall.
    pub wall: WallReading,
    /// The pit.
    pub pit: PitReading,
    /// The Tower room.
    pub tower: TowerReading,
    /// The Bullets room.
    pub bullets: BulletsReading,
    /// The Bridge room.
    pub bridge: BridgeReading,
    /// The last tick's physics over every room, in microseconds, where
    /// this build has a clock to measure it with.
    pub step_micros: Option<f64>,
    /// [`Scenes::hash`] now.
    pub hash: u64,
}

/// The gallery.
#[derive(Debug, Default)]
pub struct Scenes {
    spin: Spin,
    wall: Wall,
    pit: Pit,
    tower: Tower,
    bullets: Bullets,
    bridge: Bridge,
    view: View,
    tick: u64,
    step_micros: Option<f64>,
}

impl Scenes {
    /// Every room at its start, the camera on the wall.
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// One tick of `tick_dt` seconds for every room.
    pub fn step(&mut self, tick_dt: f64) {
        #[cfg(not(target_arch = "wasm32"))]
        {
            let epoch = std::time::Instant::now();
            let mut clock = || epoch.elapsed().as_secs_f64();
            self.spin.step(tick_dt, Some(&mut clock));
            self.wall.step(tick_dt, Some(&mut clock));
            self.pit.step(tick_dt, Some(&mut clock));
            self.tower.step(tick_dt, Some(&mut clock));
            self.bullets.step(tick_dt, Some(&mut clock));
            self.bridge.step(tick_dt, Some(&mut clock));
            self.step_micros = Some(clock() * 1.0e6);
        }
        // The browser build has no clock a module can read —
        // `std::time::Instant` panics on wasm32 — so there the readings are
        // absent rather than zero.
        #[cfg(target_arch = "wasm32")]
        {
            self.spin.step(tick_dt, None);
            self.wall.step(tick_dt, None);
            self.pit.step(tick_dt, None);
            self.tower.step(tick_dt, None);
            self.bullets.step(tick_dt, None);
            self.bridge.step(tick_dt, None);
        }
        self.tick += 1;
    }

    /// Points the camera and the panel at `view`. The simulation does not see
    /// it.
    pub fn set_view(&mut self, view: View) {
        self.view = view;
    }

    /// The room on screen.
    #[must_use]
    pub const fn view(&self) -> View {
        self.view
    }

    /// Every room's physics state under FNV-1a — a digest that, unlike `std`'s
    /// hasher, means the same thing in every build.
    #[must_use]
    pub fn hash(&self) -> u64 {
        #[cfg(test)]
        HASH_CALLS.with(|calls| calls.set(calls.get() + 1));
        let mut hasher = Fnv(0xcbf2_9ce4_8422_2325);
        self.spin.hash(&mut hasher);
        self.wall.hash(&mut hasher);
        self.pit.hash(&mut hasher);
        self.tower.hash(&mut hasher);
        self.bullets.hash(&mut hasher);
        self.bridge.hash(&mut hasher);
        hasher.finish()
    }

    /// Ticks stepped so far.
    #[must_use]
    pub const fn tick_count(&self) -> u64 {
        self.tick
    }

    /// Every room, for drawing, in a fixed order.
    #[must_use]
    pub fn rooms(&self) -> [&dyn Room; 6] {
        [
            &self.spin,
            &self.wall,
            &self.pit,
            &self.tower,
            &self.bullets,
            &self.bridge,
        ]
    }

    /// Every counter, at this instant.
    #[must_use]
    pub fn reading(&self) -> Reading {
        Reading {
            tick: self.tick,
            view: self.view,
            spin: self.spin.reading(),
            wall: self.wall.reading(),
            pit: self.pit.reading(),
            tower: self.tower.reading(),
            bullets: self.bullets.reading(),
            bridge: self.bridge.reading(),
            step_micros: self.step_micros,
            hash: self.hash(),
        }
    }
}

/// FNV-1a over the bytes a hash is fed.
struct Fnv(u64);

impl Hasher for Fnv {
    fn finish(&self) -> u64 {
        self.0
    }

    fn write(&mut self, bytes: &[u8]) {
        for byte in bytes {
            self.0 = (self.0 ^ u64::from(*byte)).wrapping_mul(0x0000_0100_0000_01b3);
        }
    }
}

#[cfg(test)]
pub(crate) mod tests {
    use super::*;

    /// The tick the engine's loop hands [`Scenes::step`] — whole nanoseconds,
    /// so not `1.0 / 60.0` — taken from the loop's own clock rather than
    /// copied, because a test stepping by a different `dt` pins a hash no
    /// running build ever reaches.
    pub(crate) fn tick_dt() -> f64 {
        crcbl::core::FrameClock::new(TICK_HZ).tick_dt_secs()
    }

    fn run(ticks: u64, view: View) -> Scenes {
        let mut scenes = Scenes::new();
        scenes.set_view(view);
        for _ in 0..ticks {
            scenes.step(tick_dt());
        }
        scenes
    }

    /// **The native half of the determinism check.** Two runs agree — one of
    /// them looking at another room the whole time — and both land on the
    /// constant the browser gate holds the wasm build to.
    #[test]
    fn the_hash_at_the_check_tick_is_the_pinned_one() {
        let first = run(CHECK_TICK, View::Wall).hash();
        assert_eq!(
            first,
            run(CHECK_TICK, View::Pit).hash(),
            "two runs disagreed"
        );
        assert_eq!(
            first, PINNED_HASH,
            "tick {CHECK_TICK} hashes to {first:#018x}, not the pinned {PINNED_HASH:#018x}"
        );
        assert_ne!(
            run(CHECK_TICK + 1, View::Wall).hash(),
            first,
            "the next tick hashed the same, so the hash cannot see the scenes move"
        );
    }

    /// The keys pick the rooms, and nothing else does.
    #[test]
    fn the_number_keys_pick_the_rooms() {
        assert_eq!(View::for_key(KeyCode::Digit1), Some(View::Spin));
        assert_eq!(View::for_key(KeyCode::Digit2), Some(View::Wall));
        assert_eq!(View::for_key(KeyCode::Digit3), Some(View::Pit));
        assert_eq!(View::for_key(KeyCode::Digit4), Some(View::Tower));
        assert_eq!(View::for_key(KeyCode::Digit5), Some(View::Bullets));
        assert_eq!(View::for_key(KeyCode::Digit6), Some(View::Bridge));
        assert_eq!(View::for_key(KeyCode::Space), None);
        assert_eq!(Scenes::new().view(), View::Wall);
    }

    /// **Once the pyramid sleeps, its solver time at rest is on the reading**,
    /// taken from a timed tick with nothing awake, and before then it is not.
    #[test]
    fn the_pyramid_reads_its_solver_time_at_rest_once_it_sleeps() {
        let early = run(10, View::Tower).reading();
        assert_eq!(early.tower.pyramid.solver_at_rest, None, "{early:?}");
        let settled = run(120, View::Tower).reading();
        let pyramid = settled.tower.pyramid;
        assert_eq!(pyramid.bodies, 0, "{pyramid:?}");
        let rest = pyramid.solver_at_rest.expect("a time at rest");
        assert!(rest >= 0.0, "{pyramid:?}");
    }

    /// A native step is timed, stage by stage, in every room with contacts.
    #[test]
    fn a_native_step_is_timed_stage_by_stage() {
        let reading = run(2, View::Wall).reading();
        assert!(reading.step_micros.is_some(), "a native step went untimed");
        for tally in [
            reading.spin.contacts,
            reading.wall.contacts,
            reading.pit.contacts,
            reading.tower.pyramid,
            reading.tower.column,
            reading.bridge.contacts,
        ] {
            assert!(tally.stages.is_some(), "{tally:?}");
        }
    }
}
