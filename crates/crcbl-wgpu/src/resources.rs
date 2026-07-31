//! Resource pools for the wgpu backend.
//!
//! Each HAL resource type gets its own [`crcbl_core::Pool`] mapping handles to
//! wgpu objects. The pools are behind `Mutex` for interior mutability (the HAL
//! trait methods take `&self`).

use std::sync::{Arc, Mutex};

use crcbl_core::Pool;

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
    #[allow(dead_code)]
    pub swapchains: Mutex<Pool<SwapchainSlot>>,
    pub semaphores: Mutex<Pool<SemaphoreSlot>>,
    #[allow(dead_code)]
    pub query_sets: Mutex<Pool<()>>,
}

#[allow(dead_code)]
pub struct CommandBufferSlot {
    pub buffer: Option<wgpu::CommandBuffer>,
    pub label: String,
}

#[allow(dead_code)]
pub struct SwapchainSlot {
    pub surface: wgpu::Surface<'static>,
    pub config: wgpu::SurfaceConfiguration,
    pub texture: Option<wgpu::SurfaceTexture>,
    pub format: crcbl_hal::Format,
    pub extent: (u32, u32),
}

pub struct SemaphoreSlot {
    pub value: u64,
}

impl Pools {
    pub fn new() -> Self {
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
        }
    }
}
