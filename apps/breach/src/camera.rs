//! The first-person camera, and **the one place a yaw becomes a world
//! direction**.
//!
//! ```text
//!   mouse / arrows ──▶ Eye { yaw, pitch } ──▶ Camera         (what the frame is drawn from)
//!                             │
//!                             ├── yaw ────────▶ walk_direction ──▶ DVec3   (what the controller is asked for)
//!                             └── yaw, pitch ─▶ forward ───────▶ DVec3   (where the pistol's ray goes)
//! ```
//!
//! # This module and `apps/puppet/src/camera.rs` are the two halves of one claim
//!
//! [`CharacterController::move_and_slide`](crcbl::phys::CharacterController::move_and_slide)
//! takes a **world-space displacement** and holds no camera, no view basis and
//! no yaw. `apps/puppet/src/camera.rs` argues that seam from the third-person
//! side: turning a stick into a direction is the one step that genuinely
//! differs between a first-person rig and a third-person one, so it belongs to
//! whichever of the two is being built.
//!
//! **This is the first-person side of the same argument, and it is what makes
//! the claim checkable rather than assertable.** One demo saying "the
//! controller does not know which camera is watching" is a comment; two demos
//! driving the same controller from cameras that do not share a line of code
//! is evidence. `crcbl-phys` gained nothing for either of them — no yaw, no
//! pitch, no eye, no view basis — and the diff for this sample is the proof of
//! that.
//!
//! The two conversions are not even spelled the same way, which is the sharper
//! form of the point. Puppet measures its yaw the way
//! [`OrbitCamera`](crcbl::render::OrbitCamera) does, where a rising yaw swings
//! the eye one way round the pivot; this module measures it the way
//! [`Flyer`](crcbl::render::Flyer) does, where a rising yaw swings the view
//! toward `+X`. Both are correct for the camera they belong to, and neither
//! could be moved into the controller without picking one and being wrong for
//! the other demo.
//!
//! # The camera does not walk, and that is the whole difference from `Flyer`
//!
//! [`Flyer`](crcbl::render::Flyer) is the engine's free camera: it holds two
//! angles **and a position**, and its `advance` integrates a held key straight
//! into the eye. That is what a reviewer flying a fixture wants and exactly
//! what a player must not have — a camera that moves itself walks through
//! walls. So [`Eye`] holds the two angles and nothing else, and where it stands
//! is wherever [`crate::game`] last left the capsule. The turn rates are the
//! engine's ([`TURN_RATE`], [`LOOK_RATE`]) because an angle is an angle.
//!
//! # Which way is forward
//!
//! `crcbl` is right-handed with `+Y` up and `-Z` forward, and a yaw of zero
//! looks down `-Z` — the pose [`Camera::default`](crcbl::render::Camera) is in
//! and the direction the range's lanes run. A rising yaw swings the view toward
//! `+X`, a rising pitch raises it. `the_shot_goes_where_the_camera_is_actually_looking`
//! is what holds this module's arithmetic to the matrix the frame is drawn with
//! rather than to this paragraph.

use crcbl::math::{DVec3, Vec2, Vec3};
use crcbl::render::orbit::PITCH_LIMIT;
use crcbl::render::{Camera, Projection};

/// How far above the character's **feet** the eye sits, in metres.
///
/// The centre of the capsule is 1.0 m up and the crown 2.0 m, so this puts the
/// eye where a head is rather than where the middle of the body is. It is also
/// the height [`crate::map::PLATE_CENTRE_Y`] is written from: a plate whose
/// centre is at eye height is one a level shot hits, which is what makes
/// "aim down the range and pull the trigger" a thing a visitor can do in the
/// first second.
pub const EYE_HEIGHT: f64 = 1.65;

/// The vertical field of view, in radians.
///
/// 70°, which at a 4:3 window is a little over 90° across — the range a
/// first-person shooter is played at, and wide enough that the two outer lanes
/// are in frame while the middle one is being shot at.
pub const FOV_Y: f32 = 70.0 * (core::f32::consts::PI / 180.0);

/// The near plane, in metres. Short, because the eye is inside the room and the
/// firing line comes within arm's reach of it.
pub const NEAR: f32 = 0.05;

