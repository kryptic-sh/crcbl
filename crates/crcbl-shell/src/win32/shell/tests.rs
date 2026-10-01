use super::*;
use crate::{AspectRatio, LogicalSize, MimeType};
use crcbl_core::KeyCode;
use crcbl_core::input::{ButtonState, Keysym, PointerButton, Scancode, ScrollDelta};
use std::path::PathBuf;
use std::time::Instant;

use super::super::ffi::{DropFiles, Lparam, MinMaxInfo, Point, Rect, msg};

/// `VK_W`, which is the letter's virtual key on every layout that has one.
const VK_W: usize = 0x57;
/// `VK_UP`.
const VK_UP: usize = 0x26;

mod desktop;
use desktop::shell;

fn window(shell: &mut Win32Shell) -> WindowId {
    shell
        .create_window(&WindowDesc {
            title: "crcbl P5C W2",
            ..WindowDesc::default()
        })
        .expect("creating a top-level window")
}

/// The `HWND`, for the messages only a user's mouse would otherwise send.
fn hwnd_of(shell: &Win32Shell, window: WindowId) -> Handle {
    shell.window(window).expect("a live window").raw()
}

/// Sends a key message with the `lParam` the keyboard driver would build.
///
/// The whole of input is otherwise unreachable from CI, which has no
/// keyboard and no mouse. `SendMessageW` runs the real window procedure
/// against the real cached state, so everything below the driver — the scan
/// code table, the modifier snapshot, the layout lookup — is exercised.
fn send_key(
    hwnd: Handle,
    message: u32,
    virtual_key: usize,
    scancode: isize,
    extended: bool,
    previous: bool,
) {
    let mut l_param = (scancode << 16) | 1;
    if extended {
        l_param |= 1 << 24;
    }
    if previous {
        l_param |= 1 << 30;
    }
    if message == msg::KEY_UP {
        // The transition bit, which is set on every release and which this
        // backend must not confuse with the previous-state bit beside it.
        l_param |= 1 << 31;
    }
    // SAFETY: a keyboard message to this shell's own window, with the
    // `wParam`/`lParam` encoding `winuser.h` documents for it.
    unsafe { ffi::SendMessageW(hwnd, message, virtual_key, l_param) };
}

/// Sends a mouse message with two coordinates packed into `lParam`.
fn send_mouse(hwnd: Handle, message: u32, w_param: usize, x: i32, y: i32) {
    let l_param = (((y as u16 as usize) << 16) | (x as u16 as usize)) as Lparam;
    // SAFETY: a mouse message to this shell's own window, with the packed
    // coordinate encoding `winuser.h` documents.
    unsafe { ffi::SendMessageW(hwnd, message, w_param, l_param) };
}

/// Sends the `WM_MOUSELEAVE` a real pointer leaving would produce.
///
/// The one-shot notification this backend arms with `TrackMouseEvent`, sent
/// by hand because CI has no mouse to move out — and, at the *start* of a
/// test, the only way to put the derived "is the pointer inside?" state on a
/// known edge when a real cursor may already be over the window.
fn send_leave(hwnd: Handle) {
    // SAFETY: a pointer-crossing message to this shell's own window, with
    // the `wParam`/`lParam` of zero `winuser.h` documents for it.
    unsafe { ffi::SendMessageW(hwnd, msg::MOUSE_LEAVE, 0, 0) };
}

/// Makes a window the foreground one.
///
/// `ClipCursor` and `SetCursorPos` are both restricted to the process that
/// owns the foreground window — the desktop's protection against exactly the
/// hostage-taking this backend is careful not to do — so a test that
/// exercises either has to arrange it. Only [`focus_and_confirm`] calls
/// it, because a grant is worth nothing until the system confirms it.
///
/// Returns whether `SetForegroundWindow` said it took. A refusal is not an
/// error Windows reports anywhere else: `ClipCursor` then does nothing and
/// still succeeds, so the refusal has to be read here or not at all.
fn make_foreground(hwnd: Handle) -> bool {
    // SAFETY: this shell's own window, from the thread that created it.
    unsafe { ffi::SetForegroundWindow(hwnd) != 0 }
}

/// Whether `hwnd` is the foreground window right now.
///
/// Asked of the system rather than of `WindowState::focused`, which any
/// `WM_SETFOCUS` sets — including the synthetic one [`send_focus`] sends.
fn is_foreground(hwnd: Handle) -> bool {
    // SAFETY: no arguments; the result is only compared, never used.
    core::ptr::eq(unsafe { ffi::GetForegroundWindow() }, hwnd)
}

/// Gives a window the keyboard and **confirms it kept it**.
///
/// [`send_focus`] delivers `WM_SETFOCUS` synchronously, so the message
/// itself always arrives. Holding the focus is the part that can fail: the
/// Windows runner is a real, non-idle desktop — `docs/backlog.md` records
/// the unbidden messages that proved it — and anything on it may take the
/// foreground straight back. The theft arrives as a `WM_KILLFOCUS` that the *same* pump then
/// processes, so granting focus and pumping once can leave the window
/// unfocused with nothing said about why.
///
/// That is worth a helper rather than an inline retry because of what it
/// costs when it is missing: the cursor is hidden only for a focused
/// window, so a silently lost focus turns
/// `hiding_the_cursor_is_balanced_however_many_times_it_is_asked_for` into
/// `left: 0, right: -1` — a reference-count failure that is really a focus
/// failure, which is how it was read the first time it happened.
///
/// **`focused` alone cannot confirm anything**, because the synthetic
/// `WM_SETFOCUS` sets it whether or not the window is really in front. The
/// first version of this helper judged by that flag and so failed only
/// when a foreign `WM_KILLFOCUS` happened to land in the same pump — and
/// passed when `SetForegroundWindow` had been refused outright, leaving
/// `ClipCursor` to do nothing and a clip assertion further down to fail
/// with no mention of focus. So every attempt also asks the system: the
/// grant has to have been accepted, and the window still has to be the
/// foreground one after the pump, before and after the synthetic message.
/// The pump before judging drains the real activation traffic the grant
/// produced, so a steal already in flight is seen rather than raced.
///
/// # Panics
///
/// If the window will not keep the keyboard for long enough to ask it a
/// question, which is a runner that cannot host this test rather than a
/// backend that is wrong.
fn focus_and_confirm(shell: &mut Win32Shell, window: WindowId, hwnd: Handle) {
    // Enough to outlast a transient steal without turning a genuinely
    // unfocusable window into a long hang.
    const ATTEMPTS: u32 = 8;
    // The first back-off, doubled per attempt up to `BACKOFF_DOUBLINGS`
    // times, so a sibling process that holds the foreground for a moment
    // gets that moment instead of eight grants inside it.
    const BACKOFF: Duration = Duration::from_millis(10);
    const BACKOFF_DOUBLINGS: u32 = 4;
    let mut granted = false;
    for attempt in 0..ATTEMPTS {
        granted = make_foreground(hwnd);
        shell.pump(&mut |_| {});
        if granted && is_foreground(hwnd) {
            send_focus(hwnd, true);
            shell.pump(&mut |_| {});
            if shell.window_state(window).expect("live").focused && is_foreground(hwnd) {
                return;
            }
        }
        std::thread::sleep(BACKOFF * (1 << attempt.min(BACKOFF_DOUBLINGS)));
    }
    // SAFETY: no arguments; read only to name the holder in the message.
    let foreground = unsafe { ffi::GetForegroundWindow() };
    panic!(
        "the window would not keep the keyboard over {ATTEMPTS} attempts, so nothing \
             below this can be asked about a focused window (last SetForegroundWindow \
             granted: {granted}; foreground window {foreground:?}, ours {hwnd:?})"
    );
}

/// Sends `WM_SETFOCUS` or `WM_KILLFOCUS`.
///
/// The clip and the cursor both follow the focus, and CI cannot click on a
/// window to give it one.
fn send_focus(hwnd: Handle, focused: bool) {
    let message = if focused {
        msg::SET_FOCUS
    } else {
        msg::KILL_FOCUS
    };
    // SAFETY: a focus message to this shell's own window.
    unsafe { ffi::SendMessageW(hwnd, message, 0, 0) };
}

/// Every [`ShellEvent::Key`] one pump produced, flattened for assertion.
fn pump_keys(
    shell: &mut Win32Shell,
) -> Vec<(Scancode, Option<KeyCode>, Keysym, ButtonState, bool)> {
    let mut keys = Vec::new();
    shell.pump(&mut |event| {
        if let ShellEvent::Key {
            scancode,
            key_code,
            keysym,
            state,
            repeat,
            ..
        } = event
        {
            keys.push((scancode, key_code, keysym, state, repeat));
        }
    });
    keys
}

/// The rectangle the cursor is currently clipped to.
fn clip_rect() -> Rect {
    let mut rect = Rect::default();
    // SAFETY: `rect` is a live, initialised `RECT` the call writes into.
    unsafe { ffi::GetClipCursor(&raw mut rect) };
    rect
}

/// The whole desktop, as one rectangle.
///
/// The origin is read rather than assumed to be zero: a monitor to the left
/// of the primary puts the virtual screen's left edge at a negative
/// coordinate, and a test that assumed the origin would pass on this
/// developer's desk and fail on somebody else's.
fn virtual_screen() -> Rect {
    // SAFETY: four metric queries by value; each reads no memory of ours
    // and answers zero for an index it does not know.
    unsafe {
        let left = ffi::GetSystemMetrics(value::SM_X_VIRTUAL_SCREEN);
        let top = ffi::GetSystemMetrics(value::SM_Y_VIRTUAL_SCREEN);
        Rect {
            left,
            top,
            right: left + ffi::GetSystemMetrics(value::SM_CX_VIRTUAL_SCREEN),
            bottom: top + ffi::GetSystemMetrics(value::SM_CY_VIRTUAL_SCREEN),
        }
    }
}

/// The rectangle a confined pointer should be held to, read **now**.
///
/// Both halves move underneath a test: the desktop can reposition a window,
/// and this runner changes its display set mid-run — the behaviour that
/// made [`Win32Shell::refresh_clip`] refuse a degenerate refresh. Capturing
/// either half early and comparing it later turns a desktop change into a
/// failure of whatever assertion happened to be running, which is how
/// `minimizing_a_captured_window_releases_the_clip` failed twice on CI for
/// commits that touched no shell code. Callers read this immediately before
/// they assert, so the comparison is against the desktop that exists.
fn confined_to_client(hwnd: Handle) -> Rect {
    super::input::client_screen_rect(hwnd)
        .expect("a live window has a client rectangle")
        .intersect(virtual_screen())
}

