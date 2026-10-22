//! A Win32 window resized from outside the shell — not a test module, and not
//! a test target either: `tests/win32/` holds no `main.rs`, so Cargo compiles
//! nothing here on its own.
//!
//! **Two suites pull this in with `#[path]`** — `tests/windowed_e2e.rs` and
//! `tests/dx12_windowed_e2e.rs` — because both resize a window the way a user's
//! drag does, by `SetWindowPos` on the `HWND`, and then check that a swapchain
//! follows. `crates/crcbl-shell/tests/win32_e2e.rs` declares the same `user32`
//! calls for the same reason: the shell's own FFI is private, and a test that
//! resizes a window from outside the shell must not go through it.

use core::ffi::c_void;

use crcbl::shell::PhysicalSize;

/// `RECT`.
#[repr(C)]
#[derive(Clone, Copy, Default)]
struct Rect {
    left: i32,
    top: i32,
    right: i32,
    bottom: i32,
}

/// `SWP_NOMOVE`.
const SWP_NO_MOVE: u32 = 0x0002;
/// `SWP_NOZORDER`.
const SWP_NO_Z_ORDER: u32 = 0x0004;
/// `SWP_NOACTIVATE`.
const SWP_NO_ACTIVATE: u32 = 0x0010;

#[link(name = "user32")]
unsafe extern "system" {
    fn GetWindowRect(hwnd: *mut c_void, rect: *mut Rect) -> i32;
    fn GetClientRect(hwnd: *mut c_void, rect: *mut Rect) -> i32;
    fn SetWindowPos(
        hwnd: *mut c_void,
        after: *mut c_void,
        x: i32,
        y: i32,
        width: i32,
        height: i32,
        flags: u32,
    ) -> i32;
}

/// `SetWindowPos`, sized so the **client area** of `window` lands on `size`.
///
/// `SetWindowPos` takes the outer size, frame included, and the frame's
/// thickness depends on the style and the monitor's DPI. So the frame is
/// measured off the window as it stands — outer minus client — rather than
/// computed from a style this file would have to keep in step with the shell's.
///
/// `window` must be a live window of this process: both suites pass the `HWND`
/// their own shell handed over, and destroy it only after the last resize.
pub fn resize_client_area(window: *mut c_void, size: PhysicalSize) {
    let mut outer = Rect::default();
    let mut client = Rect::default();
    // SAFETY: `window` is a live window of this process, and both rectangles
    // are initialised locals the calls write into.
    let measured = unsafe {
        GetWindowRect(window, &raw mut outer) != 0 && GetClientRect(window, &raw mut client) != 0
    };
    assert!(
        measured,
        "GetWindowRect/GetClientRect refused a live window"
    );
    let frame_width = (outer.right - outer.left) - (client.right - client.left);
    let frame_height = (outer.bottom - outer.top) - (client.bottom - client.top);
    let width = i32::try_from(size.width).expect("a window-sized width") + frame_width;
    let height = i32::try_from(size.height).expect("a window-sized height") + frame_height;
    // SAFETY: as above. `SWP_NOMOVE` makes the position arguments unused and
    // the null insert-after handle is unused under `SWP_NOZORDER`;
    // `SWP_NOACTIVATE` keeps the resize from taking the foreground.
    unsafe {
        SetWindowPos(
            window,
            core::ptr::null_mut(),
            0,
            0,
            width,
            height,
            SWP_NO_MOVE | SWP_NO_Z_ORDER | SWP_NO_ACTIVATE,
        )
    };
}
