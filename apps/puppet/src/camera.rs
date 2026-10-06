//! The third-person camera, and **where a yaw becomes a world direction**.
//!
//! ```text
//!   keys ──▶ Follow { yaw, pitch } ──▶ Camera   (what the frame is drawn from)
//!                    │
//!                    └── yaw ──▶ OrbitCamera::walk_direction ──▶ DVec3
//!                                       (what the controller is asked for)
//! ```
//!
//! # Why the conversion is not in `crcbl-phys`
//!
//! [`CharacterController::move_and_slide`](crcbl::phys::CharacterController::move_and_slide)
//! takes a **world-space displacement** and holds no camera, no view basis and
//! no yaw. That is deliberate, and its module docs say why: turning a stick into
//! a direction differs between a first-person rig and a third-person one, so it
//! belongs to whichever rig is being built. This sample is a third-person one,
//! so the conversion it asks for is
//! [`OrbitCamera::walk_direction`](crcbl::render::OrbitCamera::walk_direction)
//! — the rig's own, because the yaw it takes is the yaw [`Follow`] hands
//! [`OrbitCamera`]. `apps/shard` is the second sample to ask for it, which is
//! why it is beside the rig rather than in either of them.
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
//! # The boom stops short of the map
//!
//! The eye sits [`DISTANCE`] behind the focus only where nothing is in the
//! way: [`Follow::camera`] sweeps a sphere of [`BOOM_RADIUS`] from the focus
//! out to that eye through the client's [`ClientQueryWorld`] — the map's
//! colliders, the same ones the server's controller walks — and puts the eye
//! where the sphere first meets something. So the camera never sees through
//! the ground when it is pitched below the character, nor through a step it
//! has swung behind. The pull-in and the return are both immediate: a boom
//! that eased back out would spend those frames inside what it had just left.
//!
//! # Which way is forward
//!
//! `crcbl` is right-handed with `+Y` up and `-Z` forward, and
//! [`OrbitCamera`] measures a yaw so that **zero puts the eye on `+Z` looking
//! down `-Z`** — the pose [`Camera::default`](crcbl::render::Camera) is in. So a
//! yaw of zero walks the character down `-Z`. This module's
//! `the_walk_direction_is_where_the_camera_is_actually_looking` test is what
//! holds the engine's arithmetic to the matrix *this* frame is drawn with
//! rather than to a comment — and it is why the hoist did not take the test
//! with it.

use crcbl::client::ClientQueryWorld;
use crcbl::math::{DVec3, Vec3};
use crcbl::phys::{QueryFilter, Segment};
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

/// The radius of the sphere the boom sweeps from the focus to the eye, in
/// metres: the clearance the eye keeps from anything it is pulled in by.
///
/// Wider than the near plane's corners are from the eye at any window
/// shape this sample is shown in, so a pulled-in eye's near plane cannot cut
/// into the surface it stopped at — `the_boom_is_wider_than_the_near_plane`
/// holds it to that.
pub const BOOM_RADIUS: f64 = 0.2;

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

    /// The camera looking at `focus`, which is a point in world space, with
    /// its eye pulled in from [`DISTANCE`] to wherever the boom first meets
    /// one of `world`'s colliders — see the [module docs](self).
    #[must_use]
    pub fn camera(&self, focus: Vec3, world: &mut ClientQueryWorld) -> Camera {
        let projection = Projection::Perspective {
            fov_y: FOV_Y,
            near: NEAR,
        };
        let mut orbit = OrbitCamera::new(focus, DISTANCE, projection);
        orbit.orbit(self.yaw, self.pitch);
        let wanted = orbit.camera();
        let boom = Segment::new(focus.as_dvec3(), wanted.eye.as_dvec3());
        let Some((_, hit)) = world.sweep_sphere(&boom, BOOM_RADIUS, QueryFilter::ALL) else {
            return wanted;
        };
        // `t` is the share of the boom the sphere covered before it touched,
        // so the eye stands that share of the way out.
        #[allow(clippy::cast_possible_truncation)]
        let mut pulled_in = OrbitCamera::new(focus, DISTANCE * hit.t as f32, projection);
        pulled_in.orbit(self.yaw, self.pitch);
        pulled_in.camera()
    }
}

