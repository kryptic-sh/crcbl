//! `NullBackend` — a complete, recording implementation of the seam that talks
//! to no GPU.
//!
//! It exists for three jobs, in increasing order of usefulness:
//!
//! 1. **Prove the seam compiles and leaks nothing.** If a trait method could
//!    not be implemented without naming a backend type, this module would not
//!    build. `docs/plan/01-foundations.md` calls exactly this out as the slice's
//!    deliverable check.
//! 2. **Be the substrate for the graph-compile suite.** `docs/plan/12-testing.md`
//!    assigns `crcbl-hal`/`crcbl-vk`/`crcbl-wgpu` a "graph-compile unit suite on
//!    NullBackend"; the render graph that lands at P1 compiles its pass list
//!    against this backend in CI, with no GPU and no window.
//! 3. **Answer questions about a frame.** Every call is recorded, so a test can
//!    assert which passes ran in which order, which barriers separated them,
//!    and that nothing leaked. See [`Recorder`].
//!
//! # It can also be slow on purpose
//!
//! Two things the seam defers — readback and device creation — are instant
//! here, which would leave every caller's *poll loop* untested. [`Recorder`]
//! therefore takes a latency for each:
//! [`set_readback_latency`](Recorder::set_readback_latency) and
//! [`set_device_latency`](Recorder::set_device_latency) make N polls report
//! `Pending` before completing, which is what a browser's `mapAsync` and
//! `requestDevice` promises do and what no amount of native hardware will
//! reproduce.
//!
//! # And it can fail on purpose
//!
//! For the same reason, one step further: this backend never fails on its own,
//! so a caller's *error* paths would never run either.
//! [`report_device_error`](Recorder::report_device_error) queues an out-of-band
//! failure, [`fail_next_reconfigures`](Recorder::fail_next_reconfigures) refuses
//! a swapchain rebuild, [`report_swapchain_out_of_date`](Recorder::report_swapchain_out_of_date)
//! resizes the window out from under the frame loop, and
//! [`lose_device`](Recorder::lose_device) kills the device for good. The last
//! two exist because the conditions they model — a resize and a TDR — are
//! otherwise reachable only on a real driver at a moment nobody can schedule.
//!
//! # Always compiled
//!
//! Not behind a feature. See the crate docs for the argument — briefly: other
//! crates' *tests* depend on it, so a feature gate would have to be turned on
//! from their dev-dependencies, and feature unification would then make
//! `cargo test -p x` and `cargo build -p x` disagree.
//!
//! # It validates
//!
//! The backend checks pass nesting, that commands appear in the right scope, and
//! that every handle a command names is still alive. Violations are *recorded*,
//! not panicked on, so a failing test reports the whole frame's problems at once.
//! See [`ValidationError`] and [`Recorder::assert_valid`].
//!
//! # Two presets
//!
//! [`NullInstance::gpu_driven`] reports a full desktop device holding
//! [`Features::GPU_DRIVEN`] and most of what sits beside it;
//! [`NullInstance::portable`] reports the WebGPU-shaped floor — no bindless, no
//! `draw_indirect_count`, no buffer device address, no push constants, no
//! timeline semaphores, and an implicit swapchain acquire. They are two points
//! in the capability space rather than two tiers, and running the same renderer
//! code against both is how degradation gets tested before `crcbl-wgpu` exists.
//!
//! ```
//! use crcbl_hal::null::{NullInstance, Recorder};
//! use crcbl_hal::{BindingModel, GeometryPath, Instance};
//!
//! let recorder = Recorder::new();
//! let instance: Box<dyn Instance> =
//!     Box::new(NullInstance::portable().with_recorder(recorder.clone()));
//!
//! let adapter = instance.adapters()[0].clone();
//! assert_eq!(adapter.caps.binding_model(), BindingModel::ArrayPages);
//! assert_eq!(adapter.caps.geometry_path(), GeometryPath::IndirectPerBatch);
//! assert_eq!(recorder.total_live_objects(), 0);
//! ```

mod record;

pub use record::{Command, Event, ObjectKind, Recorder, ValidationError};

use record::Detail;

use core::ops::Range;
use core::sync::atomic::{AtomicU64, Ordering};
use core::time::Duration;

use crcbl_core::{Handle, SurfaceTarget};

use crate::{
    AcquiredFrame, AdapterId, AdapterInfo, BackendKind, Barriers, BindGroupDesc, BindGroupEntry,
    BindGroupHandle, BindGroupLayoutDesc, BindGroupLayoutHandle, BufferCopy, BufferDesc,
    BufferHandle, BufferImageCopy, CommandBufferHandle, CommandEncoder, CommandEncoderDesc,
    CompositeAlpha, ComputePassDesc, ComputePipelineDesc, ComputePipelineHandle, Device,
    DeviceCaps, DeviceDesc, DeviceRequestState, DeviceType, DisplayTiming, DrawIndirect,
    DrawIndirectCount, Features, Format, GraphicsPipelineDesc, GraphicsPipelineHandle, HalError,
    ImageCopy, ImageDesc, ImageHandle, ImageType, ImageUsage, ImageViewDesc, ImageViewHandle,
    IndexFormat, Instance, Limits, MemoryLocation, MeshPipelineDesc, PendingDevice,
    PipelineLayoutDesc, PipelineLayoutHandle, PresentInfo, PresentMode, QueryKind, QuerySetDesc,
    QuerySetHandle, QueueHandle, QueueKind, ReadbackDesc, ReadbackHandle, ReadbackState, Rect2d,
    RenderPassDesc, SamplerDesc, SamplerHandle, SemaphoreDesc, SemaphoreHandle, SemaphoreKind,
    SemaphoreWait, ShaderModuleDesc, ShaderModuleHandle, ShaderSources, ShaderStages, SubmitInfo,
    SurfaceCaps, SurfaceError, SurfaceHandle, SwapchainDesc, SwapchainHandle, Viewport,
};

/// Formats a null surface reports, and therefore the only ones a swapchain on
/// one may be configured with. `SwapchainDesc::format` says it must be one of
/// [`SurfaceCaps::formats`], so the two lists are one constant rather than two
/// that drift.
const SURFACE_FORMATS: &[Format] = &[Format::Bgra8UnormSrgb, Format::Bgra8Unorm];

/// Fewest images a null swapchain ring holds — [`SurfaceCaps::min_image_count`].
const RING_MIN_IMAGES: u32 = 2;
/// Most images a null swapchain ring holds — [`SurfaceCaps::max_image_count`].
const RING_MAX_IMAGES: u32 = 3;

/// Low bits of a synthesised queue handle that name the queue kind. The rest
/// carry the owning device's tag — see [`queue_handle`].
const QUEUE_KIND_BITS: u32 = 8;

/// The owning device's identity, squeezed into a queue handle's index field.
///
/// Queue handles are not pooled, so nothing else can record who issued one. 24
/// bits is more device identity than a process will ever use and leaves the low
/// byte for the kind.
fn queue_tag(device_id: u64) -> u32 {
    #[allow(clippy::cast_possible_truncation)]
    {
        (device_id % (1 << 24)) as u32
    }
}

/// Deterministic queue handles.
///
/// Queues are not pooled objects — they exist for the device's lifetime and
/// carry no state — so their handles are synthesised rather than allocated.
/// They still carry the **issuing device's tag**, because obligation 3 applies
/// to every handle and a queue handle from another device would otherwise be
/// indistinguishable from this device's own.
fn queue_handle(device_id: u64, index: u32) -> QueueHandle {
    let raw = (queue_tag(device_id) << QUEUE_KIND_BITS) | index;
    Handle::from_bits((1u64 << 32) | u64::from(raw)).expect("generation 1 is non-zero")
}

/// The queue-kind index behind a handle *this* device issued, or `None` if the
/// handle came from somewhere else.
fn queue_index_of(device_id: u64, queue: QueueHandle) -> Option<u32> {
    let raw = queue.index();
    (queue.generation() == 1 && (raw >> QUEUE_KIND_BITS) == queue_tag(device_id))
        .then_some(raw & ((1 << QUEUE_KIND_BITS) - 1))
}

/// Source of owner ids for instances and devices.
///
/// A [`Handle`] has no room for an owner (32-bit index + 32-bit generation are
/// fully spoken for), so ownership lives in the object side table instead. This
/// is the mechanism `crate::device`'s rule 3 obliges every backend to provide;
/// a real backend would key it off its `VkInstance`/`VkDevice` pointer instead
/// of a counter.
static NEXT_OWNER_ID: AtomicU64 = AtomicU64::new(1);

fn next_owner_id() -> u64 {
    NEXT_OWNER_ID.fetch_add(1, Ordering::Relaxed)
}

/// A no-op, recording [`Instance`].
///
/// Share its [`Recorder`] with [`NullInstance::with_recorder`] to inspect the
/// stream after running a frame entirely through `dyn` traits.
#[derive(Clone, Debug)]
pub struct NullInstance {
    recorder: Recorder,
    caps: DeviceCaps,
    name: String,
    /// Owner id stamped into every surface this instance creates. Cloning a
    /// `NullInstance` keeps the id, because a clone *is* the same instance;
    /// a separately constructed one gets a fresh id and its surfaces are
    /// therefore foreign to this one's devices.
    id: u64,
}

impl NullInstance {
    /// A null instance reporting one adapter with `caps`.
    #[must_use]
    pub fn new(caps: DeviceCaps) -> Self {
        Self {
            recorder: Recorder::new(),
            caps,
            name: "null adapter".to_string(),
            id: next_owner_id(),
        }
    }

    /// A full desktop device: everything in [`Features::GPU_DRIVEN`] —
    /// bindless, buffer device address, `draw_indirect_count`, multi-draw,
    /// compute, timeline semaphores — plus push constants, timestamps and the
    /// other query kinds.
    ///
    /// Not mesh shaders and not ray tracing: those are separate selector axes,
    /// and a preset that held every flag would leave
    /// [`GeometryPath::MeshShader`](crate::GeometryPath::MeshShader) as the only
    /// answer any test could get.
    #[must_use]
    pub fn gpu_driven() -> Self {
        Self::new(DeviceCaps {
            features: Features::GPU_DRIVEN
                | Features::INDIRECT_FIRST_INSTANCE
                | Features::TIMESTAMP_QUERY
                | Features::PIPELINE_STATISTICS_QUERY
                | Features::OCCLUSION_QUERY
                | Features::ASYNC_COMPUTE_QUEUE
                | Features::TRANSFER_QUEUE
                | Features::PUSH_CONSTANTS
                | Features::DEPTH_CLAMP
                | Features::DEPTH_BIAS_CLAMP
                | Features::POLYGON_MODE_LINE
                | Features::TEXTURE_COMPRESSION_BC
                | Features::SAMPLER_ANISOTROPY
                | Features::DEBUG_MARKERS,
            limits: Limits::desktop(),
        })
    }

