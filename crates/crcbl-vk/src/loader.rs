//! One process-wide lock around the Vulkan loader calls that race each other.
//!
//! # Why: a loader bug, worked around here
//!
//! The spec asks for external synchronization per instance and per device,
//! never across them, so two instances on two threads should need nothing.
//! The loader does not hold up its side of that. CI caught it under gdb (run
//! 41714978409: Vulkan loader 1.4.341, Mesa 26.0.8 llvmpipe, the Khronos
//! validation layer on), with `apps/viewer`'s lib tests segfaulting:
//!
//! - one test thread was inside `vkSetDebugUtilsObjectNameEXT` for a buffer it
//!   had just created, and had passed through the validation layer into the
//!   loader's own terminator, where it crashed;
//! - another test thread, on a *different* instance, was inside
//!   `vkEnumeratePhysicalDevices`, where the loader re-scans its drivers and
//!   was `dlclose`-ing libraries at that moment.
//!
//! The name terminator finds the device's driver by walking the loader's global
//! list of instances and their drivers, which is state no single instance owns
//! and which a re-scan rewrites. A test suite has two instances live in one
//! process all the time, so it hits this; an application with one instance
//! never does.
//!
//! # Exclusive and shared
//!
//! [`exclusive`] is for the calls that *change* the loader's global view: the
//! pre-instance queries (the loader answers them by re-reading every driver
//! and layer manifest, and for the extension and version queries by loading
//! and unloading driver or implicit-layer libraries), `vkCreateInstance`,
//! `vkEnumeratePhysicalDevices` and `vkDestroyInstance`. [`shared`] is for the
//! calls that only *read* it, `vkSetDebugUtilsObjectNameEXT`, so naming objects
//! on many devices at once stays concurrent.
//!
//! # Left unlocked
//!
//! Calls that dispatch straight through their handle's own table take nothing,
//! and that is nearly all of Vulkan. That includes the command-buffer label
//! calls in `command`, which sit on the recording hot path: ash loads them
//! through `vkGetDeviceProcAddr` like the name call, but the loader's
//! terminators for them look the driver up through the command buffer's own
//! dispatch table rather than walking the instance list. That is read from the
//! loader's source, not observed, and the gdb capture shows only the name
//! call; a crash with a label call on the stack would overturn it.
//! `vkSetDebugUtilsObjectTagEXT` and the queue label calls would need the same
//! look, and nothing in this crate calls them.
//!
//! # Re-entrancy
//!
//! Each guard wraps one loader call and nothing else, so no other crcbl-vk
//! code runs while it is held. The one thing that can run inside is the debug
//! messenger the instance chains, called back synchronously by the layer or
//! the loader; `debug::messenger_body` only formats, logs through
//! `crcbl_core::log` and records into the sink, and never makes a Vulkan call.
//! That is what keeps a thread from asking for this lock while it already
//! holds it, which would deadlock — `std`'s lock is not re-entrant, for
//! readers either.

use std::sync::{PoisonError, RwLock};

static LOADER: RwLock<()> = RwLock::new(());

/// Runs `call` with no other loader call of either kind in flight.
///
/// The lock guards no data, so a panic inside another holder leaves nothing
/// half-written: poison is ignored, as it is for the crate's other locks.
pub(crate) fn exclusive<R>(call: impl FnOnce() -> R) -> R {
    let _guard = LOADER.write().unwrap_or_else(PoisonError::into_inner);
    call()
}

/// Runs `call` with no [`exclusive`] call in flight; other shared calls may
/// run beside it.
pub(crate) fn shared<R>(call: impl FnOnce() -> R) -> R {
    let _guard = LOADER.read().unwrap_or_else(PoisonError::into_inner);
    call()
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::mpsc;
    use std::time::Duration;

    /// Long enough that a call not waiting on the lock has finished, on any
    /// machine this suite runs on. A too-short wait can only let a broken
    /// guard pass, never fail a working one: a call that does wait never
    /// finishes before the lock is released, however long this is.
    const BLOCKED_FOR: Duration = Duration::from_millis(250);

    /// Runs `call` on another thread while this one holds `hold`'s guard, and
    /// asserts it could not finish until the guard was released.
    fn assert_waits_for<G>(hold: impl FnOnce() -> G, call: impl FnOnce() + Send) {
        let (done, finished) = mpsc::channel();
        std::thread::scope(|scope| {
            let guard = hold();
            scope.spawn(move || {
                call();
                done.send(()).expect("the test thread is still waiting");
            });
            assert_eq!(
                finished.recv_timeout(BLOCKED_FOR),
                Err(mpsc::RecvTimeoutError::Timeout),
                "the call finished while the loader lock was held against it"
            );
            drop(guard);
            finished
                .recv()
                .expect("the call finishes once the lock is released");
        });
    }

    fn hold_exclusive() -> std::sync::RwLockWriteGuard<'static, ()> {
        LOADER.write().unwrap_or_else(PoisonError::into_inner)
    }

    fn hold_shared() -> std::sync::RwLockReadGuard<'static, ()> {
        LOADER.read().unwrap_or_else(PoisonError::into_inner)
    }

    #[test]
    fn a_shared_call_waits_for_an_exclusive_one() {
        assert_waits_for(hold_exclusive, || shared(|| ()));
    }

    #[test]
    fn an_exclusive_call_waits_for_a_shared_one() {
        assert_waits_for(hold_shared, || exclusive(|| ()));
    }

    #[test]
    fn shared_calls_do_not_wait_for_each_other() {
        let _held = hold_shared();
        std::thread::scope(|scope| {
            scope.spawn(|| shared(|| ())).join().expect("no panic");
        });
    }

    /// The guard on `vkSetDebugUtilsObjectNameEXT`, observed through the seam:
    /// a labelled buffer's creation names it, and must wait while an instance
    /// is being created or enumerated elsewhere.
    ///
    /// Ignored: it needs a Vulkan loader and a device.
    #[test]
    #[ignore = "needs a Vulkan loader and a device"]
    fn naming_an_object_waits_for_an_instance_being_opened() {
        use crcbl_hal::{BufferDesc, BufferUsage, DeviceDesc, Features, Instance, MemoryLocation};

        let instance = crate::VkInstance::open().expect("a Vulkan instance");
        let adapter = instance.adapters()[0].id;
        // `DEBUG_MARKERS` is offered exactly when the instance has
        // `VK_EXT_debug_utils`, without which no name is set and nothing waits.
        assert!(
            instance
                .adapter_caps(adapter)
                .expect("a listed adapter describes itself")
                .features
                .contains(Features::DEBUG_MARKERS),
            "without VK_EXT_debug_utils this test could not fail"
        );
        let device = instance
            .create_device(&DeviceDesc::for_adapter(adapter))
            .expect("a device");
        let desc = BufferDesc {
            label: Some("loader lock"),
            size: 256,
            usage: BufferUsage::TRANSFER_DST,
            memory: MemoryLocation::DeviceLocal,
        };
        assert_waits_for(hold_exclusive, || {
            let buffer = device.create_buffer(&desc).expect("a buffer");
            device.destroy_buffer(buffer);
        });
    }

    /// The guard on instance creation and enumeration: opening an instance
    /// must wait while an object is being named elsewhere.
    ///
    /// Ignored: it needs a Vulkan loader.
    #[test]
    #[ignore = "needs a Vulkan loader"]
    fn opening_an_instance_waits_for_an_object_being_named() {
        assert_waits_for(hold_shared, || {
            drop(crate::VkInstance::open().expect("a Vulkan instance"));
        });
    }
}
