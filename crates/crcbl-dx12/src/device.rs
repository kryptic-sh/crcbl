//! [`Dx12Device`]: one `ID3D12Device`, its queue, and the resource tables this
//! slice fills.
//!
//! # What this slice implements, and what it still refuses
//!
//! Buffers, images, image views, samplers, command buffers and readbacks —
//! created, destroyed, and looked up through generational handles — plus
//! [`Device::backend`], [`Device::caps`], [`Device::queue`],
//! [`Device::write_buffer`], [`Device::create_command_encoder`],
//! [`Device::submit`], [`Device::request_readback`], [`Device::poll_readback`]
//! and [`Device::wait_idle`], the synchronisation block — both semaphore kinds,
//! the waits and signals a submission carries, and the CPU-side read and
//! deadline wait — the query block: heaps of all three kinds, and both of the
//! seam's ways of reading one back — and the presentation block: swapchains,
//! acquire, present and
//! the present wait. **No entry point here refuses any more** — the mesh
//! pipeline was the last, and the one refusal the backend still makes moved to
//! the encoder, where a `fill_buffer` with a non-zero value answers
//! [`HalError::Unsupported`] naming the capability. Nothing here is a stub that
//! reports success.
//!
//! The presentation block is **thin on purpose**: the DXGI calls live in
//! [`crate::swapchain`] and every decision that is arithmetic lives in
//! [`crate::present`], which is the only module in this crate a non-Windows
//! `cargo test` can execute. What is left here is the pool wiring — a swapchain
//! is device-scoped while the surface it names is instance-scoped, which is
//! `crcbl-hal`'s obligation 2 and the reason the two tables sit in different
//! structs.
//!
//! # One lock over every table
//!
//! The seam takes `&self` everywhere so a device can be shared behind an `Arc`,
//! which means a backend owes its own interior synchronisation. This one uses a
//! single [`Mutex`] over the four pools *and* the descriptor heaps, which is the
//! same call `crcbl-vk` and `crcbl-mtl` made and for the same reason: the
//! traffic is a few dozen operations per frame, and a lock-per-table scheme has
//! a deadlock-ordering problem to design before it has a contention problem to
//! solve. The descriptor allocator has to be inside it either way — creating a
//! view touches the image table and a heap in one step.
//!
//! # No `unsafe` marker impl, and this time the crate docs were right
//!
//! `crcbl-mtl` had to add one in its device slice, because `MTLBuffer` and
//! `MTLTexture` inherit from `MTLResource`, which objc2 leaves unmarked.
//! `windows-rs` declares `Send` **and** `Sync` for every interface this module
//! holds — `ID3D12Device`, `ID3D12CommandQueue`, `ID3D12Resource`,
//! `ID3D12DescriptorHeap` and `ID3D12Fence` — so the markers come from the
//! compiler rather than from an assertion written here, exactly as
//! `crcbl_dx12`'s crate docs predicted for this slice. The one Win32 type that
//! is *not* `Send` is the event `HANDLE` a fence wait uses, and
//! [`Device::wait_idle`] creates and closes it inside the call rather than
//! storing it, so it never becomes this struct's problem.
//!
//! # `destroy_*` frees on the spot, and the retire queue is why that is sound
//!
//! A D3D12 command list does **not** retain the resources it references, so
//! releasing the last reference to one with work in flight is a use-after-free
//! in the driver — the gap DX2's docs said the command slice owed. It is
//! discharged in [`crate::retire`], and the shape it took is *not* `crcbl-vk`'s:
//!
//! `crcbl_dx12::command`'s encoder takes its own reference to every resource it
//! records against, [`Device::submit`] parks that set on the retire queue keyed
//! on the fence value the submission signals, and the queue releases it once
//! `GetCompletedValue` has reached that value. So a resource stays alive because
//! the submission using it holds a reference, which is why every `destroy_*`
//! below still drops the pool's reference immediately and needs no change.
//! `crcbl-vk` cannot do that — a `VkBuffer` has no refcount to hold — so its
//! queue keeps destroyed objects and re-keys them against each submission's
//! recorded handles instead.
//!
//! What the queue also holds is the `ID3D12GraphicsCommandList` and
//! `ID3D12CommandAllocator` themselves: `ExecuteCommandLists` does not retain
//! those either, and [`Device::destroy_command_buffer`] may arrive while the
//! list is still running.

use std::num::NonZeroU64;
use std::sync::{Arc, Mutex, MutexGuard};
use std::time::{Duration, Instant};

use crcbl_core::Pool;
use crcbl_hal::{
    AcquiredFrame, BackendKind, BindGroupDesc, BindGroupEntry, BindGroupHandle,
    BindGroupLayoutDesc, BindGroupLayoutHandle, BindingResource, BufferDesc, BufferHandle,
    BufferUsage, Capability, CommandBufferHandle, CommandEncoder, CommandEncoderDesc,
    ComputePipelineDesc, ComputePipelineHandle, Device, DeviceCaps, DeviceDesc, DisplayTiming,
    Extent3d, Features, Format, GraphicsPipelineDesc, GraphicsPipelineHandle, HalError, ImageDesc,
    ImageHandle, ImageSubresourceRange, ImageType, ImageUsage, ImageViewDesc, ImageViewHandle,
    ImageViewType, MemoryLocation, PipelineLayoutDesc, PipelineLayoutHandle, PresentInfo,
    QueryKind, QuerySetDesc, QuerySetHandle, QueueHandle, QueueKind, ReadbackDesc, ReadbackHandle,
    ReadbackState, SamplerDesc, SamplerHandle, SemaphoreDesc, SemaphoreHandle, SemaphoreKind,
    ShaderModuleDesc, ShaderModuleHandle, SubmitInfo, Support, SurfaceError, SwapchainDesc,
    SwapchainHandle,
};
use windows::Win32::Foundation::{CloseHandle, E_OUTOFMEMORY, WAIT_OBJECT_0, WAIT_TIMEOUT};
use windows::Win32::Graphics::Direct3D::{D3D_FEATURE_LEVEL_11_0, D3D_PRIMITIVE_TOPOLOGY};
use windows::Win32::Graphics::Direct3D12::{
    D3D12_COMMAND_LIST_TYPE_DIRECT, D3D12_COMMAND_QUEUE_DESC, D3D12_COMMAND_QUEUE_FLAG_NONE,
    D3D12_COMMAND_QUEUE_PRIORITY_NORMAL, D3D12_COMMAND_SIGNATURE_DESC,
    D3D12_COMPARISON_FUNC_ALWAYS, D3D12_CPU_DESCRIPTOR_HANDLE, D3D12_CPU_PAGE_PROPERTY_UNKNOWN,
    D3D12_FENCE_FLAG_NONE, D3D12_GPU_DESCRIPTOR_HANDLE, D3D12_HEAP_FLAG_NONE,
    D3D12_HEAP_PROPERTIES, D3D12_INDIRECT_ARGUMENT_DESC, D3D12_INDIRECT_ARGUMENT_TYPE_DISPATCH,
    D3D12_INDIRECT_ARGUMENT_TYPE_DISPATCH_MESH, D3D12_INDIRECT_ARGUMENT_TYPE_DRAW,
    D3D12_INDIRECT_ARGUMENT_TYPE_DRAW_INDEXED, D3D12_MEMORY_POOL_UNKNOWN, D3D12_QUERY_HEAP_DESC,
    D3D12_QUERY_TYPE, D3D12_RANGE, D3D12_RESOURCE_BARRIER_ALL_SUBRESOURCES, D3D12_RESOURCE_DESC,
    D3D12_RESOURCE_DIMENSION_BUFFER, D3D12_RESOURCE_FLAG_NONE, D3D12_ROOT_PARAMETER_TYPE,
    D3D12_SAMPLER_DESC, D3D12_TEXTURE_LAYOUT_ROW_MAJOR, D3D12_TEXTURE_LAYOUT_UNKNOWN,
    D3D12CreateDevice, ID3D12CommandAllocator, ID3D12CommandList, ID3D12CommandQueue,
    ID3D12CommandSignature, ID3D12DescriptorHeap, ID3D12Device, ID3D12Fence,
    ID3D12GraphicsCommandList, ID3D12Object, ID3D12PipelineState, ID3D12QueryHeap, ID3D12Resource,
    ID3D12RootSignature,
};
use windows::Win32::Graphics::Dxgi::Common::{DXGI_FORMAT_UNKNOWN, DXGI_SAMPLE_DESC};
use windows::Win32::System::Threading::{CreateEventW, WaitForSingleObject};
use windows::core::{Interface, PCWSTR};

use crate::binding::{self, BindGroupLayoutRecord, BindGroupRecord, VisibleHeaps};
use crate::command::Dx12CommandEncoder;
use crate::debug;
use crate::descriptor::{Descriptors, Kind, Slot};
use crate::draw::IndirectKind;
use crate::dxil::ShaderModuleEntry;
use crate::handle::{self, Owned, Owner};
use crate::instance::{AdapterRecord, InstanceInner, OFFSCREEN_HWND, next_owner_id};
use crate::pipeline::{self, ComputePipelineEntry, GraphicsPipelineEntry, PipelineLayoutEntry};
use crate::present::{self, PresentWait};
use crate::query;
use crate::resolve;
use crate::retire::RetireQueue;
use crate::swapchain::{self, SwapchainEntry};
use crate::sync;
use crate::view::Subresource;
use crate::{buffer, conv, validate};

/// Node mask naming the single adapter node the seam models.
///
/// `crcbl-hal` has no multi-adapter vocabulary at all, so a linked-node rig is
/// described as its first node rather than wrongly. D3D12 wants a *mask* for
/// creation and visibility, not the index `D3D12_FEATURE_DATA_ARCHITECTURE1`
/// takes, which is why this is a bit and not a zero.
const FIRST_NODE: u32 = 1;

/// How many zeroed bytes [`DeviceInner::zero`] holds.
///
/// D3D12 has no valued device-side fill this backend takes, so
/// `crate::command`'s `fill_buffer` writes zero by copying out of that resource
/// in a loop of `ceil(size / ZERO_SOURCE_BYTES)` steps. That makes this number a
/// straight trade with two sides: every device that opens pays it once in device
/// memory whether it ever fills anything or not, and every fill longer than it
/// pays one `CopyBufferRegion` per chunk.
///
/// A quarter of a megabyte is where `wgpu-hal`'s dx12 backend settles the same
/// trade. It is a whole number of the 64 KiB blocks D3D12 aligns a committed
/// buffer to, so nothing is rounded away; and the fills this seam exists for —
/// an indirect count buffer, a handful of counter words — are one copy at any
/// size worth choosing, which is what leaves the memory side free to be the
/// modest number.
pub(crate) const ZERO_SOURCE_BYTES: u64 = 256 << 10;

macro_rules! owned {
    ($($ty:ty),+ $(,)?) => {
        $(impl Owned for $ty {
            fn owner(&self) -> u64 {
                self.owner
            }
        })+
    };
}

/// A buffer, its size and where its memory lives.
///
/// `location` is kept rather than re-derived from the resource: D3D12 will
/// answer `GetHeapProperties`, but that is a call across the ABI to learn
/// something this crate already decided, and `request_readback` will need to
/// tell the two host-visible heaps apart.
#[derive(Debug)]
struct BufferEntry {
    owner: u64,
    raw: ID3D12Resource,
    /// The size the caller asked for, which is the only size the seam knows —
    /// every bounds check a write, a copy or a binding makes is against this.
    size: u64,
    /// The bytes the resource actually occupies, which is `size` rounded up for
    /// a buffer that may be bound as a constant buffer. See
    /// [`buffer::allocation_size`]; a constant buffer view is the one descriptor
    /// allowed to read past `size`, into padding nothing else can observe.
    allocation: u64,
    location: MemoryLocation,
}

/// An image, plus everything a view of it has to be checked against.
///
/// The seam-side [`Format`] is kept rather than the `DXGI_FORMAT`, because a
/// sampled depth image's resource format is *typeless* — see
/// [`conv::resource_format`] — so comparing DXGI formats would compare a view's
/// concrete format against a storage format that deliberately has no type.
#[derive(Debug)]
struct ImageEntry {
    owner: u64,
    raw: ID3D12Resource,
    format: Format,
    image_type: ImageType,
    usage: ImageUsage,
    /// The extent at mip zero, kept because a copy's footprint and its bounds
    /// check are both derived from it and `GetDesc` is a call across the ABI to
    /// learn something this crate already decided.
    extent: Extent3d,
    mip_levels: u32,
    /// Array layers, or depth slices for a volume — the two readings of
    /// [`Extent3d::depth_or_layers`](crcbl_hal::Extent3d::depth_or_layers), kept
    /// as one number because a view addresses both through the same
    /// `FirstArraySlice`/`FirstWSlice` pair and the seam names both
    /// `base_layer`.
    slices: u32,
    samples: u32,
}

/// The descriptors one image view owns.
///
/// Up to four, because D3D12 has no view object: a texture that is sampled and
/// rendered to needs a shader resource view *and* a render target view, written
/// into two different heaps. See [`crate::view`].
#[derive(Clone, Copy, Debug, Default)]
struct ViewDescriptors {
    shader_resource: Option<Slot>,
    unordered_access: Option<Slot>,
    render_target: Option<Slot>,
    depth_stencil: Option<Slot>,
}

impl ViewDescriptors {
    /// Every slot, so freeing cannot miss one the way four `if let`s can.
    fn slots(&self) -> impl Iterator<Item = Slot> {
        [
            self.shader_resource,
            self.unordered_access,
            self.render_target,
            self.depth_stencil,
        ]
        .into_iter()
        .flatten()
    }
}

/// An image view: the descriptors it wrote, and the resource they point into.
#[derive(Debug)]
struct ViewEntry {
    owner: u64,
    descriptors: ViewDescriptors,
    /// What this view's attachment descriptor addresses, computed once while
    /// `create_image_view` still holds the image's geometry.
    ///
    /// Kept so a render pass can ask what the view covers without resolving the
    /// image handle it was never given: the format, so a depth-stencil clear
    /// knows whether there is a stencil plane, and the sample count, extent and
    /// subresource indices, so a colour attachment's resolve view can be turned
    /// into `ResolveSubresource` calls. See [`crate::resolve`].
    attached: resolve::Attachment,
    /// Held so the resource cannot outlive its own descriptors. The seam already
    /// obliges a caller to destroy every view before its image, but a descriptor
    /// is a raw address into a freed resource if it does not, and a refcount is
    /// cheaper than the debugging.
    image: ID3D12Resource,
}

/// A sampler: one descriptor in the sampler heap.
#[derive(Debug)]
struct SamplerEntry {
    owner: u64,
    slot: Slot,
}

/// A finished command buffer: what runs, and everything it needs alive.
#[derive(Debug)]
pub(crate) struct CommandBufferEntry {
    pub(crate) owner: u64,
    /// The allocator holding the recorded commands. Released with the list,
    /// never before it: the list's memory *is* the allocator's.
    pub(crate) allocator: ID3D12CommandAllocator,
    pub(crate) list: ID3D12GraphicsCommandList,
    /// Every resource the recorded commands name. See [`crate::retire`].
    pub(crate) retained: Vec<ID3D12Resource>,
    /// Every query heap the recorded commands name, held for the same reason
    /// and separately because it is a different interface. See
    /// [`Retired::QueryHeap`].
    pub(crate) query_heaps: Vec<ID3D12QueryHeap>,
}

/// An in-flight readback request.
#[derive(Debug)]
struct ReadbackEntry {
    owner: u64,
    /// Stored as a handle so it is re-resolved at poll time. A buffer destroyed
    /// between the request and the poll then fails lookup rather than having its
    /// freed mapping read.
    buffer: BufferHandle,
    offset: u64,
    size: u64,
    wait: ReadbackWait,
}

/// The completion point a readback observes.
///
/// Two arms because the seam has two: [`ReadbackDesc::after`] names a caller's
/// timeline, and its absence means "everything submitted to this device before
/// the request". `crcbl-mtl` splits the same call the same way.
#[derive(Clone, Copy, Debug)]
enum ReadbackWait {
    /// The highest device-fence value handed out when
    /// [`Device::request_readback`] was called, which is exactly "everything
    /// submitted to this device before this call" and needs no synchronisation
    /// object at all.
    Submission(u64),
    /// A caller's timeline semaphore reaching a value.
    ///
    /// Stored as a handle rather than as the fence, for the reason `buffer`
    /// above is: a semaphore destroyed between the request and the poll then
    /// fails lookup instead of being observed through a reference this entry
    /// kept alive behind the caller's back.
    Timeline {
        semaphore: SemaphoreHandle,
        value: u64,
    },
}

/// Something a submission still needs, held until its fence value passes.
///
/// Release is [`Drop`], because every arm is a refcounted COM interface — which
/// is the whole reason [`crate::retire`]'s queue needs no destroy callback.
#[derive(Debug)]
pub(crate) enum Retired {
    /// A resource a recorded command names.
    ///
    /// Held rather than read — releasing it is the whole job, and the release is
    /// this value's own `Drop`. That is why the field carries the leading
    /// underscore this crate uses everywhere it keeps an interface alive without
    /// reading it back.
    Resource { _raw: ID3D12Resource },
    /// The list and the allocator its commands live in, together because
    /// neither is meaningful without the other: the list's memory *is* the
    /// allocator's.
    Recording {
        _list: ID3D12GraphicsCommandList,
        _allocator: ID3D12CommandAllocator,
    },
    /// A destroyed semaphore's fence, held until the queue has reached the
    /// operations that name it. See [`Device::destroy_semaphore`].
    Semaphore { _raw: ID3D12Fence },
    /// A query heap a recorded command names.
    ///
    /// Apart from [`Resource`](Self::Resource) because a query heap is **not**
    /// an `ID3D12Resource` — it is its own interface with no resource states, no
    /// GPU virtual address and no place in `CommandBufferEntry::retained`. It
    /// needs the same treatment all the same: `EndQuery` and `ResolveQueryData`
    /// capture the heap at record time and the list retains nothing, so a set
    /// destroyed while a submission naming it is in flight is the same
    /// use-after-free every resource here is parked against.
    QueryHeap { _raw: ID3D12QueryHeap },
}

/// A query set: the heap the GPU writes into, and the buffer a read comes back
/// through.
///
/// # Why a query set owns a buffer nothing above the seam asked for
///
/// **D3D12 has no CPU-side read of a query heap.** `ID3D12QueryHeap` cannot be
/// mapped, has no `GetData`, and exposes nothing but its own creation: the only
/// way a result leaves it is `ResolveQueryData`, which is a *command list* call
/// writing into a buffer resource. `vkGetQueryPoolResults` and
/// `MTLCounterSampleBuffer`'s `resolveCounterRange:` are both device-side calls
/// with no encoder, and [`Device::query_results`] is shaped like them — so this
/// backend is the one that has to supply the buffer and the submission
/// underneath, and this field is that buffer.
///
/// It sits on the readback heap, whose resources are created in
/// `D3D12_RESOURCE_STATE_COPY_DEST` and can never leave it — which is exactly
/// the state `ResolveQueryData` requires of a destination, so the resolve needs
/// no barrier and the map afterwards needs no copy.
///
/// The cost is one committed resource per query set, sized by
/// [`query::resolve_buffer_bytes`]. Created with the set rather than on first
/// read: [`Capability::TimestampQuery`] names `query_results` as part of what
/// the capability *is*, so a timestamp set that is never read that way is the
/// exception rather than the case worth deferring an allocation for.
#[derive(Debug)]
struct QuerySetEntry {
    owner: u64,
    raw: ID3D12QueryHeap,
    kind: QueryKind,
    /// The type every `EndQuery` and `ResolveQueryData` on this heap names. Kept
    /// beside `kind` rather than re-derived at each call, because
    /// [`conv::query_types`] returns it with the heap type it must agree with.
    query_type: D3D12_QUERY_TYPE,
    count: u32,
    /// The readback buffer [`Device::query_results`] resolves into.
    ///
    /// A seam handle rather than the resource, so the destination is allocated,
    /// labelled, bounds-checked and released by exactly the code every other
    /// buffer on this device is — and so a set destroyed while a read of it is
    /// in flight frees its pool slot rather than leaking one. See above for why
    /// a query set owns a buffer at all.
    resolve: BufferHandle,
}

/// What [`crate::command`] needs to know about a query set.
///
/// A copy of the fields rather than a borrow, for the reason [`BufferRef`] is:
/// the encoder resolves handles with the device lock held and records without
/// it.
#[derive(Debug)]
pub(crate) struct QuerySetRef {
    pub(crate) raw: ID3D12QueryHeap,
    pub(crate) kind: QueryKind,
    pub(crate) query_type: D3D12_QUERY_TYPE,
    pub(crate) count: u32,
}

/// A semaphore: an `ID3D12Fence` either way, and `kind` is what tells them
/// apart.
///
/// D3D12 has one synchronisation primitive rather than Vulkan's two, so the
/// seam's binary semaphore is the same object used one-shot — the shape
/// `crcbl-mtl` arrived at from the other direction, where `MTLSharedEvent` and
/// `MTLEvent` are two types and the *timeline* half is the one with extra
/// methods.
#[derive(Debug)]
struct SemaphoreEntry {
    owner: u64,
    raw: ID3D12Fence,
    kind: SemaphoreKind,
    /// The highest value **submitted** so far, which is not the same as the
    /// highest reached: a `Signal` sits in the queue until the GPU gets to it.
    /// The monotonicity check has to compare against this, or two submissions
    /// in flight can signal the same value and the second wait is satisfied by
    /// the first submission's work.
    ///
    /// It is also what a binary semaphore's wait and signal are *made of*: the
    /// seam says a binary semaphore's `value` field is ignored, so a signal
    /// takes the next integer and a wait takes the one most recently handed
    /// out.
    submitted: u64,
}

impl SemaphoreEntry {
    /// Whether this is a timeline, which is the half with a CPU-visible value.
    const fn is_timeline(&self) -> bool {
        matches!(self.kind, SemaphoreKind::Timeline { .. })
    }
}

owned!(
    BufferEntry,
    ImageEntry,
    ViewEntry,
    SamplerEntry,
    CommandBufferEntry,
    ReadbackEntry,
    SemaphoreEntry,
    QuerySetEntry,
);

/// Every table the device owns, behind one lock.
#[derive(Debug)]
pub(crate) struct DeviceState {
    buffers: Pool<BufferEntry>,
    images: Pool<ImageEntry>,
    views: Pool<ViewEntry>,
    samplers: Pool<SamplerEntry>,
    command_buffers: Pool<CommandBufferEntry>,
    readbacks: Pool<ReadbackEntry>,
    semaphores: Pool<SemaphoreEntry>,
    query_sets: Pool<QuerySetEntry>,
    shader_modules: Pool<ShaderModuleEntry>,
    bind_group_layouts: Pool<BindGroupLayoutRecord>,
    bind_groups: Pool<BindGroupRecord>,
    pipeline_layouts: Pool<PipelineLayoutEntry>,
    graphics_pipelines: Pool<GraphicsPipelineEntry>,
    compute_pipelines: Pool<ComputePipelineEntry>,
    /// Swapchains are **device**-scoped while the surfaces they present to are
    /// instance-scoped, which is `crcbl-hal`'s obligation 2 and the reason this
    /// pool is here and `crate::instance`'s surface pool is not.
    swapchains: Pool<SwapchainEntry>,
    descriptors: Descriptors,
    /// The shader-visible heaps a root signature binds against, which
    /// `crate::descriptor` deliberately never creates. See `crate::binding`.
    visible: VisibleHeaps,
    retire: RetireQueue<Retired>,
    /// The command signatures `ExecuteIndirect` reads its arguments through,
    /// created on first use and keyed by what each describes.
    ///
    /// One per `(kind, stride)` rather than one per device: a signature
    /// describes an argument *layout* — a fixed structure, no root arguments —
    /// so every call sharing a layout and a stride can share the object, and a
    /// device that records no indirect work creates none. The stride is part of
    /// the key because D3D12 puts `ByteStride` on the signature rather than on
    /// the call; see [`crate::draw`].
    ///
    /// A `Vec` rather than a map: three kinds, and one stride each for every
    /// caller that packs its arguments tightly.
    signatures: Vec<(IndirectKind, u32, ID3D12CommandSignature)>,
    /// The last fence value handed out, by [`Device::submit`] or
    /// [`Device::wait_idle`].
    ///
    /// **The lock is what makes the fence monotonic, and an atomic counter is
    /// not enough.** Reserving under an atomic gives two concurrent callers two
    /// distinct values, but nothing then orders the two `Signal` calls: the one
    /// holding the higher value can reach the queue first, so the fence is set
    /// to that value and then back down to the lower one. The caller that
    /// reserved the higher value samples the fence *after* the drop, sees less
    /// than its own value, arms an event for a value nothing will signal again,
    /// and blocks on it forever.
    ///
    /// One counter for both calls rather than two, because they are the same
    /// question — how much work has been issued — and two counters on one fence
    /// would be two sequences interleaved on one monotonic number.
    next_fence_value: u64,
}

/// The device's shared state.
///
/// No `unsafe impl Send`/`Sync`: every field is either plain data or a
/// `windows-rs` interface the bindings already declare both for. See the module
/// docs.
pub(crate) struct DeviceInner {
    /// Obligation 1: a `Device` may outlive its `Instance`, so the instance's
    /// state — the DXGI factory, the enumerated adapters and the surface table
    /// — is kept alive here rather than borrowed. See [`InstanceInner`].
    ///
    /// Read as well as held, since the swapchain slice: obligation 2 makes a
    /// surface instance-scoped, so `create_swapchain` resolves its handle
    /// through here rather than through any table of its own.
    pub(crate) instance: Arc<InstanceInner>,
    pub(crate) raw: ID3D12Device,
    /// The one queue, `D3D12_COMMAND_LIST_TYPE_DIRECT`, which accepts graphics,
    /// compute and copy work. The compute and copy queue *types* are exactly
    /// [`Features::ASYNC_COMPUTE_QUEUE`] and [`Features::TRANSFER_QUEUE`], and
    /// neither is reported, so neither is created.
    queue: ID3D12CommandQueue,
    /// The one fence every submission and every wait moves. One per device
    /// rather than one per call: a fence is a monotonic counter and reusing it
    /// is what makes each wait cheaper than the `CreateFence` it would otherwise
    /// need — and it is what lets [`crate::retire`] key on a single number.
    ///
    /// The value it is being driven towards lives in
    /// [`DeviceState::next_fence_value`], because ordering the signals needs a
    /// lock and this device already has exactly one.
    fence: ID3D12Fence,
    pub(crate) caps: DeviceCaps,
    /// Ticks per second of the timestamp clock behind [`DeviceInner::queue`],
    /// from `ID3D12CommandQueue::GetTimestampFrequency`. Never zero — see
    /// [`query::timestamp_frequency`].
    ///
    /// **Not in [`DeviceInner::caps`], because the seam has no field for it and
    /// should not.** D3D12 describes a timestamp as a frequency and Vulkan as
    /// its reciprocal, while Metal has no fixed period at all and WebGPU
    /// reports nanoseconds outright — so the number stays inside the backend
    /// that has it, and [`Device::query_results`] spends it turning a read into
    /// the nanoseconds the seam asks for.
    timestamp_frequency: NonZeroU64,
    /// [`ZERO_SOURCE_BYTES`] of zeroes on the default heap: the source every
    /// zero `fill_buffer` copies out of.
    ///
    /// One per device, created here rather than per call, because a fill is on
    /// the frame path and `CreateCommittedResource` is not — and because the
    /// alternative is an allocation inside a recording method that has nowhere
    /// to report a failure.
    ///
    /// **Nothing ever writes it.** `CreateCommittedResource` zeroes a resource
    /// unless it is passed `D3D12_HEAP_FLAG_CREATE_NOT_ZEROED`, which
    /// [`zero_source`] deliberately does not, so the guarantee comes from the
    /// runtime rather than from an upload this backend would have to record —
    /// there is no CPU route into a default-heap resource anyway.
    pub(crate) zero: ID3D12Resource,
    /// Which device this is, and the tag it stamps into every handle it issues.
    /// See [`crate::handle`].
    pub(crate) owner: Owner,
    state: Mutex<DeviceState>,
}

impl core::fmt::Debug for DeviceInner {
    /// The interfaces underneath print as raw pointers and say nothing a reader
    /// wants, so only the device's own identity is shown.
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        f.debug_struct("DeviceInner")
            .field("id", &self.owner.id)
            .field("geometry", &self.caps.geometry_path())
            .field("binding", &self.caps.binding_model())
            .field("lighting", &self.caps.lighting_path())
            .finish_non_exhaustive()
    }
}

/// What [`crate::command`] needs to know about a buffer.
///
/// A copy of the fields rather than a borrow of the table entry, because the
/// device lock has to be released before the encoder records: holding it across
/// a recording call would put a `MutexGuard` inside an encoder that the seam
/// says may be moved to a worker thread.
#[derive(Debug)]
pub(crate) struct BufferRef {
    pub(crate) raw: ID3D12Resource,
    pub(crate) size: u64,
    pub(crate) location: MemoryLocation,
}

/// What [`crate::command`] needs to know about an image.
#[derive(Debug)]
pub(crate) struct ImageRef {
    pub(crate) raw: ID3D12Resource,
    pub(crate) format: Format,
    pub(crate) image_type: ImageType,
    pub(crate) extent: Extent3d,
    pub(crate) mip_levels: u32,
    /// Array layers, or depth slices for a volume — see [`ImageEntry::slices`].
    pub(crate) slices: u32,
    /// Samples per texel. Read by the image-to-image copy, which D3D12 requires
    /// to name two resources of the same sample count — the differing-count copy
    /// is `ResolveSubresource` and a different call.
    pub(crate) samples: u32,
}

/// The extent of one mip level of an image, in texels.
///
/// A mip halves each *spatial* dimension and rounds up to one; an array's layer
/// count is not a spatial dimension and does not change, which is the
/// distinction `depth_or_layers` folds together and this has to unfold.
///
/// A free function rather than a method because [`create_image_view`] needs it
/// while it holds an image *table entry* rather than an [`ImageRef`], and one
/// halving that both callers read is the whole point.
///
/// [`create_image_view`]: crcbl_hal::Device::create_image_view
fn mip_extent(extent: Extent3d, image_type: ImageType, mip: u32) -> (u32, u32, u32) {
    let halve = |size: u32| (size >> mip.min(31)).max(1);
    let depth = if matches!(image_type, ImageType::D3) {
        halve(extent.depth_or_layers)
    } else {
        1
    };
    (halve(extent.width), halve(extent.height), depth)
}

/// D3D12's subresource index for a mip and array layer, in plane zero.
///
/// The plane a barrier names, and the only plane a colour format has. A copy
/// that may name another goes through [`ImageRef::subresource_in_plane`]. Free
/// for the reason [`mip_extent`] is.
const fn subresource_index(mip: u32, layer: u32, mip_levels: u32) -> u32 {
    mip + layer * mip_levels
}

impl ImageRef {
    /// The extent of one mip level, in texels. See [`mip_extent`].
    pub(crate) fn mip_extent(&self, mip: u32) -> (u32, u32, u32) {
        mip_extent(self.extent, self.image_type, mip)
    }

    /// D3D12's subresource index for a mip and array layer, in plane zero. See
    /// [`subresource_index`].
    pub(crate) fn subresource(&self, mip: u32, layer: u32) -> u32 {
        subresource_index(mip, layer, self.mip_levels)
    }

    /// D3D12's subresource index for a mip, array layer and plane.
    ///
    /// `D3D12CalcSubresource` is `mip + layer * mip_levels + plane * mip_levels
    /// * array_size`, and a **volume's array size is one**: its slices are its
    /// depth, addressed by a copy box rather than by a subresource, so
    /// [`slices`](Self::slices) is not the multiplier there.
    pub(crate) fn subresource_in_plane(&self, mip: u32, layer: u32, plane: u32) -> u32 {
        let array_size = if matches!(self.image_type, ImageType::D3) {
            1
        } else {
            self.slices
        };
        self.subresource(mip, layer) + plane * self.mip_levels * array_size
    }

    /// Every subresource index a seam subrange covers, or the "all of them"
    /// sentinel when it covers the whole image.
    ///
    /// The sentinel is not an optimisation: a barrier on a whole image is one
    /// entry rather than `mips * layers`, and D3D12 reads the two forms
    /// identically.
    pub(crate) fn subresources(&self, range: ImageSubresourceRange) -> Vec<u32> {
        let mips = resolve_count(range.mip_count, range.base_mip, self.mip_levels);
        let layers = resolve_count(range.layer_count, range.base_layer, self.slices);
        if range.base_mip == 0
            && range.base_layer == 0
            && mips >= self.mip_levels
            && layers >= self.slices
        {
            return vec![D3D12_RESOURCE_BARRIER_ALL_SUBRESOURCES];
        }
        let mut out = Vec::with_capacity((mips * layers) as usize);
        for layer in range.base_layer..range.base_layer + layers {
            for mip in range.base_mip..range.base_mip + mips {
                out.push(self.subresource(mip, layer));
            }
        }
        out
    }
}

/// A graphics pipeline resolved to everything the command list must be told.
///
/// The root signature travels with the pipeline because `SetPipelineState` does
/// not set one — a command list carries its own — so binding a pipeline is two
/// calls, and the encoder has only the pipeline handle to reach the second from.
#[derive(Debug)]
pub(crate) struct BoundPipeline {
    pub(crate) raw: ID3D12PipelineState,
    pub(crate) root_signature: ID3D12RootSignature,
    /// `None` for a mesh pipeline; see
    /// [`GraphicsPipelineEntry::topology`](crate::pipeline::GraphicsPipelineEntry::topology).
    pub(crate) topology: Option<D3D_PRIMITIVE_TOPOLOGY>,
}

/// A compute pipeline resolved to what the command list must be told.
///
/// The same pair as [`BoundPipeline`] and for the same reason, minus the piece
/// of graphics state D3D12 keeps outside a pipeline state object: a dispatch has
/// no topology.
#[derive(Debug)]
pub(crate) struct BoundCompute {
    pub(crate) raw: ID3D12PipelineState,
    pub(crate) root_signature: ID3D12RootSignature,
}

/// A bind group resolved to the root parameters and GPU addresses it binds.
#[derive(Debug)]
pub(crate) struct BoundGroup {
    /// The shader-visible heaps that must be bound before the table is. Set on
    /// every `bind_group` rather than once, because a command list's heaps are
    /// state a later `Reset` clears and this slice has no frame loop to hang the
    /// one call on.
    pub(crate) heaps: Vec<Option<ID3D12DescriptorHeap>>,
    /// `(root parameter index, table base)` for the CBV/SRV/UAV table.
    pub(crate) views: Option<(u32, D3D12_GPU_DESCRIPTOR_HANDLE)>,
    /// The same for the sampler table.
    pub(crate) samplers: Option<(u32, D3D12_GPU_DESCRIPTOR_HANDLE)>,
    /// One root descriptor per dynamic binding, with its offset already applied.
    pub(crate) roots: Vec<BoundRoot>,
    /// Every resource the group's descriptors point into, so the encoder can
    /// hold a reference for the length of the submission.
    pub(crate) retained: Vec<ID3D12Resource>,
}

/// One root descriptor a bind sets: which parameter, which call, which address.
#[derive(Clone, Copy, Debug)]
pub(crate) struct BoundRoot {
    pub(crate) parameter: u32,
    /// Which of `SetGraphicsRootConstantBufferView`'s family the address goes
    /// to. A root descriptor's type is part of the signature, so setting a CBV's
    /// address through the SRV call is a mismatch D3D12's debug layer reports
    /// and a release runtime does not.
    pub(crate) parameter_type: D3D12_ROOT_PARAMETER_TYPE,
    /// The buffer's GPU virtual address, plus the entry's offset, plus the
    /// dynamic one.
    pub(crate) address: u64,
}

/// A render pass attachment: its descriptor, the resource behind it, and what
/// the descriptor addresses.
///
/// The format lives in [`attached`](Self::attached) rather than beside it, so
/// there is one copy of it and not two that can disagree.
#[derive(Debug)]
pub(crate) struct AttachmentRef {
    pub(crate) descriptor: D3D12_CPU_DESCRIPTOR_HANDLE,
    pub(crate) image: ID3D12Resource,
    pub(crate) attached: resolve::Attachment,
}

impl DeviceInner {
    pub(crate) fn state(&self) -> MutexGuard<'_, DeviceState> {
        self.state
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
    }

    /// Resolves a queue handle against *this* device.
    ///
    /// Obligation 3 covers queues too, and the three outcomes are kept apart for
    /// the same reason they are everywhere else: a handle carrying another
    /// device's tag is [`HalError::ForeignObject`] — the caller crossed two
    /// objects that never met — while one carrying no tag at all was never
    /// issued by any device and is [`HalError::InvalidHandle`].
    pub(crate) fn check_queue(&self, queue: QueueHandle) -> Result<(), HalError> {
        if queue == handle::queue(self.owner, QueueKind::Graphics) {
            return Ok(());
        }
        Err(if handle::tag_of(queue) == 0 {
            HalError::invalid_handle("queue", queue)
        } else {
            HalError::ForeignObject {
                kind: "queue",
                bits: queue.to_bits(),
            }
        })
    }

    /// Opens an allocator and a command list recording into it.
    ///
    /// Both are `D3D12_COMMAND_LIST_TYPE_DIRECT`, the one type this device's
    /// queue accepts: an allocator's type must match the list's, and the list's
    /// must match the queue's.
    pub(crate) fn open_list(
        &self,
        label: Option<&str>,
    ) -> Result<(ID3D12CommandAllocator, ID3D12GraphicsCommandList), HalError> {
        // SAFETY: `raw` is a live `ID3D12Device` this crate owns a reference to,
        // the call takes one scalar, and `ID3D12CommandAllocator` is the IID
        // asked for.
        let allocator: ID3D12CommandAllocator = unsafe {
            self.raw
                .CreateCommandAllocator(D3D12_COMMAND_LIST_TYPE_DIRECT)
        }
        .map_err(|error| creation_error(&self.raw, "CreateCommandAllocator", &error))?;
        // SAFETY: `allocator` is the allocator just created, of the same type as
        // the list being asked for, and no list has been created from it yet —
        // D3D12 permits one recording list per allocator at a time. The initial
        // pipeline state is `None`, which is legal and means "no PSO bound";
        // nothing in this slice draws, and a draw is what would need one.
        let list: ID3D12GraphicsCommandList = unsafe {
            self.raw
                .CreateCommandList(FIRST_NODE, D3D12_COMMAND_LIST_TYPE_DIRECT, &allocator, None)
        }
        .map_err(|error| creation_error(&self.raw, "CreateCommandList", &error))?;
        if let Some(label) = label {
            label_object(&list, label);
        }
        Ok((allocator, list))
    }

    /// Resolves a buffer handle for the encoder.
    ///
    /// # Errors
    ///
    /// As [`handle::lookup`].
    pub(crate) fn buffer(&self, handle: BufferHandle) -> Result<BufferRef, HalError> {
        let state = self.state();
        let entry = handle::lookup(&state.buffers, "buffer", handle, self.owner)?;
        Ok(BufferRef {
            raw: entry.raw.clone(),
            size: entry.size,
            location: entry.location,
        })
    }

    /// Resolves an image handle for the encoder.
    ///
    /// # Errors
    ///
    /// As [`handle::lookup`].
    pub(crate) fn image(&self, handle: ImageHandle) -> Result<ImageRef, HalError> {
        let state = self.state();
        let entry = handle::lookup(&state.images, "image", handle, self.owner)?;
        Ok(ImageRef {
            raw: entry.raw.clone(),
            format: entry.format,
            image_type: entry.image_type,
            extent: entry.extent,
            mip_levels: entry.mip_levels,
            slices: entry.slices,
            samples: entry.samples,
        })
    }

    /// Resolves a query set handle for the encoder.
    ///
    /// # Errors
    ///
    /// As [`handle::lookup`].
    pub(crate) fn query_set(&self, handle: QuerySetHandle) -> Result<QuerySetRef, HalError> {
        let state = self.state();
        let entry = handle::lookup(&state.query_sets, "query set", handle, self.owner)?;
        Ok(QuerySetRef {
            raw: entry.raw.clone(),
            kind: entry.kind,
            query_type: entry.query_type,
            count: entry.count,
        })
    }

    /// Runs one command list on the queue and blocks until the GPU has finished
    /// it.
    ///
    /// **The one-shot submission [`Device::query_results`] is built on**, and
    /// the reason that call needs one at all: D3D12's only route out of a query
    /// heap is `ResolveQueryData`, which is recorded rather than called, while
    /// the seam's read is a device call with no encoder anywhere in it.
    ///
    /// The list and its allocator go on [`crate::retire`]'s queue at the value
    /// this reserves rather than being dropped when the wait returns, so a
    /// `Signal` that failed leaks them instead of freeing a list the driver may
    /// still be reading — the same trade [`Device::submit`] makes and for the
    /// same reason.
    ///
    /// # Errors
    ///
    /// [`HalError::DeviceLost`] from the signal or the wait.
    fn run_and_wait(
        &self,
        allocator: ID3D12CommandAllocator,
        list: ID3D12GraphicsCommandList,
    ) -> Result<(), HalError> {
        let value = {
            let mut state = self.state();
            let lists = [Some(ID3D12CommandList::from(list.clone()))];
            // SAFETY: `list` is a live, closed command list this device created
            // and is held both by this call and by the retire queue below for
            // the duration of its execution. The array is a live local borrowed
            // for the call, and the queue is externally synchronised by the
            // state lock held here.
            unsafe { self.queue.ExecuteCommandLists(&lists) };
            let signalled = self.signal(&mut state);
            let at = state.next_fence_value;
            state.retire.park(
                at,
                Retired::Recording {
                    _list: list,
                    _allocator: allocator,
                },
            );
            signalled?
        };
        self.wait_for(value)?;
        let mut state = self.state();
        self.poll_retire(&mut state);
        Ok(())
    }

    /// Resolves a colour attachment's render target view.
    ///
    /// # Errors
    ///
    /// As [`handle::lookup`], plus [`HalError::InvalidDescriptor`] when the view
    /// has no render target descriptor — which means its image was created
    /// without [`ImageUsage::COLOR_ATTACHMENT`], since that is the only thing
    /// `create_image_view` builds one from.
    pub(crate) fn color_attachment(
        &self,
        view: ImageViewHandle,
    ) -> Result<AttachmentRef, HalError> {
        self.attachment(view, false)
    }

    /// Resolves a depth/stencil attachment's view. See
    /// [`color_attachment`](Self::color_attachment).
    ///
    /// # Errors
    ///
    /// As [`color_attachment`](Self::color_attachment), naming
    /// [`ImageUsage::DEPTH_STENCIL_ATTACHMENT`] instead.
    pub(crate) fn depth_attachment(
        &self,
        view: ImageViewHandle,
    ) -> Result<AttachmentRef, HalError> {
        self.attachment(view, true)
    }

    fn attachment(&self, view: ImageViewHandle, depth: bool) -> Result<AttachmentRef, HalError> {
        let mut state = self.state();
        let (slot, image, attached) = {
            let entry = handle::lookup(&state.views, "image view", view, self.owner)?;
            let slot = if depth {
                entry.descriptors.depth_stencil
            } else {
                entry.descriptors.render_target
            };
            (slot, entry.image.clone(), entry.attached)
        };
        let Some(slot) = slot else {
            let usage = if depth {
                "ImageUsage::DEPTH_STENCIL_ATTACHMENT"
            } else {
                "ImageUsage::COLOR_ATTACHMENT"
            };
            return Err(HalError::InvalidDescriptor(format!(
                "this view of a {:?} image has no attachment descriptor, because its image was \
                 not created with {usage}",
                attached.format
            )));
        };
        Ok(AttachmentRef {
            descriptor: state.descriptors.cpu_handle(slot),
            image,
            attached,
        })
    }

    /// Resolves a graphics pipeline for the encoder.
    ///
    /// Cloned out from under the lock, exactly as [`buffer`](Self::buffer) is
    /// and for the same reason: the encoder resolves handles with the lock held
    /// and then records without it.
    ///
    /// # Errors
    ///
    /// As [`handle::lookup`].
    pub(crate) fn graphics_pipeline(
        &self,
        handle: GraphicsPipelineHandle,
    ) -> Result<BoundPipeline, HalError> {
        let state = self.state();
        let entry = handle::lookup(
            &state.graphics_pipelines,
            "graphics pipeline",
            handle,
            self.owner,
        )?;
        Ok(BoundPipeline {
            raw: entry.raw.clone(),
            root_signature: entry.root_signature.clone(),
            topology: entry.topology,
        })
    }

    /// The command signature one indirect layout and stride is executed
    /// through, created on first use.
    ///
    /// **A command signature holding only `DISPATCH`, `DRAW`, `DRAW_INDEXED` or
    /// `DISPATCH_MESH` takes no root signature.**
    /// `CreateCommandSignature`'s second argument is required only when the
    /// argument layout writes root arguments — a constant, a root CBV, a vertex
    /// or index buffer view — and none of these four writes any, so one object
    /// is valid against every pipeline this device has. That is what makes the
    /// cache key `(kind, stride)` rather than the pipeline.
    ///
    /// `DISPATCH_MESH` is named here because it arrived after this sentence did
    /// and the rule reaches it unchanged: it is the mesh stage's thread-group
    /// counts and nothing else, so it writes no root argument either. Passing a
    /// root signature for a layout that needs none is an error D3D12 rejects,
    /// which is why this is a claim about the argument kinds rather than a
    /// habit.
    ///
    /// # Errors
    ///
    /// [`HalError::Backend`] carrying D3D12's own message when creation fails.
    pub(crate) fn indirect_signature(
        &self,
        kind: IndirectKind,
        stride: u32,
    ) -> Result<ID3D12CommandSignature, HalError> {
        let mut state = self.state();
        if let Some((_, _, signature)) = state
            .signatures
            .iter()
            .find(|(cached, width, _)| *cached == kind && *width == stride)
        {
            return Ok(signature.clone());
        }
        let argument = D3D12_INDIRECT_ARGUMENT_DESC {
            Type: match kind {
                IndirectKind::Dispatch => D3D12_INDIRECT_ARGUMENT_TYPE_DISPATCH,
                IndirectKind::Draw => D3D12_INDIRECT_ARGUMENT_TYPE_DRAW,
                IndirectKind::DrawIndexed => D3D12_INDIRECT_ARGUMENT_TYPE_DRAW_INDEXED,
                IndirectKind::DispatchMesh => D3D12_INDIRECT_ARGUMENT_TYPE_DISPATCH_MESH,
            },
            ..Default::default()
        };
        let desc = D3D12_COMMAND_SIGNATURE_DESC {
            ByteStride: stride,
            NumArgumentDescs: 1,
            pArgumentDescs: &raw const argument,
            NodeMask: 0,
        };
        let mut created: Option<ID3D12CommandSignature> = None;
        // SAFETY: `desc` is a live local whose `pArgumentDescs` points at
        // `argument`, also a live local, and both outlive the call. The root
        // signature is `None`, which these layouts permit — see above. `created`
        // is a live local of the interface type asked for.
        unsafe {
            self.raw
                .CreateCommandSignature(&raw const desc, None, &raw mut created)
        }
        .map_err(|error| {
            HalError::Backend(format!(
                "ID3D12Device::CreateCommandSignature failed for {} at a stride of {stride}: \
                 {error}",
                kind.what()
            ))
        })?;
        let signature = created.ok_or_else(|| {
            HalError::Backend(
                "ID3D12Device::CreateCommandSignature reported success and no signature"
                    .to_string(),
            )
        })?;
        state.signatures.push((kind, stride, signature.clone()));
        Ok(signature)
    }

    /// Resolves a compute pipeline for the encoder. See
    /// [`graphics_pipeline`](Self::graphics_pipeline).
    ///
    /// # Errors
    ///
    /// As [`handle::lookup`].
    pub(crate) fn compute_pipeline(
        &self,
        handle: ComputePipelineHandle,
    ) -> Result<BoundCompute, HalError> {
        let state = self.state();
        let entry = handle::lookup(
            &state.compute_pipelines,
            "compute pipeline",
            handle,
            self.owner,
        )?;
        Ok(BoundCompute {
            raw: entry.raw.clone(),
            root_signature: entry.root_signature.clone(),
        })
    }

    /// Resolves a bind group against the pipeline layout it is being bound
    /// with.
    ///
    /// # Errors
    ///
    /// As [`handle::lookup`] for either handle, plus
    /// [`HalError::InvalidDescriptor`] when `index` is past the layout's sets,
    /// when the group's own layout is not the one that set declares — the case
    /// that otherwise binds a table of the wrong length, which D3D12 reads as
    /// arithmetic and never reports — or when `dynamic_offsets` cannot be
    /// applied to the set's root descriptors. See [`crate::root::apply`].
    pub(crate) fn bind_group(
        &self,
        index: u32,
        group: BindGroupHandle,
        dynamic_offsets: &[u32],
        layout: PipelineLayoutHandle,
    ) -> Result<BoundGroup, HalError> {
        let state = self.state();
        let layout = handle::lookup(
            &state.pipeline_layouts,
            "pipeline layout",
            layout,
            self.owner,
        )?;
        let Some(placement) = layout.sets.get(index as usize) else {
            return Err(HalError::InvalidDescriptor(format!(
                "bind group {index} is past the {} set(s) this pipeline layout declares",
                layout.sets.len()
            )));
        };
        let record = handle::lookup(&state.bind_groups, "bind group", group, self.owner)?;
        if layout.layouts.get(index as usize) != Some(&record.layout) {
            return Err(HalError::InvalidDescriptor(format!(
                "the bind group offered at set {index} was created from a different bind group \
                 layout than the one this pipeline layout declares there"
            )));
        }
        // The group's layout is the one the pipeline layout declares here, so
        // all three lists — the layout's plans, the group's addresses and the
        // placement's parameter indices — are the same length and in the same
        // ascending-binding order.
        let plans = handle::lookup(
            &state.bind_group_layouts,
            "bind group layout",
            record.layout,
            self.owner,
        )?;
        let dynamic: Vec<crate::root::Dynamic> = plans
            .roots
            .iter()
            .zip(&record.roots)
            .zip(&placement.roots)
            .map(|((plan, bound), parameter)| crate::root::Dynamic {
                binding: plan.binding,
                parameter: *parameter,
                uniform: plan.uniform(),
                address: bound.address,
                offset: bound.offset,
                size: bound.size,
                capacity: bound.capacity,
            })
            .collect();
        let roots = crate::root::apply(index, &dynamic, dynamic_offsets, &self.caps.limits)?
            .into_iter()
            .zip(&plans.roots)
            .map(|((parameter, address), plan)| BoundRoot {
                parameter,
                parameter_type: plan.parameter_type(),
                address,
            })
            .collect();

        Ok(BoundGroup {
            heaps: state.visible.bound(),
            views: placement
                .views
                .zip(record.views)
                .map(|(root, block)| (root, state.visible.gpu_views(block))),
            samplers: placement
                .samplers
                .zip(record.samplers)
                .map(|(root, block)| (root, state.visible.gpu_samplers(block))),
            roots,
            retained: record.retained.clone(),
        })
    }

    /// Resolves one `push_constants` call against the pipeline layout it names.
    ///
    /// The layout lookup and the arithmetic are together here for the reason
    /// [`bind_group`](Self::bind_group)'s are: the encoder resolves handles with
    /// the device lock held and records without it, so everything that reads
    /// device state happens before the call returns.
    ///
    /// # Errors
    ///
    /// As [`handle::lookup`] for the layout, plus every refusal
    /// [`crate::root::write`] makes — a layout declaring no range, a write that
    /// is not a whole number of 32-bit values, or one falling outside the range.
    pub(crate) fn push_constants(
        &self,
        layout: PipelineLayoutHandle,
        offset: u32,
        data: &[u8],
    ) -> Result<crate::root::Write, HalError> {
        let state = self.state();
        let entry = handle::lookup(
            &state.pipeline_layouts,
            "pipeline layout",
            layout,
            self.owner,
        )?;
        crate::root::write(entry.push_constants, offset, data)
    }

    /// Files a finished command buffer and stamps its handle.
    pub(crate) fn register_command_buffer(&self, entry: CommandBufferEntry) -> CommandBufferHandle {
        let handle = self.state().command_buffers.insert(entry);
        handle::stamp(self.owner, handle)
    }

    /// Reserves the next fence value and signals it on the queue.
    ///
    /// The reservation and the signal happen together under `state`, so the
    /// queue receives the signals in increasing order — see
    /// [`DeviceState::next_fence_value`] for the deadlock the lock rules out.
    ///
    /// **The value is committed whether or not `Signal` succeeded**, and the
    /// caller parks against it either way: a fence that will never reach a value
    /// leaks whatever was parked at it, which on a lost device is the right
    /// trade against releasing memory the driver may still be reading.
    fn signal(&self, state: &mut DeviceState) -> Result<u64, HalError> {
        state.next_fence_value += 1;
        let value = state.next_fence_value;
        // SAFETY: `queue` and `fence` are live interfaces this device owns,
        // created together in `open`. `Signal` takes the fence by reference and
        // a scalar.
        unsafe { self.queue.Signal(&self.fence, value) }.map_err(|error| {
            HalError::DeviceLost(format!(
                "ID3D12CommandQueue::Signal failed: {error}{}",
                debug::diagnosis(&self.raw)
            ))
        })?;
        Ok(value)
    }

    /// Blocks until the device fence has reached `value`.
    ///
    /// The unbounded form, for the two callers that have nothing to time out
    /// against: [`Device::wait_idle`] and this struct's `Drop`. `Duration::MAX`
    /// is not literally forever — see [`present::timeout_millis`] — so a wait
    /// that outlives its clamp arrives here as the same `DeviceLost` a fence
    /// short of its value would, which is the honest reading of a queue that
    /// has not moved in seven weeks.
    fn wait_for(&self, value: u64) -> Result<(), HalError> {
        if self.wait_fence_until(&self.fence, value, Duration::MAX)? {
            return Ok(());
        }
        Err(HalError::DeviceLost(format!(
            "the wait for fence value {value} timed out at its ceiling{}",
            debug::diagnosis(&self.raw)
        )))
    }

    /// Blocks until `fence` has reached `value`, or until `timeout` elapses.
    ///
    /// `Ok(false)` is the timeout, which is a normal outcome the seam's
    /// [`Device::wait_semaphores`] contract names explicitly; every other way of
    /// not reaching the value is an error.
    ///
    /// # The wait uses a real event, and checks that it waited
    ///
    /// `SetEventOnCompletion` accepts a null handle and is documented to block
    /// until the value is reached, which would be less code. The event is used
    /// anyway because it is the version that can be *checked*:
    /// `WaitForSingleObject` reports which way it returned, so a wait that did
    /// not happen is an `Err` here rather than a wait that silently does not
    /// wait — and a silent one is worse than none, because it would be trusted
    /// at shutdown. A null handle also cannot express a timeout at all.
    ///
    /// The event is created and closed inside the call rather than kept on the
    /// device. Two reasons, and the first is enough: an auto-reset event shared
    /// between two concurrent waiters lets one consume the other's signal, and a
    /// Win32 `HANDLE` is a raw pointer that `windows-rs` declares neither `Send`
    /// nor `Sync`, so storing one would cost this module the marker impl it
    /// otherwise does not need. `wgpu-hal`'s `dx12` backend creates and drops
    /// one per timed wait for the same reasons.
    fn wait_fence_until(
        &self,
        fence: &ID3D12Fence,
        value: u64,
        timeout: Duration,
    ) -> Result<bool, HalError> {
        // SAFETY: `fence` is a live interface this device created, and
        // `GetCompletedValue` reads no pointer of ours.
        if unsafe { fence.GetCompletedValue() } >= value {
            return Ok(true);
        }
        // SAFETY: no security attributes, auto-reset, initially unsignalled,
        // unnamed. Every argument is a scalar or a null pointer the API
        // documents as optional.
        let event = unsafe { CreateEventW(None, false, false, PCWSTR::null()) }
            .map_err(|error| HalError::DeviceLost(format!("CreateEventW failed: {error}")))?;
        // SAFETY: `event` is the handle just created and `fence` is live. The
        // runtime signals the event when the fence reaches the value, including
        // immediately if it already has.
        let armed = unsafe { fence.SetEventOnCompletion(value, event) };
        let waited = if armed.is_ok() {
            // SAFETY: `event` is a live event handle owned by this call.
            Some(unsafe { WaitForSingleObject(event, present::timeout_millis(timeout)) })
        } else {
            None
        };
        // SAFETY: `event` is this call's handle and is not used again. Closed on
        // both paths, so a failed `SetEventOnCompletion` leaks nothing.
        if let Err(error) = unsafe { CloseHandle(event) } {
            crcbl_core::log::debug!("crcbl-dx12: could not close a fence wait event: {error}");
        }
        armed.map_err(|error| {
            HalError::DeviceLost(format!(
                "SetEventOnCompletion failed: {error}{}",
                debug::diagnosis(&self.raw)
            ))
        })?;
        if waited == Some(WAIT_TIMEOUT) {
            return Ok(false);
        }
        if waited != Some(WAIT_OBJECT_0) {
            return Err(HalError::DeviceLost(format!(
                "waiting for fence value {value} returned {waited:?} rather than WAIT_OBJECT_0"
            )));
        }
        // SAFETY: as above.
        let completed = unsafe { fence.GetCompletedValue() };
        if completed < value {
            // The shape a device removed mid-submission takes: the wait is
            // satisfied because the runtime abandoned the fence, not because
            // the work finished. `diagnosis` is what separates that from a
            // fence this crate mis-signalled.
            return Err(HalError::DeviceLost(format!(
                "the wait returned with the fence at {completed}, short of {value}{}",
                debug::diagnosis(&self.raw)
            )));
        }
        Ok(true)
    }

    /// What the GPU has finished.
    fn completed(&self) -> u64 {
        // SAFETY: `fence` is live and `GetCompletedValue` reads no pointer of
        // ours and returns a `u64` by value.
        unsafe { self.fence.GetCompletedValue() }
    }

    /// Releases everything the fence has passed. See [`crate::retire`].
    fn poll_retire(&self, state: &mut DeviceState) {
        state.retire.retire(self.completed());
    }
}

impl Drop for DeviceInner {
    /// Waits for the queue before anything the queue may still be reading is
    /// released.
    ///
    /// Dropping this struct releases the retire queue's references *and* every
    /// live pool entry's, and D3D12 does not wait for its queue when the queue
    /// itself is released. So a device dropped with work in flight would free
    /// resources the GPU is mid-copy on — the same use-after-free the retire
    /// queue exists to prevent, arriving through the one path the queue cannot
    /// see.
    ///
    /// A failed wait is logged rather than propagated: `Drop` has nowhere to
    /// return, and a device that has already been lost has nothing left to
    /// protect.
    fn drop(&mut self) {
        let target = self.state().next_fence_value;
        if let Err(error) = self.wait_for(target) {
            crcbl_core::log::error!(
                "crcbl-dx12: a device was dropped without reaching fence {target}: {error}"
            );
        }
        let mut state = self.state();
        // **What the caller never destroyed, named.** `crcbl-vk` has reported
        // this since it was written and found four real leaks the afternoon it
        // learned to name kinds rather than count; this backend's suites had no
        // equivalent, so a leak here was invisible. Same message and same
        // shape, so a reader who knows one knows the other.
        //
        // Dropping the pools below releases the COM references regardless, so
        // this is a diagnostic and not a repair — which is why it warns rather
        // than failing anything.
        let kinds = [
            ("buffer", state.buffers.len()),
            ("image", state.images.len()),
            ("image view", state.views.len()),
            ("sampler", state.samplers.len()),
            ("command buffer", state.command_buffers.len()),
            ("readback", state.readbacks.len()),
            ("semaphore", state.semaphores.len()),
            ("query set", state.query_sets.len()),
            ("shader module", state.shader_modules.len()),
            ("bind group layout", state.bind_group_layouts.len()),
            ("bind group", state.bind_groups.len()),
            ("pipeline layout", state.pipeline_layouts.len()),
            ("graphics pipeline", state.graphics_pipelines.len()),
            ("compute pipeline", state.compute_pipelines.len()),
        ];
        let live: usize = kinds.iter().map(|(_, count)| count).sum();
        if live > 0 {
            let named = kinds
                .iter()
                .filter(|(_, count)| *count > 0)
                .map(|(kind, count)| format!("{count} {kind}"))
                .collect::<Vec<_>>()
                .join(", ");
            crcbl_core::log::warn!(
                "crcbl-dx12: {live} object(s) still alive at device teardown ({named})"
            );
        }
        let pending = state.retire.pending();
        if pending > 0 {
            crcbl_core::log::debug!(
                "crcbl-dx12: releasing {pending} retired batches at device teardown"
            );
        }
        state.retire.drain_all();
    }
}

/// Every resource a resolved batch of bindings points into, deduplicated.
///
/// Deduplicated by interface pointer for the reason `crate::command`'s `retain`
/// is: a group binding one buffer at ten array indices should hold one
/// reference, not ten.
fn retained(resolved: &[binding::Resolved]) -> Vec<ID3D12Resource> {
    let mut out: Vec<ID3D12Resource> = Vec::new();
    for resource in resolved.iter().filter_map(binding::Resolved::resource) {
        let raw = resource.as_raw();
        if !out.iter().any(|held| held.as_raw() == raw) {
            out.push(resource.clone());
        }
    }
    out
}

/// Puts a debug name on a D3D12 object, for PIX and the debug layer.
///
/// A failure is logged and not propagated: a name is a diagnostic, and losing
/// the whole resource because the driver would not take one would be absurd.
fn label_object(object: &ID3D12Object, label: &str) {
    // `SetName` takes a NUL-terminated UTF-16 string and copies it, so the
    // buffer only has to outlive the call.
    let wide: Vec<u16> = label.encode_utf16().chain(core::iter::once(0)).collect();
    // SAFETY: `object` is a live COM interface this crate owns a reference to,
    // and `wide` is a NUL-terminated UTF-16 buffer that outlives the call — it
    // is dropped at the end of this function, after `SetName` has returned and
    // copied it.
    if let Err(error) = unsafe { object.SetName(PCWSTR::from_raw(wide.as_ptr())) } {
        crcbl_core::log::debug!("crcbl-dx12: could not name an object \"{label}\": {error}");
    }
}

/// The heap properties for one of the seam's memory locations.
fn heap_properties(memory: MemoryLocation) -> D3D12_HEAP_PROPERTIES {
    D3D12_HEAP_PROPERTIES {
        Type: conv::heap_type(memory),
        // Both are `UNKNOWN` for every heap type except `CUSTOM`, which this
        // backend never asks for — D3D12 derives them from the heap type and
        // rejects a non-`UNKNOWN` value beside a standard one.
        CPUPageProperty: D3D12_CPU_PAGE_PROPERTY_UNKNOWN,
        MemoryPoolPreference: D3D12_MEMORY_POOL_UNKNOWN,
        CreationNodeMask: FIRST_NODE,
        VisibleNodeMask: FIRST_NODE,
    }
}

/// Creates [`DeviceInner::zero`], the zeroed resource a zero fill copies out of.
///
/// A committed resource on the default heap in `COMMON`, exactly as
/// [`create_buffer`](crcbl_hal::Device::create_buffer) makes a
/// [`MemoryLocation::DeviceLocal`] buffer — a copy source needs no flag of its
/// own, and D3D12 promotes a buffer out of `COMMON` into `COPY_SOURCE`
/// implicitly, so nothing has to barrier it. `wgpu-hal`'s dx12 backend allocates
/// the same thing at device creation for the same call.
///
/// # Errors
///
/// As every other creation here: [`HalError::OutOfDeviceMemory`] for
/// `E_OUTOFMEMORY` and [`HalError::Backend`] otherwise, through
/// [`creation_error`].
fn zero_source(device: &ID3D12Device) -> Result<ID3D12Resource, HalError> {
    let resource_desc = D3D12_RESOURCE_DESC {
        Dimension: D3D12_RESOURCE_DIMENSION_BUFFER,
        Alignment: 0,
        Width: ZERO_SOURCE_BYTES,
        Height: 1,
        DepthOrArraySize: 1,
        MipLevels: 1,
        Format: DXGI_FORMAT_UNKNOWN,
        SampleDesc: DXGI_SAMPLE_DESC {
            Count: 1,
            Quality: 0,
        },
        Layout: D3D12_TEXTURE_LAYOUT_ROW_MAJOR,
        Flags: D3D12_RESOURCE_FLAG_NONE,
    };
    let properties = heap_properties(MemoryLocation::DeviceLocal);
    let mut resource: Option<ID3D12Resource> = None;
    // SAFETY: both descriptors are live, fully initialised locals borrowed for
    // the duration of the call, and `resource` is a live `Option` the call
    // writes through. `D3D12_HEAP_FLAG_NONE` is what leaves the zeroing on,
    // since the flag that turns it off is `CREATE_NOT_ZEROED`. No optimised
    // clear value is passed, which is required for a buffer.
    unsafe {
        device.CreateCommittedResource(
            &properties,
            D3D12_HEAP_FLAG_NONE,
            &resource_desc,
            conv::initial_state(MemoryLocation::DeviceLocal),
            None,
            &mut resource,
        )
    }
    .map_err(|error| {
        creation_error(device, "CreateCommittedResource (zero fill source)", &error)
    })?;
    let raw = resource.ok_or_else(|| {
        HalError::Backend(
            "CreateCommittedResource reported success and wrote no zero fill source".to_string(),
        )
    })?;
    label_object(&raw, "crcbl-dx12 zero fill source");
    Ok(raw)
}

/// Turns a resource-creation failure into the seam's word for it.
///
/// Only `E_OUTOFMEMORY` becomes [`HalError::OutOfDeviceMemory`]. Everything else
/// keeps the driver's message, because the other failures here are descriptor
/// bugs — an illegal flag for a heap type, a dimension past the device's
/// ceiling — and reporting them as an allocation failure would send a reader
/// looking for memory pressure that is not there.
///
/// **The device is taken so the message can carry a diagnosis.** A creation
/// call is where a device that was broken *earlier* first reports itself, and
/// `DXGI_ERROR_DEVICE_REMOVED` alone names neither the offending call nor the
/// mistake — [`crate::debug`]'s `diagnosis` is what adds
/// `GetDeviceRemovedReason`'s answer and the debug layer's messages, and the
/// empty string when there is nothing wrong with the device at all.
fn creation_error(device: &ID3D12Device, what: &str, error: &windows::core::Error) -> HalError {
    if error.code() == E_OUTOFMEMORY {
        HalError::OutOfDeviceMemory
    } else {
        HalError::Backend(format!(
            "{what} failed: {error}{}",
            debug::diagnosis(device)
        ))
    }
}

/// The D3D12 implementation of [`Device`].
#[derive(Debug)]
pub struct Dx12Device {
    inner: Arc<DeviceInner>,
}

impl Dx12Device {
    /// Opens a device on `record`'s adapter.
    ///
    /// `D3D12CreateDevice` at [`D3D_FEATURE_LEVEL_11_0`], the same floor
    /// enumeration used, for the same reason: which tier a device can run is
    /// decided by [`Features`], never by the feature level it was opened at, and
    /// asking for a higher one would refuse adapters this backend has already
    /// described. Both this call and `CreateCommandQueue` return before this
    /// function does, which is why
    /// [`Instance::request_device`](crcbl_hal::Instance::request_device)
    /// completes on its first poll.
    pub(crate) fn open(
        instance: Arc<InstanceInner>,
        record: &AdapterRecord,
        desc: &DeviceDesc<'_>,
    ) -> Result<Self, HalError> {
        let mut device: Option<ID3D12Device> = None;
        // SAFETY: `record.adapter` is a live `IDXGIAdapter1` the instance owns a
        // reference to, and `device` is a live `Option<ID3D12Device>` the call
        // writes through. A failure leaves it `None`, which is why it is read
        // back rather than assumed.
        unsafe { D3D12CreateDevice(&record.adapter, D3D_FEATURE_LEVEL_11_0, &mut device) }
            .map_err(|error| {
                HalError::Backend(format!(
                    "D3D12CreateDevice failed for \"{}\": {error}",
                    record.info.name
                ))
            })?;
        // Enumeration already opened this adapter once, so a `None` here is not
        // a case anyone expects — but the binding's out-parameter is an `Option`
        // and a `None` would be a null pointer to call through.
        let raw = device.ok_or_else(|| {
            HalError::Backend(format!(
                "D3D12CreateDevice reported success and wrote no device for \"{}\"",
                record.info.name
            ))
        })?;
        // Says whether this device's validation messages can be read back at
        // all, and drops whatever the info queue already held so a later
        // failure reports its own messages rather than the process's.
        debug::attach(&raw);

        let queue_desc = D3D12_COMMAND_QUEUE_DESC {
            Type: D3D12_COMMAND_LIST_TYPE_DIRECT,
            Priority: D3D12_COMMAND_QUEUE_PRIORITY_NORMAL.0,
            // `DISABLE_GPU_TIMEOUT` is the only other flag and it turns off the
            // driver's watchdog, which is how a hung shader becomes a hung
            // desktop instead of a device removal this backend can report.
            Flags: D3D12_COMMAND_QUEUE_FLAG_NONE,
            NodeMask: FIRST_NODE,
        };
        // SAFETY: `queue_desc` is a live, fully initialised descriptor borrowed
        // for the duration of the call, and `ID3D12CommandQueue` is the IID
        // asked for.
        let queue: ID3D12CommandQueue = unsafe { raw.CreateCommandQueue(&queue_desc) }
            .map_err(|error| HalError::Backend(format!("CreateCommandQueue failed: {error}")))?;

        // SAFETY: `raw` is the device just created. `CreateFence` takes only
        // scalars and writes the interface it returns.
        let fence: ID3D12Fence = unsafe { raw.CreateFence(0, D3D12_FENCE_FLAG_NONE) }
            .map_err(|error| HalError::Backend(format!("CreateFence failed: {error}")))?;

        // Before any command list exists, because `fill_buffer` may not fail for
        // want of it: an encoder has nowhere to report an allocation failure
        // until `finish`, and this device would then have opened successfully
        // while every fill on it refused.
        let zero = zero_source(&raw)?;

        if let Some(label) = desc.label {
            // The device object names hardware rather than something a program
            // made, so the caller's name goes on the queue — which is where PIX
            // shows it.
            label_object(&queue, label);
        }

        // **D3D12 has no `pEnabledFeatures`.** A `VkDevice` reports only what was
        // switched on at creation, so `crcbl-vk` intersects the adapter's
        // features with what the caller asked for. An `ID3D12Device` can do
        // whatever `CheckFeatureSupport` answered, always, so `caps` is the
        // adapter's caps verbatim. `required_features` was checked in
        // `open_device` before this call; `optional_features` is satisfied by
        // construction, which is exactly what `DeviceDesc::optional_features`
        // documents ("check `Device::caps` afterwards to find out").
        //
        let caps = record.info.caps;
        // **The timestamp clock's rate is a *queue* question**, and
        // `crate::adapter` computes its caps before any device or queue exists.
        // So `Features::TIMESTAMP_QUERY` is reported there, where
        // `DeviceDesc::required_features` is checked against it, and the rate is
        // read here, where there is finally something to ask. It does not reach
        // the seam at all — `Device::query_results` spends it converting a tick
        // to the nanoseconds the seam reports.
        //
        // SAFETY: `queue` is the live `ID3D12CommandQueue` created immediately
        // above. `GetTimestampFrequency` takes no pointer of ours and writes the
        // `u64` it returns.
        let frequency = unsafe { queue.GetTimestampFrequency() }.map_err(|error| {
            HalError::Backend(format!(
                "ID3D12CommandQueue::GetTimestampFrequency failed on a DIRECT queue, which is \
                 where D3D12 defines it: {error}{}",
                debug::diagnosis(&raw)
            ))
        })?;
        let timestamp_frequency = query::timestamp_frequency(frequency)?;
        let owner = Owner::new(next_owner_id());
        let state = DeviceState {
            buffers: Pool::new(),
            images: Pool::new(),
            views: Pool::new(),
            samplers: Pool::new(),
            command_buffers: Pool::new(),
            readbacks: Pool::new(),
            semaphores: Pool::new(),
            query_sets: Pool::new(),
            shader_modules: Pool::new(),
            bind_group_layouts: Pool::new(),
            bind_groups: Pool::new(),
            pipeline_layouts: Pool::new(),
            graphics_pipelines: Pool::new(),
            compute_pipelines: Pool::new(),
            swapchains: Pool::new(),
            descriptors: Descriptors::new(&raw),
            visible: VisibleHeaps::new(),
            retire: RetireQueue::new(),
            signatures: Vec::new(),
            next_fence_value: 0,
        };
        let inner = Arc::new(DeviceInner {
            instance,
            raw,
            queue,
            fence,
            caps,
            timestamp_frequency,
            zero,
            owner,
            state: Mutex::new(state),
        });
        crcbl_core::log::info!(
            "crcbl-dx12: opened \"{}\" (geometry {:?}, binding {:?}, lighting {:?})",
            record.info.name,
            caps.geometry_path(),
            caps.binding_model(),
            caps.lighting_path()
        );
        Ok(Self { inner })
    }

    fn state(&self) -> MutexGuard<'_, DeviceState> {
        self.inner.state()
    }

    /// Resolves every [`BindingResource`] in a batch against this device's
    /// tables, **before** the caller takes the lock.
    ///
    /// Separate from writing them because this device has exactly one
    /// [`Mutex`], which is not reentrant: resolving inside a call that already
    /// holds the guard would deadlock. It also means a batch naming one dead
    /// handle fails before any descriptor is written, so a group is never left
    /// half-updated by a caller error.
    fn resolve_bindings(
        &self,
        entries: &[BindGroupEntry],
    ) -> Result<Vec<binding::Resolved>, HalError> {
        let mut state = self.state();
        let owner = self.inner.owner;
        entries
            .iter()
            .map(|entry| match entry.resource {
                BindingResource::Buffer {
                    buffer,
                    offset,
                    size,
                } => {
                    let record = handle::lookup(&state.buffers, "buffer", buffer, owner)?;
                    if offset > record.size {
                        return Err(HalError::InvalidDescriptor(format!(
                            "binding {} starts at {offset} of a {}-byte buffer",
                            entry.binding, record.size
                        )));
                    }
                    let remaining = record.size - offset;
                    let size = if size == BindingResource::WHOLE_BUFFER {
                        remaining
                    } else if size > remaining {
                        return Err(HalError::InvalidDescriptor(format!(
                            "binding {} binds {size} bytes at offset {offset} of a {}-byte buffer",
                            entry.binding, record.size
                        )));
                    } else {
                        size
                    };
                    // SAFETY: `record.raw` is a live buffer resource this device
                    // owns a reference to. `GetGPUVirtualAddress` reads no
                    // pointer of ours and returns an address by value.
                    let address = unsafe { record.raw.GetGPUVirtualAddress() };
                    Ok(binding::Resolved::Buffer {
                        raw: record.raw.clone(),
                        address,
                        offset,
                        size,
                        capacity: record.size,
                        allocation: record.allocation,
                        location: record.location,
                    })
                }
                BindingResource::ImageView(view) => {
                    let (slot, image) = {
                        let record = handle::lookup(&state.views, "image view", view, owner)?;
                        // A sampled binding wants the shader resource view and a
                        // storage binding the unordered access one; both are
                        // written by `create_image_view` from the image's usage,
                        // so an image created without the usage the binding
                        // needs has no descriptor and says so here rather than
                        // binding the other one.
                        let slot = record
                            .descriptors
                            .shader_resource
                            .or(record.descriptors.unordered_access);
                        (slot, record.image.clone())
                    };
                    let Some(slot) = slot else {
                        return Err(HalError::InvalidDescriptor(format!(
                            "binding {} names an image view with no shader-readable descriptor, \
                             because its image was created without ImageUsage::SAMPLED or \
                             ImageUsage::STORAGE",
                            entry.binding
                        )));
                    };
                    Ok(binding::Resolved::View {
                        raw: image,
                        descriptor: state.descriptors.cpu_handle(slot),
                    })
                }
                BindingResource::Sampler(sampler) => {
                    let slot = handle::lookup(&state.samplers, "sampler", sampler, owner)?.slot;
                    Ok(binding::Resolved::Sampler {
                        descriptor: state.descriptors.cpu_handle(slot),
                    })
                }
            })
            .collect()
    }

    /// The `ID3D12Device` underneath, for the tests that need to build
    /// something against it directly.
    #[cfg(test)]
    pub(crate) fn raw(&self) -> &ID3D12Device {
        &self.inner.raw
    }

    /// Files a swapchain's back buffers in this device's tables and gives each
    /// one a whole-image render target view.
    ///
    /// Shared by `create_swapchain` and `reconfigure_swapchain`, which need the
    /// identical work: `ResizeBuffers` invalidates every `GetBuffer` result, so
    /// a reconfigure re-registers from scratch rather than patching.
    ///
    /// **Nothing is left half-created.** A failure part-way through destroys
    /// what this call already made, so the caller is not left with orphaned
    /// descriptors in the heaps and rows in the pools that no swapchain names.
    ///
    /// # Errors
    ///
    /// `GetBuffer`'s failure through [`swapchain::surface_error`], or
    /// `create_image_view`'s — which here can only be a descriptor heap that
    /// will not grow.
    fn register_backbuffers(
        &self,
        created: &swapchain::Created,
        desc: &SwapchainDesc<'_>,
    ) -> Result<(Vec<ImageHandle>, Vec<ImageViewHandle>), SurfaceError> {
        let mut images: Vec<ImageHandle> = Vec::with_capacity(created.buffers as usize);
        let mut views: Vec<ImageViewHandle> = Vec::with_capacity(created.buffers as usize);
        let outcome = (|| -> Result<(), SurfaceError> {
            for index in 0..created.buffers {
                // SAFETY: `created.raw` is a live swapchain this device just
                // created or resized, `index` is below its buffer count, and
                // `ID3D12Resource` is the IID asked for — which is what a
                // D3D12 swapchain's buffers are.
                let raw: ID3D12Resource =
                    unsafe { created.raw.GetBuffer(index) }.map_err(|error| {
                        swapchain::surface_error(
                            &self.inner.raw,
                            "IDXGISwapChain::GetBuffer",
                            &error,
                        )
                    })?;
                if let Some(label) = desc.label {
                    label_object(&raw, label);
                }
                let slot = self.state().images.insert(ImageEntry {
                    owner: self.inner.owner.id,
                    raw,
                    // The format the *views* carry, which is what a render pass
                    // and a later `create_image_view` are checked against. The
                    // resource underneath is `present::buffer_format` of it —
                    // see `crate::swapchain`.
                    format: desc.format,
                    image_type: ImageType::D2,
                    // `TRANSFER_SRC` beside the attachment usage so a presented
                    // frame can be copied out — `crcbl screenshot`'s shape,
                    // and the same pair `crcbl-vk` puts on its swapchain
                    // images. Neither usage adds a descriptor beyond the render
                    // target view.
                    usage: ImageUsage::COLOR_ATTACHMENT | ImageUsage::TRANSFER_SRC,
                    extent: Extent3d::d2(created.extent.0, created.extent.1),
                    mip_levels: 1,
                    slices: 1,
                    samples: 1,
                });
                let image = handle::stamp(self.inner.owner, slot);
                images.push(image);
                let view = self.create_image_view(&ImageViewDesc {
                    label: desc.label,
                    image,
                    view_type: ImageViewType::D2,
                    format: desc.format,
                    range: ImageSubresourceRange::all(desc.format),
                })?;
                views.push(view);
            }
            Ok(())
        })();
        if let Err(error) = outcome {
            for view in views {
                self.destroy_image_view(view);
            }
            for image in images {
                self.destroy_image(image);
            }
            return Err(error);
        }
        Ok((images, views))
    }

    /// The offscreen ring's images: [`register_backbuffers`](Self::register_backbuffers)
    /// with `create_image` where `GetBuffer` was.
    ///
    /// The two are deliberately **not** one function with a flag. What they
    /// share is the rollback and the loop shape; what differs is every line
    /// that matters — where the resource comes from, what usage it carries, and
    /// what format it is created with. A ring image is this backend's own
    /// texture, so it takes the caller's format directly rather than
    /// `present::buffer_format`'s linear spelling: the sRGB strip exists only
    /// because `DXGI_SWAP_EFFECT_FLIP_DISCARD` rejects an `_SRGB` back buffer,
    /// and there is no swap effect here to reject anything.
    ///
    /// The usage set is `crcbl-vk`'s for the same ring, and each bit is load
    /// bearing: `COLOR_ATTACHMENT` because a frame is rendered into it,
    /// `TRANSFER_SRC` because a screenshot copies out of it, `TRANSFER_DST`
    /// because a pass may clear it through a copy, and `SAMPLED` because a
    /// tonemap reads the previous target.
    ///
    /// # Errors
    ///
    /// `create_image`'s — a descriptor D3D12 refuses, or an allocation that
    /// failed — or `create_image_view`'s, which here can only be a descriptor
    /// heap that will not grow.
    fn register_ring_images(
        &self,
        extent: (u32, u32),
        count: u32,
        desc: &SwapchainDesc<'_>,
    ) -> Result<(Vec<ImageHandle>, Vec<ImageViewHandle>), SurfaceError> {
        let mut images: Vec<ImageHandle> = Vec::with_capacity(count as usize);
        let mut views: Vec<ImageViewHandle> = Vec::with_capacity(count as usize);
        let outcome = (|| -> Result<(), HalError> {
            for index in 0..count {
                let label = desc.label.map(|label| format!("{label} [{index}]"));
                let image = self.create_image(&ImageDesc {
                    label: label.as_deref(),
                    image_type: ImageType::D2,
                    extent: Extent3d::d2(extent.0, extent.1),
                    format: desc.format,
                    mip_levels: 1,
                    samples: 1,
                    usage: ImageUsage::COLOR_ATTACHMENT
                        | ImageUsage::TRANSFER_SRC
                        | ImageUsage::TRANSFER_DST
                        | ImageUsage::SAMPLED,
                })?;
                images.push(image);
                let view = self.create_image_view(&ImageViewDesc {
                    label: label.as_deref(),
                    image,
                    view_type: ImageViewType::D2,
                    format: desc.format,
                    range: ImageSubresourceRange::all(desc.format),
                })?;
                views.push(view);
            }
            Ok(())
        })();
        if let Err(error) = outcome {
            for view in views {
                self.destroy_image_view(view);
            }
            for image in images {
                self.destroy_image(image);
            }
            return Err(SurfaceError::Hal(error));
        }
        Ok((images, views))
    }

    /// Builds the ring of plain images an offscreen surface's "swapchain" is.
    ///
    /// Nothing DXGI owns is created: no `IDXGISwapChain3`, so no back buffers,
    /// no waitable object and no `MakeWindowAssociation`. See
    /// [`crate::swapchain`] for the table of what each seam call does on one of
    /// these instead.
    ///
    /// # Errors
    ///
    /// [`HalError::InvalidDescriptor`] through `swapchain::check_offscreen` for
    /// a descriptor the caps did not offer, or
    /// [`register_ring_images`](Self::register_ring_images)'.
    fn create_offscreen_ring(
        &self,
        desc: &SwapchainDesc<'_>,
    ) -> Result<SwapchainHandle, SurfaceError> {
        let (extent, count) = swapchain::check_offscreen(desc)?;
        let (images, views) = self.register_ring_images(extent, count, desc)?;
        let entry = SwapchainEntry {
            owner: self.inner.owner.id,
            // The field that decides everything else about this entry.
            raw: None,
            // Zero is "there is nothing to wait on", which `swapchain::wait`
            // answers immediately — see `SwapchainEntry::waitable`.
            waitable: 0,
            hwnd: OFFSCREEN_HWND,
            extent,
            format: desc.format,
            buffers: count,
            next_offscreen: 0,
            acquired: None,
            present_mode: present::resolve_offscreen_present_mode(desc.present_mode),
            flags: swapchain::NO_SWAP_CHAIN_FLAGS,
            images,
            views,
            ledger: present::PresentLedger::default(),
        };
        crcbl_core::log::info!(
            "crcbl-dx12: offscreen ring {}x{} {:?}, {count} image(s)",
            extent.0,
            extent.1,
            desc.format,
        );
        let handle = self.state().swapchains.insert(entry);
        Ok(handle::stamp(self.inner.owner, handle))
    }

    /// The fence and value each of a submission's waits becomes.
    ///
    /// Resolved before any of them reaches the queue, for the reason
    /// [`Device::submit`] resolves its command buffers first: a submission that
    /// failed halfway would leave the queue holding some of the ordering the
    /// caller asked for and not the rest.
    ///
    /// Which value each wait carries is [`sync::wait_value`]'s decision, taken
    /// away from D3D12 so it can be checked on any host.
    ///
    /// # Errors
    ///
    /// [`HalError::InvalidHandle`] or [`HalError::ForeignObject`].
    fn resolve_waits(
        &self,
        state: &DeviceState,
        submit: &SubmitInfo<'_>,
    ) -> Result<Vec<(ID3D12Fence, u64)>, HalError> {
        let mut out = Vec::with_capacity(submit.waits.len());
        for wait in submit.waits {
            let entry = handle::lookup(
                &state.semaphores,
                "semaphore",
                wait.semaphore,
                self.inner.owner,
            )?;
            let value = sync::wait_value(entry.is_timeline(), entry.submitted, wait.value);
            out.push((entry.raw.clone(), value));
        }
        Ok(out)
    }

    /// The fence and value each of a submission's signals becomes, with the
    /// handle so the entry's floor can be moved once the queue has taken it.
    ///
    /// # Errors
    ///
    /// [`HalError::InvalidHandle`] or [`HalError::ForeignObject`], and
    /// [`HalError::InvalidDescriptor`] for a timeline signal that does not
    /// exceed everything already submitted onto that semaphore.
    fn resolve_signals(
        &self,
        state: &DeviceState,
        submit: &SubmitInfo<'_>,
    ) -> Result<Vec<(SemaphoreHandle, ID3D12Fence, u64)>, HalError> {
        let mut out: Vec<(SemaphoreHandle, ID3D12Fence, u64)> =
            Vec::with_capacity(submit.signals.len());
        for signal in submit.signals {
            let entry = handle::lookup(
                &state.semaphores,
                "semaphore",
                signal.semaphore,
                self.inner.owner,
            )?;
            // The floor is the highest value submitted onto this semaphore so
            // far, *including* by an earlier signal in this same `SubmitInfo` —
            // otherwise two signals on one semaphore in one submission would
            // both be checked against the stale value and could take the same
            // number twice. [`sync`] owns the rule and says what that costs.
            let floor = out
                .iter()
                .filter(|(handle, _, _)| *handle == signal.semaphore)
                .map(|(_, _, value)| *value)
                .max()
                .unwrap_or(entry.submitted);
            let value = sync::signal_value(entry.is_timeline(), floor, signal.value)?;
            out.push((signal.semaphore, entry.raw.clone(), value));
        }
        Ok(out)
    }
}

impl Device for Dx12Device {
    fn backend(&self) -> BackendKind {
        BackendKind::Dx12
    }

    fn caps(&self) -> DeviceCaps {
        self.inner.caps
    }

    /// What this backend does with each seam behaviour.
    ///
    /// **This is the backend mid-build, and the reasons say so.** D3D12 has
    /// every capability below; what is missing is this crate's expression of it,
    /// and each refusal names the slice that owes it — the exception being the
    /// two *valued* buffer fills, which are a deliberate decline argued at
    /// [`fill_buffer`](crate::Dx12CommandEncoder) rather than an unwritten
    /// slice. The zero fill is supported and lands as a copy out of
    /// [`DeviceInner::zero`].
    ///
    /// The honest reading of this list is that `crcbl-dx12` is the furthest from
    /// parity of the five, and the point of writing it down is that the number
    /// is now visible instead of being spread across twenty-six refusal sites.
    ///
    /// Exhaustive with no wildcard arm, and `deny`-ed as such.
    #[deny(clippy::wildcard_enum_match_arm)]
    fn supports(&self, capability: Capability) -> Support {
        let has = self.inner.caps.features;
        let gated = |feature: Features, why: &'static str| -> Support {
            Support::granted(has, feature, why)
        };
        // One sentence for both mesh rows, shared for the reason
        // `METAL_NO_DRAW_INDIRECT_COUNT` is — the declaration a caller reads and
        // the parity record a reviewer reads drifted apart last time they were
        // written twice. The two valued fills used to share one the same way,
        // and both rows are gone: the amplification
        // stage is not separately missing, it is behind the same unreported flag.
        const NO_MESH_FLAG: &str = "the mesh and amplification stages are built — crcbl_dx12::pipeline packs the \
             D3D12_PIPELINE_STATE_STREAM_DESC and the encoder records DispatchMesh — but this \
             backend does not yet report Features::MESH_SHADER, because doing so moves every \
             adapter onto GeometryPath::MeshShader and re-keys every golden image (the DX12 mesh \
             reporting slice)";
        match capability {
            // A CopyBufferRegion in a loop; see crate::command's clear_buffer
            // and DeviceInner::zero.
            Capability::BufferFillZero => Support::Yes,
            // Both sides are subresource-index locations, which is the copy
            // `crate::command::plan_image_copy` builds — including the plane
            // slice a depth format's aspect names, since an image-to-image copy
            // needs no placed footprint at all.
            Capability::ImageToImageCopy => Support::Yes,
            // `crate::conv::copy_footprint_format` is the placed footprint's
            // fourth column and `crate::command::plan_copy` lays the rows out
            // against the plane's own texel, so a depth plane copies both ways.
            // The capability asks whether the backend has an expression for the
            // copy at all; the one pair D3D12 withholds — `D24UnormS8Uint`'s
            // depth plane, which no fully typed single-plane DXGI format
            // describes — is refused by name at the call, which is what the
            // capability's own documentation says a `Yes` still does.
            Capability::DepthImageCopy => Support::Yes,
            // `crate::command`'s `end_render_pass` records the
            // `ResolveSubresource` a resolve view asks for, one call per array
            // layer, and `crate::resolve` refuses by name every pairing the seam
            // allows and D3D12 has no such call for — a format or extent
            // mismatch, a volume, a destination that is itself multisampled.
            // The capability asks whether the backend has an expression for the
            // resolve at all, which is what those refusals leave intact.
            Capability::MsaaResolveAttachment => Support::Yes,
            Capability::StencilReference => Support::Yes,
            Capability::DrawIndirectCount => gated(
                Features::DRAW_INDIRECT_COUNT,
                "this device reports no DRAW_INDIRECT_COUNT",
            ),
            // An `ID3D12CommandSignature` is built per stride, so a padded one
            // is described rather than assumed away.
            Capability::IndirectArgumentPaddedStride => Support::Yes,
            // **The calls are written; the flag is not reported.** `pipeline`'s
            // `mesh` packs the subobject stream, `CommandEncoder`'s
            // `draw_mesh_tasks` records `DispatchMesh` and its indirect twin
            // executes a `DISPATCH_MESH` command signature. What is missing is
            // `adapter`'s `features_of` reading
            // `D3D12_FEATURE_DATA_D3D12_OPTIONS7::MeshShaderTier` and reporting
            // `Features::MESH_SHADER` — which flips
            // `GeometryPath::from_features` to `MeshShader` for every adapter
            // and re-keys every golden image, so it is its own change.
            //
            // Until then this stays `No`: a `granted` here would answer
            // `NotOnThisDevice` on a device that never withheld anything, and
            // `parity_verdict` calls that `FalseDeviceGate` — which is the loud
            // failure, and rightly.
            Capability::MeshShading | Capability::TaskShaderStage => Support::No(NO_MESH_FLAG),
            Capability::UpdateBindGroup => Support::Yes,
            // `create_pipeline_layout` builds a
            // `D3D12_ROOT_PARAMETER_TYPE_32BIT_CONSTANTS` parameter at the `b`
            // register the committed DXIL puts the block at, and
            // `CommandEncoder::push_constants` sets it through
            // `SetGraphicsRoot32BitConstants` or its compute twin. Root
            // constants are core D3D12, so the gate below never fires — it is
            // the flag's own arm, not a device question.
            Capability::PushConstants => gated(
                Features::PUSH_CONSTANTS,
                "this device reports no PUSH_CONSTANTS",
            ),
            Capability::BindlessDescriptorArray => gated(
                Features::DESCRIPTOR_INDEXING,
                "this device reports no DESCRIPTOR_INDEXING, so its resource binding tier is below \
                 3",
            ),
            Capability::StorageImageBinding => Support::Yes,
            Capability::PolygonModeLine => gated(
                Features::POLYGON_MODE_LINE,
                "this device reports no POLYGON_MODE_LINE",
            ),
            Capability::DepthClamp => {
                gated(Features::DEPTH_CLAMP, "this device reports no DEPTH_CLAMP")
            }
            Capability::SamplerAnisotropy => gated(
                Features::SAMPLER_ANISOTROPY,
                "this device reports no SAMPLER_ANISOTROPY",
            ),
            // `CreateQueryHeap` takes all three heap types on every D3D12
            // device, so `features_of` reports the three flags unconditionally
            // — but the answer is still read off this device's own caps, for the
            // reason the semaphore arms below give. `Device::create_query_set`
            // builds the heap and the readback buffer a result comes back
            // through, `RenderPassDesc::timestamp_writes` and
            // `CommandEncoder::resolve_query_set` record `EndQuery` and
            // `ResolveQueryData`, and
            // `Device::query_results` submits a resolve of its own and reads it.
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
            // `CreateFence` is on every D3D12 device, so `features_of` reports
            // the flag unconditionally — but the answer is still the device's
            // rather than a constant, because a backend claiming a capability
            // its own caps deny is the lie this enum exists to catch.
            // `ID3D12Fence` carries all three: `GetCompletedValue` to read,
            // `SetEventOnCompletion` to wait, and a CPU-side `Signal` to
            // advance. One object, so one flag.
            Capability::TimelineSemaphore
            | Capability::CpuTimelineWait
            | Capability::CpuTimelineSignal => gated(
                Features::TIMELINE_SEMAPHORE,
                "this device reports no TIMELINE_SEMAPHORE",
            ),
            // D3D12 has one primitive, so a binary semaphore is the same
            // `ID3D12Fence` driven one integer at a time and it is handed out
            // whatever the flag says — which is what the seam requires, since a
            // device with no timeline must still have a binary one to give.
            Capability::BinarySemaphore => Support::Yes,
            // A real `ID3D12Fence` blocks until somebody signals it, and it has
            // a CPU-side `Signal` of its own, so `ID3D12CommandQueue::Wait` may
            // be issued for a value nothing has submitted yet — which is how
            // D3D12 expresses ordering in the first place.
            Capability::TimelineWaitBeforeSignal => gated(
                Features::TIMELINE_SEMAPHORE,
                "this device reports no TIMELINE_SEMAPHORE, so there is no timeline to wait on",
            ),
        }
    }

    /// The graphics queue, and only ever the graphics queue.
    ///
    /// D3D12's command list types are exactly this enum's three variants —
    /// which is what [`QueueKind`] says is why it is not named `QueueFamily` —
    /// so [`QueueKind::Compute`] and [`QueueKind::Transfer`] are creatable and
    /// deliberately not created. The seam ties each to a feature
    /// ([`Features::ASYNC_COMPUTE_QUEUE`], [`Features::TRANSFER_QUEUE`]), this
    /// backend reports neither, and a queue handed out for a feature that is not
    /// reported is the flag-without-a-call mistake in reverse.
    fn queue(&self, kind: QueueKind) -> Option<QueueHandle> {
        match kind {
            QueueKind::Graphics => Some(handle::queue(self.inner.owner, kind)),
            QueueKind::Compute | QueueKind::Transfer => None,
        }
    }

    // --- resources ---

    /// Creates a buffer as a committed resource on the heap its memory location
    /// names.
    ///
    /// Committed rather than placed: a placed resource needs a suballocator over
    /// `ID3D12Heap`, which is what `gpu-allocator` is to `crcbl-vk`, and picking
    /// one is a decision with a lifetime and defragmentation story attached.
    /// **Committed is correct and slower**, which is the right direction for a
    /// slice with no frame loop to be slow in.
    ///
    /// There is no [`BufferUsage::DEVICE_ADDRESS`](crcbl_hal::BufferUsage::DEVICE_ADDRESS)
    /// check, and that is not an omission: every D3D12 buffer answers
    /// `GetGPUVirtualAddress`, which is why `crcbl_dx12::adapter` reports
    /// [`Features::BUFFER_DEVICE_ADDRESS`] unconditionally. A check against a
    /// feature that is always present is a check that cannot fail.
    fn create_buffer(&self, desc: &BufferDesc<'_>) -> Result<BufferHandle, HalError> {
        if desc.size == 0 {
            return Err(HalError::InvalidDescriptor(
                "BufferDesc::size must be non-zero".to_string(),
            ));
        }
        // **The allocation, not the requested size.** A buffer that may be bound
        // as a constant buffer is padded to a whole number of D3D12's
        // 256-byte blocks here, because the view over it has to be one and a
        // view may not run past its resource — see `crate::buffer`, which owns
        // that arithmetic and its tests. The table below keeps `desc.size`, so
        // nothing above the seam can see the difference.
        let allocation = buffer::allocation_size(desc.size, desc.usage)?;
        let resource_desc = D3D12_RESOURCE_DESC {
            Dimension: D3D12_RESOURCE_DIMENSION_BUFFER,
            // Zero means "the default for this resource", which for a buffer is
            // the 64 KiB D3D12 requires.
            Alignment: 0,
            Width: allocation,
            Height: 1,
            DepthOrArraySize: 1,
            MipLevels: 1,
            // A buffer is untyped in D3D12; the format belongs to the view.
            Format: DXGI_FORMAT_UNKNOWN,
            SampleDesc: DXGI_SAMPLE_DESC {
                Count: 1,
                Quality: 0,
            },
            // The only layout a buffer may have.
            Layout: D3D12_TEXTURE_LAYOUT_ROW_MAJOR,
            Flags: conv::buffer_flags(desc.usage, desc.memory),
        };
        let properties = heap_properties(desc.memory);
        let mut resource: Option<ID3D12Resource> = None;
        // SAFETY: both descriptors are live, fully initialised locals borrowed
        // for the duration of the call, and `resource` is a live `Option` the
        // call writes through. The initial state is the one D3D12 requires for
        // this heap type — see `conv::initial_state`, which is the only source
        // of it — and no optimised clear value is passed, which is legal for a
        // buffer and required for one.
        unsafe {
            self.inner.raw.CreateCommittedResource(
                &properties,
                D3D12_HEAP_FLAG_NONE,
                &resource_desc,
                conv::initial_state(desc.memory),
                None,
                &mut resource,
            )
        }
        .map_err(|error| {
            creation_error(&self.inner.raw, "CreateCommittedResource (buffer)", &error)
        })?;
        let raw = resource.ok_or_else(|| {
            HalError::Backend(
                "CreateCommittedResource reported success and wrote no buffer".to_string(),
            )
        })?;
        if let Some(label) = desc.label {
            label_object(&raw, label);
        }

        let handle = self.state().buffers.insert(BufferEntry {
            owner: self.inner.owner.id,
            raw,
            size: desc.size,
            allocation,
            location: desc.memory,
        });
        Ok(handle::stamp(self.inner.owner, handle))
    }

    fn destroy_buffer(&self, buffer: BufferHandle) {
        let mut state = self.state();
        drop(handle::take_owned(
            &mut state.buffers,
            buffer,
            self.inner.owner,
        ));
    }

    /// Copies `data` into a host-visible buffer.
    ///
    /// # `DeviceLocal` is refused, never silently dropped
    ///
    /// A [`MemoryLocation::DeviceLocal`] buffer lives on the default heap, which
    /// `ID3D12Resource::Map` rejects — D3D12's only route into one is a copy
    /// from a buffer on the upload heap, and the copy command list is the DX12
    /// command slice. So this refuses with [`HalError::InvalidDescriptor`]
    /// naming the location, which is both what the seam documents ("
    /// `InvalidDescriptor` … if the buffer is not host-visible") and what
    /// `crcbl-vk` and `crcbl-mtl` answer for the same call, so the backends
    /// disagree about nothing.
    ///
    /// # The map ranges are not decoration
    ///
    /// `Map` is given an empty read range and `Unmap` the exact range written.
    /// Both matter on a discrete GPU: an empty read range tells the runtime the
    /// CPU will not read what is there, and a precise written range is what
    /// keeps a write-combined upload buffer from flushing bytes nobody touched.
    /// Passing `None` to either is the "assume everything" answer and is
    /// correct, slowly — which for a staging ring written every frame is the
    /// wrong default to start from.
    fn write_buffer(&self, buffer: BufferHandle, offset: u64, data: &[u8]) -> Result<(), HalError> {
        let state = self.state();
        let entry = handle::lookup(&state.buffers, "buffer", buffer, self.inner.owner)?;
        if !entry.location.is_mappable() {
            return Err(HalError::InvalidDescriptor(format!(
                "write_buffer needs a host-visible buffer; this one is {:?}, which D3D12 can only \
                 reach through a copy from an upload buffer (the DX12 command slice)",
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
        // A `D3D12_RANGE` is expressed in `usize`, so a 64-bit offset that does
        // not fit one is refused rather than truncated. Only reachable on a
        // 32-bit host, where the allocation could not have existed either.
        let begin = usize::try_from(offset).map_err(|_| {
            HalError::InvalidDescriptor(format!(
                "write_buffer offset {offset} does not fit this host's address space"
            ))
        })?;
        // **A readback heap is told nothing was written, even though it was.**
        // `pWrittenRange` is what the CPU promises the *GPU* will need to see,
        // and a `D3D12_HEAP_TYPE_READBACK` allocation is stuck in `COPY_DEST`,
        // so the GPU can never read what the CPU put there. D3D12's debug layer
        // says so by name — "Readback resources can be written by the CPU but
        // there's not much utility … The range should be empty (Begin >= End)"
        // — and it is a warning this backend's tests treat as a failure, which
        // is how it was found rather than shipped.
        //
        // The bytes are still there for the CPU: these heaps are write-back
        // cached, so the caller's own later read sees them. Only the promise to
        // the GPU is withdrawn, because it was never true.
        let written = if entry.location == MemoryLocation::HostReadback {
            D3D12_RANGE { Begin: 0, End: 0 }
        } else {
            D3D12_RANGE {
                Begin: begin,
                End: begin + data.len(),
            }
        };
        // Begin == End says "the CPU read nothing", which is what this call
        // does.
        let read_nothing = D3D12_RANGE { Begin: 0, End: 0 };
        let mut mapped: *mut core::ffi::c_void = core::ptr::null_mut();
        // SAFETY: `entry.raw` is a live buffer on a host-visible heap — checked
        // above — and subresource 0 is the only one a buffer has. Both range
        // pointers name live locals, and `mapped` is a live pointer the call
        // writes through.
        unsafe { entry.raw.Map(0, Some(&read_nothing), Some(&mut mapped)) }.map_err(|error| {
            // A failed Map is where a removed device surfaces on this
            // backend — the readback is the first call that touches
            // memory the GPU was writing — so it is the one place the
            // breadcrumbs are worth most. Without this the caller gets
            // 0x887A0005 and no diagnosis, which is what a CI run
            // reproducing a device removal actually printed.
            // Still `Backend`, not `DeviceLost`: a Map can fail for reasons
            // that are not a removal, and `debug::diagnosis` returns the
            // empty string on a healthy device, so the variant must not be
            // decided by the diagnostic being attached.
            HalError::Backend(format!(
                "ID3D12Resource::Map failed: {error}{}",
                debug::diagnosis(&self.inner.raw)
            ))
        })?;
        if mapped.is_null() {
            return Err(HalError::Backend(
                "ID3D12Resource::Map reported success and wrote no pointer".to_string(),
            ));
        }
        // SAFETY: `mapped` points at `entry.size` bytes of this buffer's
        // storage, the range `begin..begin + data.len()` was bounds-checked
        // against that size immediately above, and the two regions cannot
        // overlap because `data` is a caller-owned slice and the destination is
        // the buffer's own mapping. The pointer does not escape this block, and
        // the device lock is held across the whole map/copy/unmap.
        unsafe {
            core::ptr::copy_nonoverlapping(
                data.as_ptr(),
                mapped.cast::<u8>().add(begin),
                data.len(),
            );
        }
        // SAFETY: the matching `Unmap` for the `Map` above, on the same
        // subresource of the same live resource, with a range inside the
        // allocation.
        unsafe { entry.raw.Unmap(0, Some(&written)) };
        Ok(())
    }

    /// Records what a later [`Device::poll_readback`] has to wait for.
    ///
    /// Nothing happens on the GPU here and nothing blocks: the whole request is
    /// a completion point plus a range, which is what makes the seam's
    /// poll-shaped readback implementable in a browser and is why this backend
    /// is shaped the same way. See [`crcbl_hal::readback`].
    ///
    /// # `after` is a completion point, so it must be a timeline
    ///
    /// [`ReadbackDesc::after`] names a semaphore and a value, and the poll
    /// reads that counter instead of the device fence. A **binary** semaphore
    /// is refused rather than approximated: it has no CPU-visible value, so
    /// there is nothing for a poll to compare against and the readback would
    /// resolve against the wrong completion point — handing back whatever
    /// happened to be in the buffer. `crcbl-mtl` refuses the same case for the
    /// same reason.
    ///
    /// # Errors
    ///
    /// [`HalError::InvalidHandle`] or [`HalError::ForeignObject`],
    /// [`HalError::InvalidDescriptor`] for a buffer that is not
    /// [`MemoryLocation::HostReadback`] or a range outside it, and
    /// [`HalError::Unsupported`] for an `after` naming a binary semaphore.
    fn request_readback(&self, desc: &ReadbackDesc<'_>) -> Result<ReadbackHandle, HalError> {
        let mut state = self.state();
        let entry = handle::lookup(&state.buffers, "buffer", desc.buffer, self.inner.owner)?;
        if !matches!(entry.location, MemoryLocation::HostReadback) {
            return Err(HalError::InvalidDescriptor(format!(
                "request_readback needs a HostReadback buffer; this one is {:?}, and D3D12 will \
                 not map anything else for reading",
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
                let semaphore = handle::lookup(
                    &state.semaphores,
                    "semaphore",
                    after.semaphore,
                    self.inner.owner,
                )?;
                if !semaphore.is_timeline() {
                    return Err(HalError::Unsupported {
                        backend: BackendKind::Dx12,
                        what: "ReadbackDesc::after must name a timeline semaphore",
                    });
                }
                ReadbackWait::Timeline {
                    semaphore: after.semaphore,
                    value: after.value,
                }
            }
            // "Everything submitted to this device before this call" is exactly
            // the highest fence value handed out, so an unqualified request
            // needs no synchronisation object at all.
            None => ReadbackWait::Submission(state.next_fence_value),
        };
        let handle = state.readbacks.insert(ReadbackEntry {
            owner: self.inner.owner.id,
            buffer: desc.buffer,
            offset: desc.offset,
            size: desc.size,
            wait,
        });
        Ok(handle::stamp(self.inner.owner, handle))
    }

    /// A poll, never a wait.
    ///
    /// The fence is sampled and the answer is [`ReadbackState::Pending`] if it
    /// has not arrived — no event is armed and nothing blocks, because a caller
    /// polling once per frame must not lose the frame to a readback that is one
    /// submission late.
    ///
    /// Both completion points are read the same way — a counter sampled, never
    /// an event armed. [`ReadbackWait::Submission`] reads the device fence and
    /// [`ReadbackWait::Timeline`] reads the caller's semaphore; neither blocks.
    ///
    /// The buffer and the semaphore are re-resolved from the handles stored at
    /// request time rather than kept as pointers, so either one destroyed
    /// between the request and the poll fails lookup instead of being read
    /// through a reference the caller thought they had released.
    fn poll_readback(
        &self,
        readback: ReadbackHandle,
        out: &mut [u8],
    ) -> Result<ReadbackState, HalError> {
        let mut state = self.state();
        let (buffer, offset, size, wait) = {
            let entry = handle::lookup(&state.readbacks, "readback", readback, self.inner.owner)?;
            (entry.buffer, entry.offset, entry.size, entry.wait)
        };
        if out.len() as u64 != size {
            return Err(HalError::InvalidDescriptor(format!(
                "poll_readback needs exactly {size} bytes, got {}",
                out.len()
            )));
        }
        let reached = match wait {
            ReadbackWait::Submission(after) => self.inner.completed() >= after,
            ReadbackWait::Timeline { semaphore, value } => {
                let entry =
                    handle::lookup(&state.semaphores, "semaphore", semaphore, self.inner.owner)?;
                // SAFETY: `raw` is a live fence this device created, and
                // `GetCompletedValue` reads no pointer of ours.
                let completed = unsafe { entry.raw.GetCompletedValue() };
                completed >= value
            }
        };
        if !reached {
            return Ok(ReadbackState::Pending);
        }
        // The work is done, so this is also a natural moment to sweep — a caller
        // that only ever polls still drains the retire queue.
        self.inner.poll_retire(&mut state);
        if size == 0 {
            return Ok(ReadbackState::Ready);
        }
        let entry = handle::lookup(&state.buffers, "buffer", buffer, self.inner.owner)?;
        // A `D3D12_RANGE` is expressed in `usize`; only reachable on a 32-bit
        // host, where the allocation could not have existed either.
        let begin = usize::try_from(offset).map_err(|_| {
            HalError::InvalidDescriptor(format!(
                "readback offset {offset} does not fit this host's address space"
            ))
        })?;
        // The read range is not decoration on a readback heap: it is what tells
        // the runtime which bytes the CPU is about to look at, and a `None`
        // means "all of them".
        let read = D3D12_RANGE {
            Begin: begin,
            End: begin + out.len(),
        };
        let mut mapped: *mut core::ffi::c_void = core::ptr::null_mut();
        // SAFETY: `entry.raw` is a live buffer on the readback heap — checked at
        // request time and re-resolved above — and subresource 0 is the only one
        // a buffer has. Both pointers name live locals.
        unsafe { entry.raw.Map(0, Some(&read), Some(&mut mapped)) }.map_err(|error| {
            // A failed Map is where a removed device surfaces on this
            // backend — the readback is the first call that touches
            // memory the GPU was writing — so it is the one place the
            // breadcrumbs are worth most. Without this the caller gets
            // 0x887A0005 and no diagnosis, which is what a CI run
            // reproducing a device removal actually printed.
            // Still `Backend`, not `DeviceLost`: a Map can fail for reasons
            // that are not a removal, and `debug::diagnosis` returns the
            // empty string on a healthy device, so the variant must not be
            // decided by the diagnostic being attached.
            HalError::Backend(format!(
                "ID3D12Resource::Map failed: {error}{}",
                debug::diagnosis(&self.inner.raw)
            ))
        })?;
        if mapped.is_null() {
            return Err(HalError::Backend(
                "ID3D12Resource::Map reported success and wrote no pointer".to_string(),
            ));
        }
        // SAFETY: `mapped` points at the buffer's whole allocation, the range
        // `begin..begin + out.len()` was bounds-checked against its size at
        // request time and the buffer is the same one — the generational handle
        // saw to that. The two regions cannot overlap: `out` is a caller-owned
        // slice and the source is the buffer's own mapping. The fence has passed
        // `after`, so every write the submission made is complete.
        unsafe {
            core::ptr::copy_nonoverlapping(
                mapped.cast::<u8>().add(begin),
                out.as_mut_ptr(),
                out.len(),
            );
        }
        // SAFETY: the matching `Unmap`. The written range is empty because this
        // call wrote nothing.
        unsafe {
            entry.raw.Unmap(0, Some(&D3D12_RANGE { Begin: 0, End: 0 }));
        }
        Ok(ReadbackState::Ready)
    }

    /// Drops the tracking entry.
    ///
    /// There is no driver object: the mapping belongs to the buffer, which the
    /// caller still owns, and the completion point is a number.
    fn destroy_readback(&self, readback: ReadbackHandle) {
        let mut state = self.state();
        handle::take_owned(&mut state.readbacks, readback, self.inner.owner);
    }

    fn create_image(&self, desc: &ImageDesc<'_>) -> Result<ImageHandle, HalError> {
        validate::check_image(&self.inner.caps, desc)?;
        let extent = desc.extent;
        let mip_levels = desc.mip_levels.max(1);
        let samples = desc.samples.max(1);
        // One field for two meanings, which is exactly what `depth_or_layers`
        // already is: D3D12 spells a volume's depth and an array's layer count
        // in the same 16-bit `DepthOrArraySize`, so there is nothing to choose
        // between here.
        //
        // `DepthOrArraySize` and `MipLevels` are 16-bit in the resource
        // descriptor. The limit checks above bound both, so this is the
        // conversion those checks make safe rather than a second policy.
        let depth_or_array = u16::try_from(extent.depth_or_layers).map_err(|_| {
            HalError::InvalidDescriptor(format!(
                "{} layers or slices do not fit D3D12's 16-bit DepthOrArraySize",
                extent.depth_or_layers
            ))
        })?;
        let mips = u16::try_from(mip_levels).map_err(|_| {
            HalError::InvalidDescriptor(format!(
                "{mip_levels} mip levels do not fit D3D12's 16-bit MipLevels"
            ))
        })?;

        let resource_desc = D3D12_RESOURCE_DESC {
            Dimension: conv::resource_dimension(desc.image_type),
            Alignment: 0,
            Width: u64::from(extent.width),
            Height: extent.height,
            DepthOrArraySize: depth_or_array,
            MipLevels: mips,
            Format: conv::resource_format(desc.format, desc.usage),
            SampleDesc: DXGI_SAMPLE_DESC {
                Count: samples,
                // The default pattern. A non-zero quality names a vendor
                // pattern from `CheckFeatureSupport`, which is a per-format
                // query this backend does not make — and asking for one that
                // does not exist fails resource creation.
                Quality: 0,
            },
            Layout: D3D12_TEXTURE_LAYOUT_UNKNOWN,
            Flags: conv::image_flags(desc.usage),
        };
        // An image is device-local — the default heap — and `ImageDesc` has no
        // field that could say otherwise. D3D12 is the reason: its upload and
        // readback heaps admit `D3D12_RESOURCE_DIMENSION_BUFFER` only.
        let properties = heap_properties(MemoryLocation::DeviceLocal);
        let mut resource: Option<ID3D12Resource> = None;
        // SAFETY: as `create_buffer` — live descriptors borrowed for the call,
        // a live out-parameter, and the initial state D3D12 requires for the
        // default heap. No optimised clear value is passed: it is a hint, and a
        // wrong one costs more than none at all.
        unsafe {
            self.inner.raw.CreateCommittedResource(
                &properties,
                D3D12_HEAP_FLAG_NONE,
                &resource_desc,
                conv::initial_state(MemoryLocation::DeviceLocal),
                None,
                &mut resource,
            )
        }
        .map_err(|error| {
            creation_error(&self.inner.raw, "CreateCommittedResource (image)", &error)
        })?;
        let raw = resource.ok_or_else(|| {
            HalError::Backend(
                "CreateCommittedResource reported success and wrote no image".to_string(),
            )
        })?;
        if let Some(label) = desc.label {
            label_object(&raw, label);
        }

        let handle = self.state().images.insert(ImageEntry {
            owner: self.inner.owner.id,
            raw,
            format: desc.format,
            image_type: desc.image_type,
            usage: desc.usage,
            extent,
            mip_levels,
            slices: extent.depth_or_layers,
            samples,
        });
        Ok(handle::stamp(self.inner.owner, handle))
    }

    fn destroy_image(&self, image: ImageHandle) {
        let mut state = self.state();
        drop(handle::take_owned(
            &mut state.images,
            image,
            self.inner.owner,
        ));
    }

    /// Creates the descriptors a view of `desc.image` needs.
    ///
    /// # The view format must equal the image's, and that is a real gap
    ///
    /// [`ImageViewDesc::format`](crcbl_hal::ImageViewDesc::format) is documented
    /// as free to differ from its image's "for sRGB reinterpretation", and
    /// `crcbl-mtl` honours that. **This backend refuses it**, because D3D12
    /// offers two ways to allow the cast and neither is free:
    ///
    /// * Create every colour image with a **typeless** format. That works
    ///   everywhere and gives up the driver's format-specific compression on
    ///   every render target in the engine, for a capability almost none of them
    ///   use.
    /// * Query `D3D12_FEATURE_DATA_D3D12_OPTIONS3::CastingFullyTypedFormatSupported`
    ///   and allow the cast where it is reported. That makes the seam's promise
    ///   hold on some machines and not others, which is the "correct on one
    ///   class of machine" failure `crcbl-mtl`'s storage-mode decision was
    ///   written to avoid.
    ///
    /// Refusing uniformly is the only one of the three that is honest on every
    /// machine, so that is what this does — with the reason in the error. The
    /// depth spelling is *not* a reinterpretation and is handled underneath: a
    /// sampled depth image is stored typeless and its shader view names the
    /// depth plane's format, both derived from the one seam format the caller
    /// passed.
    fn create_image_view(&self, desc: &ImageViewDesc<'_>) -> Result<ImageViewHandle, HalError> {
        let mut state = self.state();
        // Everything the view needs is copied out of the entry here, so the
        // borrow of the image table ends before the descriptor heaps — in the
        // same lock — are borrowed mutably below.
        let (image, image_format, image_type, usage, extent, levels, slices, samples) = {
            let entry = handle::lookup(&state.images, "image", desc.image, self.inner.owner)?;
            (
                entry.raw.clone(),
                entry.format,
                entry.image_type,
                entry.usage,
                entry.extent,
                entry.mip_levels,
                entry.slices,
                entry.samples,
            )
        };
        if desc.format != image_format {
            return Err(HalError::InvalidDescriptor(format!(
                "a view of a {image_format:?} image cannot be created as {:?}: D3D12 permits a \
                 differing view format only on a typeless resource, and this backend stores \
                 colour images with their own format so the driver can compress them",
                desc.format
            )));
        }

        validate::check_view_type(image_type, slices, desc)?;

        let range = desc.range;
        if range.base_mip >= levels || range.base_layer >= slices {
            return Err(HalError::InvalidDescriptor(format!(
                "view starts at mip {} layer {}, and the image has {levels} mips and {slices} \
                 layers",
                range.base_mip, range.base_layer
            )));
        }
        let sub = Subresource {
            base_mip: range.base_mip,
            mip_count: resolve_count(range.mip_count, range.base_mip, levels),
            base_layer: range.base_layer,
            layer_count: resolve_count(range.layer_count, range.base_layer, slices),
            samples,
        };
        if sub.mip_count == 0 || sub.layer_count == 0 {
            return Err(HalError::InvalidDescriptor(
                "an image view covering no mip levels or no layers is not a view".to_string(),
            ));
        }
        let built = validate::build_views(image_format, usage, desc, sub)?;

        // What the attachment descriptors below address, worked out here because
        // this is the last place the image's own geometry is in hand. A render
        // target or depth stencil view covers exactly one mip — `Subresource`
        // says so — so `base_mip` is the mip, and `attached_layers` is what
        // decides how many array layers the descriptor reaches rather than how
        // many the caller's range names.
        let attached = resolve::Attachment {
            format: image_format,
            image_type,
            samples,
            subresource: subresource_index(sub.base_mip, sub.base_layer, levels),
            layer_stride: levels,
            layers: resolve::attached_layers(desc.view_type, sub.layer_count),
            extent: mip_extent(extent, image_type, sub.base_mip),
        };

        // Every slot is taken before any descriptor is written, so a heap that
        // will not grow leaves nothing half-created — and the ones already taken
        // go back rather than leaking.
        let mut descriptors = ViewDescriptors::default();
        let mut failure = None;
        for (wanted, kind, out) in [
            (
                built.shader_resource.is_some(),
                Kind::ShaderResource,
                &mut descriptors.shader_resource,
            ),
            (
                built.unordered_access.is_some(),
                Kind::ShaderResource,
                &mut descriptors.unordered_access,
            ),
            (
                built.render_target.is_some(),
                Kind::RenderTarget,
                &mut descriptors.render_target,
            ),
            (
                built.depth_stencil.is_some(),
                Kind::DepthStencil,
                &mut descriptors.depth_stencil,
            ),
        ] {
            if !wanted || failure.is_some() {
                continue;
            }
            match state.descriptors.allocate(&self.inner.raw, kind) {
                Ok(slot) => *out = Some(slot),
                Err(error) => failure = Some(error),
            }
        }
        if let Some(error) = failure {
            for slot in descriptors.slots() {
                state.descriptors.free(slot);
            }
            return Err(error);
        }

        // Writing a descriptor cannot fail and cannot report: every one of these
        // returns `void`. That is why the whole descriptor was validated in
        // `build_views` above.
        if let (Some(view_desc), Some(slot)) = (built.shader_resource, descriptors.shader_resource)
        {
            let at = state.descriptors.cpu_handle(slot);
            // SAFETY: `image` is a live resource this device created, `view_desc`
            // is a fully initialised descriptor borrowed for the call, and `at`
            // is a descriptor this device just allocated out of a
            // `CBV_SRV_UAV` heap — which is the heap type
            // `CreateShaderResourceView` writes into.
            unsafe {
                self.inner
                    .raw
                    .CreateShaderResourceView(&image, Some(&view_desc), at);
            }
        }
        if let (Some(view_desc), Some(slot)) =
            (built.unordered_access, descriptors.unordered_access)
        {
            let at = state.descriptors.cpu_handle(slot);
            // SAFETY: as above. The counter resource is `None`, which is the
            // only legal value for a texture UAV — append/consume counters are
            // a buffer feature.
            unsafe {
                self.inner
                    .raw
                    .CreateUnorderedAccessView(&image, None, Some(&view_desc), at);
            }
        }
        if let (Some(view_desc), Some(slot)) = (built.render_target, descriptors.render_target) {
            let at = state.descriptors.cpu_handle(slot);
            // SAFETY: as above, into an `RTV` heap.
            unsafe {
                self.inner
                    .raw
                    .CreateRenderTargetView(&image, Some(&view_desc), at);
            }
        }
        if let (Some(view_desc), Some(slot)) = (built.depth_stencil, descriptors.depth_stencil) {
            let at = state.descriptors.cpu_handle(slot);
            // SAFETY: as above, into a `DSV` heap.
            unsafe {
                self.inner
                    .raw
                    .CreateDepthStencilView(&image, Some(&view_desc), at);
            }
        }

        let handle = state.views.insert(ViewEntry {
            owner: self.inner.owner.id,
            descriptors,
            attached,
            image,
        });
        Ok(handle::stamp(self.inner.owner, handle))
    }

    fn destroy_image_view(&self, view: ImageViewHandle) {
        let mut state = self.state();
        let Some(entry) = handle::take_owned(&mut state.views, view, self.inner.owner) else {
            return;
        };
        for slot in entry.descriptors.slots() {
            state.descriptors.free(slot);
        }
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
        let Some(filter) = conv::filter(desc) else {
            return Err(HalError::InvalidDescriptor(format!(
                "anisotropy {} with min {:?} / mag {:?} / mip {:?} is not a D3D12 filter: \
                 D3D12_FILTER_ANISOTROPIC is linear on all three, and leaving MaxAnisotropy set \
                 beside a point filter would sample one tap while claiming {}",
                desc.anisotropy, desc.min_filter, desc.mag_filter, desc.mip_filter, desc.anisotropy
            )));
        };

        // D3D12 takes an integer here; the seam's `f32` is a Vulkan-shaped
        // spelling of the same knob. Truncating rather than rounding keeps the
        // promise a limit makes: never sample with more taps than the caller
        // asked for.
        #[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)]
        let anisotropy = desc.anisotropy.min(cap) as u32;
        let sampler_desc = D3D12_SAMPLER_DESC {
            Filter: filter,
            AddressU: conv::address_mode(desc.address_mode[0]),
            AddressV: conv::address_mode(desc.address_mode[1]),
            AddressW: conv::address_mode(desc.address_mode[2]),
            // The seam has no LOD bias, and zero is "no bias" rather than a
            // value picked here.
            MipLODBias: 0.0,
            MaxAnisotropy: anisotropy.max(1),
            // A non-comparison sampler ignores this field, and `ALWAYS` is the
            // neutral filler for it. The debug layer says so out loud —
            // `CREATE_SAMPLER_COMPARISON_FUNC_IGNORED`, "This is OK, as the
            // ComparisonFunc will simply be ignored" — which is why that id is
            // in `crate::debug`'s `ALLOWED`, with the argument and with what
            // would retire it.
            ComparisonFunc: desc
                .compare
                .map_or(D3D12_COMPARISON_FUNC_ALWAYS, conv::comparison_func),
            // Transparent black, which is what the seam's `ClampToBorder`
            // documents and what a shadow atlas needs; opaque black would put a
            // black frame around every clamped sample.
            BorderColor: [0.0, 0.0, 0.0, 0.0],
            MinLOD: desc.lod_min,
            MaxLOD: desc.lod_max,
        };

        let mut state = self.state();
        let slot = state.descriptors.allocate(&self.inner.raw, Kind::Sampler)?;
        let at = state.descriptors.cpu_handle(slot);
        // SAFETY: `sampler_desc` is a fully initialised descriptor borrowed for
        // the call, and `at` is a descriptor this device just allocated out of a
        // `SAMPLER` heap — the only heap type `CreateSampler` writes into.
        unsafe { self.inner.raw.CreateSampler(&sampler_desc, at) };

        let handle = state.samplers.insert(SamplerEntry {
            owner: self.inner.owner.id,
            slot,
        });
        Ok(handle::stamp(self.inner.owner, handle))
    }

    fn destroy_sampler(&self, sampler: SamplerHandle) {
        let mut state = self.state();
        let Some(entry) = handle::take_owned(&mut state.samplers, sampler, self.inner.owner) else {
            return;
        };
        state.descriptors.free(entry.slot);
    }

    // --- shaders and pipelines ---

    /// Validates the caller's DXIL containers and files them.
    ///
    /// See [`crate::pipeline`] for why nothing is compiled, why the containers
    /// are parsed here rather than left to the driver, and why a module holds
    /// **one container per entry point**.
    fn create_shader_module(
        &self,
        desc: &ShaderModuleDesc<'_>,
    ) -> Result<ShaderModuleHandle, HalError> {
        let entry = crate::dxil::module(desc, self.inner.owner.id)?;
        let handle = self.state().shader_modules.insert(entry);
        Ok(handle::stamp(self.inner.owner, handle))
    }

    fn destroy_shader_module(&self, module: ShaderModuleHandle) {
        let mut state = self.state();
        handle::take_owned(&mut state.shader_modules, module, self.inner.owner);
    }

    /// Plans the descriptor ranges a set becomes. See [`crate::binding`].
    ///
    /// `plan_layout` runs `BindGroupLayoutDesc::check_entries` — the seam's own
    /// rules, mesh-stage visibility among them — before adding what only D3D12
    /// refuses.
    fn create_bind_group_layout(
        &self,
        desc: &BindGroupLayoutDesc<'_>,
    ) -> Result<BindGroupLayoutHandle, HalError> {
        let record = binding::plan_layout(desc, &self.inner.caps, self.inner.owner.id)?;
        let handle = self.state().bind_group_layouts.insert(record);
        Ok(handle::stamp(self.inner.owner, handle))
    }

    fn destroy_bind_group_layout(&self, layout: BindGroupLayoutHandle) {
        let mut state = self.state();
        handle::take_owned(&mut state.bind_group_layouts, layout, self.inner.owner);
    }

    /// Takes a block in the shader-visible heaps and writes the descriptors.
    ///
    /// The entries are applied through the same path
    /// [`update_bind_group`](Self::update_bind_group) uses, so a group created
    /// with entries and one created empty and then updated are the same object.
    fn create_bind_group(&self, desc: &BindGroupDesc<'_>) -> Result<BindGroupHandle, HalError> {
        // Resolved before the lock is taken, because resolving takes it: a
        // `Mutex` is not reentrant and this device has exactly one.
        let resolved = self.resolve_bindings(desc.entries)?;
        let mut guard = self.state();
        // Through the guard once, so the pools and the heaps below are disjoint
        // field borrows rather than two borrows of the guard.
        let state = &mut *guard;
        let layout = handle::lookup(
            &state.bind_group_layouts,
            "bind group layout",
            desc.layout,
            self.inner.owner,
        )?;
        let (views, samplers) =
            binding::allocate_group(&self.inner.raw, &mut state.visible, layout, desc)?;
        let mut record = BindGroupRecord {
            owner: self.inner.owner.id,
            layout: desc.layout,
            views,
            samplers,
            // One slot per dynamic binding, in the layout's ascending order, so
            // the group's addresses and the layout's root plans index each
            // other. Unwritten until an entry names the binding, which a bind
            // refuses rather than reading as zero.
            roots: vec![binding::BoundBuffer::default(); layout.roots.len()],
            retained: retained(&resolved),
        };

        // The writes are one expression so their borrow of the layout table ends
        // before the error arm needs `state.visible` mutably to give the block
        // back — which it must, or a group that failed half way through would
        // leak its descriptors for the device's lifetime.
        let mut roots: Vec<(usize, binding::BoundBuffer)> = Vec::new();
        let written = desc.entries.iter().zip(&resolved).try_for_each(
            |(entry, resource)| -> Result<(), HalError> {
                let layout = handle::lookup(
                    &state.bind_group_layouts,
                    "bind group layout",
                    desc.layout,
                    self.inner.owner,
                )?;
                if let Some(bound) = binding::write_entry(
                    &self.inner.raw,
                    &state.visible,
                    layout,
                    &record,
                    entry,
                    resource,
                )? {
                    roots.push(bound);
                }
                Ok(())
            },
        );
        if let Err(error) = written {
            binding::free_group(&mut state.visible, &record);
            // The references were taken before the writes and are dropped with
            // the record, which never reaches a pool.
            record.retained.clear();
            return Err(error);
        }
        // After the loop rather than inside it, because a dynamic binding's
        // address is the record's and the loop borrows the record to reach the
        // heaps.
        for (index, bound) in roots {
            if let Some(slot) = record.roots.get_mut(index) {
                *slot = bound;
            }
        }
        let handle = state.bind_groups.insert(record);
        Ok(handle::stamp(self.inner.owner, handle))
    }

    /// Rewrites some of a group's descriptors.
    ///
    /// The references the group holds **grow** rather than being replaced: a
    /// descriptor this call overwrote may still be recorded into a command
    /// buffer that has not been submitted, so releasing the resource behind it
    /// now would be the use-after-free the retire queue exists to prevent. The
    /// group's own destruction is what drops them.
    fn update_bind_group(
        &self,
        group: BindGroupHandle,
        entries: &[BindGroupEntry],
    ) -> Result<(), HalError> {
        let resolved = self.resolve_bindings(entries)?;
        let mut guard = self.state();
        let state = &mut *guard;
        // Resolved before the loop rather than inside it, so an *empty* batch
        // against a destroyed group is still the `InvalidHandle` it should be
        // rather than a silent success.
        let index = {
            let record = handle::lookup(&state.bind_groups, "bind group", group, self.inner.owner)?;
            record.layout
        };
        let mut roots: Vec<(usize, binding::BoundBuffer)> = Vec::new();
        for (entry, resource) in entries.iter().zip(&resolved) {
            let record = handle::lookup(&state.bind_groups, "bind group", group, self.inner.owner)?;
            let layout = handle::lookup(
                &state.bind_group_layouts,
                "bind group layout",
                index,
                self.inner.owner,
            )?;
            if let Some(bound) = binding::write_entry(
                &self.inner.raw,
                &state.visible,
                layout,
                record,
                entry,
                resource,
            )? {
                roots.push(bound);
            }
        }
        let slot = handle::local::<BindGroupRecord, _>("bind group", group, self.inner.owner)?;
        if let Some(record) = state.bind_groups.get_mut(slot) {
            record.retained.extend(retained(&resolved));
            // As `create_bind_group`: a root descriptor's address is state on
            // the record rather than a descriptor in a heap, so it is written
            // once the loop's borrow of the record has ended.
            for (index, bound) in roots {
                if let Some(root) = record.roots.get_mut(index) {
                    *root = bound;
                }
            }
        }
        Ok(())
    }

    fn destroy_bind_group(&self, group: BindGroupHandle) {
        let mut state = self.state();
        let Some(record) = handle::take_owned(&mut state.bind_groups, group, self.inner.owner)
        else {
            return;
        };
        binding::free_group(&mut state.visible, &record);
    }

    /// Builds a root signature from the sets. See [`crate::pipeline`].
    fn create_pipeline_layout(
        &self,
        desc: &PipelineLayoutDesc<'_>,
    ) -> Result<PipelineLayoutHandle, HalError> {
        let ceiling = self.inner.caps.limits.max_bind_groups as usize;
        if desc.bind_group_layouts.len() > ceiling {
            return Err(HalError::InvalidDescriptor(format!(
                "{} bind group layouts exceed this device's limit of {ceiling}",
                desc.bind_group_layouts.len()
            )));
        }
        let mut state = self.state();
        let mut sets = Vec::with_capacity(desc.bind_group_layouts.len());
        // One running count for the whole signature, because `dxc` numbers a
        // source's registers end to end and does not restart at a set boundary.
        // See `crate::binding`.
        let mut registers = crate::dxil::Registers::default();
        for handle in desc.bind_group_layouts {
            let record = handle::lookup(
                &state.bind_group_layouts,
                "bind group layout",
                *handle,
                self.inner.owner,
            )?;
            sets.push((*handle, binding::ranges(record, &mut registers)));
        }
        // **After the sets, and that is the whole rule.** HLSL has no push
        // constants: the block is a `cbuffer` `dxc` numbers in the `b` file with
        // every other constant buffer, and `crcbl-shaders` requires each source
        // to declare it last — so its register is the one left once the
        // bindings have taken theirs. See `crate::root`.
        let push = crate::root::plan_push_constants(
            desc.push_constants,
            &mut registers,
            &self.inner.caps,
        )?;
        let entry = pipeline::layout(&self.inner.raw, desc, &sets, push, self.inner.owner.id)?;
        if let Some(label) = desc.label {
            label_object(&entry.raw, label);
        }
        let handle = state.pipeline_layouts.insert(entry);
        Ok(handle::stamp(self.inner.owner, handle))
    }

    fn destroy_pipeline_layout(&self, layout: PipelineLayoutHandle) {
        let mut state = self.state();
        handle::take_owned(&mut state.pipeline_layouts, layout, self.inner.owner);
    }

    /// Builds a `D3D12_GRAPHICS_PIPELINE_STATE_DESC` and the object from it.
    fn create_graphics_pipeline(
        &self,
        desc: &GraphicsPipelineDesc<'_>,
    ) -> Result<GraphicsPipelineHandle, HalError> {
        let ceiling = self.inner.caps.limits.max_color_attachments as usize;
        if desc.color_targets.len() > ceiling {
            return Err(HalError::InvalidDescriptor(format!(
                "{} colour targets exceed this device's limit of {ceiling}",
                desc.color_targets.len()
            )));
        }
        let mut state = self.state();
        let layout = handle::lookup(
            &state.pipeline_layouts,
            "pipeline layout",
            desc.layout,
            self.inner.owner,
        )?;
        let vertex = handle::lookup(
            &state.shader_modules,
            "shader module",
            desc.vertex.module,
            self.inner.owner,
        )?;
        let fragment = match desc.fragment {
            Some(entry) => Some(handle::lookup(
                &state.shader_modules,
                "shader module",
                entry.module,
                self.inner.owner,
            )?),
            None => None,
        };
        let entry = pipeline::graphics(
            &self.inner.raw,
            desc,
            layout,
            vertex,
            fragment,
            self.inner.owner.id,
        )?;
        if let Some(label) = desc.label {
            label_object(&entry.raw, label);
        }
        let handle = state.graphics_pipelines.insert(entry);
        Ok(handle::stamp(self.inner.owner, handle))
    }

    /// Builds a `D3D12_PIPELINE_STATE_STREAM_DESC` and the object from it.
    ///
    /// The same lookups [`create_graphics_pipeline`](Self::create_graphics_pipeline)
    /// makes, with the vertex module replaced by a mesh module and an optional
    /// amplification one, and `ID3D12Device2::CreatePipelineState` in place of
    /// `CreateGraphicsPipelineState` — see [`crate::pipeline`]'s `mesh` for why
    /// there is no other call, and [`crate::stream`] for the packing.
    ///
    /// **This backend still reports no
    /// [`Features::MESH_SHADER`](crcbl_hal::Features::MESH_SHADER)**, so a bind
    /// group layout naming [`ShaderStages::MESH`](crcbl_hal::ShaderStages::MESH)
    /// or `TASK` is still refused by
    /// [`ShaderStages::check_supported`](crcbl_hal::ShaderStages::check_supported)
    /// — a mesh pipeline built here reads its resources through sets declared
    /// `ShaderStages::ALL`, which D3D12 maps to
    /// `D3D12_SHADER_VISIBILITY_ALL` and which does reach both stages. Reporting
    /// the flag flips
    /// [`GeometryPath::from_features`](crcbl_hal::GeometryPath::from_features)
    /// to [`MeshShader`](crcbl_hal::GeometryPath::MeshShader) for every adapter,
    /// which re-keys every golden image `crcbl-render` holds; that is a separate
    /// change with a re-bless in it, and `docs/backlog.md` carries it.
    fn create_mesh_pipeline(
        &self,
        desc: &crcbl_hal::MeshPipelineDesc<'_>,
    ) -> Result<GraphicsPipelineHandle, HalError> {
        let ceiling = self.inner.caps.limits.max_color_attachments as usize;
        if desc.color_targets.len() > ceiling {
            return Err(HalError::InvalidDescriptor(format!(
                "{} colour targets exceed this device's limit of {ceiling}",
                desc.color_targets.len()
            )));
        }
        // D3D12 reads the declared size out of the DXIL and takes workgroup
        // *counts* at `DispatchMesh`, so these two fields say nothing here —
        // `crcbl_hal::MeshPipelineDesc::mesh_workgroup_size` explains that they
        // exist for Metal, which has no declaration to read. What this backend
        // can still do is refuse the values no API has a field for, so a
        // descriptor that is wrong everywhere fails here too rather than only
        // on the one backend that reads it.
        desc.check_workgroup_sizes()?;
        let mut state = self.state();
        let layout = handle::lookup(
            &state.pipeline_layouts,
            "pipeline layout",
            desc.layout,
            self.inner.owner,
        )?;
        let mesh = handle::lookup(
            &state.shader_modules,
            "shader module",
            desc.mesh.module,
            self.inner.owner,
        )?;
        let task = match desc.task {
            Some(entry) => Some(handle::lookup(
                &state.shader_modules,
                "shader module",
                entry.module,
                self.inner.owner,
            )?),
            None => None,
        };
        let fragment = match desc.fragment {
            Some(entry) => Some(handle::lookup(
                &state.shader_modules,
                "shader module",
                entry.module,
                self.inner.owner,
            )?),
            None => None,
        };
        let entry = pipeline::mesh(
            &self.inner.raw,
            desc,
            layout,
            task,
            mesh,
            fragment,
            self.inner.owner.id,
        )?;
        if let Some(label) = desc.label {
            label_object(&entry.raw, label);
        }
        let handle = state.graphics_pipelines.insert(entry);
        Ok(handle::stamp(self.inner.owner, handle))
    }

    fn destroy_graphics_pipeline(&self, pipeline: GraphicsPipelineHandle) {
        let mut state = self.state();
        handle::take_owned(&mut state.graphics_pipelines, pipeline, self.inner.owner);
    }

    /// Builds a `D3D12_COMPUTE_PIPELINE_STATE_DESC` and the object from it.
    ///
    /// The workgroup size is checked twice and each check catches something the
    /// other cannot: [`ComputePipelineDesc::check_workgroup_size`] against this
    /// device's limits, which is the half every backend performs, and
    /// `crate::pipeline`'s against the `[numthreads(…)]` the container declares,
    /// which is the half only a backend that can see the artifact's own number
    /// can.
    fn create_compute_pipeline(
        &self,
        desc: &ComputePipelineDesc<'_>,
    ) -> Result<ComputePipelineHandle, HalError> {
        desc.check_workgroup_size(&self.inner.caps.limits)?;
        let mut state = self.state();
        let layout = handle::lookup(
            &state.pipeline_layouts,
            "pipeline layout",
            desc.layout,
            self.inner.owner,
        )?;
        let module = handle::lookup(
            &state.shader_modules,
            "shader module",
            desc.compute.module,
            self.inner.owner,
        )?;
        let entry = pipeline::compute(&self.inner.raw, desc, layout, module, self.inner.owner.id)?;
        if let Some(label) = desc.label {
            label_object(&entry.raw, label);
        }
        let handle = state.compute_pipelines.insert(entry);
        Ok(handle::stamp(self.inner.owner, handle))
    }

    fn destroy_compute_pipeline(&self, pipeline: ComputePipelineHandle) {
        let mut state = self.state();
        handle::take_owned(&mut state.compute_pipelines, pipeline, self.inner.owner);
    }

    // --- queries ---

    /// Creates a query heap, and the readback buffer a result comes back
    /// through.
    ///
    /// The buffer is the part no other backend needs; [`QuerySetEntry`] says
    /// why. Both objects are created here so that a failure leaves neither
    /// behind — a heap filed in the pool with no destination would be a set
    /// [`Device::query_results`] could never answer for.
    ///
    /// # Errors
    ///
    /// [`HalError::InvalidDescriptor`] for a count of zero, or
    /// [`HalError::Backend`] / [`HalError::OutOfDeviceMemory`] from
    /// `CreateQueryHeap` or the resolve buffer's creation.
    fn create_query_set(&self, desc: &QuerySetDesc<'_>) -> Result<QuerySetHandle, HalError> {
        let bytes = query::resolve_buffer_bytes(desc.kind, desc.count)?;
        let (heap_type, query_type) = conv::query_types(desc.kind);
        let heap_desc = D3D12_QUERY_HEAP_DESC {
            Type: heap_type,
            Count: desc.count,
            NodeMask: FIRST_NODE,
        };
        let mut heap: Option<ID3D12QueryHeap> = None;
        // SAFETY: `heap_desc` is a live, fully initialised local borrowed for
        // the duration of the call, `heap` is a live `Option` the call writes
        // through, and `ID3D12QueryHeap` is the IID asked for. A failure leaves
        // it `None`, which is why it is read back rather than assumed.
        unsafe { self.inner.raw.CreateQueryHeap(&heap_desc, &mut heap) }
            .map_err(|error| creation_error(&self.inner.raw, "CreateQueryHeap", &error))?;
        let raw = heap.ok_or_else(|| {
            HalError::Backend("CreateQueryHeap reported success and wrote no heap".to_string())
        })?;

        // A plain seam buffer, so the resolve destination is allocated,
        // labelled and released by exactly the code every other buffer is.
        // `HostReadback` is what makes it mappable *and* what leaves it
        // permanently in `COPY_DEST`, which is the state `ResolveQueryData`
        // requires of a destination — so the resolve needs no barrier.
        let resolve_label = desc.label.map(|label| format!("{label} [resolve]"));
        let resolve = self.create_buffer(&BufferDesc {
            label: resolve_label.as_deref(),
            size: bytes,
            usage: BufferUsage::QUERY_RESOLVE | BufferUsage::TRANSFER_DST,
            memory: MemoryLocation::HostReadback,
        })?;

        if let Some(label) = desc.label {
            label_object(&raw, label);
        }
        let handle = self.state().query_sets.insert(QuerySetEntry {
            owner: self.inner.owner.id,
            raw,
            kind: desc.kind,
            query_type,
            count: desc.count,
            resolve,
        });
        Ok(handle::stamp(self.inner.owner, handle))
    }

    /// Releases the heap and its resolve buffer.
    ///
    /// **The heap is parked and the buffer is not**, which is the difference
    /// between the two objects rather than an inconsistency. The heap can be
    /// named by a command list a caller recorded and has not submitted yet, so
    /// it goes on [`crate::retire`]'s queue at the last value handed out, for
    /// the reason [`Device::destroy_command_buffer`] parks. The buffer is
    /// touched by exactly one list — the one [`Device::query_results`] records,
    /// submits and *waits on* inside a single call, holding its own reference
    /// throughout — so dropping this reference here can never free a resource
    /// the GPU is reading, and it is what every other `destroy_buffer` does.
    fn destroy_query_set(&self, set: QuerySetHandle) {
        let taken = {
            let mut state = self.state();
            let Some(entry) = handle::take_owned(&mut state.query_sets, set, self.inner.owner)
            else {
                return;
            };
            let at = state.next_fence_value;
            state
                .retire
                .park(at, Retired::QueryHeap { _raw: entry.raw });
            self.inner.poll_retire(&mut state);
            entry.resolve
        };
        // Outside the guard, because this takes the same lock and it is not
        // reentrant. The buffer's own reference goes here; a `query_results`
        // running concurrently holds its own clone for the length of its call,
        // and so cannot be reading a released resource.
        self.destroy_buffer(taken);
    }

    /// Reads a query set back, through a resolve and a submission of this
    /// call's own.
    ///
    /// # This is the one seam call D3D12 has no shape for
    ///
    /// The seam reads results "directly, without a resolve-to-buffer round
    /// trip", which is `vkGetQueryPoolResults` and Metal's
    /// `resolveCounterRange:`: device calls, no encoder, no queue.
    /// **`ID3D12QueryHeap` has no such call at all** — it cannot be mapped and
    /// has no `GetData`, and `ResolveQueryData` is a command list method. So the
    /// round trip is not avoidable here; it is moved *inside* this call, which
    /// records a one-shot list, submits it and blocks on the fence through
    /// [`DeviceInner::run_and_wait`] before mapping the set's own readback
    /// buffer.
    ///
    /// That is genuinely expensive — a submission and a queue drain per read —
    /// and it is why [`resolve_query_set`](crcbl_hal::CommandEncoder::resolve_query_set)
    /// exists beside it: a profiler reading a timer ring every frame should
    /// resolve into its own buffer inside the frame's command buffer and read
    /// that back through [`Device::request_readback`], which costs nothing
    /// extra. This call is for the one-off, and it is correct rather than fast.
    ///
    /// # Errors
    ///
    /// [`HalError::InvalidHandle`] or [`HalError::ForeignObject`],
    /// [`HalError::InvalidDescriptor`] if the range exceeds the set, and
    /// [`HalError::Unsupported`] for a
    /// [`QueryKind::PipelineStatistics`] set — see below.
    ///
    /// # A statistics set is refused rather than half-read
    ///
    /// `out` is one `u64` per query and D3D12 resolves a whole
    /// `D3D12_QUERY_DATA_PIPELINE_STATISTICS` — eleven of them — so there is no
    /// `out.len()` that both names a legal query range and matches what the
    /// resolve wrote. Returning the first counter would be answering a different
    /// question in the shape of this one, so this refuses and says so.
    /// `crcbl-vk` meets the same wall from the other side, where the mismatch is
    /// `VUID-vkGetQueryPoolResults-dataSize-00817`; the fix is a seam that
    /// carries a result width, and it is not this slice's.
    fn query_results(
        &self,
        set: QuerySetHandle,
        first_query: u32,
        out: &mut [u64],
    ) -> Result<(), HalError> {
        let (heap, kind, query_type, resolve) = {
            let state = self.state();
            let entry = handle::lookup(&state.query_sets, "query set", set, self.inner.owner)?;
            query::check_range(entry.count, first_query, out.len() as u64)?;
            // The refusal is written as "this kind does not resolve one `u64`
            // per query" rather than as "this kind is statistics", so it is the
            // shape of `out` that decides it — and a fourth query kind wider
            // than a `u64` would be refused here rather than reading whichever
            // of its counters happened to come first.
            if query::result_bytes(entry.kind) != size_of::<u64>() as u64 {
                return Err(HalError::Unsupported {
                    backend: BackendKind::Dx12,
                    what: "query_results reads one u64 per query and D3D12 resolves a whole \
                           D3D12_QUERY_DATA_PIPELINE_STATISTICS per statistics query; use \
                           resolve_query_set with a destination sized for that",
                });
            }
            let resolve =
                handle::lookup(&state.buffers, "buffer", entry.resolve, self.inner.owner)?
                    .raw
                    .clone();
            (entry.raw.clone(), entry.kind, entry.query_type, resolve)
        };
        if out.is_empty() {
            return Ok(());
        }

        // Every kind that reaches here resolves exactly one `u64` per query —
        // the refusal above is what guarantees it — so the bytes the resolve
        // writes and the bytes `out` holds are the same number, and the copy
        // below cannot be told to overrun either.
        let bytes = size_of_val(out);
        let offset = query::span_bytes(kind, u64::from(first_query));
        let (allocator, list) = self.inner.open_list(Some("crcbl-dx12 query_results"))?;
        // SAFETY: `list` is a live list this call just opened and is recording,
        // `heap` and `resolve` are live interfaces this device created and this
        // call holds references to for longer than the submission below, and the
        // range was bounds-checked against the heap's own query count. The
        // destination offset is a multiple of the result width, which is what
        // `AlignedDestinationBufferOffset` requires, and the destination is on
        // the readback heap and so permanently in `COPY_DEST`.
        unsafe {
            list.ResolveQueryData(
                &heap,
                query_type,
                first_query,
                out.len() as u32,
                &resolve,
                offset,
            );
        }
        // SAFETY: `list` is the list recorded into immediately above and is
        // closed exactly once — nothing else holds it in a recording state.
        unsafe { list.Close() }.map_err(|error| {
            HalError::Backend(format!(
                "ID3D12GraphicsCommandList::Close failed for a query resolve: {error}"
            ))
        })?;
        self.inner.run_and_wait(allocator, list)?;

        // A `D3D12_RANGE` is expressed in `usize`; only reachable on a 32-bit
        // host, where the resolve buffer could not have existed either.
        let begin = usize::try_from(offset).map_err(|_| {
            HalError::InvalidDescriptor(format!(
                "query offset {offset} does not fit this host's address space"
            ))
        })?;
        // The read range is not decoration on a readback heap: it is what tells
        // the runtime which bytes the CPU is about to look at.
        let read = D3D12_RANGE {
            Begin: begin,
            End: begin + bytes,
        };
        let mut mapped: *mut core::ffi::c_void = core::ptr::null_mut();
        // SAFETY: `resolve` is a live buffer on the readback heap this device
        // created with the set, and subresource 0 is the only one a buffer has.
        // Both pointers name live locals.
        unsafe { resolve.Map(0, Some(&read), Some(&mut mapped)) }.map_err(|error| {
            // A failed Map is where a removed device surfaces on this
            // backend — the readback is the first call that touches
            // memory the GPU was writing — so it is the one place the
            // breadcrumbs are worth most. Without this the caller gets
            // 0x887A0005 and no diagnosis, which is what a CI run
            // reproducing a device removal actually printed.
            // Still `Backend`, not `DeviceLost`: a Map can fail for reasons
            // that are not a removal, and `debug::diagnosis` returns the
            // empty string on a healthy device, so the variant must not be
            // decided by the diagnostic being attached.
            HalError::Backend(format!(
                "ID3D12Resource::Map failed: {error}{}",
                debug::diagnosis(&self.inner.raw)
            ))
        })?;
        if mapped.is_null() {
            return Err(HalError::Backend(
                "ID3D12Resource::Map reported success and wrote no pointer".to_string(),
            ));
        }
        // SAFETY: `mapped` points at the resolve buffer's whole allocation,
        // which `create_query_set` sized for the set's whole query count, and
        // the span was bounds-checked against that count above. The two regions
        // cannot overlap: `out` is a caller-owned slice and the source is the
        // buffer's own mapping. The fence has passed the resolve, so every byte
        // it wrote is visible.
        //
        // Copied as **bytes** into `out`, rather than as `u64`s out of the
        // mapping: `Map` promises no particular alignment for the pointer it
        // returns, so a `*const u64` built from it would be an unaligned read,
        // while `out` is a `&mut [u64]` and is aligned by construction. D3D12
        // wrote the results in this machine's own byte order, so the bytes are
        // the values.
        unsafe {
            core::ptr::copy_nonoverlapping(
                mapped.cast::<u8>().add(begin),
                out.as_mut_ptr().cast::<u8>(),
                bytes,
            );
        }
        // SAFETY: the matching `Unmap`. The written range is empty because this
        // call wrote nothing.
        unsafe {
            resolve.Unmap(0, Some(&D3D12_RANGE { Begin: 0, End: 0 }));
        }
        // The seam reports a timestamp in nanoseconds and D3D12 resolves ticks,
        // so the queue's frequency is spent here — the one place that knows it.
        // An occlusion count is a count and has no unit to convert.
        // `resolve_query_set` writes the raw ticks instead, which its own seam
        // documentation says and which this backend cannot change:
        // `ResolveQueryData` never reaches the CPU.
        if kind == QueryKind::Timestamp {
            for value in out.iter_mut() {
                *value = query::timestamp_nanos(*value, self.inner.timestamp_frequency);
            }
        }
        Ok(())
    }

    // --- synchronisation ---

    /// Creates a semaphore, which is an `ID3D12Fence` for either kind.
    ///
    /// # One primitive, two seam kinds
    ///
    /// D3D12 has no binary semaphore: `ID3D12Fence` is a monotonic `u64`
    /// counter and that is the whole of its synchronisation vocabulary. So a
    /// [`SemaphoreKind::Binary`] is the same object driven one integer at a
    /// time, with the value kept in [`SemaphoreEntry::submitted`] — a signal
    /// takes the next one and a wait takes the one most recently handed out,
    /// which is the seam's "`value` is ignored for a binary semaphore" made
    /// concrete. `crcbl-mtl` reached the same arrangement from the opposite
    /// side, where the two kinds *are* two Metal types.
    ///
    /// **So a binary semaphore must be signalled by an earlier submission than
    /// the one that waits on it**, which is how the seam says they are used —
    /// the swapchain owns them, and `crcbl_dx12::swapchain` creates none: DXGI's
    /// flip model hands back a back-buffer index synchronously, so acquire and
    /// present are the implicit shape [`AcquiredFrame`] documents and neither
    /// half needs one.
    ///
    /// `initial_value` is `CreateFence`'s own first argument, so a timeline
    /// starts where the caller asked rather than at zero and then being nudged.
    ///
    /// # Errors
    ///
    /// [`HalError::Backend`] if `CreateFence` fails.
    fn create_semaphore(&self, desc: &SemaphoreDesc<'_>) -> Result<SemaphoreHandle, HalError> {
        // **The declaration has to be true.** `supports` answers the timeline
        // rows through `Features::TIMELINE_SEMAPHORE`, and `ID3D12Fence` is core
        // D3D12 — so without this check a device opened without the feature
        // declares timelines unsupported and then builds one anyway. The same
        // inconsistency was found and fixed on `crcbl-vk` by running the seam
        // suite with `CRCBL_SEAM_WITHHOLD=all`; this backend is held to it by
        // the same step on its own CI job.
        if matches!(desc.kind, SemaphoreKind::Timeline { .. })
            && !self
                .inner
                .caps
                .features
                .contains(Features::TIMELINE_SEMAPHORE)
        {
            return Err(HalError::Unsupported {
                backend: BackendKind::Dx12,
                what: "a timeline semaphore on a device opened without Features::TIMELINE_SEMAPHORE",
            });
        }
        let initial = match desc.kind {
            SemaphoreKind::Timeline { initial_value } => initial_value,
            // A binary semaphore's counter is private to this crate, so it
            // starts at zero and the first signal is one.
            SemaphoreKind::Binary => 0,
        };
        // SAFETY: `raw` is this device's live interface. `CreateFence` takes
        // only scalars and writes the interface it returns.
        let raw: ID3D12Fence =
            unsafe { self.inner.raw.CreateFence(initial, D3D12_FENCE_FLAG_NONE) }.map_err(
                |error| {
                    HalError::Backend(format!(
                        "CreateFence failed for a semaphore: {error}{}",
                        debug::diagnosis(&self.inner.raw)
                    ))
                },
            )?;
        if let Some(label) = desc.label {
            label_object(&raw, label);
        }
        let handle = self.state().semaphores.insert(SemaphoreEntry {
            owner: self.inner.owner.id,
            raw,
            kind: desc.kind,
            submitted: initial,
        });
        Ok(handle::stamp(self.inner.owner, handle))
    }

    /// Releases a semaphore, once the queue has passed the operations naming it.
    ///
    /// The fence goes on the retire queue rather than being dropped here, for
    /// the reason [`Device::destroy_command_buffer`] gives: the seam's rule that
    /// a caller waits first is a rule above the seam, and the cost of it being
    /// broken is the driver reading a released fence out of a queued `Wait` or
    /// `Signal`. Parking costs one queue entry.
    ///
    /// The value it is parked at is the last the device fence has handed out,
    /// and that is sufficient rather than approximate: every submission issues
    /// its semaphore operations *before* the device fence's own `Signal` on the
    /// same queue, so a device fence that has reached that value has already
    /// executed every semaphore operation submitted before this call.
    fn destroy_semaphore(&self, semaphore: SemaphoreHandle) {
        let mut state = self.state();
        let Some(entry) = handle::take_owned(&mut state.semaphores, semaphore, self.inner.owner)
        else {
            return;
        };
        let at = state.next_fence_value;
        state
            .retire
            .park(at, Retired::Semaphore { _raw: entry.raw });
        self.inner.poll_retire(&mut state);
    }

    /// What the GPU has reached on a timeline.
    ///
    /// # Errors
    ///
    /// [`HalError::InvalidHandle`] or [`HalError::ForeignObject`], and
    /// [`HalError::Unsupported`] for a binary semaphore, whose counter is this
    /// crate's private bookkeeping rather than a value the seam defines.
    fn semaphore_value(&self, semaphore: SemaphoreHandle) -> Result<u64, HalError> {
        let state = self.state();
        let entry = handle::lookup(&state.semaphores, "semaphore", semaphore, self.inner.owner)?;
        if !entry.is_timeline() {
            return Err(HalError::Unsupported {
                backend: BackendKind::Dx12,
                what: "a binary semaphore has no value to read",
            });
        }
        // SAFETY: `raw` is a live fence this device created and
        // `GetCompletedValue` reads no pointer of ours, returning a `u64` by
        // value.
        Ok(unsafe { entry.raw.GetCompletedValue() })
    }

    /// Advances a timeline from the host with `ID3D12Fence::Signal`.
    ///
    /// The CPU-side twin of the queue's `Signal`, and it goes through the same
    /// [`sync::signal_value`] rule against the same floor: the highest value
    /// *submitted*, not the highest reached. Comparing against
    /// `GetCompletedValue` instead would let the host take a number a queued
    /// `Signal` is already going to use, and the fence would then go backwards
    /// when the queue got to it — silently, which is what [`crate::sync`] exists
    /// for.
    ///
    /// The floor is moved only after `Signal` returns, for the reason the
    /// submission path gives: a failed signal that had already claimed the value
    /// would block the number a retry would use.
    ///
    /// # Errors
    ///
    /// [`HalError::InvalidHandle`] or [`HalError::ForeignObject`];
    /// [`HalError::Unsupported`] for a binary semaphore, whose counter is this
    /// crate's private bookkeeping rather than a value the seam defines;
    /// [`HalError::InvalidDescriptor`] for a value that does not exceed the
    /// floor; and [`HalError::DeviceLost`] if `Signal` fails.
    fn signal_semaphore(&self, semaphore: SemaphoreHandle, value: u64) -> Result<(), HalError> {
        let mut state = self.state();
        let entry = handle::lookup(&state.semaphores, "semaphore", semaphore, self.inner.owner)?;
        if !entry.is_timeline() {
            return Err(HalError::Unsupported {
                backend: BackendKind::Dx12,
                what: "a binary semaphore has no value to signal; its counter is crcbl-dx12's own \
                       bookkeeping, driven one integer at a time by the submissions that use it",
            });
        }
        let signalled = sync::signal_value(true, entry.submitted, value)?;
        let fence = entry.raw.clone();
        // SAFETY: `fence` is a live fence this device created, and `Signal`
        // takes it and a `u64` by value.
        unsafe { fence.Signal(signalled) }.map_err(|error| {
            HalError::DeviceLost(format!(
                "ID3D12Fence::Signal failed for a semaphore: {error}{}",
                debug::diagnosis(&self.inner.raw)
            ))
        })?;
        handle::lookup_mut(
            &mut state.semaphores,
            "semaphore",
            semaphore,
            self.inner.owner,
        )?
        .submitted = signalled;
        Ok(())
    }

    /// Blocks until every wait is satisfied, or until the timeout runs out.
    ///
    /// # A timeout is `Ok(false)`, and that is what the event buys
    ///
    /// [`DeviceInner::wait_for`] passes `Duration::MAX` because a device idle
    /// has nothing to time out against, and treats a timeout as a lost device.
    /// Here the seam's contract is explicit that a timeout is "a normal outcome
    /// for a frame-pacing poll, not an error", so the same wait is given the
    /// caller's budget and `WAIT_TIMEOUT` becomes `Ok(false)`. Returning an
    /// error there would make a caller's pacing poll indistinguishable from a
    /// lost device.
    ///
    /// `ID3D12Fence` offers one wait per fence, so several waits are performed
    /// in sequence against a **shared deadline** — the same answer either way,
    /// because the seam's contract is that *all* of them must be reached, and
    /// the shared deadline is what stops N waits taking N times as long to time
    /// out. `crcbl-mtl` does exactly this for exactly this reason.
    ///
    /// The event is created and closed per wait, which is the arrangement
    /// `wgpu-hal`'s `dx12` backend uses for its own timed fence wait: the
    /// registration a timed-out wait leaves on the fence names a handle this
    /// call has closed, and `SetEvent` on a closed handle is the runtime's
    /// problem to shrug at rather than a wait that silently did not wait. A
    /// shared, longer-lived event would be worse in both directions — an
    /// auto-reset event lets one concurrent waiter consume another's signal,
    /// and a Win32 `HANDLE` is neither `Send` nor `Sync`.
    ///
    /// # Errors
    ///
    /// [`HalError::InvalidHandle`] or [`HalError::ForeignObject`],
    /// [`HalError::Unsupported`] for a binary semaphore — which D3D12 can only
    /// wait on from a queue — and [`HalError::DeviceLost`] if the wait could not
    /// be armed or returned with the fence short of its value.
    fn wait_semaphores(
        &self,
        waits: &[crcbl_hal::SemaphoreWait],
        timeout_ns: u64,
    ) -> Result<bool, HalError> {
        if waits.is_empty() {
            return Ok(true);
        }
        let mut fences = Vec::with_capacity(waits.len());
        {
            let state = self.state();
            for wait in waits {
                let entry = handle::lookup(
                    &state.semaphores,
                    "semaphore",
                    wait.semaphore,
                    self.inner.owner,
                )?;
                if !entry.is_timeline() {
                    return Err(HalError::Unsupported {
                        backend: BackendKind::Dx12,
                        what: "a binary semaphore cannot be waited on from the CPU",
                    });
                }
                fences.push((entry.raw.clone(), wait.value));
            }
        }
        // The lock is released before blocking: holding it across a wait would
        // deadlock against the very submission that is going to signal.
        let start = Instant::now();
        for (fence, value) in fences {
            let remaining = Duration::from_nanos(
                timeout_ns
                    .saturating_sub(u64::try_from(start.elapsed().as_nanos()).unwrap_or(u64::MAX)),
            );
            if !self.inner.wait_fence_until(&fence, value, remaining)? {
                return Ok(false);
            }
        }
        Ok(true)
    }

    /// Blocks until the device has finished everything submitted so far.
    ///
    /// D3D12 has no `vkDeviceWaitIdle`. What it has is the fence idiom, and this
    /// is it: signal the queue's fence with a value nobody else took, then wait
    /// for the fence to reach it. A queue executes in submission order, so the
    /// signal lands after everything submitted before it.
    ///
    /// The wait itself is outside the device lock — two callers block
    /// concurrently, they just do not race to signal — which is why the retire
    /// sweep afterwards takes the lock again rather than holding it across a
    /// block that can last a frame.
    fn wait_idle(&self) -> Result<(), HalError> {
        let value = {
            let mut state = self.state();
            self.inner.signal(&mut state)?
        };
        self.inner.wait_for(value)?;
        // An idle device is the cheapest possible moment to release what the GPU
        // has finished with, and at shutdown it is the only one left.
        let mut state = self.state();
        self.inner.poll_retire(&mut state);
        Ok(())
    }

    // --- commands ---

    /// Opens an encoder, which takes its command list immediately.
    ///
    /// The seam returns a bare `Box` here, so a queue handle from another device
    /// and a driver that would not open a list both become a failure the encoder
    /// carries to [`CommandEncoder::finish`]. See [`crate::command`].
    fn create_command_encoder(&self, desc: &CommandEncoderDesc<'_>) -> Box<dyn CommandEncoder> {
        Box::new(Dx12CommandEncoder::new(Arc::clone(&self.inner), desc))
    }

    /// Releases a command buffer, once the GPU has finished with it.
    ///
    /// The list, the allocator and every resource the recording names go on the
    /// retire queue at the last value handed out rather than being dropped here.
    /// The seam says this call must not arrive before the submission that used
    /// the buffer completed — but "must not" is a rule above the seam, and the
    /// cost of it being broken is a driver reading a freed command list. Parking
    /// costs one queue entry.
    fn destroy_command_buffer(&self, buffer: CommandBufferHandle) {
        let mut state = self.state();
        let Some(entry) = handle::take_owned(&mut state.command_buffers, buffer, self.inner.owner)
        else {
            return;
        };
        let at = state.next_fence_value;
        for resource in entry.retained {
            state.retire.park(at, Retired::Resource { _raw: resource });
        }
        for heap in entry.query_heaps {
            state.retire.park(at, Retired::QueryHeap { _raw: heap });
        }
        state.retire.park(
            at,
            Retired::Recording {
                _list: entry.list,
                _allocator: entry.allocator,
            },
        );
        self.inner.poll_retire(&mut state);
    }

    /// Executes command buffers on the queue, and signals the fence they retire
    /// against.
    ///
    /// # Waits go on the queue before the lists, signals after
    ///
    /// `ID3D12CommandQueue::Wait` and `Signal` are queue operations rather than
    /// parameters of the execute, so the submission's shape is spelled out in
    /// the order they are issued: every wait, then `ExecuteCommandLists`, then
    /// every signal. A queue processes its operations in order, which is what
    /// makes that ordering the seam's "waits happen before any command buffer
    /// runs; signals happen after all of them complete".
    ///
    /// **A wait on a value nothing has signalled is accepted**, which is
    /// [`Capability::TimelineWaitBeforeSignal`] and where this backend parts
    /// company with `crcbl-mtl`. An `ID3D12Fence` is a real object with a
    /// CPU-side `Signal` of its own, so a queue wait blocks until *somebody*
    /// signals rather than until an earlier submission on this queue does —
    /// exactly `crcbl-vk`'s reading of a `VkSemaphore`. Metal has to refuse
    /// because a wait it cannot satisfy stops the queue with no diagnostic
    /// anywhere.
    ///
    /// A **timeline** signal must exceed everything already submitted onto that
    /// semaphore, including an earlier signal in this same
    /// [`SubmitInfo`]; a fence value that went backwards would leave every
    /// waiter past it asleep for good, so it is
    /// [`HalError::InvalidDescriptor`] rather than a hang. A **binary**
    /// semaphore's `value` is ignored per the seam, and takes the next integer.
    ///
    /// # An empty submission is a no-op that still moves the fence
    ///
    /// There is no work to run, but "everything submitted before this call" is a
    /// question [`Device::request_readback`] asks of the same counter, so the
    /// value advances either way. That costs one `Signal` and keeps a readback
    /// requested after an empty submit coherent with one requested before it.
    fn submit(&self, queue: QueueHandle, submit: &SubmitInfo<'_>) -> Result<(), HalError> {
        // The queue check comes first: a handle from another device is a caller
        // bug with its own contract, and reporting it after a resolution failure
        // further down would lose it.
        self.inner.check_queue(queue)?;

        let mut state = self.state();
        // Everything is resolved before anything executes: a submission that
        // failed halfway would leave some of its command buffers running and
        // some not, which no caller can recover from.
        let mut lists: Vec<Option<ID3D12CommandList>> =
            Vec::with_capacity(submit.command_buffers.len());
        let mut held: Vec<Retired> = Vec::new();
        for &buffer in submit.command_buffers {
            let entry = handle::lookup(
                &state.command_buffers,
                "command buffer",
                buffer,
                self.inner.owner,
            )?;
            lists.push(Some(ID3D12CommandList::from(entry.list.clone())));
            held.extend(
                entry
                    .retained
                    .iter()
                    .cloned()
                    .map(|raw| Retired::Resource { _raw: raw }),
            );
            held.extend(
                entry
                    .query_heaps
                    .iter()
                    .cloned()
                    .map(|raw| Retired::QueryHeap { _raw: raw }),
            );
            held.push(Retired::Recording {
                _list: entry.list.clone(),
                _allocator: entry.allocator.clone(),
            });
        }
        let waits = self.resolve_waits(&state, submit)?;
        let signals = self.resolve_signals(&state, submit)?;

        // Every wait is issued before any list executes, which is the seam's
        // ordering rule expressed in the only place D3D12 has to express it.
        for (fence, value) in &waits {
            // SAFETY: `fence` is a live interface this device created and holds
            // a reference to for the duration of the call, and the queue is
            // externally synchronised by the state lock held here.
            unsafe { self.inner.queue.Wait(fence, *value) }.map_err(|error| {
                HalError::DeviceLost(format!(
                    "ID3D12CommandQueue::Wait failed: {error}{}",
                    debug::diagnosis(&self.inner.raw)
                ))
            })?;
        }

        if !lists.is_empty() {
            // SAFETY: every entry is a live, closed command list this device
            // created, held by the `CommandBufferEntry` it came from for the
            // duration of this call and by the retire queue below for the
            // duration of its execution. The array is a live local borrowed for
            // the call. The queue is externally synchronised by the state lock
            // held here, which is the rule `ExecuteCommandLists` imposes.
            unsafe { self.inner.queue.ExecuteCommandLists(&lists) };
        }

        // The semaphore signals go on after the lists and before the device
        // fence's own, so a caller waiting on a signalled value is waiting for
        // this submission's work and not for the next one's.
        for (semaphore, fence, value) in &signals {
            // SAFETY: as the wait above.
            unsafe { self.inner.queue.Signal(fence, *value) }.map_err(|error| {
                HalError::DeviceLost(format!(
                    "ID3D12CommandQueue::Signal failed for a semaphore: {error}{}",
                    debug::diagnosis(&self.inner.raw)
                ))
            })?;
            // Recorded only once the queue has taken it, so a failed signal
            // leaves the semaphore's floor where it was rather than blocking
            // the value a retry would use.
            handle::lookup_mut(
                &mut state.semaphores,
                "semaphore",
                *semaphore,
                self.inner.owner,
            )?
            .submitted = *value;
        }

        // Reserve and signal, then park **whatever the signal answered**: a
        // fence that will never reach the value leaks what is parked at it,
        // which is the right side to err on against releasing memory a running
        // list still names.
        let signalled = self.inner.signal(&mut state);
        let at = state.next_fence_value;
        for item in held {
            state.retire.park(at, item);
        }
        signalled?;
        self.inner.poll_retire(&mut state);
        Ok(())
    }

    // --- presentation ---

    /// Configures a flip-model swapchain on a window.
    ///
    /// The surface handle is resolved through the **instance**, not this
    /// device: obligation 2 makes surfaces instance-scoped and obligation 3
    /// says they are checked against the instance id, so any device from that
    /// instance may present to them and another instance's handle is
    /// [`HalError::ForeignObject`].
    ///
    /// # The swapchain owns its images and its views
    ///
    /// D3D12 hands back plain `ID3D12Resource` back buffers, so each one is
    /// filed in this device's image table and given a whole-image render target
    /// view — which is what makes `AcquiredFrame::view` a real object a render
    /// pass can take, rather than something every caller rebuilds. They are
    /// reissued on every reconfigure and removed with the swapchain, exactly as
    /// [`crcbl_hal::swapchain`] requires.
    ///
    /// The views carry the **caller's** format, which may be sRGB while the
    /// buffer is linear; see [`crate::swapchain`] for why that is the only way
    /// to present sRGB through flip-discard, and note that it is the one place
    /// this backend performs the differing-format cast `create_image_view`
    /// refuses — legal here because a swapchain buffer is the case D3D12
    /// permits it on.
    fn create_swapchain(&self, desc: &SwapchainDesc<'_>) -> Result<SwapchainHandle, SurfaceError> {
        let Some(hwnd) = self.inner.instance.surface_hwnd(desc.surface)? else {
            return self.create_offscreen_ring(desc);
        };
        let created = swapchain::create(
            &self.inner.instance.factory,
            &self.inner.raw,
            &self.inner.queue,
            hwnd,
            desc,
            self.inner.instance.allow_tearing,
        )?;
        let (images, views) = self.register_backbuffers(&created, desc)?;
        let entry = SwapchainEntry {
            owner: self.inner.owner.id,
            raw: Some(created.raw),
            waitable: created.waitable,
            hwnd: hwnd.0 as usize,
            extent: created.extent,
            format: desc.format,
            buffers: created.buffers,
            next_offscreen: 0,
            acquired: None,
            present_mode: created.present_mode,
            flags: created.flags,
            images,
            views,
            ledger: present::PresentLedger::default(),
        };
        let handle = self.state().swapchains.insert(entry);
        Ok(handle::stamp(self.inner.owner, handle))
    }

    /// Resizes, reformats or re-paces an existing swapchain in place.
    ///
    /// `ResizeBuffers` is the call, and it has two preconditions this method
    /// discharges in order: **every reference to the old back buffers must be
    /// gone**, and the GPU must be finished with them. So the queue is waited
    /// on — which also lets [`crate::retire`] release the references a
    /// submission was holding — and then the views and images are destroyed,
    /// and only then does DXGI see the resize. Skipping either is
    /// `E_INVALIDARG` from `ResizeBuffers` at best and a use-after-free at
    /// worst.
    ///
    /// **One reference this cannot reach is a command buffer the caller has not
    /// destroyed.** `crcbl_dx12::command`'s encoder takes its own reference to
    /// every resource it records against, and a
    /// [`CommandBufferHandle`](crcbl_hal::CommandBufferHandle) still in the
    /// caller's hand still holds it — so a caller that renders into a swapchain
    /// image and keeps the command buffer across a resize gets DXGI's refusal
    /// rather than a resized swapchain. Destroying finished command buffers is
    /// something the seam already asks for; this is the first call where
    /// forgetting is visible.
    ///
    /// The handle survives, which is the seam's promise across a resize storm —
    /// but the image and view handles do **not**: they are reissued, and a
    /// caller holding one from before this call gets
    /// [`HalError::InvalidHandle`] rather than a stale object. That is the same
    /// contract `AcquiredFrame::view` states.
    ///
    /// The present numbering starts over here, which is what
    /// [`PresentInfo::present_id`] documents: the ledger is replaced with a
    /// fresh one, so a wait for an id the old configuration presented is
    /// answered at once instead of blocking on a swapchain that has forgotten
    /// it.
    fn reconfigure_swapchain(
        &self,
        swapchain: SwapchainHandle,
        desc: &SwapchainDesc<'_>,
    ) -> Result<(), SurfaceError> {
        let hwnd = self.inner.instance.surface_hwnd(desc.surface)?;
        // Which surface this descriptor names decides which rules apply, and
        // the comparison below is what refuses a descriptor that named the
        // other kind: a window is a non-zero address and a ring is zero, so
        // "move this swapchain onto an offscreen surface" fails here rather
        // than half-way through a `ResizeBuffers` on nothing.
        let offscreen = hwnd.is_none();
        let (extent, buffers) = if offscreen {
            swapchain::check_offscreen(desc)?
        } else {
            swapchain::check(desc)?
        };
        let (old_images, old_views) = {
            let state = self.state();
            let entry =
                handle::lookup(&state.swapchains, "swapchain", swapchain, self.inner.owner)?;
            if entry.hwnd != hwnd.map_or(OFFSCREEN_HWND, |hwnd| hwnd.0 as usize) {
                return Err(SurfaceError::Hal(HalError::InvalidDescriptor(
                    "reconfigure_swapchain cannot move a swapchain to a different surface"
                        .to_string(),
                )));
            }
            if entry.format != desc.format {
                // `ResizeBuffers` does take a new format, and this backend
                // still refuses one: the views are created from the swapchain's
                // format and the entry's `format` is what a later reconfigure
                // resizes with, so a changed format would have to be threaded
                // through the failure path too. Refused by name rather than
                // half-applied.
                return Err(SurfaceError::Hal(HalError::InvalidDescriptor(format!(
                    "reconfigure_swapchain cannot change a swapchain's format from {:?} to {:?} \
                     on this backend; destroy it and create another",
                    entry.format, desc.format
                ))));
            }
            (entry.images.clone(), entry.views.clone())
        };

        // Both preconditions of `ResizeBuffers`, in the only order that works.
        self.wait_idle()?;
        for view in old_views {
            self.destroy_image_view(view);
        }
        for image in old_images {
            self.destroy_image(image);
        }
        {
            // Forgotten *now*, not when the new ones are ready. Every step
            // below can fail, and an entry left naming handles that have just
            // been destroyed would hand a dead view to the next
            // `acquire_next_frame`; empty lists make that acquire an error
            // instead, which is what a caller can act on.
            let mut state = self.state();
            let entry = handle::lookup_mut(
                &mut state.swapchains,
                "swapchain",
                swapchain,
                self.inner.owner,
            )?;
            entry.images.clear();
            entry.views.clear();
        }

        let (present_mode, images, views) = if offscreen {
            // There is nothing to resize: a ring is plain images, and the new
            // extent is simply what the replacements are created at. The old
            // ones are already gone, which is the same precondition
            // `ResizeBuffers` has and the reason both paths destroy first.
            let (images, views) = self.register_ring_images(extent, buffers, desc)?;
            (
                present::resolve_offscreen_present_mode(desc.present_mode),
                images,
                views,
            )
        } else {
            let present_mode =
                present::resolve_present_mode(desc.present_mode, self.inner.instance.allow_tearing);
            {
                let state = self.state();
                let entry =
                    handle::lookup(&state.swapchains, "swapchain", swapchain, self.inner.owner)?;
                swapchain::resize(&self.inner.raw, entry, extent, buffers)?;
            }

            // Re-fetched after the lock was released, because `create_image_view`
            // takes it: this device has one non-reentrant `Mutex`.
            let created = {
                let state = self.state();
                let entry =
                    handle::lookup(&state.swapchains, "swapchain", swapchain, self.inner.owner)?;
                swapchain::Created {
                    raw: entry
                        .raw
                        .clone()
                        .unwrap_or_else(|| unreachable!("a windowed swapchain has a DXGI object")),
                    waitable: entry.waitable,
                    extent,
                    buffers,
                    present_mode,
                    flags: entry.flags,
                }
            };
            let (images, views) = self.register_backbuffers(&created, desc)?;
            (present_mode, images, views)
        };

        let mut state = self.state();
        let entry = handle::lookup_mut(
            &mut state.swapchains,
            "swapchain",
            swapchain,
            self.inner.owner,
        )?;
        entry.extent = extent;
        entry.buffers = buffers;
        entry.present_mode = present_mode;
        entry.images = images;
        entry.views = views;
        // The ring restarts at its first image for the same reason the ledger
        // restarts: a cursor left past the end of a shorter ring would index
        // out of `images` on the very next acquire.
        entry.next_offscreen = 0;
        // The numbering restarts, per `PresentInfo::present_id`.
        entry.ledger = present::PresentLedger::default();
        Ok(())
    }

    /// Destroys a swapchain and everything it owns.
    ///
    /// The queue is waited on first, for the same reason `ResizeBuffers`
    /// demands it: releasing the last reference to a back buffer the GPU is
    /// still writing is the use-after-free [`crate::retire`] exists to prevent,
    /// arriving through the one path that queue cannot see. A failed wait is
    /// logged rather than propagated — this signature returns `()`, and a
    /// device already lost has nothing left to protect.
    fn destroy_swapchain(&self, swapchain: SwapchainHandle) {
        let (images, views) = {
            let state = self.state();
            let Ok(entry) =
                handle::lookup(&state.swapchains, "swapchain", swapchain, self.inner.owner)
            else {
                return;
            };
            (entry.images.clone(), entry.views.clone())
        };
        if let Err(error) = self.wait_idle() {
            crcbl_core::log::error!(
                "crcbl-dx12: a swapchain was destroyed with the queue unfinished: {error}"
            );
        }
        for view in views {
            self.destroy_image_view(view);
        }
        for image in images {
            self.destroy_image(image);
        }
        let mut state = self.state();
        drop(handle::take_owned(
            &mut state.swapchains,
            swapchain,
            self.inner.owner,
        ));
    }

    /// The back buffer DXGI says is next, with no synchronisation attached.
    ///
    /// **This is the implicit-acquire shape the seam documents for
    /// `crcbl-wgpu` and `crcbl-mtl`, and D3D12 is a third example of it**:
    /// there is no acquire semaphore to signal and no present semaphore to
    /// wait, because `GetCurrentBackBufferIndex` is a read of state DXGI
    /// already maintains and `Present` is ordered on the queue the swapchain
    /// was created against. Both are `None`, so the renderer's splice becomes
    /// an empty slice with no tier branch.
    ///
    /// `suboptimal` is always `false`, and that is not a stub: DXGI has no
    /// notion of a swapchain that no longer matches its window — see
    /// [`crate::swapchain`] — so there is nothing to report and inventing one
    /// would put a caller into an unending reconfigure. An offscreen ring has
    /// no window to stop matching in the first place.
    ///
    /// **An offscreen ring answers from its own cursor**, and nothing else
    /// about the call changes: the same handle lookup, the same bounds check,
    /// the same two `None` semaphores. That is what makes `crcbl screenshot`
    /// exercise the path a window uses rather than a second one.
    fn acquire_next_frame(
        &self,
        swapchain: SwapchainHandle,
    ) -> Result<AcquiredFrame, SurfaceError> {
        let mut state = self.state();
        self.inner.poll_retire(&mut state);
        let owner = self.inner.owner;
        let entry = handle::lookup_mut(&mut state.swapchains, "swapchain", swapchain, owner)?;
        let index = match entry.raw.as_ref() {
            // SAFETY: `raw` is a live swapchain this device created. The call
            // reads no pointer of ours and returns an index by value.
            Some(raw) => unsafe { raw.GetCurrentBackBufferIndex() },
            // The offscreen ring's own cursor, which `present` bumps. Same
            // implicit-acquire shape, with the rotation kept here because there
            // is no DXGI object keeping it.
            None => entry.next_offscreen,
        };
        let slot = index as usize;
        let (Some(&image), Some(&view)) = (entry.images.get(slot), entry.views.get(slot)) else {
            let source = if entry.is_offscreen() {
                "the offscreen ring's cursor is"
            } else {
                "GetCurrentBackBufferIndex answered"
            };
            return Err(SurfaceError::Hal(HalError::Backend(format!(
                "{source} {index} for a swapchain of {} image(s)",
                entry.buffers
            ))));
        };
        // Recorded only once there is a frame to hand back, so an acquire that
        // failed does not leave a present looking matched. Overwriting an index
        // already sitting here is deliberate rather than an oversight: nothing
        // in DXGI is consumed by an acquire — `GetCurrentBackBufferIndex` is a
        // read and the ring cursor only moves on a present — so a second
        // acquire costs nothing and is simply the one the next present matches.
        entry.acquired = Some(index);
        Ok(AcquiredFrame {
            image,
            view,
            extent: entry.extent,
            index,
            acquire_semaphore: None,
            present_semaphore: None,
            suboptimal: false,
        })
    }

    /// Presents the current back buffer, and numbers it if the caller asked.
    ///
    /// **There must be an outstanding acquire**, and that is checked here
    /// rather than left to D3D12, which has nothing to check it with: the
    /// implicit-acquire shape means `Present` is handed no image at all, so a
    /// present nothing acquired is a call DXGI serves. The outstanding acquire
    /// is [`SwapchainEntry::acquired`](crate::swapchain::SwapchainEntry), taken
    /// here, and a present that finds it empty is
    /// [`HalError::InvalidDescriptor`].
    ///
    /// The id is recorded **only on a present that succeeded**, which is the
    /// half that makes [`Device::wait_until_presented`]'s "no record of this
    /// id" answer mean something: a caller that spent an id on a present DXGI
    /// refused has a number nothing will ever complete, and blocking for the
    /// whole timeout on it is the worst of the available answers.
    fn present(&self, queue: QueueHandle, present: &PresentInfo<'_>) -> Result<(), SurfaceError> {
        // The queue first, as `submit` does: a handle from another device is a
        // caller bug with its own contract, and reporting it after a resolution
        // failure further down would lose it.
        self.inner.check_queue(queue)?;
        if let Some(wait) = present.waits.first() {
            // `acquire_next_frame` hands out no present semaphore — the
            // implicit-acquire shape — so a caller following the seam splices an
            // empty slice here, and `DXGI_PRESENT_PARAMETERS` has nowhere to put
            // one anyway. The handle is resolved first so the two ways of
            // getting here stay apart: a handle no device issued is
            // [`HalError::InvalidHandle`], and a live semaphore this swapchain
            // never gave out is a descriptor that named the wrong object.
            // Refused either way rather than dropped, which would present a
            // frame before the work it waits on has run.
            let state = self.state();
            handle::lookup(&state.semaphores, "semaphore", *wait, self.inner.owner)?;
            return Err(SurfaceError::Hal(HalError::InvalidDescriptor(
                "PresentInfo::waits names a semaphore, and this backend's acquire_next_frame hands \
                 none out: DXGI presents with no wait to put one in"
                    .to_string(),
            )));
        }
        let owner = self.inner.owner;
        // Resolved, then released: `Present` blocks when the frame queue is
        // full, and this device has one lock over every table.
        let (raw, mode) = {
            let mut state = self.state();
            let entry =
                handle::lookup_mut(&mut state.swapchains, "swapchain", present.swapchain, owner)?;
            // Before either shape does anything: a present with nothing behind
            // it is a caller bug, and it has to be named rather than served.
            // `Present` names no image, so DXGI would happily show the current
            // back buffer a second time and a ring would rotate onto a frame
            // nothing drew — both of which look like a rendering fault several
            // frames later instead of the mistake they are. The sentence is the
            // one `crcbl-vk` and `crcbl-mtl` already answer with; nothing
            // asserts the text, but a seam obligation reading differently on
            // each backend is a message no caller can learn.
            if entry.acquired.take().is_none() {
                return Err(SurfaceError::Hal(HalError::InvalidDescriptor(
                    "present without a matching acquire_next_frame".to_string(),
                )));
            }
            let Some(raw) = entry.raw.clone() else {
                // "Presenting" a ring image is advancing the ring. The image
                // stays valid and is reused when the cursor comes back round,
                // exactly as a back buffer is.
                //
                // The id is deliberately not recorded: nothing will ever
                // complete it, because there is no display and no waitable
                // object, and an unrecorded id is what makes
                // `wait_until_presented` answer at once instead of blocking on
                // a frame that is already as finished as it will ever be.
                entry.advance_offscreen();
                return Ok(());
            };
            (raw, entry.present_mode)
        };
        swapchain::present(&self.inner.raw, &raw, mode)?;

        let Some(id) = present.present_id else {
            return Ok(());
        };
        let mut state = self.state();
        // The frame has gone out, so a swapchain destroyed from another thread
        // while it was in flight is not a failure to report back — there is
        // nothing left to record the id on, and nothing left to wait for it.
        let Ok(entry) =
            handle::lookup_mut(&mut state.swapchains, "swapchain", present.swapchain, owner)
        else {
            crcbl_core::log::debug!(
                "crcbl-dx12: a swapchain went away during a present, so id {id} is lost"
            );
            return Ok(());
        };
        if !entry.ledger.record_present(id) {
            crcbl_core::log::debug!(
                "crcbl-dx12: present id {id} does not follow this swapchain's last, so nothing \
                 will be able to wait for it"
            );
        }
        Ok(())
    }

    /// Blocks until fewer than the swapchain's maximum frame latency presents
    /// are outstanding.
    ///
    /// # What a return actually promises here
    ///
    /// The seam's guarantee is "the numbered present is no longer waiting to
    /// happen", and it says in the same breath that it takes the **weakest** of
    /// the three platforms' answers — one of which is, in its own words, "only
    /// knows that fewer than *n* presents are still outstanding". That is
    /// exactly this one. DXGI's waitable object carries no id at all, so the id
    /// is matched against [`crate::present`]'s ledger and the *blocking* is
    /// done against a count.
    ///
    /// Two consequences worth stating rather than discovering:
    ///
    /// * The first [`present::frame_latency`] waits on a fresh swapchain return
    ///   immediately, because the object starts signalled that many times. That
    ///   is the pipeline filling, not a wait that failed.
    /// * One call blocks at most once. The seam's own advice — "ask for a frame
    ///   or more back, never the one just submitted" — is what makes that the
    ///   right cadence: a caller pacing on this calls it once per frame, which
    ///   is exactly how DXGI's object is meant to be consumed.
    ///
    /// # The seam's immediate answers
    ///
    /// Two of the three arise here. There is no "device without the
    /// capability": `crcbl_dx12::adapter` reports
    /// [`Features::PRESENT_FEEDBACK`] for every adapter and every swapchain
    /// this backend creates carries the waitable-object flag. The other two are
    /// the ledger's: an id of zero numbers nothing, and an id above the highest
    /// this swapchain **object** was given names a frame it was never asked to
    /// present — a present that failed after the caller spent the id, or one
    /// from before a `reconfigure_swapchain`, which is where `ResizeBuffers`
    /// restarts the numbering.
    ///
    /// The lock is released before the wait. Blocking with the device's one
    /// [`Mutex`] held would stall every other thread's `create_buffer` for the
    /// length of a frame.
    fn wait_until_presented(
        &self,
        swapchain: SwapchainHandle,
        present_id: u64,
        timeout: Duration,
    ) -> Result<(), SurfaceError> {
        let waitable = {
            let state = self.state();
            let entry =
                handle::lookup(&state.swapchains, "swapchain", swapchain, self.inner.owner)?;
            match entry.ledger.plan(present_id) {
                PresentWait::NothingToWaitFor => return Ok(()),
                PresentWait::Block => entry.waitable,
            }
        };
        swapchain::wait(waitable, timeout)
    }

    /// Always [`DisplayTiming::Unknown`]: this backend does not advertise
    /// [`Features::PRESENT_TIMING`](crcbl_hal::Features::PRESENT_TIMING).
    ///
    /// # What DXGI actually offers, which is less than it looks
    ///
    /// Three things come close and none of them answers the question:
    ///
    /// * `IDXGIOutput::GetFrameStatistics` gives `SyncRefreshCount` and
    ///   `SyncQPCTime`, so differencing two samples yields an *average* vblank
    ///   period. That is a measurement of what the display did, which is what
    ///   [`DisplayTiming::Fixed`] and [`DisplayTiming::Variable`] look
    ///   identical under whenever the frame rate happens to be steady — and
    ///   telling those two apart is the whole point of the query.
    /// * `DXGI_MODE_DESC::RefreshRate`, from `IDXGIOutput::GetDisplayModeList`,
    ///   gives the mode's nominal rate. On a VRR panel that is the *maximum*,
    ///   presented as though it were the cadence.
    /// * `DXGI_FEATURE_PRESENT_ALLOW_TEARING`, which this backend already
    ///   queries at instance creation, says tearing presents are permitted.
    ///   That is a precondition for variable refresh, not an observation that
    ///   it is engaged; it is equally true on a fixed-refresh monitor.
    ///
    /// So an honest DXGI implementation could reach
    /// [`DisplayTiming::Stepped`]/[`Variable`](DisplayTiming::Variable) only by
    /// inference, and reporting [`Fixed`](DisplayTiming::Fixed) from a steady
    /// average is precisely the lie this seam exists to prevent. Windows has no
    /// equivalent of `VK_EXT_present_timing`'s `refreshInterval` — a value that
    /// states the dynamics rather than letting a caller guess at them.
    ///
    /// The handle is resolved first regardless, per the seam's obligation 3.
    fn display_timing(&self, swapchain: SwapchainHandle) -> Result<DisplayTiming, SurfaceError> {
        let state = self.state();
        handle::lookup(&state.swapchains, "swapchain", swapchain, self.inner.owner)?;
        Ok(DisplayTiming::Unknown)
    }
}

/// Resolves a seam subresource count against the object's real extent.
///
/// [`ImageSubresourceRange::ALL`] means "every remaining one", and a count that
/// runs past the end is clamped rather than refused — the request is
/// satisfiable, just wider than the object, and D3D12 would read the raw number
/// as a real count of levels that do not exist.
fn resolve_count(requested: u32, base: u32, total: u32) -> u32 {
    let remaining = total.saturating_sub(base);
    if requested == ImageSubresourceRange::ALL {
        return remaining;
    }
    requested.min(remaining)
}

#[cfg(test)]
pub(crate) mod tests {
    use super::*;
    use crcbl_core::Handle;
    use crcbl_hal::{
        Barriers, BindGroupLayoutEntry, BindingFlags, BindingKind, BindingResource, BufferCopy,
        BufferImageCopy, BufferUsage, ClearValue, ColorAttachment, ColorTargetState, CompareOp,
        CompositeAlpha, ComputePassDesc, DrawIndirect, DrawIndirectCount, Extent3d, Features,
        FilterMode, ImageAspect, ImageBarrier, ImageCopy, ImageSubresourceLayers, ImageViewType,
        IndexFormat, Instance, LoadOp, MultisampleState, Offset3d, PresentMode, PrimitiveState,
        QueryKind, Rect2d, RenderPassDesc, ResourceState, SemaphoreKind, SemaphoreSignal,
        SemaphoreWait, ShaderEntry, ShaderStages, StoreOp,
    };

    use crate::instance::tests::{desc as device_desc, open as open_instance, pinned_adapter};

    /// Every [`MemoryLocation`] the seam has, so the buffer tests cover all
    /// three rather than the one that was convenient.
    const LOCATIONS: &[MemoryLocation] = &[
        MemoryLocation::DeviceLocal,
        MemoryLocation::HostUpload,
        MemoryLocation::HostReadback,
    ];

    /// A device, opened through this crate's own type so a test can reach the
    /// pools and heaps underneath it.
    ///
    /// On the adapter [`pinned_adapter`] names rather than on whichever one DXGI
    /// listed first, so a `CRCBL_DX12_ADAPTER=warp` run is a WARP run in every
    /// test and not only in the ones that happened to look.
    ///
    /// **The instance comes back wrapped in [`debug::Validated`], and that is
    /// the teardown every device test in this crate now has.** It derefs to
    /// [`Dx12Instance`](crate::Dx12Instance), so a caller reads exactly as
    /// before; what it adds is a
    /// `Drop` that asserts the debug layer was on and said nothing. Wiring it
    /// here rather than at the end of each test is the only version that cannot
    /// be forgotten by the seventy-fourth one.
    pub(crate) fn open_device() -> (debug::Validated, Dx12Device) {
        let instance = open_instance();
        let adapter = pinned_adapter(&instance);
        let device = instance
            .open_device(&device_desc(adapter))
            .expect("a D3D12 device opens with no required features");
        let validated = debug::Validated::new(instance, &device.inner.raw);
        (validated, device)
    }

    /// The render target every clear test uses.
    ///
    /// 64 texels wide is not arbitrary: at four bytes a texel that is exactly
    /// [`D3D12_TEXTURE_DATA_PITCH_ALIGNMENT`] bytes per row, which is the pitch
    /// a placed footprint must be a multiple of. A narrower target is a copy
    /// this backend refuses by name — see
    /// `a_copy_d3d12_cannot_place_is_refused_by_name`.
    const TARGET: Extent3d = Extent3d::d2(64, 4);

    /// Bytes one whole [`TARGET`] readback occupies.
    const TARGET_BYTES: usize = 64 * 4 * 4;

    /// A byte no clear and no copy in this file ever writes, so "left untouched"
    /// is distinguishable from "written with zeros".
    pub(crate) const POISON: u8 = 0xA5;

    /// The clear colour, and the bytes it must land as.
    ///
    /// Every channel differs and none is zero or `0xFF` except alpha, so a
    /// buffer that was never written, a channel swizzle and a dropped alpha are
    /// three different failures rather than one. Each value is an exact eighth
    /// of `255`'s neighbourhood — `17`, `34`, `51` — so the `f32`→`unorm8`
    /// round trip is exact and the assertion is on equality rather than a
    /// tolerance.
    const CLEAR: [f32; 4] = [17.0 / 255.0, 34.0 / 255.0, 51.0 / 255.0, 1.0];
    const CLEAR_TEXEL: [u8; 4] = [0x11, 0x22, 0x33, 0xFF];
    const OTHER: [f32; 4] = [204.0 / 255.0, 187.0 / 255.0, 170.0 / 255.0, 1.0];
    const OTHER_TEXEL: [u8; 4] = [0xCC, 0xBB, 0xAA, 0xFF];

    /// A colour target and a view of it, which is what a render pass needs.
    fn color_target(device: &Dx12Device) -> (ImageHandle, ImageViewHandle) {
        let handle = device
            .create_image(&image(
                Format::Rgba8Unorm,
                ImageUsage::COLOR_ATTACHMENT | ImageUsage::TRANSFER_SRC,
                TARGET,
            ))
            .expect("a colour target");
        let view = device
            .create_image_view(&whole(handle, Format::Rgba8Unorm))
            .expect("a render target view");
        (handle, view)
    }

    /// A readback buffer pre-filled with [`POISON`], so an assertion on its
    /// contents fails when nothing wrote them.
    pub(crate) fn readback_buffer(device: &Dx12Device, bytes: usize) -> BufferHandle {
        let handle = device
            .create_buffer(&BufferDesc {
                label: Some("crcbl-dx12 test readback"),
                size: bytes as u64,
                usage: BufferUsage::TRANSFER_DST,
                memory: MemoryLocation::HostReadback,
            })
            .expect("a readback buffer");
        device
            .write_buffer(handle, 0, &vec![POISON; bytes])
            .expect("a readback buffer is host-visible");
        handle
    }

    /// A one-attachment pass that clears `colour` over `area`.
    fn clear_pass(
        view: ImageViewHandle,
        colour: [f32; 4],
        load: LoadOp,
        area: Rect2d,
    ) -> ClearPass {
        ClearPass {
            attachment: ColorAttachment {
                view,
                resolve: None,
                load,
                store: StoreOp::Store,
                clear: ClearValue::color(colour),
            },
            area,
        }
    }

    /// A render pass descriptor's owned parts, because `RenderPassDesc` borrows
    /// its attachment slice and a temporary would not outlive the call.
    struct ClearPass {
        attachment: ColorAttachment,
        area: Rect2d,
    }

    impl ClearPass {
        fn desc(&self) -> RenderPassDesc<'_> {
            RenderPassDesc {
                label: Some("crcbl-dx12 clear"),
                color_attachments: core::slice::from_ref(&self.attachment),
                depth_stencil_attachment: None,
                render_area: self.area,
                timestamp_writes: None,
            }
        }
    }

    /// The whole of [`TARGET`], as a copy of mip zero layer zero.
    fn whole_image_copy(buffer: BufferHandle, offset: u64, image: ImageHandle) -> BufferImageCopy {
        BufferImageCopy {
            buffer,
            buffer_offset: offset,
            buffer_row_length: 0,
            buffer_image_height: 0,
            image,
            image_subresource: ImageSubresourceLayers {
                aspect: ImageAspect::COLOR,
                mip: 0,
                base_layer: 0,
                layer_count: 1,
            },
            image_offset: Offset3d::default(),
            image_extent: TARGET,
        }
    }

    /// Polls a readback to completion, with a deadline rather than a sleep.
    ///
    /// `docs/plan/12-testing.md`'s rule, and the reason the deadline is here
    /// rather than left to `slow-timeout`: a readback that never becomes ready
    /// fails as a named panic naming the stage it reached, where a bare loop
    /// would be a SIGKILL four minutes later with nothing in the log.
    pub(crate) fn drain(device: &Dx12Device, readback: ReadbackHandle, bytes: usize) -> Vec<u8> {
        let mut out = vec![POISON; bytes];
        let deadline = std::time::Instant::now() + std::time::Duration::from_secs(10);
        let mut polls = 0u64;
        loop {
            polls += 1;
            match device.poll_readback(readback, &mut out) {
                Ok(ReadbackState::Ready) => return out,
                Ok(ReadbackState::Pending) => {
                    assert!(
                        std::time::Instant::now() < deadline,
                        "a readback was still Pending after 10s and {polls} polls: the submission \
                         it waits on never completed"
                    );
                    std::thread::yield_now();
                }
                Err(error) => panic!("poll_readback after {polls} polls: {error:?}"),
            }
        }
    }

    /// The texels a whole-[`TARGET`] readback must hold when every one is
    /// `texel`.
    fn expected(texel: [u8; 4]) -> Vec<u8> {
        texel.iter().copied().cycle().take(TARGET_BYTES).collect()
    }

    /// Records, finishes and submits one encoder, panicking with the stage that
    /// failed rather than the error alone.
    ///
    /// **This is the shape the WARP question needs.** If `windows-latest`'s
    /// software rasteriser cannot execute a clear, the failure has to say
    /// whether it was recording, `finish`, `submit` or the readback — a bare
    /// timeout says only that four minutes went by.
    fn run(device: &Dx12Device, record: impl FnOnce(&mut dyn CommandEncoder)) {
        let queue = device
            .queue(QueueKind::Graphics)
            .expect("the graphics queue exists");
        let mut encoder = device.create_command_encoder(&CommandEncoderDesc {
            label: Some("crcbl-dx12 test encoder"),
            queue,
        });
        record(encoder.as_mut());
        let buffer = encoder
            .finish()
            .unwrap_or_else(|error| panic!("stage=finish: {error:?}"));
        device
            .submit(queue, &SubmitInfo::new(&[buffer]))
            .unwrap_or_else(|error| panic!("stage=submit: {error:?}"));
        device
            .wait_idle()
            .unwrap_or_else(|error| panic!("stage=wait_idle: {error:?}"));
        device.destroy_command_buffer(buffer);
    }

    /// A handle no device ever issued.
    ///
    /// Generation 1 — the lowest a `Handle` admits — and index 0, so the device
    /// tag `crcbl_dx12::handle` reads out of the index half is `0`, which that
    /// module reserves for "nobody". Every entry point that takes a handle and
    /// still refuses is offered one of these, and it is the same claim each
    /// time, so it is written once.
    fn unissued<T>() -> Handle<T> {
        Handle::from_bits(1 << 32).expect("generation 1 is non-zero")
    }

    fn buffer(size: u64, memory: MemoryLocation) -> BufferDesc<'static> {
        BufferDesc {
            label: Some("crcbl-dx12 test buffer"),
            size,
            usage: BufferUsage::STORAGE | BufferUsage::TRANSFER_DST,
            memory,
        }
    }

    fn image(format: Format, usage: ImageUsage, extent: Extent3d) -> ImageDesc<'static> {
        ImageDesc {
            label: Some("crcbl-dx12 test image"),
            image_type: ImageType::D2,
            extent,
            format,
            mip_levels: 1,
            samples: 1,
            usage,
        }
    }

    fn whole(image: ImageHandle, format: Format) -> ImageViewDesc<'static> {
        ImageViewDesc {
            label: Some("crcbl-dx12 test view"),
            image,
            view_type: ImageViewType::D2,
            format,
            range: ImageSubresourceRange::all(format),
        }
    }

    /// Reads a host-visible buffer's bytes straight out of D3D12, bypassing the
    /// seam — which has no read path yet, and which is exactly why this is the
    /// only way to observe that `write_buffer` wrote anything.
    fn read_back(device: &Dx12Device, handle: BufferHandle, len: usize) -> Vec<u8> {
        let state = device.state();
        let entry = handle::lookup(&state.buffers, "buffer", handle, device.inner.owner)
            .expect("the buffer is live and this device's");
        assert!(entry.location.is_mappable(), "not a readable buffer");
        assert!(len as u64 <= entry.size, "reading past the buffer");
        let read = D3D12_RANGE { Begin: 0, End: len };
        let mut mapped: *mut core::ffi::c_void = core::ptr::null_mut();
        // SAFETY: `entry.raw` is a live buffer on a host-visible heap, checked
        // just above, and subresource 0 is the only one a buffer has. The read
        // range is inside the allocation, and `mapped` is a live local the call
        // writes through.
        unsafe { entry.raw.Map(0, Some(&read), Some(&mut mapped)) }.expect("mapping for a read");
        assert!(!mapped.is_null(), "Map wrote no pointer");
        // SAFETY: `mapped` covers `entry.size` bytes of a live host-visible
        // allocation, `len` was just asserted to be within it, and the read
        // happens under the device lock with no GPU work in flight.
        let bytes = unsafe { core::slice::from_raw_parts(mapped.cast::<u8>(), len) }.to_vec();
        // SAFETY: the matching `Unmap`. Nothing was written, which is what the
        // empty written range says.
        unsafe {
            entry.raw.Unmap(0, Some(&D3D12_RANGE { Begin: 0, End: 0 }));
        }
        bytes
    }

    /// The square target the triangle is drawn into.
    ///
    /// Square rather than [`TARGET`]'s 64×4, because the assertions below sample
    /// *inside* the triangle near each of its three corners and a four-row target
    /// has nowhere to put them. 64 texels wide keeps the row pitch at exactly
    /// [`D3D12_TEXTURE_DATA_PITCH_ALIGNMENT`], which is the rule `plan_copy`
    /// enforces on the readback copy.
    const SQUARE: Extent3d = Extent3d::d2(64, 64);

    /// Bytes one whole [`SQUARE`] readback occupies.
    const SQUARE_BYTES: usize = 64 * 64 * 4;

    /// One texel of a [`SQUARE`] readback, as `(r, g, b, a)`.
    fn texel(bytes: &[u8], x: usize, y: usize) -> [u8; 4] {
        let at = (y * 64 + x) * 4;
        bytes[at..at + 4].try_into().expect("four bytes")
    }

    /// Asserts that a [`SQUARE`] readback holds `crcbl_shaders::triangle`'s
    /// triangle and nothing else.
    ///
    /// Shared by every draw entry point this crate implements, because the
    /// picture is the *same* one whichever call produced it — that is the whole
    /// claim an indexed or indirect draw makes, so asserting it twice from two
    /// copies would let the copies drift into two different claims.
    ///
    /// # What makes each assertion able to fail
    ///
    /// * **A corner is still the clear colour.** The clear already worked before
    ///   any of these slices, so a draw that covered the whole target — the
    ///   shape a full-screen fallback or an ignored viewport produces — fails
    ///   here.
    /// * **The centre is not the clear colour.** A draw that recorded nothing,
    ///   or a pipeline bound but never used, leaves the clear behind. For the
    ///   indexed and indirect calls this is also what a dropped index buffer or
    ///   an ignored argument offset produces, because both leave a degenerate
    ///   triangle that rasterises nothing.
    /// * **The three probes are red, green and blue dominant, in that
    ///   arrangement.** `crcbl_shaders::triangle` puts one saturated primary at
    ///   each corner precisely so a Y flip, an X mirror or a vertex-order
    ///   mistake produces a *different* picture rather than a plausible one. A
    ///   flat fill fails all three at once; a Y flip swaps the apex probe with
    ///   the two base probes; an X mirror swaps green and blue.
    /// * **Every probe's channels sum to about full scale.** Barycentric weights
    ///   sum to one and each vertex contributes full intensity in exactly one
    ///   channel, so this holds for any point inside the triangle — and fails if
    ///   the vertex stage read the wrong offsets in the storage buffer, which is
    ///   what an index buffer read at the wrong stride or offset makes it do.
    ///   That is the failure a picture "looking about right" hides.
    fn assert_triangle_drawn(bytes: &[u8], what: &str) {
        assert_eq!(
            texel(bytes, 0, 0),
            CLEAR_TEXEL,
            "{what}: the top-left corner is outside the triangle, so the draw covered more than \
             it should"
        );
        assert_eq!(
            texel(bytes, 63, 63),
            CLEAR_TEXEL,
            "{what}: the bottom-right corner is outside the triangle too"
        );
        let centre = texel(bytes, 32, 32);
        assert_ne!(
            centre, CLEAR_TEXEL,
            "{what}: the centre is inside the triangle and still holds the clear colour, so \
             nothing drew"
        );
        assert_eq!(
            centre[3], 0xFF,
            "{what}: every vertex has alpha 1: {centre:?}"
        );

        // `(column, row, which channel must dominate, what that corner is)`.
        // Each row and column is inside the triangle — see the sum assertion
        // below for the check that says so — and near exactly one of its
        // corners.
        let probes = [
            (32usize, 12usize, 0usize, "the red apex, near the top"),
            (16, 48, 2, "the blue corner, bottom left"),
            (48, 48, 1, "the green corner, bottom right"),
        ];
        for (x, y, channel, corner) in probes {
            let texel = texel(bytes, x, y);
            assert_ne!(
                texel, CLEAR_TEXEL,
                "{what}: ({x}, {y}) is inside the triangle and holds the clear colour: {corner}"
            );
            for other in 0..3 {
                if other == channel {
                    continue;
                }
                assert!(
                    texel[channel] > texel[other],
                    "{what}: ({x}, {y}) is {texel:?}, which is not dominated by channel \
                     {channel} — expected {corner}"
                );
            }
            // Barycentric weights sum to one and each vertex is saturated in
            // exactly one channel, so any interior point sums to full scale.
            let sum = u32::from(texel[0]) + u32::from(texel[1]) + u32::from(texel[2]);
            assert!(
                (250..=260).contains(&sum),
                "{what}: ({x}, {y}) is {texel:?}, summing to {sum} rather than full scale — the \
                 vertex stage is not reading the colours the storage buffer holds"
            );
        }
    }

    /// Asserts that a [`SQUARE`] readback holds the clear colour and nothing
    /// else — the frame a draw of zero instances or zero commands leaves.
    ///
    /// The inverse of [`assert_triangle_drawn`], and it is what makes each of
    /// those assertions mean something: a backend that drew unconditionally
    /// would pass every one of them and fail here.
    fn assert_nothing_drawn(bytes: &[u8], what: &str) {
        if let Some((at, found)) = bytes
            .chunks_exact(4)
            .enumerate()
            .find(|(_, texel)| *texel != CLEAR_TEXEL)
        {
            panic!(
                "{what}: texel ({}, {}) is {found:?} rather than the clear colour, so something \
                 drew when nothing should have",
                at % 64,
                at / 64
            );
        }
    }

    /// **The deliverable of DX4: a triangle WARP actually drew, read back and
    /// asserted texel by texel.**
    ///
    /// Everything in the chain is real and none of it is this crate's own
    /// bookkeeping: two DXIL containers from `crcbl-shaders`, a root signature
    /// serialised from a bind group layout, a shader-visible descriptor heap, an
    /// SRV over the vertex buffer the vertex stage *pulls* from, a
    /// `D3D12_GRAPHICS_PIPELINE_STATE_DESC`, `SetGraphicsRootDescriptorTable`
    /// and `DrawInstanced`.
    ///
    /// # What makes each assertion able to fail
    ///
    /// * **A corner is still the clear colour.** The clear already worked before
    ///   this slice, so a draw that covered the whole target — the shape a
    ///   full-screen fallback or an ignored viewport produces — fails here.
    /// * **The centre is not the clear colour.** A draw that recorded nothing,
    ///   or a pipeline bound but never used, leaves the clear behind.
    /// * **The three probes are red, green and blue dominant, in that
    ///   arrangement.** `crcbl_shaders::triangle` puts one saturated primary at
    ///   each corner precisely so a Y flip, an X mirror or a vertex-order
    ///   mistake produces a *different* picture rather than a plausible one. A
    ///   flat fill fails all three at once; a Y flip swaps the apex probe with
    ///   the two base probes; an X mirror swaps green and blue.
    /// * **Every probe's channels sum to about full scale.** Barycentric weights
    ///   sum to one and each vertex contributes full intensity in exactly one
    ///   channel, so this holds for any point inside the triangle — and fails if
    ///   the vertex stage read the wrong offsets in the storage buffer, which is
    ///   what it would do if the SRV's element stride or first element were
    ///   wrong. That is the failure a picture "looking about right" hides.
    ///
    /// # What only CI can settle
    ///
    /// All of it. This crate compiles on Windows alone and the development box
    /// is Linux, so nothing here has ever executed outside a CI runner. The
    /// panics name the stage they reached — see [`run`] — because a WARP that
    /// cannot execute a *shader* must fail legibly rather than as a timeout: it
    /// has already been shown to execute a clear and a readback, so a failure
    /// here is about shaders specifically.
    #[test]
    #[ignore = "needs a real D3D12 device; run tests/run-dx12-e2e.sh"]
    fn a_pulled_triangle_is_drawn_by_d3d12_and_read_back_texel_by_texel() {
        use crcbl_shaders::{TRIANGLE, triangle};

        let (_instance, device) = open_device();
        let target = device
            .create_image(&image(
                Format::Rgba8Unorm,
                ImageUsage::COLOR_ATTACHMENT | ImageUsage::TRANSFER_SRC,
                SQUARE,
            ))
            .expect("a colour target");
        let view = device
            .create_image_view(&whole(target, Format::Rgba8Unorm))
            .expect("a render target view");
        let readback = readback_buffer(&device, SQUARE_BYTES);

        // The geometry the vertex stage pulls, on the upload heap so it needs no
        // copy: D3D12 leaves an upload-heap buffer in `GENERIC_READ` for its
        // whole life, which already admits a shader read.
        let geometry = triangle::vertex_bytes();
        let vertices = device
            .create_buffer(&BufferDesc {
                label: Some("triangle vertices"),
                size: geometry.len() as u64,
                usage: BufferUsage::STORAGE,
                memory: MemoryLocation::HostUpload,
            })
            .expect("a vertex storage buffer");
        device
            .write_buffer(vertices, 0, &geometry)
            .expect("an upload-heap buffer is host-visible");

        let set_layout = device
            .create_bind_group_layout(&BindGroupLayoutDesc {
                label: Some("triangle geometry"),
                entries: &[BindGroupLayoutEntry {
                    binding: 0,
                    // The vertex stage alone reads it, which is what
                    // `triangle.slang` declares and what makes the root
                    // parameter's visibility a real value rather than `ALL`.
                    visibility: ShaderStages::VERTEX,
                    kind: BindingKind::StorageBuffer {
                        read_only: true,
                        dynamic: false,
                    },
                    count: 1,
                    flags: BindingFlags::empty(),
                }],
            })
            .expect("one read-only storage buffer");
        let pipeline_layout = device
            .create_pipeline_layout(&PipelineLayoutDesc {
                label: Some("triangle"),
                bind_group_layouts: &[set_layout],
                push_constants: None,
            })
            .expect("a root signature with one descriptor table");
        let group = device
            .create_bind_group(&BindGroupDesc {
                label: Some("triangle geometry"),
                layout: set_layout,
                entries: &[BindGroupEntry {
                    binding: 0,
                    array_index: 0,
                    resource: BindingResource::whole_buffer(vertices),
                }],
                variable_count: None,
            })
            .expect("a bind group over the vertex buffer");

        // **One module for both stages**, exactly as every other backend
        // creates: `dxc` compiles a single entry point per container, and the
        // seam carries a container per entry point so that shape stops at
        // `crate::pipeline`'s lookup rather than reaching the call site.
        let module = device
            .create_shader_module(&ShaderModuleDesc {
                label: Some("triangle.slang"),
                spirv: TRIANGLE.spirv(),
                wgsl: TRIANGLE.wgsl(),
                msl: TRIANGLE.msl(),
                dxil: &TRIANGLE.dxil_containers(),
            })
            .unwrap_or_else(|error| panic!("stage=create_shader_module: {error:?}"));

        let targets = [ColorTargetState::opaque(Format::Rgba8Unorm)];
        let pipeline = device
            .create_graphics_pipeline(&GraphicsPipelineDesc {
                label: Some("triangle"),
                layout: pipeline_layout,
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
            .unwrap_or_else(|error| panic!("stage=create_graphics_pipeline: {error:?}"));

        let pass = clear_pass(
            view,
            CLEAR,
            LoadOp::Clear,
            Rect2d::from_size(SQUARE.width, SQUARE.height),
        );
        run(&device, |encoder| {
            encoder.pipeline_barrier(&Barriers {
                images: &[ImageBarrier::new(
                    target,
                    ImageSubresourceRange::all(Format::Rgba8Unorm),
                    ResourceState::Undefined,
                    ResourceState::ColorAttachment,
                )],
                ..Barriers::default()
            });
            encoder.begin_render_pass(&pass.desc());
            // Pipeline before group: setting a root signature resets every root
            // argument, so the reverse order would bind a table the next call
            // discards.
            encoder.bind_graphics_pipeline(pipeline);
            encoder.bind_group(0, group, &[], pipeline_layout);
            encoder.draw(0..3, 0..1);
            encoder.end_render_pass();
            encoder.pipeline_barrier(&Barriers {
                images: &[ImageBarrier::new(
                    target,
                    ImageSubresourceRange::all(Format::Rgba8Unorm),
                    ResourceState::ColorAttachment,
                    ResourceState::TransferSrc,
                )],
                ..Barriers::default()
            });
            encoder.copy_image_to_buffer(&BufferImageCopy {
                buffer: readback,
                buffer_offset: 0,
                buffer_row_length: 0,
                buffer_image_height: 0,
                image: target,
                image_subresource: ImageSubresourceLayers {
                    aspect: ImageAspect::COLOR,
                    mip: 0,
                    base_layer: 0,
                    layer_count: 1,
                },
                image_offset: Offset3d::default(),
                image_extent: SQUARE,
            });
        });

        let request = device
            .request_readback(&ReadbackDesc {
                label: Some("crcbl-dx12 triangle readback"),
                buffer: readback,
                offset: 0,
                size: SQUARE_BYTES as u64,
                after: None,
            })
            .expect("a readback of a HostReadback buffer");
        let bytes = drain(&device, request, SQUARE_BYTES);
        assert_triangle_drawn(&bytes, "a direct draw of three vertices");

        device.destroy_readback(request);
        device.destroy_graphics_pipeline(pipeline);
        device.destroy_shader_module(module);
        device.destroy_bind_group(group);
        device.destroy_pipeline_layout(pipeline_layout);
        device.destroy_bind_group_layout(set_layout);
        device.destroy_buffer(vertices);
        device.destroy_buffer(readback);
        device.destroy_image_view(view);
        device.destroy_image(target);
    }

    /// The depth attachment a **depth-only** mesh pipeline rasterises into, and
    /// the readback that observes it.
    ///
    /// `mesh_cluster.slang` has no fragment entry point, so the probe that runs
    /// it takes `ForwardRenderer::depth_pipeline`'s shape — `fragment: None`, no
    /// colour target, and the depth attachment as the only observable. That
    /// shape is machinery `crcbl-dx12` had never built before: no test here had
    /// created a mesh pipeline without a pixel shader, and none had copied a
    /// `D32Float` image back. So it is a suspect in its own right, and
    /// [`a_depth_only_mesh_pipeline_draws_the_toy_triangle_on_this_device`]
    /// drives exactly it with the toy shader every passing mesh probe above
    /// runs.
    ///
    /// This type is what makes those two runs the *same* shape rather than two
    /// spellings of it: one image, one view, one readback, one pass descriptor,
    /// one pair of barriers and one copy, shared by both. What each test owns is
    /// its pipeline, its bindings and its dispatch.
    struct DepthProbe {
        target: ImageHandle,
        view: ImageViewHandle,
        readback: BufferHandle,
    }

    impl DepthProbe {
        fn new(device: &Dx12Device) -> Self {
            let target = device
                .create_image(&image(
                    Format::D32Float,
                    ImageUsage::DEPTH_STENCIL_ATTACHMENT | ImageUsage::TRANSFER_SRC,
                    SQUARE,
                ))
                .expect("a depth target");
            let view = device
                .create_image_view(&whole(target, Format::D32Float))
                .expect("a depth stencil view");
            // `SQUARE_BYTES` is four bytes a texel, which is what `D32Float` is
            // as well as what `Rgba8Unorm` is.
            let readback = readback_buffer(device, SQUARE_BYTES);
            Self {
                target,
                view,
                readback,
            }
        }

        /// The barrier that takes the attachment from `Undefined` to
        /// `DepthStencilWrite`.
        ///
        /// Handed back rather than recorded, because a caller with images of its
        /// own transitions them in the same `pipeline_barrier` this belongs to.
        fn acquire(&self) -> ImageBarrier {
            ImageBarrier::new(
                self.target,
                ImageSubresourceRange::all(Format::D32Float),
                ResourceState::Undefined,
                ResourceState::DepthStencilWrite,
            )
        }

        /// The pass every depth-only probe here draws through: no colour
        /// attachment, a cleared and stored depth attachment, and the whole
        /// [`SQUARE`].
        fn pass(&self, label: &'static str) -> RenderPassDesc<'static> {
            RenderPassDesc {
                label: Some(label),
                color_attachments: &[],
                depth_stencil_attachment: Some(crcbl_hal::DepthStencilAttachment {
                    view: self.view,
                    read_only: false,
                    depth_load: LoadOp::Clear,
                    depth_store: StoreOp::Store,
                    stencil_load: LoadOp::DontCare,
                    stencil_store: StoreOp::Discard,
                    clear: ClearValue::default(),
                }),
                render_area: Rect2d::from_size(SQUARE.width, SQUARE.height),
                timestamp_writes: None,
            }
        }

        /// The barrier out of the pass, for the same reason [`Self::acquire`] is
        /// returned rather than recorded.
        fn release(&self) -> ImageBarrier {
            ImageBarrier::new(
                self.target,
                ImageSubresourceRange::all(Format::D32Float),
                ResourceState::DepthStencilWrite,
                ResourceState::TransferSrc,
            )
        }

        /// Copies the whole attachment into the readback buffer. Recorded after
        /// the barrier [`Self::release`] hands back.
        fn resolve(&self, encoder: &mut dyn CommandEncoder) {
            encoder.copy_image_to_buffer(&BufferImageCopy {
                buffer: self.readback,
                buffer_offset: 0,
                buffer_row_length: 0,
                buffer_image_height: 0,
                image: self.target,
                image_subresource: ImageSubresourceLayers {
                    aspect: ImageAspect::DEPTH,
                    mip: 0,
                    base_layer: 0,
                    layer_count: 1,
                },
                image_offset: Offset3d::default(),
                image_extent: SQUARE,
            });
        }

        /// The attachment's texels, one `f32` each, in row order.
        fn read(&self, device: &Dx12Device) -> Vec<f32> {
            let request = device
                .request_readback(&ReadbackDesc {
                    label: Some("crcbl-dx12 depth readback"),
                    buffer: self.readback,
                    offset: 0,
                    size: SQUARE_BYTES as u64,
                    after: None,
                })
                .expect("a readback of a HostReadback buffer");
            let bytes = drain(device, request, SQUARE_BYTES);
            device.destroy_readback(request);
            bytes
                .chunks_exact(size_of::<f32>())
                .map(|word| f32::from_le_bytes(word.try_into().expect("four bytes")))
                .collect()
        }

        fn destroy(self, device: &Dx12Device) {
            device.destroy_buffer(self.readback);
            device.destroy_image_view(self.view);
            device.destroy_image(self.target);
        }
    }

    /// The three texels of a depth attachment both depth-only mesh probes
    /// assert, and the one place the discipline is written down.
    ///
    /// The centre must hold `emitted` and both corners the clear. **The centre
    /// is what makes a pass mean something**: a device that survived the
    /// dispatch and rasterised nothing leaves the clear in all three, which is
    /// the failure a "the frame came back" assertion cannot see. The corners are
    /// what makes it a triangle rather than a full-target write. Both triangles
    /// here are flat in Z, so these are equalities and there is deliberately no
    /// tolerance, skip or catch on either side of a call to this.
    fn assert_depth_triangle(depths: &[f32], emitted: f32, what: &str) {
        assert_eq!(
            depths.len(),
            SQUARE_BYTES / size_of::<f32>(),
            "{what}: the readback is not the whole depth attachment"
        );
        let at = |x: usize, y: usize| depths[y * SQUARE.width as usize + x];
        assert_eq!(
            at(0, 0),
            crcbl_hal::depth::CLEAR,
            "{what}: top-left corner, which the triangle does not cover"
        );
        assert_eq!(
            at(SQUARE.width as usize - 1, SQUARE.height as usize - 1),
            crcbl_hal::depth::CLEAR,
            "{what}: bottom-right corner, which the triangle does not cover"
        );
        let centre = (SQUARE.width as usize / 2, SQUARE.height as usize / 2);
        assert_eq!(
            at(centre.0, centre.1),
            emitted,
            "{what}: the mesh stage rasterised nothing over the centre of the target"
        );
    }

    /// Everything the mesh probes below share: `mesh_shader.slang`'s geometry,
    /// the root signature and bind group that reach it, and the one module all
    /// four entry points come out of.
    ///
    /// The same shape [`IndexedTriangle`] has, and for the same reason — what
    /// the tests vary is the *dispatch* and nothing else, so a picture is
    /// evidence about which call produced it. The attachment, readback and
    /// pipeline are per-frame rather than shared: see [`Self::frame`].
    ///
    /// # The bind group is `ShaderStages::ALL`, not `MESH`
    ///
    /// `mesh_shader.slang` declares its geometry buffer mesh-visible, and this
    /// backend reports no
    /// [`Features::MESH_SHADER`](crcbl_hal::Features::MESH_SHADER) yet — so
    /// [`ShaderStages::check_supported`](crcbl_hal::ShaderStages::check_supported)
    /// refuses a layout naming that bit, and rightly. `ALL` is
    /// `D3D12_SHADER_VISIBILITY_ALL`, which does reach the amplification and
    /// mesh stages; the slice that reports the flag is what lets this say `MESH`
    /// and get `D3D12_SHADER_VISIBILITY_MESH` instead.
    struct MeshProbe {
        vertices: BufferHandle,
        set_layout: BindGroupLayoutHandle,
        pipeline_layout: PipelineLayoutHandle,
        group: BindGroupHandle,
        module: ShaderModuleHandle,
    }

    impl MeshProbe {
        /// `writes` is the **writable** storage buffers this probe's set binds
        /// after the geometry, at bindings 1 and up in the order given.
        ///
        /// Every probe whose shaders write nothing passes `&[]`, which builds
        /// the one-binding layout and bind group this type has always built —
        /// the same root signature, the same descriptor table, the same
        /// registers. Only
        /// [`storage_writes_from_the_amplification_stage_do_not_remove_the_device`]
        /// passes anything, because its amplification stage is the first here
        /// that writes.
        fn new(device: &Dx12Device, writes: &[BufferHandle]) -> Self {
            use crcbl_shaders::{MESH_SHADER, mesh_shader};

            let geometry = mesh_shader::vertex_bytes();
            let vertices = device
                .create_buffer(&BufferDesc {
                    label: Some("mesh_shader vertices"),
                    size: geometry.len() as u64,
                    usage: BufferUsage::STORAGE,
                    memory: MemoryLocation::HostUpload,
                })
                .expect("a vertex storage buffer");
            device
                .write_buffer(vertices, 0, &geometry)
                .expect("an upload-heap buffer is host-visible");

            // The geometry, then one writable binding per buffer the caller
            // named. `task_write_probe.slang` declares its two in the same
            // order, which is what puts them on the registers this layout
            // assigns: a read-only storage binding is an SRV and a writable one
            // a UAV, so the geometry is `t0` and these are `u0` and `u1`.
            let mut layout_entries = vec![BindGroupLayoutEntry {
                binding: 0,
                // See the doc comment on why this is not `MESH`.
                visibility: ShaderStages::ALL,
                kind: BindingKind::StorageBuffer {
                    read_only: true,
                    dynamic: false,
                },
                count: 1,
                flags: BindingFlags::empty(),
            }];
            layout_entries.extend((0..writes.len()).map(|index| BindGroupLayoutEntry {
                binding: 1 + index as u32,
                visibility: ShaderStages::ALL,
                kind: BindingKind::StorageBuffer {
                    read_only: false,
                    dynamic: false,
                },
                count: 1,
                flags: BindingFlags::empty(),
            }));
            let set_layout = device
                .create_bind_group_layout(&BindGroupLayoutDesc {
                    label: Some("mesh_shader geometry"),
                    entries: &layout_entries,
                })
                .expect("a read-only storage buffer and the caller's writable ones");
            let pipeline_layout = device
                .create_pipeline_layout(&PipelineLayoutDesc {
                    label: Some("mesh_shader"),
                    bind_group_layouts: &[set_layout],
                    push_constants: None,
                })
                .expect("a root signature with one descriptor table");
            let mut bound = vec![BindGroupEntry {
                binding: 0,
                array_index: 0,
                resource: BindingResource::whole_buffer(vertices),
            }];
            bound.extend(
                writes
                    .iter()
                    .enumerate()
                    .map(|(index, buffer)| BindGroupEntry {
                        binding: 1 + index as u32,
                        array_index: 0,
                        resource: BindingResource::whole_buffer(*buffer),
                    }),
            );
            let group = device
                .create_bind_group(&BindGroupDesc {
                    label: Some("mesh_shader geometry"),
                    layout: set_layout,
                    entries: &bound,
                    variable_count: None,
                })
                .expect("a bind group over the vertex buffer and the caller's writable ones");

            // One module for all four entry points, as everywhere else in this
            // backend. `mesh_shader.slang` declares no WGSL target, so the
            // descriptor carries DXIL alone.
            let module = device
                .create_shader_module(&ShaderModuleDesc {
                    label: Some("mesh_shader.slang"),
                    dxil: &MESH_SHADER.dxil_containers(),
                    ..ShaderModuleDesc::default()
                })
                .unwrap_or_else(|error| panic!("stage=create_shader_module: {error:?}"));

            Self {
                vertices,
                set_layout,
                pipeline_layout,
                group,
                module,
            }
        }

        /// One stage of a pipeline, named in this probe's own module.
        fn entry(&self, entry_point: &'static str) -> ShaderEntry<'static> {
            ShaderEntry {
                module: self.module,
                entry_point,
            }
        }

        /// Draws one frame through a mesh pipeline over `entry` — with `task` as
        /// its amplification stage when there is one — and reads it back.
        ///
        /// `dispatch` is the *only* thing a caller varies. It is recorded with the
        /// pipeline and the bind group already bound, inside the render pass, where
        /// [`a_mesh_pipeline_draws_through_d3d12_and_its_amplification_stage_is_visible`]
        /// records a direct `DispatchMesh` and
        /// [`an_indirect_mesh_dispatch_of_the_same_extents_draws_the_same_triangle`]
        /// records an `ExecuteIndirect` of the same three extents.
        ///
        /// `task` is a whole [`ShaderEntry`] rather than an entry-point name
        /// because the amplification stage is the one stage a caller here may
        /// take from a *different* module: D3D12 resolves each stage's DXIL
        /// container on its own, and
        /// [`a_zero_group_dispatch_mesh_does_not_remove_the_device`] uses that to
        /// pair `zero_dispatch_probe.slang`'s stage with this module's mesh and
        /// fragment containers unchanged. [`Self::entry`] spells the ordinary
        /// case.
        fn frame(
            &self,
            device: &Dx12Device,
            label: &'static str,
            task: Option<ShaderEntry<'static>>,
            entry: &'static str,
            dispatch: impl FnOnce(&mut dyn CommandEncoder),
        ) -> Vec<u8> {
            // Off `self` and into locals, so the body below is the closure it was
            // and the encoder closure captures three `Copy` handles rather than
            // borrowing the probe.
            let Self {
                pipeline_layout,
                group,
                module,
                ..
            } = *self;
            let targets = [ColorTargetState::opaque(Format::Rgba8Unorm)];
            // One frame: its own attachment and readback, so the two runs never
            // share an image whose state the second barrier would have to guess.
            let target = device
                .create_image(&image(
                    Format::Rgba8Unorm,
                    ImageUsage::COLOR_ATTACHMENT | ImageUsage::TRANSFER_SRC,
                    SQUARE,
                ))
                .expect("a colour target");
            let view = device
                .create_image_view(&whole(target, Format::Rgba8Unorm))
                .expect("a render target view");
            let readback = readback_buffer(device, SQUARE_BYTES);
            let pipeline = device
                .create_mesh_pipeline(&crcbl_hal::MeshPipelineDesc {
                    label: Some(label),
                    layout: pipeline_layout,
                    task,
                    // `mesh_shader.slang`'s own numbers. D3D12 reads them out
                    // of the DXIL and ignores these; they are here because
                    // Metal cannot.
                    task_workgroup_size: [1, 1, 1],
                    mesh: ShaderEntry {
                        module,
                        entry_point: entry,
                    },
                    mesh_workgroup_size: [3, 1, 1],
                    fragment: Some(ShaderEntry {
                        module,
                        entry_point: "fragmentMain",
                    }),
                    // `topology` is ignored here — the mesh shader's own
                    // `[outputtopology("triangle")]` decides it — and the
                    // default's winding and cull mode are what the three
                    // vertices were authored for.
                    primitive: PrimitiveState::default(),
                    depth_stencil: None,
                    multisample: MultisampleState::default(),
                    color_targets: &targets,
                })
                .unwrap_or_else(|error| panic!("stage=create_mesh_pipeline {label}: {error:?}"));

            let pass = clear_pass(
                view,
                CLEAR,
                LoadOp::Clear,
                Rect2d::from_size(SQUARE.width, SQUARE.height),
            );
            run(device, |encoder| {
                encoder.pipeline_barrier(&Barriers {
                    images: &[ImageBarrier::new(
                        target,
                        ImageSubresourceRange::all(Format::Rgba8Unorm),
                        ResourceState::Undefined,
                        ResourceState::ColorAttachment,
                    )],
                    ..Barriers::default()
                });
                encoder.begin_render_pass(&pass.desc());
                encoder.bind_graphics_pipeline(pipeline);
                encoder.bind_group(0, group, &[], pipeline_layout);
                dispatch(encoder);
                encoder.end_render_pass();
                encoder.pipeline_barrier(&Barriers {
                    images: &[ImageBarrier::new(
                        target,
                        ImageSubresourceRange::all(Format::Rgba8Unorm),
                        ResourceState::ColorAttachment,
                        ResourceState::TransferSrc,
                    )],
                    ..Barriers::default()
                });
                encoder.copy_image_to_buffer(&BufferImageCopy {
                    buffer: readback,
                    buffer_offset: 0,
                    buffer_row_length: 0,
                    buffer_image_height: 0,
                    image: target,
                    image_subresource: ImageSubresourceLayers {
                        aspect: ImageAspect::COLOR,
                        mip: 0,
                        base_layer: 0,
                        layer_count: 1,
                    },
                    image_offset: Offset3d::default(),
                    image_extent: SQUARE,
                });
            });

            // Between the submit and the `Map`, because `DXGI_ERROR_DEVICE_REMOVED`
            // is reported at the *next* call: a frame that took the device down
            // otherwise arrives as `ID3D12Resource::Map failed` and a code, where
            // `still_alive` names `GetDeviceRemovedReason` and DRED's breadcrumbs.
            still_alive(device, label);

            let request = device
                .request_readback(&ReadbackDesc {
                    label: Some("crcbl-dx12 mesh readback"),
                    buffer: readback,
                    offset: 0,
                    size: SQUARE_BYTES as u64,
                    after: None,
                })
                .expect("a readback of a HostReadback buffer");
            let bytes = drain(device, request, SQUARE_BYTES);

            device.destroy_readback(request);
            device.destroy_graphics_pipeline(pipeline);
            device.destroy_buffer(readback);
            device.destroy_image_view(view);
            device.destroy_image(target);
            bytes
        }

        /// [`Self::frame`]'s **depth-only** twin: the same stages and the same
        /// bind group through a pipeline with `fragment: None` and no colour
        /// target, rasterising into a [`DepthProbe`] instead.
        ///
        /// A separate method rather than a flag on `frame`, because almost
        /// nothing survives the change: the pipeline, the pass, both barriers
        /// and the copy all differ, and the two would share a body that was a
        /// `match` on the flag. What they do share — the depth attachment, its
        /// readback and the three texels asserted on it — is in [`DepthProbe`]
        /// and [`assert_depth_triangle`], which is also what
        /// [`the_cluster_shaders_dag_descent_draws_the_cut_it_chose`] uses.
        ///
        /// Returns the attachment's texels, one `f32` each, in row order.
        fn depth_frame(
            &self,
            device: &Dx12Device,
            label: &'static str,
            task: Option<ShaderEntry<'static>>,
            entry: &'static str,
            dispatch: impl FnOnce(&mut dyn CommandEncoder),
        ) -> Vec<f32> {
            let Self {
                pipeline_layout,
                group,
                module,
                ..
            } = *self;
            let depth = DepthProbe::new(device);
            let pipeline = device
                .create_mesh_pipeline(&crcbl_hal::MeshPipelineDesc {
                    label: Some(label),
                    layout: pipeline_layout,
                    task,
                    // `mesh_shader.slang`'s own numbers, as in `frame` — D3D12
                    // reads them out of the DXIL and ignores these.
                    task_workgroup_size: [1, 1, 1],
                    mesh: ShaderEntry {
                        module,
                        entry_point: entry,
                    },
                    mesh_workgroup_size: [3, 1, 1],
                    // The whole point of this method. `mesh_shader.slang`'s
                    // `fragmentMain` exists and is deliberately not named: what
                    // is under test is the pipeline D3D12 builds without one.
                    fragment: None,
                    primitive: PrimitiveState::default(),
                    depth_stencil: Some(crcbl_hal::DepthStencilState::default()),
                    multisample: MultisampleState::default(),
                    color_targets: &[],
                })
                .unwrap_or_else(|error| panic!("stage=create_mesh_pipeline {label}: {error:?}"));

            let pass = depth.pass(label);
            run(device, |encoder| {
                encoder.pipeline_barrier(&Barriers {
                    images: &[depth.acquire()],
                    ..Barriers::default()
                });
                encoder.begin_render_pass(&pass);
                encoder.bind_graphics_pipeline(pipeline);
                encoder.bind_group(0, group, &[], pipeline_layout);
                dispatch(encoder);
                encoder.end_render_pass();
                encoder.pipeline_barrier(&Barriers {
                    images: &[depth.release()],
                    ..Barriers::default()
                });
                depth.resolve(encoder);
            });

            // Between the submit and the `Map`, for the reason [`Self::frame`]
            // gives: `DXGI_ERROR_DEVICE_REMOVED` is reported at the *next* call.
            still_alive(device, label);

            let depths = depth.read(device);
            device.destroy_graphics_pipeline(pipeline);
            depth.destroy(device);
            depths
        }

        fn destroy(self, device: &Dx12Device) {
            device.destroy_shader_module(self.module);
            device.destroy_bind_group(self.group);
            device.destroy_pipeline_layout(self.pipeline_layout);
            device.destroy_bind_group_layout(self.set_layout);
            device.destroy_buffer(self.vertices);
        }
    }

    /// **The mesh path, end to end: a packed subobject stream, `DispatchMesh`,
    /// and a triangle read back — twice, so the amplification stage is a
    /// difference rather than an assumption.**
    ///
    /// This is what proves [`crate::stream`]'s arithmetic against a real
    /// runtime. Every other check on it is arithmetic checked against
    /// arithmetic; a subobject packed at the wrong offset still adds up, and
    /// what disagrees is `CreatePipelineState` — which on this job runs with the
    /// debug layer on, so a malformed stream arrives as a named message in the
    /// info queue `open_device`'s `Validated` asserts is clean on drop, rather
    /// than as a wrong picture.
    ///
    /// # Why it is run twice
    ///
    /// `taskMain` tints every colour by `(0, 1, 1, 1)`, so a frame drawn through
    /// the amplification stage has the **red channel killed** and nothing else
    /// changed. Drawing without it and with it and comparing the same texel is
    /// what makes the `AS` subobject observable: a stream that dropped it, or
    /// packed it where the runtime did not look, draws the identical picture the
    /// mesh-only pipeline does — which is exactly the failure a single
    /// "something was drawn" assertion cannot see. `crcbl-shaders`'
    /// `mesh_shader.slang` says the same thing about its own payload.
    ///
    /// # The pipeline is [`MeshProbe`]'s
    ///
    /// Geometry, root signature, bind group and module all come from there, and
    /// so does the frame; the line this test owns is the `draw_mesh_tasks` below.
    /// That is what lets
    /// [`an_indirect_mesh_dispatch_of_the_same_extents_draws_the_same_triangle`]
    /// differ from it in one call and nothing else.
    #[test]
    #[ignore = "needs a real D3D12 device; run tests/run-dx12-e2e.sh"]
    fn a_mesh_pipeline_draws_through_d3d12_and_its_amplification_stage_is_visible() {
        let (_instance, device) = open_device();
        let probe = MeshProbe::new(&device, &[]);

        // One workgroup: `meshMain` and `amplifiedMeshMain` are
        // `[numthreads(3, 1, 1)]`, one thread per vertex, and `taskMain`
        // is `[numthreads(1, 1, 1)]` and dispatches one mesh group.
        let dispatch = |encoder: &mut dyn CommandEncoder| encoder.draw_mesh_tasks(1, 1, 1);
        let plain = probe.frame(&device, "mesh_shader", None, "meshMain", dispatch);
        let amplified = probe.frame(
            &device,
            "mesh_shader amplified",
            Some(probe.entry("taskMain")),
            "amplifiedMeshMain",
            dispatch,
        );

        // The triangle is apex-*down* and covers the centre of the target; both
        // corners are outside it whichever way the rasteriser reads Y, which is
        // what makes this assertion independent of the winding question the
        // golden images settle elsewhere.
        for (what, bytes) in [("mesh only", &plain), ("amplified", &amplified)] {
            assert_eq!(texel(bytes, 0, 0), CLEAR_TEXEL, "{what}: top-left corner");
            assert_eq!(
                texel(bytes, SQUARE.width as usize - 1, SQUARE.height as usize - 1),
                CLEAR_TEXEL,
                "{what}: bottom-right corner"
            );
            assert_ne!(
                texel(bytes, 32, 32),
                CLEAR_TEXEL,
                "{what}: the mesh stage emitted nothing over the centre of the target"
            );
        }

        // **The amplification stage, as a difference.** `taskMain`'s tint is
        // exactly `(0, 1, 1, 1)`, so the red channel goes to zero and the other
        // three are multiplied by one — bit-identical inputs to the same
        // interpolation, so this is an equality rather than a tolerance.
        let centre = texel(&plain, 32, 32);
        let tinted = texel(&amplified, 32, 32);
        assert!(
            centre[0] > 0,
            "the mesh-only frame must have red to lose: {centre:?}"
        );
        assert_eq!(
            tinted,
            [0, centre[1], centre[2], centre[3]],
            "the amplification stage's payload did not reach the mesh stage: {centre:?} became \
             {tinted:?}"
        );

        probe.destroy(&device);
    }

    /// **`ExecuteIndirect` of a `DISPATCH_MESH` signature, drawing the triangle
    /// the direct `DispatchMesh` above already draws — the one call the D3D12
    /// mesh failure is narrowed to.**
    ///
    /// # What it distinguishes
    ///
    /// `docs/backlog.md` records a WARP that reports `MeshShaderTier = TIER_1`,
    /// passes every mesh test this crate has, and then loses the device inside
    /// `crcbl-render`'s frame — `DXGI_ERROR_DEVICE_REMOVED` out of
    /// `ID3D12Resource::Map`, no debug-layer error, no DRED breadcrumb. The two
    /// paths differ in exactly one call:
    ///
    /// * [`a_mesh_pipeline_draws_through_d3d12_and_its_amplification_stage_is_visible`]
    ///   — the probe that passes — records `draw_mesh_tasks`, a **direct**
    ///   `DispatchMesh` of three CPU scalars.
    /// * `crcbl_render::forward`'s mesh arm — the frame that dies — records
    ///   `draw_mesh_tasks_indirect`, an **`ExecuteIndirect`** through a
    ///   `D3D12_INDIRECT_ARGUMENT_TYPE_DISPATCH_MESH` signature whose extents a
    ///   compute pass wrote.
    ///
    /// Nothing had ever *executed* the second one on this backend. Where
    /// `draw_mesh_tasks_indirect` appears elsewhere in this suite it is
    /// [`the_entry_points_that_landed_never_answer_unsupported_again`], whose
    /// subject is that recording it with a dead handle refuses — nothing is ever
    /// submitted. So this is the first indirect mesh dispatch the crate has run.
    ///
    /// # What each outcome says
    ///
    /// * **It draws.** `ExecuteIndirect` of `DISPATCH_MESH` is fine on this
    ///   device, and the blocker is something larger in the renderer's frame —
    ///   the amplification stage descending the cluster DAG, the bind groups it
    ///   reads, or the extents the culling pass wrote — not the call itself.
    /// * **The device goes away.** The blocker narrows from "our renderer" to one
    ///   call with a minimal repro: the same module, layout, bind group, target
    ///   and workgroup count as the passing probe, differing only in how the
    ///   three extents reach the mesh stage. That is a bug report someone else
    ///   can run, and it is why this test carries no skip and no tolerance — a
    ///   removal is the measurement, so it must be a red test and not a quiet
    ///   one. [`MeshProbe::frame`]'s `still_alive` names the removal reason and
    ///   DRED's breadcrumbs, so the log says more than `Map` failed.
    ///
    /// # What only CI can settle
    ///
    /// All of it. This crate compiles on Windows alone and the development box
    /// is Linux, so nothing here has ever executed outside a CI runner.
    #[test]
    #[ignore = "needs a real D3D12 device; run tests/run-dx12-e2e.sh"]
    fn an_indirect_mesh_dispatch_of_the_same_extents_draws_the_same_triangle() {
        let (_instance, device) = open_device();
        let probe = MeshProbe::new(&device, &[]);

        // One `D3D12_DISPATCH_MESH_ARGUMENTS`: the three `u32`s the direct probe
        // passes to `DispatchMesh` as scalars, in GPU memory instead. Written
        // through `indirect_buffer`, so they arrive device-local and in
        // `ResourceState::IndirectArgument` by the transition a GPU-driven frame
        // makes — which is the state `ExecuteIndirect` requires and the one the
        // renderer's culling pass leaves behind.
        let extents: Vec<u8> = [1u32, 1, 1].iter().flat_map(|n| n.to_le_bytes()).collect();
        let args = indirect_buffer(&device, "mesh dispatch extents", &extents);

        let drawn = probe.frame(
            &device,
            "mesh_shader indirect",
            None,
            "meshMain",
            |encoder| {
                encoder.draw_mesh_tasks_indirect(&DrawIndirect {
                    args,
                    offset: 0,
                    draw_count: 1,
                    // A single structure is read at the offset and never strided
                    // over, so this is what a one-command caller passes;
                    // `crate::draw::plan_indirect` fills the signature's
                    // `ByteStride` in from the structure's own width.
                    stride: 0,
                });
            },
        );

        // The three texels the direct probe asserts on its mesh-only frame, in the
        // same order. Surviving the dispatch is not the claim — a device that
        // executed nothing survives too — so the centre is what makes a pass mean
        // the indirect path drew.
        assert_eq!(texel(&drawn, 0, 0), CLEAR_TEXEL, "top-left corner");
        assert_eq!(
            texel(
                &drawn,
                SQUARE.width as usize - 1,
                SQUARE.height as usize - 1
            ),
            CLEAR_TEXEL,
            "bottom-right corner"
        );
        assert_ne!(
            texel(&drawn, 32, 32),
            CLEAR_TEXEL,
            "the indirect mesh dispatch emitted nothing over the centre of the target"
        );

        device.destroy_buffer(args);
        probe.destroy(&device);
    }

    /// **The combination the renderer actually uses: an indirect dispatch
    /// *through the amplification stage*.**
    ///
    /// The sibling above eliminated one suspect and this narrows what is left.
    /// The four combinations of {direct, indirect} x {mesh only, amplified} are
    /// now all driven here, and until this test three of them were:
    /// `the_mesh_shader_pipeline_draws_through_both_entry_points` covers direct
    /// with and without `taskMain`, the sibling above covers indirect without
    /// it, and `crcbl-render`'s frame is the only thing that has ever run
    /// indirect **with** it — which is the frame that removes the device.
    ///
    /// So this is the smallest step from a frame WARP survives to the frame it
    /// does not. A failure here puts the fault in
    /// `ExecuteIndirect(DISPATCH_MESH)` reaching an amplification stage, which
    /// is one call and one stage rather than a renderer; a pass eliminates the
    /// pipeline shape entirely and leaves scale, the cluster shader's own size,
    /// and the bindless heap as what differs.
    ///
    /// One workgroup and the same three texels as every mesh frame here, for the
    /// reason its sibling gives: surviving is not the claim.
    ///
    /// # What only CI can settle
    ///
    /// All of it — this crate compiles on Windows alone and the development box
    /// is Linux.
    #[test]
    #[ignore = "needs a real D3D12 device; run tests/run-dx12-e2e.sh"]
    fn an_indirect_dispatch_through_the_amplification_stage_draws_the_same_triangle() {
        let (_instance, device) = open_device();
        let probe = MeshProbe::new(&device, &[]);

        // The same one `D3D12_DISPATCH_MESH_ARGUMENTS` the sibling writes.
        // `taskMain` is `[numthreads(1, 1, 1)]` and dispatches one mesh group,
        // so `1, 1, 1` is one amplification group exactly as it is one mesh
        // group there.
        let extents: Vec<u8> = [1u32, 1, 1].iter().flat_map(|n| n.to_le_bytes()).collect();
        let args = indirect_buffer(&device, "amplified mesh dispatch extents", &extents);

        let drawn = probe.frame(
            &device,
            "mesh_shader amplified indirect",
            Some(probe.entry("taskMain")),
            "amplifiedMeshMain",
            |encoder| {
                encoder.draw_mesh_tasks_indirect(&DrawIndirect {
                    args,
                    offset: 0,
                    draw_count: 1,
                    stride: 0,
                });
            },
        );

        assert_eq!(texel(&drawn, 0, 0), CLEAR_TEXEL, "top-left corner");
        assert_eq!(
            texel(
                &drawn,
                SQUARE.width as usize - 1,
                SQUARE.height as usize - 1
            ),
            CLEAR_TEXEL,
            "bottom-right corner"
        );
        assert_ne!(
            texel(&drawn, 32, 32),
            CLEAR_TEXEL,
            "the amplified indirect dispatch emitted nothing over the centre of the target"
        );

        device.destroy_buffer(args);
        probe.destroy(&device);
    }

    /// **Scale, which is what is left once the pipeline shape is eliminated.**
    ///
    /// The three siblings above drive every combination of {direct, indirect} x
    /// {mesh only, amplified} and WARP survives all four — but each dispatches
    /// **one** group, and `crcbl-render`'s frame dispatches one per (cluster,
    /// surviving instance). So "the shape is fine and the size is not" is the
    /// next thing worth ruling in or out, and it is one number.
    ///
    /// [`MANY_GROUPS`] amplification groups, each running the same `taskMain`
    /// that emits one mesh group, through the same indirect path. Every group
    /// draws the same triangle over the same pixels, so the assertions do not
    /// change: what changes is how much work one `ExecuteIndirect` asks for.
    ///
    /// A failure here says the renderer's mesh frame dies of *size* — which
    /// would make the fix a dispatch bound rather than a call to avoid. A pass
    /// leaves `mesh_cluster.slang` itself: its groupshared use, its payload, and
    /// the bindless heap the toy shader here does not touch.
    ///
    /// # What only CI can settle
    ///
    /// All of it — this crate compiles on Windows alone and the development box
    /// is Linux.
    #[test]
    #[ignore = "needs a real D3D12 device; run tests/run-dx12-e2e.sh"]
    fn many_indirect_amplification_groups_do_not_remove_the_device() {
        let (_instance, device) = open_device();
        let probe = MeshProbe::new(&device, &[]);

        let extents: Vec<u8> = [MANY_GROUPS, 1, 1]
            .iter()
            .flat_map(|n| n.to_le_bytes())
            .collect();
        let args = indirect_buffer(&device, "many amplified groups", &extents);

        let drawn = probe.frame(
            &device,
            "mesh_shader amplified indirect at scale",
            Some(probe.entry("taskMain")),
            "amplifiedMeshMain",
            |encoder| {
                encoder.draw_mesh_tasks_indirect(&DrawIndirect {
                    args,
                    offset: 0,
                    draw_count: 1,
                    stride: 0,
                });
            },
        );

        // Identical to the one-group case: every group draws the same triangle,
        // so more of them changes the work and not the picture.
        assert_eq!(texel(&drawn, 0, 0), CLEAR_TEXEL, "top-left corner");
        assert_eq!(
            texel(
                &drawn,
                SQUARE.width as usize - 1,
                SQUARE.height as usize - 1
            ),
            CLEAR_TEXEL,
            "bottom-right corner"
        );
        assert_ne!(
            texel(&drawn, 32, 32),
            CLEAR_TEXEL,
            "{MANY_GROUPS} amplification groups emitted nothing over the centre"
        );

        device.destroy_buffer(args);
        probe.destroy(&device);
    }

    /// **A zero-group `DispatchMesh`: the first difference in shader *content*
    /// between the toy amplification stage WARP survives and the one
    /// `crcbl-render` runs.**
    ///
    /// The four probes above drive every combination of {direct, indirect} x
    /// {mesh only, amplified}, at one group and at [`MANY_GROUPS`], and WARP
    /// survives all of them — so pipeline shape and dispatch scale are both
    /// eliminated and what is left is what the shaders *do*. This is the first
    /// feature taken off that list, and it is one line: `mesh_shader.slang`'s
    /// `taskMain` ends with `DispatchMesh(1, 1, 1, payload)`, while
    /// `mesh_cluster.slang`'s ends with `DispatchMesh(keep, 1, 1, payload)`
    /// where `keep` is `0` on a culled cluster. Every frame the renderer culls
    /// anything, it asks D3D12 for a zero-group `DispatchMesh` — and no probe
    /// here had ever asked for one on its own.
    ///
    /// # What differs from its sibling
    ///
    /// The amplification entry point, and nothing else. The extents, the
    /// `ExecuteIndirect`, the layout, the bind group, the target and the mesh
    /// and fragment DXIL containers are
    /// [`many_indirect_amplification_groups_do_not_remove_the_device`]'s —
    /// `zero_dispatch_probe.slang`'s `culledTaskMain` dispatches one mesh group
    /// for odd `SV_GroupID` and **zero** for even, so with [`MANY_GROUPS`]
    /// groups both arms run in the one dispatch and half of them ask for
    /// nothing.
    ///
    /// It is a second module because Slang 2026.14's Metal backend miscompiles
    /// any module holding two amplification entry points — the shader source
    /// says what it emits — and D3D12 takes one DXIL container per stage, so
    /// the mesh and fragment stages here are the same *bytes* the passing
    /// sibling runs.
    ///
    /// # What each outcome says
    ///
    /// * **It draws.** A zero-group `DispatchMesh` is not what removes the
    ///   device, and the bisect moves to the next difference in content: the
    ///   cluster shader's groupshared use, its payload, and the bindless heap
    ///   this shader does not touch.
    /// * **The device goes away.** The removal has a four-line repro with no
    ///   renderer in it, and the fix is a shape the amplification stage must
    ///   avoid rather than a call.
    ///
    /// The three texels are its siblings', for the reason they give: surviving
    /// is not the claim, so the centre is what makes a pass mean the groups that
    /// *did* dispatch drew.
    ///
    /// # What only CI can settle
    ///
    /// All of it — this crate compiles on Windows alone and the development box
    /// is Linux.
    #[test]
    #[ignore = "needs a real D3D12 device; run tests/run-dx12-e2e.sh"]
    fn a_zero_group_dispatch_mesh_does_not_remove_the_device() {
        let (_instance, device) = open_device();
        let probe = MeshProbe::new(&device, &[]);
        let culled = device
            .create_shader_module(&ShaderModuleDesc {
                label: Some("zero_dispatch_probe.slang"),
                dxil: &crcbl_shaders::ZERO_DISPATCH_PROBE.dxil_containers(),
                ..ShaderModuleDesc::default()
            })
            .unwrap_or_else(|error| panic!("stage=create_shader_module: {error:?}"));

        let extents: Vec<u8> = [MANY_GROUPS, 1, 1]
            .iter()
            .flat_map(|n| n.to_le_bytes())
            .collect();
        let args = indirect_buffer(&device, "zero-count amplification groups", &extents);

        let drawn = probe.frame(
            &device,
            "mesh_shader zero-count dispatch",
            Some(ShaderEntry {
                module: culled,
                entry_point: "culledTaskMain",
            }),
            "amplifiedMeshMain",
            |encoder| {
                encoder.draw_mesh_tasks_indirect(&DrawIndirect {
                    args,
                    offset: 0,
                    draw_count: 1,
                    stride: 0,
                });
            },
        );

        // The groups that dispatch draw the triangle every other mesh frame here
        // draws, over the same pixels, so the picture is the sibling's.
        assert_eq!(texel(&drawn, 0, 0), CLEAR_TEXEL, "top-left corner");
        assert_eq!(
            texel(
                &drawn,
                SQUARE.width as usize - 1,
                SQUARE.height as usize - 1
            ),
            CLEAR_TEXEL,
            "bottom-right corner"
        );
        assert_ne!(
            texel(&drawn, 32, 32),
            CLEAR_TEXEL,
            "the amplification groups that dispatched a mesh group emitted nothing over the centre"
        );

        device.destroy_shader_module(culled);
        device.destroy_buffer(args);
        probe.destroy(&device);
    }

    /// **Two storage-buffer writes from the amplification stage — an atomic
    /// and a plain indexed store — read back and asserted, rather than merely
    /// survived.**
    ///
    /// The probes above eliminate pipeline shape, dispatch scale and a
    /// zero-group `DispatchMesh`: WARP survives every one of them. What is left
    /// is what the amplification stage *does*, and the next difference in
    /// content is that `mesh_cluster.slang`'s `taskMain` writes to two storage
    /// buffers before it dispatches while every shader those probes run writes
    /// nothing at all from that stage. A UAV write from an amplification stage
    /// — an atomic one above all — is a far less travelled path than the
    /// dispatch it precedes.
    ///
    /// # What differs from [`a_zero_group_dispatch_mesh_does_not_remove_the_device`]
    ///
    /// The amplification stage writes, and the set it is bound through carries
    /// two writable buffers for it to write to.
    /// `task_write_probe.slang`'s `writingTaskMain` adds one to a single
    /// contended counter on the odd groups and stores its own index plus one
    /// into its own slot on the even ones — one atomic and one plain store,
    /// each under its own branch, which is the shape `mesh_cluster.slang` has.
    /// Its `DispatchMesh` is `mesh_shader.slang`'s unbranched one, because the
    /// zero-group form is the probe above and the bisect takes one feature at a
    /// time.
    ///
    /// It is a module of its own for the reason its source gives: Slang
    /// 2026.14's Metal backend miscompiles any module holding two amplification
    /// entry points. The mesh and fragment stages are still the passing
    /// siblings' containers, byte for byte.
    ///
    /// # It asserts the writes, not only the picture
    ///
    /// That is what this says and its siblings cannot. Both buffers are primed
    /// before the frame — the counter at zero, because an atomic add
    /// accumulates onto whatever is there, and every slot at [`PROBE_SENTINEL`]
    /// — and both are copied back after it. What they must hold follows from
    /// the dispatch extent alone: half of [`MANY_GROUPS`] groups are odd, so
    /// the counter holds exactly that many, and each even slot holds its own
    /// index plus one while every odd slot still holds the sentinel. A device
    /// that ran the stage with its writes dropped, or that sent them somewhere
    /// else, therefore fails here rather than drawing the sibling's triangle
    /// and passing.
    ///
    /// # What each outcome says
    ///
    /// * **It draws and both buffers read back.** Storage writes from an
    ///   amplification stage are not what removes the device, and the bisect
    ///   moves to what is left of `mesh_cluster.slang`: its payload, its
    ///   `groupshared` use, and the bindless heap none of these probes touch.
    /// * **The device goes away.** The removal has a repro with no renderer in
    ///   it, and the fix is a write the amplification stage must not perform —
    ///   the cull statistics would have to be counted by a later pass.
    /// * **It draws and a buffer disagrees.** The stage ran and its writes did
    ///   not land where the root signature says they should, which is a binding
    ///   defect rather than a mesh-shading one — and one nothing else here
    ///   would have caught, because the picture is the same either way.
    ///
    /// The three texels are its siblings', for the reason they give: surviving
    /// is not the claim.
    ///
    /// # What only CI can settle
    ///
    /// All of it — this crate compiles on Windows alone and the development box
    /// is Linux.
    #[test]
    #[ignore = "needs a real D3D12 device; run tests/run-dx12-e2e.sh"]
    fn storage_writes_from_the_amplification_stage_do_not_remove_the_device() {
        let (_instance, device) = open_device();

        // One word for the atomic and one per group for the store, primed from
        // a single upload buffer: the counter at zero because an add
        // accumulates, every slot at the sentinel because a slot the shader
        // skipped has to be distinguishable from one it wrote.
        const COUNTER_BYTES: u64 = size_of::<u32>() as u64;
        let slot_bytes = u64::from(MANY_GROUPS) * size_of::<u32>() as u64;
        let mut primer = 0u32.to_le_bytes().to_vec();
        primer.extend(
            core::iter::repeat_n(PROBE_SENTINEL, MANY_GROUPS as usize).flat_map(u32::to_le_bytes),
        );
        let staged = device
            .create_buffer(&BufferDesc {
                label: Some("amplification writes primer"),
                size: primer.len() as u64,
                usage: BufferUsage::TRANSFER_SRC,
                memory: MemoryLocation::HostUpload,
            })
            .expect("an upload buffer");
        device
            .write_buffer(staged, 0, &primer)
            .expect("an upload-heap buffer is host-visible");

        // `DeviceLocal` because D3D12 admits an unordered access view on no
        // other heap — `crcbl_dx12::buffer::check_unordered_access` refuses the
        // rest by name — so a written buffer is reached by a copy at both ends.
        let written = |label, size| {
            device
                .create_buffer(&BufferDesc {
                    label: Some(label),
                    size,
                    usage: BufferUsage::STORAGE
                        | BufferUsage::TRANSFER_DST
                        | BufferUsage::TRANSFER_SRC,
                    memory: MemoryLocation::DeviceLocal,
                })
                .unwrap_or_else(|error| panic!("stage=create_buffer({label}): {error:?}"))
        };
        let counter = written("amplification write counter", COUNTER_BYTES);
        let slots = written("amplification write slots", slot_bytes);
        let counter_staging = readback_buffer(&device, COUNTER_BYTES as usize);
        let slot_staging = readback_buffer(&device, slot_bytes as usize);

        run(&device, |encoder| {
            encoder.pipeline_barrier(&Barriers {
                buffers: &[
                    buffer_barrier(
                        counter,
                        ResourceState::Undefined,
                        ResourceState::TransferDst,
                    ),
                    buffer_barrier(slots, ResourceState::Undefined, ResourceState::TransferDst),
                ],
                ..Barriers::default()
            });
            encoder.copy_buffer_to_buffer(&BufferCopy {
                src: staged,
                src_offset: 0,
                dst: counter,
                dst_offset: 0,
                size: COUNTER_BYTES,
            });
            encoder.copy_buffer_to_buffer(&BufferCopy {
                src: staged,
                src_offset: COUNTER_BYTES,
                dst: slots,
                dst_offset: 0,
                size: slot_bytes,
            });
            // `ShaderReadWrite`, not `ShaderWrite`: a barrier names the access
            // the *descriptor* permits rather than the one the source performs,
            // and an unordered-access view is both.
            encoder.pipeline_barrier(&Barriers {
                buffers: &[
                    buffer_barrier(
                        counter,
                        ResourceState::TransferDst,
                        ResourceState::ShaderReadWrite,
                    ),
                    buffer_barrier(
                        slots,
                        ResourceState::TransferDst,
                        ResourceState::ShaderReadWrite,
                    ),
                ],
                ..Barriers::default()
            });
        });

        let probe = MeshProbe::new(&device, &[counter, slots]);
        let writing = device
            .create_shader_module(&ShaderModuleDesc {
                label: Some("task_write_probe.slang"),
                dxil: &crcbl_shaders::TASK_WRITE_PROBE.dxil_containers(),
                ..ShaderModuleDesc::default()
            })
            .unwrap_or_else(|error| panic!("stage=create_shader_module: {error:?}"));

        let extents: Vec<u8> = [MANY_GROUPS, 1, 1]
            .iter()
            .flat_map(|n| n.to_le_bytes())
            .collect();
        let args = indirect_buffer(&device, "writing amplification groups", &extents);

        let drawn = probe.frame(
            &device,
            "mesh_shader amplification writes",
            Some(ShaderEntry {
                module: writing,
                entry_point: "writingTaskMain",
            }),
            "amplifiedMeshMain",
            |encoder| {
                encoder.draw_mesh_tasks_indirect(&DrawIndirect {
                    args,
                    offset: 0,
                    draw_count: 1,
                    stride: 0,
                });
            },
        );

        // Every group dispatches one mesh group, so the picture is the one
        // `many_indirect_amplification_groups_do_not_remove_the_device` draws.
        assert_eq!(texel(&drawn, 0, 0), CLEAR_TEXEL, "top-left corner");
        assert_eq!(
            texel(
                &drawn,
                SQUARE.width as usize - 1,
                SQUARE.height as usize - 1
            ),
            CLEAR_TEXEL,
            "bottom-right corner"
        );
        assert_ne!(
            texel(&drawn, 32, 32),
            CLEAR_TEXEL,
            "the amplification groups that wrote a storage buffer emitted nothing over the centre"
        );

        run(&device, |encoder| {
            encoder.pipeline_barrier(&Barriers {
                buffers: &[
                    buffer_barrier(
                        counter,
                        ResourceState::ShaderReadWrite,
                        ResourceState::TransferSrc,
                    ),
                    buffer_barrier(
                        slots,
                        ResourceState::ShaderReadWrite,
                        ResourceState::TransferSrc,
                    ),
                ],
                ..Barriers::default()
            });
            encoder.copy_buffer_to_buffer(&BufferCopy {
                src: counter,
                src_offset: 0,
                dst: counter_staging,
                dst_offset: 0,
                size: COUNTER_BYTES,
            });
            encoder.copy_buffer_to_buffer(&BufferCopy {
                src: slots,
                src_offset: 0,
                dst: slot_staging,
                dst_offset: 0,
                size: slot_bytes,
            });
        });

        let words = |buffer, bytes: u64| -> Vec<u32> {
            let request = device
                .request_readback(&ReadbackDesc {
                    label: Some("amplification writes readback"),
                    buffer,
                    offset: 0,
                    size: bytes,
                    after: None,
                })
                .expect("a readback of a HostReadback buffer");
            let read = drain(&device, request, bytes as usize);
            device.destroy_readback(request);
            read.chunks_exact(size_of::<u32>())
                .map(|word| u32::from_le_bytes([word[0], word[1], word[2], word[3]]))
                .collect()
        };

        // The odd groups added one each and nothing else touched the word.
        assert_eq!(
            words(counter_staging, COUNTER_BYTES),
            vec![MANY_GROUPS / 2],
            "the atomic add from the amplification stage did not land the expected total"
        );

        let stored = words(slot_staging, slot_bytes);
        assert_eq!(
            stored.len(),
            MANY_GROUPS as usize,
            "the readback is not the whole slot buffer"
        );
        let expected: Vec<u32> = (0..MANY_GROUPS)
            .map(|group| {
                if group.is_multiple_of(2) {
                    group + 1
                } else {
                    PROBE_SENTINEL
                }
            })
            .collect();
        if let Some((index, (got, want))) = stored
            .iter()
            .zip(&expected)
            .enumerate()
            .find(|(_, (got, want))| got != want)
        {
            panic!(
                "slot {index} holds {got} ({got:#x}) and the amplification stage should have left \
                 {want} ({want:#x}) there"
            );
        }

        device.destroy_shader_module(writing);
        device.destroy_buffer(args);
        probe.destroy(&device);
        device.destroy_buffer(slot_staging);
        device.destroy_buffer(counter_staging);
        device.destroy_buffer(slots);
        device.destroy_buffer(counter);
        device.destroy_buffer(staged);
    }

    /// **The depth-only pipeline shape, driven by the toy shader every passing
    /// mesh probe above already runs — the discriminator for what removes the
    /// WARP device.**
    ///
    /// [`the_cluster_shaders_dag_descent_draws_the_cut_it_chose`] below runs
    /// `mesh_cluster.slang`'s own containers and **reproduces the removal**, and
    /// that is the first repro in this investigation with no renderer in it. But
    /// it changes two things at once, and only one of them is the shader.
    ///
    /// `mesh_cluster.slang` has no fragment entry point, so that probe takes
    /// `ForwardRenderer::depth_pipeline`'s shape: `fragment: None`, no colour
    /// targets, and the depth attachment as the observable. **`crcbl-dx12` had
    /// never built a mesh pipeline without a pixel shader, and had never copied
    /// a `D32Float` image back.** That machinery is exactly as untried as the
    /// cluster shader is, so the removal is one or the other and nothing so far
    /// separates them.
    ///
    /// This drives the untried half alone. Same [`DepthProbe`], same
    /// `fragment: None` pipeline, same `ExecuteIndirect` of a `DISPATCH_MESH`
    /// through an amplification stage, same three texels — and
    /// `mesh_shader.slang`'s `taskMain` and `amplifiedMeshMain`, the stages
    /// [`an_indirect_dispatch_through_the_amplification_stage_draws_the_same_triangle`]
    /// already runs to a triangle on this device. The only difference between
    /// the two probes is the shader stages and the bindings that reach them.
    ///
    /// # What each outcome says
    ///
    /// * **The toy dies too.** The depth-only mesh pipeline, or the `D32Float`
    ///   readback, is the defect. `mesh_cluster.slang` is then **unjudged** —
    ///   the probe below proves nothing about it — and the bisect moves into
    ///   `crcbl-dx12`'s own pipeline construction: the null pixel shader, the
    ///   `DSVFormat` on a mesh state stream, and `plan_copy`'s depth aspect.
    /// * **The toy draws.** The depth-only shape is sound, and
    ///   `mesh_cluster.slang` is what removes the device — the first *positive*
    ///   identification in an investigation whose every result so far has been a
    ///   negative one, and the point at which the bisect moves inside one
    ///   `.slang` file.
    ///
    /// # What it asserts, and why the centre texel
    ///
    /// The mesh stage writes `SV_Position` and nothing else reaches the
    /// attachment, so depth is what rasterising the triangle produces. The
    /// triangle is apex-down over the centre of the target with both corners
    /// outside it — the property
    /// [`a_mesh_pipeline_draws_through_d3d12_and_its_amplification_stage_is_visible`]
    /// already samples at the same texel on the colour path — so
    /// [`assert_depth_triangle`] reads the centre and both corners.
    ///
    /// The expected depth is **derived from `crcbl_shaders::mesh_shader`'s own
    /// positions rather than written here**, and two assertions below keep that
    /// derivation honest: all three vertices must carry one clip-space depth, or
    /// the centre texel would hold an interpolation instead of a constant, and
    /// that depth must differ from [`crcbl_hal::depth::CLEAR`], or the centre
    /// assertion would pass on a device that rasterised nothing.
    ///
    /// # What only CI can settle
    ///
    /// All of it. This crate compiles on Windows alone and the development box
    /// is Linux.
    #[test]
    #[ignore = "known-red: a depth-only mesh pipeline removes the WARP device (docs/backlog.md)"]
    fn a_depth_only_mesh_pipeline_draws_the_toy_triangle_on_this_device() {
        // What the rasteriser must leave at the centre, taken from the toy
        // shader's own vertices rather than written here — clip-space depth over
        // `w`, which is the NDC depth the attachment holds. Both assertions are
        // about this test being able to fail, so they run before the device
        // does any work.
        let emitted: Vec<f32> = crcbl_shaders::mesh_shader::POSITIONS
            .iter()
            .map(|position| position[2] / position[3])
            .collect();
        assert!(
            emitted.iter().all(|depth| *depth == emitted[0]),
            "the toy triangle is no longer flat in Z ({emitted:?}), so the centre texel holds an \
             interpolation and the equality below is the wrong assertion"
        );
        assert_ne!(
            emitted[0],
            crcbl_hal::depth::CLEAR,
            "the toy triangle must emit a depth the clear could not have left, or the centre \
             texel below is satisfied by a frame that rasterised nothing"
        );

        let (_instance, device) = open_device();
        let probe = MeshProbe::new(&device, &[]);

        // The one `D3D12_DISPATCH_MESH_ARGUMENTS` every amplified probe here
        // writes: `taskMain` is `[numthreads(1, 1, 1)]` and dispatches one mesh
        // group. Indirect rather than direct because the probe this
        // discriminates for is indirect, and the dispatch has to be the thing
        // that does *not* vary between them.
        let args = indirect_buffer(
            &device,
            "depth-only mesh dispatch extents",
            &pack_words(&[1, 1, 1]),
        );

        let depths = probe.depth_frame(
            &device,
            "mesh_shader depth",
            Some(probe.entry("taskMain")),
            "amplifiedMeshMain",
            |encoder| {
                encoder.draw_mesh_tasks_indirect(&DrawIndirect {
                    args,
                    offset: 0,
                    draw_count: 1,
                    stride: 0,
                });
            },
        );

        assert_depth_triangle(&depths, emitted[0], "the toy shader with no fragment stage");

        device.destroy_buffer(args);
        probe.destroy(&device);
    }

    /// Clusters the probe below gives its synthetic mesh, and the x extent of
    /// its dispatch.
    ///
    /// **Five, because five is the smallest number that puts every arm of both
    /// stages' decisions on one frame**: one cluster the descent selects and the
    /// cull keeps, one it selects and the frustum rejects, one it selects and
    /// the normal cone rejects, one it drops because the group that produced it
    /// expanded, and one it drops because the group that contains it did not.
    /// A smaller mesh would leave at least one of `taskMain`'s branches taken by
    /// nothing, which is the failure mode a "something was drawn" assertion
    /// cannot see.
    const PROBE_CLUSTERS: u32 = 5;

    /// Groups the probe's synthetic DAG holds for one instance, which is
    /// `ClusterDrawConstants::group_stride`.
    ///
    /// Three: group 0 is unexpanded and unused, group 1 is **expanded** — which
    /// is what drops the cluster naming it as its producer — and group 2 is
    /// unexpanded, which is what drops the cluster naming it as its container.
    const PROBE_DAG_GROUPS: u32 = 3;

    /// The clip-space depth every vertex the probe emits carries.
    ///
    /// The engine is reversed-Z — [`crcbl_hal::depth::CLEAR`] is `0.0` and
    /// [`crcbl_hal::DepthStencilState::default`] compares `GREATER` — so any
    /// value above the clear passes, and one strictly between the clear and the
    /// near plane is a value neither end of the range could have produced by
    /// accident. `w` is `1.0` at every vertex, so this is the NDC depth too and
    /// the assertion below is on the number written here.
    const PROBE_DEPTH: f32 = 0.5;

    /// **`mesh_cluster.slang`'s own amplification and mesh stages, descending a
    /// real cluster DAG over synthetic data and rasterising the one cluster the
    /// cut keeps.**
    ///
    /// The five probes above grew a toy shader one feature at a time and every
    /// one passed, which left this bisect three suspects: the real shader's
    /// twenty-two bindings against their one or three, the cluster-DAG descent —
    /// the only real control flow in either stage, and a task-stage container of
    /// 10,992 bytes against the toy's 2,648 — and the draw-argument pass that
    /// writes this path's indirect extents. This runs the **committed
    /// containers** `crcbl-render` builds its mesh pipeline from, with no
    /// renderer in the test, so a pass leaves only the third.
    ///
    /// # The layout is `ForwardRenderer::mesh_layout` minus its three fragment-only rows
    ///
    /// `crcbl-dx12` sits below `crcbl-render` and cannot call that function, so
    /// the entries below are transcribed from it: the same binding numbers, the
    /// same kinds, the same read-only-ness, in the same ascending order. What is
    /// **not** transcribed is bindings 15, 16 and 22 — the shadow atlas, its
    /// comparison sampler and the occlusion channel — and dropping them is
    /// load-bearing rather than tidy.
    ///
    /// **A D3D12 register is not a binding number.** Slang's HLSL output
    /// annotates each resource with a `register(…)` it numbers per class, from
    /// zero, in the declaration order of **that source file**; `crate::binding`'s
    /// `ranges` reproduces that by counting the entries of the *layout*. The two
    /// agree only when the layout's binding set is exactly the source's. It is
    /// for `mesh.slang` on the raster path — that layout is bindings 0–8, 15, 16
    /// and 20–23, which is precisely what that file declares. It is **not** for
    /// either stage of the mesh path: `mesh_cluster.slang` declares no binding
    /// 15, 16 or 22, so the layout carrying them numbers its `cluster_select`
    /// `t11` where its own container says `t10`, and its `tables` `t17` where the
    /// container says `t15`. Dropping the three rows the geometry stages do not
    /// declare is what makes every register this probe binds the register the
    /// containers ask for; see the module-level note this test's report carries.
    ///
    /// # There is no fragment stage, and that is the same fact from the other side
    ///
    /// `mesh_cluster.slang` has no fragment entry point: the renderer pairs it
    /// with `mesh.slang`'s `fragmentMain`, which numbers **its** registers from
    /// **its** declarations and puts the shadow atlas on `t6` — the register
    /// `mesh_cluster.slang` gives `clusters`. One root signature cannot satisfy
    /// both, so a probe that bound a fragment stage would be testing the
    /// disagreement rather than the shader.
    ///
    /// What it uses instead is the shape `ForwardRenderer::depth_pipeline`
    /// already builds for the shadow cascades: the same task and mesh stages,
    /// `fragment: None`, and no colour target. The observable is the **depth
    /// attachment**, which is what the mesh stage's `SV_Position` writes, so
    /// "the device survived and rasterised nothing" still fails here.
    ///
    /// # What the data makes the shader do
    ///
    /// One instance at the identity transform, one mesh, and [`PROBE_CLUSTERS`]
    /// clusters that all share one three-vertex, one-triangle run and differ
    /// only in their bounds and their [`crcbl_shaders::cluster_select::ClusterSelect`]
    /// record. `view_proj` is the identity too, so a vertex's position is already
    /// clip space and the triangle covers the centre of the target and neither
    /// corner.
    ///
    /// * **Cluster 0** has no producer and no container, so the descent selects
    ///   it; its cone is omnidirectional and its sphere is inside the frustum,
    ///   so the cull keeps it. It is the one cluster that reaches the mesh stage.
    /// * **Cluster 1** is selected and sits ten units along `+x`, outside the
    ///   probe's frustum box — `CLUSTER_REJECTED_BY_FRUSTUM`.
    /// * **Cluster 2** is selected, inside the frustum, and faces directly away
    ///   from the camera at `(0, 0, 5)` with a `0.9` cone cutoff —
    ///   `CLUSTER_REJECTED_BY_CONE`.
    /// * **Cluster 3** names group 1 as its producer, and `group_state[1]` is
    ///   `1`, so its producer expanded and the descent drops it.
    /// * **Cluster 4** names group 2 as its container, and `group_state[2]` is
    ///   `0`, so its container did not expand and the descent drops it.
    ///
    /// # What it asserts, and why each assertion can fail
    ///
    /// * **The centre texel of the depth attachment holds [`PROBE_DEPTH`] and
    ///   both corners hold the clear.** A device that survived the dispatch and
    ///   rasterised nothing leaves the clear in all three, which is the failure
    ///   the sibling probes' centre-texel assertion catches and the reason there
    ///   is deliberately no tolerance, skip or catch anywhere below.
    /// * **`cull_stats` holds exactly one survivor, one frustum rejection and
    ///   one cone rejection**, and the two words this shader does not own —
    ///   element 0, which is `cull.slang`'s surviving instances, and element 2,
    ///   which is `light_cluster.slang`'s dropped assignments — still hold
    ///   [`PROBE_SENTINEL`]. The three cluster words sum to **three**, which is
    ///   the size of the cut and not the five clusters dispatched: that is the
    ///   arithmetic `mesh_cluster.slang`'s own comment states and `apps/quarry`'s
    ///   device suite asserts a version of, and it is what shows the descent
    ///   descended rather than every group taking one branch.
    /// * **`cluster_selection` holds `[1, 1, 1, 0, 0]`**, primed at
    ///   [`PROBE_SENTINEL`] beforehand, so a stage that wrote nothing and a stage
    ///   that wrote zeros are different failures.
    ///
    /// # This test is red today, and that is the measurement
    ///
    /// **It ran, and WARP removed the device** — `DXGI_ERROR_DEVICE_REMOVED`,
    /// zero debug-layer errors, DRED reporting `0 command list(s) with recorded
    /// work`: `crcbl-render`'s exact signature, out of a test with no renderer
    /// in it. That is what `docs/backlog.md`'s "DECISION NEEDED — dx12 mesh
    /// shading: WARP claims it and dies, hardware works" entry records, and this
    /// is the repro it names.
    ///
    /// Its `#[ignore]` reason therefore names the **defect**, not a hardware
    /// requirement — but read it as documentation and nothing more, because
    /// **the reason string does not keep this test out of any run.** Every
    /// device test in this crate is `#[ignore]`d so that
    /// `tests/run-dx12-e2e.sh` can name them, and that harness selects them with
    /// `--run-ignored only`, which takes every ignored test whatever its reason
    /// says. Keeping a known-red one out of that run needs a filter — a
    /// `-E 'not test(…)'` the harness or its caller passes — and there is
    /// deliberately no such mechanism invented here.
    ///
    /// Run it by itself with
    ///
    /// ```text
    /// cargo nextest run -p crcbl-dx12 --run-ignored only \
    ///     -E 'test(the_cluster_shaders_dag_descent_draws_the_cut_it_chose)'
    /// ```
    ///
    /// **What will make it pass is a fix to the removal, and nothing else.** Its
    /// assertions are not weakened, skipped or tolerance-widened for the red
    /// state, because a repro that passes on a removed device is not a repro.
    ///
    /// # What each outcome says
    ///
    /// * **It draws and both buffers read back.** The real shader, its real
    ///   bindings and its DAG descent are not what removes the device, and what
    ///   is left of this bisect is the draw-argument pass that feeds the mesh
    ///   path its indirect extents.
    /// * **The device goes away**, which is what it does today. The removal has
    ///   a repro with no renderer in it and the bisect continues inside one
    ///   `.slang` file — **provided**
    ///   [`a_depth_only_mesh_pipeline_draws_the_toy_triangle_on_this_device`]
    ///   draws. That test holds this shape and swaps in the toy shader every
    ///   passing mesh probe already runs; until it is green, a removal here
    ///   could equally be the depth-only pipeline or the `D32Float` readback
    ///   rather than the shader, and this result says nothing about
    ///   `mesh_cluster.slang`.
    /// * **It refuses at `create_mesh_pipeline`.** The root signature and the
    ///   containers disagree about a register after all, which is a layout
    ///   defect the message names rather than a mesh-shading one.
    ///
    /// # What is simplified, stated plainly
    ///
    /// The data is synthetic and the DAG is three groups deep in name only —
    /// nothing here builds a real hierarchy, and the frame is one bucket, one
    /// instance and one triangle. There is **no fragment stage**, so nothing
    /// below says anything about `mesh.slang`'s shading, its texture page or the
    /// registers that stage asks for. And this is not `crcbl-render`'s layout: it
    /// is that layout minus three rows, for the reason above, so a pass here does
    /// **not** say the renderer's own mesh pipeline would survive.
    ///
    /// # What only CI can settle
    ///
    /// All of it. This crate compiles on Windows alone and the development box
    /// is Linux.
    #[test]
    #[ignore = "known-red: mesh_cluster.slang removes the WARP device (docs/backlog.md)"]
    fn the_cluster_shaders_dag_descent_draws_the_cut_it_chose() {
        use crcbl_shaders::cluster_select::{CLUSTER_SELECT_STRIDE, ClusterSelect};
        use crcbl_shaders::cull::{
            CLUSTER_CONE_REJECT_WORD, CLUSTER_FRUSTUM_REJECT_WORD, CLUSTER_SURVIVOR_WORD,
            STATS_WORDS,
        };
        use crcbl_shaders::draw_gen::DrawIndexedArgs;
        use crcbl_shaders::level_select::LEVEL_GROUP_STRIDE;
        use crcbl_shaders::light::LIGHT_STRIDE;
        use crcbl_shaders::mesh::{
            FrameUniforms, GpuInstance, GpuMaterial, GpuMesh, SHADOW_CASCADES, SHADOW_LIGHT_TILES,
            VERTEX_STRIDE,
        };
        use crcbl_shaders::meshlet::{ClusterBounds, Meshlet, corner_words};
        use crcbl_shaders::probe::{PROBE_STRIDE, ProbeVolume};

        let (_instance, device) = open_device();

        // --- the synthetic frame ---

        const IDENTITY: [f32; 16] = [
            1.0, 0.0, 0.0, 0.0, //
            0.0, 1.0, 0.0, 0.0, //
            0.0, 0.0, 1.0, 0.0, //
            0.0, 0.0, 0.0, 1.0,
        ];
        /// Where the probe's camera is, which is what the normal-cone test
        /// measures against.
        const EYE: [f32; 4] = [0.0, 0.0, 5.0, 1.0];

        let frame = FrameUniforms {
            view_proj: IDENTITY,
            camera_position: EYE,
            // **`w` is zero on purpose.** It is the overlay lane, and both
            // overlays sit above it — so `emit_cluster` takes neither arm and
            // `tables`, whose only reader is the heatmap, is bound and not read.
            ambient: [0.0; 4],
            shadow_view_proj: [[0.0; 16]; SHADOW_CASCADES],
            cascade_far: [0.0; 4],
            shadow_params: [0.0; 4],
            cluster_grid: [0; 4],
            light_view_proj: [[0.0; 16]; SHADOW_LIGHT_TILES],
            probes: ProbeVolume {
                origin: [0.0; 3],
                inv_spacing: [0.0; 3],
                counts: [0; 3],
            },
            lod_params: [1.0, 1.0, 1.0, 0.0],
        };

        // One triangle over the centre of the target and neither corner, in
        // clip space: `view_proj` and the instance transform are both the
        // identity, so what is written here is what the rasteriser gets.
        let pack = |values: &[f32]| -> Vec<u8> {
            values
                .iter()
                .flat_map(|value| value.to_le_bytes())
                .collect()
        };
        let vertex = |x: f32, y: f32| -> Vec<u8> {
            pack(&[
                x,
                y,
                PROBE_DEPTH,
                1.0, // position
                0.0,
                0.0,
                1.0,
                0.0, // normal
                1.0,
                1.0,
                1.0,
                1.0, // colour
                0.0,
                0.0,
                0.0,
                0.0, // uv
            ])
        };
        let mut vertex_bytes = Vec::new();
        for (x, y) in [(-0.5, -0.5), (0.5, -0.5), (0.0, 0.5)] {
            vertex_bytes.extend(vertex(x, y));
        }
        assert_eq!(
            vertex_bytes.len(),
            3 * VERTEX_STRIDE,
            "the packed vertices are not three of the stride crcbl-shaders publishes"
        );

        // Every cluster shares the one three-vertex, one-triangle run; what
        // differs is the bounds each is culled by.
        let cluster =
            |center: [f32; 3], radius: f32, cone_axis: [f32; 3], cone_cutoff: f32| Meshlet {
                vertex_offset: 0,
                vertex_count: 3,
                triangle_offset: 0,
                triangle_count: 1,
                bounds: ClusterBounds {
                    center,
                    radius,
                    cone_axis,
                    cone_cutoff,
                },
            };
        let clusters = [
            // Kept: inside the box below, and a cone that rejects nothing.
            cluster(
                [0.0, 0.0, 0.0],
                1.0,
                ClusterBounds::OMNIDIRECTIONAL_AXIS,
                ClusterBounds::OMNIDIRECTIONAL_CUTOFF,
            ),
            // Ten units along `+x`, so the `-x` half-space rejects its whole
            // sphere.
            cluster(
                [10.0, 0.0, 0.0],
                0.5,
                ClusterBounds::OMNIDIRECTIONAL_AXIS,
                ClusterBounds::OMNIDIRECTIONAL_CUTOFF,
            ),
            // Inside the box and facing straight away from `EYE`: with a cutoff
            // of `0.9` the test's right-hand side is `0.436 * 5 + 0.1`, which
            // `dot(axis, to_center) = 5` clears.
            cluster([0.0, 0.0, 0.0], 0.1, [0.0, 0.0, -1.0], 0.9),
            // Both of these would survive the cull; the descent is what drops
            // them, which is why their bounds are cluster 0's.
            cluster(
                [0.0, 0.0, 0.0],
                1.0,
                ClusterBounds::OMNIDIRECTIONAL_AXIS,
                ClusterBounds::OMNIDIRECTIONAL_CUTOFF,
            ),
            cluster(
                [0.0, 0.0, 0.0],
                1.0,
                ClusterBounds::OMNIDIRECTIONAL_AXIS,
                ClusterBounds::OMNIDIRECTIONAL_CUTOFF,
            ),
        ];
        assert_eq!(
            clusters.len(),
            PROBE_CLUSTERS as usize,
            "the dispatch extent and the cluster array must be the same number"
        );

        // The descent's half: two flags and two group indices per cluster.
        let select = |flags: u32, producer_group: u32, container_group: u32| ClusterSelect {
            flags,
            vertex_base: 0,
            producer_group,
            container_group,
        };
        let selects = [
            select(0, 0, 0),
            select(0, 0, 0),
            select(0, 0, 0),
            select(ClusterSelect::HAS_PRODUCER, 1, 0),
            select(ClusterSelect::HAS_CONTAINER, 0, 2),
        ];
        // Group 1 expanded, so cluster 3's producer is expanded and it is not in
        // the cut; group 2 did not, so cluster 4's container never gave it up.
        let group_state: [u32; PROBE_DAG_GROUPS as usize] = [0, 1, 0];

        // The frustum: an axis-aligned box two units either side of the origin,
        // as six unnormalized half-spaces `dot(xyz, p) + w >= 0`.
        let cull_params = crcbl_shaders::cull::Params {
            planes: [
                [1.0, 0.0, 0.0, 2.0],
                [-1.0, 0.0, 0.0, 2.0],
                [0.0, 1.0, 0.0, 2.0],
                [0.0, -1.0, 0.0, 2.0],
                [0.0, 0.0, 1.0, 2.0],
                [0.0, 0.0, -1.0, 2.0],
            ],
            instance_count: 1,
            capacity: 1,
        };

        let draw_constants = crcbl_shaders::meshlet::ClusterDrawConstants {
            base: 0,
            cluster_base: 0,
            cluster_count: PROBE_CLUSTERS,
            bucket: 0,
            group_stride: PROBE_DAG_GROUPS,
            level_groups_at: 0,
        };

        // --- the resources ---

        // A read-only binding is an SRV or a CBV, and D3D12 admits both on the
        // upload heap — so everything the stages only read is written straight
        // through `write_buffer`, exactly as `MeshProbe` writes its geometry.
        let uploaded = |label: &'static str, usage: BufferUsage, bytes: &[u8]| -> BufferHandle {
            let handle = device
                .create_buffer(&BufferDesc {
                    label: Some(label),
                    size: bytes.len() as u64,
                    usage,
                    memory: MemoryLocation::HostUpload,
                })
                .unwrap_or_else(|error| panic!("stage=create_buffer({label}): {error:?}"));
            device
                .write_buffer(handle, 0, bytes)
                .unwrap_or_else(|error| panic!("stage=write_buffer({label}): {error:?}"));
            handle
        };
        let read_storage = |label: &'static str, bytes: &[u8]| -> BufferHandle {
            uploaded(label, BufferUsage::STORAGE, bytes)
        };
        let uniform = |label: &'static str, bytes: &[u8]| -> BufferHandle {
            uploaded(label, BufferUsage::UNIFORM, bytes)
        };

        let frame_buffer = uniform("mesh_cluster frame", &frame.to_bytes());
        let vertices = read_storage("mesh_cluster vertices", &vertex_bytes);
        let instances = read_storage(
            "mesh_cluster instances",
            &GpuInstance {
                transform: IDENTITY,
                mesh: 0,
                material: 0,
                sector: 0,
                flags: GpuInstance::LIVE,
            }
            .to_bytes(),
        );
        let draw_buffer = uniform("mesh_cluster draw constants", &draw_constants.to_bytes());
        let meshes = read_storage(
            "mesh_cluster meshes",
            &GpuMesh {
                base_vertex: 0,
                base_index: 0,
                index_count: 3,
                bounds_min: [-1.0; 3],
                bounds_max: [1.0; 3],
            }
            .to_bytes(),
        );
        let visible_instances = read_storage("mesh_cluster visible instances", &0u32.to_le_bytes());
        let materials = read_storage("mesh_cluster materials", &GpuMaterial::UNTINTED.to_bytes());
        let cluster_bytes: Vec<u8> = clusters
            .iter()
            .flat_map(|cluster| cluster.to_bytes())
            .collect();
        let cluster_buffer = read_storage("mesh_cluster clusters", &cluster_bytes);
        let cluster_vertices =
            read_storage("mesh_cluster cluster vertices", &pack_words(&[0, 1, 2]));
        let cluster_corners = read_storage(
            "mesh_cluster cluster corners",
            &pack_words(&corner_words(&[0, 1, 2])),
        );
        let draw_args = read_storage(
            "mesh_cluster draw args",
            &DrawIndexedArgs {
                index_count: 3,
                instance_count: 1,
                first_index: 0,
                vertex_offset: 0,
                first_instance: 0,
            }
            .to_bytes(),
        );
        let cull_buffer = uniform("mesh_cluster cull params", &cull_params.to_bytes());
        let select_bytes: Vec<u8> = selects
            .iter()
            .flat_map(|record| record.to_bytes())
            .collect();
        assert_eq!(
            select_bytes.len(),
            PROBE_CLUSTERS as usize * CLUSTER_SELECT_STRIDE,
            "one selection record per cluster"
        );
        let cluster_select = read_storage("mesh_cluster cluster select", &select_bytes);
        let group_state_buffer =
            read_storage("mesh_cluster group state", &pack_words(&group_state));
        // Declared by the shader and read by neither stage — see that file's own
        // comments on why it declares the fragment stage's tables at all. They
        // are still bound, because a descriptor table with a hole in it is a
        // descriptor read out of an empty slot.
        let lights = read_storage("mesh_cluster lights", &[0u8; LIGHT_STRIDE]);
        let cluster_lights = read_storage("mesh_cluster froxel grid", &pack_words(&[0, 0, 0, 0]));
        let probes = read_storage("mesh_cluster probes", &[0u8; PROBE_STRIDE]);
        let tables = read_storage(
            "mesh_cluster tables",
            &vec![0u8; PROBE_DAG_GROUPS as usize * LEVEL_GROUP_STRIDE],
        );

        // The two written bindings. `DeviceLocal` because D3D12 admits an
        // unordered access view on no other heap — `crate::buffer`'s
        // `check_unordered_access` refuses the rest by name — so both are primed
        // by a copy and read back by another.
        let stats_bytes = u64::from(STATS_WORDS) * size_of::<u32>() as u64;
        let selection_bytes = u64::from(PROBE_CLUSTERS) * size_of::<u32>() as u64;
        let mut primer = Vec::new();
        for word in 0..STATS_WORDS {
            // The three cluster words start at zero because an atomic add
            // accumulates; the two this shader does not own start at the
            // sentinel, so "untouched" is distinguishable from "written zero".
            let counted = word == CLUSTER_SURVIVOR_WORD
                || word == CLUSTER_FRUSTUM_REJECT_WORD
                || word == CLUSTER_CONE_REJECT_WORD;
            primer.extend((if counted { 0 } else { PROBE_SENTINEL }).to_le_bytes());
        }
        primer.extend(
            core::iter::repeat_n(PROBE_SENTINEL, PROBE_CLUSTERS as usize)
                .flat_map(u32::to_le_bytes),
        );
        let staged = uploaded("mesh_cluster primer", BufferUsage::TRANSFER_SRC, &primer);
        let written = |label: &'static str, size| {
            device
                .create_buffer(&BufferDesc {
                    label: Some(label),
                    size,
                    usage: BufferUsage::STORAGE
                        | BufferUsage::TRANSFER_DST
                        | BufferUsage::TRANSFER_SRC,
                    memory: MemoryLocation::DeviceLocal,
                })
                .unwrap_or_else(|error| panic!("stage=create_buffer({label}): {error:?}"))
        };
        let cull_stats = written("mesh_cluster cull stats", stats_bytes);
        let cluster_selection = written("mesh_cluster cluster selection", selection_bytes);
        let stats_staging = readback_buffer(&device, stats_bytes as usize);
        let selection_staging = readback_buffer(&device, selection_bytes as usize);

        run(&device, |encoder| {
            encoder.pipeline_barrier(&Barriers {
                buffers: &[
                    buffer_barrier(
                        cull_stats,
                        ResourceState::Undefined,
                        ResourceState::TransferDst,
                    ),
                    buffer_barrier(
                        cluster_selection,
                        ResourceState::Undefined,
                        ResourceState::TransferDst,
                    ),
                ],
                ..Barriers::default()
            });
            encoder.copy_buffer_to_buffer(&BufferCopy {
                src: staged,
                src_offset: 0,
                dst: cull_stats,
                dst_offset: 0,
                size: stats_bytes,
            });
            encoder.copy_buffer_to_buffer(&BufferCopy {
                src: staged,
                src_offset: stats_bytes,
                dst: cluster_selection,
                dst_offset: 0,
                size: selection_bytes,
            });
            // `ShaderReadWrite`, not `ShaderWrite`: a barrier names the access
            // the *descriptor* permits rather than the one the source performs,
            // and an unordered-access view is both.
            encoder.pipeline_barrier(&Barriers {
                buffers: &[
                    buffer_barrier(
                        cull_stats,
                        ResourceState::TransferDst,
                        ResourceState::ShaderReadWrite,
                    ),
                    buffer_barrier(
                        cluster_selection,
                        ResourceState::TransferDst,
                        ResourceState::ShaderReadWrite,
                    ),
                ],
                ..Barriers::default()
            });
        });

        // The base-colour page and its sampler: declared by the shader, sampled
        // by neither stage, and bound for the reason the buffers above are.
        let page = device
            .create_image(&image(
                Format::Rgba8Unorm,
                ImageUsage::SAMPLED,
                Extent3d::d2(4, 4),
            ))
            .expect("a base-colour page");
        let page_view = device
            .create_image_view(&ImageViewDesc {
                label: Some("mesh_cluster base colour page"),
                image: page,
                // `D2Array`, which is what the layout entry claims and what
                // `Texture2DArray<float4>` in the source is.
                view_type: ImageViewType::D2Array,
                format: Format::Rgba8Unorm,
                range: ImageSubresourceRange::all(Format::Rgba8Unorm),
            })
            .expect("an array view of the page");
        let sampler = device
            .create_sampler(&SamplerDesc {
                label: Some("mesh_cluster base colour sampler"),
                ..SamplerDesc::default()
            })
            .expect("a sampler");

        // --- the layout ---

        // Transcribed from `crcbl_render::ForwardRenderer::mesh_layout` — see
        // the doc comment on which rows are absent and why. `ShaderStages::ALL`
        // rather than `MESH | TASK` for [`MeshProbe`]'s reason: this backend
        // reports no `Features::MESH_SHADER`, so the seam refuses a layout
        // naming those bits, and `ALL` is `D3D12_SHADER_VISIBILITY_ALL`, which
        // does reach both stages.
        let read_only = BindingKind::StorageBuffer {
            read_only: true,
            dynamic: false,
        };
        let writable = BindingKind::StorageBuffer {
            read_only: false,
            dynamic: false,
        };
        let entry = |binding: u32, kind: BindingKind| BindGroupLayoutEntry {
            binding,
            visibility: ShaderStages::ALL,
            kind,
            count: 1,
            flags: BindingFlags::empty(),
        };
        let layout_entries = vec![
            entry(0, BindingKind::UniformBuffer { dynamic: false }),
            entry(1, read_only),
            entry(2, read_only),
            // **Dynamic, and that is the renderer's mechanism**: one block per
            // bucket reached through an offset. It becomes a root descriptor
            // here rather than a table entry, and it still takes a `b` register
            // in declaration order — which is what puts `cull` on `b2`.
            entry(3, BindingKind::UniformBuffer { dynamic: true }),
            entry(4, read_only),
            entry(5, read_only),
            entry(6, read_only),
            BindGroupLayoutEntry {
                binding: 7,
                visibility: ShaderStages::ALL,
                kind: BindingKind::SampledImage {
                    view_type: ImageViewType::D2Array,
                    sample_type: crcbl_hal::SampleType::Float,
                },
                count: 1,
                flags: BindingFlags::empty(),
            },
            BindGroupLayoutEntry {
                binding: 8,
                visibility: ShaderStages::ALL,
                kind: BindingKind::Sampler { comparison: false },
                count: 1,
                flags: BindingFlags::empty(),
            },
            entry(9, read_only),
            entry(10, read_only),
            entry(11, read_only),
            entry(12, read_only),
            entry(13, BindingKind::UniformBuffer { dynamic: false }),
            entry(14, writable),
            entry(17, read_only),
            entry(18, writable),
            entry(19, read_only),
            entry(20, read_only),
            entry(21, read_only),
            entry(23, read_only),
            entry(24, read_only),
        ];
        let set_layout = device
            .create_bind_group_layout(&BindGroupLayoutDesc {
                label: Some("mesh_cluster"),
                entries: &layout_entries,
            })
            .expect("the cluster layout's twenty-two bindings");
        let pipeline_layout = device
            .create_pipeline_layout(&PipelineLayoutDesc {
                label: Some("mesh_cluster"),
                bind_group_layouts: &[set_layout],
                push_constants: None,
            })
            .expect("a root signature with one table, one sampler table and one root descriptor");

        let buffers = [
            (0u32, frame_buffer),
            (1, vertices),
            (2, instances),
            (3, draw_buffer),
            (4, meshes),
            (5, visible_instances),
            (6, materials),
            (9, cluster_buffer),
            (10, cluster_vertices),
            (11, cluster_corners),
            (12, draw_args),
            (13, cull_buffer),
            (14, cull_stats),
            (17, cluster_select),
            (18, cluster_selection),
            (19, group_state_buffer),
            (20, lights),
            (21, cluster_lights),
            (23, probes),
            (24, tables),
        ];
        let mut bound: Vec<BindGroupEntry> = buffers
            .iter()
            .map(|(binding, buffer)| BindGroupEntry {
                binding: *binding,
                array_index: 0,
                resource: BindingResource::whole_buffer(*buffer),
            })
            .collect();
        bound.push(BindGroupEntry {
            binding: 7,
            array_index: 0,
            resource: BindingResource::ImageView(page_view),
        });
        bound.push(BindGroupEntry {
            binding: 8,
            array_index: 0,
            resource: BindingResource::Sampler(sampler),
        });
        assert_eq!(
            bound.len(),
            layout_entries.len(),
            "every binding of the layout must be filled; a hole is a descriptor read out of an \
             empty slot"
        );
        let group = device
            .create_bind_group(&BindGroupDesc {
                label: Some("mesh_cluster"),
                layout: set_layout,
                entries: &bound,
                variable_count: None,
            })
            .expect("a bind group over the cluster set");

        // --- the pipeline ---

        let module = device
            .create_shader_module(&ShaderModuleDesc {
                label: Some("mesh_cluster.slang"),
                dxil: &crcbl_shaders::MESH_CLUSTER.dxil_containers(),
                ..ShaderModuleDesc::default()
            })
            .unwrap_or_else(|error| panic!("stage=create_shader_module: {error:?}"));
        let pipeline = device
            .create_mesh_pipeline(&crcbl_hal::MeshPipelineDesc {
                label: Some("mesh_cluster depth"),
                layout: pipeline_layout,
                task: Some(ShaderEntry {
                    module,
                    entry_point: "taskMain",
                }),
                // `mesh_cluster.slang`'s own numbers. D3D12 reads them out of
                // the DXIL and ignores these; they are here because Metal
                // cannot.
                task_workgroup_size: [1, 1, 1],
                mesh: ShaderEntry {
                    module,
                    entry_point: "amplifiedMeshMain",
                },
                mesh_workgroup_size: [crcbl_shaders::meshlet::MAX_CLUSTER_VERTICES as u32, 1, 1],
                fragment: None,
                primitive: PrimitiveState::default(),
                depth_stencil: Some(crcbl_hal::DepthStencilState::default()),
                multisample: MultisampleState::default(),
                color_targets: &[],
            })
            .unwrap_or_else(|error| panic!("stage=create_mesh_pipeline: {error:?}"));

        // The attachment, its readback, its pass and both of its barriers, from
        // the same [`DepthProbe`] the toy shader's depth-only probe uses — so
        // the two differ in their stages and their bindings and in nothing else.
        let depth = DepthProbe::new(&device);

        let extents: Vec<u8> = pack_words(&[PROBE_CLUSTERS, 1, 1]);
        let args = indirect_buffer(&device, "mesh_cluster dispatch extents", &extents);

        let pass = depth.pass("mesh_cluster depth");

        run(&device, |encoder| {
            encoder.pipeline_barrier(&Barriers {
                images: &[
                    depth.acquire(),
                    ImageBarrier::new(
                        page,
                        ImageSubresourceRange::all(Format::Rgba8Unorm),
                        ResourceState::Undefined,
                        ResourceState::ShaderRead,
                    ),
                ],
                ..Barriers::default()
            });
            encoder.begin_render_pass(&pass);
            // Pipeline before group: setting a root signature resets every root
            // argument, so the reverse order would bind a table the next call
            // discards.
            encoder.bind_graphics_pipeline(pipeline);
            // One dynamic offset, for binding 3: this frame draws the bucket at
            // the start of the block.
            encoder.bind_group(0, group, &[0], pipeline_layout);
            encoder.draw_mesh_tasks_indirect(&DrawIndirect {
                args,
                offset: 0,
                draw_count: 1,
                stride: 0,
            });
            encoder.end_render_pass();
            encoder.pipeline_barrier(&Barriers {
                buffers: &[
                    buffer_barrier(
                        cull_stats,
                        ResourceState::ShaderReadWrite,
                        ResourceState::TransferSrc,
                    ),
                    buffer_barrier(
                        cluster_selection,
                        ResourceState::ShaderReadWrite,
                        ResourceState::TransferSrc,
                    ),
                ],
                images: &[depth.release()],
                ..Barriers::default()
            });
            depth.resolve(encoder);
            encoder.copy_buffer_to_buffer(&BufferCopy {
                src: cull_stats,
                src_offset: 0,
                dst: stats_staging,
                dst_offset: 0,
                size: stats_bytes,
            });
            encoder.copy_buffer_to_buffer(&BufferCopy {
                src: cluster_selection,
                src_offset: 0,
                dst: selection_staging,
                dst_offset: 0,
                size: selection_bytes,
            });
        });

        // Between the submit and the `Map`, because `DXGI_ERROR_DEVICE_REMOVED`
        // is reported at the *next* call — see [`MeshProbe::frame`], which does
        // the same for the same reason.
        still_alive(&device, "mesh_cluster depth");

        // The two written bindings, read back as words. The depth attachment
        // comes back through [`DepthProbe::read`] instead.
        let words = |buffer, bytes: u64| -> Vec<u32> {
            let request = device
                .request_readback(&ReadbackDesc {
                    label: Some("mesh_cluster readback"),
                    buffer,
                    offset: 0,
                    size: bytes,
                    after: None,
                })
                .expect("a readback of a HostReadback buffer");
            let out = drain(&device, request, bytes as usize);
            device.destroy_readback(request);
            out.chunks_exact(size_of::<u32>())
                .map(|word| u32::from_le_bytes([word[0], word[1], word[2], word[3]]))
                .collect()
        };

        // --- what the frame must have produced ---

        // The same three texels the toy shader's depth-only probe asserts, at
        // the same depth the vertices above carry.
        assert_depth_triangle(
            &depth.read(&device),
            PROBE_DEPTH,
            "the one cluster the descent selected and the cull kept",
        );

        let stats = words(stats_staging, stats_bytes);
        assert_eq!(
            stats.len(),
            STATS_WORDS as usize,
            "the readback is not the whole statistics buffer"
        );
        let mut expected_stats = vec![PROBE_SENTINEL; STATS_WORDS as usize];
        expected_stats[CLUSTER_SURVIVOR_WORD as usize] = 1;
        expected_stats[CLUSTER_FRUSTUM_REJECT_WORD as usize] = 1;
        expected_stats[CLUSTER_CONE_REJECT_WORD as usize] = 1;
        assert_eq!(
            stats, expected_stats,
            "the amplification stage's three counters are not one survivor, one frustum rejection \
             and one cone rejection, with the two words it does not own untouched"
        );
        // Stated separately because it is the arithmetic `mesh_cluster.slang`'s
        // own comment makes: the three words sum to the size of the **cut**, not
        // to the clusters dispatched, because a cluster the descent did not
        // select was never put to either test.
        assert_eq!(
            stats[CLUSTER_SURVIVOR_WORD as usize]
                + stats[CLUSTER_FRUSTUM_REJECT_WORD as usize]
                + stats[CLUSTER_CONE_REJECT_WORD as usize],
            3,
            "the tested clusters must sum to the cut and not to the {PROBE_CLUSTERS} dispatched"
        );

        assert_eq!(
            words(selection_staging, selection_bytes),
            vec![1, 1, 1, 0, 0],
            "the cut the descent recorded is not the one two flags and three group words say"
        );

        // --- teardown ---

        device.destroy_buffer(args);
        depth.destroy(&device);
        device.destroy_graphics_pipeline(pipeline);
        device.destroy_shader_module(module);
        device.destroy_bind_group(group);
        device.destroy_pipeline_layout(pipeline_layout);
        device.destroy_bind_group_layout(set_layout);
        device.destroy_sampler(sampler);
        device.destroy_image_view(page_view);
        device.destroy_image(page);
        device.destroy_buffer(selection_staging);
        device.destroy_buffer(stats_staging);
        device.destroy_buffer(staged);
        // `cull_stats` and `cluster_selection` are in this list too, which is
        // why neither is released above.
        for (_, buffer) in buffers {
            device.destroy_buffer(buffer);
        }
    }

    /// `words` as the little-endian bytes a `StructuredBuffer<uint>` holds.
    fn pack_words(words: &[u32]) -> Vec<u8> {
        words.iter().flat_map(|word| word.to_le_bytes()).collect()
    }

    /// Amplification groups [`many_indirect_amplification_groups_do_not_remove_the_device`]
    /// and [`a_zero_group_dispatch_mesh_does_not_remove_the_device`] each ask one
    /// `ExecuteIndirect` for.
    ///
    /// Well inside D3D12's `DispatchMesh` bound — each of X, Y and Z is capped
    /// at 65535 and their product at 2^22 — so this is a size a conforming
    /// device must serve, not a limit being probed. It is chosen to be far more
    /// than the one group the siblings dispatch and comparable to the cluster
    /// counts `crcbl-render` reaches on a real scene.
    const MANY_GROUPS: u32 = 1024;

    /// The index pool every indexed draw below reads, and the decoy in front of
    /// it.
    ///
    /// Six `u32`s: three zeros, then the triangle's own `0, 1, 2`. Every draw
    /// reads indices `3..6`, so a backend that ignored the index buffer, dropped
    /// the first index, or read from the start of the buffer draws vertex zero
    /// three times — a degenerate triangle that rasterises nothing and leaves
    /// the clear behind, which [`assert_triangle_drawn`] fails on.
    const INDICES: [u32; 6] = [0, 0, 0, 0, 1, 2];

    /// The first index every draw below starts at, which is the decoy's length.
    const FIRST_INDEX: u32 = 3;

    /// Everything an indexed draw of `crcbl_shaders::triangle` needs, built
    /// once and drawn through several times.
    ///
    /// The same geometry, pipeline and bind group
    /// `a_pulled_triangle_is_drawn_by_d3d12_and_read_back_texel_by_texel` builds, plus an
    /// index buffer — so what the tests below vary is the *draw call* and
    /// nothing else, which is the only way the picture can be evidence about
    /// which call produced it.
    struct IndexedTriangle {
        target: ImageHandle,
        view: ImageViewHandle,
        readback: BufferHandle,
        vertices: BufferHandle,
        indices: BufferHandle,
        set_layout: BindGroupLayoutHandle,
        pipeline_layout: PipelineLayoutHandle,
        group: BindGroupHandle,
        module: ShaderModuleHandle,
        pipeline: GraphicsPipelineHandle,
        /// The state [`Self::run`] last left [`Self::target`] in, so the barrier
        /// that opens the next run declares where the image really is.
        ///
        /// **A barrier is a claim about the state its image is already in, and
        /// D3D12 validates the claim.** Every run ends with the copy, so the
        /// second and later ones start from [`ResourceState::TransferSrc`] —
        /// declaring [`ResourceState::Undefined`] again would say `COMMON` of an
        /// image sitting in `COPY_SOURCE`, which is
        /// `ID3D12CommandQueue::ExecuteCommandLists`' "before state … does not
        /// match with the state specified in preceding ResourceBarrier".
        left_in: core::cell::Cell<ResourceState>,
    }

    impl IndexedTriangle {
        fn new(device: &Dx12Device) -> Self {
            use crcbl_shaders::{TRIANGLE, triangle};

            let target = device
                .create_image(&image(
                    Format::Rgba8Unorm,
                    ImageUsage::COLOR_ATTACHMENT | ImageUsage::TRANSFER_SRC,
                    SQUARE,
                ))
                .expect("a colour target");
            let view = device
                .create_image_view(&whole(target, Format::Rgba8Unorm))
                .expect("a render target view");
            let readback = readback_buffer(device, SQUARE_BYTES);

            // Both on the upload heap, which D3D12 leaves in `GENERIC_READ` for
            // the resource's whole life — a state that already admits a shader
            // read *and* an index-buffer read, so neither needs a copy or a
            // barrier. The indirect arguments below are a different story and
            // get both.
            let geometry = triangle::vertex_bytes();
            let vertices = device
                .create_buffer(&BufferDesc {
                    label: Some("triangle vertices"),
                    size: geometry.len() as u64,
                    usage: BufferUsage::STORAGE,
                    memory: MemoryLocation::HostUpload,
                })
                .expect("a vertex storage buffer");
            device
                .write_buffer(vertices, 0, &geometry)
                .expect("an upload-heap buffer is host-visible");

            let index_bytes: Vec<u8> = INDICES.iter().flat_map(|i| i.to_le_bytes()).collect();
            let indices = device
                .create_buffer(&BufferDesc {
                    label: Some("triangle indices"),
                    size: index_bytes.len() as u64,
                    usage: BufferUsage::INDEX,
                    memory: MemoryLocation::HostUpload,
                })
                .expect("an index buffer");
            device
                .write_buffer(indices, 0, &index_bytes)
                .expect("an upload-heap buffer is host-visible");

            let set_layout = device
                .create_bind_group_layout(&BindGroupLayoutDesc {
                    label: Some("triangle geometry"),
                    entries: &[BindGroupLayoutEntry {
                        binding: 0,
                        visibility: ShaderStages::VERTEX,
                        kind: BindingKind::StorageBuffer {
                            read_only: true,
                            dynamic: false,
                        },
                        count: 1,
                        flags: BindingFlags::empty(),
                    }],
                })
                .expect("one read-only storage buffer");
            let pipeline_layout = device
                .create_pipeline_layout(&PipelineLayoutDesc {
                    label: Some("triangle"),
                    bind_group_layouts: &[set_layout],
                    push_constants: None,
                })
                .expect("a root signature with one descriptor table");
            let group = device
                .create_bind_group(&BindGroupDesc {
                    label: Some("triangle geometry"),
                    layout: set_layout,
                    entries: &[BindGroupEntry {
                        binding: 0,
                        array_index: 0,
                        resource: BindingResource::whole_buffer(vertices),
                    }],
                    variable_count: None,
                })
                .expect("a bind group over the vertex buffer");
            let module = device
                .create_shader_module(&ShaderModuleDesc {
                    label: Some("triangle.slang"),
                    dxil: &TRIANGLE.dxil_containers(),
                    ..ShaderModuleDesc::default()
                })
                .unwrap_or_else(|error| panic!("stage=create_shader_module: {error:?}"));
            let targets = [ColorTargetState::opaque(Format::Rgba8Unorm)];
            let pipeline = device
                .create_graphics_pipeline(&GraphicsPipelineDesc {
                    label: Some("triangle"),
                    layout: pipeline_layout,
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
                .unwrap_or_else(|error| panic!("stage=create_graphics_pipeline: {error:?}"));

            Self {
                target,
                view,
                readback,
                vertices,
                indices,
                set_layout,
                pipeline_layout,
                group,
                module,
                pipeline,
                left_in: core::cell::Cell::new(ResourceState::Undefined),
            }
        }

        /// Clears the target, records `record` inside the pass with the
        /// pipeline, group and index buffer already bound, and reads the frame
        /// back.
        ///
        /// The readback is re-poisoned first, so a run that copied nothing is a
        /// frame of [`POISON`] rather than the previous run's picture — which is
        /// what stops one green draw making every later assertion vacuous.
        ///
        /// The opening barrier comes from [`Self::left_in`] rather than from
        /// [`ResourceState::Undefined`] every time, which is what makes a second
        /// run's transition true; see that field.
        fn run(
            &self,
            device: &Dx12Device,
            record: impl FnOnce(&mut dyn CommandEncoder),
        ) -> Vec<u8> {
            device
                .write_buffer(self.readback, 0, &vec![POISON; SQUARE_BYTES])
                .expect("a readback buffer is host-visible");
            let pass = clear_pass(
                self.view,
                CLEAR,
                LoadOp::Clear,
                Rect2d::from_size(SQUARE.width, SQUARE.height),
            );
            run(device, |encoder| {
                encoder.pipeline_barrier(&Barriers {
                    images: &[ImageBarrier::new(
                        self.target,
                        ImageSubresourceRange::all(Format::Rgba8Unorm),
                        self.left_in.get(),
                        ResourceState::ColorAttachment,
                    )],
                    ..Barriers::default()
                });
                encoder.begin_render_pass(&pass.desc());
                // Pipeline before group: setting a root signature resets every
                // root argument, so the reverse order would bind a table the
                // next call discards.
                encoder.bind_graphics_pipeline(self.pipeline);
                encoder.bind_group(0, self.group, &[], self.pipeline_layout);
                encoder.bind_index_buffer(self.indices, 0, IndexFormat::Uint32);
                record(encoder);
                encoder.end_render_pass();
                encoder.pipeline_barrier(&Barriers {
                    images: &[ImageBarrier::new(
                        self.target,
                        ImageSubresourceRange::all(Format::Rgba8Unorm),
                        ResourceState::ColorAttachment,
                        ResourceState::TransferSrc,
                    )],
                    ..Barriers::default()
                });
                encoder.copy_image_to_buffer(&BufferImageCopy {
                    buffer: self.readback,
                    buffer_offset: 0,
                    buffer_row_length: 0,
                    buffer_image_height: 0,
                    image: self.target,
                    image_subresource: ImageSubresourceLayers {
                        aspect: ImageAspect::COLOR,
                        mip: 0,
                        base_layer: 0,
                        layer_count: 1,
                    },
                    image_offset: Offset3d::default(),
                    image_extent: SQUARE,
                });
            });
            self.left_in.set(ResourceState::TransferSrc);

            let request = device
                .request_readback(&ReadbackDesc {
                    label: Some("crcbl-dx12 indexed triangle readback"),
                    buffer: self.readback,
                    offset: 0,
                    size: SQUARE_BYTES as u64,
                    after: None,
                })
                .expect("a readback of a HostReadback buffer");
            let bytes = drain(device, request, SQUARE_BYTES);
            device.destroy_readback(request);
            bytes
        }

        fn destroy(self, device: &Dx12Device) {
            device.destroy_graphics_pipeline(self.pipeline);
            device.destroy_shader_module(self.module);
            device.destroy_bind_group(self.group);
            device.destroy_pipeline_layout(self.pipeline_layout);
            device.destroy_bind_group_layout(self.set_layout);
            device.destroy_buffer(self.indices);
            device.destroy_buffer(self.vertices);
            device.destroy_buffer(self.readback);
            device.destroy_image_view(self.view);
            device.destroy_image(self.target);
        }
    }

    /// One `D3D12_DRAW_INDEXED_ARGUMENTS` as the bytes an argument buffer holds.
    ///
    /// Written through `crcbl_shaders::draw_gen::DrawIndexedArgs` rather than by
    /// hand, because that is the struct `draw_gen.slang` writes and the whole
    /// claim of the indirect path is that D3D12 reads the *same* five words the
    /// GPU wrote for every other backend. A private copy here would be a second
    /// layout that agreed by coincidence.
    fn indexed_args(index_count: u32, instance_count: u32) -> [u8; ARGS_BYTES] {
        crcbl_shaders::draw_gen::DrawIndexedArgs {
            index_count,
            instance_count,
            first_index: FIRST_INDEX,
            // Both bases zero, which is the rule `mesh.slang`'s header sets and
            // the only value the four shader targets agree on. D3D12 would read
            // either field happily; the shaders would not agree about what it
            // meant.
            vertex_offset: 0,
            first_instance: 0,
        }
        .to_bytes()
    }

    /// Bytes of one indexed indirect argument structure.
    const ARGS_BYTES: usize = crcbl_shaders::draw_gen::DRAW_ARGS_SIZE;

    /// Uploads `bytes` into a `DeviceLocal` buffer left in
    /// [`ResourceState::IndirectArgument`], which is where `ExecuteIndirect`
    /// requires its arguments and its count to be.
    ///
    /// Device-local with a copy and two barriers rather than the upload heap the
    /// geometry uses: `GENERIC_READ` would already admit the read, and taking
    /// the easy road would leave the state transition every real GPU-driven
    /// frame makes — compute writes the arguments, then they are read as
    /// arguments — untested on this backend.
    fn indirect_buffer(device: &Dx12Device, label: &'static str, bytes: &[u8]) -> BufferHandle {
        let upload = device
            .create_buffer(&BufferDesc {
                label: Some("indirect upload"),
                size: bytes.len() as u64,
                usage: BufferUsage::TRANSFER_SRC,
                memory: MemoryLocation::HostUpload,
            })
            .expect("a staging buffer");
        device.write_buffer(upload, 0, bytes).expect("write");
        let handle = device
            .create_buffer(&BufferDesc {
                label: Some(label),
                size: bytes.len() as u64,
                usage: BufferUsage::INDIRECT | BufferUsage::TRANSFER_DST,
                memory: MemoryLocation::DeviceLocal,
            })
            .expect("an indirect buffer");
        run(device, |encoder| {
            encoder.pipeline_barrier(&Barriers {
                buffers: &[buffer_barrier(
                    handle,
                    ResourceState::Undefined,
                    ResourceState::TransferDst,
                )],
                ..Barriers::default()
            });
            encoder.copy_buffer_to_buffer(&BufferCopy {
                src: upload,
                src_offset: 0,
                dst: handle,
                dst_offset: 0,
                size: bytes.len() as u64,
            });
            encoder.pipeline_barrier(&Barriers {
                buffers: &[buffer_barrier(
                    handle,
                    ResourceState::TransferDst,
                    ResourceState::IndirectArgument,
                )],
                ..Barriers::default()
            });
        });
        device.destroy_buffer(upload);
        handle
    }

    /// **`DrawIndexedInstanced` reads the indices it was bound, at the first
    /// index it was given.**
    ///
    /// The index buffer carries a **decoy**: three zeros in front of the
    /// triangle's own `0, 1, 2`, and the draw reads `3..6`. So three different
    /// failures are distinguishable rather than confusable — a backend that
    /// never set the view, one that set it at the wrong offset, and one that
    /// dropped `StartIndexLocation` all draw vertex zero three times, which
    /// rasterises nothing and leaves the clear the second assertion rejects.
    ///
    /// The base vertex and the base instance are **zero**, and that is the
    /// engine's rule rather than D3D12's: `crates/crcbl-shaders/shaders/mesh.slang`'s
    /// header measured `slangc` lowering `SV_VertexID` and `SV_InstanceID` four
    /// different ways, of which D3D12's excludes both bases and WGSL's and MSL's
    /// include them, so zero is the only value all four agree on. D3D12 would
    /// read a non-zero one; the shader would mean something else by it.
    ///
    /// # What only CI can settle
    ///
    /// All of it. This crate compiles on Windows alone and the development box
    /// is Linux, so nothing here has ever executed outside a CI runner.
    #[test]
    #[ignore = "needs a real D3D12 device; run tests/run-dx12-e2e.sh"]
    fn a_d3d12_indexed_draw_reads_the_index_buffer_it_was_bound() {
        let (_instance, device) = open_device();
        let triangle = IndexedTriangle::new(&device);

        let drawn = triangle.run(&device, |encoder| {
            encoder.draw_indexed(FIRST_INDEX..FIRST_INDEX + 3, 0, 0..1);
        });
        assert_triangle_drawn(&drawn, "an indexed draw of indices 3..6");

        // The decoy's own three indices are all vertex zero, so drawing them is
        // a degenerate triangle and nothing rasterises. That is what makes the
        // assertion above about the *range* rather than about the draw.
        let decoy = triangle.run(&device, |encoder| {
            encoder.draw_indexed(0..FIRST_INDEX, 0, 0..1);
        });
        assert_nothing_drawn(&decoy, "an indexed draw of the decoy's three zeros");

        triangle.destroy(&device);
    }

    /// **`ExecuteIndirect` reads a `DRAW_INDEXED` argument structure out of GPU
    /// memory, at the offset it was given, as many times as it was told.**
    ///
    /// The argument buffer holds two structures: a first that draws **zero**
    /// instances and a second that draws one. So the picture says which
    /// structures were read:
    ///
    /// * One command at offset zero — the first structure — draws nothing.
    /// * One command at offset [`ARGS_BYTES`] — the second — draws the triangle,
    ///   which is a backend that honoured `ArgumentBufferOffset`.
    /// * Two commands from offset zero draw the triangle too, and *only* if
    ///   `MaxCommandCount` and the signature's `ByteStride` both landed: a
    ///   backend that executed one command, or strode by the wrong number, reads
    ///   the zero-instance structure and nothing else.
    ///
    /// That last case is the whole of
    /// [`Features::MULTI_DRAW_INDIRECT`] on this backend — `ExecuteIndirect`
    /// emits `MaxCommandCount` draws natively, where `crcbl-mtl` has to loop.
    ///
    /// # What only CI can settle
    ///
    /// All of it, for the reason
    /// [`a_d3d12_indexed_draw_reads_the_index_buffer_it_was_bound`] gives.
    #[test]
    #[ignore = "needs a real D3D12 device; run tests/run-dx12-e2e.sh"]
    fn an_indexed_indirect_draw_reads_its_arguments_at_the_offset_and_count_it_was_given() {
        let (_instance, device) = open_device();
        assert!(
            device
                .caps()
                .features
                .contains(Features::MULTI_DRAW_INDIRECT),
            "ExecuteIndirect takes a MaxCommandCount on every D3D12 device; adapter caps report \
             {:?}",
            device.caps().features
        );

        let triangle = IndexedTriangle::new(&device);
        let mut bytes = Vec::with_capacity(ARGS_BYTES * 2);
        bytes.extend_from_slice(&indexed_args(3, 0));
        bytes.extend_from_slice(&indexed_args(3, 1));
        let args = indirect_buffer(&device, "indexed draw arguments", &bytes);
        let stride = ARGS_BYTES as u32;

        let first = triangle.run(&device, |encoder| {
            encoder.draw_indexed_indirect(&DrawIndirect {
                args,
                offset: 0,
                draw_count: 1,
                stride,
            });
        });
        assert_nothing_drawn(&first, "one command reading the zero-instance structure");

        let second = triangle.run(&device, |encoder| {
            encoder.draw_indexed_indirect(&DrawIndirect {
                args,
                offset: ARGS_BYTES as u64,
                draw_count: 1,
                stride,
            });
        });
        assert_triangle_drawn(&second, "one command at the second structure's offset");

        let both = triangle.run(&device, |encoder| {
            encoder.draw_indexed_indirect(&DrawIndirect {
                args,
                offset: 0,
                draw_count: 2,
                stride,
            });
        });
        assert_triangle_drawn(&both, "two commands strided over both structures");

        device.destroy_buffer(args);
        triangle.destroy(&device);
    }

    /// **`ExecuteIndirect` reads its draw count out of GPU memory, and that is
    /// the evidence behind [`Features::DRAW_INDIRECT_COUNT`].**
    ///
    /// One argument structure that draws the triangle, and two count buffers:
    /// one holding `1` and one holding `0`. Nothing else differs between the two
    /// runs, so the picture is a readout of the `u32` the GPU held — a backend
    /// that passed no count buffer draws the triangle both times, and one that
    /// read the wrong offset draws it neither.
    ///
    /// The flag assertion and the picture are in the same test on purpose: this
    /// is the flag that selects
    /// [`GeometryPath::IndirectCount`](crcbl_hal::GeometryPath::IndirectCount)
    /// for every adapter, so a flag reported over a call that does nothing is
    /// the "unsupported arriving as passed" shape the crate docs name, and
    /// neither half passes without the other.
    ///
    /// # What only CI can settle
    ///
    /// All of it, for the reason
    /// [`a_d3d12_indexed_draw_reads_the_index_buffer_it_was_bound`] gives.
    #[test]
    #[ignore = "needs a real D3D12 device; run tests/run-dx12-e2e.sh"]
    fn an_indexed_indirect_count_draw_reads_its_count_from_gpu_memory() {
        /// Where the count sits in its buffer. Non-zero, and a `0` sits at zero,
        /// so a backend that ignored `CountBufferOffset` reads a count of no
        /// draws and the assertion below fails on the run that must draw.
        const COUNT_OFFSET: u64 = 4;

        let (_instance, device) = open_device();
        let caps = device.caps();
        assert!(
            caps.features.contains(Features::DRAW_INDIRECT_COUNT),
            "ExecuteIndirect takes a count buffer on every D3D12 device; adapter caps report {:?}",
            caps.features
        );
        assert_eq!(
            caps.geometry_path(),
            crcbl_hal::GeometryPath::IndirectCount,
            "DRAW_INDIRECT_COUNT is what selects this path, and it is reported"
        );
        assert!(
            caps.limits.max_draw_indirect_count > 1,
            "a reported DRAW_INDIRECT_COUNT with a ceiling of one draw is not the feature"
        );

        let triangle = IndexedTriangle::new(&device);
        let args = indirect_buffer(&device, "counted draw arguments", &indexed_args(3, 1));

        // Two `u32`s, `[0, 1]`: the decoy at offset zero and the real count at
        // `COUNT_OFFSET`.
        let counts = |value: u32| {
            let mut bytes = [0u8; 8];
            bytes[COUNT_OFFSET as usize..].copy_from_slice(&value.to_le_bytes());
            bytes
        };
        let draw = |count_buffer| DrawIndirectCount {
            args,
            args_offset: 0,
            count_buffer,
            count_offset: COUNT_OFFSET,
            max_draw_count: 1,
            stride: ARGS_BYTES as u32,
        };

        let one = indirect_buffer(&device, "a count of one", &counts(1));
        let drawn = triangle.run(&device, |encoder| {
            encoder.draw_indexed_indirect_count(&draw(one));
        });
        assert_triangle_drawn(&drawn, "a GPU-side count of one");

        let zero = indirect_buffer(&device, "a count of zero", &counts(0));
        let blank = triangle.run(&device, |encoder| {
            encoder.draw_indexed_indirect_count(&draw(zero));
        });
        assert_nothing_drawn(&blank, "a GPU-side count of zero");

        device.destroy_buffer(zero);
        device.destroy_buffer(one);
        device.destroy_buffer(args);
        triangle.destroy(&device);
    }

    /// Every draw this backend refuses at record time, and one it accepts.
    ///
    /// The refusals are the ones `ExecuteIndirect` and `DrawIndexedInstanced`
    /// would not report: D3D12 bounds-checks no argument span, reports no
    /// missing index-buffer view, and takes an unaligned offset as a fault
    /// rather than an error. Each is asserted by the text it names, so a backend
    /// that refused everything for one reason would fail on the wrong message
    /// rather than pass.
    ///
    /// The accepted draw at the end is what makes the six above about their own
    /// descriptors rather than about an encoder that refuses draws.
    #[test]
    #[ignore = "needs a real D3D12 device; run tests/run-dx12-e2e.sh"]
    fn the_draws_d3d12_cannot_express_are_refused_by_name() {
        let (_instance, device) = open_device();
        let triangle = IndexedTriangle::new(&device);
        let args = indirect_buffer(&device, "one structure", &indexed_args(3, 1));
        let stride = ARGS_BYTES as u32;
        let queue = device
            .queue(QueueKind::Graphics)
            .expect("the graphics queue exists");

        // `(what the error must say, what this is, whether to bind the index
        // buffer, the call)`.
        type Refused = (
            &'static str,
            &'static str,
            bool,
            Box<dyn Fn(&mut dyn CommandEncoder)>,
        );
        let cases: Vec<Refused> = vec![
            (
                "no index buffer bound",
                "an indexed draw with no view set",
                false,
                Box::new(|encoder: &mut dyn CommandEncoder| {
                    encoder.draw_indexed(0..3, 0, 0..1);
                }),
            ),
            (
                "no index buffer bound",
                "an indexed indirect draw with no view set",
                false,
                Box::new(move |encoder: &mut dyn CommandEncoder| {
                    encoder.draw_indexed_indirect(&DrawIndirect {
                        args,
                        offset: 0,
                        draw_count: 1,
                        stride,
                    });
                }),
            ),
            (
                "runs past a",
                "two structures in a buffer holding one",
                true,
                Box::new(move |encoder: &mut dyn CommandEncoder| {
                    encoder.draw_indexed_indirect(&DrawIndirect {
                        args,
                        offset: 0,
                        draw_count: 2,
                        stride,
                    });
                }),
            ),
            (
                "ArgumentBufferOffset",
                "an unaligned argument offset",
                true,
                Box::new(move |encoder: &mut dyn CommandEncoder| {
                    encoder.draw_indexed_indirect(&DrawIndirect {
                        args,
                        offset: 2,
                        draw_count: 1,
                        stride,
                    });
                }),
            ),
            (
                "CountBufferOffset",
                "an unaligned count offset",
                true,
                Box::new(move |encoder: &mut dyn CommandEncoder| {
                    encoder.draw_indexed_indirect_count(&DrawIndirectCount {
                        args,
                        args_offset: 0,
                        count_buffer: args,
                        count_offset: 2,
                        max_draw_count: 1,
                        stride,
                    });
                }),
            ),
            (
                "byte count at offset",
                "a count read past the end of its buffer",
                true,
                Box::new(move |encoder: &mut dyn CommandEncoder| {
                    encoder.draw_indexed_indirect_count(&DrawIndirectCount {
                        args,
                        args_offset: 0,
                        count_buffer: args,
                        count_offset: ARGS_BYTES as u64,
                        max_draw_count: 1,
                        stride,
                    });
                }),
            ),
            (
                "not a multiple of the",
                "an index buffer bound at an offset the width forbids",
                false,
                Box::new(move |encoder: &mut dyn CommandEncoder| {
                    encoder.bind_index_buffer(triangle.indices, 2, IndexFormat::Uint32);
                }),
            ),
        ];
        assert!(!cases.is_empty(), "nothing to check");

        for (expected, what, bind, record) in &cases {
            let mut encoder =
                device.create_command_encoder(&CommandEncoderDesc { label: None, queue });
            encoder.bind_graphics_pipeline(triangle.pipeline);
            if *bind {
                encoder.bind_index_buffer(triangle.indices, 0, IndexFormat::Uint32);
            }
            record(encoder.as_mut());
            let Err(error) = encoder.finish() else {
                panic!("{what} recorded successfully, so the encoder reported a lie");
            };
            let HalError::InvalidDescriptor(text) = &error else {
                panic!("{what}: a descriptor D3D12 cannot express is not {error:?}");
            };
            assert!(text.contains(expected), "{what}: {text}");
        }

        // And the draw every refusal above is a variation on still records, so
        // none of them is this encoder refusing draws.
        let mut good = device.create_command_encoder(&CommandEncoderDesc {
            label: Some("a well-formed indexed indirect draw"),
            queue,
        });
        good.bind_graphics_pipeline(triangle.pipeline);
        good.bind_index_buffer(triangle.indices, 0, IndexFormat::Uint32);
        good.draw_indexed_indirect(&DrawIndirect {
            args,
            offset: 0,
            draw_count: 1,
            stride,
        });
        let commands = good.finish().expect("a well-formed indirect draw records");
        device.destroy_command_buffer(commands);

        device.destroy_buffer(args);
        triangle.destroy(&device);
    }

    /// [`LoadOp::Load`] keeps what is there and [`LoadOp::Clear`] replaces it,
    /// in one command buffer so the two answers cannot come from two different
    /// images.
    ///
    /// The three readbacks share one buffer at three offsets, each a multiple of
    /// [`D3D12_TEXTURE_DATA_PLACEMENT_ALIGNMENT`] because a placed footprint's
    /// offset must be — which is the rule `plan_copy` enforces and this exercises
    /// from the outside.
    #[test]
    #[ignore = "needs a real D3D12 device; run tests/run-dx12-e2e.sh"]
    fn a_d3d12_load_op_preserves_what_clear_replaces() {
        let (_instance, device) = open_device();
        let (target, view) = color_target(&device);
        let readback = readback_buffer(&device, TARGET_BYTES * 3);
        let whole = Rect2d::from_size(TARGET.width, TARGET.height);
        let first = clear_pass(view, CLEAR, LoadOp::Clear, whole);
        let second = clear_pass(view, OTHER, LoadOp::Load, whole);
        let third = clear_pass(view, OTHER, LoadOp::Clear, whole);
        let range = ImageSubresourceRange::all(Format::Rgba8Unorm);
        let to_source = |encoder: &mut dyn CommandEncoder| {
            encoder.pipeline_barrier(&Barriers {
                images: &[ImageBarrier::new(
                    target,
                    range,
                    ResourceState::ColorAttachment,
                    ResourceState::TransferSrc,
                )],
                ..Barriers::default()
            });
        };
        let to_attachment = |encoder: &mut dyn CommandEncoder| {
            encoder.pipeline_barrier(&Barriers {
                images: &[ImageBarrier::new(
                    target,
                    range,
                    ResourceState::TransferSrc,
                    ResourceState::ColorAttachment,
                )],
                ..Barriers::default()
            });
        };

        run(&device, |encoder| {
            encoder.pipeline_barrier(&Barriers {
                images: &[ImageBarrier::new(
                    target,
                    range,
                    ResourceState::Undefined,
                    ResourceState::ColorAttachment,
                )],
                ..Barriers::default()
            });
            for (index, pass) in [&first, &second, &third].into_iter().enumerate() {
                if index > 0 {
                    to_attachment(encoder);
                }
                encoder.begin_render_pass(&pass.desc());
                encoder.end_render_pass();
                to_source(encoder);
                encoder.copy_image_to_buffer(&whole_image_copy(
                    readback,
                    (index * TARGET_BYTES) as u64,
                    target,
                ));
            }
        });

        let request = device
            .request_readback(&ReadbackDesc {
                label: None,
                buffer: readback,
                offset: 0,
                size: (TARGET_BYTES * 3) as u64,
                after: None,
            })
            .expect("a readback of the whole buffer");
        let bytes = drain(&device, request, TARGET_BYTES * 3);
        assert_eq!(
            &bytes[..TARGET_BYTES],
            expected(CLEAR_TEXEL),
            "pass 1 cleared"
        );
        assert_eq!(
            &bytes[TARGET_BYTES..TARGET_BYTES * 2],
            expected(CLEAR_TEXEL),
            "LoadOp::Load overwrote the attachment with its own clear value"
        );
        assert_eq!(
            &bytes[TARGET_BYTES * 2..],
            expected(OTHER_TEXEL),
            "LoadOp::Clear did not replace what LoadOp::Load preserved"
        );

        device.destroy_readback(request);
        device.destroy_buffer(readback);
        device.destroy_image_view(view);
        device.destroy_image(target);
    }

    /// **A clear honours the render area, which is Vulkan's semantic and not
    /// Metal's.**
    ///
    /// `crcbl-mtl` documents the opposite: a Metal `loadAction` clears the whole
    /// attachment whatever the pass's area. D3D12's clears take a rectangle
    /// list, so this backend passes the area through — and that is a claim worth
    /// an assertion rather than a sentence, because a backend that dropped the
    /// rectangle would clear everything and pass every other test in this file.
    ///
    /// The falsifying value is the right-hand half: it must still hold the
    /// colour the *first* pass wrote.
    #[test]
    #[ignore = "needs a real D3D12 device; run tests/run-dx12-e2e.sh"]
    fn a_clear_covers_the_render_area_and_leaves_the_rest_alone() {
        let (_instance, device) = open_device();
        let (target, view) = color_target(&device);
        let readback = readback_buffer(&device, TARGET_BYTES);
        let half = TARGET.width / 2;
        let whole = clear_pass(
            view,
            OTHER,
            LoadOp::Clear,
            Rect2d::from_size(TARGET.width, TARGET.height),
        );
        // `LoadOp::Clear` over half the attachment. D3D12 has no load op — the
        // clear *is* a `ClearRenderTargetView` with a rectangle list — so this
        // is the call whose rectangle the assertion below is about.
        let left = clear_pass(
            view,
            CLEAR,
            LoadOp::Clear,
            Rect2d::from_size(half, TARGET.height),
        );
        let range = ImageSubresourceRange::all(Format::Rgba8Unorm);

        run(&device, |encoder| {
            encoder.pipeline_barrier(&Barriers {
                images: &[ImageBarrier::new(
                    target,
                    range,
                    ResourceState::Undefined,
                    ResourceState::ColorAttachment,
                )],
                ..Barriers::default()
            });
            encoder.begin_render_pass(&whole.desc());
            encoder.end_render_pass();
            encoder.begin_render_pass(&left.desc());
            encoder.end_render_pass();
            encoder.pipeline_barrier(&Barriers {
                images: &[ImageBarrier::new(
                    target,
                    range,
                    ResourceState::ColorAttachment,
                    ResourceState::TransferSrc,
                )],
                ..Barriers::default()
            });
            encoder.copy_image_to_buffer(&whole_image_copy(readback, 0, target));
        });

        let request = device
            .request_readback(&ReadbackDesc {
                label: None,
                buffer: readback,
                offset: 0,
                size: TARGET_BYTES as u64,
                after: None,
            })
            .expect("a readback");
        let bytes = drain(&device, request, TARGET_BYTES);
        let row = TARGET.width as usize * 4;
        let split = half as usize * 4;
        for y in 0..TARGET.height as usize {
            let start = y * row;
            assert_eq!(
                &bytes[start..start + 4],
                &CLEAR_TEXEL,
                "row {y}: the second pass did not clear inside its render area"
            );
            assert_eq!(
                &bytes[start + split..start + split + 4],
                &OTHER_TEXEL,
                "row {y}: the second pass cleared outside its render area, so the rectangle was \
                 dropped"
            );
        }

        device.destroy_readback(request);
        device.destroy_buffer(readback);
        device.destroy_image_view(view);
        device.destroy_image(target);
    }

    /// A buffer copy moves the bytes, at both offsets, and nothing else.
    ///
    /// The window is deliberately not the whole buffer: a copy that ignored one
    /// of the two offsets would still move the right *number* of bytes, and only
    /// the poison either side of the window catches it.
    #[test]
    #[ignore = "needs a real D3D12 device; run tests/run-dx12-e2e.sh"]
    fn a_d3d12_buffer_to_buffer_copy_moves_the_bytes_at_both_offsets() {
        let (_instance, device) = open_device();
        let source = device
            .create_buffer(&BufferDesc {
                label: Some("crcbl-dx12 copy source"),
                size: 64,
                usage: BufferUsage::TRANSFER_SRC,
                memory: MemoryLocation::HostUpload,
            })
            .expect("an upload buffer");
        let payload: Vec<u8> = (0..64u8).collect();
        device
            .write_buffer(source, 0, &payload)
            .expect("an upload buffer is host-visible");
        let readback = readback_buffer(&device, 64);

        run(&device, |encoder| {
            encoder.copy_buffer_to_buffer(&BufferCopy {
                src: source,
                src_offset: 8,
                dst: readback,
                dst_offset: 16,
                size: 32,
            });
        });

        let request = device
            .request_readback(&ReadbackDesc {
                label: None,
                buffer: readback,
                offset: 0,
                size: 64,
                after: None,
            })
            .expect("a readback");
        let bytes = drain(&device, request, 64);
        let mut want = vec![POISON; 64];
        want[16..48].copy_from_slice(&payload[8..40]);
        assert_eq!(bytes, want, "the copy landed at the wrong offsets");

        device.destroy_readback(request);
        device.destroy_buffer(readback);
        device.destroy_buffer(source);
    }

    /// **A resource destroyed while its submission is in flight is not freed
    /// under the GPU.**
    ///
    /// This is what `crate::retire` exists for, and the two halves it can pin
    /// deterministically are here:
    ///
    /// * The copy still delivers the right bytes although `destroy_buffer` ran
    ///   between `submit` and completion. A backend whose `destroy_buffer`
    ///   released the last reference would be reading freed driver memory —
    ///   which is the bug, and which WARP is free to survive by luck, so this
    ///   half is necessary and not sufficient.
    /// * The handle is genuinely dead the moment `destroy_buffer` returns, so
    ///   the reference that keeps the resource alive is the submission's and not
    ///   a destroy that quietly deferred.
    /// * The retire queue drains to empty once the device is idle, so the
    ///   references it took are released rather than leaked for the process's
    ///   life.
    ///
    /// The half that is **not** here is "nothing is released early", which is a
    /// race at this level and is pinned instead by `crate::retire`'s own unit
    /// tests over a payload that records its release.
    #[test]
    #[ignore = "needs a real D3D12 device; run tests/run-dx12-e2e.sh"]
    fn a_buffer_destroyed_while_its_submission_is_in_flight_survives_it() {
        let (_instance, device) = open_device();
        let queue = device
            .queue(QueueKind::Graphics)
            .expect("the graphics queue exists");
        let source = device
            .create_buffer(&BufferDesc {
                label: Some("crcbl-dx12 doomed source"),
                size: 64,
                usage: BufferUsage::TRANSFER_SRC,
                memory: MemoryLocation::HostUpload,
            })
            .expect("an upload buffer");
        let payload: Vec<u8> = (0..64u8).map(|byte| byte ^ 0x5A).collect();
        device
            .write_buffer(source, 0, &payload)
            .expect("an upload buffer is host-visible");
        let readback = readback_buffer(&device, 64);

        let mut encoder = device.create_command_encoder(&CommandEncoderDesc {
            label: Some("crcbl-dx12 doomed copy"),
            queue,
        });
        encoder.copy_buffer_to_buffer(&BufferCopy {
            src: source,
            src_offset: 0,
            dst: readback,
            dst_offset: 0,
            size: 64,
        });
        let command_buffer = encoder.finish().expect("a recorded copy");
        device
            .submit(queue, &SubmitInfo::new(&[command_buffer]))
            .expect("a copy submits");

        // Destroyed with the submission in flight — the whole point. The
        // command buffer goes too: `ExecuteCommandLists` does not retain the
        // list any more than it retains the resources, so this is the release
        // the retire queue is actually the only thing standing between.
        device.destroy_buffer(source);
        device.destroy_command_buffer(command_buffer);
        let error = device
            .write_buffer(source, 0, &[0u8; 4])
            .expect_err("the handle died with the destroy, whatever the resource did");
        assert!(
            matches!(error, HalError::InvalidHandle { kind, .. } if kind == "buffer"),
            "{error:?}"
        );
        let error = device
            .submit(queue, &SubmitInfo::new(&[command_buffer]))
            .expect_err("the command buffer's handle died too");
        assert!(
            matches!(error, HalError::InvalidHandle { kind, .. } if kind == "command buffer"),
            "{error:?}"
        );

        device.wait_idle().expect("the copy completes");
        let request = device
            .request_readback(&ReadbackDesc {
                label: None,
                buffer: readback,
                offset: 0,
                size: 64,
                after: None,
            })
            .expect("a readback");
        assert_eq!(
            drain(&device, request, 64),
            payload,
            "the copy read a buffer that had already been freed"
        );

        device.wait_idle().expect("nothing is left in flight");
        assert_eq!(
            device.state().retire.pending(),
            0,
            "the retire queue held references past an idle device, so it leaks"
        );

        device.destroy_readback(request);
        device.destroy_buffer(readback);
    }

    /// The copies D3D12 cannot place are refused by name, before anything is
    /// recorded.
    ///
    /// Every case here is a rule the seam has no field for, so a backend that
    /// did not check would hand D3D12 a footprint it rejects — and a rejected
    /// `CopyTextureRegion` returns `void`, so the failure would arrive as a
    /// readback of the wrong bytes rather than as an error.
    #[test]
    #[ignore = "needs a real D3D12 device; run tests/run-dx12-e2e.sh"]
    fn a_copy_d3d12_cannot_place_is_refused_by_name() {
        let (instance, device) = open_device();
        // **The dirty report is the point of this test.** The refusal asserted
        // below is D3D12's own — the seam does not reject this row pitch before
        // the call, which is exactly why the layer has something to say about
        // it. So the teardown assertion stands down here, the way `crcbl-vk`'s
        // gate tests decline to call `Headless::finish`.
        instance.defuse();
        let queue = device
            .queue(QueueKind::Graphics)
            .expect("the graphics queue exists");
        // 63 texels is 252 bytes a row, which is not a multiple of D3D12's
        // 256-byte pitch — the case that makes the whole rule visible.
        let narrow = device
            .create_image(&image(
                Format::Rgba8Unorm,
                ImageUsage::COLOR_ATTACHMENT | ImageUsage::TRANSFER_SRC,
                Extent3d::d2(63, 4),
            ))
            .expect("a 63-texel-wide image");
        let wide = device
            .create_image(&image(
                Format::Rgba8Unorm,
                ImageUsage::COLOR_ATTACHMENT | ImageUsage::TRANSFER_SRC,
                TARGET,
            ))
            .expect("a 64-texel-wide image");
        let readback = readback_buffer(&device, TARGET_BYTES * 2);

        type Case = (&'static str, &'static str, BufferImageCopy);
        let cases: Vec<Case> = vec![
            (
                "an unaligned row pitch",
                "row pitch",
                BufferImageCopy {
                    image_extent: Extent3d::d2(63, 4),
                    ..whole_image_copy(readback, 0, narrow)
                },
            ),
            (
                "an unaligned buffer offset",
                "multiple of",
                whole_image_copy(readback, 4, wide),
            ),
            (
                "a region past the mip",
                "runs past mip",
                BufferImageCopy {
                    image_offset: Offset3d { x: 4, y: 0, z: 0 },
                    ..whole_image_copy(readback, 0, wide)
                },
            ),
            (
                "a mip the image does not have",
                "mips",
                BufferImageCopy {
                    image_subresource: ImageSubresourceLayers {
                        aspect: ImageAspect::COLOR,
                        mip: 3,
                        base_layer: 0,
                        layer_count: 1,
                    },
                    ..whole_image_copy(readback, 0, wide)
                },
            ),
            (
                "a buffer too small for the region",
                "byte buffer",
                whole_image_copy(readback, (TARGET_BYTES + 512) as u64, wide),
            ),
        ];
        assert!(!cases.is_empty(), "nothing to check");
        for (what, fragment, copy) in cases {
            let mut encoder =
                device.create_command_encoder(&CommandEncoderDesc { label: None, queue });
            encoder.copy_image_to_buffer(&copy);
            let error = encoder
                .finish()
                .err()
                .unwrap_or_else(|| panic!("{what} was accepted"));
            let HalError::InvalidDescriptor(text) = error else {
                panic!("{what}: expected InvalidDescriptor, got {error:?}");
            };
            assert!(
                text.contains(fragment),
                "{what}: the refusal must name the rule; got {text}"
            );
        }

        // And the aligned copy of the same image really is accepted, so the
        // refusals above are about the layout and not about the image.
        let mut encoder = device.create_command_encoder(&CommandEncoderDesc { label: None, queue });
        encoder.copy_image_to_buffer(&whole_image_copy(readback, 0, wide));
        let accepted = encoder
            .finish()
            .expect("a 256-byte-pitch copy at offset zero is exactly what D3D12 wants");
        device.destroy_command_buffer(accepted);

        device.destroy_buffer(readback);
        device.destroy_image(wide);
        device.destroy_image(narrow);
    }

    /// **The image-to-image copies D3D12 cannot express are refused by name.**
    ///
    /// The happy path is driven on this same runner by
    /// `crates/crcbl/tests/hal_seam_e2e.rs`'s `exercise_image_to_image_copy`,
    /// which reads the destination back and compares it texel for texel — so
    /// what is left uncovered, and what this covers, is every pair the seam lets
    /// a caller write and `CopyTextureRegion` would either reject or perform
    /// wrongly. Each is a caller bug rather than a missing slice, so each is
    /// [`HalError::InvalidDescriptor`] naming the rule it broke.
    ///
    /// **Nothing here reaches the debug layer**, unlike
    /// `a_copy_d3d12_cannot_place_is_refused_by_name`: every refusal below is
    /// this crate's, decided before a D3D12 call is made, so the teardown
    /// assertion stands.
    #[test]
    #[ignore = "needs a real D3D12 device; run tests/run-dx12-e2e.sh"]
    fn an_image_to_image_copy_d3d12_cannot_express_is_refused_by_name() {
        let (_instance, device) = open_device();
        let queue = device
            .queue(QueueKind::Graphics)
            .expect("the graphics queue exists");
        let transfer = ImageUsage::TRANSFER_SRC | ImageUsage::TRANSFER_DST;
        let source = device
            .create_image(&image(Format::Rgba8Unorm, transfer, TARGET))
            .expect("a transfer-only colour image");
        let destination = device
            .create_image(&image(Format::Rgba8Unorm, transfer, TARGET))
            .expect("a second one");
        let other_format = device
            .create_image(&image(Format::Bgra8Unorm, transfer, TARGET))
            .expect("the same extent in another format");
        let two_deep = Extent3d {
            width: TARGET.width,
            height: TARGET.height,
            depth_or_layers: 2,
        };
        let volume = device
            .create_image(&ImageDesc {
                image_type: ImageType::D3,
                extent: two_deep,
                ..image(Format::Rgba8Unorm, transfer, TARGET)
            })
            .expect("a volume of the same footprint");
        let array = device
            .create_image(&image(Format::Rgba8Unorm, transfer, two_deep))
            .expect("a two-layer array of the same footprint");

        let layers = ImageSubresourceLayers {
            aspect: ImageAspect::COLOR,
            mip: 0,
            base_layer: 0,
            layer_count: 1,
        };
        let whole = ImageCopy {
            src: source,
            src_subresource: layers,
            src_offset: Offset3d::default(),
            dst: destination,
            dst_subresource: layers,
            dst_offset: Offset3d::default(),
            extent: TARGET,
        };

        type Case = (&'static str, &'static str, ImageCopy);
        let cases: Vec<Case> = vec![
            (
                "two different formats",
                "same format",
                ImageCopy {
                    dst: other_format,
                    ..whole
                },
            ),
            (
                "a volume and a flat image",
                "not the same region",
                ImageCopy {
                    dst: volume,
                    ..whole
                },
            ),
            (
                "one subresource on both sides",
                "same subresource",
                ImageCopy {
                    dst: source,
                    ..whole
                },
            ),
            (
                "a mip the destination does not have",
                "mips",
                ImageCopy {
                    dst_subresource: ImageSubresourceLayers { mip: 3, ..layers },
                    ..whole
                },
            ),
            (
                "a source region past the mip",
                "runs past mip",
                ImageCopy {
                    src_offset: Offset3d { x: 4, y: 0, z: 0 },
                    ..whole
                },
            ),
            (
                "more layers written than read",
                "array layers",
                ImageCopy {
                    dst: array,
                    dst_subresource: ImageSubresourceLayers {
                        layer_count: 2,
                        ..layers
                    },
                    ..whole
                },
            ),
            (
                "a plane a colour format does not have",
                "exactly one plane",
                ImageCopy {
                    dst_subresource: ImageSubresourceLayers {
                        aspect: ImageAspect::DEPTH,
                        ..layers
                    },
                    ..whole
                },
            ),
        ];
        assert!(!cases.is_empty(), "nothing to check");
        for (what, fragment, copy) in cases {
            let mut encoder =
                device.create_command_encoder(&CommandEncoderDesc { label: None, queue });
            encoder.copy_image_to_image(&copy);
            let error = encoder
                .finish()
                .err()
                .unwrap_or_else(|| panic!("{what} was accepted"));
            let HalError::InvalidDescriptor(text) = error else {
                panic!("{what}: expected InvalidDescriptor, got {error:?}");
            };
            assert!(
                text.contains(fragment),
                "{what}: the refusal must name the rule; got {text}"
            );
        }

        // And the well-formed copy of the same two images records, so none of
        // the refusals above is this entry point refusing everything.
        let mut encoder = device.create_command_encoder(&CommandEncoderDesc { label: None, queue });
        encoder.copy_image_to_image(&whole);
        let accepted = encoder
            .finish()
            .expect("two subresources of one format and one sample count is the copy D3D12 takes");
        device.destroy_command_buffer(accepted);

        device.destroy_image(array);
        device.destroy_image(volume);
        device.destroy_image(other_format);
        device.destroy_image(destination);
        device.destroy_image(source);
    }

    /// An encoder built on another device's queue refuses at `finish`, which is
    /// the first call it has to refuse through.
    ///
    /// `create_command_encoder` returns a bare `Box`, so this is the deferred
    /// failure path end to end — and the error is `ForeignObject` rather than a
    /// missing slice, because the caller crossed two devices.
    #[test]
    #[ignore = "needs a real D3D12 device; run tests/run-dx12-e2e.sh"]
    fn a_d3d12_encoder_built_on_a_foreign_queue_refuses_at_finish() {
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

        // A hand-made queue handle carries no device tag at all.
        let encoder = device.create_command_encoder(&CommandEncoderDesc {
            label: None,
            queue: unissued(),
        });
        let error = encoder.finish().expect_err("nobody issued that queue");
        assert!(
            matches!(error, HalError::InvalidHandle { kind, .. } if kind == "queue"),
            "{error:?}"
        );
    }

    /// An encoder that mis-nests or leaves a pass open refuses, rather than
    /// handing back a command buffer D3D12 would run half of.
    #[test]
    #[ignore = "needs a real D3D12 device; run tests/run-dx12-e2e.sh"]
    fn a_mis_nested_or_unclosed_pass_refuses_at_finish() {
        let (_instance, device) = open_device();
        let queue = device
            .queue(QueueKind::Graphics)
            .expect("the graphics queue exists");
        let (target, view) = color_target(&device);
        let pass = clear_pass(
            view,
            CLEAR,
            LoadOp::Clear,
            Rect2d::from_size(TARGET.width, TARGET.height),
        );

        let mut encoder = device.create_command_encoder(&CommandEncoderDesc { label: None, queue });
        encoder.begin_render_pass(&pass.desc());
        let error = encoder
            .finish()
            .expect_err("a pass left open is a command buffer nobody can submit");
        assert!(matches!(error, HalError::InvalidDescriptor(_)), "{error:?}");

        let mut encoder = device.create_command_encoder(&CommandEncoderDesc { label: None, queue });
        encoder.begin_render_pass(&pass.desc());
        encoder.begin_render_pass(&pass.desc());
        encoder.end_render_pass();
        let error = encoder.finish().expect_err("passes do not nest");
        assert!(matches!(error, HalError::InvalidDescriptor(_)), "{error:?}");

        // A copy inside a pass is the other side of the same rule.
        let readback = readback_buffer(&device, TARGET_BYTES);
        let mut encoder = device.create_command_encoder(&CommandEncoderDesc { label: None, queue });
        encoder.begin_render_pass(&pass.desc());
        encoder.copy_image_to_buffer(&whole_image_copy(readback, 0, target));
        encoder.end_render_pass();
        let error = encoder.finish().expect_err("a copy belongs between passes");
        assert!(matches!(error, HalError::InvalidDescriptor(_)), "{error:?}");

        // And a well-formed pass on the same objects still finishes, so none of
        // the three refusals is the encoder simply refusing everything.
        let mut encoder = device.create_command_encoder(&CommandEncoderDesc { label: None, queue });
        encoder.begin_render_pass(&pass.desc());
        encoder.end_render_pass();
        let buffer = encoder.finish().expect("a closed pass is a command buffer");
        device.destroy_command_buffer(buffer);

        device.destroy_buffer(readback);
        device.destroy_image_view(view);
        device.destroy_image(target);
    }

    /// A destroyed command buffer stops resolving, so a second submission of it
    /// is a stale handle rather than a second execution.
    #[test]
    #[ignore = "needs a real D3D12 device; run tests/run-dx12-e2e.sh"]
    fn a_destroyed_command_buffer_stops_resolving() {
        let (_instance, device) = open_device();
        let queue = device
            .queue(QueueKind::Graphics)
            .expect("the graphics queue exists");
        let encoder = device.create_command_encoder(&CommandEncoderDesc { label: None, queue });
        let buffer = encoder.finish().expect("an empty command buffer is legal");
        device
            .submit(queue, &SubmitInfo::new(&[buffer]))
            .expect("an empty command buffer submits");
        device.wait_idle().expect("it completes");

        device.destroy_command_buffer(buffer);
        let error = device
            .submit(queue, &SubmitInfo::new(&[buffer]))
            .expect_err("the handle was destroyed");
        assert!(
            matches!(error, HalError::InvalidHandle { kind, .. } if kind == "command buffer"),
            "{error:?}"
        );
    }

    /// A readback names a range of a `HostReadback` buffer, and every other
    /// shape is refused by name.
    ///
    /// The two rejected memory locations are the point: D3D12 will map an
    /// upload buffer, so a backend that only checked "is it mappable" would
    /// accept one and hand back write-combined memory the CPU reads at a crawl
    /// and the GPU never wrote.
    #[test]
    #[ignore = "needs a real D3D12 device; run tests/run-dx12-e2e.sh"]
    fn a_readback_refuses_the_buffers_and_ranges_it_cannot_serve() {
        let (_instance, device) = open_device();
        let readback = readback_buffer(&device, 64);
        let private = device
            .create_buffer(&buffer(64, MemoryLocation::DeviceLocal))
            .expect("a device-local buffer");
        let upload = device
            .create_buffer(&buffer(64, MemoryLocation::HostUpload))
            .expect("an upload buffer");

        let refusals: Vec<(&str, BufferHandle, u64, u64)> = vec![
            ("a device-local buffer", private, 0, 4),
            ("an upload buffer", upload, 0, 4),
            ("a range past the end", readback, 48, 32),
            ("a range that overflows", readback, u64::MAX, 8),
        ];
        assert!(!refusals.is_empty(), "nothing to check");
        for (what, handle, offset, size) in refusals {
            let error = device
                .request_readback(&ReadbackDesc {
                    label: None,
                    buffer: handle,
                    offset,
                    size,
                    after: None,
                })
                .err()
                .unwrap_or_else(|| panic!("{what} was accepted"));
            assert!(
                matches!(error, HalError::InvalidDescriptor(_)),
                "{what}: {error:?}"
            );
        }

        // The readback buffer itself is accepted, so the refusals are about the
        // location and the range rather than about readbacks.
        let request = device
            .request_readback(&ReadbackDesc {
                label: None,
                buffer: readback,
                offset: 0,
                size: 64,
                after: None,
            })
            .expect("a HostReadback buffer is what a readback is for");
        // The output length is the contract, not a hint: a short slice would
        // otherwise be filled with a prefix the caller reads as the whole thing.
        let mut wrong = [0u8; 8];
        let error = device
            .poll_readback(request, &mut wrong)
            .expect_err("8 bytes is not 64");
        assert!(matches!(error, HalError::InvalidDescriptor(_)), "{error:?}");
        assert_eq!(
            drain(&device, request, 64),
            vec![POISON; 64],
            "nothing was submitted, so the readback is what write_buffer left"
        );

        // A destroyed readback stops resolving.
        device.destroy_readback(request);
        let error = device
            .poll_readback(request, &mut [0u8; 64])
            .expect_err("the readback was destroyed");
        assert!(
            matches!(error, HalError::InvalidHandle { kind, .. } if kind == "readback"),
            "{error:?}"
        );

        device.destroy_buffer(upload);
        device.destroy_buffer(private);
        device.destroy_buffer(readback);
    }

    /// The device opens, says which backend it is, and has exactly the queue
    /// this backend creates.
    #[test]
    #[ignore = "needs a real D3D12 device; run tests/run-dx12-e2e.sh"]
    fn a_device_reports_dx12_and_one_graphics_queue() {
        let (_instance, device) = open_device();
        assert_eq!(device.backend(), BackendKind::Dx12);
        assert!(
            device.queue(QueueKind::Graphics).is_some(),
            "every device creates a DIRECT queue"
        );
        assert!(
            device.queue(QueueKind::Compute).is_none(),
            "no COMPUTE queue is created while ASYNC_COMPUTE_QUEUE is unreported"
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

    /// `caps` is the adapter's caps: D3D12 enables nothing and disables nothing,
    /// so a device that reported less than its adapter would be lying about
    /// hardware it can use.
    ///
    /// **Every limit, with nothing normalised.** The timestamp clock's rate used
    /// to be the one exception — an adapter has no queue to ask
    /// `GetTimestampFrequency` — and it no longer crosses the seam at all, so
    /// there is no field to exclude and this compares the whole struct.
    #[test]
    #[ignore = "needs a real D3D12 device; run tests/run-dx12-e2e.sh"]
    fn d3d12_device_caps_match_the_adapter_they_came_from() {
        let instance = open_instance();
        let adapter = pinned_adapter(&instance);
        let info = instance
            .adapters()
            .into_iter()
            .find(|info| info.id == adapter)
            .expect("the pin resolved against this same enumeration");
        let device = instance
            .open_device(&device_desc(adapter))
            .expect("a D3D12 device opens with no required features");
        assert_eq!(device.caps().features, info.caps.features);
        assert!(
            info.caps.features.contains(Features::TIMESTAMP_QUERY),
            "an adapter that stopped reporting timestamps would make every timestamp \
             assertion in this suite vacuous: {:?}",
            info.caps.features
        );
        assert_eq!(device.caps().limits, info.caps.limits);
    }

    /// A buffer of every memory location creates and destroys, and a destroyed
    /// handle fails **as a stale handle** rather than as anything else.
    ///
    /// The variant matters and is the reason this asserts on it: a
    /// `DeviceLocal` buffer that is still alive fails `write_buffer` with
    /// `InvalidDescriptor`, so "it returned an error" would pass whether or not
    /// the handle was ever invalidated.
    #[test]
    #[ignore = "needs a real D3D12 device; run tests/run-dx12-e2e.sh"]
    fn d3d12_buffers_of_every_memory_location_create_and_then_stop_resolving() {
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
        let error = device
            .create_buffer(&buffer(0, MemoryLocation::HostUpload))
            .expect_err("a zero-byte buffer is not a buffer");
        assert!(matches!(error, HalError::InvalidDescriptor(_)), "{error:?}");
    }

    /// `write_buffer` writes, at the offset it was given, and refuses the
    /// location D3D12 cannot map.
    #[test]
    #[ignore = "needs a real D3D12 device; run tests/run-dx12-e2e.sh"]
    fn write_buffer_writes_host_visible_memory_and_refuses_what_d3d12_cannot_map() {
        let (_instance, device) = open_device();
        let readback = device
            .create_buffer(&buffer(16, MemoryLocation::HostReadback))
            .expect("a readback buffer");

        // Two writes, so the result is fully determined whatever D3D12 left in
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
            .expect_err("a default-heap buffer cannot be mapped");
        let HalError::InvalidDescriptor(text) = error else {
            panic!("expected InvalidDescriptor, got {error:?}");
        };
        assert!(text.contains("DeviceLocal"), "{text}");
        assert!(
            text.contains("copy"),
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
    #[ignore = "needs a real D3D12 device; run tests/run-dx12-e2e.sh"]
    fn d3d12_images_views_and_samplers_create_and_destroy() {
        let (_instance, device) = open_device();
        let extent = Extent3d::d2(64, 64);
        let handle = device
            .create_image(&ImageDesc {
                mip_levels: extent.full_mip_levels(ImageType::D2),
                ..image(
                    Format::Rgba8Unorm,
                    ImageUsage::SAMPLED | ImageUsage::COLOR_ATTACHMENT,
                    extent,
                )
            })
            .expect("a 2D colour image");

        let view = device
            .create_image_view(&whole(handle, Format::Rgba8Unorm))
            .expect("a whole-image view");

        // One mip of the chain, which is the case the ALL sentinel resolution
        // has to get right.
        let one_mip = device
            .create_image_view(&ImageViewDesc {
                range: ImageSubresourceRange {
                    aspect: ImageAspect::COLOR,
                    base_mip: 1,
                    mip_count: 1,
                    base_layer: 0,
                    layer_count: 1,
                },
                ..whole(handle, Format::Rgba8Unorm)
            })
            .expect("a single-mip view");
        assert_ne!(view, one_mip);

        let sampler = device
            .create_sampler(&SamplerDesc {
                label: Some("crcbl-dx12 test sampler"),
                ..SamplerDesc::default()
            })
            .expect("the seam's default sampler");

        device.destroy_image_view(one_mip);
        device.destroy_image_view(view);
        device.destroy_sampler(sampler);
        device.destroy_image(handle);

        let error = device
            .create_image_view(&whole(handle, Format::Rgba8Unorm))
            .expect_err("the image was destroyed");
        assert!(
            matches!(error, HalError::InvalidHandle { kind, .. } if kind == "image"),
            "{error:?}"
        );
    }

    /// **Every descriptor a view took goes back when the view is destroyed.**
    ///
    /// The key is the descriptor *address*, which is what a leak moves and a
    /// correct free does not: creating and destroying the same view repeatedly
    /// must keep handing back the same slot. Counting views would pass whether
    /// or not the heap slots were ever returned, and the heap grows a chunk at a
    /// time, so a leak of a handful would not even show up as a failure to
    /// allocate.
    #[test]
    #[ignore = "needs a real D3D12 device; run tests/run-dx12-e2e.sh"]
    fn destroying_a_view_returns_every_descriptor_it_took() {
        let (_instance, device) = open_device();
        // Sampled *and* rendered to, so the view really owns two descriptors in
        // two different heaps — the case a free that only released one leaves
        // behind.
        let handle = device
            .create_image(&image(
                Format::Rgba8Unorm,
                ImageUsage::SAMPLED | ImageUsage::COLOR_ATTACHMENT,
                Extent3d::d2(16, 16),
            ))
            .expect("a colour image");

        let mut addresses = Vec::new();
        for round in 0..4 {
            let view = device
                .create_image_view(&whole(handle, Format::Rgba8Unorm))
                .unwrap_or_else(|error| panic!("round {round}: {error:?}"));
            {
                let mut state = device.state();
                let local = handle::local::<ViewEntry, _>("view", view, device.inner.owner)
                    .expect("this device's own handle");
                let entry = state
                    .views
                    .get(local)
                    .expect("the view is live")
                    .descriptors;
                let srv = entry
                    .shader_resource
                    .expect("a SAMPLED image view owns a shader resource view");
                let rtv = entry
                    .render_target
                    .expect("a COLOR_ATTACHMENT image view owns a render target view");
                addresses.push((
                    state.descriptors.cpu_handle(srv).ptr,
                    state.descriptors.cpu_handle(rtv).ptr,
                ));
            }
            device.destroy_image_view(view);
        }
        assert_eq!(addresses.len(), 4, "the loop did not run");
        let first = addresses[0];
        for (round, pair) in addresses.iter().enumerate() {
            assert_eq!(
                *pair, first,
                "round {round} took a different descriptor pair, so the previous round's was \
                 never freed"
            );
        }
        device.destroy_image(handle);
    }

    /// A sampler's descriptor comes back too, by the same test.
    #[test]
    #[ignore = "needs a real D3D12 device; run tests/run-dx12-e2e.sh"]
    fn destroying_a_sampler_returns_its_descriptor() {
        let (_instance, device) = open_device();
        let mut addresses = Vec::new();
        for round in 0..4 {
            let sampler = device
                .create_sampler(&SamplerDesc::default())
                .unwrap_or_else(|error| panic!("round {round}: {error:?}"));
            {
                let mut state = device.state();
                let local =
                    handle::local::<SamplerEntry, _>("sampler", sampler, device.inner.owner)
                        .expect("this device's own handle");
                let slot = state.samplers.get(local).expect("the sampler is live").slot;
                addresses.push(state.descriptors.cpu_handle(slot).ptr);
            }
            device.destroy_sampler(sampler);
        }
        assert_eq!(addresses.len(), 4, "the loop did not run");
        assert!(
            addresses.iter().all(|&ptr| ptr == addresses[0]),
            "a sampler descriptor was not freed: {addresses:?}"
        );
    }

    /// A sampled depth image is stored typeless and read through the depth
    /// plane's own format, which is what makes a shadow map creatable at all.
    ///
    /// The falsifying value is a backend that stored `D32_FLOAT`: D3D12 refuses
    /// a shader resource view on a fully-typed depth resource, and refuses it by
    /// writing nothing and returning `void`, so this would pass creation and
    /// sample black.
    #[test]
    #[ignore = "needs a real D3D12 device; run tests/run-dx12-e2e.sh"]
    fn a_sampled_depth_image_gets_both_of_its_views() {
        let (_instance, device) = open_device();
        let handle = device
            .create_image(&image(
                Format::D32Float,
                ImageUsage::SAMPLED | ImageUsage::DEPTH_STENCIL_ATTACHMENT,
                Extent3d::d2(32, 32),
            ))
            .expect("a sampled depth image");

        let view = device
            .create_image_view(&whole(handle, Format::D32Float))
            .expect("a depth view of a depth image");
        {
            let state = device.state();
            let local = handle::local::<ViewEntry, _>("view", view, device.inner.owner)
                .expect("this device's own handle");
            let descriptors = state
                .views
                .get(local)
                .expect("the view is live")
                .descriptors;
            assert!(
                descriptors.shader_resource.is_some(),
                "a SAMPLED depth image must get a shader resource view"
            );
            assert!(
                descriptors.depth_stencil.is_some(),
                "a DEPTH_STENCIL_ATTACHMENT image must get a depth stencil view"
            );
            assert!(descriptors.render_target.is_none());
            assert!(descriptors.unordered_access.is_none());
        }
        device.destroy_image_view(view);
        device.destroy_image(handle);
    }

    /// The image descriptors D3D12 cannot satisfy are refused before anything is
    /// created.
    #[test]
    #[ignore = "needs a real D3D12 device; run tests/run-dx12-e2e.sh"]
    fn images_d3d12_cannot_create_are_refused_by_name() {
        let (_instance, device) = open_device();
        let base = image(
            Format::Rgba8Unorm,
            ImageUsage::SAMPLED,
            Extent3d::d2(16, 16),
        );

        let refusals: Vec<(&str, ImageDesc<'_>)> = vec![
            (
                "a zero extent",
                ImageDesc {
                    extent: Extent3d::d2(0, 16),
                    ..base
                },
            ),
            (
                "no usage at all",
                ImageDesc {
                    usage: ImageUsage::empty(),
                    ..base
                },
            ),
            (
                "a sample count that is not a power of two",
                ImageDesc { samples: 3, ..base },
            ),
            (
                "a multisampled mip chain",
                ImageDesc {
                    samples: 4,
                    mip_levels: 4,
                    ..base
                },
            ),
            (
                "an extent past the device's ceiling",
                ImageDesc {
                    extent: Extent3d::d2(device.caps().limits.max_image_2d + 1, 16),
                    ..base
                },
            ),
        ];
        assert!(!refusals.is_empty(), "nothing to check");
        for (what, desc) in refusals {
            let error = device
                .create_image(&desc)
                .err()
                .unwrap_or_else(|| panic!("{what} was accepted"));
            assert!(
                matches!(error, HalError::InvalidDescriptor(_)),
                "{what}: {error:?}"
            );
        }
    }

    /// A view format that differs from its image's is refused, with the reason
    /// in the message — this backend's one documented divergence from
    /// `crcbl-mtl`.
    #[test]
    #[ignore = "needs a real D3D12 device; run tests/run-dx12-e2e.sh"]
    fn a_differing_view_format_is_refused_and_says_why() {
        let (_instance, device) = open_device();
        let handle = device
            .create_image(&image(
                Format::Rgba8Unorm,
                ImageUsage::SAMPLED,
                Extent3d::d2(32, 32),
            ))
            .expect("a linear image");

        let error = device
            .create_image_view(&ImageViewDesc {
                format: Format::Rgba8UnormSrgb,
                ..whole(handle, Format::Rgba8Unorm)
            })
            .expect_err("D3D12 needs a typeless resource to cast between fully typed formats");
        let HalError::InvalidDescriptor(text) = error else {
            panic!("expected InvalidDescriptor, got {error:?}");
        };
        assert!(text.contains("typeless"), "{text}");

        // The same-format view is still fine, so the refusal is about the cast
        // and not about the image.
        let view = device
            .create_image_view(&whole(handle, Format::Rgba8Unorm))
            .expect("a same-format view");
        device.destroy_image_view(view);
        device.destroy_image(handle);
    }

    /// An image with only transfer usage has no D3D12 view, and says so rather
    /// than handing back a view that names nothing.
    #[test]
    #[ignore = "needs a real D3D12 device; run tests/run-dx12-e2e.sh"]
    fn a_transfer_only_image_has_no_view_to_create() {
        let (_instance, device) = open_device();
        let handle = device
            .create_image(&image(
                Format::Rgba8Unorm,
                ImageUsage::TRANSFER_SRC | ImageUsage::TRANSFER_DST,
                Extent3d::d2(16, 16),
            ))
            .expect("a transfer-only image");
        let error = device
            .create_image_view(&whole(handle, Format::Rgba8Unorm))
            .expect_err("there is no descriptor kind for a transfer-only image");
        assert!(matches!(error, HalError::InvalidDescriptor(_)), "{error:?}");
        device.destroy_image(handle);
    }

    /// A view whose shape does not match its image's is refused, because D3D12
    /// would write it and say nothing.
    ///
    /// The two cases are the two silent losses: a 3D view of a 2D image is a
    /// descriptor the runtime ignores, and a non-array view starting at a
    /// non-zero layer has no field to carry the layer, so it would quietly view
    /// layer zero.
    #[test]
    #[ignore = "needs a real D3D12 device; run tests/run-dx12-e2e.sh"]
    fn a_view_whose_shape_disagrees_with_its_image_is_refused() {
        let (_instance, device) = open_device();
        let layered = device
            .create_image(&image(
                Format::Rgba8Unorm,
                ImageUsage::SAMPLED,
                Extent3d {
                    width: 32,
                    height: 32,
                    depth_or_layers: 6,
                },
            ))
            .expect("a six-layer 2D image");

        let wrong_shape = device
            .create_image_view(&ImageViewDesc {
                view_type: ImageViewType::D3,
                ..whole(layered, Format::Rgba8Unorm)
            })
            .expect_err("a volume view of a layered 2D image is not a D3D12 view");
        assert!(
            matches!(wrong_shape, HalError::InvalidDescriptor(_)),
            "{wrong_shape:?}"
        );

        let dropped_layer = device
            .create_image_view(&ImageViewDesc {
                view_type: ImageViewType::D2,
                range: ImageSubresourceRange {
                    base_layer: 3,
                    layer_count: 1,
                    ..ImageSubresourceRange::all(Format::Rgba8Unorm)
                },
                ..whole(layered, Format::Rgba8Unorm)
            })
            .expect_err("a D2 view has no first-slice field to carry layer 3");
        let HalError::InvalidDescriptor(text) = dropped_layer else {
            panic!("expected InvalidDescriptor, got {dropped_layer:?}");
        };
        assert!(
            text.contains("D2Array"),
            "the refusal must say the fix: {text}"
        );

        // The array view of the same layer really is accepted, so the two
        // refusals above are about the shape and not about the image.
        let view = device
            .create_image_view(&ImageViewDesc {
                view_type: ImageViewType::D2Array,
                range: ImageSubresourceRange {
                    base_layer: 3,
                    layer_count: 1,
                    ..ImageSubresourceRange::all(Format::Rgba8Unorm)
                },
                ..whole(layered, Format::Rgba8Unorm)
            })
            .expect("an array view can start where it likes");
        device.destroy_image_view(view);

        // And a cube view of the same six layers, which is the case the seam has
        // a view type for and no image type.
        let cube = device
            .create_image_view(&ImageViewDesc {
                view_type: ImageViewType::Cube,
                ..whole(layered, Format::Rgba8Unorm)
            })
            .expect("six layers are a cube");
        device.destroy_image_view(cube);
        device.destroy_image(layered);
    }

    /// Anisotropy is bounded on both sides, and a point filter beside it is
    /// refused rather than silently dropped.
    #[test]
    #[ignore = "needs a real D3D12 device; run tests/run-dx12-e2e.sh"]
    fn d3d12_samplers_reject_anisotropy_outside_the_reported_cap() {
        let (_instance, device) = open_device();
        let cap = device.caps().limits.max_sampler_anisotropy;
        assert!(
            cap > 1.0,
            "this backend reports SAMPLER_ANISOTROPY, so the cap must exceed the value that \
             disables it"
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

        let point_with_anisotropy = device.create_sampler(&SamplerDesc {
            anisotropy: cap,
            mip_filter: FilterMode::Nearest,
            ..SamplerDesc::default()
        });
        assert!(
            matches!(point_with_anisotropy, Err(HalError::InvalidDescriptor(_))),
            "D3D12 has no point-filtered anisotropic sampler: {point_with_anisotropy:?}"
        );

        // And a comparison sampler at the cap is accepted — the shadow-map
        // shape, with the reversed-Z comparison the seam documents.
        let sampler = device
            .create_sampler(&SamplerDesc {
                label: Some("shadow"),
                anisotropy: cap,
                compare: Some(CompareOp::Greater),
                ..SamplerDesc::default()
            })
            .expect("a comparison sampler at the reported cap");
        device.destroy_sampler(sampler);
    }

    /// `wait_idle` really waits: it signals the queue's fence and blocks on it.
    /// A queue or fence that could not do either fails here.
    #[test]
    #[ignore = "needs a real D3D12 device; run tests/run-dx12-e2e.sh"]
    fn wait_idle_signals_the_queue_fence_and_waits_for_it() {
        let (_instance, device) = open_device();
        device
            .wait_idle()
            .expect("an empty queue reaches the value");
        // Twice, because a fence that only works once is a fence whose value
        // never advanced.
        device.wait_idle().expect("and again");
        // The value really moved, which is what makes the second wait a wait
        // rather than a repeat of the first.
        // SAFETY: `idle_fence` is live and `GetCompletedValue` returns a `u64`
        // by value.
        let completed = unsafe { device.inner.fence.GetCompletedValue() };
        assert!(
            completed >= 2,
            "two waits left the fence at {completed}, so at least one did not signal"
        );
    }

    /// Concurrent `wait_idle`s all return, and the fence ends where the count
    /// says it should.
    ///
    /// **What this does and does not prove.** The final value pins the
    /// mechanism: every call signalled exactly once, so a reservation that
    /// skipped or repeated a value fails here deterministically. The *ordering*
    /// half — that the signals reach the queue in increasing order, which is
    /// what stops a waiter arming an event for a value the fence has already
    /// passed and dropped below — is a race, and a passing run is not evidence
    /// the race cannot happen. Its failure mode is a hang rather than an
    /// assertion, caught by `slow-timeout` in `.config/nextest.toml`.
    #[test]
    #[ignore = "needs a real D3D12 device; run tests/run-dx12-e2e.sh"]
    fn concurrent_waits_each_signal_once_and_all_return() {
        let (_instance, device) = open_device();
        let before = {
            // SAFETY: `idle_fence` is live and `GetCompletedValue` returns a
            // `u64` by value.
            unsafe { device.inner.fence.GetCompletedValue() }
        };

        let waiters: u64 = 8;
        std::thread::scope(|scope| {
            for index in 0..waiters {
                let device = &device;
                scope.spawn(move || {
                    device
                        .wait_idle()
                        .unwrap_or_else(|error| panic!("waiter {index}: {error:?}"));
                });
            }
        });

        // SAFETY: as above.
        let after = unsafe { device.inner.fence.GetCompletedValue() };
        assert_eq!(
            after,
            before + waiters,
            "the fence moved by {} across {waiters} waits, so a value was skipped or reused",
            after - before
        );
    }

    /// **A queue wait for a value nothing has signalled blocks, and a CPU signal
    /// releases it** — [`Capability::TimelineWaitBeforeSignal`], and the one
    /// synchronisation claim `crates/crcbl/tests/hal_seam_e2e.rs` cannot make.
    ///
    /// It cannot make it because the failure mode is a *hang*: a backend that
    /// deadlocked rather than refusing would take the shared suite down with it,
    /// so that suite leaves the capability unexercised and the evidence has to
    /// live here — where a test can reach `ID3D12Fence::Signal` directly, which
    /// is the CPU-side signal the seam has no verb for and `crcbl-mtl`'s
    /// divergence entry names as the thing it is missing.
    ///
    /// **Both halves are asserted, and the first is what makes this a test
    /// rather than a demonstration.** After the wait-only submission the device
    /// fence must still be short of the value that submission reserved: a
    /// `Wait` the queue never received would let it through at once, and the
    /// signal below would then be releasing something that was never held.
    #[test]
    #[ignore = "needs a real D3D12 device; run tests/run-dx12-e2e.sh"]
    fn a_queue_wait_for_an_unsignalled_value_blocks_until_the_cpu_signals_it() {
        /// What the submission waits for. Above the semaphore's initial value,
        /// so nothing anywhere has reached it.
        const AWAITED: u64 = 7;
        /// How long the queue is watched for the wait it must be honouring.
        /// Only the "the wait was dropped" direction is timing-dependent, and
        /// that direction cannot fail spuriously: an honoured wait can never
        /// let the fence through, however long this is.
        const HELD: Duration = Duration::from_millis(50);

        let (_instance, device) = open_device();
        let queue = device
            .queue(QueueKind::Graphics)
            .expect("the graphics queue exists");
        let semaphore = device
            .create_semaphore(&SemaphoreDesc {
                label: Some("wait before signal"),
                kind: SemaphoreKind::Timeline { initial_value: 0 },
            })
            .expect("every D3D12 device creates fences");
        // The fence itself, so this test can play the part no seam call has: a
        // signal that does not come from a submission.
        let raw = {
            let state = device.state();
            handle::lookup(
                &state.semaphores,
                "semaphore",
                semaphore,
                device.inner.owner,
            )
            .expect("the semaphore was created a moment ago")
            .raw
            .clone()
        };

        device
            .submit(
                queue,
                &SubmitInfo {
                    command_buffers: &[],
                    waits: &[SemaphoreWait {
                        semaphore,
                        value: AWAITED,
                    }],
                    signals: &[],
                },
            )
            .expect("a wait on an unsignalled value is accepted, not refused");
        let reserved = device.state().next_fence_value;

        std::thread::sleep(HELD);
        // SAFETY: the device fence is live and `GetCompletedValue` returns a
        // `u64` by value.
        let completed = unsafe { device.inner.fence.GetCompletedValue() };
        assert!(
            completed < reserved,
            "the queue reached fence {completed} while waiting on a semaphore value nothing had \
             signalled, so the Wait was dropped rather than issued"
        );

        // SAFETY: `raw` is the semaphore's live fence, held by this test for the
        // duration of the call, and `Signal` takes a scalar.
        unsafe { raw.Signal(AWAITED) }.expect("the CPU side of an ID3D12Fence");
        device
            .wait_idle()
            .expect("the CPU signal released the queue");
        assert_eq!(
            device
                .semaphore_value(semaphore)
                .expect("a timeline has one"),
            AWAITED,
            "the semaphore did not end at the value the CPU signalled"
        );
        device.destroy_semaphore(semaphore);
    }

    /// **A CPU wait that runs out of time is `Ok(false)`, and a binary semaphore
    /// has no CPU side at all.**
    ///
    /// The seam calls a timeout "a normal outcome for a frame-pacing poll, not
    /// an error". A backend that answered `Err` there would compile, pass every
    /// other test in this file, and turn a pacing poll into a lost device — so
    /// this is what separates `WAIT_TIMEOUT` from the failure codes beside it,
    /// and it goes red if they are ever folded together.
    ///
    /// The already-reached wait is asserted first for the reason the timeout
    /// exists: a `wait_semaphores` that answered `Ok(false)` unconditionally
    /// would satisfy the timeout assertion on its own.
    #[test]
    #[ignore = "needs a real D3D12 device; run tests/run-dx12-e2e.sh"]
    fn a_cpu_wait_times_out_as_ok_false_and_a_binary_semaphore_has_no_cpu_side() {
        /// What the semaphore is created holding, so "already reached" is a
        /// value rather than the zero every fence starts at.
        const INITIAL: u64 = 3;
        /// A value nothing will ever signal onto it.
        const NEVER: u64 = INITIAL + 1;
        /// Long enough to be a real wait, short enough not to pace the suite.
        const BUDGET_NS: u64 = 2_000_000;

        let (_instance, device) = open_device();
        let timeline = device
            .create_semaphore(&SemaphoreDesc {
                label: Some("cpu wait"),
                kind: SemaphoreKind::Timeline {
                    initial_value: INITIAL,
                },
            })
            .expect("every D3D12 device creates fences");
        assert_eq!(
            device
                .semaphore_value(timeline)
                .expect("a timeline has one"),
            INITIAL,
            "the semaphore did not start at the value it was created with"
        );
        assert!(
            device
                .wait_semaphores(
                    &[SemaphoreWait {
                        semaphore: timeline,
                        value: INITIAL,
                    }],
                    0,
                )
                .expect("a value already reached"),
            "a wait for the value the fence already holds reported a timeout"
        );
        assert!(
            !device
                .wait_semaphores(
                    &[SemaphoreWait {
                        semaphore: timeline,
                        value: NEVER,
                    }],
                    BUDGET_NS,
                )
                .expect("a timeout is not an error"),
            "a value nothing signalled was reported as reached"
        );
        assert!(
            device.wait_semaphores(&[], 0).expect("nothing to wait for"),
            "an empty wait list has nothing left to satisfy"
        );

        // The binary half. It is handed out — the seam requires that of every
        // device — and both CPU-side calls refuse it by name rather than
        // reading the private counter this crate keeps for it.
        let binary = device
            .create_semaphore(&SemaphoreDesc {
                label: Some("binary"),
                kind: SemaphoreKind::Binary,
            })
            .expect("a device with no timeline would still owe one of these");
        let error = device
            .semaphore_value(binary)
            .expect_err("a binary semaphore has no seam-visible value");
        assert!(
            matches!(error, HalError::Unsupported { backend, .. } if backend == BackendKind::Dx12),
            "{error:?}"
        );
        let error = device
            .wait_semaphores(
                &[SemaphoreWait {
                    semaphore: binary,
                    value: 0,
                }],
                0,
            )
            .expect_err("a binary semaphore is GPU-waitable only");
        assert!(
            matches!(error, HalError::Unsupported { backend, .. } if backend == BackendKind::Dx12),
            "{error:?}"
        );
        device.destroy_semaphore(binary);
        device.destroy_semaphore(timeline);
    }

    /// **A readback keyed on a caller's timeline observes that timeline**, not
    /// the device fence.
    ///
    /// [`ReadbackDesc::after`] is the seam's way of saying "this data is ready
    /// when *that* value arrives", and a backend that ignored it and used its
    /// own submission counter would answer `Ready` for a buffer nothing had
    /// written yet — a wrong picture with a clean log. So the `Pending` half is
    /// asserted first, against a value nothing has signalled: without it a
    /// backend that answered `Ready` immediately would pass on the second half
    /// alone.
    ///
    /// A **binary** semaphore is refused, because it has no CPU-visible value
    /// for a poll to compare against.
    #[test]
    #[ignore = "needs a real D3D12 device; run tests/run-dx12-e2e.sh"]
    fn a_readback_keyed_on_a_semaphore_waits_for_that_semaphore() {
        /// What the readback waits for, and what the submission signals.
        const AWAITED: u64 = 6;
        const BYTES: u64 = 4;

        let (_instance, device) = open_device();
        let queue = device
            .queue(QueueKind::Graphics)
            .expect("the graphics queue exists");
        let target = device
            .create_buffer(&buffer(BYTES, MemoryLocation::HostReadback))
            .expect("a readback buffer");
        let timeline = device
            .create_semaphore(&SemaphoreDesc {
                label: Some("readback after"),
                kind: SemaphoreKind::Timeline { initial_value: 0 },
            })
            .expect("every D3D12 device creates fences");

        let request = device
            .request_readback(&ReadbackDesc {
                label: Some("keyed on a timeline"),
                buffer: target,
                offset: 0,
                size: BYTES,
                after: Some(SemaphoreWait {
                    semaphore: timeline,
                    value: AWAITED,
                }),
            })
            .expect("a readback of a HostReadback buffer");

        // Nothing has signalled the value, and the device fence is irrelevant —
        // so a backend reading the wrong counter shows up right here.
        let mut out = [POISON; BYTES as usize];
        assert!(
            matches!(
                device.poll_readback(request, &mut out),
                Ok(ReadbackState::Pending)
            ),
            "a readback keyed on an unsignalled timeline reported ready"
        );
        assert_eq!(out, [POISON; BYTES as usize], "a Pending poll wrote bytes");

        device
            .submit(
                queue,
                &SubmitInfo {
                    command_buffers: &[],
                    waits: &[],
                    signals: &[SemaphoreSignal {
                        semaphore: timeline,
                        value: AWAITED,
                    }],
                },
            )
            .expect("a signal-only submission");
        drain(&device, request, BYTES as usize);

        // And a binary semaphore has no value for the poll to read, so it is
        // refused at request time rather than silently keyed on something else.
        let binary = device
            .create_semaphore(&SemaphoreDesc {
                label: Some("readback after"),
                kind: SemaphoreKind::Binary,
            })
            .expect("every device owes one of these");
        let error = device
            .request_readback(&ReadbackDesc {
                label: None,
                buffer: target,
                offset: 0,
                size: BYTES,
                after: Some(SemaphoreWait {
                    semaphore: binary,
                    value: 1,
                }),
            })
            .expect_err("a binary semaphore has no CPU-visible value");
        assert!(
            matches!(error, HalError::Unsupported { backend, .. } if backend == BackendKind::Dx12),
            "{error:?}"
        );

        device.destroy_readback(request);
        device.destroy_semaphore(binary);
        device.destroy_semaphore(timeline);
        device.destroy_buffer(target);
    }

    /// **A timeline signal that does not move forwards is refused**, because the
    /// alternative is a caller asleep for good.
    ///
    /// `ID3D12CommandQueue::Signal` will happily set a fence backwards, and
    /// nothing in D3D12 reports it: every waiter past the higher value simply
    /// never wakes, on a queue that is otherwise healthy. So the check is here,
    /// and it is [`HalError::InvalidDescriptor`] — the value is a field the
    /// caller can correct — which is the answer `crcbl-mtl` and `crcbl-wgpu`
    /// both give.
    ///
    /// The second half is the one that is easy to miss: two signals on the same
    /// semaphore in **one** `SubmitInfo` have to be checked against each other,
    /// not only against what the semaphore already holds.
    #[test]
    #[ignore = "needs a real D3D12 device; run tests/run-dx12-e2e.sh"]
    fn a_timeline_signal_that_does_not_move_forwards_is_refused() {
        const INITIAL: u64 = 4;

        let (_instance, device) = open_device();
        let queue = device
            .queue(QueueKind::Graphics)
            .expect("the graphics queue exists");
        let timeline = device
            .create_semaphore(&SemaphoreDesc {
                label: Some("monotonic"),
                kind: SemaphoreKind::Timeline {
                    initial_value: INITIAL,
                },
            })
            .expect("every D3D12 device creates fences");

        for value in [INITIAL, INITIAL - 1] {
            let error = device
                .submit(
                    queue,
                    &SubmitInfo {
                        command_buffers: &[],
                        waits: &[],
                        signals: &[SemaphoreSignal {
                            semaphore: timeline,
                            value,
                        }],
                    },
                )
                .expect_err("a timeline only moves forwards");
            assert!(
                matches!(error, HalError::InvalidDescriptor(_)),
                "signalling {value} over {INITIAL}: {error:?}"
            );
        }

        // Two signals in one submission, the second no higher than the first.
        let error = device
            .submit(
                queue,
                &SubmitInfo {
                    command_buffers: &[],
                    waits: &[],
                    signals: &[
                        SemaphoreSignal {
                            semaphore: timeline,
                            value: INITIAL + 2,
                        },
                        SemaphoreSignal {
                            semaphore: timeline,
                            value: INITIAL + 1,
                        },
                    ],
                },
            )
            .expect_err("the second signal is behind the first one in the same submission");
        assert!(matches!(error, HalError::InvalidDescriptor(_)), "{error:?}");
        // Nothing was submitted, so the floor never moved and the value the
        // refused pair started with is still available.
        device
            .submit(
                queue,
                &SubmitInfo {
                    command_buffers: &[],
                    waits: &[],
                    signals: &[SemaphoreSignal {
                        semaphore: timeline,
                        value: INITIAL + 2,
                    }],
                },
            )
            .expect("a signal above everything submitted so far");
        device.wait_idle().expect("idle");
        assert_eq!(
            device
                .semaphore_value(timeline)
                .expect("a timeline has one"),
            INITIAL + 2
        );
        device.destroy_semaphore(timeline);
    }

    /// **The entry points that crossed over must never answer `Unsupported`
    /// again.**
    ///
    /// This is the half that rots. The test above asserts that what this backend
    /// refuses still refuses; without its inverse, an entry point that was
    /// implemented and then *regressed* to a refusal — a merge that reverted a
    /// match arm, a refactor that reinstated a stub — would go on passing every
    /// test in this file. `crcbl-mtl` added this after exactly that happened.
    ///
    /// Each call below is given deliberately bad arguments, because the claim is
    /// not that they succeed: it is that they now *diagnose*. A stale handle is
    /// [`HalError::InvalidHandle`], a descriptor D3D12 cannot express is
    /// [`HalError::InvalidDescriptor`], an artifact it cannot use is
    /// [`HalError::ShaderCompilation`] — and none of them is
    /// [`HalError::Unsupported`].
    #[test]
    #[ignore = "needs a real D3D12 device; run tests/run-dx12-e2e.sh"]
    fn the_entry_points_that_landed_never_answer_unsupported_again() {
        let (_instance, device) = open_device();
        let queue = device
            .queue(QueueKind::Graphics)
            .expect("the graphics queue exists");

        let landed: Vec<(&str, HalError)> = vec![
            (
                "shader modules",
                device
                    .create_shader_module(&ShaderModuleDesc {
                        label: Some("nothing.slang"),
                        ..ShaderModuleDesc::default()
                    })
                    .expect_err("a descriptor with no artifact is not a shader"),
            ),
            (
                "bind group layouts",
                device
                    .create_bind_group_layout(&BindGroupLayoutDesc {
                        label: None,
                        entries: &[BindGroupLayoutEntry {
                            binding: 0,
                            visibility: ShaderStages::ALL,
                            kind: BindingKind::UniformBuffer { dynamic: true },
                            // A dynamic binding is one root descriptor, so an
                            // array of them has nothing to offset per element.
                            count: 4,
                            flags: BindingFlags::empty(),
                        }],
                    })
                    .expect_err("a root descriptor is not an array"),
            ),
            (
                "pipeline layouts",
                device
                    .create_pipeline_layout(&PipelineLayoutDesc {
                        label: None,
                        bind_group_layouts: &[unissued()],
                        push_constants: None,
                    })
                    .expect_err("that bind group layout was never issued"),
            ),
            (
                "bind groups",
                device
                    .create_bind_group(&BindGroupDesc {
                        label: None,
                        layout: unissued(),
                        entries: &[],
                        variable_count: None,
                    })
                    .expect_err("that bind group layout was never issued"),
            ),
            (
                "bind group updates",
                device
                    .update_bind_group(
                        unissued(),
                        &[BindGroupEntry {
                            binding: 0,
                            array_index: 0,
                            resource: BindingResource::Sampler(unissued()),
                        }],
                    )
                    .expect_err("that sampler was never issued"),
            ),
            (
                "graphics pipelines",
                device
                    .create_graphics_pipeline(&GraphicsPipelineDesc {
                        label: None,
                        layout: unissued(),
                        vertex: ShaderEntry {
                            module: unissued(),
                            entry_point: "vertexMain",
                        },
                        fragment: None,
                        primitive: PrimitiveState::default(),
                        depth_stencil: None,
                        multisample: MultisampleState::default(),
                        color_targets: &[],
                    })
                    .expect_err("that pipeline layout was never issued"),
            ),
            (
                // The last `Device` call to arrive, and the one whose regression
                // this list would otherwise not catch: a `create_mesh_pipeline`
                // reverted to a refusal answers `Unsupported` for a descriptor
                // that is merely wrong, which is what every entry here rules out.
                "mesh pipelines",
                device
                    .create_mesh_pipeline(&crcbl_hal::MeshPipelineDesc {
                        label: None,
                        layout: unissued(),
                        task: None,
                        task_workgroup_size: [1, 1, 1],
                        mesh: ShaderEntry {
                            module: unissued(),
                            entry_point: "meshMain",
                        },
                        mesh_workgroup_size: [1, 1, 1],
                        fragment: None,
                        primitive: PrimitiveState::default(),
                        depth_stencil: None,
                        multisample: MultisampleState::default(),
                        color_targets: &[],
                    })
                    .expect_err("that pipeline layout was never issued"),
            ),
            (
                "compute pipelines",
                device
                    .create_compute_pipeline(&ComputePipelineDesc {
                        label: None,
                        layout: unissued(),
                        compute: ShaderEntry {
                            module: unissued(),
                            entry_point: "computeMain",
                        },
                        workgroup_size: [crcbl_shaders::compute_probe::WORKGROUP_SIZE, 1, 1],
                    })
                    .expect_err("that pipeline layout was never issued"),
            ),
            (
                "semaphore waits",
                device
                    .wait_semaphores(
                        &[SemaphoreWait {
                            semaphore: unissued(),
                            value: 1,
                        }],
                        0,
                    )
                    .expect_err("that semaphore was never issued"),
            ),
            (
                "query sets",
                device
                    .create_query_set(&QuerySetDesc {
                        label: None,
                        kind: QueryKind::Timestamp,
                        count: 0,
                    })
                    .expect_err("a set of no queries is not a query heap"),
            ),
            (
                "query reads",
                device
                    .query_results(unissued(), 0, &mut [0u64; 1])
                    .expect_err("that query set was never issued"),
            ),
        ];
        assert!(!landed.is_empty(), "nothing to check");
        for (what, error) in &landed {
            assert!(
                !matches!(error, HalError::Unsupported { .. }),
                "{what} answered Unsupported, so the slice regressed to a refusal: {error:?}"
            );
        }

        type Landed = (&'static str, fn(&mut dyn CommandEncoder));
        let recording: &[Landed] = &[
            ("graphics pipelines", |encoder| {
                encoder.bind_graphics_pipeline(unissued());
            }),
            ("bind groups", |encoder| {
                encoder.bind_group(0, unissued(), &[], unissued());
            }),
            ("push constants", |encoder| {
                encoder.push_constants(ShaderStages::ALL, 0, &[0u8; 4], unissued());
            }),
            ("draws", |encoder| encoder.draw(0..3, 0..1)),
            // Both mesh draws, because they are two entry points and either
            // could regress to a refusal on its own. Each refuses here for the
            // reason the plain draw above does — no pipeline is bound — which is
            // an `InvalidDescriptor` and not an `Unsupported`.
            ("mesh draws", |encoder| encoder.draw_mesh_tasks(1, 1, 1)),
            ("indirect mesh draws", |encoder| {
                encoder.draw_mesh_tasks_indirect(&DrawIndirect {
                    args: unissued(),
                    offset: 0,
                    draw_count: 1,
                    stride: 0,
                });
            }),
            // The buffer clear, and the one this list would otherwise not catch
            // regressing: a `clear_buffer` reverted to a refusal would answer
            // `Unsupported` for a handle that is merely dead. **This backend
            // now refuses no command at all** — the non-zero fill was the last
            // one it chose, and it went when the seam dropped the value, so the
            // test that held the refusals has no subject left and is gone.
            ("buffer clears", |encoder| {
                encoder.clear_buffer(unissued(), 0, 4);
            }),
            ("image-to-image copies", |encoder| {
                let layers = ImageSubresourceLayers {
                    aspect: ImageAspect::COLOR,
                    mip: 0,
                    base_layer: 0,
                    layer_count: 1,
                };
                encoder.copy_image_to_image(&ImageCopy {
                    src: unissued(),
                    src_subresource: layers,
                    src_offset: Offset3d::default(),
                    dst: unissued(),
                    dst_subresource: layers,
                    dst_offset: Offset3d::default(),
                    extent: Extent3d::d2(1, 1),
                });
            }),
            // Each opens the pass first, because the scope is what decides the
            // bind point and a compute command outside one is a descriptor
            // error rather than a handle one — which would pass this test
            // without ever reaching the code it is about.
            ("compute pipelines", |encoder| {
                encoder.begin_compute_pass(&ComputePassDesc {
                    label: None,
                    timestamp_writes: None,
                });
                encoder.bind_compute_pipeline(unissued());
            }),
            ("dispatches", |encoder| {
                encoder.begin_compute_pass(&ComputePassDesc {
                    label: None,
                    timestamp_writes: None,
                });
                encoder.dispatch(1, 1, 1);
            }),
            ("indirect dispatches", |encoder| {
                encoder.begin_compute_pass(&ComputePassDesc {
                    label: None,
                    timestamp_writes: None,
                });
                encoder.dispatch_indirect(unissued(), 0);
            }),
            // All three query verbs, because `reset_query_set` records no
            // command at all: it must still resolve the handle, or the seam
            // suite's "reset it through a submitted command buffer" check would
            // pass against a set this device never issued.
            ("query resets", |encoder| {
                encoder.reset_query_set(unissued(), 0..1);
            }),
            ("pass timestamps", |encoder| {
                encoder.begin_compute_pass(&ComputePassDesc {
                    label: None,
                    timestamp_writes: Some(crcbl_hal::PassTimestampWrites {
                        set: unissued(),
                        beginning_of_pass: 0,
                        end_of_pass: 1,
                    }),
                });
                encoder.end_compute_pass();
            }),
            ("query resolves", |encoder| {
                encoder.resolve_query_set(unissued(), 0..1, unissued(), 0);
            }),
        ];
        assert!(!recording.is_empty(), "nothing to check");
        for (what, record) in recording {
            let mut encoder =
                device.create_command_encoder(&CommandEncoderDesc { label: None, queue });
            record(encoder.as_mut());
            let Err(error) = encoder.finish() else {
                panic!("{what} recorded successfully against handles nothing issued");
            };
            assert!(
                !matches!(error, HalError::Unsupported { .. }),
                "{what} answered Unsupported, so the slice regressed to a refusal: {error:?}"
            );
        }
    }

    /// **Every presentation entry point refuses a handle nothing issued as a
    /// dead handle, and never as `OutOfDate` or `Lost`.**
    ///
    /// This is what the slice's four refusals were protecting, restated for a
    /// slice that has landed. The refusals themselves are gone — the calls
    /// work now — but the property underneath them was never about `Unsupported`
    /// as such. It was that **the refusal has to arrive before any handle is
    /// resolved into a real object, and it must not be a surface condition**:
    /// a call that panicked instead of returning looks the same as one nobody
    /// tested, and a dead handle miscast as [`SurfaceError::OutOfDate`] would
    /// put a render loop into an unending reconfigure, reconfiguring a
    /// swapchain that does not exist.
    ///
    /// So the same five calls are made with the same unissued handles, and the
    /// answer demanded is [`HalError::InvalidHandle`] naming the kind. Every
    /// handle carries no device tag at all, which is the case
    /// `crate::handle`'s `local` separates from another device's — so none of
    /// them can resolve, whatever this device has created.
    ///
    /// `wait_until_presented` is in the list and answers `Err` rather than the
    /// seam's immediate `Ok(())`: that answer is for an *id* with no record,
    /// not for a swapchain handle that never existed, and collapsing the two
    /// would make a caller's typo indistinguishable from a frame already shown.
    ///
    /// Red the moment any of them answers `OutOfDate`, `Lost`, `Timeout` or
    /// `Unsupported`, and red if one starts resolving a handle it should not.
    #[test]
    #[ignore = "needs a real D3D12 device; run tests/run-dx12-e2e.sh"]
    fn the_presentation_entry_points_refuse_a_dead_handle_and_never_as_out_of_date() {
        let (_instance, device) = open_device();
        let queue = device
            .queue(QueueKind::Graphics)
            .expect("the graphics queue exists");
        let swapchain = SwapchainDesc {
            label: Some("crcbl-dx12 test swapchain"),
            surface: unissued(),
            format: Format::Bgra8UnormSrgb,
            extent: (320, 200),
            image_count: 3,
            present_mode: PresentMode::Fifo,
            composite_alpha: CompositeAlpha::Opaque,
        };

        let refusals: Vec<(&str, &str, SurfaceError)> = vec![
            (
                "swapchain creation",
                "surface",
                device
                    .create_swapchain(&swapchain)
                    .expect_err("no instance issued that surface"),
            ),
            (
                "swapchain reconfiguration",
                "surface",
                device
                    .reconfigure_swapchain(unissued(), &swapchain)
                    .expect_err("nor for a reconfigure"),
            ),
            (
                "acquire",
                "swapchain",
                device
                    .acquire_next_frame(unissued())
                    .expect_err("no device issued that swapchain"),
            ),
            (
                "present",
                "swapchain",
                device
                    .present(
                        queue,
                        &PresentInfo {
                            swapchain: unissued(),
                            waits: &[],
                            present_id: Some(1),
                        },
                    )
                    .expect_err("there is nothing to present"),
            ),
            (
                "present wait",
                "swapchain",
                device
                    .wait_until_presented(unissued(), 1, Duration::from_millis(1))
                    .expect_err("a dead handle is not an id with no record"),
            ),
        ];
        assert!(!refusals.is_empty(), "nothing to check");
        for (what, kind, error) in &refusals {
            let SurfaceError::Hal(hal) = error else {
                panic!("{what}: a dead handle is not a surface condition: {error:?}");
            };
            assert!(
                matches!(hal, HalError::InvalidHandle { kind: named, .. } if named == kind),
                "{what}: expected a dead {kind}, got {hal:?}"
            );
            // `SurfaceError::Hal` is transparent, so the error's own words are
            // what a caller prints. A wrapper that swallowed them would leave
            // the caller with a message naming nothing at all.
            let text = error.to_string();
            assert!(text.contains(kind), "{what}: {text}");
        }
    }

    /// A present whose `waits` name a semaphore is refused on the semaphore,
    /// because `acquire_next_frame` hands none out.
    ///
    /// The seam's implicit-acquire shape means a conforming caller splices an
    /// empty slice here, so anything in it is either a handle no device issued
    /// or a semaphore this swapchain never gave out — and the answer has to say
    /// *semaphore*, not "swapchain", or a caller reading the message goes
    /// looking at the wrong object. Asserted with the swapchain handle
    /// deliberately left unissued too: the semaphore is checked first.
    ///
    /// **Both cases are asserted, because `create_semaphore` landed.** A live
    /// handle here used to be impossible; now it is not, and answering
    /// `InvalidHandle` for a semaphore the caller is holding would send them
    /// looking for a lifetime bug that is not there.
    #[test]
    #[ignore = "needs a real D3D12 device; run tests/run-dx12-e2e.sh"]
    fn a_present_with_a_semaphore_is_refused_on_the_semaphore() {
        let (_instance, device) = open_device();
        let queue = device
            .queue(QueueKind::Graphics)
            .expect("the graphics queue exists");
        let error = device
            .present(
                queue,
                &PresentInfo {
                    swapchain: unissued(),
                    waits: &[unissued()],
                    present_id: None,
                },
            )
            .expect_err("no semaphore was ever issued");
        let SurfaceError::Hal(hal) = error else {
            panic!("a dead semaphore is not a surface condition: {error:?}");
        };
        assert!(
            matches!(hal, HalError::InvalidHandle { kind, .. } if kind == "semaphore"),
            "{hal:?}"
        );

        let binary = device
            .create_semaphore(&SemaphoreDesc {
                label: Some("present wait"),
                kind: SemaphoreKind::Binary,
            })
            .expect("every D3D12 device creates fences");
        let error = device
            .present(
                queue,
                &PresentInfo {
                    swapchain: unissued(),
                    waits: &[binary],
                    present_id: None,
                },
            )
            .expect_err("this swapchain hands out no present semaphore");
        let SurfaceError::Hal(hal) = error else {
            panic!("a misplaced semaphore is not a surface condition: {error:?}");
        };
        assert!(matches!(hal, HalError::InvalidDescriptor(_)), "{hal:?}");
        assert!(hal.to_string().contains("semaphore"), "{hal}");
        device.destroy_semaphore(binary);
    }

    /// **`Features::PRESENT_FEEDBACK` is reported, and the seam's immediate
    /// answers do not depend on a swapchain existing.**
    ///
    /// The flag is read once at device open, before any swapchain — which is
    /// the whole reason a per-swapchain capability has to be answered at device
    /// level — so this is where the claim is checked. `crcbl_dx12::adapter`
    /// argues why it is unconditional here where `crcbl-vk` has to probe.
    ///
    /// Red when the flag stops being reported, and red if it ever creeps into
    /// [`Features::GPU_DRIVEN`], which it must not: the seam says a device without
    /// present feedback renders the same frames.
    #[test]
    #[ignore = "needs a real D3D12 device; run tests/run-dx12-e2e.sh"]
    fn every_device_reports_present_feedback_and_it_is_not_a_gpu_driven_flag() {
        let (_instance, device) = open_device();
        assert!(
            device.caps().features.contains(Features::PRESENT_FEEDBACK),
            "every D3D12 machine can create a waitable swapchain: {:?}",
            device.caps().features
        );
        assert!(
            !Features::GPU_DRIVEN.contains(Features::PRESENT_FEEDBACK),
            "pacing is not part of the GPU-driven bundle"
        );
    }

    /// **A handle that cannot resolve is not the same answer as a slice that has
    /// not landed**, and these are the two entry points that say so.
    ///
    /// The crate docs draw the line: `Unsupported` for a call this backend has
    /// not written, and [`HalError::InvalidHandle`] for anything it can genuinely
    /// diagnose. `query_results` and `semaphore_value` are on the far side of it
    /// — they take a handle nothing issued, so "you handed me something that
    /// never resolved" is both true and more useful than "the query slice is not
    /// here".
    ///
    /// Asserted beside `create_pipeline_layout`, which refuses the other way,
    /// because the claim is about the difference: a backend that answered
    /// `Unsupported` everywhere would pass either assertion alone.
    /// `create_semaphore` and `create_query_set` both used to stand on the
    /// `Unsupported` side and now *succeed*, so they are here as the third
    /// answer — calls that landed, checked here so the pair above cannot quietly
    /// become a rule about one call each.
    ///
    /// **The `Unsupported` half of that contrast is no longer a `Device` call.**
    /// `create_mesh_pipeline` was the last one and the mesh slice took it: it now
    /// resolves its descriptor, so it is asserted below as a *handle* diagnosis
    /// like the two above. What still answers `Unsupported` is the encoder's
    /// buffer fill with a **non-zero** value — the zero fill lands — and
    /// [`the_commands_this_backend_refuses_still_refuse_and_name_themselves`]
    /// is where that is checked.
    #[test]
    #[ignore = "needs a real D3D12 device; run tests/run-dx12-e2e.sh"]
    fn a_query_set_and_a_semaphore_handle_refuse_as_unresolvable_not_as_unimplemented() {
        let (_instance, device) = open_device();

        // Poisoned, so "left untouched" is distinguishable from "written with
        // the zeros a timestamp-less device is allowed to report".
        let mut results = [u64::MAX; 2];
        let error = device
            .query_results(unissued(), 0, &mut results)
            .expect_err("no query set was ever issued");
        assert!(
            matches!(error, HalError::InvalidHandle { kind, .. } if kind == "query set"),
            "{error:?}"
        );
        assert_eq!(
            results,
            [u64::MAX; 2],
            "a refused query wrote results anyway: {results:?}"
        );

        let error = device
            .semaphore_value(unissued())
            .expect_err("no semaphore was ever issued");
        assert!(
            matches!(error, HalError::InvalidHandle { kind, .. } if kind == "semaphore"),
            "{error:?}"
        );

        // **This block used to assert the opposite, and the mesh slice is why it
        // changed.** It read `create_mesh_pipeline` as the unarrived-slice side
        // of the contrast — `Unsupported` rather than blaming the caller's
        // handle — and said in as many words that "a backend that resolved them
        // first would answer `InvalidHandle` and fail this". That is now the
        // correct answer rather than a failure: the call packs a real pipeline
        // state stream, so it diagnoses its descriptor, and the layout is the
        // first handle it resolves. Asserting the kind rather than only the
        // variant is what keeps this a statement about *which* handle was looked
        // at first, so a reordering has to be a deliberate edit here.
        let error = device
            .create_mesh_pipeline(&crcbl_hal::MeshPipelineDesc {
                label: None,
                layout: unissued(),
                task: None,
                task_workgroup_size: [1, 1, 1],
                mesh: ShaderEntry {
                    module: unissued(),
                    entry_point: "meshMain",
                },
                mesh_workgroup_size: [1, 1, 1],
                fragment: None,
                primitive: PrimitiveState::default(),
                depth_stencil: None,
                multisample: MultisampleState::default(),
                color_targets: &[],
            })
            .expect_err("every handle in that descriptor was unissued");
        assert!(
            matches!(error, HalError::InvalidHandle { kind, .. } if kind == "pipeline layout"),
            "{error:?}"
        );

        // The `Unsupported` side of the contrast still exists, but no `Device`
        // entry point carries it any more — it moved to the encoder, where
        // `Dx12CommandEncoder::fill_buffer` refuses a non-zero value before
        // touching its handle.
        // `the_commands_this_backend_refuses_still_refuse_and_name_themselves`
        // owns that assertion, and this comment is the pointer that stops the
        // pair being read as "this backend answers `InvalidHandle` to everything".

        // And the third answer, on the two calls that used to be on the
        // `Unsupported` side: they create the object, and the handle dies with
        // it rather than resolving to whatever takes its slot next.
        let set = device
            .create_query_set(&QuerySetDesc {
                label: Some("unresolvable-handle probe"),
                kind: QueryKind::Timestamp,
                count: 1,
            })
            .expect("every D3D12 device creates timestamp query heaps");
        device.destroy_query_set(set);
        let error = device
            .query_results(set, 0, &mut results)
            .expect_err("that query set was destroyed");
        assert!(
            matches!(error, HalError::InvalidHandle { kind, .. } if kind == "query set"),
            "{error:?}"
        );

        let semaphore = device
            .create_semaphore(&SemaphoreDesc {
                label: Some("unresolvable-handle probe"),
                kind: SemaphoreKind::Timeline { initial_value: 0 },
            })
            .expect("every D3D12 device creates fences");
        device.destroy_semaphore(semaphore);
        // And the handle is dead afterwards rather than resolving to whatever
        // takes its slot next, which is the whole of what `destroy` means here.
        let error = device
            .semaphore_value(semaphore)
            .expect_err("that semaphore was destroyed");
        assert!(
            matches!(error, HalError::InvalidHandle { kind, .. } if kind == "semaphore"),
            "{error:?}"
        );

        // **The three entry points that now take a semaphore they cannot get.**
        // `submit` and `request_readback` both work, so neither may answer
        // `Unsupported` — the caller handed over a handle, and no device issued
        // it. Answering "the semaphore slice is not here" would send a reader
        // looking for a missing feature instead of a dead handle.
        let queue = device
            .queue(QueueKind::Graphics)
            .expect("the graphics queue exists");
        let wait = SemaphoreWait {
            semaphore: unissued(),
            value: 1,
        };
        let error = device
            .submit(
                queue,
                &SubmitInfo {
                    command_buffers: &[],
                    waits: &[wait],
                    signals: &[],
                },
            )
            .expect_err("no semaphore was ever issued");
        assert!(
            matches!(error, HalError::InvalidHandle { kind, .. } if kind == "semaphore"),
            "{error:?}"
        );
        let error = device
            .submit(
                queue,
                &SubmitInfo {
                    command_buffers: &[],
                    waits: &[],
                    signals: &[SemaphoreSignal {
                        semaphore: unissued(),
                        value: 1,
                    }],
                },
            )
            .expect_err("no semaphore was ever issued");
        assert!(
            matches!(error, HalError::InvalidHandle { kind, .. } if kind == "semaphore"),
            "{error:?}"
        );
        let readback = device
            .create_buffer(&buffer(TARGET_BYTES as u64, MemoryLocation::HostReadback))
            .expect("a readback buffer");
        let error = device
            .request_readback(&ReadbackDesc {
                label: None,
                buffer: readback,
                offset: 0,
                size: 4,
                after: Some(wait),
            })
            .expect_err("ReadbackDesc::after names a semaphore nothing issued");
        assert!(
            matches!(error, HalError::InvalidHandle { kind, .. } if kind == "semaphore"),
            "{error:?}"
        );

        // A readback handle nothing issued is unresolvable too, and the poll
        // must leave the output alone rather than write the zeros a caller would
        // read as data.
        let mut poisoned = [POISON; 4];
        let error = device
            .poll_readback(unissued(), &mut poisoned)
            .expect_err("no readback was ever issued");
        assert!(
            matches!(error, HalError::InvalidHandle { kind, .. } if kind == "readback"),
            "{error:?}"
        );
        assert_eq!(poisoned, [POISON; 4], "a refused poll wrote to the output");
        device.destroy_buffer(readback);
    }

    // --- queries ---

    /// Queries the timestamp tests write: one before the timed work, one after.
    const TIMED_QUERIES: u32 = 2;

    /// What a resolve destination holds before the resolve, so "never written"
    /// stays distinguishable from "these really are the ticks".
    const QUERY_POISON: u64 = 0x5A5A_5A5A_5A5A_5A5A;

    /// **Both of the seam's read paths report the same two ticks, and the ticks
    /// came from a clock.**
    ///
    /// The claim `Capability::TimestampQuery` makes here, driven end to end:
    /// `EndQuery` writes a timestamp at each boundary of a compute pass that
    /// really dispatches,
    /// [`Device::query_results`] reads them through its own resolve and
    /// submission, and `resolve_query_set` reads the *same two queries* into a
    /// buffer this test maps.
    ///
    /// **The two reads are in different units, and that is the claim.**
    /// `query_results` reports nanoseconds — the seam's unit, converted here
    /// from `GetTimestampFrequency` — while `resolve_query_set` is a GPU-side
    /// copy that writes the device's own ticks and has nothing to convert with.
    /// So the second read is held to `query::timestamp_nanos` of the first,
    /// which is both halves at once: a path reaching a different heap, range or
    /// stride fails it, and so does a `query_results` that forgot to convert.
    ///
    /// The failures it cannot pass through: a pass whose `timestamp_writes`
    /// were accepted and dropped reads back two zeros, a counter that is not running reads the
    /// same value twice, and a resolve that wrote nothing leaves
    /// [`QUERY_POISON`] behind.
    ///
    /// **The resolve goes straight into a readback-heap buffer**, which is the
    /// same assumption `Device::query_results` is built on: such a resource is
    /// created in `D3D12_RESOURCE_STATE_COPY_DEST` and can never leave it, which
    /// is exactly the state `ResolveQueryData` requires of a destination.
    /// `wgpu-hal` does the same — a `MAP_READ | QUERY_RESOLVE` buffer there is a
    /// readback-heap resource its `copy_query_results` resolves into with no
    /// barrier — and `open_device` fails this test on any debug-layer message,
    /// so a runtime that disagreed would say so rather than be assumed about.
    #[test]
    #[ignore = "needs a real D3D12 device; run tests/run-dx12-e2e.sh"]
    fn d3d12_timestamps_advance_and_both_read_paths_report_the_same_ticks() {
        let (_instance, device) = open_device();
        assert!(
            device.caps().features.contains(Features::TIMESTAMP_QUERY),
            "every D3D12 device takes a timestamp query heap: {:?}",
            device.caps().features
        );

        let set = device
            .create_query_set(&QuerySetDesc {
                label: Some("crcbl-dx12 timestamps"),
                kind: QueryKind::Timestamp,
                count: TIMED_QUERIES,
            })
            .expect("a timestamp query heap");
        // The timed work is the probe's own dispatch, because the seam times a
        // *pass* and nothing else: a copy is only legal outside one, so the
        // 4 MiB copy this used to bracket has no boundary to be measured at.
        // The probe's `run` records the reset beside the descriptor.
        let probe = ComputeProbe::new(&device);
        let values = probe.run(
            &device,
            Some(crcbl_hal::PassTimestampWrites {
                set,
                beginning_of_pass: 0,
                end_of_pass: 1,
            }),
            |encoder| encoder.dispatch(PROBE_GROUPS, 1, 1),
        );
        assert_probe(
            &values,
            &probe_expected(PROBE_ELEMENTS),
            "the timed pass really dispatched",
        );

        let mut nanos = [0u64; TIMED_QUERIES as usize];
        device
            .query_results(set, 0, &mut nanos)
            .expect("reading the range this set was created with");
        let [start, end] = nanos;
        assert!(
            start != 0 || end != 0,
            "both timestamps read back as zero, which is what a pass whose timestampWrites were \
             accepted and dropped produces"
        );
        assert!(
            end > start,
            "the timestamps bracketing the dispatched compute pass came back as {start} then \
             {end} nanoseconds; a clock that ran cannot report the closing write at or before \
             the opening one"
        );
        // The other half of the pair: the read above is only evidence beside a
        // read that is refused, since `Ok` is what an implementation doing
        // nothing answers to everything.
        let mut past_the_end = [0u64; TIMED_QUERIES as usize + 1];
        let error = device
            .query_results(set, 0, &mut past_the_end)
            .expect_err("one query past the end of the set");
        assert!(matches!(error, HalError::InvalidDescriptor(_)), "{error:?}");

        // The second read path, over the same two queries.
        const BYTES: u64 = TIMED_QUERIES as u64 * 8;
        let resolved = device
            .create_buffer(&BufferDesc {
                label: Some("crcbl-dx12 timestamp resolve"),
                size: BYTES,
                usage: BufferUsage::QUERY_RESOLVE | BufferUsage::TRANSFER_DST,
                memory: MemoryLocation::HostReadback,
            })
            .expect("a readback buffer");
        let primer: Vec<u8> = core::iter::repeat_n(QUERY_POISON, TIMED_QUERIES as usize)
            .flat_map(u64::to_le_bytes)
            .collect();
        device
            .write_buffer(resolved, 0, &primer)
            .expect("a readback heap is writable from the CPU");
        run(&device, |encoder| {
            encoder.resolve_query_set(set, 0..TIMED_QUERIES, resolved, 0);
        });
        let words: Vec<u64> = read_back(&device, resolved, BYTES as usize)
            .chunks_exact(8)
            .map(|word| u64::from_le_bytes(word.try_into().expect("eight bytes")))
            .collect();
        assert_ne!(
            words,
            vec![QUERY_POISON; TIMED_QUERIES as usize],
            "resolve_query_set left the destination untouched"
        );
        // The resolve wrote ticks and `query_results` returned nanoseconds, so
        // the comparison runs through the same conversion the read did. Exact,
        // not approximate: both sides are the identical queries, already
        // available, so nothing but the unit separates them.
        let converted: Vec<u64> = words
            .iter()
            .map(|ticks| query::timestamp_nanos(*ticks, device.inner.timestamp_frequency))
            .collect();
        assert_eq!(
            converted.as_slice(),
            nanos.as_slice(),
            "resolve_query_set wrote {words:?} ticks for the same two queries query_results had \
             just read as {nanos:?} nanoseconds, which is {converted:?} once converted at this \
             queue's {} ticks per second; either one path is reaching a different heap, range or \
             stride, or query_results is handing the seam raw ticks",
            device.inner.timestamp_frequency
        );

        device.destroy_buffer(resolved);
        probe.destroy(&device);
        device.destroy_query_set(set);
    }

    /// **Every kind of query set is creatable, and the reads the seam's shape
    /// cannot express are refused rather than half-answered.**
    ///
    /// Occlusion and statistics heaps are created and reset through a submitted
    /// command buffer, which is what shows the handle reaches this crate's own
    /// encoder — `reset_query_set` records no D3D12 command at all, so a set
    /// this device never issued has nothing but the handle check to fail on.
    ///
    /// A read of a *statistics* set is [`HalError::Unsupported`]: `out` is one
    /// `u64` per query while D3D12 resolves a whole
    /// `D3D12_QUERY_DATA_PIPELINE_STATISTICS`, so there is no length that both
    /// names a legal range and matches what the resolve wrote. Refusing is the
    /// honest answer and returning the first counter would be the dishonest one;
    /// `crate::query`'s `check_destination` is what stops the same mismatch
    /// becoming a buffer overrun on the resolve path, and the encoder half of
    /// that is asserted below.
    ///
    /// # No occlusion set is *read* here, and that is deliberate
    ///
    /// The seam has no begin/end verb, so no work this crate can record ever
    /// reaches an occlusion pool — which makes every occlusion read a
    /// `ResolveQueryData` over queries that were never ended, and D3D12 has a
    /// debug-layer message for exactly that
    /// (`D3D12_MESSAGE_ID_RESOLVE_QUERY_INVALID_QUERY_STATE`). Whether it fires
    /// for a query that was never *begun* is not something this workspace can
    /// settle, and `open_device` fails a test on any warning — so the successful
    /// read that makes the refusal below mean something is asserted on a
    /// **timestamp** set instead, in
    /// [`d3d12_timestamps_advance_and_both_read_paths_report_the_same_ticks`],
    /// whose queries really were written. The seam suite drives the occlusion
    /// read itself on WARP.
    #[test]
    #[ignore = "needs a real D3D12 device; run tests/run-dx12-e2e.sh"]
    fn d3d12_query_sets_of_every_kind_create_and_refuse_the_reads_the_seam_cannot_shape() {
        let (_instance, device) = open_device();
        let queue = device
            .queue(QueueKind::Graphics)
            .expect("the graphics queue exists");
        const COUNT: u32 = 2;

        for kind in [QueryKind::Occlusion, QueryKind::PipelineStatistics] {
            let set = device
                .create_query_set(&QuerySetDesc {
                    label: Some("crcbl-dx12 query set"),
                    kind,
                    count: COUNT,
                })
                .unwrap_or_else(|error| panic!("{kind:?}: {error:?}"));
            run(&device, |encoder| encoder.reset_query_set(set, 0..COUNT));

            // The range is checked before the kind is, so a read past the end of
            // a statistics set says `InvalidDescriptor` too — which is the right
            // order: the caller's range is a field it can correct, and the
            // width mismatch is not.
            let mut past_the_end = [0u64; COUNT as usize + 1];
            let error = device
                .query_results(set, 0, &mut past_the_end)
                .expect_err("one query past the end of the set");
            assert!(
                matches!(error, HalError::InvalidDescriptor(_)),
                "{kind:?}: {error:?}"
            );

            if matches!(kind, QueryKind::PipelineStatistics) {
                let mut inside = [0u64; COUNT as usize];
                let error = device
                    .query_results(set, 0, &mut inside)
                    .expect_err("one u64 per query is not this pool's layout");
                assert!(
                    matches!(error, HalError::Unsupported { backend, .. } if backend == BackendKind::Dx12),
                    "{error:?}"
                );
            }
            device.destroy_query_set(set);
        }

        // The resolve path's two refusals, which are the ones D3D12 itself would
        // not report: an offset it cannot take, and a destination sized at the
        // seam's one `u64` per query for a set that resolves eleven.
        let statistics = device
            .create_query_set(&QuerySetDesc {
                label: Some("crcbl-dx12 statistics"),
                kind: QueryKind::PipelineStatistics,
                count: COUNT,
            })
            .expect("a statistics query heap");
        let narrow = device
            .create_buffer(&BufferDesc {
                label: Some("a destination sized for timestamps"),
                size: u64::from(COUNT) * 8,
                usage: BufferUsage::QUERY_RESOLVE | BufferUsage::TRANSFER_DST,
                memory: MemoryLocation::HostReadback,
            })
            .expect("a readback buffer");
        type Refused = (&'static str, u64);
        let refusals: &[Refused] = &[
            ("an unaligned destination offset", 4),
            ("a destination sized for one u64 per query", 0),
        ];
        assert!(!refusals.is_empty(), "nothing to check");
        for (what, dst_offset) in refusals {
            let mut encoder = device.create_command_encoder(&CommandEncoderDesc {
                label: Some("crcbl-dx12 statistics resolve"),
                queue,
            });
            encoder.resolve_query_set(statistics, 0..COUNT, narrow, *dst_offset);
            let Err(error) = encoder.finish() else {
                panic!("{what} recorded successfully, so D3D12 was handed it");
            };
            assert!(
                matches!(error, HalError::InvalidDescriptor(_)),
                "{what}: {error:?}"
            );
        }

        // And a timestamp written into a set of another kind, which D3D12 would
        // take and quietly fill with a number from a heap nobody asked about.
        let mut encoder = device.create_command_encoder(&CommandEncoderDesc {
            label: Some("crcbl-dx12 mismatched timestamp"),
            queue,
        });
        encoder.begin_compute_pass(&ComputePassDesc {
            label: None,
            timestamp_writes: Some(crcbl_hal::PassTimestampWrites {
                set: statistics,
                beginning_of_pass: 0,
                end_of_pass: 1,
            }),
        });
        encoder.end_compute_pass();
        let Err(error) = encoder.finish() else {
            panic!("a timestamp was written into a statistics set");
        };
        assert!(matches!(error, HalError::InvalidDescriptor(_)), "{error:?}");

        device.destroy_buffer(narrow);
        device.destroy_query_set(statistics);
    }

    // --- the compute path ---
    //
    // The D3D12 half of `crcbl-vk`'s `vk_e2e/compute.rs` and `crcbl-mtl`'s
    // dispatch tests: the same shader, the same expectations and the same
    // sentinel. `dispatch` returns nothing, so a backend that recorded no
    // `Dispatch` at all would submit cleanly and leave a buffer full of
    // [`PROBE_SENTINEL`] — only reading the destination back tells the two
    // apart, which is why `compute_probe.slang` exists.

    /// Workgroups the probe's buffers are sized for.
    ///
    /// Eight, so the indirect dispatch can ask for two and leave six
    /// workgroups' worth of untouched sentinel behind it — which is what tells
    /// "the argument buffer was read" apart from "everything was dispatched
    /// anyway".
    const PROBE_GROUPS: u32 = 8;

    /// Elements the probe transforms.
    const PROBE_ELEMENTS: u32 = PROBE_GROUPS * crcbl_shaders::compute_probe::WORKGROUP_SIZE;

    /// What the destination holds before every dispatch.
    ///
    /// Deliberately not zero and deliberately not a square: a destination that
    /// was never written must not be confusable with one the shader wrote, and
    /// zero is both its own square and what fresh device memory tends to be.
    const PROBE_SENTINEL: u32 = 0xDEAD_BEEF;

    /// Bytes one probe buffer occupies.
    const fn probe_bytes() -> u64 {
        PROBE_ELEMENTS as u64 * 4
    }

    /// Bytes between the two `Params` blocks of a dynamic probe's uniform
    /// buffer.
    ///
    /// `D3D12_CONSTANT_BUFFER_DATA_PLACEMENT_ALIGNMENT`, which is what
    /// `crcbl_dx12::adapter` reports as
    /// `min_uniform_buffer_offset_alignment` — a root CBV's address must be a
    /// multiple of it, so this is the smallest non-zero dynamic offset a caller
    /// may pass here.
    const PROBE_PARAMS_STRIDE: u32 = 256;

    /// The element count the *second* `Params` block carries.
    ///
    /// Not [`PROBE_ELEMENTS`] and not a multiple of the workgroup size, so a
    /// dispatch that read the second block leaves a boundary partway through a
    /// group and the sentinel behind it — while one that read the first writes
    /// every element and leaves no sentinel at all. The two readbacks cannot be
    /// confused, which is what makes the offset provable rather than assumed.
    const PROBE_DYNAMIC_COUNT: u32 = PROBE_ELEMENTS / 2 + 1;

    /// The probe's input, one distinct value per index.
    ///
    /// Distinct matters: with a constant input, a shader that indexed `source`
    /// wrongly would still produce the right number in every slot. `index + 1`
    /// avoids zero, whose square is itself.
    fn probe_source() -> Vec<u32> {
        (0..PROBE_ELEMENTS).map(|index| index + 1).collect()
    }

    /// What the destination must hold for `elements` dispatched elements, and
    /// the sentinel beyond them.
    ///
    /// Written out here rather than derived from the shader: squaring is a
    /// closed form the test states for itself, which is the whole reason the
    /// probe squares.
    fn probe_expected(elements: u32) -> Vec<u32> {
        probe_source()
            .into_iter()
            .enumerate()
            .map(|(index, value)| {
                if (index as u32) < elements {
                    value * value
                } else {
                    PROBE_SENTINEL
                }
            })
            .collect()
    }

    /// Everything one compute dispatch needs, built through the seam.
    struct ComputeProbe {
        params: BufferHandle,
        source: BufferHandle,
        destination: BufferHandle,
        /// The upload-heap buffer the sentinel is copied from before each run.
        ///
        /// A copy rather than `fill_buffer`: the sentinel is non-zero and this
        /// backend fills only to zero, so the reset is a transfer the encoder
        /// already records rather than a fill it would refuse.
        sentinel: BufferHandle,
        /// Host-readable copy target, so the result can be asserted rather than
        /// assumed.
        staging: BufferHandle,
        set_layout: BindGroupLayoutHandle,
        group: BindGroupHandle,
        pipeline_layout: PipelineLayoutHandle,
        pipeline: ComputePipelineHandle,
        /// What [`run`](Self::run) passes to `bind_group`. Empty unless the
        /// probe was built by [`dynamic`](Self::dynamic), and writable so one
        /// probe can be run at two offsets — which is what makes a difference in
        /// the readback a statement about the offset and nothing else.
        dynamic_offsets: Vec<u32>,
    }

    impl ComputeProbe {
        /// Builds the pipeline, stages the input in, and leaves every buffer in
        /// the state [`run`](Self::run) expects.
        ///
        /// The destination is left in `TransferSrc` rather than `Undefined`
        /// because D3D12 checks a transition's *before* state: every run starts
        /// from the same place only if the setup ends where a run ends.
        fn new(device: &Dx12Device) -> Self {
            Self::build(device, false)
        }

        /// The same probe with binding 0 declared `dynamic`, so it becomes a
        /// **root CBV** rather than an entry in the set's descriptor table.
        ///
        /// Its uniform buffer holds two `Params` blocks
        /// [`PROBE_PARAMS_STRIDE`] apart: the first says every element, the
        /// second says [`PROBE_DYNAMIC_COUNT`]. Which one the shader reads is
        /// decided entirely by the dynamic offset a bind passes, and the two
        /// produce different destinations.
        fn dynamic(device: &Dx12Device) -> Self {
            Self::build(device, true)
        }

        fn build(device: &Dx12Device, dynamic: bool) -> Self {
            let mut params = crcbl_shaders::compute_probe::Params {
                count: PROBE_ELEMENTS,
            }
            .to_bytes()
            .to_vec();
            if dynamic {
                // Zero padding out to the stride, then the second block. The
                // gap is never read: a root CBV reads `Params` from whichever
                // of the two addresses it was given.
                params.resize(PROBE_PARAMS_STRIDE as usize, 0);
                params.extend_from_slice(
                    &crcbl_shaders::compute_probe::Params {
                        count: PROBE_DYNAMIC_COUNT,
                    }
                    .to_bytes(),
                );
            }
            let source_bytes: Vec<u8> = probe_source()
                .iter()
                .flat_map(|value| value.to_le_bytes())
                .collect();

            let staged = device
                .create_buffer(&BufferDesc {
                    label: Some("compute probe upload"),
                    size: params.len() as u64 + probe_bytes(),
                    usage: BufferUsage::TRANSFER_SRC,
                    memory: MemoryLocation::HostUpload,
                })
                .expect("a staging buffer");
            device.write_buffer(staged, 0, &params).expect("write");
            device
                .write_buffer(staged, params.len() as u64, &source_bytes)
                .expect("write");

            let sentinel = device
                .create_buffer(&BufferDesc {
                    label: Some("compute probe sentinel"),
                    size: probe_bytes(),
                    usage: BufferUsage::TRANSFER_SRC,
                    memory: MemoryLocation::HostUpload,
                })
                .expect("a sentinel buffer");
            let sentinel_bytes: Vec<u8> =
                core::iter::repeat_n(PROBE_SENTINEL, PROBE_ELEMENTS as usize)
                    .flat_map(u32::to_le_bytes)
                    .collect();
            device
                .write_buffer(sentinel, 0, &sentinel_bytes)
                .expect("write");

            let device_buffer = |label, usage| {
                device
                    .create_buffer(&BufferDesc {
                        label: Some(label),
                        size: probe_bytes(),
                        usage,
                        memory: MemoryLocation::DeviceLocal,
                    })
                    .unwrap_or_else(|error| panic!("stage=create_buffer({label}): {error:?}"))
            };
            let params_buffer = device
                .create_buffer(&BufferDesc {
                    label: Some("compute probe params"),
                    size: params.len() as u64,
                    usage: BufferUsage::UNIFORM | BufferUsage::TRANSFER_DST,
                    memory: MemoryLocation::DeviceLocal,
                })
                .expect("a uniform buffer");
            let source = device_buffer(
                "compute probe source",
                BufferUsage::STORAGE | BufferUsage::TRANSFER_DST,
            );
            let destination = device_buffer(
                "compute probe destination",
                BufferUsage::STORAGE | BufferUsage::TRANSFER_DST | BufferUsage::TRANSFER_SRC,
            );
            let staging = readback_buffer(device, probe_bytes() as usize);

            run(device, |encoder| {
                encoder.pipeline_barrier(&Barriers {
                    buffers: &[
                        buffer_barrier(
                            params_buffer,
                            ResourceState::Undefined,
                            ResourceState::TransferDst,
                        ),
                        buffer_barrier(
                            source,
                            ResourceState::Undefined,
                            ResourceState::TransferDst,
                        ),
                    ],
                    ..Barriers::default()
                });
                encoder.copy_buffer_to_buffer(&BufferCopy {
                    src: staged,
                    src_offset: 0,
                    dst: params_buffer,
                    dst_offset: 0,
                    size: params.len() as u64,
                });
                encoder.copy_buffer_to_buffer(&BufferCopy {
                    src: staged,
                    src_offset: params.len() as u64,
                    dst: source,
                    dst_offset: 0,
                    size: probe_bytes(),
                });
                encoder.pipeline_barrier(&Barriers {
                    buffers: &[
                        buffer_barrier(
                            params_buffer,
                            ResourceState::TransferDst,
                            ResourceState::ShaderRead,
                        ),
                        buffer_barrier(
                            source,
                            ResourceState::TransferDst,
                            ResourceState::ShaderRead,
                        ),
                        buffer_barrier(
                            destination,
                            ResourceState::Undefined,
                            ResourceState::TransferSrc,
                        ),
                    ],
                    ..Barriers::default()
                });
            });
            device.destroy_buffer(staged);

            let set_layout = device
                .create_bind_group_layout(&BindGroupLayoutDesc {
                    label: Some("compute probe"),
                    entries: &[
                        BindGroupLayoutEntry {
                            binding: 0,
                            visibility: ShaderStages::COMPUTE,
                            kind: BindingKind::UniformBuffer { dynamic },
                            count: 1,
                            flags: BindingFlags::empty(),
                        },
                        BindGroupLayoutEntry {
                            binding: 1,
                            visibility: ShaderStages::COMPUTE,
                            kind: BindingKind::StorageBuffer {
                                read_only: true,
                                dynamic: false,
                            },
                            count: 1,
                            flags: BindingFlags::empty(),
                        },
                        BindGroupLayoutEntry {
                            binding: 2,
                            visibility: ShaderStages::COMPUTE,
                            kind: BindingKind::StorageBuffer {
                                read_only: false,
                                dynamic: false,
                            },
                            count: 1,
                            flags: BindingFlags::empty(),
                        },
                    ],
                })
                .expect("the probe's layout");
            let pipeline_layout = device
                .create_pipeline_layout(&PipelineLayoutDesc {
                    label: Some("compute probe"),
                    bind_group_layouts: &[set_layout],
                    push_constants: None,
                })
                .expect("a root signature with one descriptor table");
            let group = device
                .create_bind_group(&BindGroupDesc {
                    label: Some("compute probe"),
                    layout: set_layout,
                    entries: &[
                        BindGroupEntry {
                            binding: 0,
                            array_index: 0,
                            // **One block, not the whole buffer**, when the
                            // binding is dynamic: the offset is added on top of
                            // this one, and `offset + dynamic + size` has to
                            // stay inside the buffer. Bound whole, the second
                            // block would be out of range.
                            resource: if dynamic {
                                BindingResource::Buffer {
                                    buffer: params_buffer,
                                    offset: 0,
                                    size: crcbl_shaders::compute_probe::PARAMS_SIZE as u64,
                                }
                            } else {
                                BindingResource::whole_buffer(params_buffer)
                            },
                        },
                        BindGroupEntry {
                            binding: 1,
                            array_index: 0,
                            resource: BindingResource::whole_buffer(source),
                        },
                        BindGroupEntry {
                            binding: 2,
                            array_index: 0,
                            resource: BindingResource::whole_buffer(destination),
                        },
                    ],
                    variable_count: None,
                })
                .expect("a bind group over the probe's three buffers");

            let module = device
                .create_shader_module(&ShaderModuleDesc {
                    label: Some("compute_probe.slang"),
                    dxil: &crcbl_shaders::COMPUTE_PROBE.dxil_containers(),
                    ..ShaderModuleDesc::default()
                })
                .expect("the committed DXIL is accepted");
            let pipeline = device
                .create_compute_pipeline(&ComputePipelineDesc {
                    label: Some("compute probe"),
                    layout: pipeline_layout,
                    compute: ShaderEntry {
                        module,
                        entry_point: PROBE_ENTRY,
                    },
                    // The shader's own number, not a literal: `crcbl-shaders`
                    // checks this constant against the `[numthreads(…)]` in
                    // `compute_probe.slang`, and this backend checks it again
                    // against the container it is handing to D3D12.
                    workgroup_size: [crcbl_shaders::compute_probe::WORKGROUP_SIZE, 1, 1],
                })
                .unwrap_or_else(|error| panic!("stage=create_compute_pipeline: {error:?}"));
            device.destroy_shader_module(module);

            Self {
                params: params_buffer,
                source,
                destination,
                sentinel,
                staging,
                set_layout,
                group,
                pipeline_layout,
                pipeline,
                dynamic_offsets: if dynamic { vec![0] } else { Vec::new() },
            }
        }

        /// Resets the destination to the sentinel, runs `record` inside a
        /// compute pass, and reads the destination back.
        ///
        /// `record` is the *only* thing that varies between the direct and
        /// indirect tests, so both go through the same barriers and the same
        /// readback — and a difference in the result is a difference in the
        /// dispatch.
        ///
        /// `timestamps` brackets that compute pass. It is a parameter rather
        /// than something the timestamp test records for itself because the
        /// seam has no free-standing timestamp left: the two queries are named
        /// by the pass descriptor, and this method owns the descriptor. The
        /// reset the seam requires of every caller goes in beside it, outside
        /// the pass where the seam puts it.
        fn run(
            &self,
            device: &Dx12Device,
            timestamps: Option<crcbl_hal::PassTimestampWrites>,
            record: impl FnOnce(&mut dyn CommandEncoder),
        ) -> Vec<u32> {
            run(device, |encoder| {
                encoder.pipeline_barrier(&Barriers {
                    buffers: &[buffer_barrier(
                        self.destination,
                        ResourceState::TransferSrc,
                        ResourceState::TransferDst,
                    )],
                    ..Barriers::default()
                });
                encoder.copy_buffer_to_buffer(&BufferCopy {
                    src: self.sentinel,
                    src_offset: 0,
                    dst: self.destination,
                    dst_offset: 0,
                    size: probe_bytes(),
                });
                // `ShaderReadWrite`, not `ShaderWrite`: a barrier names the
                // access the *descriptor* permits rather than the one the
                // source performs, and an unordered-access view is both.
                encoder.pipeline_barrier(&Barriers {
                    buffers: &[buffer_barrier(
                        self.destination,
                        ResourceState::TransferDst,
                        ResourceState::ShaderReadWrite,
                    )],
                    ..Barriers::default()
                });

                if let Some(writes) = timestamps {
                    let first = writes.beginning_of_pass.min(writes.end_of_pass);
                    let last = writes.beginning_of_pass.max(writes.end_of_pass);
                    encoder.reset_query_set(writes.set, first..last + 1);
                }
                encoder.begin_compute_pass(&ComputePassDesc {
                    label: Some("compute probe"),
                    timestamp_writes: timestamps,
                });
                encoder.bind_compute_pipeline(self.pipeline);
                // Inside the pass, because the open scope is the only signal the
                // seam gives the backend about which bind point a group is for.
                encoder.bind_group(0, self.group, &self.dynamic_offsets, self.pipeline_layout);
                record(encoder);
                encoder.end_compute_pass();

                encoder.pipeline_barrier(&Barriers {
                    buffers: &[buffer_barrier(
                        self.destination,
                        ResourceState::ShaderReadWrite,
                        ResourceState::TransferSrc,
                    )],
                    ..Barriers::default()
                });
                encoder.copy_buffer_to_buffer(&BufferCopy {
                    src: self.destination,
                    src_offset: 0,
                    dst: self.staging,
                    dst_offset: 0,
                    size: probe_bytes(),
                });
            });

            let request = device
                .request_readback(&ReadbackDesc {
                    label: Some("compute probe readback"),
                    buffer: self.staging,
                    offset: 0,
                    size: probe_bytes(),
                    after: None,
                })
                .expect("a readback of a HostReadback buffer");
            let bytes = drain(device, request, probe_bytes() as usize);
            device.destroy_readback(request);
            bytes
                .chunks_exact(4)
                .map(|word| u32::from_le_bytes([word[0], word[1], word[2], word[3]]))
                .collect()
        }

        fn destroy(self, device: &Dx12Device) {
            device.destroy_compute_pipeline(self.pipeline);
            device.destroy_bind_group(self.group);
            device.destroy_pipeline_layout(self.pipeline_layout);
            device.destroy_bind_group_layout(self.set_layout);
            device.destroy_buffer(self.staging);
            device.destroy_buffer(self.sentinel);
            device.destroy_buffer(self.destination);
            device.destroy_buffer(self.source);
            device.destroy_buffer(self.params);
        }
    }

    /// The probe's entry point, which is also its container's file name.
    const PROBE_ENTRY: &str = "computeMain";

    /// One buffer barrier, on the one queue this backend creates.
    fn buffer_barrier(
        buffer: BufferHandle,
        from: ResourceState,
        to: ResourceState,
    ) -> crcbl_hal::BufferBarrier {
        crcbl_hal::BufferBarrier {
            buffer,
            from,
            to,
            queue_transfer: None,
        }
    }

    /// Compares a probe result against what the CPU says it should be, and says
    /// which element disagreed first.
    ///
    /// The element count is asserted before the values: a readback that came
    /// back short would otherwise satisfy a `zip` over nothing at all.
    fn assert_probe(actual: &[u32], expected: &[u32], what: &str) {
        assert_eq!(
            actual.len(),
            PROBE_ELEMENTS as usize,
            "{what}: the readback is not the whole destination buffer"
        );
        assert_eq!(expected.len(), actual.len(), "{what}: expectation length");
        if let Some((index, (got, want))) = actual
            .iter()
            .zip(expected)
            .enumerate()
            .find(|(_, (got, want))| got != want)
        {
            panic!(
                "{what}: element {index} is {got} ({got:#x}), expected {want} ({want:#x}). \
                 {} of {} elements were expected to be written.",
                expected
                    .iter()
                    .filter(|value| **value != PROBE_SENTINEL)
                    .count(),
                expected.len()
            );
        }
    }

    /// **A dynamic offset reaches the shader as a different constant buffer**,
    /// and the offset that proves it is not zero.
    ///
    /// The probe's uniform buffer holds two `Params` blocks
    /// [`PROBE_PARAMS_STRIDE`] apart, saying [`PROBE_ELEMENTS`] and
    /// [`PROBE_DYNAMIC_COUNT`]. One bind group, one buffer, one pipeline: the
    /// **only** difference between the two runs below is the number passed to
    /// `bind_group`, and the two destinations differ in more than a hundred
    /// elements. So this cannot pass with the offset dropped — that run is the
    /// first one, and it is asserted to look different.
    ///
    /// It is also the only test of the root parameter *index*. Binding 0 is a
    /// root CBV and bindings 1 and 2 are a descriptor table, so the set is two
    /// root parameters of different types; `SetComputeRootConstantBufferView` on
    /// the table's index, or the table on the root descriptor's, is a shader
    /// reading somewhere else entirely — which is exactly what the destination
    /// would show.
    ///
    /// # What only CI can settle
    ///
    /// All of it. `crcbl_dx12::root`'s own tests run anywhere and cover the
    /// arithmetic; that D3D12 accepts the signature and that the address
    /// arrives at the shader is a claim about a driver.
    #[test]
    #[ignore = "needs a real D3D12 device; run tests/run-dx12-e2e.sh"]
    fn a_dynamic_offset_binds_the_block_of_the_uniform_buffer_it_names() {
        let (_instance, device) = open_device();
        assert_eq!(
            device.caps().limits.min_uniform_buffer_offset_alignment,
            u64::from(PROBE_PARAMS_STRIDE),
            "the offsets below are built on this device's reported alignment"
        );

        let mut probe = ComputeProbe::dynamic(&device);

        // Offset zero: the first block, which says every element.
        probe.dynamic_offsets = vec![0];
        let whole = probe.run(&device, None, |encoder| {
            encoder.dispatch(PROBE_GROUPS, 1, 1);
        });
        assert_probe(&whole, &probe_expected(PROBE_ELEMENTS), "at offset 0");

        // The same everything, one number apart: the second block, which says
        // half the elements and one more.
        probe.dynamic_offsets = vec![PROBE_PARAMS_STRIDE];
        let half = probe.run(&device, None, |encoder| {
            encoder.dispatch(PROBE_GROUPS, 1, 1);
        });
        assert_probe(
            &half,
            &probe_expected(PROBE_DYNAMIC_COUNT),
            "at a non-zero dynamic offset",
        );
        assert!(
            half.contains(&PROBE_SENTINEL),
            "the second block's count leaves elements unwritten, and none were"
        );
        assert_ne!(
            whole, half,
            "the two runs differ only in the dynamic offset, so an offset that \
             was dropped makes them equal"
        );

        probe.destroy(&device);
    }

    /// **A pipeline layout that does not fit D3D12's root budget is refused
    /// here, by name, rather than at the draw that would have bound nothing.**
    ///
    /// The boundary is the assertion. A root descriptor costs two of the
    /// signature's 64 DWORDs, so 32 dynamic bindings are exactly the budget and
    /// 36 are over it — and the accepted half is what stops this from passing
    /// with a backend that refused every dynamic binding. `crcbl_dx12::root`'s
    /// own test covers the arithmetic on any host; this is the half that says
    /// the refusal reaches `create_pipeline_layout`, and that D3D12 really does
    /// serialise the signature the accepted arm asks for.
    #[test]
    #[ignore = "needs a real D3D12 device; run tests/run-dx12-e2e.sh"]
    fn a_root_signature_over_the_budget_is_refused_at_pipeline_layout_creation() {
        let (_instance, device) = open_device();
        assert!(
            device.caps().limits.max_bind_groups >= 4,
            "the counts below assume four sets are allowed"
        );

        let set_of = |bindings: u32| {
            let entries: Vec<BindGroupLayoutEntry> = (0..bindings)
                .map(|binding| BindGroupLayoutEntry {
                    binding,
                    visibility: ShaderStages::COMPUTE,
                    kind: BindingKind::UniformBuffer { dynamic: true },
                    count: 1,
                    flags: BindingFlags::empty(),
                })
                .collect();
            device
                .create_bind_group_layout(&BindGroupLayoutDesc {
                    label: Some("root budget"),
                    entries: &entries,
                })
                .expect("dynamic uniform buffers are a layout this backend plans")
        };
        // Four sets, because that is what `max_bind_groups` allows: eight
        // dynamic bindings each is 32 root descriptors and exactly 64 DWORDs;
        // nine each is 36 and 72.
        let fits = set_of(8);
        let over = set_of(9);

        device
            .create_pipeline_layout(&PipelineLayoutDesc {
                label: Some("exactly the budget"),
                bind_group_layouts: &[fits; 4],
                push_constants: None,
            })
            .map(|layout| device.destroy_pipeline_layout(layout))
            .expect("32 root descriptors are exactly D3D12's 64 root DWORDs");

        let error = device
            .create_pipeline_layout(&PipelineLayoutDesc {
                label: Some("over the budget"),
                bind_group_layouts: &[over; 4],
                push_constants: None,
            })
            .expect_err("36 root descriptors are 72 root DWORDs");
        let HalError::InvalidDescriptor(text) = &error else {
            panic!("a signature that does not fit is not {error:?}");
        };
        assert!(text.contains("72 root DWORD(s)"), "{text}");
        assert!(text.contains("holds 64"), "{text}");

        device.destroy_bind_group_layout(over);
        device.destroy_bind_group_layout(fits);
    }

    /// **D3D12 serialises the root-constants parameter this backend builds, and
    /// refuses to be talked into one it has no room for.**
    ///
    /// `crcbl_dx12::root`'s own tests check the register, the word count and the
    /// budget arithmetic on any host. This is the half that needs a device:
    /// `D3D12SerializeRootSignature` is what says a `D3D12_ROOT_CONSTANTS` with
    /// this `ShaderRegister` and `Num32BitValues` is a parameter at all, and it
    /// reports a malformed one as a blob of text nothing else in this crate
    /// would produce.
    ///
    /// The accepted arm is what stops this passing against a backend that
    /// refused every range — which is exactly what it did before this slice. The
    /// whole-budget arm is the one the reported limit makes reachable: the seam
    /// has no way to say "shared with your bind groups", so a caller may ask for
    /// all of `max_push_constant_size` and a table besides, and that has to be a
    /// refusal by name rather than a serialiser error.
    #[test]
    #[ignore = "needs a real D3D12 device; run tests/run-dx12-e2e.sh"]
    fn a_root_constants_parameter_serialises_and_its_budget_is_shared() {
        let (_instance, device) = open_device();
        let limit = device.caps().limits.max_push_constant_size;
        assert!(
            device.caps().features.contains(Features::PUSH_CONSTANTS) && limit > 0,
            "root constants are core D3D12, so every device must report them"
        );

        let range = |size| crcbl_hal::PushConstantRange {
            stages: ShaderStages::COMPUTE,
            offset: 0,
            size,
        };
        let layout = device
            .create_pipeline_layout(&PipelineLayoutDesc {
                label: Some("root constants"),
                bind_group_layouts: &[],
                push_constants: Some(range(16)),
            })
            .expect("D3D12 serialises a 4-DWORD root-constants parameter");
        device.destroy_pipeline_layout(layout);

        // The whole signature, spent on constants and nothing else: the ceiling
        // the adapter reports, which must be a signature D3D12 accepts or the
        // number is wrong.
        let layout = device
            .create_pipeline_layout(&PipelineLayoutDesc {
                label: Some("the whole root signature"),
                bind_group_layouts: &[],
                push_constants: Some(range(limit)),
            })
            .expect("max_push_constant_size must be a range D3D12 accepts");
        device.destroy_pipeline_layout(layout);
        still_alive(&device, "a root-constants signature");

        // And one byte past it is refused by the limit, before any signature is
        // serialised.
        let error = device
            .create_pipeline_layout(&PipelineLayoutDesc {
                label: Some("past the budget"),
                bind_group_layouts: &[],
                push_constants: Some(range(limit + 4)),
            })
            .expect_err("four bytes more than the whole root signature");
        let HalError::InvalidDescriptor(text) = &error else {
            panic!("a range past the limit is not {error:?}");
        };
        assert!(text.contains("max_push_constant_size"), "{text}");

        // A range that fits *alone* and not beside a bind group is the case only
        // the shared budget catches, and it names the DWORDs on both sides.
        let set = device
            .create_bind_group_layout(&BindGroupLayoutDesc {
                label: Some("one table"),
                entries: &[BindGroupLayoutEntry {
                    binding: 0,
                    visibility: ShaderStages::COMPUTE,
                    kind: BindingKind::StorageBuffer {
                        read_only: true,
                        dynamic: false,
                    },
                    count: 1,
                    flags: BindingFlags::empty(),
                }],
            })
            .expect("a one-binding layout");
        let error = device
            .create_pipeline_layout(&PipelineLayoutDesc {
                label: Some("the whole budget and a table"),
                bind_group_layouts: &[set],
                push_constants: Some(range(limit)),
            })
            .expect_err("the table costs a DWORD the constants already spent");
        let HalError::InvalidDescriptor(text) = &error else {
            panic!("a signature that does not fit is not {error:?}");
        };
        assert!(text.contains("push-constant word(s)"), "{text}");
        assert!(text.contains("holds 64"), "{text}");

        // The same table with a range small enough to leave room for it does
        // serialise, so the refusal above is the arithmetic rather than a
        // blanket refusal of the combination.
        let layout = device
            .create_pipeline_layout(&PipelineLayoutDesc {
                label: Some("a table and room for it"),
                bind_group_layouts: &[set],
                push_constants: Some(range(limit - 4)),
            })
            .expect("63 words and one table are exactly the budget");
        device.destroy_pipeline_layout(layout);
        device.destroy_bind_group_layout(set);
        still_alive(&device, "a signature with a table and root constants");
    }

    /// Panics unless the device is still alive and the debug layer logged
    /// nothing against it, naming `what` and quoting everything it did say.
    ///
    /// `GetDeviceRemovedReason` is what makes this a check rather than a hope:
    /// `DXGI_ERROR_DEVICE_REMOVED` is reported at the *next* call, so a test
    /// that only asserted its own call returned `Ok` passes with a device the
    /// runtime has already taken down. A `WARNING` is tolerated because it is
    /// not this test's subject; an `ERROR` is exactly what an invalid view is.
    fn still_alive(device: &Dx12Device, what: &str) {
        let diagnosis = debug::diagnosis(&device.inner.raw);
        assert!(
            !diagnosis.contains("GetDeviceRemovedReason"),
            "{what}: the device was removed{diagnosis}"
        );
        assert!(
            !diagnosis.contains("[ERROR]") && !diagnosis.contains("[CORRUPTION]"),
            "{what}: the debug layer objected{diagnosis}"
        );
    }

    /// **A uniform buffer smaller than D3D12's constant-buffer block gets a view
    /// over it and the device survives.**
    ///
    /// The reproduction of the call that killed the engine's D3D12 frame, at the
    /// two sizes it died at: `forward params` is 16 bytes and `forward cull
    /// params` is 112, and a constant buffer view's `SizeInBytes` has to be a
    /// multiple of 256. Rounding the view up over an unpadded resource is
    /// `CreateConstantBufferView` writing a view past the end of its buffer,
    /// which is `DXGI_ERROR_INVALID_CALL` and a removed device — reported at
    /// whatever call comes next, which is why [`still_alive`] asks the device
    /// rather than reading this call's own `Ok`.
    ///
    /// The storage buffer beside them is the other half: a writable binding is a
    /// UAV, which needs the resource created with `ALLOW_UNORDERED_ACCESS`.
    ///
    /// # What only CI can settle
    ///
    /// Whether D3D12 accepts the descriptors. `crcbl_dx12::buffer`'s own tests
    /// run on any host and cover the arithmetic; that a padded buffer is one the
    /// runtime will take a 256-byte view of is a claim about a driver.
    #[test]
    #[ignore = "needs a real D3D12 device; run tests/run-dx12-e2e.sh"]
    fn a_uniform_buffer_under_one_block_and_a_storage_buffer_both_get_views() {
        let (_instance, device) = open_device();

        // The two sizes from the frame, and one that is already a whole block.
        let uniforms: Vec<BufferHandle> = [16u64, 112, 256]
            .iter()
            .map(|&size| {
                device
                    .create_buffer(&BufferDesc {
                        label: Some("params"),
                        size,
                        usage: BufferUsage::UNIFORM,
                        memory: MemoryLocation::HostUpload,
                    })
                    .unwrap_or_else(|error| panic!("a {size}-byte uniform buffer: {error:?}"))
            })
            .collect();
        let counter = device
            .create_buffer(&BufferDesc {
                label: Some("visible count"),
                size: 4,
                usage: BufferUsage::STORAGE | BufferUsage::TRANSFER_SRC,
                memory: MemoryLocation::DeviceLocal,
            })
            .expect("a storage buffer a shader writes");
        still_alive(&device, "creating the buffers");

        let mut entries: Vec<BindGroupLayoutEntry> = (0..uniforms.len() as u32)
            .map(|binding| BindGroupLayoutEntry {
                binding,
                visibility: ShaderStages::COMPUTE,
                kind: BindingKind::UniformBuffer { dynamic: false },
                count: 1,
                flags: BindingFlags::empty(),
            })
            .collect();
        entries.push(BindGroupLayoutEntry {
            binding: uniforms.len() as u32,
            visibility: ShaderStages::COMPUTE,
            kind: BindingKind::StorageBuffer {
                read_only: false,
                dynamic: false,
            },
            count: 1,
            flags: BindingFlags::empty(),
        });
        let layout = device
            .create_bind_group_layout(&BindGroupLayoutDesc {
                label: Some("small uniforms"),
                entries: &entries,
            })
            .expect("uniform and storage buffers are a layout this backend builds");

        let mut bound: Vec<BindGroupEntry> = uniforms
            .iter()
            .enumerate()
            .map(|(binding, &buffer)| BindGroupEntry {
                binding: binding as u32,
                array_index: 0,
                resource: BindingResource::whole_buffer(buffer),
            })
            .collect();
        bound.push(BindGroupEntry {
            binding: uniforms.len() as u32,
            array_index: 0,
            resource: BindingResource::whole_buffer(counter),
        });
        let group = device
            .create_bind_group(&BindGroupDesc {
                label: Some("small uniforms"),
                layout,
                entries: &bound,
                variable_count: None,
            })
            .expect("a constant buffer view of a padded buffer and an unordered access view");
        still_alive(&device, "writing the descriptors");

        device.destroy_bind_group(group);
        device.destroy_bind_group_layout(layout);
        device.destroy_buffer(counter);
        for buffer in uniforms {
            device.destroy_buffer(buffer);
        }
    }

    /// **A host-visible buffer bound for writing is refused by name, and the
    /// device is untouched.**
    ///
    /// D3D12 has no unordered access view of an upload-heap resource: the flag
    /// is rejected at creation and the heap pins the resource to a state a
    /// shader cannot write from. The seam permits the combination because Vulkan
    /// does, so this backend answers with
    /// [`HalError::InvalidDescriptor`](crcbl_hal::HalError::InvalidDescriptor)
    /// at the binding that asked, rather than letting
    /// `CreateUnorderedAccessView` write nothing and take the device down at the
    /// next call.
    ///
    /// The read-only twin is the half that keeps the refusal honest: a shader
    /// resource view of the same buffer is legal and is what the engine's
    /// instance and table buffers take, so a backend that refused every
    /// host-visible storage binding would fail here.
    #[test]
    #[ignore = "needs a real D3D12 device; run tests/run-dx12-e2e.sh"]
    fn a_host_visible_buffer_is_refused_for_writing_and_accepted_for_reading() {
        let (_instance, device) = open_device();
        let staged = device
            .create_buffer(&BufferDesc {
                label: Some("instances"),
                size: 64,
                usage: BufferUsage::STORAGE,
                memory: MemoryLocation::HostUpload,
            })
            .expect("a host-visible storage buffer");

        let layout_of = |read_only: bool| {
            device
                .create_bind_group_layout(&BindGroupLayoutDesc {
                    label: Some("staged instances"),
                    entries: &[BindGroupLayoutEntry {
                        binding: 0,
                        visibility: ShaderStages::COMPUTE,
                        kind: BindingKind::StorageBuffer {
                            read_only,
                            dynamic: false,
                        },
                        count: 1,
                        flags: BindingFlags::empty(),
                    }],
                })
                .expect("a storage buffer binding")
        };
        let bind_to = |layout| {
            device.create_bind_group(&BindGroupDesc {
                label: Some("staged instances"),
                layout,
                entries: &[BindGroupEntry {
                    binding: 0,
                    array_index: 0,
                    resource: BindingResource::whole_buffer(staged),
                }],
                variable_count: None,
            })
        };

        let writable = layout_of(false);
        let error = bind_to(writable).expect_err("D3D12 has no UAV of an upload-heap buffer");
        let HalError::InvalidDescriptor(text) = &error else {
            panic!("a heap that cannot carry a UAV is not {error:?}");
        };
        assert!(text.contains("HostUpload"), "{text}");
        assert!(text.contains("DeviceLocal"), "{text}");
        still_alive(&device, "refusing the writable binding");

        let readable = layout_of(true);
        let group = bind_to(readable).expect("a shader resource view of an upload-heap buffer");
        still_alive(&device, "writing the read-only descriptor");

        device.destroy_bind_group(group);
        device.destroy_bind_group_layout(readable);
        device.destroy_bind_group_layout(writable);
        device.destroy_buffer(staged);
    }

    /// **A `workgroup_size` that is not the one the container declares is
    /// refused, and an indirect dispatch that would read past its buffer is
    /// too.**
    ///
    /// The first is the check the seam's field exists to make possible and that
    /// only a backend which can see `[numthreads(…)]` can make — see
    /// `crate::dxil`. Without it a caller that wrote `[32, 1, 1]` against this
    /// `[numthreads(64, 1, 1)]` shader would compute half as many groups as the
    /// work needs and leave the back half of every buffer untouched, on every
    /// backend at once.
    ///
    /// The second is the bounds check `ExecuteIndirect` does not do. Both are
    /// asserted beside a descriptor that *is* accepted, so neither is a backend
    /// refusing everything.
    #[test]
    #[ignore = "needs a real D3D12 device; run tests/run-dx12-e2e.sh"]
    fn a_workgroup_size_the_container_disagrees_with_is_refused_by_name() {
        let (_instance, device) = open_device();
        let probe = ComputeProbe::new(&device);
        let module = device
            .create_shader_module(&ShaderModuleDesc {
                label: Some("compute_probe.slang"),
                dxil: &crcbl_shaders::COMPUTE_PROBE.dxil_containers(),
                ..ShaderModuleDesc::default()
            })
            .expect("the committed DXIL is accepted");
        let desc = |workgroup_size| ComputePipelineDesc {
            label: Some("disagreeing probe"),
            layout: probe.pipeline_layout,
            compute: ShaderEntry {
                module,
                entry_point: PROBE_ENTRY,
            },
            workgroup_size,
        };

        let declared = crcbl_shaders::compute_probe::WORKGROUP_SIZE;
        let error = device
            .create_compute_pipeline(&desc([declared / 2, 1, 1]))
            .expect_err("half the declared thread count is not the shader's own number");
        let HalError::ShaderCompilation(text) = &error else {
            panic!("a descriptor that disagrees with the artifact is not {error:?}");
        };
        assert!(text.contains("numthreads"), "{text}");

        // The same descriptor with the shader's own number is accepted, so the
        // refusal above is the comparison firing rather than this backend
        // refusing every compute pipeline.
        let agreeing = device
            .create_compute_pipeline(&desc([declared, 1, 1]))
            .expect("the container's own thread count");
        device.destroy_compute_pipeline(agreeing);
        device.destroy_shader_module(module);

        let queue = device
            .queue(QueueKind::Graphics)
            .expect("the graphics queue exists");
        let short = device
            .create_buffer(&BufferDesc {
                label: Some("too short for dispatch arguments"),
                size: 8,
                usage: BufferUsage::INDIRECT,
                memory: MemoryLocation::DeviceLocal,
            })
            .expect("an eight-byte buffer");
        let mut encoder = device.create_command_encoder(&CommandEncoderDesc { label: None, queue });
        encoder.begin_compute_pass(&ComputePassDesc {
            label: None,
            timestamp_writes: None,
        });
        encoder.bind_compute_pipeline(probe.pipeline);
        encoder.dispatch_indirect(short, 0);
        encoder.end_compute_pass();
        let error = encoder
            .finish()
            .expect_err("twelve bytes of arguments do not fit in eight");
        let HalError::InvalidDescriptor(text) = &error else {
            panic!("a short argument buffer is not {error:?}");
        };
        assert!(text.contains("dispatch_indirect"), "{text}");

        device.destroy_buffer(short);
        probe.destroy(&device);
    }

    /// The compute scope's own rules, at record time.
    ///
    /// `crcbl-hal`'s null recorder rejects a nested pass, an unclosed one and a
    /// dispatch outside one, and the seam says a backend "may assume these
    /// hold". This backend does not merely assume: it reports all three, so a
    /// graph that mis-nests fails here rather than recording a dispatch onto the
    /// graphics bind point — which D3D12 would accept and quietly get wrong.
    #[test]
    #[ignore = "needs a real D3D12 device; run tests/run-dx12-e2e.sh"]
    fn compute_commands_outside_a_pass_and_with_no_pipeline_are_refused() {
        let (_instance, device) = open_device();
        let queue = device
            .queue(QueueKind::Graphics)
            .expect("the graphics queue exists");

        type Refused = (&'static str, &'static str, fn(&mut dyn CommandEncoder));
        let cases: &[Refused] = &[
            ("outside a compute pass", "dispatch", |encoder| {
                encoder.dispatch(1, 1, 1);
            }),
            (
                "outside a compute pass",
                "bind_compute_pipeline",
                |encoder| {
                    encoder.bind_compute_pipeline(unissued());
                },
            ),
            ("no compute pipeline bound", "dispatch", |encoder| {
                encoder.begin_compute_pass(&ComputePassDesc {
                    label: None,
                    timestamp_writes: None,
                });
                encoder.dispatch(1, 1, 1);
            }),
            (
                "no compute pipeline bound",
                "dispatch_indirect",
                |encoder| {
                    encoder.begin_compute_pass(&ComputePassDesc {
                        label: None,
                        timestamp_writes: None,
                    });
                    encoder.dispatch_indirect(unissued(), 0);
                },
            ),
            ("do not nest", "a nested compute pass", |encoder| {
                encoder.begin_compute_pass(&ComputePassDesc {
                    label: None,
                    timestamp_writes: None,
                });
                encoder.begin_compute_pass(&ComputePassDesc {
                    label: None,
                    timestamp_writes: None,
                });
            }),
            ("compute pass still open", "an unclosed pass", |encoder| {
                encoder.begin_compute_pass(&ComputePassDesc {
                    label: None,
                    timestamp_writes: None,
                });
            }),
        ];
        assert!(!cases.is_empty(), "nothing to check");
        for (expected, what, record) in cases {
            let mut encoder =
                device.create_command_encoder(&CommandEncoderDesc { label: None, queue });
            record(encoder.as_mut());
            let Err(error) = encoder.finish() else {
                panic!("{what} recorded successfully, so the encoder reported a lie");
            };
            let HalError::InvalidDescriptor(text) = &error else {
                panic!("{what}: a mis-scoped command is not {error:?}");
            };
            assert!(text.contains(expected), "{what}: {text}");
        }

        // A well-formed empty pass still records on the same device, so the six
        // failures above are the shape of each call rather than the encoder.
        let mut good = device.create_command_encoder(&CommandEncoderDesc {
            label: Some("well-formed compute pass"),
            queue,
        });
        good.begin_compute_pass(&ComputePassDesc {
            label: Some("empty"),
            timestamp_writes: None,
        });
        good.end_compute_pass();
        let commands = good.finish().expect("an empty compute pass records");
        device
            .submit(queue, &SubmitInfo::new(&[commands]))
            .expect("submit");
        device.wait_idle().expect("idle");
        device.destroy_command_buffer(commands);
    }
}
