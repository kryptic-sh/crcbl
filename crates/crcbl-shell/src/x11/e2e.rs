//! Test-only scaffolding: a **second X client** that plays every part of the
//! desktop the backend needs and cannot be.
//!
//! # Why this has to exist
//!
//! The Wayland end-to-end suite needed scaffolding because a Wayland surface is
//! only managed once it has a buffer, and attaching one is the renderer's job.
//! X11's version of that problem is different and, if anything, larger:
//! **almost everything the backend talks to is another program.**
//!
//! | What the suite needs to test | Who normally does it | Who does it here |
//! | --- | --- | --- |
//! | A close request | the window manager's title-bar button | `Peer::request_close` |
//! | Keyboard focus | the window manager's focus policy | `Peer::focus` |
//! | A resize the client did not ask for | the window manager, or the user | `Peer::resize` |
//! | Key, button and pointer input | a physical device | `Peer::key` etc., through `XTEST` |
//! | Something on the clipboard | any other application | `Peer::own_clipboard` |
//! | Reading *our* clipboard, including `INCR` | any other application | `Peer::read_clipboard` |
//!
//! Every one of those is a real X client doing a real thing over a real socket.
//! Nothing here reaches inside the shell; the peer only ever sees XIDs and
//! properties, exactly as `xterm` would.
//!
//! # Decision: one thread, cooperative servicing
//!
//! The peer is driven by `Peer::service`, which the test calls in the same
//! loop as [`Shell::pump`](crate::Shell::pump). It is not a thread, and that is
//! the point: the X11 clipboard's hardest case is an owner and a requestor
//! making alternating progress, and a test that put the peer on its own thread
//! would let the operating system schedule around a bug this backend is
//! supposed to be structured to avoid. Running both in one loop means a
//! backend that blocked waiting for the peer would deadlock the test, loudly.

use core::ffi::{CStr, c_char, c_int, c_void};
use core::ptr;
use std::collections::HashMap;

use super::ffi::{self, Connection, Lib};
use super::read_wire;

/// `xcb_test_fake_input`'s event types.
mod fake {
    /// `KeyPress`.
    pub const KEY_PRESS: u8 = 2;
    /// `KeyRelease`.
    pub const KEY_RELEASE: u8 = 3;
    /// `ButtonPress`.
    pub const BUTTON_PRESS: u8 = 4;
    /// `ButtonRelease`.
    pub const BUTTON_RELEASE: u8 = 5;
    /// `MotionNotify`.
    pub const MOTION: u8 = 6;
}

/// The extra libxcb entry points only the harness needs.
///
/// Deliberately **not** in [`ffi::Lib`]: that struct is audited by use, and a
/// symbol the shipping backend never calls does not belong in it. `SetInputFocus`
/// in particular is a window manager's job and a game has no business calling
/// it.
struct TestLib {
    /// `xcb_void_cookie_t xcb_set_input_focus(xcb_connection_t *,
    /// uint8_t revert_to, xcb_window_t focus, xcb_timestamp_t time)`
    set_input_focus: unsafe extern "C" fn(*mut Connection, u8, u32, u32) -> ffi::Cookie,
    /// `xcb_void_cookie_t xcb_test_fake_input(xcb_connection_t *, uint8_t type,
    /// uint8_t detail, uint32_t time, xcb_window_t root, int16_t rootX,
    /// int16_t rootY, uint8_t deviceid)` — from libxcb-xtest.
    fake_input:
        unsafe extern "C" fn(*mut Connection, u8, u8, u32, u32, i16, i16, u8) -> ffi::Cookie,
}

unsafe extern "C" {
    fn dlopen(filename: *const c_char, flags: c_int) -> *mut c_void;
    fn dlsym(handle: *mut c_void, symbol: *const c_char) -> *mut c_void;
}

/// `RTLD_NOW`.
const RTLD_NOW: c_int = 2;