    /// The floor every backend clears, shaped like WebGPU: compute only.
    ///
    /// No descriptor indexing, no buffer device address, no
    /// `draw_indirect_count`, no multi-draw, no push constants, no timeline
    /// semaphores, and — the shape that matters most — an **implicit swapchain
    /// acquire**, so [`AcquiredFrame`] comes back with no semaphores at all.
    /// Every selector lands on its lowest value here. Renderer code that works
    /// against both presets works against `crcbl-vk` and `crcbl-wgpu`.
    #[must_use]
    pub fn portable() -> Self {
        let mut instance = Self::new(DeviceCaps {
            features: Features::COMPUTE | Features::TEXTURE_COMPRESSION_BC,
            limits: Limits::minimum(),
        });
        instance.name = "null adapter (portable)".to_string();
        instance
    }

    /// Uses `recorder` instead of this instance's own, so a caller can keep a
    /// handle on the stream after the instance is boxed as `dyn Instance`.
    #[must_use]
    pub fn with_recorder(mut self, recorder: Recorder) -> Self {
        self.recorder = recorder;
        self
    }

    /// A clone of this instance's recorder.
    #[must_use]
    pub fn recorder(&self) -> Recorder {
        self.recorder.clone()
    }

    /// Whether this backend models an implicit (WebGPU-style) swapchain
    /// acquire, in which case [`AcquiredFrame`] carries no semaphores.
    fn implicit_acquire(&self) -> bool {
        !self.caps.features.contains(Features::TIMELINE_SEMAPHORE)
    }

    /// Resolves a surface handle against *this* instance.
    fn check_surface(&self, surface: SurfaceHandle) -> Result<(), HalError> {
        let state = self.recorder.lock();
        if state.is_owned_by(ObjectKind::Surface, surface.to_bits(), self.id) {
            Ok(())
        } else if state.is_live(ObjectKind::Surface, surface.to_bits()) {
            Err(HalError::ForeignObject {
                kind: "surface",
                bits: surface.to_bits(),
            })
        } else {
            Err(HalError::invalid_handle("surface", surface))
        }
    }
}

impl Instance for NullInstance {
    fn backend(&self) -> BackendKind {
        BackendKind::Null
    }

    fn adapters(&self) -> Vec<AdapterInfo> {
        vec![AdapterInfo {
            id: AdapterId(0),
            name: self.name.clone(),
            vendor_id: 0,
            device_id: 0,
            device_type: DeviceType::Cpu,
            driver: format!("crcbl-hal {}", env!("CARGO_PKG_VERSION")),
            backend: BackendKind::Null,
            caps: self.caps,
        }]
    }

    unsafe fn create_surface(&self, target: &SurfaceTarget) -> Result<SurfaceHandle, HalError> {
        // Nothing is dereferenced, which is why this implementation is sound for
        // any input — the `unsafe` on the trait method is a contract real
        // backends need, not one this one uses.
        let mut state = self.recorder.lock();
        Ok(state.insert_owned(
            ObjectKind::Surface,
            self.id,
            Some(target.platform_name().to_string()),
            Detail::None,
        ))
    }

    fn destroy_surface(&self, surface: SurfaceHandle) {
        // Only this instance's surfaces; another instance's handle may collide
        // on bits and must not be destroyed by mistake. One lock, not two: a
        // check-then-act pair on a non-reentrant mutex is both racy and a
        // deadlock waiting for a scoping change.
        let mut state = self.recorder.lock();
        if state.is_owned_by(ObjectKind::Surface, surface.to_bits(), self.id) {
            state.remove(ObjectKind::Surface, surface.to_bits());
        }
    }

    fn surface_caps(
        &self,
        surface: SurfaceHandle,
        adapter: AdapterId,
    ) -> Result<SurfaceCaps, HalError> {
        if adapter != AdapterId(0) {
            return Err(HalError::NoSuchAdapter(adapter.0));
        }
        self.check_surface(surface)?;
        Ok(SurfaceCaps {
            formats: SURFACE_FORMATS.to_vec(),
            present_modes: if self.implicit_acquire() {
                vec![PresentMode::Fifo]
            } else {
                vec![
                    PresentMode::Fifo,
                    PresentMode::Mailbox,
                    PresentMode::Immediate,
                ]
            },
            composite_alpha: vec![CompositeAlpha::Opaque],
            min_image_count: RING_MIN_IMAGES,
            max_image_count: RING_MAX_IMAGES,
            current_extent: None,
        })
    }

    fn request_device(&self, desc: &DeviceDesc<'_>) -> Result<Box<dyn PendingDevice>, HalError> {
        if desc.adapter != AdapterId(0) {
            return Err(HalError::NoSuchAdapter(desc.adapter.0));
        }
        let missing = self.caps.missing(desc.required_features);
        if !missing.is_empty() {
            return Err(HalError::UnsupportedFeatures { missing });
        }
        if let Some(surface) = desc.compatible_surface {
            self.check_surface(surface)?;
        }
        // Optional features are granted only where the adapter has them, which
        // is the behaviour a caller must handle on a real backend. The union
        // with `required_features` that used to be here was dead: anything
        // required and absent left through `missing` above.
        let caps = DeviceCaps {
            features: self
                .caps
                .features
                .intersection(desc.required_features.union(desc.optional_features)),
            limits: self.caps.limits,
        };
        // Everything checkable is checked above, synchronously, which is the
        // seam's rule: only what depends on a driver answering may be deferred.
        // The null backend has no driver, so all that is left to defer is the
        // simulated latency.
        let device = NullDevice {
            recorder: self.recorder.clone(),
            instance_id: self.id,
            device_id: next_owner_id(),
            // The *device's* granted features decide the acquire shape, not the
            // adapter's. A caller that never asked for timeline semaphores gets
            // the implicit-acquire shape even on a fully featured adapter,
            // which is
            // exactly what it would get from a real backend.
            implicit_acquire: !caps.features.contains(Features::TIMELINE_SEMAPHORE),
            caps,
            adapter_features: self.caps.features,
        };
        let polls_remaining = self.recorder.lock().device_latency;
        Ok(Box::new(NullPendingDevice {
            polls_remaining,
            device: Some(Box::new(device)),
        }))
    }
}

/// A null device request in flight.
///
/// Reached through `dyn PendingDevice` from
/// [`Instance::request_device`](crate::Instance::request_device). The device is
/// built up front — nothing about it can fail late — and handed over once the
/// simulated latency from
/// [`Recorder::set_device_latency`] has elapsed, which is how a caller's poll
/// loop gets exercised with no GPU and no browser.
#[derive(Debug)]
struct NullPendingDevice {
    polls_remaining: u32,
    /// `None` once the device has been handed over, so a second poll is the
    /// caller bug it is rather than a second device.
    device: Option<Box<dyn Device>>,
}

impl PendingDevice for NullPendingDevice {
    fn backend(&self) -> BackendKind {
        BackendKind::Null
    }

    fn poll(&mut self) -> Result<DeviceRequestState, HalError> {
        let Some(device) = self.device.take() else {
            return Err(HalError::InvalidDescriptor(
                "this device request already produced its device".to_string(),
            ));
        };
        if self.polls_remaining > 0 {
            self.polls_remaining -= 1;
            self.device = Some(device);
            return Ok(DeviceRequestState::Pending);
        }
        Ok(DeviceRequestState::Ready(device))
    }
}

/// A no-op, recording [`Device`].
///
/// Reached through `dyn Device` from [`Instance::create_device`]; there is no
/// reason for a test to name this type.
#[derive(Debug)]
struct NullDevice {
    recorder: Recorder,
    /// Id of the instance that created this device. Surfaces are checked
    /// against this, so any device from that instance may use them.
    instance_id: u64,
    /// Id stamped into every object this device creates.
    device_id: u64,
    caps: DeviceCaps,
    /// Everything the adapter can do, which may exceed what this device
    /// enabled. Used to decide which queues exist.
    adapter_features: Features,
    implicit_acquire: bool,
}

impl NullDevice {
    /// Resolves a handle against an expected owner.
    ///
    /// Three outcomes, kept distinct because their fixes differ: resolved,
    /// resolved-but-someone-else's ([`HalError::ForeignObject`]), and gone
    /// ([`HalError::InvalidHandle`]).
    fn check_owner(
        &self,
        kind: ObjectKind,
        bits: u64,
        name: &'static str,
        owner: u64,
    ) -> Result<(), HalError> {
        self.check_lost()?;
        let state = self.recorder.lock();
        if state.is_owned_by(kind, bits, owner) {
            Ok(())
        } else if state.is_live(kind, bits) {
            Err(HalError::ForeignObject { kind: name, bits })
        } else {
            Err(HalError::InvalidHandle { kind: name, bits })
        }
    }

    /// Resolves a device-owned handle.
    fn check(&self, kind: ObjectKind, bits: u64, name: &'static str) -> Result<(), HalError> {
        self.check_owner(kind, bits, name, self.device_id)
    }

    /// Fails once [`Recorder::lose_device`] has been called, and thereafter.
    ///
    /// One insertion point rather than one per method: [`check_owner`] and
    /// [`check_queue`] are the two funnels every fallible call that names a
    /// handle already passes through, so gating them is what makes a lost
    /// device lost everywhere at once — and it lands the error in the right
    /// vocabulary for free, since the presentation calls wrap what they return
    /// in [`SurfaceError::Hal`], which is exactly what `crcbl-vk`'s
    /// `surface_error` does with `VK_ERROR_DEVICE_LOST`.
    ///
    /// [`check_owner`]: Self::check_owner
    /// [`check_queue`]: Self::check_queue
    fn check_lost(&self) -> Result<(), HalError> {
        match self.recorder.lock().lost_device.clone() {
            Some(message) => Err(HalError::DeviceLost(message)),
            None => Ok(()),
        }
    }

    /// Resolves a swapchain handle for one of the three calls a frame makes on
    /// it, and reports the out-of-date latch first.
    ///
    /// `acquire_next_frame`, `present` and `wait_until_presented` all have to
    /// answer the same way while the surface has moved under the swapchain —
    /// that is what makes a resize storm a storm rather than one bad call — so
    /// they ask in one place instead of three.
    ///
    /// The handle is resolved *before* the latch is read, so a swapchain that
    /// was destroyed still reports its own bug rather than an out-of-date one;
    /// and a lost device beats both, through [`check_lost`](Self::check_lost)
    /// inside the resolve.
    fn check_current(&self, swapchain: SwapchainHandle) -> Result<(), SurfaceError> {
        self.check(ObjectKind::Swapchain, swapchain.to_bits(), "swapchain")
            .map_err(SurfaceError::Hal)?;
        if self.recorder.lock().swapchain_out_of_date {
            return Err(SurfaceError::OutOfDate);
        }
        Ok(())
    }

