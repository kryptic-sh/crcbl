//! The X11 backend: connection, window lifecycle, EWMH, RandR, XKB, XI2 raw
//! motion, pointer grabs and the selection protocol.
//!
//! `docs/plan/15-windowing.md`'s Linux policy applied to the other half:
//! **libxcb owns the connection and the wire encoding; the request/event layer
//! above it — core, EWMH atoms, RandR, XKB — is ours.** [`ffi`] is the first
//! part, `dlopen`'d for the reasons that module states. [`atoms`], [`keys`] and
//! [`selection`] are the second. [`xkb`](crate::linux::xkb) and
//! [`keymap`](crate::linux::keymap) are shared with the Wayland backend,
//! because a keymap is a Linux fact rather than a protocol one.
//!
//! # What is in this backend, and what was cut
//!
//! | Area | State |
//! | --- | --- |
//! | Window lifecycle: create, map, unmap, destroy, title, `WM_CLASS`, `WM_NORMAL_HINTS`, `WM_DELETE_WINDOW` | complete |
//! | Windowed ↔ borderless through `_NET_WM_STATE_FULLSCREEN`, on a named monitor | complete |
//! | Monitors: RandR CRTC enumeration, geometry, refresh, primary, work area | complete |
//! | Keyboard: XKB keymap from the server, keysyms, text, modifiers, repeat flag | complete |
//! | Pointer: motion, buttons, wheel, enter/leave, grabs, warp | complete |
//! | XI2 raw relative motion | complete |
//! | Clipboard: `TARGETS`, `INCR` both directions, own and read | complete |
//! | **Drag and drop (XDND)** | **cut — see below** |
//! | Key repeat synthesis | not needed — the X server does it |
//! | Themed cursor shapes | recorded, not applied — same gap the Wayland backend documents |
//!
//! ## What was cut, and why that seam
//!
//! **XDND.** [`ShellCaps::DRAG_DROP`] is clear on this backend and
//! [`ShellEvent::DroppedFile`] is never emitted. XDND is a five-message
//! handshake (`XdndEnter`/`Position`/`Status`/`Drop`/`Finished`) layered on top
//! of a *second* selection (`XdndSelection`) with its own version negotiation
//! and its own timestamp rules — in other words, the whole of
//! [`selection`] again with a different trigger and a protocol on top. On
//! Wayland it was its own slice (P0.5c) for a reason, and the honest choice
//! here was a smaller backend that is tested against a real server over a
//! larger one that is half-verified. The clipboard stayed because [obligations
//! 4, 5 and 6](Shell) are stated in terms of it and could not otherwise be
//! answered.
//!
//! # What X11 does that neither Wayland nor `HeadlessShell` models
//!
//! In the order they cost time:
//!
//! 1. **A window has a size before anyone asks.** `CreateWindow` takes width
//!    and height and the server keeps them; there is no configure round trip
//!    and no "0 × 0 means you choose". The P0.4 contract still holds and is
//!    still honest — [`WindowState::size`] is `None` until the first
//!    [`Resized`](ShellEvent::Resized) — but the wait is **one
//!    [`pump`](Shell::pump)**, not a round trip, because the answer was already
//!    known when [`create_window`](Shell::create_window) returned. Nothing is
//!    delayed to look symmetrical with Wayland; see
//!    [`create_window`](X11Shell::create_window).
//! 2. **The event clock has an origin nobody can name.** Wayland stamps input
//!    with `CLOCK_MONOTONIC` milliseconds, so a backend knows the origin a
//!    priori. X11 stamps with milliseconds **since the server started**, which
//!    is a clock this process has never seen, on a machine that may not even be
//!    this one. So the offset has to be *calibrated* — see [`TimeBase`] — and
//!    on a remote connection that calibration silently absorbs the network
//!    latency, which is the honest answer rather than a wrong one.
//! 3. **The window manager is a separate program that may not exist.** Not a
//!    compositor detail: on X11 nothing about `_NET_WM_STATE`, reparenting,
//!    focus policy, or size hints happens unless some *other process* is
//!    running and implementing it. Under bare `Xvfb` there is no window
//!    manager, `WM_NORMAL_HINTS` is a property nobody reads, and
//!    `_NET_WM_STATE_FULLSCREEN` is a client message sent into the void. This
//!    backend detects it through `_NET_SUPPORTING_WM_CHECK` and latches
//!    [`ASPECT_HINT_HONORED`](ShellCaps::ASPECT_HINT_HONORED) and
//!    [`SERVER_DECORATIONS`](ShellCaps::SERVER_DECORATIONS) from the answer.
//!    Neither Wayland nor [`HeadlessShell`](crate::HeadlessShell) has anything
//!    like this: a Wayland compositor *is* the window manager and cannot be
//!    absent.
//! 4. **Hiding a window is real, and reversible.** `UnmapWindow` and
//!    `MapWindow` are a matched pair, so [`set_visible`](Shell::set_visible)
//!    genuinely works here — where the Wayland backend records intent it cannot
//!    act on until there is a renderer, because a Wayland surface is mapped
//!    exactly while it has a buffer.
//! 5. **There is exactly one device-pixel coordinate space.** Every monitor is
//!    a rectangle in the screen's pixels, so [`MonitorInfo::bounds`] tiles and
//!    means something — which the type's own documentation says is *not* true
//!    on Wayland at mixed scales. That is what makes
//!    [`DisplayMode::Borderless`] with a named monitor implementable, and it is
//!    why [`WINDOW_POSITION`](ShellCaps::WINDOW_POSITION) is set here for the
//!    first time; see [`caps`](X11Shell::caps) for exactly what that bit is
//!    and is not claiming.
//! 6. **Scale is a string in a property, not a protocol.** X11 has no
//!    per-monitor scale factor and no fractional-scale extension. What every
//!    toolkit actually reads is `Xft.dpi` out of the `RESOURCE_MANAGER`
//!    property on the root window — an X resource database, parsed as text.
//!    So the scale is global, it is a user preference rather than a hardware
//!    fact, and a monitor's physical millimetres (which RandR *does* report)
//!    are deliberately not used for it: deriving scale from DPI is how
//!    applications end up 1.6× on a laptop panel the user wanted at 1×.
//! 7. **The clipboard's owner is another process on the same socket.** A
//!    `SelectionRequest` for data we own arrives on the same connection a
//!    `ConvertSelection` of ours is waiting on, so any synchronous read of our
//!    own selection self-deadlocks — the failure P0.5c predicted would be worse
//!    here, and it is. [`selection`] states the shape that avoids it.
//! 8. **A stalled `INCR` transfer has no end.** The one obligation X11 did *not*
//!    get for free; see [`selection`]'s deadline discussion and the report in
//!    [`clipboard_request`](Shell::clipboard_request).
//!
//! # Decision: the server generates key repeats, so this backend does not
//!
//! The exact opposite of the Wayland backend, and for the exact opposite
//! reason. `wl_keyboard.repeat_info` hands a client a rate and a delay and no
//! events, so the client must synthesize. X11 auto-repeat is a *server*
//! feature: a held key produces `KeyPress`/`KeyRelease` pairs at the server's
//! configured rate, with real server timestamps, and there is nothing to
//! synthesize.
//!
//! What there *is* to do is recognize them, because the naive reading is that
//! the user is tapping the key very fast. A repeat is a `KeyPress` whose
//! keycode is already held, and [`ShellEvent::Key`] carries
//! [`repeat: true`](ShellEvent::Key) for it. The intervening `KeyRelease` is
//! **not delivered**, because it is not a release — the key is still down, and
//! forwarding it would make a jump button fire on every repeat interval.
//!
//! Getting it not to arrive at all is
//! [`Conn::setup_detectable_auto_repeat`], one XKB request at connect time.
//! The alternative every toolkit reaches for first — a one-event lookahead for
//! a `KeyRelease`/`KeyPress` pair at the same timestamp — is kept in
//! [`input`] as the fallback, and is documented there as unsound on its own:
//! the two halves are not guaranteed to be read together, and under load they
//! are not.