/// Whether any window of this process has the clipboard open.
///
/// The observable behind "closed on every path out": the guard's `Drop` is
/// what has to have run, and this is the only thing that can see whether it
/// did.
/// Whether *this* window holds the clipboard open.
///
/// This replaced a `clipboard_is_open` that asked `GetOpenClipboardWindow`
/// whether **anybody** holds it — a wider question than these tests are
/// asking. Every caller wanted "did our own guard give it back", and the
/// broad form answers that wrongly whenever a foreign process on a shared
/// desktop happens to hold the clipboard, which says nothing about our
/// code. That is a real CI flake, recorded in `docs/backlog.md`.
fn clipboard_held_by(hwnd: *mut core::ffi::c_void) -> bool {
    // SAFETY: as above — a read of window-station state, no arguments.
    core::ptr::eq(unsafe { ffi::GetOpenClipboardWindow() }, hwnd)
}

/// A window that accepts file drops, which is off by default.
fn drop_window(shell: &mut Win32Shell) -> WindowId {
    shell
        .create_window(&WindowDesc {
            title: "crcbl P5C W3 — drops",
            accept_drops: true,
            ..WindowDesc::default()
        })
        .expect("creating a drop target")
}

/// Builds the `HDROP` a file manager's drop would have handed over.
///
/// CI has nobody to drag a file onto a window, so this is the drag: the
/// block is exactly what the shell puts behind an `HDROP`, and everything
/// downstream of it — `DragQueryFileW`, `DragQueryPoint`, `DragFinish`, the
/// window procedure, the shared drop queue and the translation — is the
/// real path. The handle is **not** freed here: `DragFinish` inside the
/// procedure owns it, and that is part of what this exercises.
fn make_hdrop(paths: &[&str], x: i32, y: i32) -> Handle {
    let mut list: Vec<u16> = Vec::new();
    for path in paths {
        list.extend(path.encode_utf16());
        list.push(0);
    }
    // The list itself is terminated by an empty entry, which is the second
    // NUL after the last path — and the whole list when there are none.
    list.push(0);

    let header = size_of::<DropFiles>();
    let bytes = header + list.len() * 2;
    // SAFETY: an allocation request by value. `GMEM_MOVEABLE` is what an
    // `HDROP` is, which is why `DragQueryFileW` can lock it.
    let mem = unsafe { ffi::GlobalAlloc(value::GMEM_MOVEABLE, bytes) };
    assert!(!mem.is_null(), "GlobalAlloc for a synthetic HDROP");
    // SAFETY: `mem` is a live moveable block this function owns until the
    // message is sent.
    let locked = unsafe { ffi::GlobalLock(mem) };
    assert!(!locked.is_null(), "GlobalLock of a synthetic HDROP");
    // SAFETY: `locked` points at `bytes` writable bytes, which is
    // `size_of::<DropFiles>()` followed by exactly `list.len()` code units;
    // `GlobalAlloc` returns at least 8-byte alignment, so both the header
    // write and the `u16` copy at the even offset `header` are aligned.
    unsafe {
        ptr::write_bytes(locked.cast::<u8>(), 0, bytes);
        locked.cast::<DropFiles>().write(DropFiles {
            p_files: header as u32,
            pt: Point { x, y },
            f_nc: 0,
            f_wide: 1,
        });
        ptr::copy_nonoverlapping(
            list.as_ptr(),
            locked.cast::<u8>().add(header).cast::<u16>(),
            list.len(),
        );
        ffi::GlobalUnlock(mem);
    }
    mem
}

/// Sends a `WM_DROPFILES` carrying a synthetic `HDROP`.
fn send_drop(hwnd: Handle, paths: &[&str], x: i32, y: i32) {
    let hdrop = make_hdrop(paths, x, y);
    // SAFETY: a drop message to this shell's own window, with the `wParam`
    // `shell32` documents for it. The procedure finishes the handle.
    unsafe { ffi::SendMessageW(hwnd, msg::DROP_FILES, hdrop as usize, 0) };
}

/// Every [`ShellEvent::DroppedFile`] one pump produced.
fn pump_drops(shell: &mut Win32Shell) -> Vec<(WindowId, PathBuf, Option<PhysicalPoint>)> {
    let mut dropped = Vec::new();
    shell.pump(&mut |event| {
        if let ShellEvent::DroppedFile {
            window,
            path,
            position,
            ..
        } = event
        {
            dropped.push((window, path, position));
        }
    });
    dropped
}

/// Reads the clipboard and pumps until the answer arrives.
///
/// One pump is always enough on this backend — the read happens inside
/// `clipboard_request` — and the loop is not a retry: it asserts that.
fn paste(shell: &mut Win32Shell, window: WindowId, requested: MimeType) -> ClipboardContent {
    let request = shell
        .clipboard_request(window, requested)
        .expect("CLIPBOARD is claimed");
    let mut answers = Vec::new();
    shell.pump(&mut |event| {
        if let ShellEvent::ClipboardData {
            window: asked,
            request: answered,
            mime,
            content,
        } = event
        {
            assert_eq!(asked, window, "the answer names the window that asked");
            assert_eq!(answered, request, "and the request it answers");
            assert!(
                mime.matches(requested),
                "a Win32 format is a number, so the answer echoes the request: {mime}"
            );
            answers.push(content);
        }
    });
    assert_eq!(
        answers.len(),
        1,
        "exactly one answer per accepted request, on the first pump: {answers:?}"
    );
    answers.remove(0)
}

/// The cursor's display count, read without changing it.
///
/// There is no read-only accessor: `ShowCursor` returns the count *after*
/// its own adjustment, so the only way to see it is to move it and put it
/// back. The pair is balanced, which is exactly what the code under test has
/// to be as well.
fn cursor_display_count() -> i32 {
    // SAFETY: two integers by value; neither call can fail, and together
    // they leave the count where they found it.
    unsafe {
        let raised = ffi::ShowCursor(1);
        ffi::ShowCursor(0);
        raised - 1
    }
}

#[test]
fn a_window_has_no_size_until_the_first_pump_and_exactly_one_is_enough() {
    // The P0.4 contract, and the Win32 shape of it: the system knew the
    // size before `create_window` returned, so the wait is one pump — not a
    // round trip, and not an invented delay to look like Wayland.
    let mut shell = shell();
    let window = window(&mut shell);
    assert_eq!(
        shell.window_state(window).expect("live").size(),
        None,
        "a freshly created window has not been configured yet"
    );

    let mut events = Vec::new();
    shell.pump(&mut |event| events.push(event));
    let size = shell
        .window_state(window)
        .expect("live")
        .size()
        .expect("one pump is enough on Win32");
    assert!(!size.is_empty(), "{size}");
    assert!(
        events.iter().any(|event| matches!(
            event,
            ShellEvent::Resized { window: reported, .. } if *reported == window
        )),
        "the size arrives as an event, not only as state: {events:?}"
    );

    // And it is not re-delivered: the `WM_SIZE` that `CreateWindowExW`
    // sent is a restatement of a size already published.
    let mut again = Vec::new();
    shell.pump(&mut |event| again.push(event));
    assert!(
        !again
            .iter()
            .any(|event| matches!(event, ShellEvent::Resized { .. })),
        "{again:?}"
    );
}

#[test]
fn a_window_opens_flips_to_borderless_and_back_and_is_destroyed() {
    let mut shell = shell();
    let window = window(&mut shell);
    shell.pump(&mut |_| {});
    let windowed = shell
        .window_state(window)
        .expect("live")
        .size()
        .expect("configured");

    let primary = shell
        .monitors()
        .iter()
        .find(|monitor| monitor.is_primary)
        .expect("a desktop session has a primary monitor")
        .id;
    let bounds = shell.monitor(primary).expect("just found it").bounds;

    shell
        .set_mode(
            window,
            DisplayMode::Borderless {
                monitor: Some(primary),
            },
        )
        .expect("borderless on a named monitor is what WINDOW_POSITION claims");
    let mut events = Vec::new();
    shell.pump(&mut |event| events.push(event));
    let state = shell.window_state(window).expect("live");
    assert_eq!(
        state.size(),
        Some(bounds.size()),
        "a borderless window covers its monitor exactly"
    );
    assert!(
        state.mode_request_honoured(),
        "Win32 has no window manager to refuse: {state:?}"
    );
    assert!(
        events
            .iter()
            .any(|event| matches!(event, ShellEvent::Resized { .. })),
        "the mode change is reported as a resize: {events:?}"
    );

    shell
        .set_mode(window, DisplayMode::Windowed)
        .expect("and back");
    shell.pump(&mut |_| {});
    let state = shell.window_state(window).expect("live");
    assert_eq!(
        state.size(),
        Some(windowed),
        "the saved WINDOWPLACEMENT restores the windowed size exactly"
    );
    assert_eq!(state.effective_mode(), Some(DisplayMode::Windowed));
    assert!(state.mode_request_honoured());

    shell.destroy_window(window).expect("destroy");
    let mut destroyed = false;
    shell.pump(&mut |event| {
        destroyed |=
            matches!(event, ShellEvent::WindowDestroyed { window: gone } if gone == window);
    });
    assert!(destroyed, "destruction is reported");
    assert!(matches!(
        shell.window_state(window),
        Err(ShellError::InvalidWindow { .. })
    ));
}

