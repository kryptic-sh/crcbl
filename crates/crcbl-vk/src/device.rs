//! [`VkDevice`]: the logical device, its queues, its object tables and the
//! frame loop's submit/acquire/present path.
//!
//! # One lock, and why that is enough *today*
//!
//! The seam takes `&self` everywhere so a device can be shared behind an `Arc`,
//! which means a backend owes its own interior synchronisation. This one uses a
//! single [`Mutex`] over every object table plus the queue. That is the coarsest
//! possible answer and it is deliberate for P1.1: the call rate through it is a
//! few dozen operations per frame, and a lock-per-table scheme has a
//! deadlock-ordering problem to design before it has a contention problem to
//! solve. P8's job system uploading from worker threads is the point at which
//! that stops being true, and splitting the lock is a change confined to this
//! file.
//!
//! # Everything a driver owns is retired, never freed inline
//!
//! `destroy_*` invalidates the handle immediately and parks the driver object in
//! [`RetireQueue`], keyed on the submission
//! counter. The one exception is [`Device::destroy_command_buffer`], which the
//! seam explicitly says "must not be called until the submission that used it
//! has completed" — so the caller has already done the waiting and the pool is
//! freed inline.
//!
//! # Where the rest of the `Device` surface lives
//!
//! Shader modules, descriptor layouts, bind groups, samplers and pipelines
//! landed at P1.2 and their bodies are in [`crate::pipeline`]; the `Device` impl
//! here forwards to them so it stays a readable index of the seam. Everything
//! else — resources, queries, sync, submission, presentation — is in this file.

use core::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex, MutexGuard, Once};
use std::time::Duration;

use ash::vk::Handle as _;
use ash::{ext, khr, vk};

use crcbl_core::{Handle, Pool};
use crcbl_hal::{
    AcquiredFrame, BackendKind, BindGroupDesc, BindGroupEntry, BindGroupHandle,
    BindGroupLayoutDesc, BindGroupLayoutHandle, BufferDesc, BufferHandle, Capability,
    CommandBufferHandle, CommandEncoder, CommandEncoderDesc, ComputePipelineDesc,
    ComputePipelineHandle, Device, DeviceCaps, DeviceDesc, DisplayTiming, Features, Format,
    GraphicsPipelineDesc, GraphicsPipelineHandle, HalError, ImageDesc, ImageHandle, ImageViewDesc,
    ImageViewHandle, MemoryLocation, PipelineLayoutDesc, PipelineLayoutHandle, PresentInfo,
    QueryKind, QuerySetDesc, QuerySetHandle, QueueHandle, QueueKind, ReadbackDesc, ReadbackHandle,
    ReadbackState, SamplerDesc, SamplerHandle, SemaphoreDesc, SemaphoreHandle, SemaphoreKind,
    SemaphoreWait, ShaderModuleDesc, ShaderModuleHandle, SubmitInfo, Support, SurfaceError,
    SwapchainDesc, SwapchainHandle,
};

use crate::adapter::AdapterRecord;
use crate::command::VkCommandEncoder;
use crate::conv;
use crate::deletion::RetireQueue;
use crate::instance::{InstanceInner, next_owner_id};
use crate::mem::{self, MemoryRequest};
use crate::pipeline::{
    BindGroupEntryRecord, BindGroupLayoutEntryRecord, PipelineEntry, PipelineLayoutEntry,
    SamplerEntry, ShaderModuleEntry,
};
use crate::present_timing;
use crate::swapchain::{self, FrameSync, SwapchainEntry};

/// Anything the object tables hold, so one lookup helper serves them all.
pub(crate) trait Owned {
    /// Id of the device that created it, per `crcbl-hal`'s obligation 3.
    fn owner(&self) -> u64;
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

/// A buffer and its dedicated allocation.
#[derive(Debug)]
pub(crate) struct BufferEntry {
    pub(crate) owner: u64,
    pub(crate) raw: vk::Buffer,
    memory: vk::DeviceMemory,
    size: u64,
    location: MemoryLocation,
    /// Persistently mapped for the two host-visible locations, null otherwise.
    /// Mapping once at creation beats a map/unmap pair per write: Vulkan allows
    /// only one mapping at a time per allocation, and the staging ring writes
    /// every frame.
    mapped: *mut u8,
}

/// An image, plus the allocation if this backend made it.
#[derive(Debug)]
pub(crate) struct ImageEntry {
    pub(crate) owner: u64,
    pub(crate) raw: vk::Image,
    /// Null for a swapchain image, which the driver owns.
    memory: vk::DeviceMemory,
    pub(crate) format: Format,
    /// Whether a swapchain owns this image. Destroying such a handle drops the
    /// name without touching the `VkImage`.
    swapchain_owned: bool,
    /// Whether this image is an **offscreen ring** image, whose reuse nothing
    /// else orders.
    ///
    /// A WSI image arrives with an acquire semaphore, and the wait on it is the
    /// execution dependency separating the frame that is about to write the
    /// image from whatever last read it. An offscreen ring has no semaphore to
    /// hand out — the seam calls its acquire implicit — so that dependency has
    /// to come from somewhere, and the only thing on the queue at the right
    /// moment is the frame's own opening barrier. See
    /// [`super::command`]'s `pipeline_barrier`, which widens it.
    pub(crate) ring_reuse: bool,
}

/// An image view.
#[derive(Debug)]
pub(crate) struct ViewEntry {
    pub(crate) owner: u64,
    pub(crate) raw: vk::ImageView,
    /// The format the view reinterprets its image as.
    ///
    /// Kept because `begin_render_pass` needs it: whether a depth attachment
    /// gets a `pStencilAttachment` is a property of the **format**, not of the
    /// stencil ops the caller happened to set, and passing one for a depth-only
    /// view is a VU violation.
    pub(crate) format: Format,
    /// Whether a swapchain owns it. The seam hands these out through
    /// [`AcquiredFrame::view`] and nothing above ever destroys one — exactly
    /// the rule the acquire/present semaphores already follow.
    swapchain_owned: bool,
}

/// A semaphore, timeline or binary.
#[derive(Debug)]
pub(crate) struct SemaphoreEntry {
    pub(crate) owner: u64,
    pub(crate) raw: vk::Semaphore,
    pub(crate) timeline: bool,
    /// Whether a swapchain owns it. The seam hands these out through
    /// [`AcquiredFrame`] and nothing above ever destroys one.
    swapchain_owned: bool,
}

/// A query pool.
#[derive(Debug)]
pub(crate) struct QuerySetEntry {
    pub(crate) owner: u64,
    pub(crate) raw: vk::QueryPool,
    pub(crate) count: u32,
    pub(crate) kind: QueryKind,
}

/// A recorded command buffer and the pool it came from.
#[derive(Debug)]
pub(crate) struct CommandBufferEntry {
    pub(crate) owner: u64,
    pub(crate) raw: vk::CommandBuffer,
    pub(crate) pool: vk::CommandPool,
    /// Raw device objects (buffers, images, views, semaphores, pools, pipelines,
    /// layouts, samplers) the recorded commands use, collected at record time. A
    /// submission extends the deletion-queue retirement of any of these that are
    /// parked, so an object destroyed after recording stays alive until the last
    /// submission referencing it completes.
    pub(crate) references: Vec<u64>,
    /// Whether this command buffer has ever been handed to a queue.
    ///
    /// Until it has, its [`references`](Self::references) are referenced by work
    /// no timeline value covers, so [`DeviceInner::poll_retire`] treats them as
    /// held and refuses to free them however far the timeline has run. `submit`
    /// sets this once the driver has taken the submission, which is the moment
    /// the retire timeline starts describing this recording instead.
    pub(crate) submitted: bool,
}

/// An in-flight readback request.
#[derive(Debug)]
pub(crate) struct ReadbackEntry {
    pub(crate) owner: u64,
    /// The buffer being read back, stored as a handle so it can be re-resolved
    /// at poll time. If the buffer was destroyed between request and poll, the
    /// generational handle fails lookup rather than dereferencing unmapped
    /// memory.
    buffer: BufferHandle,
    offset: u64,
    size: u64,
    /// The completion point to watch: the generational handle of an explicit
    /// timeline wait when the caller named one, otherwise `None` for this
    /// device's own retire timeline — device-owned and never destroyed — plus
    /// the value to wait for, snapshotted at request time. The handle is
    /// re-resolved at poll time like the buffer, so a destroyed semaphore fails
    /// lookup instead of dereferencing a dead `VkSemaphore`.
    wait_semaphore: Option<SemaphoreHandle>,
    wait_value: u64,
}

owned!(
    BufferEntry,
    ImageEntry,
    ViewEntry,
    SemaphoreEntry,
    QuerySetEntry,
    CommandBufferEntry,
    ReadbackEntry,
    SwapchainEntry,
    ShaderModuleEntry,
    BindGroupLayoutEntryRecord,
    BindGroupEntryRecord,
    PipelineLayoutEntry,
    PipelineEntry,
    SamplerEntry,
);

/// A driver object parked until the GPU is done with it.
#[derive(Debug)]
pub(crate) enum Trash {
    Buffer(vk::Buffer, vk::DeviceMemory),
    Image(vk::Image, vk::DeviceMemory),
    ImageView(vk::ImageView),
    Semaphore(vk::Semaphore),
    QueryPool(vk::QueryPool),
    ShaderModule(vk::ShaderModule),
    DescriptorSetLayout(vk::DescriptorSetLayout),
    /// A pool and, implicitly, the one set allocated from it — see
    /// `pipeline.rs` on why a bind group owns a whole pool.
    DescriptorPool(vk::DescriptorPool),
    PipelineLayout(vk::PipelineLayout),
    Pipeline(vk::Pipeline),
    Sampler(vk::Sampler),
    /// A whole swapchain, including the surface reference it holds. The surface
    /// release is what lets `Instance::destroy_surface` be honoured lazily —
    /// obligation 2.
    ///
    /// **Never parked in the [`RetireQueue`].** It is a `Trash` variant only so
    /// that one `destroy_trash` covers every kind of object this device owns,
    /// including the teardown sweeps. [`DeviceInner::retire_swapchain`] builds
    /// one and destroys it immediately, after idling, because the queue is keyed
    /// on the submission timeline and `vkQueuePresentKHR` is queue work no
    /// timeline semaphore signals — see that method.
    ///
    /// Boxed anyway: it is by far the largest variant, so inlining it would pad
    /// every buffer and view that *is* parked up to its size.
    Swapchain(Box<TrashSwapchain>),
}

/// The raw handle value this parked object carries, for matching a command
/// buffer's recorded references. `Swapchain` is never parked in the queue
/// (see `retire_swapchain`), so it returns a sentinel that matches nothing.
fn trash_raw(item: &Trash) -> u64 {
    match item {
        Trash::Buffer(buffer, _) => buffer.as_raw(),
        Trash::Image(image, _) => image.as_raw(),
        Trash::ImageView(view) => view.as_raw(),
        Trash::Semaphore(semaphore) => semaphore.as_raw(),
        Trash::QueryPool(pool) => pool.as_raw(),
        Trash::ShaderModule(module) => module.as_raw(),
        Trash::DescriptorSetLayout(layout) => layout.as_raw(),
        Trash::DescriptorPool(pool) => pool.as_raw(),
        Trash::PipelineLayout(layout) => layout.as_raw(),
        Trash::Pipeline(pipeline) => pipeline.as_raw(),
        Trash::Sampler(sampler) => sampler.as_raw(),
        Trash::Swapchain(_) => u64::MAX,
    }
}

/// Everything a retired swapchain owns.
#[derive(Debug)]
pub(crate) struct TrashSwapchain {
    swapchain: vk::SwapchainKHR,
    surface: vk::SurfaceKHR,
    images: Vec<vk::Image>,
    /// Always ours, even when the images are the driver's.
    views: Vec<vk::ImageView>,
    memory: Vec<vk::DeviceMemory>,
    sync: Option<FrameSync>,
}

/// Every table the device owns, behind one lock.
#[derive(Debug, Default)]
pub(crate) struct DeviceState {
    buffers: Pool<BufferEntry>,
    images: Pool<ImageEntry>,
    views: Pool<ViewEntry>,
    semaphores: Pool<SemaphoreEntry>,
    query_sets: Pool<QuerySetEntry>,
    command_buffers: Pool<CommandBufferEntry>,
    readbacks: Pool<ReadbackEntry>,
    swapchains: Pool<SwapchainEntry>,
    // Milestone 2's tables. `pub(crate)` because `pipeline.rs` owns their
    // creation logic; the older fields stay private because everything that
    // touches them lives in this file.
    pub(crate) shader_modules: Pool<ShaderModuleEntry>,
    pub(crate) bind_group_layouts: Pool<BindGroupLayoutEntryRecord>,
    pub(crate) bind_groups: Pool<BindGroupEntryRecord>,
    pub(crate) pipeline_layouts: Pool<PipelineLayoutEntry>,
    pub(crate) pipelines: Pool<PipelineEntry>,
    pub(crate) samplers: Pool<SamplerEntry>,
    trash: RetireQueue<Trash>,
}

impl DeviceState {
    /// The command-buffer table, for [`CommandEncoder::finish`] to register in.
    pub(crate) fn command_buffers_mut(&mut self) -> &mut Pool<CommandBufferEntry> {
        &mut self.command_buffers
    }
}

/// One real queue behind a [`QueueKind`].
#[derive(Clone, Copy, Debug)]
pub(crate) struct QueueSlot {
    pub(crate) family: u32,
    pub(crate) raw: vk::Queue,
}

/// The device's shared state, held by the public device *and* by every command
/// encoder it hands out.
pub(crate) struct DeviceInner {
    /// Obligation 1: a `Device` may outlive its `Instance`, so the instance's
    /// state is kept alive here rather than borrowed.
    pub(crate) instance: Arc<InstanceInner>,
    pub(crate) raw: ash::Device,
    pub(crate) physical: vk::PhysicalDevice,
    pub(crate) swapchain_ext: khr::swapchain::Device,
    /// `Some` when `VK_KHR_present_id` and `VK_KHR_present_wait` were both
    /// enabled — that is, when this device advertises
    /// [`Features::PRESENT_FEEDBACK`]. `None` is what makes
    /// [`Device::wait_until_presented`] the immediate `Ok(())` the seam
    /// documents, and what keeps `VkPresentIdKHR` off the present chain.
    pub(crate) present_wait_ext: Option<khr::present_wait::Device>,
    /// `Some` when the whole `VK_EXT_present_timing` chain was enabled and its
    /// one entry point resolved — that is, when this device advertises
    /// [`Features::PRESENT_TIMING`]. `None` is what makes
    /// [`Device::display_timing`] the `Ok(DisplayTiming::Unknown)` the seam
    /// documents.
    pub(crate) present_timing_ext: Option<present_timing::Device>,
    /// `Some` when `VK_EXT_mesh_shader` was enabled — that is, when this device
    /// advertises [`Features::MESH_SHADER`]. `vkCmdDrawMeshTasksEXT` lives in
    /// this table and nowhere else, so `None` is what makes a mesh dispatch on
    /// a device without the extension impossible rather than a call through an
    /// unresolved pointer; `create_mesh_pipeline` has already refused, so
    /// reaching a dispatch with `None` here means a pipeline was bound that
    /// this device never created.
    pub(crate) mesh_shader_ext: Option<ext::mesh_shader::Device>,
    /// Says, once, what the display turned out to be doing.
    ///
    /// The same idiom and the same argument as [`first_present_wait`](Self::first_present_wait):
    /// a device can negotiate the whole extension chain and never be asked, and
    /// nothing else in a log distinguishes that from a query that ran and
    /// answered. Carries the resolved [`DisplayTiming`] because the arm is the
    /// fact worth having — "present timing enabled" is already said at open,
    /// and it is the *answer* that no amount of frame timing reveals.
    first_display_timing: Once,
    /// Says, once, that a wait actually reached the driver.
    ///
    /// A second fact from the line `open` logs, and not one that can be
    /// inferred from it: a device can negotiate both extensions and then never
    /// wait on anything — which is what this backend did before the wait was
    /// implemented — and **the difference is invisible in a frame time**,
    /// because `vkQueuePresentKHR` in FIFO already paces the loop to the
    /// display on its own. So the loop being genuinely closed is something
    /// only the backend can report, and this is where it reports it.
    first_present_wait: Once,
    pub(crate) debug_ext: Option<ext::debug_utils::Device>,
    pub(crate) caps: DeviceCaps,
    /// `VkPhysicalDeviceLimits::timestampPeriod`, the nanoseconds one tick of
    /// this device's clock is worth. `0.0` without
    /// [`Features::TIMESTAMP_QUERY`], where there is no clock to describe.
    ///
    /// Kept here rather than in [`DeviceInner::caps`] because the seam's
    /// [`Limits`](crcbl_hal::Limits) has no field for it and should not: it is
    /// Vulkan's way of describing a timestamp, D3D12 describes the same thing
    /// as a frequency, and Metal and WebGPU have no such number at all.
    /// [`Device::query_results`] spends it
    /// here and reports nanoseconds, which is a unit every backend has.
    timestamp_period_ns: f32,
    pub(crate) id: u64,
    /// This device's stamp on every handle it issues. See the handle-tagging
    /// section above; never zero.
    tag: u32,
    memory_properties: vk::PhysicalDeviceMemoryProperties,
    queues: [Option<QueueSlot>; 3],
    pub(crate) state: Mutex<DeviceState>,
    /// Signalled by every submission with a strictly increasing value. The
    /// deletion queue's clock, and the default completion point a readback
    /// without an explicit wait watches.
    retire_timeline: vk::Semaphore,
    submissions: AtomicU64,
}

// SAFETY: `ash::Device` and the extension tables are `Send + Sync`; the object
// tables are behind a `Mutex`; the two raw pointers inside `BufferEntry` and
// `ReadbackEntry` are persistent Vulkan mappings, which are valid from any
// thread and are only ever reached through that same `Mutex`.
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
            .field("submissions", &self.submissions.load(Ordering::Relaxed))
            .finish_non_exhaustive()
    }
}

// --- handle tagging --------------------------------------------------------
//
// **Every object table in this backend is per-owner** — per device here, per
// instance for surfaces — and every insert stamps `owner: self.id`. That made
// `entry.owner() != owner` unreachable and `HalError::ForeignObject`
// unproducible: obligation 3 was met only by accident, because owner B's handle
// *usually* failed to resolve in owner A's pool. Usually is not a guarantee —
// two of them allocating in step reach the same slot index at the same
// generation almost immediately, and from that moment A silently accepts B's
// handle and writes, or destroys, its own unrelated object.
//
// So the handle carries the owner that issued it. The top byte of the index
// half is the owner's tag; the rest is the pool's own index, restored before any
// lookup. The generation half is left alone, so `Pool`'s generation-exhaustion
// rule is untouched.
//
// These primitives are `pub(crate)` because `instance.rs` applies the same
// scheme to `SurfaceHandle`, where two `VkInstance`s have exactly the colliding
// pools described above.

/// Bits of a handle's index half given over to the owning object's tag.
pub(crate) const OWNER_TAG_SHIFT: u32 = 24;
/// The part of a handle's index half that is the pool's own index.
pub(crate) const POOL_INDEX_MASK: u32 = (1 << OWNER_TAG_SHIFT) - 1;
/// How many distinct owner tags exist. Tag `0` is reserved for "nobody", so a
/// hand-made or un-stamped handle is foreign to every owner.
const OWNER_TAG_COUNT: u64 = (u32::MAX >> OWNER_TAG_SHIFT) as u64;

