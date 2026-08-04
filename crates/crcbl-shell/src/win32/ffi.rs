//! Hand-written FFI to `user32`, `gdi32`, `shcore` and `kernel32`, and the
//! wire structures the Win32 backend passes across it.
//!
//! # Decision: `#[link]`, not `dlopen` — the opposite of the two Linux backends
//!
//! [`x11::ffi`](crate::x11::ffi) and [`wayland::ffi`](crate::wayland::ffi) both
//! load their libraries with `dlopen`, and both state the same reason: the
//! registry in [`backend`](crate::backend) is a fall-through list, so an entry
//! has to be able to fail at *runtime*. A `DT_NEEDED` on `libwayland-client.so.0`
//! kills the process in `ld.so` before [`open`](crate::open) runs, on any
//! machine that does not have it — and a machine without libwayland is an
//! ordinary machine.
//!
//! **That premise does not hold here, and the asymmetry is deliberate rather
//! than an oversight.** `user32.dll`, `gdi32.dll` and `kernel32.dll` are part of
//! the operating system: a Windows that lacks them is not a Windows this engine
//! failed to support, it is a Windows that cannot run any process at all.
//! `shcore.dll` ships with every Windows since 8.1, which is below this
//! backend's floor. There is nothing for a runtime probe to discover, and
//! `LoadLibraryW` + `GetProcAddress` for four system libraries would buy exactly
//! one thing: a fallible code path with no failure.
//!
//! The registry consequence is the one that matters and it is satisfied:
//! Windows has a single backend, so there is no fall-through to protect. If a
//! second Windows backend is ever added, the entry that must be able to fail is
//! *that* one.
//!
//! # Decision: Windows 10 version 1607 is the floor
//!
//! Set by DPI, not by anything else here. `docs/plan/15-windowing.md` requires
//! "one `scale_factor` concept over … per-monitor-v2 (Windows)", and the three
//! APIs that make per-monitor DPI work — `SetProcessDpiAwarenessContext`,
//! `GetDpiForWindow` and `AdjustWindowRectExForDpi` — all arrive in 1607.
//! Everything else this module declares is Windows XP-era.
//!
//! One wrinkle is worth stating rather than discovering: the *awareness context
//! value* [`DPI_AWARENESS_CONTEXT_PER_MONITOR_AWARE_V2`](value::DPI_PER_MONITOR_AWARE_V2)
//! is documented from 1703, one release after the function that takes it. On a
//! 1607 host the call therefore fails with `ERROR_INVALID_PARAMETER` instead of
//! being missing, which is why
//! `shell::set_dpi_awareness` inspects
//! `GetLastError` rather than treating a `FALSE` return as fatal. Below 1607
//! the process fails to *start*, in the loader, exactly as the Linux backends'
//! `dlopen` argument predicts — and that is the price of linking, paid
//! knowingly on a platform where the libraries cannot be absent.
//!
//! # What is deliberately not declared
//!
//! | Not used | What it would buy | Why not |
//! | --- | --- | --- |
//! | `EnumDisplayDevicesW` | a monitor's marketing name | it usually answers "Generic PnP Monitor", which is worse than the device name for telling two displays apart |
//! | `QueryDisplayConfig` | exact refresh as a numerator/denominator | the only way to see 59.94 rather than 60 — recorded in `docs/backlog.md`, not implemented |
//! | `ImmGetContext` and the `WM_IME_*` family | a real input method | W2 leaves [`TEXT_IME`](crate::ShellCaps::TEXT_IME) clear rather than claiming what `WM_CHAR` alone earns; see [`Win32Shell::caps`](super::Win32Shell) |
//! | `ToUnicode` | the character a key produces | it **consumes** dead-key state, so calling it would eat the accent `WM_CHAR` was about to deliver. [`MapVirtualKeyW`] with `MAPVK_VK_TO_CHAR` answers the same question without side effects |
//! | `GetAsyncKeyState` | modifier state | it reads the hardware *now*, not at the message's time. [`GetKeyboardState`] is the snapshot that belongs to the message being processed |
//! | `OpenClipboard`, `RegisterDragDrop` | clipboard and drag-drop | W3, and the capability set says so today |
//! | `SetTimer` | a frame during a modal resize loop | see the `win32` module docs — it needs a callback the seam does not have |

#![allow(non_snake_case)]

use core::ffi::c_void;

// ---------------------------------------------------------------------------
// Scalar aliases
// ---------------------------------------------------------------------------

/// Every Win32 handle this backend touches: `HWND`, `HINSTANCE`, `HMONITOR`,
/// `HCURSOR`, `HBRUSH`, `HMENU`, `HDC`.
///
/// One alias rather than a newtype per kind, for the reason the X11 backend
/// uses one `Cookie` for every `xcb_*_cookie_t`: they are the same thing on the
/// wire, and the type system cannot check which one a message actually carried.
/// Where a distinction matters it is in the parameter name.
pub type Handle = *mut c_void;

/// `WPARAM`.
pub type Wparam = usize;
/// `LPARAM`.
pub type Lparam = isize;
/// `LRESULT`.
pub type Lresult = isize;
/// `BOOL` — zero is false, anything else is true. **Not** `bool`: Win32
/// functions return `TRUE` as `1` but callbacks may return any non-zero value,
/// and `bool` has exactly two valid bit patterns.
pub type Bool32 = i32;

/// `WNDPROC`.
pub type WindowProc = unsafe extern "system" fn(Handle, u32, Wparam, Lparam) -> Lresult;

/// `MONITORENUMPROC`.
pub type MonitorEnumProc = unsafe extern "system" fn(Handle, Handle, *mut Rect, Lparam) -> Bool32;

// ---------------------------------------------------------------------------
// Structures
// ---------------------------------------------------------------------------

/// `RECT` — left, top, right, bottom, and the right/bottom edges are
/// **exclusive**.
///
/// The single most common Win32 mistake is treating it as a position and a
/// size; it is two corners, and `GetClientRect` always answers with a zero
/// origin.
#[repr(C)]
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Rect {
    /// Left edge, inclusive.
    pub left: i32,
    /// Top edge, inclusive.
    pub top: i32,
    /// Right edge, exclusive.
    pub right: i32,
    /// Bottom edge, exclusive.
    pub bottom: i32,
}

impl Rect {
    /// Width, clamped at zero — a rectangle whose right edge is left of its
    /// left one is degenerate rather than negative-width.
    pub const fn width(self) -> i32 {
        if self.right > self.left {
            self.right - self.left
        } else {
            0
        }
    }

