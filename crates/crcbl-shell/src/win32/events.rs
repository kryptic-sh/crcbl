//! What the window procedure records, and how a storm of it is collapsed.
//!
//! # Why the procedure records instead of acting
//!
//! A `WNDPROC` is called **by the system, re-entrantly, while the shell is
//! somewhere else**. `SetWindowPos`, `ShowWindow`, `SetWindowTextW` and
//! `DestroyWindow` all dispatch messages synchronously before they return, so
//! any of them called from a [`Shell`](crate::Shell) method runs the window
//! procedure *inside* that method — with `&mut self` already borrowed, and with
//! the shell's own invariants halfway through an update.
//!
//! So the procedure translates nothing and looks nothing up. It converts a
//! message into one of these, pushes it, and returns; [`pump`](crate::Shell::pump)
//! resolves the `HWND` against the window pool, updates state and produces the
//! [`ShellEvent`](crate::ShellEvent). The X11 backend has the same split for
//! the same reason and a milder version of the problem — there, events arrive
//! from a socket and re-entrancy is impossible; here it is the normal case.
//!
//! A window is named by its `HWND` as an `isize` rather than as a pointer, so
//! this type stays `Copy`, comparable and printable, and so nothing in the
//! queue is a handle that could be dereferenced after the window died.
//!
//! # The resize storm, and the modal loop behind it
//!
//! Dragging a window's edge produces a `WM_SIZE` per mouse movement — hundreds
//! per second — and, worse, produces them from inside a **modal message loop
//! the system runs itself** between `WM_ENTERSIZEMOVE` and `WM_EXITSIZEMOVE`.
//! `pump` does not return during a drag, so nothing drains the queue until the
//! user lets go. See the [module docs](super) for what that costs and why this
//! backend accepts it.
//!
//! [`enqueue`] is what keeps that from being a memory leak *and* an event
//! flood: a resize supersedes a pending resize for the same window, so a
//! three-second drag arrives as one [`Resized`](crate::ShellEvent::Resized)
//! carrying the size the window actually ended at. Coalescing is safe here in a
//! way it would not be for input, because a resize is a **state**, not an
//! event: the intermediate sizes are not information anyone can act on, since
//! no frame was rendered at any of them.

//! # Input is recorded raw, and interpreted with the shell in hand
//!
//! Every input variant below carries the numbers the message came with and
//! nothing derived from them — a scan code rather than a
//! [`KeyCode`](crcbl_core::KeyCode), a detent count rather than a
//! [`ScrollDelta`](crcbl_core::input::ScrollDelta), a `RAWMOUSE` flag word
//! rather than a delta. Two reasons, and both are about the window procedure
//! rather than about taste:
//!
//! * The interpretations that need *state* — a surrogate pair spanning two
//!   `WM_CHAR`s, an absolute raw report differenced against the previous one —
//!   have nowhere to keep it in a procedure that must not touch the shell.
//! * The interpretations that need a *system call* — the modifier snapshot, the
//!   layout's keysym — are two more calls in a callback the system runs inside
//!   our own `SetWindowPos`.
//!
//! So the procedure copies fields and the shell does the arithmetic, on the
//! pure functions in [`keys`](super::keys) and [`pointer`](super::pointer).
//!
//! Each of them also carries `millis`: `GetMessageTime` for the message being
//! processed, which [`TimeBase`](super::TimeBase) widens and rebases. It is read
//! in the procedure rather than at translation time because by then the value
//! belongs to a *later* message — the whole point of a timestamp is that it is
//! not the moment the queue was drained.

use crcbl_core::input::{ButtonState, PointerButton};

use crate::PhysicalSize;

