//! The two rung 0 scenes: a T-handle tumbling in zero g, and a box dropped
//! flat.
//!
//! ```text
//!        zero g                         under gravity
//!
//!        ━━━┳━━━   spun about its        ┌──┐  dropped flat, no spin
//!           ┃      middle axis, it       └──┘
//!           ┃      flips over and back     │
//!                                          ▼
//!   ──────────────────────────────────────────────────────  the floor
//!                                          ▼  and straight through it:
//!                                             no contact solver yet
//! ```
//!
//! Both live in one [`PhysicsSystem`], stepped [`SUBSTEPS`] times a tick. The
//! T-handle feels nothing; the box feels gravity, applied to it alone through
//! [`PhysicsSystem::apply_force`], because a gravity provider would pull the
//! handle down with it.
//!
//! # What each scene shows, and what it cannot yet
//!
//! **The T-handle** is `docs/plan/36-contact-solver.md` rung 0's proving scene
//! whole: the inertia tensor from two boxes, the gyroscopic term that turns a
//! spin about the intermediate axis into a flip, and the conservation that keeps
//! it flipping for as long as the page is open. Its counters are the flips, and
//! how far the angular momentum and the energy have drifted from where they
//! started.
//!
//! **The box** can show only half of what its name promises. "Dropped flat"
//! means landing flat, and landing needs contacts, which are rung 1. What it
//! can show honestly is that a body dropped with no spin **gains none**: its
//! orientation stays the identity to the bit and its angular velocity stays
//! zero all the way down — and then it falls through the floor, which the page
//! says in as many words rather than hiding. It is put back at the top once it
//! is well below the floor, and the page counts the drops.
//!
//! # Determinism
//!
//! [`Scenes::hash`] is the physics system's own state hash under FNV-1a. The
//! scenes start the same every time and take no input, so the hash at
//! [`CHECK_TICK`] is a constant: [`PINNED_HASH`] is it, taken natively and
//! asserted by `the_hash_at_the_check_tick_is_the_pinned_one`, and the browser
//! gate reads the same tick's hash off the wasm build's heartbeat and holds it
//! to the same constant — which is the native-against-wasm check
//! `docs/plan/sample/24-tumble.md` asks for once the simulation's trigonometry
//! is pinned.

use std::hash::Hasher;

use crcbl::ecs::{Entity, SystemTrait as _};
use crcbl::math::{DQuat, DVec3};
use crcbl::phys::{MassProperties, PhysicsSystem, RigidBody, Transform};

/// How fast the loop ticks, in ticks a second: the engine's own default.
pub const TICK_HZ: u32 = 60;

/// Physics substeps per tick, so the scenes step at 240 Hz — the rate
/// `crcbl-phys`'s rotation tests measure their bounds at.
pub const SUBSTEPS: u32 = 4;

/// The tick whose hash [`PINNED_HASH`] is: ten simulated seconds in, far enough
/// that the handle has flipped and the box has been dropped more than once.
pub const CHECK_TICK: u64 = 600;

/// [`Scenes::hash`] at [`CHECK_TICK`], taken on x86-64 Linux on 2026-09-17.
pub const PINNED_HASH: u64 = 0xd73f_feb1_805f_1a71;

/// Standard gravity, in m/s², pulling the box and nothing else.
pub const GRAVITY: f64 = 9.81;

/// The T-handle's bar: half-extents, and where its centre sits above the stem's
/// before the pair is moved onto their shared centre of mass.
pub const BAR_HALF: DVec3 = DVec3::new(0.4, 0.05, 0.05);
/// The bar's centre above the stem's.
pub const BAR_ABOVE_STEM: f64 = 0.35;
/// The stem's half-extents.
pub const STEM_HALF: DVec3 = DVec3::new(0.05, 0.3, 0.05);
/// Both parts' density, in kg/m³: water, so the handle weighs what a real one
/// of this size would.
const DENSITY: f64 = 1000.0;

/// Where the handle's centre of mass floats.
pub const HANDLE_AT: DVec3 = DVec3::new(-1.6, 2.2, 0.0);
/// How fast it is spun about its intermediate axis, in rad/s.
pub const SPIN_RATE: f64 = 6.0;
/// The nudge about its axis of least inertia: the imperfection every real
/// spin has, and what the instability grows out of.
pub const NUDGE_RATE: f64 = 0.06;

