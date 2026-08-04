//! Where AppKit input meets the shell: the translation of what the view
//! recorded, the pointer modes, the cursor, and the warp.
//!
//! The arithmetic is in [`keys`](super::keys) and [`pointer`](super::pointer) and
//! is tested on any host; this module is what cannot be — every function here
//! either sends a message or needs the shell's `&mut self`.
//!
//! # Decision: locked is a frozen cursor, and there is nothing to recentre
//!
//! Win32 implements [`PointerMode::Locked`] as a clip plus a recentre, and X11 as
//! a grab plus a recentre, because on both platforms a bounded pointer still
//! *moves* and eventually comes to rest against an edge. macOS has a mechanism
//! neither of them has: `CGAssociateMouseAndMouseCursorPosition(false)`
//! disconnects the hardware from the cursor's position outright — the cursor
//! stops where it is, the deltas keep arriving, and there is no drift to correct.
//!
//! So this backend's lock is three calls and no policy: warp the cursor to the
//! middle of the window so that unlocking leaves it somewhere findable,
//! disconnect, and hide. No margin, no fraction of the window, no fighting the
//! user in the frame between a move and its notification. It is the one place
//! macOS is simpler than both other desktop platforms rather than harder.
//!
//! # Decision: `Confined` is refused, because macOS cannot do it
//!
//! There is no `ClipCursor`, no `XGrabPointer(confine_to)` and no
//! `zwp_confined_pointer_v1`. The techniques people reach for are all the same
//! technique — warp the cursor back whenever it crosses a boundary — and it is
//! not confinement: it runs a frame late, so the pointer visibly leaves the
//! window and snaps back, it fights the user's own motion at the edge, and it
//! generates a motion event per correction that a consumer cannot tell from a
//! real one.
//!
//! [`POINTER_CONFINE`](crate::ShellCaps::POINTER_CONFINE) is therefore **clear**
//! and [`set_pointer_mode`](crate::Shell::set_pointer_mode) refuses
//! [`Confined`](PointerMode::Confined) by name. macOS is the only desktop
//! backend of the four where that bit is clear, and it is a fact about the
//! platform rather than a gap here. A strategy game that wants edge scrolling on
//! this platform reads [`PointerFocus`](crate::ShellEvent::PointerFocus) and the
//! pointer position, which is what the mode would have been an optimisation of.
//!
//! # Decision: the freeze is released the moment focus goes
//!
//! `CGAssociateMouseAndMouseCursorPosition` is **process-wide and desktop-wide**:
//! a disconnected mouse is a cursor that does not move for *any* application. A
//! shell that kept the lock while its window was in the background would take
//! the whole desktop hostage, and a shell that was dropped while locked would
//! leave it that way until the user logged out.
//!
//! That is the same hazard the Win32 backend documents for its cursor clip, one
//! size larger, and it gets the same answer: the capture is derived from whether
//! a **focused** window wants it, re-derived on every focus change, and handed
//! back in [`Drop`](super::AppKitShell). The cursor's hidden count is released
//! with it.

use crcbl_core::input::{ButtonState, DeviceId, Keysym, Scancode};
use crcbl_core::{EventTime, KeyCode};

use crate::{
    CursorIcon, PhysicalPoint, PointerMode, ShellError, ShellEvent, WindowId,
    appkit::events::RawEvent,
};

use super::ffi::{self, Id, NSRect, NSSize, Sel};
use super::geometry::Flip;
use super::keys;
use super::pointer::{self, Show};
use super::shell::AppKitShell;

/// Which keyboard and pointer an event came from.
///
/// Constants rather than a table, and the same admission the X11 and Win32
/// backends make: [`DeviceId`] exists so the P2 action layer can tell two mice
/// apart, and on this backend it cannot yet. An `NSEvent` does carry a
/// `deviceID`, but it identifies a *tablet* rather than a mouse and is
/// documented as meaningful only for tablet-family events; the real per-device
/// answer on this platform is IOKit, which is a slice of its own.
/// `docs/backlog.md` carries it.
pub(super) const KEYBOARD_DEVICE: DeviceId = DeviceId(1);
/// See [`KEYBOARD_DEVICE`].
pub(super) const POINTER_DEVICE: DeviceId = DeviceId(2);