/// How fast a held arrow key turns the view, in radians a second.
///
/// The engine's own, because a keyboard turn rate is not this sample's to
/// invent — and because the arrows are the *fallback* here rather than the
/// binding: see [`Eye::look`].
pub const TURN_RATE: f32 = crcbl::render::TURN;

/// How far the view turns per pixel of mouse movement, in radians.
///
/// The engine's own, for [`TURN_RATE`]'s reason.
pub const LOOK_RATE: f32 = crcbl::render::LOOK;

/// A first-person view: two angles, and no position at all.
///
/// Where it stands is the character's, which is the whole shape of this sample
/// — see the module docs.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct Eye {
    yaw: f32,
    pitch: f32,
}

impl Eye {
    /// Turns the view by `yaw` and `pitch` radians — one frame's worth of a
    /// **held key**, already multiplied by [`TURN_RATE`] and the frame length.
    ///
    /// The pitch is clamped to [`PITCH_LIMIT`]: at exactly vertical the forward
    /// vector is parallel to `up` and [`Camera::view`](crcbl::render::Camera)
    /// panics rather than hand back a matrix of `NaN`s.
    ///
    /// The yaw is left to run, and wrapping it would change nothing a sine or a
    /// cosine can see.
    ///
    /// # Panics
    ///
    /// If either delta is not finite. An angle that goes `NaN` here stays `NaN`
    /// for the rest of the session, it crosses the wire into
    /// [`crate::game`], and the panic it causes is several ticks from the input
    /// that caused it.
    pub fn turn(&mut self, yaw: f32, pitch: f32) {
        assert!(
            yaw.is_finite() && pitch.is_finite(),
            "view deltas must be finite, got yaw {yaw} and pitch {pitch}"
        );
        self.yaw += yaw;
        self.pitch = (self.pitch + pitch).clamp(-PITCH_LIMIT, PITCH_LIMIT);
    }

    /// Turns by one frame's worth of **mouse** movement.
    ///
    /// `motion` is [`PointerUpdate::motion`](crcbl::engine::PointerUpdate):
    /// framebuffer pixels, **Y down**. That is why the pitch subtracts where
    /// the yaw adds — pushing the mouse away from the hand is a negative `Y`
    /// and has to raise the view — and it is the one sign in this file no
    /// compiler can check. `the_mouse_swings_the_view_the_way_it_moved` is what
    /// does.
    ///
    /// # Not scaled by the frame length, unlike [`turn`](Self::turn)
    ///
    /// A held key is a *rate* and has to be integrated against the clock. A
    /// mouse delta is already a distance the hand moved, so scaling it by `dt`
    /// would make the same sweep turn a different amount on a slow frame.
    /// [`Flyer::look`](crcbl::render::Flyer::look) draws the same distinction
    /// for the same reason.
    pub fn look(&mut self, motion: Vec2) {
        if motion == Vec2::ZERO {
            return;
        }
        self.turn(motion.x * LOOK_RATE, -motion.y * LOOK_RATE);
    }

    /// Points the view at `yaw` and `pitch` outright.
    ///
    /// **What the range's own warm-up is shown through.** While nobody has
    /// taken the controls the simulation is aiming, and a first-person camera
    /// that did not follow that aim would be showing a different room from the
    /// one being shot at — so [`crate::app`] writes the simulation's angles
    /// here every frame until the player's first key ends it. See
    /// [`crate::game::RenderState::imposed_aim`].
    pub fn point_at(&mut self, yaw: f32, pitch: f32) {
        self.yaw = yaw;
        self.pitch = pitch.clamp(-PITCH_LIMIT, PITCH_LIMIT);
    }

    /// The azimuth the view is at — what the walk direction is derived from,
    /// and one of the two numbers the client puts on the wire.
    #[must_use]
    pub const fn yaw(&self) -> f32 {
        self.yaw
    }

    /// The elevation the view is at, positive up — the other one.
    #[must_use]
    pub const fn pitch(&self) -> f32 {
        self.pitch
    }

    /// The camera looking out of `eye`, which is a point in world space.
    #[must_use]
    pub fn camera(&self, eye: Vec3) -> Camera {
        let ahead = forward(f64::from(self.yaw), f64::from(self.pitch));
        #[allow(clippy::cast_possible_truncation)]
        let ahead = Vec3::new(ahead.x as f32, ahead.y as f32, ahead.z as f32);
        Camera {
            eye,
            target: eye + ahead,
            up: Vec3::Y,
            projection: Projection::Perspective {
                fov_y: FOV_Y,
                near: NEAR,
            },
        }
    }
}

