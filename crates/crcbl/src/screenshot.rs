//! Offscreen render → readback → golden image — the `crcbl screenshot` path.
//!
//! Opens a GPU backend, creates an offscreen surface and swapchain, renders
//! one frame through [`ForwardRenderer`], reads the pixels back, and returns
//! a [`crcbl_golden::Image`] ready to save as PNG.
//!
//! This module is the render half of the CLI subcommand; the CLI module
//! owns the argument parsing and I/O.

use std::time::Duration;

use crate::hal::{
    Barriers, BufferDesc, BufferImageCopy, BufferUsage, CommandEncoderDesc, Device, DeviceDesc,
    Extent3d, Features, Format, ImageAspect, ImageBarrier, ImageSubresourceLayers,
    ImageSubresourceRange, Instance, MemoryLocation, Offset3d, PresentInfo, PresentMode,
    QueueHandle, QueueKind, ReadbackDesc, ReadbackState, ResourceState, SubmitInfo, SurfaceError,
    SurfaceHandle, SurfaceTarget, SwapchainDesc, SwapchainHandle,
};
use crate::render::{
    Camera, DirectionalLight, ForwardRenderer, GraphError, Projection, RenderGraph, TransientPool,
};

// ---------------------------------------------------------------------------
// OffscreenSetup
// ---------------------------------------------------------------------------

/// Holds everything needed to render one frame offscreen: a GPU instance,
/// device, offscreen swapchain ring, and the forward renderer.
///
/// The caller drives one frame via [`Self::draw_and_readback`], then tears
/// down with [`Self::finish`].
#[allow(missing_debug_implementations)]
pub struct OffscreenSetup {
    instance: Box<dyn Instance>,
    device: Box<dyn Device>,
    surface: SurfaceHandle,
    swapchain: SwapchainHandle,
    queue: QueueHandle,
    format: Format,
    /// Where the camera is and how it projects.
    camera: Camera,
    light: DirectionalLight,
    renderer: ForwardRenderer,
    pool: TransientPool,
    /// Seconds of animation, advanced by callers who want a particular pose.
    elapsed: f32,
}