    /// Height, on the same terms as [`width`](Self::width).
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

/// `MSG`.
///
/// `l_private` is present in current SDK headers and absent in older ones; the
/// struct is 48 bytes either way because of tail padding, so declaring it is
/// free and reading a message into a buffer the system considers larger than
/// ours is the failure that is not free.
#[repr(C)]
#[derive(Clone, Copy, Debug, Default)]
pub struct Msg {
    /// Window the message is for, or null for a thread message.
    pub hwnd: Handle,
    /// `WM_*`.
    pub message: u32,
    /// First parameter.
    pub w_param: Wparam,
    /// Second parameter.
    pub l_param: Lparam,
    /// `GetTickCount` milliseconds when the message was posted — the clock
    /// [`TimeBase`](super::TimeBase) rebases.
    pub time: u32,
    /// Cursor position in screen coordinates when the message was posted.
    pub pt: Point,
    /// Undocumented tail; see the type docs.
    pub l_private: u32,
}

// The derived `Default` zeroes every field, including the `Handle`, which is
// what `PeekMessageW` expects to be handed: it writes the whole structure and
// reads none of it.

/// `WNDCLASSEXW`.
#[repr(C)]
#[derive(Clone, Copy, Debug)]
pub struct WndClassExW {
    /// `size_of::<WndClassExW>()`, which the system validates.
    pub cb_size: u32,
    /// `CS_*` class styles.
    pub style: u32,
    /// The window procedure.
    pub lpfn_wnd_proc: WindowProc,
    /// Extra bytes after the class structure. Zero.
    pub cb_cls_extra: i32,
    /// Extra bytes after each window instance. Zero — this backend keeps its
    /// per-window state in `GWLP_USERDATA` instead; see [`super::proc`].
    pub cb_wnd_extra: i32,
    /// Module that owns the class.
    pub h_instance: Handle,
    /// Large icon. Null: the default is used.
    pub h_icon: Handle,
    /// Class cursor.
    pub h_cursor: Handle,
    /// Background brush.
    pub hbr_background: Handle,
    /// Menu resource name. Null.
    pub lpsz_menu_name: *const u16,
    /// Class name, NUL-terminated UTF-16.
    pub lpsz_class_name: *const u16,
    /// Small icon. Null.
    pub h_icon_sm: Handle,
}

/// `CREATESTRUCTW`, of which this backend reads exactly one field.
///
/// [`lp_create_params`](Self::lp_create_params) is how the `lpParam` handed to
/// `CreateWindowExW` reaches the window procedure, and it is the only way:
/// `WM_NCCREATE` arrives before `CreateWindowExW` has returned, so there is no
/// `HWND` to have stored anything against yet.
#[repr(C)]
#[derive(Clone, Copy, Debug)]
pub struct CreateStructW {
    /// The `lpParam` argument of `CreateWindowExW`.
    pub lp_create_params: *mut c_void,
    /// Module handle.
    pub h_instance: Handle,
    /// Menu handle.
    pub h_menu: Handle,
    /// Parent window.
    pub hwnd_parent: Handle,
    /// Height.
    pub cy: i32,
    /// Width.
    pub cx: i32,
    /// Top.
    pub y: i32,
    /// Left.
    pub x: i32,
    /// Window style.
    pub style: i32,
    /// Window name.
    pub lpsz_name: *const u16,
    /// Class name.
    pub lpsz_class: *const u16,
    /// Extended style.
    pub dw_ex_style: u32,
}

/// `MINMAXINFO` — what `WM_GETMINMAXINFO` asks the window to fill in.
///
/// The two fields that matter are the **track** sizes, which bound an
/// interactive resize. `pt_max_size` bounds a *maximize* and is deliberately
/// left alone: a maximized window that is smaller than the monitor because a
/// size constraint said so is not what a maximize button means.
#[repr(C)]
#[derive(Clone, Copy, Debug, Default)]
pub struct MinMaxInfo {
    /// Reserved.
    pub pt_reserved: Point,
    /// Size of a maximized window.
    pub pt_max_size: Point,
    /// Position of a maximized window.
    pub pt_max_position: Point,
    /// Smallest the user may drag the window to, in **window** pixels.
    pub pt_min_track_size: Point,
    /// Largest the user may drag the window to, in **window** pixels.
    pub pt_max_track_size: Point,
}

/// `WINDOWPLACEMENT` — the whole of a window's restored geometry and show
/// state.
///
/// Saved on the way into borderless and restored on the way out, because
/// remembering only the window rectangle loses whether the window was
/// maximized, and restoring a maximized window as a floating one of the same
/// size is visibly wrong.
#[repr(C)]
#[derive(Clone, Copy, Debug, Default)]
pub struct WindowPlacement {
    /// `size_of::<WindowPlacement>()`, which the system validates.
    pub length: u32,
    /// `WPF_*` flags.
    pub flags: u32,
    /// `SW_*` show state.
    pub show_cmd: u32,
    /// Minimized position.
    pub pt_min_position: Point,
    /// Maximized position.
    pub pt_max_position: Point,
    /// Restored rectangle, in workspace coordinates.
    pub rc_normal_position: Rect,
}

/// `MONITORINFOEXW`.
///
/// The `EX` half is [`sz_device`](Self::sz_device), and it is why this is the
/// structure used rather than plain `MONITORINFO`: the device name is the only
/// stable per-monitor identity Win32 offers without a second API, and
/// [`MonitorId`](crate::MonitorId) must not be reused within a session.
#[repr(C)]
#[derive(Clone, Copy, Debug)]
pub struct MonitorInfoExW {
    /// `size_of::<MonitorInfoExW>()`, which selects the `EX` form.
    pub cb_size: u32,
    /// The monitor's rectangle in virtual-screen coordinates.
    pub rc_monitor: Rect,
    /// The same, minus taskbars and appbars.
    pub rc_work: Rect,
    /// `MONITORINFOF_PRIMARY`.
    pub dw_flags: u32,
    /// `\\.\DISPLAY1` and friends, NUL-padded.
    pub sz_device: [u16; 32],
}

impl Default for MonitorInfoExW {
    fn default() -> Self {
        Self {
            cb_size: size_of::<Self>() as u32,
            rc_monitor: Rect::default(),
            rc_work: Rect::default(),
            dw_flags: 0,
            sz_device: [0; 32],
        }
    }
}

/// `DEVMODEW`, flattened.
///
/// The two unions in the real structure are replaced by byte-equivalent arrays,
/// because this backend reads exactly one field out of it —
/// [`dm_display_frequency`](Self::dm_display_frequency) — and a union whose
/// display arm we never touch is layout, not meaning. The size assertion in
/// this module's tests is what makes that safe: get the padding wrong and every
/// field after the mistake is read from the wrong offset, which produces a
/// plausible number rather than a crash.
#[repr(C)]
#[derive(Clone, Copy, Debug)]
pub struct DevModeW {
    /// Device name.
    pub dm_device_name: [u16; 32],
    /// Structure version.
    pub dm_spec_version: u16,
    /// Driver version.
    pub dm_driver_version: u16,
    /// `size_of::<DevModeW>()`, which the driver validates.
    pub dm_size: u16,
    /// Extra driver bytes after this structure. Zero.
    pub dm_driver_extra: u16,
    /// Which fields are meaningful.
    pub dm_fields: u32,
    /// The printer/display union: `POINTL dmPosition` and two `DWORD`s in the
    /// display arm.
    pub dm_union_position: [u32; 4],
    /// Colour.
    pub dm_color: i16,
    /// Duplex.
    pub dm_duplex: i16,
    /// Y resolution.
    pub dm_y_resolution: i16,
    /// TrueType option.
    pub dm_tt_option: i16,
    /// Collation.
    pub dm_collate: i16,
    /// Form name.
    pub dm_form_name: [u16; 32],
    /// Logical pixels per inch.
    pub dm_log_pixels: u16,
    /// Bits per pixel.
    pub dm_bits_per_pel: u32,
    /// Width in pixels.
    pub dm_pels_width: u32,
    /// Height in pixels.
    pub dm_pels_height: u32,
    /// The display-flags/n-up union.
    pub dm_display_flags: u32,
    /// **Refresh rate in whole hertz.** The only field this backend reads.
    pub dm_display_frequency: u32,
    /// ICM method.
    pub dm_icm_method: u32,
    /// ICM intent.
    pub dm_icm_intent: u32,
    /// Media type.
    pub dm_media_type: u32,
    /// Dither type.
    pub dm_dither_type: u32,
    /// Reserved.
    pub dm_reserved1: u32,
    /// Reserved.
    pub dm_reserved2: u32,
    /// Panning width.
    pub dm_panning_width: u32,
    /// Panning height.
    pub dm_panning_height: u32,
}

impl Default for DevModeW {
    fn default() -> Self {
        // Zeroed apart from `dm_size`, which is how the driver knows which
        // fields of which generation of the structure it may write.
        Self {
            dm_device_name: [0; 32],
            dm_spec_version: 0,
            dm_driver_version: 0,
            dm_size: size_of::<Self>() as u16,
            dm_driver_extra: 0,
            dm_fields: 0,
            dm_union_position: [0; 4],
            dm_color: 0,
            dm_duplex: 0,
            dm_y_resolution: 0,
            dm_tt_option: 0,
            dm_collate: 0,
            dm_form_name: [0; 32],
            dm_log_pixels: 0,
            dm_bits_per_pel: 0,
            dm_pels_width: 0,
            dm_pels_height: 0,
            dm_display_flags: 0,
            dm_display_frequency: 0,
            dm_icm_method: 0,
            dm_icm_intent: 0,
            dm_media_type: 0,
            dm_dither_type: 0,
            dm_reserved1: 0,
            dm_reserved2: 0,
            dm_panning_width: 0,
            dm_panning_height: 0,
        }
    }
}

/// `TRACKMOUSEEVENT` — what `TrackMouseEvent` is asked to watch for.
///
/// Needed because **Windows has no `WM_MOUSEENTER`**. A window is told when the
/// pointer leaves and never when it arrives, and only if it asked: this
/// structure is the asking, and it re-arms one shot at a time. See
/// [`proc`](super::proc) for where the entry half is derived from.
#[repr(C)]
#[derive(Clone, Copy, Debug, Default)]
pub struct TrackMouse {
    /// `size_of::<TrackMouse>()`, which the system validates.
    pub cb_size: u32,
    /// `TME_*`.
    pub dw_flags: u32,
    /// The window to watch.
    pub hwnd_track: Handle,
    /// Hover timeout, unused without `TME_HOVER`.
    pub dw_hover_time: u32,
}

/// `RAWINPUTDEVICE` — one usage page/usage pair to receive `WM_INPUT` for.
#[repr(C)]
#[derive(Clone, Copy, Debug, Default)]
pub struct RawInputDevice {
    /// HID usage page. `0x01` is the generic desktop page.
    pub us_usage_page: u16,
    /// HID usage within the page. `0x02` is the mouse.
    pub us_usage: u16,
    /// `RIDEV_*`.
    pub dw_flags: u32,
    /// Where `WM_INPUT` is delivered. Null means "follow the keyboard focus",
    /// which is what this backend wants — see [`input`](super::input).
    pub hwnd_target: Handle,
}

/// `RAWINPUTHEADER`.
#[repr(C)]
#[derive(Clone, Copy, Debug, Default)]
pub struct RawInputHeader {
    /// `RIM_TYPE*`.
    pub dw_type: u32,
    /// Size of the whole `RAWINPUT` this heads.
    pub dw_size: u32,
    /// The device that produced it. Not used here; see
    /// [`input`](super::input) on why device ids are constants.
    pub h_device: Handle,
    /// The `wParam` of the `WM_INPUT` that carried it.
    pub w_param: Wparam,
}

/// `RAWMOUSE`.
///
/// The two-byte hole after [`us_flags`](Self::us_flags) is the C union's
/// alignment, spelled out rather than left to the compiler: the union's first
/// arm is a `ULONG`, so it starts at offset 4 whatever `usFlags` did. Naming it
/// changes nothing the compiler would not have done — `#[repr(C)]` inserts the
/// same two bytes — and it is here so a reader diffing this against `winuser.h`
/// can see that the gap was noticed rather than missed.
///
/// What the layout assertion in this module's tests actually catches is a field
/// that is **absent or the wrong width**, which moves every offset after it.
/// That is the real hazard: reading [`l_last_x`](Self::l_last_x) from offset 8
/// instead of 12 gives half a button mask as a mouse delta, and a plausible
/// number rather than a crash. Same shape as [`DevModeW`]'s, checked the same
/// way.
#[repr(C)]
#[derive(Clone, Copy, Debug, Default)]
pub struct RawMouse {
    /// `MOUSE_MOVE_RELATIVE`, `MOUSE_MOVE_ABSOLUTE`, `MOUSE_VIRTUAL_DESKTOP`.
    /// **The field that decides what the two coordinates mean.**
    pub us_flags: u16,
    /// The union's alignment padding.
    pub padding: u16,
    /// Button transitions. Not read: the button *messages* carry position and
    /// ordering, and raw button edges would double every click.
    pub ul_buttons: u32,
    /// Raw button state, likewise unread.
    pub ul_raw_buttons: u32,
    /// Relative motion, or an absolute coordinate — see
    /// [`us_flags`](Self::us_flags).
    pub l_last_x: i32,
    /// See [`l_last_x`](Self::l_last_x).
    pub l_last_y: i32,
    /// Driver-supplied extra information.
    pub ul_extra_information: u32,
}

/// `RAWINPUT`, narrowed to its mouse arm.
///
/// The real structure's tail is a union of `RAWMOUSE`, `RAWKEYBOARD` and
/// `RAWHID`, and `RAWMOUSE` is the largest of the two fixed-size arms — this
/// backend registers only the mouse usage, so nothing else can arrive. The
/// [`header`](Self::header)'s `dwType` is checked before the mouse arm is read,
/// which is what makes narrowing it sound rather than hopeful.
#[repr(C)]
#[derive(Clone, Copy, Debug, Default)]
pub struct RawInput {
    /// Type, size and source device.
    pub header: RawInputHeader,
    /// Valid only when `header.dw_type` is `RIM_TYPE_MOUSE`.
    pub mouse: RawMouse,
}

// ---------------------------------------------------------------------------
// Constants
// ---------------------------------------------------------------------------

/// Window and extended window styles.
pub mod style {
    /// `WS_POPUP` — no frame, no caption. Borderless is this bit and the
    /// absence of the others.
    pub const POPUP: u32 = 0x8000_0000;
    /// `WS_VISIBLE`.
    pub const VISIBLE: u32 = 0x1000_0000;
    /// `WS_CLIPSIBLINGS`.
    pub const CLIP_SIBLINGS: u32 = 0x0400_0000;
    /// `WS_CLIPCHILDREN`.
    pub const CLIP_CHILDREN: u32 = 0x0200_0000;
    /// `WS_CAPTION` — the title bar, and `WS_BORDER | WS_DLGFRAME`.
    pub const CAPTION: u32 = 0x00C0_0000;
    /// `WS_SYSMENU` — the system menu, and therefore the close button.
    pub const SYS_MENU: u32 = 0x0008_0000;
    /// `WS_THICKFRAME` — the resize border.
    pub const THICK_FRAME: u32 = 0x0004_0000;
    /// `WS_MINIMIZEBOX`.
    pub const MINIMIZE_BOX: u32 = 0x0002_0000;
    /// `WS_MAXIMIZEBOX`.
    pub const MAXIMIZE_BOX: u32 = 0x0001_0000;
    /// `WS_OVERLAPPEDWINDOW`.
    pub const OVERLAPPED_WINDOW: u32 =
        CAPTION | SYS_MENU | THICK_FRAME | MINIMIZE_BOX | MAXIMIZE_BOX;

