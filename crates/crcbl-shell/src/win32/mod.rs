//! The Win32 backend: window class, message pump, display modes, size
//! constraints, monitors, per-monitor DPI, and the whole of input.
//!
//! `docs/plan/15-windowing.md`'s Windows row, which reads in full: "hand-written
//! Win32 FFI (`extern "system"` decls for the surface we use)". Every
//! declaration in [`ffi`] is ours; there is no `windows-rs`, no `winapi`, no
//! framework, and — unlike the two Linux backends — no `dlopen`, for the reason
//! that module states at length.
//!
//! # What is here, and what is not
//!
//! **P5C W1** was the window lifecycle, **W2** added input, and **W3** added
//! the clipboard and file drops. What is left is stated here rather than
//! implied, and [`ShellCaps`](crate::ShellCaps) agrees with it bit for bit:
//!
//! | Area | State |
//! | --- | --- |
//! | Window lifecycle: create, show, hide, destroy, title, close-request interception | complete |
//! | Windowed ↔ borderless on a named monitor, with the windowed placement restored exactly | complete |
//! | Size constraints: `WM_GETMINMAXINFO` track sizes and the `WM_SIZING` aspect lock | complete |
//! | Monitors: geometry, work area, per-monitor DPI, refresh, primary, hotplug | complete |
//! | Per-monitor-v2 DPI, and `WM_DPICHANGED` mid-session | complete |
//! | Message pump, and a blocking [`wait_events`](crate::Shell::wait_events) | complete |
//! | [`SurfaceTarget::Win32`](crcbl_core::SurfaceTarget::Win32) for the HAL | complete |
//! | Keyboard: scan codes, [`KeyCode`](crcbl_core::KeyCode), keysyms, modifiers, auto-repeat | complete — [`keys`] |
//! | Text: `WM_CHAR` with surrogate pairs, as [`TextCommit`](crate::ShellEvent::TextCommit) | complete, but **not** [`TEXT_IME`](crate::ShellCaps::TEXT_IME) — see [`caps`](Win32Shell::caps) |
//! | Pointer: motion, five buttons, enter/leave, capture, both wheel axes | complete — [`pointer`] |
//! | Raw relative motion, absolute devices included | complete — [`RAW_POINTER_MOTION`](crate::ShellCaps::RAW_POINTER_MOTION), latched on the registration |
//! | [`PointerMode`](crate::PointerMode) confine and lock, and [`warp_pointer`](crate::Shell::warp_pointer) | complete — [`POINTER_CONFINE`](crate::ShellCaps::POINTER_CONFINE), [`POINTER_LOCK`](crate::ShellCaps::POINTER_LOCK), [`POINTER_WARP`](crate::ShellCaps::POINTER_WARP) |
//! | Cursor shapes and hiding | complete — stock `IDC_*` cursors through `WM_SETCURSOR`, hiding through a balanced `ShowCursor` |
//! | Clipboard, both directions: `CF_UNICODETEXT` plus a registered format per mime | complete — [`CLIPBOARD`](crate::ShellCaps::CLIPBOARD), see [`clipboard`] |
//! | File drops in: `DragAcceptFiles`, `WM_DROPFILES`, the `accept_drops` gate | complete — [`DRAG_DROP`](crate::ShellCaps::DRAG_DROP), see [`dnd`] |
//! | Drag *feedback* — a drop cursor, hover highlighting, non-file formats | **not implemented**: it is `IDropTarget`, which is COM. [`dnd`] gives the argument |
//!
//! Two things input needs that no other area of this backend does are worth
//! finding here rather than in a call stack. [`ShellCaps::TEXT_IME`](crate::ShellCaps::TEXT_IME)
//! is deliberately **clear** although typing works — [`caps`](Win32Shell::caps)
//! gives the argument. And a [`DeviceId`](crcbl_core::input::DeviceId) is a
//! constant per device *kind* rather than per device, exactly as on X11; raw
//! input carries a per-device handle that a later slice can turn into a real id.
//!
//! # What Win32 does that neither Linux backend nor `HeadlessShell` models
//!
//! In the order they shaped the code:
//!
//! 1. **The window procedure is a callback the system calls re-entrantly, from
//!    inside our own calls.** On Wayland and X11 events arrive on a socket and
//!    are read when this crate asks. Here, `SetWindowPos`, `ShowWindow`,
//!    `SetWindowTextW` and `DestroyWindow` all dispatch messages *synchronously
//!    before they return*, so a [`Shell`](crate::Shell) method that calls one of them runs the
//!    window procedure inside itself, with `&mut self` already borrowed. That is
//!    why the procedure records rather than acts — see [`events`] — and it is
//!    the single fact that most shaped this backend.
//! 2. **A user drag-resize runs the system's message loop, not ours.** Between
//!    `WM_ENTERSIZEMOVE` and `WM_EXITSIZEMOVE` Windows spins a **modal** loop of
//!    its own; [`pump`](crate::Shell::pump) does not return for as long as the user
//!    holds the mouse button, so no frame is rendered and the window's contents
//!    are frozen for the length of the drag. See the section below — this
//!    backend *accepts* it, and that is a decision rather than an omission.
//! 3. **A window has a size before anyone asks**, as on X11 and unlike Wayland:
//!    `CreateWindowExW` takes a size and `GetClientRect` answers immediately.
//!    The P0.4 contract still holds and is still honest —
//!    [`WindowState::size`](crate::WindowState::size) is `None` until the first
//!    [`Resized`](crate::ShellEvent::Resized) — but the wait is **one
//!    [`pump`](crate::Shell::pump)** rather than a round trip. Nothing is delayed to
//!    look symmetrical with Wayland.
//! 4. **There is no window manager to refuse anything.** X11's
//!    `_NET_WM_STATE_FULLSCREEN` is a request to another process that may not
//!    be running; borderless here is *this* process changing its own window's
//!    style. So the effective mode is known when
//!    [`set_mode`](crate::Shell::set_mode) returns, and
//!    [`mode_request_honoured`](crate::WindowState::mode_request_honoured) is true as
//!    soon as the configuration that follows is published. The
//!    requested-versus-effective split in the seam is not redundant here — it
//!    still carries the monitor the window actually landed on — but this is the
//!    one backend where the answer is never "no".
//! 5. **DPI is per window, and changes while the window is open.** X11 has one
//!    global `Xft.dpi` for the whole desktop; Wayland has a per-surface scale
//!    that follows the output. Win32 has both a per-monitor DPI *and* a
//!    `WM_DPICHANGED` that arrives with a **suggested window rectangle** the
//!    application is expected to apply — dragging a window between a 100% and a
//!    150% monitor resizes it, and refusing to move it leaves it at the wrong
//!    size on the new monitor. The message is answered before the resize it
//!    causes is reported, which is what makes
//!    [`ScaleFactorChanged`](crate::ShellEvent::ScaleFactorChanged) precede the
//!    [`Resized`](crate::ShellEvent::Resized) rather than contradict it.
//! 6. **A minimized window genuinely reports 0×0.** `crate::geom` says so in as
//!    many words, and this is the backend it is about. A `WM_SIZE` carrying
//!    `SIZE_MINIMIZED` is treated as a *visibility* change and its zero extent
//!    is never published, so [`WindowState::size`](crate::WindowState::size) never becomes an extent no
//!    swapchain can be created at. [`visible`](crate::WindowState::visible) is defined
//!    as "mapped and not minimized" and this is where the second half comes
//!    from.
//! 7. **A window belongs to the thread that created it.** Messages are
//!    delivered to that thread's queue and `DestroyWindow` fails from any
//!    other, which is exactly the thread-affinity [`Shell`](crate::Shell) refuses to promise
//!    away by not being `Send`. Create the shell on the thread that will pump
//!    it — the crate documentation says so for this backend specifically.
//! 8. **The event clock is already the engine's clock, 32 bits of it.**
//!    `MSG::time` is a `GetTickCount` value and [`TimeBase`] reads
//!    `GetTickCount64`, so unlike X11 there is nothing to *calibrate*: the two
//!    are the same counter and widening the message's low 32 bits against the
//!    full-width reading is exact. The wrap is 49.7 days of system uptime,
//!    which a Windows desktop reaches routinely. A window procedure is never
//!    handed its `MSG`, though, so the value comes from `GetMessageTime` — the
//!    time of the message *currently being dispatched*, which is the whole
//!    point of a timestamp and not the moment the queue was drained.
//! 9. **There is no "the pointer arrived" message.** `wl_pointer.enter` and
//!    X11's `EnterNotify` both exist; Win32 has `WM_MOUSELEAVE` and nothing on
//!    the way in — and even the leave has to be *asked for*, one notification at
//!    a time, with `TrackMouseEvent`. So the entry half of
//!    [`PointerFocus`](crate::ShellEvent::PointerFocus) is derived from the
//!    first movement after a leave, in the window procedure, which is also where
//!    the next leave is armed.
//! 10. **The cursor is a desktop-wide resource, not a window property.** Its
//!     *clip* is one rectangle for the whole session and its *visibility* is a
//!     per-thread reference count — neither is scoped to a window by the API, so
//!     both have to be scoped by this backend. That is why losing focus releases
//!     the clip synchronously inside the window procedure (a process that keeps
//!     it has taken the desktop hostage) and why hiding goes through a balanced
//!     counter rather than a call per request. See [`input`].
//! 11. **Which key it was and what it produces come from two different fields of
//!     one message.** The scan code in `lParam` is the physical position and the
//!     virtual key in `wParam` is the layout's opinion; Wayland and X11 hand over
//!     one number and ask XKB for the rest. Reading [`KeyCode`](crcbl_core::KeyCode)
//!     out of the virtual key is the mistake that binds AZERTY's Z to
//!     `KeyCode::KeyW`'s neighbour instead of to `KeyW`.
//! 12. **The clipboard is content, not ownership.** X11 and Wayland both make
//!     one client the *owner* of a selection and have it serve the bytes on
//!     demand, which is why both backends carry a transfer state machine, a
//!     timeout and a payload they hold for as long as they own it.
//!     `SetClipboardData` takes the memory: the bytes live in the window
//!     station, there is no later conversation, and this shell keeps nothing.
//!     A read is `GetClipboardData` and is answered before
//!     [`clipboard_request`](crate::Shell::clipboard_request) returns — so
//!     [obligation 5](crate::Shell) has nothing to hold and
//!     [obligation 6](crate::Shell) has no rule to name. See [`clipboard`].
//! 13. **Opening the clipboard can fail because somebody else has it open**,
//!     which is routine rather than exceptional and has no analogue on either
//!     Linux backend — there, "another client owns the selection" is the normal
//!     state and costs nothing. It is retried with a bound rather than being
//!     reported as a failure the first time; [`clipboard`] argues that against
//!     [obligation 4](crate::Shell).
//!
//! # Decision: the modal resize loop is accepted, and here is what that costs
//!
//! While the user drags a window's edge or moves it by its title bar, Windows
//! runs a message loop inside `DefWindowProc`. Our `pump` is not on the stack;
//! it is *below* it, blocked in whichever call started the drag. The `WM_SIZE`
//! messages arrive at our window procedure normally and are queued normally —
//! and nothing drains that queue, because draining it is `pump`'s job and
//! `pump` has not returned.
//!
//! The consequences, exactly:
//!
//! * **No frame is rendered during the drag.** The window shows its last
//!   presented image, stretched or clipped by the system, until the mouse is
//!   released. Every engine with a `pump`-shaped loop has this; it is the
//!   visible symptom people describe as "the window goes white when I resize
//!   it", except that it does not go white here, because the class background
//!   brush is black and the last frame is what stays on screen.
//! * **The event flood is bounded rather than deferred.** [`events::enqueue`]
//!   makes a resize supersede a pending resize for the same window, so a
//!   three-second drag delivers exactly one [`Resized`](crate::ShellEvent::Resized) —
//!   carrying the size the window ended at, which is the only size any frame
//!   will ever be rendered at. Intermediate sizes are not information; they are
//!   the record of a frame that was never drawn.
//! * **Nothing else is delayed.** Input, close requests and DPI changes are
//!   physically impossible during a modal drag, because the user's hands are on
//!   the window edge.
//!
//! The fix everyone reaches for is `SetTimer` in `WM_ENTERSIZEMOVE` and a frame
//! rendered from `WM_TIMER`, inside the modal loop. **This backend cannot do
//! that**, and not for want of trying: rendering a frame means calling the
//! engine, and the shell has no callback to call. The crate documentation is
//! explicit that there is no `Shell::run(closure)` and never will be — a
//! framework-shaped `run()` compiles on wasm and deadlocks on the first frame —
//! so the only thing a timer could call back into is the `sink` of a `pump`
//! that is not running. Resolving that means a second seam ("render one frame
//! now"), which is a decision above this crate. `docs/backlog.md` carries it.
//!
//! # Decision: capabilities are latched, and almost nothing latches them
//!
//! [`caps`](crate::caps) requires a fixed value for the shell's lifetime, and
//! the two Linux backends compute one from what the server turned out to have:
//! an X server with no RandR, a compositor with no `wp_viewporter`. Windows has
//! almost no such variation above its version floor — every API this backend
//! uses is present or the process did not start — so the set is very nearly a
//! constant, computed once in [`Win32Shell::open`] and stated in
//! [`caps`](Win32Shell::caps).
//!
//! The one genuine latch is
//! [`RAW_POINTER_MOTION`](crate::ShellCaps::RAW_POINTER_MOTION), which follows
//! whether `RegisterRawInputDevices` was accepted — the only call here that can
//! be refused for a reason the version floor does not cover. What the set is
//! *not* is a wish list: every bit that is set is exercised by a test in this
//! module, and every bit that is clear is clear for a reason the method's
//! documentation gives.

