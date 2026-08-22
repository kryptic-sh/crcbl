//! The AppKit backend: the Objective-C runtime, `NSApplication`, the window
//! lifecycle, display modes, size constraints, `NSScreen` and the
//! `CAMetalLayer`.
//!
//! Same terms as the other three native backends: private, reached only through
//! [`open`](crate::open). The `cfg` is the Win32 one for the Win32 reason —
//! **the parts of this backend that are pure arithmetic compile on every host
//! under `cfg(test)`** — and it earns more here than it did there, because more
//! of this backend *is* arithmetic: macOS is the only platform whose coordinate
//! system disagrees with the seam's about which way the Y axis points, and the
//! flip that reconciles them is exercised by `cargo test` on the Linux machine
//! this engine is developed on. Everything that calls AppKit is
//! `#[cfg(target_os = "macos")]` inside the module, which is also why there is
//! no `dlopen` here — see [`ffi`], which makes the same argument
//! `src/win32/ffi.rs` does and the reverse of `src/x11/ffi.rs`'s.
//!
//! `docs/plan/15-windowing.md`'s macOS row, which reads in full: "hand-written
//! Objective-C runtime FFI (`objc_msgSend`) to AppKit". Every declaration in
//! [`ffi`] is ours; there is no `objc2`, no `cocoa`, no `core-foundation`, no
//! framework — and, like the Win32 backend and unlike the two Linux ones, no
//! `dlopen`, for the reason [`ffi`] states at length.
//!
//! # What is here, and what is not
//!
//! **P5C M1** was the window lifecycle, **M2** added input and **M3** the
//! pasteboard and drag and drop. M4 is the end-to-end pass. What is left is
//! stated here rather than implied, and [`ShellCaps`] agrees
//! with it bit for bit:
//!
//! | Area | State |
//! | --- | --- |
//! | Objective-C runtime FFI: classes, selectors, `objc_msgSend`, runtime-built classes, autorelease pools | complete — [`ffi`] |
//! | `NSApplication` bootstrap: activation policy, `finishLaunching`, activation | complete — [`app`] |
//! | Window lifecycle: create, show, hide, destroy, title, close-request interception | complete |
//! | Windowed ↔ borderless on a named display, with the windowed frame and style mask restored exactly | complete |
//! | Size constraints: `setContentMinSize:`, `setContentMaxSize:`, `setContentAspectRatio:` | complete — [`ASPECT_HINT_HONORED`](crate::ShellCaps::ASPECT_HINT_HONORED) |
//! | `NSScreen` enumeration: frame, visible frame, backing scale, refresh, primary, hotplug | complete — [`monitors`] |
//! | Event pump, and a blocking [`wait_events`](crate::Shell::wait_events) | complete — [`EVENT_WAIT`](crate::ShellCaps::EVENT_WAIT) |
//! | `CAMetalLayer` on a layer-hosting `NSView`, as [`SurfaceTarget::AppKit`](crcbl_core::SurfaceTarget::AppKit) | complete — [`window`] |
//! | Keyboard: `kVK_*` codes, [`KeyCode`](crcbl_core::KeyCode), keysyms, modifiers through `flagsChanged:`, auto-repeat | complete — [`keys`] |
//! | Text: `interpretKeyEvents:` through a real `NSTextInputClient`, as [`TextCommit`](crate::ShellEvent::TextCommit) | complete — [`TEXT_IME`](crate::ShellCaps::TEXT_IME), see [`view`] and [`caps`](AppKitShell::caps) |
//! | Pointer: motion, buttons past the fifth, enter and leave through an `NSTrackingArea`, both scroll units | complete — [`mod@pointer`] |
//! | Relative motion from `NSEvent`'s `deltaX`/`deltaY` | complete — [`RAW_POINTER_MOTION`](crate::ShellCaps::RAW_POINTER_MOTION), **accelerated**; [`caps`](AppKitShell::caps) says exactly what the bit means here |
//! | [`PointerMode::Locked`](crate::PointerMode) and [`warp_pointer`](crate::Shell::warp_pointer) | complete — [`POINTER_LOCK`](crate::ShellCaps::POINTER_LOCK), [`POINTER_WARP`](crate::ShellCaps::POINTER_WARP) |
//! | [`PointerMode::Confined`](crate::PointerMode) | **never** — macOS has no confine API at all, and every approximation is a warp fighting the user. [`POINTER_CONFINE`](crate::ShellCaps::POINTER_CONFINE) is clear; see [`set_pointer_mode`](crate::Shell::set_pointer_mode) |
//! | Cursor shapes and hiding | complete — `NSCursor` through `cursorUpdate:`, hiding through a balanced hide count |
//! | Event timestamps rebased onto the engine clock | complete — [`TimeBase`] |
//! | `NSPasteboard`, both directions, `public.utf8-plain-text` beside the engine's own format | complete — [`CLIPBOARD`](crate::ShellCaps::CLIPBOARD), see [`pasteboard`] |
//! | File drops in: `registerForDraggedTypes:`, `NSDraggingDestination`, the `accept_drops` gate | complete — [`DRAG_DROP`](crate::ShellCaps::DRAG_DROP), see [`view`] |
//! | Drag and drop **out** — starting a drag from a window | **never in this plan** — `docs/plan/15-windowing.md` scopes drag-and-drop to "file paths in (viewer/editor import)", which is what every other backend implements too |
//! | Lazily provided pasteboard data (`pasteboard:provideDataForType:`) | **never** — structurally unavailable to a shell whose callbacks record rather than act; [`pasteboard`] argues it in full |
//! | Spaces fullscreen (`toggleFullScreen:`) | **never** — `docs/plan/15-windowing.md` keeps two display modes, and borderless here is a frameless window at screen size |
//!
//! # What AppKit does that no other backend models
//!
//! In the order they shaped the code:
//!
//! 1. **The Y axis points up, and the origin is the bottom-left of the primary
//!    display.** Win32's virtual screen, X11's screen and the seam's
//!    [`PhysicalRect`](crate::PhysicalRect) all put the origin at the top-left
//!    with Y increasing downwards; AppKit is the one that disagrees. Every
//!    screen rectangle, window frame and display bound crosses that flip, which
//!    is why it is a single pure function — [`geometry::Flip`] — that is its own
//!    inverse, and why its fixtures run on the Linux host rather than only on a
//!    Mac.
//! 2. **The global coordinate space is *points*, not pixels.** The other two
//!    desktop platforms lay their monitors out in one space of device pixels;
//!    AppKit lays them out in logical units and each display carries its own
//!    `backingScaleFactor`. So [`MonitorInfo::bounds`](crate::MonitorInfo::bounds)
//!    does not tile across displays of different scales — the caveat that type
//!    already states for Wayland, arriving on a second platform for a different
//!    reason — while window *placement* stays exact, because it is expressed in
//!    the space AppKit actually has. [`monitors`] keeps both forms for exactly
//!    that reason.
//! 3. **Logical units are the platform's own units.** [`SizeConstraints`](crate::SizeConstraints)
//!    is in logical units and `setContentMinSize:` takes points, and they are the
//!    same thing. The Win32 backend multiplies by the DPI and adds the window
//!    frame; the X11 backend divides by `Xft.dpi`. This is the only backend of
//!    the five where the conversion is the identity, and
//!    [`geometry::content_limits`] is where that shows.
//! 4. **`backingScaleFactor` is 1.0 or 2.0 and nothing between**, so
//!    [`FRACTIONAL_SCALE`](crate::ShellCaps::FRACTIONAL_SCALE) is **clear** on
//!    the one desktop backend a reader would expect it to be set on. A "scaled"
//!    HiDPI mode changes the point resolution and leaves the factor at 2.0, so
//!    the fractional part lands in the geometry rather than in the scale.
//! 5. **AppKit is main-thread-only, and it enforces that by raising.**
//!    `nextEventMatchingMask:untilDate:inMode:dequeue:` asserts on the thread
//!    and throws an `NSException`, which unwinding through a Rust frame is
//!    undefined behaviour rather than an error. This is the concrete reason
//!    [`Shell`](crate::Shell) is not `Send`, and [`AppKitShell::open`] refuses
//!    off the main thread instead of finding out. It has a consequence for the
//!    test suite that is written up in [`app`] and in `docs/backlog.md`, and it
//!    is a stronger rule than Win32's thread affinity: there, *any* thread may
//!    own a window as long as it is the one pumping.
//! 6. **A plain binary is not a bundled `.app`**, and starts as a background
//!    application that cannot become frontmost. `setActivationPolicy:` is the
//!    lever, it is asked for explicitly before the first window exists, and its
//!    answer is kept — see [`app`] — because "the window opened but nothing
//!    could focus it" otherwise arrives with no diagnosis at all. No other
//!    platform has an equivalent: a Win32 process is an application by existing.
//!    (This backend does not build a menu bar. An unbundled application with the
//!    Regular policy gets the system's default one, which is enough for a window
//!    to be focusable and is not enough to ship; `docs/backlog.md` carries it.)
//! 7. **The menu bar and the Dock are process-wide state a borderless window has
//!    to take.** `NSApplicationPresentationOptions` is not a window property, so
//!    the shell derives it from *all* its windows and hands it back on the way
//!    out — the same shape as the Win32 backend's cursor clip, and for the same
//!    reason. And an invalid combination **raises**, which is why
//!    [`geometry::presentation_options`] never produces the menu-bar bit on its
//!    own and a test asserts the pairing rather than the number.
//! 8. **A borderless window cannot take the keyboard unless it is told it can.**
//!    `-[NSWindow canBecomeKeyWindow]` answers `NO` for a style mask with
//!    neither `Titled` nor `Resizable` in it, and `Borderless` is zero. So this
//!    backend builds an `NSWindow` subclass at runtime that overrides it — see
//!    [`window`]. Nothing on Windows or X11 has an equivalent; a frameless
//!    window there is a window.
//! 9. **The geometry is state, so the notification carries none of it.**
//!    `WM_SIZE` arrives with the new size because that is the only moment it
//!    exists; `windowDidResize:` arrives with a notification and `[window
//!    frame]` answers whenever it is asked. So [`events::RawEvent`] records
//!    *that* something changed and never *what to*, `translate` reads once, and
//!    a recorded number cannot disagree with the window it describes. It is the
//!    one place this backend is simpler than the Windows one rather than harder.
//! 10. **`releasedWhenClosed` defaults to `YES`.** A programmatically created
//!     `NSWindow` releases itself when it closes, and this shell holds the
//!     pointer afterwards — so leaving it on is a use-after-free that fires only
//!     on the close path. It is turned off at creation, before anything that
//!     could close the window. There is no reference counting anywhere else in
//!     this crate; this is the only backend whose objects have any.
//! 11. **The delegate is a class this crate writes at runtime.** Win32 registers
//!     one window procedure and switches on a message id; AppKit dispatches by
//!     selector, so the equivalent is `objc_allocateClassPair` plus one
//!     `class_addMethod` per callback — which is what `@implementation` compiles
//!     to, written out. [`app`] does it for the window delegate, [`window`] for
//!     the `NSWindow` subclass and [`view`] for the `NSView` subclass that is
//!     the whole of input.
//! 12. **Four things have to be switched on before a real event exists at all**,
//!     and each of them is invisible to a test that calls the responder method
//!     itself. This is the AppKit shape of the `TranslateMessage` gap the Win32
//!     half of P5C paid a CI round trip for, and it is written out in [`view`]
//!     because it is four gaps rather than one: `setAcceptsMouseMovedEvents:`,
//!     `makeFirstResponder:`, the `NSTrackingArea`, and `interpretKeyEvents:`.
//! 13. **A modifier key produces no key event.** Shift, Control, Option and
//!     Command deliver `flagsChanged:` and never `keyDown:`, so the down and up
//!     edges are reconstructed from device-dependent flag bits — one per
//!     physical key, because the documented flag stays set while the *other*
//!     Shift is held. [`keys::modifier_is_down`] is that reconstruction, and
//!     nothing on Windows, X11 or Wayland needs an equivalent.
//! 14. **A position is Y-up and the delta beside it is Y-down, on one event.**
//!     `locationInWindow` is AppKit's own space and `deltaX`/`deltaY` are
//!     Quartz's, which already agrees with this seam. So the position is
//!     reflected and the delta is not — see [`mod@pointer`], which exists to
//!     make that visible rather than surprising.
//! 15. **macOS cannot confine the pointer, and can lock it more simply than
//!     anyone else.** There is no `ClipCursor` and no `confine_to`;
//!     [`POINTER_CONFINE`](crate::ShellCaps::POINTER_CONFINE) is therefore clear
//!     on a desktop backend, which is the only one of the four where it is. What
//!     the platform *does* have is
//!     `CGAssociateMouseAndMouseCursorPosition(false)`, which freezes the cursor
//!     outright while the deltas keep arriving — so [`Locked`](crate::PointerMode::Locked)
//!     needs none of the clip-and-recentre machinery Win32 and X11 both carry.
//! 16. **The pointer is not AppKit's.** Warping it, freezing it and asking where
//!     it is are all CoreGraphics, in a coordinate space that is the seam's
//!     rather than AppKit's — so a warp crosses **two** reflections, one
//!     window-local and one about the primary display. [`geometry::Flip::point`]
//!     is the second.
//! 17. **The clipboard is versioned, not locked, and not owned.** X11 and
//!     Wayland have an *owner* that serves transfers later; Win32 has content
//!     plus an exclusive `OpenClipboard` another process can be holding.
//!     `NSPasteboard` has neither: `setData:forType:` copies the bytes to the
//!     pasteboard server and `changeCount` records that somebody claimed it. So
//!     this is the only backend whose clipboard needs no deadline, no retry
//!     budget and no state carried across a [`pump`](crate::Shell::pump) — and
//!     the only one where a *write* is assertable as a mechanism rather than
//!     inferred from a read, which is what [`session_support`] does with it.
//! 18. **A drop is read through a pasteboard, so it is the same code as a
//!     paste.** On Win32 a drop is an `HDROP` delivered as a window message and
//!     shares nothing with the clipboard, which is why that backend has two
//!     modules. `-[NSDraggingInfo draggingPasteboard]` answers an
//!     `NSPasteboard`, so this backend has one — the "one implementation, two
//!     triggers" `docs/plan/15-windowing.md` states for the two Linux backends,
//!     arriving on a third platform for a reason of its own.
//!
//! # Decision: `objc_msgSend` is transmuted per call site, from one place
//!
//! **There is no variadic ABI for it on `aarch64-apple-darwin`** — which is the
//! architecture of both the CI runner and every Mac sold since 2020 — so the
//! trampoline must be called with the exact signature of the method it
//! dispatches to. Getting that wrong compiles, links, runs, and hands the method
//! whatever was in the register it looked in. [`ffi::msg_send`] is the one place
//! the symbol's address is taken and the one mechanism every call goes through;
//! [`ffi::msg_send_stret`] is the x86_64 large-struct-return split, implemented
//! on both sides rather than only on the one the runner uses. [`ffi`] argues
//! both at length, and the macOS suite dispatches every shape of it against a
//! class built for the purpose.
//!
//! # Decision: the live resize drag is accepted, on the same terms as Windows
//!
//! While the user drags a window's edge, AppKit runs the resize inside an
//! event-tracking run-loop mode and `sendEvent:` does not return until the mouse
//! comes up — so [`pump`](crate::Shell::pump) does not return either, and no
//! frame is rendered for the length of the drag. It is the same cost the Win32
//! backend documents for `WM_ENTERSIZEMOVE`, it has the same fix (a timer that
//! calls the engine back, which needs a seam this crate deliberately does not
//! have), and `docs/backlog.md` carries it once for both platforms rather than
//! twice.
//!
//! The event flood is bounded rather than deferred: [`events::enqueue`] collapses
//! a run of `windowDidResize:` markers into one, so a three-second drag delivers
//! exactly one [`Resized`](crate::ShellEvent::Resized) carrying the size the
//! window ended at.

