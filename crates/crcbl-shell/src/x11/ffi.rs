//! Hand-written FFI to libxcb, and the wire structures the X11 backend reads.
//!
//! # Decision: `dlopen`, not `#[link]` — the same argument, with more force
//!
//! [`wayland::ffi`](crate::wayland::ffi) states the case: a backend in a
//! fall-through registry has to be able to fail at *runtime*, and
//! `#[link(name = "xcb")]` makes a missing library kill the process in `ld.so`
//! before [`open`](crate::open) ever runs. Everything there applies here, and
//! two things make it sharper:
//!
//! * **X11 is the fallback, not the preference.** The Wayland entry failing is
//!   normal and expected on an X11 desktop; the X11 entry failing is normal and
//!   expected on a Wayland-only one. If either can pre-empt the loader, the
//!   list is decorative.
//! * **There are four libraries, not one.** libxcb is always present where
//!   there is an X server; `libxcb-randr` and `libxcb-xinput` are separately
//!   packaged and a minimal container may have neither. With `dlopen` a missing
//!   extension library is a cleared [`ShellCaps`](crate::ShellCaps) bit and a
//!   log line — see [`Ext`] — where with `DT_NEEDED` it would be a process that
//!   does not start.
//!
//! ## Sharing the connection with the Vulkan WSI
//!
//! `crcbl-vk` hands the `xcb_connection_t*` and the `xcb_window_t` from
//! [`SurfaceTarget::Xcb`](crcbl_core::SurfaceTarget) to
//! `vkCreateXcbSurfaceKHR`, and the driver calls libxcb on them. That works
//! across the `dlopen` boundary for the reason the Wayland module gives: the
//! driver's own `DT_NEEDED` on `libxcb.so.1` resolves to the object already
//! mapped in this process. One copy, one connection state, one sequence
//! number space.
//!
//! # Decision: the typed request wrappers, not `xcb_send_request`
//!
//! libxcb exposes both. `xcb_send_request` takes an `xcb_protocol_request_t`
//! plus an iovec array and would let this module resolve four symbols instead
//! of fifty — at the cost of hand-encoding every request body, including the
//! padding rules, in Rust. The typed wrappers already encode exactly that, they
//! are the part of libxcb whose ABI is generated from the same protocol
//! description the server implements, and a mistake in one of them is a
//! compile error in C rather than a silently malformed request from us. The
//! symbol count is the price of not reimplementing the protocol encoder.
//!
//! What is **not** taken from libxcb is anything above the wire:
//! `xcb-util`'s ICCCM and EWMH helpers are not loaded, because
//! `docs/plan/15-windowing.md` puts "our request/event layer (core, EWMH atoms,
//! RandR, XKB)" on our side of the line. [`super::atoms`] and
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
//!
//! Deliberately absent: `libxcb-xfixes` (pointer barriers are not needed —
//! `GrabPointer`'s `confine_to` *is* [`PointerMode::Confined`](crate::PointerMode)
//! and hiding the cursor is a core-protocol empty pixmap), `libxcb-cursor`
//! (a themed cursor needs an XCursor theme loader, which is the same gap the
//! Wayland backend documents and defers), and `libxcb-icccm`/`libxcb-ewmh`
//! (policy, see above).
//!
//! libxcb-xkb is used for **one request**. The keymap itself still comes from
//! libxkbcommon-x11, which links libxcb-xkb itself and is where that
//! conversation belongs; what is taken directly here is `XkbPerClientFlags`,
//! which libxkbcommon deliberately does not expose because it is an input-policy
//! decision rather than a keymap one. See
//! [`setup_detectable_auto_repeat`](super::Conn::setup_detectable_auto_repeat).

use core::ffi::{CStr, c_char, c_int, c_long, c_uint, c_ulong, c_void};
use core::ptr;
use std::sync::OnceLock;

// ---------------------------------------------------------------------------
// Opaque types and wire structures
// ---------------------------------------------------------------------------

/// `xcb_connection_t` — the connection. Opaque; libxcb owns the layout.
#[repr(C)]
pub struct Connection {
    _opaque: [u8; 0],
}

/// `xcb_void_cookie_t`, and every other cookie.
///
/// Every `xcb_*_cookie_t` in the protocol is `struct { unsigned int sequence; }`
/// — one field, same layout, returned by value. One Rust type covers all of
/// them, which is what keeps [`prototype`] readable.
#[repr(C)]
#[derive(Clone, Copy, Debug)]
pub struct Cookie {
    /// The request's sequence number.
    pub sequence: c_uint,
}

/// `xcb_generic_event_t` — the 32-byte header every event starts with.
///
/// Events are exactly 32 bytes on the wire (plus `full_sequence`, which libxcb
/// appends locally, and any trailing data for a `GeGeneric`). The backend reads
/// this to dispatch and then reinterprets the same allocation as the specific
/// event type.
#[repr(C)]
#[derive(Clone, Copy, Debug)]
pub struct GenericEvent {
    /// Event number, with bit 7 set when the event was sent by `SendEvent`.
    pub response_type: u8,
    /// Second byte — `detail` for most events, `error_code` for an error.
    pub pad0: u8,
    /// Low 16 bits of the sequence number of the last request seen.
    pub sequence: u16,
    /// Payload; interpreted by the specific event type.
    pub pad: [u32; 7],
    /// Full 32-bit sequence, reconstructed by libxcb.
    pub full_sequence: u32,
}

/// `xcb_generic_error_t`.
#[repr(C)]
#[derive(Clone, Copy, Debug)]
pub struct GenericError {
    /// Always `0` for an error.
    pub response_type: u8,
    /// Which error — `BadWindow`, `BadAtom`, …
    pub error_code: u8,
    /// Sequence number of the request that failed.
    pub sequence: u16,
    /// The resource id the request named, where one applies.
    pub resource_id: u32,
    /// Minor opcode, for extension requests.
    pub minor_code: u16,
    /// Major opcode.
    pub major_code: u8,
    /// Padding.
    pub pad0: u8,
    /// Padding.
    pub pad: [u32; 5],
    /// Full 32-bit sequence.
    pub full_sequence: u32,
}

/// `xcb_setup_t` — the connection handshake reply.
#[repr(C)]
#[derive(Clone, Copy, Debug)]
pub struct Setup {
    /// Always `1` on a successful connection.
    pub status: u8,
    /// Padding.
    pub pad0: u8,
    /// Protocol major version — `11`.
    pub protocol_major_version: u16,
    /// Protocol minor version.
    pub protocol_minor_version: u16,
    /// Length of this reply in 4-byte units.
    pub length: u16,
    /// Server release number.
    pub release_number: u32,
    /// Base of the XID range this client may allocate from.
    pub resource_id_base: u32,
    /// Mask of the XID range this client may allocate from.
    pub resource_id_mask: u32,
    /// Motion buffer size.
    pub motion_buffer_size: u32,
    /// Length of the vendor string.
    pub vendor_len: u16,
    /// **The cap on a single request, in 4-byte units.**
    ///
    /// The number that makes `INCR` exist: a property larger than this cannot
    /// be transferred in one `ChangeProperty`. See [`super::selection`].
    pub maximum_request_length: u16,
    /// Number of screens.
    pub roots_len: u8,
    /// Number of pixmap formats.
    pub pixmap_formats_len: u8,
    /// Image byte order.
    pub image_byte_order: u8,
    /// Bitmap format bit order.
    pub bitmap_format_bit_order: u8,
    /// Bitmap scanline unit.
    pub bitmap_format_scanline_unit: u8,
    /// Bitmap scanline pad.
    pub bitmap_format_scanline_pad: u8,
    /// Lowest keycode the server will ever report.
    pub min_keycode: u8,
    /// Highest keycode the server will ever report.
    pub max_keycode: u8,
    /// Padding.
    pub pad1: [u8; 4],
}

/// `xcb_screen_t`.
#[repr(C)]
#[derive(Clone, Copy, Debug)]
pub struct Screen {
    /// The root window — where EWMH properties and RandR live.
    pub root: u32,
    /// Default colormap.
    pub default_colormap: u32,
    /// White pixel value.
    pub white_pixel: u32,
    /// Black pixel value.
    pub black_pixel: u32,
    /// Event mask currently selected on the root by *other* clients.
    pub current_input_masks: u32,
    /// Screen width in pixels.
    pub width_in_pixels: u16,
    /// Screen height in pixels.
    pub height_in_pixels: u16,
    /// Screen width in millimetres — the only physical-size fact core X11 has.
    pub width_in_millimeters: u16,
    /// Screen height in millimetres.
    pub height_in_millimeters: u16,
    /// Minimum installed colormaps.
    pub min_installed_maps: u16,
    /// Maximum installed colormaps.
    pub max_installed_maps: u16,
    /// Visual of the root window; what a window inherits with `CopyFromParent`.
    pub root_visual: u32,
    /// Backing-store hint.
    pub backing_stores: u8,
    /// Save-unders hint.
    pub save_unders: u8,
    /// Depth of the root window.
    pub root_depth: u8,
    /// Number of depths in the allowed-depths list.
    pub allowed_depths_len: u8,
}

/// `xcb_screen_iterator_t`.
#[repr(C)]
#[derive(Clone, Copy, Debug)]
pub struct ScreenIterator {
    /// The screen this iterator is on.
    pub data: *mut Screen,
    /// How many remain after this one.
    pub rem: c_int,
    /// Byte index into the reply.
    pub index: c_int,
}

/// `xcb_intern_atom_reply_t`.
#[repr(C)]
#[derive(Clone, Copy, Debug)]
pub struct InternAtomReply {
    /// `1` for a reply.
    pub response_type: u8,
    /// Padding.
    pub pad0: u8,
    /// Sequence number.
    pub sequence: u16,
    /// Extra length in 4-byte units.
    pub length: u32,
    /// The atom, or `0` when `only_if_exists` was set and it did not.
    pub atom: u32,
}