/// The clock `-[NSEvent timestamp]` is measured against, read now.
///
/// A free function rather than a method so that [`TimeBase`](super::TimeBase)
/// stays a pure value that a host test can drive — see its own docs.
#[must_use]
pub(super) fn now_seconds() -> f64 {
    // SAFETY: takes nothing, returns a `CFTimeInterval` by value, and is
    // documented thread-safe.
    unsafe { ffi::CACurrentMediaTime() }
}

impl AppKitShell {
    /// An event's timestamp on the engine's clock.
    ///
    /// The clock is read *now* rather than cached, because a synthesized event
    /// carries no timestamp and is stamped with the current time instead — a
    /// stale reading would put every one of them at the same instant.
    pub(super) fn event_time(&self, seconds: f64) -> EventTime {
        self.time.event_time_at(now_seconds(), seconds)
    }

    /// The pointer mode a window is in, or [`Free`](PointerMode::Free) for a
    /// handle that has gone stale mid-batch.
    fn pointer_mode_of(&self, window: WindowId) -> PointerMode {
        self.window(window)
            .map_or(PointerMode::Free, |state| state.pointer_mode)
    }

    /// A window's content view height in points and its backing scale, which is
    /// everything the coordinate conversions need.
    ///
    /// `None` for a stale handle. Read from the view rather than remembered:
    /// the height is what a position is reflected about, and a remembered one
    /// that missed a resize would put every click the wrong distance from the
    /// bottom.
    fn view_metrics(&self, window: WindowId) -> Option<(f64, f64)> {
        let state = self.window(window).ok()?;
        // SAFETY: a live view of a live window, on the main thread.
        let bounds = unsafe { ffi::msg_rect(state.view, ffi::sel(c"bounds")) };
        Some((bounds.size.height, state.scale))
    }

    /// Where a recorded event happened, in the window's pixels.
    ///
    /// `None` while the pointer is locked, which is the invariant
    /// [`PointerMotion`](ShellEvent::PointerMotion) and
    /// [`Button`](ShellEvent::Button) both document: a frozen cursor has no
    /// meaningful position, and reporting the one it froze at would look correct
    /// until the day something differenced two of them.
    fn position_of(&self, window: WindowId, location: ffi::NSPoint) -> Option<PhysicalPoint> {
        if self.pointer_mode_of(window) == PointerMode::Locked {
            return None;
        }
        let (height, scale) = self.view_metrics(window)?;
        Some(pointer::view_point(location, height, scale))
    }

