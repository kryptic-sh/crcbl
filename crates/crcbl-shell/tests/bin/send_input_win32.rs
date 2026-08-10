//! Types and clicks at whatever Windows is delivering input to — a *different*
//! process.
//!
//! ```text
//! crcbl-e2e-win32-input < a stream of commands, one per line
//! ```
//!
//! **Compiled only with the `win32-e2e` feature**, which nothing but
//! `tests/run-win32-e2e.ps1` turns on. The Windows counterpart of
//! `tests/bin/send_key_wayland.rs` and `tests/bin/send_key_x11.rs`, and a
//! `[[bin]]` for the same reason those are: the thing it drives is another
//! program.
//!
//! # Why this cannot be a function in the test
//!
//! `crcbl-shell`'s in-crate Windows suite reaches the window procedure with
//! `SendMessageW`, which runs the **real** procedure against the **real** cached
//! keyboard state — and never touches the message queue. `TranslateMessage` is
//! not on that path, so `WM_CHAR` is never synthesized from a `WM_KEYDOWN`; the
//! `lParam` is this project's idea of what a keyboard driver builds rather than
//! one; and `GetMessageTime` answers for a message that was never posted.
//!
//! `SendInput` is the way in, and it has two properties that decide the shape of
//! this program. It injects into the **session's** input stream rather than into
//! a window, and that stream follows the **foreground** window. So a test that
//! called it would be typing at its own window, from the thread that owns it,
//! with its own queue — which is the one arrangement that proves nothing. This
//! runs somewhere else, and the input has to find its way back.
//!
//! # Why it reads a stream instead of taking the keys as arguments
//!
//! The same reason the X11 sender does: the harness has to type *after* its
//! window is up and has taken the foreground, and a process started at that
//! moment would race its own start-up. Reading a stream lets the suite start it
//! once and decide when.
//!
//! # The command language
//!
//! One command per line. Scan codes are **PS/2 set 1**, which is what the
//! `lParam` of a key message carries and what `win32::keys::key_code` is written
//! against — `A` is `0x1E` (30) and `ArrowUp` is the extended `0xE048`.
//!
//! ```text
//! key <scancode>          a press and its release
//! down <scancode>         a press on its own
//! up <scancode>           a release on its own
//! move <dx> <dy>          relative pointer motion, in mickeys
//! click <left|right|middle>
//! wheel <notches>         positive scrolls away from the user
//! ```
//!
//! Every line is acknowledged on stdout, and the acknowledgement carries the
//! foreground window handle **at the moment of the send**. That is the single
//! most useful thing this program can say: if the input went nowhere, the first
//! question is whether it was aimed at the suite's window or at something else,
//! and a run that only reported "sent" leaves that unanswerable.
//!
//! # Windows only, and it says so out loud
//!
//! `--all-features` turns `win32-e2e` on for every target, so this is built on
//! Linux, macOS and `wasm32` by the lint jobs. They get a `main` that fails and
//! names the reason rather than a `cfg` that quietly compiles to nothing: a
//! helper reporting success on a platform where it cannot have typed anything is
//! the failure this harness exists to avoid.

#[cfg(target_os = "windows")]
use std::io::{BufRead, Write};
use std::process::ExitCode;

#[cfg(not(target_os = "windows"))]
fn main() -> ExitCode {
    eprintln!(
        "crcbl-e2e-win32-input: SendInput is a Win32 call; there is no desktop here to type at"
    );
    ExitCode::FAILURE
}

/// The `user32` surface this needs, hand-written like every other declaration in
/// this crate.
///
/// It is the sender's own rather than `crcbl_shell::win32::ffi`'s: that module
/// is `pub(crate)`, and a test helper reaching into a backend's private ABI
/// table to drive that same backend is a circle. What is declared here is what a
/// *user* of the desktop calls, which is the side of the seam this program is
/// on.
#[cfg(target_os = "windows")]
mod win32 {
    use core::ffi::c_void;

    /// `HWND`.
    pub type Handle = *mut c_void;

