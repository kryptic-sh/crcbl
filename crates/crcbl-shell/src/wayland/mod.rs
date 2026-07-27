//! The Wayland backend: connection, registry, `xdg-shell` window lifecycle.
//!
//! `docs/plan/15-windowing.md`'s Linux policy in one sentence:
//! **libwayland-client owns the connection and the proxy objects; the protocol
//! layer above `wl_proxy_marshal_array_flags` is ours.** [`ffi`] is the first
//! half — fourteen hand-written `extern "C"` declarations, reached by `dlopen`
//! for the reasons that module states. [`protocol`] is the second — generated
//! at build time by `crcbl-wl-scanner` from the vendored XML.
//!
//! # What is in this slice (P0.5a)
//!
//! Window lifecycle only: connect, bind `wl_compositor` / `xdg_wm_base` /
//! `wl_output`, create `wl_surface` + `xdg_surface` + `xdg_toplevel`, the
//! configure/ack handshake, title, size constraints, windowed ↔ borderless, the
//! close request, and monitor enumeration.
//!
//! Deliberately absent: `wl_seat` and everything downstream of it (P0.5b), and
//! `data-device` (P0.5c). [`ShellCaps`] reflects that exactly — see
//! [`WaylandShell::caps`] — so a consumer that checks capabilities gets a
//! correct answer today and does not have to be rewritten when they arrive.
//!
//! # What a real compositor does that `HeadlessShell` does not model
//!
//! Three differences, all of which this backend absorbs rather than papering
//! over. They are worth reading before writing any consumer:
//!
//! 1. **The first configure usually carries no size.** `xdg-shell` says the
//!    compositor answers the initial commit with an `xdg_surface.configure`,
//!    and that a `0 × 0` in the accompanying `xdg_toplevel.configure` means
//!    "you choose". [`HeadlessShell`](crate::HeadlessShell) always dictates a
//!    size. Both shapes satisfy the seam's contract — the window is unconfigured
//!    until a configure arrives, and configured after — but a backend has to
//!    supply the fallback, and this one falls back to
//!    [`WindowDesc::size`](crate::WindowDesc::size) scaled by the current
//!    factor.
//! 2. **Size arrives before scale.** A window's scale factor comes from
//!    `wl_surface.enter`, which a compositor only sends once the surface is
//!    *mapped* — and mapping requires a buffer, which requires a swapchain,
//!    which requires the size from the configure. So the first configure is
//!    necessarily at scale 1.0, and the true scale arrives later as a
//!    [`ShellEvent::ScaleFactorChanged`]. `WindowConfiguration` groups size and
//!    scale because they arrive together *in one message*; it does not promise
//!    the first message is correct about both.
//! 3. **Configures arrive in two messages and take effect on the third.**
//!    `xdg_toplevel.configure` carries the size and the states,
//!    `xdg_surface.configure` carries the serial and means "that is the whole
//!    update", and `ack_configure` + `commit` is the reply. This backend
//!    accumulates and only publishes a [`WindowConfiguration`] on the
//!    `xdg_surface.configure`, which is what keeps a consumer from ever seeing
//!    a size without its states.
//! 4. **Configured is not the same as managed** — the finding with real
//!    consequences. A surface is mapped exactly while it has a *buffer*, and an
//!    unmapped `xdg_toplevel` gets its one initial configure and nothing else
//!    ever again: no compositor-chosen geometry, no answer to
//!    `set_fullscreen`, and no entry in the window manager's tree at all
//!    (`swaymsg [app_id=…] …` matches nothing). Attaching a buffer is the
//!    renderer's job — `crcbl-vk`'s swapchain, at P1 — so **this backend cannot
//!    map a window on its own, by design**, and the correct sequence for a
//!    consumer is: configure → create swapchain → present → *then* expect the
//!    compositor's real geometry as a second [`ShellEvent::Resized`]. The
//!    end-to-end suite reaches the same state with a stand-in `wl_shm` buffer;
//!    see [`e2e`], which exists only under the `wayland-e2e` feature and is
//!    deleted when P1 can do it for real.

pub mod ffi;
pub mod protocol;

/// Test-only: maps a window's surface with a stand-in buffer.
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

use crcbl_core::{EventTime, Pool, SurfaceTarget};

