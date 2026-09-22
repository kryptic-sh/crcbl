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
//! F11 at it. What is missing is that suite, not a renderer to run it against:
//! `windows-latest` has had one since 2026-08-10, when `vk-e2e-windows` began
//! registering a lavapipe ICD, and `dx12-e2e` presents through a real `HWND` on
//! WARP. It is recorded in `docs/backlog.md` as the gap it is rather than
//! approximated here.
//!
//! Also absent, each for a reason `docs/backlog.md` carries: a **real** drag and
//! drop (the shell allocates the `HDROP` in the target's own context, so no test
//! process can hand one over), the `WM_IME_*` family, and multi-monitor
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
use std::path::PathBuf;
use std::process::{Child, ChildStdin, Command, Stdio};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use crcbl_shell::{
    ButtonState, ClipboardContent, ClipboardOffer, ContactId, CursorIcon, DeviceId, DisplayMode,
    KeyCode, Keysym, LogicalSize, MimeType, PhysicalPoint, PhysicalRect, PointerButton,
    PointerMode, ScrollDelta, Shell, ShellBackend, ShellCaps, ShellError, ShellEvent,
    SurfaceTarget, TouchPhase, WindowDesc, WindowId, parse_uri_list,
};

/// PS/2 set 1 scan codes, spelled as the sender takes them and as
/// `win32::keys::scancode` reports them.
mod scancode {
    /// `A`.
    pub const A: u32 = 0x1E;
    /// Left `Alt`.
    pub const ALT: u32 = 0x38;
    /// `E`.
    pub const E: u32 = 0x12;
    /// The key right of `;` on a US keyboard: `'`, and on US-International the
    /// dead acute accent.
    pub const QUOTE: u32 = 0x28;
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

    // Both as the SDK lays them out on x64 — the Windows ABI, and the numbers
    // `win32::ffi`'s layout test carries for its own copies. The closures name
    // every field with no `..` and return them as `i32`s, which pins each
    // field's width and fails to compile if a field is added.
    const _: () = {
        let _: fn(Rect) -> [i32; 4] = |rect| {
            let Rect {
                left,
                top,
                right,
                bottom,
            } = rect;
            [left, top, right, bottom]
        };
        let _: fn(Point) -> [i32; 2] = |point| {
            let Point { x, y } = point;
            [x, y]
        };
        assert!(size_of::<Rect>() == 16, "RECT");
        assert!(core::mem::offset_of!(Rect, left) == 0);
        assert!(core::mem::offset_of!(Rect, top) == 4);
        assert!(core::mem::offset_of!(Rect, right) == 8);
        assert!(core::mem::offset_of!(Rect, bottom) == 12);
        assert!(size_of::<Point>() == 8, "POINT");
        assert!(core::mem::offset_of!(Point, x) == 0);
        assert!(core::mem::offset_of!(Point, y) == 4);
    };

