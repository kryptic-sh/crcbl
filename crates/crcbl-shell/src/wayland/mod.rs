//! The Wayland backend: connection, registry, `xdg-shell` window lifecycle,
//! `wl_seat` input, pointer constraints, fractional scale and decorations.
//!
//! Not `pub`: [`backend`](crate::backend) is the only way to reach a real
//! shell, because `WaylandShell::new()` in a consumer's source is the platform
//! leak this whole seam exists to prevent. `#[cfg(target_os = "linux")]` rather
//! than `#[cfg(unix)]` — macOS is a Unix with no Wayland, and the BSDs are not
//! a target this engine claims.
//!
//! `docs/plan/15-windowing.md`'s Linux policy in one sentence:
//! **libwayland-client owns the connection and the proxy objects; the protocol
//! layer above `wl_proxy_marshal_array_flags` is ours.** [`ffi`] is the first
//! half — hand-written `extern "C"` declarations, reached by `dlopen` for the
//! reasons that module states. [`protocol`] is the second — generated at build
//! time by `crcbl-wl-scanner` from the vendored XML. [`xkb`] applies the same
//! test to libxkbcommon and reaches the same answer, for reasons written out
//! there.
//!
//! # What is in this backend
//!
//! P0.5a landed the window lifecycle: connect, bind `wl_compositor` /
//! `xdg_wm_base` / `wl_output`, create `wl_surface` + `xdg_surface` +
//! `xdg_toplevel`, the configure/ack handshake, title, size constraints,
//! windowed ↔ borderless, the close request, and monitor enumeration.
//!
//! P0.5b added everything downstream of `wl_seat`:
//!
//! | Protocol | What it gives the engine |
//! | --- | --- |
//! | `wl_seat` / `wl_pointer` / `wl_keyboard` | [`ShellEvent::Key`], [`PointerMotion`](ShellEvent::PointerMotion), [`Button`](ShellEvent::Button), [`Wheel`](ShellEvent::Wheel), [`PointerFocus`](ShellEvent::PointerFocus), [`Focus`](ShellEvent::Focus), [`TextCommit`](ShellEvent::TextCommit) |
//! | `pointer-constraints-v1` | [`PointerMode::Locked`] and [`PointerMode::Confined`] |
//! | `relative-pointer-v1` | `raw_delta` — unaccelerated aim input |
//! | `fractional-scale-v1` + `wp_viewporter` | non-integer [`WindowConfiguration::scale_factor`] |
//! | `xdg-output-v1` | [`MonitorInfo::bounds`] that means something above scale 1 |
//! | `xdg-decoration-v1` | server-side title bars where the compositor does them |
//!
//! P0.5c added `wl_data_device_manager` and the three interfaces under it:
//!
//! | Protocol | What it gives the engine |
//! | --- | --- |
//! | `wl_data_device` + `wl_data_offer` | [`Shell::clipboard_request`] → [`ClipboardData`](ShellEvent::ClipboardData), and drops → [`DroppedFile`](ShellEvent::DroppedFile) |
//! | `wl_data_source` | [`Shell::clipboard_offer`] — we own the selection and produce the bytes on demand |
//!
//! Still deliberately absent: touch, tablet, IME pre-edit, the *primary*
//! selection (`wp_primary_selection_v1` — middle-click paste, which the seam
//! has no vocabulary for) and starting a drag from one of our own windows. That
//! last one is a real gap and is stated as one: the destination half is here,
//! the origin half needs a seam request and a drag icon, and an icon is a
//! surface with a buffer, which is the renderer's. [`ShellCaps`] reflects all of
//! this exactly — see `WaylandShell::caps`.
//!
//! # What a real compositor does that `HeadlessShell` does not model
//!
//! Findings from the nested-sway suite, in the order they cost time:
//!
//! 1. **The first configure usually carries no size.** `xdg-shell` says the
//!    compositor answers the initial commit with an `xdg_surface.configure`,
//!    and that a `0 × 0` in the accompanying `xdg_toplevel.configure` means
//!    "you choose". [`HeadlessShell`](crate::HeadlessShell) always dictates a
//!    size. Both shapes satisfy the seam's contract, but a backend has to
//!    supply the fallback, and this one falls back to
//!    [`WindowDesc::size`](crate::WindowDesc::size) scaled by the current
//!    factor.
//! 2. **Size arrives before scale.** A window's integer scale comes from
//!    `wl_surface.enter`, which a compositor only sends once the surface is
//!    *mapped* — and mapping requires a buffer. So the first configure is
//!    necessarily at scale 1.0 and the true scale arrives later as a
//!    [`ShellEvent::ScaleFactorChanged`]. `fractional-scale-v1` improves on
//!    this but does not fix it: `wp_fractional_scale_v1.preferred_scale` is
//!    also only sent for a mapped surface.
//! 3. **Configures arrive in two messages and take effect on the third.**
//!    `xdg_toplevel.configure` carries the size and the states,
//!    `xdg_surface.configure` carries the serial and means "that is the whole
//!    update", and `ack_configure` + `commit` is the reply. This backend
//!    accumulates and only publishes a [`WindowConfiguration`] on the
//!    `xdg_surface.configure`, which is what keeps a consumer from ever seeing
//!    a size without its states.
//! 4. **Configured is not the same as managed.** A surface is mapped exactly
//!    while it has a *buffer*, and an unmapped `xdg_toplevel` gets its one
//!    initial configure and nothing else ever again: no compositor-chosen
//!    geometry, no answer to `set_fullscreen`, no entry in the window
//!    manager's tree, **and no `wl_pointer.enter` or `wl_keyboard.enter`** —
//!    an unmapped surface cannot receive input, because there is nothing on
//!    screen to point at. Attaching a buffer is the renderer's job, so this
//!    backend cannot map a window on its own, by design; see [`e2e`].
//! 5. **A seat can have no devices at all.** `wl_seat.capabilities` is `0` on
//!    a headless compositor and changes at runtime when a device is plugged
//!    in. There is no "the keyboard" — there is whatever the seat currently
//!    has, and it can go away mid-session.
//! 6. **The clipboard is focus-gated and arrives late.**
//!    `wl_data_device.selection` is delivered *only* to the client that has
//!    keyboard focus on that seat, and again whenever focus arrives — so a
//!    client's knowledge of the clipboard is acquired on focus and goes stale
//!    when focus leaves, and a background window cannot read the clipboard at
//!    all. This is what [`Shell::clipboard_readable`] reports and why
//!    [`clipboard_request`](Shell::clipboard_request) *holds* a read it cannot
//!    yet answer: answering "empty" would be indistinguishable from an empty
//!    clipboard, and the end-to-end suite proved the cost of that by needing a
//!    retry loop no editor would contain.
//!
//!    A corollary that only showed up once "held, not empty" was enforced:
//!    **claiming the selection invalidates what we know until the compositor
//!    echoes it back.** Between `set_selection` and the `selection` event it
//!    provokes, this client's own offer describes the clipboard it *replaced*
//!    — so pasting one's own copy answered `Empty` until
//!    [`clipboard_offer`](Shell::clipboard_offer) started clearing
//!    `selection_seen` too.
//! 7. **Claiming the selection needs an input serial.** A client that has
//!    received no input on a seat cannot take the clipboard, because
//!    `set_selection` quotes the serial of the event that caused it and a
//!    compositor checks it. That is a *feature* — it is what stops a background
//!    process stealing the clipboard — and it means
//!    [`clipboard_offer`](Shell::clipboard_offer) can legitimately fail on a
//!    window nobody has touched.
//!
//! # Decision: the shell synthesizes key repeats
//!
//! `wl_keyboard.repeat_info` hands the client a rate and a delay and **no
//! repeat events**: on Wayland, generating them is the client's job by
//! protocol design. The choice is therefore where in the client they are
//! generated, and this backend does it, marking every one
//! [`repeat: true`](ShellEvent::Key).
//!
//! The case for pushing it up to `docs/plan/19-input.md`'s action layer is
//! real — a shell that fabricates edges can confuse hold-pattern detection —
//! and it loses on three counts:
//!
//! * **The seam already decided.** [`ShellEvent::Key`] carries `repeat`, and
//!   its documentation is explicit that filtering repeats at the source breaks
//!   text fields while dropping the flag breaks jump buttons, "so the fact is
//!   carried and the consumer decides".
//!   [`HeadlessShell::key_repeat`](crate::HeadlessShell::key_repeat) already
//!   produces them. A Wayland backend that never did would make the flag dead
//!   on the only real platform and `HeadlessShell` a model of nothing.
//! * **Hold patterns are protected by construction, not by luck.** A repeat is
//!   `repeat: true` and never `Released`, so `hold(400ms)` — which measures
//!   press edge to release edge — cannot see one. A pattern evaluator that
//!   keys on `repeat == false` is correct with no coordination.
//! * **Nobody else can do it better.** The timer has to live next to the
//!   socket. Repeats here are scheduled on `CLOCK_MONOTONIC` and stamped with
//!   the instant they were *due*, not the instant the queue was drained, so a
//!   16 ms frame does not quantize them — the exact property
//!   [`event`](crate::event) says timestamps exist for.
//!   [`wait_events`](Shell::wait_events) shortens its timeout to the next
//!   repeat, so an editor idling at zero frames per second still repeats.
//!
//! Guards, because a fabricated event stream needs them: no repeat is
//! generated for a key XKB says does not repeat (modifiers, Caps Lock); a
//! repeat stops on release, on focus loss, on the pointer's seat losing its
//! keyboard, and on a keymap change; and a run that falls more than four
//! intervals behind resynchronises instead of emitting the backlog.

mod data;
mod fd;
pub mod ffi;
pub mod protocol;

/// The evdev table and libxkbcommon, which are Linux facts rather than Wayland
/// ones and are therefore shared with the X11 backend.
///
/// Re-exported under this module's own namespace so that everything below
/// spells them `keymap::` and `xkb::` the way it always did; see
/// [`crate::linux`] for why they live one level up.
pub(crate) use crate::linux::{keymap, xkb};

/// Test-only: maps a window's surface with a stand-in buffer, and drives real
/// input through virtual devices.
///
/// Behind the `wayland-e2e` feature because it is scaffolding, not shell — see
/// the module's own docs for why the end-to-end suite cannot do without it and
/// why the shipping backend must not contain it.
#[cfg(feature = "wayland-e2e")]
pub mod e2e;

use core::ffi::{c_int, c_void};
use core::ptr::{self, NonNull};
use core::time::Duration;
use std::collections::VecDeque;
use std::ffi::CString;
use std::os::fd::AsRawFd;

use crcbl_core::input::{ButtonState, DeviceId, Keysym, Modifiers, Scancode, ScrollDelta};
use crcbl_core::{EventTime, KeyCode, Pool, SurfaceTarget};

use crate::{
    ClipboardContent, ClipboardOffer, ClipboardRequestId, CloseReply, CursorIcon, DisplayMode,
    LogicalSize, MimeType, MonitorId, MonitorInfo, PhysicalPoint, PhysicalRect, PointerMode,
    ReceivedMime, Shell, ShellBackend, ShellCaps, ShellError, ShellEvent, SizeConstraints,
    WindowConfiguration, WindowDesc, WindowId, WindowState,
};

use data::{Delivery, HeldRead, Resolution, Transfer};
use ffi::{Lib, WlArgument, WlDisplay, WlMessage, WlProxy};
use protocol::fractional_scale::{wp_fractional_scale_manager_v1, wp_fractional_scale_v1};
use protocol::pointer_constraints::{zwp_locked_pointer_v1, zwp_pointer_constraints_v1};
use protocol::relative_pointer::{zwp_relative_pointer_manager_v1, zwp_relative_pointer_v1};
use protocol::viewporter::{wp_viewport, wp_viewporter};
use protocol::wayland::{
    wl_compositor, wl_data_device, wl_data_device_manager, wl_data_offer, wl_data_source,
    wl_keyboard, wl_output, wl_pointer, wl_registry, wl_seat, wl_surface,
};
use protocol::xdg_decoration::{zxdg_decoration_manager_v1, zxdg_toplevel_decoration_v1};
use protocol::xdg_output::{zxdg_output_manager_v1, zxdg_output_v1};
use protocol::xdg_shell::{xdg_surface, xdg_toplevel, xdg_wm_base};

/// Versions this backend binds globals at.
///
/// Capped at what the code actually implements, never at what the compositor
/// offers: binding a higher version means promising to handle events this build
/// has never seen. `wl_output` 4 is the lowest version that reports
/// [`wl_output.name`](protocol::wayland::wl_output), which is the only stable
/// identifier a monitor has.
const COMPOSITOR_VERSION: u32 = 4;
/// See [`COMPOSITOR_VERSION`]. `xdg_wm_base` 1 is all the window lifecycle
/// needs; 4 and 5 add `configure_bounds` and `wm_capabilities`, which this
/// backend does not act on.
const WM_BASE_VERSION: u32 = 1;
/// See [`COMPOSITOR_VERSION`].
const OUTPUT_VERSION: u32 = 4;
/// `wl_seat` 8 is the lowest version whose `wl_pointer` sends
/// [`axis_value120`](protocol::wayland::wl_pointer) — the high-resolution wheel
/// event a smooth-scrolling UI needs, and the reason not to stop at 5.
///
/// 9 adds `axis_relative_direction` (which physical direction produced an
/// already-inverted value — informational) and 10 adds
/// `key_state.repeated`, a compositor-generated repeat. Neither is acted on
/// here, and binding 10 in particular would give this backend two repeat
/// sources with no way to reconcile them — see the [module docs](self).
const SEAT_VERSION: u32 = 8;
/// `zxdg_output_manager_v1` 2 deprecates `name`/`description` in favour of
/// `wl_output` 4's, and 3 stops sending `zxdg_output_v1.done` in favour of
/// `wl_output.done`. This backend settles outputs on `wl_output.done` either
/// way, so 3 is free.
const XDG_OUTPUT_VERSION: u32 = 3;
/// `zxdg_decoration_manager_v1` 1 is the whole protocol; 2 only adds an error
/// code for a case this backend cannot reach (it never orphans a decoration).
const DECORATION_VERSION: u32 = 1;
/// `wl_data_device_manager` 3 is the version that has drag-and-drop actions:
/// `wl_data_offer.set_actions`/`finish` and `wl_data_source.set_actions`. A
/// version-1 or -2 device works for the clipboard and negotiates no action at
/// all, which is why every `set_actions` below is version-guarded.
///
/// 4 adds only `wl_data_device_manager.release`, a destructor for the manager
/// itself. This backend destroys the manager proxy client-side at shutdown, on
/// a connection that is being closed in the same breath, so there is nothing to
/// tell the compositor and nothing to gain — see [`COMPOSITOR_VERSION`] for the
/// rule about binding versions the code does not implement.
const DATA_DEVICE_VERSION: u32 = 3;
/// `wp_fractional_scale_v1` reports scale as a fraction of this.
const FRACTIONAL_SCALE_DENOMINATOR: f64 = 120.0;
/// The longest [`wait_events`](Shell::wait_events) may sleep while a clipboard
/// transfer is in flight.
///
/// The descriptors are in the poll, so a peer that writes wakes the wait
/// immediately; this bounds only the case where nothing happens at all, so that
/// [`fd::TIMEOUT`] is reached instead of slept through.
const TRANSFER_POLL: Duration = Duration::from_millis(50);

// ---------------------------------------------------------------------------
// Timestamps
// ---------------------------------------------------------------------------

/// Converts Wayland's `CLOCK_MONOTONIC` milliseconds onto the engine epoch.
///
/// [`EventTime`] requires "a duration measured from the same origin as the
/// [`TimeSource`](crcbl_core::time::TimeSource) driving
/// [`FrameClock::update`](crcbl_core::FrameClock::update)". Wayland gives a
/// **32-bit** millisecond counter sampled from `CLOCK_MONOTONIC`, which is a
/// different origin *and* wraps every 49.7 days, so a backend that forwards it
/// produces an input pipeline that silently breaks after a month and a half of
/// uptime.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct TimeBase {
    /// `CLOCK_MONOTONIC` nanoseconds at the engine epoch.
    epoch_nanos: u64,
}

impl TimeBase {
    /// An epoch of "now", which is the shell's creation.
    fn now() -> Self {
        Self {
            epoch_nanos: ffi::monotonic_nanos(),
        }
    }

    /// Moves the epoch so that this instant reads as `elapsed`.
    ///
    /// See [`WaylandShell::align_event_clock`].
    fn align(&mut self, elapsed: Duration) {
        let now = ffi::monotonic_nanos();
        self.epoch_nanos =
            now.saturating_sub(u64::try_from(elapsed.as_nanos()).unwrap_or(u64::MAX));
    }

    /// Widens a 32-bit Wayland timestamp and rebases it.
    ///
    /// Pure so it can be tested without a compositor, which matters because the
    /// wrap is unobservable in any session shorter than seven weeks.
    ///
    /// `wayland_millis` is a raw `u32` off the wire, carried by every pointer
    /// and keyboard event and validated by nobody, so the widening is written as
    /// "pick the nearest candidate that exists" rather than as an adjustment.
    /// The candidate one wrap *below* the current one only exists on a machine
    /// that has been up for 49.7 days; before that `now_millis` has no high bits
    /// and subtracting a wrap underflows — which is what the earlier
    /// `full -= WRAP` did, panicking in debug and producing a timestamp around
    /// 1.8 × 10¹⁹ ms in release for any event whose stamp merely ran ahead of
    /// our sample.
    fn rebase(epoch_nanos: u64, now_nanos: u64, wayland_millis: u32) -> EventTime {
        const WRAP: u64 = 1 << 32;
        let now_millis = now_nanos / 1_000_000;
        // An event is always within a few milliseconds of the present, so of
        // the reconstructions that are representable at all, the one nearest
        // `now` is the right one.
        let current = (now_millis & !(WRAP - 1)) | u64::from(wayland_millis);
        let full = [
            current.checked_sub(WRAP),
            Some(current),
            current.checked_add(WRAP),
        ]
        .into_iter()
        .flatten()
        .min_by_key(|candidate| candidate.abs_diff(now_millis))
        .expect("the current wrap is always a candidate");
        let epoch_millis = epoch_nanos / 1_000_000;
        EventTime::from_millis(full.saturating_sub(epoch_millis))
    }

    /// This base applied to a compositor millisecond timestamp.
    ///
    /// What every `wl_pointer` and `wl_keyboard` event goes through.
    fn event_time(self, wayland_millis: u32) -> EventTime {
        Self::rebase(self.epoch_nanos, ffi::monotonic_nanos(), wayland_millis)
    }

    /// This base applied to a 64-bit `CLOCK_MONOTONIC` microsecond timestamp.
    ///
    /// `zwp_relative_pointer_v1.relative_motion` carries one, split across two
    /// `uint`s. It needs no wrap handling at all — 64 bits of microseconds is
    /// half a million years — which is why relative motion is the more accurate
    /// of the two clocks a pointer event can arrive on, and why a merged
    /// motion event prefers it.
    fn event_time_micros(self, micros: u64) -> EventTime {
        EventTime::from_micros(micros.saturating_sub(self.epoch_nanos / 1_000))
    }

    /// The current instant, on the engine epoch.
    ///
    /// For the events a compositor sends with **no timestamp at all**:
    /// `wl_pointer.enter` and `wl_pointer.leave` carry a serial and a position
    /// and no time, and neither does a focus change synthesized because a seat
    /// lost its pointer. [`EventTime`] states the rule for exactly this case —
    /// "it stamps the event with the *current* time rather than
    /// [`EventTime::ZERO`], because a zero timestamp reads as 'this happened at
    /// process start' to every consumer downstream".
    fn event_time_now(self) -> EventTime {
        self.event_time_nanos(ffi::monotonic_nanos())
    }

    /// This base applied to a raw `CLOCK_MONOTONIC` nanosecond reading.
    ///
    /// Used for synthesized key repeats, which are stamped with the instant
    /// they were *due* rather than the instant the queue happened to be
    /// drained.
    fn event_time_nanos(self, nanos: u64) -> EventTime {
        EventTime::from_duration(Duration::from_nanos(nanos.saturating_sub(self.epoch_nanos)))
    }
}

// ---------------------------------------------------------------------------
// The dispatcher's side of the world
// ---------------------------------------------------------------------------

/// Which interface a proxy we attached a dispatcher to belongs to.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum ObjectKind {
    Registry,
    WmBase,
    Output,
    XdgOutput,
    Surface,
    XdgSurface,
    XdgToplevel,
    Decoration,
    FractionalScale,
    Seat,
    Pointer,
    Keyboard,
    RelativePointer,
    DataDevice,
    DataOffer,
    DataSource,
    /// A proxy whose events we deliberately do not read.
    ///
    /// Attaching a do-nothing dispatcher rather than leaving the proxy bare:
    /// libwayland logs a warning for every event delivered to a listener-less
    /// proxy, which buries the compositor log the e2e harness prints on failure.
    Ignored,
}

/// One decoded event, with every borrow already copied out.
///
/// The dispatcher runs *inside* libwayland, where strings and arrays point into
/// closure storage that dies when it returns. Copying here rather than
/// borrowing is what lets [`Sink`] be a plain queue that the pump drains
/// afterwards, with no reentrancy and no lifetimes crossing the FFI boundary.
#[derive(Clone, Debug)]
enum RawEvent {
    Global {
        name: u32,
        interface: String,
        version: u32,
    },
    GlobalRemove {
        name: u32,
    },
    Ping {
        serial: u32,
    },
    OutputGeometry {
        output: usize,
        x: i32,
        y: i32,
    },
    OutputMode {
        output: usize,
        flags: u32,
        width: i32,
        height: i32,
        refresh: i32,
    },
    OutputScale {
        output: usize,
        factor: i32,
    },
    OutputName {
        output: usize,
        name: String,
    },
    OutputDone {
        output: usize,
    },
    XdgOutputPosition {
        xdg_output: usize,
        x: i32,
        y: i32,
    },
    XdgOutputSize {
        xdg_output: usize,
        width: i32,
        height: i32,
    },
    SurfaceEnter {
        surface: usize,
        output: usize,
    },
    SurfaceLeave {
        surface: usize,
        output: usize,
    },
    XdgSurfaceConfigure {
        xdg_surface: usize,
        serial: u32,
    },
    ToplevelConfigure {
        toplevel: usize,
        width: i32,
        height: i32,
        states: Vec<u32>,
    },
    ToplevelClose {
        toplevel: usize,
    },
    DecorationConfigure {
        decoration: usize,
        mode: u32,
    },
    PreferredScale {
        object: usize,
        scale: u32,
    },
    SeatCapabilities {
        seat: usize,
        capabilities: u32,
    },
    PointerEnter {
        pointer: usize,
        serial: u32,
        surface: usize,
        x: i32,
        y: i32,
    },
    PointerLeave {
        pointer: usize,
        serial: u32,
        surface: usize,
    },
    PointerMotion {
        pointer: usize,
        time: u32,
        x: i32,
        y: i32,
    },
    PointerButton {
        pointer: usize,
        serial: u32,
        time: u32,
        button: u32,
        state: u32,
    },
    PointerAxis {
        pointer: usize,
        time: u32,
        axis: u32,
        value: i32,
    },
    PointerAxisSource {
        pointer: usize,
        source: u32,
    },
    PointerAxisDiscrete {
        pointer: usize,
        axis: u32,
        discrete: i32,
    },
    PointerAxisValue120 {
        pointer: usize,
        axis: u32,
        value120: i32,
    },
    PointerFrame {
        pointer: usize,
    },
    RelativeMotion {
        relative: usize,
        utime_hi: u32,
        utime_lo: u32,
        dx_unaccel: i32,
        dy_unaccel: i32,
    },
    KeyboardKeymap {
        keyboard: usize,
        format: u32,
        fd: i32,
        size: u32,
    },
    KeyboardEnter {
        keyboard: usize,
        serial: u32,
        surface: usize,
    },
    KeyboardLeave {
        keyboard: usize,
        serial: u32,
        surface: usize,
    },
    KeyboardKey {
        keyboard: usize,
        serial: u32,
        time: u32,
        key: u32,
        state: u32,
    },
    KeyboardModifiers {
        keyboard: usize,
        serial: u32,
        depressed: u32,
        latched: u32,
        locked: u32,
        group: u32,
    },
    KeyboardRepeatInfo {
        keyboard: usize,
        rate: i32,
        delay: i32,
    },
    /// A `wl_data_offer` has been created. The dispatcher has already attached
    /// itself to it; see [`Sink::watch`].
    DataOffer {
        device: usize,
        offer: usize,
    },
    /// One `wl_data_offer.offer` — a mime type the peer can produce.
    OfferMime {
        offer: usize,
        mime: String,
    },
    /// The clipboard changed. `offer` is zero when it was cleared.
    Selection {
        device: usize,
        offer: usize,
    },
    DragEnter {
        device: usize,
        serial: u32,
        surface: usize,
        x: i32,
        y: i32,
        offer: usize,
    },
    DragLeave {
        device: usize,
    },
    DragMotion {
        device: usize,
        time: u32,
        x: i32,
        y: i32,
    },
    DragDrop {
        device: usize,
    },
    /// A peer wants the bytes behind a format we published.
    ///
    /// **Carries an owned file descriptor.** Every path out of
    /// [`WaylandShell::process_data`] either adopts it or closes it; a leak
    /// here is one descriptor per paste, in a process that may run for days.
    SourceSend {
        source: usize,
        mime: String,
        fd: i32,
    },
    /// Another client took the selection, so our source is dead.
    SourceCancelled {
        source: usize,
    },
}