#[test]
fn a_window_created_borderless_can_still_be_made_windowed() {
    // The case with no saved placement to restore: the window has never
    // had a windowed style, so one is built from the descriptor rather
    // than the recorded mode being changed under a window that is still
    // `WS_POPUP`.
    let mut shell = shell();
    let primary = shell
        .monitors()
        .iter()
        .find(|monitor| monitor.is_primary)
        .expect("a primary monitor")
        .id;
    let bounds = shell.monitor(primary).expect("just found it").bounds;
    let window = shell
        .create_window(&WindowDesc {
            title: "crcbl P5C W1 — borderless from the start",
            mode: DisplayMode::Borderless {
                monitor: Some(primary),
            },
            size: LogicalSize::new(800.0, 600.0),
            ..WindowDesc::default()
        })
        .expect("create borderless");
    shell.pump(&mut |_| {});
    let state = shell.window_state(window).expect("live");
    assert_eq!(state.size(), Some(bounds.size()), "it covers the monitor");
    assert!(state.mode_request_honoured());

    shell
        .set_mode(window, DisplayMode::Windowed)
        .expect("and back to a mode it has never been in");
    shell.pump(&mut |_| {});
    let state = shell.window_state(window).expect("live");
    assert_eq!(state.effective_mode(), Some(DisplayMode::Windowed));
    let size = state.size().expect("configured");
    assert_ne!(size, bounds.size(), "it is no longer the monitor: {size}");
    let scale = shell.window(window).expect("live").scale_factor;
    assert_eq!(
        size,
        LogicalSize::new(800.0, 600.0).to_physical(scale),
        "the descriptor's windowed size is what it becomes"
    );
}

#[test]
fn the_capabilities_are_exactly_what_this_backend_implements() {
    let mut shell = shell();
    let window = window(&mut shell);
    let caps = shell.caps();
    for present in [
        ShellCaps::MULTI_WINDOW,
        ShellCaps::EVENT_WAIT,
        ShellCaps::WINDOW_POSITION,
        ShellCaps::SERVER_DECORATIONS,
        ShellCaps::FRACTIONAL_SCALE,
        ShellCaps::ASPECT_HINT_HONORED,
        ShellCaps::POINTER_LOCK,
        ShellCaps::POINTER_CONFINE,
        ShellCaps::POINTER_WARP,
        ShellCaps::CLIPBOARD,
        ShellCaps::DRAG_DROP,
        ShellCaps::TEXT_IME,
        ShellCaps::TOUCH,
    ] {
        assert!(caps.contains(present), "{present:?} is implemented");
    }
    // Latched from the registration rather than assumed — the one bit on
    // this backend that is not a constant.
    assert!(
        caps.contains(ShellCaps::RAW_POINTER_MOTION),
        "RegisterRawInputDevices was refused on this runner, which is a finding"
    );
    assert!(caps.has_mouselook(), "both halves, which is the point");

    // Clear, for a stated reason: a plain `HWND` presents at its own size.
    // A capability that overstates itself is worse than one that is missing.
    assert!(!caps.contains(ShellCaps::HW_UPSCALE));
    assert_eq!(caps, shell.caps(), "latched for the shell's lifetime");

    // And the methods agree with the bits, which is what makes them
    // checkable rather than decorative.
    assert!(shell.clipboard_offer(window, &[]).is_ok());
    assert!(shell.clipboard_request(window, MimeType::TextUtf8).is_ok());
    assert!(
        shell.clipboard_readable(window),
        "Win32 has no focus gate, so the trait's default is exactly right"
    );
    for mode in [
        PointerMode::Free,
        PointerMode::Confined,
        PointerMode::Locked,
        PointerMode::Free,
    ] {
        assert!(
            shell.set_pointer_mode(window, mode).is_ok(),
            "{mode:?} is claimed by the capability set"
        );
        assert_eq!(shell.window_state(window).expect("live").pointer_mode, mode);
    }
    assert!(shell.warp_pointer(window, PhysicalPoint::ORIGIN).is_ok());
    assert!(
        shell
            .set_cursor(window, Some(CursorIcon::Crosshair))
            .is_ok()
    );
    assert!(shell.set_cursor(window, None).is_ok());
}

#[test]
fn a_key_press_carries_its_position_its_symbol_and_its_repeat_flag() {
    // The real window procedure, the real scan-code table, the real
    // `GetKeyboardState` and the real `MapVirtualKeyW` — driven by a
    // message CI has no keyboard to send.
    let mut shell = shell();
    let window = window(&mut shell);
    shell.pump(&mut |_| {});
    let hwnd = hwnd_of(&shell, window);

    send_key(hwnd, msg::KEY_DOWN, VK_W, 0x0011, false, false);
    send_key(hwnd, msg::KEY_DOWN, VK_W, 0x0011, false, true);
    send_key(hwnd, msg::KEY_UP, VK_W, 0x0011, false, true);
    // An extended key, which is the half a dropped `E0` bit gets wrong.
    send_key(hwnd, msg::KEY_DOWN, VK_UP, 0x0048, true, false);

    let keys = pump_keys(&mut shell);
    assert_eq!(keys.len(), 4, "{keys:?}");
    let (scancode, key_code, keysym, state, repeat) = keys[0];
    assert_eq!(scancode, Scancode(0x0011));
    assert_eq!(key_code, Some(KeyCode::KeyW));
    assert_eq!(state, ButtonState::Pressed);
    assert!(!repeat, "the first press is not a repeat");
    assert_eq!(
        keysym.to_char(),
        Some('w'),
        "a US layout labels this key w; a different layout is a different \
             letter and still a character"
    );

    assert!(keys[1].4, "the second press is the system repeating it");
    assert_eq!(keys[2].3, ButtonState::Released);
    assert!(
        !keys[2].4,
        "bit 30 is always set on a release and must not be read there"
    );

    assert_eq!(keys[3].0, Scancode(0xE048), "the E0 prefix is folded in");
    assert_eq!(keys[3].1, Some(KeyCode::ArrowUp));
    assert_eq!(keys[3].2, Keysym(0xFF52), "XK_Up, from the named table");
}

#[test]
fn typing_commits_text_and_a_control_key_commits_nothing() {
    let mut shell = shell();
    let window = window(&mut shell);
    shell.pump(&mut |_| {});
    let hwnd = hwnd_of(&shell, window);

    // A BMP character, then a surrogate pair, then the carriage return
    // Windows really does deliver when Enter is pressed.
    for unit in [
        u32::from(b'a'),
        0x3042, // あ
        0xD83C, // 🎮, high half
        0xDFAE, // 🎮, low half
        0x000D, // Enter
        0x0008, // Backspace
    ] {
        // SAFETY: `WM_CHAR` to this shell's own window, with `wParam` the
        // UTF-16 code unit the system would have put there.
        unsafe { ffi::SendMessageW(hwnd, msg::CHAR, unit as usize, 0) };
    }

    let mut text = String::new();
    shell.pump(&mut |event| {
        if let ShellEvent::TextCommit { text: piece, .. } = event {
            text.push_str(&piece);
        }
    });
    assert_eq!(
        text, "aあ🎮",
        "the pair is one character and the controls are not text"
    );
}

