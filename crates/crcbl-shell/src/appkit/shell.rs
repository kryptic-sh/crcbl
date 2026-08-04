//! [`AppKitShell`] — the shell itself, the event pump, and the [`Shell`]
//! implementation.

use core::ptr::{self, NonNull};
use core::time::Duration;
use std::collections::VecDeque;
use std::rc::Rc;

use crcbl_core::{Pool, SurfaceTarget};

use crate::{
    ClipboardOffer, ClipboardRequestId, CloseReply, CursorIcon, DisplayMode, LogicalSize, MimeType,
    MonitorId, MonitorInfo, PhysicalPoint, PointerMode, Shell, ShellBackend, ShellCaps, ShellError,
    ShellEvent, SizeConstraints, WindowConfiguration, WindowDesc, WindowId, WindowState,
};

use super::app::{self, Delegate, Shared};
use super::events::RawEvent;
use super::ffi::{self, Id, NSRect, Pool as AutoreleasePool, Sel, value};
use super::geometry;
use super::monitors::{self, Screen};
use super::window;

/// Why [`AppKitShell::wait`] came back.
///
/// `nextEventMatchingMask:untilDate:inMode:dequeue:` answers `nil` on a timeout
/// and an `NSEvent` otherwise, and those two are *the same duration* when the
/// event was already queued. Naming them is what makes
/// [`EVENT_WAIT`](ShellCaps::EVENT_WAIT) assertable as a mechanism rather than
/// as a stopwatch reading — the lesson the Win32 half of P5C paid a CI round
/// trip for, transferred here rather than relearned.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum Wake {
    /// The date passed with nothing to deliver: the wait slept, which is what
    /// [`EVENT_WAIT`](ShellCaps::EVENT_WAIT) promises is possible.
    TimedOut,
    /// An event was dequeued — either it arrived during the wait, or it was
    /// already there. Correct behaviour, and indistinguishable from a broken
    /// wait unless the queue is known to have been empty.
    Event,
}

/// The windowed style mask and frame a borderless window will be restored to.
///
/// Captured on the way into [`DisplayMode::Borderless`] and applied verbatim on
/// the way out; see [`window`](super::window) for why there is no
/// `WINDOWPLACEMENT` equivalent to save.
#[derive(Clone, Copy, Debug, PartialEq)]
pub(super) struct Saved {
    /// `styleMask` as it was.
    pub mask: usize,
    /// `frame` as it was, in AppKit points.
    pub frame: NSRect,
}

/// One window.
#[derive(Debug)]
pub(super) struct AppWindow {
    /// The `NSWindow`. Owned: released by `destroy_window` or `Drop`.
    pub window: Id,
    /// The content view, owned by the window.
    pub view: Id,
    /// The `CAMetalLayer`, which is what
    /// [`SurfaceTarget::AppKit`](crcbl_core::SurfaceTarget::AppKit) carries.
    /// Retained by this shell as well as by the view.
    pub layer: Id,
    /// The same pointer as the integer the raw queue matches on.
    pub key: usize,
    title: String,
    /// The **windowed** size that was asked for. Kept because a window created
    /// borderless has no saved frame to be restored to, and
    /// [`apply_mode`](AppKitShell::apply_mode) has to build one.
    pub requested_size: LogicalSize,
    requested_mode: DisplayMode,
    requested_constraints: SizeConstraints,
    pub resizable: bool,
    /// `backingScaleFactor`. 1.0 or 2.0 and nothing between; see
    /// [`geometry::usable_scale`].
    pub scale: f64,
    configuration: Option<WindowConfiguration>,
    /// The configuration AppKit has already told us about, waiting to be
    /// published on the next [`pump`](Shell::pump).
    ///
    /// The same shape as the X11 and Win32 backends', and for the same reason:
    /// the size is known synchronously, and the only thing making the caller
    /// wait is that [`WindowState::size`] is defined as `None` until a
    /// [`Resized`](ShellEvent::Resized) has been delivered.
    pending: Option<WindowConfiguration>,
    /// The last configuration published, so that the notification storm of a
    /// live resize does not produce a run of identical
    /// [`Resized`](ShellEvent::Resized) events.
    last_config: Option<WindowConfiguration>,
    /// The mode in effect. On AppKit this is whatever was last applied — there
    /// is no window manager to disagree — but the *screen* in it is read back
    /// rather than assumed.
    pub effective_mode: DisplayMode,
    pub saved: Option<Saved>,
    /// The cursor last asked for. **Recorded and not applied**; see
    /// [`set_cursor`](AppKitShell::set_cursor).
    cursor: Option<Option<CursorIcon>>,
    focused: bool,
    close_pending: bool,
}

/// A [`Shell`] backed by AppKit.
///
/// Constructed through [`open`](crate::open) or
/// [`open_backend`](crate::open_backend); see the [module docs](super) for what
/// this backend implements, what waits for M2 and M3, and where macOS differs
/// from every other implementation of this seam.
pub struct AppKitShell {
    /// The `NSApplication` singleton, which the runtime owns for the life of the
    /// process.
    app: Id,
    /// Whether this process was allowed to become a regular, focusable
    /// application. See [`app::Bootstrap`](super::app::Bootstrap).
    policy_accepted: bool,
    /// What the delegate callbacks write into.
    ///
    /// An `Rc` because [`Delegate`] holds a raw pointer into it that must stay
    /// valid for as long as the delegate is registered — which
    /// [`drop`](Self::drop) guarantees by dropping the delegate first.
    shared: Rc<Shared>,
    delegate: Delegate,
    windows: Pool<AppWindow>,
    /// The screens in AppKit's own space, for placement. See
    /// [`monitors`](super::monitors) for why both forms are kept.
    pub(super) screens: Vec<Screen>,
    monitors: Vec<MonitorInfo>,
    /// `CGDirectDisplayID` → the [`MonitorId`] it was given, so an id is never
    /// reused within a session — the obligation [`monitor`](crate::monitor)
    /// states.
    pub(super) monitor_ids: Vec<(u32, MonitorId)>,
    pub(super) next_monitor_id: u32,
    queue: VecDeque<ShellEvent>,
    /// The presentation options last written to `NSApplication`, so that the
    /// process-wide setting is touched only when it changes.
    presentation: usize,
    caps: ShellCaps,
}

impl core::fmt::Debug for AppKitShell {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        f.debug_struct("AppKitShell")
            .field("windows", &self.windows.len())
            .field("monitors", &self.monitors.len())
            .field("queued_events", &self.queue.len())
            .field("policy_accepted", &self.policy_accepted)
            .finish()
    }
}

impl AppKitShell {
    /// Brings `NSApplication` up, builds the delegate and enumerates the
    /// screens.
    ///
    /// There is no connection to make in the Linux sense — the window server is
    /// reached by loading AppKit — so the things that can genuinely fail are the
    /// thread, the singleton and the runtime-built classes.
    ///
    /// # Errors
    ///
    /// [`ShellError::Backend`] if this is not the process's main thread — see
    /// [`app`](super::app), which is where that rule is argued and where the
    /// consequence for the test suite is written down — or
    /// [`ShellError::Connect`] if `NSApplication` is not in this image or its
    /// singleton could not be created, which is what a process with no
    /// WindowServer session looks like.
    pub fn open() -> Result<Self, ShellError> {
        app::require_main_thread()?;
        let _pool = AutoreleasePool::push();
        let bootstrap = app::bootstrap()?;
        let shared = Rc::new(Shared::default());
        let delegate = Delegate::new(Rc::as_ptr(&shared))?;

        let mut shell = Self {
            app: bootstrap.app,
            policy_accepted: bootstrap.policy_accepted,
            shared,
            delegate,
            windows: Pool::new(),
            screens: Vec::new(),
            monitors: Vec::new(),
            monitor_ids: Vec::new(),
            next_monitor_id: 1,
            queue: VecDeque::new(),
            presentation: value::PRESENTATION_DEFAULT,
            caps: super::caps(),
        };
        // SAFETY: the main thread, with a pool in scope.
        let (screens, monitors) = unsafe { monitors::enumerate(&mut shell) };
        shell.screens = screens;
        shell.monitors = monitors;
        Ok(shell)
    }

