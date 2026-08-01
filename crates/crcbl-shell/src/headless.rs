//! [`HeadlessShell`] — a complete [`Shell`] with a virtual monitor, scripted
//! event injection, and no OS calls anywhere.
//!
//! It is the windowing counterpart of `crcbl-hal`'s `NullBackend`, and it has
//! the same three jobs, in increasing order of usefulness:
//!
//! 1. **Prove the seam is implementable without platform types.** If a trait
//!    method could not be written without naming a compositor, this module
//!    would not compile.
//! 2. **Be the substrate CI runs the engine through.**
//!    `docs/plan/15-windowing.md` is explicit that this is not a stub: "CI and
//!    `crcbl screenshot`/`sim` run the identical engine loop through it". A
//!    render test pairs it with [`SurfaceTarget::Offscreen`] — which
//!    [`surface_target`](Shell::surface_target) returns — so the golden-image
//!    path exercises the same acquire/present code a real window does.
//! 3. **Make window behaviour testable.** Resize storms, scale-factor changes
//!    mid-session, aspect-lock clamping, focus loss: all scriptable, all
//!    deterministic, none needing a nested compositor.
//!
//! # Deterministic by construction
//!
//! Nothing here reads a clock, a socket or an environment variable. Time comes
//! from an internal [`ManualTime`], so a test that drives both the shell and a
//! [`FrameClock`](crcbl_core::FrameClock) from the same schedule gets a session
//! that is byte-identical on every machine — the property
//! `docs/plan/12-testing.md` demands of every nondeterminism source.
//! [`HeadlessShell::set_time`] exists so the shell's clock can be slaved to a
//! test's own `ManualTime` rather than drifting alongside it.
//!
//! # It is *stricter* than a compositor, on purpose
//!
//! The temptation with a test double is to make every call succeed
//! immediately. That is the shape that proves nothing: a consumer written
//! against it grows an assumption the real backend then breaks, all at once,
//! long after the code was written. So where headless can be the harsher of the
//! two, it is.
//!
//! * **A new window has no size.** [`create_window`](Shell::create_window)
//!   returns a [`WindowId`] and queues *nothing*. The first
//!   [`Resized`](ShellEvent::Resized) is delivered by a later
//!   [`pump`](Shell::pump) — by default the second one, and
//!   [`set_configure_delay`](HeadlessShell::set_configure_delay) makes it
//!   later still. This models Wayland exactly: an `xdg_surface` is unsized and
//!   may not be given a buffer until the compositor answers with a `configure`,
//!   a round trip away. Consumers must loop on
//!   [`WindowState::size`] before creating a swapchain, and here they are made
//!   to. (The same trick as `crcbl-hal`'s injected readback latency, for the
//!   same reason.)
//! * **`set_mode` and `set_constraints` are requests, not commands.** They
//!   record the request in [`WindowState::requested_mode`] /
//!   [`requested_constraints`](WindowState::requested_constraints) and schedule
//!   a configure; the *effective* mode only appears in
//!   [`WindowConfiguration::mode`] once that configure is delivered. A consumer
//!   that reads back its own request as fact fails here.
//! * **Constraints are enforced on delivered configures.**
//!   [`SizeConstraints::apply`] runs, so aspect-locked and min/max behaviour is
//!   exercised. A tiling window manager enforces none of it — which is why
//!   in-renderer letterboxing is the universal fallback, and why
//!   [`inject`](HeadlessShell::inject) exists to feed sizes that violate the
//!   constraints outright.
//!
//! Where it is still *easier* than a compositor, and knowingly so: the injection
//! helpers ([`resize`](HeadlessShell::resize),
//! [`change_scale_factor`](HeadlessShell::change_scale_factor)) apply at once,
//! because they *are* the compositor speaking rather than the client asking.
//! They are also the escape hatch for a test that wants a configured window in
//! one line.
//!
//! # Where the model is now known to be wrong (P0.5a)
//!
//! The Wayland backend's end-to-end suite is the first thing that could check
//! any of this against a real compositor. Three differences came back, and they
//! are recorded here rather than only in the backend, because this is the file
//! consumers are written against:
//!
//! 1. **A first configure usually dictates no size at all.** Headless always
//!    delivers one. `xdg-shell` defines `0 × 0` as "you choose", and it is what
//!    sway sends for a freshly created toplevel — so a real backend supplies
//!    the fallback and the requested size stands. A consumer is unaffected
//!    (both shapes are "a size arrived"), but a *backend* author reading only
//!    this file would not expect it.
//! 2. **Size arrives before scale.** A window's scale factor comes from
//!    `wl_surface.enter`, which only fires once the surface is mapped — which
//!    needs a buffer, which needs the size. So the first configure is at scale
//!    1.0 and the true scale follows as a separate
//!    [`ScaleFactorChanged`](ShellEvent::ScaleFactorChanged).
//!    [`WindowConfiguration`] groups the two because they arrive in *one
//!    message*; it does not promise the first message is right about both.
//!    [`change_scale_factor`](HeadlessShell::change_scale_factor) is how a test
//!    reproduces the sequence.
//! 3. **Configured is not the same as managed.** A Wayland window that has
//!    never been given a buffer is answered once and then ignored: no
//!    compositor-chosen geometry, no fullscreen configure, not even present in
//!    the window manager's tree. Headless has no such state, and nothing in the
//!    seam expresses it. It is not a defect in the seam — the sequence a
//!    consumer must write (configure → swapchain → present → *then* expect real
//!    geometry) is the same either way — but a test that resizes a headless
//!    window before its first present is exercising something a compositor
//!    would not have done.
//!
//! # The clipboard's hard cases are settable (P0.5c)
//!
//! P0.5c found the same "test double only knows the easy path" problem in the
//! clipboard that P0.4 had fixed for window sizes: a real compositor's
//! clipboard is *not* always readable and *not* always claimable, and a shell
//! that always answered instantly would let a consumer grow exactly the
//! assumption Wayland then breaks. All three cases are now scriptable, and none
//! of them needs a compositor:
//!
//! | Real behaviour | Modelled by | What a consumer learns |
//! | --- | --- | --- |
//! | Wayland delivers `wl_data_device.selection` only to the focused client, asynchronously | [`set_clipboard_readable`](HeadlessShell::set_clipboard_readable) | a read is **held** until it can be answered, and is never answered [`Empty`](ClipboardContent::Empty) for "I have not looked yet" |
//! | A peer takes the transfer descriptor and never writes | [`set_clipboard_stalled`](HeadlessShell::set_clipboard_stalled) | the read ends as [`Unavailable`](ClipboardContent::Unavailable) — a failure, not an empty clipboard |
//! | `set_selection` needs a serial from real user input | [`require_user_interaction`](HeadlessShell::require_user_interaction) | [`clipboard_offer`](Shell::clipboard_offer) can fail with [`ShellError::NeedsUserInteraction`], and that is a policy rather than a bug |
//!
//! The deadline a held read gives up after is
//! [`clipboard_deadline`](HeadlessShell::clipboard_deadline), counted in
//! **pumps** rather than milliseconds so a test asserting the timeout is not a
//! flake. Defaults are the permissive ones — readable, not stalled, no
//! interaction required — because most platforms have no such rules and a test
//! that does not care about them should not have to say so.
//!
//! # Capabilities are settable, which is the point
//!
//! [`HeadlessShell::new`] reports [`ShellCaps::DESKTOP`] — everything. But
//! [`with_caps`](HeadlessShell::with_caps) lets a test report a *degraded* set:
//! a Wayland-shaped shell with no [`POINTER_WARP`](ShellCaps::POINTER_WARP) and
//! no [`ASPECT_HINT_HONORED`](ShellCaps::ASPECT_HINT_HONORED), say. That is how
//! the caps system gets tested before a second backend exists — a consumer that
//! branches on the platform instead of on caps fails against a headless shell
//! configured to look like the platform it forgot.

use std::collections::VecDeque;
use std::path::PathBuf;

use core::time::Duration;

use crcbl_core::input::{
    ButtonState, DeviceId, Keysym, Modifiers, PointerButton, Scancode, ScrollDelta,
};
use crcbl_core::time::{ManualTime, TimeSource};
use crcbl_core::{EventTime, Pool, SurfaceTarget};

use crate::{
    ClipboardContent, ClipboardOffer, ClipboardRequestId, CloseReply, CursorIcon, DisplayMode,
    KeyCode, MimeType, MonitorId, MonitorInfo, PhysicalPoint, PhysicalRect, PhysicalSize,
    PointerMode, ReceivedMime, Shell, ShellBackend, ShellCaps, ShellError, ShellEvent,
    SizeConstraints, WindowConfiguration, WindowDesc, WindowId, WindowState,
};

/// Resolution of the virtual monitor [`HeadlessShell::new`] creates.
///
/// 1920×1080 at scale 1.0: the size golden images are most often blessed at,
/// and small enough that a readback in a test is cheap.
pub const DEFAULT_MONITOR_SIZE: PhysicalSize = PhysicalSize::new(1920, 1080);

/// Refresh rate of the virtual monitor, in millihertz — exactly 60 Hz.
pub const DEFAULT_REFRESH_MILLIHERTZ: u32 = 60_000;

/// Pumps that must complete before a newly requested configure is delivered.
///
/// One, not zero: a consumer that creates a window and immediately reads its
/// size must fail, because that is what it will do on Wayland. See the [module
/// docs](self).
pub const DEFAULT_CONFIGURE_DELAY: u32 = 1;

/// The device id every injected input event carries unless a helper says
/// otherwise.
///
/// A real id rather than [`DeviceId::UNKNOWN`], so a consumer that routes by
/// device is exercised rather than short-circuited.
pub const VIRTUAL_DEVICE: DeviceId = DeviceId(1);

/// Pumps a held clipboard read waits before giving up.
///
/// The modelled form of the deadline every real backend needs: a read that
/// cannot be answered has to *end* rather than stay outstanding, and
/// [`ClipboardContent::Unavailable`] is how it ends. Counted in pumps rather
/// than in milliseconds, because a headless shell has no wall clock and a test
/// that waited on one would be a flake.
pub const DEFAULT_CLIPBOARD_DEADLINE: u32 = 4;

/// A clipboard read that has been accepted and not yet answered.
///
/// The seam's obligation 5 — "a read that cannot be answered yet is held, never
/// answered empty" — made into state. `HeadlessShell` models the case because
/// it is the one a consumer written only against the easy path gets wrong.
#[derive(Clone, Debug)]
struct PendingRead {
    window: WindowId,
    request: ClipboardRequestId,
    mime: MimeType,
    /// Pumps still to go before this is reported unavailable.
    countdown: u32,
}

/// A configure the window system has decided on but not yet delivered.
///
/// The whole point of the delay; see the [module docs](self).
#[derive(Clone, Copy, Debug)]
struct PendingConfigure {
    size: PhysicalSize,
    scale_factor: f64,
    mode: DisplayMode,
    /// Pumps still to go before this is delivered.
    countdown: u32,
}

/// A window inside the headless shell.
#[derive(Clone, Debug)]
struct HeadlessWindow {
    title: String,
    /// What has actually been reported. `None` until the first configure.
    configuration: Option<WindowConfiguration>,
    /// What is on its way, if anything.
    pending: Option<PendingConfigure>,
    requested_mode: DisplayMode,
    requested_constraints: SizeConstraints,
    focused: bool,
    visible: bool,
    pointer_mode: PointerMode,
    cursor: Option<CursorIcon>,
    close_pending: bool,
    accept_drops: bool,
}

impl HeadlessWindow {
    /// The client area, or `None` while unconfigured.
    fn size(&self) -> Option<PhysicalSize> {
        self.configuration.map(|config| config.size)
    }

    /// The scale factor to apply to a logical size right now.
    ///
    /// Falls back to the pending configure's, then to 1.0 — which is exactly
    /// what a Wayland client does before its first configure: render at scale 1
    /// and correct itself when told.
    fn scale_factor(&self) -> f64 {
        self.configuration
            .map(|config| config.scale_factor)
            .or_else(|| self.pending.map(|pending| pending.scale_factor))
            .unwrap_or(1.0)
    }