/// `xcb_get_property_reply_t`.
///
/// The payload follows the struct and is reached through
/// [`Lib::get_property_value`] rather than by pointer arithmetic here, because
/// libxcb — not this file — owns where it put it.
#[repr(C)]
#[derive(Clone, Copy, Debug)]
pub struct GetPropertyReply {
    /// `1` for a reply.
    pub response_type: u8,
    /// Bits per element: `8`, `16` or `32`. `0` means the property is absent.
    pub format: u8,
    /// Sequence number.
    pub sequence: u16,
    /// Extra length in 4-byte units.
    pub length: u32,
    /// The property's type atom. `0` means it does not exist.
    pub type_: u32,
    /// Bytes left after what was returned — how a chunked read knows to
    /// continue.
    pub bytes_after: u32,
    /// Number of elements returned, in units of `format` bits.
    pub value_len: u32,
    /// Padding.
    pub pad0: [u8; 12],
}

/// `xcb_query_tree_reply_t`.
///
/// The children follow the struct and are reached through
/// [`Lib::query_tree_children`] rather than by pointer arithmetic here, because
/// libxcb — not this file — owns where it put them.
///
/// Only the harness walks another process's window tree, so this is behind the
/// `x11-e2e` feature like the [`Lib`] entry points that use it; see
/// [`Lib::query_tree`].
#[cfg(feature = "x11-e2e")]
#[repr(C)]
#[derive(Clone, Copy, Debug)]
pub struct QueryTreeReply {
    /// `1` for a reply.
    pub response_type: u8,
    /// Padding.
    pub pad0: u8,
    /// Sequence number.
    pub sequence: u16,
    /// Extra length in 4-byte units.
    pub length: u32,
    /// The root of the query.
    pub root: u32,
    /// The window's parent.
    pub parent: u32,
    /// How many child windows follow the struct.
    pub children_len: u16,
    /// Padding.
    pub pad1: [u8; 14],
}

/// `xcb_get_geometry_reply_t`.
#[repr(C)]
#[derive(Clone, Copy, Debug)]
pub struct GetGeometryReply {
    /// `1` for a reply.
    pub response_type: u8,
    /// Depth of the drawable.
    pub depth: u8,
    /// Sequence number.
    pub sequence: u16,
    /// Extra length.
    pub length: u32,
    /// The drawable's root window.
    pub root: u32,
    /// X relative to the parent — *not* the desktop, under a reparenting WM.
    pub x: i16,
    /// Y relative to the parent.
    pub y: i16,
    /// Width in pixels.
    pub width: u16,
    /// Height in pixels.
    pub height: u16,
    /// Border width.
    pub border_width: u16,
    /// Padding.
    pub pad0: [u8; 2],
}

/// `xcb_translate_coordinates_reply_t`.
#[repr(C)]
#[derive(Clone, Copy, Debug)]
pub struct TranslateCoordinatesReply {
    /// `1` for a reply.
    pub response_type: u8,
    /// Whether the destination window contains the point.
    pub same_screen: u8,
    /// Sequence number.
    pub sequence: u16,
    /// Extra length.
    pub length: u32,
    /// The child of the destination the point falls in, or `0`.
    pub child: u32,
    /// X in the destination window's coordinates.
    pub dst_x: i16,
    /// Y in the destination window's coordinates.
    pub dst_y: i16,
}

