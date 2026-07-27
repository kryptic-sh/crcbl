//! Monitor enumeration: geometry, work area, scale and refresh rate.
//!
//! # Why monitor ids are a plain newtype and window ids are generational
//!
//! [`WindowId`](crate::WindowId) is a [`Handle`](crcbl_core::Handle) because a
//! window is an object the engine creates, destroys and then keeps a stale
//! reference to — the exact failure a generation counter catches. A monitor is
//! not created by anyone; it is *observed*. The backend enumerates whatever the
//! server advertises, and the only stale-id hazard is a monitor unplugged
//! between [`Shell::monitors`](crate::Shell::monitors) and the call that used
//! its id.
//!
//! That hazard is handled by an obligation instead of a generation: **a
//! [`MonitorId`] is never reused within a shell session.** A backend that
//! recycles ids (RandR output ids and Wayland registry names both can, after a
//! hotplug cycle) must map them through its own table. The failure mode is then
//! a clean [`ShellError::NoSuchMonitor`](crate::ShellError::NoSuchMonitor)
//! rather than a borderless window appearing on the wrong display.
//!
//! # Refresh rate is millihertz
//!
//! Because 59.94 Hz exists, is extremely common, and is not 60. An integer
//! millihertz value is exact for every mode any of these APIs report
//! (`wl_output.mode` is already millihertz; RandR gives dot clock and totals;
//! Win32 gives integer Hz, and macOS a `Double`), and it avoids putting a float
//! in a `PartialEq`-compared struct that a settings screen will diff.

use core::fmt;

use crate::PhysicalRect;

/// Identifier of a connected monitor.
///
/// Stable for as long as the monitor stays connected, never reused within a
/// session. See the [module docs](self).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct MonitorId(pub u32);

impl fmt::Display for MonitorId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "monitor {}", self.0)
    }
}

/// What the shell knows about one monitor.
///
/// Owned rather than borrowed: enumeration happens at startup and on hotplug,
/// so the `String` is not worth avoiding, and borrowing from the shell would
/// infect every consumer with a lifetime — the same argument
/// `crcbl-hal`'s `AdapterInfo` makes.
///
/// Not `Eq`: [`scale_factor`](Self::scale_factor) is an `f64`. A settings
/// screen diffing two of these gets `PartialEq`, which is the correct relation
/// for a struct holding a float.
#[derive(Clone, Debug, PartialEq)]
pub struct MonitorInfo {
    /// Stable id for this monitor within the session.
    pub id: MonitorId,

    /// Human-readable name, e.g. `"DP-2"` or `"Dell U2723QE"`.
    ///
    /// For settings UI and logs only. Never key behaviour off it: it is not
    /// unique (two identical monitors report identical names on some drivers)
    /// and it is not stable across drivers.
    pub name: String,

    /// Position and size in the desktop's device-pixel coordinate space.
    ///
    /// The origin is wherever the window system puts it, and coordinates are
    /// signed because a monitor placed left of or above the primary one has
    /// negative ones.
    ///
    /// On Wayland this comes from `xdg_output` logical geometry scaled back to
    /// pixels by *that output's own* scale; a compositor without `xdg_output`
    /// falls back to `wl_output.geometry`, whose position is logical while its
    /// size is physical, so the two only agree at scale 1.
    ///
    /// **There is a limit to how meaningful this can be**, and it is worth
    /// knowing before building a layout on it: on a desktop whose outputs run
    /// at *different* scales there is no single device-pixel coordinate space
    /// at all, and these rectangles will not tile — a 1× monitor beside a 2×
    /// one has two device-pixel spaces, and only the compositor's logical space
    /// is globally coherent. This type cannot express that, so it reports each
    /// monitor in its own pixels. Use it to name a monitor and to tell a
    /// settings screen which one is on the left; do not use it as a desktop
    /// layout. [`ShellCaps::WINDOW_POSITION`](crate::ShellCaps::WINDOW_POSITION)
    /// is the bit that says a *window* can be placed, and Wayland never sets
    /// it.
    pub bounds: PhysicalRect,

