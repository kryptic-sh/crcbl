//! Panels on Win32: frameless tool windows placed by their anchors.
//!
//! What each [`PanelDesc`] field becomes here:
//!
//! | field | Win32 |
//! | --- | --- |
//! | `layer` | `WS_EX_TOPMOST` for `Top` and `Overlay`, which share the one topmost band; the bottom of the z-order for `Bottom` and `Background`, which is above the desktop and no lower |
//! | `anchor`, `margins`, the size | a rectangle on the monitor, from [`geometry::panel_rect`] |
//! | `reserve` | `None` places the panel inside the monitor's work area, clear of the taskbar and every appbar; `IgnoreOthers` places it on the whole monitor. `OwnExtent` reserves nothing yet, which [`ShellCaps::PANEL_RESERVE`](crate::ShellCaps::PANEL_RESERVE) being clear says, and is placed as `None` is |
//! | `keyboard` | `None` is `WS_EX_NOACTIVATE`, and the panel is shown without activating, so clicking and hovering it leave the keyboard where it was. `OnDemand` activates on a click. `Exclusive` takes the foreground when shown |
//! | `monitor` | the one asked for, else the one under the pointer |
//! | `namespace` | nothing: Win32 has no rules keyed by one |
//!
//! Every panel is `WS_EX_TOOLWINDOW`, which keeps it out of the taskbar and
//! Alt-Tab.
//!
//! # Placed again when the desktop moves
//!
//! A panel's rectangle comes from its monitor's geometry, so it is computed
//! again after `WM_DISPLAYCHANGE` and after a DPI change, and a panel whose
//! monitor was unplugged is asked to close; see
//! [`ShellEvent::MonitorRemoved`](crate::ShellEvent::MonitorRemoved). A work
//! area that changes without a display change (the taskbar moved, an appbar
//! appeared) arrives as `WM_SETTINGCHANGE`, which this backend does not read
//! yet; `docs/backlog.md` carries it.

use core::ptr;

use crate::{
    LogicalSize, Margins, MonitorId, MonitorInfo, PanelAnchor, PanelDesc, PanelKeyboard,
    PanelLayer, PhysicalRect, Reserve, ShellError, WindowDesc, WindowId,
};

use super::ffi::{self, Handle, Point, value};
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
}

impl Panel {
    pub(super) const fn new(desc: &PanelDesc<'_>, size: LogicalSize) -> Self {
        Self {
            monitor: desc.monitor,
            layer: desc.layer,
            anchor: desc.anchor,
            reserve: desc.reserve,
            margins: desc.margins,
            keyboard: desc.keyboard,
            size,
        }
    }

    /// Its rectangle on `monitor`, in desktop pixels.
    pub(super) fn rect_on(&self, monitor: &MonitorInfo) -> PhysicalRect {
        let area = match self.reserve {
            Reserve::IgnoreOthers => monitor.bounds,
            Reserve::None | Reserve::OwnExtent => monitor.work_area,
        };
        let scale = monitor.scale_factor;
        let insets = Insets {
            top: geometry::pixels(self.margins.top, scale),
            right: geometry::pixels(self.margins.right, scale),
            bottom: geometry::pixels(self.margins.bottom, scale),
            left: geometry::pixels(self.margins.left, scale),
        };
        geometry::panel_rect(area, self.anchor, self.size.to_physical(scale), insets)
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
        panel: &Panel,
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
        // is `show_panel`'s decision about activation and not the system's.
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
        if desc.visible {
            Self::show_panel(hwnd, panel.keyboard);
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
    /// The system may refuse the foreground to a process the user is not
    /// working in. That is logged, and the panel's
    /// [`Focus`](crate::ShellEvent::Focus) never arrives, which is how a
    /// program finds out.
    pub(super) fn show_panel(hwnd: Handle, keyboard: PanelKeyboard) {
        let command = match keyboard {
            PanelKeyboard::Exclusive => value::SW_SHOW_NORMAL,
            PanelKeyboard::None | PanelKeyboard::OnDemand => value::SW_SHOW_NO_ACTIVATE,
        };
        // SAFETY: this shell's own window; dispatches `WM_SHOWWINDOW` into the
        // window procedure synchronously, and nothing here is borrowed.
        unsafe { ffi::ShowWindow(hwnd, command) };
        if keyboard == PanelKeyboard::Exclusive {
            // SAFETY: a request about this shell's own window.
            let granted = unsafe { ffi::SetForegroundWindow(hwnd) } != 0;
            if !granted {
                crcbl_core::log::warn!(
                    "win32: the system refused a panel that takes the keyboard the foreground; \
                     keys go to the window the user was in"
                );
            }
        }
    }

    /// Places every panel again after the monitors changed, and asks each
    /// panel created for a monitor in `removed` to close.
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
            self.place_panel(window);
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
}
