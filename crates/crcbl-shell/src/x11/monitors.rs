//! Monitor enumeration through RandR, and the fallback when there is none.
//!
//! # Decision: enumerate CRTCs, not RandR 1.5 monitors
//!
//! RandR 1.5's `GetMonitors` is the newer and tidier request: it hands back a
//! list with names, geometry and a primary flag already assembled, and it
//! understands a monitor that spans two outputs. It is not what this backend
//! uses, for one concrete reason — **it does not carry a refresh rate.**
//!
//! [`MonitorInfo::refresh_millihertz`] exists because frame pacing needs it and
//! because 59.94 Hz is not 60, and RandR only ever states a rate as
//! `dot_clock / (htotal × vtotal)` on a *mode*, which is reached through a
//! CRTC. So the choice was `GetMonitors` plus a second CRTC walk to fill in the
//! rate, or the CRTC walk alone — and the CRTC walk alone also works on RandR
//! 1.2, which is a decade more of servers, including the `Xvfb` the end-to-end
//! suite runs against.
//!
//! What it costs is the 1.5 feature this gives up: two outputs configured as
//! one logical monitor appear as two here. That is rare, it is visible in a
//! settings screen rather than broken, and it is stated rather than hidden.
//!
//! # One coordinate space, unlike Wayland
//!
//! A CRTC's `x`/`y` are in the **screen's** pixels, and every window's position
//! is in the same space. So [`MonitorInfo::bounds`] tiles here — the thing that
//! type's own documentation says is *not* true on a Wayland desktop at mixed
//! scales, where each output has its own device-pixel space and only the
//! compositor's logical space is globally coherent.
//!
//! That is the fact behind [`ShellCaps::WINDOW_POSITION`](crate::ShellCaps) on
//! this backend, and behind
//! [`DisplayMode::Borderless`](crate::DisplayMode::Borderless) with a named
//! monitor working at all: the backend can move a window onto a chosen
//! rectangle because it and the rectangle are numbers in the same space.

use super::{Conn, MonitorId, MonitorInfo, PhysicalRect, X11Shell, ffi};

/// One CRTC's geometry, before it is turned into a [`MonitorInfo`].
struct Crtc {
    id: u32,
    x: i16,
    y: i16,
    width: u16,
    height: u16,
    mode: u32,
    output: Option<u32>,
}

impl X11Shell {
    /// Every monitor the server currently has.
    ///
    /// Called at [`open`](X11Shell::open) and again whenever RandR says the
    /// layout changed. [`MonitorId`]s are assigned through
    /// [`monitor_id_for`](Self::monitor_id_for), so a monitor that stays
    /// connected keeps its id across a hotplug of some *other* monitor — the
    /// obligation [`monitor`](crate::monitor) states, and the reason a
    /// borderless window does not jump to a different display when an
    /// unrelated cable is pulled.
    pub(super) fn enumerate_monitors(&mut self) -> Vec<MonitorInfo> {
        let scale = self.scale_factor;
        let work_area = self.conn.read_work_area();
        let Some(crtcs) = self.conn.read_crtcs() else {
            return vec![self.fallback_monitor(scale, work_area)];
        };
        if crtcs.is_empty() {
            return vec![self.fallback_monitor(scale, work_area)];
        }
        let primary_output = self.conn.read_primary_output();

        let mut monitors = Vec::with_capacity(crtcs.len());
        for crtc in crtcs {
            let bounds = PhysicalRect::new(
                i32::from(crtc.x),
                i32::from(crtc.y),
                u32::from(crtc.width),
                u32::from(crtc.height),
            );
            let (name, is_primary) = crtc.output.map_or_else(
                || (format!("CRTC-{}", crtc.id), false),
                |output| {
                    (
                        self.conn.read_output_name(output),
                        primary_output == Some(output),
                    )
                },
            );
            monitors.push(MonitorInfo {
                id: self.monitor_id_for(crtc.id),
                name,
                bounds,
                work_area: work_area.map_or(bounds, |area| intersect(bounds, area)),
                scale_factor: scale,
                refresh_millihertz: self.conn.refresh_of(crtc.mode),
                is_primary,
            });
        }
        // The primary flag is a hint a consumer defaults to, so exactly one
        // monitor must carry it even on a server that never set one.
        if !monitors.iter().any(|monitor| monitor.is_primary)
            && let Some(first) = monitors.first_mut()
        {
            first.is_primary = true;
        }
        monitors
    }