    /// Resolves an instance-owned handle — surfaces, and only surfaces.
    fn check_instance_owned(
        &self,
        kind: ObjectKind,
        bits: u64,
        name: &'static str,
    ) -> Result<(), HalError> {
        self.check_owner(kind, bits, name, self.instance_id)
    }

    /// Resolves one pipeline stage: its module handle, and that the module can
    /// actually supply the entry point the stage names.
    ///
    /// The second half only bites a module that offered DXIL. That is the one
    /// artifact format with a container *per entry point*, so it is the one a
    /// call site can under-supply — offering the vertex container and not the
    /// fragment one leaves every other backend working and D3D12 with no
    /// bytecode for half the pipeline. Every real backend that consumes DXIL
    /// refuses that; saying so here means the no-GPU suite catches it on a
    /// machine with no D3D12, which is where these call sites are written.
    ///
    /// A module with no DXIL at all is not held to anything: it made no claim,
    /// and the backends that read its SPIR-V, WGSL or MSL find every entry
    /// point inside the one artifact.
    fn check_stage(&self, stage: crate::ShaderEntry<'_>) -> Result<(), HalError> {
        let bits = stage.module.to_bits();
        self.check(ObjectKind::ShaderModule, bits, "shader module")?;
        let state = self.recorder.lock();
        let Some(Detail::ShaderModule { dxil_entry_points }) = state
            .get(ObjectKind::ShaderModule, bits)
            .map(|object| &object.detail)
        else {
            return Ok(());
        };
        if dxil_entry_points.is_empty()
            || dxil_entry_points
                .iter()
                .any(|name| name == stage.entry_point)
        {
            return Ok(());
        }
        Err(HalError::ShaderCompilation(format!(
            "shader module `{label}` offers DXIL for {held}, and this stage names `{wanted}`; a \
             DXIL container is compiled for one entry point, so a module used at two stages has \
             to offer a container for each",
            label = state
                .get(ObjectKind::ShaderModule, bits)
                .and_then(|object| object.label.as_deref())
                .unwrap_or("<unlabelled>"),
            held = dxil_entry_points.join(", "),
            wanted = stage.entry_point,
        )))
    }

    fn insert<T>(&self, kind: ObjectKind, label: Option<&str>, detail: Detail) -> Handle<T> {
        self.recorder.lock().insert_owned(
            kind,
            self.device_id,
            label.map(ToOwned::to_owned),
            detail,
        )
    }

    /// Destroys a handle, but only if this device owns it.
    ///
    /// One lock for the check and the removal — see `destroy_surface`.
    fn remove(&self, kind: ObjectKind, bits: u64) {
        let mut state = self.recorder.lock();
        if state.is_owned_by(kind, bits, self.device_id) {
            state.remove(kind, bits);
        }
    }

    fn unsupported(&self, what: &'static str) -> HalError {
        HalError::Unsupported {
            backend: BackendKind::Null,
            what,
        }
    }

    /// The checks a raster and a mesh pipeline share.
    ///
    /// Everything after the geometry stages is identical between the two —
    /// same layout, same fragment stage, same attachment formats, same
    /// rasteriser state — so the two `create_*_pipeline` methods differ only in
    /// the stages they resolve and the capability they demand. Splitting it
    /// this way is what keeps a fix to one of these checks from landing in one
    /// pipeline kind and not the other.
    fn check_raster_state(
        &self,
        layout: PipelineLayoutHandle,
        fragment: Option<crate::ShaderEntry<'_>>,
        color_targets: &[crate::ColorTargetState],
        primitive: crate::PrimitiveState,
        depth_stencil: Option<crate::DepthStencilState>,
    ) -> Result<(), HalError> {
        self.check(
            ObjectKind::PipelineLayout,
            layout.to_bits(),
            "pipeline layout",
        )?;
        if let Some(fragment) = fragment {
            self.check_stage(fragment)?;
        }
        if color_targets.len() > self.caps.limits.max_color_attachments as usize {
            return Err(HalError::InvalidDescriptor(format!(
                "{} colour targets exceeds max_color_attachments {}",
                color_targets.len(),
                self.caps.limits.max_color_attachments
            )));
        }
        if primitive.polygon_mode == crate::PolygonMode::Line
            && !self.caps.features.contains(Features::POLYGON_MODE_LINE)
        {
            return Err(self.unsupported("wireframe polygon mode"));
        }
        if primitive.depth_clamp && !self.caps.features.contains(Features::DEPTH_CLAMP) {
            return Err(self.unsupported("depth clamping"));
        }
        if let Some(depth) = depth_stencil
            && !depth.format.is_depth_stencil()
        {
            return Err(HalError::InvalidDescriptor(format!(
                "{:?} is not a depth/stencil format",
                depth.format
            )));
        }
        Ok(())
    }

    /// Whether this device has a queue of `kind` at all.
    fn has_queue(&self, kind: QueueKind) -> bool {
        match kind {
            QueueKind::Graphics => true,
            QueueKind::Compute => self
                .adapter_features
                .contains(Features::ASYNC_COMPUTE_QUEUE),
            QueueKind::Transfer => self.adapter_features.contains(Features::TRANSFER_QUEUE),
        }
    }

    /// Resolves a queue handle against *this* device.
    ///
    /// Obligation 3 covers queues too. `submit`, `present` and
    /// `create_command_encoder` used to ignore the argument entirely, so a
    /// handle from another device — or a hand-made one — submitted happily.
    fn check_queue(&self, queue: QueueHandle) -> Result<(), HalError> {
        self.check_lost()?;
        let bits = queue.to_bits();
        let Some(index) = queue_index_of(self.device_id, queue) else {
            // Another device's queue handle is well-formed but not ours; a
            // hand-made one is neither, and there is no way to tell them apart
            // from the bits, so the more specific error is the honest one only
            // when the shape is right.
            return Err(if queue.generation() == 1 {
                HalError::ForeignObject {
                    kind: "queue",
                    bits,
                }
            } else {
                HalError::InvalidHandle {
                    kind: "queue",
                    bits,
                }
            });
        };
        let kind = match index {
            0 => QueueKind::Graphics,
            1 => QueueKind::Compute,
            2 => QueueKind::Transfer,
            _ => {
                return Err(HalError::InvalidHandle {
                    kind: "queue",
                    bits,
                });
            }
        };
        if self.has_queue(kind) {
            Ok(())
        } else {
            Err(HalError::InvalidHandle {
                kind: "queue",
                bits,
            })
        }
    }
}

/// The index a queue kind occupies in a synthesised handle.
const fn queue_kind_index(kind: QueueKind) -> u32 {
    match kind {
        QueueKind::Graphics => 0,
        QueueKind::Compute => 1,
        QueueKind::Transfer => 2,
    }
}

impl Device for NullDevice {
    fn backend(&self) -> BackendKind {
        BackendKind::Null
    }

    fn caps(&self) -> DeviceCaps {
        self.caps
    }

    /// Whatever [`Recorder::report_device_error`] queued, oldest first.
    ///
    /// Nothing in this backend fails on its own — it is a recorder, not a
    /// driver — so every error here was put there by a test that wants to see
    /// what its caller does with one.
    fn take_error(&self) -> Option<String> {
        self.recorder.take_device_error()
    }

    fn queue(&self, kind: QueueKind) -> Option<QueueHandle> {
        self.has_queue(kind)
            .then(|| queue_handle(self.device_id, queue_kind_index(kind)))
    }

    // --- resources ---

    fn create_buffer(&self, desc: &BufferDesc<'_>) -> Result<BufferHandle, HalError> {
        if desc.size == 0 {
            return Err(HalError::InvalidDescriptor(
                "buffer size must be non-zero".to_string(),
            ));
        }
        let bytes = if desc.memory.is_mappable() {
            vec![
                0u8;
                usize::try_from(desc.size).map_err(|_| {
                    HalError::InvalidDescriptor("mappable buffer larger than usize".to_string())
                })?
            ]
        } else {
            Vec::new()
        };
        Ok(self.insert(
            ObjectKind::Buffer,
            desc.label,
            Detail::Buffer {
                size: desc.size,
                memory: desc.memory,
                bytes,
            },
        ))
    }

    fn destroy_buffer(&self, buffer: BufferHandle) {
        self.remove(ObjectKind::Buffer, buffer.to_bits());
    }

    fn write_buffer(&self, buffer: BufferHandle, offset: u64, data: &[u8]) -> Result<(), HalError> {
        self.check(ObjectKind::Buffer, buffer.to_bits(), "buffer")?;
        let mut state = self.recorder.lock();
        let Some(object) = state.get_mut(ObjectKind::Buffer, buffer.to_bits()) else {
            return Err(HalError::invalid_handle("buffer", buffer));
        };
        let Detail::Buffer {
            size,
            memory,
            bytes,
        } = &mut object.detail
        else {
            unreachable!("a buffer handle always carries buffer detail");
        };
        if *memory != MemoryLocation::HostUpload {
            return Err(HalError::InvalidDescriptor(format!(
                "write_buffer needs HostUpload memory, buffer is {memory:?}"
            )));
        }
        let start = usize::try_from(offset)
            .map_err(|_| HalError::InvalidDescriptor("offset exceeds usize".to_string()))?;
        let end = start
            .checked_add(data.len())
            .ok_or_else(|| HalError::InvalidDescriptor("offset + len overflows".to_string()))?;
        if end as u64 > *size {
            return Err(HalError::InvalidDescriptor(format!(
                "write of {} bytes at {offset} exceeds the {size}-byte buffer",
                data.len(),
            )));
        }
        bytes[start..end].copy_from_slice(data);
        state.events.push(Event::BufferWritten {
            buffer,
            offset,
            len: data.len(),
        });
        Ok(())
    }

