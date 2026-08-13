//! On-screen controls: the widgets a finger drives.
//!
//! A phone has no keyboard, so a game whose movement is a stick has nothing to
//! bind until something on the glass *is* a stick. These are those things: a
//! [`TouchStick`] and a [`TouchButton`], each of which takes raw contacts, holds
//! the one that grabbed it, and reports a value a game feeds straight into
//! `crcbl_input`'s `ActionMap::virtual_stick` / `ActionMap::virtual_button`.
//! From the action map's side they are a **device**, indistinguishable from a
//! key — see `crcbl_input::Binding::Virtual`. Named rather than linked because
//! this crate does not depend on that one, and must not: a widget that knew
//! about action maps would be a widget only one input layer could use.
//!
//! # Why a widget and not a binding on a contact
//!
//! A contact reports a *place on the glass*. What that place means is a question
//! only the thing drawn under it can answer, and the only code that knows what
//! is drawn where is the code that drew it. So the split is: the shell delivers
//! every contact, the game offers each contact to its controls, and the controls
//! report actions. An action map that hit-tested widgets would need to be told
//! the whole layout every frame, and would still not know which of two
//! overlapping controls the player meant.
//!
//! # Which contact owns which control
//!
//! **First come, first served, and a control keeps its contact until that
//! contact ends.** A second finger landing on a held control is not offered to
//! it — [`TouchStick::offer`] and [`TouchButton::offer`] return `false` and the
//! caller passes it on to the next control — because the alternative is a stick
//! that teleports under the second thumb of a two-handed grip. The caller's
//! offer order is its priority order, which is the only thing that decides an
//! overlap.
//!
//! **A control drops its contact the moment that contact ends**, which is what
//! [`ContactId`]'s "unique among the contacts that are down together, and reused
//! afterwards" contract requires: state keyed on an id that outlives the finger
//! is state the next finger inherits.
//!
//! # Screen pixels in, action units out
//!
//! Every rectangle in this crate is Y-**down** with the origin at the
//! framebuffer's top-left, and contacts are offered in that space. A stick's
//! *value* is the other convention — −1…1 with +X right and **+Y up**, the units
//! `crcbl_input` resolves an `Axis2` in — so the flip happens once, here, rather
//! than in each game's unexplained sign.

use crate::draw_list::DrawList;
use crate::widget::Style;
use crcbl_core::input::{ContactId, TouchPhase};
use glam::Vec2;

/// How much of the pad's radius the knob is drawn as.
///
/// The knob is what the player's thumb is under, so it is drawn large enough to
/// be seen around a thumb and small enough that the pad is still visible with
/// the stick pushed all the way over.
const KNOB_FRACTION: f32 = 0.42;

/// The pad's outline thickness, in pixels.
const PAD_OUTLINE: f32 = 2.0;

/// A **floating** on-screen stick: it appears where the finger lands.
///
/// # Floating, not fixed, and why
///
/// A fixed stick is a target the player has to find without looking, on a
/// surface with no edges to feel for. Held two-handed, a phone puts the thumb
/// wherever the hands happen to grip, and every fixed position is wrong for some
/// grip — the player either looks down mid-run or slides off the control they
/// cannot feel. A floating stick has no wrong position: the centre is wherever
/// the thumb landed, so the first frame of a gesture always reads zero and the
/// throw is always in reach.
///
/// What it costs is discoverability — nothing is on screen until a finger is
/// down — which is why the page's control list still names it, and why the pad
/// is drawn from the moment the contact lands rather than only once it moves.
///
/// # What it reports
///
/// The offset from that landing point, divided by [`radius`](Self::radius) and
/// **clamped to the unit disc**: full deflection in every direction is the same
/// speed, and pushing past the pad is not a faster run. There is no dead zone
/// and none is needed — the origin is where the finger already is, so a stick
/// nobody has moved reports exactly `(0.0, 0.0)`.
///
/// # A cancelled contact and a lifted one both centre it
///
/// [`TouchPhase::Cancelled`] means "undo rather than commit", and for a stick
/// the two are the same thing: a stick has nothing latched to commit. Its value
/// *is* the command, continuously, and it is zero from the moment the contact
/// goes away — so the player never lunges in the direction the finger happened
/// to be pointing when the system took the gesture. The distinction is real for
/// [`TouchButton`], which does have something to commit, and that is where the
/// two phases are told apart.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct TouchStick {
    radius: f32,
    owner: Option<ContactId>,
    origin: Vec2,
    at: Vec2,
}

