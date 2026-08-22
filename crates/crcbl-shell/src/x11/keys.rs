//! X11 input numbering → engine vocabulary, and the two encodings the window
//! manager reads.
//!
//! Everything in this module is a pure function over integers, which is the
//! point: it is the part of the backend that can be wrong in a way no X server
//! would ever complain about, so it is the part that is unit-tested without
//! one.
//!
//! # X11 keycodes are evdev plus eight
//!
//! And that is not a coincidence to be memorized — it is the *original*. X11
//! reserved keycodes 0–7 (0 means "no key" and the protocol requires
//! `min_keycode >= 8`), the XFree86 keymaps were numbered around that, and
//! Linux's evdev codes were later defined to line up with them minus the
//! offset. So the direction of the debt runs the other way from what the
//! Wayland backend's [`EVDEV_OFFSET`] comment
//! might suggest, and the shared [`keymap`] table is
//! reachable from here by subtracting eight —
//! [`scancode`] is that subtraction, in one place, named.
//!
//! # Buttons 4 to 7 are the wheel
//!
//! Core X11 has no scroll axis. A wheel notch is a **button press immediately
//! followed by a release** on button 4 (up), 5 (down), 6 (left) or 7 (right),
//! and horizontal scrolling was retrofitted the same way. A backend that
//! forwarded them as buttons would give a UI two extra mouse buttons that fire
//! in pairs and no scrolling at all, so [`button`] classifies rather than maps.
//!
//! This is also why the wheel is not smooth on core X11: a notch is one
//! discrete event with no magnitude, which is exactly
//! [`ScrollDelta::Lines`]. XInput 2 has
//! smooth scroll valuators; this slice uses XI2 only for raw motion, and says
//! so in [the backend docs](super).

use crcbl_core::KeyCode;
use crcbl_core::input::{PointerButton, ScrollDelta};

use crate::linux::keymap;
use crate::linux::xkb::EVDEV_OFFSET;

/// The evdev code an X11 keycode stands for.
///
/// `None` below the offset: keycodes 0–7 do not name a key, and `0` in
/// particular is the protocol's "no key". Subtracting blindly would turn them
/// into evdev codes near zero, which are real keys.
#[must_use]
pub const fn scancode(keycode: u8) -> Option<u32> {
    let keycode = keycode as u32;
    if keycode < EVDEV_OFFSET {
        return None;
    }
    Some(keycode - EVDEV_OFFSET)
}

/// The engine key an X11 keycode names, layout-independently.
///
/// Goes through the shared evdev table, so `KeyCode::KeyW` is the same physical
/// position on both Linux backends by construction rather than by two tables
/// agreeing.
#[must_use]
pub fn key_code(keycode: u8) -> Option<KeyCode> {
    keymap::key_code(scancode(keycode)?)
}

/// What an X11 button number means.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Button {
    /// A real button press or release.
    Press(PointerButton),
    /// A wheel notch. Only the *press* produces one; see [`button`].
    Scroll(ScrollDelta),
    /// The release half of a wheel notch, which is delivered and ignored.
    ScrollRelease,
}

/// One line of scroll per notch.
///
/// Core X11 gives no magnitude, so this is a convention rather than a
/// measurement — and it is the same convention every X11 toolkit uses, which is
/// what makes a crcbl window scroll at the same speed as the browser next to
/// it.
const LINES_PER_NOTCH: f32 = 1.0;