    fn request_readback(&self, desc: &ReadbackDesc<'_>) -> Result<ReadbackHandle, HalError> {
        self.check(ObjectKind::Buffer, desc.buffer.to_bits(), "buffer")?;
        if let Some(wait) = desc.after {
            self.check(ObjectKind::Semaphore, wait.semaphore.to_bits(), "semaphore")?;
        }
        // Re-resolved under a fresh lock, so a concurrent `destroy_buffer` in
        // the gap since `check` is an `InvalidHandle` rather than a panic:
        // `Device` is `&self + Send + Sync`, so that gap is reachable.
        let (memory, buffer_size) = {
            let state = self.recorder.lock();
            let Some(object) = state.get(ObjectKind::Buffer, desc.buffer.to_bits()) else {
                return Err(HalError::invalid_handle("buffer", desc.buffer));
            };
            let Detail::Buffer { size, memory, .. } = &object.detail else {
                unreachable!("a buffer handle always carries buffer detail");
            };
            (*memory, *size)
        };
        if memory != MemoryLocation::HostReadback {
            return Err(HalError::InvalidDescriptor(format!(
                "readback needs HostReadback memory, buffer is {memory:?}"
            )));
        }
        let end = desc
            .offset
            .checked_add(desc.size)
            .ok_or_else(|| HalError::InvalidDescriptor("offset + size overflows".to_string()))?;
        if end > buffer_size {
            return Err(HalError::InvalidDescriptor(format!(
                "readback of {} bytes at {} exceeds the {buffer_size}-byte buffer",
                desc.size, desc.offset
            )));
        }
        let mut state = self.recorder.lock();
        let polls_remaining = state.readback_latency;
        let handle = state.insert_owned(
            ObjectKind::Readback,
            self.device_id,
            desc.label.map(ToOwned::to_owned),
            Detail::Readback {
                buffer: desc.buffer,
                offset: desc.offset,
                size: desc.size,
                polls_remaining,
            },
        );
        state.events.push(Event::ReadbackRequested {
            buffer: desc.buffer,
            offset: desc.offset,
            size: desc.size,
        });
        Ok(handle)
    }

    fn poll_readback(
        &self,
        readback: ReadbackHandle,
        out: &mut [u8],
    ) -> Result<ReadbackState, HalError> {
        self.check(ObjectKind::Readback, readback.to_bits(), "readback")?;
        let (buffer, offset, ready) = {
            let mut state = self.recorder.lock();
            let Some(object) = state.get_mut(ObjectKind::Readback, readback.to_bits()) else {
                return Err(HalError::invalid_handle("readback", readback));
            };
            let Detail::Readback {
                buffer,
                offset,
                size,
                polls_remaining,
            } = &mut object.detail
            else {
                unreachable!("a readback handle always carries readback detail");
            };
            // The descriptor check happens *before* the simulated latency
            // advances. A rejected poll must not consume one: a caller whose
            // slice is the wrong length would otherwise see the configured
            // latency shortened by however many times it got the length wrong,
            // which makes the latency it is being tested against a lie.
            if out.len() as u64 != *size {
                return Err(HalError::InvalidDescriptor(format!(
                    "poll_readback needs a {size}-byte slice, got {}",
                    out.len()
                )));
            }
            let ready = *polls_remaining == 0;
            if !ready {
                *polls_remaining -= 1;
            }
            (*buffer, *offset, ready)
        };
        if !ready {
            return Ok(ReadbackState::Pending);
        }
        // The source buffer may have been destroyed between request and poll,
        // which is a caller bug and must be reported rather than read.
        self.check(ObjectKind::Buffer, buffer.to_bits(), "buffer")?;
        let mut state = self.recorder.lock();
        let Some(object) = state.get(ObjectKind::Buffer, buffer.to_bits()) else {
            return Err(HalError::invalid_handle("buffer", buffer));
        };
        let Detail::Buffer { bytes, .. } = &object.detail else {
            unreachable!("a buffer handle always carries buffer detail");
        };
        let start = usize::try_from(offset)
            .map_err(|_| HalError::InvalidDescriptor("offset exceeds usize".to_string()))?;
        out.copy_from_slice(&bytes[start..start + out.len()]);
        let len = out.len();
        state.events.push(Event::BufferRead {
            buffer,
            offset,
            len,
        });
        Ok(ReadbackState::Ready)
    }

    fn destroy_readback(&self, readback: ReadbackHandle) {
        self.remove(ObjectKind::Readback, readback.to_bits());
    }

    fn create_image(&self, desc: &ImageDesc<'_>) -> Result<ImageHandle, HalError> {
        let extent = desc.extent;
        if extent.width == 0 || extent.height == 0 || extent.depth_or_layers == 0 {
            return Err(HalError::InvalidDescriptor(
                "image extent must be non-zero in every dimension".to_string(),
            ));
        }
        let limits = self.caps.limits;
        // A 3D image is bounded by `max_image_3d` on **every** axis, including
        // its depth; a 1D/2D one by `max_image_2d` on width and height and by
        // `max_image_array_layers` on its layer count. Checking
        // `max(width, height)` against `max_image_2d` for both left
        // `max_image_3d` read by nothing and a volume's depth checked against
        // nothing at all.
        if desc.image_type == ImageType::D3 {
            let longest = extent.width.max(extent.height).max(extent.depth_or_layers);
            if longest > limits.max_image_3d {
                return Err(HalError::InvalidDescriptor(format!(
                    "3D image extent {extent:?} exceeds max_image_3d {}",
                    limits.max_image_3d
                )));
            }
        } else {
            let longest = extent.width.max(extent.height);
            if longest > limits.max_image_2d {
                return Err(HalError::InvalidDescriptor(format!(
                    "image extent {longest} exceeds max_image_2d {}",
                    limits.max_image_2d
                )));
            }
            if extent.depth_or_layers > limits.max_image_array_layers {
                return Err(HalError::InvalidDescriptor(format!(
                    "{} array layers exceeds max_image_array_layers {}",
                    extent.depth_or_layers, limits.max_image_array_layers
                )));
            }
        }
        if desc.mip_levels == 0 {
            return Err(HalError::InvalidDescriptor(
                "image must have at least one mip level".to_string(),
            ));
        }
        let full_chain = extent.full_mip_levels(desc.image_type);
        if desc.mip_levels > full_chain {
            return Err(HalError::InvalidDescriptor(format!(
                "{} mip levels exceeds the {full_chain} a {extent:?} {:?} image can have",
                desc.mip_levels, desc.image_type
            )));
        }
        // A sample count is a bit in a mask on every API underneath, so `3`
        // is not "three samples" — it is two bits set, which reaches a driver
        // as a nonsense request rather than an error.
        if !desc.samples.is_power_of_two() || desc.samples > limits.max_sample_count {
            return Err(HalError::InvalidDescriptor(format!(
                "{} samples is not a power of two in 1..={}",
                desc.samples, limits.max_sample_count
            )));
        }
        if desc.usage.is_empty() {
            return Err(HalError::InvalidDescriptor(
                "an image with no usage flags can never be used".to_string(),
            ));
        }
        Ok(self.insert(ObjectKind::Image, desc.label, Detail::None))
    }

    fn destroy_image(&self, image: ImageHandle) {
        self.remove(ObjectKind::Image, image.to_bits());
    }

    fn create_image_view(&self, desc: &ImageViewDesc<'_>) -> Result<ImageViewHandle, HalError> {
        self.check(ObjectKind::Image, desc.image.to_bits(), "image")?;
        Ok(self.insert(ObjectKind::ImageView, desc.label, Detail::None))
    }

    fn destroy_image_view(&self, view: ImageViewHandle) {
        self.remove(ObjectKind::ImageView, view.to_bits());
    }

    fn create_sampler(&self, desc: &SamplerDesc<'_>) -> Result<SamplerHandle, HalError> {
        if desc.anisotropy > self.caps.limits.max_sampler_anisotropy {
            return Err(HalError::InvalidDescriptor(format!(
                "anisotropy {} exceeds max_sampler_anisotropy {}",
                desc.anisotropy, self.caps.limits.max_sampler_anisotropy
            )));
        }
        Ok(self.insert(ObjectKind::Sampler, desc.label, Detail::None))
    }

    fn destroy_sampler(&self, sampler: SamplerHandle) {
        self.remove(ObjectKind::Sampler, sampler.to_bits());
    }

    // --- shaders and pipelines ---

    /// Accepts every format the seam defines and records which ones it was
    /// handed.
    ///
    /// The null backend compiles nothing, so it is the one backend with no
    /// reason to prefer a format — and that is what makes it the place a test
    /// asserts what a *caller* supplied. See
    /// [`Recorder::shader_module_sources`](crate::null::Recorder::shader_module_sources).
    fn create_shader_module(
        &self,
        desc: &ShaderModuleDesc<'_>,
    ) -> Result<ShaderModuleHandle, HalError> {
        let sources = desc.provided();
        if sources.is_empty() {
            // Every real backend rejects this too; saying so here means the
            // no-GPU suite catches a call site that stopped passing artifacts.
            return Err(desc.unusable(ShaderSources::all()));
        }
        // The SPIR-V magic number. Checking it costs nothing and catches the
        // classic "passed bytes where words were wanted" mistake, which
        // otherwise surfaces as a driver error at P1. Only when SPIR-V was
        // offered at all: a WGSL-only module is legal and has no magic number.
        const SPIRV_MAGIC: u32 = 0x0723_0203;
        if let Some(&word) = desc.spirv.first()
            && word != SPIRV_MAGIC
        {
            return Err(HalError::ShaderCompilation(format!(
                "not SPIR-V: first word is {word:#010x}, expected {SPIRV_MAGIC:#010x}"
            )));
        }
        self.recorder
            .lock()
            .shader_modules_created
            .push((desc.label.map(ToOwned::to_owned), sources));
        Ok(self.insert(
            ObjectKind::ShaderModule,
            desc.label,
            Detail::ShaderModule {
                dxil_entry_points: desc
                    .dxil
                    .iter()
                    .map(|(entry_point, _)| (*entry_point).to_owned())
                    .collect(),
            },
        ))
    }

    fn destroy_shader_module(&self, module: ShaderModuleHandle) {
        self.remove(ObjectKind::ShaderModule, module.to_bits());
    }

    fn create_bind_group_layout(
        &self,
        desc: &BindGroupLayoutDesc<'_>,
    ) -> Result<BindGroupLayoutHandle, HalError> {
        desc.check_entries(&self.caps, BackendKind::Null)?;
        let update_after_bind = desc
            .entries
            .iter()
            .any(|entry| entry.flags.contains(crate::BindingFlags::UPDATE_AFTER_BIND));
        let variable_binding = desc
            .entries
            .iter()
            .find(|entry| entry.flags.contains(crate::BindingFlags::VARIABLE_COUNT))
            .map(|entry| entry.binding);
        Ok(self.insert(
            ObjectKind::BindGroupLayout,
            desc.label,
            Detail::BindGroupLayout {
                entries: desc.entries.to_vec(),
                update_after_bind,
                variable_binding,
            },
        ))
    }

    fn destroy_bind_group_layout(&self, layout: BindGroupLayoutHandle) {
        self.remove(ObjectKind::BindGroupLayout, layout.to_bits());
    }

