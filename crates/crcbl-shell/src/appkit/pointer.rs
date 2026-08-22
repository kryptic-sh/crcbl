//! The mouse, arithmetically: buttons, scroll units, the coordinate conversions
//! and the two pieces of cursor bookkeeping that are a counter and a table
//! rather than calls.
//!
//! Pure, and for the reason [`geometry`](super::geometry) states — every
//! function here is checked by `cargo test -p crcbl-shell` on the machine this
//! engine is developed on, rather than by a CI runner nobody is watching. Which
//! matters most for the one that is wrong-but-plausible when it is wrong: the Y
//! axis, which points **up** in an `NSEvent`'s position and **down** in its
//! delta.
//!
//! # A position is flipped and a delta is not, and they arrive on one event
//!
//! `-[NSEvent locationInWindow]` is in AppKit's window space: origin at the
//! bottom-left, Y increasing upwards, in points. The seam's
//! [`PhysicalPoint`] is in the window's pixels from the **top**-left, so it
//! needs the flip [`view_point`] performs and the scale after it.
//!
//! `-[NSEvent deltaX]`/`deltaY` do **not**. They are the Quartz event's own
//! `kCGMouseEventDelta*`, and Quartz's event space is the one CoreGraphics uses
//! everywhere else — origin at the top-left of the main display, Y increasing
//! downwards, which is already the seam's convention. So the delta crosses
//! unchanged while the position beside it is reflected, on the same event, and
//! "flip both because one needed it" is the mistake this module exists to make
//! visible.
//!
//! # A scroll is pixels or lines, and macOS is the only backend that has both
//!
//! `hasPreciseScrollingDeltas` is the device telling us which. A trackpad and a
//! high-resolution wheel report **pixels** — continuous, already accelerated by
//! the system, and the thing that makes a scroll feel native; a notched wheel
//! reports **lines**. [`ScrollDelta`] keeps the two apart precisely so a backend
//! does not have to invent a conversion factor, and this is the first backend
//! where both arms are reachable: Win32 has only detents, and the X11 backend
//! only has smooth-scroll valuators where the driver exposes them.

use core::ffi::CStr;

use crcbl_core::input::{PointerButton, ScrollDelta};

use crate::{CursorIcon, PhysicalPoint};

use super::ffi::{NSPoint, NSSize};

/// The pointer button an `NSEvent`'s `buttonNumber` names.
///
/// # The numbering is the device's, and it goes past five
///
/// `0` and `1` are the primary and secondary buttons — already swapped by the
/// system for a left-handed mouse, which is the swap
/// [`PointerButton`] says must not be applied again. `2` is the wheel click, and
/// `3` and `4` are the thumb pair in the same role assignment
/// `BTN_SIDE`/`BTN_EXTRA` and `XBUTTON1`/`XBUTTON2` have, so a binding made on
/// one platform means the same physical button on the others.
///
/// Above that, macOS keeps counting: `otherMouseDown:` really does arrive with
/// `buttonNumber` 5, 6 and 7 from a mouse that has them, where Win32 has no
/// message that could report one. That is what
/// [`Other`](PointerButton::Other) is for, and this is the only backend of the
/// four that can reach it.
#[must_use]
pub const fn button(number: isize) -> PointerButton {
    match number {
        0 => PointerButton::Left,
        1 => PointerButton::Right,
        2 => PointerButton::Middle,
        3 => PointerButton::Back,
        4 => PointerButton::Forward,
        // Negative is not something AppKit produces; folding it onto zero would
        // report a stray report as a left click, so it becomes an `Other` that
        // nothing binds by accident.
        other if other > 0 => PointerButton::Other(other as u16),
        _ => PointerButton::Other(u16::MAX),
    }
}