    /// `WS_EX_APPWINDOW` — force a taskbar button.
    pub const EX_APP_WINDOW: u32 = 0x0004_0000;
    /// `WS_EX_WINDOWEDGE`.
    pub const EX_WINDOW_EDGE: u32 = 0x0000_0100;
}

/// The messages the window procedure recognizes.
pub mod msg {
    /// `WM_DESTROY`.
    pub const DESTROY: u32 = 0x0002;
    /// `WM_MOVE` — the window moved, so a pointer clip built from its client
    /// rectangle is now in the wrong place.
    pub const MOVE: u32 = 0x0003;
    /// `WM_SIZE`.
    pub const SIZE: u32 = 0x0005;
    /// `WM_SETFOCUS`.
    pub const SET_FOCUS: u32 = 0x0007;
    /// `WM_KILLFOCUS`.
    pub const KILL_FOCUS: u32 = 0x0008;
    /// `WM_CLOSE`.
    pub const CLOSE: u32 = 0x0010;
    /// `WM_SETCURSOR` — asked on every pointer movement over the window, and
    /// the **only** place a cursor shape may be set.
    pub const SET_CURSOR: u32 = 0x0020;
    /// `WM_GETMINMAXINFO`.
    pub const GET_MIN_MAX_INFO: u32 = 0x0024;
    /// `WM_DISPLAYCHANGE`.
    pub const DISPLAY_CHANGE: u32 = 0x007E;
    /// `WM_NCCREATE`.
    pub const NC_CREATE: u32 = 0x0081;
    /// `WM_NCDESTROY`.
    pub const NC_DESTROY: u32 = 0x0082;
    /// `WM_INPUT` — a raw input report.
    pub const INPUT: u32 = 0x00FF;
    /// `WM_KEYDOWN`.
    pub const KEY_DOWN: u32 = 0x0100;
    /// `WM_KEYUP`.
    pub const KEY_UP: u32 = 0x0101;
    /// `WM_CHAR` — one UTF-16 code unit of committed text.
    pub const CHAR: u32 = 0x0102;
    /// `WM_SYSKEYDOWN` — a key pressed with Alt held, or F10.
    pub const SYS_KEY_DOWN: u32 = 0x0104;
    /// `WM_SYSKEYUP`.
    pub const SYS_KEY_UP: u32 = 0x0105;
    /// `WM_MOUSEMOVE`.
    pub const MOUSE_MOVE: u32 = 0x0200;
    /// `WM_LBUTTONDOWN`.
    pub const L_BUTTON_DOWN: u32 = 0x0201;
    /// `WM_LBUTTONUP`.
    pub const L_BUTTON_UP: u32 = 0x0202;
    /// `WM_RBUTTONDOWN`.
    pub const R_BUTTON_DOWN: u32 = 0x0204;
    /// `WM_RBUTTONUP`.
    pub const R_BUTTON_UP: u32 = 0x0205;
    /// `WM_MBUTTONDOWN`.
    pub const M_BUTTON_DOWN: u32 = 0x0207;
    /// `WM_MBUTTONUP`.
    pub const M_BUTTON_UP: u32 = 0x0208;
    /// `WM_MOUSEWHEEL`.
    pub const MOUSE_WHEEL: u32 = 0x020A;
    /// `WM_XBUTTONDOWN` — the two thumb buttons, told apart by `wParam`.
    pub const X_BUTTON_DOWN: u32 = 0x020B;
    /// `WM_XBUTTONUP`.
    pub const X_BUTTON_UP: u32 = 0x020C;
    /// `WM_MOUSEHWHEEL` — the tilt wheel.
    pub const MOUSE_H_WHEEL: u32 = 0x020E;
    /// `WM_SIZING`.
    pub const SIZING: u32 = 0x0214;
    /// `WM_CAPTURECHANGED` — somebody else took the mouse capture, so ours is
    /// gone whether we released it or not.
    pub const CAPTURE_CHANGED: u32 = 0x0215;
    /// `WM_EXITSIZEMOVE` — the modal drag loop finished.
    pub const EXIT_SIZE_MOVE: u32 = 0x0232;
    /// `WM_MOUSELEAVE` — the one-shot `TrackMouseEvent` armed for.
    pub const MOUSE_LEAVE: u32 = 0x02A3;
    /// `WM_DPICHANGED`.
    pub const DPI_CHANGED: u32 = 0x02E0;
}

/// Everything else, by the name it has in the SDK.
pub mod value {
    /// `SIZE_MINIMIZED`, the `wParam` of a `WM_SIZE` that means "iconified".
    pub const SIZE_MINIMIZED: usize = 1;

