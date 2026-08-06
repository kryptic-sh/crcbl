//! The Crucible windowing seam: one trait, engine-typed vocabulary, and a
//! complete headless implementation.
//!
//! Everything above this crate — the engine loop, the renderer, the samples,
//! the editor — talks to the window system exclusively through [`Shell`].
//! Everything below it (the Wayland and X11 backends at P0.5/P0.6, Win32 at
//! P5C, AppKit next to it, the canvas at P5) implements it. **No platform
//! type ever appears in a signature here, and a `#[cfg(target_os = …)]` in a
//! consumer is a regression** — the same discipline `crcbl-hal` applies to the
//! GPU, applied to the window.
//!
//! The one sanctioned leak is [`SurfaceTarget`], produced by
//! [`Shell::surface_target`] and destructured only by HAL backends. It lives in
//! `crcbl-core` because this crate and `crcbl-hal` must not depend on each
//! other; see [`crcbl_core::surface`] for the argument. The dependency
//! direction across the two seams is:
//!
//! ```text
//! crcbl-shell ──▶ crcbl-core ◀── crcbl-hal ◀── crcbl-vk / crcbl-wgpu / …
//! ```
//!
//! # What is in this crate
//!
//! P0.4 added the seam and [`HeadlessShell`]. P0.5a added the **Wayland
//! backend**'s window lifecycle — connection, registry, `xdg-shell`,
//! configure/ack, modes, monitors. P0.5b added everything downstream of
//! `wl_seat`: keyboard and pointer input with XKB keymaps, pointer lock and
//! confinement, raw relative motion, fractional scale, `xdg_output` monitor
//! geometry and `xdg-decoration`. P0.5c added `data-device`: the clipboard in
//! both directions and drag-and-drop *reception*, with the payload transfers
//! run across pipes without ever blocking the frame loop.
//!
//! P0.6 added the **X11 backend**: libxcb through `dlopen`, the window
//! lifecycle with ICCCM and EWMH properties, RandR monitor enumeration, XKB
//! keymaps read from the server, XInput 2 raw motion, pointer grabs, and the
//! selection protocol with `TARGETS` negotiation and `INCR` transfers in both
//! directions. Drag and drop (XDND) is deliberately *not* in it and
//! [`ShellCaps::DRAG_DROP`] says so. Both backends are compiled only on Linux,
//! reached only through [`open`], and built on hand-written `extern "C"`
//! declarations — for libwayland-client, libxcb and libxkbcommon — with the
//! Wayland protocol marshalling generated at build time from vendored XML by
//! `crcbl-wl-scanner`. There is no `wayland-client`, no `x11rb` and no `winit`.
//!
//! P5C added the **Win32 backend**: the window class and its procedure,
//! create/destroy, title, visibility, windowed ↔ borderless on a named monitor,
//! `WM_GETMINMAXINFO` and `WM_SIZING` size constraints, monitor enumeration with
//! per-monitor-v2 DPI, the message pump and a blocking `wait_events` — then
//! keyboard, text, pointer, wheel, raw relative motion, pointer lock and
//! confinement, cursors and warping. The clipboard and drag-and-drop are the
//! slice after it, and [`ShellCaps`] is clear on every bit they would set. It is
//! hand-written `extern "system"` FFI to `user32`, `gdi32`, `shcore` and
//! `kernel32` — **linked rather than `dlopen`ed**, which is the one place the
//! three native backends deliberately disagree: a Linux box can genuinely lack
//! libwayland, and a Windows cannot lack `user32.dll`. There is no
//! `windows-rs`.
//!
//! P5C also added the **AppKit backend**: the Objective-C runtime FFI, the
//! `NSApplication` bootstrap, `NSWindow` and a layer-hosting `NSView` carrying
//! the `CAMetalLayer` that *is* [`SurfaceTarget::AppKit`], the event pump and a
//! blocking `wait_events`, windowed ↔ borderless on a named display, `NSScreen`
//! enumeration with scale and refresh, size constraints and close-request
//! interception — then keyboard, text through a real `NSTextInputClient`,
//! pointer, both scroll units, relative motion, pointer lock, cursors and
//! warping. The pasteboard and drag-and-drop are the slice after it, and
//! [`ShellCaps`] is clear on every bit they would set. It is hand-written
//! `objc_msgSend` FFI — **linked rather than `dlopen`ed**, for the same reason
//! the Win32 backend gives — with no `objc2` and no framework.
//!
//! Four macOS facts are worth knowing at this level rather than at that one.
//! **AppKit is main-thread-only and enforces it by raising an Objective-C
//! exception**, which is the concrete reason [`Shell`] is not `Send` and is a
//! stronger rule than Win32's thread affinity — there, any thread may own a
//! window as long as it is the one pumping. **The Y axis points up**, and macOS
//! is the only one of the five platforms whose coordinate system disagrees with
//! this seam's about that. **`backingScaleFactor` is 1.0 or 2.0 and nothing
//! between**, so [`ShellCaps::FRACTIONAL_SCALE`] is clear on the one desktop
//! backend a reader would expect it to be set on. And **macOS cannot confine a
//! pointer at all** — there is no `ClipCursor` and no `confine_to`, only warping
//! it back after it has already left — so [`ShellCaps::POINTER_CONFINE`] is
//! clear while [`ShellCaps::POINTER_LOCK`] is set, which is a split no other
//! desktop backend has.
//!
//! Two Win32 facts the other backends do not have are worth knowing at this
//! level rather than at that one. A window's contents freeze while the user
//! drags its edge, because Windows runs a **modal message loop** of its own
//! there and [`pump`](Shell::pump) does not return until the mouse is released.
//! And a window belongs to the thread that created it, which is the concrete
//! reason [`Shell`] is not `Send`.
//!
//! Two properties of the Wayland clipboard shaped the seam rather than being
//! worked around in the backend, because they are properties of the *platform*:
//! a client may only read the selection while one of its windows has keyboard
//! focus, and may only *claim* it while holding a serial from real user input.
//! So [`Shell::clipboard_readable`] says whether a read would answer now,
//! [`Shell::clipboard_request`] **holds** a read it cannot yet answer instead of
//! reporting an empty clipboard, [`ClipboardContent`] separates "empty" from
//! "could not be read", and [`ShellError::NeedsUserInteraction`] names the
//! refusal. [`HeadlessShell`] can simulate all three, so a consumer is tested
//! against them with no compositor in the room.
//!
//! P0.6 checked that prediction against the other platform, and it held with
//! one correction worth recording: X11 needs none of the *holding* (its
//! `GetSelectionOwner` answers synchronously and there is no focus gate) and
//! never returns [`ShellError::NeedsUserInteraction`] (there is no such rule),
//! but it does **not** get "answered within a bounded time" for free. A stalled
//! `INCR` transfer has no descriptor to poll and no protocol event for "the
//! owner gave up", so the X11 backend carries its own deadline. The seam did
//! not move; one backend obligation turned out to cost more than predicted, and
//! that is written down where it is paid.
//!
//! `HeadlessShell` is not a stub standing in for the real thing: per
//! `docs/plan/15-windowing.md` it is a first-class implementation that CI,
//! `crcbl screenshot` and `crcbl sim` run the *identical* engine loop through.
//! Where the real compositor turned out to differ from what it models, the
//! difference is documented in the Wayland backend rather than smoothed over —
//! the first configure carrying no size at all is the one to know about.
//!
//! # Decision: `dyn`-primary, like the HAL
//!
//! [`Shell`] is object-safe and [`open`] returns `Box<dyn Shell>`. This is
//! forced rather than chosen: `docs/plan/15-windowing.md` requires Wayland and
//! X11 to be compiled in together and selected *at runtime*, so the backend
//! cannot be a type parameter. The consequences are the ones `crcbl-hal`
//! documents — no generic methods, no `impl Trait` returns, no cross-crate
//! inlining — and they cost less here: a frame makes a handful of shell calls,
//! not one per draw. Generic use still works (`fn f<S: Shell + ?Sized>(s: &S)`)
//! and monomorphises normally.
//!
//! The trait requires [`Debug`](core::fmt::Debug) and deliberately **not**
//! `Send`/`Sync`. Shells are thread-affine and cannot promise otherwise: a
//! Win32 `HWND` belongs to the thread pumping its message queue, and AppKit is
//! main-thread-only. [`SurfaceTarget`] reached the same conclusion for the same
//! reason. Create the shell on the thread that will pump it.
//!
//! # Decision: `pump` is a drain, never a loop
//!
//! ```text
//! loop {                                  // native: the outer loop is yours
//!     shell.pump(&mut |event| …);         // ← drain what arrived
//!     clock.update(time.elapsed());
//!     while clock.consume_tick() { tick(clock.tick_dt()); }
//!     render(clock.alpha());
//! }
//! ```
//!
//! There is no `Shell::run(closure)` and there never will be, because
//! `docs/plan/10-wasm-webgpu.md` requires the engine loop to be "a `fn tick(dt)`
//! driven by an outer loop, not a `loop {}` that owns the thread" — in a
//! browser the outer loop is `requestAnimationFrame`, which *calls* the engine
//! and cannot be called by it. A framework-shaped `run()` would compile on wasm
//! and then deadlock on the first frame, which is the worst possible failure
//! schedule. The rAF build is the same body with the `loop {` line deleted:
//!
//! ```text
//! // wasm: the outer loop is the browser's
//! extern "C" fn crcbl_frame(now_ms: f64) {   // called from rAF by the JS shim
//!     shell.pump(&mut |event| …);
//!     clock.update(Duration::from_secs_f64(now_ms / 1000.0));
//!     while clock.consume_tick() { tick(clock.tick_dt()); }
//!     render(clock.alpha());
//! }
//! ```
//!
//! [`Shell::wait_events`] exists for the editor, which idles at zero frames per
//! second on battery. It is *advisory*: returning immediately is always a
//! correct implementation, which is what makes it portable to a browser that
//! cannot block at all.
//!
//! # Decision: where the input vocabulary lives
//!
//! `docs/plan/01-foundations.md` §1.4 puts input normalization in
//! `crcbl-core::input`, and [`ShellEvent`] is naturally a `crcbl-shell` type.
//! Both are true, and the split is:
//!
//! | Lives in | What | Why |
//! | --- | --- | --- |
//! | [`crcbl_core::input`] | [`KeyCode`], [`Keysym`], [`PointerButton`], [`Modifiers`], [`ScrollDelta`], [`DeviceId`] | the P2 action layer, the profile store, the ECS and the UI all need to *name* a key; none of them should link a windowing library to spell `Space` |
//! | `crcbl-shell` | [`ShellEvent`] | it names [`WindowId`], and only a shell backend ever produces one |
//!
//! Dependency direction: `crcbl-shell → crcbl-core`, never the reverse, and
//! `crcbl-input` (P2) will depend on `crcbl-core` for vocabulary plus
//! `crcbl-shell` for the event stream. The vocabulary types are re-exported
//! here so an event consumer needs one `use`, not two.
//!
//! # Decision: raw events only
//!
//! This crate produces *normalized raw* events. Actions, bindings, contexts and
//! patterns are `docs/plan/19-input.md`'s action layer and land at P2. What
//! this slice owes that layer is the information it cannot reconstruct later —
//! per-device ids, and timestamps that are the window system's rather than the
//! frame's. Both are on every input event; see [`event`].
//!
//! # Decision: a window has no size until the window system says so
//!
//! [`Shell::create_window`] returns a [`WindowId`] and *nothing else*.
//! [`WindowState::size`] is `None` until the first
//! [`ShellEvent::Resized`] arrives, and a swapchain — which needs an extent —
//! cannot be created before then.
//!
//! This is the shape Wayland forces: an `xdg_surface` is unsized and may not be
//! given a buffer until the compositor answers with a `configure`, a round trip
//! away. Modelling it in the type rather than in prose is the difference
//! between every consumer written before P0.5 being correct and all of them
//! breaking on the same day. [`HeadlessShell`] deliberately defers the first
//! configure by at least one [`pump`](Shell::pump) — same reasoning as
//! `crcbl-hal`'s injected readback latency — so the create-then-wait loop is
//! *exercised* in CI rather than merely documented.
//!
//! The same distinction runs through [`WindowState`]: what was **requested**
//! ([`requested_mode`](WindowState::requested_mode),
//! [`requested_constraints`](WindowState::requested_constraints)) is always
//! known because we said it, while what is **effective**
//! ([`WindowConfiguration`]) only exists once the window system has answered
//! and may disagree forever. A compositor can refuse a fullscreen request; a
//! tiling window manager ignores every size hint there is.
//!
//! # Stability
//!
//! Provisional, like the HAL seam, and with a specific reason to be: it has no
//! real backend behind it yet. The places most likely to move are stated rather
//! than glossed — [`HeadlessShell`]'s event ordering is a *model* of a
//! compositor's, not a recording of one, and its configure delay is a fixed
//! number of pumps where a compositor's is a round trip.
//!
//! # Example
//!
//! A complete, deterministic window session with no window system anywhere:
//!
//! ```
//! use crcbl_shell::{CloseReply, HeadlessShell, PhysicalSize, Shell, WindowDesc};
//!
//! let mut shell = HeadlessShell::new();
//! let window = shell.create_window(&WindowDesc {
//!     title: "sandbox",
//!     ..WindowDesc::default()
//! })?;
//!
//! // The window has no size yet, so there is no swapchain to create.
//! assert_eq!(shell.window_state(window)?.size(), None);
//!
//! // Wait for the window system to configure it. This loop is mandatory, not
//! // defensive: on Wayland the answer is a round trip away.
//! let size = loop {
//!     shell.pump(&mut |_event| {});
//!     if let Some(size) = shell.window_state(window)?.size() {
//!         break size;
//!     }
//! };
//! assert_eq!(size, PhysicalSize::new(1280, 720));
//!
//! // Script what a compositor would do next.
//! shell.resize(window, PhysicalSize::new(1920, 1080))?;
//! shell.request_close(window)?;
//!
//! let mut names = Vec::new();
//! shell.pump(&mut |event| names.push(event.name()));
//! assert_eq!(names, ["Resized", "CloseRequested"]);
//!
//! // The close request is a question, not a notification.
//! shell.reply_close_request(window, CloseReply::Keep)?;
//! assert!(shell.window_state(window).is_ok(), "still open");
//! # Ok::<(), crcbl_shell::ShellError>(())
//! ```

