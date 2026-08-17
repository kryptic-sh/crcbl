//! The Crucible graphics backend seam: traits plus POD descriptors, and nothing
//! else.
//!
//! Everything above this crate (`crcbl-render`, the samples, the editor) talks
//! to the GPU exclusively through the traits here. Everything below it
//! (`crcbl-vk`, `crcbl-wgpu`, later `crcbl-mtl` / `crcbl-dx12`) implements them.
//! No backend type ever appears in a signature on this side of the seam, and no
//! `#[cfg(target_os = "…")]` ever appears above it.
//!
//! # Shape: Vulkan-flavoured on purpose
//!
//! Per `docs/plan/01-foundations.md` §1.3, the lowest common denominator of
//! Vulkan / Metal / DX12 is "Vulkan-flavoured", so the seam is explicit about
//! the things those APIs are explicit about:
//!
//! * **Explicit passes.** [`CommandEncoder::begin_render_pass`] takes its
//!   attachments inline, dynamic-rendering style. There are no `RenderPass` or
//!   `Framebuffer` objects — Vulkan 1.3 does not need them, and
//!   Metal/DX12/WebGPU never had them.
//! * **Explicit sync, expressed as resource states.** The seam has
//!   [`CommandEncoder::pipeline_barrier`] and timeline semaphores; it does not
//!   track resource state, insert barriers, or reorder anything. Placement is
//!   the render graph's job, above the seam. See [`ResourceState`] and
//!   [`sync`].
//! * **Bindless-capable descriptors.** [`BindGroupLayoutEntry`] carries a
//!   `count` and [`BindingFlags`], so one declaration produces a runtime-sized
//!   descriptor array where the device has descriptor indexing and a fixed
//!   texture-array page where it does not.
//! * **Indirect from day one.** [`CommandEncoder::draw_indexed_indirect_count`]
//!   and friends are not bolted on for topic 03; they are the steady-state draw
//!   path, and [`CommandEncoder::draw`] exists mostly for full-screen triangles
//!   and bring-up.
//!
//! Explicitly **not** here, per the same section: the render graph, frame
//! pacing, materials, and any notion of a mesh. Those live in `crcbl-render`.
//!
//! # Decision: dynamic dispatch is the primary form
//!
//! Every trait in this crate is **object-safe**, and `Instance::create_device`
//! returns `Box<dyn Device>`. Generic use (`fn frame<D: Device + ?Sized>(d: &D)`)
//! still works and monomorphises normally — the bounds are unchanged — but
//! `dyn` is the form the engine is built around and the one the tests exercise.
//!
//! Why:
//!
//! 1. **Two backends are compiled in at once.** `docs/plan/10-wasm-webgpu.md`
//!    wants `crcbl-wgpu` available on native as a triage tool ("does it repro on
//!    wgpu?") alongside `crcbl-vk`. With a generics-first seam, backend choice
//!    would be a compile-time decision, and supporting both would monomorphise
//!    the entire renderer — graph included — once per backend, doubling compile
//!    time and binary size to serve a runtime choice.
//! 2. **The renderer holds one backend for the process lifetime.** Nothing swaps
//!    backends in a loop, so there is nothing for monomorphisation to specialise
//!    across.
//! 3. **Per-draw dispatch cost does not apply.** The dangerous case for `dyn`
//!    would be a vtable call per object. This engine is GPU-driven (topic 03):
//!    the steady-state frame is a fixed set of passes, each emitting one
//!    [`draw_indexed_indirect_count`](CommandEncoder::draw_indexed_indirect_count)
//!    whose draw count lives in a GPU buffer. Command count scales with pass and
//!    bucket count — order 10² — not with object count. A device with no
//!    `draw_indirect_count` still gets one call per bucket through
//!    [`DrawIndirect::draw_count`].
//!
//! Costs, stated plainly:
//!
//! * No cross-crate inlining of encoder methods, and no constant-folding of
//!   descriptors into backend calls.
//! * Object safety forbids generic methods, `impl Trait` returns and
//!   `where Self: Sized`. That constraint is what drove resources to be handles
//!   rather than associated types — see below. It is a real restriction, and the
//!   seam is designed around it rather than fighting it.
//! * If profiling at P7 ever shows encoder dispatch on the critical path, the
//!   escape hatch is a monomorphic fast path inside `crcbl-render` behind a
//!   generic parameter. The traits do not have to change for that.
//!
//! # Decision: resources are handles, not associated types
//!
//! A buffer here is a [`BufferHandle`] — a [`Handle<Buffer>`](crcbl_core::Handle)
//! from `crcbl-core`, where [`Buffer`] is an uninhabited marker type. The backend
//! keeps the real object in its own [`Pool`](crcbl_core::Pool), and the
//! generational handle turns use-after-destroy into a clean lookup failure
//! ([`HalError::InvalidHandle`]) instead of an alias.
//!
//! The alternative — `trait Device { type Buffer; … }` — is what makes a HAL
//! *not* object-safe, forces every consumer to be generic over the backend, and
//! makes "two backends in one binary" a type-level problem. Handles cost a table
//! lookup inside the backend, which a validation-friendly backend wants anyway.
//! They also keep every descriptor in this crate genuinely POD: `Copy`, no
//! lifetimes beyond `&str` labels and `&[…]` slices, and trivially loggable.
//!
//! # Decision: `NullBackend` is always compiled, not feature-gated
//!
//! [`null`] is a complete, recording implementation and is built
//! unconditionally. A feature gate was considered and rejected: the backend is
//! the substrate for the graph-compile suite that lands in *other* crates at P1
//! (`docs/plan/12-testing.md`), so the feature would have to be enabled from
//! those crates' dev-dependencies — the classic unification footgun where
//! `cargo test -p crcbl-render` passes and `cargo build -p crcbl-render` does
//! not. It has no dependencies of its own and is dead-code eliminated when
//! unused, so building it always costs approximately nothing.
//!
//! # Decision: readback is polled, never blocking
//!
//! There is no `read_buffer`. GPU→CPU readback is
//! [`request_readback`](Device::request_readback) →
//! [`poll_readback`](Device::poll_readback) →
//! [`destroy_readback`](Device::destroy_readback), because WebGPU's `mapAsync`
//! resolves on a later turn of the event loop and the browser main thread
//! cannot block on it. A synchronous signature would be a trait method that
//! returns [`HalError::Unsupported`] on the target
//! `docs/plan/10-wasm-webgpu.md` most wants to ship to — the same mistake as a
//! literal `acquire_next_image(semaphore)`. Both are solved the same way: make
//! the portable shape the only shape. See [`readback`] for the argument and the
//! per-backend mapping, and note that poll-shaped is also what makes topic 03's
//! "N frames latent" debug readback expressible at all.
//!
//! # Decision: device creation is polled too
//!
//! [`Instance::request_device`] returns a [`PendingDevice`] whose
//! [`poll`](PendingDevice::poll) yields
//! [`Pending`](DeviceRequestState::Pending) or the finished
//! [`Box<dyn Device>`](Device) — deliberately the same request/poll pair as
//! readback, because it is the same problem: WebGPU's `requestDevice` is a
//! promise and the browser main thread cannot block on it. `crcbl-vk` completes
//! on its first poll, `crcbl-wgpu` drives the real future, and [`null`] can be
//! told to take N polls so a caller's loop is testable with no GPU.
//! `Instance::create_device` remains as the blocking wrapper **on native
//! only** — it is `#[cfg]`-ed out of `wasm32` builds so a browser caller gets a
//! compile error instead of a run-time refusal. See [`device`] for the argument.
//!
//! # Object lifetimes are a stated contract, not an inference
//!
//! `Instance::create_device` returns `Box<dyn Device>`, which is `'static`,
//! and in Vulkan a `VkDevice` genuinely must not outlive its `VkInstance`. The
//! seam resolves that rather than leaving it to the backend author: a `Device`
//! **does** outlive its `Instance` and backends must keep the instance's state
//! alive internally; surfaces are instance-scoped while swapchains are
//! device-scoped, with a stated teardown order; and a handle crossing instances
//! or devices must produce [`HalError::ForeignObject`], never undefined
//! behaviour. All three are implementation obligations spelled out in
//! [`device`], and [`null`] implements and tests them.
//!
//! # Reversed-Z is locked
//!
//! The engine uses reversed depth everywhere: **1.0 at the near plane, 0.0 at
//! infinity, infinite far plane** (`docs/plan/02-vulkan-backend.md`). The seam
//! bakes that into its *defaults*, not into its vocabulary — [`CompareOp`] is
//! named by comparison, so `Greater` means `Greater`. What changes is:
//!
//! * [`DepthStencilState::default`] uses [`CompareOp::Greater`] and
//!   [`Format::D32Float`].
//! * [`ClearValue::default`] clears depth to [`depth::CLEAR`] = `0.0`.
//! * [`Viewport::default`] keeps `depth_min: 0.0, depth_max: 1.0`. Reversed-Z is
//!   produced by the *projection matrix*, never by inverting the viewport depth
//!   range; see [`Viewport`] for why that distinction is load-bearing.
//! * [`SamplerDesc::compare`] and [`DepthBias`] carry the same warning, because
//!   shadow comparisons and polygon offset both flip sign under reversed-Z.
//!
//! # HDR from P1
//!
//! [`Format`] carries [`Rgba16Float`](Format::Rgba16Float) and friends because
//! the renderer targets `RGBA16F` + tonemap from the first lit mesh, so P7's
//! real HDR stack does not force a re-bless of every golden image.
//!
//! # Where `SurfaceTarget` lives
//!
//! In [`crcbl-core`](crcbl_core::surface), re-exported here as
//! [`SurfaceTarget`]. `crcbl-shell` produces it and `crcbl-hal` consumes it, and
//! neither may depend on the other, so it belongs in the crate they both already
//! depend on. The full argument is in that module's docs; the dependency
//! direction is:
//!
//! ```text
//! crcbl-shell ──▶ crcbl-core ◀── crcbl-hal ◀── crcbl-vk / crcbl-wgpu / …
//! ```
//!
//! # Stability
//!
//! This seam is **provisional**, not frozen. Per `docs/plan/ROADMAP.md` it
//! freezes at **P5 exit**, when two backends (`crcbl-vk` and `crcbl-wgpu`) have
//! implemented it — not at P0, and not at P1. Changes before then are expected
//! and cheap; changes after need justification.
//!
//! # Example
//!
//! ```
//! use crcbl_hal::null::NullInstance;
//! use crcbl_hal::{BindingModel, BufferDesc, BufferUsage, DeviceDesc, Instance, MemoryLocation};
//!
//! let instance: Box<dyn Instance> = Box::new(NullInstance::gpu_driven());
//! let adapters = instance.adapters();
//! let device = instance.create_device(&DeviceDesc::for_adapter(adapters[0].id))?;
//!
//! assert_eq!(device.caps().binding_model(), BindingModel::Bindless);
//!
//! let vertices = device.create_buffer(&BufferDesc {
//!     label: Some("vertex pool"),
//!     size: 4 << 20,
//!     usage: BufferUsage::STORAGE | BufferUsage::TRANSFER_DST,
//!     memory: MemoryLocation::DeviceLocal,
//! })?;
//! device.destroy_buffer(vertices);
//! # Ok::<(), crcbl_hal::HalError>(())
//! ```