fn load_test_lib() -> Option<TestLib> {
    // SAFETY: both are NUL-terminated literals and `RTLD_NOW` is valid.
    let (xcb, xtest) = unsafe {
        (
            dlopen(c"libxcb.so.1".as_ptr(), RTLD_NOW),
            dlopen(c"libxcb-xtest.so.0".as_ptr(), RTLD_NOW),
        )
    };
    if xcb.is_null() || xtest.is_null() {
        return None;
    }
    // SAFETY: both handles are live, the names are NUL-terminated literals, and
    // the asserted signatures are the ones `xcb/xproto.h` and `xcb/xtest.h`
    // declare.
    unsafe {
        let focus = dlsym(xcb, c"xcb_set_input_focus".as_ptr());
        let input = dlsym(xtest, c"xcb_test_fake_input".as_ptr());
        if focus.is_null() || input.is_null() {
            return None;
        }
        Some(TestLib {
            set_input_focus: core::mem::transmute::<
                *mut c_void,
                unsafe extern "C" fn(*mut Connection, u8, u32, u32) -> ffi::Cookie,
            >(focus),
            fake_input: core::mem::transmute::<
                *mut c_void,
                unsafe extern "C" fn(
                    *mut Connection,
                    u8,
                    u8,
                    u32,
                    u32,
                    i16,
                    i16,
                    u8,
                ) -> ffi::Cookie,
            >(input),
        })
    }
}

/// One payload the peer is offering on the clipboard.
struct Offer {
    target: u32,
    bytes: Vec<u8>,
}

/// An `INCR` transfer the peer is feeding to the shell.
struct Outgoing {
    requestor: u32,
    property: u32,
    target: u32,
    bytes: Vec<u8>,
    offset: usize,
    chunk: usize,
    terminated: bool,
}

/// What [`Peer::read_clipboard`] has got to.
#[derive(Debug, Default)]
struct Incoming {
    active: bool,
    incremental: bool,
    bytes: Vec<u8>,
    done: Option<Option<Vec<u8>>>,
}

/// A second X client, on its own connection.
pub struct Peer {
    lib: &'static Lib,
    test: TestLib,
    connection: *mut Connection,
    root: u32,
    window: u32,
    atoms: HashMap<String, u32>,
    offers: Vec<Offer>,
    outgoing: Vec<Outgoing>,
    incoming: Incoming,
    /// The most recent server timestamp any event has carried.
    ///
    /// X11 has no "what time is it" request, so the only way to hold a valid
    /// timestamp is to keep the last one the server sent. See
    /// [`server_time`](Self::server_time).
    last_time: u32,
    /// Bytes per `INCR` chunk when the peer owns the clipboard.
    ///
    /// Deliberately settable and deliberately tiny by default when
    /// [`own_clipboard_incrementally`](Self::own_clipboard_incrementally) is
    /// used: a real peer only uses `INCR` above the server's request limit
    /// (~256 KiB), so a test that waited for that would be slow *and* would
    /// exercise the chunking exactly once. Forcing three-byte chunks makes the
    /// state machine run its whole length in milliseconds.
    chunk: Option<usize>,
}

impl core::fmt::Debug for Peer {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        f.debug_struct("Peer")
            .field("window", &self.window)
            .field("offers", &self.offers.len())
            .field("outgoing", &self.outgoing.len())
            .finish()
    }
}

impl Peer {
    /// Connects a second client to the same display.
    ///
    /// `None` when libxcb-xtest is missing or the server refuses — both of
    /// which the harness turns into a skipped run with a message rather than a
    /// mysterious failure.
    #[must_use]
    pub fn new() -> Option<Self> {
        let lib = ffi::load().ok()?;
        let test = load_test_lib()?;
        // SAFETY: a null display name means "read `DISPLAY`".
        let connection = unsafe { (lib.connect)(ptr::null(), ptr::null_mut()) };
        if connection.is_null() {
            return None;
        }
        // SAFETY: the connection is live whether or not it has an error.
        if unsafe { (lib.connection_has_error)(connection) } != 0 {
            // SAFETY: released exactly once.
            unsafe { (lib.disconnect)(connection) };
            return None;
        }
        // SAFETY: the connection is live and error-free, so the setup reply and
        // its first screen are present and owned by libxcb.
        let root = unsafe {
            let setup = (lib.get_setup)(connection);
            let iterator = (lib.setup_roots_iterator)(setup);
            if iterator.data.is_null() {
                (lib.disconnect)(connection);
                return None;
            }
            (*iterator.data).root
        };

        // An unmapped 1×1 window is all a selection needs: ICCCM requires an
        // owner to be a window, not a visible one.
        let window = {
            // SAFETY: the connection is live.
            let id = unsafe { (lib.generate_id)(connection) };
            let values = [ffi::event_mask::PROPERTY_CHANGE];
            // SAFETY: the connection is live, `id` is fresh, the parent is the
            // screen's root, and the value list has exactly the one word the
            // `EVENT_MASK` bit declares.
            unsafe {
                (lib.create_window)(
                    connection,
                    0,
                    id,
                    root,
                    0,
                    0,
                    1,
                    1,
                    0,
                    ffi::value::INPUT_OUTPUT,
                    0,
                    ffi::cw::EVENT_MASK,
                    values.as_ptr().cast::<c_void>(),
                );
                (lib.flush)(connection);
            }
            id
        };

        Some(Self {
            lib,
            test,
            connection,
            root,
            window,
            atoms: HashMap::new(),
            offers: Vec::new(),
            outgoing: Vec::new(),
            incoming: Incoming::default(),
            last_time: 0,
            chunk: None,
        })
    }

