//! Resource pools for the wgpu backend.
//!
//! Each HAL resource type gets its own [`crcbl_core::Pool`] mapping handles to
//! wgpu objects. The pools are behind `Mutex` for interior mutability (the HAL
//! trait methods take `&self`).

use std::sync::{Arc, Mutex};

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
    pub buffers: Mutex<Pool<wgpu::Buffer>>,
    pub images: Mutex<Pool<wgpu::Texture>>,
    pub image_views: Mutex<Pool<wgpu::TextureView>>,
    pub samplers: Mutex<Pool<wgpu::Sampler>>,
    pub shader_modules: Mutex<Pool<wgpu::ShaderModule>>,
    pub bind_group_layouts: Mutex<Pool<wgpu::BindGroupLayout>>,
    pub bind_groups: Mutex<Pool<wgpu::BindGroup>>,
    pub pipeline_layouts: Mutex<Pool<wgpu::PipelineLayout>>,
    pub graphics_pipelines: Mutex<Pool<wgpu::RenderPipeline>>,
    pub compute_pipelines: Mutex<Pool<wgpu::ComputePipeline>>,
    pub command_buffers: Arc<Mutex<Pool<CommandBufferSlot>>>,
    pub swapchains: Mutex<Pool<SwapchainSlot>>,
    pub semaphores: Mutex<Pool<SemaphoreSlot>>,
    #[allow(dead_code)]
    pub query_sets: Mutex<Pool<()>>,
    /// Shared surface pool, cloned from the instance.
    pub surfaces: Arc<Mutex<Pool<SurfaceSlot>>>,
}

impl Pools {
    pub fn new(surfaces: Arc<Mutex<Pool<SurfaceSlot>>>) -> Self {
        Self {
            buffers: Mutex::new(Pool::new()),
            images: Mutex::new(Pool::new()),
            image_views: Mutex::new(Pool::new()),
            samplers: Mutex::new(Pool::new()),
            shader_modules: Mutex::new(Pool::new()),
            bind_group_layouts: Mutex::new(Pool::new()),
            bind_groups: Mutex::new(Pool::new()),
            pipeline_layouts: Mutex::new(Pool::new()),
            graphics_pipelines: Mutex::new(Pool::new()),
            compute_pipelines: Mutex::new(Pool::new()),
            command_buffers: Arc::new(Mutex::new(Pool::new())),
            swapchains: Mutex::new(Pool::new()),
            semaphores: Mutex::new(Pool::new()),
            query_sets: Mutex::new(Pool::new()),
            surfaces,
        }
    }
}
