//! Panels on X11: dock windows placed by their anchors.
//!
//! What each [`PanelDesc`] field becomes here:
//!
//! | field | X11 |
//! | --- | --- |
//! | `layer` | `_NET_WM_STATE_ABOVE` for `Top` and `Overlay`, `_NET_WM_STATE_BELOW` for `Bottom` and `Background`; EWMH has those two bands and no more |
//! | `anchor`, `margins`, the size | a rectangle on the monitor, from [`role::panel_rect`](crate::role::panel_rect) |
//! | `reserve` | `None` places the panel inside the monitor's work area, clear of what other panels reserved; `IgnoreOthers` and `OwnExtent` on the whole monitor; `OwnExtent` also writes `_NET_WM_STRUT_PARTIAL` — see below |
//! | `keyboard` | `WM_HINTS`'s input flag: off for `None`, so the window manager never focuses it; on for `OnDemand`, which a click focuses; on for `Exclusive`, which is also given the focus when it maps. Nothing stops the user clicking it away, so [`PANEL_EXCLUSIVE_KEYBOARD`](crate::ShellCaps::PANEL_EXCLUSIVE_KEYBOARD) is clear |
//! | `monitor` | the one asked for, else the one under the pointer |
//! | `namespace` | nothing: X11 has no rules keyed by one |
//!
//! Every panel is a `_NET_WM_WINDOW_TYPE_DOCK` window, which a window manager
//! leaves undecorated and out of its task list, on every desktop
//! (`_NET_WM_DESKTOP` all ones, and `_NET_WM_STATE_STICKY`; a window manager
//! that applies a strut per desktop applies it only where its window is), at
//! the position `WM_NORMAL_HINTS` gives with `USPosition`.
//!
//! # A reserving panel writes a strut
//!
//! `_NET_WM_STRUT_PARTIAL` is how a window reserves an edge: the window
//! manager takes the space out of `_NET_WORKAREA`, and maximised windows stop
//! at it. Only a window manager reads it, so
//! [`PANEL_RESERVE`](crate::ShellCaps::PANEL_RESERVE) is set only while one is
//! running. A strut is measured from the **root window's** edges, so a panel
//! on an edge of a monitor that is not an edge of the whole screen reserves
//! nothing a window manager will honour. And a reserving panel is placed on
//! the whole monitor, not its work area: the work area already has the
//! panel's own strut taken out, and placing it there would push it off its own
//! space; so two reserving panels on one edge overlap.
//!
//! # Placed again when the desktop moves
//!
//! A panel's rectangle comes from its monitor's geometry, so it is worked out
//! again by [`Shell::set_panel`](crate::Shell::set_panel), when RandR says the
//! monitors changed, and when the window manager rewrites `_NET_WORKAREA`
//! (another panel came or went), which the root's `PropertyChange` brings once
//! a panel exists. A panel whose monitor was unplugged is asked to close.

use crate::role::{Insets, KeptPanel};
use crate::{
    LogicalSize, MonitorId, MonitorInfo, PanelAnchor, PanelDesc, PanelKeyboard, PanelLayer,
    PhysicalRect, PhysicalSize, Reserve, ShellError, WindowId,
};

use super::keys::WmStateAction;
use super::window::words_to_bytes;
use super::{X11Shell, ffi};

/// `logical` at `scale`, in whole pixels, never negative.
#[expect(
    clippy::cast_possible_truncation,
    reason = "rounded and clamped into i32's range before the cast"
)]
fn pixels(logical: f64, scale: f64) -> i32 {
    (logical * scale).round().clamp(0.0, f64::from(i32::MAX)) as i32
}

/// What a panel was created with, kept so it can be placed again.
#[derive(Clone, Debug, PartialEq)]
pub(super) struct Panel {
    pub kept: KeptPanel,
    /// [`WindowDesc::size`](crate::WindowDesc::size): the size along every
    /// axis that is not stretched.
    pub size: LogicalSize,
}