/// Classifies an X11 button number.
///
/// `pressed` distinguishes the two halves of a wheel notch: the press carries
/// the scroll, the release carries nothing, and forwarding both would double
/// every scroll event.
#[must_use]
pub const fn button(detail: u8, pressed: bool) -> Button {
    match detail {
        // 1/2/3 are left/middle/right — note that X11's middle is *2*, not 3,
        // which is the opposite of every other platform's numbering and the
        // single most common off-by-one in X11 input code.
        1 => Button::Press(PointerButton::Left),
        2 => Button::Press(PointerButton::Middle),
        3 => Button::Press(PointerButton::Right),
        4..=7 if !pressed => Button::ScrollRelease,
        4 => Button::Scroll(ScrollDelta::Lines {
            x: 0.0,
            y: LINES_PER_NOTCH,
        }),
        5 => Button::Scroll(ScrollDelta::Lines {
            x: 0.0,
            y: -LINES_PER_NOTCH,
        }),
        6 => Button::Scroll(ScrollDelta::Lines {
            x: -LINES_PER_NOTCH,
            y: 0.0,
        }),
        7 => Button::Scroll(ScrollDelta::Lines {
            x: LINES_PER_NOTCH,
            y: 0.0,
        }),
        // 8 and 9 are the thumb buttons on every mouse that has them, and X11
        // numbers them after the wheel rather than by their evdev codes.
        8 => Button::Press(PointerButton::Back),
        9 => Button::Press(PointerButton::Forward),
        // Anything higher is translated back to the evdev code the X input
        // driver derived it from, so `PointerButton::Other(n)` names the same
        // physical button on both Linux backends.
        //
        // It used to keep the raw X11 number, on the reasoning that a raw code
        // is what a profile round-trips. That is true and was the wrong
        // conclusion: `linux::keymap::pointer_button` stores the *evdev* code,
        // and its own documentation says two backends must not disagree about
        // what `Other(2)` means — so a profile saved under Wayland bound a
        // different physical button when replayed under X11, on the same mouse.
        // The X11 number is the derived one, so it is the one that converts.
        other => Button::Press(PointerButton::Other(evdev_of(other))),
    }
}

/// The evdev code an X11 button number above 9 was derived from.
///
/// `xf86-input-libinput` — and `evdev` before it — assigns X11 button numbers
/// in a fixed order: 1–3 for the primary trio, 4–7 for the two scroll axes,
/// then 8 upward for the rest of the evdev button block starting at `BTN_SIDE`.
/// So 8 is `BTN_SIDE` (`0x113`), 9 is `BTN_EXTRA`, 10 is `BTN_FORWARD`, and the
/// arithmetic below continues that run.
///
/// The first two of those are named buttons and never reach here; the run
/// starts at 10 → `BTN_FORWARD`.
const fn evdev_of(x11_button: u8) -> u16 {
    /// `BTN_SIDE`, the evdev code X11 button 8 is derived from.
    const BTN_SIDE: u16 = 0x113;
    /// The first X11 button number in the derived run.
    const FIRST_DERIVED: u16 = 8;
    BTN_SIDE + (x11_button as u16 - FIRST_DERIVED)
}

/// `WM_SIZE_HINTS`, as `WM_NORMAL_HINTS` carries it.
///
/// Eighteen 32-bit words, in the order ICCCM froze them. Three of the fields
/// are marked obsolete by ICCCM itself (`x`, `y`, `width`, `height`) and are
/// still part of the struct, because the length is what a window manager reads
/// it by — a short property is ignored wholesale.
///
/// # This is where `ASPECT_HINT_HONORED` becomes true
///
/// `min_aspect` and `max_aspect` are two rationals, and setting both to the
/// same ratio is how a window is locked to it during an interactive resize.
/// Wayland's `xdg_toplevel` has min and max size and no aspect field at all, so
/// [`ShellCaps::ASPECT_HINT_HONORED`](crate::ShellCaps::ASPECT_HINT_HONORED)
/// has never been set by any backend before this one — see
/// [`X11Shell::caps`](super::X11Shell::caps) for the condition it is *still*
/// latched behind.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct SizeHints {
    /// Which of the fields below are meaningful.
    pub flags: u32,
    /// Obsolete position, kept for the length.
    pub x: i32,
    /// Obsolete position, kept for the length.
    pub y: i32,
    /// Obsolete size, kept for the length.
    pub width: i32,
    /// Obsolete size, kept for the length.
    pub height: i32,
    /// Smallest client area.
    pub min_width: i32,
    /// Smallest client area.
    pub min_height: i32,
    /// Largest client area.
    pub max_width: i32,
    /// Largest client area.
    pub max_height: i32,
    /// Resize granularity.
    pub width_inc: i32,
    /// Resize granularity.
    pub height_inc: i32,
    /// Numerator of the minimum aspect.
    pub min_aspect_num: i32,
    /// Denominator of the minimum aspect.
    pub min_aspect_den: i32,
    /// Numerator of the maximum aspect.
    pub max_aspect_num: i32,
    /// Denominator of the maximum aspect.
    pub max_aspect_den: i32,
    /// Base size, subtracted before applying `*_inc`.
    pub base_width: i32,
    /// Base size.
    pub base_height: i32,
    /// Gravity.
    pub win_gravity: i32,
}