#[test]
fn the_pointer_enters_moves_clicks_scrolls_and_leaves() {
    /// Where this test's **first** movement puts the pointer, and therefore
    /// the position the arrival derived from it carries.
    ///
    /// This is the payload that identifies a crossing as ours: a real
    /// movement reports where the physical cursor actually is, so no event
    /// the desktop contributes carries this point.
    const ARRIVED_AT: PhysicalPoint = PhysicalPoint::new(40.0, 30.0);
    /// Where the **second** movement puts it. No arrival may carry this:
    /// the pointer is already inside by then, and `TrackMouseEvent` is
    /// re-armed only on a transition.
    const MOVED_TO: PhysicalPoint = PhysicalPoint::new(41.0, 31.0);

    let mut shell = shell();
    let window = window(&mut shell);
    let hwnd = hwnd_of(&shell, window);

    // **A Windows desktop has a real cursor on it, and it is somewhere.**
    // Showing a window under it delivers a genuine `WM_MOUSEMOVE` that
    // derives the arrival before this test sends anything, so the first
    // synthetic movement below is then not the first movement at all. That
    // is not a runner quirk — it is true of any machine where the pointer
    // happens to be over the new window, this developer's included.
    //
    // The leave is what puts the derived state on a known edge: the backend
    // ignores one for a pointer it already thinks is outside, so this is
    // either a no-op or exactly the transition needed. Nothing here moves the
    // cursor, which would only trade one assumption about the environment for
    // another.
    //
    // # The drain comes *before* the leave, and the order is the whole fix
    //
    // It used to be the other way round, and a `windows-latest` run collected
    // `[(false, None)]` — a leave with no arrival in front of it. The
    // sequence explains itself once written out: the leave marked the pointer
    // outside, then the discarding pump dispatched a **real** `WM_MOUSEMOVE`
    // the desktop had queued, which derived the arrival and threw it away,
    // and the synthetic movements that followed found the pointer already
    // inside. The runner delivers real motion every few milliseconds, so this
    // was never a rare interleaving.
    //
    // Draining first and leaving second closes the gap completely, because
    // `send_mouse` and `send_leave` are `SendMessageW`: they call the window
    // procedure **synchronously**, and nothing between the leave and the
    // first movement pumps. No queued message can be processed in that
    // window, so the arrival below is derived from this test's own movement
    // and from nothing else. What the desktop queued meanwhile is dispatched
    // during the collecting pump — *after* everything sent here, because
    // `pump` translates what the procedure recorded in the order it recorded
    // it. What that does **not** buy is a known position in the sequence:
    // the discarding pump above runs before any of it and can leave a
    // crossing of the desktop's in front, which is what the assertions below
    // are written around.
    shell.pump(&mut |_| {});
    let frame = super::input::client_screen_rect(hwnd).expect("a live window has one");
    send_leave(hwnd);

    // A movement into a window nothing has been over is an arrival, and
    // Windows sends no message for that — it is derived.
    send_mouse(hwnd, msg::MOUSE_MOVE, 0, 40, 30);
    send_mouse(hwnd, msg::MOUSE_MOVE, 0, 41, 31);
    // A press and its release, which also takes and gives back the capture.
    send_mouse(hwnd, msg::L_BUTTON_DOWN, 0, 41, 31);
    send_mouse(hwnd, msg::L_BUTTON_UP, 0, 41, 31);
    // The thumb button, whose identity is in the *high* word.
    send_mouse(hwnd, msg::X_BUTTON_DOWN, 1 << 16, 41, 31);
    send_mouse(hwnd, msg::X_BUTTON_UP, 1 << 16, 41, 31);
    // The wheel, whose position is in **screen** coordinates.
    send_mouse(
        hwnd,
        msg::MOUSE_WHEEL,
        (120usize) << 16,
        frame.left + 41,
        frame.top + 31,
    );
    send_leave(hwnd);

    // Collected rather than compared position by position: a runner whose
    // physical cursor happens to sit over the window contributes real
    // messages of its own, and the claim under test is that ours produced
    // the events below — not that nothing else did.
    let mut names = Vec::new();
    let mut motions = Vec::new();
    let mut buttons = Vec::new();
    let mut wheel = None;
    // Every crossing, carried with the number of movements seen ahead of
    // it. That count is what pairs an arrival with the movement it was
    // derived from: the procedure pushes the derived arrival *before* the
    // motion that triggered it, so the motion sits at exactly that index.
    // Kept alongside rather than compared on the spot so the failure can
    // print both sides.
    let mut crossings: Vec<(bool, Option<PhysicalPoint>, usize)> = Vec::new();
    shell.pump(&mut |event| {
        names.push(event.name());
        match event {
            ShellEvent::PointerMotion { abs, raw_delta, .. } => {
                motions.push(abs);
                assert_eq!(raw_delta, None, "WM_MOUSEMOVE is not raw motion");
            }
            ShellEvent::Button {
                button,
                state,
                position,
                ..
            } => buttons.push((button, state, position)),
            ShellEvent::Wheel {
                delta, position, ..
            } => wheel = Some((delta, position)),
            ShellEvent::PointerFocus {
                entered, position, ..
            } => crossings.push((entered, position, motions.len())),
            _ => {}
        }
    });

    // **Our own crossings are found by their payload, never by their
    // index**, and nothing at all is claimed about the ones the desktop
    // contributes.
    //
    // # Three runs, three failures, and the fourth shape is different in kind
    //
    // This assertion has now been rewritten three times, each version
    // assuming a slightly quieter machine than the last, and `windows-latest`
    // voted against every one. The last of them required the first crossing
    // to be an arrival, and came back with:
    //
    // ```text
    // the first crossing after a leave is the arrival:
    //   [(false, None), (true, Some(PhysicalPoint { x: 40.0, y: 30.0 })), (false, None)]
    //   out of ["PointerFocus", "PointerFocus", "PointerMotion", "PointerMotion",
    //           "Button", ×4, "Wheel", "PointerFocus", "Focus"]
    // ```
    //
    // A leave in front, then our arrival at the injected point, then our
    // leave. The discarding pump above had dispatched a real `WM_MOUSEMOVE`
    // whose derived arrival it threw away, so `send_leave` was a genuine
    // transition rather than the no-op it is on a still desktop — and a
    // crossing nothing in this test's own sequence accounts for was sitting
    // ahead of everything.
    //
    // So the shape changed rather than tightening again: **a live desktop is
    // entitled to insert crossings before, between and after ours**, and any
    // claim about position-in-the-sequence is a claim that it held still.
    // What this test injected is identifiable — the arrival it derives
    // carries [`ARRIVED_AT`], which is a point no real movement reports —
    // so the property is stated about that pair and about nothing else:
    //
    // * an arrival carries the position this test moved to,
    // * the movement it was derived from is the very next motion in the
    //   stream, since the procedure pushes the arrival ahead of it,
    // * the crossing straight after it is a leave carrying no position —
    //   this test's own, and nothing can be dispatched between the two
    //   because every `send_*` here is a synchronous `SendMessageW`, and
    // * **no arrival carries [`MOVED_TO`]**, which is the one-shot claim:
    //   `TrackMouseEvent` is re-armed only on a transition, so a backend
    //   deriving an arrival per movement would announce the second movement
    //   as an arrival too.
    let ours = crossings
        .iter()
        .position(|(entered, at, _)| *entered && *at == Some(ARRIVED_AT))
        .unwrap_or_else(|| {
            panic!(
                "no arrival carried {ARRIVED_AT:?}, which is where this test's first \
                     movement put the pointer: {crossings:?} out of {names:?}"
            )
        });
    let derived_from = crossings[ours].2;
    assert_eq!(
        motions.get(derived_from).copied(),
        Some(Some(ARRIVED_AT)),
        "an arrival carries the position of the movement it was derived from, and that \
             movement is still delivered: motion {derived_from} of {motions:?}. Events {names:?}"
    );
    assert_eq!(
        crossings
            .get(ours + 1)
            .map(|(entered, at, _)| (*entered, *at)),
        Some((false, None)),
        "the leave this test sent follows its arrival and carries no position; every \
             message between the two is a synchronous SendMessageW, so nothing else can be \
             dispatched in there: {crossings:?} out of {names:?}"
    );
    assert!(
        !crossings
            .iter()
            .any(|(entered, at, _)| *entered && *at == Some(MOVED_TO)),
        "the pointer was already inside by the second movement, so {MOVED_TO:?} is a \
             movement and not an arrival; announcing it would mean TrackMouseEvent is re-armed \
             per message rather than per transition: {crossings:?} out of {names:?}"
    );
    assert!(motions.contains(&Some(MOVED_TO)), "{motions:?}");
    assert_eq!(
        buttons,
        vec![
            (PointerButton::Left, ButtonState::Pressed, Some(MOVED_TO)),
            (PointerButton::Left, ButtonState::Released, Some(MOVED_TO)),
            (PointerButton::Back, ButtonState::Pressed, Some(MOVED_TO)),
            (PointerButton::Back, ButtonState::Released, Some(MOVED_TO)),
        ],
        "{names:?}"
    );
    let (delta, position) = wheel.expect("the wheel scrolled: {names:?}");
    assert_eq!(delta, ScrollDelta::Lines { x: 0.0, y: 1.0 });
    assert_eq!(
        position,
        Some(MOVED_TO),
        "the screen coordinates a wheel message carries were converted"
    );
}

// **Held out of the ordinary sweep after three flakes.** This test and
// `minimizing_a_captured_window_releases_the_clip` compare a clip rectangle
// the system applied against one this process computed, and both operands
// move underneath it: the desktop repositions windows, focus is contended
// by whatever else the runner is doing, and this runner changes its display
// set mid-run — the behaviour that made `refresh_clip` refuse a degenerate
// refresh. Two rounds of narrowing the read window (`confined_to_client`,
// and re-deriving after a restore) cut the failures down but did not stop
// them, and they landed on CI for commits that touched no shell code.
//
// `#[ignore]` moves them to `run-win32-e2e.ps1`, which passes
// `--run-ignored all` and runs on a real interactive desktop — the only
// place their preconditions hold. They still gate there; they just stop
// failing the workspace sweep, where nothing guarantees a foreground window
// or a stable display set. Deleting them was the alternative and is worse:
// a process that keeps the cursor clipped after losing focus has taken the
// desktop hostage, and that is worth a test.
#[ignore = "needs an uncontended interactive desktop; run-win32-e2e.ps1 runs it"]
#[test]
fn confining_the_pointer_clips_it_and_losing_focus_gives_the_desktop_back() {
    // The hazard that a compile cannot catch: a process that keeps the
    // cursor clipped after it stops being the foreground window has taken
    // the desktop hostage. The clip is read back from the *system*, because
    // that is the thing being claimed.
    //
    // What the clip is compared against is **not** the client rectangle,
    // and the difference is the runner's answer rather than a loosening.
    // `ClipCursor` intersects with the virtual screen before it applies: the
    // Windows runner's display is 1024×768, which is smaller than the
    // default 1280×720 window once the frame is added, so the clip came back
    // 12 px narrower on the right and the right edge was exactly the screen
    // width. Asserting the client rectangle asserts something the API does
    // not promise; asserting the intersection asserts what it does, and it
    // is the same clamp a game meets on a 1366×768 laptop.
    let mut shell = shell();
    let window = window(&mut shell);
    shell.pump(&mut |_| {});
    let hwnd = hwnd_of(&shell, window);
    let client = super::input::client_screen_rect(hwnd).expect("a live window has one");
    let clipped = client.intersect(virtual_screen());
    assert_ne!(clip_rect(), clipped, "nothing is clipped to start with");

    // The clip follows the focus, so the window has to have it. CI cannot
    // click on a window; the message is what a click would have produced,
    // and the foreground call is what makes `ClipCursor` allowed at all.
    focus_and_confirm(&mut shell, window, hwnd);

    shell
        .set_pointer_mode(window, PointerMode::Confined)
        .expect("POINTER_CONFINE is claimed");
    assert_eq!(clip_rect(), clipped, "the pointer is bounded by the window");

    // **Released synchronously**, inside the window procedure, without a
    // pump — one frame of a hostage desktop is one frame too many.
    send_focus(hwnd, false);
    assert_ne!(clip_rect(), clipped, "focus loss releases the clip");

    // And it comes back with the focus, because the request has not been
    // withdrawn.
    send_focus(hwnd, true);
    assert_eq!(clip_rect(), clipped);
    shell.pump(&mut |_| {});

    // The other order: a mode asked for while the window does **not** have
    // the keyboard gets no clip — the hostage rule does not care why the
    // focus is missing — and takes effect when the keyboard arrives, rather
    // than being lost.
    shell
        .set_pointer_mode(window, PointerMode::Free)
        .expect("start from nothing");
    send_focus(hwnd, false);
    shell.pump(&mut |_| {});
    shell
        .set_pointer_mode(window, PointerMode::Confined)
        .expect("asking is allowed whether or not it takes effect now");
    assert_ne!(
        clip_rect(),
        clipped,
        "an unfocused window does not get the pointer"
    );
    send_focus(hwnd, true);
    assert_eq!(
        clip_rect(),
        clipped,
        "and gets it when the keyboard arrives"
    );
    shell.pump(&mut |_| {});

    // Going back to free withdraws it for good.
    shell
        .set_pointer_mode(window, PointerMode::Free)
        .expect("free always works");
    assert_ne!(clip_rect(), clipped);
    send_focus(hwnd, true);
    assert_ne!(clip_rect(), clipped, "and focus does not resurrect it");
}