use crate::{
    ClipboardOffer, ClipboardRequestId, CloseReply, CursorIcon, DisplayMode, LogicalSize, MimeType,
    MonitorId, MonitorInfo, PhysicalPoint, PhysicalRect, PointerMode, Shell, ShellBackend,
    ShellCaps, ShellError, ShellEvent, SizeConstraints, WindowConfiguration, WindowDesc, WindowId,
    WindowState,
};

use ffi::{Lib, WlArgument, WlDisplay, WlMessage, WlProxy};
use protocol::wayland::{wl_compositor, wl_output, wl_registry, wl_surface};
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
/// slice does not act on.
const WM_BASE_VERSION: u32 = 1;
/// See [`COMPOSITOR_VERSION`].
const OUTPUT_VERSION: u32 = 4;

// ---------------------------------------------------------------------------
// Timestamps
// ---------------------------------------------------------------------------

/// Converts Wayland's `CLOCK_MONOTONIC` milliseconds onto the engine epoch.
///
/// [`EventTime`] requires "a duration measured from the same origin as the
/// [`TimeSource`](crcbl_core::TimeSource) driving
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
    /// See [`WaylandShell::align_time_base`].
    #[allow(
        dead_code,
        reason = "reachable only through WaylandShell::align_time_base, which \
                  has no caller until the engine loop lands"
    )]
    fn align(&mut self, elapsed: Duration) {
        let now = ffi::monotonic_nanos();
        self.epoch_nanos =
            now.saturating_sub(u64::try_from(elapsed.as_nanos()).unwrap_or(u64::MAX));
    }

    /// Widens a 32-bit Wayland timestamp and rebases it.
    ///
    /// Pure so it can be tested without a compositor, which matters because the
    /// wrap is unobservable in any session shorter than seven weeks.
    fn rebase(epoch_nanos: u64, now_nanos: u64, wayland_millis: u32) -> EventTime {
        const WRAP: u64 = 1 << 32;
        let now_millis = now_nanos / 1_000_000;
        // Reconstruct the full-width timestamp closest to now: an event is
        // always within a few milliseconds of the present, so of the three
        // candidate epochs the nearest one is the right one.
        let mut full = (now_millis & !(WRAP - 1)) | u64::from(wayland_millis);
        if full > now_millis + WRAP / 2 {
            full -= WRAP;
        } else if full + WRAP / 2 < now_millis {
            full += WRAP;
        }
        let epoch_millis = epoch_nanos / 1_000_000;
        EventTime::from_millis(full.saturating_sub(epoch_millis))
    }

    /// This base applied to a compositor timestamp.
    ///
    /// Unused in P0.5a — no event this slice produces carries a timestamp,
    /// because every timestamped [`ShellEvent`] is an input event and input is
    /// P0.5b. It exists and is tested now so that the epoch contract is settled
    /// before the first `wl_pointer.motion` rather than after.
    #[allow(dead_code, reason = "P0.5b's input events are the first consumers")]
    fn event_time(self, wayland_millis: u32) -> EventTime {
        Self::rebase(self.epoch_nanos, ffi::monotonic_nanos(), wayland_millis)
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
    Surface,
    XdgSurface,
    XdgToplevel,
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
}

/// Everything the C dispatcher touches.
///
/// Reached **only** through a raw pointer, never through a Rust reference held
/// by [`WaylandShell`]. That is deliberate: the dispatcher runs synchronously
/// inside `wl_display_dispatch_pending`, so if the shell held a live `&mut` to
/// this allocation at the same time the two would alias. Keeping the raw
/// pointer as the sole root means the shell's `&mut Sink` and the dispatcher's
/// exist at strictly disjoint times.
#[derive(Debug, Default)]
struct Sink {
    /// Proxies we have attached the dispatcher to, and what they are.
    objects: Vec<(usize, ObjectKind)>,
    /// Decoded events awaiting the pump.
    events: Vec<RawEvent>,
}

impl Sink {
    fn kind_of(&self, proxy: usize) -> Option<ObjectKind> {
        self.objects
            .iter()
            .find(|(candidate, _)| *candidate == proxy)
            .map(|(_, kind)| *kind)
    }