    /// The mode that is in effect, or — before the first configure — the one
    /// that is on its way.
    ///
    /// Deliberately *not* the pending configure's when the window is already
    /// configured: a compositor answering a `set_mode` with something else is a
    /// case the seam exists to express, so an injected
    /// [`resize`](HeadlessShell::resize) must report the mode currently in
    /// effect rather than adopting the one that was merely asked for.
    fn settled_mode(&self) -> DisplayMode {
        self.configuration
            .map(|config| config.mode)
            .or_else(|| self.pending.map(|pending| pending.mode))
            .unwrap_or(self.requested_mode)
    }

    /// The size a fresh configure should start from.
    fn base_size(&self) -> PhysicalSize {
        self.size()
            .or_else(|| self.pending.map(|pending| pending.size))
            .unwrap_or(PhysicalSize::new(1, 1))
    }
}

/// A [`Shell`] with no window system behind it.
///
/// See the [module docs](self) for what it models and where it deliberately
/// differs from a real compositor.
#[derive(Debug)]
pub struct HeadlessShell {
    caps: ShellCaps,
    monitors: Vec<MonitorInfo>,
    next_monitor_id: u32,
    windows: Pool<HeadlessWindow>,
    queue: VecDeque<ShellEvent>,
    clock: ManualTime,
    modifiers: Modifiers,
    clipboard: Vec<(ReceivedMime, Vec<u8>)>,
    next_request_id: u32,
    /// Reads that have been accepted and not yet answered; see
    /// [`PendingRead`].
    reads: Vec<PendingRead>,
    /// Whether the clipboard can be read at all right now — the modelled form
    /// of Wayland's focus gate. See
    /// [`set_clipboard_readable`](HeadlessShell::set_clipboard_readable).
    clipboard_readable: bool,
    /// Whether a peer holds the clipboard and never delivers.
    clipboard_stalled: bool,
    /// Whether claiming the clipboard requires a recent input event.
    clipboard_needs_interaction: bool,
    /// Whether any input has been injected since the last
    /// [`clear_user_interaction`](HeadlessShell::clear_user_interaction).
    saw_interaction: bool,
    /// Pumps a held read waits before it reports
    /// [`ClipboardContent::Unavailable`].
    clipboard_deadline: u32,
    waits: u32,
    configure_delay: u32,
}

impl Default for HeadlessShell {
    fn default() -> Self {
        Self::new()
    }
}

impl HeadlessShell {
    /// A shell with one 1920×1080 monitor at scale 1.0 and every capability.
    #[must_use]
    pub fn new() -> Self {
        Self {
            caps: ShellCaps::DESKTOP,
            monitors: vec![MonitorInfo {
                id: MonitorId(1),
                name: "HEADLESS-1".to_string(),
                bounds: PhysicalRect::new(
                    0,
                    0,
                    DEFAULT_MONITOR_SIZE.width,
                    DEFAULT_MONITOR_SIZE.height,
                ),
                work_area: PhysicalRect::new(
                    0,
                    0,
                    DEFAULT_MONITOR_SIZE.width,
                    DEFAULT_MONITOR_SIZE.height,
                ),
                scale_factor: 1.0,
                refresh_millihertz: DEFAULT_REFRESH_MILLIHERTZ,
                is_primary: true,
            }],
            next_monitor_id: 2,
            windows: Pool::new(),
            queue: VecDeque::new(),
            clock: ManualTime::new(),
            modifiers: Modifiers::empty(),
            clipboard: Vec::new(),
            next_request_id: 1,
            reads: Vec::new(),
            clipboard_readable: true,
            clipboard_stalled: false,
            clipboard_needs_interaction: false,
            saw_interaction: false,
            clipboard_deadline: DEFAULT_CLIPBOARD_DEADLINE,
            waits: 0,
            configure_delay: DEFAULT_CONFIGURE_DELAY,
        }
    }

    /// This shell, reporting only `caps`.
    ///
    /// The lever that makes the capability system testable; see the [module
    /// docs](self).
    #[must_use]
    pub fn with_caps(mut self, caps: ShellCaps) -> Self {
        self.caps = caps;
        self
    }

    /// This shell, with `monitors` instead of the default one.
    ///
    /// # Panics
    ///
    /// If `monitors` is empty. A shell with no monitor cannot answer where a
    /// borderless window goes, and every backend that could report zero
    /// monitors (a headless X server, a disconnected session) is a case the
    /// engine treats as "no display".
    ///
    /// If two monitors share a [`MonitorId`], or if one uses `u32::MAX`. Ids
    /// are the key [`Shell::monitor`] looks up and the ones
    /// [`plug_monitor`](Self::plug_monitor) hands out afterwards, so a
    /// duplicate makes the lookup ambiguous and `u32::MAX` leaves no id for the
    /// next hotplug.
    #[must_use]
    pub fn with_monitors(mut self, monitors: Vec<MonitorInfo>) -> Self {
        assert!(!monitors.is_empty(), "a shell needs at least one monitor");
        for (index, monitor) in monitors.iter().enumerate() {
            assert!(
                monitor.id.0 != u32::MAX,
                "monitor id {} leaves no id for the next hotplug",
                monitor.id.0
            );
            assert!(
                !monitors[..index].iter().any(|seen| seen.id == monitor.id),
                "monitor id {} appears twice",
                monitor.id.0
            );
        }
        self.next_monitor_id = monitors
            .iter()
            .map(|monitor| monitor.id.0 + 1)
            .max()
            .unwrap_or(1);
        self.monitors = monitors;
        self
    }

    /// This shell, with the primary monitor at `scale_factor`.
    ///
    /// Convenience for the common DPI test, which otherwise has to build a
    /// whole [`MonitorInfo`].
    #[must_use]
    pub fn with_scale_factor(mut self, scale_factor: f64) -> Self {
        assert!(scale_factor > 0.0, "scale factor must be positive");
        self.monitors[0].scale_factor = scale_factor;
        self
    }

    /// This shell, delivering the first configure after `pumps` calls to
    /// [`pump`](Shell::pump).
    ///
    /// See [`set_configure_delay`](Self::set_configure_delay).
    #[must_use]
    pub const fn with_configure_delay(mut self, pumps: u32) -> Self {
        self.configure_delay = pumps;
        self
    }

    /// How many pumps a configure waits before it is delivered.
    ///
    /// Defaults to [`DEFAULT_CONFIGURE_DELAY`]. Raise it to prove that a
    /// consumer's create-then-wait loop is a real loop rather than an
    /// off-by-one that happens to work; drop it to `0` when a test is about
    /// something else entirely and wants a window configured on the first
    /// pump.
    ///
    /// Applies to configures scheduled *after* this call.
    pub const fn set_configure_delay(&mut self, pumps: u32) {
        self.configure_delay = pumps;
    }

    /// Replaces the reported capabilities mid-session.
    ///
    /// Real backends must not do this — [`Shell::caps`] is latched — but a test
    /// building one shell per capability set is noise. Setting it before the
    /// first [`create_window`](Shell::create_window) is the intended use.
    pub fn set_caps(&mut self, caps: ShellCaps) {
        self.caps = caps;
    }

    /// The shell's current time, which every injected event is stamped with.
    #[must_use]
    pub fn now(&self) -> EventTime {
        EventTime::from_duration(self.clock.elapsed())
    }

    /// Moves the shell's clock forward.
    ///
    /// Injected events are stamped with the time *after* the advance, so
    /// `advance(200ms)` between a press and a release produces a 200 ms hold —
    /// which is how the P2 pattern evaluator's tap/hold table gets tested.
    pub fn advance(&mut self, delta: Duration) {
        self.clock.advance(delta);
    }

    /// Slaves the shell's clock to an absolute timestamp.
    ///
    /// Pass `time.elapsed()` from the test's own
    /// [`ManualTime`] each frame and the shell's
    /// event timestamps land on the same timeline as the
    /// [`FrameClock`](crcbl_core::FrameClock)'s, which is what
    /// [`EventTime`]'s epoch contract requires of a real backend.
    pub fn set_time(&mut self, now: Duration) {
        self.clock.set(now);
    }

    /// The modifier state stamped onto subsequent injected events.
    ///
    /// Sticky, like a real backend's tracked state — see [`event`](crate::event)
    /// for why modifiers ride on each event rather than arriving separately.
    pub fn set_modifiers(&mut self, modifiers: Modifiers) {
        self.modifiers = modifiers;
    }

    /// How many events are queued and not yet pumped.
    #[must_use]
    pub fn pending_events(&self) -> usize {
        self.queue.len()
    }

    /// How many times [`Shell::wait_events`] has been called.
    ///
    /// A test for an editor-style idle loop asserts that it actually waits
    /// instead of spinning, and there is nothing else observable about a
    /// headless wait.
    #[must_use]
    pub const fn wait_count(&self) -> u32 {
        self.waits
    }

    /// Queues an event verbatim, with no state update and no validation.
    ///
    /// The escape hatch: use it to inject sequences a well-behaved compositor
    /// would not produce — a resize that violates the constraints, events for a
    /// destroyed window, a scale change with no resize — because those are
    /// exactly the cases a consumer is most likely to get wrong. The typed
    /// helpers below keep the shell's state consistent; this one does not.
    pub fn inject(&mut self, event: ShellEvent) {
        self.queue.push_back(event);
    }

    /// Delivers a configure at `size` immediately, as a compositor answering a
    /// resize would.
    ///
    /// This is the *compositor* speaking, not the client asking, so it takes
    /// effect at once and supersedes any pending configure. It is therefore
    /// also the one-line way to get a configured window in a test that is about
    /// something other than the configure handshake.
    ///
    /// The window's constraints are applied, because a compositor that honours
    /// hints would have applied them. Use [`inject`](Self::inject) to feed a
    /// size that violates them, which is what a tiling window manager does.
    ///
    /// # Errors
    ///
    /// [`ShellError::InvalidWindow`] if the handle is stale.
    pub fn resize(&mut self, window: WindowId, size: PhysicalSize) -> Result<(), ShellError> {
        let state = self.window_mut(window)?;
        let scale_factor = state.scale_factor();
        let mode = state.settled_mode();
        let constrained = state.requested_constraints.apply(size, scale_factor);
        state.pending = None;
        state.configuration = Some(WindowConfiguration {
            size: constrained,
            scale_factor,
            mode,
        });
        self.queue.push_back(ShellEvent::Resized {
            window,
            size: constrained,
            scale_factor,
        });
        Ok(())
    }

    /// Changes a window's scale factor, as dragging it to another monitor
    /// would.
    ///
    /// The window's *logical* size is preserved and the physical size
    /// recomputed, which is what both Wayland (`wl_surface.set_buffer_scale`
    /// against an unchanged window geometry) and Win32 (`WM_DPICHANGED`'s
    /// suggested rectangle) do. Queues a
    /// [`ScaleFactorChanged`](ShellEvent::ScaleFactorChanged) carrying both
    /// facts, so a consumer cannot hold a mismatched size/scale pair, followed
    /// by the [`Resized`](ShellEvent::Resized) it implies — which is the pair
    /// both Linux backends emit for a scale change, and the pair
    /// [`deliver_due_configures`](Self::deliver_due_configures) emits for a
    /// scale change that arrives with a configure.
    ///
    /// # Errors
    ///
    /// [`ShellError::InvalidWindow`] if the handle is stale.
    ///
    /// # Panics
    ///
    /// If `scale_factor` is not positive.
    pub fn change_scale_factor(
        &mut self,
        window: WindowId,
        scale_factor: f64,
    ) -> Result<(), ShellError> {
        assert!(scale_factor > 0.0, "scale factor must be positive");
        let state = self.window_mut(window)?;
        let logical = state.base_size().to_logical(state.scale_factor());
        let mode = state.settled_mode();
        let size = state
            .requested_constraints
            .apply(logical.to_physical(scale_factor), scale_factor);
        state.pending = None;
        state.configuration = Some(WindowConfiguration {
            size,
            scale_factor,
            mode,
        });
        self.queue.push_back(ShellEvent::ScaleFactorChanged {
            window,
            scale_factor,
            size,
        });
        self.queue.push_back(ShellEvent::Resized {
            window,
            size,
            scale_factor,
        });
        Ok(())
    }

