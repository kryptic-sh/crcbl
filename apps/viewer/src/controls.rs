//! Mouse and keyboard to camera motion: this application's half of
//! [`OrbitCamera`].
//!
//! ```text
//! left drag            ─▶ orbit   (the model follows the pointer)
//! middle / right drag  ─▶ pan
//! wheel                ─▶ zoom
//! F                    ─▶ frame the model again   (crate::app, not here)
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
//! # The engine's input, not the shell's
//!
//! Every method below takes what [`crcbl::engine::HostedGame`] hands a game —
//! [`PointerUpdate`], a [`PointerButton`] edge, a [`ScrollDelta`] — rather than a
//! raw `ShellEvent`. That is the whole of this milestone's engine change: the
//! hosted loop used to fold away every button but the primary one and drop the
//! wheel entirely, so a viewer could not be hosted at all and this file matched
//! on shell events inside a loop of its own.
//!
//! Three things moved out of here and into the loop as a result, and none of
//! them is gone:
//!
//! * **The last-position bookkeeping.** [`PointerUpdate::motion`] is the
//!   unaccelerated delta where the backend has one and the difference of
//!   successive positions where it does not; `crcbl::engine::Pending` carries
//!   the position across frames and drops it when the pointer leaves, which is
//!   what stops walking out of one edge and back in at another from arriving as
//!   one enormous drag.
//! * **Focus loss.** The loop releases every held key and every held pointer
//!   button when the window goes away, so the release that no platform sends
//!   arrives here as an ordinary one and a drag cannot survive an alt-tab.
//! * **`F`.** Framing needs the model's bounds and the window's aspect, neither
//!   of which is input, so it lives in [`crate::app`] beside them.
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

use crcbl::core::input::{PointerButton, ScrollDelta};
use crcbl::engine::PointerUpdate;
use crcbl::math::Vec2;
use crcbl::render::OrbitCamera;

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

/// Which drag is running, and nothing else.
///
/// One field, where this used to carry the last pointer position too — see the
/// [module docs](self) for where that went.
#[derive(Clone, Copy, Debug, Default)]
pub struct Controls {
    /// The drag in progress, or `None` when no button is down.
    drag: Option<Drag>,
}

impl Controls {
    /// Nothing held, nothing dragging.
    #[must_use]
    pub const fn new() -> Self {
        Self { drag: None }
    }

    /// Whether a drag is in progress.
    #[must_use]
    pub const fn is_dragging(&self) -> bool {
        self.drag.is_some()
    }

    /// The primary button's edges and the pointer's movement.
    ///
    /// `extent` is the window's size in pixels; the height is what both drag
    /// gestures are measured against, so the same movement of the hand does the
    /// same thing whatever size the window is.
    ///
    /// **The press is taken before the motion**, because that is the order the
    /// hand made them: a batch that both starts a drag and moves must apply its
    /// movement, or the first frame of every drag is dropped. The loop dispatches
    /// [`button`](Self::button) before this for the same reason.
    pub fn pointer(
        &mut self,
        pointer: PointerUpdate,
        extent: (u32, u32),
        camera: &mut OrbitCamera,
    ) {
        if pointer.pressed {
            self.drag = Some(Drag::Orbit);
        }
        // Only the button that started the drag ends it, so releasing the
        // primary button mid-pan does not stop the pan.
        if pointer.released && self.drag == Some(Drag::Orbit) {
            self.drag = None;
        }
        if let Some(moved) = pointer.motion {
            self.apply_drag(moved, extent, camera);
        }
    }

    /// A non-primary button's edge: the pan drag's press and release.
    pub fn button(&mut self, button: PointerButton, pressed: bool) {
        // `Left` never arrives here — the loop delivers the primary button as
        // `PointerUpdate`'s two edges, because it is the one a menu arbitrates —
        // and the thumb buttons are not bound to anything.
        if !matches!(button, PointerButton::Middle | PointerButton::Right) {
            return;
        }
        if pressed {
            self.drag = Some(Drag::Pan);
        } else if self.drag == Some(Drag::Pan) {
            self.drag = None;
        }
    }

    /// One scroll: in and out along the view direction.
    pub fn wheel(delta: ScrollDelta, camera: &mut OrbitCamera) {
        camera.zoom(detents(delta) * ZOOM_PER_DETENT);
    }

