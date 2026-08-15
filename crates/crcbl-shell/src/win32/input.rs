//! Where Win32 input meets the system: raw-input registration, the clip, the
//! cursor, and the translation of what the window procedure recorded into
//! [`ShellEvent`]s.
//!
//! The arithmetic is in [`keys`](super::keys) and [`pointer`](super::pointer)
//! and is tested on any host; this module is what cannot be — every function
//! here either calls into `user32` or needs the shell's `&mut self`.
//!
//! # Decision: raw input is registered once and never removed
//!
//! `RegisterRawInputDevices` is a **process** registration, keyed by HID usage,
//! exactly as a window class is a process registration keyed by name — so it
//! gets the same treatment for the same reasons, behind a [`OnceLock`] and never
//! unregistered. Removing it on drop would break a second [`Win32Shell`] still
//! running in the same process, and there is nothing to reclaim: an unused
//! registration costs a message that arrives only while one of our windows has
//! the keyboard focus.
//!
//! `hwndTarget` is null, which is the documented way to say "follow the keyboard
//! focus". That is not a shortcut, it is the behaviour this backend wants, and
//! it is the same conclusion the X11 backend reached from the opposite
//! direction. There, `XI_RawMotion` must be selected on the **root** and
//! therefore arrives while some other application is focused, so that backend
//! has to reconstruct the attribution by hand — "a first-person camera that
//! keeps turning while the player is typing in another window" is what it is
//! avoiding. Win32 gives that for free unless `RIDEV_INPUTSINK` is asked for,
//! and it is not.
//!
//! # Decision: locked is clip-plus-recentre, and the recentre is not optional
//!
//! [`ShellCaps::POINTER_LOCK`](crate::ShellCaps::POINTER_LOCK) names this
//! platform's technique in as many words — "Win32 clip-and-recentre" — and both
//! halves earn their place:
//!
//! * The **clip** is `ClipCursor` over the window's client rectangle in screen
//!   coordinates. It is what stops the pointer reaching another window, and it
//!   is the same call [`Confined`](crate::PointerMode::Confined) makes; the
//!   difference between the two modes is what is reported and whether the cursor
//!   is drawn, not how the pointer is bounded.
//! * The **recentre** is a `SetCursorPos` back to the middle when the pointer
//!   drifts more than a quarter of the window from it — the same rule and the
//!   same fraction as the X11 backend's, deliberately. A clipped pointer still
//!   *moves*, so it eventually rests against an edge; without the recentre,
//!   leaving the mode leaves the invisible cursor wherever it drifted to, which
//!   the user then finds in a corner.
//!
//! What the recentre is **not** needed for is the delta: raw input is read
//! before the cursor is moved at all, so a locked camera's aim is unaffected by
//! either the clip or the warp. That is the whole reason
//! [`PointerMotion::abs`](crate::ShellEvent::PointerMotion) is suppressed while
//! locked rather than being made to work — there is no honest absolute position,
//! and a synthetic one would look correct until the day it did not.
//!
//! # Decision: the cursor is hidden for the focused window, not for any window
//!
//! `ShowCursor`'s count is per **thread**, and every window of a shell is on one
//! thread — so "hide the cursor" cannot be scoped to a window by the API itself.
//! The rule this backend applies is that the cursor is hidden while the window
//! that has the keyboard focus wants it hidden, either by
//! [`PointerMode::Locked`](crate::PointerMode::Locked) or by an explicit
//! [`set_cursor(None)`](crate::Shell::set_cursor).
//!
//! Keying on focus rather than on which window the pointer is inside has one
//! visible consequence and one useful one. The consequence: a focused window
//! that hid the cursor also hides it over another window of the *same* shell.
//! The useful part: losing focus un-hides, so alt-tabbing away from a game in
//! mouselook does not leave the desktop without a pointer. The count cannot
//! escape onto another application's windows in any case — it applies only while
//! the cursor is over this thread's.

use core::ptr;
use std::sync::OnceLock;

use crcbl_core::KeyCode;
use crcbl_core::input::{DeviceId, Keysym, Modifiers, Scancode};

use crate::{
    CursorIcon, PhysicalPoint, PhysicalSize, PointerMode, ShellError, ShellEvent, WindowId,
};