    /// Gives or takes keyboard focus.
    ///
    /// # Errors
    ///
    /// [`ShellError::InvalidWindow`] if the handle is stale.
    pub fn set_focus(&mut self, window: WindowId, focused: bool) -> Result<(), ShellError> {
        self.window_mut(window)?.focused = focused;
        self.queue.push_back(ShellEvent::Focus { window, focused });
        Ok(())
    }

    /// Asks the application to close the window, as a title-bar button would.
    ///
    /// The window stays open until
    /// [`reply_close_request`](Shell::reply_close_request).
    ///
    /// # Errors
    ///
    /// [`ShellError::InvalidWindow`] if the handle is stale.
    pub fn request_close(&mut self, window: WindowId) -> Result<(), ShellError> {
        self.window_mut(window)?.close_pending = true;
        self.queue.push_back(ShellEvent::CloseRequested { window });
        Ok(())
    }

    /// Injects a key press or release.
    ///
    /// The scancode and keysym are synthesized deterministically from `key`:
    /// the scancode is the key's index in [`KeyCode::ALL`] offset by 8 (evdev's
    /// convention, so the numbers look plausible in a log), and the keysym is
    /// the US-layout symbol where there is one. Neither is a claim about any
    /// real keyboard; they exist so a consumer reading those fields is
    /// exercised rather than reading zero.
    ///
    /// # Errors
    ///
    /// [`ShellError::InvalidWindow`] if the handle is stale.
    pub fn key(
        &mut self,
        window: WindowId,
        key: KeyCode,
        state: ButtonState,
    ) -> Result<(), ShellError> {
        self.key_with(window, key, state, false)
    }

    /// Injects a key press.
    ///
    /// # Errors
    ///
    /// [`ShellError::InvalidWindow`] if the handle is stale.
    pub fn key_press(&mut self, window: WindowId, key: KeyCode) -> Result<(), ShellError> {
        self.key(window, key, ButtonState::Pressed)
    }

    /// Injects a key release.
    ///
    /// # Errors
    ///
    /// [`ShellError::InvalidWindow`] if the handle is stale.
    pub fn key_release(&mut self, window: WindowId, key: KeyCode) -> Result<(), ShellError> {
        self.key(window, key, ButtonState::Released)
    }

    /// Injects an auto-repeat press, the kind a text field wants and a jump
    /// button must ignore.
    ///
    /// # Errors
    ///
    /// [`ShellError::InvalidWindow`] if the handle is stale.
    pub fn key_repeat(&mut self, window: WindowId, key: KeyCode) -> Result<(), ShellError> {
        self.key_with(window, key, ButtonState::Pressed, true)
    }

    fn key_with(
        &mut self,
        window: WindowId,
        key: KeyCode,
        state: ButtonState,
        repeat: bool,
    ) -> Result<(), ShellError> {
        self.check_window(window)?;
        self.saw_interaction = true;
        let event = ShellEvent::Key {
            window,
            device: VIRTUAL_DEVICE,
            time: self.now(),
            scancode: synthetic_scancode(key),
            key_code: Some(key),
            keysym: synthetic_keysym(key),
            state,
            repeat,
            modifiers: self.modifiers,
        };
        self.queue.push_back(event);
        Ok(())
    }

    /// Moves the pointer to an absolute position, reporting the implied
    /// relative delta as well.
    ///
    /// Under [`PointerMode::Locked`] the absolute position is dropped, matching
    /// what the mode documents — which is what makes a camera that wrongly
    /// reads `abs` fail here rather than in a playtest.
    ///
    /// # Errors
    ///
    /// [`ShellError::InvalidWindow`] if the handle is stale.
    pub fn move_pointer(
        &mut self,
        window: WindowId,
        position: PhysicalPoint,
        raw_delta: (f64, f64),
    ) -> Result<(), ShellError> {
        let locked = self.window(window)?.pointer_mode == PointerMode::Locked;
        let raw = if self.caps.contains(ShellCaps::RAW_POINTER_MOTION) {
            Some(raw_delta)
        } else {
            None
        };
        self.saw_interaction = true;
        let event = ShellEvent::PointerMotion {
            window,
            device: VIRTUAL_DEVICE,
            time: self.now(),
            abs: if locked { None } else { Some(position) },
            raw_delta: raw,
        };
        self.queue.push_back(event);
        Ok(())
    }

    /// Moves the pointer into or out of the window.
    ///
    /// # Errors
    ///
    /// [`ShellError::InvalidWindow`] if the handle is stale.
    pub fn set_pointer_focus(
        &mut self,
        window: WindowId,
        entered: bool,
        position: Option<PhysicalPoint>,
    ) -> Result<(), ShellError> {
        self.check_window(window)?;
        let event = ShellEvent::PointerFocus {
            window,
            device: VIRTUAL_DEVICE,
            time: self.now(),
            entered,
            position: if entered { position } else { None },
        };
        self.queue.push_back(event);
        Ok(())
    }

    /// Injects a pointer button press or release.
    ///
    /// # Errors
    ///
    /// [`ShellError::InvalidWindow`] if the handle is stale.
    pub fn button(
        &mut self,
        window: WindowId,
        button: PointerButton,
        state: ButtonState,
        position: Option<PhysicalPoint>,
    ) -> Result<(), ShellError> {
        self.check_window(window)?;
        self.saw_interaction = true;
        let event = ShellEvent::Button {
            window,
            device: VIRTUAL_DEVICE,
            time: self.now(),
            button,
            state,
            position,
            modifiers: self.modifiers,
        };
        self.queue.push_back(event);
        Ok(())
    }

    /// Injects a scroll.
    ///
    /// # Errors
    ///
    /// [`ShellError::InvalidWindow`] if the handle is stale.
    pub fn scroll(
        &mut self,
        window: WindowId,
        delta: ScrollDelta,
        position: Option<PhysicalPoint>,
    ) -> Result<(), ShellError> {
        self.check_window(window)?;
        self.saw_interaction = true;
        let event = ShellEvent::Wheel {
            window,
            device: VIRTUAL_DEVICE,
            time: self.now(),
            delta,
            position,
            modifiers: self.modifiers,
        };
        self.queue.push_back(event);
        Ok(())
    }

    /// Injects committed text, as an input method would.
    ///
    /// # Errors
    ///
    /// [`ShellError::InvalidWindow`] if the handle is stale.
    ///
    /// # Panics
    ///
    /// If `text` is empty — [`ShellEvent::TextCommit`] documents that it never
    /// is, and a shell that could produce one would make that a lie.
    pub fn commit_text(&mut self, window: WindowId, text: &str) -> Result<(), ShellError> {
        assert!(!text.is_empty(), "a text commit is never empty");
        self.check_window(window)?;
        let event = ShellEvent::TextCommit {
            window,
            time: self.now(),
            text: text.to_string(),
        };
        self.queue.push_back(event);
        Ok(())
    }

    /// Drops a file on the window.
    ///
    /// # Errors
    ///
    /// [`ShellError::InvalidWindow`] if the handle is stale, or
    /// [`ShellError::Unsupported`] if the window did not opt into drops or the
    /// shell lacks [`ShellCaps::DRAG_DROP`].
    pub fn drop_file(
        &mut self,
        window: WindowId,
        path: impl Into<PathBuf>,
        position: Option<PhysicalPoint>,
    ) -> Result<(), ShellError> {
        if !self.caps.contains(ShellCaps::DRAG_DROP) {
            return Err(self.unsupported("drag and drop"));
        }
        if !self.window(window)?.accept_drops {
            return Err(ShellError::Unsupported {
                backend: ShellBackend::Headless,
                what: "drops on a window created without accept_drops",
            });
        }
        let event = ShellEvent::DroppedFile {
            window,
            time: self.now(),
            path: path.into(),
            position,
        };
        self.queue.push_back(event);
        Ok(())
    }

    /// Adds a monitor and queues a
    /// [`MonitorsChanged`](ShellEvent::MonitorsChanged).
    ///
    /// The id is assigned by the shell and never reuses one that has been
    /// handed out, which is the obligation
    /// [`monitor`](crate::monitor) places on every backend.
    pub fn plug_monitor(&mut self, mut info: MonitorInfo) -> MonitorId {
        let id = MonitorId(self.next_monitor_id);
        self.next_monitor_id += 1;
        info.id = id;
        self.monitors.push(info);
        self.queue.push_back(ShellEvent::MonitorsChanged);
        id
    }

    /// Removes a monitor and queues a
    /// [`MonitorsChanged`](ShellEvent::MonitorsChanged).
    ///
    /// # Errors
    ///
    /// [`ShellError::NoSuchMonitor`] if it is not connected.
    ///
    /// # Panics
    ///
    /// If it is the last monitor; see [`with_monitors`](Self::with_monitors).
    pub fn unplug_monitor(&mut self, id: MonitorId) -> Result<(), ShellError> {
        let index = self
            .monitors
            .iter()
            .position(|monitor| monitor.id == id)
            .ok_or(ShellError::NoSuchMonitor(id.0))?;
        assert!(
            self.monitors.len() > 1,
            "a shell needs at least one monitor"
        );
        self.monitors.remove(index);
        self.queue.push_back(ShellEvent::MonitorsChanged);
        Ok(())
    }

    /// The window's title, which no [`Shell`] method exposes.
    ///
    /// Titles are write-only across the seam — no backend can read one back
    /// reliably, since a window manager may decorate or truncate it — so this
    /// is a headless-only accessor for tests that assert
    /// [`set_title`](Shell::set_title) did something.
    ///
    /// # Errors
    ///
    /// [`ShellError::InvalidWindow`] if the handle is stale.
    pub fn title(&self, window: WindowId) -> Result<&str, ShellError> {
        Ok(self.window(window)?.title.as_str())
    }

    /// The window's current cursor, or `None` if it is hidden.
    ///
    /// Headless-only, for the same reason as [`title`](Self::title).
    ///
    /// # Errors
    ///
    /// [`ShellError::InvalidWindow`] if the handle is stale.
    pub fn cursor(&self, window: WindowId) -> Result<Option<CursorIcon>, ShellError> {
        Ok(self.window(window)?.cursor)
    }

    /// The bytes currently offered on the in-process clipboard for `mime`.
    ///
    /// Headless-only.
    #[must_use]
    pub fn clipboard_bytes(&self, mime: MimeType) -> Option<&[u8]> {
        self.clipboard
            .iter()
            .find(|(offered, _)| offered.matches(mime))
            .map(|(_, bytes)| bytes.as_slice())
    }

    /// Whether the clipboard can be read at all right now.
    ///
    /// **The focus gate, modelled.** On Wayland a client learns what is on the
    /// clipboard from `wl_data_device.selection`, which arrives only while one
    /// of its windows has keyboard focus — so a background window cannot read
    /// the clipboard, and a read issued a moment before focus arrives has
    /// nothing to answer from. `HeadlessShell` is readable by default, because
    /// most platforms have no such gate; set this to `false` to make a test
    /// walk the path a Wayland consumer will walk.
    ///
    /// A read issued while this is `false` is **held**, not answered: it is
    /// answered as soon as this becomes `true` again, or reported
    /// [`ClipboardContent::Unavailable`] once
    /// [`clipboard_deadline`](Self::clipboard_deadline) pumps have gone by.
    /// That is the seam's obligation 5, and this is how a consumer is tested
    /// against it.
    pub fn set_clipboard_readable(&mut self, readable: bool) {
        self.clipboard_readable = readable;
    }

    /// Whether a peer holds the clipboard and never hands the data over.
    ///
    /// The other way a read can fail to complete, and the one that has no
    /// protocol event behind it: on Wayland the transfer is a pipe another
    /// application writes, and an application that accepts the descriptor and
    /// then hangs produces exactly this. The read is held and then reported
    /// [`ClipboardContent::Unavailable`] — never `Empty`, because there really
    /// is something on the clipboard.
    pub fn set_clipboard_stalled(&mut self, stalled: bool) {
        self.clipboard_stalled = stalled;
    }

