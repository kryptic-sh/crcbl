//! The third-person camera, and **the one place a yaw becomes a world
//! direction**.
//!
//! ```text
//!   keys ──▶ Follow { yaw, pitch } ──▶ Camera        (what the frame is drawn from)
//!                    │
//!                    └── yaw ──▶ walk_direction ──▶ DVec3   (what the controller is asked for)
//! ```
//!
//! # Why the conversion is here and not in `crcbl-phys`
//!
//! [`CharacterController::move_and_slide`](crcbl::phys::CharacterController::move_and_slide)
//! takes a **world-space displacement** and holds no camera, no view basis and
//! no yaw. That is deliberate, and its module docs say why: turning a stick into
//! a direction is the only step that genuinely differs between a first-person
//! rig and a third-person one, so it belongs to whichever of the two is
//! actually being built. This sample is the third-person one, and this function
//! is its answer.
//!
//! **Facing is the demo's too.** There is no orientation on the controller;
//! [`crate::game`] turns the body toward
//! [`MoveOutcome::motion`](crcbl::phys::MoveOutcome::motion) over time, and the
//! camera never has to fight it because the camera's yaw and the body's yaw are
//! two different numbers that happen to be measured the same way.
//!
//! # The geometry is the engine's
//!
//! [`OrbitCamera`] already turns a pivot, a distance and two angles into a
//! [`Camera`], with the pitch clamp that keeps the view matrix out of its
//! degenerate pose. This module holds the two angles and rebuilds one of those
//! per frame rather than keeping one, because a follow camera's pivot moves
//! every frame and there is nothing left in it to preserve between two.
//!
//! # Which way is forward
//!
//! `crcbl` is right-handed with `+Y` up and `-Z` forward, and
//! [`OrbitCamera`] measures a yaw so that **zero puts the eye on `+Z` looking
//! down `-Z`** — the pose [`Camera::default`](crcbl::render::Camera) is in. So a
//! yaw of zero walks the character down `-Z`. This module's
//! `the_walk_direction_is_where_the_camera_is_actually_looking` test is what
//! holds the arithmetic to the matrix the frame is drawn with rather than to a
//! comment.

use crcbl::math::{DVec3, Vec3};
use crcbl::render::orbit::PITCH_LIMIT;
use crcbl::render::{Camera, OrbitCamera, Projection};

/// How far behind the character the eye sits, in metres.
///
/// Far enough that the whole 2 m body and the ground in front of it are in
/// frame, close enough that the [`crate::map`] lane's steps are read as heights
/// rather than as lines.
pub const DISTANCE: f32 = 6.0;

/// How far above the character's **feet** the camera looks, in metres.
///
/// Chest height rather than the feet, so the horizon sits behind the body
/// instead of under it.
pub const FOCUS_HEIGHT: f32 = 1.2;

/// The vertical field of view, in radians.
pub const FOV_Y: f32 = core::f32::consts::FRAC_PI_4;

/// The near plane, in metres. Short, because the eye is only [`DISTANCE`] from
/// what it is looking at and a mound can come between them.
pub const NEAR: f32 = 0.05;

/// The elevation the camera opens at, in radians — looking slightly down on the
/// character, which is where a third-person camera starts.
pub const START_PITCH: f32 = 0.28;

/// How fast a held camera key turns the view, in radians a second.
pub const TURN_RATE: f32 = 1.8;

/// A third-person camera: two angles and a pivot it is handed every frame.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Follow {
    yaw: f32,
    pitch: f32,
}

impl Default for Follow {
    /// Behind the character, looking slightly down at it.
    fn default() -> Self {
        Self {
            yaw: 0.0,
            pitch: START_PITCH,
        }
    }
}

impl Follow {
    /// Turns the view by `yaw` and `pitch` radians.
    ///
    /// The pitch is clamped to [`PITCH_LIMIT`], which is the same bound
    /// [`OrbitCamera::orbit`] would apply — applied here as well because this
    /// module keeps the angle and hands the whole of it to a fresh controller
    /// each frame, so an unclamped one would accumulate past vertical and be
    /// clamped back on every use while going on growing.
    ///
    /// The yaw is left to run, and wrapping it would change nothing a sine or a
    /// cosine can see.
    ///
    /// # Panics
    ///
    /// If either delta is not finite, on
    /// [`OrbitCamera::orbit`](crcbl::render::OrbitCamera::orbit)'s terms: an
    /// angle that goes `NaN` here stays `NaN` for the rest of the session, and
    /// the panic it causes is several frames from the input that caused it.
    pub fn turn(&mut self, yaw: f32, pitch: f32) {
        assert!(
            yaw.is_finite() && pitch.is_finite(),
            "camera deltas must be finite, got yaw {yaw} and pitch {pitch}"
        );
        self.yaw += yaw;
        self.pitch = (self.pitch + pitch).clamp(-PITCH_LIMIT, PITCH_LIMIT);
    }

    /// The azimuth the view is at — **what the walk direction is derived
    /// from**, and what the client puts on the wire.
    #[must_use]
    pub const fn yaw(&self) -> f32 {
        self.yaw
    }

    /// The camera looking at `focus`, which is a point in world space.
    #[must_use]
    pub fn camera(&self, focus: Vec3) -> Camera {
        let mut orbit = OrbitCamera::new(
            focus,
            DISTANCE,
            Projection::Perspective {
                fov_y: FOV_Y,
                near: NEAR,
            },
        );
        orbit.orbit(self.yaw, self.pitch);
        orbit.camera()
    }
}