    /// `SM_CXSCREEN` — the primary monitor's width.
    const SM_CX_SCREEN: i32 = 0;
    /// `SM_CYSCREEN`.
    const SM_CY_SCREEN: i32 = 1;
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
        fn ClientToScreen(hwnd: Handle, point: *mut Point) -> i32;
        fn GetWindowTextW(hwnd: Handle, buffer: *mut u16, capacity: i32) -> i32;
    }

    #[link(name = "kernel32")]
    unsafe extern "system" {
        fn GetCurrentThreadId() -> u32;
        fn GetCurrentProcessId() -> u32;
    }

    /// `HKEY_CURRENT_USER`: a predefined key, `(HKEY)(ULONG_PTR)(LONG)0x80000001`,
    /// so the 32-bit value is sign-extended into the pointer.
    const HKEY_CURRENT_USER: isize = 0x8000_0001_u32 as i32 as isize;
    /// `RRF_RT_REG_DWORD` — refuse the read unless the value is a `REG_DWORD`.
    const RRF_RT_REG_DWORD: u32 = 0x0000_0010;
    /// `ERROR_SUCCESS`.
    const ERROR_SUCCESS: i32 = 0;

    #[link(name = "advapi32")]
    unsafe extern "system" {
        fn RegGetValueW(
            key: *mut c_void,
            sub_key: *const u16,
            value: *const u16,
            flags: u32,
            kind: *mut u32,
            data: *mut c_void,
            size: *mut u32,
        ) -> i32;
    }

    /// `HKL`.
    pub type Layout = *mut c_void;

    /// `KLF_NOTELLSHELL` — load the layout without announcing it to the shell's
    /// language bar.
    const KLF_NO_TELL_SHELL: u32 = 0x0000_0080;

    #[link(name = "user32")]
    unsafe extern "system" {
        fn LoadKeyboardLayoutW(id: *const u16, flags: u32) -> Layout;
        fn ActivateKeyboardLayout(layout: Layout, flags: u32) -> Layout;
        fn UnloadKeyboardLayout(layout: Layout) -> i32;
        fn GetKeyboardLayoutList(capacity: i32, list: *mut Layout) -> i32;
    }

    /// A keyboard layout made active on **this thread** for the guard's
    /// lifetime, and put back on drop.
    ///
    /// Per-thread on purpose (`ActivateKeyboardLayout` without
    /// `KLF_SETFORPROCESS`): only the thread pumping the test window translates
    /// keys through it, so the developer's own typing elsewhere is untouched.
    /// The layout is unloaded again only if it was not already in the user's
    /// list, so running the suite never removes a layout somebody chose.
    pub struct ThreadLayout {
        previous: Layout,
        loaded: Option<Layout>,
    }

    impl ThreadLayout {
        /// Activates the layout named by `id`, a KLID such as `"00020409"`, or
        /// `None` if the system does not have it.
        #[must_use]
        pub fn activate(id: &str) -> Option<Self> {
            let before = layouts();
            let wide: Vec<u16> = id.encode_utf16().chain([0]).collect();
            // SAFETY: `wide` is a live NUL-terminated UTF-16 KLID; the call
            // reads it and returns a handle or null.
            let layout = unsafe { LoadKeyboardLayoutW(wide.as_ptr(), KLF_NO_TELL_SHELL) };
            if layout.is_null() {
                return None;
            }
            // SAFETY: a layout handle the call above returned, activated for the
            // calling thread only.
            let previous = unsafe { ActivateKeyboardLayout(layout, 0) };
            let loaded = (!before.contains(&layout)).then_some(layout);
            Some(Self { previous, loaded })
        }
    }

    impl Drop for ThreadLayout {
        fn drop(&mut self) {
            // SAFETY: the handle `ActivateKeyboardLayout` reported as active
            // before, which is still loaded because nothing here unloaded it.
            unsafe { ActivateKeyboardLayout(self.previous, 0) };
            if let Some(layout) = self.loaded {
                // SAFETY: a layout this guard loaded and that is no longer
                // active on this thread.
                unsafe { UnloadKeyboardLayout(layout) };
            }
        }
    }

    /// Every layout currently loaded for this session.
    fn layouts() -> Vec<Layout> {
        // SAFETY: a zero capacity with a null list asks only for the count.
        let count = unsafe { GetKeyboardLayoutList(0, core::ptr::null_mut()) };
        let mut list = vec![core::ptr::null_mut(); usize::try_from(count).unwrap_or(0)];
        // SAFETY: `list` holds exactly `count` writable handles.
        let written = unsafe { GetKeyboardLayoutList(count, list.as_mut_ptr()) };
        list.truncate(usize::try_from(written).unwrap_or(0));
        list
    }

    /// Whether the user has reversed the mouse wheel in Settings.
    ///
    /// Windows 11's "Scroll direction" setting stores
    /// `ReverseMouseWheelDirection` under `HKCU\Control Panel\Mouse`, and the
    /// system applies it **before** a wheel message reaches any window, injected
    /// input included: a `SendInput` notch away from the user arrives as a
    /// negative `WM_MOUSEWHEEL`. A backend that passes the sign through is
    /// honouring the user's choice, so the expectation has to follow the setting
    /// rather than assume its default. Absent on a stock install, and so on CI's
    /// runner, which reads as not reversed.
    #[must_use]
    pub fn wheel_reversed() -> bool {
        let sub_key: Vec<u16> = "Control Panel\\Mouse\0".encode_utf16().collect();
        let value: Vec<u16> = "ReverseMouseWheelDirection\0".encode_utf16().collect();
        let mut data: u32 = 0;
        let mut size = u32::try_from(size_of::<u32>()).expect("a DWORD's size fits a DWORD");
        // SAFETY: both names are live, NUL-terminated UTF-16 buffers;
        // `RRF_RT_REG_DWORD` makes the call refuse any value that is not exactly
        // a `DWORD`, so it writes at most the four bytes `size` announces into
        // `data`, and a null type pointer is documented as "do not report it".
        let status = unsafe {
            RegGetValueW(
                core::ptr::without_provenance_mut(HKEY_CURRENT_USER as usize),
                sub_key.as_ptr(),
                value.as_ptr(),
                RRF_RT_REG_DWORD,
                core::ptr::null_mut(),
                (&raw mut data).cast::<c_void>(),
                &raw mut size,
            )
        };
        status == ERROR_SUCCESS && data != 0
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

    /// The primary monitor's size in pixels: the extent a `MOUSEEVENTF_ABSOLUTE`
    /// move without `MOUSEEVENTF_VIRTUALDESK` is normalized over.
    #[must_use]
    pub fn primary_screen() -> (i32, i32) {
        // SAFETY: two metric queries by value; each reads no memory of ours.
        unsafe {
            (
                GetSystemMetrics(SM_CX_SCREEN),
                GetSystemMetrics(SM_CY_SCREEN),
            )
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

    /// A client-area point of `hwnd` in desktop coordinates, which is what a
    /// touch injected from another process is addressed by.
    #[must_use]
    pub fn client_to_screen(hwnd: Handle, x: i32, y: i32) -> Point {
        let mut point = Point { x, y };
        // SAFETY: `point` is a live, initialised `POINT` converted in place, and
        // `hwnd` is a window this process created and has not destroyed.
        unsafe { ClientToScreen(hwnd, &raw mut point) };
        point
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

/// A running `crcbl-e2e-win32-clip hold`: another process with the clipboard
/// open.
struct Holder {
    child: Child,
    stdout: BufReader<std::process::ChildStdout>,
    /// When the peer said it had the clipboard open.
    since: Instant,
}

impl Holder {
    /// Starts the peer holding the clipboard for `span`, and returns once it
    /// says the clipboard is open.
    ///
    /// Read on this thread, blocking, rather than through [`drain`]'s polled
    /// list: the time between the peer's `holding` line and this test acting on
    /// it is time the hold is running out, and a poll interval would be spent
    /// out of it for nothing.
    fn start(span: Duration) -> Self {
        let mut child = Command::new(env!("CARGO_BIN_EXE_crcbl-e2e-win32-clip"))
            .args(["hold", &span.as_millis().to_string()])
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()
            .expect("crcbl-e2e-win32-clip is a [[bin]] of this package under win32-e2e");
        let mut stdout = BufReader::new(child.stdout.take().expect("stdout was piped"));
        let mut line = String::new();
        stdout
            .read_line(&mut line)
            .expect("the clipboard peer's stdout is readable");
        let since = Instant::now();
        if !line.contains("crcbl-e2e-win32-clip: holding") {
            let output = child.wait_with_output().expect("the peer exits");
            panic!(
                "the clipboard peer did not take the clipboard: it said {line:?}, then {:?}",
                String::from_utf8_lossy(&output.stderr)
            );
        }
        Self {
            child,
            stdout,
            since,
        }
    }

    /// Waits for the peer to give the clipboard back and exit cleanly.
    fn finish(mut self) {
        let mut rest = String::new();
        std::io::Read::read_to_string(&mut self.stdout, &mut rest)
            .expect("the clipboard peer's stdout is readable");
        let output = self.child.wait_with_output().expect("the peer exits");
        assert!(
            output.status.success() && rest.contains("crcbl-e2e-win32-clip: released"),
            "the clipboard peer did not release cleanly ({:?}): it said {rest:?} and {:?}",
            output.status,
            String::from_utf8_lossy(&output.stderr)
        );
    }
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
        // Composed commits, which the dead-key test below proves.
        ShellCaps::TEXT_IME,
    ] {
        assert!(caps.contains(present), "{present:?} is implemented");
    }
    assert!(
        caps.contains(ShellCaps::RAW_POINTER_MOTION),
        "RegisterRawInputDevices was refused on this runner, which is a finding about the runner"
    );
    assert!(caps.has_mouselook(), "both halves, which is the point");

    assert!(
        caps.contains(ShellCaps::TOUCH),
        "WM_POINTER touch, which the finger tests below prove"
    );
    assert!(
        !caps.contains(ShellCaps::HW_UPSCALE),
        "a plain HWND presents at its own size"
    );

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

/// A window hidden while borderless stays hidden across a second borderless
/// request and across the windowed restore.
///
/// `WS_VISIBLE` is live state, never the snapshot captured at the first
/// borderless entry: the second request must not re-show the window, and the
/// restore must not either — `SetWindowPlacement` re-shows according to
/// `showCmd`.
#[test]
#[ignore = "needs a Windows desktop; run tests/run-win32-e2e.ps1"]
fn a_hidden_window_stays_hidden_across_a_second_borderless_request_and_back() {
    let mut session = Session::open();
    let window = session.window("hidden across modes");

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

    session
        .shell
        .set_visible(window, false)
        .expect("set_visible");
    session.pump();
    assert!(
        !session.shell.window_state(window).expect("state").visible,
        "hiding while borderless takes effect"
    );

    // **The regression:** a second borderless request read the `WS_VISIBLE`
    // bit from the first entry's snapshot and showed the window again.
    session
        .shell
        .set_mode(window, DisplayMode::Borderless { monitor: None })
        .expect("set_mode");
    session.pump();
    assert!(
        !session.shell.window_state(window).expect("state").visible,
        "a second borderless request must not re-show a window hidden in between"
    );

    // And neither may the restore: the placement's `showCmd` re-shows what the
    // window was at the first borderless entry.
    session
        .shell
        .set_mode(window, DisplayMode::Windowed)
        .expect("set_mode");
    session.pump();
    assert!(
        !session.shell.window_state(window).expect("state").visible,
        "the windowed restore must not re-show a window hidden while borderless"
    );

    // Put the window back, so the session's `Drop` hides and destroys a window
    // in the state the other mode tests leave theirs.
    session
        .shell
        .set_visible(window, true)
        .expect("set_visible");
    session.pump();
    assert!(
        session.shell.window_state(window).expect("state").visible,
        "showing it again works"
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
        // Refresh is either the documented zero — "the backend cannot
        // determine it", which is what a virtual output like this runner's
        // desktop honestly reports — or a plausible rate. The unit is
        // millihertz: a value in whole hertz here (59 instead of 59940) would
        // fail this band as loudly as a nonsense rate would.
        assert!(
            monitor.refresh_millihertz == 0
                || (10_000..=1_000_000).contains(&monitor.refresh_millihertz),
            "{}'s refresh {} mHz is not a plausible display rate",
            monitor.name,
            monitor.refresh_millihertz,
        );
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

    // `SendInput`'s raw reports carry a null `hDevice`, so an injected key is
    // attributed to the keyboard fallback and never to a real device's id.
    let devices: Vec<DeviceId> = session
        .events
        .iter()
        .filter_map(|event| match event {
            ShellEvent::Key { device, .. } => Some(*device),
            _ => None,
        })
        .collect();
    assert!(
        devices.iter().all(|&device| device == DeviceId(1)),
        "injected keys carry the keyboard fallback: {devices:?}"
    );

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

/// A dead key composes with the next key into one committed character.
///
/// This is what [`ShellCaps::TEXT_IME`] claims on every backend that sets it:
/// composed text reaches the engine through the platform's input method, not
/// that a pre-edit exists. On US-International `'` is a dead acute accent, so
/// `'` then `e` must commit exactly `é` — never the bare accent, never `'e`. The
/// system does the composing (`TranslateMessage` posts `WM_DEADCHAR` and then a
/// `WM_CHAR` carrying the result); what this proves is that the backend lets it,
/// which calling `ToUnicode` on the way through would not, because that
/// consumes the pending accent.
#[test]
#[ignore = "needs a Windows desktop; run tests/run-win32-e2e.ps1"]
fn a_dead_key_typed_by_another_process_composes_with_the_next_key() {
    let mut session = Session::open();
    let window = session.window("dead key");
    session.foreground(window);
    // After the foreground, not before: with Windows' default of one input
    // method for every app, taking the foreground re-syncs this thread's layout
    // to the user's, which would undo an earlier activation.
    let _layout = desktop::ThreadLayout::activate("00020409")
        .expect("US-International ships with every Windows install");
    session.take_names();

    let mut sender = Sender::start();
    sender.send(&format!("key {:#x}", scancode::QUOTE));
    sender.send(&format!("key {:#x}", scancode::E));
    session.pump_until("both keystrokes and their releases", |session| {
        session.keys().len() >= 4
    });

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
        "é",
        "the dead accent composes with the vowel into one character; the events were {:?} and \
         the sender said {:?}",
        session.names(),
        sender.lines()
    );
}

/// A second injected press of a key that is already down is **never a fresh
/// press**, and the release that follows is never a repeat.
///
/// # Windows answers this sequence two ways, and which one depends on raw input
///
/// Two `down`s and an `up`, with no release between the downs. The first CI run
/// reported one `Pressed` for the two downs: the second, for a key already down,
/// produced no `WM_KEYDOWN` at all. That is what happens while **no** raw
/// keyboard registration is active. Once one is, and this backend registers the
/// keyboard to name the device behind each key (see `win32::devices`), Windows
/// reports the second down as a `WM_KEYDOWN` with the previous-state bit set:
/// an auto-repeat, which is how a real keyboard's held key arrives. Measured on
/// a desktop on 2026-09-21: ten of ten runs dropped the second down with the
/// keyboard registration switched off, and ten of ten reported it as a repeat
/// with it on. One run earlier that day repeated with nothing of ours
/// registered; another program on that desktop registering raw keyboard input
/// is the likely cause, not verified.
///
/// So the test asserts what holds either way and is the backend's to get
/// right: exactly one press that is not a repeat, at most one more press and
/// that one marked as a repeat, and a release that is not a repeat. A backend
/// that decoded the second down as a fresh press, or read the previous-state
/// bit on a release (where it is always set), fails.
///
/// Genuine typematic timing comes from a physically held key, which no amount
/// of `SendInput` reproduces; `tests/bin/hands_on_win32.rs` covers it with a
/// person at the keyboard.
#[test]
#[ignore = "needs a Windows desktop; run tests/run-win32-e2e.ps1"]
fn a_second_injected_press_of_a_held_key_is_never_a_fresh_press() {
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
    let dropped = [
        (ButtonState::Pressed, false),
        (ButtonState::Released, false),
    ];
    let repeated = [
        (ButtonState::Pressed, false),
        (ButtonState::Pressed, true),
        (ButtonState::Released, false),
    ];
    assert!(
        states == dropped || states == repeated,
        "one fresh press, the second down either dropped or reported as a repeat, and a \
         release that is never a repeat however long the key was held: {states:?}; the \
         sender said {:?}",
        sender.lines()
    );
}

/// A bare Alt tap does not swallow the keys that follow it.
///
/// `DefWindowProc` answers the release of an Alt pressed on its own (or of F10)
/// with `WM_SYSCOMMAND`/`SC_KEYMENU` and an `lParam` of zero, and handing that
/// on enters the system's modal menu loop: every key after it is the menu's
/// until Alt is tapped again. A game that binds Alt-chords, or that a player
/// merely brushes Alt in, loses its keyboard. Found by EW, whose bindings
/// include Alt+R, Alt+T and Alt-click.
#[test]
#[ignore = "needs a Windows desktop; run tests/run-win32-e2e.ps1"]
fn a_bare_alt_tap_leaves_the_keyboard_with_the_window() {
    let mut session = Session::open();
    let window = session.window("alt tap");
    session.foreground(window);
    session.take_names();

    let mut sender = Sender::start();
    sender.send(&format!("key {}", scancode::ALT));
    sender.send(&format!("key {}", scancode::A));
    session.pump_until("the key after the Alt tap", |session| {
        session.keys().iter().any(|&(code, ..)| code == scancode::A)
    });

    let keys = session.keys();
    let pressed: Vec<u32> = keys
        .iter()
        .filter(|key| key.3 == ButtonState::Pressed)
        .map(|key| key.0)
        .collect();
    assert_eq!(
        pressed,
        vec![scancode::ALT, scancode::A],
        "the Alt tap and the key after it both reach the window; the sender said {:?}",
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
    // The sender's notch is away from the user; the user's scroll-direction
    // setting decides which way it reaches every window, this one included.
    let reversed = desktop::wheel_reversed();
    let away = if reversed { -1.0 } else { 1.0 };
    assert_eq!(
        wheels,
        vec![ScrollDelta::Lines { x: 0.0, y: away }],
        "one notch is one line, and WHEEL_DELTA is not one (wheel reversed in Settings: \
         {reversed}): {:?}, sender said {:?}",
        session.names(),
        sender.lines()
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

/// Both thumb buttons, clicked from another process, arrive as the buttons
/// they are.
///
/// They share one message pair, `WM_XBUTTONDOWN`/`WM_XBUTTONUP`, and are told
/// apart only by the high word of `wParam`, so a backend that read the wrong
/// half or swapped the two ids reports every thumb click as the same button, or
/// as the other one. The hands-on check could only ask a person for Back; this
/// is the one place Forward is pressed at all.
#[test]
#[ignore = "needs a Windows desktop; run tests/run-win32-e2e.ps1"]
fn the_back_and_forward_buttons_clicked_by_another_process_are_told_apart() {
    let mut session = Session::open();
    let window = session.window("thumb buttons");
    session.foreground(window);
    // Parked inside the client area, as for the other clicks: an X button
    // message goes to the window under the cursor.
    session
        .shell
        .warp_pointer(window, PhysicalPoint::new(100.0, 80.0))
        .expect("POINTER_WARP is claimed");
    session.settle();
    session.events.clear();

    let mut sender = Sender::start();
    sender.send("click back");
    sender.send("click forward");
    session.pump_until("both thumb clicks", |session| buttons(session).len() >= 4);
    let clicked: Vec<_> = buttons(&session)
        .into_iter()
        .map(|(button, state, _)| (button, state))
        .collect();
    assert_eq!(
        clicked,
        vec![
            (PointerButton::Back, ButtonState::Pressed),
            (PointerButton::Back, ButtonState::Released),
            (PointerButton::Forward, ButtonState::Pressed),
            (PointerButton::Forward, ButtonState::Released),
        ],
        "XBUTTON1 is Back and XBUTTON2 is Forward: {:?}, sender said {:?}",
        session.names(),
        sender.lines()
    );
}

/// Every touch event delivered so far, flattened for assertion.
fn touches(session: &Session) -> Vec<(ContactId, TouchPhase, PhysicalPoint)> {
    session
        .events
        .iter()
        .filter_map(|event| match event {
            ShellEvent::Touch {
                contact,
                phase,
                position,
                ..
            } => Some((*contact, *phase, *position)),
            _ => None,
        })
        .collect()
}

/// Every button edge delivered so far, with where it was.
fn buttons(session: &Session) -> Vec<(PointerButton, ButtonState, Option<PhysicalPoint>)> {
    session
        .events
        .iter()
        .filter_map(|event| match event {
            ShellEvent::Button {
                button,
                state,
                position,
                ..
            } => Some((*button, *state, *position)),
            _ => None,
        })
        .collect()
}

/// Sends a `touch` command aimed at a client point of `window`.
fn touch_at(session: &Session, sender: &mut Sender, verb: &str, window: WindowId, x: i32, y: i32) {
    let at = desktop::client_to_screen(session.hwnd(window), x, y);
    sender.send(&format!("touch {verb} {} {}", at.x, at.y));
}

/// One finger, injected from another process, is one contact from landing to
/// lifting, and it also drives the pointer.
///
/// # What this settles
///
/// `InjectTouchInput` needs no touchscreen, which is what makes this runnable
/// on a desktop nobody can touch. The backlog could not say whether the CI
/// runner allows it; a pass on a machine with no digitizer is the answer for
/// the API, and `win32-e2e` running this is the answer for the runner.
///
/// # The emulated pointer is Windows', not this backend's
///
/// [`ShellEvent::Touch`] obliges a backend that reports touch to also report the
/// primary contact as pointer motion and buttons. This backend does not write
/// that emulation: it passes every `WM_POINTER*` on to `DefWindowProc`, which
/// synthesizes the legacy mouse messages the ordinary arms already record. The
/// button assertions are what prove the pass-through happened.
#[test]
#[ignore = "needs a Windows desktop; run tests/run-win32-e2e.ps1"]
fn a_finger_touched_by_another_process_is_one_contact_and_drives_the_pointer() {
    let mut session = Session::open();
    let window = session.window("touch");
    session.foreground(window);
    session.settle();
    session.events.clear();

    let mut sender = Sender::start();
    touch_at(&session, &mut sender, "down 0", window, 100, 80);
    touch_at(&session, &mut sender, "move 0", window, 140, 110);
    sender.send("touch up 0");
    session.pump_until("the finger to lift", |session| {
        touches(session)
            .iter()
            .any(|&(_, phase, _)| phase.ends_contact())
    });
    // The emulated release can trail the contact's own end by a message or two.
    session.settle();

    let contacts = touches(&session);
    let (first, _, landed) = *contacts.first().expect("just waited for it");
    let (_, last_phase, lifted) = *contacts.last().expect("just waited for it");
    assert!(
        contacts.iter().all(|&(contact, _, _)| contact == first),
        "one finger is one contact: {contacts:?}"
    );
    assert_eq!(contacts[0].1, TouchPhase::Began, "{contacts:?}");
    assert_eq!(last_phase, TouchPhase::Ended, "{contacts:?}");
    assert!(
        contacts[1..contacts.len() - 1]
            .iter()
            .all(|&(_, phase, _)| phase == TouchPhase::Moved),
        "only moves between landing and lifting: {contacts:?}"
    );
    assert_eq!(
        landed,
        PhysicalPoint::new(100.0, 80.0),
        "the finger landed on the client pixel it was aimed at, so the screen-to-client \
         conversion ran: {contacts:?}; the sender said {:?}",
        sender.lines()
    );
    assert_eq!(lifted, PhysicalPoint::new(140.0, 110.0), "{contacts:?}");

    let left: Vec<_> = buttons(&session)
        .into_iter()
        .filter(|&(button, _, _)| button == PointerButton::Left)
        .map(|(_, state, _)| state)
        .collect();
    assert_eq!(
        left,
        vec![ButtonState::Pressed, ButtonState::Released],
        "the primary contact is also a left click, which DefWindowProc synthesizes only if the \
         pointer messages reach it; the events were {:?}",
        session.names()
    );
}

/// A second finger down at the same time is its own contact, and moves no
/// pointer.
///
/// The seam's reason for [`ShellEvent::Touch`] existing at all: a game reading
/// two thumbs needs two identities, and a second finger reported through the
/// pointer would be the first one teleporting.
#[test]
#[ignore = "needs a Windows desktop; run tests/run-win32-e2e.ps1"]
fn a_second_finger_is_its_own_contact_and_moves_no_pointer() {
    let mut session = Session::open();
    let window = session.window("two fingers");
    session.foreground(window);
    session.settle();
    session.events.clear();

    let mut sender = Sender::start();
    touch_at(&session, &mut sender, "down 0", window, 60, 60);
    touch_at(&session, &mut sender, "down 1", window, 300, 200);
    touch_at(&session, &mut sender, "move 1", window, 320, 220);
    sender.send("touch up 1");
    sender.send("touch up 0");
    session.pump_until("both fingers to lift", |session| {
        touches(session)
            .iter()
            .filter(|&&(_, phase, _)| phase.ends_contact())
            .count()
            >= 2
    });
    session.settle();

    let contacts = touches(&session);
    let first = contacts[0].0;
    let second = contacts
        .iter()
        .map(|&(contact, _, _)| contact)
        .find(|&contact| contact != first)
        .unwrap_or_else(|| panic!("two fingers are two contacts: {contacts:?}"));
    let of = |wanted: ContactId| -> Vec<(TouchPhase, PhysicalPoint)> {
        contacts
            .iter()
            .filter(|&&(contact, _, _)| contact == wanted)
            .map(|&(_, phase, position)| (phase, position))
            .collect()
    };
    let first_path = of(first);
    let second_path = of(second);
    assert_eq!(
        first_path.first().map(|&(phase, _)| phase),
        Some(TouchPhase::Began)
    );
    assert_eq!(
        first_path.last().map(|&(phase, _)| phase),
        Some(TouchPhase::Ended)
    );
    // Repeated in every frame while the other finger moves, and never moved.
    assert!(
        first_path
            .iter()
            .all(|&(_, position)| position == PhysicalPoint::new(60.0, 60.0)),
        "the finger that held still stayed where it landed: {first_path:?}"
    );
    assert_eq!(
        second_path.first(),
        Some(&(TouchPhase::Began, PhysicalPoint::new(300.0, 200.0)))
    );
    assert_eq!(
        second_path.last(),
        Some(&(TouchPhase::Ended, PhysicalPoint::new(320.0, 220.0))),
        "{second_path:?}"
    );

    // Given the second finger, `DefWindowProc` read the pair as a pinch and
    // synthesized a Ctrl press and release: a key nobody pressed, on whatever
    // the game bound to Ctrl. The backend keeps secondary contacts away from it.
    let keys = session.keys();
    assert!(
        keys.is_empty(),
        "two fingers type nothing; the keys delivered were {keys:?}"
    );

    // Only the primary contact is emulated: one click, where the first finger
    // was, and nothing anywhere near the second one.
    let clicks = buttons(&session);
    assert_eq!(
        clicks
            .iter()
            .map(|&(button, state, _)| (button, state))
            .collect::<Vec<_>>(),
        vec![
            (PointerButton::Left, ButtonState::Pressed),
            (PointerButton::Left, ButtonState::Released),
        ],
        "only the first finger is a mouse: {clicks:?}; the events were {:#?}",
        session
            .events
            .iter()
            .filter(|event| !matches!(event, ShellEvent::Touch { .. }))
            .collect::<Vec<_>>()
    );
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

/// A desktop pixel as the normalized `0..=65535` coordinate a
/// `MOUSEEVENTF_ABSOLUTE` move over the primary monitor takes.
///
/// Rounded to the nearest unit rather than truncated, so the pixel the system
/// maps it back to is the one asked for.
fn normalized(pixel: i32, extent: i32) -> i32 {
    const RANGE: i64 = 65_536;
    let scaled = (i64::from(pixel) * RANGE + i64::from(extent) / 2) / i64::from(extent);
    i32::try_from(scaled.clamp(0, RANGE - 1)).expect("clamped into the normalized range")
}

/// An **absolute** raw report — what a remote-desktop session or a tablet
/// sends — is differenced into relative motion rather than read as a delta.
///
/// Read as a delta, the first absolute report moves a first-person camera by
/// tens of thousands of pixels; `pointer::RawMotion` is what stops that, and
/// until this test no absolute report had ever reached it.
///
/// Two moves, and each one says something the other cannot. The first report
/// of an absolute run has nothing to subtract from, so it must produce **no**
/// raw delta at all — which is also what shows the report arrived absolute: had
/// the system turned the injected move into a relative one, the first move
/// would already be a delta. The second must be the distance between the two
/// points, in pixels, not in normalized units.
#[test]
#[ignore = "needs a Windows desktop; run tests/run-win32-e2e.ps1"]
fn an_injected_absolute_move_is_differenced_into_raw_motion() {
    /// The two client points the moves land on, and so the delta between them.
    const FROM: (i32, i32) = (90, 70);
    const TO: (i32, i32) = (150, 110);
    /// How far the landing may be off, in pixels, from the rounding of a
    /// normalized coordinate each way.
    const SLACK: f64 = 2.0;

    let mut session = Session::open();
    let window = session.window("absolute");
    session.foreground(window);
    session
        .shell
        .warp_pointer(window, PhysicalPoint::new(100.0, 80.0))
        .expect("POINTER_WARP is claimed");
    session.settle();
    session.events.clear();

    let hwnd = session.hwnd(window);
    let (width, height) = desktop::primary_screen();
    let aim = |(x, y): (i32, i32)| {
        let at = desktop::client_to_screen(hwnd, x, y);
        format!(
            "abs {} {}",
            normalized(at.x, width),
            normalized(at.y, height)
        )
    };
    let raw_deltas = |session: &Session| -> Vec<(f64, f64)> {
        session
            .events
            .iter()
            .filter_map(|event| match event {
                ShellEvent::PointerMotion {
                    raw_delta: Some(delta),
                    ..
                } => Some(*delta),
                _ => None,
            })
            .collect()
    };

    let mut sender = Sender::start();
    sender.send(&aim(FROM));
    session.pump_until("the pointer to follow the first absolute move", |session| {
        session
            .events
            .iter()
            .any(|event| matches!(event, ShellEvent::PointerMotion { .. }))
    });
    session.settle();
    let landed = session.events.iter().rev().find_map(|event| match event {
        ShellEvent::PointerMotion { abs: Some(at), .. } => Some(*at),
        _ => None,
    });
    assert!(
        landed.is_some_and(|at| (at.x - f64::from(FROM.0)).abs() <= SLACK
            && (at.y - f64::from(FROM.1)).abs() <= SLACK),
        "the absolute move put the pointer on the client point {FROM:?} it was aimed at, not \
         {landed:?}; the sender said {:?}",
        sender.lines()
    );
    assert_eq!(
        raw_deltas(&session),
        vec![],
        "the first absolute report has nothing to difference against, so it moves the pointer \
         and reports no raw motion; a delta here means the report arrived relative, or was read \
         as a delta. The events were {:?}, the sender said {:?}",
        session.names(),
        sender.lines()
    );

    session.events.clear();
    sender.send(&aim(TO));
    session.pump_until("raw motion from the second absolute move", |session| {
        !raw_deltas(session).is_empty()
    });
    session.settle();
    let deltas = raw_deltas(&session);
    let (dx, dy) = (f64::from(TO.0 - FROM.0), f64::from(TO.1 - FROM.1));
    assert!(
        deltas.len() == 1 && (deltas[0].0 - dx).abs() <= SLACK && (deltas[0].1 - dy).abs() <= SLACK,
        "one report, differenced into the ({dx}, {dy}) pixels between the two points: {deltas:?} \
         on a primary monitor of {width}x{height}; the sender said {:?}",
        sender.lines()
    );
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

/// Files another process copied, the way Explorer copies them, read as a
/// `text/uri-list`.
///
/// Explorer's "copy" publishes `CF_HDROP` and no registered `text/uri-list`,
/// so before this was read a paste of copied files answered `Empty`. The two
/// names are the shapes a URI encoder gets wrong: a space, and a name outside
/// ASCII with an astral character in it. What has to hold is that
/// [`parse_uri_list`] — what a consumer calls — hands back the very paths the
/// peer copied.
#[test]
#[ignore = "needs a Windows desktop; run tests/run-win32-e2e.ps1"]
fn files_another_process_copied_read_as_a_uri_list() {
    const COPIED: [&str; 2] = [r"C:\crcbl e2e\My Scene.ron", r"C:\プロジェクト\café 🎮.png"];
    let mut session = Session::open();
    let window = session.window("paste files");

    clip(&["put-files", COPIED[0], COPIED[1]]);
    let request = session
        .shell
        .clipboard_request(window, MimeType::UriList)
        .expect("CLIPBOARD is claimed");
    let (mime, content) = session.clipboard_answer(request);
    assert!(mime.matches(MimeType::UriList));
    let list = content
        .text()
        .unwrap_or_else(|| panic!("a CF_HDROP answers a UTF-8 uri-list, not {content:?}"));
    assert!(
        list.starts_with("file:///C:/crcbl%20e2e/My%20Scene.ron\r\n"),
        "RFC 2483 lines, CRLF-terminated, with the space escaped: {list:?}"
    );
    assert!(
        list.is_ascii() && list.ends_with("\r\n") && list.lines().count() == COPIED.len(),
        "one URI per file, the non-ASCII name percent-encoded: {list:?}"
    );
    assert_eq!(
        parse_uri_list(list.as_bytes()),
        COPIED.map(PathBuf::from),
        "the URIs decode back to the paths that were copied"
    );

    // A registered `text/uri-list`, which is what a uri-list-aware
    // application publishes, is still read byte for byte.
    clip(&[
        "put",
        MimeType::UriList.as_str(),
        "file:///C:/registered.ron",
    ]);
    let request = session
        .shell
        .clipboard_request(window, MimeType::UriList)
        .expect("CLIPBOARD is claimed");
    let (_, content) = session.clipboard_answer(request);
    assert_eq!(content.text(), Some("file:///C:/registered.ron"));
}

/// The `file` lines of a peer `get-files`, and its `effect` line; `None` when
/// the clipboard held no `CF_HDROP`.
fn clip_files() -> Option<(Vec<String>, String)> {
    let printed = clip(&["get-files"]);
    if printed.contains("crcbl-e2e-win32-clip: absent") {
        return None;
    }
    let files = printed
        .lines()
        .filter_map(|line| line.strip_prefix("crcbl-e2e-win32-clip: file "))
        .map(str::to_owned)
        .collect();
    let effect = printed
        .lines()
        .find_map(|line| line.strip_prefix("crcbl-e2e-win32-clip: effect "))
        .unwrap_or_else(|| panic!("the peer read a CF_HDROP and reported no effect: {printed:?}"))
        .to_owned();
    Some((files, effect))
}

/// Files we copied as a `text/uri-list` are a `CF_HDROP` another process reads
/// with `DragQueryFileW`, which is what Explorer's paste does.
///
/// The list carries what a file list cannot — a comment line and an `https:`
/// URI — between the two files, and those are left out of the `CF_HDROP`
/// while the registered format beside it keeps every byte. The
/// `Preferred DropEffect` is `DROPEFFECT_COPY` (1), so a paste in Explorer
/// copies rather than moves. A list naming no Windows file at all publishes
/// no `CF_HDROP`, rather than an empty one.
#[test]
#[ignore = "needs a Windows desktop; run tests/run-win32-e2e.ps1"]
fn files_we_copied_as_a_uri_list_are_a_file_list_another_process_reads() {
    const LIST: &str = "file:///C:/crcbl%20e2e/My%20Scene.ron\r\n\
                        # a comment\r\n\
                        https://example.com/x\r\n\
                        file:///C:/%E3%83%97%E3%83%AD%E3%82%B8%E3%82%A7%E3%82%AF%E3%83%88/\
                        caf%C3%A9%20%F0%9F%8E%AE.png\r\n";
    let mut session = Session::open();
    let window = session.window("copy files");

    session
        .shell
        .clipboard_offer(
            window,
            &[ClipboardOffer {
                mime: MimeType::UriList,
                bytes: LIST.as_bytes(),
            }],
        )
        .expect("Win32 needs no recent user interaction to claim the clipboard");
    let (files, effect) =
        clip_files().expect("a uri-list naming Windows files is published as CF_HDROP");
    assert_eq!(
        files,
        [r"C:\crcbl e2e\My Scene.ron", r"C:\プロジェクト\café 🎮.png"],
        "each file: URI as the path it names, in order, and nothing for the comment or the URL"
    );
    assert_eq!(effect, "1", "Preferred DropEffect is DROPEFFECT_COPY");
    // Read as the whole of the peer's output rather than through `clip_text`,
    // which keeps one line and this payload is several.
    let printed = clip(&["get", MimeType::UriList.as_str()]);
    assert!(
        printed.ends_with(&format!("crcbl-e2e-win32-clip: text {LIST}\n")),
        "the registered format still carries the offer byte for byte: {printed:?}"
    );

    session
        .shell
        .clipboard_offer(
            window,
            &[ClipboardOffer {
                mime: MimeType::UriList,
                bytes: b"https://example.com/x\r\nfile:///tmp/posix\r\n",
            }],
        )
        .expect("the registered format alone is still a publish");
    assert_eq!(
        clip_files(),
        None,
        "no URI names a Windows file, so there is no file list to paste"
    );
    let printed = clip(&["get", MimeType::UriList.as_str()]);
    assert!(
        printed.ends_with(
            "crcbl-e2e-win32-clip: text https://example.com/x\r\nfile:///tmp/posix\r\n\n"
        ),
        "and the registered format is published without it: {printed:?}"
    );
}

/// When a registered `text/uri-list` and a `CF_HDROP` are both on the
/// clipboard, a `text/uri-list` read answers the registered one, as
/// `win32::clipboard`'s module docs decide.
///
/// The peer publishes the two in one write with different contents, so the
/// answer says which was read.
#[test]
#[ignore = "needs a Windows desktop; run tests/run-win32-e2e.ps1"]
fn a_registered_uri_list_is_read_in_preference_to_a_file_list_beside_it() {
    let mut session = Session::open();
    let window = session.window("paste both");

    clip(&[
        "put-uri-list-and-files",
        "file:///C:/registered.ron",
        r"C:\from the file list.ron",
    ]);
    // Both really are there, or the answer below says nothing about a choice.
    assert_eq!(
        clip_files().map(|(files, _)| files),
        Some(vec![r"C:\from the file list.ron".to_owned()]),
    );
    assert_eq!(
        clip_text(&["get", MimeType::UriList.as_str()]).as_deref(),
        Some("file:///C:/registered.ron"),
    );

    let request = session
        .shell
        .clipboard_request(window, MimeType::UriList)
        .expect("CLIPBOARD is claimed");
    let (mime, content) = session.clipboard_answer(request);
    assert!(mime.matches(MimeType::UriList));
    assert_eq!(
        content.text(),
        Some("file:///C:/registered.ron"),
        "the registered format wins over the CF_HDROP synthesis"
    );
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

/// A clipboard another process holds for a moment is waited for, not reported
/// unavailable.
///
/// `win32::clipboard` retries a refused `OpenClipboard` for `OPEN_BUDGET`
/// because a clipboard manager or Explorer holding it for an instant is
/// routine. The peer holds it for a fraction of that budget, and the read has to
/// come back with the bytes.
///
/// Success alone would not show the retry ran: had the hold already ended when
/// the read began, the first attempt would take the clipboard. The backend logs
/// the attempt count when an open had to wait (`Opened::After`), and that line
/// is what the rounds below look for. A round whose read began after the hold
/// ended is repeated rather than failed, because on a loaded runner that is a
/// scheduling fact, not a backend one.
#[test]
#[ignore = "needs a Windows desktop; run tests/run-win32-e2e.ps1"]
fn a_clipboard_another_process_holds_briefly_is_waited_for() {
    /// Well inside `win32::clipboard::OPEN_BUDGET`, with room to spare for the
    /// peer oversleeping on a loaded runner.
    const SHORT_HOLD: Duration = Duration::from_millis(30);
    /// How many holds to try before concluding no read ever had to wait.
    const ROUNDS: u32 = 5;
    /// The start of the line the backend logs when an open succeeded on a
    /// retry.
    const WAITED: &str = "the clipboard was held by another process for";

    let mut session = Session::open();
    let window = session.window("contended");
    clip(&["put", "text", "under", "contention"]);

    let logs = crcbl_core::log::capture();
    let mut waited_in = None;
    for round in 1..=ROUNDS {
        let holder = Holder::start(SHORT_HOLD);
        let asked_after = holder.since.elapsed();
        let request = session
            .shell
            .clipboard_request(window, MimeType::TextUtf8)
            .expect("CLIPBOARD is claimed");
        holder.finish();
        let (_, content) = session.clipboard_answer(request);
        session.events.clear();
        assert_eq!(
            content.text(),
            Some("under contention"),
            "round {round}: a hold of {SHORT_HOLD:?} is inside the retry budget, so the read \
             waits it out; it was asked {asked_after:?} into the hold and answered {content:?}"
        );
        if logs
            .records()
            .iter()
            .any(|record| record.message.starts_with(WAITED))
        {
            waited_in = Some(round);
            break;
        }
    }
    assert!(
        waited_in.is_some(),
        "in {ROUNDS} rounds no read ever found the clipboard held, so the retry never ran; the \
         shell logged {:?}",
        logs.records()
    );

    // A write under the same contention publishes rather than failing.
    let holder = Holder::start(SHORT_HOLD);
    let offered = session
        .shell
        .clipboard_offer(window, &[ClipboardOffer::text("written under contention")]);
    holder.finish();
    offered.expect("a hold inside the retry budget is waited out by a write as well");
    assert_eq!(
        clip_text(&["get", "text"]).as_deref(),
        Some("written under contention")
    );
}

/// A clipboard another process will not let go of is answered `Unavailable`
/// within the budget, and a write fails with the backend's error rather than
/// waiting.
///
/// Obligation 4's bound, observed: the peer holds the clipboard far longer than
/// `win32::clipboard::OPEN_BUDGET`, and both calls must return while it still
/// holds it. A backend that retried without a bound would sit here until the
/// peer let go, and then answer with the bytes.
#[test]
#[ignore = "needs a Windows desktop; run tests/run-win32-e2e.ps1"]
fn a_clipboard_another_process_will_not_release_is_refused_within_the_budget() {
    /// Far beyond `win32::clipboard::OPEN_BUDGET`, even on a runner slow
    /// enough to stretch each of the backend's retry sleeps several times.
    const LONG_HOLD: Duration = Duration::from_secs(3);

    let mut session = Session::open();
    let window = session.window("refused");

    let holder = Holder::start(LONG_HOLD);
    let request = session
        .shell
        .clipboard_request(window, MimeType::TextUtf8)
        .expect("a refused open is an answer, not an error");
    let offered = session
        .shell
        .clipboard_offer(window, &[ClipboardOffer::text("never published")]);
    let acted_within = holder.since.elapsed();
    let (_, content) = session.clipboard_answer(request);
    holder.finish();

    assert_eq!(
        content,
        ClipboardContent::Unavailable,
        "another process held the clipboard for {LONG_HOLD:?}, so the read gives up and says \
         so; both calls returned {acted_within:?} into the hold"
    );
    assert!(
        matches!(&offered, Err(ShellError::Backend(message))
            if message.contains("another process is holding it")),
        "a write that cannot open the clipboard is the backend error naming why: {offered:?}"
    );
    assert!(
        acted_within < LONG_HOLD,
        "both calls returned while the peer still held the clipboard, which is the bound: \
         {acted_within:?} of {LONG_HOLD:?}"
    );
}