/// Everything the C dispatcher touches.
///
/// Reached **only** through a raw pointer, never through a Rust reference held
/// by [`WaylandShell`]. That is deliberate: the dispatcher runs synchronously
/// inside `wl_display_dispatch_pending`, so if the shell held a live `&mut` to
/// this allocation at the same time the two would alias. Keeping the raw
/// pointer as the sole root means the shell's `&mut Sink` and the dispatcher's
/// exist at strictly disjoint times.
#[derive(Debug)]
struct Sink {
    /// libwayland, so that [`watch`](Self::watch) can be called from inside the
    /// dispatcher — see that method for the event that forces it.
    lib: &'static Lib,
    /// This allocation's own address, as it came out of `Box::into_raw`.
    ///
    /// The pointer every `wl_proxy_add_dispatcher` is given, and the reason it
    /// is a stored field rather than a `ptr::from_mut(self)` at the call site:
    /// `self` there is a transient reborrow — of `Conn::sink()`'s `&mut *self.sink`,
    /// or of the dispatcher's own `&mut *user_data` — and every *later*
    /// `Conn::sink()` re-derives from the raw root, which invalidates that
    /// reborrow and everything descended from it. libwayland would then be
    /// holding pointers whose provenance is dead, and the dispatcher's
    /// `&mut *user_data` would be undefined behaviour from the second `watch`
    /// onward. Handing over the root itself is what makes each dispatcher entry
    /// a fresh, valid derivation.
    root: *mut Sink,
    /// Proxies we have attached the dispatcher to, and what they are.
    objects: Vec<(usize, ObjectKind)>,
    /// Decoded events awaiting the pump.
    events: Vec<RawEvent>,
}

impl Sink {
    /// Allocates a sink and records its own address.
    ///
    /// Returns the raw pointer rather than the value because [`root`](Self::root)
    /// can only be filled in once the allocation has an address — which is the
    /// whole point of the field.
    fn boxed(lib: &'static Lib) -> *mut Sink {
        let raw = Box::into_raw(Box::new(Self {
            lib,
            root: ptr::null_mut(),
            objects: Vec::new(),
            events: Vec::new(),
        }));
        // SAFETY: `raw` was just returned by `Box::into_raw`, so it is live,
        // aligned and uniquely owned; nothing else refers to the allocation
        // yet.
        unsafe { (*raw).root = raw };
        raw
    }

    fn kind_of(&self, proxy: usize) -> Option<ObjectKind> {
        self.objects
            .iter()
            .find(|(candidate, _)| *candidate == proxy)
            .map(|(_, kind)| *kind)
    }

    fn forget(&mut self, proxy: usize) {
        self.objects.retain(|(candidate, _)| *candidate != proxy);
    }

    /// Starts routing `proxy`'s events, recording what it is.
    ///
    /// # Why this is on the sink rather than only on [`Conn`]
    ///
    /// Almost every object this backend owns is created by a request *it*
    /// sends, so the dispatcher can be attached from ordinary code with the
    /// connection in hand. `wl_data_offer` is the exception: the **compositor**
    /// creates it, with `wl_data_device.data_offer`, and then immediately sends
    /// the `wl_data_offer.offer` events listing its mime types — all inside the
    /// same `wl_display_dispatch_pending`. An offer whose dispatcher were
    /// attached afterwards, from `process_raw`, would have missed every one of
    /// them, and a clipboard offer with no mime types is a clipboard that is
    /// always empty.
    ///
    /// So the dispatcher attaches to the new object from inside itself, which
    /// is ordinary libwayland usage — `wl_proxy_add_dispatcher` is not
    /// reentrant into the client — and the reason the sink holds [`Lib`] at
    /// all.
    fn watch(&mut self, proxy: *mut WlProxy, kind: ObjectKind) {
        if proxy.is_null() {
            return;
        }
        self.objects.push((proxy as usize, kind));
        // SAFETY: `proxy` is live on this connection and has no dispatcher yet;
        // `dispatch` matches libwayland's `wl_dispatcher_func_t`; and the
        // `*mut Sink` handed over is `self.root` — the pointer `Box::into_raw`
        // produced, *not* a reborrow of the `&mut self` this method holds, so it
        // stays valid across every later `Conn::sink()`. The allocation outlives
        // every proxy, because `WaylandShell::drop` destroys them all before
        // freeing it.
        unsafe {
            (self.lib.proxy_add_dispatcher)(proxy, dispatch, self.root.cast(), ptr::null_mut());
        }
    }
}

/// libwayland's generic event dispatcher, for every object this backend owns.
///
/// One function rather than a listener struct per interface: `wl_proxy_add_dispatcher`
/// hands over the opcode and the raw argument array, so the *only* per-interface
/// code is the generated decoder. A listener struct would instead need one
/// `extern "C" fn` per event with a hand-written C signature — dozens of
/// hand-transcribed ABIs, each silently wrong if it drifts from the XML, which
/// is exactly what the generator exists to avoid.
unsafe extern "C" fn dispatch(
    user_data: *const c_void,
    target: *mut c_void,
    opcode: u32,
    _message: *const WlMessage,
    args: *mut WlArgument,
) -> c_int {
    // SAFETY: `user_data` is `Sink::root` — the pointer `Box::into_raw`
    // produced in `Sink::boxed`, which every `watch` hands over verbatim and
    // which stays live until `WaylandShell::drop`. Deriving a `&mut` from the
    // allocation's root here is what keeps it valid across the shell's own
    // `Conn::sink()` reborrows. No Rust reference to the allocation is alive at
    // this point: the shell only takes one outside the
    // `dispatch_pending`/`roundtrip`/`read_events` calls that can reach this
    // function.
    let sink = unsafe { &mut *user_data.cast::<Sink>().cast_mut() };
    let proxy = target as usize;
    let Some(kind) = sink.kind_of(proxy) else {
        // An event for an object we already destroyed. libwayland can deliver
        // one that was in flight when the destructor was sent; dropping it is
        // the documented behaviour.
        return 0;
    };
    // SAFETY: libwayland always hands the dispatcher its closure's argument
    // array, which has one slot per signature character and lives for the
    // duration of this call. Borrowing it here rather than passing the raw
    // pointer is what pins the decoders' `'a` to this stack frame: with a raw
    // pointer the lifetime is inferred, and a `&CStr` field could outlive the
    // closure storage it points into.
    let args: &WlArgument = unsafe { &*args.cast_const() };

    // SAFETY (all decoders): `kind` records which interface this proxy was
    // created as, so `args` is that interface's argument array for `opcode` —
    // which is exactly the dispatcher contract — and every borrow is copied out
    // before this function returns, which is the lifetime the decoders
    // document.
    match kind {
        ObjectKind::Ignored => {}
        ObjectKind::Registry => {
            if let Some(event) = unsafe { wl_registry::decode_event(opcode, args) } {
                sink.events.push(match event {
                    wl_registry::Event::Global {
                        name,
                        interface,
                        version,
                    } => RawEvent::Global {
                        name,
                        interface: interface.to_string_lossy().into_owned(),
                        version,
                    },
                    wl_registry::Event::GlobalRemove { name } => RawEvent::GlobalRemove { name },
                });
            }
        }
        ObjectKind::WmBase => {
            // SAFETY: see the note above the `match`.
            if let Some(xdg_wm_base::Event::Ping { serial }) =
                unsafe { xdg_wm_base::decode_event(opcode, args) }
            {
                sink.events.push(RawEvent::Ping { serial });
            }
        }
        ObjectKind::Output => {
            // SAFETY: see the note above the `match`.
            if let Some(event) = unsafe { wl_output::decode_event(opcode, args) } {
                let raw = match event {
                    wl_output::Event::Geometry { x, y, .. } => Some(RawEvent::OutputGeometry {
                        output: proxy,
                        x,
                        y,
                    }),
                    wl_output::Event::Mode {
                        flags,
                        width,
                        height,
                        refresh,
                    } => Some(RawEvent::OutputMode {
                        output: proxy,
                        flags,
                        width,
                        height,
                        refresh,
                    }),
                    wl_output::Event::Scale { factor } => Some(RawEvent::OutputScale {
                        output: proxy,
                        factor,
                    }),
                    wl_output::Event::Name { name } => Some(RawEvent::OutputName {
                        output: proxy,
                        name: name.to_string_lossy().into_owned(),
                    }),
                    wl_output::Event::Done => Some(RawEvent::OutputDone { output: proxy }),
                    wl_output::Event::Description { .. } => None,
                };
                sink.events.extend(raw);
            }
        }
        ObjectKind::XdgOutput => {
            // SAFETY: see the note above the `match`.
            if let Some(event) = unsafe { zxdg_output_v1::decode_event(opcode, args) } {
                let raw = match event {
                    zxdg_output_v1::Event::LogicalPosition { x, y } => {
                        Some(RawEvent::XdgOutputPosition {
                            xdg_output: proxy,
                            x,
                            y,
                        })
                    }
                    zxdg_output_v1::Event::LogicalSize { width, height } => {
                        Some(RawEvent::XdgOutputSize {
                            xdg_output: proxy,
                            width,
                            height,
                        })
                    }
                    // `done` is deprecated from version 3: `wl_output.done` is
                    // the atomic signal, and this backend settles on that one
                    // for every version so the two paths cannot disagree.
                    _ => None,
                };
                sink.events.extend(raw);
            }
        }
        ObjectKind::Surface => {
            // SAFETY: see the note above the `match`.
            if let Some(event) = unsafe { wl_surface::decode_event(opcode, args) } {
                let raw = match event {
                    wl_surface::Event::Enter { output } => Some(RawEvent::SurfaceEnter {
                        surface: proxy,
                        output: output as usize,
                    }),
                    wl_surface::Event::Leave { output } => Some(RawEvent::SurfaceLeave {
                        surface: proxy,
                        output: output as usize,
                    }),
                    // `preferred_buffer_scale`/`preferred_buffer_transform` are
                    // version 6; this backend binds 4 and never sees them.
                    _ => None,
                };
                sink.events.extend(raw);
            }
        }
        ObjectKind::XdgSurface => {
            // SAFETY: see the note above the `match`.
            if let Some(xdg_surface::Event::Configure { serial }) =
                unsafe { xdg_surface::decode_event(opcode, args) }
            {
                sink.events.push(RawEvent::XdgSurfaceConfigure {
                    xdg_surface: proxy,
                    serial,
                });
            }
        }
        ObjectKind::XdgToplevel => {
            // SAFETY: see the note above the `match`.
            if let Some(event) = unsafe { xdg_toplevel::decode_event(opcode, args) } {
                let raw = match event {
                    xdg_toplevel::Event::Configure {
                        width,
                        height,
                        states,
                    } => Some(RawEvent::ToplevelConfigure {
                        toplevel: proxy,
                        width,
                        height,
                        states: decode_state_array(states),
                    }),
                    xdg_toplevel::Event::Close => Some(RawEvent::ToplevelClose { toplevel: proxy }),
                    _ => None,
                };
                sink.events.extend(raw);
            }
        }
        ObjectKind::Decoration => {
            // SAFETY: see the note above the `match`.
            if let Some(zxdg_toplevel_decoration_v1::Event::Configure { mode }) =
                unsafe { zxdg_toplevel_decoration_v1::decode_event(opcode, args) }
            {
                sink.events.push(RawEvent::DecorationConfigure {
                    decoration: proxy,
                    mode,
                });
            }
        }
        ObjectKind::FractionalScale => {
            // SAFETY: see the note above the `match`.
            if let Some(wp_fractional_scale_v1::Event::PreferredScale { scale }) =
                unsafe { wp_fractional_scale_v1::decode_event(opcode, args) }
            {
                sink.events.push(RawEvent::PreferredScale {
                    object: proxy,
                    scale,
                });
            }
        }
        ObjectKind::Seat => {
            // SAFETY: see the note above the `match`.
            if let Some(wl_seat::Event::Capabilities { capabilities }) =
                unsafe { wl_seat::decode_event(opcode, args) }
            {
                sink.events.push(RawEvent::SeatCapabilities {
                    seat: proxy,
                    capabilities,
                });
            }
        }
        ObjectKind::Pointer => {
            // SAFETY: see the note above the `match`.
            if let Some(event) = unsafe { wl_pointer::decode_event(opcode, args) } {
                sink.events.extend(decode_pointer(proxy, event));
            }
        }
        ObjectKind::Keyboard => {
            // SAFETY: see the note above the `match`.
            if let Some(event) = unsafe { wl_keyboard::decode_event(opcode, args) } {
                sink.events.extend(decode_keyboard(proxy, event));
            }
        }
        ObjectKind::DataDevice => {
            // SAFETY: see the note above the `match`.
            if let Some(event) = unsafe { wl_data_device::decode_event(opcode, args) } {
                // `data_offer` introduces an object the compositor created for
                // us, and its mime types follow immediately — so the dispatcher
                // has to be attached here rather than after the drain. See
                // `Sink::watch`.
                if let wl_data_device::Event::DataOffer { id } = event {
                    sink.watch(id, ObjectKind::DataOffer);
                }
                sink.events.extend(decode_data_device(proxy, event));
            }
        }
        ObjectKind::DataOffer => {
            // SAFETY: see the note above the `match`.
            if let Some(wl_data_offer::Event::Offer { mime_type }) =
                unsafe { wl_data_offer::decode_event(opcode, args) }
            {
                sink.events.push(RawEvent::OfferMime {
                    offer: proxy,
                    mime: mime_type.to_string_lossy().into_owned(),
                });
            }
            // `source_actions` and `action` report which drag action the
            // compositor settled on. This backend asks for `copy` and nothing
            // else, so there is no negotiation to observe.
        }
        ObjectKind::DataSource => {
            // SAFETY: see the note above the `match`.
            if let Some(event) = unsafe { wl_data_source::decode_event(opcode, args) } {
                let raw = match event {
                    wl_data_source::Event::Send { mime_type, fd } => Some(RawEvent::SourceSend {
                        source: proxy,
                        mime: mime_type.to_string_lossy().into_owned(),
                        fd,
                    }),
                    wl_data_source::Event::Cancelled => {
                        Some(RawEvent::SourceCancelled { source: proxy })
                    }
                    // `target`, `dnd_drop_performed`, `dnd_finished` and
                    // `action` are all drag-*source* events, and this backend
                    // starts no drags outside its own test scaffolding.
                    _ => None,
                };
                sink.events.extend(raw);
            }
        }
        ObjectKind::RelativePointer => {
            // SAFETY: see the note above the `match`.
            if let Some(zwp_relative_pointer_v1::Event::RelativeMotion {
                utime_hi,
                utime_lo,
                dx_unaccel,
                dy_unaccel,
                ..
            }) = unsafe { zwp_relative_pointer_v1::decode_event(opcode, args) }
            {
                sink.events.push(RawEvent::RelativeMotion {
                    relative: proxy,
                    utime_hi,
                    utime_lo,
                    dx_unaccel,
                    dy_unaccel,
                });
            }
        }
    }
    0
}

/// Splits `wl_pointer`'s ten events out of the dispatcher, purely so
/// [`dispatch`] stays readable.
fn decode_pointer(proxy: usize, event: wl_pointer::Event) -> Option<RawEvent> {
    Some(match event {
        wl_pointer::Event::Enter {
            serial,
            surface,
            surface_x,
            surface_y,
        } => RawEvent::PointerEnter {
            pointer: proxy,
            serial,
            surface: surface as usize,
            x: surface_x,
            y: surface_y,
        },
        wl_pointer::Event::Leave { serial, surface } => RawEvent::PointerLeave {
            pointer: proxy,
            serial,
            surface: surface as usize,
        },
        wl_pointer::Event::Motion {
            time,
            surface_x,
            surface_y,
        } => RawEvent::PointerMotion {
            pointer: proxy,
            time,
            x: surface_x,
            y: surface_y,
        },
        wl_pointer::Event::Button {
            serial,
            time,
            button,
            state,
        } => RawEvent::PointerButton {
            pointer: proxy,
            serial,
            time,
            button,
            state,
        },
        wl_pointer::Event::Axis { time, axis, value } => RawEvent::PointerAxis {
            pointer: proxy,
            time,
            axis,
            value,
        },
        wl_pointer::Event::AxisSource { axis_source } => RawEvent::PointerAxisSource {
            pointer: proxy,
            source: axis_source,
        },
        wl_pointer::Event::AxisDiscrete { axis, discrete } => RawEvent::PointerAxisDiscrete {
            pointer: proxy,
            axis,
            discrete,
        },
        wl_pointer::Event::AxisValue120 { axis, value120 } => RawEvent::PointerAxisValue120 {
            pointer: proxy,
            axis,
            value120,
        },
        wl_pointer::Event::Frame => RawEvent::PointerFrame { pointer: proxy },
        // `axis_stop` says a kinetic scroll ended, which produces no delta and
        // nothing this seam reports; `axis_relative_direction` is version 9,
        // which this backend does not bind.
        _ => return None,
    })
}

/// Splits `wl_keyboard`'s six events out of the dispatcher; see
/// [`decode_pointer`].
fn decode_keyboard(proxy: usize, event: wl_keyboard::Event<'_>) -> Option<RawEvent> {
    Some(match event {
        wl_keyboard::Event::Keymap { format, fd, size } => RawEvent::KeyboardKeymap {
            keyboard: proxy,
            format,
            fd,
            size,
        },
        // The `keys` array lists what was already held when focus arrived. It
        // is deliberately dropped: those presses happened before this window
        // had focus, and synthesizing edges for them would make the action
        // layer fire a jump for a key the player was holding in another
        // application. `ShellEvent::Focus` already tells a consumer to treat
        // focus as a clean slate.
        wl_keyboard::Event::Enter {
            serial, surface, ..
        } => RawEvent::KeyboardEnter {
            keyboard: proxy,
            serial,
            surface: surface as usize,
        },
        wl_keyboard::Event::Leave { serial, surface } => RawEvent::KeyboardLeave {
            keyboard: proxy,
            serial,
            surface: surface as usize,
        },
        wl_keyboard::Event::Key {
            serial,
            time,
            key,
            state,
        } => RawEvent::KeyboardKey {
            keyboard: proxy,
            serial,
            time,
            key,
            state,
        },
        wl_keyboard::Event::Modifiers {
            serial,
            mods_depressed,
            mods_latched,
            mods_locked,
            group,
        } => RawEvent::KeyboardModifiers {
            keyboard: proxy,
            serial,
            depressed: mods_depressed,
            latched: mods_latched,
            locked: mods_locked,
            group,
        },
        wl_keyboard::Event::RepeatInfo { rate, delay } => RawEvent::KeyboardRepeatInfo {
            keyboard: proxy,
            rate,
            delay,
        },
    })
}

/// Splits `wl_data_device`'s six events out of the dispatcher; see
/// [`decode_pointer`].
///
/// The proxy arguments are flattened to addresses here, exactly as the pointer
/// and keyboard decoders flatten surfaces: the raw pointer must not outlive the
/// dispatcher, and an address is all `process_data` needs to find the object
/// again.
fn decode_data_device(proxy: usize, event: wl_data_device::Event) -> Option<RawEvent> {
    Some(match event {
        wl_data_device::Event::DataOffer { id } => RawEvent::DataOffer {
            device: proxy,
            offer: id as usize,
        },
        wl_data_device::Event::Selection { id } => RawEvent::Selection {
            device: proxy,
            offer: id as usize,
        },
        wl_data_device::Event::Enter {
            serial,
            surface,
            x,
            y,
            id,
        } => RawEvent::DragEnter {
            device: proxy,
            serial,
            surface: surface as usize,
            x,
            y,
            offer: id as usize,
        },
        wl_data_device::Event::Leave => RawEvent::DragLeave { device: proxy },
        wl_data_device::Event::Motion { time, x, y } => RawEvent::DragMotion {
            device: proxy,
            time,
            x,
            y,
        },
        wl_data_device::Event::Drop => RawEvent::DragDrop { device: proxy },
    })
}

/// `xdg_toplevel.configure`'s states array is a `wl_array` of native-endian
/// `uint32_t`.
fn decode_state_array(bytes: &[u8]) -> Vec<u32> {
    bytes
        .chunks_exact(4)
        .map(|chunk| u32::from_ne_bytes([chunk[0], chunk[1], chunk[2], chunk[3]]))
        .collect()
}

// ---------------------------------------------------------------------------
// Connection
// ---------------------------------------------------------------------------

/// The connection and the globals bound on it.
struct Conn {
    lib: &'static Lib,
    display: NonNull<WlDisplay>,
    fd: c_int,
    registry: *mut WlProxy,
    compositor: *mut WlProxy,
    wm_base: *mut WlProxy,
    xdg_output_manager: *mut WlProxy,
    decoration_manager: *mut WlProxy,
    fractional_scale_manager: *mut WlProxy,
    viewporter: *mut WlProxy,
    relative_pointer_manager: *mut WlProxy,
    pointer_constraints: *mut WlProxy,
    data_device_manager: *mut WlProxy,
    /// Owned by the connection, reached only as a raw pointer; see [`Sink`].
    sink: *mut Sink,
}

impl Conn {
    /// The event queue, as a mutable reference.
    ///
    /// Takes `&mut self` even though the pointer would allow `&self`: the
    /// dispatcher's `&mut Sink` and this one must never overlap, and requiring
    /// a unique borrow of the connection to get one is the cheapest way to make
    /// that visible at every call site.
    fn sink(&mut self) -> &mut Sink {
        // SAFETY: `self.sink` came from `Box::into_raw` and is live for the
        // connection's lifetime. The dispatcher is the only other holder of a
        // reference to it, and it runs strictly inside the libwayland calls in
        // `Conn::drain`, where this method is not called.
        unsafe { &mut *self.sink }
    }

    /// Attaches the dispatcher to `proxy` and records what it is.
    fn watch(&mut self, proxy: *mut WlProxy, kind: ObjectKind) {
        self.sink().watch(proxy, kind);
    }