    /// Whether claiming the clipboard requires a recent user interaction.
    ///
    /// **The serial requirement, modelled.** Off by default, because X11 and
    /// Win32 have no such rule; on, [`clipboard_offer`](Shell::clipboard_offer)
    /// fails with [`ShellError::NeedsUserInteraction`] until an input event has
    /// been injected — a key, a pointer button, a scroll or a motion. That is
    /// what Wayland's `set_selection` serial and the browser's user-gesture gate
    /// both amount to, and it is a *feature*: it is what stops a background
    /// process taking the clipboard.
    ///
    /// ```
    /// use crcbl_shell::{ClipboardOffer, HeadlessShell, KeyCode, Shell, ShellError, WindowDesc};
    ///
    /// let mut shell = HeadlessShell::new();
    /// let window = shell.create_window(&WindowDesc::default())?;
    /// shell.require_user_interaction(true);
    ///
    /// // Nobody has touched anything, so the clipboard is not ours to take.
    /// let refused = shell.clipboard_offer(window, &[ClipboardOffer::text("x")]);
    /// assert!(matches!(refused, Err(ShellError::NeedsUserInteraction { .. })));
    ///
    /// // The copy the user actually asked for goes through.
    /// shell.key_press(window, KeyCode::KeyC)?;
    /// shell.clipboard_offer(window, &[ClipboardOffer::text("x")])?;
    /// # Ok::<(), ShellError>(())
    /// ```
    pub fn require_user_interaction(&mut self, required: bool) {
        self.clipboard_needs_interaction = required;
    }

    /// Forgets that any input has been injected.
    ///
    /// For a test that wants a *second* refusal after a successful copy — a
    /// real serial goes stale the same way.
    pub fn clear_user_interaction(&mut self) {
        self.saw_interaction = false;
    }

    /// How many pumps a held read waits before it reports
    /// [`ClipboardContent::Unavailable`].
    ///
    /// Defaults to [`DEFAULT_CLIPBOARD_DEADLINE`]. Zero means the very next
    /// pump gives up, which is the shortest way to write the timeout case.
    pub fn clipboard_deadline(&mut self, pumps: u32) {
        self.clipboard_deadline = pumps;
    }

    /// How many reads have been accepted and not yet answered.
    ///
    /// Headless-only, so a test can assert that a request really is being
    /// *held* rather than having been answered and missed.
    #[must_use]
    pub fn held_clipboard_reads(&self) -> usize {
        self.reads.len()
    }

    /// Replaces the clipboard with content spelled the way *another*
    /// application would spell it.
    ///
    /// [`clipboard_offer`](Shell::clipboard_offer) can only publish the mimes
    /// this engine names, so it cannot produce the case that matters: a paste
    /// from a foreign application whose mime string is neither `'static` nor
    /// one of ours. This can.
    pub fn set_foreign_clipboard(&mut self, offers: Vec<(ReceivedMime, Vec<u8>)>) {
        self.clipboard = offers;
    }

    /// Whether a read issued now could be answered from what this shell knows.
    ///
    /// Two ways to say no, and they model different real things: the clipboard
    /// is not readable at all (Wayland's focus gate), or a peer owns it and
    /// never delivers (a hung application on the other end of the pipe).
    fn answerable(&self) -> bool {
        self.clipboard_readable && !self.clipboard_stalled
    }

    /// The answer to one read, from the current clipboard.
    fn answer(&self, window: WindowId, request: ClipboardRequestId, mime: MimeType) -> ShellEvent {
        // Match on format, not on spelling: a peer offering `text/plain`
        // satisfies a request for `text/plain;charset=utf-8`. The answer
        // reports the peer's spelling, because that is what a real backend has
        // in hand and what an editor may need to echo back.
        let found = self
            .clipboard
            .iter()
            .find(|(offered, _)| offered.matches(mime));
        let (mime, content) = match found {
            Some((offered, bytes)) => (offered.clone(), ClipboardContent::Bytes(bytes.clone())),
            // Nothing on the clipboard, or nothing in this format — one case,
            // and emphatically not the same as "could not read it".
            None => (ReceivedMime::from(mime), ClipboardContent::Empty),
        };
        ShellEvent::ClipboardData {
            window,
            request,
            mime,
            content,
        }
    }

    /// Answers every held read that has become answerable, and gives up on the
    /// ones that have run out of pumps.
    ///
    /// Runs at the top of [`pump`](Shell::pump), beside the configure delivery,
    /// because a read is the same kind of thing: something the window system
    /// will get to, not something that has already happened.
    fn resolve_reads(&mut self) {
        if self.reads.is_empty() {
            return;
        }
        let answerable = self.answerable();
        let mut held = Vec::with_capacity(self.reads.len());
        for mut read in core::mem::take(&mut self.reads) {
            // A window destroyed while its read was outstanding: the answer
            // would name a stale handle, which the seam forbids. Obligation 4
            // is about *accepted* requests, and this one's window is gone.
            if self.window(read.window).is_err() {
                continue;
            }
            if answerable {
                let answer = self.answer(read.window, read.request, read.mime);
                self.queue.push_back(answer);
                continue;
            }
            if read.countdown > 0 {
                read.countdown -= 1;
                held.push(read);
                continue;
            }
            // Out of time. Ending the request is the obligation; ending it with
            // `Empty` would be the lie this whole shape exists to prevent.
            self.queue.push_back(ShellEvent::ClipboardData {
                window: read.window,
                request: read.request,
                mime: ReceivedMime::from(read.mime),
                content: ClipboardContent::Unavailable,
            });
        }
        self.reads = held;
    }

    /// Delivers every configure whose countdown has run out, and ticks the
    /// rest.
    ///
    /// Runs at the top of [`pump`](Shell::pump), so a configure scheduled
    /// during frame *n* is visible to the consumer no earlier than frame
    /// *n + 1 + delay*.
    fn deliver_due_configures(&mut self) {
        // Two phases: the window pool is borrowed mutably while deciding, and
        // the event queue is a separate field that cannot be touched until that
        // borrow ends.
        let mut due: Vec<(WindowId, PendingConfigure, Option<f64>)> = Vec::new();
        for (handle, state) in self.windows.iter_mut() {
            let Some(pending) = state.pending.as_mut() else {
                continue;
            };
            if pending.countdown > 0 {
                pending.countdown -= 1;
                continue;
            }
            let pending = state.pending.take().expect("checked just above");
            let previous_scale = state.configuration.map(|config| config.scale_factor);
            state.configuration = Some(WindowConfiguration {
                size: pending.size,
                scale_factor: pending.scale_factor,
                mode: pending.mode,
            });
            due.push((handle.cast(), pending, previous_scale));
        }

        for (window, pending, previous_scale) in due {
            // A *change* of scale needs its own event; the first configure does
            // not, because `Resized` already carries the scale and there was
            // nothing to change from.
            if previous_scale.is_some_and(|previous| previous != pending.scale_factor) {
                self.queue.push_back(ShellEvent::ScaleFactorChanged {
                    window,
                    scale_factor: pending.scale_factor,
                    size: pending.size,
                });
            }
            self.queue.push_back(ShellEvent::Resized {
                window,
                size: pending.size,
                scale_factor: pending.scale_factor,
            });
        }
    }

    fn window(&self, window: WindowId) -> Result<&HeadlessWindow, ShellError> {
        self.windows
            .get(window.cast())
            .ok_or_else(|| ShellError::invalid_window(window))
    }

    fn window_mut(&mut self, window: WindowId) -> Result<&mut HeadlessWindow, ShellError> {
        self.windows
            .get_mut(window.cast())
            .ok_or_else(|| ShellError::invalid_window(window))
    }

    fn check_window(&self, window: WindowId) -> Result<(), ShellError> {
        self.window(window).map(|_| ())
    }

    fn unsupported(&self, what: &'static str) -> ShellError {
        ShellError::Unsupported {
            backend: ShellBackend::Headless,
            what,
        }
    }
}

/// A stable, plausible-looking scancode for a key.
///
/// Not a claim about any real keyboard; see [`HeadlessShell::key`].
fn synthetic_scancode(key: KeyCode) -> Scancode {
    let index = KeyCode::ALL
        .iter()
        .position(|candidate| *candidate == key)
        .unwrap_or(0);
    // evdev codes reach the client offset by 8 on both Wayland and X11, so
    // starting there keeps the numbers in a range that looks right in a log.
    Scancode(index as u32 + 8)
}

/// The US-layout symbol a key produces, where it produces one.
fn synthetic_keysym(key: KeyCode) -> Keysym {
    let name = key.as_str();
    if let Some(letter) = name.strip_prefix("Key") {
        // Unshifted, as XKB reports for a key with no modifiers.
        return Keysym::from_char(letter.to_ascii_lowercase().chars().next().unwrap_or('?'));
    }
    if let Some(digit) = name.strip_prefix("Digit") {
        return Keysym::from_char(digit.chars().next().unwrap_or('?'));
    }
    match key {
        KeyCode::Space => Keysym::from_char(' '),
        KeyCode::Minus => Keysym::from_char('-'),
        KeyCode::Equal => Keysym::from_char('='),
        KeyCode::Comma => Keysym::from_char(','),
        KeyCode::Period => Keysym::from_char('.'),
        KeyCode::Slash => Keysym::from_char('/'),
        KeyCode::Semicolon => Keysym::from_char(';'),
        KeyCode::Quote => Keysym::from_char('\''),
        KeyCode::Backslash => Keysym::from_char('\\'),
        KeyCode::BracketLeft => Keysym::from_char('['),
        KeyCode::BracketRight => Keysym::from_char(']'),
        KeyCode::Backquote => Keysym::from_char('`'),
        // Function keys, modifiers and navigation produce no character; a real
        // backend reports their named keysyms, which nothing in the engine
        // reads today.
        _ => Keysym::NONE,
    }
}

impl Shell for HeadlessShell {
    fn backend(&self) -> ShellBackend {
        ShellBackend::Headless
    }

    fn caps(&self) -> ShellCaps {
        self.caps
    }

    fn create_window(&mut self, desc: &WindowDesc<'_>) -> Result<WindowId, ShellError> {
        if !self.windows.is_empty() && !self.caps.contains(ShellCaps::MULTI_WINDOW) {
            return Err(self.unsupported("a second window"));
        }
        if let DisplayMode::Borderless {
            monitor: Some(monitor),
        } = desc.mode
            && self.monitor(monitor).is_none()
        {
            return Err(ShellError::NoSuchMonitor(monitor.0));
        }

        // `desc.accept_drops` without `ShellCaps::DRAG_DROP` is deliberately not
        // an error: it is a hint, and on a backend with no drag-and-drop it is
        // simply a hint nobody reads — which is what X11 does with it too. The
        // request is inert rather than ignored, because
        // [`drop_file`](Self::drop_file) refuses without the capability.
        //
        // The scale factor comes from the monitor the window lands on, not from
        // the primary one: a borderless window on a HiDPI secondary monitor is
        // exactly the case `docs/plan/15-windowing.md`'s DPI matrix is looking
        // for, and reporting the primary's scale there produces a swapchain at
        // the wrong size. `set_mode` takes the same pair from the same place.
        let (size, scale_factor) = match desc.mode {
            DisplayMode::Windowed => {
                let scale_factor = self.monitors[0].scale_factor;
                (
                    desc.constraints
                        .apply(desc.size.to_physical(scale_factor), scale_factor),
                    scale_factor,
                )
            }
            DisplayMode::Borderless { monitor } => {
                let monitor = monitor
                    .and_then(|id| self.monitor(id))
                    .unwrap_or(&self.monitors[0]);
                (monitor.size(), monitor.scale_factor)
            }
        };

        let countdown = self.configure_delay;
        let window: WindowId = self
            .windows
            .insert(HeadlessWindow {
                title: desc.title.to_string(),
                // Unsized until the compositor says otherwise. This is the
                // whole point; see the module docs.
                configuration: None,
                pending: Some(PendingConfigure {
                    size,
                    scale_factor,
                    mode: desc.mode,
                    countdown,
                }),
                requested_mode: desc.mode,
                requested_constraints: desc.constraints,
                focused: false,
                visible: desc.visible,
                pointer_mode: PointerMode::Free,
                cursor: Some(CursorIcon::Default),
                close_pending: false,
                accept_drops: desc.accept_drops,
            })
            .cast();
        Ok(window)
    }

    fn destroy_window(&mut self, window: WindowId) -> Result<(), ShellError> {
        self.windows
            .remove(window.cast())
            .ok_or_else(|| ShellError::invalid_window(window))?;
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
        self.window_mut(window)?.title = title.to_string();
        Ok(())
    }

