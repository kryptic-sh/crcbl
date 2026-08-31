//! Draining the connection, and turning X11 events into [`ShellEvent`]s.
//!
//! # Decision: auto-repeat is recognized by holding, not by lookahead
//!
//! X11 auto-repeat is the *server's*, so unlike Wayland there is nothing to
//! synthesize — but the default wire form is a trap. Without the XKB
//! extension's detectable-autorepeat mode, a repeat arrives as a `KeyRelease`
//! immediately followed by a `KeyPress` of the same keycode *with the same
//! timestamp*, and neither half means what it says: the key never came up.
//! Forwarding both makes a jump button fire thirty times a second while held.
//!
//! The textbook client-side answer is a **one-event lookahead** — a
//! `KeyRelease` whose successor is a `KeyPress` of the same keycode at the same
//! millisecond is a repeat pair — and this module still implements it, in
//! [`is_repeat_pair`]. It is also **unsound on its own**, which is the finding
//! this arrangement exists for: the lookahead can only see events already
//! collected, and what has been collected is a socket-read boundary rather than
//! a protocol unit. On a loaded machine the two halves land in different
//! [`pump`](crate::Shell::pump)s, the lookahead sees a lone `KeyRelease`, and
//! the backend emits exactly the phantom release the lookahead was written to
//! prevent — under load only, which is the worst way for it to be wrong.
//!
//! So the pair is not relied on.
//! [`setup_detectable_auto_repeat`](super::Conn::setup_detectable_auto_repeat)
//! asks the server, at connect time, to stop synthesizing the release at all;
//! a repeat is then a bare `KeyPress` of a keycode that is **already in
//! [`X11Shell::held`]**, which no batching can split. The lookahead remains as
//! the fallback for a server or a machine with no XKB, where it is the best
//! that can be done.
//!
//! # Decision: collect the batch, then interpret it
//!
//! [`pump`](crate::Shell::pump) drains everything libxcb has queued into a
//! `Vec` of raw event bytes *before* interpreting any of it. That costs one
//! allocation per frame and is what gives the fallback lookahead anything to
//! look at.
//!
//! # Decision: raw motion is attributed to the focused window
//!
//! `XI_RawMotion` is selected on the **root**, because the server refuses to
//! deliver raw events to anything else — they are pre-transform and pre-clip,
//! so they belong to no window. That means this backend receives raw motion
//! while some *other* application is focused, which
//! [`relative-pointer-v1`](crate::wayland) structurally cannot do.
//!
//! Forwarding it would be a first-person camera that keeps turning while the
//! player is typing in another window. So raw motion is attributed to whichever
//! of our windows has keyboard focus and dropped when none does — the same
//! behaviour Wayland gets from the protocol, reconstructed.

use crcbl_core::input::{ButtonState, Keysym, Modifiers, Scancode};

use super::{
    Conn, DisplayMode, KEYBOARD_DEVICE, KeyCode, LOCK_RECENTRE_FRACTION, POINTER_DEVICE,
    PhysicalPoint, PhysicalSize, PointerMode, ShellEvent, WindowConfiguration, WindowId, X11Shell,
    ffi, keys, read_wire, xkb,
};

/// `NotifyGrab` and `NotifyUngrab`, the two `FocusIn`/`FocusOut` modes that are
/// a **grab artefact rather than a focus change**.
///
/// Every pointer grab this backend takes for
/// [`PointerMode`] generates a `FocusOut(NotifyGrab)` and a
/// matching `FocusIn(NotifyUngrab)`. Forwarding them would tell a game it lost
/// focus at the exact moment it locked the pointer — and
/// [`ShellEvent::Focus`]'s own documentation says a consumer must treat focus
/// loss as "every key is now up", so the player would drop everything they were
/// holding on entering mouselook.
const NOTIFY_GRAB: u8 = 1;
/// See [`NOTIFY_GRAB`].
const NOTIFY_UNGRAB: u8 = 2;

/// One event, copied out of libxcb's allocation.
///
/// Owned bytes rather than a borrow, because the batch outlives each
/// `xcb_poll_for_event` result and libxcb hands over ownership of every one.
type Raw = Vec<u8>;