    /// Destroys a proxy client-side and stops routing its events.
    ///
    /// For an object whose interface has **no** destructor request. Anything
    /// with one must go through [`release`](Self::release) instead, or the
    /// compositor never learns the object is gone — which for a
    /// `zwp_locked_pointer_v1` means the pointer stays locked forever.
    fn destroy(&mut self, proxy: *mut WlProxy) {
        if proxy.is_null() {
            return;
        }
        self.sink().forget(proxy as usize);
        // SAFETY: `proxy` is live on this connection and is not used again —
        // every caller clears its own copy of the pointer.
        unsafe { (self.lib.proxy_destroy)(proxy) };
    }

    /// Sends a protocol destructor and stops routing the proxy's events.
    ///
    /// The generated destructor wrappers marshal with `WL_MARSHAL_FLAG_DESTROY`,
    /// which both tells the compositor and frees the proxy, so this must not be
    /// followed by a [`destroy`](Self::destroy).
    ///
    /// # Safety
    ///
    /// `destructor` must be the generated destructor request of `proxy`'s own
    /// interface, and `proxy` must be live on this connection.
    unsafe fn release(&mut self, proxy: *mut WlProxy, destructor: unsafe fn(*mut WlProxy)) {
        if proxy.is_null() {
            return;
        }
        self.sink().forget(proxy as usize);
        // SAFETY: the caller guarantees `destructor` belongs to this proxy's
        // interface and that the proxy is live.
        unsafe { destructor(proxy) };
    }

    /// [`release`](Self::release), but only if the proxy was bound at a
    /// version that has the destructor.
    ///
    /// `wl_pointer.release` and `wl_keyboard.release` arrived in `wl_seat`
    /// version 3. Sending a request a proxy's bound version does not have is a
    /// protocol error that disconnects the client — so on a hypothetical
    /// version-1 or -2 seat the proxy is dropped client-side instead. The
    /// server keeps its object until the seat goes, which is the same outcome
    /// those versions offered in the first place.
    ///
    /// # Safety
    ///
    /// As [`release`](Self::release).
    unsafe fn release_since(
        &mut self,
        proxy: *mut WlProxy,
        since: u32,
        destructor: unsafe fn(*mut WlProxy),
    ) {
        if proxy.is_null() {
            return;
        }
        // SAFETY: the caller guarantees the proxy is live.
        if unsafe { ffi::proxy_version(proxy) } >= since {
            // SAFETY: the caller guarantees `destructor` is this interface's.
            unsafe { self.release(proxy, destructor) };
        } else {
            self.destroy(proxy);
        }
    }

    fn display_proxy(&self) -> *mut WlProxy {
        ffi::display_as_proxy(self.display.as_ptr())
    }

    /// Sends everything queued.
    fn flush(&self) {
        // SAFETY: the display is live for the connection's lifetime.
        unsafe { (self.lib.display_flush)(self.display.as_ptr()) };
    }

    /// A blocking round trip: send everything, then wait for the server to
    /// answer a `wl_display.sync`.
    ///
    /// Used only during [`WaylandShell::open`], where blocking is correct —
    /// there is no frame loop yet, and the alternative is a shell that reports
    /// no monitors for the first few frames.
    fn roundtrip(&self) -> Result<(), ShellError> {
        // SAFETY: the display is live.
        if unsafe { (self.lib.display_roundtrip)(self.display.as_ptr()) } < 0 {
            return Err(self.disconnected());
        }
        Ok(())
    }

    /// Non-blocking read + dispatch. See [`WaylandShell::pump`].
    ///
    /// `aux` are clipboard-transfer pipes: they never drive the protocol, and
    /// they are in the wait so that a blocked editor wakes when a paste's bytes
    /// arrive rather than sleeping on them.
    fn drain(&self, timeout_ms: c_int, aux: &[c_int]) -> Result<(), ShellError> {
        let display = self.display.as_ptr();
        // SAFETY (whole block): `display` is live, and this is libwayland's
        // documented non-blocking read sequence. Every `prepare_read` is paired
        // with exactly one `read_events` or `cancel_read`, which is the
        // invariant that keeps other threads from deadlocking on the read
        // barrier — and the reason the sequence is written out rather than
        // replaced by `wl_display_dispatch`, which blocks.
        unsafe {
            if (self.lib.display_dispatch_pending)(display) < 0 {
                return Err(self.disconnected());
            }
            // `prepare_read` fails while this thread still has queued events;
            // dispatch them and try again. Bounded by the queue, not by time.
            while (self.lib.display_prepare_read)(display) != 0 {
                if (self.lib.display_dispatch_pending)(display) < 0 {
                    return Err(self.disconnected());
                }
            }
            if (self.lib.display_flush)(display) < 0 && timeout_ms != 0 {
                // A full socket with nothing to read would otherwise block for
                // the whole timeout waiting for a peer that is waiting for us.
                (self.lib.display_cancel_read)(display);
                return Ok(());
            }
            if !ffi::poll_readable_with(self.fd, aux, timeout_ms) {
                (self.lib.display_cancel_read)(display);
            } else if (self.lib.display_read_events)(display) < 0 {
                return Err(self.disconnected());
            }
            if (self.lib.display_dispatch_pending)(display) < 0 {
                return Err(self.disconnected());
            }
        }
        Ok(())
    }

    fn disconnected(&self) -> ShellError {
        // SAFETY: the display is live; `wl_display_get_error` is pure.
        let code = unsafe { (self.lib.display_get_error)(self.display.as_ptr()) };
        ShellError::Disconnected {
            backend: ShellBackend::Wayland,
            detail: format!("errno {code}"),
        }
    }
}

// ---------------------------------------------------------------------------
// Monitors
// ---------------------------------------------------------------------------

/// A `wl_output` and what it has told us so far.
#[derive(Debug)]
struct Output {
    proxy: usize,
    /// The matching `zxdg_output_v1`, or null before the manager was bound.
    xdg: *mut WlProxy,
    /// `wl_registry.global`'s name, for the matching `global_remove`.
    global: u32,
    id: MonitorId,
    x: i32,
    y: i32,
    width: i32,
    height: i32,
    refresh_millihertz: u32,
    scale: i32,
    /// `zxdg_output_v1.logical_position`, in the compositor's logical space.
    logical_x: i32,
    logical_y: i32,
    /// `zxdg_output_v1.logical_size`; zero until it arrives.
    logical_width: i32,
    logical_height: i32,
    name: String,
    /// Whether a `wl_output.done` has been seen, i.e. whether the fields above
    /// are a consistent snapshot rather than a half-applied update.
    settled: bool,
}

impl Output {
    /// The output's true scale, which is not necessarily `wl_output.scale`.
    ///
    /// `wl_output.scale` is an **integer**, so a compositor running an output
    /// at 150 % reports `2` — it is defined as the buffer scale a client should
    /// use, not as the desktop's scale. `xdg_output`'s logical size is the same
    /// output measured in the desktop's own units, so the ratio between the
    /// mode and it is the real number. This is how a monitor at 1.5 stops
    /// claiming to be at 2.
    fn scale_factor(&self) -> f64 {
        if self.logical_width > 0 && self.width > 0 {
            f64::from(self.width) / f64::from(self.logical_width)
        } else {
            f64::from(self.scale.max(1))
        }
    }

    #[expect(
        clippy::cast_possible_truncation,
        reason = "a desktop coordinate that overflows i32 after scaling would \
                  need a monitor layout billions of pixels wide; the clamp \
                  keeps it defined regardless"
    )]
    fn info(&self, is_primary: bool) -> MonitorInfo {
        let width = self.width.max(0).unsigned_abs();
        let height = self.height.max(0).unsigned_abs();
        let scale = self.scale_factor();
        // Position from `xdg_output`, scaled back into this output's device
        // pixels so that it and the size below are in one unit.
        //
        // The honest caveat, which nothing may paper over: on a desktop whose
        // outputs have *different* scales there is no single device-pixel
        // coordinate space, and these rectangles will not tile. The compositor's
        // logical space is the only globally coherent one, and `PhysicalRect`
        // cannot express it. `ShellCaps::WINDOW_POSITION` stays clear, so
        // nothing is entitled to treat this as a desktop layout — it is good
        // enough to name a monitor and to tell a settings screen which one is
        // on the left.
        let (x, y) = if self.logical_width > 0 {
            let to_pixels = |value: i32| (f64::from(value) * scale).round();
            (
                to_pixels(self.logical_x).clamp(f64::from(i32::MIN), f64::from(i32::MAX)) as i32,
                to_pixels(self.logical_y).clamp(f64::from(i32::MIN), f64::from(i32::MAX)) as i32,
            )
        } else {
            // No `xdg_output`: `wl_output.geometry`'s position is logical while
            // the size is physical, so they only agree at scale 1. Kept as the
            // fallback because it is what the protocol guarantees.
            (self.x, self.y)
        };
        MonitorInfo {
            id: self.id,
            name: self.name.clone(),
            bounds: PhysicalRect::new(x, y, width, height),
            // Wayland has no work-area protocol at all. `MonitorInfo` documents
            // that a backend which cannot find out reports the full area.
            work_area: PhysicalRect::new(x, y, width, height),
            scale_factor: scale,
            refresh_millihertz: self.refresh_millihertz,
            is_primary,
        }
    }
}

// ---------------------------------------------------------------------------
// Seats
// ---------------------------------------------------------------------------

/// One `wl_seat` and the devices it currently has.
///
/// # One [`DeviceId`] per seat, not per physical device
///
/// `docs/plan/19-input.md` wants per-device ids so that local multiplayer can
/// assign devices to players later. On Wayland the *seat* is that unit and
/// there is no finer one: libinput merges every physical keyboard on a seat
/// into one `wl_keyboard` and every mouse into one `wl_pointer`, and the
/// protocol exposes no device list at all. Reporting a made-up id per physical
/// device would be a fiction; reporting the seat is the true grouping, and a
/// multi-seat session — which is exactly how Linux does local multiplayer —
/// produces exactly the distinct ids that layer needs.
struct Seat {
    proxy: *mut WlProxy,
    global: u32,
    device: DeviceId,
    capabilities: u32,
    pointer: *mut WlProxy,
    keyboard: *mut WlProxy,
    relative_pointer: *mut WlProxy,
    /// This seat's `wl_data_device`, once the manager global exists.
    ///
    /// One per seat, not one per shell: the clipboard *is* a seat's, and a
    /// multi-seat session has as many independent clipboards as it has seats.
    data: Option<data::Device>,
    /// The serial of the last `wl_pointer.enter`, which `set_cursor` and
    /// `set_shape` both have to quote.
    enter_serial: u32,
    /// The serial of the most recent input event of **any** kind on this seat.
    ///
    /// `wl_data_device.set_selection` takes "the serial of the event that
    /// triggered this request", and a compositor is entitled to check it —
    /// wlroots compares it against the serial of the selection currently in
    /// effect and drops a request that looks older, so a backend that passed
    /// zero would have its clipboard writes silently ignored. Tracking every
    /// serial the seat delivers is what makes "the user pressed Ctrl+C, so this
    /// copy is current" expressible from a seam that has no serials in it.
    last_serial: u32,
    /// The serial of the last pointer **press**, which is the implicit grab a
    /// `wl_data_device.start_drag` has to name. Distinct from
    /// [`last_serial`](Self::last_serial) because any later event would
    /// invalidate the grab.
    press_serial: u32,
    pointer_focus: Option<WindowId>,
    /// Last known position, in the focused window's device pixels.
    pointer_position: PhysicalPoint,
    frame: PointerFrame,
    keyboard_focus: Option<WindowId>,
    keymap: Option<xkb::Keymap>,
    modifiers: Modifiers,
    /// Held keys, for the degraded modifier path only; see [`xkb`].
    held: Vec<KeyCode>,
    repeat: Repeat,
}

impl core::fmt::Debug for Seat {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        f.debug_struct("Seat")
            .field("device", &self.device)
            .field("capabilities", &self.capabilities)
            .field("keymap", &self.keymap.is_some())
            .field("data_device", &self.data.is_some())
            .finish()
    }
}

/// Everything a `wl_pointer` said between two `wl_pointer.frame`s.
///
/// The protocol groups pointer events into frames precisely so a client does
/// not treat one physical movement as several logical ones: a diagonal move
/// arrives as one `motion`, a wheel click as an `axis` plus an `axis_value120`
/// plus an `axis_source`, and a locked pointer's motion arrives on a *different
/// object* (`zwp_relative_pointer_v1`) inside the same frame. Accumulating and
/// emitting once per frame is what turns that into one
/// [`ShellEvent::PointerMotion`] and one [`ShellEvent::Wheel`].
#[derive(Clone, Copy, Debug, Default)]
struct PointerFrame {
    /// Compositor milliseconds, from whichever event carried one.
    time: u32,
    /// Surface-local position, in logical units.
    motion: Option<(f64, f64)>,
    /// Unaccelerated relative motion, in the device's own units.
    raw: Option<(f64, f64)>,
    /// `CLOCK_MONOTONIC` microseconds from `relative_motion`, which is the more
    /// precise of the two clocks a motion can arrive on.
    raw_micros: Option<u64>,
    /// `[vertical, horizontal]`, in 1/120ths of a detent.
    value120: [f64; 2],
    /// `[vertical, horizontal]`, in whole detents (`wl_pointer` 5 to 7).
    discrete: [f64; 2],
    /// `[vertical, horizontal]`, in surface-local units.
    continuous: [f64; 2],
    axis_source: Option<u32>,
    has_axis: bool,
}

impl PointerFrame {
    fn is_empty(&self) -> bool {
        self.motion.is_none() && self.raw.is_none() && !self.has_axis
    }

    /// The scroll this frame describes, or `None` when it describes none.
    ///
    /// # Detents versus pixels
    ///
    /// [`ScrollDelta`] refuses to collapse the two, and this is where the
    /// distinction is made. A notched wheel reports `axis_value120` (or
    /// `axis_discrete` before version 8) *in addition to* a continuous `axis`
    /// value, so preferring the discrete number is what stops a wheel click
    /// being reported as "15 pixels" — a number the compositor made up from its
    /// own scroll-speed setting. A touchpad reports only `axis`, and gets
    /// pixels, which is the truth about a touchpad.
    ///
    /// # Sign
    ///
    /// Wayland's vertical axis is positive **downwards** (content moves up as
    /// the value grows) and its horizontal axis positive rightwards.
    /// [`ScrollDelta`] documents positive `y` as "scrolls the content up (away
    /// from the user)", the convention every other platform uses, so both axes
    /// are negated here — once, in one place.
    fn scroll(&self, scale: f64) -> Option<ScrollDelta> {
        if !self.has_axis {
            return None;
        }
        let detents = |raw: [f64; 2], divisor: f64| ScrollDelta::Lines {
            #[expect(
                clippy::cast_possible_truncation,
                reason = "ScrollDelta::Lines is f32 by seam definition; a detent \
                          count is a small integer"
            )]
            x: (-raw[1] / divisor) as f32,
            #[expect(clippy::cast_possible_truncation, reason = "as above")]
            y: (-raw[0] / divisor) as f32,
        };
        if self.value120 != [0.0, 0.0] {
            return Some(detents(self.value120, 120.0));
        }
        if self.discrete != [0.0, 0.0] {
            return Some(detents(self.discrete, 1.0));
        }
        if self.continuous != [0.0, 0.0] {
            return Some(ScrollDelta::Pixels {
                x: -self.continuous[1] * scale,
                y: -self.continuous[0] * scale,
            });
        }
        // An `axis_stop`, or a frame whose only axis content was a zero. The
        // seam's `ScrollDelta::is_zero` exists for consumers that want these;
        // producing one here would mean a `Wheel` event per kinetic-scroll
        // teardown, which is noise.
        None
    }
}

/// Key-repeat parameters and the key currently repeating.
///
/// See the [module docs](self) for why this lives in the backend at all.
#[derive(Clone, Copy, Debug, Default)]
struct Repeat {
    /// Repeats per second; `0` disables repeat entirely, which is what
    /// `wl_keyboard.repeat_info` means by it.
    rate: i32,
    /// Milliseconds before the first repeat.
    delay: i32,
    key: Option<RepeatKey>,
}

/// The one key a seat is repeating.
#[derive(Clone, Copy, Debug)]
struct RepeatKey {
    window: WindowId,
    scancode: u32,
    key_code: Option<KeyCode>,
    keysym: Keysym,
    /// `CLOCK_MONOTONIC` nanoseconds at which the next repeat is due.
    next_nanos: u64,
    /// Nanoseconds between repeats.
    interval_nanos: u64,
}

impl Repeat {
    /// Starts repeating `key`, if repeat is enabled at all.
    fn start(&mut self, now_nanos: u64, key: RepeatKeyRequest) {
        if self.rate <= 0 {
            self.key = None;
            return;
        }
        let interval_nanos = 1_000_000_000 / u64::try_from(self.rate).unwrap_or(1).max(1);
        let delay_nanos = u64::try_from(self.delay.max(0)).unwrap_or(0) * 1_000_000;
        self.key = Some(RepeatKey {
            window: key.window,
            scancode: key.scancode,
            key_code: key.key_code,
            keysym: key.keysym,
            next_nanos: now_nanos.saturating_add(delay_nanos),
            interval_nanos,
        });
    }

    /// Stops repeating, if `scancode` is what is repeating.
    fn stop(&mut self, scancode: u32) {
        if self.key.is_some_and(|key| key.scancode == scancode) {
            self.key = None;
        }
    }

    /// `CLOCK_MONOTONIC` nanoseconds at which the next repeat is due.
    fn due_at(&self) -> Option<u64> {
        self.key.map(|key| key.next_nanos)
    }
}

/// What [`Repeat::start`] needs to know, so it does not take six arguments.
#[derive(Clone, Copy, Debug)]
struct RepeatKeyRequest {
    window: WindowId,
    scancode: u32,
    key_code: Option<KeyCode>,
    keysym: Keysym,
}

// ---------------------------------------------------------------------------
// Windows
// ---------------------------------------------------------------------------

/// State accumulated from an `xdg_toplevel.configure` that has not been
/// completed by an `xdg_surface.configure` yet.
#[derive(Clone, Debug, Default)]
struct PendingConfigure {
    /// Logical size, or `None` when the compositor sent `0 × 0` — "you choose".
    size: Option<LogicalSize>,
    states: Vec<u32>,
}

/// One `xdg_toplevel` and its surface stack.
#[derive(Debug)]
struct WlWindow {
    surface: *mut WlProxy,
    xdg_surface: *mut WlProxy,
    toplevel: *mut WlProxy,
    decoration: *mut WlProxy,
    fractional_scale: *mut WlProxy,
    viewport: *mut WlProxy,
    title: String,
    requested_size: LogicalSize,
    requested_mode: DisplayMode,
    requested_constraints: SizeConstraints,
    /// [`WindowDesc::resizable`], kept because xdg-shell has no way to *ask*
    /// what it is: the flag is expressed as `min == max`, so every later
    /// `set_min_size`/`set_max_size` has to restate it. Without the field,
    /// `set_constraints` had to guess `true`, and a window created
    /// `resizable: false` became user-resizable the first time a caller touched
    /// its constraints — with no way back. The X11 backend keeps the same flag
    /// for the same reason.
    resizable: bool,
    configuration: Option<WindowConfiguration>,
    pending: PendingConfigure,
    /// Outputs the surface is on, in `wl_surface.enter` order. Empty until the
    /// surface is mapped, which is why the first configure is always scale 1.
    outputs: Vec<usize>,
    scale: i32,
    /// `wp_fractional_scale_v1.preferred_scale`, in 1/120ths, once it arrives.
    preferred_scale: Option<u32>,
    pointer_mode: PointerMode,
    /// Live `zwp_locked_pointer_v1`/`zwp_confined_pointer_v1` objects as
    /// `(the wl_pointer they name, the constraint)`.
    ///
    /// Keyed by the pointer because a constraint is a per-`(surface, pointer)`
    /// object: when one seat unplugs its mouse, only *its* constraint may be
    /// torn down, and another seat's has to keep working.
    constraints: Vec<(usize, *mut WlProxy)>,
    /// The cursor last asked for. The outer `Option` distinguishes "never set",
    /// where the compositor's default stands, from "set to hidden".
    cursor: Option<Option<CursorIcon>>,
    /// [`WindowDesc::accept_drops`], which this backend enforces rather than
    /// records: a window that did not ask for drops answers
    /// `wl_data_offer.accept(null)` and never reads the dropped data.
    accept_drops: bool,
    focused: bool,
    visible: bool,
    close_pending: bool,
}

impl WlWindow {
    /// The logical size to use when the compositor declines to pick one.
    fn logical_size(&self) -> LogicalSize {
        self.pending.size.unwrap_or_else(|| {
            self.configuration.map_or(self.requested_size, |config| {
                LogicalSize::new(
                    f64::from(config.size.width) / config.scale_factor,
                    f64::from(config.size.height) / config.scale_factor,
                )
            })
        })
    }

    /// Device pixels per logical unit for this window.
    ///
    /// `fractional-scale-v1` when the compositor offered it, and the integer
    /// `wl_surface.enter` scale otherwise. The two are mutually exclusive by
    /// protocol: a surface using a `wp_viewport` to express its size must not
    /// also set a buffer scale.
    fn scale_factor(&self) -> f64 {
        match self.preferred_scale {
            Some(scale) if scale > 0 => f64::from(scale) / FRACTIONAL_SCALE_DENOMINATOR,
            _ => f64::from(self.scale.max(1)),
        }
    }
}

// ---------------------------------------------------------------------------
// The shell
// ---------------------------------------------------------------------------

/// A [`Shell`] backed by a real Wayland compositor.
///
/// Constructed through [`open`](crate::open) or
/// [`open_backend`](crate::open_backend); see the [module docs](self) for what
/// this backend implements and how a real compositor differs from
/// [`HeadlessShell`](crate::HeadlessShell).
pub struct WaylandShell {
    conn: Conn,
    windows: Pool<WlWindow>,
    outputs: Vec<Output>,
    seats: Vec<Seat>,
    monitors: Vec<MonitorInfo>,
    next_monitor_id: u32,
    next_device_id: u32,
    next_request_id: u32,
    /// Pipes being drained into a clipboard answer or a drop, and the payloads
    /// being fed into pipes a peer is reading. Both are serviced once per
    /// [`pump`](Shell::pump) and neither ever blocks; see [`fd`].
    transfers: Vec<Transfer>,
    writes: Vec<fd::Writing>,
    /// Reads accepted while the clipboard was not readable; see [`HeldRead`].
    held: Vec<HeldRead>,
    queue: VecDeque<ShellEvent>,
    /// The epoch every event timestamp is measured from.
    time: TimeBase,
    /// Latched at [`open`](Self::open); see [`caps`](Shell::caps).
    caps: ShellCaps,
    lost: Option<String>,
}

impl core::fmt::Debug for WaylandShell {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        f.debug_struct("WaylandShell")
            .field("windows", &self.windows.len())
            .field("monitors", &self.monitors.len())
            .field("seats", &self.seats.len())
            .field(
                "transfers",
                &(self.transfers.len() + self.writes.len() + self.held.len()),
            )
            .field("queued_events", &self.queue.len())
            .field("connected", &self.lost.is_none())
            .finish()
    }
}

