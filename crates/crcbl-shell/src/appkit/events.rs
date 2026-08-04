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

/// One thing the window delegate saw.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
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
            | Self::Closed { window } => Some(window),
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

    #[test]
    fn every_windowed_event_names_its_window_and_the_desktop_one_does_not() {
        // The routing predicate: an event with no window is not a stale-handle
        // problem, it is a shell-wide fact.
        assert_eq!(RawEvent::Resized { window: A }.window(), Some(A));
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