use super::events::RawEvent;
use super::ffi::{
    self, Handle, Lparam, Point, RawInput, RawInputDevice, RawMouse, Rect, TrackMouse, value,
};
use super::keys;
use super::pointer::{self, RawMotion, Show};
use super::proc::{Shape, Shared};
use super::shell::Win32Shell;

/// Which keyboard and pointer an event came from.
///
/// Constants rather than a table, and the same admission the X11 backend makes:
/// [`DeviceId`] exists so the P2 action layer can
/// tell two mice apart, and on this backend it cannot yet. Raw input *does*
/// carry a per-device `hDevice`, which is more than core X11 offers — turning it
/// into a stable [`DeviceId`] needs a handle table
/// and a hotplug story (`WM_INPUT_DEVICE_CHANGE`), and that is a slice of its
/// own. `docs/backlog.md` carries it.
pub(super) const KEYBOARD_DEVICE: DeviceId = DeviceId(1);
/// See [`KEYBOARD_DEVICE`].
pub(super) const POINTER_DEVICE: DeviceId = DeviceId(2);

/// How far from the centre a locked pointer may drift before it is warped back.
///
/// A fraction of the window rather than a fixed margin, so the behaviour is the
/// same on a 640×480 window and a 4K one. The same value as the X11 backend's,
/// because it is the same technique solving the same problem.
const LOCK_RECENTRE_FRACTION: f64 = 0.25;

/// Whether the process has asked for `WM_INPUT`. See the [module docs](self).
static RAW_INPUT: OnceLock<bool> = OnceLock::new();

/// A layout's character for a key, in the case XKB names it in.
///
/// `MAPVK_VK_TO_CHAR`'s "unshifted" means "with no dead key applied", not "with
/// no Shift": for a letter key it answers the **uppercase** letter. XKB — and
/// therefore [`Keysym`] and both Linux backends — names the lowercase symbol, so
/// a rebind menu would read `W` on Windows and `w` everywhere else for the same
/// physical key.
///
/// A character whose lowercase form is more than one codepoint (there are a
/// handful in Unicode, none of them on a keyboard) keeps its original form
/// rather than being truncated into a different letter.
fn unshifted(character: char) -> char {
    let mut lowered = character.to_lowercase();
    match (lowered.next(), lowered.next()) {
        (Some(single), None) => single,
        _ => character,
    }
}

/// Registers the mouse for raw input, once per process.
///
/// Returns whether the registration succeeded, which is what
/// [`RAW_POINTER_MOTION`](crate::ShellCaps::RAW_POINTER_MOTION) is latched from:
/// a shell that could not register it must not claim to deliver it.
pub(super) fn register_raw_input() -> bool {
    *RAW_INPUT.get_or_init(|| {
        let device = RawInputDevice {
            us_usage_page: value::HID_USAGE_PAGE_GENERIC,
            us_usage: value::HID_USAGE_GENERIC_MOUSE,
            // No `RIDEV_INPUTSINK`: reports should stop when another
            // application has the keyboard, which is what a null target gives.
            dw_flags: 0,
            hwnd_target: ptr::null_mut(),
        };
        // SAFETY: `device` is a fully initialised `RAWINPUTDEVICE` that outlives
        // the call — the system copies the registration — and the size argument
        // is the array element's own size, which the call validates.
        let ok = unsafe {
            ffi::RegisterRawInputDevices(&raw const device, 1, size_of::<RawInputDevice>() as u32)
        };
        if ok == 0 {
            // SAFETY: reads this thread's last error code, set by the call
            // above.
            let error = unsafe { ffi::GetLastError() };
            crcbl_core::log::warn!(
                "RegisterRawInputDevices for the mouse failed with Win32 error {error}; \
                 relative pointer motion will not be reported and RAW_POINTER_MOTION is clear"
            );
            return false;
        }
        crcbl_core::log::debug!("raw mouse input is registered for this process");
        true
    })
}

