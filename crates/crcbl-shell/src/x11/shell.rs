//! The [`Shell`] implementation, and the window lifecycle it drives.

use core::ffi::{c_int, c_void};
use core::time::Duration;
use std::ffi::CString;

use crcbl_core::SurfaceTarget;

use super::{
    ClipboardOffer, ClipboardRequestId, CloseReply, CursorIcon, DisplayMode, MimeType, MonitorInfo,
    PhysicalPoint, PointerMode, Shell, ShellBackend, ShellCaps, ShellError, ShellEvent,
    SizeConstraints, TRANSFER_POLL, WindowConfiguration, WindowDesc, WindowId, WindowState,
    X11Shell, XWindow, ffi,
};

/// Everything a window wants to hear about.
///
/// One mask, set at creation and never changed, because every one of these is
/// something the seam has an event for. `PROPERTY_CHANGE` is the one that is
/// not obviously input: it carries both halves of an `INCR` clipboard transfer
/// *and* the window manager's `_NET_WM_STATE` writes, which is how the
/// effective display mode is learned.
const WINDOW_EVENT_MASK: u32 = ffi::event_mask::KEY_PRESS
    | ffi::event_mask::KEY_RELEASE
    | ffi::event_mask::BUTTON_PRESS
    | ffi::event_mask::BUTTON_RELEASE
    | ffi::event_mask::ENTER_WINDOW
    | ffi::event_mask::LEAVE_WINDOW
    | ffi::event_mask::POINTER_MOTION
    | ffi::event_mask::EXPOSURE
    | ffi::event_mask::STRUCTURE_NOTIFY
    | ffi::event_mask::FOCUS_CHANGE
    | ffi::event_mask::PROPERTY_CHANGE;

/// What a pointer grab asks to keep receiving.
///
/// A grab **redirects** the events in this mask to the grab window, so anything
/// left out simply stops arriving for the duration — a grab that forgot
/// `BUTTON_RELEASE` leaves the button stuck down from the consumer's point of
/// view for as long as the pointer is captured.
const GRAB_EVENT_MASK: u16 = (ffi::event_mask::BUTTON_PRESS
    | ffi::event_mask::BUTTON_RELEASE
    | ffi::event_mask::POINTER_MOTION
    | ffi::event_mask::ENTER_WINDOW
    | ffi::event_mask::LEAVE_WINDOW) as u16;

impl X11Shell {
    /// Publishes configurations the server has already told us about.
    ///
    /// The one place X11's synchronous geometry meets P0.4's asynchronous
    /// contract; see [`create_window`](Shell::create_window).
    fn publish_configurations(&mut self) {
        let ready: Vec<(WindowId, WindowConfiguration)> = self
            .windows
            .iter_mut()
            .filter_map(|(handle, window)| {
                window.pending.take().map(|config| (handle.cast(), config))
            })
            .collect();
        for (window, config) in ready {
            let previous = self
                .windows
                .get(window.cast())
                .and_then(|state| state.configuration);
            // The effective mode is the window manager's answer, not ours, and
            // it is tracked separately from the size — so a configure must not
            // overwrite it with a guess.
            let mode = self
                .windows
                .get(window.cast())
                .map_or(DisplayMode::Windowed, XWindow::effective_mode);
            let config = WindowConfiguration { mode, ..config };
            if let Some(state) = self.windows.get_mut(window.cast()) {
                state.configuration = Some(config);
            }
            if previous.is_some_and(|previous| {
                (previous.scale_factor - config.scale_factor).abs() > f64::EPSILON
            }) {
                self.queue.push_back(ShellEvent::ScaleFactorChanged {
                    window,
                    scale_factor: config.scale_factor,
                    size: config.size,
                });
            }
            self.queue.push_back(ShellEvent::Resized {
                window,
                size: config.size,
                scale_factor: config.scale_factor,
            });
        }
    }

