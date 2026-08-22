//! Monitor enumeration: `EnumDisplayMonitors`, per-monitor DPI, and the
//! refresh rate Windows will not tell the truth about.
//!
//! # One virtual screen, like X11 and unlike Wayland
//!
//! Every monitor is a rectangle in one signed **virtual screen** coordinate
//! space whose origin is the primary monitor's top-left, and a window's
//! position is in the same space. So [`MonitorInfo::bounds`] tiles here, which
//! that type's own documentation says is *not* true on a Wayland desktop at
//! mixed scales — and that is what makes
//! [`DisplayMode::Borderless`](crate::DisplayMode::Borderless) with a named
//! monitor implementable, and why
//! [`ShellCaps::WINDOW_POSITION`](crate::ShellCaps::WINDOW_POSITION) is set.
//!
//! Per-monitor-v2 DPI awareness is load-bearing for that claim. Without it the
//! system lies to the process about both the coordinates and the sizes,
//! scaling them to a pretend 96-DPI desktop, and two monitors at different
//! scales stop tiling. [`Win32Shell::open`](super::Win32Shell) asks for it
//! before anything else for exactly this reason.
//!
//! # Decision: the device name is the identity
//!
//! An `HMONITOR` is a pseudo-handle that the system may reuse after a hotplug,
//! and [`monitor`](crate::monitor) requires that a [`MonitorId`] never be
//! reused within a session. `MONITORINFOEXW::szDevice` — `\\.\DISPLAY1` and
//! friends — is stable while a monitor stays attached and is what
//! `EnumDisplaySettingsW` is keyed by anyway, so it is the identity this
//! backend maps through its own table. The X11 backend does the same thing with
//! RandR CRTC ids and for the same reason.
//!
//! It is also the *name* reported to a settings screen, which needs saying
//! because it is not the marketing name. Getting "Dell U2723QE" needs
//! `EnumDisplayDevicesW` (which usually answers "Generic PnP Monitor") or
//! `QueryDisplayConfig` plus a target-device-name request, and neither is worth
//! a second display API for a string [`MonitorInfo::name`] already documents as
//! unstable and not-for-keying.
//!
//! # Decision: refresh comes from `QueryDisplayConfig` first, exact when the
//! path walk can answer
//!
//! [`MonitorInfo::refresh_millihertz`] exists because 59.94 Hz is not 60, and
//! `docs/plan/15-windowing.md`'s frame pacing needs the difference.
//! `EnumDisplaySettingsW` cannot express it: `DEVMODEW::dmDisplayFrequency` is
//! an integer count of hertz and reports 60 for a 59.94 Hz mode. So the exact
//! figure comes from `QueryDisplayConfig` first: its `DISPLAYCONFIG_RATIONAL`
//! carries 60000/1001 for that mode, which the integer path rounds away. The
//! whole hertz `EnumDisplaySettingsW` reports is the fallback, used when the
//! path walk cannot answer — session 0, a remote session, a driver that
//! refuses, or a path with no mode.

use core::ptr;

use crate::{MonitorId, MonitorInfo};

use super::ffi::{self, Bool32, DevModeW, Handle, Lparam, MonitorInfoExW, Rect, value};
use super::geometry;
use super::shell::Win32Shell;

/// Collects the handles `EnumDisplayMonitors` reports.
///
/// Only the handles: everything else needs three more calls per monitor, and a
/// callback is the one place in this backend that must not do anything it could
/// get wrong — it runs inside the system's enumeration, where a panic would
/// unwind across an FFI boundary.
///
/// # Safety
///
/// Called only by `EnumDisplayMonitors`, with the `data` this module passed it.
unsafe extern "system" fn collect(
    monitor: Handle,
    _hdc: Handle,
    _clip: *mut Rect,
    data: Lparam,
) -> Bool32 {
    // SAFETY: `data` is the `&mut Vec<Handle>` handed to `EnumDisplayMonitors`
    // in `enumerate_monitors`, which owns it for the whole of that call and
    // does not touch it while the enumeration runs.
    let handles = unsafe { &mut *(data as *mut Vec<Handle>) };
    handles.push(monitor);
    // `TRUE`: keep enumerating.
    1
}