    pub(super) fn window(&self, handle: WindowId) -> Result<&AppWindow, ShellError> {
        self.windows
            .get(handle.cast())
            .ok_or_else(|| ShellError::invalid_window(handle))
    }

    pub(super) fn window_mut(&mut self, handle: WindowId) -> Result<&mut AppWindow, ShellError> {
        self.windows
            .get_mut(handle.cast())
            .ok_or_else(|| ShellError::invalid_window(handle))
    }

    /// The [`WindowId`] owning an `NSWindow *`, for routing a raw event.
    fn window_by_key(&self, key: usize) -> Option<WindowId> {
        self.windows
            .iter()
            .find(|(_, state)| state.key == key)
            .map(|(handle, _)| handle.cast())
    }

    /// Which of the enumerated screens a window is on.
    ///
    /// `None` when the window is off every screen — which AppKit reports as a
    /// `nil` `screen` and is a real state for a window ordered out — or when the
    /// display it names is one this shell has not enumerated, which is a hotplug
    /// between the two.
    pub(super) fn screen_of(&self, window: Id) -> Option<MonitorId> {
        // SAFETY: a live window of this shell; `screen` is an accessor that may
        // answer nil.
        let screen = unsafe { ffi::msg(window, ffi::sel(c"screen")) };
        if screen.is_null() {
            return None;
        }
        // SAFETY: a live `NSScreen`, on the main thread, with a pool in scope —
        // every caller of this is inside one.
        let display = unsafe { monitors::display_id(screen) }?;
        self.monitor_ids
            .iter()
            .find(|(known, _)| *known == display)
            .map(|(_, id)| *id)
    }

    /// Puts the menu bar and the Dock where the current set of windows wants
    /// them.
    ///
    /// Presentation options are a **process** property — there is one menu bar —
    /// so they cannot be a per-window setting the way a style mask is. This is
    /// the Win32 backend's cursor-clip problem in a different costume, and it
    /// gets the same answer: the shell owns the desktop-wide state, derives it
    /// from all its windows, and hands it back on the way out.
    ///
    /// Written only when it changes, because `setPresentationOptions:` is a
    /// visible transition rather than an idempotent assignment.
    pub(super) fn refresh_presentation(&mut self) {
        let borderless = self
            .windows
            .iter()
            .any(|(_, state)| state.effective_mode.is_borderless());
        // Through `geometry` rather than inline, so the pairing rule — the
        // menu-bar bit is only legal beside a Dock bit, and an illegal
        // combination *raises* — is enforced in the one place that has a test.
        let wanted = geometry::presentation_options(if borderless {
            DisplayMode::Borderless { monitor: None }
        } else {
            DisplayMode::Windowed
        });
        if wanted == self.presentation {
            return;
        }
        self.presentation = wanted;
        // SAFETY: `self.app` is the live `NSApplication` singleton, and `wanted`
        // is one of the two combinations `geometry::presentation_options`
        // produces — never the menu-bar bit on its own, which would raise.
        unsafe { ffi::msg_set_usize(self.app, ffi::sel(c"setPresentationOptions:"), wanted) };
    }

    /// Recomputes a window's configuration from AppKit and queues it if it
    /// changed.
    ///
    /// **The whole of the resize path**, and it reads rather than being told:
    /// AppKit's geometry is state, so the notification only says "look again".
    /// See [`events`](super::events) for why that is the one place this backend
    /// is simpler than the Windows one.
    fn refresh_configuration(&mut self, handle: WindowId) {
        let Ok(state) = self.window(handle) else {
            return;
        };
        let (window, view, layer) = (state.window, state.view, state.layer);
        // SAFETY: a live window and its live view, on the main thread.
        let scale = unsafe { window::backing_scale(window) };
        // SAFETY: the layer and its host are alive for as long as the window is.
        unsafe { window::size_layer(layer, view, scale) };
        // SAFETY: a live view whose window device exists.
        let size = unsafe { window::backing_size(view) };
        let Ok(state) = self.window_mut(handle) else {
            return;
        };
        state.scale = scale;
        let config = WindowConfiguration {
            size,
            scale_factor: scale,
            mode: state.effective_mode,
        };
        // A live resize delivers a notification per frame; only a *change* is an
        // event. Comparing the whole configuration rather than the size alone
        // is what makes a scale change with an unchanged pixel extent — a
        // window dragged between two displays that happen to agree — still
        // reportable.
        if state.last_config == Some(config) {
            return;
        }
        state.last_config = Some(config);
        state.pending = Some(config);
    }

    /// Turns what the delegate recorded into shell state and events.
    ///
    /// Takes the whole queue first, so no borrow is held while this calls back
    /// into AppKit — several of these branches do, and a callback re-entering
    /// [`Shared`] while it was borrowed would panic.
    fn translate(&mut self) {
        for event in self.shared.take_events() {
            let handle = event.window().and_then(|key| self.window_by_key(key));
            match event {
                RawEvent::Resized { .. } | RawEvent::BackingChanged { .. } => {
                    let Some(handle) = handle else { continue };
                    self.refresh_configuration(handle);
                }
                RawEvent::Focus { focused, .. } => {
                    let Some(handle) = handle else { continue };
                    if let Ok(state) = self.window_mut(handle) {
                        state.focused = focused;
                    }
                    self.queue.push_back(ShellEvent::Focus {
                        window: handle,
                        focused,
                    });
                }
                RawEvent::CloseRequested { .. } => {
                    let Some(handle) = handle else { continue };
                    if let Ok(state) = self.window_mut(handle) {
                        state.close_pending = true;
                    }
                    self.queue
                        .push_back(ShellEvent::CloseRequested { window: handle });
                }
                RawEvent::Closed { .. } => {
                    // Only reachable when the window went away without this
                    // shell asking — `destroy_window` clears the delegate
                    // *before* closing, so its own `windowWillClose:` is never
                    // recorded and no second `WindowDestroyed` is reported.
                    let Some(handle) = handle else { continue };
                    if let Some(removed) = self.windows.remove(handle.cast()) {
                        self.shared.forget(removed.key);
                        // SAFETY: the window is closing itself, so only this
                        // shell's own references are released here.
                        unsafe { release_window(removed.window, removed.layer, false) };
                        self.queue
                            .push_back(ShellEvent::WindowDestroyed { window: handle });
                        self.refresh_presentation();
                    }
                }
                RawEvent::ScreensChanged => {
                    let (screens, monitors) = {
                        // SAFETY: the main thread, inside the pool `pump` holds.
                        unsafe { monitors::enumerate(self) }
                    };
                    self.screens = screens;
                    self.monitors = monitors;
                    self.queue.push_back(ShellEvent::MonitorsChanged);
                }
            }
        }
    }