// **Held out of the ordinary sweep after three flakes.** This test and
// `minimizing_a_captured_window_releases_the_clip` compare a clip rectangle
// the system applied against one this process computed, and both operands
// move underneath it: the desktop repositions windows, focus is contended
// by whatever else the runner is doing, and this runner changes its display
// set mid-run — the behaviour that made `refresh_clip` refuse a degenerate
// refresh. Two rounds of narrowing the read window (`confined_to_client`,
// and re-deriving after a restore) cut the failures down but did not stop
// them, and they landed on CI for commits that touched no shell code.
//
// `#[ignore]` moves them to `run-win32-e2e.ps1`, which passes
// `--run-ignored all` and runs on a real interactive desktop — the only
// place their preconditions hold. They still gate there; they just stop
// failing the workspace sweep, where nothing guarantees a foreground window
// or a stable display set. Deleting them was the alternative and is worse:
// a process that keeps the cursor clipped after losing focus has taken the
// desktop hostage, and that is worth a test.
#[ignore = "needs an uncontended interactive desktop; run-win32-e2e.ps1 runs it"]
#[test]
fn minimizing_a_captured_window_releases_the_clip() {
    // A minimized window is 0×0, and `client_screen_rect` maps both corners
    // of that through `ClientToScreen` to the *same* point — so a clip
    // re-applied from it pins the cursor there for the whole minimized
    // period. The window is minimized for real rather than handed a
    // synthetic `WM_SIZE(SIZE_MINIMIZED)`: the degenerate 0×0 client
    // rectangle only exists for an actually-iconic window.
    let mut shell = shell();
    let window = window(&mut shell);
    shell.pump(&mut |_| {});
    let hwnd = hwnd_of(&shell, window);

    focus_and_confirm(&mut shell, window, hwnd);

    shell
        .set_pointer_mode(window, PointerMode::Confined)
        .expect("POINTER_CONFINE is claimed");
    // **Read after the clip exists, never before it.** Both operands move:
    // the window can be repositioned by the desktop, and this runner
    // changes its display set mid-run — the same behaviour that made
    // `refresh_clip` refuse a degenerate refresh. A rectangle captured
    // before the call and compared after it turns any of that into a
    // failure of this assertion, which is about whether confining clips to
    // the client area at all.
    let clipped = confined_to_client(hwnd);
    assert_eq!(clip_rect(), clipped, "the pointer is bounded by the window");

    // SAFETY: this shell's own window; SW_MINIMIZE is a documented command.
    unsafe { ffi::ShowWindow(hwnd, value::SW_MINIMIZE) };
    shell.pump(&mut |_| {});
    let released = clip_rect();
    assert!(
        released.right > released.left && released.bottom > released.top,
        "a minimized captured window must not pin the cursor to a point: {released:?}"
    );
    assert_ne!(
        released, clipped,
        "the clip is released, not re-applied from the 0×0 client area"
    );

    // SAFETY: this shell's own window; SW_RESTORE is a documented command.
    unsafe { ffi::ShowWindow(hwnd, value::SW_RESTORE) };
    shell.pump(&mut |_| {});
    // **The keyboard has to come back before the clip can.** `refresh_clip`
    // applies to a captured window only while it is focused, and minimizing
    // took the focus away — so on a runner where something else grabs the
    // foreground in between, restore leaves the clip released and the
    // assertion below reports the *rectangle* being wrong when the real
    // answer is that nobody was focused. What this test is about is which
    // rectangle a restore re-clips from, not who ends up focused.
    focus_and_confirm(&mut shell, window, hwnd);
    // Recomputed, for the reason the confine above is: `clipped` describes
    // a desktop that may have moved since, and this assertion is about
    // which rectangle a restore clips from.
    let restored = confined_to_client(hwnd);
    // Recomputing costs the comparison its teeth if the window is somehow
    // still minimized, because then both sides are the 0×0 area and agree.
    // The defect this test exists for is a restore that clips from exactly
    // that, so the rectangle it compares against has to be a real one.
    assert!(
        restored.right > restored.left && restored.bottom > restored.top,
        "the window did not actually restore, so the comparison below \
             would pass on two degenerate rectangles: {restored:?}"
    );
    assert_eq!(
        clip_rect(),
        restored,
        "restore re-clips from the real client rectangle"
    );

    // Going back to free withdraws it for good.
    shell
        .set_pointer_mode(window, PointerMode::Free)
        .expect("free always works");
    assert_ne!(clip_rect(), clipped);
}

#[test]
fn a_second_window_takes_the_pointer_capture_from_the_first() {
    // One cursor, so one clip. A shell with two windows must not leave the
    // first one reporting `Locked` while the second holds the clip.
    let mut shell = shell();
    let first = window(&mut shell);
    let second = shell
        .create_window(&WindowDesc {
            title: "crcbl P5C W2 — second",
            size: LogicalSize::new(640.0, 480.0),
            ..WindowDesc::default()
        })
        .expect("a second window");
    shell.pump(&mut |_| {});

    shell
        .set_pointer_mode(first, PointerMode::Locked)
        .expect("lock");
    shell
        .set_pointer_mode(second, PointerMode::Confined)
        .expect("confine");
    assert_eq!(
        shell.window_state(first).expect("live").pointer_mode,
        PointerMode::Free,
        "the first window no longer holds anything"
    );
    assert_eq!(
        shell.window_state(second).expect("live").pointer_mode,
        PointerMode::Confined
    );
}

#[test]
fn hiding_the_cursor_is_balanced_however_many_times_it_is_asked_for() {
    // The `ShowCursor` reference-count bug, observed through the count
    // itself: two hides and one show leave the cursor invisible for the
    // rest of the process's life, with no error anywhere.
    let mut shell = shell();
    let window = window(&mut shell);
    shell.pump(&mut |_| {});
    let hwnd = hwnd_of(&shell, window);

    // **Focus first, and proved rather than assumed** — every count below
    // is a consequence of it, because `refresh_cursor_visibility` hides
    // only for a window that has the keyboard. See [`focus_and_confirm`].
    focus_and_confirm(&mut shell, window, hwnd);
    // **Read after the focus, not before it.** A count taken first is a
    // count from a different focus state, and the difference would be
    // charged to `set_cursor`.
    let baseline = cursor_display_count();

    shell.set_cursor(window, None).expect("hide");
    assert_eq!(cursor_display_count(), baseline - 1, "one hide is owed");
    shell.set_cursor(window, None).expect("hide again");
    assert_eq!(
        cursor_display_count(),
        baseline - 1,
        "asking twice must not owe two shows"
    );

    shell
        .set_cursor(window, Some(CursorIcon::Text))
        .expect("a shape reveals it again");
    assert_eq!(cursor_display_count(), baseline);
    shell
        .set_cursor(window, Some(CursorIcon::Crosshair))
        .expect("and another shape changes nothing about visibility");
    assert_eq!(cursor_display_count(), baseline);

    // Losing focus gives the cursor back, which is what stops a game in
    // mouselook leaving the desktop without a pointer.
    shell.set_cursor(window, None).expect("hide");
    assert_eq!(cursor_display_count(), baseline - 1);
    send_focus(hwnd, false);
    shell.pump(&mut |_| {});
    assert_eq!(cursor_display_count(), baseline, "focus loss reveals it");
}

#[test]
fn a_cursor_shape_is_answered_from_the_window_procedure() {
    // `WM_SETCURSOR` is the only message allowed to set a shape, and the
    // system uses `DefWindowProc`'s answer — the class arrow — unless ours
    // says it handled it. Sending the message runs exactly the path a
    // mouse movement would.
    let mut shell = shell();
    let window = window(&mut shell);
    shell
        .set_cursor(window, Some(CursorIcon::Text))
        .expect("an I-beam is a stock Windows cursor");
    let hwnd = hwnd_of(&shell, window);

    // SAFETY: `WM_SETCURSOR` to this shell's own window. `wParam` is the
    // window under the cursor and `lParam` packs the hit-test code in its
    // low word and the triggering message in its high one.
    let handled = unsafe {
        ffi::SendMessageW(
            hwnd,
            msg::SET_CURSOR,
            hwnd as usize,
            value::HT_CLIENT as isize,
        )
    };
    assert_eq!(handled, 1, "the shape was applied, not the class cursor");

    // The frame is the system's: answering for it would replace the resize
    // arrows on the window border with an I-beam.
    const HT_BOTTOM_RIGHT: isize = 17;
    // SAFETY: as above, with the hit-test code for a corner of the frame.
    let frame = unsafe { ffi::SendMessageW(hwnd, msg::SET_CURSOR, hwnd as usize, HT_BOTTOM_RIGHT) };
    assert_ne!(frame, 1, "the non-client area is left to DefWindowProc");
}

// **The third test of this class, and it is here for the same reason as the
// two above.** Windows refuses `SetCursorPos` from a process that is not in
// the foreground, so this test's real precondition is the foreground
// having taken — which nothing on a shared runner guarantees, and which
// `focus_and_confirm` now checks with the system before the warp. It failed
// on `5889a3c`, a commit that changed a JavaScript file and two markdown
// files, reading back exactly the client origin: the warp had not moved the
// pointer at all.
//
// What that flake bought was a real defect, now fixed: `warp_to_client`
// discarded `SetCursorPos`'s `BOOL`, so a warp that moved nothing returned
// success and the mismatch surfaced here as a coordinate off by precisely
// the offset asked for. It now returns `ShellError::Backend`, which is why
// the `expect` below is the assertion that fires first.
#[ignore = "needs an uncontended interactive desktop; run-win32-e2e.ps1 runs it"]
#[test]
fn warping_the_pointer_moves_it_to_a_position_in_the_window() {
    // What `POINTER_WARP` claims, and the conversion it depends on: the
    // seam is in window pixels and `SetCursorPos` is in screen ones.
    let mut shell = shell();
    let window = window(&mut shell);
    shell.pump(&mut |_| {});
    let hwnd = hwnd_of(&shell, window);
    let client = super::input::client_screen_rect(hwnd).expect("a live window has one");
    focus_and_confirm(&mut shell, window, hwnd);

    shell
        .warp_pointer(window, PhysicalPoint::new(30.0, 20.0))
        .expect("POINTER_WARP is claimed");
    let mut point = super::super::ffi::Point::default();
    // SAFETY: `point` is a live, initialised `POINT` the call writes into.
    let read = unsafe { ffi::GetCursorPos(&raw mut point) };
    assert_ne!(read, 0, "a session with a desktop has a cursor position");
    assert_eq!(
        (point.x, point.y),
        (client.left + 30, client.top + 20),
        "the warp landed in screen space at the client offset asked for"
    );
}

