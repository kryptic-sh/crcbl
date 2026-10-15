//! Panels on Win32: frameless tool windows placed by their anchors.
//!
//! What each [`PanelDesc`] field becomes here:
//!
//! | field | Win32 |
//! | --- | --- |
//! | `layer` | `WS_EX_TOPMOST` for `Top` and `Overlay`, which share the one topmost band; the bottom of the z-order for `Bottom` and `Background`, which is above the desktop and no lower |
//! | `anchor`, `margins`, the size | a rectangle on the monitor, from [`geometry::panel_rect`] |
//! | `reserve` | `None` places the panel inside the monitor's work area, clear of the taskbar and every appbar; `IgnoreOthers` places it on the whole monitor; `OwnExtent` makes it an appbar — see below |
//! | `keyboard` | `None` is `WS_EX_NOACTIVATE`, and the panel is shown without activating, so clicking and hovering it leave the keyboard where it was. `OnDemand` activates on a click. `Exclusive` takes the foreground when shown |
//! | `monitor` | the one asked for, else the one under the pointer |
//! | `namespace` | nothing: Win32 has no rules keyed by one |
//!
//! Every panel is `WS_EX_TOOLWINDOW`, which keeps it out of the taskbar and
//! Alt-Tab.
//!
//! # A reserving panel is an appbar
//!
//! `SHAppBarMessage` is how Windows reserves an edge: the taskbar is one, and
//! a maximised window stops at every one. [`Reserve::OwnExtent`] registers the
//! panel with `ABM_NEW`, proposes a strip of its monitor as deep as the
//! panel's extent across the edge plus its margin on it, lets `ABM_QUERYPOS`
//! move that strip clear of the taskbar and the other appbars, and reserves
//! the result with `ABM_SETPOS`. The window goes inside the strip, its margins
//! from the strip's sides. When the shell sends `ABN_POSCHANGED` — the
//! taskbar moved, another appbar came, the display changed — the panel asks
//! again. `ABM_REMOVE` gives the space back when the panel is destroyed and
//! when the shell is dropped.
//!
//! **A process that dies without `ABM_REMOVE` leaves the space reserved**
//! until Explorer restarts, and nothing here can run after a hard crash. A
//! panic unwinds through [`Drop`](Win32Shell), which removes it.
//! `docs/backlog.md` carries the crash-safe path. If the shell refuses
//! `ABM_NEW`, the panel is placed as `Reserve::None` and a warning says so.
//!
//! # Placed again when the desktop moves
//!
//! A panel's rectangle comes from its monitor's geometry, so it is computed
//! again after `WM_DISPLAYCHANGE`, after a DPI change, and after a work area
//! moves without a display change (the taskbar moved, an appbar came or went),
//! which arrives as `WM_SETTINGCHANGE`. A panel whose monitor was unplugged is
//! asked to close; see
//! [`ShellEvent::MonitorRemoved`](crate::ShellEvent::MonitorRemoved).

use core::ptr;

use crate::{
    LogicalSize, Margins, MonitorId, MonitorInfo, PanelAnchor, PanelDesc, PanelKeyboard,
    PanelLayer, PhysicalRect, Reserve, ShellError, WindowDesc, WindowId,
};

use super::ffi::{self, AppBarData, Handle, Point, appbar, msg, value};
use super::geometry::{self, Insets};
use super::shell::Win32Shell;
use super::window::{Created, class_name};

/// What a panel was created with, kept so it can be placed again.
#[derive(Clone, Copy, Debug, PartialEq)]
pub(super) struct Panel {
    /// The monitor asked for. `None` was the one under the pointer when the
    /// panel was made, and is then whichever one the panel is on.
    pub monitor: Option<MonitorId>,
    pub layer: PanelLayer,
    pub anchor: PanelAnchor,
    pub reserve: Reserve,
    pub margins: Margins,
    pub keyboard: PanelKeyboard,
    /// [`WindowDesc::size`]: the size along every axis that is not
    /// stretched.
    pub size: LogicalSize,
    /// The edge [`Reserve::OwnExtent`] reserves, or `None` for a panel that
    /// reserves nothing.
    pub edge: Option<PanelAnchor>,
    /// Whether the shell accepted the panel as an appbar.
    pub appbar: bool,
}

impl Panel {
    pub(super) fn new(desc: &PanelDesc<'_>, size: LogicalSize) -> Self {
        Self {
            monitor: desc.monitor,
            layer: desc.layer,
            anchor: desc.anchor,
            reserve: desc.reserve,
            margins: desc.margins,
            keyboard: desc.keyboard,
            size,
            edge: match desc.reserve {
                Reserve::OwnExtent => desc.reserved_edge(),
                Reserve::None | Reserve::IgnoreOthers => None,
            },
            appbar: false,
        }
    }

