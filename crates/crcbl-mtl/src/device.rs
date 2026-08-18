//! [`MetalDevice`]: one `MTLDevice`, its queue, and the four resource tables
//! this slice fills.
//!
//! # What this slice implements, and what it still refuses
//!
//! Buffers, images, image views and samplers — created, destroyed, and looked
//! up through generational handles — plus [`Device::backend`],
//! [`Device::caps`], [`Device::queue`], [`Device::write_buffer`] and
//! [`Device::wait_idle`]. Everything else on the trait refuses with
//! [`HalError::Unsupported`] whose `what` names the slice it arrives in, in the
//! same voice `MetalInstance` established. Nothing here is a stub that reports
//! success.
//!
//! # One lock over every table
//!
//! The seam takes `&self` everywhere so a device can be shared behind an `Arc`,
//! which means a backend owes its own interior synchronisation. This one uses a
//! single [`Mutex`] over all four pools, which is the same call the Vulkan
//! backend made and for the same reason: the traffic is a few dozen operations
//! per frame, and a lock-per-table scheme has a deadlock-ordering problem to
//! design before it has a contention problem to solve.
//!
//! # There is no deletion queue here, and that is not an oversight
//!
//! `crcbl-vk` parks every destroyed object on a timeline-keyed retire queue,
//! because `vkDestroyBuffer` while the GPU is reading is undefined behaviour.
//! Metal does not have that hazard: an `MTLCommandBuffer` retains every
//! resource it references, so releasing the last handle to a buffer the GPU is
//! still reading frees it *after* the command buffer completes, not during.
//! `destroy_*` therefore drops the `Retained` and is done. When the command
//! slice adds `commandBufferWithUnretainedReferences` — the opt-out — it
//! reintroduces the hazard and must reintroduce the queue with it.

use std::sync::{Arc, Mutex, MutexGuard, Once};
use std::time::{Duration, Instant};

use crcbl_core::{Handle, Pool};
use crcbl_hal::{
    AcquiredFrame, BackendKind, BindGroupDesc, BindGroupEntry, BindGroupHandle,
    BindGroupLayoutDesc, BindGroupLayoutHandle, BufferDesc, BufferHandle, BufferUsage, Capability,
    CommandBufferHandle, CommandEncoder, CommandEncoderDesc, ComputePipelineDesc,
    ComputePipelineHandle, Device, DeviceCaps, DeviceDesc, DisplayTiming, Features, Format,
    GraphicsPipelineDesc, GraphicsPipelineHandle, HalError, ImageDesc, ImageHandle, ImageType,
    ImageViewDesc, ImageViewHandle, MemoryLocation, PipelineLayoutDesc, PipelineLayoutHandle,
    PresentInfo, QueryKind, QuerySetDesc, QuerySetHandle, QueueHandle, QueueKind, ReadbackDesc,
    ReadbackHandle, ReadbackState, SamplerDesc, SamplerHandle, SemaphoreDesc, SemaphoreHandle,
    SemaphoreKind, SemaphoreWait, ShaderModuleDesc, ShaderModuleHandle, SubmitInfo, Support,
    SurfaceError, SwapchainDesc, SwapchainHandle,
};
use objc2::rc::Retained;
use objc2::runtime::ProtocolObject;
use objc2_foundation::{NSRange, NSString, NSUInteger};
use objc2_metal::{
    MTLArgumentBuffersTier, MTLBuffer, MTLCommandBuffer, MTLCommandBufferStatus, MTLCommandQueue,
    MTLDepthStencilState, MTLDevice, MTLEvent, MTLResource, MTLSamplerDescriptor, MTLSamplerState,
    MTLSharedEvent, MTLTexture, MTLTextureDescriptor,
};

use crate::command::MetalCommandEncoder;
use crate::conv;
use crate::instance::{AdapterRecord, InstanceInner, next_owner_id};

/// Largest anisotropy `MTLSamplerDescriptor::maxAnisotropy` accepts.
///
/// Fixed by the Metal API rather than by any device — the property is
/// documented as taking a value in `1...16` and Metal raises outside it — which
/// is why it is a constant here and why
/// [`Limits::max_sampler_anisotropy`](crcbl_hal::Limits::max_sampler_anisotropy)
/// reports it unconditionally.
pub(crate) const MAX_SAMPLER_ANISOTROPY: f32 = 16.0;

/// Why a [`QueryKind::Timestamp`](crcbl_hal::QueryKind::Timestamp) or
/// [`QueryKind::PipelineStatistics`](crcbl_hal::QueryKind::PipelineStatistics)
/// set is refused.
///
/// One constant because [`Device::supports`] and [`Device::create_query_set`]
/// must not drift: the declaration and the refusal are the same claim, and this
/// crate has had them disagree before.
///
/// **Both kinds need an `MTLCounterSampleBuffer`, and the occlusion kind does
/// not** — see `crate::query` — which is the whole of why one of the three is
/// served and two are not.
///
/// **Two separate obstacles, and this refuses on the first, which holds
/// everywhere.** [`Device::create_query_set`] matches on
/// [`QuerySetDesc::kind`](crcbl_hal::QuerySetDesc) alone and never asks the
/// device anything, so the refusal is the same on every Mac — and the reason it
/// is the same is the one `crate::adapter`'s `features_of` gives for withholding
/// [`Features::TIMESTAMP_QUERY`](crcbl_hal::Features::TIMESTAMP_QUERY) and
/// [`Features::PIPELINE_STATISTICS_QUERY`](crcbl_hal::Features::PIPELINE_STATISTICS_QUERY):
/// reporting either obliges a
/// [`Limits::timestamp_period_ns`](crcbl_hal::Limits::timestamp_period_ns), and
/// Metal correlates the GPU clock to the host's through
/// `sampleTimestamps:gpuTimestamp:` at sample time rather than ticking at a
/// fixed period, so there is no honest number to report and this workspace will
/// not fabricate one.
///
/// The device-specific fact is the *second* obstacle and is why CI cannot even
/// measure its way out: an `MTLCounterSampleBufferDescriptor` must name a set
/// from `MTLDevice::counterSets`, and the paravirtual GPU this backend's CI runs
/// on reports **zero** of them while answering `supportsCounterSampling:` yes at
/// three boundaries. `crate::adapter`'s
/// `a_device_reports_its_counter_sampling_gpu_families_and_timestamp_correlation`
/// is the probe that printed it, and it prints the correlated period too — it is
/// the measurement the first obstacle waits on.
///
/// Said the other way round: another Mac advertising counter sets would still be
/// refused here, so a message blaming only the CI device would be untrue on it.
const NO_COUNTER_SAMPLED_SET: &str = "a timestamp or pipeline-statistics query set needs an \
     MTLCounterSampleBuffer, and this backend builds neither on any device: reporting the \
     features obliges a timestamp_period_ns, and Metal correlates the GPU clock to the host at \
     sample time rather than ticking at a fixed period, so there is no honest period to report \
     (the Metal timestamp slice). The GPU this backend's CI runs on additionally advertises no \
     MTLDevice::counterSets. The occlusion kind is a plain MTLBuffer and is served";

/// Anything the object tables hold, so one lookup helper serves them all.
pub(crate) trait Owned {
    /// Id of the device that created it, per `crcbl-hal`'s obligation 3.
    fn owner(&self) -> u64;
}

/// Anything that owns an object table: a device, or — since the surface slice —
/// the instance.
///
/// Obligation 3 splits ownership two ways, checking surfaces against the
/// *instance* and everything else against the *device*, and both halves need
/// the same two facts and the same three-way answer (mine, somebody else's,
/// nobody's). This is that pair, so [`lookup`] and friends are written once
/// rather than once per owner.
pub(crate) trait Owner {
    /// The id every entry in this owner's tables is stamped with.
    fn owner_id(&self) -> u64;
    /// The tag this owner stamps into the handles it issues. Never zero; see
    /// the handle-tagging section below.
    fn tag(&self) -> u32;
}

impl Owner for DeviceInner {
    fn owner_id(&self) -> u64 {
        self.id
    }

    fn tag(&self) -> u32 {
        self.tag
    }
}

macro_rules! owned {
    ($($ty:ty),+ $(,)?) => {
        $(impl Owned for $ty {
            fn owner(&self) -> u64 {
                self.owner
            }
        })+
    };
}
pub(crate) use owned;

/// A buffer, its size and where its memory lives.
///
/// `location` is kept rather than re-derived from `raw.storageMode()` because
/// the seam's three locations do not round-trip through Metal's two storage
/// modes — `HostUpload` and `HostReadback` are both `Shared` — and
/// `request_readback` will need to tell them apart.
#[derive(Debug)]
struct BufferEntry {
    owner: u64,
    raw: Retained<ProtocolObject<dyn MTLBuffer>>,
    size: u64,
    location: MemoryLocation,
}

/// A texture, plus the seam-side facts Metal cannot answer from the object.
///
/// The format is kept because `create_image_view` needs to compare against it,
/// and comparing `MTLPixelFormat`s would answer a subtly different question:
/// two seam formats never share a Metal format (`conv`'s injectivity test), but
/// the reverse direction is what the view check is about. `image_type` is kept
/// because [`Extent3d::depth_or_layers`](crcbl_hal::Extent3d::depth_or_layers)
/// is a depth for a volume and a layer count for everything else, and a copy
/// region has to be built the right way round.
///
/// `swapchain_owned` is what stops [`Device::destroy_image`] freeing a row the
/// swapchain still hands out. The seam says an
/// [`AcquiredFrame::image`](crcbl_hal::AcquiredFrame::image) is the swapchain's,
/// and a caller that destroys one anyway must get a no-op rather than a ring
/// with a hole in it — `crcbl_mtl::swapchain` owns both the flag and every path
/// that removes such a row.
#[derive(Debug)]
struct ImageEntry {
    owner: u64,
    raw: Retained<ProtocolObject<dyn MTLTexture>>,
    format: Format,
    image_type: ImageType,
    swapchain_owned: bool,
}

/// A texture view, and the format it reinterprets its image as.
///
/// The format is what tells `begin_render_pass` whether a depth/stencil
/// attachment has a stencil plane to attach at all — a question the view's own
/// `MTLPixelFormat` could answer too, but only by a second mapping table
/// running backwards.
///
/// `swapchain_owned` is [`ImageEntry`]'s flag, for the same reason: the seam
/// says "do not destroy it" about [`AcquiredFrame::view`](crcbl_hal::AcquiredFrame::view)
/// too.
#[derive(Debug)]
struct ViewEntry {
    owner: u64,
    raw: Retained<ProtocolObject<dyn MTLTexture>>,
    format: Format,
    swapchain_owned: bool,
}

/// A sampler state.
#[derive(Debug)]
struct SamplerEntry {
    owner: u64,
    raw: Retained<ProtocolObject<dyn MTLSamplerState>>,
}

/// A finished command buffer, waiting to be submitted.
#[derive(Debug)]
pub(crate) struct CommandBufferEntry {
    pub(crate) owner: u64,
    pub(crate) raw: Retained<ProtocolObject<dyn MTLCommandBuffer>>,
    /// Set by [`Device::submit`]. **Metal raises on a second `commit`**, and a
    /// raise aborts the process, so a handle submitted twice has to be refused
    /// before it reaches the driver rather than after.
    pub(crate) committed: bool,
}

/// A semaphore: an `MTLSharedEvent` for a timeline, a plain `MTLEvent` for a
/// binary one.
///
/// Both are held as [`MTLEvent`], which is the protocol
/// `encodeWaitForEvent:value:` and `encodeSignalEvent:value:` take, with the
/// shared one kept beside it because `signaledValue` and
/// `waitUntilSignaledValue:timeoutMS:` — the two calls the CPU side of the seam
/// needs — exist only on [`MTLSharedEvent`].
struct SemaphoreEntry {
    owner: u64,
    raw: Retained<ProtocolObject<dyn MTLEvent>>,
    /// `None` for a binary semaphore, which has no CPU-visible value.
    shared: Option<Retained<ProtocolObject<dyn MTLSharedEvent>>>,
    /// The highest value **encoded** so far, which is not the same as the
    /// highest signalled: a signal sits in a committed command buffer until the
    /// GPU reaches it. The monotonicity check has to compare against this, or
    /// two submissions in flight can encode the same value and the second wait
    /// is satisfied by the first submission's work.
    encoded: u64,
}

impl core::fmt::Debug for SemaphoreEntry {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        f.debug_struct("SemaphoreEntry")
            .field("timeline", &self.shared.is_some())
            .field("encoded", &self.encoded)
            .finish_non_exhaustive()
    }
}

/// What a readback is waiting for.
#[derive(Debug)]
enum ReadbackWait {
    /// "Everything submitted to this device before the request" — the seam's
    /// meaning for [`ReadbackDesc::after`] of `None`, expressed as the last
    /// command buffer of the last submission. Metal runs a queue's command
    /// buffers in commit order, so that one completing means every earlier one
    /// has. `None` inside means nothing had been submitted at all, whose
    /// completion point is now.
    Submission(Option<Retained<ProtocolObject<dyn MTLCommandBuffer>>>),
    /// An explicit timeline point the caller named.
    Timeline {
        semaphore: SemaphoreHandle,
        value: u64,
    },
}

/// An in-flight readback request.
#[derive(Debug)]
struct ReadbackEntry {
    owner: u64,
    buffer: BufferHandle,
    offset: u64,
    size: u64,
    wait: ReadbackWait,
}

/// An occlusion query set: the `MTLBuffer` a render pass counts into, and how
/// many queries it holds.
///
/// There is no kind field because there is only one kind — [`Device::create_query_set`]
/// refuses the two counter-sampled kinds, and `crate::query` says why Metal's
/// occlusion pool is a plain buffer while they are not. The count is kept
/// because Metal has no way to answer it: the object is `count * RESULT_BYTES`
/// bytes of ordinary memory, so dividing its `length()` back down would be
/// re-deriving the descriptor rather than reading it.
#[derive(Debug)]
struct QuerySetEntry {
    owner: u64,
    raw: Retained<ProtocolObject<dyn MTLBuffer>>,
    count: u32,
}

owned!(
    BufferEntry,
    ImageEntry,
    ViewEntry,
    SamplerEntry,
    CommandBufferEntry,
    SemaphoreEntry,
    ReadbackEntry,
    QuerySetEntry,
);

/// A semaphore resolved to what a GPU-side wait needs: the event, and the value
/// to wait for.
pub(crate) type ResolvedWait = (Retained<ProtocolObject<dyn MTLEvent>>, u64);

/// Every table the device owns, behind one lock.
#[derive(Debug, Default)]
pub(crate) struct DeviceState {
    buffers: Pool<BufferEntry>,
    images: Pool<ImageEntry>,
    views: Pool<ViewEntry>,
    samplers: Pool<SamplerEntry>,
    pub(crate) command_buffers: Pool<CommandBufferEntry>,
    semaphores: Pool<SemaphoreEntry>,
    readbacks: Pool<ReadbackEntry>,
    query_sets: Pool<QuerySetEntry>,
    /// The pipeline slice's four tables; `crcbl_mtl::pipeline` owns their
    /// entries and every call that touches them.
    pub(crate) shader_modules: Pool<crate::pipeline::ShaderModuleEntry>,
    pub(crate) pipeline_layouts: Pool<crate::pipeline::PipelineLayoutEntry>,
    pub(crate) graphics_pipelines: Pool<crate::pipeline::GraphicsPipelineEntry>,
    pub(crate) compute_pipelines: Pool<crate::pipeline::ComputePipelineEntry>,
    /// The binding slice's two tables; `crcbl_mtl::binding` owns their entries
    /// and every call that touches them.
    pub(crate) bind_group_layouts: Pool<crate::binding::BindGroupLayoutRecord>,
    pub(crate) bind_groups: Pool<crate::binding::BindGroupRecord>,
    /// The surface slice's one table; `crcbl_mtl::swapchain` owns its entries
    /// and every call that touches them.
    pub(crate) swapchains: Pool<crate::swapchain::SwapchainEntry>,
    /// The last command buffer of the most recent submission, retained.
    ///
    /// This is the whole of this backend's completion tracking, and it is one
    /// object rather than a queue because Metal's own ordering does the rest: a
    /// command buffer completing implies every buffer committed before it has.
    /// A readback clones it, so a later submission replacing it here does not
    /// take the completion point out from under a request already in flight.
    last_submission: Option<Retained<ProtocolObject<dyn MTLCommandBuffer>>>,
    /// Every command buffer this device has committed and not yet seen finish.
    ///
    /// Separate from [`last_submission`](Self::last_submission), which is one
    /// object and exists to give a readback something to wait on. This is the
    /// set, and it exists because **a failed `MTLCommandBuffer` reports through
    /// nothing but its own `status`**: no callback, no exception, no later call
    /// that fails because of it. A submission whose result nobody reads fails in
    /// total silence, and until this there was no path on which one was noticed.
    ///
    /// It does not grow without bound: [`crate::fault::sweep`] empties every
    /// entry that has finished, and runs before each submission, so what is
    /// retained is what is genuinely still running.
    pub(crate) in_flight: Vec<Retained<ProtocolObject<dyn MTLCommandBuffer>>>,
    /// The ones that failed. See [`crate::fault::FaultLog`].
    pub(crate) faults: crate::fault::FaultLog,
}

impl DeviceState {
    /// Files every command buffer that has finished, and every failure among
    /// them. See [`crate::fault::sweep`].
    ///
    /// A method on the state rather than a call at each site, because
    /// `fault::sweep` borrows two of its fields at once and a `MutexGuard` will
    /// not split a borrow the way a `&mut DeviceState` does.
    pub(crate) fn sweep(&mut self) {
        crate::fault::sweep(&mut self.in_flight, &mut self.faults);
    }

    /// Adds one committed command buffer to the in-flight set, sweeping first.
    pub(crate) fn track(&mut self, command_buffer: Retained<ProtocolObject<dyn MTLCommandBuffer>>) {
        self.sweep();
        self.in_flight.push(command_buffer);
    }
}

/// An image handle resolved to everything a copy or a pass needs from it.
pub(crate) struct ResolvedImage {
    pub(crate) raw: Retained<ProtocolObject<dyn MTLTexture>>,
    pub(crate) format: Format,
    pub(crate) image_type: ImageType,
}

/// The device's shared state.
pub(crate) struct DeviceInner {
    /// Obligation 1: a `Device` may outlive its `Instance`, so the instance's
    /// state — on Metal, the enumerated `MTLDevice` objects and, since the
    /// surface slice, the surface table — is kept alive here rather than
    /// borrowed. See [`InstanceInner`].
    ///
    /// It stopped being write-only with surfaces: `create_swapchain` resolves a
    /// [`SurfaceHandle`](crcbl_hal::SurfaceHandle) through it, which is how
    /// obligation 3's instance half — a surface from another instance is
    /// [`HalError::ForeignObject`] — is checked at all.
    pub(crate) instance: Arc<InstanceInner>,
    pub(crate) raw: Retained<ProtocolObject<dyn MTLDevice>>,
    /// The one queue. Metal has a single `MTLCommandQueue` type and no queue
    /// families, which is exactly why the seam's enum is named
    /// [`QueueKind`] rather than `QueueFamily`.
    pub(crate) queue: Retained<ProtocolObject<dyn MTLCommandQueue>>,
    /// The always-pass, never-write `MTLDepthStencilState` that stands in for
    /// nil, and the reason `setDepthStencilState:` has no nil path at all:
    /// passing nil hangs Apple's paravirtual GPU. `crcbl_mtl::pipeline`'s
    /// [`default_depth_stencil_state`](crate::pipeline::default_depth_stencil_state)
    /// builds it and argues both halves — the hang, and why the substitution
    /// cannot change an image.
    ///
    /// **One object per device, created at open.** It is immutable, it is
    /// `Send + Sync` on its own (`objc2-metal` declares `MTLDepthStencilState`
    /// so), and every pipeline that needs it holds a `Retained` clone — so it
    /// outlives every encoder that binds it without living under `state`'s
    /// `Mutex` the way the tables do. Eager rather than `crcbl-dx12`'s
    /// create-on-first-use `indirect_signature`, because that cache is keyed on
    /// `(kind, stride)` and this is a single object with no key: there is
    /// nothing for a lazy path to decide, and the one message send belongs
    /// where its failure can still be reported as device creation failing.
    pub(crate) default_depth_stencil: Retained<ProtocolObject<dyn MTLDepthStencilState>>,
    pub(crate) caps: DeviceCaps,
    /// Says, once, that a present wait actually reached a drawable.
    ///
    /// Not inferable from anything else this device logs. `PRESENT_FEEDBACK` is
    /// unconditional on Metal, so its being advertised says nothing about
    /// whether a handler ever fired — and a `CAMetalLayer` with
    /// `displaySyncEnabled` already paces the loop to the display on its own,
    /// so a closed loop and a wait that answers immediately forever have the
    /// same frame time. `crcbl_mtl::swapchain`'s `wait_until_presented_impl` is
    /// where it is said, and `crcbl-vk` keeps the identical fact.
    pub(crate) first_present_wait: Once,
    pub(crate) id: u64,
    /// This device's stamp on every handle it issues; see the handle-tagging
    /// section below. Never zero.
    tag: u32,
    state: Mutex<DeviceState>,
}

// SAFETY: this is the marker impl the crate docs used to say a device slice
// would not need, and the reason it does is narrower than "Objective-C is not
// thread-safe".
//
// `MTLDevice`, `MTLCommandQueue`, `MTLDepthStencilState` and `MTLSamplerState`
// are all declared `NSObjectProtocol + Send + Sync` in `objc2-metal`, so those
// fields carry the markers themselves and are not why this impl exists.
// `MTLBuffer` and `MTLTexture` are not: they inherit from `MTLResource`, which
// objc2 leaves unmarked because `MTLBuffer::contents` hands out a raw pointer
// into the allocation and a binding cannot know what a user will do with it.
//
// This backend can answer that, which is what makes the assertion sound rather
// than optimistic:
//
// * Every `MTLBuffer` and `MTLTexture` lives inside `state`, so every access to
//   one is already under the `Mutex`.
// * The only use of `contents` outside the tests is `write_buffer`, which copies
//   into the pointer while holding that same lock and never lets it escape.
//   (`tests::read_back` is the other caller and takes the same lock; it exists
//   because reading the bytes is the only way to observe that `write_buffer`
//   wrote anything.) There is no persistent mapping handed across the seam — the
//   seam has no shape for one, which is the argument `Device::write_buffer`
//   makes for being a copy.
// * Retain and release are atomic in the Objective-C runtime, so moving a
//   `Retained` between threads and dropping it on another is sound on its own.
//
// MTL3 added two more unmarked kinds to `state`, and each is covered by the
// same lock argument:
//
// * **`MTLCommandBuffer`**, in the command-buffer table and in
//   `last_submission`. Every access — `commit`, `status`, `error`,
//   `encodeSignalEvent:value:` — happens with the `Mutex` held, so no two
//   threads touch one at once. A command buffer being *recorded* lives in
//   `MetalCommandEncoder` instead and never enters this table until `finish`,
//   and that type carries its own marker impl with its own argument.
// * **`MTLEvent`**, in the semaphore table. `MTLSharedEvent` inherits from
//   `MTLEvent`, which `objc2-metal` declares `NSObjectProtocol + Send + Sync`
//   upstream — so the events are not why this impl exists either; they are
//   named here only so the next reader does not have to re-derive that.
//
// MTL5 added two more, and they are the ones worth being careful about because
// they are **Core Animation** objects rather than Metal ones:
//
// * **`CAMetalLayer`**, cloned into every swapchain entry, and
// * **`CAMetalDrawable`**, held between an acquire and its present.
//
//   Both live in `state` and every access is under the `Mutex`, so the
//   exclusion half is the same argument as above. The half that is *not* about
//   exclusion is thread affinity, and it is discharged by what this crate does
//   not do: the only messages it ever sends a layer are the Metal-facing ones —
//   `setDevice:`, `setPixelFormat:`, `setFramebufferOnly:`, `setOpaque:`,
//   `setMaximumDrawableCount:`, `setDisplaySyncEnabled:`, `setDrawableSize:`,
//   `setName:`, `drawableSize` and `nextDrawable` — plus `texture` and
//   `presentDrawable:` on a drawable. **No `NSView`, `NSWindow` or `NSScreen`
//   is ever reached**, and nothing walks a layer's `superlayer` or `delegate`
//   to find one; those are the genuinely main-thread-only objects, and touching
//   one off the main thread is what `crcbl_core::surface`'s thread-safety note
//   is about. Rendering to a `CAMetalLayer` from a thread other than the main
//   one is the ordinary Metal arrangement, and `wgpu-hal`'s Metal backend makes
//   the same assertion about the same object for the same reason.
//
//   The claim is deliberately **not** widened past that: the seam still
//   requires `create_surface` to be called from the window's own thread
//   (`Instance::create_surface` obligation 3), because retaining the layer is a
//   message send to an object the shell may still be constructing, and nothing
//   here can know that it is not.
//
// Apple documents `MTLDevice` and the objects created from it as safe to use
// from multiple threads; the two impls below narrow that to the accesses this
// crate actually performs.
unsafe impl Send for DeviceInner {}
// SAFETY: as above.
unsafe impl Sync for DeviceInner {}

impl core::fmt::Debug for DeviceInner {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        f.debug_struct("DeviceInner")
            .field("id", &self.id)
            .field("geometry", &self.caps.geometry_path())
            .field("binding", &self.caps.binding_model())
            .field("lighting", &self.caps.lighting_path())
            .finish_non_exhaustive()
    }
}

// --- handle tagging --------------------------------------------------------
//
// **Every object table here is per-device**, and every insert stamps
// `owner: self.id` — which on its own makes `HalError::ForeignObject`
// unproducible rather than satisfied. Two devices allocating in step reach the
// same slot index at the same generation immediately, so device A's first
// buffer handle resolves inside device B's pool to *B's* first buffer, whose
// owner is B: the check passes and B writes into the wrong object. That is
// obligation 3 met by accident and only while the two pools happen to differ.
//
// So the handle carries the issuing device. The top byte of the index half is
// the device's tag; the rest is the pool's own index, restored before any
// lookup. The generation half is untouched, so `Pool`'s generation-exhaustion
// rule is unaffected. `crcbl-vk` uses the same scheme, arrived at from the same
// bug.
//
// The residual hole is stated rather than hidden: tags repeat every
// `DEVICE_TAG_COUNT` devices, so the 256th device in a process shares a tag
// with the first and falls back to the owner check, which cannot separate them.
// A process that opens hundreds of Metal devices has a different problem.

/// Bits of a handle's index half given over to the owning device's tag.
const DEVICE_TAG_SHIFT: u32 = 24;
/// The part of a handle's index half that is the pool's own index.
const POOL_INDEX_MASK: u32 = (1 << DEVICE_TAG_SHIFT) - 1;
/// How many distinct device tags exist. Tag `0` is reserved for "nobody", so a
/// hand-made or un-stamped handle is foreign to every device.
const DEVICE_TAG_COUNT: u64 = (u32::MAX >> DEVICE_TAG_SHIFT) as u64;

/// The tag an owner with this id stamps into its handles. Never zero.
///
/// Instances take one from the same counter as devices, and the two ranges
/// deliberately overlap: a surface handle is only ever looked up in an
/// instance's surface pool and an image handle only ever in a device's image
/// pool, so a shared tag value cannot make one resolve in the other.
pub(crate) fn device_tag(id: u64) -> u32 {
    #[allow(clippy::cast_possible_truncation)]
    {
        1 + (id % DEVICE_TAG_COUNT) as u32
    }
}

/// The device tag a handle carries, or `0` if it carries none.
const fn handle_tag<M>(handle: Handle<M>) -> u32 {
    handle.index() >> DEVICE_TAG_SHIFT
}

/// Strips the device tag, recovering the pool's own handle.
fn untag<A, B>(handle: Handle<A>) -> Handle<B> {
    Handle::from_bits(
        (u64::from(handle.generation()) << 32) | u64::from(handle.index() & POOL_INDEX_MASK),
    )
    .unwrap_or_else(|| unreachable!("a handle's generation is never zero"))
}

/// Deterministic queue handles.
///
/// Queues are not pooled: there is exactly one for the device's lifetime and it
/// carries no state a caller can hold, so [`Device::queue`] stays a pure
/// function. The device's tag rides in the same place it does on a pooled
/// handle, because obligation 3 covers queues too — a `QueueHandle` synthesised
/// from the kind alone would carry no device identity at all, and every device
/// would accept every other device's.
fn queue_handle(tag: u32, kind: QueueKind) -> QueueHandle {
    Handle::from_bits((1u64 << 32) | u64::from((tag << DEVICE_TAG_SHIFT) | queue_index(kind)))
        .unwrap_or_else(|| unreachable!("generation 1 is non-zero"))
}

const fn queue_index(kind: QueueKind) -> u32 {
    match kind {
        QueueKind::Graphics => 0,
        QueueKind::Compute => 1,
        QueueKind::Transfer => 2,
    }
}

/// Stamps `owner`'s tag into a handle its pool just issued.
///
/// Every handle that crosses the seam goes through here; every handle that
/// comes back goes through [`local_handle`]. A pool index too large to carry
/// the tag gets tag `0`, which resolves nowhere — the object leaks until the
/// owner is dropped, which is far better than a handle that might resolve to
/// another owner's object. It takes more live objects of one kind than
/// [`POOL_INDEX_MASK`] admits to reach.
pub(crate) fn stamp<A, B>(owner: &impl Owner, handle: Handle<A>) -> Handle<B> {
    let index = handle.index();
    let tag = if index > POOL_INDEX_MASK {
        crcbl_core::log::error!(
            "crcbl-mtl: pool index {index} is too large to carry an owner tag; issuing a handle \
             that resolves nowhere rather than one that might resolve to another device's object"
        );
        0
    } else {
        owner.tag()
    };
    Handle::from_bits(
        (u64::from(handle.generation()) << 32) | u64::from((tag << DEVICE_TAG_SHIFT) | index),
    )
    .unwrap_or_else(|| unreachable!("a handle's generation is never zero"))
}

/// Decodes a handle for `owner`'s pools, or says why it is not one.
pub(crate) fn local_handle<E, M>(
    kind: &'static str,
    handle: Handle<M>,
    owner: &impl Owner,
) -> Result<Handle<E>, HalError> {
    let tag = handle_tag(handle);
    if tag == owner.tag() {
        return Ok(untag(handle));
    }
    // Tag zero was never issued by any device — a hand-made handle, or one
    // whose pool index overflowed the tagged range. Anything else is a real
    // handle belonging to a real, different device, which is the case
    // obligation 3 exists for.
    Err(if tag == 0 {
        HalError::invalid_handle(kind, handle)
    } else {
        HalError::ForeignObject {
            kind,
            bits: handle.to_bits(),
        }
    })
}

/// Resolves a handle against a pool and its owner.
pub(crate) fn lookup<'p, E: Owned, M>(
    pool: &'p Pool<E>,
    kind: &'static str,
    handle: Handle<M>,
    owner: &impl Owner,
) -> Result<&'p E, HalError> {
    let local = local_handle(kind, handle, owner)?;
    match pool.get(local) {
        Some(entry) if entry.owner() == owner.owner_id() => Ok(entry),
        Some(_) => Err(HalError::ForeignObject {
            kind,
            bits: handle.to_bits(),
        }),
        None => Err(HalError::invalid_handle(kind, handle)),
    }
}