    fn create_bind_group(&self, desc: &BindGroupDesc<'_>) -> Result<BindGroupHandle, HalError> {
        self.check(
            ObjectKind::BindGroupLayout,
            desc.layout.to_bits(),
            "bind group layout",
        )?;
        let layout = self.layout_of(desc.layout)?;
        // `variable_count` is what the *pool* is sized with on a real backend,
        // so an over-large one fails `vkAllocateDescriptorSets` with a VUID
        // rather than the named error the rest of the seam produces.
        if let Some(count) = desc.variable_count {
            let Some(binding) = layout.variable_binding else {
                return Err(HalError::InvalidDescriptor(
                    "BindGroupDesc::variable_count on a layout with no VARIABLE_COUNT binding"
                        .to_string(),
                ));
            };
            let ceiling = layout.binding_ceiling(binding, &self.caps);
            if count > ceiling {
                return Err(HalError::InvalidDescriptor(format!(
                    "variable_count {count} exceeds the {ceiling} descriptors binding {binding} \
                     declares"
                )));
            }
        }
        self.check_entries_against(&layout, desc.entries)?;
        Ok(self.insert(
            ObjectKind::BindGroup,
            desc.label,
            Detail::BindGroup {
                layout: desc.layout,
                update_after_bind: layout.update_after_bind,
            },
        ))
    }

    fn update_bind_group(
        &self,
        group: BindGroupHandle,
        entries: &[BindGroupEntry],
    ) -> Result<(), HalError> {
        self.check(ObjectKind::BindGroup, group.to_bits(), "bind group")?;
        let (layout_handle, update_after_bind) = {
            let state = self.recorder.lock();
            let Some(object) = state.get(ObjectKind::BindGroup, group.to_bits()) else {
                return Err(HalError::invalid_handle("bind group", group));
            };
            let Detail::BindGroup {
                layout,
                update_after_bind,
            } = &object.detail
            else {
                unreachable!("a bind group handle always carries bind group detail");
            };
            (*layout, *update_after_bind)
        };
        if !update_after_bind {
            // The error `Device::update_bind_group` promises. Without the flag,
            // writing a set a pending command buffer might reference is
            // undefined behaviour, so refusing is the only safe answer — and
            // the null backend accepting it made the seam's own doc untestable.
            return Err(self.unsupported("update_bind_group on a layout without UPDATE_AFTER_BIND"));
        }
        let layout = self.layout_of(layout_handle)?;
        self.check_entries_against(&layout, entries)
    }

    fn destroy_bind_group(&self, group: BindGroupHandle) {
        self.remove(ObjectKind::BindGroup, group.to_bits());
    }

    fn create_pipeline_layout(
        &self,
        desc: &PipelineLayoutDesc<'_>,
    ) -> Result<PipelineLayoutHandle, HalError> {
        for layout in desc.bind_group_layouts {
            self.check(
                ObjectKind::BindGroupLayout,
                layout.to_bits(),
                "bind group layout",
            )?;
        }
        if desc.bind_group_layouts.len() > self.caps.limits.max_bind_groups as usize {
            return Err(HalError::InvalidDescriptor(format!(
                "{} bind groups exceeds max_bind_groups {}",
                desc.bind_group_layouts.len(),
                self.caps.limits.max_bind_groups
            )));
        }
        if let Some(range) = desc.push_constants {
            if !self.caps.features.contains(Features::PUSH_CONSTANTS) {
                // Loudly, at layout creation — not by silently dropping the
                // writes later. This is the trap the seam exists to make
                // visible.
                return Err(self.unsupported("push constants"));
            }
            range
                .stages
                .check_supported(self.caps.features, BackendKind::Null)?;
            // Saturating, not wrapping: `PushConstantRange { offset: u32::MAX,
            // size: 1 }` panicked in debug and wrapped to 0 in release, and 0
            // then *passed* the check it was supposed to fail.
            let end = range.offset.saturating_add(range.size);
            if end > self.caps.limits.max_push_constant_size {
                return Err(HalError::InvalidDescriptor(format!(
                    "push constant range ends at {end} but max_push_constant_size is {}",
                    self.caps.limits.max_push_constant_size
                )));
            }
        }
        Ok(self.insert(ObjectKind::PipelineLayout, desc.label, Detail::None))
    }

    fn destroy_pipeline_layout(&self, layout: PipelineLayoutHandle) {
        self.remove(ObjectKind::PipelineLayout, layout.to_bits());
    }

    fn create_graphics_pipeline(
        &self,
        desc: &GraphicsPipelineDesc<'_>,
    ) -> Result<GraphicsPipelineHandle, HalError> {
        self.check_stage(desc.vertex)?;
        self.check_raster_state(
            desc.layout,
            desc.fragment,
            desc.color_targets,
            desc.primitive,
            desc.depth_stencil,
        )?;
        Ok(self.insert(ObjectKind::GraphicsPipeline, desc.label, Detail::None))
    }

    fn create_mesh_pipeline(
        &self,
        desc: &MeshPipelineDesc<'_>,
    ) -> Result<GraphicsPipelineHandle, HalError> {
        // Before anything else, so a device modelling the absent capability
        // refuses for the reason the caller needs to hear rather than tripping
        // over a stale handle first.
        if !self.caps.features.contains(Features::MESH_SHADER) {
            return Err(self.unsupported("mesh shading without Features::MESH_SHADER"));
        }
        if desc.task.is_some() && !self.caps.features.contains(Features::TASK_SHADER) {
            return Err(self.unsupported("a task stage without Features::TASK_SHADER"));
        }
        self.check_stage(desc.mesh)?;
        if let Some(task) = desc.task {
            self.check_stage(task)?;
        }
        self.check_raster_state(
            desc.layout,
            desc.fragment,
            desc.color_targets,
            desc.primitive,
            desc.depth_stencil,
        )?;
        // The same pool as a raster pipeline, because the seam gives the two
        // one handle type — see `MeshPipelineDesc`'s decision note.
        Ok(self.insert(ObjectKind::GraphicsPipeline, desc.label, Detail::None))
    }

    fn destroy_graphics_pipeline(&self, pipeline: GraphicsPipelineHandle) {
        self.remove(ObjectKind::GraphicsPipeline, pipeline.to_bits());
    }

    fn create_compute_pipeline(
        &self,
        desc: &ComputePipelineDesc<'_>,
    ) -> Result<ComputePipelineHandle, HalError> {
        if !self.caps.features.contains(Features::COMPUTE) {
            return Err(self.unsupported("compute shaders"));
        }
        self.check(
            ObjectKind::PipelineLayout,
            desc.layout.to_bits(),
            "pipeline layout",
        )?;
        self.check_stage(desc.compute)?;
        // This recorder has no module contents to compare the declared size
        // against, so it checks the half that needs none: a size the device's
        // own limits could not launch.
        desc.check_workgroup_size(&self.caps.limits)?;
        Ok(self.insert(ObjectKind::ComputePipeline, desc.label, Detail::None))
    }

    fn destroy_compute_pipeline(&self, pipeline: ComputePipelineHandle) {
        self.remove(ObjectKind::ComputePipeline, pipeline.to_bits());
    }

    // --- queries ---

    fn create_query_set(&self, desc: &QuerySetDesc<'_>) -> Result<QuerySetHandle, HalError> {
        let required = match desc.kind {
            QueryKind::Timestamp => Features::TIMESTAMP_QUERY,
            QueryKind::Occlusion => Features::OCCLUSION_QUERY,
            QueryKind::PipelineStatistics => Features::PIPELINE_STATISTICS_QUERY,
        };
        if !self.caps.features.contains(required) {
            return Err(self.unsupported("this query kind"));
        }
        if desc.count == 0 {
            return Err(HalError::InvalidDescriptor(
                "query set must hold at least one query".to_string(),
            ));
        }
        Ok(self.insert(
            ObjectKind::QuerySet,
            desc.label,
            Detail::QuerySet { count: desc.count },
        ))
    }

    fn destroy_query_set(&self, set: QuerySetHandle) {
        self.remove(ObjectKind::QuerySet, set.to_bits());
    }

    fn query_results(
        &self,
        set: QuerySetHandle,
        first_query: u32,
        out: &mut [u64],
    ) -> Result<(), HalError> {
        self.check(ObjectKind::QuerySet, set.to_bits(), "query set")?;
        let count = {
            let state = self.recorder.lock();
            let Some(object) = state.get(ObjectKind::QuerySet, set.to_bits()) else {
                return Err(HalError::invalid_handle("query set", set));
            };
            let Detail::QuerySet { count } = &object.detail else {
                unreachable!("a query set handle always carries query set detail");
            };
            *count
        };
        // The range check `Device::query_results` documents. It was impossible
        // before: `first_query` was ignored and the set's size was not even
        // stored, so the null backend accepted ranges `crcbl-vk` rejects.
        let end = u64::from(first_query) + out.len() as u64;
        if end > u64::from(count) {
            return Err(HalError::InvalidDescriptor(format!(
                "query range {first_query}..{end} exceeds the set's {count} queries"
            )));
        }
        // Zeros, as a device without timestamp support would report: the
        // profiler HUD degrades rather than breaking.
        out.fill(0);
        Ok(())
    }

    // --- synchronisation ---

    fn create_semaphore(&self, desc: &SemaphoreDesc<'_>) -> Result<SemaphoreHandle, HalError> {
        if matches!(desc.kind, SemaphoreKind::Timeline { .. })
            && !self.caps.features.contains(Features::TIMELINE_SEMAPHORE)
        {
            return Err(self.unsupported("timeline semaphores"));
        }
        Ok(self.insert(ObjectKind::Semaphore, desc.label, Detail::None))
    }

    fn destroy_semaphore(&self, semaphore: SemaphoreHandle) {
        self.remove(ObjectKind::Semaphore, semaphore.to_bits());
    }

    fn semaphore_value(&self, semaphore: SemaphoreHandle) -> Result<u64, HalError> {
        self.check(ObjectKind::Semaphore, semaphore.to_bits(), "semaphore")?;
        // Nothing ever executes, so every submitted signal has already
        // "completed"; reporting the highest signalled value would need a
        // per-semaphore counter that no test has asked for yet.
        Ok(0)
    }

    fn wait_semaphores(&self, waits: &[SemaphoreWait], _timeout_ns: u64) -> Result<bool, HalError> {
        for wait in waits {
            self.check(ObjectKind::Semaphore, wait.semaphore.to_bits(), "semaphore")?;
        }
        // No work is ever outstanding, so every wait is satisfied immediately.
        Ok(true)
    }

    /// Records the wait and returns at once — nothing here is ever outstanding.
    ///
    /// The one method that names no handle and can still fail, which is why the
    /// device-loss gate is spelled out here rather than reached through
    /// [`NullDevice::check_lost`]'s two funnels. `vkDeviceWaitIdle` reports
    /// `VK_ERROR_DEVICE_LOST` for the same reason: waiting for a dead device to
    /// finish is not something that can succeed.
    fn wait_idle(&self) -> Result<(), HalError> {
        self.check_lost()?;
        self.recorder.lock().events.push(Event::WaitedIdle);
        Ok(())
    }

    // --- commands ---

