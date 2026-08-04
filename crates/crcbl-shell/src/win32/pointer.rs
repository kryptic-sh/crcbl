//! The mouse, arithmetically: button messages, wheel detents, raw reports and
//! the two pieces of cursor bookkeeping that are counters rather than calls.
//!
//! Pure, and for the reason [`geometry`](super::geometry) states — every
//! function here is checked by `cargo test -p crcbl-shell` on the machine this
//! engine is developed on, rather than by a CI runner nobody is watching. Which
//! matters most for the two that are wrong-but-plausible when they are wrong:
//! a sign-extension that turns a pointer at `x = -3` into one at `x = 65533`,
//! and a raw report whose coordinates are a *position* being read as a delta.
//!
//! # Coordinates in a message are signed, and Win32 does not say so
//!
//! `lParam` on a mouse message packs two 16-bit coordinates. They are **signed**
//! — a pointer can legitimately be at a negative client coordinate while a
//! button is captured outside the window, and on a multi-monitor desktop a
//! screen coordinate is negative on any monitor left of or above the primary.
//! The SDK's own `GET_X_LPARAM` casts through `short` for exactly this, and the
//! `LOWORD` macro next to it does not; picking the wrong one of the two is the
//! single most common Win32 pointer bug. [`point`] is the cast, in one place,
//! named.
//!
//! # A wheel is `WHEEL_DELTA`, and `WHEEL_DELTA` is not one
//!
//! `WM_MOUSEWHEEL` reports 120 per detent, and a high-resolution wheel reports
//! fractions of it — 40, or 30, several times per physical click. Dividing as an
//! integer throws those away and makes such a mouse scroll in nothing but
//! multiples of a full line, so [`wheel`] divides in floating point and produces
//! a fractional [`ScrollDelta::Lines`]. It stays `Lines` rather than becoming
//! `Pixels`: the device is reporting detents, and the lines-to-pixels factor is
//! a policy decision `crcbl_core::input` puts above this crate.

use crcbl_core::input::{ButtonState, PointerButton, ScrollDelta};

use crate::CursorIcon;

use super::ffi::{msg, value};

/// The `(x, y)` a mouse message's `lParam` packs, **sign-extended**.
///
/// See the [module docs](self). Client coordinates for every message except the
/// two wheel ones, which carry screen coordinates instead — a difference the
/// window procedure resolves with `ScreenToClient`, because it is the only party
/// that still knows which window the message was for.
#[must_use]
pub const fn point(l_param: isize) -> (i32, i32) {
    let x = (l_param & 0xFFFF) as u16 as i16;
    let y = ((l_param >> 16) & 0xFFFF) as u16 as i16;
    (x as i32, y as i32)
}

/// The button a `WM_*BUTTON*` message is about, and which way it went.
///
/// `None` for a message this is not one of, which is what makes the window
/// procedure's arm total without a second list of message numbers to keep in
/// step.
///
/// # The two thumb buttons share one message
///
/// `WM_XBUTTONDOWN` covers both, told apart by the **high** word of `wParam` —
/// the low word carries the modifier keys, so reading the wrong half reports
/// every thumb click as `XBUTTON1` whenever Shift is not held. `XBUTTON1` is
/// "back" and `XBUTTON2` is "forward", which is the same role assignment
/// `BTN_SIDE`/`BTN_EXTRA` have on Linux, so a binding made on one platform means
/// the same physical button on the other.
///
/// A mouse with more than five buttons reports the rest through its driver
/// rather than through `WM_XBUTTON*`, so there is no
/// [`Other`](PointerButton::Other) arm here: Windows has no message that would
/// reach it.
#[must_use]
pub const fn button(message: u32, w_param: usize) -> Option<(PointerButton, ButtonState)> {
    let state = match message {
        msg::L_BUTTON_DOWN | msg::R_BUTTON_DOWN | msg::M_BUTTON_DOWN | msg::X_BUTTON_DOWN => {
            ButtonState::Pressed
        }
        msg::L_BUTTON_UP | msg::R_BUTTON_UP | msg::M_BUTTON_UP | msg::X_BUTTON_UP => {
            ButtonState::Released
        }
        _ => return None,
    };
    let button = match message {
        msg::L_BUTTON_DOWN | msg::L_BUTTON_UP => PointerButton::Left,
        msg::R_BUTTON_DOWN | msg::R_BUTTON_UP => PointerButton::Right,
        msg::M_BUTTON_DOWN | msg::M_BUTTON_UP => PointerButton::Middle,
        // The high word, not the low one.
        _ => match ((w_param >> 16) & 0xFFFF) as u32 {
            value::XBUTTON1 => PointerButton::Back,
            value::XBUTTON2 => PointerButton::Forward,
            // A driver reporting a third X button is not something Windows
            // documents; naming it `Back` would be a guess.
            _ => return None,
        },
    };
    Some((button, state))
}

