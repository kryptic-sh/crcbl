//! Resource pools for the wgpu backend.
//!
//! Each HAL resource type gets its own [`crcbl_core::Pool`] mapping handles to
//! wgpu objects. The pools are behind `Mutex` for interior mutability (the HAL
//! trait methods take `&self`).
//!
//! Every entry is wrapped in [`Owned`], which carries the id of the device — or,
//! for surfaces, the instance — that created it. That is `crcbl-hal`'s
//! obligation 3; [`crate::handle`] is where it is enforced, and nothing here
//! reaches into a pool without going through it.

use crate::cell::{Lock, Shared};
use crate::handle::{Owned, Owner};

use crcbl_core::Pool;
use crcbl_hal::{BufferHandle, ImageHandle, ImageViewHandle, MemoryLocation, SemaphoreKind};

/// The surface pool an instance shares with every device it opens, and the
/// instance identity those surfaces are checked against.
///
/// One type rather than two fields threaded side by side, because the pool and
/// the owner are only ever correct together: a device handed one instance's
/// surfaces and another's id would accept every surface and resolve none.
#[derive(Clone)]
pub struct Surfaces {
    pub pool: Shared<Lock<Pool<Owned<SurfaceSlot>>>>,
    pub owner: Owner,
}

impl Surfaces {
    /// A fresh, empty surface pool for one instance.
    pub fn new() -> Self {
        Self {
            pool: Shared::new(Lock::new(Pool::new())),
            owner: Owner::next(),
        }
    }
}

/// A wgpu surface, stored in the instance's surface pool so Device swapchain
/// methods can reach it. `'static` because the caller (`create_surface_unsafe`)
/// guarantees the window handles outlive the surface.
pub struct SurfaceSlot {
    /// `None` for [`SurfaceTarget::Offscreen`](crcbl_core::SurfaceTarget::Offscreen):
    /// there is no window-system object to wrap, and the swapchain built on it
    /// is an [`OffscreenRing`] of plain textures.
    pub surface: Option<wgpu::Surface<'static>>,
    /// Platform tag from the SurfaceTarget that created it, for logs.
    pub platform: &'static str,
    /// Whether this surface can only reach an sRGB format through a *view*.
    ///
    /// True for a WebGPU canvas and nothing else: the spec's supported context
    /// formats are `rgba8unorm`, `bgra8unorm` and `rgba16float`, so `configure`
    /// refuses an `-srgb` one — while `viewFormats` accepts it, which is the
    /// whole reason that member exists. Read by `Instance::surface_caps`, which
    /// advertises the counterparts, and by `Device::create_swapchain`, which
    /// configures the canvas linear and views it sRGB.
    pub srgb_view_only: bool,
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

/// A wgpu bind-group layout and the array length every binding in it declares.
///
/// The counts are kept because a `wgpu::BindGroupLayout` cannot be asked for
/// them, and [`crate::binding`] needs them: wgpu picks the scalar or the array
/// spelling of `wgpu::BindingResource` from the **layout's** count, not from how
/// many entries a bind group happens to fill a binding with. A binding declared
/// with a count and filled with one element still has to arrive as
/// `TextureViewArray`.
///
/// Cheap to clone — `wgpu::BindGroupLayout` is a handle into wgpu's own
/// refcounted storage, and a layout here declares a handful of bindings.
#[derive(Clone)]
pub struct BindGroupLayoutSlot {
    pub layout: wgpu::BindGroupLayout,
    /// `(binding, count)` for every binding the layout declares, sorted by
    /// binding. A `Vec` and a scan rather than a map: these are short enough
    /// that the map's hashing costs more than the scan saves, and the sorted
    /// order keeps the lookup deterministic.
    counts: Vec<(u32, u32)>,
    /// The binding carrying [`BindingFlags::VARIABLE_COUNT`](crcbl_hal::BindingFlags::VARIABLE_COUNT),
    /// if the layout declares one.
    ///
    /// Kept for the same reason the counts are — wgpu's layout has no flags to
    /// read back — and needed because
    /// [`BindGroupDesc::variable_count`](crcbl_hal::BindGroupDesc::variable_count)
    /// describes *that* binding and no other. Without it a group could only be
    /// checked against whichever binding happened to be highest.
    variable_binding: Option<u32>,
}

impl BindGroupLayoutSlot {
    pub fn new(
        layout: wgpu::BindGroupLayout,
        mut counts: Vec<(u32, u32)>,
        variable_binding: Option<u32>,
    ) -> Self {
        counts.sort_unstable_by_key(|(binding, _)| *binding);
        Self {
            layout,
            counts,
            variable_binding,
        }
    }

    /// The array length `binding` declares, or `None` when the layout does not
    /// declare that binding at all.
    pub fn count(&self, binding: u32) -> Option<u32> {
        self.counts
            .iter()
            .find(|(declared, _)| *declared == binding)
            .map(|(_, count)| *count)
    }