    /// `GWL_STYLE`.
    pub const GWL_STYLE: i32 = -16;
    /// `GWL_EXSTYLE`.
    pub const GWL_EX_STYLE: i32 = -20;
    /// `GWLP_USERDATA` — where the window procedure finds this shell's shared
    /// state. See the `proc` module.
    pub const GWLP_USERDATA: i32 = -21;

    /// `SW_HIDE`.
    pub const SW_HIDE: i32 = 0;
    /// `SW_SHOWNORMAL`.
    pub const SW_SHOW_NORMAL: i32 = 1;

    /// `SWP_NOSIZE`.
    pub const SWP_NO_SIZE: u32 = 0x0001;
    /// `SWP_NOMOVE`.
    pub const SWP_NO_MOVE: u32 = 0x0002;
    /// `SWP_NOZORDER`.
    pub const SWP_NO_Z_ORDER: u32 = 0x0004;
    /// `SWP_NOACTIVATE`.
    pub const SWP_NO_ACTIVATE: u32 = 0x0010;
    /// `SWP_FRAMECHANGED` — **required** after a style change, or the system
    /// keeps drawing the old frame until something else forces a recalculation.
    pub const SWP_FRAME_CHANGED: u32 = 0x0020;
    /// `SWP_NOOWNERZORDER`.
    pub const SWP_NO_OWNER_Z_ORDER: u32 = 0x0200;