impl TouchStick {
    /// A stick nobody is touching, with the given throw in screen pixels.
    #[must_use]
    pub const fn new(radius: f32) -> Self {
        Self {
            radius,
            owner: None,
            origin: Vec2::ZERO,
            at: Vec2::ZERO,
        }
    }

    /// The throw, in screen pixels: how far the finger travels for full
    /// deflection.
    #[must_use]
    pub const fn radius(&self) -> f32 {
        self.radius
    }

    /// Resize the throw — a surface that changed size wants a stick a thumb can
    /// still cover.
    ///
    /// Takes effect immediately, including under a finger that is already
    /// holding it: the alternative is a stick whose throw disagrees with the pad
    /// it is drawn as until the player lets go.
    pub const fn set_radius(&mut self, radius: f32) {
        self.radius = radius;
    }

    /// Which contact is holding it, if any.
    #[must_use]
    pub const fn owner(&self) -> Option<ContactId> {
        self.owner
    }

    /// Whether a finger is on it.
    #[must_use]
    pub const fn is_held(&self) -> bool {
        self.owner.is_some()
    }

    /// Where the finger landed, in screen pixels — the pad's centre — or `None`
    /// when nothing is holding it.
    #[must_use]
    pub const fn origin(&self) -> Option<Vec2> {
        match self.owner {
            Some(_) => Some(self.origin),
            None => None,
        }
    }

    /// The deflection, −1…1 per axis with +X right and **+Y up**, never longer
    /// than one.
    ///
    /// `(0.0, 0.0)` when nothing is holding it, which is the same value a stick
    /// held at its centre reports — a released stick and a centred one ask for
    /// the same thing.
    #[must_use]
    pub fn value(&self) -> Vec2 {
        if self.owner.is_none() || self.radius <= 0.0 {
            return Vec2::ZERO;
        }
        let offset = self.at - self.origin;
        // The Y flip: screen pixels count down, an axis counts up.
        let value = Vec2::new(offset.x, -offset.y) / self.radius;
        let length = value.length();
        if length > 1.0 { value / length } else { value }
    }

    /// Offer one contact to this stick, and say whether it took it.
    ///
    /// `at` is in screen pixels. A [`Began`](TouchPhase::Began) is taken only
    /// when the stick is free; every later phase is taken only from the contact
    /// that is holding it. `false` means "not mine" — pass it to the next
    /// control.
    ///
    /// A floating stick claims **any** contact offered to it, wherever it
    /// landed: the caller decides what counts as offering one, and a stick that
    /// hit-tested a rectangle it had not drawn yet would be a fixed stick with
    /// extra steps.
    pub fn offer(&mut self, contact: ContactId, phase: TouchPhase, at: Vec2) -> bool {
        match phase {
            TouchPhase::Began => {
                if self.owner.is_some() {
                    return false;
                }
                self.owner = Some(contact);
                self.origin = at;
                self.at = at;
                true
            }
            TouchPhase::Moved => {
                if self.owner != Some(contact) {
                    return false;
                }
                self.at = at;
                true
            }
            // Ended and Cancelled alike: see the type's docs for why a stick
            // cannot tell them apart.
            TouchPhase::Ended | TouchPhase::Cancelled => {
                if self.owner != Some(contact) {
                    return false;
                }
                self.release();
                true
            }
        }
    }

    /// Let go of whatever contact is holding it, centring the stick.
    ///
    /// For the caller that has to take a control away without a contact event
    /// to hang it on — a panel opening over the field, a game state that stops
    /// accepting movement. It is the [`Cancelled`](TouchPhase::Cancelled) path:
    /// the player did not let go, so nothing is committed and the stick centres.
    pub const fn release(&mut self) {
        self.owner = None;
        self.origin = Vec2::ZERO;
        self.at = Vec2::ZERO;
    }

