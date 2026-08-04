//! The Win32 backend against a real Windows desktop.
//!
//! Run with `crates/crcbl-shell/tests/run-win32-e2e.ps1`, which turns these on
//! and fails when the suite reports zero tests run — `docs/plan/12-testing.md`
//! calls a silently-skipped e2e job a known trap, and this is the
//! counter-measure. Like the Wayland and X11 suites they are gated twice, behind
//! the `win32-e2e` feature *and* `#[ignore]`, so that
//! `cargo nextest run --workspace --all-features` stays green everywhere else.
//!
//! # What these are for, over the suite that already exists
//!
//! `crcbl-shell`'s in-crate Windows tests reach the **real** window procedure
//! with `SendMessageW`, against the real cached keyboard state, the real
//! `MapVirtualKeyW`, the real `DragQueryFileW`. What they cannot reach is
//! anything that happens *between* processes or *in* the message queue, and that
//! is the whole of what is here:
//!
//! * **Input as a real message stream.** `SendInput` injects into the session's
//!   input stream and the stream follows the foreground window, so the sender
//!   has to be somewhere else — `tests/bin/send_input_win32.rs`, the Windows
//!   counterpart of the two Linux key senders. A message that arrives this way
//!   was posted, queued, translated and dispatched; one sent with `SendMessageW`
//!   was none of those things. The first run of this suite is what found
//!   `TranslateMessage` missing from the pump.
//! * **Mode flips and resize storms through the seam**, with the *system's* idea
//!   of the window rectangle as the judge rather than the backend's.
//! * **Monitors, DPI and focus** against the desktop the runner actually has.
//! * **A clipboard round trip between two processes**, which is the only
//!   arrangement that can tell "the window station has the bytes" from "the
//!   shell answered its own read out of a cache".
//!
//! Every test goes through [`crcbl_shell::open_backend`] and `dyn Shell`; no
//! backend type is named anywhere below, exactly as a consumer would have it.
//!
//! # What is deliberately not here
//!
//! **The sample-level pass.** The Linux suites drive a running game and press
//! F11 at it; that needs a renderer, and `crcbl-vk` has no software device on a
//! `windows-latest` runner. `docs/plan/ROADMAP.md`'s 2026-08-04 correction says
//! so in as many words and schedules it for P14. It is recorded in
//! `docs/backlog.md` as the gap it is rather than approximated here.
//!
//! Also absent, each for a reason `docs/backlog.md` carries: a **real** drag and
//! drop (the shell allocates the `HDROP` in the target's own context, so no test
//! process can hand one over), the `WM_IME_*` family, absolute raw reports
//! (which need a remote-desktop session or a tablet), and multi-monitor
//! behaviour (the runner has one display).
//!
//! # Three facts about the runner that shaped every test below
//!
//! * **The desktop is 1024×768.** Anything that assumes a default-sized window
//!   fits on it is wrong there, which is why the windows here are small and why
//!   nothing compares a clip rectangle against an unclamped client area.
//! * **The desktop is not idle.** A real cursor sits over the window, messages
//!   arrive that this process did not cause, and the foreground is something
//!   before this suite asks for it. Three CI round trips were spent learning
//!   that one assertion at a time, so nothing below assumes its own event is the
//!   first one, and every wait is a poll with a deadline.
//! * **The foreground is locked, and asking for it politely does not work.**
//!   `SetForegroundWindow` is granted to a process that already has the
//!   foreground or received the last input event, and under `nextest` every test
//!   is a fresh process that has neither. The first run of this suite lost three
//!   tests to twenty seconds each of being refused by the job's own console
//!   window. What defeats it is in [`desktop::take_foreground`], it is in this
//!   harness rather than in the backend, and that placement is argued there.
//!
//! # Nothing here has been run
//!
//! This suite was written on Linux, where it does not compile and cannot
//! execute. Every claim it makes about Windows is unverified until the runner
//! reports; the failure messages are written on that assumption, and carry the
//! queue word, the rectangle, the foreground handle and the helper's own output
//! rather than a duration.

#![cfg(all(target_os = "windows", feature = "win32-e2e"))]

use std::io::{BufRead, BufReader, Write};
use std::process::{Child, ChildStdin, Command, Stdio};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use crcbl_shell::{
    ButtonState, ClipboardContent, ClipboardOffer, CursorIcon, DisplayMode, KeyCode, Keysym,
    LogicalSize, MimeType, PhysicalPoint, PhysicalRect, PointerButton, PointerMode, ScrollDelta,
    Shell, ShellBackend, ShellCaps, ShellError, ShellEvent, SurfaceTarget, WindowDesc, WindowId,
};

/// PS/2 set 1 scan codes, spelled as the sender takes them and as
/// `win32::keys::scancode` reports them.
mod scancode {
    /// `A`.
    pub const A: u32 = 0x1E;
    /// `ArrowUp`, which is `E0`-prefixed — the identity, not a flag beside it.
    pub const ARROW_UP: u32 = 0xE048;
}

/// How long any single wait may take before the test fails.
///
/// Generous on purpose. A `windows-latest` runner shares its cores and is
/// roughly half the speed of a Linux one, and a deadline tight enough to notice
/// that is a deadline that reports the runner rather than the backend. Bounded
/// all the same, and bounded *here*: nextest reports a test as SLOW at 60s and
/// kills it at 4× that, so a wait that expires inside this file fails with a
/// message naming what never happened, while one left to nextest is a SIGKILL
/// with no context at all.
const WAIT: Duration = Duration::from_secs(20);

/// How long [`Session::foreground`] may go on asking before the desktop is
/// judged to be refusing.
///
/// Deliberately a fraction of [`WAIT`]: nothing is being waited *for* here — the
/// window station answers the request on the turn it is made — so the only thing
/// a long deadline buys is a window that is still being shown, and the only
/// thing it costs is a CI job spending a minute repeating one refusal. See
/// [`Session::foreground`], which is where that bill was paid.
const FOREGROUND_WAIT: Duration = Duration::from_secs(4);

/// The most any single [`Session::pump`] may take before the frame loop is
/// judged to have blocked.
///
/// **Not a performance budget**, and it cannot be one — this process can be
/// descheduled mid-pump for as long as the scheduler likes. What it catches is
/// the failure it was written for: a clipboard read that waited on another
/// process is not late by a scheduling quantum, it is late by a whole
/// conversation. On Win32 there is no conversation to have, and that is exactly
/// the claim.
const SLOWEST_PUMP: Duration = Duration::from_millis(1_500);

/// The window size every test here asks for.
///
/// Deliberately small: the runner's desktop is 1024×768, so the default
/// 1280×720 does not fit on it and a borderless flip would have nowhere to go
/// that is bigger than where it started.
const SIZE: LogicalSize = LogicalSize::new(400.0, 300.0);

/// The `user32` surface the *suite* needs, as distinct from the backend's.
///
/// Hand-written here rather than borrowed from `crcbl_shell::win32::ffi`, which
/// is `pub(crate)` — and rightly so. A test that judged the backend by the
/// backend's own ABI table would be grading its own homework; these are the
/// calls an outside observer makes about somebody else's window.
///
/// Every entry point is wrapped in a safe function, so that the `unsafe` is in
/// one place with its invariant beside it and the tests below read as tests.
mod desktop {
    use core::ffi::c_void;

    /// `HWND`.
    pub type Handle = *mut c_void;

    /// `RECT` — two corners, not an origin and a size.
    #[repr(C)]
    #[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
    pub struct Rect {
        /// Left edge.
        pub left: i32,
        /// Top edge.
        pub top: i32,
        /// One past the right edge.
        pub right: i32,
        /// One past the bottom edge.
        pub bottom: i32,
    }

    impl Rect {
        /// Width in pixels, never negative.
        #[must_use]
        pub const fn width(self) -> i32 {
            if self.right > self.left {
                self.right - self.left
            } else {
                0
            }
        }

        /// Height in pixels, never negative.
        #[must_use]
        pub const fn height(self) -> i32 {
            if self.bottom > self.top {
                self.bottom - self.top
            } else {
                0
            }
        }
    }

    /// `POINT`.
    #[repr(C)]
    #[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
    pub struct Point {
        /// Horizontal coordinate.
        pub x: i32,
        /// Vertical coordinate.
        pub y: i32,
    }

    /// `SM_XVIRTUALSCREEN`.
    const SM_X_VIRTUAL_SCREEN: i32 = 76;
    /// `SM_YVIRTUALSCREEN`.
    const SM_Y_VIRTUAL_SCREEN: i32 = 77;
    /// `SM_CXVIRTUALSCREEN`.
    const SM_CX_VIRTUAL_SCREEN: i32 = 78;
    /// `SM_CYVIRTUALSCREEN`.
    const SM_CY_VIRTUAL_SCREEN: i32 = 79;

    /// `SWP_NOMOVE`.
    const SWP_NO_MOVE: u32 = 0x0002;
    /// `SWP_NOZORDER`.
    const SWP_NO_Z_ORDER: u32 = 0x0004;
    /// `SWP_NOACTIVATE` — resize it without stealing the foreground, so a resize
    /// in one test cannot decide the focus in the next.
    const SWP_NO_ACTIVATE: u32 = 0x0010;