/// One thing the window procedure saw.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum RawEvent {
    /// `WM_SIZE` with a real size — the client area changed.
    Resized {
        /// `HWND` as an integer.
        hwnd: isize,
        /// The new client area.
        size: PhysicalSize,
    },
    /// `WM_SIZE` with `SIZE_MINIMIZED`.
    ///
    /// Kept apart from [`Resized`](Self::Resized) because a minimized window
    /// reports a client area of 0×0, and publishing that as the swapchain's
    /// extent is a validation error rather than a fact. It is a visibility
    /// change; [`WindowState::visible`](crate::WindowState::visible) is defined
    /// as "mapped **and not minimized**".
    Minimized {
        /// `HWND` as an integer.
        hwnd: isize,
    },
    /// `WM_DPICHANGED` — the window moved to a monitor at another scale, or the
    /// user changed the scale of the one it is on.
    DpiChanged {
        /// `HWND` as an integer.
        hwnd: isize,
        /// The new DPI. 96 is 100%.
        dpi: u32,
    },
    /// `WM_SETFOCUS` or `WM_KILLFOCUS`.
    Focus {
        /// `HWND` as an integer.
        hwnd: isize,
        /// Whether the window now has the keyboard focus.
        focused: bool,
    },
    /// `WM_CLOSE`, which this backend intercepts rather than passing to
    /// `DefWindowProc` — that would destroy the window before anyone could ask
    /// about unsaved work.
    CloseRequested {
        /// `HWND` as an integer.
        hwnd: isize,
    },
    /// `WM_DESTROY` — the window is going away whether we asked or not.
    Destroyed {
        /// `HWND` as an integer.
        hwnd: isize,
    },
    /// `WM_KEYDOWN`, `WM_KEYUP` or either of their `WM_SYS*` twins.
    Key {
        /// `HWND` as an integer.
        hwnd: isize,
        /// The scan code out of `lParam`, with the `E0` prefix folded in — see
        /// [`keys::scancode`](super::keys::scancode).
        scancode: u32,
        /// The virtual key out of `wParam`, which is the only thing that can
        /// name the *symbol* the key produces in the current layout.
        virtual_key: u16,
        /// Down or up.
        state: ButtonState,
        /// Whether `lParam` said the key was already held — an auto-repeat.
        repeat: bool,
        /// `GetMessageTime` milliseconds.
        millis: u32,
    },
    /// `WM_CHAR` — one **UTF-16 code unit** of committed text.
    ///
    /// Not a character: an astral codepoint arrives as two of these carrying a
    /// surrogate pair, which [`keys::Utf16`](super::keys::Utf16) reassembles.
    /// Deliberately separate from [`Key`](Self::Key), which is the relationship
    /// [`TextCommit`](crate::ShellEvent::TextCommit) documents as not being
    /// one-to-one.
    Char {
        /// `HWND` as an integer.
        hwnd: isize,
        /// The code unit out of `wParam`.
        unit: u16,
        /// `GetMessageTime` milliseconds.
        millis: u32,
    },
    /// `WM_MOUSEMOVE`, in client pixels.
    PointerMotion {
        /// `HWND` as an integer.
        hwnd: isize,
        /// Client x, **signed** — a captured pointer is legitimately outside.
        x: i32,
        /// Client y, likewise.
        y: i32,
        /// `GetMessageTime` milliseconds.
        millis: u32,
    },
    /// The pointer arrived or left.
    ///
    /// The arrival half has **no message of its own**: Windows sends
    /// `WM_MOUSELEAVE` and nothing for the entry, so the procedure derives it
    /// from the first motion after a leave. See [`proc`](super::proc).
    PointerFocus {
        /// `HWND` as an integer.
        hwnd: isize,
        /// Whether the pointer is now inside the client area.
        entered: bool,
        /// Client x where it entered. Meaningless on leave, which
        /// [`PointerFocus`](crate::ShellEvent::PointerFocus) reports as `None`.
        x: i32,
        /// Client y where it entered.
        y: i32,
        /// `GetMessageTime` milliseconds.
        millis: u32,
    },
    /// One of the five `WM_*BUTTON*` messages.
    Button {
        /// `HWND` as an integer.
        hwnd: isize,
        /// Which button, already told apart from the `WM_XBUTTON*` pair.
        button: PointerButton,
        /// Down or up.
        state: ButtonState,
        /// Client x.
        x: i32,
        /// Client y.
        y: i32,
        /// `GetMessageTime` milliseconds.
        millis: u32,
    },
    /// `WM_MOUSEWHEEL` or `WM_MOUSEHWHEEL`.
    Wheel {
        /// `HWND` as an integer.
        hwnd: isize,
        /// Whether this is the tilt axis.
        horizontal: bool,
        /// The signed detent count, in `WHEEL_DELTA` units.
        ticks: i16,
        /// Client x — converted from the **screen** coordinates the wheel
        /// messages carry, which is the one place they differ from the rest.
        x: i32,
        /// Client y.
        y: i32,
        /// `GetMessageTime` milliseconds.
        millis: u32,
    },
    /// `WM_INPUT` carrying a `RAWMOUSE` report.
    ///
    /// The coordinates are a delta **or** a position depending on `flags`; see
    /// [`pointer::RawMotion`](super::pointer::RawMotion), which is where that is
    /// resolved and why it cannot be resolved here.
    RawMotion {
        /// `HWND` as an integer — raw input follows the keyboard focus, so this
        /// is whichever of our windows had it.
        hwnd: isize,
        /// `RAWMOUSE::usFlags`.
        flags: u16,
        /// `RAWMOUSE::lLastX`.
        x: i32,
        /// `RAWMOUSE::lLastY`.
        y: i32,
        /// `GetMessageTime` milliseconds.
        millis: u32,
    },
    /// `WM_DROPFILES` — files were dropped on the window.
    ///
    /// **The paths are not here.** A drop carries a `Vec<PathBuf>`, which this
    /// enum cannot hold without giving up `Copy` — see the [module docs](self)
    /// — and the `HDROP` they come out of is live only for the length of the
    /// message, so the procedure reads them and puts them on
    /// [`Shared`](super::proc::Shared)'s own drop queue. This is the marker that
    /// keeps the drop in its place in the stream, so that a drop still follows
    /// the pointer motion which positioned it.
    FilesDropped {
        /// `HWND` as an integer. Also what the payload is matched by, so a
        /// drop on a window that dies before the pump is discarded with it
        /// rather than being handed to the next drop's marker.
        hwnd: isize,
        /// `GetMessageTime` milliseconds.
        millis: u32,
    },
    /// `WM_DISPLAYCHANGE` — a monitor was plugged, unplugged or reconfigured.
    MonitorsChanged,
}

