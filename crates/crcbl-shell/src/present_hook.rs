//! Telling the shell a frame is about to reach a window.
//!
//! A swapchain's present is the window system's business, not the shell's: on
//! Wayland the driver attaches the image and commits the surface itself, and
//! the shell never sees it. Some surface state belongs with a particular frame
//! — a new size, its viewport, the acknowledgement of a configure — and has to
//! be on the wire ahead of the commit that carries that frame, and not ahead
//! of the one before it. A [`PresentHook`] is how the presenter says when that
//! moment is: [`will_present`](PresentHook::will_present) with the size the
//! frame was drawn at, just before the present, and
//! [`presented`](PresentHook::presented) once the present went through or
//! [`present_failed`](PresentHook::present_failed) if it did not.
//!
//! A presenter keeps its hook for as long as it presents to the window, and
//! drops it when it stops: on Wayland, the window is then sent what it owed,
//! and treated from there as one whose frames never came through a hook.
//!
//! The presenter calls it, not the program: `crcbl-app`'s `Presenter` takes
//! the hook when it opens on a window and calls it around every present. A
//! program presenting through its own swapchain, from the window's
//! [`surface_target`](crate::Shell::surface_target), calls it the same way.
//!
//! A hook holds no borrow of the shell, so the presenter keeps it for as long
//! as it lives. Calling it after the window or the shell is gone does nothing.
//! It is `Send` and `Sync`, so a presenter holding one may move, but it is
//! called on the thread that runs the shell; see
//! [`Shell::present_hook`](crate::Shell::present_hook).

use core::fmt;
use std::sync::Arc;

use crate::PhysicalSize;

/// What a backend does around a present. One implementation per backend that
/// has a use for it; [`PresentHook`] is the type every caller sees.
pub(crate) trait PresentSignal: Send + Sync + fmt::Debug {
    /// A frame drawn at `size` is about to be presented to the window.
    fn will_present(&self, size: PhysicalSize);

    /// The present announced by the last `will_present` reached the window.
    fn presented(&self);

    /// The present announced by the last `will_present` failed, and nothing
    /// it carried reached the window with it.
    fn present_failed(&self);
}

/// The calls a presenter makes around each present to one window — see the
/// [module docs](self).
///
/// From [`Shell::present_hook`](crate::Shell::present_hook). Cheap to clone,
/// `Send` and `Sync`, so a presenter that holds one stays both.
#[derive(Clone)]
pub struct PresentHook {
    signal: Arc<dyn PresentSignal>,
}

impl PresentHook {
    pub(crate) fn new(signal: Arc<dyn PresentSignal>) -> Self {
        Self { signal }
    }

    /// Says a frame drawn at `size`, in device pixels, is about to be
    /// presented: call it after the frame's submit and immediately before the
    /// present, on the thread that runs the shell — see
    /// [`Shell::present_hook`](crate::Shell::present_hook).
    pub fn will_present(&self, size: PhysicalSize) {
        self.signal.will_present(size);
    }

    /// Says the present [`will_present`](Self::will_present) announced went
    /// through.
    pub fn presented(&self) {
        self.signal.presented();
    }

    /// Says the present [`will_present`](Self::will_present) announced
    /// failed — a swapchain out of date, a lost device — so nothing waits for
    /// it to go through.
    pub fn present_failed(&self) {
        self.signal.present_failed();
    }
}

impl fmt::Debug for PresentHook {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_tuple("PresentHook").field(&self.signal).finish()
    }
}