impl WaylandShell {
    /// Connects to the compositor named by `WAYLAND_DISPLAY`.
    ///
    /// Binds every global this backend understands and round-trips twice: once
    /// for the registry listing, once for the properties of what was bound.
    /// Blocking here is correct — there is no frame loop yet, and the
    /// alternative is a shell that reports no monitors and no capabilities for
    /// the first few frames.
    ///
    /// # Errors
    ///
    /// [`ShellError::Connect`] if libwayland-client cannot be loaded, if there
    /// is no compositor socket, or if the compositor does not implement
    /// `xdg_wm_base` — a compositor without it cannot show a window at all, and
    /// pretending otherwise produces a process with an invisible window and no
    /// error.
    pub fn open() -> Result<Self, ShellError> {
        let lib = ffi::load().map_err(|detail| ShellError::Connect {
            backend: ShellBackend::Wayland,
            detail: detail.to_string(),
        })?;

        // SAFETY: a null name means "read WAYLAND_DISPLAY", which is
        // `wl_display_connect`'s documented behaviour.
        let display = unsafe { (lib.display_connect)(ptr::null()) };
        let Some(display) = NonNull::new(display) else {
            return Err(ShellError::Connect {
                backend: ShellBackend::Wayland,
                detail: match std::env::var("WAYLAND_DISPLAY") {
                    Ok(name) => format!("no compositor at WAYLAND_DISPLAY={name}"),
                    Err(_) => "WAYLAND_DISPLAY is not set".to_string(),
                },
            });
        };

        // SAFETY: the display was just returned live by `wl_display_connect`.
        let fd = unsafe { (lib.display_get_fd)(display.as_ptr()) };
        let conn = Conn {
            lib,
            display,
            fd,
            registry: ptr::null_mut(),
            compositor: ptr::null_mut(),
            wm_base: ptr::null_mut(),
            xdg_output_manager: ptr::null_mut(),
            decoration_manager: ptr::null_mut(),
            fractional_scale_manager: ptr::null_mut(),
            viewporter: ptr::null_mut(),
            relative_pointer_manager: ptr::null_mut(),
            pointer_constraints: ptr::null_mut(),
            data_device_manager: ptr::null_mut(),
            sink: Sink::boxed(lib),
        };

        let mut shell = Self {
            conn,
            windows: Pool::new(),
            outputs: Vec::new(),
            seats: Vec::new(),
            monitors: Vec::new(),
            next_monitor_id: 1,
            next_device_id: 1,
            next_request_id: 1,
            transfers: Vec::new(),
            writes: Vec::new(),
            held: Vec::new(),
            queue: VecDeque::new(),
            time: TimeBase::now(),
            caps: ShellCaps::empty(),
            lost: None,
        };

        // SAFETY: the display proxy is live and `wl_display.get_registry` takes
        // no arguments beyond the new object.
        shell.conn.registry =
            unsafe { protocol::wayland::wl_display::get_registry(shell.conn.display_proxy()) };
        if shell.conn.registry.is_null() {
            return Err(ShellError::Connect {
                backend: ShellBackend::Wayland,
                detail: "wl_display.get_registry failed".to_string(),
            });
        }
        let registry = shell.conn.registry;
        shell.conn.watch(registry, ObjectKind::Registry);

        // First round trip: the registry listing.
        shell.conn.roundtrip()?;
        shell.process_raw();
        // Second: the properties of everything bound in the first, including
        // the `xdg_output`s and the seat capabilities the first round created.
        shell.conn.roundtrip()?;
        shell.process_raw();

        if shell.conn.compositor.is_null() {
            return Err(ShellError::Connect {
                backend: ShellBackend::Wayland,
                detail: "the compositor does not advertise wl_compositor".to_string(),
            });
        }
        if shell.conn.wm_base.is_null() {
            return Err(ShellError::Connect {
                backend: ShellBackend::Wayland,
                detail: "the compositor does not advertise xdg_wm_base, so it cannot show \
                         a window"
                    .to_string(),
            });
        }
        shell.caps = shell.latch_caps();
        // The registry listing and the initial output properties are startup
        // facts, not events anyone asked for.
        shell.queue.clear();
        Ok(shell)
    }

    /// The capability set, computed once from the globals that actually bound.
    ///
    /// [`caps`](crate::caps) requires this to be latched: a Wayland compositor
    /// may advertise a global after startup, and a renderer that chose the blit
    /// path at init cannot be told mid-frame that it should have chosen the
    /// viewport path. So this runs at the end of [`open`](Self::open) and never
    /// again — a protocol that appears later stays unavailable until the next
    /// run.
    fn latch_caps(&self) -> ShellCaps {
        let mut caps = ShellCaps::MULTI_WINDOW | ShellCaps::EVENT_WAIT;
        caps.set(ShellCaps::HW_UPSCALE, !self.conn.viewporter.is_null());
        caps.set(
            ShellCaps::FRACTIONAL_SCALE,
            !self.conn.fractional_scale_manager.is_null() && !self.conn.viewporter.is_null(),
        );
        caps.set(
            ShellCaps::SERVER_DECORATIONS,
            !self.conn.decoration_manager.is_null(),
        );
        caps.set(
            ShellCaps::RAW_POINTER_MOTION,
            !self.conn.relative_pointer_manager.is_null(),
        );
        // `pointer-constraints` advertises both forms in one global; the seam
        // keeps them as separate bits because other platforms do not.
        let constraints = !self.conn.pointer_constraints.is_null();
        caps.set(ShellCaps::POINTER_LOCK, constraints);
        caps.set(ShellCaps::POINTER_CONFINE, constraints);
        // Both bits come from one global, because on Wayland they are one
        // protocol: a `wl_data_device` is the clipboard *and* the drop target,
        // and a compositor cannot offer one without the other. The seam keeps
        // them separate because other platforms can — a browser has a clipboard
        // behind a permission prompt and file drops without one.
        let data_device = !self.conn.data_device_manager.is_null();
        caps.set(ShellCaps::CLIPBOARD, data_device);
        caps.set(ShellCaps::DRAG_DROP, data_device);
        // Not "there is an input method": the seam's own wording is that this
        // bit "says only that the commit path is wired to an input method".
        // What it is really asserting here is that composed text can reach the
        // engine at all, which on Wayland means libxkbcommon resolved. Full
        // `text-input-v3` pre-edit is post-MVP; a compositor-side IME that
        // commits through the keyboard still works today.
        caps.set(ShellCaps::TEXT_IME, xkb::available());
        caps
    }

    /// Aligns event timestamps with an engine
    /// [`TimeSource`](crcbl_core::time::TimeSource).
    ///
    /// See [`Shell::align_event_clock`], which this implements.
    pub fn align_time_base(&mut self, elapsed: Duration) {
        self.time.align(elapsed);
    }

    fn window(&self, window: WindowId) -> Result<&WlWindow, ShellError> {
        self.windows
            .get(window.cast())
            .ok_or_else(|| ShellError::invalid_window(window))
    }

    fn window_mut(&mut self, window: WindowId) -> Result<&mut WlWindow, ShellError> {
        self.windows
            .get_mut(window.cast())
            .ok_or_else(|| ShellError::invalid_window(window))
    }

    fn unsupported(what: &'static str) -> ShellError {
        ShellError::Unsupported {
            backend: ShellBackend::Wayland,
            what,
        }
    }

    fn window_by_proxy(&self, proxy: usize, pick: fn(&WlWindow) -> usize) -> Option<WindowId> {
        self.windows
            .iter()
            .find(|(_, window)| pick(window) == proxy)
            .map(|(handle, _)| handle.cast())
    }

    fn seat_index(&self, proxy: usize, pick: fn(&Seat) -> usize) -> Option<usize> {
        self.seats.iter().position(|seat| pick(seat) == proxy)
    }