    /// The binding a `variable_count` describes, or `None` when the layout
    /// declares no `VARIABLE_COUNT` binding at all.
    pub fn variable_binding(&self) -> Option<u32> {
        self.variable_binding
    }
}

/// Per-swapchain state. A wgpu "swapchain" is either a configured surface or —
/// for [`SurfaceTarget::Offscreen`](crcbl_core::SurfaceTarget::Offscreen) — a
/// ring of plain textures this backend owns.
pub struct SwapchainSlot {
    /// Surface handle (from `Instance::create_surface`) the swapchain is
    /// configured on.
    pub surface_handle: crcbl_hal::SurfaceHandle,
    /// Which of the two shapes this is.
    pub kind: SwapchainKind,
    /// Extent this swapchain was configured at.
    pub extent: (u32, u32),
    /// Format the swapchain is configured with.
    pub format: crcbl_hal::Format,
    /// Suboptimal flag, carried forward per swapchain module docs.
    pub suboptimal: bool,
}

/// The two things a `SwapchainHandle` can name on this backend.
pub enum SwapchainKind {
    /// A configured `wgpu::Surface`: acquire is `get_current_texture` and
    /// present is `wgpu::Queue::present`.
    Windowed(WindowedSwapchain),
    /// A ring of textures with no window system behind it. Acquire rotates the
    /// ring and present advances it — exactly what `crcbl-vk`'s offscreen ring
    /// does, so `crcbl screenshot` and the headless shell run through the same
    /// caller code on both backends.
    Offscreen(OffscreenRing),
}

/// The windowed half of [`SwapchainKind`].
pub struct WindowedSwapchain {
    /// The last-configured surface description.
    pub config: wgpu::SurfaceConfiguration,
    /// The currently acquired frame, valid between acquire and present.
    ///
    /// **Taken and handed to `wgpu::Queue::present`, never dropped.**
    /// `wgpu::SurfaceTexture`'s `Drop` *discards* the image rather than
    /// presenting it, and a discarded image is never handed back to the
    /// presentation engine — so dropping this exhausts the swapchain after
    /// `image_count` frames and every later acquire blocks until it times
    /// out.
    pub acquired: Option<wgpu::SurfaceTexture>,
    /// Handles the current acquire put in the image and view pools.
    ///
    /// Removed on the next acquire and on present: a swapchain texture is
    /// re-acquired every frame, so leaving them in would leak two pool slots
    /// per frame for the life of the process.
    pub frame_image: Option<ImageHandle>,
    pub frame_view: Option<ImageViewHandle>,
}

/// The offscreen half of [`SwapchainKind`]: images the swapchain owns for its
/// whole life, rather than one texture borrowed per frame.
pub struct OffscreenRing {
    /// Pool handles for the ring's textures. Reissued on every reconfigure,
    /// which is what invalidates a caller that held one across a resize.
    pub images: Vec<ImageHandle>,
    /// One whole-image view per image, per the seam's "the swapchain owns the
    /// views too".
    pub views: Vec<ImageViewHandle>,
    /// Ring cursor: the image the next acquire hands out.
    pub next: u32,
    /// Index handed out by the last acquire, if one is outstanding.
    pub acquired: Option<u32>,
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

/// Where a `map_async` has got to, as seen from the poll side.
///
/// wgpu hands the answer to a callback that runs inside some later
/// `Device::poll`, so the callback's only job is to write one of these — the
/// seam's [`poll_readback`](crcbl_hal::Device::poll_readback) reads it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum MapStatus {
    /// `map_async` has not resolved yet.
    Pending,
    /// The range is mapped and can be read.
    Mapped,
    /// wgpu refused the mapping; the message is what it said.
    Failed(String),
}

/// An in-flight `map_async`, which is what a readback *is* on this backend.
pub struct ReadbackSlot {
    /// The buffer being read, so `poll_readback` can resolve it again — and
    /// find it gone if the caller destroyed it in between.
    pub buffer: BufferHandle,
    pub offset: u64,
    pub size: u64,
    /// Written by wgpu's map callback, read by `poll_readback`.
    pub status: Shared<Lock<MapStatus>>,
    /// The submission [`ReadbackDesc::after`](crcbl_hal::ReadbackDesc::after)
    /// named, when it named one. `map_async` already covers "everything
    /// submitted before this call"; an explicit completion point can be later
    /// than that, so it is checked as well rather than instead.
    pub after: Option<wgpu::SubmissionIndex>,
}

pub struct Pools {
    pub buffers: Lock<Pool<Owned<BufferSlot>>>,
    pub images: Lock<Pool<Owned<wgpu::Texture>>>,
    pub image_views: Lock<Pool<Owned<wgpu::TextureView>>>,
    pub samplers: Lock<Pool<Owned<wgpu::Sampler>>>,
    pub shader_modules: Lock<Pool<Owned<wgpu::ShaderModule>>>,
    pub bind_group_layouts: Lock<Pool<Owned<BindGroupLayoutSlot>>>,
    pub bind_groups: Lock<Pool<Owned<wgpu::BindGroup>>>,
    pub pipeline_layouts: Lock<Pool<Owned<wgpu::PipelineLayout>>>,
    pub graphics_pipelines: Lock<Pool<Owned<wgpu::RenderPipeline>>>,
    pub compute_pipelines: Lock<Pool<Owned<wgpu::ComputePipeline>>>,
    pub command_buffers: Shared<Lock<Pool<Owned<CommandBufferSlot>>>>,
    pub swapchains: Lock<Pool<Owned<SwapchainSlot>>>,
    pub semaphores: Lock<Pool<Owned<SemaphoreSlot>>>,
    pub readbacks: Lock<Pool<Owned<ReadbackSlot>>>,
    /// Shared surface pool and the instance that owns it, cloned from the
    /// instance. Surfaces are checked against the *instance* id, so any device
    /// from that instance may use them.
    pub surfaces: Surfaces,
    /// The device every pool above belongs to. Held here rather than on
    /// [`crate::device::WgpuDevice`] because the command encoder resolves
    /// handles through these same pools and must apply the same check.
    pub owner: Owner,
}

impl Pools {
    pub fn new(surfaces: Surfaces) -> Self {
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
            readbacks: Lock::new(Pool::new()),
            surfaces,
            owner: Owner::next(),
        }
    }
}