/// A monitor's `\\.\DISPLAYn` device name.
///
/// `None` when the handle is stale — which happens for real, between a hotplug
/// and the enumeration that notices it.
pub(super) fn device_name(monitor: Handle) -> Option<String> {
    let mut info = MonitorInfoExW::default();
    // SAFETY: `info` is a live, initialised `MONITORINFOEXW` whose `cb_size` is
    // its own size — which is what selects the `EX` form and therefore whether
    // the device name is filled in at all.
    let ok = unsafe { ffi::GetMonitorInfoW(monitor, &raw mut info) };
    if ok == 0 {
        return None;
    }
    Some(wide_to_string(&info.sz_device))
}

/// A NUL-padded UTF-16 buffer as a `String`.
///
/// Lossy, because a device name that is not valid UTF-16 is still better shown
/// with a replacement character than dropped: it is a display string and an
/// identity key, and both survive one bad code unit.
fn wide_to_string(buffer: &[u16]) -> String {
    let end = buffer
        .iter()
        .position(|unit| *unit == 0)
        .unwrap_or(buffer.len());
    String::from_utf16_lossy(&buffer[..end])
}

impl Win32Shell {
    /// Every monitor the system currently has.
    ///
    /// Called at [`open`](Win32Shell::open) and again on every
    /// `WM_DISPLAYCHANGE`. Ids come from
    /// [`monitor_id_of`](Win32Shell::monitor_id_of)'s table, so a monitor that
    /// stays attached keeps its id across an unrelated hotplug — the obligation
    /// [`monitor`](crate::monitor) states, and the reason a borderless window
    /// does not jump to another display when a second cable is pulled.
    pub(super) fn enumerate_monitors(&mut self) -> Vec<MonitorInfo> {
        let mut handles: Vec<Handle> = Vec::new();
        // SAFETY: a null device context and a null clip rectangle ask for every
        // monitor on the virtual screen; `collect` has the `MONITORENUMPROC`
        // signature; and the `Lparam` is a pointer to `handles`, which is alive
        // and untouched for the whole of this call.
        unsafe {
            ffi::EnumDisplayMonitors(
                ptr::null_mut(),
                ptr::null(),
                collect,
                (&raw mut handles) as Lparam,
            );
        }

        let mut monitors = Vec::with_capacity(handles.len());
        let exact_refreshes = exact_refreshes();
        for handle in handles {
            let mut info = MonitorInfoExW::default();
            // SAFETY: as `device_name` — a live, self-describing structure.
            if unsafe { ffi::GetMonitorInfoW(handle, &raw mut info) } == 0 {
                continue;
            }
            let device = wide_to_string(&info.sz_device);
            let bounds = geometry::desktop_rect(info.rc_monitor);
            monitors.push(MonitorInfo {
                id: self.monitor_id_for(&device),
                scale_factor: geometry::scale_from_dpi(dpi_of(handle)),
                refresh_millihertz: refresh_of(&exact_refreshes, &info.sz_device),
                is_primary: info.dw_flags & value::MONITOR_PRIMARY != 0,
                bounds,
                // Unlike X11's `_NET_WORKAREA`, this is already per monitor and
                // already in the same space as the bounds, so there is nothing
                // to intersect.
                work_area: geometry::desktop_rect(info.rc_work),
                name: device,
            });
        }

        // The primary flag is a hint consumers default to, so exactly one
        // monitor must carry it. Windows always marks one — but a session with
        // a monitor that vanished mid-enumeration can leave none.
        if !monitors.iter().any(|monitor| monitor.is_primary)
            && let Some(first) = monitors.first_mut()
        {
            first.is_primary = true;
        }
        monitors
    }

    /// The [`MonitorId`] for a device name, allocating one the first time.
    ///
    /// Never reuses an id within the session; see the [module docs](self).
    fn monitor_id_for(&mut self, device: &str) -> MonitorId {
        if let Some(id) = self.monitor_id_of(device) {
            return id;
        }
        let id = MonitorId(self.next_monitor_id);
        self.next_monitor_id += 1;
        self.monitor_ids.push((device.to_string(), id));
        id
    }
}

