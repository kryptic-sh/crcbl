//! The orbit camera: turning drags into a [`Camera`], and nothing else.
//!
//! ```text
//! orbit(dyaw, dpitch) ─┐
//! pan(right, up) ──────┼─▶ OrbitCamera { pivot, distance, yaw, pitch } ─▶ Camera
//! zoom(delta) ─────────┤
//! frame(bounds, aspect)┘
//! ```
//!
//! `docs/plan/sample/05-viewer.md`'s orbit controls and the stage-8 editor's
//! viewport are the same controller, which is why it is here beside [`Camera`]
//! rather than in an app: it is arithmetic, it needs no device, and every one of
//! its claims is a unit test.
//!
//! # It takes deltas, never keys
//!
//! Nothing in this module names a key, a mouse button or a modifier. Which
//! gesture produces a yaw delta is the *app's* decision and the DCC tools
//! disagree about it — orbit-follows-mouse and object-follows-mouse are opposite
//! signs for the same drag, and an editor whose users came from a different tool
//! will want the other one. A controller that read the mouse itself could not be
//! reused by an app whose input model is not the sample's.
//!
//! So this module fixes the *geometry* and leaves the *gesture* alone. The
//! conventions it does fix are:
//!
//! * **A positive yaw delta swings the eye toward the camera's own right**, and
//!   the view therefore turns left. That is the same rotation sense as
//!   [`crate::fly::Flyer`] — a rising yaw is a rotation about `+Y` that carries
//!   `+Z` toward `+X` — so the two cameras in this engine agree about which way
//!   an angle goes, and an app that wants the object to follow the pointer
//!   negates the delta on its way in.
//! * **A positive pitch delta lifts the eye**, so the camera looks *down* on the
//!   pivot. `pitch` here is the eye's elevation above the pivot's horizontal
//!   plane, not the view's inclination; they are the same angle with opposite
//!   signs, and this one is the one a turntable is usually written in.
//! * **Pan is measured in fractions of the viewport height**, not in metres or
//!   pixels. A drag of half the window's height moves the pivot by half of what
//!   the window shows at the pivot's depth, so the scene keeps up with the
//!   pointer at any zoom and on any window size. Doing that conversion needs the
//!   field of view and the distance, which is exactly the arithmetic that would
//!   otherwise be copied into every app.
//!
//! # Perspective only, and the type says so
//!
//! [`OrbitCamera::new`] takes a [`Projection`] and refuses an orthographic one,
//! keeping the field of view and near plane rather than the enum. Zoom under an
//! orthographic projection is a change of `half_height` and not of distance, so
//! a controller that accepted one would have a zoom that moved the eye and
//! changed nothing on screen — "not supported here" arriving as "worked".
//! `docs/backlog.md` carries the orthographic mode as the thing that would
//! change this.

use core::f32::consts::FRAC_PI_2;

use glam::Vec3;

use crate::camera::{Camera, Projection};
use crate::cull::Aabb;

/// How far the eye may rise or fall from the pivot's horizontal plane, in
/// radians.
///
/// Just short of straight up and straight down, and [`crate::fly::Flyer`]'s own
/// pitch limit is the precedent. At *exactly* vertical the direction from pivot
/// to eye is parallel to `Vec3::Y` and [`Camera::view`] panics rather than hand
/// back a matrix of `NaN`s — so an unclamped turntable does not wobble, it takes
/// the process down.
pub const PITCH_LIMIT: f32 = FRAC_PI_2 - 0.05;

/// The closest the eye may come to the pivot, in metres.
///
/// A geometric guard, **not** the near plane: zooming until the pivot is inside
/// the near plane is ordinary DCC behaviour and this does not prevent it. What
/// it prevents is distance reaching zero, where `eye == target` and
/// [`Camera::view`] panics, and negative distance, where the camera passes
/// through the pivot and comes out looking at it backwards.
pub const MIN_DISTANCE: f32 = 0.01;

/// The furthest the eye may retreat, in metres — a thousand kilometres.
///
/// [`OrbitCamera::zoom`] is multiplicative, so without an upper bound a user
/// leaning on the wheel reaches `inf` in a few hundred notches and every
/// subsequent frame is `NaN`.
pub const MAX_DISTANCE: f32 = 1.0e6;

/// How much larger than the framed bounds the view is, as a ratio.
///
/// [`OrbitCamera::frame`] fits the bounding sphere and then backs off by this,
/// so a framed object has a little air around it instead of touching all four
/// edges.
pub const FRAME_MARGIN: f32 = 1.1;