    /// Binds one global, if it is one we want.
    #[expect(
        clippy::too_many_lines,
        reason = "one arm per protocol; splitting it would only move the same \
                  list somewhere a reader has to follow to"
    )]
    fn bind_global(&mut self, name: u32, interface: &str, version: u32) {
        let registry = self.conn.registry;
        /// Binds a global into a field of [`Conn`], at the version we cap it to.
        macro_rules! bind {
            ($field:ident, $module:path, $version:expr) => {{
                use $module as target;
                if self.conn.$field.is_null() {
                    // SAFETY: `registry` is live, and `bind` marshals the
                    // interface descriptor's own name and the version asked for.
                    self.conn.$field = unsafe {
                        wl_registry::bind(registry, name, &target::INTERFACE, version.min($version))
                    };
                }
            }};
        }
        match interface {
            "wl_compositor" => bind!(compositor, wl_compositor, COMPOSITOR_VERSION),
            "xdg_wm_base" => {
                // Only watch the proxy the *first* time. A compositor may
                // advertise two globals of the same interface, and re-watching
                // an already-bound one pushed a duplicate into `Sink::objects`
                // and made libwayland log "proxy already has a dispatcher".
                let fresh = self.conn.wm_base.is_null();
                bind!(wm_base, xdg_wm_base, WM_BASE_VERSION);
                if fresh {
                    let proxy = self.conn.wm_base;
                    self.conn.watch(proxy, ObjectKind::WmBase);
                }
            }
            "wp_viewporter" => bind!(viewporter, wp_viewporter, 1),
            "wp_fractional_scale_manager_v1" => {
                bind!(fractional_scale_manager, wp_fractional_scale_manager_v1, 1);
            }
            "zxdg_output_manager_v1" => {
                bind!(
                    xdg_output_manager,
                    zxdg_output_manager_v1,
                    XDG_OUTPUT_VERSION
                );
            }
            "zxdg_decoration_manager_v1" => {
                bind!(
                    decoration_manager,
                    zxdg_decoration_manager_v1,
                    DECORATION_VERSION
                );
            }
            "zwp_relative_pointer_manager_v1" => {
                bind!(relative_pointer_manager, zwp_relative_pointer_manager_v1, 1);
            }
            "zwp_pointer_constraints_v1" => {
                bind!(pointer_constraints, zwp_pointer_constraints_v1, 1);
            }
            "wl_data_device_manager" => {
                bind!(
                    data_device_manager,
                    wl_data_device_manager,
                    DATA_DEVICE_VERSION
                );
            }
            "wl_output" => {
                // SAFETY: `registry` is live; see the macro above.
                let proxy = unsafe {
                    wl_registry::bind(
                        registry,
                        name,
                        &wl_output::INTERFACE,
                        version.min(OUTPUT_VERSION),
                    )
                };
                if proxy.is_null() {
                    return;
                }
                self.conn.watch(proxy, ObjectKind::Output);
                let id = MonitorId(self.next_monitor_id);
                // Never reused within a session, which is the obligation
                // `monitor`'s docs put on a backend in place of a generation.
                self.next_monitor_id += 1;
                self.outputs.push(Output {
                    proxy: proxy as usize,
                    xdg: ptr::null_mut(),
                    global: name,
                    id,
                    x: 0,
                    y: 0,
                    width: 0,
                    height: 0,
                    refresh_millihertz: 0,
                    scale: 1,
                    logical_x: 0,
                    logical_y: 0,
                    logical_width: 0,
                    logical_height: 0,
                    name: format!("wl-output-{}", id.0),
                    settled: false,
                });
            }
            "wl_seat" => {
                // SAFETY: `registry` is live; see the macro above.
                let proxy = unsafe {
                    wl_registry::bind(
                        registry,
                        name,
                        &wl_seat::INTERFACE,
                        version.min(SEAT_VERSION),
                    )
                };
                if proxy.is_null() {
                    return;
                }
                self.conn.watch(proxy, ObjectKind::Seat);
                let device = DeviceId(self.next_device_id);
                self.next_device_id += 1;
                self.seats.push(Seat {
                    proxy,
                    global: name,
                    device,
                    capabilities: 0,
                    pointer: ptr::null_mut(),
                    keyboard: ptr::null_mut(),
                    relative_pointer: ptr::null_mut(),
                    data: None,
                    enter_serial: 0,
                    last_serial: 0,
                    press_serial: 0,
                    pointer_focus: None,
                    pointer_position: PhysicalPoint::ORIGIN,
                    frame: PointerFrame::default(),
                    keyboard_focus: None,
                    keymap: None,
                    modifiers: Modifiers::empty(),
                    held: Vec::new(),
                    repeat: Repeat::default(),
                });
            }
            _ => {}
        }
    }

    /// Gives every seat a `wl_data_device`, once there is a manager to make one
    /// with.
    ///
    /// Called after each registry batch for the same reason
    /// [`ensure_xdg_outputs`](Self::ensure_xdg_outputs) is: the order globals
    /// arrive in is the compositor's choice, and a `wl_seat` announced before
    /// `wl_data_device_manager` would otherwise never get a clipboard. A seat
    /// that appears *later* is covered too, because it goes through the same
    /// registry batch.
    fn ensure_data_devices(&mut self) {
        if self.conn.data_device_manager.is_null() {
            return;
        }
        let manager = self.conn.data_device_manager;
        for index in 0..self.seats.len() {
            if self.seats[index].data.is_some() {
                continue;
            }
            let seat = self.seats[index].proxy;
            // SAFETY: the manager and the seat are both live on this
            // connection; `get_data_device` creates the per-seat endpoint.
            let device = unsafe { wl_data_device_manager::get_data_device(manager, seat) };
            if device.is_null() {
                continue;
            }
            self.conn.watch(device, ObjectKind::DataDevice);
            self.seats[index].data = Some(data::Device::new(device));
        }
        self.conn.flush();
    }

    fn output_mut(&mut self, proxy: usize) -> Option<&mut Output> {
        self.outputs.iter_mut().find(|output| output.proxy == proxy)
    }

    /// Creates a `zxdg_output_v1` for every output that has none.
    ///
    /// Called after each registry batch rather than inside
    /// [`bind_global`](Self::bind_global) because the order globals arrive in
    /// is the compositor's choice: `wl_output` can and does precede
    /// `zxdg_output_manager_v1`, and an output bound before the manager would
    /// otherwise never get its logical geometry.
    fn ensure_xdg_outputs(&mut self) {
        if self.conn.xdg_output_manager.is_null() {
            return;
        }
        let manager = self.conn.xdg_output_manager;
        let pending: Vec<usize> = self
            .outputs
            .iter()
            .filter(|output| output.xdg.is_null())
            .map(|output| output.proxy)
            .collect();
        for proxy in pending {
            // SAFETY: the manager and the `wl_output` are both live on this
            // connection; `get_xdg_output` creates the add-on object.
            let xdg =
                unsafe { zxdg_output_manager_v1::get_xdg_output(manager, proxy as *mut WlProxy) };
            self.conn.watch(xdg, ObjectKind::XdgOutput);
            if let Some(output) = self.output_mut(proxy) {
                output.xdg = xdg;
            }
        }
    }

    /// Rebuilds [`monitors`](Shell::monitors) from the settled outputs.
    fn republish_monitors(&mut self) {
        let monitors: Vec<MonitorInfo> = self
            .outputs
            .iter()
            .filter(|output| output.settled)
            .enumerate()
            // Wayland has no primary-output concept, so the first one seen is
            // the default, exactly as `MonitorInfo::is_primary` prescribes.
            .map(|(index, output)| output.info(index == 0))
            .collect();
        if monitors != self.monitors {
            self.monitors = monitors;
            self.queue.push_back(ShellEvent::MonitorsChanged);
        }
    }

    /// Turns everything the dispatcher queued into state changes and
    /// [`ShellEvent`]s.
    ///
    /// Runs *after* libwayland has returned, never inside it, so it can take
    /// `&mut self` freely.
    #[expect(
        clippy::too_many_lines,
        reason = "a flat routing table from wire events to handlers; every arm \
                  is one or two lines and splitting it hides the routing"
    )]
    fn process_raw(&mut self) {
        let events = core::mem::take(&mut self.conn.sink().events);
        let mut monitors_dirty = false;
        for event in events {
            match event {
                RawEvent::Global {
                    name,
                    interface,
                    version,
                } => self.bind_global(name, &interface, version),
                RawEvent::GlobalRemove { name } => {
                    if let Some(index) =
                        self.outputs.iter().position(|output| output.global == name)
                    {
                        let output = self.outputs.remove(index);
                        let proxy = output.proxy as *mut WlProxy;
                        // Both interfaces have a protocol destructor, which
                        // `Conn::destroy`'s own documentation forbids using it
                        // for: a client-side destroy frees the proxy without
                        // telling the compositor, so on every hotplug the
                        // compositor kept the object until we disconnected.
                        // `zxdg_output_v1.destroy` has existed since version 1;
                        // `wl_output.release` since 3, and this binds 4.
                        // SAFETY: both proxies are live, and each destructor is
                        // its own interface's.
                        unsafe {
                            self.conn.release(output.xdg, zxdg_output_v1::destroy);
                            self.conn.release_since(proxy, 3, wl_output::release);
                        }
                        monitors_dirty = true;
                    }
                    if let Some(index) = self.seats.iter().position(|seat| seat.global == name) {
                        self.remove_seat(index);
                    }
                }
                RawEvent::Ping { serial } => {
                    // Answer or be killed: a compositor that does not get a
                    // pong marks the client unresponsive.
                    // SAFETY: `wm_base` is live whenever a ping can arrive on it.
                    unsafe { xdg_wm_base::pong(self.conn.wm_base, serial) };
                    self.conn.flush();
                }
                RawEvent::OutputGeometry { output, x, y } => {
                    if let Some(output) = self.output_mut(output) {
                        output.x = x;
                        output.y = y;
                    }
                }
                RawEvent::OutputMode {
                    output,
                    flags,
                    width,
                    height,
                    refresh,
                } => {
                    // Bit 0 is `current`; a compositor lists every mode it
                    // supports and only one of them is in effect.
                    if flags & wl_output::mode::CURRENT != 0
                        && let Some(output) = self.output_mut(output)
                    {
                        output.width = width;
                        output.height = height;
                        output.refresh_millihertz = refresh.max(0).unsigned_abs();
                    }
                }
                RawEvent::OutputScale { output, factor } => {
                    if let Some(output) = self.output_mut(output) {
                        output.scale = factor.max(1);
                    }
                }
                RawEvent::OutputName { output, name } => {
                    if let Some(output) = self.output_mut(output) {
                        output.name = name;
                    }
                }
                RawEvent::OutputDone { output } => {
                    if let Some(output) = self.output_mut(output) {
                        output.settled = true;
                        monitors_dirty = true;
                    }
                }
                // Both arms mark the monitor list dirty. On a version-3
                // manager that is belt and braces — the protocol requires a
                // `wl_output.done` after the `xdg_output` burst, and that is
                // what settles the output. On version 1 and 2 it is the only
                // thing that works: those send `zxdg_output_v1.done` instead,
                // which this backend does not listen for (it would be a second
                // settle signal that could disagree with the first), so without
                // this the monitor list would keep the pre-`xdg_output`
                // fallback for the whole session and every fractional scale
                // would read as an integer.
                RawEvent::XdgOutputPosition { xdg_output, x, y } => {
                    if let Some(output) = self.output_by_xdg(xdg_output) {
                        output.logical_x = x;
                        output.logical_y = y;
                        monitors_dirty = true;
                    }
                }
                RawEvent::XdgOutputSize {
                    xdg_output,
                    width,
                    height,
                } => {
                    if let Some(output) = self.output_by_xdg(xdg_output) {
                        output.logical_width = width;
                        output.logical_height = height;
                        monitors_dirty = true;
                    }
                }
                RawEvent::SurfaceEnter { surface, output } => {
                    self.surface_output_changed(surface, output, true);
                }
                RawEvent::SurfaceLeave { surface, output } => {
                    self.surface_output_changed(surface, output, false);
                }
                RawEvent::ToplevelConfigure {
                    toplevel,
                    width,
                    height,
                    states,
                } => {
                    let Some(id) = self.window_by_proxy(toplevel, |w| w.toplevel as usize) else {
                        continue;
                    };
                    let Ok(window) = self.window_mut(id) else {
                        continue;
                    };
                    // `0 × 0` means "you choose" — the usual content of a first
                    // configure, and the single biggest difference from what
                    // `HeadlessShell` models.
                    window.pending.size = (width > 0 && height > 0)
                        .then(|| LogicalSize::new(f64::from(width), f64::from(height)));
                    window.pending.states = states;
                }
                RawEvent::XdgSurfaceConfigure {
                    xdg_surface,
                    serial,
                } => self.apply_configure(xdg_surface, serial),
                RawEvent::ToplevelClose { toplevel } => {
                    let Some(id) = self.window_by_proxy(toplevel, |w| w.toplevel as usize) else {
                        continue;
                    };
                    if let Ok(window) = self.window_mut(id) {
                        window.close_pending = true;
                    }
                    self.queue
                        .push_back(ShellEvent::CloseRequested { window: id });
                }
                RawEvent::DecorationConfigure { decoration, mode } => {
                    self.decoration_configured(decoration, mode);
                }
                RawEvent::PreferredScale { object, scale } => {
                    self.preferred_scale_changed(object, scale);
                }
                RawEvent::SeatCapabilities { seat, capabilities } => {
                    self.seat_capabilities_changed(seat, capabilities);
                }
                event @ (RawEvent::DataOffer { .. }
                | RawEvent::OfferMime { .. }
                | RawEvent::Selection { .. }
                | RawEvent::DragEnter { .. }
                | RawEvent::DragLeave { .. }
                | RawEvent::DragMotion { .. }
                | RawEvent::DragDrop { .. }
                | RawEvent::SourceSend { .. }
                | RawEvent::SourceCancelled { .. }) => self.process_data(event),
                other => self.process_input(other),
            }
        }
        self.ensure_xdg_outputs();
        self.ensure_data_devices();
        if monitors_dirty {
            self.republish_monitors();
        }
        // A compositor is not obliged to end a burst with a `wl_pointer.frame`
        // before the socket goes quiet — and a version-4 pointer has no frame
        // event at all. Flushing here as well means a motion is never held back
        // to the next pump.
        self.flush_pointer_frames();
    }

    fn output_by_xdg(&mut self, xdg: usize) -> Option<&mut Output> {
        self.outputs
            .iter_mut()
            .find(|output| output.xdg as usize == xdg)
    }

    // -----------------------------------------------------------------------
    // Seats and input
    // -----------------------------------------------------------------------

    /// Adds or removes a seat's devices to match what it now advertises.
    ///
    /// A seat gains and loses capabilities at runtime — plugging in a mouse is
    /// the everyday case, and a headless compositor starts with *neither* a
    /// pointer nor a keyboard — so this is a diff against the previous set, not
    /// a one-time setup.
    fn seat_capabilities_changed(&mut self, proxy: usize, capabilities: u32) {
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
    fn remove_seat(&mut self, index: usize) {
        if !self.seats[index].pointer.is_null() {
            self.remove_pointer(index);
        }
        if !self.seats[index].keyboard.is_null() {
            self.remove_keyboard(index);
        }
        self.remove_data_device(index);
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
    fn process_input(&mut self, event: RawEvent) {
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
        let Some(window) = self.seats[index].keyboard_focus else {
            return;
        };
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

        let seat = &self.seats[index];
        let keysym = seat
            .keymap
            .as_ref()
            .map_or(Keysym::NONE, |keymap| keymap.keysym(key));
        let text = pressed
            .then(|| seat.keymap.as_ref().and_then(|keymap| keymap.text(key)))
            .flatten();
        let event_time = self.time.event_time(time);
        self.queue.push_back(ShellEvent::Key {
            window,
            device: seat.device,
            time: event_time,
            scancode: Scancode(key),
            key_code,
            keysym,
            state: ButtonState::from_pressed(pressed),
            repeat: false,
            modifiers: seat.modifiers,
        });
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
    /// Called from [`pump`](Shell::pump) rather than driven by a timer thread:
    /// the shell is thread-affine, and a repeat that arrived between frames
    /// would have to be queued anyway. The timestamp is the instant the repeat
    /// was *due*, so a slow frame does not smear the interval.
    fn drive_repeats(&mut self) {
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
                let seat = &self.seats[index];
                self.queue.push_back(ShellEvent::Key {
                    window: key.window,
                    device: seat.device,
                    time: self.time.event_time_nanos(key.next_nanos),
                    scancode: Scancode(key.scancode),
                    key_code: key.key_code,
                    keysym: key.keysym,
                    state: ButtonState::Pressed,
                    repeat: true,
                    modifiers: seat.modifiers,
                });
                key.next_nanos = key.next_nanos.saturating_add(key.interval_nanos);
                emitted += 1;
            }
            self.seats[index].repeat.key = Some(key);
        }
    }

    /// How long until the earliest repeat is due, if any is.
    fn next_repeat_in(&self) -> Option<Duration> {
        let now = ffi::monotonic_nanos();
        self.seats
            .iter()
            .filter_map(|seat| seat.repeat.due_at())
            .min()
            .map(|due| Duration::from_nanos(due.saturating_sub(now)))
    }

    fn flush_pointer_frames(&mut self) {
        for index in 0..self.seats.len() {
            self.flush_pointer_frame(index);
        }
    }

    /// Turns one accumulated [`PointerFrame`] into at most one motion and at
    /// most one wheel event.
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
    fn set_focus(&mut self, window: WindowId, focused: bool) {
        let Ok(state) = self.window_mut(window) else {
            return;
        };
        if state.focused == focused {
            return;
        }
        state.focused = focused;
        self.queue.push_back(ShellEvent::Focus { window, focused });
    }

    // -----------------------------------------------------------------------
    // Scale, configure, cursor, constraints
    // -----------------------------------------------------------------------

    /// The monitor a surface is on, when there is exactly one answer.
    ///
    /// `entered` is a list of `wl_output` **proxies** — what
    /// `wl_surface.enter` carries — not indices into
    /// [`outputs`](Self::outputs).
    ///
    /// `None` when the surface is on no output (not mapped yet) or on more
    /// than one (straddling two displays): both are "the backend cannot say
    /// which", which is a different statement from a *request* that did not
    /// care. See [`DisplayMode::satisfied_by`].
    fn monitor_of(&self, entered: &[usize]) -> Option<MonitorId> {
        let [only] = entered else {
            return None;
        };
        self.outputs
            .iter()
            .find(|output| output.proxy == *only)
            .map(|output| output.id)
    }

    fn surface_output_changed(&mut self, surface: usize, output: usize, entered: bool) {
        let Some(id) = self.window_by_proxy(surface, |w| w.surface as usize) else {
            return;
        };
        // The scale of an output the surface is on, resolved before the
        // borrow of the window.
        let scales: Vec<(usize, i32)> = self
            .outputs
            .iter()
            .map(|output| (output.proxy, output.scale))
            .collect();
        let Ok(window) = self.window_mut(id) else {
            return;
        };
        if entered {
            if !window.outputs.contains(&output) {
                window.outputs.push(output);
            }
        } else {
            window.outputs.retain(|candidate| *candidate != output);
        }
        // **A surface can change output without being reconfigured.** A window
        // fullscreened onto another monitor is configured at the new size
        // first; the `wl_surface.enter` for the monitor it landed on arrives
        // afterwards, and no further configure follows it. So the effective
        // mode's monitor is refreshed here as well as in `apply_configure`, or
        // it would be whatever the surface was on at the last configure —
        // which for the move that matters is the monitor it came *from*.
        let entered = window.outputs.clone();
        let monitor = self.monitor_of(&entered);
        if let Ok(window) = self.window_mut(id)
            && let Some(config) = window.configuration.as_mut()
            && config.mode.is_borderless()
        {
            config.mode = DisplayMode::Borderless { monitor };
        }
        let Ok(window) = self.window_mut(id) else {
            return;
        };
        // Without `fractional-scale-v1` the only correct integer scale for a
        // surface spanning several outputs is the largest of them: anything
        // smaller is visibly blurry on the sharpest one.
        let scale = window
            .outputs
            .iter()
            .filter_map(|proxy| {
                scales
                    .iter()
                    .find(|(candidate, _)| candidate == proxy)
                    .map(|(_, scale)| *scale)
            })
            .max()
            .unwrap_or(1)
            .max(1);
        if scale == window.scale {
            return;
        }
        window.scale = scale;
        // `wl_surface.set_buffer_scale` and a `wp_viewport` destination are two
        // ways of saying the same thing, and the viewporter protocol forbids
        // using both. When the compositor is driving the scale fractionally,
        // the viewport is the one in effect.
        if window.preferred_scale.is_none() {
            let surface_proxy = window.surface;
            // SAFETY: the surface is live and `set_buffer_scale` takes one int.
            unsafe { wl_surface::set_buffer_scale(surface_proxy, scale) };
        }
        self.rescale(id);
    }

    /// A `wp_fractional_scale_v1.preferred_scale`.
    fn preferred_scale_changed(&mut self, object: usize, scale: u32) {
        let Some(id) = self.window_by_proxy(object, |w| w.fractional_scale as usize) else {
            return;
        };
        let Ok(window) = self.window_mut(id) else {
            return;
        };
        if window.preferred_scale == Some(scale) {
            return;
        }
        window.preferred_scale = Some(scale);
        self.rescale(id);
    }

    /// Republishes a window's configuration after its scale changed.
    fn rescale(&mut self, id: WindowId) {
        let Ok(window) = self.window_mut(id) else {
            return;
        };
        let Some(config) = window.configuration else {
            // Not configured yet: the scale folds into the first configure
            // rather than being announced on its own.
            return;
        };
        let scale_factor = window.scale_factor();
        if (config.scale_factor - scale_factor).abs() < f64::EPSILON {
            return;
        }
        let logical = window.logical_size();
        let size = logical.to_physical(scale_factor);
        window.configuration = Some(WindowConfiguration {
            size,
            scale_factor,
            mode: config.mode,
        });
        let (surface, viewport) = (window.surface, window.viewport);
        // SAFETY: both proxies are live; a scale change is only in effect after
        // a commit, and the viewport destination is what tells the compositor
        // how large the (physical) buffer should appear.
        unsafe {
            set_viewport(viewport, logical);
            wl_surface::commit(surface);
        }
        self.queue.push_back(ShellEvent::ScaleFactorChanged {
            window: id,
            scale_factor,
            size,
        });
        self.conn.flush();
    }

    /// Completes a configure: acknowledge it, publish the new configuration,
    /// and commit.
    fn apply_configure(&mut self, xdg_surface_proxy: usize, serial: u32) {
        let Some(id) = self.window_by_proxy(xdg_surface_proxy, |w| w.xdg_surface as usize) else {
            return;
        };
        // **Which monitor a fullscreen surface ended up on is observable, even
        // though asking for one is only a hint.** `wl_surface.enter` names the
        // outputs the surface is on, and a fullscreen surface is on exactly
        // one — so the *answer* can name a monitor where the *request* could
        // only suggest it. Without this a request for `Borderless { monitor:
        // Some(second) }` was answered `Borderless { monitor: None }` and
        // `mode_request_honoured` said no, for a window covering exactly the
        // monitor that was asked for.
        //
        // Exactly one, deliberately: a surface straddling two outputs has no
        // single answer, and `None` there means "the backend cannot say", which
        // is what it is. See `DisplayMode::satisfied_by` for how the two
        // meanings of `None` differ.
        let on = {
            let Ok(window) = self.window(id) else {
                return;
            };
            self.monitor_of(window.outputs.as_slice())
        };
        let Ok(window) = self.window_mut(id) else {
            return;
        };

        let fullscreen = window
            .pending
            .states
            .contains(&xdg_toplevel::state::FULLSCREEN);
        let mode = if fullscreen {
            DisplayMode::Borderless { monitor: on }
        } else {
            DisplayMode::Windowed
        };
        let scale_factor = window.scale_factor();
        let logical = window.logical_size();
        // Constraints are hints the compositor may ignore, so they are applied
        // here too — the same thing `SizeConstraints::apply` does for
        // `HeadlessShell`, and the reason the seam has no "effective
        // constraints".
        let size = window
            .requested_constraints
            .apply(logical.to_physical(scale_factor), scale_factor);
        let config = WindowConfiguration {
            size,
            scale_factor,
            mode,
        };
        let changed = window.configuration != Some(config);
        let scale_changed = window
            .configuration
            .is_some_and(|previous| previous.scale_factor != scale_factor);
        window.configuration = Some(config);
        // `xdg_toplevel.state.activated` is a focus signal a compositor sets
        // even before a `wl_seat` has a keyboard. Compare against the previous
        // value rather than emitting on every configure: a compositor restates
        // the full state set every time.
        let focused = window
            .pending
            .states
            .contains(&xdg_toplevel::state::ACTIVATED);
        let xdg = window.xdg_surface;
        let surface = window.surface;
        let viewport = window.viewport;

        // SAFETY: every proxy is live. `ack_configure` must precede the commit,
        // and the commit is what makes the acknowledged state current. The
        // viewport destination is the logical size the compositor just asked
        // for, which is exactly what `wp_viewport` wants.
        unsafe {
            set_viewport(viewport, logical);
            xdg_surface::ack_configure(xdg, serial);
            wl_surface::commit(surface);
        }
        self.conn.flush();

        if changed {
            if scale_changed {
                self.queue.push_back(ShellEvent::ScaleFactorChanged {
                    window: id,
                    scale_factor,
                    size,
                });
            }
            self.queue.push_back(ShellEvent::Resized {
                window: id,
                size,
                scale_factor,
            });
        }
        self.set_focus(id, focused);
    }

    /// Sends the cursor a window asked for on a seat that has just entered it.
    ///
    /// Only the hidden case is expressible; see [`Shell::set_cursor`].
    fn apply_cursor(&mut self, index: usize, window: WindowId) {
        let Ok(state) = self.window(window) else {
            return;
        };
        // `Some(None)` is "hide"; `None` (never set) and `Some(Some(_))` (a
        // shape) both leave the compositor's cursor alone.
        if state.cursor != Some(None) {
            return;
        }
        let seat = &self.seats[index];
        let (pointer, serial) = (seat.pointer, seat.enter_serial);
        if pointer.is_null() {
            return;
        }
        // SAFETY: the pointer is live and a null surface argument is the
        // documented way to say "no cursor" in core Wayland.
        unsafe { wl_pointer::set_cursor(pointer, serial, ptr::null_mut(), 0, 0) };
        self.conn.flush();
    }

    /// Rebuilds every window's pointer constraint against the current seats.
    ///
    /// Called when a mode changes and when a seat gains a pointer, because a
    /// constraint is a per-`(surface, pointer)` object and a pointer that did
    /// not exist yet could not have one.
    fn reapply_constraints(&mut self) {
        let windows: Vec<WindowId> = self
            .windows
            .iter()
            .filter(|(_, window)| window.pointer_mode.is_captured())
            .map(|(handle, _)| handle.cast())
            .collect();
        for window in windows {
            let mode = self
                .window(window)
                .map_or(PointerMode::Free, |w| w.pointer_mode);
            self.rebuild_constraint(window, mode);
        }
    }

    /// Destroys and recreates one window's constraints for `mode`.
    fn rebuild_constraint(&mut self, window: WindowId, mode: PointerMode) {
        let existing = match self.window_mut(window) {
            Ok(state) => {
                state.pointer_mode = mode;
                core::mem::take(&mut state.constraints)
            }
            Err(_) => return,
        };
        for (_, proxy) in existing {
            // SAFETY: each proxy is a live constraint object created below, and
            // the two interfaces share one destructor — see
            // `constraint_destructor`.
            unsafe { self.conn.release(proxy, constraint_destructor()) };
        }
        if mode == PointerMode::Free || self.conn.pointer_constraints.is_null() {
            self.conn.flush();
            return;
        }
        let Ok(surface) = self.window(window).map(|state| state.surface) else {
            return;
        };
        let manager = self.conn.pointer_constraints;
        let pointers: Vec<*mut WlProxy> = self
            .seats
            .iter()
            .map(|seat| seat.pointer)
            .filter(|pointer| !pointer.is_null())
            .collect();
        let mut created = Vec::with_capacity(pointers.len());
        for pointer in pointers {
            // A null region means "the whole surface", and `persistent` means
            // the constraint reactivates whenever the compositor is willing
            // again — the behaviour a game wants across an alt-tab, rather than
            // a one-shot that silently never comes back.
            // SAFETY: the manager, surface and pointer are all live on this
            // connection; a null region is the protocol's own "entire surface".
            let proxy = unsafe {
                match mode {
                    PointerMode::Locked => zwp_pointer_constraints_v1::lock_pointer(
                        manager,
                        surface,
                        pointer,
                        ptr::null_mut(),
                        zwp_pointer_constraints_v1::lifetime::PERSISTENT,
                    ),
                    _ => zwp_pointer_constraints_v1::confine_pointer(
                        manager,
                        surface,
                        pointer,
                        ptr::null_mut(),
                        zwp_pointer_constraints_v1::lifetime::PERSISTENT,
                    ),
                }
            };
            // The `locked`/`unlocked` and `confined`/`unconfined` events say
            // whether the constraint is currently active. Nothing in the seam
            // reports that — `WindowState::pointer_mode` is what was asked for
            // — so they are swallowed rather than left to warn.
            self.conn.watch(proxy, ObjectKind::Ignored);
            created.push((pointer as usize, proxy));
        }
        if let Ok(state) = self.window_mut(window) {
            state.constraints = created;
        }
        self.conn.flush();
    }

    /// Destroys the constraints that name a seat's pointer, because it is going
    /// away.
    ///
    /// Sent as the protocol destructor and sent **first**, before the
    /// `wl_pointer` itself is released: a constraint whose proxy is merely
    /// freed client-side leaves the server-side object alive for the rest of
    /// the session, and on a compositor that keys the lock off the object
    /// rather than the pointer that is a pointer that never comes back.
    ///
    /// Only this seat's constraints go — another seat's mouse is still plugged
    /// in and still locked. The windows keep their
    /// [`PointerMode`], so
    /// [`add_pointer`](Self::add_pointer) restores this seat's when a pointer
    /// reappears.
    fn drop_window_constraints_for_seat(&mut self, index: usize) {
        let pointer = self.seats[index].pointer as usize;
        let windows: Vec<WindowId> = self
            .windows
            .iter()
            .filter(|(_, window)| {
                window
                    .constraints
                    .iter()
                    .any(|(owner, _)| *owner == pointer)
            })
            .map(|(handle, _)| handle.cast())
            .collect();
        for window in windows {
            let doomed: Vec<*mut WlProxy> = match self.window_mut(window) {
                Ok(state) => {
                    let (mine, theirs) = state
                        .constraints
                        .iter()
                        .partition::<Vec<_>, _>(|(owner, _)| *owner == pointer);
                    state.constraints = theirs;
                    mine.into_iter().map(|(_, proxy)| proxy).collect()
                }
                Err(_) => continue,
            };
            for proxy in doomed {
                // SAFETY: each is a live constraint object created by
                // `rebuild_constraint`, destroyed exactly once.
                unsafe { self.conn.release(proxy, constraint_destructor()) };
            }
        }
        self.conn.flush();
    }

    // -----------------------------------------------------------------------
    // Clipboard and drag-and-drop
    // -----------------------------------------------------------------------

    /// The seat whose `wl_data_device` is `proxy`.
    fn data_seat(&self, proxy: usize) -> Option<usize> {
        self.seats.iter().position(|seat| {
            seat.data
                .as_ref()
                .is_some_and(|device| device.proxy as usize == proxy)
        })
    }

    /// The seat whose `wl_data_source` is `proxy`.
    fn source_seat(&self, proxy: usize) -> Option<usize> {
        self.seats.iter().position(|seat| {
            seat.data
                .as_ref()
                .and_then(|device| device.source.as_ref())
                .is_some_and(|source| source.proxy as usize == proxy)
        })
    }

    /// Seats in the order a clipboard operation for `window` should try them.
    ///
    /// The clipboard belongs to a **seat**, and the seam names a *window*. The
    /// bridge is focus: the seat that is typing into this window is the one
    /// whose Ctrl+C this is. Seats that are not focused on it come after, so a
    /// single-seat session — every desktop — behaves identically either way,
    /// and a multi-seat one prefers the right clipboard instead of the first.
    fn seats_for_window(&self, window: WindowId) -> Vec<usize> {
        let mut order: Vec<usize> = (0..self.seats.len())
            .filter(|index| self.seats[*index].data.is_some())
            .collect();
        order.sort_by_key(|index| {
            let seat = &self.seats[*index];
            match (seat.keyboard_focus, seat.pointer_focus) {
                (Some(focused), _) if focused == window => 0,
                (_, Some(hovered)) if hovered == window => 1,
                _ => 2,
            }
        });
        order
    }

    /// What this shell currently knows about the clipboard, for `window` and
    /// `mime`.
    ///
    /// The three-way answer the seam's obligation 5 turns on:
    /// [`Ready`](Resolution::Ready) starts a transfer,
    /// [`Empty`](Resolution::Empty) is an answer, and
    /// [`Unknown`](Resolution::Unknown) means **hold** — the compositor has not
    /// told us what is on the clipboard, which is not the same as it being
    /// empty.
    fn resolve_selection(&self, window: WindowId, mime: MimeType) -> Resolution {
        let mut known = false;
        for index in self.seats_for_window(window) {
            let Some(device) = self.seats[index].data.as_ref() else {
                continue;
            };
            if !device.selection_seen {
                continue;
            }
            known = true;
            if let Some(offer) = device.selection.as_ref()
                && let Some(spelling) = offer.pick(mime)
            {
                return Resolution::Ready {
                    offer: offer.proxy,
                    spelling: spelling.to_string(),
                };
            }
        }
        if known {
            Resolution::Empty
        } else {
            Resolution::Unknown
        }
    }

    /// Queues one [`ShellEvent::ClipboardData`].
    fn answer_read(
        &mut self,
        window: WindowId,
        request: ClipboardRequestId,
        mime: ReceivedMime,
        content: ClipboardContent,
    ) {
        self.queue.push_back(ShellEvent::ClipboardData {
            window,
            request,
            mime,
            content,
        });
    }

    /// Starts, answers or keeps holding every read that is waiting for the
    /// clipboard to become readable.
    ///
    /// Called from [`pump`](Shell::pump) and whenever a `selection` event
    /// arrives, which is the moment a held read most often becomes answerable.
    fn resolve_held_reads(&mut self) {
        if self.held.is_empty() {
            return;
        }
        let now = ffi::monotonic_nanos();
        for read in core::mem::take(&mut self.held) {
            // The window went away while its read was outstanding. Obligation 4
            // is about answering an accepted request; an answer naming a
            // destroyed window would break obligation 1 instead.
            if self.window(read.window).is_err() {
                continue;
            }
            match self.resolve_selection(read.window, read.mime) {
                Resolution::Ready { offer, spelling } => {
                    let delivery = Delivery::Clipboard {
                        window: read.window,
                        request: read.request,
                        mime: ReceivedMime::new(&spelling),
                    };
                    if self.start_receive(offer, &spelling, delivery).is_err() {
                        self.answer_read(
                            read.window,
                            read.request,
                            ReceivedMime::new(&spelling),
                            ClipboardContent::Unavailable,
                        );
                    }
                }
                Resolution::Empty => self.answer_read(
                    read.window,
                    read.request,
                    ReceivedMime::from(read.mime),
                    ClipboardContent::Empty,
                ),
                Resolution::Unknown if now < read.deadline_nanos => self.held.push(read),
                Resolution::Unknown => {
                    // Out of time: no window of ours has had keyboard focus for
                    // the whole wait, so there is nothing to read from and
                    // never was anything to report. `Unavailable`, not `Empty`
                    // — there may well be something on the clipboard.
                    crcbl_core::log::debug!(
                        "{:?}: the clipboard never became readable; \
                         no window of this client had keyboard focus",
                        read.window
                    );
                    self.answer_read(
                        read.window,
                        read.request,
                        ReceivedMime::from(read.mime),
                        ClipboardContent::Unavailable,
                    );
                }
            }
        }
    }

    /// Sends a `wl_data_offer` destructor, if the offer still has a proxy.
    fn destroy_offer(&mut self, offer: &data::Offer) {
        if offer.proxy.is_null() {
            return;
        }
        // SAFETY: the proxy is a live `wl_data_offer` created by the compositor
        // on this connection, and `destroy` is that interface's own destructor.
        unsafe { self.conn.release(offer.proxy, wl_data_offer::destroy) };
    }

    /// Routes one `data-device` event.
    ///
    /// Every path that can see a [`RawEvent::SourceSend`] either adopts its
    /// descriptor or closes it; see that variant.
    #[expect(
        clippy::too_many_lines,
        reason = "the data-device routing table; see process_raw"
    )]
    fn process_data(&mut self, event: RawEvent) {
        match event {
            RawEvent::DataOffer { device, offer } => {
                let proxy = offer as *mut WlProxy;
                let Some(index) = self.data_seat(device) else {
                    // The seat went away between the event and the drain.
                    self.destroy_offer(&data::Offer::new(proxy));
                    return;
                };
                if let Some(device) = self.seats[index].data.as_mut() {
                    let evicted = device.push_incoming(data::Offer::new(proxy));
                    if let Some(evicted) = evicted {
                        self.destroy_offer(&evicted);
                    }
                }
            }
            RawEvent::OfferMime { offer, mime } => {
                for seat in &mut self.seats {
                    if let Some(device) = seat.data.as_mut()
                        && let Some(offer) = device.announced_mut(offer)
                    {
                        offer.note_mime(mime);
                        return;
                    }
                }
            }
            RawEvent::Selection { device, offer } => {
                let Some(index) = self.data_seat(device) else {
                    return;
                };
                // "The client must destroy the previous selection data_offer,
                // if any, upon receiving this event" — and the *new* offer was
                // announced first, so both are in hand at once.
                let (previous, claimed) = match self.seats[index].data.as_mut() {
                    Some(device) => (device.selection.take(), device.claim(offer)),
                    None => return,
                };
                if let Some(previous) = previous {
                    self.destroy_offer(&previous);
                }
                if let Some(device) = self.seats[index].data.as_mut() {
                    device.selection = claimed;
                    // Including for the null offer: "the clipboard was cleared"
                    // is knowledge, and it is what lets a read be answered
                    // `Empty` rather than held.
                    device.selection_seen = true;
                }
                // A read that was waiting for exactly this can now be answered.
                self.resolve_held_reads();
            }
            RawEvent::DragEnter {
                device,
                serial,
                surface,
                x,
                y,
                offer,
            } => self.drag_entered(device, serial, surface, x, y, offer),
            RawEvent::DragMotion {
                device, x, y, time, ..
            } => {
                let _ = time;
                let Some(index) = self.data_seat(device) else {
                    return;
                };
                let scale = self.seats[index]
                    .data
                    .as_ref()
                    .and_then(|device| device.drag.as_ref())
                    .and_then(|drag| drag.window)
                    .and_then(|window| self.window(window).ok())
                    .map_or(1.0, WlWindow::scale_factor);
                if let Some(drag) = self.seats[index]
                    .data
                    .as_mut()
                    .and_then(|device| device.drag.as_mut())
                {
                    drag.position = PhysicalPoint::new(
                        keymap::fixed_to_f64(x) * scale,
                        keymap::fixed_to_f64(y) * scale,
                    );
                }
            }
            RawEvent::DragLeave { device } => {
                let Some(index) = self.data_seat(device) else {
                    return;
                };
                // `None` when the drag already became a transfer at `drop`; a
                // compositor is entitled to send `leave` after one.
                let drag = self.seats[index]
                    .data
                    .as_mut()
                    .and_then(|device| device.drag.take());
                if let Some(drag) = drag {
                    self.destroy_offer(&drag.offer);
                }
            }
            RawEvent::DragDrop { device } => self.drag_dropped(device),
            RawEvent::SourceSend { source, mime, fd } => {
                let Some(file) = fd::adopt(fd) else { return };
                let Some(index) = self.source_seat(source) else {
                    // Our source is already gone; closing the descriptor is the
                    // end-of-file that tells the peer there is nothing coming.
                    return;
                };
                let bytes = self.seats[index]
                    .data
                    .as_ref()
                    .and_then(|device| device.source.as_ref())
                    .and_then(|source| source.bytes_for(&mime))
                    .map(<[u8]>::to_vec);
                let Some(bytes) = bytes else {
                    crcbl_core::log::debug!(
                        "a peer asked for {mime}, which this selection does not offer"
                    );
                    return;
                };
                // Never written here: the peer may be reading slowly, or may be
                // *this process* pasting its own selection, in which case the
                // read that would drain the pipe cannot happen until this
                // function has returned. See `fd`.
                self.writes
                    .push(fd::Writing::new(file, bytes, ffi::monotonic_nanos()));
                self.service_transfers();
            }
            RawEvent::SourceCancelled { source } => {
                let Some(index) = self.source_seat(source) else {
                    return;
                };
                // Another client took the selection. The source is dead and the
                // protocol says to destroy it; the bytes go with it, because
                // nothing will ever ask for them again.
                self.clear_source(index);
            }
            _ => {}
        }
    }

    /// Tells a drag source that nothing here suits us.
    ///
    /// `wl_data_offer.accept` with a **null** mime type is the protocol's "no",
    /// and it has to be sent even for an offer this backend is not tracking: a
    /// drag that gets no answer at all leaves the source's cursor promising a
    /// drop that will never happen.
    fn refuse_drag(&mut self, proxy: *mut WlProxy, serial: u32) {
        if proxy.is_null() {
            return;
        }
        // SAFETY: `proxy` is the `wl_data_offer` the compositor created and
        // named in the `enter` event being handled, so it is live; `accept`
        // takes a nullable string.
        unsafe { wl_data_offer::accept(proxy, serial, None) };
        self.conn.flush();
    }

    /// A drag arrived over one of our surfaces.
    fn drag_entered(
        &mut self,
        device: usize,
        serial: u32,
        surface: usize,
        x: i32,
        y: i32,
        offer: usize,
    ) {
        let proxy = offer as *mut WlProxy;
        let Some(index) = self.data_seat(device) else {
            self.refuse_drag(proxy, serial);
            // Refusing is not enough: the offer was announced for this `enter`,
            // but with the seat gone it can never be claimed, so nothing will
            // ever destroy it but this.
            self.destroy_offer(&data::Offer::new(proxy));
            return;
        };
        let Some(offer) = self.seats[index]
            .data
            .as_mut()
            .and_then(|device| device.claim(offer))
        else {
            self.refuse_drag(proxy, serial);
            // Same shape as the seat-gone path: the `enter` named an offer that
            // was never announced, so it is unclaimable and refusing alone
            // would leak the proxy.
            self.destroy_offer(&data::Offer::new(proxy));
            return;
        };
        // A window that did not ask for drops is not a drop target: the source
        // is told we accept nothing, which is what makes its cursor say so, and
        // the payload is never read. `WindowDesc::accept_drops` is opt-in
        // precisely so a game does not show a drop cursor it will ignore.
        let target = self
            .window_by_proxy(surface, |window| window.surface as usize)
            .filter(|window| self.window(*window).is_ok_and(|state| state.accept_drops));
        let scale = target
            .and_then(|window| self.window(window).ok())
            .map_or(1.0, WlWindow::scale_factor);
        // `text/uri-list` is the only drag format this seam can express: a
        // `DroppedFile` carries a path. A drag of text or of an image is
        // refused rather than half-accepted.
        let mime = target
            .and(offer.pick(MimeType::UriList))
            .map(str::to_string);

        if offer.proxy.is_null() {
            // Nothing to answer on. The null check used to guard only the
            // version query, and then `accept` and `set_actions` dereferenced
            // the same pointer two lines later — the generated wrappers read
            // the proxy before marshalling anything.
            return;
        }
        // SAFETY: the offer proxy is live on this connection.
        let version = unsafe { ffi::proxy_version(offer.proxy) };
        let encoded = mime.as_deref().and_then(|mime| CString::new(mime).ok());
        // SAFETY: the offer is live (checked just above); `accept` takes a
        // nullable string, and a null one is the protocol's own way of saying
        // "nothing here suits me".
        unsafe {
            wl_data_offer::accept(offer.proxy, serial, encoded.as_deref());
            if version >= 3 {
                // Only `copy`: see `data::ACTION_COPY`. A source that offers
                // only `move` gets no action, which is a refused drop rather
                // than a promise this engine cannot keep.
                let action = if encoded.is_some() {
                    data::ACTION_COPY
                } else {
                    0
                };
                wl_data_offer::set_actions(offer.proxy, action, action);
            }
        }
        self.conn.flush();

        // A second `enter` without `leave` violates the protocol, but it must
        // not leak: the previous drag's offer is destroyed before being
        // replaced, exactly as `leave` would have done.
        let previous = self.seats[index]
            .data
            .as_mut()
            .and_then(|device| device.drag.take());
        if let Some(previous) = previous {
            self.destroy_offer(&previous.offer);
        }

        if let Some(device) = self.seats[index].data.as_mut() {
            device.drag = Some(data::Drag {
                offer,
                window: target,
                position: PhysicalPoint::new(
                    keymap::fixed_to_f64(x) * scale,
                    keymap::fixed_to_f64(y) * scale,
                ),
                mime,
            });
        }
    }

    /// The drag was released. Starts the read that becomes
    /// [`ShellEvent::DroppedFile`].
    fn drag_dropped(&mut self, device: usize) {
        let Some(index) = self.data_seat(device) else {
            return;
        };
        let Some(drag) = self.seats[index]
            .data
            .as_mut()
            .and_then(|device| device.drag.take())
        else {
            return;
        };
        let (Some(window), Some(mime)) = (drag.window, drag.mime.clone()) else {
            // Refused on `enter`, so there is nothing to read and the offer is
            // ours to dispose of.
            self.destroy_offer(&drag.offer);
            return;
        };
        let delivery = Delivery::Drop {
            window,
            position: drag.position,
            device,
            offer: drag.offer.proxy as usize,
        };
        if self
            .start_receive(drag.offer.proxy, &mime, delivery)
            .is_err()
        {
            self.destroy_offer(&drag.offer);
        }
    }

    /// Asks a peer for `mime` over a fresh pipe, and records what the bytes
    /// become.
    ///
    /// The pipe's write end goes to the peer and is dropped here immediately:
    /// the descriptor is duplicated across the socket by the compositor, and a
    /// client that kept its own copy open would keep the pipe from ever
    /// reaching end-of-file — which is the only signal that a transfer is
    /// complete.
    fn start_receive(
        &mut self,
        offer: *mut WlProxy,
        mime: &str,
        delivery: Delivery,
    ) -> Result<(), ShellError> {
        let encoded = CString::new(mime)
            .map_err(|_| ShellError::Backend("a mime type with a NUL byte".to_string()))?;
        let (reader, writer) = std::io::pipe()
            .map_err(|error| ShellError::Backend(format!("cannot create a pipe: {error}")))?;
        // SAFETY: the offer is live on this connection, and the descriptor is
        // valid for the duration of the call — which is all `receive` needs,
        // since the compositor duplicates it out of the message.
        unsafe { wl_data_offer::receive(offer, &encoded, writer.as_raw_fd()) };
        self.conn.flush();
        drop(writer);
        self.transfers.push(Transfer {
            reading: fd::Reading::new(
                std::fs::File::from(std::os::fd::OwnedFd::from(reader)),
                ffi::monotonic_nanos(),
            ),
            delivery,
        });
        Ok(())
    }

    /// Destroys the selection source this seat published, if any.
    fn clear_source(&mut self, index: usize) {
        let source = self.seats[index]
            .data
            .as_mut()
            .and_then(|device| device.source.take());
        let Some(source) = source else { return };
        if source.proxy.is_null() {
            return;
        }
        // SAFETY: the proxy is a live `wl_data_source` this backend created,
        // and `destroy` is its own destructor.
        unsafe { self.conn.release(source.proxy, wl_data_source::destroy) };
    }

    /// Tears down one seat's `wl_data_device` and everything hanging off it.
    ///
    /// Order matters: the offers and the source are objects the *device* owns,
    /// so they go first, exactly as a pointer's constraints go before the
    /// pointer.
    fn remove_data_device(&mut self, index: usize) {
        let Some(device) = self.seats[index].data.take() else {
            return;
        };
        // A drop whose device is going away can never be finished, and its
        // offer would outlive the device that owns it. Both end here, while the
        // device is still alive to destroy the offer against.
        let doomed: Vec<usize> = self
            .transfers
            .iter()
            .filter(|transfer| {
                matches!(transfer.delivery, Delivery::Drop { device: owner, .. }
                    if owner == device.proxy as usize)
            })
            .filter_map(|transfer| transfer.delivery.drop_offer())
            .collect();
        self.transfers.retain(|transfer| {
            !matches!(transfer.delivery, Delivery::Drop { device: owner, .. }
                if owner == device.proxy as usize)
        });
        let offers = device
            .incoming
            .iter()
            .chain(device.selection.iter())
            .chain(device.drag.as_ref().map(|drag| &drag.offer))
            .map(|offer| offer.proxy)
            .chain(doomed.into_iter().map(|offer| offer as *mut WlProxy))
            .collect::<Vec<_>>();
        for proxy in offers {
            self.destroy_offer(&data::Offer::new(proxy));
        }
        if let Some(source) = device.source.as_ref() {
            // SAFETY: a live `wl_data_source` this backend created.
            unsafe { self.conn.release(source.proxy, wl_data_source::destroy) };
        }
        // SAFETY: the device is live and `release` is `wl_data_device`'s own
        // destructor — which arrived in version 2, so an older one is dropped
        // client-side instead.
        unsafe {
            self.conn
                .release_since(device.proxy, 2, wl_data_device::release);
        }
        self.conn.flush();
    }

    /// The descriptors a blocking wait should include; see [`Conn::drain`].
    ///
    /// **Reads only.** A pending write is nearly always writable — that is what
    /// "pending" means for one whose peer is merely slow — so putting it in the
    /// poll would make every wait return immediately and turn an idle editor
    /// into a spin loop for the length of the transfer. Writes are covered by
    /// [`TRANSFER_POLL`] instead, which is a bound on the sleep rather than an
    /// invitation to skip it.
    fn transfer_fds(&self) -> Vec<c_int> {
        self.transfers
            .iter()
            .map(|transfer| transfer.reading.raw_fd())
            .collect()
    }

    /// Moves every transfer as far as it will go without blocking, and turns
    /// the finished ones into events.
    ///
    /// # Why this is a loop
    ///
    /// The two ends of a transfer can both be this process — pasting our own
    /// selection is the ordinary case, and it is the one that deadlocks a
    /// backend that reads on the event-loop thread. Writing fills the pipe,
    /// reading empties it, and neither can finish alone, so the pass repeats
    /// while *anything* moved. It terminates because every iteration that
    /// repeats has moved bytes and no payload is infinite.
    fn service_transfers(&mut self) {
        while !self.transfers.is_empty() || !self.writes.is_empty() {
            let now = ffi::monotonic_nanos();
            let mut moved = false;

            // Writes first: a read of our own selection cannot progress until
            // the bytes it is waiting for have been put in the pipe.
            let mut index = 0;
            while index < self.writes.len() {
                let (state, progressed) = self.writes[index].service(now);
                moved |= progressed;
                if state == fd::State::Pending {
                    index += 1;
                } else {
                    // Dropping the writer closes the descriptor, which is the
                    // end-of-file the peer reads as "that was all".
                    self.writes.remove(index);
                }
            }

            let mut finished = Vec::new();
            let mut index = 0;
            while index < self.transfers.len() {
                let (state, progressed) = self.transfers[index].reading.service(now);
                moved |= progressed;
                match state {
                    fd::State::Pending => index += 1,
                    fd::State::Done => finished.push((self.transfers.remove(index), true)),
                    fd::State::Failed => finished.push((self.transfers.remove(index), false)),
                }
            }
            for (transfer, complete) in finished {
                self.deliver(transfer, complete);
            }
            if !moved {
                break;
            }
        }
    }

    /// Turns one finished transfer into the events it promised.
    fn deliver(&mut self, transfer: Transfer, complete: bool) {
        let Transfer { reading, delivery } = transfer;
        let bytes = reading.take();
        match delivery {
            Delivery::Clipboard {
                window,
                request,
                mime,
            } => {
                // A transfer that completed reports its bytes even when there
                // are none of them: a peer is entitled to publish an empty
                // selection, and `Bytes(vec![])` says the read worked. One that
                // broke — the peer went away mid-write, or never wrote at all —
                // is `Unavailable`, which is emphatically not `Empty`: there
                // *is* something on the clipboard, we just could not get it.
                let content = if complete {
                    ClipboardContent::Bytes(bytes)
                } else {
                    ClipboardContent::Unavailable
                };
                self.answer_read(window, request, mime, content);
            }
            Delivery::Drop {
                window,
                position,
                offer,
                ..
            } => {
                let offer = offer as *mut WlProxy;
                if complete {
                    let time = self.time.event_time_now();
                    // One event per file, which is what `DroppedFile`
                    // specifies; a URI that is not a local path produces none.
                    for path in crate::parse_uri_list(&bytes) {
                        self.queue.push_back(ShellEvent::DroppedFile {
                            window,
                            time,
                            path,
                            position: Some(position),
                        });
                    }
                }
                // SAFETY: the offer is live — nothing destroys it between the
                // `drop` event and here — and both requests belong to its
                // interface.
                unsafe {
                    // `finish` tells the source the transfer is over so it can
                    // release its own copy. It arrived with the actions in
                    // version 3, and calling it on an older offer is a protocol
                    // error that disconnects the client.
                    if !offer.is_null() && ffi::proxy_version(offer) >= 3 && complete {
                        wl_data_offer::finish(offer);
                    }
                    self.conn.release(offer, wl_data_offer::destroy);
                }
                self.conn.flush();
            }
        }
    }

    /// Tears one window's objects down, innermost first.
    fn destroy_objects(&mut self, id: WindowId, window: &WlWindow) {
        for (_, proxy) in &window.constraints {
            // SAFETY: each is a live constraint object; see `rebuild_constraint`.
            unsafe { self.conn.release(*proxy, constraint_destructor()) };
        }
        // SAFETY: each proxy is live and each destructor is its own interface's.
        unsafe {
            self.conn
                .release(window.decoration, zxdg_toplevel_decoration_v1::destroy);
            self.conn
                .release(window.fractional_scale, wp_fractional_scale_v1::destroy);
            self.conn.release(window.viewport, wp_viewport::destroy);
        }
        // Order matters: `xdg_toplevel` before `xdg_surface` before
        // `wl_surface`. Destroying a `wl_surface` that still has a role object
        // is a protocol error and disconnects the client.
        self.conn.destroy(window.toplevel);
        self.conn.destroy(window.xdg_surface);
        self.conn.destroy(window.surface);
        // A seat pointing at a window that no longer exists would keep
        // reporting motion into a stale handle. Only *this* window's focus is
        // cleared: destroying one window must not make a seat forget that it is
        // pointing at another.
        // A transfer whose answer names a window that no longer exists would
        // deliver an event with a stale handle, which the seam forbids. The
        // descriptors close with the transfer, and a drop's offer — which
        // nothing else owns once the drag became a transfer — is destroyed
        // rather than leaked.
        let doomed: Vec<usize> = self
            .transfers
            .iter()
            .filter(|transfer| transfer.delivery.window() == id)
            .filter_map(|transfer| transfer.delivery.drop_offer())
            .collect();
        self.transfers
            .retain(|transfer| transfer.delivery.window() != id);
        for offer in doomed {
            self.destroy_offer(&data::Offer::new(offer as *mut WlProxy));
        }
        let drags: Vec<data::Offer> = self
            .seats
            .iter_mut()
            .filter_map(|seat| {
                let device = seat.data.as_mut()?;
                let over_this_window = device
                    .drag
                    .as_ref()
                    .is_some_and(|drag| drag.window == Some(id));
                over_this_window.then(|| device.drag.take()).flatten()
            })
            .map(|drag| drag.offer)
            .collect();
        for offer in &drags {
            self.destroy_offer(offer);
        }
        for seat in &mut self.seats {
            if seat.pointer_focus == Some(id) {
                seat.pointer_focus = None;
                seat.frame = PointerFrame::default();
            }
            if seat.keyboard_focus == Some(id) {
                seat.keyboard_focus = None;
                seat.held.clear();
            }
            if seat.repeat.key.is_some_and(|key| key.window == id) {
                seat.repeat.key = None;
            }
        }
        self.conn.flush();
    }
}

