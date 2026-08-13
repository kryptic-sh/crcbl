//! Horde's on-screen controls: what a phone plays it with.
//!
//! # What this game needs on the glass, and what it does not
//!
//! Horde binds six things: **movement**, the start/restart edge, and three
//! level-up choices. Five of the six already survive a screen without a widget —
//! `PLAY`, `TRY AGAIN` and the three upgrades are *menu buttons*, and a menu is
//! hit-tested against the emulated pointer that every platform builds from the
//! primary contact, so a finger has always been able to press them. Movement is
//! the one that does not: it is a direction held continuously while the game is
//! being played, there is no menu up, and there is nothing to tap.
//!
//! So the movement stick is the control this game was missing, and it is the one
//! this module exists for. The other control here is the **pause button**, and
//! it is not a game action at all: [`crcbl::engine::PAUSE_KEY`] is the loop's
//! and never reaches a game, so a phone could start a run and then never stop
//! it, and the pause menu — the only place fullscreen and the debug panel are
//! tappable — could not be opened. One button reaches all three.
//!
//! **Deliberately not here:** a fire button (the gun aims and fires itself —
//! that is the genre), an on-screen restart (the two menus that offer one are
//! already tappable, and a restart button live during play is a run lost to a
//! stray thumb), and on-screen level-up choices (the panel's own three buttons
//! are it).
//!
//! # A held control takes the pointer away, which is why the button is a contact
//!
//! Only the **primary** contact is reported as an emulated pointer. A thumb
//! resting on the stick is that primary contact for as long as it is down, so
//! every other finger raises no pointer event at all: while the stick is held, a
//! menu button cannot be pressed by anyone. A second control that went through
//! the pointer would therefore be unusable at exactly the moment it is wanted —
//! which is why both controls here read **contacts**, and why this is the first
//! sample where two fingers can do two things at once.
//!
//! # Where each control lives
//!
//! The stick **floats**: it appears where the thumb lands. Held two-handed, a
//! phone puts the thumb wherever the grip does, and every fixed position is
//! wrong for some grip — see [`crcbl::ui::TouchStick`], which carries the whole
//! argument. The pause button is **fixed**, in the top-right corner: it means
//! one specific thing, so it has to be somewhere a player can aim at
//! deliberately, and the top-right is the one corner neither the HUD nor a
//! two-handed grip is already using.
//!
//! # When they are on screen
//!
//! **Only once a contact has actually arrived**, and only while no panel is up.
//! Not on `ShellCaps::TOUCH`, which a desktop with a touchscreen also sets: a
//! player using a mouse on such a machine would grow a control they never asked
//! for, and the first finger to land is the only honest evidence that anyone
//! wants one. A desktop that is never touched sees no change at all — which is
//! also why no golden frame moves.
//!
//! A panel on screen takes the controls away for the frames it is up: the menu's
//! buttons are the whole interaction there, and a stick that stayed live behind
//! the level-up screen would be walking the wizard the moment the panel closed.

use crcbl::engine::TouchUpdate;
use crcbl::math::Vec2;
use crcbl::ui::draw_list::DrawList;
use crcbl::ui::text::FontAtlas;
use crcbl::ui::{Button, ButtonState, Style, TouchButton, TouchStick};

/// The stick's throw, as a fraction of the surface's shorter side.
///
/// A fraction rather than a pixel count because the surface is a phone's screen,
/// a browser canvas or a desktop window, and a throw that fits a thumb on one of
/// those is a throw the thumb cannot reach the edge of on another.
const STICK_RADIUS_FRACTION: f32 = 0.11;

/// …and the smallest throw it is ever given, in pixels, so a very small surface
/// still leaves room between the centre and the edge for the eight sectors
/// `game::eight_way` splits it into.
const STICK_RADIUS_MIN: f32 = 40.0;

/// The pause button's size in pixels: a deliberate target for a thumb, which is
/// wider than a label needs.
const PAUSE_SIZE: Vec2 = Vec2::new(112.0, 56.0);

/// How far the pause button sits from the surface's top-right corner.
const PAUSE_MARGIN: f32 = 12.0;