pub mod backend;
pub mod caps;
pub mod clipboard;
pub mod cursor;
pub mod error;
pub mod event;
pub mod geom;
pub mod headless;
pub mod monitor;
pub mod window;

/// The `KeyCode` → keysym table that has no platform in it, shared by the
/// backends that need one (see the module docs for why it is a separate module
/// rather than a third copy beside the evdev table).
///
/// Compiled wherever a consumer exists — the AppKit and Win32 backends — and,
/// under `cfg(test)`, on every host so the table's own tests run in `cargo
/// test` on the machine the engine is developed on.
#[cfg(any(target_os = "macos", target_os = "windows", test))]
pub(crate) mod keysym;

/// What the two Linux backends share: the evdev key table and libxkbcommon.
///
/// Not `pub`, like the backends themselves. See the module's own docs for why
/// exactly two things are in it and nothing protocol-shaped ever will be.
#[cfg(target_os = "linux")]
pub(crate) mod linux;

/// The Wayland backend (P0.5).
///
/// Not `pub`: [`backend`] is the only way to reach a real shell, because
/// `WaylandShell::new()` in a consumer's source is the platform leak this whole
/// seam exists to prevent. `#[cfg(target_os = "linux")]` rather than
/// `#[cfg(unix)]` — macOS is a Unix with no Wayland, and the BSDs are not a
/// target this engine claims.
#[cfg(target_os = "linux")]
pub(crate) mod wayland;

