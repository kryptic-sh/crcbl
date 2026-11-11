//! The extension libraries the X11 backend loads beside libxcb, and the
//! descriptor waits and clock it runs on.
//!
//! The binding itself — libxcb and libxcb-randr, loaded with `dlopen`, and the
//! core and RandR wire structures — is `crcbl_x11::ffi`, re-exported whole so
//! that `ffi::Lib`, `ffi::value` and the rest keep their paths here. See that
//! module for why `dlopen` rather than `#[link]`, and why the typed request
//! wrappers. What stays here is what only a windowing backend needs: the
//! XInput 2, XKB, libxcb-cursor, SHAPE and DRI3 libraries, loaded the same
//! way, and the backend's own use of libc — `poll` on the connection and wake
//! pipe, and `CLOCK_MONOTONIC`.
//!
//! What is **not** taken from libxcb is anything above the wire:
//! `xcb-util`'s ICCCM and EWMH helpers are not loaded, because
//! the windowing backend table (`docs/notes/backends.md`) puts "our
//! request/event layer (core, EWMH atoms, RandR, XKB)" on our side of the line. [`super::atoms`] and
//! [`super::selection`] are that layer.
//!
//! # Which libraries, and why each one
//!
//! | Library | What it buys | Without it |
//! | --- | --- | --- |
//! | `libxcb.so.1` | the connection, windows, properties, selections, grabs | no backend at all — [`open`](super::X11Shell::open) fails |
//! | `libxcb-randr.so.0` | monitor geometry, refresh, primary | one monitor covering the whole screen, refresh `0` |
//! | `libxcb-xinput.so.0` | XI2 raw motion — unaccelerated aim | [`RAW_POINTER_MOTION`](crate::ShellCaps::RAW_POINTER_MOTION) cleared, no mouselook |
//! | `libxcb-xkb.so.1` | detectable auto-repeat — a repeat with no fake release | the release/press lookahead in [`input`](super::input), which cannot see a pair the socket split |
//! | `libxcb-cursor.so.0` | themed cursor shapes — the user's XCursor theme, or the core cursor font when there is none | [`CURSOR_SHAPE`](crate::ShellCaps::CURSOR_SHAPE) cleared; a shape is recorded and the root's cursor shows |
//! | `libxcb-shape.so.0` | a window's SHAPE input region — the pointer outside it goes to the window beneath | [`INPUT_REGION`](crate::ShellCaps::INPUT_REGION) cleared; the whole window keeps the pointer |
//! | `libxcb-dri3.so.0` | DRI3 `Open`: which GPU the server renders on, for the renderer's adapter choice | `SurfaceTarget::Xcb`'s `main_device` is `None`; the preference alone ranks adapters |
//!
//! With `dlopen` a missing extension library is a cleared
//! [`ShellCaps`](crate::ShellCaps) bit and a log line — see [`Ext`] — where
//! with `DT_NEEDED` it would be a process that does not start.
//!
//! Deliberately absent: `libxcb-xfixes` (pointer barriers are not needed —
//! `GrabPointer`'s `confine_to` *is* [`PointerMode::Confined`](crate::PointerMode)
//! and hiding the cursor is a core-protocol empty pixmap), and
//! `libxcb-icccm`/`libxcb-ewmh` (policy, see above).
//!
//! libxcb-cursor is the XCursor theme loader as a library — what Xlib programs
//! get from libXcursor. X11 has no compositor to hand a shape's *name* to, as
//! Wayland's `cursor-shape-v1` does, so a client that wants the user's themed
//! cursor loads it itself. See [`super::cursor`].
//!
//! libxcb-xkb is used for **one request**. The keymap itself still comes from
//! libxkbcommon-x11, which links libxcb-xkb itself and is where that
//! conversation belongs; what is taken directly here is `XkbPerClientFlags`,
//! which libxkbcommon deliberately does not expose because it is an input-policy
//! decision rather than a keymap one. See
//! [`setup_detectable_auto_repeat`](super::Conn::setup_detectable_auto_repeat).

pub use crcbl_x11::ffi::*;

use core::ffi::{CStr, c_char, c_int, c_long, c_void};
use std::sync::OnceLock;

use crate::linux::{CLOCK_MONOTONIC, Nfds};

// ---------------------------------------------------------------------------
// Extension wire structures
// ---------------------------------------------------------------------------

/// `xcb_cursor_context_t` — libxcb-cursor's theme state for one connection.
/// Opaque; libxcb-cursor owns the layout.
#[repr(C)]
pub struct CursorContext {
    _opaque: [u8; 0],
}