/// The `RAWMOUSE` a `WM_INPUT` carried, if it carried one.
///
/// # Safety
///
/// `l_param` must be the `HRAWINPUT` of a `WM_INPUT` currently being processed,
/// which is the only thing `GetRawInputData` accepts.
pub(super) unsafe fn read_raw_mouse(l_param: Lparam) -> Option<RawMouse> {
    let mut report = RawInput::default();
    let mut size = size_of::<RawInput>() as u32;
    // SAFETY: the caller guarantees the handle; `report` is a live, initialised
    // buffer of exactly `size` bytes, and `header_size` is the header's own size
    // as the call requires. A report larger than our buffer — impossible for the
    // mouse usage, which is the only one registered — returns `u32::MAX` rather
    // than overrunning.
    let written = unsafe {
        ffi::GetRawInputData(
            l_param as Handle,
            value::RID_INPUT,
            (&raw mut report).cast(),
            &raw mut size,
            size_of::<ffi::RawInputHeader>() as u32,
        )
    };
    if written == u32::MAX || written == 0 {
        return None;
    }
    // Checked rather than assumed: only the mouse usage is registered, but the
    // mouse arm of the union is not what a keyboard report would have put there.
    if report.header.dw_type != value::RIM_TYPE_MOUSE {
        return None;
    }
    Some(report.mouse)
}

/// Asks for one `WM_MOUSELEAVE` for this window.
///
/// One-shot: the request is consumed by the notification, so it is re-armed on
/// every derived entry rather than once at creation.
pub(super) fn track_leave(hwnd: Handle) {
    let mut track = TrackMouse {
        cb_size: size_of::<TrackMouse>() as u32,
        dw_flags: value::TME_LEAVE,
        hwnd_track: hwnd,
        dw_hover_time: 0,
    };
    // SAFETY: `track` is a live, initialised `TRACKMOUSEEVENT` whose `cbSize` is
    // its own size — which the system validates — and `hwnd` is a live window of
    // this shell.
    unsafe { ffi::TrackMouseEvent(&raw mut track) };
}

/// A window's client area as a rectangle in screen coordinates.
///
/// `GetClientRect` answers with a zero origin, so both corners have to be mapped
/// through `ClientToScreen`: `ClipCursor` is in screen space and a clip built
/// from the client rectangle unmapped confines the pointer to the top-left of
/// the *desktop*.
pub(super) fn client_screen_rect(hwnd: Handle) -> Option<Rect> {
    let mut rect = Rect::default();
    // SAFETY: `rect` is a live, initialised `RECT` the call writes into.
    if unsafe { ffi::GetClientRect(hwnd, &raw mut rect) } == 0 {
        return None;
    }
    let mut top_left = Point {
        x: rect.left,
        y: rect.top,
    };
    let mut bottom_right = Point {
        x: rect.right,
        y: rect.bottom,
    };
    // SAFETY: both points are live, initialised `POINT`s the calls convert in
    // place, against a live window.
    unsafe {
        if ffi::ClientToScreen(hwnd, &raw mut top_left) == 0
            || ffi::ClientToScreen(hwnd, &raw mut bottom_right) == 0
        {
            return None;
        }
    }
    Some(Rect {
        left: top_left.x,
        top: top_left.y,
        right: bottom_right.x,
        bottom: bottom_right.y,
    })
}

/// Clips the cursor to a window's client area.
pub(super) fn apply_clip(hwnd: Handle) {
    let Some(rect) = client_screen_rect(hwnd) else {
        return;
    };
    // SAFETY: `rect` is a live, initialised `RECT` the call reads and does not
    // retain.
    unsafe { ffi::ClipCursor(&raw const rect) };
}

/// Releases the cursor clip, whoever set it.
///
/// A null rectangle is `ClipCursor`'s documented "unclip", and it is a no-op
/// when nothing is clipped — so this is safe to call unconditionally, which is
/// what the paths that must not think about it depend on.
pub(super) fn release_clip() {
    // SAFETY: a null rectangle is the documented argument for releasing the
    // clip; the call reads no memory of ours.
    unsafe { ffi::ClipCursor(ptr::null()) };
}

/// Re-establishes the clip for a window that has just moved or been resized.
///
/// Called from the window procedure, which is why it takes [`Shared`] rather
/// than the shell: the target is recorded there precisely so that the geometry
/// messages can act on it without the shell being reachable.
pub(super) fn reclip(shared: &Shared, hwnd: Handle, window: isize) {
    if shared.clipped() != window {
        return;
    }
    apply_clip(hwnd);
}