    /// `PM_REMOVE`.
    pub const PM_REMOVE: u32 = 0x0001;

    /// `MONITORINFOF_PRIMARY`.
    pub const MONITOR_PRIMARY: u32 = 0x0000_0001;
    /// `MONITOR_DEFAULTTONEAREST`.
    pub const MONITOR_DEFAULT_TO_NEAREST: u32 = 0x0000_0002;

    /// `MDT_EFFECTIVE_DPI` — the scale the *user* chose, which is the one every
    /// other window on the desktop is laid out at.
    pub const MDT_EFFECTIVE_DPI: i32 = 0;

    /// `ENUM_CURRENT_SETTINGS`.
    pub const ENUM_CURRENT_SETTINGS: u32 = 0xFFFF_FFFF;

    /// `DPI_AWARENESS_CONTEXT_PER_MONITOR_AWARE_V2`, which is a pseudo-handle
    /// rather than an enumerator.
    pub const DPI_PER_MONITOR_AWARE_V2: isize = -4;

    /// `USER_DEFAULT_SCREEN_DPI` — the DPI that is 100%.
    pub const DEFAULT_DPI: u32 = 96;

    /// `CW_USEDEFAULT` — "you choose", for a position with no monitor to
    /// place against.
    ///
    /// The literal is `0x80000000` as a **signed** `int`, which is
    /// `i32::MIN` — not a large positive number, and a window placed at a
    /// large positive number is off the desktop.
    pub const CW_USE_DEFAULT: i32 = i32::MIN;

    /// `IDC_ARROW`, as the integer resource id `MAKEINTRESOURCE` wraps.
    pub const IDC_ARROW: usize = 32_512;
    /// `IDC_IBEAM`.
    pub const IDC_IBEAM: usize = 32_513;
    /// `IDC_WAIT`.
    pub const IDC_WAIT: usize = 32_514;
    /// `IDC_CROSS`.
    pub const IDC_CROSS: usize = 32_515;
    /// `IDC_SIZENWSE`.
    pub const IDC_SIZE_NWSE: usize = 32_642;
    /// `IDC_SIZENESW`.
    pub const IDC_SIZE_NESW: usize = 32_643;
    /// `IDC_SIZEWE`.
    pub const IDC_SIZE_WE: usize = 32_644;
    /// `IDC_SIZENS`.
    pub const IDC_SIZE_NS: usize = 32_645;
    /// `IDC_SIZEALL`.
    pub const IDC_SIZE_ALL: usize = 32_646;
    /// `IDC_NO`.
    pub const IDC_NO: usize = 32_648;
    /// `IDC_HAND`.
    pub const IDC_HAND: usize = 32_649;
    /// `IDC_APPSTARTING`.
    pub const IDC_APP_STARTING: usize = 32_650;

    /// `BLACK_BRUSH`.
    pub const BLACK_BRUSH: i32 = 4;
    /// `CS_HREDRAW | CS_VREDRAW` — repaint the whole client area on a resize,
    /// so a window being dragged wider does not show stale pixels in the strip
    /// that just appeared.
    pub const CS_REDRAW: u32 = 0x0002 | 0x0001;

    /// `ERROR_ACCESS_DENIED` — what `SetProcessDpiAwarenessContext`
    /// reports when the awareness was already set, by a manifest or by an
    /// earlier call.
    pub const ERROR_ACCESS_DENIED: u32 = 5;
    /// `ERROR_CLASS_ALREADY_EXISTS`.
    pub const ERROR_CLASS_ALREADY_EXISTS: u32 = 1410;

    /// `QS_ALLINPUT` — every kind of message wakes the wait.
    pub const QS_ALL_INPUT: u32 = 0x04FF;
    /// `MWMO_INPUTAVAILABLE` — also wake for messages already in the queue,
    /// not only for ones that arrive after the call. Without it a wait entered
    /// with a message pending sleeps out its whole timeout.
    pub const MWMO_INPUT_AVAILABLE: u32 = 0x0004;
    /// `INFINITE`.
    pub const INFINITE: u32 = 0xFFFF_FFFF;
    /// `WAIT_OBJECT_0` — with a handle count of zero this means one thing only:
    /// a message is available.
    pub const WAIT_OBJECT_0: u32 = 0;
    /// `WAIT_TIMEOUT` — the wait slept for its whole timeout, which is the
    /// outcome [`ShellCaps::EVENT_WAIT`](crate::ShellCaps::EVENT_WAIT) claims is
    /// reachable.
    pub const WAIT_TIMEOUT: u32 = 258;
    /// `WAIT_FAILED`. `GetLastError` says why, and a wait that fails returns
    /// *immediately* — which is indistinguishable from a wait that woke, unless
    /// the return value is read.
    pub const WAIT_FAILED: u32 = 0xFFFF_FFFF;

