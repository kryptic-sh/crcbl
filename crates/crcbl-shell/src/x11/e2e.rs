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
//! | Dragging a file onto a window | a file manager | `Peer::xdnd_enter` and the rest of the XDND source half |
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
use std::time::{Duration, Instant};

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

/// The most events one [`Shell::pump`](crate::Shell::pump) of the X11 backend
/// will interpret, re-exported so a test can send more than that without
/// guessing the number.
///
/// A burst past it is the one case where the connection's descriptor stops
/// being the whole truth about what is pending — see
/// [`burst`](Peer::burst).
pub const MAX_EVENTS_PER_PUMP: usize = super::input::MAX_EVENTS_PER_PUMP;

/// The most `INCR` clipboard transfers one selection may be feeding at once.
///
/// The backend's bound, re-exported: the module that owns it is crate-private,
/// and a test that spelled the number instead would go on passing against a cap
/// that had moved. See `selection::MAX_PENDING_WRITES` for why there is one and
/// why the **newest** conversion past it is the one refused.
pub const MAX_PENDING_CLIPBOARD_WRITES: usize = super::selection::MAX_PENDING_WRITES;

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

/// An `INCR` transfer this peer is pulling out of one of its own properties.
///
/// [`Incoming`] covers the one transfer an ordinary conversion starts. A
/// `MULTIPLE` starts one per pair, all on this window at the same time, so
/// those are tracked by property atom instead of by "the read that is
/// outstanding".
struct Pull {
    property: u32,
    bytes: Vec<u8>,
    done: bool,
}

/// What [`Peer::read_clipboard`] has got to.
#[derive(Debug, Default)]
struct Incoming {
    active: bool,
    incremental: bool,
    bytes: Vec<u8>,
    done: Option<Option<Vec<u8>>>,
}

