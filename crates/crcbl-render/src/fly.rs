//! The free-fly camera: keys and mouse deltas in, a [`Camera`] out.
//!
//! ```text
//! key(code, pressed) ─┐
//! advance(dt) ────────┼─▶ Flyer { eye, yaw, pitch, speed } ─▶ Camera
//! look(motion) ───────┘
//! ```
//!
//! `docs/plan/sample/13-lumen.md`'s Scope asks for "a free-fly camera, a fixed
//! camera set for goldens". The two are the same [`Camera`] type and the same
//! projection — what differs is whether anything moves it — so this module is
//! the moving half, and the pose it starts at is the app's to choose. It is here
//! beside [`OrbitCamera`](crate::OrbitCamera) rather than in a sample for the
//! same reason that one is: it is arithmetic, it needs no device, and every one
//! of its claims is a unit test.
//!
//! # Why it is stepped on the fixed timestep
//!
//! [`Flyer::advance`] is meant to be called from `tick`, not from `draw`. A
//! camera integrated on the frame rate flies at a speed proportional to how fast
//! the machine is, which makes "walk to the corner and look at the contact
//! shadow" a different journey on every machine — and it makes a headless run's
//! camera a function of the frame budget rather than of the clock.
//!
//! # The mouse and the arrow keys both turn it
//!
//! [`Flyer::look`] is driven by `PointerUpdate::motion` — the unaccelerated
//! delta — and never by differencing positions: an absolute position is clamped
//! at the edge of the display and has pointer acceleration already applied, so a
//! look built from one stops turning exactly when the cursor runs out of screen.
//! The pointer is captured for it, which the app's `HostedGame::pointer_mode` is
//! what asks for; a look that turned while a *visible* cursor walked out of the
//! window would click on whatever is behind it, so [`Flyer::look`] should be
//! called only on the frames that capture produces.
//!
//! The arrow keys stay, and are not a fallback: they are what a reviewer with no
//! mouse hand free uses to nudge the framing a few degrees, and they are the
//! only turn a shell without pointer lock has.

use core::f32::consts::{FRAC_PI_2, PI};

use crcbl_core::input::KeyCode;
use glam::{Vec2, Vec3};

use crate::camera::{Camera, Projection};

/// How fast the camera walks by default, in metres a second.
///
/// A brisk walk rather than a fly-through speed, because the scene a fly camera
/// is pointed at is usually a room-sized one: a camera that crossed a
/// six-metre room in a second is one a reviewer overshoots every time they try
/// to stand in the corner. A scene measured in hundreds of metres wants more,
/// and [`Flyer::with_speed`] is how it says so.
pub const SPEED: f32 = 2.4;

/// How fast the arrow keys turn it, in radians a second.
pub const TURN: f32 = 1.6;

/// How far the view turns per pixel of mouse movement, in radians.
///
/// A tenth of a degree a pixel, written as that arithmetic so the number and
/// the sentence cannot drift apart. It is a *rate* rather than a scale: the
/// units are `PointerUpdate::motion`'s — framebuffer pixels — so the same
/// movement of the hand turns the same amount whatever size the window is, and
/// no aspect ratio enters into it.
///
/// Where the value comes from: this is the low end of what a first-person game
/// ships as its default, and a lighting fixture wants the slow end. A reviewer's
/// job here is to hold a highlight in frame and walk around it, not to flick
/// onto a target.
pub const LOOK: f32 = 0.1 * (PI / 180.0);

/// How far the pitch may go from level, in radians.
///
/// Just short of straight up and straight down: at exactly vertical the
/// forward vector is parallel to `up` and the view matrix is degenerate, which
/// arrives as a frame of nothing.
const PITCH_LIMIT: f32 = FRAC_PI_2 - 0.05;

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
    /// How fast a held movement key walks, in metres a second.
    speed: f32,
    held: Held,
    /// Whether any key has moved it since it was built — what a debug panel
    /// reports, so "free-fly, and nobody has flown it" is distinguishable from
    /// "this is the golden pose".
    moved: bool,
}