    /// Re-reads `_NET_WM_STATE` and works out which monitor the window is on.
    ///
    /// The *effective* mode is a fact only a window manager can state, so it is
    /// read back rather than assumed. Under bare `Xvfb` there is no window
    /// manager, the property never appears, and the effective mode stays
    /// [`Windowed`](DisplayMode::Windowed) however many fullscreen requests are
    /// made — which is the honest answer and exactly what
    /// [`WindowState::mode_request_honoured`] exists to report.
    pub(super) fn refresh_effective_mode(&mut self, window: WindowId) {
        let Ok(state) = self.window(window) else {
            return;
        };
        let xid = state.id;
        // **`_NET_WM_STATE` is only an answer when somebody else writes it.**
        // Before a window is first mapped, EWMH has the *client* write the
        // property to request an initial state — see `apply_fullscreen` — and a
        // window manager then takes ownership and rewrites it. With no window
        // manager there is nobody to take ownership, so reading the property
        // back returns this process's own request, and reporting that as the
        // effective mode would make `mode_request_honoured` true for a request
        // nothing honoured. A bare X session, a kiosk and every `Xvfb` in CI are
        // all that case.
        let fullscreen = self.has_window_manager
            && self
                .conn
                .get_property_words(xid, self.conn.atoms.net_wm_state)
                .contains(&self.conn.atoms.net_wm_state_fullscreen);
        let monitor = if fullscreen {
            self.origin_now(xid).and_then(|(x, y)| {
                self.monitors
                    .iter()
                    .find(|info| info.bounds.contains(x, y))
                    .map(|info| info.id)
            })
        } else {
            None
        };
        if let Ok(state) = self.window_mut(window) {
            state.effective_fullscreen = fullscreen;
            state.effective_monitor = monitor;
            let mode = state.effective_mode();
            if let Some(config) = state.configuration.as_mut() {
                config.mode = mode;
            }
        }
    }
}

impl XWindow {
    /// The mode the window manager has actually put this window in.
    pub(super) fn effective_mode(&self) -> DisplayMode {
        if self.effective_fullscreen {
            DisplayMode::Borderless {
                monitor: self.effective_monitor,
            }
        } else {
            DisplayMode::Windowed
        }
    }
}

impl Shell for X11Shell {
    fn backend(&self) -> ShellBackend {
        ShellBackend::X11
    }

    /// What this backend can do, latched at [`open`](X11Shell::open).
    ///
    /// Three of these are set here for the first time by any backend, and each
    /// is worth stating precisely, because a capability that overstates itself
    /// is worse than one that is missing:
    ///
    /// * [`ASPECT_HINT_HONORED`](ShellCaps::ASPECT_HINT_HONORED). X11 is the
    ///   only platform in this engine with an aspect hint — `WM_NORMAL_HINTS`
    ///   carries `min_aspect` and `max_aspect`, and setting both to one ratio
    ///   is a lock. The bit is **not** set unconditionally: it is set only when
    ///   an EWMH window manager is running, because with no window manager the
    ///   property is one nobody reads. That is not a headless-CI edge case, it
    ///   is the honest state of a bare X session.
    /// * [`WINDOW_POSITION`](ShellCaps::WINDOW_POSITION). Every monitor is a
    ///   rectangle in one device-pixel space, a window's position is in the
    ///   same space, and `ConfigureWindow` moves it — none of which is true on
    ///   Wayland. What that buys **today** is
    ///   [`DisplayMode::Borderless`] with a named monitor, which this backend
    ///   honours and the Wayland one can only hint at, plus a
    ///   [`MonitorInfo::bounds`] that tiles. The seam has no `set_position` yet
    ///   — when it grows one, this is the bit that says it will work.
    /// * [`FRACTIONAL_SCALE`](ShellCaps::FRACTIONAL_SCALE). Not a protocol:
    ///   `Xft.dpi` is an arbitrary integer in a text property, so 1.25 is an
    ///   ordinary value. See [`connect`](super::connect) for why RandR's
    ///   physical millimetres are deliberately not used instead.
    ///
    /// And two that are clear for reasons rather than for want of work:
    /// [`HW_UPSCALE`](ShellCaps::HW_UPSCALE), because X11 has no viewporter and
    /// no equivalent — the renderer does an upscale blit, which is the exact
    /// decision `docs/plan/15-windowing.md` names caps for — and
    /// [`DRAG_DROP`](ShellCaps::DRAG_DROP), which is XDND and is cut from this
    /// slice.
    fn caps(&self) -> ShellCaps {
        self.caps
    }