/// A `scrollWheel:` event's deltas as the seam's scroll delta.
///
/// `precise` is `hasPreciseScrollingDeltas`; the two values are
/// `scrollingDeltaX`/`scrollingDeltaY`, which are pixels when it is set and
/// lines when it is not.
///
/// # The sign is the system's answer, not ours
///
/// A positive `scrollingDeltaY` is content moving up — a wheel turned away from
/// the user — which is already what [`ScrollDelta`] documents, so there is no
/// negation here and adding one "to match" is how a backend ends up scrolling
/// backwards.
///
/// **`isDirectionInvertedFromDevice` is deliberately not read.** macOS's natural
/// scrolling flips the sign of these values *before* they reach the application,
/// exactly as it swaps the buttons of a left-handed mouse before they reach it —
/// it is the user's setting, already applied, and un-applying it would give
/// every Mac player the opposite of what every other application does.
#[must_use]
pub fn scroll(precise: bool, x: f64, y: f64) -> ScrollDelta {
    if precise {
        ScrollDelta::Pixels { x, y }
    } else {
        ScrollDelta::Lines {
            x: x as f32,
            y: y as f32,
        }
    }
}

/// A point in a view's own coordinates as the seam's position in that window's
/// pixels.
///
/// `height` is the view's height in points and `scale` its
/// `backingScaleFactor`. The flip is about the view rather than about the
/// screen: [`geometry::Flip`](super::geometry::Flip) reconciles *desktop*
/// spaces, and this is a window-local reflection that has nothing to do with
/// where the window is.
///
/// Not clamped. A pointer is legitimately outside the view while a button is
/// held — AppKit keeps delivering `mouseDragged:` past the edge, as Win32 does
/// with a capture — and a negative window coordinate is the honest report of it.
#[must_use]
pub fn view_point(point: NSPoint, height: f64, scale: f64) -> PhysicalPoint {
    let scale = super::geometry::usable_scale(scale);
    PhysicalPoint::new(point.x * scale, (height - point.y) * scale)
}

/// The centre of a view, in the seam's pixels.
///
/// Where [`PointerMode::Locked`](crate::PointerMode) puts the cursor before it
/// freezes it, so that unlocking leaves it somewhere the user can find.
#[must_use]
pub fn centre_of(size: NSSize, scale: f64) -> PhysicalPoint {
    let scale = super::geometry::usable_scale(scale);
    PhysicalPoint::new(size.width * scale / 2.0, size.height * scale / 2.0)
}

/// A seam position in a window's pixels as the point in AppKit's window space
/// that `convertRectToScreen:` takes.
///
/// The inverse of [`view_point`], and the first of the **two** reflections a
/// warp crosses: this one is window-local and undoes the view's own Y flip;
/// [`geometry::Flip::point`](super::geometry::Flip::point) is the second and
/// undoes the desktop's. Getting either one wrong puts the cursor the same
/// distance on the wrong side of the middle, which is the failure that looks
/// like a working warp until the window is not centred.
#[must_use]
pub fn warp_source(position: PhysicalPoint, height: f64, scale: f64) -> NSPoint {
    let scale = super::geometry::usable_scale(scale);
    NSPoint {
        x: position.x / scale,
        y: height - position.y / scale,
    }
}

/// Keeps `+[NSCursor hide]`'s reference count balanced.
///
/// # The same bug Win32's `ShowCursor` has, in a second costume
///
/// `hide` and `unhide` are **counted**: `hide` adds one to a hidden-cursor count
/// and `unhide` takes one away, and the cursor is drawn while the count is zero.
/// So two hides need two unhides, and an unhide issued when nothing was hidden
/// is documented as doing nothing — which is the half that differs from Win32,
/// where the same mistake pushes the count positive and makes the *next* hide a
/// no-op. Either way the failure is a cursor missing for the rest of the
/// process's life with no error anywhere.
///
/// This type is the whole of the fix, and it is the same type
/// `win32::pointer::Visibility` is: it records what this shell has asked for
/// and answers each request with the calls actually owed, which is zero
/// whenever the state already matches and never more than one.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Visibility {
    /// Whether this shell currently holds one hide.
    hidden: bool,
}

/// What [`Visibility::want`] says to do.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Show {
    /// Nothing: the cursor is already in the state that was asked for.
    Nothing,
    /// One `+[NSCursor hide]`.
    Hide,
    /// One `+[NSCursor unhide]`, releasing the hide this shell holds.
    Reveal,
}

