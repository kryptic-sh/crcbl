//! The shell's side of `wl_seat`: devices appearing and going, keys and their
//! repeats, pointer frames, and keyboard focus.
//!
//! The decoding of the wire events is in [`super`]'s dispatcher; what lives
//! here is what the shell does with them once they are [`RawEvent`]s.

use core::ptr;
use core::time::Duration;

use crcbl_core::input::{ButtonState, Keysym, Scancode};

use crate::{PhysicalPoint, PointerMode, ShellEvent, WindowId};

use super::protocol::relative_pointer::{zwp_relative_pointer_manager_v1, zwp_relative_pointer_v1};
use super::protocol::wayland::{wl_keyboard, wl_pointer, wl_seat};
use super::{
    ObjectKind, RawEvent, RepeatKeyRequest, WaylandShell, WlWindow, axis_slot, fd, ffi, keymap, xkb,
};

impl WaylandShell {
    /// Adds or removes a seat's devices to match what it now advertises.
    ///
    /// A seat gains and loses capabilities at runtime — plugging in a mouse is
    /// the everyday case, and a headless compositor starts with *neither* a
    /// pointer nor a keyboard — so this is a diff against the previous set, not
    /// a one-time setup.
    pub(super) fn seat_capabilities_changed(&mut self, proxy: usize, capabilities: u32) {
        let Some(index) = self.seat_index(proxy, |seat| seat.proxy as usize) else {
            return;
        };
        let previous = self.seats[index].capabilities;
        self.seats[index].capabilities = capabilities;

        let had_pointer = previous & wl_seat::capability::POINTER != 0;
        let has_pointer = capabilities & wl_seat::capability::POINTER != 0;
        if has_pointer && !had_pointer {
            self.add_pointer(index);
        } else if !has_pointer && had_pointer {
            self.remove_pointer(index);
        }

        let had_keyboard = previous & wl_seat::capability::KEYBOARD != 0;
        let has_keyboard = capabilities & wl_seat::capability::KEYBOARD != 0;
        if has_keyboard && !had_keyboard {
            // SAFETY: the seat proxy is live and advertises the capability, so
            // `get_keyboard` cannot raise `missing_capability`.
            let keyboard = unsafe { wl_seat::get_keyboard(self.seats[index].proxy) };
            self.conn.watch(keyboard, ObjectKind::Keyboard);
            self.seats[index].keyboard = keyboard;
        } else if !has_keyboard && had_keyboard {
            self.remove_keyboard(index);
        }
        self.conn.flush();
    }

    /// Creates the objects that hang off a new `wl_pointer`.
    fn add_pointer(&mut self, index: usize) {
        // SAFETY: the seat proxy is live and advertises the capability.
        let pointer = unsafe { wl_seat::get_pointer(self.seats[index].proxy) };
        self.conn.watch(pointer, ObjectKind::Pointer);
        self.seats[index].pointer = pointer;

        if !self.conn.relative_pointer_manager.is_null() {
            // SAFETY: both proxies are live; `get_relative_pointer` creates the
            // add-on object that carries unaccelerated motion.
            let relative = unsafe {
                zwp_relative_pointer_manager_v1::get_relative_pointer(
                    self.conn.relative_pointer_manager,
                    pointer,
                )
            };
            self.conn.watch(relative, ObjectKind::RelativePointer);
            self.seats[index].relative_pointer = relative;
        }
        self.add_cursor_shape(index);
        // A window that asked to be locked before any pointer existed gets its
        // constraint now. This is the hotplug case that would otherwise leave a
        // first-person camera with a free cursor until the next mode change.
        self.reapply_constraints();
    }