    /// `HTCLIENT` — the `WM_SETCURSOR` hit-test code for the drawable area.
    ///
    /// Every other value is a piece of the frame the *system* owns a cursor
    /// for: the resize arrows on a border, the arrow over the caption. Setting
    /// ours for those would replace a working resize affordance with an arrow.
    pub const HT_CLIENT: u32 = 1;

    /// `TME_LEAVE`.
    pub const TME_LEAVE: u32 = 0x0000_0002;

    /// `XBUTTON1` — the `wParam` high word of a `WM_XBUTTON*`, "back".
    pub const XBUTTON1: u32 = 0x0001;
    /// `XBUTTON2` — "forward".
    pub const XBUTTON2: u32 = 0x0002;

    /// `WHEEL_DELTA` — one detent of a notched wheel.
    ///
    /// A high-resolution wheel reports fractions of it, which is why the
    /// division that produces
    /// [`ScrollDelta::Lines`](crcbl_core::input::ScrollDelta::Lines) is done in
    /// floating point rather than as an integer count of detents.
    ///
    /// `i16` because that is the width the message carries it in, and because
    /// it converts to `f32` losslessly — a widening cast on the divisor is the
    /// kind of thing that goes unnoticed until a lint changes.
    pub const WHEEL_DELTA: i16 = 120;

    /// `HID_USAGE_PAGE_GENERIC`.
    pub const HID_USAGE_PAGE_GENERIC: u16 = 0x01;
    /// `HID_USAGE_GENERIC_MOUSE`.
    pub const HID_USAGE_GENERIC_MOUSE: u16 = 0x02;
    /// `RID_INPUT` — ask `GetRawInputData` for the report rather than its
    /// header.
    pub const RID_INPUT: u32 = 0x1000_0003;
    /// `RIM_TYPEMOUSE`.
    pub const RIM_TYPE_MOUSE: u32 = 0;
    /// `MOUSE_MOVE_ABSOLUTE` — the report carries a *position*, not a delta.
    ///
    /// Set by a remote-desktop session, a tablet and some virtual machines. A
    /// backend that assumes relative motion reads a screen coordinate as a
    /// delta, which sends the camera to the far wall on the first event.
    pub const MOUSE_MOVE_ABSOLUTE: u16 = 0x0001;
    /// `MOUSE_VIRTUAL_DESKTOP` — an absolute report normalized over the whole
    /// virtual desktop rather than over the primary monitor.
    pub const MOUSE_VIRTUAL_DESKTOP: u16 = 0x0002;
    /// The range an absolute [`RawMouse`] coordinate is normalized to.
    pub const ABSOLUTE_RANGE: i32 = 65_535;

    /// `SM_CXSCREEN`.
    pub const SM_CX_SCREEN: i32 = 0;
    /// `SM_CYSCREEN`.
    pub const SM_CY_SCREEN: i32 = 1;
    /// `SM_CXVIRTUALSCREEN`.
    pub const SM_CX_VIRTUAL_SCREEN: i32 = 78;
    /// `SM_CYVIRTUALSCREEN`.
    pub const SM_CY_VIRTUAL_SCREEN: i32 = 79;

    /// `MAPVK_VK_TO_CHAR` — the unshifted character a virtual key produces in
    /// the current layout, with no dead-key side effect.
    pub const MAPVK_VK_TO_CHAR: u32 = 2;

    /// `VK_SHIFT`-adjacent virtual keys, left and right told apart.
    ///
    /// The unsided `VK_SHIFT`/`VK_CONTROL`/`VK_MENU` are deliberately not here:
    /// the AltGr disambiguation in [`keys::modifiers`](super::keys::modifiers)
    /// depends on seeing *which* Control is down.
    pub const VK_L_SHIFT: usize = 0xA0;
    /// See [`VK_L_SHIFT`].
    pub const VK_R_SHIFT: usize = 0xA1;
    /// See [`VK_L_SHIFT`].
    pub const VK_L_CONTROL: usize = 0xA2;
    /// See [`VK_L_SHIFT`].
    pub const VK_R_CONTROL: usize = 0xA3;
    /// See [`VK_L_SHIFT`]. `VK_MENU` is Windows' name for Alt.
    pub const VK_L_MENU: usize = 0xA4;
    /// See [`VK_L_SHIFT`].
    pub const VK_R_MENU: usize = 0xA5;
    /// `VK_LWIN`.
    pub const VK_L_WIN: usize = 0x5B;
    /// `VK_RWIN`.
    pub const VK_R_WIN: usize = 0x5C;
    /// `VK_CAPITAL` — Caps Lock, read for its *toggle* bit.
    pub const VK_CAPITAL: usize = 0x14;
    /// `VK_NUMLOCK`, likewise.
    pub const VK_NUM_LOCK: usize = 0x90;