mod atoms;
pub mod ffi;
pub mod keys;
pub mod selection;

/// Test-only: a second connection that drives the shell from the outside.
///
/// Behind the `x11-e2e` feature because it is scaffolding, not shell. See the
/// module's own docs for why the end-to-end suite cannot do without it.
#[cfg(feature = "x11-e2e")]
pub mod e2e;

use core::ffi::{c_int, c_void};
use core::ptr::{self, NonNull};
use core::time::Duration;
use std::collections::VecDeque;

use crcbl_core::input::DeviceId;
use crcbl_core::{EventTime, KeyCode, Pool};

use crate::linux::xkb;
use crate::{
    ClipboardContent, ClipboardOffer, ClipboardRequestId, CloseReply, CursorIcon, DisplayMode,
    LogicalSize, MimeType, MonitorId, MonitorInfo, PhysicalPoint, PhysicalRect, PhysicalSize,
    PointerMode, ReceivedMime, Shell, ShellBackend, ShellCaps, ShellError, ShellEvent,
    SizeConstraints, WindowConfiguration, WindowDesc, WindowId, WindowState,
};

use atoms::Atoms;
use ffi::{Connection, Lib};
use keys::{SizeHints, WmStateAction};
use selection::{Read, Step, Write};

/// Which keyboard and pointer an event came from.
///
/// Core X11 has exactly one of each — the *master* devices — and this backend
/// reads XInput 2 only for raw motion, not for device enumeration. So these are
/// constants rather than a table, and they are documented as such:
/// [`DeviceId`] exists so the P2 action layer can tell two mice apart, and on
/// this backend it cannot. A consumer that needs per-device input on X11 needs
/// XI2 device events, which is a later slice.
const KEYBOARD_DEVICE: DeviceId = DeviceId(1);
/// See [`KEYBOARD_DEVICE`].
const POINTER_DEVICE: DeviceId = DeviceId(2);

/// The scale a desktop with no `Xft.dpi` runs at.
const DEFAULT_SCALE: f64 = 1.0;
/// `Xft.dpi` is a DPI, and 96 is the number every toolkit calls 1×.
const BASE_DPI: f64 = 96.0;
/// The longest [`wait_events`](Shell::wait_events) may sleep while a selection
/// transfer is outstanding.
///
/// The X socket is in the poll, so a peer that answers wakes the wait
/// immediately; this bounds only the case where nothing happens at all, so that
/// [`selection::TIMEOUT`] is reached instead of slept through.
const TRANSFER_POLL: Duration = Duration::from_millis(50);
/// How far from the centre a locked pointer may drift before it is warped back.
///
/// A fraction of the window rather than a fixed margin, so the behaviour is the
/// same on a 640×480 window and a 4K one. See
/// [`set_pointer_mode`](Shell::set_pointer_mode).
const LOCK_RECENTRE_FRACTION: f64 = 0.25;

// ---------------------------------------------------------------------------
// Timestamps
// ---------------------------------------------------------------------------

/// Puts X11 server timestamps on the engine's clock.
///
/// # Why this is harder than Wayland's
///
/// [`EventTime`] requires "a duration measured from the same origin as the
/// [`TimeSource`](crcbl_core::TimeSource) driving
/// [`FrameClock::update`](crcbl_core::FrameClock::update)". The Wayland backend
/// gets most of the way there for free, because `wl_pointer` and `wl_keyboard`
/// timestamps are `CLOCK_MONOTONIC` milliseconds — the same clock, just a
/// different zero.
///
/// X11 timestamps are milliseconds **since the X server started**. That is a
/// clock this process has never read and cannot read: there is no request for
/// "what is your uptime", the server may have been running for weeks, and over
/// a TCP connection it may be on another machine with another clock entirely.
/// So the offset between the two clocks has to be **calibrated**, and the only
/// calibration available is the first event that carries a timestamp: at the
/// moment it is received, `now` and `event.time` are the same instant to within
/// the time it took to arrive.
///
/// What that buys and what it costs:
///
/// * **Durations between two events are exact.** Both are on the server's
///   clock, and the offset cancels. That is what
///   `docs/plan/19-input.md`'s tap-versus-hold evaluation subtracts, so the
///   part that has to be right is right.
/// * **The absolute offset carries one connection's worth of latency.** On a
///   local socket that is microseconds. On a remote display it is the network
///   round trip, and input-to-photon latency measured across it would be
///   understated by exactly that much. Stating it is the honest option;
///   pretending otherwise would put a wrong number in a measurement whose whole
///   purpose is to be right.
///
/// The 32-bit wrap is handled the same way as Wayland's, because it is the same
/// wrap: 49.7 days of server uptime, which is *routine* for an X server in a
/// way it is not for a compositor session.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct TimeBase {
    /// `CLOCK_MONOTONIC` nanoseconds at the engine epoch.
    epoch_nanos: u64,
    /// `CLOCK_MONOTONIC` nanoseconds that server-millisecond zero corresponds
    /// to, once an event has been seen. `None` until then.
    ///
    /// **Signed on purpose.** The server's clock counts from *its* start, on a
    /// machine that may not be this one, so millisecond zero can easily fall
    /// before this machine booted — an X server up longer than we have been is
    /// the ordinary remote-display case, and a freshly booted machine makes it
    /// the ordinary local case too. Unsigned arithmetic here saturates to zero
    /// and forwards every event by the difference; CI caught exactly that,
    /// reporting an event 3950s in the future on a runner with 49s of uptime.
    server_origin_nanos: Option<i128>,
    /// The widest server timestamp seen so far, for wrap detection.
    high_water_millis: u64,
}

