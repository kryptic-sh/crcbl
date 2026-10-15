//! The Win32 wait's doorbell: an auto-reset event in the handle set
//! `MsgWaitForMultipleObjectsEx` waits on.
//!
//! [`wake`](WakeSignal::wake) is `SetEvent`, which any thread may call on a
//! handle it holds. The event is **auto-reset**: the wait that it ends resets
//! it, so a ring is consumed by exactly one wait, any number of rings before
//! that wait are one, and a ring with no wait running is kept for the next.
//! That is [`ShellWaker`](crate::ShellWaker)'s whole contract with no state of
//! this module's own.
//!
//! The handle is an [`OwnedHandle`], which is `Send`, `Sync` and closes itself
//! when the last [`Arc`](std::sync::Arc) holding it goes, so a waker that
//! outlives the shell signals a live event nobody waits on, which is harmless.

use std::io;
use std::os::windows::io::{AsRawHandle as _, FromRawHandle as _, OwnedHandle};

use super::ffi;
use crate::waker::WakeSignal;

/// The doorbell: one auto-reset event.
#[derive(Debug)]
pub(super) struct WakeEvent {
    event: OwnedHandle,
}

impl WakeEvent {
    /// A new, unsignalled event.
    ///
    /// # Errors
    ///
    /// `CreateEventW`'s refusal, as the OS error it left.
    pub(super) fn new() -> io::Result<Self> {
        // SAFETY: null attributes and a null name ask for an unnamed event
        // with default security; FALSE, FALSE is auto-reset and unsignalled.
        let event = unsafe { ffi::CreateEventW(core::ptr::null(), 0, 0, core::ptr::null()) };
        if event.is_null() {
            return Err(io::Error::last_os_error());
        }
        // SAFETY: a handle `CreateEventW` just returned, owned by nothing else.
        let event = unsafe { OwnedHandle::from_raw_handle(event) };
        Ok(Self { event })
    }

    /// The handle to put in the wait's handle set.
    pub(super) fn handle(&self) -> ffi::Handle {
        self.event.as_raw_handle()
    }
}

impl WakeSignal for WakeEvent {
    fn wake(&self) {
        // SAFETY: `handle` is the live event this value owns.
        if unsafe { ffi::SetEvent(self.handle()) } == 0 {
            crcbl_core::log::warn!(
                "SetEvent on the shell's wake event failed: {}",
                io::Error::last_os_error()
            );
        }
    }
}