    /// The single monitor a server with no usable RandR has.
    ///
    /// Not a stub: a server without RandR genuinely has one screen of a known
    /// size, and reporting that is the correct answer. What is missing is the
    /// refresh rate, and [`MonitorInfo::refresh_millihertz`] already defines
    /// zero as "the backend cannot determine it" rather than as 0 Hz.
    fn fallback_monitor(&mut self, scale: f64, work_area: Option<PhysicalRect>) -> MonitorInfo {
        let bounds = PhysicalRect::new(
            0,
            0,
            self.conn.screen_size.width,
            self.conn.screen_size.height,
        );
        MonitorInfo {
            // CRTC ids start at 1 on every server; `0` is `XCB_NONE` and can
            // never collide with a real one, so the fallback monitor keeps a
            // stable id across a RandR that appears mid-session.
            id: self.monitor_id_for(0),
            name: "screen".to_string(),
            bounds,
            work_area: work_area.map_or(bounds, |area| intersect(bounds, area)),
            scale_factor: scale,
            refresh_millihertz: 0,
            is_primary: true,
        }
    }

    /// The [`MonitorId`] for a CRTC, allocating one the first time.
    ///
    /// Never reuses an id within the session, which is exactly the obligation
    /// [`monitor`](crate::monitor) puts on a backend whose native ids *can* be
    /// recycled — and RandR CRTC ids can, after a hotplug cycle.
    fn monitor_id_for(&mut self, crtc: u32) -> MonitorId {
        if let Some((_, id)) = self.monitor_ids.iter().find(|(known, _)| *known == crtc) {
            return *id;
        }
        let id = MonitorId(self.next_monitor_id);
        self.next_monitor_id += 1;
        self.monitor_ids.push((crtc, id));
        id
    }
}

impl Conn {
    /// Every enabled CRTC, or `None` when RandR is unusable.
    ///
    /// The two-phase shape [`atoms`](super::atoms) argues for: every
    /// `GetCrtcInfo` is fired before any reply is read, so a six-monitor
    /// desktop costs one round trip rather than six.
    fn read_crtcs(&self) -> Option<Vec<Crtc>> {
        let randr = self.ext.randr?;
        self.randr_event_base?;

        // SAFETY: the connection and root window are live; a null error
        // pointer discards the error and a null reply is handled below.
        let resources = unsafe {
            let cookie = (randr.get_screen_resources_current)(self.raw(), self.root);
            (randr.get_screen_resources_current_reply)(self.raw(), cookie, core::ptr::null_mut())
        };
        if resources.is_null() {
            return None;
        }
        // SAFETY: `resources` is a live reply this call owns, and
        // `xcb_randr_get_screen_resources_current_crtcs` returns a pointer to
        // `num_crtcs` ids inside the same allocation.
        let (config_timestamp, ids) = unsafe {
            let count = usize::from((*resources).num_crtcs);
            let crtcs = (randr.resources_crtcs)(resources);
            let ids = if count > 0 && !crtcs.is_null() {
                core::slice::from_raw_parts(crtcs, count).to_vec()
            } else {
                Vec::new()
            };
            ((*resources).config_timestamp, ids)
        };
        // SAFETY: freed exactly once.
        unsafe { ffi::free_reply(resources) };

        // Phase one: fire every request.
        let cookies: Vec<(u32, ffi::Cookie)> = ids
            .iter()
            .map(|id| {
                // SAFETY: the connection is live and `id` came from the
                // resources reply, so it names a CRTC on this screen.
                (*id, unsafe {
                    (randr.get_crtc_info)(self.raw(), *id, config_timestamp)
                })
            })
            .collect();

        // Phase two: collect.
        let mut crtcs = Vec::new();
        for (id, cookie) in cookies {
            // SAFETY: the cookie came from `get_crtc_info` on this connection.
            let reply =
                unsafe { (randr.get_crtc_info_reply)(self.raw(), cookie, core::ptr::null_mut()) };
            if reply.is_null() {
                continue;
            }
            // SAFETY: `reply` is a live reply this call owns, and
            // `xcb_randr_get_crtc_info_outputs` points at `num_outputs` ids
            // inside it.
            let crtc = unsafe {
                let info = *reply;
                let outputs = (randr.crtc_outputs)(reply);
                let output = (info.num_outputs > 0 && !outputs.is_null()).then(|| *outputs);
                Crtc {
                    id,
                    x: info.x,
                    y: info.y,
                    width: info.width,
                    height: info.height,
                    mode: info.mode,
                    output,
                }
            };
            // SAFETY: freed exactly once.
            unsafe { ffi::free_reply(reply) };
            // A CRTC with no mode is *disabled*, not a monitor of size zero.
            // Reporting it would put a phantom display in a settings screen.
            if crtc.mode != 0 && crtc.width > 0 && crtc.height > 0 {
                crtcs.push(crtc);
            }
        }
        Some(crtcs)
    }