impl X11Shell {
    /// Reads everything queued, without blocking.
    fn collect_events(&mut self) -> Vec<Raw> {
        let mut batch = Vec::new();
        loop {
            // SAFETY: the connection is live; `xcb_poll_for_event` returns
            // either null or a `malloc`'d event this call now owns.
            let event = unsafe { (self.conn.lib.poll_for_event)(self.conn.raw()) };
            if event.is_null() {
                break;
            }
            // SAFETY: `event` is a live event. An ordinary X11 event is 32
            // wire bytes; a `GeGeneric` is 32 plus its `length` field's count
            // of 4-byte units, plus the four bytes libxcb *inserts* at offset
            // 32 for the reconstructed sequence number — see
            // [`ffi::XiRawEvent`]. libxcb has read all of it into this one
            // allocation.
            let bytes = unsafe {
                let header = *event;
                let size = if header.response_type & 0x7f == ffi::event::GE_GENERIC {
                    // `pad[0]` is the `length` field: the header's first four
                    // bytes are response_type/extension/sequence, so the u32 at
                    // offset 4 is the first of `pad`.
                    36 + (header.pad[0] as usize).saturating_mul(4)
                } else {
                    32
                };
                core::slice::from_raw_parts(event.cast::<u8>(), size).to_vec()
            };
            // SAFETY: the event came from libxcb's `malloc` and is freed once.
            unsafe { ffi::free_reply(event) };
            batch.push(bytes);
            // A pathological peer could in principle keep the queue non-empty
            // forever; `pump` promises to be finite, so the batch is bounded.
            // Whatever is left over is left in *libxcb's* queue rather than on
            // the socket, which no descriptor poll can see — so the cap is
            // recorded for `wait_events`.
            if batch.len() >= MAX_EVENTS_PER_PUMP {
                self.undrained_events = true;
                break;
            }
        }
        batch
    }

    /// Drains the connection and queues the [`ShellEvent`]s it produced.
    pub(super) fn drain(&mut self) {
        self.undrained_events = false;
        let batch = self.collect_events();
        let mut index = 0;
        while index < batch.len() {
            let event = &batch[index];
            // The auto-repeat pair: this `KeyRelease` is not a release, and the
            // `KeyPress` after it is not a fresh press. Only reachable when the
            // server would not turn detectable auto-repeat on — with it on the
            // release half is never sent, so there is no pair to find and no
            // batch boundary to be caught out by.
            if !self.conn.detectable_auto_repeat && is_repeat_pair(event, batch.get(index + 1)) {
                self.handle_key(&batch[index + 1], true);
                index += 2;
                continue;
            }
            self.handle_event(event);
            index += 1;
        }
        if let Some(detail) = self.conn.has_error()
            && self.lost.is_none()
        {
            crcbl_core::log::error!("x11 connection lost: {detail}");
            self.lost = Some(detail);
        }
    }

    /// Dispatches one event.
    fn handle_event(&mut self, raw: &Raw) {
        let response = raw[0] & 0x7f;
        // RandR's events are numbered from a base the server assigns per
        // connection, so they are checked before the core numbers — the two
        // ranges overlap and a hard-coded comparison would read a
        // `ScreenChangeNotify` as whatever core event has that number.
        if let Some(base) = self.conn.randr_event_base
            && response >= base
            && response <= base.saturating_add(1)
        {
            self.handle_monitors_changed();
            return;
        }
        match response {
            ffi::event::KEY_PRESS => self.handle_key(raw, false),
            ffi::event::KEY_RELEASE => self.handle_key(raw, false),
            ffi::event::BUTTON_PRESS => self.handle_button(raw, true),
            ffi::event::BUTTON_RELEASE => self.handle_button(raw, false),
            ffi::event::MOTION_NOTIFY => self.handle_motion(raw),
            ffi::event::ENTER_NOTIFY => self.handle_crossing(raw, true),
            ffi::event::LEAVE_NOTIFY => self.handle_crossing(raw, false),
            ffi::event::FOCUS_IN => self.handle_focus(raw, true),
            ffi::event::FOCUS_OUT => self.handle_focus(raw, false),
            ffi::event::CONFIGURE_NOTIFY => self.handle_configure(raw),
            ffi::event::MAP_NOTIFY => self.handle_map(raw, true),
            ffi::event::UNMAP_NOTIFY => self.handle_map(raw, false),
            ffi::event::DESTROY_NOTIFY => self.handle_destroy(raw),
            ffi::event::CLIENT_MESSAGE => self.handle_client_message(raw),
            ffi::event::PROPERTY_NOTIFY => self.handle_property_notify(raw),
            ffi::event::SELECTION_CLEAR => self.handle_selection_clear(raw),
            ffi::event::SELECTION_REQUEST => self.handle_selection_request(raw),
            ffi::event::SELECTION_NOTIFY => self.handle_selection_notify(raw),
            ffi::event::MAPPING_NOTIFY => self.handle_mapping_notify(raw),
            ffi::event::GE_GENERIC => self.handle_generic(raw),
            // `Expose` and the rest are real events this backend has nothing to
            // do with: there is no renderer to repaint with until P1, and the
            // seam has no damage vocabulary. Silently ignored rather than
            // logged, because a moving window produces a stream of them.
            _ => {}
        }
    }

