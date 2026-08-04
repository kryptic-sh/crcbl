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
//!
//! # The four things input adds to that list
//!
//! Input mostly fits the record-and-answer-later shape, and four pieces of it
//! do not. Each is here because deferring it to the next
//! [`pump`](crate::Shell::pump) would be observably wrong, not merely late:
//!
//! 1. **`WM_SETCURSOR`.** The system asks which cursor to draw and uses
//!    `DefWindowProc`'s answer — the *class* cursor — if we do not give one.
//!    Answering next frame means answering after the arrow has already been
//!    drawn, on every mouse movement.
//! 2. **`SetCapture` on a button press.** A button released outside the window
//!    is delivered to whatever window is under the pointer unless the capture
//!    was taken *when the button went down*. Taking it a frame later is taking
//!    it after the drag has already escaped.
//! 3. **`TrackMouseEvent` on the derived enter.** Windows has no
//!    `WM_MOUSEENTER`; the leave notification has to be armed at the moment the
//!    pointer arrives, and the request is one-shot.
//! 4. **`ClipCursor(NULL)` on focus loss.** This is the one that is not a
//!    latency question at all. A process that keeps the cursor clipped after it
//!    stops being the foreground window has **taken the desktop hostage**: the
//!    user cannot reach the taskbar, another window, or the button that would
//!    close ours. Deferring it to the next pump makes the hostage-taking last
//!    exactly as long as the application's next frame takes — which, for the
//!    application that is about to hang, is forever. So the clip is released
//!    here, synchronously, before the procedure returns.
//!
//! The cursor's *visibility* deliberately does **not** join that list. It is
//! `ShowCursor`'s per-thread reference count, which only takes effect while the
//! pointer is over a window of this thread — so an un-hide that arrives a frame
//! late is an invisible cursor over our own window for one frame, and cannot
//! escape onto the desktop the way a clip can. It stays with the shell, where
//! the count has one owner. See [`pointer::Visibility`](super::pointer::Visibility).

use core::cell::{Cell, RefCell};
use std::collections::VecDeque;
use std::path::PathBuf;

use crate::AspectRatio;

use super::events::{RawEvent, enqueue};
use super::ffi::Handle;
use super::geometry::{Frame, Track};

// The procedure itself is the only part of this module that calls into the
// system, so everything above it — the shared queue, the cached limits and the
// word arithmetic — compiles and is tested on every host. See
// [`geometry`](super::geometry) for why that matters.
#[cfg(target_os = "windows")]
use super::dnd;
#[cfg(target_os = "windows")]
use super::ffi::{
    self, CreateStructW, Lparam, Lresult, MinMaxInfo, Point, Rect, Wparam, msg, value,
};
#[cfg(target_os = "windows")]
use super::geometry;
#[cfg(target_os = "windows")]
use super::input;
#[cfg(target_os = "windows")]
use super::{keys, pointer};
#[cfg(target_os = "windows")]
use crate::PhysicalSize;
#[cfg(target_os = "windows")]
use core::ptr;
#[cfg(target_os = "windows")]
use crcbl_core::input::ButtonState;

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

/// A window's cursor shape, for the one message allowed to set it.
///
/// Recorded by [`set_cursor`](crate::Shell::set_cursor) and read by
/// `WM_SETCURSOR`. Hiding is **not** here: it is `ShowCursor`'s reference count,
/// which is per thread rather than per window and belongs to the shell.
#[derive(Clone, Copy, Debug)]
pub(super) struct Shape {
    /// Which window.
    pub hwnd: isize,
    /// The `HCURSOR`, owned by the system — a stock cursor from `LoadCursorW`,
    /// which must not be destroyed.
    pub cursor: Handle,
}