pub mod events;
pub mod geometry;
pub mod keys;
pub mod pasteboard;
pub mod pointer;

// The ABI declarations and the pure arithmetic over them are compiled on every
// host, so that the coordinate flip and the points-to-pixels conversion are
// exercised by `cargo test` on the Linux machine this engine is developed on —
// see `lib.rs`, and see `ffi` for why the `extern` blocks inside it are the one
// part that cannot be. The allowance is scoped to the non-macOS build: on macOS
// an unused declaration is still an error, which is what keeps this module
// "audited by use" rather than a table of everything `AppKit.h` has.
#[cfg_attr(not(target_os = "macos"), allow(dead_code))]
pub mod ffi;

#[cfg(target_os = "macos")]
mod app;
#[cfg(target_os = "macos")]
mod input;
#[cfg(target_os = "macos")]
mod monitors;
#[cfg(target_os = "macos")]
pub mod session_support;
#[cfg(target_os = "macos")]
mod shell;
#[cfg(target_os = "macos")]
mod view;
#[cfg(target_os = "macos")]
mod window;

#[cfg(target_os = "macos")]
pub use shell::AppKitShell;

use core::time::Duration;

use crcbl_core::EventTime;

use crate::ShellCaps;

/// Puts `-[NSEvent timestamp]` on the engine's clock.
///
/// # A `double` of seconds, where the other three backends count milliseconds
///
/// [`EventTime`] wants "a duration measured from the same origin as the
/// [`TimeSource`](crcbl_core::time::TimeSource) driving
/// [`FrameClock::update`](crcbl_core::FrameClock::update)". Each platform
/// arrives at that from somewhere different, and this one is the odd member:
///
/// * **Wayland** stamps with `CLOCK_MONOTONIC` milliseconds — the right clock,
///   the wrong zero.
/// * **X11** stamps with milliseconds since the *server* started, which is a
///   clock this process has never read, so the offset is calibrated from the
///   first event.
/// * **Win32** stamps with `GetTickCount` milliseconds and reads the same
///   counter at full width, so nothing is calibrated and only the 49.7-day wrap
///   has to be resolved.
/// * **AppKit** stamps with an `NSTimeInterval` — a `double` of **seconds since
///   the system booted**, on `mach_absolute_time`'s clock.
///
/// Two consequences follow from the type rather than from the platform, and both
/// are why this is not the Win32 one with a different constant.
///
/// **There is no wrap.** A 32-bit millisecond counter turns over after seven
/// weeks of uptime and both other native backends carry machinery for it; an
/// `f64` of seconds does not turn over at all. What it has instead is
/// *precision*, and it is not a problem: at a year of uptime (about 3×10⁷
/// seconds) a `double`'s ulp is still under a microsecond, which is finer than
/// the input stack that produced the timestamp.
///
/// **The clock has to be read through [`CACurrentMediaTime`](ffi::CACurrentMediaTime)
/// and not through anything else that is also called uptime.**
/// `-[NSProcessInfo systemUptime]` is a different number with a similar name;
/// rebasing against it would put every event a constant offset from the truth,
/// which is exactly the failure a timestamp cannot report about itself.
///
/// # An event with no timestamp
///
/// AppKit hands out synthesized events — `+[NSEvent otherEventWithType:…]` and
/// the ones a `sendEvent:` interception makes — with a timestamp of zero.
/// [`EventTime`] is explicit that such an event is stamped with the *current*
/// time rather than [`EventTime::ZERO`], because a zero reads as "this happened
/// at process start" to everything downstream. [`event_time_at`](Self::event_time_at)
/// is where that substitution happens, once, rather than at each call site.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct TimeBase {
    /// [`CACurrentMediaTime`](ffi::CACurrentMediaTime) seconds at the engine
    /// epoch.
    epoch_seconds: f64,
}

