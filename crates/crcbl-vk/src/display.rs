//! Which adapter drives the display a window is on, for
//! [`Instance::display_adapter`](crcbl_hal::Instance::display_adapter).
//!
//! # Win32: the monitor's GDI adapter, by LUID
//!
//! Vulkan has no query for it, but it does report each physical device's
//! `deviceLUID`, which on Windows is the same LUID the kernel graphics stack
//! gives the adapter. So the window's monitor is named with
//! `MonitorFromWindow`, its GDI device name (`\\.\DISPLAY1`) read with
//! `GetMonitorInfoW`, and `D3DKMTOpenAdapterFromGdiDisplayName` answers the
//! LUID of the adapter that scans that display out. This crate has no Win32
//! bindings, so the four calls are declared here, as `crcbl-shell` declares
//! its own.
//!
//! `crcbl-dx12` answers the same question through DXGI's `EnumOutputs`; the two
//! are different mechanisms for one fact, which is what lets a test hold one
//! against the other.
//!
//! # Wayland and X11: not built
//!
//! There is no Win32 window off Windows — `create_surface` refuses one without
//! `VK_KHR_win32_surface`, which no loader there offers — so the window
//! systems that exist there answer `None` and the caller ranks by its power
//! preference. `docs/backlog.md` names the mechanism each would need.

#[cfg(target_os = "windows")]
mod win32 {
    use core::ffi::c_void;

    /// `MONITOR_DEFAULTTONEAREST`: the monitor holding most of the window, or
    /// the nearest one to a window on none, so the answer is never null.
    const MONITOR_DEFAULT_TO_NEAREST: u32 = 2;

    /// `CCHDEVICENAME`, the length of a GDI display device name.
    const DEVICE_NAME_LEN: usize = 32;

    /// `RECT`.
    #[repr(C)]
    #[derive(Default)]
    struct Rect {
        left: i32,
        top: i32,
        right: i32,
        bottom: i32,
    }

    /// `MONITORINFOEXW`.
    #[repr(C)]
    #[derive(Default)]
    struct MonitorInfoEx {
        size: u32,
        monitor: Rect,
        work: Rect,
        flags: u32,
        device: [u16; DEVICE_NAME_LEN],
    }

    /// `LUID`.
    #[repr(C)]
    #[derive(Default)]
    struct Luid {
        low_part: u32,
        high_part: i32,
    }

    /// `D3DKMT_OPENADAPTERFROMGDIDISPLAYNAME`.
    #[repr(C)]
    #[derive(Default)]
    struct OpenAdapterFromGdiDisplayName {
        device_name: [u16; DEVICE_NAME_LEN],
        adapter: u32,
        adapter_luid: Luid,
        vid_pn_source_id: u32,
    }

    /// `D3DKMT_CLOSEADAPTER`.
    #[repr(C)]
    struct CloseAdapter {
        adapter: u32,
    }

    #[link(name = "user32")]
    unsafe extern "system" {
        fn MonitorFromWindow(hwnd: *mut c_void, flags: u32) -> *mut c_void;
        fn GetMonitorInfoW(monitor: *mut c_void, info: *mut MonitorInfoEx) -> i32;
    }

    #[link(name = "gdi32")]
    unsafe extern "system" {
        fn D3DKMTOpenAdapterFromGdiDisplayName(open: *mut OpenAdapterFromGdiDisplayName) -> i32;
        fn D3DKMTCloseAdapter(close: *const CloseAdapter) -> i32;
    }

    /// The LUID of the adapter scanning out the monitor `hwnd` is on, packed
    /// as [`AdapterInfo::luid`](crcbl_hal::AdapterInfo::luid) documents, or
    /// `None` when Windows will not say.
    pub(super) fn display_luid(hwnd: usize) -> Option<u64> {
        // SAFETY: `hwnd` is the window the caller promised in
        // `create_surface`'s contract is live. The call takes no pointer of
        // ours and, with this flag, answers a monitor handle.
        let monitor = unsafe {
            MonitorFromWindow(
                core::ptr::without_provenance_mut(hwnd),
                MONITOR_DEFAULT_TO_NEAREST,
            )
        };
        let mut info = MonitorInfoEx {
            size: u32::try_from(size_of::<MonitorInfoEx>()).ok()?,
            ..MonitorInfoEx::default()
        };
        // SAFETY: `monitor` came from `MonitorFromWindow` just now, and `info`
        // is a live local whose `size` says it is the `EX` form, so the call
        // writes within it.
        if unsafe { GetMonitorInfoW(monitor, &raw mut info) } == 0 {
            crcbl_core::log::debug!("crcbl-vk: GetMonitorInfoW refused the window's monitor");
            return None;
        }
        let mut open = OpenAdapterFromGdiDisplayName {
            device_name: info.device,
            ..OpenAdapterFromGdiDisplayName::default()
        };
        // SAFETY: `open` is a live local with the NUL-terminated device name
        // GDI just wrote; the call writes the adapter handle, LUID and source
        // id into it and reads nothing else.
        let status = unsafe { D3DKMTOpenAdapterFromGdiDisplayName(&raw mut open) };
        // An `NTSTATUS`: negative is failure.
        if status < 0 {
            crcbl_core::log::debug!(
                "crcbl-vk: D3DKMTOpenAdapterFromGdiDisplayName failed ({status:#x})"
            );
            return None;
        }
        let close = CloseAdapter {
            adapter: open.adapter,
        };
        // SAFETY: `open.adapter` is the handle the call above opened, closed
        // exactly once here. A failure leaves nothing to release.
        let closed = unsafe { D3DKMTCloseAdapter(&raw const close) };
        if closed < 0 {
            crcbl_core::log::debug!("crcbl-vk: D3DKMTCloseAdapter failed ({closed:#x})");
        }
        let luid = open.adapter_luid;
        Some((u64::from(luid.high_part.cast_unsigned()) << 32) | u64::from(luid.low_part))
    }
}

/// The LUID of the adapter driving the display the window at `hwnd` is on.
///
/// `hwnd` is a Win32 window's address, which only a Win32 surface carries.
#[cfg(target_os = "windows")]
pub(crate) fn display_luid(hwnd: usize) -> Option<u64> {
    win32::display_luid(hwnd)
}

/// No Win32 window exists off Windows; see the module docs.
#[cfg(not(target_os = "windows"))]
pub(crate) const fn display_luid(_hwnd: usize) -> Option<u64> {
    None
}
