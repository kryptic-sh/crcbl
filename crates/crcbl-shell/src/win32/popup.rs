//! Popups on Win32: owned, frameless tool windows placed against a rectangle
//! of their parent.
//!
//! A [`WindowRole::Popup`](crate::WindowRole::Popup) is a `WS_POPUP` window
//! **owned** by its parent, which is what keeps it above the parent and takes
//! it along when the parent is minimised. Its rectangle is worked out here —
//! Win32 has no positioner to ask — by [`role::popup_rect`](crate::role::popup_rect): the
//! [`PopupDesc::anchor`] turned into desktop pixels at the parent's scale, the
//! popup put on the [`PopupPlacement`](crate::PopupPlacement) side of it, and
//! flipped or slid to stay in the work area of the monitor the parent is on.
//!
//! [`PopupInput`] becomes:
//!
//! * `Passthrough` — never activated, and `WM_NCHITTEST` answers
//!   `HTTRANSPARENT`, so the pointer goes to the window beneath on this thread,
//!   which a tooltip's bar is.
//! * `Pointer` — never activated: the pointer, not the keyboard.
//! * `Grab` — activated when shown, and asked to close when it loses the
//!   keyboard: a click outside it activates whatever was clicked, which is the
//!   dismissal a menu has.
//!
//! A popup of a topmost window is topmost too, or it would open behind the
//! bar that opened it.

use core::ptr;

use crate::{
    LogicalRect, LogicalSize, PhysicalRect, PopupDesc, PopupInput, ShellError, WindowDesc, WindowId,
};

use super::ffi::{self, Point, style, value};
use super::geometry;
use super::proc;
use super::shell::Win32Shell;
use super::window::{Created, class_name};

/// The area a popup is kept in when the session has no monitor at all: the
/// whole coordinate space, so nothing is flipped or slid.
const WHOLE_DESKTOP: PhysicalRect = PhysicalRect::new(i32::MIN, i32::MIN, u32::MAX, u32::MAX);

impl Win32Shell {
    /// Where a popup of logical `size` goes against `popup`'s anchor, in
    /// desktop pixels; see the [module docs](self).
    ///
    /// # Errors
    ///
    /// [`ShellError::InvalidWindow`] for a parent that is not a live window.
    fn popup_rect_for(
        &self,
        popup: &PopupDesc,
        size: LogicalSize,
    ) -> Result<PhysicalRect, ShellError> {
        let parent = self.window(popup.parent)?;
        let (parent_hwnd, scale) = (parent.raw(), parent.scale_factor);
        let mut origin = Point::default();
        // SAFETY: a live window of this shell, and a live `POINT` the call
        // converts in place from its client space to the desktop's.
        unsafe { ffi::ClientToScreen(parent_hwnd, &raw mut origin) };
        let anchor = PhysicalRect::new(
            origin
                .x
                .saturating_add(geometry::pixels(popup.anchor.origin.x, scale)),
            origin
                .y
                .saturating_add(geometry::pixels(popup.anchor.origin.y, scale)),
            geometry::pixels(popup.anchor.size.width, scale)
                .max(1)
                .unsigned_abs(),
            geometry::pixels(popup.anchor.size.height, scale)
                .max(1)
                .unsigned_abs(),
        );
        let area = self
            .monitor_of(parent_hwnd)
            .and_then(|id| self.monitors.iter().find(|monitor| monitor.id == id))
            .or_else(|| self.monitors.iter().find(|monitor| monitor.is_primary))
            .or_else(|| self.monitors.first())
            .map_or(WHOLE_DESKTOP, |monitor| monitor.work_area);
        Ok(crate::role::popup_rect(
            anchor,
            size.to_physical(scale),
            popup.placement,
            area,
        ))
    }

    /// Moves a popup against `anchor` and gives it `size`: the rectangle
    /// worked out as at creation, and the window moved there without
    /// activating it or changing its place in the z-order. The resize it
    /// causes is reported as any other, through `WM_SIZE`.
    ///
    /// # Errors
    ///
    /// [`ShellError::InvalidWindow`] for a stale handle or a parent gone, and
    /// [`ShellError::InvalidDescriptor`] for a window that is not a popup.
    pub(super) fn move_popup(
        &mut self,
        window: WindowId,
        anchor: LogicalRect,
        size: LogicalSize,
    ) -> Result<(), ShellError> {
        let state = self.window(window)?;
        let hwnd = state.raw();
        let mut popup = state
            .popup
            .ok_or_else(|| crate::role::not_a_popup(window))?;
        popup.anchor = anchor;
        let rect = self.popup_rect_for(&popup, size)?;
        // SAFETY: a live window of this shell, moved and sized in desktop
        // pixels; the null insert-after is ignored under `SWP_NOZORDER`.
        let moved = unsafe {
            ffi::SetWindowPos(
                hwnd,
                ptr::null_mut(),
                rect.x,
                rect.y,
                i32::try_from(rect.width).unwrap_or(i32::MAX),
                i32::try_from(rect.height).unwrap_or(i32::MAX),
                value::SWP_NO_Z_ORDER | value::SWP_NO_ACTIVATE,
            )
        };
        if moved == 0 {
            // SAFETY: reads this thread's last error code.
            let error = unsafe { ffi::GetLastError() };
            return Err(ShellError::Backend(format!(
                "SetWindowPos failed with Win32 error {error}"
            )));
        }
        let state = self.window_mut(window)?;
        state.popup = Some(popup);
        state.requested_size = size;
        Ok(())
    }

    /// Creates a popup's window, placed as the [module docs](self) say.
    ///
    /// # Errors
    ///
    /// [`ShellError::InvalidWindow`] for a parent that is not a live window,
    /// [`ShellError::InvalidDescriptor`] for a title with a NUL in it, and
    /// [`ShellError::WindowCreation`] if the system refused the window.
    pub(super) fn create_popup(
        &self,
        desc: &WindowDesc<'_>,
        popup: &PopupDesc,
    ) -> Result<Created, ShellError> {
        let parent_hwnd = self.window(popup.parent)?.raw();
        let title = ffi::wide(desc.title).ok_or_else(|| {
            ShellError::InvalidDescriptor("title contains a NUL byte".to_string())
        })?;
        let rect = self.popup_rect_for(popup, desc.size)?;

        // SAFETY: reading this shell's own window's extended style.
        let parent_ex_style =
            unsafe { ffi::GetWindowLongPtrW(parent_hwnd, value::GWL_EX_STYLE) } as u32;
        let styles = geometry::with_transparency(
            geometry::popup_styles(popup.input, parent_ex_style & style::EX_TOPMOST != 0),
            desc.transparent,
        );
        let name = class_name();
        // SAFETY: as `create_native_window`, with the parent as the owner: a
        // live window of this shell on this thread. Created hidden; showing is
        // `create_window`'s, once the window has a handle.
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
                parent_hwnd,
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
        if popup.input == PopupInput::Passthrough {
            self.shared().set_passthrough(proc::key(hwnd));
        }
        Ok(Created {
            hwnd,
            dpi: Self::dpi_of(hwnd),
            size: Self::client_size_of(hwnd),
        })
    }
}