    /// A key went down or came up.
    ///
    /// `paired` is the fallback lookahead's verdict — see the [module
    /// docs](self). With detectable auto-repeat on it is always `false` and the
    /// repeat is recognized here instead, from the key already being held.
    fn handle_key(&mut self, raw: &Raw, paired: bool) {
        let Some(event) = read_wire::<ffi::InputEvent>(raw) else {
            return;
        };
        let Some(window) = self.window_by_xid(event.event) else {
            return;
        };
        let pressed = event.response_type & 0x7f == ffi::event::KEY_PRESS;
        // A press of a key that is already down is the server repeating it.
        // Nothing else can produce one: a second press without an intervening
        // release is not something a keyboard can send, and `held` is cleared
        // on every focus change, so a key held while another client had focus
        // cannot leave a stale entry behind.
        let repeat = paired || (pressed && self.held.contains(&event.detail));
        self.last_server_time = event.time;
        let time = self.time.event_time(event.time);

        let Some(scancode) = keys::scancode(event.detail) else {
            return;
        };
        // XKB's contract: read the symbol for a press *before* applying the
        // key, because a modifier takes effect from the next key onwards. The
        // same ordering makes `modifiers` the state at the time of the event,
        // which is what X11's own `state` field means and what
        // [`event`](crate::event) specifies for this seam.
        let keysym = self
            .keymap
            .as_ref()
            .map_or(Keysym::NONE, |keymap| keymap.keysym(scancode));
        let text = if pressed {
            self.keymap
                .as_ref()
                .and_then(|keymap| keymap.text(scancode))
        } else {
            None
        };
        let modifiers = self.modifiers();

        if let Some(keymap) = self.keymap.as_mut() {
            keymap.update_key(u32::from(event.detail), pressed);
        }
        if pressed {
            if !self.held.contains(&event.detail) {
                self.held.push(event.detail);
            }
        } else {
            self.held.retain(|held| *held != event.detail);
        }

        self.queue.push_back(ShellEvent::Key {
            window,
            device: KEYBOARD_DEVICE,
            time,
            scancode: Scancode(scancode),
            key_code: keys::key_code(event.detail),
            keysym,
            state: if pressed {
                ButtonState::Pressed
            } else {
                ButtonState::Released
            },
            repeat,
            modifiers,
        });
        if let Some(text) = text {
            self.queue
                .push_back(ShellEvent::TextCommit { window, time, text });
        }
    }

    /// A pointer button went down or came up — or the wheel turned.
    fn handle_button(&mut self, raw: &Raw, pressed: bool) {
        let Some(event) = read_wire::<ffi::InputEvent>(raw) else {
            return;
        };
        let Some(window) = self.window_by_xid(event.event) else {
            return;
        };
        self.last_server_time = event.time;
        let time = self.time.event_time(event.time);
        let modifiers = self.modifiers();
        let position = self.position_of(window, &event);

        match keys::button(event.detail, pressed) {
            keys::Button::Press(button) => self.queue.push_back(ShellEvent::Button {
                window,
                device: POINTER_DEVICE,
                time,
                button,
                state: if pressed {
                    ButtonState::Pressed
                } else {
                    ButtonState::Released
                },
                position,
                modifiers,
            }),
            keys::Button::Scroll(delta) => self.queue.push_back(ShellEvent::Wheel {
                window,
                device: POINTER_DEVICE,
                time,
                delta,
                position,
                modifiers,
            }),
            // The release half of a wheel notch. Delivered by the server,
            // meaningless to the seam, and forwarding it would double every
            // scroll.
            keys::Button::ScrollRelease => {}
        }
    }

    /// The pointer moved.
    ///
    /// Under [`PointerMode::Locked`] the *absolute* position is suppressed —
    /// [`ShellEvent::PointerMotion`] documents `abs: None` as the invariant of
    /// that mode — and the delta a camera reads comes from XI2 raw motion
    /// instead. What core motion is still needed for while locked is the
    /// re-centring: `GrabPointer`'s `confine_to` keeps the pointer inside the
    /// window but it still *moves*, so it eventually rests against an edge and
    /// stops generating motion in that direction.
    fn handle_motion(&mut self, raw: &Raw) {
        let Some(event) = read_wire::<ffi::InputEvent>(raw) else {
            return;
        };
        let Some(window) = self.window_by_xid(event.event) else {
            return;
        };
        self.last_server_time = event.time;
        let time = self.time.event_time(event.time);
        let locked = self
            .window(window)
            .is_ok_and(|state| state.pointer_mode == PointerMode::Locked);
        if locked {
            self.recentre_if_near_edge(window, &event);
            // Raw motion carries the delta; a locked pointer's absolute
            // position is meaningless and a synthetic one would be worse than
            // none. If XI2 is absent the camera gets nothing, which is exactly
            // what `RAW_POINTER_MOTION` being clear promised.
            return;
        }
        let position = self.position_of(window, &event);
        self.queue.push_back(ShellEvent::PointerMotion {
            window,
            device: POINTER_DEVICE,
            time,
            abs: position,
            // Core X11 motion is accelerated and clamped, so it is never
            // `raw_delta`; XI2 delivers that separately.
            raw_delta: None,
        });
    }

