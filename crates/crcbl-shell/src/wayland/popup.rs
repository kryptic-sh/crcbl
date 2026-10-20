//! Popups as `xdg_popup` surfaces.
//!
//! A [`WindowRole::Popup`](crate::WindowRole::Popup) is an `xdg_popup` placed
//! by an `xdg_positioner`: the anchor rectangle is the
//! [`PopupDesc::anchor`] in the parent's logical coordinates, and
//! [`PopupPlacement`] becomes an anchor edge, a gravity and the adjustments
//! the compositor may make to keep the popup on screen — flipped to the other
//! side, or slid along this one, never resized.
//!
//! The parent is a toplevel, another popup, or a panel. A panel's popup is
//! made with no parent and given to the layer surface with
//! `zwlr_layer_surface_v1.get_popup` before its first commit, which is how
//! layer-shell parents one.
//!
//! [`PopupInput`] is the surface's input: `Passthrough` sets an empty input
//! region, so the pointer goes through to whatever is beneath; `Pointer` is a
//! surface's default; `Grab` sends `xdg_popup.grab` with the serial of the
//! seat's latest input, which the compositor checks, and which is why a grab
//! needs a user's input to have happened first. A grabbing popup is dismissed
//! by a click outside every surface of this program, and the compositor's
//! `popup_done` arrives as a
//! [`CloseRequested`](crate::ShellEvent::CloseRequested). **A click on the
//! program's own window is that window's**, delivered to it with the grab left
//! in place: a program closes its menu on that click itself. Win32 and AppKit
//! dismiss on it, because there the click moves the keyboard.

use crate::{
    LogicalRect, LogicalSize, PointerMode, PopupDesc, PopupInput, PopupPlacement, ShellBackend,
    ShellError, WindowConfiguration, WindowDesc, WindowId,
};
use core::ptr;

use super::ffi::WlProxy;
use super::protocol::layer_shell::zwlr_layer_surface_v1;
use super::protocol::wayland::{wl_compositor, wl_region, wl_surface};
use super::protocol::xdg_shell::{xdg_popup, xdg_positioner, xdg_surface, xdg_wm_base};
use super::{ObjectKind, PendingConfigure, WaylandShell, WlWindow, optional, round_logical};

/// The positioner's anchor edge, gravity and constraint adjustments for a
/// placement.
///
/// The anchor is the point of the anchor rectangle the popup is placed
/// against and the gravity the direction it grows from there: below, left
/// edges aligned, is the anchor's bottom-left corner growing down and right.
/// A popup may be flipped to the opposite side or slid along the side it is
/// on, so a menu at the screen's edge stays whole; resizing it is left out,
/// because the program drew it for the size it asked.
const fn positioner_of(placement: PopupPlacement) -> (u32, u32, u32) {
    use xdg_positioner::{anchor, constraint_adjustment as adjust, gravity};
    match placement {
        PopupPlacement::Below => (
            anchor::BOTTOM_LEFT,
            gravity::BOTTOM_RIGHT,
            adjust::FLIP_Y | adjust::SLIDE_X,
        ),
        PopupPlacement::Above => (
            anchor::TOP_LEFT,
            gravity::TOP_RIGHT,
            adjust::FLIP_Y | adjust::SLIDE_X,
        ),
        PopupPlacement::Left => (
            anchor::TOP_LEFT,
            gravity::BOTTOM_LEFT,
            adjust::FLIP_X | adjust::SLIDE_Y,
        ),
        PopupPlacement::Right => (
            anchor::TOP_RIGHT,
            gravity::BOTTOM_RIGHT,
            adjust::FLIP_X | adjust::SLIDE_Y,
        ),
    }
}

/// The `xdg_wm_base` version that brought `xdg_popup.reposition`.
pub(super) const REPOSITION_SINCE: u32 = 3;

/// The token a reposition is sent with. The compositor echoes it in
/// `repositioned`, for a client that has several in flight to tell them
/// apart; the configure after it is what this backend reads, so one constant
/// serves.
const REPOSITION_TOKEN: u32 = 0;

/// A positioner for a popup of `size` against `anchor`, placed by
/// `placement`, or null if the compositor would not make one.
///
/// # Safety
///
/// `wm_base` must be a live `xdg_wm_base`. The caller destroys the
/// positioner once the request that takes it has been sent.
unsafe fn positioner(
    wm_base: *mut WlProxy,
    size: LogicalSize,
    anchor_rect: LogicalRect,
    placement: PopupPlacement,
) -> *mut WlProxy {
    let (anchor, gravity, adjustment) = positioner_of(placement);
    let LogicalRect {
        origin,
        size: extent_of,
    } = anchor_rect;
    // SAFETY: the caller guarantees a live `xdg_wm_base`, and every request
    // after the first goes to the positioner it made, checked non-null.
    unsafe {
        let positioner = xdg_wm_base::create_positioner(wm_base);
        if positioner.is_null() {
            return positioner;
        }
        xdg_positioner::set_size(positioner, extent(size.width), extent(size.height));
        xdg_positioner::set_anchor_rect(
            positioner,
            round_logical(origin.x),
            round_logical(origin.y),
            extent(extent_of.width),
            extent(extent_of.height),
        );
        xdg_positioner::set_anchor(positioner, anchor);
        xdg_positioner::set_gravity(positioner, gravity);
        xdg_positioner::set_constraint_adjustment(positioner, adjustment);
        positioner
    }
}

