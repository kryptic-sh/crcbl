//! A window's [`PresentHook`](crate::PresentHook): what the swapchain is
//! about to commit.
//!
//! The present is the driver's `wl_surface.commit`, which this backend never
//! sees; [`Shell::present_hook`](crate::Shell::present_hook) is how the
//! presenter tells it one is coming, on this connection and ahead of the
//! commit on the wire. The size of each frame announced is recorded as the
//! size shown.
//!
//! **Nothing reads that size yet, and nothing is sent from here.** Surface
//! state that should ride a frame drawn at its size — a panel's own new size,
//! a resizing configure's acknowledgement and viewport, a new buffer scale —
//! is still sent as before, by the request or event that produced it, and
//! committed with whatever commit comes next. Holding it for the frame it
//! belongs to is the work this hook is the seam for, and is not done.

use std::sync::{Mutex, PoisonError};

use crate::PhysicalSize;
use crate::present_hook::PresentSignal;

/// One window's half of its [`PresentHook`](crate::PresentHook), shared
/// between the window and every hook handed out for it.
#[derive(Debug, Default)]
pub(super) struct WlPresent {
    /// The size of the last frame announced, or `None` before the first: a
    /// window whose presents never come through a hook.
    shown: Mutex<Option<PhysicalSize>>,
}

impl PresentSignal for WlPresent {
    fn will_present(&self, size: PhysicalSize) {
        *self.shown.lock().unwrap_or_else(PoisonError::into_inner) = Some(size);
    }

    /// Nothing waits on a present's outcome yet; see the module docs.
    fn presented(&self) {}
}