    /// Turns one recorded input event into the events it produced.
    ///
    /// Split from [`translate`](AppKitShell) so the window-lifecycle half of that
    /// function stays readable; the two halves share nothing beyond the window
    /// lookup the caller has already done.
    pub(super) fn translate_input(&mut self, event: RawEvent, window: Option<WindowId>) {
        let Some(window) = window else { return };
        match event {
            RawEvent::Key {
                keycode,
                character,
                flags,
                state,
                repeat,
                seconds,
                ..
            } => {
                let time = self.event_time(seconds);
                let key_code = keys::key_code(keycode);
                self.queue_event(ShellEvent::Key {
                    window,
                    device: KEYBOARD_DEVICE,
                    time,
                    scancode: Scancode(u32::from(keycode)),
                    key_code,
                    keysym: keysym_of(key_code, character),
                    state,
                    repeat,
                    modifiers: keys::modifiers(flags),
                });
            }

            RawEvent::FlagsChanged {
                keycode,
                flags,
                seconds,
                ..
            } => {
                // A modifier key, and the flags are the only thing that says
                // which way it went. A `flagsChanged:` for a key this engine has
                // no name for — the `fn` key, most often — is dropped rather
                // than reported as an edge nobody can attribute.
                let Some(key_code) = keys::key_code(keycode) else {
                    return;
                };
                let Some(down) = keys::modifier_is_down(key_code, flags) else {
                    return;
                };
                let time = self.event_time(seconds);
                self.queue_event(ShellEvent::Key {
                    window,
                    device: KEYBOARD_DEVICE,
                    time,
                    scancode: Scancode(u32::from(keycode)),
                    key_code: Some(key_code),
                    keysym: keys::named_keysym(key_code).unwrap_or(Keysym::NONE),
                    state: ButtonState::from_pressed(down),
                    // A held modifier does not repeat on this platform: the
                    // flags stop changing, so no further event arrives.
                    repeat: false,
                    modifiers: keys::modifiers(flags),
                });
            }

            RawEvent::TextCommitted {
                window: key,
                seconds,
            } => {
                let time = self.event_time(seconds);
                // The string the input method committed, claimed by this
                // marker. Absent only if the window died between the two, which
                // `Shared::forget` is what makes possible.
                let Some(text) = self.shared_text(key) else {
                    return;
                };
                self.queue_event(ShellEvent::TextCommit { window, time, text });
            }

            RawEvent::PointerMotion {
                location,
                dx,
                dy,
                seconds,
                ..
            } => {
                let time = self.event_time(seconds);
                self.queue_event(ShellEvent::PointerMotion {
                    window,
                    device: POINTER_DEVICE,
                    time,
                    abs: self.position_of(window, location),
                    // Quartz's own delta, in Quartz's own space — which is
                    // already the seam's, so it crosses unflipped while the
                    // position beside it is reflected. See `pointer`.
                    raw_delta: Some((dx, dy)),
                });
            }

            RawEvent::PointerFocus {
                entered,
                location,
                seconds,
                ..
            } => {
                let time = self.event_time(seconds);
                let position = if entered {
                    self.position_of(window, location)
                } else {
                    None
                };
                self.queue_event(ShellEvent::PointerFocus {
                    window,
                    device: POINTER_DEVICE,
                    time,
                    entered,
                    position,
                });
            }

            RawEvent::Button {
                number,
                state,
                location,
                flags,
                seconds,
                ..
            } => {
                let time = self.event_time(seconds);
                self.queue_event(ShellEvent::Button {
                    window,
                    device: POINTER_DEVICE,
                    time,
                    button: pointer::button(number),
                    state,
                    position: self.position_of(window, location),
                    modifiers: keys::modifiers(flags),
                });
            }

            RawEvent::FilesDropped {
                window: key,
                location,
                seconds,
            } => {
                // **Taken before anything can return.** The payload and its
                // marker are one drop; leaving the payload behind when the
                // marker is discarded would hand it to the next marker for this
                // window, which is a scene importing the assets dropped on the
                // previous one.
                let Some(paths) = self.shared_drop(key) else {
                    return;
                };
                if !self.window(window).is_ok_and(|state| state.accept_drops) {
                    // AppKit sends no dragging message to a view that has not
                    // registered, so this is the case the system cannot cover:
                    // something outside this backend registered our view. The
                    // descriptor said no; that is the answer.
                    log::debug!(
                        "discarding a drop of {} file(s) on a window created without accept_drops",
                        paths.len()
                    );
                    return;
                }
                let time = self.event_time(seconds);
                // The drop's own position, converted like every other one.
                // Deliberately not through `position_of`, which answers `None`
                // for a locked pointer: a drag cannot happen against a frozen
                // cursor, and a drop whose position went missing would be one a
                // consumer could not place in its scene.
                let position = self
                    .view_metrics(window)
                    .map(|(height, scale)| pointer::view_point(location, height, scale));
                // One event per file, which is what `DroppedFile` documents: a
                // multi-file drop is several.
                for path in paths {
                    self.queue_event(ShellEvent::DroppedFile {
                        window,
                        time,
                        path,
                        position,
                    });
                }
            }

            RawEvent::Scroll {
                dx,
                dy,
                precise,
                location,
                flags,
                seconds,
                ..
            } => {
                let time = self.event_time(seconds);
                self.queue_event(ShellEvent::Wheel {
                    window,
                    device: POINTER_DEVICE,
                    time,
                    delta: pointer::scroll(precise, dx, dy),
                    position: self.position_of(window, location),
                    modifiers: keys::modifiers(flags),
                });
            }

            // The window-lifecycle half, which `translate` has already handled.
            RawEvent::Resized { .. }
            | RawEvent::BackingChanged { .. }
            | RawEvent::Focus { .. }
            | RawEvent::CloseRequested { .. }
            | RawEvent::Closed { .. }
            | RawEvent::ScreensChanged => {}
        }
    }