/// The yaw a body facing `direction` is at, in the same measure
/// [`Follow::yaw`] is in.
///
/// The inverse of the `ahead` vector in [`OrbitCamera::walk_direction`]: zero faces `-Z`.
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

    use crcbl::phys::PhysicsWorld;

    use crate::map::{LOW_STEP_FAR_Z, LOW_STEP_TOP, Map, SPAWN, STEEP_MOUND};

    /// A query world with nothing in it, so the boom never pulls in.
    fn open_air() -> ClientQueryWorld {
        ClientQueryWorld::new(PhysicsWorld::new())
    }

    /// The query world the sample's client builds: the committed map.
    fn the_map() -> ClientQueryWorld {
        ClientQueryWorld::new(Map::built_in().world())
    }

    /// Whether `point` is within [`NEAR`] of any collider — where the near
    /// plane would cut into it.
    fn too_close(world: &mut ClientQueryWorld, point: Vec3) -> bool {
        let mut hits = Vec::new();
        world.overlap_sphere_ids_into(point.as_dvec3(), f64::from(NEAR), &mut hits);
        !hits.is_empty()
    }

    /// Where the camera's focus is for a character whose feet are at `feet`.
    #[allow(clippy::cast_possible_truncation)]
    fn focus_over(feet: DVec3) -> Vec3 {
        Vec3::new(feet.x as f32, feet.y as f32 + FOCUS_HEIGHT, feet.z as f32)
    }

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
            let camera = follow.camera(Vec3::new(1.0, 2.0, 3.0), &mut open_air());
            let view = camera.target - camera.eye;
            let ahead = Vec3::new(view.x, 0.0, view.z).normalize();

            let walk = OrbitCamera::walk_direction(f64::from(follow.yaw()), 1.0, 0.0);
            assert!(
                (walk.x - f64::from(ahead.x)).abs() < 1e-5
                    && (walk.z - f64::from(ahead.z)).abs() < 1e-5,
                "at yaw {yaw}: the camera looks along {ahead}, forward walks {walk}",
            );

            // And the strafe is the camera's own right: a quarter turn from
            // ahead, in the sense that leaves the pair right-handed about +Y.
            let strafe = OrbitCamera::walk_direction(f64::from(follow.yaw()), 0.0, 1.0);
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
        let diagonal = OrbitCamera::walk_direction(0.7, 1.0, 1.0);
        assert!((diagonal.length() - 1.0).abs() < 1e-12);
        assert_eq!(OrbitCamera::walk_direction(0.7, 0.0, 0.0), DVec3::ZERO);
    }

    /// [`facing_of`] reads back the yaw [`OrbitCamera::walk_direction`] was given.
    #[test]
    fn a_facing_read_back_is_the_yaw_it_was_walked_at() {
        for sixteenth in 0..16 {
            let yaw = core::f64::consts::TAU * f64::from(sixteenth) / 16.0 - core::f64::consts::PI;
            let facing = facing_of(OrbitCamera::walk_direction(yaw, 1.0, 0.0))
                .expect("a forward walk has one");
            // Compared as directions rather than as angles: the two are equal
            // modulo a full turn, and the wrap is not what this is about.
            let (want, got) = (
                OrbitCamera::walk_direction(yaw, 1.0, 0.0),
                OrbitCamera::walk_direction(facing, 1.0, 0.0),
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
        let _ = follow.camera(Vec3::ZERO, &mut open_air()).view();
    }

    /// **In the clear the boom is its full length**: the camera the sample
    /// opens on, behind the character at the spawn, is the one it drew before
    /// the boom was swept.
    #[test]
    fn the_boom_keeps_its_length_in_the_clear() {
        let follow = Follow::default();
        let focus = focus_over(SPAWN);
        let swept = follow.camera(focus, &mut the_map());
        assert_eq!(swept, follow.camera(focus, &mut open_air()));
        assert!(((swept.eye - focus).length() - DISTANCE).abs() < 1e-4);
    }

    /// **Pitched below the character, the eye stops above the ground** rather
    /// than going under it, and nearer than [`DISTANCE`].
    #[test]
    fn the_boom_pulls_in_rather_than_going_under_the_ground() {
        let mut follow = Follow::default();
        follow.turn(0.0, -1.0);
        let focus = focus_over(SPAWN);
        let mut world = the_map();
        let camera = follow.camera(focus, &mut world);

        let wanted = follow.camera(focus, &mut open_air());
        assert!(
            wanted.eye.y < 0.0,
            "the unswept eye is not under the ground"
        );
        assert!(
            camera.eye.y > 0.0,
            "the eye went under the ground: {camera:?}"
        );
        assert!((camera.eye - focus).length() < DISTANCE);
        assert!(!too_close(&mut world, camera.eye), "{camera:?}");
        // Still looking at the character, along the same line.
        assert_eq!(camera.target, focus);
        let along = (camera.eye - focus).normalize();
        assert!((along - (wanted.eye - focus).normalize()).length() < 1e-4);
    }

    /// **The eye is never in the map**, wherever the character stands and
    /// however the camera is turned: on the spawn pad, on the low step with
    /// the high one ahead, and beside the steep mound — swung all the way
    /// round at every pitch the camera can reach.
    #[test]
    fn the_eye_is_never_inside_the_map() {
        let mut world = the_map();
        let stands = [
            SPAWN,
            DVec3::new(0.0, LOW_STEP_TOP, LOW_STEP_FAR_Z + 1.0),
            DVec3::new(STEEP_MOUND.0 + STEEP_MOUND.2 + 0.5, 0.0, STEEP_MOUND.1),
        ];
        let mut pulled_in = 0;
        for feet in stands {
            let focus = focus_over(feet);
            assert!(
                !too_close(&mut world, focus),
                "the focus {focus} is in the map"
            );
            for yaw_step in 0..24 {
                for pitch_step in -8..=8 {
                    let mut follow = Follow::default();
                    follow.turn(
                        core::f32::consts::TAU * yaw_step as f32 / 24.0,
                        PITCH_LIMIT * pitch_step as f32 / 8.0 - START_PITCH,
                    );
                    let camera = follow.camera(focus, &mut world);
                    assert!(
                        !too_close(&mut world, camera.eye),
                        "standing at {feet}, turned {yaw_step}/{pitch_step}: {camera:?}"
                    );
                    if (camera.eye - focus).length() < DISTANCE - 1e-3 {
                        pulled_in += 1;
                    }
                }
            }
        }
        // The turns that would have put the eye in the map are what this is
        // about, so some of them have to have happened.
        assert!(pulled_in > 0, "no turn pulled the boom in");
    }

    /// The widest window shape [`BOOM_RADIUS`] is checked against: a 32:9
    /// super-ultrawide, wider than any display this sample is shown on.
    const WIDEST_ASPECT: f32 = 32.0 / 9.0;

    /// **The boom's clearance covers the near plane**: its corners, the part
    /// of it farthest from the eye, are nearer than [`BOOM_RADIUS`] at the
    /// widest window, so the plane cannot cut into what the boom stopped at.
    #[test]
    fn the_boom_is_wider_than_the_near_plane() {
        let half_height = NEAR * (0.5 * FOV_Y).tan();
        let corner = Vec3::new(half_height * WIDEST_ASPECT, half_height, NEAR).length();
        assert!(f64::from(corner) < BOOM_RADIUS, "{corner} m");
    }
}