    /// `QS_ALLINPUT`.
    const QS_ALL_INPUT: u32 = 0x04FF;

    /// `SPI_GETFOREGROUNDLOCKTIMEOUT`.
    const SPI_GET_FOREGROUND_LOCK_TIMEOUT: u32 = 0x2000;
    /// `SPI_SETFOREGROUNDLOCKTIMEOUT`.
    const SPI_SET_FOREGROUND_LOCK_TIMEOUT: u32 = 0x2001;

    /// `USER_DEFAULT_SCREEN_DPI` — the DPI that is 100%.
    pub const DEFAULT_DPI: u32 = 96;

    #[link(name = "user32")]
    unsafe extern "system" {
        fn SetForegroundWindow(hwnd: Handle) -> i32;
        fn GetForegroundWindow() -> Handle;
        fn GetWindowRect(hwnd: Handle, rect: *mut Rect) -> i32;
        fn GetClientRect(hwnd: Handle, rect: *mut Rect) -> i32;
        fn SetWindowPos(
            hwnd: Handle,
            after: Handle,
            x: i32,
            y: i32,
            width: i32,
            height: i32,
            flags: u32,
        ) -> i32;
        fn GetSystemMetrics(index: i32) -> i32;
        fn GetDpiForWindow(hwnd: Handle) -> u32;
        fn GetQueueStatus(flags: u32) -> u32;
        fn GetCursorPos(point: *mut Point) -> i32;
        fn SystemParametersInfoW(action: u32, param: u32, value: *mut c_void, ini: u32) -> i32;
        fn GetWindowThreadProcessId(hwnd: Handle, process: *mut u32) -> u32;
        fn AttachThreadInput(attach: u32, attach_to: u32, join: i32) -> i32;
        fn BringWindowToTop(hwnd: Handle) -> i32;
        fn SetFocus(hwnd: Handle) -> Handle;
        fn GetClassNameW(hwnd: Handle, buffer: *mut u16, capacity: i32) -> i32;
        fn GetWindowTextW(hwnd: Handle, buffer: *mut u16, capacity: i32) -> i32;
    }

    #[link(name = "kernel32")]
    unsafe extern "system" {
        fn GetCurrentThreadId() -> u32;
        fn GetCurrentProcessId() -> u32;
    }

    /// Takes the foreground the way a test harness has to, and answers whether
    /// this window now has it.
    ///
    /// # The foreground lock, and why a bare `SetForegroundWindow` is not enough
    ///
    /// The first CI run of this suite failed three tests identically: twenty
    /// seconds of asking, a drained queue, and the foreground still on `0x10200`
    /// — a window belonging to the job's own console rather than to any of these
    /// tests. That is **Windows' foreground lock** doing exactly what it is for.
    /// `SetForegroundWindow` is granted only to a process that already owns the
    /// foreground, was started by the process that does, received the last input
    /// event, or is asking after the lock timeout has expired with no user input
    /// at all. Under `nextest` every test is its own short-lived process running
    /// serially, so the *first* one to ask may well qualify — the job had been
    /// compiling silently for minutes, so the timeout had expired — and every
    /// later one is asking a few seconds after the previous test injected input,
    /// which restarts that clock and hands the qualification to nobody.
    ///
    /// # The two levers, and both are pulled
    ///
    /// * [`unlock_foreground`] sets the lock timeout to zero, which removes the
    ///   "following user input" window the grant is refused inside of. It is
    ///   itself refusable, which is why it is not the only lever.
    /// * **`AttachThreadInput` against the thread that owns the current
    ///   foreground window.** Attaching merges the two threads' input queues, so
    ///   for the length of the attachment this thread *is* part of the
    ///   foreground queue and the grant is not a request. `BringWindowToTop`
    ///   raises the window out from under whatever was covering it and `SetFocus`
    ///   moves the keyboard within the shared queue, which is the state that
    ///   survives the detach.
    ///
    /// The attachment is held across three calls and no longer. Attaching to a
    /// thread that has stopped pumping is a documented way to hang, and the
    /// runner's console is a thread this suite does not control.
    ///
    /// Neither lever is in `src/win32/`, and that is deliberate: **a game does
    /// not get to steal the focus.** Everything here is what an automated
    /// *harness* does to arrange a precondition a human would have arranged by
    /// clicking, and a backend that knew how to do it would be a backend that
    /// could do it to a user.
    ///
    /// Called once per turn of a poll rather than once, because a single refusal
    /// must not decide a test, and judged by [`foreground_window`] rather than by
    /// any return value.
    #[must_use]
    pub fn take_foreground(hwnd: Handle) -> bool {
        let front = foreground_window();
        if front == hwnd {
            return true;
        }
        // SAFETY: a window handle by value and a null out-pointer, which the
        // call documents as "do not report the process".
        let theirs = unsafe { GetWindowThreadProcessId(front, core::ptr::null_mut()) };
        // SAFETY: reads the calling thread's own id and cannot fail.
        let ours = unsafe { GetCurrentThreadId() };
        // SAFETY: two thread ids by value. Attaching this thread to the
        // foreground one shares their input state; it is undone below on every
        // path, and is skipped entirely when there is no other thread to attach
        // to.
        let attached =
            theirs != 0 && theirs != ours && unsafe { AttachThreadInput(ours, theirs, 1) } != 0;
        // SAFETY: three window-station calls taking this process's own window by
        // value. None reads memory of ours.
        unsafe {
            BringWindowToTop(hwnd);
            SetForegroundWindow(hwnd);
            SetFocus(hwnd);
        }
        if attached {
            // SAFETY: the same two ids, undoing the attachment made above.
            unsafe { AttachThreadInput(ours, theirs, 0) };
        }
        foreground_window() == hwnd
    }

    /// Sets the foreground lock timeout, answering the value that was there
    /// before so a caller can put it back.
    ///
    /// `fWinIni` is zero on purpose: the change is wanted for this session and
    /// not written into the user's profile or broadcast to every window on the
    /// desktop. A CI runner is thrown away afterwards; a developer running this
    /// suite on their own machine is not, and [`Session`](super::Session) puts
    /// the old value back on the way out.
    ///
    /// `None` means the system refused to say what the old value was, which is
    /// itself worth printing — the set is documented as being available only to
    /// a process that could already take the foreground, so a refusal here is
    /// the first thing to look at when [`take_foreground`] never takes.
    pub fn unlock_foreground(timeout_ms: u32) -> Option<u32> {
        let mut previous: u32 = 0;
        // SAFETY: `previous` is a live, initialised `DWORD` and this action
        // documents `pvParam` as pointing at one. `uiParam` must be zero.
        let read = unsafe {
            SystemParametersInfoW(
                SPI_GET_FOREGROUND_LOCK_TIMEOUT,
                0,
                (&raw mut previous).cast::<c_void>(),
                0,
            )
        };
        // SAFETY: the *set* action takes the new value **in** `pvParam` rather
        // than through it — the pointer is never dereferenced — which is why
        // this one is a provenance-free integer and not a borrow.
        unsafe {
            SystemParametersInfoW(
                SPI_SET_FOREGROUND_LOCK_TIMEOUT,
                0,
                core::ptr::without_provenance_mut(timeout_ms as usize),
                0,
            )
        };
        (read != 0).then_some(previous)
    }

    /// The foreground lock timeout the system reports right now, in
    /// milliseconds.
    ///
    /// Only ever read into a failure message, and it is the one number that says
    /// whether the lock is still the reason: a zero here with the foreground
    /// still somewhere else means the refusal is not the timeout, and every
    /// other value means [`unlock_foreground`] was refused.
    #[must_use]
    pub fn foreground_lock_timeout() -> Option<u32> {
        let mut timeout: u32 = 0;
        // SAFETY: as in `unlock_foreground`'s read above.
        let read = unsafe {
            SystemParametersInfoW(
                SPI_GET_FOREGROUND_LOCK_TIMEOUT,
                0,
                (&raw mut timeout).cast::<c_void>(),
                0,
            )
        };
        (read != 0).then_some(timeout)
    }

    /// A window named the way a person reading a CI log can act on.
    ///
    /// The handle alone is what made the first failure diagnosable in one round
    /// — `0x10200` was the same window every time and was not ours — and it is
    /// still printed. What it could not say is *whose* window it is, which took
    /// a guess; the class and the title say it outright, and the thread and
    /// process ids say whether it belongs to this process at all.
    #[must_use]
    pub fn describe(hwnd: Handle) -> String {
        if hwnd.is_null() {
            return "0x0 (there is no foreground window at all)".to_owned();
        }
        let mut process: u32 = 0;
        // SAFETY: a window handle by value and a live `DWORD` the call writes
        // the owning process id into.
        let thread = unsafe { GetWindowThreadProcessId(hwnd, &raw mut process) };
        format!(
            "{hwnd:?} class {:?} title {:?} thread {thread} process {process}",
            // SAFETY of both: each call writes at most `capacity` UTF-16 units
            // into a buffer of that length and answers how many it wrote.
            text(|buffer, capacity| unsafe { GetClassNameW(hwnd, buffer, capacity) }),
            text(|buffer, capacity| unsafe { GetWindowTextW(hwnd, buffer, capacity) }),
        )
    }