/// A camera that turns about a pivot: orbit, pan, zoom, frame.
///
/// Holds a pivot, a distance and two angles rather than a [`Camera`], for the
/// same reason [`crate::fly::Flyer`] holds angles: an eye and a target
/// integrated directly drift apart, and "orbit" would depend on how far away the
/// target happened to be.
///
/// ```
/// use crcbl_render::{Camera, OrbitCamera, Projection};
/// use glam::Vec3;
///
/// let mut orbit = OrbitCamera::new(Vec3::ZERO, 4.0, Projection::default());
/// // Zero yaw stands where the engine's default camera stands: back along +Z.
/// assert_eq!(orbit.camera().eye, Vec3::new(0.0, 0.0, 4.0));
///
/// // A quarter turn to the eye's right puts it on +X, still looking at the pivot.
/// orbit.orbit(core::f32::consts::FRAC_PI_2, 0.0);
/// let camera = orbit.camera();
/// assert!((camera.eye - Vec3::new(4.0, 0.0, 0.0)).length() < 1e-5);
/// assert_eq!(camera.target, Vec3::ZERO);
/// ```
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct OrbitCamera {
    /// What the camera looks at and turns around.
    pivot: Vec3,
    /// How far the eye is from the pivot, in metres.
    distance: f32,
    /// Azimuth about `+Y`, measured so that `0` puts the eye on `+Z`.
    yaw: f32,
    /// Elevation above the pivot's horizontal plane, positive up.
    pitch: f32,
    /// Vertical field of view, in radians. Kept rather than the [`Projection`]
    /// because this controller is perspective-only and both [`Self::pan`] and
    /// [`Self::frame`] need the angle itself.
    fov_y: f32,
    /// Near plane distance, carried so [`Self::camera`] can rebuild the
    /// projection it was given.
    near: f32,
}

impl OrbitCamera {
    /// A camera `distance` metres from `pivot`, looking at it from `+Z`.
    ///
    /// Zero yaw and zero pitch put the eye where [`Camera::default`] puts it —
    /// back along `+Z`, looking down `-Z` — so an orbit camera nobody has
    /// dragged yet frames the world the same way the rest of the engine does.
    ///
    /// # Panics
    ///
    /// If `projection` is [`Projection::Orthographic`], which this controller
    /// does not support (see the [module docs](self)), if `distance` is not
    /// finite, or on the same field of view and near plane
    /// [`Projection::matrix`] refuses — each of which otherwise turns into a
    /// `NaN` matrix and a blank screen with no error anywhere.
    #[must_use]
    pub fn new(pivot: Vec3, distance: f32, projection: Projection) -> Self {
        let Projection::Perspective { fov_y, near } = projection else {
            panic!("an orbit camera is perspective-only, got {projection:?}");
        };
        assert!(
            distance.is_finite(),
            "the orbit distance must be finite, got {distance}"
        );
        assert!(near > 0.0, "the near plane must be positive, got {near}");
        assert!(
            fov_y > 0.0 && fov_y < core::f32::consts::PI,
            "the field of view must be in (0, π), got {fov_y}"
        );
        Self {
            pivot,
            distance: distance.clamp(MIN_DISTANCE, MAX_DISTANCE),
            yaw: 0.0,
            pitch: 0.0,
            fov_y,
            near,
        }
    }

    /// What the camera turns around and looks at.
    #[must_use]
    pub const fn pivot(&self) -> Vec3 {
        self.pivot
    }

    /// How far the eye is from the pivot, in metres.
    #[must_use]
    pub const fn distance(&self) -> f32 {
        self.distance
    }

    /// Turns the eye about the pivot by `yaw` and `pitch` radians.
    ///
    /// The pivot does not move and the distance does not change; this is the
    /// operation that makes it a turntable rather than a look-around. Positive
    /// `yaw` swings the eye toward the camera's own right, positive `pitch`
    /// lifts it — see the [module docs](self) — and the pitch is clamped to
    /// [`PITCH_LIMIT`] short of vertical, where the view matrix is degenerate.
    ///
    /// # Panics
    ///
    /// If either delta is not finite. An angle that goes `NaN` here stays `NaN`
    /// for the rest of the session, and the panic it eventually causes is in
    /// [`Camera::view`], several frames from the input that caused it.
    pub fn orbit(&mut self, yaw: f32, pitch: f32) {
        assert!(
            yaw.is_finite() && pitch.is_finite(),
            "orbit deltas must be finite, got yaw {yaw} and pitch {pitch}"
        );
        self.yaw += yaw;
        self.pitch = (self.pitch + pitch).clamp(-PITCH_LIMIT, PITCH_LIMIT);
    }

