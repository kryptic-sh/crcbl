//! Window properties: the ICCCM and EWMH conversation, geometry, and the
//! cursor.
//!
//! Everything a window manager reads about a window is a *property* on it, and
//! everything this backend asks a window manager to do is either a property
//! written before the window was mapped or a client message sent after. That
//! split is the one thing to know about EWMH and it is the source of the
//! subtlest bug in the protocol; see [`X11Shell::apply_fullscreen`].

use super::{
    Conn, DisplayMode, MonitorId, PhysicalSize, ShellError, SizeHints, WindowConfiguration,
    WmStateAction, X11Shell, XWindow, ffi, keys,
};

impl X11Shell {
    /// Sets both title properties.
    ///
    /// Two, because two generations of window manager read different ones:
    /// `_NET_WM_NAME` is UTF-8 and is what everything modern uses, and `WM_NAME`
    /// is ICCCM's Latin-1 `STRING` that older tools — `xwininfo`, `wmctrl`,
    /// some tiling window managers' matching rules — still read. Writing only
    /// the first gives a window with no name in half the tools on the desktop.
    ///
    /// The `WM_NAME` copy is written as UTF-8 bytes anyway, which is
    /// technically a lie about the encoding and is what every toolkit does:
    /// the alternative is transliterating to Latin-1 and losing every character
    /// outside it.
    pub(super) fn write_title(&self, xid: u32, title: &str) {
        self.conn.set_property(
            xid,
            self.conn.atoms.net_wm_name,
            self.conn.atoms.utf8_string,
            8,
            title.as_bytes(),
        );
        self.conn.set_property(
            xid,
            ffi::value::ATOM_WM_NAME,
            ffi::value::ATOM_STRING,
            8,
            title.as_bytes(),
        );
    }

    /// Writes the properties a window manager expects to find at map time.
    ///
    /// `WM_CLASS` is two NUL-terminated strings — instance then class — and
    /// both terminators are part of the property. This is the property a
    /// desktop matches against a `.desktop` file for the icon and the taskbar
    /// grouping, so [`WindowDesc::app_id`](crate::WindowDesc::app_id) landing
    /// here correctly is the difference between the game's icon and a generic
    /// gear.
    pub(super) fn write_identity(&self, xid: u32, app_id: &str) {
        let mut class = Vec::with_capacity(app_id.len() * 2 + 2);
        class.extend_from_slice(app_id.as_bytes());
        class.push(0);
        class.extend_from_slice(app_id.as_bytes());
        class.push(0);
        self.conn.set_property(
            xid,
            ffi::value::ATOM_WM_CLASS,
            ffi::value::ATOM_STRING,
            8,
            &class,
        );

        // `WM_PROTOCOLS` is how the close request exists at all: without
        // `WM_DELETE_WINDOW` in it, a window manager closing the window calls
        // `KillClient` and the process dies mid-frame with no chance to ask
        // about unsaved work. This one property is the entire difference
        // between `CloseRequested` and SIGKILL.
        let protocols = [self.conn.atoms.wm_delete_window];
        self.conn.set_property(
            xid,
            self.conn.atoms.wm_protocols,
            ffi::value::ATOM_ATOM,
            32,
            &words_to_bytes(&protocols),
        );

        // `_NET_WM_PID` lets a desktop's "force quit" find the process, and
        // lets a session manager attribute the window. One word, and free.
        let pid = [std::process::id()];
        self.conn.set_property(
            xid,
            self.conn.atoms.net_wm_pid,
            ffi::value::ATOM_CARDINAL,
            32,
            &words_to_bytes(&pid),
        );

        // **`WM_HINTS` is how a window says it wants the keyboard**, and
        // without it a window manager is free to decide it does not.
        //
        // ICCCM 4.1.2.4's `input` field selects the focus model, and 4.1.7 says
        // a window manager "may assume convenient values for all fields" when a
        // window is mapped without the property. "This window takes no input"
        // is a convenient value: under `openbox` the window is mapped, drawn
        // and never focused, so the game receives no key for the whole of its
        // run. Under bare `Xvfb` nothing was managing focus in the first place,
        // so the omission cost nothing and stayed invisible.
        //
        // `input = True` with no `WM_TAKE_FOCUS` in `WM_PROTOCOLS` is ICCCM's
        // **passive** model: the window manager sets the focus, the client does
        // nothing. That is what a game wants. The other three models exist for
        // clients that manage focus among sub-windows of their own.
        //
        // Nine words, in ICCCM's order: flags, input, initial_state,
        // icon_pixmap, icon_window, icon_x, icon_y, icon_mask, window_group.
        // Only the first two flags are set, so the remaining six words are the
        // zeroes a window manager will not read.
        /// `InputHint | StateHint`.
        const HINT_FLAGS: u32 = (1 << 0) | (1 << 1);
        /// ICCCM's `NormalState`.
        const NORMAL_STATE: u32 = 1;
        let hints = [HINT_FLAGS, 1, NORMAL_STATE, 0, 0, 0, 0, 0, 0];
        self.conn.set_property(
            xid,
            ffi::value::ATOM_WM_HINTS,
            ffi::value::ATOM_WM_HINTS,
            32,
            &words_to_bytes(&hints),
        );
    }