    /// `MOUSEINPUT`.
    #[repr(C)]
    #[derive(Clone, Copy, Default)]
    pub struct MouseInput {
        /// Motion or absolute x, depending on the flags.
        pub dx: i32,
        /// Motion or absolute y.
        pub dy: i32,
        /// Wheel delta for `MOUSEEVENTF_WHEEL`, button id for the X buttons.
        pub mouse_data: u32,
        /// `MOUSEEVENTF_*`.
        pub flags: u32,
        /// Zero means "stamp it with the system's own time".
        pub time: u32,
        /// Passed through to `GetMessageExtraInfo`; unused here.
        pub extra_info: usize,
    }

    /// `KEYBDINPUT`.
    #[repr(C)]
    #[derive(Clone, Copy, Default)]
    pub struct KeybdInput {
        /// Virtual key, ignored when `KEYEVENTF_SCANCODE` is set.
        pub vk: u16,
        /// The scan code, which is what this sender always uses.
        pub scan: u16,
        /// `KEYEVENTF_*`.
        pub flags: u32,
        /// Zero means "stamp it with the system's own time".
        pub time: u32,
        /// Passed through to `GetMessageExtraInfo`; unused here.
        pub extra_info: usize,
    }

    /// The `INPUT` union, narrowed to the two arms this sends.
    ///
    /// `HARDWAREINPUT` is the third arm and is smaller than both of these, so
    /// omitting it changes neither the size nor the alignment — which the
    /// assertions below are what actually check.
    #[repr(C)]
    #[derive(Clone, Copy)]
    pub union InputBody {
        /// `INPUT_MOUSE`.
        pub mouse: MouseInput,
        /// `INPUT_KEYBOARD`.
        pub keyboard: KeybdInput,
    }

    /// `INPUT`.
    #[repr(C)]
    #[derive(Clone, Copy)]
    pub struct Input {
        /// `INPUT_MOUSE` or `INPUT_KEYBOARD`.
        pub kind: u32,
        /// The arm `kind` selects.
        pub body: InputBody,
    }

    // The layout `SendInput` validates by size, checked at compile time on the
    // 64-bit Windows targets this engine claims. A `cbSize` that disagrees with
    // the system's is the classic way for this call to fail with
    // `ERROR_INVALID_PARAMETER` and no other symptom, and passing
    // `size_of::<Input>()` is only correct if the structure is right.
    #[cfg(target_pointer_width = "64")]
    const _: () = {
        assert!(
            size_of::<MouseInput>() == 32,
            "MOUSEINPUT on 64-bit Windows"
        );
        assert!(
            size_of::<KeybdInput>() == 24,
            "KEYBDINPUT on 64-bit Windows"
        );
        assert!(size_of::<Input>() == 40, "INPUT on 64-bit Windows");
        assert!(align_of::<Input>() == 8, "ULONG_PTR alignment");
    };

    /// `INPUT_MOUSE`.
    pub const INPUT_MOUSE: u32 = 0;
    /// `INPUT_KEYBOARD`.
    pub const INPUT_KEYBOARD: u32 = 1;

    /// `KEYEVENTF_EXTENDEDKEY` — the `E0` prefix, as a flag.
    pub const KEYEVENTF_EXTENDED: u32 = 0x0001;
    /// `KEYEVENTF_KEYUP`.
    pub const KEYEVENTF_KEY_UP: u32 = 0x0002;
    /// `KEYEVENTF_SCANCODE` — identify the key by position, not by virtual key.
    pub const KEYEVENTF_SCANCODE: u32 = 0x0008;

    /// `MOUSEEVENTF_MOVE` — relative motion.
    pub const MOUSEEVENTF_MOVE: u32 = 0x0001;
    /// `MOUSEEVENTF_LEFTDOWN`.
    pub const MOUSEEVENTF_LEFT_DOWN: u32 = 0x0002;
    /// `MOUSEEVENTF_LEFTUP`.
    pub const MOUSEEVENTF_LEFT_UP: u32 = 0x0004;
    /// `MOUSEEVENTF_RIGHTDOWN`.
    pub const MOUSEEVENTF_RIGHT_DOWN: u32 = 0x0008;
    /// `MOUSEEVENTF_RIGHTUP`.
    pub const MOUSEEVENTF_RIGHT_UP: u32 = 0x0010;
    /// `MOUSEEVENTF_MIDDLEDOWN`.
    pub const MOUSEEVENTF_MIDDLE_DOWN: u32 = 0x0020;
    /// `MOUSEEVENTF_MIDDLEUP`.
    pub const MOUSEEVENTF_MIDDLE_UP: u32 = 0x0040;
    /// `MOUSEEVENTF_WHEEL`.
    pub const MOUSEEVENTF_WHEEL: u32 = 0x0800;
    /// `WHEEL_DELTA` — one notch, which is not one.
    pub const WHEEL_DELTA: i32 = 120;