    /// Slides the pivot across the view plane, in fractions of the viewport
    /// height.
    ///
    /// Positive `right` moves the pivot toward the camera's own right and
    /// positive `up` toward its own up, so the scene appears to move the *other*
    /// way — an app implementing grab-and-drag negates both. The eye follows
    /// rigidly: panning changes what the camera looks at, never how far away it
    /// is or which way it faces.
    ///
    /// One unit is the whole viewport height **at the pivot's depth**, which is
    /// what makes a drag track the pointer: the point under the cursor when the
    /// drag began is still under it when the drag ends, as long as that point
    /// was near the pivot's plane. Points nearer or further than the pivot move
    /// by more or less, which is parallax and is correct.
    ///
    /// # Panics
    ///
    /// If either delta is not finite, for the reason [`Self::orbit`] gives.
    pub fn pan(&mut self, right: f32, up: f32) {
        assert!(
            right.is_finite() && up.is_finite(),
            "pan deltas must be finite, got right {right} and up {up}"
        );
        let (right_axis, up_axis) = self.basis();
        self.pivot += (right_axis * right + up_axis * up) * self.viewport_height();
    }

    /// Moves the eye along its own view ray. Positive `delta` moves closer.
    ///
    /// **Multiplicative, and the field of view never changes.** Each unit of
    /// `delta` divides the distance by `e`, so a wheel notch covers the same
    /// fraction of the way in whether the camera is a metre out or a kilometre —
    /// which is the only behaviour that is usable across the range a model
    /// viewer sees. Changing the field of view instead would be a dolly zoom:
    /// the framing would match and the perspective would not.
    ///
    /// The result is clamped to [`MIN_DISTANCE`]`..=`[`MAX_DISTANCE`], so no
    /// amount of zooming reaches the pivot, passes through it, or runs off to
    /// infinity.
    ///
    /// # Panics
    ///
    /// If `delta` is not finite, for the reason [`Self::orbit`] gives.
    pub fn zoom(&mut self, delta: f32) {
        assert!(
            delta.is_finite(),
            "the zoom delta must be finite, got {delta}"
        );
        self.distance = (self.distance * (-delta).exp()).clamp(MIN_DISTANCE, MAX_DISTANCE);
    }

    /// Centres `bounds` and backs off far enough to see all of it, keeping the
    /// direction the camera is already looking from.
    ///
    /// **The fit is on the bounding sphere and on both axes.** A sphere because
    /// a box's silhouette changes as the camera orbits, so a box fit would frame
    /// the object at one angle and clip it at the next; the sphere's does not
    /// change at all, at the cost of a little air around a flat object. Both
    /// axes because the horizontal half-angle is
    /// `atan(tan(fov_y / 2) · aspect)`, which in a window narrower than it is
    /// tall is *smaller* than the vertical one — so framing a wide object by
    /// height alone leaves it hanging off both sides of a narrow window. The
    /// limiting half-angle `θ` is whichever is smaller, and a sphere of radius
    /// `r` fits inside it at `r / sin θ`, times [`FRAME_MARGIN`].
    ///
    /// The distance is then held to at least `near + r`, so the whole sphere is
    /// in front of the near plane rather than half of it behind — which matters
    /// for a small object, where the fit distance alone can be shorter than the
    /// near plane.
    ///
    /// # Panics
    ///
    /// If `aspect` is not finite and positive, as [`Projection::matrix`], or if
    /// `bounds` is not finite.
    pub fn frame(&mut self, bounds: Aabb, aspect: f32) {
        assert!(
            aspect.is_finite() && aspect > 0.0,
            "aspect ratio must be finite and positive, got {aspect}"
        );
        assert!(
            bounds.min.is_finite() && bounds.max.is_finite(),
            "the framed bounds must be finite, got {bounds:?}"
        );
        let radius = bounds.half_extent().length();
        let half_fov_y = 0.5 * self.fov_y;
        let half_fov_x = (half_fov_y.tan() * aspect).atan();
        let half_fov = half_fov_y.min(half_fov_x);
        self.pivot = bounds.center();
        self.distance = (radius * FRAME_MARGIN / half_fov.sin())
            .max(self.near + radius)
            .clamp(MIN_DISTANCE, MAX_DISTANCE);
    }

