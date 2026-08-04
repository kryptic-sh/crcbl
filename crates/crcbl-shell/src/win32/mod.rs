//! The Win32 backend: window class, message pump, display modes, size
//! constraints, monitors and per-monitor DPI.
//!
//! `docs/plan/15-windowing.md`'s Windows row, which reads in full: "hand-written
//! Win32 FFI (`extern "system"` decls for the surface we use)". Every
//! declaration in [`ffi`] is ours; there is no `windows-rs`, no `winapi`, no
//! framework, and — unlike the two Linux backends — no `dlopen`, for the reason
//! that module states at length.
//!
//! # What is in this slice, and what is not
//!
//! This is **P5C W1, the window lifecycle half.** Input, the clipboard and
//! drag-and-drop are W2 and W3, and [`ShellCaps`](crate::ShellCaps) says so today rather than
//! promising them:
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
//! | **Keyboard, pointer, wheel, text, cursors, pointer capture** | **W2** — [`POINTER_LOCK`](crate::ShellCaps::POINTER_LOCK), [`POINTER_WARP`](crate::ShellCaps::POINTER_WARP), [`RAW_POINTER_MOTION`](crate::ShellCaps::RAW_POINTER_MOTION) and [`TEXT_IME`](crate::ShellCaps::TEXT_IME) are clear |
//! | **Clipboard and drag-and-drop** | **W3** — [`CLIPBOARD`](crate::ShellCaps::CLIPBOARD) and [`DRAG_DROP`](crate::ShellCaps::DRAG_DROP) are clear |
//!
//! [`set_cursor`](crate::Shell::set_cursor) is the one method in between: it **records**
//! the request and does not apply it, exactly as the X11 backend records a
//! shape it cannot load a theme for. [`WindowState`](crate::WindowState) can report what was asked
//! and nothing on screen changes until W2. That is stated here, in the method's
//! own documentation and in `docs/backlog.md`, because a recorded-and-ignored
//! request that is not written down is indistinguishable from a bug.
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
//!    which a Windows desktop reaches routinely.
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
//! # Decision: capabilities are latched, and there is nothing to latch them
//! *from*
//!
//! [`caps`](crate::caps) requires a fixed value for the shell's lifetime, and
//! the two Linux backends compute one from what the server turned out to have:
//! an X server with no RandR, a compositor with no `wp_viewporter`. Windows has
//! no such variation above its version floor — every API this backend uses is
//! present or the process did not start — so the set is a constant, computed
//! once in [`Win32Shell::open`] and stated in
//! [`caps`](Win32Shell::caps). What it is *not* is a wish list: every bit that
//! is set is exercised by a test in this module, and every bit that is clear is
//! clear because the code for it is in W2 or W3.

pub mod events;
pub mod geometry;
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
/// Input events land in W2. This type is here now because the alternative is
/// discovering at W2 that [`align_event_clock`](crate::Shell::align_event_clock) has
/// nowhere to write, and because the wrap is testable today.
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
    // W2 delivers the first input event; nothing in the window lifecycle
    // carries a timestamp, because `event`'s state transitions deliberately do
    // not have one.
    #[allow(dead_code, reason = "W2 is the first caller; the wrap is tested now")]
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