    /// Publishes configurations AppKit has already told us about.
    ///
    /// A copy of the X11 and Win32 backends', for the reason they both give: a
    /// scale change is emitted **before** the resize it came with, because a
    /// consumer that recreates a swapchain on `Resized` must already know the
    /// scale it is recreating at.
    fn publish_configurations(&mut self) {
        let ready: Vec<(WindowId, WindowConfiguration)> = self
            .windows
            .iter_mut()
            .filter_map(|(handle, state)| {
                state.pending.take().map(|config| (handle.cast(), config))
            })
            .collect();
        for (handle, config) in ready {
            let previous = self
                .windows
                .get(handle.cast())
                .and_then(|state| state.configuration);
            if let Some(state) = self.windows.get_mut(handle.cast()) {
                state.configuration = Some(config);
            }
            if previous.is_some_and(|previous| {
                (previous.scale_factor - config.scale_factor).abs() > f64::EPSILON
            }) {
                self.queue.push_back(ShellEvent::ScaleFactorChanged {
                    window: handle,
                    scale_factor: config.scale_factor,
                    size: config.size,
                });
            }
            self.queue.push_back(ShellEvent::Resized {
                window: handle,
                size: config.size,
                scale_factor: config.scale_factor,
            });
        }
    }

    /// Dequeues and dispatches everything `NSApplication` has, without blocking.
    ///
    /// `dequeue: YES` with `untilDate:[NSDate distantPast]` is AppKit's
    /// "whatever is there, right now" — the same shape as `PeekMessageW` with
    /// `PM_REMOVE`. `sendEvent:` is what routes an event to its window and is
    /// therefore what causes the delegate callbacks to fire, synchronously,
    /// inside this loop.
    ///
    /// # Safety
    ///
    /// The main thread, with an autorelease pool in scope.
    unsafe fn drain_events(&mut self) {
        // SAFETY: `distantPast` is a class method returning an autoreleased
        // date, valid for the pool the caller holds.
        let Some(date_class) = ffi::class(c"NSDate") else {
            return;
        };
        // SAFETY: a live class.
        let past = unsafe { ffi::msg(date_class, ffi::sel(c"distantPast")) };
        loop {
            // SAFETY: `self.app` is the live singleton; the mask is
            // `NSEventMaskAny`, the date and the mode are live objects, and
            // `dequeue: YES` takes the event out of the queue.
            let event = unsafe { next_event(self.app, past) };
            if event.is_null() {
                break;
            }
            // SAFETY: an event this call just dequeued. This dispatches into the
            // delegate re-entrantly, which is why nothing here holds a borrow of
            // `shared`.
            unsafe { ffi::msg1_void(self.app, ffi::sel(c"sendEvent:"), event) };
        }
        // What `-[NSApplication run]` does at the bottom of its loop, and the
        // only part of it this backend has to reproduce: it is what gives
        // windows a chance to update themselves after a batch of events.
        // SAFETY: the live singleton.
        unsafe { ffi::msg_void(self.app, ffi::sel(c"updateWindows")) };
    }

    /// [`wait_events`](Shell::wait_events), with the reason it came back.
    ///
    /// # Decision: drain first, then sleep
    ///
    /// The hazard is a wait that sleeps out its whole timeout with work already
    /// queued — an editor that called [`pump`](Shell::pump), was handed
    /// something that queued another event, and then went to sleep. Draining
    /// first closes it, and it is also what makes a [`Wake::TimedOut`] mean
    /// something: with the queue known empty, a timeout is evidence that this
    /// call genuinely blocks, which is the whole of
    /// [`EVENT_WAIT`](ShellCaps::EVENT_WAIT).
    ///
    /// Whatever the delegate recorded here is delivered by the next `pump`,
    /// exactly as if the events had been drained there. That is the same
    /// arrangement the Win32 backend arrived at, and it is written here rather
    /// than inherited because the two reached it from different directions:
    /// there it replaced a flag that could not be cleared, here it is the only
    /// way to know what a `nil` return means.
    fn wait(&mut self, timeout: Option<Duration>) -> Wake {
        let _pool = AutoreleasePool::push();
        // SAFETY: the main thread — a `Shell` is not `Send` — with the pool
        // above in scope.
        unsafe { self.drain_events() };
        let Some(date_class) = ffi::class(c"NSDate") else {
            return Wake::TimedOut;
        };
        // SAFETY: both are `NSDate` class methods returning autoreleased dates.
        let until = unsafe {
            match timeout {
                None => ffi::msg(date_class, ffi::sel(c"distantFuture")),
                Some(timeout) => {
                    let after: unsafe extern "C" fn(Id, Sel, f64) -> Id = ffi::msg_send();
                    after(
                        date_class,
                        ffi::sel(c"dateWithTimeIntervalSinceNow:"),
                        timeout.as_secs_f64(),
                    )
                }
            }
        };
        // SAFETY: the live singleton and a live date; this is the call that
        // blocks, and it is the whole of `EVENT_WAIT` on this platform.
        let event = unsafe { next_event(self.app, until) };
        if event.is_null() {
            return Wake::TimedOut;
        }
        // SAFETY: an event just dequeued; dropping it would lose it, because
        // `dequeue: YES` already took it off the queue.
        unsafe { ffi::msg1_void(self.app, ffi::sel(c"sendEvent:"), event) };
        Wake::Event
    }
}

/// `-[NSApplication nextEventMatchingMask:untilDate:inMode:dequeue:]`.
///
/// One helper because the signature is four arguments long and both callers need
/// exactly it — and because getting `dequeue:` wrong is an infinite loop rather
/// than an error.
///
/// # Safety
///
/// `app` must be the live `NSApplication` singleton and `until` a live `NSDate`,
/// on the main thread, with an autorelease pool in scope.
unsafe fn next_event(app: Id, until: Id) -> Id {
    // SAFETY: the signature written here is the method's.
    let send: unsafe extern "C" fn(Id, Sel, usize, Id, Id, ffi::ObjcBool) -> Id =
        unsafe { ffi::msg_send() };
    unsafe {
        send(
            app,
            ffi::sel(c"nextEventMatchingMask:untilDate:inMode:dequeue:"),
            value::EVENT_MASK_ANY,
            until,
            ffi::NSDefaultRunLoopMode,
            ffi::YES,
        )
    }
}

/// Gives back everything one window owns.
///
/// Takes the two pointers rather than the [`AppWindow`] they came out of, so
/// that [`AppKitShell::drop`] — which cannot move a window out of the pool while
/// it is iterating it — does not have to reconstruct one.
///
/// `close` is skipped for a window that is closing itself, because it is already
/// inside `windowWillClose:` and asking again is a re-entrant close.
///
/// # Safety
///
/// The main thread. Both objects must be live and must not be used afterwards.
unsafe fn release_window(window: Id, layer: Id, close: bool) {
    // SAFETY: live objects of this shell. The delegate is cleared *first*: a
    // window's `delegate` reference is unretained, so a window that outlived its
    // delegate — which cannot happen here, and which this order is what
    // guarantees — would message freed memory. Clearing it also means the close
    // below records nothing.
    unsafe {
        ffi::msg1_void(window, ffi::sel(c"setDelegate:"), ptr::null_mut());
        if close {
            ffi::msg_void(window, ffi::sel(c"close"));
        }
        // The retain `create_native_window` took, because this shell hands the
        // pointer out in `SurfaceTarget::AppKit`. The view holds its own.
        ffi::msg_void(layer, ffi::sel(c"release"));
        // The one retain `alloc` gave the window. `releasedWhenClosed` was
        // turned off at creation precisely so that this is the only release.
        ffi::msg_void(window, ffi::sel(c"release"));
    }
}

impl Shell for AppKitShell {
    fn backend(&self) -> ShellBackend {
        ShellBackend::AppKit
    }

