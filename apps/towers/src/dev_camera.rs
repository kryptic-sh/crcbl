//! The dev camera: the fixed overhead view, or a free **fly** camera, or a
//! **walk** camera on a capsule [`crcbl::phys::CharacterController`] drives
//! over the field — `docs/plan/sample/07-towers.md`'s slice 4, and the
//! controller's first real terrain.
//!
//! ```text
//!   C ──▶ Mode: Overhead ──▶ Fly ──▶ Walk ──▶ Overhead
//!
//!   WASD, Space/Shift, arrows ──▶ Steer ──▶ yaw, pitch ─▶ Camera
//!                                    │
//!                     Fly:  eye += step through everything
//!                     Walk: OrbitCamera::walk_direction ──▶ Walker::step
//! ```
//!
//! # It is the client's, and the simulation never sees it
//!
//! Nothing here holds a [`crate::game::Game`] or writes a
//! [`crate::game::Controls`]: the camera's keys reach the action map and stop
//! there, and the walker moves in a physics world of its own — see
//! [`walker`]'s docs for why it cannot share the stage's. So the stage, its
//! physics world and its state hash are the same tick for tick whatever the
//! camera does, which `crate::app`'s
//! `walking_the_dev_camera_leaves_the_stage_hash_alone` holds, and in a LAN
//! session a joiner walking its field sends its host nothing it would not
//! have sent anyway.
//!
//! # Its keys are a context of their own
//!
//! The toggle is in [`crcbl::input::GLOBAL_CONTEXT`], so it works over every
//! screen; the camera's movement keys are [`CONTEXT`], pushed while the camera
//! is anything but overhead. A pushed context takes the keys it binds from
//! the field's — `S` is a step back rather than a save, the arrows turn rather
//! than walk the build cursor — and lets the rest fall through, so `B`, `U`,
//! `N` and the digits still play the field from the walk camera. Both are in
//! the one [`crcbl::input::ActionMap`] towers plays on, so the console's
//! `bind` moves them.
//!
//! # Which way is forward
//!
//! The yaw is measured as [`OrbitCamera`] measures it — zero looks down `-Z`
//! and a rising yaw swings the view toward `-X` — because
//! [`OrbitCamera::walk_direction`] is the conversion the walk takes, as
//! `apps/puppet` and `apps/shard` take it.
//! `tests::the_walk_is_where_the_camera_looks` holds the two together.

use crcbl::math::Vec3;
use crcbl::render::orbit::PITCH_LIMIT;
use crcbl::render::{Camera, OrbitCamera, Projection};

pub mod walker;

use walker::Walker;

use crate::map::{MAX_PLOTS, Map};
use crate::tower::TowerView;

/// The action map context the camera's movement keys are in, pushed while the
/// camera is not the overhead one.
pub const CONTEXT: &str = "dev-camera";

/// How fast the fly camera flies, in metres a second: across the field's
/// width in three seconds.
pub const FLY_SPEED: f32 = 12.0;

/// How fast the arrows turn either camera, in radians a second — the engine
/// free camera's, because an angle is an angle.
pub const TURN_RATE: f32 = crcbl::render::TURN;

/// The near plane of either moving camera, in metres. The overhead camera's
/// is thirty-odd metres from anything; a walker's eye comes within arm's
/// reach of a tower.
pub const NEAR: f32 = 0.05;

/// Which camera the frame is drawn from.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum Mode {
    /// The fixed view of the whole field — [`crate::camera`].
    #[default]
    Overhead,
    /// A free camera that passes through everything.
    Fly,
    /// The walker's eye.
    Walk,
}

impl Mode {
    /// The mode the toggle goes to next.
    #[must_use]
    pub const fn next(self) -> Self {
        match self {
            Self::Overhead => Self::Fly,
            Self::Fly => Self::Walk,
            Self::Walk => Self::Overhead,
        }
    }

    /// How the debug panel names it.
    #[must_use]
    pub const fn label(self) -> &'static str {
        match self {
            Self::Overhead => "overhead",
            Self::Fly => "fly",
            Self::Walk => "walk",
        }
    }
}

/// One tick's worth of the camera's keys, each in `-1..=1`.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct Steer {
    /// Positive away from the eye.
    pub ahead: f32,
    /// Positive to the eye's right.
    pub strafe: f32,
    /// Positive up. The fly camera's alone: a walker has no jump.
    pub rise: f32,
    /// Positive turns the view right.
    pub turn: f32,
    /// Positive tilts it up.
    pub tilt: f32,
}

/// The dev camera — see the module docs.
#[derive(Debug)]
pub struct DevCamera {
    mode: Mode,
    /// Where the fly camera is.
    eye: Vec3,
    /// [`OrbitCamera`]'s measure — see the module docs.
    yaw: f32,
    /// Positive up, held inside [`PITCH_LIMIT`].
    pitch: f32,
    walker: Walker,
}

impl DevCamera {
    /// The overhead camera over `map`, with the fly camera waiting at its pose.
    #[must_use]
    pub fn new(map: &Map) -> Self {
        let (eye, yaw, pitch) = overhead_pose();
        Self {
            mode: Mode::Overhead,
            eye,
            yaw,
            pitch,
            walker: Walker::new(map),
        }
    }

    /// Which camera the frame is drawn from.
    #[must_use]
    pub const fn mode(&self) -> Mode {
        self.mode
    }

    /// The walker, wherever it was last left.
    #[must_use]
    pub const fn walker(&self) -> &Walker {
        &self.walker
    }