/// [`lookup`], for the tables an operation has to modify — which since the
/// surface slice means swapchains, whose acquire cursor and outstanding
/// drawable both move.
pub(crate) fn lookup_mut<'p, E: Owned, M>(
    pool: &'p mut Pool<E>,
    kind: &'static str,
    handle: Handle<M>,
    owner: &impl Owner,
) -> Result<&'p mut E, HalError> {
    let local = local_handle(kind, handle, owner)?;
    let owner_id = owner.owner_id();
    match pool.get(local) {
        Some(entry) if entry.owner() == owner_id => {}
        Some(_) => {
            return Err(HalError::ForeignObject {
                kind,
                bits: handle.to_bits(),
            });
        }
        None => return Err(HalError::invalid_handle(kind, handle)),
    }
    pool.get_mut(local)
        .ok_or_else(|| HalError::invalid_handle(kind, handle))
}

/// Removes a handle from `pool` and hands the entry back, but **only** if
/// `owner` owns it.
///
/// The order is the point: removing first and checking the owner afterwards
/// would already have dropped the entry, so a foreign handle that happened to
/// resolve would destroy this owner's own unrelated object.
pub(crate) fn remove_owned<E: Owned, M>(
    pool: &mut Pool<E>,
    handle: Handle<M>,
    owner: &impl Owner,
) -> Option<E> {
    let local = local_handle::<E, M>("object", handle, owner).ok()?;
    if !pool
        .get(local)
        .is_some_and(|entry| entry.owner() == owner.owner_id())
    {
        return None;
    }
    pool.remove(local)
}

/// [`remove_owned`] for the callers that only need to know whether it happened.
pub(crate) fn take_owned<E: Owned, M>(
    pool: &mut Pool<E>,
    handle: Handle<M>,
    owner: &impl Owner,
) -> bool {
    remove_owned(pool, handle, owner).is_some()
}

/// The Metal implementation of [`Device`].
#[derive(Debug)]
pub struct MetalDevice {
    pub(crate) inner: Arc<DeviceInner>,
}

impl MetalDevice {
    /// Opens a device on `record`'s adapter.
    ///
    /// There is no `vkCreateDevice` equivalent: the `MTLDevice` already exists
    /// from enumeration, so opening a device means taking a reference to it and
    /// creating the queue every submission will go through. Both steps are
    /// synchronous, which is why [`crate::MetalInstance::request_device`]
    /// completes on its first poll.
    pub(crate) fn open(
        instance: Arc<InstanceInner>,
        record: &AdapterRecord,
        desc: &DeviceDesc<'_>,
    ) -> Result<Self, HalError> {
        let raw = record.raw.clone();
        let Some(queue) = raw.newCommandQueue() else {
            return Err(HalError::Backend(format!(
                "MTLDevice::newCommandQueue returned nil for adapter {:?}",
                record.info.name
            )));
        };
        // `MTLDevice` has no settable label — it names hardware, not an object
        // a program made — so the device's debug name goes on the queue, which
        // is where Xcode's GPU capture shows it.
        if let Some(label) = desc.label {
            queue.setLabel(Some(&NSString::from_str(label)));
        }
        // What validation this device is running under, stated once at open.
        // `crcbl_dx12::debug::enable_debug_layer` and `crcbl_vk::debug` both log
        // the equivalent, and for the same reason: Metal's switches are
        // environment variables set before the process started, so without a
        // line here a log gives a reader no way to tell a validated run from an
        // unvalidated one.
        crcbl_core::log::info!(
            "crcbl-mtl: {}",
            crate::fault::ValidationReport::of(&raw, &crate::fault::FaultLog::default()).line()
        );
        // Before any pipeline exists, because every pipeline without a
        // depth/stencil state binds it and nil is not an alternative here.
        let default_depth_stencil = crate::pipeline::default_depth_stencil_state(&raw)?;

        // **Metal has no `pEnabledFeatures`.** A `VkDevice` reports only what
        // was switched on at creation, so `crcbl-vk` intersects the adapter's
        // features with what the caller asked for. An `MTLDevice` can do
        // whatever the adapter answered, always — there is nothing to enable
        // and nothing to leave off — so `caps` is the adapter's caps verbatim.
        // `required_features` was checked in `open_device` before this call;
        // `optional_features` is satisfied by construction, which is exactly
        // what `DeviceDesc::optional_features` documents ("check `Device::caps`
        // afterwards to find out").
        let caps = record.info.caps;
        let id = next_owner_id();
        let inner = Arc::new(DeviceInner {
            instance,
            raw,
            queue,
            default_depth_stencil,
            caps,
            first_present_wait: Once::new(),
            id,
            tag: device_tag(id),
            state: Mutex::new(DeviceState::default()),
        });
        crcbl_core::log::info!(
            "crcbl-mtl: opened {:?} (geometry {:?}, binding {:?}, lighting {:?})",
            record.info.name,
            caps.geometry_path(),
            caps.binding_model(),
            caps.lighting_path()
        );
        Ok(Self { inner })
    }

    pub(crate) fn state(&self) -> MutexGuard<'_, DeviceState> {
        self.inner.state()
    }

    /// Stamps this device's tag into a handle its pools just issued.
    pub(crate) fn stamp<A, B>(&self, handle: Handle<A>) -> Handle<B> {
        stamp(&*self.inner, handle)
    }

    /// Registers the drawable a `nextDrawable` just handed back as an image and
    /// a whole-image view, and hands their two handles to the swapchain.
    ///
    /// **The view's texture is the drawable's own.** Metal has no separate view
    /// object — `newTextureViewWithPixelFormat:…` returns another `MTLTexture` —
    /// and the seam wants "a whole-image view of the image, in the swapchain's
    /// format", which the drawable's texture already is. Cutting a view would
    /// allocate an object per frame and additionally require
    /// `MTLTextureUsagePixelFormatView` on a texture Core Animation, not this
    /// backend, created.
    pub(crate) fn insert_drawable_rows(
        &self,
        state: &mut DeviceState,
        texture: &Retained<ProtocolObject<dyn MTLTexture>>,
        format: Format,
    ) -> (ImageHandle, ImageViewHandle) {
        let image = state.images.insert(ImageEntry {
            owner: self.inner.id,
            raw: texture.clone(),
            format,
            image_type: ImageType::D2,
            swapchain_owned: true,
        });
        let view = state.views.insert(ViewEntry {
            owner: self.inner.id,
            raw: texture.clone(),
            format,
            swapchain_owned: true,
        });
        (self.stamp(image), self.stamp(view))
    }

    /// Marks a ring image and its view as the swapchain's, so
    /// [`Device::destroy_image`] and [`Device::destroy_image_view`] leave them
    /// alone.
    pub(crate) fn mark_swapchain_owned(&self, image: ImageHandle, view: ImageViewHandle) {
        let mut state = self.state();
        if let Ok(local) = local_handle::<ImageEntry, _>("image", image, &*self.inner)
            && let Some(entry) = state.images.get_mut(local)
        {
            entry.swapchain_owned = true;
        }
        if let Ok(local) = local_handle::<ViewEntry, _>("image view", view, &*self.inner)
            && let Some(entry) = state.views.get_mut(local)
        {
            entry.swapchain_owned = true;
        }
    }

    /// Removes an image and its view, both of which the swapchain owns.
    ///
    /// This and the two below are the only paths that free a `swapchain_owned`
    /// row, which is what makes the flag a guard rather than a leak:
    /// `destroy_image` refuses, and none of these consults the flag at all.
    pub(crate) fn remove_swapchain_rows(
        &self,
        state: &mut DeviceState,
        rows: (ImageHandle, ImageViewHandle),
    ) {
        self.remove_swapchain_view(state, rows.1);
        self.remove_swapchain_image(state, rows.0);
    }

    /// [`remove_swapchain_rows`](Self::remove_swapchain_rows) for the image
    /// alone — the state a ring build is in when the *view* is what failed.
    pub(crate) fn remove_swapchain_image(&self, state: &mut DeviceState, image: ImageHandle) {
        remove_owned(&mut state.images, image, &*self.inner);
    }

    /// [`remove_swapchain_rows`](Self::remove_swapchain_rows) for the view
    /// alone.
    pub(crate) fn remove_swapchain_view(&self, state: &mut DeviceState, view: ImageViewHandle) {
        remove_owned(&mut state.views, view, &*self.inner);
    }

    /// An empty command buffer: for the waits and signals that have nowhere
    /// else to go (see [`Device::submit`]), and for the one
    /// [`Device::wait_idle`] commits to have something to wait on.
    ///
    /// It goes through `crcbl_mtl::fault` rather than calling
    /// `MTLCommandQueue::commandBuffer` itself, so that this one is as able to
    /// report which of its encoders faulted as any other — a command buffer
    /// created the short way is one whose fault arrives unattributed.
    fn new_command_buffer(
        &self,
        label: &str,
    ) -> Result<Retained<ProtocolObject<dyn MTLCommandBuffer>>, HalError> {
        crate::fault::command_buffer(&self.inner.queue, label).ok_or_else(|| {
            HalError::DeviceLost(
                "MTLCommandQueue::commandBufferWithDescriptor: returned nil".to_string(),
            )
        })
    }
}

impl DeviceInner {
    pub(crate) fn state(&self) -> MutexGuard<'_, DeviceState> {
        self.state
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
    }

    /// Resolves a queue handle against *this* device.
    ///
    /// Obligation 3 covers queues too, and the three outcomes are kept apart
    /// for the same reason they are everywhere else: a handle carrying another
    /// device's tag is [`HalError::ForeignObject`] — the caller crossed two
    /// objects that never met — while one carrying no tag at all was never
    /// issued by any device and is [`HalError::InvalidHandle`].
    pub(crate) fn check_queue(&self, queue: QueueHandle) -> Result<(), HalError> {
        if queue == queue_handle(self.tag, QueueKind::Graphics) {
            return Ok(());
        }
        Err(if handle_tag(queue) == 0 {
            HalError::invalid_handle("queue", queue)
        } else {
            HalError::ForeignObject {
                kind: "queue",
                bits: queue.to_bits(),
            }
        })
    }

    /// Stamps this device's tag into a handle its pools just issued. See
    /// [`stamp`].
    pub(crate) fn stamp<A, B>(&self, handle: Handle<A>) -> Handle<B> {
        stamp(self, handle)
    }

    /// The event a GPU-side wait on `handle` has to encode, and the value.
    ///
    /// **The value is the highest one *encoded* onto the semaphore**, whichever
    /// kind it is, and that is not a shortcut for a timeline: the only caller is
    /// [`Device::present`], and [`PresentInfo::waits`] carries handles with no
    /// values beside them, so "everything signalled onto this so far" is the
    /// only thing the seam can be asking for. [`Device::submit`] takes its
    /// values from the caller instead and does its own monotonicity check; this
    /// is deliberately not that path.
    pub(crate) fn semaphore_wait(
        &self,
        state: &DeviceState,
        handle: SemaphoreHandle,
    ) -> Result<ResolvedWait, HalError> {
        let entry = lookup(&state.semaphores, "semaphore", handle, self)?;
        Ok((entry.raw.clone(), entry.encoded))
    }

    /// The `MTLBuffer` a handle names, cloned so the lock can be released
    /// before the object is used.
    ///
    /// The clone is a retain, not a copy of the allocation, and it is what lets
    /// the command encoder resolve handles under the lock and then encode
    /// without holding it — which matters because encoding is the slow part and
    /// `&self` on this trait means the lock is shared with every other thread's
    /// resource creation.
    pub(crate) fn buffer_raw(
        &self,
        handle: BufferHandle,
    ) -> Result<Retained<ProtocolObject<dyn MTLBuffer>>, HalError> {
        Ok(self.buffer_raw_locked(&self.state(), handle)?.0)
    }

    /// [`buffer_raw`](Self::buffer_raw) for a caller that already holds the
    /// lock, with the allocation's size beside the object.
    ///
    /// The `_locked` pair exists because [`Mutex`] is not reentrant: the
    /// binding slice resolves a whole bind group's worth of handles under one
    /// guard, and calling the self-locking form from inside that would deadlock
    /// rather than block. The size comes back too because every caller of this
    /// form is bounds-checking a range against it.
    pub(crate) fn buffer_raw_locked(
        &self,
        state: &DeviceState,
        handle: BufferHandle,
    ) -> Result<(Retained<ProtocolObject<dyn MTLBuffer>>, u64), HalError> {
        let entry = lookup(&state.buffers, "buffer", handle, self)?;
        Ok((entry.raw.clone(), entry.size))
    }

    /// The `MTLTexture` an image-view handle names. See
    /// [`buffer_raw_locked`](Self::buffer_raw_locked) for why the `_locked`
    /// pair exists.
    pub(crate) fn view_raw_locked(
        &self,
        state: &DeviceState,
        handle: ImageViewHandle,
    ) -> Result<Retained<ProtocolObject<dyn MTLTexture>>, HalError> {
        Ok(lookup(&state.views, "image view", handle, self)?
            .raw
            .clone())
    }

    /// The `MTLSamplerState` a handle names. See
    /// [`buffer_raw_locked`](Self::buffer_raw_locked).
    pub(crate) fn sampler_raw_locked(
        &self,
        state: &DeviceState,
        handle: SamplerHandle,
    ) -> Result<Retained<ProtocolObject<dyn MTLSamplerState>>, HalError> {
        Ok(lookup(&state.samplers, "sampler", handle, self)?
            .raw
            .clone())
    }

    /// The `MTLTexture` a handle names, with the two seam facts the object
    /// cannot answer. See [`ImageEntry`].
    pub(crate) fn image_raw(&self, handle: ImageHandle) -> Result<ResolvedImage, HalError> {
        let state = self.state();
        let entry = lookup(&state.images, "image", handle, self)?;
        Ok(ResolvedImage {
            raw: entry.raw.clone(),
            format: entry.format,
            image_type: entry.image_type,
        })
    }

    /// The visibility-result buffer a query-set handle names, with the number of
    /// queries in it.
    ///
    /// The count comes back because every caller is bounds-checking a range
    /// against it, and it is the set's own count rather than the buffer's
    /// `length()`: those agree today and only one of them is the descriptor the
    /// caller asked for.
    pub(crate) fn query_set_raw(
        &self,
        handle: QuerySetHandle,
    ) -> Result<(Retained<ProtocolObject<dyn MTLBuffer>>, u32), HalError> {
        let state = self.state();
        let entry = lookup(&state.query_sets, "query set", handle, self)?;
        Ok((entry.raw.clone(), entry.count))
    }

    /// The `MTLTexture` an image-view handle names, and the format it
    /// reinterprets its image as.
    pub(crate) fn view_raw(
        &self,
        handle: ImageViewHandle,
    ) -> Result<(Retained<ProtocolObject<dyn MTLTexture>>, Format), HalError> {
        let state = self.state();
        let entry = lookup(&state.views, "image view", handle, self)?;
        Ok((entry.raw.clone(), entry.format))
    }
}

/// The "this slice has not arrived" answer, in one place so the voice is
/// uniform across the whole trait.
fn not_yet(what: &'static str) -> HalError {
    crate::MetalInstance::not_yet(what)
}

/// The "Metal itself has not got this" answer — same variant as
/// [`not_yet`], different sentence. See
/// [`MetalInstance::unsupported`](crate::MetalInstance::unsupported).
fn unsupported(what: &'static str) -> HalError {
    crate::MetalInstance::unsupported(what)
}

/// Saturating `u64` → `NSUInteger`, for a length already bounds-checked against
/// a device limit.
pub(crate) fn to_ns(value: u64) -> NSUInteger {
    NSUInteger::try_from(value).unwrap_or(NSUInteger::MAX)
}

impl Device for MetalDevice {
    fn backend(&self) -> BackendKind {
        BackendKind::Metal
    }

    fn caps(&self) -> DeviceCaps {
        self.inner.caps
    }

    /// What this backend does with each seam behaviour.
    ///
    /// Kinds of refusal live here and the reasons keep them apart, because the
    /// answer to "should this be fixed?" differs —
    /// [`crcbl_hal::DivergenceKind`] is the same split as data:
    ///
    /// * **Metal has not got it.** The byte-wide buffer fill is the API, not
    ///   this crate, and no slice will change it.
    /// * **This crate has not built it.** Mesh pipelines and the indirect
    ///   command buffer behind a GPU-side draw count both exist in Metal and are
    ///   owed here; the reason names the slice that owes them.
    /// * **No device here can do it, and another might.** The counter-sampled
    ///   queries depend on what a device answers to `supportsCounterSampling:`
    ///   and `counterSets`, and the only Mac this backend runs on advertises no
    ///   counter set at all — so the reason names the measurement rather than
    ///   claiming Metal cannot.
    ///
    /// Exhaustive with no wildcard arm, and `deny`-ed as such: a capability
    /// added to the enum must be answered here.
    #[deny(clippy::wildcard_enum_match_arm)]
    fn supports(&self, capability: Capability) -> Support {
        let has = self.inner.caps.features;
        let gated = |feature: Features, why: &'static str| -> Support {
            Support::granted(has, feature, why)
        };