impl Panel {
    pub(super) fn new(desc: &PanelDesc<'_>, size: LogicalSize) -> Self {
        Self {
            kept: KeptPanel::new(desc),
            size,
        }
    }

    /// The edge [`Reserve::OwnExtent`] reserves, or `None` for a panel that
    /// reserves nothing.
    fn edge(&self) -> Option<PanelAnchor> {
        let desc = self.kept.desc();
        match desc.reserve {
            Reserve::OwnExtent => desc.reserved_edge(),
            Reserve::None | Reserve::IgnoreOthers => None,
        }
    }

    /// Its rectangle on `monitor`, in root pixels at `scale`.
    pub(super) fn rect_on(&self, monitor: &MonitorInfo, scale: f64) -> PhysicalRect {
        let desc = self.kept.desc();
        let area = match desc.reserve {
            Reserve::None => monitor.work_area,
            Reserve::IgnoreOthers | Reserve::OwnExtent => monitor.bounds,
        };
        let margins = desc.margins;
        crate::role::panel_rect(
            area,
            desc.anchor,
            self.size.to_physical(scale),
            Insets {
                top: pixels(margins.top, scale),
                right: pixels(margins.right, scale),
                bottom: pixels(margins.bottom, scale),
                left: pixels(margins.left, scale),
            },
        )
    }
}

/// `_NET_WM_STRUT_PARTIAL` for a panel at `rect` reserving `edge` of a root
/// window of `screen`: twelve words, left, right, top and bottom depths from
/// the root's edges, then the start and end of each along its edge.
fn strut_partial(edge: PanelAnchor, rect: PhysicalRect, screen: PhysicalSize) -> [u32; 12] {
    let word = |value: i64| u32::try_from(value.max(0)).unwrap_or(u32::MAX);
    let (x, y) = (i64::from(rect.x), i64::from(rect.y));
    let (right, bottom) = (x + i64::from(rect.width), y + i64::from(rect.height));
    let mut strut = [0; 12];
    match edge {
        PanelAnchor::LEFT => {
            strut[0] = word(right);
            strut[4] = word(y);
            strut[5] = word(bottom - 1);
        }
        PanelAnchor::RIGHT => {
            strut[1] = word(i64::from(screen.width) - x);
            strut[6] = word(y);
            strut[7] = word(bottom - 1);
        }
        PanelAnchor::BOTTOM => {
            strut[3] = word(i64::from(screen.height) - y);
            strut[10] = word(x);
            strut[11] = word(right - 1);
        }
        _ => {
            strut[2] = word(bottom);
            strut[8] = word(x);
            strut[9] = word(right - 1);
        }
    }
    strut
}

impl X11Shell {
    /// The monitor a panel goes on: the one it asked for, else the one under
    /// the pointer — where the user is.
    pub(super) fn panel_monitor(&self, monitor: Option<MonitorId>) -> Option<MonitorInfo> {
        if let Some(id) = monitor {
            return self.monitors.iter().find(|m| m.id == id).cloned();
        }
        let under = self
            .conn
            .query_pointer(self.conn.root)
            .and_then(|(x, y, _)| {
                let (x, y) = (i32::from(x), i32::from(y));
                self.monitors.iter().find(|monitor| {
                    let bounds = monitor.bounds;
                    x >= bounds.x
                        && y >= bounds.y
                        && i64::from(x) < i64::from(bounds.x) + i64::from(bounds.width)
                        && i64::from(y) < i64::from(bounds.y) + i64::from(bounds.height)
                })
            });
        under
            .or_else(|| self.monitors.iter().find(|m| m.is_primary))
            .or_else(|| self.monitors.first())
            .cloned()
    }

