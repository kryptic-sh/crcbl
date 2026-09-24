//! The Spin room, rung 0's scenes: a T-handle tumbling in zero g, and a box
//! dropped flat onto a floor.
//!
//! ```text
//!        zero g                         under gravity
//!
//!        ━━━┳━━━   spun about its        ┌──┐  dropped flat, no spin
//!           ┃      middle axis, it       └──┘
//!           ┃      flips over and back     │
//!                                          ▼
//!   ─────────────────────────────────────┌──┐────────  the floor: a plane
//!                                        └──┘ lands on its four corners
//! ```
//!
//! Both live in one [`PhysicsSystem`] with contacts. The T-handle has no
//! collider and feels nothing; the box feels gravity, applied to it alone
//! through [`PhysicsSystem::apply_force`], because a gravity provider would
//! pull the handle down with it.
//!
//! # What each scene shows
//!
//! **The T-handle** is contact-solver rung 0's proving scene
//! whole: the inertia tensor from two boxes, the gyroscopic term that turns a
//! spin about the intermediate axis into a flip, and the conservation that keeps
//! it flipping. The contact pipeline integrates it as the same substeps would
//! without contacts, to the bit, so its drift is rung 0's.
//!
//! **The box** lands, which is rung 1: a box against a plane is its corners
//! below the plane, the one box pair `crcbl_phys::contact::manifold` has. It
//! rests on four points and is put back at the top every
//! [`DROP_EVERY_TICKS`] to land again. What it cannot show is a box against a
//! box, which is rung 2.

use crcbl::ecs::SystemTrait as _;
use crcbl::math::{DQuat, DVec3};
use crcbl::phys::{
    ColliderComponent, ContactSettings, MassProperties, PhysicsSystem, RigidBody, SurfaceMaterial,
    Transform,
};

use crate::scene::{GRAVITY, Room, Shape, Tally, Tint, entity};

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
/// How often it is put back at the top, in ticks: long enough to land and
/// settle.
pub const DROP_EVERY_TICKS: u64 = 240;
/// The box's surface, and the floor's: a little grip and no bounce.
const SURFACE: SurfaceMaterial = SurfaceMaterial::new(0.5, 0.0);

/// How near to reversed an axis must come, as a cosine, to count as a flip.
const FLIPPED: f64 = 0.9;

/// The handle's entity.
const HANDLE: u32 = 0;
/// The box's.
const BOX: u32 = 1;

/// One part of the T-handle, in the handle's own frame.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Part {
    /// Its centre, relative to the handle's centre of mass.
    pub centre: DVec3,
    /// Its half-extents.
    pub half: DVec3,
}

/// What the Spin room's counters read.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct SpinReading {
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
    /// How far the box's up axis has tipped from vertical, as the sine of the
    /// angle.
    pub box_tilt: f64,
    /// How many times the box has been dropped.
    pub drops: u64,
    /// The contact counters.
    pub contacts: Tally,
}

/// The Spin room.
#[derive(Debug)]
pub struct Spin {
    phys: PhysicsSystem,
    parts: [Part; 2],
    /// The handle's principal axes in ascending order of moment.
    axes: [DVec3; 3],
    momentum: DVec3,
    energy: f64,
    flips: u64,
    /// Whether the intermediate axis last pointed with the momentum.
    aligned: bool,
    momentum_drift: f64,
    energy_drift: f64,
    drops: u64,
    tick: u64,
    tally: Tally,
}

impl Default for Spin {
    fn default() -> Self {
        Self::new()
    }
}

impl Spin {
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

        let mut phys = PhysicsSystem::with_contacts(ContactSettings::DEFAULT);
        phys.add_plane(DVec3::Y, 0.0, SURFACE);
        phys.set_body(entity(HANDLE), handle);
        phys.set_transform(entity(HANDLE), Transform::from_position(HANDLE_AT));

        let crate_ = MassProperties::cuboid(BOX_MASS, DVec3::splat(BOX_HALF), DVec3::ZERO);
        phys.set_body(
            entity(BOX),
            RigidBody::new_dynamic(BOX_MASS).with_inertia(crate_.inertia),
        );
        let start = Transform::from_position(BOX_START);
        phys.set_transform(entity(BOX), start);
        phys.set_collider(
            entity(BOX),
            &ColliderComponent::Box {
                offset: DVec3::ZERO,
                half_extents: DVec3::splat(BOX_HALF),
                is_trigger: false,
            },
            &start,
        );
        phys.set_material(entity(BOX), SURFACE);