pub mod capability;
pub mod caps;
pub mod command;
pub mod device;
pub mod error;
pub mod format;
pub mod null;
pub mod pipeline;
pub mod query;
pub mod readback;
pub mod resource;
pub mod shader;
pub mod swapchain;
pub mod sync;
pub mod threading;

pub use capability::{
    Capability, DIVERGENCES, Divergence, DivergenceKind, METAL_NO_DRAW_INDIRECT_COUNT, Support,
    WEBGPU_BIND_GROUPS_ARE_IMMUTABLE, divergence, is_parity_gap, parity_blockers,
};
pub use caps::{
    AdapterId, AdapterInfo, BackendKind, BindingModel, DeviceCaps, DeviceType, Downgrade,
    Downgrades, Features, GeometryPath, LightingPath, Limits, SelectedPath, downgrades,
};
pub use command::{
    Barriers, BufferBarrier, BufferCopy, BufferImageCopy, ClearValue, ColorAttachment,
    CommandBuffer, CommandBufferHandle, CommandEncoder, CommandEncoderDesc, ComputePassDesc,
    DepthStencilAttachment, DrawIndirect, DrawIndirectCount, ImageBarrier, ImageCopy, LoadOp,
    QueueTransfer, RenderPassDesc, ResourceState, StoreOp, depth,
};
pub use device::{
    Device, DeviceDesc, DeviceRequestState, Instance, PendingDevice, Queue, QueueHandle, QueueKind,
};
pub use error::{HalError, SurfaceError};
pub use format::{Format, IndexFormat};
pub use pipeline::{
    BindGroup, BindGroupDesc, BindGroupEntry, BindGroupHandle, BindGroupLayout,
    BindGroupLayoutDesc, BindGroupLayoutEntry, BindGroupLayoutHandle, BindingFlags, BindingKind,
    BindingResource, BlendFactor, BlendOp, BlendState, ColorTargetState, ColorWrites, CompareOp,
    ComputePipeline, ComputePipelineDesc, ComputePipelineHandle, CullMode, DepthBias,
    DepthStencilState, FrontFace, GraphicsPipeline, GraphicsPipelineDesc, GraphicsPipelineHandle,
    MeshPipelineDesc, MultisampleState, PORTABLE_STORAGE_BUFFERS_PER_STAGE, PipelineLayout,
    PipelineLayoutDesc, PipelineLayoutHandle, PolygonMode, PrimitiveState, PrimitiveTopology,
    PushConstantRange, SampleType, StencilFaceState, StencilOp, StencilState, Viewport,
    check_portable_storage_buffers,
};
pub use query::{QueryKind, QuerySet, QuerySetDesc, QuerySetHandle};
pub use readback::{Readback, ReadbackDesc, ReadbackHandle, ReadbackState};
pub use resource::{
    Buffer, BufferDesc, BufferHandle, BufferUsage, Extent3d, FilterMode, Image, ImageAspect,
    ImageDesc, ImageHandle, ImageSubresourceLayers, ImageSubresourceRange, ImageType, ImageUsage,
    ImageView, ImageViewDesc, ImageViewHandle, ImageViewType, MemoryLocation, Offset3d, Rect2d,
    Sampler, SamplerAddressMode, SamplerDesc, SamplerHandle,
};
pub use shader::{
    ShaderEntry, ShaderModule, ShaderModuleDesc, ShaderModuleHandle, ShaderSources, ShaderStages,
};
pub use swapchain::{
    AcquiredFrame, CompositeAlpha, DisplayTiming, PresentInfo, PresentMode, Surface, SurfaceCaps,
    SurfaceHandle, Swapchain, SwapchainDesc, SwapchainHandle, display_timing_from_refresh_nanos,
};
pub use sync::{
    Semaphore, SemaphoreDesc, SemaphoreHandle, SemaphoreKind, SemaphoreSignal, SemaphoreWait,
    SubmitInfo,
};

/// The native window handles a backend needs to create a surface.
///
/// Defined in `crcbl-core` and re-exported here so backends and the renderer
/// name one type. See [`crcbl_core::surface`] for why it lives there rather than
/// in this crate or in `crcbl-shell`.
pub use crcbl_core::SurfaceTarget;