/// A monitor's effective DPI, or the 100% default.
///
/// `MDT_EFFECTIVE_DPI` is the scale the *user* chose for this display, which is
/// what every other window on it is laid out at. The other two types the API
/// offers — angular and raw — describe the panel rather than the preference,
/// and using them is the Windows equivalent of deriving a scale from RandR's
/// physical millimetres, which the X11 backend refuses for the same reason.
fn dpi_of(monitor: Handle) -> u32 {
    let mut dpi_x = 0u32;
    let mut dpi_y = 0u32;
    // SAFETY: `monitor` is a handle the enumeration just produced, and both
    // outputs are live `u32`s the call writes through. The `HRESULT` is checked
    // rather than assumed — it fails for a stale handle.
    let result = unsafe {
        ffi::GetDpiForMonitor(
            monitor,
            value::MDT_EFFECTIVE_DPI,
            &raw mut dpi_x,
            &raw mut dpi_y,
        )
    };
    if result < 0 || dpi_x == 0 {
        return value::DEFAULT_DPI;
    }
    // The Y DPI is deliberately ignored: no Windows display has ever reported
    // an anisotropic one, and the seam has a single `scale_factor`.
    dpi_x
}

/// A display's current refresh rate in millihertz, or zero if it will not say.
///
/// Zero is documented by [`MonitorInfo::refresh_millihertz`] as "the backend
/// cannot determine it" rather than as 0 Hz, so a virtual display that reports
/// nothing is honest rather than broken. The exact rate from
/// [`exact_refreshes`] wins; a display the path walk cannot name falls back to
/// `EnumDisplaySettingsW`'s whole hertz.
fn refresh_of(table: &[(String, u32)], device: &[u16; 32]) -> u32 {
    let name = wide_to_string(device);
    table
        .iter()
        .find(|(entry, _)| *entry == name)
        .map(|(_, millihertz)| *millihertz)
        .unwrap_or_else(|| integer_refresh_of(device))
}

/// The lowest rate a real display can be running, in millihertz (10 Hz).
///
/// 24 Hz cinema is the lowest common mode, so no real mode is refused; the
/// placeholder rationals a virtual display reports are far below it — the
/// GitHub runner's desktop has reported 1 mHz — and refusing them is what
/// keeps the seam's documented "zero = the backend cannot determine it" for
/// such displays, instead of reporting a rate nothing shows.
const MIN_PLAUSIBLE_REFRESH_MHZ: u32 = 10_000;

