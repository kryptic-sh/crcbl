//! The isometric-ish camera — **the third rig on one character controller** —
//! and the one place a yaw becomes a world direction.
//!
//! ```text
//!   Q / E ──▶ Iso { yaw → target_yaw } ──▶ Camera        (what the frame is drawn from)
//!                        │
//!                        └── yaw ──▶ walk_direction ──▶ DVec3   (what the controller is asked for)
//! ```
//!
//! # Three rigs, one controller, and no line of `crcbl-phys` between them
//!
//! [`CharacterController::move_and_slide`](crcbl::phys::CharacterController::move_and_slide)
//! takes a **world-space displacement** and holds no camera, no view basis and
//! no yaw. Its own module docs argue why: turning a stick into a direction is
//! the one step that genuinely differs between rigs, so it belongs to whichever
//! rig is being built.
//!
//! `apps/puppet/src/camera.rs` drives that controller from a third-person orbit
//! that follows the character's back. `apps/breach/src/camera.rs` drives it from
//! inside the character's head, measuring its yaw the *other* way round —
//! [`Flyer`](crcbl::render::Flyer)'s convention rather than
//! [`OrbitCamera`]'s. **This module is the third**,
//! and it is the one whose camera the player barely controls at all: the pitch
//! is fixed, the distance is fixed, and the yaw moves only in quarter turns. The
//! controller gained nothing for any of the three, and the diff for this sample
//! is the proof of that.
//!
//! **What is genuinely this module's is the rig, not the trigonometry.**
//! [`walk_direction`] measures its yaw the way [`OrbitCamera`] does, which is
//! the way `apps/puppet` measures its yaw too — the same three lines, because
//! two rigs built on the same orbit basis have the same conversion and pretending
//! otherwise would be a third spelling of one fact. The claim this sample adds is
//! that the *controller* takes a rig it has never seen without changing, not
//! that the arithmetic is novel.
//!
//! # Isometric-**ish**, and the "ish" is a perspective divide
//!
//! `docs/plan/sample/15-shard.md` asks for a camera "isometric-ish but pulled
//! closer than the genre's convention, so lighting and material detail stay
//! legible". So:
//!
//! * [`ISO_PITCH`] is the true isometric elevation, `atan(1/√2)`, and the player
//!   cannot change it. A camera whose elevation drifts is not an isometric one.
//! * [`YAW_STEP`] is a quarter turn. The yaw is the one thing a player moves, and
//!   it moves between four bearings rather than freely, which is what keeps the
//!   zone's tiling reading as tiling.
//! * [`DISTANCE`] is nine metres, which is close. The genre's convention is two
//!   or three times that, and the whole reason this sample exists is that a
//!   torch's falloff, a pillar's shadow and a floor's reflection have to be
//!   legible in the frame.
//! * The projection is **perspective** rather than orthographic, at a narrow
//!   [`FOV_Y`] that flattens it into something axonometric. That is deliberate:
//!   the froxel grid the many-lights path clusters into and the ray march
//!   `docs/plan/18-render-features.md`'s screen-space reflections walk are both
//!   parametrised over a perspective divide, and a sample whose subject is those
//!   two would be exercising the unusual path if it drew them under an
//!   orthographic camera.
//!
//! # Which way is forward
//!
//! `crcbl` is right-handed with `+Y` up and `−Z` forward, and [`OrbitCamera`]
//! measures a yaw so that **zero puts the eye on `+Z` looking down `−Z`** — the
//! pose [`Camera::default`](crcbl::render::Camera) is in, and the direction
//! [`crate::zone`]'s layout runs away from the spawn.
//! `the_walk_is_where_the_camera_is_actually_looking` is what holds this
//! module's arithmetic to the matrix the frame is drawn with rather than to this
//! paragraph.

use crcbl::math::{DVec3, Vec3};
use crcbl::render::{Camera, OrbitCamera, Projection};

/// How far the eye sits from the character, in metres.
///
/// Nine, which is close for the genre — see the module docs.
pub const DISTANCE: f32 = 9.0;