/// Which bit of the held-button mask a button owns.
///
/// The mask exists so that the mouse capture is taken on the *first* button down
/// and released on the *last* button up — see [`proc`](super::proc). A count
/// would do for a well-behaved stream and is wrong for a real one: a button
/// pressed before the window existed produces a release with no press, and a
/// count would then go negative and never release the capture.
#[must_use]
pub const fn button_bit(button: PointerButton) -> u32 {
    match button {
        PointerButton::Left => 1 << 0,
        PointerButton::Right => 1 << 1,
        PointerButton::Middle => 1 << 2,
        PointerButton::Back => 1 << 3,
        PointerButton::Forward => 1 << 4,
        PointerButton::Other(_) => 1 << 5,
    }
}

/// A wheel message's detents as the seam's scroll delta.
///
/// `ticks` is the **signed** high word of `wParam`; `horizontal` picks the axis.
/// Both platforms' sign conventions already agree with the seam's — a positive
/// `WM_MOUSEWHEEL` is away from the user and a positive `WM_MOUSEHWHEEL` is to
/// the right — so there is no negation here, and adding one "to match" is how a
/// backend ends up scrolling backwards.
#[must_use]
pub fn wheel(horizontal: bool, ticks: i16) -> ScrollDelta {
    let lines = f32::from(ticks) / f32::from(value::WHEEL_DELTA);
    if horizontal {
        ScrollDelta::Lines { x: lines, y: 0.0 }
    } else {
        ScrollDelta::Lines { x: 0.0, y: lines }
    }
}

/// Turns a stream of `RAWMOUSE` reports into relative deltas.
///
/// # `RAWMOUSE` reports a delta *or* a position, and the caller does not choose
///
/// The obvious reading of raw input is that `lLastX`/`lLastY` are the movement
/// since the last report — and for a USB mouse plugged into the machine, they
/// are. They are not for:
///
/// * a **remote desktop** session, where the client sends absolute pointer
///   positions and `mstsc`'s virtual mouse reports them through;
/// * a **tablet or touch digitizer**, which has no notion of relative motion;
/// * several **virtual machines**' guest pointer drivers, which use an absolute
///   device precisely so the guest cursor tracks the host's.
///
/// In all three `usFlags` carries [`MOUSE_MOVE_ABSOLUTE`](value::MOUSE_MOVE_ABSOLUTE)
/// and the coordinates are normalized to `0..=65535` over either the primary
/// monitor or — with [`MOUSE_VIRTUAL_DESKTOP`](value::MOUSE_VIRTUAL_DESKTOP) —
/// the whole desktop. Read as a delta, the first report moves a first-person
/// camera roughly thirty thousand pixels, which is the "my mouse look spins to
/// the ceiling over RDP" bug in one sentence.
///
/// So absolute reports are **differenced** against the previous one, in pixels,
/// and the first report of a run yields nothing: there is no previous position
/// to subtract, and inventing one is the same bug in a smaller size. Switching
/// coordinate spaces mid-stream — a monitor unplugged under a virtual-desktop
/// device — also drops one report rather than reporting the jump, which is what
/// the space check is for.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct RawMotion {
    /// The previous absolute sample, and the space it was measured in.
    ///
    /// `None` for a relative device, which never needs one.
    last: Option<(u16, i32, i32)>,
}