    /// The camera this controller is.
    ///
    /// `up` is the world's `+Y` rather than the view's own up vector: the pitch
    /// clamp is what keeps the two from becoming parallel, and a turntable whose
    /// up drifted with the view would roll.
    #[must_use]
    pub fn camera(&self) -> Camera {
        Camera {
            eye: self.pivot + self.direction() * self.distance,
            target: self.pivot,
            up: Vec3::Y,
            projection: Projection::Perspective {
                fov_y: self.fov_y,
                near: self.near,
            },
        }
    }

    /// The unit vector from the pivot towards the eye.
    fn direction(&self) -> Vec3 {
        let (sin_yaw, cos_yaw) = self.yaw.sin_cos();
        let (sin_pitch, cos_pitch) = self.pitch.sin_cos();
        Vec3::new(sin_yaw * cos_pitch, sin_pitch, cos_yaw * cos_pitch)
    }

    /// The camera's own right and up axes, both unit length.
    ///
    /// Built from the view direction the same way [`Camera::view`] builds its
    /// basis, so "the camera's right" means one thing in this module and in the
    /// matrix it produces. The cross with `Vec3::Y` is never degenerate because
    /// [`PITCH_LIMIT`] stops the direction short of vertical.
    fn basis(&self) -> (Vec3, Vec3) {
        let forward = -self.direction();
        let right = forward.cross(Vec3::Y).normalize();
        (right, right.cross(forward))
    }