impl Flyer {
    /// A flyer standing where `camera` stands and looking where it looks, at
    /// [`SPEED`].
    ///
    /// Starting at the golden pose is what makes a sample's free camera and its
    /// fixed camera the same first frame: a reviewer comparing a windowed run
    /// against a golden is comparing the same picture until they press a key.
    #[must_use]
    pub fn at(camera: &Camera) -> Self {
        let forward = (camera.target - camera.eye).normalize_or_zero();
        Self {
            eye: camera.eye,
            // `atan2(x, -z)`, not `atan2(z, x)`: yaw is measured from `-Z`,
            // which is the direction a zero yaw looks.
            yaw: forward.x.atan2(-forward.z),
            pitch: forward.y.clamp(-1.0, 1.0).asin(),
            speed: SPEED,
            held: Held::default(),
            moved: false,
        }
    }

    /// The same flyer walking at `speed` metres a second instead of [`SPEED`].
    ///
    /// The default is sized for a room, and a scene that is not room-sized needs
    /// its own number: a camera that takes a minute and a half to cross the
    /// subject is as unusable as one that overshoots it. The turn rates are not
    /// configurable beside it, because an angle is an angle at every scale.
    #[must_use]
    pub const fn with_speed(mut self, speed: f32) -> Self {
        self.speed = speed;
        self
    }