    /// This process, for comparing against [`describe`]'s answer.
    #[must_use]
    pub fn ours() -> String {
        // SAFETY: both read the caller's own identity and cannot fail.
        unsafe {
            format!(
                "thread {} process {}",
                GetCurrentThreadId(),
                GetCurrentProcessId()
            )
        }
    }

    /// Runs a `GetClassNameW`-shaped call and decodes what it wrote.
    fn text(fill: impl FnOnce(*mut u16, i32) -> i32) -> String {
        /// Longer than any window class or a title worth printing.
        const CAPACITY: usize = 256;
        let mut buffer = [0u16; CAPACITY];
        let written = fill(
            buffer.as_mut_ptr(),
            i32::try_from(CAPACITY).expect("a small constant"),
        );
        let written = usize::try_from(written).unwrap_or(0).min(CAPACITY);
        String::from_utf16_lossy(&buffer[..written])
    }

    /// The window the session's input stream is currently pointed at.
    #[must_use]
    pub fn foreground_window() -> Handle {
        // SAFETY: a handle by value; the call only reads window-station state.
        unsafe { GetForegroundWindow() }
    }

    /// The window's outer rectangle, frame included, in desktop coordinates.
    #[must_use]
    pub fn window_rect(hwnd: Handle) -> Rect {
        let mut rect = Rect::default();
        // SAFETY: `rect` is a live, initialised `RECT` the call writes into.
        unsafe { GetWindowRect(hwnd, &raw mut rect) };
        rect
    }

    /// The window's client rectangle, which is always at the origin.
    #[must_use]
    pub fn client_rect(hwnd: Handle) -> Rect {
        let mut rect = Rect::default();
        // SAFETY: `rect` is a live, initialised `RECT` the call writes into.
        unsafe { GetClientRect(hwnd, &raw mut rect) };
        rect
    }

    /// Resizes a window from outside the shell, without moving or activating it.
    pub fn resize(hwnd: Handle, width: i32, height: i32) {
        // SAFETY: a window of this process, from the thread that created it.
        // `SWP_NOMOVE` makes the position arguments unused and the null
        // insert-after handle is unused under `SWP_NOZORDER`.
        unsafe {
            SetWindowPos(
                hwnd,
                core::ptr::null_mut(),
                0,
                0,
                width,
                height,
                SWP_NO_MOVE | SWP_NO_Z_ORDER | SWP_NO_ACTIVATE,
            )
        };
    }

    /// The whole desktop as one rectangle.
    ///
    /// The origin is read rather than assumed to be zero: a monitor to the left
    /// of the primary puts the virtual screen's left edge at a negative
    /// coordinate, and a test that assumed the origin would pass on a runner
    /// with one display and fail on a developer's desk.
    #[must_use]
    pub fn virtual_screen() -> Rect {
        // SAFETY: four metric queries by value; each reads no memory of ours and
        // answers zero for an index it does not know.
        unsafe {
            let left = GetSystemMetrics(SM_X_VIRTUAL_SCREEN);
            let top = GetSystemMetrics(SM_Y_VIRTUAL_SCREEN);
            Rect {
                left,
                top,
                right: left + GetSystemMetrics(SM_CX_VIRTUAL_SCREEN),
                bottom: top + GetSystemMetrics(SM_CY_VIRTUAL_SCREEN),
            }
        }
    }

    /// The window's DPI, as the system reports it to anyone who asks.
    #[must_use]
    pub fn dpi(hwnd: Handle) -> u32 {
        // SAFETY: a window handle by value; the call only reads.
        unsafe { GetDpiForWindow(hwnd) }
    }

    /// Which kinds of message this thread's queue holds, as `QS_*` bits.
    ///
    /// Only ever read into a failure message. A test that timed out waiting for
    /// an injected keystroke and printed only a duration costs a whole CI round
    /// trip; one that prints the queue word says whether anything arrived at all.
    #[must_use]
    pub fn queue_status() -> u32 {
        // SAFETY: reading the calling thread's own queue state. The call takes
        // no pointers and has no failure mode.
        unsafe { GetQueueStatus(QS_ALL_INPUT) }
    }

    /// Where the cursor is, in desktop coordinates.
    #[must_use]
    pub fn cursor_position() -> Point {
        let mut point = Point::default();
        // SAFETY: `point` is a live, initialised `POINT` the call writes into.
        unsafe { GetCursorPos(&raw mut point) };
        point
    }
}

// ---------------------------------------------------------------------------
// The session
// ---------------------------------------------------------------------------

/// A shell and everything it has produced.
struct Session {
    shell: Box<dyn Shell>,
    events: Vec<ShellEvent>,
    /// Every window [`window`](Self::window) created, so [`Drop`] can destroy
    /// them.
    ///
    /// A process that exits with windows still up leaves the desktop's idea of
    /// the foreground pointing at handles that are gone, and the next test's
    /// process then cannot take it. That is the Windows shape of the tail of
    /// failures the X11 suite had — each test passing alone and failing in a
    /// full run — and the cure is the same: put everything back.
    windows: Vec<WindowId>,
    /// The longest any single [`pump`](Self::pump) has taken.
    slowest_pump: Duration,
    /// How many times [`pump`](Self::pump) has been called.
    ///
    /// The load-independent half of the same question. Win32's clipboard is
    /// content rather than ownership, so a read is answered *inside*
    /// `clipboard_request` and the answer is delivered on the very next pump —
    /// a count of one, on an idle machine and on a runner with nothing left to
    /// give alike.
    pumps: u32,
    /// The foreground lock timeout as it was before this session lowered it, to
    /// be put back on the way out.
    ///
    /// The setting is **session-global** rather than per-process, so it outlives
    /// the test that changed it: on a CI runner that is free, and on a
    /// developer's own desktop it is their machine's focus-stealing protection
    /// left switched off by a test suite. `None` means the system would not say
    /// what the old value was, and then there is nothing to restore.
    previous_lock_timeout: Option<u32>,
}

impl Session {
    fn open() -> Self {
        let shell = crcbl_shell::open_backend(ShellBackend::Win32).expect(
            "opening the Win32 shell needs a usable window station; a failure here is the \
             runner's answer to whether it has one",
        );
        assert_eq!(shell.backend(), ShellBackend::Win32);
        Self {
            shell,
            events: Vec::new(),
            windows: Vec::new(),
            slowest_pump: Duration::ZERO,
            pumps: 0,
            // Lowered here rather than inside `foreground`, because the lock is
            // a property of the *session* and not of a window: doing it once per
            // process is enough, and doing it before any window exists means the
            // very first request is already made under the relaxed rule. See
            // `desktop::take_foreground` for what the lock is and why this is
            // only one of the two levers against it.
            previous_lock_timeout: desktop::unlock_foreground(0),
        }
    }

    /// One turn of the loop.
    fn pump(&mut self) {
        let started = Instant::now();
        let events = &mut self.events;
        self.shell.pump(&mut |event| events.push(event));
        self.slowest_pump = self.slowest_pump.max(started.elapsed());
        self.pumps += 1;
    }

    /// Pumps until `ready`, or fails naming what never happened.
    ///
    /// A deadline and a poll, never a fixed sleep, which
    /// `docs/plan/12-testing.md` makes the rule for anything asynchronous. Here
    /// the asynchronous thing is usually *another process* — a helper injecting
    /// input, or the desktop deciding who is in front — which is the case the
    /// rule was written for.
    fn pump_until(&mut self, what: &str, mut ready: impl FnMut(&mut Self) -> bool) {
        let deadline = Instant::now() + WAIT;
        loop {
            self.pump();
            if ready(self) {
                return;
            }
            assert!(
                Instant::now() < deadline,
                "timed out after {WAIT:?} waiting for {what}; the queue holds {:#06x}, the \
                 foreground window is {}, we are {}, and the events so far are {:?}",
                desktop::queue_status(),
                desktop::describe(desktop::foreground_window()),
                desktop::ours(),
                self.names()
            );
            // Also exercises `ShellCaps::EVENT_WAIT`: a backend that claimed it
            // and did not block would spin this loop, and one that blocked
            // forever would never reach the deadline check.
            self.shell.wait_events(Some(Duration::from_millis(10)));
        }
    }

    /// Pumps until the desktop has stopped producing events.
    ///
    /// **The runner's desktop is not idle.** Showing a window under a real
    /// cursor produces motion, the system decides the foreground on its own
    /// schedule, and a window that has just been created is still being
    /// configured. A test that started measuring at the first configure counts
    /// all of that as its own result.
    ///
    /// There is no message that says "the desktop has finished", so quiet is the
    /// only available definition, and it fails rather than returning when the
    /// desktop never goes quiet — a desktop that never stops talking is a
    /// finding, not something to wait out.
    fn settle(&mut self) {
        /// Consecutive silent pumps that count as quiet. Each is followed by a
        /// 10 ms wait, so this is a tenth of a second of nothing.
        const QUIET_TURNS: u32 = 10;
        let deadline = Instant::now() + WAIT;
        let mut quiet = 0;
        while quiet < QUIET_TURNS {
            let before = self.events.len();
            self.pump();
            if self.events.len() == before {
                quiet += 1;
            } else {
                quiet = 0;
            }
            assert!(
                Instant::now() < deadline,
                "the desktop never went quiet; the queue holds {:#06x} and the events so far \
                 are {:?}",
                desktop::queue_status(),
                self.names()
            );
            self.shell.wait_events(Some(Duration::from_millis(10)));
        }
    }

