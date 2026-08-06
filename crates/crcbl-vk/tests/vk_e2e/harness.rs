use crcbl_core::SurfaceTarget;
use crcbl_hal::{
    CompositeAlpha, Device, DeviceDesc, Features, Format, Instance, PresentMode, ReadbackDesc,
    ReadbackState, SwapchainDesc,
};
use crcbl_vk::{OpenError, VkInstance};

/// The size every offscreen test renders at. Small enough that lavapipe is
/// fast, large enough that a row-pitch mistake shows up.
pub(crate) const EXTENT: (u32, u32) = (64, 48);

/// A distinctive clear colour. Chosen so every channel differs and none is 0 or
/// 1: a channel-swap or an sRGB round-trip bug is then visible in the bytes.
pub(crate) const CLEAR: [f32; 4] = [0.25, 0.5, 0.75, 1.0];

/// Opens an instance, or explains why the suite cannot run.
///
/// A missing loader is a hard failure here, not a skip: this suite is only ever
/// started by the harness, which has already established that Vulkan is present.
pub(crate) fn instance() -> VkInstance {
    match VkInstance::open() {
        Ok(instance) => {
            let (major, minor, patch) = instance.loader_version();
            eprintln!("vk e2e: loader {major}.{minor}.{patch}");
            for adapter in instance.adapters() {
                eprintln!(
                    "vk e2e: adapter {:?} ({:?}) driver {:?} tier {:?}",
                    adapter.name,
                    adapter.device_type,
                    adapter.driver,
                    adapter.caps.tier()
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

/// An offscreen surface, a device, and a swapchain-shaped image ring.
pub(crate) struct Headless {
    pub(crate) instance: VkInstance,
    pub(crate) device: Box<dyn Device>,
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
                // Never `TIER_A`: lavapipe and radv genuinely differ, and this
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
                optional_features: Features::TIER_A
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
            device,
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
        let deadline = std::time::Instant::now() + std::time::Duration::from_secs(30);
        loop {
            match device
                .poll_readback(readback, out)
                .expect("the readback did not fail")
            {
                ReadbackState::Ready => break,
                ReadbackState::Pending => assert!(
                    std::time::Instant::now() < deadline,
                    "the readback never completed"
                ),
            }
            std::thread::yield_now();
        }
        device.destroy_readback(readback);
    }

    /// Tears down in the order `crcbl-hal`'s obligation 2 requires, then
    /// asserts the layer saw nothing.
    pub(crate) fn finish(self) {
        self.device.wait_idle().expect("idle");
        self.device.destroy_swapchain(self.swapchain);
        self.instance.destroy_surface(self.surface);
        drop(self.device);
        self.instance.validation_report().assert_clean();
    }
}