/// `xcb_xkb_use_extension_reply_t`.
///
/// XKB requires every client to announce itself with `UseExtension` before any
/// other XKB request; one that skips it gets a `BadAccess` on the next one.
#[repr(C)]
#[derive(Clone, Copy, Debug)]
pub struct XkbUseExtensionReply {
    /// `1` for a reply.
    pub response_type: u8,
    /// Whether the server will speak the version that was asked for. **Zero is
    /// a successful reply saying no**, which is why this is read rather than
    /// the reply merely being checked for null.
    pub supported: u8,
    /// Sequence number.
    pub sequence: u16,
    /// Extra length.
    pub length: u32,
    /// The version the server offers.
    pub server_major: u16,
    /// See [`server_major`](Self::server_major).
    pub server_minor: u16,
    /// Padding.
    pub pad0: [u8; 20],
}

/// `xcb_shape_query_version_reply_t`.
#[repr(C)]
#[derive(Clone, Copy, Debug)]
pub struct ShapeQueryVersionReply {
    /// `1` for a reply.
    pub response_type: u8,
    /// Padding.
    pub pad0: u8,
    /// Sequence number.
    pub sequence: u16,
    /// Extra length.
    pub length: u32,
    /// Major version the server speaks.
    pub major_version: u16,
    /// Minor version the server speaks.
    pub minor_version: u16,
}

/// `xcb_dri3_open_reply_t`. Its one descriptor is fetched with
/// `xcb_dri3_open_reply_fds`.
#[repr(C)]
#[derive(Clone, Copy, Debug)]
pub struct Dri3OpenReply {
    /// `1` for a reply.
    pub response_type: u8,
    /// How many descriptors came with it: one.
    pub nfd: u8,
    /// Sequence number.
    pub sequence: u16,
    /// Extra length.
    pub length: u32,
    /// Padding.
    pub pad0: [u8; 24],
}

/// `xcb_xkb_per_client_flags_reply_t`.
#[repr(C)]
#[derive(Clone, Copy, Debug)]
pub struct XkbPerClientFlagsReply {
    /// `1` for a reply.
    pub response_type: u8,
    /// The device the flags apply to.
    pub device_id: u8,
    /// Sequence number.
    pub sequence: u16,
    /// Extra length.
    pub length: u32,
    /// Which flags this server implements at all.
    pub supported: u32,
    /// Which flags are now **set for this client**. The authority on whether
    /// the request took effect: a server may reply happily and leave the bit
    /// clear.
    pub value: u32,
    /// Auto-reset controls; unused here.
    pub auto_ctrls: u32,
    /// See [`auto_ctrls`](Self::auto_ctrls).
    pub auto_ctrls_values: u32,
    /// Padding.
    pub pad0: [u8; 8],
}

// ---------------------------------------------------------------------------
// XInput 2
// ---------------------------------------------------------------------------

/// `xcb_input_xi_query_version_reply_t`.
#[repr(C)]
#[derive(Clone, Copy, Debug)]
pub struct XiQueryVersionReply {
    /// `1` for a reply.
    pub response_type: u8,
    /// Padding.
    pub pad0: u8,
    /// Sequence number.
    pub sequence: u16,
    /// Extra length.
    pub length: u32,
    /// Major version the server settled on.
    pub major_version: u16,
    /// Minor version the server settled on.
    pub minor_version: u16,
    /// Padding.
    pub pad1: [u8; 20],
}

/// `xcb_input_event_mask_t`, followed by `mask_len` 32-bit mask words.
#[repr(C)]
#[derive(Clone, Copy, Debug)]
pub struct XiEventMask {
    /// Device to select on — `0` all, `1` all master devices.
    pub deviceid: u16,
    /// Number of 32-bit words of mask that follow.
    pub mask_len: u16,
}

/// The fixed part of `xcb_input_raw_motion_event_t`.
///
/// Everything after it is variable: a valuator bitmask of `valuators_len`
/// 32-bit words, then one `FP3232` per set bit of *accelerated* value, then the
/// same count again of *raw* values. [`super::input`] does that walk, because
/// the offsets are arithmetic rather than a struct.
///
/// # `full_sequence` is in the middle, and that is not a mistake
///
/// A `GeGeneric` event is longer than 32 bytes, so libxcb cannot append its
/// reconstructed 32-bit sequence number after the payload the way it does for
/// an ordinary event — it would then be at a different offset for every event.
/// It **inserts** it at offset 32 instead and shifts the extension data up by
/// four bytes. So this struct is 36 bytes and the valuator mask starts at 36,
/// not at 32. Reading the arrays from 32 gets the mask word right by accident
/// on a little-endian machine with a small sequence number and then reads every
/// axis value four bytes early, which produces motion deltas that are almost
/// right — the worst possible symptom.
#[repr(C)]
#[derive(Clone, Copy, Debug)]
pub struct XiRawEvent {
    /// `35` — `GeGeneric`.
    pub response_type: u8,
    /// The XInput extension's major opcode.
    pub extension: u8,
    /// Sequence number.
    pub sequence: u16,
    /// Extra length in 4-byte units — how much follows the 32-byte header.
    pub length: u32,
    /// `XI_RawMotion` and friends.
    pub event_type: u16,
    /// Which device.
    pub deviceid: u16,
    /// Server timestamp in milliseconds.
    pub time: u32,
    /// Button number for a raw button event; unused for motion.
    pub detail: u32,
    /// The physical device behind a master.
    pub sourceid: u16,
    /// Number of 32-bit words in the valuator mask.
    pub valuators_len: u16,
    /// Event flags.
    pub flags: u32,
    /// Padding.
    pub pad0: [u8; 4],
    /// The full 32-bit sequence libxcb reconstructs — **inserted here**, before
    /// the extension data. See the type's docs.
    pub full_sequence: u32,
}

