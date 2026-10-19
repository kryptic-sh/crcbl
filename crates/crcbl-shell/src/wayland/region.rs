//! A window's input region, as `wl_surface.set_input_region`.
//!
//! The region is surface-local and logical, so each device-pixel rectangle
//! [`Shell::set_input_region`](crate::Shell::set_input_region) gives is
//! scaled by the window's factor and grown outward to whole units, as the
//! caret rectangle `text-input-v3` takes is. Like every other piece of surface
//! state it is pending until the surface is committed, and this commits it at
//! once rather than waiting for the renderer's next present: a window that is
//! idle, drawing nothing, would otherwise keep its old region.
//!
//! A null region is the protocol's own "the whole surface", which is what
//! `None` asks for. A passthrough popup's empty region is set at creation (see
//! [`popup`](super::popup)) and is left alone.

use crate::{PhysicalRect, PopupInput, ShellError, WindowId};
use core::ptr;

use super::WaylandShell;
use super::protocol::wayland::{wl_compositor, wl_region, wl_surface};
use super::text_input::logical_rect;

impl WaylandShell {
    /// [`Shell::set_input_region`](crate::Shell::set_input_region).
    pub(super) fn apply_input_region(
        &mut self,
        window: WindowId,
        region: Option<&[PhysicalRect]>,
    ) -> Result<(), ShellError> {
        let state = self.window(window)?;
        if state.popup_input == Some(PopupInput::Passthrough) {
            return Ok(());
        }
        let (surface, scale) = (state.surface, state.scale_factor());
        let compositor = self.conn.compositor;
        let built = match region {
            None => ptr::null_mut(),
            Some(_) if compositor.is_null() => {
                return Err(ShellError::Backend(
                    "the compositor withdrew wl_compositor, so no region can be made".to_string(),
                ));
            }
            Some(rects) => {
                // SAFETY: the compositor was just checked non-null and is a
                // live global.
                let built = unsafe { wl_compositor::create_region(compositor) };
                if built.is_null() {
                    return Err(ShellError::Backend(
                        "wl_compositor.create_region failed".to_string(),
                    ));
                }
                for &rect in rects {
                    let [x, y, width, height] = logical_rect(rect, scale);
                    if width > 0 && height > 0 {
                        // SAFETY: the region was just made; every argument is
                        // a plain integer.
                        unsafe { wl_region::add(built, x, y, width, height) };
                    }
                }
                built
            }
        };
        // SAFETY: the surface is live, and the region is null or the one made
        // above. The surface keeps a copy of a region, so it is destroyed
        // straight after; the commit is what makes it current.
        unsafe {
            wl_surface::set_input_region(surface, built);
            if !built.is_null() {
                wl_region::destroy(built);
            }
            wl_surface::commit(surface);
        }
        self.conn.flush();
        Ok(())
    }
}
