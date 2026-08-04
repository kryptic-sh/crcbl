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
            | Self::Destroyed { hwnd } => Some(hwnd),
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
        assert_eq!(RawEvent::MonitorsChanged.hwnd(), None);
    }
}