    /// Creates a window. **The window has no size yet.**
    ///
    /// # X11 knows the size, and this still returns without one
    ///
    /// `CreateWindow` takes a width and a height and the server keeps them, so
    /// unlike Wayland there is no round trip and no "0 × 0 means you choose".
    /// The size is known before this function returns.
    ///
    /// It is not *reported* before this function returns, because
    /// [`WindowState::size`] is defined as `None` until a
    /// [`Resized`](ShellEvent::Resized) has been delivered, and delivering
    /// events is [`pump`](Shell::pump)'s job. So the create-then-wait loop is
    /// real here as well — it just completes on the **first pump** rather than
    /// after a round trip. Nothing is delayed to look symmetrical with Wayland;
    /// the wait is exactly as long as the platform makes it.
    ///
    /// # Errors
    ///
    /// [`ShellError::NoSuchMonitor`] for a borderless request naming a monitor
    /// that is gone, [`ShellError::InvalidDescriptor`] for a title or app id
    /// containing a NUL byte, and [`ShellError::WindowCreation`] if the server
    /// refused.
    fn create_window(&mut self, desc: &WindowDesc<'_>) -> Result<WindowId, ShellError> {
        if let DisplayMode::Borderless {
            monitor: Some(monitor),
        } = desc.mode
            && self.monitor(monitor).is_none()
        {
            return Err(ShellError::NoSuchMonitor(monitor.0));
        }
        // Validated rather than used: X11 properties are counted byte strings,
        // so an embedded NUL is legal on the wire and would silently truncate
        // the title in every window manager that reads it as a C string. The
        // seam's other backends reject it, so this one does too.
        CString::new(desc.title)
            .map_err(|_| ShellError::InvalidDescriptor("title contains a NUL byte".to_string()))?;
        CString::new(desc.app_id)
            .map_err(|_| ShellError::InvalidDescriptor("app_id contains a NUL byte".to_string()))?;

        let size = desc.size.to_physical(self.scale_factor);
        if size.is_empty() {
            return Err(ShellError::InvalidDescriptor(
                "size rounds to zero physical pixels".to_string(),
            ));
        }
        let xid = self.conn.generate_id();
        // The value list is read in **mask-bit order**, one word per set bit:
        // `BACK_PIXEL` is bit 1 and `EVENT_MASK` is bit 11, so they go in that
        // order. Getting the order wrong makes the event mask the background
        // colour, which produces a window that receives nothing.
        let values = [0u32, WINDOW_EVENT_MASK];
        // SAFETY: the connection is live, `xid` is fresh from
        // `xcb_generate_id`, the parent is the root of the screen this
        // connection selected, and `values` has exactly the two words the mask
        // declares and outlives the call.
        unsafe {
            (self.conn.lib.create_window)(
                self.conn.raw(),
                ffi::value::COPY_FROM_PARENT as u8,
                xid,
                self.conn.root,
                0,
                0,
                u16::try_from(size.width).unwrap_or(u16::MAX),
                u16::try_from(size.height).unwrap_or(u16::MAX),
                0,
                ffi::value::INPUT_OUTPUT,
                self.conn.root_visual,
                ffi::cw::BACK_PIXEL | ffi::cw::EVENT_MASK,
                values.as_ptr().cast::<c_void>(),
            );
        }

        let mut window = XWindow {
            id: xid,
            title: desc.title.to_string(),
            requested_size: desc.size,
            requested_mode: desc.mode,
            requested_constraints: desc.constraints,
            resizable: desc.resizable,
            // Unconfigured until the first pump; see the doc comment above.
            configuration: None,
            pending: None,
            last_size: None,
            effective_fullscreen: false,
            effective_monitor: None,
            pointer_mode: PointerMode::Free,
            cursor: None,
            pointer_inside: false,
            focused: false,
            visible: desc.visible,
            mapped: false,
            map_requested: false,
            close_pending: false,
        };

        self.write_title(xid, desc.title);
        self.write_identity(xid, desc.app_id);
        self.write_size_hints(&window);
        if let DisplayMode::Borderless { monitor } = desc.mode {
            if let Some(monitor) = monitor {
                self.move_to_monitor(xid, monitor)?;
            }
            // Before the first map, so the property *is* the request — see
            // [`apply_fullscreen`](X11Shell::apply_fullscreen).
            self.apply_fullscreen(&window, true);
        }
        if desc.visible {
            // SAFETY: the connection and window are live.
            unsafe { (self.conn.lib.map_window)(self.conn.raw(), xid) };
            // From here the window manager is managing it, whatever `MapNotify`
            // has or has not arrived — see `apply_fullscreen`.
            window.map_requested = true;
        }
        self.conn.flush();

        window.pending = self.geometry_now(xid, DisplayMode::Windowed);
        if window.pending.is_none() {
            self.conn.destroy_x_window(xid);
            self.conn.flush();
            return Err(ShellError::WindowCreation(
                "the server refused CreateWindow".to_string(),
            ));
        }
        Ok(self.windows.insert(window).cast())
    }