/// The palette both controls are drawn in.
///
/// Dark and translucent, because they are drawn **over the arena** and a control
/// a player cannot see past is a control hiding the thing that is about to kill
/// them.
const CONTROL_STYLE: Style = Style {
    text: [0.9, 0.9, 1.0, 0.9],
    bg: [0.08, 0.08, 0.12, 0.45],
    bg_hover: [0.15, 0.15, 0.22, 0.55],
    bg_active: [0.55, 0.55, 0.75, 0.7],
    border: [0.7, 0.7, 0.85, 0.55],
};

/// Horde's on-screen controls, and the bookkeeping that decides whether they are
/// there at all.
#[derive(Debug)]
pub struct Controls {
    stick: TouchStick,
    pause: TouchButton,
    /// Whether any contact has ever reached this run.
    ///
    /// The gate on the pause button being drawn — see the module header. The
    /// stick needs no such gate: a floating stick is drawn only while a finger
    /// is on it, which is the same claim made structurally.
    touched: bool,
    /// Whether a menu was on screen the last time the loop asked which one to
    /// show.
    ///
    /// **Last frame's, deliberately**, and it decides what happens to a
    /// *contact*: exactly as the loop resolves its own pointer against last
    /// frame's menu, the panel the player put a thumb through is the one that
    /// was on screen when they did it. What is *drawn* asks a fresher question —
    /// see [`render`](Self::render).
    panel_up: bool,
    /// The surface, in pixels, as of the last frame drawn.
    extent: (u32, u32),
    /// Where the pause button was drawn last frame, and so where a contact this
    /// frame is hit-tested against.
    pause_rect: (Vec2, Vec2),
}

impl Controls {
    /// Controls nobody has touched, on a surface of the given size.
    #[must_use]
    pub fn new(extent: (u32, u32)) -> Self {
        Self {
            stick: TouchStick::new(stick_radius(extent)),
            pause: TouchButton::new(),
            touched: false,
            panel_up: false,
            extent,
            pause_rect: (Vec2::ZERO, Vec2::ZERO),
        }
    }

    /// The movement stick's deflection, +X right and +Y up — the value fed to
    /// `Game::stick_moved`, and `(0, 0)` whenever nothing is holding it.
    #[must_use]
    pub fn stick(&self) -> Vec2 {
        self.stick.value()
    }

    /// Offer one contact to the controls.
    ///
    /// **The button first, then the stick.** The button is the one with a
    /// rectangle, so it can say the contact is not its business; the stick has
    /// none and claims whatever is left, which is what makes the whole field a
    /// place to put a thumb. A contact one control has taken is never offered to
    /// the other — that is what the `bool` each `offer` returns is for.
    ///
    /// A contact that arrives while a panel is up reaches neither: the menu owns
    /// the screen, and its buttons are pressed through the emulated pointer the
    /// loop already routes.
    pub fn touch(&mut self, touch: TouchUpdate) {
        self.touched = true;
        if self.panel_up {
            return;
        }
        let at = touch.pixels(self.extent);
        if self
            .pause
            .offer(self.pause_rect, touch.contact, touch.phase, at)
        {
            return;
        }
        self.stick.offer(touch.contact, touch.phase, at);
    }

    /// Tells the controls which menu the frame that just drew is showing, and
    /// takes them away while one is.
    ///
    /// **A panel appearing cancels a held stick rather than ending it.** The
    /// player did not lift the thumb — a level-up froze the field under it — so
    /// the stick centres and the wizard stops, instead of walking off in
    /// whatever direction the thumb happened to be resting when the game took
    /// the control away. Same for a half-pressed pause button: it does not fire.
    pub const fn set_panel_up(&mut self, panel_up: bool) {
        self.panel_up = panel_up;
        if panel_up {
            self.stick.release();
            self.pause.release();
        }
    }

    /// Whether the pause button was tapped since the loop last asked.
    ///
    /// The loop asks once a frame whatever else happened, so a tap cannot be
    /// left waiting — and it asks *before* the draw, so a tap folded this frame
    /// pauses this frame rather than the next one.
    pub const fn take_pause(&mut self) -> bool {
        self.pause.take_fired()
    }