impl TimeBase {
    /// An epoch of `now_seconds`, which at [`AppKitShell::open`] is the shell's
    /// creation.
    #[must_use]
    pub const fn at(now_seconds: f64) -> Self {
        Self {
            epoch_seconds: now_seconds,
        }
    }

    /// Moves the epoch so that `now_seconds` reads as `elapsed`.
    ///
    /// See [`Shell::align_event_clock`](crate::Shell::align_event_clock), which
    /// this implements: a shell is created some time *after* the engine clock
    /// starts, and only the caller knows how long.
    pub fn align_at(&mut self, now_seconds: f64, elapsed: Duration) {
        self.epoch_seconds = (now_seconds - elapsed.as_secs_f64()).max(0.0);
    }

    /// This base applied to an event's timestamp, with the clock passed in.
    ///
    /// Split from the reading of the clock for the reason `crcbl-core`'s
    /// `TimeSource` gives: nothing reads a clock where a test needs to drive
    /// one, and neither a synthesized event nor a shell created an hour into a
    /// session is something a test can arrange by waiting.
    ///
    /// A timestamp of zero — or of anything that is not a finite, positive
    /// number of seconds — is an event AppKit did not stamp, and reads as
    /// `now_seconds`. An event from before the epoch reads as the epoch:
    /// [`EventTime`] is a duration since it and there is no negative one, which
    /// is reachable after an [`align_at`](Self::align_at) that moves the epoch
    /// past an event already in the queue.
    #[must_use]
    pub fn event_time_at(self, now_seconds: f64, event_seconds: f64) -> EventTime {
        let stamped = if event_seconds.is_finite() && event_seconds > 0.0 {
            event_seconds
        } else {
            now_seconds
        };
        let since_epoch = stamped - self.epoch_seconds;
        if !since_epoch.is_finite() || since_epoch <= 0.0 {
            return EventTime::ZERO;
        }
        EventTime::from_duration(Duration::from_secs_f64(since_epoch))
    }
}

