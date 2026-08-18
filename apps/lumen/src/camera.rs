//! The free-fly camera, and the one thing that makes it a *fixture's* camera.
//!
//! `docs/plan/sample/13-lumen.md`'s Scope asks for "a free-fly camera, a fixed
//! camera set for goldens". The two are the same [`Camera`] type and the same
//! projection — what differs is whether anything moves it — so this module is
//! the moving half and [`crate::room::fixed_camera`] is the pose both start at.
//!
//! # Why it is stepped on the fixed timestep
//!
//! [`Flyer::advance`] is called from `tick`, not from `draw`. A camera
//! integrated on the frame rate flies at a speed proportional to how fast the
//! machine is, which makes "walk to the corner and look at the contact shadow"
//! a different journey on every machine — and it makes a headless run's camera a
//! function of the frame budget rather than of the clock.
//!
//! # Keyboard only, and that is a decision
//!
//! No pointer look, and the reason has moved. It used to be that the loop's own
//! `PointerUpdate` carried a position and nothing else, so a mouse-look camera
//! would have had to difference clamped, accelerated positions and would stop
//! turning the moment the cursor reached the edge of the frame. It carries
//! `motion` now — the unaccelerated delta, added so `apps/viewer` could be
//! hosted — so that objection is gone and what is left is the cursor itself:
//! nothing here asks for `PointerMode::Locked`, so a look would drag the visible
//! pointer out of the window and onto whatever is behind it. The arrow keys turn
//! instead, and `docs/backlog.md` carries the grab as the thing that would
//! change this.

use crcbl::core::input::KeyCode;
use crcbl::math::Vec3;
use crcbl::render::Camera;

/// How fast the camera walks, in metres a second.
///
/// A brisk walk rather than a fly-through speed: the room is six metres across,
/// and a camera that crossed it in a second would be one a reviewer overshoots
/// every time they try to stand in the corner.
pub const SPEED: f32 = 2.4;

/// How fast the arrow keys turn it, in radians a second.
pub const TURN: f32 = 1.6;

/// How far the pitch may go from level, in radians.
///
/// Just short of straight up and straight down: at exactly vertical the
/// forward vector is parallel to `up` and the view matrix is degenerate, which
/// arrives as a frame of nothing.
const PITCH_LIMIT: f32 = std::f32::consts::FRAC_PI_2 - 0.05;

/// Which of the movement keys are down.
///
/// A struct of flags rather than a set, because the loop hands keys over one
/// edge at a time and what [`Flyer::advance`] needs is the *held* state — a
/// camera driven by edges alone would move one step per press.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
struct Held {
    forward: bool,
    back: bool,
    left: bool,
    right: bool,
    up: bool,
    down: bool,
    turn_left: bool,
    turn_right: bool,
    look_up: bool,
    look_down: bool,
}

/// A camera the keyboard flies.
///
/// Holds a position and two angles rather than a [`Camera`], because a camera is
/// an eye and a *target*: integrating a target directly makes turning depend on
/// how far away it happens to be.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Flyer {
    eye: Vec3,
    /// Rotation about `+Y`, measured so that `0` looks down `-Z`.
    yaw: f32,
    /// Rotation away from the horizon, positive up.
    pitch: f32,
    held: Held,
    /// Whether any key has moved it since it was built — what the debug panel
    /// reports, so "free-fly, and nobody has flown it" is distinguishable from
    /// "this is the golden pose".
    moved: bool,
}

impl Flyer {
    /// A flyer standing where `camera` stands and looking where it looks.
    ///
    /// Starting at the golden pose is what makes `--camera free` and
    /// `--camera fixed` the same first frame: a reviewer comparing a windowed
    /// run against a golden is comparing the same picture until they press a
    /// key.
    #[must_use]
    pub fn at(camera: &Camera) -> Self {
        let forward = (camera.target - camera.eye).normalize_or_zero();
        Self {
            eye: camera.eye,
            // `atan2(x, -z)`, not `atan2(z, x)`: yaw is measured from `-Z`,
            // which is the direction a zero yaw looks.
            yaw: forward.x.atan2(-forward.z),
            pitch: forward.y.clamp(-1.0, 1.0).asin(),
            held: Held::default(),
            moved: false,
        }
    }

    /// Whether a key has ever moved this camera.
    #[must_use]
    pub const fn has_moved(&self) -> bool {
        self.moved
    }

    /// Where the camera is.
    #[must_use]
    pub const fn eye(&self) -> Vec3 {
        self.eye
    }

    /// Takes a key edge, and says whether it was one of this camera's.
    ///
    /// Returning the answer rather than swallowing everything: the loop hands a
    /// game every key its own three did not claim, and a camera that reported
    /// "mine" for all of them would leave the sample unable to bind anything
    /// else later.
    pub const fn key(&mut self, key: KeyCode, pressed: bool) -> bool {
        let held = &mut self.held;
        let slot = match key {
            KeyCode::KeyW => &mut held.forward,
            KeyCode::KeyS => &mut held.back,
            KeyCode::KeyA => &mut held.left,
            KeyCode::KeyD => &mut held.right,
            KeyCode::Space => &mut held.up,
            KeyCode::ShiftLeft | KeyCode::ShiftRight => &mut held.down,
            KeyCode::ArrowLeft => &mut held.turn_left,
            KeyCode::ArrowRight => &mut held.turn_right,
            KeyCode::ArrowUp => &mut held.look_up,
            KeyCode::ArrowDown => &mut held.look_down,
            _ => return false,
        };
        *slot = pressed;
        true
    }

