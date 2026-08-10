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
//! and [`Device::wait_idle`], and the presentation block: swapchains, acquire,
//! present and the present wait. Everything else on the trait refuses with
//! [`HalError::Unsupported`] whose `what` names the slice it arrives in, in the
//! same voice `Dx12Instance` established. Nothing here is a stub that reports
//! success.
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

use std::sync::{Arc, Mutex, MutexGuard};
use std::time::Duration;

use crcbl_core::Pool;
use crcbl_hal::{
    AcquiredFrame, BackendKind, BindGroupDesc, BindGroupEntry, BindGroupHandle,
    BindGroupLayoutDesc, BindGroupLayoutHandle, BindingResource, BufferDesc, BufferHandle,
    CommandBufferHandle, CommandEncoder, CommandEncoderDesc, ComputePipelineDesc,
    ComputePipelineHandle, Device, DeviceCaps, DeviceDesc, DisplayTiming, Extent3d, Format,
    GraphicsPipelineDesc, GraphicsPipelineHandle, HalError, ImageDesc, ImageHandle,
    ImageSubresourceRange, ImageType, ImageUsage, ImageViewDesc, ImageViewHandle, ImageViewType,
    MemoryLocation, PipelineLayoutDesc, PipelineLayoutHandle, PresentInfo, QuerySetDesc,
    QuerySetHandle, QueueHandle, QueueKind, ReadbackDesc, ReadbackHandle, ReadbackState,
    SamplerDesc, SamplerHandle, SemaphoreDesc, SemaphoreHandle, ShaderModuleDesc,
    ShaderModuleHandle, SubmitInfo, SurfaceError, SwapchainDesc, SwapchainHandle,
};
use windows::Win32::Foundation::{CloseHandle, E_OUTOFMEMORY, WAIT_OBJECT_0};
use windows::Win32::Graphics::Direct3D::{D3D_FEATURE_LEVEL_11_0, D3D_PRIMITIVE_TOPOLOGY};
use windows::Win32::Graphics::Direct3D12::{
    D3D12_COMMAND_LIST_TYPE_DIRECT, D3D12_COMMAND_QUEUE_DESC, D3D12_COMMAND_QUEUE_FLAG_NONE,
    D3D12_COMMAND_QUEUE_PRIORITY_NORMAL, D3D12_COMMAND_SIGNATURE_DESC,
    D3D12_COMPARISON_FUNC_ALWAYS, D3D12_CPU_DESCRIPTOR_HANDLE, D3D12_CPU_PAGE_PROPERTY_UNKNOWN,
    D3D12_FENCE_FLAG_NONE, D3D12_GPU_DESCRIPTOR_HANDLE, D3D12_HEAP_FLAG_NONE,
    D3D12_HEAP_PROPERTIES, D3D12_INDIRECT_ARGUMENT_DESC, D3D12_INDIRECT_ARGUMENT_TYPE_DISPATCH,
    D3D12_INDIRECT_ARGUMENT_TYPE_DRAW, D3D12_INDIRECT_ARGUMENT_TYPE_DRAW_INDEXED,
    D3D12_MEMORY_POOL_UNKNOWN, D3D12_RANGE, D3D12_RESOURCE_BARRIER_ALL_SUBRESOURCES,
    D3D12_RESOURCE_DESC, D3D12_RESOURCE_DIMENSION_BUFFER, D3D12_ROOT_PARAMETER_TYPE,
    D3D12_SAMPLER_DESC, D3D12_TEXTURE_LAYOUT_ROW_MAJOR, D3D12_TEXTURE_LAYOUT_UNKNOWN,
    D3D12CreateDevice, ID3D12CommandAllocator, ID3D12CommandList, ID3D12CommandQueue,
    ID3D12CommandSignature, ID3D12DescriptorHeap, ID3D12Device, ID3D12Fence,
    ID3D12GraphicsCommandList, ID3D12Object, ID3D12PipelineState, ID3D12Resource,
    ID3D12RootSignature,
};
use windows::Win32::Graphics::Dxgi::Common::{DXGI_FORMAT_UNKNOWN, DXGI_SAMPLE_DESC};
use windows::Win32::System::Threading::{CreateEventW, INFINITE, WaitForSingleObject};
use windows::core::{Interface, PCWSTR};

use crate::binding::{self, BindGroupLayoutRecord, BindGroupRecord, VisibleHeaps};
use crate::command::Dx12CommandEncoder;
use crate::debug;
use crate::descriptor::{Descriptors, Kind, Slot};
use crate::draw::IndirectKind;
use crate::dxil::ShaderModuleEntry;
use crate::handle::{self, Owned, Owner};
use crate::instance::{AdapterRecord, InstanceInner, OFFSCREEN_HWND, next_owner_id, not_yet};
use crate::pipeline::{self, ComputePipelineEntry, GraphicsPipelineEntry, PipelineLayoutEntry};
use crate::present::{self, PresentWait};
use crate::retire::RetireQueue;
use crate::swapchain::{self, SwapchainEntry};
use crate::view::Subresource;
use crate::{buffer, conv, validate};