/// How high above the character's **feet** the camera looks, in metres.
///
/// Chest height rather than the feet, so the floor in front of the character is
/// in frame rather than under the pivot.
pub const FOCUS_HEIGHT: f32 = 1.1;

/// The elevation the camera is locked at, in radians.
///
/// `atan(1/√2)`, the true isometric elevation: the angle at which the three axes
/// of a cube project to equal lengths. `the_pitch_is_the_isometric_elevation` is
/// what holds this literal to that definition rather than to this sentence.
pub const ISO_PITCH: f32 = 0.615_479_7;

/// The vertical field of view, in radians.
///
/// 32°, narrow enough that the frame reads as axonometric at [`DISTANCE`] and
/// wide enough that a room and the character in it are both in it.
pub const FOV_Y: f32 = 32.0 * (core::f32::consts::PI / 180.0);

/// The near plane, in metres.
pub const NEAR: f32 = 0.1;

/// How far one press of a rotate key turns the view, in radians.
pub const YAW_STEP: f32 = core::f32::consts::FRAC_PI_2;

/// How fast the view swings between two bearings, in radians a second.
///
/// A quarter turn takes a little under a second, which is fast enough not to be
/// waited on and slow enough that a player can see which way the zone turned.
pub const SWING_RATE: f32 = 1.9;

/// An isometric camera: a bearing, the bearing it is swinging toward, and
/// nothing else.
///
/// Where it stands is the character's, which is the shape every follow camera in
/// this repository has — see [`crate::camera`]'s module docs for what makes this
/// one a different rig from the other two.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct Iso {
    yaw: f32,
    target: f32,
}

impl Iso {
    /// Asks for `steps` quarter turns, positive anticlockwise about `+Y`.
    ///
    /// Recorded on the target rather than applied, because the swing is what the
    /// player sees: [`advance`](Self::advance) is what closes the gap.
    ///
    /// # Panics
    ///
    /// If the resulting bearing is not finite, which needs a step count no
    /// keyboard can produce. Asserted anyway: an angle that goes `NaN` here stays
    /// `NaN` for the rest of the session, it reaches
    /// [`walk_direction`] and puts the character somewhere nothing can recover
    /// from, and the panic would be several ticks from the input that caused it.
    pub fn rotate(&mut self, steps: i32) {
        #[allow(clippy::cast_precision_loss)]
        let asked = self.target + steps as f32 * YAW_STEP;
        assert!(asked.is_finite(), "a bearing must be finite, got {asked}");
        self.target = asked;
    }

    /// Swings the view `seconds` closer to the bearing it was last asked for.
    ///
    /// Runs on the **frame's** clock rather than the tick's, for the reason
    /// `apps/breach/src/app.rs` gives: the view is presentation, so it turns
    /// smoothly on a machine whose frames do not line up with its ticks, and a
    /// paused frame can still finish the swing it was in the middle of.
    ///
    /// # Panics
    ///
    /// If `seconds` is not finite, for [`rotate`](Self::rotate)'s reason.
    pub fn advance(&mut self, seconds: f32) {
        assert!(
            seconds.is_finite(),
            "a frame length must be finite, got {seconds}"
        );
        let gap = self.target - self.yaw;
        let step = SWING_RATE * seconds;
        self.yaw = if gap.abs() <= step {
            self.target
        } else {
            self.yaw + step.copysign(gap)
        };
    }

    /// The bearing the view is at — **what the walk direction is derived from**,
    /// and what the client puts on the wire.
    #[must_use]
    pub const fn yaw(&self) -> f32 {
        self.yaw
    }

    /// The bearing it is swinging toward, for the readout.
    #[must_use]
    pub const fn target(&self) -> f32 {
        self.target
    }

    /// Whether the view has arrived where it was last asked to go.
    #[must_use]
    pub fn settled(&self) -> bool {
        (self.target - self.yaw).abs() <= f32::EPSILON
    }