/// The tag an owner with this id stamps into its handles. Never zero.
pub(crate) fn owner_tag(id: u64) -> u32 {
    #[allow(clippy::cast_possible_truncation)]
    {
        1 + (id % OWNER_TAG_COUNT) as u32
    }
}

/// The owner tag a handle carries, or `0` if it carries none.
pub(crate) const fn handle_tag<M>(handle: Handle<M>) -> u32 {
    handle.index() >> OWNER_TAG_SHIFT
}

/// Strips the owner tag, recovering the pool's own handle.
pub(crate) fn untag<A, B>(handle: Handle<A>) -> Handle<B> {
    Handle::from_bits(
        (u64::from(handle.generation()) << 32) | u64::from(handle.index() & POOL_INDEX_MASK),
    )
    .unwrap_or_else(|| unreachable!("a handle's generation is never zero"))
}

/// Stamps `tag` into a handle a pool just issued.
///
/// A pool index too large to carry the tag gets tag `0` instead, which resolves
/// nowhere — the object leaks until teardown, which is far better than a handle
/// that might resolve to another owner's object. Reaching it takes
/// [`POOL_INDEX_MASK`] live objects of one kind on one owner.
pub(crate) fn stamp<A, B>(tag: u32, handle: Handle<A>, what: &str) -> Handle<B> {
    let index = handle.index();
    let tag = if index > POOL_INDEX_MASK {
        crcbl_core::log::error!(
            "crcbl-vk: {what} pool index {index} is too large to carry an owner tag; issuing a \
             handle that resolves nowhere rather than one that might resolve to another owner's \
             object"
        );
        0
    } else {
        tag
    };
    Handle::from_bits(
        (u64::from(handle.generation()) << 32)
            | u64::from((index & POOL_INDEX_MASK) | (tag << OWNER_TAG_SHIFT)),
    )
    .unwrap_or_else(|| unreachable!("a pool handle's generation is never zero"))
}

/// Deterministic queue handles.
///
/// Queues are not pooled: they live for the device's lifetime and carry no
/// state a caller can hold, so [`Device::queue`] stays a pure function. The
/// device's tag rides in the same place it does on a pooled handle, because
/// obligation 3 covers queues too — a `QueueHandle` synthesised from the kind
/// index alone carried no device identity at all, so every device accepted
/// every other device's.
fn queue_handle(tag: u32, kind: QueueKind) -> QueueHandle {
    Handle::from_bits((1u64 << 32) | u64::from((tag << OWNER_TAG_SHIFT) | queue_index(kind)))
        .unwrap_or_else(|| unreachable!("generation 1 is non-zero"))
}

const fn queue_index(kind: QueueKind) -> u32 {
    match kind {
        QueueKind::Graphics => 0,
        QueueKind::Compute => 1,
        QueueKind::Transfer => 2,
    }
}

/// Adds the capabilities a granted one cannot be enabled without.
///
/// [`adapter::features_of`](crate::adapter::features_of) never reports a ray
/// capability without the acceleration structure it traverses, nor the task
/// stage without the mesh stage it feeds, so every flag this can add is one the
/// adapter already has. What loses them is the *intersection with the caller's
/// request*: ask for `RAY_QUERY` and not `ACCELERATION_STRUCTURE` and the
/// granted set carries a dependency it cannot stand on.
///
/// Vulkan will not infer them either — `VK_KHR_ray_query` declares
/// `VK_KHR_acceleration_structure` as a dependency, and enabling the dependent
/// extension without its dependency is invalid usage. Restoring them here
/// rather than at the enable site is what keeps [`DeviceCaps`] a description of
/// the device that was actually created: a device reporting `RAY_QUERY` and not
/// `ACCELERATION_STRUCTURE` would be describing one that cannot exist.
#[must_use]
fn granted_with_dependencies(granted: Features) -> Features {
    let mut granted = granted;
    if granted.contains(Features::TASK_SHADER) {
        granted |= Features::MESH_SHADER;
    }
    if granted.intersects(Features::RAY_QUERY | Features::RAY_TRACING_PIPELINE) {
        granted |= Features::ACCELERATION_STRUCTURE;
    }
    granted
}

/// The Vulkan implementation of [`Device`].
#[derive(Debug)]
pub struct VkDevice {
    inner: Arc<DeviceInner>,
}

impl VkDevice {
    /// The shared state, for the sibling modules that implement parts of the
    /// `Device` surface — `pipeline.rs` owns everything from
    /// `create_shader_module` down.
    pub(crate) fn inner(&self) -> &Arc<DeviceInner> {
        &self.inner
    }

    /// Removes a handle this device owns and parks its driver object.
    ///
    /// Eleven `destroy_*` bodies differed only in which pool they name and
    /// which [`Trash`] variant they build, and all eleven had the same bug:
    /// `pool.remove` first, `entry.owner != id` afterwards. By then the row was
    /// gone and the entry dropped, so the driver object leaked — and a foreign
    /// handle that happened to resolve destroyed this device's own unrelated
    /// object. One place to get the order right is one place to keep it right.
    fn retire_from<E: Owned, M>(
        &self,
        pool: impl FnOnce(&mut DeviceState) -> &mut Pool<E>,
        handle: Handle<M>,
        to_trash: impl FnOnce(E) -> Trash,
    ) {
        let mut state = self.inner.state();
        let Some(entry) = take_owned(pool(&mut state), handle, &self.inner) else {
            return;
        };
        self.inner.park(&mut state, to_trash(entry));
    }

    /// Retires a graphics or compute pipeline.
    ///
    /// One body for both, because `GraphicsPipeline` and `ComputePipeline` are
    /// distinct marker types above the seam and the same `VkPipeline` below it.
    fn destroy_pipeline_handle(&self, pipeline: Handle<crcbl_hal::GraphicsPipeline>) {
        self.retire_from(
            |state| &mut state.pipelines,
            pipeline,
            |entry| Trash::Pipeline(entry.raw),
        );
    }

    pub(crate) fn open(
        instance: Arc<InstanceInner>,
        record: &AdapterRecord,
        desc: &DeviceDesc<'_>,
        present_surface: Option<vk::SurfaceKHR>,
    ) -> Result<Self, HalError> {
        if !record.core_1_3.is_complete() {
            // `docs/plan/02-vulkan-backend.md`: "No fallback paths for missing
            // features in MVP. If the device lacks them, error clearly and
            // exit."
            return Err(HalError::Backend(format!(
                "adapter {:?} lacks Vulkan 1.3 {:?}, which crcbl-vk requires",
                record.info.name,
                record.core_1_3.missing()
            )));
        }
        let missing = record.info.caps.missing(desc.required_features);
        if !missing.is_empty() {
            return Err(HalError::UnsupportedFeatures { missing });
        }
        let Some(graphics_family) = record.families.graphics else {
            return Err(HalError::Backend(format!(
                "adapter {:?} has no graphics+compute queue family",
                record.info.name
            )));
        };

        // Vulkan wants a present-capable family chosen at device-creation time,
        // which is why `DeviceDesc` carries `compatible_surface` at all. A
        // graphics family that cannot present is real (multi-GPU laptops), and
        // finding out at swapchain creation instead would be far too late.
        if let Some(surface) = present_surface.filter(|raw| *raw != vk::SurfaceKHR::null())
            && let Some(surface_ext) = instance.surface_ext.as_ref()
        {
            // SAFETY: `record.physical` and `surface` both come from
            // `instance`, which is live.
            let supported = unsafe {
                surface_ext.get_physical_device_surface_support(
                    record.physical,
                    graphics_family,
                    surface,
                )
            }
            .map_err(|error| conv::hal_error("vkGetPhysicalDeviceSurfaceSupportKHR", error))?;
            if !supported {
                return Err(HalError::Backend(format!(
                    "adapter {:?}'s graphics queue family {graphics_family} cannot present \
                     to this surface",
                    record.info.name
                )));
            }
        }

        // Only features the adapter actually has, intersected with what the
        // caller asked for. Enabling a feature a device lacks fails
        // `vkCreateDevice`; enabling one nobody asked for is a cost with no
        // caller.
        let granted = record
            .info
            .caps
            .features
            .intersection(desc.required_features.union(desc.optional_features))
            .union(desc.required_features);
        // …plus whatever those cannot be enabled without, which the
        // intersection above is exactly where a caller loses.
        let granted = granted_with_dependencies(granted);

        let priorities = [1.0_f32];
        let queue_infos: Vec<vk::DeviceQueueCreateInfo<'_>> = record
            .families
            .distinct()
            .into_iter()
            .map(|family| {
                vk::DeviceQueueCreateInfo::default()
                    .queue_family_index(family)
                    .queue_priorities(&priorities)
            })
            .collect();

        let core_features = vk::PhysicalDeviceFeatures::default()
            .multi_draw_indirect(granted.contains(Features::MULTI_DRAW_INDIRECT))
            .draw_indirect_first_instance(granted.contains(Features::INDIRECT_FIRST_INSTANCE))
            .pipeline_statistics_query(granted.contains(Features::PIPELINE_STATISTICS_QUERY))
            .occlusion_query_precise(granted.contains(Features::OCCLUSION_QUERY))
            .depth_clamp(granted.contains(Features::DEPTH_CLAMP))
            .depth_bias_clamp(granted.contains(Features::DEPTH_BIAS_CLAMP))
            .fill_mode_non_solid(granted.contains(Features::POLYGON_MODE_LINE))
            .texture_compression_bc(granted.contains(Features::TEXTURE_COMPRESSION_BC))
            .sampler_anisotropy(granted.contains(Features::SAMPLER_ANISOTROPY));

        let bindless = granted.contains(Features::DESCRIPTOR_INDEXING);
        let mut vulkan_1_2 = vk::PhysicalDeviceVulkan12Features::default()
            .descriptor_indexing(bindless)
            .runtime_descriptor_array(bindless)
            .descriptor_binding_partially_bound(bindless)
            .descriptor_binding_sampled_image_update_after_bind(bindless)
            .descriptor_binding_storage_buffer_update_after_bind(bindless)
            .descriptor_binding_variable_descriptor_count(bindless)
            .shader_sampled_image_array_non_uniform_indexing(bindless)
            .shader_storage_buffer_array_non_uniform_indexing(bindless)
            .buffer_device_address(granted.contains(Features::BUFFER_DEVICE_ADDRESS))
            .draw_indirect_count(granted.contains(Features::DRAW_INDIRECT_COUNT))
            // Not optional: the seam's whole sync model is timeline semaphores,
            // and the device floor already requires Vulkan 1.3.
            .timeline_semaphore(true);
        // Not optional, and not a seam `Feature`: `SV_VertexID` in Slang lowers
        // to `gl_VertexIndex - gl_BaseVertex`, which declares the SPIR-V
        // `DrawParameters` capability, and **every** vertex-pulling shader this
        // engine has uses it. It is Vulkan 1.1 core-optional and present on
        // radv, lavapipe and every desktop driver, so it joins `dynamicRendering`
        // and friends in this backend's floor rather than becoming a capability
        // the renderer would have to branch on. `AdapterRecord::core_1_3` is
        // where the floor is checked; this feature is checked by
        // `vkCreateDevice` failing, which is loud enough for something no
        // supported driver lacks.
        let mut vulkan_1_1 =
            vk::PhysicalDeviceVulkan11Features::default().shader_draw_parameters(true);
        let mut vulkan_1_3 = vk::PhysicalDeviceVulkan13Features::default()
            .dynamic_rendering(true)
            .synchronization2(true)
            .maintenance4(true);

        // `granted` already carries the probe's answer: `adapter::describe`
        // sets `PRESENT_FEEDBACK` only when `vkEnumerateDeviceExtensionProperties`
        // listed **both** extensions and `vkGetPhysicalDeviceFeatures2` returned
        // both feature bits, and `granted` is that intersected with what this
        // caller asked for. Requesting an absent extension fails
        // `vkCreateDevice` outright, so this pair must never be asked for on a
        // guess.
        let present_feedback = granted.contains(Features::PRESENT_FEEDBACK);
        // The same argument, for the four-extension chain `VK_EXT_present_timing`
        // sits at the end of: `adapter::describe` sets `PRESENT_TIMING` only
        // when every member of that chain was listed *and* both feature bits
        // came back true, so this is never a guess either. `VK_KHR_swapchain`
        // is already unconditional below and
        // `VK_KHR_get_surface_capabilities2` is an instance extension the
        // instance enabled, which leaves these two to add here.
        let present_timing = granted.contains(Features::PRESENT_TIMING);
        // The same argument once more for the mesh-shading and ray-tracing
        // extensions: `adapter::describe` reports each of these only when the
        // extension was listed *and* its feature bit came back true, so asking
        // is never a guess — and asking only when the caller wanted the
        // capability is what keeps device creation identical for everyone else.
        // One extension carries both mesh stages, so `MESH_SHADER` alone
        // decides it; `granted_with_dependencies` has already put that flag
        // beside a lone `TASK_SHADER`.
        let mesh_shader = granted.contains(Features::MESH_SHADER);
        // `VK_KHR_acceleration_structure` depends on
        // `VK_KHR_deferred_host_operations`, which is not a seam capability at
        // all — it is the end of a chain, exactly like present timing's
        // `VK_KHR_calibrated_timestamps` above, and an extension chain is not
        // partially satisfiable.
        let acceleration_structure = granted.contains(Features::ACCELERATION_STRUCTURE);
        let ray_query = granted.contains(Features::RAY_QUERY);
        let ray_tracing_pipeline = granted.contains(Features::RAY_TRACING_PIPELINE);
        // A named local, not a temporary in the chain: the builder stores the
        // pointer, so `&[…]` inline would dangle by the time `vkCreateDevice`
        // reads it.
        let mut device_extensions = vec![khr::swapchain::NAME.as_ptr()];
        if present_feedback {
            device_extensions.push(khr::present_id::NAME.as_ptr());
            device_extensions.push(khr::present_wait::NAME.as_ptr());
        }
        if present_timing {
            device_extensions.push(khr::calibrated_timestamps::NAME.as_ptr());
            device_extensions.push(present_timing::PRESENT_ID2_NAME.as_ptr());
            device_extensions.push(present_timing::PRESENT_TIMING_NAME.as_ptr());
        }
        if mesh_shader {
            device_extensions.push(ext::mesh_shader::NAME.as_ptr());
        }
        if acceleration_structure {
            device_extensions.push(khr::deferred_host_operations::NAME.as_ptr());
            device_extensions.push(khr::acceleration_structure::NAME.as_ptr());
        }
        if ray_query {
            device_extensions.push(khr::ray_query::NAME.as_ptr());
        }
        if ray_tracing_pipeline {
            device_extensions.push(khr::ray_tracing_pipeline::NAME.as_ptr());
        }
        // The extension names alone enable nothing: chaining a `VkPresentIdKHR`
        // onto a present without `presentId` granted here is a validation
        // error, and `vkWaitForPresentKHR` without `presentWait` is undefined.
        let mut present_id_features =
            vk::PhysicalDevicePresentIdFeaturesKHR::default().present_id(true);
        let mut present_wait_features =
            vk::PhysicalDevicePresentWaitFeaturesKHR::default().present_wait(true);
        // As above, and with the dependency's `presentId2` asked for alongside
        // the extension's own `presentTiming`: enabling `VK_KHR_present_id2`
        // without granting its feature would leave the chain half-satisfied.
        let mut present_timing_features =
            present_timing::PhysicalDevicePresentTimingFeaturesEXT::enabling_timing();
        let mut present_id2_features =
            present_timing::PhysicalDevicePresentId2FeaturesKHR::enabling_present_id2();
        // And the same for these: the extension name admits the stage, the
        // feature bit is what turns it on. The task stage is asked for
        // separately because the seam reports it separately — a caller wanting
        // only the mesh stage gets only the mesh stage.
        let mut mesh_shader_features = vk::PhysicalDeviceMeshShaderFeaturesEXT::default()
            .mesh_shader(true)
            .task_shader(granted.contains(Features::TASK_SHADER));
        let mut acceleration_features =
            vk::PhysicalDeviceAccelerationStructureFeaturesKHR::default()
                .acceleration_structure(true);
        let mut ray_query_features =
            vk::PhysicalDeviceRayQueryFeaturesKHR::default().ray_query(true);
        let mut ray_pipeline_features =
            vk::PhysicalDeviceRayTracingPipelineFeaturesKHR::default().ray_tracing_pipeline(true);
        let mut create_info = vk::DeviceCreateInfo::default()
            .queue_create_infos(&queue_infos)
            .enabled_features(&core_features)
            .enabled_extension_names(&device_extensions)
            .push_next(&mut vulkan_1_1)
            .push_next(&mut vulkan_1_2)
            .push_next(&mut vulkan_1_3);
        if present_feedback {
            create_info = create_info
                .push_next(&mut present_id_features)
                .push_next(&mut present_wait_features);
        }
        if present_timing {
            create_info = create_info
                .push_next(&mut present_timing_features)
                .push_next(&mut present_id2_features);
        }
        if mesh_shader {
            create_info = create_info.push_next(&mut mesh_shader_features);
        }
        if acceleration_structure {
            create_info = create_info.push_next(&mut acceleration_features);
        }
        if ray_query {
            create_info = create_info.push_next(&mut ray_query_features);
        }
        if ray_tracing_pipeline {
            create_info = create_info.push_next(&mut ray_pipeline_features);
        }

        // SAFETY: `record.physical` came from `instance`; every pointer in
        // `create_info` borrows a local that outlives the call; the requested
        // features were checked against the adapter above.
        let raw = unsafe {
            instance
                .raw
                .create_device(record.physical, &create_info, None)
        }
        .map_err(|error| conv::hal_error("vkCreateDevice", error))?;

        let swapchain_ext = khr::swapchain::Device::new(&instance.raw, &raw);
        // `Some` is the whole record that the capability is live: every present
        // and every wait branches on it rather than re-reading `caps`.
        let present_wait_ext =
            present_feedback.then(|| khr::present_wait::Device::new(&instance.raw, &raw));
        // Flattened, unlike the tables above: `ash` generates its own
        // infallibly, but this one is resolved by hand through
        // `vkGetDeviceProcAddr` and a null answer is possible in principle. It
        // degrades to the same `DisplayTiming::Unknown` a device without the
        // extension gives, rather than to a call through a null pointer.
        let present_timing_ext = present_timing
            .then(|| present_timing::Device::load(&instance.raw, &raw))
            .flatten();
        // Same idiom as `present_wait_ext`: `Some` *is* the record that the
        // extension is live, and every mesh dispatch branches on it rather than
        // re-reading `caps`.
        let mesh_shader_ext =
            mesh_shader.then(|| ext::mesh_shader::Device::new(&instance.raw, &raw));
        let debug_ext = instance
            .debug_ext
            .as_ref()
            .map(|_| ext::debug_utils::Device::new(&instance.raw, &raw));

        let queue_of = |family: Option<u32>| {
            family.map(|family| QueueSlot {
                family,
                // SAFETY: `family` was named in `queue_infos` above with one
                // queue, so index 0 exists.
                raw: unsafe { raw.get_device_queue(family, 0) },
            })
        };
        let queues = [
            queue_of(Some(graphics_family)),
            queue_of(
                record
                    .families
                    .async_compute
                    .filter(|_| granted.contains(Features::ASYNC_COMPUTE_QUEUE)),
            ),
            queue_of(
                record
                    .families
                    .transfer
                    .filter(|_| granted.contains(Features::TRANSFER_QUEUE)),
            ),
        ];

        let mut timeline_info =
            vk::SemaphoreTypeCreateInfo::default().semaphore_type(vk::SemaphoreType::TIMELINE);
        let semaphore_info = vk::SemaphoreCreateInfo::default().push_next(&mut timeline_info);
        // SAFETY: `raw` is a live device with `timelineSemaphore` enabled.
        let retire_timeline = unsafe { raw.create_semaphore(&semaphore_info, None) }
            .map_err(|error| conv::hal_error("vkCreateSemaphore (retire timeline)", error))?;

        // SAFETY: `record.physical` came from `instance`.
        let memory_properties = unsafe {
            instance
                .raw
                .get_physical_device_memory_properties(record.physical)
        };

        let id = next_owner_id();
        let inner = Arc::new(DeviceInner {
            instance,
            raw,
            physical: record.physical,
            swapchain_ext,
            present_wait_ext,
            present_timing_ext,
            mesh_shader_ext,
            first_display_timing: Once::new(),
            first_present_wait: Once::new(),
            debug_ext,
            caps: DeviceCaps {
                features: granted,
                limits: record.info.caps.limits,
            },
            // The adapter's, and zeroed there when the adapter had no
            // timestamps — not re-derived from `granted`, because a caller that
            // declined the optional feature still opened a device whose clock
            // ticks at the rate the adapter reported.
            timestamp_period_ns: record.timestamp_period_ns,
            id,
            tag: owner_tag(id),
            memory_properties,
            queues,
            state: Mutex::new(DeviceState::default()),
            retire_timeline,
            submissions: AtomicU64::new(0),
        });
        inner.set_object_name(inner.raw.handle(), desc.label);
        inner.set_object_name(retire_timeline, Some("crcbl retire timeline"));
        if present_feedback {
            crcbl_core::log::info!(
                "crcbl-vk: present feedback enabled ({} + {})",
                khr::present_id::NAME.to_string_lossy(),
                khr::present_wait::NAME.to_string_lossy(),
            );
        }
        if inner.present_timing_ext.is_some() {
            crcbl_core::log::info!(
                "crcbl-vk: present timing enabled ({} + {})",
                present_timing::PRESENT_TIMING_NAME.to_string_lossy(),
                present_timing::PRESENT_ID2_NAME.to_string_lossy(),
            );
        }
        crcbl_core::log::info!(
            "crcbl-vk: opened {:?} (geometry {:?}, binding {:?}, lighting {:?}), \
             graphics family {graphics_family}, async compute {:?}, transfer {:?}",
            record.info.name,
            inner.caps.geometry_path(),
            inner.caps.binding_model(),
            inner.caps.lighting_path(),
            record.families.async_compute,
            record.families.transfer,
        );
        Ok(Self { inner })
    }
}

