//! The two central traits: [`Instance`] and [`Device`].
//!
//! ```text
//! Instance ──adapters()──▶ [AdapterInfo]      (caps known before a device exists)
//!    │
//!    ├──create_surface(SurfaceTarget)──▶ SurfaceHandle
//!    └──create_device(DeviceDesc)──────▶ Box<dyn Device>
//!                                            │
//!                                            ├─ resources: buffers, images, views, samplers
//!                                            ├─ pipelines, layouts, bind groups
//!                                            ├─ swapchains: create / reconfigure / acquire / present
//!                                            ├─ command encoders ──▶ Box<dyn CommandEncoder>
//!                                            └─ submit / wait
//! ```
//!
//! # Queues are handles, not a trait
//!
//! `docs/plan/01-foundations.md` asks for "`Device` + `Queue`". A `Queue` here
//! is a [`QueueHandle`] and submission is [`Device::submit`], rather than a
//! third trait object. Queues carry no state a caller can usefully hold, and
//! `Device::submit(queue, …)` keeps the number of trait objects in the engine at
//! three. The plurality that matters — async compute and a dedicated transfer
//! queue, which `docs/plan/02-vulkan-backend.md` says must be *modelled* even
//! though the MVP uses one — is fully expressible: [`Device::queue`] returns a
//! handle per [`QueueKind`], and [`QueueTransfer`](crate::QueueTransfer)
//! already carries ownership transfers between them.
//!
//! # `&self`, not `&mut self`
//!
//! Every method takes `&self`, so a `Device` can be shared behind an `Arc`
//! across threads (the traits are `Send + Sync`). Backends provide their own
//! interior synchronisation, which they need regardless: `vkCreateBuffer` is
//! thread-safe but the underlying suballocator is not, and P8's job system will
//! upload from worker threads. A `&mut self` seam would have forced an
//! engine-wide lock on the device just to create a buffer.
//!
//! Command *recording* is the exception, and it is expressed in the type: a
//! [`CommandEncoder`] is `Send` but not `Sync`, matching Vulkan's
//! external-synchronisation rule for command buffers.
//!
//! # Destruction is explicit and immediate-looking
//!
//! `destroy_*` takes the handle by value and returns nothing. It means "this
//! handle is dead now"; it does **not** promise the GPU is finished with the
//! object. Deferring the real free until N frames later — the deletion queue
//! from `docs/plan/02-vulkan-backend.md` §2.2 — is the backend's business.
//! Destroying a resource the GPU is still using is a caller bug that the graph's
//! lifetime tracking is responsible for preventing, exactly as it is in Vulkan.
//!
//! # Object lifetimes — implementation obligations on backends
//!
//! [`Instance::create_device`] returns `Box<dyn Device>`, which is `'static`, so
//! the type system permits a `Device` to outlive the `Instance` that made it.
//! In Vulkan that relationship is real and inverted: a `VkDevice` must not
//! outlive its `VkInstance`, and a `VkSurfaceKHR` is instance-owned while the
//! swapchain built from it is device-owned. Rather than leave the P1 backend
//! author to guess, the seam states the rules here. **These are obligations on
//! backends, not merely descriptions.**
//!
//! ## 1. A `Device` may outlive its `Instance`
//!
//! A backend **must** keep whatever instance-level state its device needs alive
//! internally — in practice an `Arc` to the instance's inner data, so dropping
//! the public `Instance` object does not destroy the underlying one while a
//! device still exists.
//!
//! The two alternatives were both worse:
//!
//! * `Box<dyn Device + 'a>` borrowed from the instance would express the truth,
//!   but any struct holding both an instance and a device becomes
//!   self-referential. `crcbl-render`'s renderer is exactly that struct, so the
//!   tax would be paid on every line of the layer this seam exists to serve.
//! * An unenforced "you must not drop the instance first" rule is the worst of
//!   both: no compile-time help *and* no runtime detection, with
//!   use-after-free as the failure mode.
//!
//! Keeping the instance alive internally costs one `Arc` per backend and makes
//! the `'static` that the type system already promises actually true.
//!
//! ## 2. Surfaces are instance-scoped; swapchains are device-scoped
//!
//! The legal teardown order is:
//!
//! ```text
//! 1. Device::destroy_swapchain(swapchain)
//! 2. Instance::destroy_surface(surface)
//! 3. drop the Device
//! 4. drop the Instance          (and only now may the window close)
//! ```
//!
//! Steps 3 and 4 may be reordered, per rule 1. Steps 1 and 2 may not, and
//! neither may be skipped while the window is still open — the
//! [`create_surface`](Instance::create_surface) safety contract requires the
//! native handles to outlive the surface.
//!
//! A caller that gets it wrong must get a **clean error, never undefined
//! behaviour**. Concretely, a backend must:
//!
//! * Invalidate the handle immediately on `destroy_surface`, so a later use
//!   fails the generational lookup and returns [`HalError::InvalidHandle`].
//! * **Defer** destroying the underlying driver object until the last swapchain
//!   configured on it is destroyed. Calling `vkDestroySurfaceKHR` with a live
//!   swapchain is undefined behaviour in the driver, so "pass it straight down"
//!   is not an option; the deletion queue that already exists for retiring
//!   resources N frames later is the same machinery.
//!
//! In short: the handle dies when the caller says so, the object dies when it is
//! safe. [`crate::null`] enforces the detectable half — a swapchain created from
//! a destroyed surface, or a present on a destroyed swapchain, is an error and
//! is covered by tests.
//!
//! ## 3. Handles never cross instances or devices
//!
//! A [`SurfaceHandle`] from instance A passed to a device created from instance
//! B is a caller bug, and so is a buffer from device X used on device Y. The
//! required behaviour is [`HalError::ForeignObject`] — detected, not undefined.
//!
//! This one needs a mechanism, because a [`Handle`] cannot carry it: its 32-bit
//! index and 32-bit generation are fully spoken for, and two instances' pools
//! genuinely do issue identical bits. Backends **must** therefore stamp an
//! owner identity into their own side table — one `u64` per tracked object,
//! compared on every lookup. That is one integer compare on a path that is
//! already doing a table lookup.
//!
//! [`crate::null`] implements exactly this: every object records the id of the
//! instance or device that created it, and every lookup checks it. Surfaces are
//! checked against the *instance* id (so any device from that instance may use
//! them) and everything else against the *device* id.