/// `xcb_query_pointer_reply_t`.
#[repr(C)]
#[derive(Clone, Copy, Debug)]
pub struct QueryPointerReply {
    /// `1` for a reply.
    pub response_type: u8,
    /// Whether the pointer is on the same screen as the queried window.
    pub same_screen: u8,
    /// Sequence number.
    pub sequence: u16,
    /// Extra length.
    pub length: u32,
    /// The root the pointer is on.
    pub root: u32,
    /// The child of the queried window the pointer is in, or `0`.
    pub child: u32,
    /// Pointer X in root coordinates.
    pub root_x: i16,
    /// Pointer Y in root coordinates.
    pub root_y: i16,
    /// Pointer X in the queried window's coordinates.
    pub win_x: i16,
    /// Pointer Y in the queried window's coordinates.
    pub win_y: i16,
    /// Modifier and button mask.
    pub mask: u16,
    /// Padding.
    pub pad0: [u8; 2],
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

/// `xcb_get_selection_owner_reply_t`.
#[repr(C)]
#[derive(Clone, Copy, Debug)]
pub struct GetSelectionOwnerReply {
    /// `1` for a reply.
    pub response_type: u8,
    /// Padding.
    pub pad0: u8,
    /// Sequence number.
    pub sequence: u16,
    /// Extra length.
    pub length: u32,
    /// The owning window, or `0` — which is "nobody owns it", the X11 spelling
    /// of an empty clipboard.
    pub owner: u32,
}

/// `xcb_grab_pointer_reply_t`.
#[repr(C)]
#[derive(Clone, Copy, Debug)]
pub struct GrabPointerReply {
    /// `1` for a reply.
    pub response_type: u8,
    /// `0` is `Success`; anything else means another client holds the grab, or
    /// the window is not viewable.
    pub status: u8,
    /// Sequence number.
    pub sequence: u16,
    /// Extra length.
    pub length: u32,
}

/// `xcb_query_extension_reply_t`.
#[repr(C)]
#[derive(Clone, Copy, Debug)]
pub struct QueryExtensionReply {
    /// `1` for a reply.
    pub response_type: u8,
    /// Padding.
    pub pad0: u8,
    /// Sequence number.
    pub sequence: u16,
    /// Extra length.
    pub length: u32,
    /// Whether the server has the extension.
    pub present: u8,
    /// Major opcode to use for its requests.
    pub major_opcode: u8,
    /// **Event number of the extension's first event.**
    ///
    /// Extension events are numbered from here, so `first_event + 0` is
    /// RandR's `ScreenChangeNotify`. Getting this wrong reads an unrelated core
    /// event as an extension one.
    pub first_event: u8,
    /// Error number of the extension's first error.
    pub first_error: u8,
}

/// `xcb_extension_t` — the per-extension descriptor libxcb caches lazily.
///
/// A **data** symbol, not a function: `xcb_randr_id` and `xcb_input_id` are
/// mutable globals inside their libraries that libxcb fills in on first use, so
/// they are resolved with `dlsym` like everything else and then only ever
/// passed back.
#[repr(C)]
pub struct Extension {
    _opaque: [u8; 0],
}

// ---------------------------------------------------------------------------
// RandR
// ---------------------------------------------------------------------------

/// `xcb_randr_query_version_reply_t`.
#[repr(C)]
#[derive(Clone, Copy, Debug)]
pub struct RandrQueryVersionReply {
    /// `1` for a reply.
    pub response_type: u8,
    /// Padding.
    pub pad0: u8,
    /// Sequence number.
    pub sequence: u16,
    /// Extra length.
    pub length: u32,
    /// Major version the server settled on.
    pub major_version: u32,
    /// Minor version the server settled on.
    pub minor_version: u32,
}

/// `xcb_randr_get_screen_resources_current_reply_t`.
#[repr(C)]
#[derive(Clone, Copy, Debug)]
pub struct RandrScreenResourcesReply {
    /// `1` for a reply.
    pub response_type: u8,
    /// Padding.
    pub pad0: u8,
    /// Sequence number.
    pub sequence: u16,
    /// Extra length.
    pub length: u32,
    /// Timestamp of the configuration.
    pub timestamp: u32,
    /// Timestamp of the last configuration change.
    pub config_timestamp: u32,
    /// Number of CRTCs in the reply.
    pub num_crtcs: u16,
    /// Number of outputs in the reply.
    pub num_outputs: u16,
    /// Number of mode descriptors.
    pub num_modes: u16,
    /// Total bytes of mode-name text.
    pub names_len: u16,
    /// Padding.
    pub pad1: [u8; 8],
}

/// `xcb_randr_mode_info_t` — one video mode.
///
/// The refresh rate is not in it: it is `dot_clock / (htotal * vtotal)`, which
/// is where 59.94 Hz comes from and why
/// [`MonitorInfo::refresh_millihertz`](crate::MonitorInfo) is millihertz.
#[repr(C)]
#[derive(Clone, Copy, Debug)]
pub struct RandrModeInfo {
    /// Mode id, as referenced by a CRTC.
    pub id: u32,
    /// Visible width.
    pub width: u16,
    /// Visible height.
    pub height: u16,
    /// Pixel clock in Hz.
    pub dot_clock: u32,
    /// Horizontal sync start.
    pub hsync_start: u16,
    /// Horizontal sync end.
    pub hsync_end: u16,
    /// Horizontal total.
    pub htotal: u16,
    /// Horizontal skew.
    pub hskew: u16,
    /// Vertical sync start.
    pub vsync_start: u16,
    /// Vertical sync end.
    pub vsync_end: u16,
    /// Vertical total.
    pub vtotal: u16,
    /// Length of the mode's name.
    pub name_len: u16,
    /// Mode flags — interlace, doublescan.
    pub mode_flags: u32,
}

/// `xcb_randr_get_crtc_info_reply_t`.
#[repr(C)]
#[derive(Clone, Copy, Debug)]
pub struct RandrCrtcInfoReply {
    /// `1` for a reply.
    pub response_type: u8,
    /// Configuration status.
    pub status: u8,
    /// Sequence number.
    pub sequence: u16,
    /// Extra length.
    pub length: u32,
    /// Timestamp.
    pub timestamp: u32,
    /// X in the screen's coordinate space.
    pub x: i16,
    /// Y in the screen's coordinate space.
    pub y: i16,
    /// Width in pixels.
    pub width: u16,
    /// Height in pixels.
    pub height: u16,
    /// The mode this CRTC is driving. `0` means the CRTC is disabled.
    pub mode: u32,
    /// Rotation.
    pub rotation: u16,
    /// Rotations this CRTC can do.
    pub rotations: u16,
    /// Number of outputs driven by this CRTC.
    pub num_outputs: u16,
    /// Number of outputs that *could* be driven by it.
    pub num_possible_outputs: u16,
}

/// `xcb_randr_get_output_info_reply_t`.
#[repr(C)]
#[derive(Clone, Copy, Debug)]
pub struct RandrOutputInfoReply {
    /// `1` for a reply.
    pub response_type: u8,
    /// Configuration status.
    pub status: u8,
    /// Sequence number.
    pub sequence: u16,
    /// Extra length.
    pub length: u32,
    /// Timestamp.
    pub timestamp: u32,
    /// The CRTC this output is on, or `0`.
    pub crtc: u32,
    /// Physical width in millimetres — the only real DPI source X11 has.
    pub mm_width: u32,
    /// Physical height in millimetres.
    pub mm_height: u32,
    /// Connection state: `0` connected, `1` disconnected, `2` unknown.
    pub connection: u8,
    /// Subpixel order.
    pub subpixel_order: u8,
    /// Number of CRTCs this output could be on.
    pub num_crtcs: u16,
    /// Number of modes.
    pub num_modes: u16,
    /// Number of preferred modes.
    pub num_preferred: u16,
    /// Number of clones.
    pub num_clones: u16,
    /// Length of the output's name.
    pub name_len: u16,
}

/// `xcb_randr_get_output_primary_reply_t`.
#[repr(C)]
#[derive(Clone, Copy, Debug)]
pub struct RandrOutputPrimaryReply {
    /// `1` for a reply.
    pub response_type: u8,
    /// Padding.
    pub pad0: u8,
    /// Sequence number.
    pub sequence: u16,
    /// Extra length.
    pub length: u32,
    /// The primary output, or `0` if none is set.
    pub output: u32,
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

// ---------------------------------------------------------------------------
// Event bodies
// ---------------------------------------------------------------------------

/// `xcb_key_press_event_t`, and byte-identically `KeyRelease`, `ButtonPress`,
/// `ButtonRelease` and `MotionNotify`.
///
/// One struct for five events because X11 defines them with one layout — the
/// only difference is what `detail` means (a keycode, a button number, or
/// nothing) and that is the caller's business.
///
/// `state` is the flattened modifier-and-button mask, sampled **before** this
/// event is applied. See
/// [`Keymap::update_key`](crate::linux::xkb::Keymap::update_key) for why that
/// makes it unsuitable as the modifier source.
#[repr(C)]
#[derive(Clone, Copy, Debug)]
pub struct InputEvent {
    /// Event number.
    pub response_type: u8,
    /// Keycode, button number, or motion detail.
    pub detail: u8,
    /// Sequence number.
    pub sequence: u16,
    /// Server timestamp in milliseconds since the server started.
    pub time: u32,
    /// The root window the event happened on.
    pub root: u32,
    /// The window the event was *delivered* to.
    pub event: u32,
    /// The child of `event` the pointer was in, or `0`.
    pub child: u32,
    /// Pointer X in root coordinates.
    pub root_x: i16,
    /// Pointer Y in root coordinates.
    pub root_y: i16,
    /// Pointer X in `event`'s coordinates.
    pub event_x: i16,
    /// Pointer Y in `event`'s coordinates.
    pub event_y: i16,
    /// Modifier and button mask.
    pub state: u16,
    /// Whether `event` is on the same screen as `root`.
    pub same_screen: u8,
    /// Padding.
    pub pad0: u8,
}

/// `xcb_focus_in_event_t`, and byte-identically `FocusOut`.
#[repr(C)]
#[derive(Clone, Copy, Debug)]
pub struct FocusEvent {
    /// Event number.
    pub response_type: u8,
    /// Why focus moved — `NotifyAncestor`, `NotifyPointer`, …
    pub detail: u8,
    /// Sequence number.
    pub sequence: u16,
    /// The window.
    pub event: u32,
    /// Grab mode: `NotifyNormal`, `NotifyGrab`, `NotifyUngrab`.
    ///
    /// The field that keeps a pointer grab from looking like a focus change —
    /// see [`X11Shell`](super::X11Shell)'s focus handling.
    pub mode: u8,
    /// Padding.
    pub pad0: [u8; 3],
}

/// `xcb_configure_notify_event_t`.
#[repr(C)]
#[derive(Clone, Copy, Debug)]
pub struct ConfigureNotifyEvent {
    /// Event number.
    pub response_type: u8,
    /// Padding.
    pub pad0: u8,
    /// Sequence number.
    pub sequence: u16,
    /// The window the event was selected on.
    pub event: u32,
    /// The window that changed.
    pub window: u32,
    /// The sibling this window is now above.
    pub above_sibling: u32,
    /// X **relative to the parent**, which under a reparenting window manager
    /// is the frame and not the desktop.
    pub x: i16,
    /// Y relative to the parent.
    pub y: i16,
    /// New width — this one *is* authoritative.
    pub width: u16,
    /// New height.
    pub height: u16,
    /// Border width.
    pub border_width: u16,
    /// Whether the window has override-redirect set.
    pub override_redirect: u8,
    /// Padding.
    pub pad1: u8,
}

/// `xcb_map_notify_event_t`, and byte-identically `UnmapNotify` and
/// `DestroyNotify` (whose trailing byte means something else and is unread).
#[repr(C)]
#[derive(Clone, Copy, Debug)]
pub struct WindowNotifyEvent {
    /// Event number.
    pub response_type: u8,
    /// Padding.
    pub pad0: u8,
    /// Sequence number.
    pub sequence: u16,
    /// The window the event was selected on.
    pub event: u32,
    /// The window it is about.
    pub window: u32,
    /// `override_redirect` for `MapNotify`, `from_configure` for `UnmapNotify`.
    pub flag: u8,
    /// Padding.
    pub pad1: [u8; 3],
}

/// [`WindowNotifyEvent`] padded to the 32 bytes `SendEvent` reads.
///
/// The same wire layout — a synthetic `UnmapNotify` is byte-identical to a real
/// one — but `xcb_send_event` copies 32 bytes from whatever it is handed, so a
/// 16-byte struct would send 16 bytes of this process's stack to the window
/// manager. `Conn::send_event` refuses to compile for anything else, which is
/// how that is prevented rather than remembered.
#[repr(C)]
#[derive(Clone, Copy, Debug)]
pub struct SyntheticWindowNotify {
    /// The event proper.
    pub event: WindowNotifyEvent,
    /// The bytes past the event's end, which the protocol leaves unspecified
    /// and this backend sends as zero.
    pub pad: [u8; 16],
}

/// `xcb_property_notify_event_t`.
#[repr(C)]
#[derive(Clone, Copy, Debug)]
pub struct PropertyNotifyEvent {
    /// Event number.
    pub response_type: u8,
    /// Padding.
    pub pad0: u8,
    /// Sequence number.
    pub sequence: u16,
    /// The window whose property changed.
    pub window: u32,
    /// Which property.
    pub atom: u32,
    /// Server timestamp.
    pub time: u32,
    /// `0` is `NewValue`, `1` is `Deleted`. The whole `INCR` handshake is these
    /// two alternating.
    pub state: u8,
    /// Padding.
    pub pad1: [u8; 3],
}

/// `xcb_selection_clear_event_t`.
#[repr(C)]
#[derive(Clone, Copy, Debug)]
pub struct SelectionClearEvent {
    /// Event number.
    pub response_type: u8,
    /// Padding.
    pub pad0: u8,
    /// Sequence number.
    pub sequence: u16,
    /// Server timestamp.
    pub time: u32,
    /// The window that *was* the owner.
    pub owner: u32,
    /// Which selection was lost.
    pub selection: u32,
}

/// `xcb_selection_request_event_t`.
#[repr(C)]
#[derive(Clone, Copy, Debug)]
pub struct SelectionRequestEvent {
    /// Event number.
    pub response_type: u8,
    /// Padding.
    pub pad0: u8,
    /// Sequence number.
    pub sequence: u16,
    /// The timestamp the requestor quoted.
    pub time: u32,
    /// The window that owns the selection — one of ours.
    pub owner: u32,
    /// The window that wants the data.
    pub requestor: u32,
    /// Which selection.
    pub selection: u32,
    /// Which format.
    pub target: u32,
    /// Where to write it. `0` is an obsolete client asking us to choose.
    pub property: u32,
}

/// `xcb_selection_notify_event_t`, padded to the 32 bytes `SendEvent` reads.
///
/// Both received (answering our `ConvertSelection`) and **sent** — the owner
/// half of the protocol replies with one of these through `SendEvent`, which is
/// why this struct is constructed here and not only read. The C struct is 24
/// bytes and `xcb_send_event` unconditionally reads 32 from the pointer it is
/// given, so a faithful 24-byte struct on the stack would send eight bytes of
/// whatever followed it. The trailing padding is that bug, fixed by
/// construction.
#[repr(C)]
#[derive(Clone, Copy, Debug, Default)]
pub struct SelectionNotifyEvent {
    /// Event number.
    pub response_type: u8,
    /// Padding.
    pub pad0: u8,
    /// Sequence number; ignored on send.
    pub sequence: u16,
    /// Server timestamp.
    pub time: u32,
    /// The window that asked.
    pub requestor: u32,
    /// Which selection.
    pub selection: u32,
    /// Which format was delivered.
    pub target: u32,
    /// Where it was written, or `0` for "I could not".
    pub property: u32,
    /// Padding to the 32 bytes `SendEvent` requires.
    pub pad1: [u8; 8],
}

/// `xcb_client_message_event_t`.
///
/// `WM_DELETE_WINDOW` arrives as one of these, and `_NET_WM_STATE` changes are
/// sent as one.
#[repr(C)]
#[derive(Clone, Copy, Debug, Default)]
pub struct ClientMessageEvent {
    /// Event number.
    pub response_type: u8,
    /// `8`, `16` or `32` — how `data` is to be read.
    pub format: u8,
    /// Sequence number; ignored on send.
    pub sequence: u16,
    /// The window it is about.
    pub window: u32,
    /// The message type atom.
    pub type_: u32,
    /// Five 32-bit words. The union's other arms are twenty bytes and ten
    /// shorts; format 32 is the only one this backend sends or reads.
    pub data: [u32; 5],
}

/// `xcb_mapping_notify_event_t`.
#[repr(C)]
#[derive(Clone, Copy, Debug)]
pub struct MappingNotifyEvent {
    /// Event number.
    pub response_type: u8,
    /// Padding.
    pub pad0: u8,
    /// Sequence number.
    pub sequence: u16,
    /// `0` modifier, `1` keyboard, `2` pointer.
    pub request: u8,
    /// First keycode affected.
    pub first_keycode: u8,
    /// How many keycodes.
    pub count: u8,
    /// Padding.
    pub pad1: u8,
}

// ---------------------------------------------------------------------------
// Protocol constants
// ---------------------------------------------------------------------------

/// Event numbers, from the core protocol. `response_type & 0x7f` is compared
/// against these; bit 7 marks an event delivered by `SendEvent`.
pub mod event {
    /// A key went down.
    pub const KEY_PRESS: u8 = 2;
    /// A key came up.
    pub const KEY_RELEASE: u8 = 3;
    /// A pointer button went down. Wheel notches arrive as buttons 4–7.
    pub const BUTTON_PRESS: u8 = 4;
    /// A pointer button came up.
    pub const BUTTON_RELEASE: u8 = 5;
    /// The pointer moved.
    pub const MOTION_NOTIFY: u8 = 6;
    /// The pointer entered a window.
    pub const ENTER_NOTIFY: u8 = 7;
    /// The pointer left a window.
    pub const LEAVE_NOTIFY: u8 = 8;
    /// The window gained keyboard focus.
    pub const FOCUS_IN: u8 = 9;
    /// The window lost keyboard focus.
    pub const FOCUS_OUT: u8 = 10;
    /// The window was destroyed.
    pub const DESTROY_NOTIFY: u8 = 17;
    /// The window was unmapped.
    pub const UNMAP_NOTIFY: u8 = 18;
    /// The window was mapped.
    pub const MAP_NOTIFY: u8 = 19;
    /// The window moved, resized, or changed stacking.
    pub const CONFIGURE_NOTIFY: u8 = 22;
    /// A property changed. `INCR` transfers are driven entirely by this.
    pub const PROPERTY_NOTIFY: u8 = 28;
    /// We lost ownership of a selection.
    pub const SELECTION_CLEAR: u8 = 29;
    /// Someone wants the selection converted.
    pub const SELECTION_REQUEST: u8 = 30;
    /// Our own `ConvertSelection` was answered.
    pub const SELECTION_NOTIFY: u8 = 31;
    /// The keyboard mapping changed.
    pub const MAPPING_NOTIFY: u8 = 34;
    /// An extension event with a payload longer than 32 bytes.
    pub const GE_GENERIC: u8 = 35;
    /// A message from another client — `WM_DELETE_WINDOW` arrives this way.
    pub const CLIENT_MESSAGE: u8 = 33;
}

/// `xcb_event_mask_t` bits, for `CreateWindow`/`ChangeWindowAttributes`.
pub mod event_mask {
    /// `XCB_EVENT_MASK_KEY_PRESS`.
    pub const KEY_PRESS: u32 = 1;
    /// `XCB_EVENT_MASK_KEY_RELEASE`.
    pub const KEY_RELEASE: u32 = 1 << 1;
    /// `XCB_EVENT_MASK_BUTTON_PRESS`.
    pub const BUTTON_PRESS: u32 = 1 << 2;
    /// `XCB_EVENT_MASK_BUTTON_RELEASE`.
    pub const BUTTON_RELEASE: u32 = 1 << 3;
    /// `XCB_EVENT_MASK_ENTER_WINDOW`.
    pub const ENTER_WINDOW: u32 = 1 << 4;
    /// `XCB_EVENT_MASK_LEAVE_WINDOW`.
    pub const LEAVE_WINDOW: u32 = 1 << 5;
    /// `XCB_EVENT_MASK_POINTER_MOTION`.
    pub const POINTER_MOTION: u32 = 1 << 6;
    /// `XCB_EVENT_MASK_EXPOSURE`.
    pub const EXPOSURE: u32 = 1 << 15;
    /// `XCB_EVENT_MASK_STRUCTURE_NOTIFY` — configure, map, unmap, destroy.
    pub const STRUCTURE_NOTIFY: u32 = 1 << 17;
    /// `XCB_EVENT_MASK_FOCUS_CHANGE`.
    pub const FOCUS_CHANGE: u32 = 1 << 21;
    /// `XCB_EVENT_MASK_PROPERTY_CHANGE`.
    pub const PROPERTY_CHANGE: u32 = 1 << 22;
}

/// `xcb_cw_t` — which attributes a `CreateWindow` value list carries.
///
/// The list must be written in **mask-bit order**, which is why these are used
/// as an ordered set rather than looked up individually.
pub mod cw {
    /// `XCB_CW_BACK_PIXEL`.
    pub const BACK_PIXEL: u32 = 1 << 1;
    /// `XCB_CW_EVENT_MASK`.
    pub const EVENT_MASK: u32 = 1 << 11;
}

/// `xcb_config_window_t` — which fields a `ConfigureWindow` carries.
pub mod config_window {
    /// `XCB_CONFIG_WINDOW_X`.
    pub const X: u16 = 1;
    /// `XCB_CONFIG_WINDOW_Y`.
    pub const Y: u16 = 1 << 1;
    /// `XCB_CONFIG_WINDOW_WIDTH`.
    ///
    /// Only the harness resizes a window it does not own — the backend asks a
    /// window manager through `_NET_WM_STATE` and is told the answer, and a
    /// client that resized itself behind the window manager's back would be
    /// fighting it. Feature-gated rather than left dead so that the reason is
    /// visible at the definition.
    #[cfg(feature = "x11-e2e")]
    pub const WIDTH: u16 = 1 << 2;
    /// `XCB_CONFIG_WINDOW_HEIGHT`; see [`WIDTH`].
    #[cfg(feature = "x11-e2e")]
    pub const HEIGHT: u16 = 1 << 3;
}

/// Assorted single-purpose protocol constants.
pub mod value {
    /// `XCB_COPY_FROM_PARENT`, for depth and visual.
    pub const COPY_FROM_PARENT: u32 = 0;
    /// `XCB_WINDOW_CLASS_INPUT_OUTPUT`.
    pub const INPUT_OUTPUT: u16 = 1;
    /// `XCB_PROP_MODE_REPLACE`.
    pub const PROP_MODE_REPLACE: u8 = 0;
    /// `XCB_PROP_MODE_APPEND`. Appending *nothing* is how a client with no
    /// recent input event learns the server's current time; see
    /// [`super::xselection`].
    pub const PROP_MODE_APPEND: u8 = 2;
    /// `XCB_ATOM_NONE` / `XCB_NONE` / `XCB_WINDOW_NONE`.
    pub const NONE: u32 = 0;
    /// `XCB_CURRENT_TIME`.
    pub const CURRENT_TIME: u32 = 0;
    /// `XCB_GRAB_MODE_ASYNC`.
    pub const GRAB_MODE_ASYNC: u8 = 1;
    /// `XCB_GRAB_STATUS_SUCCESS`.
    pub const GRAB_SUCCESS: u8 = 0;
    /// `XCB_ATOM_ATOM`.
    pub const ATOM_ATOM: u32 = 4;
    /// `XCB_ATOM_CARDINAL`.
    pub const ATOM_CARDINAL: u32 = 6;
    /// `XCB_ATOM_INTEGER`, the type ICCCM's `TIMESTAMP` target answers in.
    pub const ATOM_INTEGER: u32 = 19;
    /// `XCB_ATOM_STRING` — Latin-1, which is why `UTF8_STRING` exists.
    pub const ATOM_STRING: u32 = 31;
    /// `XCB_ATOM_WM_NAME`.
    pub const ATOM_WM_NAME: u32 = 39;
    /// `XCB_ATOM_WM_NORMAL_HINTS`.
    pub const ATOM_WM_NORMAL_HINTS: u32 = 40;
    /// `XCB_ATOM_WM_SIZE_HINTS`.
    pub const ATOM_WM_SIZE_HINTS: u32 = 41;
    /// `XCB_ATOM_WM_CLASS`.
    pub const ATOM_WM_CLASS: u32 = 67;
    /// `XCB_ATOM_WM_HINTS` — the ICCCM property *and* its own type atom.
    ///
    /// Not to be confused with [`ATOM_WM_NORMAL_HINTS`](Self::ATOM_WM_NORMAL_HINTS),
    /// which is the geometry one. This is the property that says whether the
    /// window wants the keyboard.
    pub const ATOM_WM_HINTS: u32 = 35;
    /// `XCB_ATOM_RESOURCE_MANAGER` — the root property `Xft.dpi` lives in.
    pub const ATOM_RESOURCE_MANAGER: u32 = 23;
    /// `XCB_INPUT_DEVICE_ALL_MASTER`.
    pub const XI_ALL_MASTER_DEVICES: u16 = 1;
    /// `XCB_INPUT_RAW_MOTION` — the XI2 event number, not a mask bit.
    pub const XI_RAW_MOTION: u16 = 17;
    /// The mask bit for [`XI_RAW_MOTION`](Self::XI_RAW_MOTION) in an
    /// `xcb_input_event_mask_t`.
    pub const XI_RAW_MOTION_MASK: u32 = 1 << 17;
    /// `XCB_RANDR_NOTIFY_MASK_SCREEN_CHANGE`.
    pub const RANDR_SCREEN_CHANGE_MASK: u16 = 1;
    /// `XCB_RANDR_NOTIFY_MASK_CRTC_CHANGE`.
    pub const RANDR_CRTC_CHANGE_MASK: u16 = 1 << 1;
    /// `XCB_RANDR_NOTIFY_MASK_OUTPUT_CHANGE`.
    pub const RANDR_OUTPUT_CHANGE_MASK: u16 = 1 << 2;
    /// `XCB_XKB_ID_USE_CORE_KBD` — the device spec meaning "whichever keyboard
    /// the core protocol is reporting", which is the one core `KeyPress` events
    /// come from and therefore the only one this backend has an opinion about.
    pub const XKB_USE_CORE_KBD: u16 = 0x0100;
    /// `XCB_XKB_PER_CLIENT_FLAG_DETECTABLE_AUTO_REPEAT`.
    pub const XKB_DETECTABLE_AUTO_REPEAT: u32 = 1;
    /// The XKB version this backend asks for. 1.0 is the version that defined
    /// `XkbPerClientFlags`, so anything newer also has it.
    pub const XKB_MAJOR: u16 = 1;
    /// See [`XKB_MAJOR`](Self::XKB_MAJOR).
    pub const XKB_MINOR: u16 = 0;
}

// ---------------------------------------------------------------------------
// The libraries
// ---------------------------------------------------------------------------

// As in `wayland::ffi`: three loader functions and `poll` with a stable ABI,
// taken from libc, which `std` already links on every Unix target.
unsafe extern "C" {
    fn dlopen(filename: *const c_char, flags: c_int) -> *mut c_void;
    fn dlsym(handle: *mut c_void, symbol: *const c_char) -> *mut c_void;
    fn dlerror() -> *const c_char;
    fn free(ptr: *mut c_void);
    fn poll(fds: *mut PollFd, nfds: c_ulong, timeout: c_int) -> c_int;
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

/// `RTLD_NOW`; see [`wayland::ffi`](crate::wayland::ffi).
const RTLD_NOW: c_int = 2;
/// `POLLIN`.
const POLLIN: i16 = 0x001;
/// `CLOCK_MONOTONIC` — the clock the X server stamps events from.
const CLOCK_MONOTONIC: c_int = 1;

/// The C prototypes this backend asserts about libxcb.
///
/// Same convention as [`wayland::ffi::prototype`](crate::wayland::ffi): the
/// type is named once and used both in [`Lib`] and in the `transmute` that
/// fills it. Every one is copied from `xcb/xproto.h` or `xcb/xcb.h` and is
/// ABI-stable across libxcb 1.x.
///
/// Requests that return no reply are declared as returning [`Cookie`] rather
/// than `()` because they do — `xcb_void_cookie_t` — and the return value is
/// what [`Lib::request_check`] needs to turn a request into a checked one.
mod prototype {
    #[cfg(feature = "x11-e2e")]
    use super::QueryTreeReply;
    use super::{
        Connection, Cookie, Extension, GenericError, GenericEvent, GetGeometryReply,
        GetPropertyReply, GetSelectionOwnerReply, GrabPointerReply, InternAtomReply,
        QueryExtensionReply, QueryPointerReply, RandrCrtcInfoReply, RandrOutputInfoReply,
        RandrOutputPrimaryReply, RandrQueryVersionReply, RandrScreenResourcesReply, ScreenIterator,
        Setup, TranslateCoordinatesReply, XiEventMask, XiQueryVersionReply, XkbPerClientFlagsReply,
        XkbUseExtensionReply, c_char, c_int, c_void,
    };

    /// `xcb_connection_t *xcb_connect(const char *displayname, int *screenp)`
    pub type Connect = unsafe extern "C" fn(*const c_char, *mut c_int) -> *mut Connection;
    /// `void xcb_disconnect(xcb_connection_t *)`
    pub type Disconnect = unsafe extern "C" fn(*mut Connection);
    /// `int xcb_connection_has_error(xcb_connection_t *)`, and
    /// `int xcb_flush(…)`, and `int xcb_get_file_descriptor(…)`.
    pub type ConnInt = unsafe extern "C" fn(*mut Connection) -> c_int;
    /// `uint32_t xcb_generate_id(xcb_connection_t *)`
    pub type GenerateId = unsafe extern "C" fn(*mut Connection) -> u32;
    /// `const struct xcb_setup_t *xcb_get_setup(xcb_connection_t *)`
    pub type GetSetup = unsafe extern "C" fn(*mut Connection) -> *const Setup;
    /// `xcb_screen_iterator_t xcb_setup_roots_iterator(const xcb_setup_t *)`
    pub type SetupRootsIterator = unsafe extern "C" fn(*const Setup) -> ScreenIterator;
    /// `void xcb_screen_next(xcb_screen_iterator_t *)`
    pub type ScreenNext = unsafe extern "C" fn(*mut ScreenIterator);
    /// `xcb_generic_event_t *xcb_poll_for_event(xcb_connection_t *)`
    pub type PollForEvent = unsafe extern "C" fn(*mut Connection) -> *mut GenericEvent;
    /// `const xcb_query_extension_reply_t *xcb_get_extension_data(
    /// xcb_connection_t *, xcb_extension_t *)`
    pub type GetExtensionData =
        unsafe extern "C" fn(*mut Connection, *mut Extension) -> *const QueryExtensionReply;

    /// `xcb_void_cookie_t xcb_create_window(xcb_connection_t *, uint8_t depth,
    /// xcb_window_t wid, xcb_window_t parent, int16_t x, int16_t y,
    /// uint16_t width, uint16_t height, uint16_t border_width,
    /// uint16_t _class, xcb_visualid_t visual, uint32_t value_mask,
    /// const void *value_list)`
    pub type CreateWindow = unsafe extern "C" fn(
        *mut Connection,
        u8,
        u32,
        u32,
        i16,
        i16,
        u16,
        u16,
        u16,
        u16,
        u32,
        u32,
        *const c_void,
    ) -> Cookie;
    /// `xcb_void_cookie_t xcb_destroy_window(xcb_connection_t *, xcb_window_t)`,
    /// and `xcb_map_window`/`xcb_unmap_window`, which have the same signature.
    pub type WindowRequest = unsafe extern "C" fn(*mut Connection, u32) -> Cookie;
    /// `xcb_void_cookie_t xcb_change_window_attributes(xcb_connection_t *,
    /// xcb_window_t, uint32_t value_mask, const void *value_list)`
    pub type ChangeWindowAttributes =
        unsafe extern "C" fn(*mut Connection, u32, u32, *const c_void) -> Cookie;
    /// `xcb_void_cookie_t xcb_configure_window(xcb_connection_t *,
    /// xcb_window_t, uint16_t value_mask, const void *value_list)`
    pub type ConfigureWindow =
        unsafe extern "C" fn(*mut Connection, u32, u16, *const c_void) -> Cookie;
    /// `xcb_void_cookie_t xcb_change_property(xcb_connection_t *, uint8_t mode,
    /// xcb_window_t, xcb_atom_t property, xcb_atom_t type, uint8_t format,
    /// uint32_t data_len, const void *data)`
    pub type ChangeProperty =
        unsafe extern "C" fn(*mut Connection, u8, u32, u32, u32, u8, u32, *const c_void) -> Cookie;
    /// `xcb_void_cookie_t xcb_delete_property(xcb_connection_t *, xcb_window_t,
    /// xcb_atom_t)`
    pub type DeleteProperty = unsafe extern "C" fn(*mut Connection, u32, u32) -> Cookie;
    /// `xcb_get_property_cookie_t xcb_get_property(xcb_connection_t *,
    /// uint8_t _delete, xcb_window_t, xcb_atom_t property, xcb_atom_t type,
    /// uint32_t long_offset, uint32_t long_length)`
    pub type GetProperty =
        unsafe extern "C" fn(*mut Connection, u8, u32, u32, u32, u32, u32) -> Cookie;
    /// `xcb_get_property_reply_t *xcb_get_property_reply(xcb_connection_t *,
    /// xcb_get_property_cookie_t, xcb_generic_error_t **)`
    pub type GetPropertyReplyFn = unsafe extern "C" fn(
        *mut Connection,
        Cookie,
        *mut *mut GenericError,
    ) -> *mut GetPropertyReply;
    /// `void *xcb_get_property_value(const xcb_get_property_reply_t *)`
    pub type GetPropertyValue = unsafe extern "C" fn(*const GetPropertyReply) -> *mut c_void;
    /// `int xcb_get_property_value_length(const xcb_get_property_reply_t *)`
    pub type GetPropertyValueLength = unsafe extern "C" fn(*const GetPropertyReply) -> c_int;
    /// `xcb_intern_atom_cookie_t xcb_intern_atom(xcb_connection_t *,
    /// uint8_t only_if_exists, uint16_t name_len, const char *name)`
    pub type InternAtom = unsafe extern "C" fn(*mut Connection, u8, u16, *const c_char) -> Cookie;
    /// `xcb_intern_atom_reply_t *xcb_intern_atom_reply(xcb_connection_t *,
    /// xcb_intern_atom_cookie_t, xcb_generic_error_t **)`
    pub type InternAtomReplyFn = unsafe extern "C" fn(
        *mut Connection,
        Cookie,
        *mut *mut GenericError,
    ) -> *mut InternAtomReply;
    /// `xcb_get_geometry_cookie_t xcb_get_geometry(xcb_connection_t *,
    /// xcb_drawable_t)`, and `xcb_get_selection_owner`, which is the same shape.
    pub type DrawableRequest = unsafe extern "C" fn(*mut Connection, u32) -> Cookie;
    /// `xcb_get_geometry_reply_t *xcb_get_geometry_reply(…)`
    pub type GetGeometryReplyFn = unsafe extern "C" fn(
        *mut Connection,
        Cookie,
        *mut *mut GenericError,
    ) -> *mut GetGeometryReply;
    /// `xcb_translate_coordinates_cookie_t xcb_translate_coordinates(
    /// xcb_connection_t *, xcb_window_t src, xcb_window_t dst, int16_t src_x,
    /// int16_t src_y)`
    pub type TranslateCoordinates =
        unsafe extern "C" fn(*mut Connection, u32, u32, i16, i16) -> Cookie;
    /// `xcb_translate_coordinates_reply_t *xcb_translate_coordinates_reply(…)`
    pub type TranslateCoordinatesReplyFn = unsafe extern "C" fn(
        *mut Connection,
        Cookie,
        *mut *mut GenericError,
    )
        -> *mut TranslateCoordinatesReply;
    /// `xcb_query_tree_cookie_t xcb_query_tree(xcb_connection_t *,
    /// xcb_window_t window)`
    ///
    /// Only the harness walks another process's window tree, so these four are
    /// behind the `x11-e2e` feature like the [`Lib`] fields that use them;
    /// see [`super::Lib::query_tree`].
    #[cfg(feature = "x11-e2e")]
    pub type QueryTree = unsafe extern "C" fn(*mut Connection, u32) -> Cookie;
    /// `xcb_query_tree_reply_t *xcb_query_tree_reply(xcb_connection_t *,
    /// xcb_query_tree_cookie_t, xcb_generic_error_t **)`
    #[cfg(feature = "x11-e2e")]
    pub type QueryTreeReplyFn = unsafe extern "C" fn(
        *mut Connection,
        Cookie,
        *mut *mut GenericError,
    ) -> *mut QueryTreeReply;
    /// `xcb_window_t *xcb_query_tree_children(const xcb_query_tree_reply_t *)`
    #[cfg(feature = "x11-e2e")]
    pub type QueryTreeChildren = unsafe extern "C" fn(*const QueryTreeReply) -> *mut u32;
    /// `int xcb_query_tree_children_length(const xcb_query_tree_reply_t *)`
    #[cfg(feature = "x11-e2e")]
    pub type QueryTreeChildrenLength = unsafe extern "C" fn(*const QueryTreeReply) -> c_int;
    /// `xcb_void_cookie_t xcb_send_event(xcb_connection_t *, uint8_t propagate,
    /// xcb_window_t destination, uint32_t event_mask, const char *event)`
    pub type SendEvent =
        unsafe extern "C" fn(*mut Connection, u8, u32, u32, *const c_char) -> Cookie;
    /// `xcb_void_cookie_t xcb_set_selection_owner(xcb_connection_t *,
    /// xcb_window_t owner, xcb_atom_t selection, xcb_timestamp_t time)`
    pub type SetSelectionOwner = unsafe extern "C" fn(*mut Connection, u32, u32, u32) -> Cookie;
    /// `xcb_get_selection_owner_reply_t *xcb_get_selection_owner_reply(…)`
    pub type GetSelectionOwnerReplyFn = unsafe extern "C" fn(
        *mut Connection,
        Cookie,
        *mut *mut GenericError,
    ) -> *mut GetSelectionOwnerReply;
    /// `xcb_void_cookie_t xcb_convert_selection(xcb_connection_t *,
    /// xcb_window_t requestor, xcb_atom_t selection, xcb_atom_t target,
    /// xcb_atom_t property, xcb_timestamp_t time)`
    pub type ConvertSelection =
        unsafe extern "C" fn(*mut Connection, u32, u32, u32, u32, u32) -> Cookie;
    /// `xcb_grab_pointer_cookie_t xcb_grab_pointer(xcb_connection_t *,
    /// uint8_t owner_events, xcb_window_t grab_window, uint16_t event_mask,
    /// uint8_t pointer_mode, uint8_t keyboard_mode, xcb_window_t confine_to,
    /// xcb_cursor_t cursor, xcb_timestamp_t time)`
    pub type GrabPointer =
        unsafe extern "C" fn(*mut Connection, u8, u32, u16, u8, u8, u32, u32, u32) -> Cookie;
    /// `xcb_grab_pointer_reply_t *xcb_grab_pointer_reply(…)`
    pub type GrabPointerReplyFn = unsafe extern "C" fn(
        *mut Connection,
        Cookie,
        *mut *mut GenericError,
    ) -> *mut GrabPointerReply;
    /// `xcb_void_cookie_t xcb_ungrab_pointer(xcb_connection_t *,
    /// xcb_timestamp_t time)`
    pub type UngrabPointer = unsafe extern "C" fn(*mut Connection, u32) -> Cookie;
    /// `xcb_void_cookie_t xcb_warp_pointer(xcb_connection_t *,
    /// xcb_window_t src_window, xcb_window_t dst_window, int16_t src_x,
    /// int16_t src_y, uint16_t src_width, uint16_t src_height, int16_t dst_x,
    /// int16_t dst_y)`
    pub type WarpPointer =
        unsafe extern "C" fn(*mut Connection, u32, u32, i16, i16, u16, u16, i16, i16) -> Cookie;
    /// `xcb_query_pointer_reply_t *xcb_query_pointer_reply(…)`
    pub type QueryPointerReplyFn = unsafe extern "C" fn(
        *mut Connection,
        Cookie,
        *mut *mut GenericError,
    ) -> *mut QueryPointerReply;
    /// `xcb_void_cookie_t xcb_create_pixmap(xcb_connection_t *, uint8_t depth,
    /// xcb_pixmap_t pid, xcb_drawable_t drawable, uint16_t width,
    /// uint16_t height)`
    pub type CreatePixmap = unsafe extern "C" fn(*mut Connection, u8, u32, u32, u16, u16) -> Cookie;
    /// `xcb_void_cookie_t xcb_create_cursor(xcb_connection_t *,
    /// xcb_cursor_t cid, xcb_pixmap_t source, xcb_pixmap_t mask,
    /// uint16_t fore_red, fore_green, fore_blue, back_red, back_green,
    /// back_blue, uint16_t x, uint16_t y)`
    pub type CreateCursor = unsafe extern "C" fn(
        *mut Connection,
        u32,
        u32,
        u32,
        u16,
        u16,
        u16,
        u16,
        u16,
        u16,
        u16,
        u16,
    ) -> Cookie;
    /// `xcb_void_cookie_t xcb_free_pixmap(xcb_connection_t *, xcb_pixmap_t)`,
    /// and `xcb_free_cursor`, which is the same shape.
    pub type FreeResource = unsafe extern "C" fn(*mut Connection, u32) -> Cookie;

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

    /// `xcb_randr_query_version_cookie_t xcb_randr_query_version(
    /// xcb_connection_t *, uint32_t major, uint32_t minor)`
    pub type RandrQueryVersion = unsafe extern "C" fn(*mut Connection, u32, u32) -> Cookie;
    /// `xcb_randr_query_version_reply_t *xcb_randr_query_version_reply(…)`
    pub type RandrQueryVersionReplyFn = unsafe extern "C" fn(
        *mut Connection,
        Cookie,
        *mut *mut GenericError,
    ) -> *mut RandrQueryVersionReply;
    /// `xcb_randr_get_screen_resources_current_cookie_t
    /// xcb_randr_get_screen_resources_current(xcb_connection_t *,
    /// xcb_window_t)`, and `xcb_randr_get_output_primary`.
    pub type RandrWindowRequest = unsafe extern "C" fn(*mut Connection, u32) -> Cookie;
    /// `xcb_randr_get_screen_resources_current_reply_t
    /// *xcb_randr_get_screen_resources_current_reply(…)`
    pub type RandrScreenResourcesReplyFn = unsafe extern "C" fn(
        *mut Connection,
        Cookie,
        *mut *mut GenericError,
    )
        -> *mut RandrScreenResourcesReply;
    /// `xcb_randr_crtc_t *xcb_randr_get_screen_resources_current_crtcs(
    /// const xcb_randr_get_screen_resources_current_reply_t *)`
    pub type RandrResourcesCrtcs =
        unsafe extern "C" fn(*const RandrScreenResourcesReply) -> *mut u32;
    /// `xcb_randr_mode_info_t *xcb_randr_get_screen_resources_current_modes(
    /// const xcb_randr_get_screen_resources_current_reply_t *)`
    pub type RandrResourcesModes =
        unsafe extern "C" fn(*const RandrScreenResourcesReply) -> *mut super::RandrModeInfo;
    /// `xcb_randr_get_crtc_info_cookie_t xcb_randr_get_crtc_info(
    /// xcb_connection_t *, xcb_randr_crtc_t, xcb_timestamp_t)`, and
    /// `xcb_randr_get_output_info`, which is the same shape.
    pub type RandrInfoRequest = unsafe extern "C" fn(*mut Connection, u32, u32) -> Cookie;
    /// `xcb_randr_get_crtc_info_reply_t *xcb_randr_get_crtc_info_reply(…)`
    pub type RandrCrtcInfoReplyFn = unsafe extern "C" fn(
        *mut Connection,
        Cookie,
        *mut *mut GenericError,
    ) -> *mut RandrCrtcInfoReply;
    /// `xcb_randr_output_t *xcb_randr_get_crtc_info_outputs(
    /// const xcb_randr_get_crtc_info_reply_t *)`
    pub type RandrCrtcOutputs = unsafe extern "C" fn(*const RandrCrtcInfoReply) -> *mut u32;
    /// `xcb_randr_get_output_info_reply_t *xcb_randr_get_output_info_reply(…)`
    pub type RandrOutputInfoReplyFn = unsafe extern "C" fn(
        *mut Connection,
        Cookie,
        *mut *mut GenericError,
    ) -> *mut RandrOutputInfoReply;
    /// `uint8_t *xcb_randr_get_output_info_name(
    /// const xcb_randr_get_output_info_reply_t *)`
    pub type RandrOutputName = unsafe extern "C" fn(*const RandrOutputInfoReply) -> *mut u8;
    /// `xcb_randr_get_output_primary_reply_t
    /// *xcb_randr_get_output_primary_reply(…)`
    pub type RandrOutputPrimaryReplyFn = unsafe extern "C" fn(
        *mut Connection,
        Cookie,
        *mut *mut GenericError,
    ) -> *mut RandrOutputPrimaryReply;
    /// `xcb_void_cookie_t xcb_randr_select_input(xcb_connection_t *,
    /// xcb_window_t, uint16_t enable)`
    pub type RandrSelectInput = unsafe extern "C" fn(*mut Connection, u32, u16) -> Cookie;

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
}

/// Every libxcb entry point this backend uses.
///
/// Forty-four functions, plus four behind the `x11-e2e` feature for the
/// harness that walks another process's window tree. Each one is used; a
/// function that stops being used is deleted from this struct in the same
/// commit, which is what "audited by use" means in
/// `docs/plan/15-windowing.md`.
pub struct Lib {
    pub connect: prototype::Connect,
    pub disconnect: prototype::Disconnect,
    pub connection_has_error: prototype::ConnInt,
    pub flush: prototype::ConnInt,
    pub get_file_descriptor: prototype::ConnInt,
    pub generate_id: prototype::GenerateId,
    pub get_setup: prototype::GetSetup,
    pub setup_roots_iterator: prototype::SetupRootsIterator,
    pub screen_next: prototype::ScreenNext,
    pub poll_for_event: prototype::PollForEvent,
    pub get_extension_data: prototype::GetExtensionData,
    pub create_window: prototype::CreateWindow,
    pub destroy_window: prototype::WindowRequest,
    pub map_window: prototype::WindowRequest,
    pub unmap_window: prototype::WindowRequest,
    pub change_window_attributes: prototype::ChangeWindowAttributes,
    pub configure_window: prototype::ConfigureWindow,
    pub change_property: prototype::ChangeProperty,
    pub delete_property: prototype::DeleteProperty,
    pub get_property: prototype::GetProperty,
    pub get_property_reply: prototype::GetPropertyReplyFn,
    pub get_property_value: prototype::GetPropertyValue,
    pub get_property_value_length: prototype::GetPropertyValueLength,
    /// `xcb_query_tree_cookie_t xcb_query_tree(xcb_connection_t *,
    /// xcb_window_t window)`
    ///
    /// Only the harness walks another process's window tree — `find_window`
    /// in `x11_test_support` descends it to find the sandbox's window and
    /// close it cleanly. Feature-gated rather than left dead so that the
    /// reason is visible at the definition.
    #[cfg(feature = "x11-e2e")]
    pub query_tree: prototype::QueryTree,
    /// `xcb_query_tree_reply_t *xcb_query_tree_reply(…)`; see [`query_tree`].
    #[cfg(feature = "x11-e2e")]
    pub query_tree_reply: prototype::QueryTreeReplyFn,
    /// `xcb_window_t *xcb_query_tree_children(const xcb_query_tree_reply_t *)`;
    /// see [`query_tree`].
    #[cfg(feature = "x11-e2e")]
    pub query_tree_children: prototype::QueryTreeChildren,
    /// `int xcb_query_tree_children_length(const xcb_query_tree_reply_t *)`;
    /// see [`query_tree`].
    #[cfg(feature = "x11-e2e")]
    pub query_tree_children_length: prototype::QueryTreeChildrenLength,
    pub intern_atom: prototype::InternAtom,
    pub intern_atom_reply: prototype::InternAtomReplyFn,
    pub get_geometry: prototype::DrawableRequest,
    pub get_geometry_reply: prototype::GetGeometryReplyFn,
    pub translate_coordinates: prototype::TranslateCoordinates,
    pub translate_coordinates_reply: prototype::TranslateCoordinatesReplyFn,
    pub send_event: prototype::SendEvent,
    pub set_selection_owner: prototype::SetSelectionOwner,
    pub get_selection_owner: prototype::DrawableRequest,
    pub get_selection_owner_reply: prototype::GetSelectionOwnerReplyFn,
    pub convert_selection: prototype::ConvertSelection,
    pub grab_pointer: prototype::GrabPointer,
    pub grab_pointer_reply: prototype::GrabPointerReplyFn,
    pub ungrab_pointer: prototype::UngrabPointer,
    pub warp_pointer: prototype::WarpPointer,
    pub query_pointer: prototype::DrawableRequest,
    pub query_pointer_reply: prototype::QueryPointerReplyFn,
    pub create_pixmap: prototype::CreatePixmap,
    pub create_cursor: prototype::CreateCursor,
    pub free_pixmap: prototype::FreeResource,
    pub free_cursor: prototype::FreeResource,
}

impl core::fmt::Debug for Lib {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        f.write_str("Lib(libxcb)")
    }
}

/// libxcb-randr, when it is there.
///
/// A separate struct behind an [`Option`] rather than more fields on [`Lib`],
/// because "RandR is missing" has to be an ordinary runtime state — see the
/// [module docs](self).
pub struct RandrLib {
    /// `xcb_randr_id`, a data symbol libxcb fills in on first use.
    ///
    /// Held as an address rather than a pointer so the whole struct is
    /// `Send + Sync` by construction and can live in a `OnceLock` — see
    /// [`extension_ptr`].
    pub id: usize,
    pub query_version: prototype::RandrQueryVersion,
    pub query_version_reply: prototype::RandrQueryVersionReplyFn,
    pub get_screen_resources_current: prototype::RandrWindowRequest,
    pub get_screen_resources_current_reply: prototype::RandrScreenResourcesReplyFn,
    pub resources_crtcs: prototype::RandrResourcesCrtcs,
    pub resources_modes: prototype::RandrResourcesModes,
    pub get_crtc_info: prototype::RandrInfoRequest,
    pub get_crtc_info_reply: prototype::RandrCrtcInfoReplyFn,
    pub crtc_outputs: prototype::RandrCrtcOutputs,
    pub get_output_info: prototype::RandrInfoRequest,
    pub get_output_info_reply: prototype::RandrOutputInfoReplyFn,
    pub output_name: prototype::RandrOutputName,
    pub get_output_primary: prototype::RandrWindowRequest,
    pub get_output_primary_reply: prototype::RandrOutputPrimaryReplyFn,
    pub select_input: prototype::RandrSelectInput,
}

impl core::fmt::Debug for RandrLib {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        f.write_str("RandrLib(libxcb-randr)")
    }
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
}

/// The sonames tried, in order.
///
/// The versioned one first, for the reason
/// [`wayland::ffi::SONAMES`](crate::wayland::ffi) gives: the unversioned
/// symlink is shipped only by the `-dev` package.
const SONAMES: [&CStr; 2] = [c"libxcb.so.1", c"libxcb.so"];
/// As [`SONAMES`], for libxcb-randr.
const RANDR_SONAMES: [&CStr; 2] = [c"libxcb-randr.so.0", c"libxcb-randr.so"];
/// As [`SONAMES`], for libxcb-xinput.
const XI_SONAMES: [&CStr; 2] = [c"libxcb-xinput.so.0", c"libxcb-xinput.so"];
/// As [`SONAMES`], for libxcb-xkb.
const XKB_SONAMES: [&CStr; 2] = [c"libxcb-xkb.so.1", c"libxcb-xkb.so"];

static LIB: OnceLock<Result<Lib, String>> = OnceLock::new();
static RANDR: OnceLock<Option<RandrLib>> = OnceLock::new();
static XI: OnceLock<Option<XiLib>> = OnceLock::new();
static XKB: OnceLock<Option<XkbLib>> = OnceLock::new();

/// The `xcb_extension_t*` an [`Ext`] entry holds, as libxcb wants it.
///
/// The descriptor is a mutable global inside the extension library that libxcb
/// initialises lazily under its own lock; this process never writes to it and
/// only ever hands it straight back to `xcb_get_extension_data`. It is stored
/// as an address so the surrounding struct needs no `unsafe impl Sync` — a raw
/// pointer in a `static` would otherwise require asserting thread-safety for a
/// whole struct in order to say it about one field.
#[inline]
#[must_use]
pub const fn extension_ptr(id: usize) -> *mut Extension {
    id as *mut Extension
}

/// Loads libxcb, once per process.
///
/// # Errors
///
/// The `dlerror` text if no soname could be opened, or the name of the first
/// symbol that could not be resolved — a string, because it is for a human
/// reading a log line rather than for a caller to branch on.
pub fn load() -> Result<&'static Lib, &'static str> {
    match LIB.get_or_init(load_uncached) {
        Ok(lib) => Ok(lib),
        Err(message) => Err(message.as_str()),
    }
}

/// Loads the optional extension libraries, once per process.
///
/// Never fails: a missing library is a cleared capability bit.
#[must_use]
pub fn load_ext() -> Ext {
    Ext {
        randr: RANDR.get_or_init(load_randr).as_ref(),
        xi: XI.get_or_init(load_xi).as_ref(),
        xkb: XKB.get_or_init(load_xkb).as_ref(),
    }
}

/// Opens the first soname that will open.
fn open_library(sonames: &[&CStr]) -> *mut c_void {
    for soname in sonames {
        // SAFETY: `soname` is a NUL-terminated literal, and `RTLD_NOW` is a
        // valid flag. `dlopen` either returns a handle or null; it does not
        // take ownership of the string.
        let handle = unsafe { dlopen(soname.as_ptr(), RTLD_NOW) };
        if !handle.is_null() {
            return handle;
        }
    }
    ptr::null_mut()
}

/// The loader's last error, copied out of its static buffer.
fn last_dl_error() -> String {
    // SAFETY: `dlerror` returns a pointer to a static buffer owned by the
    // loader, valid until the next call on this thread. It is copied out
    // immediately.
    unsafe {
        let message = dlerror();
        if message.is_null() {
            "not found".to_string()
        } else {
            CStr::from_ptr(message).to_string_lossy().into_owned()
        }
    }
}

fn load_uncached() -> Result<Lib, String> {
    let handle = open_library(&SONAMES);
    if handle.is_null() {
        return Err(format!("cannot load libxcb.so.1: {}", last_dl_error()));
    }

    /// Resolves one symbol, or reports which one was missing.
    ///
    /// The `transmute` is the unavoidable core of `dlopen`-based loading:
    /// `dlsym` returns `void*` and the caller asserts the signature. The
    /// asserted type is named at every use site and defined, with its C
    /// prototype, in [`prototype`].
    macro_rules! symbol {
        ($name:literal, $ty:ty) => {{
            // SAFETY: `handle` is a live `dlopen` handle and the name is a
            // NUL-terminated literal.
            let raw = unsafe { dlsym(handle, concat!($name, "\0").as_ptr().cast()) };
            if raw.is_null() {
                return Err(format!("libxcb.so.1 has no symbol {}", $name));
            }
            // SAFETY: `$ty` is the prototype `xcb/xproto.h` declares for this
            // symbol, and libxcb's ABI is stable across 1.x.
            unsafe { core::mem::transmute::<*mut c_void, $ty>(raw) }
        }};
    }

    Ok(Lib {
        connect: symbol!("xcb_connect", prototype::Connect),
        disconnect: symbol!("xcb_disconnect", prototype::Disconnect),
        connection_has_error: symbol!("xcb_connection_has_error", prototype::ConnInt),
        flush: symbol!("xcb_flush", prototype::ConnInt),
        get_file_descriptor: symbol!("xcb_get_file_descriptor", prototype::ConnInt),
        generate_id: symbol!("xcb_generate_id", prototype::GenerateId),
        get_setup: symbol!("xcb_get_setup", prototype::GetSetup),
        setup_roots_iterator: symbol!("xcb_setup_roots_iterator", prototype::SetupRootsIterator),
        screen_next: symbol!("xcb_screen_next", prototype::ScreenNext),
        poll_for_event: symbol!("xcb_poll_for_event", prototype::PollForEvent),
        get_extension_data: symbol!("xcb_get_extension_data", prototype::GetExtensionData),
        create_window: symbol!("xcb_create_window", prototype::CreateWindow),
        destroy_window: symbol!("xcb_destroy_window", prototype::WindowRequest),
        map_window: symbol!("xcb_map_window", prototype::WindowRequest),
        unmap_window: symbol!("xcb_unmap_window", prototype::WindowRequest),
        change_window_attributes: symbol!(
            "xcb_change_window_attributes",
            prototype::ChangeWindowAttributes
        ),
        configure_window: symbol!("xcb_configure_window", prototype::ConfigureWindow),
        change_property: symbol!("xcb_change_property", prototype::ChangeProperty),
        delete_property: symbol!("xcb_delete_property", prototype::DeleteProperty),
        get_property: symbol!("xcb_get_property", prototype::GetProperty),
        get_property_reply: symbol!("xcb_get_property_reply", prototype::GetPropertyReplyFn),
        get_property_value: symbol!("xcb_get_property_value", prototype::GetPropertyValue),
        get_property_value_length: symbol!(
            "xcb_get_property_value_length",
            prototype::GetPropertyValueLength
        ),
        #[cfg(feature = "x11-e2e")]
        query_tree: symbol!("xcb_query_tree", prototype::QueryTree),
        #[cfg(feature = "x11-e2e")]
        query_tree_reply: symbol!("xcb_query_tree_reply", prototype::QueryTreeReplyFn),
        #[cfg(feature = "x11-e2e")]
        query_tree_children: symbol!("xcb_query_tree_children", prototype::QueryTreeChildren),
        #[cfg(feature = "x11-e2e")]
        query_tree_children_length: symbol!(
            "xcb_query_tree_children_length",
            prototype::QueryTreeChildrenLength
        ),
        intern_atom: symbol!("xcb_intern_atom", prototype::InternAtom),
        intern_atom_reply: symbol!("xcb_intern_atom_reply", prototype::InternAtomReplyFn),
        get_geometry: symbol!("xcb_get_geometry", prototype::DrawableRequest),
        get_geometry_reply: symbol!("xcb_get_geometry_reply", prototype::GetGeometryReplyFn),
        translate_coordinates: symbol!(
            "xcb_translate_coordinates",
            prototype::TranslateCoordinates
        ),
        translate_coordinates_reply: symbol!(
            "xcb_translate_coordinates_reply",
            prototype::TranslateCoordinatesReplyFn
        ),
        send_event: symbol!("xcb_send_event", prototype::SendEvent),
        set_selection_owner: symbol!("xcb_set_selection_owner", prototype::SetSelectionOwner),
        get_selection_owner: symbol!("xcb_get_selection_owner", prototype::DrawableRequest),
        get_selection_owner_reply: symbol!(
            "xcb_get_selection_owner_reply",
            prototype::GetSelectionOwnerReplyFn
        ),
        convert_selection: symbol!("xcb_convert_selection", prototype::ConvertSelection),
        grab_pointer: symbol!("xcb_grab_pointer", prototype::GrabPointer),
        grab_pointer_reply: symbol!("xcb_grab_pointer_reply", prototype::GrabPointerReplyFn),
        ungrab_pointer: symbol!("xcb_ungrab_pointer", prototype::UngrabPointer),
        warp_pointer: symbol!("xcb_warp_pointer", prototype::WarpPointer),
        query_pointer: symbol!("xcb_query_pointer", prototype::DrawableRequest),
        query_pointer_reply: symbol!("xcb_query_pointer_reply", prototype::QueryPointerReplyFn),
        create_pixmap: symbol!("xcb_create_pixmap", prototype::CreatePixmap),
        create_cursor: symbol!("xcb_create_cursor", prototype::CreateCursor),
        free_pixmap: symbol!("xcb_free_pixmap", prototype::FreeResource),
        free_cursor: symbol!("xcb_free_cursor", prototype::FreeResource),
    })
}

fn load_randr() -> Option<RandrLib> {
    let handle = open_library(&RANDR_SONAMES);
    if handle.is_null() {
        crcbl_core::log::warn!(
            "libxcb-randr is not available: one monitor covering the whole screen, \
             and no refresh rate"
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
                crcbl_core::log::warn!("libxcb-randr has no symbol {}; degrading", $name);
                return None;
            }
            // SAFETY: `$ty` is the prototype `xcb/randr.h` declares for this
            // symbol.
            unsafe { core::mem::transmute::<*mut c_void, $ty>(raw) }
        }};
    }
    // SAFETY: `xcb_randr_id` is a data symbol — an `xcb_extension_t` global —
    // so the `dlsym` result is its address and is not transmuted to a function.
    let id = unsafe { dlsym(handle, c"xcb_randr_id".as_ptr()) } as usize;
    if id == 0 {
        crcbl_core::log::warn!("libxcb-randr has no xcb_randr_id; degrading");
        return None;
    }
    Some(RandrLib {
        id,
        query_version: symbol!("xcb_randr_query_version", prototype::RandrQueryVersion),
        query_version_reply: symbol!(
            "xcb_randr_query_version_reply",
            prototype::RandrQueryVersionReplyFn
        ),
        get_screen_resources_current: symbol!(
            "xcb_randr_get_screen_resources_current",
            prototype::RandrWindowRequest
        ),
        get_screen_resources_current_reply: symbol!(
            "xcb_randr_get_screen_resources_current_reply",
            prototype::RandrScreenResourcesReplyFn
        ),
        resources_crtcs: symbol!(
            "xcb_randr_get_screen_resources_current_crtcs",
            prototype::RandrResourcesCrtcs
        ),
        resources_modes: symbol!(
            "xcb_randr_get_screen_resources_current_modes",
            prototype::RandrResourcesModes
        ),
        get_crtc_info: symbol!("xcb_randr_get_crtc_info", prototype::RandrInfoRequest),
        get_crtc_info_reply: symbol!(
            "xcb_randr_get_crtc_info_reply",
            prototype::RandrCrtcInfoReplyFn
        ),
        crtc_outputs: symbol!(
            "xcb_randr_get_crtc_info_outputs",
            prototype::RandrCrtcOutputs
        ),
        get_output_info: symbol!("xcb_randr_get_output_info", prototype::RandrInfoRequest),
        get_output_info_reply: symbol!(
            "xcb_randr_get_output_info_reply",
            prototype::RandrOutputInfoReplyFn
        ),
        output_name: symbol!("xcb_randr_get_output_info_name", prototype::RandrOutputName),
        get_output_primary: symbol!(
            "xcb_randr_get_output_primary",
            prototype::RandrWindowRequest
        ),
        get_output_primary_reply: symbol!(
            "xcb_randr_get_output_primary_reply",
            prototype::RandrOutputPrimaryReplyFn
        ),
        select_input: symbol!("xcb_randr_select_input", prototype::RandrSelectInput),
    })
}