    /// The pointer entered or left a window.
    fn handle_crossing(&mut self, raw: &Raw, entered: bool) {
        let Some(event) = read_wire::<ffi::InputEvent>(raw) else {
            return;
        };
        let Some(window) = self.window_by_xid(event.event) else {
            return;
        };
        self.last_server_time = event.time;
        let time = self.time.event_time(event.time);
        let position = if entered {
            self.position_of(window, &event)
        } else {
            None
        };
        if let Ok(state) = self.window_mut(window) {
            state.pointer_inside = entered;
        }
        self.queue.push_back(ShellEvent::PointerFocus {
            window,
            device: POINTER_DEVICE,
            time,
            entered,
            position,
        });
    }

    /// Keyboard focus arrived or left.
    fn handle_focus(&mut self, raw: &Raw, focused: bool) {
        let Some(event) = read_wire::<ffi::FocusEvent>(raw) else {
            return;
        };
        if event.mode == NOTIFY_GRAB || event.mode == NOTIFY_UNGRAB {
            return;
        }
        let Some(window) = self.window_by_xid(event.event) else {
            return;
        };
        if let Ok(state) = self.window_mut(window) {
            if state.focused == focused {
                return;
            }
            state.focused = focused;
        }
        if focused {
            // Everything that happened while another client had focus is
            // invisible to us, so the incrementally-tracked key and modifier
            // state is stale by definition — the classic stuck-Shift-after-
            // alt-tab bug. The server's own view is the authority.
            self.held.clear();
            self.resync_modifiers();
        } else {
            self.held.clear();
        }
        self.queue.push_back(ShellEvent::Focus { window, focused });
    }

    /// The server told us a window's geometry.
    ///
    /// The **only** authoritative size, and unlike Wayland's configure it is
    /// not a negotiation: there is nothing to acknowledge and nothing to commit.
    /// A `ConfigureNotify` that only moved the window is not a resize, which is
    /// why the previous size is kept — a window dragged across the desktop
    /// otherwise recreates the swapchain on every motion event.
    fn handle_configure(&mut self, raw: &Raw) {
        let Some(event) = read_wire::<ffi::ConfigureNotifyEvent>(raw) else {
            return;
        };
        let Some(window) = self.window_by_xid(event.window) else {
            return;
        };
        let size = PhysicalSize::new(u32::from(event.width), u32::from(event.height));
        if size.is_empty() {
            return;
        }
        let scale_factor = self.scale_factor;
        let Ok(state) = self.window_mut(window) else {
            return;
        };
        if state.last_size == Some(size) && state.configuration.is_some() {
            return;
        }
        state.last_size = Some(size);
        let config = WindowConfiguration {
            size,
            scale_factor,
            // The *effective* mode, which is a question only the window manager
            // can answer — so it is read back from `_NET_WM_STATE` rather than
            // assumed to be what was asked for.
            mode: DisplayMode::Windowed,
        };
        state.pending = Some(config);
    }

    /// A window was mapped or unmapped.
    fn handle_map(&mut self, raw: &Raw, mapped: bool) {
        let Some(event) = read_wire::<ffi::WindowNotifyEvent>(raw) else {
            return;
        };
        let Some(window) = self.window_by_xid(event.window) else {
            return;
        };
        if let Ok(state) = self.window_mut(window) {
            state.mapped = mapped;
            state.visible = mapped;
        }
        if mapped {
            // A window's effective mode can only be read once a window manager
            // has had the chance to set it, and mapping is that chance.
            self.refresh_effective_mode(window);
        }
    }

    /// A window went away without us asking.
    ///
    /// The window manager can do this — a session ending, or `xkill` — and the
    /// seam's answer is the same as an explicit destroy: the handle goes stale
    /// and a [`WindowDestroyed`](ShellEvent::WindowDestroyed) says so.
    fn handle_destroy(&mut self, raw: &Raw) {
        let Some(event) = read_wire::<ffi::WindowNotifyEvent>(raw) else {
            return;
        };
        let Some(window) = self.window_by_xid(event.window) else {
            return;
        };
        self.windows.remove(window.cast());
        // The same cleanup the explicit path does, through the same function:
        // an outstanding read left alive here answered two seconds later,
        // naming a window this very event has just reported as destroyed.
        self.forget_selection_state(window, event.window);
        self.queue.push_back(ShellEvent::WindowDestroyed { window });
    }