    /// Connects or disconnects the mouse from the cursor, for whichever window
    /// currently wants it.
    ///
    /// Derived from the **focused** window rather than from any window, for the
    /// reason the module docs give: the disconnection is desktop-wide, so a
    /// background process holding it has taken the pointer away from everybody.
    /// Written only when it changes, because the call is a state transition
    /// rather than an idempotent assignment.
    pub(super) fn refresh_pointer_capture(&mut self) {
        let wanted = self
            .windows_iter()
            .any(|(_, state)| state.focused && state.pointer_mode == PointerMode::Locked);
        if wanted == self.pointer_frozen {
            return;
        }
        self.pointer_frozen = wanted;
        // SAFETY: takes an `int` by value and returns a `CGError`; nothing of
        // ours is read.
        let error = unsafe { ffi::CGAssociateMouseAndMouseCursorPosition(i32::from(!wanted)) };
        if error != 0 {
            // Not fatal and not silent: the mode is still recorded, the camera
            // still reads deltas, and what the player sees is a cursor that
            // keeps moving. Saying so is the only thing that can be done here.
            log::warn!(
                "CGAssociateMouseAndMouseCursorPosition({}) failed with CGError {error}; the \
                 pointer will keep moving while it is locked",
                !wanted
            );
            self.pointer_frozen = !wanted;
        }
    }

    /// Hides or reveals the cursor, with the hide count kept balanced.
    ///
    /// The rule is the Win32 backend's, and it is the same rule for the same
    /// reason: the cursor is hidden while the window that has the **keyboard
    /// focus** wants it hidden, either by [`PointerMode::Locked`] or by an
    /// explicit [`set_cursor(None)`](crate::Shell::set_cursor). Keying on focus
    /// is what makes alt-tabbing away from a game in mouselook give the desktop
    /// its pointer back.
    pub(super) fn refresh_cursor_visibility(&mut self) {
        let hidden = self.windows_iter().any(|(_, state)| {
            state.focused && (state.pointer_mode == PointerMode::Locked || state.cursor_hidden())
        });
        Self::show_cursor(self.visibility.want(hidden));
    }

    /// Issues the `NSCursor` call a [`Show`] asks for, and no other.
    pub(super) fn show_cursor(show: Show) {
        let selector = match show {
            Show::Nothing => return,
            Show::Hide => c"hide",
            Show::Reveal => c"unhide",
        };
        let Some(class) = ffi::class(c"NSCursor") else {
            return;
        };
        // SAFETY: both are `NSCursor` class methods taking nothing and
        // returning nothing.
        unsafe { ffi::msg_void(class, ffi::sel(selector)) };
    }

    /// Records the `NSCursor` for a shape and applies it now.
    ///
    /// Both halves are needed and neither is enough. The record is what
    /// `cursorUpdate:` reads — see [`view`](super::view), which is the only
    /// place a shape survives the next pointer movement — and the `set` is what
    /// changes the cursor the player is looking at *this* frame, before anything
    /// has moved.
    pub(super) fn apply_cursor_shape(&mut self, window: WindowId, icon: CursorIcon) {
        let Ok(state) = self.window(window) else {
            return;
        };
        let key = state.key;
        let Some(class) = ffi::class(c"NSCursor") else {
            return;
        };
        // SAFETY: every selector `cursor_selector` returns is a public
        // `NSCursor` class method taking nothing and returning the shared
        // cursor object, which AppKit owns for the life of the process.
        let cursor = unsafe { ffi::msg(class, ffi::sel(pointer::cursor_selector(icon))) };
        if cursor.is_null() {
            log::warn!("+[NSCursor {icon:?}] returned nil; the cursor shape is unchanged");
            return;
        }
        self.record_cursor(key, cursor as usize);
        // SAFETY: a live cursor object.
        unsafe { ffi::msg_void(cursor, ffi::sel(c"set")) };
    }