/// The exact refresh of every active display, as `(GDI device name, millihertz)`.
///
/// `QueryDisplayConfig`'s `DISPLAYCONFIG_RATIONAL` carries the refresh a
/// `DEVMODEW` cannot: 60000/1001 for a 59.94 Hz mode, which
/// `EnumDisplaySettingsW` reports as the integer 60. Each path is named through
/// `DisplayConfigGetDeviceInfo`'s source-name request so it can be matched to
/// the monitor whose `\\.\DISPLAYn`
/// [`enumerate_monitors`](Win32Shell::enumerate_monitors) already keys on.
/// Empty when the API will not answer at all — a remote session or a driver
/// that refuses — which is what the caller falls back from.
fn exact_refreshes() -> Vec<(String, u32)> {
    let mut path_count = 0u32;
    let mut mode_count = 0u32;
    // SAFETY: two live `u32`s the call writes through.
    let status = unsafe {
        ffi::GetDisplayConfigBufferSizes(
            value::QDC_ONLY_ACTIVE_PATHS,
            &raw mut path_count,
            &raw mut mode_count,
        )
    };
    if status != value::ERROR_SUCCESS || path_count == 0 {
        crcbl_core::log::warn!(
            "win32: GetDisplayConfigBufferSizes failed ({status}); monitor refresh falls back to whole hertz"
        );
        return Vec::new();
    }
    let mut paths = vec![ffi::DisplayConfigPathInfo::default(); path_count as usize];
    let mut modes = vec![ffi::DisplayConfigModeInfo::default(); mode_count as usize];
    // SAFETY: both vectors are sized from the call above, the pointers are to
    // their live buffers, and the counts may only shrink.
    let status = unsafe {
        ffi::QueryDisplayConfig(
            value::QDC_ONLY_ACTIVE_PATHS,
            &raw mut path_count,
            paths.as_mut_ptr(),
            &raw mut mode_count,
            modes.as_mut_ptr(),
            core::ptr::null_mut(),
        )
    };
    if status != value::ERROR_SUCCESS {
        crcbl_core::log::warn!(
            "win32: QueryDisplayConfig failed ({status}); monitor refresh falls back to whole hertz"
        );
        return Vec::new();
    }
    paths.truncate(path_count as usize);
    modes.truncate(mode_count as usize);

    let mut refreshes = Vec::new();
    for path in &paths {
        let Some(mode_index) = path.target_info.target_mode_index() else {
            continue;
        };
        let Some(mode) = modes.get(mode_index) else {
            continue;
        };
        if mode.info_type != value::DISPLAYCONFIG_MODE_INFO_TYPE_TARGET {
            continue;
        }
        let signal = mode.target_mode.target_video_signal_info;
        let divider = (signal.additional_signal_info >> 16) & 0x3F;
        let Some(millihertz) = signal.v_sync_freq.millihertz(divider) else {
            continue;
        };
        // A virtual display's placeholder signal is not a rate; refuse it so
        // `refresh_of` falls back to the integer path, which answers 0 for
        // such a display (frequency 0 or 1 = "hardware default").
        if millihertz < MIN_PLAUSIBLE_REFRESH_MHZ {
            continue;
        }
        let mut request = ffi::DisplayConfigSourceDeviceName {
            header: ffi::DisplayConfigDeviceInfoHeader {
                kind: value::DISPLAYCONFIG_DEVICE_INFO_GET_SOURCE_NAME,
                size: core::mem::size_of::<ffi::DisplayConfigSourceDeviceName>() as u32,
                adapter_id: path.source_info.adapter_id,
                id: path.source_info.id,
            },
            ..ffi::DisplayConfigSourceDeviceName::default()
        };
        // SAFETY: `request` is a live, correctly sized structure; the system
        // fills `view_gdi_device_name` and reads nothing else.
        let status = unsafe { ffi::DisplayConfigGetDeviceInfo(&raw mut request.header) };
        if status != value::ERROR_SUCCESS {
            continue;
        }
        let name = wide_to_string(&request.view_gdi_device_name);
        if name.is_empty() {
            continue;
        }
        crcbl_core::log::info!("win32: exact refresh for {name}: {millihertz} mHz");
        refreshes.push((name, millihertz));
    }
    refreshes
}

/// The whole-hertz fallback: `EnumDisplaySettingsW`'s integer refresh, which
/// is what this module reported before `QueryDisplayConfig` was asked first.
fn integer_refresh_of(device: &[u16; 32]) -> u32 {
    let mut mode = DevModeW::default();
    // SAFETY: `device` is the NUL-padded name out of `MONITORINFOEXW`, which
    // is exactly what this call keys on; `mode` is a live, initialised
    // `DEVMODEW` whose `dm_size` tells the driver which generation of the
    // structure it may write into.
    let ok = unsafe {
        ffi::EnumDisplaySettingsW(device.as_ptr(), value::ENUM_CURRENT_SETTINGS, &raw mut mode)
    };
    if ok == 0 {
        return 0;
    }
    // 0 and 1 are both documented as meaning "the hardware default", which is
    // not a rate.
    if mode.dm_display_frequency <= 1 {
        return 0;
    }
    mode.dm_display_frequency.saturating_mul(1_000)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_device_name_stops_at_the_first_nul() {
        // `szDevice` is a fixed 32-unit buffer, so everything after the
        // terminator is whatever the driver left there. Keeping it would make
        // the identity key differ between two reads of the same monitor.
        let mut buffer = [0u16; 32];
        for (slot, unit) in buffer.iter_mut().zip(r"\\.\DISPLAY1".encode_utf16()) {
            *slot = unit;
        }
        buffer[20] = u16::from(b'X');
        assert_eq!(wide_to_string(&buffer), r"\\.\DISPLAY1");

        // A buffer with no terminator at all is still a name.
        let full = [u16::from(b'A'); 32];
        assert_eq!(wide_to_string(&full).len(), 32);
        assert_eq!(wide_to_string(&[0u16; 32]), "");
    }
}
