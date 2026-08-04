//! What the window delegate records, and how a storm of it is collapsed.
//!
//! # Why the delegate records instead of acting
//!
//! A delegate method is called **by AppKit, re-entrantly, while the shell is
//! somewhere else**. `[NSApp sendEvent:]` dispatches a resize into
//! `windowDidResize:` before it returns, and `sendEvent:` is called from inside
//! this crate's own [`pump`](crate::Shell::pump) — so the callback runs *inside*
//! that method, with `&mut self` already borrowed and the shell's invariants
//! halfway through an update. `setFrame:display:` and `setStyleMask:` do the
//! same thing from inside [`set_mode`](crate::Shell::set_mode).
//!
//! That is the same shape the Win32 backend's window procedure has, and the
//! answer is the same: the callback pushes one of these and returns;
//! [`pump`](crate::Shell::pump) resolves the window, reads the new geometry and
//! produces the [`ShellEvent`](crate::ShellEvent).
//!
//! A window is named by its `NSWindow *` as a `usize` rather than as a pointer,
//! so this type stays `Copy`, comparable and printable, and so nothing in the
//! queue is a handle that could be dereferenced after the window died.
//!
//! # These variants carry no geometry, and Win32's do
//!
//! [`RawEvent::Resized`] says *that* a window's frame changed and not what it
//! changed to, which is the one place this backend is simpler than the Windows
//! one rather than harder. `WM_SIZE` arrives with the new size in its `lParam`
//! and the Win32 procedure copies it out because that is the only moment it
//! exists; AppKit's geometry is **state**, readable from `[window frame]` at any
//! time, and the notification is only a hint that reading it again is worth
//! doing. So the callback copies nothing, `translate` reads once, and there is
//! no way for a recorded number to disagree with the window it describes.
//!
//! It is also what makes the coalescing below trivially sound: two
//! [`Resized`](RawEvent::Resized) markers for one window are the same marker,
//! because neither of them carries a value that could differ.
//!
//! # Input is recorded raw, and interpreted with the shell in hand
//!
//! Every input variant carries the numbers the `NSEvent` came with and almost
//! nothing derived from them — a `kVK_*` code rather than a
//! [`KeyCode`](crcbl_core::KeyCode), a modifier flags word rather than
//! [`Modifiers`](crcbl_core::input::Modifiers), a scrolling delta and a
//! precise-or-not flag rather than a
//! [`ScrollDelta`](crcbl_core::input::ScrollDelta). The reason is the Win32
//! backend's: a responder method is called re-entrantly from inside this crate's
//! own `pump`, so it must not touch the shell, and the interpretations that need
//! the window's scale or its view's height have nowhere to read them from.
//!
//! **The one exception is the character**, and it is forced rather than chosen:
//! an `NSEvent`'s `charactersIgnoringModifiers` is a string owned by an event
//! that is gone by the time the pump gets back to it. Win32 can ask
//! `MapVirtualKeyW` about the layout at any later moment; here the only moment
//! is inside the callback, so the callback reads the first character and records
//! it.
//!
//! # `TextCommit` is a marker, and its string is on the side
//!
//! An input method commits *a string*, not a character — that is the whole
//! reason [`TextCommit`](crate::ShellEvent::TextCommit) exists as something
//! other than a key event — and a `String` cannot live in a [`Copy`] enum. So
//! [`TextCommitted`](RawEvent::TextCommitted) is a marker that keeps the commit
//! in its place in the stream and the text itself is queued on
//! [`Shared`](super::app::Shared), which is the same arrangement the Win32
//! backend uses for a file drop's paths and for the same reason.
//!
//! # This enum is `PartialEq` and not `Eq`, unlike Win32's
//!
//! Positions, deltas and timestamps are all `f64`, because that is what AppKit
//! reports them as — a fractional point coordinate is normal on a Retina
//! display, and the timestamp is a `double` of seconds. [`enqueue`] compares
//! only the three variants that carry no value at all, so nothing here depends
//! on float equality; the derive is what the type is, not what the queue uses.

use crcbl_core::input::ButtonState;

use super::ffi::{NSPoint, NSUInteger};