impl TimeBase {
    /// An epoch of "now", which is the shell's creation.
    fn now() -> Self {
        Self {
            epoch_nanos: ffi::monotonic_nanos(),
            server_origin_nanos: None,
            high_water_millis: 0,
        }
    }

    /// Moves the epoch so that this instant reads as `elapsed`.
    ///
    /// See [`X11Shell::align_event_clock`].
    fn align(&mut self, elapsed: Duration) {
        let now = ffi::monotonic_nanos();
        self.epoch_nanos =
            now.saturating_sub(u64::try_from(elapsed.as_nanos()).unwrap_or(u64::MAX));
    }

    /// Widens a 32-bit server timestamp against a running high-water mark.
    ///
    /// Pure, so the wrap is testable without waiting seven weeks. A timestamp
    /// that goes *backwards* by more than half the range is a wrap; one that
    /// goes backwards by less is an event delivered out of order, which X11
    /// permits and which must not advance the mark.
    fn widen(high_water_millis: u64, server_millis: u32) -> u64 {
        const WRAP: u64 = 1 << 32;
        let low = u64::from(server_millis);
        let candidate = (high_water_millis & !(WRAP - 1)) | low;
        if candidate + WRAP / 2 < high_water_millis {
            candidate + WRAP
        } else {
            candidate
        }
    }

    /// This base applied to a server millisecond timestamp.
    fn event_time(&mut self, server_millis: u32) -> EventTime {
        self.event_time_at(ffi::monotonic_nanos(), server_millis)
    }

    /// [`event_time`](Self::event_time) with the clock passed in.
    ///
    /// Split out so calibration is testable without depending on this
    /// machine's uptime — the bug this shape prevents only appeared on a host
    /// whose `CLOCK_MONOTONIC` was smaller than the server's timestamp, which
    /// is not something a test can arrange by reading the real clock. Same rule
    /// as `crcbl-core`'s `TimeSource`: nothing reads a clock where a test needs
    /// to drive it.
    fn event_time_at(&mut self, now_nanos: u64, server_millis: u32) -> EventTime {
        let widened = Self::widen(self.high_water_millis, server_millis);
        self.high_water_millis = self.high_water_millis.max(widened);
        let server_nanos = i128::from(widened) * 1_000_000;
        let origin = *self
            .server_origin_nanos
            // Calibration: this event happened *now*, so millisecond zero was
            // `widened` milliseconds ago — possibly before this machine booted.
            .get_or_insert_with(|| i128::from(now_nanos) - server_nanos);
        let since_epoch = origin + server_nanos - i128::from(self.epoch_nanos);
        // An event before the epoch reads as the epoch: `EventTime` is a
        // duration since it, and there is no such thing as a negative one.
        EventTime::from_duration(Duration::from_nanos(
            u64::try_from(since_epoch.max(0)).unwrap_or(u64::MAX),
        ))
    }
}

// X11 sends `FocusIn`, `FocusOut` and `ConfigureNotify` with **no timestamp at
// all**, which on Wayland would need [`EventTime`]'s "stamp it with now" rule.
// It does not here, and the reason is a property of the seam rather than of the
// platform: the three events those turn into — [`ShellEvent::Focus`],
// [`ShellEvent::Resized`] and [`ShellEvent::ScaleFactorChanged`] — are the state
// transitions [`event`](crate::event) deliberately gives no timestamp to. So
// there is nothing to stamp, and no `event_time_now` here to match the Wayland
// backend's.

// ---------------------------------------------------------------------------
// The connection
// ---------------------------------------------------------------------------

/// The X connection and everything latched from it at startup.
struct Conn {
    lib: &'static Lib,
    ext: ffi::Ext,
    connection: NonNull<Connection>,
    fd: c_int,
    /// The screen this connection's `DISPLAY` named.
    root: u32,
    root_visual: u32,
    screen_size: PhysicalSize,
    /// The interned atom table; see [`atoms`].
    atoms: Atoms,
    /// Largest payload one `ChangeProperty` may carry, from the setup reply.
    max_property_bytes: usize,
    /// RandR's first event number, or `None` when the extension is absent.
    randr_event_base: Option<u8>,
    /// XInput's major opcode, or `None`.
    xi_opcode: Option<u8>,
    /// Whether the server agreed to stop faking a `KeyRelease` in front of
    /// every auto-repeat; see
    /// [`setup_detectable_auto_repeat`](Self::setup_detectable_auto_repeat).
    ///
    /// Read by [`input`] to decide *how* a repeat is recognized, never whether
    /// one is: the flag on [`ShellEvent::Key`] is set either way.
    detectable_auto_repeat: bool,
}

impl core::fmt::Debug for Conn {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        f.debug_struct("Conn")
            .field("root", &self.root)
            .field("randr", &self.randr_event_base.is_some())
            .field("xinput2", &self.xi_opcode.is_some())
            .finish()
    }
}

impl Conn {
    /// The raw pointer, for the many calls that need it.
    fn raw(&self) -> *mut Connection {
        self.connection.as_ptr()
    }