    /// How deep its appbar strip is, in pixels at `scale`: its extent across
    /// the reserved edge, plus its margin on that edge.
    fn depth(&self, edge: PanelAnchor, scale: f64) -> i32 {
        let (extent, margin) = match edge {
            PanelAnchor::BOTTOM => (self.size.height, self.margins.bottom),
            PanelAnchor::LEFT => (self.size.width, self.margins.left),
            PanelAnchor::RIGHT => (self.size.width, self.margins.right),
            _ => (self.size.height, self.margins.top),
        };
        geometry::pixels(extent, scale).saturating_add(geometry::pixels(margin, scale))
    }

    /// The insets its margins are at `scale`.
    fn insets(&self, scale: f64) -> Insets {
        Insets {
            top: geometry::pixels(self.margins.top, scale),
            right: geometry::pixels(self.margins.right, scale),
            bottom: geometry::pixels(self.margins.bottom, scale),
            left: geometry::pixels(self.margins.left, scale),
        }
    }

    /// Its rectangle on `monitor`, in desktop pixels.
    pub(super) fn rect_on(&self, monitor: &MonitorInfo) -> PhysicalRect {
        let area = match self.reserve {
            Reserve::IgnoreOthers => monitor.bounds,
            Reserve::None | Reserve::OwnExtent => monitor.work_area,
        };
        let scale = monitor.scale_factor;
        geometry::panel_rect(
            area,
            self.anchor,
            self.size.to_physical(scale),
            self.insets(scale),
        )
    }
}

impl Win32Shell {
    /// The monitor `panel` belongs on: the one it asked for, else the one
    /// under the pointer — where the user is, which is what a launcher has to
    /// open on.
    fn panel_monitor(&self, panel: &Panel) -> Option<MonitorInfo> {
        if let Some(id) = panel.monitor {
            return self.monitors.iter().find(|m| m.id == id).cloned();
        }
        let mut cursor = Point::default();
        // SAFETY: `cursor` is a live `POINT` the call writes into.
        let under = if unsafe { ffi::GetCursorPos(&raw mut cursor) } == 0 {
            None
        } else {
            // SAFETY: a pure query; the `HMONITOR` is a system-owned
            // pseudo-handle that needs no release.
            let handle =
                unsafe { ffi::MonitorFromPoint(cursor, value::MONITOR_DEFAULT_TO_NEAREST) };
            super::monitors::device_name(handle).and_then(|device| self.monitor_id_of(&device))
        };
        under
            .and_then(|id| self.monitors.iter().find(|m| m.id == id))
            .or_else(|| self.monitors.iter().find(|m| m.is_primary))
            .or_else(|| self.monitors.first())
            .cloned()
    }

    /// Creates a panel's window, placed and shown as the [module docs](self)
    /// say.
    ///
    /// # Errors
    ///
    /// [`ShellError::InvalidDescriptor`] for a title with a NUL in it, and
    /// [`ShellError::WindowCreation`] if there is no monitor to place it on or
    /// the system refused the window.
    pub(super) fn create_panel(
        &self,
        desc: &WindowDesc<'_>,
        panel: &mut Panel,
    ) -> Result<Created, ShellError> {
        let title = ffi::wide(desc.title).ok_or_else(|| {
            ShellError::InvalidDescriptor("title contains a NUL byte".to_string())
        })?;
        let monitor = self.panel_monitor(panel).ok_or_else(|| {
            ShellError::WindowCreation("there is no monitor to place a panel on".to_string())
        })?;
        let rect = panel.rect_on(&monitor);
        let styles = geometry::panel_styles(panel.layer, panel.keyboard);
        let name = class_name();
        // SAFETY: as `create_native_window`: NUL-terminated strings that
        // outlive the call, this shell's registered class, a top-level window
        // with no menu, and the `Shared` this shell owns as the parameter. The
        // window is created hidden, whatever `visible` says, so that showing it
        // is `show_panel`'s decision about activation and not the system's,
        // made by `create_window` once the window has a handle to report a
        // refused foreground against.
        let hwnd = unsafe {
            ffi::CreateWindowExW(
                styles.ex_style,
                name.as_ptr(),
                title.as_ptr(),
                styles.style,
                rect.x,
                rect.y,
                i32::try_from(rect.width).unwrap_or(i32::MAX),
                i32::try_from(rect.height).unwrap_or(i32::MAX),
                ptr::null_mut(),
                ptr::null_mut(),
                self.instance(),
                self.shared_ptr(),
            )
        };
        if hwnd.is_null() {
            // SAFETY: reads this thread's last error code.
            let error = unsafe { ffi::GetLastError() };
            return Err(ShellError::WindowCreation(format!(
                "CreateWindowExW failed with Win32 error {error}"
            )));
        }
        if let Some(edge) = panel.edge {
            panel.appbar = Self::register_appbar(hwnd);
            if panel.appbar {
                Self::place_appbar(hwnd, panel, &monitor, edge);
            }
        }
        if matches!(panel.layer, PanelLayer::Bottom | PanelLayer::Background) {
            // SAFETY: this shell's own window; `HWND_BOTTOM` is the documented
            // pseudo-handle, and the size and position are unused.
            unsafe {
                ffi::SetWindowPos(
                    hwnd,
                    value::HWND_BOTTOM as Handle,
                    0,
                    0,
                    0,
                    0,
                    value::SWP_NO_MOVE | value::SWP_NO_SIZE | value::SWP_NO_ACTIVATE,
                );
            }
        }
        Ok(Created {
            hwnd,
            dpi: Self::dpi_of(hwnd),
            size: Self::client_size_of(hwnd),
        })
    }