/// The paths one `WM_DROPFILES` carried.
///
/// Kept beside the event queue rather than in it, because a
/// [`RawEvent`] is `Copy` and this is not — see
/// [`dnd`](super::dnd) for why the reading cannot be deferred to the pump
/// either. [`RawEvent::FilesDropped`] is the marker that keeps this in order
/// relative to everything else the procedure saw.
#[derive(Clone, Debug)]
pub(super) struct FileDrop {
    /// Which window, so a marker claims its own payload.
    pub hwnd: isize,
    /// The dropped paths, one [`DroppedFile`](crate::ShellEvent::DroppedFile)
    /// each. May be empty for a drop of nothing this backend could decode.
    pub paths: Vec<PathBuf>,
    /// Client x of the drop point.
    pub x: i32,
    /// Client y.
    pub y: i32,
}

/// What the shell and its window procedure share.
///
/// Owned by the shell as an `Rc`, and reached by the procedure as a raw pointer
/// out of `GWLP_USERDATA`. The soundness argument is one sentence and the shell
/// is what has to keep it true: **every window is destroyed before this is
/// dropped**, so no procedure can run after the allocation goes away. See
/// [`Win32Shell::drop`](super::Win32Shell).
///
/// The four `Cell`s are the state the *procedure* owns rather than the shell,
/// because each answers a question the procedure has to act on before it
/// returns — see the [module docs](self). They are `Cell` rather than `RefCell`
/// because each is one `Copy` value: there is no borrow to be held across a call
/// back into the system, which is the hazard the `RefCell`s carry.
#[derive(Debug, Default)]
pub(super) struct Shared {
    events: RefCell<Vec<RawEvent>>,
    limits: RefCell<Vec<Limits>>,
    shapes: RefCell<Vec<Shape>>,
    /// Drop payloads, oldest first, each claimed by the
    /// [`FilesDropped`](RawEvent::FilesDropped) marker that was pushed with it.
    drops: RefCell<VecDeque<FileDrop>>,
    /// The window `TrackMouseEvent` is currently armed for, or zero.
    ///
    /// Also the answer to "is the pointer inside?", which is the only way to
    /// derive the entry Windows does not send.
    tracked: Cell<isize>,
    /// The window holding the mouse capture, and which buttons are down on it.
    ///
    /// A mask rather than a count; [`pointer::button_bit`](super::pointer::button_bit)
    /// says why.
    capture: Cell<(isize, u32)>,
    /// The window the cursor is clipped to, or zero.
    ///
    /// Kept across a focus loss — the clip is *released* there and re-applied
    /// when focus comes back — so this records what the shell asked for rather
    /// than what the system currently has.
    clipped: Cell<isize>,
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

    /// Records the paths a `WM_DROPFILES` carried, for the marker pushed with
    /// it to claim.
    fn push_drop(&self, drop: FileDrop) {
        self.drops.borrow_mut().push_back(drop);
    }