use crate::{
    AcquiredFrame, AdapterId, AdapterInfo, BackendKind, BindGroupDesc, BindGroupHandle,
    BindGroupLayoutDesc, BindGroupLayoutHandle, BufferDesc, BufferHandle, CommandBufferHandle,
    CommandEncoder, CommandEncoderDesc, ComputePipelineDesc, ComputePipelineHandle, DeviceCaps,
    Features, GraphicsPipelineDesc, GraphicsPipelineHandle, HalError, ImageDesc, ImageHandle,
    ImageViewDesc, ImageViewHandle, PipelineLayoutDesc, PipelineLayoutHandle, PresentInfo,
    QuerySetDesc, QuerySetHandle, ReadbackDesc, ReadbackHandle, ReadbackState, SamplerDesc,
    SamplerHandle, SemaphoreDesc, SemaphoreHandle, ShaderModuleDesc, ShaderModuleHandle,
    SubmitInfo, SurfaceCaps, SurfaceError, SurfaceHandle, SwapchainDesc, SwapchainHandle,
};
use crcbl_core::{Handle, SurfaceTarget};

/// Marker type for queue handles. Uninhabited.
#[derive(Debug)]
pub enum Queue {}

/// A queue on a device.
pub type QueueHandle = Handle<Queue>;