impl RawEvent {
    /// The window this is about, or `None` for a desktop-wide event.
    #[must_use]
    pub const fn hwnd(self) -> Option<isize> {
        match self {
            Self::Resized { hwnd, .. }
            | Self::Minimized { hwnd }
            | Self::DpiChanged { hwnd, .. }
            | Self::Focus { hwnd, .. }
            | Self::CloseRequested { hwnd }
            | Self::Destroyed { hwnd }
            | Self::Key { hwnd, .. }
            | Self::Char { hwnd, .. }
            | Self::PointerMotion { hwnd, .. }
            | Self::PointerFocus { hwnd, .. }
            | Self::Button { hwnd, .. }
            | Self::Wheel { hwnd, .. }
            | Self::RawMotion { hwnd, .. }
            | Self::FilesDropped { hwnd, .. } => Some(hwnd),
            Self::MonitorsChanged => None,
        }
    }
}

/// Appends `event`, letting it supersede a pending one where that is sound.
///
/// Exactly two kinds are collapsed, and neither loses information:
///
/// * A [`Resized`](RawEvent::Resized) replaces a pending resize **of the same
///   window**, in place, so the ordering against other events is unchanged and
///   the size that survives is the latest one. Two windows resizing never
///   collapse into each other.
/// * A [`MonitorsChanged`](RawEvent::MonitorsChanged) is dropped if one is
///   already queued: it carries nothing, the shell re-enumerates from scratch
///   when it sees one, and a single display reconfiguration sends several.
///
/// Everything else is kept, in order. A close request, a focus change and a
/// destruction are each a discrete thing that happened, and dropping the first
/// of two would lose a question the consumer has to answer.
///
/// **No input is collapsed**, deliberately. A resize is a *state* and its
/// intermediate values describe frames nobody rendered; a keystroke, a click and
/// a motion sample are *events*, and the durations between them are what
/// `docs/plan/19-input.md`'s pattern evaluator is a function of. Coalescing two
/// motion samples into their endpoints would erase the path a drag took, and
/// coalescing two key edges would turn a double-tap into a tap. Windows already
/// collapses `WM_MOUSEMOVE` in its own queue, which is the only place that
/// collapse is sound — the system knows which samples the application never saw.
pub fn enqueue(queue: &mut Vec<RawEvent>, event: RawEvent) {
    match event {
        RawEvent::Resized { hwnd, .. } => {
            if let Some(pending) = queue.iter_mut().find(
                |queued| matches!(queued, RawEvent::Resized { hwnd: queued, .. } if *queued == hwnd),
            ) {
                *pending = event;
                return;
            }
        }
        RawEvent::MonitorsChanged if queue.contains(&RawEvent::MonitorsChanged) => return,
        _ => {}
    }
    queue.push(event);
}

#[cfg(test)]
mod tests {
    use super::*;

    const A: isize = 0x1234;
    const B: isize = 0x5678;