/// The extent an absolute `RAWMOUSE` coordinate is normalized over.
///
/// Two different rectangles depending on the report's flags — see
/// [`pointer::RawMotion`]. A zero from `GetSystemMetrics` (no display attached)
/// becomes one, so the division that follows cannot be by zero.
fn absolute_screen(flags: u16) -> (i32, i32) {
    let (width_index, height_index) = if RawMotion::is_virtual_desktop(flags) {
        (value::SM_CX_VIRTUAL_SCREEN, value::SM_CY_VIRTUAL_SCREEN)
    } else {
        (value::SM_CX_SCREEN, value::SM_CY_SCREEN)
    };
    // SAFETY: `GetSystemMetrics` takes an index by value and reads no memory of
    // ours; an unknown index answers zero rather than failing.
    let (width, height) = unsafe {
        (
            ffi::GetSystemMetrics(width_index),
            ffi::GetSystemMetrics(height_index),
        )
    };
    (width.max(1), height.max(1))
}

impl Win32Shell {
    /// The modifiers in effect for the message being translated.
    ///
    /// `GetKeyboardState` rather than `GetAsyncKeyState`: the former is the
    /// snapshot that belongs to the message currently being processed, and the
    /// latter reads the hardware *now*. The difference shows up exactly when it
    /// matters — a batch of events drained after the user let go of Shift would
    /// every one of them report Shift as up.
    pub(super) fn modifiers_now() -> Modifiers {
        let mut state = [0u8; 256];
        // SAFETY: `state` is a live, initialised array of exactly the 256 bytes
        // `GetKeyboardState` documents that it writes.
        if unsafe { ffi::GetKeyboardState(state.as_mut_ptr()) } == 0 {
            return Modifiers::empty();
        }
        keys::modifiers(&state)
    }

    /// The symbol a key produces in the current layout.
    ///
    /// Two sources, and the split is [`keys::named_keysym`]'s: a key whose
    /// symbol cannot move with the layout answers from the table, and everything
    /// else is asked of the layout through `MapVirtualKeyW`.
    ///
    /// `MAPVK_VK_TO_CHAR` and **not** `ToUnicode`, which is the trap here.
    /// `ToUnicode` consumes dead-key state: calling it to label a key would eat
    /// the pending accent, so the `WM_CHAR` that was about to deliver `é` would
    /// deliver `e`. The mapping call has no such side effect. It answers the
    /// *unshifted* character, which is what a rebind menu and a shortcut label
    /// both want — `Ctrl+/` should read as `/` however the layout reaches it.
    fn keysym_of(key_code: Option<KeyCode>, virtual_key: u16) -> Keysym {
        if let Some(named) = key_code.and_then(keys::named_keysym) {
            return named;
        }
        // SAFETY: `MapVirtualKeyW` takes two integers by value and reads no
        // memory of ours.
        let mapped =
            unsafe { ffi::MapVirtualKeyW(u32::from(virtual_key), value::MAPVK_VK_TO_CHAR) };
        if mapped == 0 {
            // No translation — a key that produces no character in this layout.
            return Keysym::NONE;
        }
        // The top bit marks a dead key, whose *own* symbol is the diacritic and
        // not the character it will compose into. Masking it off is what the
        // documentation asks for.
        let Some(character) = char::from_u32(mapped & 0x7FFF_FFFF) else {
            return Keysym::NONE;
        };
        Keysym::from_char(unshifted(character))
    }