    /// What this backend can do, fixed for the shell's lifetime.
    ///
    /// Each bit is a claim about code that exists in this slice:
    ///
    /// * [`MULTI_WINDOW`](ShellCaps::MULTI_WINDOW) — nothing about an
    ///   `NSWindow`, a delegate or the event queue is single-window.
    /// * [`EVENT_WAIT`](ShellCaps::EVENT_WAIT) —
    ///   [`wait_events`](Shell::wait_events) is
    ///   `nextEventMatchingMask:untilDate:` with a real date, which genuinely
    ///   blocks the thread in the run loop.
    /// * [`WINDOW_POSITION`](ShellCaps::WINDOW_POSITION) — one global
    ///   coordinate space, `setFrame:` moves a window in it, and a borderless
    ///   window is placed on a **named** screen by writing that screen's own
    ///   frame. See [`monitors`](super::monitors) for why the space being
    ///   *points* rather than pixels makes this bit more honest here than the
    ///   pixel rectangles in [`MonitorInfo::bounds`] would suggest.
    /// * [`SERVER_DECORATIONS`](ShellCaps::SERVER_DECORATIONS) — a windowed
    ///   window carries `NSWindowStyleMaskTitled`, so AppKit draws the title bar
    ///   and the UI layer must not.
    /// * [`ASPECT_HINT_HONORED`](ShellCaps::ASPECT_HINT_HONORED) —
    ///   `setContentAspectRatio:`, which `docs/plan/15-windowing.md` names as
    ///   this platform's form of it, and which AppKit enforces during the drag
    ///   itself.
    ///
    /// # What is clear, and why each one is a decision
    ///
    /// * [`FRACTIONAL_SCALE`](ShellCaps::FRACTIONAL_SCALE) — **macOS does not
    ///   have one.** `backingScaleFactor` is 1.0 or 2.0; a display running a
    ///   "scaled" HiDPI mode still answers 2.0 and changes the *point*
    ///   resolution instead, so the fractional part lands in the geometry rather
    ///   than in the scale. This is the one desktop backend where the bit is
    ///   clear, and it is a fact about the platform rather than a gap here.
    /// * [`HW_UPSCALE`](ShellCaps::HW_UPSCALE) — the platform *can*: a
    ///   `CAMetalLayer`'s `drawableSize` is independent of its bounds and Core
    ///   Animation scales the difference in hardware, which is exactly what
    ///   Wayland's `wp_viewport` buys. **The seam has no way to ask for it
    ///   yet** — nothing in [`Shell`] says "present a smaller buffer" — so
    ///   setting the bit would be a claim with no mechanism behind it and no
    ///   test. `docs/backlog.md` carries it.
    /// * [`POINTER_LOCK`](ShellCaps::POINTER_LOCK),
    ///   [`POINTER_CONFINE`](ShellCaps::POINTER_CONFINE),
    ///   [`POINTER_WARP`](ShellCaps::POINTER_WARP),
    ///   [`RAW_POINTER_MOTION`](ShellCaps::RAW_POINTER_MOTION),
    ///   [`TEXT_IME`](ShellCaps::TEXT_IME) — M2. There is no `NSTrackingArea`,
    ///   no `NSEvent` translation and no `NSTextInputClient` in this slice, and
    ///   [`set_pointer_mode`](Shell::set_pointer_mode) and
    ///   [`warp_pointer`](Shell::warp_pointer) say so by refusing.
    /// * [`CLIPBOARD`](ShellCaps::CLIPBOARD),
    ///   [`DRAG_DROP`](ShellCaps::DRAG_DROP) — M3. No `NSPasteboard`, no
    ///   `registerForDraggedTypes:`.
    fn caps(&self) -> ShellCaps {
        self.caps
    }

    /// Creates a window. **The window has no size yet.**
    ///
    /// # AppKit knows the size, and this still returns without one
    ///
    /// `initWithContentRect:` takes a rectangle and the content view's bounds
    /// answer immediately, so — as on X11 and Win32, and unlike Wayland — the
    /// size is known before this function returns. It is not *reported* before
    /// this function returns, because [`WindowState::size`] is defined as `None`
    /// until a [`Resized`](ShellEvent::Resized) has been delivered and
    /// delivering events is [`pump`](Shell::pump)'s job. The create-then-wait
    /// loop is real here as well; it just completes on the **first pump**.
    ///
    /// # `app_id` is validated and not applied
    ///
    /// macOS has no `WM_CLASS` and no per-window application identity: the
    /// equivalent is `CFBundleIdentifier` in an `Info.plist`, which is a
    /// property of the *bundle* and cannot be set by a running process at all.
    /// So [`WindowDesc::app_id`](crate::WindowDesc::app_id) is checked for a NUL
    /// byte — so that a descriptor rejected on the other backends is rejected
    /// here too — and is otherwise unused. Unlike the Win32 equivalent this is
    /// not a deferral; there is nothing to defer to.
    ///
    /// # Errors
    ///
    /// [`ShellError::NoSuchMonitor`] for a borderless request naming a display
    /// that is gone, [`ShellError::InvalidDescriptor`] for a title or app id
    /// containing a NUL byte, [`ShellError::Connect`] if a class is missing, and
    /// [`ShellError::WindowCreation`] if AppKit refused.
    fn create_window(&mut self, desc: &WindowDesc<'_>) -> Result<WindowId, ShellError> {
        let _pool = AutoreleasePool::push();
        if let DisplayMode::Borderless {
            monitor: Some(monitor),
        } = desc.mode
            && self.monitor(monitor).is_none()
        {
            return Err(ShellError::NoSuchMonitor(monitor.0));
        }
        if desc.app_id.as_bytes().contains(&0) {
            return Err(ShellError::InvalidDescriptor(
                "app_id contains a NUL byte".to_string(),
            ));
        }
        let target = match desc.mode {
            DisplayMode::Borderless {
                monitor: Some(monitor),
            } => self.screens.iter().find(|screen| screen.id == monitor),
            // Index zero is the menu-bar screen — macOS's primary.
            _ => self.screens.first(),
        }
        .copied();

        let created = self.create_native_window(desc, target.as_ref())?;
        let effective_mode = match desc.mode {
            DisplayMode::Windowed => DisplayMode::Windowed,
            DisplayMode::Borderless { .. } => DisplayMode::Borderless {
                monitor: self.screen_of(created.window),
            },
        };
        let config = WindowConfiguration {
            size: created.size,
            scale_factor: created.scale,
            mode: effective_mode,
        };
        let state = AppWindow {
            window: created.window,
            view: created.view,
            layer: created.layer,
            key: created.window as usize,
            title: desc.title.to_string(),
            requested_size: desc.size,
            requested_mode: desc.mode,
            requested_constraints: desc.constraints,
            resizable: desc.resizable,
            scale: created.scale,
            configuration: None,
            // Known now, delivered on the first pump; see the doc comment.
            pending: Some(config),
            last_config: Some(config),
            effective_mode,
            saved: None,
            cursor: None,
            focused: false,
            close_pending: false,
        };
        let handle: WindowId = self.windows.insert(state).cast();

        // **After the pool entry exists**, so that a delegate callback fired by
        // `setDelegate:` — AppKit sends nothing here today, but the ordering is
        // the invariant rather than the observation — finds a window to route to.
        // SAFETY: a live window and the live delegate this shell owns.
        unsafe {
            ffi::msg1_void(
                created.window,
                ffi::sel(c"setDelegate:"),
                self.delegate.object(),
            );
        }
        self.refresh_presentation();
        Ok(handle)
    }