    /// Takes the oldest drop payload recorded for `hwnd`.
    ///
    /// Matched by window rather than taken from the front, so that a payload
    /// discarded by [`forget`](Self::forget) — the window died before the pump
    /// reached its marker — cannot hand the *next* window's paths to a marker
    /// that is not theirs.
    pub(super) fn take_drop(&self, hwnd: isize) -> Option<FileDrop> {
        let mut drops = self.drops.borrow_mut();
        let index = drops.iter().position(|drop| drop.hwnd == hwnd)?;
        drops.remove(index)
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

    /// Drops everything recorded for a window, on the way out.
    ///
    /// Every table **and** every `Cell` that could still name it: an `HWND`
    /// value is handed to a new window after the old one dies, so a leftover
    /// clip or capture target would be applied to whichever window inherits the
    /// number.
    pub(super) fn forget(&self, hwnd: isize) {
        self.limits.borrow_mut().retain(|known| known.hwnd != hwnd);
        self.shapes.borrow_mut().retain(|known| known.hwnd != hwnd);
        self.drops.borrow_mut().retain(|drop| drop.hwnd != hwnd);
        if self.tracked.get() == hwnd {
            self.tracked.set(0);
        }
        if self.capture.get().0 == hwnd {
            self.capture.set((0, 0));
        }
        if self.clipped.get() == hwnd {
            self.clipped.set(0);
        }
    }

    /// A window's limits, by value so no borrow outlives the lookup.
    fn limits_for(&self, hwnd: isize) -> Option<Limits> {
        self.limits
            .borrow()
            .iter()
            .find(|known| known.hwnd == hwnd)
            .copied()
    }

    /// Publishes the cursor `WM_SETCURSOR` will set for a window.
    pub(super) fn set_shape(&self, shape: Shape) {
        let mut table = self.shapes.borrow_mut();
        match table.iter_mut().find(|known| known.hwnd == shape.hwnd) {
            Some(known) => *known = shape,
            None => table.push(shape),
        }
    }

    /// A window's cursor, or `None` to leave the class cursor alone.
    fn shape_for(&self, hwnd: isize) -> Option<Handle> {
        self.shapes
            .borrow()
            .iter()
            .find(|known| known.hwnd == hwnd)
            .map(|known| known.cursor)
    }

    /// Whether the pointer has just arrived in `hwnd`.
    ///
    /// `true` exactly once per entry, which is what makes it both the
    /// [`PointerFocus`](RawEvent::PointerFocus) edge and the moment to arm
    /// `TrackMouseEvent`. Two windows cannot both be tracked, because the
    /// pointer is in one place.
    fn enter(&self, hwnd: isize) -> bool {
        if self.tracked.get() == hwnd {
            return false;
        }
        self.tracked.set(hwnd);
        true
    }

    /// Whether a `WM_MOUSELEAVE` belongs to the window currently tracked.
    ///
    /// A leave for a window we are not tracking is a stale one-shot from before
    /// the last entry, and reporting it would tell a consumer the pointer left a
    /// window it is currently inside.
    fn leave(&self, hwnd: isize) -> bool {
        if self.tracked.get() != hwnd {
            return false;
        }
        self.tracked.set(0);
        true
    }

    /// Records a button going down; `true` when the capture must be taken.
    ///
    /// A press in a *different* window replaces the mask rather than merging
    /// into it: the previous window's buttons cannot still be down if the
    /// pointer is here, and merging would leave a bit set that no release ever
    /// clears — a capture that is never given back.
    fn press(&self, hwnd: isize, bit: u32) -> bool {
        let (holder, held) = self.capture.get();
        if holder != hwnd {
            self.capture.set((hwnd, bit));
            return true;
        }
        self.capture.set((hwnd, held | bit));
        held == 0
    }

    /// Records a button coming up; `true` when the capture must be released.
    fn release(&self, hwnd: isize, bit: u32) -> bool {
        let (holder, held) = self.capture.get();
        if holder != hwnd {
            // A release with no press — a button held down before this window
            // existed, or one whose press went to another application.
            return false;
        }
        let held = held & !bit;
        if held == 0 {
            self.capture.set((0, 0));
            return true;
        }
        self.capture.set((hwnd, held));
        false
    }

    /// Forgets the capture because somebody else took it.
    ///
    /// `WM_CAPTURECHANGED` arrives when another window grabs the mouse — a
    /// system menu, a drag from another application. Our mask would otherwise
    /// still say a button is down and the next release would try to give back a
    /// capture we no longer hold.
    fn capture_lost(&self) {
        self.capture.set((0, 0));
    }

    /// Records which window the cursor is clipped to; zero for none.
    pub(super) fn set_clipped(&self, hwnd: isize) {
        self.clipped.set(hwnd);
    }

    /// The window the cursor is clipped to, or zero.
    pub(super) fn clipped(&self) -> isize {
        self.clipped.get()
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
    // The message's own time, not the time the queue is drained. A window
    // procedure is never handed its `MSG`, so this is the only way to reach it;
    // `TimeBase` widens the 32 bits and rebases them onto the engine's clock.
    //
    // SAFETY: `GetMessageTime` takes no arguments, reads no memory of ours and
    // answers for the message this thread is currently dispatching — which is
    // the one that called us.
    let millis = unsafe { ffi::GetMessageTime() } as u32;

    match message {
        msg::SIZE => {
            if w_param == value::SIZE_MINIMIZED {
                // A minimized window reports 0×0, which is a visibility change
                // wearing a resize's clothes. See the module docs.
                shared.push(RawEvent::Minimized { hwnd: window });
                // **No reclip here.** `client_screen_rect` of an iconic window
                // maps both corners of the 0×0 client area to the same point,
                // and re-applying that pins the cursor for the whole minimized
                // period — the desktop-hostage invariant this module exists to
                // keep. The recorded target survives, so the first `WM_SIZE`
                // after restore re-clips from the real rectangle.
                if shared.clipped() == window {
                    input::release_clip();
                }
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
                // A clip is a screen rectangle built from a client rectangle
                // that has just changed shape.
                input::reclip(shared, hwnd, window);
            }
            0
        }

        // Nothing is recorded — the seam has no "the window moved" event — and
        // the clip still has to follow, because it was computed in screen
        // coordinates from a window that is no longer there.
        msg::MOVE => {
            input::reclip(shared, hwnd, window);
            0
        }

        // The modal drag loop ran our procedure for every intermediate position
        // and returned here. Re-establishing once at the end is not redundant
        // with the `WM_MOVE`/`WM_SIZE` arms above: a drag that the user cancels
        // with Escape ends with neither.
        msg::EXIT_SIZE_MOVE => {
            input::reclip(shared, hwnd, window);
            default()
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
            let focused = message == msg::SET_FOCUS;
            // **Not deferred to the pump.** A window that keeps the cursor
            // clipped after it stops being the foreground window has taken the
            // desktop hostage; see the module docs for why one frame of that is
            // one frame too many. The recorded target survives, so focus coming
            // back restores the clip the caller asked for.
            if shared.clipped() == window {
                if focused {
                    input::apply_clip(hwnd);
                } else {
                    input::release_clip();
                }
            }
            shared.push(RawEvent::Focus {
                hwnd: window,
                focused,
            });
            0
        }

        msg::KEY_DOWN | msg::KEY_UP | msg::SYS_KEY_DOWN | msg::SYS_KEY_UP => {
            let pressed = message == msg::KEY_DOWN || message == msg::SYS_KEY_DOWN;
            shared.push(RawEvent::Key {
                hwnd: window,
                scancode: keys::scancode(l_param),
                virtual_key: low_word(w_param) as u16,
                state: ButtonState::from_pressed(pressed),
                // Bit 30 is meaningful only on the way down; on a release it is
                // always set, and reading it there reports every release as a
                // repeat.
                repeat: pressed && keys::was_already_down(l_param),
                millis,
            });
            if message == msg::SYS_KEY_DOWN || message == msg::SYS_KEY_UP {
                // The `WM_SYS*` pair **must** reach the default handler. It is
                // what turns Alt+F4 into the `WM_CLOSE` this backend intercepts
                // into a close *request*, and what opens the window menu on
                // Alt+Space. Swallowing them silently removes the platform's own
                // keyboard contract from every Crucible window.
                default()
            } else {
                0
            }
        }

        // Text, which is not the same thing as a keystroke — see
        // [`TextCommit`](crate::ShellEvent::TextCommit). `WM_SYSCHAR` is
        // deliberately absent: Alt+E is a menu accelerator, not the letter E.
        msg::CHAR => {
            shared.push(RawEvent::Char {
                hwnd: window,
                unit: low_word(w_param) as u16,
                millis,
            });
            0
        }

        msg::MOUSE_MOVE => {
            let (x, y) = pointer::point(l_param);
            if shared.enter(window) {
                // There is no `WM_MOUSEENTER`. The arrival is derived from the
                // first movement after a leave, and the leave has to be asked
                // for — `TrackMouseEvent` is one-shot, so this is also where it
                // is re-armed.
                input::track_leave(hwnd);
                shared.push(RawEvent::PointerFocus {
                    hwnd: window,
                    entered: true,
                    x,
                    y,
                    millis,
                });
            }
            shared.push(RawEvent::PointerMotion {
                hwnd: window,
                x,
                y,
                millis,
            });
            0
        }

        msg::MOUSE_LEAVE => {
            if shared.leave(window) {
                shared.push(RawEvent::PointerFocus {
                    hwnd: window,
                    entered: false,
                    x: 0,
                    y: 0,
                    millis,
                });
            }
            0
        }

        msg::L_BUTTON_DOWN
        | msg::L_BUTTON_UP
        | msg::R_BUTTON_DOWN
        | msg::R_BUTTON_UP
        | msg::M_BUTTON_DOWN
        | msg::M_BUTTON_UP
        | msg::X_BUTTON_DOWN
        | msg::X_BUTTON_UP => {
            let Some((button, state)) = pointer::button(message, w_param) else {
                return default();
            };
            let (x, y) = pointer::point(l_param);
            let bit = pointer::button_bit(button);
            // The capture, taken on the first button down and given back on the
            // last button up. Without it a drag that leaves the window is
            // delivered to whatever is under the pointer, so the release never
            // arrives here and the button stays down forever from the
            // consumer's point of view.
            if state.is_pressed() {
                if shared.press(window, bit) {
                    // SAFETY: this shell's own window, on the thread that owns
                    // it — a window procedure runs on that thread by
                    // construction. `SetCapture` sends `WM_CAPTURECHANGED` to
                    // whoever held it, which this procedure handles below.
                    unsafe { ffi::SetCapture(hwnd) };
                }
            } else if shared.release(window, bit) {
                // SAFETY: releasing a capture this thread holds; a no-op when
                // it does not.
                unsafe { ffi::ReleaseCapture() };
            }
            shared.push(RawEvent::Button {
                hwnd: window,
                button,
                state,
                x,
                y,
                millis,
            });
            if message == msg::X_BUTTON_DOWN || message == msg::X_BUTTON_UP {
                // The two `WM_XBUTTON*` messages are the only mouse messages
                // documented to want `TRUE` rather than zero.
                1
            } else {
                0
            }
        }

        // Somebody else took the mouse. Our mask has to be cleared or the next
        // release tries to give back a capture we no longer hold.
        //
        // `l_param` is the window *gaining* it, and the check is not
        // decoration: `SetCapture` sends this to the window losing the capture,
        // and our own call above is what sends it. Clearing unconditionally
        // would wipe the mask one line after the press that set it, and the
        // matching release would then never give the capture back.
        msg::CAPTURE_CHANGED => {
            if l_param != hwnd as Lparam {
                shared.capture_lost();
            }
            default()
        }

        msg::MOUSE_WHEEL | msg::MOUSE_H_WHEEL => {
            // **The one pair that carries screen coordinates.** Every other
            // mouse message is in client space; reading these the same way puts
            // a scroll at a position off the bottom-right of the window on any
            // window that is not at the desktop origin.
            let (x, y) = pointer::point(l_param);
            let mut position = Point { x, y };
            // SAFETY: `position` is a live, initialised `POINT` the call
            // converts in place, and `hwnd` is a live window of this shell.
            unsafe { ffi::ScreenToClient(hwnd, &raw mut position) };
            shared.push(RawEvent::Wheel {
                hwnd: window,
                horizontal: message == msg::MOUSE_H_WHEEL,
                ticks: high_word(w_param) as u16 as i16,
                x: position.x,
                y: position.y,
                millis,
            });
            0
        }

        msg::INPUT => {
            // SAFETY: for `WM_INPUT` the system documents `l_param` as an
            // `HRAWINPUT` handle valid for the duration of this message, which
            // is exactly what `GetRawInputData` takes.
            if let Some(mouse) = unsafe { input::read_raw_mouse(l_param) } {
                shared.push(RawEvent::RawMotion {
                    hwnd: window,
                    flags: mouse.us_flags,
                    x: mouse.l_last_x,
                    y: mouse.l_last_y,
                    millis,
                });
            }
            // `WM_INPUT` must reach the default handler even when it was read:
            // that is what releases the report's buffer.
            default()
        }

        msg::SET_CURSOR => {
            // Only over the drawable area. Every other hit-test code names a
            // piece of the frame the system owns a cursor for, and answering
            // for those replaces the resize arrows with ours.
            if low_word(l_param as usize) != value::HT_CLIENT {
                return default();
            }
            let Some(cursor) = shared.shape_for(window) else {
                // Nothing was asked for, so the class cursor is right and
                // `DefWindowProc` is what sets it.
                return default();
            };
            // SAFETY: `cursor` is a stock cursor handle from `LoadCursorW`,
            // owned by the system and valid for the process's lifetime.
            unsafe { ffi::SetCursor(cursor) };
            // `TRUE`: the cursor was set, so `DefWindowProc` must not overwrite
            // it with the class one on the next mouse movement.
            1
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

        // The one message whose payload cannot wait for the pump: the `HDROP`
        // is live for the length of this call and is owed a `DragFinish`. The
        // paths therefore come out here and go on the shared drop queue, and
        // the event pushed after them is the marker that claims them. See
        // [`dnd`](super::dnd).
        msg::DROP_FILES => {
            // SAFETY: for `WM_DROPFILES` the system documents `w_param` as the
            // `HDROP` for this drop, valid until it is finished — which
            // `take_drop` does, exactly once, before returning.
            let (paths, point) = unsafe { dnd::take_drop(w_param as Handle) };
            shared.push_drop(FileDrop {
                hwnd: window,
                paths,
                x: point.x,
                y: point.y,
            });
            shared.push(RawEvent::FilesDropped {
                hwnd: window,
                millis,
            });
            0
        }

        msg::DESTROY => {
            shared.push(RawEvent::Destroyed { hwnd: window });
            0
        }

        msg::NC_DESTROY => {
            // The clip outlives the window that asked for it unless somebody
            // takes it back, and a window being destroyed is the one moment
            // nothing else will.
            if shared.clipped() == window {
                input::release_clip();
            }
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
    fn the_pointer_enters_once_per_arrival_and_a_stale_leave_is_ignored() {
        // Windows sends no `WM_MOUSEENTER`, so the arrival is derived from the
        // first movement after a leave — which means the latch has to be right
        // or every mouse movement reports an entry.
        let shared = Shared::default();
        assert!(shared.enter(1), "the first movement is an arrival");
        assert!(!shared.enter(1), "and the next two hundred are not");
        assert!(!shared.enter(1));

        // The pointer is in one place, so moving to another window is a leave
        // of the first even before its one-shot fires.
        assert!(shared.enter(2));
        assert!(
            !shared.leave(1),
            "a leave for a window we stopped tracking is stale"
        );
        assert!(shared.leave(2));
        assert!(!shared.leave(2), "and it only fires once");
        // After a leave the next movement is an arrival again.
        assert!(shared.enter(2));
    }

    #[test]
    fn the_capture_is_taken_on_the_first_button_and_given_back_on_the_last() {
        // The whole point of the mask: a two-button drag that releases one
        // button outside the window must not release the capture, or the second
        // release goes to whatever is underneath and the button stays down
        // forever from the consumer's point of view.
        let shared = Shared::default();
        const LEFT: u32 = 1;
        const RIGHT: u32 = 2;
        assert!(shared.press(1, LEFT), "the first press takes it");
        assert!(!shared.press(1, RIGHT), "the second does not take it twice");
        assert!(!shared.release(1, LEFT), "one button is still down");
        assert!(shared.release(1, RIGHT), "the last one gives it back");
        assert!(!shared.release(1, RIGHT), "and not a second time");

        // A release with no press — a button held before the window existed.
        // A counter would go negative here and never release again.
        assert!(!shared.release(1, LEFT));
        assert!(shared.press(1, LEFT), "which must still work afterwards");

        // A press in another window replaces rather than merging: the first
        // window's bit would otherwise never be cleared.
        assert!(shared.press(2, RIGHT));
        assert!(shared.release(2, RIGHT));

        // And somebody else taking the mouse clears the mask outright.
        assert!(shared.press(1, LEFT));
        shared.capture_lost();
        assert!(
            shared.press(1, RIGHT),
            "the next press takes the capture again"
        );
    }

    #[test]
    fn a_cursor_shape_is_replaced_per_window_and_read_back_for_wm_setcursor() {
        let shared = Shared::default();
        let arrow = 0x1234 as Handle;
        let beam = 0x5678 as Handle;
        assert_eq!(shared.shape_for(1), None, "nothing asked for yet");

        shared.set_shape(Shape {
            hwnd: 1,
            cursor: arrow,
        });
        shared.set_shape(Shape {
            hwnd: 2,
            cursor: beam,
        });
        assert_eq!(shared.shape_for(1), Some(arrow));
        assert_eq!(shared.shape_for(2), Some(beam));

        // Replaced in place: a second `set_cursor` must not leave the first
        // shape for `WM_SETCURSOR` to find.
        shared.set_shape(Shape {
            hwnd: 1,
            cursor: beam,
        });
        assert_eq!(shared.shape_for(1), Some(beam));
        assert_eq!(shared.shapes.borrow().len(), 2);
    }

    #[test]
    fn a_destroyed_window_leaves_nothing_behind_that_names_it() {
        // An `HWND` value is handed to a *new* window after the old one dies,
        // so a leftover clip or capture target would be applied to whichever
        // window inherits the number — and a leftover tracking latch would
        // suppress the new window's first pointer entry.
        let shared = Shared::default();
        shared.set_limits(limits(1));
        shared.set_shape(Shape {
            hwnd: 1,
            cursor: 0x1234 as Handle,
        });
        assert!(shared.enter(1));
        assert!(shared.press(1, 1));
        shared.set_clipped(1);

        shared.forget(1);
        assert!(shared.limits_for(1).is_none());
        assert_eq!(shared.shape_for(1), None);
        assert_eq!(shared.clipped(), 0);
        assert!(shared.enter(1), "the reused handle is a fresh arrival");
        assert!(shared.press(1, 1), "and a fresh capture");

        // Another window's state is untouched.
        shared.set_clipped(2);
        shared.set_shape(Shape {
            hwnd: 2,
            cursor: 0x5678 as Handle,
        });
        shared.forget(1);
        assert_eq!(shared.clipped(), 2);
        assert!(shared.shape_for(2).is_some());
    }

    #[test]
    fn each_drop_marker_claims_its_own_payload_in_arrival_order() {
        // The pairing the `FilesDropped` marker depends on. Getting it wrong
        // hands one window's dropped files to another window's event, which is
        // an asset imported into the wrong scene rather than an error.
        let shared = Shared::default();
        let drop = |hwnd: isize, name: &str| FileDrop {
            hwnd,
            paths: vec![PathBuf::from(name)],
            x: 4,
            y: 5,
        };
        shared.push_drop(drop(1, "first"));
        shared.push_drop(drop(2, "other window"));
        shared.push_drop(drop(1, "second"));

        let first = shared.take_drop(1).expect("queued");
        assert_eq!(
            first.paths,
            [PathBuf::from("first")],
            "oldest first, per window"
        );
        assert_eq!(
            (first.x, first.y),
            (4, 5),
            "the drop point travels with its paths — `DroppedFile` carries both"
        );
        assert_eq!(
            shared.take_drop(1).expect("queued").paths,
            [PathBuf::from("second")]
        );
        assert!(shared.take_drop(1).is_none(), "and no third");
        assert_eq!(
            shared.take_drop(2).expect("untouched").paths,
            [PathBuf::from("other window")],
            "another window's drop was never in the running"
        );
    }

    #[test]
    fn a_drop_whose_window_died_before_the_pump_is_discarded_with_it() {
        // The reason `take_drop` matches on the window rather than popping the
        // front: `forget` removes the payload, and a marker still in the raw
        // queue must then find nothing — not the next window's files.
        let shared = Shared::default();
        shared.push_drop(FileDrop {
            hwnd: 1,
            paths: vec![PathBuf::from("doomed")],
            x: 0,
            y: 0,
        });
        shared.push_drop(FileDrop {
            hwnd: 2,
            paths: vec![PathBuf::from("survivor")],
            x: 0,
            y: 0,
        });
        shared.forget(1);
        assert!(shared.take_drop(1).is_none());
        assert_eq!(
            shared.take_drop(2).expect("still there").paths,
            [PathBuf::from("survivor")]
        );
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