    /// Lets go of every key.
    ///
    /// Called when the window loses focus: a platform sends no release for a key
    /// that was down when focus went away, so without this the camera keeps
    /// walking into a wall for as long as the window is in the background.
    pub fn release_all(&mut self) {
        self.held = Held::default();
    }

    /// Advances by `dt` seconds of held input.
    pub fn advance(&mut self, dt: f32) {
        let axis = |positive: bool, negative: bool| f32::from(positive) - f32::from(negative);
        // **`turn_right` first, and that order is the bug this had.** Yaw is
        // measured from `-Z` and `ahead` is `(sin_yaw, 0, -cos_yaw)`, so a
        // *rising* yaw swings the view toward `+X` — which is right, as `aside`
        // below asserts by being `cross(ahead, up)` and being driven by
        // `axis(right, left)`. Naming `turn_left` first therefore added to yaw
        // for the left arrow and turned the camera the other way, which reached
        // a user. Both lines now read the same way round: positive argument is
        // the `+X` side.
        let turn = axis(self.held.turn_right, self.held.turn_left) * TURN * dt;
        let tilt = axis(self.held.look_up, self.held.look_down) * TURN * dt;
        self.yaw += turn;
        self.pitch = (self.pitch + tilt).clamp(-PITCH_LIMIT, PITCH_LIMIT);

        // **Walking is on the level plane, and rising is on the world's `+Y`.**
        // Folding the pitch into the walk would make `W` fly downhill whenever
        // the reviewer was looking at the floor, which in a room whose whole
        // subject is the floor is most of the time.
        let (sin_yaw, cos_yaw) = self.yaw.sin_cos();
        let ahead = Vec3::new(sin_yaw, 0.0, -cos_yaw);
        let aside = Vec3::new(cos_yaw, 0.0, sin_yaw);
        let step = ahead * axis(self.held.forward, self.held.back)
            + aside * axis(self.held.right, self.held.left)
            + Vec3::Y * axis(self.held.up, self.held.down);
        let step = step.normalize_or_zero() * SPEED * dt;

        if step != Vec3::ZERO || turn != 0.0 || tilt != 0.0 {
            self.moved = true;
        }
        self.eye += step;
    }

