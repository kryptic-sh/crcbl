//! The window procedure, and the state it reaches through `GWLP_USERDATA`.
//!
//! # How the procedure finds this shell
//!
//! A `WNDPROC` is a bare `extern "system"` function: the system calls it with
//! an `HWND` and nothing else, so a backend with per-window state has to attach
//! that state to the window. The standard mechanism, and the one used here, is
//! `GWLP_USERDATA` — a pointer-sized slot on every window — bootstrapped in
//! `WM_NCCREATE`, which carries the `lpParam` handed to `CreateWindowExW` and
//! is the **first** message a window ever receives that can.
//!
//! What the slot holds is one [`Shared`] for the whole shell, not one record
//! per window. Two reasons:
//!
//! * The procedure needs the event queue, which is shell-wide, far more often
//!   than it needs anything per-window.
//! * A per-window `Box` would have to be freed exactly once, from
//!   `WM_NCDESTROY`, on a path that also runs when `CreateWindowExW` fails
//!   halfway. One allocation with the shell's lifetime has no such path.
//!
//! The pointer is cleared in `WM_NCDESTROY`, which is the last message a window
//! receives. After that the `HWND` value may be handed to a *new* window by the
//! system, and a stale pointer left in the slot would be read by whatever ran
//! next.
//!
//! # What the procedure may and may not do
//!
//! It runs **inside** whatever the shell was doing — see [`events`](super::events)
//! — so it must not touch the shell. It may:
//!
//! * push a [`RawEvent`], which borrows a `RefCell` for the length of one
//!   `push`;
//! * read a window's cached [`Limits`], for the two messages the system
//!   demands an answer to *now*;
//! * answer those two messages by writing into the structure the system lent
//!   us.
//!
//! The `RefCell` borrows are the part that has to stay short. A shell method
//! holding one across a `SetWindowPos` would panic when the procedure it
//! synchronously invokes tried to take it — which is a real hazard rather than
//! a theoretical one, because every mode change does exactly that call. It is
//! why [`Shared`]'s methods each take and release a borrow rather than handing
//! one out.
//!
//! # The two messages that cannot wait for the pump
//!
//! Everything else this backend handles is *recorded* and answered later.
//! `WM_GETMINMAXINFO` and `WM_SIZING` cannot be: the system is asking a
//! question mid-drag and will use whatever is in the structure when the
//! procedure returns. Both are answered from numbers the shell precomputed —
//! see [`geometry`](super::geometry) — so the procedure does arithmetic and
//! nothing else. That is what makes the aspect lock
//! ([`ShellCaps::ASPECT_HINT_HONORED`](crate::ShellCaps::ASPECT_HINT_HONORED))
//! and the size constraints work *during* a drag rather than after it.

use core::cell::RefCell;

use crate::AspectRatio;

use super::events::{RawEvent, enqueue};
use super::ffi::Handle;
use super::geometry::{Frame, Track};

// The procedure itself is the only part of this module that calls into the
// system, so everything above it — the shared queue, the cached limits and the
// word arithmetic — compiles and is tested on every host. See
// [`geometry`](super::geometry) for why that matters.
#[cfg(target_os = "windows")]
use super::ffi::{self, CreateStructW, Lparam, Lresult, MinMaxInfo, Rect, Wparam, msg, value};
#[cfg(target_os = "windows")]
use super::geometry;
#[cfg(target_os = "windows")]
use crate::PhysicalSize;
#[cfg(target_os = "windows")]
use core::ptr;

/// An `HWND` as the integer the queue and the window pool match on.
///
/// Never converted back: the shell keeps the real handle, and a pointer
/// reconstructed from a queue entry could name a window that has since been
/// destroyed.
pub(super) fn key(hwnd: Handle) -> isize {
    hwnd as isize
}

/// The numbers the window procedure answers a synchronous message from.
///
/// Precomputed by the shell whenever the constraints, the style or the DPI
/// change, because working them out needs `AdjustWindowRectExForDpi` and the
/// procedure must not call into the system beyond answering.
#[derive(Clone, Copy, Debug)]
pub(super) struct Limits {
    /// Which window.
    pub hwnd: isize,
    /// Interactive resize limits, in window pixels.
    pub track: Track,
    /// The aspect lock, if one was asked for.
    pub aspect: Option<AspectRatio>,
    /// The window's non-client padding at its current style and DPI.
    pub frame: Frame,
}