    fn create_command_encoder(&self, desc: &CommandEncoderDesc<'_>) -> Box<dyn CommandEncoder> {
        Box::new(NullEncoder {
            recorder: self.recorder.clone(),
            device_id: self.device_id,
            label: desc.label.map(ToOwned::to_owned),
            commands: Vec::new(),
            scope: Scope::Outside,
            // There is no error path out of `create_command_encoder`, so a
            // queue that is not this device's is remembered and reported by
            // `finish` — exactly what `crcbl-vk` does when its command pool
            // cannot be created.
            failed: self.check_queue(desc.queue).err(),
        })
    }

    fn destroy_command_buffer(&self, buffer: CommandBufferHandle) {
        self.remove(ObjectKind::CommandBuffer, buffer.to_bits());
    }

    fn submit(&self, queue: QueueHandle, submit: &SubmitInfo<'_>) -> Result<(), HalError> {
        self.check_queue(queue)?;
        for buffer in submit.command_buffers {
            self.check(
                ObjectKind::CommandBuffer,
                buffer.to_bits(),
                "command buffer",
            )?;
        }
        for wait in submit.waits {
            self.check(ObjectKind::Semaphore, wait.semaphore.to_bits(), "semaphore")?;
        }
        for signal in submit.signals {
            self.check(
                ObjectKind::Semaphore,
                signal.semaphore.to_bits(),
                "semaphore",
            )?;
        }
        self.recorder.lock().events.push(Event::Submitted {
            queue,
            command_buffers: submit.command_buffers.to_vec(),
            waits: submit.waits.to_vec(),
            signals: submit.signals.to_vec(),
        });
        Ok(())
    }

    // --- presentation ---

    fn create_swapchain(&self, desc: &SwapchainDesc<'_>) -> Result<SwapchainHandle, SurfaceError> {
        self.check_instance_owned(ObjectKind::Surface, desc.surface.to_bits(), "surface")
            .map_err(SurfaceError::Hal)?;
        if desc.extent.0 == 0 || desc.extent.1 == 0 {
            return Err(SurfaceError::Hal(HalError::InvalidDescriptor(
                "swapchain extent must be non-zero".to_string(),
            )));
        }
        let ring = self.build_ring(desc)?;
        Ok(self.insert(ObjectKind::Swapchain, desc.label, ring))
    }

    fn reconfigure_swapchain(
        &self,
        swapchain: SwapchainHandle,
        desc: &SwapchainDesc<'_>,
    ) -> Result<(), SurfaceError> {
        // First, so a faulted call records nothing and changes nothing — the
        // property `GpuContext::set_pacing`'s rollback asserts on.
        if self.recorder.take_reconfigure_failure() {
            return Err(SurfaceError::Hal(HalError::OutOfDeviceMemory));
        }
        if desc.extent.0 == 0 || desc.extent.1 == 0 {
            return Err(SurfaceError::Hal(HalError::InvalidDescriptor(
                "swapchain extent must be non-zero".to_string(),
            )));
        }
        self.check(ObjectKind::Swapchain, swapchain.to_bits(), "swapchain")
            .map_err(SurfaceError::Hal)?;

        // Reissue the whole ring — images, views *and* semaphores — which is
        // what a real backend does, since `crcbl-vk` recreates the
        // `VkSwapchainKHR`. A caller that wrongly held one of those handles
        // across a resize therefore fails here, on the backend that needs no
        // GPU, rather than on the one that does.
        //
        // Built **before** anything old is destroyed. The other order looks
        // tidier and is wrong: `build_ring` can fail (an extent past
        // `max_image_2d`), and a failed reconfigure that had already destroyed
        // the old ring would leave the swapchain naming dead handles — a call
        // that returned `Err` having broken the object it was given.
        let fresh = self.build_ring(desc)?;

        let stale = {
            let mut state = self.recorder.lock();
            let Some(object) = state.get_mut(ObjectKind::Swapchain, swapchain.to_bits()) else {
                return Err(SurfaceError::Hal(HalError::invalid_handle(
                    "swapchain",
                    swapchain,
                )));
            };
            let previous = core::mem::replace(&mut object.detail, fresh);
            // The rebuild is what clears the out-of-date latch, so a caller
            // that handles the resize gets a working swapchain back and one
            // that ignores it keeps being told. Here rather than at the top of
            // the method: a reconfigure that failed rebuilt nothing.
            state.swapchain_out_of_date = false;
            state.events.push(Event::Reconfigured {
                swapchain,
                extent: desc.extent,
            });
            previous
        };

        let Detail::Swapchain {
            images,
            views,
            acquire_semaphores,
            present_semaphores,
            ..
        } = stale
        else {
            unreachable!("a swapchain handle always carries swapchain detail");
        };
        // Views before images: a view must not outlive what it views.
        for view in views {
            self.destroy_image_view(view);
        }
        for image in images {
            self.destroy_image(image);
        }
        for semaphore in acquire_semaphores
            .into_iter()
            .chain(present_semaphores)
            .flatten()
        {
            self.destroy_semaphore(semaphore);
        }
        Ok(())
    }

    fn destroy_swapchain(&self, swapchain: SwapchainHandle) {
        let owned = {
            let state = self.recorder.lock();
            match state
                .get(ObjectKind::Swapchain, swapchain.to_bits())
                .map(|object| &object.detail)
            {
                Some(Detail::Swapchain {
                    images,
                    views,
                    acquire_semaphores,
                    present_semaphores,
                    ..
                }) => Some((
                    images.clone(),
                    views.clone(),
                    acquire_semaphores.clone(),
                    present_semaphores.clone(),
                )),
                _ => None,
            }
        };
        if let Some((images, views, acquires, presents)) = owned {
            // Views before images: the seam says every view of an image must
            // already be destroyed when the image is.
            for view in views {
                self.destroy_image_view(view);
            }
            for image in images {
                self.destroy_image(image);
            }
            for semaphore in acquires.into_iter().chain(presents).flatten() {
                self.destroy_semaphore(semaphore);
            }
        }
        self.remove(ObjectKind::Swapchain, swapchain.to_bits());
    }

    fn acquire_next_frame(
        &self,
        swapchain: SwapchainHandle,
    ) -> Result<AcquiredFrame, SurfaceError> {
        self.check_current(swapchain)?;
        let mut state = self.recorder.lock();
        let Some(object) = state.get_mut(ObjectKind::Swapchain, swapchain.to_bits()) else {
            return Err(SurfaceError::Hal(HalError::invalid_handle(
                "swapchain",
                swapchain,
            )));
        };
        let Detail::Swapchain {
            extent,
            images,
            views,
            acquire_semaphores,
            present_semaphores,
            next,
        } = &mut object.detail
        else {
            unreachable!("a swapchain handle always carries swapchain detail");
        };
        let index = *next;
        let slot = index as usize;
        // `.max(1)`, not `unwrap_or(1)`: `u32::try_from(0usize)` is `Ok(0)`, so
        // the fallback never fired for the one input it looked like it guarded
        // against, and an empty ring divided by zero.
        *next = (index + 1) % u32::try_from(images.len()).unwrap_or(u32::MAX).max(1);
        let frame = AcquiredFrame {
            image: images[slot],
            view: views[slot],
            // This backend has no window system to clamp against, so the
            // configured extent is always the requested one — which is exactly
            // the case obligation 3 says costs nothing to obey.
            extent: *extent,
            index,
            acquire_semaphore: acquire_semaphores[slot],
            present_semaphore: present_semaphores[slot],
            suboptimal: false,
        };
        state.events.push(Event::Acquired { swapchain, index });
        Ok(frame)
    }

    fn present(&self, queue: QueueHandle, present: &PresentInfo<'_>) -> Result<(), SurfaceError> {
        self.check_queue(queue).map_err(SurfaceError::Hal)?;
        self.check_current(present.swapchain)?;
        for semaphore in present.waits {
            self.check(ObjectKind::Semaphore, semaphore.to_bits(), "semaphore")
                .map_err(SurfaceError::Hal)?;
        }
        let mut state = self.recorder.lock();
        state.events.push(Event::Presented {
            swapchain: present.swapchain,
            present_id: present.present_id,
        });
        Ok(())
    }

    /// Records the request and returns at once.
    ///
    /// There is no display under this backend, so there is nothing a wait could
    /// observe — which is exactly the seam's answer for a device that does not
    /// advertise [`Features::PRESENT_FEEDBACK`], and this one never does. The
    /// handle is still checked, because a caller waiting on a swapchain it
    /// already destroyed has a bug whether or not anyone was going to block —
    /// and so is the out-of-date latch, because a pacing wait is one of the
    /// three calls a resize makes fail. See [`check_current`](Self::check_current).
    fn wait_until_presented(
        &self,
        swapchain: SwapchainHandle,
        present_id: u64,
        _timeout: Duration,
    ) -> Result<(), SurfaceError> {
        self.check_current(swapchain)?;
        let mut state = self.recorder.lock();
        state.events.push(Event::PresentWaited {
            swapchain,
            present_id,
        });
        Ok(())
    }

    /// Always [`DisplayTiming::Unknown`], and the handle is checked first.
    ///
    /// There is no display under this backend to have a cadence, so this device
    /// never advertises [`Features::PRESENT_TIMING`] and the seam's answer for
    /// a device without it is the honest one here. The check is the same
    /// obligation-3 lookup every other method does: asking about a destroyed or
    /// foreign swapchain is a caller bug whether or not there was an answer to
    /// give.
    fn display_timing(&self, swapchain: SwapchainHandle) -> Result<DisplayTiming, SurfaceError> {
        self.check(ObjectKind::Swapchain, swapchain.to_bits(), "swapchain")
            .map_err(SurfaceError::Hal)?;
        Ok(DisplayTiming::Unknown)
    }
}