/// The box's half-extent, in metres.
pub const BOX_HALF: f64 = 0.3;
/// Its mass, in kilograms.
const BOX_MASS: f64 = 20.0;
/// Where it is dropped from.
pub const BOX_START: DVec3 = DVec3::new(1.6, 4.0, 0.0);
/// How far below the floor it falls before it is put back.
pub const RESPAWN_BELOW: f64 = -6.0;

/// How near to reversed an axis must come, as a cosine, to count as a flip.
const FLIPPED: f64 = 0.9;

/// The two bodies' entities. There is no ECS world on this page — the scenes
/// are one physics system — so they are named by hand, generation one being
/// the first a pool issues.
fn entity(index: u32) -> Entity {
    Entity::from_bits((1u64 << 32) | u64::from(index)).expect("generation 1 is never zero")
}

/// One part of the T-handle, in the handle's own frame.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Part {
    /// Its centre, relative to the handle's centre of mass.
    pub centre: DVec3,
    /// Its half-extents.
    pub half: DVec3,
}

/// What the page and the heartbeat both read, at one instant.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Reading {
    /// Ticks stepped.
    pub tick: u64,
    /// How many times the handle's intermediate axis has reversed.
    pub flips: u64,
    /// The worst relative drift of its angular momentum so far.
    pub momentum_drift: f64,
    /// The worst relative drift of its kinetic energy so far, either way.
    pub energy_drift: f64,
    /// How fast it is turning, in rad/s.
    pub handle_spin: f64,
    /// The box's height, in metres.
    pub box_height: f64,
    /// How fast the box is turning, in rad/s — zero, if nothing gave it spin.
    pub box_spin: f64,
    /// Whether the box's orientation is still the identity, bit for bit.
    pub box_level: bool,
    /// How many times the box has been put back at the top.
    pub drops: u64,
    /// The last tick's physics step, in microseconds, where this build has a
    /// clock to measure it with.
    pub step_micros: Option<f64>,
    /// [`Scenes::hash`] now.
    pub hash: u64,
}

/// Both scenes, in one physics system.
#[derive(Debug)]
pub struct Scenes {
    phys: PhysicsSystem,
    parts: [Part; 2],
    /// The handle's principal axes in ascending order of moment.
    axes: [DVec3; 3],
    momentum: DVec3,
    energy: f64,
    tick: u64,
    flips: u64,
    /// Whether the intermediate axis last pointed with the momentum.
    aligned: bool,
    momentum_drift: f64,
    energy_drift: f64,
    drops: u64,
    step_micros: Option<f64>,
}

impl Default for Scenes {
    fn default() -> Self {
        Self::new()
    }
}

impl Scenes {
    /// Both scenes at their start.
    #[must_use]
    pub fn new() -> Self {
        let volume = |half: DVec3| 8.0 * half.x * half.y * half.z;
        let stem = MassProperties::cuboid(DENSITY * volume(STEM_HALF), STEM_HALF, DVec3::ZERO);
        let bar = MassProperties::cuboid(
            DENSITY * volume(BAR_HALF),
            BAR_HALF,
            DVec3::new(0.0, BAR_ABOVE_STEM, 0.0),
        );
        let props = MassProperties::combine(&[bar, stem]);
        let parts = [
            Part {
                centre: bar.centre_of_mass - props.centre_of_mass,
                half: BAR_HALF,
            },
            Part {
                centre: stem.centre_of_mass - props.centre_of_mass,
                half: STEM_HALF,
            },
        ];

        let inertia = props.inertia;
        let mut moments = [
            (inertia.x_axis.x, DVec3::X),
            (inertia.y_axis.y, DVec3::Y),
            (inertia.z_axis.z, DVec3::Z),
        ];
        moments.sort_by(|a, b| a.0.total_cmp(&b.0));
        let axes = [moments[0].1, moments[1].1, moments[2].1];

        let mut handle = RigidBody::new_dynamic(props.mass).with_inertia(inertia);
        handle.angular_velocity = axes[1] * SPIN_RATE + axes[0] * NUDGE_RATE;
        let momentum = handle.angular_momentum(DQuat::IDENTITY);
        let energy = handle.kinetic_energy(DQuat::IDENTITY);

        let mut phys = PhysicsSystem::new();
        phys.set_body(entity(0), handle);
        phys.set_transform(entity(0), Transform::from_position(HANDLE_AT));
        let crate_ = MassProperties::cuboid(BOX_MASS, DVec3::splat(BOX_HALF), DVec3::ZERO);
        phys.set_body(
            entity(1),
            RigidBody::new_dynamic(BOX_MASS).with_inertia(crate_.inertia),
        );
        phys.set_transform(entity(1), Transform::from_position(BOX_START));

        Self {
            phys,
            parts,
            axes,
            momentum,
            energy,
            tick: 0,
            flips: 0,
            aligned: true,
            momentum_drift: 0.0,
            energy_drift: 0.0,
            drops: 0,
            step_micros: None,
        }
    }