/// The destructor request shared by both constraint interfaces.
///
/// `zwp_locked_pointer_v1.destroy` and `zwp_confined_pointer_v1.destroy` are
/// opcode 0 on both interfaces, take no arguments, and are the only requests
/// this backend ever sends on either — so one function genuinely serves both,
/// and which of the two names it is spelled with is a documentation choice, not
/// a dispatch. Nothing here can dispatch on the kind anyway:
/// [`WlWindow::constraints`] records the pointer and the proxy, not which
/// interface the proxy is. This used to carry a
/// `let _ = zwp_confined_pointer_v1::REQ_DESTROY;` that did nothing but make
/// the call sites' "named per kind" comments look substantiated.
const fn constraint_destructor() -> unsafe fn(*mut WlProxy) {
    zwp_locked_pointer_v1::destroy
}

/// `wl_pointer.axis`'s enum as an index into a `[vertical, horizontal]` pair.
const fn axis_slot(axis: u32) -> Option<usize> {
    match axis {
        wl_pointer::axis::VERTICAL_SCROLL => Some(0),
        wl_pointer::axis::HORIZONTAL_SCROLL => Some(1),
        _ => None,
    }
}

/// Sets a viewport's destination to a logical size, if there is a viewport.
///
/// # Safety
///
/// `viewport` must be null or a live `wp_viewport`.
unsafe fn set_viewport(viewport: *mut WlProxy, logical: LogicalSize) {
    if viewport.is_null() {
        return;
    }
    // `wp_viewport` rejects a non-positive destination with `bad_value`, and a
    // window can legitimately be asked to be zero-sized before its first real
    // configure.
    let round = |value: f64| {
        let rounded = value.round();
        if rounded.is_finite() && rounded >= 1.0 {
            #[expect(
                clippy::cast_possible_truncation,
                reason = "clamped to i32::MAX on the line above"
            )]
            let clamped = rounded.min(f64::from(i32::MAX)) as i32;
            clamped
        } else {
            1
        }
    };
    // SAFETY: the caller guarantees the proxy is live; both arguments are
    // positive integers, which is what the protocol requires.
    unsafe { wp_viewport::set_destination(viewport, round(logical.width), round(logical.height)) };
}

impl Drop for WaylandShell {
    fn drop(&mut self) {
        let windows: Vec<WindowId> = self
            .windows
            .iter()
            .map(|(handle, _)| handle.cast())
            .collect();
        for id in windows {
            if let Some(window) = self.windows.remove(id.cast()) {
                self.destroy_objects(id, &window);
            }
        }
        for index in (0..self.seats.len()).rev() {
            self.remove_seat(index);
        }
        let outputs: Vec<(*mut WlProxy, *mut WlProxy)> = self
            .outputs
            .iter()
            .map(|output| (output.xdg, output.proxy as *mut WlProxy))
            .collect();
        for (xdg, output) in outputs {
            // SAFETY: both proxies are live and each destructor is its own
            // interface's; see the hotplug path for why these are protocol
            // destructors rather than `Conn::destroy`.
            unsafe {
                self.conn.release(xdg, zxdg_output_v1::destroy);
                self.conn.release_since(output, 3, wl_output::release);
            }
        }
        let globals = [
            self.conn.data_device_manager,
            self.conn.pointer_constraints,
            self.conn.relative_pointer_manager,
            self.conn.fractional_scale_manager,
            self.conn.viewporter,
            self.conn.decoration_manager,
            self.conn.xdg_output_manager,
            self.conn.wm_base,
            self.conn.compositor,
            self.conn.registry,
        ];
        for global in globals {
            self.conn.destroy(global);
        }
        self.conn.flush();
        // SAFETY: the display was returned live by `wl_display_connect` and no
        // proxy on it survives — every window, seat and output was destroyed
        // above, and the globals with them.
        unsafe { (self.conn.lib.display_disconnect)(self.conn.display.as_ptr()) };
        // SAFETY: `sink` came from `Box::into_raw` in `open`, is freed exactly
        // once here, and no proxy that could dispatch into it is alive.
        drop(unsafe { Box::from_raw(self.conn.sink) });
    }
}

impl Shell for WaylandShell {
    fn backend(&self) -> ShellBackend {
        ShellBackend::Wayland
    }

    /// What the compositor and this build between them can actually do.
    ///
    /// Latched at connection time by [`latch_caps`](Self::latch_caps) — a bit
    /// here is a promise to every consumer that checked it at startup, so it
    /// never changes afterwards even though a Wayland global can appear
    /// mid-session.
    ///
    /// Two bits are absent permanently rather than pending:
    ///
    /// * [`POINTER_WARP`](ShellCaps::POINTER_WARP) — Wayland has no request to
    ///   move the cursor and will not grow one, because a client that can move
    ///   the cursor can spoof clicks.
    /// * [`WINDOW_POSITION`](ShellCaps::WINDOW_POSITION) — `xdg_output` now
    ///   gives a real *monitor* layout, but the bit is about *windows*, and a
    ///   Wayland client is neither told where its window is nor allowed to ask
    ///   to be moved. `xdg-shell` has no equivalent of `XMoveWindow` by design.
    ///
    /// [`ASPECT_HINT_HONORED`](ShellCaps::ASPECT_HINT_HONORED) is also absent
    /// permanently: `xdg_toplevel` has min and max size and no aspect hint, so
    /// the renderer letterboxes.
    ///
    /// [`CLIPBOARD`](ShellCaps::CLIPBOARD) and
    /// [`DRAG_DROP`](ShellCaps::DRAG_DROP) are one bit's worth of information
    /// on this platform — both come from `wl_data_device_manager` — and are
    /// set together or not at all.
    fn caps(&self) -> ShellCaps {
        self.caps
    }