    /// Where `panel` goes now, in root pixels.
    ///
    /// # Errors
    ///
    /// [`ShellError::WindowCreation`] when there is no monitor to place it on.
    pub(super) fn panel_rect_for(&self, panel: &Panel) -> Result<PhysicalRect, ShellError> {
        let monitor = self
            .panel_monitor(panel.kept.desc().monitor)
            .ok_or_else(|| ShellError::WindowCreation("no monitor to place the panel on".into()))?;
        Ok(panel.rect_on(&monitor, self.scale_factor))
    }

    /// Writes what makes `xid` a panel: a dock, on every desktop, in its
    /// layer, taking the keyboard or not, at its position, and reserving its
    /// edge. `mapped` says whether a window manager already manages it, when
    /// its state is a request rather than a property.
    pub(super) fn write_panel_properties(
        &self,
        xid: u32,
        panel: &Panel,
        was: Option<&Panel>,
        rect: PhysicalRect,
        mapped: bool,
    ) {
        let atoms = self.conn.atoms;
        let desc = panel.kept.desc();
        self.conn.set_property(
            xid,
            atoms.net_wm_window_type,
            ffi::value::ATOM_ATOM,
            32,
            &words_to_bytes(&[atoms.net_wm_window_type_dock]),
        );
        let band = |layer: PanelLayer| match layer {
            PanelLayer::Top | PanelLayer::Overlay => atoms.net_wm_state_above,
            PanelLayer::Bottom | PanelLayer::Background => atoms.net_wm_state_below,
        };
        if mapped {
            if let Some(was) = was.map(|was| band(was.kept.desc().layer))
                && was != band(desc.layer)
            {
                self.request_wm_state(xid, WmStateAction::Remove, was);
            }
            self.request_wm_state(xid, WmStateAction::Add, band(desc.layer));
        } else {
            self.conn.set_property(
                xid,
                atoms.net_wm_desktop,
                ffi::value::ATOM_CARDINAL,
                32,
                &words_to_bytes(&[u32::MAX]),
            );
            self.conn.set_property(
                xid,
                atoms.net_wm_state,
                ffi::value::ATOM_ATOM,
                32,
                &words_to_bytes(&[band(desc.layer), atoms.net_wm_state_sticky]),
            );
        }

        // `WM_HINTS`: `InputHint`, and the input flag the keyboard asks for.
        let input = u32::from(desc.keyboard != PanelKeyboard::None);
        self.conn.set_property(
            xid,
            ffi::value::ATOM_WM_HINTS,
            ffi::value::ATOM_WM_HINTS,
            32,
            &words_to_bytes(&[1, input, 0, 0, 0, 0, 0, 0, 0]),
        );

        // `WM_NORMAL_HINTS` with `USPosition | USSize | PMinSize | PMaxSize`:
        // the position is the panel's own, and the size is fixed. Eighteen
        // words in ICCCM's order; the obsolete x, y, width and height are
        // filled in too, for the window managers that still read them.
        /// `USPosition | USSize | PMinSize | PMaxSize`.
        const FLAGS: u32 = 1 | 2 | 16 | 32;
        let (width, height) = (rect.width.max(1), rect.height.max(1));
        let mut hints = [0u32; 18];
        hints[0] = FLAGS;
        hints[1] = rect.x.cast_unsigned();
        hints[2] = rect.y.cast_unsigned();
        hints[3] = width;
        hints[4] = height;
        hints[5] = width;
        hints[6] = height;
        hints[7] = width;
        hints[8] = height;
        self.conn.set_property(
            xid,
            ffi::value::ATOM_WM_NORMAL_HINTS,
            ffi::value::ATOM_WM_SIZE_HINTS,
            32,
            &words_to_bytes(&hints),
        );

        match panel.edge() {
            Some(edge) => {
                let strut = strut_partial(edge, rect, self.conn.screen_size);
                self.conn.set_property(
                    xid,
                    atoms.net_wm_strut_partial,
                    ffi::value::ATOM_CARDINAL,
                    32,
                    &words_to_bytes(&strut),
                );
                // The older four-word form, for a window manager that reads
                // only that one.
                self.conn.set_property(
                    xid,
                    atoms.net_wm_strut,
                    ffi::value::ATOM_CARDINAL,
                    32,
                    &words_to_bytes(&strut[..4]),
                );
            }
            None => {
                self.conn.delete_property(xid, atoms.net_wm_strut_partial);
                self.conn.delete_property(xid, atoms.net_wm_strut);
            }
        }
    }

