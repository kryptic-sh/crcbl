//! Mouse and keyboard to camera motion: this application's half of
//! [`OrbitCamera`].
//!
//! ```text
//! left drag            ─▶ orbit   (the model follows the pointer)
//! middle / right drag  ─▶ pan
//! wheel                ─▶ zoom
//! F                    ─▶ frame the model again
//! ```
//!
//! [`OrbitCamera`] fixes the geometry and deliberately names no key, no button
//! and no modifier — its module docs say why, and the short version is that the
//! DCC tools disagree about which drag turns which way. So the gesture is here,
//! and the bindings are the ones a user arriving from Sketchfab, `three.js`'s
//! `OrbitControls` or a browser model viewer already has in their hands: the
//! primary button turns the object, the other two slide it, the wheel moves in
//! and out.
//!
//! # The model follows the pointer, so both drag deltas are negated
//!
//! A positive yaw delta swings the *eye* to the camera's own right, which turns
//! the *view* left. Grab the front of an object and drag it right and the object
//! must turn right, so the eye has to go left: the yaw delta is `-dx`. The same
//! argument in the other axis is why dragging *down* lifts the eye — pulling the
//! top of the object toward you is looking down on it — which is `+dy` in screen
//! coordinates, where `y` grows downward. Panning is the same reasoning once
//! more: [`OrbitCamera::pan`] moves the pivot, so the scene appears to move the
//! other way, and a grab-and-drag negates it.
//!
//! Every one of those four signs is asserted below against the camera the
//! mapping produced, never against the delta it passed in — a mapping that
//! negates twice and a mapping that negates neither agree about the sign of a
//! stored number and disagree about where the eye ends up.
//!
//! # Deltas come from `raw_delta` where there is one, and from `abs` where there
//! is not
//!
//! [`ShellEvent::PointerMotion`] carries both and documents that a camera must
//! prefer the unaccelerated one, because differencing absolute positions stops
//! working when the cursor reaches the edge of the display. That is exactly
//! right for a drag that can run off the window, so `raw_delta` is used
//! wherever the backend has one — and where it has none
//! ([`ShellCaps::RAW_POINTER_MOTION`](crcbl::shell::ShellCaps::RAW_POINTER_MOTION)
//! is absent, which is what the browser backend reports) the difference of
//! successive `abs` positions is the only signal there is, so it is used rather
//! than the drag silently doing nothing.

use crcbl::core::input::{ButtonState, KeyCode, PointerButton, ScrollDelta};
use crcbl::math::Vec2;
use crcbl::render::OrbitCamera;
use crcbl::shell::ShellEvent;

/// How far a drag across the window's full height turns the camera, in radians.
///
/// A little over half a turn, so a model can be inspected from behind without
/// letting go and a small drag is still a small change. `three.js`'s
/// `OrbitControls` uses a full turn across the height for azimuth and half a
/// turn for elevation; one constant for both is what keeps a diagonal drag
/// feeling like one gesture instead of two at different rates.
const ORBIT_RADIANS_PER_HEIGHT: f32 = core::f32::consts::PI * 1.2;

/// How far one wheel detent moves the eye, as [`OrbitCamera::zoom`]'s
/// multiplicative delta.
///
/// One notch covers about a tenth of the way to the pivot — `e^-0.1` — so ten
/// notches roughly a third of the distance, at any scale. Small enough that a
/// notch is a nudge rather than a jump, large enough that crossing a model's
/// whole depth is a flick of the finger.
const ZOOM_PER_DETENT: f32 = 0.1;

/// How many pixels of touchpad scroll count as one wheel detent.
///
/// [`ScrollDelta`] deliberately does not collapse its two variants and says the
/// conversion between them is a *policy* decision belonging to the application —
/// so here it is, and this application is the first thing in the workspace to
/// need one. The number is the browsers' own `DOM_DELTA_LINE` conversion, which
/// is what the touchpads of the world have been tuned against.
const PIXELS_PER_DETENT: f64 = 53.0;

/// Which button starts which drag.
///
/// Both non-primary buttons pan, because half the world's mice put the wheel
/// click where the other half puts the context menu, and neither does anything
/// else in a viewer with no context menu.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Drag {
    /// The primary button: turn the model.
    Orbit,
    /// The middle or secondary button: slide it.
    Pan,
}