    /// Interns an atom, caching it.
    pub fn atom(&mut self, name: &str) -> u32 {
        if let Some(atom) = self.atoms.get(name) {
            return *atom;
        }
        // SAFETY: the connection is live and `name` outlives the call.
        let atom = unsafe {
            let cookie = (self.lib.intern_atom)(
                self.connection,
                0,
                u16::try_from(name.len()).unwrap_or(u16::MAX),
                name.as_ptr().cast::<c_char>(),
            );
            let reply = (self.lib.intern_atom_reply)(self.connection, cookie, ptr::null_mut());
            if reply.is_null() {
                0
            } else {
                let value = (*reply).atom;
                ffi::free_reply(reply);
                value
            }
        };
        self.atoms.insert(name.to_string(), atom);
        atom
    }

    fn flush(&self) {
        // SAFETY: the connection is live.
        unsafe { (self.lib.flush)(self.connection) };
    }

    /// Gives a window keyboard focus.
    ///
    /// **Under bare `Xvfb` nothing else ever will.** With no window manager the
    /// input focus stays at its default of `PointerRoot`, so key events follow
    /// the pointer rather than any window, and a test that just sent keystrokes
    /// would watch them go nowhere. This is the window manager's focus policy,
    /// in one call.
    pub fn focus(&self, xid: u32) {
        /// `RevertToParent`.
        const REVERT_TO_PARENT: u8 = 2;
        // SAFETY: the connection is live and `xid` is a window on this display.
        unsafe {
            (self.test.set_input_focus)(
                self.connection,
                REVERT_TO_PARENT,
                xid,
                ffi::value::CURRENT_TIME,
            );
        }
        self.flush();
    }

    /// PROBE
    #[must_use]
    pub fn probe_root(&self) -> u32 {
        self.root
    }

    /// The server's current time, as close as X11 lets a client get to it.
    ///
    /// **There is no request that asks.** The only timestamps a client holds
    /// are the ones events carried, so the standard idiom — the one every
    /// toolkit uses, and the one this backend's own selection code uses — is to
    /// append *nothing* to a property and read the time off the
    /// `PropertyNotify` that provokes. The append is a no-op on the property;
    /// the event is the whole point.
    ///
    /// Why the harness needs one: `_NET_ACTIVE_WINDOW` carries the timestamp of
    /// the user action that asked for the activation, and a window manager
    /// compares it against the last input it saw. `CurrentTime` is zero, which
    /// is *older than everything*, so once any test in this suite had synthesised
    /// a keystroke `openbox` read every later activation request as stale and
    /// refused it — and every focus-needing test from that point in the run
    /// onwards timed out, while each passed on its own.
    ///
    /// Bounded: if the server never answers, the last time known is returned
    /// and the request is merely as good as it was before.
    fn server_time(&mut self) -> u32 {
        /// Long enough for a loaded server, short enough not to be felt. A
        /// local X socket answers in microseconds.
        const TURNS: u32 = 500;

        let stamp = self.atom("CRCBL_E2E_TIME");
        let before = self.last_time;
        // SAFETY: the connection and window are live. Appending zero elements
        // is well-defined and leaves the property's value untouched.
        unsafe {
            (self.lib.change_property)(
                self.connection,
                ffi::value::PROP_MODE_APPEND,
                self.window,
                stamp,
                ffi::value::ATOM_STRING,
                8,
                0,
                ptr::null(),
            );
        }
        self.flush();
        for _ in 0..TURNS {
            self.service();
            if self.last_time != before {
                break;
            }
        }
        self.last_time
    }