    /// Creates a window, waits for its first configuration, and lets the
    /// desktop settle around it.
    fn window(&mut self, title: &str) -> WindowId {
        self.window_with(&WindowDesc {
            title,
            app_id: "sh.kryptic.crcbl.e2e",
            size: SIZE,
            ..WindowDesc::default()
        })
    }

    /// The same, for a test that needs a descriptor of its own.
    fn window_with(&mut self, desc: &WindowDesc<'_>) -> WindowId {
        let window = self.shell.create_window(desc).expect("create_window");
        self.windows.push(window);
        self.pump_until("the first configuration", |session| {
            session
                .shell
                .window_state(window)
                .is_ok_and(|state| state.is_configured())
        });
        if desc.visible {
            self.settle();
        }
        window
    }

    /// The window's `HWND`, which is how the desktop names it.
    fn hwnd(&self, window: WindowId) -> desktop::Handle {
        match self.shell.surface_target(window).expect("surface target") {
            SurfaceTarget::Win32 { hwnd, .. } => hwnd.as_ptr(),
            other => panic!(
                "the Win32 backend produced a {} target",
                other.platform_name()
            ),
        }
    }

    /// Puts a window in front and waits for the shell to agree it has the
    /// keyboard.
    ///
    /// Both halves matter and they are different facts. The **foreground** is
    /// what `SendInput` follows, so without it an injected keystroke goes to
    /// somebody else's window; the shell's own `focused` is what says the
    /// `WM_SETFOCUS` reached the window procedure and was translated. A test
    /// that waited for one and assumed the other would report the wrong one of
    /// the two as broken.
    ///
    /// Asked once per turn rather than once: `SetForegroundWindow` is refused
    /// while another process still owns the foreground, and one refusal must not
    /// decide a test.
    ///
    /// # Its own deadline, and a shorter one
    ///
    /// [`WAIT`] is twenty seconds because the things it waits on are *other
    /// processes* — a helper injecting a keystroke, the desktop settling — and a
    /// slow runner must not read as a broken backend. **Taking the foreground is
    /// not one of those.** The grant is decided by the window station on the turn
    /// it is asked for, against rules that do not change while this loop spins;
    /// twenty seconds of asking is the same refusal four hundred times over. The
    /// first CI run of this suite spent sixty seconds across three tests learning
    /// nothing that the first second did not already say, so this waits long
    /// enough to cover a window that is still being shown and no longer.
    ///
    /// A refusal that survives the deadline is a **finding about the runner**,
    /// and it fails carrying the evidence to act on it: which window holds the
    /// foreground and whose it is, whether the lock timeout took, and whether the
    /// shell believes it has the keyboard. What it must not do is pass — the
    /// tests that call this inject input, and input follows the foreground, so a
    /// session that skipped it would assert against a keystroke that went to
    /// somebody else's window.
    fn foreground(&mut self, window: WindowId) {
        let hwnd = self.hwnd(window);
        let deadline = Instant::now() + FOREGROUND_WAIT;
        loop {
            self.pump();
            let in_front = desktop::take_foreground(hwnd);
            let focused = self
                .shell
                .window_state(window)
                .is_ok_and(|state| state.focused);
            if in_front && focused {
                return;
            }
            assert!(
                Instant::now() < deadline,
                "the desktop would not give {hwnd:?} the foreground within {FOREGROUND_WAIT:?}: \
                 it is on {}, we are {}, the lock timeout reads {:?} ms after asking for 0, the \
                 shell says focused={focused}, the queue holds {:#06x} and the events so far are \
                 {:?}.\nA timeout of 0 with the foreground still elsewhere means the lock is not \
                 what is refusing and `SetForegroundWindow` is being denied for another reason; \
                 anything else means SPI_SETFOREGROUNDLOCKTIMEOUT was itself refused, which it is \
                 documented to be for a process that cannot already take the foreground. Either \
                 way this test cannot inject input at its own window on this runner.",
                desktop::describe(desktop::foreground_window()),
                desktop::ours(),
                desktop::foreground_lock_timeout(),
                desktop::queue_status(),
                self.names(),
            );
            self.shell.wait_events(Some(Duration::from_millis(10)));
        }
    }

    fn names(&self) -> Vec<&'static str> {
        self.events.iter().map(ShellEvent::name).collect()
    }

    fn take_names(&mut self) -> Vec<&'static str> {
        let names = self.names();
        self.events.clear();
        names
    }

    /// Every key event delivered so far, flattened for assertion.
    fn keys(&self) -> Vec<(u32, Option<KeyCode>, Keysym, ButtonState, bool)> {
        self.events
            .iter()
            .filter_map(|event| match event {
                ShellEvent::Key {
                    scancode,
                    key_code,
                    keysym,
                    state,
                    repeat,
                    ..
                } => Some((scancode.0, *key_code, *keysym, *state, *repeat)),
                _ => None,
            })
            .collect()
    }

    /// The one `ClipboardData` answering `request`, waiting for it to arrive.
    fn clipboard_answer(
        &mut self,
        request: crcbl_shell::ClipboardRequestId,
    ) -> (crcbl_shell::ReceivedMime, ClipboardContent) {
        self.pump_until("the clipboard answer", |session| {
            session.events.iter().any(|event| {
                matches!(event, ShellEvent::ClipboardData { request: got, .. } if *got == request)
            })
        });
        let mut answers = self.events.iter().filter_map(|event| match event {
            ShellEvent::ClipboardData {
                request: got,
                mime,
                content,
                ..
            } if *got == request => Some((mime.clone(), content.clone())),
            _ => None,
        });
        let answer = answers.next().expect("just waited for it");
        assert!(
            answers.next().is_none(),
            "obligation 4: exactly one answer per accepted request"
        );
        answer
    }
}

impl Drop for Session {
    /// Destroys everything this session opened.
    ///
    /// Failures are ignored on purpose: a test that already destroyed its window
    /// — or one panicking its way out with a stale handle — must not turn a real
    /// assertion failure into a confusing second panic while unwinding.
    fn drop(&mut self) {
        for window in core::mem::take(&mut self.windows) {
            let _ = self.shell.set_visible(window, false);
            let _ = self.shell.destroy_window(window);
        }
        // Give the procedure the `WM_DESTROY`s before the process goes, so the
        // desktop is not left holding the foreground for a window that no longer
        // exists.
        self.shell.pump(&mut |_| {});
        // And put the desktop's focus-stealing protection back, for the machine
        // that is not a CI runner. Ignored on purpose, like everything else on
        // this path.
        if let Some(previous) = self.previous_lock_timeout {
            let _ = desktop::unlock_foreground(previous);
        }
    }
}

// ---------------------------------------------------------------------------
// The helper processes
// ---------------------------------------------------------------------------

/// A running `crcbl-e2e-win32-input`, and everything it has said.
///
/// Both of its output streams are drained on threads of their own. That is not
/// tidiness: a child whose stderr pipe fills up blocks in `write`, and a sender
/// blocked in `write` types nothing — which arrives as "the keystroke never
/// came" and points at the backend.
struct Sender {
    child: Child,
    stdin: Option<ChildStdin>,
    lines: Arc<Mutex<Vec<String>>>,
}

impl Sender {
    /// Starts the sender and waits for it to say it is up.
    ///
    /// `CARGO_BIN_EXE_*` rather than a path built by hand: cargo sets it for
    /// every `[[bin]]` of this package when it compiles an integration test, so
    /// the binary that runs is the one that was built beside this file — with
    /// the same features and into the same profile.
    fn start() -> Self {
        let mut child = Command::new(env!("CARGO_BIN_EXE_crcbl-e2e-win32-input"))
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()
            .expect("crcbl-e2e-win32-input is a [[bin]] of this package under win32-e2e");
        let lines = Arc::new(Mutex::new(Vec::new()));
        drain(
            child.stdout.take().expect("stdout was piped"),
            "out: ",
            Arc::clone(&lines),
        );
        drain(
            child.stderr.take().expect("stderr was piped"),
            "err: ",
            Arc::clone(&lines),
        );
        let stdin = child.stdin.take().expect("stdin was piped");
        let sender = Self {
            child,
            stdin: Some(stdin),
            lines,
        };
        sender.wait_for("ready");
        sender
    }

    /// Everything the sender has printed, for a failure message.
    fn lines(&self) -> Vec<String> {
        self.lines
            .lock()
            .expect("the drain threads never panic while holding it")
            .clone()
    }