    fn destroy_window(&mut self, handle: WindowId) -> Result<(), ShellError> {
        let _pool = AutoreleasePool::push();
        let removed = self
            .windows
            .remove(handle.cast())
            .ok_or_else(|| ShellError::invalid_window(handle))?;
        self.shared.forget(removed.key);
        // The pool entry is gone **before** the AppKit call, deliberately:
        // `close` posts `windowWillClose:` synchronously, and the
        // `RawEvent::Closed` that would produce must not find a live window and
        // report a second `WindowDestroyed` for it. Clearing the delegate inside
        // `release_window` is the belt to that braces.
        //
        // SAFETY: the objects are this shell's own and are not used afterwards.
        unsafe { release_window(removed.window, removed.layer, true) };
        self.queue
            .push_back(ShellEvent::WindowDestroyed { window: handle });
        self.refresh_presentation();
        Ok(())
    }

    fn window_state(&self, handle: WindowId) -> Result<WindowState, ShellError> {
        let state = self.window(handle)?;
        Ok(WindowState {
            configuration: state.configuration,
            requested_mode: state.requested_mode,
            requested_constraints: state.requested_constraints,
            focused: state.focused,
            // Asked rather than remembered: `orderOut:` and a miniaturised
            // window are both "not visible", and only AppKit knows.
            //
            // SAFETY: a live window; `isVisible` is an accessor.
            visible: unsafe { ffi::msg_bool(state.window, ffi::sel(c"isVisible")) },
            // M2 owns the pointer; until then the only honest answer is the
            // mode nothing has changed.
            pointer_mode: PointerMode::Free,
            close_pending: state.close_pending,
        })
    }

    /// Sets the title bar text.
    ///
    /// # Errors
    ///
    /// [`ShellError::InvalidWindow`] if the handle is stale, or
    /// [`ShellError::InvalidDescriptor`] if the title contains a NUL byte —
    /// which would silently truncate it, since `stringWithUTF8String:` takes a
    /// NUL-terminated buffer.
    fn set_title(&mut self, handle: WindowId, title: &str) -> Result<(), ShellError> {
        let _pool = AutoreleasePool::push();
        let window = self.window(handle)?.window;
        // SAFETY: an autoreleased string, valid for the pool above.
        let Some(string) = (unsafe { ffi::nsstring(title) }) else {
            return Err(ShellError::InvalidDescriptor(
                "title contains a NUL byte".to_string(),
            ));
        };
        // SAFETY: a live window and a live string, which the window copies.
        unsafe { ffi::msg1_void(window, ffi::sel(c"setTitle:"), string) };
        self.window_mut(handle)?.title = title.to_string();
        Ok(())
    }

    /// Shows or hides the window — **really**, as on X11 and Win32 and unlike
    /// Wayland.
    ///
    /// `makeKeyAndOrderFront:` and `orderOut:` are a reversible pair with no
    /// state lost, so this is one of the places the desktop backends can do
    /// something the Wayland one documents that it cannot: there, a surface is
    /// mapped exactly while it has a buffer, and buffers belong to the renderer.
    ///
    /// [`WindowState::visible`] is read back from AppKit rather than set from
    /// the request, so a window that was miniaturised reports what happened.
    ///
    /// # Errors
    ///
    /// [`ShellError::InvalidWindow`] if the handle is stale.
    fn set_visible(&mut self, handle: WindowId, visible: bool) -> Result<(), ShellError> {
        let _pool = AutoreleasePool::push();
        let window = self.window(handle)?.window;
        // SAFETY: a live window; `nil` is the documented sender for both.
        unsafe {
            let selector = if visible {
                ffi::sel(c"makeKeyAndOrderFront:")
            } else {
                ffi::sel(c"orderOut:")
            };
            ffi::msg1_void(window, selector, ptr::null_mut());
        }
        Ok(())
    }

    /// Asks for windowed or borderless.
    ///
    /// A request in the seam's sense, and on this platform one that is always
    /// granted: there is no window manager to refuse it, because borderless is
    /// this process changing its own window's style mask and frame. What can
    /// still disagree with the request is the **display** —
    /// `Borderless { monitor: None }` means "wherever the window already is",
    /// and the answer names it — which is why
    /// [`mode_request_honoured`](WindowState::mode_request_honoured) compares
    /// with [`DisplayMode::satisfied_by`] rather than with `==`.
    ///
    /// # Errors
    ///
    /// [`ShellError::InvalidWindow`] for a stale handle,
    /// [`ShellError::NoSuchMonitor`] if the named display is gone, or
    /// [`ShellError::Backend`] if there is no display at all to be borderless
    /// on.
    fn set_mode(&mut self, handle: WindowId, mode: DisplayMode) -> Result<(), ShellError> {
        let _pool = AutoreleasePool::push();
        if let DisplayMode::Borderless {
            monitor: Some(monitor),
        } = mode
            && self.monitor(monitor).is_none()
        {
            return Err(ShellError::NoSuchMonitor(monitor.0));
        }
        self.window_mut(handle)?.requested_mode = mode;
        self.apply_mode(handle, mode)?;

        // The `windowDidResize:` the frame change produced is already in the raw
        // queue and `translate` would find nothing new in it, because
        // `refresh_configuration` has to be told the *mode* changed too. So the
        // configuration is published from here instead, which also covers the
        // case where the two modes happen to have the same content size.
        let state = self.window(handle)?;
        // SAFETY: a live window and its live view.
        let config = unsafe { window::configuration_of(state) };
        let state = self.window_mut(handle)?;
        state.pending = Some(config);
        state.last_config = Some(config);
        Ok(())
    }

    /// Asks for resize limits and an aspect lock.
    ///
    /// Applied straight through to `setContentMinSize:`, `setContentMaxSize:`
    /// and `setContentAspectRatio:`, in the seam's own units — see
    /// [`geometry::content_limits`] for why there is no conversion to do here
    /// and there is on both other desktop backends.
    ///
    /// Nothing is resized. A constraint bounds what the *user* may drag to; a
    /// window already outside it stays there until it is next resized, which is
    /// what [`SizeConstraints`] documents as "hints, whose only observable
    /// effect is whatever size the window system reports next".
    ///
    /// # Errors
    ///
    /// [`ShellError::InvalidWindow`] if the handle is stale.
    fn set_constraints(
        &mut self,
        handle: WindowId,
        constraints: SizeConstraints,
    ) -> Result<(), ShellError> {
        let _pool = AutoreleasePool::push();
        let state = self.window_mut(handle)?;
        state.requested_constraints = constraints;
        let window = state.window;
        // SAFETY: a live window of this shell.
        unsafe { window::apply_constraints(window, constraints) };
        Ok(())
    }

    fn monitors(&self) -> &[MonitorInfo] {
        &self.monitors
    }

    /// Runs the event queue and delivers what it produced.
    ///
    /// The order is the same as the other three native backends': the window
    /// system first, so that everything the delegate recorded during this drain
    /// is visible; then the translation into shell state; then the
    /// configurations, so a resize and its scale change land together; then
    /// delivery.
    ///
    /// **This does not return during a live resize drag**, and the reason is
    /// AppKit's rather than this loop's: `-[NSWindow setFrame:]` during a
    /// user-driven resize runs inside an event-tracking run-loop mode that
    /// `sendEvent:` does not return from until the mouse comes up. It is the
    /// same shape as the Win32 modal resize loop, and it costs the same thing —
    /// see the [module docs](super).
    fn pump(&mut self, sink: &mut dyn FnMut(ShellEvent)) {
        let _pool = AutoreleasePool::push();
        // SAFETY: the main thread — a `Shell` is not `Send`, and `open` refused
        // anywhere else — with the pool above in scope.
        unsafe { self.drain_events() };
        self.translate();
        self.publish_configurations();
        // Drain by count, not `while let`: a sink that creates a window must not
        // be able to spin this loop, and whatever it queued belongs to the next
        // frame — which is what the event queue would have done anyway.
        for _ in 0..self.queue.len() {
            let Some(event) = self.queue.pop_front() else {
                break;
            };
            sink(event);
        }
    }

