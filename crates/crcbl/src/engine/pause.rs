//! The on-screen pause button, which is the loop's control rather than a
//! game's.
//!
//! # Why this is not each sample's own
//!
//! [`PAUSE_KEY`](super::PAUSE_KEY) is the loop's and never reaches a game, so a
//! device with no keyboard can start a run and then never stop it — and the
//! pause menu is the only place fullscreen and the debug panel are tappable, so
//! all three go with it. Every sample a finger can play therefore wants the
//! same button, in the same corner, appearing under the same condition, and
//! reporting through the same
//! [`take_pending_pause`](super::HostedGame::take_pending_pause). That is one
//! piece of knowledge, and three copies of it would be three places to fix the
//! day the corner or the condition changes.
//!
//! What stays with the sample is the wiring — the four calls in the four
//! methods — because that is where the game's own controls and its own menus
//! are, and neither is this module's business.
//!
//! # It reads contacts, and takes the pointer away from the game
//!
//! The button is hit-tested against **contacts**, not the emulated pointer: a
//! game whose other control is held — horde's stick — owns the primary contact
//! for as long as the thumb is down, and every other finger raises no pointer
//! event at all. A button on the pointer would be unusable at exactly the
//! moment it is wanted.
//!
//! The same tap does still reach the pointer when it is the primary contact,
//! and a game bound to a pointer press would flap or serve on it — the player
//! asked to pause and got a flap as well. So the control also answers
//! [`takes_pointer`](PauseControl::takes_pointer), and a sample with a pointer
//! binding asks that before doing anything with an update. It only ever says
//! yes over the button's own rectangle, and only once the control is on screen:
//! a mouse on a machine nobody has touched cannot lose a click to a button that
//! is not there.
//!
//! A click on a *drawn* button is swallowed and does not pause. The control is
//! for a device with no keyboard; a machine with a mouse has
//! [`PAUSE_KEY`](super::PAUSE_KEY), and firing on both streams would pause
//! twice for one finger.

use super::{PointerUpdate, TouchUpdate, surface_pixels};
use crcbl_ui::draw_list::DrawList;
use crcbl_ui::text::FontAtlas;
use crcbl_ui::touch::{CONTROL_STYLE, TouchButton};
use crcbl_ui::{Button, ButtonState};
use glam::Vec2;

/// The button's size in pixels: a deliberate target for a thumb, which is wider
/// than the label needs.
pub(super) const SIZE: Vec2 = Vec2::new(112.0, 56.0);

/// How far it sits from the surface's top-right corner.
///
/// Read by [`console_button`](super::console_button) too, so the strip's
/// spacing is one number rather than two that could drift apart.
pub(super) const MARGIN: f32 = 12.0;

/// The label, which is also what a test reads out of the frame to find it.
pub const LABEL: &str = "PAUSE";

/// The on-screen pause button: where it is, whether it is there at all, and
/// whether it has been tapped.
///
/// # Where it lives, and when
///
/// **Fixed, in the top-right corner.** It means one specific thing, so it has
/// to be somewhere a player can aim at deliberately — the opposite of
/// [`TouchStick`](crcbl_ui::touch::TouchStick), which floats because a
/// direction has no wrong place to be pushed from. The top-right is the one
/// corner no sample's HUD is already using.
///
/// **On screen only once a contact has arrived**, and not while a panel is up.
/// Not on [`ShellCaps::TOUCH`](crcbl_shell::ShellCaps::TOUCH), which a desktop
/// with a touchscreen also sets: the first finger to land is the only honest
/// evidence that anyone wants a control, a player using a mouse sees no change
/// at all, and no golden frame moves because a headless golden never touches
/// glass.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct PauseControl {
    button: TouchButton,
    /// Whether any contact has ever reached this run — the gate on the button
    /// being drawn at all.
    ///
    /// Set by every contact including the ones refused because a panel is up:
    /// the first thing a phone player touches is the start menu, and a button
    /// that waited for a contact made *after* that would never appear at all.
    touched: bool,
    /// Whether a menu was on screen the last time the loop asked which one to
    /// show.
    ///
    /// **Last frame's, deliberately**: exactly as the loop resolves its own
    /// pointer against last frame's menu, the panel a player put a thumb
    /// through is the one that was on screen when they did it.
    panel_up: bool,
    /// The surface, in pixels, as of the last frame laid out.
    extent: (u32, u32),
    /// Where the button was drawn last frame, and so where a contact this frame
    /// is hit-tested against.
    rect: (Vec2, Vec2),
    /// Where the pointer was last seen, in framebuffer pixels.
    ///
    /// Remembered rather than read per event because [`PointerUpdate::at`] is
    /// `None` on a frame the pointer did not move — and two taps in the same
    /// place is exactly that. A press whose position was forgotten would be a
    /// press the game treats as its own.
    pointer_at: Option<Vec2>,
}