    fn create_window(&mut self, desc: &WindowDesc<'_>) -> Result<WindowId, ShellError> {
        if let DisplayMode::Borderless {
            monitor: Some(monitor),
        } = desc.mode
            && self.monitor(monitor).is_none()
        {
            return Err(ShellError::NoSuchMonitor(monitor.0));
        }
        let title = CString::new(desc.title)
            .map_err(|_| ShellError::InvalidDescriptor("title contains a NUL byte".to_string()))?;
        let app_id = CString::new(desc.app_id)
            .map_err(|_| ShellError::InvalidDescriptor("app_id contains a NUL byte".to_string()))?;

        // SAFETY: the compositor and wm_base globals are live for the shell's
        // lifetime, checked non-null in `open`. Each request creates the next
        // object in the stack, and every pointer is checked before use.
        let (surface, xdg, toplevel) = unsafe {
            let surface = wl_compositor::create_surface(self.conn.compositor);
            if surface.is_null() {
                return Err(ShellError::WindowCreation(
                    "wl_compositor.create_surface failed".to_string(),
                ));
            }
            let xdg = xdg_wm_base::get_xdg_surface(self.conn.wm_base, surface);
            if xdg.is_null() {
                self.conn.destroy(surface);
                return Err(ShellError::WindowCreation(
                    "xdg_wm_base.get_xdg_surface failed".to_string(),
                ));
            }
            let toplevel = xdg_surface::get_toplevel(xdg);
            if toplevel.is_null() {
                self.conn.destroy(xdg);
                self.conn.destroy(surface);
                return Err(ShellError::WindowCreation(
                    "xdg_surface.get_toplevel failed".to_string(),
                ));
            }
            (surface, xdg, toplevel)
        };
        self.conn.watch(surface, ObjectKind::Surface);
        self.conn.watch(xdg, ObjectKind::XdgSurface);
        self.conn.watch(toplevel, ObjectKind::XdgToplevel);

        // Every add-on object has to exist before the initial commit:
        // `xdg-decoration` says so explicitly, and a `wp_viewport` or
        // `wp_fractional_scale_v1` created afterwards would miss the first
        // configure — which is the one a swapchain is built from.
        // SAFETY: every manager is either null (checked inside each helper) or
        // a live global, and `surface`/`toplevel` were just created.
        let (decoration, fractional_scale, viewport) = unsafe {
            (
                optional(self.conn.decoration_manager, |manager| {
                    zxdg_decoration_manager_v1::get_toplevel_decoration(manager, toplevel)
                }),
                optional(self.conn.fractional_scale_manager, |manager| {
                    wp_fractional_scale_manager_v1::get_fractional_scale(manager, surface)
                }),
                optional(self.conn.viewporter, |manager| {
                    wp_viewporter::get_viewport(manager, surface)
                }),
            )
        };
        self.conn.watch(decoration, ObjectKind::Decoration);
        self.conn
            .watch(fractional_scale, ObjectKind::FractionalScale);
        // `wp_viewport` has no events.
        self.conn.watch(viewport, ObjectKind::Ignored);

        let fullscreen_output = match desc.mode {
            DisplayMode::Windowed => None,
            DisplayMode::Borderless { monitor } => Some(self.output_proxy_for(monitor)),
        };

        // SAFETY: every proxy above is live on this connection, and the strings
        // outlive the marshalling call.
        unsafe {
            xdg_toplevel::set_title(toplevel, &title);
            xdg_toplevel::set_app_id(toplevel, &app_id);
            apply_constraints(toplevel, desc.constraints, desc.resizable, desc.size);
            if !decoration.is_null() {
                // Always ask for server side. The seam has no per-window
                // decoration request, the engine has no renderer to draw
                // client-side decorations with until P1, and a compositor that
                // only does CSD answers `client_side` — which is then a known
                // state in `WindowState`, not a surprise.
                zxdg_toplevel_decoration_v1::set_mode(
                    decoration,
                    zxdg_toplevel_decoration_v1::mode::SERVER_SIDE,
                );
            }
            if let Some(output) = fullscreen_output {
                xdg_toplevel::set_fullscreen(toplevel, output);
            }
            // The initial commit with **no buffer**: xdg-shell requires it, and
            // it is what makes the compositor answer with the first configure.
            wl_surface::commit(surface);
        }
        self.conn.flush();

        let handle: WindowId = self
            .windows
            .insert(WlWindow {
                surface,
                xdg_surface: xdg,
                toplevel,
                decoration,
                fractional_scale,
                viewport,
                title: desc.title.to_string(),
                requested_size: desc.size,
                requested_mode: desc.mode,
                requested_constraints: desc.constraints,
                resizable: desc.resizable,
                // Unconfigured, and it stays that way until the compositor
                // answers — a round trip away. This is the contract P0.4 was
                // shaped around and the reason it was shaped that way.
                configuration: None,
                pending: PendingConfigure::default(),
                outputs: Vec::new(),
                scale: 1,
                preferred_scale: None,
                pointer_mode: PointerMode::Free,
                constraints: Vec::new(),
                cursor: None,
                accept_drops: desc.accept_drops,
                focused: false,
                visible: desc.visible,
                close_pending: false,
            })
            .cast();
        Ok(handle)
    }

    fn destroy_window(&mut self, window: WindowId) -> Result<(), ShellError> {
        let removed = self
            .windows
            .remove(window.cast())
            .ok_or_else(|| ShellError::invalid_window(window))?;
        self.destroy_objects(window, &removed);
        self.queue.push_back(ShellEvent::WindowDestroyed { window });
        Ok(())
    }

    fn window_state(&self, window: WindowId) -> Result<WindowState, ShellError> {
        let state = self.window(window)?;
        Ok(WindowState {
            configuration: state.configuration,
            requested_mode: state.requested_mode,
            requested_constraints: state.requested_constraints,
            focused: state.focused,
            visible: state.visible,
            pointer_mode: state.pointer_mode,
            close_pending: state.close_pending,
        })
    }

    fn set_title(&mut self, window: WindowId, title: &str) -> Result<(), ShellError> {
        let encoded = CString::new(title)
            .map_err(|_| ShellError::InvalidDescriptor("title contains a NUL byte".to_string()))?;
        let state = self.window_mut(window)?;
        state.title = title.to_string();
        let toplevel = state.toplevel;
        // SAFETY: the toplevel is live and the string outlives the call.
        unsafe { xdg_toplevel::set_title(toplevel, &encoded) };
        self.conn.flush();
        Ok(())
    }

    /// Records the requested visibility. **Intent only, and it will stay that
    /// way until P1.**
    ///
    /// Wayland has no hide request: a surface is mapped exactly while it has a
    /// buffer. Hiding is therefore `wl_surface.attach(null)` — which this
    /// backend *could* send today — and showing is attaching a real buffer,
    /// which it cannot, because buffers are the renderer's and the renderer is
    /// P1.
    ///
    /// Implementing only the direction that works would be worse than
    /// implementing neither: `xdg-shell` says an unmapped toplevel returns to
    /// its initial state and has to redo the whole configure handshake before
    /// it can be shown again, so a window hidden through this call would be a
    /// window that can never be shown again. So this records the intent,
    /// [`WindowState::visible`] reports it — matching
    /// [`HeadlessShell`](crate::HeadlessShell) — and the pair closes when the
    /// swapchain lands.
    fn set_visible(&mut self, window: WindowId, visible: bool) -> Result<(), ShellError> {
        self.window_mut(window)?.visible = visible;
        Ok(())
    }

    fn set_mode(&mut self, window: WindowId, mode: DisplayMode) -> Result<(), ShellError> {
        let output = match mode {
            DisplayMode::Windowed => None,
            DisplayMode::Borderless { monitor } => {
                if let Some(id) = monitor
                    && self.monitor(id).is_none()
                {
                    return Err(ShellError::NoSuchMonitor(id.0));
                }
                Some(self.output_proxy_for(monitor))
            }
        };
        let state = self.window_mut(window)?;
        // Recorded immediately because we said it; the *effective* mode changes
        // only when a configure arrives, and may never match.
        state.requested_mode = mode;
        let toplevel = state.toplevel;
        let surface = state.surface;
        // SAFETY: both proxies are live; a null output means "the compositor
        // chooses", which is the only thing a Wayland client may ask for.
        unsafe {
            match output {
                Some(output) => xdg_toplevel::set_fullscreen(toplevel, output),
                None => xdg_toplevel::unset_fullscreen(toplevel),
            }
            wl_surface::commit(surface);
        }
        self.conn.flush();
        Ok(())
    }

    fn set_constraints(
        &mut self,
        window: WindowId,
        constraints: SizeConstraints,
    ) -> Result<(), ShellError> {
        let state = self.window_mut(window)?;
        state.requested_constraints = constraints;
        let toplevel = state.toplevel;
        let surface = state.surface;
        let requested = state.requested_size;
        let resizable = state.resizable;
        // SAFETY: both proxies are live; the sizes are plain integers.
        unsafe {
            apply_constraints(toplevel, constraints, resizable, requested);
            wl_surface::commit(surface);
        }
        self.conn.flush();
        Ok(())
    }

    fn monitors(&self) -> &[MonitorInfo] {
        &self.monitors
    }

    /// Drains the socket without blocking, then delivers what arrived.
    ///
    /// The sequence is libwayland's `prepare_read`/`read_events` protocol
    /// rather than `wl_display_dispatch`, for one reason:
    /// `wl_display_dispatch` **blocks** when nothing is queued, and
    /// [`Shell::pump`] promises to be finite and non-blocking. `prepare_read`
    /// plus a zero-timeout `poll` gives the same delivery with a decision point
    /// in the middle — and it is the same sequence [`wait_events`](Shell::wait_events)
    /// runs with a real timeout, so the blocking and non-blocking paths cannot
    /// drift apart.
    ///
    /// Key repeats are generated here, after the socket and before delivery, so
    /// that a repeat and the real events around it arrive in one batch and in
    /// timestamp order.
    fn pump(&mut self, sink: &mut dyn FnMut(ShellEvent)) {
        if self.lost.is_none()
            && let Err(error) = self.conn.drain(0, &[])
        {
            crcbl_core::log::error!("wayland connection lost: {error}");
            self.lost = Some(error.to_string());
        }
        self.process_raw();
        // After the socket, because a `wl_data_source.send` that arrived in
        // this batch is what a read of our own selection is waiting for, and
        // before delivery, so a transfer that completed here is answered in
        // the same frame it finished.
        self.service_transfers();
        // After the socket, so a `selection` that arrived in this batch has
        // already been recorded: a held read most often becomes answerable the
        // instant keyboard focus does.
        self.resolve_held_reads();
        self.drive_repeats();
        // Drain by count, not `while let`: a sink that creates a window must
        // not be able to spin this loop, and whatever it queued belongs to the
        // next frame — which is what the socket would have done anyway.
        for _ in 0..self.queue.len() {
            let Some(event) = self.queue.pop_front() else {
                break;
            };
            sink(event);
        }
    }

    /// Blocks until an event arrives, `timeout` elapses, or a key repeat comes
    /// due.
    ///
    /// The last clause is what makes repeat work for an editor that idles at
    /// zero frames per second: without it, holding Backspace in a text field
    /// would delete one character and then wait for an unrelated event.
    fn wait_events(&mut self, timeout: Option<Duration>) {
        if self.lost.is_some() {
            return;
        }
        let mut deadline = match (timeout, self.next_repeat_in()) {
            (Some(timeout), Some(repeat)) => Some(timeout.min(repeat)),
            (Some(timeout), None) => Some(timeout),
            (None, repeat) => repeat,
        };
        // A transfer in flight is waited on two ways: its descriptor goes into
        // the poll below, which wakes the moment a peer writes, and the wait is
        // capped so that a peer which *never* writes still has its deadline
        // checked. Without the cap, an editor idling with `timeout: None` would
        // sleep through a stalled paste's timeout and answer nothing.
        let aux = self.transfer_fds();
        if !aux.is_empty() || !self.writes.is_empty() || !self.held.is_empty() {
            deadline = Some(deadline.map_or(TRANSFER_POLL, |deadline| deadline.min(TRANSFER_POLL)));
        }
        let timeout_ms = deadline.map_or(-1, |deadline| {
            c_int::try_from(deadline.as_millis()).unwrap_or(c_int::MAX)
        });
        if let Err(error) = self.conn.drain(timeout_ms, &aux) {
            crcbl_core::log::error!("wayland connection lost: {error}");
            self.lost = Some(error.to_string());
        }
    }

    /// The handles `crcbl-vk` needs for `vkCreateWaylandSurfaceKHR`.
    ///
    /// Both are available the instant the window exists — a `wl_surface` is
    /// created up front and only its *size* is pending — so a HAL surface can
    /// be created immediately and only the swapchain waits for the first
    /// configure. That is the split [`Shell::surface_target`] documents, and
    /// Wayland is the platform it was written for.
    ///
    /// Lifetime: the `wl_display` lives as long as this shell and the
    /// `wl_surface` as long as the window, so a HAL surface built from these
    /// must be destroyed before [`destroy_window`](Shell::destroy_window) and
    /// before the shell is dropped.
    fn surface_target(&self, window: WindowId) -> Result<SurfaceTarget, ShellError> {
        let state = self.window(window)?;
        let surface = NonNull::new(state.surface.cast::<c_void>())
            .ok_or_else(|| ShellError::invalid_window(window))?;
        Ok(SurfaceTarget::Wayland {
            display: self.conn.display.cast::<c_void>(),
            surface,
        })
    }

    /// Locks or confines the pointer through `pointer-constraints-v1`.
    ///
    /// The constraint is created per seat that currently has a pointer, and is
    /// rebuilt when one appears later — so a game that locks the pointer before
    /// a mouse is plugged in is locked the moment it is.
    ///
    /// `persistent` lifetime, not `oneshot`: the compositor deactivates a
    /// constraint whenever it wants (an alt-tab, a compositor keybinding) and a
    /// one-shot constraint would then be gone for good, leaving a first-person
    /// camera with a free cursor and no indication why.
    ///
    /// # Errors
    ///
    /// [`ShellError::Unsupported`] if the compositor has no
    /// `zwp_pointer_constraints_v1`, or [`ShellError::InvalidWindow`] for a
    /// stale handle.
    fn set_pointer_mode(&mut self, window: WindowId, mode: PointerMode) -> Result<(), ShellError> {
        self.window(window)?;
        if !self.caps.contains(mode.required_cap()) {
            return Err(Self::unsupported(mode.as_str()));
        }
        self.rebuild_constraint(window, mode);
        Ok(())
    }

    /// Hides the cursor with `None`. **A shape is recorded and not yet
    /// applied** — the honest half of this call, and why.
    ///
    /// # Hiding is real
    ///
    /// `wl_pointer.set_cursor` with a **null** surface *is* "no cursor" in core
    /// Wayland. It needs no buffer, no theme and no extra protocol, so
    /// [`set_cursor(window, None)`](Shell::set_cursor) works on every
    /// compositor — which is the case a first-person game and
    /// [`PointerMode::Locked`] actually need.
    ///
    /// The request is re-sent on every `wl_pointer.enter`, because a compositor
    /// resets the cursor when it crosses a surface boundary; a client that set
    /// it once would watch it flicker back on every re-entry.
    ///
    /// # A shape is not, and this is a real gap
    ///
    /// Naming a shape needs one of two things, and this slice has neither:
    ///
    /// * **A cursor buffer.** Load the user's XCursor theme, pick the image for
    ///   the right shape at the right scale, wrap it in a `wl_shm` buffer and
    ///   attach it to a cursor surface. That is a theme loader plus a second
    ///   presentation path inside the shell, and it is how the cursor ends up
    ///   *disagreeing* with the user's theme the moment either side changes.
    /// * **`cursor-shape-v1`**, which is the right answer — it hands the
    ///   compositor a name from the same CSS vocabulary
    ///   [`CursorIcon::as_css_name`] already speaks and lets the compositor
    ///   draw its own themed, correctly-scaled cursor. It is **not** vendored
    ///   here for one concrete reason: its `wl_interface` type table references
    ///   `zwp_tablet_tool_v2`, so generating it requires vendoring the whole
    ///   50 KB deprecated `tablet-unstable-v2` protocol to satisfy a single
    ///   pointer for a request this engine will never send. That trade is worth
    ///   revisiting when the editor needs resize cursors (P10); it is not worth
    ///   it for a slice with no renderer.
    ///
    /// So a shape request is **accepted and recorded** rather than failed: a UI
    /// calls this on every hover, and erroring there would make every consumer
    /// handle a case that is going to start working. It also *stops hiding* —
    /// but because un-hiding likewise needs a buffer, the compositor's arrow
    /// returns at the next `wl_pointer.enter` rather than immediately.
    /// [`WindowState`] does not claim otherwise, because the seam has no
    /// effective-cursor field.
    ///
    /// # Errors
    ///
    /// [`ShellError::InvalidWindow`] if the handle is stale.
    fn set_cursor(
        &mut self,
        window: WindowId,
        cursor: Option<CursorIcon>,
    ) -> Result<(), ShellError> {
        self.window_mut(window)?.cursor = Some(cursor);
        let focused: Vec<usize> = (0..self.seats.len())
            .filter(|index| self.seats[*index].pointer_focus == Some(window))
            .collect();
        for index in focused {
            self.apply_cursor(index, window);
        }
        Ok(())
    }

    fn warp_pointer(
        &mut self,
        window: WindowId,
        position: PhysicalPoint,
    ) -> Result<(), ShellError> {
        let _ = position;
        self.window(window)?;
        // Not "not yet" — never. A client that can move the cursor can spoof
        // clicks, so Wayland has no such request and will not grow one.
        Err(Self::unsupported("pointer warp"))
    }

    fn reply_close_request(
        &mut self,
        window: WindowId,
        reply: CloseReply,
    ) -> Result<(), ShellError> {
        let state = self.window_mut(window)?;
        if !state.close_pending {
            return Err(ShellError::NoPendingCloseRequest { window });
        }
        state.close_pending = false;
        match reply {
            CloseReply::Close => self.destroy_window(window),
            CloseReply::Keep => Ok(()),
        }
    }

    /// Claims the seat's selection and holds the bytes until somebody asks.
    ///
    /// # The serial, and why a window with no input cannot copy
    ///
    /// `wl_data_device.set_selection` takes "the serial of the event that
    /// triggered this request", and a compositor may check it — wlroots
    /// compares it against the serial of the selection currently in effect and
    /// silently drops anything that looks older. The seam has no serials in it,
    /// so this quotes the seat's most recent input serial, which is the one
    /// belonging to whatever the user did to cause the copy.
    ///
    /// A window that has never received input on any seat therefore cannot
    /// claim the selection, and this reports
    /// [`ShellError::NeedsUserInteraction`] rather than sending a request the
    /// compositor will ignore. That is not a limitation this backend invented:
    /// a Wayland client genuinely may not take the clipboard without user
    /// interaction, which is the whole point of the serial — it is what stops a
    /// background process stealing the clipboard, and the browser gates its own
    /// clipboard write the same way.
    ///
    /// An empty `offers` slice releases the selection.
    ///
    /// # Errors
    ///
    /// [`ShellError::Unsupported`] if the compositor has no
    /// `wl_data_device_manager`, [`ShellError::InvalidWindow`] for a stale
    /// handle, [`ShellError::NeedsUserInteraction`] if no seat can name a
    /// serial, or [`ShellError::Backend`] if there is no seat at all.
    fn clipboard_offer(
        &mut self,
        window: WindowId,
        offers: &[ClipboardOffer<'_>],
    ) -> Result<(), ShellError> {
        self.window(window)?;
        if !self.caps.contains(ShellCaps::CLIPBOARD) {
            return Err(Self::unsupported("clipboard"));
        }
        let index = *self
            .seats_for_window(window)
            .first()
            .ok_or_else(|| ShellError::Backend("no seat has a wl_data_device".to_string()))?;
        // Replacing our own selection: the old source is destroyed *after* the
        // new one is in effect, so nothing observes a moment with no clipboard.
        let previous = self.seats[index]
            .data
            .as_mut()
            .and_then(|device| device.source.take());

        let serial = self.seats[index].last_serial;
        let mut published = None;
        if !offers.is_empty() {
            if serial == 0 {
                if let Some(source) = previous.as_ref() {
                    // SAFETY: a live `wl_data_source` this backend created.
                    unsafe { self.conn.release(source.proxy, wl_data_source::destroy) };
                }
                if let Some(device) = self.seats[index].data.as_mut() {
                    device.source = None;
                }
                return Err(ShellError::NeedsUserInteraction {
                    backend: ShellBackend::Wayland,
                    what: "claiming the clipboard",
                });
            }
            let manager = self.conn.data_device_manager;
            // SAFETY: the manager is a live global; `create_data_source` takes
            // no arguments beyond the object it creates.
            let source = unsafe { wl_data_device_manager::create_data_source(manager) };
            if source.is_null() {
                // The previous source was `take`n out of the device above.
                // Returning without putting it back left the compositor still
                // believing we owned the selection while nothing on this side
                // did — so the next `wl_data_source.send` found no seat, and the
                // selection could never be released either. Nothing changed, so
                // nothing should have moved: put it back.
                if let Some(device) = self.seats[index].data.as_mut() {
                    device.source = previous;
                }
                return Err(ShellError::Backend(
                    "wl_data_device_manager.create_data_source failed".to_string(),
                ));
            }
            self.conn.watch(source, ObjectKind::DataSource);
            let mut payload = Vec::with_capacity(offers.len());
            for offer in offers {
                let Ok(mime) = CString::new(offer.mime.as_str()) else {
                    continue;
                };
                // SAFETY: the source is live and the string outlives the call.
                unsafe { wl_data_source::offer(source, &mime) };
                payload.push((offer.mime.as_str().to_string(), offer.bytes.to_vec()));
            }
            published = Some(data::Source {
                proxy: source,
                payload,
            });
        }

        let device = self.seats[index]
            .data
            .as_ref()
            .map_or(ptr::null_mut(), |device| device.proxy);
        let source = published
            .as_ref()
            .map_or(ptr::null_mut(), |source| source.proxy);
        // SAFETY: the device is live, and a null source is the protocol's own
        // way of releasing the selection.
        unsafe { wl_data_device::set_selection(device, source, serial) };
        self.conn.flush();
        if let Some(previous) = previous.as_ref() {
            // SAFETY: a live `wl_data_source` this backend created, destroyed
            // exactly once.
            unsafe { self.conn.release(previous.proxy, wl_data_source::destroy) };
        }
        if let Some(device) = self.seats[index].data.as_mut() {
            device.source = published;
            // We have just changed the clipboard and have **not** been told the
            // result. The offer this device still holds describes the *old*
            // selection, and answering a read from it — or worse, answering
            // `Empty` because the old one had no matching format — would report
            // a clipboard that no longer exists. The compositor echoes the new
            // selection back to us (a client sees its own, which is what makes
            // pasting one's own copy work at all), and that event is what makes
            // this true again.
            //
            // Found by the end-to-end suite the moment "held, not empty" was
            // enforced: pasting our own copy answered `Empty`, because the
            // answer was computed from what we knew a microsecond before we
            // changed it.
            device.selection_seen = false;
        }
        Ok(())
    }

    /// Starts reading the selection, over a pipe, without blocking.
    ///
    /// The answer arrives as [`ShellEvent::ClipboardData`] from a later
    /// [`pump`](Shell::pump) — never from this call, and never from a `read`
    /// on the event-loop thread. See [`fd`] for the mechanism and for what
    /// happens when the peer never writes.
    ///
    /// # There is nothing to read unless a window of ours has focus
    ///
    /// `wl_data_device.selection` is delivered only to the client with keyboard
    /// focus on that seat, and again whenever focus arrives — so a Wayland
    /// client's clipboard knowledge is *acquired on focus* and goes stale when
    /// focus leaves. A read issued while this shell has not been told what is
    /// on the clipboard is therefore **held** until it has been, and only then
    /// answered. It is not answered `Empty`, which would be indistinguishable
    /// from a clipboard that really is empty and is what forced a retry loop
    /// into the end-to-end suite before this was fixed.
    ///
    /// The wait is bounded by [`fd::TIMEOUT`], after which the answer is
    /// [`ClipboardContent::Unavailable`] — obligation 4 requires every accepted
    /// request to end. [`clipboard_readable`](Shell::clipboard_readable) is how
    /// a UI finds out in advance that the answer will not be immediate.
    ///
    /// # Errors
    ///
    /// [`ShellError::Unsupported`] if the compositor has no
    /// `wl_data_device_manager`, or [`ShellError::InvalidWindow`] for a stale
    /// handle. A clipboard that holds nothing compatible is *not* an error —
    /// it is an answer — and neither is one this client cannot see yet, which
    /// is a wait.
    fn clipboard_request(
        &mut self,
        window: WindowId,
        mime: MimeType,
    ) -> Result<ClipboardRequestId, ShellError> {
        self.window(window)?;
        if !self.caps.contains(ShellCaps::CLIPBOARD) {
            return Err(Self::unsupported("clipboard"));
        }
        let request = ClipboardRequestId(self.next_request_id);
        self.next_request_id += 1;

        match self.resolve_selection(window, mime) {
            Resolution::Ready { offer, spelling } => {
                let delivery = Delivery::Clipboard {
                    window,
                    request,
                    // The *peer's* spelling, not the one that was asked for: a
                    // peer offering bare `text/plain` answers a request for
                    // `text/plain;charset=utf-8`, and `ReceivedMime` exists so
                    // the difference survives to the consumer.
                    mime: ReceivedMime::new(&spelling),
                };
                if let Err(error) = self.start_receive(offer, &spelling, delivery) {
                    crcbl_core::log::warn!("clipboard read could not be started: {error}");
                    self.answer_read(
                        window,
                        request,
                        ReceivedMime::new(&spelling),
                        ClipboardContent::Unavailable,
                    );
                }
            }
            // Queued rather than returned, so that a consumer written against
            // the asynchronous shape takes the same path whether or not there
            // was anything on the clipboard.
            Resolution::Empty => self.answer_read(
                window,
                request,
                ReceivedMime::from(mime),
                ClipboardContent::Empty,
            ),
            // **Held**, not answered. See this method's docs and the seam's
            // obligation 5: answering `Empty` here would be a lie a consumer
            // cannot detect, and it is the lie that made the end-to-end suite
            // grow a retry loop no editor would contain.
            Resolution::Unknown => self.held.push(HeldRead {
                window,
                request,
                mime,
                deadline_nanos: ffi::monotonic_nanos()
                    .saturating_add(u64::try_from(fd::TIMEOUT.as_nanos()).unwrap_or(u64::MAX)),
            }),
        }
        Ok(request)
    }

    /// Whether a read issued now could be answered without waiting.
    ///
    /// True exactly when the compositor has told us what is on the clipboard
    /// and has not since taken keyboard focus away — which is the honest
    /// Wayland answer, and the reason this method is on the seam at all. A
    /// background window reports `false` and a paste button greys out, instead
    /// of a paste that appears to do nothing.
    fn clipboard_readable(&self, window: WindowId) -> bool {
        self.caps.contains(ShellCaps::CLIPBOARD)
            && self.window(window).is_ok()
            && self.seats_for_window(window).into_iter().any(|index| {
                self.seats[index]
                    .data
                    .as_ref()
                    .is_some_and(|device| device.selection_seen)
            })
    }

    fn align_event_clock(&mut self, elapsed: Duration) {
        self.align_time_base(elapsed);
    }
}

impl WaylandShell {
    /// The `wl_output` proxy for a monitor, or null for "the compositor
    /// chooses".
    ///
    /// Null is not a fallback: `xdg_toplevel.set_fullscreen` takes a nullable
    /// output *by design*, and `DisplayMode::Borderless { monitor: None }`
    /// exists because that is the only thing a Wayland client can reliably ask
    /// for.
    fn output_proxy_for(&self, monitor: Option<MonitorId>) -> *mut WlProxy {
        let Some(id) = monitor else {
            return ptr::null_mut();
        };
        self.outputs
            .iter()
            .find(|output| output.id == id)
            .map_or(ptr::null_mut(), |output| output.proxy as *mut WlProxy)
    }