    /// One tick of `tick_dt` seconds: [`SUBSTEPS`] physics steps, then the
    /// counters.
    pub fn step(&mut self, tick_dt: f64) {
        #[cfg(not(target_arch = "wasm32"))]
        let started = std::time::Instant::now();

        let dt = tick_dt / f64::from(SUBSTEPS);
        for _ in 0..SUBSTEPS {
            self.phys
                .apply_force(entity(1), DVec3::new(0.0, -GRAVITY * BOX_MASS, 0.0));
            self.phys.step(dt);
        }

        // The measurement ends before the bookkeeping below, which is the
        // page's rather than the engine's. The browser build has no clock a
        // module can read — `std::time::Instant` panics on wasm32 — so there
        // the reading is absent rather than zero.
        #[cfg(not(target_arch = "wasm32"))]
        {
            self.step_micros = Some(started.elapsed().as_secs_f64() * 1.0e6);
        }

        self.tick += 1;
        self.count_handle();
        self.recycle_box();
    }

    fn handle(&self) -> (RigidBody, Transform) {
        (
            *self.phys.body(entity(0)).expect("the handle has a body"),
            *self
                .phys
                .transform(entity(0))
                .expect("the handle has a transform"),
        )
    }

    fn crate_(&self) -> (RigidBody, Transform) {
        (
            *self.phys.body(entity(1)).expect("the box has a body"),
            *self
                .phys
                .transform(entity(1))
                .expect("the box has a transform"),
        )
    }

    fn count_handle(&mut self) {
        let (body, transform) = self.handle();
        let momentum = body.angular_momentum(transform.rotation);
        self.momentum_drift = self
            .momentum_drift
            .max((momentum - self.momentum).length() / self.momentum.length());
        let energy = body.kinetic_energy(transform.rotation);
        self.energy_drift = self
            .energy_drift
            .max((energy - self.energy).abs() / self.energy);

        let along = (transform.rotation * self.axes[1]).dot(self.momentum.normalize());
        if self.aligned && along < -FLIPPED {
            self.aligned = false;
            self.flips += 1;
        } else if !self.aligned && along > FLIPPED {
            self.aligned = true;
            self.flips += 1;
        }
    }

    fn recycle_box(&mut self) {
        let (_, transform) = self.crate_();
        if transform.position.y < RESPAWN_BELOW {
            self.phys
                .set_transform(entity(1), Transform::from_position(BOX_START));
            if let Some(body) = self.phys.body_mut(entity(1)) {
                body.velocity = DVec3::ZERO;
            }
            self.drops += 1;
        }
    }

    /// The physics state hash under FNV-1a — a digest that, unlike `std`'s
    /// hasher, means the same thing in every build.
    #[must_use]
    pub fn hash(&self) -> u64 {
        let mut hasher = Fnv(0xcbf2_9ce4_8422_2325);
        self.phys.hash_state(&mut hasher);
        hasher.finish()
    }

    /// Ticks stepped so far.
    #[must_use]
    pub const fn tick_count(&self) -> u64 {
        self.tick
    }

    /// The handle's parts, each as a world-space centre, the handle's
    /// orientation and the part's half-extents.
    #[must_use]
    pub fn handle_parts(&self) -> [(DVec3, DQuat, DVec3); 2] {
        let (_, transform) = self.handle();
        self.parts.map(|part| {
            (
                transform.position + transform.rotation * part.centre,
                transform.rotation,
                part.half,
            )
        })
    }

