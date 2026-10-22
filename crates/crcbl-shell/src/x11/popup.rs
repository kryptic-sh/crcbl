//! Popups on X11: override-redirect windows placed against a rectangle of
//! their parent.
//!
//! A [`WindowRole::Popup`](crate::WindowRole::Popup) is an **override-redirect**
//! window: the window manager neither reparents nor places it, so it opens
//! where it is put, which is what a menu needs and what a window manager would
//! otherwise second-guess. Nothing places it but this backend, which works its
//! rectangle out as Win32's does: the [`PopupDesc::anchor`] turned into
//! desktop pixels from the parent's origin on the root, the popup put on the
//! [`PopupPlacement`](crate::PopupPlacement) side of it, and flipped or slid to
//! stay in the work area of the monitor the anchor is on
//! ([`role::popup_rect`](crate::role::popup_rect)). `WM_TRANSIENT_FOR` names
//! the parent and `_NET_WM_WINDOW_TYPE` says menu or tooltip, for the
//! compositors that read them.
//!
//! [`PopupInput`] becomes:
//!
//! * `Passthrough` — an empty input shape, so the pointer goes to whatever is
//!   beneath. It needs SHAPE 1.1, which is why a passthrough popup is refused
//!   without it.
//! * `Pointer` — the pointer over it, as any window has; it never takes the
//!   keyboard, since an override-redirect window is never focused.
//! * `Grab` — the pointer and the keyboard grabbed to it while it is mapped,
//!   both with `owner_events` off, so everything comes to it in its own
//!   coordinates. A press outside it asks it to close and goes nowhere, as a
//!   compositor's popup grab does; motion outside it is not reported; and a
//!   fresh `Escape` asks it to close after its key is delivered
//!   ([`role::escape_dismisses`](crate::role::escape_dismisses)).
//!
//! Destroying the parent destroys its popups first, and a popup does not
//! follow its parent when the parent moves: X11 has no relation that moves one
//! window with another, so a moved parent's popup is the caller's to
//! reposition.

use crate::{
    LogicalSize, PhysicalPoint, PhysicalRect, PopupDesc, PopupInput, ShellError, WindowId,
};

use super::{X11Shell, ffi};

/// The area a popup is kept in when no monitor is known: the whole
/// coordinate space, so nothing is flipped or slid.
const WHOLE_DESKTOP: PhysicalRect = PhysicalRect::new(i32::MIN, i32::MIN, u32::MAX, u32::MAX);

/// `logical` at `scale`, in whole pixels.
#[expect(
    clippy::cast_possible_truncation,
    reason = "rounded and clamped into i32's range before the cast"
)]
fn pixels(logical: f64, scale: f64) -> i32 {
    (logical * scale)
        .round()
        .clamp(f64::from(i32::MIN), f64::from(i32::MAX)) as i32
}