    /// Records the compositor's answer to our `set_mode(server_side)`.
    ///
    /// Deliberately **not** exposed as state on the seam.
    /// [`ShellCaps::SERVER_DECORATIONS`] is the portable question — "can this
    /// session have server-side decorations at all" — and it is latched from
    /// whether `zxdg_decoration_manager_v1` bound. The per-window answer is a
    /// Wayland-shaped detail with no equivalent anywhere else, and adding a
    /// `u32` from an unstable protocol to [`WindowState`] would put a platform
    /// type in the seam this crate exists to keep platform-free.
    ///
    /// What matters is that a compositor answering `client_side` is a *known*
    /// state rather than a surprise when the UI layer starts drawing at P1, so
    /// it is logged at warning level — the engine has no client-side
    /// decorations, so on such a compositor a window has no title bar and no
    /// resize grips, and somebody has to be told why.
    fn decoration_configured(&mut self, decoration: usize, mode: u32) {
        let Some(window) = self.window_by_proxy(decoration, |state| state.decoration as usize)
        else {
            return;
        };
        if mode == zxdg_toplevel_decoration_v1::mode::SERVER_SIDE {
            crcbl_core::log::debug!("{window:?}: the compositor draws this window's decorations");
        } else {
            crcbl_core::log::warn!(
                "{window:?}: the compositor refused server-side decorations \
                 (xdg-decoration mode {mode}); this window has no title bar until \
                 the UI layer can draw one"
            );
        }
    }
}

/// Creates an add-on object, or nothing if the manager global was never bound.
///
/// # Safety
///
/// `manager` must be null or a live global, and `create` must marshal a request
/// on it.
unsafe fn optional(
    manager: *mut WlProxy,
    create: impl FnOnce(*mut WlProxy) -> *mut WlProxy,
) -> *mut WlProxy {
    if manager.is_null() {
        return ptr::null_mut();
    }
    create(manager)
}

/// Sends `set_min_size`/`set_max_size` for a constraint set.
///
/// Wayland has **no aspect hint**, which is why the backend does not set
/// [`ShellCaps::ASPECT_HINT_HONORED`] and why the renderer letterboxes instead.
/// `resizable: false` becomes min == max, which is the only way to say it.
///
/// # Safety
///
/// `toplevel` must be a live `xdg_toplevel`.
unsafe fn apply_constraints(
    toplevel: *mut WlProxy,
    constraints: SizeConstraints,
    resizable: bool,
    requested: LogicalSize,
) {
    // xdg-shell takes these in logical units, and zero means "no limit".
    let to_pair = |size: Option<LogicalSize>| {
        size.map_or((0, 0), |size| {
            #[expect(
                clippy::cast_possible_truncation,
                reason = "clamped into i32's range on the line above the cast"
            )]
            (
                size.width.round().clamp(0.0, f64::from(i32::MAX)) as i32,
                size.height.round().clamp(0.0, f64::from(i32::MAX)) as i32,
            )
        })
    };
    let (min, max) = if resizable {
        (to_pair(constraints.min), to_pair(constraints.max))
    } else {
        let fixed = to_pair(Some(requested));
        (fixed, fixed)
    };
    // SAFETY: the caller guarantees `toplevel` is live; both requests take two
    // plain integers.
    unsafe {
        xdg_toplevel::set_min_size(toplevel, min.0, min.1);
        xdg_toplevel::set_max_size(toplevel, max.0, max.1);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::PhysicalSize;

    /// A handle to nothing, for the pure-logic tests: `Repeat` stores one and
    /// never dereferences it.
    fn window_id() -> WindowId {
        let mut pool: Pool<u8> = Pool::new();
        pool.insert(0).cast()
    }

    #[test]
    fn wayland_timestamps_rebase_onto_the_engine_epoch() {
        // The shell was opened 5s after the engine clock started, and the
        // compositor stamps an event 1.25s later still.
        let epoch = 95_000_000_000; // engine epoch, in CLOCK_MONOTONIC nanos
        let now = 101_250_000_000;
        #[expect(
            clippy::cast_possible_truncation,
            reason = "the value fits by construction"
        )]
        let stamp = (now / 1_000_000) as u32;
        assert_eq!(
            TimeBase::rebase(epoch, now, stamp),
            EventTime::from_millis(6_250),
            "6.25s after the engine epoch, not 101s after boot"
        );
    }

    #[test]
    fn a_wrapped_timestamp_is_widened_towards_now_not_forwarded_raw() {
        // 49.7 days of uptime: the compositor's 32-bit counter has just
        // wrapped, and the event's low bits are tiny while `now` is huge. A
        // backend that forwarded the raw value would report an event from
        // process start.
        const WRAP: u64 = 1 << 32;
        let now_millis = WRAP + 10;
        let now = now_millis * 1_000_000;
        let epoch = 0;
        assert_eq!(
            TimeBase::rebase(epoch, now, 5),
            EventTime::from_millis(WRAP + 5),
            "the event is 5ms after the wrap, not 5ms after boot"
        );

        // The other side of the wrap: an event stamped just *before* it, read
        // just after. Widening must not push it a whole period into the future.
        let now_millis = WRAP + 3;
        assert_eq!(
            TimeBase::rebase(epoch, now_millis * 1_000_000, u32::MAX),
            EventTime::from_millis(WRAP - 1),
        );
    }

    #[test]
    fn a_timestamp_ahead_of_now_before_the_first_wrap_does_not_underflow() {
        // The whole `time` argument is an unvalidated `u32` off the wire, and on
        // a machine with an uptime under 49.7 days there is no earlier wrap to
        // move a too-large stamp into. Subtracting one anyway panicked in debug
        // and produced ~1.8e19 ms in release — for a hostile compositor, and for
        // an honest one whose event merely ran a few milliseconds ahead of the
        // `CLOCK_MONOTONIC` sample taken here.
        let now = 60_000_000_000; // one minute of uptime, in nanoseconds
        assert_eq!(
            TimeBase::rebase(0, now, u32::MAX),
            EventTime::from_millis(u64::from(u32::MAX)),
            "the only reconstruction that exists is the raw value"
        );
        // The nearby, ordinary case is unaffected.
        assert_eq!(
            TimeBase::rebase(0, now, 60_001),
            EventTime::from_millis(60_001)
        );
    }

    #[test]
    fn timestamps_never_go_negative_when_an_event_predates_the_epoch() {
        // A compositor can stamp an event microseconds before the shell was
        // created. Saturation is the documented answer; `EventTime` is a
        // duration and has no negative values.
        assert_eq!(
            TimeBase::rebase(10_000_000_000, 10_000_000_000, 9_000),
            EventTime::ZERO
        );
    }

    #[test]
    fn events_the_protocol_gives_no_time_for_are_stamped_now_not_zero() {
        // `wl_pointer.enter` and `leave` carry a serial and no timestamp at
        // all. Stamping them with the last frame's time reads as "at process
        // start" before the pointer has ever moved — which is what
        // `EventTime`'s own rule about missing timestamps forbids.
        let base = TimeBase {
            epoch_nanos: ffi::monotonic_nanos().saturating_sub(3_000_000_000),
        };
        let stamped = base.event_time_now();
        assert!(
            stamped >= EventTime::from_millis(2_900),
            "an event with no compositor time is stamped now, got {stamped:?}"
        );
        assert_ne!(stamped, EventTime::ZERO);
    }

    #[test]
    fn the_microsecond_clock_needs_no_wrap_handling_and_keeps_its_precision() {
        // `relative_motion`'s 64-bit microseconds are the more precise of the
        // two clocks a pointer event arrives on, which is why a merged motion
        // event prefers them.
        let base = TimeBase {
            epoch_nanos: 5_000_000_000,
        };
        assert_eq!(
            base.event_time_micros(5_000_500),
            EventTime::from_micros(500),
            "half a millisecond after the epoch, not rounded to zero"
        );
        // A timestamp before the epoch saturates rather than wrapping.
        assert_eq!(base.event_time_micros(1), EventTime::ZERO);
    }

    #[test]
    fn aligning_the_time_base_moves_the_epoch_backwards() {
        // The contract `Shell::align_event_clock` exists for: after alignment,
        // "now" reads as the elapsed time the engine clock reports.
        let mut base = TimeBase::now();
        let before = base.epoch_nanos;
        base.align(Duration::from_secs(2));
        assert!(
            base.epoch_nanos <= before,
            "an epoch two seconds ago is earlier than one taken now"
        );
        assert!(
            before.saturating_sub(base.epoch_nanos) >= 1_900_000_000,
            "the shift is the elapsed time, give or take the call overhead"
        );
    }

    #[test]
    fn repeats_start_after_the_delay_and_then_run_at_the_rate() {
        let mut repeat = Repeat {
            rate: 25,
            delay: 600,
            key: None,
        };
        let now = 1_000_000_000;
        repeat.start(
            now,
            RepeatKeyRequest {
                window: window_id(),
                scancode: 30,
                key_code: Some(KeyCode::KeyA),
                keysym: Keysym::from_char('a'),
            },
        );
        let key = repeat.key.expect("repeat armed");
        assert_eq!(
            key.next_nanos - now,
            600_000_000,
            "the first repeat waits the whole delay"
        );
        assert_eq!(key.interval_nanos, 40_000_000, "25 Hz is 40ms apart");

        // Releasing the key that is repeating stops it; releasing another does
        // not, which is what makes Shift+held-A keep repeating A.
        repeat.stop(31);
        assert!(repeat.key.is_some());
        repeat.stop(30);
        assert!(repeat.key.is_none());
    }

    #[test]
    fn a_zero_rate_disables_repeat_entirely() {
        // `wl_keyboard.repeat_info` defines rate 0 as "no repeat", and a user
        // who turned repeat off in their compositor must not get repeats from
        // us.
        let mut repeat = Repeat {
            rate: 0,
            delay: 300,
            key: None,
        };
        repeat.start(
            0,
            RepeatKeyRequest {
                window: window_id(),
                scancode: 30,
                key_code: Some(KeyCode::KeyA),
                keysym: Keysym::NONE,
            },
        );
        assert!(repeat.key.is_none());
        assert_eq!(repeat.due_at(), None);
    }

    #[test]
    fn value120_accumulates_into_detents_with_the_engine_sign_convention() {
        // One notch of a real wheel, scrolled away from the user: Wayland
        // reports -120 on the vertical axis, and the engine's convention is
        // that away-from-the-user is positive.
        let mut frame = PointerFrame {
            has_axis: true,
            ..PointerFrame::default()
        };
        frame.value120[0] = -120.0;
        assert_eq!(
            frame.scroll(1.0),
            Some(ScrollDelta::Lines { x: 0.0, y: 1.0 })
        );

        // A high-resolution wheel sends fractions of a detent, and they add up
        // across the frame rather than each becoming an event.
        let mut frame = PointerFrame {
            has_axis: true,
            ..PointerFrame::default()
        };
        frame.value120[0] = -30.0;
        frame.value120[0] += -30.0;
        assert_eq!(
            frame.scroll(1.0),
            Some(ScrollDelta::Lines { x: 0.0, y: 0.5 }),
            "two eighth-turns are a quarter of a detent, not two detents"
        );

        // Horizontal lands in `x`, and the sign flips there too.
        let mut frame = PointerFrame {
            has_axis: true,
            ..PointerFrame::default()
        };
        frame.value120[1] = 120.0;
        assert_eq!(
            frame.scroll(1.0),
            Some(ScrollDelta::Lines { x: -1.0, y: 0.0 })
        );
    }

    #[test]
    fn a_wheel_prefers_detents_and_a_touchpad_gets_pixels() {
        // The distinction `ScrollDelta` refuses to collapse. A notched wheel
        // sends both a discrete count and a continuous value; reporting the
        // continuous one would turn a click into whatever number the
        // compositor's scroll-speed setting produced.
        let mut frame = PointerFrame {
            has_axis: true,
            axis_source: Some(wl_pointer::axis_source::WHEEL),
            ..PointerFrame::default()
        };
        frame.value120[0] = -120.0;
        frame.continuous[0] = -15.0;
        assert_eq!(
            frame.scroll(1.0),
            Some(ScrollDelta::Lines { x: 0.0, y: 1.0 })
        );

        // A touchpad sends only the continuous value, and gets pixels — scaled
        // into device pixels, because the wire value is surface-local.
        let frame = PointerFrame {
            has_axis: true,
            axis_source: Some(wl_pointer::axis_source::FINGER),
            continuous: [-13.0, 0.0],
            ..PointerFrame::default()
        };
        assert_eq!(
            frame.scroll(2.0),
            Some(ScrollDelta::Pixels { x: 0.0, y: 26.0 })
        );

        // `axis_discrete` is the pre-version-8 spelling and behaves the same.
        let frame = PointerFrame {
            has_axis: true,
            discrete: [1.0, 0.0],
            ..PointerFrame::default()
        };
        assert_eq!(
            frame.scroll(1.0),
            Some(ScrollDelta::Lines { x: 0.0, y: -1.0 })
        );
    }

    #[test]
    fn an_axis_frame_with_no_movement_produces_no_wheel_event() {
        // `axis_stop` ends a kinetic scroll and carries no delta; emitting a
        // zero-length `Wheel` for it would be one event per touchpad lift.
        let frame = PointerFrame {
            has_axis: true,
            ..PointerFrame::default()
        };
        assert_eq!(frame.scroll(1.0), None);
        assert!(PointerFrame::default().is_empty());
        assert_eq!(PointerFrame::default().scroll(1.0), None);
    }

    #[test]
    fn a_frame_carrying_only_relative_motion_is_not_empty() {
        // The locked-pointer case: no `wl_pointer.motion` arrives at all, and a
        // flush that keyed on absolute motion would drop every aim sample.
        let frame = PointerFrame {
            raw: Some((3.0, -2.0)),
            ..PointerFrame::default()
        };
        assert!(!frame.is_empty());
    }

    #[test]
    fn axis_slots_are_vertical_then_horizontal() {
        assert_eq!(axis_slot(wl_pointer::axis::VERTICAL_SCROLL), Some(0));
        assert_eq!(axis_slot(wl_pointer::axis::HORIZONTAL_SCROLL), Some(1));
        assert_eq!(axis_slot(7), None, "an axis a later version might add");
    }

    #[test]
    fn an_outputs_scale_comes_from_the_logical_size_not_from_wl_output_scale() {
        // The finding `xdg_output` exists to fix. A 1920x1080 output run at
        // 150% reports `wl_output.scale = 2`, because that field is an integer
        // buffer scale and not the desktop's scale. The logical size says 1.5.
        let mut output = Output {
            proxy: 1,
            xdg: ptr::null_mut(),
            global: 1,
            id: MonitorId(1),
            x: 0,
            y: 0,
            width: 1920,
            height: 1080,
            refresh_millihertz: 60_000,
            scale: 2,
            logical_x: 0,
            logical_y: 0,
            logical_width: 1280,
            logical_height: 720,
            name: "HEADLESS-1".to_string(),
            settled: true,
        };
        assert!((output.scale_factor() - 1.5).abs() < 1e-9);
        let info = output.info(true);
        assert_eq!(
            info.size(),
            PhysicalSize::new(1920, 1080),
            "the size stays the mode, which is what a swapchain is built from"
        );

        // Without xdg_output there is nothing better than the integer.
        output.logical_width = 0;
        assert!((output.scale_factor() - 2.0).abs() < f64::EPSILON);
    }

    #[test]
    fn a_monitors_position_is_scaled_out_of_the_logical_desktop_space() {
        // A second output placed to the right of a 1280-logical-wide first one,
        // itself at 2x. Its logical position is 1280; in its own device pixels
        // that is 2560. Reporting the raw 1280 next to a physical size would
        // put the two monitors on top of each other.
        let output = Output {
            proxy: 2,
            xdg: ptr::null_mut(),
            global: 2,
            id: MonitorId(2),
            x: 1280,
            y: 0,
            width: 3840,
            height: 2160,
            refresh_millihertz: 60_000,
            scale: 2,
            logical_x: 1280,
            logical_y: 0,
            logical_width: 1920,
            logical_height: 1080,
            name: "DP-2".to_string(),
            settled: true,
        };
        assert_eq!(output.info(false).bounds.x, 2560);
        assert_eq!(output.info(false).bounds.y, 0);
        assert_eq!(output.info(false).work_area, output.info(false).bounds);
    }

    #[test]
    fn a_windows_scale_prefers_the_fractional_one() {
        let mut window = test_window();
        window.scale = 2;
        assert!((window.scale_factor() - 2.0).abs() < f64::EPSILON);
        // 150%, in the 1/120ths the protocol reports.
        window.preferred_scale = Some(180);
        assert!((window.scale_factor() - 1.5).abs() < 1e-9);
        // A compositor that sends a nonsense zero falls back rather than
        // dividing the window down to nothing.
        window.preferred_scale = Some(0);
        assert!((window.scale_factor() - 2.0).abs() < f64::EPSILON);
    }

    #[test]
    fn the_toplevel_state_array_decodes_as_native_endian_u32s() {
        // `xdg_toplevel.configure` carries its states as a `wl_array` of
        // `uint32_t`. Reading it a byte at a time — or as big endian — would
        // make "fullscreen" indistinguishable from "maximized", which is
        // exactly the bit `WindowConfiguration::mode` is computed from.
        let mut bytes = Vec::new();
        bytes.extend_from_slice(&xdg_toplevel::state::ACTIVATED.to_ne_bytes());
        bytes.extend_from_slice(&xdg_toplevel::state::FULLSCREEN.to_ne_bytes());
        assert_eq!(
            decode_state_array(&bytes),
            vec![
                xdg_toplevel::state::ACTIVATED,
                xdg_toplevel::state::FULLSCREEN
            ]
        );
        assert!(decode_state_array(&[]).is_empty());
        // A trailing partial element is dropped rather than read past.
        assert_eq!(decode_state_array(&[1, 0, 0, 0, 9]).len(), 1);
    }

    #[test]
    fn the_protocol_descriptors_carry_the_names_the_registry_advertises() {
        // A sanity check that the generated code is wired to the right
        // interfaces at all: `bind_global` matches on these strings, and a
        // wrong one means a global is silently never bound.
        assert_eq!(wl_compositor::NAME, c"wl_compositor");
        assert_eq!(xdg_wm_base::NAME, c"xdg_wm_base");
        assert_eq!(wl_output::NAME, c"wl_output");
        assert_eq!(wl_seat::NAME, c"wl_seat");
        assert_eq!(zxdg_output_manager_v1::NAME, c"zxdg_output_manager_v1");
        assert_eq!(
            zxdg_decoration_manager_v1::NAME,
            c"zxdg_decoration_manager_v1"
        );
        assert_eq!(
            wp_fractional_scale_manager_v1::NAME,
            c"wp_fractional_scale_manager_v1"
        );
        assert_eq!(wp_viewporter::NAME, c"wp_viewporter");
        assert_eq!(
            zwp_relative_pointer_manager_v1::NAME,
            c"zwp_relative_pointer_manager_v1"
        );
        assert_eq!(
            zwp_pointer_constraints_v1::NAME,
            c"zwp_pointer_constraints_v1"
        );
        assert_eq!(xdg_toplevel::state::FULLSCREEN, 2);
        assert_eq!(wl_output::mode::CURRENT, 1);
        assert_eq!(wl_seat::capability::POINTER, 1);
        assert_eq!(wl_seat::capability::KEYBOARD, 2);
        assert_eq!(wl_keyboard::keymap_format::XKB_V1, 1);
        assert_eq!(zxdg_toplevel_decoration_v1::mode::SERVER_SIDE, 2);
    }

    #[test]
    fn opening_without_a_compositor_fails_cleanly_rather_than_aborting() {
        // The property the whole `dlopen` decision exists for: on a machine
        // with no Wayland session this is an ordinary error the registry can
        // fall through, not a loader failure before `main`.
        //
        // Skipped when the machine *does* have a session, since then the
        // correct behaviour is to succeed — and opening a real connection in a
        // unit test is not this test's job.
        if std::env::var_os("WAYLAND_DISPLAY").is_some() {
            return;
        }
        let error = WaylandShell::open().expect_err("no compositor");
        assert!(
            matches!(
                error,
                ShellError::Connect {
                    backend: ShellBackend::Wayland,
                    ..
                }
            ),
            "{error}"
        );
    }

    /// A window with no proxies, for the pure-logic tests above.
    fn test_window() -> WlWindow {
        WlWindow {
            surface: ptr::null_mut(),
            xdg_surface: ptr::null_mut(),
            toplevel: ptr::null_mut(),
            decoration: ptr::null_mut(),
            fractional_scale: ptr::null_mut(),
            viewport: ptr::null_mut(),
            title: String::new(),
            requested_size: LogicalSize::new(640.0, 480.0),
            requested_mode: DisplayMode::Windowed,
            requested_constraints: SizeConstraints::default(),
            resizable: true,
            configuration: None,
            pending: PendingConfigure::default(),
            outputs: Vec::new(),
            scale: 1,
            preferred_scale: None,
            pointer_mode: PointerMode::Free,
            constraints: Vec::new(),
            cursor: None,
            accept_drops: false,
            focused: false,
            visible: true,
            close_pending: false,
        }
    }
}