    /// Turns one recorded input message into the events it produced.
    ///
    /// Split from [`translate`](Win32Shell) so the window-lifecycle half of that
    /// function stays readable; the two halves have no shared state beyond the
    /// window lookup the caller has already done.
    pub(super) fn translate_input(&mut self, event: RawEvent, window: Option<WindowId>) {
        let Some(window) = window else { return };
        match event {
            RawEvent::Key {
                scancode,
                virtual_key,
                state,
                repeat,
                millis,
                ..
            } => {
                let time = self.event_time(millis);
                let key_code = keys::key_code(scancode);
                self.queue_event(ShellEvent::Key {
                    window,
                    device: KEYBOARD_DEVICE,
                    time,
                    scancode: Scancode(scancode),
                    key_code,
                    keysym: Self::keysym_of(key_code, virtual_key),
                    state,
                    repeat,
                    modifiers: Self::modifiers_now(),
                });
            }

            RawEvent::Char { unit, millis, .. } => {
                let time = self.event_time(millis);
                let Some(character) = self.text.push(unit) else {
                    // The high half of a surrogate pair, or a code unit that is
                    // not a character at all. Neither is text yet.
                    return;
                };
                if !keys::is_text(character) {
                    return;
                }
                self.queue_event(ShellEvent::TextCommit {
                    window,
                    time,
                    text: character.to_string(),
                });
            }

            RawEvent::PointerMotion { x, y, millis, .. } => {
                let time = self.event_time(millis);
                if self.pointer_mode_of(window) == PointerMode::Locked {
                    // A locked pointer has no meaningful absolute position —
                    // `PointerMotion` documents `abs: None` as the invariant of
                    // the mode — and the delta comes from raw input instead.
                    // Core motion is still worth having: it is what tells us the
                    // pointer has drifted towards an edge.
                    self.recentre_if_near_edge(window, x, y);
                    return;
                }
                self.queue_event(ShellEvent::PointerMotion {
                    window,
                    device: POINTER_DEVICE,
                    time,
                    abs: Some(PhysicalPoint::new(f64::from(x), f64::from(y))),
                    // `WM_MOUSEMOVE` is accelerated and clipped, so it is never
                    // `raw_delta`; `WM_INPUT` delivers that separately.
                    raw_delta: None,
                });
            }

            RawEvent::PointerFocus {
                entered,
                x,
                y,
                millis,
                ..
            } => {
                let time = self.event_time(millis);
                self.queue_event(ShellEvent::PointerFocus {
                    window,
                    device: POINTER_DEVICE,
                    time,
                    entered,
                    position: entered.then(|| PhysicalPoint::new(f64::from(x), f64::from(y))),
                });
            }

            RawEvent::Button {
                button,
                state,
                x,
                y,
                millis,
                ..
            } => {
                let time = self.event_time(millis);
                self.queue_event(ShellEvent::Button {
                    window,
                    device: POINTER_DEVICE,
                    time,
                    button,
                    state,
                    position: self.position_or_none(window, x, y),
                    modifiers: Self::modifiers_now(),
                });
            }

            RawEvent::Wheel {
                horizontal,
                ticks,
                x,
                y,
                millis,
                ..
            } => {
                let time = self.event_time(millis);
                self.queue_event(ShellEvent::Wheel {
                    window,
                    device: POINTER_DEVICE,
                    time,
                    delta: pointer::wheel(horizontal, ticks),
                    position: self.position_or_none(window, x, y),
                    modifiers: Self::modifiers_now(),
                });
            }

            RawEvent::RawMotion {
                flags,
                x,
                y,
                millis,
                ..
            } => {
                let time = self.event_time(millis);
                let screen = absolute_screen(flags);
                let Some(delta) = self.raw_motion.delta(flags, x, y, screen) else {
                    return;
                };
                let locked = self.pointer_mode_of(window) == PointerMode::Locked;
                self.queue_event(ShellEvent::PointerMotion {
                    window,
                    device: POINTER_DEVICE,
                    time,
                    abs: if locked {
                        None
                    } else {
                        self.pointer_position(window)
                    },
                    raw_delta: Some(delta),
                });
            }

            // The window-lifecycle half, which `translate` has already handled.
            RawEvent::Resized { .. }
            | RawEvent::Minimized { .. }
            | RawEvent::DpiChanged { .. }
            | RawEvent::Focus { .. }
            | RawEvent::CloseRequested { .. }
            | RawEvent::Destroyed { .. }
            | RawEvent::FilesDropped { .. }
            | RawEvent::MonitorsChanged => {}
        }
    }

    /// A click's position, suppressed while the pointer is locked.
    fn position_or_none(&self, window: WindowId, x: i32, y: i32) -> Option<PhysicalPoint> {
        if self.pointer_mode_of(window) == PointerMode::Locked {
            return None;
        }
        Some(PhysicalPoint::new(f64::from(x), f64::from(y)))
    }