    /// Writes `WM_NORMAL_HINTS` from the window's current request.
    ///
    /// **Only meaningful with a window manager**, which is why
    /// [`ShellCaps::ASPECT_HINT_HONORED`](crate::ShellCaps) is latched from
    /// whether one is running. The property is written either way: it costs one
    /// request, a window manager started later will read it, and a property
    /// nobody reads does no harm — whereas *claiming* it is honoured when
    /// nothing will honour it is the lie the capability bit exists to prevent.
    pub(super) fn write_size_hints(&self, window: &XWindow) {
        let current = window.configuration.or(window.pending).map_or_else(
            || window.requested_size.to_physical(self.scale_factor),
            |config| config.size,
        );
        let hints = SizeHints::from_constraints(
            window.requested_constraints,
            window.resizable,
            current,
            self.scale_factor,
        );
        self.conn.set_property(
            window.id,
            ffi::value::ATOM_WM_NORMAL_HINTS,
            ffi::value::ATOM_WM_SIZE_HINTS,
            32,
            &signed_words_to_bytes(&hints.to_words()),
        );
    }

    /// Asks for or gives up `_NET_WM_STATE_FULLSCREEN`.
    ///
    /// # The property-versus-message split, which is not optional
    ///
    /// EWMH says the *window manager* owns `_NET_WM_STATE` once a window is
    /// managed. Before the window is first mapped nobody manages it, so writing
    /// the property directly is how an initial state is requested — and it is
    /// the only way, because there is no window manager conversation to have
    /// yet. After the window is mapped, writing the property is silently
    /// undone: the window manager reasserts its own view on the next change,
    /// and the client's write leaves no trace. A `ClientMessage` to the **root
    /// window** is the request, and the window manager answers by rewriting the
    /// property itself.
    ///
    /// A backend that only writes the property gets a fullscreen toggle that
    /// works exactly once, at startup, and appears to do nothing afterwards.
    ///
    /// # The boundary is the map *request*, not `MapNotify`
    ///
    /// [`XWindow::mapped`] follows the server's `MapNotify`, because
    /// [`WindowState::visible`](crate::WindowState::visible) is meant to report
    /// what happened rather than what was asked. That is the wrong question
    /// here: a window manager starts managing a window when the `MapWindow`
    /// request reaches it, and `MapNotify` comes back afterwards. Between the
    /// two, `mapped` is false and the window is already managed — so a
    /// `set_mode` in that window took the property branch, wrote a request the
    /// window manager then overwrote with its own view, and vanished.
    ///
    /// That window is not narrow. On X11 the *first configure* also arrives
    /// before `MapNotify` — the server knew the geometry before `create_window`
    /// returned — so a game that opens a window, waits for its size, and asks
    /// for fullscreen lands in it every time. [`XWindow::map_requested`] is the
    /// honest predicate, and it is separate from `mapped` rather than replacing
    /// it because the two answer different questions.
    pub(super) fn apply_fullscreen(&self, window: &XWindow, fullscreen: bool) {
        if window.map_requested {
            let message = ffi::ClientMessageEvent {
                response_type: ffi::event::CLIENT_MESSAGE,
                format: 32,
                sequence: 0,
                window: window.id,
                type_: self.conn.atoms.net_wm_state,
                data: keys::net_wm_state_message(
                    if fullscreen {
                        WmStateAction::Add
                    } else {
                        WmStateAction::Remove
                    },
                    self.conn.atoms.net_wm_state_fullscreen,
                    0,
                ),
            };
            // The message goes to the **root**, with the substructure masks the
            // window manager selected on it — sending it to our own window
            // delivers it to us and to nobody else.
            /// `SubstructureNotify | SubstructureRedirect`.
            const SUBSTRUCTURE: u32 = (1 << 19) | (1 << 20);
            self.conn.send_event(self.conn.root, SUBSTRUCTURE, &message);
        } else if fullscreen {
            let state = [self.conn.atoms.net_wm_state_fullscreen];
            self.conn.set_property(
                window.id,
                self.conn.atoms.net_wm_state,
                ffi::value::ATOM_ATOM,
                32,
                &words_to_bytes(&state),
            );
        } else {
            self.conn
                .delete_property(window.id, self.conn.atoms.net_wm_state);
        }
    }