    /// Draw the pad and its knob, or **nothing at all** when no finger is on it.
    ///
    /// A control that is not being touched is not on screen: that is what makes
    /// a floating stick invisible to a player who has never touched the glass,
    /// and it is why a desktop that happens to have a touchscreen grows no
    /// widget it did not ask for.
    ///
    /// The pad is drawn as an axis-aligned square because an axis-aligned quad
    /// is the only primitive [`DrawList`] has, while the *throw* is round — see
    /// [`value`](Self::value). The knob reaches the pad's edge at the four
    /// cardinals and stops short of its corners.
    pub fn render(&self, dl: &mut DrawList, style: &Style) {
        if self.owner.is_none() {
            return;
        }
        let pad = Vec2::splat(self.radius);
        dl.rect(self.origin - pad, self.origin + pad, style.bg);
        dl.rect_outline(
            self.origin - pad,
            self.origin + pad,
            PAD_OUTLINE,
            style.border,
        );

        let value = self.value();
        // Back into screen space for the drawing, which is the same flip
        // `value` applied, undone.
        let knob_at = self.origin + Vec2::new(value.x, -value.y) * self.radius;
        let knob = Vec2::splat(self.radius * KNOB_FRACTION);
        dl.rect(knob_at - knob, knob_at + knob, style.bg_active);
    }
}

/// A **fixed** on-screen button, hit-tested against a rectangle the caller
/// hands it.
///
/// Fixed rather than floating for the reason a stick is the other way round: a
/// button means one specific thing, so it has to be somewhere the player can
/// aim at deliberately, and a button that appeared under a stray thumb would
/// fire on every stray thumb.
///
/// The rectangle is passed per call rather than stored, the way
/// [`Button::interact`](crate::widget::Button::interact) takes its position: the
/// place a control is drawn depends on the surface's size, and a widget holding
/// a rectangle from before the last resize is a widget being tapped where it no
/// longer is.
///
/// # Ended fires it; Cancelled does not
///
/// This is where [`TouchPhase::Cancelled`] earns its separate variant. A lift
/// **on** the button commits — that is a tap. A lift somewhere else does not:
/// the player slid off, which is how anyone cancels a press they regret. And a
/// contact the *system* took away — an edge swipe, a palm rejection, the window
/// losing focus — does not commit either, wherever it was: the player never
/// finished the press, and firing on it is firing an action nobody asked for.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct TouchButton {
    owner: Option<ContactId>,
    fired: bool,
}

impl TouchButton {
    /// A button nobody is touching.
    #[must_use]
    pub const fn new() -> Self {
        Self {
            owner: None,
            fired: false,
        }
    }

    /// Which contact is holding it, if any.
    #[must_use]
    pub const fn owner(&self) -> Option<ContactId> {
        self.owner
    }

    /// Whether a finger is on it — what a caller draws its pressed art from.
    #[must_use]
    pub const fn is_held(&self) -> bool {
        self.owner.is_some()
    }

    /// Offer one contact to this button, and say whether it took it.
    ///
    /// `rect` is `(min, max)` in screen pixels and `at` is the contact's
    /// position in the same space. A [`Began`](TouchPhase::Began) inside the
    /// rectangle is taken when the button is free; every later phase is taken
    /// only from the contact that is holding it, wherever it has since moved to
    /// — a finger that slid off the button still owns it, which is what lets it
    /// slide back on.
    pub fn offer(
        &mut self,
        rect: (Vec2, Vec2),
        contact: ContactId,
        phase: TouchPhase,
        at: Vec2,
    ) -> bool {
        let inside = |point: Vec2| {
            point.x >= rect.0.x && point.x <= rect.1.x && point.y >= rect.0.y && point.y <= rect.1.y
        };
        match phase {
            TouchPhase::Began => {
                if self.owner.is_some() || !inside(at) {
                    return false;
                }
                self.owner = Some(contact);
                true
            }
            TouchPhase::Moved => self.owner == Some(contact),
            TouchPhase::Ended => {
                if self.owner != Some(contact) {
                    return false;
                }
                self.owner = None;
                // Only a lift that is still on the button commits.
                self.fired |= inside(at);
                true
            }
            TouchPhase::Cancelled => {
                if self.owner != Some(contact) {
                    return false;
                }
                self.owner = None;
                true
            }
        }
    }