/// One thing the window delegate or the view saw.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum RawEvent {
    /// `windowDidResize:` — the frame changed; read it again.
    Resized {
        /// `NSWindow *` as an integer.
        window: usize,
    },
    /// `windowDidChangeBackingProperties:` or `windowDidChangeScreen:` — the
    /// window moved to a display with a different `backingScaleFactor`, or the
    /// one it is on changed.
    ///
    /// Kept apart from [`Resized`](Self::Resized) because the seam emits
    /// [`ScaleFactorChanged`](crate::ShellEvent::ScaleFactorChanged) **before**
    /// the [`Resized`](crate::ShellEvent::Resized) it comes with, and a consumer
    /// that recreates a swapchain on the resize must already know the scale it
    /// is recreating at.
    BackingChanged {
        /// `NSWindow *` as an integer.
        window: usize,
    },
    /// `windowDidBecomeKey:` or `windowDidResignKey:`.
    ///
    /// *Key*, not *main*: key is where the keyboard goes, main is what the menu
    /// bar acts on, and a window can be one without the other. The seam's
    /// [`Focus`](crate::ShellEvent::Focus) is documented as keyboard focus.
    Focus {
        /// `NSWindow *` as an integer.
        window: usize,
        /// Whether the window now has the keyboard.
        focused: bool,
    },
    /// `windowShouldClose:`, which this backend answers `NO` to — the seam
    /// intercepts the close so that
    /// [`reply_close_request`](crate::Shell::reply_close_request) can ask about
    /// unsaved work first.
    CloseRequested {
        /// `NSWindow *` as an integer.
        window: usize,
    },
    /// `windowWillClose:` — the window is going away whether we asked or not.
    Closed {
        /// `NSWindow *` as an integer.
        window: usize,
    },
    /// `NSApplicationDidChangeScreenParametersNotification` — a display was
    /// attached, detached or reconfigured.
    ScreensChanged,

    /// `keyDown:` or `keyUp:` on the content view.
    ///
    /// **Never a modifier key**: those arrive as
    /// [`FlagsChanged`](Self::FlagsChanged), because macOS does not deliver a
    /// key event for one at all.
    Key {
        /// `NSWindow *` as an integer.
        window: usize,
        /// `-[NSEvent keyCode]`, which is a `kVK_*` value — see
        /// [`keys::key_code`](super::keys::key_code).
        keycode: u16,
        /// The first character of `charactersIgnoringModifiers`, read in the
        /// callback because the event does not outlive it. `None` for a key that
        /// produces no character in this layout.
        character: Option<char>,
        /// `-[NSEvent modifierFlags]`.
        flags: NSUInteger,
        /// Down or up.
        state: ButtonState,
        /// `-[NSEvent isARepeat]`.
        repeat: bool,
        /// `-[NSEvent timestamp]`: seconds since the system booted.
        seconds: f64,
    },

    /// `flagsChanged:` — a modifier key moved, and only the flags say which way.
    ///
    /// The `keycode` names the physical key and the flags carry one
    /// device-dependent bit per modifier;
    /// [`keys::modifier_is_down`](super::keys::modifier_is_down) is what turns
    /// the pair into an edge.
    FlagsChanged {
        /// `NSWindow *` as an integer.
        window: usize,
        /// `-[NSEvent keyCode]`.
        keycode: u16,
        /// `-[NSEvent modifierFlags]`, device-dependent bits included.
        flags: NSUInteger,
        /// `-[NSEvent timestamp]`.
        seconds: f64,
    },

    /// The input method committed text. **The string is on
    /// [`Shared`](super::app::Shared)**; see the module docs.
    TextCommitted {
        /// `NSWindow *` as an integer. Also what the payload is matched by, so a
        /// commit for a window that dies before the pump is discarded with it
        /// rather than being handed to the next commit's marker.
        window: usize,
        /// The timestamp of the key event being interpreted — not the moment
        /// `insertText:` ran, which is inside it.
        seconds: f64,
    },

    /// `mouseMoved:` or one of the three `*MouseDragged:` methods.
    PointerMotion {
        /// `NSWindow *` as an integer.
        window: usize,
        /// `-[NSEvent locationInWindow]`: AppKit's window space, Y **up**.
        location: NSPoint,
        /// `-[NSEvent deltaX]`, which is Quartz's space and needs no flip.
        dx: f64,
        /// `-[NSEvent deltaY]`, likewise — Y **down**, unlike `location` beside
        /// it.
        dy: f64,
        /// `-[NSEvent timestamp]`.
        seconds: f64,
    },

    /// `mouseEntered:` or `mouseExited:` from the view's `NSTrackingArea`.
    PointerFocus {
        /// `NSWindow *` as an integer.
        window: usize,
        /// Whether the pointer is now inside the view.
        entered: bool,
        /// Where it entered. Meaningless on leave, which
        /// [`PointerFocus`](crate::ShellEvent::PointerFocus) reports as `None`.
        location: NSPoint,
        /// `-[NSEvent timestamp]`.
        seconds: f64,
    },

    /// One of the six `*Mouse{Down,Up}:` methods.
    Button {
        /// `NSWindow *` as an integer.
        window: usize,
        /// `-[NSEvent buttonNumber]`, which goes past five — see
        /// [`pointer::button`](super::pointer::button).
        number: isize,
        /// Down or up.
        state: ButtonState,
        /// `-[NSEvent locationInWindow]`.
        location: NSPoint,
        /// `-[NSEvent modifierFlags]`.
        flags: NSUInteger,
        /// `-[NSEvent timestamp]`.
        seconds: f64,
    },

    /// `scrollWheel:`.
    Scroll {
        /// `NSWindow *` as an integer.
        window: usize,
        /// `-[NSEvent scrollingDeltaX]`.
        dx: f64,
        /// `-[NSEvent scrollingDeltaY]`.
        dy: f64,
        /// `-[NSEvent hasPreciseScrollingDeltas]`: pixels when set, lines when
        /// clear. The device answers, not the backend.
        precise: bool,
        /// `-[NSEvent locationInWindow]`.
        location: NSPoint,
        /// `-[NSEvent modifierFlags]`.
        flags: NSUInteger,
        /// `-[NSEvent timestamp]`.
        seconds: f64,
    },
}