    /// Moves a window onto a monitor, for borderless-on-a-named-display.
    ///
    /// This is the request Wayland structurally cannot make — a client there
    /// may hint an output to `set_fullscreen` and no more — and it is why
    /// [`DisplayMode::Borderless`]'s `monitor` field is honoured on this
    /// backend rather than ignored. The move happens *before* the fullscreen
    /// request, because a window manager makes a window fullscreen on whichever
    /// monitor it currently overlaps most.
    ///
    /// # Errors
    ///
    /// [`ShellError::NoSuchMonitor`] if the monitor was unplugged since
    /// [`monitors`](crate::Shell::monitors) was called.
    pub(super) fn move_to_monitor(&self, xid: u32, monitor: MonitorId) -> Result<(), ShellError> {
        let target = self
            .monitors
            .iter()
            .find(|info| info.id == monitor)
            .ok_or(ShellError::NoSuchMonitor(monitor.0))?;
        let values = [target.bounds.x, target.bounds.y];
        // SAFETY: the connection and window are live, and the value list has
        // exactly the two words the `X | Y` mask declares — `ConfigureWindow`
        // reads one word per set mask bit, in bit order.
        unsafe {
            (self.conn.lib.configure_window)(
                self.conn.raw(),
                xid,
                ffi::config_window::X | ffi::config_window::Y,
                values.as_ptr().cast::<core::ffi::c_void>(),
            );
        }
        Ok(())
    }

    /// The window's configuration as the server has it *right now*.
    ///
    /// A synchronous round trip, and the thing X11 can do that Wayland cannot:
    /// a window has a size the moment it is created, so this always answers.
    /// See [`create_window`](crate::Shell::create_window) on this backend for
    /// why the answer is still delivered as an event rather than returned.
    pub(super) fn geometry_now(&self, xid: u32, mode: DisplayMode) -> Option<WindowConfiguration> {
        // SAFETY: the connection is live; a null error pointer discards the
        // error, and a null reply is handled below.
        let reply = unsafe {
            let cookie = (self.conn.lib.get_geometry)(self.conn.raw(), xid);
            (self.conn.lib.get_geometry_reply)(self.conn.raw(), cookie, core::ptr::null_mut())
        };
        if reply.is_null() {
            return None;
        }
        // SAFETY: `reply` is a live reply this call owns.
        let size =
            unsafe { PhysicalSize::new(u32::from((*reply).width), u32::from((*reply).height)) };
        // SAFETY: freed exactly once.
        unsafe { ffi::free_reply(reply) };
        Some(WindowConfiguration {
            size,
            scale_factor: self.scale_factor,
            mode,
        })
    }

    /// The window's origin in the desktop's coordinate space.
    ///
    /// **Not** `GetGeometry`'s `x`/`y`: under a reparenting window manager
    /// those are relative to the frame the window manager wrapped the window
    /// in, so a window at the top-left of its own decorations reports `(0, 0)`
    /// no matter where the decorations are. `TranslateCoordinates` against the
    /// root is the only answer that survives reparenting, and it is why a
    /// naive X11 client's saved window position drifts up and left by the
    /// title-bar height every time it is restored.
    pub(super) fn origin_now(&self, xid: u32) -> Option<(i32, i32)> {
        // SAFETY: the connection and both windows are live.
        let reply = unsafe {
            let cookie =
                (self.conn.lib.translate_coordinates)(self.conn.raw(), xid, self.conn.root, 0, 0);
            (self.conn.lib.translate_coordinates_reply)(
                self.conn.raw(),
                cookie,
                core::ptr::null_mut(),
            )
        };
        if reply.is_null() {
            return None;
        }
        // SAFETY: `reply` is a live reply this call owns.
        let origin = unsafe { (i32::from((*reply).dst_x), i32::from((*reply).dst_y)) };
        // SAFETY: freed exactly once.
        unsafe { ffi::free_reply(reply) };
        Some(origin)
    }