/// What the shell and its window procedure share.
///
/// Owned by the shell as an `Rc`, and reached by the procedure as a raw pointer
/// out of `GWLP_USERDATA`. The soundness argument is one sentence and the shell
/// is what has to keep it true: **every window is destroyed before this is
/// dropped**, so no procedure can run after the allocation goes away. See
/// [`Win32Shell::drop`](super::Win32Shell).
#[derive(Debug, Default)]
pub(super) struct Shared {
    events: RefCell<Vec<RawEvent>>,
    limits: RefCell<Vec<Limits>>,
}

impl Shared {
    /// Records something the window procedure saw.
    fn push(&self, event: RawEvent) {
        enqueue(&mut self.events.borrow_mut(), event);
    }

    /// Takes everything recorded since the last call.
    ///
    /// Takes rather than drains, so the borrow ends before the caller starts
    /// resolving handles — which may call back into the system and therefore
    /// into the procedure.
    pub(super) fn take_events(&self) -> Vec<RawEvent> {
        core::mem::take(&mut self.events.borrow_mut())
    }

    /// Publishes the numbers the procedure answers `WM_GETMINMAXINFO` and
    /// `WM_SIZING` from, replacing any earlier ones for the same window.
    pub(super) fn set_limits(&self, limits: Limits) {
        let mut table = self.limits.borrow_mut();
        match table.iter_mut().find(|known| known.hwnd == limits.hwnd) {
            Some(known) => *known = limits,
            None => table.push(limits),
        }
    }

    /// Drops a window's limits, on the way out.
    pub(super) fn forget(&self, hwnd: isize) {
        self.limits.borrow_mut().retain(|known| known.hwnd != hwnd);
    }

    /// A window's limits, by value so no borrow outlives the lookup.
    fn limits_for(&self, hwnd: isize) -> Option<Limits> {
        self.limits
            .borrow()
            .iter()
            .find(|known| known.hwnd == hwnd)
            .copied()
    }
}

/// `LOWORD`.
const fn low_word(value: usize) -> u32 {
    (value & 0xFFFF) as u32
}

/// `HIWORD`.
const fn high_word(value: usize) -> u32 {
    ((value >> 16) & 0xFFFF) as u32
}