/// Assorted single-purpose protocol constants: the core and RandR ones from
/// `crcbl_x11::ffi::value`, and the extensions' beside them.
pub mod value {
    pub use crcbl_x11::ffi::value::*;

    /// `XCB_INPUT_DEVICE_ALL_MASTER`.
    pub const XI_ALL_MASTER_DEVICES: u16 = 1;
    /// `XCB_INPUT_RAW_MOTION` — the XI2 event number, not a mask bit.
    pub const XI_RAW_MOTION: u16 = 17;
    /// The mask bit for [`XI_RAW_MOTION`] in an `xcb_input_event_mask_t`.
    pub const XI_RAW_MOTION_MASK: u32 = 1 << 17;
    /// `XCB_XKB_ID_USE_CORE_KBD` — the device spec meaning "whichever keyboard
    /// the core protocol is reporting", which is the one core `KeyPress` events
    /// come from and therefore the only one this backend has an opinion about.
    pub const XKB_USE_CORE_KBD: u16 = 0x0100;
    /// `XCB_XKB_PER_CLIENT_FLAG_DETECTABLE_AUTO_REPEAT`.
    pub const XKB_DETECTABLE_AUTO_REPEAT: u32 = 1;
    /// The XKB version this backend asks for. 1.0 is the version that defined
    /// `XkbPerClientFlags`, so anything newer also has it.
    pub const XKB_MAJOR: u16 = 1;
    /// See [`XKB_MAJOR`].
    pub const XKB_MINOR: u16 = 0;
    /// `XCB_SHAPE_SO_SET` — the shape becomes exactly what is given.
    pub const SHAPE_SET: u8 = 0;
    /// `XCB_SHAPE_SK_INPUT` — the input shape, where the pointer is the
    /// window's. SHAPE 1.1 added it.
    pub const SHAPE_INPUT: u8 = 2;
}

// ---------------------------------------------------------------------------
// The extension libraries
// ---------------------------------------------------------------------------

// The symbols come from libc, which `std` already links on every Unix target.
unsafe extern "C" {
    fn dlsym(handle: *mut c_void, symbol: *const c_char) -> *mut c_void;
    fn poll(fds: *mut PollFd, nfds: Nfds, timeout: c_int) -> c_int;
    fn clock_gettime(clock_id: c_int, tp: *mut Timespec) -> c_int;
}

/// `struct pollfd`.
#[repr(C)]
struct PollFd {
    fd: c_int,
    events: i16,
    revents: i16,
}

/// `struct timespec`.
#[repr(C)]
struct Timespec {
    tv_sec: c_long,
    tv_nsec: c_long,
}

/// `POLLIN`.
const POLLIN: i16 = 0x001;

/// The C prototypes this backend asserts about the extension libraries.
///
/// The convention of `crcbl_x11::ffi`'s own: the type is named once and used
/// both in the library struct and in the `transmute` that fills it. Every one
/// is copied from the extension's `xcb/*.h` header.
mod prototype {
    use super::{
        Connection, Cookie, CursorContext, Dri3OpenReply, GenericError, Rectangle, Screen,
        ShapeQueryVersionReply, XiEventMask, XiQueryVersionReply, XkbPerClientFlagsReply,
        XkbUseExtensionReply, c_char, c_int,
    };