    /// The box's world-space centre and orientation.
    #[must_use]
    pub fn box_pose(&self) -> (DVec3, DQuat) {
        let (_, transform) = self.crate_();
        (transform.position, transform.rotation)
    }

    /// Every counter, at this instant.
    #[must_use]
    pub fn reading(&self) -> Reading {
        let (handle, _) = self.handle();
        let (crate_, pose) = self.crate_();
        Reading {
            tick: self.tick,
            flips: self.flips,
            momentum_drift: self.momentum_drift,
            energy_drift: self.energy_drift,
            handle_spin: handle.angular_velocity.length(),
            box_height: pose.position.y,
            box_spin: crate_.angular_velocity.length(),
            box_level: pose.rotation.to_array().map(f64::to_bits)
                == DQuat::IDENTITY.to_array().map(f64::to_bits),
            drops: self.drops,
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
mod tests {
    use super::*;

    /// The tick the engine's loop hands [`Scenes::step`] — whole nanoseconds,
    /// so not `1.0 / 60.0` — taken from the loop's own clock rather than
    /// copied, because a test stepping by a different `dt` pins a hash no
    /// running build ever reaches.
    fn tick_dt() -> f64 {
        crcbl::core::FrameClock::new(TICK_HZ).tick_dt_secs()
    }

    fn run(ticks: u64) -> Scenes {
        let mut scenes = Scenes::new();
        for _ in 0..ticks {
            scenes.step(tick_dt());
        }
        scenes
    }

    /// **The native half of the determinism check.** Two runs agree, and both
    /// land on the constant the browser gate holds the wasm build to.
    #[test]
    fn the_hash_at_the_check_tick_is_the_pinned_one() {
        let first = run(CHECK_TICK).hash();
        assert_eq!(first, run(CHECK_TICK).hash(), "two runs disagreed");
        assert_eq!(
            first, PINNED_HASH,
            "tick {CHECK_TICK} hashes to {first:#018x}, not the pinned {PINNED_HASH:#018x}"
        );
        assert_ne!(
            run(CHECK_TICK + 1).hash(),
            first,
            "the next tick hashed the same, so the hash cannot see the scenes move"
        );
    }

    /// The handle's parts sit about its centre of mass: weighted by volume,
    /// their offsets cancel.
    #[test]
    fn the_handle_is_placed_about_its_centre_of_mass() {
        let scenes = Scenes::new();
        let volume = |half: DVec3| half.x * half.y * half.z;
        let moment = scenes.parts.iter().fold(DVec3::ZERO, |sum, part| {
            sum + part.centre * volume(part.half)
        });
        assert!(moment.length() < 1e-15, "the parts balance at {moment:?}");
        assert_ne!(
            scenes.axes[1], scenes.axes[0],
            "the intermediate axis is not distinct"
        );
    }

    /// **The page's claims, over what it shows by the check tick.** The handle
    /// has flipped with its momentum and energy intact, and the box has fallen
    /// through the floor and been put back without ever turning.
    #[test]
    fn by_the_check_tick_the_handle_has_flipped_and_the_box_never_turned() {
        let scenes = run(CHECK_TICK);
        let reading = scenes.reading();
        assert!(
            reading.flips >= 2,
            "only {} flips in ten seconds",
            reading.flips
        );
        assert!(
            reading.momentum_drift < 1e-10 && reading.energy_drift < 1e-10,
            "the handle drifted {:e} in momentum and {:e} in energy",
            reading.momentum_drift,
            reading.energy_drift
        );
        assert!(reading.drops >= 1, "the box was never dropped through");
        assert_eq!(reading.box_spin, 0.0, "the box picked up spin");
        assert!(reading.box_level, "the box tilted");
        assert!(reading.step_micros.is_some(), "a native step went untimed");
    }

    /// Without the nudge the spin is balanced on its axis, and the handle keeps
    /// it: the flip is the instability growing, not something the page drives.
    #[test]
    fn a_handle_with_no_nudge_does_not_flip_within_the_check_window() {
        let mut scenes = Scenes::new();
        let body = scenes.phys.body_mut(entity(0)).expect("the handle");
        body.angular_velocity = scenes.axes[1] * SPIN_RATE;
        for _ in 0..CHECK_TICK {
            scenes.step(tick_dt());
        }
        assert_eq!(scenes.reading().flips, 0);
    }
}