impl PauseControl {
    /// A button nobody has touched, on a surface no frame has sized yet.
    #[must_use]
    pub const fn new() -> Self {
        Self {
            button: TouchButton::new(),
            touched: false,
            panel_up: false,
            extent: (0, 0),
            rect: (Vec2::ZERO, Vec2::ZERO),
            pointer_at: None,
        }
    }

    /// Lays the button out for a surface of this size.
    ///
    /// Called once a frame from the game's draw, which is the only place the
    /// extent is in hand. The rectangle it computes is what the *next* frame's
    /// contacts are hit-tested against, which is why the art and the hit test
    /// come from one [`Button`] rather than two constants that could disagree.
    pub fn layout(&mut self, extent: (u32, u32), atlas: &FontAtlas) {
        self.extent = extent;
        self.rect = button().rect(position(extent), atlas);
    }

    /// Offer one contact to the button, and say whether it took it.
    ///
    /// `false` means "not mine" — a game with other on-screen controls passes it
    /// on to them. A contact that arrives while a panel is up reaches nothing:
    /// the menu owns the screen, and its buttons are pressed through the
    /// contacts the loop routes to it.
    pub fn touch(&mut self, touch: TouchUpdate) -> bool {
        self.touched = true;
        if self.panel_up {
            return false;
        }
        let at = touch.pixels(self.extent);
        self.button.offer(self.rect, touch.contact, touch.phase, at)
    }

    /// Whether this pointer update is the button's rather than the game's.
    ///
    /// A sample with a pointer binding asks this **first** and returns when it
    /// is true, position and edges alike: the finger that is pressing the button
    /// is also the emulated pointer, and a paddle that jumped to the corner or a
    /// bird that flapped on the way to pausing is the same defect.
    ///
    /// Answers `false` for every update on a machine whose glass nobody has
    /// touched, so a mouse never loses a click to an invisible rectangle.
    pub fn takes_pointer(&mut self, pointer: PointerUpdate) -> bool {
        if let Some(at) = pointer.at {
            self.pointer_at = Some(surface_pixels(at, self.extent));
        }
        self.touched
            && !self.panel_up
            && self.pointer_at.is_some_and(|at| {
                at.x >= self.rect.0.x
                    && at.x <= self.rect.1.x
                    && at.y >= self.rect.0.y
                    && at.y <= self.rect.1.y
            })
    }

    /// Tells the control which menu the frame that just drew is showing, and
    /// takes it away while one is.
    ///
    /// A half-pressed button does not fire when a panel opens over it: the
    /// player never finished the press, and the panel is now what their next tap
    /// is aimed at.
    pub const fn set_panel_up(&mut self, panel_up: bool) {
        self.panel_up = panel_up;
        if panel_up {
            self.button.release();
        }
    }

    /// Whether the button was tapped since the loop last asked — the value
    /// [`take_pending_pause`](super::HostedGame::take_pending_pause) returns.
    pub const fn take_fired(&mut self) -> bool {
        self.button.take_fired()
    }

    /// Draws the button, which is **nothing at all** until a finger has arrived
    /// and nothing while a panel is up.
    ///
    /// `panel_up` is **this** frame's answer rather than the one
    /// [`set_panel_up`](Self::set_panel_up) holds: the loop asks a game to draw
    /// before it asks which menu the frame shows, so a control drawn from the
    /// stored flag lingers for a frame over a panel that has just opened and
    /// goes missing for a frame after one closes. Where a *contact* is concerned
    /// the stale answer is the right one, which is why there are two.
    pub fn render(&self, dl: &mut DrawList, atlas: &FontAtlas, panel_up: bool) {
        if !self.touched || panel_up {
            return;
        }
        let state = if self.button.is_held() {
            ButtonState::Pressed
        } else {
            ButtonState::Idle
        };
        button().render(dl, position(self.extent), atlas, &CONTROL_STYLE, state);
    }