/// What the input asked the application for, beyond moving the camera.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Request {
    /// Nothing but what has already been applied to the camera.
    Nothing,
    /// Fit the model in the view again — the `F` key.
    ///
    /// Not applied here because framing needs the model's bounds and the
    /// window's aspect, neither of which is input.
    Reframe,
}

/// The pointer's state between events: which drag is running, and where the
/// cursor was.
#[derive(Clone, Copy, Debug, Default)]
pub struct Controls {
    /// The drag in progress, or `None` when no button is down.
    drag: Option<Drag>,
    /// Where the pointer was last seen, in window pixels.
    ///
    /// Only used on a backend with no relative motion — see the [module
    /// docs](self) — and cleared whenever the pointer leaves, so that walking
    /// out of one edge and back in at another is not one enormous drag.
    last_abs: Option<Vec2>,
}

impl Controls {
    /// Nothing held, nothing dragging.
    #[must_use]
    pub const fn new() -> Self {
        Self {
            drag: None,
            last_abs: None,
        }
    }

    /// Whether a drag is in progress.
    #[must_use]
    pub const fn is_dragging(&self) -> bool {
        self.drag.is_some()
    }

    /// Folds one shell event into `camera`, and says what else it asked for.
    ///
    /// `extent` is the window's size in pixels; the height is what both drag
    /// gestures are measured against, so the same movement of the hand does the
    /// same thing whatever size the window is.
    pub fn apply(
        &mut self,
        event: &ShellEvent,
        extent: (u32, u32),
        camera: &mut OrbitCamera,
    ) -> Request {
        match event {
            ShellEvent::Button { button, state, .. } => {
                self.button(*button, *state);
            }
            ShellEvent::PointerMotion { abs, raw_delta, .. } => {
                let moved = self.motion(*abs, *raw_delta);
                if let Some(moved) = moved {
                    self.drag(moved, extent, camera);
                }
            }
            // A pointer that has left has no position, so the next one it
            // reports is not a movement from the last one.
            ShellEvent::PointerFocus { entered: false, .. } => self.last_abs = None,
            // Focus loss is the release no platform sends. Without this a drag
            // survives an alt-tab and resumes from wherever the pointer comes
            // back, which is a model that leaps when the window is clicked on.
            ShellEvent::Focus { focused: false, .. } => {
                self.drag = None;
                self.last_abs = None;
            }
            ShellEvent::Wheel { delta, .. } => camera.zoom(detents(*delta) * ZOOM_PER_DETENT),
            ShellEvent::Key {
                key_code: Some(KeyCode::KeyF),
                state: ButtonState::Pressed,
                repeat: false,
                ..
            } => return Request::Reframe,
            _ => {}
        }
        Request::Nothing
    }

    /// Starts or ends a drag.
    fn button(&mut self, button: PointerButton, state: ButtonState) {
        let drag = match button {
            PointerButton::Left => Some(Drag::Orbit),
            PointerButton::Middle | PointerButton::Right => Some(Drag::Pan),
            _ => None,
        };
        let Some(drag) = drag else { return };
        match state {
            ButtonState::Pressed => self.drag = Some(drag),
            // Only the button that started the drag ends it, so releasing the
            // wheel click mid-orbit does not stop the orbit.
            ButtonState::Released if self.drag == Some(drag) => self.drag = None,
            ButtonState::Released => {}
        }
    }

    /// How far the pointer moved, in window pixels, or `None` when this event
    /// carries no usable movement.
    fn motion(
        &mut self,
        abs: Option<crcbl::shell::PhysicalPoint>,
        raw: Option<(f64, f64)>,
    ) -> Option<Vec2> {
        let here = abs.map(|point| Vec2::new(point.x as f32, point.y as f32));
        // Remembered whenever there is a position to remember, and left alone
        // when there is not: `PointerMode::Locked` reports no `abs` at all, and
        // forgetting the last one there would make the first unlocked motion a
        // jump from wherever the pointer used to be.
        let previous = match here {
            Some(here) => self.last_abs.replace(here),
            None => None,
        };
        // The unaccelerated delta wherever the backend has one, whether or not
        // an absolute position came with it.
        if let Some((dx, dy)) = raw {
            return Some(Vec2::new(dx as f32, dy as f32));
        }
        Some(here? - previous?)
    }