    /// A message from another client — the window manager, or a drag source.
    fn handle_client_message(&mut self, raw: &Raw) {
        let Some(event) = read_wire::<ffi::ClientMessageEvent>(raw) else {
            return;
        };
        // XDND first, and *before* the window lookup: a drop is refused with an
        // `XdndFinished` even when it names a window this backend does not
        // know, because the source blocks its own drag loop until one arrives.
        if self.handle_xdnd_message(&event) {
            return;
        }
        let Some(window) = self.window_by_xid(event.window) else {
            return;
        };
        if event.type_ != self.conn.atoms.wm_protocols
            || event.data[0] != self.conn.atoms.wm_delete_window
        {
            return;
        }
        // ICCCM: the window is **still open**. This is a question, and the
        // answer is `reply_close_request` — which is why the seam has one.
        if let Ok(state) = self.window_mut(window) {
            if state.close_pending {
                return;
            }
            state.close_pending = true;
        }
        self.queue.push_back(ShellEvent::CloseRequested { window });
    }

    /// The keyboard layout changed.
    ///
    /// The whole keymap is recompiled rather than patched: `MappingNotify` says
    /// *that* the mapping changed and gives a keycode range, and libxkbcommon
    /// has no incremental update. Recompiling costs a round trip and happens
    /// when a user switches layout, which is not a frame-loop concern.
    fn handle_mapping_notify(&mut self, raw: &Raw) {
        let Some(event) = read_wire::<ffi::MappingNotifyEvent>(raw) else {
            return;
        };
        // `request == 2` is a *pointer* mapping change — which button is which
        // — and has nothing to do with the keymap.
        if event.request == 2 {
            return;
        }
        // SAFETY: the connection is live and this shell is not `Send`, so
        // libxkbcommon-x11's round trips cannot race anything.
        self.keymap =
            unsafe { xkb::Keymap::from_x11_device(self.conn.raw().cast::<core::ffi::c_void>()) };
        self.resync_modifiers();
    }

    /// An extension event with a payload — XI2 raw motion, and nothing else
    /// this backend selected.
    fn handle_generic(&mut self, raw: &Raw) {
        let Some(opcode) = self.conn.xi_opcode else {
            return;
        };
        let Some(header) = read_wire::<ffi::XiRawEvent>(raw) else {
            return;
        };
        if header.extension != opcode || header.event_type != ffi::value::XI_RAW_MOTION {
            return;
        }
        let Some((dx, dy)) = raw_motion_delta(raw, &header) else {
            return;
        };
        if dx == 0.0 && dy == 0.0 {
            return;
        }
        // Raw motion is selected on the root and arrives whoever is focused;
        // see the module docs.
        let Some(window) = self
            .windows
            .iter()
            .find(|(_, state)| state.focused)
            .map(|(handle, _)| handle.cast())
        else {
            return;
        };
        self.last_server_time = header.time;
        let time = self.time.event_time(header.time);
        let locked = self
            .window(window)
            .is_ok_and(|state| state.pointer_mode == PointerMode::Locked);
        let abs = if locked {
            None
        } else {
            self.pointer_position(window)
        };
        self.queue.push_back(ShellEvent::PointerMotion {
            window,
            device: POINTER_DEVICE,
            time,
            abs,
            raw_delta: Some((dx, dy)),
        });
    }

    /// RandR says the monitor layout changed.
    fn handle_monitors_changed(&mut self) {
        let monitors = self.enumerate_monitors();
        if monitors == self.monitors {
            // RandR is chatty: a screen-change and a CRTC-change arrive for one
            // physical event, and a consumer told twice re-queries twice.
            return;
        }
        self.monitors = monitors;
        self.queue.push_back(ShellEvent::MonitorsChanged);
    }

    /// The modifiers currently in effect.
    fn modifiers(&self) -> Modifiers {
        match self.keymap.as_ref() {
            Some(keymap) => keymap.modifiers(),
            None => {
                let held: Vec<KeyCode> = self
                    .held
                    .iter()
                    .filter_map(|code| keys::key_code(*code))
                    .collect();
                xkb::modifiers_from_held(&held)
            }
        }
    }

    /// Re-seeds the XKB state from the server's own modifier mask.
    ///
    /// `FocusIn` carries no `state` field — unlike every key, button and motion
    /// event — so the mask has to be fetched, and `QueryPointer` is the request
    /// that has one. One round trip, on focus, which is not a frame-loop path.
    fn resync_modifiers(&mut self) {
        let Some(mask) = self.conn.query_pointer_mask() else {
            return;
        };
        if let Some(keymap) = self.keymap.as_mut() {
            keymap.resync_from_x11_state(mask);
        }
    }

    /// Where the pointer is in a window, unless the window is locked.
    fn position_of(&self, window: WindowId, event: &ffi::InputEvent) -> Option<PhysicalPoint> {
        if self
            .window(window)
            .is_ok_and(|state| state.pointer_mode == PointerMode::Locked)
        {
            return None;
        }
        Some(PhysicalPoint::new(
            f64::from(event.event_x),
            f64::from(event.event_y),
        ))
    }

    /// The pointer's current position in a window, by asking the server.
    fn pointer_position(&self, window: WindowId) -> Option<PhysicalPoint> {
        let xid = self.window(window).ok()?.id;
        let (x, y, _) = self.conn.query_pointer(xid)?;
        Some(PhysicalPoint::new(f64::from(x), f64::from(y)))
    }

