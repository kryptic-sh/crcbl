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
//! abs <x> <y>             an absolute move, normalized 0..=65535 over the
//!                         primary monitor
//! click <left|right|middle|back|forward>
//! wheel <notches>         positive scrolls away from the user
//! touch down <id> <x> <y> a finger lands at a screen pixel
//! touch move <id> <x> <y> a finger that is down moves there
//! touch up <id>           a finger lifts where it last was
//! ```
//!
//! # Touch needs no touchscreen, and needs every finger in every frame
//!
//! `InjectTouchInput` synthesizes contacts for a machine with no digitizer at
//! all, which is what lets this suite reach `WM_POINTER*` on a desktop nobody
//! can touch. Two rules shape the `touch` commands. The call describes a
//! **frame** holding every contact that is down, not a change to one of them,
//! so this program remembers each finger's position and repeats the others
//! beside the one a command moves. And its coordinates are **physical** screen
//! pixels, so the process makes itself per-monitor DPI aware before the first
//! one; a DPI-unaware sender would have its points scaled by the system on any
//! desktop not at 100%. The `<id>` is this program's name for a finger; the
//! backend sees the system's own pointer ids, which are different numbers.
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

    /// The width of the field `select` picks out, read from its type alone —
    /// the function is never called.
    #[cfg(target_pointer_width = "64")]
    const fn field_size<T, F>(_select: fn(&T) -> &F) -> usize {
        size_of::<F>()
    }

    /// Asserts every field's offset and width, one `field: offset, width;` row
    /// per field. The destructuring pattern has no `..`, so a field declared
    /// without a row fails to compile rather than going unchecked.
    #[cfg(target_pointer_width = "64")]
    macro_rules! assert_fields {
        ($ty:ident { $($field:ident: $offset:literal, $width:literal;)+ }) => {
            let _every_field_has_a_row: fn($ty) = |value| {
                let $ty { $($field: _),+ } = value;
            };
            $(
                assert!(
                    core::mem::offset_of!($ty, $field) == $offset,
                    concat!("offset of ", stringify!($ty), "::", stringify!($field))
                );
                assert!(
                    field_size(|value: &$ty| &value.$field) == $width,
                    concat!("width of ", stringify!($ty), "::", stringify!($field))
                );
            )+
        };
    }

    // The layout `SendInput` validates by size, checked at compile time on the
    // 64-bit Windows targets this engine claims. A `cbSize` that disagrees with
    // the system's is the classic way for this call to fail with
    // `ERROR_INVALID_PARAMETER` and no other symptom, and passing
    // `size_of::<Input>()` is only correct if the structure is right.
    //
    // Every offset and width here and in the pointer structures below is the
    // SDK's own `offsetof`/`sizeof`, printed by a C program built with MSVC
    // 19.44 against Windows SDK 10.0.26100.0 for x64: the x64 Windows ABI,
    // fixed by it rather than by this file.
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
        assert_fields!(MouseInput {
            dx: 0, 4;
            dy: 4, 4;
            mouse_data: 8, 4;
            flags: 12, 4;
            time: 16, 4;
            extra_info: 24, 8;
        });
        assert_fields!(KeybdInput {
            vk: 0, 2;
            scan: 2, 2;
            flags: 4, 4;
            time: 8, 4;
            extra_info: 16, 8;
        });
        assert_fields!(Input {
            kind: 0, 4;
            body: 8, 32;
        });
        assert!(core::mem::offset_of!(Input, body.mouse) == 8);
        assert!(core::mem::offset_of!(Input, body.keyboard) == 8);
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
    /// `MOUSEEVENTF_XDOWN` — a thumb button, which `mouseData` names.
    pub const MOUSEEVENTF_X_DOWN: u32 = 0x0080;
    /// `MOUSEEVENTF_XUP`.
    pub const MOUSEEVENTF_X_UP: u32 = 0x0100;
    /// `MOUSEEVENTF_WHEEL`.
    pub const MOUSEEVENTF_WHEEL: u32 = 0x0800;
    /// `MOUSEEVENTF_ABSOLUTE` — `dx`/`dy` are a normalized position, not motion.
    pub const MOUSEEVENTF_ABSOLUTE: u32 = 0x8000;
    /// `XBUTTON1`, the `mouseData` of the back thumb button.
    pub const XBUTTON1: u32 = 0x0001;
    /// `XBUTTON2`, the `mouseData` of the forward thumb button.
    pub const XBUTTON2: u32 = 0x0002;
    /// The top of the range `MOUSEEVENTF_ABSOLUTE` coordinates are normalized
    /// over.
    pub const ABSOLUTE_MAX: i32 = 65_535;
    /// `WHEEL_DELTA` — one notch, which is not one.
    pub const WHEEL_DELTA: i32 = 120;

    /// What `win32::keys::scancode` ORs into an extended key's code, and
    /// therefore how one is spelled on this program's stdin.
    pub const EXTENDED_PREFIX: u32 = 0xE000;

    /// `POINT`.
    #[repr(C)]
    #[derive(Clone, Copy, Default)]
    pub struct Point {
        /// Horizontal coordinate.
        pub x: i32,
        /// Vertical coordinate.
        pub y: i32,
    }

    /// `RECT`.
    #[repr(C)]
    #[derive(Clone, Copy, Default)]
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

    /// `POINTER_INFO`, the part of a contact every pointer type shares.
    #[repr(C)]
    #[derive(Clone, Copy)]
    pub struct PointerInfo {
        /// `PT_TOUCH` for everything this program injects.
        pub pointer_type: u32,
        /// This program's name for the finger.
        pub pointer_id: u32,
        /// Filled in by the system.
        pub frame_id: u32,
        /// `POINTER_FLAG_*`: what this contact is doing in this frame.
        pub pointer_flags: u32,
        /// Filled in by the system.
        pub source_device: *mut c_void,
        /// Zero: the system hit-tests the point, as it would a real finger.
        pub hwnd_target: Handle,
        /// Where, in physical screen pixels.
        pub pixel_location: Point,
        /// Filled in by the system.
        pub himetric_location: Point,
        /// Filled in by the system.
        pub pixel_location_raw: Point,
        /// Filled in by the system.
        pub himetric_location_raw: Point,
        /// Zero means "stamp it with the system's own time".
        pub time: u32,
        /// Unused when injecting.
        pub history_count: u32,
        /// Unused when injecting.
        pub input_data: i32,
        /// Unused when injecting.
        pub key_states: u32,
        /// Zero means "stamp it with the system's own counter".
        pub performance_count: u64,
        /// Unused when injecting.
        pub button_change_type: u32,
    }

    /// `POINTER_TOUCH_INFO`.
    #[repr(C)]
    #[derive(Clone, Copy)]
    pub struct PointerTouchInfo {
        /// The shared part.
        pub pointer_info: PointerInfo,
        /// `TOUCH_FLAG_NONE`.
        pub touch_flags: u32,
        /// Which of the optional fields below are meaningful: none of them.
        pub touch_mask: u32,
        /// Unused without `TOUCH_MASK_CONTACTAREA`.
        pub contact: Rect,
        /// Unused without `TOUCH_MASK_CONTACTAREA`.
        pub contact_raw: Rect,
        /// Unused without `TOUCH_MASK_ORIENTATION`.
        pub orientation: u32,
        /// Unused without `TOUCH_MASK_PRESSURE`.
        pub pressure: u32,
    }

    // `InjectTouchInput` takes no size, so a wrong layout is not refused; it is
    // read as garbage. These are the SDK's layouts on 64-bit Windows, from the
    // same C program as `INPUT`'s above.
    #[cfg(target_pointer_width = "64")]
    const _: () = {
        assert!(
            size_of::<PointerInfo>() == 96,
            "POINTER_INFO on 64-bit Windows"
        );
        assert!(
            size_of::<PointerTouchInfo>() == 144,
            "POINTER_TOUCH_INFO on 64-bit Windows"
        );
        assert!(size_of::<Point>() == 8, "POINT");
        assert!(size_of::<Rect>() == 16, "RECT");
        assert_fields!(Point {
            x: 0, 4;
            y: 4, 4;
        });
        assert_fields!(Rect {
            left: 0, 4;
            top: 4, 4;
            right: 8, 4;
            bottom: 12, 4;
        });
        assert_fields!(PointerInfo {
            pointer_type: 0, 4;
            pointer_id: 4, 4;
            frame_id: 8, 4;
            pointer_flags: 12, 4;
            source_device: 16, 8;
            hwnd_target: 24, 8;
            pixel_location: 32, 8;
            himetric_location: 40, 8;
            pixel_location_raw: 48, 8;
            himetric_location_raw: 56, 8;
            time: 64, 4;
            history_count: 68, 4;
            input_data: 72, 4;
            key_states: 76, 4;
            performance_count: 80, 8;
            button_change_type: 88, 4;
        });
        assert_fields!(PointerTouchInfo {
            pointer_info: 0, 96;
            touch_flags: 96, 4;
            touch_mask: 100, 4;
            contact: 104, 16;
            contact_raw: 120, 16;
            orientation: 136, 4;
            pressure: 140, 4;
        });
    };

    /// `PT_TOUCH`.
    pub const PT_TOUCH: u32 = 2;
    /// `TOUCH_FEEDBACK_NONE` — no ripple drawn on the desktop under the point.
    pub const TOUCH_FEEDBACK_NONE: u32 = 0x3;
    /// How many fingers `InitializeTouchInjection` is told to expect.
    pub const MAX_CONTACTS: u32 = 10;
    /// `POINTER_FLAG_INRANGE`.
    pub const POINTER_FLAG_IN_RANGE: u32 = 0x0000_0002;
    /// `POINTER_FLAG_INCONTACT`.
    pub const POINTER_FLAG_IN_CONTACT: u32 = 0x0000_0004;
    /// `POINTER_FLAG_DOWN`.
    pub const POINTER_FLAG_DOWN: u32 = 0x0001_0000;
    /// `POINTER_FLAG_UPDATE`.
    pub const POINTER_FLAG_UPDATE: u32 = 0x0002_0000;
    /// `POINTER_FLAG_UP`.
    pub const POINTER_FLAG_UP: u32 = 0x0004_0000;
    /// `DPI_AWARENESS_CONTEXT_PER_MONITOR_AWARE_V2`.
    pub const DPI_PER_MONITOR_AWARE_V2: isize = -4;

    #[link(name = "user32")]
    unsafe extern "system" {
        /// Readies the calling process to inject up to `max_count` contacts.
        pub fn InitializeTouchInjection(max_count: u32, feedback: u32) -> i32;
        /// Injects one frame: every contact that is down, each saying what it
        /// is doing.
        pub fn InjectTouchInput(count: u32, contacts: *const PointerTouchInfo) -> i32;
        /// Makes screen coordinates physical pixels for this process.
        pub fn SetProcessDpiAwarenessContext(context: isize) -> i32;
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

    let mut fingers = Fingers::default();
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
        match run(command, &mut fingers) {
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
fn run(command: &str, fingers: &mut Fingers) -> Result<(), String> {
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
        "abs" => {
            let x = number(words.next(), "x")?;
            let y = number(words.next(), "y")?;
            let range = 0..=win32::ABSOLUTE_MAX;
            if !range.contains(&x) || !range.contains(&y) {
                return Err(format!(
                    "({x}, {y}) is outside the normalized range {range:?}"
                ));
            }
            mouse(
                win32::MOUSEEVENTF_MOVE | win32::MOUSEEVENTF_ABSOLUTE,
                0,
                x,
                y,
            )
        }
        "click" => {
            // The thumb buttons share one flag pair and are told apart by
            // `mouseData`, which `WM_XBUTTON*` carries on as its high word.
            let (down, up, mouse_data) = match words.next() {
                Some("left") => (win32::MOUSEEVENTF_LEFT_DOWN, win32::MOUSEEVENTF_LEFT_UP, 0),
                Some("right") => (
                    win32::MOUSEEVENTF_RIGHT_DOWN,
                    win32::MOUSEEVENTF_RIGHT_UP,
                    0,
                ),
                Some("middle") => (
                    win32::MOUSEEVENTF_MIDDLE_DOWN,
                    win32::MOUSEEVENTF_MIDDLE_UP,
                    0,
                ),
                Some("back") => (
                    win32::MOUSEEVENTF_X_DOWN,
                    win32::MOUSEEVENTF_X_UP,
                    win32::XBUTTON1,
                ),
                Some("forward") => (
                    win32::MOUSEEVENTF_X_DOWN,
                    win32::MOUSEEVENTF_X_UP,
                    win32::XBUTTON2,
                ),
                other => {
                    return Err(format!(
                        "{other:?} is not left, right, middle, back or forward"
                    ));
                }
            };
            mouse(down, mouse_data, 0, 0).and_then(|()| mouse(up, mouse_data, 0, 0))
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
        "touch" => {
            let action = words.next();
            let id = u32::try_from(number(words.next(), "finger id")?)
                .map_err(|_| "a finger id is not negative")?;
            match action {
                Some("down") => {
                    let at = point(&mut words)?;
                    fingers.down(id, at)
                }
                Some("move") => {
                    let at = point(&mut words)?;
                    fingers.moved(id, at)
                }
                Some("up") => fingers.up(id),
                other => Err(format!("{other:?} is not down, move or up")),
            }
        }
        other => Err(format!(
            "{other:?} is not one of key, down, up, move, abs, click, wheel, touch"
        )),
    }
}

/// Two numbers, `x` then `y`.
#[cfg(target_os = "windows")]
fn point<'a>(words: &mut impl Iterator<Item = &'a str>) -> Result<win32::Point, String> {
    let x = number(words.next(), "x")?;
    let y = number(words.next(), "y")?;
    Ok(win32::Point { x, y })
}

/// Every finger this program has put down and not yet lifted.
#[cfg(target_os = "windows")]
#[derive(Default)]
struct Fingers {
    /// Whether `InitializeTouchInjection` has run in this process.
    ready: bool,
    /// Each finger that is down, by this program's id, and where it is.
    down: Vec<(u32, win32::Point)>,
}

#[cfg(target_os = "windows")]
impl Fingers {
    /// A finger lands.
    fn down(&mut self, id: u32, at: win32::Point) -> Result<(), String> {
        if self.down.iter().any(|&(finger, _)| finger == id) {
            return Err(format!("finger {id} is already down"));
        }
        let flags = win32::POINTER_FLAG_DOWN
            | win32::POINTER_FLAG_IN_RANGE
            | win32::POINTER_FLAG_IN_CONTACT;
        self.inject(id, at, flags)?;
        self.down.push((id, at));
        Ok(())
    }

    /// A finger that is down moves.
    fn moved(&mut self, id: u32, at: win32::Point) -> Result<(), String> {
        let index = self.index(id)?;
        let flags = win32::POINTER_FLAG_UPDATE
            | win32::POINTER_FLAG_IN_RANGE
            | win32::POINTER_FLAG_IN_CONTACT;
        self.inject(id, at, flags)?;
        self.down[index].1 = at;
        Ok(())
    }

    /// A finger lifts, where it last was.
    fn up(&mut self, id: u32) -> Result<(), String> {
        let index = self.index(id)?;
        let at = self.down[index].1;
        self.inject(id, at, win32::POINTER_FLAG_UP)?;
        self.down.remove(index);
        Ok(())
    }

    fn index(&self, id: u32) -> Result<usize, String> {
        self.down
            .iter()
            .position(|&(finger, _)| finger == id)
            .ok_or_else(|| format!("finger {id} is not down"))
    }

    /// Injects one frame: `id` doing what `flags` say at `at`, and every other
    /// finger that is down holding still where it is.
    fn inject(&mut self, id: u32, at: win32::Point, flags: u32) -> Result<(), String> {
        if !self.ready {
            // SAFETY: a constant awareness context by value. A refusal leaves
            // the process DPI-unaware, which only matters on a scaled desktop
            // and is then visible as touches landing in the wrong place.
            unsafe { win32::SetProcessDpiAwarenessContext(win32::DPI_PER_MONITOR_AWARE_V2) };
            // SAFETY: two integers by value.
            if unsafe {
                win32::InitializeTouchInjection(win32::MAX_CONTACTS, win32::TOUCH_FEEDBACK_NONE)
            } == 0
            {
                return Err(last_error("InitializeTouchInjection"));
            }
            self.ready = true;
        }
        let holding = win32::POINTER_FLAG_UPDATE
            | win32::POINTER_FLAG_IN_RANGE
            | win32::POINTER_FLAG_IN_CONTACT;
        let frame: Vec<win32::PointerTouchInfo> = self
            .down
            .iter()
            .filter(|&&(finger, _)| finger != id)
            .map(|&(finger, still)| contact(finger, still, holding))
            .chain([contact(id, at, flags)])
            .collect();
        let count = u32::try_from(frame.len()).expect("at most MAX_CONTACTS fingers");
        // SAFETY: `frame` is `count` live, fully initialised
        // `POINTER_TOUCH_INFO`s, read and never retained.
        if unsafe { win32::InjectTouchInput(count, frame.as_ptr()) } == 0 {
            return Err(last_error("InjectTouchInput"));
        }
        Ok(())
    }
}

/// One finger's entry in a frame.
#[cfg(target_os = "windows")]
fn contact(id: u32, at: win32::Point, flags: u32) -> win32::PointerTouchInfo {
    win32::PointerTouchInfo {
        pointer_info: win32::PointerInfo {
            pointer_type: win32::PT_TOUCH,
            pointer_id: id,
            frame_id: 0,
            pointer_flags: flags,
            source_device: core::ptr::null_mut(),
            hwnd_target: core::ptr::null_mut(),
            pixel_location: at,
            himetric_location: win32::Point::default(),
            pixel_location_raw: win32::Point::default(),
            himetric_location_raw: win32::Point::default(),
            time: 0,
            history_count: 0,
            input_data: 0,
            key_states: 0,
            performance_count: 0,
            button_change_type: 0,
        },
        touch_flags: 0,
        touch_mask: 0,
        contact: win32::Rect::default(),
        contact_raw: win32::Rect::default(),
        orientation: 0,
        pressure: 0,
    }
}

/// `what` failed, and the calling thread's last error says why.
#[cfg(target_os = "windows")]
fn last_error(what: &str) -> String {
    // SAFETY: reading the calling thread's last error immediately after the
    // call that set it.
    let error = unsafe { win32::GetLastError() };
    format!("{what} failed with Win32 error {error}")
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