    /// Asks the **window manager** to focus a window, as a pager does.
    ///
    /// The counterpart to [`focus`](Self::focus), and the one to use when a
    /// window manager is running: it owns the input focus, so a client that
    /// calls `SetInputFocus` itself is overruled the moment the manager sees
    /// the `FocusIn` and applies its own policy. `_NET_ACTIVE_WINDOW` is the
    /// EWMH request that *asks* rather than takes, so nothing has to be won.
    ///
    /// `data[0]` is the source indication: `2` means a pager, which is the
    /// value a window manager honours unconditionally. `1` means an ordinary
    /// application asking to raise itself, which is exactly what focus-stealing
    /// prevention exists to refuse.
    pub fn activate(&mut self, xid: u32) {
        let active = self.atom("_NET_ACTIVE_WINDOW");
        let time = self.server_time();
        /// `SubstructureNotify | SubstructureRedirect`, the masks a window
        /// manager selects on the root.
        const SUBSTRUCTURE: u32 = (1 << 19) | (1 << 20);
        /// EWMH source indication: a pager.
        const SOURCE_PAGER: u32 = 2;
        let message = ffi::ClientMessageEvent {
            response_type: ffi::event::CLIENT_MESSAGE,
            format: 32,
            sequence: 0,
            window: xid,
            type_: active,
            data: [SOURCE_PAGER, time, 0, 0, 0],
        };
        // SAFETY: the connection and root are live, and `SendEvent` reads
        // exactly the 32 bytes a `ClientMessageEvent` is.
        unsafe {
            (self.lib.send_event)(
                self.connection,
                0,
                self.root,
                SUBSTRUCTURE,
                ptr::from_ref(&message).cast::<c_char>(),
            );
        }
        self.flush();
    }

    /// Sends `WM_DELETE_WINDOW`, exactly as a window manager's close button
    /// does.
    pub fn request_close(&mut self, xid: u32) {
        let protocols = self.atom("WM_PROTOCOLS");
        let delete = self.atom("WM_DELETE_WINDOW");
        let message = ffi::ClientMessageEvent {
            response_type: ffi::event::CLIENT_MESSAGE,
            format: 32,
            sequence: 0,
            window: xid,
            type_: protocols,
            data: [delete, ffi::value::CURRENT_TIME, 0, 0, 0],
        };
        // SAFETY: the connection and window are live, and `SendEvent` reads
        // exactly the 32 bytes a `ClientMessageEvent` is.
        unsafe {
            (self.lib.send_event)(
                self.connection,
                0,
                xid,
                0,
                ptr::from_ref(&message).cast::<c_char>(),
            );
        }
        self.flush();
    }

    /// Where someone else's window sits on the screen.
    ///
    /// **A window is at the origin only when nothing is managing it.** With no
    /// window manager the backend's window is placed at `0,0` and stays there,
    /// so a test could aim `XTEST` at a screen coordinate and know it was
    /// inside. A window manager places the window wherever its policy says —
    /// `openbox` centres it — and the same coordinate then lands on the root,
    /// where a click reaches nobody and the test waits out its deadline for
    /// events that were delivered somewhere else.
    ///
    /// `None` if the window has gone away between the caller reading its XID
    /// and this call.
    #[must_use]
    pub fn window_origin(&self, xid: u32) -> Option<(i16, i16)> {
        // SAFETY: the connection is live, `xid` names a window on this display
        // and the root always exists; a null error pointer discards the error
        // and a null reply is handled below.
        let reply = unsafe {
            let cookie = (self.lib.translate_coordinates)(self.connection, xid, self.root, 0, 0);
            (self.lib.translate_coordinates_reply)(self.connection, cookie, ptr::null_mut())
        };
        if reply.is_null() {
            return None;
        }
        // SAFETY: `reply` is a live reply this call owns.
        let origin = unsafe { ((*reply).dst_x, (*reply).dst_y) };
        // SAFETY: freed exactly once.
        unsafe { ffi::free_reply(reply) };
        Some(origin)
    }