/// One XDND drag this peer is driving, as the **source**.
///
/// The half of the protocol the backend does not implement and never will —
/// see `x11::xdnd` — so it is here, because a drop needs a source and there is
/// no other one on a headless display. Every field is what a file manager
/// would have: the window being dragged over, the version announced, the types
/// offered, and what the target answered.
struct DragSource {
    /// The window the drag is over. Every message is addressed here.
    target: u32,
    /// The version announced in `XdndEnter`'s high byte.
    version: u8,
    /// The last `XdndStatus`: whether the target accepted, and with what
    /// action.
    status: Option<(bool, u32)>,
    /// How many `XdndStatus` messages have arrived.
    ///
    /// A target that answers a position with silence is a target a real source
    /// treats as busy, so "it answered every one" is a property worth counting
    /// rather than merely observing once.
    statuses: u32,
    /// The `XdndFinished`: whether the drop was taken, and the action reported.
    finished: Option<(bool, u32)>,
    /// The timestamp [`Peer::xdnd_drop`] stamped `XdndDrop` with.
    dropped_at: Option<u32>,
    /// The timestamp the target's `ConvertSelection` quoted on `XdndSelection`.
    ///
    /// Recorded rather than merely honoured, because the specification requires
    /// it to be [`dropped_at`](Self::dropped_at) and a server answers a
    /// conversion stamped `CurrentTime` just as readily — so a target that
    /// quotes the wrong time is invisible to every other assertion here. It is
    /// the last one seen: an `INCR` transfer is a single conversion followed by
    /// property deletes, not a conversion per chunk.
    converted_at: Option<u32>,
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
    /// What this peer is offering on `XdndSelection`, kept apart from the
    /// clipboard's [`offers`](Self::offers).
    ///
    /// Two selections, two offer lists: a test that drags a file while the
    /// clipboard holds text must not have one conversion answered out of the
    /// other's payload, which is exactly the mix-up the backend's own routing
    /// is written to avoid.
    xdnd_offers: Vec<Offer>,
    /// The drag being driven, if any.
    drag: Option<DragSource>,
    /// How many `SelectionRequest`s have arrived on `XdndSelection`.
    ///
    /// The signal that a target has *started* fetching a drop's payload, which
    /// is otherwise invisible from outside the shell — and the only thing a
    /// test can wait for when what it wants to interrupt is a transfer already
    /// under way.
    xdnd_conversions: u32,
    outgoing: Vec<Outgoing>,
    incoming: Incoming,
    /// `INCR` transfers this peer is pulling, keyed by the property each one
    /// arrives in. See [`Pull`].
    pulls: Vec<Pull>,
    /// How many `PropertyNotify` events have been handled.
    ///
    /// **What [`server_time`](Self::server_time) waits on**, rather than the
    /// timestamp changing. X11 timestamps are milliseconds, so two appends
    /// inside one millisecond are answered with the same number — and a wait
    /// for a *different* value then never ends, because only one append was
    /// sent and no further event is coming. Counting the answers asks the
    /// question that was meant: has this append been answered yet.
    property_notifies: u64,
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
            xdnd_offers: Vec::new(),
            drag: None,
            xdnd_conversions: 0,
            outgoing: Vec::new(),
            incoming: Incoming::default(),
            pulls: Vec::new(),
            property_notifies: 0,
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
    /// **Bounded by a deadline, not by a turn count, and it waits for the
    /// append's own answer.** `service` does not block, so a fixed number of
    /// turns is spent in microseconds and bounds nothing: under load this
    /// returned before the `PropertyNotify` arrived and handed back the initial
    /// `last_time` of zero, which *is* `CurrentTime` — the one answer this
    /// function exists to avoid, returned silently while every assertion still
    /// passed. Measured 2026-08-31: one failure in twelve runs of the XDND drop
    /// test with the machine busy, none in twelve with it idle.
    ///
    /// It waits on [`property_notifies`](Self::property_notifies) rather than
    /// on the timestamp changing, because two appends inside one millisecond
    /// are answered with the same number and a wait for a different one would
    /// never end.
    fn server_time(&mut self) -> u32 {
        /// Long enough for a server on a loaded machine; a local X socket
        /// answers in microseconds, so this is never waited out in practice.
        const DEADLINE: Duration = Duration::from_secs(2);

        let stamp = self.atom("CRCBL_E2E_TIME");
        let answered = self.property_notifies;
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
        let started = Instant::now();
        while self.property_notifies == answered {
            assert!(
                started.elapsed() < DEADLINE,
                "the X server did not answer a property append with a \
                 PropertyNotify within {DEADLINE:?}; without one every \
                 timestamp this harness sends would be CurrentTime, which is \
                 older than everything"
            );
            self.service();
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

    /// Sends `count` client messages straight at `xid`'s client.
    ///
    /// The one thing a test cannot produce from inside the shell: **more
    /// events than one `pump` will interpret**. `SendEvent` with an empty
    /// event mask delivers to the client that created the window whatever it
    /// selected for, so `count` is exactly how many events that client's
    /// connection receives.
    ///
    /// The message type is this harness's own atom, which nothing handles: the
    /// backend reads the type, finds it is not `WM_PROTOCOLS`, and drops it.
    /// The burst is therefore silent — it moves the event *count* without
    /// adding a single [`ShellEvent`](crate::ShellEvent) to whatever the test
    /// is really asserting on.
    pub fn burst(&mut self, xid: u32, count: usize) {
        let flood = self.atom("CRCBL_E2E_BURST");
        let message = ffi::ClientMessageEvent {
            response_type: ffi::event::CLIENT_MESSAGE,
            format: 32,
            sequence: 0,
            window: xid,
            type_: flood,
            data: [0; 5],
        };
        for _ in 0..count {
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
        }
        self.flush();
        // A flush only hands the requests to the socket. A **round trip** is
        // what makes the server have executed them: it answers this connection
        // in request order, so a reply to something sent afterwards cannot
        // arrive until every message above has been delivered. Without it the
        // caller would be racing the server for its own burst.
        let _ = self.window_origin(xid);
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

    /// The window's parent, or `None` if the server answered with an error.
    ///
    /// **What a test uses it for: telling "not managed" from "not managed
    /// *yet*".** A reparenting window manager puts a client window inside a
    /// frame, so a window whose parent is still the root has not been placed —
    /// and until it has been,
    /// [`window_origin`](Self::window_origin) answers `(0, 0)`, which is
    /// indistinguishable from the legitimate unmanaged answer. A test that
    /// reads the origin in that window computes a screen coordinate that is
    /// really a window-relative one, and the failure lands somewhere else
    /// entirely.
    #[must_use]
    pub fn window_parent(&self, xid: u32) -> Option<u32> {
        // SAFETY: the connection and window are live; a null error pointer
        // discards the error and a null reply is handled below.
        let reply = unsafe {
            let cookie = (self.lib.query_tree)(self.connection, xid);
            (self.lib.query_tree_reply)(self.connection, cookie, ptr::null_mut())
        };
        if reply.is_null() {
            return None;
        }
        // SAFETY: `reply` is a live reply this call owns.
        let parent = unsafe { (*reply).parent };
        // SAFETY: freed exactly once.
        unsafe { ffi::free_reply(reply) };
        Some(parent)
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
        self.offers = self.intern_offers(payloads);
        self.set_owner("CLIPBOARD", self.window);
    }

    /// Interns each target name and copies each payload.
    fn intern_offers(&mut self, payloads: &[(&str, &[u8])]) -> Vec<Offer> {
        payloads
            .iter()
            .map(|(name, bytes)| Offer {
                target: self.atom(name),
                bytes: (*bytes).to_vec(),
            })
            .collect()
    }

    /// Gives up the selection, so nobody owns it.
    pub fn release_clipboard(&mut self) {
        self.offers.clear();
        self.outgoing.clear();
        self.set_owner("CLIPBOARD", ffi::value::NONE);
    }

    // -----------------------------------------------------------------------
    // XDND, the source half
    // -----------------------------------------------------------------------

    /// The XDND version a window publishes, or `None` when it publishes none.
    ///
    /// The first thing any real source reads, and the whole of the target's
    /// opt-in: a window without `XdndAware` is never sent an `XdndEnter` by
    /// anything, so this is also how the suite checks that
    /// `WindowDesc::accept_drops` being off means *invisible to a drag* rather
    /// than merely refused.
    #[must_use]
    pub fn xdnd_version(&mut self, xid: u32) -> Option<u8> {
        let aware = self.atom("XdndAware");
        let (_, bytes) = self.property_on(xid, aware)?;
        let word = bytes.get(..4)?;
        Some(u32::from_ne_bytes([word[0], word[1], word[2], word[3]]) as u8)
    }

    /// Begins a drag over `xid`, announcing `version` and offering `types`.
    ///
    /// `payloads` is what a conversion on `XdndSelection` will be answered
    /// with, by target name — separate from `types` so a test can announce a
    /// format it then refuses to convert, and so a list longer than the three
    /// atoms `XdndEnter` carries inline can be announced through the
    /// `XdndTypeList` property, which is a branch of the target's parsing that
    /// nothing else reaches.
    pub fn xdnd_enter(
        &mut self,
        xid: u32,
        version: u8,
        types: &[&str],
        payloads: &[(&str, &[u8])],
    ) {
        self.chunk = None;
        self.begin_drag(xid, version, types, payloads);
    }

    /// As [`xdnd_enter`](Self::xdnd_enter), but answers the payload conversion
    /// with an `INCR` transfer of `chunk`-byte pieces.
    ///
    /// The same device [`own_clipboard_incrementally`](Self::own_clipboard_incrementally)
    /// uses, for the same reason: it makes a transfer that would otherwise
    /// complete inside one `pump` take a dozen of them, so a test can act while
    /// one is still in flight.
    pub fn xdnd_enter_incrementally(
        &mut self,
        xid: u32,
        version: u8,
        types: &[&str],
        payloads: &[(&str, &[u8])],
        chunk: usize,
    ) {
        self.chunk = Some(chunk.max(1));
        self.begin_drag(xid, version, types, payloads);
    }

    /// How many conversions of `XdndSelection` this peer has been asked for.
    #[must_use]
    pub const fn xdnd_conversions(&self) -> u32 {
        self.xdnd_conversions
    }

    fn begin_drag(&mut self, xid: u32, version: u8, types: &[&str], payloads: &[(&str, &[u8])]) {
        self.xdnd_offers = self.intern_offers(payloads);
        self.set_owner("XdndSelection", self.window);
        let atoms: Vec<u32> = types.iter().map(|name| self.atom(name)).collect();
        // Bit 0 of the second word says the inline three are not the whole
        // list; the property is where the rest is.
        let long = atoms.len() > 3;
        if long {
            let type_list = self.atom("XdndTypeList");
            let bytes: Vec<u8> = atoms.iter().flat_map(|atom| atom.to_ne_bytes()).collect();
            self.set_property(self.window, type_list, ffi::value::ATOM_ATOM, 32, &bytes);
        }
        self.drag = Some(DragSource {
            target: xid,
            version,
            status: None,
            statuses: 0,
            finished: None,
            dropped_at: None,
            converted_at: None,
        });
        let flags = (u32::from(version) << 24) | u32::from(long);
        let enter = self.atom("XdndEnter");
        self.send_xdnd(
            xid,
            enter,
            [
                self.window,
                flags,
                atoms.first().copied().unwrap_or(0),
                atoms.get(1).copied().unwrap_or(0),
                atoms.get(2).copied().unwrap_or(0),
            ],
        );
    }

    /// Reports the pointer at `(x, y)` in **root** coordinates.
    ///
    /// Root coordinates because that is what the protocol carries and what a
    /// source actually knows; turning them into window-relative ones is the
    /// target's job, and is the part of it worth testing.
    pub fn xdnd_position(&mut self, x: i16, y: i16) {
        let Some(target) = self.drag.as_ref().map(|drag| drag.target) else {
            return;
        };
        let action = self.atom("XdndActionCopy");
        let position = self.atom("XdndPosition");
        let time = self.last_time;
        self.send_xdnd(
            target,
            position,
            [
                self.window,
                0,
                (u32::from(x as u16) << 16) | u32::from(y as u16),
                time,
                action,
            ],
        );
    }

    /// Leaves without dropping.
    pub fn xdnd_leave(&mut self) {
        let Some(target) = self.drag.as_ref().map(|drag| drag.target) else {
            return;
        };
        let leave = self.atom("XdndLeave");
        self.send_xdnd(target, leave, [self.window, 0, 0, 0, 0]);
    }

    /// Releases the button, which is what asks the target to fetch the data.
    ///
    /// The timestamp is a real server one rather than `CurrentTime`, provoked
    /// by the zero-length property append this harness uses everywhere it needs
    /// a stamp. The specification requires the target's `ConvertSelection` to
    /// quote it, so a harness that sent `CurrentTime` would let a backend that
    /// quoted anything at all pass; [`xdnd_drop_times`](Self::xdnd_drop_times)
    /// is what actually compares the two.
    pub fn xdnd_drop(&mut self) {
        let Some(target) = self.drag.as_ref().map(|drag| drag.target) else {
            return;
        };
        let time = self.server_time();
        let drop = self.atom("XdndDrop");
        self.send_xdnd(target, drop, [self.window, 0, time, 0, 0]);
        if let Some(drag) = self.drag.as_mut() {
            drag.dropped_at = Some(time);
        }
    }

    /// The last `XdndStatus`: whether the target accepted, and the action atom.
    #[must_use]
    pub fn xdnd_status(&self) -> Option<(bool, u32)> {
        self.drag.as_ref().and_then(|drag| drag.status)
    }

    /// How many `XdndStatus` messages have come back.
    #[must_use]
    pub fn xdnd_statuses(&self) -> u32 {
        self.drag.as_ref().map_or(0, |drag| drag.statuses)
    }

    /// The `XdndFinished`: whether the drop was taken, and the action reported.
    #[must_use]
    pub fn xdnd_finished(&self) -> Option<(bool, u32)> {
        self.drag.as_ref().and_then(|drag| drag.finished)
    }

    /// The timestamp of the drop, and the one the target's `ConvertSelection`
    /// quoted — equal exactly when the target obeyed the specification.
    ///
    /// Both `None` until a drop has been sent and its conversion answered.
    ///
    /// **Reported as a pair rather than checked here**, because a server
    /// answers a conversion stamped `CurrentTime` exactly as readily as one
    /// stamped correctly — so nothing the protocol does can tell a target that
    /// quoted the wrong time from one that did not, and the comparison has to
    /// be an assertion in a test that knows both numbers.
    #[must_use]
    pub fn xdnd_drop_times(&self) -> (Option<u32>, Option<u32>) {
        self.drag
            .as_ref()
            .map_or((None, None), |drag| (drag.dropped_at, drag.converted_at))
    }

    /// The version this drag announced, so a test can state it once.
    #[must_use]
    pub fn xdnd_announced_version(&self) -> Option<u8> {
        self.drag.as_ref().map(|drag| drag.version)
    }

    /// Sends one XDND client message to the target.
    ///
    /// Mask `0`, which delivers to the client that created the window — the
    /// routing every XDND message uses, and the only one that reaches a target
    /// whose window is not selecting anything in particular.
    fn send_xdnd(&self, xid: u32, type_: u32, data: [u32; 5]) {
        let message = ffi::ClientMessageEvent {
            response_type: ffi::event::CLIENT_MESSAGE,
            format: 32,
            sequence: 0,
            window: xid,
            type_,
            data,
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
    fn set_owner(&mut self, selection: &str, owner: u32) {
        let clipboard = self.atom(selection);
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

    /// Starts one `MULTIPLE` conversion of every `(target, property)` pair.
    ///
    /// ICCCM's batch form, from the requestor's side: the pairs go on this
    /// peer's own window as an `ATOM_PAIR` list, the request names that
    /// property, and the owner writes each conversion into the property its
    /// pair names. [`take_read`](Self::take_read) then yields the list *as the
    /// owner left it* — a pair whose property atom came back `0` is one the
    /// owner refused — and [`own_property`](Self::own_property) reads each
    /// conversion out.
    pub fn read_clipboard_multiple(&mut self, pairs: &[(&str, &str)]) {
        let clipboard = self.atom("CLIPBOARD");
        let multiple = self.atom("MULTIPLE");
        let atom_pair = self.atom("ATOM_PAIR");
        let list = self.atom("PEER_MULTIPLE");
        let words: Vec<u32> = pairs
            .iter()
            .flat_map(|(target, property)| [self.atom(target), self.atom(property)])
            .collect();
        let bytes: Vec<u8> = words.iter().flat_map(|word| word.to_ne_bytes()).collect();
        // Every property the owner will write into is cleared first, so a value
        // read back afterwards cannot be one this peer left there itself.
        for (_, property) in pairs {
            let property = self.atom(property);
            self.delete_property(property);
        }
        self.set_property(self.window, list, atom_pair, 32, &bytes);
        self.incoming = Incoming {
            active: true,
            ..Incoming::default()
        };
        // SAFETY: the connection, window and atoms are live.
        unsafe {
            (self.lib.convert_selection)(
                self.connection,
                self.window,
                clipboard,
                multiple,
                list,
                ffi::value::CURRENT_TIME,
            );
        }
        self.flush();
    }

    /// Starts pulling the `INCR` transfer the owner left in property `name`.
    ///
    /// The counterpart to
    /// [`read_clipboard_multiple`](Self::read_clipboard_multiple) for a batch
    /// whose conversions were each too large for one `ChangeProperty`: the
    /// property holds an `INCR` header rather than the data, and the transfer
    /// only advances once the requestor deletes it. Several may run at once —
    /// which is the whole reason these are keyed by property while
    /// [`read_clipboard`](Self::read_clipboard) tracks a single outstanding
    /// read.
    ///
    /// The header is deleted rather than read: its value is a size estimate and
    /// appending it would corrupt the payload.
    pub fn pull_incremental(&mut self, name: &str) {
        let property = self.atom(name);
        self.pulls.retain(|pull| pull.property != property);
        self.pulls.push(Pull {
            property,
            bytes: Vec::new(),
            done: false,
        });
        self.delete_property(property);
        self.flush();
    }

    /// The bytes of a [`pull_incremental`](Self::pull_incremental) that has
    /// reached its zero-length terminator.
    ///
    /// `None` while the transfer is still running, so a test polls this the way
    /// it polls [`take_read`](Self::take_read).
    #[must_use]
    pub fn take_pull(&mut self, name: &str) -> Option<Vec<u8>> {
        let property = self.atom(name);
        let index = self
            .pulls
            .iter()
            .position(|pull| pull.property == property && pull.done)?;
        Some(self.pulls.remove(index).bytes)
    }

    /// A property on this peer's own window, by name, as raw bytes.
    ///
    /// `None` for a property that is not there — which, after a `MULTIPLE`, is
    /// what a target the owner refused looks like.
    #[must_use]
    pub fn own_property(&mut self, name: &str) -> Option<Vec<u8>> {
        let property = self.atom(name);
        self.read_property(property).map(|(_, bytes)| bytes)
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
            ffi::event::CLIENT_MESSAGE => self.collect_xdnd(raw),
            ffi::event::SELECTION_CLEAR => {
                let cleared =
                    read_wire::<ffi::SelectionClearEvent>(raw).map_or(0, |event| event.selection);
                if cleared == self.atom("XdndSelection") {
                    self.xdnd_offers.clear();
                } else {
                    self.offers.clear();
                }
                self.outgoing.clear();
            }
            _ => {}
        }
    }

    /// Collects an `XdndStatus` or an `XdndFinished` from the target.
    fn collect_xdnd(&mut self, raw: &[u8]) {
        let Some(event) = read_wire::<ffi::ClientMessageEvent>(raw) else {
            return;
        };
        let status = self.atom("XdndStatus");
        let finished = self.atom("XdndFinished");
        let Some(drag) = self.drag.as_mut() else {
            return;
        };
        if event.type_ == status {
            drag.statuses += 1;
            drag.status = Some((event.data[1] & 1 != 0, event.data[4]));
        } else if event.type_ == finished {
            drag.finished = Some((event.data[1] & 1 != 0, event.data[2]));
        }
    }

    fn answer_request(&mut self, raw: &[u8]) {
        let Some(event) = read_wire::<ffi::SelectionRequestEvent>(raw) else {
            return;
        };
        let targets_atom = self.atom("TARGETS");
        let incr_atom = self.atom("INCR");
        // Which selection is being converted decides which payload answers it.
        // Without this a drag and a clipboard offer held at the same time would
        // answer each other's conversions, and the suite could not tell a
        // backend that routes them correctly from one that does not.
        let xdnd_selection = self.atom("XdndSelection");
        let offers = if event.selection == xdnd_selection {
            self.xdnd_conversions += 1;
            if let Some(drag) = self.drag.as_mut() {
                drag.converted_at = Some(event.time);
            }
            core::mem::take(&mut self.xdnd_offers)
        } else {
            core::mem::take(&mut self.offers)
        };
        self.answer_from(&event, &offers, targets_atom, incr_atom);
        if event.selection == xdnd_selection {
            self.xdnd_offers = offers;
        } else {
            self.offers = offers;
        }
    }

    /// The body of [`answer_request`](Self::answer_request), against one
    /// selection's offers.
    fn answer_from(
        &mut self,
        event: &ffi::SelectionRequestEvent,
        offers: &[Offer],
        targets_atom: u32,
        incr_atom: u32,
    ) {
        let property = if event.property == 0 {
            event.target
        } else {
            event.property
        };
        let mut answered = 0;

        if event.target == targets_atom && !offers.is_empty() {
            let mut list = vec![targets_atom];
            list.extend(offers.iter().map(|offer| offer.target));
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
        } else if let Some(index) = offers.iter().position(|offer| offer.target == event.target) {
            let bytes = offers[index].bytes.clone();
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
        self.property_notifies = self.property_notifies.wrapping_add(1);

        // A `MULTIPLE` starts one transfer per pair, so these are matched on
        // the property the chunk arrived in rather than on there being a single
        // outstanding read. Checked first for the same reason: the branch below
        // does not look at the property at all.
        if event.state == NEW_VALUE
            && let Some(index) = self
                .pulls
                .iter()
                .position(|pull| pull.property == event.atom && !pull.done)
        {
            let bytes = self
                .read_property(event.atom)
                .map_or_else(Vec::new, |(_, bytes)| bytes);
            self.delete_property(event.atom);
            self.flush();
            if bytes.is_empty() {
                self.pulls[index].done = true;
            } else {
                self.pulls[index].bytes.extend_from_slice(&bytes);
            }
            return;
        }

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

    /// How deep [`find_window`](Self::find_window) descends before giving up.
    ///
    /// A reparenting window manager puts the client window under a frame, so
    /// the walk has to go at least one level past the root to see a managed
    /// window at all. The bound exists so a pathological tree cannot make the
    /// harness walk it forever.
    const FIND_WINDOW_MAX_DEPTH: usize = 4;

    /// Finds a window by the instance half of its `WM_CLASS`.
    ///
    /// How the harness closes the sandbox cleanly instead of SIGTERMing it: a
    /// `WM_DELETE_WINDOW` has to name the client's window, and that XID is the
    /// backend's to know, not the harness's. So this walks the tree from the
    /// root, reading each window's `WM_CLASS` until one matches — the same read
    /// a desktop environment makes to match a window to its `.desktop` file.
    ///
    /// The walk descends: a reparenting window manager puts the client window
    /// under a frame, so the root's children are frames and the client windows
    /// are their children. Bounded by `FIND_WINDOW_MAX_DEPTH` (private, so not
    /// linkable), and `None` when nothing matches or the server answers with an
    /// error.
    #[must_use]
    pub fn find_window(&mut self, wm_class: &str) -> Option<u32> {
        self.find_window_at(self.root, wm_class, 0)
    }

    fn find_window_at(&mut self, xid: u32, wm_class: &str, depth: usize) -> Option<u32> {
        if depth > Self::FIND_WINDOW_MAX_DEPTH {
            return None;
        }
        // SAFETY: the connection and window are live; a null error pointer
        // discards the error and a null reply is handled below.
        let reply = unsafe {
            let cookie = (self.lib.query_tree)(self.connection, xid);
            (self.lib.query_tree_reply)(self.connection, cookie, ptr::null_mut())
        };
        if reply.is_null() {
            return None;
        }
        // The children live inside the reply's allocation, so they are copied
        // out before the reply is freed below.
        let mut children = Vec::new();
        // SAFETY: `reply` is a live reply this call owns, and libxcb guarantees
        // `query_tree_children_length` readable windows at
        // `query_tree_children`.
        unsafe {
            let list = (self.lib.query_tree_children)(reply);
            let length = (self.lib.query_tree_children_length)(reply);
            if length > 0 && !list.is_null() {
                children.extend_from_slice(core::slice::from_raw_parts(list, length as usize));
            }
        }
        // SAFETY: freed exactly once.
        unsafe { ffi::free_reply(reply) };

        for child in children {
            if self
                .window_property(child, "WM_CLASS")
                .is_some_and(|bytes| wm_class_instance_is(&bytes, wm_class))
            {
                return Some(child);
            }
            if let Some(found) = self.find_window_at(child, wm_class, depth + 1) {
                return Some(found);
            }
        }
        None
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

/// Whether a `WM_CLASS` value has `instance` as its first NUL-terminated
/// segment.
///
/// `WM_CLASS` is two NUL-terminated strings — `"instance\0class\0"` — and a
/// desktop matches the first one against its `.desktop` files, which is the
/// half [`Peer::find_window`] compares. A value without a terminator is
/// malformed and matches nothing.
#[must_use]
fn wm_class_instance_is(bytes: &[u8], instance: &str) -> bool {
    CStr::from_bytes_until_nul(bytes).is_ok_and(|class| class.to_bytes() == instance.as_bytes())
}
