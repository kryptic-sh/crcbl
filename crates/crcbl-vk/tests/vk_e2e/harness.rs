//! The fixture the rest of the suite opens with — not a test module.
//!
//! [`Headless`] is an offscreen surface, a device and a swapchain-shaped image
//! ring. Its device asks for the suite's interesting features
//! (`GPU_DRIVEN`, `TIMESTAMP_QUERY`, `DEBUG_MARKERS`, `PRESENT_FEEDBACK`,
//! `PRESENT_TIMING`) as *optional* and requires none of them, so the same
//! fixture opens on radv and on lavapipe and the tests branch on what actually
//! came back. [`Headless::readback`] polls against a deadline rather than
//! sleeping, per `docs/plan/12-testing.md`.
//!
//! **Callers must end with [`Headless::finish`] rather than dropping the
//! fixture.** It tears down in the order `crcbl-hal`'s obligation 2 requires
//! and then asserts the validation report is clean — which is the assertion
//! most of this suite's tests are actually resting on, and a test that drops
//! the fixture instead never asks the layer what it saw.
//!
//! Dropping it is nevertheless what a *failing* test does, because a panic
//! never reaches the last line of the test — so `Drop` prints, on that path
//! only, the two things `finish` would have asserted on: what
//! `Device::wait_idle` says and what the validation layer recorded. That is a
//! diagnostic, not a check; a green run still owes the assertions in `finish`.
//!
//! [`instance`] prints a `vk e2e: adapter …` line per adapter, and that line is
//! load-bearing outside this file: `tests/run-vk-e2e.sh` greps the first one to
//! report which driver really ran, and exits non-zero when the suite never
//! printed one. Rewording it turns a green suite into a failed harness run.

use core::ops::Deref;
use std::time::{Duration, Instant};

use crcbl_core::SurfaceTarget;
use crcbl_hal::{
    CompositeAlpha, Device, DeviceDesc, Features, Format, HalError, Instance, PresentMode,
    ReadbackDesc, ReadbackState, SwapchainDesc,
};
use crcbl_vk::{OpenError, VkInstance};

/// The size every offscreen test renders at. Small enough that lavapipe is
/// fast, large enough that a row-pitch mistake shows up.
pub(crate) const EXTENT: (u32, u32) = (64, 48);

/// A distinctive clear colour. Chosen so every channel differs and none is 0 or
/// 1: a channel-swap or an sRGB round-trip bug is then visible in the bytes.
pub(crate) const CLEAR: [f32; 4] = [0.25, 0.5, 0.75, 1.0];

/// The byte every readback destination is filled with before it is polled.
///
/// Deliberately not `0`. A frame that legitimately rendered black reads back as
/// zeroes, and so does a destination no copy ever reached — which is the
/// ambiguity that made `[0, 0, 0, 255]` on `vk e2e (lavapipe, windows)`
/// undiagnosable: the failing assertion could not say whether the frame was
/// empty or the readback had never landed. This value is neither a plausible
/// pixel nor a plausible counter, so a byte of it surviving into an assertion
/// is evidence that nothing was copied over it.
pub(crate) const POISON: u8 = 0xA5;

/// A readback destination of `len` bytes, filled with [`POISON`].
pub(crate) fn poisoned(len: usize) -> Vec<u8> {
    vec![POISON; len]
}

/// How long [`Headless::readback`] polls before it declares the copy lost.
const READBACK_DEADLINE: Duration = Duration::from_secs(30);

/// Opens an instance, or explains why the suite cannot run.
///
/// A missing loader is a hard failure here, not a skip: this suite is only ever
/// started by the harness, which has already established that Vulkan is present.
///
/// It is also where the process gets a logger. Every `VkInstance` in this suite
/// is opened through here, and without a sink the `log` facade drops the
/// validation messenger's [`log::error!`] and [`log::warn!`] on the floor —
/// which is how a suite whose premise is "the layer is being listened to"
/// produced a 66,836-line Windows job log with no VUID text anywhere in it.
pub(crate) fn instance() -> VkInstance {
    // `crcbl-core` already owns a `log::Log` and its `CRCBL_LOG` filtering, so
    // this is the engine's own sink rather than a second one written for the
    // tests. Idempotent, and it loses gracefully to a logger already installed.
    //
    // Before `VkInstance::open`, because the instance-creation-time messenger
    // reports a bad `VkInstanceCreateInfo` from inside the call below.
    crcbl_core::log::init_logging();
    match VkInstance::open() {
        Ok(instance) => {
            let (major, minor, patch) = instance.loader_version();
            eprintln!("vk e2e: loader {major}.{minor}.{patch}");
            for adapter in instance.adapters() {
                eprintln!(
                    "vk e2e: adapter {:?} ({:?}) driver {:?} geometry {:?} binding {:?} \
                     lighting {:?}",
                    adapter.name,
                    adapter.device_type,
                    adapter.driver,
                    adapter.caps.geometry_path(),
                    adapter.caps.binding_model(),
                    adapter.caps.lighting_path()
                );
            }
            instance
        }
        Err(error @ OpenError::NoLoader(_)) => panic!(
            "the harness starts this suite only when Vulkan is available, so a \
             missing loader here is a real failure: {error}"
        ),
        Err(error) => panic!("could not open the Vulkan backend: {error}"),
    }
}