    /// `xcb_xkb_use_extension_cookie_t xcb_xkb_use_extension(
    /// xcb_connection_t *, uint16_t wantedMajor, uint16_t wantedMinor)`
    pub type XkbUseExtension = unsafe extern "C" fn(*mut Connection, u16, u16) -> Cookie;
    /// `xcb_xkb_use_extension_reply_t *xcb_xkb_use_extension_reply(…)`
    pub type XkbUseExtensionReplyFn = unsafe extern "C" fn(
        *mut Connection,
        Cookie,
        *mut *mut GenericError,
    ) -> *mut XkbUseExtensionReply;
    /// `xcb_xkb_per_client_flags_cookie_t xcb_xkb_per_client_flags(
    /// xcb_connection_t *, xcb_xkb_device_spec_t deviceSpec, uint32_t change,
    /// uint32_t value, uint32_t ctrlsToChange, uint32_t autoCtrls,
    /// uint32_t autoCtrlsValues)`
    pub type XkbPerClientFlags =
        unsafe extern "C" fn(*mut Connection, u16, u32, u32, u32, u32, u32) -> Cookie;
    /// `xcb_xkb_per_client_flags_reply_t *xcb_xkb_per_client_flags_reply(…)`
    pub type XkbPerClientFlagsReplyFn = unsafe extern "C" fn(
        *mut Connection,
        Cookie,
        *mut *mut GenericError,
    ) -> *mut XkbPerClientFlagsReply;

    /// `xcb_input_xi_query_version_cookie_t xcb_input_xi_query_version(
    /// xcb_connection_t *, uint16_t major, uint16_t minor)`
    pub type XiQueryVersion = unsafe extern "C" fn(*mut Connection, u16, u16) -> Cookie;
    /// `xcb_input_xi_query_version_reply_t *xcb_input_xi_query_version_reply(…)`
    pub type XiQueryVersionReplyFn = unsafe extern "C" fn(
        *mut Connection,
        Cookie,
        *mut *mut GenericError,
    ) -> *mut XiQueryVersionReply;
    /// `xcb_void_cookie_t xcb_input_xi_select_events(xcb_connection_t *,
    /// xcb_window_t, uint16_t num_mask, const xcb_input_event_mask_t *masks)`
    pub type XiSelectEvents =
        unsafe extern "C" fn(*mut Connection, u32, u16, *const XiEventMask) -> Cookie;

    /// `int xcb_cursor_context_new(xcb_connection_t *conn,
    /// xcb_screen_t *screen, xcb_cursor_context_t **ctx)` — `0` on success, a
    /// negative `errno` otherwise.
    pub type CursorContextNew =
        unsafe extern "C" fn(*mut Connection, *mut Screen, *mut *mut CursorContext) -> c_int;
    /// `xcb_cursor_t xcb_cursor_load_cursor(xcb_cursor_context_t *ctx,
    /// const char *name)` — `XCB_NONE` when neither the theme nor the core
    /// cursor font has `name`.
    pub type CursorLoadCursor = unsafe extern "C" fn(*mut CursorContext, *const c_char) -> u32;
    /// `void xcb_cursor_context_free(xcb_cursor_context_t *ctx)`
    pub type CursorContextFree = unsafe extern "C" fn(*mut CursorContext);

    /// `xcb_shape_query_version_cookie_t xcb_shape_query_version(
    /// xcb_connection_t *)`
    pub type ShapeQueryVersion = unsafe extern "C" fn(*mut Connection) -> Cookie;
    /// `xcb_shape_query_version_reply_t *xcb_shape_query_version_reply(…)`
    pub type ShapeQueryVersionReplyFn = unsafe extern "C" fn(
        *mut Connection,
        Cookie,
        *mut *mut GenericError,
    ) -> *mut ShapeQueryVersionReply;
    /// `xcb_void_cookie_t xcb_shape_rectangles(xcb_connection_t *,
    /// xcb_shape_op_t operation, xcb_shape_kind_t destination_kind,
    /// uint8_t ordering, xcb_window_t destination_window, int16_t x_offset,
    /// int16_t y_offset, uint32_t rectangles_len,
    /// const xcb_rectangle_t *rectangles)` — both enums are `uint8_t`.
    pub type ShapeRectangles = unsafe extern "C" fn(
        *mut Connection,
        u8,
        u8,
        u8,
        u32,
        i16,
        i16,
        u32,
        *const Rectangle,
    ) -> Cookie;
    /// `xcb_void_cookie_t xcb_shape_mask(xcb_connection_t *,
    /// xcb_shape_op_t operation, xcb_shape_kind_t destination_kind,
    /// xcb_window_t destination_window, int16_t x_offset, int16_t y_offset,
    /// xcb_pixmap_t source_bitmap)`
    pub type ShapeMask =
        unsafe extern "C" fn(*mut Connection, u8, u8, u32, i16, i16, u32) -> Cookie;