    fn forget(&mut self, proxy: usize) {
        self.objects.retain(|(candidate, _)| *candidate != proxy);
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
    // SAFETY: `user_data` is the `*mut Sink` handed to every
    // `wl_proxy_add_dispatcher` call by `Conn::watch`, which comes from
    // `Box::into_raw` and stays live until `WaylandShell::drop`. No Rust
    // reference to that allocation is alive here: the shell only takes one
    // outside the `dispatch_pending`/`roundtrip`/`read_events` calls that can
    // reach this function.
    let sink = unsafe { &mut *user_data.cast::<Sink>().cast_mut() };
    let proxy = target as usize;
    let Some(kind) = sink.kind_of(proxy) else {
        // An event for an object we already destroyed. libwayland can deliver
        // one that was in flight when the destructor was sent; dropping it is
        // the documented behaviour.
        return 0;
    };
    let args: *const WlArgument = args.cast_const();

    // SAFETY (all decoders): `args` is the argument array libwayland built for
    // `opcode` on a proxy of this interface — that is exactly the dispatcher
    // contract — and every borrow is copied out before this function returns,
    // which is the lifetime the decoders document.
    match kind {
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
            // SAFETY: `kind` says this proxy is a `xdg_wm_base`, so `args` is that
            // interface's argument array for `opcode`; every borrow is copied out
            // before this function returns. See the note above the `match`.
            if let Some(xdg_wm_base::Event::Ping { serial }) =
                unsafe { xdg_wm_base::decode_event(opcode, args) }
            {
                sink.events.push(RawEvent::Ping { serial });
            }
        }
        ObjectKind::Output => {
            // SAFETY: `kind` says this proxy is a `wl_output`, so `args` is that
            // interface's argument array for `opcode`; every borrow is copied out
            // before this function returns. See the note above the `match`.
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
        ObjectKind::Surface => {
            // SAFETY: `kind` says this proxy is a `wl_surface`, so `args` is that
            // interface's argument array for `opcode`; every borrow is copied out
            // before this function returns. See the note above the `match`.
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
            // SAFETY: `kind` says this proxy is a `xdg_surface`, so `args` is that
            // interface's argument array for `opcode`; every borrow is copied out
            // before this function returns. See the note above the `match`.
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
            // SAFETY: `kind` says this proxy is a `xdg_toplevel`, so `args` is that
            // interface's argument array for `opcode`; every borrow is copied out
            // before this function returns. See the note above the `match`.
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
    }
    0
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
        self.sink().objects.push((proxy as usize, kind));
        // SAFETY: `proxy` was just created on this connection and has no
        // listener yet; `dispatch` matches libwayland's `wl_dispatcher_func_t`;
        // `self.sink` outlives the proxy because `WaylandShell::drop` destroys
        // every proxy before freeing it.
        unsafe {
            (self.lib.proxy_add_dispatcher)(proxy, dispatch, self.sink.cast(), ptr::null_mut());
        }
    }

    /// Destroys a proxy and stops routing its events.
    fn destroy(&mut self, proxy: *mut WlProxy) {
        if proxy.is_null() {
            return;
        }
        self.sink().forget(proxy as usize);
        // SAFETY: `proxy` is live on this connection and is not used again —
        // every caller clears its own copy of the pointer.
        unsafe { (self.lib.proxy_destroy)(proxy) };
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
    fn drain(&self, timeout_ms: c_int) -> Result<(), ShellError> {
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
            if !ffi::poll_readable(self.fd, timeout_ms) {
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
    /// `wl_registry.global`'s name, for the matching `global_remove`.
    global: u32,
    id: MonitorId,
    x: i32,
    y: i32,
    width: i32,
    height: i32,
    refresh_millihertz: u32,
    scale: i32,
    name: String,
    /// Whether a `wl_output.done` has been seen, i.e. whether the fields above
    /// are a consistent snapshot rather than a half-applied update.
    settled: bool,
}

impl Output {
    fn info(&self, is_primary: bool) -> MonitorInfo {
        let width = self.width.max(0).unsigned_abs();
        let height = self.height.max(0).unsigned_abs();
        MonitorInfo {
            id: self.id,
            name: self.name.clone(),
            // No `xdg_output`, so the position is `wl_output.geometry`'s, which
            // is in compositor-global *logical* coordinates while the size is
            // in physical pixels. They agree at scale 1 and disagree above it.
            // Nothing may treat this as a reliable desktop layout — the seam
            // says so through `ShellCaps::WINDOW_POSITION`, which this backend
            // does not set.
            bounds: PhysicalRect::new(self.x, self.y, width, height),
            // Wayland has no work-area protocol at all. `MonitorInfo` documents
            // that a backend which cannot find out reports the full area.
            work_area: PhysicalRect::new(self.x, self.y, width, height),
            scale_factor: f64::from(self.scale.max(1)),
            refresh_millihertz: self.refresh_millihertz,
            is_primary,
        }
    }
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
    title: String,
    requested_size: LogicalSize,
    requested_mode: DisplayMode,
    requested_constraints: SizeConstraints,
    configuration: Option<WindowConfiguration>,
    pending: PendingConfigure,
    /// Outputs the surface is on, in `wl_surface.enter` order. Empty until the
    /// surface is mapped, which is why the first configure is always scale 1.
    outputs: Vec<usize>,
    scale: i32,
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
}

// ---------------------------------------------------------------------------
// The shell
// ---------------------------------------------------------------------------

/// A [`Shell`] backed by a real Wayland compositor.
///
/// Constructed through [`open`](crate::open) or
/// [`open_backend`](crate::open_backend); see the [module docs](self) for what
/// this slice implements and how a real compositor differs from
/// [`HeadlessShell`](crate::HeadlessShell).
pub struct WaylandShell {
    conn: Conn,
    windows: Pool<WlWindow>,
    outputs: Vec<Output>,
    monitors: Vec<MonitorInfo>,
    next_monitor_id: u32,
    queue: VecDeque<ShellEvent>,
    /// The epoch every event timestamp is measured from.
    ///
    /// Nothing reads it yet because no [`ShellEvent`] this slice produces
    /// carries a timestamp — every timestamped variant is an input event, and
    /// input is P0.5b. It is established and tested now so the epoch contract
    /// is settled before the first `wl_pointer.motion` rather than after.
    #[allow(dead_code, reason = "P0.5b's input events are the first readers")]
    time: TimeBase,
    lost: Option<String>,
}

impl core::fmt::Debug for WaylandShell {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        f.debug_struct("WaylandShell")
            .field("windows", &self.windows.len())
            .field("monitors", &self.monitors.len())
            .field("queued_events", &self.queue.len())
            .field("connected", &self.lost.is_none())
            .finish()
    }
}

impl WaylandShell {
    /// Connects to the compositor named by `WAYLAND_DISPLAY`.
    ///
    /// Binds `wl_compositor`, `xdg_wm_base` and every `wl_output`, and
    /// round-trips twice: once for the registry listing, once for the output
    /// properties. Blocking here is correct — there is no frame loop yet, and
    /// the alternative is a shell that reports no monitors for the first few
    /// frames.
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
            sink: Box::into_raw(Box::new(Sink::default())),
        };

        let mut shell = Self {
            conn,
            windows: Pool::new(),
            outputs: Vec::new(),
            monitors: Vec::new(),
            next_monitor_id: 1,
            queue: VecDeque::new(),
            time: TimeBase::now(),
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
        // Second: the properties of everything bound in the first.
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
        // The registry listing and the initial output properties are startup
        // facts, not events anyone asked for.
        shell.queue.clear();
        Ok(shell)
    }

    /// Aligns event timestamps with an engine [`TimeSource`](crcbl_core::TimeSource).
    ///
    /// [`EventTime`]'s epoch is "the same origin as the `TimeSource` driving
    /// [`FrameClock`](crcbl_core::FrameClock)", which this backend cannot know:
    /// it is created some time after the clock is. Pass `time.elapsed()` once,
    /// at startup, and the offset is removed. Without it, timestamps are
    /// measured from the shell's own creation — off by however long startup
    /// took, and consistent, which is enough for durations but not for
    /// comparing against a frame time.
    ///
    /// Mirrors [`HeadlessShell::set_time`](crate::HeadlessShell::set_time).
    #[allow(
        dead_code,
        reason = "the engine loop that owns the TimeSource lands after this slice"
    )]
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