    /// How fast a held movement key walks it, in metres a second.
    #[must_use]
    pub const fn speed(&self) -> f32 {
        self.speed
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

    /// Turns by one frame's worth of mouse movement.
    ///
    /// `motion` is `PointerUpdate::motion`: framebuffer pixels, **Y down**.
    /// That is why the pitch subtracts where the yaw adds — pushing the mouse
    /// away from the hand is a *negative* Y and has to raise the view — and it
    /// is the one sign in this file that no compiler can check.
    ///
    /// # Not on the fixed timestep, unlike [`advance`](Self::advance)
    ///
    /// A held key is a *rate* and has to be integrated against the clock or the
    /// camera flies at a speed proportional to how fast the machine is. A mouse
    /// delta is already a distance the hand moved: scaling it by `dt` would make
    /// the same sweep turn a different amount on a slow frame, and running it
    /// once per tick would apply one frame's movement two or three times over on
    /// a frame that owed the clock several. So this lands where it arrives, in
    /// the frame, exactly once.
    pub fn look(&mut self, motion: Vec2) {
        if motion == Vec2::ZERO {
            return;
        }
        self.yaw += motion.x * LOOK;
        self.pitch = (self.pitch - motion.y * LOOK).clamp(-PITCH_LIMIT, PITCH_LIMIT);
        self.moved = true;
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
        // the reviewer was looking at the floor, which in an indoor scene is
        // most of the time.
        let (sin_yaw, cos_yaw) = self.yaw.sin_cos();
        let ahead = Vec3::new(sin_yaw, 0.0, -cos_yaw);
        let aside = Vec3::new(cos_yaw, 0.0, sin_yaw);
        let step = ahead * axis(self.held.forward, self.held.back)
            + aside * axis(self.held.right, self.held.left)
            + Vec3::Y * axis(self.held.up, self.held.down);
        let step = step.normalize_or_zero() * self.speed * dt;

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
    pub fn camera(&self, projection: Projection) -> Camera {
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

    /// The pose every test here starts from.
    ///
    /// **Deliberately skew**: the forward vector `(-2.2, -0.7, -6.0)` has a
    /// non-zero component on all three axes and no two of them equal, so it is
    /// neither axis-aligned nor 45°-symmetric. That is what
    /// [`the_turn_arrows_swing_the_view_toward_the_side_they_name`] and
    /// [`the_mouse_swings_the_view_the_way_it_moved`] need to keep their teeth:
    /// against a level camera looking straight down `-Z`, a swapped `atan2`
    /// argument or a flipped sign lands on a pose that is still symmetric and
    /// still passes.
    fn fixture() -> Camera {
        Camera {
            eye: Vec3::new(1.5, 1.6, 4.0),
            target: Vec3::new(-0.7, 0.9, -2.0),
            up: Vec3::Y,
            projection: Projection::default(),
        }
    }

    /// **A flyer built from a camera reproduces that camera**, which is what
    /// makes a sample's free-camera first frame the golden's frame.
    ///
    /// Round-tripping through two angles is where a sign or a swapped argument
    /// hides: every candidate mistake produces a camera pointing somewhere
    /// plausible in a room with walls on all sides.
    #[test]
    fn a_flyer_starts_looking_exactly_where_the_fixed_camera_looks() {
        let fixed = fixture();
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
        let fixed = fixture();
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

    /// **[`SPEED`] is the default and [`Flyer::with_speed`] is how a scene that
    /// is not room-sized replaces it**, both measured in metres walked.
    ///
    /// The observable is the distance a second of held `W` covers, not the
    /// field: an `advance` that read the constant instead of the field would
    /// report the new speed from [`Flyer::speed`] and still walk the old one.
    #[test]
    fn a_second_of_forward_walks_the_flyers_own_speed() {
        let mut standard = Flyer::at(&fixture());
        assert_eq!(standard.speed(), SPEED, "the default is the constant");
        standard.key(KeyCode::KeyW, true);
        standard.advance(1.0);
        let walked = standard.eye().distance(fixture().eye);
        assert!(
            (walked - SPEED).abs() < 1e-4,
            "a second of forward walked {walked} m at the default speed of {SPEED} m/s"
        );

        // Far enough from the default that no rounding could confuse the two,
        // and the scale a hundred-metre subject actually wants.
        let quick = 20.0;
        let mut fast = Flyer::at(&fixture()).with_speed(quick);
        assert_eq!(fast.speed(), quick, "with_speed did not take");
        fast.key(KeyCode::KeyW, true);
        fast.advance(1.0);
        let flown = fast.eye().distance(fixture().eye);
        assert!(
            (flown - quick).abs() < 1e-3,
            "a second of forward walked {flown} m at {quick} m/s"
        );
    }

    /// Walking is level: looking at the floor and pressing forward walks
    /// forward rather than down.
    #[test]
    fn walking_forward_does_not_follow_the_pitch_into_the_floor() {
        let fixed = fixture();
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
            let mut flyer = Flyer::at(&fixture());
            flyer.key(key, true);
            // Far more turning than the limit is away, so a missing clamp runs
            // past vertical and comes back the other side.
            for _ in 0..200 {
                flyer.advance(0.1);
            }
            let camera = flyer.camera(fixture().projection);
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
    /// about where [`fixture`] happens to look: after turning right the new
    /// forward leans toward the old *right* vector, and after turning left, away
    /// from it. A test comparing `yaw` would pass on a camera that stored the
    /// angle correctly and built its basis backwards.
    #[test]
    fn the_turn_arrows_swing_the_view_toward_the_side_they_name() {
        for (key, sign) in [(KeyCode::ArrowRight, 1.0f32), (KeyCode::ArrowLeft, -1.0)] {
            let mut flyer = Flyer::at(&fixture());
            let before = flyer.camera(fixture().projection);
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

            let after = flyer.camera(fixture().projection);
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

    /// **The mouse swings the view the way the hand moved**, which is the same
    /// claim [`the_turn_arrows_swing_the_view_toward_the_side_they_name`] makes
    /// for the arrow keys and the same way of making it: against the camera's
    /// own starting basis, so it holds at every yaw and says nothing about where
    /// [`fixture`] happens to point.
    ///
    /// Both axes, because a look with one sign right and the other inverted is
    /// the ordinary shape of this bug — `motion` is **Y down** while the pitch
    /// is measured positive up, so the two lines in [`Flyer::look`] must not
    /// read the same way round.
    #[test]
    fn the_mouse_swings_the_view_the_way_it_moved() {
        // Fifty pixels, which is five degrees: far enough that the sign is not
        // floating-point noise, short enough that the view cannot swing past the
        // perpendicular and make a wrong sign read as a right one.
        let sweep = 50.0;
        for (motion, toward_right, toward_up) in [
            (Vec2::new(sweep, 0.0), 1.0f32, 0.0f32),
            (Vec2::new(-sweep, 0.0), -1.0, 0.0),
            // Y down, so a *negative* delta is the hand pushing away from the
            // body, which raises the view.
            (Vec2::new(0.0, -sweep), 0.0, 1.0),
            (Vec2::new(0.0, sweep), 0.0, -1.0),
        ] {
            let mut flyer = Flyer::at(&fixture());
            let before = flyer.camera(fixture().projection);
            let start = (before.target - before.eye).normalize();
            let right = start.cross(Vec3::Y).normalize();
            let up = right.cross(start).normalize();

            flyer.look(motion);

            let after = flyer.camera(fixture().projection);
            let turned = (after.target - after.eye).normalize();
            assert!(
                turned.dot(right) * toward_right > 0.01 || toward_right == 0.0,
                "{motion:?} leaned {} toward the camera's own right {right:?}",
                turned.dot(right),
            );
            assert!(
                turned.dot(up) * toward_up > 0.01 || toward_up == 0.0,
                "{motion:?} leaned {} toward the camera's own up {up:?}",
                turned.dot(up),
            );
            assert!(
                turned.dot(start) > 0.99,
                "{motion:?} swung {turned:?} from {start:?} in one step, which is too far for \
                 the signs above to mean what they say"
            );
            assert!(flyer.has_moved(), "a look is a movement");
        }

        // And a frame that reported no movement is not one.
        let mut still = Flyer::at(&fixture());
        still.look(Vec2::ZERO);
        assert!(!still.has_moved());
    }

    /// The mouse cannot flip the view over the top either.
    ///
    /// [`the_pitch_stops_short_of_straight_up_and_straight_down`] makes this
    /// claim for the keys, and the clamp is shared — but the *call* is not, and
    /// a [`Flyer::look`] that assigned the pitch without going through
    /// [`PITCH_LIMIT`] would pass every keyboard test in this file.
    #[test]
    fn the_mouse_pitch_stops_short_of_vertical_too() {
        for sign in [1.0f32, -1.0] {
            let mut flyer = Flyer::at(&fixture());
            // Far more than the limit is away, in one shove and then in many
            // small ones, because a clamp applied to the delta rather than to
            // the accumulated angle survives the first and not the second.
            flyer.look(Vec2::new(0.0, -sign * 100_000.0));
            for _ in 0..200 {
                flyer.look(Vec2::new(0.0, -sign * 50.0));
            }
            let camera = flyer.camera(fixture().projection);
            let forward = (camera.target - camera.eye).normalize();
            assert!(
                forward.y * sign > 0.9 && forward.y.abs() < 1.0,
                "the mouse reached {forward:?}, which is a degenerate view"
            );
            let level = Vec3::new(forward.x, 0.0, forward.z).length();
            assert!(
                level > 0.01,
                "the mouse left no horizontal component at all"
            );
        }
    }

    /// A key this camera does not bind is reported as somebody else's, so an app
    /// can grow one later without the camera having swallowed it.
    #[test]
    fn an_unbound_key_is_not_claimed() {
        let mut flyer = Flyer::at(&fixture());
        assert!(!flyer.key(KeyCode::KeyQ, true));
        flyer.advance(1.0);
        assert!(!flyer.has_moved());
    }
}