    fn destroy_window(&mut self, window: WindowId) -> Result<(), ShellError> {
        let removed = self
            .windows
            .remove(window.cast())
            .ok_or_else(|| ShellError::invalid_window(window))?;
        self.forget_selection_state(window, removed.id);
        self.conn.destroy_x_window(removed.id);
        self.conn.flush();
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
        CString::new(title)
            .map_err(|_| ShellError::InvalidDescriptor("title contains a NUL byte".to_string()))?;
        let xid = self.window(window)?.id;
        self.window_mut(window)?.title = title.to_string();
        self.write_title(xid, title);
        self.conn.flush();
        Ok(())
    }

    /// Maps or unmaps the window — **really**, unlike on Wayland.
    ///
    /// `MapWindow` and `UnmapWindow` are a matched pair and an unmapped window
    /// can be mapped again with no state lost, so this is one of the few places
    /// the X11 backend does something the Wayland one documents that it cannot:
    /// there, a surface is mapped exactly while it has a buffer, buffers are
    /// the renderer's, and hiding a window would make it unshowable.
    ///
    /// [`WindowState::visible`] follows the server's `MapNotify`/`UnmapNotify`
    /// rather than this call, so it reports what happened rather than what was
    /// asked.
    fn set_visible(&mut self, window: WindowId, visible: bool) -> Result<(), ShellError> {
        let xid = self.window(window)?.id;
        // SAFETY: the connection and window are live; mapping an already-mapped
        // window is a no-op on the server.
        unsafe {
            if visible {
                (self.conn.lib.map_window)(self.conn.raw(), xid);
            } else {
                (self.conn.lib.unmap_window)(self.conn.raw(), xid);
            }
        }
        // Which side of `apply_fullscreen`'s split the next request takes. Set
        // from the request rather than from the notify, and deliberately —
        // `visible` below is the one that reports what the server did.
        self.window_mut(window)?.map_requested = visible;
        self.conn.flush();
        Ok(())
    }

    /// Asks for windowed or borderless.
    ///
    /// A request, and on X11 a request to a program that may not be running:
    /// with no window manager the client message goes nowhere and
    /// [`WindowState::mode_request_honoured`] stays `false` forever. That is
    /// the correct behaviour and the reason the seam separates requested from
    /// effective in the first place.
    fn set_mode(&mut self, window: WindowId, mode: DisplayMode) -> Result<(), ShellError> {
        if let DisplayMode::Borderless {
            monitor: Some(monitor),
        } = mode
            && self.monitor(monitor).is_none()
        {
            return Err(ShellError::NoSuchMonitor(monitor.0));
        }
        let xid = self.window(window)?.id;
        self.window_mut(window)?.requested_mode = mode;
        if let DisplayMode::Borderless {
            monitor: Some(monitor),
        } = mode
        {
            // Before the state change: a window manager makes a window
            // fullscreen on whichever monitor it currently overlaps most, so
            // moving it afterwards would fullscreen the wrong one and then
            // shove the fullscreen window sideways.
            self.move_to_monitor(xid, monitor)?;
        }
        let state = self.window(window)?;
        self.apply_fullscreen(state, mode.is_borderless());
        self.conn.flush();
        Ok(())
    }

    fn set_constraints(
        &mut self,
        window: WindowId,
        constraints: SizeConstraints,
    ) -> Result<(), ShellError> {
        self.window_mut(window)?.requested_constraints = constraints;
        let state = self.window(window)?;
        self.write_size_hints(state);
        self.conn.flush();
        Ok(())
    }

    fn monitors(&self) -> &[MonitorInfo] {
        &self.monitors
    }