    /// Whether the button was tapped since this was last called, clearing the
    /// flag.
    ///
    /// Taken rather than read so a press cannot be acted on twice, and so a
    /// frame that runs late still sees a tap that landed while it was busy — the
    /// same reason a queued key event survives a frame that runs no ticks.
    pub const fn take_fired(&mut self) -> bool {
        let fired = self.fired;
        self.fired = false;
        fired
    }

    /// Let go of whatever contact is holding it **without** firing.
    ///
    /// The [`Cancelled`](TouchPhase::Cancelled) path, for a caller taking the
    /// control away with no contact event to hang it on. A pending tap that has
    /// already been committed is not withdrawn: it happened.
    pub const fn release(&mut self) {
        self.owner = None;
    }
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    const RADIUS: f32 = 100.0;
    const CENTRE: Vec2 = Vec2::new(300.0, 400.0);
    const FIRST: ContactId = ContactId(1);
    const SECOND: ContactId = ContactId(2);

    /// A stick at rest, then held, then pushed a known distance: the value is
    /// the offset over the radius, with **+Y up**.
    ///
    /// Every number here is exact rather than "non-zero": a stick whose sign was
    /// inverted, whose axes were swapped, or which reported the raw pixel offset
    /// would all pass a non-zero assertion and fail this one.
    #[test]
    fn a_contact_at_a_known_offset_is_a_known_deflection() {
        let mut stick = TouchStick::new(RADIUS);
        assert_eq!(stick.value(), Vec2::ZERO, "nothing is touching it");
        assert_eq!(stick.origin(), None);

        assert!(stick.offer(FIRST, TouchPhase::Began, CENTRE));
        assert_eq!(
            stick.value(),
            Vec2::ZERO,
            "the frame the finger lands is the centre, which is what a \
             floating stick buys",
        );
        assert_eq!(stick.origin(), Some(CENTRE));

        // Half the radius to the right and a quarter of it *up* the screen —
        // which is up on the axis too, hence the sign flip.
        stick.offer(FIRST, TouchPhase::Moved, CENTRE + Vec2::new(50.0, -25.0));
        assert_eq!(stick.value(), Vec2::new(0.5, 0.25));

        // …and down the screen is −Y on the axis.
        stick.offer(FIRST, TouchPhase::Moved, CENTRE + Vec2::new(0.0, 75.0));
        assert_eq!(stick.value(), Vec2::new(0.0, -0.75));
    }

    /// Past the radius the value stays on the unit disc, **keeping its
    /// direction**.
    #[test]
    fn a_contact_past_the_radius_clamps() {
        let mut stick = TouchStick::new(RADIUS);
        stick.offer(FIRST, TouchPhase::Began, CENTRE);

        // 3-4-5 again: five radii out, so the clamp lands on exactly (0.6, 0.8)
        // rather than somewhere that merely looks capped.
        stick.offer(FIRST, TouchPhase::Moved, CENTRE + Vec2::new(300.0, -400.0));
        let value = stick.value();
        assert!(
            (value - Vec2::new(0.6, 0.8)).length() < 1e-5,
            "expected the direction kept at unit length, got {value}",
        );

        // Straight out along one axis, a long way: exactly the axis's end.
        stick.offer(FIRST, TouchPhase::Moved, CENTRE + Vec2::new(-9_000.0, 0.0));
        assert_eq!(stick.value(), Vec2::new(-1.0, 0.0));
    }

    /// A lift centres the stick and frees it for the next finger.
    #[test]
    fn a_released_stick_centres() {
        let mut stick = TouchStick::new(RADIUS);
        stick.offer(FIRST, TouchPhase::Began, CENTRE);
        stick.offer(FIRST, TouchPhase::Moved, CENTRE + Vec2::new(RADIUS, 0.0));
        assert_eq!(stick.value(), Vec2::new(1.0, 0.0));

        assert!(stick.offer(FIRST, TouchPhase::Ended, CENTRE + Vec2::new(RADIUS, 0.0)));
        assert_eq!(
            stick.value(),
            Vec2::ZERO,
            "a lifted stick that kept its deflection is a player still walking",
        );
        assert_eq!(stick.owner(), None);
        assert!(!stick.is_held());

        // And the next finger gets a stick centred where *it* landed, not where
        // the last one was.
        let elsewhere = CENTRE + Vec2::new(-200.0, 120.0);
        assert!(stick.offer(SECOND, TouchPhase::Began, elsewhere));
        assert_eq!(stick.origin(), Some(elsewhere));
        assert_eq!(stick.value(), Vec2::ZERO);
    }