/// The world-space direction a stick means, given the yaw the view is at.
///
/// `forward` is positive away from the camera and `strafe` positive to the
/// camera's right; both are expected in `-1..=1`. The result is a **unit**
/// direction, or zero where nothing was asked for — the caller multiplies by a
/// speed and a timestep to get the displacement the controller takes.
///
/// Normalised rather than passed through, so holding two keys does not walk
/// `√2` times faster than holding one — which is the oldest bug in this
/// conversion.
#[must_use]
pub fn walk_direction(yaw: f64, forward: f64, strafe: f64) -> DVec3 {
    let (sin, cos) = yaw.sin_cos();
    // The camera's own basis on the ground plane. `ahead` is where the view
    // looks with the vertical taken out; `right` is `ahead × up`, which is the
    // same cross product `OrbitCamera` builds its own right from.
    let ahead = DVec3::new(-sin, 0.0, -cos);
    let right = DVec3::new(cos, 0.0, -sin);
    (ahead * forward + right * strafe).normalize_or_zero()
}

/// The yaw a body facing `direction` is at, in the same measure
/// [`Follow::yaw`] is in.
///
/// The inverse of the `ahead` vector in [`walk_direction`]: zero faces `-Z`.
/// Returns `None` for a direction with no horizontal part, where there is no
/// facing to read.
#[must_use]
pub fn facing_of(direction: DVec3) -> Option<f64> {
    let flat = DVec3::new(direction.x, 0.0, direction.z);
    if flat.length_squared() <= 0.0 {
        return None;
    }
    Some((-flat.x).atan2(-flat.z))
}

#[cfg(test)]
mod tests {
    use super::*;

    /// **The direction the character walks is the direction the camera is
    /// pointing**, checked against the [`Camera`] the frame is actually drawn
    /// from rather than against a restatement of the same trigonometry.
    ///
    /// This is the assertion the whole module exists for: get the sign of
    /// either term wrong and the demo walks sideways or backwards, which every
    /// other test here would pass through.
    #[test]
    fn the_walk_direction_is_where_the_camera_is_actually_looking() {
        for sixteenth in 0..16 {
            let yaw = core::f32::consts::TAU * sixteenth as f32 / 16.0;
            let mut follow = Follow::default();
            follow.turn(yaw, 0.0);
            let camera = follow.camera(Vec3::new(1.0, 2.0, 3.0));
            let view = camera.target - camera.eye;
            let ahead = Vec3::new(view.x, 0.0, view.z).normalize();

            let walk = walk_direction(f64::from(follow.yaw()), 1.0, 0.0);
            assert!(
                (walk.x - f64::from(ahead.x)).abs() < 1e-5
                    && (walk.z - f64::from(ahead.z)).abs() < 1e-5,
                "at yaw {yaw}: the camera looks along {ahead}, forward walks {walk}",
            );

            // And the strafe is the camera's own right: a quarter turn from
            // ahead, in the sense that leaves the pair right-handed about +Y.
            let strafe = walk_direction(f64::from(follow.yaw()), 0.0, 1.0);
            let cross = walk.cross(strafe);
            assert!(
                cross.y < -0.99,
                "at yaw {yaw}: forward {walk} and strafe {strafe} are not a right-handed pair",
            );
        }
    }

    /// Holding two keys walks at one speed, not at `√2` of it.
    #[test]
    fn a_diagonal_is_not_faster_than_a_straight_line() {
        let diagonal = walk_direction(0.7, 1.0, 1.0);
        assert!((diagonal.length() - 1.0).abs() < 1e-12);
        assert_eq!(walk_direction(0.7, 0.0, 0.0), DVec3::ZERO);
    }

    /// [`facing_of`] reads back the yaw [`walk_direction`] was given.
    #[test]
    fn a_facing_read_back_is_the_yaw_it_was_walked_at() {
        for sixteenth in 0..16 {
            let yaw = core::f64::consts::TAU * f64::from(sixteenth) / 16.0 - core::f64::consts::PI;
            let facing = facing_of(walk_direction(yaw, 1.0, 0.0)).expect("a forward walk has one");
            // Compared as directions rather than as angles: the two are equal
            // modulo a full turn, and the wrap is not what this is about.
            let (want, got) = (
                walk_direction(yaw, 1.0, 0.0),
                walk_direction(facing, 1.0, 0.0),
            );
            assert!((want - got).length() < 1e-12, "{yaw} came back as {facing}");
        }
        assert_eq!(facing_of(DVec3::Y), None);
    }

    /// The pitch cannot accumulate past the bound the view matrix needs, however
    /// long a key is held.
    #[test]
    fn the_pitch_stops_short_of_vertical_however_far_it_is_pushed() {
        let mut follow = Follow::default();
        for _ in 0..1000 {
            follow.turn(0.0, 1.0);
        }
        assert!((follow.pitch - PITCH_LIMIT).abs() < 1e-6);
        for _ in 0..2000 {
            follow.turn(0.0, -1.0);
        }
        assert!((follow.pitch + PITCH_LIMIT).abs() < 1e-6);
        // And the camera it builds is still a camera: `Camera::view` panics on
        // an eye directly above its target.
        let _ = follow.camera(Vec3::ZERO).view();
    }
}
