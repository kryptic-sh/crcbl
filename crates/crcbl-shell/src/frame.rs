//! Frame requests answered at the next pump, for the backends whose window
//! system does not pace them.
//!
//! [`Shell::request_frame`](crate::Shell::request_frame) promises one
//! [`ShellEvent::FrameReady`] per request, shared by every request made before
//! it, and none for a window destroyed first. Where
//! [`ShellCaps::FRAME_CALLBACK`](crate::ShellCaps::FRAME_CALLBACK) is clear
//! nothing outside the process answers, so each such backend keeps a
//! [`FrameRequests`] and turns it into events as its `pump` delivers. One type
//! rather than a copy per backend, so the coalescing and the destroyed-window
//! rule cannot drift apart between them.

use crate::{ShellEvent, WindowId};

/// The windows that asked for a frame since the last delivery.
#[derive(Debug, Default)]
pub(crate) struct FrameRequests {
    windows: Vec<WindowId>,
}

impl FrameRequests {
    /// Records a request. A window that already has one keeps just that one.
    pub(crate) fn request(&mut self, window: WindowId) {
        if !self.windows.contains(&window) {
            self.windows.push(window);
        }
    }

    /// Whether any request awaits delivery, which a wait must not sleep
    /// through.
    pub(crate) fn is_pending(&self) -> bool {
        !self.windows.is_empty()
    }

    /// Whether `window` has a request awaiting delivery.
    pub(crate) fn contains(&self, window: WindowId) -> bool {
        self.windows.contains(&window)
    }

    /// Takes every request, answering those whose window `is_live` still
    /// names.
    ///
    /// Called once per pump, before the sink runs: a request the sink makes
    /// belongs to the next pump, and a window destroyed since its request is
    /// already gone from the backend's table, so it is dropped here rather
    /// than answered with a stale handle.
    pub(crate) fn take_due(
        &mut self,
        is_live: impl Fn(WindowId) -> bool,
    ) -> impl Iterator<Item = ShellEvent> {
        core::mem::take(&mut self.windows)
            .into_iter()
            .filter(move |window| is_live(*window))
            .map(|window| ShellEvent::FrameReady { window })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crcbl_core::Pool;

    fn windows() -> (WindowId, WindowId) {
        let mut pool: Pool<u8> = Pool::new();
        (pool.insert(0).cast(), pool.insert(1).cast())
    }

    #[test]
    fn requests_before_the_delivery_share_one_answer() {
        let (first, second) = windows();
        let mut frames = FrameRequests::default();
        assert!(!frames.is_pending());
        frames.request(first);
        frames.request(second);
        frames.request(first);
        assert!(frames.is_pending());
        assert!(frames.contains(first));
        let due: Vec<ShellEvent> = frames.take_due(|_| true).collect();
        assert_eq!(
            due,
            [
                ShellEvent::FrameReady { window: first },
                ShellEvent::FrameReady { window: second },
            ]
        );
        assert!(!frames.is_pending(), "a delivery answers every request");
        assert!(!frames.contains(first));
        assert_eq!(frames.take_due(|_| true).count(), 0);
    }

    #[test]
    fn a_window_gone_before_the_delivery_gets_no_answer() {
        let (gone, live) = windows();
        let mut frames = FrameRequests::default();
        frames.request(gone);
        frames.request(live);
        let due: Vec<ShellEvent> = frames.take_due(|window| window == live).collect();
        assert_eq!(due, [ShellEvent::FrameReady { window: live }]);
        assert!(!frames.is_pending(), "the dead window's request is dropped");
    }
}