/// Which class of work a queue accepts.
///
/// # Decision: `QueueKind`, not `QueueFamily`
///
/// "Queue family" is a Vulkan noun and nothing else's: Metal has one
/// `MTLCommandQueue` type and no families at all, and DX12 has *command list
/// types* (`D3D12_COMMAND_LIST_TYPE_DIRECT` / `_COMPUTE` / `_COPY`), which is
/// exactly this enum's three variants under a different name. Since
/// `docs/plan/01-foundations.md`'s P0 exit criterion is "no obviously vk-only
/// concept in the trait names", the noun that maps onto all three APIs is the
/// one the seam uses, and `Kind` is already this crate's word for it
/// ([`SemaphoreKind`](crate::SemaphoreKind), [`QueryKind`](crate::QueryKind),
/// [`BindingKind`](crate::BindingKind), [`BackendKind`]).
///
/// A Vulkan backend still maps each variant to a real queue family index; that
/// is a backend's business and not the seam's vocabulary.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum QueueKind {
    /// Graphics, compute and transfer. Always present; the MVP uses only this
    /// one.
    Graphics,
    /// Compute and transfer, independent of graphics. Present only with
    /// [`Features::ASYNC_COMPUTE_QUEUE`].
    Compute,
    /// Transfer only. Present only with [`Features::TRANSFER_QUEUE`].
    Transfer,
}

/// Parameters for device creation.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct DeviceDesc<'a> {
    /// Debug name; see [`BufferDesc::label`](crate::BufferDesc::label).
    pub label: Option<&'a str>,
    /// Adapter to open, from [`Instance::adapters`].
    pub adapter: AdapterId,
    /// Features the caller cannot run without. Creation fails with
    /// [`HalError::UnsupportedFeatures`] naming the exact gap if the adapter
    /// lacks any of them.
    ///
    /// `crcbl-vk` passes [`Features::TIER_A`] and refuses anything less; there
    /// are no partial fallback paths in the MVP
    /// (`docs/plan/02-vulkan-backend.md`).
    pub required_features: Features,
    /// Features to enable if available. Absent ones are simply not enabled;
    /// check [`Device::caps`] afterwards to find out.
    pub optional_features: Features,
    /// A surface the device must be able to present to.
    ///
    /// Vulkan needs this at device-creation time to choose a present-capable
    /// queue family, and WebGPU's `requestAdapter` takes a `compatibleSurface`
    /// for the same reason. `None` for a headless device.
    pub compatible_surface: Option<SurfaceHandle>,
}

impl DeviceDesc<'_> {
    /// A Tier A device on `adapter`, with no surface.
    ///
    /// The shape a headless render test or `crcbl screenshot` uses.
    #[must_use]
    pub const fn for_adapter(adapter: AdapterId) -> Self {
        Self {
            label: None,
            adapter,
            required_features: Features::TIER_A,
            optional_features: Features::empty(),
            compatible_surface: None,
        }
    }
}

/// A backend's entry point: enumerates adapters and opens devices and surfaces.
///
/// One instance exists per process per backend. Both `crcbl-vk` and
/// `crcbl-wgpu` may be instantiated in the same process — that is the whole
/// point of the `dyn` decision in the crate docs, and it is how topic 10's
/// "does it repro on wgpu?" triage works.
pub trait Instance: core::fmt::Debug + crate::threading::HalThreadSafe {
    /// Which backend this is. For logs and bug reports — never for behaviour.
    fn backend(&self) -> BackendKind;

    /// Every adapter this backend can see, with its capabilities already
    /// filled in so selection logic never has to open a device to reject one.
    fn adapters(&self) -> Vec<AdapterInfo>;

    /// Creates a presentation surface from a shell's native window handles.
    ///
    /// # Safety
    ///
    /// The caller must guarantee that:
    ///
    /// 1. Every pointer and id in `target` names a **live** platform object of
    ///    exactly the kind the variant says (a `wl_surface*` really is a
    ///    `wl_surface*`, and so on). Nothing in `crcbl-core` validates this;
    ///    only the shell that created the window knows.
    /// 2. Those objects **outlive** the returned [`SurfaceHandle`] and every
    ///    swapchain configured on it. Destroying the window before calling
    ///    [`Device::destroy_swapchain`] and [`Instance::destroy_surface`] is
    ///    use-after-free inside the driver.
    /// 3. On platforms with thread-affine window handles (Win32, AppKit), this
    ///    is called from the thread that owns the window.
    ///
    /// The obligation lives here rather than at `SurfaceTarget` construction
    /// because this is the only place the handles are dereferenced; see
    /// [`crcbl_core::surface`] for the full argument.
    ///
    /// # TODO(P1): this `unsafe` currently lands in application code
    ///
    /// P0.7 drove the first shell→HAL join (`apps/sandbox`) and found the
    /// ergonomic cost: obligation 2 above — "the window outlives the surface" —
    /// can only be discharged by code holding *both* the shell and the device,
    /// which is a game's own startup path. The result is an `unsafe` block in
    /// an application that otherwise contains none, repeated in every sample
    /// and in the `crcbl new` template.
    ///
    /// The likely answer is a shell-aware constructor in `crcbl-render`, which
    /// owns both objects anyway and can tie their lifetimes together once. That
    /// crate does not exist yet, so this is a note rather than a change; the
    /// seam does not have to move for it, because a safe wrapper *above* the
    /// seam is exactly what the obligation permits.
    ///
    /// # Errors
    ///
    /// [`HalError::Unsupported`] if the backend has no support for that
    /// platform, or [`HalError::Backend`] if the window system refused.
    unsafe fn create_surface(&self, target: &SurfaceTarget) -> Result<SurfaceHandle, HalError>;