#[test]
fn a_decorated_window_really_is_decorated() {
    // What `SERVER_DECORATIONS` claims, observed rather than asserted from
    // the style word: a windowed window's outer rectangle is larger than
    // its client area, and a borderless one's is not.
    let mut shell = shell();
    let window = window(&mut shell);
    shell.pump(&mut |_| {});
    let client = shell
        .window_state(window)
        .expect("live")
        .size()
        .expect("configured");
    let frame = Win32Shell::frame_for(
        super::super::geometry::styles(DisplayMode::Windowed, true),
        shell.window(window).expect("live").dpi,
    );
    assert!(frame.height > 0, "a caption has a height: {frame:?}");
    assert!(frame.width > 0, "a border has a width: {frame:?}");
    assert!(!client.is_empty());

    let flush = Win32Shell::frame_for(
        super::super::geometry::styles(DisplayMode::Borderless { monitor: None }, true),
        shell.window(window).expect("live").dpi,
    );
    assert_eq!(
        flush,
        super::super::geometry::Frame::default(),
        "WS_POPUP has no frame at all"
    );
}

#[test]
fn the_size_constraints_reach_the_window_procedure() {
    // `WM_GETMINMAXINFO` and `WM_SIZING` are otherwise only sent by a user
    // dragging a window edge. Sending them by hand runs the real procedure
    // against the real cached limits, which is the only way to test the
    // aspect lock and the track sizes without a mouse.
    let mut shell = shell();
    let window = window(&mut shell);
    shell
        .set_constraints(
            window,
            SizeConstraints {
                min: Some(LogicalSize::new(640.0, 480.0)),
                max: Some(LogicalSize::new(1920.0, 1080.0)),
                aspect: Some(AspectRatio::WIDESCREEN),
            },
        )
        .expect("constraints are a request every backend accepts");
    let hwnd = hwnd_of(&shell, window);
    let scale = shell.window(window).expect("live").scale_factor;

    let mut info = MinMaxInfo::default();
    // SAFETY: sending a message to this shell's own window on the thread
    // that owns it; `info` is a live, initialised `MINMAXINFO` and
    // `WM_GETMINMAXINFO` is documented to take a pointer to one.
    unsafe {
        ffi::SendMessageW(
            hwnd,
            msg::GET_MIN_MAX_INFO,
            0,
            (&raw mut info) as super::super::ffi::Lparam,
        );
    }
    let min_client_width = (640.0 * scale).round() as i32;
    assert!(
        info.pt_min_track_size.x >= min_client_width,
        "the minimum is the client minimum plus the frame: {info:?}"
    );
    assert!(
        info.pt_max_track_size.x >= (1920.0 * scale).round() as i32,
        "{info:?}"
    );

    let mut rect = Rect {
        left: 100,
        top: 100,
        right: 1100,
        bottom: 1100,
    };
    // SAFETY: as above; `WM_SIZING` is documented to take a pointer to the
    // `RECT` the system is about to apply, and rewriting it is how a resize
    // is constrained.
    let handled = unsafe {
        ffi::SendMessageW(
            hwnd,
            msg::SIZING,
            super::super::ffi::value::WMSZ_RIGHT,
            (&raw mut rect) as super::super::ffi::Lparam,
        )
    };
    assert_eq!(handled, 1, "the rectangle was rewritten");
    assert_eq!(rect.width(), 1000, "the dragged edge never moves");
    assert!(
        rect.height() < 700,
        "16:9 makes a 1000-wide window a little over 560 tall: {rect:?}"
    );
    assert!(rect.height() > 450, "{rect:?}");
}

#[test]
fn a_close_request_is_intercepted_and_can_be_refused_then_accepted() {
    let mut shell = shell();
    let window = window(&mut shell);
    shell.pump(&mut |_| {});
    assert!(matches!(
        shell.reply_close_request(window, CloseReply::Keep),
        Err(ShellError::NoPendingCloseRequest { .. })
    ));

    let hwnd = hwnd_of(&shell, window);
    // SAFETY: `WM_CLOSE` to this shell's own window — exactly what the
    // title bar's close button sends.
    unsafe { ffi::SendMessageW(hwnd, msg::CLOSE, 0, 0) };
    let mut asked = false;
    shell.pump(&mut |event| {
        asked |= matches!(event, ShellEvent::CloseRequested { window: asked } if asked == window);
    });
    assert!(asked, "WM_CLOSE is a question, not a notification");
    assert!(
        shell
            .window_state(window)
            .expect("still open")
            .close_pending
    );

    shell
        .reply_close_request(window, CloseReply::Keep)
        .expect("refusing is allowed");
    assert!(
        shell.window_state(window).is_ok(),
        "the window survives a refusal — DefWindowProc would have destroyed it"
    );
    assert!(!shell.window_state(window).expect("open").close_pending);

    // SAFETY: as above.
    unsafe { ffi::SendMessageW(hwnd, msg::CLOSE, 0, 0) };
    shell.pump(&mut |_| {});
    shell
        .reply_close_request(window, CloseReply::Close)
        .expect("and accepting closes it");
    assert!(matches!(
        shell.window_state(window),
        Err(ShellError::InvalidWindow { .. })
    ));
}

#[test]
fn both_offered_formats_round_trip_and_the_reader_picks() {
    // What `CLIPBOARD` claims, against the real window station: text for
    // everything else on the desktop, a registered format for the engine's
    // own, and both on the clipboard at once so the reader chooses. The RON
    // has to come back **byte for byte** — a blob carrying the heap's
    // padding would not parse.
    let mut shell = shell();
    let window = window(&mut shell);
    shell.pump(&mut |_| {});

    const RON: &str = "(kind:\"node\",id:7,name:\"日本語 🎮\")";
    shell
        .clipboard_offer(
            window,
            &[ClipboardOffer::text("hello 🎮"), ClipboardOffer::ron(RON)],
        )
        .expect("claiming the clipboard needs no user interaction on Win32");
    assert!(
        !clipboard_held_by(hwnd_of(&shell, window)),
        "the guard closed the clipboard on the way out of the offer"
    );

    assert_eq!(
        paste(&mut shell, window, MimeType::TextUtf8).text(),
        Some("hello 🎮"),
        "an astral codepoint survives the UTF-16 round trip"
    );
    assert_eq!(
        paste(&mut shell, window, MimeType::CrcblRon),
        ClipboardContent::Bytes(RON.as_bytes().to_vec()),
        "the engine's own format is lossless, padding and all"
    );
    assert!(
        !clipboard_held_by(hwnd_of(&shell, window)),
        "and on the way out of every read as well"
    );

    // A format that was not offered is `Empty` — "holds nothing in that
    // format", which the seam merges with an empty clipboard on purpose —
    // and emphatically not `Unavailable`, which would mean the read failed.
    assert_eq!(
        paste(&mut shell, window, MimeType::UriList),
        ClipboardContent::Empty
    );
}

#[test]
fn an_empty_offer_empties_the_clipboard_and_an_empty_payload_does_not() {
    // The two cases `ClipboardContent` exists to keep apart, and the one
    // place they are easy to conflate: releasing the clipboard and
    // publishing nothing.
    let mut shell = shell();
    let window = window(&mut shell);
    shell.pump(&mut |_| {});

    // A successful transfer of zero bytes. `Bytes(vec![])`, not `Empty` —
    // reporting `Empty` here tells an editor its paste failed when it did
    // not.
    shell
        .clipboard_offer(window, &[ClipboardOffer::text("")])
        .expect("an empty payload is a payload");
    assert_eq!(
        paste(&mut shell, window, MimeType::TextUtf8),
        ClipboardContent::Bytes(Vec::new())
    );

    // An empty *slice* is the release. Windows has no owner to give up, so
    // this is `EmptyClipboard` and afterwards there is nothing there.
    shell
        .clipboard_offer(window, &[])
        .expect("an empty slice releases");
    assert_eq!(
        paste(&mut shell, window, MimeType::TextUtf8),
        ClipboardContent::Empty
    );
    assert!(!clipboard_held_by(hwnd_of(&shell, window)));
}

#[test]
fn a_read_is_answered_exactly_once_and_needs_no_second_pump() {
    // Obligation 4 in the form this backend takes: the read happens inside
    // `clipboard_request`, so the answer is queued before it returns and
    // there is no list of outstanding reads to lose one from. `paste`
    // asserts the "exactly one, on the first pump" half; this asserts that
    // nothing arrives afterwards, which is the half a duplicated answer
    // would break.
    let mut shell = shell();
    let window = window(&mut shell);
    shell.pump(&mut |_| {});
    shell
        .clipboard_offer(window, &[ClipboardOffer::text("once")])
        .expect("publish");

    let first = shell
        .clipboard_request(window, MimeType::TextUtf8)
        .expect("accepted");
    let second = shell
        .clipboard_request(window, MimeType::CrcblRon)
        .expect("accepted");
    assert_ne!(first, second, "ids are unique within a shell");

    let mut answered = Vec::new();
    shell.pump(&mut |event| {
        if let ShellEvent::ClipboardData {
            request, content, ..
        } = event
        {
            answered.push((request, content));
        }
    });
    assert_eq!(answered.len(), 2, "one each: {answered:?}");
    assert_eq!(answered[0].0, first, "in the order they were asked");
    assert_eq!(answered[1].0, second);
    assert_eq!(answered[0].1.text(), Some("once"));
    assert_eq!(
        answered[1].1,
        ClipboardContent::Empty,
        "the RON format was not published"
    );

    let mut again = Vec::new();
    shell.pump(&mut |event| {
        if matches!(event, ShellEvent::ClipboardData { .. }) {
            again.push(event.name());
        }
    });
    assert!(
        again.is_empty(),
        "answered once, not once per pump: {again:?}"
    );
}

#[test]
fn a_window_destroyed_before_the_pump_is_not_answered_at_all() {
    // Where obligation 4 meets obligation 1, and the stronger of the two
    // wins: an answer naming a handle the consumer has already been told is
    // stale is worse than a request that ends without one, because the
    // consumer can see the stale handle and cannot see the missing answer.
    // The X11 backend decides this the same way.
    let mut shell = shell();
    let window = window(&mut shell);
    let survivor = shell
        .create_window(&WindowDesc {
            title: "crcbl P5C W3 — survivor",
            size: LogicalSize::new(640.0, 480.0),
            ..WindowDesc::default()
        })
        .expect("a second window");
    shell.pump(&mut |_| {});
    shell
        .clipboard_offer(window, &[ClipboardOffer::text("gone")])
        .expect("publish");

    let doomed = shell
        .clipboard_request(window, MimeType::TextUtf8)
        .expect("accepted while the window was live");
    let kept = shell
        .clipboard_request(survivor, MimeType::TextUtf8)
        .expect("accepted");
    shell.destroy_window(window).expect("destroy");

    let mut answered = Vec::new();
    shell.pump(&mut |event| {
        if let ShellEvent::ClipboardData { request, .. } = event {
            answered.push(request);
        }
    });
    assert_eq!(
        answered,
        [kept],
        "the dead window's answer went with it and the live one's did not"
    );
    assert_ne!(doomed, kept);
}