/// The fixture's device, in a slot [`Headless::finish`] can empty.
///
/// `Headless` has a [`Drop`] impl — the failure path's diagnostics — and a type
/// with one cannot have a field moved out of it. `finish` must destroy the
/// device *before* it reads the validation report, because a teardown bug's
/// messages are emitted during `vkDestroyDevice` and would otherwise be read
/// too early, so the device it drops comes out of here instead. The [`Deref`]
/// is what keeps every `headless.device.…` in the suite reading as it did.
pub(crate) struct DeviceSlot(Option<Box<dyn Device>>);

impl DeviceSlot {
    /// Fills the slot with a freshly opened device.
    pub(crate) fn new(device: Box<dyn Device>) -> Self {
        Self(Some(device))
    }

    /// Destroys the device, so a validation report read *after* this call
    /// includes whatever `vkDestroyDevice` had to say. Emptying the slot is the
    /// point: a second call does nothing, and every later deref panics with the
    /// message above rather than reaching a dangling handle.
    pub(crate) fn destroy(&mut self) {
        drop(self.0.take());
    }
}

impl Deref for DeviceSlot {
    type Target = Box<dyn Device>;

    fn deref(&self) -> &Self::Target {
        self.0
            .as_ref()
            .expect("`Headless::finish` has already destroyed this fixture's device")
    }
}

/// An offscreen surface, a device, and a swapchain-shaped image ring.
pub(crate) struct Headless {
    pub(crate) instance: VkInstance,
    pub(crate) device: DeviceSlot,
    pub(crate) surface: crcbl_hal::SurfaceHandle,
    pub(crate) swapchain: crcbl_hal::SwapchainHandle,
    pub(crate) queue: crcbl_hal::QueueHandle,
    pub(crate) format: Format,
}

impl Headless {
    pub(crate) fn open() -> Self {
        Self::open_with(EXTENT, 2)
    }

    pub(crate) fn open_with(extent: (u32, u32), image_count: u32) -> Self {
        let instance = instance();
        let adapter = instance.adapters().remove(0);

        let target = SurfaceTarget::Offscreen;
        // SAFETY: `Offscreen` names no platform object at all, so there is
        // nothing to outlive the surface. `destroy` below tears the swapchain
        // down before the surface regardless, which is the general rule.
        let surface = unsafe { instance.create_surface(&target) }.expect("offscreen always works");

        let caps = instance
            .surface_caps(surface, adapter.id)
            .expect("the offscreen ring reports its own caps");
        assert_eq!(
            caps.current_extent, None,
            "an offscreen ring has no opinion about its size, exactly like Wayland"
        );
        let format = caps.preferred_format().expect("some format is offered");

        let device = instance
            .create_device(&DeviceDesc {
                label: Some("vk e2e"),
                adapter: adapter.id,
                // Never `GPU_DRIVEN`: lavapipe and radv genuinely differ, and this
                // suite exists partly to find out how.
                required_features: Features::empty(),
                // `PRESENT_FEEDBACK` is asked for here even though nothing
                // offscreen can be paced by a display: asking is what enables
                // `VK_KHR_present_id` and `VK_KHR_present_wait`, and the
                // offscreen ring then has to keep them *out* of its own path.
                // Not asking would leave that guard untested on every driver.
                //
                // `PRESENT_TIMING` is here for exactly the same reason, and it
                // buys more: asking is what makes `vkCreateDevice` negotiate
                // the whole four-extension chain `VK_EXT_present_timing`
                // depends on, so a mistake in that chain — a name never
                // enabled, a feature bit never granted — fails device creation
                // in this suite instead of on a user's machine. The offscreen
                // ring then has to keep the query out of its own path, which is
                // the guard the test beside the present-wait one covers.
                optional_features: Features::GPU_DRIVEN
                    | Features::TIMESTAMP_QUERY
                    | Features::DEBUG_MARKERS
                    | Features::PRESENT_FEEDBACK
                    | Features::PRESENT_TIMING,
                compatible_surface: Some(surface),
            })
            .expect("a device opens");
        let queue = device
            .queue(crcbl_hal::QueueKind::Graphics)
            .expect("a graphics queue always exists");

        let swapchain = device
            .create_swapchain(&SwapchainDesc {
                label: Some("vk e2e ring"),
                surface,
                format,
                extent,
                image_count,
                present_mode: PresentMode::Fifo,
                composite_alpha: CompositeAlpha::Opaque,
            })
            .expect("the ring is created");

        Self {
            instance,
            device: DeviceSlot::new(device),
            surface,
            swapchain,
            queue,
            format,
        }
    }