    /// Drains the connection without blocking, then delivers what arrived.
    ///
    /// The order matters and is the same as the Wayland backend's, for the same
    /// reasons: the socket first, so a `SelectionRequest` that arrived in this
    /// batch has already been answered when a read of our own selection looks
    /// for its reply; then the transfer deadlines, so a stalled paste is
    /// answered in the frame it expired rather than the frame after; then the
    /// configurations, so a resize and its scale change land together; then
    /// delivery.
    fn pump(&mut self, sink: &mut dyn FnMut(ShellEvent)) {
        if self.lost.is_none() {
            self.drain();
        }
        self.service_transfers();
        self.publish_configurations();
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

    /// Blocks until something arrives on the connection or `timeout` elapses.
    ///
    /// Capped while a selection transfer is outstanding, so that a peer which
    /// takes the selection and then says nothing has its deadline *checked* —
    /// without the cap an editor idling with `timeout: None` would sleep
    /// straight through a stalled paste's timeout and never answer it, which is
    /// obligation 4 violated by omission.
    fn wait_events(&mut self, timeout: Option<Duration>) {
        if self.lost.is_some() {
            return;
        }
        let mut deadline = timeout;
        if !self.reads.is_empty() || !self.writes.is_empty() {
            deadline = Some(deadline.map_or(TRANSFER_POLL, |deadline| deadline.min(TRANSFER_POLL)));
        }
        let timeout_ms = deadline.map_or(-1, |deadline| {
            c_int::try_from(deadline.as_millis()).unwrap_or(c_int::MAX)
        });
        // Everything libxcb had queued was drained by the last `pump`, so the
        // descriptor is the whole truth about whether anything is pending. The
        // answer is discarded on purpose: `wait_events` is advisory, and
        // `pump` is what delivers.
        let _ = ffi::poll_readable(self.conn.fd, timeout_ms);
    }

    fn align_event_clock(&mut self, elapsed: Duration) {
        self.align_time_base(elapsed);
    }

    /// The handles `crcbl-vk` needs for `vkCreateXcbSurfaceKHR`.
    ///
    /// Available the instant the window exists — X11 creates a real, sized
    /// window synchronously — so a HAL surface can be created immediately and
    /// only the swapchain waits for the first
    /// [`Resized`](ShellEvent::Resized).
    ///
    /// The `visual_id` is the **root visual**, which is what
    /// [`create_window`](Shell::create_window) passes and therefore what the
    /// window actually has. A backend that guessed a 32-bit ARGB visual here
    /// would hand Vulkan a format the window is not in, and
    /// `vkGetPhysicalDeviceXcbPresentationSupportKHR` would answer for the
    /// wrong one.
    ///
    /// Lifetime: the connection lives as long as this shell and the window as
    /// long as the [`WindowId`], so a HAL surface built from these must be
    /// destroyed before [`destroy_window`](Shell::destroy_window) and before
    /// the shell is dropped.
    fn surface_target(&self, window: WindowId) -> Result<SurfaceTarget, ShellError> {
        let state = self.window(window)?;
        Ok(SurfaceTarget::Xcb {
            connection: self.conn.connection.cast::<c_void>(),
            window: state.id,
            visual_id: self.conn.root_visual,
        })
    }

    /// Frees, confines or locks the pointer.
    ///
    /// # How the two captured modes are built
    ///
    /// Both are `GrabPointer` with `confine_to` set to the window, which is
    /// core X11 and needs no extension — the reason `libxcb-xfixes` and its
    /// pointer barriers are not loaded. The difference is what the seam
    /// promises about each:
    ///
    /// * [`Confined`](PointerMode::Confined) keeps the cursor visible and
    ///   [`abs`](ShellEvent::PointerMotion) flowing. The grab is the whole
    ///   implementation.
    /// * [`Locked`](PointerMode::Locked) additionally hides the cursor and
    ///   suppresses `abs`, which [`ShellEvent::PointerMotion`] documents as the
    ///   invariant of that mode, and the camera reads XI2 raw motion instead.
    ///   The pointer is re-centred when it drifts near an edge — see
    ///   [`input`](super::input) — because a confined pointer can still come to
    ///   rest against one.
    ///
    /// An X11 grab is not guaranteed: another client may already hold one (a
    /// window manager during an alt-tab, a screen locker), and the server says
    /// so synchronously. That is reported as [`ShellError::Backend`] rather
    /// than silently ignored, because a first-person camera that quietly did
    /// not capture the pointer is a bug report about "the mouse escaping".
    ///
    /// # Nothing is released until the replacement is held
    ///
    /// There is no pre-emptive `UngrabPointer`. Two things went wrong with one:
    /// a failed `GrabPointer` returned `Err` after the previous grab was already
    /// gone — so no grab was held while [`WindowState::pointer_mode`] still said
    /// `Locked` — and it broke a grab this same shell held for a *different*
    /// window. X11 lets the client that holds the grab re-grab with new
    /// parameters, so a successful `GrabPointer` replaces ours by itself, and a
    /// failed one leaves everything exactly as it was.
    ///
    /// # Errors
    ///
    /// [`ShellError::InvalidWindow`] for a stale handle, or
    /// [`ShellError::Backend`] if another client holds the pointer.
    fn set_pointer_mode(&mut self, window: WindowId, mode: PointerMode) -> Result<(), ShellError> {
        let state = self.window(window)?;
        let xid = state.id;
        let hidden = mode == PointerMode::Locked || state.cursor == Some(None);
        if !self.caps.contains(mode.required_cap()) {
            return Err(Self::unsupported(mode.as_str()));
        }

        if mode == PointerMode::Free {
            // Only if *this* window is the one holding the grab. A shell with
            // two windows must not have one of them free the other's capture,
            // and the grab is per-client, so an unconditional ungrab did.
            if self.grab_holder() == Some(window) {
                // SAFETY: the connection is live; ungrabbing when nothing is
                // grabbed is a no-op on the server.
                unsafe {
                    (self.conn.lib.ungrab_pointer)(self.conn.raw(), ffi::value::CURRENT_TIME);
                };
            }
            self.window_mut(window)?.pointer_mode = mode;
            let restore = self.window(window)?.cursor == Some(None);
            self.apply_cursor(xid, restore);
            self.conn.flush();
            return Ok(());
        }

        let cursor = if hidden {
            self.blank_cursor()
        } else {
            ffi::value::NONE
        };
        // SAFETY: the connection, window and cursor are live. `owner_events =
        // 1` keeps our own windows receiving events normally; both grab modes
        // are `Async`, which is what a game wants — a synchronous grab freezes
        // event processing until the client replays each event.
        let reply = unsafe {
            let cookie = (self.conn.lib.grab_pointer)(
                self.conn.raw(),
                1,
                xid,
                GRAB_EVENT_MASK,
                ffi::value::GRAB_MODE_ASYNC,
                ffi::value::GRAB_MODE_ASYNC,
                xid,
                cursor,
                ffi::value::CURRENT_TIME,
            );
            (self.conn.lib.grab_pointer_reply)(self.conn.raw(), cookie, core::ptr::null_mut())
        };
        if reply.is_null() {
            return Err(ShellError::Backend(
                "the X server did not answer GrabPointer".to_string(),
            ));
        }
        // SAFETY: `reply` is a live reply this call owns.
        let status = unsafe { (*reply).status };
        // SAFETY: freed exactly once.
        unsafe { ffi::free_reply(reply) };
        if status != ffi::value::GRAB_SUCCESS {
            // Nothing was released to get here, so nothing has to be put back:
            // whatever capture this shell held is still held, and the recorded
            // `pointer_mode` still describes it.
            return Err(ShellError::Backend(format!(
                "another client holds the pointer (GrabPointer status {status})"
            )));
        }

        // The grab replaced any previous one, so no other window of this shell
        // still holds a capture.
        let previous: Vec<WindowId> = self
            .windows
            .iter()
            .filter(|(_, state)| state.pointer_mode.is_captured())
            .map(|(handle, _)| handle.cast())
            .filter(|held| *held != window)
            .collect();
        for held in previous {
            if let Ok(state) = self.window_mut(held) {
                state.pointer_mode = PointerMode::Free;
            }
        }
        self.window_mut(window)?.pointer_mode = mode;
        if mode == PointerMode::Locked
            && let Some(size) = self.window(window)?.configuration.map(|config| config.size)
        {
            // Start from the middle, so the first re-centre is not immediate.
            self.conn.warp_to(
                xid,
                i32::try_from(size.width / 2).unwrap_or(0),
                i32::try_from(size.height / 2).unwrap_or(0),
            );
        }
        self.conn.flush();
        Ok(())
    }

    /// Hides the cursor with `None`. **A shape is recorded and not yet
    /// applied.**
    ///
    /// Hiding is real and complete: a cursor built from an empty 1×1 pixmap
    /// draws nothing, which is core protocol and works on every server — and it
    /// is the case a first-person game and [`PointerMode::Locked`] actually
    /// need.
    ///
    /// A *shape* is the same gap the Wayland backend documents, arrived at from
    /// the other side. There, naming a shape needs `cursor-shape-v1`; here it
    /// needs an XCursor theme loader — find the user's theme, parse its cursor
    /// files, pick the image for the right shape at the right size, upload it —
    /// or `libxcb-cursor`, which is that loader as a library and is a
    /// dependency for a request this engine will not send until the editor
    /// needs resize cursors at P10. So a shape request is accepted and
    /// recorded, [`WindowState`] can report it, and the pair closes in the same
    /// slice on both backends.
    fn set_cursor(
        &mut self,
        window: WindowId,
        cursor: Option<CursorIcon>,
    ) -> Result<(), ShellError> {
        let xid = self.window(window)?.id;
        self.window_mut(window)?.cursor = Some(cursor);
        let locked = self.window(window)?.pointer_mode == PointerMode::Locked;
        self.apply_cursor(xid, cursor.is_none() || locked);
        self.conn.flush();
        Ok(())
    }

    /// Moves the pointer to a position in the window.
    ///
    /// `XWarpPointer`, which is the capability
    /// [`POINTER_WARP`](ShellCaps::POINTER_WARP) names and which Wayland
    /// deliberately does not have. The warning on the trait method still
    /// applies here and is worth repeating: a camera must not be built on this.
    /// A warp generates a `MotionNotify` indistinguishable from the user's own,
    /// so a warp-based camera fights every real movement, which is why
    /// [`PointerMode::Locked`] plus raw motion exists.
    ///
    /// # Errors
    ///
    /// [`ShellError::InvalidWindow`] for a stale handle.
    fn warp_pointer(
        &mut self,
        window: WindowId,
        position: PhysicalPoint,
    ) -> Result<(), ShellError> {
        let xid = self.window(window)?.id;
        self.conn.warp_to(xid, position.x as i32, position.y as i32);
        Ok(())
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
            CloseReply::Keep => Ok(()),
            CloseReply::Close => self.destroy_window(window),
        }
    }