impl RawEvent {
    /// The window this is about, or `None` for a desktop-wide event.
    #[must_use]
    pub const fn window(self) -> Option<usize> {
        match self {
            Self::Resized { window }
            | Self::BackingChanged { window }
            | Self::Focus { window, .. }
            | Self::CloseRequested { window }
            | Self::Closed { window }
            | Self::Key { window, .. }
            | Self::FlagsChanged { window, .. }
            | Self::TextCommitted { window, .. }
            | Self::PointerMotion { window, .. }
            | Self::PointerFocus { window, .. }
            | Self::Button { window, .. }
            | Self::Scroll { window, .. } => Some(window),
            Self::ScreensChanged => None,
        }
    }
}

/// Appends `event`, dropping it where an identical one is already pending.
///
/// Exactly three kinds are collapsed, and none loses information, because none
/// of them carries a value:
///
/// * A [`Resized`](RawEvent::Resized) for a window that already has one queued.
///   A live resize drag delivers `windowDidResize:` per frame of the drag, and
///   every one of them says the same thing — "read the frame again" — which
///   `translate` will do once, at the end, and get the size the window actually
///   ended at.
/// * A [`BackingChanged`](RawEvent::BackingChanged), for the same reason. Moving
///   a window between two displays of different scales delivers both
///   `windowDidChangeScreen:` and `windowDidChangeBackingProperties:`, which are
///   one fact.
/// * A [`ScreensChanged`](RawEvent::ScreensChanged): it carries nothing, the
///   shell re-enumerates from scratch when it sees one, and one display
///   reconfiguration posts several notifications.
///
/// Everything else is kept, in order. A close request, a focus change and a
/// closure are each a discrete thing that happened, and dropping the first of
/// two would lose a question the consumer has to answer.
///
/// **Dropping rather than replacing in place**, unlike the Win32 backend's
/// equivalent: there is nothing to replace. Win32 has to overwrite the pending
/// marker because the newer one carries a newer size; here the two are equal, so
/// keeping the first preserves its position in the stream — which is what makes
/// a focus change that arrived between two resizes still land between them.
///
/// **No input is collapsed**, deliberately, and the three arms below are what
/// makes that structural rather than a promise: a resize is a *state* whose
/// intermediate values describe frames nobody rendered, while a keystroke, a
/// click and a motion sample are *events* whose durations between them are what
/// `docs/plan/19-input.md`'s pattern evaluator is a function of. Coalescing two
/// motion samples into their endpoints erases the path a drag took, and
/// coalescing two key edges turns a double-tap into a tap.
pub fn enqueue(queue: &mut Vec<RawEvent>, event: RawEvent) {
    match event {
        RawEvent::Resized { .. } | RawEvent::BackingChanged { .. } | RawEvent::ScreensChanged
            if queue.contains(&event) =>
        {
            return;
        }
        _ => {}
    }
    queue.push(event);
}