#[test]
fn a_drop_becomes_one_event_per_file_at_the_position_it_landed() {
    // What `DRAG_DROP` claims, driven through the real `WM_DROPFILES` path:
    // shell32's `DragQueryFileW` reads a real `HDROP`, the procedure
    // finishes it, and the paths reach the pump through the side queue that
    // keeps them out of a `Copy` event.
    let mut shell = shell();
    let window = drop_window(&mut shell);
    shell.pump(&mut |_| {});
    let hwnd = hwnd_of(&shell, window);

    send_drop(
        hwnd,
        &[r"C:\assets\My Scene.ron", r"C:\assets\プロジェクト.png"],
        12,
        34,
    );
    let dropped = pump_drops(&mut shell);
    assert_eq!(dropped.len(), 2, "one event per file: {dropped:?}");
    assert_eq!(dropped[0].0, window);
    assert_eq!(dropped[0].1, PathBuf::from(r"C:\assets\My Scene.ron"));
    assert_eq!(
        dropped[1].1,
        PathBuf::from(r"C:\assets\プロジェクト.png"),
        "a non-ASCII name survives UTF-16 without a lossy step"
    );
    assert_eq!(
        dropped[0].2,
        Some(PhysicalPoint::new(12.0, 34.0)),
        "the drop point is in client pixels, which is what the seam means"
    );

    // Two drops in one pump stay two drops, and each marker claims its own
    // payload — the pairing that hands one scene the other's assets when it
    // is wrong.
    send_drop(hwnd, &[r"C:\a.png"], 1, 2);
    send_drop(hwnd, &[r"C:\b.png", r"C:\c.png"], 3, 4);
    let batch = pump_drops(&mut shell);
    let paths: Vec<PathBuf> = batch.iter().map(|(_, path, _)| path.clone()).collect();
    assert_eq!(
        paths,
        [
            PathBuf::from(r"C:\a.png"),
            PathBuf::from(r"C:\b.png"),
            PathBuf::from(r"C:\c.png")
        ],
        "in order: {batch:?}"
    );
    assert_eq!(batch[0].2, Some(PhysicalPoint::new(1.0, 2.0)));
    assert_eq!(
        batch[1].2,
        Some(PhysicalPoint::new(3.0, 4.0)),
        "the second drop's point, not the first's"
    );
}

#[test]
fn a_window_that_did_not_ask_for_drops_reports_none() {
    // `accept_drops` is off by default and load-bearing rather than
    // advisory. The system enforces it — no `WS_EX_ACCEPTFILES`, no
    // message — so the message here is one the system would never send, and
    // what is under test is the backend's own half of the gate.
    let mut shell = shell();
    let window = window(&mut shell);
    shell.pump(&mut |_| {});
    let hwnd = hwnd_of(&shell, window);

    send_drop(hwnd, &[r"C:\unwanted.png"], 5, 5);
    assert!(
        pump_drops(&mut shell).is_empty(),
        "the descriptor said no, and that is the answer"
    );
}

#[test]
fn the_drop_registration_survives_a_trip_through_borderless() {
    // `DragAcceptFiles` sets `WS_EX_ACCEPTFILES`, and a mode change
    // *rewrites the extended style word*. Carrying the bit is what stops
    // drops silently ceasing to arrive the first time a window goes
    // fullscreen — with nothing anywhere reporting it.
    let mut shell = shell();
    let window = drop_window(&mut shell);
    shell.pump(&mut |_| {});
    let hwnd = hwnd_of(&shell, window);
    let accepts = || {
        // SAFETY: reading this shell's own window's extended style.
        let ex_style = unsafe { ffi::GetWindowLongPtrW(hwnd, value::GWL_EX_STYLE) } as u32;
        ex_style & super::super::ffi::style::EX_ACCEPT_FILES
    };
    assert_ne!(accepts(), 0, "created with drops accepted");

    let primary = shell
        .monitors()
        .iter()
        .find(|monitor| monitor.is_primary)
        .expect("a primary monitor")
        .id;
    shell
        .set_mode(
            window,
            DisplayMode::Borderless {
                monitor: Some(primary),
            },
        )
        .expect("borderless");
    assert_ne!(accepts(), 0, "the style rewrite carried the bit");
    shell
        .set_mode(window, DisplayMode::Windowed)
        .expect("and back");
    assert_ne!(accepts(), 0);
    shell.pump(&mut |_| {});

    // And it still works, which is the thing the bit is a proxy for.
    send_drop(hwnd, &[r"C:\after.ron"], 7, 8);
    let dropped = pump_drops(&mut shell);
    assert_eq!(dropped.len(), 1, "{dropped:?}");
    assert_eq!(dropped[0].1, PathBuf::from(r"C:\after.ron"));
}

#[test]
fn opening_the_clipboard_reports_how_it_went_and_always_closes_it() {
    // The mechanism, not a wall clock: an open that took it first time and
    // one that retried seven times take indistinguishable amounts of time on
    // a loaded runner, and only `Opened` can say which happened. What is
    // asserted is that it was not *refused* and that the guard's `Drop` ran
    // — the second being what stops this process locking every other
    // application out of the clipboard.
    // Deliberately no "nothing is open before we start" precondition: this
    // test is *about* contention — it asserts the open was not refused —
    // and a foreign process holding the clipboard is the case the retry
    // budget exists for, not a reason to fail before starting.
    let mut shell = shell();
    let window = window(&mut shell);
    let hwnd = hwnd_of(&shell, window);

    let opened = {
        let board = super::super::clipboard::Clipboard::open(hwnd)
            .expect("an idle desktop lets a process open the clipboard");
        assert!(clipboard_held_by(hwnd), "held for the guard's lifetime");
        board.opened()
    };
    assert!(
        !clipboard_held_by(hwnd),
        "and given back the moment the guard leaves scope"
    );
    assert!(
        !matches!(opened, Opened::Refused { .. }),
        "another process held the clipboard for the whole budget: {opened:?}"
    );
    if let Opened::After { attempts } = opened {
        assert!(
            attempts <= super::super::clipboard::OPEN_ATTEMPTS,
            "the retry is bounded: {attempts}"
        );
    }
}

#[test]
fn every_method_taking_a_window_rejects_a_stale_handle() {
    // Implementor obligation 1, method by method: a destroyed handle must
    // never act on whatever window took the slot.
    let mut shell = shell();
    let stale = window(&mut shell);
    shell.destroy_window(stale).expect("destroy");

    let invalid = |result: Result<(), ShellError>| {
        assert!(
            matches!(result, Err(ShellError::InvalidWindow { .. })),
            "{result:?}"
        );
    };
    invalid(shell.window_state(stale).map(|_| ()));
    invalid(shell.set_title(stale, "gone"));
    invalid(shell.set_visible(stale, true));
    invalid(shell.set_mode(stale, DisplayMode::Windowed));
    invalid(shell.set_constraints(stale, SizeConstraints::NONE));
    invalid(shell.surface_target(stale).map(|_| ()));
    invalid(shell.set_cursor(stale, None));
    invalid(shell.set_pointer_mode(stale, PointerMode::Free));
    invalid(shell.warp_pointer(stale, PhysicalPoint::ORIGIN));
    invalid(shell.reply_close_request(stale, CloseReply::Keep));
    invalid(shell.clipboard_offer(stale, &[]));
    invalid(
        shell
            .clipboard_request(stale, MimeType::TextUtf8)
            .map(|_| ()),
    );
    invalid(shell.destroy_window(stale));
    assert!(!shell.clipboard_readable(stale));
}

#[test]
fn two_windows_coexist_and_are_told_apart() {
    // What `MULTI_WINDOW` claims.
    let mut shell = shell();
    let first = window(&mut shell);
    let second = shell
        .create_window(&WindowDesc {
            title: "crcbl P5C W1 — second",
            size: LogicalSize::new(640.0, 480.0),
            ..WindowDesc::default()
        })
        .expect("a second window");
    assert_ne!(first, second);
    assert_ne!(hwnd_of(&shell, first), hwnd_of(&shell, second));

    let mut resized = Vec::new();
    shell.pump(&mut |event| {
        if let ShellEvent::Resized { window, size, .. } = event {
            resized.push((window, size));
        }
    });
    assert_eq!(resized.len(), 2, "one configuration each: {resized:?}");
    let sizes: Vec<PhysicalSize> = resized.iter().map(|(_, size)| *size).collect();
    assert_ne!(sizes[0], sizes[1], "and they are not the same window twice");

    // Destroying one leaves the other alone.
    shell.destroy_window(first).expect("destroy");
    assert!(shell.window_state(second).is_ok());
    assert!(shell.window_state(first).is_err());
}

#[test]
fn a_short_timed_wait_is_not_rounded_up_to_the_clock_tick() {
    // The frame loop's windowed idle is a few milliseconds; the default
    // system clock tick is 15.6 ms. A wait that ends on the tick instead of
    // its timeout capped a windowed game near 64 frames a second, which is
    // what the high-resolution timer in `wait` is for.
    //
    // No window: a thread with none gets no desktop traffic, so the waits
    // end by their timeout and the median below is a measurement of the
    // timer rather than of the desktop's mood. Only timed-out waits count.
    const TIMEOUT: Duration = Duration::from_millis(4);
    /// Well under the default tick and well over a timer that works, so a
    /// busy machine does not fail it and a tick-bound wait always does.
    const CEILING: Duration = Duration::from_millis(10);
    const ATTEMPTS: usize = 40;
    /// Timed-out waits needed before the median means anything.
    const SAMPLES: usize = 10;

    let mut shell = shell();
    let mut slept: Vec<Duration> = (0..ATTEMPTS)
        .filter_map(|_| {
            let start = std::time::Instant::now();
            (shell.wait(Some(TIMEOUT)) == Wake::TimedOut).then(|| start.elapsed())
        })
        .collect();
    assert!(
        slept.len() >= SAMPLES,
        "only {} of {ATTEMPTS} waits timed out on a thread with no window, too few to measure",
        slept.len()
    );
    slept.sort_unstable();
    let median = slept[slept.len() / 2];
    assert!(
        median < CEILING,
        "a {TIMEOUT:?} wait slept a median of {median:?} over {} timed-out waits; a wait \
             rounded to the system clock tick sleeps about 15.6 ms",
        slept.len()
    );
}