    /// A cancelled contact centres it too, and nothing about the last
    /// deflection survives.
    #[test]
    fn a_cancelled_contact_centres_the_stick_rather_than_committing_it() {
        let mut stick = TouchStick::new(RADIUS);
        stick.offer(FIRST, TouchPhase::Began, CENTRE);
        stick.offer(FIRST, TouchPhase::Moved, CENTRE + Vec2::new(0.0, -RADIUS));
        assert_eq!(stick.value(), Vec2::new(0.0, 1.0));

        assert!(stick.offer(
            FIRST,
            TouchPhase::Cancelled,
            CENTRE + Vec2::new(0.0, -RADIUS)
        ));
        assert_eq!(stick.value(), Vec2::ZERO);
        assert_eq!(stick.owner(), None);
    }

    /// `release` is the same undo with no contact to hang it on.
    #[test]
    fn releasing_a_stick_by_hand_centres_it() {
        let mut stick = TouchStick::new(RADIUS);
        stick.offer(FIRST, TouchPhase::Began, CENTRE);
        stick.offer(FIRST, TouchPhase::Moved, CENTRE + Vec2::new(RADIUS, 0.0));
        stick.release();
        assert_eq!(stick.value(), Vec2::ZERO);
        assert!(!stick.is_held());
        // The contact that was holding it is nobody's now: its later events are
        // not this stick's.
        assert!(!stick.offer(FIRST, TouchPhase::Moved, CENTRE));
        assert_eq!(stick.value(), Vec2::ZERO);
    }

    /// **The second finger does not steal the stick**, and does not move it.
    ///
    /// The case the whole multi-touch seam exists for: two thumbs on the glass,
    /// and the one that grabbed the control keeps it.
    #[test]
    fn a_second_contact_does_not_take_a_held_stick() {
        let mut stick = TouchStick::new(RADIUS);
        stick.offer(FIRST, TouchPhase::Began, CENTRE);
        stick.offer(FIRST, TouchPhase::Moved, CENTRE + Vec2::new(RADIUS, 0.0));
        let held = stick.value();

        let far = CENTRE + Vec2::new(-400.0, -300.0);
        assert!(
            !stick.offer(SECOND, TouchPhase::Began, far),
            "a held stick must refuse a second contact, so the caller can \
             offer it to something else",
        );
        assert_eq!(stick.owner(), Some(FIRST));
        assert_eq!(stick.origin(), Some(CENTRE));
        assert_eq!(stick.value(), held, "the second finger moved the stick");

        assert!(!stick.offer(SECOND, TouchPhase::Moved, CENTRE));
        assert_eq!(stick.value(), held);

        // The second finger lifting is not the first one letting go.
        assert!(!stick.offer(SECOND, TouchPhase::Ended, CENTRE));
        assert_eq!(stick.owner(), Some(FIRST));
        assert_eq!(stick.value(), held);

        // And the first one still owns it.
        stick.offer(FIRST, TouchPhase::Moved, CENTRE);
        assert_eq!(stick.value(), Vec2::ZERO);
    }

    /// A resize changes the throw under the finger holding it.
    #[test]
    fn a_resized_stick_reports_the_new_throw() {
        let mut stick = TouchStick::new(RADIUS);
        stick.offer(FIRST, TouchPhase::Began, CENTRE);
        stick.offer(FIRST, TouchPhase::Moved, CENTRE + Vec2::new(50.0, 0.0));
        assert_eq!(stick.value(), Vec2::new(0.5, 0.0));

        stick.set_radius(RADIUS * 0.5);
        assert_eq!(stick.radius(), 50.0);
        assert_eq!(stick.value(), Vec2::new(1.0, 0.0));
    }