    /// `xcb_dri3_open_cookie_t xcb_dri3_open(xcb_connection_t *,
    /// xcb_drawable_t drawable, uint32_t provider)`
    pub type Dri3Open = unsafe extern "C" fn(*mut Connection, u32, u32) -> Cookie;
    /// `xcb_dri3_open_reply_t *xcb_dri3_open_reply(…)`
    pub type Dri3OpenReplyFn =
        unsafe extern "C" fn(*mut Connection, Cookie, *mut *mut GenericError) -> *mut Dri3OpenReply;
    /// `int *xcb_dri3_open_reply_fds(xcb_connection_t *,
    /// xcb_dri3_open_reply_t *reply)` — points into the reply, freed with it.
    pub type Dri3OpenReplyFds =
        unsafe extern "C" fn(*mut Connection, *mut Dri3OpenReply) -> *mut c_int;
}

/// libxcb-xinput, when it is there. See [`RandrLib`].
pub struct XiLib {
    /// `xcb_input_id`; see [`RandrLib::id`].
    pub id: usize,
    pub query_version: prototype::XiQueryVersion,
    pub query_version_reply: prototype::XiQueryVersionReplyFn,
    pub select_events: prototype::XiSelectEvents,
}

impl core::fmt::Debug for XiLib {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        f.write_str("XiLib(libxcb-xinput)")
    }
}

/// libxcb-xkb, when it is there. See [`RandrLib`].
///
/// Two requests, for one purpose: turning on detectable auto-repeat. No
/// extension descriptor is held because neither entry point needs one from the
/// caller — libxcb resolves `xcb_xkb_id` internally.
pub struct XkbLib {
    pub use_extension: prototype::XkbUseExtension,
    pub use_extension_reply: prototype::XkbUseExtensionReplyFn,
    pub per_client_flags: prototype::XkbPerClientFlags,
    pub per_client_flags_reply: prototype::XkbPerClientFlagsReplyFn,
}

impl core::fmt::Debug for XkbLib {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        f.write_str("XkbLib(libxcb-xkb)")
    }
}

/// libxcb-cursor, when it is there. See [`RandrLib`].
///
/// Not an extension: libxcb-cursor is a client-side library that reads the
/// user's cursor theme and uploads its images with requests libxcb already
/// has, so there is no extension descriptor to hold.
pub struct CursorLib {
    pub context_new: prototype::CursorContextNew,
    pub load_cursor: prototype::CursorLoadCursor,
    pub context_free: prototype::CursorContextFree,
}

impl core::fmt::Debug for CursorLib {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        f.write_str("CursorLib(libxcb-cursor)")
    }
}

/// libxcb-shape, when it is there. See [`RandrLib`].
pub struct ShapeLib {
    /// `xcb_shape_id`; see [`RandrLib::id`].
    pub id: usize,
    pub query_version: prototype::ShapeQueryVersion,
    pub query_version_reply: prototype::ShapeQueryVersionReplyFn,
    pub rectangles: prototype::ShapeRectangles,
    pub mask: prototype::ShapeMask,
}

impl core::fmt::Debug for ShapeLib {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        f.write_str("ShapeLib(libxcb-shape)")
    }
}

/// libxcb-dri3, when it is there. See [`RandrLib`].
pub struct Dri3Lib {
    /// `xcb_dri3_id`; see [`RandrLib::id`].
    pub id: usize,
    pub open: prototype::Dri3Open,
    pub open_reply: prototype::Dri3OpenReplyFn,
    pub open_reply_fds: prototype::Dri3OpenReplyFds,
}

impl core::fmt::Debug for Dri3Lib {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        f.write_str("Dri3Lib(libxcb-dri3)")
    }
}

/// Which optional extension libraries loaded.
#[derive(Debug)]
pub struct Ext {
    /// RandR, or `None` — then there is one monitor and no refresh rate.
    pub randr: Option<&'static RandrLib>,
    /// XInput 2, or `None` — then there is no unaccelerated pointer motion.
    pub xi: Option<&'static XiLib>,
    /// XKB, or `None` — then auto-repeat is recognized by the lookahead in
    /// [`input`](super::input), which a split socket read can defeat.
    pub xkb: Option<&'static XkbLib>,
    /// libxcb-cursor, or `None` — then a cursor shape is recorded and not
    /// applied, and [`CURSOR_SHAPE`](crate::ShellCaps::CURSOR_SHAPE) is clear.
    pub cursor: Option<&'static CursorLib>,
    /// SHAPE, or `None` — then an input region is not applied, and
    /// [`INPUT_REGION`](crate::ShellCaps::INPUT_REGION) is clear.
    pub shape: Option<&'static ShapeLib>,
    /// DRI3, or `None` — then no target names the server's GPU.
    pub dri3: Option<&'static Dri3Lib>,
}

