//! `impl Instance for WebGpuInstance` — adapters, surfaces, the device request.

use crcbl_core::SurfaceTarget;
use crcbl_hal::{
    AdapterId, AdapterInfo, BackendKind, CompositeAlpha, DeviceDesc, Format, HalError, Instance,
    PendingDevice, PresentMode, SurfaceCaps, SurfaceHandle,
};

use crate::device::DeviceProbe;

use super::channel::{HandlePool, SharedChannel};
use super::device::WebGpuPendingDevice;

/// The opened instance: the adapters the browser granted, and the channel every
/// object it makes encodes through.
///
/// Built by [`WebGpuInstanceOpen`](super::open::WebGpuInstanceOpen) once the
/// enumeration is answered, so [`adapters`](Instance::adapters) is a settled
/// `Vec`, not a query — WebGPU grants one adapter or none, and this is that
/// list.
#[derive(Debug)]
pub struct WebGpuInstance {
    channel: SharedChannel,
    adapters: Vec<AdapterInfo>,
    pool: HandlePool,
}

impl WebGpuInstance {
    /// Assemble an instance from a settled enumeration. Called by the open
    /// future; not a public entry point, since the adapters must already be the
    /// browser's answer.
    pub(crate) fn new(
        channel: SharedChannel,
        adapters: Vec<AdapterInfo>,
        pool: HandlePool,
    ) -> Self {
        Self {
            channel,
            adapters,
            pool,
        }
    }

    /// A clone of the channel this instance encodes through, for feeding replies
    /// in a test.
    #[must_use]
    pub fn channel(&self) -> SharedChannel {
        self.channel.clone()
    }

    /// [`request_device`](Instance::request_device), keeping the concrete type
    /// so a caller (and a test) can read the request's waiting sequence off the
    /// [`WebGpuPendingDevice`] it returns.
    ///
    /// # Errors
    ///
    /// [`HalError::NoSuchAdapter`] for an adapter this instance never
    /// enumerated — decided now, per the seam — or [`HalError::Backend`] if the
    /// channel would not take the request.
    pub(crate) fn open_device(
        &self,
        desc: &DeviceDesc<'_>,
    ) -> Result<WebGpuPendingDevice, HalError> {
        // Everything decidable without the browser is decided here: an unknown
        // adapter is an `Err` from this call, not a deferred failure. The
        // features cross unfiltered — the replayer holds the WebGPU vocabulary
        // and is where a required bit it cannot satisfy is refused.
        self.adapters
            .iter()
            .find(|adapter| adapter.id == desc.adapter)
            .ok_or(HalError::NoSuchAdapter(desc.adapter.0))?;

        let probe = self
            .channel
            .with(|channel| DeviceProbe::request(channel, desc))
            .ok_or_else(|| {
                HalError::Backend(
                    "the WebGPU stream channel would not accept the device request".to_string(),
                )
            })?;

        Ok(WebGpuPendingDevice::new(
            self.channel.clone(),
            probe,
            self.pool.clone(),
        ))
    }
}

impl Instance for WebGpuInstance {
    fn backend(&self) -> BackendKind {
        BackendKind::WebGpu
    }

    fn adapters(&self) -> Vec<AdapterInfo> {
        self.adapters.clone()
    }

    unsafe fn create_surface(&self, target: &SurfaceTarget) -> Result<SurfaceHandle, HalError> {
        // Only `Web { canvas_id }` is reachable in a browser. The four pointer
        // variants carry `NonNull`s to platform objects, and a pointer sent as
        // an integer is the one thing this seam refuses outright — the encoder
        // takes a `u32`, not a `SurfaceTarget`, so the refusal has to land here,
        // where the target is still whole. `Offscreen` gets its own command in
        // a later slice.
        let SurfaceTarget::Web { canvas_id } = *target else {
            return Err(HalError::Unsupported {
                backend: BackendKind::WebGpu,
                what: "only a Web canvas surface is reachable on the WebGPU stream; \
                       the pointer-carrying targets never cross the wasm boundary",
            });
        };
        let surface: SurfaceHandle = self.pool.alloc();
        self.channel
            .with(|channel| channel.encode(|stream| stream.create_surface(surface, canvas_id)));
        Ok(surface)
    }

    fn destroy_surface(&self, surface: SurfaceHandle) {
        self.channel
            .with(|channel| channel.encode(|stream| stream.destroy_surface(surface)));
    }

    fn surface_caps(
        &self,
        _surface: SurfaceHandle,
        _adapter: AdapterId,
    ) -> Result<SurfaceCaps, HalError> {
        // Synthesised locally and synchronously, with no round trip — the one
        // reply-less answer among the calls that could defer. `crcbl::engine`'s
        // open calls `create_surface` and then this on the next line with no
        // frame between them, so a query over the reply channel could not answer
        // in time, and an `Err` meaning "not yet" would send a caller doing
        // adapter selection to an adapter a browser does not have. The constant
        // is not a placeholder: both formats are valid
        // `GPUCanvasConfiguration.format` values, `fifo` is the only present
        // mode a canvas offers, and a browser double-buffers on its own — so
        // this is what the canvas actually accepts. Preferring one format over
        // the other is a later optimisation; returning both is correct now.
        Ok(SurfaceCaps {
            formats: vec![Format::Bgra8Unorm, Format::Rgba8Unorm],
            present_modes: vec![PresentMode::Fifo],
            composite_alpha: vec![CompositeAlpha::Opaque, CompositeAlpha::PreMultiplied],
            min_image_count: 2,
            max_image_count: 2,
            current_extent: None,
        })
    }

    fn request_device(&self, desc: &DeviceDesc<'_>) -> Result<Box<dyn PendingDevice>, HalError> {
        self.open_device(desc)
            .map(|pending| Box::new(pending) as Box<dyn PendingDevice>)
    }
}
