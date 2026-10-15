//! Waking a blocked [`wait_events`](crate::Shell::wait_events) from another
//! thread.
//!
//! A program whose work arrives off the loop thread — a D-Bus reply, a socket,
//! a timer thread, a finished download — needs the loop to notice it while the
//! loop is asleep in [`wait_events`](crate::Shell::wait_events). A
//! [`ShellWaker`] is the handle that does it: cloned and sent to any thread,
//! where [`wake`](ShellWaker::wake) makes the current wait return, or the next
//! one return at once if none is running.
//!
//! **A wake carries nothing.** It is a doorbell, not a message: the program
//! puts its data in its own channel and then rings, and the loop, once
//! [`wait_events`](crate::Shell::wait_events) has returned, drains that
//! channel. That keeps every backend's mechanism the same size — a byte on a
//! pipe, a signalled event, one posted event — and no event type grows a
//! variant for data the shell never looks at.
//!
//! **Wakes coalesce.** Any number of wakes between two waits end at most the
//! next one or two of them early; none is ever lost, and none piles up. A loop
//! that wakes, drains its channel and waits again therefore never misses work
//! sent while it was draining.

use core::fmt;
use std::sync::Arc;

/// What a backend's wake mechanism does when rung. One implementation per
/// backend; [`ShellWaker`] is the type every caller sees.
pub(crate) trait WakeSignal: Send + Sync + fmt::Debug {
    /// Makes the shell's current or next wait return. Must not block, and must
    /// be callable from any thread, any number of times.
    fn wake(&self);
}

/// Wakes one shell's [`wait_events`](crate::Shell::wait_events) from any
/// thread — see the [module docs](self).
///
/// From [`Shell::waker`](crate::Shell::waker). Cheap to clone, `Send` and
/// `Sync`, and valid for as long as anyone holds it: a wake after the shell is
/// gone does nothing.
#[derive(Clone)]
pub struct ShellWaker {
    signal: Arc<dyn WakeSignal>,
}

impl ShellWaker {
    pub(crate) fn new(signal: Arc<dyn WakeSignal>) -> Self {
        Self { signal }
    }

    /// Makes the shell's current wait return, or its next one return at once.
    ///
    /// Never blocks, and is safe to call from any thread at any rate.
    pub fn wake(&self) {
        self.signal.wake();
    }
}

/// A backend's [`waker`](crate::Shell::waker): the doorbell in `slot`, made
/// by `make` on the first ask and shared by every handle after.
///
/// `None`, logged, if `make` fails — the process out of descriptors or
/// handles — and asked again next time, since that can pass.
#[cfg_attr(
    not(any(desktop_unix, target_os = "macos", target_os = "windows")),
    allow(dead_code, reason = "only the native window-system backends make one")
)]
pub(crate) fn lazy_waker<S: WakeSignal + 'static>(
    slot: &mut Option<Arc<S>>,
    backend: &str,
    make: impl FnOnce() -> std::io::Result<S>,
) -> Option<ShellWaker> {
    if slot.is_none() {
        match make() {
            Ok(signal) => *slot = Some(Arc::new(signal)),
            Err(error) => {
                crcbl_core::log::error!("no wake signal for the {backend} shell: {error}");
                return None;
            }
        }
    }
    let signal = Arc::clone(slot.as_ref()?);
    Some(ShellWaker::new(signal))
}

impl fmt::Debug for ShellWaker {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_tuple("ShellWaker").field(&self.signal).finish()
    }
}