impl NullDevice {
    /// Builds a complete swapchain ring: one image, one view and — unless this
    /// device models an implicit acquire — one acquire and one present
    /// semaphore per ring slot.
    ///
    /// Shared by create and reconfigure so the four vectors can never end up
    /// with different lengths. They did once: reconfigure used to replace only
    /// the images and views, so reconfiguring from two images to three left
    /// two-element semaphore vectors and the third acquire indexed past the end
    /// of them. The lengths are an invariant, so one constructor establishes it.
    fn build_ring(&self, desc: &SwapchainDesc<'_>) -> Result<Detail, SurfaceError> {
        // `SwapchainDesc::format` must be one of `SurfaceCaps::formats`, and
        // this backend used to accept anything at all — including `D32Float`,
        // which no surface can present.
        if !SURFACE_FORMATS.contains(&desc.format) {
            return Err(SurfaceError::Hal(HalError::InvalidDescriptor(format!(
                "{:?} is not one of this surface's formats {SURFACE_FORMATS:?}",
                desc.format
            ))));
        }
        // Clamped to what `surface_caps` reports, not to a pair of literals
        // that happened to agree with it.
        let image_count = desc.image_count.clamp(RING_MIN_IMAGES, RING_MAX_IMAGES);
        let mut images = Vec::with_capacity(image_count as usize);
        let mut views = Vec::with_capacity(image_count as usize);
        let mut acquire_semaphores = Vec::with_capacity(image_count as usize);
        let mut present_semaphores = Vec::with_capacity(image_count as usize);
        for index in 0..image_count {
            let image = self
                .create_image(&ImageDesc {
                    label: Some("swapchain image"),
                    image_type: ImageType::D2,
                    extent: crate::Extent3d::d2(desc.extent.0, desc.extent.1),
                    format: desc.format,
                    mip_levels: 1,
                    samples: 1,
                    usage: ImageUsage::COLOR_ATTACHMENT | ImageUsage::PRESENT,
                    memory: MemoryLocation::DeviceLocal,
                })
                .map_err(SurfaceError::Hal)?;
            images.push(image);
            // The swapchain owns its images, so it owns their views — see
            // `crate::swapchain`. A caller never creates or destroys one.
            views.push(
                self.create_image_view(&ImageViewDesc {
                    label: Some("swapchain image view"),
                    image,
                    view_type: crate::ImageViewType::D2,
                    format: desc.format,
                    range: crate::ImageSubresourceRange::all(desc.format),
                })
                .map_err(SurfaceError::Hal)?,
            );
            if self.implicit_acquire {
                // The WebGPU shape: no semaphores at all, so the renderer's
                // wait/signal splice is a no-op rather than a branch.
                acquire_semaphores.push(None);
                present_semaphores.push(None);
            } else {
                let label = format!("swapchain acquire {index}");
                acquire_semaphores.push(Some(self.insert::<crate::Semaphore>(
                    ObjectKind::Semaphore,
                    Some(&label),
                    Detail::None,
                )));
                let label = format!("swapchain present {index}");
                present_semaphores.push(Some(self.insert::<crate::Semaphore>(
                    ObjectKind::Semaphore,
                    Some(&label),
                    Detail::None,
                )));
            }
        }
        Ok(Detail::Swapchain {
            extent: desc.extent,
            images,
            views,
            acquire_semaphores,
            present_semaphores,
            next: 0,
        })
    }

    /// Snapshots a bind-group layout, so validation can run without holding the
    /// recorder lock across the resource lookups it also needs.
    fn layout_of(&self, handle: BindGroupLayoutHandle) -> Result<LayoutRecord, HalError> {
        let state = self.recorder.lock();
        let Some(object) = state.get(ObjectKind::BindGroupLayout, handle.to_bits()) else {
            return Err(HalError::invalid_handle("bind group layout", handle));
        };
        let Detail::BindGroupLayout {
            entries,
            update_after_bind,
            variable_binding,
        } = &object.detail
        else {
            unreachable!("a layout handle always carries layout detail");
        };
        Ok(LayoutRecord {
            entries: entries.clone(),
            update_after_bind: *update_after_bind,
            variable_binding: *variable_binding,
        })
    }

    /// Checks every assignment against the layout it claims to fill.
    ///
    /// The three things a real backend checks and this one used not to: that
    /// the binding exists, that the array index is inside it, and that the
    /// resource is the right *kind*. Without them the null backend green-lit
    /// bind groups `crcbl-vk` rejects outright.
    fn check_entries_against(
        &self,
        layout: &LayoutRecord,
        entries: &[BindGroupEntry],
    ) -> Result<(), HalError> {
        for entry in entries {
            let Some(declared) = layout
                .entries
                .iter()
                .find(|slot| slot.binding == entry.binding)
            else {
                return Err(HalError::InvalidDescriptor(format!(
                    "bind group entry names binding {}, which the layout does not declare",
                    entry.binding
                )));
            };
            let ceiling = layout.binding_ceiling(entry.binding, &self.caps);
            if entry.array_index >= ceiling {
                return Err(HalError::InvalidDescriptor(format!(
                    "bind group entry writes index {} of binding {}, which holds {ceiling}",
                    entry.array_index, entry.binding
                )));
            }
            check_resource_kind(declared.kind, &entry.resource, entry.binding)?;
            self.check_binding(&entry.resource)?;
        }
        Ok(())
    }

    fn check_binding(&self, resource: &crate::BindingResource) -> Result<(), HalError> {
        match resource {
            crate::BindingResource::Buffer { buffer, .. } => {
                self.check(ObjectKind::Buffer, buffer.to_bits(), "buffer")
            }
            crate::BindingResource::ImageView(view) => {
                self.check(ObjectKind::ImageView, view.to_bits(), "image view")
            }
            crate::BindingResource::Sampler(sampler) => {
                self.check(ObjectKind::Sampler, sampler.to_bits(), "sampler")
            }
        }
    }
}

/// A bind-group layout's declarations, copied out from under the lock.
#[derive(Debug)]
struct LayoutRecord {
    entries: Vec<crate::BindGroupLayoutEntry>,
    update_after_bind: bool,
    variable_binding: Option<u32>,
}

impl LayoutRecord {
    /// How many descriptors a binding actually holds.
    ///
    /// [`BindingFlags::VARIABLE_COUNT`](crate::BindingFlags::VARIABLE_COUNT)
    /// makes the declared `count` an upper bound, and
    /// [`BindGroupLayoutEntry::resolved_count`](crate::BindGroupLayoutEntry::resolved_count)
    /// is where the seam's `u32::MAX` becomes a number. Only the variable
    /// binding is resolved: every other binding holds exactly the length it
    /// declared, which is the bound a caller's `array_index` has to clear.
    fn binding_ceiling(&self, binding: u32, caps: &DeviceCaps) -> u32 {
        let Some(entry) = self.entries.iter().find(|slot| slot.binding == binding) else {
            return 0;
        };
        if self.variable_binding == Some(binding) {
            entry.resolved_count(&caps.limits)
        } else {
            entry.count
        }
    }
}

/// Whether a resource can fill a binding of that kind.
///
/// The same table `crcbl-vk`'s `check_resource_kind` uses, and for the same
/// reason: a mismatch must name the caller's binding number, not a descriptor
/// type in a driver message.
fn check_resource_kind(
    kind: crate::BindingKind,
    resource: &crate::BindingResource,
    binding: u32,
) -> Result<(), HalError> {
    use crate::{BindingKind as K, BindingResource as R};
    let ok = matches!(
        (kind, resource),
        (
            K::UniformBuffer { .. } | K::StorageBuffer { .. },
            R::Buffer { .. }
        ) | (
            K::SampledImage { .. } | K::StorageImage { .. },
            R::ImageView(_)
        ) | (K::Sampler, R::Sampler(_))
    );
    if ok {
        Ok(())
    } else {
        Err(HalError::InvalidDescriptor(format!(
            "binding {binding} is declared as {kind:?}, which cannot hold {resource:?}"
        )))
    }
}

/// Which pass, if any, is currently open on an encoder.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Scope {
    Outside,
    Render,
    Compute,
}

/// A no-op, recording [`CommandEncoder`].
#[derive(Debug)]
struct NullEncoder {
    recorder: Recorder,
    /// Owner id every handle a recorded command names must match.
    device_id: u64,
    label: Option<String>,
    commands: Vec<Command>,
    scope: Scope,
    /// The first hard failure, reported by [`CommandEncoder::finish`]. Distinct
    /// from a [`ValidationError`], which is recorded and carried past.
    failed: Option<HalError>,
}

impl NullEncoder {
    /// Records `command` after checking it is legal in the current scope.
    fn record(&mut self, command: Command) {
        let name = command.name();
        match &command {
            Command::BeginRenderPass { .. } | Command::BeginComputePass { .. } => {
                if self.scope != Scope::Outside {
                    self.fail(ValidationError::NestedPass { command: name });
                }
                self.scope = match command {
                    Command::BeginComputePass { .. } => Scope::Compute,
                    _ => Scope::Render,
                };
            }
            Command::EndRenderPass => {
                if self.scope != Scope::Render {
                    self.fail(ValidationError::UnopenedPass { command: name });
                }
                self.scope = Scope::Outside;
            }
            Command::EndComputePass => {
                if self.scope != Scope::Compute {
                    self.fail(ValidationError::UnopenedPass { command: name });
                }
                self.scope = Scope::Outside;
            }
            _ => {}
        }
        self.commands.push(command);
    }

    fn fail(&self, error: ValidationError) {
        self.recorder.push_validation(error);
    }

    /// Requires an open render pass.
    fn need_render(&self, command: &'static str) {
        if self.scope != Scope::Render {
            self.fail(ValidationError::OutsidePass {
                command,
                expected: "render",
            });
        }
    }

    /// Requires an open compute pass.
    fn need_compute(&self, command: &'static str) {
        if self.scope != Scope::Compute {
            self.fail(ValidationError::OutsidePass {
                command,
                expected: "compute",
            });
        }
    }

    /// Requires some pass to be open — bindings are legal in either.
    fn need_any_pass(&self, command: &'static str) {
        if self.scope == Scope::Outside {
            self.fail(ValidationError::OutsidePass {
                command,
                expected: "render or compute",
            });
        }
    }

    /// Requires no pass to be open. Copies, barriers and query writes are
    /// forbidden inside dynamic rendering.
    fn need_outside(&self, command: &'static str) {
        if self.scope != Scope::Outside {
            self.fail(ValidationError::InsidePass { command });
        }
    }

    /// Requires a handle to still resolve *and* to belong to this device.
    fn need_live(&self, command: &'static str, kind: ObjectKind, bits: u64) {
        let state = self.recorder.lock();
        if state.is_owned_by(kind, bits, self.device_id) {
            return;
        }
        let error = if state.is_live(kind, bits) {
            ValidationError::ForeignHandle {
                command,
                kind,
                bits,
            }
        } else {
            ValidationError::DeadHandle {
                command,
                kind,
                bits,
            }
        };
        drop(state);
        self.fail(error);
    }
}

impl CommandEncoder for NullEncoder {
    fn begin_debug_label(&mut self, label: &str) {
        self.record(Command::BeginDebugLabel(label.to_string()));
    }

    fn end_debug_label(&mut self) {
        self.record(Command::EndDebugLabel);
    }

    fn insert_debug_marker(&mut self, label: &str) {
        self.record(Command::DebugMarker(label.to_string()));
    }

    fn pipeline_barrier(&mut self, barriers: &Barriers<'_>) {
        self.need_outside("Barrier");
        for barrier in barriers.buffers {
            self.need_live("Barrier", ObjectKind::Buffer, barrier.buffer.to_bits());
        }
        for barrier in barriers.images {
            self.need_live("Barrier", ObjectKind::Image, barrier.image.to_bits());
        }
        self.record(Command::Barrier {
            buffers: barriers.buffers.to_vec(),
            images: barriers.images.to_vec(),
            global: barriers.global,
        });
    }