/// Node mask naming the single adapter node the seam models.
///
/// `crcbl-hal` has no multi-adapter vocabulary at all, so a linked-node rig is
/// described as its first node rather than wrongly. D3D12 wants a *mask* for
/// creation and visibility, not the index `D3D12_FEATURE_DATA_ARCHITECTURE1`
/// takes, which is why this is a bit and not a zero.
const FIRST_NODE: u32 = 1;

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
    /// The image's format, which is also the view's — `create_image_view`
    /// refuses a differing one. Kept so a render pass can ask whether a
    /// depth-stencil attachment has a stencil plane to clear without resolving
    /// the image handle it no longer has.
    format: Format,
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
    /// The fence value that covers the work this readback observes: the highest
    /// one handed out when [`Device::request_readback`] was called, which is
    /// exactly "everything submitted to this device before this call".
    after: u64,
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
}

owned!(
    BufferEntry,
    ImageEntry,
    ViewEntry,
    SamplerEntry,
    CommandBufferEntry,
    ReadbackEntry,
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
}

impl ImageRef {
    /// The extent of one mip level, in texels.
    ///
    /// A mip halves each *spatial* dimension and rounds up to one; an array's
    /// layer count is not a spatial dimension and does not change, which is the
    /// distinction `depth_or_layers` folds together and this has to unfold.
    pub(crate) fn mip_extent(&self, mip: u32) -> (u32, u32, u32) {
        let halve = |size: u32| (size >> mip.min(31)).max(1);
        let depth = if matches!(self.image_type, ImageType::D3) {
            halve(self.extent.depth_or_layers)
        } else {
            1
        };
        (halve(self.extent.width), halve(self.extent.height), depth)
    }

    /// D3D12's subresource index for a mip and array layer.
    ///
    /// Plane zero: this backend copies colour images only, and a colour format
    /// has one plane. `D3D12CalcSubresource` is `mip + layer * mip_levels +
    /// plane * mip_levels * layers`, and the plane term is what a depth copy
    /// would need.
    pub(crate) fn subresource(&self, mip: u32, layer: u32) -> u32 {
        mip + layer * self.mip_levels
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
    pub(crate) topology: D3D_PRIMITIVE_TOPOLOGY,
    pub(crate) stencil_reference: Option<u32>,
}

/// A compute pipeline resolved to what the command list must be told.
///
/// The same pair as [`BoundPipeline`] and for the same reason, minus the two
/// pieces of graphics state D3D12 keeps outside a pipeline state object: a
/// dispatch has no topology and no stencil reference.
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

/// A render pass attachment: its descriptor, and the resource behind it.
#[derive(Debug)]
pub(crate) struct AttachmentRef {
    pub(crate) descriptor: D3D12_CPU_DESCRIPTOR_HANDLE,
    pub(crate) image: ID3D12Resource,
    pub(crate) format: Format,
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
        })
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
        let (slot, image, format) = {
            let entry = handle::lookup(&state.views, "image view", view, self.owner)?;
            let slot = if depth {
                entry.descriptors.depth_stencil
            } else {
                entry.descriptors.render_target
            };
            (slot, entry.image.clone(), entry.format)
        };
        let Some(slot) = slot else {
            let usage = if depth {
                "ImageUsage::DEPTH_STENCIL_ATTACHMENT"
            } else {
                "ImageUsage::COLOR_ATTACHMENT"
            };
            return Err(HalError::InvalidDescriptor(format!(
                "this view of a {format:?} image has no attachment descriptor, because its image \
                 was not created with {usage}"
            )));
        };
        Ok(AttachmentRef {
            descriptor: state.descriptors.cpu_handle(slot),
            image,
            format,
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
            stencil_reference: entry.stencil_reference,
        })
    }

    /// The command signature one indirect layout and stride is executed
    /// through, created on first use.
    ///
    /// **A command signature holding only `DISPATCH`, `DRAW` or `DRAW_INDEXED`
    /// takes no root signature.** `CreateCommandSignature`'s second argument is
    /// required only when the argument layout writes root arguments — a
    /// constant, a root CBV, a vertex or index buffer view — and none of these
    /// three writes any, so one object is valid against every pipeline this
    /// device has. That is what makes the cache key `(kind, stride)` rather than
    /// the pipeline.
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

    /// Blocks until the fence has reached `value`.
    ///
    /// # The wait uses a real event, and checks that it waited
    ///
    /// `SetEventOnCompletion` accepts a null handle and is documented to block
    /// until the value is reached, which would be less code. The event is used
    /// anyway because it is the version that can be *checked*:
    /// `WaitForSingleObject` reports which way it returned, so a wait that did
    /// not happen is an `Err` here rather than a wait that silently does not
    /// wait — and a silent one is worse than none, because it would be trusted
    /// at shutdown.
    ///
    /// The event is created and closed inside the call rather than kept on the
    /// device. Two reasons, and the first is enough: an auto-reset event shared
    /// between two concurrent waiters lets one consume the other's signal, and a
    /// Win32 `HANDLE` is a raw pointer that `windows-rs` declares neither `Send`
    /// nor `Sync`, so storing one would cost this module the marker impl it
    /// otherwise does not need.
    fn wait_for(&self, value: u64) -> Result<(), HalError> {
        if self.completed() >= value {
            return Ok(());
        }
        // SAFETY: no security attributes, auto-reset, initially unsignalled,
        // unnamed. Every argument is a scalar or a null pointer the API
        // documents as optional.
        let event = unsafe { CreateEventW(None, false, false, PCWSTR::null()) }
            .map_err(|error| HalError::DeviceLost(format!("CreateEventW failed: {error}")))?;
        // SAFETY: `event` is the handle just created and `value` is one this
        // device signalled. The runtime signals the event when the fence reaches
        // the value, including immediately if it already has.
        let armed = unsafe { self.fence.SetEventOnCompletion(value, event) };
        let waited = if armed.is_ok() {
            // SAFETY: `event` is a live event handle owned by this call.
            Some(unsafe { WaitForSingleObject(event, INFINITE) })
        } else {
            None
        };
        // SAFETY: `event` is this call's handle and is not used again. Closed on
        // both paths, so a failed `SetEventOnCompletion` leaks nothing.
        if let Err(error) = unsafe { CloseHandle(event) } {
            log::debug!("crcbl-dx12: could not close a fence wait event: {error}");
        }
        armed.map_err(|error| {
            HalError::DeviceLost(format!(
                "SetEventOnCompletion failed: {error}{}",
                debug::diagnosis(&self.raw)
            ))
        })?;
        if waited != Some(WAIT_OBJECT_0) {
            return Err(HalError::DeviceLost(format!(
                "waiting for fence value {value} returned {waited:?} rather than WAIT_OBJECT_0"
            )));
        }
        let completed = self.completed();
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
        Ok(())
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
            log::error!(
                "crcbl-dx12: a device was dropped without reaching fence {target}: {error}"
            );
        }
        let mut state = self.state();
        let pending = state.retire.pending();
        if pending > 0 {
            log::debug!("crcbl-dx12: releasing {pending} retired batches at device teardown");
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
        log::debug!("crcbl-dx12: could not name an object \"{label}\": {error}");
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
        let caps = record.info.caps;
        let owner = Owner::new(next_owner_id());
        let state = DeviceState {
            buffers: Pool::new(),
            images: Pool::new(),
            views: Pool::new(),
            samplers: Pool::new(),
            command_buffers: Pool::new(),
            readbacks: Pool::new(),
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
            owner,
            state: Mutex::new(state),
        });
        log::info!(
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
                    memory: MemoryLocation::DeviceLocal,
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
            present_mode: present::resolve_offscreen_present_mode(desc.present_mode),
            flags: swapchain::NO_SWAP_CHAIN_FLAGS,
            images,
            views,
            ledger: present::PresentLedger::default(),
        };
        log::info!(
            "crcbl-dx12: offscreen ring {}x{} {:?}, {count} image(s)",
            extent.0,
            extent.1,
            desc.format,
        );
        let handle = self.state().swapchains.insert(entry);
        Ok(handle::stamp(self.inner.owner, handle))
    }
}