/// As `crcbl_x11::ffi`'s sonames, for libxcb-xinput: the versioned name first.
const XI_SONAMES: [&CStr; 2] = [c"libxcb-xinput.so.0", c"libxcb-xinput.so"];
/// As [`XI_SONAMES`], for libxcb-xkb.
const XKB_SONAMES: [&CStr; 2] = [c"libxcb-xkb.so.1", c"libxcb-xkb.so"];
/// As [`XI_SONAMES`], for libxcb-cursor.
const CURSOR_SONAMES: [&CStr; 2] = [c"libxcb-cursor.so.0", c"libxcb-cursor.so"];
/// As [`XI_SONAMES`], for libxcb-shape.
const SHAPE_SONAMES: [&CStr; 2] = [c"libxcb-shape.so.0", c"libxcb-shape.so"];
/// As [`XI_SONAMES`], for libxcb-dri3.
const DRI3_SONAMES: [&CStr; 2] = [c"libxcb-dri3.so.0", c"libxcb-dri3.so"];

static RANDR: OnceLock<Option<&'static RandrLib>> = OnceLock::new();
static XI: OnceLock<Option<XiLib>> = OnceLock::new();
static XKB: OnceLock<Option<XkbLib>> = OnceLock::new();
static CURSOR: OnceLock<Option<CursorLib>> = OnceLock::new();
static SHAPE: OnceLock<Option<ShapeLib>> = OnceLock::new();
static DRI3: OnceLock<Option<Dri3Lib>> = OnceLock::new();

/// Loads the optional extension libraries, once per process.
///
/// Never fails: a missing library is a cleared capability bit.
#[must_use]
pub fn load_ext() -> Ext {
    Ext {
        randr: *RANDR.get_or_init(randr_or_warn),
        xi: XI.get_or_init(load_xi).as_ref(),
        xkb: XKB.get_or_init(load_xkb).as_ref(),
        cursor: CURSOR.get_or_init(load_cursor).as_ref(),
        shape: SHAPE.get_or_init(load_shape).as_ref(),
        dri3: DRI3.get_or_init(load_dri3).as_ref(),
    }
}

/// [`load_randr`], with the log line that says what its absence costs.
fn randr_or_warn() -> Option<&'static RandrLib> {
    load_randr()
        .inspect_err(|detail| {
            crcbl_core::log::warn!(
                "libxcb-randr is not usable ({detail}): one monitor covering the whole \
                 screen, and no refresh rate"
            );
        })
        .ok()
}

fn load_xi() -> Option<XiLib> {
    // SAFETY: libxcb-xinput, by soname.
    let handle = unsafe { open_library(&XI_SONAMES) };
    if handle.is_null() {
        crcbl_core::log::warn!(
            "libxcb-xinput is not available: no unaccelerated pointer motion, so \
             ShellCaps::RAW_POINTER_MOTION stays clear"
        );
        return None;
    }
    /// Resolves one symbol or gives up on the whole extension.
    macro_rules! symbol {
        ($name:literal, $ty:ty) => {{
            // SAFETY: `handle` is a live `dlopen` handle and the name is a
            // NUL-terminated literal.
            let raw = unsafe { dlsym(handle, concat!($name, "\0").as_ptr().cast()) };
            if raw.is_null() {
                crcbl_core::log::warn!("libxcb-xinput has no symbol {}; degrading", $name);
                return None;
            }
            // SAFETY: `$ty` is the prototype `xcb/xinput.h` declares for this
            // symbol.
            unsafe { core::mem::transmute::<*mut c_void, $ty>(raw) }
        }};
    }
    // SAFETY: as for `xcb_randr_id` — a data symbol, taken by address.
    let id = unsafe { dlsym(handle, c"xcb_input_id".as_ptr()) } as usize;
    if id == 0 {
        crcbl_core::log::warn!("libxcb-xinput has no xcb_input_id; degrading");
        return None;
    }
    Some(XiLib {
        id,
        query_version: symbol!("xcb_input_xi_query_version", prototype::XiQueryVersion),
        query_version_reply: symbol!(
            "xcb_input_xi_query_version_reply",
            prototype::XiQueryVersionReplyFn
        ),
        select_events: symbol!("xcb_input_xi_select_events", prototype::XiSelectEvents),
    })
}