    /// Warps a locked pointer back to the middle when it nears an edge.
    ///
    /// The Win32 clip-and-recentre technique, needed here for the same reason:
    /// `confine_to` stops the pointer leaving the window but not from resting
    /// against its edge, and a pointer against the edge stops producing motion
    /// in that direction. Raw motion is unaffected either way — it is
    /// pre-clipping — so this exists purely so that *core* motion keeps
    /// arriving, which is what tells us to check again.
    ///
    /// A margin rather than every event: warping on every motion event doubles
    /// the event rate and makes the pointer fight the user in the frame between
    /// the warp and its notification.
    fn recentre_if_near_edge(&mut self, window: WindowId, event: &ffi::InputEvent) {
        let Ok(state) = self.window(window) else {
            return;
        };
        let Some(size) = state.configuration.map(|config| config.size) else {
            return;
        };
        if size.is_empty() {
            return;
        }
        let xid = state.id;
        let centre_x = i32::try_from(size.width / 2).unwrap_or(0);
        let centre_y = i32::try_from(size.height / 2).unwrap_or(0);
        let margin_x = (f64::from(size.width) * LOCK_RECENTRE_FRACTION) as i32;
        let margin_y = (f64::from(size.height) * LOCK_RECENTRE_FRACTION) as i32;
        let x = i32::from(event.event_x);
        let y = i32::from(event.event_y);
        if (x - centre_x).abs() < margin_x && (y - centre_y).abs() < margin_y {
            return;
        }
        self.conn.warp_to(xid, centre_x, centre_y);
    }
}

/// The most events one [`pump`](crate::Shell::pump) will interpret.
///
/// `pump` promises to be finite. A peer generating events faster than they can
/// be drained — a `PropertyNotify` storm from a misbehaving clipboard owner —
/// would otherwise keep the loop inside one frame indefinitely. Whatever is
/// left is drained on the next call, which is what the socket would have done
/// anyway.
pub(super) const MAX_EVENTS_PER_PUMP: usize = 4096;

/// Whether these two events are an auto-repeat pair.
///
/// See the [module docs](self): a `KeyRelease` immediately followed by a
/// `KeyPress` of the same keycode at the same server millisecond is the server
/// repeating a held key, and neither half means what it says.
fn is_repeat_pair(first: &Raw, second: Option<&Raw>) -> bool {
    let Some(second) = second else {
        return false;
    };
    if first[0] & 0x7f != ffi::event::KEY_RELEASE || second[0] & 0x7f != ffi::event::KEY_PRESS {
        return false;
    }
    let (Some(release), Some(press)) = (
        read_wire::<ffi::InputEvent>(first),
        read_wire::<ffi::InputEvent>(second),
    ) else {
        return false;
    };
    release.detail == press.detail && release.time == press.time
}

/// `FP3232`, XInput 2's fixed-point valuator format, as an `f64`.
///
/// A signed 32-bit integral part and an *unsigned* 32-bit fraction — which is
/// the trap: for a negative value the integral part is the floor and the
/// fraction is still added, so `-0.5` is `(-1, 0x8000_0000)`. Subtracting the
/// fraction instead, which reads more naturally, gives the wrong sign for every
/// backwards mouse movement.
fn fp3232(integral: i32, frac: u32) -> f64 {
    /// What one unit of the fraction is worth: `2^-32`, so the fraction is
    /// divided by `2^32` and **not** by `u32::MAX`.
    ///
    /// The two differ by one part in four billion, which is why dividing by
    /// `u32::MAX` — the natural-looking spelling, and what this used to do — is
    /// invisible in a mouse delta and wrong anyway: `0x8000_0000` is exactly a
    /// half in this format, and no value of the fraction can reach 1.0.
    const SCALE: f64 = 4_294_967_296.0;
    f64::from(integral) + f64::from(frac) / SCALE
}