/// The X11 backend (P0.6).
///
/// Same terms as [`wayland`]: private, reached only through [`open`], and
/// Linux-only. libxcb is loaded with `dlopen` for the reason the registry
/// exists — a hard link would kill the process in `ld.so` on a machine with no
/// X libraries and the Wayland entry above would never be tried.
#[cfg(target_os = "linux")]
pub(crate) mod x11;

/// The Win32 backend (P5C).
///
/// Same terms as the two Linux backends: private, reached only through
/// [`open`]. The `cfg` is not the same, and deliberately: **the parts of this
/// backend that are pure arithmetic compile on every host under `cfg(test)`**,
/// so the aspect-lock algebra, the resize-storm coalescing and the 32-bit
/// tick-count wrap are exercised by `cargo test` on the Linux machine the
/// engine is developed on rather than only by the Windows CI runner. Everything
/// that calls `user32` is `#[cfg(target_os = "windows")]` inside the module,
/// which is also why there is no `dlopen` here — see `win32::ffi` for that
/// argument, which is the reverse of the one `src/x11/ffi.rs` makes.
#[cfg(any(target_os = "windows", test))]
pub(crate) mod win32;

/// The AppKit backend (P5C).
///
/// Same terms as the other three native backends: private, reached only through
/// [`open`]. The `cfg` is the Win32 one for the Win32 reason — **the parts of
/// this backend that are pure arithmetic compile on every host under
/// `cfg(test)`** — and it earns more here than it did there, because more of
/// this backend *is* arithmetic: macOS is the only platform whose coordinate
/// system disagrees with the seam's about which way the Y axis points, and the
/// flip that reconciles them is exercised by `cargo test` on the Linux machine
/// this engine is developed on. Everything that calls AppKit is
/// `#[cfg(target_os = "macos")]` inside the module, which is also why there is
/// no `dlopen` here — see `appkit::ffi`, which makes the same argument
/// `src/win32/ffi.rs` does and the reverse of `src/x11/ffi.rs`'s.
#[cfg(any(target_os = "macos", test))]
pub(crate) mod appkit;