    /// Waits for a line containing `needle`, or fails printing everything it did
    /// say.
    fn wait_for(&self, needle: &str) {
        let deadline = Instant::now() + WAIT;
        loop {
            if self.lines().iter().any(|line| line.contains(needle)) {
                return;
            }
            assert!(
                Instant::now() < deadline,
                "the input sender never said {needle:?} within {WAIT:?}; it said {:?}",
                self.lines()
            );
            std::thread::sleep(Duration::from_millis(5));
        }
    }

    /// Sends one command and waits for the sender to acknowledge it.
    ///
    /// Waiting for the acknowledgement is what separates "the input was never
    /// injected" from "it was injected and never arrived", and those are
    /// findings about completely different halves of this machinery.
    fn send(&mut self, command: &str) {
        let stdin = self.stdin.as_mut().expect("still running");
        writeln!(stdin, "{command}").expect("the sender is still reading its stdin");
        stdin
            .flush()
            .expect("the sender is still reading its stdin");
        self.wait_for(&format!("sent {command:?}"));
    }
}

impl Drop for Sender {
    fn drop(&mut self) {
        // Closing stdin is how it is asked to stop; it exits on EOF. Nothing is
        // asserted here: a test already failing must not be buried under a
        // second panic about its helper.
        self.stdin = None;
        let _ = self.child.wait();
    }
}

/// Copies one of a child's streams into a shared list of lines.
fn drain<R: std::io::Read + Send + 'static>(
    stream: R,
    tag: &'static str,
    into: Arc<Mutex<Vec<String>>>,
) {
    std::thread::spawn(move || {
        for line in BufReader::new(stream).lines() {
            let Ok(line) = line else { return };
            into.lock()
                .expect("nothing panics while holding this")
                .push(format!("{tag}{line}"));
        }
    });
}

/// Runs `crcbl-e2e-win32-clip` to completion and answers what it printed.
///
/// Blocking on purpose, and the blocking is half the test: nothing pumps this
/// process's message loop while the peer talks to the clipboard. On X11 or
/// Wayland that would deadlock, because there the copier *is* the server of the
/// bytes; on Win32 the window station holds them and there is nobody to answer.
fn clip(args: &[&str]) -> String {
    let output = Command::new(env!("CARGO_BIN_EXE_crcbl-e2e-win32-clip"))
        .args(args)
        .output()
        .expect("crcbl-e2e-win32-clip is a [[bin]] of this package under win32-e2e");
    let stdout = String::from_utf8_lossy(&output.stdout).into_owned();
    assert!(
        output.status.success(),
        "the clipboard peer failed on {args:?} with {:?}; it said {stdout:?} and {:?}",
        output.status,
        String::from_utf8_lossy(&output.stderr)
    );
    stdout
}

/// The `text` line of a peer read, or `None` when the format was absent.
fn clip_text(args: &[&str]) -> Option<String> {
    let printed = clip(args);
    if printed.contains("crcbl-e2e-win32-clip: absent") {
        return None;
    }
    let line = printed
        .lines()
        .find_map(|line| line.strip_prefix("crcbl-e2e-win32-clip: text "))
        .unwrap_or_else(|| {
            panic!("the peer neither reported a format nor its absence: {printed:?}")
        });
    Some(line.to_owned())
}

// ---------------------------------------------------------------------------
// Connection and capabilities
// ---------------------------------------------------------------------------

/// The registry hands out a Win32 shell, and its capability set is this
/// platform's rather than a convenient one.
///
/// The in-crate suite asserts the same bits against `Win32Shell`; what is new
/// here is the path — `open_backend` and `dyn Shell`, which is what a consumer
/// gets — and that the methods each bit claims are reachable through it.
#[test]
#[ignore = "needs a Windows desktop; run tests/run-win32-e2e.ps1"]
fn the_backend_is_reachable_by_name_and_is_win32_shaped() {
    let mut session = Session::open();
    let window = session.window("caps");
    let caps = session.shell.caps();

    for present in [
        ShellCaps::MULTI_WINDOW,
        ShellCaps::EVENT_WAIT,
        ShellCaps::WINDOW_POSITION,
        // Set where X11's is conditional: there is no window manager to be
        // absent, so a windowed window is decorated by the system, always.
        ShellCaps::SERVER_DECORATIONS,
        ShellCaps::FRACTIONAL_SCALE,
        // Also unconditional here, and for the same reason — `WM_SIZING` is
        // answered by this process rather than requested of another one.
        ShellCaps::ASPECT_HINT_HONORED,
        ShellCaps::POINTER_LOCK,
        ShellCaps::POINTER_CONFINE,
        ShellCaps::POINTER_WARP,
        ShellCaps::CLIPBOARD,
        ShellCaps::DRAG_DROP,
    ] {
        assert!(caps.contains(present), "{present:?} is implemented");
    }
    assert!(
        caps.contains(ShellCaps::RAW_POINTER_MOTION),
        "RegisterRawInputDevices was refused on this runner, which is a finding about the runner"
    );
    assert!(caps.has_mouselook(), "both halves, which is the point");

    for absent in [ShellCaps::TEXT_IME, ShellCaps::HW_UPSCALE] {
        assert!(!caps.contains(absent), "{absent:?} is not implemented");
    }

    // Latched, as implementor obligation 3 requires — across a mode change,
    // which is the one thing on this platform that could plausibly move one.
    session
        .shell
        .set_mode(window, DisplayMode::Borderless { monitor: None })
        .expect("set_mode");
    for _ in 0..10 {
        session.pump();
        assert_eq!(session.shell.caps(), caps, "caps are latched at open");
    }
}

// ---------------------------------------------------------------------------
// The window lifecycle, judged by the system rather than by the backend
// ---------------------------------------------------------------------------

/// The P0.4 contract on a platform that knew the answer before it was asked, and
/// the size it reports is the one the *system* reports.
#[test]
#[ignore = "needs a Windows desktop; run tests/run-win32-e2e.ps1"]
fn a_window_is_configured_on_the_first_pump_at_the_size_the_system_reports() {
    let mut session = Session::open();
    let window = session
        .shell
        .create_window(&WindowDesc {
            title: "unconfigured",
            size: SIZE,
            ..WindowDesc::default()
        })
        .expect("create_window");
    session.windows.push(window);

    let state = session.shell.window_state(window).expect("state");
    assert!(!state.is_configured(), "no event has been delivered yet");
    assert_eq!(state.size(), None);
    assert_eq!(state.scale_factor(), None);
    assert_eq!(state.effective_mode(), None);

    // The surface target, by contrast, exists immediately — `CreateWindowExW`
    // has already returned an `HWND`, and only the *swapchain* has to wait.
    let hwnd = session.hwnd(window);
    assert!(!hwnd.is_null());

    session.pump();
    let state = session.shell.window_state(window).expect("state");
    assert!(
        state.is_configured(),
        "Win32 answers on the first pump: {:?}",
        session.names()
    );
    assert_eq!(
        session
            .events
            .iter()
            .filter(|event| matches!(event, ShellEvent::Resized { .. }))
            .count(),
        1,
        "one Resized, and nothing invented around it: {:?}",
        session.names()
    );

    // The two numbers the whole seam rests on, each compared against what the
    // system tells anybody who asks rather than against what the backend
    // remembers telling us.
    let client = desktop::client_rect(hwnd);
    let size = state.size().expect("configured");
    assert_eq!(
        (
            i32::try_from(size.width).expect("a client area fits in an i32"),
            i32::try_from(size.height).expect("a client area fits in an i32"),
        ),
        (client.width(), client.height()),
        "the reported size is GetClientRect's, not a remembered request; the window is at {:?}",
        desktop::window_rect(hwnd)
    );
    let dpi = desktop::dpi(hwnd);
    assert!(
        (state.scale_factor().expect("configured")
            - f64::from(dpi) / f64::from(desktop::DEFAULT_DPI))
        .abs()
            < f64::EPSILON,
        "the scale factor is GetDpiForWindow({dpi}) over 96, and the process is per-monitor-v2 \
         aware because opening the shell made it so"
    );
}

/// A storm of resizes is delivered as **one** event carrying the final size.
///
/// The coalescing the module documents, through the real queue rather than
/// through the arithmetic. It is what makes a three-second drag of a window edge
/// deliver one `Resized` instead of a few hundred, and the intermediate sizes it
/// discards are the record of frames that were never drawn.
///
/// A burst rather than a real drag because CI has nobody to hold a mouse button;
/// what the two have in common is the thing under test — many `WM_SIZE`s
/// recorded before anything drains them.
#[test]
#[ignore = "needs a Windows desktop; run tests/run-win32-e2e.ps1"]
fn a_burst_of_resizes_from_outside_is_delivered_as_one_event() {
    /// How many sizes to push through before pumping. Small enough that every
    /// one of them fits on a 1024×768 desktop.
    const STEPS: i32 = 64;
    let mut session = Session::open();
    let window = session.window("resize storm");
    let hwnd = session.hwnd(window);
    session.take_names();

    for step in 0..STEPS {
        desktop::resize(hwnd, 320 + step * 4, 240 + step * 3);
    }
    let final_client = desktop::client_rect(hwnd);
    session.pump_until("the resize", |session| {
        session
            .events
            .iter()
            .any(|event| matches!(event, ShellEvent::Resized { .. }))
    });

    let sizes: Vec<_> = session
        .events
        .iter()
        .filter_map(|event| match event {
            ShellEvent::Resized { size, .. } => Some(*size),
            _ => None,
        })
        .collect();
    assert_eq!(
        sizes.len(),
        1,
        "{STEPS} resizes, one event: {sizes:?} out of {:?}",
        session.names()
    );
    assert_eq!(
        (
            i32::try_from(sizes[0].width).expect("a client area fits in an i32"),
            i32::try_from(sizes[0].height).expect("a client area fits in an i32"),
        ),
        (final_client.width(), final_client.height()),
        "the one event carries the size the window ended at, which is the only size any frame \
         would have been rendered at"
    );
}