    fn copy_buffer_to_buffer(&mut self, copy: &BufferCopy) {
        self.need_outside("CopyBufferToBuffer");
        self.need_live("CopyBufferToBuffer", ObjectKind::Buffer, copy.src.to_bits());
        self.need_live("CopyBufferToBuffer", ObjectKind::Buffer, copy.dst.to_bits());
        self.record(Command::CopyBufferToBuffer(*copy));
    }

    fn copy_buffer_to_image(&mut self, copy: &BufferImageCopy) {
        self.need_outside("CopyBufferToImage");
        self.need_live(
            "CopyBufferToImage",
            ObjectKind::Buffer,
            copy.buffer.to_bits(),
        );
        self.need_live("CopyBufferToImage", ObjectKind::Image, copy.image.to_bits());
        self.record(Command::CopyBufferToImage(*copy));
    }

    fn copy_image_to_buffer(&mut self, copy: &BufferImageCopy) {
        self.need_outside("CopyImageToBuffer");
        self.need_live("CopyImageToBuffer", ObjectKind::Image, copy.image.to_bits());
        self.need_live(
            "CopyImageToBuffer",
            ObjectKind::Buffer,
            copy.buffer.to_bits(),
        );
        self.record(Command::CopyImageToBuffer(*copy));
    }

    fn copy_image_to_image(&mut self, copy: &ImageCopy) {
        self.need_outside("CopyImageToImage");
        self.need_live("CopyImageToImage", ObjectKind::Image, copy.src.to_bits());
        self.need_live("CopyImageToImage", ObjectKind::Image, copy.dst.to_bits());
        self.record(Command::CopyImageToImage(*copy));
    }

    fn fill_buffer(&mut self, buffer: BufferHandle, offset: u64, size: u64, value: u32) {
        self.need_outside("FillBuffer");
        self.need_live("FillBuffer", ObjectKind::Buffer, buffer.to_bits());
        self.record(Command::FillBuffer {
            buffer,
            offset,
            size,
            value,
        });
    }

    fn begin_render_pass(&mut self, desc: &RenderPassDesc<'_>) {
        for attachment in desc.color_attachments {
            self.need_live(
                "BeginRenderPass",
                ObjectKind::ImageView,
                attachment.view.to_bits(),
            );
            if let Some(resolve) = attachment.resolve {
                self.need_live("BeginRenderPass", ObjectKind::ImageView, resolve.to_bits());
            }
        }
        if let Some(attachment) = desc.depth_stencil_attachment {
            self.need_live(
                "BeginRenderPass",
                ObjectKind::ImageView,
                attachment.view.to_bits(),
            );
        }
        self.record(Command::BeginRenderPass {
            label: desc.label.map(ToOwned::to_owned),
            color_attachments: desc.color_attachments.to_vec(),
            depth_stencil_attachment: desc.depth_stencil_attachment,
            render_area: desc.render_area,
        });
    }

    fn end_render_pass(&mut self) {
        self.record(Command::EndRenderPass);
    }

    fn set_viewport(&mut self, viewport: &Viewport) {
        self.need_render("SetViewport");
        self.record(Command::SetViewport(*viewport));
    }

    fn set_scissor(&mut self, rect: &Rect2d) {
        self.need_render("SetScissor");
        self.record(Command::SetScissor(*rect));
    }

    fn set_stencil_reference(&mut self, reference: u32) {
        self.need_render("SetStencilReference");
        self.record(Command::SetStencilReference(reference));
    }

    fn bind_graphics_pipeline(&mut self, pipeline: GraphicsPipelineHandle) {
        self.need_render("BindGraphicsPipeline");
        self.need_live(
            "BindGraphicsPipeline",
            ObjectKind::GraphicsPipeline,
            pipeline.to_bits(),
        );
        self.record(Command::BindGraphicsPipeline(pipeline));
    }

    fn bind_index_buffer(&mut self, buffer: BufferHandle, offset: u64, format: IndexFormat) {
        self.need_render("BindIndexBuffer");
        self.need_live("BindIndexBuffer", ObjectKind::Buffer, buffer.to_bits());
        self.record(Command::BindIndexBuffer {
            buffer,
            offset,
            format,
        });
    }

    fn bind_group(
        &mut self,
        slot: u32,
        group: BindGroupHandle,
        dynamic_offsets: &[u32],
        _layout: PipelineLayoutHandle,
    ) {
        self.need_any_pass("BindGroup");
        self.need_live("BindGroup", ObjectKind::BindGroup, group.to_bits());
        self.record(Command::BindGroup {
            slot,
            group,
            dynamic_offsets: dynamic_offsets.to_vec(),
        });
    }

    fn push_constants(
        &mut self,
        stages: ShaderStages,
        offset: u32,
        data: &[u8],
        _layout: PipelineLayoutHandle,
    ) {
        self.need_any_pass("PushConstants");
        self.record(Command::PushConstants {
            stages,
            offset,
            len: data.len(),
        });
    }

    fn draw(&mut self, vertices: Range<u32>, instances: Range<u32>) {
        self.need_render("Draw");
        self.record(Command::Draw {
            vertices,
            instances,
        });
    }

    fn draw_indexed(&mut self, indices: Range<u32>, base_vertex: i32, instances: Range<u32>) {
        self.need_render("DrawIndexed");
        self.record(Command::DrawIndexed {
            indices,
            base_vertex,
            instances,
        });
    }

    fn draw_indirect(&mut self, draw: &DrawIndirect) {
        self.need_render("DrawIndirect");
        self.need_live("DrawIndirect", ObjectKind::Buffer, draw.args.to_bits());
        self.record(Command::DrawIndirect(*draw));
    }

    fn draw_indexed_indirect(&mut self, draw: &DrawIndirect) {
        self.need_render("DrawIndexedIndirect");
        self.need_live(
            "DrawIndexedIndirect",
            ObjectKind::Buffer,
            draw.args.to_bits(),
        );
        self.record(Command::DrawIndexedIndirect(*draw));
    }

    fn draw_indirect_count(&mut self, draw: &DrawIndirectCount) {
        self.need_render("DrawIndirectCount");
        self.need_live("DrawIndirectCount", ObjectKind::Buffer, draw.args.to_bits());
        self.need_live(
            "DrawIndirectCount",
            ObjectKind::Buffer,
            draw.count_buffer.to_bits(),
        );
        self.record(Command::DrawIndirectCount(*draw));
    }

    fn draw_indexed_indirect_count(&mut self, draw: &DrawIndirectCount) {
        self.need_render("DrawIndexedIndirectCount");
        self.need_live(
            "DrawIndexedIndirectCount",
            ObjectKind::Buffer,
            draw.args.to_bits(),
        );
        self.need_live(
            "DrawIndexedIndirectCount",
            ObjectKind::Buffer,
            draw.count_buffer.to_bits(),
        );
        self.record(Command::DrawIndexedIndirectCount(*draw));
    }

    fn draw_mesh_tasks(&mut self, x: u32, y: u32, z: u32) {
        // A draw, so `need_render` rather than `need_compute` — the three
        // counts are workgroups, but they are workgroups of a stage that emits
        // fragments.
        self.need_render("DrawMeshTasks");
        self.record(Command::DrawMeshTasks { x, y, z });
    }

    fn begin_compute_pass(&mut self, desc: &ComputePassDesc<'_>) {
        self.record(Command::BeginComputePass {
            label: desc.label.map(ToOwned::to_owned),
        });
    }

    fn end_compute_pass(&mut self) {
        self.record(Command::EndComputePass);
    }

    fn bind_compute_pipeline(&mut self, pipeline: ComputePipelineHandle) {
        self.need_compute("BindComputePipeline");
        self.need_live(
            "BindComputePipeline",
            ObjectKind::ComputePipeline,
            pipeline.to_bits(),
        );
        self.record(Command::BindComputePipeline(pipeline));
    }

    fn dispatch(&mut self, x: u32, y: u32, z: u32) {
        self.need_compute("Dispatch");
        self.record(Command::Dispatch { x, y, z });
    }

    fn dispatch_indirect(&mut self, args: BufferHandle, offset: u64) {
        self.need_compute("DispatchIndirect");
        self.need_live("DispatchIndirect", ObjectKind::Buffer, args.to_bits());
        self.record(Command::DispatchIndirect { args, offset });
    }

    fn reset_query_set(&mut self, set: QuerySetHandle, range: Range<u32>) {
        self.need_outside("ResetQuerySet");
        self.need_live("ResetQuerySet", ObjectKind::QuerySet, set.to_bits());
        self.record(Command::ResetQuerySet { set, range });
    }

    fn write_timestamp(&mut self, set: QuerySetHandle, index: u32) {
        self.need_outside("WriteTimestamp");
        self.need_live("WriteTimestamp", ObjectKind::QuerySet, set.to_bits());
        self.record(Command::WriteTimestamp { set, index });
    }

    fn resolve_query_set(
        &mut self,
        set: QuerySetHandle,
        range: Range<u32>,
        dst: BufferHandle,
        dst_offset: u64,
    ) {
        self.need_outside("ResolveQuerySet");
        self.need_live("ResolveQuerySet", ObjectKind::QuerySet, set.to_bits());
        self.need_live("ResolveQuerySet", ObjectKind::Buffer, dst.to_bits());
        self.record(Command::ResolveQuerySet {
            set,
            range,
            dst,
            dst_offset,
        });
    }

    fn finish(self: Box<Self>) -> Result<CommandBufferHandle, HalError> {
        if let Some(error) = self.failed {
            return Err(error);
        }
        if self.scope != Scope::Outside {
            // Recorded *and* returned. The seam's `finish` doc names an
            // unclosed pass as an `Err` case and `crcbl-vk` returns one, so a
            // null backend that answered `Ok` let CI green-light a command
            // stream Vulkan refuses to end at all — `vkEndCommandBuffer` with
            // a `vkCmdBeginRendering` outstanding is itself illegal.
            self.fail(ValidationError::UnclosedPass);
            return Err(HalError::InvalidDescriptor(
                "finish with a pass still open".to_string(),
            ));
        }
        let mut state = self.recorder.lock();
        let command_buffer: CommandBufferHandle = state.insert_owned(
            ObjectKind::CommandBuffer,
            self.device_id,
            self.label.clone(),
            Detail::None,
        );
        for command in self.commands {
            state.events.push(Event::Command {
                command_buffer,
                command,
            });
        }
        state.events.push(Event::Finished {
            label: self.label,
            command_buffer,
        });
        Ok(command_buffer)
    }
}

#[cfg(test)]
mod tests;