        match capability {
            // `MTLBlitCommandEncoder fillBuffer:range:value:` takes a `uint8_t`,
            // so a word whose bytes agree has an encoding and one whose bytes
            // differ has none. This is the backend that makes the three fill
            // capabilities three rather than one.
            Capability::BufferFillZero | Capability::BufferFillRepeatedByte => Support::Yes,
            Capability::BufferFillWord => Support::No(
                "MTLBlitCommandEncoder fillBuffer:range:value: repeats a single byte, so only a \
                 u32 whose four bytes are equal has an encoding",
            ),
            Capability::ImageToImageCopy => Support::Yes,
            // A Metal depth texture is a single-plane typed texture, so the blit
            // encoder's texture↔buffer calls take it with the bytes-per-row
            // `conv::copy_footprint` computes from the depth aspect's own texel
            // size — the same call and the same footprint a colour copy uses.
            Capability::DepthImageCopy => Support::Yes,
            // `MTLRenderPassColorAttachmentDescriptor` carries `resolveTexture`
            // and a `MTLStoreAction` that resolves.
            Capability::MsaaResolveAttachment => Support::Yes,
            Capability::StencilReference => Support::Yes,
            // The sentence `crcbl_hal::DIVERGENCES` carries for this pair, so
            // the declaration and the parity record cannot drift apart — they
            // had, and the constant is what settles it.
            Capability::DrawIndirectCount => Support::No(crcbl_hal::METAL_NO_DRAW_INDIRECT_COUNT),
            // A multi-draw is a loop over the argument structures, so a stride
            // larger than one of them is exactly what the loop steps by.
            Capability::IndirectArgumentPaddedStride => Support::Yes,
            Capability::MeshShading => Support::No(
                "Metal has the object/mesh stages and Slang emits them, but this backend builds \
                 no MTLMeshRenderPipelineDescriptor (the Metal mesh slice)",
            ),
            Capability::TaskShaderStage => Support::No(
                "no mesh pipeline to put an object stage in front of (the Metal mesh slice)",
            ),
            Capability::UpdateBindGroup => Support::Yes,
            // `create_pipeline_layout` places the block at the buffer index the
            // committed MSL puts it at — one past every binding, which
            // `crcbl_mtl::argument` derives — and `push_constants` sends it with
            // `setBytes:length:atIndex:` or its vertex/fragment twins.
            // `setBytes:` is on every Metal encoder, so the gate below never
            // fires: it is the flag's own arm, not a device question.
            Capability::PushConstants => gated(
                Features::PUSH_CONSTANTS,
                "this device reports no PUSH_CONSTANTS",
            ),
            Capability::BindlessDescriptorArray => Support::No(
                "this backend binds Metal's flat argument tables, which have a fixed length and \
                 no runtime-sized array (the Metal argument-buffer slice)",
            ),
            Capability::StorageImageBinding => Support::Yes,
            Capability::PolygonModeLine => gated(
                Features::POLYGON_MODE_LINE,
                "this device reports no POLYGON_MODE_LINE",
            ),
            // The one flag `crate::adapter` withholds from a measurement rather
            // than a query: a paravirtual GPU accepts `setDepthClipMode:` and
            // clips the primitive regardless, so `crate::quirk` takes the flag
            // off that device and `create_graphics_pipeline` refuses the state.
            Capability::DepthClamp => gated(
                Features::DEPTH_CLAMP,
                "this device reports no DEPTH_CLAMP; crcbl_mtl::quirk withholds it from a \
                 paravirtual GPU, which was measured to accept MTLDepthClipMode::Clamp and clip \
                 the primitive anyway",
            ),
            Capability::SamplerAnisotropy => gated(
                Features::SAMPLER_ANISOTROPY,
                "this device reports no SAMPLER_ANISOTROPY",
            ),
            // **This claims what the capability defines and no more.**
            // `Capability::OcclusionQuery` is "a QueryKind::Occlusion query set"
            // — `crcbl_hal::CommandEncoder` has no begin/end query verb, so
            // nothing a caller records through this seam can ever write one, and
            // the same is true of the Vulkan and WebGPU backends' `Yes`. What is
            // claimed is that `create_query_set` builds a visibility-result
            // buffer of the size asked for, that `reset_query_set` and
            // `resolve_query_set` reach it, and that `query_results` reads it
            // back. `visibilityResultBuffer` is a property of every
            // `MTLRenderPassDescriptor`, so `crate::adapter` reports the flag
            // unconditionally and the gate below never fires — it is the flag's
            // own arm, not a device question, exactly as `PushConstants` above.
            Capability::OcclusionQuery => gated(
                Features::OCCLUSION_QUERY,
                "this device reports no OCCLUSION_QUERY",
            ),
            // The counter-sampled two are the ones a visibility-result buffer
            // cannot serve: both want an `MTLCounterSampleBuffer`, which is
            // built from a set this device advertises.
            Capability::TimestampQuery | Capability::PipelineStatisticsQuery => {
                Support::No(NO_COUNTER_SAMPLED_SET)
            }
            // `MTLSharedEvent` is the seam's timeline almost verbatim.
            Capability::TimelineSemaphore | Capability::CpuTimelineWait => gated(
                Features::TIMELINE_SEMAPHORE,
                "this device reports no TIMELINE_SEMAPHORE",
            ),
            Capability::BinarySemaphore => Support::Yes,
            Capability::TimelineWaitBeforeSignal => Support::No(
                "this backend runs one queue and the seam gives no way to signal an MTLSharedEvent \
                 from the CPU, so only an earlier submission can satisfy such a wait — it would \
                 stop the queue rather than wait",
            ),
        }
    }

    /// The graphics queue, and only ever the graphics queue.
    ///
    /// Metal has one `MTLCommandQueue` type that accepts render, compute and
    /// blit work, and no queue families at all — so there is nothing an
    /// [`QueueKind::Compute`] or [`QueueKind::Transfer`] handle could name that
    /// the graphics one does not. `crcbl-mtl`'s adapter module reports neither
    /// [`Features::ASYNC_COMPUTE_QUEUE`] nor [`Features::TRANSFER_QUEUE`] for
    /// the same reason, and the seam's contract — "check the feature first, or
    /// just fall back to `Graphics`, which always exists" — is then answered
    /// consistently from both ends.
    fn queue(&self, kind: QueueKind) -> Option<QueueHandle> {
        match kind {
            QueueKind::Graphics => Some(queue_handle(self.inner.tag, kind)),
            QueueKind::Compute | QueueKind::Transfer => None,
        }
    }

    // --- resources ---

    fn create_buffer(&self, desc: &BufferDesc<'_>) -> Result<BufferHandle, HalError> {
        if desc.size == 0 {
            return Err(HalError::InvalidDescriptor(
                "BufferDesc::size must be non-zero".to_string(),
            ));
        }
        let ceiling = self.inner.caps.limits.max_storage_buffer_range;
        if desc.size > ceiling {
            return Err(HalError::InvalidDescriptor(format!(
                "BufferDesc::size {} exceeds this device's maxBufferLength of {ceiling}",
                desc.size
            )));
        }
        // Metal has no buffer usage flags — every `MTLBuffer` can be bound
        // anywhere — with one exception the seam already models: taking a
        // buffer's GPU address is `MTLBuffer::gpuAddress`, a Metal 3 call, and
        // `BufferUsage::DEVICE_ADDRESS` is documented as requiring the feature
        // that reports it. Refusing here is what stops a Tier B caller getting
        // a buffer whose address it cannot take.
        if desc.usage.contains(BufferUsage::DEVICE_ADDRESS)
            && !self
                .inner
                .caps
                .features
                .contains(Features::BUFFER_DEVICE_ADDRESS)
        {
            return Err(HalError::InvalidDescriptor(
                "BufferUsage::DEVICE_ADDRESS needs Features::BUFFER_DEVICE_ADDRESS, which this \
                 device does not report"
                    .to_string(),
            ));
        }

        let Some(raw) = self
            .inner
            .raw
            .newBufferWithLength_options(to_ns(desc.size), conv::resource_options(desc.memory))
        else {
            // `newBufferWithLength:options:` returns nil when the allocation
            // fails, which is the one failure the seam has a name for here.
            return Err(HalError::OutOfDeviceMemory);
        };
        if let Some(label) = desc.label {
            raw.setLabel(Some(&NSString::from_str(label)));
        }
        let handle = self.state().buffers.insert(BufferEntry {
            owner: self.inner.id,
            raw,
            size: desc.size,
            location: desc.memory,
        });
        Ok(self.stamp(handle))
    }

    fn destroy_buffer(&self, buffer: BufferHandle) {
        let mut state = self.state();
        take_owned(&mut state.buffers, buffer, &*self.inner);
    }

    /// Copies `data` into a host-visible buffer.
    ///
    /// # `DeviceLocal` is refused, never silently dropped
    ///
    /// A [`MemoryLocation::DeviceLocal`] buffer is `MTLStorageMode::Private`
    /// and has no `contents` pointer at all — Metal's only route into one is a
    /// blit from a staging buffer, which is
    /// [`CommandEncoder::copy_buffer_to_buffer`](crcbl_hal::CommandEncoder::copy_buffer_to_buffer)
    /// and not this call. So this refuses with
    /// [`HalError::InvalidDescriptor`] naming the location,
    /// which is both what the seam documents ("`InvalidDescriptor` … if the
    /// buffer is not host-visible") and what `crcbl-vk` answers for the same
    /// call, so the two backends disagree about nothing.
    ///
    /// The alternative — accepting the call and writing nothing — is the shape
    /// this workspace treats as a defect: a caller would upload a mesh, draw
    /// it, and see an empty screen with no error anywhere.
    ///
    /// # No `didModifyRange:`
    ///
    /// That call exists for `MTLStorageMode::Managed` and, per Metal's own
    /// header, "is not valid to invoke … on buffers of other storage modes".
    /// Both host-visible locations here are `Shared` — see
    /// `conv::storage_mode` for that decision — where the write is coherent
    /// at the next command-buffer boundary with no call at all.
    fn write_buffer(&self, buffer: BufferHandle, offset: u64, data: &[u8]) -> Result<(), HalError> {
        let state = self.state();
        let entry = lookup(&state.buffers, "buffer", buffer, &*self.inner)?;
        if !entry.location.is_mappable() {
            return Err(HalError::InvalidDescriptor(format!(
                "write_buffer needs a host-visible buffer; this one is {:?}, which Metal can \
                 only reach through a blit from a staging buffer — record a copy_buffer_to_buffer \
                 instead",
                entry.location
            )));
        }
        let end = offset.checked_add(data.len() as u64).ok_or_else(|| {
            HalError::InvalidDescriptor("write_buffer range overflows".to_string())
        })?;
        if end > entry.size {
            return Err(HalError::InvalidDescriptor(format!(
                "write_buffer range {offset}..{end} exceeds the buffer's {} bytes",
                entry.size
            )));
        }
        if data.is_empty() {
            return Ok(());
        }
        let contents = entry.raw.contents();
        // SAFETY: `contents` points at `entry.size` bytes of a `Shared`
        // allocation this device made, the range was bounds-checked against
        // that size immediately above, and the two regions cannot overlap
        // because `data` is a caller-owned slice and the destination is the
        // buffer's own storage. The pointer does not escape this block, and the
        // device lock is held for the whole copy.
        unsafe {
            core::ptr::copy_nonoverlapping(
                data.as_ptr(),
                contents.as_ptr().cast::<u8>().add(offset as usize),
                data.len(),
            );
        }
        Ok(())
    }

    /// Records what a readback is waiting for. It does not wait.
    ///
    /// # The completion point, and why it is a command buffer
    ///
    /// The seam says a readback covers "everything submitted to this device
    /// before this call" unless [`ReadbackDesc::after`] names a timeline point.
    /// On Metal the first of those is **the last command buffer of the last
    /// submission**, retained here: a queue runs its command buffers in commit
    /// order, so that one reaching
    /// [`MTLCommandBufferStatus::Completed`] means every buffer committed
    /// before it has completed too.
    ///
    /// Nothing submitted yet is not an error — the set of work being waited for
    /// is empty, and an empty set is already finished — so the request is
    /// created ready. That is the same answer WebGPU's `mapAsync` gives for a
    /// buffer nothing has written.
    ///
    /// An explicit [`ReadbackDesc::after`] naming a value **nothing has
    /// signalled yet is perfectly legal here**, unlike the same value in a
    /// [`Device::submit`] wait. The difference is who is waiting: a submission's
    /// wait occupies the queue and so cannot be satisfied by anything the queue
    /// has not yet reached, while this one is a CPU-side poll that simply keeps
    /// answering [`ReadbackState::Pending`] until a later submission signals it.
    ///
    /// # Why a `Shared` buffer still needs the wait
    ///
    /// It is tempting to read a `HostReadback` buffer's `contents` immediately,
    /// since the pointer is valid the moment the buffer exists. Metal
    /// guarantees coherency for a `Shared` resource **at command buffer
    /// boundaries**, so a blit that has not completed has not necessarily
    /// landed. Reading early is how a screenshot comes back as the frame before
    /// it, intermittently, on one machine.
    fn request_readback(&self, desc: &ReadbackDesc<'_>) -> Result<ReadbackHandle, HalError> {
        let mut state = self.state();
        let entry = lookup(&state.buffers, "buffer", desc.buffer, &*self.inner)?;
        if entry.location != MemoryLocation::HostReadback {
            return Err(HalError::InvalidDescriptor(format!(
                "request_readback needs a HostReadback buffer; this one is {:?}",
                entry.location
            )));
        }
        let end = desc
            .offset
            .checked_add(desc.size)
            .ok_or_else(|| HalError::InvalidDescriptor("readback range overflows".to_string()))?;
        if end > entry.size {
            return Err(HalError::InvalidDescriptor(format!(
                "readback range {}..{end} exceeds the buffer's {} bytes",
                desc.offset, entry.size
            )));
        }

        let wait = match desc.after {
            Some(after) => {
                let semaphore = lookup(
                    &state.semaphores,
                    "semaphore",
                    after.semaphore,
                    &*self.inner,
                )?;
                if semaphore.shared.is_none() {
                    return Err(HalError::Unsupported {
                        backend: BackendKind::Metal,
                        what: "ReadbackDesc::after must name a timeline semaphore",
                    });
                }
                ReadbackWait::Timeline {
                    semaphore: after.semaphore,
                    value: after.value,
                }
            }
            None => ReadbackWait::Submission(state.last_submission.clone()),
        };
        let handle = state.readbacks.insert(ReadbackEntry {
            owner: self.inner.id,
            buffer: desc.buffer,
            offset: desc.offset,
            size: desc.size,
            wait,
        });
        Ok(self.stamp(handle))
    }

    /// Observes the completion point, and copies the bytes once it is reached.
    ///
    /// A poll, never a wait: the two branches below are
    /// `MTLCommandBuffer::status` and `MTLSharedEvent::signaledValue`, both of
    /// which answer immediately. Neither `waitUntilCompleted` nor
    /// `waitUntilSignaledValue:timeoutMS:` appears here, which is the whole
    /// point of the shape [`crcbl_hal::readback`] argues for.
    fn poll_readback(
        &self,
        readback: ReadbackHandle,
        out: &mut [u8],
    ) -> Result<ReadbackState, HalError> {
        let state = self.state();
        let entry = lookup(&state.readbacks, "readback", readback, &*self.inner)?;
        if out.len() as u64 != entry.size {
            return Err(HalError::InvalidDescriptor(format!(
                "poll_readback needs exactly {} bytes, got {}",
                entry.size,
                out.len()
            )));
        }
        match &entry.wait {
            ReadbackWait::Submission(None) => {}
            ReadbackWait::Submission(Some(command_buffer)) => match command_buffer.status() {
                MTLCommandBufferStatus::Completed => {}
                MTLCommandBufferStatus::Error => {
                    return Err(HalError::DeviceLost(format!(
                        "the submission a readback was waiting for failed: {}",
                        crate::fault::describe(command_buffer)
                    )));
                }
                _ => return Ok(ReadbackState::Pending),
            },
            ReadbackWait::Timeline { semaphore, value } => {
                // Resolved from the handle stored at request time, so a
                // semaphore destroyed in between fails lookup here rather than
                // being dereferenced after its last reference went.
                let entry = lookup(&state.semaphores, "semaphore", *semaphore, &*self.inner)?;
                let Some(shared) = entry.shared.as_ref() else {
                    return Err(HalError::Unsupported {
                        backend: BackendKind::Metal,
                        what: "ReadbackDesc::after must name a timeline semaphore",
                    });
                };
                if shared.signaledValue() < *value {
                    return Ok(ReadbackState::Pending);
                }
            }
        }
        if entry.size == 0 {
            return Ok(ReadbackState::Ready);
        }
        // Resolved from the handle stored at request time for the same reason:
        // a buffer destroyed between request and poll fails lookup rather than
        // having its freed `contents` read.
        let buffer = lookup(&state.buffers, "buffer", entry.buffer, &*self.inner)?;
        let contents = buffer.raw.contents();
        // SAFETY: `contents` covers the whole `Shared` allocation, the range was
        // bounds-checked against `entry.size` at request time and `out.len()`
        // was just checked to equal it, the completion point above has been
        // reached so the GPU's writes are visible, and the read happens under
        // the device lock with the pointer never escaping this block.
        unsafe {
            core::ptr::copy_nonoverlapping(
                contents.as_ptr().cast::<u8>().add(entry.offset as usize),
                out.as_mut_ptr(),
                out.len(),
            );
        }
        Ok(ReadbackState::Ready)
    }

    fn destroy_readback(&self, readback: ReadbackHandle) {
        // No driver object of its own: the mapping belongs to the buffer, which
        // the caller still owns, and the retained command buffer is released
        // with the entry. Dropping the tracking entry is the whole of it.
        let mut state = self.state();
        take_owned(&mut state.readbacks, readback, &*self.inner);
    }

    fn create_image(&self, desc: &ImageDesc<'_>) -> Result<ImageHandle, HalError> {
        let limits = self.inner.caps.limits;
        let extent = desc.extent;
        if extent.width == 0 || extent.height == 0 || extent.depth_or_layers == 0 {
            return Err(HalError::InvalidDescriptor(format!(
                "ImageDesc::extent has a zero dimension: {extent:?}"
            )));
        }
        if desc.usage.is_empty() {
            return Err(HalError::InvalidDescriptor(
                "ImageDesc::usage is empty, so the image could never be used".to_string(),
            ));
        }
        if !desc.samples.is_power_of_two() || desc.samples > limits.max_sample_count {
            return Err(HalError::InvalidDescriptor(format!(
                "ImageDesc::samples is {} but this device supports powers of two up to {}",
                desc.samples, limits.max_sample_count
            )));
        }
        // Two rules `MTLTextureDescriptor` states in its own headers, checked
        // here because Metal enforces them by *raising* rather than by
        // returning nil — and an Objective-C exception crossing back into Rust
        // aborts the process, which is a far worse answer than an `Err` for
        // what is a caller's descriptor bug either way.
        if desc.samples > 1 && desc.mip_levels > 1 {
            return Err(HalError::InvalidDescriptor(format!(
                "a multisampled image has one mip level, not {}",
                desc.mip_levels
            )));
        }
        if matches!(desc.image_type, ImageType::D1) && extent.height != 1 {
            return Err(HalError::InvalidDescriptor(format!(
                "ImageType::D1 requires a height of 1, not {}",
                extent.height
            )));
        }
        let is_3d = matches!(desc.image_type, ImageType::D3);
        let ceiling = if is_3d {
            limits.max_image_3d
        } else {
            limits.max_image_2d
        };
        if extent.width > ceiling || extent.height > ceiling {
            return Err(HalError::InvalidDescriptor(format!(
                "ImageDesc::extent {extent:?} exceeds this device's {ceiling}-texel limit"
            )));
        }
        if is_3d && extent.depth_or_layers > limits.max_image_3d {
            return Err(HalError::InvalidDescriptor(format!(
                "a volume's depth {} exceeds this device's {} limit",
                extent.depth_or_layers, limits.max_image_3d
            )));
        }
        if !is_3d && extent.depth_or_layers > limits.max_image_array_layers {
            return Err(HalError::InvalidDescriptor(format!(
                "{} array layers exceed this device's {} limit",
                extent.depth_or_layers, limits.max_image_array_layers
            )));
        }
        // Two format questions Metal answers per device rather than per API,
        // and both would otherwise raise rather than return nil.
        if desc.format == Format::D24UnormS8Uint
            && !self.inner.raw.isDepth24Stencil8PixelFormatSupported()
        {
            return Err(HalError::InvalidDescriptor(
                "Format::D24UnormS8Uint is not supported by this device — Apple silicon reports \
                 no; use Format::D32Float, which the seam already prefers"
                    .to_string(),
            ));
        }
        if desc.format.is_compressed()
            && !self
                .inner
                .caps
                .features
                .contains(Features::TEXTURE_COMPRESSION_BC)
        {
            return Err(HalError::InvalidDescriptor(format!(
                "{:?} needs Features::TEXTURE_COMPRESSION_BC, which this device does not report",
                desc.format
            )));
        }

        let mip_levels = desc.mip_levels.max(1);
        let layers = if is_3d { 1 } else { extent.depth_or_layers };
        let descriptor = MTLTextureDescriptor::new();
        descriptor.setTextureType(conv::texture_type(desc.image_type, layers, desc.samples));
        descriptor.setPixelFormat(conv::pixel_format(desc.format));
        descriptor.setUsage(conv::texture_usage(desc.usage, desc.format));
        // An image is device-local — `MTLStorageMode::Private` — and
        // `ImageDesc` has no field that could say otherwise; see
        // `MemoryLocation`. Metal ignores a `Private` texture's cache mode, so
        // the mapping's neutral value is the one to hand it.
        descriptor.setStorageMode(conv::storage_mode(MemoryLocation::DeviceLocal));
        descriptor.setCpuCacheMode(conv::cpu_cache_mode(MemoryLocation::DeviceLocal));
        // SAFETY: `objc2` marks these setters unsafe because Metal does not
        // bounds-check them and raises on an out-of-range value. Every argument
        // below was checked against this device's `Limits` above — the extent
        // against the dimension ceilings, the layer count against
        // `max_image_array_layers`, the sample count against
        // `max_sample_count` — and `mip_levels` is clamped to at least one, so
        // none of them can be the value Metal objects to.
        unsafe {
            descriptor.setWidth(extent.width as NSUInteger);
            descriptor.setHeight(extent.height as NSUInteger);
            descriptor.setDepth(if is_3d {
                extent.depth_or_layers as NSUInteger
            } else {
                1
            });
            descriptor.setArrayLength(layers as NSUInteger);
            descriptor.setMipmapLevelCount(mip_levels as NSUInteger);
            descriptor.setSampleCount(desc.samples.max(1) as NSUInteger);
        }

        let Some(raw) = self.inner.raw.newTextureWithDescriptor(&descriptor) else {
            return Err(HalError::OutOfDeviceMemory);
        };
        if let Some(label) = desc.label {
            raw.setLabel(Some(&NSString::from_str(label)));
        }
        let handle = self.state().images.insert(ImageEntry {
            owner: self.inner.id,
            raw,
            format: desc.format,
            image_type: desc.image_type,
            // The offscreen ring is built through this call and flips the flag
            // afterwards, so the default here is what a caller's own image is.
            swapchain_owned: false,
        });
        Ok(self.stamp(handle))
    }

    /// Destroys an image — unless the swapchain owns it.
    ///
    /// The seam says an [`AcquiredFrame`] image is the swapchain's and must not
    /// be destroyed, and this is what makes that a no-op rather than a ring with
    /// a hole in it. `destroy_*` has no way to report anything, which is why it
    /// logs.
    fn destroy_image(&self, image: ImageHandle) {
        let mut state = self.state();
        if let Ok(local) = local_handle::<ImageEntry, _>("image", image, &*self.inner)
            && state
                .images
                .get(local)
                .is_some_and(|entry| entry.owner == self.inner.id && entry.swapchain_owned)
        {
            crcbl_core::log::warn!(
                "crcbl-mtl: destroy_image on a swapchain-owned image {image:?}; the swapchain owns \
                 its images and destroys them with itself, so this is ignored"
            );
            return;
        }
        take_owned(&mut state.images, image, &*self.inner);
    }

    /// Creates a view onto a subrange of an image.
    ///
    /// Metal's `newTextureViewWithPixelFormat:textureType:levels:slices:` takes
    /// absolute ranges, so the seam's
    /// [`ImageSubresourceRange::ALL`](crcbl_hal::ImageSubresourceRange::ALL)
    /// sentinel is resolved here against the texture's own level and slice
    /// counts, read back off the object rather than remembered. The texture's
    /// `textureType` is read the same way, because it is what decides whether
    /// the seam's `D2` view is a multisampled one — see `conv::view_texture_type`.
    ///
    /// A view that reinterprets nothing is the image's own texture rather than a
    /// second object; the block below the range checks argues why.
    fn create_image_view(&self, desc: &ImageViewDesc<'_>) -> Result<ImageViewHandle, HalError> {
        let mut state = self.state();
        let entry = lookup(&state.images, "image", desc.image, &*self.inner)?;
        // A depth or stencil format has no compatible reinterpretation in
        // Metal — the depth formats are their own class — so the texture was
        // not created with `MTLTextureUsagePixelFormatView` and asking for a
        // different format here would raise. Refuse it while it is still an
        // error a caller can catch.
        if desc.format != entry.format
            && (desc.format.is_depth_stencil() || entry.format.is_depth_stencil())
        {
            return Err(HalError::InvalidDescriptor(format!(
                "a view of a {:?} image cannot reinterpret it as {:?}: Metal permits no \
                 reinterpretation involving a depth or stencil format",
                entry.format, desc.format
            )));
        }

        let texture = entry.raw.clone();
        // The texture's own type, not one derived from the descriptor a second
        // time: it is what Metal checks the requested view type against, and
        // `conv::view_texture_type` needs it to tell a multisampled 2D texture
        // from a single-sampled one, which `ImageViewType` cannot say.
        let source_type = texture.textureType();
        let Some(view_type) = conv::view_texture_type(desc.view_type, source_type) else {
            return Err(HalError::InvalidDescriptor(format!(
                "a {:?} view of a multisampled image is not a view Metal can make: the only \
                 texture types compatible with a multisampled texture are 2D multisample and 2D \
                 multisample array",
                desc.view_type
            )));
        };
        let levels = texture.mipmapLevelCount();
        let slices = texture.arrayLength();
        let range = desc.range;
        let base_mip = range.base_mip as NSUInteger;
        let base_layer = range.base_layer as NSUInteger;
        if base_mip >= levels || base_layer >= slices {
            return Err(HalError::InvalidDescriptor(format!(
                "view starts at mip {base_mip} layer {base_layer}, and the image has {levels} \
                 mips and {slices} layers"
            )));
        }
        let mip_count = resolve_count(range.mip_count, base_mip, levels);
        let layer_count = resolve_count(range.layer_count, base_layer, slices);
        if mip_count == 0 || layer_count == 0 {
            return Err(HalError::InvalidDescriptor(
                "an image view covering no mip levels or no layers is not a view".to_string(),
            ));
        }

        // **A view that reinterprets nothing is the texture itself**, the same
        // answer `Device::insert_drawable_rows` gives for a drawable and for
        // the same reason: Metal has no separate view object, so a view whose
        // format, type and ranges all match its texture would be a second
        // `MTLTexture` object describing exactly what the first one already
        // does. `wgpu-hal`'s Metal backend short-circuits the identical case
        // (`create_texture_view` in `wgpu-hal-30.0.0/src/metal/device.rs`),
        // noting that framebuffer-only textures cannot be aliased at all and
        // that it also works around Metal bugs with aliased array textures.
        let view = if desc.format == entry.format
            && view_type == source_type
            && base_mip == 0
            && mip_count == levels
            && base_layer == 0
            && layer_count == slices
        {
            texture
        } else {
            // SAFETY: `objc2` marks this unsafe because Metal does not
            // bounds-check the two ranges. Both were just clamped to the
            // texture's own `mipmapLevelCount` and `arrayLength`, read off the
            // texture above, and both bases were checked to be inside them.
            let view = unsafe {
                texture.newTextureViewWithPixelFormat_textureType_levels_slices(
                    conv::pixel_format(desc.format),
                    view_type,
                    NSRange::new(base_mip, mip_count),
                    NSRange::new(base_layer, layer_count),
                )
            };
            let Some(view) = view else {
                return Err(HalError::Backend(
                    "MTLTexture::newTextureViewWithPixelFormat:textureType:levels:slices: \
                     returned nil"
                        .to_string(),
                ));
            };
            // Only this branch labels. The other one holds the image's own
            // texture, and `setLabel:` on it would rename the *image* in every
            // capture and every Metal diagnostic — a view's debug name is not
            // worth overwriting its image's. Such a view shows up under the
            // image's label, which names the same object anyway.
            if let Some(label) = desc.label {
                view.setLabel(Some(&NSString::from_str(label)));
            }
            view
        };
        let handle = state.views.insert(ViewEntry {
            owner: self.inner.id,
            raw: view,
            format: desc.format,
            swapchain_owned: false,
        });
        Ok(self.stamp(handle))
    }

    /// Destroys a view — unless the swapchain owns it. See
    /// [`destroy_image`](Self::destroy_image).
    fn destroy_image_view(&self, view: ImageViewHandle) {
        let mut state = self.state();
        if let Ok(local) = local_handle::<ViewEntry, _>("image view", view, &*self.inner)
            && state
                .views
                .get(local)
                .is_some_and(|entry| entry.owner == self.inner.id && entry.swapchain_owned)
        {
            crcbl_core::log::warn!(
                "crcbl-mtl: destroy_image_view on a swapchain-owned view {view:?}; the swapchain \
                 owns its views and reissues them on every reconfigure, so this is ignored"
            );
            return;
        }
        take_owned(&mut state.views, view, &*self.inner);
    }

    fn create_sampler(&self, desc: &SamplerDesc<'_>) -> Result<SamplerHandle, HalError> {
        let cap = self.inner.caps.limits.max_sampler_anisotropy;
        if desc.anisotropy < 1.0 {
            return Err(HalError::InvalidDescriptor(format!(
                "anisotropy {} is below 1.0, which is the value that disables it",
                desc.anisotropy
            )));
        }
        if desc.anisotropy > cap {
            return Err(HalError::InvalidDescriptor(format!(
                "anisotropy {} exceeds max_sampler_anisotropy {cap}",
                desc.anisotropy
            )));
        }

        let descriptor = MTLSamplerDescriptor::new();
        descriptor.setMinFilter(conv::min_mag_filter(desc.min_filter));
        descriptor.setMagFilter(conv::min_mag_filter(desc.mag_filter));
        descriptor.setMipFilter(conv::mip_filter(desc.mip_filter));
        descriptor.setSAddressMode(conv::address_mode(desc.address_mode[0]));
        descriptor.setTAddressMode(conv::address_mode(desc.address_mode[1]));
        descriptor.setRAddressMode(conv::address_mode(desc.address_mode[2]));
        // Transparent black, which is what the seam's `ClampToBorder`
        // documents and what a shadow atlas needs; opaque black would put a
        // black frame around every clamped sample.
        descriptor.setBorderColor(objc2_metal::MTLSamplerBorderColor::TransparentBlack);
        descriptor.setLodMinClamp(desc.lod_min);
        descriptor.setLodMaxClamp(desc.lod_max);
        // Metal takes an integer here and rejects anything outside 1...16; the
        // seam's `f32` is a Vulkan-shaped spelling of the same knob. Truncating
        // rather than rounding keeps the promise a limit makes: never sample
        // with more taps than the caller asked for.
        #[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)]
        let anisotropy = desc.anisotropy.min(MAX_SAMPLER_ANISOTROPY) as NSUInteger;
        descriptor.setMaxAnisotropy(anisotropy.max(1));
        if let Some(compare) = desc.compare {
            descriptor.setCompareFunction(conv::compare_function(compare));
        }
        // A sampler's argument-buffer support is fixed at creation and cannot
        // be retrofitted, so a sampler made now that could not be written into
        // an argument buffer is one a later slice would have to re-create.
        // `crcbl_mtl::binding` binds flat argument tables and does not need
        // this; it is asked for anyway, keyed off the device's own tier query
        // rather than off a reported feature, because the feature it used to be
        // keyed off is exactly the one the binding slice took away.
        if self.inner.raw.argumentBuffersSupport() == MTLArgumentBuffersTier::Tier2 {
            descriptor.setSupportArgumentBuffers(true);
        }
        if let Some(label) = desc.label {
            descriptor.setLabel(Some(&NSString::from_str(label)));
        }

        let Some(raw) = self.inner.raw.newSamplerStateWithDescriptor(&descriptor) else {
            return Err(HalError::Backend(
                "MTLDevice::newSamplerStateWithDescriptor: returned nil".to_string(),
            ));
        };
        let handle = self.state().samplers.insert(SamplerEntry {
            owner: self.inner.id,
            raw,
        });
        Ok(self.stamp(handle))
    }

    fn destroy_sampler(&self, sampler: SamplerHandle) {
        let mut state = self.state();
        take_owned(&mut state.samplers, sampler, &*self.inner);
    }

    // --- shaders and pipelines ---

    /// Compiles [`ShaderModuleDesc::msl`] through
    /// `MTLDevice::newLibraryWithSource:options:error:`; see
    /// `crcbl_mtl::pipeline`.
    fn create_shader_module(
        &self,
        desc: &ShaderModuleDesc<'_>,
    ) -> Result<ShaderModuleHandle, HalError> {
        self.create_shader_module_impl(desc)
    }

    fn destroy_shader_module(&self, module: ShaderModuleHandle) {
        self.destroy_shader_module_impl(module);
    }

    /// Places a set's bindings in Metal's flat argument tables. See
    /// `crcbl_mtl::binding` for the model and for why a bindless layout is
    /// refused.
    fn create_bind_group_layout(
        &self,
        desc: &BindGroupLayoutDesc<'_>,
    ) -> Result<BindGroupLayoutHandle, HalError> {
        self.create_bind_group_layout_impl(desc)
    }

    fn destroy_bind_group_layout(&self, layout: BindGroupLayoutHandle) {
        self.destroy_bind_group_layout_impl(layout);
    }

    fn create_bind_group(&self, desc: &BindGroupDesc<'_>) -> Result<BindGroupHandle, HalError> {
        self.create_bind_group_impl(desc)
    }

    fn update_bind_group(
        &self,
        group: BindGroupHandle,
        entries: &[BindGroupEntry],
    ) -> Result<(), HalError> {
        self.update_bind_group_impl(group, entries)
    }

    fn destroy_bind_group(&self, group: BindGroupHandle) {
        self.destroy_bind_group_impl(group);
    }

    /// Lays out every set in the argument tables. See `crcbl_mtl::pipeline`.
    fn create_pipeline_layout(
        &self,
        desc: &PipelineLayoutDesc<'_>,
    ) -> Result<PipelineLayoutHandle, HalError> {
        self.create_pipeline_layout_impl(desc)
    }

    fn destroy_pipeline_layout(&self, layout: PipelineLayoutHandle) {
        self.destroy_pipeline_layout_impl(layout);
    }

    fn create_graphics_pipeline(
        &self,
        desc: &GraphicsPipelineDesc<'_>,
    ) -> Result<GraphicsPipelineHandle, HalError> {
        self.create_graphics_pipeline_impl(desc)
    }

    /// Still refused, and the obstacle is this backend rather than Metal.
    ///
    /// Metal 3 has the object/mesh stages and Slang emits them for
    /// `-target metal` — `crates/crcbl-shaders/msl/mesh_shader.metal` is
    /// committed proof, with `[[mesh]]` and `[[object]]` functions in it. What
    /// is missing here is the backend: a mesh pipeline is built from an
    /// `MTLMeshRenderPipelineDescriptor` rather than the
    /// `MTLRenderPipelineDescriptor` `create_graphics_pipeline_impl` fills, and
    /// the dispatch is `drawMeshThreadgroups:`.
    ///
    /// This backend accordingly reports no `Features::MESH_SHADER`, so the
    /// capability model keeps a caller away from here; the refusal is what a
    /// caller that ignored it gets.
    fn create_mesh_pipeline(
        &self,
        _desc: &crcbl_hal::MeshPipelineDesc<'_>,
    ) -> Result<GraphicsPipelineHandle, HalError> {
        Err(not_yet(
            "mesh pipelines: Metal has the object/mesh stages and Slang emits them, but this \
             backend builds no MTLMeshRenderPipelineDescriptor (the Metal mesh slice)",
        ))
    }

    fn destroy_graphics_pipeline(&self, pipeline: GraphicsPipelineHandle) {
        self.destroy_graphics_pipeline_impl(pipeline);
    }

    fn create_compute_pipeline(
        &self,
        desc: &ComputePipelineDesc<'_>,
    ) -> Result<ComputePipelineHandle, HalError> {
        self.create_compute_pipeline_impl(desc)
    }

    fn destroy_compute_pipeline(&self, pipeline: ComputePipelineHandle) {
        self.destroy_compute_pipeline_impl(pipeline);
    }

    // --- queries ---

    /// Creates an **occlusion** query set, and refuses the other two kinds by
    /// name.
    ///
    /// An occlusion pool on Metal is a plain `MTLBuffer` — the one a render pass
    /// names through `MTLRenderPassDescriptor::visibilityResultBuffer` — so it
    /// is allocated here at `crate::query`'s stride and nothing about it asks the
    /// device a question. `HostReadback` is what makes it CPU-readable, which is
    /// what [`Device::query_results`] reads it through.
    ///
    /// **This builds the pool and no way to write it, deliberately.**
    /// [`Capability::OcclusionQuery`] is defined as "a
    /// [`QueryKind::Occlusion`](crcbl_hal::QueryKind::Occlusion) query set" and
    /// nothing more, because [`crcbl_hal::CommandEncoder`] has no begin/end query
    /// verb: `setVisibilityResultMode:offset:` has no seam call to be reached
    /// from, so no work a caller records can ever count into this buffer. That is
    /// what `crcbl-vk`'s and `crcbl-webgpu`'s `Support::Yes` mean too.
    /// `crate::query` records which `MTLVisibilityResultMode` the slice that adds
    /// the verb should pass, and why it is not the one `wgpu-hal` picks.
    ///
    /// # The two refusals, and why they are Metal's rather than this slice's
    ///
    /// MTL3 owns the blit encoder, which is what a Vulkan-shaped query set
    /// resolves through, so this looked like the slice that would land
    /// `MTLCounterSampleBuffer` as well. It is not, and there are two obstacles:
    ///
    /// * **A sample buffer must name a counter set, and this device has none.**
    ///   `MTLCounterSampleBufferDescriptor::setCounterSet:` takes one of
    ///   `MTLDevice::counterSets`, and the paravirtual GPU this backend's CI runs
    ///   on reports zero of them — measured by `crate::adapter`'s counter probe,
    ///   which prints the list. `NO_COUNTER_SAMPLED_SET` in this module carries
    ///   the sentence, so this refusal and [`Device::supports`]'s declaration
    ///   cannot drift.
    /// * **Metal samples counters at *boundaries*, and which boundaries exist is
    ///   a per-device question.** `MTLDevice::supportsCounterSampling:` takes an
    ///   `MTLCounterSamplingPoint` and answers separately for each — the query
    ///   would not exist if every device answered yes. The seam's
    ///   [`write_timestamp`](crcbl_hal::CommandEncoder::write_timestamp) is a
    ///   free-standing command legal anywhere outside a pass, and the weakest
    ///   thing a device can offer is a *stage* boundary declared in a pass
    ///   descriptor **before the pass is created**.
    ///
    ///   That last sentence used to end "which is not somewhere the seam's call
    ///   can reach", and **that was asserted rather than checked.** Reading the
    ///   callers says otherwise: `crcbl_hal::null`'s recorder puts
    ///   `WriteTimestamp` through the same `need_outside` check copies and
    ///   barriers get, so the seam forbids the call *inside* a pass, and the one
    ///   caller obeys it — `crcbl_render::timing`'s `pass_begin` and `pass_end`
    ///   are called by `crcbl_render::graph` immediately before
    ///   `begin_render_pass` and immediately after `end_render_pass`. A
    ///   stage-boundary sample is therefore where the seam's timestamps already
    ///   are, and how much narrowing this costs is open rather than settled.
    ///
    /// A half-built version would be worse than none: sampling only where the
    /// device happens to support a blit or dispatch boundary gives a profiler HUD
    /// that silently reports timings on some Macs and zeroes on others, which is
    /// the "not implemented arriving as passed" failure this workspace treats as
    /// a defect. So [`Features::TIMESTAMP_QUERY`] stays absent — with it,
    /// [`Limits::timestamp_period_ns`](crcbl_hal::Limits::timestamp_period_ns),
    /// which Metal has no fixed answer for either.
    ///
    /// # Errors
    ///
    /// [`HalError::Unsupported`] for the two counter-sampled kinds,
    /// [`HalError::InvalidDescriptor`] for a set of no queries, and
    /// [`HalError::OutOfDeviceMemory`] if the allocation fails.
    fn create_query_set(&self, desc: &QuerySetDesc<'_>) -> Result<QuerySetHandle, HalError> {
        match desc.kind {
            QueryKind::Occlusion => {}
            QueryKind::Timestamp | QueryKind::PipelineStatistics => {
                return Err(unsupported(NO_COUNTER_SAMPLED_SET));
            }
        }
        let bytes = crate::query::buffer_bytes(desc.count)?;
        let Some(raw) = self.inner.raw.newBufferWithLength_options(
            to_ns(bytes),
            conv::resource_options(MemoryLocation::HostReadback),
        ) else {
            // As `create_buffer`: nil is the allocation failing, and it is the
            // one failure the seam has a name for here.
            return Err(HalError::OutOfDeviceMemory);
        };
        if let Some(label) = desc.label {
            raw.setLabel(Some(&NSString::from_str(label)));
        }
        let handle = self.state().query_sets.insert(QuerySetEntry {
            owner: self.inner.id,
            raw,
            count: desc.count,
        });
        Ok(self.stamp(handle))
    }

    /// Releases the visibility-result buffer.
    ///
    /// No deletion queue, for the reason this module's docs give for every other
    /// `destroy_*`: an `MTLCommandBuffer` retains the resources its encoders
    /// name, so releasing the last reference to a buffer a submitted
    /// [`reset_query_set`](crcbl_hal::CommandEncoder::reset_query_set) is still
    /// filling frees it after that command buffer completes rather than during.
    fn destroy_query_set(&self, set: QuerySetHandle) {
        let mut state = self.state();
        take_owned(&mut state.query_sets, set, &*self.inner);
    }

    /// Reads visibility counts straight out of the pool's buffer.
    ///
    /// The seam's "directly, without a resolve-to-buffer round trip" is free
    /// here: the pool *is* the destination, it is `Shared` storage with a
    /// `contents` pointer, and there is nothing to resolve. `crcbl-dx12` has to
    /// submit a command list inside this call for the same read, because an
    /// `ID3D12QueryHeap` cannot be mapped.
    ///
    /// **A caller still has to have waited.** Metal makes a `Shared` allocation
    /// coherent at command-buffer boundaries, so a read taken while the command
    /// buffer that filled the pool is still running sees whatever was there
    /// before — the same rule `Device::request_readback` is built around, and the
    /// reason it exists rather than this call blocking.
    ///
    /// # Errors
    ///
    /// [`HalError::InvalidHandle`] or [`HalError::ForeignObject`] for a set this
    /// device did not create, and [`HalError::InvalidDescriptor`] for a range
    /// that exceeds the set.
    fn query_results(
        &self,
        set: QuerySetHandle,
        first_query: u32,
        out: &mut [u64],
    ) -> Result<(), HalError> {
        let state = self.state();
        let entry = lookup(&state.query_sets, "query set", set, &*self.inner)?;
        crate::query::check_range(entry.count, first_query, out.len() as u64)?;
        if out.is_empty() {
            return Ok(());
        }
        let offset = crate::query::span_bytes(u64::from(first_query));
        let contents = entry.raw.contents();
        // SAFETY: `contents` points at the `buffer_bytes(entry.count)` bytes of
        // a `Shared` allocation this device made for this set, and `check_range`
        // has just bounded `first_query + out.len()` by `entry.count` — so the
        // source span is inside the allocation. The two regions cannot overlap:
        // `out` is a caller-owned slice and the source is the buffer's own
        // storage. The pointer does not escape this block, and the device lock is
        // held for the whole copy. The copy is byte-wise, so neither end needs an
        // alignment `contents` does not promise.
        unsafe {
            core::ptr::copy_nonoverlapping(
                contents.as_ptr().cast::<u8>().add(offset as usize),
                out.as_mut_ptr().cast::<u8>(),
                size_of_val(out),
            );
        }
        Ok(())
    }

    // --- synchronisation ---

    /// Creates a semaphore: `MTLSharedEvent` for a timeline, `MTLEvent` for a
    /// binary one.
    ///
    /// # Timeline
    ///
    /// `docs/plan/09-backends-metal-dx12.md`'s mapping table names
    /// `MTLSharedEvent`, and it is a one-for-one fit: a monotonic `u64` that
    /// the GPU signals through `encodeSignalEvent:value:`, that the GPU waits on
    /// through `encodeWaitForEvent:value:`, and that the CPU can both read
    /// (`signaledValue`) and block on (`waitUntilSignaledValue:timeoutMS:`).
    /// The seam's [`SemaphoreKind::Timeline::initial_value`] becomes a
    /// CPU-side `setSignaledValue:` before the object is ever handed out.
    ///
    /// # Binary, and the one rule it comes with
    ///
    /// Metal has no one-shot GPU-only signal, so a binary semaphore is a plain
    /// `MTLEvent` — device-private, and deliberately *not* a shared one, because
    /// a binary semaphore has no CPU-visible value and lending it one would
    /// invite exactly the reads [`Device::semaphore_value`] refuses. The value
    /// is kept in this crate's own `SemaphoreEntry`: a submission that signals it
    /// encodes the next integer, and a submission that waits on it waits for
    /// the one most recently encoded. **So a binary semaphore must be signalled
    /// by an earlier submission than the one that waits on it**, which is how
    /// the seam says they are used ("the swapchain owns them").
    ///
    /// MTL3 left that rule open against WSI acquire, where in Vulkan the
    /// *presentation engine* signals and no submission does. **The surface
    /// slice closed it by not needing it:** `nextDrawable` blocks the CPU and
    /// hands back a ready texture, so `crcbl_mtl::swapchain` creates no
    /// semaphore at all and hands back `None` for both of
    /// [`AcquiredFrame`]'s — the implicit-acquire shape the seam documents for
    /// `crcbl-wgpu`. Nothing here signals a binary semaphore from outside a
    /// submission, so the rule stands unweakened.
    fn create_semaphore(&self, desc: &SemaphoreDesc<'_>) -> Result<SemaphoreHandle, HalError> {
        let label = desc.label.map(NSString::from_str);
        let (raw, shared) = match desc.kind {
            SemaphoreKind::Timeline { initial_value } => {
                let Some(shared) = self.inner.raw.newSharedEvent() else {
                    return Err(HalError::Backend(
                        "MTLDevice::newSharedEvent returned nil".to_string(),
                    ));
                };
                shared.setSignaledValue(initial_value);
                if let Some(label) = label.as_deref() {
                    shared.setLabel(Some(label));
                }
                let raw: Retained<ProtocolObject<dyn MTLEvent>> =
                    ProtocolObject::from_retained(shared.clone());
                (raw, Some(shared))
            }
            SemaphoreKind::Binary => {
                let Some(raw) = self.inner.raw.newEvent() else {
                    return Err(HalError::Backend(
                        "MTLDevice::newEvent returned nil".to_string(),
                    ));
                };
                if let Some(label) = label.as_deref() {
                    raw.setLabel(Some(label));
                }
                (raw, None)
            }
        };
        let encoded = match desc.kind {
            SemaphoreKind::Timeline { initial_value } => initial_value,
            SemaphoreKind::Binary => 0,
        };
        let handle = self.state().semaphores.insert(SemaphoreEntry {
            owner: self.inner.id,
            raw,
            shared,
            encoded,
        });
        Ok(self.stamp(handle))
    }

    fn destroy_semaphore(&self, semaphore: SemaphoreHandle) {
        // No deletion queue, for the reason the module docs give: an
        // `MTLCommandBuffer` retains every object it references, so an event a
        // committed submission still has to signal outlives this call by
        // itself.
        let mut state = self.state();
        take_owned(&mut state.semaphores, semaphore, &*self.inner);
    }

    fn semaphore_value(&self, semaphore: SemaphoreHandle) -> Result<u64, HalError> {
        let state = self.state();
        let entry = lookup(&state.semaphores, "semaphore", semaphore, &*self.inner)?;
        let Some(shared) = entry.shared.as_ref() else {
            return Err(HalError::Unsupported {
                backend: BackendKind::Metal,
                what: "a binary semaphore has no value to read",
            });
        };
        Ok(shared.signaledValue())
    }

    /// Blocks until every wait is satisfied, or until the timeout runs out.
    ///
    /// Metal offers one wait per event (`waitUntilSignaledValue:timeoutMS:`)
    /// rather than Vulkan's wait-on-many, so several waits are performed in
    /// sequence against a shared deadline — which is the same answer either way,
    /// because the seam's contract is that *all* of them must be reached. The
    /// deadline rather than a per-wait timeout is what stops N waits taking N
    /// times as long to time out.
    ///
    /// Metal counts in **milliseconds** and the seam counts in nanoseconds, so
    /// the remaining budget is rounded *up*: rounding down would turn a
    /// sub-millisecond timeout into a busy poll that never waits at all.
    fn wait_semaphores(&self, waits: &[SemaphoreWait], timeout_ns: u64) -> Result<bool, HalError> {
        if waits.is_empty() {
            return Ok(true);
        }
        let mut events = Vec::with_capacity(waits.len());
        {
            let state = self.state();
            for wait in waits {
                let entry = lookup(&state.semaphores, "semaphore", wait.semaphore, &*self.inner)?;
                let Some(shared) = entry.shared.as_ref() else {
                    return Err(HalError::Unsupported {
                        backend: BackendKind::Metal,
                        what: "a binary semaphore cannot be waited on from the CPU",
                    });
                };
                events.push((shared.clone(), wait.value));
            }
        }
        // The lock is released before blocking: holding it across a wait would
        // deadlock against the very submission that is going to signal.
        let start = Instant::now();
        for (event, value) in events {
            let remaining = if timeout_ns == u64::MAX {
                u64::MAX
            } else {
                let elapsed = u64::try_from(start.elapsed().as_nanos()).unwrap_or(u64::MAX);
                timeout_ns.saturating_sub(elapsed)
            };
            // Round up, and never to zero for a non-zero budget.
            let milliseconds = if remaining == u64::MAX {
                u64::MAX
            } else {
                remaining.div_ceil(1_000_000)
            };
            if !event.waitUntilSignaledValue_timeoutMS(value, milliseconds) {
                // Not an error: a frame-pacing poll times out routinely.
                return Ok(false);
            }
        }
        Ok(true)
    }

    /// Blocks until the device has finished everything submitted so far.
    ///
    /// Metal has no `vkDeviceWaitIdle`. What it has is ordering: an
    /// `MTLCommandQueue` runs its command buffers in commit order, so an empty
    /// command buffer committed now completes only after everything committed
    /// before it, and waiting on that one waits for all of them. That is the
    /// standard Metal idiom for a device idle and it is what this does.
    ///
    /// It is a real wait today even though [`Device::submit`] still refuses:
    /// the queue is real, the command buffer is real, and this is the call that
    /// proves both work. When submission lands, nothing here changes.
    fn wait_idle(&self) -> Result<(), HalError> {
        let command_buffer = self.new_command_buffer("crcbl wait_idle")?;
        command_buffer.commit();
        command_buffer.waitUntilCompleted();
        if command_buffer.status() == MTLCommandBufferStatus::Error {
            return Err(HalError::DeviceLost(format!(
                "the wait_idle command buffer failed: {}",
                crate::fault::describe(&command_buffer)
            )));
        }
        Ok(())
    }

    // --- commands ---

    fn create_command_encoder(&self, desc: &CommandEncoderDesc<'_>) -> Box<dyn CommandEncoder> {
        Box::new(MetalCommandEncoder::new(Arc::clone(&self.inner), desc))
    }

    fn destroy_command_buffer(&self, buffer: CommandBufferHandle) {
        // Releasing the last reference is the whole of it, and it is safe even
        // mid-flight: an `MTLCommandBuffer` is retained by its queue until it
        // completes, and it retains every resource it references. The seam
        // requires the caller to have waited anyway.
        let mut state = self.state();
        take_owned(&mut state.command_buffers, buffer, &*self.inner);
    }

    /// Commits command buffers to the queue, with the waits before and the
    /// signals after.
    ///
    /// # Metal has no `vkQueueSubmit`, so this is three things at once
    ///
    /// An `MTLCommandBuffer` is created already recording and `commit` *is* the
    /// submission, so there is no submit-info structure to fill and nowhere to
    /// hang a wait or a signal except on a command buffer. The three parts of
    /// [`SubmitInfo`] therefore land in three different places:
    ///
    /// * **Signals go on the last command buffer**, through
    ///   `encodeSignalEvent:value:`. That call is legal only when no encoder is
    ///   open on the buffer, which is exactly the state
    ///   [`CommandEncoder::finish`] leaves it in, and it fires when the GPU has
    ///   finished everything encoded before it — which is the seam's "signalled
    ///   on completion".
    /// * **Waits go on a command buffer of their own, committed first.**
    ///   `encodeWaitForEvent:value:` has to precede the work it gates, and the
    ///   work's command buffer already has its encoders recorded, so there is no
    ///   way to insert one at the front. A leading empty buffer holding only the
    ///   waits does the job because a queue schedules its command buffers in
    ///   commit order — the same property `wait_idle` has relied on since MTL2.
    /// * **The command buffers themselves** are committed in order.
    ///
    /// # Wait-before-signal is refused rather than deadlocked
    ///
    /// That same commit ordering is what makes a wait for a value **no earlier
    /// submission has encoded** unsatisfiable: the queue would have to reach a
    /// later submission to signal it, and it cannot while an earlier one is
    /// still waiting. This device has one queue and the seam offers no CPU-side
    /// signal, so there is nowhere else the value could come from. Metal reports
    /// nothing for that — the queue simply stops, with the process alive and no
    /// log line anywhere — so the check is made here and turns a silent hang
    /// into an [`HalError::Unsupported`]. That variant rather than
    /// [`HalError::InvalidDescriptor`]: the wait is well-formed and `crcbl-vk`
    /// performs it, so what is missing is the capability
    /// ([`Capability::TimelineWaitBeforeSignal`](crcbl_hal::Capability::TimelineWaitBeforeSignal),
    /// on the reviewed divergence list for this backend) rather than a field the
    /// caller got wrong. The seam's own use satisfies it by construction: a
    /// frames-in-flight timeline waits on the value the *previous* frame
    /// signalled.
    ///
    /// # Timeline values may not go backwards
    ///
    /// `MTLSharedEvent` is monotonic and so is the seam
    /// ([`SemaphoreSignal::value`](crcbl_hal::SemaphoreSignal::value) "must
    /// exceed the semaphore's current value"). Signalling a value at or below
    /// one already encoded does not fail loudly on Metal: the event simply
    /// never reaches the value a later waiter is blocked on, and the process
    /// hangs. The check below is against the highest value *encoded*, not the
    /// highest *signalled*, because a signal sitting in a committed buffer has
    /// not fired yet and comparing against `signaledValue` would let two
    /// in-flight submissions encode the same value.
    fn submit(&self, queue: QueueHandle, submit: &SubmitInfo<'_>) -> Result<(), HalError> {
        // First, because it is the one thing here with its own contract: a
        // handle from another device is a caller bug that must be reported as
        // the crossing it is.
        self.inner.check_queue(queue)?;
        let mut state = self.state();

        // Everything is resolved and checked before anything is committed. A
        // submission that fails halfway would leave some of its command buffers
        // running and some not, with no way to tell a caller which.
        let mut commands = Vec::with_capacity(submit.command_buffers.len());
        for handle in submit.command_buffers {
            let entry = lookup(
                &state.command_buffers,
                "command buffer",
                *handle,
                &*self.inner,
            )?;
            if entry.committed {
                return Err(HalError::InvalidDescriptor(
                    "this command buffer was already submitted; Metal raises on a second commit"
                        .to_string(),
                ));
            }
            // The same handle twice in one `SubmitInfo` would pass the flag
            // check above — nothing has been committed yet — and then reach
            // `commit` twice inside this call.
            if submit
                .command_buffers
                .iter()
                .filter(|other| *other == handle)
                .count()
                > 1
            {
                return Err(HalError::InvalidDescriptor(
                    "the same command buffer appears twice in one submission; Metal raises on a \
                     second commit"
                        .to_string(),
                ));
            }
            commands.push(entry.raw.clone());
        }
        let mut waits = Vec::with_capacity(submit.waits.len());
        for wait in submit.waits {
            let entry = lookup(&state.semaphores, "semaphore", wait.semaphore, &*self.inner)?;
            // A binary semaphore's value is the one most recently encoded onto
            // it; the seam says its `value` field is ignored.
            let value = if entry.shared.is_some() {
                // **Wait-before-signal is a deadlock here, so it is refused.**
                // This device has one queue and the seam gives no way to signal
                // an event from the CPU, so the only thing that can ever move a
                // timeline is a submission on this queue — and a queue cannot
                // reach a later submission's signal while an earlier one is
                // still waiting for it. Metal has no diagnostic for that: the
                // queue simply stops, with the process alive and nothing in any
                // log. Vulkan behaves the same way for the same reason; the
                // difference is only that this one can be caught.
                //
                // **`Unsupported`, not `InvalidDescriptor`.** The wait is
                // well-formed and `crcbl-vk` performs it; what is absent is
                // anywhere for the signal to come from, which is exactly
                // `Capability::TimelineWaitBeforeSignal` and is on the reviewed
                // divergence list for this backend. A caller matching on the
                // variant can split its submission in two; one reading "your
                // descriptor is wrong" would go looking for a typo.
                if wait.value > entry.encoded {
                    return Err(unsupported(
                        "a wait for a timeline value nothing submitted has encoded a signal past: \
                         on a single queue only an earlier submission can satisfy it, so this \
                         would stop the queue rather than wait",
                    ));
                }
                wait.value
            } else {
                entry.encoded
            };
            waits.push((entry.raw.clone(), value));
        }
        let mut signals = Vec::with_capacity(submit.signals.len());
        for signal in submit.signals {
            let entry = lookup(
                &state.semaphores,
                "semaphore",
                signal.semaphore,
                &*self.inner,
            )?;
            // The floor is the highest value encoded onto this semaphore so
            // far, *including* by an earlier signal in this same submission —
            // otherwise two signals on one semaphore in one `SubmitInfo` would
            // both be checked against the stale value and could encode the same
            // number twice, which is the silent version of the hang below.
            let floor = signals
                .iter()
                .filter(|(handle, _, _)| *handle == signal.semaphore)
                .map(|(_, _, value)| *value)
                .max()
                .unwrap_or(entry.encoded);
            let value = if entry.shared.is_some() {
                if signal.value <= floor {
                    return Err(HalError::InvalidDescriptor(format!(
                        "a timeline semaphore signalled with {} has already been signalled with \
                         {floor}; MTLSharedEvent values are monotonic and a waiter on the higher \
                         value would never wake",
                        signal.value
                    )));
                }
                signal.value
            } else {
                floor + 1
            };
            signals.push((signal.semaphore, entry.raw.clone(), value));
        }
        if signals.is_empty() && commands.is_empty() && waits.is_empty() {
            // Nothing to do, and nothing to record as the completion point.
            return Ok(());
        }

        // A submission with signals but no work still needs somewhere to encode
        // them, and one with waits needs a buffer in front. Both are empty
        // command buffers, which is the cheapest object Metal has.
        let mut committed: Vec<Retained<ProtocolObject<dyn MTLCommandBuffer>>> = Vec::new();
        if !waits.is_empty() {
            let leading = self.new_command_buffer("crcbl submit waits")?;
            for (event, value) in &waits {
                leading.encodeWaitForEvent_value(event, *value);
            }
            committed.push(leading);
        }
        committed.extend(commands);
        if !signals.is_empty() {
            // Onto the last command buffer of the submission when there is one,
            // so the signal fires when *its* work is done rather than one
            // command buffer later.
            let last = match committed.last() {
                Some(last) => last.clone(),
                None => {
                    let trailing = self.new_command_buffer("crcbl submit signals")?;
                    committed.push(trailing.clone());
                    trailing
                }
            };
            for (_, event, value) in &signals {
                last.encodeSignalEvent_value(event, *value);
            }
        }

        for raw in &committed {
            raw.commit();
        }
        // Retained so that a failure is noticed even when nothing ever waits on
        // this submission — see `DeviceState::in_flight`. The sweep inside
        // `track` is what keeps that set bounded.
        for raw in &committed {
            state.track(raw.clone());
        }
        // Committed only now, for the same reason `crcbl-vk` bumps its
        // submission counter after `vkQueueSubmit2` rather than before: a value
        // recorded for a submission that never reached the driver would leave
        // every waiter on it stuck for the life of the process.
        for handle in submit.command_buffers {
            if let Ok(local) =
                local_handle::<CommandBufferEntry, _>("command buffer", *handle, &*self.inner)
                && let Some(entry) = state.command_buffers.get_mut(local)
            {
                entry.committed = true;
            }
        }
        for (handle, _, value) in signals {
            if let Ok(local) = local_handle::<SemaphoreEntry, _>("semaphore", handle, &*self.inner)
                && let Some(entry) = state.semaphores.get_mut(local)
            {
                entry.encoded = value;
            }
        }
        state.last_submission = committed.pop();
        Ok(())
    }

    // --- presentation ---

    /// Configures a `CAMetalLayer`, or builds the offscreen ring. See
    /// `crcbl_mtl::swapchain`.
    fn create_swapchain(&self, desc: &SwapchainDesc<'_>) -> Result<SwapchainHandle, SurfaceError> {
        self.create_swapchain_impl(desc)
    }

    fn reconfigure_swapchain(
        &self,
        swapchain: SwapchainHandle,
        desc: &SwapchainDesc<'_>,
    ) -> Result<(), SurfaceError> {
        self.reconfigure_swapchain_impl(swapchain, desc)
    }

    fn destroy_swapchain(&self, swapchain: SwapchainHandle) {
        self.destroy_swapchain_impl(swapchain);
    }

    /// `nextDrawable` on a layer, a ring step offscreen. See
    /// `crcbl_mtl::swapchain` for why neither returns a semaphore.
    fn acquire_next_frame(
        &self,
        swapchain: SwapchainHandle,
    ) -> Result<AcquiredFrame, SurfaceError> {
        self.acquire_next_frame_impl(swapchain)
    }

    fn present(&self, queue: QueueHandle, present: &PresentInfo<'_>) -> Result<(), SurfaceError> {
        self.present_impl(queue, present)
    }

    /// Blocks until `present_id`'s drawable has been shown, which on Metal
    /// means until the `addPresentedHandler:` block that present attached has
    /// run.
    ///
    /// Metal has neither a number for a present nor a handle to wait on, so the
    /// count is kept on this side of the seam and the caller's own id is what
    /// it is kept under; `crcbl_mtl::present` holds that ledger and
    /// `crcbl_mtl::swapchain` attaches the handler. The seam's immediate
    /// answers that do apply are an offscreen ring, which has no drawable, and
    /// an id this swapchain was never given — one whose present was refused, or
    /// one from before a reconfigure. The third, a device without
    /// [`Features::PRESENT_FEEDBACK`](crcbl_hal::Features::PRESENT_FEEDBACK),
    /// cannot arise here: every Metal device has the capability and
    /// `crcbl_mtl::adapter` reports it unconditionally.
    fn wait_until_presented(
        &self,
        swapchain: SwapchainHandle,
        present_id: u64,
        timeout: Duration,
    ) -> Result<(), SurfaceError> {
        self.wait_until_presented_impl(swapchain, present_id, timeout)
    }

    /// Always [`DisplayTiming::Unknown`]: this backend does not advertise
    /// [`Features::PRESENT_TIMING`](crcbl_hal::Features::PRESENT_TIMING).
    ///
    /// # What Metal and Core Animation offer, and why none of it is reachable
    ///
    /// `CAMetalLayer` has no refresh-cadence property at all. Its
    /// `maximumDrawableCount` is the size of the drawable ring — how many
    /// frames may be in flight — which is a swapchain image count and says
    /// nothing about the panel. What this backend *does* already have is
    /// `MTLDrawable`'s `addPresentedHandler:` and the `presentedTime` it
    /// carries, which is what [`Features::PRESENT_FEEDBACK`] is built on; two
    /// of those differenced give the interval between frames that were shown,
    /// which is a measurement of the loop rather than a statement about the
    /// display, and looks the same on a fixed panel and on a ProMotion one
    /// holding steady.
    ///
    /// The real answers are `CADisplayLink` — whose `targetTimestamp`,
    /// `duration` and `preferredFrameRateRange` between them do express the
    /// dynamics — and `NSScreen`'s `maximumFramesPerSecond` /
    /// `minimumRefreshInterval`. **Both are out of reach by this crate's own
    /// design, not by omission**: the module docs state that no `NSView`,
    /// `NSWindow` or `NSScreen` is ever reached from anywhere here, and that
    /// restriction is what discharges the crate's thread-affinity argument.
    /// Answering this properly on Metal is therefore a shell-side change that
    /// hands the timing down, not a query to add in this file.
    ///
    /// The handle is resolved first regardless, per the seam's obligation 3.
    fn display_timing(&self, swapchain: SwapchainHandle) -> Result<DisplayTiming, SurfaceError> {
        self.display_timing_impl(swapchain)
    }
}