#[cfg(test)]
mod tests {
    use super::*;

    const A: usize = 0x1234;
    const B: usize = 0x5678;

    #[test]
    fn a_live_resize_drag_collapses_to_one_marker() {
        // AppKit delivers `windowDidResize:` per frame of a drag. Every one says
        // "read the frame again", so a three-second drag is one read — of the
        // size the window ended at, which is the only size any frame will ever
        // be rendered at.
        let mut queue = Vec::new();
        for _ in 0..200 {
            enqueue(&mut queue, RawEvent::Resized { window: A });
        }
        assert_eq!(queue, [RawEvent::Resized { window: A }]);
    }

    #[test]
    fn two_windows_resizing_do_not_collapse_into_each_other() {
        let mut queue = Vec::new();
        enqueue(&mut queue, RawEvent::Resized { window: A });
        enqueue(&mut queue, RawEvent::Resized { window: B });
        enqueue(&mut queue, RawEvent::Resized { window: A });
        assert_eq!(
            queue,
            [
                RawEvent::Resized { window: A },
                RawEvent::Resized { window: B }
            ]
        );
    }

    #[test]
    fn the_collapse_never_reorders_what_it_kept() {
        // The property that makes "drop the newer" correct where Win32 has to
        // "replace the older": a focus change that arrived between two resizes
        // still lands between them, because the surviving resize is the first
        // one and it never moved.
        let mut queue = Vec::new();
        let focus = RawEvent::Focus {
            window: A,
            focused: true,
        };
        enqueue(&mut queue, RawEvent::Resized { window: A });
        enqueue(&mut queue, focus);
        enqueue(&mut queue, RawEvent::Resized { window: A });
        assert_eq!(queue, [RawEvent::Resized { window: A }, focus]);
    }

    #[test]
    fn moving_between_two_displays_is_one_backing_change() {
        // `windowDidChangeScreen:` and `windowDidChangeBackingProperties:` both
        // arrive, and they are one fact.
        let mut queue = Vec::new();
        enqueue(&mut queue, RawEvent::BackingChanged { window: A });
        enqueue(&mut queue, RawEvent::BackingChanged { window: A });
        enqueue(&mut queue, RawEvent::BackingChanged { window: B });
        assert_eq!(
            queue,
            [
                RawEvent::BackingChanged { window: A },
                RawEvent::BackingChanged { window: B }
            ]
        );
    }

    #[test]
    fn one_display_reconfiguration_is_one_screens_changed() {
        let mut queue = Vec::new();
        enqueue(&mut queue, RawEvent::ScreensChanged);
        enqueue(&mut queue, RawEvent::Resized { window: A });
        enqueue(&mut queue, RawEvent::ScreensChanged);
        assert_eq!(
            queue,
            [RawEvent::ScreensChanged, RawEvent::Resized { window: A }]
        );
    }

    #[test]
    fn every_occurrence_of_a_discrete_event_is_kept_in_order() {
        // A close request asked twice is two questions, and dropping one leaves
        // the consumer's reply addressed to a request that no longer exists.
        let mut queue = Vec::new();
        let events = [
            RawEvent::Focus {
                window: A,
                focused: true,
            },
            RawEvent::CloseRequested { window: A },
            RawEvent::Focus {
                window: A,
                focused: false,
            },
            RawEvent::Focus {
                window: A,
                focused: true,
            },
            RawEvent::CloseRequested { window: A },
            RawEvent::Closed { window: A },
        ];
        for event in events {
            enqueue(&mut queue, event);
        }
        assert_eq!(queue, events);
    }

    /// A motion sample, for the burst below.
    fn motion(window: usize, x: f64, seconds: f64) -> RawEvent {
        RawEvent::PointerMotion {
            window,
            location: NSPoint { x, y: 100.0 },
            dx: 1.0,
            dy: -1.0,
            seconds,
        }
    }