    /// Applies `moved` pixels of pointer movement to whichever drag is running.
    fn apply_drag(&self, moved: Vec2, extent: (u32, u32), camera: &mut OrbitCamera) {
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

    /// The window every gesture below is played into.
    const EXTENT: (u32, u32) = (800, 600);

    /// A scripted session: the updates the engine's loop hands a hosted game,
    /// folded into a real [`OrbitCamera`].
    ///
    /// The gestures reach the *loop* in `crate::app`'s tests, which is what says
    /// the wiring exists; this checks what the mapping does with them.
    struct Script {
        controls: Controls,
        camera: OrbitCamera,
    }

    impl Script {
        fn new() -> Self {
            Self {
                controls: Controls::new(),
                camera: OrbitCamera::new(Vec3::ZERO, 4.0, Projection::default()),
            }
        }

        fn press(&mut self, button: PointerButton) -> &mut Self {
            match button {
                PointerButton::Left => self.controls.pointer(
                    PointerUpdate {
                        pressed: true,
                        ..PointerUpdate::default()
                    },
                    EXTENT,
                    &mut self.camera,
                ),
                other => self.controls.button(other, true),
            }
            self
        }

        fn release(&mut self, button: PointerButton) -> &mut Self {
            match button {
                PointerButton::Left => self.controls.pointer(
                    PointerUpdate {
                        released: true,
                        ..PointerUpdate::default()
                    },
                    EXTENT,
                    &mut self.camera,
                ),
                other => self.controls.button(other, false),
            }
            self
        }

        /// Moves the pointer by `(dx, dy)` window pixels.
        fn moved(&mut self, dx: f32, dy: f32) -> &mut Self {
            self.moved_in(EXTENT, dx, dy)
        }

        /// The same, in a window of some other size.
        fn moved_in(&mut self, extent: (u32, u32), dx: f32, dy: f32) -> &mut Self {
            self.controls.pointer(
                PointerUpdate {
                    motion: Some(Vec2::new(dx, dy)),
                    ..PointerUpdate::default()
                },
                extent,
                &mut self.camera,
            );
            self
        }

        fn wheel(&mut self, lines: f32) -> &mut Self {
            Controls::wheel(ScrollDelta::Lines { x: 0.0, y: lines }, &mut self.camera);
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

    /// **A drag started with one button is not ended by another**, which is
    /// what lets a hand rest on the wheel click through an orbit.
    #[test]
    fn only_the_button_that_started_a_drag_ends_it() {
        let mut script = Script::new();
        script.press(PointerButton::Left);
        script.release(PointerButton::Middle);
        assert!(script.controls.is_dragging(), "the orbit was cancelled");

        let before = script.camera();
        script.moved(40.0, 0.0);
        assert_ne!(script.camera(), before, "the orbit stopped following");

        script.release(PointerButton::Left);
        assert!(!script.controls.is_dragging());
    }

    /// **The release the loop synthesises after a focus loss ends the drag.**
    ///
    /// This used to be a `ShellEvent::Focus` arm here; it is the engine's now —
    /// see the [module docs](self) — and what arrives is an ordinary release. A
    /// drag that survived it would resume from wherever the pointer came back,
    /// which is a model that leaps when the window is clicked on.
    #[test]
    fn the_loops_synthesised_release_ends_a_drag_in_progress() {
        for button in [
            PointerButton::Left,
            PointerButton::Middle,
            PointerButton::Right,
        ] {
            let mut script = Script::new();
            script.press(button);
            assert!(script.controls.is_dragging());

            script.release(button);
            assert!(!script.controls.is_dragging(), "{button:?} kept dragging");

            let before = script.camera();
            script.moved(80.0, 0.0);
            assert_eq!(script.camera(), before);
        }
    }

    /// A minimised window reports a zero height, and dividing a drag by it
    /// would hand the camera an infinity — which it asserts against, taking the
    /// process down over a window nobody is even looking at.
    #[test]
    fn a_zero_height_window_does_not_take_the_camera_down() {
        let mut script = Script::new();
        script.press(PointerButton::Left);
        let before = script.camera();

        script.moved_in((0, 0), 30.0, 30.0);
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