/// Resolves a seam subresource count against the object's real extent.
///
/// [`ImageSubresourceRange::ALL`](crcbl_hal::ImageSubresourceRange::ALL) means
/// "every remaining one", and a count that runs past the end is clamped rather
/// than refused — the request is satisfiable, just wider than the object, and
/// Metal would raise on the raw number.
fn resolve_count(requested: u32, base: NSUInteger, total: NSUInteger) -> NSUInteger {
    let remaining = total.saturating_sub(base);
    if requested == crcbl_hal::ImageSubresourceRange::ALL {
        return remaining;
    }
    (requested as NSUInteger).min(remaining)
}

#[cfg(test)]
pub(crate) mod tests {
    use super::*;
    use std::time::Duration;

    use crcbl_hal::{
        Barriers, BufferCopy, BufferImageCopy, ClearValue, ColorAttachment, ColorTargetState,
        DepthStencilState, Extent3d, ImageAspect, ImageBarrier, ImageSubresourceLayers,
        ImageSubresourceRange, ImageUsage, ImageViewType, Instance, LoadOp, MultisampleState,
        Offset3d, PrimitiveState, Rect2d, RenderPassDesc, ResourceState, SemaphoreSignal,
        ShaderEntry, StoreOp,
    };
    use objc2_metal::MTLHazardTrackingMode;

    use crate::instance::tests::{desc as device_desc, open as open_instance};
    // Only the hardware suite draws, and only a draw sets a viewport, carries a
    // depth attachment or names a comparison for one.
    #[cfg(feature = "mtl-e2e")]
    use crcbl_hal::{CompareOp, DepthBias, DepthStencilAttachment, Viewport};

    /// Every [`MemoryLocation`] the seam has, so the buffer tests cover all
    /// three rather than the one that was convenient.
    const LOCATIONS: &[MemoryLocation] = &[
        MemoryLocation::DeviceLocal,
        MemoryLocation::HostUpload,
        MemoryLocation::HostReadback,
    ];

    /// A device, opened through this crate's own type so a test can reach the
    /// pools underneath it.
    ///
    /// **The instance comes back wrapped in [`crate::fault::Validated`], and
    /// that is the teardown every device test in this crate now has.** It
    /// derefs to [`MetalInstance`], so a caller reads exactly as before; what it
    /// adds is a `Drop` that asserts Metal's validation layer was interposed and
    /// that no command buffer this device submitted failed. Wiring it here
    /// rather than at the end of each test is the only version that cannot be
    /// forgotten by the seventy-second one.
    pub(crate) fn open_device() -> (crate::fault::Validated, MetalDevice) {
        let instance = open_instance();
        let adapters = instance.adapters();
        assert!(!adapters.is_empty(), "a Mac has at least one adapter");
        let device = instance
            .open_device(&device_desc(adapters[0].id))
            .expect("a Metal device opens with no required features");
        let validated = crate::fault::Validated::new(instance, &device);
        (validated, device)
    }