/// Borderless covers a monitor, and going back restores the windowed placement
/// **exactly** — position included.
///
/// The position is not in the seam, so it is read from the system. That is the
/// point: "restores the windowed placement" is a claim about a rectangle nobody
/// but Windows remembers, and a backend that restored the size and forgot the
/// origin would satisfy every in-seam assertion and still walk a window across
/// the desktop once per mode flip.
#[test]
#[ignore = "needs a Windows desktop; run tests/run-win32-e2e.ps1"]
fn borderless_covers_a_monitor_and_windowed_restores_the_exact_placement() {
    let mut session = Session::open();
    let window = session.window("mode");
    let hwnd = session.hwnd(window);
    let windowed = desktop::window_rect(hwnd);
    let windowed_size = session
        .shell
        .window_state(window)
        .expect("state")
        .size()
        .expect("configured");

    session
        .shell
        .set_mode(window, DisplayMode::Borderless { monitor: None })
        .expect("set_mode");
    session.pump_until("the borderless configuration", |session| {
        session
            .shell
            .window_state(window)
            .is_ok_and(|state| state.mode_request_honoured())
    });

    // **This is the one backend where the answer is never "no".** There is no
    // window manager to refuse a style change this process makes to its own
    // window, which is exactly why the X11 suite has a branch here and this does
    // not.
    let state = session.shell.window_state(window).expect("state");
    let effective = state.effective_mode().expect("configured");
    assert!(
        effective.is_borderless(),
        "nothing on this platform can refuse it: {effective:?}"
    );
    let landed = match effective {
        DisplayMode::Borderless {
            monitor: Some(monitor),
        } => monitor,
        other => panic!("the effective mode names the monitor it landed on, not {other:?}"),
    };
    let bounds = session
        .shell
        .monitor(landed)
        .expect("the mode names a live monitor")
        .bounds;
    assert_eq!(
        desktop::window_rect(hwnd),
        desktop::Rect {
            left: bounds.x,
            top: bounds.y,
            right: bounds.x + i32::try_from(bounds.width).expect("a monitor fits in an i32"),
            bottom: bounds.y + i32::try_from(bounds.height).expect("a monitor fits in an i32"),
        },
        "borderless is the monitor's whole rectangle; the virtual screen is {:?}",
        desktop::virtual_screen()
    );

    session
        .shell
        .set_mode(window, DisplayMode::Windowed)
        .expect("set_mode");
    session.pump_until("the windowed configuration", |session| {
        session
            .shell
            .window_state(window)
            .is_ok_and(|state| state.effective_mode() == Some(DisplayMode::Windowed))
    });
    assert_eq!(
        desktop::window_rect(hwnd),
        windowed,
        "the windowed placement is restored exactly — origin as well as size"
    );
    assert_eq!(
        session.shell.window_state(window).expect("state").size(),
        Some(windowed_size),
        "and the client area with it"
    );
}

/// A borderless request naming a monitor lands on that monitor, and one naming a
/// monitor that is not there is a clean error.
#[test]
#[ignore = "needs a Windows desktop; run tests/run-win32-e2e.ps1"]
fn borderless_on_a_named_monitor_lands_on_that_monitor() {
    let mut session = Session::open();
    let window = session.window("named monitor");
    let hwnd = session.hwnd(window);

    let monitors: Vec<(crcbl_shell::MonitorId, PhysicalRect, String)> = session
        .shell
        .monitors()
        .iter()
        .map(|monitor| (monitor.id, monitor.bounds, monitor.name.clone()))
        .collect();
    // A runner has one display, so this loop runs once — and the assertion that
    // it ran at all is what stops "no monitors" from reading as a pass.
    assert!(
        !monitors.is_empty(),
        "a session with a window station has at least one display"
    );
    for (id, bounds, name) in &monitors {
        session
            .shell
            .set_mode(window, DisplayMode::Borderless { monitor: Some(*id) })
            .expect("set_mode");
        session.pump_until("the borderless configuration", |session| {
            session
                .shell
                .window_state(window)
                .is_ok_and(|state| state.mode_request_honoured())
        });
        let rect = desktop::window_rect(hwnd);
        assert_eq!(
            (rect.left, rect.top, rect.width(), rect.height()),
            (
                bounds.x,
                bounds.y,
                i32::try_from(bounds.width).expect("a monitor fits in an i32"),
                i32::try_from(bounds.height).expect("a monitor fits in an i32"),
            ),
            "borderless on {name} put the window at {rect:?} rather than over {bounds:?}"
        );
        session
            .shell
            .set_mode(window, DisplayMode::Windowed)
            .expect("set_mode");
        session.pump_until("the windowed configuration", |session| {
            session
                .shell
                .window_state(window)
                .is_ok_and(|state| state.effective_mode() == Some(DisplayMode::Windowed))
        });
    }

    let absent = crcbl_shell::MonitorId(9_999);
    assert!(matches!(
        session.shell.set_mode(
            window,
            DisplayMode::Borderless {
                monitor: Some(absent)
            }
        ),
        Err(ShellError::NoSuchMonitor(9_999))
    ));
    assert!(matches!(
        session.shell.create_window(&WindowDesc {
            title: "born on a monitor that is not there",
            mode: DisplayMode::Borderless {
                monitor: Some(absent)
            },
            size: SIZE,
            ..WindowDesc::default()
        }),
        Err(ShellError::NoSuchMonitor(9_999))
    ));
}

/// The enumerated monitors describe the desktop the system describes.
#[test]
#[ignore = "needs a Windows desktop; run tests/run-win32-e2e.ps1"]
fn the_monitors_tile_the_virtual_screen_the_system_reports() {
    let session = Session::open();
    let screen = desktop::virtual_screen();
    let monitors = session.shell.monitors();
    assert!(
        !monitors.is_empty(),
        "a session with a window station has at least one display; the virtual screen is {screen:?}"
    );
    assert_eq!(
        monitors.iter().filter(|monitor| monitor.is_primary).count(),
        1,
        "exactly one primary, so a consumer's default is unambiguous"
    );

    let mut ids: Vec<_> = monitors.iter().map(|monitor| monitor.id).collect();
    let count = ids.len();
    ids.sort_unstable();
    ids.dedup();
    assert_eq!(ids.len(), count, "ids are unique within a session");

    for monitor in monitors {
        assert!(!monitor.size().is_empty(), "{}", monitor.name);
        assert!(monitor.scale_factor > 0.0, "{}", monitor.name);
        // Every monitor is inside the virtual screen, which is the definition of
        // the virtual screen. Signed on both sides: a display left of the
        // primary has a negative origin, and a test that assumed zero would pass
        // on a one-display runner and fail on a desk.
        let bounds = monitor.bounds;
        let right = bounds.x + i32::try_from(bounds.width).expect("a monitor fits in an i32");
        let bottom = bounds.y + i32::try_from(bounds.height).expect("a monitor fits in an i32");
        assert!(
            bounds.x >= screen.left
                && bounds.y >= screen.top
                && right <= screen.right
                && bottom <= screen.bottom,
            "{} at {bounds:?} is not inside the virtual screen {screen:?}",
            monitor.name
        );
        // The work area is the taskbar's doing and is a *subset*, never a
        // superset — a backend that swapped the two would report a work area
        // larger than the display.
        let work = monitor.work_area;
        assert!(
            work.x >= bounds.x
                && work.y >= bounds.y
                && work.x + i32::try_from(work.width).expect("a work area fits in an i32") <= right
                && work.y + i32::try_from(work.height).expect("a work area fits in an i32")
                    <= bottom,
            "{}'s work area {work:?} is not inside its bounds {bounds:?}",
            monitor.name
        );
        assert_eq!(
            session.shell.monitor(monitor.id).map(|found| &found.name),
            Some(&monitor.name)
        );
    }
    assert_eq!(
        session.shell.monitor(crcbl_shell::MonitorId(9_999)),
        None,
        "a monitor that is not there is None, not a panic"
    );
}