    /// Destroys a surface. Every swapchain on it must already be destroyed.
    fn destroy_surface(&self, surface: SurfaceHandle);

    /// What `surface` supports on `adapter`.
    ///
    /// # This is also how adapter selection works
    ///
    /// [`adapters`](Instance::adapters) enumerates without reference to any
    /// surface, so **this call is the only way to find out whether a given
    /// adapter can present to a given window** — and the answer is routinely
    /// "no". A discrete GPU under an X server with no DRI3 cannot present to it
    /// while the software rasteriser beside it can; the same is true of the
    /// second GPU in a hybrid laptop. P1.1 hit exactly that, and taking
    /// `adapters()[0]` is therefore not a shortcut but a bug that only shows up
    /// on someone else's machine.
    ///
    /// The contract, in both directions:
    ///
    /// * **Backends** must report an adapter that cannot present to this
    ///   surface as [`HalError::Unsupported`]. Returning empty
    ///   [`formats`](SurfaceCaps::formats) is **not** an option — it collides
    ///   with [`SurfaceCaps::preferred_format`]'s documented meaning — and nor
    ///   is an empty [`present_modes`](SurfaceCaps::present_modes), which would
    ///   break the promise that [`PresentMode::Fifo`](crate::PresentMode::Fifo)
    ///   is always there.
    /// * **Callers** doing selection must treat an `Err` from this call as "try
    ///   the next adapter", not as fatal. Only running out of adapters is
    ///   fatal. `apps/sandbox` is the reference implementation.
    ///
    /// # Errors
    ///
    /// [`HalError::Unsupported`] when the adapter cannot present to this
    /// surface, [`HalError::InvalidHandle`] for a stale surface,
    /// [`HalError::NoSuchAdapter`] for an unknown adapter, or a backend error if
    /// the query failed.
    fn surface_caps(
        &self,
        surface: SurfaceHandle,
        adapter: AdapterId,
    ) -> Result<SurfaceCaps, HalError>;

    /// Opens a device.
    ///
    /// # Errors
    ///
    /// [`HalError::NoSuchAdapter`], or [`HalError::UnsupportedFeatures`] naming
    /// exactly which of `required_features` the adapter lacks.
    fn create_device(&self, desc: &DeviceDesc<'_>) -> Result<Box<dyn Device>, HalError>;
}

/// An open device: creates resources, records and submits work, presents.
///
/// See the module docs for why methods take `&self` and why destruction is
/// explicit.
pub trait Device: core::fmt::Debug + crate::threading::HalThreadSafe {
    /// Which backend this device belongs to.
    fn backend(&self) -> BackendKind;

    /// What this device can do. Drives the Tier A / Tier B choice; see
    /// [`DeviceCaps::tier`].
    fn caps(&self) -> DeviceCaps;

    /// A queue of the given kind.
    ///
    /// Returns `None` when the device has no such queue — check
    /// [`Features::ASYNC_COMPUTE_QUEUE`] / [`Features::TRANSFER_QUEUE`] first,
    /// or just fall back to [`QueueKind::Graphics`], which always exists.
    fn queue(&self, kind: QueueKind) -> Option<QueueHandle>;

    // --- resources ---