impl Visibility {
    /// The call owed to reach `hidden`, and the state updated as if it was made.
    ///
    /// Idempotent by construction: asking twice for the same thing owes one call
    /// and then none.
    pub const fn want(&mut self, hidden: bool) -> Show {
        if hidden == self.hidden {
            return Show::Nothing;
        }
        self.hidden = hidden;
        if hidden { Show::Hide } else { Show::Reveal }
    }
}

/// The `NSCursor` class method that answers with the cursor for a shape.
///
/// A selector rather than an integer, because macOS's stock cursors are
/// **objects reached by name** where Windows' are numbered resources — there is
/// no `IDC_*` equivalent to look up.
///
/// # Four shapes macOS has no public cursor for
///
/// AppKit's published set is arrow, I-beam, pointing hand, open and closed hand,
/// crosshair, the four axis-aligned resizes, drag-link, drag-copy,
/// disappearing-item, contextual-menu and operation-not-allowed. It has **no**
/// busy cursor and **no** diagonal resize: the ones AppKit itself draws are
/// `_windowResizeNorthEastSouthWestCursor` and `busyButClickableCursor`, which
/// are private selectors this backend does not call. So four shapes are
/// approximated, and they are named here because "approximated" is invisible on
/// screen:
///
/// | Shape | Drawn as | Why |
/// | --- | --- | --- |
/// | [`Wait`](CursorIcon::Wait), [`Progress`](CursorIcon::Progress) | `arrowCursor` | the spinning wait indicator is the *system's*, shown when an application stops answering; there is no public way to ask for it, and a crosshair or a not-allowed sign would say something false |
/// | [`ResizeNorthEastSouthWest`](CursorIcon::ResizeNorthEastSouthWest), [`ResizeNorthWestSouthEast`](CursorIcon::ResizeNorthWestSouthEast) | `arrowCursor` | there is no public diagonal; substituting a horizontal or vertical one would point the user at the wrong axis, which is worse than pointing at nothing |
///
/// [`Move`](CursorIcon::Move) is `closedHandCursor` rather than an approximation
/// of the four-way arrow Windows uses, because a closed hand *is* what macOS
/// shows while something is being dragged — the platforms disagree about the
/// picture, not about the meaning.
///
/// Exhaustive despite [`CursorIcon`] being `#[non_exhaustive]`, because this
/// crate defines it: a shape added later fails to compile here rather than
/// silently becoming an arrow.
#[must_use]
pub const fn cursor_selector(icon: CursorIcon) -> &'static CStr {
    match icon {
        CursorIcon::Default => c"arrowCursor",
        CursorIcon::Pointer => c"pointingHandCursor",
        CursorIcon::Text => c"IBeamCursor",
        CursorIcon::Crosshair => c"crosshairCursor",
        CursorIcon::NotAllowed => c"operationNotAllowedCursor",
        CursorIcon::Grab => c"openHandCursor",
        CursorIcon::Grabbing | CursorIcon::Move => c"closedHandCursor",
        CursorIcon::ResizeNorthSouth | CursorIcon::ResizeRow => c"resizeUpDownCursor",
        CursorIcon::ResizeEastWest | CursorIcon::ResizeColumn => c"resizeLeftRightCursor",
        // The four with no public answer; see the table above.
        CursorIcon::Wait
        | CursorIcon::Progress
        | CursorIcon::ResizeNorthEastSouthWest
        | CursorIcon::ResizeNorthWestSouthEast => c"arrowCursor",
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Every shape the seam names, so a new one has to be added here too.
    const SHAPES: [CursorIcon; 16] = [
        CursorIcon::Default,
        CursorIcon::Pointer,
        CursorIcon::Text,
        CursorIcon::Crosshair,
        CursorIcon::Wait,
        CursorIcon::Progress,
        CursorIcon::NotAllowed,
        CursorIcon::Grab,
        CursorIcon::Grabbing,
        CursorIcon::Move,
        CursorIcon::ResizeNorthSouth,
        CursorIcon::ResizeEastWest,
        CursorIcon::ResizeNorthEastSouthWest,
        CursorIcon::ResizeNorthWestSouthEast,
        CursorIcon::ResizeColumn,
        CursorIcon::ResizeRow,
    ];

    #[test]
    fn the_five_named_buttons_decode_and_the_rest_stay_bindable() {
        assert_eq!(button(0), PointerButton::Left);
        assert_eq!(button(1), PointerButton::Right);
        assert_eq!(button(2), PointerButton::Middle);
        assert_eq!(button(3), PointerButton::Back);
        assert_eq!(button(4), PointerButton::Forward);
        // The arm no other desktop backend can reach: `otherMouseDown:` really
        // does arrive with a sixth button.
        assert_eq!(button(5), PointerButton::Other(5));
        assert_eq!(button(31), PointerButton::Other(31));
        // And a number AppKit does not produce is not folded onto a real button.
        assert_eq!(button(-1), PointerButton::Other(u16::MAX));
    }

    /// The thumb buttons name the same physical button as the Linux backends'.
    ///
    /// Linux-only because [`linux::keymap`](crate::linux::keymap) is compiled
    /// there and only there. The claim is a cross-platform one — an input
    /// profile saved on a Mac must bind the same button when replayed on Linux —
    /// and one side of it can only be read where both tables exist.
    #[cfg(target_os = "linux")]
    #[test]
    fn the_thumb_buttons_agree_with_the_linux_backends() {
        assert_eq!(
            crate::linux::keymap::pointer_button(0x113),
            button(3),
            "BTN_SIDE and buttonNumber 3 are both Back"
        );
        assert_eq!(
            crate::linux::keymap::pointer_button(0x114),
            button(4),
            "BTN_EXTRA and buttonNumber 4 are both Forward"
        );
    }

    #[test]
    fn a_trackpad_scrolls_in_pixels_and_a_wheel_in_lines() {
        // The distinction `ScrollDelta` exists to keep, and this is the one
        // backend where a single call site reaches both arms.
        assert_eq!(
            scroll(true, 0.0, -13.5),
            ScrollDelta::Pixels { x: 0.0, y: -13.5 }
        );
        assert_eq!(
            scroll(false, 0.0, 1.0),
            ScrollDelta::Lines { x: 0.0, y: 1.0 }
        );
        // A wheel turned away from the user is positive on both platforms, so
        // there is no negation anywhere in this function.
        assert_eq!(
            scroll(false, 0.0, -1.0),
            ScrollDelta::Lines { x: 0.0, y: -1.0 }
        );
        // Horizontal rides the other axis and leaves the vertical alone.
        assert_eq!(
            scroll(true, 40.0, 0.0),
            ScrollDelta::Pixels { x: 40.0, y: 0.0 }
        );
        assert!(scroll(true, 0.0, 0.0).is_zero());
        assert!(scroll(false, 0.0, 0.0).is_zero());
        // A precise device's fractional pixel survives, which is the whole
        // reason it is not rounded into a line count.
        let ScrollDelta::Pixels { y, .. } = scroll(true, 0.0, 0.5) else {
            panic!("a precise device reports pixels");
        };
        assert!((y - 0.5).abs() < f64::EPSILON);
    }

    #[test]
    fn a_position_is_reflected_and_scaled_and_the_top_of_the_view_is_zero() {
        // A 720-point-tall view at 1x: AppKit's y = 720 is the top edge, which
        // the seam calls zero.
        let top = view_point(NSPoint { x: 100.0, y: 720.0 }, 720.0, 1.0);
        assert_eq!(top, PhysicalPoint::new(100.0, 0.0));
        let bottom = view_point(NSPoint { x: 0.0, y: 0.0 }, 720.0, 1.0);
        assert_eq!(bottom, PhysicalPoint::new(0.0, 720.0));

        // And at 2x, where mixing points and pixels would put the pointer at
        // half the distance from the corner on every Retina Mac.
        let retina = view_point(NSPoint { x: 100.0, y: 620.0 }, 720.0, 2.0);
        assert_eq!(retina, PhysicalPoint::new(200.0, 200.0));

        // A drag past the edge is outside the view, and reporting it as inside
        // would be a click at coordinates the user never pointed at.
        let outside = view_point(NSPoint { x: -8.0, y: 740.0 }, 720.0, 1.0);
        assert_eq!(outside, PhysicalPoint::new(-8.0, -20.0));
    }

    #[test]
    fn a_warp_target_is_the_exact_inverse_of_a_reported_position() {
        // The round trip is the property: warping to where the pointer already
        // is must not move it, whatever the scale or the view height.
        for (height, scale) in [(720.0, 1.0), (720.0, 2.0), (1055.5, 2.0)] {
            for point in [
                NSPoint { x: 0.0, y: 0.0 },
                NSPoint { x: 640.0, y: 360.0 },
                NSPoint {
                    x: 1279.5,
                    y: height,
                },
            ] {
                let reported = view_point(point, height, scale);
                let back = warp_source(reported, height, scale);
                assert!(
                    (back.x - point.x).abs() < 1e-9 && (back.y - point.y).abs() < 1e-9,
                    "{point:?} at {height}x{scale} came back as {back:?}"
                );
            }
        }
    }

    #[test]
    fn the_centre_of_a_retina_view_is_in_pixels_like_everything_else() {
        assert_eq!(
            centre_of(NSSize::new(1280.0, 720.0), 1.0),
            PhysicalPoint::new(640.0, 360.0)
        );
        assert_eq!(
            centre_of(NSSize::new(1280.0, 720.0), 2.0),
            PhysicalPoint::new(1280.0, 720.0)
        );
        // A nonsensical scale factor leaves the centre where 1x would put it,
        // for the reason `geometry::usable_scale` gives.
        assert_eq!(
            centre_of(NSSize::new(640.0, 480.0), 0.0),
            PhysicalPoint::new(320.0, 240.0)
        );
    }

    #[test]
    fn hiding_the_cursor_twice_owes_one_unhide_and_not_two() {
        // `+[NSCursor hide]` is counted, so two hides and one unhide leave the
        // cursor invisible for the rest of the process's life.
        let mut visibility = Visibility::default();
        assert_eq!(visibility.want(true), Show::Hide);
        assert_eq!(visibility.want(true), Show::Nothing, "already hidden");
        assert_eq!(visibility.want(false), Show::Reveal);
        // And an unhide with nothing hidden is not issued either.
        assert_eq!(visibility.want(false), Show::Nothing);
        assert_eq!(visibility.want(true), Show::Hide);
        assert_eq!(Visibility::default(), Visibility { hidden: false });
    }

    #[test]
    fn every_cursor_shape_names_a_public_nscursor_class_method() {
        // The check that matters here is not "each is distinct" — four of them
        // deliberately are not — but that none of them is a private selector.
        // AppKit's private cursors all begin with an underscore.
        for shape in SHAPES {
            let selector = cursor_selector(shape);
            let name = selector.to_str().expect("an ASCII selector");
            assert!(
                !name.starts_with('_'),
                "{shape:?} names {name}, a private selector"
            );
            assert!(name.ends_with("Cursor"), "{shape:?} names {name}");
        }
    }

    #[test]
    fn the_documented_approximations_are_asserted_rather_than_drifted_into() {
        // Changing one of these is a decision; without this test it is a
        // one-character edit nobody reviews.
        assert_eq!(cursor_selector(CursorIcon::Wait), c"arrowCursor");
        assert_eq!(cursor_selector(CursorIcon::Progress), c"arrowCursor");
        assert_eq!(
            cursor_selector(CursorIcon::ResizeNorthEastSouthWest),
            c"arrowCursor"
        );
        assert_eq!(
            cursor_selector(CursorIcon::ResizeNorthWestSouthEast),
            c"arrowCursor"
        );
        assert_eq!(cursor_selector(CursorIcon::Move), c"closedHandCursor");
        assert_eq!(
            cursor_selector(CursorIcon::ResizeColumn),
            c"resizeLeftRightCursor"
        );
        assert_eq!(
            cursor_selector(CursorIcon::ResizeRow),
            c"resizeUpDownCursor"
        );

        // Exactly five shapes fall back to the arrow: the default and the four
        // macOS has no public cursor for. A sixth would mean a shape lost its
        // own cursor without anybody saying so.
        let arrows = SHAPES
            .iter()
            .filter(|shape| cursor_selector(**shape) == c"arrowCursor")
            .count();
        assert_eq!(arrows, 5, "the default plus the four approximations");
    }
}