/// A logical length as a positioner extent: rounded, and at least one, which
/// the protocol requires of every size and anchor rectangle it takes.
fn extent(value: f64) -> i32 {
    round_logical(value).max(1)
}

impl WaylandShell {
    /// Creates a popup window; see the [module docs](self).
    ///
    /// # Errors
    ///
    /// [`ShellError::InvalidWindow`] for a parent that is not a live window,
    /// [`ShellError::NeedsUserInteraction`] for a grab no seat has an input
    /// serial for, and [`ShellError::WindowCreation`] if a global is gone or a
    /// request fails.
    pub(super) fn create_popup(
        &mut self,
        desc: &WindowDesc<'_>,
        popup: &PopupDesc,
    ) -> Result<WindowId, ShellError> {
        let parent = self.window(popup.parent)?;
        let (parent_xdg, parent_layer) = (parent.xdg_surface, parent.layer_surface);
        if self.conn.compositor.is_null() || self.conn.wm_base.is_null() {
            return Err(ShellError::WindowCreation(
                "the compositor withdrew wl_compositor or xdg_wm_base".to_string(),
            ));
        }
        let grab = match popup.input {
            PopupInput::Grab => Some(self.grab_serial()?),
            PopupInput::Passthrough | PopupInput::Pointer => None,
        };
        let wm_base = self.conn.wm_base;
        let compositor = self.conn.compositor;
        // SAFETY: both globals were just checked non-null; every object is
        // created by the request before it and checked before use, and a
        // failure releases what this function made, innermost first.
        let (surface, xdg, popup_proxy) = unsafe {
            let surface = wl_compositor::create_surface(compositor);
            if surface.is_null() {
                return Err(ShellError::WindowCreation(
                    "wl_compositor.create_surface failed".to_string(),
                ));
            }
            let positioner = positioner(wm_base, desc.size, popup.anchor, popup.placement);
            if positioner.is_null() {
                self.conn.release(surface, wl_surface::destroy);
                return Err(ShellError::WindowCreation(
                    "xdg_wm_base.create_positioner failed".to_string(),
                ));
            }

            let xdg = xdg_wm_base::get_xdg_surface(wm_base, surface);
            if xdg.is_null() {
                xdg_positioner::destroy(positioner);
                self.conn.release(surface, wl_surface::destroy);
                return Err(ShellError::WindowCreation(
                    "xdg_wm_base.get_xdg_surface failed".to_string(),
                ));
            }
            // A panel's popup has no xdg parent; the layer surface adopts it
            // below, before the first commit.
            let xdg_parent = if parent_layer.is_null() {
                parent_xdg
            } else {
                ptr::null_mut()
            };
            let popup_proxy = xdg_surface::get_popup(xdg, xdg_parent, positioner);
            xdg_positioner::destroy(positioner);
            if popup_proxy.is_null() {
                self.conn.release(xdg, xdg_surface::destroy);
                self.conn.release(surface, wl_surface::destroy);
                return Err(ShellError::WindowCreation(
                    "xdg_surface.get_popup failed".to_string(),
                ));
            }
            if !parent_layer.is_null() {
                zwlr_layer_surface_v1::get_popup(parent_layer, popup_proxy);
            }
            (surface, xdg, popup_proxy)
        };
        self.conn.watch(surface, ObjectKind::Surface);
        self.conn.watch(xdg, ObjectKind::XdgSurface);
        self.conn.watch(popup_proxy, ObjectKind::XdgPopup);

        // SAFETY: every manager is null (checked inside the helper) or a live
        // global, and `surface` was just created.
        let (fractional_scale, viewport) = unsafe {
            (
                optional(self.conn.fractional_scale_manager, |manager| {
                    super::wp_fractional_scale_manager_v1::get_fractional_scale(manager, surface)
                }),
                optional(self.conn.viewporter, |manager| {
                    super::wp_viewporter::get_viewport(manager, surface)
                }),
            )
        };
        self.conn
            .watch(fractional_scale, ObjectKind::FractionalScale);
        self.conn.watch(viewport, ObjectKind::Ignored);

        // SAFETY: the proxies are live; the region is created, applied and
        // destroyed here, and the surface keeps a copy of it.
        unsafe {
            if popup.input == PopupInput::Passthrough {
                let region = wl_compositor::create_region(compositor);
                if !region.is_null() {
                    wl_surface::set_input_region(surface, region);
                    wl_region::destroy(region);
                }
            }
            if let Some((seat, serial)) = grab {
                xdg_popup::grab(popup_proxy, seat, serial);
            }
            // The initial commit with no buffer, which asks for the first
            // configure, as for every other role.
            wl_surface::commit(surface);
        }
        self.conn.flush();

        let handle: WindowId = self
            .windows
            .insert(WlWindow {
                surface,
                xdg_surface: xdg,
                toplevel: ptr::null_mut(),
                layer_surface: ptr::null_mut(),
                popup: popup_proxy,
                parent: Some(popup.parent),
                placement: Some(popup.placement),
                popup_input: Some(popup.input),
                panel: None,
                decoration: ptr::null_mut(),
                fractional_scale,
                viewport,
                frame_callback: ptr::null_mut(),
                title: desc.title.to_string(),
                requested_size: desc.size,
                requested_mode: desc.mode,
                requested_constraints: desc.constraints,
                resizable: desc.resizable,
                configuration: None::<WindowConfiguration>,
                pending: PendingConfigure::default(),
                outputs: Vec::new(),
                scale: 1,
                preferred_scale: None,
                pointer_mode: PointerMode::Free,
                constraints: Vec::new(),
                cursor: None,
                accept_drops: desc.accept_drops,
                transparent: desc.transparent,
                text_input: super::text_input::Field::default(),
                focused: false,
                visible: desc.visible,
                close_pending: false,
            })
            .cast();
        self.opened(handle);
        Ok(handle)
    }