    /// The camera looking at `feet`, which is where the character is standing.
    #[must_use]
    pub fn camera(&self, feet: Vec3) -> Camera {
        let mut orbit = OrbitCamera::new(
            feet + Vec3::Y * FOCUS_HEIGHT,
            DISTANCE,
            Projection::Perspective {
                fov_y: FOV_Y,
                near: NEAR,
            },
        );
        orbit.orbit(self.yaw, ISO_PITCH);
        orbit.camera()
    }
}

/// The world-space direction a stick means, given the bearing the view is at.
///
/// `ahead` is positive away from the camera and `strafe` positive to the
/// camera's right; both are expected in `-1..=1`. The result is a **unit**
/// direction, or zero where nothing was asked for — the caller multiplies by a
/// speed and a timestep to get the displacement the controller takes.
///
/// [`OrbitCamera`]'s measure, which is `apps/puppet/src/camera.rs`'s too: see
/// this module's docs for why that is one fact spelled twice rather than a third
/// convention.
///
/// Normalised rather than passed through, so holding two keys does not walk `√2`
/// times faster than holding one — which is the oldest bug in this conversion.
#[must_use]
pub fn walk_direction(yaw: f64, ahead: f64, strafe: f64) -> DVec3 {
    let (sin, cos) = yaw.sin_cos();
    // The camera's own basis on the ground plane. `along` is where the view
    // looks with the vertical taken out; `right` is `along × up`, which is the
    // same cross product `OrbitCamera` builds its own right from.
    let along = DVec3::new(-sin, 0.0, -cos);
    let right = DVec3::new(cos, 0.0, -sin);
    (along * ahead + right * strafe).normalize_or_zero()
}

#[cfg(test)]
mod tests {
    use super::*;

    /// **The pitch is the isometric elevation** — the angle whose tangent is
    /// `1/√2` — rather than a number that looks about right.
    #[test]
    fn the_pitch_is_the_isometric_elevation() {
        let wanted = (1.0f32 / core::f32::consts::SQRT_2).atan();
        assert!(
            (ISO_PITCH - wanted).abs() < 1e-6,
            "the pitch is {ISO_PITCH} and the isometric elevation is {wanted}",
        );
        // And it is inside the bound `OrbitCamera` clamps to, so the rig is
        // never quietly flattened by the camera it is built on.
        const { assert!(ISO_PITCH < crcbl::render::orbit::PITCH_LIMIT) };
        const { assert!(ISO_PITCH > 0.0, "an isometric camera looks down") };
    }

    /// **The walk is where the camera is actually looking, with the vertical
    /// taken out**, and the strafe is the camera's own right.
    ///
    /// Asserted against the [`Camera`] the frame is drawn from rather than
    /// against a restatement of the same trigonometry: get one sign wrong and
    /// the character walks toward the camera on every bearing, and every other
    /// test here passes through it.
    #[test]
    fn the_walk_is_where_the_camera_is_actually_looking() {
        for sixteenth in 0..16 {
            #[allow(clippy::cast_precision_loss)]
            let yaw = core::f32::consts::TAU * sixteenth as f32 / 16.0 - core::f32::consts::PI;
            let mut iso = Iso::default();
            iso.rotate(0);
            iso.target = yaw;
            iso.advance(f32::MAX / SWING_RATE);
            assert!((iso.yaw() - yaw).abs() < 1e-5);

            let camera = iso.camera(Vec3::ZERO);
            let view = camera.target - camera.eye;
            let flat = Vec3::new(view.x, 0.0, view.z).normalize();

            let walk = walk_direction(f64::from(iso.yaw()), 1.0, 0.0);
            assert_eq!(walk.y, 0.0, "the walk left the ground");
            assert!(
                (walk.x - f64::from(flat.x)).abs() < 1e-5
                    && (walk.z - f64::from(flat.z)).abs() < 1e-5,
                "at yaw {yaw}: the camera looks along {flat}, forward walks {walk}",
            );

            // And the strafe is the character's own right: a quarter turn from
            // ahead, in the sense that leaves the pair right-handed about +Y.
            let strafe = walk_direction(f64::from(iso.yaw()), 0.0, 1.0);
            let cross = walk.cross(strafe);
            assert!(
                cross.y < -0.99,
                "at yaw {yaw}: ahead {walk} and strafe {strafe} are not a right-handed pair",
            );
        }
    }