impl RawMotion {
    /// The `(dx, dy)` a report describes, in device pixels.
    ///
    /// `screen` is the extent the absolute range maps onto — the primary
    /// monitor's, or the virtual desktop's, chosen by
    /// [`is_virtual_desktop`](Self::is_virtual_desktop). It is unused for a
    /// relative report.
    ///
    /// `None` means "nothing to report": a zero-motion report (raw input sends
    /// them for button-only transitions), or the first absolute sample of a run.
    pub fn delta(&mut self, flags: u16, x: i32, y: i32, screen: (i32, i32)) -> Option<(f64, f64)> {
        if flags & value::MOUSE_MOVE_ABSOLUTE == 0 {
            // A relative device. Any previous absolute sample is from a
            // different device and must not be differenced against.
            self.last = None;
            if x == 0 && y == 0 {
                return None;
            }
            return Some((f64::from(x), f64::from(y)));
        }

        // Normalized to `0..=65535` over `screen`, which is the whole reason the
        // extent has to be passed in: the same report means different pixel
        // distances on a 1080p monitor and a 4K one.
        let space = flags & value::MOUSE_VIRTUAL_DESKTOP;
        let to_pixels = |value: i32, extent: i32| -> i32 {
            (i64::from(value) * i64::from(extent) / i64::from(value::ABSOLUTE_RANGE)) as i32
        };
        let position = (to_pixels(x, screen.0), to_pixels(y, screen.1));
        let previous = self.last.replace((space, position.0, position.1));
        let (previous_space, previous_x, previous_y) = previous?;
        if previous_space != space {
            // The device changed which rectangle it normalizes against, so the
            // difference between the two samples is not a movement.
            return None;
        }
        let delta = (position.0 - previous_x, position.1 - previous_y);
        if delta == (0, 0) {
            return None;
        }
        Some((f64::from(delta.0), f64::from(delta.1)))
    }

    /// Whether a report's coordinates are normalized over the virtual desktop
    /// rather than over the primary monitor.
    ///
    /// Answered here rather than at the call site so that the flag is decoded in
    /// the same module that consumes it, and so the caller reads one screen
    /// extent rather than choosing between two `GetSystemMetrics` pairs it has
    /// to keep in step with this function.
    #[must_use]
    pub const fn is_virtual_desktop(flags: u16) -> bool {
        flags & value::MOUSE_VIRTUAL_DESKTOP != 0
    }
}

/// Keeps `ShowCursor`'s reference count balanced.
///
/// # The classic bug in this API, made into a value
///
/// `ShowCursor` does not take a visibility; it increments or decrements a
/// **per-thread display count** and the cursor is drawn while the count is at
/// least zero. So two calls to hide need two calls to show, a hide issued twice
/// leaves the cursor invisible after one show, and a show issued when nothing
/// was hidden makes the *next* hide a no-op. Every one of those is a cursor that
/// is missing or stuck for the rest of the process's life, with no error
/// anywhere.
///
/// This type is the whole of the fix: it records what this shell has asked for,
/// and answers each request with the number of `ShowCursor` calls actually owed
/// — which is zero whenever the state already matches, and never more than one.
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
    /// One `ShowCursor(FALSE)`.
    Hide,
    /// One `ShowCursor(TRUE)`, releasing the hide this shell holds.
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