    /// Resizes someone else's window, as a window manager or a user drag would.
    pub fn resize(&self, xid: u32, width: u32, height: u32) {
        let values = [width, height];
        // SAFETY: the connection and window are live, and the value list has
        // exactly the two words the `WIDTH | HEIGHT` mask declares.
        unsafe {
            (self.lib.configure_window)(
                self.connection,
                xid,
                ffi::config_window::WIDTH | ffi::config_window::HEIGHT,
                values.as_ptr().cast::<c_void>(),
            );
        }
        self.flush();
    }

    /// Synthesizes a key press or release through `XTEST`.
    ///
    /// A *real* event: the server generates it as though a device had, so it
    /// goes through the same focus routing, the same XKB state and the same
    /// auto-repeat machinery as a physical keyboard. `keycode` is the X11 one,
    /// which is the evdev code plus eight.
    pub fn key(&self, keycode: u8, pressed: bool) {
        self.fake(
            if pressed {
                fake::KEY_PRESS
            } else {
                fake::KEY_RELEASE
            },
            keycode,
            0,
            0,
        );
    }

    /// Synthesizes a pointer button. `1` is left, `2` is middle, `3` is right,
    /// `4`–`7` are the wheel.
    pub fn button(&self, button: u8, pressed: bool) {
        self.fake(
            if pressed {
                fake::BUTTON_PRESS
            } else {
                fake::BUTTON_RELEASE
            },
            button,
            0,
            0,
        );
    }

    /// Moves the pointer to an absolute position on the root window.
    pub fn motion(&self, x: i16, y: i16) {
        self.fake(fake::MOTION, 0, x, y);
    }

    fn fake(&self, kind: u8, detail: u8, x: i16, y: i16) {
        // SAFETY: the connection and root are live. Device `0` is the
        // `XTestFakeInput` default, which the server maps to the core device.
        unsafe {
            (self.test.fake_input)(
                self.connection,
                kind,
                detail,
                ffi::value::CURRENT_TIME,
                self.root,
                x,
                y,
                0,
            );
        }
        self.flush();
    }

    /// Takes the `CLIPBOARD` selection, offering `payloads` by target name.
    pub fn own_clipboard(&mut self, payloads: &[(&str, &[u8])]) {
        self.chunk = None;
        self.claim(payloads);
    }

    /// As [`own_clipboard`](Self::own_clipboard), but answers every conversion
    /// with an `INCR` transfer of `chunk`-byte pieces.
    ///
    /// The only way to exercise the receiving state machine in a test that
    /// finishes quickly.
    pub fn own_clipboard_incrementally(&mut self, payloads: &[(&str, &[u8])], chunk: usize) {
        self.chunk = Some(chunk.max(1));
        self.claim(payloads);
    }

    fn claim(&mut self, payloads: &[(&str, &[u8])]) {
        self.offers = payloads
            .iter()
            .map(|(name, bytes)| Offer {
                target: self.atom(name),
                bytes: (*bytes).to_vec(),
            })
            .collect();
        self.set_owner(self.window);
    }

    /// Gives up the selection, so nobody owns it.
    pub fn release_clipboard(&mut self) {
        self.offers.clear();
        self.outgoing.clear();
        self.set_owner(ffi::value::NONE);
    }