    /// The part of [`bounds`](Self::bounds) not covered by panels, docks or
    /// taskbars.
    ///
    /// What a *windowed* window should be placed within. Equal to `bounds` when
    /// the backend cannot find out — Wayland has no protocol for it, so the
    /// Wayland backend will report the full area and nothing may assume
    /// otherwise.
    pub work_area: PhysicalRect,

    /// Device pixels per logical unit, e.g. `1.0`, `1.5`, `2.0`.
    ///
    /// Fractional values only appear when
    /// [`ShellCaps::FRACTIONAL_SCALE`](crate::ShellCaps::FRACTIONAL_SCALE) is
    /// set. Note that a *window's* scale factor is not necessarily any
    /// monitor's: a window straddling two outputs gets one of them, chosen by
    /// the compositor, and it can change while the window is dragged — which is
    /// what [`ShellEvent::ScaleFactorChanged`](crate::ShellEvent::ScaleFactorChanged)
    /// is for.
    pub scale_factor: f64,

    /// Refresh rate in millihertz — `60_000` for 60 Hz, `59_940` for 59.94 Hz.
    ///
    /// Zero when the backend cannot determine it (a virtual output, a browser
    /// canvas). See the [module docs](self) for the unit choice.
    pub refresh_millihertz: u32,

    /// Whether the window system calls this the primary monitor.
    ///
    /// Wayland has no such concept, so a Wayland backend marks the first output
    /// it sees. Use it as a default, never as a guarantee.
    pub is_primary: bool,
}

impl MonitorInfo {
    /// Refresh rate in hertz, for display and frame pacing.
    #[inline]
    #[must_use]
    pub fn refresh_hz(&self) -> f64 {
        f64::from(self.refresh_millihertz) / 1000.0
    }

    /// The frame period implied by [`refresh_hz`](Self::refresh_hz), or `None`
    /// if the rate is unknown.
    #[must_use]
    pub fn frame_period(&self) -> Option<core::time::Duration> {
        if self.refresh_millihertz == 0 {
            return None;
        }
        Some(core::time::Duration::from_secs_f64(
            1000.0 / f64::from(self.refresh_millihertz),
        ))
    }

    /// The monitor's resolution in device pixels.
    #[inline]
    #[must_use]
    pub const fn size(&self) -> crate::PhysicalSize {
        self.bounds.size()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::PhysicalSize;

    fn monitor() -> MonitorInfo {
        MonitorInfo {
            id: MonitorId(1),
            name: "DP-2".to_string(),
            bounds: PhysicalRect::new(0, 0, 3840, 2160),
            work_area: PhysicalRect::new(0, 32, 3840, 2128),
            scale_factor: 1.5,
            refresh_millihertz: 143_998,
            is_primary: true,
        }
    }

    #[test]
    fn refresh_rates_that_are_not_integers_survive() {
        let mut info = monitor();
        info.refresh_millihertz = 59_940;
        assert!((info.refresh_hz() - 59.94).abs() < 1e-9);
        let period = info.frame_period().expect("known rate");
        assert!((period.as_secs_f64() - 1.0 / 59.94).abs() < 1e-9);

        // The awkward one that motivated millihertz in the first place.
        assert!((monitor().refresh_hz() - 143.998).abs() < 1e-9);
    }

    #[test]
    fn an_unknown_refresh_rate_is_absent_not_zero_hz() {
        let mut info = monitor();
        info.refresh_millihertz = 0;
        assert_eq!(info.frame_period(), None);
        assert_eq!(info.refresh_hz(), 0.0);
    }

    #[test]
    fn geometry_accessors_agree_with_the_bounds() {
        let info = monitor();
        assert_eq!(info.size(), PhysicalSize::new(3840, 2160));
        assert_eq!(info.work_area.size(), PhysicalSize::new(3840, 2128));
        assert_eq!(info.id.to_string(), "monitor 1");
        assert_eq!(
            monitor().frame_period().map(|period| period.as_millis()),
            Some(6)
        );
    }
}