    /// Pushes everything buffered to the server.
    fn flush(&self) {
        // SAFETY: the connection is live for this value's lifetime.
        unsafe { (self.lib.flush)(self.raw()) };
    }

    /// A fresh XID.
    fn generate_id(&self) -> u32 {
        // SAFETY: the connection is live.
        unsafe { (self.lib.generate_id)(self.raw()) }
    }

    /// Whether the connection has failed.
    ///
    /// libxcb latches the first error and answers every later call with it, so
    /// this is the one check that distinguishes "nothing happened" from "the
    /// server went away" — [`Shell::pump`] must not spin silently on a dead
    /// socket.
    fn has_error(&self) -> Option<String> {
        // SAFETY: the connection is live; `xcb_connection_has_error` is valid
        // on a connection that has already failed, which is the point.
        let code = unsafe { (self.lib.connection_has_error)(self.raw()) };
        if code == 0 {
            return None;
        }
        Some(match code {
            1 => "the connection was closed by the server".to_string(),
            2 => "the server does not support an extension we required".to_string(),
            3 => "out of memory".to_string(),
            4 => "the server hit its request length limit".to_string(),
            5 => "cannot parse the display string".to_string(),
            6 => "cannot open the display".to_string(),
            7 => "the display is invalid".to_string(),
            other => format!("libxcb error {other}"),
        })
    }