    /// Takes or releases `CLIPBOARD`, and **does not return until the server
    /// has done it**.
    ///
    /// The barrier is the point, and it is a protocol guarantee rather than a
    /// wait: X11 processes one client's requests strictly in order, so a
    /// `GetSelectionOwner` *reply* on this connection proves the
    /// `SetSelectionOwner` before it has already taken effect.
    ///
    /// Without it every clipboard test here is a race between two connections.
    /// The shell asks `GetSelectionOwner` from its own connection the moment
    /// [`Shell::clipboard_request`](crate::Shell::clipboard_request) is called,
    /// and the server is under no obligation to have read this one's socket
    /// first — so a claim that has been *sent* but not yet *processed* reads
    /// back as an unowned clipboard, and the shell correctly answers
    /// [`Empty`](crate::ClipboardContent::Empty) to a test expecting the
    /// payload. Pumping first does not help: pumping drives the shell, and this
    /// request is not the shell's.
    fn set_owner(&mut self, owner: u32) {
        let clipboard = self.atom("CLIPBOARD");
        // SAFETY: the connection is live, and `owner` is this peer's own window
        // or `XCB_NONE`.
        unsafe {
            (self.lib.set_selection_owner)(
                self.connection,
                owner,
                clipboard,
                ffi::value::CURRENT_TIME,
            );
        }
        self.flush();
        // SAFETY: the connection is live; a null error pointer discards the
        // error and a null reply is handled below.
        let reply = unsafe {
            let cookie = (self.lib.get_selection_owner)(self.connection, clipboard);
            (self.lib.get_selection_owner_reply)(self.connection, cookie, ptr::null_mut())
        };
        assert!(
            !reply.is_null(),
            "the X server did not answer GetSelectionOwner"
        );
        // SAFETY: `reply` is a live reply this call owns.
        let settled = unsafe { (*reply).owner };
        // SAFETY: freed exactly once.
        unsafe { ffi::free_reply(reply) };
        assert_eq!(
            settled, owner,
            "the server did not accept the peer's SetSelectionOwner"
        );
    }

    /// Starts reading whatever owns the clipboard, in `target`.
    ///
    /// Asynchronous, like the real thing: [`service`](Self::service) drives it
    /// and [`take_read`](Self::take_read) collects the answer.
    pub fn read_clipboard(&mut self, target: &str) {
        let clipboard = self.atom("CLIPBOARD");
        let target = self.atom(target);
        let property = self.atom("PEER_SELECTION");
        self.incoming = Incoming {
            active: true,
            ..Incoming::default()
        };
        // SAFETY: the connection, window and atoms are live.
        unsafe {
            (self.lib.delete_property)(self.connection, self.window, property);
            (self.lib.convert_selection)(
                self.connection,
                self.window,
                clipboard,
                target,
                property,
                ffi::value::CURRENT_TIME,
            );
        }
        self.flush();
    }

    /// Whether the outstanding read has been answered, without taking it.
    #[must_use]
    pub fn has_answer(&self) -> bool {
        self.incoming.done.is_some()
    }

    /// The answer to a [`read_clipboard`](Self::read_clipboard), once it
    /// arrives.
    ///
    /// `None` while it is still outstanding; `Some(None)` when the owner
    /// refused.
    #[must_use]
    pub fn take_read(&mut self) -> Option<Option<Vec<u8>>> {
        self.incoming.done.take()
    }

    /// Drains the peer's connection and answers whatever arrived.
    ///
    /// Call this in the same loop as [`Shell::pump`](crate::Shell::pump); see
    /// the [module docs](self) for why it is not a thread.
    pub fn service(&mut self) {
        loop {
            // SAFETY: the connection is live; the event is `malloc`'d and this
            // call owns it.
            let event = unsafe { (self.lib.poll_for_event)(self.connection) };
            if event.is_null() {
                return;
            }
            // SAFETY: every X11 event is at least 32 bytes.
            let bytes = unsafe { core::slice::from_raw_parts(event.cast::<u8>(), 32).to_vec() };
            // SAFETY: freed exactly once.
            unsafe { ffi::free_reply(event) };
            self.handle(&bytes);
        }
    }

    fn handle(&mut self, raw: &[u8]) {
        match raw[0] & 0x7f {
            ffi::event::SELECTION_REQUEST => self.answer_request(raw),
            ffi::event::SELECTION_NOTIFY => self.collect_notify(raw),
            ffi::event::PROPERTY_NOTIFY => self.property(raw),
            ffi::event::SELECTION_CLEAR => {
                self.offers.clear();
                self.outgoing.clear();
            }
            _ => {}
        }
    }