/// Focus follows the foreground window, and exactly one window has it.
///
/// New over the in-crate suite in the way that matters: those tests send
/// `WM_SETFOCUS` and `WM_KILLFOCUS` by hand, because CI cannot click on a
/// window. Here the *system* decides who is in front and the messages arrive
/// through the queue, which is the only version of this that could catch a
/// backend that never handled them.
#[test]
#[ignore = "needs a Windows desktop; run tests/run-win32-e2e.ps1"]
fn focus_follows_the_foreground_window() {
    let mut session = Session::open();
    let first = session.window("focus one");
    let second = session.window("focus two");
    assert_ne!(session.hwnd(first), session.hwnd(second));

    for (front, back) in [(first, second), (second, first)] {
        // Drained *before* the request, not after: creating and showing two
        // windows produces focus changes of its own, and an `any` over events
        // that included them would be satisfied by a `Focus` this iteration did
        // not cause.
        session.events.clear();
        session.foreground(front);
        session.pump_until("the other window to give up the keyboard", |session| {
            session
                .shell
                .window_state(back)
                .is_ok_and(|state| !state.focused)
        });
        assert!(
            session.shell.window_state(front).expect("state").focused,
            "the foreground window has the keyboard; the desktop says it is {:?} and this one \
             is {:?}",
            desktop::foreground_window(),
            session.hwnd(front)
        );
        assert!(
            session
                .events
                .iter()
                .any(|event| matches!(event, ShellEvent::Focus { window, focused: true, .. } if *window == front)),
            "and it was announced rather than only observed: {:?}",
            session.names()
        );
        session.events.clear();
    }
}

// ---------------------------------------------------------------------------
// Input, injected from another process
// ---------------------------------------------------------------------------

/// A keystroke typed by another process arrives with its position, its symbol
/// and — the part no in-process test could reach — its **text**.
///
/// The text is the whole reason this test is shaped the way it is.
/// `ShellEvent::TextCommit` comes from `WM_CHAR`, and a `WM_CHAR` exists only
/// because `TranslateMessage` ran over a key message *in the queue*. A test that
/// sends `WM_CHAR` with `SendMessageW` — which is what the in-crate suite does,
/// correctly, for what it is testing — proves the reassembly and says nothing
/// about whether a keyboard could ever produce one.
#[test]
#[ignore = "needs a Windows desktop; run tests/run-win32-e2e.ps1"]
fn a_key_typed_by_another_process_carries_its_position_its_symbol_and_its_text() {
    let mut session = Session::open();
    let window = session.window("keys");
    session.foreground(window);
    session.take_names();

    let mut sender = Sender::start();
    sender.send(&format!("key {}", scancode::A));
    session.pump_until("the keystroke and its release", |session| {
        session.keys().len() >= 2
    });

    let keys = session.keys();
    let (code, key_code, keysym, state, repeat) = keys[0];
    assert_eq!(
        code,
        scancode::A,
        "the raw code is the set 1 position the key message's lParam carried; the sender said \
         {:?}",
        sender.lines()
    );
    assert_eq!(key_code, Some(KeyCode::KeyA), "the physical position");
    assert_eq!(state, ButtonState::Pressed);
    assert!(!repeat, "the first press is not a repeat");
    assert_eq!(
        keysym,
        Keysym::from_char('a'),
        "the layout's symbol, lowercased so a rebind menu reads the same as it does on Linux"
    );
    assert_eq!(keys[1].3, ButtonState::Released);
    assert!(!keys[1].4);

    // The half that only a queued message can produce.
    let committed: String = session
        .events
        .iter()
        .filter_map(|event| match event {
            ShellEvent::TextCommit { text, .. } => Some(text.as_str()),
            _ => None,
        })
        .collect();
    assert_eq!(
        committed,
        "a",
        "a printable key commits text, which needs TranslateMessage in the pump; the events \
         were {:?} and the sender said {:?}",
        session.names(),
        sender.lines()
    );
}

/// A second injected press of a key that is already down produces **no second
/// event**, and the release still arrives.
///
/// # This test asserted the opposite, and the backend was right
///
/// It was written as "press, repeat, release" on the reasoning that bit 30 of
/// the `lParam` — the previous-key-state bit — would be set by the system on the
/// second `WM_KEYDOWN` because the system knows the key is down. The first CI
/// run against a real desktop reported two `down` commands sent and one
/// `Pressed` delivered, and that is correct: **`SendInput` describes a state
/// transition, not a keystroke.** The input system tracks which keys are down,
/// a second down for a key already down is not a transition, and no second
/// `WM_KEYDOWN` is generated at all.
///
/// Genuine auto-repeat comes from the keyboard driver's typematic timer holding
/// a *physically* depressed key, which no amount of `SendInput` reproduces — so
/// this suite cannot reach the repeat bit, and `docs/backlog.md` carries that as
/// a real gap rather than a solved problem. What it *can* reach is the
/// coalescing above, which is worth pinning precisely because it surprised us.
///
/// The decoding of the repeat bit is covered where it can be: the in-crate suite
/// in `src/win32/shell.rs` builds the `lParam` itself and delivers it with
/// `SendMessageW`, so it exercises `keys::` on a message with bit 30 set without
/// needing the system to have set it.
#[test]
#[ignore = "needs a Windows desktop; run tests/run-win32-e2e.ps1"]
fn a_second_injected_press_of_a_held_key_produces_no_second_event() {
    let mut session = Session::open();
    let window = session.window("held");
    session.foreground(window);
    session.take_names();

    let mut sender = Sender::start();
    sender.send(&format!("down {}", scancode::A));
    sender.send(&format!("down {}", scancode::A));
    sender.send(&format!("up {}", scancode::A));
    session.pump_until("the release, last", |session| {
        matches!(
            session.keys().last(),
            Some((_, _, _, ButtonState::Released, _))
        )
    });

    let keys = session.keys();
    let states: Vec<_> = keys
        .iter()
        .map(|(_, _, _, state, repeat)| (*state, *repeat))
        .collect();
    assert_eq!(
        states,
        vec![
            (ButtonState::Pressed, false),
            (ButtonState::Released, false),
        ],
        "two injected downs are one transition, so one press — and the release is never a \
         repeat however long the key was held; the sender said {:?}",
        sender.lines()
    );
}

/// An extended key keeps its `E0` prefix, which is its identity rather than a
/// flag beside it.
///
/// `ArrowUp` and the numeric keypad's `8` send the same low byte, and only the
/// prefix tells them apart. Reading it out of a message the system built is the
/// thing this adds: the in-crate suite sets bit 24 of the `lParam` itself, which
/// asserts that this project agrees with this project.
#[test]
#[ignore = "needs a Windows desktop; run tests/run-win32-e2e.ps1"]
fn an_extended_key_typed_by_another_process_keeps_its_e0_prefix() {
    let mut session = Session::open();
    let window = session.window("extended");
    session.foreground(window);
    session.take_names();

    let mut sender = Sender::start();
    sender.send(&format!("key {:#x}", scancode::ARROW_UP));
    session.pump_until("the arrow key", |session| !session.keys().is_empty());

    let (code, key_code, _, state, _) = session.keys()[0];
    assert_eq!(
        code,
        scancode::ARROW_UP,
        "the extended prefix is part of the code, not a bit dropped on the way through; the \
         sender said {:?}",
        sender.lines()
    );
    assert_eq!(key_code, Some(KeyCode::ArrowUp));
    assert_eq!(state, ButtonState::Pressed);
    assert!(
        !session.names().contains(&"TextCommit"),
        "an arrow key moves a cursor; it is not a character in a text field: {:?}",
        session.names()
    );
}

