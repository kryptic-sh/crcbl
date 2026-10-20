//! Which GPU the compositor composites on, through `zwp_linux_dmabuf_v1`.
//!
//! The shell presents nothing through dmabuf — the Vulkan driver does that —
//! and binds the manager only to read the default feedback's `main_device`:
//! the DRM device a buffer reaches without a copy between GPUs. It travels to
//! the renderer in [`SurfaceTarget::Wayland`](crcbl_core::SurfaceTarget)'s
//! `main_device`, where a backend matches it to an adapter for
//! `Instance::display_adapter`. On a hybrid laptop that is how the engine
//! learns the panel is the iGPU's.
//!
//! The default feedback rather than one per surface: it exists by the end of
//! `open`'s second round trip, so a window's target carries the answer from
//! the moment the window exists. The protocol lets a surface's own feedback
//! name a different main device; reading that is not built.

use super::fd;
use super::protocol::linux_dmabuf::{zwp_linux_dmabuf_feedback_v1, zwp_linux_dmabuf_v1};
use super::{ObjectKind, RawEvent, WaylandShell};

/// `get_default_feedback` arrived in version 4, and version 6 stops sending
/// `main_device` in favour of tranche flags; 4 is the one this module reads.
pub(super) const DMABUF_VERSION: u32 = 4;

/// The `dev_t` a `main_device` array holds, or `None` for an array that is not
/// exactly one. The protocol sends a native `dev_t`, which is 64 bits on every
/// Linux libc.
pub(super) fn decode_device(bytes: &[u8]) -> Option<u64> {
    let bytes: [u8; 8] = bytes.try_into().ok()?;
    Some(u64::from_ne_bytes(bytes))
}

/// The one feedback event this backend reads, copied out of the dispatcher.
/// `format_table`'s descriptor is closed there, since nothing maps it; the
/// tranche events say nothing [`decode_device`]'s reader needs.
pub(super) fn decode_event(
    proxy: usize,
    event: zwp_linux_dmabuf_feedback_v1::Event<'_>,
) -> Option<RawEvent> {
    match event {
        zwp_linux_dmabuf_feedback_v1::Event::MainDevice { device } => {
            Some(RawEvent::DmabufMainDevice {
                feedback: proxy,
                device: decode_device(device),
            })
        }
        zwp_linux_dmabuf_feedback_v1::Event::Done => Some(RawEvent::DmabufDone { feedback: proxy }),
        zwp_linux_dmabuf_feedback_v1::Event::FormatTable { fd, .. } => {
            fd::close(fd);
            None
        }
        _ => None,
    }
}

impl WaylandShell {
    /// Asks the just-bound manager for its default feedback, whose first batch
    /// arrives on the next round trip.
    pub(super) fn add_dmabuf_feedback(&mut self) {
        let manager = self.conn.dmabuf;
        if manager.is_null() || !self.dmabuf_feedback.is_null() {
            return;
        }
        // SAFETY: the manager is live on this connection and was bound at
        // version 4, where `get_default_feedback` exists.
        let feedback = unsafe { zwp_linux_dmabuf_v1::get_default_feedback(manager) };
        if feedback.is_null() {
            return;
        }
        self.conn.watch(feedback, ObjectKind::DmabufFeedback);
        self.dmabuf_feedback = feedback;
    }

    /// Records a feedback batch's main device, which takes effect at its
    /// `done` so a batch is seen whole, as the protocol asks.
    pub(super) fn dmabuf_main_device(&mut self, feedback: usize, device: Option<u64>) {
        if feedback == self.dmabuf_feedback as usize {
            self.pending_main_device = device;
        }
    }

    /// Ends a feedback batch.
    pub(super) fn dmabuf_done(&mut self, feedback: usize) {
        if feedback != self.dmabuf_feedback as usize {
            return;
        }
        if self.main_device != self.pending_main_device {
            crcbl_core::log::debug!(
                "crcbl-shell: the compositor composites on DRM device {:?}",
                self.pending_main_device
            );
        }
        self.main_device = self.pending_main_device;
    }

    /// Destroys the feedback, for a manager that is going away or a shell
    /// being dropped. The device it named is forgotten with it: a target read
    /// afterwards says `None` rather than a device nothing vouches for.
    pub(super) fn remove_dmabuf_feedback(&mut self) {
        let feedback = core::mem::replace(&mut self.dmabuf_feedback, core::ptr::null_mut());
        // SAFETY: a null proxy is skipped, a live one is this connection's,
        // and `destroy` is its own destructor.
        unsafe {
            self.conn
                .release(feedback, zwp_linux_dmabuf_feedback_v1::destroy);
        }
        self.main_device = None;
        self.pending_main_device = None;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_main_device_is_exactly_one_native_dev_t() {
        // `makedev(226, 128)`, a DRM render node.
        let render = 0xe280_u64;
        assert_eq!(decode_device(&render.to_ne_bytes()), Some(render));
        assert_eq!(decode_device(&[]), None, "an empty array names nothing");
        assert_eq!(
            decode_device(&[0; 4]),
            None,
            "a 32-bit dev_t is not Linux's"
        );
        assert_eq!(decode_device(&[0; 16]), None, "two devices are not one");
    }
}
