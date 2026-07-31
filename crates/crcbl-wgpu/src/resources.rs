//! Resource pools for the wgpu backend.
//!
//! Each HAL resource type gets its own [`crcbl_core::Pool`] mapping handles to
//! wgpu objects. The pools are behind `Mutex` for interior mutability (the HAL
//! trait methods take `&self`).

use crate::cell::{Lock, Shared};

use crcbl_core::Pool;

/// A wgpu surface, stored in the instance's surface pool so Device swapchain
/// methods can reach it. `'static` because the caller (`create_surface_unsafe`)
/// guarantees the window handles outlive the surface.
pub struct SurfaceSlot {
    pub surface: wgpu::Surface<'static>,
    /// Platform tag from the SurfaceTarget that created it.
    #[allow(dead_code)]
    pub platform: &'static str,
}

/// Per-swapchain state. A wgpu "swapchain" is a configured surface plus an
/// acquired-texture ring.
pub struct SwapchainSlot {
    /// ID of the SurfaceSlot in the surface pool. The surface is owned by the
    /// instance; the swapchain holds a key for lookups.
    #[allow(dead_code)]
    pub surface_handle_id: u64,
    /// Surface handle (from `Instance::create_surface`) for tracing.
    pub surface_handle: crcbl_hal::SurfaceHandle,
    /// The last-configured surface description.
    pub config: Option<wgpu::SurfaceConfiguration>,
    /// The currently acquired frame, valid between acquire and present.
    pub acquired: Option<wgpu::SurfaceTexture>,
    /// Swapchain image/texture-view handles allocated on acquire.
    pub frame_image: Option<u64>,
    pub frame_view: Option<u64>,
    /// Extent this swapchain was configured at.
    pub extent: (u32, u32),
    /// Format the swapchain is configured with.
    pub format: crcbl_hal::Format,
    /// Suboptimal flag, carried forward per swapchain module docs.
    pub suboptimal: bool,
}

pub struct CommandBufferSlot {
    pub buffer: Option<wgpu::CommandBuffer>,
    #[allow(dead_code)]
    pub label: String,
}

pub struct SemaphoreSlot {
    pub value: u64,
}

pub struct Pools {
    pub buffers: Lock<Pool<wgpu::Buffer>>,
    pub images: Lock<Pool<wgpu::Texture>>,
    pub image_views: Lock<Pool<wgpu::TextureView>>,
    pub samplers: Lock<Pool<wgpu::Sampler>>,
    pub shader_modules: Lock<Pool<wgpu::ShaderModule>>,
    pub bind_group_layouts: Lock<Pool<wgpu::BindGroupLayout>>,
    pub bind_groups: Lock<Pool<wgpu::BindGroup>>,
    pub pipeline_layouts: Lock<Pool<wgpu::PipelineLayout>>,
    pub graphics_pipelines: Lock<Pool<wgpu::RenderPipeline>>,
    pub compute_pipelines: Lock<Pool<wgpu::ComputePipeline>>,
    pub command_buffers: Shared<Lock<Pool<CommandBufferSlot>>>,
    pub swapchains: Lock<Pool<SwapchainSlot>>,
    pub semaphores: Lock<Pool<SemaphoreSlot>>,
    #[allow(dead_code)]
    pub query_sets: Lock<Pool<()>>,
    /// Shared surface pool, cloned from the instance.
    pub surfaces: Shared<Lock<Pool<SurfaceSlot>>>,
}

impl Pools {
    pub fn new(surfaces: Shared<Lock<Pool<SurfaceSlot>>>) -> Self {
        Self {
            buffers: Lock::new(Pool::new()),
            images: Lock::new(Pool::new()),
            image_views: Lock::new(Pool::new()),
            samplers: Lock::new(Pool::new()),
            shader_modules: Lock::new(Pool::new()),
            bind_group_layouts: Lock::new(Pool::new()),
            bind_groups: Lock::new(Pool::new()),
            pipeline_layouts: Lock::new(Pool::new()),
            graphics_pipelines: Lock::new(Pool::new()),
            compute_pipelines: Lock::new(Pool::new()),
            command_buffers: Shared::new(Lock::new(Pool::new())),
            swapchains: Lock::new(Pool::new()),
            semaphores: Lock::new(Pool::new()),
            query_sets: Lock::new(Pool::new()),
            surfaces,
        }
    }
}