    /// Blocks until an event arrives or `timeout` elapses.
    ///
    /// See [`wait`](Self::wait) for why the queue is drained first and why the
    /// reason it came back is a value rather than a duration.
    fn wait_events(&mut self, timeout: Option<Duration>) {
        match self.wait(timeout) {
            Wake::TimedOut | Wake::Event => {}
        }
    }

    /// The `CAMetalLayer` `crcbl-vk` needs for `VK_EXT_metal_surface`, and
    /// `crcbl-wgpu` for a Metal surface.
    ///
    /// Available the instant the window exists — AppKit creates a real, sized
    /// window synchronously, and this backend passes `defer: NO` so that the
    /// window device exists too — so a HAL surface can be created immediately
    /// and only the swapchain waits for the first
    /// [`Resized`](ShellEvent::Resized).
    ///
    /// **The layer, not the view**, which is the whole point:
    /// `crcbl_core::surface` states that the shell creates and owns it precisely
    /// so that no HAL backend ever has to touch AppKit. This shell keeps a
    /// retain on it for as long as the window lives.
    ///
    /// Lifetime: the layer lives as long as the [`WindowId`], so a HAL surface
    /// built from it must be destroyed before
    /// [`destroy_window`](Shell::destroy_window) and before the shell is
    /// dropped.
    ///
    /// # Errors
    ///
    /// [`ShellError::InvalidWindow`] if the handle is stale.
    fn surface_target(&self, handle: WindowId) -> Result<SurfaceTarget, ShellError> {
        let state = self.window(handle)?;
        let layer = NonNull::new(state.layer)
            .ok_or_else(|| ShellError::Backend("this window has no CAMetalLayer".to_string()))?;
        Ok(SurfaceTarget::AppKit { layer })
    }

    /// **Not implemented in this slice.**
    ///
    /// The pointer is M2: there is no `NSTrackingArea`, no
    /// `CGAssociateMouseAndMouseCursorPosition` and no `NSCursor` handling here
    /// yet, so every mode except [`PointerMode::Free`] is refused and
    /// [`caps`](Shell::caps) has none of the pointer bits set. A caller that
    /// checked [`PointerMode::required_cap`] never reaches this.
    ///
    /// # Errors
    ///
    /// [`ShellError::InvalidWindow`] for a stale handle, or
    /// [`ShellError::Unsupported`] naming the mode.
    fn set_pointer_mode(&mut self, handle: WindowId, mode: PointerMode) -> Result<(), ShellError> {
        self.window(handle)?;
        if mode == PointerMode::Free {
            return Ok(());
        }
        Err(ShellError::Unsupported {
            backend: ShellBackend::AppKit,
            what: mode.as_str(),
        })
    }

    /// Records the requested cursor. **It is not applied.**
    ///
    /// The same gap the X11 and Wayland backends document, and stated the same
    /// way rather than hidden: applying a shape means `NSCursor` and a
    /// `cursorUpdate:`/`resetCursorRects` path, which is pointer work and
    /// belongs with the rest of it in M2. No capability bit claims cursor
    /// shapes, so nothing here is overstated — but a caller that sets a
    /// crosshair will see an arrow.
    ///
    /// The trait's contract is that this fails only for a stale handle, so
    /// recording is what it does; refusing would be a different method.
    ///
    /// # Errors
    ///
    /// [`ShellError::InvalidWindow`] if the handle is stale.
    fn set_cursor(
        &mut self,
        handle: WindowId,
        cursor: Option<CursorIcon>,
    ) -> Result<(), ShellError> {
        self.window_mut(handle)?.cursor = Some(cursor);
        Ok(())
    }

    /// **Not implemented in this slice.**
    ///
    /// [`POINTER_WARP`](ShellCaps::POINTER_WARP) is clear, so this always
    /// refuses — which is exactly what the trait documents for a backend without
    /// the capability, and what the Wayland backend does permanently.
    ///
    /// # Errors
    ///
    /// [`ShellError::InvalidWindow`] for a stale handle, otherwise
    /// [`ShellError::Unsupported`].
    fn warp_pointer(
        &mut self,
        handle: WindowId,
        position: PhysicalPoint,
    ) -> Result<(), ShellError> {
        self.window(handle)?;
        let _ = position;
        Err(ShellError::Unsupported {
            backend: ShellBackend::AppKit,
            what: "pointer warp",
        })
    }

    fn reply_close_request(
        &mut self,
        handle: WindowId,
        reply: CloseReply,
    ) -> Result<(), ShellError> {
        let state = self.window_mut(handle)?;
        if !state.close_pending {
            return Err(ShellError::NoPendingCloseRequest { window: handle });
        }
        state.close_pending = false;
        match reply {
            CloseReply::Keep => Ok(()),
            CloseReply::Close => self.destroy_window(handle),
        }
    }