impl Device for Dx12Device {
    fn backend(&self) -> BackendKind {
        BackendKind::Dx12
    }

    fn caps(&self) -> DeviceCaps {
        self.inner.caps
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
        let written = D3D12_RANGE {
            Begin: begin,
            End: begin + data.len(),
        };
        // Begin == End says "the CPU read nothing", which is what this call
        // does.
        let read_nothing = D3D12_RANGE { Begin: 0, End: 0 };
        let mut mapped: *mut core::ffi::c_void = core::ptr::null_mut();
        // SAFETY: `entry.raw` is a live buffer on a host-visible heap — checked
        // above — and subresource 0 is the only one a buffer has. Both range
        // pointers name live locals, and `mapped` is a live pointer the call
        // writes through.
        unsafe { entry.raw.Map(0, Some(&read_nothing), Some(&mut mapped)) }
            .map_err(|error| HalError::Backend(format!("ID3D12Resource::Map failed: {error}")))?;
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
    /// # `after` is refused rather than ignored
    ///
    /// [`ReadbackDesc::after`] names a semaphore, and [`Device::create_semaphore`]
    /// refuses — so no handle a caller can pass was ever issued, and the answer
    /// is [`HalError::InvalidHandle`]. Ignoring it would be the bad kind of
    /// silence: the readback would resolve against the wrong completion point
    /// and hand back whatever happened to be in the buffer.
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
        if let Some(wait) = desc.after {
            return Err(HalError::invalid_handle("semaphore", wait.semaphore));
        }
        // "Everything submitted to this device before this call" is exactly the
        // highest fence value handed out, so an unqualified request needs no
        // synchronisation object at all.
        let after = state.next_fence_value;
        let handle = state.readbacks.insert(ReadbackEntry {
            owner: self.inner.owner.id,
            buffer: desc.buffer,
            offset: desc.offset,
            size: desc.size,
            after,
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
    /// The buffer is re-resolved from the handle stored at request time rather
    /// than kept as a pointer, so a buffer destroyed between the request and the
    /// poll fails lookup instead of having a freed mapping read.
    fn poll_readback(
        &self,
        readback: ReadbackHandle,
        out: &mut [u8],
    ) -> Result<ReadbackState, HalError> {
        let mut state = self.state();
        let (buffer, offset, size, after) = {
            let entry = handle::lookup(&state.readbacks, "readback", readback, self.inner.owner)?;
            (entry.buffer, entry.offset, entry.size, entry.after)
        };
        if out.len() as u64 != size {
            return Err(HalError::InvalidDescriptor(format!(
                "poll_readback needs exactly {size} bytes, got {}",
                out.len()
            )));
        }
        if self.inner.completed() < after {
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
        unsafe { entry.raw.Map(0, Some(&read), Some(&mut mapped)) }
            .map_err(|error| HalError::Backend(format!("ID3D12Resource::Map failed: {error}")))?;
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
        let properties = heap_properties(desc.memory);
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
                conv::initial_state(desc.memory),
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
        let (image, image_format, image_type, usage, levels, slices, samples) = {
            let entry = handle::lookup(&state.images, "image", desc.image, self.inner.owner)?;
            (
                entry.raw.clone(),
                entry.format,
                entry.image_type,
                entry.usage,
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
            format: image_format,
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
            // A non-comparison sampler ignores this field, but the debug layer
            // reads a zero as `D3D12_COMPARISON_FUNC_NONE`, which is a sampler
            // feedback value and not a comparison at all. `ALWAYS` is the
            // neutral one.
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
    /// The mesh-stage check is here rather than in `plan_layout` because it
    /// reads the *device* rather than the descriptor: this backend reports no
    /// `Features::MESH_SHADER`, so a layout naming the mesh stage is refused
    /// here — `conv::shader_visibility` would otherwise widen it to
    /// `D3D12_SHADER_VISIBILITY_ALL` and say nothing.
    fn create_bind_group_layout(
        &self,
        desc: &BindGroupLayoutDesc<'_>,
    ) -> Result<BindGroupLayoutHandle, HalError> {
        for entry in desc.entries {
            entry
                .visibility
                .check_supported(self.inner.caps.features, BackendKind::Dx12)?;
        }
        let record = binding::plan_layout(desc, self.inner.owner.id)?;
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
        let entry = pipeline::layout(&self.inner.raw, desc, &sets, self.inner.owner.id)?;
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

    /// Still refused, and the obstacle is this backend rather than D3D12.
    ///
    /// D3D12 has amplification and mesh shaders from SM6.5, and the DXIL is
    /// already committed — `crates/crcbl-shaders/dxil/mesh_shader.*.dxil`, at
    /// `ms_6_6` and `as_6_6`. What is missing here is the backend: a mesh
    /// pipeline needs the `D3D12_PIPELINE_STATE_STREAM_DESC` path rather than
    /// the fixed `D3D12_GRAPHICS_PIPELINE_STATE_DESC` `pipeline::graphics`
    /// fills, and the draw is `DispatchMesh` on an `ID3D12GraphicsCommandList6`.
    ///
    /// This backend accordingly reports no `Features::MESH_SHADER`.
    fn create_mesh_pipeline(
        &self,
        _desc: &crcbl_hal::MeshPipelineDesc<'_>,
    ) -> Result<GraphicsPipelineHandle, HalError> {
        Err(not_yet(
            "mesh pipelines: D3D12 has the stages and the DXIL is committed, but this backend \
             builds no pipeline state stream (the DX12 mesh slice)",
        ))
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

    fn create_query_set(&self, _desc: &QuerySetDesc<'_>) -> Result<QuerySetHandle, HalError> {
        Err(not_yet("query sets (the DX12 query slice)"))
    }

    fn destroy_query_set(&self, _set: QuerySetHandle) {}

    fn query_results(
        &self,
        set: QuerySetHandle,
        _first_query: u32,
        _out: &mut [u64],
    ) -> Result<(), HalError> {
        // The seam says a device without `TIMESTAMP_QUERY` returns zeros rather
        // than failing, so the profiler HUD degrades instead of breaking — but
        // that applies to a query set that exists. None can, so the handle
        // cannot resolve, and `InvalidHandle` is the honest answer.
        Err(HalError::invalid_handle("query set", set))
    }

    // --- synchronisation ---

    fn create_semaphore(&self, _desc: &SemaphoreDesc<'_>) -> Result<SemaphoreHandle, HalError> {
        // `ID3D12Fence` is the seam's timeline semaphore almost verbatim, and
        // `wait_idle` below already drives one. What is missing is the other
        // half — `ID3D12CommandQueue::Wait`, and a submission to attach it to —
        // so a semaphore handed out now would be a counter nothing can signal
        // from the GPU.
        Err(not_yet("semaphores (the DX12 command slice)"))
    }

    fn destroy_semaphore(&self, _semaphore: SemaphoreHandle) {}

    fn semaphore_value(&self, semaphore: SemaphoreHandle) -> Result<u64, HalError> {
        Err(HalError::invalid_handle("semaphore", semaphore))
    }

    fn wait_semaphores(
        &self,
        _waits: &[crcbl_hal::SemaphoreWait],
        _timeout_ns: u64,
    ) -> Result<bool, HalError> {
        Err(not_yet("semaphores (the DX12 command slice)"))
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
    /// # Waits and signals are refused, not ignored
    ///
    /// A [`SemaphoreWait`](crcbl_hal::SemaphoreWait) or
    /// [`SemaphoreSignal`](crcbl_hal::SemaphoreSignal) names a semaphore, and
    /// [`Device::create_semaphore`] refuses — so no handle in either list was
    /// ever issued by any device, and the honest answer is
    /// [`HalError::InvalidHandle`] rather than a refusal naming the slice.
    /// Accepting them and doing nothing is the failure worth avoiding: the
    /// caller believes the ordering it asked for exists.
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
        if let Some(wait) = submit.waits.first() {
            return Err(HalError::invalid_handle("semaphore", wait.semaphore));
        }
        if let Some(signal) = submit.signals.first() {
            return Err(HalError::invalid_handle("semaphore", signal.semaphore));
        }

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
            held.push(Retired::Recording {
                _list: entry.list.clone(),
                _allocator: entry.allocator.clone(),
            });
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
            log::error!("crcbl-dx12: a swapchain was destroyed with the queue unfinished: {error}");
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
        let entry = handle::lookup(&state.swapchains, "swapchain", swapchain, owner)?;
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
            // `acquire_next_frame` hands out no present semaphore, so a caller
            // following the seam splices an empty slice here. Anything else is
            // a handle no device issued.
            return Err(SurfaceError::Hal(HalError::invalid_handle(
                "semaphore",
                *wait,
            )));
        }
        let owner = self.inner.owner;
        // Resolved, then released: `Present` blocks when the frame queue is
        // full, and this device has one lock over every table.
        let (raw, mode) = {
            let mut state = self.state();
            let entry =
                handle::lookup_mut(&mut state.swapchains, "swapchain", present.swapchain, owner)?;
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
            log::debug!("crcbl-dx12: a swapchain went away during a present, so id {id} is lost");
            return Ok(());
        };
        if !entry.ledger.record_present(id) {
            log::debug!(
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

    use crate::Dx12Instance;
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
    pub(crate) fn open_device() -> (Dx12Instance, Dx12Device) {
        let instance = open_instance();
        let adapter = pinned_adapter(&instance);
        let device = instance
            .open_device(&device_desc(adapter))
            .expect("a D3D12 device opens with no required features");
        (instance, device)
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
            memory: MemoryLocation::DeviceLocal,
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

    /// **The deliverable of this slice, and the measurement `docs/backlog.md`
    /// asked for.**
    ///
    /// A render pass with [`LoadOp::Clear`] writes the attachment, a copy moves
    /// it into a readback buffer, a submission runs both, and a poll reads the
    /// bytes back. Everything in the chain is real: `OMSetRenderTargets`,
    /// `ClearRenderTargetView`, two resource transitions, `CopyTextureRegion`,
    /// `ExecuteCommandLists` and an `ID3D12Fence`.
    ///
    /// **What would make it fail.** A clear that never happened leaves
    /// [`POISON`], because the readback buffer is filled with it before the
    /// submission — so this cannot pass against a copy that moved nothing. A
    /// clear that reached the wrong channels fails on the texel rather than on
    /// the length, because no two channels of [`CLEAR_TEXEL`] are equal. A copy
    /// that got the row pitch wrong fails on the later rows, because every row
    /// is asserted rather than the first texel.
    ///
    /// **What it settles.** Whether WARP can execute anything at all. Reporting
    /// `ResourceBindingTier=3` and `HighestShaderModel=6.8` is a claim about the
    /// API surface; this is the pixel.
    #[test]
    #[ignore = "needs a real D3D12 device; run tests/run-dx12-e2e.sh"]
    fn a_d3d12_render_pass_clear_reads_back_the_exact_texels() {
        let (_instance, device) = open_device();
        let (target, view) = color_target(&device);
        let readback = readback_buffer(&device, TARGET_BYTES);

        let pass = clear_pass(
            view,
            CLEAR,
            LoadOp::Clear,
            Rect2d::from_size(TARGET.width, TARGET.height),
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
            encoder.copy_image_to_buffer(&whole_image_copy(readback, 0, target));
        });

        let request = device
            .request_readback(&ReadbackDesc {
                label: Some("crcbl-dx12 clear readback"),
                buffer: readback,
                offset: 0,
                size: TARGET_BYTES as u64,
                after: None,
            })
            .expect("a readback of a HostReadback buffer");
        let bytes = drain(&device, request, TARGET_BYTES);
        assert_eq!(
            &bytes[..4],
            &CLEAR_TEXEL,
            "the first texel is {:?}, not the colour the pass cleared to",
            &bytes[..4]
        );
        assert_eq!(
            bytes,
            expected(CLEAR_TEXEL),
            "the clear did not reach every texel of the attachment"
        );

        device.destroy_readback(request);
        device.destroy_buffer(readback);
        device.destroy_image_view(view);
        device.destroy_image(target);
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
            }
        }

        /// Clears the target, records `record` inside the pass with the
        /// pipeline, group and index buffer already bound, and reads the frame
        /// back.
        ///
        /// The readback is re-poisoned first, so a run that copied nothing is a
        /// frame of [`POISON`] rather than the previous run's picture — which is
        /// what stops one green draw making every later assertion vacuous.
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
                        ResourceState::Undefined,
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
        let (_instance, device) = open_device();
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

    /// A recycled slot must not resurrect the handle that used to name it.
    #[test]
    #[ignore = "needs a real D3D12 device; run tests/run-dx12-e2e.sh"]
    fn a_destroyed_d3d12_handle_does_not_alias_the_buffer_that_replaces_it() {
        let (_instance, device) = open_device();
        let first = device
            .create_buffer(&buffer(256, MemoryLocation::HostUpload))
            .expect("first buffer");
        device.destroy_buffer(first);
        let second = device
            .create_buffer(&buffer(256, MemoryLocation::HostUpload))
            .expect("second buffer");

        assert_ne!(
            first, second,
            "the pool reissued the identical handle, so the generation never moved"
        );
        assert_eq!(
            first.index(),
            second.index(),
            "the free list should have handed back the same slot; if not, this test is not \
             exercising recycling at all"
        );
        device
            .write_buffer(second, 0, &[1u8; 4])
            .expect("the live handle resolves");
        let error = device
            .write_buffer(first, 0, &[1u8; 4])
            .expect_err("the dead handle must not name its replacement");
        assert!(matches!(error, HalError::InvalidHandle { .. }), "{error:?}");
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

    /// **Obligation 3.** A handle from device A used on device B is a caller
    /// bug that must be *detected*.
    ///
    /// Device B is given a buffer of its own first, and that is the whole design
    /// of this test: without the device tag in the handle, A's first handle and
    /// B's first handle are bit-identical, so B would resolve A's handle to B's
    /// own buffer, find the owner matching, and write into the wrong object with
    /// no error anywhere.
    #[test]
    #[ignore = "needs a real D3D12 device; run tests/run-dx12-e2e.sh"]
    fn a_handle_from_another_d3d12_device_is_foreign_not_merely_unresolvable() {
        let instance = open_instance();
        let adapter = pinned_adapter(&instance);
        let a = instance
            .open_device(&device_desc(adapter))
            .expect("device A");
        let b = instance
            .open_device(&device_desc(adapter))
            .expect("device B");

        let on_a = a
            .create_buffer(&buffer(64, MemoryLocation::HostUpload))
            .expect("a buffer on A");
        let on_b = b
            .create_buffer(&buffer(64, MemoryLocation::HostUpload))
            .expect("a buffer on B, occupying the slot A's handle would land in");
        assert_eq!(
            on_a.generation(),
            on_b.generation(),
            "both pools are fresh, so only the tag can tell these apart"
        );

        let error = b
            .write_buffer(on_a, 0, &[0xFF; 4])
            .expect_err("A's buffer is not B's to write");
        assert!(
            matches!(error, HalError::ForeignObject { kind, .. } if kind == "buffer"),
            "expected ForeignObject, got {error:?}"
        );

        // And B's own handle still works, so the check is not simply refusing
        // everything.
        b.write_buffer(on_b, 0, &[0xFF; 4])
            .expect("B's own buffer resolves");

        // A destroy with a foreign handle must not destroy the local object
        // that shares its bits.
        b.destroy_buffer(on_a);
        b.write_buffer(on_b, 0, &[0xEE; 4])
            .expect("B's buffer survived a foreign destroy");
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
                "a host-visible image",
                ImageDesc {
                    memory: MemoryLocation::HostUpload,
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

    /// Every slice that has not arrived still refuses, by name — so none of them
    /// can be half-implemented without this saying so.
    #[test]
    #[ignore = "needs a real D3D12 device; run tests/run-dx12-e2e.sh"]
    fn the_d3d12_slices_that_have_not_arrived_still_refuse_and_name_themselves() {
        let (_instance, device) = open_device();

        let refusals: Vec<(&str, HalError)> = vec![
            (
                "query sets",
                device
                    .create_query_set(&QuerySetDesc {
                        label: None,
                        kind: QueryKind::Timestamp,
                        count: 1,
                    })
                    .expect_err("no query heaps yet"),
            ),
            (
                "semaphores",
                device
                    .create_semaphore(&SemaphoreDesc {
                        label: None,
                        kind: SemaphoreKind::Timeline { initial_value: 0 },
                    })
                    .expect_err("no shared fence yet"),
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
                    .expect_err("no fence a caller can hold yet"),
            ),
        ];
        assert!(!refusals.is_empty(), "nothing to check");
        for (what, error) in &refusals {
            assert!(
                matches!(error, HalError::Unsupported { backend, .. } if *backend == BackendKind::Dx12),
                "{what}: {error:?}"
            );
            let text = error.to_string();
            assert!(text.contains("dx12"), "{what}: {text}");
            assert!(
                text.contains("DX12") && text.contains("slice"),
                "{what}: {text}"
            );
        }

        // **Recording works now, so the encoder's refusals moved to the commands
        // that still need a pipeline state object or a root signature.** This is
        // the inverse half, and it is the half that rots: without it a command
        // that started working would keep passing a test written when nothing
        // did. `create_command_encoder` returns a bare `Box`, so a draw has
        // nowhere to report itself and `finish` carries the refusal.
        let queue = device
            .queue(QueueKind::Graphics)
            .expect("the graphics queue exists");
        type Refused = (&'static str, fn(&mut dyn CommandEncoder));
        let recording: &[Refused] = &[
            ("buffer fills", |encoder| {
                encoder.fill_buffer(unissued(), 0, 4, 0);
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
                matches!(error, HalError::Unsupported { backend, .. } if backend == BackendKind::Dx12),
                "{what}: {error:?}"
            );
            let text = error.to_string();
            assert!(
                text.contains("dx12") && text.contains("DX12") && text.contains("slice"),
                "{what}: {text}"
            );
        }

        // And an empty submission is a legal no-op now rather than a refusal,
        // which is the only honest answer: there is no work to run and nothing
        // to signal for.
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

        let untagged = unissued();
        let error = device
            .submit(untagged, &SubmitInfo::new(&[]))
            .expect_err("no device ever issued that handle");
        assert!(
            matches!(error, HalError::InvalidHandle { kind, .. } if kind == "queue"),
            "{error:?}"
        );
    }

    /// **The entry points that crossed over must never answer `Unsupported`
    /// again.**
    ///
    /// This is the half that rots. The test above asserts that unwritten slices
    /// still refuse; without its inverse, an entry point that was implemented
    /// and then *regressed* to `not_yet` — a merge that reverted a match arm, a
    /// refactor that reinstated a stub — would go on passing every test in this
    /// file. `crcbl-mtl` added this after exactly that happened.
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
            // Each opens the pass first, because the scope is what decides the
            // bind point and a compute command outside one is a descriptor
            // error rather than a handle one — which would pass this test
            // without ever reaching the code it is about.
            ("compute pipelines", |encoder| {
                encoder.begin_compute_pass(&ComputePassDesc { label: None });
                encoder.bind_compute_pipeline(unissued());
            }),
            ("dispatches", |encoder| {
                encoder.begin_compute_pass(&ComputePassDesc { label: None });
                encoder.dispatch(1, 1, 1);
            }),
            ("indirect dispatches", |encoder| {
                encoder.begin_compute_pass(&ComputePassDesc { label: None });
                encoder.dispatch_indirect(unissued(), 0);
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
    /// empty slice here, so anything in it is a handle no device issued — and
    /// the answer has to say *semaphore*, not "swapchain", or a caller reading
    /// the message goes looking at the wrong object. Asserted with a live-ish
    /// swapchain handle position deliberately left unissued too: the semaphore
    /// is checked first.
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
    /// — they take a handle, no query set or semaphore exists to have issued
    /// one, so "you handed me something that never resolved" is both true and
    /// more useful than "the query slice is not here".
    ///
    /// Asserted beside the *creation* calls, which refuse the other way, because
    /// the claim is about the difference: a backend that answered `Unsupported`
    /// everywhere would pass either assertion alone.
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

        // The other side of the line, on the calls that create the very objects
        // those handles would have named.
        let error = device
            .create_query_set(&QuerySetDesc {
                label: None,
                kind: QueryKind::Timestamp,
                count: 1,
            })
            .expect_err("no query heaps yet");
        assert!(
            matches!(error, HalError::Unsupported { backend, .. } if backend == BackendKind::Dx12),
            "{error:?}"
        );
        let error = device
            .create_semaphore(&SemaphoreDesc {
                label: None,
                kind: SemaphoreKind::Timeline { initial_value: 0 },
            })
            .expect_err("no shared fence yet");
        assert!(
            matches!(error, HalError::Unsupported { backend, .. } if backend == BackendKind::Dx12),
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
        /// A copy rather than `fill_buffer`, which this backend still refuses —
        /// so the reset is a transfer the encoder already records rather than a
        /// second slice this test would have to wait for.
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
        fn run(
            &self,
            device: &Dx12Device,
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

                encoder.begin_compute_pass(&ComputePassDesc {
                    label: Some("compute probe"),
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

    /// **A dispatch that really ran, and really wrote the values it was asked
    /// for** — and the evidence behind [`Features::COMPUTE`].
    ///
    /// This is what the flag is reported from. `crcbl_dx12::adapter` reports
    /// compute unconditionally, and a feature whose calls do nothing is the
    /// "unsupported arriving as passed" shape the crate docs name, so the
    /// assertion on the flag and the assertion on the buffer are in the same
    /// test on purpose: neither passes without the other.
    ///
    /// Every stage this goes through is named in its panic — see [`run`] — for
    /// the reason the triangle test gives: a WARP that cannot execute a compute
    /// shader must fail legibly rather than as a timeout.
    ///
    /// # What only CI can settle
    ///
    /// All of it. This crate compiles on Windows alone and the development box
    /// is Linux.
    #[test]
    #[ignore = "needs a real D3D12 device; run tests/run-dx12-e2e.sh"]
    fn a_d3d12_compute_dispatch_writes_the_values_it_was_asked_for() {
        let (_instance, device) = open_device();
        // Not a skip. Every D3D12 device accepts compute work on its DIRECT
        // queue, so an absence here is a capability-reporting bug rather than a
        // machine this suite should tiptoe around.
        assert!(
            device.caps().features.contains(Features::COMPUTE),
            "every D3D12 device has compute; adapter caps report {:?}",
            device.caps().features
        );

        let probe = ComputeProbe::new(&device);
        let values = probe.run(&device, |encoder| {
            encoder.dispatch(PROBE_GROUPS, 1, 1);
        });

        assert_probe(&values, &probe_expected(PROBE_ELEMENTS), "a full dispatch");
        assert!(
            !values.contains(&PROBE_SENTINEL),
            "a full dispatch must leave no element unwritten"
        );

        // And a pass with no dispatch in it writes nothing, which is what makes
        // the assertion above about the dispatch rather than about the copy that
        // reset the buffer.
        let empty = probe.run(&device, |_| {});
        assert!(
            empty.iter().all(|value| *value == PROBE_SENTINEL),
            "a compute pass with no dispatch in it wrote to the destination"
        );

        probe.destroy(&device);
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
        let whole = probe.run(&device, |encoder| {
            encoder.dispatch(PROBE_GROUPS, 1, 1);
        });
        assert_probe(&whole, &probe_expected(PROBE_ELEMENTS), "at offset 0");

        // The same everything, one number apart: the second block, which says
        // half the elements and one more.
        probe.dynamic_offsets = vec![PROBE_PARAMS_STRIDE];
        let half = probe.run(&device, |encoder| {
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

    /// `dispatch_indirect` reads its workgroup count out of GPU memory, at the
    /// offset it was given.
    ///
    /// The argument buffer carries a **decoy** at offset zero that would
    /// dispatch every workgroup. So three different failures are
    /// distinguishable rather than confusable: a backend that ignored the offset
    /// dispatches eight groups and overwrites the tail; one that ignored the
    /// argument buffer entirely writes nothing; a correct one writes exactly the
    /// front of the buffer and leaves the sentinel behind it.
    ///
    /// It is also the only test of the command signature: `ExecuteIndirect` is
    /// how D3D12 spells this call, and a signature with the wrong stride or the
    /// wrong argument type reads three words from somewhere else in the buffer.
    #[test]
    #[ignore = "needs a real D3D12 device; run tests/run-dx12-e2e.sh"]
    fn a_d3d12_indirect_dispatch_reads_its_workgroup_count_from_the_buffer() {
        /// Workgroups the real arguments ask for. Fewer than [`PROBE_GROUPS`],
        /// so the difference is visible in the readback.
        const DISPATCHED_GROUPS: u32 = 2;
        /// Where the real arguments live. Non-zero, and the decoy sits at zero.
        const ARGS_OFFSET: u64 = 16;

        let (_instance, device) = open_device();
        let probe = ComputeProbe::new(&device);

        // `D3D12_DISPATCH_ARGUMENTS`: three `u32`s, `x`, `y`, `z`. Fixed by
        // D3D12 rather than by this engine — `crcbl-hal` does not spell the
        // argument layout, because it is the backend's native one, and this is a
        // `crcbl-dx12` test.
        let mut args_bytes = vec![0u8; ARGS_OFFSET as usize + 12];
        for (slot, value) in [PROBE_GROUPS, 1, 1].iter().enumerate() {
            args_bytes[slot * 4..slot * 4 + 4].copy_from_slice(&value.to_le_bytes());
        }
        for (slot, value) in [DISPATCHED_GROUPS, 1, 1].iter().enumerate() {
            let at = ARGS_OFFSET as usize + slot * 4;
            args_bytes[at..at + 4].copy_from_slice(&value.to_le_bytes());
        }

        let upload = device
            .create_buffer(&BufferDesc {
                label: Some("dispatch args upload"),
                size: args_bytes.len() as u64,
                usage: BufferUsage::TRANSFER_SRC,
                memory: MemoryLocation::HostUpload,
            })
            .expect("a staging buffer");
        device.write_buffer(upload, 0, &args_bytes).expect("write");
        let args = device
            .create_buffer(&BufferDesc {
                label: Some("dispatch args"),
                size: args_bytes.len() as u64,
                usage: BufferUsage::INDIRECT | BufferUsage::TRANSFER_DST,
                memory: MemoryLocation::DeviceLocal,
            })
            .expect("an indirect buffer");

        run(&device, |encoder| {
            encoder.pipeline_barrier(&Barriers {
                buffers: &[buffer_barrier(
                    args,
                    ResourceState::Undefined,
                    ResourceState::TransferDst,
                )],
                ..Barriers::default()
            });
            encoder.copy_buffer_to_buffer(&BufferCopy {
                src: upload,
                src_offset: 0,
                dst: args,
                dst_offset: 0,
                size: args_bytes.len() as u64,
            });
            encoder.pipeline_barrier(&Barriers {
                buffers: &[buffer_barrier(
                    args,
                    ResourceState::TransferDst,
                    ResourceState::IndirectArgument,
                )],
                ..Barriers::default()
            });
        });
        device.destroy_buffer(upload);

        let values = probe.run(&device, |encoder| {
            encoder.dispatch_indirect(args, ARGS_OFFSET);
        });

        let dispatched = DISPATCHED_GROUPS * crcbl_shaders::compute_probe::WORKGROUP_SIZE;
        assert!(
            dispatched > 0 && dispatched < PROBE_ELEMENTS,
            "the indirect dispatch must cover part of the buffer, not none and not all"
        );
        assert_probe(&values, &probe_expected(dispatched), "an indirect dispatch");
        // Said again in its own words, because the two halves fail for different
        // reasons: the front proves work happened, the tail proves the *count*
        // came from the buffer at the offset that was named.
        assert!(
            values[..dispatched as usize]
                .iter()
                .all(|value| *value != PROBE_SENTINEL),
            "the dispatched workgroups wrote nothing"
        );
        assert!(
            values[dispatched as usize..]
                .iter()
                .all(|value| *value == PROBE_SENTINEL),
            "the workgroups past the indirect count ran anyway — the argument buffer or its \
             offset was not honoured"
        );

        device.destroy_buffer(args);
        probe.destroy(&device);
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
        encoder.begin_compute_pass(&ComputePassDesc { label: None });
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
                encoder.begin_compute_pass(&ComputePassDesc { label: None });
                encoder.dispatch(1, 1, 1);
            }),
            (
                "no compute pipeline bound",
                "dispatch_indirect",
                |encoder| {
                    encoder.begin_compute_pass(&ComputePassDesc { label: None });
                    encoder.dispatch_indirect(unissued(), 0);
                },
            ),
            ("do not nest", "a nested compute pass", |encoder| {
                encoder.begin_compute_pass(&ComputePassDesc { label: None });
                encoder.begin_compute_pass(&ComputePassDesc { label: None });
            }),
            ("compute pass still open", "an unclosed pass", |encoder| {
                encoder.begin_compute_pass(&ComputePassDesc { label: None });
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
