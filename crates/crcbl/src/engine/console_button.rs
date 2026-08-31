//! The on-screen button that opens the debug console, which is the loop's
//! rather than any game's.
//!
//! # Why it has to exist for the on-screen keyboard to mean anything
//!
//! [`CONSOLE_KEY`](super::CONSOLE_KEY) is the backtick and nothing else, so
//! before this a finger had **no route to the console at all** — a fact
//! `web/templates/demo-loop-keys.html` had already written down while
//! `docs/plan/52-debug-console.md` decision 6 recorded only the missing
//! keyboard. A keyboard drawn inside a panel nobody with a phone can open is a
//! feature that cannot be reached, so the two land together.
//!
//! # Why the loop draws it and [`PauseControl`](super::PauseControl) does not
//!
//! The pause button is wired by each sample, in four calls, because a game's
//! own on-screen controls and its own menus are around it and the loop cannot
//! see either. Nothing about the console is a game's: the panel is drawn by
//! [`Loop::frame`](super::Loop::frame) after everything else, so the button
//! that opens it is drawn there too and no `apps/*` crate gains a line. That is
//! the plan's exit criterion — "with no per-app code" — kept.
//!
//! # It reads contacts, not the pointer
//!
//! [`PauseControl`]'s argument, and it holds here for the same reason: a game
//! whose other control is held owns the primary contact, and every other finger
//! raises no pointer event at all. So the button is offered contacts, and the
//! loop asks [`takes_pointer`](ConsoleButton::takes_pointer) before it hands
//! the same finger's pointer press to the game.
//!
//! [`PauseControl`]: super::PauseControl

use super::{PointerUpdate, TouchUpdate, pause, surface_pixels};
use crcbl_ui::draw_list::DrawList;
use crcbl_ui::text::FontAtlas;
use crcbl_ui::touch::{CONTROL_STYLE, TouchButton};
use crcbl_ui::{Button, ButtonState};
use glam::Vec2;

/// The label, which is also what a test reads out of the frame to find it.
pub const LABEL: &str = "CONSOLE";

/// The button's size in pixels.
///
/// The pause button's height, so the two sit level in the same strip, and wider
/// because the label is longer.
const SIZE: Vec2 = Vec2::new(148.0, pause::SIZE.y);

/// The on-screen console toggle: where it is, whether it is there at all, and
/// whether it has been tapped.
///
/// **Immediately left of [`PauseControl`](super::PauseControl)**, in the corner
/// strip that control's docs already establish as the one no sample's HUD is
/// using — and beside it because both are the engine's own controls rather than
/// the game's.
///
/// **On screen only once a contact has arrived.** A machine with a keyboard has
/// the backtick, and a button drawn for it would be a button in every demo's
/// corner that nobody there needs. No golden frame moves, because a headless
/// golden never touches glass.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct ConsoleButton {
    button: TouchButton,
    /// Whether any contact has ever reached this run — the gate on the button
    /// being drawn at all.
    touched: bool,
    /// The surface, in pixels, as of the last frame laid out.
    extent: (u32, u32),
    /// Where the button was drawn last frame, and so where a contact this frame
    /// is hit-tested against.
    rect: (Vec2, Vec2),
    /// Where the pointer was last seen, in framebuffer pixels.
    ///
    /// Remembered rather than read per frame for [`PauseControl`]'s reason:
    /// [`PointerUpdate::at`] is `None` on a frame the pointer did not move, and
    /// two taps in the same place is exactly that.
    ///
    /// [`PauseControl`]: super::PauseControl
    pointer_at: Option<Vec2>,
}

impl ConsoleButton {
    /// A button nobody has touched, on a surface no frame has sized yet.
    #[must_use]
    pub const fn new() -> Self {
        Self {
            button: TouchButton::new(),
            touched: false,
            extent: (0, 0),
            rect: (Vec2::ZERO, Vec2::ZERO),
            pointer_at: None,
        }
    }

    /// Lays the button out for a surface of this size.
    ///
    /// The rectangle it computes is what the *next* frame's contacts are
    /// hit-tested against, which is why the art and the hit test come from one
    /// [`Button`] rather than two constants that could disagree.
    pub fn layout(&mut self, extent: (u32, u32), atlas: &FontAtlas) {
        self.extent = extent;
        self.rect = button().rect(position(extent), atlas);
    }

    /// Offer one contact to the button, and say whether it took it.
    ///
    /// Every contact sets the "a finger has arrived" latch, including the ones
    /// that land nowhere near it: the first thing anyone touches is the game,
    /// and a button that waited for a contact made *on itself* could never
    /// appear.
    pub fn touch(&mut self, touch: TouchUpdate) -> bool {
        self.touched = true;
        let at = touch.pixels(self.extent);
        self.button.offer(self.rect, touch.contact, touch.phase, at)
    }

    /// Whether this pointer update is the button's rather than the game's.
    ///
    /// Answers `false` for every update on a machine whose glass nobody has
    /// touched, so a mouse never loses a click to an invisible rectangle — and
    /// so a desktop's click on the corner still reaches the game.
    pub fn takes_pointer(&mut self, pointer: PointerUpdate) -> bool {
        if let Some(at) = pointer.at {
            self.pointer_at = Some(surface_pixels(at, self.extent));
        }
        self.touched
            && self.pointer_at.is_some_and(|at| {
                at.x >= self.rect.0.x
                    && at.x <= self.rect.1.x
                    && at.y >= self.rect.0.y
                    && at.y <= self.rect.1.y
            })
    }