    /// Lays the controls out for a surface of this size.
    ///
    /// Called once a frame from the draw, which is the only place the extent is
    /// known.
    pub fn layout(&mut self, extent: (u32, u32), atlas: &FontAtlas) {
        self.extent = extent;
        self.stick.set_radius(stick_radius(extent));
        self.pause_rect = pause_button().rect(pause_pos(extent), atlas);
    }

    /// Draws whatever is on screen this frame — which is nothing at all until a
    /// finger has arrived, and nothing while a panel is up.
    ///
    /// `panel_up` is **this** frame's answer, not the one
    /// [`set_panel_up`](Self::set_panel_up) holds: the loop asks a game to draw
    /// before it asks which menu the frame shows, so a control drawn from the
    /// stored flag is a frame behind the panel it is hiding from — visibly, as a
    /// button that lingers for a frame over a panel that has just opened and
    /// goes missing for a frame after one closes. Where a *contact* is concerned
    /// the stale answer is the right one, and that is the field's job.
    pub fn render(&self, dl: &mut DrawList, atlas: &FontAtlas, panel_up: bool) {
        if !self.touched || panel_up {
            return;
        }
        self.stick.render(dl, &CONTROL_STYLE);
        let state = if self.pause.is_held() {
            ButtonState::Pressed
        } else {
            ButtonState::Idle
        };
        pause_button().render(dl, pause_pos(self.extent), atlas, &CONTROL_STYLE, state);
    }
}

/// The stick's throw for a surface of this size.
fn stick_radius(extent: (u32, u32)) -> f32 {
    let shorter = extent.0.min(extent.1) as f32;
    (shorter * STICK_RADIUS_FRACTION).max(STICK_RADIUS_MIN)
}

/// The pause button, as a widget: the art and the hit rectangle come from one
/// place, so a tap cannot land somewhere the label is not.
fn pause_button() -> Button {
    Button::new("PAUSE").with_fixed_size(PAUSE_SIZE)
}

/// The middle of the pause button, for the tests that have to put a finger on
/// it without knowing which corner it is laid out from.
#[cfg(test)]
pub fn pause_centre(extent: (u32, u32)) -> Vec2 {
    pause_pos(extent) + PAUSE_SIZE * 0.5
}

/// Its top-left corner, inset from the surface's top-right.
fn pause_pos(extent: (u32, u32)) -> Vec2 {
    Vec2::new(extent.0 as f32 - PAUSE_MARGIN - PAUSE_SIZE.x, PAUSE_MARGIN)
}