    /// Moves the window `xid` to `rect`.
    fn configure_to(&self, xid: u32, rect: PhysicalRect) {
        // In mask-bit order: X, Y, WIDTH, HEIGHT, one 32-bit word each.
        let values = [
            rect.x.cast_unsigned(),
            rect.y.cast_unsigned(),
            rect.width.max(1),
            rect.height.max(1),
        ];
        // SAFETY: the connection and window are live, and `values` has one
        // word for each bit of the mask.
        unsafe {
            (self.conn.lib.configure_window)(
                self.conn.raw(),
                xid,
                ffi::config_window::X
                    | ffi::config_window::Y
                    | ffi::config_window::WIDTH
                    | ffi::config_window::HEIGHT,
                values.as_ptr().cast(),
            );
        }
    }

    /// Gives a panel a new size and descriptor in place, for
    /// [`Shell::set_panel`](crate::Shell::set_panel): its properties written
    /// again and its place worked out again. The resize is reported as any
    /// other, from `ConfigureNotify`.
    ///
    /// # Errors
    ///
    /// What [`crate::role::check_set_panel`] refuses, and
    /// [`ShellError::WindowCreation`] with no monitor to place it on.
    pub(super) fn reconfigure_panel(
        &mut self,
        window: WindowId,
        size: LogicalSize,
        desc: &PanelDesc<'_>,
    ) -> Result<(), ShellError> {
        let state = self.window(window)?;
        let was = state.panel.clone();
        crate::role::check_set_panel(
            window,
            was.as_ref().map(|panel| panel.kept.desc()),
            desc,
            self.caps,
            crate::ShellBackend::X11,
            |monitor| self.monitors.iter().any(|m| m.id == monitor),
        )?;
        let (xid, mapped) = (state.id, state.map_requested);
        let panel = Panel::new(desc, size);
        let rect = self.panel_rect_for(&panel)?;
        self.write_panel_properties(xid, &panel, was.as_ref(), rect, mapped);
        self.configure_to(xid, rect);
        self.conn.flush();
        let state = self.window_mut(window)?;
        state.panel = Some(panel);
        state.requested_size = size;
        Ok(())
    }

    /// Places every panel again after the monitors or the work area changed,
    /// and asks the ones whose monitor is in `removed` to close.
    pub(super) fn place_panels(&mut self, removed: &[MonitorId]) {
        let panels: Vec<(WindowId, u32, Panel, bool)> = self
            .windows
            .iter()
            .filter_map(|(handle, state)| {
                let panel = state.panel.clone()?;
                Some((handle.cast(), state.id, panel, state.map_requested))
            })
            .collect();
        for (window, xid, panel, mapped) in panels {
            if panel
                .kept
                .desc()
                .monitor
                .is_some_and(|monitor| removed.contains(&monitor))
            {
                self.request_close(window);
                continue;
            }
            if let Ok(rect) = self.panel_rect_for(&panel) {
                self.write_panel_properties(xid, &panel, Some(&panel), rect, mapped);
                self.configure_to(xid, rect);
            }
        }
        self.conn.flush();
    }

    /// Gives an `Exclusive` panel the keyboard once it is viewable.
    pub(super) fn focus_exclusive_panel(&self, window: WindowId) {
        let Ok(state) = self.window(window) else {
            return;
        };
        if state
            .panel
            .as_ref()
            .is_some_and(|panel| panel.kept.desc().keyboard == PanelKeyboard::Exclusive)
        {
            // SAFETY: the connection and window are live, and the window has
            // just mapped, so it is viewable.
            unsafe {
                (self.conn.lib.set_input_focus)(
                    self.conn.raw(),
                    ffi::value::FOCUS_POINTER_ROOT,
                    state.id,
                    ffi::value::CURRENT_TIME,
                );
            }
            self.conn.flush();
        }
    }