    fn buffer(size: u64, memory: MemoryLocation) -> BufferDesc<'static> {
        BufferDesc {
            label: Some("crcbl-mtl test buffer"),
            size,
            usage: BufferUsage::STORAGE | BufferUsage::TRANSFER_DST,
            memory,
        }
    }

    /// Reads a host-visible buffer's bytes straight out of Metal, bypassing the
    /// seam — which has no read path yet, and which is exactly why this is the
    /// only way to observe that `write_buffer` wrote anything.
    fn read_back(device: &MetalDevice, handle: BufferHandle, len: usize) -> Vec<u8> {
        let state = device.state();
        let entry = lookup(&state.buffers, "buffer", handle, &*device.inner)
            .expect("the buffer is live and this device's");
        assert!(entry.location.is_mappable(), "not a readable buffer");
        assert!(len as u64 <= entry.size, "reading past the buffer");
        let contents = entry.raw.contents();
        // SAFETY: `contents` covers `entry.size` bytes of a live `Shared`
        // allocation, `len` was just asserted to be within it, and the read
        // happens under the device lock with no GPU work in flight.
        unsafe { core::slice::from_raw_parts(contents.as_ptr().cast::<u8>(), len) }.to_vec()
    }

    // --- handle tagging, with no device in it -------------------------------
    //
    // Every other test in this file goes through a live `MTLDevice`, which
    // cannot construct the case the tagging exists for: two owners whose pools
    // issue *identical bits*. Live devices allocate at their own pace and drift
    // apart immediately, so a foreign-handle check made through them passes on
    // the pool index and proves nothing about the tag. Two fresh `Pool`s here do
    // issue identical bits, and that is asserted before it is relied on.

    /// A stand-in owner, so the rules above are testable without a device or an
    /// instance — the two things that implement [`Owner`] for real.
    #[derive(Clone, Copy, Debug)]
    struct TestOwner {
        id: u64,
        tag: u32,
    }

    impl TestOwner {
        /// An owner with `id`, tagged exactly as a device with that id would be.
        fn new(id: u64) -> Self {
            Self {
                id,
                tag: device_tag(id),
            }
        }
    }

    impl Owner for TestOwner {
        fn owner_id(&self) -> u64 {
            self.id
        }

        fn tag(&self) -> u32 {
            self.tag
        }
    }

    /// A stand-in table entry, so the lookup rules do not need a real
    /// `MTLBuffer` to be checked.
    #[derive(Debug)]
    struct TestEntry {
        owner: u64,
    }

    owned!(TestEntry);

    /// A tag is never zero, because zero is what a handle nobody issued carries.
    ///
    /// The wrap is asserted rather than assumed: it is the residual hole the
    /// module comment above describes, and a `%` that produced a zero tag would
    /// make the wrapping owner accept every hand-made handle.
    #[test]
    fn every_metal_owner_id_gets_a_non_zero_tag_and_ids_wrap_rather_than_collide_at_zero() {
        for id in [1u64, 2, 254, 255, 256, 1_000_000] {
            assert_ne!(
                device_tag(id),
                0,
                "owner {id} would accept an untagged handle"
            );
        }
        // Neighbouring ids must differ, or two devices opened back to back would
        // share a tag and fall through to the id check.
        assert_ne!(device_tag(1), device_tag(2));
    }

    /// A stamped handle round-trips: the tag comes back out and the pool index
    /// and generation survive untouched.
    ///
    /// The generation is the half that must not move — `Pool` keys its staleness
    /// on it, so a stamp that disturbed it would make every handle stale on its
    /// first use.
    #[test]
    fn stamping_a_metal_handle_preserves_the_pool_index_and_the_generation() {
        let owner = TestOwner::new(7);
        let mut pool: Pool<TestEntry> = Pool::new();
        let raw = pool.insert(TestEntry { owner: owner.id });

        let stamped: Handle<crcbl_hal::Buffer> = stamp(&owner, raw);
        assert_eq!(handle_tag(stamped), owner.tag, "the tag did not survive");
        assert_eq!(
            stamped.generation(),
            raw.generation(),
            "the generation moved, which would make the handle stale at once"
        );
        assert_eq!(
            stamped.index() & POOL_INDEX_MASK,
            raw.index(),
            "the pool index did not survive"
        );

        let back: Handle<TestEntry> =
            local_handle("entry", stamped, &owner).expect("this owner's own handle");
        assert_eq!(
            back, raw,
            "the round trip did not recover the pool's handle"
        );
        assert_eq!(
            lookup(&pool, "entry", stamped, &owner)
                .expect("the entry is live")
                .owner,
            owner.id
        );
    }

    /// **The three outcomes are three errors.** This is the property the tagging
    /// exists for, and it is checked on handles whose *bits are identical* — two
    /// fresh pools issue the same index and generation, so the tag is the only
    /// thing that can tell them apart.
    #[test]
    fn a_foreign_metal_handle_is_foreign_a_stale_one_is_stale_and_an_untagged_one_is_neither() {
        let a = TestOwner::new(1);
        let b = TestOwner::new(2);
        let mut pool_a: Pool<TestEntry> = Pool::new();
        let mut pool_b: Pool<TestEntry> = Pool::new();
        let raw_a = pool_a.insert(TestEntry { owner: a.id });
        let raw_b = pool_b.insert(TestEntry { owner: b.id });
        assert_eq!(
            raw_a, raw_b,
            "two fresh pools must issue identical bits, or this test proves nothing"
        );

        let on_a: Handle<crcbl_hal::Buffer> = stamp(&a, raw_a);
        let on_b: Handle<crcbl_hal::Buffer> = stamp(&b, raw_b);
        assert_ne!(on_a, on_b, "the tag is the only difference and it vanished");

        let error = lookup(&pool_b, "entry", on_a, &b).expect_err("A's handle is not B's");
        assert!(
            matches!(error, HalError::ForeignObject { kind, .. } if kind == "entry"),
            "{error:?}"
        );
        // B's own still resolves, so the check is not simply refusing
        // everything.
        lookup(&pool_b, "entry", on_b, &b).expect("B's own handle");
        // The mutable path answers the same three ways, and it is the one the
        // swapchain slice reaches for.
        let error = lookup_mut(&mut pool_b, "entry", on_a, &b).expect_err("A's handle is not B's");
        assert!(
            matches!(error, HalError::ForeignObject { kind, .. } if kind == "entry"),
            "{error:?}"
        );
        lookup_mut(&mut pool_b, "entry", on_b, &b).expect("B's own handle");

        // A destroy with a foreign handle must not take the local object that
        // shares its bits.
        assert!(
            !take_owned(&mut pool_b, on_a, &b),
            "a foreign handle removed a local entry"
        );
        lookup(&pool_b, "entry", on_b, &b).expect("B's entry survived a foreign destroy");

        // Destroyed, then stale — a different error from foreign.
        assert!(
            remove_owned(&mut pool_b, on_b, &b).is_some(),
            "B's own handle names B's own entry"
        );
        let error = lookup(&pool_b, "entry", on_b, &b).expect_err("the entry was removed");
        assert!(matches!(error, HalError::InvalidHandle { .. }), "{error:?}");

        // A hand-made handle carries no tag at all, so no owner ever issued it.
        let untagged: Handle<crcbl_hal::Buffer> =
            Handle::from_bits(1 << 32).expect("generation 1 is non-zero");
        assert_eq!(handle_tag(untagged), 0);
        let error =
            local_handle::<TestEntry, _>("entry", untagged, &a).expect_err("nobody issued that");
        assert!(matches!(error, HalError::InvalidHandle { .. }), "{error:?}");
    }

    /// Queue handles are per owner and per kind, and carry the tag so a queue
    /// from another device is detectable.
    #[test]
    fn metal_queue_handles_differ_by_device_and_by_kind() {
        let kinds = [QueueKind::Graphics, QueueKind::Compute, QueueKind::Transfer];
        let mut seen: Vec<QueueHandle> = Vec::new();
        for owner in [TestOwner::new(1), TestOwner::new(2)] {
            for kind in kinds {
                let handle = queue_handle(owner.tag, kind);
                assert_eq!(handle_tag(handle), owner.tag, "{kind:?}");
                assert!(
                    !seen.contains(&handle),
                    "{kind:?} on owner {} duplicates an earlier queue handle",
                    owner.id
                );
                seen.push(handle);
            }
        }
        assert_eq!(seen.len(), kinds.len() * 2);
    }

    // --- the clear rung ----------------------------------------------------

    /// The attachment every render-pass test clears.
    ///
    /// 64 texels wide so a row of `Rgba8Unorm` is 256 bytes, which clears every
    /// buffer-row alignment any Metal implementation asks for by a wide margin;
    /// four rows so the row stride is exercised at all rather than being the
    /// whole copy.
    const TARGET: Extent3d = Extent3d::d2(64, 4);

    /// Bytes one full copy of [`TARGET`] occupies: `Rgba8Unorm` is four bytes a
    /// texel, tightly packed.
    const TARGET_BYTES: usize = 64 * 4 * 4;

    /// **The clear colour, and why the expected bytes are what they are.**
    ///
    /// [`Format::Rgba8Unorm`] is a plain UNORM format: **no sRGB transfer
    /// function on either read or write**, so the hardware stores a clear
    /// component `c` as `round(c × 255)` and nothing else happens to it. Every
    /// component here is written as `n / 255`, so `round((n / 255) × 255)` is
    /// exactly `n` — the round trip is exact for all 256 values, and the
    /// half-ulp of `f32` division is nowhere near the 0.5 that would round to a
    /// neighbour.
    ///
    /// The four values are chosen so that **no byte of the result is zero** and
    /// no two channels are equal. A clear to black asserting zeroes passes just
    /// as well against a buffer nothing ever wrote, and equal channels pass
    /// against a channel swizzle.
    ///
    /// The linear/sRGB trap this avoids is real and this repo has hit it: had
    /// the attachment been `Rgba8UnormSrgb`, a clear of `0.5` would land as
    /// `188`, not `128`, because the hardware encodes on write. See
    /// `crcbl_mtl::conv`.
    const CLEAR: [f32; 4] = [17.0 / 255.0, 34.0 / 255.0, 51.0 / 255.0, 1.0];

    /// What [`CLEAR`] must read back as, per the derivation above.
    const CLEAR_TEXEL: [u8; 4] = [0x11, 0x22, 0x33, 0xFF];

    /// A second colour, for the load/store pair. Same derivation.
    const OTHER: [f32; 4] = [204.0 / 255.0, 187.0 / 255.0, 170.0 / 255.0, 1.0];

    /// What [`OTHER`] must read back as.
    const OTHER_TEXEL: [u8; 4] = [0xCC, 0xBB, 0xAA, 0xFF];

    /// A byte pattern that appears nowhere in any expected result, written into
    /// every readback buffer before it is used.
    ///
    /// Without it "the copy never ran" and "the copy ran" are indistinguishable
    /// on a buffer Metal happened to hand back zeroed.
    const POISON: u8 = 0xA5;

    /// A colour target of `extent` in `format`, with a whole-image view of it.
    ///
    /// The image, the view and the view's subresource range all take the one
    /// `format`: Metal permits a view to reinterpret a texture's format, and a
    /// test that reinterpreted one here by accident would be asserting about a
    /// pair of formats rather than the one it named.
    fn color_target_of(
        device: &MetalDevice,
        extent: Extent3d,
        format: Format,
    ) -> (ImageHandle, ImageViewHandle) {
        let image = device
            .create_image(&ImageDesc {
                label: Some("crcbl-mtl clear target"),
                image_type: ImageType::D2,
                extent,
                format,
                mip_levels: 1,
                samples: 1,
                usage: ImageUsage::COLOR_ATTACHMENT | ImageUsage::TRANSFER_SRC,
            })
            .expect("a colour attachment");
        let view = device
            .create_image_view(&ImageViewDesc {
                label: Some("crcbl-mtl clear view"),
                image,
                view_type: ImageViewType::D2,
                format,
                range: ImageSubresourceRange::all(format),
            })
            .expect("a whole-image view");
        (image, view)
    }

    /// The [`TARGET`]-sized colour target every clear test uses.
    fn color_target(device: &MetalDevice) -> (ImageHandle, ImageViewHandle) {
        color_target_of(device, TARGET, Format::Rgba8Unorm)
    }

    /// A host-readable buffer, poisoned so an absent copy cannot pass.
    fn readback_buffer(device: &MetalDevice, size: u64) -> BufferHandle {
        let handle = device
            .create_buffer(&buffer(size, MemoryLocation::HostReadback))
            .expect("a readback buffer");
        device
            .write_buffer(handle, 0, &vec![POISON; size as usize])
            .expect("HostReadback is host-visible");
        handle
    }

    /// The whole of an `extent`-sized image, copied into the start of a buffer.
    fn whole_image_copy_of(
        image: ImageHandle,
        into: BufferHandle,
        extent: Extent3d,
    ) -> BufferImageCopy {
        BufferImageCopy {
            buffer: into,
            buffer_offset: 0,
            buffer_row_length: 0,
            buffer_image_height: 0,
            image,
            image_subresource: ImageSubresourceLayers {
                aspect: ImageAspect::COLOR,
                mip: 0,
                base_layer: 0,
                layer_count: 1,
            },
            image_offset: Offset3d { x: 0, y: 0, z: 0 },
            image_extent: extent,
        }
    }

    /// The same, for the [`TARGET`]-sized image the clear tests use.
    fn whole_image_copy(image: ImageHandle, into: BufferHandle) -> BufferImageCopy {
        whole_image_copy_of(image, into, TARGET)
    }

    /// Polls a readback to completion, with a deadline rather than a sleep.
    ///
    /// This is the loop `crcbl_hal::readback` sanctions for "callers that
    /// genuinely have nothing else to do"; the deadline is what turns a
    /// completion point that is never reached into a failed test instead of a
    /// hung one.
    fn drain(device: &MetalDevice, readback: ReadbackHandle, size: usize) -> Vec<u8> {
        let mut out = vec![0u8; size];
        let deadline = Instant::now() + Duration::from_secs(10);
        loop {
            let state = device
                .poll_readback(readback, &mut out)
                .expect("the readback resolves");
            if state.is_ready() {
                return out;
            }
            assert!(
                Instant::now() < deadline,
                "the readback never became ready; the submission it waits on has not completed"
            );
            std::thread::yield_now();
        }
    }

    /// One texel repeated across a whole [`TARGET`]-sized copy.
    fn expected(texel: [u8; 4]) -> Vec<u8> {
        texel.iter().copied().cycle().take(TARGET_BYTES).collect()
    }

    /// Clears to `first`, then runs a second pass with `load` and the clear
    /// value `second`, and returns what the image holds afterwards.
    fn two_passes(
        device: &MetalDevice,
        first: [f32; 4],
        load: LoadOp,
        second: [f32; 4],
    ) -> Vec<u8> {
        let (image, view) = color_target(device);
        let readback = readback_buffer(device, TARGET_BYTES as u64);
        let queue = device
            .queue(QueueKind::Graphics)
            .expect("the graphics queue exists");
        let mut encoder = device.create_command_encoder(&CommandEncoderDesc {
            label: Some("crcbl-mtl load/clear"),
            queue,
        });
        for (colour, load) in [(first, LoadOp::Clear), (second, load)] {
            encoder.begin_render_pass(&RenderPassDesc {
                label: None,
                color_attachments: &[ColorAttachment {
                    view,
                    resolve: None,
                    load,
                    store: StoreOp::Store,
                    clear: ClearValue::color(colour),
                }],
                depth_stencil_attachment: None,
                render_area: Rect2d::from_size(TARGET.width, TARGET.height),
            });
            encoder.end_render_pass();
        }
        encoder.copy_image_to_buffer(&whole_image_copy(image, readback));
        let commands = encoder.finish().expect("the recording is complete");
        device
            .submit(queue, &SubmitInfo::new(&[commands]))
            .expect("the queue accepts it");
        let request = device
            .request_readback(&ReadbackDesc {
                label: None,
                buffer: readback,
                offset: 0,
                size: TARGET_BYTES as u64,
                after: None,
            })
            .expect("a HostReadback buffer, in range");
        let bytes = drain(device, request, TARGET_BYTES);
        device.destroy_readback(request);
        device.destroy_command_buffer(commands);
        device.destroy_image_view(view);
        device.destroy_image(image);
        device.destroy_buffer(readback);
        bytes
    }

    /// **`LoadOp::Load` keeps what `LoadOp::Clear` replaces**, which is the
    /// pair that catches a load action wired to the wrong constant.
    ///
    /// Two runs differing in exactly one field, so neither result can be
    /// explained by anything else. **What turns it red:** mapping
    /// [`LoadOp::Load`] onto `MTLLoadAction::Clear` makes the first assertion
    /// return `OTHER_TEXEL`; mapping [`LoadOp::Clear`] onto
    /// `MTLLoadAction::Load` makes the second return `CLEAR_TEXEL`; collapsing
    /// both onto one action makes the two runs agree, which either assertion
    /// then catches.
    #[test]
    #[ignore = "needs a real Metal device; run tests/run-mtl-e2e.sh"]
    fn a_metal_load_action_preserves_what_clear_replaces() {
        let (_instance, device) = open_device();
        assert_ne!(
            CLEAR_TEXEL, OTHER_TEXEL,
            "the two colours must differ or neither assertion means anything"
        );
        assert_eq!(
            two_passes(&device, CLEAR, LoadOp::Load, OTHER),
            expected(CLEAR_TEXEL),
            "a Load pass must keep the first pass's clear, not apply its own clear value"
        );
        assert_eq!(
            two_passes(&device, CLEAR, LoadOp::Clear, OTHER),
            expected(OTHER_TEXEL),
            "a Clear pass must replace what the first pass left"
        );
    }

    // --- the triangle rung -------------------------------------------------

    #[cfg(feature = "mtl-e2e")]
    /// The image the triangle is drawn into: square, so "the centre" and "a
    /// corner" are far apart, and 64 texels wide so a row is 256 bytes — the
    /// same comfortable stride [`TARGET`] was chosen for.
    const CANVAS: Extent3d = Extent3d::d2(64, 64);

    #[cfg(feature = "mtl-e2e")]
    /// Bytes one full copy of [`CANVAS`] occupies, at four bytes a texel.
    const CANVAS_BYTES: usize = 64 * 64 * 4;

    /// **The triangle's colour, derived exactly as [`CLEAR`] is.**
    ///
    /// [`Format::Rgba8Unorm`] applies no transfer function, so a component
    /// written as `n / 255` is stored as `round((n / 255) × 255)` = `n`. The
    /// four values are mutually distinct, none is zero, and none matches a
    /// channel of [`CLEAR`] — so a channel swizzle, a dropped draw and a clear
    /// that leaked into the triangle are three different failures.
    ///
    /// A fragment shader returns this as a literal rather than interpolating
    /// it, because an interpolated colour would make the expected value at the
    /// centre depend on the exact rasterisation of the triangle's vertices.
    const INK: [f32; 4] = [64.0 / 255.0, 128.0 / 255.0, 192.0 / 255.0, 1.0];

    #[cfg(feature = "mtl-e2e")]
    /// What [`INK`] must read back as, per the derivation above.
    const INK_TEXEL: [u8; 4] = [0x40, 0x80, 0xC0, 0xFF];

    /// A triangle with no resources at all, in Metal Shading Language.
    ///
    /// **This is not the engine's `triangle.slang`, and the difference is one
    /// thing: bindings.** The committed shader pulls its vertices from a
    /// `StructuredBuffer`, which Slang lowers to a `[[buffer(0)]]` argument on
    /// *both* stages — and binding a buffer needs bind groups, which this slice
    /// deliberately still refuses. So the draw below uses a shader that reads
    /// nothing and builds its geometry from `[[vertex_id]]`, which is the
    /// largest triangle this slice can honestly render end to end.
    ///
    /// The engine's own artifact is still exercised, one rung lower down:
    /// `the_engines_own_triangle_artifact_builds_a_real_pipeline` compiles
    /// `msl/triangle.metal` and builds an `MTLRenderPipelineState` from it. What
    /// is missing is the draw, not the compile.
    ///
    /// The three positions are chosen so that **every corner of the image is
    /// outside the triangle and the centre is well inside it**, whichever way up
    /// clip space turns out to be — the shape is symmetric in X and its extreme
    /// point in Y reaches only 0.8, so no corner at `(±1, ±1)` is covered.
    ///
    /// Built with [`INK`] formatted in rather than written out, so the colour
    /// the shader returns and the colour the assertions expect cannot drift
    /// apart.
    fn ink_msl() -> String {
        ink_msl_at(0.0)
    }

    /// [`ink_msl`]'s triangle at an arbitrary clip-space depth.
    ///
    /// `w` stays at 1.0, so `depth` *is* the post-divide depth and a value
    /// outside `0..=1` puts the primitive outside the clip volume in Z and
    /// nowhere else — which is the only thing
    /// [`a_device_says_whether_it_clamps_depth_without_a_depth_attachment`]
    /// varies. The fragment stage still writes nothing but colour: a shader
    /// that declared `[[depth]]` would change what Metal's clip mode applies
    /// to, and that is a different question from this one.
    fn ink_msl_at(depth: f32) -> String {
        format!(
            "#include <metal_stdlib>\n\
             using namespace metal;\n\
             struct VertexOutput {{ float4 position [[position]]; }};\n\
             [[vertex]] VertexOutput vertexMain(uint index [[vertex_id]]) {{\n\
                 const float2 corners[3] = {{ float2(0.0, 0.8), float2(-0.8, -0.8), \
                     float2(0.8, -0.8) }};\n\
                 VertexOutput out;\n\
                 out.position = float4(corners[index], {depth:?}, 1.0);\n\
                 return out;\n\
             }}\n\
             [[fragment]] float4 fragmentMain() {{\n\
                 return float4({r}, {g}, {b}, {a});\n\
             }}\n",
            r = INK[0],
            g = INK[1],
            b = INK[2],
            a = INK[3],
        )
    }

    /// The MSL-only descriptor a Metal caller supplies.
    fn msl_module<'a>(msl: &'a str, label: &'a str) -> ShaderModuleDesc<'a> {
        ShaderModuleDesc {
            label: Some(label),
            spirv: &[],
            wgsl: None,
            msl: Some(msl),
            dxil: &[],
        }
    }

    /// The empty pipeline layout, which is the only one this slice can make.
    fn empty_layout(device: &MetalDevice) -> PipelineLayoutHandle {
        device
            .create_pipeline_layout(&PipelineLayoutDesc {
                label: Some("crcbl-mtl empty layout"),
                bind_group_layouts: &[],
                push_constants: None,
            })
            .expect("a layout naming no bind groups and no push constants")
    }

    #[cfg(feature = "mtl-e2e")]
    /// The texel at `(x, y)` of a [`CANVAS`]-sized readback.
    fn texel_at(bytes: &[u8], x: u32, y: u32) -> [u8; 4] {
        let offset = ((y * CANVAS.width + x) * 4) as usize;
        bytes[offset..offset + 4]
            .try_into()
            .expect("four bytes of an Rgba8Unorm texel")
    }

    /// **The triangle.** A real `MTLRenderPipelineState`, a real
    /// `drawPrimitives:`, and the exact texels read back.
    ///
    /// Every stage the pipeline slice added is on the path: MSL compiled into an
    /// `MTLLibrary`, both entry points resolved by name, a pipeline layout, a
    /// render pipeline descriptor with a colour format, the viewport and scissor
    /// calls, `setRenderPipelineState:` with the rasteriser state replayed
    /// beside it, and `drawPrimitives:` — on top of MTL3's pass, blit,
    /// submission and readback.
    ///
    /// **What turns it red.** Dropping the draw, or binding no pipeline, or
    /// failing the pipeline creation — the centre comes back [`CLEAR_TEXEL`],
    /// which is asserted against explicitly rather than merely differing from
    /// [`INK_TEXEL`]. Rasterising the whole target instead of a triangle — the
    /// four corner assertions. A channel swizzle in
    /// [`conv::color_write_mask`](crate::conv) or the clear colour — both
    /// texels are asymmetric in every channel. Writing only some channels —
    /// the write mask is `ALL` and the alpha assertion is part of the texel
    /// comparison. Never running the copy, or reading before the command buffer
    /// completes — every texel comes back [`POISON`], which is neither of the
    /// two colours the last assertion admits.
    /// **Needs a device that executes a shader.** Feature-gated *and*
    /// `#[ignore]`d, the shape `crcbl-vk` and `crcbl-wgpu` already use for the
    /// same reason: `--all-features` on a machine that cannot run it must stay
    /// green, and `tests/run-mtl-e2e.sh` is the only thing that turns it on —
    /// and that script fails when the suite reports zero tests run, because
    /// `docs/plan/12-testing.md` calls a silently-skipped e2e job a known trap.
    ///
    /// **That gate used to say CI could never satisfy it, and that was wrong.**
    /// A paravirtual device was long assumed to execute nothing at all, which
    /// was generalised from macos-14 — the one hosted image whose
    /// `MTLCreateSystemDefaultDevice()` returns nil. macos-15 and macos-26 both
    /// run a standalone Swift compute dispatch and triangle draw correctly, and
    /// `macos-latest` resolves to macos-26, so the script has a CI job now.
    /// `docs/backlog.md` carries the per-image measurements.
    ///
    /// **This test still faults there**, with
    /// `kIOGPUCommandBufferCallbackErrorHang`, every encoder reported
    /// `completed` and none faulted — one of four draws that do, which
    /// `.github/workflows/ci.yml` holds out by name so the rest of the suite
    /// stays a gate. The probe's own draw succeeding on that same image is what
    /// makes this a defect in this backend's command stream rather than a
    /// property of the device, and
    /// [`a_triangle_draw_into_a_bgra_target_paints_the_same_image`] is the
    /// controlled experiment that narrows which part of it.
    ///
    /// What a person on a real Mac still adds is an unvirtualised GPU: Metal
    /// has no lavapipe to cross-check against, and a paravirtual device is one
    /// implementation with one set of tolerances.
    #[cfg(feature = "mtl-e2e")]
    #[test]
    #[ignore = "executes a shader on a real Metal device; run tests/run-mtl-e2e.sh"]
    fn a_metal_triangle_draw_paints_the_centre_and_leaves_the_corners_clear() {
        let (_instance, device) = open_device();
        assert_ne!(
            INK_TEXEL, CLEAR_TEXEL,
            "the two colours must differ or no assertion below means anything"
        );

        let ink = ink_msl();
        let module = device
            .create_shader_module(&msl_module(&ink, "crcbl-mtl ink.metal"))
            .expect("a shader with no bindings compiles");
        let layout = empty_layout(&device);
        let targets = [ColorTargetState::opaque(Format::Rgba8Unorm)];
        let pipeline = device
            .create_graphics_pipeline(&GraphicsPipelineDesc {
                label: Some("crcbl-mtl triangle"),
                layout,
                vertex: ShaderEntry {
                    module,
                    entry_point: "vertexMain",
                },
                fragment: Some(ShaderEntry {
                    module,
                    entry_point: "fragmentMain",
                }),
                primitive: PrimitiveState::default(),
                depth_stencil: None,
                multisample: MultisampleState::default(),
                color_targets: &targets,
            })
            .expect("a colour-only pipeline over an Rgba8Unorm target");

        let bytes = draw_canvas(&device, Format::Rgba8Unorm, |encoder| {
            encoder.bind_graphics_pipeline(pipeline);
            encoder.draw(0..3, 0..1);
        });
        assert_ink_triangle(&bytes, Format::Rgba8Unorm);

        device.destroy_graphics_pipeline(pipeline);
        device.destroy_pipeline_layout(layout);
        device.destroy_shader_module(module);
    }

    /// **The same triangle, into a [`Format::Bgra8Unorm`] target.** Worth having
    /// on its own — `CAMetalLayer` refuses RGBA8, so BGRA8 is the format a real
    /// Metal application actually renders to — and it began as a controlled
    /// experiment whose answer is recorded below.
    ///
    /// # The experiment, and what it settled
    ///
    /// [`a_metal_triangle_draw_paints_the_centre_and_leaves_the_corners_clear`] faults
    /// on the CI runner (`macos-latest`, an Apple Paravirtual device) with
    /// `kIOGPUCommandBufferCallbackErrorHang`, every encoder reported
    /// `completed` and none faulted — one of four draws that do, which
    /// `.github/workflows/ci.yml` holds out by name. A standalone Swift script
    /// drawing a triangle into a `Bgra8Unorm` texture on that same image
    /// produced a correct image, so the fault is in this backend's command
    /// stream rather than in the device. The two streams differed in more than
    /// one way; `docs/backlog.md` lists the candidates.
    ///
    /// This test isolated one of them. **The render-target format is the only
    /// difference between it and its twin above** — same MSL from [`ink_msl`],
    /// same [`CANVAS`], same [`CLEAR`], same [`draw_canvas`] helper and so the
    /// same command-buffer construction, same `drawPrimitives:`, same assertions
    /// through [`assert_ink_triangle`] with the channel order the format
    /// implies.
    ///
    /// **It faults too**, byte-identically: run 31080128007 reported
    /// `kIOGPUCommandBufferCallbackErrorHang`, `canvas` and `crcbl copies` both
    /// `completed`, neither faulted. So the render-target format is **ruled
    /// out**, and the remaining candidate is the command buffer itself: every
    /// one in this backend comes from
    /// [`command_buffer`](crate::fault::command_buffer), which sets
    /// `MTLCommandBufferErrorOption::EncoderExecutionStatus`, where the Swift
    /// probe used a plain `makeCommandBuffer()`. That is the next thing to test.
    /// This test is quarantined alongside the other four until it is.
    ///
    /// **What turns it red for ordinary reasons**, as for its twin: a dropped
    /// draw or a failed pipeline leaves the centre at [`CLEAR_TEXEL`], a
    /// full-target rasterisation trips the corner assertions, and a channel
    /// swizzle trips the byte comparison — which on this format is a stricter
    /// check than on RGBA8, because the expected bytes are the reordered ones.
    ///
    /// **Needs a device that executes a shader**, like every other gated draw.
    #[cfg(feature = "mtl-e2e")]
    #[test]
    #[ignore = "executes a shader on a real Metal device; run tests/run-mtl-e2e.sh"]
    fn a_triangle_draw_into_a_bgra_target_paints_the_same_image() {
        const FORMAT: Format = Format::Bgra8Unorm;

        let (_instance, device) = open_device();
        assert_ne!(
            INK_TEXEL, CLEAR_TEXEL,
            "the two colours must differ or no assertion below means anything"
        );
        assert_ne!(
            texel_in(FORMAT, INK_TEXEL),
            INK_TEXEL,
            "a BGRA readback must expect different bytes from an RGBA one, or this test is not \
             checking the channel order it claims to"
        );

        let ink = ink_msl();
        let module = device
            .create_shader_module(&msl_module(&ink, "crcbl-mtl ink.metal"))
            .expect("a shader with no bindings compiles");
        let layout = empty_layout(&device);
        let targets = [ColorTargetState::opaque(FORMAT)];
        let pipeline = device
            .create_graphics_pipeline(&GraphicsPipelineDesc {
                label: Some("crcbl-mtl bgra triangle"),
                layout,
                vertex: ShaderEntry {
                    module,
                    entry_point: "vertexMain",
                },
                fragment: Some(ShaderEntry {
                    module,
                    entry_point: "fragmentMain",
                }),
                primitive: PrimitiveState::default(),
                depth_stencil: None,
                multisample: MultisampleState::default(),
                color_targets: &targets,
            })
            .expect("a colour-only pipeline over a Bgra8Unorm target");

        let bytes = draw_canvas(&device, FORMAT, |encoder| {
            encoder.bind_graphics_pipeline(pipeline);
            encoder.draw(0..3, 0..1);
        });
        assert_ink_triangle(&bytes, FORMAT);

        device.destroy_graphics_pipeline(pipeline);
        device.destroy_pipeline_layout(layout);
        device.destroy_shader_module(module);
    }

    /// **A device that reports [`Features::DEPTH_CLAMP`] has to clamp, and one
    /// that does not has to be refused.** Half measurement and half assertion,
    /// because the measurement this started as has been made and its answer is
    /// now `crate::quirk`.
    ///
    /// # The question it settled
    ///
    /// `crates/crcbl/tests/hal_seam_e2e.rs`'s `exercise_depth_clamp` draws a
    /// triangle past the far plane at `w = 1` with
    /// [`PrimitiveState::depth_clamp`] set, into a pass carrying **no depth
    /// attachment at all**, so nothing but the clipper can discard it: drawn
    /// means the clamp replaced the clip, background means the primitive was
    /// clipped anyway. It is drawn on Vulkan, on WebGPU and on D3D12's WARP.
    /// It was *not* drawn on CI's `Apple Paravirtual device`, and the parity
    /// report there read "declares it supported, accepted the call and changed
    /// nothing".
    ///
    /// Two explanations survived reading the code, and they needed opposite
    /// fixes: either the device ignores `setDepthClipMode:`, or Metal's clip
    /// mode needs a depth attachment to apply — Metal describes `depthClipMode`
    /// in terms of *fragments* where Vulkan defines a *primitive* clip, and
    /// Dawn's Metal backend only ever exercises it against a pass that has one.
    /// A third, that something in this crate defeats the call, was ruled out by
    /// reading: `setDepthClipMode:` has exactly one call site
    /// (`crate::command`'s `bind_graphics_pipeline`), nothing between it and
    /// `drawPrimitives:` touches encoder state, and `RasterState::clip` has one
    /// producer.
    ///
    /// **The two printed lines separate them, because the passes behind them
    /// differ in the depth attachment and in nothing else** — same MSL but for
    /// its Z literal, same [`CANVAS`], same clear, same submission, same
    /// readback. On that runner both came back `false` with every control
    /// passing, which is the first explanation and kills the second.
    ///
    /// # So what it does now, and it is different on the two kinds of device
    ///
    /// * **The flag is reported** — every Metal GPU that is not the one above.
    ///   The two lines are printed exactly as they were, and a `false` on
    ///   either is a second device needing the same treatment rather than a
    ///   settled question re-opening.
    /// * **The flag is withheld** — the paravirtual runner, by
    ///   `crate::quirk`. Then both clamped pipelines must come back
    ///   [`HalError::Unsupported`], and that assertion is the half of the parity
    ///   contract the withholding would otherwise break: a backend that takes
    ///   the flag off and encodes the clip mode anyway has moved the untruth
    ///   from the adapter to the encoder rather than fixed it.
    ///
    /// # What it fails on
    ///
    /// A printed measurement whose fixture is broken is a green light wired to
    /// nothing, so both halves of the comparison are backed by controls that
    /// assert, once with a depth attachment and once without. They ask for no
    /// clamp at all, so they run on both kinds of device:
    ///
    /// * a triangle **inside** the clip volume must be drawn, or the pass, the
    ///   pipeline or the readback is what is being measured rather than the
    ///   clip mode;
    /// * the same triangle past the far plane under `MTLDepthClipMode::Clip`
    ///   must **not** be drawn, or this device clips nothing and "it was
    ///   clipped" is not a reading either printed line could report.
    ///
    /// A centre texel that is neither colour fails on the spot rather than
    /// being folded into "not drawn".
    ///
    /// The controls run **after** the measurement is printed, on purpose:
    /// a device that cannot render the depth-attached control at all is a
    /// finding of its own, and it should not also cost this run the two lines it
    /// came for.
    ///
    /// nextest captures a passing test's stdout, so read this with
    /// `--success-output immediate`, which is what `tests/run-mtl-e2e.sh`
    /// passes.
    #[cfg(feature = "mtl-e2e")]
    #[test]
    #[ignore = "executes a shader on a real Metal device; run tests/run-mtl-e2e.sh"]
    fn a_device_says_whether_it_clamps_depth_without_a_depth_attachment() {
        /// Past the far plane at `w = 1`, the depth `hal_seam_e2e.rs` draws its
        /// own clamp triangle at.
        const BEYOND_FAR: f32 = 1.5;
        /// A depth inside the clip volume, for the control that must be drawn.
        const INSIDE: f32 = 0.5;

        let (_instance, device) = open_device();

        // Whether the centre of the canvas came back the triangle's colour,
        // for one triangle at `depth` under one clip mode and one attachment
        // set. Everything else about the draw is held fixed.
        //
        // The pipeline's refusal is handed back rather than unwrapped, because
        // on a device `crate::quirk` withheld the flag from it is the answer
        // this test came for and not a failure to build a fixture.
        let drew =
            |depth: f32, clamp: bool, attachment: Option<Format>| -> Result<bool, HalError> {
                let msl = ink_msl_at(depth);
                let module = device
                    .create_shader_module(&msl_module(&msl, "crcbl-mtl depth clip.metal"))
                    .expect("a shader with no bindings compiles");
                let layout = empty_layout(&device);
                let targets = [ColorTargetState::opaque(Format::Rgba8Unorm)];
                let built = device.create_graphics_pipeline(&GraphicsPipelineDesc {
                    label: Some("crcbl-mtl depth clip"),
                    layout,
                    vertex: ShaderEntry {
                        module,
                        entry_point: "vertexMain",
                    },
                    fragment: Some(ShaderEntry {
                        module,
                        entry_point: "fragmentMain",
                    }),
                    primitive: PrimitiveState {
                        depth_clamp: clamp,
                        ..PrimitiveState::default()
                    },
                    // Always, and no writes: the depth attachment is here to
                    // exist, not to test anything, so it cannot become the
                    // reason a fragment vanished.
                    depth_stencil: attachment.map(|format| DepthStencilState {
                        format,
                        depth_write: false,
                        depth_compare: CompareOp::Always,
                        stencil: None,
                        bias: DepthBias::default(),
                    }),
                    multisample: MultisampleState::default(),
                    color_targets: &targets,
                });
                let pipeline = match built {
                    Ok(pipeline) => pipeline,
                    Err(error) => {
                        device.destroy_pipeline_layout(layout);
                        device.destroy_shader_module(module);
                        return Err(error);
                    }
                };

                let bytes = draw_canvas_over(&device, Format::Rgba8Unorm, attachment, |encoder| {
                    encoder.bind_graphics_pipeline(pipeline);
                    encoder.draw(0..3, 0..1);
                });

                device.destroy_graphics_pipeline(pipeline);
                device.destroy_pipeline_layout(layout);
                device.destroy_shader_module(module);

                let centre = texel_at(&bytes, CANVAS.width / 2, CANVAS.height / 2);
                assert!(
                    centre == INK_TEXEL || centre == CLEAR_TEXEL,
                    "the centre of the canvas is {centre:02X?}, which is neither the triangle's \
                 colour nor the clear's, so this pass did not run the way the controls assume"
                );
                Ok(centre == INK_TEXEL)
            };

        // The measurement runs and is printed **before** the controls, so a
        // control that fails on this device leaves the two numbers on the log
        // rather than taking them down with it.
        if device.caps().features.contains(Features::DEPTH_CLAMP) {
            let colour_only = drew(BEYOND_FAR, true, None)
                .expect("a device reporting DEPTH_CLAMP builds a clamped pipeline");
            let with_depth = drew(BEYOND_FAR, true, Some(Format::D32Float))
                .expect("a device reporting DEPTH_CLAMP builds a clamped pipeline");
            println!(
                "crcbl-mtl depth clip: Clamp at z={BEYOND_FAR}, no depth attachment, drew = \
                 {colour_only}"
            );
            println!(
                "crcbl-mtl depth clip: Clamp at z={BEYOND_FAR}, D32Float depth attachment, drew = \
                 {with_depth}"
            );
            println!(
                "crcbl-mtl depth clip: this device {}",
                match (colour_only, with_depth) {
                    (true, true) => "honours MTLDepthClipMode::Clamp",
                    (false, true) =>
                        "honours MTLDepthClipMode::Clamp only when the pass has a \
                                      depth attachment",
                    (false, false) => "ignores MTLDepthClipMode::Clamp entirely",
                    (true, false) =>
                        "clamped without an attachment and not with one, which neither \
                                      explanation predicts",
                }
            );
        } else {
            // `crate::quirk` took the flag off, so the backend owes a refusal
            // rather than a clip mode — the half that makes the withholding
            // honest instead of merely quieter.
            for attachment in [None, Some(Format::D32Float)] {
                let error = drew(BEYOND_FAR, true, attachment)
                    .expect_err("a device without DEPTH_CLAMP must refuse a clamped pipeline");
                assert!(
                    matches!(
                        error,
                        HalError::Unsupported { backend, .. } if backend == BackendKind::Metal
                    ),
                    "the clamped pipeline was refused with {error} and attachment \
                     {attachment:?}, which is not the HalError::Unsupported a caller branches on \
                     to take the fallback"
                );
            }
            println!(
                "crcbl-mtl depth clip: this device reports no DEPTH_CLAMP — crcbl_mtl::quirk \
                 withholds it — and refused both clamped pipelines"
            );
        }

        for attachment in [None, Some(Format::D32Float)] {
            assert!(
                drew(INSIDE, false, attachment)
                    .expect("a pipeline that asks for no clamp needs no feature"),
                "a triangle at z={INSIDE} was not drawn with attachment {attachment:?}, so this \
                 fixture reports nothing about any clip mode"
            );
            assert!(
                !drew(BEYOND_FAR, false, attachment)
                    .expect("a pipeline that asks for no clamp needs no feature"),
                "a triangle at z={BEYOND_FAR} survived MTLDepthClipMode::Clip with attachment \
                 {attachment:?}, so this device clips nothing and \"it was clipped\" is not a \
                 reading the lines above could report"
            );
        }
    }

    /// **The engine's own `triangle.slang` artifact, compiled by a real Metal
    /// compiler, built into a real pipeline, and recorded into a real draw.**
    ///
    /// MTL4 could only do the first two, because the shader pulls its geometry
    /// from a `StructuredBuffer` that Slang lowers to `[[buffer(0)]]` on both
    /// stages and nothing could bind a buffer. The binding slice closes that,
    /// and this test now records the whole draw — pipeline, bind group,
    /// `draw(0..3, 0..1)` — and lets `finish` report it, which is the half a
    /// machine with no working GPU can still check. **It deliberately does not
    /// submit**; `the_engines_own_triangle_draws_through_a_bind_group` is the
    /// gated test that runs the shader and asserts the pixels.
    ///
    /// **What turns it red.** A truncated or non-MSL artifact — the module
    /// creation fails. Slang mangling a name across targets, or the manifest
    /// recording a name the MSL does not define — `newFunctionWithName:`
    /// returns nil and pipeline creation reports which name was missing. Any
    /// refusal or descriptor error anywhere in the bind-group chain — every
    /// recording failure surfaces at `finish`, which is `expect`ed.
    #[test]
    #[ignore = "needs a real Metal device; run tests/run-mtl-e2e.sh"]
    fn the_engines_own_triangle_artifact_builds_a_real_pipeline() {
        use crcbl_shaders::{Stage as ShaderStage, TRIANGLE};

        let (_instance, device) = open_device();
        let msl = TRIANGLE.msl().expect("the triangle ships an MSL artifact");
        let module = device
            .create_shader_module(&msl_module(msl, "triangle.slang"))
            .expect("the committed MSL compiles on a real Metal compiler");

        let vertex = TRIANGLE
            .entry_point(ShaderStage::Vertex)
            .expect("one vertex entry point");
        let fragment = TRIANGLE
            .entry_point(ShaderStage::Fragment)
            .expect("one fragment entry point");
        let layout = empty_layout(&device);
        // `Rgba16Float` because that is the target `triangle.slang`'s own
        // comments say its `color.a` is carried for.
        let targets = [ColorTargetState::opaque(Format::Rgba16Float)];
        let pipeline = device
            .create_graphics_pipeline(&GraphicsPipelineDesc {
                label: Some("triangle.slang"),
                layout,
                vertex: ShaderEntry {
                    module,
                    entry_point: vertex,
                },
                fragment: Some(ShaderEntry {
                    module,
                    entry_point: fragment,
                }),
                primitive: PrimitiveState::default(),
                depth_stencil: None,
                multisample: MultisampleState::default(),
                color_targets: &targets,
            })
            .unwrap_or_else(|error| {
                panic!("the engine's own triangle does not build a Metal pipeline: {error}")
            });

        // A name the artifact does not define is reported as the missing
        // function it is, not as a nil reaching the descriptor.
        let error = device
            .create_graphics_pipeline(&GraphicsPipelineDesc {
                label: Some("triangle.slang, misnamed"),
                layout,
                vertex: ShaderEntry {
                    module,
                    entry_point: "crcbl_no_such_entry_point",
                },
                fragment: None,
                primitive: PrimitiveState::default(),
                depth_stencil: None,
                multisample: MultisampleState::default(),
                color_targets: &[],
            })
            .expect_err("the library has no such function");
        let HalError::ShaderCompilation(text) = error else {
            panic!("a missing entry point is a shader-compilation failure, got {error:?}");
        };
        assert!(text.contains("crcbl_no_such_entry_point"), "{text}");

        // And the draw the shader actually needs: one set, one read-only
        // storage buffer, visible to both stages because Slang emitted the
        // argument on both. Recorded and finished rather than submitted —
        // this test is behind neither the feature nor the ignore, so it runs
        // on every machine that can open a device at all, including one whose
        // GPU executes nothing. `the_engines_own_triangle_draws_through_a_bind_group`
        // is the gated test that submits, and `finish` is where every
        // recording refusal lands anyway.
        let set = device
            .create_bind_group_layout(&BindGroupLayoutDesc {
                label: Some("triangle.slang set 0"),
                entries: &[crcbl_hal::BindGroupLayoutEntry {
                    binding: 0,
                    visibility: crcbl_hal::ShaderStages::VERTEX | crcbl_hal::ShaderStages::FRAGMENT,
                    kind: crcbl_hal::BindingKind::StorageBuffer {
                        read_only: true,
                        dynamic: false,
                    },
                    count: 1,
                    flags: crcbl_hal::BindingFlags::empty(),
                }],
            })
            .expect("one read-only storage buffer, which is what the shader declares");
        let bound_layout = device
            .create_pipeline_layout(&PipelineLayoutDesc {
                label: Some("triangle.slang"),
                bind_group_layouts: &[set],
                push_constants: None,
            })
            .expect("one set");
        let geometry = device
            .create_buffer(&buffer(96, MemoryLocation::HostUpload))
            .expect("three 32-byte vertices");
        let group = device
            .create_bind_group(&BindGroupDesc {
                label: Some("triangle.slang vertices"),
                layout: set,
                entries: &[BindGroupEntry {
                    binding: 0,
                    array_index: 0,
                    resource: crcbl_hal::BindingResource::whole_buffer(geometry),
                }],
                variable_count: None,
            })
            .expect("the geometry buffer in binding 0");
        // `Rgba8Unorm` here rather than the `Rgba16Float` above, because this
        // pipeline is recorded into a pass whose attachment is the one
        // `color_target` makes, and Metal requires the two to agree.
        let targets = [ColorTargetState::opaque(Format::Rgba8Unorm)];
        let bound_pipeline = device
            .create_graphics_pipeline(&GraphicsPipelineDesc {
                label: Some("triangle.slang, bound"),
                layout: bound_layout,
                vertex: ShaderEntry {
                    module,
                    entry_point: vertex,
                },
                fragment: Some(ShaderEntry {
                    module,
                    entry_point: fragment,
                }),
                primitive: PrimitiveState::default(),
                depth_stencil: None,
                multisample: MultisampleState::default(),
                color_targets: &targets,
            })
            .expect("the same shader over a layout that names its buffer");

        let (image, view) = color_target(&device);
        let queue = device
            .queue(QueueKind::Graphics)
            .expect("the graphics queue exists");
        let mut encoder = device.create_command_encoder(&CommandEncoderDesc {
            label: Some("triangle.slang"),
            queue,
        });
        encoder.begin_render_pass(&RenderPassDesc {
            label: Some("triangle.slang"),
            color_attachments: &[ColorAttachment {
                view,
                resolve: None,
                load: LoadOp::Clear,
                store: StoreOp::Store,
                clear: ClearValue::color(CLEAR),
            }],
            depth_stencil_attachment: None,
            render_area: Rect2d::from_size(TARGET.width, TARGET.height),
        });
        encoder.bind_graphics_pipeline(bound_pipeline);
        encoder.bind_group(0, group, &[], bound_layout);
        encoder.draw(0..3, 0..1);
        encoder.end_render_pass();
        let commands = encoder
            .finish()
            .expect("the engine's own triangle records a complete draw");
        device.destroy_command_buffer(commands);

        device.destroy_image_view(view);
        device.destroy_image(image);
        device.destroy_graphics_pipeline(bound_pipeline);
        device.destroy_bind_group(group);
        device.destroy_buffer(geometry);
        device.destroy_pipeline_layout(bound_layout);
        device.destroy_bind_group_layout(set);
        device.destroy_graphics_pipeline(pipeline);
        device.destroy_pipeline_layout(layout);
        device.destroy_shader_module(module);
    }

    /// A shader module compiles, and a broken one comes back with **Metal's own
    /// diagnostic**.
    ///
    /// The broken source names an identifier that exists nowhere, so the
    /// compiler's message must quote it — "use of undeclared identifier
    /// `crcbl_not_a_symbol`". Asserting on that substring is what distinguishes
    /// "the error text is Metal's" from "the error text is ours": no wording
    /// this crate could invent contains a token that only appears inside the
    /// caller's source.
    ///
    /// **What turns it red.** Discarding the `NSError` and returning a fixed
    /// message — the substring assertion. Returning `Ok` for a source that does
    /// not compile — the `expect_err`. Reporting it as anything other than a
    /// compilation failure — the `let else`.
    #[test]
    #[ignore = "needs a real Metal device; run tests/run-mtl-e2e.sh"]
    fn a_broken_shader_module_carries_metals_own_message() {
        let (_instance, device) = open_device();

        let good = device
            .create_shader_module(&msl_module(&ink_msl(), "good.metal"))
            .expect("valid MSL compiles");
        device.destroy_shader_module(good);

        let broken = "[[fragment]] float4 fragmentMain() { return crcbl_not_a_symbol; }";
        let error = device
            .create_shader_module(&msl_module(broken, "broken.metal"))
            .expect_err("that identifier is declared nowhere");
        let HalError::ShaderCompilation(text) = error else {
            panic!("a source Metal rejects is a shader-compilation failure, got {error:?}");
        };
        assert!(
            text.contains("broken.metal"),
            "the message must name the module: {text}"
        );
        assert!(
            text.contains("crcbl_not_a_symbol"),
            "the message must be Metal's own, which quotes the undeclared identifier: {text}"
        );

        // And a descriptor with no MSL at all names the gap rather than being
        // reported as a compiler error — nothing was compiled.
        let error = device
            .create_shader_module(&ShaderModuleDesc {
                label: Some("spirv-only.slang"),
                spirv: crcbl_shaders::TRIANGLE.spirv(),
                wgsl: crcbl_shaders::TRIANGLE.wgsl(),
                msl: None,
                dxil: &[],
            })
            .expect_err("this backend compiles MSL and nothing else");
        let HalError::ShaderCompilation(text) = error else {
            panic!("an unusable descriptor is a shader-compilation failure, got {error:?}");
        };
        assert!(text.contains("only compile MSL"), "{text}");
        assert!(text.contains("SPIR-V and WGSL"), "{text}");
    }

    /// Pipelines and their layouts create, destroy, and stop resolving — and a
    /// compute pipeline is created from a real kernel rather than assumed.
    ///
    /// **What turns it red.** Never inserting into the pool — the creations
    /// fail. Not removing on destroy, or removing without the owner check — the
    /// stale-handle assertions, which demand `InvalidHandle` specifically:
    /// "it returned an error" would pass against a pipeline layout that was
    /// never valid in the first place.
    #[test]
    #[ignore = "needs a real Metal device; run tests/run-mtl-e2e.sh"]
    fn pipelines_create_and_then_stop_resolving() {
        let (_instance, device) = open_device();
        let layout = empty_layout(&device);

        let compute_msl = "\
#include <metal_stdlib>\n\
using namespace metal;\n\
[[kernel]] void computeMain(uint3 id [[thread_position_in_grid]]) {}\n";
        let compute_module = device
            .create_shader_module(&msl_module(compute_msl, "crcbl-mtl kernel.metal"))
            .expect("a kernel compiles");
        assert!(
            device.caps().features.contains(Features::COMPUTE),
            "this backend reports COMPUTE because this call is the one that backs it"
        );
        let compute = device
            .create_compute_pipeline(&ComputePipelineDesc {
                label: Some("crcbl-mtl compute"),
                layout,
                compute: ShaderEntry {
                    module: compute_module,
                    entry_point: "computeMain",
                },
                // The kernel above declares nothing about thread counts —
                // MSL cannot — so this is what the dispatch would launch.
                workgroup_size: [1, 1, 1],
            })
            .expect("a kernel with no bindings builds a compute pipeline");

        let ink = ink_msl();
        let module = device
            .create_shader_module(&msl_module(&ink, "crcbl-mtl ink.metal"))
            .expect("valid MSL compiles");
        let targets = [ColorTargetState::opaque(Format::Rgba8Unorm)];
        let graphics = device
            .create_graphics_pipeline(&GraphicsPipelineDesc {
                label: Some("crcbl-mtl graphics"),
                layout,
                vertex: ShaderEntry {
                    module,
                    entry_point: "vertexMain",
                },
                fragment: Some(ShaderEntry {
                    module,
                    entry_point: "fragmentMain",
                }),
                primitive: PrimitiveState::default(),
                depth_stencil: None,
                multisample: MultisampleState::default(),
                color_targets: &targets,
            })
            .expect("a colour-only pipeline");

        device.destroy_compute_pipeline(compute);
        device.destroy_graphics_pipeline(graphics);
        device.destroy_shader_module(module);
        device.destroy_shader_module(compute_module);
        device.destroy_pipeline_layout(layout);

        // A destroyed module cannot be named by a pipeline, and a destroyed
        // layout cannot back one.
        let error = device
            .create_graphics_pipeline(&GraphicsPipelineDesc {
                label: None,
                layout,
                vertex: ShaderEntry {
                    module,
                    entry_point: "vertexMain",
                },
                fragment: None,
                primitive: PrimitiveState::default(),
                depth_stencil: None,
                multisample: MultisampleState::default(),
                color_targets: &[],
            })
            .expect_err("both the layout and the module were destroyed");
        assert!(
            matches!(error, HalError::InvalidHandle { kind, .. } if kind == "pipeline layout"),
            "{error:?}"
        );
    }

    /// **A pipeline that declares no depth/stencil state binds the device's
    /// default object — never nil.**
    ///
    /// `setDepthStencilState:nil` hangs Apple's paravirtual GPU, which is what
    /// made every draw this backend recorded fault on GitHub's macOS runner
    /// while a render-pass clear passed. `crcbl_mtl::pipeline`'s
    /// [`default_depth_stencil_state`](crate::pipeline::default_depth_stencil_state)
    /// carries the bisect that named the call and the argument that the
    /// substitution cannot change an image.
    ///
    /// **What turns it red.** Handing Metal nil again — the shape the seam used
    /// to carry, an `Option` that was `None` for every pipeline whose pass has
    /// no depth attachment. And, from the other side, substituting the default
    /// for a pipeline that *did* declare depth state, which would silently
    /// disable a depth test the caller asked for: the second half asserts that a
    /// declared state resolves to its own object.
    ///
    /// **This needs a Metal device and not a working one.** It creates state
    /// objects and reads them back through the same resolve the encoder uses;
    /// nothing is submitted and no shader runs, so it is `#[ignore]`d — which
    /// is what every test needing a device carries — but stays outside
    /// `mtl-e2e`, the gate for tests that make the GPU execute a shader. It
    /// runs on the paravirtual device that faults on a draw.
    #[test]
    #[ignore = "needs a real Metal device; run tests/run-mtl-e2e.sh"]
    fn a_pipeline_without_depth_state_binds_the_devices_default_rather_than_nil() {
        let (_instance, device) = open_device();
        let layout = empty_layout(&device);
        let module = device
            .create_shader_module(&msl_module(&ink_msl(), "crcbl-mtl ink.metal"))
            .expect("valid MSL compiles");
        let targets = [ColorTargetState::opaque(Format::Rgba8Unorm)];
        let desc = GraphicsPipelineDesc {
            label: Some("crcbl-mtl no depth state"),
            layout,
            vertex: ShaderEntry {
                module,
                entry_point: "vertexMain",
            },
            fragment: Some(ShaderEntry {
                module,
                entry_point: "fragmentMain",
            }),
            primitive: PrimitiveState::default(),
            depth_stencil: None,
            multisample: MultisampleState::default(),
            color_targets: &targets,
        };
        let colour_only = device
            .create_graphics_pipeline(&desc)
            .expect("a colour-only pipeline");
        let with_depth = device
            .create_graphics_pipeline(&GraphicsPipelineDesc {
                label: Some("crcbl-mtl reversed-Z depth state"),
                depth_stencil: Some(DepthStencilState::default()),
                ..desc
            })
            .expect("a pipeline declaring the seam's default reversed-Z depth state");

        let default = &device.inner.default_depth_stencil;
        let bound = device
            .inner
            .graphics_pipeline_raw(colour_only)
            .expect("the pipeline is live and this device's");
        assert!(
            std::ptr::eq(
                Retained::as_ptr(&bound.depth_stencil),
                Retained::as_ptr(default)
            ),
            "a pipeline with no depth/stencil state must bind the device's shared always-pass \
             object, because nil is what hangs the paravirtual GPU"
        );

        let bound = device
            .inner
            .graphics_pipeline_raw(with_depth)
            .expect("the pipeline is live and this device's");
        assert!(
            !std::ptr::eq(
                Retained::as_ptr(&bound.depth_stencil),
                Retained::as_ptr(default)
            ),
            "a pipeline that declares depth state must bind that state, not the always-pass \
             default that would drop its depth test"
        );

        device.destroy_graphics_pipeline(with_depth);
        device.destroy_graphics_pipeline(colour_only);
        device.destroy_shader_module(module);
        device.destroy_pipeline_layout(layout);
    }

    /// A pipeline layout is the empty one, one carrying a push-constant range, or
    /// a refusal that names the caller's own mistake.
    ///
    /// The push-constant half used to be the refusal: no range of any size was
    /// accepted, because nothing here knew which argument-table index the
    /// committed MSL put a block at. `crcbl_mtl::argument` derives that index,
    /// so the range is a *placement* now — which is what makes the
    /// budget assertions below reachable at all.
    ///
    /// **What turns it red.** Refusing a range this device's own
    /// `max_push_constant_size` says fits — the flag and the budget are reported
    /// together, so a layout asking for exactly the budget must be one this
    /// backend builds or the number is wrong. Accepting one four bytes past it.
    /// Accepting a non-empty bind-group list of a handle nobody issued — nothing
    /// could satisfy it. Collapsing the last two onto one error: "you asked for
    /// more than I said I had" and "you handed me a dead handle" send a caller to
    /// different places.
    #[test]
    #[ignore = "needs a real Metal device; run tests/run-mtl-e2e.sh"]
    fn a_pipeline_layout_is_empty_or_refused_by_cause() {
        let (_instance, device) = open_device();
        let budget = device.caps().limits.max_push_constant_size;
        assert!(
            device.caps().features.contains(Features::PUSH_CONSTANTS) && budget > 0,
            "setBytes:length:atIndex: is on every Metal encoder, so every device reports the flag \
             and a budget with it"
        );

        let layout = empty_layout(&device);
        device.destroy_pipeline_layout(layout);

        let range = |size| crcbl_hal::PushConstantRange {
            stages: crcbl_hal::ShaderStages::COMPUTE,
            offset: 0,
            size,
        };
        // The whole reported budget, which must be a block this backend places
        // or the figure the adapter reports is not one a caller can spend.
        let layout = device
            .create_pipeline_layout(&PipelineLayoutDesc {
                label: Some("the whole inlined-buffer budget"),
                bind_group_layouts: &[],
                push_constants: Some(range(budget)),
            })
            .expect("max_push_constant_size must be a range this backend accepts");
        device.destroy_pipeline_layout(layout);

        let error = device
            .create_pipeline_layout(&PipelineLayoutDesc {
                label: Some("past the budget"),
                bind_group_layouts: &[],
                push_constants: Some(range(budget + 4)),
            })
            .expect_err("four bytes more than setBytes: takes");
        let HalError::InvalidDescriptor(text) = &error else {
            panic!("a range past the reported budget is not {error:?}");
        };
        assert!(text.contains("max_push_constant_size"), "{text}");

        // Until the binding slice this was `Unsupported`, because no bind group
        // layout could exist at all. Now that they are real, a hand-made handle
        // is a *stale* handle and must be told apart from a backend that cannot
        // do the thing — which is the stronger claim, and the one the seam asks
        // for. Reporting `Unsupported` here now would hide a caller's bug behind
        // a "not yet".
        let error = device
            .create_pipeline_layout(&PipelineLayoutDesc {
                label: Some("with groups"),
                bind_group_layouts: &[Handle::from_bits(1 << 32).expect("generation 1")],
                push_constants: None,
            })
            .expect_err("that handle was never issued by this device");
        assert!(
            matches!(&error, HalError::InvalidHandle { kind, .. } if *kind == "bind group layout"),
            "{error:?}"
        );
    }

    /// A buffer-to-buffer copy moves the bytes, at the offsets it was given.
    ///
    /// **What turns it red.** Dropping the copy, or swapping source and
    /// destination — the poison survives. Ignoring either offset — the moved
    /// window lands in the wrong place, which the untouched head and tail
    /// assertions catch. Ignoring the size — the tail is overwritten.
    #[test]
    #[ignore = "needs a real Metal device; run tests/run-mtl-e2e.sh"]
    fn a_metal_buffer_to_buffer_copy_moves_the_bytes_at_both_offsets() {
        let (_instance, device) = open_device();
        let queue = device
            .queue(QueueKind::Graphics)
            .expect("the graphics queue exists");

        // A source whose bytes are all distinct within the copied window, so a
        // copy that is off by one lands somewhere the assertion notices.
        let source: Vec<u8> = (0..64u8).map(|byte| byte.wrapping_add(1)).collect();
        let upload = device
            .create_buffer(&buffer(64, MemoryLocation::HostUpload))
            .expect("an upload buffer");
        device
            .write_buffer(upload, 0, &source)
            .expect("HostUpload is what write_buffer is for");
        let readback = readback_buffer(&device, 64);

        let mut encoder = device.create_command_encoder(&CommandEncoderDesc {
            label: Some("crcbl-mtl b2b"),
            queue,
        });
        encoder.copy_buffer_to_buffer(&BufferCopy {
            src: upload,
            src_offset: 8,
            dst: readback,
            dst_offset: 16,
            size: 32,
        });
        let commands = encoder.finish().expect("the recording is complete");
        device
            .submit(queue, &SubmitInfo::new(&[commands]))
            .expect("the queue accepts it");
        let request = device
            .request_readback(&ReadbackDesc {
                label: None,
                buffer: readback,
                offset: 0,
                size: 64,
                after: None,
            })
            .expect("a HostReadback buffer, in range");

        let bytes = drain(&device, request, 64);
        let mut want = vec![POISON; 64];
        want[16..48].copy_from_slice(&source[8..40]);
        assert_eq!(bytes, want, "the copy moved the wrong window");

        device.destroy_readback(request);
        device.destroy_command_buffer(commands);
        device.destroy_buffer(upload);
        device.destroy_buffer(readback);
    }

    /// **The premise the barrier decision rests on**: every resource this
    /// backend allocates is hazard-tracked by the driver, so Metal inserts the
    /// dependency between encoders itself and
    /// [`CommandEncoder::pipeline_barrier`] has only to create the boundary.
    ///
    /// Read off the objects rather than asserted on paper. **What turns it
    /// red:** adding `MTLResourceHazardTrackingModeUntracked` to
    /// `conv::resource_options`, or moving allocation onto an `MTLHeap` — the
    /// two changes that would make `pipeline_barrier` a silent no-op instead of
    /// a working barrier.
    #[test]
    #[ignore = "needs a real Metal device; run tests/run-mtl-e2e.sh"]
    fn every_resource_is_hazard_tracked() {
        let (_instance, device) = open_device();
        assert!(!LOCATIONS.is_empty(), "nothing to check");
        for &location in LOCATIONS {
            let handle = device
                .create_buffer(&buffer(256, location))
                .unwrap_or_else(|error| panic!("{location:?}: {error:?}"));
            let state = device.state();
            let entry = lookup(&state.buffers, "buffer", handle, &*device.inner)
                .expect("the buffer is live");
            assert_ne!(
                entry.raw.hazardTrackingMode(),
                MTLHazardTrackingMode::Untracked,
                "{location:?}: an untracked buffer gets no implicit dependency, so \
                 pipeline_barrier would be a no-op that looks like a barrier"
            );
            assert!(
                !entry
                    .raw
                    .resourceOptions()
                    .contains(objc2_metal::MTLResourceOptions::HazardTrackingModeUntracked),
                "{location:?}: resource_options asked for untracked"
            );
            drop(state);
            device.destroy_buffer(handle);
        }

        let (image, view) = color_target(&device);
        let state = device.state();
        let entry =
            lookup(&state.images, "image", image, &*device.inner).expect("the image is live");
        assert_ne!(
            entry.raw.hazardTrackingMode(),
            MTLHazardTrackingMode::Untracked,
            "an untracked texture has the same problem as an untracked buffer"
        );
        drop(state);
        device.destroy_image_view(view);
        device.destroy_image(image);
    }

    /// A timeline semaphore carries its initial value, is signalled by a
    /// submission, and is observable and waitable from the CPU.
    ///
    /// **What turns it red.** Dropping the `setSignaledValue:` for
    /// `initial_value` — the first assertion. Not encoding the signal, or
    /// encoding it onto a command buffer that is never committed — the wait
    /// times out and the value never moves. Returning `Ok(true)` from a wait
    /// that timed out — the second assertion, which is made *before* anything
    /// signals. Dropping the monotonicity check — the last one.
    #[test]
    #[ignore = "needs a real Metal device; run tests/run-mtl-e2e.sh"]
    fn a_metal_timeline_semaphore_signals_from_a_submission_and_the_cpu_sees_it() {
        let (_instance, device) = open_device();
        let queue = device
            .queue(QueueKind::Graphics)
            .expect("the graphics queue exists");
        let semaphore = device
            .create_semaphore(&SemaphoreDesc {
                label: Some("crcbl-mtl timeline"),
                kind: crcbl_hal::SemaphoreKind::Timeline { initial_value: 5 },
            })
            .expect("this device reports TIMELINE_SEMAPHORE");
        assert_eq!(
            device.semaphore_value(semaphore).expect("a timeline value"),
            5,
            "initial_value must reach MTLSharedEvent::setSignaledValue:"
        );

        // Nothing has signalled 9 yet, so this must time out — and a timeout is
        // a normal outcome the seam spells `Ok(false)`, not an error.
        assert!(
            !device
                .wait_semaphores(
                    &[SemaphoreWait {
                        semaphore,
                        value: 9
                    }],
                    1_000_000
                )
                .expect("a timeout is not a failure"),
            "a wait for a value nothing has signalled must not be satisfied"
        );

        device
            .submit(
                queue,
                &SubmitInfo {
                    command_buffers: &[],
                    waits: &[],
                    signals: &[SemaphoreSignal {
                        semaphore,
                        value: 9,
                    }],
                },
            )
            .expect("a signal-only submission is legal");
        assert!(
            device
                .wait_semaphores(
                    &[SemaphoreWait {
                        semaphore,
                        value: 9
                    }],
                    10_000_000_000
                )
                .expect("the wait resolves"),
            "the submission's signal never reached the event"
        );
        assert_eq!(
            device.semaphore_value(semaphore).expect("a timeline value"),
            9
        );

        // Backwards is refused. Without this the event would never reach the
        // value a later waiter is blocked on, and the process would hang with
        // nothing to point at.
        let error = device
            .submit(
                queue,
                &SubmitInfo {
                    command_buffers: &[],
                    waits: &[],
                    signals: &[SemaphoreSignal {
                        semaphore,
                        value: 9,
                    }],
                },
            )
            .expect_err("9 has already been signalled");
        assert!(matches!(error, HalError::InvalidDescriptor(_)), "{error:?}");

        device.destroy_semaphore(semaphore);
    }

    /// **`poll_readback` is a poll**: pending before its completion point and
    /// ready after, with the right bytes.
    ///
    /// The pending half is deterministic rather than a race, and that is the
    /// whole design of this test: the request names an explicit timeline value
    /// that **nothing has signalled yet**, so a correct implementation cannot
    /// report `Ready` however fast the GPU is.
    ///
    /// **What turns it red.** Returning `Ready` unconditionally, or reading the
    /// `Shared` buffer without observing the completion point at all — the
    /// first assertion. Never reaching `Ready` — `drain`'s deadline. Copying
    /// from the wrong offset — the byte comparison.
    #[test]
    #[ignore = "needs a real Metal device; run tests/run-mtl-e2e.sh"]
    fn a_readback_is_pending_before_its_completion_point_and_ready_after() {
        let (_instance, device) = open_device();
        let queue = device
            .queue(QueueKind::Graphics)
            .expect("the graphics queue exists");
        let semaphore = device
            .create_semaphore(&SemaphoreDesc {
                label: Some("crcbl-mtl readback gate"),
                kind: crcbl_hal::SemaphoreKind::Timeline { initial_value: 0 },
            })
            .expect("this device reports TIMELINE_SEMAPHORE");

        let source = [0x11u8, 0x22, 0x33, 0x44, 0x55, 0x66, 0x77, 0x88];
        let upload = device
            .create_buffer(&buffer(source.len() as u64, MemoryLocation::HostUpload))
            .expect("an upload buffer");
        device
            .write_buffer(upload, 0, &source)
            .expect("HostUpload is writable");
        let readback = readback_buffer(&device, source.len() as u64);

        let mut encoder = device.create_command_encoder(&CommandEncoderDesc {
            label: Some("crcbl-mtl gated copy"),
            queue,
        });
        encoder.copy_buffer_to_buffer(&BufferCopy {
            src: upload,
            src_offset: 0,
            dst: readback,
            dst_offset: 0,
            size: source.len() as u64,
        });
        let commands = encoder.finish().expect("the recording is complete");
        device
            .submit(queue, &SubmitInfo::new(&[commands]))
            .expect("the queue accepts it");

        let request = device
            .request_readback(&ReadbackDesc {
                label: Some("gated"),
                buffer: readback,
                offset: 0,
                size: source.len() as u64,
                after: Some(SemaphoreWait {
                    semaphore,
                    value: 1,
                }),
            })
            .expect("a HostReadback buffer and a timeline semaphore");
        let mut out = vec![0u8; source.len()];
        assert_eq!(
            device
                .poll_readback(request, &mut out)
                .expect("the readback resolves"),
            ReadbackState::Pending,
            "nothing has signalled value 1, so this cannot be ready"
        );
        assert!(
            out.iter().all(|byte| *byte == 0),
            "a pending poll must leave the output slice untouched"
        );

        device
            .submit(
                queue,
                &SubmitInfo {
                    command_buffers: &[],
                    waits: &[],
                    signals: &[SemaphoreSignal {
                        semaphore,
                        value: 1,
                    }],
                },
            )
            .expect("a signal-only submission is legal");
        assert_eq!(drain(&device, request, source.len()), source);

        // The wrong output length is a caller bug with its own contract.
        let mut wrong = vec![0u8; source.len() + 1];
        let error = device
            .poll_readback(request, &mut wrong)
            .expect_err("the slice must be exactly ReadbackDesc::size");
        assert!(matches!(error, HalError::InvalidDescriptor(_)), "{error:?}");

        device.destroy_readback(request);
        device.destroy_command_buffer(commands);
        device.destroy_semaphore(semaphore);
        device.destroy_buffer(upload);
        device.destroy_buffer(readback);
    }

    /// A submission carrying a wait runs, and a wait nothing can satisfy is
    /// refused instead of stopping the queue.
    ///
    /// The two halves are the two things that matter about
    /// `encodeWaitForEvent:value:` on this backend. The first is that a wait on
    /// an already-signalled value does not **wedge** the submission it gates —
    /// the failure that would otherwise appear as a hang with nothing to point
    /// at. The second is the deadlock guard: with one queue and no CPU-side
    /// signal, a wait for a value no earlier submission encoded can never be
    /// satisfied, and refusing it is the only way that ends in a message rather
    /// than in silence.
    ///
    /// **What turns it red.** A wait encoded onto a command buffer that is
    /// never committed, or encoded after the work rather than before it, leaves
    /// the copy's completion point unreachable and `drain` hits its deadline.
    /// Dropping the guard makes the first half return `Ok` — and, run for real
    /// in that shape, would stop the queue, which is what the guard exists to
    /// prevent. Refusing it as anything but [`HalError::Unsupported`] turns it
    /// red too: that variant is the one a caller branches on, and
    /// [`Capability::TimelineWaitBeforeSignal`](crcbl_hal::Capability::TimelineWaitBeforeSignal)
    /// is what the refusal answers.
    ///
    /// It deliberately does **not** claim to prove the wait *gated* anything.
    /// Proving that needs an observation taken between two submissions, and
    /// every such observation is a race rather than an assertion.
    #[test]
    #[ignore = "needs a real Metal device; run tests/run-mtl-e2e.sh"]
    fn a_wait_runs_when_it_is_satisfiable_and_is_refused_when_it_is_not() {
        let (_instance, device) = open_device();
        let queue = device
            .queue(QueueKind::Graphics)
            .expect("the graphics queue exists");
        let semaphore = device
            .create_semaphore(&SemaphoreDesc {
                label: Some("crcbl-mtl gate"),
                kind: crcbl_hal::SemaphoreKind::Timeline { initial_value: 0 },
            })
            .expect("this device reports TIMELINE_SEMAPHORE");

        // Nothing has encoded a signal yet, so this wait could only be met by a
        // later submission — which the queue can never reach while it waits.
        let error = device
            .submit(
                queue,
                &SubmitInfo {
                    command_buffers: &[],
                    waits: &[SemaphoreWait {
                        semaphore,
                        value: 1,
                    }],
                    signals: &[],
                },
            )
            .expect_err("nothing can ever satisfy that wait");
        // `Unsupported`, because the wait is well-formed — `crcbl-vk` performs
        // exactly this one — and what is absent is the capability. A caller
        // matching on the variant can split its submission in two; one told its
        // descriptor was invalid would go looking for a typo.
        assert!(
            matches!(&error, HalError::Unsupported { backend, .. } if *backend == BackendKind::Metal),
            "{error:?}"
        );

        let source = [0xDEu8, 0xAD, 0xBE, 0xEF];
        let upload = device
            .create_buffer(&buffer(4, MemoryLocation::HostUpload))
            .expect("an upload buffer");
        device.write_buffer(upload, 0, &source).expect("writable");
        let readback = readback_buffer(&device, 4);

        // Signal in its own, earlier submission, so the gate is open before the
        // work that waits on it is committed.
        device
            .submit(
                queue,
                &SubmitInfo {
                    command_buffers: &[],
                    waits: &[],
                    signals: &[SemaphoreSignal {
                        semaphore,
                        value: 1,
                    }],
                },
            )
            .expect("a signal-only submission is legal");

        let mut encoder = device.create_command_encoder(&CommandEncoderDesc {
            label: Some("crcbl-mtl gated"),
            queue,
        });
        encoder.copy_buffer_to_buffer(&BufferCopy {
            src: upload,
            src_offset: 0,
            dst: readback,
            dst_offset: 0,
            size: 4,
        });
        let commands = encoder.finish().expect("the recording is complete");
        device
            .submit(
                queue,
                &SubmitInfo {
                    command_buffers: &[commands],
                    waits: &[SemaphoreWait {
                        semaphore,
                        value: 1,
                    }],
                    signals: &[],
                },
            )
            .expect("the gate is already open");
        let request = device
            .request_readback(&ReadbackDesc {
                label: None,
                buffer: readback,
                offset: 0,
                size: 4,
                after: None,
            })
            .expect("a HostReadback buffer, in range");
        assert_eq!(drain(&device, request, 4), source);

        device.destroy_readback(request);
        device.destroy_command_buffer(commands);
        device.destroy_semaphore(semaphore);
        device.destroy_buffer(upload);
        device.destroy_buffer(readback);
    }

    /// A command buffer is submitted once. Metal raises on a second `commit`,
    /// and a raise aborts the process, so the guard has to be here.
    ///
    /// **What turns it red:** dropping `CommandBufferEntry::committed` — the
    /// second submit would then reach the driver, and this test would abort the
    /// whole run rather than fail, which is itself the loudest possible signal.
    #[test]
    #[ignore = "needs a real Metal device; run tests/run-mtl-e2e.sh"]
    fn a_command_buffer_cannot_be_submitted_twice() {
        let (_instance, device) = open_device();
        let queue = device
            .queue(QueueKind::Graphics)
            .expect("the graphics queue exists");
        let encoder = device.create_command_encoder(&CommandEncoderDesc {
            label: Some("crcbl-mtl empty"),
            queue,
        });
        let commands = encoder.finish().expect("an empty recording is complete");
        device
            .submit(queue, &SubmitInfo::new(&[commands]))
            .expect("the first submission");
        let error = device
            .submit(queue, &SubmitInfo::new(&[commands]))
            .expect_err("the second submission must not reach commit");
        assert!(matches!(error, HalError::InvalidDescriptor(_)), "{error:?}");

        device.wait_idle().expect("the queue drains");
        device.destroy_command_buffer(commands);
        // And the handle stops resolving once released.
        let error = device
            .submit(queue, &SubmitInfo::new(&[commands]))
            .expect_err("the handle was destroyed");
        assert!(
            matches!(error, HalError::InvalidHandle { kind, .. } if kind == "command buffer"),
            "{error:?}"
        );
    }

    /// An encoder made against another device's queue refuses at `finish`
    /// rather than recording onto the wrong device.
    #[test]
    #[ignore = "needs a real Metal device; run tests/run-mtl-e2e.sh"]
    fn a_metal_encoder_built_on_a_foreign_queue_refuses() {
        let (_instance, device) = open_device();
        let (_other_instance, other) = open_device();
        let foreign = other
            .queue(QueueKind::Graphics)
            .expect("the other device has a queue");
        let encoder = device.create_command_encoder(&CommandEncoderDesc {
            label: None,
            queue: foreign,
        });
        let error = encoder
            .finish()
            .expect_err("that queue is not this device's");
        assert!(
            matches!(error, HalError::ForeignObject { kind, .. } if kind == "queue"),
            "{error:?}"
        );
    }

    /// The device opens, says which backend it is, and has exactly the queue
    /// Metal has.
    #[test]
    #[ignore = "needs a real Metal device; run tests/run-mtl-e2e.sh"]
    fn a_device_reports_metal_and_one_graphics_queue() {
        let (_instance, device) = open_device();
        assert_eq!(device.backend(), BackendKind::Metal);
        assert!(
            device.queue(QueueKind::Graphics).is_some(),
            "every MTLDevice has a command queue"
        );
        assert!(
            device.queue(QueueKind::Compute).is_none(),
            "Metal has no queue families, so there is no separate compute queue"
        );
        assert!(device.queue(QueueKind::Transfer).is_none());
        // A queue handle is per device, not per process.
        let (_second_instance, other) = open_device();
        assert_ne!(
            device.queue(QueueKind::Graphics),
            other.queue(QueueKind::Graphics),
            "two devices must not issue the same queue handle"
        );
    }

    /// `caps` is the adapter's caps: Metal enables nothing and disables
    /// nothing, so a device that reported less than its adapter would be
    /// lying about hardware it can use.
    #[test]
    #[ignore = "needs a real Metal device; run tests/run-mtl-e2e.sh"]
    fn metal_device_caps_match_the_adapter_they_came_from() {
        let instance = open_instance();
        let adapters = instance.adapters();
        assert!(!adapters.is_empty(), "nothing to check");
        let device = instance
            .open_device(&device_desc(adapters[0].id))
            .expect("a Metal device opens with no required features");
        assert_eq!(device.caps().features, adapters[0].caps.features);
        assert_eq!(device.caps().limits, adapters[0].caps.limits);
    }

    /// A buffer of every memory location creates and destroys, and a destroyed
    /// handle fails **as a stale handle** rather than as anything else.
    ///
    /// The variant matters and is the reason this asserts on it: a
    /// `DeviceLocal` buffer that is still alive fails `write_buffer` with
    /// `InvalidDescriptor`, so "it returned an error" would pass whether or not
    /// the handle was ever invalidated.
    #[test]
    #[ignore = "needs a real Metal device; run tests/run-mtl-e2e.sh"]
    fn metal_buffers_of_every_memory_location_create_and_then_stop_resolving() {
        let (_instance, device) = open_device();
        assert!(!LOCATIONS.is_empty(), "nothing to check");
        for &location in LOCATIONS {
            let handle = device
                .create_buffer(&buffer(4096, location))
                .unwrap_or_else(|error| panic!("{location:?}: {error:?}"));

            device.destroy_buffer(handle);

            let error = device
                .write_buffer(handle, 0, &[0u8; 4])
                .expect_err("the handle was destroyed");
            assert!(
                matches!(error, HalError::InvalidHandle { kind, .. } if kind == "buffer"),
                "{location:?}: expected a stale-handle failure, got {error:?}"
            );
        }
    }

    /// `write_buffer` writes, at the offset it was given, and refuses the
    /// location Metal cannot reach without a blit.
    #[test]
    #[ignore = "needs a real Metal device; run tests/run-mtl-e2e.sh"]
    fn write_buffer_writes_host_visible_memory_and_refuses_what_metal_cannot_map() {
        let (_instance, device) = open_device();
        let readback = device
            .create_buffer(&buffer(16, MemoryLocation::HostReadback))
            .expect("a readback buffer");

        // Two writes, so the result is fully determined whatever Metal left in
        // the fresh allocation: fill, then overwrite a window at an offset.
        device
            .write_buffer(readback, 0, &[0xAA; 16])
            .expect("the whole range");
        device
            .write_buffer(readback, 4, &[0x01, 0x02, 0x03, 0x04])
            .expect("a window at an offset");
        let mut expected = [0xAA_u8; 16];
        expected[4..8].copy_from_slice(&[0x01, 0x02, 0x03, 0x04]);
        assert_eq!(
            read_back(&device, readback, 16),
            expected,
            "write_buffer either wrote nothing or ignored the offset"
        );

        // An upload buffer is write-combined, so it is written and not read
        // here — the acceptance is the assertion.
        let upload = device
            .create_buffer(&buffer(16, MemoryLocation::HostUpload))
            .expect("an upload buffer");
        device
            .write_buffer(upload, 0, &[0x7F; 16])
            .expect("HostUpload is what write_buffer is for");

        let private = device
            .create_buffer(&buffer(16, MemoryLocation::DeviceLocal))
            .expect("a device-local buffer");
        let error = device
            .write_buffer(private, 0, &[0u8; 4])
            .expect_err("a Private buffer has no contents pointer");
        let HalError::InvalidDescriptor(text) = error else {
            panic!("expected InvalidDescriptor, got {error:?}");
        };
        assert!(text.contains("DeviceLocal"), "{text}");
        assert!(
            text.contains("blit"),
            "the refusal must say what would make it work: {text}"
        );

        // Out of range is the other refusal the seam names.
        let error = device
            .write_buffer(readback, 13, &[0u8; 4])
            .expect_err("13..17 does not fit in 16 bytes");
        assert!(matches!(error, HalError::InvalidDescriptor(_)), "{error:?}");

        for handle in [readback, upload, private] {
            device.destroy_buffer(handle);
        }
    }

    /// Images, views and samplers all the way through, and the handles stop
    /// resolving when they are destroyed.
    #[test]
    #[ignore = "needs a real Metal device; run tests/run-mtl-e2e.sh"]
    fn metal_images_views_and_samplers_create_and_destroy() {
        let (_instance, device) = open_device();
        let extent = Extent3d::d2(64, 64);
        let image = device
            .create_image(&ImageDesc {
                label: Some("crcbl-mtl test image"),
                image_type: ImageType::D2,
                extent,
                format: Format::Rgba8Unorm,
                mip_levels: extent.full_mip_levels(ImageType::D2),
                samples: 1,
                usage: ImageUsage::SAMPLED | ImageUsage::COLOR_ATTACHMENT,
            })
            .expect("a 2D colour image");

        let view = device
            .create_image_view(&ImageViewDesc {
                label: Some("crcbl-mtl test view"),
                image,
                view_type: ImageViewType::D2,
                format: Format::Rgba8Unorm,
                range: ImageSubresourceRange::all(Format::Rgba8Unorm),
            })
            .expect("a whole-image view");

        // One mip of the chain, which is the case the ALL sentinel resolution
        // has to get right.
        let one_mip = device
            .create_image_view(&ImageViewDesc {
                label: Some("crcbl-mtl one mip"),
                image,
                view_type: ImageViewType::D2,
                format: Format::Rgba8Unorm,
                range: ImageSubresourceRange {
                    aspect: ImageAspect::COLOR,
                    base_mip: 1,
                    mip_count: 1,
                    base_layer: 0,
                    layer_count: 1,
                },
            })
            .expect("a single-mip view");
        assert_ne!(view, one_mip);

        let sampler = device
            .create_sampler(&SamplerDesc {
                label: Some("crcbl-mtl test sampler"),
                ..SamplerDesc::default()
            })
            .expect("the seam's default sampler");

        device.destroy_image_view(one_mip);
        device.destroy_image_view(view);
        device.destroy_sampler(sampler);
        device.destroy_image(image);

        let error = device
            .create_image_view(&ImageViewDesc {
                label: None,
                image,
                view_type: ImageViewType::D2,
                format: Format::Rgba8Unorm,
                range: ImageSubresourceRange::all(Format::Rgba8Unorm),
            })
            .expect_err("the image was destroyed");
        assert!(
            matches!(error, HalError::InvalidHandle { kind, .. } if kind == "image"),
            "{error:?}"
        );
    }

    /// The sRGB reinterpretation the seam names, exercised end to end.
    ///
    /// This is what `MTLTextureUsagePixelFormatView` is set for. Without it
    /// Metal refuses the view, so this is the assertion that keeps the flag
    /// from being "optimised away" later.
    #[test]
    #[ignore = "needs a real Metal device; run tests/run-mtl-e2e.sh"]
    fn a_linear_image_can_be_viewed_as_its_srgb_partner() {
        let (_instance, device) = open_device();
        let image = device
            .create_image(&ImageDesc {
                label: Some("linear albedo"),
                image_type: ImageType::D2,
                extent: Extent3d::d2(32, 32),
                format: Format::Rgba8Unorm,
                mip_levels: 1,
                samples: 1,
                usage: ImageUsage::SAMPLED,
            })
            .expect("a linear image");
        let view = device
            .create_image_view(&ImageViewDesc {
                label: Some("sRGB view"),
                image,
                view_type: ImageViewType::D2,
                format: Format::Rgba8UnormSrgb,
                range: ImageSubresourceRange::all(Format::Rgba8Unorm),
            })
            .expect("an sRGB view of a linear image");
        device.destroy_image_view(view);
        device.destroy_image(image);
    }

    /// A depth image cannot be reinterpreted, and says so instead of letting
    /// Metal raise.
    #[test]
    #[ignore = "needs a real Metal device; run tests/run-mtl-e2e.sh"]
    fn a_depth_image_refuses_a_differing_view_format() {
        let (_instance, device) = open_device();
        let image = device
            .create_image(&ImageDesc {
                label: Some("depth"),
                image_type: ImageType::D2,
                extent: Extent3d::d2(32, 32),
                format: Format::D32Float,
                mip_levels: 1,
                samples: 1,
                usage: ImageUsage::DEPTH_STENCIL_ATTACHMENT,
            })
            .expect("a depth image");
        let error = device
            .create_image_view(&ImageViewDesc {
                label: None,
                image,
                view_type: ImageViewType::D2,
                format: Format::R32Float,
                range: ImageSubresourceRange::all(Format::D32Float),
            })
            .expect_err("depth formats have no compatible reinterpretation");
        assert!(matches!(error, HalError::InvalidDescriptor(_)), "{error:?}");

        // The same-format view is still fine, so the refusal is about the
        // reinterpretation and not about depth images.
        let view = device
            .create_image_view(&ImageViewDesc {
                label: None,
                image,
                view_type: ImageViewType::D2,
                format: Format::D32Float,
                range: ImageSubresourceRange::all(Format::D32Float),
            })
            .expect("a depth view of a depth image");
        device.destroy_image_view(view);
        device.destroy_image(image);
    }

    /// Anisotropy is bounded on both sides, and the bound is the one the
    /// adapter reports.
    #[test]
    #[ignore = "needs a real Metal device; run tests/run-mtl-e2e.sh"]
    fn metal_samplers_reject_anisotropy_outside_the_reported_cap() {
        let (_instance, device) = open_device();
        let cap = device.caps().limits.max_sampler_anisotropy;
        assert!(
            cap > 1.0,
            "this backend reports SAMPLER_ANISOTROPY, so the cap must exceed the value that \
             disables it — at 1.0 every assertion below still passes and the feature has \
             silently gone away"
        );

        let too_much = device.create_sampler(&SamplerDesc {
            anisotropy: cap + 1.0,
            ..SamplerDesc::default()
        });
        assert!(
            matches!(too_much, Err(HalError::InvalidDescriptor(_))),
            "{too_much:?}"
        );

        let too_little = device.create_sampler(&SamplerDesc {
            anisotropy: 0.5,
            ..SamplerDesc::default()
        });
        assert!(
            matches!(too_little, Err(HalError::InvalidDescriptor(_))),
            "{too_little:?}"
        );

        // And a comparison sampler at the cap is accepted — the shadow-map
        // shape, with the reversed-Z comparison the seam documents.
        let sampler = device
            .create_sampler(&SamplerDesc {
                label: Some("shadow"),
                anisotropy: cap,
                compare: Some(crcbl_hal::CompareOp::Greater),
                ..SamplerDesc::default()
            })
            .expect("a comparison sampler at the reported cap");
        device.destroy_sampler(sampler);
    }

    /// **`wait_idle` waits for work committed before it, and something says so.**
    ///
    /// Two `Ok`s and nothing else is exactly what a `wait_idle` that returned
    /// without waiting would also produce, so the observable is a timeline
    /// semaphore signalled by a submission committed *before* the wait. An
    /// `MTLCommandQueue` runs its command buffers in commit order — the property
    /// `wait_idle`'s own documentation rests on — so its empty buffer cannot
    /// complete until that submission has, and the signal encoded on that
    /// submission has therefore fired by the time the call returns.
    ///
    /// The submission carries a real blit rather than being empty, so a
    /// `wait_idle` that dropped `waitUntilCompleted` has actual GPU work to
    /// outrun instead of a signal that may already have landed while it was
    /// being asked about.
    ///
    /// **What turns it red.** Dropping the `waitUntilCompleted` — the value read
    /// back is the one the previous line asserted, not the one the submission
    /// signals. Committing `wait_idle`'s buffer to something other than the
    /// device's own queue — the ordering it relies on is between buffers on one
    /// queue, and across two the wait says nothing about the blit. The second
    /// round is what a queue that works exactly once fails.
    #[test]
    #[ignore = "needs a real Metal device; run tests/run-mtl-e2e.sh"]
    fn wait_idle_waits_for_the_work_committed_before_it() {
        let (_instance, device) = open_device();
        let queue = device
            .queue(QueueKind::Graphics)
            .expect("the graphics queue exists");
        let semaphore = device
            .create_semaphore(&SemaphoreDesc {
                label: Some("crcbl-mtl wait_idle observer"),
                kind: SemaphoreKind::Timeline { initial_value: 0 },
            })
            .expect("this device reports TIMELINE_SEMAPHORE");

        /// Bytes moved by the blit each round.
        ///
        /// Large enough that a `wait_idle` which did not wait would have to
        /// outrun a real copy rather than a command buffer with nothing in it,
        /// and small enough to allocate twice on the smallest Mac this runs on.
        const COPIED: u64 = 16 << 20;

        let source = device
            .create_buffer(&BufferDesc {
                label: Some("crcbl-mtl wait_idle source"),
                size: COPIED,
                usage: BufferUsage::TRANSFER_SRC,
                memory: MemoryLocation::DeviceLocal,
            })
            .expect("a device-local blit source");
        let destination = device
            .create_buffer(&BufferDesc {
                label: Some("crcbl-mtl wait_idle destination"),
                size: COPIED,
                usage: BufferUsage::TRANSFER_DST,
                memory: MemoryLocation::DeviceLocal,
            })
            .expect("a device-local blit destination");

        let mut submitted = Vec::new();
        for round in 1..=2u64 {
            assert_ne!(
                device.semaphore_value(semaphore).expect("a timeline value"),
                round,
                "round {round} is being observed with the value it is about to signal"
            );

            let mut encoder = device.create_command_encoder(&CommandEncoderDesc {
                label: Some("crcbl-mtl wait_idle work"),
                queue,
            });
            encoder.copy_buffer_to_buffer(&BufferCopy {
                src: source,
                src_offset: 0,
                dst: destination,
                dst_offset: 0,
                size: COPIED,
            });
            let commands = encoder.finish().expect("the recording is complete");
            device
                .submit(
                    queue,
                    &SubmitInfo {
                        command_buffers: &[commands],
                        waits: &[],
                        signals: &[SemaphoreSignal {
                            semaphore,
                            value: round,
                        }],
                    },
                )
                .expect("the queue accepts it");
            submitted.push(commands);

            device
                .wait_idle()
                .unwrap_or_else(|error| panic!("round {round}: {error:?}"));

            let observed = device.semaphore_value(semaphore).expect("a timeline value");
            assert_eq!(
                observed, round,
                "round {round}: wait_idle returned with the timeline at {observed}, so it did \
                 not wait for the blit committed before it"
            );
        }

        for commands in submitted {
            device.destroy_command_buffer(commands);
        }
        device.destroy_semaphore(semaphore);
        device.destroy_buffer(source);
        device.destroy_buffer(destination);
    }

    /// Every slice that has not arrived still refuses, by name — so none of
    /// them can be half-implemented without this saying so.
    ///
    /// **The binding and dispatch slices emptied most of this list**, and what
    /// is left is the two things that are not "a slice nobody has written": the
    /// two **counter-sampled** query kinds, whose obstacle is a device that
    /// advertises no counter set, and the indirect-count draws, whose obstacle is
    /// named in `crcbl_mtl::command`'s `indirect_count`. The occlusion kind is
    /// not among them any more — `Device::create_query_set` builds it, and
    /// `crates/crcbl/tests/hal_seam_e2e.rs`'s `exercise_query_set_creation` is
    /// what drives it. The calls that stopped refusing are asserted in
    /// `the_binding_slice_replaced_refusals_with_real_errors` and in that same
    /// suite's `a_compute_dispatch_writes_the_values_it_was_asked_for` rather
    /// than merely dropped from here.
    #[test]
    #[ignore = "needs a real Metal device; run tests/run-mtl-e2e.sh"]
    fn the_metal_slices_that_have_not_arrived_still_refuse_and_name_themselves() {
        let (_instance, device) = open_device();

        let refusals: Vec<(&str, HalError)> = vec![
            (
                "timestamp query sets",
                device
                    .create_query_set(&QuerySetDesc {
                        label: None,
                        kind: QueryKind::Timestamp,
                        count: 1,
                    })
                    .expect_err("no counter sample buffer yet"),
            ),
            (
                "pipeline-statistics query sets",
                device
                    .create_query_set(&QuerySetDesc {
                        label: None,
                        kind: QueryKind::PipelineStatistics,
                        count: 1,
                    })
                    .expect_err("no counter sample buffer yet"),
            ),
        ];
        assert!(!refusals.is_empty(), "nothing to check");
        for (what, error) in &refusals {
            assert!(
                matches!(error, HalError::Unsupported { backend, .. } if *backend == BackendKind::Metal),
                "{what}: {error:?}"
            );
            let text = error.to_string();
            assert!(text.contains("metal"), "{what}: {text}");
            assert!(
                text.contains("Metal") && text.contains("slice"),
                "{what}: {text}"
            );
        }

        // Recording works now, so the encoder's refusals moved to the commands
        // that still need a pipeline. `create_command_encoder` returns a bare
        // `Box`, so a draw has nowhere to report itself and `finish` carries it.
        let queue = device
            .queue(QueueKind::Graphics)
            .expect("the graphics queue exists");
        /// One recording call that still has to refuse, and the name it must
        /// refuse under.
        type Refused = (&'static str, fn(&mut dyn CommandEncoder));

        fn count(encoder: &mut dyn CommandEncoder, indexed: bool) {
            let draw = crcbl_hal::DrawIndirectCount {
                args: Handle::from_bits(1 << 32).expect("generation 1"),
                args_offset: 0,
                count_buffer: Handle::from_bits(1 << 32).expect("generation 1"),
                count_offset: 0,
                max_draw_count: 1,
                stride: 20,
            };
            if indexed {
                encoder.draw_indexed_indirect_count(&draw);
            } else {
                encoder.draw_indirect_count(&draw);
            }
        }
        let recording: &[Refused] = &[
            ("indirect-count draws", |encoder| count(encoder, false)),
            ("indexed indirect-count draws", |encoder| {
                count(encoder, true)
            }),
        ];
        assert!(!recording.is_empty(), "nothing to check");
        for (what, record) in recording {
            let mut encoder =
                device.create_command_encoder(&CommandEncoderDesc { label: None, queue });
            record(encoder.as_mut());
            let Err(error) = encoder.finish() else {
                panic!("{what} recorded successfully, so the encoder reported a lie");
            };
            assert!(
                matches!(error, HalError::Unsupported { backend, .. } if backend == BackendKind::Metal),
                "{what}: {error:?}"
            );
            let text = error.to_string();
            assert!(
                text.contains("metal") && text.contains("Metal") && text.contains("slice"),
                "{what}: {text}"
            );
        }

        // An empty submission is legal now and does nothing, which is the only
        // honest answer: there is no work to run and nothing to signal.
        device
            .submit(queue, &SubmitInfo::new(&[]))
            .expect("an empty submission is a no-op, not a refusal");

        // A queue handle really belonging to another device is foreign; a
        // hand-made one carries no device tag at all and was never issued.
        let (_other_instance, other) = open_device();
        let other_queue = other
            .queue(QueueKind::Graphics)
            .expect("the other device has a queue too");
        assert_ne!(queue, other_queue);
        let error = device
            .submit(other_queue, &SubmitInfo::new(&[]))
            .expect_err("that queue belongs to the other device");
        assert!(
            matches!(error, HalError::ForeignObject { kind, .. } if kind == "queue"),
            "{error:?}"
        );

        let untagged = Handle::from_bits(1 << 32).expect("generation 1");
        let error = device
            .submit(untagged, &SubmitInfo::new(&[]))
            .expect_err("no device ever issued that handle");
        assert!(
            matches!(error, HalError::InvalidHandle { kind, .. } if kind == "queue"),
            "{error:?}"
        );
    }

    /// The calls the binding slice took off the refusal list now fail for the
    /// reasons they *should* fail for — never with
    /// [`HalError::Unsupported`].
    ///
    /// The distinction is the whole point: "this backend cannot do that" and
    /// "you passed a handle nobody issued" send a caller to different places,
    /// and a call that kept the first message after gaining an implementation
    /// would send them to the wrong one forever.
    ///
    /// **What turns it red.** Any of these four regressing to `Unsupported`, or
    /// — for the first two — succeeding against a handle no device issued.
    #[test]
    #[ignore = "needs a real Metal device; run tests/run-mtl-e2e.sh"]
    fn the_binding_slice_replaced_refusals_with_real_errors() {
        let (_instance, device) = open_device();
        let unissued = Handle::from_bits(1 << 32).expect("generation 1");

        let error = device
            .create_bind_group(&BindGroupDesc {
                label: None,
                layout: unissued,
                entries: &[],
                variable_count: None,
            })
            .expect_err("no device issued that layout handle");
        assert!(
            matches!(error, HalError::InvalidHandle { kind, .. } if kind == "bind group layout"),
            "{error:?}"
        );

        let error = device
            .create_pipeline_layout(&PipelineLayoutDesc {
                label: None,
                bind_group_layouts: &[unissued],
                push_constants: None,
            })
            .expect_err("no device issued that layout handle");
        assert!(
            matches!(error, HalError::InvalidHandle { kind, .. } if kind == "bind group layout"),
            "{error:?}"
        );

        // The empty layout still creates, so the refusal above is about the
        // handle rather than about naming any set at all.
        let empty = empty_layout(&device);
        device.destroy_pipeline_layout(empty);

        let queue = device
            .queue(QueueKind::Graphics)
            .expect("the graphics queue exists");
        for (what, record) in [
            (
                "an index buffer nobody issued",
                (|encoder: &mut dyn CommandEncoder| {
                    encoder.bind_index_buffer(
                        Handle::from_bits(1 << 32).expect("generation 1"),
                        0,
                        crcbl_hal::IndexFormat::Uint32,
                    );
                }) as fn(&mut dyn CommandEncoder),
            ),
            // Not a capability answer any more: `setBytes:length:atIndex:`
            // writes the open encoder's argument table, so a write with no
            // encoder open is the caller's own error and must read as one.
            ("push constants outside any pass", |encoder| {
                encoder.push_constants(
                    crcbl_hal::ShaderStages::ALL,
                    0,
                    &[0u8; 4],
                    Handle::from_bits(1 << 32).expect("generation 1"),
                );
            }),
        ] {
            let mut encoder =
                device.create_command_encoder(&CommandEncoderDesc { label: None, queue });
            record(encoder.as_mut());
            let Err(error) = encoder.finish() else {
                panic!("{what} recorded successfully, so the encoder reported a lie");
            };
            assert!(
                !matches!(error, HalError::Unsupported { .. }),
                "{what} still refuses as unsupported: {error:?}"
            );
        }
    }

    /// The compute pass **opens a Metal encoder**, and the three calls that
    /// used to refuse now fail only for the reasons a caller can fix.
    ///
    /// The half of the dispatch slice a machine with no working GPU can still
    /// check: the encoder is created and closed, a well-formed empty pass
    /// submits, and every misuse comes back as a descriptor or handle error
    /// rather than as [`HalError::Unsupported`] — which is what "this backend
    /// cannot do that" means and is now the wrong answer.
    ///
    /// **What turns it red.** `dispatch` or `bind_compute_pipeline` regressing
    /// to `Unsupported`. `begin_compute_pass` opening no encoder — a dispatch
    /// with nothing bound would then be reported as "outside a compute pass",
    /// and the two messages are asserted apart. Letting a copy through inside
    /// the pass, which would end the pass's encoder underneath it and take the
    /// argument tables with it. Leaving the encoder open at `finish` — Metal
    /// raises on `commit`, which aborts this process rather than failing the
    /// test, so the empty-pass submission is the check that it closes.
    #[test]
    #[ignore = "needs a real Metal device; run tests/run-mtl-e2e.sh"]
    fn the_compute_pass_opens_an_encoder_and_its_calls_fail_only_as_themselves() {
        let (_instance, device) = open_device();
        let queue = device
            .queue(QueueKind::Graphics)
            .expect("the graphics queue exists");
        let unissued = Handle::from_bits(1 << 32).expect("generation 1");
        let encoder_of =
            || device.create_command_encoder(&CommandEncoderDesc { label: None, queue });

        // An empty pass: the encoder opens, closes, and the command buffer
        // commits — which is the only observation available without a shader.
        let mut empty = encoder_of();
        empty.begin_compute_pass(&crcbl_hal::ComputePassDesc {
            label: Some("empty compute pass"),
        });
        empty.end_compute_pass();
        let commands = empty.finish().expect("an empty compute pass records");
        device
            .submit(queue, &SubmitInfo::new(&[commands]))
            .expect("and submits");
        device.wait_idle().expect("and completes");
        device.destroy_command_buffer(commands);

        /// One recording call, and the substring its failure must contain.
        type Misuse = (&'static str, &'static str, fn(&mut dyn CommandEncoder));
        let misuses: &[Misuse] = &[
            (
                "a dispatch outside any pass",
                "outside a compute pass",
                |encoder| encoder.dispatch(1, 1, 1),
            ),
            (
                "a dispatch with no pipeline bound",
                "no compute pipeline bound",
                |encoder| {
                    encoder.begin_compute_pass(&crcbl_hal::ComputePassDesc { label: None });
                    encoder.dispatch(1, 1, 1);
                    encoder.end_compute_pass();
                },
            ),
            (
                "an indirect dispatch with no pipeline bound",
                "no compute pipeline bound",
                |encoder| {
                    encoder.begin_compute_pass(&crcbl_hal::ComputePassDesc { label: None });
                    encoder.dispatch_indirect(Handle::from_bits(1 << 32).expect("generation 1"), 0);
                    encoder.end_compute_pass();
                },
            ),
            (
                "a pipeline bind outside any pass",
                "outside a compute pass",
                |encoder| {
                    encoder
                        .bind_compute_pipeline(Handle::from_bits(1 << 32).expect("generation 1"));
                },
            ),
        ];
        for (what, expected, record) in misuses {
            let mut encoder = encoder_of();
            record(encoder.as_mut());
            let Err(error) = encoder.finish() else {
                panic!("{what} recorded successfully, so the encoder reported a lie");
            };
            assert!(
                !matches!(error, HalError::Unsupported { .. }),
                "{what} still refuses as unsupported: {error:?}"
            );
            assert!(
                error.to_string().contains(expected),
                "{what}: expected a message naming {expected:?}, got {error}"
            );
        }

        // A copy inside the pass, against a buffer that really exists so the
        // refusal is about the scope rather than the handle. Opening a blit
        // encoder here would `endEncoding` the pass's own encoder and take its
        // argument tables with it.
        let scratch = device
            .create_buffer(&buffer(16, MemoryLocation::HostUpload))
            .expect("a small host buffer");
        let mut copying = encoder_of();
        copying.begin_compute_pass(&crcbl_hal::ComputePassDesc { label: None });
        copying.fill_buffer(scratch, 0, 4, 0);
        copying.end_compute_pass();
        let error = copying
            .finish()
            .expect_err("a copy inside a compute pass is refused");
        assert!(error.to_string().contains("inside a pass"), "{error}");
        device.destroy_buffer(scratch);

        // A pipeline handle nobody issued, bound inside a real pass: the
        // failure is about the handle, which is only reachable because the
        // encoder exists to bind onto.
        let mut stale = encoder_of();
        stale.begin_compute_pass(&crcbl_hal::ComputePassDesc { label: None });
        stale.bind_compute_pipeline(unissued);
        stale.end_compute_pass();
        let error = stale.finish().expect_err("no device issued that pipeline");
        assert!(
            matches!(error, HalError::InvalidHandle { kind, .. } if kind == "compute pipeline"),
            "{error:?}"
        );
    }

    /// The indexed and indirect draws **record**, and their descriptor errors
    /// reach the caller through `finish` rather than reaching Metal.
    ///
    /// `crcbl_mtl::draw`'s own tests check the arithmetic without a device;
    /// this is the half that checks the encoder is wired to it — that a bind
    /// really is remembered until the draw, that a draw with nothing bound is
    /// caught, and that an out-of-range range or a bad stride comes back as an
    /// error instead of as a Metal raise, which aborts the process.
    ///
    /// **What turns it red.** `draw_indexed` succeeding with no index buffer
    /// bound, or failing with one. An over-long index range or a stride below
    /// one argument structure being passed through. Every case here is a
    /// *recording* check, so none of them needs the GPU to execute anything —
    /// which is why this one is not gated.
    #[test]
    #[ignore = "needs a real Metal device; run tests/run-mtl-e2e.sh"]
    fn indexed_and_indirect_draws_record_and_report_their_descriptor_errors() {
        let (_instance, device) = open_device();
        let ink = ink_msl();
        let module = device
            .create_shader_module(&msl_module(&ink, "crcbl-mtl ink.metal"))
            .expect("a shader with no bindings compiles");
        let layout = empty_layout(&device);
        let targets = [ColorTargetState::opaque(Format::Rgba8Unorm)];
        let pipeline = device
            .create_graphics_pipeline(&GraphicsPipelineDesc {
                label: Some("crcbl-mtl draws"),
                layout,
                vertex: ShaderEntry {
                    module,
                    entry_point: "vertexMain",
                },
                fragment: Some(ShaderEntry {
                    module,
                    entry_point: "fragmentMain",
                }),
                primitive: PrimitiveState::default(),
                depth_stencil: None,
                multisample: MultisampleState::default(),
                color_targets: &targets,
            })
            .expect("a colour-only pipeline");
        // 64 bytes: sixteen 32-bit indices, or three 16-byte argument
        // structures with room to spare.
        let data = device
            .create_buffer(&BufferDesc {
                label: Some("crcbl-mtl draw arguments"),
                size: 64,
                usage: BufferUsage::INDEX | BufferUsage::INDIRECT,
                memory: MemoryLocation::HostUpload,
            })
            .expect("an index and indirect buffer");

        let queue = device
            .queue(QueueKind::Graphics)
            .expect("the graphics queue exists");
        let (image, view) = color_target(&device);
        let record = |what: &str, paint: &dyn Fn(&mut dyn CommandEncoder)| {
            let mut encoder = device.create_command_encoder(&CommandEncoderDesc {
                label: Some("crcbl-mtl draws"),
                queue,
            });
            encoder.begin_render_pass(&RenderPassDesc {
                label: Some(what),
                color_attachments: &[ColorAttachment {
                    view,
                    resolve: None,
                    load: LoadOp::Clear,
                    store: StoreOp::Store,
                    clear: ClearValue::color(CLEAR),
                }],
                depth_stencil_attachment: None,
                render_area: Rect2d::from_size(TARGET.width, TARGET.height),
            });
            encoder.bind_graphics_pipeline(pipeline);
            paint(encoder.as_mut());
            encoder.end_render_pass();
            encoder.finish().map(|commands| {
                device.destroy_command_buffer(commands);
            })
        };

        record("indexed", &|encoder| {
            encoder.bind_index_buffer(data, 0, crcbl_hal::IndexFormat::Uint32);
            encoder.draw_indexed(0..3, 0, 0..1);
        })
        .expect("sixteen indices hold a range of three");
        record("indirect", &|encoder| {
            encoder.draw_indirect(&crcbl_hal::DrawIndirect {
                args: data,
                offset: 0,
                draw_count: 3,
                stride: 16,
            });
        })
        .expect("three 16-byte structures fit in 64 bytes");
        record("indexed indirect", &|encoder| {
            encoder.bind_index_buffer(data, 0, crcbl_hal::IndexFormat::Uint16);
            encoder.draw_indexed_indirect(&crcbl_hal::DrawIndirect {
                args: data,
                offset: 0,
                draw_count: 2,
                stride: 20,
            });
        })
        .expect("two 20-byte structures fit in 64 bytes");

        for (what, paint) in [
            (
                "an indexed draw with nothing bound",
                &(|encoder: &mut dyn CommandEncoder| {
                    encoder.draw_indexed(0..3, 0, 0..1);
                }) as &dyn Fn(&mut dyn CommandEncoder),
            ),
            ("an index range past the end", &|encoder| {
                encoder.bind_index_buffer(data, 0, crcbl_hal::IndexFormat::Uint32);
                encoder.draw_indexed(0..17, 0, 0..1);
            }),
            ("a stride below one argument structure", &|encoder| {
                encoder.draw_indirect(&crcbl_hal::DrawIndirect {
                    args: data,
                    offset: 0,
                    draw_count: 2,
                    stride: 8,
                });
            }),
            ("an indirect span past the end", &|encoder| {
                encoder.draw_indirect(&crcbl_hal::DrawIndirect {
                    args: data,
                    offset: 0,
                    draw_count: 5,
                    stride: 16,
                });
            }),
            ("an indexed indirect draw with nothing bound", &|encoder| {
                encoder.draw_indexed_indirect(&crcbl_hal::DrawIndirect {
                    args: data,
                    offset: 0,
                    draw_count: 1,
                    stride: 20,
                });
            }),
        ] {
            let error = record(what, paint).expect_err(what);
            assert!(
                matches!(error, HalError::InvalidDescriptor(_)),
                "{what}: {error:?}"
            );
        }

        device.destroy_image_view(view);
        device.destroy_image(image);
        device.destroy_buffer(data);
        device.destroy_graphics_pipeline(pipeline);
        device.destroy_pipeline_layout(layout);
        device.destroy_shader_module(module);
    }

    /// **The milestone this phase exists for: the engine's own
    /// `triangle.slang`, drawn.**
    ///
    /// MTL4 could compile `msl/triangle.metal` and build an
    /// `MTLRenderPipelineState` from it and could not *draw* with it, because
    /// the shader pulls its vertices from a `StructuredBuffer` that Slang
    /// lowers to `[[buffer(0)]]` on **both** stages and nothing could bind a
    /// buffer. This is the same artifact, the same entry points, and a real
    /// bind group carrying real vertices — so what it paints is produced by the
    /// engine's shader rather than by one written for the test.
    ///
    /// The three vertices carry **the same colour**, which is deliberate: the
    /// shader interpolates `color` across the triangle, and identical inputs
    /// interpolate to exactly that value at every covered pixel. An
    /// interpolated *gradient* would make the expected centre texel depend on
    /// the exact rasterisation of the vertices, which is not a thing to assert.
    ///
    /// The layout declares the buffer visible to vertex **and** fragment for
    /// the same reason: Slang emitted the argument on both stages, and a
    /// vertex-only layout would leave the fragment stage's copy unbound.
    ///
    /// **What turns it red.** Binding nothing, or binding at the wrong
    /// argument-table index — the vertex stage reads an unbound buffer and the
    /// draw produces no triangle, so the centre comes back [`CLEAR_TEXEL`],
    /// which is asserted against explicitly. Losing the fragment-stage bind —
    /// same. Rasterising the whole target — the four corner assertions.
    /// Swapping a channel anywhere — [`INK_TEXEL`] is asymmetric in all four.
    /// Never running the copy — every texel comes back [`POISON`], which the
    /// last assertion admits nowhere.
    ///
    /// **Needs a real GPU, and CI does not have one** — see
    /// `a_metal_triangle_draw_paints_the_centre_and_leaves_the_corners_clear`, whose
    /// gating argument and measured evidence apply unchanged.
    #[cfg(feature = "mtl-e2e")]
    #[test]
    #[ignore = "executes a shader on a real Metal device; run tests/run-mtl-e2e.sh"]
    fn the_engines_own_triangle_draws_through_a_bind_group() {
        use crcbl_shaders::{Stage as ShaderStage, TRIANGLE};

        let (_instance, device) = open_device();
        assert_ne!(
            INK_TEXEL, CLEAR_TEXEL,
            "the two colours must differ or no assertion below means anything"
        );

        // `triangle.slang`'s `Vertex` is two `float4`s: clip-space position
        // then colour. The three positions are the ones `ink_msl` uses, so both
        // triangles cover the centre and leave every corner clear.
        let mut vertices = Vec::new();
        for position in [[0.0f32, 0.8], [-0.8, -0.8], [0.8, -0.8]] {
            for value in [position[0], position[1], 0.0, 1.0] {
                vertices.extend_from_slice(&value.to_ne_bytes());
            }
            for value in INK {
                vertices.extend_from_slice(&value.to_ne_bytes());
            }
        }
        let geometry = device
            .create_buffer(&BufferDesc {
                label: Some("triangle.slang vertices"),
                size: vertices.len() as u64,
                usage: BufferUsage::STORAGE,
                memory: MemoryLocation::HostUpload,
            })
            .expect("a storage buffer for three vertices");
        device
            .write_buffer(geometry, 0, &vertices)
            .expect("HostUpload is what write_buffer is for");

        let msl = TRIANGLE.msl().expect("the triangle ships an MSL artifact");
        let module = device
            .create_shader_module(&msl_module(msl, "triangle.slang"))
            .expect("the committed MSL compiles on a real Metal compiler");
        let set = device
            .create_bind_group_layout(&BindGroupLayoutDesc {
                label: Some("triangle.slang set 0"),
                entries: &[crcbl_hal::BindGroupLayoutEntry {
                    binding: 0,
                    visibility: crcbl_hal::ShaderStages::VERTEX | crcbl_hal::ShaderStages::FRAGMENT,
                    kind: crcbl_hal::BindingKind::StorageBuffer {
                        read_only: true,
                        dynamic: false,
                    },
                    count: 1,
                    flags: crcbl_hal::BindingFlags::empty(),
                }],
            })
            .expect("one read-only storage buffer, which is what the shader declares");
        let layout = device
            .create_pipeline_layout(&PipelineLayoutDesc {
                label: Some("triangle.slang"),
                bind_group_layouts: &[set],
                push_constants: None,
            })
            .expect("one set");
        let group = device
            .create_bind_group(&BindGroupDesc {
                label: Some("triangle.slang vertices"),
                layout: set,
                entries: &[BindGroupEntry {
                    binding: 0,
                    array_index: 0,
                    resource: crcbl_hal::BindingResource::whole_buffer(geometry),
                }],
                variable_count: None,
            })
            .expect("the geometry buffer in binding 0");

        let targets = [ColorTargetState::opaque(Format::Rgba8Unorm)];
        let pipeline = device
            .create_graphics_pipeline(&GraphicsPipelineDesc {
                label: Some("triangle.slang"),
                layout,
                vertex: ShaderEntry {
                    module,
                    entry_point: TRIANGLE
                        .entry_point(ShaderStage::Vertex)
                        .expect("one vertex entry point"),
                },
                fragment: Some(ShaderEntry {
                    module,
                    entry_point: TRIANGLE
                        .entry_point(ShaderStage::Fragment)
                        .expect("one fragment entry point"),
                }),
                primitive: PrimitiveState::default(),
                depth_stencil: None,
                multisample: MultisampleState::default(),
                color_targets: &targets,
            })
            .expect("the engine's own triangle over an Rgba8Unorm target");

        let bytes = draw_canvas(&device, Format::Rgba8Unorm, |encoder| {
            encoder.bind_graphics_pipeline(pipeline);
            encoder.bind_group(0, group, &[], layout);
            encoder.draw(0..3, 0..1);
        });
        assert_ink_triangle(&bytes, Format::Rgba8Unorm);

        device.destroy_graphics_pipeline(pipeline);
        device.destroy_bind_group(group);
        device.destroy_pipeline_layout(layout);
        device.destroy_bind_group_layout(set);
        device.destroy_shader_module(module);
        device.destroy_buffer(geometry);
    }

    /// **An indexed draw**, reading the index buffer
    /// [`bind_index_buffer`](crcbl_hal::CommandEncoder::bind_index_buffer)
    /// recorded — which is the whole of what Metal takes at the draw call
    /// rather than as encoder state.
    ///
    /// The index buffer names the same three vertices as
    /// `a_metal_triangle_draw_paints_the_centre_and_leaves_the_corners_clear`, in a
    /// **rotated** order and from a non-zero first index, so a draw that
    /// ignored the binding and drew `0..3` directly would still cover the
    /// centre — and would fail the offset arithmetic that this exercises:
    /// there are six indices, the draw reads `3..6`, and only the second half
    /// spells the triangle.
    ///
    /// **What turns it red.** Ignoring `indices.start` — the draw reads
    /// indices `0..3`, which are all vertex 0, so nothing is covered and the
    /// centre comes back [`CLEAR_TEXEL`]. Scaling the first index by the wrong
    /// width — same. Dropping the bind entirely — `draw_indexed` refuses and
    /// `finish` fails.
    ///
    /// **Needs a real GPU**; see the other gated draw for the evidence.
    #[cfg(feature = "mtl-e2e")]
    #[test]
    #[ignore = "executes a shader on a real Metal device; run tests/run-mtl-e2e.sh"]
    fn a_metal_indexed_draw_reads_the_bound_index_range() {
        let (_instance, device) = open_device();
        let ink = ink_msl();
        let module = device
            .create_shader_module(&msl_module(&ink, "crcbl-mtl ink.metal"))
            .expect("a shader with no bindings compiles");
        let layout = empty_layout(&device);
        let targets = [ColorTargetState::opaque(Format::Rgba8Unorm)];
        let pipeline = device
            .create_graphics_pipeline(&GraphicsPipelineDesc {
                label: Some("crcbl-mtl indexed"),
                layout,
                vertex: ShaderEntry {
                    module,
                    entry_point: "vertexMain",
                },
                fragment: Some(ShaderEntry {
                    module,
                    entry_point: "fragmentMain",
                }),
                primitive: PrimitiveState::default(),
                depth_stencil: None,
                multisample: MultisampleState::default(),
                color_targets: &targets,
            })
            .expect("a colour-only pipeline");

        // Six indices: three degenerate ones first, then the triangle.
        let indices: [u32; 6] = [0, 0, 0, 0, 1, 2];
        let mut bytes = Vec::new();
        for index in indices {
            bytes.extend_from_slice(&index.to_ne_bytes());
        }
        let index_buffer = device
            .create_buffer(&BufferDesc {
                label: Some("crcbl-mtl indices"),
                size: bytes.len() as u64,
                usage: BufferUsage::INDEX,
                memory: MemoryLocation::HostUpload,
            })
            .expect("an index buffer");
        device
            .write_buffer(index_buffer, 0, &bytes)
            .expect("HostUpload is writable");

        let painted = draw_canvas(&device, Format::Rgba8Unorm, |encoder| {
            encoder.bind_graphics_pipeline(pipeline);
            encoder.bind_index_buffer(index_buffer, 0, crcbl_hal::IndexFormat::Uint32);
            encoder.draw_indexed(3..6, 0, 0..1);
        });
        assert_ink_triangle(&painted, Format::Rgba8Unorm);

        device.destroy_buffer(index_buffer);
        device.destroy_graphics_pipeline(pipeline);
        device.destroy_pipeline_layout(layout);
        device.destroy_shader_module(module);
    }

    /// **A multi-draw indirect**, which on Metal is one
    /// `drawPrimitives:indirectBuffer:indirectBufferOffset:` per argument
    /// structure — the call that earns
    /// [`Features::MULTI_DRAW_INDIRECT`].
    ///
    /// Two structures, and only the **second** draws anything: the first has an
    /// `instanceCount` of zero, which is how a culling pass suppresses a draw
    /// it decided against. So a backend that read one structure and stopped, or
    /// that read the first one twice, paints nothing at all.
    ///
    /// **What turns it red.** Emitting one draw instead of `draw_count` — the
    /// centre comes back [`CLEAR_TEXEL`]. Ignoring the stride — the second read
    /// lands on the first structure and again draws nothing. Ignoring the base
    /// offset — likewise.
    ///
    /// **Needs a real GPU**; see the other gated draws for the evidence.
    #[cfg(feature = "mtl-e2e")]
    #[test]
    #[ignore = "executes a shader on a real Metal device; run tests/run-mtl-e2e.sh"]
    fn a_multi_draw_indirect_emits_every_argument_structure() {
        let (_instance, device) = open_device();
        let ink = ink_msl();
        let module = device
            .create_shader_module(&msl_module(&ink, "crcbl-mtl ink.metal"))
            .expect("a shader with no bindings compiles");
        let layout = empty_layout(&device);
        let targets = [ColorTargetState::opaque(Format::Rgba8Unorm)];
        let pipeline = device
            .create_graphics_pipeline(&GraphicsPipelineDesc {
                label: Some("crcbl-mtl indirect"),
                layout,
                vertex: ShaderEntry {
                    module,
                    entry_point: "vertexMain",
                },
                fragment: Some(ShaderEntry {
                    module,
                    entry_point: "fragmentMain",
                }),
                primitive: PrimitiveState::default(),
                depth_stencil: None,
                multisample: MultisampleState::default(),
                color_targets: &targets,
            })
            .expect("a colour-only pipeline");

        // `MTLDrawPrimitivesIndirectArguments`: vertexCount, instanceCount,
        // vertexStart, baseInstance — the same four fields, in the same order,
        // as Vulkan's `VkDrawIndirectCommand`.
        let structures: [[u32; 4]; 2] = [[3, 0, 0, 0], [3, 1, 0, 0]];
        let mut bytes = Vec::new();
        for structure in structures {
            for field in structure {
                bytes.extend_from_slice(&field.to_ne_bytes());
            }
        }
        let args = device
            .create_buffer(&BufferDesc {
                label: Some("crcbl-mtl indirect args"),
                size: bytes.len() as u64,
                usage: BufferUsage::INDIRECT,
                memory: MemoryLocation::HostUpload,
            })
            .expect("an indirect argument buffer");
        device
            .write_buffer(args, 0, &bytes)
            .expect("HostUpload is writable");

        let painted = draw_canvas(&device, Format::Rgba8Unorm, |encoder| {
            encoder.bind_graphics_pipeline(pipeline);
            encoder.draw_indirect(&crcbl_hal::DrawIndirect {
                args,
                offset: 0,
                draw_count: 2,
                stride: 16,
            });
        });
        assert_ink_triangle(&painted, Format::Rgba8Unorm);

        device.destroy_buffer(args);
        device.destroy_graphics_pipeline(pipeline);
        device.destroy_pipeline_layout(layout);
        device.destroy_shader_module(module);
    }

    /// Records `paint` into a [`CANVAS`]-sized pass over a `format` target,
    /// copies the result back, and hands over the texels.
    ///
    /// Shared by the gated draw tests so the pass, the barrier, the copy and
    /// the readback are written once and only the recording between
    /// `begin_render_pass` and `end_render_pass` — and the attachment's format
    /// — differ. The readback is in `format`'s channel order, which is what
    /// [`assert_ink_triangle`] takes the format for.
    ///
    /// The viewport and the scissor are always recorded, both covering the
    /// whole canvas, because that is what every draw in this backend's own
    /// suite records. The retired draw-hang probes once varied them, and
    /// eliminated both calls as suspects.
    #[cfg(feature = "mtl-e2e")]
    fn draw_canvas(
        device: &MetalDevice,
        format: Format,
        paint: impl FnOnce(&mut dyn CommandEncoder),
    ) -> Vec<u8> {
        draw_canvas_over(device, format, None, paint)
    }

    /// [`draw_canvas`], with `depth` deciding whether the pass carries a
    /// depth attachment of that format as well as the colour one.
    ///
    /// The two exist as one function because the depth attachment is the
    /// **only** difference between the halves of
    /// [`a_device_says_whether_it_clamps_depth_without_a_depth_attachment`]'s
    /// comparison: same canvas, same clear, same submission and same readback,
    /// so a difference in what comes back is a difference the attachment made.
    #[cfg(feature = "mtl-e2e")]
    fn draw_canvas_over(
        device: &MetalDevice,
        format: Format,
        depth: Option<Format>,
        paint: impl FnOnce(&mut dyn CommandEncoder),
    ) -> Vec<u8> {
        let depth_target = depth.map(|depth_format| {
            let image = device
                .create_image(&ImageDesc {
                    label: Some("crcbl-mtl canvas depth"),
                    image_type: ImageType::D2,
                    extent: CANVAS,
                    format: depth_format,
                    mip_levels: 1,
                    samples: 1,
                    usage: ImageUsage::DEPTH_STENCIL_ATTACHMENT,
                })
                .expect("a depth attachment");
            let view = device
                .create_image_view(&ImageViewDesc {
                    label: Some("crcbl-mtl canvas depth view"),
                    image,
                    view_type: ImageViewType::D2,
                    format: depth_format,
                    range: ImageSubresourceRange::all(depth_format),
                })
                .expect("a whole-image depth view");
            (image, view)
        });
        let (image, view) = color_target_of(device, CANVAS, format);
        let readback = readback_buffer(device, CANVAS_BYTES as u64);
        let queue = device
            .queue(QueueKind::Graphics)
            .expect("the graphics queue exists");
        let mut encoder = device.create_command_encoder(&CommandEncoderDesc {
            label: Some("crcbl-mtl canvas"),
            queue,
        });
        encoder.begin_render_pass(&RenderPassDesc {
            label: Some("canvas"),
            color_attachments: &[ColorAttachment {
                view,
                resolve: None,
                load: LoadOp::Clear,
                store: StoreOp::Store,
                clear: ClearValue::color(CLEAR),
            }],
            // Cleared and discarded, and the pipeline half of the pair tests
            // `Always` with writes off — so the attachment's *contents* can
            // never be why a fragment did or did not land, and the only thing
            // its presence changes is what Metal's clip mode is applied to.
            depth_stencil_attachment: depth_target.map(|(_, view)| DepthStencilAttachment {
                view,
                read_only: false,
                depth_load: LoadOp::Clear,
                depth_store: StoreOp::Discard,
                stencil_load: LoadOp::Clear,
                stencil_store: StoreOp::Discard,
                clear: ClearValue::default(),
            }),
            render_area: Rect2d::from_size(CANVAS.width, CANVAS.height),
        });
        encoder.set_viewport(&Viewport::from_size(CANVAS.width, CANVAS.height));
        encoder.set_scissor(&Rect2d::from_size(CANVAS.width, CANVAS.height));
        paint(encoder.as_mut());
        encoder.end_render_pass();
        encoder.pipeline_barrier(&Barriers {
            images: &[ImageBarrier::new(
                image,
                ImageSubresourceRange::all(format),
                ResourceState::ColorAttachment,
                ResourceState::TransferSrc,
            )],
            ..Barriers::default()
        });
        encoder.copy_image_to_buffer(&whole_image_copy_of(image, readback, CANVAS));
        let commands = encoder.finish().expect("the recording is complete");
        device
            .submit(queue, &SubmitInfo::new(&[commands]))
            .expect("the queue accepts it");
        let request = device
            .request_readback(&ReadbackDesc {
                label: Some("the canvas"),
                buffer: readback,
                offset: 0,
                size: CANVAS_BYTES as u64,
                after: None,
            })
            .expect("a HostReadback buffer, in range");
        let bytes = drain(device, request, CANVAS_BYTES);

        device.destroy_readback(request);
        device.destroy_command_buffer(commands);
        device.destroy_image_view(view);
        device.destroy_image(image);
        if let Some((depth_image, depth_view)) = depth_target {
            device.destroy_image_view(depth_view);
            device.destroy_image(depth_image);
        }
        device.destroy_buffer(readback);
        bytes
    }

    /// An RGBA-ordered texel as a `format` attachment stores it.
    ///
    /// [`CLEAR_TEXEL`] and [`INK_TEXEL`] are written in R, G, B, A order
    /// because that is the order [`Format::Rgba8Unorm`] lays them out in
    /// memory; a [`Format::Bgra8Unorm`] readback of the same colour comes back
    /// B, G, R, A. Reordering here rather than relaxing the comparison is what
    /// keeps the assertion able to catch a channel swizzle: both texels are
    /// asymmetric in every channel, so the expected bytes for one format are
    /// wrong for the other.
    ///
    /// Panics on any other format rather than passing one through, because a
    /// silent identity for a format whose layout is neither of these two would
    /// be an assertion that no longer knows what it is comparing.
    #[cfg(feature = "mtl-e2e")]
    fn texel_in(format: Format, rgba: [u8; 4]) -> [u8; 4] {
        match format {
            Format::Rgba8Unorm => rgba,
            Format::Bgra8Unorm => [rgba[2], rgba[1], rgba[0], rgba[3]],
            other => panic!("{other:?} has no RGBA byte reordering defined here"),
        }
    }

    /// The assertions every gated draw makes about a [`CANVAS`] readback in
    /// `format`: the centre is the triangle's colour, all four corners are the
    /// clear's, and nothing else is anywhere.
    ///
    /// Taking the format means the comparison is against the exact bytes that
    /// attachment must hold, so it proves the channel *order* as well as the
    /// coverage — see [`texel_in`].
    #[cfg(feature = "mtl-e2e")]
    fn assert_ink_triangle(bytes: &[u8], format: Format) {
        assert_eq!(bytes.len(), CANVAS_BYTES, "the readback is the wrong size");
        let ink = texel_in(format, INK_TEXEL);
        let clear = texel_in(format, CLEAR_TEXEL);
        let (centre_x, centre_y) = (CANVAS.width / 2, CANVAS.height / 2);
        assert_eq!(
            texel_at(bytes, centre_x, centre_y),
            ink,
            "the centre of the image is not the triangle's colour, so nothing was drawn"
        );
        let last = (CANVAS.width - 1, CANVAS.height - 1);
        for (x, y) in [(0, 0), (last.0, 0), (0, last.1), (last.0, last.1)] {
            assert_eq!(
                texel_at(bytes, x, y),
                clear,
                "corner ({x}, {y}) is not the clear colour, so the draw covered the whole target \
                 rather than a triangle"
            );
        }
        // And nothing else is in the image at all: with one sample per pixel
        // and no blending, every texel is exactly one of the two colours. This
        // is what rules out the poison pattern surviving anywhere, and any
        // stray content the point checks would step over.
        for (index, texel) in bytes.chunks_exact(4).enumerate() {
            assert!(
                texel == ink || texel == clear,
                "texel {index} is {texel:02X?}, which is neither the triangle nor the clear colour"
            );
        }
    }
}