    fn answer_request(&mut self, raw: &[u8]) {
        let Some(event) = read_wire::<ffi::SelectionRequestEvent>(raw) else {
            return;
        };
        let targets_atom = self.atom("TARGETS");
        let incr_atom = self.atom("INCR");
        let property = if event.property == 0 {
            event.target
        } else {
            event.property
        };
        let mut answered = 0;

        if event.target == targets_atom && !self.offers.is_empty() {
            let mut list = vec![targets_atom];
            list.extend(self.offers.iter().map(|offer| offer.target));
            self.set_property(
                event.requestor,
                property,
                ffi::value::ATOM_ATOM,
                32,
                &list
                    .iter()
                    .flat_map(|word| word.to_ne_bytes())
                    .collect::<Vec<u8>>(),
            );
            answered = property;
        } else if let Some(index) = self
            .offers
            .iter()
            .position(|offer| offer.target == event.target)
        {
            let bytes = self.offers[index].bytes.clone();
            match self.chunk {
                Some(chunk) => {
                    let estimate = u32::try_from(bytes.len()).unwrap_or(u32::MAX);
                    self.set_property(
                        event.requestor,
                        property,
                        incr_atom,
                        32,
                        &estimate.to_ne_bytes(),
                    );
                    self.select_property_changes(event.requestor);
                    self.outgoing.push(Outgoing {
                        requestor: event.requestor,
                        property,
                        target: event.target,
                        bytes,
                        offset: 0,
                        chunk,
                        terminated: false,
                    });
                }
                None => {
                    self.set_property(event.requestor, property, event.target, 8, &bytes);
                }
            }
            answered = property;
        }

        let notify = ffi::SelectionNotifyEvent {
            response_type: ffi::event::SELECTION_NOTIFY,
            pad0: 0,
            sequence: 0,
            time: event.time,
            requestor: event.requestor,
            selection: event.selection,
            target: event.target,
            property: answered,
            pad1: [0; 8],
        };
        // SAFETY: the connection and requestor are live, and `SendEvent` reads
        // exactly the 32 bytes a `SelectionNotifyEvent` is.
        unsafe {
            (self.lib.send_event)(
                self.connection,
                0,
                event.requestor,
                0,
                ptr::from_ref(&notify).cast::<c_char>(),
            );
        }
        self.flush();
    }

    fn collect_notify(&mut self, raw: &[u8]) {
        let Some(event) = read_wire::<ffi::SelectionNotifyEvent>(raw) else {
            return;
        };
        if !self.incoming.active {
            return;
        }
        if event.property == 0 {
            self.incoming.active = false;
            self.incoming.done = Some(None);
            return;
        }
        let incr = self.atom("INCR");
        let Some((type_, bytes)) = self.read_property(event.property) else {
            self.incoming.active = false;
            self.incoming.done = Some(None);
            return;
        };
        if type_ == incr {
            self.incoming.incremental = true;
            self.delete_property(event.property);
            self.flush();
            return;
        }
        self.incoming.active = false;
        self.incoming.done = Some(Some(bytes));
    }

    fn property(&mut self, raw: &[u8]) {
        /// `XCB_PROPERTY_NEW_VALUE`.
        const NEW_VALUE: u8 = 0;
        /// `XCB_PROPERTY_DELETE`.
        const DELETED: u8 = 1;
        let Some(event) = read_wire::<ffi::PropertyNotifyEvent>(raw) else {
            return;
        };
        self.last_time = event.time;

        if event.state == NEW_VALUE && self.incoming.active && self.incoming.incremental {
            let bytes = self
                .read_property(event.atom)
                .map_or_else(Vec::new, |(_, bytes)| bytes);
            self.delete_property(event.atom);
            self.flush();
            if bytes.is_empty() {
                self.incoming.active = false;
                self.incoming.done = Some(Some(core::mem::take(&mut self.incoming.bytes)));
            } else {
                self.incoming.bytes.extend_from_slice(&bytes);
            }
            return;
        }

        if event.state == DELETED
            && let Some(index) = self
                .outgoing
                .iter()
                .position(|out| out.requestor == event.window && out.property == event.atom)
        {
            self.push_chunk(index);
        }
    }

    fn push_chunk(&mut self, index: usize) {
        let (requestor, property, target, chunk, finished) = {
            let out = &mut self.outgoing[index];
            if out.terminated {
                (out.requestor, out.property, out.target, Vec::new(), true)
            } else if out.offset >= out.bytes.len() {
                out.terminated = true;
                (out.requestor, out.property, out.target, Vec::new(), true)
            } else {
                let end = out.bytes.len().min(out.offset + out.chunk);
                let piece = out.bytes[out.offset..end].to_vec();
                out.offset = end;
                (out.requestor, out.property, out.target, piece, false)
            }
        };
        self.set_property(requestor, property, target, 8, &chunk);
        self.flush();
        if finished {
            self.outgoing.remove(index);
        }
    }