pub mod clipboard;
pub mod dnd;
pub mod events;
pub mod geometry;
pub mod keys;
pub mod pointer;
pub mod proc;

// The ABI declarations are compiled on every host so that the structure sizes
// and the pure helpers over them are checked by `cargo test` here — see
// `lib.rs` — but only the Windows build has callers for most of them. The
// allowance is therefore scoped to the host build: on Windows an unused
// declaration is still an error, which is what keeps this module "audited by
// use" rather than a table of everything `winuser.h` has.
#[cfg_attr(not(target_os = "windows"), allow(dead_code))]
pub mod ffi;

#[cfg(target_os = "windows")]
mod input;
#[cfg(target_os = "windows")]
mod monitors;
#[cfg(target_os = "windows")]
mod shell;
#[cfg(target_os = "windows")]
mod window;

#[cfg(target_os = "windows")]
pub use shell::Win32Shell;

use core::time::Duration;

use crcbl_core::EventTime;

/// Puts `MSG::time` on the engine's clock.
///
/// # The easiest of the three native backends, and still not free
///
/// [`EventTime`] wants "a duration measured from the same origin as the
/// [`TimeSource`](crcbl_core::TimeSource) driving
/// [`FrameClock::update`](crcbl_core::FrameClock::update)". The three platforms
/// are three different problems:
///
/// * **Wayland** stamps with `CLOCK_MONOTONIC` milliseconds — the right clock,
///   the wrong zero.
/// * **X11** stamps with milliseconds since the *server* started, which is a
///   clock this process has never read and may not even be on this machine, so
///   the offset has to be calibrated from the first event that arrives.
/// * **Win32** stamps with `GetTickCount` — milliseconds since this system
///   booted — and `GetTickCount64` reads the same counter at full width. So
///   there is nothing to calibrate and nothing to guess: a message's low 32
///   bits plus the high bits of a reading taken now *is* the message's time,
///   exactly.
///
/// What is left is the wrap, and it is not hypothetical: 2³² milliseconds is
/// 49.7 days of uptime, which a desktop that is only ever slept reaches
/// routinely. [`widen`](Self::widen) resolves it against the full-width
/// reading rather than against a high-water mark, which is stronger than the
/// X11 backend's equivalent — there, no wider clock exists to compare against.
///
/// The 32 bits come from `GetMessageTime` rather than from a `MSG`: a window
/// procedure is called with four arguments and none of them is the message's
/// timestamp. See the [module docs](self).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct TimeBase {
    /// `GetTickCount64` nanoseconds at the engine epoch.
    epoch_nanos: u64,
}