/// The window procedure for every window this backend creates.
///
/// # Safety
///
/// Called only by the system, for a window created by
/// [`window`](super::window)'s class. The `w_param`/`l_param` interpretation of
/// each message is the one `winuser.h` documents, and each is asserted in the
/// `SAFETY` comment where it is relied on.
#[cfg(target_os = "windows")]
pub(super) unsafe extern "system" fn window_proc(
    hwnd: Handle,
    message: u32,
    w_param: Wparam,
    l_param: Lparam,
) -> Lresult {
    // SAFETY: the default handler accepts every message for a live window, and
    // `hwnd` is live for the whole of this call — the system owns it and is
    // what called us.
    let default = || unsafe { ffi::DefWindowProcW(hwnd, message, w_param, l_param) };

    if message == msg::NC_CREATE {
        // SAFETY: for `WM_NCCREATE` the system documents `l_param` as a
        // pointer to the `CREATESTRUCTW` it built from the `CreateWindowExW`
        // arguments; it is valid for the duration of this message, and only
        // one field is read out of it.
        let shared = unsafe { (*(l_param as *const CreateStructW)).lp_create_params };
        // SAFETY: `hwnd` is this window and `GWLP_USERDATA` is a pointer-sized
        // slot the system reserves for the application on every window. The
        // value is a pointer to the shell's `Shared`, which outlives every
        // window it created.
        unsafe { ffi::SetWindowLongPtrW(hwnd, value::GWLP_USERDATA, shared as isize) };
        // `WM_NCCREATE` must reach the default handler: refusing it aborts the
        // creation, and swallowing it leaves the non-client area uninitialised.
        return default();
    }

    // SAFETY: reading back the slot written above. Zero until `WM_NCCREATE`
    // (`WM_GETMINMAXINFO` genuinely arrives before it) and zero again after
    // `WM_NCDESTROY`, both of which the null check below handles.
    let shared = unsafe { ffi::GetWindowLongPtrW(hwnd, value::GWLP_USERDATA) } as *const Shared;
    // SAFETY: non-null here means the pointer was stored by `create_window`
    // from an `Rc<Shared>` the shell owns and has not dropped — the shell
    // destroys every window before releasing it, and a destroyed window's slot
    // is cleared in `WM_NCDESTROY`. The reference does not escape this call.
    let Some(shared) = (unsafe { shared.as_ref() }) else {
        return default();
    };
    let window = key(hwnd);

    match message {
        msg::SIZE => {
            if w_param == value::SIZE_MINIMIZED {
                // A minimized window reports 0×0, which is a visibility change
                // wearing a resize's clothes. See the module docs.
                shared.push(RawEvent::Minimized { hwnd: window });
            } else {
                shared.push(RawEvent::Resized {
                    hwnd: window,
                    // `l_param` carries the **client** area, which is exactly
                    // what the seam means by a window's size, so there is no
                    // frame to subtract here.
                    size: PhysicalSize::new(
                        low_word(l_param as usize),
                        high_word(l_param as usize),
                    ),
                });
            }
            0
        }

        // Intercepted, never forwarded: `DefWindowProc`'s answer to `WM_CLOSE`
        // is `DestroyWindow`, and the whole point of
        // [`CloseReply`](crate::CloseReply) is that the application gets to say
        // no. This one missing `default()` call is the difference between a
        // save prompt and a lost document.
        msg::CLOSE => {
            shared.push(RawEvent::CloseRequested { hwnd: window });
            0
        }

        msg::SET_FOCUS | msg::KILL_FOCUS => {
            shared.push(RawEvent::Focus {
                hwnd: window,
                focused: message == msg::SET_FOCUS,
            });
            0
        }

        msg::DPI_CHANGED => {
            // SAFETY: for `WM_DPICHANGED` the system documents `l_param` as a
            // pointer to the `RECT` it suggests the window move to; it is
            // valid for the duration of this message and is only read.
            let suggested = unsafe { *(l_param as *const Rect) };
            // Applying the suggestion is not optional in the per-monitor-v2
            // contract: a window that ignores it stays at its old pixel size on
            // a monitor at a different scale, which is the "everything is tiny
            // on the second screen" bug. The `WM_SIZE` this produces is what
            // reports the new size.
            //
            // SAFETY: moving this shell's own window, with a null insert-after
            // that `SWP_NO_Z_ORDER` makes unused.
            unsafe {
                ffi::SetWindowPos(
                    hwnd,
                    ptr::null_mut(),
                    suggested.left,
                    suggested.top,
                    suggested.width(),
                    suggested.height(),
                    value::SWP_NO_Z_ORDER | value::SWP_NO_ACTIVATE,
                );
            }
            shared.push(RawEvent::DpiChanged {
                hwnd: window,
                // Both halves of `w_param` carry a DPI and Windows has never
                // produced an anisotropic one; the X half is what
                // `GetDpiForWindow` would answer.
                dpi: low_word(w_param),
            });
            0
        }

        msg::GET_MIN_MAX_INFO => {
            let Some(limits) = shared.limits_for(window) else {
                return default();
            };
            // SAFETY: for `WM_GETMINMAXINFO` the system documents `l_param` as
            // a pointer to a `MINMAXINFO` it has already filled with defaults
            // and expects to be written through; it is valid for the duration
            // of this message.
            let info = unsafe { &mut *(l_param as *mut MinMaxInfo) };
            if let Some(min) = limits.track.min {
                info.pt_min_track_size = min;
            }
            if let Some(max) = limits.track.max {
                info.pt_max_track_size = max;
            }
            // `pt_max_size` is deliberately untouched — a constraint bounds a
            // *drag*, and a maximize button that produced a window smaller than
            // the monitor would be a different feature.
            0
        }

        msg::SIZING => {
            let Some(limits) = shared.limits_for(window) else {
                return default();
            };
            let Some(aspect) = limits.aspect else {
                return default();
            };
            // SAFETY: for `WM_SIZING` the system documents `l_param` as a
            // pointer to the `RECT` it is about to apply, and rewriting it is
            // the documented way to constrain an interactive resize. Valid for
            // the duration of this message.
            let rect = unsafe { &mut *(l_param as *mut Rect) };
            *rect = geometry::adjust_sizing_rect(*rect, w_param, aspect, limits.frame);
            // `TRUE`: the rectangle was changed.
            1
        }

        msg::DESTROY => {
            shared.push(RawEvent::Destroyed { hwnd: window });
            0
        }

        msg::NC_DESTROY => {
            shared.forget(window);
            // The last message this window will ever receive, and the last
            // moment the slot is ours: the `HWND` value can be reused
            // afterwards, and a stale `Shared` pointer left behind would be
            // read by whatever window inherits it.
            //
            // SAFETY: as the write in `WM_NCCREATE`; the window is still live
            // for the duration of this message.
            unsafe { ffi::SetWindowLongPtrW(hwnd, value::GWLP_USERDATA, 0) };
            default()
        }

        // Sent to every top-level window, so a two-window shell sees it twice
        // for one cable; `enqueue` collapses that. Forwarded as well, because
        // the default handler does bookkeeping of its own.
        msg::DISPLAY_CHANGE => {
            shared.push(RawEvent::MonitorsChanged);
            default()
        }

        _ => default(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{LogicalSize, SizeConstraints};

    const FRAME: Frame = Frame {
        width: 16,
        height: 39,
    };

    fn limits(hwnd: isize) -> Limits {
        Limits {
            hwnd,
            track: super::super::geometry::track_sizes(
                SizeConstraints::min(LogicalSize::new(320.0, 240.0)),
                1.0,
                FRAME,
            ),
            aspect: None,
            frame: FRAME,
        }
    }

    #[test]
    fn a_window_is_keyed_by_its_handle_and_a_null_one_is_zero() {
        // The queue matches on this, so it has to be the handle and nothing
        // derived from it. Null is what an `HWND` field holds before a window
        // exists, and it must not collide with a real window — no window has
        // handle zero.
        assert_eq!(key(core::ptr::null_mut()), 0);
        assert_eq!(key(0x1234 as Handle), 0x1234);
    }

    #[test]
    fn a_words_halves_are_the_client_width_and_height() {
        // `WM_SIZE` packs both into one `LPARAM`, and reading them the wrong
        // way round produces a window that is exactly as tall as it is wide.
        let packed = (720usize << 16) | 1280usize;
        assert_eq!(low_word(packed), 1280);
        assert_eq!(high_word(packed), 720);
        // A 4K width is still inside 16 bits; the ceiling is 65535, which no
        // client area reaches.
        assert_eq!(low_word(3840), 3840);
        assert_eq!(high_word(0xFFFF_FFFF), 0xFFFF);
    }

    #[test]
    fn limits_are_replaced_per_window_and_forgotten_on_the_way_out() {
        let shared = Shared::default();
        shared.set_limits(limits(1));
        shared.set_limits(limits(2));
        assert_eq!(shared.limits_for(1).map(|found| found.hwnd), Some(1));

        // Replaced in place, not appended: a second `set_constraints` must not
        // leave the first answer for `WM_GETMINMAXINFO` to find.
        let mut updated = limits(1);
        updated.aspect = Some(AspectRatio::WIDESCREEN);
        shared.set_limits(updated);
        assert_eq!(shared.limits.borrow().len(), 2);
        assert_eq!(
            shared.limits_for(1).and_then(|found| found.aspect),
            Some(AspectRatio::WIDESCREEN)
        );

        // The two numbers `WM_GETMINMAXINFO` and `WM_SIZING` are answered
        // from, carried whole rather than recomputed in the procedure.
        let found = shared.limits_for(2).expect("still there");
        assert_eq!(found.frame, FRAME);
        assert_eq!(
            found.track.min,
            Some(super::super::ffi::Point {
                x: 320 + FRAME.width,
                y: 240 + FRAME.height
            })
        );

        shared.forget(1);
        assert!(shared.limits_for(1).is_none());
        assert!(shared.limits_for(2).is_some(), "one window, not the table");
    }

    #[test]
    fn taking_the_queue_empties_it_and_holds_no_borrow_afterwards() {
        // The re-entrancy rule in one test: after `take_events` the `RefCell`
        // is free, so the window procedure a subsequent `SetWindowPos` invokes
        // can push into it. Holding a borrow here would panic instead.
        let shared = Shared::default();
        shared.push(RawEvent::MonitorsChanged);
        shared.push(RawEvent::CloseRequested { hwnd: 7 });
        let taken = shared.take_events();
        assert_eq!(taken.len(), 2);
        assert!(shared.take_events().is_empty());
        shared.push(RawEvent::Destroyed { hwnd: 7 });
        assert_eq!(shared.take_events(), [RawEvent::Destroyed { hwnd: 7 }]);
    }
}