impl DeviceInner {
    pub(crate) fn state(&self) -> MutexGuard<'_, DeviceState> {
        self.state
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
    }

    /// Stamps this device's tag into a handle its pools just issued.
    ///
    /// Every handle that crosses the seam goes through here; every handle that
    /// comes back goes through [`local_handle`].
    pub(crate) fn stamp<A, B>(&self, handle: Handle<A>) -> Handle<B> {
        stamp(self.tag, handle, "device")
    }

    /// Attaches a debug name, if the instance has `VK_EXT_debug_utils`.
    ///
    /// `docs/plan/02-vulkan-backend.md` §2.1: "names show up in RenderDoc from
    /// the first triangle onward". Wiring it at every creation site now costs
    /// one line each and is invisible to add later only in the sense that
    /// nobody ever does.
    pub(crate) fn set_object_name<T: vk::Handle>(&self, handle: T, label: Option<&str>) {
        let (Some(debug_ext), Some(label)) = (self.debug_ext.as_ref(), label) else {
            return;
        };
        let Ok(name) = std::ffi::CString::new(label) else {
            return;
        };
        // `object_handle` infers `VkObjectType` from the handle's own type, so
        // a name can never be attached with the wrong type tag.
        let info = vk::DebugUtilsObjectNameInfoEXT::default()
            .object_handle(handle)
            .object_name(&name);
        // SAFETY: `handle` names a live object created from this device, and
        // `name` outlives the call.
        let _ = unsafe { debug_ext.set_debug_utils_object_name(&info) };
    }

    fn queue_slot(&self, queue: QueueHandle) -> Result<QueueSlot, HalError> {
        let tag = handle_tag(queue);
        if tag != self.tag || queue.generation() != 1 {
            return Err(if tag == 0 || queue.generation() != 1 {
                HalError::invalid_handle("queue", queue)
            } else {
                HalError::ForeignObject {
                    kind: "queue",
                    bits: queue.to_bits(),
                }
            });
        }
        self.queues
            .get((queue.index() & POOL_INDEX_MASK) as usize)
            .copied()
            .flatten()
            .ok_or_else(|| HalError::invalid_handle("queue", queue))
    }

    /// The family index behind a queue handle. The encoder needs it for its
    /// command pool and for queue-ownership transfers.
    pub(crate) fn queue_family(&self, queue: QueueHandle) -> Result<u32, HalError> {
        self.queue_slot(queue).map(|slot| slot.family)
    }

    /// Resolves a buffer handle for the command encoder.
    pub(crate) fn buffer_raw(
        &self,
        state: &DeviceState,
        handle: BufferHandle,
    ) -> Result<vk::Buffer, HalError> {
        lookup(&state.buffers, "buffer", handle, self).map(|entry| entry.raw)
    }

    /// Resolves an image handle, returning its format too — the encoder needs
    /// it to fill in an aspect mask the caller left empty.
    pub(crate) fn image_raw(
        &self,
        state: &DeviceState,
        handle: ImageHandle,
    ) -> Result<(vk::Image, Format), HalError> {
        lookup(&state.images, "image", handle, self).map(|entry| (entry.raw, entry.format))
    }

    /// Whether `handle` is an offscreen ring image, whose reuse the seam orders
    /// with no semaphore of its own.
    ///
    /// A miss answers `false`: the caller has already resolved the image for
    /// its own purposes and reported the bad handle, and this only ever widens
    /// a barrier that is about to be recorded anyway.
    pub(crate) fn image_ring_reuse(&self, state: &DeviceState, handle: ImageHandle) -> bool {
        lookup(&state.images, "image", handle, self).is_ok_and(|entry| entry.ring_reuse)
    }

    /// Resolves an image-view handle, returning its format too — the encoder
    /// needs it to decide whether a depth attachment has a stencil plane.
    pub(crate) fn view_raw(
        &self,
        state: &DeviceState,
        handle: ImageViewHandle,
    ) -> Result<(vk::ImageView, Format), HalError> {
        lookup(&state.views, "image view", handle, self).map(|entry| (entry.raw, entry.format))
    }

    /// Resolves a query-set handle, returning its pool, kind and query count —
    /// the encoder bounds-checks caller-supplied ranges against the count before
    /// handing them to the driver.
    pub(crate) fn query_set_raw(
        &self,
        state: &DeviceState,
        handle: QuerySetHandle,
    ) -> Result<(vk::QueryPool, QueryKind, u32), HalError> {
        lookup(&state.query_sets, "query set", handle, self)
            .map(|entry| (entry.raw, entry.kind, entry.count))
    }

    /// The submission counter's current value: the retirement key for anything
    /// destroyed right now.
    fn submissions(&self) -> u64 {
        self.submissions.load(Ordering::Acquire)
    }

    /// Reads the retire timeline and frees whatever the GPU has finished with.
    ///
    /// A failed read is **reported**, not swallowed. `ERROR_DEVICE_LOST` and
    /// "nothing has retired yet" produce the same control flow here, so a
    /// silent `return` made a lost device look like an idle one: the queue
    /// stops draining and the symptom that eventually surfaces is a slow leak
    /// rather than the device loss that caused it. Nothing above can act on it
    /// mid-poll — the callers that can are `submit`, `poll_readback` and
    /// `wait_semaphores`, which all report the same failure themselves — so
    /// this logs and returns.
    pub(crate) fn poll_retire(&self, state: &mut DeviceState) {
        // SAFETY: `retire_timeline` is a live timeline semaphore of this device.
        let completed = unsafe { self.raw.get_semaphore_counter_value(self.retire_timeline) };
        let completed = match completed {
            Ok(completed) => completed,
            Err(error) => {
                crcbl_core::log::error!(
                    "crcbl-vk: vkGetSemaphoreCounterValue on the retire timeline failed \
                     ({error:?}); {} object(s) stay parked and nothing will be freed until it \
                     succeeds",
                    state.trash.pending()
                );
                return;
            }
        };
        // What the timeline cannot say: a command buffer that is recorded and
        // not yet submitted references everything it names, and no submission
        // has been issued that would extend those objects' keys — `submit`
        // extends for the buffers in *that* submit. So an earlier submission
        // completing is not proof the object is unreferenced, and freeing on the
        // timeline alone runs `vkDestroy*` under a recording, which the layer
        // reports as `VUID-vkQueueSubmit2-commandBuffer-03874` at the submit
        // that follows. Collected by value so the borrow of the command-buffer
        // table ends before the queue is mutated.
        let held: Vec<u64> = state
            .command_buffers
            .iter()
            .filter(|(_, entry)| !entry.submitted)
            .flat_map(|(_, entry)| entry.references.iter().copied())
            .collect();
        let raw = &self.raw;
        let swapchain_ext = &self.swapchain_ext;
        let instance = &self.instance;
        state.trash.retire(
            completed,
            |item| held.contains(&trash_raw(item)),
            |item| {
                // SAFETY: every object in the queue was created by this device
                // and is destroyed exactly once. The timeline reaching its key
                // and no live recording naming it are together the proof that
                // nothing still references it — which is the whole contract of
                // this module.
                unsafe { destroy_trash(raw, swapchain_ext, instance, item) };
            },
        );
    }

    /// Parks a driver object until it is safe to free.
    ///
    /// The key is `submissions() + 1`, not `submissions()`. A caller may record
    /// a command buffer that uses an object, destroy the handle, and *then*
    /// submit — the seam explicitly says `destroy_*` means "this handle is dead
    /// now", not "the GPU is finished" — so the earliest submission that can
    /// still reference it is the next one, not the last one already issued.
    /// Keying on `submissions()` frees it during that very submit, because the
    /// timeline has already reached that value.
    fn park(&self, state: &mut DeviceState, item: Trash) {
        state.trash.push(self.submissions() + 1, item);
    }

    /// Frees a swapchain, after making sure nothing is still using it.
    ///
    /// **A swapchain cannot go through the deletion queue.** The queue is keyed
    /// on the submission timeline, and `vkQueuePresentKHR` is queue work that
    /// *no* timeline semaphore signals — so "the timeline reached N" says
    /// nothing about whether the presents on this swapchain have finished.
    /// P1.1 found this the way it deserves to be found: validation reporting
    /// `VUID-vkDestroySwapchainKHR-swapchain-01282` ("currently in use by
    /// VkQueue") on the first window resize under a real compositor, where the
    /// headless ring had never noticed.
    ///
    /// So this waits. A full device idle per resize is heavy and rare, and it
    /// is the boring correct answer; `VK_EXT_swapchain_maintenance1`'s present
    /// fences are the surgical one, and belong with the frame ring at P1.3.
    ///
    /// # `vkDeviceWaitIdle` is not enough, and this is the sequence that proves
    /// it
    ///
    /// **A pending `vkAcquireNextImageKHR` is not queue work**, so idling the
    /// device does not complete it. The ordinary shape of a resize is `acquire
    /// → resize event arrives → reconfigure before ever presenting`, and that
    /// leaves exactly one slot with a fence the presentation engine has not
    /// signalled and a semaphore with a signal still outstanding. Destroying
    /// either is `VUID-vkDestroyFence-fence-01120` /
    /// `VUID-vkDestroySemaphore-semaphore-05149`, and on a real driver it is a
    /// use-after-free in the WSI layer. [`FrameSync::acquire_armed`] already
    /// records which slots are outstanding; this waits on those.
    fn retire_swapchain(&self, entry: SwapchainEntry) {
        self.drain_pending_acquires(entry.sync.as_ref());
        // SAFETY: `raw` is a live device; waiting is always legal.
        let _ = unsafe { self.raw.device_wait_idle() };
        let trash = Trash::Swapchain(Box::new(TrashSwapchain {
            swapchain: entry.raw,
            surface: entry.surface_raw,
            images: if entry.raw == vk::SwapchainKHR::null() {
                entry.images
            } else {
                Vec::new()
            },
            views: entry.views,
            memory: entry.memory,
            sync: entry.sync,
        }));
        // SAFETY: the device is idle, so nothing references any of it.
        unsafe { destroy_trash(&self.raw, &self.swapchain_ext, &self.instance, trash) };
    }

    /// Waits out every acquire this swapchain still has in flight.
    ///
    /// Called before the device idle in [`retire_swapchain`](Self::retire_swapchain),
    /// because idling does not complete an acquire — see that method. A slot is
    /// only waited on when its previous acquire actually *armed* it: a failed
    /// `vkAcquireNextImageKHR` signals neither the fence nor the semaphore, so
    /// an unconditional wait on every slot would block forever.
    fn drain_pending_acquires(&self, sync: Option<&FrameSync>) {
        let Some(sync) = sync else {
            return;
        };
        let pending: Vec<vk::Fence> = sync
            .acquire_fence
            .iter()
            .zip(&sync.acquire_armed)
            .filter_map(|(fence, armed)| armed.then_some(*fence))
            .collect();
        if pending.is_empty() {
            return;
        }
        // Bounded rather than `u64::MAX`: a compositor that never returns the
        // image would otherwise hang the process here with the device lock
        // released but the caller stuck mid-resize. Five seconds is far beyond
        // any real acquire, and timing out is loud rather than silent — the
        // destroy that follows is then a genuine (and reported) risk rather
        // than a certainty.
        const ACQUIRE_TIMEOUT_NS: u64 = 5_000_000_000;
        // SAFETY: every fence named is a live fence of this device, armed by a
        // successful acquire on this swapchain.
        let waited = unsafe { self.raw.wait_for_fences(&pending, true, ACQUIRE_TIMEOUT_NS) };
        if let Err(error) = waited {
            crcbl_core::log::error!(
                "crcbl-vk: {} acquire fence(s) still pending after 5s while retiring a \
                 swapchain ({error:?}); destroying them anyway, which the driver may report",
                pending.len()
            );
        }
    }

    /// Allocates a dedicated block for `requirements`.
    ///
    /// # Both tiers of the preference, not just the first
    ///
    /// [`MemoryRequest`] states a `preferred` set and a `required` one, and
    /// `find_memory_type` falls back from one to the other when no memory
    /// *type* matches. That is only half the story: on a discrete GPU without
    /// resizable BAR, `HostUpload`'s preferred `DEVICE_LOCAL | HOST_VISIBLE`
    /// type exists and sits on a **256 MB** heap. Once that heap fills,
    /// `vkAllocateMemory` fails while a plain host-visible type with gigabytes
    /// free satisfies `required` perfectly well — and the old code reported
    /// `OutOfDeviceMemory` with the machine barely warm.
    ///
    /// So the fallback happens on *allocation* failure too, not only on
    /// selection failure.
    fn allocate(
        &self,
        requirements: vk::MemoryRequirements,
        location: MemoryLocation,
    ) -> Result<vk::DeviceMemory, HalError> {
        let request = MemoryRequest::for_location(location);
        let preferred = mem::find_memory_type(
            &self.memory_properties,
            requirements.memory_type_bits,
            request,
        );
        let fallback = mem::find_memory_type(
            &self.memory_properties,
            requirements.memory_type_bits,
            MemoryRequest {
                required: request.required,
                preferred: request.required,
            },
        );
        let mut candidates: Vec<u32> = Vec::with_capacity(2);
        candidates.extend(preferred);
        candidates.extend(fallback.filter(|index| Some(*index) != preferred));
        if candidates.is_empty() {
            return Err(HalError::OutOfDeviceMemory);
        }

        let mut last = HalError::OutOfDeviceMemory;
        for index in candidates {
            let info = vk::MemoryAllocateInfo::default()
                .allocation_size(requirements.size)
                .memory_type_index(index);
            // SAFETY: `info` names a memory type this device reported.
            match unsafe { self.raw.allocate_memory(&info, None) } {
                Ok(memory) => return Ok(memory),
                Err(error) => {
                    crcbl_core::log::debug!(
                        "crcbl-vk: memory type {index} could not satisfy a {} byte \
                         {location:?} allocation ({error:?})",
                        requirements.size
                    );
                    last = conv::hal_error("vkAllocateMemory", error);
                }
            }
        }
        Err(last)
    }

    /// Creates an image this backend owns, with a dedicated allocation.
    ///
    /// Used by [`Device::create_image`] and by the offscreen swapchain ring,
    /// which is the reason it exists as a helper at all.
    ///
    /// The allocation is always [`MemoryLocation::DeviceLocal`]: `ImageDesc`
    /// has no memory field, and the swapchain ring's images are attachments.
    pub(crate) fn create_owned_image(
        &self,
        info: &vk::ImageCreateInfo<'_>,
        label: Option<&str>,
    ) -> Result<(vk::Image, vk::DeviceMemory), HalError> {
        // SAFETY: `info` is a fully populated descriptor with no dangling
        // chained structs.
        let image = unsafe { self.raw.create_image(info, None) }
            .map_err(|error| conv::hal_error("vkCreateImage", error))?;
        // SAFETY: `image` was just created by this device.
        let requirements = unsafe { self.raw.get_image_memory_requirements(image) };
        let memory = match self.allocate(requirements, MemoryLocation::DeviceLocal) {
            Ok(memory) => memory,
            Err(error) => {
                // SAFETY: `image` is live, unbound and unused.
                unsafe { self.raw.destroy_image(image, None) };
                return Err(error);
            }
        };
        // SAFETY: `memory` was allocated for exactly these requirements and is
        // bound once.
        if let Err(error) = unsafe { self.raw.bind_image_memory(image, memory, 0) } {
            // SAFETY: both are live and unused.
            unsafe {
                self.raw.destroy_image(image, None);
                self.raw.free_memory(memory, None);
            }
            return Err(conv::hal_error("vkBindImageMemory", error));
        }
        self.set_object_name(image, label);
        Ok((image, memory))
    }
}