/// The world-space direction the view is pointing, given its two angles.
///
/// **Where the pistol's ray goes**, and a unit vector: [`crate::game`] casts
/// from the eye along it, so this is the one function in the sample that
/// decides what a trigger pull is aimed at.
#[must_use]
pub fn forward(yaw: f64, pitch: f64) -> DVec3 {
    let (sin_yaw, cos_yaw) = yaw.sin_cos();
    let (sin_pitch, cos_pitch) = pitch.sin_cos();
    DVec3::new(sin_yaw * cos_pitch, sin_pitch, -cos_yaw * cos_pitch)
}

/// The world-space direction a stick means, given the yaw the view is at.
///
/// `ahead` is positive away from the eye and `strafe` positive to the player's
/// right; both are expected in `-1..=1`. The result is a **unit** direction, or
/// zero where nothing was asked for — the caller multiplies by a speed and a
/// timestep to get the displacement the controller takes.
///
/// **The pitch is deliberately not in it.** Looking at the ceiling must not
/// walk a player into it, which is what folding the pitch into the walk would
/// do; the shot takes the pitch and the walk does not, and that is the
/// difference between [`forward`] and this.
///
/// Normalised rather than passed through, so holding two keys does not walk
/// `√2` times faster than holding one — which is the oldest bug in this
/// conversion.
#[must_use]
pub fn walk_direction(yaw: f64, ahead: f64, strafe: f64) -> DVec3 {
    let (sin, cos) = yaw.sin_cos();
    // The view's own basis on the ground plane. `along` is where the eye looks
    // with the vertical taken out; `right` is `along × up`.
    let along = DVec3::new(sin, 0.0, -cos);
    let right = DVec3::new(cos, 0.0, sin);
    (along * ahead + right * strafe).normalize_or_zero()
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Sixteen yaws and five pitches, none of them axis-aligned by accident.
    fn poses() -> impl Iterator<Item = (f32, f32)> {
        (0..16).flat_map(|sixteenth| {
            let yaw = core::f32::consts::TAU * sixteenth as f32 / 16.0 - core::f32::consts::PI;
            (-2..=2).map(move |fifth| (yaw, PITCH_LIMIT * fifth as f32 / 2.5))
        })
    }

    /// **The pistol's ray goes where the camera is pointing**, checked against
    /// the [`Camera`] the frame is actually drawn from rather than against a
    /// restatement of the same trigonometry.
    ///
    /// This is the assertion the whole module exists for: get one sign wrong
    /// and the demo shoots behind the player, or at the floor, and every other
    /// test here passes through it.
    #[test]
    fn the_shot_goes_where_the_camera_is_actually_looking() {
        for (yaw, pitch) in poses() {
            let mut eye = Eye::default();
            eye.turn(yaw, pitch);
            let camera = eye.camera(Vec3::new(1.0, 2.0, 3.0));
            let view = (camera.target - camera.eye).normalize();

            let shot = forward(f64::from(eye.yaw()), f64::from(eye.pitch()));
            assert!(
                (shot.x - f64::from(view.x)).abs() < 1e-5
                    && (shot.y - f64::from(view.y)).abs() < 1e-5
                    && (shot.z - f64::from(view.z)).abs() < 1e-5,
                "at yaw {yaw} pitch {pitch}: the camera looks along {view}, the shot goes {shot}",
            );
            assert!((shot.length() - 1.0).abs() < 1e-12, "the ray is not unit");
        }
    }

    /// **The walk is that same direction with the vertical taken out**, and the
    /// strafe is the player's own right.
    ///
    /// The pitch is swept as well as the yaw, because "looking up must not walk
    /// upward" is the claim [`walk_direction`]'s missing pitch makes and the
    /// one a reader cannot see by reading it.
    #[test]
    fn the_walk_is_where_the_camera_looks_with_the_vertical_taken_out() {
        for (yaw, pitch) in poses() {
            let mut eye = Eye::default();
            eye.turn(yaw, pitch);
            let camera = eye.camera(Vec3::ZERO);
            let view = camera.target - camera.eye;
            let flat = Vec3::new(view.x, 0.0, view.z).normalize();

            let walk = walk_direction(f64::from(eye.yaw()), 1.0, 0.0);
            assert_eq!(walk.y, 0.0, "at pitch {pitch} the walk left the ground");
            assert!(
                (walk.x - f64::from(flat.x)).abs() < 1e-5
                    && (walk.z - f64::from(flat.z)).abs() < 1e-5,
                "at yaw {yaw}: the camera looks along {flat}, forward walks {walk}",
            );

            // And the strafe is the player's own right: a quarter turn from
            // ahead, in the sense that leaves the pair right-handed about +Y.
            let strafe = walk_direction(f64::from(eye.yaw()), 0.0, 1.0);
            let cross = walk.cross(strafe);
            assert!(
                cross.y < -0.99,
                "at yaw {yaw}: ahead {walk} and strafe {strafe} are not a right-handed pair",
            );
        }
    }

    /// A yaw of zero looks down `-Z`, which is where [`crate::map`]'s lanes
    /// are, and a quarter turn to the right looks down `+X`.
    ///
    /// The one test here written against literal axes rather than against the
    /// camera, because "the range runs away from the spawn" is a fact about the
    /// map that the map's own constants are written to.
    #[test]
    fn a_zero_yaw_looks_down_the_range() {
        let level = forward(0.0, 0.0);
        assert!((level - DVec3::new(0.0, 0.0, -1.0)).length() < 1e-12);
        let right = forward(core::f64::consts::FRAC_PI_2, 0.0);
        assert!((right - DVec3::new(1.0, 0.0, 0.0)).length() < 1e-12);
        let up = forward(0.0, core::f64::consts::FRAC_PI_2);
        assert!((up - DVec3::Y).length() < 1e-12);
    }

    /// Holding two keys walks at one speed, not at `√2` of it.
    #[test]
    fn a_diagonal_is_not_faster_than_a_straight_line() {
        let diagonal = walk_direction(0.7, 1.0, 1.0);
        assert!((diagonal.length() - 1.0).abs() < 1e-12);
        assert_eq!(walk_direction(0.7, 0.0, 0.0), DVec3::ZERO);
    }

    /// **The mouse swings the view the way the hand moved**, which is the one
    /// sign in this module a compiler cannot check.
    ///
    /// Asserted against the camera rather than against the fields: a swapped
    /// sign leaves a perfectly plausible pair of angles and an inverted look.
    #[test]
    fn the_mouse_swings_the_view_the_way_it_moved() {
        let at = Vec3::new(0.0, EYE_HEIGHT as f32, 0.0);

        // Moving the mouse right swings the view toward +X.
        let mut eye = Eye::default();
        eye.look(Vec2::new(100.0, 0.0));
        let view = eye.camera(at).target - at;
        assert!(view.x > 0.0, "a rightward mouse looked toward {view}");

        // Pushing it away from the hand — a negative Y, screen coordinates
        // being Y down — raises the view.
        let mut eye = Eye::default();
        eye.look(Vec2::new(0.0, -100.0));
        let view = eye.camera(at).target - at;
        assert!(view.y > 0.0, "pushing the mouse away looked toward {view}");

        // And a frame the pointer did not move is not a turn.
        let mut eye = Eye::default();
        eye.look(Vec2::ZERO);
        assert_eq!(eye, Eye::default());
    }

    /// The pitch cannot accumulate past the bound the view matrix needs, however
    /// long a key is held or however far the hand is dragged.
    #[test]
    fn the_pitch_stops_short_of_vertical_however_far_it_is_pushed() {
        let mut eye = Eye::default();
        for _ in 0..1000 {
            eye.turn(0.0, 1.0);
        }
        assert!((eye.pitch() - PITCH_LIMIT).abs() < 1e-6);
        for _ in 0..2000 {
            eye.look(Vec2::new(0.0, 10_000.0));
        }
        assert!((eye.pitch() + PITCH_LIMIT).abs() < 1e-6);
        // And an imposed aim is clamped too — the warm-up's angles come from
        // the simulation, which is not this module and cannot be trusted to
        // have applied the bound.
        eye.point_at(0.0, 100.0);
        assert!((eye.pitch() - PITCH_LIMIT).abs() < 1e-6);
        // The camera it builds is still a camera: `Camera::view` panics on an
        // eye looking straight up its own axis.
        let _ = eye.camera(Vec3::ZERO).view();
    }
}