    fn resize(hwnd: isize, width: u32) -> RawEvent {
        RawEvent::Resized {
            hwnd,
            size: PhysicalSize::new(width, 720),
        }
    }

    #[test]
    fn a_drag_resize_storm_collapses_to_the_size_the_window_ended_at() {
        // The modal-loop case: nothing drains the queue while the user drags,
        // so without this a three-second drag delivers a few hundred `Resized`
        // events in one pump, every one of them describing a size no frame was
        // ever rendered at.
        let mut queue = Vec::new();
        for width in [1280, 1281, 1290, 1400, 1401] {
            enqueue(&mut queue, resize(A, width));
        }
        assert_eq!(queue.len(), 1, "{queue:?}");
        assert_eq!(
            queue[0],
            RawEvent::Resized {
                hwnd: A,
                size: PhysicalSize::new(1401, 720)
            }
        );
    }

    #[test]
    fn two_windows_resizing_do_not_collapse_into_each_other() {
        let mut queue = Vec::new();
        enqueue(&mut queue, resize(A, 800));
        enqueue(&mut queue, resize(B, 1024));
        enqueue(&mut queue, resize(A, 900));
        assert_eq!(queue.len(), 2);
        // In place, so B is still second — the collapse must not reorder.
        assert_eq!(queue[0], resize(A, 900));
        assert_eq!(queue[1], resize(B, 1024));
    }

    #[test]
    fn everything_that_is_not_a_resize_keeps_every_occurrence_in_order() {
        // A close request asked twice is two questions, and dropping one leaves
        // the consumer's reply addressed to a request that no longer exists.
        let mut queue = Vec::new();
        let events = [
            RawEvent::Focus {
                hwnd: A,
                focused: true,
            },
            RawEvent::CloseRequested { hwnd: A },
            RawEvent::Focus {
                hwnd: A,
                focused: false,
            },
            RawEvent::CloseRequested { hwnd: A },
            RawEvent::DpiChanged { hwnd: A, dpi: 144 },
            RawEvent::DpiChanged { hwnd: A, dpi: 96 },
            // Two drops in one pump are two drops, and collapsing them would
            // leave the second one's paths on the shared queue with no marker
            // to claim them.
            RawEvent::FilesDropped {
                hwnd: A,
                millis: 1_000,
            },
            RawEvent::FilesDropped {
                hwnd: A,
                millis: 1_200,
            },
            RawEvent::Minimized { hwnd: A },
            RawEvent::Destroyed { hwnd: A },
        ];
        for event in events {
            enqueue(&mut queue, event);
        }
        assert_eq!(queue, events);
    }

    #[test]
    fn one_display_reconfiguration_is_one_monitors_changed() {
        // Windows sends `WM_DISPLAYCHANGE` to every top-level window, so a
        // two-window shell sees it twice for one cable.
        let mut queue = Vec::new();
        enqueue(&mut queue, RawEvent::MonitorsChanged);
        enqueue(&mut queue, resize(A, 1024));
        enqueue(&mut queue, RawEvent::MonitorsChanged);
        assert_eq!(queue, [RawEvent::MonitorsChanged, resize(A, 1024)]);
    }

    #[test]
    fn a_burst_of_input_keeps_every_sample_in_order() {
        // The counterpart to the resize collapse, and the reason `enqueue` is
        // not "coalesce anything repeated": a drag path, a double-click and an
        // auto-repeat run are all sequences whose *members* are the
        // information. Collapsing them turns a double-tap into a tap.
        let mut queue = Vec::new();
        let samples = [
            RawEvent::PointerMotion {
                hwnd: A,
                x: 10,
                y: 10,
                millis: 1_000,
            },
            RawEvent::PointerMotion {
                hwnd: A,
                x: 11,
                y: 12,
                millis: 1_004,
            },
            RawEvent::Button {
                hwnd: A,
                button: PointerButton::Left,
                state: ButtonState::Pressed,
                x: 11,
                y: 12,
                millis: 1_008,
            },
            RawEvent::Button {
                hwnd: A,
                button: PointerButton::Left,
                state: ButtonState::Released,
                x: 11,
                y: 12,
                millis: 1_060,
            },
            RawEvent::Button {
                hwnd: A,
                button: PointerButton::Left,
                state: ButtonState::Pressed,
                x: 11,
                y: 12,
                millis: 1_140,
            },
            RawEvent::Key {
                hwnd: A,
                scancode: 0x0011,
                virtual_key: 0x57,
                state: ButtonState::Pressed,
                repeat: false,
                millis: 1_200,
            },
            RawEvent::Key {
                hwnd: A,
                scancode: 0x0011,
                virtual_key: 0x57,
                state: ButtonState::Pressed,
                repeat: true,
                millis: 1_500,
            },
            RawEvent::RawMotion {
                hwnd: A,
                flags: 0,
                x: 3,
                y: -2,
                millis: 1_501,
            },
            RawEvent::RawMotion {
                hwnd: A,
                flags: 0,
                x: 4,
                y: -1,
                millis: 1_502,
            },
            RawEvent::Char {
                hwnd: A,
                unit: 0xD83C,
                millis: 1_600,
            },
            RawEvent::Char {
                hwnd: A,
                unit: 0xDFAE,
                millis: 1_600,
            },
        ];
        for sample in samples {
            enqueue(&mut queue, sample);
        }
        assert_eq!(queue, samples);
    }