/// Frees one parked object.
///
/// # Safety
///
/// `item` must have been created by `raw` (or, for a swapchain's surface
/// reference, by the instance behind `instance`), and must not be referenced by
/// any submission still executing.
unsafe fn destroy_trash(
    raw: &ash::Device,
    swapchain_ext: &khr::swapchain::Device,
    instance: &Arc<InstanceInner>,
    item: Trash,
) {
    // SAFETY: discharged by the caller's contract, one arm at a time.
    unsafe {
        match item {
            Trash::Buffer(buffer, memory) => {
                raw.destroy_buffer(buffer, None);
                raw.free_memory(memory, None);
            }
            Trash::Image(image, memory) => {
                raw.destroy_image(image, None);
                if memory != vk::DeviceMemory::null() {
                    raw.free_memory(memory, None);
                }
            }
            Trash::ImageView(view) => raw.destroy_image_view(view, None),
            Trash::Semaphore(semaphore) => raw.destroy_semaphore(semaphore, None),
            Trash::QueryPool(pool) => raw.destroy_query_pool(pool, None),
            Trash::ShaderModule(module) => raw.destroy_shader_module(module, None),
            Trash::DescriptorSetLayout(layout) => raw.destroy_descriptor_set_layout(layout, None),
            // Destroying the pool frees the set with it, which is the whole
            // reason a bind group owns one.
            Trash::DescriptorPool(pool) => raw.destroy_descriptor_pool(pool, None),
            Trash::PipelineLayout(layout) => raw.destroy_pipeline_layout(layout, None),
            Trash::Pipeline(pipeline) => raw.destroy_pipeline(pipeline, None),
            Trash::Sampler(sampler) => raw.destroy_sampler(sampler, None),
            Trash::Swapchain(entry) => {
                let TrashSwapchain {
                    swapchain,
                    surface,
                    images,
                    views,
                    memory,
                    sync,
                } = *entry;
                // Views before images and before the swapchain: a view must not
                // outlive what it views, whoever owns that.
                for view in views {
                    raw.destroy_image_view(view, None);
                }
                if swapchain == vk::SwapchainKHR::null() {
                    // The offscreen ring: images are ours.
                    for image in images {
                        raw.destroy_image(image, None);
                    }
                    for block in memory {
                        raw.free_memory(block, None);
                    }
                } else {
                    swapchain_ext.destroy_swapchain(swapchain, None);
                }
                if let Some(sync) = sync {
                    for semaphore in sync.acquire.into_iter().chain(sync.present) {
                        raw.destroy_semaphore(semaphore, None);
                    }
                    for fence in sync.acquire_fence {
                        raw.destroy_fence(fence, None);
                    }
                }
                // Obligation 2: the last swapchain going away is what finally
                // permits `vkDestroySurfaceKHR`.
                instance.release_surface(surface);
            }
        }
    }
}

/// Invalidates every handle a swapchain issued.
///
/// The images, their views and the acquire/present semaphores are all
/// `swapchain_owned`, so this is the *only* thing that takes them out of the
/// tables — `destroy_image`, `destroy_image_view` and `destroy_semaphore` all
/// deliberately refuse them. Every path that lets go of a `SwapchainEntry` must
/// call this, or the rows outlive the objects they name.
fn forget_swapchain_rows(inner: &DeviceInner, state: &mut DeviceState, entry: &SwapchainEntry) {
    for handle in &entry.image_handles {
        take_owned(&mut state.images, *handle, inner);
    }
    for handle in &entry.view_handles {
        take_owned(&mut state.views, *handle, inner);
    }
    for handle in entry
        .sync
        .as_ref()
        .into_iter()
        .flat_map(|sync| sync.acquire_handles.iter().chain(&sync.present_handles))
    {
        take_owned(&mut state.semaphores, *handle, inner);
    }
}

/// Decodes a handle for `inner`'s pools, or says why it is not one.
fn local_handle<E, M>(
    kind: &'static str,
    handle: Handle<M>,
    inner: &DeviceInner,
) -> Result<Handle<E>, HalError> {
    let tag = handle_tag(handle);
    if tag == inner.tag {
        return Ok(untag(handle));
    }
    // Tag zero was never issued by any device — a hand-made handle, or one
    // whose pool index overflowed the tagged range. Anything else is a real
    // handle belonging to a real, different device, which is the case
    // obligation 3 exists for and which this backend could not previously
    // report at all.
    Err(if tag == 0 {
        HalError::invalid_handle(kind, handle)
    } else {
        HalError::ForeignObject {
            kind,
            bits: handle.to_bits(),
        }
    })
}

/// Resolves a handle against a pool and its owning device.
pub(crate) fn lookup<'p, E: Owned, M>(
    pool: &'p Pool<E>,
    kind: &'static str,
    handle: Handle<M>,
    inner: &DeviceInner,
) -> Result<&'p E, HalError> {
    let local = local_handle(kind, handle, inner)?;
    match pool.get(local) {
        Some(entry) if entry.owner() == inner.id => Ok(entry),
        Some(_) => Err(HalError::ForeignObject {
            kind,
            bits: handle.to_bits(),
        }),
        None => Err(HalError::invalid_handle(kind, handle)),
    }
}

fn lookup_mut<'p, E: Owned, M>(
    pool: &'p mut Pool<E>,
    kind: &'static str,
    handle: Handle<M>,
    inner: &DeviceInner,
) -> Result<&'p mut E, HalError> {
    let local = local_handle(kind, handle, inner)?;
    match pool.get(local).map(Owned::owner) {
        Some(entry_owner) if entry_owner == inner.id => Ok(pool
            .get_mut(local)
            .unwrap_or_else(|| unreachable!("resolved immediately above"))),
        Some(_) => Err(HalError::ForeignObject {
            kind,
            bits: handle.to_bits(),
        }),
        None => Err(HalError::invalid_handle(kind, handle)),
    }
}

/// Removes a handle from `pool`, but **only** if this device owns it.
///
/// The order is the whole point. Every `destroy_*` used to `remove` first and
/// check `owner` afterwards, by which time the row was gone and the entry
/// dropped: the driver object leaked, and a foreign handle that happened to
/// resolve killed this device's own unrelated object outright.
/// `Instance::destroy_surface` always had this right; this is that shape, once,
/// for the eleven bodies that did not.
fn take_owned<E: Owned, M>(
    pool: &mut Pool<E>,
    handle: Handle<M>,
    inner: &DeviceInner,
) -> Option<E> {
    let local: Handle<E> = local_handle("object", handle, inner).ok()?;
    if !pool
        .get(local)
        .is_some_and(|entry| entry.owner() == inner.id)
    {
        return None;
    }
    pool.remove(local)
}

/// The "this device cannot" answer, in one place so the message is uniform.
///
/// P1.1 used this for the whole pipeline surface; P1.2 implemented it, so what
/// is left are the genuine per-device capability refusals.
fn not_yet(what: &'static str) -> HalError {
    HalError::Unsupported {
        backend: BackendKind::Vulkan,
        what,
    }
}

impl Device for VkDevice {
    fn backend(&self) -> BackendKind {
        BackendKind::Vulkan
    }

    /// What this backend does with each seam behaviour.
    ///
    /// **Vulkan is the reference implementation**, so almost every arm is
    /// [`Support::Yes`] and the rest are the device's own report rather than
    /// this backend's limits — `crcbl-vk` refuses a capability only where the
    /// adapter said the device has not got it, which is
    /// [`crcbl_hal::Features`] doing its job and not a divergence. That is why
    /// `crcbl_hal::DIVERGENCES` names Vulkan nowhere.
    ///
    /// Exhaustive with no wildcard arm, and `deny`-ed as such: a capability
    /// added to the enum must be answered here, and an arm that swept the rest
    /// under a `_` would put this backend back where the enum found it.
    #[deny(clippy::wildcard_enum_match_arm)]
    fn supports(&self, capability: Capability) -> Support {
        let has = self.inner.caps.features;
        let gated = |feature: Features, why: &'static str| -> Support {
            Support::granted(has, feature, why)
        };