    /// Claims the `CLIPBOARD` selection, or releases it with an empty slice.
    ///
    /// # X11 never returns `NeedsUserInteraction`, and that is complete
    ///
    /// [Obligation 6](Shell) says a backend on a platform with no
    /// user-interaction rule never returns
    /// [`ShellError::NeedsUserInteraction`]. X11 has no such rule:
    /// `SetSelectionOwner` takes a timestamp and no serial, and the server
    /// validates nothing about it beyond ordering two claimants. So a
    /// background window can take the clipboard here, which Wayland forbids by
    /// design — a real difference in what the two platforms let a program do,
    /// not a gap in this backend.
    ///
    /// What *is* checked is that the claim took effect, by reading the owner
    /// back. See [`Conn::set_selection_owner`](super::Conn::set_selection_owner)
    /// for the race that makes the read-back load-bearing.
    ///
    /// # Errors
    ///
    /// [`ShellError::InvalidWindow`] for a stale handle, or
    /// [`ShellError::Backend`] if the server did not accept the claim.
    fn clipboard_offer(
        &mut self,
        window: WindowId,
        offers: &[ClipboardOffer<'_>],
    ) -> Result<(), ShellError> {
        let xid = self.window(window)?.id;
        if offers.is_empty() {
            self.conn
                .set_selection_owner(ffi::value::NONE, self.last_server_time);
            self.offers.clear();
            self.writes.clear();
            self.owner_window = 0;
            self.owner_time = ffi::value::CURRENT_TIME;
            return Ok(());
        }
        // A format this backend has no atom for cannot be advertised in
        // `TARGETS` and cannot be asked for, so it would be an offer no peer
        // could ever take. Filtering happens **before** the claim, not after
        // it: an offer list of nothing but `MimeType::Other` used to pass the
        // `offers.is_empty()` check on the caller's slice, get emptied by this
        // filter, and still take ownership of `CLIPBOARD` — leaving this
        // process owning the desktop clipboard and answering nothing, `TARGETS`
        // included, until it released it.
        let publishable: Vec<(MimeType, Vec<u8>)> = offers
            .iter()
            .filter(|offer| self.conn.atoms.for_mime(offer.mime) != 0)
            .map(|offer| (offer.mime, offer.bytes.to_vec()))
            .collect();
        if publishable.is_empty() {
            return Err(Self::unsupported(
                "a clipboard offer in which no format has an X11 atom",
            ));
        }
        // A fresh timestamp first: the server silently ignores a claim older
        // than the current owner's, and a process that has been idle has no
        // recent one. See `xselection`'s module docs.
        self.refresh_server_time(xid);
        // That probe is this backend's one synchronous wait, and it drains the
        // connection to get its answer — which can process a `DestroyNotify`
        // for the very window whose XID was captured above. Re-resolve the
        // handle rather than claiming the selection for a window that no longer
        // exists.
        let xid = self.window(window)?.id;
        // The bytes are held here until a peer asks for them, which is what
        // makes the write synchronous on every platform: "claim the selection"
        // completes now, the transfer happens later.
        self.offers = publishable;
        if !self.conn.set_selection_owner(xid, self.last_server_time) {
            self.offers.clear();
            return Err(ShellError::Backend(
                "the X server did not accept our claim on the CLIPBOARD selection".to_string(),
            ));
        }
        self.owner_window = xid;
        self.owner_time = self.last_server_time;
        Ok(())
    }

    /// Asks for the clipboard's content in `mime`.
    ///
    /// # Which obligations this satisfies, and how
    ///
    /// * **Exactly one answer** ([obligation 4](Shell)): a request is either
    ///   answered synchronously — queued as
    ///   [`Empty`](crate::ClipboardContent::Empty) when nobody owns the
    ///   selection — or recorded as a [`Read`](super::selection::Read) that is
    ///   removed from the list in the same expression that emits its answer.
    /// * **Within a bounded time** (also 4): every read carries
    ///   [`selection::TIMEOUT`](super::selection::TIMEOUT), refreshed on
    ///   progress. This is the obligation X11 did **not** get for free — an
    ///   `INCR` transfer whose peer stops answering has no other end, because
    ///   there is no descriptor to notice and no protocol event for it.
    /// * **Held rather than answered empty** ([obligation 5](Shell)): there is
    ///   nothing to hold. `GetSelectionOwner` answers "is there anything there"
    ///   synchronously and with no focus gate, so the wait Wayland has does not
    ///   exist here. [`clipboard_readable`](Shell::clipboard_readable) is
    ///   therefore left at the trait's provided default, which is exactly right
    ///   for this platform — that method's own documentation says so, and this
    ///   backend is the confirmation.
    ///
    /// # Errors
    ///
    /// [`ShellError::InvalidWindow`] for a stale handle. An empty clipboard is
    /// an answer, not an error.
    fn clipboard_request(
        &mut self,
        window: WindowId,
        mime: MimeType,
    ) -> Result<ClipboardRequestId, ShellError> {
        self.window(window)?;
        Ok(self.start_read(window, mime))
    }
}

impl Drop for X11Shell {
    /// Releases the selection, destroys every window and closes the connection.
    ///
    /// The order matters for the selection specifically: a client that
    /// disconnects while owning it leaves the server naming a dead window until
    /// something else claims it, and every paste from another application in
    /// that window fails. Releasing first is one request and closes the hole.
    fn drop(&mut self) {
        if self.lost.is_none() {
            if self.owner_window != 0 {
                self.conn
                    .set_selection_owner(ffi::value::NONE, self.last_server_time);
            }
            for (_, window) in self.windows.iter() {
                self.conn.destroy_x_window(window.id);
            }
            if self.blank_cursor != 0 {
                // SAFETY: the connection is live and the cursor was created by
                // `blank_cursor`, which caches it, so this frees it once.
                unsafe { (self.conn.lib.free_cursor)(self.conn.raw(), self.blank_cursor) };
            }
            self.conn.flush();
        }
        // SAFETY: the connection was returned live by `xcb_connect` and is
        // disconnected exactly once, here, after every resource created on it
        // has been released. `NonNull` guarantees it is not null.
        unsafe { (self.conn.lib.disconnect)(self.conn.raw()) };
    }
}