    fn set_visible(&mut self, window: WindowId, visible: bool) -> Result<(), ShellError> {
        self.window_mut(window)?.visible = visible;
        Ok(())
    }

    fn set_mode(&mut self, window: WindowId, mode: DisplayMode) -> Result<(), ShellError> {
        let target = match mode {
            DisplayMode::Windowed => None,
            DisplayMode::Borderless { monitor } => {
                let monitor = match monitor {
                    Some(id) => self
                        .monitor(id)
                        .ok_or(ShellError::NoSuchMonitor(id.0))?
                        .clone(),
                    None => self.monitors[0].clone(),
                };
                Some(monitor)
            }
        };
        let countdown = self.configure_delay;

        let state = self.window_mut(window)?;
        // The request is recorded immediately — we said it, so it is a fact —
        // and the *effect* is scheduled. Nothing about the window's reported
        // configuration changes until the configure is delivered, so a consumer
        // that reads back its own request as fact is wrong here exactly as it
        // would be on Wayland.
        state.requested_mode = mode;
        let (size, scale_factor) = match &target {
            Some(monitor) => (monitor.size(), monitor.scale_factor),
            // Leaving borderless restores nothing in particular — a real
            // compositor restores the pre-fullscreen geometry, and a test that
            // cares injects the resize it expects.
            None => (state.base_size(), state.scale_factor()),
        };
        state.pending = Some(PendingConfigure {
            size,
            scale_factor,
            mode,
            countdown,
        });
        Ok(())
    }

    fn set_constraints(
        &mut self,
        window: WindowId,
        constraints: SizeConstraints,
    ) -> Result<(), ShellError> {
        let countdown = self.configure_delay;
        let state = self.window_mut(window)?;
        state.requested_constraints = constraints;

        // A configure already on its way is the authoritative statement of what
        // is coming, whether or not the window has been configured before, so
        // the hints fold into it rather than scheduling a second one. Rebuilding
        // it from the *effective* configuration instead would silently discard
        // whatever `set_mode` had put there — leaving `requested_mode` set,
        // `effective_mode` unchanged and `mode_request_honoured` false forever.
        if let Some(pending) = state.pending.as_mut() {
            pending.size = constraints.apply(pending.size, pending.scale_factor);
            return Ok(());
        }

        // Nothing in flight: the window system already told us a size, and if
        // the new hints disagree with it, a compositor that honours them answers
        // with a fresh configure — later, not now.
        if let Some(config) = state.configuration {
            let constrained = constraints.apply(config.size, config.scale_factor);
            if constrained != config.size {
                state.pending = Some(PendingConfigure {
                    size: constrained,
                    scale_factor: config.scale_factor,
                    mode: config.mode,
                    countdown,
                });
            }
        }
        Ok(())
    }

    fn monitors(&self) -> &[MonitorInfo] {
        &self.monitors
    }

    fn pump(&mut self, sink: &mut dyn FnMut(ShellEvent)) {
        self.deliver_due_configures();
        self.resolve_reads();
        // Drain by count rather than `while let`: a sink that injects further
        // events (a UI that opens a window on a click) must not be able to spin
        // this loop forever, and the events it queued belong to the next frame
        // anyway — which is exactly what a real backend's socket read does.
        for _ in 0..self.queue.len() {
            let Some(event) = self.queue.pop_front() else {
                break;
            };
            sink(event);
        }
    }

    /// Sleeps as far as a shell with no window system behind it can.
    ///
    /// This shell reports [`ShellCaps::EVENT_WAIT`] by default, and that bit
    /// says `wait_events` *genuinely blocks* — so it does: with the capability
    /// set and nothing already queued, this really sleeps for `timeout`, and an
    /// editor-style idle loop tested here idles rather than spinning. Without
    /// the capability it returns immediately, like every backend that cannot
    /// block.
    ///
    /// `None` — "until something happens" — returns immediately whatever the
    /// capability says, because the only thing that can make something happen
    /// in a headless shell is the caller, on this thread. Blocking forever
    /// there would be a deadlock by construction rather than a wait.
    fn wait_events(&mut self, timeout: Option<Duration>) {
        self.waits += 1;
        if !self.caps.contains(ShellCaps::EVENT_WAIT) || !self.queue.is_empty() {
            return;
        }
        if let Some(timeout) = timeout {
            std::thread::sleep(timeout);
        }
    }

    /// The seam-level spelling of [`set_time`](Self::set_time).
    ///
    /// Both exist because a test drives this shell through the concrete type
    /// and calls `set_time` every frame, while a consumer written against
    /// `dyn Shell` can only reach the trait — and the trait method is the one
    /// a real backend has to implement, so it must be exercised here too.
    fn align_event_clock(&mut self, elapsed: Duration) {
        self.set_time(elapsed);
    }

    fn surface_target(&self, window: WindowId) -> Result<SurfaceTarget, ShellError> {
        // Available from creation — the *handle* exists as soon as the window
        // does — and constant thereafter. It deliberately carries no size: the
        // extent travels through `WindowState::size` and the swapchain
        // descriptor, so nothing here goes stale on a resize. The only thing
        // this call can fail on is a dead handle.
        self.window(window)?;
        Ok(SurfaceTarget::Offscreen)
    }

    fn set_pointer_mode(&mut self, window: WindowId, mode: PointerMode) -> Result<(), ShellError> {
        if !self.caps.contains(mode.required_cap()) {
            return Err(self.unsupported(mode.as_str()));
        }
        self.window_mut(window)?.pointer_mode = mode;
        Ok(())
    }

    fn set_cursor(
        &mut self,
        window: WindowId,
        cursor: Option<CursorIcon>,
    ) -> Result<(), ShellError> {
        self.window_mut(window)?.cursor = cursor;
        Ok(())
    }