    fn set_property(&self, window: u32, property: u32, type_: u32, format: u8, data: &[u8]) {
        let elements = if format == 32 {
            data.len() / 4
        } else {
            data.len()
        };
        // SAFETY: the connection and window are live, and `data` outlives the
        // call — libxcb copies it into its request buffer before returning.
        unsafe {
            (self.lib.change_property)(
                self.connection,
                ffi::value::PROP_MODE_REPLACE,
                window,
                property,
                type_,
                format,
                u32::try_from(elements).unwrap_or(u32::MAX),
                data.as_ptr().cast::<c_void>(),
            );
        }
    }

    fn delete_property(&self, property: u32) {
        // SAFETY: the connection and window are live.
        unsafe { (self.lib.delete_property)(self.connection, self.window, property) };
    }

    fn select_property_changes(&self, xid: u32) {
        let values = [ffi::event_mask::PROPERTY_CHANGE];
        // SAFETY: the connection is live and the value list has exactly the one
        // word the `EVENT_MASK` bit declares.
        unsafe {
            (self.lib.change_window_attributes)(
                self.connection,
                xid,
                ffi::cw::EVENT_MASK,
                values.as_ptr().cast::<c_void>(),
            );
        }
    }

    /// The screen's root window, which is where a desktop's own properties
    /// live — `_NET_CLIENT_LIST`, `_NET_ACTIVE_WINDOW`, `_NET_SUPPORTING_WM_CHECK`.
    #[must_use]
    pub const fn root(&self) -> u32 {
        self.root
    }

    /// Reads a property on **someone else's** window.
    ///
    /// How the suite checks that a title, a `WM_CLASS` or a `WM_NORMAL_HINTS`
    /// actually reached the server, rather than that the backend believes it
    /// sent one. A window manager would read exactly these bytes.
    #[must_use]
    pub fn window_property(&mut self, xid: u32, name: &str) -> Option<Vec<u8>> {
        let property = self.atom(name);
        self.property_on(xid, property).map(|(_, bytes)| bytes)
    }

    /// Reads a property on the peer's own window, in full.
    fn read_property(&self, property: u32) -> Option<(u32, Vec<u8>)> {
        self.property_on(self.window, property)
    }

    /// Reads any window's property, in full.
    fn property_on(&self, xid: u32, property: u32) -> Option<(u32, Vec<u8>)> {
        // SAFETY: the connection and window are live; `0` is `AnyPropertyType`
        // and a null error pointer discards the error.
        let reply = unsafe {
            let cookie = (self.lib.get_property)(self.connection, 0, xid, property, 0, 0, 1 << 20);
            (self.lib.get_property_reply)(self.connection, cookie, ptr::null_mut())
        };
        if reply.is_null() {
            return None;
        }
        // SAFETY: `reply` is a live reply this call owns, and libxcb guarantees
        // `get_property_value_length` readable bytes at `get_property_value`.
        let answer = unsafe {
            let type_ = (*reply).type_;
            let length = (self.lib.get_property_value_length)(reply);
            let data = (self.lib.get_property_value)(reply);
            let bytes = if length > 0 && !data.is_null() {
                core::slice::from_raw_parts(data.cast::<u8>(), length as usize).to_vec()
            } else {
                Vec::new()
            };
            (type_ == 0)
                .then_some(())
                .map_or(Some((type_, bytes)), |()| None)
        };
        // SAFETY: freed exactly once.
        unsafe { ffi::free_reply(reply) };
        answer
    }
}

impl Drop for Peer {
    fn drop(&mut self) {
        // SAFETY: the connection and window are live, and each is released
        // exactly once.
        unsafe {
            (self.lib.destroy_window)(self.connection, self.window);
            (self.lib.flush)(self.connection);
            (self.lib.disconnect)(self.connection);
        }
    }
}

/// The name a harness prints when [`Peer::new`] fails.
pub const MISSING_XTEST: &CStr = c"libxcb-xtest is required by the X11 e2e harness";