impl X11Shell {
    /// Where a popup of logical `size` goes against `popup`'s anchor, in
    /// desktop pixels; see the [module docs](self).
    ///
    /// # Errors
    ///
    /// [`ShellError::InvalidWindow`] for a parent that is not a live window,
    /// and [`ShellError::Backend`] when the server cannot say where it is.
    pub(super) fn popup_rect_for(
        &self,
        popup: &PopupDesc,
        size: LogicalSize,
    ) -> Result<PhysicalRect, ShellError> {
        let parent = self.window(popup.parent)?;
        let (x, y) = self.origin_now(parent.id).ok_or_else(|| {
            ShellError::Backend("the X server could not say where the parent is".to_string())
        })?;
        let scale = self.scale_factor;
        let anchor = PhysicalRect::new(
            x.saturating_add(pixels(popup.anchor.origin.x, scale)),
            y.saturating_add(pixels(popup.anchor.origin.y, scale)),
            pixels(popup.anchor.size.width, scale).max(1).unsigned_abs(),
            pixels(popup.anchor.size.height, scale)
                .max(1)
                .unsigned_abs(),
        );
        let contains = |rect: PhysicalRect| {
            let (right, bottom) = (
                i64::from(rect.x) + i64::from(rect.width),
                i64::from(rect.y) + i64::from(rect.height),
            );
            anchor.x >= rect.x
                && anchor.y >= rect.y
                && i64::from(anchor.x) < right
                && i64::from(anchor.y) < bottom
        };
        let area = self
            .monitors
            .iter()
            .find(|monitor| contains(monitor.bounds))
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

    /// Marks the window `xid` as `popup`'s: its parent, and the kind of window
    /// it is. Before the map, so a compositor reads both before drawing it.
    pub(super) fn write_popup_properties(&self, xid: u32, popup: &PopupDesc) {
        let Ok(parent) = self.window(popup.parent) else {
            return;
        };
        let kind = if popup.input == PopupInput::Passthrough {
            self.conn.atoms.net_wm_window_type_tooltip
        } else {
            self.conn.atoms.net_wm_window_type_popup_menu
        };
        // SAFETY: the connection and window are live, and each value is one
        // 32-bit word, the format given.
        unsafe {
            (self.conn.lib.change_property)(
                self.conn.raw(),
                ffi::value::PROP_MODE_REPLACE,
                xid,
                ffi::value::ATOM_WM_TRANSIENT_FOR,
                ffi::value::ATOM_WINDOW,
                32,
                1,
                (&raw const parent.id).cast(),
            );
            (self.conn.lib.change_property)(
                self.conn.raw(),
                ffi::value::PROP_MODE_REPLACE,
                xid,
                self.conn.atoms.net_wm_window_type,
                ffi::value::ATOM_ATOM,
                32,
                1,
                (&raw const kind).cast(),
            );
        }
    }

    /// Grabs the pointer and the keyboard to the mapped popup `window`, as
    /// [`PopupInput::Grab`] has them.
    ///
    /// # Errors
    ///
    /// [`ShellError::Backend`] when another client holds either; whatever this
    /// call grabbed is let go again.
    pub(super) fn grab_for_popup(&mut self, window: WindowId) -> Result<(), ShellError> {
        let xid = self.window(window)?.id;
        self.grab_pointer(xid, false, ffi::value::NONE, ffi::value::NONE)?;
        self.forget_captures_except(window);
        // SAFETY: the connection and window are live; both modes `Async`, as
        // the pointer's, so nothing freezes.
        let reply = unsafe {
            let cookie = (self.conn.lib.grab_keyboard)(
                self.conn.raw(),
                0,
                xid,
                ffi::value::CURRENT_TIME,
                ffi::value::GRAB_MODE_ASYNC,
                ffi::value::GRAB_MODE_ASYNC,
            );
            (self.conn.lib.grab_keyboard_reply)(self.conn.raw(), cookie, core::ptr::null_mut())
        };
        let status = if reply.is_null() {
            None
        } else {
            // SAFETY: `reply` is a live reply this call owns, freed once.
            unsafe {
                let status = (*reply).status;
                ffi::free_reply(reply);
                Some(status)
            }
        };
        if status != Some(ffi::value::GRAB_SUCCESS) {
            self.release_popup_grab();
            return Err(ShellError::Backend(format!(
                "another client holds the keyboard (GrabKeyboard status {status:?})"
            )));
        }
        Ok(())
    }

    /// Lets go of a grabbing popup's pointer and keyboard.
    pub(super) fn release_popup_grab(&self) {
        // SAFETY: the connection is live; letting go of a grab nothing holds
        // is a no-op on the server.
        unsafe {
            (self.conn.lib.ungrab_pointer)(self.conn.raw(), ffi::value::CURRENT_TIME);
            (self.conn.lib.ungrab_keyboard)(self.conn.raw(), ffi::value::CURRENT_TIME);
        }
    }

    /// Whether `position` in `window` is outside a grabbing popup: where a
    /// press dismisses it and motion is not reported. `false` for any other
    /// window, and for a popup whose size is not known yet.
    pub(super) fn outside_grabbing_popup(
        &self,
        window: WindowId,
        position: Option<PhysicalPoint>,
    ) -> bool {
        let Ok(state) = self.window(window) else {
            return false;
        };
        if state.popup.map(|popup| popup.input) != Some(PopupInput::Grab) {
            return false;
        }
        // The size the app was told, or is about to be: a popup created at
        // its size hears no `ConfigureNotify`, so `last_size` stays unset.
        let size = state
            .configuration
            .or(state.pending)
            .map(|config| config.size);
        let (Some(size), Some(position)) = (size, position) else {
            return false;
        };
        position.x < 0.0
            || position.y < 0.0
            || position.x >= f64::from(size.width)
            || position.y >= f64::from(size.height)
    }

    /// Moves a popup against `anchor` and gives it `size`: the rectangle
    /// worked out as at creation, and the window configured there. The resize
    /// it causes is reported as any other, from `ConfigureNotify`.
    ///
    /// # Errors
    ///
    /// [`ShellError::InvalidWindow`] for a stale handle or a parent gone, and
    /// [`ShellError::InvalidDescriptor`] for a window that is not a popup.
    pub(super) fn move_popup(
        &mut self,
        window: WindowId,
        anchor: crate::LogicalRect,
        size: LogicalSize,
    ) -> Result<(), ShellError> {
        let state = self.window(window)?;
        let xid = state.id;
        let mut popup = state
            .popup
            .ok_or_else(|| crate::role::not_a_popup(window))?;
        popup.anchor = anchor;
        let rect = self.popup_rect_for(&popup, size)?;
        // In mask-bit order: X, Y, WIDTH, HEIGHT. Each is a 32-bit word on the
        // wire, the coordinates two's-complement.
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
        self.conn.flush();
        let state = self.window_mut(window)?;
        state.popup = Some(popup);
        state.requested_size = size;
        Ok(())
    }

    /// The popups whose parent is `window`.
    pub(super) fn popups_of(&self, window: WindowId) -> Vec<WindowId> {
        self.windows
            .iter()
            .filter(|(_, state)| state.popup.is_some_and(|popup| popup.parent == window))
            .map(|(handle, _)| handle.cast())
            .collect()
    }
}

#[cfg(test)]
mod tests {
    use super::pixels;

    #[test]
    fn an_anchor_rounds_to_whole_pixels_at_the_scale() {
        assert_eq!(pixels(10.0, 1.0), 10);
        assert_eq!(pixels(10.0, 1.5), 15);
        assert_eq!(pixels(10.3, 1.0), 10);
        assert_eq!(pixels(-4.0, 2.0), -8, "an anchor left of the parent");
    }
}