    /// **Not implemented in this slice.**
    ///
    /// [`CLIPBOARD`](ShellCaps::CLIPBOARD) is clear: `NSPasteboard` is M3.
    ///
    /// # Errors
    ///
    /// [`ShellError::InvalidWindow`] for a stale handle, otherwise
    /// [`ShellError::Unsupported`].
    fn clipboard_offer(
        &mut self,
        handle: WindowId,
        offers: &[ClipboardOffer<'_>],
    ) -> Result<(), ShellError> {
        self.window(handle)?;
        let _ = offers;
        Err(ShellError::Unsupported {
            backend: ShellBackend::AppKit,
            what: "the clipboard",
        })
    }

    /// **Not implemented in this slice.** See
    /// [`clipboard_offer`](Self::clipboard_offer).
    ///
    /// # Errors
    ///
    /// [`ShellError::InvalidWindow`] for a stale handle, otherwise
    /// [`ShellError::Unsupported`].
    fn clipboard_request(
        &mut self,
        handle: WindowId,
        mime: MimeType,
    ) -> Result<ClipboardRequestId, ShellError> {
        self.window(handle)?;
        let _ = mime;
        Err(ShellError::Unsupported {
            backend: ShellBackend::AppKit,
            what: "the clipboard",
        })
    }
}

impl Drop for AppKitShell {
    /// Closes every window this shell still owns, hands the menu bar back, and
    /// then lets the delegate go.
    ///
    /// The order is the soundness argument for the raw pointer the delegate
    /// holds. `close` posts `windowWillClose:` **synchronously**, and the
    /// callback reads [`Shared`] through that pointer — so every window has to
    /// be gone while the `Rc` is still alive, which is what doing it here, before
    /// the fields are dropped, guarantees. [`Delegate::drop`] then removes the
    /// registry entry before the `Rc` itself is dropped, and Rust's field drop
    /// order puts `delegate` before `shared` because that is the order they are
    /// declared in.
    ///
    /// The presentation options are process-wide and are handed back for the
    /// reason the Win32 backend gives about the cursor clip: a shell dropped by
    /// a host application that keeps running would otherwise leave that
    /// application with no menu bar and nothing to point at.
    fn drop(&mut self) {
        let _pool = AutoreleasePool::push();
        if self.presentation != value::PRESENTATION_DEFAULT {
            // SAFETY: the live singleton; `PRESENTATION_DEFAULT` is always a
            // legal combination.
            unsafe {
                ffi::msg_set_usize(
                    self.app,
                    ffi::sel(c"setPresentationOptions:"),
                    value::PRESENTATION_DEFAULT,
                );
            }
        }
        let windows: Vec<(Id, Id)> = self
            .windows
            .iter()
            .map(|(_, state)| (state.window, state.layer))
            .collect();
        for (window, layer) in windows {
            // SAFETY: this shell's own windows, on the thread that created them
            // — a `Shell` is not `Send`, so this is that thread.
            unsafe { release_window(window, layer, true) };
        }
    }
}

/// Tests that need a real Mac, and are therefore the point of the
/// `build + test (macos-latest)` CI job.
///
/// **Nothing here is `#[ignore]`d**, on the same terms as the Win32 suite:
/// `docs/plan/12-testing.md` calls a silently-skipped suite a known trap, and a
/// failure here is the runner's answer rather than a gap in the schedule.
///
/// # What this suite deliberately does **not** do, and why
///
/// It never creates a window. That is not a scoping choice — it is forced, and
/// the measurement is worth recording where the next person will look for it:
///
/// * AppKit is main-thread-only, and
///   `nextEventMatchingMask:untilDate:inMode:dequeue:` **raises** an
///   Objective-C exception when it is called anywhere else. An exception
///   unwinding through a Rust frame is undefined behaviour, so
///   [`AppKitShell::open`] refuses off the main thread rather than finding out.
/// * Rust's `libtest` runs a test body on a thread it **spawns**, always. That
///   was measured on this workspace's toolchain rather than assumed, both under
///   `cargo nextest` and by invoking the test binary directly with
///   `--test-threads=1`; the body is off the main thread in every case.
///
/// So a `#[test]` cannot drive a window on this platform, and the window-session
/// pass — open, pump to the first `Resized`, flip to borderless and back,
/// destroy — needs a target whose `main` this crate owns. `docs/backlog.md`
/// carries what that is and what it would take.
///
/// What is here instead is everything that genuinely can be checked from a
/// spawned thread, and it is not nothing: the **Objective-C runtime FFI itself**
/// — every `objc_msgSend` signature shape this backend transmutes, against
/// Foundation classes that are thread-safe, plus a class built and dispatched at
/// runtime the same way [`app`](super::app)'s delegate is. That is the single
/// highest-risk item in this backend, it is the one thing a Linux `cargo check`
/// cannot say anything about, and it is fully reachable here.
#[cfg(all(test, target_os = "macos"))]
mod tests {
    use super::super::ffi::{Imp, NSPoint, NSSize, ObjcBool};
    use super::*;
    use core::ffi::CStr;

    /// A rectangle no ABI would produce by accident, so a struct return that
    /// went through the wrong trampoline is visibly wrong rather than
    /// plausibly zero.
    const PROBE_RECT: NSRect = NSRect::new(-12.5, 37.25, 1280.75, 720.125);

    /// `(id, SEL) -> NSRect` — the shape that needs `objc_msgSend_stret` on
    /// x86_64 and must not use it on aarch64.
    ///
    /// # Safety
    ///
    /// Called only by the runtime, on an instance of the probe class.
    unsafe extern "C" fn probe_rect(_this: Id, _cmd: Sel) -> NSRect {
        PROBE_RECT
    }

    /// `(id, SEL, NSRect) -> f64` — a large struct **argument**, which is the
    /// other half of the same ABI question.
    ///
    /// # Safety
    ///
    /// Called only by the runtime, on an instance of the probe class.
    unsafe extern "C" fn probe_area(_this: Id, _cmd: Sel, rect: NSRect) -> f64 {
        rect.size.width * rect.size.height
    }

    /// `(id, SEL) -> BOOL`.
    ///
    /// # Safety
    ///
    /// Called only by the runtime, on an instance of the probe class.
    unsafe extern "C" fn probe_flag(_this: Id, _cmd: Sel) -> ObjcBool {
        ffi::YES
    }

    /// Builds the probe class once, exactly as `app::delegate_class` builds the
    /// delegate: allocate, add methods with their encodings, register.
    fn probe_class() -> Id {
        use std::sync::OnceLock;
        static PROBE: OnceLock<usize> = OnceLock::new();
        *PROBE.get_or_init(|| {
            let superclass = ffi::class(c"NSObject").expect("Foundation is linked");
            // SAFETY: a live superclass and a NUL-terminated literal name.
            let class =
                unsafe { ffi::objc_allocateClassPair(superclass, c"CrcblFfiProbe".as_ptr(), 0) };
            assert!(!class.is_null(), "objc_allocateClassPair refused the probe");
            let bool_encoding = std::ffi::CString::new(format!("{}@:", ffi::ENC_BOOL)).unwrap();
            let methods: [(&CStr, Imp, &CStr); 3] = [
                (
                    c"crcblProbeRect",
                    // SAFETY: a correctly-typed `extern "C"` function cast to
                    // the runtime's opaque IMP, with the matching encoding.
                    unsafe {
                        core::mem::transmute::<unsafe extern "C" fn(Id, Sel) -> NSRect, Imp>(
                            probe_rect,
                        )
                    },
                    c"{CGRect={CGPoint=dd}{CGSize=dd}}@:",
                ),
                (
                    c"crcblProbeArea:",
                    // SAFETY: as above.
                    unsafe {
                        core::mem::transmute::<unsafe extern "C" fn(Id, Sel, NSRect) -> f64, Imp>(
                            probe_area,
                        )
                    },
                    c"d@:{CGRect={CGPoint=dd}{CGSize=dd}}",
                ),
                (
                    c"crcblProbeFlag",
                    // SAFETY: as above.
                    unsafe {
                        core::mem::transmute::<unsafe extern "C" fn(Id, Sel) -> ObjcBool, Imp>(
                            probe_flag,
                        )
                    },
                    bool_encoding.as_c_str(),
                ),
            ];
            for (name, imp, types) in methods {
                // SAFETY: the class pair is allocated and not yet registered.
                let added =
                    unsafe { ffi::class_addMethod(class, ffi::sel(name), imp, types.as_ptr()) };
                assert_ne!(added, ffi::NO, "class_addMethod refused {name:?}");
            }
            // SAFETY: a complete, unregistered class pair.
            unsafe { ffi::objc_registerClassPair(class) };
            class as usize
        }) as Id
    }

    #[test]
    fn every_message_signature_this_backend_transmutes_dispatches_correctly() {
        // The aarch64 hazard, checked end to end: `objc_msgSend` has no variadic
        // ABI there, so a wrong signature reads its arguments out of the wrong
        // registers — it compiles, it links, it runs, and it is wrong. Both
        // sides of these calls are ours, so a mismatch is visible as a value
        // rather than as a crash somewhere later.
        let _pool = AutoreleasePool::push();
        let class = probe_class();
        // SAFETY: a registered class; `alloc`/`init` is how an `NSObject` is
        // made, and the object is released at the end of this test.
        let probe = unsafe {
            let allocated = ffi::msg(class, ffi::sel(c"alloc"));
            ffi::msg(allocated, ffi::sel(c"init"))
        };
        assert!(!probe.is_null(), "[[CrcblFfiProbe alloc] init] was nil");

        // A 32-byte struct **return**. On x86_64 this must go through
        // `objc_msgSend_stret` and on aarch64 it must not; getting it wrong on
        // either produces garbage rather than `PROBE_RECT`.
        // SAFETY: the probe implements this selector with this signature.
        let returned = unsafe { ffi::msg_rect(probe, ffi::sel(c"crcblProbeRect")) };
        assert_eq!(
            returned, PROBE_RECT,
            "an NSRect returned through the wrong msgSend trampoline"
        );

        // A 32-byte struct **argument**, which never uses the stret trampoline.
        // SAFETY: the probe implements this selector with this signature.
        let area = unsafe {
            let send: unsafe extern "C" fn(Id, Sel, NSRect) -> f64 = ffi::msg_send();
            send(probe, ffi::sel(c"crcblProbeArea:"), PROBE_RECT)
        };
        assert!(
            (area - PROBE_RECT.size.width * PROBE_RECT.size.height).abs() < 1e-9,
            "an NSRect argument arrived as something else: {area}"
        );

        // `BOOL`, which is `signed char` on one architecture and `_Bool` on the
        // other — the reason `ObjcBool` is `i8` and never Rust's `bool`.
        // SAFETY: the probe implements this selector with this signature.
        assert!(unsafe { ffi::msg_bool(probe, ffi::sel(c"crcblProbeFlag")) });

        // And the runtime's own answer about what it installed.
        // SAFETY: every object answers `respondsToSelector:`.
        unsafe {
            assert!(ffi::responds_to(probe, ffi::sel(c"crcblProbeRect")));
            assert!(!ffi::responds_to(probe, ffi::sel(c"crcblNoSuchMethod")));
            ffi::msg_void(probe, ffi::sel(c"release"));
        }
    }