    /// Reads a whole image back into `out`, polling with a deadline.
    pub(crate) fn readback(&self, staging: crcbl_hal::BufferHandle, size: u64, out: &mut [u8]) {
        let device = self.device.as_ref();
        let readback = device
            .request_readback(&ReadbackDesc {
                label: Some("vk e2e pixels"),
                buffer: staging,
                offset: 0,
                size,
                after: None,
            })
            .expect("a readback request");
        // Poll with a deadline, never a fixed sleep — `docs/plan/12-testing.md`.
        let started = Instant::now();
        let deadline = started + READBACK_DEADLINE;
        loop {
            match device
                .poll_readback(readback, out)
                .expect("the readback did not fail")
            {
                ReadbackState::Ready => break,
                ReadbackState::Pending => assert!(
                    Instant::now() < deadline,
                    "the {size}-byte readback was still Pending after {:?}, past the \
                     {READBACK_DEADLINE:?} this polls for. Nothing was copied into the \
                     destination, so it still holds {POISON:#04x} and every byte a \
                     caller reads out of it is this harness's fill and not a frame.",
                    started.elapsed(),
                ),
            }
            std::thread::yield_now();
        }
        device.destroy_readback(readback);
    }

    /// Tears down in the order `crcbl-hal`'s obligation 2 requires, then
    /// asserts the layer saw nothing.
    pub(crate) fn finish(mut self) {
        self.device.wait_idle().expect("idle");
        self.device.destroy_swapchain(self.swapchain);
        self.instance.destroy_surface(self.surface);
        self.device.destroy();
        self.instance.validation_report().assert_clean();
    }
}

impl Drop for Headless {
    /// The failure path's copy of what [`Headless::finish`] reports.
    ///
    /// `finish` is the last line of a test, so a test that panics never reaches
    /// it and the device's verdict and the layer's are both discarded on
    /// exactly the runs that go red. This prints them instead, and prints
    /// nothing at all otherwise — the success path already asserts on both.
    ///
    /// **Nothing here may panic.** It runs while the thread is already
    /// unwinding, and a second panic aborts the process, which would destroy
    /// the output this exists to produce. So the device is taken out of the
    /// slot with `Option::as_ref` rather than through [`Deref`]'s `expect`, and
    /// every error is matched rather than unwrapped. What is left is
    /// `eprintln!` on a closed stderr — the case `crcbl_vk::debug`'s messenger
    /// guards against, and one the test harness owning this pipe rules out.
    fn drop(&mut self) {
        if !std::thread::panicking() {
            return;
        }
        eprintln!(
            "vk e2e: the fixture was dropped by a panicking test, so `finish` never ran. \
             What it can still see:"
        );
        match self.device.0.as_ref() {
            // The distinction the flake needs: a lost device is a driver-side
            // failure that makes every other symptom downstream noise, and it
            // is otherwise invisible because nothing in a failing test asks.
            Some(device) => match device.wait_idle() {
                Ok(()) => eprintln!(
                    "vk e2e:   wait_idle: Ok — the device is alive and idle, so the \
                     submission completed and the failure is in what it produced"
                ),
                Err(HalError::DeviceLost(detail)) => eprintln!(
                    "vk e2e:   wait_idle: VK_ERROR_DEVICE_LOST ({detail}) — the device \
                     died, so nothing this test read back means anything"
                ),
                Err(error) => eprintln!("vk e2e:   wait_idle: {error}"),
            },
            None => {
                eprintln!("vk e2e:   wait_idle: not asked, `finish` already destroyed the device")
            }
        }
        let report = self.instance.validation_report();
        eprintln!(
            "vk e2e:   validation: enabled={}, {} error(s), {} warning(s)",
            report.enabled, report.errors, report.warnings
        );
        if !report.messages.is_empty() {
            eprintln!("{}", report.summary());
        }
    }
}