fn load_xkb() -> Option<XkbLib> {
    // SAFETY: libxcb-xkb, by soname.
    let handle = unsafe { open_library(&XKB_SONAMES) };
    if handle.is_null() {
        crcbl_core::log::warn!(
            "libxcb-xkb is not available: auto-repeat falls back to the release/press \
             lookahead, which misreads a repeat whose two halves arrive in different reads"
        );
        return None;
    }
    /// Resolves one symbol or gives up on the whole extension.
    macro_rules! symbol {
        ($name:literal, $ty:ty) => {{
            // SAFETY: `handle` is a live `dlopen` handle and the name is a
            // NUL-terminated literal.
            let raw = unsafe { dlsym(handle, concat!($name, "\0").as_ptr().cast()) };
            if raw.is_null() {
                crcbl_core::log::warn!("libxcb-xkb has no symbol {}; degrading", $name);
                return None;
            }
            // SAFETY: `$ty` is the prototype `xcb/xkb.h` declares for this
            // symbol.
            unsafe { core::mem::transmute::<*mut c_void, $ty>(raw) }
        }};
    }
    Some(XkbLib {
        use_extension: symbol!("xcb_xkb_use_extension", prototype::XkbUseExtension),
        use_extension_reply: symbol!(
            "xcb_xkb_use_extension_reply",
            prototype::XkbUseExtensionReplyFn
        ),
        per_client_flags: symbol!("xcb_xkb_per_client_flags", prototype::XkbPerClientFlags),
        per_client_flags_reply: symbol!(
            "xcb_xkb_per_client_flags_reply",
            prototype::XkbPerClientFlagsReplyFn
        ),
    })
}

fn load_cursor() -> Option<CursorLib> {
    // SAFETY: libxcb-cursor, by soname.
    let handle = unsafe { open_library(&CURSOR_SONAMES) };
    if handle.is_null() {
        crcbl_core::log::warn!(
            "libxcb-cursor is not available: cursor shapes are recorded and not applied, so \
             ShellCaps::CURSOR_SHAPE stays clear"
        );
        return None;
    }
    /// Resolves one symbol or gives up on the whole library.
    macro_rules! symbol {
        ($name:literal, $ty:ty) => {{
            // SAFETY: `handle` is a live `dlopen` handle and the name is a
            // NUL-terminated literal.
            let raw = unsafe { dlsym(handle, concat!($name, "\0").as_ptr().cast()) };
            if raw.is_null() {
                crcbl_core::log::warn!("libxcb-cursor has no symbol {}; degrading", $name);
                return None;
            }
            // SAFETY: `$ty` is the prototype `xcb/xcb_cursor.h` declares for
            // this symbol.
            unsafe { core::mem::transmute::<*mut c_void, $ty>(raw) }
        }};
    }
    Some(CursorLib {
        context_new: symbol!("xcb_cursor_context_new", prototype::CursorContextNew),
        load_cursor: symbol!("xcb_cursor_load_cursor", prototype::CursorLoadCursor),
        context_free: symbol!("xcb_cursor_context_free", prototype::CursorContextFree),
    })
}

fn load_shape() -> Option<ShapeLib> {
    // SAFETY: libxcb-shape, by soname.
    let handle = unsafe { open_library(&SHAPE_SONAMES) };
    if handle.is_null() {
        crcbl_core::log::warn!(
            "libxcb-shape is not available: an input region is not applied, so \
             ShellCaps::INPUT_REGION stays clear"
        );
        return None;
    }
    /// Resolves one symbol or gives up on the whole extension.
    macro_rules! symbol {
        ($name:literal, $ty:ty) => {{
            // SAFETY: `handle` is a live `dlopen` handle and the name is a
            // NUL-terminated literal.
            let raw = unsafe { dlsym(handle, concat!($name, "\0").as_ptr().cast()) };
            if raw.is_null() {
                crcbl_core::log::warn!("libxcb-shape has no symbol {}; degrading", $name);
                return None;
            }
            // SAFETY: `$ty` is the prototype `xcb/shape.h` declares for this
            // symbol.
            unsafe { core::mem::transmute::<*mut c_void, $ty>(raw) }
        }};
    }
    // SAFETY: `xcb_shape_id` is a data symbol — an `xcb_extension_t` global —
    // so the `dlsym` result is its address and is not transmuted to a function.
    let id = unsafe { dlsym(handle, c"xcb_shape_id".as_ptr()) } as usize;
    if id == 0 {
        crcbl_core::log::warn!("libxcb-shape has no xcb_shape_id; degrading");
        return None;
    }
    Some(ShapeLib {
        id,
        query_version: symbol!("xcb_shape_query_version", prototype::ShapeQueryVersion),
        query_version_reply: symbol!(
            "xcb_shape_query_version_reply",
            prototype::ShapeQueryVersionReplyFn
        ),
        rectangles: symbol!("xcb_shape_rectangles", prototype::ShapeRectangles),
        mask: symbol!("xcb_shape_mask", prototype::ShapeMask),
    })
}