    /// The `WMSZ_*` edge a `WM_SIZING` is being dragged by.
    pub const WMSZ_LEFT: usize = 1;
    /// See [`WMSZ_LEFT`].
    pub const WMSZ_RIGHT: usize = 2;
    /// See [`WMSZ_LEFT`].
    pub const WMSZ_TOP: usize = 3;
    /// See [`WMSZ_LEFT`].
    pub const WMSZ_TOP_LEFT: usize = 4;
    /// See [`WMSZ_LEFT`].
    pub const WMSZ_TOP_RIGHT: usize = 5;
    /// See [`WMSZ_LEFT`].
    pub const WMSZ_BOTTOM: usize = 6;
    /// See [`WMSZ_LEFT`].
    pub const WMSZ_BOTTOM_LEFT: usize = 7;
    /// See [`WMSZ_LEFT`].
    pub const WMSZ_BOTTOM_RIGHT: usize = 8;
}

// ---------------------------------------------------------------------------
// The imports
// ---------------------------------------------------------------------------

// The `user32` surface: window classes, windows, messages, monitors and DPI.
#[cfg(target_os = "windows")]
#[link(name = "user32")]
unsafe extern "system" {
    pub fn RegisterClassExW(class: *const WndClassExW) -> u16;
    pub fn CreateWindowExW(
        ex_style: u32,
        class_name: *const u16,
        window_name: *const u16,
        style: u32,
        x: i32,
        y: i32,
        width: i32,
        height: i32,
        parent: Handle,
        menu: Handle,
        instance: Handle,
        param: *mut c_void,
    ) -> Handle;
    pub fn DestroyWindow(hwnd: Handle) -> Bool32;
    pub fn DefWindowProcW(hwnd: Handle, message: u32, w_param: Wparam, l_param: Lparam) -> Lresult;
    pub fn GetWindowLongPtrW(hwnd: Handle, index: i32) -> isize;
    pub fn SetWindowLongPtrW(hwnd: Handle, index: i32, value: isize) -> isize;
    pub fn ShowWindow(hwnd: Handle, command: i32) -> Bool32;
    pub fn IsWindowVisible(hwnd: Handle) -> Bool32;
    pub fn IsIconic(hwnd: Handle) -> Bool32;
    pub fn SetWindowTextW(hwnd: Handle, text: *const u16) -> Bool32;
    pub fn SetWindowPos(
        hwnd: Handle,
        insert_after: Handle,
        x: i32,
        y: i32,
        width: i32,
        height: i32,
        flags: u32,
    ) -> Bool32;
    pub fn GetWindowPlacement(hwnd: Handle, placement: *mut WindowPlacement) -> Bool32;
    pub fn SetWindowPlacement(hwnd: Handle, placement: *const WindowPlacement) -> Bool32;
    pub fn GetClientRect(hwnd: Handle, rect: *mut Rect) -> Bool32;
    pub fn AdjustWindowRectExForDpi(
        rect: *mut Rect,
        style: u32,
        menu: Bool32,
        ex_style: u32,
        dpi: u32,
    ) -> Bool32;
    pub fn PeekMessageW(
        message: *mut Msg,
        hwnd: Handle,
        filter_min: u32,
        filter_max: u32,
        remove: u32,
    ) -> Bool32;
    pub fn DispatchMessageW(message: *const Msg) -> Lresult;
    // Test-only: the two synchronous messages this backend answers —
    // `WM_GETMINMAXINFO` and `WM_SIZING` — are otherwise only sent by a user
    // dragging a window edge, which CI cannot do. Sending them by hand is what
    // makes the constraint path testable at all.
    #[cfg(test)]
    pub fn SendMessageW(hwnd: Handle, message: u32, w_param: Wparam, l_param: Lparam) -> Lresult;
    pub fn MsgWaitForMultipleObjectsEx(
        count: u32,
        handles: *const Handle,
        milliseconds: u32,
        wake_mask: u32,
        flags: u32,
    ) -> u32;
    // Test-only, like `SendMessageW` above and for a related reason: outside a
    // test nothing knows what the queue *should* hold, so a `QS_*` reading
    // answers no question the backend can act on. Inside one it is how a wait
    // that woke when the queue was known to be empty explains itself. The high
    // word is "there is a message now", the low word "there has been one since
    // you last looked" — the distinction `MWMO_INPUTAVAILABLE` turns on.
    #[cfg(test)]
    pub fn GetQueueStatus(flags: u32) -> u32;
    pub fn MonitorFromWindow(hwnd: Handle, flags: u32) -> Handle;
    pub fn EnumDisplayMonitors(
        hdc: Handle,
        clip: *const Rect,
        callback: MonitorEnumProc,
        data: Lparam,
    ) -> Bool32;
    pub fn GetMonitorInfoW(monitor: Handle, info: *mut MonitorInfoExW) -> Bool32;
    pub fn EnumDisplaySettingsW(device: *const u16, mode: u32, dev_mode: *mut DevModeW) -> Bool32;
    pub fn GetDpiForWindow(hwnd: Handle) -> u32;
    pub fn SetProcessDpiAwarenessContext(context: isize) -> Bool32;
    pub fn LoadCursorW(instance: Handle, name: usize) -> Handle;
    // The time of the message currently being processed, on the same
    // `GetTickCount` clock as `Msg::time`. A window procedure is not handed its
    // message's `MSG`, so this is the only way to stamp an event from inside
    // one — see `TimeBase`.
    pub fn GetMessageTime() -> i32;
    pub fn TrackMouseEvent(track: *mut TrackMouse) -> Bool32;
    pub fn SetCapture(hwnd: Handle) -> Handle;
    pub fn ReleaseCapture() -> Bool32;
    // A null rectangle *releases* the clip, which is the call that must run
    // when a window loses focus. See `input` for why that is not optional.
    pub fn ClipCursor(rect: *const Rect) -> Bool32;
    // Test-only: the clip is otherwise invisible from inside the process that
    // set it, and "the pointer is confined" is a claim about the *system's*
    // state rather than about a field this backend keeps. Reading it back is
    // what makes the confinement tests able to fail.
    #[cfg(test)]
    pub fn GetClipCursor(rect: *mut Rect) -> Bool32;
    // Test-only, and for one reason: `ClipCursor` and `SetCursorPos` are
    // restricted to the process that owns the foreground window, and CI has
    // nobody to click on ours. This is the click.
    #[cfg(test)]
    pub fn SetForegroundWindow(hwnd: Handle) -> Bool32;
    pub fn ClientToScreen(hwnd: Handle, point: *mut Point) -> Bool32;
    pub fn ScreenToClient(hwnd: Handle, point: *mut Point) -> Bool32;
    pub fn SetCursorPos(x: i32, y: i32) -> Bool32;
    pub fn GetCursorPos(point: *mut Point) -> Bool32;
    pub fn SetCursor(cursor: Handle) -> Handle;
    // Returns the new display count; the cursor is drawn while it is >= 0,
    // which is why the count has to be balanced rather than merely decremented.
    pub fn ShowCursor(show: Bool32) -> i32;
    pub fn GetKeyboardState(state: *mut u8) -> Bool32;
    pub fn MapVirtualKeyW(code: u32, map_type: u32) -> u32;
    pub fn RegisterRawInputDevices(devices: *const RawInputDevice, count: u32, size: u32)
    -> Bool32;
    pub fn GetRawInputData(
        raw_input: Handle,
        command: u32,
        data: *mut c_void,
        size: *mut u32,
        header_size: u32,
    ) -> u32;
    pub fn GetSystemMetrics(index: i32) -> i32;
}

// The `gdi32` surface: one call, for the class background brush.
#[cfg(target_os = "windows")]
#[link(name = "gdi32")]
unsafe extern "system" {
    pub fn GetStockObject(object: i32) -> Handle;
}

// The `shcore` surface: per-monitor DPI. `GetDpiForMonitor` lives here rather
// than in `user32` because it arrived with Windows 8.1's shell rather than with
// 1607's window manager. It is the per-*monitor* counterpart of
// `GetDpiForWindow`, and it is needed because a `MonitorInfo` has a scale
// factor even when no window of ours is on that monitor.
#[cfg(target_os = "windows")]
#[link(name = "shcore")]
unsafe extern "system" {
    pub fn GetDpiForMonitor(
        monitor: Handle,
        dpi_type: i32,
        dpi_x: *mut u32,
        dpi_y: *mut u32,
    ) -> i32;
}

// The `kernel32` surface.
#[cfg(target_os = "windows")]
#[link(name = "kernel32")]
unsafe extern "system" {
    pub fn GetModuleHandleW(name: *const u16) -> Handle;
    pub fn GetLastError() -> u32;
    // Milliseconds since the system started, 64-bit: the clock `MSG::time` is
    // on. See `TimeBase` for why that makes Win32's timestamp rebasing the
    // easiest of the three native backends' and still not free.
    pub fn GetTickCount64() -> u64;
}

// ---------------------------------------------------------------------------
// Small helpers over the imports
// ---------------------------------------------------------------------------

/// A Rust string as a NUL-terminated UTF-16 buffer, or `None` if it contains an
/// interior NUL.
///
/// Every `*W` entry point takes a NUL-terminated string, so an interior NUL
/// silently truncates rather than failing — a window titled `"save\0nothing"`
/// would appear as `"save"` in the taskbar with no error anywhere. The two
/// Linux backends reject the same input for the same reason, so the seam gives
/// one answer on every platform.
pub fn wide(text: &str) -> Option<Vec<u16>> {
    if text.as_bytes().contains(&0) {
        return None;
    }
    let mut buffer: Vec<u16> = text.encode_utf16().collect();
    buffer.push(0);
    Some(buffer)
}

/// `CLOCK`-equivalent: system uptime in nanoseconds.
///
/// `GetTickCount64` rather than `QueryPerformanceCounter`, and deliberately:
/// [`Msg::time`] is a `GetTickCount` value, so this is the clock the events
/// this backend has to rebase are already on. Reading a *different*, higher
/// resolution clock and subtracting a tick count from it would introduce an
/// error of up to one tick period (about 15.6 ms by default) into every
/// timestamp, in exchange for precision the source data does not have.
#[cfg(target_os = "windows")]
pub fn tick_nanos() -> u64 {
    // SAFETY: `GetTickCount64` takes no arguments, reads no memory and cannot
    // fail; it is available on every Windows this backend supports.
    unsafe { GetTickCount64() }.saturating_mul(1_000_000)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The layouts, checked against the sizes `windows.h` produces on x64.
    ///
    /// These run on the host as well as on Windows, because every field is a
    /// fixed-width type or a pointer and 64-bit Linux lays them out
    /// identically — which is the point: a padding mistake in [`DevModeW`]
    /// would read the refresh rate from the wrong offset and report a
    /// plausible number, and that is not something a Windows-only test would
    /// have caught any better.
    #[test]
    fn the_structures_match_the_c_layout() {
        assert_eq!(size_of::<Rect>(), 16);
        assert_eq!(size_of::<Point>(), 8);
        assert_eq!(size_of::<Msg>(), 48);
        assert_eq!(size_of::<WndClassExW>(), 80);
        assert_eq!(size_of::<CreateStructW>(), 80);
        assert_eq!(size_of::<MinMaxInfo>(), 40);
        assert_eq!(size_of::<WindowPlacement>(), 44);
        assert_eq!(size_of::<MonitorInfoExW>(), 104);
        // The one with two unions in it, and the one whose fields are read
        // past the padding.
        assert_eq!(size_of::<DevModeW>(), 220);
        assert_eq!(core::mem::offset_of!(DevModeW, dm_display_frequency), 184);
        assert_eq!(core::mem::offset_of!(MonitorInfoExW, sz_device), 40);

        // The input structures. `RAWMOUSE` is the one that matters: its
        // `usFlags` is followed by two bytes of union alignment, and reading
        // `lLastX` from offset 6 instead of 12 would produce a delta made of
        // half a button mask — a plausible number, never a crash.
        assert_eq!(size_of::<TrackMouse>(), 24);
        assert_eq!(size_of::<RawInputDevice>(), 16);
        assert_eq!(size_of::<RawMouse>(), 24);
        assert_eq!(core::mem::offset_of!(RawMouse, ul_buttons), 4);
        assert_eq!(core::mem::offset_of!(RawMouse, l_last_x), 12);
        assert_eq!(core::mem::offset_of!(RawMouse, l_last_y), 16);
        assert_eq!(size_of::<RawInputHeader>(), 24);
        assert_eq!(size_of::<RawInput>(), 48);
        assert_eq!(core::mem::offset_of!(RawInput, mouse), 24);
    }

    #[test]
    fn the_defaults_carry_the_sizes_the_system_validates() {
        // Three structures are self-describing, and a wrong `cbSize` is
        // rejected rather than tolerated — `GetMonitorInfoW` with the plain
        // `MONITORINFO` size silently does not fill in the device name.
        assert_eq!(
            u32::from(DevModeW::default().dm_size),
            size_of::<DevModeW>() as u32
        );
        assert_eq!(
            MonitorInfoExW::default().cb_size,
            size_of::<MonitorInfoExW>() as u32
        );
    }

    #[test]
    fn a_rectangle_is_two_corners_and_never_has_a_negative_size() {
        let rect = Rect {
            left: 100,
            top: 50,
            right: 1380,
            bottom: 770,
        };
        assert_eq!(rect.width(), 1280);
        assert_eq!(rect.height(), 720);
        // Degenerate rather than negative: a window rectangle that has been
        // dragged inside out is zero-sized, and a negative width propagated
        // into a `u32` size would be four billion pixels.
        let inverted = Rect {
            left: 10,
            top: 10,
            right: 0,
            bottom: 0,
        };
        assert_eq!(inverted.width(), 0);
        assert_eq!(inverted.height(), 0);
    }

    #[test]
    fn a_string_with_an_interior_nul_has_no_utf16_form() {
        assert_eq!(
            wide("crcbl"),
            Some(vec![0x0063, 0x0072, 0x0063, 0x0062, 0x006C, 0])
        );
        assert_eq!(wide(""), Some(vec![0]));
        // Non-BMP characters become surrogate pairs, which is what UTF-16 is.
        assert_eq!(wide("🎮").map(|text| text.len()), Some(3));
        assert_eq!(wide("save\0nothing"), None);
    }

    #[test]
    fn the_overlapped_window_style_is_the_sum_of_its_parts() {
        // The literal in `winuser.h` is 0x00CF0000, and spelling it as a union
        // of named bits is only correct if it comes to the same number.
        assert_eq!(style::OVERLAPPED_WINDOW, 0x00CF_0000);
        assert_eq!(style::CAPTION, 0x00C0_0000);
        assert_eq!(value::CS_REDRAW, 0x0003);
    }
}