/// What this backend can do, fixed for a shell's lifetime.
///
/// # Nothing latches, and that is a statement about macOS
///
/// The two Linux backends compute a capability set from what the server turned
/// out to have — an X server with no RandR, a compositor with no
/// `wp_viewporter` — and the Win32 backend latches one bit, on whether raw input
/// registered. Here there is nothing to latch: every API this slice uses is
/// AppKit's own and is present, or the image has no AppKit in it and
/// `AppKitShell::open` has already failed with a `Connect` error naming the
/// missing class.
///
/// So this is a `const fn` and lives here rather than on the shell, and **that
/// is what makes the set assertable without a window** — which matters more on
/// this platform than on any other, for the reason [`app`] gives about the test
/// harness. Every bit and every deliberate absence is argued on
/// [`AppKitShell::caps`](crate::Shell::caps).
#[must_use]
pub(crate) const fn caps() -> ShellCaps {
    ShellCaps::MULTI_WINDOW
        .union(ShellCaps::EVENT_WAIT)
        .union(ShellCaps::WINDOW_POSITION)
        .union(ShellCaps::SERVER_DECORATIONS)
        .union(ShellCaps::ASPECT_HINT_HONORED)
        .union(ShellCaps::POINTER_LOCK)
        .union(ShellCaps::POINTER_WARP)
        .union(ShellCaps::RAW_POINTER_MOTION)
        .union(ShellCaps::TEXT_IME)
        .union(ShellCaps::CLIPBOARD)
        .union(ShellCaps::DRAG_DROP)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::PointerMode;

    #[test]
    fn the_capability_set_is_exactly_what_this_slice_implements() {
        let caps = caps();
        for present in [
            ShellCaps::MULTI_WINDOW,
            ShellCaps::EVENT_WAIT,
            ShellCaps::WINDOW_POSITION,
            ShellCaps::SERVER_DECORATIONS,
            ShellCaps::ASPECT_HINT_HONORED,
            ShellCaps::POINTER_LOCK,
            ShellCaps::POINTER_WARP,
            ShellCaps::RAW_POINTER_MOTION,
            ShellCaps::TEXT_IME,
            ShellCaps::CLIPBOARD,
            ShellCaps::DRAG_DROP,
        ] {
            assert!(caps.contains(present), "{present:?} is implemented");
        }
        // Clear, and each for a reason `Shell::caps` gives on this backend:
        // `FRACTIONAL_SCALE` because macOS has no fractional scale factor at
        // all, `HW_UPSCALE` because the platform can do it and the seam has no
        // way to ask, and `POINTER_CONFINE` because **macOS has no confine API**
        // and the alternatives are warp-based approximations. A capability that
        // overstates itself is worse than one that is missing.
        for absent in [
            ShellCaps::FRACTIONAL_SCALE,
            ShellCaps::HW_UPSCALE,
            ShellCaps::POINTER_CONFINE,
        ] {
            assert!(!caps.contains(absent), "{absent:?} is not implemented");
        }
        assert!(
            caps.has_mouselook(),
            "a locked pointer and a delta beside it are both here"
        );
        // Capture exists in one form and not the other, which is the shape no
        // other desktop backend has: Win32 and X11 bound the pointer with one
        // mechanism that serves both modes, and macOS has a mechanism for
        // exactly one of them.
        assert!(caps.has_pointer_capture());
        assert!(!caps.contains(PointerMode::Confined.required_cap()));
        assert!(caps.contains(PointerMode::Locked.required_cap()));
        // The set is a constant, so "latched for the shell's lifetime" is not
        // something a test has to watch for — it is the type.
        assert_eq!(caps, super::caps());
    }

    #[test]
    fn an_events_own_timestamp_is_what_survives_and_the_epoch_cancels() {
        // The property the P2 pattern evaluator is a function of: two events'
        // difference is theirs, whatever the shell's epoch was.
        let base = TimeBase::at(9_000.0);
        let pressed = base.event_time_at(9_003.5, 9_003.0);
        assert_eq!(pressed.as_duration(), Duration::from_secs(3));
        let released = base.event_time_at(9_003.5, 9_003.18);
        assert_eq!(
            released.saturating_since(pressed),
            Duration::from_millis(180),
            "180 ms is a tap and not a hold, and no frame boundary got a vote"
        );
    }

    #[test]
    fn an_event_appkit_did_not_stamp_reads_as_now_rather_than_as_process_start() {
        // Synthesized events carry a timestamp of zero, and `EventTime::ZERO`
        // means "at the engine's epoch" to everything downstream — so an input
        // event stamped with it would read as having happened before the game
        // started.
        let base = TimeBase::at(1_000.0);
        assert_eq!(
            base.event_time_at(1_042.0, 0.0).as_duration(),
            Duration::from_secs(42)
        );
        assert_eq!(
            base.event_time_at(1_042.0, f64::NAN).as_duration(),
            Duration::from_secs(42)
        );
        assert_eq!(
            base.event_time_at(1_042.0, -1.0).as_duration(),
            Duration::from_secs(42)
        );
    }

    #[test]
    fn an_event_from_before_the_epoch_reads_as_the_epoch() {
        // `align_event_clock` can move the epoch forward past an event already
        // in the queue. A duration since the epoch has no negative, so clamping
        // is the only honest answer — the same one the other three backends
        // give.
        let base = TimeBase::at(10.0);
        assert_eq!(base.event_time_at(10.0, 9.0), EventTime::ZERO);
        assert_eq!(base.event_time_at(10.0, 10.0), EventTime::ZERO);
    }

    #[test]
    fn aligning_makes_now_read_as_the_engines_elapsed_time() {
        // The shell was created 40 ms into the engine's run, so an event stamped
        // "now" must read as 40 ms rather than as the system's uptime.
        let mut base = TimeBase::at(500_000.0);
        base.align_at(500_000.0, Duration::from_millis(40));
        assert_eq!(
            base.event_time_at(500_000.0, 500_000.0).as_duration(),
            Duration::from_millis(40)
        );
        // An engine clock older than the system's uptime cannot happen, and
        // clamping rather than panicking is what a backend does about a number
        // it did not compute.
        base.align_at(1.0, Duration::from_secs(9_999));
        assert_eq!(base.epoch_seconds, 0.0);
    }

    #[test]
    fn a_year_of_uptime_still_resolves_a_millisecond() {
        // The precision question an `f64` of seconds raises where a 32-bit
        // millisecond counter raises a wrap. A year is about 3.15e7 seconds, and
        // a `double` there has an ulp of a few nanoseconds — so two events a
        // millisecond apart are still a millisecond apart to within far less
        // than the input stack that produced them could resolve.
        let year = 31_536_000.0;
        let base = TimeBase::at(year);
        let first = base.event_time_at(year + 1.0, year + 0.500);
        let second = base.event_time_at(year + 1.0, year + 0.501);
        let apart = second.saturating_since(first);
        let error = apart.abs_diff(Duration::from_micros(1_000));
        assert!(
            error < Duration::from_micros(1),
            "a millisecond at a year of uptime came back as {apart:?}, off by {error:?}"
        );
        // And the wrap the other two native backends carry machinery for does
        // not exist here at all: seven weeks in, the arithmetic is the same.
        let weeks = 4_294_967.296;
        let base = TimeBase::at(weeks);
        assert_eq!(
            base.event_time_at(weeks + 1.0, weeks + 0.25).as_duration(),
            Duration::from_millis(250)
        );
    }
}
