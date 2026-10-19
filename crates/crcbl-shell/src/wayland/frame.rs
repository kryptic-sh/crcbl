//! Frame pacing, as `wl_surface.frame`.
//!
//! [`Shell::request_frame`](crate::Shell::request_frame) asks the compositor
//! for a `wl_callback` on the window's surface, and its `done` is
//! [`ShellEvent::FrameReady`]. A compositor
//! sends `done` when it wants the surface's next frame, and not while the
//! surface is on no output it is drawing — an output turned off for power
//! saving above all — which is what makes the event worth waiting for there.
//!
//! The request is double-buffered surface state: it joins the next
//! `wl_surface.commit`, and nothing answers it until one comes. That commit is
//! normally the swapchain's — a present attaches the image and commits the
//! surface, on this same connection, so a request sent before the present is
//! ahead of its commit on the wire — though one this backend makes to
//! acknowledge a configure or set an input region carries it too, if it comes
//! first. This never commits for the request itself: a commit with no new
//! image gives the compositor nothing to draw, and the loop the request paces
//! presents straight after making it.
//!
//! One callback per window. A second request before the first is answered
//! shares it, rather than asking again, so the window gets one `FrameReady`
//! however many requests the loop made. The callback dies with the window (see
//! `WaylandShell::destroy_objects`), and a `done` already on its way for it is
//! dropped by the dispatcher, which no longer knows the proxy.

use crate::{ShellError, ShellEvent, WindowId};

use super::protocol::wayland::wl_surface;
use super::{ObjectKind, WaylandShell, WlProxy};

impl WaylandShell {
    /// [`Shell::request_frame`](crate::Shell::request_frame).
    pub(super) fn request_frame_callback(&mut self, window: WindowId) -> Result<(), ShellError> {
        let state = self.window(window)?;
        if !state.frame_callback.is_null() {
            return Ok(());
        }
        // SAFETY: the surface is live for as long as its window is.
        let callback = unsafe { wl_surface::frame(state.surface) };
        if callback.is_null() {
            return Err(ShellError::Backend("wl_surface.frame failed".to_string()));
        }
        self.conn.watch(callback, ObjectKind::FrameCallback);
        self.window_mut(window)?.frame_callback = callback;
        self.conn.flush();
        Ok(())
    }

    /// A `wl_callback.done`: the window it was asked for may present.
    ///
    /// `done` is the callback's destructor event, so the compositor has
    /// already forgotten the object and this only frees the proxy.
    pub(super) fn frame_done(&mut self, callback: usize) {
        let Some(window) = self.window_by_proxy(callback, |w| w.frame_callback as usize) else {
            return;
        };
        let Ok(state) = self.window_mut(window) else {
            return;
        };
        let proxy: *mut WlProxy =
            core::mem::replace(&mut state.frame_callback, core::ptr::null_mut());
        self.conn.destroy(proxy);
        self.queue.push_back(ShellEvent::FrameReady { window });
    }
}
