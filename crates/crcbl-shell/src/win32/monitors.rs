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
//! # Decision: refresh comes from `EnumDisplaySettingsW`, and is a whole hertz
//!
//! [`MonitorInfo::refresh_millihertz`] exists because 59.94 Hz is not 60, and
//! `docs/plan/15-windowing.md`'s frame pacing needs the difference. **Windows
//! is the one platform of the five that cannot express it here**:
//! `DEVMODEW::dmDisplayFrequency` is an integer count of hertz and reports 60
//! for a 59.94 Hz mode. So this backend reports 60_000 millihertz for such a
//! display — precise-looking and wrong in the third decimal.
//!
//! The exact figure exists, in `QueryDisplayConfig`'s
//! `DISPLAYCONFIG_RATIONAL` (a numerator over a denominator, 60000/1001 for the
//! mode in question). It is not implemented here: it is a second display API
//! with its own path-and-mode array walk, and this slice is the window
//! lifecycle. `docs/backlog.md` carries it with that reasoning, rather than
//! this module quietly reporting a rounded number as if it were exact.

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
                refresh_millihertz: refresh_of(&info.sz_device),
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
/// nothing is honest rather than broken. See the [module docs](self) for why
/// the non-zero answer is a whole hertz.
fn refresh_of(device: &[u16; 32]) -> u32 {
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