/// The `(dx, dy)` of an `XI_RawMotion`, from its *raw* valuators.
///
/// The wire layout after the 32-byte header is three variable-length arrays and
/// the whole point is to read the third:
///
/// 1. `valuator_mask`: `valuators_len` 32-bit words, one bit per axis.
/// 2. `axisvalues`: one `FP3232` per set bit — the **accelerated** value, which
///    is what core `MotionNotify` would have carried.
/// 3. `axisvalues_raw`: one `FP3232` per set bit — the device's own
///    unaccelerated number.
///
/// [`ShellCaps::RAW_POINTER_MOTION`](crate::ShellCaps) promises "unaccelerated
/// and unclamped", so this reads the second array and not the first. A backend
/// that stops at the first has a camera whose sensitivity changes with the
/// desktop's pointer-acceleration setting, which is the exact failure the
/// capability exists to rule out.
///
/// Valuators 0 and 1 are X and Y on every relative pointing device; a device
/// that reports only one of them (a scroll-only valuator) contributes to the
/// other axis nothing rather than garbage.
fn raw_motion_delta(raw: &Raw, header: &ffi::XiRawEvent) -> Option<(f64, f64)> {
    let mask_words = usize::from(header.valuators_len);
    let mask_start = size_of::<ffi::XiRawEvent>();
    let mask_end = mask_start.checked_add(mask_words.checked_mul(4)?)?;
    let mask = raw.get(mask_start..mask_end)?;

    let set: u32 = mask
        .chunks_exact(4)
        .map(|word| u32::from_ne_bytes([word[0], word[1], word[2], word[3]]).count_ones())
        .sum();
    let set = set as usize;
    // Skip the accelerated array to reach the raw one.
    let raw_start = mask_end.checked_add(set.checked_mul(8)?)?;

    let mut delta = (0.0, 0.0);
    let mut slot = 0usize;
    for (word_index, word) in mask.chunks_exact(4).enumerate() {
        let bits = u32::from_ne_bytes([word[0], word[1], word[2], word[3]]);
        for bit in 0..32u32 {
            if bits & (1 << bit) == 0 {
                continue;
            }
            let axis = word_index * 32 + bit as usize;
            let offset = raw_start.checked_add(slot.checked_mul(8)?)?;
            let value = raw.get(offset..offset.checked_add(8)?)?;
            let integral = i32::from_ne_bytes([value[0], value[1], value[2], value[3]]);
            let frac = u32::from_ne_bytes([value[4], value[5], value[6], value[7]]);
            match axis {
                0 => delta.0 = fp3232(integral, frac),
                1 => delta.1 = fp3232(integral, frac),
                _ => {}
            }
            slot += 1;
        }
    }
    Some(delta)
}

impl Conn {
    /// The pointer's position in a window, and the modifier mask with it.
    pub(super) fn query_pointer(&self, xid: u32) -> Option<(i16, i16, u16)> {
        // SAFETY: the connection and window are live; a null error pointer
        // discards the error and a null reply is handled below.
        let reply = unsafe {
            let cookie = (self.lib.query_pointer)(self.raw(), xid);
            (self.lib.query_pointer_reply)(self.raw(), cookie, core::ptr::null_mut())
        };
        if reply.is_null() {
            return None;
        }
        // SAFETY: `reply` is a live reply this call owns.
        let answer = unsafe { ((*reply).win_x, (*reply).win_y, (*reply).mask) };
        // SAFETY: freed exactly once.
        unsafe { ffi::free_reply(reply) };
        Some(answer)
    }

    /// The server's own modifier mask, for a focus resync.
    fn query_pointer_mask(&self) -> Option<u16> {
        self.query_pointer(self.root).map(|(_, _, mask)| mask)
    }