/// The pointer's three event kinds, driven from outside this process.
#[test]
#[ignore = "needs a Windows desktop; run tests/run-win32-e2e.ps1"]
fn a_pointer_driven_by_another_process_moves_clicks_and_scrolls() {
    let mut session = Session::open();
    let window = session.window("pointer");
    session.foreground(window);

    // Parked with the seam's own warp rather than by injecting an absolute
    // move: `POINTER_WARP` is a capability this backend claims, the click below
    // has to land inside the client area, and only the shell knows where that
    // is. Drained afterwards, so the motion this parking produces cannot be
    // read as the motion the test is about.
    session
        .shell
        .warp_pointer(window, PhysicalPoint::new(100.0, 80.0))
        .expect("POINTER_WARP is claimed");
    session.pump_until("the pointer to park", |session| {
        session
            .events
            .iter()
            .any(|event| matches!(event, ShellEvent::PointerMotion { .. }))
    });
    session.settle();
    session.events.clear();

    let mut sender = Sender::start();
    sender.send("move 12 9");
    session.pump_until("the injected motion", |session| {
        session
            .events
            .iter()
            .any(|event| matches!(event, ShellEvent::PointerMotion { abs: Some(_), .. }))
    });
    let moved_to = session
        .events
        .iter()
        .rev()
        .find_map(|event| match event {
            ShellEvent::PointerMotion { abs: Some(at), .. } => Some(*at),
            _ => None,
        })
        .expect("just waited for it");
    // Not an exact landing: a relative move goes through the system's pointer
    // acceleration, so the distance is the system's business. The direction is
    // not, and a backend that transposed the coordinates would fail here.
    assert!(
        moved_to.x > 100.0 && moved_to.y > 80.0,
        "the pointer moved right and down from where it was parked, to {moved_to:?}; the cursor \
         is at {:?} on a desktop of {:?}",
        desktop::cursor_position(),
        desktop::virtual_screen()
    );

    session.events.clear();
    sender.send("click left");
    session.pump_until("the click", |session| {
        session
            .events
            .iter()
            .filter(|event| matches!(event, ShellEvent::Button { .. }))
            .count()
            >= 2
    });
    let buttons: Vec<_> = session
        .events
        .iter()
        .filter_map(|event| match event {
            ShellEvent::Button { button, state, .. } => Some((*button, *state)),
            _ => None,
        })
        .collect();
    assert_eq!(
        buttons,
        vec![
            (PointerButton::Left, ButtonState::Pressed),
            (PointerButton::Left, ButtonState::Released),
        ],
        "one click, two events: {:?}, sender said {:?}",
        session.names(),
        sender.lines()
    );

    session.events.clear();
    sender.send("wheel 1");
    session.pump_until("the wheel", |session| {
        session
            .events
            .iter()
            .any(|event| matches!(event, ShellEvent::Wheel { .. }))
    });
    let wheels: Vec<_> = session
        .events
        .iter()
        .filter_map(|event| match event {
            ShellEvent::Wheel { delta, .. } => Some(*delta),
            _ => None,
        })
        .collect();
    assert_eq!(
        wheels,
        vec![ScrollDelta::Lines { x: 0.0, y: 1.0 }],
        "one notch is one line, and WHEEL_DELTA is not one: {:?}",
        session.names()
    );
    assert!(
        !session
            .events
            .iter()
            .any(|event| matches!(event, ShellEvent::Button { .. })),
        "a wheel notch is not a button press: {:?}",
        session.names()
    );

    // The cursor calls, which have no observable of their own through the seam
    // and are asserted to be accepted rather than to have an effect.
    session.shell.set_cursor(window, None).expect("hide");
    session
        .shell
        .set_cursor(window, Some(CursorIcon::Crosshair))
        .expect("shape");
}

/// Injected motion arrives as **raw**, unaccelerated relative motion — the thing
/// a first-person camera reads.
///
/// `WM_INPUT` is the one part of this backend that no in-crate test can reach:
/// a raw report needs an `HRAWINPUT` that only the system can produce, so
/// `input::read_raw_mouse` and the `RIM_TYPE_MOUSE` check have never run. That
/// is what this closes, and it is the assertion here most likely to be answered
/// by the runner rather than by the backend — if `SendInput` does not feed the
/// raw input stack on a `windows-latest` image, the failure below is a finding
/// about the runner and `docs/backlog.md` says so.
#[test]
#[ignore = "needs a Windows desktop; run tests/run-win32-e2e.ps1"]
fn injected_motion_arrives_as_raw_relative_motion_for_mouselook() {
    let mut session = Session::open();
    assert!(session.shell.caps().has_mouselook());
    let window = session.window("mouselook");
    session.foreground(window);
    session
        .shell
        .warp_pointer(window, PhysicalPoint::new(100.0, 80.0))
        .expect("POINTER_WARP is claimed");
    session.settle();
    session.events.clear();

    let mut sender = Sender::start();
    sender.send("move 20 15");
    session.pump_until("raw motion", |session| {
        session.events.iter().any(|event| {
            matches!(
                event,
                ShellEvent::PointerMotion {
                    raw_delta: Some(_),
                    ..
                }
            )
        })
    });
    let (dx, dy) = session
        .events
        .iter()
        .find_map(|event| match event {
            ShellEvent::PointerMotion {
                raw_delta: Some(delta),
                ..
            } => Some(*delta),
            _ => None,
        })
        .expect("just waited for it");
    assert!(
        dx > 0.0 && dy > 0.0,
        "the pointer moved right and down: ({dx}, {dy}); the sender said {:?}",
        sender.lines()
    );

    // Locking suppresses the absolute position, which is what
    // `PointerMotion::abs` documents for that mode — and the reason a camera can
    // read `raw_delta` without a position that has stopped meaning anything.
    session
        .shell
        .set_pointer_mode(window, PointerMode::Locked)
        .expect("POINTER_LOCK is claimed");
    session.events.clear();
    sender.send("move 9 7");
    session.pump_until("motion while locked", |session| {
        session
            .events
            .iter()
            .any(|event| matches!(event, ShellEvent::PointerMotion { .. }))
    });
    for event in &session.events {
        if let ShellEvent::PointerMotion { abs, .. } = event {
            assert_eq!(*abs, None, "a locked pointer has no meaningful position");
        }
    }
    session
        .shell
        .set_pointer_mode(window, PointerMode::Free)
        .expect("unlock");
}

// ---------------------------------------------------------------------------
// The clipboard, across a process boundary
// ---------------------------------------------------------------------------

/// Another process reads what we copied, while this one is not pumping at all.
///
/// **The claim this backend makes that neither Linux one can.**
/// `SetClipboardData` gives the bytes to the window station, so there is no
/// later conversation and this shell keeps nothing — which means the peer below
/// can read the copy with our message loop stopped dead. On X11 or Wayland the
/// same arrangement deadlocks, because there the copier is the *server* of the
/// bytes and has to be running to answer.
///
/// The pump count is how that is stated without a clock: not one turn of this
/// shell's loop happens between the copy and the peer's read.
#[test]
#[ignore = "needs a Windows desktop; run tests/run-win32-e2e.ps1"]
fn another_process_reads_what_we_copied_while_we_are_not_pumping() {
    let mut session = Session::open();
    let window = session.window("copy");

    session
        .shell
        .clipboard_offer(
            window,
            &[
                ClipboardOffer::text("copied from crcbl"),
                ClipboardOffer::ron("(entity: 1)"),
            ],
        )
        .expect("Win32 needs no recent user interaction to claim the clipboard");

    let before = session.pumps;
    assert_eq!(
        clip_text(&["get", "text"]).as_deref(),
        Some("copied from crcbl"),
        "CF_UNICODETEXT is what every other application on the desktop reads"
    );
    assert_eq!(
        clip_text(&["get", MimeType::CrcblRon.as_str()]).as_deref(),
        Some("(entity: 1)"),
        "the engine's own format is registered under its mime string, so a second Crucible \
         process interns the same id"
    );
    // A format never offered is absent rather than answered with the wrong
    // bytes.
    assert_eq!(
        clip_text(&["get", "image/png"]),
        None,
        "a format we did not publish is not on the clipboard"
    );
    assert_eq!(
        session.pumps, before,
        "the peer read all three with this shell's loop stopped; a backend that served its own \
         copy on demand could not have answered at all"
    );
}

/// We read what another process copied, and the answer arrives on the first
/// pump.
#[test]
#[ignore = "needs a Windows desktop; run tests/run-win32-e2e.ps1"]
fn we_read_what_another_process_copied() {
    let mut session = Session::open();
    let window = session.window("paste");

    clip(&["put", "text", "from", "the", "peer"]);
    let request = session
        .shell
        .clipboard_request(window, MimeType::TextUtf8)
        .expect("CLIPBOARD is claimed");
    let before = session.pumps;
    let (mime, content) = session.clipboard_answer(request);
    assert_eq!(content.text(), Some("from the peer"));
    assert!(mime.matches(MimeType::TextUtf8));
    assert_eq!(
        session.pumps - before,
        1,
        "a Win32 read is answered inside clipboard_request, so exactly one pump delivers it — \
         there is no transfer to alternate with"
    );
    assert!(
        session.slowest_pump < SLOWEST_PUMP,
        "and the frame loop never stalled: {:?}",
        session.slowest_pump
    );

    // The engine's own format across the same boundary, which is what an
    // editor-to-editor copy is.
    clip(&["put", MimeType::CrcblRon.as_str(), "(entity:", "7)"]);
    let request = session
        .shell
        .clipboard_request(window, MimeType::CrcblRon)
        .expect("CLIPBOARD is claimed");
    let (_, content) = session.clipboard_answer(request);
    assert_eq!(content.text(), Some("(entity: 7)"));
}

/// An empty offer empties the clipboard for every process, not just for this
/// one.
#[test]
#[ignore = "needs a Windows desktop; run tests/run-win32-e2e.ps1"]
fn an_empty_offer_empties_the_clipboard_for_every_process() {
    let mut session = Session::open();
    let window = session.window("release");
    session
        .shell
        .clipboard_offer(window, &[ClipboardOffer::text("temporary")])
        .expect("claim");
    assert_eq!(
        clip_text(&["get", "text"]).as_deref(),
        Some("temporary"),
        "it was there to begin with, or the release below proves nothing"
    );

    session.shell.clipboard_offer(window, &[]).expect("release");
    assert_eq!(
        clip_text(&["get", "text"]),
        None,
        "the window station holds nothing, as seen from outside this process"
    );

    // And this shell agrees with the desktop about it.
    let request = session
        .shell
        .clipboard_request(window, MimeType::TextUtf8)
        .expect("accepted");
    let (_, content) = session.clipboard_answer(request);
    assert_eq!(
        content,
        ClipboardContent::Empty,
        "there is nothing on the clipboard, which is not the same as a read that failed"
    );
}