    #[test]
    fn foundation_round_trips_through_the_named_message_wrappers() {
        // The other shapes, against classes whose behaviour is defined by
        // somebody else — so this checks the wrappers rather than checking a
        // probe against itself.
        let _pool = AutoreleasePool::push();

        // `msg_send` with a `*const c_char` argument, `msg` with no argument and
        // a pointer return, and the string helpers over both.
        // SAFETY: an autoreleased string, valid for the pool above.
        let string = unsafe { ffi::nsstring("59.94 Hz — ✓") }.expect("no interior NUL");
        // SAFETY: a live NSString.
        assert_eq!(
            unsafe { ffi::string_from_nsstring(string) }.as_deref(),
            Some("59.94 Hz — ✓"),
            "UTF-8 did not survive the round trip"
        );
        // SAFETY: a live NSString; a string with an interior NUL is refused
        // before any message is sent.
        assert!(unsafe { ffi::nsstring("bad\0title") }.is_none());

        // `msg_usize`: a count, in UTF-16 code units, which is what `length`
        // means and is not the byte length.
        // SAFETY: a live NSString.
        let length = unsafe { ffi::msg_usize(string, ffi::sel(c"length")) };
        assert_eq!(length, "59.94 Hz — ✓".encode_utf16().count());

        // `msg_f64`: a `CGFloat`/`double` return.
        // SAFETY: a live NSString; `doubleValue` parses its leading number.
        let parsed = unsafe { ffi::msg_f64(string, ffi::sel(c"doubleValue")) };
        assert!((parsed - 59.94).abs() < 1e-9, "doubleValue gave {parsed}");

        // A 16-byte struct return, which goes back in registers on both ABIs and
        // therefore must **not** use the stret trampoline — `msg_send` rather
        // than `msg_send_stret`, and the signature spelled out here because no
        // named wrapper has a second caller to earn one.
        let value_class = ffi::class(c"NSValue").expect("Foundation is linked");
        // SAFETY: `valueWithSize:` takes an `NSSize` and returns an
        // autoreleased `NSValue`; `sizeValue` reads it back.
        let (boxed, unboxed) = unsafe {
            let with_size: unsafe extern "C" fn(Id, Sel, NSSize) -> Id = ffi::msg_send();
            let boxed = with_size(
                value_class,
                ffi::sel(c"valueWithSize:"),
                NSSize::new(1280.0, 720.5),
            );
            let size_value: unsafe extern "C" fn(Id, Sel) -> NSSize = ffi::msg_send();
            (boxed, size_value(boxed, ffi::sel(c"sizeValue")))
        };
        assert!(!boxed.is_null());
        assert_eq!(unboxed, NSSize::new(1280.0, 720.5));

        // And `msg_rect` against Foundation rather than against our own probe,
        // so both halves of the stret decision are checked against code we did
        // not write.
        // SAFETY: `valueWithRect:` takes an `NSRect` and returns an
        // autoreleased `NSValue`; `rectValue` reads it back.
        let round_tripped = unsafe {
            let with_rect: unsafe extern "C" fn(Id, Sel, NSRect) -> Id = ffi::msg_send();
            let boxed = with_rect(value_class, ffi::sel(c"valueWithRect:"), PROBE_RECT);
            ffi::msg_rect(boxed, ffi::sel(c"rectValue"))
        };
        assert_eq!(round_tripped, PROBE_RECT);
        assert_eq!(round_tripped.origin, NSPoint { x: -12.5, y: 37.25 });
    }

    #[test]
    fn a_class_this_image_does_not_have_is_none_rather_than_nil() {
        // The difference between a diagnosis and a shell that sends messages to
        // `nil` forever, doing nothing and reporting zero.
        assert!(ffi::class(c"NSObject").is_some());
        assert!(ffi::class(c"NSApplication").is_some(), "AppKit is linked");
        assert!(
            ffi::class(c"CAMetalLayer").is_some(),
            "QuartzCore is linked"
        );
        assert!(ffi::class(c"NoSuchClassAnywhere").is_none());

        // Selectors always exist, because registering one is what asking for it
        // does — which is why `sel` has no `Option` and `class` does.
        assert!(!ffi::sel(c"description").is_null());
        assert_eq!(ffi::sel(c"description"), ffi::sel(c"description"));
        assert_ne!(ffi::sel(c"description"), ffi::sel(c"hash"));
    }

    #[test]
    fn opening_the_shell_off_the_main_thread_is_refused_by_name() {
        // The guard, exercised the way it will really be hit. AppKit raises an
        // Objective-C exception from `nextEventMatchingMask:` on a secondary
        // thread and an exception unwinding into Rust is undefined behaviour, so
        // "fails loudly" has to mean *before* the first AppKit call, not during
        // it.
        //
        // A thread is spawned explicitly rather than relying on the test body
        // already being on one: libtest's threading is not part of this
        // backend's contract, and a harness that ever ran bodies on the main
        // thread would silently turn this into a test of nothing.
        let outcome = std::thread::spawn(|| match AppKitShell::open() {
            Ok(_) => Err("a shell was opened on a spawned thread".to_string()),
            Err(error) => Ok(error.to_string()),
        })
        .join()
        .expect("the probe thread panicked");
        let message = outcome.expect("open() must refuse off the main thread");
        assert!(
            message.contains("main thread"),
            "the refusal has to name the rule, or nobody can act on it: {message}"
        );
    }

    #[test]
    fn the_runner_has_a_graphics_session_at_all() {
        // CoreGraphics rather than AppKit, deliberately: these calls are
        // documented thread-safe, so this is the one thing about the runner's
        // display that a spawned test body can ask. `0` is what a process with
        // no window server gets, and it is what a future window-session pass
        // would fail on first.
        let display = monitors::main_display();
        assert_ne!(
            display, 0,
            "CGMainDisplayID answered 0, so this runner has no graphics session and no \
             AppKit window could open on it either"
        );
        // Not asserted to be non-zero: CoreGraphics genuinely declines to state
        // a rate for some displays, and `MonitorInfo::refresh_millihertz`
        // documents zero as "cannot determine". Printed so that the first run
        // says which kind of display this is.
        let millihertz = monitors::refresh_millihertz(display);
        println!("main display {display}: {millihertz} mHz");
        assert!(millihertz == 0 || millihertz > 20_000, "{millihertz} mHz");
    }
}