impl SizeHints {
    /// `USSize` — the size came from the user.
    pub const US_SIZE: u32 = 1 << 1;
    /// `PMinSize`.
    pub const P_MIN_SIZE: u32 = 1 << 4;
    /// `PMaxSize`.
    pub const P_MAX_SIZE: u32 = 1 << 5;
    /// `PAspect`.
    pub const P_ASPECT: u32 = 1 << 7;

    /// The property's 18 words, ready for `ChangeProperty` with format 32.
    ///
    /// `i32` rather than `u32` because half the fields are signed in ICCCM and
    /// the X server does not care; the bytes are identical either way and the
    /// signedness is documentation.
    #[must_use]
    pub const fn to_words(self) -> [i32; 18] {
        [
            self.flags as i32,
            self.x,
            self.y,
            self.width,
            self.height,
            self.min_width,
            self.min_height,
            self.max_width,
            self.max_height,
            self.width_inc,
            self.height_inc,
            self.min_aspect_num,
            self.min_aspect_den,
            self.max_aspect_num,
            self.max_aspect_den,
            self.base_width,
            self.base_height,
            self.win_gravity,
        ]
    }

    /// The hints for a set of seam constraints.
    ///
    /// `resizable == false` is expressed as `min == max == current`, which is
    /// what ICCCM gives and what every window manager reads as "no resize
    /// handles". It is deliberately *not* the same as
    /// [`SizeConstraints`](crate::SizeConstraints) with equal min and max: the
    /// seam distinguishes them, and this is the encoding that makes both
    /// expressible at once.
    ///
    /// Sizes come in as [`LogicalSize`](crate::LogicalSize) and go out as
    /// device pixels, because `WM_NORMAL_HINTS` is in pixels — X11 has no
    /// logical unit. `scale_factor` is the window's, so a 320-logical-pixel
    /// minimum on a 2× desktop is a 640-pixel one to the window manager.
    #[must_use]
    pub fn from_constraints(
        constraints: crate::SizeConstraints,
        resizable: bool,
        current: crate::PhysicalSize,
        scale_factor: f64,
    ) -> Self {
        let mut hints = Self {
            flags: Self::US_SIZE,
            win_gravity: 1,
            ..Self::default()
        };
        if !resizable {
            hints.flags |= Self::P_MIN_SIZE | Self::P_MAX_SIZE;
            let (width, height) = clamp_extent(current.width, current.height);
            hints.min_width = width;
            hints.min_height = height;
            hints.max_width = width;
            hints.max_height = height;
            return hints;
        }
        if let Some(min) = constraints.min {
            let min = min.to_physical(scale_factor);
            hints.flags |= Self::P_MIN_SIZE;
            let (width, height) = clamp_extent(min.width, min.height);
            hints.min_width = width;
            hints.min_height = height;
        }
        if let Some(max) = constraints.max {
            let max = max.to_physical(scale_factor);
            hints.flags |= Self::P_MAX_SIZE;
            let (width, height) = clamp_extent(max.width, max.height);
            hints.max_width = width;
            hints.max_height = height;
        }
        if let Some(aspect) = constraints.aspect {
            // Both bounds equal: a *lock*, not a range. ICCCM allows a range
            // and no consumer of this seam has one, since `AspectRatio` is a
            // single ratio.
            hints.flags |= Self::P_ASPECT;
            let numerator = i32::try_from(aspect.numerator()).unwrap_or(i32::MAX);
            let denominator = i32::try_from(aspect.denominator()).unwrap_or(i32::MAX);
            hints.min_aspect_num = numerator;
            hints.min_aspect_den = denominator;
            hints.max_aspect_num = numerator;
            hints.max_aspect_den = denominator;
        }
        hints
    }
}

/// A `u32` extent as the `i32` ICCCM wants, without wrapping into a negative.
///
/// A window manager reading a negative minimum size does arbitrary things with
/// it, and a `u32` big enough to overflow could only come from a caller bug —
/// so it is clamped rather than propagated.
const fn clamp_extent(width: u32, height: u32) -> (i32, i32) {
    const fn clamp(value: u32) -> i32 {
        if value > i32::MAX as u32 {
            i32::MAX
        } else {
            value as i32
        }
    }
    (clamp(width), clamp(height))
}

/// What to do to `_NET_WM_STATE`, as the client message's first word.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum WmStateAction {
    /// `_NET_WM_STATE_REMOVE`.
    Remove = 0,
    /// `_NET_WM_STATE_ADD`.
    Add = 1,
}