// ---- tests ------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use crcbl::core::input::{ContactId, TouchPhase};

    const EXTENT: (u32, u32) = (960, 720);
    const FIRST: ContactId = ContactId(1);
    const SECOND: ContactId = ContactId(2);

    /// A contact at a place on the surface, as the loop would deliver it —
    /// normalised, +Y up.
    fn at_pixels(contact: ContactId, phase: TouchPhase, x: f32, y: f32) -> TouchUpdate {
        TouchUpdate {
            contact,
            phase,
            at: Vec2::new(
                x / EXTENT.0 as f32 * 2.0 - 1.0,
                1.0 - y / EXTENT.1 as f32 * 2.0,
            ),
        }
    }

    /// Controls laid out and told the game is being played, which is the state
    /// every check below is about.
    fn playing() -> Controls {
        let mut controls = Controls::new(EXTENT);
        controls.layout(EXTENT, &FontAtlas::built_in());
        controls.set_panel_up(false);
        controls
    }

    /// A drag on the field is a deflection, and it is the one the geometry
    /// says: half a throw to the right is exactly `(0.5, 0.0)`.
    #[test]
    fn a_drag_on_the_field_deflects_the_stick() {
        let mut controls = playing();
        assert_eq!(controls.stick(), Vec2::ZERO);

        let radius = stick_radius(EXTENT);
        controls.touch(at_pixels(FIRST, TouchPhase::Began, 300.0, 500.0));
        assert_eq!(
            controls.stick(),
            Vec2::ZERO,
            "the stick centres itself where the thumb landed",
        );

        controls.touch(at_pixels(
            FIRST,
            TouchPhase::Moved,
            300.0 + radius * 0.5,
            500.0,
        ));
        let value = controls.stick();
        assert!(
            (value - Vec2::new(0.5, 0.0)).length() < 1e-3,
            "expected half a throw right, got {value}",
        );

        // Up the *screen* is +Y on the stick, which is the flip that would put
        // the wizard through the bottom of the arena if it were missing.
        controls.touch(at_pixels(FIRST, TouchPhase::Moved, 300.0, 500.0 - radius));
        let value = controls.stick();
        assert!(
            (value - Vec2::new(0.0, 1.0)).length() < 1e-3,
            "expected full throw up, got {value}",
        );

        controls.touch(at_pixels(FIRST, TouchPhase::Ended, 300.0, 500.0 - radius));
        assert_eq!(controls.stick(), Vec2::ZERO, "a lifted stick centres");
    }

    /// **The pause button and the stick are two controls, and two fingers work
    /// them at once.**
    ///
    /// The claim this whole slice exists to make: the thumb holding the stick is
    /// the primary contact, so the second finger has no pointer to press
    /// anything with, and it pauses the game anyway.
    #[test]
    fn a_second_finger_pauses_while_the_first_holds_the_stick() {
        let mut controls = playing();
        let radius = stick_radius(EXTENT);
        controls.touch(at_pixels(FIRST, TouchPhase::Began, 300.0, 500.0));
        controls.touch(at_pixels(FIRST, TouchPhase::Moved, 300.0 - radius, 500.0));
        let held = controls.stick();
        assert!(
            (held - Vec2::new(-1.0, 0.0)).length() < 1e-3,
            "the first thumb is walking left, not {held}",
        );

        // The second finger, on the button, while the first has not moved.
        let button = pause_pos(EXTENT) + PAUSE_SIZE * 0.5;
        controls.touch(at_pixels(SECOND, TouchPhase::Began, button.x, button.y));
        assert!(!controls.take_pause(), "a press is not a tap");
        assert_eq!(
            controls.stick(),
            held,
            "the second finger stole the first one's stick",
        );

        controls.touch(at_pixels(SECOND, TouchPhase::Ended, button.x, button.y));
        assert!(controls.take_pause(), "the second finger did not pause");
        assert!(!controls.take_pause(), "taken means taken");
        assert_eq!(
            controls.stick(),
            held,
            "pausing let go of the walk the other thumb is still holding",
        );

        // …and the first thumb still owns the stick afterwards.
        controls.touch(at_pixels(FIRST, TouchPhase::Moved, 300.0, 500.0));
        assert_eq!(controls.stick(), Vec2::ZERO);
    }

    /// A contact that lands on the button is the button's and never the
    /// stick's, so tapping pause does not also lurch the wizard.
    #[test]
    fn the_button_takes_the_contact_that_lands_on_it() {
        let mut controls = playing();
        let button = pause_pos(EXTENT) + PAUSE_SIZE * 0.5;
        controls.touch(at_pixels(FIRST, TouchPhase::Began, button.x, button.y));
        // A finger that slid a long way while pressing the button: the stick
        // would report a full deflection if it had taken this contact.
        controls.touch(at_pixels(FIRST, TouchPhase::Moved, 100.0, 700.0));
        assert_eq!(
            controls.stick(),
            Vec2::ZERO,
            "the stick took the button's contact",
        );
    }

    /// **A cancelled contact undoes rather than commits**: the stick centres and
    /// the button does not fire.
    #[test]
    fn a_cancelled_contact_undoes_both_controls() {
        let mut controls = playing();
        let radius = stick_radius(EXTENT);
        controls.touch(at_pixels(FIRST, TouchPhase::Began, 300.0, 500.0));
        controls.touch(at_pixels(FIRST, TouchPhase::Moved, 300.0, 500.0 - radius));
        assert_eq!(controls.stick(), Vec2::new(0.0, 1.0));
        controls.touch(at_pixels(
            FIRST,
            TouchPhase::Cancelled,
            300.0,
            500.0 - radius,
        ));
        assert_eq!(
            controls.stick(),
            Vec2::ZERO,
            "a cancelled gesture left the wizard walking north",
        );

        let button = pause_pos(EXTENT) + PAUSE_SIZE * 0.5;
        controls.touch(at_pixels(SECOND, TouchPhase::Began, button.x, button.y));
        controls.touch(at_pixels(SECOND, TouchPhase::Cancelled, button.x, button.y));
        assert!(
            !controls.take_pause(),
            "the system took the gesture away and the game paused anyway",
        );
    }

    /// A panel takes the controls away, and takes a held stick with it.
    #[test]
    fn a_panel_takes_the_controls_away() {
        let mut controls = playing();
        let radius = stick_radius(EXTENT);
        controls.touch(at_pixels(FIRST, TouchPhase::Began, 300.0, 500.0));
        controls.touch(at_pixels(FIRST, TouchPhase::Moved, 300.0 + radius, 500.0));
        assert_eq!(controls.stick(), Vec2::new(1.0, 0.0));

        controls.set_panel_up(true);
        assert_eq!(
            controls.stick(),
            Vec2::ZERO,
            "the wizard kept walking behind the level-up screen",
        );

        // And nothing lands while it is up — including the finger that is
        // already down.
        controls.touch(at_pixels(FIRST, TouchPhase::Moved, 300.0 + radius, 500.0));
        assert_eq!(controls.stick(), Vec2::ZERO);
        let button = pause_pos(EXTENT) + PAUSE_SIZE * 0.5;
        controls.touch(at_pixels(SECOND, TouchPhase::Began, button.x, button.y));
        controls.touch(at_pixels(SECOND, TouchPhase::Ended, button.x, button.y));
        assert!(!controls.take_pause(), "a panel's screen fired the button");
    }

    /// **Nothing is drawn until a finger arrives**, which is what keeps the
    /// desktop's picture exactly what it was.
    #[test]
    fn an_untouched_run_draws_no_controls() {
        let atlas = FontAtlas::built_in();
        let mut controls = playing();
        let mut dl = DrawList::new();
        controls.render(&mut dl, &atlas, false);
        assert!(
            dl.is_empty(),
            "a run nobody touched drew {} control command(s)",
            dl.len(),
        );

        // The first contact is what puts the button on screen — and the stick
        // is on screen only while it is held.
        let button = pause_pos(EXTENT) + PAUSE_SIZE * 0.5;
        controls.touch(at_pixels(FIRST, TouchPhase::Began, button.x, button.y));
        controls.render(&mut dl, &atlas, false);
        let with_button = dl.len();
        assert!(with_button > 0, "the button never appeared");

        dl.clear();
        controls.touch(at_pixels(FIRST, TouchPhase::Ended, button.x, button.y));
        controls.touch(at_pixels(SECOND, TouchPhase::Began, 300.0, 500.0));
        controls.render(&mut dl, &atlas, false);
        assert!(
            dl.len() > with_button,
            "a held stick drew nothing on top of the button",
        );

        // A panel puts them away again — on the frame it opens, which is why
        // `render` is told about it rather than reading the stored answer.
        dl.clear();
        controls.render(&mut dl, &atlas, true);
        assert!(dl.is_empty(), "the controls stayed up behind a panel");
    }

    /// The throw scales with the surface and never falls below the floor.
    #[test]
    fn the_stick_fits_the_surface_it_is_drawn_on() {
        assert!(
            (stick_radius(EXTENT) - 720.0 * STICK_RADIUS_FRACTION).abs() < 1e-3,
            "the throw is a fraction of the *shorter* side",
        );
        assert_eq!(
            stick_radius((3_000, 200)),
            STICK_RADIUS_MIN,
            "a short surface still gets a usable throw",
        );
    }

    /// The button stays inside the surface, and clear of the HUD panel.
    #[test]
    fn the_pause_button_sits_in_the_corner_it_claims() {
        let (min, max) = pause_button().rect(pause_pos(EXTENT), &FontAtlas::built_in());
        assert!(min.x > 0.0 && min.y > 0.0, "off the top-left at {min}");
        assert!(
            (max.x - (EXTENT.0 as f32 - PAUSE_MARGIN)).abs() < 1e-3,
            "not inset from the right edge: {max}",
        );
        assert!(
            max.y < EXTENT.1 as f32,
            "hanging off the bottom of the surface",
        );
        assert!(
            min.x >= crate::app::HUD_PANEL_RIGHT,
            "the button overlaps the HUD panel, which ends at {}",
            crate::app::HUD_PANEL_RIGHT,
        );
    }
}