    /// Shows a panel: without activating it, unless it takes the keyboard
    /// outright, in which case it is also brought to the foreground.
    ///
    /// `false` when the system refused the foreground, which it does to a
    /// process the user is not working in; the caller reports it as
    /// [`FocusRefused`](crate::ShellEvent::FocusRefused).
    #[must_use]
    pub(super) fn show_panel(hwnd: Handle, keyboard: PanelKeyboard) -> bool {
        let command = match keyboard {
            PanelKeyboard::Exclusive => value::SW_SHOW_NORMAL,
            PanelKeyboard::None | PanelKeyboard::OnDemand => value::SW_SHOW_NO_ACTIVATE,
        };
        // SAFETY: this shell's own window; dispatches `WM_SHOWWINDOW` into the
        // window procedure synchronously, and nothing here is borrowed.
        unsafe { ffi::ShowWindow(hwnd, command) };
        if keyboard == PanelKeyboard::Exclusive {
            // SAFETY: a request about this shell's own window.
            return unsafe { ffi::SetForegroundWindow(hwnd) } != 0;
        }
        true
    }

    /// Places every panel again after the monitors changed, and asks each
    /// panel created for a monitor in `removed` to close.
    ///
    /// Except appbars, which the shell tells to move itself, with
    /// `ABN_POSCHANGED`. Reserving moves the work area, which arrives here as
    /// a monitor change, so an appbar placed again from here would reserve
    /// again in answer to its own reservation.
    pub(super) fn place_panels(&mut self, removed: &[MonitorId]) {
        let panels: Vec<WindowId> = self
            .windows_iter()
            .filter(|(_, window)| window.panel.is_some())
            .map(|(id, _)| id)
            .collect();
        for window in panels {
            let Ok(state) = self.window(window) else {
                continue;
            };
            let Some(panel) = state.panel else { continue };
            if panel
                .monitor
                .is_some_and(|monitor| removed.contains(&monitor))
            {
                self.close_requested(window);
                continue;
            }
            if !panel.appbar {
                self.place_panel(window);
            }
        }
    }

    /// Moves one panel to where it belongs now. The resize it causes is
    /// reported as any other, through `WM_SIZE`.
    pub(super) fn place_panel(&self, window: WindowId) {
        let Ok(state) = self.window(window) else {
            return;
        };
        let Some(panel) = state.panel else { return };
        let hwnd = state.raw();
        let monitor = match panel.monitor {
            Some(_) => self.panel_monitor(&panel),
            None => self
                .monitor_of(hwnd)
                .and_then(|id| self.monitors.iter().find(|m| m.id == id).cloned()),
        };
        let Some(monitor) = monitor else { return };
        if let (true, Some(edge)) = (panel.appbar, panel.edge) {
            Self::place_appbar(hwnd, &panel, &monitor, edge);
            return;
        }
        let rect = panel.rect_on(&monitor);
        // SAFETY: this shell's own window; the z-order is left alone, and the
        // panel is not activated by being moved.
        unsafe {
            ffi::SetWindowPos(
                hwnd,
                ptr::null_mut(),
                rect.x,
                rect.y,
                i32::try_from(rect.width).unwrap_or(i32::MAX),
                i32::try_from(rect.height).unwrap_or(i32::MAX),
                value::SWP_NO_Z_ORDER | value::SWP_NO_ACTIVATE,
            );
        }
    }