    /// A bearing of zero walks down `−Z`, which is where [`crate::zone`]'s zone
    /// reaches away from the spawn.
    ///
    /// The one test here written against a literal axis rather than against the
    /// camera, because "the zone runs away from the entrance" is a fact about
    /// the layout that the layout's own constants are written to.
    #[test]
    fn a_zero_bearing_walks_into_the_zone() {
        let ahead = walk_direction(0.0, 1.0, 0.0);
        assert!((ahead - DVec3::new(0.0, 0.0, -1.0)).length() < 1e-12);
        let right = walk_direction(0.0, 0.0, 1.0);
        assert!((right - DVec3::new(1.0, 0.0, 0.0)).length() < 1e-12);
    }

    /// Holding two keys walks at one speed, not at `√2` of it.
    #[test]
    fn a_diagonal_is_not_faster_than_a_straight_line() {
        let diagonal = walk_direction(0.7, 1.0, 1.0);
        assert!((diagonal.length() - 1.0).abs() < 1e-12);
        assert_eq!(walk_direction(0.7, 0.0, 0.0), DVec3::ZERO);
    }

    /// **A rotate key swings the view a quarter turn and stops there**, which is
    /// the whole of what makes this rig isometric rather than an orbit the
    /// player flies.
    ///
    /// The stop is the control: a rig that kept turning for as long as the key
    /// was held would pass "the yaw moved" and fail this.
    #[test]
    fn a_rotate_key_swings_one_quarter_turn_and_settles() {
        let mut iso = Iso::default();
        assert!(iso.settled());
        iso.rotate(1);
        assert!(!iso.settled(), "the swing finished before it started");
        assert_eq!(iso.target(), YAW_STEP);

        // Frames of a sixtieth, which is what a browser at sixty is handing it.
        let mut frames = 0;
        while !iso.settled() && frames < 1000 {
            iso.advance(1.0 / 60.0);
            frames += 1;
            assert!(
                iso.yaw() <= YAW_STEP + 1e-6,
                "the swing overshot to {}",
                iso.yaw(),
            );
        }
        assert!(iso.settled(), "it never arrived: {}", iso.yaw());
        assert!(
            (iso.yaw() - YAW_STEP).abs() < 1e-6,
            "it settled at {}",
            iso.yaw(),
        );
        // …and it stays there with nothing asked of it.
        for _ in 0..60 {
            iso.advance(1.0 / 60.0);
        }
        assert_eq!(iso.yaw(), YAW_STEP);

        // The other way round, and past zero, which is what says the swing
        // follows the sign of the gap rather than one direction.
        iso.rotate(-2);
        assert_eq!(iso.target(), -YAW_STEP);
        for _ in 0..1000 {
            iso.advance(1.0 / 60.0);
        }
        assert_eq!(iso.yaw(), -YAW_STEP);
    }

    /// **The pitch is not something a player can move**, which is the property
    /// the whole rig is named after: whatever the bearing, the eye is the same
    /// height above the pivot and the same distance from it.
    #[test]
    fn the_eye_keeps_its_elevation_whatever_the_bearing_is() {
        let feet = Vec3::new(3.0, 0.0, -4.0);
        let focus = feet + Vec3::Y * FOCUS_HEIGHT;
        let mut heights = Vec::new();
        for step in -4..=4 {
            let mut iso = Iso::default();
            iso.rotate(step);
            for _ in 0..1000 {
                iso.advance(1.0 / 60.0);
            }
            let camera = iso.camera(feet);
            assert!(
                (camera.eye.distance(focus) - DISTANCE).abs() < 1e-4,
                "at step {step} the eye is {:.3} m out",
                camera.eye.distance(focus),
            );
            assert!(
                camera.eye.y > focus.y,
                "at step {step} the camera is under the character",
            );
            heights.push(camera.eye.y);
        }
        let first = heights[0];
        for height in &heights {
            assert!(
                (height - first).abs() < 1e-4,
                "the eye's height moved between bearings: {heights:?}",
            );
        }
    }
}