#[test]
fn wait_events_genuinely_blocks() {
    // What `EVENT_WAIT` claims, asserted as the *mechanism* it is rather
    // than as a duration. The first version of this measured three 50 ms
    // waits against a wall clock and failed on the runner at 47 ms with
    // nothing to say why — a wait that found a queued message, a wait that
    // failed outright, and a wait that never slept are all instant, and a
    // clock cannot tell them apart. `Wake` can.
    //
    // A window that has just been created and shown has messages waiting,
    // so the queue is drained to quiescence first: `Wake::Message` before
    // that point is correct behaviour and asserting against it would be
    // asserting that the drain does not work.
    //
    // Whether it reached quiescence is *reported* rather than asserted, and
    // that is this test's second correction — see below. On a runner where
    // the desktop delivers messages unbidden every few milliseconds, "the
    // queue went quiet" is a fact about the machine's mood.
    let mut shell = shell();
    let _window = window(&mut shell);
    let mut settled = false;
    for _ in 0..16 {
        shell.pump(&mut |_| {});
        if shell.wait(Some(Duration::from_millis(0))) == Wake::TimedOut {
            settled = true;
            break;
        }
    }

    // # The observable is "it slept", not "it reached the timeout"
    //
    // **This assertion has been wrong twice, in the same direction, and the
    // evidence for the current shape is worth keeping so nobody tightens it
    // back.** It first measured a wall clock and failed at 47 ms of a 50 ms
    // timeout. It then asked for `Wake::TimedOut` on at least one of five
    // attempts — and a `windows-latest` run printed all five:
    //
    // ```text
    // attempt 0: Message after  5.7433ms, queue 0x80008, message id 799
    // attempt 1: Message after 16.0896ms, queue 0x400040, message None
    // attempt 2: Message after 31.6668ms, queue 0x400040, message None
    // attempt 3: Message after   299.7µs, queue 0x80008, message id 96
    // attempt 4: Message after 10.9571ms, queue 0x20002, message id 512
    // ```
    //
    // 799 is `WM_DWMNCRENDERINGCHANGED` and 512 is `WM_MOUSEMOVE`: the
    // desktop is delivering compositor notifications and **real mouse
    // movement** to this window every few milliseconds. An idle window with a
    // drained queue does not exist there, and no amount of draining will make
    // one.
    //
    // Read those numbers again, though, because they also say the wait
    // *works*: four of the five blocked for between 5.7 ms and 31.7 ms before
    // something woke them. That is what blocking looks like on a busy
    // desktop. Only attempt 3, at 299 µs, returned to an already-full queue.
    //
    // So the assertion is now the thing that actually separates a blocking
    // wait from a no-op one. [`ShellCaps::EVENT_WAIT`] claims the call
    // *sleeps until something happens*; it never claimed the machine would go
    // quiet. A wait that does nothing returns in microseconds **every single
    // time** — five of those in a row is unmistakable and is what fails here.
    // A wait woken after milliseconds slept, whatever woke it.
    //
    // Every attempt carries its evidence, because two of the CI rounds spent
    // on this were settled only by the failure printing more than the last
    // one did. The queue word says what kind of message woke it; the `MSG`
    // beside it says which one — an id, an `hwnd` and two parameters, which
    // is a thing that can be looked up rather than guessed at. `None` there,
    // with a bit set, is the observation that identified `QS_SENDMESSAGE`;
    // see [`Win32Shell::wait`].
    const ATTEMPTS: usize = 5;
    const TIMEOUT: Duration = Duration::from_millis(50);
    // The full timeout, loose on the low side because Windows' timer
    // granularity is 15.6 ms. Only a `TimedOut` is held to it: a
    // `WAIT_TIMEOUT` that arrived instantly would mean the timeout was not
    // the one that was asked for, so it is not evidence of sleeping.
    const FLOOR: Duration = Duration::from_millis(40);
    // The line between "it slept and was woken" and "it never slept", drawn
    // between the two populations the run above measured: 299 µs for a return
    // to a full queue, 5.7 ms for the shortest genuine block. An order of
    // magnitude of clearance on each side, and no dependence on the desktop
    // being quiet.
    const BLOCKED: Duration = Duration::from_millis(2);

    let mut slept = false;
    let mut woken = Vec::new();
    for attempt in 0..ATTEMPTS {
        let start = Instant::now();
        let woke = shell.wait(Some(TIMEOUT));
        let waited = start.elapsed();
        woken.push(format!(
            "attempt {attempt}: {woke:?} after {waited:?}, queue {:#06x}, message {:?}",
            Win32Shell::queue_status(),
            Win32Shell::peek_pending()
        ));
        slept = match woke {
            Wake::TimedOut => waited >= FLOOR,
            // Woken early, which on a live desktop is the ordinary case —
            // but a wait has to have slept to be woken out of.
            _ => waited >= BLOCKED,
        };
        if slept {
            break;
        }
        // Whatever woke it is still in the queue; leaving it there would
        // wake the next attempt too.
        shell.pump(&mut |_| {});
    }
    assert!(
        slept,
        "every one of {ATTEMPTS} waits came back inside {BLOCKED:?} without reaching its \
             {TIMEOUT:?} timeout, so this backend's EVENT_WAIT never sleeps at all (the queue \
             {}): {woken:#?}",
        if settled {
            "was drained to quiescence first"
        } else {
            "never went quiet in 16 drain-and-check rounds, so a return to a full queue is \
                 expected — but not five in a row, each in microseconds"
        }
    );
}

#[test]
fn the_monitors_are_enumerated_with_one_primary_and_a_scale_factor() {
    let shell = shell();
    let monitors = shell.monitors();
    assert!(
        !monitors.is_empty(),
        "a session with a window station has at least one display"
    );
    assert_eq!(
        monitors.iter().filter(|monitor| monitor.is_primary).count(),
        1,
        "exactly one primary: {monitors:?}"
    );
    for monitor in monitors {
        assert!(!monitor.name.is_empty(), "the device name: {monitor:?}");
        assert!(!monitor.size().is_empty(), "{monitor:?}");
        assert!(monitor.scale_factor >= 0.5, "{monitor:?}");
        assert!(
            monitor.work_area.width <= monitor.bounds.width
                && monitor.work_area.height <= monitor.bounds.height,
            "the work area is the bounds minus the taskbar: {monitor:?}"
        );
        assert_eq!(
            shell.monitor(monitor.id).map(|found| &found.name),
            Some(&monitor.name),
            "every id resolves"
        );
    }
    let ids: Vec<MonitorId> = monitors.iter().map(|monitor| monitor.id).collect();
    let mut unique = ids.clone();
    unique.sort_unstable();
    unique.dedup();
    assert_eq!(unique.len(), ids.len(), "ids are not reused: {ids:?}");
    assert_eq!(shell.monitor(MonitorId(u32::MAX)), None);
}

#[test]
fn hiding_a_window_is_real_and_reversible() {
    let mut shell = shell();
    let window = window(&mut shell);
    shell.pump(&mut |_| {});
    assert!(shell.window_state(window).expect("live").visible);

    shell.set_visible(window, false).expect("hide");
    assert!(
        !shell.window_state(window).expect("live").visible,
        "visibility is read back from the system, not assumed from the request"
    );
    shell.set_visible(window, true).expect("show");
    assert!(shell.window_state(window).expect("live").visible);

    // A window created hidden is hidden, which is the fix for the
    // black-window flash on startup.
    let hidden = shell
        .create_window(&WindowDesc {
            visible: false,
            ..WindowDesc::default()
        })
        .expect("create hidden");
    assert!(!shell.window_state(hidden).expect("live").visible);
}

#[test]
fn the_title_round_trips_and_a_nul_is_refused() {
    let mut shell = shell();
    let window = window(&mut shell);
    shell
        .set_title(window, "crcbl — 日本語 🎮")
        .expect("UTF-16");
    assert!(matches!(
        shell.set_title(window, "save\0nothing"),
        Err(ShellError::InvalidDescriptor(_))
    ));
    assert!(matches!(
        shell.create_window(&WindowDesc {
            app_id: "sh.kryptic\0crcbl",
            ..WindowDesc::default()
        }),
        Err(ShellError::InvalidDescriptor(_)),
    ));
}

#[test]
fn the_surface_target_exists_before_the_window_is_configured() {
    // The seam's promise: the *handle* exists as soon as the window does,
    // so a HAL surface can be created immediately and only the swapchain
    // waits for the first `Resized`.
    let mut shell = shell();
    let window = window(&mut shell);
    assert_eq!(shell.window_state(window).expect("live").size(), None);

    let target = shell.surface_target(window).expect("available already");
    let SurfaceTarget::Win32 { hinstance, hwnd } = target else {
        panic!("the Win32 backend produces a Win32 target: {target:?}");
    };
    assert_eq!(hwnd.as_ptr(), hwnd_of(&shell, window));
    assert_eq!(hinstance, shell.instance);

    // And it does not change across a resize, which is what makes
    // "re-query after a mode change" a complete rule.
    shell.pump(&mut |_| {});
    assert_eq!(shell.surface_target(window).expect("still live"), target);
}

#[test]
fn the_backend_reports_itself_and_is_reachable_by_name() {
    let shell = shell();
    assert_eq!(shell.backend(), ShellBackend::Win32);
    assert_eq!(ShellBackend::Win32.as_str(), "win32");
    let boxed = crate::open_backend(ShellBackend::Win32).expect("registered on Windows");
    assert_eq!(boxed.backend(), ShellBackend::Win32);
    // And the automatic path finds it, because it is the only backend on
    // this platform. Skipped when the environment has an opinion, since
    // `open` is documented to obey it.
    if std::env::var(crate::BACKEND_ENV_VAR).is_err() {
        let automatic = crate::open().expect("auto-selection has one candidate here");
        assert_eq!(automatic.backend(), ShellBackend::Win32);
    }
}