    /// Where the pointer is inside a window, by asking the system.
    ///
    /// Needed because a `WM_INPUT` carries no position at all — it is a device
    /// report, not a window event — and an unlocked consumer still wants `abs`
    /// alongside the delta.
    fn pointer_position(&self, window: WindowId) -> Option<PhysicalPoint> {
        let hwnd = self.window(window).ok()?.raw();
        let mut point = Point::default();
        // SAFETY: `point` is a live, initialised `POINT` the call writes into.
        if unsafe { ffi::GetCursorPos(&raw mut point) } == 0 {
            return None;
        }
        // SAFETY: as above, converted in place against a live window.
        if unsafe { ffi::ScreenToClient(hwnd, &raw mut point) } == 0 {
            return None;
        }
        Some(PhysicalPoint::new(f64::from(point.x), f64::from(point.y)))
    }

    /// The pointer mode a window is in, or [`Free`](PointerMode::Free) for a
    /// handle that has gone stale mid-batch.
    fn pointer_mode_of(&self, window: WindowId) -> PointerMode {
        self.window(window)
            .map_or(PointerMode::Free, |state| state.pointer_mode)
    }

    /// Warps a locked pointer back to the middle when it nears an edge.
    ///
    /// See the [module docs](self): the clip stops the pointer leaving, and this
    /// stops it coming to rest against an edge. A margin rather than every
    /// event, so the warp does not double the event rate and fight the user in
    /// the frame between the move and its notification.
    fn recentre_if_near_edge(&mut self, window: WindowId, x: i32, y: i32) {
        let Some(size) = self.client_size(window) else {
            return;
        };
        if size.is_empty() {
            return;
        }
        let centre_x = i32::try_from(size.width / 2).unwrap_or(0);
        let centre_y = i32::try_from(size.height / 2).unwrap_or(0);
        let margin_x = (f64::from(size.width) * LOCK_RECENTRE_FRACTION) as i32;
        let margin_y = (f64::from(size.height) * LOCK_RECENTRE_FRACTION) as i32;
        if (x - centre_x).abs() < margin_x && (y - centre_y).abs() < margin_y {
            return;
        }
        // Logged rather than propagated: this runs inside event handling, where
        // there is no caller to hand a `Result` to, and a recentre that did not
        // happen costs the user a pointer resting against an edge rather than a
        // broken frame. Silence was the old behaviour and is what made the same
        // refusal invisible in `warp_to_client`.
        if let Err(why) = self.warp_to_client(window, centre_x, centre_y) {
            crcbl_core::log::warn!("the locked pointer was not recentred: {why}");
        }
    }

    /// The window's client area as the system has it, for the recentre.
    fn client_size(&self, window: WindowId) -> Option<PhysicalSize> {
        let hwnd = self.window(window).ok()?.raw();
        Some(Self::client_size_of(hwnd))
    }

    /// Moves the pointer to a client-space position.
    ///
    /// `ClientToScreen` then `SetCursorPos`: the seam speaks in window pixels
    /// and `SetCursorPos` is in screen ones, and skipping the conversion warps
    /// the pointer to the same offset from the *desktop* corner.
    ///
    /// # `SetCursorPos` fails when this process is not in the foreground
    ///
    /// Windows refuses a cursor move from a background process, and the refusal
    /// is the `BOOL` rather than an error the caller trips over. **All three
    /// failures here used to be discarded and this returned `()`**, so a warp
    /// that moved nothing reported success: the pointer stayed where it was and
    /// the mismatch surfaced much later, as a coordinate that disagreed with
    /// what was asked for by exactly the offset requested.
    ///
    /// # Errors
    ///
    /// [`ShellError::InvalidWindow`] if `window` is not this shell's, and
    /// [`ShellError::Backend`] if either system call refuses.
    pub(super) fn warp_to_client(
        &self,
        window: WindowId,
        x: i32,
        y: i32,
    ) -> Result<(), ShellError> {
        let hwnd = self.window(window)?.raw();
        let mut point = Point { x, y };
        // SAFETY: `point` is a live, initialised `POINT` the call converts in
        // place, against a live window of this shell.
        if unsafe { ffi::ClientToScreen(hwnd, &raw mut point) } == 0 {
            return Err(ShellError::Backend(format!(
                "ClientToScreen refused the conversion of ({x}, {y}) for window {window:?}"
            )));
        }
        // SAFETY: two integers by value; the system clamps the position to the
        // current clip and to the virtual screen.
        if unsafe { ffi::SetCursorPos(point.x, point.y) } == 0 {
            return Err(ShellError::Backend(format!(
                "SetCursorPos refused ({}, {}) — the usual cause is that this \
                 process is not in the foreground, which Windows requires of a \
                 caller that moves the cursor",
                point.x, point.y
            )));
        }
        Ok(())
    }

