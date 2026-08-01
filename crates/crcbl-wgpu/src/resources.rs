//! Resource pools for the wgpu backend.
//!
//! Each HAL resource type gets its own [`crcbl_core::Pool`] mapping handles to
//! wgpu objects. The pools are behind `Mutex` for interior mutability (the HAL
//! trait methods take `&self`).

use crate::cell::{Lock, Shared};

use crcbl_core::Pool;
use crcbl_hal::{ImageHandle, ImageViewHandle, MemoryLocation, SemaphoreKind};

/// A wgpu surface, stored in the instance's surface pool so Device swapchain
/// methods can reach it. `'static` because the caller (`create_surface_unsafe`)
/// guarantees the window handles outlive the surface.
pub struct SurfaceSlot {
    pub surface: wgpu::Surface<'static>,
    /// Platform tag from the SurfaceTarget that created it, for logs.
    pub platform: &'static str,
}

/// A buffer plus the two facts the seam validates writes against.
///
/// `crcbl-vk` keeps the same pair for the same reason: `write_buffer`'s
/// contract is `InvalidDescriptor` for an out-of-range write or a buffer that
/// is not host-visible, and neither is answerable from a `wgpu::Buffer` alone.
pub struct BufferSlot {
    pub buffer: wgpu::Buffer,
    pub size: u64,
    pub memory: MemoryLocation,
}

/// Per-swapchain state. A wgpu "swapchain" is a configured surface plus an
/// acquired-texture ring.
pub struct SwapchainSlot {
    /// Surface handle (from `Instance::create_surface`) the swapchain is
    /// configured on.
    pub surface_handle: crcbl_hal::SurfaceHandle,
    /// The last-configured surface description.
    pub config: Option<wgpu::SurfaceConfiguration>,
    /// The currently acquired frame, valid between acquire and present.
    pub acquired: Option<wgpu::SurfaceTexture>,
    /// Handles the current acquire put in the image and view pools.
    ///
    /// Removed on the next acquire and on present: a swapchain texture is
    /// re-acquired every frame, so leaving them in would leak two pool slots
    /// per frame for the life of the process.
    pub frame_image: Option<ImageHandle>,
    pub frame_view: Option<ImageViewHandle>,
    /// Extent this swapchain was configured at.
    pub extent: (u32, u32),
    /// Format the swapchain is configured with.
    pub format: crcbl_hal::Format,
    /// Suboptimal flag, carried forward per swapchain module docs.
    pub suboptimal: bool,
}

pub struct CommandBufferSlot {
    pub buffer: Option<wgpu::CommandBuffer>,
    pub label: String,
}

/// A signal a submission will perform, waiting on that submission to complete.
pub struct PendingSignal {
    pub submission: wgpu::SubmissionIndex,
    pub value: u64,
}

/// The seam's timeline semaphore, over a queue wgpu synchronises itself.
///
/// wgpu has no semaphore object: submissions on one queue execute in order and
/// the implementation inserts its own hazard barriers, so a *wait* is satisfied
/// by ordering alone. A *signal* is not — `semaphore_value` is how a frame
/// pacer learns that frame N has retired — so signals are recorded against the
/// submission that will perform them and promoted once it completes.
pub struct SemaphoreSlot {
    pub kind: SemaphoreKind,
    pub value: u64,
    pub pending: Vec<PendingSignal>,
}

pub struct Pools {
    pub buffers: Lock<Pool<BufferSlot>>,
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
            surfaces,
        }
    }
}