    /// Replaces a property with `data`.
    fn set_property(&self, window: u32, property: u32, type_: u32, format: u8, data: &[u8]) {
        let elements = match format {
            32 => data.len() / 4,
            16 => data.len() / 2,
            _ => data.len(),
        };
        // SAFETY: the connection and window are live, and `data` outlives the
        // call — libxcb copies it into its request buffer before returning.
        // `elements` is `data.len()` divided by the element width `format`
        // names, which is what `ChangeProperty` counts in.
        unsafe {
            (self.lib.change_property)(
                self.raw(),
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

    /// Reads a property in full, following `bytes_after` until it is exhausted.
    ///
    /// Returns `(type, format, bytes)`, or `None` when the property cannot be
    /// read at all: a null reply (the connection failed) or a property larger
    /// than [`selection::MAX_BYTES`]. A type-less property — one the owner
    /// deleted, which ICCCM's `INCR` protocol uses as its transfer terminator —
    /// answers `Some((0, …))` and is a *value*, not an absence. A property that
    /// exists and is empty answers `Some` with an empty vector — the
    /// distinction [`ClipboardContent`] is built on.
    ///
    /// # The size cap is here, not in the caller
    ///
    /// Whoever wrote the property chose how big it is, and following
    /// `bytes_after` to exhaustion means this process allocates whatever a
    /// clipboard owner answered a `ConvertSelection` with — and then copies it
    /// again. [`selection::MAX_BYTES`] used to be consulted only in
    /// `Read::on_chunk`, which is *after* the bytes are resident, and the
    /// non-`INCR` path had no check at all: one multi-gigabyte property was
    /// enough. A property past the cap reads as **absent**, because a truncated
    /// property is not the property and a caller must not be handed half a
    /// value; the clipboard path turns that into
    /// [`Unavailable`](ClipboardContent::Unavailable), which is the same answer
    /// `on_chunk` gives past the same cap.
    fn get_property(&self, window: u32, property: u32) -> Option<(u32, u8, Vec<u8>)> {
        /// Words per `GetProperty`. 4 MiB, which is larger than any chunk a
        /// peer may write (see [`selection::max_property_bytes`]) and so
        /// normally reads everything in one request.
        const CHUNK_WORDS: u32 = 1 << 20;

        let mut bytes = Vec::new();
        let mut offset = 0u32;
        let mut type_;
        let mut format;
        loop {
            // SAFETY: the connection and window are live. `AnyPropertyType` is
            // `0`, which asks for whatever type is there rather than filtering.
            let reply = unsafe {
                let cookie = (self.lib.get_property)(
                    self.raw(),
                    0,
                    window,
                    property,
                    0,
                    offset,
                    CHUNK_WORDS,
                );
                (self.lib.get_property_reply)(self.raw(), cookie, ptr::null_mut())
            };
            if reply.is_null() {
                return None;
            }
            // SAFETY: `reply` is a live reply this call owns.
            let (reply_type, reply_format, bytes_after, value) = unsafe {
                let length = (self.lib.get_property_value_length)(reply);
                let data = (self.lib.get_property_value)(reply);
                let value = if length > 0 && !data.is_null() {
                    // SAFETY: libxcb guarantees `length` readable bytes at
                    // `data`, inside the same allocation as `reply`.
                    core::slice::from_raw_parts(data.cast::<u8>(), length as usize).to_vec()
                } else {
                    Vec::new()
                };
                ((*reply).type_, (*reply).format, (*reply).bytes_after, value)
            };
            // SAFETY: `reply` came from libxcb's `malloc` and is freed once.
            unsafe { ffi::free_reply(reply) };

            if reply_type == 0 {
                // A type-less property is not an absence: it is what the owner
                // deleting the property looks like, and ICCCM's `INCR` protocol
                // ends its transfers on exactly this read. `reply_format` is 0
                // for a deleted property, and the accumulated `bytes` are what
                // this loop had read so far.
                return Some((0, reply_format, bytes));
            }
            type_ = reply_type;
            format = reply_format;
            let read = value.len();
            if bytes.len().saturating_add(read) > selection::MAX_BYTES {
                crcbl_core::log::warn!(
                    "property {property} on window {window:#x} is larger than \
                     {} bytes; treating it as absent",
                    selection::MAX_BYTES
                );
                return None;
            }
            bytes.extend_from_slice(&value);
            if bytes_after == 0 || read == 0 {
                break;
            }
            offset += u32::try_from(read / 4).unwrap_or(0).max(1);
        }
        Some((type_, format, bytes))
    }

    /// Deletes a property, ignoring the case where it was not there.
    fn delete_property(&self, window: u32, property: u32) {
        // SAFETY: the connection is live; deleting an absent property is a
        // no-op on the server rather than an error.
        unsafe { (self.lib.delete_property)(self.raw(), window, property) };
    }

    /// A property read as a list of 32-bit words.
    fn get_property_words(&self, window: u32, property: u32) -> Vec<u32> {
        let Some((_, format, bytes)) = self.get_property(window, property) else {
            return Vec::new();
        };
        if format != 32 {
            return Vec::new();
        }
        bytes
            .chunks_exact(4)
            .map(|word| u32::from_ne_bytes([word[0], word[1], word[2], word[3]]))
            .collect()
    }

    /// Sends a 32-byte event to `window`.
    ///
    /// `xcb_send_event` reads exactly 32 bytes from the pointer whatever it is
    /// given, and this module also defines 28- and 12-byte wire structs, so the
    /// requirement is enforced *structurally* rather than by a comment: the
    /// `const` block below is evaluated at monomorphisation and fails the build
    /// for any other `T`. A remark in a SAFETY comment could not have.
    fn send_event<T>(&self, window: u32, mask: u32, event: &T) {
        const {
            assert!(
                size_of::<T>() == 32,
                "xcb_send_event always reads 32 bytes, so the event must be exactly that large"
            );
        }
        // SAFETY: the connection and window are live, and `T` is 32 bytes by
        // the assertion above, so the 32 bytes libxcb reads are all inside
        // `event`.
        unsafe {
            (self.lib.send_event)(
                self.raw(),
                0,
                window,
                mask,
                ptr::from_ref(event).cast::<core::ffi::c_char>(),
            );
        }
    }
}

/// Reinterprets an event's bytes as a wire struct.
///
/// `None` when `raw` is shorter than `T`. That is meant never to happen —
/// libxcb delivers events of at least 32 bytes and every `T` read through this
/// is no larger, which [`ffi`]'s size assertions pin down — but "cannot happen"
/// is not a safety argument. The three byte-identical copies this replaces
/// copied `size_of::<T>().min(raw.len())` bytes and then called
/// `assume_init()`, so a short slice left the tail of `T` uninitialised, and
/// reading uninitialised memory is undefined *regardless* of every bit pattern
/// of `T` being valid. A short slice is now an outcome the caller handles.
///
/// A copy rather than a cast, because an event buffer is a `Vec<u8>` with no
/// alignment guarantee and reading a `u32` through a misaligned reference is
/// undefined even on x86.
pub(super) fn read_wire<T: Copy>(raw: &[u8]) -> Option<T> {
    if raw.len() < size_of::<T>() {
        crcbl_core::log::debug!(
            "an X11 event of {} bytes is too short for a {}-byte wire struct",
            raw.len(),
            size_of::<T>()
        );
        return None;
    }
    let mut value = core::mem::MaybeUninit::<T>::uninit();
    // SAFETY: `T` is a `#[repr(C)]` wire struct of plain integers, so every bit
    // pattern is valid; `raw` is at least `size_of::<T>()` bytes by the check
    // above, so the whole of `value` is written before it is read; and the
    // destination is a fresh local, so the copy cannot overlap.
    unsafe {
        core::ptr::copy_nonoverlapping(
            raw.as_ptr(),
            value.as_mut_ptr().cast::<u8>(),
            size_of::<T>(),
        );
        Some(value.assume_init())
    }
}

// ---------------------------------------------------------------------------
// Windows
// ---------------------------------------------------------------------------

/// One X11 window.
#[derive(Debug)]
struct XWindow {
    /// The XID. Also what [`SurfaceTarget::Xcb`] carries.
    id: u32,
    title: String,
    requested_size: LogicalSize,
    requested_mode: DisplayMode,
    requested_constraints: SizeConstraints,
    resizable: bool,
    configuration: Option<WindowConfiguration>,
    /// The configuration the server already told us about, waiting to be
    /// published on the next [`pump`](Shell::pump).
    ///
    /// This is the whole of X11's difference from Wayland's configure
    /// handshake: the size is known synchronously, and the *only* reason it is
    /// not published immediately is that [`WindowState::size`] is defined to be
    /// `None` until a [`Resized`](ShellEvent::Resized) has been delivered. So
    /// the wait is one pump rather than a round trip, and it is a real wait
    /// rather than an invented one.
    pending: Option<WindowConfiguration>,
    /// Last size the server reported, so a `ConfigureNotify` that only moved
    /// the window does not produce a spurious resize.
    last_size: Option<PhysicalSize>,
    /// Whether `_NET_WM_STATE` currently lists `_NET_WM_STATE_FULLSCREEN`.
    ///
    /// The *effective* mode, which only a window manager can state — and which
    /// stays `false` forever when there is none, however many requests are
    /// made. That is what [`WindowState::mode_request_honoured`] reports.
    effective_fullscreen: bool,
    /// Which monitor the window is actually on while borderless, worked out
    /// from its desktop origin rather than from what was asked for.
    effective_monitor: Option<MonitorId>,
    pointer_mode: PointerMode,
    /// The cursor last asked for. The outer [`Option`] distinguishes "never
    /// set", where the server's default stands, from "set to hidden".
    cursor: Option<Option<CursorIcon>>,
    /// Whether the pointer is inside — for suppressing a warp when it is not.
    pointer_inside: bool,
    focused: bool,
    visible: bool,
    mapped: bool,
    /// Whether a `MapWindow` request has been sent and not yet undone.
    ///
    /// Distinct from [`mapped`](Self::mapped), which follows `MapNotify`: a
    /// window manager begins managing a window when the map *request* reaches
    /// it, so between the request and the notify a window is managed and
    /// `mapped` is still false. `apply_fullscreen` needs the earlier boundary —
    /// see its docs for the fullscreen request that used to disappear into that
    /// gap.
    map_requested: bool,
    close_pending: bool,
}

// ---------------------------------------------------------------------------
// The shell
// ---------------------------------------------------------------------------

/// A [`Shell`] backed by a real X server.
///
/// Constructed through [`open`](crate::open) or
/// [`open_backend`](crate::open_backend); see the [module docs](self) for what
/// this backend implements, what was cut, and where X11 differs from every
/// other implementation of this seam.
pub struct X11Shell {
    conn: Conn,
    windows: Pool<XWindow>,
    monitors: Vec<MonitorInfo>,
    /// RandR CRTC id → the [`MonitorId`] it was given, so an id is never reused
    /// within a session — the obligation [`monitor`](crate::monitor) states.
    monitor_ids: Vec<(u32, MonitorId)>,
    next_monitor_id: u32,
    next_request_id: u32,
    /// Device pixels per logical unit, from `Xft.dpi`. Global: X11 has no
    /// per-monitor scale.
    scale_factor: f64,
    keymap: Option<xkb::Keymap>,
    /// Keys currently held, for the degraded modifier path and for repeat
    /// detection.
    held: Vec<u8>,
    /// The most recent server timestamp seen, for requests that must quote one.
    last_server_time: u32,
    /// Whether the current timestamp probe's own `PropertyNotify` has arrived.
    ///
    /// `refresh_server_time` waits for this rather than for `last_server_time`
    /// to change: at ≥1 kHz event rates the server may stamp the probe with
    /// the same millisecond as the previous event, and the arrival of the
    /// probe is the one signal that does not depend on the stamp.
    timestamp_probe_seen: bool,
    /// Reads waiting on the selection owner; see [`selection`].
    reads: Vec<Read>,
    /// `INCR` transfers we are feeding to a peer.
    writes: Vec<Write>,
    /// What [`clipboard_offer`](Shell::clipboard_offer) published, and which
    /// window owns it.
    offers: Vec<(MimeType, Vec<u8>)>,
    owner_window: u32,
    /// The server timestamp the selection was acquired with.
    ///
    /// ICCCM's `TIMESTAMP` target answers with exactly this, and it must be the
    /// acquisition time rather than "now": a peer uses it to tell one ownership
    /// from the next.
    owner_time: u32,
    /// A hidden cursor, created once and shared. `0` until something asks.
    blank_cursor: u32,
    queue: VecDeque<ShellEvent>,
    time: TimeBase,
    caps: ShellCaps,
    /// Whether a window manager was running at startup.
    has_window_manager: bool,
    lost: Option<String>,
}

impl core::fmt::Debug for X11Shell {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        f.debug_struct("X11Shell")
            .field("windows", &self.windows.len())
            .field("monitors", &self.monitors.len())
            .field("window_manager", &self.has_window_manager)
            .field("transfers", &(self.reads.len() + self.writes.len()))
            .field("queued_events", &self.queue.len())
            .field("connected", &self.lost.is_none())
            .finish()
    }
}

impl X11Shell {
    /// Connects to the server named by `DISPLAY`.
    ///
    /// Interns the atom table, queries RandR and XInput 2, reads the keymap and
    /// enumerates monitors — all before returning, and all with the request
    /// pipelining [`atoms`] describes, so the whole of it costs a handful of
    /// round trips rather than one per fact. Blocking here is correct: there is
    /// no frame loop yet, and the alternative is a shell that reports no
    /// monitors and no capabilities for the first few frames.
    ///
    /// # Errors
    ///
    /// [`ShellError::Connect`] if libxcb cannot be loaded, if `DISPLAY` names
    /// no server, or if the connection failed for any of the reasons
    /// [`Conn::has_error`] names.
    pub fn open() -> Result<Self, ShellError> {
        let lib = ffi::load().map_err(|detail| ShellError::Connect {
            backend: ShellBackend::X11,
            detail: detail.to_string(),
        })?;

        let mut screen_index: c_int = 0;
        // SAFETY: a null display name means "read `DISPLAY`", which is
        // `xcb_connect`'s documented behaviour, and `screen_index` is a live
        // local it writes the preferred screen number into.
        let connection = unsafe { (lib.connect)(ptr::null(), &raw mut screen_index) };
        // `xcb_connect` never returns null — it returns a connection with an
        // error latched — so the pointer is always valid to hand to
        // `xcb_disconnect`, and always has to be.
        let Some(connection) = NonNull::new(connection) else {
            return Err(ShellError::Connect {
                backend: ShellBackend::X11,
                detail: "xcb_connect returned null".to_string(),
            });
        };
        // SAFETY: the connection is live, whether or not it has an error.
        let error = unsafe { (lib.connection_has_error)(connection.as_ptr()) };
        if error != 0 {
            // SAFETY: the connection was returned by `xcb_connect` and is
            // released exactly once, here.
            unsafe { (lib.disconnect)(connection.as_ptr()) };
            return Err(ShellError::Connect {
                backend: ShellBackend::X11,
                detail: match std::env::var("DISPLAY") {
                    Ok(display) => format!("no X server at DISPLAY={display} (libxcb {error})"),
                    Err(_) => "DISPLAY is not set".to_string(),
                },
            });
        }

        // SAFETY: the connection is live and error-free, so the setup reply is
        // present and owned by libxcb for the connection's lifetime.
        let (root, root_visual, screen_size, max_request_length) = unsafe {
            let setup = (lib.get_setup)(connection.as_ptr());
            if setup.is_null() {
                (lib.disconnect)(connection.as_ptr());
                return Err(ShellError::Connect {
                    backend: ShellBackend::X11,
                    detail: "the server sent no setup reply".to_string(),
                });
            }
            let mut iterator = (lib.setup_roots_iterator)(setup);
            for _ in 0..screen_index.max(0) {
                if iterator.rem <= 1 {
                    break;
                }
                (lib.screen_next)(&raw mut iterator);
            }
            if iterator.data.is_null() {
                (lib.disconnect)(connection.as_ptr());
                return Err(ShellError::Connect {
                    backend: ShellBackend::X11,
                    detail: "the server has no screens".to_string(),
                });
            }
            let screen = *iterator.data;
            (
                screen.root,
                screen.root_visual,
                PhysicalSize::new(
                    u32::from(screen.width_in_pixels),
                    u32::from(screen.height_in_pixels),
                ),
                (*setup).maximum_request_length,
            )
        };

        // SAFETY: the connection is live and error-free.
        let fd = unsafe { (lib.get_file_descriptor)(connection.as_ptr()) };
        // SAFETY: as above; `intern` only issues `InternAtom` requests.
        let atom_table = unsafe { Atoms::intern(lib, connection.as_ptr()) };

        let mut conn = Conn {
            lib,
            ext: ffi::load_ext(),
            connection,
            fd,
            root,
            root_visual,
            screen_size,
            atoms: atom_table,
            max_property_bytes: selection::max_property_bytes(max_request_length),
            randr_event_base: None,
            xi_opcode: None,
            detectable_auto_repeat: false,
        };
        conn.randr_event_base = conn.setup_randr();
        conn.xi_opcode = conn.setup_xinput2();
        conn.detectable_auto_repeat = conn.setup_detectable_auto_repeat();

        // SAFETY: the connection is live; libxkbcommon-x11 round-trips on it
        // and this shell is not `Send`, so nothing else can be using it.
        let keymap = unsafe { xkb::Keymap::from_x11_device(conn.raw().cast::<c_void>()) };

        let has_window_manager = conn.detect_window_manager();
        let scale_factor = conn.read_xft_scale();

        let mut shell = Self {
            conn,
            windows: Pool::new(),
            monitors: Vec::new(),
            monitor_ids: Vec::new(),
            next_monitor_id: 1,
            next_request_id: 1,
            scale_factor,
            keymap,
            held: Vec::new(),
            last_server_time: ffi::value::CURRENT_TIME,
            timestamp_probe_seen: false,
            reads: Vec::new(),
            writes: Vec::new(),
            offers: Vec::new(),
            owner_window: 0,
            owner_time: ffi::value::CURRENT_TIME,
            blank_cursor: 0,
            queue: VecDeque::new(),
            time: TimeBase::now(),
            caps: ShellCaps::empty(),
            has_window_manager,
            lost: None,
        };
        shell.monitors = shell.enumerate_monitors();
        shell.caps = shell.latch_caps();
        shell.conn.flush();
        Ok(shell)
    }

    /// The capability set, computed once from what the server actually has.
    ///
    /// [`caps`](crate::caps) requires this to be latched, and X11 makes that
    /// easy in a way Wayland does not: an extension the server lacks at connect
    /// time cannot appear later, because extensions are a property of the
    /// server binary. What *can* change mid-session is the window manager —
    /// someone may start one — and that is exactly why the two window-manager
    /// bits are read here and never again. A renderer told at init that aspect
    /// hints are ignored must not be contradicted when a window manager
    /// appears three minutes in.
    fn latch_caps(&self) -> ShellCaps {
        let mut caps = ShellCaps::MULTI_WINDOW
            | ShellCaps::EVENT_WAIT
            | ShellCaps::CLIPBOARD
            // Every X11 client may move its own windows and is told where they
            // are, and — unlike Wayland — every monitor is a rectangle in one
            // device-pixel space. See `caps` on the trait impl for exactly what
            // this bit is claiming today.
            | ShellCaps::WINDOW_POSITION
            // `XWarpPointer`, which the seam names as this bit's X11 form.
            | ShellCaps::POINTER_WARP
            // `GrabPointer` with `confine_to` is confinement; lock is that plus
            // a hidden cursor and raw motion.
            | ShellCaps::POINTER_LOCK
            | ShellCaps::POINTER_CONFINE
            // `Xft.dpi` is an arbitrary integer, so 1.25 and 1.5 are ordinary
            // values here. See the module docs for how little of a *protocol*
            // that is.
            | ShellCaps::FRACTIONAL_SCALE;

        // Both of these need a window manager, and a window manager is a
        // separate program that may simply not be running — the finding this
        // backend exists to record. Under bare `Xvfb` both are false and that
        // is the correct answer, not a degraded one.
        caps.set(ShellCaps::ASPECT_HINT_HONORED, self.has_window_manager);
        caps.set(ShellCaps::SERVER_DECORATIONS, self.has_window_manager);

        // XI2 is the only source of unaccelerated motion on X11; core
        // `MotionNotify` has pointer acceleration already applied and is
        // clamped at the screen edge, which is exactly what
        // `RAW_POINTER_MOTION` exists to distinguish.
        caps.set(ShellCaps::RAW_POINTER_MOTION, self.conn.xi_opcode.is_some());

        // As on Wayland, this asserts that composed text can reach the engine
        // at all, which here means libxkbcommon **and** its X11 companion
        // resolved and the server had a keymap to give.
        caps.set(
            ShellCaps::TEXT_IME,
            xkb::x11_available() && self.keymap.is_some(),
        );

        // Cut from this slice; see the module docs.
        caps.remove(ShellCaps::DRAG_DROP);
        // X11 has no viewporter and no equivalent: a client presents pixels at
        // the window's size, full stop. The renderer does the upscale blit.
        caps.remove(ShellCaps::HW_UPSCALE);
        caps
    }

    fn window(&self, window: WindowId) -> Result<&XWindow, ShellError> {
        self.windows
            .get(window.cast())
            .ok_or_else(|| ShellError::invalid_window(window))
    }

    fn window_mut(&mut self, window: WindowId) -> Result<&mut XWindow, ShellError> {
        self.windows
            .get_mut(window.cast())
            .ok_or_else(|| ShellError::invalid_window(window))
    }

    /// Drops everything a window that has gone away was carrying.
    ///
    /// Called from **both** ways a window dies — [`Shell::destroy_window`] and
    /// a server-driven `DestroyNotify` — because the two used to disagree, and
    /// each of them was wrong in its own direction: the explicit path dropped
    /// outstanding reads and left `writes` alone, and the `DestroyNotify` path
    /// left the reads *alive*, so two seconds later `service_transfers` emitted
    /// a [`ClipboardData`](ShellEvent::ClipboardData) naming a window it had
    /// already reported as [`WindowDestroyed`](ShellEvent::WindowDestroyed).
    ///
    /// # Why an outstanding read is dropped rather than answered
    ///
    /// [Obligation 4](Shell) is about *accepted* requests, and an answer has to
    /// name the window it was asked for. Emitting one for a handle the consumer
    /// has just been told is stale would break
    /// [obligation 1](Shell) — the stronger of the two, because a consumer can
    /// see a stale handle and cannot see a request that will never be answered
    /// on a window it no longer has. `HeadlessShell::resolve_reads` makes the
    /// same call for the same reason, and this is the two backends agreeing.
    fn forget_selection_state(&mut self, window: WindowId, xid: u32) {
        self.reads.retain(|read| read.window != window);
        if self.owner_window == xid {
            // A selection owned by a window that has stopped existing is a
            // clipboard nobody can ever read: the server would keep naming a
            // dead window as the owner until some other client claimed it. The
            // outstanding writes are answers on that selection, so they go with
            // it — exactly as they do when `clipboard_offer` releases it.
            self.conn
                .set_selection_owner(ffi::value::NONE, self.last_server_time);
            self.offers.clear();
            self.writes.clear();
            self.owner_window = 0;
            self.owner_time = ffi::value::CURRENT_TIME;
        }
    }

    /// The window this shell currently holds the pointer grab for, if any.
    ///
    /// Derived rather than stored, because an X11 grab is *per client*: there
    /// is at most one, and [`XWindow::pointer_mode`] already records which
    /// window asked for it — set only after the `GrabPointer` succeeded. A
    /// separate field would be a second source of truth that a destroyed window
    /// could leave stale.
    fn grab_holder(&self) -> Option<WindowId> {
        self.windows
            .iter()
            .find(|(_, state)| state.pointer_mode.is_captured())
            .map(|(handle, _)| handle.cast())
    }

    /// The [`WindowId`] owning an XID, for routing an incoming event.
    fn window_by_xid(&self, xid: u32) -> Option<WindowId> {
        self.windows
            .iter()
            .find(|(_, window)| window.id == xid)
            .map(|(handle, _)| handle.cast())
    }

    fn unsupported(what: &'static str) -> ShellError {
        ShellError::Unsupported {
            backend: ShellBackend::X11,
            what,
        }
    }

    /// Aligns event timestamps with an engine
    /// [`TimeSource`](crcbl_core::TimeSource).
    ///
    /// See [`Shell::align_event_clock`], which this implements.
    pub fn align_time_base(&mut self, elapsed: Duration) {
        self.time.align(elapsed);
    }
}

mod connect;
mod input;
mod monitors;
mod shell;
mod window;
mod xselection;

#[cfg(test)]
mod tests {
    use super::*;

    /// A `TimeBase` whose epoch is `uptime_nanos`, as if the shell had been
    /// created at that point on this machine's monotonic clock.
    fn base_at(uptime_nanos: u64) -> TimeBase {
        TimeBase {
            epoch_nanos: uptime_nanos,
            server_origin_nanos: None,
            high_water_millis: 0,
        }
    }

    #[test]
    fn the_server_clock_is_calibrated_on_the_first_event_and_not_before() {
        // The X11-specific problem: server milliseconds count from an origin
        // this process has never read. Calibration makes the *first* event read
        // as "now", and everything after it is exact relative to that.
        let mut base = base_at(9_000_000_000_000);
        assert_eq!(base.server_origin_nanos, None, "nothing to calibrate from");
        // The shell was created 3s ago; the server says 4000s.
        let first = base.event_time_at(9_003_000_000_000, 4_000_000);
        assert!(
            base.server_origin_nanos.is_some(),
            "the first timestamp calibrates"
        );
        assert_eq!(
            first.as_duration(),
            Duration::from_secs(3),
            "the first event reads as now, not as the server's uptime"
        );
    }

    #[test]
    fn a_server_clock_ahead_of_this_machines_uptime_still_calibrates() {
        // The case CI caught and a local run could not: a host booted 49s ago
        // talking to a server that has been up 4000s — ordinary for a remote
        // display, and ordinary locally on a fresh machine. Millisecond zero
        // then falls ~3951s before this machine booted, which is only
        // representable because the origin is signed. The unsigned form
        // saturated to zero and reported the event 3950.86s in the future.
        let mut base = base_at(49_000_000_000);
        let first = base.event_time_at(49_500_000_000, 4_000_000);
        assert_eq!(
            first.as_duration(),
            Duration::from_millis(500),
            "calibrated, not forwarded: {first:?}"
        );
        assert!(
            base.server_origin_nanos.expect("calibrated") < 0,
            "millisecond zero predates this machine's boot"
        );
    }

    #[test]
    fn an_event_stamped_before_the_epoch_reads_as_the_epoch() {
        // Alignment can move the epoch forward past an already-calibrated
        // origin. `EventTime` is a duration since the epoch, so there is no
        // negative to report; clamping is the honest answer and is what the
        // Wayland backend's pre-epoch case already does.
        let mut base = TimeBase {
            epoch_nanos: 10_000_000_000,
            server_origin_nanos: Some(0),
            high_water_millis: 0,
        };
        let early = base.event_time_at(10_000_000_000, 1_000);
        assert_eq!(early.as_duration(), Duration::ZERO);
    }

    #[test]
    fn durations_between_two_events_survive_calibration_exactly() {
        // The property that has to hold, because `docs/plan/19-input.md`'s
        // tap-versus-hold evaluation is a subtraction between two of these.
        let mut base = TimeBase {
            epoch_nanos: 0,
            server_origin_nanos: Some(0),
            high_water_millis: 0,
        };
        let pressed = base.event_time(1_000);
        let released = base.event_time(1_180);
        assert_eq!(
            released.saturating_since(pressed),
            Duration::from_millis(180),
            "the offset cancels"
        );
    }

    #[test]
    fn a_thirty_two_bit_server_clock_wrap_does_not_go_backwards() {
        // 49.7 days of *server* uptime, which for an X server is routine — a
        // login session that has been open since the machine was last rebooted
        // hits this, where a compositor session rarely does.
        const WRAP: u64 = 1 << 32;
        // Just before the wrap, and just after.
        let before = TimeBase::widen(WRAP - 1_000, (WRAP - 1_000) as u32);
        assert_eq!(before, WRAP - 1_000);
        let after = TimeBase::widen(WRAP - 1_000, 500);
        assert_eq!(after, WRAP + 500, "the wrap is widened, not misread");
        assert!(after > before, "time never goes backwards across a wrap");

        // An event delivered slightly out of order is *not* a wrap and must not
        // add four billion milliseconds.
        assert_eq!(TimeBase::widen(10_000, 9_990), 9_990);
    }

    #[test]
    fn the_two_input_devices_are_distinct_and_not_the_unknown_one() {
        // `DeviceId::UNKNOWN` means "the backend could not say". This backend
        // can say which *kind*, even though core X11 cannot say which mouse.
        assert_ne!(KEYBOARD_DEVICE, POINTER_DEVICE);
        assert_ne!(KEYBOARD_DEVICE, DeviceId::UNKNOWN);
        assert_ne!(POINTER_DEVICE, DeviceId::UNKNOWN);
    }
}