/// The Web/canvas backend (P5).
///
/// Compiled on `wasm32`, where it is the real backend, and under `cfg(test)`,
/// where its own tests drive the JS→wasm entry points exactly as a browser
/// would. **Not** in a native build: there is no shim to call it, and compiling
/// it everywhere exported the shim's `__crcbl_web_*` symbols from every binary
/// that links the engine. The registry entry stays on every target regardless,
/// so `CRCBL_SHELL=web` off wasm names the problem instead of hanging — see
/// [`backend`].
#[cfg(any(target_arch = "wasm32", test))]
pub(crate) mod web;

/// Scaffolding for the nested-compositor end-to-end suite, behind the
/// `wayland-e2e` feature.
///
/// Not part of the seam and not for consumers: it exists because a Wayland
/// surface is only managed by the compositor once it has a *buffer*, and
/// attaching one is the renderer's job — which does not exist until P1. See
/// the module's docs for the full finding.
#[cfg(all(target_os = "linux", feature = "wayland-e2e"))]
pub use wayland::e2e as wayland_test_support;

/// Scaffolding for the Xvfb end-to-end suite, behind the `x11-e2e` feature.
///
/// Not part of the seam and not for consumers. Where the Wayland version exists
/// because a surface needs a buffer, this one exists because on X11 the window
/// manager, the input devices and the clipboard owner are all *other programs* —
/// see the module's docs.
#[cfg(all(target_os = "linux", feature = "x11-e2e"))]
pub use x11::e2e as x11_test_support;

/// Scaffolding for the AppKit window session, `tests/appkit_session.rs`.
///
/// Not part of the seam and not for consumers. Behind no feature, unlike the
/// two above, because the session target is behind none either: `libtest`
/// always runs a body on a thread it spawns and AppKit is main-thread-only, so
/// that target owns its `main` and there is no version of it that a feature
/// flag could turn off.
///
/// What it holds is the checks that need a live `NSApplication` **on that
/// thread** — a distinction P5C has now paid for twice, and which the module's
/// own docs state as a rule.
#[cfg(target_os = "macos")]
pub use appkit::session_support;