impl TimeBase {
    /// An epoch of `now_nanos`, which at [`open`](Win32Shell::open) is the
    /// shell's creation.
    #[must_use]
    pub const fn at(now_nanos: u64) -> Self {
        Self {
            epoch_nanos: now_nanos,
        }
    }

    /// Moves the epoch so that `now_nanos` reads as `elapsed`.
    ///
    /// See [`Shell::align_event_clock`](crate::Shell::align_event_clock), which this implements: a shell is
    /// created some time *after* the engine clock starts, and only the caller
    /// knows how long.
    pub const fn align_at(&mut self, now_nanos: u64, elapsed_nanos: u64) {
        self.epoch_nanos = now_nanos.saturating_sub(elapsed_nanos);
    }

    /// A 32-bit tick count widened against a full-width reading of the same
    /// counter.
    ///
    /// Pure, so the 49.7-day wrap is testable without waiting seven weeks. A
    /// message is stamped when it is *posted*, so its time is never in the
    /// future; a candidate that lands ahead of `now_millis` by more than half
    /// the range is therefore the previous wrap, not a clock that ran
    /// backwards. Half the range rather than zero, so that a message posted a
    /// millisecond ago and a reading taken now do not read as 49 days apart.
    #[must_use]
    pub const fn widen(now_millis: u64, message_millis: u32) -> u64 {
        const WRAP: u64 = 1 << 32;
        let candidate = (now_millis & !(WRAP - 1)) | (message_millis as u64);
        if candidate > now_millis + WRAP / 2 {
            candidate - WRAP
        } else {
            candidate
        }
    }