fn load_dri3() -> Option<Dri3Lib> {
    // SAFETY: libxcb-dri3, by soname.
    let handle = unsafe { open_library(&DRI3_SONAMES) };
    if handle.is_null() {
        crcbl_core::log::debug!(
            "libxcb-dri3 is not available: the renderer is not told which GPU the X server uses"
        );
        return None;
    }
    /// Resolves one symbol or gives up on the whole extension.
    macro_rules! symbol {
        ($name:literal, $ty:ty) => {{
            // SAFETY: `handle` is a live `dlopen` handle and the name is a
            // NUL-terminated literal.
            let raw = unsafe { dlsym(handle, concat!($name, "\0").as_ptr().cast()) };
            if raw.is_null() {
                crcbl_core::log::warn!("libxcb-dri3 has no symbol {}; degrading", $name);
                return None;
            }
            // SAFETY: `$ty` is the prototype `xcb/dri3.h` declares for this
            // symbol.
            unsafe { core::mem::transmute::<*mut c_void, $ty>(raw) }
        }};
    }
    // SAFETY: `xcb_dri3_id` is a data symbol — an `xcb_extension_t` global —
    // so the `dlsym` result is its address and is not transmuted to a function.
    let id = unsafe { dlsym(handle, c"xcb_dri3_id".as_ptr()) } as usize;
    if id == 0 {
        crcbl_core::log::warn!("libxcb-dri3 has no xcb_dri3_id; degrading");
        return None;
    }
    Some(Dri3Lib {
        id,
        open: symbol!("xcb_dri3_open", prototype::Dri3Open),
        open_reply: symbol!("xcb_dri3_open_reply", prototype::Dri3OpenReplyFn),
        open_reply_fds: symbol!("xcb_dri3_open_reply_fds", prototype::Dri3OpenReplyFds),
    })
}

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

/// Whether `fd` is readable right now.
///
/// A zero timeout makes this a poll in the strict sense — it never blocks,
/// which is what [`Shell::pump`](crate::Shell::pump)'s finite-drain contract
/// requires. `timeout_ms` of `-1` blocks indefinitely, which is what
/// [`Shell::wait_events`](crate::Shell::wait_events) wants.
#[must_use]
pub fn poll_readable(fd: c_int, timeout_ms: c_int) -> bool {
    let mut fds = PollFd {
        fd,
        events: POLLIN,
        revents: 0,
    };
    // SAFETY: `fds` is one initialised `pollfd` and the count matches.
    let result = unsafe { poll(&raw mut fds, 1, timeout_ms) };
    result > 0 && fds.revents & POLLIN != 0
}

/// Waits until `primary` or `other` is readable or the timeout elapses;
/// reports whether **`primary`** is readable.
///
/// `other` is the wake pipe: it ends the wait and changes nothing else, since
/// only the connection's descriptor carries the protocol.
#[must_use]
pub fn poll_readable_or(primary: c_int, other: c_int, timeout_ms: c_int) -> bool {
    let mut fds = [primary, other].map(|fd| PollFd {
        fd,
        events: POLLIN,
        revents: 0,
    });
    let count = Nfds::try_from(fds.len()).unwrap_or(1);
    // SAFETY: `fds` is a live, initialised array of exactly `count` `pollfd`s.
    let result = unsafe { poll(fds.as_mut_ptr(), count, timeout_ms) };
    result > 0 && fds[0].revents & POLLIN != 0
}

/// `CLOCK_MONOTONIC` in nanoseconds.
///
/// Returns zero if the call fails, which cannot happen for `CLOCK_MONOTONIC` on
/// any Linux this engine supports.
#[must_use]
pub fn monotonic_nanos() -> u64 {
    let mut ts = Timespec {
        tv_sec: 0,
        tv_nsec: 0,
    };
    // SAFETY: `ts` is an initialised `timespec` and `CLOCK_MONOTONIC` is valid.
    if unsafe { clock_gettime(CLOCK_MONOTONIC, &raw mut ts) } != 0 {
        return 0;
    }
    let seconds = u64::try_from(ts.tv_sec).unwrap_or(0);
    let nanos = u64::try_from(ts.tv_nsec).unwrap_or(0);
    seconds * 1_000_000_000 + nanos
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_extension_wire_structures_match_the_c_layout() {
        // As `crcbl_x11::ffi`'s layout test, for the structures that stayed
        // here. The sizes are the ones the extensions' `xcb/*.h` generate.
        assert_eq!(size_of::<XkbUseExtensionReply>(), 32);
        assert_eq!(size_of::<XkbPerClientFlagsReply>(), 32);
        assert_eq!(size_of::<ShapeQueryVersionReply>(), 12);
        assert_eq!(
            size_of::<XiRawEvent>(),
            36,
            "a GeGeneric's fixed part, with full_sequence inserted at 32"
        );
    }

    #[test]
    fn the_monotonic_clock_advances() {
        let first = monotonic_nanos();
        assert!(first > 0, "CLOCK_MONOTONIC is available on Linux");
        assert!(monotonic_nanos() >= first);
    }

    #[test]
    fn polling_a_closed_descriptor_does_not_block() {
        assert!(!poll_readable(-1, 0));
    }
}