    /// Creates a buffer.
    ///
    /// # Errors
    ///
    /// [`HalError::OutOfDeviceMemory`], or [`HalError::InvalidDescriptor`] for a
    /// zero size or a usage/memory combination the backend cannot satisfy.
    fn create_buffer(&self, desc: &BufferDesc<'_>) -> Result<BufferHandle, HalError>;

    /// Destroys a buffer.
    fn destroy_buffer(&self, buffer: BufferHandle);

    /// Copies `data` into a mappable buffer at `offset`.
    ///
    /// Only valid for [`MemoryLocation::HostUpload`](crate::MemoryLocation::HostUpload).
    /// Deliberately a copy rather than a `map`/`unmap` pair returning a slice:
    /// a returned `&mut [u8]` cannot be made sound through a trait object
    /// without a lifetime the object cannot carry, and every real caller is
    /// writing a prepared block into a staging ring anyway. If a caller ever
    /// needs to build data in place, that is a persistent-mapping addition to
    /// this trait, not a reason to leak a pointer today.
    ///
    /// # Errors
    ///
    /// [`HalError::InvalidHandle`], or [`HalError::InvalidDescriptor`] if the
    /// range is out of bounds or the buffer is not host-visible.
    fn write_buffer(&self, buffer: BufferHandle, offset: u64, data: &[u8]) -> Result<(), HalError>;

    /// Starts a GPU→CPU readback of a range of a host-readable buffer.
    ///
    /// **There is no synchronous read.** WebGPU's `mapAsync` completes on a
    /// later turn of the event loop and cannot be blocked on from the browser
    /// main thread, so a blocking signature would be unimplementable on the
    /// target `docs/plan/10-wasm-webgpu.md` cares most about. See
    /// [`crate::readback`] for the full argument and the per-backend mapping.
    ///
    /// The result is observed with [`Device::poll_readback`] and released with
    /// [`Device::destroy_readback`]. A readback covers work already submitted
    /// when this is called, or the completion point named by
    /// [`ReadbackDesc::after`].
    ///
    /// This is still the *one* readback `docs/plan/03-gpu-driven-rendering.md`
    /// §3.5 permits in the frame loop — culling stats, N frames latent, debug
    /// builds only. Poll-shaped is what makes "N frames latent" expressible.
    ///
    /// # Errors
    ///
    /// [`HalError::InvalidHandle`] or [`HalError::ForeignObject`] for the
    /// buffer, or [`HalError::InvalidDescriptor`] if the range is out of bounds
    /// or the buffer is not
    /// [`MemoryLocation::HostReadback`](crate::MemoryLocation::HostReadback).
    fn request_readback(&self, desc: &ReadbackDesc<'_>) -> Result<ReadbackHandle, HalError>;

    /// Checks whether a readback has completed, writing its bytes into `out` if
    /// so.
    ///
    /// `out` must be exactly [`ReadbackDesc::size`] bytes. It is left untouched
    /// while the state is [`ReadbackState::Pending`]. Polling again after
    /// [`ReadbackState::Ready`] is legal and yields the same bytes until the
    /// readback is destroyed.
    ///
    /// Call this at most once per frame per readback; it is a poll, not a wait.
    ///
    /// # Errors
    ///
    /// [`HalError::InvalidHandle`] or [`HalError::ForeignObject`] for the
    /// readback, [`HalError::InvalidDescriptor`] if `out` is the wrong length,
    /// or [`HalError::DeviceLost`] if the work behind it failed. Failure is an
    /// `Err` rather than a third [`ReadbackState`] so the reason survives.
    fn poll_readback(
        &self,
        readback: ReadbackHandle,
        out: &mut [u8],
    ) -> Result<ReadbackState, HalError>;

    /// Releases a readback request, whether or not it completed.
    ///
    /// On WebGPU this is the `unmap`; abandoning readbacks without destroying
    /// them leaks a mapped buffer, which is why this exists rather than being
    /// implicit.
    fn destroy_readback(&self, readback: ReadbackHandle);

    /// Creates an image.
    ///
    /// # Errors
    ///
    /// [`HalError::OutOfDeviceMemory`], or [`HalError::InvalidDescriptor`] if
    /// the extent, mip count or sample count exceeds [`Device::caps`].
    fn create_image(&self, desc: &ImageDesc<'_>) -> Result<ImageHandle, HalError>;

    /// Destroys an image. Every view of it must already be destroyed.
    fn destroy_image(&self, image: ImageHandle);

    /// Creates an image view.
    ///
    /// # Errors
    ///
    /// [`HalError::InvalidHandle`] for a stale image, or
    /// [`HalError::InvalidDescriptor`] for an incompatible format or an
    /// out-of-range subresource.
    fn create_image_view(&self, desc: &ImageViewDesc<'_>) -> Result<ImageViewHandle, HalError>;

    /// Destroys an image view.
    fn destroy_image_view(&self, view: ImageViewHandle);

    /// Creates a sampler.
    ///
    /// # Errors
    ///
    /// [`HalError::InvalidDescriptor`] if anisotropy exceeds
    /// [`Limits::max_sampler_anisotropy`](crate::Limits::max_sampler_anisotropy).
    fn create_sampler(&self, desc: &SamplerDesc<'_>) -> Result<SamplerHandle, HalError>;

    /// Destroys a sampler.
    fn destroy_sampler(&self, sampler: SamplerHandle);

    // --- shaders and pipelines ---

    /// Creates a shader module from SPIR-V.
    ///
    /// # Errors
    ///
    /// [`HalError::ShaderCompilation`] if the SPIR-V is malformed or uses a
    /// capability the device lacks. Backends that cross-compile (Metal, DX12,
    /// WebGPU) report translation failures here too.
    fn create_shader_module(
        &self,
        desc: &ShaderModuleDesc<'_>,
    ) -> Result<ShaderModuleHandle, HalError>;

    /// Destroys a shader module. Pipelines built from it stay valid.
    fn destroy_shader_module(&self, module: ShaderModuleHandle);

    /// Creates a bind-group layout.
    ///
    /// # Errors
    ///
    /// [`HalError::InvalidDescriptor`] if an entry sets a
    /// [`BindingFlags`](crate::BindingFlags) the device does not support, or if
    /// [`VARIABLE_COUNT`](crate::BindingFlags::VARIABLE_COUNT) is not on the
    /// last binding.
    fn create_bind_group_layout(
        &self,
        desc: &BindGroupLayoutDesc<'_>,
    ) -> Result<BindGroupLayoutHandle, HalError>;

    /// Destroys a bind-group layout.
    fn destroy_bind_group_layout(&self, layout: BindGroupLayoutHandle);

    /// Creates a bind group.
    ///
    /// # Errors
    ///
    /// [`HalError::InvalidHandle`] for a stale layout or resource, or
    /// [`HalError::InvalidDescriptor`] if an entry does not match the layout.
    fn create_bind_group(&self, desc: &BindGroupDesc<'_>) -> Result<BindGroupHandle, HalError>;

    /// Updates entries of an existing bind group in place.
    ///
    /// The bindless write path: with
    /// [`BindingFlags::UPDATE_AFTER_BIND`](crate::BindingFlags::UPDATE_AFTER_BIND)
    /// this may be called while command buffers referencing the group are
    /// pending, which is what removes per-object descriptor updates from the
    /// frame loop (topic 03's "zero per-frame descriptor writes in steady
    /// state" is about the *steady* state; streaming a new texture in still
    /// writes one entry).
    ///
    /// # Errors
    ///
    /// [`HalError::InvalidHandle`], or [`HalError::Unsupported`] if the group's
    /// layout was not created with `UPDATE_AFTER_BIND`.
    fn update_bind_group(
        &self,
        group: BindGroupHandle,
        entries: &[crate::BindGroupEntry],
    ) -> Result<(), HalError>;

    /// Destroys a bind group.
    fn destroy_bind_group(&self, group: BindGroupHandle);

    /// Creates a pipeline layout.
    ///
    /// # Errors
    ///
    /// [`HalError::InvalidHandle`], or [`HalError::Unsupported`] if it declares
    /// push constants on a device without
    /// [`Features::PUSH_CONSTANTS`] — loudly, rather than dropping the writes
    /// later.
    fn create_pipeline_layout(
        &self,
        desc: &PipelineLayoutDesc<'_>,
    ) -> Result<PipelineLayoutHandle, HalError>;

    /// Destroys a pipeline layout.
    fn destroy_pipeline_layout(&self, layout: PipelineLayoutHandle);

    /// Creates a graphics pipeline.
    ///
    /// # Errors
    ///
    /// [`HalError::PipelineCreation`] with the driver's message, or
    /// [`HalError::InvalidHandle`] for a stale layout or shader module.
    fn create_graphics_pipeline(
        &self,
        desc: &GraphicsPipelineDesc<'_>,
    ) -> Result<GraphicsPipelineHandle, HalError>;

    /// Destroys a graphics pipeline.
    fn destroy_graphics_pipeline(&self, pipeline: GraphicsPipelineHandle);

    /// Creates a compute pipeline.
    ///
    /// # Errors
    ///
    /// As [`Device::create_graphics_pipeline`].
    fn create_compute_pipeline(
        &self,
        desc: &ComputePipelineDesc<'_>,
    ) -> Result<ComputePipelineHandle, HalError>;

    /// Destroys a compute pipeline.
    fn destroy_compute_pipeline(&self, pipeline: ComputePipelineHandle);

    // --- queries ---

    /// Creates a query set.
    ///
    /// # Errors
    ///
    /// [`HalError::Unsupported`] if the device lacks the corresponding feature.
    fn create_query_set(&self, desc: &QuerySetDesc<'_>) -> Result<QuerySetHandle, HalError>;

    /// Destroys a query set.
    fn destroy_query_set(&self, set: QuerySetHandle);

    /// Reads query results directly, without a resolve-to-buffer round trip.
    ///
    /// `out.len()` results are read starting at `first_query`. Timestamp values
    /// are raw ticks; multiply differences by
    /// [`Limits::timestamp_period_ns`](crate::Limits::timestamp_period_ns).
    ///
    /// Returns zeros on a device without
    /// [`Features::TIMESTAMP_QUERY`] rather than failing — the profiler HUD
    /// degrades, it does not break.
    ///
    /// # Errors
    ///
    /// [`HalError::InvalidHandle`], or [`HalError::InvalidDescriptor`] if the
    /// range exceeds the set.
    fn query_results(
        &self,
        set: QuerySetHandle,
        first_query: u32,
        out: &mut [u64],
    ) -> Result<(), HalError>;

    // --- synchronisation ---

    /// Creates a semaphore. See [`crate::sync`] on why timeline is the default.
    ///
    /// # Errors
    ///
    /// [`HalError::Unsupported`] for a timeline semaphore on a device without
    /// [`Features::TIMELINE_SEMAPHORE`].
    fn create_semaphore(&self, desc: &SemaphoreDesc<'_>) -> Result<SemaphoreHandle, HalError>;

    /// Destroys a semaphore.
    fn destroy_semaphore(&self, semaphore: SemaphoreHandle);

    /// The current value of a timeline semaphore.
    ///
    /// # Errors
    ///
    /// [`HalError::InvalidHandle`], or [`HalError::Unsupported`] for a binary
    /// semaphore, which has no value to read.
    fn semaphore_value(&self, semaphore: SemaphoreHandle) -> Result<u64, HalError>;

    /// Blocks until every `(semaphore, value)` pair has been reached, or until
    /// `timeout_ns` elapses.
    ///
    /// Returns `Ok(true)` if the waits were satisfied and `Ok(false)` on
    /// timeout — a timeout is a normal outcome for a frame-pacing poll, not an
    /// error.
    ///
    /// # Errors
    ///
    /// [`HalError::InvalidHandle`] or [`HalError::DeviceLost`].
    fn wait_semaphores(
        &self,
        waits: &[crate::SemaphoreWait],
        timeout_ns: u64,
    ) -> Result<bool, HalError>;

    /// Blocks until the device has finished everything submitted so far.
    ///
    /// A shutdown and test primitive. Using it in a frame loop destroys
    /// pipelining; the frames-in-flight timeline is the tool for that.
    ///
    /// # Errors
    ///
    /// [`HalError::DeviceLost`].
    fn wait_idle(&self) -> Result<(), HalError>;

    // --- commands ---

    /// Creates a command encoder.
    fn create_command_encoder(&self, desc: &CommandEncoderDesc<'_>) -> Box<dyn CommandEncoder>;

    /// Releases a finished command buffer's resources.
    ///
    /// Must not be called until the submission that used it has completed.
    fn destroy_command_buffer(&self, buffer: CommandBufferHandle);

    /// Submits work to a queue.
    ///
    /// # Errors
    ///
    /// [`HalError::InvalidHandle`] or [`HalError::DeviceLost`].
    fn submit(&self, queue: QueueHandle, submit: &SubmitInfo<'_>) -> Result<(), HalError>;

    // --- presentation ---

    /// Configures a swapchain on a surface.
    ///
    /// # Errors
    ///
    /// [`SurfaceError::Lost`], or a [`HalError`] for an unsupported format or
    /// present mode.
    fn create_swapchain(&self, desc: &SwapchainDesc<'_>) -> Result<SwapchainHandle, SurfaceError>;

    /// Reconfigures an existing swapchain — resize, present-mode change, format
    /// change.
    ///
    /// The handle stays valid, so nothing above has to re-fetch it across a
    /// resize storm. See the [module docs](crate::swapchain) on why this exists
    /// instead of destroy-and-recreate.
    ///
    /// # Errors
    ///
    /// [`SurfaceError::Lost`], or a [`HalError`] for an unsupported
    /// configuration.
    fn reconfigure_swapchain(
        &self,
        swapchain: SwapchainHandle,
        desc: &SwapchainDesc<'_>,
    ) -> Result<(), SurfaceError>;

    /// Destroys a swapchain.
    fn destroy_swapchain(&self, swapchain: SwapchainHandle);

    /// Acquires the next image to render into.
    ///
    /// The returned [`AcquiredFrame`] carries the synchronisation the backend
    /// needs, which is `None`/`None` on backends with an implicit acquire —
    /// see the [module docs](crate::swapchain) for why the seam is shaped this
    /// way rather than taking a caller-supplied semaphore.
    ///
    /// # Errors
    ///
    /// [`SurfaceError::OutOfDate`] after a resize — **expected traffic**;
    /// reconfigure and retry next frame. [`SurfaceError::Lost`] if the window is
    /// gone, [`SurfaceError::Timeout`] if no image became available.
    fn acquire_next_frame(&self, swapchain: SwapchainHandle)
    -> Result<AcquiredFrame, SurfaceError>;

    /// Presents the acquired image.
    ///
    /// # Errors
    ///
    /// [`SurfaceError::OutOfDate`] or [`SurfaceError::Lost`], handled as for
    /// [`Device::acquire_next_frame`].
    fn present(&self, queue: QueueHandle, present: &PresentInfo<'_>) -> Result<(), SurfaceError>;
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The crate's central claim: both traits are usable as trait objects. If
    /// anyone adds a generic method, an `impl Trait` return, or a
    /// `where Self: Sized`, this stops compiling — which is the point.
    #[test]
    fn the_seam_is_object_safe() {
        const _INSTANCE: Option<&dyn Instance> = None;
        const _DEVICE: Option<&dyn Device> = None;
        const _ENCODER: Option<&dyn CommandEncoder> = None;
        // Boxing each one is the property that matters; naming all three in a
        // single function type only trips `clippy::type_complexity`.
        fn boxes_instance(_: Box<dyn Instance>) {}
        fn boxes_device(_: Box<dyn Device>) {}
        fn boxes_encoder(_: Box<dyn CommandEncoder>) {}
        let _ = (
            boxes_instance as fn(_),
            boxes_device as fn(_),
            boxes_encoder as fn(_),
        );
    }

    #[test]
    fn default_device_desc_demands_tier_a_and_no_surface() {
        let desc = DeviceDesc::for_adapter(AdapterId(0));
        assert_eq!(desc.required_features, Features::TIER_A);
        assert!(
            desc.compatible_surface.is_none(),
            "the headless shape must not require a window"
        );
        assert!(desc.optional_features.is_empty());
    }
}