    /// Moves the pointer to a point inside a window.
    pub(super) fn warp_to(&self, xid: u32, x: i32, y: i32) {
        // SAFETY: the connection and window are live. A zero source window
        // means "warp unconditionally", which is `XWarpPointer`'s documented
        // behaviour for a `None` source.
        unsafe {
            (self.lib.warp_pointer)(
                self.raw(),
                ffi::value::NONE,
                xid,
                0,
                0,
                0,
                0,
                i16::try_from(x).unwrap_or(0),
                i16::try_from(y).unwrap_or(0),
            );
        }
        self.flush();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A 32-byte key event on the wire.
    fn key_event(response_type: u8, detail: u8, time: u32) -> Raw {
        let event = ffi::InputEvent {
            response_type,
            detail,
            sequence: 0,
            time,
            root: 1,
            event: 2,
            child: 0,
            root_x: 0,
            root_y: 0,
            event_x: 0,
            event_y: 0,
            state: 0,
            same_screen: 1,
            pad0: 0,
        };
        // SAFETY: `InputEvent` is a `#[repr(C)]` struct of plain integers with
        // no padding holes that matter here, so its 32 bytes are readable as a
        // byte slice.
        unsafe {
            core::slice::from_raw_parts(core::ptr::from_ref(&event).cast::<u8>(), 32).to_vec()
        }
    }

    #[test]
    fn an_auto_repeat_pair_is_a_release_and_press_at_the_same_millisecond() {
        // The test every toolkit uses, and the reason `pump` collects the batch
        // before interpreting it. Without it a held key produces a stream of
        // press/release pairs, and a jump button fires thirty times a second.
        let release = key_event(ffi::event::KEY_RELEASE, 38, 1_000);
        let press = key_event(ffi::event::KEY_PRESS, 38, 1_000);
        assert!(is_repeat_pair(&release, Some(&press)));

        // A human cannot release and press within one server millisecond, so a
        // different timestamp is two real events.
        let later = key_event(ffi::event::KEY_PRESS, 38, 1_001);
        assert!(!is_repeat_pair(&release, Some(&later)));
        // A different key is two real events however close together.
        let other = key_event(ffi::event::KEY_PRESS, 39, 1_000);
        assert!(!is_repeat_pair(&release, Some(&other)));
        // A release at the end of the batch is a release.
        assert!(!is_repeat_pair(&release, None));
        // Two presses are not a pair, however identical.
        assert!(!is_repeat_pair(&press, Some(&press)));
    }

    #[test]
    fn fp3232_adds_the_fraction_even_when_the_integral_part_is_negative() {
        // The trap: the fraction is unsigned and the integral part is the
        // floor, so -0.5 is (-1, 0x8000_0000). Subtracting instead gives -1.5,
        // and every leftward mouse movement is then three times too far.
        // Exact equality, because these values are exact in the format and in
        // `f64` alike: the fraction is a count of `2^-32`, so a half is
        // `0x8000_0000` and nothing rounds. Asserted to a tolerance before,
        // which let the scale be `u32::MAX` — a division that can never quite
        // produce a half.
        assert_eq!(fp3232(3, 0), 3.0);
        assert_eq!(fp3232(0, 1 << 31), 0.5);
        assert_eq!(fp3232(-1, 1 << 31), -0.5);
        assert_eq!(fp3232(-3, 0), -3.0);
        // The largest fraction is one unit short of the next integer, and never
        // reaches it.
        assert_eq!(fp3232(0, u32::MAX), 1.0 - 1.0 / 4_294_967_296.0);
        assert!(fp3232(0, u32::MAX) < 1.0);
    }

    #[test]
    fn raw_motion_reads_the_unaccelerated_array_and_not_the_first_one() {
        // The whole reason XInput 2 is loaded. Both arrays are present in every
        // raw event; the first is what core motion would have said, and the
        // second is what a camera needs.
        let header = ffi::XiRawEvent {
            response_type: ffi::event::GE_GENERIC,
            extension: 131,
            sequence: 0,
            length: 0,
            event_type: ffi::value::XI_RAW_MOTION,
            deviceid: 2,
            time: 0,
            detail: 0,
            sourceid: 9,
            valuators_len: 1,
            flags: 0,
            pad0: [0; 4],
            full_sequence: 0,
        };
        let mut raw = Vec::new();
        // SAFETY: `XiRawEvent` is a `#[repr(C)]` struct of plain integers.
        raw.extend_from_slice(unsafe {
            core::slice::from_raw_parts(core::ptr::from_ref(&header).cast::<u8>(), 36)
        });
        // Valuator mask: axes 0 and 1 present.
        raw.extend_from_slice(&0b11u32.to_ne_bytes());
        // Accelerated values — deliberately different, so reading the wrong
        // array is a failing assertion rather than a coincidence.
        raw.extend_from_slice(&99i32.to_ne_bytes());
        raw.extend_from_slice(&0u32.to_ne_bytes());
        raw.extend_from_slice(&99i32.to_ne_bytes());
        raw.extend_from_slice(&0u32.to_ne_bytes());
        // Raw values: (7, -3.5).
        raw.extend_from_slice(&7i32.to_ne_bytes());
        raw.extend_from_slice(&0u32.to_ne_bytes());
        raw.extend_from_slice(&(-4i32).to_ne_bytes());
        raw.extend_from_slice(&(u32::MAX / 2).to_ne_bytes());

        let (dx, dy) = raw_motion_delta(&raw, &header).expect("well-formed");
        assert!((dx - 7.0).abs() < 1e-6, "{dx}");
        assert!((dy + 3.5).abs() < 1e-6, "{dy}");
    }

    #[test]
    fn a_truncated_raw_event_answers_none_rather_than_reading_past_the_end() {
        // A malformed or unexpectedly-short extension event must not be a
        // buffer overrun; the arrays are sized from a field the *server* set.
        let header = ffi::XiRawEvent {
            response_type: ffi::event::GE_GENERIC,
            extension: 131,
            sequence: 0,
            length: 0,
            event_type: ffi::value::XI_RAW_MOTION,
            deviceid: 2,
            time: 0,
            detail: 0,
            sourceid: 9,
            valuators_len: 4,
            flags: 0,
            pad0: [0; 4],
            full_sequence: 0,
        };
        assert_eq!(raw_motion_delta(&vec![0u8; 36], &header), None);
        assert_eq!(raw_motion_delta(&vec![0u8; 48], &header), None);
    }

    #[test]
    fn a_grab_is_not_a_focus_change() {
        // Every pointer grab produces a FocusOut/FocusIn pair with these modes.
        // Forwarding them tells a game it lost focus at the instant it entered
        // mouselook — and `ShellEvent::Focus` says focus loss means "every key
        // is now up".
        assert_ne!(NOTIFY_GRAB, NOTIFY_UNGRAB);
        assert_eq!((NOTIFY_GRAB, NOTIFY_UNGRAB), (1, 2));
    }
}