    /// Applies `moved` pixels of pointer movement to whichever drag is running.
    fn drag(&self, moved: Vec2, extent: (u32, u32), camera: &mut OrbitCamera) {
        let Some(drag) = self.drag else { return };
        // A window can report a zero height while minimised, and dividing by it
        // would hand `OrbitCamera` an infinity, which it asserts against.
        if extent.1 == 0 {
            return;
        }
        let fraction = moved / extent.1 as f32;
        match drag {
            // Negated in both axes: the model follows the pointer. See the
            // module docs.
            Drag::Orbit => camera.orbit(
                -fraction.x * ORBIT_RADIANS_PER_HEIGHT,
                fraction.y * ORBIT_RADIANS_PER_HEIGHT,
            ),
            // And here too, for the same reason: moving the pivot right moves
            // the scene left.
            Drag::Pan => camera.pan(-fraction.x, fraction.y),
        }
    }
}

/// A scroll in wheel detents, positive away from the user.
///
/// Only the vertical axis: a horizontal scroll is a sideways gesture with no
/// meaning in a turntable, and mapping it to anything would be inventing a
/// binding rather than honouring one.
fn detents(delta: ScrollDelta) -> f32 {
    match delta {
        ScrollDelta::Lines { y, .. } => y,
        ScrollDelta::Pixels { y, .. } => (y / PIXELS_PER_DETENT) as f32,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crcbl::math::Vec3;
    use crcbl::render::{Camera, Projection};
    use crcbl::shell::{
        HeadlessShell, LogicalSize, PhysicalPoint, Shell, ShellCaps, WindowDesc, WindowId,
    };

    /// The window every gesture below is played into.
    const EXTENT: (u32, u32) = (800, 600);

    /// A scripted session: a real [`HeadlessShell`] producing real
    /// [`ShellEvent`]s, folded into a real [`OrbitCamera`].
    ///
    /// The events are the backend's rather than struct literals written here,
    /// which is the whole point — a mapping tested against events this file
    /// invented is a mapping tested against this file's idea of the shell.
    struct Script {
        shell: HeadlessShell,
        window: WindowId,
        controls: Controls,
        camera: OrbitCamera,
        at: PhysicalPoint,
    }

    impl Script {
        /// A session on a shell reporting `caps`.
        fn on(caps: ShellCaps) -> Self {
            let mut shell = HeadlessShell::new().with_caps(caps);
            let window = shell
                .create_window(&WindowDesc {
                    title: "controls",
                    app_id: "sh.kryptic.crcbl.viewer.tests",
                    size: LogicalSize::new(EXTENT.0 as f64, EXTENT.1 as f64),
                    ..WindowDesc::default()
                })
                .expect("the headless shell always makes a window");
            Self {
                shell,
                window,
                controls: Controls::new(),
                camera: OrbitCamera::new(Vec3::ZERO, 4.0, Projection::default()),
                at: PhysicalPoint::ORIGIN,
            }
        }

        /// The ordinary case: a desktop backend with unaccelerated motion.
        fn new() -> Self {
            Self::on(ShellCaps::DESKTOP)
        }

        /// Drains everything queued into the mapping, and hands back what the
        /// last event asked for.
        fn pump(&mut self) -> Request {
            let (controls, camera) = (&mut self.controls, &mut self.camera);
            let mut request = Request::Nothing;
            self.shell.pump(&mut |event| {
                request = controls.apply(&event, EXTENT, camera);
            });
            request
        }

        fn press(&mut self, button: PointerButton) -> &mut Self {
            self.shell
                .button(self.window, button, ButtonState::Pressed, Some(self.at))
                .expect("the window is live");
            self.pump();
            self
        }

        fn release(&mut self, button: PointerButton) -> &mut Self {
            self.shell
                .button(self.window, button, ButtonState::Released, Some(self.at))
                .expect("the window is live");
            self.pump();
            self
        }

        /// Moves the pointer by `(dx, dy)` window pixels.
        fn moved(&mut self, dx: f64, dy: f64) -> &mut Self {
            self.at = PhysicalPoint::new(self.at.x + dx, self.at.y + dy);
            self.shell
                .move_pointer(self.window, self.at, (dx, dy))
                .expect("the window is live");
            self.pump();
            self
        }

        fn wheel(&mut self, lines: f32) -> &mut Self {
            self.shell
                .scroll(self.window, ScrollDelta::Lines { x: 0.0, y: lines }, None)
                .expect("the window is live");
            self.pump();
            self
        }

        fn camera(&self) -> Camera {
            self.camera.camera()
        }
    }

    /// The camera's own right vector, from the [`Camera`] rather than from the
    /// controller — so a claim about "the camera's right" is a claim about the
    /// basis the view matrix is built from.
    fn right_of(camera: &Camera) -> Vec3 {
        (camera.target - camera.eye)
            .normalize()
            .cross(Vec3::Y)
            .normalize()
    }

    /// **Dragging right with the primary button turns the model right**, which
    /// is the eye going to its own left.
    ///
    /// Asserted against the camera's own basis rather than a world axis, so it
    /// holds from any starting pose — and the loop runs it again from an
    /// orbited one, because a mapping that got the sign right only at yaw zero
    /// is wrong everywhere else.
    #[test]
    fn a_rightward_drag_swings_the_eye_to_the_cameras_own_left() {
        for (start_yaw, start_pitch) in [(0.0f32, 0.0f32), (2.1, 0.5), (-1.3, -0.4)] {
            let mut script = Script::new();
            script.camera.orbit(start_yaw, start_pitch);

            let before = script.camera();
            let right = right_of(&before);
            script.press(PointerButton::Left).moved(40.0, 0.0);
            let after = script.camera();

            let leaned = (after.eye - before.eye).normalize().dot(right);
            assert!(
                leaned < -0.9,
                "a rightward drag from ({start_yaw}, {start_pitch}) moved the eye {leaned} \
                 along its own right, so the model turned the wrong way",
            );
            assert_eq!(
                after.target, before.target,
                "an orbit does not move the pivot"
            );
        }
    }

    /// **Dragging down lifts the eye**, which is looking down on the model —
    /// the other half of "the model follows the pointer".
    #[test]
    fn a_downward_drag_lifts_the_eye_above_the_pivot() {
        let mut script = Script::new();
        script.press(PointerButton::Left).moved(0.0, 60.0);
        assert!(
            script.camera().eye.y > 0.0,
            "the eye is at {:?} and should be above the pivot",
            script.camera().eye,
        );

        // And back the other way, so the assertion above is about the sign and
        // not about a clamp.
        script.moved(0.0, -120.0);
        assert!(script.camera().eye.y < 0.0, "{:?}", script.camera().eye);
    }

    /// **The non-primary buttons pan, and the scene follows the pointer.**
    ///
    /// A rightward pan drag moves the pivot to the camera's own left, so what
    /// is on screen slides right with the hand. Both buttons are checked
    /// because binding one and forgetting the other is a drag that silently
    /// does nothing on half the mice in the world.
    #[test]
    fn the_middle_and_right_buttons_slide_the_pivot_against_the_drag() {
        for button in [PointerButton::Middle, PointerButton::Right] {
            let mut script = Script::new();
            let right = right_of(&script.camera());
            let before = script.camera.pivot();

            script.press(button).moved(50.0, 0.0);
            let slid = script.camera.pivot() - before;

            assert!(
                slid.dot(right) < 0.0,
                "{button:?}: the pivot moved {slid:?}, which is not against the drag",
            );
            assert!(
                (script.camera.distance() - 4.0).abs() < 1e-4,
                "a pan does not change the distance",
            );
        }
    }

    /// A wheel pushed away moves the eye in, and pulled back moves it out.
    ///
    /// Both directions, because a zoom wired to the wrong sign still zooms.
    #[test]
    fn the_wheel_moves_the_eye_in_and_out() {
        let mut script = Script::new();
        let start = script.camera.distance();

        script.wheel(1.0);
        assert!(
            script.camera.distance() < start,
            "{} is not nearer than {start}",
            script.camera.distance(),
        );

        script.wheel(-2.0);
        assert!(
            script.camera.distance() > start,
            "{} is not further than {start}",
            script.camera.distance(),
        );
    }

    /// **No button down is no motion**, which is what separates a viewer from a
    /// camera that spins whenever the pointer crosses the window.
    #[test]
    fn moving_the_pointer_with_no_button_down_moves_nothing() {
        let mut script = Script::new();
        let before = script.camera();

        script.moved(80.0, 40.0);
        assert_eq!(script.camera(), before, "a hover is not a drag");

        script
            .press(PointerButton::Left)
            .release(PointerButton::Left);
        assert!(!script.controls.is_dragging());
        script.moved(80.0, 40.0);
        assert_eq!(script.camera(), before, "the drag was released");
    }

    /// **Losing focus ends the drag**, because the release is one no platform
    /// will send. Without it the model leaps the next time the window is
    /// clicked on.
    #[test]
    fn losing_focus_ends_a_drag_in_progress() {
        let mut script = Script::new();
        script.press(PointerButton::Left);
        assert!(script.controls.is_dragging());

        script
            .shell
            .set_focus(script.window, false)
            .expect("the window is live");
        script.pump();
        assert!(!script.controls.is_dragging());

        let before = script.camera();
        script.moved(80.0, 0.0);
        assert_eq!(script.camera(), before);
    }

    /// **A backend with no relative motion still orbits.**
    ///
    /// The browser is that backend, and `ShellCaps` is how it says so. The
    /// first absolute position establishes where the pointer is and moves
    /// nothing — there is no previous one to difference against — and the
    /// second is a drag.
    #[test]
    fn a_backend_with_only_absolute_positions_still_drags() {
        let mut script = Script::on(ShellCaps::DESKTOP - ShellCaps::RAW_POINTER_MOTION);
        script.press(PointerButton::Left);

        let before = script.camera();
        script.moved(60.0, 0.0);
        assert_eq!(
            script.camera(),
            before,
            "the first position is where the pointer is, not how far it went",
        );

        script.moved(60.0, 0.0);
        let leaned = (script.camera().eye - before.eye)
            .normalize()
            .dot(right_of(&before));
        assert!(
            leaned < -0.9,
            "the differenced drag turned the wrong way: {leaned}",
        );
    }

    /// A pointer that leaves and comes back somewhere else is not one huge drag
    /// across the window. Only the differencing path can get this wrong, so it
    /// is the one the case is played on.
    #[test]
    fn a_pointer_that_leaves_and_returns_does_not_jump_the_model() {
        let mut script = Script::on(ShellCaps::DESKTOP - ShellCaps::RAW_POINTER_MOTION);
        script.press(PointerButton::Left).moved(700.0, 300.0);
        script.moved(10.0, 0.0);

        script
            .shell
            .set_pointer_focus(script.window, false, None)
            .expect("the window is live");
        script.pump();

        let before = script.camera();
        script.at = PhysicalPoint::new(10.0, 300.0);
        script
            .shell
            .move_pointer(script.window, script.at, (0.0, 0.0))
            .expect("the window is live");
        script.pump();
        assert_eq!(
            script.camera(),
            before,
            "re-entering the window is not a 700-pixel drag",
        );
    }

    /// `F` asks for a re-frame once per press, and nothing else does.
    #[test]
    fn f_asks_for_a_reframe_and_a_repeat_does_not() {
        let mut script = Script::new();
        script
            .shell
            .key_press(script.window, KeyCode::KeyF)
            .expect("the window is live");
        assert_eq!(script.pump(), Request::Reframe);

        script
            .shell
            .key_repeat(script.window, KeyCode::KeyF)
            .expect("the window is live");
        assert_eq!(
            script.pump(),
            Request::Nothing,
            "a held key must not re-frame every repeat",
        );

        script
            .shell
            .key_press(script.window, KeyCode::KeyG)
            .expect("the window is live");
        assert_eq!(script.pump(), Request::Nothing);
    }

    /// A minimised window reports a zero height, and dividing a drag by it
    /// would hand the camera an infinity — which it asserts against, taking the
    /// process down over a window nobody is even looking at.
    #[test]
    fn a_zero_height_window_does_not_take_the_camera_down() {
        let mut script = Script::new();
        script.press(PointerButton::Left);
        let before = script.camera();

        script.at = PhysicalPoint::new(30.0, 30.0);
        script
            .shell
            .move_pointer(script.window, script.at, (30.0, 30.0))
            .expect("the window is live");
        let (controls, camera) = (&mut script.controls, &mut script.camera);
        script.shell.pump(&mut |event| {
            controls.apply(&event, (0, 0), camera);
        });
        assert_eq!(script.camera(), before);
    }

    /// A touchpad's pixel scroll and a wheel's detents mean the same thing, so
    /// one policy converts between them rather than each call site guessing.
    #[test]
    fn a_pixel_scroll_is_a_fraction_of_a_detent() {
        assert!((detents(ScrollDelta::Lines { x: 0.0, y: 3.0 }) - 3.0).abs() < 1e-6);
        assert!(
            (detents(ScrollDelta::Pixels {
                x: 0.0,
                y: PIXELS_PER_DETENT,
            }) - 1.0)
                .abs()
                < 1e-6,
            "one line of pixels is one detent",
        );
        // A sideways scroll is not a zoom.
        assert!(detents(ScrollDelta::Lines { x: 4.0, y: 0.0 }).abs() < 1e-6);
    }
}