    /// Whether the button was tapped since the loop last asked.
    pub const fn take_fired(&mut self) -> bool {
        self.button.take_fired()
    }

    /// Draws the button, which is **nothing at all** until a finger has arrived.
    pub fn render(&self, dl: &mut DrawList, atlas: &FontAtlas) {
        if !self.touched {
            return;
        }
        let state = if self.button.is_held() {
            ButtonState::Pressed
        } else {
            ButtonState::Idle
        };
        button().render(dl, position(self.extent), atlas, &CONTROL_STYLE, state);
    }

    /// The middle of the button on a surface of this size — where a finger has
    /// to land to press it.
    ///
    /// A pure function of the extent, so a test and the browser gate can aim at
    /// it without reading a laid-out control.
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

/// Its top-left corner: the slot immediately left of the pause button.
///
/// Measured off [`pause`]'s own constants rather than off a second copy of
/// them, so moving that corner moves both controls and cannot leave one of them
/// overlapping the other.
fn position(extent: (u32, u32)) -> Vec2 {
    let pause_left = extent.0 as f32 - pause::MARGIN - pause::SIZE.x;
    Vec2::new(pause_left - pause::MARGIN - SIZE.x, pause::MARGIN)
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use crate::engine::PauseControl;
    use crcbl_core::input::{ContactId, TouchPhase};

    const EXTENT: (u32, u32) = (960, 720);
    const FIRST: ContactId = ContactId(1);

    fn atlas() -> FontAtlas {
        FontAtlas::built_in()
    }

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

    fn laid_out() -> ConsoleButton {
        let mut control = ConsoleButton::new();
        control.layout(EXTENT, &atlas());
        control
    }

    /// A tap on it fires it, which is the whole of what the loop reads.
    #[test]
    fn a_tap_opens_the_console() {
        let mut control = laid_out();
        let centre = ConsoleButton::centre(EXTENT);
        assert!(control.touch(at_pixels(FIRST, TouchPhase::Began, centre)));
        assert!(control.touch(at_pixels(FIRST, TouchPhase::Ended, centre)));
        assert!(control.take_fired(), "a tap on the button fired nothing");
        assert!(!control.take_fired(), "one tap fired twice");
    }

    /// A tap that lands elsewhere is not the button's, and it still counts as a
    /// finger having arrived.
    #[test]
    fn a_tap_elsewhere_is_not_the_buttons_but_still_brings_it_on_screen() {
        let mut control = laid_out();
        let elsewhere = Vec2::new(20.0, 600.0);
        assert!(!control.touch(at_pixels(FIRST, TouchPhase::Began, elsewhere)));
        assert!(!control.take_fired());

        let mut dl = DrawList::new();
        control.render(&mut dl, &atlas());
        assert!(
            !dl.commands().is_empty(),
            "a contact anywhere did not put the button on screen",
        );
    }

    /// **Nothing is drawn, and no click is taken, until a finger has arrived.**
    ///
    /// This is what keeps the button out of every desktop demo and out of every
    /// golden frame: a run nobody has touched draws exactly what it drew before
    /// this control existed.
    #[test]
    fn an_untouched_run_draws_nothing_and_keeps_its_clicks() {
        let mut control = laid_out();
        let mut dl = DrawList::new();
        control.render(&mut dl, &atlas());
        assert!(
            dl.commands().is_empty(),
            "an untouched run drew {} commands",
            dl.commands().len(),
        );

        let centre = ConsoleButton::centre(EXTENT);
        let at = at_pixels(FIRST, TouchPhase::Moved, centre).at;
        assert!(
            !control.takes_pointer(PointerUpdate {
                at: Some(at),
                pressed: true,
                ..PointerUpdate::default()
            }),
            "a mouse click lost itself to a button that is not on screen",
        );
    }

    /// The finger that presses the button is also the emulated pointer, so the
    /// loop has to be able to keep that press away from the game.
    #[test]
    fn the_button_claims_the_pointer_the_same_finger_raises() {
        let mut control = laid_out();
        let centre = ConsoleButton::centre(EXTENT);
        control.touch(at_pixels(FIRST, TouchPhase::Began, centre));
        let at = at_pixels(FIRST, TouchPhase::Moved, centre).at;
        assert!(control.takes_pointer(PointerUpdate {
            at: Some(at),
            pressed: true,
            ..PointerUpdate::default()
        }));

        // And not over the rest of the frame, or the game would never see a tap
        // again on a device that has been touched once.
        let elsewhere = at_pixels(FIRST, TouchPhase::Moved, Vec2::new(20.0, 600.0)).at;
        assert!(!control.takes_pointer(PointerUpdate {
            at: Some(elsewhere),
            pressed: true,
            ..PointerUpdate::default()
        }));
    }

    /// **The two engine controls do not overlap**, which is the one thing about
    /// putting a second button in that corner that could go wrong silently: a
    /// console button drawn under the pause button would swallow every pause.
    #[test]
    fn it_does_not_overlap_the_pause_button() {
        for extent in [(960, 720), (800, 600), (1920, 1080), (640, 480)] {
            let mut console = ConsoleButton::new();
            console.layout(extent, &atlas());
            let mut pause = PauseControl::new();
            pause.layout(extent, &atlas());
            assert!(
                console.rect.1.x <= pause.rect().0.x,
                "on {extent:?} the console button reaches {} and the pause \
                 button starts at {}",
                console.rect.1.x,
                pause.rect().0.x,
            );
        }
    }
}