    #[test]
    fn a_burst_of_input_keeps_every_sample_in_order() {
        // The counterpart to the resize collapse, and the reason `enqueue` names
        // three variants rather than "anything repeated": a drag path, a
        // double-click and an auto-repeat run are all sequences whose *members*
        // are the information.
        let mut queue = Vec::new();
        let samples = [
            motion(A, 10.0, 1.000),
            motion(A, 11.0, 1.004),
            RawEvent::Button {
                window: A,
                number: 0,
                state: ButtonState::Pressed,
                location: NSPoint { x: 11.0, y: 100.0 },
                flags: 0,
                seconds: 1.008,
            },
            RawEvent::Button {
                window: A,
                number: 0,
                state: ButtonState::Released,
                location: NSPoint { x: 11.0, y: 100.0 },
                flags: 0,
                seconds: 1.060,
            },
            RawEvent::Button {
                window: A,
                number: 0,
                state: ButtonState::Pressed,
                location: NSPoint { x: 11.0, y: 100.0 },
                flags: 0,
                seconds: 1.140,
            },
            RawEvent::Key {
                window: A,
                keycode: 0x0D,
                character: Some('w'),
                flags: 0,
                state: ButtonState::Pressed,
                repeat: false,
                seconds: 1.200,
            },
            RawEvent::Key {
                window: A,
                keycode: 0x0D,
                character: Some('w'),
                flags: 0,
                state: ButtonState::Pressed,
                repeat: true,
                seconds: 1.500,
            },
            RawEvent::Scroll {
                window: A,
                dx: 0.0,
                dy: -3.0,
                precise: true,
                location: NSPoint { x: 11.0, y: 100.0 },
                flags: 0,
                seconds: 1.600,
            },
            RawEvent::TextCommitted {
                window: A,
                seconds: 1.200,
            },
        ];
        for sample in samples {
            enqueue(&mut queue, sample);
        }
        assert_eq!(queue, samples);
    }

    #[test]
    fn two_identical_motion_samples_are_two_samples() {
        // The one case that would be collapsed by a `contains` check applied to
        // everything: a pointer that came back to where it was is not the same
        // event twice, and a commit of the same string twice is two commits.
        let mut queue = Vec::new();
        let sample = motion(A, 10.0, 1.0);
        let commit = RawEvent::TextCommitted {
            window: A,
            seconds: 1.0,
        };
        enqueue(&mut queue, sample);
        enqueue(&mut queue, sample);
        enqueue(&mut queue, commit);
        enqueue(&mut queue, commit);
        assert_eq!(queue, [sample, sample, commit, commit]);
    }

    #[test]
    fn every_windowed_event_names_its_window_and_the_desktop_one_does_not() {
        // The routing predicate: an event with no window is not a stale-handle
        // problem, it is a shell-wide fact.
        assert_eq!(RawEvent::Resized { window: A }.window(), Some(A));
        assert_eq!(motion(B, 0.0, 0.0).window(), Some(B));
        assert_eq!(
            RawEvent::Key {
                window: B,
                keycode: 0x35,
                character: None,
                flags: 0,
                state: ButtonState::Released,
                repeat: false,
                seconds: 0.0,
            }
            .window(),
            Some(B)
        );
        assert_eq!(
            RawEvent::FlagsChanged {
                window: B,
                keycode: 0x38,
                flags: 0,
                seconds: 0.0,
            }
            .window(),
            Some(B)
        );
        assert_eq!(
            RawEvent::TextCommitted {
                window: B,
                seconds: 0.0
            }
            .window(),
            Some(B),
            "a commit's marker has to name its window, because that is what the \
             text is matched by"
        );
        assert_eq!(
            RawEvent::PointerFocus {
                window: B,
                entered: false,
                location: NSPoint::default(),
                seconds: 0.0,
            }
            .window(),
            Some(B)
        );
        assert_eq!(
            RawEvent::Button {
                window: B,
                number: 2,
                state: ButtonState::Pressed,
                location: NSPoint::default(),
                flags: 0,
                seconds: 0.0,
            }
            .window(),
            Some(B)
        );
        assert_eq!(
            RawEvent::Scroll {
                window: B,
                dx: 0.0,
                dy: 1.0,
                precise: false,
                location: NSPoint::default(),
                flags: 0,
                seconds: 0.0,
            }
            .window(),
            Some(B)
        );
        assert_eq!(RawEvent::BackingChanged { window: B }.window(), Some(B));
        assert_eq!(
            RawEvent::Focus {
                window: A,
                focused: false
            }
            .window(),
            Some(A)
        );
        assert_eq!(RawEvent::CloseRequested { window: B }.window(), Some(B));
        assert_eq!(RawEvent::Closed { window: A }.window(), Some(A));
        assert_eq!(RawEvent::ScreensChanged.window(), None);
    }
}
