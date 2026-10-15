//! The AppKit wait's doorbell: an application-defined event posted to
//! `NSApp`'s queue.
//!
//! The wait is `nextEventMatchingMask:untilDate:`, which returns when any event
//! arrives, so the way in from another thread is an event. An
//! `NSEventTypeApplicationDefined` one carries nothing, `sendEvent:` delivers it
//! to nothing, and `-[NSApplication postEvent:atStart:]` is documented as
//! callable from any thread — GLFW's `glfwPostEmptyEvent` is the same call for
//! the same reason.
//!
//! A posted event stays queued until a wait or a pump takes it, so a ring with
//! no wait running ends the next one at once — the pump that takes it records
//! the ring, and [`wait`](super::shell::AppKitShell) returns before sleeping.
//! [`pending`](AppKitWake::pending) makes rings merge as the pipe's and the
//! Win32 event's do: a ring posts an event only when none is queued, and the
//! shell clears the flag as it takes the event, so at most one is ever on the
//! queue. [`WAKE_MARKER`] in the event's `data1` is how the shell tells its
//! own from an application-defined event another library posted, which it
//! dispatches as usual.

use core::ptr;
use core::sync::atomic::{AtomicBool, Ordering};

use super::ffi::{self, Id, NSInteger, NSPoint, NSUInteger, ObjcBool, Pool, Sel, value};
use crate::waker::WakeSignal;

/// `data1` of the event a ring posts: `"crcb"` in ASCII, so it reads as
/// itself in a debugger and no other poster would choose it by accident.
pub(super) const WAKE_MARKER: NSInteger = 0x6372_6362;

/// The doorbell.
#[derive(Debug, Default)]
pub(super) struct AppKitWake {
    /// True from a ring until the shell takes the event it posted.
    pending: AtomicBool,
}

impl AppKitWake {
    /// Records that the shell took the ring's event off the queue, so the next
    /// ring posts another.
    pub(super) fn taken(&self) {
        self.pending.store(false, Ordering::Release);
    }
}

/// Whether `event` is a ring's: application-defined, with [`WAKE_MARKER`].
///
/// # Safety
///
/// `event` is a live `NSEvent`.
pub(super) unsafe fn is_wake(event: Id) -> bool {
    // SAFETY: `type` is an `NSEvent` accessor returning an `NSUInteger`, and
    // `data1` one returning an `NSInteger`, valid for this event type only —
    // which is why it is read second.
    unsafe {
        ffi::msg_usize(event, ffi::sel(c"type")) == value::EVENT_TYPE_APPLICATION_DEFINED
            && ffi::msg_usize(event, ffi::sel(c"data1")) as NSInteger == WAKE_MARKER
    }
}

impl WakeSignal for AppKitWake {
    fn wake(&self) {
        if self.pending.swap(true, Ordering::AcqRel) {
            return;
        }
        // The calling thread may have no pool of its own, and both calls below
        // hand back autoreleased objects.
        let _pool = Pool::push();
        let (Some(app_class), Some(event_class)) =
            (ffi::class(c"NSApplication"), ffi::class(c"NSEvent"))
        else {
            return;
        };
        // SAFETY: `sharedApplication` returns the singleton the shell created
        // on the main thread; the other two are the documented signatures of
        // `+otherEventWithType:…` and `-postEvent:atStart:`, and `postEvent`
        // may be called from any thread.
        unsafe {
            let app = ffi::msg(app_class, ffi::sel(c"sharedApplication"));
            let make: unsafe extern "C" fn(
                Id,
                Sel,
                NSUInteger,
                NSPoint,
                NSUInteger,
                f64,
                NSInteger,
                Id,
                i16,
                NSInteger,
                NSInteger,
            ) -> Id = ffi::msg_send();
            let event = make(
                event_class,
                ffi::sel(c"otherEventWithType:location:modifierFlags:timestamp:windowNumber:context:subtype:data1:data2:"),
                value::EVENT_TYPE_APPLICATION_DEFINED,
                NSPoint::default(),
                0,
                0.0,
                0,
                ptr::null_mut(),
                0,
                WAKE_MARKER,
                0,
            );
            if app.is_null() || event.is_null() {
                // Nothing posted, so nothing will be taken: unring.
                self.taken();
                return;
            }
            let post: unsafe extern "C" fn(Id, Sel, Id, ObjcBool) = ffi::msg_send();
            post(app, ffi::sel(c"postEvent:atStart:"), event, ffi::YES);
        }
    }
}