    #[test]
    fn a_resize_still_collapses_past_the_input_that_arrived_with_it() {
        // The collapse is in-place, so it must not reorder a resize past the
        // input events queued after it — a click delivered before the resize
        // that produced the window it was aimed at is a click at the wrong
        // coordinates.
        let mut queue = Vec::new();
        let click = RawEvent::Button {
            hwnd: A,
            button: PointerButton::Right,
            state: ButtonState::Pressed,
            x: 4,
            y: 4,
            millis: 9,
        };
        enqueue(&mut queue, resize(A, 800));
        enqueue(&mut queue, click);
        enqueue(&mut queue, resize(A, 900));
        assert_eq!(queue, [resize(A, 900), click]);
    }

    #[test]
    fn every_windowed_event_names_its_window_and_the_desktop_one_does_not() {
        // The routing predicate: an event with no `HWND` is not a stale-window
        // problem, it is a shell-wide fact.
        assert_eq!(resize(A, 640).hwnd(), Some(A));
        assert_eq!(RawEvent::Minimized { hwnd: B }.hwnd(), Some(B));
        assert_eq!(RawEvent::DpiChanged { hwnd: A, dpi: 96 }.hwnd(), Some(A));
        assert_eq!(
            RawEvent::Focus {
                hwnd: A,
                focused: true
            }
            .hwnd(),
            Some(A)
        );
        assert_eq!(RawEvent::CloseRequested { hwnd: A }.hwnd(), Some(A));
        assert_eq!(RawEvent::Destroyed { hwnd: A }.hwnd(), Some(A));
        assert_eq!(
            RawEvent::Key {
                hwnd: B,
                scancode: 1,
                virtual_key: 0x1B,
                state: ButtonState::Released,
                repeat: false,
                millis: 0,
            }
            .hwnd(),
            Some(B)
        );
        assert_eq!(
            RawEvent::Char {
                hwnd: B,
                unit: 0x61,
                millis: 0
            }
            .hwnd(),
            Some(B)
        );
        assert_eq!(
            RawEvent::PointerMotion {
                hwnd: B,
                x: 0,
                y: 0,
                millis: 0
            }
            .hwnd(),
            Some(B)
        );
        assert_eq!(
            RawEvent::PointerFocus {
                hwnd: B,
                entered: false,
                x: 0,
                y: 0,
                millis: 0,
            }
            .hwnd(),
            Some(B)
        );
        assert_eq!(
            RawEvent::Button {
                hwnd: B,
                button: PointerButton::Middle,
                state: ButtonState::Pressed,
                x: 0,
                y: 0,
                millis: 0,
            }
            .hwnd(),
            Some(B)
        );
        assert_eq!(
            RawEvent::Wheel {
                hwnd: B,
                horizontal: false,
                ticks: 120,
                x: 0,
                y: 0,
                millis: 0,
            }
            .hwnd(),
            Some(B)
        );
        assert_eq!(
            RawEvent::RawMotion {
                hwnd: B,
                flags: 0,
                x: 1,
                y: 1,
                millis: 0
            }
            .hwnd(),
            Some(B)
        );
        assert_eq!(
            RawEvent::FilesDropped { hwnd: B, millis: 0 }.hwnd(),
            Some(B),
            "a drop's marker has to name its window, because that is what the \
             payload is matched by"
        );
        assert_eq!(RawEvent::MonitorsChanged.hwnd(), None);
    }
}