    /// Binds one global, if it is one we want.
    fn bind_global(&mut self, name: u32, interface: &str, version: u32) {
        let registry = self.conn.registry;
        match interface {
            "wl_compositor" if self.conn.compositor.is_null() => {
                // SAFETY: `registry` is live, and `bind` marshals the interface
                // descriptor's own name and the version we ask for.
                self.conn.compositor = unsafe {
                    wl_registry::bind(
                        registry,
                        name,
                        &wl_compositor::INTERFACE,
                        version.min(COMPOSITOR_VERSION),
                    )
                };
            }
            "xdg_wm_base" if self.conn.wm_base.is_null() => {
                // SAFETY: as above.
                let proxy = unsafe {
                    wl_registry::bind(
                        registry,
                        name,
                        &xdg_wm_base::INTERFACE,
                        version.min(WM_BASE_VERSION),
                    )
                };
                self.conn.wm_base = proxy;
                self.conn.watch(proxy, ObjectKind::WmBase);
            }
            "wl_output" => {
                // SAFETY: as above.
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
                    global: name,
                    id,
                    x: 0,
                    y: 0,
                    width: 0,
                    height: 0,
                    refresh_millihertz: 0,
                    scale: 1,
                    name: format!("wl-output-{}", id.0),
                    settled: false,
                });
            }
            _ => {}
        }
    }

    fn output_mut(&mut self, proxy: usize) -> Option<&mut Output> {
        self.outputs.iter_mut().find(|output| output.proxy == proxy)
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
                        self.conn.destroy(output.proxy as *mut WlProxy);
                        monitors_dirty = true;
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
            }
        }
        if monitors_dirty {
            self.republish_monitors();
        }
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
        let surface_proxy = window.surface;
        // SAFETY: the surface is live and `set_buffer_scale` takes one int.
        unsafe { wl_surface::set_buffer_scale(surface_proxy, scale) };

        let Some(config) = window.configuration else {
            // Not configured yet: the scale will be folded into the first
            // configure rather than announced on its own.
            return;
        };
        let logical = window.logical_size();
        let size = logical.to_physical(f64::from(scale));
        window.configuration = Some(WindowConfiguration {
            size,
            scale_factor: f64::from(scale),
            mode: config.mode,
        });
        let surface_proxy = window.surface;
        // SAFETY: the surface is live; a scale change is only in effect after a
        // commit.
        unsafe { wl_surface::commit(surface_proxy) };
        self.queue.push_back(ShellEvent::ScaleFactorChanged {
            window: id,
            scale_factor: f64::from(scale),
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
        let Ok(window) = self.window_mut(id) else {
            return;
        };

        let fullscreen = window
            .pending
            .states
            .contains(&xdg_toplevel::state::FULLSCREEN);
        let mode = if fullscreen {
            DisplayMode::Borderless { monitor: None }
        } else {
            DisplayMode::Windowed
        };
        let scale_factor = f64::from(window.scale.max(1));
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
        // `xdg_toplevel.state.activated` is the only focus signal there is
        // without a `wl_seat`, and it is the one a window manager sets. Compare
        // against the previous value rather than emitting on every configure: a
        // compositor restates the full state set every time, so a backend that
        // did not diff would deliver a `Focus` per resize.
        let focused = window
            .pending
            .states
            .contains(&xdg_toplevel::state::ACTIVATED);
        let focus_changed = window.focused != focused;
        window.focused = focused;
        let xdg = window.xdg_surface;
        let surface = window.surface;

        // SAFETY: both proxies are live. `ack_configure` must precede the
        // commit, and the commit is what makes the acknowledged state current.
        unsafe {
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
        if focus_changed {
            self.queue.push_back(ShellEvent::Focus {
                window: id,
                focused,
            });
        }
    }

    /// Tears one window's objects down, innermost first.
    fn destroy_objects(&mut self, window: &WlWindow) {
        // Order matters: `xdg_toplevel` before `xdg_surface` before
        // `wl_surface`. Destroying a `wl_surface` that still has a role object
        // is a protocol error and disconnects the client.
        self.conn.destroy(window.toplevel);
        self.conn.destroy(window.xdg_surface);
        self.conn.destroy(window.surface);
        self.conn.flush();
    }
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
                self.destroy_objects(&window);
            }
        }
        let (wm_base, compositor, registry) =
            (self.conn.wm_base, self.conn.compositor, self.conn.registry);
        self.conn.destroy(wm_base);
        self.conn.destroy(compositor);
        self.conn.destroy(registry);
        self.conn.flush();
        // SAFETY: the display was returned live by `wl_display_connect` and no
        // proxy on it survives — every window was destroyed above, and the
        // globals with them.
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

    /// What this slice actually implements — no more.
    ///
    /// Capabilities are latched, and a bit set here is a promise to every
    /// consumer that checked it at startup. `POINTER_LOCK`, `RAW_POINTER_MOTION`,
    /// `TEXT_IME`, `CLIPBOARD` and `DRAG_DROP` all need a `wl_seat` or a
    /// `wl_data_device` and land in P0.5b/c; `HW_UPSCALE` needs
    /// `wp_viewporter`, `FRACTIONAL_SCALE` needs `fractional-scale-v1`, and
    /// `SERVER_DECORATIONS` needs `xdg-decoration` — none of which this slice
    /// binds. `POINTER_WARP` and `WINDOW_POSITION` are absent permanently:
    /// Wayland forbids both by design.
    fn caps(&self) -> ShellCaps {
        ShellCaps::MULTI_WINDOW | ShellCaps::EVENT_WAIT
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
                title: desc.title.to_string(),
                requested_size: desc.size,
                requested_mode: desc.mode,
                requested_constraints: desc.constraints,
                // Unconfigured, and it stays that way until the compositor
                // answers — a round trip away. This is the contract P0.4 was
                // shaped around and the reason it was shaped that way.
                configuration: None,
                pending: PendingConfigure::default(),
                outputs: Vec::new(),
                scale: 1,
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
        self.destroy_objects(&removed);
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
            pointer_mode: PointerMode::Free,
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

    /// Records the requested visibility.
    ///
    /// Wayland has no hide request: a surface is mapped exactly while it has a
    /// buffer, so unmapping means `wl_surface.attach(null)` and mapping means
    /// attaching a real one. Neither is possible in this slice, which never
    /// attaches a buffer — the renderer does, at P1. Until then this records
    /// the intent and [`WindowState::visible`] reports it, which is what
    /// `HeadlessShell` does too.
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
        // SAFETY: both proxies are live; the sizes are plain integers.
        unsafe {
            apply_constraints(toplevel, constraints, true, requested);
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
    fn pump(&mut self, sink: &mut dyn FnMut(ShellEvent)) {
        if self.lost.is_none()
            && let Err(error) = self.conn.drain(0)
        {
            log::error!("wayland connection lost: {error}");
            self.lost = Some(error.to_string());
        }
        self.process_raw();
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

    fn wait_events(&mut self, timeout: Option<Duration>) {
        if self.lost.is_some() {
            return;
        }
        let timeout_ms = timeout.map_or(-1, |timeout| {
            c_int::try_from(timeout.as_millis()).unwrap_or(c_int::MAX)
        });
        if let Err(error) = self.conn.drain(timeout_ms) {
            log::error!("wayland connection lost: {error}");
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

    fn set_pointer_mode(&mut self, window: WindowId, mode: PointerMode) -> Result<(), ShellError> {
        self.window(window)?;
        match mode {
            PointerMode::Free => Ok(()),
            PointerMode::Confined => Err(Self::unsupported("pointer confinement")),
            PointerMode::Locked => Err(Self::unsupported("pointer lock")),
        }
    }

    /// Accepted and inert until there is a pointer to set it on.
    ///
    /// A cursor is set through `wl_pointer.set_cursor`, which needs a `wl_seat`
    /// (P0.5b). Failing here instead would make every UI that sets a resize
    /// cursor error-handle a case that is about to start working.
    fn set_cursor(
        &mut self,
        window: WindowId,
        cursor: Option<CursorIcon>,
    ) -> Result<(), ShellError> {
        let _ = cursor;
        self.window(window).map(|_| ())
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

    fn clipboard_offer(
        &mut self,
        window: WindowId,
        offers: &[ClipboardOffer<'_>],
    ) -> Result<(), ShellError> {
        let _ = offers;
        self.window(window)?;
        Err(Self::unsupported("clipboard"))
    }

    fn clipboard_request(
        &mut self,
        window: WindowId,
        mime: MimeType,
    ) -> Result<ClipboardRequestId, ShellError> {
        let _ = mime;
        self.window(window)?;
        Err(Self::unsupported("clipboard"))
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
        // interfaces at all: `bind` matches on these strings, and a wrong one
        // means a global is silently never bound.
        assert_eq!(wl_compositor::NAME, c"wl_compositor");
        assert_eq!(xdg_wm_base::NAME, c"xdg_wm_base");
        assert_eq!(wl_output::NAME, c"wl_output");
        assert_eq!(xdg_toplevel::state::FULLSCREEN, 2);
        assert_eq!(wl_output::mode::CURRENT, 1);
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
}