/// The system cursor a [`CursorIcon`] is drawn with.
///
/// An `IDC_*` integer resource id for `LoadCursorW`, which is the whole of
/// Windows' stock cursor set. Three of the seam's shapes have no exact stock
/// equivalent and are approximated rather than dropped, which is stated here
/// because "approximated" is invisible on screen:
///
/// | Shape | Drawn as | Why |
/// | --- | --- | --- |
/// | [`Grab`](CursorIcon::Grab), [`Grabbing`](CursorIcon::Grabbing) | `IDC_SIZEALL` | Windows has no open/closed hand; the four-way arrow is what File Explorer uses for a drag and is the closest honest signal |
/// | [`ResizeColumn`](CursorIcon::ResizeColumn) | `IDC_SIZEWE` | a split-pane divider is a horizontal resize with a different decoration, and the decoration is the part Windows does not have |
/// | [`ResizeRow`](CursorIcon::ResizeRow) | `IDC_SIZENS` | the same, the other way round |
///
/// Exhaustive despite [`CursorIcon`] being `#[non_exhaustive]`, because this
/// crate defines it: a shape added later fails to compile here rather than
/// silently becoming an arrow.
#[must_use]
pub const fn cursor_id(icon: CursorIcon) -> usize {
    match icon {
        CursorIcon::Default => value::IDC_ARROW,
        CursorIcon::Pointer => value::IDC_HAND,
        CursorIcon::Text => value::IDC_IBEAM,
        CursorIcon::Crosshair => value::IDC_CROSS,
        CursorIcon::Wait => value::IDC_WAIT,
        CursorIcon::Progress => value::IDC_APP_STARTING,
        CursorIcon::NotAllowed => value::IDC_NO,
        CursorIcon::Grab | CursorIcon::Grabbing | CursorIcon::Move => value::IDC_SIZE_ALL,
        CursorIcon::ResizeNorthSouth | CursorIcon::ResizeRow => value::IDC_SIZE_NS,
        CursorIcon::ResizeEastWest | CursorIcon::ResizeColumn => value::IDC_SIZE_WE,
        CursorIcon::ResizeNorthEastSouthWest => value::IDC_SIZE_NESW,
        CursorIcon::ResizeNorthWestSouthEast => value::IDC_SIZE_NWSE,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The `lParam` a mouse message packs two coordinates into.
    const fn packed(x: i32, y: i32) -> isize {
        (((y as u16 as usize) << 16) | (x as u16 as usize)) as isize
    }

    /// A `wParam` for `WM_XBUTTON*`, with modifier bits in the low word to
    /// prove the high one is what is read.
    const fn x_button(which: u32) -> usize {
        ((which as usize) << 16) | 0x000C
    }

    #[test]
    fn a_negative_coordinate_stays_negative_rather_than_becoming_sixty_five_thousand() {
        // The `GET_X_LPARAM`-versus-`LOWORD` bug. Reachable every time a button
        // is dragged out of the window, because the capture keeps delivering
        // motion at coordinates outside the client area.
        assert_eq!(point(packed(100, 50)), (100, 50));
        assert_eq!(point(packed(-3, -1)), (-3, -1));
        assert_eq!(point(packed(-1920, 40)), (-1920, 40));
        assert_eq!(point(packed(0, 0)), (0, 0));
        // The extremes of the signed 16-bit range the message can carry.
        assert_eq!(point(packed(32_767, -32_768)), (32_767, -32_768));
    }

    #[test]
    fn the_five_buttons_decode_and_the_thumb_pair_comes_from_the_high_word() {
        assert_eq!(
            button(msg::L_BUTTON_DOWN, 0),
            Some((PointerButton::Left, ButtonState::Pressed))
        );
        assert_eq!(
            button(msg::L_BUTTON_UP, 0),
            Some((PointerButton::Left, ButtonState::Released))
        );
        assert_eq!(
            button(msg::R_BUTTON_DOWN, 0),
            Some((PointerButton::Right, ButtonState::Pressed))
        );
        assert_eq!(
            button(msg::M_BUTTON_DOWN, 0),
            Some((PointerButton::Middle, ButtonState::Pressed))
        );

        // The half that is easy to get wrong: the low word is the modifier
        // mask, so reading it reports every thumb click as the same button.
        assert_eq!(
            button(msg::X_BUTTON_DOWN, x_button(value::XBUTTON1)),
            Some((PointerButton::Back, ButtonState::Pressed))
        );
        assert_eq!(
            button(msg::X_BUTTON_UP, x_button(value::XBUTTON2)),
            Some((PointerButton::Forward, ButtonState::Released))
        );
        assert_eq!(button(msg::X_BUTTON_DOWN, x_button(3)), None);

        // Anything that is not a button message.
        assert_eq!(button(msg::MOUSE_MOVE, 0), None);
        assert_eq!(button(msg::MOUSE_WHEEL, 0), None);
    }

    /// The thumb buttons name the same physical button as the Linux backends'.
    ///
    /// Linux-only because [`linux::keymap`](crate::linux::keymap) is compiled
    /// there and only there. The claim it checks is a cross-platform one — an
    /// input profile saved on Windows must bind the same button when replayed
    /// on Linux — and one side of it can only be read where both tables exist.
    #[cfg(target_os = "linux")]
    #[test]
    fn the_thumb_buttons_agree_with_the_linux_backends() {
        assert_eq!(
            crate::linux::keymap::pointer_button(0x113),
            PointerButton::Back,
            "BTN_SIDE and XBUTTON1"
        );
        assert_eq!(
            crate::linux::keymap::pointer_button(0x114),
            PointerButton::Forward,
            "BTN_EXTRA and XBUTTON2"
        );
        assert_eq!(
            button(msg::X_BUTTON_DOWN, x_button(value::XBUTTON1)).map(|(button, _)| button),
            Some(crate::linux::keymap::pointer_button(0x113))
        );
    }

    #[test]
    fn the_button_mask_gives_every_button_its_own_bit() {
        // A shared bit would release the mouse capture while another button is
        // still held, and the release that follows would land on whatever
        // window is under the pointer.
        let bits = [
            button_bit(PointerButton::Left),
            button_bit(PointerButton::Right),
            button_bit(PointerButton::Middle),
            button_bit(PointerButton::Back),
            button_bit(PointerButton::Forward),
        ];
        let mut unique = bits.to_vec();
        unique.sort_unstable();
        unique.dedup();
        assert_eq!(unique.len(), bits.len(), "{bits:?}");
        assert_eq!(bits.iter().fold(0, |all, bit| all | bit).count_ones(), 5);
    }

    #[test]
    fn one_detent_is_one_line_and_a_high_resolution_wheel_keeps_its_fraction() {
        assert_eq!(wheel(false, 120), ScrollDelta::Lines { x: 0.0, y: 1.0 });
        assert_eq!(wheel(false, -120), ScrollDelta::Lines { x: 0.0, y: -1.0 });
        assert_eq!(wheel(false, 240), ScrollDelta::Lines { x: 0.0, y: 2.0 });
        // Horizontal goes on the other axis, and positive is rightwards — the
        // same sign the X11 backend's tilt-right notch produces.
        assert_eq!(wheel(true, 120), ScrollDelta::Lines { x: 1.0, y: 0.0 });
        assert_eq!(wheel(true, -120), ScrollDelta::Lines { x: -1.0, y: 0.0 });

        // A high-resolution wheel: three reports of 40 make one line, and an
        // integer division would have made three zeroes.
        let ScrollDelta::Lines { y, .. } = wheel(false, 40) else {
            panic!("a wheel reports detents, not pixels");
        };
        assert!((y - 1.0 / 3.0).abs() < 1e-6, "{y}");
        assert!(!wheel(false, 40).is_zero());
        assert!(wheel(false, 0).is_zero());
    }

    #[test]
    fn a_relative_report_is_the_delta_and_a_zero_one_is_not_an_event() {
        let mut motion = RawMotion::default();
        let screen = (1920, 1080);
        assert_eq!(
            motion.delta(0, 7, -3, screen),
            Some((7.0, -3.0)),
            "relative motion passes through unscaled"
        );
        // Raw input sends zero-motion reports for button-only transitions;
        // forwarding them would be a stream of no-op camera updates.
        assert_eq!(motion.delta(0, 0, 0, screen), None);
    }

    #[test]
    fn an_absolute_report_is_differenced_instead_of_being_read_as_a_delta() {
        // The remote-desktop case. Read literally, the first report below is a
        // 30 000-pixel movement.
        let mut motion = RawMotion::default();
        let screen = (1920, 1080);
        let half = value::ABSOLUTE_RANGE / 2;
        assert_eq!(
            motion.delta(value::MOUSE_MOVE_ABSOLUTE, half, half, screen),
            None,
            "the first absolute sample has nothing to subtract from"
        );
        // A tenth of the range further right is 192 pixels on a 1920 monitor.
        let tenth = value::ABSOLUTE_RANGE / 10;
        let moved = motion
            .delta(value::MOUSE_MOVE_ABSOLUTE, half + tenth, half, screen)
            .expect("a second sample is a movement");
        assert!((moved.0 - 192.0).abs() <= 1.0, "{moved:?}");
        assert!(moved.1.abs() < 1.0, "{moved:?}");
        // Standing still is not a movement.
        assert_eq!(
            motion.delta(value::MOUSE_MOVE_ABSOLUTE, half + tenth, half, screen),
            None
        );
        // The same report on a 4K monitor is twice the distance, which is why
        // the extent is an argument rather than a constant.
        let mut wide = RawMotion::default();
        assert_eq!(
            wide.delta(value::MOUSE_MOVE_ABSOLUTE, half, half, (3840, 2160)),
            None
        );
        let moved = wide
            .delta(value::MOUSE_MOVE_ABSOLUTE, half + tenth, half, (3840, 2160))
            .expect("a movement");
        assert!((moved.0 - 384.0).abs() <= 1.0, "{moved:?}");
    }

    #[test]
    fn switching_between_absolute_and_relative_never_produces_a_jump() {
        // Two devices on one machine — a laptop trackpad and a remote session,
        // or a tablet beside a mouse. A stale sample from the other kind must
        // not be differenced against.
        let mut motion = RawMotion::default();
        let screen = (1920, 1080);
        assert_eq!(motion.delta(value::MOUSE_MOVE_ABSOLUTE, 0, 0, screen), None);
        assert_eq!(motion.delta(0, 5, 5, screen), Some((5.0, 5.0)));
        assert_eq!(
            motion.delta(value::MOUSE_MOVE_ABSOLUTE, 60_000, 60_000, screen),
            None,
            "the relative report cleared the absolute history"
        );

        // And a change of the rectangle the coordinates are normalized to.
        let virtual_desktop = value::MOUSE_MOVE_ABSOLUTE | value::MOUSE_VIRTUAL_DESKTOP;
        assert_eq!(motion.delta(virtual_desktop, 60_000, 60_000, screen), None);
        assert!(RawMotion::is_virtual_desktop(virtual_desktop));
        assert!(!RawMotion::is_virtual_desktop(value::MOUSE_MOVE_ABSOLUTE));
        assert!(!RawMotion::is_virtual_desktop(0));
    }

    #[test]
    fn hiding_the_cursor_twice_owes_one_show_and_not_two() {
        // The `ShowCursor` reference-count bug. Two hides and one show leave
        // the cursor invisible for the rest of the process's life.
        let mut visibility = Visibility::default();
        assert_eq!(visibility.want(true), Show::Hide);
        assert_eq!(visibility.want(true), Show::Nothing, "already hidden");
        assert_eq!(visibility.want(false), Show::Reveal);
        // And a show with nothing hidden must not be issued either: it would
        // push the count to +1 and make the next hide a no-op.
        assert_eq!(visibility.want(false), Show::Nothing);
        assert_eq!(visibility.want(true), Show::Hide);
        assert_eq!(Visibility::default(), Visibility { hidden: false });
    }

    #[test]
    fn every_cursor_shape_maps_to_a_stock_windows_cursor() {
        // No shape falls through to the arrow by accident: `IDC_ARROW` is the
        // answer for exactly one of them.
        let shapes = [
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
        assert_eq!(
            shapes
                .iter()
                .filter(|shape| cursor_id(**shape) == value::IDC_ARROW)
                .count(),
            1,
            "only the default is an arrow"
        );
        for shape in shapes {
            let id = cursor_id(shape);
            assert!(
                (32_512..=32_651).contains(&id),
                "{shape:?} is not an IDC_* id: {id}"
            );
        }
        // The documented approximations, asserted so that changing one is a
        // decision rather than a drift.
        assert_eq!(cursor_id(CursorIcon::Grab), value::IDC_SIZE_ALL);
        assert_eq!(cursor_id(CursorIcon::ResizeColumn), value::IDC_SIZE_WE);
        assert_eq!(cursor_id(CursorIcon::ResizeRow), value::IDC_SIZE_NS);
    }
}