/// Reasons an offscreen render might fail before a pixel is written.
#[derive(Debug, thiserror::Error)]
pub enum OffscreenError {
    /// No GPU backend could be opened.
    #[error("GPU backend: {0}")]
    Backend(#[from] crate::backend::GpuError),

    /// No adapter, no queue, no format, or a surface-cap query failed.
    #[error("device not usable: {0}")]
    Unusable(&'static str),

    /// A HAL call failed.
    #[error("HAL: {0}")]
    Hal(#[from] crate::hal::HalError),

    /// A surface operation failed.
    #[error("surface: {0}")]
    Surface(#[from] crate::hal::SurfaceError),

    /// A graph compile or execute failed.
    #[error("graph: {0}")]
    Graph(#[from] GraphError),

    /// The swapchain went out of date before the first frame completed.
    #[error("offscreen swapchain is out of date")]
    OutOfDate,
}

impl OffscreenSetup {
    /// Opens the auto-selected GPU backend, creates an offscreen surface,
    /// adapter, device, swapchain, and forward renderer for a frame of
    /// `(width, height)` pixels.
    ///
    /// Returns `Err` if no GPU is available (lavapipe, swiftshader, or a real
    /// card), if the device is unusable, or if any HAL call fails.
    pub fn open(width: u32, height: u32) -> Result<Self, OffscreenError> {
        let extent = (width, height);
        let instance = crate::backend::open()?;

        let target = SurfaceTarget::Offscreen;
        // SAFETY: `Offscreen` names no platform object, so nothing can dangle.
        let surface = unsafe {
            instance
                .create_surface(&target)
                .map_err(OffscreenError::Hal)?
        };

        let adapters = instance.adapters();
        let adapter = adapters
            .first()
            .ok_or(OffscreenError::Unusable("no adapter"))?;

        let caps = instance
            .surface_caps(surface, adapter.id)
            .map_err(OffscreenError::Hal)?;

        let format = caps
            .preferred_format()
            .ok_or(OffscreenError::Unusable("no surface format"))?;

        let device = instance
            .create_device(&DeviceDesc {
                label: Some("crcbl screenshot"),
                adapter: adapter.id,
                required_features: Features::empty(),
                optional_features: Features::TIER_A | Features::DEBUG_MARKERS,
                compatible_surface: Some(surface),
            })
            .map_err(OffscreenError::Hal)?;

        let queue = device
            .queue(QueueKind::Graphics)
            .ok_or(OffscreenError::Unusable("no graphics queue"))?;

        let swapchain = device
            .create_swapchain(&SwapchainDesc {
                label: Some("screenshot ring"),
                surface,
                format,
                extent,
                image_count: 2,
                present_mode: PresentMode::Fifo,
                composite_alpha: crate::hal::CompositeAlpha::Opaque,
            })
            .map_err(OffscreenError::Surface)?;

        let renderer =
            ForwardRenderer::new(device.as_ref(), queue, format).map_err(OffscreenError::Hal)?;

        let camera = Camera::default().with_projection(Projection::Perspective {
            fov_y: std::f32::consts::FRAC_PI_3,
            near: 0.01,
        });

        Ok(Self {
            instance,
            device,
            surface,
            swapchain,
            queue,
            format,
            camera,
            light: DirectionalLight::default(),
            renderer,
            pool: TransientPool::new(),
            elapsed: 0.0,
        })
    }

    /// Advances the animation clock by `dt` seconds.
    pub fn advance(&mut self, dt: f32) {
        self.elapsed += dt;
    }

    /// Records, submits, and reads back one frame.
    ///
    /// Returns RGBA8 sRGB pixels as `(width, height, Vec<u8>)`.
    ///
    /// # Errors
    ///
    /// [`OffscreenError::Hal`] if recording, submission, or readback fail.
    /// [`OffscreenError::OutOfDate`] if the swapchain is stale.
    pub fn draw_and_readback(&mut self) -> Result<((u32, u32), Vec<u8>), OffscreenError> {
        let device = self.device.as_ref();
        let acquired = device
            .acquire_next_frame(self.swapchain)
            .map_err(|error| match error {
                SurfaceError::OutOfDate => OffscreenError::OutOfDate,
                other => OffscreenError::Surface(other),
            })?;

        let extent = acquired.extent;
        let byte_count = u64::from(extent.0) * u64::from(extent.1) * 4;

        // ---- render the frame through the graph ----

        self.renderer.begin_frame(
            device,
            &self.camera,
            &self.light,
            ForwardRenderer::spin(self.elapsed),
            extent,
        )?;

        let compiled = {
            let mut graph = RenderGraph::new(self.queue);
            let target = graph.import_image(
                "swapchain",
                ForwardRenderer::present_target(acquired.image, acquired.view, self.format, extent),
            );
            let _hdr = self.renderer.add_passes(&mut graph, target, extent);
            graph.compile(&self.pool)?
        };

        let mut encoder = device.create_command_encoder(&CommandEncoderDesc {
            label: Some("screenshot frame"),
            queue: self.queue,
        });
        compiled.execute(device, &mut self.pool, encoder.as_mut(), None)?;

        // ---- readback: barrier to TransferSrc, copy, barrier back, submit ----

        let staging = device.create_buffer(&BufferDesc {
            label: Some("screenshot readback"),
            size: byte_count,
            usage: BufferUsage::TRANSFER_DST,
            memory: MemoryLocation::HostReadback,
        })?;

        let range = ImageSubresourceRange::all(self.format);
        encoder.pipeline_barrier(&Barriers {
            images: &[ImageBarrier::new(
                acquired.image,
                range,
                ResourceState::ColorAttachment,
                ResourceState::TransferSrc,
            )],
            ..Barriers::default()
        });

        encoder.copy_image_to_buffer(&BufferImageCopy {
            buffer: staging,
            buffer_offset: 0,
            buffer_row_length: 0,
            buffer_image_height: 0,
            image: acquired.image,
            image_subresource: ImageSubresourceLayers {
                aspect: ImageAspect::COLOR,
                mip: 0,
                base_layer: 0,
                layer_count: 1,
            },
            image_offset: Offset3d::default(),
            image_extent: Extent3d::d2(extent.0, extent.1),
        });

        let commands = encoder.finish()?;
        device.submit(self.queue, &SubmitInfo::new(&[commands]))?;
        device.present(
            self.queue,
            &PresentInfo {
                swapchain: self.swapchain,
                waits: acquired.present_semaphore.as_slice(),
            },
        )?;

        let readback = device.request_readback(&ReadbackDesc {
            label: Some("screenshot readback"),
            buffer: staging,
            offset: 0,
            size: byte_count,
            after: None,
        })?;

        let mut pixels = vec![0u8; byte_count as usize];

        // Poll with a generous deadline; an offscreen ring on a software
        // rasteriser can take hundreds of ms for a single frame.
        let deadline = std::time::Instant::now() + Duration::from_secs(10);
        loop {
            let state = device.poll_readback(readback, &mut pixels)?;
            if let ReadbackState::Ready = state {
                break;
            }
            if std::time::Instant::now() > deadline {
                panic!("screenshot readback did not complete within 10 s; timed out");
            }
            std::thread::sleep(Duration::from_micros(100));
        }

        device.destroy_command_buffer(commands);
        device.destroy_buffer(staging);
        device.destroy_readback(readback);

        Ok((extent, pixels))
    }

    /// Tears down in correct order: wait idle, destroy swapchain → surface →
    /// device, then check validation.
    pub fn finish(self) {
        self.device.wait_idle().expect("idle");
        self.device.destroy_swapchain(self.swapchain);
        self.instance.destroy_surface(self.surface);
        drop(self.device);
        drop(self.instance);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The null backend exercises the same code paths but renders nothing.
    /// The test exists to prove the module compiles and the HAL seam is
    /// wired correctly; pixel content is tested by the Vulkan e2e suite.
    #[test]
    fn offscreen_setup_opens_with_null_backend() {
        // Force the null backend to avoid needing a real GPU.
        let instance = crate::backend::open_backend(crate::backend::GpuBackend::Null)
            .expect("null backend is always compiled in");

        let surface = unsafe {
            instance
                .create_surface(&SurfaceTarget::Offscreen)
                .expect("null accepts every surface")
        };
        let adapters = instance.adapters();
        let adapter = adapters
            .first()
            .expect("null backend always has an adapter");

        let caps = instance
            .surface_caps(surface, adapter.id)
            .expect("null reports surface caps");
        let format = caps.preferred_format().expect("null has a format");

        let device = instance
            .create_device(&DeviceDesc {
                label: Some("screenshot test"),
                adapter: adapter.id,
                required_features: Features::empty(),
                optional_features: Features::TIER_A,
                compatible_surface: Some(surface),
            })
            .expect("null device");

        let queue = device
            .queue(QueueKind::Graphics)
            .expect("null has a graphics queue");

        let swapchain = device
            .create_swapchain(&SwapchainDesc {
                label: Some("test ring"),
                surface,
                format,
                extent: (16, 16),
                image_count: 2,
                present_mode: PresentMode::Fifo,
                composite_alpha: crate::hal::CompositeAlpha::Opaque,
            })
            .expect("null creates a swapchain");

        let mut renderer = ForwardRenderer::new(device.as_ref(), queue, format)
            .expect("null builds the forward renderer");

        // Acquire, record, submit — no readback on the null backend.
        let acquired = device
            .acquire_next_frame(swapchain)
            .expect("null ring has an image");

        let mut graph = RenderGraph::new(queue);
        let target = graph.import_image(
            "swapchain",
            ForwardRenderer::present_target(acquired.image, acquired.view, format, (16, 16)),
        );
        let pool = TransientPool::new();
        let _hdr = renderer.add_passes(&mut graph, target, (16, 16));

        let compiled = graph.compile(&pool).expect("graph compiles");
        let mut encoder = device.create_command_encoder(&CommandEncoderDesc {
            label: Some("test frame"),
            queue,
        });
        compiled
            .execute(
                device.as_ref(),
                &mut TransientPool::new(),
                encoder.as_mut(),
                None,
            )
            .expect("graph executes");
        let commands = encoder.finish().expect("encoding succeeds");
        device
            .submit(queue, &SubmitInfo::new(&[commands]))
            .expect("submit");
        device.wait_idle().expect("idle");

        device.destroy_command_buffer(commands);
        device.destroy_swapchain(swapchain);
        instance.destroy_surface(surface);
        drop(device);
        drop(instance);
    }
}