pub use backend::{BACKEND_ENV_VAR, ShellBackend, open, open_backend};
pub use caps::ShellCaps;
pub use clipboard::{
    ClipboardContent, ClipboardOffer, ClipboardRequestId, MimeType, ReceivedMime, parse_uri_list,
};
pub use cursor::{CursorIcon, PointerMode};
pub use error::ShellError;
pub use event::ShellEvent;
pub use geom::{AspectRatio, LogicalPoint, LogicalSize, PhysicalPoint, PhysicalRect, PhysicalSize};
pub use headless::HeadlessShell;
pub use monitor::{MonitorId, MonitorInfo};
pub use window::{
    CloseReply, DisplayMode, SizeConstraints, Window, WindowConfiguration, WindowDesc, WindowId,
    WindowState,
};

/// Input vocabulary, defined in [`crcbl_core::input`] and re-exported so a
/// consumer of the event stream needs one `use` rather than two.
///
/// See the [crate docs](self) for why these live in `crcbl-core`.
pub use crcbl_core::input::{
    ButtonState, DeviceId, Keysym, Modifiers, PointerButton, Scancode, ScrollDelta,
};
/// Cross-seam vocabulary from `crcbl-core`: the input-event clock, the key
/// names, and the one sanctioned platform leak.
pub use crcbl_core::{EventTime, KeyCode, SurfaceTarget};

use core::time::Duration;

/// The window system, as the engine sees it.
///
/// Object-safe by requirement; see the [crate docs](self) for why, and
/// [`backend`] for how an implementation is selected at runtime.
///
/// # Implementing this
///
/// Six obligations that are not visible in the signatures. The first three are
/// P0.4's; the last three are P0.5c's, written down because a real compositor
/// showed that leaving them unstated pushes the work onto every consumer.
///
/// 1. **Stale handles fail cleanly.** Every method taking a [`WindowId`] must
///    return [`ShellError::InvalidWindow`] for a destroyed or foreign one, never
///    act on whatever window took the slot.
/// 2. **Timestamps are rebased.** Input events carry the window system's
///    timestamp converted to the engine epoch — see [`EventTime`], which
///    documents the conversion each platform needs.
/// 3. **Capabilities are latched.** [`caps`](Self::caps) must return the same
///    value for the shell's lifetime, so a consumer that branched on it at
///    startup is never contradicted mid-session. See [`caps`] for the
///    Wayland-binds-a-global-late edge this rules out.
/// 4. **Every accepted clipboard read is answered exactly once, and within a
///    bounded time.** [`clipboard_request`](Self::clipboard_request) that
///    returns `Ok` promises exactly one
///    [`ShellEvent::ClipboardData`] carrying that
///    [`ClipboardRequestId`]. A backend that cannot complete the read answers
///    [`ClipboardContent::Unavailable`] rather than dropping the request; a
///    request with no answer is a UI stuck on "pasting…" forever, which no
///    consumer can detect.
/// 5. **A read that cannot be answered *yet* is held, never answered "empty".**
///    If the backend does not yet know what is on the clipboard — Wayland's
///    focus-gated, asynchronous `wl_data_device.selection` is the case this
///    exists for — it holds the request until it does, and bounds that wait.
///    Reporting [`Empty`](ClipboardContent::Empty) for "I have not looked yet"
///    is indistinguishable from an empty clipboard, so it makes every consumer
///    write a retry loop with a timeout it cannot choose correctly. Where the
///    answer is always immediately available (X11 asks the server who owns the
///    selection; a headless shell owns it), holding costs nothing and this
///    obligation is discharged by answering.
/// 6. **A refusal for want of user interaction is named.** Where the window
///    system requires a recent input event to claim the clipboard — Wayland
///    does, the browser does, X11 and Win32 do not —
///    [`clipboard_offer`](Self::clipboard_offer) returns
///    [`ShellError::NeedsUserInteraction`] and not a generic backend error, so
///    a caller can distinguish "ask the user to press the key again" from "this
///    is broken". A backend on a platform without the rule never returns it.
pub trait Shell: core::fmt::Debug {
    /// Which implementation this is. Logs and bug reports only — behaviour
    /// branches on [`caps`](Self::caps).
    fn backend(&self) -> ShellBackend;

    /// What this backend can do. Fixed for the shell's lifetime.
    fn caps(&self) -> ShellCaps;