    /// Registers `hwnd` as an appbar, with [`msg::APP_BAR`] for its
    /// notifications. `false`, with a warning, if the shell refuses.
    fn register_appbar(hwnd: Handle) -> bool {
        let mut data = AppBarData::new(hwnd);
        data.callback_message = msg::APP_BAR;
        // SAFETY: `data` is a live, self-describing `APPBARDATA` for this
        // shell's own window.
        let registered = unsafe { ffi::SHAppBarMessage(appbar::NEW, &raw mut data) } != 0;
        if !registered {
            crcbl_core::log::warn!(
                "win32: the shell refused a reserving panel as an appbar; it reserves nothing"
            );
        }
        registered
    }

    /// Reserves an appbar's strip of `monitor` and moves the window into it;
    /// see the [module docs](self).
    fn place_appbar(hwnd: Handle, panel: &Panel, monitor: &MonitorInfo, edge: PanelAnchor) {
        let scale = monitor.scale_factor;
        let depth = panel.depth(edge, scale);
        let mut data = AppBarData::new(hwnd);
        data.edge = match edge {
            PanelAnchor::BOTTOM => appbar::EDGE_BOTTOM,
            PanelAnchor::LEFT => appbar::EDGE_LEFT,
            PanelAnchor::RIGHT => appbar::EDGE_RIGHT,
            _ => appbar::EDGE_TOP,
        };
        data.rc = geometry::with_depth(geometry::rect_from(monitor.bounds), edge, depth);
        // SAFETY: `data` is a live, self-describing `APPBARDATA` for this
        // shell's registered appbar; each call writes `data.rc`.
        unsafe {
            ffi::SHAppBarMessage(appbar::QUERY_POS, &raw mut data);
            data.rc = geometry::with_depth(data.rc, edge, depth);
            ffi::SHAppBarMessage(appbar::SET_POS, &raw mut data);
        }
        let rect = geometry::panel_rect(
            geometry::desktop_rect(data.rc),
            panel.anchor,
            panel.size.to_physical(scale),
            panel.insets(scale),
        );
        // SAFETY: this shell's own window; the z-order is left alone.
        unsafe {
            ffi::SetWindowPos(
                hwnd,
                ptr::null_mut(),
                rect.x,
                rect.y,
                i32::try_from(rect.width).unwrap_or(i32::MAX),
                i32::try_from(rect.height).unwrap_or(i32::MAX),
                value::SWP_NO_Z_ORDER | value::SWP_NO_ACTIVATE,
            );
        }
    }

    /// Gives an appbar's space back.
    pub(super) fn remove_appbar(hwnd: Handle) {
        let mut data = AppBarData::new(hwnd);
        // SAFETY: `data` is a live, self-describing `APPBARDATA` for this
        // shell's own appbar.
        unsafe { ffi::SHAppBarMessage(appbar::REMOVE, &raw mut data) };
    }
}

/// What an `ABN_*` notification asks of the appbar it went to.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum AppBarNotice {
    /// The taskbar or another appbar moved: ask for this one's place again.
    Moved,
    /// A full-screen window opened on the appbar's monitor (`true`) or the
    /// last one closed (`false`).
    FullScreenApp(bool),
}

/// Decodes the `ABN_*` code and `lParam` of an appbar's callback message, or
/// `None` for one a panel does nothing with.
pub(super) const fn appbar_notice(notification: u32, param: isize) -> Option<AppBarNotice> {
    match notification {
        appbar::POS_CHANGED => Some(AppBarNotice::Moved),
        appbar::FULL_SCREEN_APP => Some(AppBarNotice::FullScreenApp(param != 0)),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// `ABN_STATECHANGE` and `ABN_WINDOWARRANGE`, the codes a panel ignores.
    const STATE_CHANGE: u32 = 0;
    const WINDOW_ARRANGE: u32 = 3;

    #[test]
    fn an_appbar_notice_is_decoded_from_its_code_and_lparam() {
        assert_eq!(
            appbar_notice(appbar::POS_CHANGED, 0),
            Some(AppBarNotice::Moved)
        );
        assert_eq!(
            appbar_notice(appbar::FULL_SCREEN_APP, 1),
            Some(AppBarNotice::FullScreenApp(true))
        );
        assert_eq!(
            appbar_notice(appbar::FULL_SCREEN_APP, 0),
            Some(AppBarNotice::FullScreenApp(false))
        );
        assert_eq!(
            appbar_notice(appbar::FULL_SCREEN_APP, -1),
            Some(AppBarNotice::FullScreenApp(true)),
            "any nonzero BOOL is TRUE"
        );
        assert_eq!(appbar_notice(STATE_CHANGE, 1), None);
        assert_eq!(appbar_notice(WINDOW_ARRANGE, 1), None);
    }
}