    /// A 1×1 fully-transparent cursor, created once and shared.
    ///
    /// Core X11's way of hiding the pointer, and it needs no extension: a
    /// cursor built from an empty 1-bit pixmap with an empty mask draws
    /// nothing. `XFixesHideCursor` would be one call instead of four and would
    /// cost a whole extra library for it — see [`ffi`]'s table of what is
    /// deliberately not loaded.
    pub(super) fn blank_cursor(&mut self) -> u32 {
        if self.blank_cursor != 0 {
            return self.blank_cursor;
        }
        let pixmap = self.conn.generate_id();
        let cursor = self.conn.generate_id();
        // SAFETY: the connection and root are live, both ids are fresh from
        // `xcb_generate_id`, and depth 1 is what a cursor source and mask must
        // be. A newly created pixmap's contents are undefined by protocol, but
        // using the *same* pixmap as source and mask makes that irrelevant: the
        // mask is what decides which pixels are drawn, and a pixel is drawn
        // only where source and mask agree, so an all-zero or all-one pixmap
        // both yield an invisible cursor.
        unsafe {
            (self.conn.lib.create_pixmap)(self.conn.raw(), 1, pixmap, self.conn.root, 1, 1);
            (self.conn.lib.create_cursor)(
                self.conn.raw(),
                cursor,
                pixmap,
                pixmap,
                0,
                0,
                0,
                0,
                0,
                0,
                0,
                0,
            );
            (self.conn.lib.free_pixmap)(self.conn.raw(), pixmap);
        }
        self.blank_cursor = cursor;
        cursor
    }

    /// Applies a window's recorded cursor to its attributes.
    ///
    /// Only the hidden case is real; a *shape* is recorded and not applied, for
    /// the same reason the Wayland backend gives — naming a shape needs an
    /// XCursor theme loader, which is a second asset pipeline inside the shell
    /// and is how a cursor ends up disagreeing with the user's theme. See
    /// [`set_cursor`](crate::Shell::set_cursor) on this backend.
    pub(super) fn apply_cursor(&mut self, xid: u32, hidden: bool) {
        let cursor = if hidden {
            self.blank_cursor()
        } else {
            // `XCB_NONE` on `CWCursor` means "inherit the parent's", which for
            // a top-level window is the root's — the user's themed default.
            ffi::value::NONE
        };
        /// `XCB_CW_CURSOR`.
        const CW_CURSOR: u32 = 1 << 14;
        let values = [cursor];
        // SAFETY: the connection and window are live, and the value list has
        // exactly the one word `CW_CURSOR` declares.
        unsafe {
            (self.conn.lib.change_window_attributes)(
                self.conn.raw(),
                xid,
                CW_CURSOR,
                values.as_ptr().cast::<core::ffi::c_void>(),
            );
        }
    }
}

impl Conn {
    /// Destroys a window, ignoring the case where the server already did.
    pub(super) fn destroy_x_window(&self, xid: u32) {
        // SAFETY: the connection is live. Destroying a window that is already
        // gone yields a `BadWindow` the server reports asynchronously and that
        // nothing here checks — which is correct, because the only way to
        // reach it is a race the outcome of which is the one we wanted.
        unsafe { (self.lib.destroy_window)(self.raw(), xid) };
    }
}

/// A `u32` slice as native-endian bytes, for `ChangeProperty` format 32.
///
/// Native endianness is right: libxcb byte-swaps for a connection whose server
/// has the other order, and doing it here as well would swap twice.
pub(super) fn words_to_bytes(words: &[u32]) -> Vec<u8> {
    words.iter().flat_map(|word| word.to_ne_bytes()).collect()
}

/// As [`words_to_bytes`], for the signed words `WM_SIZE_HINTS` is defined with.
pub(super) fn signed_words_to_bytes(words: &[i32]) -> Vec<u8> {
    words.iter().flat_map(|word| word.to_ne_bytes()).collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn property_words_are_native_endian_bytes() {
        // libxcb swaps for a foreign-endian server, so swapping here too would
        // swap twice and hand the window manager a mangled atom.
        assert_eq!(words_to_bytes(&[1, 2]), {
            let mut expected = 1u32.to_ne_bytes().to_vec();
            expected.extend_from_slice(&2u32.to_ne_bytes());
            expected
        });
        assert_eq!(words_to_bytes(&[]), Vec::<u8>::new());
        assert_eq!(signed_words_to_bytes(&[-1]).len(), 4);
        assert_eq!(signed_words_to_bytes(&[-1]), (-1i32).to_ne_bytes());
    }

    #[test]
    fn the_size_hints_property_is_the_right_length_in_bytes() {
        // A window manager reads `WM_NORMAL_HINTS` by length; 18 words is 72
        // bytes and anything else is ignored wholesale.
        let hints = SizeHints::default();
        assert_eq!(signed_words_to_bytes(&hints.to_words()).len(), 72);
    }
}