    /// How much world the viewport shows vertically at the pivot's depth.
    fn viewport_height(&self) -> f32 {
        2.0 * self.distance * (0.5 * self.fov_y).tan()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use core::f32::consts::FRAC_PI_2;

    const ASPECT: f32 = 16.0 / 9.0;

    fn orbit_camera() -> OrbitCamera {
        OrbitCamera::new(Vec3::new(1.0, 2.0, -3.0), 5.0, Projection::default())
    }

    /// The camera's own right vector, from the [`Camera`] itself rather than
    /// from the controller's internals — so a test using it is asserting about
    /// the basis the view matrix will be built from.
    fn right_of(camera: &Camera) -> Vec3 {
        (camera.target - camera.eye)
            .normalize()
            .cross(Vec3::Y)
            .normalize()
    }

    /// Zero yaw and zero pitch stand where the rest of the engine's cameras
    /// stand, which is what makes the first frame of a viewer the frame every
    /// other tool would draw.
    #[test]
    fn an_untouched_orbit_camera_matches_the_engine_default_pose() {
        let orbit = OrbitCamera::new(Vec3::ZERO, 2.0, Projection::default());
        assert_eq!(orbit.camera(), Camera::default());
    }

    /// **A positive yaw delta swings the eye toward the camera's own right.**
    ///
    /// The claim is made against the camera's *own* starting basis, never
    /// against a world axis, so it holds at every orientation — which is why the
    /// loop runs it again from an already-orbited pose. `apps/lumen` shipped a
    /// camera that turned the wrong way because its only yaw test compared a
    /// stored angle, and a stored angle is right on a controller that builds its
    /// basis backwards too.
    #[test]
    fn a_positive_yaw_delta_swings_the_eye_toward_the_cameras_own_right() {
        for (turn, sign) in [(0.1f32, 1.0f32), (-0.1, -1.0)] {
            for (start_yaw, start_pitch) in [(0.0f32, 0.0f32), (2.3, 0.6), (-1.1, -0.4)] {
                let mut orbit = orbit_camera();
                orbit.orbit(start_yaw, start_pitch);

                let before = orbit.camera();
                let right = right_of(&before);
                orbit.orbit(turn, 0.0);
                let after = orbit.camera();

                let moved = (after.eye - before.eye).normalize();
                let leaned = moved.dot(right);
                assert!(
                    leaned * sign > 0.9,
                    "a yaw of {turn} from ({start_yaw}, {start_pitch}) moved the eye {leaned} of \
                     the way along the camera's own right {right:?}, and the sign says it went \
                     the other way"
                );
                // Small enough that the eye has not swung past the perpendicular
                // and made a wrong sign read as a right one.
                assert!(
                    (after.eye - before.eye).length() < 0.2 * orbit.distance(),
                    "a yaw of {turn} moved the eye too far for the sign above to mean what it says"
                );
                // And it is an *orbit*: the pivot stays put and the distance
                // does not change, which is what separates it from a look-around.
                assert_eq!(after.target, before.target, "the pivot moved");
                assert!(
                    ((after.eye - after.target).length() - (before.eye - before.target).length())
                        .abs()
                        < 1e-4,
                    "the distance changed"
                );
            }
        }
    }

    /// **A positive pitch delta lifts the eye**, so the view looks down on the
    /// pivot. The two halves of that sentence are asserted separately: a
    /// controller that raised the eye and then looked the other way would pass
    /// the first alone.
    #[test]
    fn a_positive_pitch_delta_lifts_the_eye_and_aims_the_view_downward() {
        for (tilt, sign) in [(0.5f32, 1.0f32), (-0.5, -1.0)] {
            let mut orbit = orbit_camera();
            let before = orbit.camera();
            orbit.orbit(0.0, tilt);
            let after = orbit.camera();

            assert!(
                (after.eye.y - before.eye.y) * sign > 0.0,
                "a pitch of {tilt} moved the eye from {:?} to {:?}, which is the wrong way",
                before.eye,
                after.eye
            );
            let forward = (after.target - after.eye).normalize();
            assert!(
                forward.y * sign < 0.0,
                "a pitch of {tilt} left the view looking {forward:?}, which is not back down at \
                 the pivot"
            );
            assert_eq!(after.target, before.target, "the pivot moved");
        }
    }

    /// The pitch stops short of vertical, where the direction is parallel to
    /// `up` and [`Camera::view`] panics rather than return a matrix of `NaN`s.
    ///
    /// The view matrix is *built* here, not just inspected: the panic this
    /// guards against is inside it, so a clamp that left the basis degenerate
    /// while keeping the angle plausible would still take this test down.
    ///
    /// **The claim is monotonicity across the sweep, not the pose at the end**,
    /// because an unclamped turntable sails over the top and comes back down the
    /// far side into a pose that looks entirely legal — this test first passed
    /// against a controller with the clamp deleted, for exactly that reason.
    #[test]
    fn the_pitch_clamp_stops_short_of_vertical_and_the_view_still_builds() {
        for sign in [1.0f32, -1.0] {
            let mut orbit = orbit_camera();
            let mut highest = f32::NEG_INFINITY;
            // Far more tilt than the limit is away, in steps that straddle
            // vertical rather than landing on it.
            for step in 0..40 {
                orbit.orbit(0.0, sign * 0.5);
                let camera = orbit.camera();
                let direction = (camera.eye - camera.target).normalize();
                let elevation = direction.y * sign;
                assert!(
                    elevation >= highest - 1e-6,
                    "at step {step} the eye's elevation fell from {highest} to {elevation}: it \
                     went over the top instead of stopping"
                );
                assert!(
                    elevation <= PITCH_LIMIT.sin() + 1e-6,
                    "at step {step} the eye reached an elevation of {elevation}, past the limit"
                );
                assert!(
                    camera.view().is_finite(),
                    "at step {step} the pose produced a NaN view matrix"
                );
                highest = elevation;
            }

            // And one enormous delta, which is what a fast drag or a wheel
            // gesture arrives as: it lands *on* the limit rather than wherever
            // the raw angle happens to wrap to.
            let mut orbit = orbit_camera();
            orbit.orbit(0.0, sign * 10.0);
            let camera = orbit.camera();
            let direction = (camera.eye - camera.target).normalize();
            assert!(
                (direction.y * sign - PITCH_LIMIT.sin()).abs() < 1e-5,
                "a ten-radian tilt left the eye at {direction:?} rather than on the limit"
            );
            let level = Vec3::new(direction.x, 0.0, direction.z).length();
            assert!(level > 0.01, "no horizontal component left at all");
            assert!(
                camera.view().is_finite(),
                "the clamped pose still produced a NaN view matrix"
            );
        }
    }

    /// **Pan moves the pivot in the view plane**, so the same delta goes a
    /// different way once the camera has orbited.
    ///
    /// Two poses, and the second is a quarter turn from the first: a controller
    /// that panned along world axes would move the pivot the same way in both
    /// and fail the perpendicularity check, while still passing a pan test
    /// written at one orientation only.
    #[test]
    fn panning_follows_the_view_plane_rather_than_the_world_axes() {
        let displace = |yaw: f32, pitch: f32| {
            let mut orbit = orbit_camera();
            orbit.orbit(yaw, pitch);
            let before = orbit.camera();
            orbit.pan(1.0, 0.0);
            let after = orbit.camera();
            // The whole camera slides: panning changes what it looks at, not
            // where it looks from relative to that.
            assert!(
                (after.eye - before.eye - (after.target - before.target)).length() < 1e-4,
                "pan moved the eye and the pivot by different amounts"
            );
            (after.target - before.target, right_of(&before))
        };

        for pitch in [0.0f32, 0.7, -0.7] {
            let (level, right) = displace(0.0, pitch);
            assert!(
                level.normalize().dot(right) > 0.999,
                "a rightward pan moved the pivot {level:?}, not along the camera's right {right:?}"
            );
            // Pitched or not, a rightward pan is level: the camera's right is
            // always horizontal, because `up` is the world's.
            assert!(
                level.y.abs() < 1e-5,
                "a rightward pan at pitch {pitch} changed the height by {}",
                level.y
            );
        }

        let (facing_z, _) = displace(0.0, 0.0);
        let (facing_x, right) = displace(FRAC_PI_2, 0.0);
        assert!(
            facing_x.normalize().dot(right) > 0.999,
            "after a quarter turn a rightward pan moved the pivot {facing_x:?}, not along the \
             camera's new right {right:?}"
        );
        assert!(
            facing_z.normalize().dot(facing_x.normalize()).abs() < 1e-5,
            "a quarter turn left the pan direction unchanged at {facing_z:?} and {facing_x:?}, \
             which is what panning in world axes looks like"
        );

        // And up is up: with the eye above the pivot, panning "up" the screen
        // pushes the pivot away and higher, not straight up the world's +Y.
        let mut orbit = orbit_camera();
        orbit.orbit(0.0, 0.7);
        let before = orbit.camera();
        orbit.pan(0.0, 1.0);
        let moved = orbit.camera().target - before.target;
        let (_, up_axis) = orbit.basis();
        assert!(
            moved.normalize().dot(up_axis) > 0.999,
            "an upward pan moved the pivot {moved:?}, not along the camera's up {up_axis:?}"
        );
        assert!(
            moved.y > 0.0 && moved.dot(Vec3::Y).abs() < moved.length(),
            "an upward pan from a raised eye must rise *and* travel, got {moved:?}"
        );
    }

    /// **A drag tracks the pointer**, which is the whole reason pan is measured
    /// in fractions of the viewport height rather than in metres.
    ///
    /// The observable is where the *old* pivot lands on screen: pan up by half
    /// the viewport and it must sit exactly on the bottom edge of the frame,
    /// `y = -1` in NDC. A pan in world units would land it somewhere that
    /// depended on the zoom, and a pan that forgot the factor of two would land
    /// it in the middle of the lower half.
    #[test]
    fn a_half_height_pan_moves_the_scene_exactly_half_a_screen() {
        for distance in [0.5f32, 5.0, 500.0] {
            let mut orbit = OrbitCamera::new(Vec3::ZERO, distance, Projection::default());
            orbit.orbit(0.9, 0.4);
            let was = orbit.pivot();
            orbit.pan(0.0, 0.5);

            let clip = orbit.camera().view_projection(ASPECT) * was.extend(1.0);
            let ndc_y = clip.y / clip.w;
            assert!(
                (ndc_y + 1.0).abs() < 1e-4,
                "at {distance} m a half-height pan put the old pivot at y = {ndc_y} in NDC, not on \
                 the bottom edge at -1"
            );
            assert!(
                (clip.x / clip.w).abs() < 1e-4,
                "a vertical pan moved the scene sideways"
            );
        }
    }

    /// Zoom changes the distance and nothing else: same direction, same pivot,
    /// same field of view. A dolly zoom would keep the framing and change the
    /// perspective, which is a shot, not a control.
    #[test]
    fn zoom_moves_the_eye_along_its_ray_and_leaves_the_lens_alone() {
        let mut orbit = orbit_camera();
        orbit.orbit(0.8, 0.3);
        let before = orbit.camera();

        orbit.zoom(1.0);
        let after = orbit.camera();
        assert!(
            after.eye.distance(after.target) < before.eye.distance(before.target),
            "a positive zoom delta moved the eye away from the pivot"
        );
        assert_eq!(after.target, before.target, "the pivot moved");
        assert_eq!(after.projection, before.projection, "the lens changed");
        assert!(
            (after.eye - after.target)
                .normalize()
                .dot((before.eye - before.target).normalize())
                > 0.9999,
            "the eye left its own view ray"
        );

        // Multiplicative: the same delta covers the same *fraction* of the way
        // in, wherever it starts from. An additive zoom would take a fixed
        // number of metres and be unusable at one end of the range or the other.
        let ratio = |start: f32| {
            let mut orbit = OrbitCamera::new(Vec3::ZERO, start, Projection::default());
            orbit.zoom(0.25);
            orbit.distance() / start
        };
        assert!(
            (ratio(2.0) - ratio(2000.0)).abs() < 1e-5,
            "zoom is not scale-free"
        );
    }

    /// **Zoom never reaches the pivot and never passes through it.** At distance
    /// zero `eye == target` and [`Camera::view`] panics; past it the camera is
    /// on the far side looking back, which reads as the model turning inside out.
    #[test]
    fn zoom_stops_short_of_the_pivot_and_short_of_infinity() {
        let mut orbit = orbit_camera();
        orbit.orbit(0.4, 0.2);
        let approach = (orbit.camera().eye - orbit.pivot()).normalize();

        for _ in 0..200 {
            orbit.zoom(5.0);
        }
        assert_eq!(
            orbit.distance(),
            MIN_DISTANCE,
            "the near clamp did not hold"
        );
        let camera = orbit.camera();
        let offset = camera.eye - camera.target;
        assert!(
            offset.dot(approach) > 0.0,
            "zooming in passed through the pivot: the eye is at {offset:?} against an approach \
             along {approach:?}"
        );
        assert!(
            offset.length() > 0.0 && camera.view().is_finite(),
            "the fully zoomed camera has no view matrix"
        );

        for _ in 0..200 {
            orbit.zoom(-5.0);
        }
        assert_eq!(orbit.distance(), MAX_DISTANCE, "the far clamp did not hold");
        assert!(orbit.camera().eye.is_finite());
    }

    /// **Framing fits both axes**, which is the case a height-only fit gets
    /// wrong: a wide object in a window narrower than it is tall hangs off both
    /// sides.
    ///
    /// The test projects the box's own corners through the real
    /// [`Camera::view_projection`] and demands every one inside the frame, then
    /// re-projects them at the distance a height-only fit would have chosen and
    /// demands that one *fails* — because a fit test that both formulas pass is
    /// not testing the thing that distinguishes them.
    #[test]
    fn framing_fits_a_wide_object_in_a_narrow_window_where_height_alone_would_not() {
        let bounds = Aabb {
            min: Vec3::new(-5.0, -0.5, -0.5),
            max: Vec3::new(5.0, 0.5, 0.5),
        };
        let corners = |bounds: Aabb| {
            let (lo, hi) = (bounds.min, bounds.max);
            [
                Vec3::new(lo.x, lo.y, lo.z),
                Vec3::new(hi.x, lo.y, lo.z),
                Vec3::new(lo.x, hi.y, lo.z),
                Vec3::new(hi.x, hi.y, lo.z),
                Vec3::new(lo.x, lo.y, hi.z),
                Vec3::new(hi.x, lo.y, hi.z),
                Vec3::new(lo.x, hi.y, hi.z),
                Vec3::new(hi.x, hi.y, hi.z),
            ]
        };
        // The widest a point sits in the frame, over both screen axes: 1.0 is
        // the edge.
        let extent = |camera: &Camera, aspect: f32, points: &[Vec3]| {
            let view_projection = camera.view_projection(aspect);
            points.iter().fold(0.0f32, |worst, &point| {
                let clip = view_projection * point.extend(1.0);
                assert!(clip.w > 0.0, "{point:?} is behind the eye");
                worst
                    .max((clip.x / clip.w).abs())
                    .max((clip.y / clip.w).abs())
            })
        };

        let mut narrow = 0.0;
        for aspect in [2.0f32, 1.0, 0.5] {
            let mut orbit = OrbitCamera::new(Vec3::ZERO, 1.0, Projection::default());
            orbit.frame(bounds, aspect);
            assert_eq!(orbit.pivot(), bounds.center(), "framing did not recentre");

            let camera = orbit.camera();
            let worst = extent(&camera, aspect, &corners(bounds));
            assert!(
                worst <= 1.0,
                "at aspect {aspect} a corner projected to {worst}, outside the frame"
            );
            // And it is *framed*, not merely somewhere in front: the bounding
            // sphere's silhouette reaches the edge, less the margin.
            let radius = bounds.half_extent().length();
            let (right, up) = orbit.basis();
            let silhouette = [
                camera.target + right * radius,
                camera.target - right * radius,
                camera.target + up * radius,
                camera.target - up * radius,
            ];
            let reach = extent(&camera, aspect, &silhouette);
            assert!(
                (0.8..=1.0).contains(&reach),
                "at aspect {aspect} the framed sphere only reached {reach} of the frame"
            );

            if aspect == 0.5 {
                narrow = orbit.distance();
            } else if aspect == 2.0 {
                assert!(
                    orbit.distance() < narrow || narrow == 0.0,
                    "a narrow window must back further off than a wide one"
                );
            }
        }

        // The control. Height alone gives `r / sin(fov_y / 2)`, which is what
        // this returns at every aspect — and at 0.5 it is not enough.
        let Projection::Perspective { fov_y, .. } = Projection::default() else {
            unreachable!("the default projection is perspective")
        };
        let radius = bounds.half_extent().length();
        let height_only = Camera {
            eye: Vec3::Z * (radius * FRAME_MARGIN / (0.5 * fov_y).sin()),
            ..Camera::default()
        };
        let missed = extent(&height_only, 0.5, &corners(bounds));
        assert!(
            missed > 1.0,
            "the control is only meaningful if a height-only fit genuinely fails here; it kept \
             every corner inside the frame at {missed}"
        );
    }

    /// Framing keeps the direction the camera is already looking from — the
    /// behaviour that makes it a "frame selection" rather than a "reset view".
    #[test]
    fn framing_moves_the_camera_without_turning_it() {
        let mut orbit = orbit_camera();
        orbit.orbit(1.4, -0.6);
        let before = orbit.camera();
        let facing = (before.eye - before.target).normalize();

        orbit.frame(
            Aabb {
                min: Vec3::new(8.0, 8.0, 8.0),
                max: Vec3::new(12.0, 10.0, 12.0),
            },
            ASPECT,
        );
        let after = orbit.camera();
        assert!(
            (after.eye - after.target).normalize().dot(facing) > 0.9999,
            "framing turned the camera from {facing:?} to {:?}",
            (after.eye - after.target).normalize()
        );
        assert_eq!(after.target, Vec3::new(10.0, 9.0, 10.0));
    }

    /// A zero-size bound is a real input — one imported node, one selected
    /// vertex — and the fit distance for it is zero, which is a camera that
    /// panics. The near-plane floor is what catches it.
    ///
    /// **The observable is the framed point's depth**, not merely that it has
    /// one: [`Camera::depth_of`] answers `Some` for anything in front of the
    /// *eye*, so a camera parked a centimetre from the point — inside its own
    /// near plane, where nothing is drawn — passes an `is_some` check happily.
    /// It did, until this test read the depth instead.
    #[test]
    fn framing_a_single_point_leaves_a_usable_camera() {
        let point = Vec3::new(3.0, -1.0, 2.0);
        let mut orbit = orbit_camera();
        orbit.frame(
            Aabb {
                min: point,
                max: point,
            },
            ASPECT,
        );
        assert!(
            orbit.distance() >= MIN_DISTANCE,
            "framing a point put the eye {} m from it",
            orbit.distance()
        );
        let camera = orbit.camera();
        assert!(camera.view().is_finite());
        let Projection::Perspective { near, .. } = camera.projection else {
            unreachable!("an orbit camera is perspective-only")
        };
        assert!(
            orbit.distance() >= near,
            "framing a point put the eye {} m from it, inside its own {near} m near plane",
            orbit.distance()
        );
        let depth = camera
            .depth_of(point, ASPECT)
            .expect("the framed point is in front of the eye");
        assert!(
            depth > 0.0 && depth <= crcbl_hal::depth::NEAR + 1e-5,
            "the framed point projected to depth {depth}, and anything above {} is nearer than \
             the near plane and never drawn",
            crcbl_hal::depth::NEAR
        );
    }

    /// An orthographic projection has no field of view to fit against and a zoom
    /// that would move the eye and change nothing on screen, so it is refused at
    /// the boundary rather than silently mis-served.
    #[test]
    #[should_panic(expected = "perspective-only")]
    fn an_orthographic_projection_is_refused() {
        let _ = OrbitCamera::new(
            Vec3::ZERO,
            4.0,
            Projection::Orthographic {
                half_height: 1.0,
                near: 0.1,
                far: 100.0,
            },
        );
    }

    /// A `NaN` delta stays in the angle forever and takes the view matrix down
    /// several frames later, so it stops here instead.
    #[test]
    #[should_panic(expected = "orbit deltas must be finite")]
    fn a_non_finite_orbit_delta_is_refused() {
        orbit_camera().orbit(f32::NAN, 0.0);
    }

    #[test]
    #[should_panic(expected = "pan deltas must be finite")]
    fn a_non_finite_pan_delta_is_refused() {
        orbit_camera().pan(0.0, f32::INFINITY);
    }

    #[test]
    #[should_panic(expected = "zoom delta must be finite")]
    fn a_non_finite_zoom_delta_is_refused() {
        orbit_camera().zoom(f32::NAN);
    }
}