    /// The refresh rate of a mode, in millihertz.
    ///
    /// `dot_clock / (htotal × vtotal)` — the calculation that produces 59.94
    /// rather than 60, which is the whole reason
    /// [`MonitorInfo::refresh_millihertz`] is an integer count of thousandths.
    /// Interlaced and double-scan modes halve and double it respectively, per
    /// the RandR conventions every tool implements.
    fn refresh_of(&self, mode: u32) -> u32 {
        let Some(randr) = self.ext.randr else {
            return 0;
        };
        if mode == 0 {
            return 0;
        }
        // SAFETY: the connection and root are live.
        let resources = unsafe {
            let cookie = (randr.get_screen_resources_current)(self.raw(), self.root);
            (randr.get_screen_resources_current_reply)(self.raw(), cookie, core::ptr::null_mut())
        };
        if resources.is_null() {
            return 0;
        }
        // SAFETY: `resources` is a live reply, and the modes pointer names
        // `num_modes` descriptors inside the same allocation.
        let found = unsafe {
            let count = usize::from((*resources).num_modes);
            let modes = (randr.resources_modes)(resources);
            if count == 0 || modes.is_null() {
                None
            } else {
                core::slice::from_raw_parts(modes, count)
                    .iter()
                    .find(|info| info.id == mode)
                    .copied()
            }
        };
        // SAFETY: freed exactly once.
        unsafe { ffi::free_reply(resources) };

        let Some(info) = found else {
            return 0;
        };
        let total = u64::from(info.htotal) * u64::from(info.vtotal);
        if total == 0 {
            return 0;
        }
        /// `RR_Interlace`.
        const INTERLACE: u32 = 1 << 4;
        /// `RR_DoubleScan`.
        const DOUBLE_SCAN: u32 = 1 << 5;
        let mut millihertz = u64::from(info.dot_clock) * 1_000 / total;
        if info.mode_flags & DOUBLE_SCAN != 0 {
            millihertz /= 2;
        }
        if info.mode_flags & INTERLACE != 0 {
            millihertz *= 2;
        }
        u32::try_from(millihertz).unwrap_or(0)
    }

    /// An output's name, e.g. `"DP-2"`, or a placeholder.
    fn read_output_name(&self, output: u32) -> String {
        let Some(randr) = self.ext.randr else {
            return format!("output-{output}");
        };
        // SAFETY: the connection is live; `0` is `CurrentTime`, which
        // `GetOutputInfo` accepts.
        let reply = unsafe {
            let cookie = (randr.get_output_info)(self.raw(), output, 0);
            (randr.get_output_info_reply)(self.raw(), cookie, core::ptr::null_mut())
        };
        if reply.is_null() {
            return format!("output-{output}");
        }
        // SAFETY: `reply` is a live reply, and the name pointer names
        // `name_len` bytes inside the same allocation. The name is Latin-1 by
        // protocol and ASCII in practice.
        let name = unsafe {
            let length = usize::from((*reply).name_len);
            let bytes = (randr.output_name)(reply);
            if length == 0 || bytes.is_null() {
                None
            } else {
                Some(
                    core::slice::from_raw_parts(bytes, length)
                        .iter()
                        .map(|byte| char::from(*byte))
                        .collect::<String>(),
                )
            }
        };
        // SAFETY: freed exactly once.
        unsafe { ffi::free_reply(reply) };
        name.unwrap_or_else(|| format!("output-{output}"))
    }