    /// Tears down a seat's pointer and everything that hangs off it.
    fn remove_pointer(&mut self, index: usize) {
        self.flush_pointer_frame(index);
        if let Some(window) = self.seats[index].pointer_focus.take() {
            let device = self.seats[index].device;
            self.queue.push_back(ShellEvent::PointerFocus {
                window,
                device,
                // Synthesized because the device went away, so there is no
                // compositor timestamp at all; see `TimeBase::event_time_now`.
                time: self.time.event_time_now(),
                entered: false,
                position: None,
            });
        }
        // Before the pointer goes: a constraint names it, and the destructor
        // has to reach the compositor while the object it refers to still
        // exists.
        self.drop_window_constraints_for_seat(index);
        // An add-on object of the pointer, like the relative pointer below.
        self.remove_cursor_shape(index);
        let (pointer, relative) = (
            self.seats[index].pointer,
            self.seats[index].relative_pointer,
        );
        // SAFETY: each proxy is live and each destructor belongs to its own
        // interface. Order is innermost first: the relative pointer is an
        // add-on object that references the pointer.
        unsafe {
            self.conn
                .release(relative, zwp_relative_pointer_v1::destroy);
            self.conn.release(pointer, wl_pointer::release);
        }
        self.seats[index].pointer = ptr::null_mut();
        self.seats[index].relative_pointer = ptr::null_mut();
    }

    /// Tears down a seat's keyboard, clearing focus and any repeat with it.
    fn remove_keyboard(&mut self, index: usize) {
        self.seats[index].repeat.key = None;
        self.seats[index].held.clear();
        self.seats[index].keymap = None;
        if let Some(window) = self.seats[index].keyboard_focus.take() {
            self.set_focus(window, false);
        }
        let keyboard = self.seats[index].keyboard;
        // SAFETY: the proxy is live and `release` is `wl_keyboard`'s own
        // destructor.
        unsafe { self.conn.release_since(keyboard, 3, wl_keyboard::release) };
        self.seats[index].keyboard = ptr::null_mut();
    }

    /// Drops a seat entirely, which is what `wl_registry.global_remove` means
    /// for one.
    pub(super) fn remove_seat(&mut self, index: usize) {
        if !self.seats[index].pointer.is_null() {
            self.remove_pointer(index);
        }
        if !self.seats[index].keyboard.is_null() {
            self.remove_keyboard(index);
        }
        self.remove_data_device(index);
        self.remove_text_input(index);
        let proxy = self.seats[index].proxy;
        // `wl_seat.release` since version 5, and this binds 8. See the output
        // case in `process_raw` for why a client-side destroy is wrong here.
        // SAFETY: the proxy is live and `release` is `wl_seat`'s destructor.
        unsafe { self.conn.release_since(proxy, 5, wl_seat::release) };
        self.seats.remove(index);
    }