        match capability {
            // `vkCmdFillBuffer` takes a `uint32_t` and writes it whole, which is
            // the seam's own wording and the reason the three fill capabilities
            // are separable at all: this is the backend that has all three.
            Capability::BufferFillZero => Support::Yes,
            Capability::ImageToImageCopy => Support::Yes,
            // `VkBufferImageCopy::imageSubresource` names the aspect, so a depth
            // plane is addressed by the same call and the same struct a colour
            // copy is — there is no second footprint to describe.
            Capability::DepthImageCopy => Support::Yes,
            // `VkRenderingAttachmentInfo` carries `resolveImageView` and
            // `resolveMode`, filled from `ColorAttachment::resolve`.
            Capability::MsaaResolveAttachment => Support::Yes,
            Capability::StencilReference => Support::Yes,
            Capability::DrawIndirectCount => gated(
                Features::DRAW_INDIRECT_COUNT,
                "this device reports no DRAW_INDIRECT_COUNT, so vkCmdDrawIndirectCount is not \
                 available on it",
            ),
            // `vkCmdDrawIndirect` takes `stride` and honours it.
            Capability::IndirectArgumentPaddedStride => Support::Yes,
            Capability::MeshShading => gated(
                Features::MESH_SHADER,
                "this device reports no MESH_SHADER, so VK_EXT_mesh_shader was not enabled on it",
            ),
            Capability::TaskShaderStage => gated(
                Features::TASK_SHADER,
                "this device reports no TASK_SHADER, so its mesh-shader extension has no task \
                 stage",
            ),
            // Refused per *layout*, on the UPDATE_AFTER_BIND flag the caller
            // built it with — writing a set a pending command buffer might
            // reference is otherwise undefined behaviour. The operation exists.
            // **Gated on descriptor indexing, because this backend's rewrite
            // needs the flag that rides with it.** `update_bind_group` refuses a
            // layout without `UPDATE_AFTER_BIND`, and the
            // `descriptor_binding_*_update_after_bind` bits this device is
            // opened with all come from `Features::DESCRIPTOR_INDEXING` — so
            // without it there is no layout this call will take. Answering `Yes`
            // unconditionally was true on every device CI opens and false on a
            // lesser one; `CRCBL_SEAM_WITHHOLD=all` is what found it.
            Capability::UpdateBindGroup => gated(
                Features::DESCRIPTOR_INDEXING,
                "this device reports no DESCRIPTOR_INDEXING, so no layout carries \
                 UPDATE_AFTER_BIND and update_bind_group has nothing it will take",
            ),
            Capability::PushConstants => gated(
                Features::PUSH_CONSTANTS,
                "this device reports no PUSH_CONSTANTS",
            ),
            Capability::BindlessDescriptorArray => gated(
                Features::DESCRIPTOR_INDEXING,
                "this device reports no DESCRIPTOR_INDEXING, so it has no runtime-sized descriptor \
                 array",
            ),
            Capability::StorageImageBinding => Support::Yes,
            Capability::PolygonModeLine => gated(
                Features::POLYGON_MODE_LINE,
                "this device reports no POLYGON_MODE_LINE, so `fillModeNonSolid` was not enabled \
                 on it",
            ),
            Capability::DepthClamp => gated(
                Features::DEPTH_CLAMP,
                "this device reports no DEPTH_CLAMP, so `depthClamp` was not enabled on it",
            ),
            Capability::SamplerAnisotropy => gated(
                Features::SAMPLER_ANISOTROPY,
                "this device reports no SAMPLER_ANISOTROPY, so `samplerAnisotropy` was not enabled \
                 on it",
            ),
            Capability::TimestampQuery => gated(
                Features::TIMESTAMP_QUERY,
                "this device reports no TIMESTAMP_QUERY",
            ),
            Capability::OcclusionQuery => gated(
                Features::OCCLUSION_QUERY,
                "this device reports no OCCLUSION_QUERY",
            ),
            Capability::PipelineStatisticsQuery => gated(
                Features::PIPELINE_STATISTICS_QUERY,
                "this device reports no PIPELINE_STATISTICS_QUERY",
            ),
            // `crcbl-vk` requires Vulkan 1.3, where `timelineSemaphore` is core,
            // so the flag is reported by every device this backend opens — but
            // the answer is still the device's rather than a constant, because a
            // backend claiming a capability its own caps deny is the exact lie
            // this enum exists to catch.
            // `vkSignalSemaphore` is part of the same Vulkan 1.2 promotion as
            // the counter read and the host wait, and `crcbl-vk` requires
            // `timelineSemaphore` at device creation — so the three arrive
            // together or the device is not opened.
            Capability::TimelineSemaphore
            | Capability::CpuTimelineWait
            | Capability::CpuTimelineSignal => gated(
                Features::TIMELINE_SEMAPHORE,
                "this device reports no TIMELINE_SEMAPHORE",
            ),
            Capability::BinarySemaphore => Support::Yes,
            // A real `VkSemaphore` blocks until somebody signals it, so a wait
            // may be recorded or submitted before the signal exists — but only
            // where there is a timeline to wait on at all, which is why this
            // rides the same gate the timeline rows do rather than answering
            // `Yes` unconditionally. Found by `CRCBL_SEAM_WITHHOLD=all`: on a
            // device opened without the feature this declared support and then
            // refused, because `create_semaphore` now honours the declaration.
            Capability::TimelineWaitBeforeSignal => gated(
                Features::TIMELINE_SEMAPHORE,
                "this device reports no TIMELINE_SEMAPHORE, so there is no timeline to wait on",
            ),
        }
    }

    fn caps(&self) -> DeviceCaps {
        self.inner.caps
    }

    fn queue(&self, kind: QueueKind) -> Option<QueueHandle> {
        self.inner.queues[queue_index(kind) as usize].map(|_| queue_handle(self.inner.tag, kind))
    }

    // --- resources ---

    fn create_buffer(&self, desc: &BufferDesc<'_>) -> Result<BufferHandle, HalError> {
        if desc.size == 0 {
            return Err(HalError::InvalidDescriptor(
                "BufferDesc::size must be non-zero".to_string(),
            ));
        }
        let info = vk::BufferCreateInfo::default()
            .size(desc.size)
            .usage(conv::buffer_usage(desc.usage))
            .sharing_mode(vk::SharingMode::EXCLUSIVE);
        // SAFETY: `info` is fully populated with no chained structs.
        let raw = unsafe { self.inner.raw.create_buffer(&info, None) }
            .map_err(|error| conv::hal_error("vkCreateBuffer", error))?;
        // SAFETY: `raw` was just created by this device.
        let requirements = unsafe { self.inner.raw.get_buffer_memory_requirements(raw) };
        let memory = match self.inner.allocate(requirements, desc.memory) {
            Ok(memory) => memory,
            Err(error) => {
                // SAFETY: `raw` is live, unbound and unused.
                unsafe { self.inner.raw.destroy_buffer(raw, None) };
                return Err(error);
            }
        };
        // SAFETY: allocated for exactly these requirements, bound once.
        if let Err(error) = unsafe { self.inner.raw.bind_buffer_memory(raw, memory, 0) } {
            // SAFETY: both live and unused.
            unsafe {
                self.inner.raw.destroy_buffer(raw, None);
                self.inner.raw.free_memory(memory, None);
            }
            return Err(conv::hal_error("vkBindBufferMemory", error));
        }

        let mapped = if desc.memory.is_mappable() {
            // SAFETY: `memory` is host-visible (the memory-type request
            // required it), is not already mapped, and the mapping lives
            // exactly as long as the allocation.
            match unsafe {
                self.inner
                    .raw
                    .map_memory(memory, 0, vk::WHOLE_SIZE, vk::MemoryMapFlags::empty())
            } {
                Ok(pointer) => pointer.cast::<u8>(),
                Err(error) => {
                    // SAFETY: both live and unused.
                    unsafe {
                        self.inner.raw.destroy_buffer(raw, None);
                        self.inner.raw.free_memory(memory, None);
                    }
                    return Err(conv::hal_error("vkMapMemory", error));
                }
            }
        } else {
            core::ptr::null_mut()
        };

        self.inner.set_object_name(raw, desc.label);
        Ok(self
            .inner
            .stamp(self.inner.state().buffers.insert(BufferEntry {
                owner: self.inner.id,
                raw,
                memory,
                size: desc.size,
                location: desc.memory,
                mapped,
            })))
    }

    fn destroy_buffer(&self, buffer: BufferHandle) {
        let mut state = self.inner.state();
        let Some(entry) = take_owned(&mut state.buffers, buffer, &self.inner) else {
            return;
        };
        if !entry.mapped.is_null() {
            // SAFETY: `entry.memory` was mapped once in `create_buffer` and is
            // unmapped once here, before the allocation is parked for freeing.
            unsafe { self.inner.raw.unmap_memory(entry.memory) };
        }
        self.inner
            .park(&mut state, Trash::Buffer(entry.raw, entry.memory));
    }

    fn write_buffer(&self, buffer: BufferHandle, offset: u64, data: &[u8]) -> Result<(), HalError> {
        let state = self.inner.state();
        let entry = lookup(&state.buffers, "buffer", buffer, &self.inner)?;
        if !entry.location.is_mappable() {
            return Err(HalError::InvalidDescriptor(format!(
                "write_buffer needs a mappable buffer; this one is {:?}",
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
        // SAFETY: the mapping covers the whole allocation, the range was just
        // bounds-checked against the buffer's size, and the two regions cannot
        // overlap because `data` is a caller-owned slice and the mapping is
        // device memory.
        unsafe {
            core::ptr::copy_nonoverlapping(
                data.as_ptr(),
                entry.mapped.add(offset as usize),
                data.len(),
            );
        }
        Ok(())
    }

    fn request_readback(&self, desc: &ReadbackDesc<'_>) -> Result<ReadbackHandle, HalError> {
        let mut state = self.inner.state();
        let entry = lookup(&state.buffers, "buffer", desc.buffer, &self.inner)?;
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
        // "Everything submitted to this device before this call" is exactly a
        // snapshot of the submission counter, which is what the retire timeline
        // counts — so a readback with no explicit wait needs no extra object.
        let (wait_semaphore, wait_value) = match desc.after {
            Some(wait) => {
                let semaphore =
                    lookup(&state.semaphores, "semaphore", wait.semaphore, &self.inner)?;
                if !semaphore.timeline {
                    return Err(HalError::Unsupported {
                        backend: BackendKind::Vulkan,
                        what: "ReadbackDesc::after must name a timeline semaphore",
                    });
                }
                (Some(wait.semaphore), wait.value)
            }
            None => (None, self.inner.submissions()),
        };

        Ok(self.inner.stamp(state.readbacks.insert(ReadbackEntry {
            owner: self.inner.id,
            buffer: desc.buffer,
            offset: desc.offset,
            size: desc.size,
            wait_semaphore,
            wait_value,
        })))
    }

    fn poll_readback(
        &self,
        readback: ReadbackHandle,
        out: &mut [u8],
    ) -> Result<ReadbackState, HalError> {
        let state = self.inner.state();
        let entry = lookup(&state.readbacks, "readback", readback, &self.inner)?;
        if out.len() as u64 != entry.size {
            return Err(HalError::InvalidDescriptor(format!(
                "poll_readback needs exactly {} bytes, got {}",
                entry.size,
                out.len()
            )));
        }
        // Resolve the wait semaphore from the handle stored at request time,
        // exactly like the buffer below. A handle that passes `lookup` is live
        // in this device's pool; `None` names the retire timeline, which is
        // device-owned and never destroyed — either way the raw semaphore this
        // call dereferences is live.
        // SAFETY: the resolved semaphore is live by construction: `lookup`
        // succeeded, or it is this device's own retire timeline.
        let wait_semaphore = match entry.wait_semaphore {
            Some(handle) => lookup(&state.semaphores, "semaphore", handle, &self.inner)?.raw,
            None => self.inner.retire_timeline,
        };
        let reached = unsafe { self.inner.raw.get_semaphore_counter_value(wait_semaphore) }
            .map_err(|error| conv::hal_error("vkGetSemaphoreCounterValue", error))?;
        if reached < entry.wait_value {
            return Ok(ReadbackState::Pending);
        }
        if entry.size > 0 {
            // Resolve the buffer's mapped pointer from the handle stored at
            // request time. If the buffer was destroyed between request and
            // poll, the generational handle fails lookup here — rather than
            // silently dereferencing unmapped memory.
            let buffer_entry = lookup(&state.buffers, "buffer", entry.buffer, &self.inner)?;
            if buffer_entry.mapped.is_null() {
                return Err(HalError::InvalidDescriptor(
                    "buffer for readback is no longer mapped".to_string(),
                ));
            }
            // SAFETY: the mapping covers the whole allocation and the range was
            // bounds-checked at request time; the memory is `HOST_COHERENT`, so
            // the writes the GPU made before the timeline value are visible
            // without an explicit invalidate.
            unsafe {
                core::ptr::copy_nonoverlapping(
                    buffer_entry.mapped.add(entry.offset as usize),
                    out.as_mut_ptr(),
                    out.len(),
                );
            }
        }
        Ok(ReadbackState::Ready)
    }

    fn destroy_readback(&self, readback: ReadbackHandle) {
        // No driver object: the mapping belongs to the buffer, which the caller
        // still owns. Dropping the tracking entry is the whole of it.
        let mut state = self.inner.state();
        take_owned(&mut state.readbacks, readback, &self.inner);
    }

    fn create_image(&self, desc: &ImageDesc<'_>) -> Result<ImageHandle, HalError> {
        if desc.extent.width == 0 || desc.extent.height == 0 || desc.extent.depth_or_layers == 0 {
            return Err(HalError::InvalidDescriptor(format!(
                "ImageDesc::extent {:?} has a zero dimension",
                desc.extent
            )));
        }
        let limits = self.inner.caps.limits;
        let is_3d = matches!(desc.image_type, crcbl_hal::ImageType::D3);
        // A 3D image is bounded by `max_image_3d` on every axis; a 1D/2D one by
        // `max_image_2d` on width and height and by `max_image_array_layers` on
        // its layer count. Only the 2D extent used to be checked at all, so a
        // volume's depth and an array's layer count reached the driver as VUID
        // failures rather than as the descriptor error the seam promises.
        if is_3d {
            let longest = desc
                .extent
                .width
                .max(desc.extent.height)
                .max(desc.extent.depth_or_layers);
            if longest > limits.max_image_3d {
                return Err(HalError::InvalidDescriptor(format!(
                    "ImageDesc::extent {:?} exceeds max_image_3d {}",
                    desc.extent, limits.max_image_3d
                )));
            }
        } else {
            if desc.extent.width > limits.max_image_2d || desc.extent.height > limits.max_image_2d {
                return Err(HalError::InvalidDescriptor(format!(
                    "ImageDesc::extent {:?} exceeds max_image_2d {}",
                    desc.extent, limits.max_image_2d
                )));
            }
            if desc.extent.depth_or_layers > limits.max_image_array_layers {
                return Err(HalError::InvalidDescriptor(format!(
                    "ImageDesc::extent {:?} asks for more array layers than \
                     max_image_array_layers {}",
                    desc.extent, limits.max_image_array_layers
                )));
            }
        }
        // `depth_or_layers` mips for a volume and does not for an array, which
        // is why the image type is a parameter.
        let full_chain = desc.extent.full_mip_levels(desc.image_type);
        if desc.mip_levels > full_chain {
            return Err(HalError::InvalidDescriptor(format!(
                "ImageDesc::mip_levels is {} but {:?} has only {full_chain} levels",
                desc.mip_levels, desc.extent
            )));
        }
        // Through `conv::sample_count`, which exists precisely to reject a
        // non-power-of-two: `from_raw(3)` decodes as `TYPE_1 | TYPE_2`, so a
        // caller asking for three samples reached the driver as a two-bit mask
        // rather than as an error. `create_graphics_pipeline_impl` always used
        // it; this did not.
        let Some(samples) = conv::sample_count(desc.samples.max(1)) else {
            return Err(HalError::InvalidDescriptor(format!(
                "ImageDesc::samples is {}, which is not a power of two in 1..=64",
                desc.samples
            )));
        };
        if desc.samples > limits.max_sample_count.max(1) {
            return Err(HalError::InvalidDescriptor(format!(
                "ImageDesc::samples is {} but this device supports at most {}",
                desc.samples, limits.max_sample_count
            )));
        }
        if desc.usage.is_empty() {
            return Err(HalError::InvalidDescriptor(
                "ImageDesc::usage is empty, so the image could never be used".to_string(),
            ));
        }
        // **Ask the device whether it can serve this format at all**, which
        // nothing here did. `vkCreateImage` is not required to fail for a
        // format/usage pair the implementation does not support — radv returns
        // success for `D24UnormS8Uint` as a depth-stencil attachment and the
        // validation layer reports `VK_ERROR_FORMAT_NOT_SUPPORTED` from this
        // very query, then two more VUIDs at view and pipeline creation. So a
        // caller got a live-looking handle and found out much later, or not at
        // all: the seam suite's raster fixture passed on undefined behaviour
        // once before the layer output was read.
        //
        // Refused as `Unsupported` rather than `InvalidDescriptor`: the
        // descriptor is well formed and another device would serve it, which is
        // exactly the distinction the seam draws between the two.
        let format_info = vk::PhysicalDeviceImageFormatInfo2::default()
            .format(conv::format(desc.format))
            .ty(conv::image_type(desc.image_type))
            .tiling(vk::ImageTiling::OPTIMAL)
            .usage(conv::image_usage(desc.usage))
            .flags(vk::ImageCreateFlags::empty());
        let mut properties = vk::ImageFormatProperties2::default();
        // SAFETY: `format_info` borrows only locals that outlive the call, and
        // `properties` is a fresh output struct with its `sType` set by
        // `default()`.
        if let Err(error) = unsafe {
            self.inner
                .instance
                .raw
                .get_physical_device_image_format_properties2(
                    self.inner.physical,
                    &format_info,
                    &mut properties,
                )
        } {
            return Err(HalError::Unsupported {
                backend: BackendKind::Vulkan,
                what: "an image of this format, type and usage — the device does not serve the \
                       combination",
            })
            .inspect_err(|_| {
                crcbl_core::log::debug!(
                    "crcbl-vk: {:?} as {:?} with {:?} is not a servable image here ({error})",
                    desc.format,
                    desc.image_type,
                    desc.usage,
                );
            });
        }
        let extent = vk::Extent3D {
            width: desc.extent.width,
            height: desc.extent.height,
            depth: if is_3d {
                desc.extent.depth_or_layers
            } else {
                1
            },
        };
        let info = vk::ImageCreateInfo::default()
            .image_type(conv::image_type(desc.image_type))
            .format(conv::format(desc.format))
            .extent(extent)
            .mip_levels(desc.mip_levels.max(1))
            .array_layers(if is_3d {
                1
            } else {
                desc.extent.depth_or_layers
            })
            .samples(samples)
            .tiling(vk::ImageTiling::OPTIMAL)
            .usage(conv::image_usage(desc.usage))
            .sharing_mode(vk::SharingMode::EXCLUSIVE)
            .initial_layout(vk::ImageLayout::UNDEFINED);
        let (raw, memory) = self.inner.create_owned_image(&info, desc.label)?;
        Ok(self
            .inner
            .stamp(self.inner.state().images.insert(ImageEntry {
                owner: self.inner.id,
                raw,
                memory,
                format: desc.format,
                swapchain_owned: false,
                // An ordinary image is reused only when the caller reuses it,
                // and the caller then owns the barrier that says so.
                ring_reuse: false,
            })))
    }

    fn destroy_image(&self, image: ImageHandle) {
        let mut state = self.inner.state();
        // Checked *before* the remove, exactly as `destroy_image_view` does. A
        // swapchain's images are named by the swapchain for as long as it
        // lives, so removing the row would leave every later `AcquiredFrame`
        // handing out a handle that no longer resolves — the swapchain would
        // be permanently unusable, from one stray call.
        if local_handle("image", image, &self.inner)
            .ok()
            .and_then(|local| state.images.get(local))
            .is_some_and(|entry| entry.swapchain_owned)
        {
            return;
        }
        let Some(entry) = take_owned(&mut state.images, image, &self.inner) else {
            return;
        };
        self.inner
            .park(&mut state, Trash::Image(entry.raw, entry.memory));
    }

    fn create_image_view(&self, desc: &ImageViewDesc<'_>) -> Result<ImageViewHandle, HalError> {
        let mut state = self.inner.state();
        // Copied out before the pool is borrowed mutably below.
        let image_raw = lookup(&state.images, "image", desc.image, &self.inner)?.raw;
        let info = vk::ImageViewCreateInfo::default()
            .image(image_raw)
            .view_type(conv::image_view_type(desc.view_type))
            .format(conv::format(desc.format))
            .subresource_range(conv::subresource_range(desc.range));
        // SAFETY: `image.raw` is a live image of this device and `info` borrows
        // nothing beyond this call.
        let raw = unsafe { self.inner.raw.create_image_view(&info, None) }
            .map_err(|error| conv::hal_error("vkCreateImageView", error))?;
        self.inner.set_object_name(raw, desc.label);
        Ok(self.inner.stamp(state.views.insert(ViewEntry {
            owner: self.inner.id,
            raw,
            format: desc.format,
            swapchain_owned: false,
        })))
    }

    fn destroy_image_view(&self, view: ImageViewHandle) {
        let mut state = self.inner.state();
        // A swapchain's views are handed out through `AcquiredFrame` and are
        // the swapchain's to destroy; a caller passing one here gets nothing,
        // rather than a swapchain whose attachments have been freed.
        if local_handle("image view", view, &self.inner)
            .ok()
            .and_then(|local| state.views.get(local))
            .is_some_and(|entry| entry.swapchain_owned)
        {
            return;
        }
        let Some(entry) = take_owned(&mut state.views, view, &self.inner) else {
            return;
        };
        self.inner.park(&mut state, Trash::ImageView(entry.raw));
    }

    fn create_sampler(&self, desc: &SamplerDesc<'_>) -> Result<SamplerHandle, HalError> {
        self.create_sampler_impl(desc)
    }

    fn destroy_sampler(&self, sampler: SamplerHandle) {
        self.retire_from(
            |state| &mut state.samplers,
            sampler,
            |entry| Trash::Sampler(entry.raw),
        );
    }

    // --- shaders and pipelines ---
    //
    // The bodies live in `pipeline.rs`; these forward so the `Device` impl stays
    // a readable index of the seam rather than a second thousand lines.

    fn create_shader_module(
        &self,
        desc: &ShaderModuleDesc<'_>,
    ) -> Result<ShaderModuleHandle, HalError> {
        self.create_shader_module_impl(desc)
    }

    fn destroy_shader_module(&self, module: ShaderModuleHandle) {
        self.retire_from(
            |state| &mut state.shader_modules,
            module,
            |entry| Trash::ShaderModule(entry.raw),
        );
    }

    fn create_bind_group_layout(
        &self,
        desc: &BindGroupLayoutDesc<'_>,
    ) -> Result<BindGroupLayoutHandle, HalError> {
        self.create_bind_group_layout_impl(desc)
    }

    fn destroy_bind_group_layout(&self, layout: BindGroupLayoutHandle) {
        self.retire_from(
            |state| &mut state.bind_group_layouts,
            layout,
            |entry| Trash::DescriptorSetLayout(entry.raw),
        );
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
        self.retire_from(
            |state| &mut state.bind_groups,
            group,
            |entry| Trash::DescriptorPool(entry.pool),
        );
    }

    fn create_pipeline_layout(
        &self,
        desc: &PipelineLayoutDesc<'_>,
    ) -> Result<PipelineLayoutHandle, HalError> {
        self.create_pipeline_layout_impl(desc)
    }

    fn destroy_pipeline_layout(&self, layout: PipelineLayoutHandle) {
        self.retire_from(
            |state| &mut state.pipeline_layouts,
            layout,
            |entry| Trash::PipelineLayout(entry.raw),
        );
    }

    fn create_graphics_pipeline(
        &self,
        desc: &GraphicsPipelineDesc<'_>,
    ) -> Result<GraphicsPipelineHandle, HalError> {
        self.create_graphics_pipeline_impl(desc)
    }

    fn create_mesh_pipeline(
        &self,
        desc: &crcbl_hal::MeshPipelineDesc<'_>,
    ) -> Result<GraphicsPipelineHandle, HalError> {
        self.create_mesh_pipeline_impl(desc)
    }

    fn destroy_graphics_pipeline(&self, pipeline: GraphicsPipelineHandle) {
        self.destroy_pipeline_handle(pipeline.cast());
    }

    fn create_compute_pipeline(
        &self,
        desc: &ComputePipelineDesc<'_>,
    ) -> Result<ComputePipelineHandle, HalError> {
        self.create_compute_pipeline_impl(desc)
    }

    fn destroy_compute_pipeline(&self, pipeline: ComputePipelineHandle) {
        self.destroy_pipeline_handle(pipeline.cast());
    }

    // --- queries ---

    fn create_query_set(&self, desc: &QuerySetDesc<'_>) -> Result<QuerySetHandle, HalError> {
        let (kind, statistics) = match desc.kind {
            QueryKind::Timestamp => {
                if !self.inner.caps.features.contains(Features::TIMESTAMP_QUERY) {
                    return Err(not_yet("this device has no timestamp queries"));
                }
                (
                    vk::QueryType::TIMESTAMP,
                    vk::QueryPipelineStatisticFlags::empty(),
                )
            }
            QueryKind::Occlusion => {
                if !self.inner.caps.features.contains(Features::OCCLUSION_QUERY) {
                    return Err(not_yet("this device has no occlusion queries"));
                }
                (
                    vk::QueryType::OCCLUSION,
                    vk::QueryPipelineStatisticFlags::empty(),
                )
            }
            QueryKind::PipelineStatistics => {
                if !self
                    .inner
                    .caps
                    .features
                    .contains(Features::PIPELINE_STATISTICS_QUERY)
                {
                    return Err(not_yet("this device has no pipeline-statistics queries"));
                }
                (
                    vk::QueryType::PIPELINE_STATISTICS,
                    vk::QueryPipelineStatisticFlags::VERTEX_SHADER_INVOCATIONS
                        | vk::QueryPipelineStatisticFlags::FRAGMENT_SHADER_INVOCATIONS
                        | vk::QueryPipelineStatisticFlags::CLIPPING_PRIMITIVES,
                )
            }
        };
        if desc.count == 0 {
            return Err(HalError::InvalidDescriptor(
                "QuerySetDesc::count must be non-zero".to_string(),
            ));
        }
        let info = vk::QueryPoolCreateInfo::default()
            .query_type(kind)
            .query_count(desc.count)
            .pipeline_statistics(statistics);
        // SAFETY: `info` names a query type this device supports, checked above.
        let raw = unsafe { self.inner.raw.create_query_pool(&info, None) }
            .map_err(|error| conv::hal_error("vkCreateQueryPool", error))?;
        self.inner.set_object_name(raw, desc.label);
        Ok(self
            .inner
            .stamp(self.inner.state().query_sets.insert(QuerySetEntry {
                owner: self.inner.id,
                raw,
                count: desc.count,
                kind: desc.kind,
            })))
    }

    fn destroy_query_set(&self, set: QuerySetHandle) {
        self.retire_from(
            |state| &mut state.query_sets,
            set,
            |entry| Trash::QueryPool(entry.raw),
        );
    }

    fn query_results(
        &self,
        set: QuerySetHandle,
        first_query: u32,
        out: &mut [u64],
    ) -> Result<(), HalError> {
        let state = self.inner.state();
        let entry = lookup(&state.query_sets, "query set", set, &self.inner)?;
        let end = first_query as u64 + out.len() as u64;
        if end > u64::from(entry.count) {
            return Err(HalError::InvalidDescriptor(format!(
                "query range {first_query}..{end} exceeds the set's {} queries",
                entry.count
            )));
        }
        if out.is_empty() {
            return Ok(());
        }
        #[allow(clippy::cast_possible_truncation)]
        // SAFETY: the range was bounds-checked against the pool's query count,
        // and `out` is a live slice of exactly that many `u64`s.
        let result = unsafe {
            self.inner.raw.get_query_pool_results(
                entry.raw,
                first_query,
                out,
                vk::QueryResultFlags::TYPE_64,
            )
        };
        match result {
            Ok(()) => {
                // The seam reports a timestamp in nanoseconds and Vulkan counts
                // ticks, so the period is spent here — the one place that knows
                // it. The other kinds count occurrences and have no unit to
                // convert. `resolve_query_set` writes the raw ticks instead,
                // which its own seam documentation says and which this backend
                // cannot change: `vkCmdCopyQueryPoolResults` never reaches the
                // CPU.
                if entry.kind == QueryKind::Timestamp {
                    for value in out.iter_mut() {
                        *value = conv::timestamp_nanos(*value, self.inner.timestamp_period_ns);
                    }
                }
                Ok(())
            }
            // A query that has not been written yet is not an error: the seam
            // says the profiler HUD degrades rather than breaks.
            Err(vk::Result::NOT_READY) => {
                out.fill(0);
                Ok(())
            }
            Err(error) => Err(conv::hal_error("vkGetQueryPoolResults", error)),
        }
    }

    // --- synchronisation ---

    fn create_semaphore(&self, desc: &SemaphoreDesc<'_>) -> Result<SemaphoreHandle, HalError> {
        let timeline = matches!(desc.kind, SemaphoreKind::Timeline { .. });
        // **The declaration has to be true, and until now it was not.** `supports`
        // answers `Capability::TimelineSemaphore` through `Features::TIMELINE_SEMAPHORE`,
        // so a device opened without it declares timelines unsupported — and this
        // call built one anyway, because `vkCreateSemaphore` with a
        // `TIMELINE` type does not consult what the device was opened with.
        //
        // Found by running the seam suite with `CRCBL_SEAM_WITHHOLD=all`, which
        // is the only configuration where the two disagree: every device CI
        // opens has the feature, so the wide run cannot reach this.
        if timeline
            && !self
                .inner
                .caps
                .features
                .contains(Features::TIMELINE_SEMAPHORE)
        {
            return Err(not_yet(
                "a timeline semaphore on a device opened without Features::TIMELINE_SEMAPHORE",
            ));
        }
        let initial = match desc.kind {
            SemaphoreKind::Timeline { initial_value } => initial_value,
            SemaphoreKind::Binary => 0,
        };
        let mut type_info = vk::SemaphoreTypeCreateInfo::default()
            .semaphore_type(if timeline {
                vk::SemaphoreType::TIMELINE
            } else {
                vk::SemaphoreType::BINARY
            })
            .initial_value(initial);
        let info = vk::SemaphoreCreateInfo::default().push_next(&mut type_info);
        // SAFETY: `info` borrows only locals that outlive the call.
        let raw = unsafe { self.inner.raw.create_semaphore(&info, None) }
            .map_err(|error| conv::hal_error("vkCreateSemaphore", error))?;
        self.inner.set_object_name(raw, desc.label);
        Ok(self
            .inner
            .stamp(self.inner.state().semaphores.insert(SemaphoreEntry {
                owner: self.inner.id,
                raw,
                timeline,
                swapchain_owned: false,
            })))
    }

    fn destroy_semaphore(&self, semaphore: SemaphoreHandle) {
        let mut state = self.inner.state();
        // A swapchain's semaphores are handed out through `AcquiredFrame` and
        // are the swapchain's to destroy; a caller passing one here gets the
        // handle removed and nothing else, rather than a dangling swapchain.
        if local_handle("semaphore", semaphore, &self.inner)
            .ok()
            .and_then(|local| state.semaphores.get(local))
            .is_some_and(|entry| entry.swapchain_owned)
        {
            return;
        }
        let Some(entry) = take_owned(&mut state.semaphores, semaphore, &self.inner) else {
            return;
        };
        self.inner.park(&mut state, Trash::Semaphore(entry.raw));
    }

    fn semaphore_value(&self, semaphore: SemaphoreHandle) -> Result<u64, HalError> {
        let state = self.inner.state();
        let entry = lookup(&state.semaphores, "semaphore", semaphore, &self.inner)?;
        if !entry.timeline {
            return Err(HalError::Unsupported {
                backend: BackendKind::Vulkan,
                what: "a binary semaphore has no value to read",
            });
        }
        // SAFETY: `entry.raw` is a live timeline semaphore of this device.
        unsafe { self.inner.raw.get_semaphore_counter_value(entry.raw) }
            .map_err(|error| conv::hal_error("vkGetSemaphoreCounterValue", error))
    }

    /// `vkSignalSemaphore`, with the seam's forwards-only rule checked first.
    ///
    /// Vulkan requires the rule itself — the layers answer a value at or below
    /// the counter with `VUID-VkSemaphoreSignalInfo-value-03258`, seen from this
    /// crate while red-checking the seam suite's exercise — but a validation
    /// message is not a `Result`, and on a build without layers there is nothing
    /// at all. So the counter is read and compared here,
    /// which is what makes the refusal the same *returned* error a caller sees
    /// on D3D12 and Metal, where the API would set the value backwards in
    /// silence.
    ///
    /// # Errors
    ///
    /// [`HalError::InvalidHandle`]; [`HalError::Unsupported`] for a binary
    /// semaphore, which has no value to signal; and
    /// [`HalError::InvalidDescriptor`] for a value that does not exceed the one
    /// the timeline already holds.
    fn signal_semaphore(&self, semaphore: SemaphoreHandle, value: u64) -> Result<(), HalError> {
        let state = self.inner.state();
        let entry = lookup(&state.semaphores, "semaphore", semaphore, &self.inner)?;
        if !entry.timeline {
            return Err(HalError::Unsupported {
                backend: BackendKind::Vulkan,
                what: "a binary semaphore has no value to signal",
            });
        }
        // SAFETY: `entry.raw` is a live timeline semaphore of this device.
        let held = unsafe { self.inner.raw.get_semaphore_counter_value(entry.raw) }
            .map_err(|error| conv::hal_error("vkGetSemaphoreCounterValue", error))?;
        if value <= held {
            return Err(HalError::InvalidDescriptor(format!(
                "a timeline semaphore signalled with {value} already holds {held}; a timeline only \
                 moves forwards and a waiter on the higher value would never wake"
            )));
        }
        let info = vk::SemaphoreSignalInfo::default()
            .semaphore(entry.raw)
            .value(value);
        // SAFETY: `info` borrows only locals that outlive the call and names a
        // live timeline semaphore of this device.
        unsafe { self.inner.raw.signal_semaphore(&info) }
            .map_err(|error| conv::hal_error("vkSignalSemaphore", error))
    }

    fn wait_semaphores(&self, waits: &[SemaphoreWait], timeout_ns: u64) -> Result<bool, HalError> {
        if waits.is_empty() {
            return Ok(true);
        }
        let mut state = self.inner.state();
        let mut semaphores = Vec::with_capacity(waits.len());
        let mut values = Vec::with_capacity(waits.len());
        for wait in waits {
            let entry = lookup(&state.semaphores, "semaphore", wait.semaphore, &self.inner)?;
            if !entry.timeline {
                return Err(HalError::Unsupported {
                    backend: BackendKind::Vulkan,
                    what: "a binary semaphore cannot be waited on from the CPU",
                });
            }
            semaphores.push(entry.raw);
            values.push(wait.value);
        }
        let info = vk::SemaphoreWaitInfo::default()
            .semaphores(&semaphores)
            .values(&values);
        // SAFETY: both arrays are the same length and name live timeline
        // semaphores of this device.
        let result = unsafe { self.inner.raw.wait_semaphores(&info, timeout_ns) };
        match result {
            Ok(()) => {
                // A satisfied wait is the cheapest possible moment to free what
                // the GPU has finished with.
                self.inner.poll_retire(&mut state);
                Ok(true)
            }
            // Not an error: a frame-pacing poll times out routinely.
            Err(vk::Result::TIMEOUT) => Ok(false),
            Err(error) => Err(conv::hal_error("vkWaitSemaphores", error)),
        }
    }

    fn wait_idle(&self) -> Result<(), HalError> {
        // SAFETY: `raw` is a live device; every queue is externally
        // synchronised by the state lock, which is taken immediately after.
        unsafe { self.inner.raw.device_wait_idle() }
            .map_err(|error| conv::hal_error("vkDeviceWaitIdle", error))?;
        let mut state = self.inner.state();
        self.inner.poll_retire(&mut state);
        Ok(())
    }

    // --- commands ---

    fn create_command_encoder(&self, desc: &CommandEncoderDesc<'_>) -> Box<dyn CommandEncoder> {
        Box::new(VkCommandEncoder::new(Arc::clone(&self.inner), desc))
    }

    fn destroy_command_buffer(&self, buffer: CommandBufferHandle) {
        let mut state = self.inner.state();
        let Some(entry) = take_owned(&mut state.command_buffers, buffer, &self.inner) else {
            return;
        };
        // The one object freed inline rather than parked: the seam says this
        // "must not be called until the submission that used it has completed",
        // so the caller has already done the waiting the deletion queue exists
        // to avoid.
        // SAFETY: the pool was created by this device, holds only this buffer,
        // and the caller has guaranteed the submission using it is done.
        unsafe { self.inner.raw.destroy_command_pool(entry.pool, None) };
    }

    fn submit(&self, queue: QueueHandle, submit: &SubmitInfo<'_>) -> Result<(), HalError> {
        let slot = self.inner.queue_slot(queue)?;
        let mut state = self.inner.state();

        let mut commands = Vec::with_capacity(submit.command_buffers.len());
        for handle in submit.command_buffers {
            let entry = lookup(
                &state.command_buffers,
                "command buffer",
                *handle,
                &self.inner,
            )?;
            commands.push(
                vk::CommandBufferSubmitInfo::default()
                    .command_buffer(entry.raw)
                    .device_mask(0),
            );
        }

        let mut waits = Vec::with_capacity(submit.waits.len());
        for wait in submit.waits {
            let entry = lookup(&state.semaphores, "semaphore", wait.semaphore, &self.inner)?;
            waits.push(
                vk::SemaphoreSubmitInfo::default()
                    .semaphore(entry.raw)
                    // Ignored for a binary semaphore, and the seam says so.
                    .value(if entry.timeline { wait.value } else { 0 })
                    // The seam deliberately does not expose per-stage wait
                    // masks (`crcbl-hal`'s sync module says why: Metal cannot
                    // express them and the graph splits submissions instead),
                    // so the conservative mask is the only correct one.
                    .stage_mask(vk::PipelineStageFlags2::ALL_COMMANDS),
            );
        }

        // Read, not incremented. The counter must only move once the
        // submission that signals this value is *actually in flight*: the
        // lookups below `?`-return and `vkQueueSubmit2` can fail, and a counter
        // bumped past a value nothing will ever signal leaves `poll_retire`
        // stuck below every later-parked object forever — the deletion queue
        // never drains again, and every `request_readback` without an explicit
        // wait returns `Pending` for the rest of the process's life. The state
        // lock serialises `submit` against itself, so reading here and
        // committing after the driver call is safe.
        let value = self.inner.submissions() + 1;
        // A recorded command buffer may use driver objects that were destroyed
        // after recording (the seam's record → destroy → submit order). Each such
        // object parked in the deletion queue must stay parked until THIS
        // submission completes, or it is freed when an earlier submission finishes
        // while this one still runs. Matching is by raw handle, which is exact:
        // a parked object is still allocated in the driver, so its raw value has
        // not been reused by a new object.
        for handle in submit.command_buffers {
            let entry = lookup(
                &state.command_buffers,
                "command buffer",
                *handle,
                &self.inner,
            )?;
            // Cloned so the borrow of `entry` (through the state lock) ends
            // before the queue is mutated below — the two are disjoint fields
            // of the guarded state, which a `Deref` cannot see.
            let references = entry.references.clone();
            for raw in &references {
                state
                    .trash
                    .extend_matching(value, |item| trash_raw(item) == *raw);
            }
        }
        let mut signals = Vec::with_capacity(submit.signals.len() + 1);
        for signal in submit.signals {
            let entry = lookup(
                &state.semaphores,
                "semaphore",
                signal.semaphore,
                &self.inner,
            )?;
            signals.push(
                vk::SemaphoreSubmitInfo::default()
                    .semaphore(entry.raw)
                    .value(if entry.timeline { signal.value } else { 0 })
                    .stage_mask(vk::PipelineStageFlags2::ALL_COMMANDS),
            );
        }
        // Always last: the deletion queue's clock, and the completion point a
        // readback without an explicit wait watches. Appending it here rather
        // than asking callers for it is what makes `destroy_*` mean "the handle
        // is dead now" with no cooperation from above the seam.
        signals.push(
            vk::SemaphoreSubmitInfo::default()
                .semaphore(self.inner.retire_timeline)
                .value(value)
                .stage_mask(vk::PipelineStageFlags2::ALL_COMMANDS),
        );

        let info = vk::SubmitInfo2::default()
            .wait_semaphore_infos(&waits)
            .command_buffer_infos(&commands)
            .signal_semaphore_infos(&signals);
        // SAFETY: `slot.raw` is a live queue of this device, externally
        // synchronised by the state lock held here, and every handle in `info`
        // was resolved against this device's tables above.
        unsafe {
            self.inner
                .raw
                .queue_submit2(slot.raw, &[info], vk::Fence::null())
        }
        .map_err(|error| conv::hal_error("vkQueueSubmit2", error))?;
        // Committed only now: the submission is in flight, so the retire
        // timeline *will* reach `value`.
        self.inner.submissions.store(value, Ordering::Release);

        // And only now do these recordings stop holding what they reference in
        // the deletion queue: the extend above pinned their objects to `value`,
        // which the timeline will reach. Marked after the driver call for the
        // same reason the counter is committed after it — a submission that was
        // refused never runs, so its command buffers go on holding.
        for handle in submit.command_buffers {
            let entry = lookup_mut(
                &mut state.command_buffers,
                "command buffer",
                *handle,
                &self.inner,
            )
            .unwrap_or_else(|_| unreachable!("resolved twice above under this lock"));
            entry.submitted = true;
        }

        self.inner.poll_retire(&mut state);
        Ok(())
    }

    // --- presentation ---

    fn create_swapchain(&self, desc: &SwapchainDesc<'_>) -> Result<SwapchainHandle, SurfaceError> {
        let surface_raw = self.inner.instance.surface_raw(desc.surface)?;
        self.inner.instance.retain_surface(desc.surface)?;
        let built = match self.build_swapchain(desc, surface_raw, vk::SwapchainKHR::null()) {
            Ok(built) => built,
            Err(error) => {
                self.inner.instance.release_surface(surface_raw);
                return Err(error);
            }
        };
        let handle = self.inner.state().swapchains.insert(built);
        Ok(self.inner.stamp(handle))
    }

    fn reconfigure_swapchain(
        &self,
        swapchain: SwapchainHandle,
        desc: &SwapchainDesc<'_>,
    ) -> Result<(), SurfaceError> {
        // Reconfigure, never destroy-and-recreate: the handle stays valid
        // across a resize storm, which is what the seam promises callers.
        let (old_raw, old_surface) = {
            let state = self.inner.state();
            let entry = lookup(&state.swapchains, "swapchain", swapchain, &self.inner)?;
            (entry.raw, entry.surface_raw)
        };
        let surface_raw = self.inner.instance.surface_raw(desc.surface)?;
        if surface_raw != old_surface {
            return Err(SurfaceError::Hal(HalError::InvalidDescriptor(
                "reconfigure_swapchain cannot move a swapchain to a different surface".to_string(),
            )));
        }
        // The new swapchain takes its own reference on the surface, because the
        // old one gives *its* back when it is retired below. Without this the
        // count reaches zero while a live swapchain is still configured, and
        // the next `destroy_surface` calls `vkDestroySurfaceKHR` underneath it
        // — which validation reports as
        // `VUID-vkDestroySurfaceKHR-surface-01266`, and which is undefined
        // behaviour in the driver. Found on the first resize under sway.
        self.inner.instance.retain_surface(desc.surface)?;
        // The old swapchain is handed to the new one so the driver can reuse
        // its images and keep presenting until the handoff completes; it is
        // retired below, once the device is idle — see `retire_swapchain` for
        // why a swapchain cannot go through the deletion queue.
        let built = match self.build_swapchain(desc, surface_raw, old_raw) {
            Ok(built) => built,
            Err(error) => {
                self.inner.instance.release_surface(surface_raw);
                return Err(error);
            }
        };

        let mut state = self.inner.state();
        let entry = match lookup_mut(&mut state.swapchains, "swapchain", swapchain, &self.inner) {
            Ok(entry) => entry,
            Err(error) => {
                // The lock was released while `built` was under construction,
                // so a concurrent `destroy_swapchain` can land here. `built` is
                // complete and has no `Drop`, and its rows are all
                // `swapchain_owned` — which `Drop for DeviceInner` skips — so
                // returning without undoing it would leak the swapchain, its
                // views, its semaphores and its fences past `vkDestroyDevice`.
                forget_swapchain_rows(&self.inner, &mut state, &built);
                drop(state);
                self.inner.retire_swapchain(built);
                return Err(error.into());
            }
        };
        let previous = core::mem::replace(entry, built);
        // Invalidating the old handles is the point: a caller holding an image
        // or a view across a resize gets `InvalidHandle`, not a stale object.
        forget_swapchain_rows(&self.inner, &mut state, &previous);
        // Everything else about the old configuration retires on the timeline
        // as usual; the swapchain itself cannot. See `retire_swapchain`.
        drop(state);
        self.inner.retire_swapchain(previous);
        Ok(())
    }

    fn destroy_swapchain(&self, swapchain: SwapchainHandle) {
        let mut state = self.inner.state();
        let Some(entry) = take_owned(&mut state.swapchains, swapchain, &self.inner) else {
            return;
        };
        forget_swapchain_rows(&self.inner, &mut state, &entry);
        drop(state);
        self.inner.retire_swapchain(entry);
    }

    fn acquire_next_frame(
        &self,
        swapchain: SwapchainHandle,
    ) -> Result<AcquiredFrame, SurfaceError> {
        let mut state = self.inner.state();
        self.inner.poll_retire(&mut state);
        let inner = Arc::clone(&self.inner);
        let entry = lookup_mut(&mut state.swapchains, "swapchain", swapchain, &inner)?;

        if entry.is_offscreen() {
            // The implicit-acquire shape, which is also `crcbl-wgpu`'s: no
            // semaphores, and the caller's `Option::as_slice()` splices nothing.
            // The reuse dependency is not established here. Blocking the host
            // until the previous trip round the ring retired was tried and is
            // the wrong tool twice over: it costs exactly the frame overlap the
            // ring exists to provide, and it is not a queue dependency, so the
            // validation layer goes on reporting the hazard — a CPU wait is
            // invisible to a model that reasons about submitted commands. The
            // dependency belongs in the command stream, and `pipeline_barrier`
            // puts it there when it widens an `Undefined` source on a
            // `ring_reuse` image.
            let index = entry.next_offscreen;
            entry.acquired = Some(index);
            return Ok(entry.frame(index, None, false));
        }

        let raw = entry.raw;
        let sync = entry
            .sync
            .as_mut()
            .unwrap_or_else(|| unreachable!("a windowed swapchain always has sync"));
        let slot = sync.next_slot;
        let fence = sync.acquire_fence[slot];
        let semaphore = sync.acquire[slot];
        // Reusing an acquire semaphore while its acquire is still pending is a
        // validation error, and the classic hand-rolled-swapchain bug. The
        // fence is what makes reuse provably safe rather than probably safe.
        //
        // It is only waited on when this slot's previous acquire actually
        // *armed* it. A failed `vkAcquireNextImageKHR` signals neither the
        // semaphore nor the fence, so an unconditional wait on a slot whose
        // acquire failed would block forever — while holding the device lock,
        // which deadlocks everything else. That is why the slot cursor also
        // only advances on success.
        if sync.acquire_armed[slot] {
            // SAFETY: `fence` is a live fence of this device, armed by this
            // slot's previous successful acquire.
            unsafe {
                let _ = self.inner.raw.wait_for_fences(&[fence], true, u64::MAX);
                let _ = self.inner.raw.reset_fences(&[fence]);
            }
            sync.acquire_armed[slot] = false;
        }

        // SAFETY: `raw` is a live swapchain of this device, and the semaphore
        // and fence belong to its own ring.
        let acquired = unsafe {
            self.inner
                .swapchain_ext
                .acquire_next_image(raw, u64::MAX, semaphore, fence)
        };
        let (index, suboptimal) = match acquired {
            Ok(result) => result,
            // The slot is untouched — neither its semaphore nor its fence was
            // signalled — so it is reused as-is next time rather than skipped.
            Err(error) => return Err(conv::surface_error("vkAcquireNextImageKHR", error)),
        };
        let sync = entry
            .sync
            .as_mut()
            .unwrap_or_else(|| unreachable!("a windowed swapchain always has sync"));
        sync.acquire_armed[slot] = true;
        sync.next_slot = (slot + 1) % sync.acquire.len();
        entry.acquired = Some(index);
        let suboptimal = suboptimal || core::mem::take(&mut entry.pending_suboptimal);
        Ok(entry.frame(index, Some(slot), suboptimal))
    }

    fn present(&self, queue: QueueHandle, present: &PresentInfo<'_>) -> Result<(), SurfaceError> {
        let slot = self.inner.queue_slot(queue)?;
        let mut state = self.inner.state();
        let inner = Arc::clone(&self.inner);

        let mut waits = Vec::with_capacity(present.waits.len());
        for handle in present.waits {
            let entry = lookup(&state.semaphores, "semaphore", *handle, &inner)?;
            waits.push(entry.raw);
        }

        let entry = lookup_mut(
            &mut state.swapchains,
            "swapchain",
            present.swapchain,
            &inner,
        )?;
        let Some(index) = entry.acquired.take() else {
            return Err(SurfaceError::Hal(HalError::InvalidDescriptor(
                "present without a matching acquire_next_frame".to_string(),
            )));
        };
        if entry.is_offscreen() {
            // "Presenting" a ring image is advancing the ring. The image stays
            // valid and is reused when the cursor comes back round, exactly as
            // a real swapchain image is.
            #[allow(clippy::cast_possible_truncation)]
            {
                entry.next_offscreen = (index + 1) % entry.images.len() as u32;
            }
            return Ok(());
        }

        let swapchains = [entry.raw];
        let indices = [index];
        // Numbering the present is the only thing that makes
        // `wait_until_presented` able to answer about it, and it is chained
        // only when the capability is live and the number is one Vulkan will
        // accept. `VUID-VkPresentIdKHR-presentIds-04999` requires ids to
        // **strictly increase** for a swapchain, so an id that does not is
        // dropped rather than chained: the alternative is a validation error on
        // a caller's bookkeeping slip, and an unnumbered present is exactly the
        // "no record" case the wait already answers immediately.
        let requested_id = present.present_id.unwrap_or(0);
        let can_number = self.inner.present_wait_ext.is_some();
        let numbered = can_number && requested_id > entry.presented_id;
        if can_number && requested_id != 0 && !numbered {
            crcbl_core::log::warn!(
                "crcbl-vk: present id {requested_id} does not follow {}; presenting unnumbered",
                entry.presented_id
            );
        }
        // Named locals, as everywhere a builder stores a pointer.
        let present_ids = [requested_id];
        let mut present_id = vk::PresentIdKHR::default().present_ids(&present_ids);
        let mut info = vk::PresentInfoKHR::default()
            .wait_semaphores(&waits)
            .swapchains(&swapchains)
            .image_indices(&indices);
        if numbered {
            info = info.push_next(&mut present_id);
        }
        // SAFETY: `slot.raw` is a live queue externally synchronised by the
        // state lock, the swapchain is live, `index` came from this
        // swapchain's own acquire, and the semaphores were resolved above.
        let result = unsafe { self.inner.swapchain_ext.queue_present(slot.raw, &info) };
        // Only a present the driver **accepted** counts. One that failed with
        // `OutOfDate` never reaches the display, so recording its id would be
        // promising a frame that will never arrive — which is the wait that
        // blocks for a whole timeout.
        if numbered && result.is_ok() {
            entry.record_presented(requested_id);
        }
        match result {
            Ok(false) => Ok(()),
            // A suboptimal present is not an error and there is nowhere in the
            // seam to return it from here — so it is remembered and reported by
            // the *next* acquire, which is where the caller already handles it.
            Ok(true) => {
                entry.pending_suboptimal = true;
                Ok(())
            }
            Err(error) => Err(conv::surface_error("vkQueuePresentKHR", error)),
        }
    }

    /// `vkWaitForPresentKHR`, on a device that has
    /// [`Features::PRESENT_FEEDBACK`](crcbl_hal::Features::PRESENT_FEEDBACK),
    /// and an immediate `Ok(())` on one that does not.
    ///
    /// Three things answer at once rather than blocking, and each is the seam's
    /// documented answer rather than a shortcut: a device without the
    /// extensions has nothing to wait on; an offscreen ring's "present" is a
    /// cursor bump with no display behind it and no `VkSwapchainKHR` to name;
    /// and an id this swapchain object was never given — one whose present
    /// failed, or one from before a reconfigure — names a frame that will never
    /// arrive, which `vkWaitForPresentKHR` would sit out the whole timeout for.
    fn wait_until_presented(
        &self,
        swapchain: SwapchainHandle,
        present_id: u64,
        timeout: Duration,
    ) -> Result<(), SurfaceError> {
        let mut state = self.inner.state();
        let inner = Arc::clone(&self.inner);
        let entry = lookup_mut(&mut state.swapchains, "swapchain", swapchain, &inner)?;
        let Some(present_wait) = self.inner.present_wait_ext.as_ref() else {
            return Ok(());
        };
        if entry.is_offscreen() || !entry.has_presented(present_id) {
            return Ok(());
        }
        let raw = entry.raw;
        self.inner.first_present_wait.call_once(|| {
            crcbl_core::log::info!(
                "crcbl-vk: vkWaitForPresentKHR on present {present_id}; the loop is closed"
            );
        });
        // SAFETY: `raw` is a live swapchain of this device and `present_id` was
        // chained onto a `vkQueuePresentKHR` this same object accepted.
        // `vkWaitForPresentKHR` requires the swapchain to be externally
        // synchronised, and the state lock — held across the whole call — is
        // this backend's external synchronisation for swapchains; it is also
        // what stops a concurrent `destroy_swapchain` or `reconfigure_swapchain`
        // from freeing `raw` underneath the wait. `acquire_next_frame` already
        // blocks under this same lock with an infinite timeout.
        let result =
            unsafe { present_wait.wait_for_present(raw, present_id, present_wait_ns(timeout)) };
        match result {
            Ok(()) => Ok(()),
            // A success code, not a failure: the frame is up and the swapchain
            // merely wants rebuilding. It goes where a suboptimal *present*
            // goes — folded into the next acquire — because this call's whole
            // contract is "the numbered present is no longer waiting to
            // happen", and it is not.
            Err(vk::Result::SUBOPTIMAL_KHR) => {
                entry.pending_suboptimal = true;
                Ok(())
            }
            Err(error) => Err(conv::surface_error("vkWaitForPresentKHR", error)),
        }
    }

    /// `vkGetSwapchainTimingPropertiesEXT`, on a device that has
    /// [`Features::PRESENT_TIMING`](crcbl_hal::Features::PRESENT_TIMING), and
    /// `Ok(DisplayTiming::Unknown)` on one that does not.
    ///
    /// Two things answer without reaching the driver, and each is the seam's
    /// documented answer rather than a shortcut: a device without the extension
    /// chain has nothing to ask, and an offscreen ring is a set of images with
    /// no `VkSwapchainKHR` to name and no display behind it to have a cadence.
    /// The handle is resolved before either, so a foreign or destroyed
    /// swapchain is still the `ForeignObject`/`InvalidHandle` the seam's
    /// obligation 3 requires.
    ///
    /// Read under the state lock and not cached: the answer is driver-side
    /// state that changes with the panel, so it is queried afresh each call.
    ///
    /// **The lock is required, not merely convenient.** `vk.xml` marks this
    /// command's `swapchain` parameter `externsync="true"`, exactly as it marks
    /// `vkWaitForPresentKHR`'s, so the swapchain must be externally
    /// synchronised for the duration of the call — and the state lock is this
    /// backend's external synchronisation for swapchains. It is also what stops
    /// a concurrent `destroy_swapchain` or `reconfigure_swapchain` from freeing
    /// the handle underneath the query.
    fn display_timing(&self, swapchain: SwapchainHandle) -> Result<DisplayTiming, SurfaceError> {
        // Held across the driver call below, which is the `externsync`
        // obligation above; dropping it early would be a data race the compiler
        // cannot see, because `raw` is a plain `Copy` handle.
        let mut state = self.inner.state();
        let inner = Arc::clone(&self.inner);
        let entry = lookup_mut(&mut state.swapchains, "swapchain", swapchain, &inner)?;
        let Some(present_timing) = self.inner.present_timing_ext.as_ref() else {
            return Ok(DisplayTiming::Unknown);
        };
        if entry.is_offscreen() {
            return Ok(DisplayTiming::Unknown);
        }
        let raw = entry.raw;
        let timing = present_timing
            .swapchain_timing(raw)
            .map_err(|error| conv::surface_error("vkGetSwapchainTimingPropertiesEXT", error))?;
        self.inner.first_display_timing.call_once(|| {
            crcbl_core::log::info!("crcbl-vk: vkGetSwapchainTimingPropertiesEXT says {timing:?}");
        });
        Ok(timing)
    }
}

/// A seam [`Duration`] as the nanosecond timeout `vkWaitForPresentKHR` takes.
///
/// Saturating, and the saturation is not a rounding detail: `u64::MAX`
/// nanoseconds is the spec's spelling of "no timeout at all", and it is also
/// what any `Duration` longer than ~584 years means in practice. A `Duration`
/// that overflowed into a small number would turn a caller asking to wait
/// forever into one that gives up immediately, which is the opposite answer.
#[must_use]
fn present_wait_ns(timeout: Duration) -> u64 {
    u64::try_from(timeout.as_nanos()).unwrap_or(u64::MAX)
}

impl VkDevice {
    /// Builds a swapchain — WSI or offscreen ring — without touching the tables
    /// the caller will store it in.
    fn build_swapchain(
        &self,
        desc: &SwapchainDesc<'_>,
        surface_raw: vk::SurfaceKHR,
        old: vk::SwapchainKHR,
    ) -> Result<SwapchainEntry, SurfaceError> {
        if surface_raw == vk::SurfaceKHR::null() {
            return self.build_offscreen_ring(desc);
        }
        let Some(surface_ext) = self.inner.instance.surface_ext.as_ref() else {
            return Err(SurfaceError::Hal(HalError::Unsupported {
                backend: BackendKind::Vulkan,
                what: "VK_KHR_surface is not available",
            }));
        };
        // SAFETY: both handles came from this instance and are live.
        let capabilities = unsafe {
            surface_ext.get_physical_device_surface_capabilities(self.inner.physical, surface_raw)
        }
        .map_err(|error| conv::surface_error("vkGetPhysicalDeviceSurfaceCapabilitiesKHR", error))?;

        // The extent rule's raw inputs, in one line, because "what did the
        // surface actually say?" is the first question every swapchain-sizing
        // bug raises — and the answer differs structurally between Wayland (no
        // opinion, wide range) and X11 (a real size, and a range pinned to it).
        crcbl_core::log::debug!(
            "crcbl-vk: surface extents — current {:?}, min {:?}, max {:?}; shell asked for {:?}",
            swapchain::resolve_current_extent(capabilities.current_extent),
            (
                capabilities.min_image_extent.width,
                capabilities.min_image_extent.height
            ),
            (
                capabilities.max_image_extent.width,
                capabilities.max_image_extent.height
            ),
            desc.extent,
        );

        let extent = swapchain::resolve_swapchain_extent(desc.extent, &capabilities)?;
        if extent.clamped {
            // The seam's obligation 1 says the shell's size is authoritative;
            // Vulkan says `imageExtent` must be inside the surface's permitted
            // range, and on X11 that range is `currentExtent` exactly. When
            // they disagree there is no legal swapchain at the requested size,
            // so the clamp is forced — and is never silent.
            crcbl_core::log::warn!(
                "crcbl-vk: the shell asked for {:?} but the surface permits only \
                 {:?}..={:?} (currentExtent {:?}); configuring at {:?}. The shell's \
                 size is authoritative per the seam, so this means the two are out \
                 of step — usually a resize event still in flight.",
                desc.extent,
                (
                    capabilities.min_image_extent.width,
                    capabilities.min_image_extent.height
                ),
                (
                    capabilities.max_image_extent.width,
                    capabilities.max_image_extent.height
                ),
                swapchain::resolve_current_extent(capabilities.current_extent),
                extent.configured,
            );
        }

        // SAFETY: as above.
        let modes = unsafe {
            surface_ext.get_physical_device_surface_present_modes(self.inner.physical, surface_raw)
        }
        .map_err(|error| conv::surface_error("vkGetPhysicalDeviceSurfacePresentModesKHR", error))?;
        let wanted = conv::present_mode(desc.present_mode);
        let present_mode = if modes.contains(&wanted) {
            wanted
        } else {
            // The seam says the backend falls back to `Fifo`, which every
            // surface supports.
            crcbl_core::log::debug!(
                "crcbl-vk: {:?} is unavailable on this surface; falling back to Fifo",
                desc.present_mode
            );
            vk::PresentModeKHR::FIFO
        };
        let image_count = swapchain::resolve_image_count(desc.image_count, &capabilities);

        let info = swapchain::swapchain_create_info(
            surface_raw,
            desc,
            &capabilities,
            extent.configured,
            image_count,
            present_mode,
            old,
        );
        // SAFETY: `info` names a live surface of this device's instance and
        // borrows nothing beyond the call.
        let raw = unsafe { self.inner.swapchain_ext.create_swapchain(&info, None) }
            .map_err(|error| conv::surface_error("vkCreateSwapchainKHR", error))?;
        self.inner.set_object_name(raw, desc.label);

        // SAFETY: `raw` was just created by this device.
        let images = unsafe { self.inner.swapchain_ext.get_swapchain_images(raw) }
            .map_err(|error| conv::surface_error("vkGetSwapchainImagesKHR", error))?;

        let mut state = self.inner.state();
        let image_handles: Vec<ImageHandle> = images
            .iter()
            .map(|image| {
                self.inner.stamp(state.images.insert(ImageEntry {
                    owner: self.inner.id,
                    raw: *image,
                    memory: vk::DeviceMemory::null(),
                    format: desc.format,
                    swapchain_owned: true,
                    // A WSI image's acquire semaphore already carries it.
                    ring_reuse: false,
                }))
            })
            .collect();
        // These rows are `swapchain_owned`, so `destroy_image` /
        // `destroy_image_view` refuse them and `Drop for DeviceInner` skips
        // them — nothing would ever reclaim them, and the `VkSwapchainKHR` they
        // name would outlive the device. So every failure below undoes them by
        // hand.
        let unwind = |state: &mut DeviceState,
                      image_handles: &[ImageHandle],
                      views: &[vk::ImageView],
                      view_handles: &[ImageViewHandle]| {
            for handle in image_handles {
                take_owned(&mut state.images, *handle, &self.inner);
            }
            for handle in view_handles {
                take_owned(&mut state.views, *handle, &self.inner);
            }
            // SAFETY: every view was created moments ago by this device, has
            // never been used, and is destroyed exactly once.
            unsafe {
                for view in views {
                    self.inner.raw.destroy_image_view(*view, None);
                }
                // SAFETY: the swapchain was created moments ago, nothing has
                // been acquired from it, and it is destroyed exactly once.
                self.inner.swapchain_ext.destroy_swapchain(raw, None);
            }
        };

        let (views, view_handles) =
            match self.build_views(&mut state, &images, desc.format, desc.label) {
                Ok(views) => views,
                Err(error) => {
                    unwind(&mut state, &image_handles, &[], &[]);
                    return Err(error);
                }
            };
        let sync = match self.build_sync(&mut state, images.len()) {
            Ok(sync) => sync,
            Err(error) => {
                unwind(&mut state, &image_handles, &views, &view_handles);
                return Err(error);
            }
        };
        drop(state);

        crcbl_core::log::info!(
            "crcbl-vk: swapchain {}x{} {:?} {present_mode:?}, {} image(s)",
            extent.configured.0,
            extent.configured.1,
            desc.format,
            images.len()
        );
        Ok(SwapchainEntry {
            surface_raw,
            raw,
            sync: Some(sync),
            ..SwapchainEntry::fresh(
                self.inner.id,
                extent.configured,
                images,
                views,
                view_handles,
                image_handles,
            )
        })
    }

    /// The offscreen ring: a swapchain-shaped rotation of plain images.
    fn build_offscreen_ring(
        &self,
        desc: &SwapchainDesc<'_>,
    ) -> Result<SwapchainEntry, SurfaceError> {
        let caps = swapchain::offscreen_surface_caps();
        let extent = swapchain::resolve_swapchain_extent(
            desc.extent,
            &vk::SurfaceCapabilitiesKHR {
                min_image_extent: vk::Extent2D {
                    width: 1,
                    height: 1,
                },
                max_image_extent: vk::Extent2D {
                    width: self.inner.caps.limits.max_image_2d,
                    height: self.inner.caps.limits.max_image_2d,
                },
                ..Default::default()
            },
        )?;
        let count = desc
            .image_count
            .clamp(caps.min_image_count, caps.max_image_count);

        let info = vk::ImageCreateInfo::default()
            .image_type(vk::ImageType::TYPE_2D)
            .format(conv::format(desc.format))
            .extent(vk::Extent3D {
                width: extent.configured.0,
                height: extent.configured.1,
                depth: 1,
            })
            .mip_levels(1)
            .array_layers(1)
            .samples(vk::SampleCountFlags::TYPE_1)
            .tiling(vk::ImageTiling::OPTIMAL)
            // `TRANSFER_SRC` is what makes this a screenshot target;
            // `SAMPLED` is what will make it a tonemap input at P1.3.
            .usage(
                vk::ImageUsageFlags::COLOR_ATTACHMENT
                    | vk::ImageUsageFlags::TRANSFER_SRC
                    | vk::ImageUsageFlags::TRANSFER_DST
                    | vk::ImageUsageFlags::SAMPLED,
            )
            .sharing_mode(vk::SharingMode::EXCLUSIVE)
            .initial_layout(vk::ImageLayout::UNDEFINED);

        let mut images = Vec::with_capacity(count as usize);
        let mut memory = Vec::with_capacity(count as usize);
        for index in 0..count {
            let label = desc.label.map(|label| format!("{label} [{index}]"));
            match self.inner.create_owned_image(&info, label.as_deref()) {
                Ok((image, block)) => {
                    images.push(image);
                    memory.push(block);
                }
                Err(error) => {
                    // SAFETY: everything created so far belongs to this device
                    // and has never been used.
                    unsafe {
                        for image in images {
                            self.inner.raw.destroy_image(image, None);
                        }
                        for block in memory {
                            self.inner.raw.free_memory(block, None);
                        }
                    }
                    return Err(SurfaceError::Hal(error));
                }
            }
        }

        let mut state = self.inner.state();
        let image_handles: Vec<ImageHandle> = images
            .iter()
            .map(|image| {
                // The ring owns them, so `destroy_image` must not free one —
                // the same rule a WSI image follows, for the same reason. The
                // rule they do *not* share is `ring_reuse`: these come back
                // round with no semaphore to order the reuse.
                self.inner.stamp(state.images.insert(ImageEntry {
                    owner: self.inner.id,
                    ring_reuse: true,
                    raw: *image,
                    memory: vk::DeviceMemory::null(),
                    format: desc.format,
                    swapchain_owned: true,
                }))
            })
            .collect();
        let (views, view_handles) =
            match self.build_views(&mut state, &images, desc.format, desc.label) {
                Ok(views) => views,
                Err(error) => {
                    for handle in &image_handles {
                        take_owned(&mut state.images, *handle, &self.inner);
                    }
                    drop(state);
                    // SAFETY: every image and allocation was created moments
                    // ago by this device and has never been used.
                    unsafe {
                        for image in images {
                            self.inner.raw.destroy_image(image, None);
                        }
                        for block in memory {
                            self.inner.raw.free_memory(block, None);
                        }
                    }
                    return Err(error);
                }
            };
        drop(state);

        crcbl_core::log::info!(
            "crcbl-vk: offscreen ring {}x{} {:?}, {count} image(s)",
            extent.configured.0,
            extent.configured.1,
            desc.format,
        );
        Ok(SwapchainEntry {
            memory,
            ..SwapchainEntry::fresh(
                self.inner.id,
                extent.configured,
                images,
                views,
                view_handles,
                image_handles,
            )
        })
    }

    /// Creates one whole-image view per ring image, and registers them.
    ///
    /// The swapchain owns its images, so it owns their views —
    /// `crcbl-hal`'s swapchain module states why, and the alternative is the
    /// identical per-image cache in every consumer. As in
    /// [`build_sync`](Self::build_sync), every driver object is created before
    /// anything reaches the pools, so a failure part-way through has one thing
    /// to undo.
    fn build_views(
        &self,
        state: &mut DeviceState,
        images: &[vk::Image],
        format: Format,
        label: Option<&str>,
    ) -> Result<(Vec<vk::ImageView>, Vec<ImageViewHandle>), SurfaceError> {
        let mut created: Vec<vk::ImageView> = Vec::with_capacity(images.len());
        for (index, image) in images.iter().enumerate() {
            let info = vk::ImageViewCreateInfo::default()
                .image(*image)
                .view_type(vk::ImageViewType::TYPE_2D)
                .format(conv::format(format))
                .subresource_range(conv::subresource_range(
                    crcbl_hal::ImageSubresourceRange::all(format),
                ));
            // SAFETY: `image` belongs to the swapchain this device just created
            // and `info` borrows nothing beyond the call.
            match unsafe { self.inner.raw.create_image_view(&info, None) } {
                Ok(view) => {
                    self.inner.set_object_name(
                        view,
                        Some(&match label {
                            Some(label) => format!("{label} view [{index}]"),
                            None => format!("crcbl swapchain view [{index}]"),
                        }),
                    );
                    created.push(view);
                }
                Err(error) => {
                    // SAFETY: everything created so far belongs to this device
                    // and has never been used.
                    unsafe {
                        for view in created {
                            self.inner.raw.destroy_image_view(view, None);
                        }
                    }
                    return Err(conv::surface_error("vkCreateImageView (swapchain)", error));
                }
            }
        }

        // Nothing below can fail, so every row is reachable through the entry
        // the caller is about to store.
        let handles = created
            .iter()
            .map(|view| {
                self.inner.stamp(state.views.insert(ViewEntry {
                    owner: self.inner.id,
                    raw: *view,
                    format,
                    swapchain_owned: true,
                }))
            })
            .collect();
        Ok((created, handles))
    }

    /// Creates the per-image and per-slot synchronisation a windowed swapchain
    /// owns.
    fn build_sync(
        &self,
        state: &mut DeviceState,
        image_count: usize,
    ) -> Result<FrameSync, SurfaceError> {
        // One more acquire slot than there are images: the ring must be able to
        // hold an acquire for every image plus the one being started.
        let slots = image_count + 1;
        let mut acquire = Vec::with_capacity(slots);
        let mut acquire_handles = Vec::with_capacity(slots);
        let mut present = Vec::with_capacity(image_count);
        let mut present_handles = Vec::with_capacity(image_count);

        let info = vk::SemaphoreCreateInfo::default();
        // Unsignalled. `acquire_armed` records whether a slot's fence is
        // actually pending, so a signalled-on-create fence would only be a
        // second, less accurate copy of that answer.
        let fence_info = vk::FenceCreateInfo::default();

        // Every driver object is created *before* anything reaches the pools,
        // so a failure part-way through has exactly one thing to undo. The
        // interleaved version leaks: the pool rows are `swapchain_owned`, which
        // `destroy_semaphore` refuses and `Drop for DeviceInner` skips, so
        // nothing would ever reclaim them.
        let mut created: Vec<vk::Semaphore> = Vec::with_capacity(slots + image_count);
        let mut created_fences: Vec<vk::Fence> = Vec::with_capacity(slots);
        let undo = |semaphores: &[vk::Semaphore], fences: &[vk::Fence]| {
            // SAFETY: everything named was created by this device moments ago,
            // has never been used, and is destroyed exactly once.
            unsafe {
                for semaphore in semaphores {
                    self.inner.raw.destroy_semaphore(*semaphore, None);
                }
                for fence in fences {
                    self.inner.raw.destroy_fence(*fence, None);
                }
            }
        };
        for index in 0..(slots + image_count) {
            // SAFETY: `info` is a plain binary-semaphore descriptor.
            let semaphore = match unsafe { self.inner.raw.create_semaphore(&info, None) } {
                Ok(semaphore) => semaphore,
                Err(error) => {
                    undo(&created, &created_fences);
                    return Err(conv::surface_error("vkCreateSemaphore", error));
                }
            };
            created.push(semaphore);
            if index < slots {
                self.inner
                    .set_object_name(semaphore, Some(&format!("crcbl acquire [{index}]")));
                // SAFETY: `fence_info` is a plain fence descriptor.
                match unsafe { self.inner.raw.create_fence(&fence_info, None) } {
                    Ok(fence) => created_fences.push(fence),
                    Err(error) => {
                        undo(&created, &created_fences);
                        return Err(conv::surface_error("vkCreateFence (acquire)", error));
                    }
                }
            } else {
                self.inner.set_object_name(
                    semaphore,
                    Some(&format!("crcbl present [{}]", index - slots)),
                );
            }
        }

        // Nothing below can fail, so every pool row inserted here is reachable
        // through the `SwapchainEntry` the caller is about to store.
        for (index, semaphore) in created.iter().copied().enumerate() {
            let handle = self.inner.stamp(state.semaphores.insert(SemaphoreEntry {
                owner: self.inner.id,
                raw: semaphore,
                timeline: false,
                swapchain_owned: true,
            }));
            if index < slots {
                acquire.push(semaphore);
                acquire_handles.push(handle);
            } else {
                present.push(semaphore);
                present_handles.push(handle);
            }
        }

        Ok(FrameSync {
            acquire,
            acquire_fence: created_fences,
            acquire_armed: vec![false; slots],
            acquire_handles,
            present,
            present_handles,
            next_slot: 0,
        })
    }
}

impl Drop for DeviceInner {
    fn drop(&mut self) {
        // Nothing may be destroyed while the device might still be using it,
        // and at this point nobody is left to have waited.
        // SAFETY: `raw` is a live device.
        let _ = unsafe { self.raw.device_wait_idle() };

        let mut state = self.state();
        // Anything the caller never destroyed is a leak worth naming, but it
        // still has to be freed — the driver's own validation reports it
        // otherwise, drowning the real signal.
        //
        // **Named by kind, because a bare count is not actionable.** This said
        // only "N object(s)" until 2026-08-19, which tells a reader that
        // something leaked and nothing about where to look; the suites that
        // trip it have hundreds of creations between them. The kind is the one
        // piece of information that narrows it, and it is free here.
        let kinds = [
            ("buffer", state.buffers.len()),
            ("image", state.images.len()),
            ("image view", state.views.len()),
            ("semaphore", state.semaphores.len()),
            ("query set", state.query_sets.len()),
            ("command buffer", state.command_buffers.len()),
            ("swapchain", state.swapchains.len()),
            ("shader module", state.shader_modules.len()),
            ("bind group layout", state.bind_group_layouts.len()),
            ("bind group", state.bind_groups.len()),
            ("pipeline layout", state.pipeline_layouts.len()),
            ("pipeline", state.pipelines.len()),
            ("sampler", state.samplers.len()),
        ];
        let live: usize = kinds.iter().map(|(_, count)| count).sum();
        if live > 0 {
            let named = kinds
                .iter()
                .filter(|(_, count)| *count > 0)
                .map(|(kind, count)| format!("{count} {kind}"))
                .collect::<Vec<_>>()
                .join(", ");
            // Images and views carry a format and an ownership flag already, so
            // saying which costs nothing and is usually the whole diagnosis: a
            // depth format is a shadow or a prepass target, and a
            // swapchain-owned one is the ring rather than anything a caller
            // made. Only these two kinds get it because only these two have
            // anything to say — a leaked pipeline layout is just a leaked
            // pipeline layout.
            let mut shapes: Vec<String> = Vec::new();
            for (label, formats) in [
                (
                    "image",
                    state
                        .images
                        .iter()
                        .map(|(_, entry)| (entry.format, entry.swapchain_owned))
                        .collect::<Vec<_>>(),
                ),
                (
                    "image view",
                    state
                        .views
                        .iter()
                        .map(|(_, entry)| (entry.format, entry.swapchain_owned))
                        .collect::<Vec<_>>(),
                ),
            ] {
                let mut seen: Vec<((Format, bool), usize)> = Vec::new();
                for key in formats {
                    match seen.iter_mut().find(|(had, _)| *had == key) {
                        Some((_, count)) => *count += 1,
                        None => seen.push((key, 1)),
                    }
                }
                for ((format, owned), count) in seen {
                    let owner = if owned { ", swapchain-owned" } else { "" };
                    shapes.push(format!("{count} {label} {format:?}{owner}"));
                }
            }
            let shapes = if shapes.is_empty() {
                String::new()
            } else {
                format!("; {}", shapes.join(", "))
            };
            crcbl_core::log::warn!(
                "crcbl-vk: {live} object(s) still alive at device teardown \
                 ({named}{shapes}), and {} still parked in the deletion queue",
                state.trash.pending()
            );
        }

        // **Order matters, and it is views first.** A `VkImageView` must not
        // outlive the `VkImage` it views, nor the `VkSwapchainKHR` that owns
        // that image — and a caller may legitimately have made its own view of
        // a swapchain image (an sRGB reinterpretation, say) and then leaked it.
        // Sweeping views after the swapchains, as this used to, destroys them
        // in exactly the wrong order in precisely the case this teardown exists
        // to clean up.
        //
        // The *parked* views go in the same first pass, for the same reason:
        // the deletion queue is ordered by submission, not by dependency, so a
        // parked `Trash::Image` can precede a parked `Trash::ImageView` of it,
        // and a parked view can name a live image the caller leaked.
        let mut parked: Vec<Trash> = Vec::with_capacity(state.trash.pending());
        state.trash.drain_all(|item| parked.push(item));
        parked.retain(|item| match item {
            Trash::ImageView(view) => {
                // SAFETY: the device is idle, and every parked object was
                // created by this device and is destroyed exactly once.
                unsafe { self.raw.destroy_image_view(*view, None) };
                false
            }
            _ => true,
        });

        for (_, entry) in state.views.iter() {
            if entry.swapchain_owned {
                // Destroyed with its swapchain, below — one owner, one free.
                continue;
            }
            // SAFETY: the device is idle.
            unsafe { self.raw.destroy_image_view(entry.raw, None) };
        }
        state.views.clear();

        // Destroyed in place and then cleared, rather than drained by value:
        // `Pool` has no owning iterator, and nothing here needs one.
        for (_, entry) in state.swapchains.iter() {
            // SAFETY: the device is idle, so nothing references any of this,
            // and every object was created by this device.
            unsafe {
                // Views first: one must not outlive what it views.
                for view in &entry.views {
                    self.raw.destroy_image_view(*view, None);
                }
                if entry.raw == vk::SwapchainKHR::null() {
                    for image in &entry.images {
                        self.raw.destroy_image(*image, None);
                    }
                    for block in &entry.memory {
                        self.raw.free_memory(*block, None);
                    }
                } else {
                    self.swapchain_ext.destroy_swapchain(entry.raw, None);
                }
                if let Some(sync) = entry.sync.as_ref() {
                    for semaphore in sync.acquire.iter().chain(&sync.present) {
                        self.raw.destroy_semaphore(*semaphore, None);
                    }
                    for fence in &sync.acquire_fence {
                        self.raw.destroy_fence(*fence, None);
                    }
                }
            }
            self.instance.release_surface(entry.surface_raw);
        }
        state.swapchains.clear();

        for (_, entry) in state.command_buffers.iter() {
            // SAFETY: the device is idle.
            unsafe { self.raw.destroy_command_pool(entry.pool, None) };
        }
        state.command_buffers.clear();

        for (_, entry) in state.buffers.iter() {
            // SAFETY: mapped once in `create_buffer`, unmapped once here; the
            // device is idle.
            unsafe {
                if !entry.mapped.is_null() {
                    self.raw.unmap_memory(entry.memory);
                }
                self.raw.destroy_buffer(entry.raw, None);
                self.raw.free_memory(entry.memory, None);
            }
        }
        state.buffers.clear();

        for (_, entry) in state.images.iter() {
            if entry.swapchain_owned {
                continue;
            }
            // SAFETY: the device is idle and this image is ours.
            unsafe {
                self.raw.destroy_image(entry.raw, None);
                if entry.memory != vk::DeviceMemory::null() {
                    self.raw.free_memory(entry.memory, None);
                }
            }
        }
        state.images.clear();

        for (_, entry) in state.semaphores.iter() {
            if entry.swapchain_owned {
                continue;
            }
            // SAFETY: the device is idle.
            unsafe { self.raw.destroy_semaphore(entry.raw, None) };
        }
        state.semaphores.clear();

        // **Order matters here too**: a pipeline names its layout and a
        // descriptor set names its set layout, so the dependents go first. The
        // driver does not require it — a `VkPipelineLayout` may be destroyed
        // while pipelines built from it live — but destroying a set's pool
        // before the layout keeps the sweep readable as one direction.
        for (_, entry) in state.pipelines.iter() {
            // SAFETY: the device is idle.
            unsafe { self.raw.destroy_pipeline(entry.raw, None) };
        }
        state.pipelines.clear();

        for (_, entry) in state.bind_groups.iter() {
            // SAFETY: the device is idle. Freeing the pool frees its one set.
            unsafe { self.raw.destroy_descriptor_pool(entry.pool, None) };
        }
        state.bind_groups.clear();

        for (_, entry) in state.pipeline_layouts.iter() {
            // SAFETY: the device is idle.
            unsafe { self.raw.destroy_pipeline_layout(entry.raw, None) };
        }
        state.pipeline_layouts.clear();

        for (_, entry) in state.bind_group_layouts.iter() {
            // SAFETY: the device is idle.
            unsafe { self.raw.destroy_descriptor_set_layout(entry.raw, None) };
        }
        state.bind_group_layouts.clear();

        for (_, entry) in state.shader_modules.iter() {
            // SAFETY: the device is idle.
            unsafe { self.raw.destroy_shader_module(entry.raw, None) };
        }
        state.shader_modules.clear();

        for (_, entry) in state.samplers.iter() {
            // SAFETY: the device is idle.
            unsafe { self.raw.destroy_sampler(entry.raw, None) };
        }
        state.samplers.clear();

        for (_, entry) in state.query_sets.iter() {
            // SAFETY: the device is idle.
            unsafe { self.raw.destroy_query_pool(entry.raw, None) };
        }
        state.query_sets.clear();
        state.readbacks.clear();

        for item in parked {
            // SAFETY: the device is idle, so nothing references any of this.
            unsafe { destroy_trash(&self.raw, &self.swapchain_ext, &self.instance, item) };
        }
        drop(state);

        // SAFETY: created in `open`, destroyed once, with the device idle.
        unsafe { self.raw.destroy_semaphore(self.retire_timeline, None) };
        // SAFETY: everything created from this device is gone.
        unsafe { self.raw.destroy_device(None) };
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// `vkWaitForPresentKHR` counts nanoseconds and the seam hands over a
    /// `Duration`, so the conversion is the whole contract of the timeout.
    ///
    /// The saturating end is the one worth pinning: `Duration::MAX` is
    /// ~584 billion years and its nanosecond count does not fit in a `u64`, so
    /// a wrapping conversion would turn "wait as long as it takes" into a
    /// number near zero — a caller that asked to block would return instantly
    /// and read it as the frame being up.
    #[test]
    fn a_present_timeout_becomes_nanoseconds_and_saturates_upwards() {
        assert_eq!(present_wait_ns(Duration::ZERO), 0);
        assert_eq!(present_wait_ns(Duration::from_nanos(1)), 1);
        assert_eq!(present_wait_ns(Duration::from_millis(16)), 16_000_000);
        assert_eq!(present_wait_ns(Duration::from_secs(1)), 1_000_000_000);

        // The last `Duration` that still fits, and the first that does not.
        let exact = Duration::from_nanos(u64::MAX);
        assert_eq!(present_wait_ns(exact), u64::MAX);
        assert_eq!(
            present_wait_ns(exact + Duration::from_nanos(1)),
            u64::MAX,
            "past the ceiling means 'no timeout', never a wrap to nearly none"
        );
        assert_eq!(present_wait_ns(Duration::MAX), u64::MAX);
    }

    /// Queue handles are synthesised, not pooled — so the mapping must be a
    /// bijection, or `Device::queue(Compute)` would submit to graphics.
    #[test]
    fn queue_handles_are_distinct_per_kind_and_round_trip() {
        let kinds = [QueueKind::Graphics, QueueKind::Compute, QueueKind::Transfer];
        let tag = owner_tag(1);
        let handles: Vec<QueueHandle> = kinds
            .iter()
            .copied()
            .map(|kind| queue_handle(tag, kind))
            .collect();
        for (index, handle) in handles.iter().enumerate() {
            assert_eq!((handle.index() & POOL_INDEX_MASK) as usize, index);
            assert_eq!(handle_tag(*handle), tag);
        }
        let mut bits: Vec<u64> = handles.iter().map(|handle| handle.to_bits()).collect();
        bits.sort_unstable();
        bits.dedup();
        assert_eq!(bits.len(), kinds.len(), "no two kinds may collide");
        assert_eq!(queue_index(QueueKind::Graphics), 0);
    }

    /// Two devices issue *different* handles for the same pool slot, which is
    /// the whole point: without it `entry.owner() != owner` was unreachable,
    /// `HalError::ForeignObject` was unproducible, and device A silently
    /// accepted device B's handle the moment their pools stepped in time.
    #[test]
    fn a_handle_names_the_device_that_issued_it() {
        let mut pool: Pool<u8> = Pool::new();
        let slot: Handle<u8> = pool.insert(0);

        let tags: Vec<u32> = (1..=4).map(owner_tag).collect();
        assert!(tags.iter().all(|tag| *tag != 0), "tag 0 means 'nobody'");
        assert_eq!(
            tags.iter().collect::<std::collections::HashSet<_>>().len(),
            tags.len(),
            "consecutive devices must not share a tag"
        );

        // Every stamped handle round-trips to the same pool slot, and reports
        // the device that stamped it.
        for tag in tags {
            let stamped: Handle<u8> = Handle::from_bits(
                (u64::from(slot.generation()) << 32)
                    | u64::from(slot.index() | (tag << OWNER_TAG_SHIFT)),
            )
            .expect("generation is non-zero");
            assert_eq!(handle_tag(stamped), tag);
            assert_eq!(untag::<u8, u8>(stamped), slot);
        }
    }

    /// A handle nobody stamped resolves nowhere, rather than aliasing slot 0.
    #[test]
    fn an_unstamped_handle_belongs_to_no_device() {
        let handle: Handle<u8> = Handle::from_bits((1u64 << 32) | 7).expect("non-zero generation");
        assert_eq!(handle_tag(handle), 0);
    }

    /// A granted capability drags in what it cannot be enabled without.
    ///
    /// The case is a caller naming one half of a pair — `RAY_QUERY` without the
    /// acceleration structure it traverses, `TASK_SHADER` without the mesh
    /// stage it feeds. The adapter never reports those halves apart, so the
    /// gap opens at the intersection with the request, and a device created
    /// from the gapped set would enable `VK_KHR_ray_query` without its
    /// dependency and then report caps describing a device that cannot exist.
    #[test]
    fn a_granted_capability_carries_what_it_cannot_be_enabled_without() {
        assert_eq!(
            granted_with_dependencies(Features::RAY_QUERY),
            Features::RAY_QUERY | Features::ACCELERATION_STRUCTURE
        );
        assert_eq!(
            granted_with_dependencies(Features::RAY_TRACING_PIPELINE),
            Features::RAY_TRACING_PIPELINE | Features::ACCELERATION_STRUCTURE
        );
        assert_eq!(
            granted_with_dependencies(Features::TASK_SHADER),
            Features::TASK_SHADER | Features::MESH_SHADER
        );

        // Nothing is invented in the other direction: an acceleration
        // structure on its own is a perfectly good thing to ask for, and a
        // mesh stage does not imply a task stage in front of it.
        assert_eq!(
            granted_with_dependencies(Features::ACCELERATION_STRUCTURE),
            Features::ACCELERATION_STRUCTURE
        );
        assert_eq!(
            granted_with_dependencies(Features::MESH_SHADER),
            Features::MESH_SHADER
        );
        // And a set with none of them is returned untouched, which is every
        // caller that predates this slice.
        assert_eq!(
            granted_with_dependencies(Features::GPU_DRIVEN),
            Features::GPU_DRIVEN
        );
        assert_eq!(
            granted_with_dependencies(Features::empty()),
            Features::empty()
        );
    }

    /// The refusal must name the backend and the capability, because a caller
    /// hitting it needs to know whether to branch on a tier or to file a bug.
    #[test]
    fn unimplemented_paths_name_the_slice_that_lands_them() {
        let error = not_yet("this device has no timestamp queries");
        let text = error.to_string();
        assert!(text.contains("vulkan"), "{text}");
        assert!(text.contains("timestamp"), "{text}");
    }
}