    /// The primary output, or `None` when the server has no notion of one.
    fn read_primary_output(&self) -> Option<u32> {
        let randr = self.ext.randr?;
        // SAFETY: the connection and root are live.
        let reply = unsafe {
            let cookie = (randr.get_output_primary)(self.raw(), self.root);
            (randr.get_output_primary_reply)(self.raw(), cookie, core::ptr::null_mut())
        };
        if reply.is_null() {
            return None;
        }
        // SAFETY: `reply` is a live reply this call owns.
        let output = unsafe { (*reply).output };
        // SAFETY: freed exactly once.
        unsafe { ffi::free_reply(reply) };
        (output != 0).then_some(output)
    }

    /// `_NET_WORKAREA` — the desktop minus panels and docks.
    ///
    /// A window manager property, so it is `None` under bare `Xvfb`, and
    /// [`MonitorInfo::work_area`] then equals `bounds` — which that field's
    /// documentation already defines as the answer when the backend cannot
    /// find out.
    ///
    /// EWMH gives four words *per virtual desktop* and this reads the first,
    /// because the seam has no vocabulary for virtual desktops and the first is
    /// the one the current desktop almost always is. It is a rectangle over the
    /// whole screen rather than per monitor, which is why the caller intersects
    /// it with each monitor's bounds instead of using it directly.
    fn read_work_area(&self) -> Option<PhysicalRect> {
        let words = self.get_property_words(self.root, self.atoms.net_workarea);
        let [x, y, width, height, ..] = words[..] else {
            return None;
        };
        if width == 0 || height == 0 {
            return None;
        }
        Some(PhysicalRect::new(
            i32::try_from(x).unwrap_or(0),
            i32::try_from(y).unwrap_or(0),
            width,
            height,
        ))
    }
}

/// The part of `bounds` inside `area`, or `bounds` when they do not overlap.
///
/// A monitor entirely outside the work area — which happens for a moment during
/// a hotplug, before the window manager recomputes it — must not end up with a
/// zero-sized work area, because a consumer placing a window in it would then
/// place a zero-sized window.
fn intersect(bounds: PhysicalRect, area: PhysicalRect) -> PhysicalRect {
    let left = bounds.x.max(area.x);
    let top = bounds.y.max(area.y);
    let right = (bounds.x.saturating_add_unsigned(bounds.width))
        .min(area.x.saturating_add_unsigned(area.width));
    let bottom = (bounds.y.saturating_add_unsigned(bounds.height))
        .min(area.y.saturating_add_unsigned(area.height));
    if right <= left || bottom <= top {
        return bounds;
    }
    PhysicalRect::new(left, top, right.abs_diff(left), bottom.abs_diff(top))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_work_area_clips_the_monitor_it_overlaps() {
        // A 32-pixel panel across the top of a 1920×1080 primary.
        let bounds = PhysicalRect::new(0, 0, 1920, 1080);
        let area = PhysicalRect::new(0, 32, 1920, 1048);
        assert_eq!(
            intersect(bounds, area),
            PhysicalRect::new(0, 32, 1920, 1048)
        );
    }

    #[test]
    fn a_second_monitor_outside_the_work_area_keeps_its_full_bounds() {
        // `_NET_WORKAREA` is one rectangle over the whole desktop, so a monitor
        // to the right of it does not overlap at all. Clipping to nothing would
        // give a consumer a zero-sized rectangle to place windows in.
        let right = PhysicalRect::new(1920, 0, 1280, 1024);
        let area = PhysicalRect::new(0, 32, 1920, 1048);
        assert_eq!(intersect(right, area), right);
    }

    #[test]
    fn a_partial_overlap_keeps_only_the_overlapping_part() {
        let bounds = PhysicalRect::new(-800, -100, 1600, 1200);
        let area = PhysicalRect::new(0, 0, 1920, 1080);
        assert_eq!(intersect(bounds, area), PhysicalRect::new(0, 0, 800, 1080));
    }
}