    fn warp_pointer(
        &mut self,
        window: WindowId,
        position: PhysicalPoint,
    ) -> Result<(), ShellError> {
        if !self.caps.contains(ShellCaps::POINTER_WARP) {
            return Err(self.unsupported("pointer warp"));
        }
        self.check_window(window)?;
        // A warp produces motion, as it does on every platform that has one —
        // which is why code that warps in response to motion oscillates.
        let event = ShellEvent::PointerMotion {
            window,
            device: VIRTUAL_DEVICE,
            time: self.now(),
            abs: Some(position),
            raw_delta: None,
        };
        self.queue.push_back(event);
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
            CloseReply::Close => self.destroy_window(window),
            CloseReply::Keep => Ok(()),
        }
    }

    /// Claims the in-process clipboard.
    ///
    /// # Errors
    ///
    /// [`ShellError::Unsupported`] without [`ShellCaps::CLIPBOARD`],
    /// [`ShellError::InvalidWindow`] for a stale handle, or
    /// [`ShellError::NeedsUserInteraction`] when
    /// [`require_user_interaction`](Self::require_user_interaction) is on and
    /// no input has been injected since — the modelled form of Wayland's serial
    /// requirement and the browser's user-gesture gate.
    fn clipboard_offer(
        &mut self,
        window: WindowId,
        offers: &[ClipboardOffer<'_>],
    ) -> Result<(), ShellError> {
        if !self.caps.contains(ShellCaps::CLIPBOARD) {
            return Err(self.unsupported("clipboard"));
        }
        self.check_window(window)?;
        if self.clipboard_needs_interaction && !self.saw_interaction {
            return Err(ShellError::NeedsUserInteraction {
                backend: ShellBackend::Headless,
                what: "claiming the clipboard",
            });
        }
        self.clipboard = offers
            .iter()
            .map(|offer| (ReceivedMime::from(offer.mime), offer.bytes.to_vec()))
            .collect();
        Ok(())
    }

    /// Accepts a read, answering it now if the clipboard is readable and
    /// **holding** it if it is not.
    ///
    /// The held path is the seam's obligation 5, modelled: with
    /// [`set_clipboard_readable(false)`](Self::set_clipboard_readable) the
    /// answer waits, exactly as it does on Wayland while another client has
    /// focus, and a consumer that assumed an immediate empty answer sees the
    /// difference here rather than on someone's desktop.
    ///
    /// # Errors
    ///
    /// [`ShellError::Unsupported`] without [`ShellCaps::CLIPBOARD`], or
    /// [`ShellError::InvalidWindow`] for a stale handle.
    fn clipboard_request(
        &mut self,
        window: WindowId,
        mime: MimeType,
    ) -> Result<ClipboardRequestId, ShellError> {
        if !self.caps.contains(ShellCaps::CLIPBOARD) {
            return Err(self.unsupported("clipboard"));
        }
        self.check_window(window)?;
        let request = ClipboardRequestId(self.next_request_id);
        self.next_request_id += 1;
        if self.answerable() {
            let answer = self.answer(window, request, mime);
            // Queued rather than returned: the whole point of the asynchronous
            // shape is that a consumer written against it also works on X11,
            // where the answer is several round trips away.
            self.queue.push_back(answer);
        } else {
            self.reads.push(PendingRead {
                window,
                request,
                mime,
                countdown: self.clipboard_deadline,
            });
        }
        Ok(request)
    }

    /// Whether a read would be answered without waiting.
    ///
    /// The default would be "yes, always"; this one can be told otherwise, so
    /// that a consumer can be tested against a backend that has a focus gate
    /// without there being a compositor in the room.
    fn clipboard_readable(&self, window: WindowId) -> bool {
        self.caps.contains(ShellCaps::CLIPBOARD) && self.window(window).is_ok() && self.answerable()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{AspectRatio, LogicalSize};

    /// A shell plus an *unconfigured* window — the state every window starts in.
    fn shell_with_window() -> (HeadlessShell, WindowId) {
        let mut shell = HeadlessShell::new();
        let window = shell
            .create_window(&WindowDesc::default())
            .expect("headless window creation never fails");
        (shell, window)
    }

    /// A shell plus a window that has been through the configure handshake.
    fn shell_with_configured_window() -> (HeadlessShell, WindowId) {
        let (mut shell, window) = shell_with_window();
        settle(&mut shell);
        assert!(shell.window_state(window).expect("state").is_configured());
        (shell, window)
    }

    /// Pumps until nothing new arrives, discarding events.
    fn settle(shell: &mut HeadlessShell) {
        for _ in 0..8 {
            let mut seen = 0;
            shell.pump(&mut |_| seen += 1);
            if seen == 0 && shell.windows.iter().all(|(_, w)| w.pending.is_none()) {
                return;
            }
        }
        panic!("the shell never settled");
    }

    fn drain(shell: &mut HeadlessShell) -> Vec<ShellEvent> {
        let mut events = Vec::new();
        shell.pump(&mut |event| events.push(event));
        events
    }

    fn names(events: &[ShellEvent]) -> Vec<&'static str> {
        events.iter().map(ShellEvent::name).collect()
    }

    /// The shape a P1 swapchain-creating consumer has: it needs an extent, and
    /// there is no extent before the first configure.
    fn create_swapchain(state: &WindowState) -> Result<PhysicalSize, &'static str> {
        let size = state.size().ok_or("window is not configured yet")?;
        if size.is_empty() {
            return Err("zero-extent swapchain");
        }
        Ok(size)
    }

    // ---------------------------------------------------------------- configure

    #[test]
    fn a_new_window_has_no_size_until_a_later_pump() {
        let (mut shell, window) = shell_with_window();

        // Nothing has been reported. Not "0x0", not "the requested size" —
        // nothing.
        let state = shell.window_state(window).expect("state");
        assert!(!state.is_configured());
        assert_eq!(state.size(), None);
        assert_eq!(state.scale_factor(), None);
        assert_eq!(state.effective_mode(), None);
        assert_eq!(state.requested_mode, DisplayMode::Windowed);

        // The default delay is one pump: the first delivers nothing.
        assert_eq!(drain(&mut shell), Vec::new());
        assert!(!shell.window_state(window).expect("state").is_configured());

        // The second delivers the configure.
        let events = drain(&mut shell);
        assert_eq!(
            events,
            vec![ShellEvent::Resized {
                window,
                size: PhysicalSize::new(1280, 720),
                scale_factor: 1.0,
            }]
        );
        let state = shell.window_state(window).expect("state");
        assert_eq!(state.size(), Some(PhysicalSize::new(1280, 720)));
        assert_eq!(state.scale_factor(), Some(1.0));
        assert_eq!(state.effective_mode(), Some(DisplayMode::Windowed));
        assert!(state.mode_request_honoured());
    }

    #[test]
    fn a_consumer_that_skips_the_wait_gets_a_clean_error_not_a_wrong_extent() {
        let (mut shell, window) = shell_with_window();

        // The failure mode this whole design exists to force.
        assert_eq!(
            create_swapchain(&shell.window_state(window).expect("state")),
            Err("window is not configured yet")
        );

        // And the surface handle *is* available meanwhile — only the extent is
        // pending. The target carries no size at all, which is the stronger
        // version of the same guarantee: there is no plausible-looking number
        // in it for a consumer to mistake for the size the window is about to
        // have.
        let target = shell.surface_target(window).expect("the handle exists");
        assert_eq!(target, SurfaceTarget::Offscreen);

        // The loop a consumer must write.
        let size = loop {
            shell.pump(&mut |_| {});
            if let Ok(size) = create_swapchain(&shell.window_state(window).expect("state")) {
                break size;
            }
        };
        assert_eq!(size, PhysicalSize::new(1280, 720));
        // Configuring the window did not change the target: a HAL surface made
        // from the pre-configure value is still describing the right object.
        assert_eq!(
            shell.surface_target(window).expect("target"),
            target,
            "the target is invariant under a configure"
        );
    }

    #[test]
    fn the_configure_delay_is_a_knob() {
        let mut shell = HeadlessShell::new().with_configure_delay(3);
        let window = shell.create_window(&WindowDesc::default()).expect("window");
        for pump in 0..3 {
            assert_eq!(drain(&mut shell), Vec::new(), "pump {pump}");
            assert!(!shell.window_state(window).expect("state").is_configured());
        }
        assert_eq!(names(&drain(&mut shell)), ["Resized"]);
        assert!(shell.window_state(window).expect("state").is_configured());

        // Zero means "configured on the first pump", for tests about something
        // else.
        let mut shell = HeadlessShell::new().with_configure_delay(0);
        let window = shell.create_window(&WindowDesc::default()).expect("window");
        assert_eq!(names(&drain(&mut shell)), ["Resized"]);
        assert!(shell.window_state(window).expect("state").is_configured());

        shell.set_configure_delay(2);
        shell
            .set_mode(window, DisplayMode::Borderless { monitor: None })
            .expect("request");
        assert_eq!(drain(&mut shell), Vec::new());
        assert_eq!(drain(&mut shell), Vec::new());
        assert_eq!(names(&drain(&mut shell)), ["Resized"]);
    }

    #[test]
    fn set_mode_is_a_request_whose_effect_arrives_later() {
        let (mut shell, window) = shell_with_configured_window();
        shell
            .set_mode(window, DisplayMode::Borderless { monitor: None })
            .expect("request");

        // The request is a fact; the effect is not.
        let state = shell.window_state(window).expect("state");
        assert_eq!(
            state.requested_mode,
            DisplayMode::Borderless { monitor: None }
        );
        assert_eq!(
            state.effective_mode(),
            Some(DisplayMode::Windowed),
            "still windowed until the window system says otherwise"
        );
        assert!(
            !state.mode_request_honoured(),
            "a UI toggle reading the request instead of the effect would lie here"
        );
        assert_eq!(state.size(), Some(PhysicalSize::new(1280, 720)));

        settle(&mut shell);
        let state = shell.window_state(window).expect("state");
        assert_eq!(
            state.effective_mode(),
            Some(DisplayMode::Borderless { monitor: None })
        );
        assert!(state.mode_request_honoured());
        assert_eq!(state.size(), Some(PhysicalSize::new(1920, 1080)));
    }

    #[test]
    fn a_refused_mode_is_expressible_through_injection() {
        // What a compositor that says no looks like: the request stands, and
        // the configure that arrives says something else.
        let (mut shell, window) = shell_with_configured_window();
        shell
            .set_mode(window, DisplayMode::Borderless { monitor: None })
            .expect("request");
        // The compositor answers with a windowed configure instead.
        shell
            .resize(window, PhysicalSize::new(1280, 720))
            .expect("answer");
        let state = shell.window_state(window).expect("state");
        assert_eq!(state.effective_mode(), Some(DisplayMode::Windowed));
        assert!(!state.mode_request_honoured());
    }

    #[test]
    fn set_constraints_is_a_request_too() {
        let (mut shell, window) = shell_with_configured_window();
        let constraints = SizeConstraints::min(LogicalSize::new(1600.0, 900.0));
        shell.set_constraints(window, constraints).expect("request");

        let state = shell.window_state(window).expect("state");
        assert_eq!(state.requested_constraints, constraints);
        assert_eq!(
            state.size(),
            Some(PhysicalSize::new(1280, 720)),
            "the size only changes when the window system says so"
        );

        settle(&mut shell);
        assert_eq!(
            shell.window_state(window).expect("state").size(),
            Some(PhysicalSize::new(1600, 900))
        );
    }

    #[test]
    fn constraints_do_not_cancel_a_mode_request_that_has_not_landed() {
        // Both are requests, and they are independent. Rebuilding the pending
        // configure from the *effective* configuration threw the mode away:
        // `requested_mode` stayed `Borderless`, `effective_mode` stayed
        // `Windowed`, and `mode_request_honoured` was false forever.
        let (mut shell, window) = shell_with_configured_window();
        shell
            .set_mode(window, DisplayMode::Borderless { monitor: None })
            .expect("mode request");
        shell
            .set_constraints(window, SizeConstraints::min(LogicalSize::new(320.0, 180.0)))
            .expect("constraint request");
        settle(&mut shell);

        let state = shell.window_state(window).expect("state");
        assert_eq!(
            state.effective_mode(),
            Some(DisplayMode::Borderless { monitor: None })
        );
        assert!(state.mode_request_honoured());
    }

    #[test]
    fn a_borderless_window_reports_its_own_monitors_scale() {
        // The scale used to be hoisted from `monitors[0]` above the match, so a
        // borderless window on a HiDPI secondary monitor took that monitor's
        // *size* and the primary's *scale* — a swapchain sized for one and a UI
        // laid out for the other.
        let mut shell = HeadlessShell::new();
        let hidpi = shell.plug_monitor(MonitorInfo {
            id: MonitorId(0),
            name: "HEADLESS-2".to_string(),
            bounds: PhysicalRect::new(1920, 0, 2560, 1440),
            work_area: PhysicalRect::new(1920, 0, 2560, 1440),
            scale_factor: 2.0,
            refresh_millihertz: 143_998,
            is_primary: false,
        });
        let window = shell
            .create_window(&WindowDesc {
                mode: DisplayMode::Borderless {
                    monitor: Some(hidpi),
                },
                ..WindowDesc::default()
            })
            .expect("window");
        settle(&mut shell);

        let state = shell.window_state(window).expect("state");
        assert_eq!(state.size(), Some(PhysicalSize::new(2560, 1440)));
        assert_eq!(
            state.scale_factor(),
            Some(2.0),
            "the scale is the borderless monitor's, not the primary's"
        );
        // And `set_mode` onto the same monitor agrees, which is the pair that
        // used to disagree.
        let other = shell
            .create_window(&WindowDesc::default())
            .expect("second window");
        settle(&mut shell);
        shell
            .set_mode(
                other,
                DisplayMode::Borderless {
                    monitor: Some(hidpi),
                },
            )
            .expect("request");
        settle(&mut shell);
        assert_eq!(
            shell.window_state(other).expect("state").scale_factor(),
            shell.window_state(window).expect("state").scale_factor()
        );
    }

    #[test]
    fn constraints_set_before_the_first_configure_fold_into_it() {
        let mut shell = HeadlessShell::new();
        let window = shell.create_window(&WindowDesc::default()).expect("window");
        shell
            .set_constraints(
                window,
                SizeConstraints::NONE.with_aspect(AspectRatio::SQUARE),
            )
            .expect("request");
        settle(&mut shell);
        // One configure, already square — not a 1280x720 configure followed by
        // a correction.
        assert_eq!(
            shell.window_state(window).expect("state").size(),
            Some(PhysicalSize::new(720, 720))
        );
    }

    #[test]
    fn creation_uses_the_requested_size_and_the_monitors_scale() {
        let mut shell = HeadlessShell::new();
        let window = shell
            .create_window(&WindowDesc {
                size: LogicalSize::new(800.0, 600.0),
                ..WindowDesc::default()
            })
            .expect("window");
        settle(&mut shell);
        assert_eq!(
            shell.window_state(window).expect("state").size(),
            Some(PhysicalSize::new(800, 600))
        );

        let mut shell = HeadlessShell::new().with_scale_factor(2.0);
        let window = shell
            .create_window(&WindowDesc {
                size: LogicalSize::new(640.0, 360.0),
                ..WindowDesc::default()
            })
            .expect("window");
        settle(&mut shell);
        let state = shell.window_state(window).expect("state");
        assert_eq!(state.size(), Some(PhysicalSize::new(1280, 720)));
        assert_eq!(state.scale_factor(), Some(2.0));
    }

    // ------------------------------------------------------------ compositor side

    #[test]
    fn scale_changes_preserve_logical_size_and_report_both_facts() {
        let (mut shell, window) = shell_with_configured_window();
        shell
            .change_scale_factor(window, 1.5)
            .expect("scale change");
        let events = drain(&mut shell);
        assert_eq!(
            events,
            vec![
                ShellEvent::ScaleFactorChanged {
                    window,
                    scale_factor: 1.5,
                    size: PhysicalSize::new(1920, 1080),
                },
                // The resize the scale change implies, in the same order both
                // Linux backends report it.
                ShellEvent::Resized {
                    window,
                    size: PhysicalSize::new(1920, 1080),
                    scale_factor: 1.5,
                },
            ]
        );
        // 1280x720 logical at 1.5 is 1920x1080 physical — the swapchain must be
        // recreated at the new size, which is the leak the DPI matrix test in
        // `docs/plan/15-windowing.md` is looking for.
        assert_eq!(
            shell.window_state(window).expect("state").size(),
            Some(PhysicalSize::new(1920, 1080))
        );
    }

    #[test]
    fn a_scale_change_scheduled_by_a_mode_switch_arrives_before_the_resize() {
        let mut shell = HeadlessShell::new();
        let hidpi = shell.plug_monitor(MonitorInfo {
            id: MonitorId(0),
            name: "HEADLESS-2".to_string(),
            bounds: PhysicalRect::new(1920, 0, 2560, 1440),
            work_area: PhysicalRect::new(1920, 0, 2560, 1440),
            scale_factor: 2.0,
            refresh_millihertz: 143_998,
            is_primary: false,
        });
        assert_eq!(hidpi, MonitorId(2), "ids are never reused");
        let window = shell.create_window(&WindowDesc::default()).expect("window");
        settle(&mut shell);

        shell
            .set_mode(
                window,
                DisplayMode::Borderless {
                    monitor: Some(hidpi),
                },
            )
            .expect("request");
        assert_eq!(drain(&mut shell), Vec::new(), "still only a request");
        let events = drain(&mut shell);
        assert_eq!(names(&events), ["ScaleFactorChanged", "Resized"]);

        let state = shell.window_state(window).expect("state");
        assert_eq!(state.size(), Some(PhysicalSize::new(2560, 1440)));
        assert_eq!(state.scale_factor(), Some(2.0));
        assert!(state.effective_mode().expect("configured").is_borderless());
        // A mode change is the one event the seam says can invalidate a target.
        // Here it does not, because this backend's target names nothing that a
        // mode switch recreates — and, crucially, it does not encode the new
        // size either.
        assert_eq!(
            shell.surface_target(window).expect("target"),
            SurfaceTarget::Offscreen
        );

        assert!(matches!(
            shell.set_mode(
                window,
                DisplayMode::Borderless {
                    monitor: Some(MonitorId(99)),
                },
            ),
            Err(ShellError::NoSuchMonitor(99))
        ));
    }

    #[test]
    fn resizes_honour_constraints_and_inject_bypasses_them() {
        let (mut shell, window) = shell_with_configured_window();
        shell
            .set_constraints(
                window,
                SizeConstraints::min(LogicalSize::new(640.0, 360.0))
                    .with_aspect(AspectRatio::WIDESCREEN),
            )
            .expect("request");
        settle(&mut shell);

        shell
            .resize(window, PhysicalSize::new(1000, 1000))
            .expect("resize");
        assert_eq!(
            drain(&mut shell),
            vec![ShellEvent::Resized {
                window,
                size: PhysicalSize::new(1000, 562),
                scale_factor: 1.0,
            }]
        );

        // A tiling window manager ignores every hint, and `inject` is how that
        // gets tested.
        shell.inject(ShellEvent::Resized {
            window,
            size: PhysicalSize::new(300, 900),
            scale_factor: 1.0,
        });
        assert_eq!(
            drain(&mut shell)[0],
            ShellEvent::Resized {
                window,
                size: PhysicalSize::new(300, 900),
                scale_factor: 1.0,
            }
        );
    }

    #[test]
    fn a_compositor_configure_supersedes_a_pending_one() {
        let (mut shell, window) = shell_with_configured_window();
        shell
            .set_mode(window, DisplayMode::Borderless { monitor: None })
            .expect("request");
        // The compositor answers before the scheduled configure lands.
        shell
            .resize(window, PhysicalSize::new(640, 480))
            .expect("answer");
        assert_eq!(names(&drain(&mut shell)), ["Resized"]);
        assert_eq!(
            shell.window_state(window).expect("state").size(),
            Some(PhysicalSize::new(640, 480))
        );
        // And nothing else arrives later.
        assert_eq!(drain(&mut shell), Vec::new());
    }

    // ------------------------------------------------------------------- input

    #[test]
    fn timestamps_come_from_the_shells_clock_not_from_the_pump() {
        let (mut shell, window) = shell_with_configured_window();
        shell.set_time(Duration::from_secs(2));
        shell.key_press(window, KeyCode::Space).expect("press");
        shell.advance(Duration::from_millis(180));
        shell.key_release(window, KeyCode::Space).expect("release");

        let events = drain(&mut shell);
        let held = events[1]
            .time()
            .expect("input")
            .saturating_since(events[0].time().expect("input"));
        assert_eq!(held, Duration::from_millis(180));
        assert_eq!(events[0].time(), Some(EventTime::from_millis(2_000)));
    }

    #[test]
    fn synthesized_key_fields_are_stable_and_plausible() {
        let (mut shell, window) = shell_with_configured_window();
        shell.set_modifiers(Modifiers::CTRL);
        shell.key_press(window, KeyCode::KeyS).expect("press");
        let events = drain(&mut shell);
        let ShellEvent::Key {
            scancode,
            key_code,
            keysym,
            modifiers,
            repeat,
            device,
            ..
        } = events[0].clone()
        else {
            panic!("wrong variant");
        };
        assert_eq!(key_code, Some(KeyCode::KeyS));
        assert_eq!(keysym, Keysym::from_char('s'));
        assert_eq!(modifiers, Modifiers::CTRL);
        assert!(!repeat);
        assert_eq!(device, VIRTUAL_DEVICE);
        assert!(scancode.0 >= 8, "evdev-shaped scancodes start at 8");
        assert_eq!(synthetic_keysym(KeyCode::Digit7), Keysym::from_char('7'));
        assert_eq!(synthetic_keysym(KeyCode::F5), Keysym::NONE);
        assert_eq!(synthetic_keysym(KeyCode::Slash), Keysym::from_char('/'));

        shell.key_repeat(window, KeyCode::KeyS).expect("repeat");
        let events = drain(&mut shell);
        let ShellEvent::Key { repeat, .. } = events[0] else {
            panic!("wrong variant");
        };
        assert!(repeat);
    }

    #[test]
    fn a_locked_pointer_reports_no_absolute_position() {
        let (mut shell, window) = shell_with_configured_window();
        shell
            .set_pointer_mode(window, PointerMode::Locked)
            .expect("lock");
        shell
            .move_pointer(window, PhysicalPoint::new(10.0, 10.0), (3.0, -4.0))
            .expect("motion");
        let events = drain(&mut shell);
        let ShellEvent::PointerMotion { abs, raw_delta, .. } = events[0] else {
            panic!("wrong variant");
        };
        assert_eq!(abs, None);
        assert_eq!(raw_delta, Some((3.0, -4.0)));
    }

    #[test]
    fn a_backend_without_raw_motion_reports_none() {
        let mut shell = HeadlessShell::new()
            .with_caps(ShellCaps::DESKTOP - ShellCaps::RAW_POINTER_MOTION)
            .with_configure_delay(0);
        let window = shell.create_window(&WindowDesc::default()).expect("window");
        drain(&mut shell);
        shell
            .move_pointer(window, PhysicalPoint::new(1.0, 2.0), (5.0, 5.0))
            .expect("motion");
        let events = drain(&mut shell);
        let ShellEvent::PointerMotion { abs, raw_delta, .. } = events[0] else {
            panic!("wrong variant");
        };
        assert_eq!(abs, Some(PhysicalPoint::new(1.0, 2.0)));
        assert_eq!(raw_delta, None, "no relative-motion source");
        assert!(!shell.caps().has_mouselook());
    }

    #[test]
    fn pointer_focus_and_scroll_and_text_are_injectable() {
        let (mut shell, window) = shell_with_configured_window();
        shell
            .set_pointer_focus(window, true, Some(PhysicalPoint::new(1.0, 1.0)))
            .expect("enter");
        shell
            .scroll(window, ScrollDelta::Lines { x: 0.0, y: -1.0 }, None)
            .expect("scroll");
        shell
            .button(
                window,
                PointerButton::Left,
                ButtonState::Pressed,
                Some(PhysicalPoint::new(1.0, 1.0)),
            )
            .expect("click");
        shell.commit_text(window, "ありがとう").expect("text");
        shell
            .set_pointer_focus(window, false, Some(PhysicalPoint::new(1.0, 1.0)))
            .expect("leave");

        let events = drain(&mut shell);
        assert_eq!(
            names(&events),
            [
                "PointerFocus",
                "Wheel",
                "Button",
                "TextCommit",
                "PointerFocus"
            ]
        );
        let ShellEvent::PointerFocus {
            entered, position, ..
        } = &events[4]
        else {
            panic!("wrong variant");
        };
        assert!(!entered);
        assert_eq!(*position, None, "a leave has no position");
        assert!(events.iter().all(ShellEvent::is_input));
    }

    #[test]
    fn warping_produces_motion_like_every_platform_that_has_it() {
        let (mut shell, window) = shell_with_configured_window();
        shell
            .warp_pointer(window, PhysicalPoint::new(320.0, 180.0))
            .expect("warp");
        let events = drain(&mut shell);
        let ShellEvent::PointerMotion { abs, raw_delta, .. } = events[0] else {
            panic!("wrong variant");
        };
        assert_eq!(abs, Some(PhysicalPoint::new(320.0, 180.0)));
        assert_eq!(raw_delta, None, "a warp is not physical motion");
    }

    // ------------------------------------------------------------- lifecycle

    #[test]
    fn capabilities_gate_the_operations_that_need_them() {
        let mut shell = HeadlessShell::new().with_caps(ShellCaps::empty());
        let window = shell.create_window(&WindowDesc::default()).expect("window");

        assert!(matches!(
            shell.warp_pointer(window, PhysicalPoint::ORIGIN),
            Err(ShellError::Unsupported { .. })
        ));
        assert!(matches!(
            shell.set_pointer_mode(window, PointerMode::Locked),
            Err(ShellError::Unsupported { .. })
        ));
        assert!(matches!(
            shell.clipboard_request(window, MimeType::TextUtf8),
            Err(ShellError::Unsupported { .. })
        ));
        // Free needs no capability, so it always works.
        shell
            .set_pointer_mode(window, PointerMode::Free)
            .expect("Free is always available");
        // And a second window is refused without MULTI_WINDOW.
        assert!(matches!(
            shell.create_window(&WindowDesc::default()),
            Err(ShellError::Unsupported { .. })
        ));
    }

    #[test]
    fn stale_handles_fail_cleanly_on_every_accessor() {
        let (mut shell, window) = shell_with_configured_window();
        shell.destroy_window(window).expect("destroy");
        assert_eq!(
            drain(&mut shell),
            vec![ShellEvent::WindowDestroyed { window }]
        );

        assert!(matches!(
            shell.window_state(window),
            Err(ShellError::InvalidWindow { .. })
        ));
        assert!(matches!(
            shell.surface_target(window),
            Err(ShellError::InvalidWindow { .. })
        ));
        assert!(matches!(
            shell.resize(window, PhysicalSize::new(1, 1)),
            Err(ShellError::InvalidWindow { .. })
        ));
        assert!(matches!(
            shell.set_title(window, "gone"),
            Err(ShellError::InvalidWindow { .. })
        ));
        assert!(matches!(
            shell.destroy_window(window),
            Err(ShellError::InvalidWindow { .. })
        ));
        assert!(matches!(
            shell.key_press(window, KeyCode::KeyA),
            Err(ShellError::InvalidWindow { .. })
        ));
    }

    #[test]
    fn destroying_a_window_cancels_its_pending_configure() {
        // The bug a naive delayed-configure implementation has: an event for a
        // window that no longer exists.
        let (mut shell, window) = shell_with_window();
        shell.destroy_window(window).expect("destroy");
        assert_eq!(names(&drain(&mut shell)), ["WindowDestroyed"]);
        assert_eq!(drain(&mut shell), Vec::new());
        assert_eq!(drain(&mut shell), Vec::new());
    }

    #[test]
    fn close_requests_are_questions() {
        let (mut shell, window) = shell_with_configured_window();
        assert!(matches!(
            shell.reply_close_request(window, CloseReply::Close),
            Err(ShellError::NoPendingCloseRequest { .. })
        ));

        shell.request_close(window).expect("request");
        assert!(shell.window_state(window).expect("state").close_pending);
        shell
            .reply_close_request(window, CloseReply::Keep)
            .expect("keep");
        assert!(!shell.window_state(window).expect("state").close_pending);
        assert!(shell.window_state(window).is_ok(), "still open");

        shell.request_close(window).expect("request again");
        shell
            .reply_close_request(window, CloseReply::Close)
            .expect("close");
        assert!(matches!(
            shell.window_state(window),
            Err(ShellError::InvalidWindow { .. })
        ));
    }

    #[test]
    fn monitor_hotplug_reports_a_change_and_keeps_ids_unique() {
        let mut shell = HeadlessShell::new();
        let extra = shell.plug_monitor(MonitorInfo {
            id: MonitorId(0),
            name: "HEADLESS-2".to_string(),
            bounds: PhysicalRect::new(1920, 0, 1920, 1080),
            work_area: PhysicalRect::new(1920, 0, 1920, 1080),
            scale_factor: 1.0,
            refresh_millihertz: 60_000,
            is_primary: false,
        });
        assert_eq!(shell.monitors().len(), 2);
        assert_eq!(drain(&mut shell), vec![ShellEvent::MonitorsChanged]);

        shell.unplug_monitor(extra).expect("unplug");
        assert_eq!(shell.monitors().len(), 1);
        assert_eq!(drain(&mut shell), vec![ShellEvent::MonitorsChanged]);
        assert!(matches!(
            shell.unplug_monitor(extra),
            Err(ShellError::NoSuchMonitor(_))
        ));
        assert_eq!(
            shell.monitor(MonitorId(1)).map(|m| m.name.as_str()),
            Some("HEADLESS-1")
        );
        assert!(shell.monitor(extra).is_none());
    }

    #[test]
    fn drops_require_both_the_capability_and_the_opt_in() {
        let mut shell = HeadlessShell::new();
        let plain = shell.create_window(&WindowDesc::default()).expect("window");
        assert!(matches!(
            shell.drop_file(plain, "/tmp/a.gltf", None),
            Err(ShellError::Unsupported { .. })
        ));

        let dropper = shell
            .create_window(&WindowDesc {
                accept_drops: true,
                ..WindowDesc::default()
            })
            .expect("window");
        settle(&mut shell);
        shell
            .drop_file(dropper, "/tmp/a.gltf", Some(PhysicalPoint::new(4.0, 5.0)))
            .expect("drop");
        assert_eq!(names(&drain(&mut shell))[0], "DroppedFile");
    }

    #[test]
    fn a_sink_that_queues_more_events_does_not_spin_the_pump() {
        let (mut shell, window) = shell_with_configured_window();
        shell.inject(ShellEvent::MonitorsChanged);
        let mut extra = Vec::new();
        shell.pump(&mut |event| extra.push(event));
        assert_eq!(extra.len(), 1);
        shell.inject(ShellEvent::CloseRequested { window });
        assert_eq!(shell.pending_events(), 1);
    }

    #[test]
    fn accessors_that_no_real_backend_can_provide_are_headless_only() {
        let (mut shell, window) = shell_with_configured_window();
        shell.set_title(window, "renamed").expect("title");
        assert_eq!(shell.title(window).expect("title"), "renamed");
        shell.set_cursor(window, None).expect("hide");
        assert_eq!(shell.cursor(window).expect("cursor"), None);
        shell
            .set_cursor(window, Some(CursorIcon::Crosshair))
            .expect("shape");
        assert_eq!(
            shell.cursor(window).expect("cursor"),
            Some(CursorIcon::Crosshair)
        );
        shell.set_visible(window, false).expect("hide window");
        assert!(!shell.window_state(window).expect("state").visible);
    }

    #[test]
    fn waiting_is_observable_and_focus_transitions_are_events() {
        let (mut shell, window) = shell_with_configured_window();
        assert_eq!(shell.wait_count(), 0);
        shell.wait_events(Some(Duration::from_millis(5)));
        shell.wait_events(None);
        assert_eq!(shell.wait_count(), 2);

        shell.set_focus(window, true).expect("focus");
        assert!(shell.window_state(window).expect("state").focused);
        shell.set_focus(window, false).expect("unfocus");
        assert_eq!(names(&drain(&mut shell)), ["Focus", "Focus"]);
    }

    // ------------------------------------------------------------- clipboard

    #[test]
    fn the_clipboard_round_trips_through_an_event() {
        let (mut shell, window) = shell_with_configured_window();
        shell
            .clipboard_offer(
                window,
                &[
                    ClipboardOffer::text("node"),
                    ClipboardOffer::ron("(kind:\"node\")"),
                ],
            )
            .expect("offer");
        assert_eq!(
            shell.clipboard_bytes(MimeType::TextUtf8),
            Some(&b"node"[..])
        );

        let request = shell
            .clipboard_request(window, MimeType::CrcblRon)
            .expect("request");
        assert_eq!(
            drain(&mut shell)[0],
            ShellEvent::ClipboardData {
                window,
                request,
                mime: ReceivedMime::from(MimeType::CrcblRon),
                content: ClipboardContent::Bytes(b"(kind:\"node\")".to_vec()),
            }
        );

        // A format nobody offered comes back `Empty` — an answer, and a
        // different one from "the read failed".
        let request = shell
            .clipboard_request(window, MimeType::UriList)
            .expect("request");
        assert_eq!(
            drain(&mut shell)[0],
            ShellEvent::ClipboardData {
                window,
                request,
                mime: ReceivedMime::from(MimeType::UriList),
                content: ClipboardContent::Empty,
            }
        );

        // A published payload of zero bytes is a *successful* read of nothing,
        // which the old `Option<Vec<u8>>` shape could not say.
        shell
            .clipboard_offer(window, &[ClipboardOffer::text("")])
            .expect("offer");
        let request = shell
            .clipboard_request(window, MimeType::TextUtf8)
            .expect("request");
        assert_eq!(
            drain(&mut shell)[0],
            ShellEvent::ClipboardData {
                window,
                request,
                mime: ReceivedMime::from(MimeType::TextUtf8),
                content: ClipboardContent::Bytes(Vec::new()),
            }
        );
    }

    #[test]
    fn a_read_taken_while_the_clipboard_is_unreadable_is_held_not_answered_empty() {
        // The Wayland focus gate, modelled. A consumer written only against a
        // clipboard that answers instantly cannot tell "empty" from "I have
        // not looked yet" — which is exactly what the seam's obligation 5
        // exists to stop backends doing, and what this shell can now reproduce
        // with no compositor in the room.
        let (mut shell, window) = shell_with_configured_window();
        shell
            .clipboard_offer(window, &[ClipboardOffer::text("published")])
            .expect("offer");
        shell.set_clipboard_readable(false);
        assert!(!shell.clipboard_readable(window));

        let request = shell
            .clipboard_request(window, MimeType::TextUtf8)
            .expect("a read is accepted even when it cannot be answered yet");
        assert_eq!(shell.held_clipboard_reads(), 1);
        assert!(
            drain(&mut shell).is_empty(),
            "a held read produces no event at all — least of all an empty one"
        );

        // Focus arrives. The *same* request answers; the caller asked once.
        shell.set_clipboard_readable(true);
        let events = drain(&mut shell);
        assert_eq!(events.len(), 1);
        assert_eq!(
            events[0],
            ShellEvent::ClipboardData {
                window,
                request,
                mime: ReceivedMime::from(MimeType::TextUtf8),
                content: ClipboardContent::Bytes(b"published".to_vec()),
            }
        );
        assert_eq!(shell.held_clipboard_reads(), 0);
    }

    #[test]
    fn a_read_that_never_becomes_answerable_ends_as_unavailable() {
        // Obligation 4: every accepted request is answered, within a bounded
        // time. A request that stayed outstanding forever is a UI stuck on
        // "pasting…" that no consumer can detect.
        let (mut shell, window) = shell_with_configured_window();
        shell
            .clipboard_offer(window, &[ClipboardOffer::text("unreachable")])
            .expect("offer");
        shell.set_clipboard_readable(false);
        shell.clipboard_deadline(2);

        let request = shell
            .clipboard_request(window, MimeType::TextUtf8)
            .expect("request");
        assert!(drain(&mut shell).is_empty(), "pump 1: still held");
        assert!(drain(&mut shell).is_empty(), "pump 2: still held");
        let events = drain(&mut shell);
        assert_eq!(
            events[0],
            ShellEvent::ClipboardData {
                window,
                request,
                mime: ReceivedMime::from(MimeType::TextUtf8),
                // Not `Empty`: there is something on the clipboard, and this
                // shell simply could not get at it.
                content: ClipboardContent::Unavailable,
            }
        );
        assert_eq!(shell.held_clipboard_reads(), 0);
    }

    #[test]
    fn a_peer_that_holds_the_clipboard_and_never_delivers_is_unavailable() {
        // The other failure with no protocol event behind it: on Wayland the
        // transfer is a pipe another application writes, and an application
        // that takes the descriptor and hangs produces exactly this.
        let (mut shell, window) = shell_with_configured_window();
        shell.set_foreign_clipboard(vec![(
            ReceivedMime::new("text/plain"),
            b"never sent".to_vec(),
        )]);
        shell.set_clipboard_stalled(true);
        shell.clipboard_deadline(1);
        assert!(
            !shell.clipboard_readable(window),
            "a clipboard whose owner will not answer is not readable"
        );

        let request = shell
            .clipboard_request(window, MimeType::TextUtf8)
            .expect("request");
        assert!(drain(&mut shell).is_empty());
        let events = drain(&mut shell);
        let ShellEvent::ClipboardData {
            request: answered,
            content,
            ..
        } = &events[0]
        else {
            panic!("wrong variant");
        };
        assert_eq!(*answered, request);
        assert_eq!(*content, ClipboardContent::Unavailable);

        // The peer wakes up; a fresh read now succeeds.
        shell.set_clipboard_stalled(false);
        let request = shell
            .clipboard_request(window, MimeType::TextUtf8)
            .expect("request");
        assert_eq!(
            drain(&mut shell)[0],
            ShellEvent::ClipboardData {
                window,
                request,
                mime: ReceivedMime::new("text/plain"),
                content: ClipboardContent::Bytes(b"never sent".to_vec()),
            }
        );
    }

    #[test]
    fn claiming_the_clipboard_can_require_a_user_interaction() {
        // Wayland's `set_selection` serial and the browser's user-gesture gate,
        // modelled as one thing — because they are one thing: a background
        // process must not be able to take the clipboard.
        let (mut shell, window) = shell_with_configured_window();
        shell.require_user_interaction(true);

        let refused = shell.clipboard_offer(window, &[ClipboardOffer::text("stolen")]);
        assert!(
            matches!(
                refused,
                Err(ShellError::NeedsUserInteraction {
                    backend: ShellBackend::Headless,
                    ..
                })
            ),
            "{refused:?}"
        );
        assert!(
            shell.clipboard_bytes(MimeType::TextUtf8).is_none(),
            "a refused claim must not have taken the clipboard anyway"
        );

        // The copy the user actually asked for. Any injected input counts, as
        // any real input serial would.
        shell.key_press(window, KeyCode::KeyC).expect("key");
        shell
            .clipboard_offer(window, &[ClipboardOffer::text("copied")])
            .expect("with a serial in hand, the claim goes through");
        assert_eq!(
            shell.clipboard_bytes(MimeType::TextUtf8),
            Some(&b"copied"[..])
        );

        // And the permission goes stale, exactly as a serial does.
        shell.clear_user_interaction();
        assert!(matches!(
            shell.clipboard_offer(window, &[ClipboardOffer::text("later")]),
            Err(ShellError::NeedsUserInteraction { .. })
        ));
    }

    #[test]
    fn a_held_read_for_a_destroyed_window_is_dropped_rather_than_answered() {
        // Obligation 1 outranks obligation 4: an answer naming a window that no
        // longer exists is a stale handle in a consumer's hands.
        let (mut shell, window) = shell_with_configured_window();
        shell.set_clipboard_readable(false);
        let _request = shell
            .clipboard_request(window, MimeType::TextUtf8)
            .expect("request");
        assert_eq!(shell.held_clipboard_reads(), 1);

        shell.destroy_window(window).expect("destroy");
        let events = drain(&mut shell);
        assert_eq!(shell.held_clipboard_reads(), 0);
        assert!(
            !events
                .iter()
                .any(|event| matches!(event, ShellEvent::ClipboardData { .. })),
            "no clipboard answer may name a destroyed window: {events:?}"
        );
    }

    #[test]
    fn a_paste_from_a_foreign_application_keeps_the_foreign_spelling() {
        let (mut shell, window) = shell_with_configured_window();
        shell.set_foreign_clipboard(vec![
            // Exactly what a GTK application offers, and a string no
            // `&'static str` in this crate could have held.
            (ReceivedMime::new("text/plain"), b"from gedit".to_vec()),
            (
                ReceivedMime::new("application/vnd.some-editor.scene+json"),
                b"{}".to_vec(),
            ),
        ]);

        // Asking for our canonical spelling still finds it …
        let request = shell
            .clipboard_request(window, MimeType::TextUtf8)
            .expect("request");
        let events = drain(&mut shell);
        let ShellEvent::ClipboardData {
            request: answered,
            mime,
            content,
            ..
        } = &events[0]
        else {
            panic!("wrong variant");
        };
        assert_eq!(*answered, request);
        assert_eq!(content.bytes(), Some(&b"from gedit"[..]));
        // … and the answer reports what the peer actually called it.
        assert_eq!(mime.as_str(), "text/plain");
        assert!(mime.matches(MimeType::TextUtf8));
        assert_ne!(*mime, ReceivedMime::from(MimeType::TextUtf8));

        // A format the engine has no name for is still reachable and still
        // reported verbatim.
        assert_eq!(
            shell.clipboard_bytes(MimeType::Other("application/vnd.some-editor.scene+json")),
            Some(&b"{}"[..])
        );
    }

    // ------------------------------------------------------------------ panics

    #[test]
    #[should_panic(expected = "a shell needs at least one monitor")]
    fn a_shell_with_no_monitors_is_rejected() {
        let _ = HeadlessShell::new().with_monitors(Vec::new());
    }

    #[test]
    #[should_panic(expected = "a text commit is never empty")]
    fn an_empty_text_commit_is_rejected() {
        let (mut shell, window) = shell_with_configured_window();
        let _ = shell.commit_text(window, "");
    }
}