    /// The camera this flyer is, with `projection` taken from the fixed one.
    ///
    /// The projection is the caller's rather than a field here because it is
    /// the *lens*, and the whole point of the pair is that the free camera and
    /// the golden camera see through the same one.
    #[must_use]
    pub fn camera(&self, projection: crcbl::render::Projection) -> Camera {
        let (sin_yaw, cos_yaw) = self.yaw.sin_cos();
        let (sin_pitch, cos_pitch) = self.pitch.sin_cos();
        let forward = Vec3::new(sin_yaw * cos_pitch, sin_pitch, -cos_yaw * cos_pitch);
        Camera {
            eye: self.eye,
            target: self.eye + forward,
            up: Vec3::Y,
            projection,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::room;

    /// **A flyer built from a camera reproduces that camera**, which is what
    /// makes `--camera free`'s first frame the golden's frame.
    ///
    /// Round-tripping through two angles is where a sign or a swapped argument
    /// hides: every candidate mistake produces a camera pointing somewhere
    /// plausible in a room with walls on all sides.
    #[test]
    fn a_flyer_starts_looking_exactly_where_the_fixed_camera_looks() {
        let fixed = room::fixed_camera();
        let flyer = Flyer::at(&fixed);
        let rebuilt = flyer.camera(fixed.projection);
        assert_eq!(rebuilt.eye, fixed.eye);
        let want = (fixed.target - fixed.eye).normalize();
        let got = (rebuilt.target - rebuilt.eye).normalize();
        assert!(
            got.dot(want) > 0.9999,
            "the flyer looks {got:?} where the fixed camera looks {want:?}"
        );
        assert!(!flyer.has_moved(), "building one is not flying it");
    }

    /// Holding a key moves the camera, and letting go stops it.
    ///
    /// The observable is the eye, not the flag: a `moved` bit set by the key
    /// handler rather than by the integration would report movement for a
    /// camera that never went anywhere.
    #[test]
    fn holding_forward_walks_and_releasing_it_stops() {
        let fixed = room::fixed_camera();
        let mut flyer = Flyer::at(&fixed);
        assert!(flyer.key(KeyCode::KeyW, true), "W is the camera's");
        flyer.advance(0.5);
        let walked = flyer.eye();
        assert_ne!(walked, fixed.eye, "a held key did not move the camera");
        assert!(flyer.has_moved());

        flyer.key(KeyCode::KeyW, false);
        flyer.advance(0.5);
        assert_eq!(flyer.eye(), walked, "a released key kept walking");

        // And losing focus drops a key nobody will send a release for.
        flyer.key(KeyCode::KeyW, true);
        flyer.release_all();
        flyer.advance(0.5);
        assert_eq!(
            flyer.eye(),
            walked,
            "a key held across a focus loss kept walking"
        );
    }

    /// Walking is level: looking at the floor and pressing forward walks
    /// forward rather than down.
    #[test]
    fn walking_forward_does_not_follow_the_pitch_into_the_floor() {
        let fixed = room::fixed_camera();
        let mut flyer = Flyer::at(&fixed);
        flyer.key(KeyCode::ArrowDown, true);
        flyer.advance(1.0);
        flyer.key(KeyCode::ArrowDown, false);
        let looking_down = flyer.eye();

        flyer.key(KeyCode::KeyW, true);
        flyer.advance(1.0);
        assert!(
            (flyer.eye().y - looking_down.y).abs() < 1e-5,
            "walking while looking down changed the height: {:?} -> {:?}",
            looking_down,
            flyer.eye(),
        );
        // Space is what changes it.
        flyer.key(KeyCode::KeyW, false);
        flyer.key(KeyCode::Space, true);
        flyer.advance(1.0);
        assert!(flyer.eye().y > looking_down.y, "space did not rise");
    }

    /// The pitch cannot reach vertical, where the view matrix is degenerate and
    /// the frame comes back empty.
    #[test]
    fn the_pitch_stops_short_of_straight_up_and_straight_down() {
        for (key, sign) in [(KeyCode::ArrowUp, 1.0f32), (KeyCode::ArrowDown, -1.0)] {
            let mut flyer = Flyer::at(&room::fixed_camera());
            flyer.key(key, true);
            // Far more turning than the limit is away, so a missing clamp runs
            // past vertical and comes back the other side.
            for _ in 0..200 {
                flyer.advance(0.1);
            }
            let camera = flyer.camera(room::fixed_camera().projection);
            let forward = (camera.target - camera.eye).normalize();
            assert!(
                forward.y * sign > 0.9 && forward.y.abs() < 1.0,
                "{key:?} reached {forward:?}, which is a degenerate view"
            );
            let level = Vec3::new(forward.x, 0.0, forward.z).length();
            assert!(level > 0.01, "{key:?} left no horizontal component at all");
        }
    }

    /// **The arrow that says right turns right**, which shipped inverted.
    ///
    /// The pitch axis beside this one has had
    /// [`the_pitch_stops_short_of_straight_up_and_straight_down`] asserting its
    /// *sign* since it was written; yaw had nothing, so
    /// `axis(turn_left, turn_right)` — whose positive argument is the left key —
    /// added to a yaw that rises toward `+X` and reached a user turning the
    /// camera the wrong way.
    ///
    /// The claim is made against the camera's **own** starting basis rather than
    /// against a world axis, so it holds at every yaw and needs no arithmetic
    /// about where `room::fixed_camera` happens to look: after turning right the
    /// new forward leans toward the old *right* vector, and after turning left,
    /// away from it. A test comparing `yaw` would pass on a camera that stored
    /// the angle correctly and built its basis backwards.
    #[test]
    fn the_turn_arrows_swing_the_view_toward_the_side_they_name() {
        for (key, sign) in [(KeyCode::ArrowRight, 1.0f32), (KeyCode::ArrowLeft, -1.0)] {
            let mut flyer = Flyer::at(&room::fixed_camera());
            let before = flyer.camera(room::fixed_camera().projection);
            let start = (before.target - before.eye).normalize();
            // `cross(forward, up)` is the right-hand side in this engine's
            // right-handed, `+Y` up, `-Z` forward world — the same vector
            // `Flyer::advance` calls `aside` and strafes along.
            let right = start.cross(Vec3::Y).normalize();

            flyer.key(key, true);
            // A tenth of a radian or so: far enough that the sign is not
            // floating-point noise, short enough that the view cannot swing past
            // the perpendicular and make a wrong sign read as a right one.
            flyer.advance(0.05);

            let after = flyer.camera(room::fixed_camera().projection);
            let turned = (after.target - after.eye).normalize();
            let leaned = turned.dot(right);
            assert!(
                leaned * sign > 0.01,
                "{key:?} moved the view {leaned} of the way toward the camera's own right \
                 vector {right:?}, and the sign says it turned the other way"
            );
            assert!(
                turned.dot(start) > 0.99,
                "{key:?} swung {turned:?} from {start:?} in one step, which is too far for the \
                 sign above to mean what it says"
            );
        }
    }

    /// A key this camera does not bind is reported as somebody else's, so the
    /// sample can grow one later without the camera having swallowed it.
    #[test]
    fn an_unbound_key_is_not_claimed() {
        let mut flyer = Flyer::at(&room::fixed_camera());
        assert!(!flyer.key(KeyCode::KeyQ, true));
        flyer.advance(1.0);
        assert!(!flyer.has_moved());
    }
}