    /// Moves the cursor to a position in a window.
    ///
    /// **Two reflections**, which is one more than any other backend's warp
    /// needs: [`pointer::warp_source`] undoes the view's own Y flip to reach
    /// AppKit's window space, `convertRectToScreen:` moves that into AppKit's
    /// screen space, and [`Flip::point`] turns *that* into the top-left-origin
    /// space CoreGraphics warps in. Missing either one puts the cursor the same
    /// distance on the wrong side of the middle — which looks like a working
    /// warp for as long as the target happens to be centred.
    ///
    /// `convertRectToScreen:` rather than `convertPointToScreen:`: the second
    /// arrived in 10.12 and the first has been there since 10.7, and a zero-size
    /// rectangle carries a point perfectly well.
    ///
    /// # Errors
    ///
    /// [`ShellError::InvalidWindow`] for a stale handle, or
    /// [`ShellError::Backend`] if there is no display to express a global
    /// position in — a Mac with nothing plugged in.
    pub(super) fn warp_to_window(
        &self,
        window: WindowId,
        position: PhysicalPoint,
    ) -> Result<(), ShellError> {
        let state = self.window(window)?;
        let native = state.window;
        let (height, scale) = self
            .view_metrics(window)
            .ok_or_else(|| ShellError::invalid_window(window))?;
        let flip = self
            .screens
            .first()
            .map(|screen| Flip::about_primary(screen.frame))
            .ok_or_else(|| {
                ShellError::Backend(
                    "there is no display to express a pointer position on".to_string(),
                )
            })?;

        let local = pointer::warp_source(position, height, scale);
        // SAFETY: `convertRectToScreen:` takes and returns an `NSRect`, which is
        // the one shape that goes through the struct-return trampoline on
        // x86_64.
        let screen = unsafe {
            let convert: unsafe extern "C" fn(Id, Sel, NSRect) -> NSRect = ffi::msg_send_stret();
            convert(
                native,
                ffi::sel(c"convertRectToScreen:"),
                NSRect {
                    origin: local,
                    size: NSSize::new(0.0, 0.0),
                },
            )
        };
        // SAFETY: takes a `CGPoint` by value and returns a `CGError`.
        let error = unsafe { ffi::CGWarpMouseCursorPosition(flip.point(screen.origin)) };
        if error != 0 {
            return Err(ShellError::Backend(format!(
                "CGWarpMouseCursorPosition failed with CGError {error}"
            )));
        }
        Ok(())
    }

    /// Puts the cursor in the middle of a window, which is where a lock starts.
    ///
    /// Best effort: a warp that fails leaves the cursor where it was and the
    /// lock that follows still works — the only cost is that unlocking returns
    /// the cursor to wherever the player left it rather than to the middle. It
    /// has to happen **before** the disconnection, because a warp against a
    /// disconnected mouse moves a cursor nothing is going to redraw.
    pub(super) fn centre_pointer(&self, window: WindowId) {
        let Ok(state) = self.window(window) else {
            return;
        };
        let scale = state.scale;
        // SAFETY: a live view of a live window, on the main thread.
        let bounds = unsafe { ffi::msg_rect(state.view, ffi::sel(c"bounds")) };
        if let Err(error) = self.warp_to_window(window, pointer::centre_of(bounds.size, scale)) {
            log::debug!("could not centre the pointer before locking it: {error}");
        }
    }
}

/// The symbol a key produces, from the two sources that can answer.
///
/// The split is [`keys::named_keysym`]'s: a key whose symbol cannot move with
/// the layout answers from the table, and everything else answers from the
/// character the event carried — which is what `charactersIgnoringModifiers`
/// is, read in the callback because the event does not outlive it. A key with
/// neither, a media key on an external keyboard, is
/// [`Keysym::NONE`](crcbl_core::input::Keysym::NONE): the seam's way of saying
/// this key produces no symbol in this layout.
fn keysym_of(key_code: Option<KeyCode>, character: Option<char>) -> Keysym {
    if let Some(named) = key_code.and_then(keys::named_keysym) {
        return named;
    }
    character.map_or(Keysym::NONE, keys::keysym_from_character)
}

/// What a real Mac can be asked from a `libtest` thread.
///
/// Almost nothing in this module can be: it needs `&mut self` on a shell, and a
/// shell cannot exist on a spawned thread — see [`shell`](super::shell)'s own
/// test module for that measurement. **Two things can**, because they are
/// CoreGraphics rather than AppKit, and CoreGraphics is thread-safe and needs
/// no application: the warp and the freeze. They are also the two calls in this
/// slice whose failure would be silent, so they are the two worth reaching.
///
/// # What used to be here, and why it is not
///
/// A third test asserted that every [`CursorIcon`]'s `NSCursor` selector exists
/// and answers a real object. It was green on the host and **failed on the
/// macOS runner** with `+[NSCursor "arrowCursor"] answered nil`: an AppKit
/// object needs `NSApplication` and the main thread, and a `#[test]` supplies
/// neither. It now lives in
/// [`session_support::every_cursor_shape_exists`](super::session_support::every_cursor_shape_exists),
/// where both hold, with the same assertion and the same message. That module
/// states the rule the two failures together establish.
#[cfg(all(test, target_os = "macos"))]
mod tests {
    use super::*;
    use core::ptr;