    /// Nothing is drawn until a finger is on it, and then both parts are.
    #[test]
    fn a_stick_nobody_is_touching_draws_nothing() {
        let mut stick = TouchStick::new(RADIUS);
        let style = Style::default();

        let mut dl = DrawList::new();
        stick.render(&mut dl, &style);
        assert!(
            dl.is_empty(),
            "an untouched stick drew {} command(s), so a desktop grows a \
             control nobody asked for",
            dl.len(),
        );

        stick.offer(FIRST, TouchPhase::Began, CENTRE);
        stick.render(&mut dl, &style);
        assert!(
            dl.len() > 1,
            "a held stick drew {} command(s): the pad and the knob are two",
            dl.len(),
        );

        // …and it goes away again when the finger does.
        let drawn = dl.len();
        stick.offer(FIRST, TouchPhase::Ended, CENTRE);
        stick.render(&mut dl, &style);
        assert_eq!(dl.len(), drawn, "a released stick is still on screen");
    }

    // -- the button ----------------------------------------------------------

    const RECT: (Vec2, Vec2) = (Vec2::new(800.0, 20.0), Vec2::new(900.0, 70.0));
    const ON: Vec2 = Vec2::new(850.0, 45.0);
    const OFF: Vec2 = Vec2::new(100.0, 600.0);

    /// A tap on the button fires it — once.
    #[test]
    fn a_tap_fires_the_button_once() {
        let mut button = TouchButton::new();
        assert!(!button.take_fired(), "nothing has been tapped");

        assert!(button.offer(RECT, FIRST, TouchPhase::Began, ON));
        assert!(button.is_held());
        assert!(!button.take_fired(), "a press is not a tap");

        assert!(button.offer(RECT, FIRST, TouchPhase::Ended, ON));
        assert!(!button.is_held());
        assert!(button.take_fired());
        assert!(!button.take_fired(), "taken means taken");
    }

    /// A press that started somewhere else is not this button's, and a press
    /// that slid off does not fire.
    #[test]
    fn a_press_outside_it_and_a_slide_off_do_not_fire() {
        let mut button = TouchButton::new();
        assert!(!button.offer(RECT, FIRST, TouchPhase::Began, OFF));
        assert!(!button.is_held());
        assert!(!button.offer(RECT, FIRST, TouchPhase::Ended, ON));
        assert!(
            !button.take_fired(),
            "a lift over a button nobody pressed fired it",
        );

        // Pressed, slid off, lifted: the ordinary way to change your mind.
        button.offer(RECT, FIRST, TouchPhase::Began, ON);
        assert!(button.offer(RECT, FIRST, TouchPhase::Moved, OFF));
        assert!(button.is_held(), "sliding off does not let go of the press");
        button.offer(RECT, FIRST, TouchPhase::Ended, OFF);
        assert!(!button.take_fired());

        // Slid off and back on again: that one fires.
        button.offer(RECT, FIRST, TouchPhase::Began, ON);
        button.offer(RECT, FIRST, TouchPhase::Moved, OFF);
        button.offer(RECT, FIRST, TouchPhase::Moved, ON);
        button.offer(RECT, FIRST, TouchPhase::Ended, ON);
        assert!(button.take_fired());
    }

    /// **A cancelled press does not fire**, which is the whole reason
    /// [`TouchPhase::Cancelled`] is not [`TouchPhase::Ended`].
    #[test]
    fn a_cancelled_press_does_not_fire_the_button() {
        let mut button = TouchButton::new();
        button.offer(RECT, FIRST, TouchPhase::Began, ON);
        assert!(button.offer(RECT, FIRST, TouchPhase::Cancelled, ON));
        assert!(!button.is_held());
        assert!(
            !button.take_fired(),
            "the system took the gesture and the button fired anyway",
        );

        // And by hand, for a caller taking the control away.
        button.offer(RECT, FIRST, TouchPhase::Began, ON);
        button.release();
        assert!(!button.is_held());
        assert!(!button.take_fired());
    }

    /// A second finger does not take a held button, and cannot fire it.
    #[test]
    fn a_second_contact_does_not_take_a_held_button() {
        let mut button = TouchButton::new();
        button.offer(RECT, FIRST, TouchPhase::Began, ON);

        assert!(!button.offer(RECT, SECOND, TouchPhase::Began, ON));
        assert_eq!(button.owner(), Some(FIRST));
        assert!(!button.offer(RECT, SECOND, TouchPhase::Ended, ON));
        assert!(
            !button.take_fired(),
            "a second finger lifted off and fired the first one's button",
        );
        assert!(button.is_held(), "and it let go of the first one's press");

        button.offer(RECT, FIRST, TouchPhase::Ended, ON);
        assert!(button.take_fired());
    }
}