    /// Goes to the next mode and answers it.
    ///
    /// Into the walk, the walker is dropped from under the fly camera's eye, so
    /// flying to a place and switching is how a walk starts there. Back to the
    /// overhead camera, the fly camera is put back at its pose, so the next fly
    /// starts from the view a player was just looking at.
    pub fn cycle(&mut self) -> Mode {
        self.mode = self.mode.next();
        match self.mode {
            Mode::Walk => self.walker.drop_from(self.eye.as_dvec3()),
            Mode::Overhead => (self.eye, self.yaw, self.pitch) = overhead_pose(),
            Mode::Fly => {}
        }
        self.mode
    }

    /// Moves the camera by `steer` for `dt` seconds: turns either moving
    /// camera, flies the fly camera through everything, and walks the walker
    /// among `towers` — what the frame draws on each plot, which the walker's
    /// world is brought in step with first ([`Walker::sync_towers`]). The
    /// overhead camera does not move.
    pub fn step(&mut self, steer: Steer, dt: f64, towers: &[Option<TowerView>; MAX_PLOTS]) {
        if self.mode == Mode::Overhead {
            return;
        }
        // The camera's own rate is an `f32`, as `Flyer`'s is.
        #[allow(clippy::cast_possible_truncation)]
        let seconds = dt as f32;
        // A rising yaw swings the view left, so a right turn lowers it.
        self.yaw -= steer.turn * TURN_RATE * seconds;
        self.pitch =
            (self.pitch + steer.tilt * TURN_RATE * seconds).clamp(-PITCH_LIMIT, PITCH_LIMIT);

        let flat = OrbitCamera::walk_direction(
            f64::from(self.yaw),
            f64::from(steer.ahead),
            f64::from(steer.strafe),
        );
        match self.mode {
            Mode::Fly => {
                let step = (flat.as_vec3() + Vec3::Y * steer.rise).normalize_or_zero();
                self.eye += step * FLY_SPEED * seconds;
            }
            Mode::Walk => {
                self.walker.sync_towers(towers);
                self.walker.step(flat, dt);
            }
            Mode::Overhead => {}
        }
    }

    /// The camera the frame is drawn from: [`crate::camera::camera`] itself
    /// overhead, and otherwise the fly camera's eye or the walker's, looking
    /// along the yaw and the pitch.
    #[must_use]
    pub fn camera(&self) -> Camera {
        let eye = match self.mode {
            Mode::Overhead => return crate::camera::camera(),
            Mode::Fly => self.eye,
            Mode::Walk => self.walker.eye().as_vec3(),
        };
        Camera {
            eye,
            target: eye + look(self.yaw, self.pitch),
            up: Vec3::Y,
            projection: Projection::Perspective {
                fov_y: crate::camera::FOV_Y,
                near: NEAR,
            },
        }
    }
}

/// The overhead camera's eye, yaw and pitch, which is where the fly camera
/// starts.
fn overhead_pose() -> (Vec3, f32, f32) {
    let overhead = crate::camera::camera();
    let forward = (overhead.target - overhead.eye).normalize();
    (
        overhead.eye,
        // `atan2(-x, -z)`: zero looks down `-Z`, and a view toward `-X` is a
        // positive yaw — `OrbitCamera`'s measure.
        (-forward.x).atan2(-forward.z),
        forward.y.asin().clamp(-PITCH_LIMIT, PITCH_LIMIT),
    )
}

/// The unit vector a view at `yaw` and `pitch` looks along — on the ground
/// plane, [`OrbitCamera::walk_direction`]'s "ahead".
fn look(yaw: f32, pitch: f32) -> Vec3 {
    let (sin_yaw, cos_yaw) = yaw.sin_cos();
    let (sin_pitch, cos_pitch) = pitch.sin_cos();
    Vec3::new(-sin_yaw * cos_pitch, sin_pitch, -cos_yaw * cos_pitch)
}

/// The camera's mode and, while walking, what the walker stands on and what
/// its last move met — [`crcbl::phys::CharacterController::move_and_slide_into`]'s
/// contacts, one row each.
impl crcbl::ui::DebugModule for DevCamera {
    fn debug_section(&self, section: &mut crcbl::ui::DebugSection) {
        section.set_title("camera");
        section.row_str("mode", self.mode.label());
        if self.mode == Mode::Overhead {
            return;
        }
        let eye = self.camera().eye;
        section.row(
            "eye",
            format_args!("{:.1} {:.1} {:.1}", eye.x, eye.y, eye.z),
        );
        if self.mode != Mode::Walk {
            return;
        }
        let walker = &self.walker;
        match walker.ground() {
            Some(part) => section.row("grounded", format_args!("on the {}", part.label())),
            // A tower taken down under the walker's feet: the last move's
            // ground is a collider this world no longer holds.
            None if walker.is_grounded() => section.row_str("grounded", "yes"),
            None => section.row_str("grounded", "no"),
        }
        let last = walker.last();
        section.row(
            "moved",
            format_args!(
                "{:.3} m{}{}",
                last.motion.length(),
                if last.stepped_up { ", stepped up" } else { "" },
                if last.hit_wall { ", hit a wall" } else { "" },
            ),
        );
        section.row("contacts", format_args!("{}", walker.contacts().len()));
        for contact in walker.contacts() {
            let what = walker
                .part_of(contact.collider)
                .map_or("?", walker::Part::label);
            section.row(
                what,
                format_args!(
                    "n {:.2} {:.2} {:.2} at {:.2}{}",
                    contact.normal.x,
                    contact.normal.y,
                    contact.normal.z,
                    contact.fraction,
                    if contact.stepped_up { " step" } else { "" },
                ),
            );
        }
    }
}

#[cfg(test)]
mod tests;