    /// Selects `PropertyChange` on the root, once, so a window manager
    /// rewriting `_NET_WORKAREA` reaches [`place_panels`](Self::place_panels).
    pub(super) fn watch_work_area(&mut self) {
        if self.watching_root {
            return;
        }
        let values = [ffi::event_mask::PROPERTY_CHANGE];
        // SAFETY: the connection and root are live, and the value list has
        // the one word `EVENT_MASK` declares. This client selected no core
        // events on the root before, so nothing is replaced.
        unsafe {
            (self.conn.lib.change_window_attributes)(
                self.conn.raw(),
                self.conn.root,
                ffi::cw::EVENT_MASK,
                values.as_ptr().cast(),
            );
        }
        self.watching_root = true;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// **A strut is measured from the root's edges**: a 30-pixel bar across
    /// the top of the second of two 1920-wide monitors reserves 30 from the
    /// top along that monitor's span, and a bottom bar 1050 down a 1080-high
    /// screen reserves the 30 below its top.
    #[test]
    fn a_strut_is_the_panel_measured_from_the_root() {
        let screen = PhysicalSize::new(3840, 1080);
        let top = strut_partial(
            PanelAnchor::TOP,
            PhysicalRect::new(1920, 0, 1920, 30),
            screen,
        );
        assert_eq!(top, [0, 0, 30, 0, 0, 0, 0, 0, 1920, 3839, 0, 0]);
        let bottom = strut_partial(
            PanelAnchor::BOTTOM,
            PhysicalRect::new(0, 1050, 1920, 30),
            screen,
        );
        assert_eq!(bottom, [0, 0, 0, 30, 0, 0, 0, 0, 0, 0, 0, 1919]);
        let left = strut_partial(
            PanelAnchor::LEFT,
            PhysicalRect::new(4, 100, 40, 500),
            screen,
        );
        assert_eq!(
            left,
            [44, 0, 0, 0, 100, 599, 0, 0, 0, 0, 0, 0],
            "its margin too"
        );
        let right = strut_partial(
            PanelAnchor::RIGHT,
            PhysicalRect::new(3790, 0, 50, 1080),
            screen,
        );
        assert_eq!(right, [0, 50, 0, 0, 0, 0, 0, 1079, 0, 0, 0, 0]);
    }

    /// A panel that does not reserve has the work area; one that does, or
    /// ignores the others, has the whole monitor.
    #[test]
    fn a_reserving_panel_is_placed_on_the_whole_monitor() {
        let monitor = MonitorInfo {
            id: MonitorId(1),
            name: "HEAD-0".to_string(),
            bounds: PhysicalRect::new(0, 0, 1920, 1080),
            work_area: PhysicalRect::new(0, 30, 1920, 1050),
            scale_factor: 1.0,
            refresh_millihertz: 60_000,
            is_primary: true,
        };
        let desc = |reserve| PanelDesc {
            anchor: PanelAnchor::TOP | PanelAnchor::LEFT | PanelAnchor::RIGHT,
            reserve,
            ..PanelDesc::default()
        };
        let size = LogicalSize::new(0.0, 20.0);
        let rect = |reserve| Panel::new(&desc(reserve), size).rect_on(&monitor, 1.0);
        assert_eq!(rect(Reserve::None), PhysicalRect::new(0, 30, 1920, 20));
        assert_eq!(rect(Reserve::OwnExtent), PhysicalRect::new(0, 0, 1920, 20));
        assert_eq!(
            rect(Reserve::IgnoreOthers),
            PhysicalRect::new(0, 0, 1920, 20)
        );
    }
}