    /// Routes one pointer, keyboard or relative-motion event.
    #[expect(
        clippy::too_many_lines,
        reason = "the input routing table; see process_raw"
    )]
    pub(super) fn process_input(&mut self, event: RawEvent) {
        match event {
            RawEvent::PointerEnter {
                pointer,
                serial,
                surface,
                x,
                y,
            } => {
                let Some(index) = self.seat_index(pointer, |seat| seat.pointer as usize) else {
                    return;
                };
                let Some(window) = self.window_by_proxy(surface, |w| w.surface as usize) else {
                    return;
                };
                let scale = self.window(window).map_or(1.0, WlWindow::scale_factor);
                let position = PhysicalPoint::new(
                    keymap::fixed_to_f64(x) * scale,
                    keymap::fixed_to_f64(y) * scale,
                );
                self.seats[index].enter_serial = serial;
                self.seats[index].last_serial = serial;
                self.seats[index].pointer_focus = Some(window);
                self.seats[index].pointer_position = position;
                // Wayland requires the cursor to be set again on every enter:
                // the compositor resets it to the default when it crosses a
                // surface boundary, so a game that hid its cursor once would
                // see it flicker back on every re-entry.
                self.apply_cursor(index, window);
                let device = self.seats[index].device;
                self.queue.push_back(ShellEvent::PointerFocus {
                    window,
                    device,
                    // `wl_pointer.enter` has no `time` argument; see
                    // `TimeBase::event_time_now`.
                    time: self.time.event_time_now(),
                    entered: true,
                    position: Some(position),
                });
            }
            RawEvent::PointerLeave {
                pointer,
                serial,
                surface,
            } => {
                let Some(index) = self.seat_index(pointer, |seat| seat.pointer as usize) else {
                    return;
                };
                self.seats[index].last_serial = serial;
                self.flush_pointer_frame(index);
                let window = self.window_by_proxy(surface, |w| w.surface as usize);
                self.seats[index].pointer_focus = None;
                let Some(window) = window else { return };
                let device = self.seats[index].device;
                self.queue.push_back(ShellEvent::PointerFocus {
                    window,
                    device,
                    // As for `enter`: the protocol carries no timestamp.
                    time: self.time.event_time_now(),
                    entered: false,
                    position: None,
                });
            }
            RawEvent::PointerMotion {
                pointer,
                time,
                x,
                y,
            } => {
                let Some(index) = self.seat_index(pointer, |seat| seat.pointer as usize) else {
                    return;
                };
                self.seats[index].frame.time = time;
                self.seats[index].frame.motion =
                    Some((keymap::fixed_to_f64(x), keymap::fixed_to_f64(y)));
            }
            RawEvent::PointerButton {
                pointer,
                serial,
                time,
                button,
                state,
            } => {
                let Some(index) = self.seat_index(pointer, |seat| seat.pointer as usize) else {
                    return;
                };
                self.seats[index].last_serial = serial;
                if state == wl_pointer::button_state::PRESSED {
                    // The implicit grab a drag would be started from.
                    self.seats[index].press_serial = serial;
                }
                // Motion accumulated so far happened *before* this button, and
                // a click's position is the point of the whole event.
                self.seats[index].frame.time = time;
                self.flush_pointer_frame(index);
                let Some(window) = self.seats[index].pointer_focus else {
                    return;
                };
                let locked = self
                    .window(window)
                    .is_ok_and(|w| w.pointer_mode == PointerMode::Locked);
                let seat = &self.seats[index];
                self.queue.push_back(ShellEvent::Button {
                    window,
                    device: seat.device,
                    time: self.time.event_time(time),
                    button: keymap::pointer_button(button),
                    state: ButtonState::from_pressed(state == wl_pointer::button_state::PRESSED),
                    position: (!locked).then_some(seat.pointer_position),
                    modifiers: seat.modifiers,
                });
            }
            RawEvent::PointerAxis {
                pointer,
                time,
                axis,
                value,
            } => {
                let Some(index) = self.seat_index(pointer, |seat| seat.pointer as usize) else {
                    return;
                };
                let Some(slot) = axis_slot(axis) else { return };
                let frame = &mut self.seats[index].frame;
                frame.time = time;
                frame.continuous[slot] += keymap::fixed_to_f64(value);
                frame.has_axis = true;
            }
            RawEvent::PointerAxisSource { pointer, source } => {
                if let Some(index) = self.seat_index(pointer, |seat| seat.pointer as usize) {
                    self.seats[index].frame.axis_source = Some(source);
                }
            }
            RawEvent::PointerAxisDiscrete {
                pointer,
                axis,
                discrete,
            } => {
                let Some(index) = self.seat_index(pointer, |seat| seat.pointer as usize) else {
                    return;
                };
                let Some(slot) = axis_slot(axis) else { return };
                let frame = &mut self.seats[index].frame;
                frame.discrete[slot] += f64::from(discrete);
                frame.has_axis = true;
            }
            RawEvent::PointerAxisValue120 {
                pointer,
                axis,
                value120,
            } => {
                let Some(index) = self.seat_index(pointer, |seat| seat.pointer as usize) else {
                    return;
                };
                let Some(slot) = axis_slot(axis) else { return };
                let frame = &mut self.seats[index].frame;
                frame.value120[slot] += f64::from(value120);
                frame.has_axis = true;
            }
            RawEvent::PointerFrame { pointer } => {
                if let Some(index) = self.seat_index(pointer, |seat| seat.pointer as usize) {
                    self.flush_pointer_frame(index);
                }
            }
            RawEvent::RelativeMotion {
                relative,
                utime_hi,
                utime_lo,
                dx_unaccel,
                dy_unaccel,
            } => {
                let Some(index) = self.seat_index(relative, |seat| seat.relative_pointer as usize)
                else {
                    return;
                };
                let frame = &mut self.seats[index].frame;
                let (dx, dy) = frame.raw.unwrap_or((0.0, 0.0));
                frame.raw = Some((
                    dx + keymap::fixed_to_f64(dx_unaccel),
                    dy + keymap::fixed_to_f64(dy_unaccel),
                ));
                frame.raw_micros = Some((u64::from(utime_hi) << 32) | u64::from(utime_lo));
            }
            RawEvent::KeyboardKeymap {
                keyboard,
                format,
                fd,
                size,
            } => self.keymap_changed(keyboard, format, fd, size),
            RawEvent::KeyboardEnter {
                keyboard,
                serial,
                surface,
            } => {
                let Some(index) = self.seat_index(keyboard, |seat| seat.keyboard as usize) else {
                    return;
                };
                self.seats[index].last_serial = serial;
                self.seats[index].held.clear();
                let window = self.window_by_proxy(surface, |w| w.surface as usize);
                self.seats[index].keyboard_focus = window;
                if let Some(window) = window {
                    self.set_focus(window, true);
                }
            }
            RawEvent::KeyboardLeave {
                keyboard,
                serial,
                surface,
            } => {
                let Some(index) = self.seat_index(keyboard, |seat| seat.keyboard as usize) else {
                    return;
                };
                self.seats[index].last_serial = serial;
                // Our knowledge of the clipboard goes with the focus: another
                // client may replace the selection while we are not looking,
                // and the compositor will not tell us until focus comes back.
                // Reads issued in the meantime are held rather than answered
                // from a stale offer.
                if let Some(device) = self.seats[index].data.as_mut() {
                    device.selection_seen = false;
                }
                // Focus loss ends every repeat: nothing will deliver the
                // release, and a shell that kept fabricating presses would hold
                // a key down in a window that is not even focused.
                self.seats[index].repeat.key = None;
                self.seats[index].held.clear();
                self.seats[index].keyboard_focus = None;
                if let Some(keymap) = self.seats[index].keymap.as_mut() {
                    keymap.reset_compose();
                }
                if let Some(window) = self.window_by_proxy(surface, |w| w.surface as usize) {
                    self.set_focus(window, false);
                }
            }
            RawEvent::KeyboardKey {
                keyboard,
                serial,
                time,
                key,
                state,
            } => self.key_event(keyboard, serial, time, key, state),
            RawEvent::KeyboardModifiers {
                keyboard,
                serial,
                depressed,
                latched,
                locked,
                group,
            } => {
                let Some(index) = self.seat_index(keyboard, |seat| seat.keyboard as usize) else {
                    return;
                };
                self.seats[index].last_serial = serial;
                if let Some(keymap) = self.seats[index].keymap.as_mut() {
                    keymap.update(depressed, latched, locked, group);
                }
                self.refresh_modifiers(index);
            }
            RawEvent::KeyboardRepeatInfo {
                keyboard,
                rate,
                delay,
            } => {
                let Some(index) = self.seat_index(keyboard, |seat| seat.keyboard as usize) else {
                    return;
                };
                self.seats[index].repeat.rate = rate;
                self.seats[index].repeat.delay = delay;
                if rate <= 0 {
                    self.seats[index].repeat.key = None;
                }
            }
            _ => {}
        }
    }

    /// Compiles a new keymap, replacing whatever the seat had.
    fn keymap_changed(&mut self, keyboard: usize, format: u32, fd: i32, size: u32) {
        let index = self.seat_index(keyboard, |seat| seat.keyboard as usize);
        let Some(index) = index else {
            // The keyboard went away between the event and the drain. The
            // descriptor is still ours and still has to be returned.
            fd::close(fd);
            return;
        };
        // A layout switch invalidates which key was repeating — the keysym it
        // would produce has changed under it.
        self.seats[index].repeat.key = None;
        if format != wl_keyboard::keymap_format::XKB_V1 {
            // `no_keymap` is a real value: a compositor may say "there is no
            // layout at all". Keys still map to `KeyCode`; see `xkb`.
            fd::close(fd);
            self.seats[index].keymap = None;
            return;
        }
        self.seats[index].keymap = xkb::Keymap::from_fd(fd, size);
        self.refresh_modifiers(index);
    }

    /// Recomputes a seat's modifier set from whichever source it has.
    fn refresh_modifiers(&mut self, index: usize) {
        let seat = &mut self.seats[index];
        seat.modifiers = match seat.keymap.as_ref() {
            Some(keymap) => keymap.modifiers(),
            None => xkb::modifiers_from_held(&seat.held),
        };
    }

    /// One `wl_keyboard.key`.
    fn key_event(&mut self, keyboard: usize, serial: u32, time: u32, key: u32, state: u32) {
        let Some(index) = self.seat_index(keyboard, |seat| seat.keyboard as usize) else {
            return;
        };
        // Recorded before the focus check below: a key pressed while no window
        // of ours has focus still advances the seat's serial, and a
        // `set_selection` quoting a stale one is a clipboard write a compositor
        // may drop.
        self.seats[index].last_serial = serial;
        let Some(focused) = self.seats[index].keyboard_focus else {
            return;
        };
        let window = self.keyboard_target(focused);
        let pressed = state == wl_keyboard::key_state::PRESSED;
        let key_code = keymap::key_code(key);

        // Held-key bookkeeping feeds the degraded modifier path and nothing
        // else; with a keymap, XKB's `modifiers` event is authoritative.
        if let Some(code) = key_code {
            let held = &mut self.seats[index].held;
            if pressed {
                if !held.contains(&code) {
                    held.push(code);
                }
            } else {
                held.retain(|candidate| *candidate != code);
            }
        }
        if self.seats[index].keymap.is_none() {
            self.refresh_modifiers(index);
        }

        let text = pressed
            .then(|| {
                self.seats[index]
                    .keymap
                    .as_mut()
                    .and_then(|keymap| keymap.text(key))
            })
            .flatten();
        let seat = &self.seats[index];
        let keysym = seat
            .keymap
            .as_ref()
            .map_or(Keysym::NONE, |keymap| keymap.keysym(key));
        let event_time = self.time.event_time(time);
        let event = ShellEvent::Key {
            window,
            device: seat.device,
            time: event_time,
            scancode: Scancode(key),
            key_code,
            keysym,
            state: ButtonState::from_pressed(pressed),
            repeat: false,
            modifiers: seat.modifiers,
        };
        let popup = self.window(window).ok().and_then(|state| state.popup_input);
        let dismissed = crate::role::escape_dismisses(popup, &event);
        self.queue.push_back(event);
        // Text follows its key, never precedes it: a field that inserts a
        // character before it has seen the keystroke cannot implement "Ctrl
        // suppresses text".
        if let Some(text) = text {
            self.queue.push_back(ShellEvent::TextCommit {
                window,
                time: event_time,
                text,
            });
        }
        if dismissed {
            self.close_requested(window);
        }

        let repeats = self.seats[index]
            .keymap
            .as_ref()
            // With no keymap there is nothing that knows which keys repeat, and
            // repeating a modifier would hold Shift down forever. Repeat is
            // therefore off entirely in the degraded path.
            .is_some_and(|keymap| keymap.repeats(key));
        if pressed && repeats {
            let now = ffi::monotonic_nanos();
            self.seats[index].repeat.start(
                now,
                RepeatKeyRequest {
                    window,
                    scancode: key,
                    key_code,
                    keysym,
                },
            );
        } else if !pressed {
            self.seats[index].repeat.stop(key);
        }
    }

    /// Emits every repeat that has come due, on every seat.
    ///
    /// Called from [`pump`](crate::Shell::pump) rather than driven by a timer
    /// thread: the shell is thread-affine, and a repeat that arrived between
    /// frames would have to be queued anyway. The timestamp is the instant the
    /// repeat was *due*, so a slow frame does not smear the interval.
    pub(super) fn drive_repeats(&mut self) {
        let now = ffi::monotonic_nanos();
        for index in 0..self.seats.len() {
            let Some(mut key) = self.seats[index].repeat.key else {
                continue;
            };
            if key.next_nanos > now {
                continue;
            }
            // A stall — a long frame, a breakpoint, a suspended laptop — must
            // not release a backlog of presses into the action layer. Four
            // intervals is the point at which catching up stops being catching
            // up and starts being a burst.
            let behind = now.saturating_sub(key.next_nanos);
            if behind > key.interval_nanos.saturating_mul(4) {
                key.next_nanos = now;
            }
            let mut emitted = 0;
            while key.next_nanos <= now && emitted < 8 {
                let seat = &mut self.seats[index];
                // A repeat types as a press does, through the compose
                // sequence and with the modifiers held now, which is what an
                // X server's repeats are: presses.
                let text = seat
                    .keymap
                    .as_mut()
                    .and_then(|keymap| keymap.text(key.scancode));
                let time = self.time.event_time_nanos(key.next_nanos);
                self.queue.push_back(ShellEvent::Key {
                    window: key.window,
                    device: seat.device,
                    time,
                    scancode: Scancode(key.scancode),
                    key_code: key.key_code,
                    keysym: key.keysym,
                    state: ButtonState::Pressed,
                    repeat: true,
                    modifiers: seat.modifiers,
                });
                if let Some(text) = text {
                    self.queue.push_back(ShellEvent::TextCommit {
                        window: key.window,
                        time,
                        text,
                    });
                }
                key.next_nanos = key.next_nanos.saturating_add(key.interval_nanos);
                emitted += 1;
            }
            self.seats[index].repeat.key = Some(key);
        }
    }

    /// How long until the earliest repeat is due, if any is.
    pub(super) fn next_repeat_in(&self) -> Option<Duration> {
        let now = ffi::monotonic_nanos();
        self.seats
            .iter()
            .filter_map(|seat| seat.repeat.due_at())
            .min()
            .map(|due| Duration::from_nanos(due.saturating_sub(now)))
    }

    pub(super) fn flush_pointer_frames(&mut self) {
        for index in 0..self.seats.len() {
            self.flush_pointer_frame(index);
        }
    }

    /// Turns one accumulated [`PointerFrame`](super::PointerFrame) into at most
    /// one motion and at most one wheel event.
    fn flush_pointer_frame(&mut self, index: usize) {
        if self.seats[index].frame.is_empty() {
            return;
        }
        let frame = core::mem::take(&mut self.seats[index].frame);
        // The frame's time survives the take: it is the seat's clock, not the
        // frame's payload, and the next event without one should still be
        // stamped sensibly.
        self.seats[index].frame.time = frame.time;
        let Some(window) = self.seats[index].pointer_focus else {
            return;
        };
        let scale = self.window(window).map_or(1.0, WlWindow::scale_factor);
        let locked = self
            .window(window)
            .is_ok_and(|w| w.pointer_mode == PointerMode::Locked);

        if let Some((x, y)) = frame.motion {
            self.seats[index].pointer_position = PhysicalPoint::new(x * scale, y * scale);
        }
        let seat = &self.seats[index];
        if frame.motion.is_some() || frame.raw.is_some() {
            let time = frame.raw_micros.map_or_else(
                || self.time.event_time(frame.time),
                |micros| self.time.event_time_micros(micros),
            );
            self.queue.push_back(ShellEvent::PointerMotion {
                window,
                device: seat.device,
                time,
                // Under a lock the pointer does not move, so reporting the
                // frozen position would make anything that reads it appear to
                // work until the day it does not. `PointerMode::Locked` says so.
                abs: (!locked).then_some(seat.pointer_position),
                raw_delta: frame.raw,
            });
        }
        if let Some(delta) = frame.scroll(scale) {
            self.queue.push_back(ShellEvent::Wheel {
                window,
                device: seat.device,
                time: self.time.event_time(frame.time),
                delta,
                position: (!locked).then_some(seat.pointer_position),
                modifiers: seat.modifiers,
            });
        }
    }

    /// Sets a window's focus flag, emitting only on a change.
    ///
    /// Two sources say a window is focused — `wl_keyboard.enter`/`leave` and
    /// `xdg_toplevel.state.activated` — and on every compositor worth the name
    /// they agree. Funnelling both through one diffing setter means a consumer
    /// sees one [`ShellEvent::Focus`] rather than two, and that a compositor
    /// restating `activated` on every resize does not look like a focus change.
    pub(super) fn set_focus(&mut self, window: WindowId, focused: bool) {
        let Ok(state) = self.window_mut(window) else {
            return;
        };
        if state.focused == focused {
            return;
        }
        state.focused = focused;
        self.queue.push_back(ShellEvent::Focus { window, focused });
    }
}