    /// What `win32::keys::scancode` ORs into an extended key's code, and
    /// therefore how one is spelled on this program's stdin.
    pub const EXTENDED_PREFIX: u32 = 0xE000;

    #[link(name = "user32")]
    unsafe extern "system" {
        /// Injects events into the session's input stream.
        ///
        /// Returns how many were inserted, which is less than asked for when the
        /// system blocked them — `UIPI` refusing a lower-integrity process, or a
        /// locked workstation. A short return is the failure this program is
        /// there to report rather than to shrug off.
        pub fn SendInput(count: u32, inputs: *const Input, size: i32) -> u32;
        /// The window the input stream is currently pointed at.
        pub fn GetForegroundWindow() -> Handle;
    }

    #[link(name = "kernel32")]
    unsafe extern "system" {
        /// The calling thread's last error code.
        pub fn GetLastError() -> u32;
    }
}

#[cfg(target_os = "windows")]
fn main() -> ExitCode {
    say(&format!("ready fg={}", foreground()));

    for line in std::io::stdin().lock().lines() {
        let line = match line {
            Ok(line) => line,
            Err(error) => {
                eprintln!("crcbl-e2e-win32-input: could not read stdin: {error}");
                return ExitCode::FAILURE;
            }
        };
        let command = line.trim();
        if command.is_empty() {
            continue;
        }
        match run(command) {
            Ok(()) => say(&format!("sent {command:?} fg={}", foreground())),
            Err(problem) => {
                eprintln!("crcbl-e2e-win32-input: {command:?}: {problem}");
                return ExitCode::from(2);
            }
        }
    }
    ExitCode::SUCCESS
}

/// Runs one command line, or says what was wrong with it.
#[cfg(target_os = "windows")]
fn run(command: &str) -> Result<(), String> {
    let mut words = command.split_whitespace();
    let verb = words.next().unwrap_or_default();
    match verb {
        "key" | "down" | "up" => {
            let scancode = scancode(words.next())?;
            match verb {
                "down" => key(scancode, true),
                "up" => key(scancode, false),
                _ => key(scancode, true).and_then(|()| key(scancode, false)),
            }
        }
        "move" => {
            let dx = number(words.next(), "dx")?;
            let dy = number(words.next(), "dy")?;
            mouse(win32::MOUSEEVENTF_MOVE, 0, dx, dy)
        }
        "click" => {
            let (down, up) = match words.next() {
                Some("left") => (win32::MOUSEEVENTF_LEFT_DOWN, win32::MOUSEEVENTF_LEFT_UP),
                Some("right") => (win32::MOUSEEVENTF_RIGHT_DOWN, win32::MOUSEEVENTF_RIGHT_UP),
                Some("middle") => (win32::MOUSEEVENTF_MIDDLE_DOWN, win32::MOUSEEVENTF_MIDDLE_UP),
                other => return Err(format!("{other:?} is not left, right or middle")),
            };
            mouse(down, 0, 0, 0).and_then(|()| mouse(up, 0, 0, 0))
        }
        "wheel" => {
            let notches: i32 = number(words.next(), "notches")?;
            let delta = notches
                .checked_mul(win32::WHEEL_DELTA)
                .ok_or("that many notches does not fit in a wheel delta")?;
            // `mouseData` is a signed count carried in an unsigned field, which
            // is what makes a scroll towards the user expressible at all.
            mouse(win32::MOUSEEVENTF_WHEEL, delta as u32, 0, 0)
        }
        other => Err(format!(
            "{other:?} is not one of key, down, up, move, click, wheel"
        )),
    }
}