    /// This base applied to a `MSG::time`, with the clock passed in.
    ///
    /// Split from the reading of the clock for the reason `crcbl-core`'s
    /// `TimeSource` gives: nothing reads a clock where a test needs to drive
    /// one, and the wrap is not something a test can arrange by waiting.
    #[must_use]
    pub fn event_time_at(self, now_nanos: u64, message_millis: u32) -> EventTime {
        let widened = Self::widen(now_nanos / 1_000_000, message_millis);
        let nanos = widened.saturating_mul(1_000_000);
        // An event before the epoch reads as the epoch: `EventTime` is a
        // duration since it and there is no negative one. Reachable after an
        // `align_event_clock` that moves the epoch past a message already in
        // the queue.
        EventTime::from_duration(Duration::from_nanos(nanos.saturating_sub(self.epoch_nanos)))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Nanoseconds from whole milliseconds, for readable fixtures.
    const fn ms(millis: u64) -> u64 {
        millis * 1_000_000
    }

    #[test]
    fn a_message_is_stamped_against_the_epoch_with_no_calibration_step() {
        // The Win32 difference from X11: the first event needs no special
        // handling, because the message clock and the shell's clock are the
        // same counter.
        let base = TimeBase::at(ms(9_000_000));
        let stamped = base.event_time_at(ms(9_003_000), 9_003_000);
        assert_eq!(stamped.as_duration(), Duration::from_secs(3));

        // And the one after it is exactly 180 ms later, which is what the P2
        // tap-versus-hold evaluation subtracts.
        let later = base.event_time_at(ms(9_003_200), 9_003_180);
        assert_eq!(
            later.saturating_since(stamped),
            Duration::from_millis(180),
            "the epoch cancels"
        );
    }

    #[test]
    fn a_thirty_two_bit_tick_wrap_is_resolved_against_the_full_width_clock() {
        const WRAP: u64 = 1 << 32;
        // 49.7 days of uptime, plus a second. A message posted 100 ms before
        // the wrap is still on the old side of it.
        let now = WRAP + 1_000;
        assert_eq!(TimeBase::widen(now, 500), WRAP + 500, "after the wrap");
        assert_eq!(
            TimeBase::widen(now, (WRAP - 100) as u32),
            WRAP - 100,
            "a message from just before the wrap is not read as 49 days early"
        );
        // Time never goes backwards across the boundary.
        assert!(TimeBase::widen(now, 500) > TimeBase::widen(now, (WRAP - 100) as u32));

        // Before any wrap has happened the low bits are the whole answer.
        assert_eq!(TimeBase::widen(10_000, 9_990), 9_990);
        assert_eq!(TimeBase::widen(10_000, 10_000), 10_000);
    }

    #[test]
    fn a_message_from_before_the_epoch_reads_as_the_epoch() {
        // `align_event_clock` can move the epoch forward past a message
        // already in the queue. `EventTime` is a duration since the epoch, so
        // clamping is the only honest answer — the same one the other two
        // backends give.
        let base = TimeBase::at(ms(10_000));
        assert_eq!(
            base.event_time_at(ms(10_000), 9_000).as_duration(),
            Duration::ZERO
        );
    }

    #[test]
    fn aligning_makes_now_read_as_the_engines_elapsed_time() {
        // The shell was created 40 ms into the engine's run, so a message
        // stamped "now" must read as 40 ms rather than as the system's uptime.
        let mut base = TimeBase::at(ms(500_000));
        base.align_at(ms(500_000), ms(40));
        assert_eq!(
            base.event_time_at(ms(500_000), 500_000).as_duration(),
            Duration::from_millis(40)
        );
        // An engine clock older than the system's uptime cannot happen, and
        // saturating rather than panicking is what a backend does about a
        // number it did not compute.
        base.align_at(ms(1_000), ms(9_999_999));
        assert_eq!(base.epoch_nanos, 0);
    }
}
