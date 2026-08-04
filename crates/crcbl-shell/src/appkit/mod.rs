//! The AppKit backend: the Objective-C runtime, `NSApplication`, the window
//! lifecycle, display modes, size constraints, `NSScreen` and the
//! `CAMetalLayer`.
//!
//! `docs/plan/15-windowing.md`'s macOS row, which reads in full: "hand-written
//! Objective-C runtime FFI (`objc_msgSend`) to AppKit". Every declaration in
//! [`ffi`] is ours; there is no `objc2`, no `cocoa`, no `core-foundation`, no
//! framework — and, like the Win32 backend and unlike the two Linux ones, no
//! `dlopen`, for the reason [`ffi`] states at length.
//!
//! # What is here, and what is not
//!
//! **P5C M1** is the window lifecycle. M2 is input, M3 the pasteboard and drag
//! and drop, M4 the end-to-end pass. What is left is stated here rather than
//! implied, and [`ShellCaps`](crate::ShellCaps) agrees with it bit for bit:
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
//! | Keyboard, text, pointer, cursors, raw motion, pointer lock | **M2** — [`set_pointer_mode`](crate::Shell::set_pointer_mode) and [`warp_pointer`](crate::Shell::warp_pointer) refuse, and `set_cursor` records without applying |
//! | `NSPasteboard`, drag and drop | **M3** — [`clipboard_offer`](crate::Shell::clipboard_offer) and [`clipboard_request`](crate::Shell::clipboard_request) refuse |
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
//!     to, written out. [`app`] does it for the window delegate and [`window`]
//!     for the `NSWindow` subclass.
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
mod monitors;
#[cfg(target_os = "macos")]
mod shell;
#[cfg(target_os = "macos")]
mod window;

#[cfg(target_os = "macos")]
pub use shell::AppKitShell;

use crate::ShellCaps;

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
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_capability_set_is_exactly_what_this_slice_implements() {
        let caps = caps();
        for present in [
            ShellCaps::MULTI_WINDOW,
            ShellCaps::EVENT_WAIT,
            ShellCaps::WINDOW_POSITION,
            ShellCaps::SERVER_DECORATIONS,
            ShellCaps::ASPECT_HINT_HONORED,
        ] {
            assert!(caps.contains(present), "{present:?} is implemented");
        }
        // Clear, and each for a reason `Shell::caps` gives on this backend:
        // `FRACTIONAL_SCALE` because macOS has no fractional scale factor at
        // all, `HW_UPSCALE` because the platform can do it and the seam has no
        // way to ask, and the rest because M2 and M3 own them. A capability that
        // overstates itself is worse than one that is missing.
        for absent in [
            ShellCaps::FRACTIONAL_SCALE,
            ShellCaps::HW_UPSCALE,
            ShellCaps::POINTER_LOCK,
            ShellCaps::POINTER_CONFINE,
            ShellCaps::POINTER_WARP,
            ShellCaps::RAW_POINTER_MOTION,
            ShellCaps::TEXT_IME,
            ShellCaps::CLIPBOARD,
            ShellCaps::DRAG_DROP,
        ] {
            assert!(!caps.contains(absent), "{absent:?} is not implemented");
        }
        assert!(
            !caps.has_mouselook(),
            "neither half of mouselook exists until M2"
        );
        // The set is a constant, so "latched for the shell's lifetime" is not
        // something a test has to watch for — it is the type.
        assert_eq!(caps, super::caps());
    }
}