    /// Creates a window. **The window has no size yet.**
    ///
    /// `desc.size` is a request. The authoritative size arrives later, as the
    /// first [`ShellEvent::Resized`], and until it does
    /// [`WindowState::size`] is `None`. This is not a headless quirk: a Wayland
    /// `xdg_surface` is unsized and may not be given a buffer until the
    /// compositor answers with a `configure`, which is a round trip away.
    ///
    /// A swapchain needs an extent, so this is a hard ordering constraint:
    ///
    /// ```
    /// # use crcbl_shell::{HeadlessShell, PhysicalSize, Shell, WindowDesc};
    /// # let mut shell = HeadlessShell::new();
    /// let window = shell.create_window(&WindowDesc::default())?;
    /// let size: PhysicalSize = loop {
    ///     shell.pump(&mut |_event| { /* … */ });
    ///     if let Some(size) = shell.window_state(window)?.size() {
    ///         break size;
    ///     }
    /// };
    /// // Only now is there an extent to create a swapchain at.
    /// assert_eq!(size, PhysicalSize::new(1280, 720));
    /// # Ok::<(), crcbl_shell::ShellError>(())
    /// ```
    ///
    /// [`HeadlessShell`] deliberately defers the first configure by at least one
    /// `pump`, so a consumer that skips the loop fails in CI rather than at
    /// P0.5.
    ///
    /// # Errors
    ///
    /// [`ShellError::WindowCreation`] if the window system refused, or
    /// [`ShellError::Unsupported`] if this would be a second window on a
    /// backend without [`ShellCaps::MULTI_WINDOW`].
    fn create_window(&mut self, desc: &WindowDesc<'_>) -> Result<WindowId, ShellError>;

    /// Destroys a window. A [`ShellEvent::WindowDestroyed`] follows.
    ///
    /// The [`SurfaceTarget`] previously returned for this window is dead
    /// afterwards; the HAL surface built from it must be destroyed **first**.
    ///
    /// # Errors
    ///
    /// [`ShellError::InvalidWindow`] if the handle is stale.
    fn destroy_window(&mut self, window: WindowId) -> Result<(), ShellError>;

    /// A snapshot of the window's current state.
    ///
    /// For consumers that started late; not a substitute for handling events.
    /// Note that [`WindowState::size`] is `None` until the window system has
    /// configured the window, and that the *requested* mode and the *effective*
    /// one are separate fields — see [`WindowState`].
    ///
    /// # Errors
    ///
    /// [`ShellError::InvalidWindow`] if the handle is stale.
    fn window_state(&self, window: WindowId) -> Result<WindowState, ShellError>;

    /// Sets the title bar text.
    ///
    /// # Errors
    ///
    /// [`ShellError::InvalidWindow`] if the handle is stale.
    fn set_title(&mut self, window: WindowId, title: &str) -> Result<(), ShellError>;

    /// Maps or unmaps the window.
    ///
    /// # Errors
    ///
    /// [`ShellError::InvalidWindow`] if the handle is stale.
    fn set_visible(&mut self, window: WindowId, visible: bool) -> Result<(), ShellError>;

    /// Asks for windowed or borderless.
    ///
    /// **A request, not a command.** It updates
    /// [`WindowState::requested_mode`] immediately and nothing else; the mode
    /// that is actually in effect appears in
    /// [`WindowConfiguration::mode`] when the window system answers, which it
    /// may do with something other than what was asked (a compositor can refuse
    /// fullscreen, and a tiling window manager makes the question moot). Use
    /// [`WindowState::mode_request_honoured`] rather than reading the request
    /// back as fact.
    ///
    /// The answer usually arrives as a [`ShellEvent::Resized`], preceded by a
    /// [`ShellEvent::ScaleFactorChanged`] when the target monitor has a
    /// different scale. Never assume the new size — read the event.
    ///
    /// # Errors
    ///
    /// [`ShellError::InvalidWindow`] if the handle is stale, or
    /// [`ShellError::NoSuchMonitor`] if the named monitor is gone.
    fn set_mode(&mut self, window: WindowId, mode: DisplayMode) -> Result<(), ShellError>;

    /// Asks for resize limits and an aspect lock.
    ///
    /// Hints, not guarantees, and a request rather than a command: it updates
    /// [`WindowState::requested_constraints`] immediately, and the only
    /// observable effect is whatever size the window system reports next. No
    /// platform reports back "effective constraints", so neither does this
    /// seam. See [`SizeConstraints`].
    ///
    /// # Errors
    ///
    /// [`ShellError::InvalidWindow`] if the handle is stale.
    fn set_constraints(
        &mut self,
        window: WindowId,
        constraints: SizeConstraints,
    ) -> Result<(), ShellError>;

    /// Every connected monitor.
    ///
    /// Invalidated by [`ShellEvent::MonitorsChanged`]; call again rather than
    /// caching across it.
    fn monitors(&self) -> &[MonitorInfo];

    /// One monitor by id, or `None` if it is not connected.
    ///
    /// A provided method rather than a required one: it is a lookup over
    /// [`monitors`](Self::monitors), and every backend would write the same
    /// three lines.
    fn monitor(&self, id: MonitorId) -> Option<&MonitorInfo> {
        self.monitors().iter().find(|monitor| monitor.id == id)
    }

    /// Drains everything that has arrived from the window system into `sink`.
    ///
    /// Non-blocking and finite: it delivers what is queued and returns. Call it
    /// once at the top of every frame. See the [crate docs](self) for why there
    /// is no `run()` and how a `requestAnimationFrame`-driven build uses this.
    ///
    /// The sink is `&mut dyn FnMut` rather than a generic parameter because the
    /// trait must stay object-safe. That is not purely a cost: a closure passed
    /// here can borrow the caller's state mutably, which an
    /// `Iterator`-returning shape could not while the shell is also borrowed.
    fn pump(&mut self, sink: &mut dyn FnMut(ShellEvent));

    /// Blocks until an event arrives or `timeout` elapses.
    ///
    /// **Advisory.** Returning immediately is always correct, and backends
    /// without [`ShellCaps::EVENT_WAIT`] — a browser above all — do exactly
    /// that. A caller must treat this as "sleep if you can" and must not assume
    /// an event is pending afterwards; [`pump`](Self::pump) is still what
    /// delivers them.
    ///
    /// `timeout` of `None` means "until something happens", which on a backend
    /// without the capability still means "immediately".
    fn wait_events(&mut self, timeout: Option<Duration>) {
        let _ = timeout;
    }

    /// Puts input timestamps on the engine's clock.
    ///
    /// Call this **once, at startup**, with the reading of the
    /// [`TimeSource`](crcbl_core::time::TimeSource) that will drive
    /// [`FrameClock::update`](crcbl_core::FrameClock::update).
    ///
    /// [`EventTime`]'s epoch is defined as "the same origin as" that source,
    /// and no backend can know it: a shell is created some time *after* the
    /// clock starts, and the window system's own clock has a third origin
    /// again. Without this call, timestamps are measured from the shell's own
    /// creation — internally consistent, so durations between two input events
    /// are already correct, but offset from a
    /// [`FrameClock`](crcbl_core::FrameClock) reading by however long startup
    /// took. That offset is exactly what `docs/plan/26`'s input-to-photon
    /// latency measurement is a subtraction across.
    ///
    /// A provided method with a no-op default, because it is only meaningful
    /// for a backend whose event clock is not already the engine's — the
    /// browser's `event.timeStamp` needs nothing done to it. The obligation it
    /// discharges is the "Timestamps are rebased" one in this trait's
    /// [implementor notes](Self), which previously had no way to be satisfied
    /// from outside the crate.
    ///
    /// ```
    /// # use core::time::Duration;
    /// # use crcbl_shell::{HeadlessShell, Shell};
    /// let mut shell = HeadlessShell::new();
    /// // The engine clock has been running for 40 ms by the time the window
    /// // system is up; input stamped "now" should read as 40 ms, not as zero.
    /// shell.align_event_clock(Duration::from_millis(40));
    /// ```
    fn align_event_clock(&mut self, elapsed: Duration) {
        let _ = elapsed;
    }

    /// The native handles a HAL backend needs to create a surface for this
    /// window.
    ///
    /// Opaque to everyone except a HAL backend. Re-query it after a
    /// [`set_mode`](Self::set_mode): some backends recreate the underlying
    /// surface on a mode switch, and a cached target then names a dead object.
    /// **A resize is not such an event** — see below.
    ///
    /// # Available before the window is configured, and unchanged by a resize
    ///
    /// The *handle* exists as soon as the window does — a `wl_surface` is
    /// created up front and only its size is pending — so this succeeds
    /// immediately and a HAL surface can be created from it right away. Only
    /// the **swapchain** has to wait, because only the swapchain needs an
    /// extent.
    ///
    /// **No variant carries a size**, so the value this returns does not change
    /// when the window is resized, and a HAL surface created before the first
    /// configure stays correct forever. That is what makes "re-query after a
    /// mode change" a complete rule. [`SurfaceTarget::Offscreen`] used to carry
    /// `width`/`height` and was the one exception; P0.7 removed it, because an
    /// exception in this rule means a surface silently describing a size the
    /// window no longer has. The extent reaches a backend through
    /// [`WindowState::size`] and the swapchain descriptor, and only there —
    /// `crcbl-hal`'s `swapchain` module states that precedence as a backend
    /// obligation.
    ///
    /// # Errors
    ///
    /// [`ShellError::InvalidWindow`] if the handle is stale.
    fn surface_target(&self, window: WindowId) -> Result<SurfaceTarget, ShellError>;

    /// Frees, confines or locks the pointer.
    ///
    /// # Errors
    ///
    /// [`ShellError::Unsupported`] if [`caps`](Self::caps) lacks
    /// [`PointerMode::required_cap`], or [`ShellError::InvalidWindow`] for a
    /// stale handle.
    fn set_pointer_mode(&mut self, window: WindowId, mode: PointerMode) -> Result<(), ShellError>;

    /// Sets the cursor shape, or hides the cursor with `None`.
    ///
    /// Visibility and capture are separate axes; see [`cursor`].
    ///
    /// # Errors
    ///
    /// [`ShellError::InvalidWindow`] if the handle is stale.
    fn set_cursor(
        &mut self,
        window: WindowId,
        cursor: Option<CursorIcon>,
    ) -> Result<(), ShellError>;

    /// Moves the pointer to a position in the window.
    ///
    /// Requires [`ShellCaps::POINTER_WARP`], which **Wayland does not have and
    /// will not get**. Anything reaching for this to implement a camera should
    /// use [`PointerMode::Locked`] instead; warping is for a UI that needs to
    /// reposition a cursor (a slider snapping to a value), not for input.
    ///
    /// # Errors
    ///
    /// [`ShellError::Unsupported`] without the capability, or
    /// [`ShellError::InvalidWindow`] for a stale handle.
    fn warp_pointer(&mut self, window: WindowId, position: PhysicalPoint)
    -> Result<(), ShellError>;

    /// Answers a [`ShellEvent::CloseRequested`].
    ///
    /// Until this is called the window stays open and the request stays
    /// outstanding. See [`CloseReply`].
    ///
    /// # Errors
    ///
    /// [`ShellError::NoPendingCloseRequest`] if nothing was asked, or
    /// [`ShellError::InvalidWindow`] for a stale handle.
    fn reply_close_request(
        &mut self,
        window: WindowId,
        reply: CloseReply,
    ) -> Result<(), ShellError>;

    /// Publishes clipboard content in one or more formats.
    ///
    /// Synchronous: every platform's write is "claim the selection", and the
    /// bytes transfer later, on demand. Offer both [`MimeType::TextUtf8`] and
    /// [`MimeType::CrcblRon`] for engine data; see [`clipboard`].
    ///
    /// An empty `offers` slice releases the clipboard, where the platform can
    /// express that.
    ///
    /// # Errors
    ///
    /// [`ShellError::Unsupported`] without [`ShellCaps::CLIPBOARD`],
    /// [`ShellError::InvalidWindow`] for a stale handle, or
    /// [`ShellError::NeedsUserInteraction`] where the window system requires a
    /// recent input event to claim the clipboard and there has not been one —
    /// implementor obligation 6 above, and **not** a failure a caller should
    /// retry blindly.
    fn clipboard_offer(
        &mut self,
        window: WindowId,
        offers: &[ClipboardOffer<'_>],
    ) -> Result<(), ShellError>;

    /// Asks for the clipboard's content in `mime`.
    ///
    /// Asynchronous by necessity — the answer arrives as
    /// [`ShellEvent::ClipboardData`] carrying the returned id, and **exactly one
    /// answer arrives for every `Ok` return** (implementor obligations 4 and 5).
    /// [`clipboard`] has the full argument; briefly, X11 `INCR` transfers,
    /// Wayland file descriptors and the browser's Promise make the synchronous
    /// shape unimplementable on three of five backends.
    ///
    /// The answer may be several frames away, and on a backend whose clipboard
    /// is not readable at the moment of asking it is *held* until it is, rather
    /// than being answered [`Empty`](ClipboardContent::Empty). A caller
    /// therefore never needs to poll or retry: ask once, handle the event.
    /// [`clipboard_readable`](Self::clipboard_readable) is the way to find out
    /// in advance that the answer will not be immediate.
    ///
    /// # Errors
    ///
    /// [`ShellError::Unsupported`] without [`ShellCaps::CLIPBOARD`], or
    /// [`ShellError::InvalidWindow`] for a stale handle. An empty clipboard is
    /// **not** an error — it is an answer.
    fn clipboard_request(
        &mut self,
        window: WindowId,
        mime: MimeType,
    ) -> Result<ClipboardRequestId, ShellError>;

    /// Whether a [`clipboard_request`](Self::clipboard_request) issued for
    /// `window` right now could be answered without waiting.
    ///
    /// What a "Paste" menu item greys out on, and the visible form of a
    /// platform rule that is otherwise invisible: **on Wayland a client is told
    /// what is on the clipboard only while one of its windows has keyboard
    /// focus**, and only asynchronously, so a background window genuinely
    /// cannot read the clipboard. `clipboard_request` still works there — it
    /// holds the request — but it may take until focus arrives, and a UI that
    /// would rather not offer the action than offer a slow one asks here first.
    ///
    /// `false` for a stale handle, and for a backend without
    /// [`ShellCaps::CLIPBOARD`].
    ///
    /// # A provided method, because most platforms have nothing to say
    ///
    /// The default is "readable whenever the capability is present and the
    /// handle is live", which is exactly right for X11 (any window may
    /// `ConvertSelection` at any time, focus is not involved), for Win32, and
    /// for [`HeadlessShell`] in its default configuration. Only a backend that
    /// really has a gate overrides it — so this is a fact a consumer can rely
    /// on, not ceremony every backend has to perform.
    ///
    /// The staleness half goes through
    /// [`window_state`](Self::window_state) rather than being left to each
    /// implementation. P0.6 found the earlier default — which ignored `window`
    /// entirely — contradicting the "`false` for a stale handle" sentence
    /// above: both backends that existed had *overridden* it and so had
    /// covered for it, and the first backend to accept the default inherited
    /// the bug. A default that cannot keep its own promise is worse than no
    /// default.
    fn clipboard_readable(&self, window: WindowId) -> bool {
        self.caps().contains(ShellCaps::CLIPBOARD) && self.window_state(window).is_ok()
    }
}