/// A PS/2 set 1 scan code, extended codes included.
///
/// `0xE0`-prefixed keys are spelled the way `win32::keys::scancode` reports
/// them — `0xE048` for `ArrowUp` — and are split back into a low byte and the
/// extended flag here, because that is the shape the wire has.
#[cfg(target_os = "windows")]
fn scancode(word: Option<&str>) -> Result<u32, String> {
    let word = word.ok_or("a scan code is required")?;
    let code = if let Some(hex) = word.strip_prefix("0x") {
        u32::from_str_radix(hex, 16).map_err(|_| format!("{word:?} is not a hex scan code"))?
    } else {
        word.parse::<u32>()
            .map_err(|_| format!("{word:?} is not a scan code"))?
    };
    let extended = code & win32::EXTENDED_PREFIX == win32::EXTENDED_PREFIX;
    let low = code & !win32::EXTENDED_PREFIX;
    if low > 0xFF || (!extended && code > 0xFF) {
        return Err(format!(
            "{code:#06x} is neither a set 1 scan code nor an 0xE0-prefixed one"
        ));
    }
    Ok(code)
}

/// Parses one signed number, naming which argument it was.
#[cfg(target_os = "windows")]
fn number(word: Option<&str>, what: &str) -> Result<i32, String> {
    word.ok_or_else(|| format!("{what} is required"))?
        .parse::<i32>()
        .map_err(|_| format!("{what} must be a whole number"))
}

/// Presses or releases one key by its physical position.
#[cfg(target_os = "windows")]
fn key(scancode: u32, pressed: bool) -> Result<(), String> {
    let extended = scancode & win32::EXTENDED_PREFIX == win32::EXTENDED_PREFIX;
    let mut flags = win32::KEYEVENTF_SCANCODE;
    if extended {
        flags |= win32::KEYEVENTF_EXTENDED;
    }
    if !pressed {
        flags |= win32::KEYEVENTF_KEY_UP;
    }
    send(win32::Input {
        kind: win32::INPUT_KEYBOARD,
        body: win32::InputBody {
            keyboard: win32::KeybdInput {
                // Zero, because `KEYEVENTF_SCANCODE` says the position is the
                // identity: the system maps it to a virtual key through the
                // *foreground thread's* layout, exactly as a driver's report
                // would be mapped. Naming a virtual key here would be this
                // program deciding the layout question the backend is under
                // test for.
                vk: 0,
                scan: (scancode & 0xFF) as u16,
                flags,
                ..win32::KeybdInput::default()
            },
        },
    })
}

/// Sends one mouse event.
#[cfg(target_os = "windows")]
fn mouse(flags: u32, mouse_data: u32, dx: i32, dy: i32) -> Result<(), String> {
    send(win32::Input {
        kind: win32::INPUT_MOUSE,
        body: win32::InputBody {
            mouse: win32::MouseInput {
                dx,
                dy,
                mouse_data,
                flags,
                ..win32::MouseInput::default()
            },
        },
    })
}

/// Hands one event to the session's input stream, or says why it did not go.
#[cfg(target_os = "windows")]
fn send(input: win32::Input) -> Result<(), String> {
    let size = i32::try_from(size_of::<win32::Input>()).expect("INPUT is 40 bytes");
    // SAFETY: `input` is a live, fully initialised `INPUT` and `size` is its
    // real size, which is what the call validates its own layout against. The
    // pointer is read and never retained.
    let inserted = unsafe { win32::SendInput(1, &raw const input, size) };
    if inserted == 1 {
        return Ok(());
    }
    // SAFETY: reading the calling thread's last error immediately after the
    // call that set it.
    let error = unsafe { win32::GetLastError() };
    Err(format!(
        "SendInput inserted {inserted} of 1 events, Win32 error {error}; the session may have no \
         interactive desktop, or a higher-integrity window may hold the foreground"
    ))
}

/// The foreground window, as a number a failing test can compare against.
#[cfg(target_os = "windows")]
fn foreground() -> String {
    // SAFETY: a handle by value; the call reads only window-station state.
    format!("{:#x}", unsafe { win32::GetForegroundWindow() } as usize)
}

/// Says something the harness may be blocked waiting to read.
#[cfg(target_os = "windows")]
fn say(what: &str) {
    let mut out = std::io::stdout().lock();
    let _ = writeln!(out, "crcbl-e2e-win32-input: {what}");
    let _ = out.flush();
}