    /// Applies or releases the clip for whichever window currently wants one.
    ///
    /// The shell's half of the clip: the window procedure maintains it across
    /// moves and focus changes, and this is what establishes it in the first
    /// place and tears it down when the mode goes back to
    /// [`Free`](PointerMode::Free).
    ///
    /// # The target is recorded even while the clip is not in effect
    ///
    /// A window that wants the pointer confined and does not have the keyboard
    /// gets **no clip** — that is the hostage rule, and it holds whether the
    /// focus was lost or never arrived. What it does get is the recorded target,
    /// because that is what the window procedure re-applies from when focus
    /// arrives. Clearing the record on focus loss instead would make a mode set
    /// before the window was focused never take effect, and would make one lost
    /// alt-tab silently end a game's mouselook.
    pub(super) fn refresh_clip(&mut self) {
        let wanted = self
            .windows_iter()
            .find(|(_, state)| state.pointer_mode.is_captured())
            .map(|(_, state)| (state.raw(), state.key, state.focused));
        match wanted {
            Some((hwnd, key, focused)) => {
                self.shared().set_clipped(key);
                if focused {
                    apply_clip(hwnd);
                } else {
                    release_clip();
                }
            }
            None => {
                self.shared().set_clipped(0);
                release_clip();
            }
        }
    }

    /// Hides or reveals the cursor, with the reference count kept balanced.
    ///
    /// See the [module docs](self) for the rule this implements and for why
    /// `ShowCursor`'s count is the part that has to be got right rather than the
    /// decision itself.
    pub(super) fn refresh_cursor_visibility(&mut self) {
        let hidden = self.windows_iter().any(|(_, state)| {
            state.focused && (state.pointer_mode == PointerMode::Locked || state.cursor_hidden())
        });
        Self::show_cursor(self.visibility.want(hidden));
    }

    /// Issues the `ShowCursor` call a [`Show`] asks for, and no other.
    pub(super) fn show_cursor(show: Show) {
        let argument = match show {
            Show::Nothing => return,
            Show::Hide => 0,
            Show::Reveal => 1,
        };
        // SAFETY: one integer by value. The call adjusts this thread's cursor
        // display count and cannot fail.
        unsafe { ffi::ShowCursor(argument) };
    }

    /// Loads the stock cursor for a shape and records it for `WM_SETCURSOR`.
    ///
    /// # Errors
    ///
    /// [`ShellError::InvalidWindow`] if the handle is stale.
    pub(super) fn apply_cursor_shape(
        &mut self,
        window: WindowId,
        icon: CursorIcon,
    ) -> Result<(), ShellError> {
        let key = self.window(window)?.key;
        // SAFETY: a null instance with an integer resource id asks for a system
        // cursor, which is the documented use of the `IDC_*` values. The handle
        // is owned by the system, shared across the process and must not be
        // destroyed.
        let cursor = unsafe { ffi::LoadCursorW(ptr::null_mut(), pointer::cursor_id(icon)) };
        if cursor.is_null() {
            // Every `IDC_*` this backend names has shipped since Windows 95, so
            // this is unreachable short of a corrupted `user32`. Leaving the
            // previous shape recorded is better than recording a null one, which
            // `WM_SETCURSOR` would set as "no cursor".
            crcbl_core::log::warn!(
                "LoadCursorW returned null for {icon:?}; the cursor shape is unchanged"
            );
            return Ok(());
        }
        self.shared().set_shape(Shape { hwnd: key, cursor });
        Ok(())
    }
}