fn load_xi() -> Option<XiLib> {
    let handle = open_library(&XI_SONAMES);
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
    let handle = open_library(&XKB_SONAMES);
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

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

/// Releases a reply or event libxcb allocated with `malloc`.
///
/// Every `xcb_*_reply` and `xcb_poll_for_event` hands over ownership of a
/// `malloc`'d block and expects `free`. A crate-level wrapper rather than a
/// bare `free` at forty call sites, so the pairing is one grep away.
///
/// # Safety
///
/// `ptr` must be null, or a block libxcb returned and that has not been freed.
#[inline]
pub unsafe fn free_reply<T>(ptr: *mut T) {
    if !ptr.is_null() {
        // SAFETY: the caller guarantees `ptr` came from libxcb's `malloc` and
        // is freed exactly once.
        unsafe { free(ptr.cast::<c_void>()) };
    }
}

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
    fn the_wire_structures_match_the_c_layout() {
        // Getting these wrong reads past the end of a reply libxcb allocated,
        // or — worse — reads a field from the wrong offset and produces a
        // plausible number. The sizes are the ones `xcb/xproto.h` generates.
        assert_eq!(
            size_of::<GenericEvent>(),
            36,
            "32 wire bytes + full_sequence"
        );
        assert_eq!(size_of::<GenericError>(), 36);
        assert_eq!(size_of::<Setup>(), 40);
        assert_eq!(size_of::<Screen>(), 40);
        assert_eq!(size_of::<GetPropertyReply>(), 32);
        assert_eq!(size_of::<GetGeometryReply>(), 24);
        assert_eq!(size_of::<QueryPointerReply>(), 28);
        assert_eq!(size_of::<TranslateCoordinatesReply>(), 16);
        assert_eq!(size_of::<GetSelectionOwnerReply>(), 12);
        assert_eq!(size_of::<XkbUseExtensionReply>(), 32);
        assert_eq!(size_of::<XkbPerClientFlagsReply>(), 32);
        assert_eq!(size_of::<InternAtomReply>(), 12);
        assert_eq!(size_of::<GrabPointerReply>(), 8);
        assert_eq!(size_of::<QueryExtensionReply>(), 12);
        assert_eq!(size_of::<RandrModeInfo>(), 32);
        assert_eq!(size_of::<RandrCrtcInfoReply>(), 32);
        assert_eq!(size_of::<RandrOutputInfoReply>(), 36);
        assert_eq!(
            size_of::<XiRawEvent>(),
            36,
            "a GeGeneric's fixed part, with full_sequence inserted at 32"
        );
        // Every core event body is exactly 32 wire bytes, which is also what
        // `SendEvent` requires of the ones this backend constructs.
        assert_eq!(size_of::<InputEvent>(), 32);
        assert_eq!(size_of::<ConfigureNotifyEvent>(), 28);
        assert_eq!(size_of::<PropertyNotifyEvent>(), 20);
        assert_eq!(size_of::<SelectionRequestEvent>(), 28);
        assert_eq!(
            size_of::<SelectionNotifyEvent>(),
            32,
            "24 on the wire, padded because SendEvent reads 32"
        );
        assert_eq!(size_of::<ClientMessageEvent>(), 32, "SendEvent wants 32");
        assert_eq!(size_of::<FocusEvent>(), 12);
        assert_eq!(size_of::<WindowNotifyEvent>(), 16);
        assert_eq!(
            size_of::<SyntheticWindowNotify>(),
            32,
            "16 on the wire, padded because SendEvent reads 32"
        );
        assert_eq!(size_of::<SelectionClearEvent>(), 16);
        assert_eq!(size_of::<MappingNotifyEvent>(), 8);
        assert_eq!(size_of::<Cookie>(), size_of::<c_uint>());
        // A cookie is passed *by value* and must be exactly one word, or every
        // request in the backend is called with the wrong ABI.
        assert_eq!(align_of::<Cookie>(), align_of::<c_uint>());
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

    #[test]
    fn freeing_a_null_reply_is_a_no_op() {
        // SAFETY: null is the documented degenerate case.
        unsafe { free_reply::<GenericEvent>(ptr::null_mut()) };
    }
}