/// The `_NET_WM_STATE` client message body, as five 32-bit words.
///
/// EWMH is explicit that a *mapped* window's state is changed by sending this
/// to the root window and **not** by writing the property: the window manager
/// owns the property once the window is managed, and a client that writes it
/// directly has its write silently reverted on the next state change. The
/// property is only ours to set before the window is first mapped, which is why
/// [`X11Shell::create_window`](super::X11Shell) does both — the property for
/// the initial state, this message for every change after.
///
/// The fifth word is the *source indication*: `1` means "a normal application",
/// and `2` means "a pager". Sending `0` is the legacy value and some window
/// managers treat it as untrusted, so it is never sent.
#[must_use]
pub const fn net_wm_state_message(action: WmStateAction, first: u32, second: u32) -> [u32; 5] {
    [action as u32, first, second, 1, 0]
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{AspectRatio, LogicalSize, PhysicalSize, SizeConstraints};

    #[test]
    fn x11_keycodes_are_evdev_plus_eight_and_the_low_ones_name_nothing() {
        // The whole reason this module exists. `KEY_A` is evdev 30 and X11 38
        // — the number in every keymap file and in `xev` output.
        assert_eq!(scancode(38), Some(30));
        assert_eq!(key_code(38), Some(KeyCode::KeyA));
        assert_eq!(key_code(25), Some(KeyCode::KeyW), "evdev 17");
        assert_eq!(key_code(39), Some(KeyCode::KeyS), "evdev 31");
        assert_eq!(key_code(40), Some(KeyCode::KeyD), "evdev 32");
        assert_eq!(key_code(65), Some(KeyCode::Space), "evdev 57");
        assert_eq!(key_code(9), Some(KeyCode::Escape), "evdev 1");

        // 0 is the protocol's "no key" and 1..8 are reserved. Subtracting
        // blindly would make keycode 1 into evdev 249, and keycode 0 wrap.
        for reserved in 0..8u8 {
            assert_eq!(scancode(reserved), None, "keycode {reserved}");
            assert_eq!(key_code(reserved), None, "keycode {reserved}");
        }
        assert_eq!(scancode(8), Some(0), "the offset itself is evdev 0");
    }

    #[test]
    fn the_wheel_is_four_buttons_and_only_the_press_scrolls() {
        // Forwarding the release too would double every scroll, and treating
        // 4-7 as buttons would give the UI four phantom buttons and no scroll.
        assert_eq!(
            button(4, true),
            Button::Scroll(ScrollDelta::Lines { x: 0.0, y: 1.0 }),
            "wheel up"
        );
        assert_eq!(
            button(5, true),
            Button::Scroll(ScrollDelta::Lines { x: 0.0, y: -1.0 }),
            "wheel down"
        );
        assert_eq!(
            button(6, true),
            Button::Scroll(ScrollDelta::Lines { x: -1.0, y: 0.0 }),
            "tilt left"
        );
        assert_eq!(
            button(7, true),
            Button::Scroll(ScrollDelta::Lines { x: 1.0, y: 0.0 }),
            "tilt right"
        );
        for notch in 4..=7u8 {
            assert_eq!(button(notch, false), Button::ScrollRelease, "{notch}");
        }
    }

    #[test]
    fn x11_numbers_the_middle_button_two_not_three() {
        // The classic off-by-one: every other platform's second button is
        // right, X11's is middle.
        assert_eq!(button(1, true), Button::Press(PointerButton::Left));
        assert_eq!(button(2, true), Button::Press(PointerButton::Middle));
        assert_eq!(button(3, true), Button::Press(PointerButton::Right));
        assert_eq!(button(8, true), Button::Press(PointerButton::Back));
        assert_eq!(button(9, true), Button::Press(PointerButton::Forward));
        // A gaming mouse's tenth button reports the *evdev* code, which is
        // what the Wayland backend reports for the same physical button — so an
        // input profile binds the same thing under either.
        assert_eq!(
            button(10, true),
            Button::Press(crate::linux::keymap::pointer_button(0x115))
        );
        assert_eq!(button(10, true), Button::Press(PointerButton::Other(0x115)));
        assert_eq!(
            button(10, false),
            Button::Press(PointerButton::Other(0x115))
        );
        assert_eq!(button(11, true), Button::Press(PointerButton::Other(0x116)));
        // And the two named thumb buttons agree with the Wayland table too.
        assert_eq!(
            button(8, true),
            Button::Press(crate::linux::keymap::pointer_button(0x113))
        );
        assert_eq!(
            button(9, true),
            Button::Press(crate::linux::keymap::pointer_button(0x114))
        );
    }

    #[test]
    fn size_hints_are_eighteen_words_with_the_flags_first() {
        // A window manager reads this property by length. Nineteen words, or
        // seventeen, and the whole thing is ignored — silently.
        let hints = SizeHints::from_constraints(
            SizeConstraints::NONE,
            true,
            PhysicalSize::new(800, 600),
            1.0,
        );
        let words = hints.to_words();
        assert_eq!(words.len(), 18);
        assert_eq!(words[0], SizeHints::US_SIZE as i32);
        assert_eq!(words[17], 1, "NorthWestGravity");
    }

    #[test]
    fn an_aspect_lock_sets_both_bounds_to_the_same_ratio() {
        // The bit no backend has ever been able to set. A range would let the
        // window manager pick anything between; equal bounds are a lock.
        let hints = SizeHints::from_constraints(
            SizeConstraints::NONE.with_aspect(AspectRatio::WIDESCREEN),
            true,
            PhysicalSize::new(800, 600),
            1.0,
        );
        assert!(hints.flags & SizeHints::P_ASPECT != 0);
        assert_eq!(hints.min_aspect_num, 16);
        assert_eq!(hints.min_aspect_den, 9);
        assert_eq!(
            (hints.min_aspect_num, hints.min_aspect_den),
            (hints.max_aspect_num, hints.max_aspect_den),
            "a range would not be a lock"
        );
        let words = hints.to_words();
        assert_eq!(&words[11..15], &[16, 9, 16, 9]);
    }

    #[test]
    fn constraints_are_logical_and_the_hints_are_pixels() {
        // `WM_NORMAL_HINTS` has no logical unit, so the scale has to be applied
        // here or a 320-logical minimum becomes a 320-*pixel* one on a 2×
        // desktop — a window half the size the caller asked for.
        let constraints = SizeConstraints {
            min: Some(LogicalSize::new(320.0, 240.0)),
            max: Some(LogicalSize::new(1920.0, 1080.0)),
            aspect: None,
        };
        let one = SizeHints::from_constraints(constraints, true, PhysicalSize::new(800, 600), 1.0);
        assert_eq!((one.min_width, one.min_height), (320, 240));
        assert_eq!((one.max_width, one.max_height), (1920, 1080));

        let two = SizeHints::from_constraints(constraints, true, PhysicalSize::new(800, 600), 2.0);
        assert_eq!((two.min_width, two.min_height), (640, 480));
        assert_eq!((two.max_width, two.max_height), (3840, 2160));
        assert_eq!(
            two.flags & (SizeHints::P_MIN_SIZE | SizeHints::P_MAX_SIZE),
            SizeHints::P_MIN_SIZE | SizeHints::P_MAX_SIZE
        );
    }

    #[test]
    fn a_non_resizable_window_pins_min_and_max_to_its_current_size() {
        // The distinction the seam draws and ICCCM does not: `resizable:
        // false` removes the resize handles, and it does it by pinning both
        // bounds — even when the caller also passed a min/max range, which is
        // then irrelevant.
        let hints = SizeHints::from_constraints(
            SizeConstraints::min(LogicalSize::new(100.0, 100.0)),
            false,
            PhysicalSize::new(1280, 720),
            1.0,
        );
        assert_eq!((hints.min_width, hints.min_height), (1280, 720));
        assert_eq!((hints.max_width, hints.max_height), (1280, 720));
        assert_eq!(hints.flags & SizeHints::P_ASPECT, 0, "no aspect was asked");
    }

    #[test]
    fn the_state_message_names_a_normal_application_as_its_source() {
        // Source `0` is the legacy value and some window managers distrust it.
        let add = net_wm_state_message(WmStateAction::Add, 42, 0);
        assert_eq!(add, [1, 42, 0, 1, 0]);
        let remove = net_wm_state_message(WmStateAction::Remove, 42, 0);
        assert_eq!(remove, [0, 42, 0, 1, 0]);
        assert_eq!(add[3], 1, "source indication: normal application");
    }
}