    /// Where the cursor is, in the global display coordinates a warp takes.
    fn cursor_position() -> Option<ffi::NSPoint> {
        // SAFETY: a null source asks for the current input state and returns a
        // CF object this function owns; `CFRelease` is the matching release and
        // the event is not used afterwards.
        unsafe {
            let event = ffi::CGEventCreate(ptr::null_mut());
            if event.is_null() {
                return None;
            }
            let at = ffi::CGEventGetLocation(event);
            ffi::CFRelease(event);
            Some(at)
        }
    }

    /// Moves the cursor and reports the CGError.
    fn warp(to: ffi::NSPoint) -> i32 {
        // SAFETY: takes a `CGPoint` by value and returns a `CGError`.
        unsafe { ffi::CGWarpMouseCursorPosition(to) }
    }

    #[test]
    fn a_warp_moves_the_cursor_to_the_point_it_names() {
        // The mechanism behind `POINTER_WARP`, and the one half of this
        // backend's two-reflection warp that can be checked without a window:
        // that `CGWarpMouseCursorPosition` and `CGEventGetLocation` agree about
        // the space, and that neither introduces a flip of its own.
        //
        // The runner's desktop is real, so the cursor is put back where it was
        // found — and where it was found is printed on failure, because a
        // duration or a bare `false` would say nothing about which half moved.
        let Some(original) = cursor_position() else {
            panic!("CGEventCreate answered nil, so this runner has no input state to read");
        };

        // Two points that differ in Y by a known amount, near the top-left of
        // the main display where every arrangement has room.
        let near = ffi::NSPoint { x: 40.0, y: 30.0 };
        let far = ffi::NSPoint { x: 40.0, y: 130.0 };

        assert_eq!(warp(near), 0, "CGWarpMouseCursorPosition refused {near:?}");
        let at_near = cursor_position().expect("the cursor has a position");
        assert_eq!(warp(far), 0, "CGWarpMouseCursorPosition refused {far:?}");
        let at_far = cursor_position().expect("the cursor has a position");

        // Back first, so a failing assertion below does not leave the runner's
        // cursor in a corner.
        warp(original);

        assert!(
            (at_near.x - near.x).abs() < 1.0 && (at_near.y - near.y).abs() < 1.0,
            "warping to {near:?} left the cursor at {at_near:?}"
        );
        assert!(
            (at_far.x - far.x).abs() < 1.0 && (at_far.y - far.y).abs() < 1.0,
            "warping to {far:?} left the cursor at {at_far:?}"
        );
        // The signed difference, which is what a flip anywhere in the pair would
        // invert. That the origin is the *top* left is CoreGraphics' documented
        // contract and is not something these two calls can falsify between
        // them — it takes a second coordinate source, which is what the window
        // session in `tests/appkit_session.rs` brings.
        assert!(
            at_far.y - at_near.y > 0.0,
            "y increases downwards in Quartz's space: {at_near:?} then {at_far:?}"
        );
    }

    #[test]
    fn disconnecting_the_mouse_is_accepted_and_can_be_given_back() {
        // The mechanism behind `POINTER_LOCK`. It is desktop-wide, so this
        // reconnects immediately and unconditionally — a test that left it
        // disconnected would freeze the runner's cursor for every process on
        // the machine until the job ended.
        // SAFETY: takes an `int` by value and returns a `CGError`.
        let disconnected = unsafe { ffi::CGAssociateMouseAndMouseCursorPosition(0) };
        // SAFETY: as above; reconnecting is always legal, and doing it before
        // the assertion is what keeps a failure from being destructive.
        let reconnected = unsafe { ffi::CGAssociateMouseAndMouseCursorPosition(1) };
        assert_eq!(
            disconnected, 0,
            "CGAssociateMouseAndMouseCursorPosition(false) failed with CGError {disconnected}, \
             so pointer lock cannot work on this runner"
        );
        assert_eq!(reconnected, 0, "and it would not give the cursor back");
    }
}