    /// Moves a popup against `anchor` and gives it `size`, with
    /// `xdg_popup.reposition` and a positioner built as at creation. The
    /// compositor answers with a configure, which brings the new size as any
    /// other.
    ///
    /// # Errors
    ///
    /// [`ShellError::Unsupported`] when the compositor's `xdg_wm_base` is older
    /// than [`REPOSITION_SINCE`], [`ShellError::InvalidWindow`] for a stale
    /// handle, [`ShellError::InvalidDescriptor`] for a window that is not a
    /// popup, and [`ShellError::Backend`] if the positioner cannot be made.
    pub(super) fn move_popup(
        &mut self,
        window: WindowId,
        anchor: LogicalRect,
        size: LogicalSize,
    ) -> Result<(), ShellError> {
        if !self.caps.contains(crate::ShellCaps::POPUP_REPOSITION) {
            return Err(ShellError::Unsupported {
                backend: ShellBackend::Wayland,
                what: "repositioning a popup, which needs xdg_wm_base 3",
            });
        }
        let state = self.window(window)?;
        let (proxy, placement) = match state.placement {
            Some(placement) if !state.popup.is_null() => (state.popup, placement),
            _ => return Err(crate::role::not_a_popup(window)),
        };
        // SAFETY: `wm_base` is live, since the capability needs it; the
        // positioner is checked, sent with the popup's own live proxy, and
        // destroyed once the request has taken it.
        unsafe {
            let positioner = positioner(self.conn.wm_base, size, anchor, placement);
            if positioner.is_null() {
                return Err(ShellError::Backend(
                    "xdg_wm_base.create_positioner failed".to_string(),
                ));
            }
            xdg_popup::reposition(proxy, positioner, REPOSITION_TOKEN);
            xdg_positioner::destroy(positioner);
        }
        self.conn.flush();
        self.window_mut(window)?.requested_size = size;
        Ok(())
    }

    /// The seat and serial a popup grab quotes: the seat with the latest input
    /// event, which is what the compositor checks the grab against.
    ///
    /// # Errors
    ///
    /// [`ShellError::NeedsUserInteraction`] when no seat has had any input,
    /// since a compositor refuses a grab with no serial behind it.
    fn grab_serial(&self) -> Result<(*mut WlProxy, u32), ShellError> {
        self.seats
            .iter()
            .filter(|seat| seat.last_serial != 0)
            .max_by_key(|seat| seat.last_serial)
            .map(|seat| (seat.proxy, seat.last_serial))
            .ok_or(ShellError::NeedsUserInteraction {
                backend: ShellBackend::Wayland,
                what: "a popup that grabs the pointer and the keyboard",
            })
    }
}

#[cfg(test)]
mod tests {
    use super::{extent, positioner_of};
    use crate::PopupPlacement;

    #[test]
    fn each_placement_is_an_anchor_a_gravity_and_room_to_flip() {
        // xdg-shell's own numbers: anchor and gravity share one table, and the
        // adjustments are bits.
        assert_eq!(positioner_of(PopupPlacement::Below), (6, 8, 8 | 1));
        assert_eq!(positioner_of(PopupPlacement::Above), (5, 7, 8 | 1));
        assert_eq!(positioner_of(PopupPlacement::Left), (5, 6, 4 | 2));
        assert_eq!(positioner_of(PopupPlacement::Right), (7, 8, 4 | 2));
    }

    #[test]
    fn a_positioner_extent_is_never_zero() {
        assert_eq!(extent(0.0), 1);
        assert_eq!(extent(199.6), 200);
        assert_eq!(extent(-3.0), 1);
    }
}