    /// Where the button was last laid out, as `(min, max)` in framebuffer
    /// pixels — `(0, 0)`–`(0, 0)` before the first
    /// [`layout`](Self::layout).
    ///
    /// For a game checking that its own HUD and this do not overlap, which is
    /// the one thing about the button's placement only the game can know.
    #[must_use]
    pub const fn rect(&self) -> (Vec2, Vec2) {
        self.rect
    }

    /// The middle of the button on a surface of this size — where a finger has
    /// to land to press it.
    ///
    /// A pure function of the extent, so a test and the browser gate can aim at
    /// it without reading a laid-out control, and so neither has to know which
    /// corner it is measured from.
    #[must_use]
    pub fn centre(extent: (u32, u32)) -> Vec2 {
        position(extent) + SIZE * 0.5
    }
}

/// The button, as a widget: the art and the hit rectangle come from one place,
/// so a tap cannot land somewhere the label is not.
fn button() -> Button {
    Button::new(LABEL).with_fixed_size(SIZE)
}

/// Its top-left corner, inset from the surface's top-right.
fn position(extent: (u32, u32)) -> Vec2 {
    Vec2::new(extent.0 as f32 - MARGIN - SIZE.x, MARGIN)
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use crcbl_core::input::{ContactId, TouchPhase};

    const EXTENT: (u32, u32) = (960, 720);
    const FIRST: ContactId = ContactId(1);
    const SECOND: ContactId = ContactId(2);

    /// A contact at a place on the surface, as the loop delivers it —
    /// normalised, +Y up.
    fn at_pixels(contact: ContactId, phase: TouchPhase, at: Vec2) -> TouchUpdate {
        TouchUpdate {
            contact,
            phase,
            at: Vec2::new(
                at.x / EXTENT.0 as f32 * 2.0 - 1.0,
                1.0 - at.y / EXTENT.1 as f32 * 2.0,
            ),
        }
    }

    /// A pointer that moved to a place on the surface, in the same normalised
    /// units.
    fn pointer_at(at: Vec2) -> PointerUpdate {
        PointerUpdate {
            at: Some(at_pixels(FIRST, TouchPhase::Moved, at).at),
            pressed: true,
            ..PointerUpdate::default()
        }
    }

    /// A control laid out on the fixture's surface with no panel up.
    fn laid_out() -> PauseControl {
        let mut pause = PauseControl::new();
        pause.layout(EXTENT, &FontAtlas::built_in());
        pause
    }

    /// A tap on it fires once, and a press on its way there does not.
    #[test]
    fn a_tap_on_the_button_asks_for_the_pause_once() {
        let mut pause = laid_out();
        let centre = PauseControl::centre(EXTENT);
        assert!(!pause.take_fired(), "nothing has been tapped");

        assert!(pause.touch(at_pixels(FIRST, TouchPhase::Began, centre)));
        assert!(!pause.take_fired(), "a press is not a tap");
        assert!(pause.touch(at_pixels(FIRST, TouchPhase::Ended, centre)));
        assert!(pause.take_fired(), "the tap never reached the loop");
        assert!(!pause.take_fired(), "taken means taken");
    }

    /// A contact somewhere else is not the button's, so a game with other
    /// controls can have it.
    #[test]
    fn a_contact_off_the_button_is_left_for_the_game() {
        let mut pause = laid_out();
        let elsewhere = Vec2::new(120.0, 600.0);
        assert!(!pause.touch(at_pixels(FIRST, TouchPhase::Began, elsewhere)));
        assert!(!pause.touch(at_pixels(FIRST, TouchPhase::Ended, elsewhere)));
        assert!(!pause.take_fired(), "a tap on the field paused the run");
    }

    /// **A panel takes the button away**, and the contacts that land while it is
    /// up are the menu's.
    #[test]
    fn a_panel_takes_the_button_away_and_a_half_press_with_it() {
        let mut pause = laid_out();
        let centre = PauseControl::centre(EXTENT);
        pause.touch(at_pixels(FIRST, TouchPhase::Began, centre));
        pause.set_panel_up(true);
        assert!(!pause.touch(at_pixels(FIRST, TouchPhase::Ended, centre)));
        assert!(
            !pause.take_fired(),
            "a panel opened over a half-press and the lift paused anyway",
        );

        assert!(!pause.touch(at_pixels(SECOND, TouchPhase::Began, centre)));
        assert!(!pause.touch(at_pixels(SECOND, TouchPhase::Ended, centre)));
        assert!(!pause.take_fired(), "a panel's screen fired the button");

        pause.set_panel_up(false);
        pause.touch(at_pixels(SECOND, TouchPhase::Began, centre));
        pause.touch(at_pixels(SECOND, TouchPhase::Ended, centre));
        assert!(pause.take_fired(), "the button never came back");
    }

    /// **Nothing is drawn until a finger arrives**, which is what keeps a
    /// desktop's picture exactly what it was — and no golden frame moving is the
    /// same claim.
    #[test]
    fn an_untouched_run_draws_no_button() {
        let atlas = FontAtlas::built_in();
        let mut pause = laid_out();
        let mut dl = DrawList::new();
        pause.render(&mut dl, &atlas, false);
        assert!(
            dl.is_empty(),
            "a run nobody touched drew {} command(s)",
            dl.len(),
        );

        // Any contact is evidence enough, including one a panel refuses: the
        // first thing a phone player touches is the start menu.
        pause.set_panel_up(true);
        pause.touch(at_pixels(FIRST, TouchPhase::Began, Vec2::new(400.0, 300.0)));
        pause.set_panel_up(false);
        pause.render(&mut dl, &atlas, false);
        assert!(!dl.is_empty(), "the button never appeared");

        dl.clear();
        pause.render(&mut dl, &atlas, true);
        assert!(dl.is_empty(), "the button stayed up behind a panel");
    }

    /// **The pointer the tap also raises is the button's, not the game's** — and
    /// only over the button, and only once the glass has been touched.
    ///
    /// The last two are what keep a mouse whole: the rectangle exists on every
    /// machine, and a desktop click in that corner must still reach the game.
    #[test]
    fn the_button_takes_the_pointer_that_presses_it_and_no_other() {
        let mut pause = laid_out();
        let centre = PauseControl::centre(EXTENT);
        assert!(
            !pause.takes_pointer(pointer_at(centre)),
            "a mouse on a machine nobody has touched lost a click to an \
             invisible button",
        );

        pause.touch(at_pixels(FIRST, TouchPhase::Began, centre));
        assert!(pause.takes_pointer(pointer_at(centre)));
        assert!(
            !pause.takes_pointer(pointer_at(Vec2::new(400.0, 300.0))),
            "the button swallowed a press in the middle of the field",
        );

        // **The position is remembered**: a second tap in the same place moves
        // the pointer nowhere, so the update carries no position at all.
        assert!(pause.takes_pointer(pointer_at(centre)));
        assert!(pause.takes_pointer(PointerUpdate {
            at: None,
            pressed: true,
            ..PointerUpdate::default()
        }));

        // A panel is the menu's screen, so the pointer over it is the menu's.
        pause.set_panel_up(true);
        assert!(!pause.takes_pointer(pointer_at(centre)));
    }

    /// The button stays inside the surface it is laid out on, at every extent
    /// the samples open at — including a short wide canvas, where a
    /// corner-relative layout goes wrong.
    #[test]
    fn the_button_sits_inside_the_corner_it_claims() {
        for extent in [(960, 720), (1920, 1080), (1440, 400), (600, 900)] {
            let (min, max) = button().rect(position(extent), &FontAtlas::built_in());
            assert!(min.x > 0.0 && min.y > 0.0, "off the top-left at {min}");
            assert!(
                (max.x - (extent.0 as f32 - MARGIN)).abs() < 1e-3,
                "not inset from the right edge of {extent:?}: {max}",
            );
            assert!(
                max.y < extent.1 as f32,
                "hanging off the bottom of {extent:?}",
            );
            let centre = PauseControl::centre(extent);
            assert!(
                centre.x > min.x && centre.x < max.x && centre.y > min.y && centre.y < max.y,
                "the centre a test aims at is not on the button: {centre} in \
                 {min}..{max}",
            );
        }
    }
}