        Self {
            phys,
            parts,
            axes,
            momentum,
            energy,
            flips: 0,
            aligned: true,
            momentum_drift: 0.0,
            energy_drift: 0.0,
            drops: 1,
            tick: 0,
            tally: Tally::default(),
        }
    }

    fn handle(&self) -> (RigidBody, Transform) {
        (
            *self
                .phys
                .body(entity(HANDLE))
                .expect("the handle has a body"),
            *self
                .phys
                .transform(entity(HANDLE))
                .expect("the handle has a transform"),
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
        let transform = self.phys.transform(entity(BOX)).expect("the box");
        (transform.position, transform.rotation)
    }

    /// Every counter, at this instant.
    #[must_use]
    pub fn reading(&self) -> SpinReading {
        let (handle, _) = self.handle();
        let (position, rotation) = self.box_pose();
        let up = rotation * DVec3::Y;
        SpinReading {
            flips: self.flips,
            momentum_drift: self.momentum_drift,
            energy_drift: self.energy_drift,
            handle_spin: handle.angular_velocity.length(),
            box_height: position.y,
            box_tilt: (up.x * up.x + up.z * up.z).sqrt(),
            drops: self.drops,
            contacts: self.tally,
        }
    }

    /// The handle's principal axes, in ascending order of moment.
    #[cfg(test)]
    pub(crate) const fn axes(&self) -> [DVec3; 3] {
        self.axes
    }

    /// The parts about the handle's centre of mass.
    #[cfg(test)]
    pub(crate) const fn parts(&self) -> [Part; 2] {
        self.parts
    }

    /// Stops the handle's nudge, so its spin sits balanced on its axis.
    #[cfg(test)]
    pub(crate) fn without_nudge(mut self) -> Self {
        let axis = self.axes[1];
        let body = self.phys.body_mut(entity(HANDLE)).expect("the handle");
        body.angular_velocity = axis * SPIN_RATE;
        self
    }
}

impl Room for Spin {
    fn step(&mut self, dt: f64, clock: Option<&mut dyn FnMut() -> f64>) {
        self.phys
            .apply_force(entity(BOX), DVec3::new(0.0, -GRAVITY * BOX_MASS, 0.0));
        match clock {
            Some(clock) => self.phys.step_timed(dt, clock),
            None => self.phys.step(dt),
        }
        self.tick += 1;
        self.tally.add(&self.phys.contact_counters());
        self.count_handle();
        if self.tick.is_multiple_of(DROP_EVERY_TICKS) {
            let start = Transform::from_position(BOX_START);
            self.phys.set_transform(entity(BOX), start);
            if let Some(body) = self.phys.body_mut(entity(BOX)) {
                body.velocity = DVec3::ZERO;
                body.angular_velocity = DVec3::ZERO;
            }
            self.drops += 1;
        }
    }

    fn hash(&self, hasher: &mut dyn std::hash::Hasher) {
        self.phys.hash_state(hasher);
    }

    fn bodies(&self, out: &mut Vec<Shape>) {
        for (centre, rotation, half) in self.handle_parts() {
            out.push(Shape::Box {
                key: 0,
                centre,
                rotation,
                half,
                tint: Tint::Handle,
            });
        }
        let (centre, rotation) = self.box_pose();
        out.push(Shape::Box {
            key: 1,
            centre,
            rotation,
            half: DVec3::splat(BOX_HALF),
            tint: Tint::Box,
        });
    }

    fn fixtures(&self, _out: &mut Vec<Shape>) {}
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::scene::tests::tick_dt;

    fn run(ticks: u64) -> Spin {
        let mut spin = Spin::new();
        for _ in 0..ticks {
            spin.step(tick_dt(), None);
        }
        spin
    }

    /// The handle's parts sit about its centre of mass: weighted by volume,
    /// their offsets cancel.
    #[test]
    fn the_handle_is_placed_about_its_centre_of_mass() {
        let spin = Spin::new();
        let volume = |half: DVec3| half.x * half.y * half.z;
        let moment = spin.parts().iter().fold(DVec3::ZERO, |sum, part| {
            sum + part.centre * volume(part.half)
        });
        assert!(moment.length() < 1e-15, "the parts balance at {moment:?}");
        assert_ne!(
            spin.axes()[1],
            spin.axes()[0],
            "the intermediate axis is not distinct"
        );
    }

    /// **The page's claims for this room over ten seconds.** The handle has
    /// flipped with its momentum and energy intact, and the box has landed flat
    /// on the floor and rests there, sunk less than a millimetre and level.
    ///
    /// Measured on 2026-09-17 at tick 680, 200 ticks after the box's third
    /// drop: 2 flips, drift 5.7e-13 in momentum and 1.1e-12 in energy, the
    /// box's centre 0.29993 m up — sunk 0.069 mm, its weight on the contact
    /// springs — and its up axis tipped by 4.7e-13.
    #[test]
    fn the_handle_flips_and_the_box_lands_flat_and_rests() {
        let spin = run(DROP_EVERY_TICKS * 2 + 200);
        let reading = spin.reading();
        assert!(reading.flips >= 2, "only {} flips", reading.flips);
        assert!(
            reading.momentum_drift < 1e-10 && reading.energy_drift < 1e-10,
            "the handle drifted {:e} in momentum and {:e} in energy",
            reading.momentum_drift,
            reading.energy_drift
        );
        assert!(
            (reading.box_height - BOX_HALF).abs() < 1e-3,
            "the box rests at {} m, not on the floor",
            reading.box_height
        );
        assert!(
            reading.box_tilt < 1e-9,
            "the box tipped by {}",
            reading.box_tilt
        );
        assert_eq!(reading.drops, 3);
        assert!(reading.contacts.begun >= 3, "{:?}", reading.contacts);
    }

    /// Without the nudge the spin is balanced on its axis, and the handle keeps
    /// it: the flip is the instability growing, not something the page drives.
    #[test]
    fn a_handle_with_no_nudge_does_not_flip_within_ten_seconds() {
        let mut spin = Spin::new().without_nudge();
        for _ in 0..600 {
            spin.step(tick_dt(), None);
        }
        assert_eq!(spin.reading().flips, 0);
    }
}
