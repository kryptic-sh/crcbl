//! Bind group layouts, bind groups, and the **shader-visible** descriptor heaps
//! a root signature binds against.
//!
//! `crcbl_dx12::descriptor` owns the CPU-visible heaps every view and sampler is
//! written into, and says in its own docs that a shader-visible heap "belongs to
//! the bind-group slice that has a root signature to bind it against". This is
//! that slice.
//!
//! # A bind group is a contiguous run of descriptors, and that is what makes it
//! bindable
//!
//! D3D12 has no descriptor-set object. What
//! [`CommandEncoder::bind_group`](crcbl_hal::CommandEncoder::bind_group)
//! ultimately reaches is `SetGraphicsRootDescriptorTable`, which takes **one GPU
//! handle** and reads every range of the table at fixed offsets from it. So a
//! bind group is a *block* in a shader-visible heap, and the layout's ranges
//! carry the offsets within it.
//!
//! Samplers cannot share that block: `CreateSampler` writes only into a
//! `SAMPLER` heap and a descriptor table may not mix heap types. A layout with
//! both therefore becomes **two** root parameters and a bind group holds **two**
//! blocks. [`conv::descriptor_range_type`](crate::conv::descriptor_range_type)
//! makes the split a shape rather than a rule, by having no range type to return
//! for a sampler at all.
//!
//! # One heap per type, sized once, and it refuses rather than moving
//!
//! Only one shader-visible heap of each type may be bound at a time, and a bind
//! group's GPU handle is an address inside the heap it was allocated from — so
//! growing means reallocating, re-writing every live descriptor and invalidating
//! every handle already recorded into an unsubmitted command list. This slice
//! does not do that. It allocates one heap of each type at [`VIEW_DESCRIPTORS`]
//! / [`SAMPLER_DESCRIPTORS`] and fails
//! [`create_bind_group`](crcbl_hal::Device::create_bind_group) with
//! [`HalError::OutOfDeviceMemory`] when a device runs out, which is a named
//! ceiling rather than a silent overwrite.
//!
//! Freed blocks return to a free list and are reused **only at exactly their own
//! size**. Nothing coalesces, so a device that allocated many differently-sized
//! groups and freed them would fragment; a frame loop rebuilding one set of
//! groups per size — which is every caller this backend has — never does. The
//! alternative is a real suballocator, which is a slice of its own.
//!
//! # Descriptors are written straight into the shader-visible heap
//!
//! `CreateConstantBufferView` and friends take a CPU handle, and a
//! shader-visible heap has one. The usual two-step — write into a staging heap
//! and `CopyDescriptorsSimple` into the shader-visible one — exists to batch
//! updates and to let a caller keep a descriptor it may re-copy later. Neither
//! applies while [`update_bind_group`](crcbl_hal::Device::update_bind_group)
//! writes one entry at a time, so the copy would be a second write with no
//! reader. An image view and a sampler are the exception, and only because
//! their descriptors *already exist* in a CPU-visible heap: `create_image_view`
//! and `create_sampler` wrote them, so publishing one is the copy.
//!
//! **What that costs is the [`BindingFlags::UPDATE_AFTER_BIND`] hazard**, and it
//! is the caller's, exactly as it is on Vulkan: writing a descriptor a submitted
//! command list is reading is a race, and the flag is the caller saying it has
//! ordered that itself. Root signature 1.0 ranges are volatile by definition —
//! the version predates `D3D12_DESCRIPTOR_RANGE_FLAGS` — so D3D12 asks nothing
//! more of this backend to permit it.
//!
//! # Bindless is supported, so `DESCRIPTOR_INDEXING` stays reported
//!
//! `crcbl_dx12::adapter` reports
//! [`Features::DESCRIPTOR_INDEXING`](crcbl_hal::Features::DESCRIPTOR_INDEXING)
//! from a real `ResourceBindingTier`/`HighestShaderModel` pair and says the
//! bind-group slice owns the flag — "if `create_bind_group_layout` cannot honour
//! [`BindingFlags`] on a descriptor heap, it must come off, exactly as it came
//! off Metal". It does honour them, and each for a reason that is D3D12's rather
//! than this crate's:
//!
//! * [`BindingFlags::VARIABLE_COUNT`] — the range's `NumDescriptors` becomes
//!   [`u32::MAX`], which is how D3D12 spells an unbounded descriptor range, and
//!   the *group* allocates
//!   [`BindGroupDesc::variable_count`](crcbl_hal::BindGroupDesc::variable_count)
//!   descriptors.
//! * [`BindingFlags::PARTIALLY_BOUND`] — binding tier 3 requires only the
//!   descriptors a shader actually accesses to be valid. Every view block is
//!   still initialised to a null view before use, so an unwritten slot is a
//!   defined read of zero rather than whatever the previous owner of that heap
//!   slot left.
//! * [`BindingFlags::UPDATE_AFTER_BIND`] — see above.
//!
//! # The register a binding lands on comes from the artifact, not from its
//! number
//!
//! `[[vk::binding(binding, set)]]` reaches SPIR-V and nothing else. Slang's HLSL
//! output drops it, and `dxc` numbers each register class from zero in
//! declaration order across the whole source, in space 0 — so a set holding a
//! `ConstantBuffer`, a `StructuredBuffer` and an `RWStructuredBuffer` at
//! bindings 0, 1 and 2 is `b0`/`t0`/`u0` in the container, not `b0`/`t1`/`u2`.
//! A root signature naming registers the shader does not read is rejected by
//! pipeline creation, so this is not a subtlety a caller can absorb.
//!
//! [`ranges`] therefore assigns registers with [`root::assign_registers`],
//! threaded across a pipeline layout's sets in order, and `crate::dxil` checks
//! the rule against every committed container on any host. It is correct because
//! `crcbl-shaders`' `declaration_order` lint already requires each source to
//! declare its resources in ascending `(set, binding)` order — the same
//! guarantee `crcbl-mtl` leans on for its flat argument tables.
//!
//! # A dynamic offset leaves the table and becomes a root descriptor
//!
//! [`BindingKind::UniformBuffer`](crcbl_hal::BindingKind::UniformBuffer)'s
//! `dynamic` and its storage-buffer twin are the one binding shape a descriptor
//! table cannot express: the table is reached by a descriptor handle, and every
//! view inside it was written when the group was created. D3D12's answer is a
//! **root** CBV/SRV/UAV — a root parameter carrying a raw GPU virtual address,
//! to which the offset is simply added — so such a binding occupies no slot in
//! the set's block and is planned into [`BindGroupLayoutRecord::roots`] instead.
//! [`crate::root`] owns that decision, the budget it spends and the parameter
//! index it lands on, because two modules have to agree on the last of those.
//!
//! What this module still owns is everything about the *binding*: that a dynamic
//! binding is a single buffer rather than an array or an image, that it still
//! takes its register in declaration order beside the table's, and that the
//! address a group holds for it is the buffer's plus the entry's offset.

use crcbl_hal::{
    BackendKind, BindGroupDesc, BindGroupEntry, BindGroupLayoutDesc, BindGroupLayoutEntry,
    BindGroupLayoutHandle, BindingFlags, BindingKind, DeviceCaps, HalError, MemoryLocation,
    ShaderStages,
};
use windows::Win32::Graphics::Direct3D12::{
    D3D12_BUFFER_SRV, D3D12_BUFFER_SRV_FLAG_RAW, D3D12_BUFFER_UAV, D3D12_BUFFER_UAV_FLAG_RAW,
    D3D12_CONSTANT_BUFFER_VIEW_DESC, D3D12_CPU_DESCRIPTOR_HANDLE,
    D3D12_DEFAULT_SHADER_4_COMPONENT_MAPPING, D3D12_DESCRIPTOR_HEAP_DESC,
    D3D12_DESCRIPTOR_HEAP_FLAG_SHADER_VISIBLE, D3D12_DESCRIPTOR_HEAP_TYPE,
    D3D12_DESCRIPTOR_HEAP_TYPE_CBV_SRV_UAV, D3D12_DESCRIPTOR_HEAP_TYPE_SAMPLER,
    D3D12_DESCRIPTOR_RANGE, D3D12_DESCRIPTOR_RANGE_TYPE, D3D12_DESCRIPTOR_RANGE_TYPE_CBV,
    D3D12_DESCRIPTOR_RANGE_TYPE_SAMPLER, D3D12_DESCRIPTOR_RANGE_TYPE_SRV,
    D3D12_DESCRIPTOR_RANGE_TYPE_UAV, D3D12_GPU_DESCRIPTOR_HANDLE,
    D3D12_MAX_SHADER_VISIBLE_SAMPLER_HEAP_SIZE, D3D12_ROOT_DESCRIPTOR, D3D12_ROOT_PARAMETER_TYPE,
    D3D12_ROOT_PARAMETER_TYPE_CBV, D3D12_ROOT_PARAMETER_TYPE_SRV, D3D12_ROOT_PARAMETER_TYPE_UAV,
    D3D12_SHADER_RESOURCE_VIEW_DESC, D3D12_SHADER_RESOURCE_VIEW_DESC_0, D3D12_SRV_DIMENSION_BUFFER,
    D3D12_UAV_DIMENSION_BUFFER, D3D12_UNORDERED_ACCESS_VIEW_DESC,
    D3D12_UNORDERED_ACCESS_VIEW_DESC_0, ID3D12DescriptorHeap, ID3D12Device, ID3D12Resource,
};
use windows::Win32::Graphics::Dxgi::Common::DXGI_FORMAT_R32_TYPELESS;

use crate::buffer;
use crate::conv;
use crate::dxil;
use crate::handle::Owned;
use crate::root;

/// Descriptors in the one shader-visible CBV/SRV/UAV heap.
///
/// Not `D3D12_MAX_SHADER_VISIBLE_DESCRIPTOR_HEAP_SIZE_TIER_2`, which is a
/// million: the heap is allocated up front and in full, so the ceiling is also a
/// per-device cost every caller pays. This is large enough for a bindless page
/// of textures beside a frame's ordinary sets, and small enough to be
/// unremarkable. Raising it is a one-line change with a measurable cost, which
/// is the property that matters.
pub(crate) const VIEW_DESCRIPTORS: u32 = 4096;

/// Descriptors in the one shader-visible sampler heap.
///
/// D3D12's own hard ceiling, because a sampler descriptor is small and there is
/// no smaller number that is not arbitrary.
const SAMPLER_DESCRIPTORS: u32 = D3D12_MAX_SHADER_VISIBLE_SAMPLER_HEAP_SIZE;

/// One binding of a layout, resolved to the descriptor range it becomes.
#[derive(Clone, Copy, Debug)]
pub(crate) struct RangePlan {
    /// The seam's binding number, which is also the HLSL register.
    binding: u32,
    /// What D3D12 calls the range.
    range_type: D3D12_DESCRIPTOR_RANGE_TYPE,
    /// Descriptors this binding occupies in a group's block. Zero for an
    /// unbounded range, which the group sizes instead.
    count: u32,
    /// Where it starts in the table.
    offset: u32,
    /// `NumDescriptors` for the *root signature*, which is [`u32::MAX`] for an
    /// unbounded range and [`count`](Self::count) otherwise.
    declared: u32,
}

/// One binding that takes a dynamic offset, and so becomes a root descriptor.
///
/// It occupies no descriptor in a group's block: `SetGraphicsRootConstantBufferView`
/// and its siblings take a GPU virtual address, and the address is what a bind
/// adds the offset to. See the module docs and [`crate::root`].
#[derive(Clone, Copy, Debug)]
pub(crate) struct RootPlan {
    /// The seam's binding number.
    pub(crate) binding: u32,
    /// The range type this binding would have taken in a table, kept because it
    /// carries both the register class and the root parameter type.
    range_type: D3D12_DESCRIPTOR_RANGE_TYPE,
    /// This binding's own visibility. A root descriptor is one binding, so it
    /// need not widen to the union of the set's the way a table does.
    visibility: ShaderStages,
}

impl RootPlan {
    /// Whether this is a root CBV, and so takes the uniform-buffer alignment.
    pub(crate) fn uniform(&self) -> bool {
        self.range_type == D3D12_DESCRIPTOR_RANGE_TYPE_CBV
    }

    /// The root parameter type D3D12 spells this binding with.
    pub(crate) fn parameter_type(&self) -> D3D12_ROOT_PARAMETER_TYPE {
        match self.range_type {
            D3D12_DESCRIPTOR_RANGE_TYPE_CBV => D3D12_ROOT_PARAMETER_TYPE_CBV,
            D3D12_DESCRIPTOR_RANGE_TYPE_SRV => D3D12_ROOT_PARAMETER_TYPE_SRV,
            // `plan_layout` only routes a buffer binding here and a sampler is
            // not one, so the remaining case is the writable storage buffer.
            _ => D3D12_ROOT_PARAMETER_TYPE_UAV,
        }
    }
}

/// A block of descriptors inside one shader-visible heap.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct Block {
    start: u32,
    count: u32,
}

/// A dynamic binding's resolved buffer, as a bind reads it.
///
/// The address is `GetGPUVirtualAddress` plus the entry's own offset, and
/// `None` until an entry writes one — a root descriptor is a bare address with
/// no null view behind it, so an unwritten binding has to refuse rather than
/// bind zero.
#[derive(Clone, Copy, Debug, Default)]
pub(crate) struct BoundBuffer {
    pub(crate) address: Option<u64>,
    /// Byte offset the entry bound the range at.
    pub(crate) offset: u64,
    /// Bytes the entry bound.
    pub(crate) size: u64,
    /// The buffer's own size, which is what a dynamic offset is bounded by.
    pub(crate) capacity: u64,
}

/// A bind group layout: the ranges it declares, split by heap type.
#[derive(Debug)]
pub(crate) struct BindGroupLayoutRecord {
    pub(crate) owner: u64,
    /// CBV/SRV/UAV ranges, in declaration order.
    pub(crate) views: Vec<RangePlan>,
    /// Sampler ranges, in declaration order.
    pub(crate) samplers: Vec<RangePlan>,
    /// The bindings that take a dynamic offset, **ascending by binding number**
    /// — which is the order
    /// [`bind_group`](crcbl_hal::CommandEncoder::bind_group) documents its
    /// `dynamic_offsets` in, and therefore the order a group's addresses and
    /// the layout's root parameter indices are both kept in.
    pub(crate) roots: Vec<RootPlan>,
    /// Descriptors a group of this layout needs in the view heap, before the
    /// unbounded binding's own count is added.
    view_descriptors: u32,
    /// The same for the sampler heap.
    sampler_descriptors: u32,
    /// The unbounded binding, if any: its binding number, and whether it lives
    /// in the view heap. At most one, and [`check_entry`] makes it the last.
    variable: Option<(u32, bool)>,
    /// The union of every entry's visibility, which is what a root parameter
    /// takes.
    pub(crate) visibility: ShaderStages,
}

/// A bind group: its blocks, and the resources its descriptors point into.
#[derive(Debug)]
pub(crate) struct BindGroupRecord {
    pub(crate) owner: u64,
    pub(crate) layout: BindGroupLayoutHandle,
    pub(crate) views: Option<Block>,
    pub(crate) samplers: Option<Block>,
    /// One entry per dynamic binding of the layout, in the same ascending order
    /// [`BindGroupLayoutRecord::roots`] is kept in. Sized when the group is
    /// created, so the two lists index each other.
    pub(crate) roots: Vec<BoundBuffer>,
    /// A reference to every resource a descriptor in this group names.
    ///
    /// **A descriptor is a raw address into a resource and D3D12 refcounts
    /// nothing on its behalf**, so a buffer destroyed while a group still points
    /// at it would leave the group reading freed memory. The encoder retains
    /// these again at `bind_group` time, which covers the window between
    /// submission and completion; this covers the window between
    /// `destroy_buffer` and then.
    pub(crate) retained: Vec<ID3D12Resource>,
}

impl Owned for BindGroupLayoutRecord {
    fn owner(&self) -> u64 {
        self.owner
    }
}

impl Owned for BindGroupRecord {
    fn owner(&self) -> u64 {
        self.owner
    }
}

/// One shader-visible heap and the blocks handed out of it.
#[derive(Debug)]
struct VisibleHeap {
    kind: D3D12_DESCRIPTOR_HEAP_TYPE,
    capacity: u32,
    /// Created on the first allocation, so a device that never makes a bind
    /// group never allocates a heap.
    heap: Option<ID3D12DescriptorHeap>,
    stride: usize,
    cpu_base: usize,
    gpu_base: u64,
    issued: u32,
    /// Freed blocks, reused only at exactly their own size. See the module docs.
    recycled: Vec<Block>,
}

impl VisibleHeap {
    const fn new(kind: D3D12_DESCRIPTOR_HEAP_TYPE, capacity: u32) -> Self {
        Self {
            kind,
            capacity,
            heap: None,
            stride: 0,
            cpu_base: 0,
            gpu_base: 0,
            issued: 0,
            recycled: Vec::new(),
        }
    }

    /// Takes a block of `count` descriptors, creating the heap if this is the
    /// first.
    fn allocate(&mut self, device: &ID3D12Device, count: u32) -> Result<Block, HalError> {
        self.ensure(device)?;
        self.take(count)
    }

    /// The allocation itself, with no device in it.
    ///
    /// Split out so the arithmetic is testable: a block that overlapped a live
    /// one is a descriptor two bind groups share, which D3D12 never reports and
    /// which this backend has no other way to see.
    fn take(&mut self, count: u32) -> Result<Block, HalError> {
        if let Some(index) = self.recycled.iter().position(|block| block.count == count) {
            return Ok(self.recycled.swap_remove(index));
        }
        let start = self.issued;
        if count > self.capacity - start {
            return Err(HalError::OutOfDeviceMemory);
        }
        self.issued = start + count;
        Ok(Block { start, count })
    }

    fn free(&mut self, block: Block) {
        self.recycled.push(block);
    }

    /// Creates the heap, once.
    fn ensure(&mut self, device: &ID3D12Device) -> Result<(), HalError> {
        if self.heap.is_some() {
            return Ok(());
        }
        let desc = D3D12_DESCRIPTOR_HEAP_DESC {
            Type: self.kind,
            NumDescriptors: self.capacity,
            Flags: D3D12_DESCRIPTOR_HEAP_FLAG_SHADER_VISIBLE,
            // Node zero, as everywhere else in this crate: the seam has no
            // multi-adapter vocabulary.
            NodeMask: 0,
        };
        // SAFETY: `desc` is a live, fully initialised descriptor borrowed for
        // the call, and `ID3D12DescriptorHeap` is the IID asked for, so the call
        // either writes that interface or fails.
        let heap: ID3D12DescriptorHeap =
            unsafe { device.CreateDescriptorHeap(&desc) }.map_err(|error| {
                HalError::Backend(format!(
                    "CreateDescriptorHeap({:?}, shader-visible, {} descriptors) failed: {error}",
                    self.kind, self.capacity
                ))
            })?;
        // SAFETY: `heap` is the interface the call just returned and `device` is
        // live. All three report values the driver guarantees are stable for the
        // heap's lifetime, which is what makes caching them sound.
        unsafe {
            self.stride = device.GetDescriptorHandleIncrementSize(self.kind) as usize;
            self.cpu_base = heap.GetCPUDescriptorHandleForHeapStart().ptr;
            self.gpu_base = heap.GetGPUDescriptorHandleForHeapStart().ptr;
        }
        self.heap = Some(heap);
        Ok(())
    }

    /// The CPU address of one descriptor in a block.
    fn cpu(&self, block: Block, index: u32) -> D3D12_CPU_DESCRIPTOR_HANDLE {
        D3D12_CPU_DESCRIPTOR_HANDLE {
            ptr: self.cpu_base + (block.start + index) as usize * self.stride,
        }
    }

    /// The GPU address a descriptor table is bound at.
    fn gpu(&self, block: Block) -> D3D12_GPU_DESCRIPTOR_HANDLE {
        D3D12_GPU_DESCRIPTOR_HANDLE {
            ptr: self.gpu_base + u64::from(block.start) * self.stride as u64,
        }
    }
}

/// The shader-visible heaps a device binds.
#[derive(Debug)]
pub(crate) struct VisibleHeaps {
    views: VisibleHeap,
    samplers: VisibleHeap,
}

impl VisibleHeaps {
    pub(crate) const fn new() -> Self {
        Self {
            views: VisibleHeap::new(D3D12_DESCRIPTOR_HEAP_TYPE_CBV_SRV_UAV, VIEW_DESCRIPTORS),
            samplers: VisibleHeap::new(D3D12_DESCRIPTOR_HEAP_TYPE_SAMPLER, SAMPLER_DESCRIPTORS),
        }
    }

    /// Every heap that exists, in the order `SetDescriptorHeaps` takes them.
    ///
    /// A heap that was never created is absent rather than null: the call
    /// rejects a null entry, and a device that has bound no sampler yet has no
    /// sampler heap.
    pub(crate) fn bound(&self) -> Vec<Option<ID3D12DescriptorHeap>> {
        [self.views.heap.clone(), self.samplers.heap.clone()]
            .into_iter()
            .flatten()
            .map(Some)
            .collect()
    }

    /// Where a group's view table is bound.
    pub(crate) fn gpu_views(&self, block: Block) -> D3D12_GPU_DESCRIPTOR_HANDLE {
        self.views.gpu(block)
    }

    /// Where a group's sampler table is bound.
    pub(crate) fn gpu_samplers(&self, block: Block) -> D3D12_GPU_DESCRIPTOR_HANDLE {
        self.samplers.gpu(block)
    }
}

/// Turns a [`BindGroupLayoutDesc`] into the ranges a root signature declares.
///
/// Every rule the seam states is checked first, by
/// [`BindGroupLayoutDesc::check_entries`] — including the visibility check,
/// which is what refuses a mesh- or task-visible entry on a device reporting no
/// `Features::MESH_SHADER`, as every adapter this backend enumerates does. It is
/// called here rather than by `create_bind_group_layout` so that the refusal is
/// reachable from this module's own tests, which have no D3D12 device to open.
///
/// (This used to add that [`conv::shader_visibility`] would otherwise widen a
/// mesh stage to `D3D12_SHADER_VISIBILITY_ALL` and say nothing. The mesh slice
/// took that reason away: `shader_visibility` now answers
/// `D3D12_SHADER_VISIBILITY_MESH` and `_AMPLIFICATION`, so the check above is
/// the seam's rule being enforced and no longer this backend covering for a
/// mapping it did not have.)
///
/// # Errors
///
/// Whatever [`BindGroupLayoutDesc::check_entries`] returns, plus
/// [`HalError::InvalidDescriptor`] for a dynamic-offset binding that is an
/// array or carries a [`BindingFlags`] — neither of which a root descriptor can
/// encode. See [`check_entry`].
pub(crate) fn plan_layout(
    desc: &BindGroupLayoutDesc<'_>,
    caps: &DeviceCaps,
    owner: u64,
) -> Result<BindGroupLayoutRecord, HalError> {
    desc.check_entries(caps, BackendKind::Dx12)?;

    let mut views: Vec<RangePlan> = Vec::new();
    let mut samplers: Vec<RangePlan> = Vec::new();
    let mut roots: Vec<RootPlan> = Vec::new();
    let mut visibility = ShaderStages::empty();
    let mut variable = None;

    for entry in desc.entries {
        check_entry(entry)?;
        // A `VARIABLE_COUNT` binding becomes D3D12's own unbounded range, so
        // the seam's `u32::MAX` needs no number there. Anywhere else it does:
        // the table offsets below are sums of counts, and a raw sentinel makes
        // the next offset overflow and the heap block unallocatable.
        let count = entry.resolved_count(&caps.limits);
        let range_type = conv::descriptor_range_type(entry.kind);
        if is_dynamic(entry.kind) {
            roots.push(RootPlan {
                binding: entry.binding,
                range_type: range_type
                    .unwrap_or_else(|| unreachable!("a dynamic binding is a buffer")),
                visibility: entry.visibility,
            });
            continue;
        }
        // The union is what a *descriptor table* takes, so a root descriptor's
        // stage is left out of it: the root parameter carries its own.
        visibility |= entry.visibility;
        let unbounded = entry.flags.contains(BindingFlags::VARIABLE_COUNT);
        let in_views = range_type.is_some();
        let table = if in_views { &mut views } else { &mut samplers };
        table.push(RangePlan {
            binding: entry.binding,
            range_type: range_type.unwrap_or(D3D12_DESCRIPTOR_RANGE_TYPE_SAMPLER),
            count: if unbounded { 0 } else { count },
            offset: next_offset(table),
            declared: if unbounded { u32::MAX } else { count },
        });
        if unbounded {
            variable = Some((entry.binding, in_views));
        }
    }
    // Ascending, because that is the order the seam delivers dynamic offsets in
    // and `desc.entries` is not required to be sorted.
    roots.sort_by_key(|root| root.binding);

    Ok(BindGroupLayoutRecord {
        owner,
        view_descriptors: next_offset(&views),
        sampler_descriptors: next_offset(&samplers),
        views,
        samplers,
        roots,
        variable,
        visibility,
    })
}

/// Whether a binding takes one of `bind_group`'s dynamic offsets.
const fn is_dynamic(kind: BindingKind) -> bool {
    matches!(
        kind,
        BindingKind::UniformBuffer { dynamic: true }
            | BindingKind::StorageBuffer { dynamic: true, .. }
    )
}

/// Where the next range starts in a table.
fn next_offset(ranges: &[RangePlan]) -> u32 {
    ranges.last().map_or(0, |range| range.offset + range.count)
}

/// What only **D3D12** cannot express about one layout entry, named.
///
/// The portable rules — a zero count, a duplicate binding number, the two
/// halves of the `VARIABLE_COUNT` rule — are
/// [`BindGroupLayoutDesc::check_entries`]'s, and were stated here a second time
/// until they drifted. What is left is the root descriptor, which is a concept
/// this API has and the others do not.
fn check_entry(entry: &BindGroupLayoutEntry) -> Result<(), HalError> {
    if is_dynamic(entry.kind) {
        // A root descriptor is one address, so there is nothing for a second
        // element to be reached by — and the seam gives one offset per dynamic
        // *binding*, not per element, so an array of them could not be offset
        // either. `crcbl-mtl` refuses the same shape for the same reason.
        if entry.count != 1 {
            return Err(HalError::InvalidDescriptor(format!(
                "binding {} takes a dynamic offset and has count {}; a root descriptor is one \
                 GPU address, and bind_group carries one offset per dynamic binding rather than \
                 per element",
                entry.binding, entry.count
            )));
        }
        if !entry.flags.is_empty() {
            return Err(HalError::InvalidDescriptor(format!(
                "binding {} takes a dynamic offset and declares {:?}; every one of those flags \
                 describes a descriptor heap, and a root descriptor is not in one",
                entry.binding, entry.flags
            )));
        }
    }
    Ok(())
}

/// Every root parameter one set contributes, and the registers its bindings
/// take.
pub(crate) struct SetTables {
    /// The CBV/SRV/UAV descriptor table's ranges, empty when the set declares
    /// none.
    pub(crate) views: Vec<D3D12_DESCRIPTOR_RANGE>,
    /// The sampler descriptor table's ranges, empty when the set declares none.
    pub(crate) samplers: Vec<D3D12_DESCRIPTOR_RANGE>,
    /// One root descriptor per dynamic binding, in
    /// [`BindGroupLayoutRecord::roots`] order.
    pub(crate) roots: Vec<RootDescriptor>,
    /// The union of the *table* entries' visibilities, which is what a
    /// descriptor table takes. Each root descriptor carries its own.
    pub(crate) visibility: ShaderStages,
}

/// One root descriptor, ready to become a `D3D12_ROOT_PARAMETER`.
pub(crate) struct RootDescriptor {
    pub(crate) parameter_type: D3D12_ROOT_PARAMETER_TYPE,
    pub(crate) descriptor: D3D12_ROOT_DESCRIPTOR,
    pub(crate) visibility: ShaderStages,
}

/// The root parameters one set contributes: its two tables' ranges, and a root
/// descriptor per dynamic binding.
///
/// **The register is not the binding number, and the space is not the set.**
/// `[[vk::binding(binding, set)]]` is a Vulkan attribute; Slang's HLSL output
/// ignores it and lets `dxc` number each register class from zero in
/// declaration order, in space 0. So the registers are assigned by
/// [`root::assign_registers`] — threaded across a pipeline layout's sets by the
/// caller, because the count does not restart at a set boundary.
/// `crate::dxil` measures the rule against every committed container.
///
/// Everything the set declares is numbered in **one** call, tables and root
/// descriptors together: they share the `b`/`t`/`u` register files, so numbering
/// them separately would put the order of two calls in charge of what the
/// artifact is compared against.
pub(crate) fn ranges(layout: &BindGroupLayoutRecord, registers: &mut dxil::Registers) -> SetTables {
    // One list in a fixed order — views, then samplers, then root descriptors —
    // so the registers come back indexable by the same order.
    let bindings: Vec<root::Binding> = layout
        .views
        .iter()
        .chain(&layout.samplers)
        .map(|plan| root::Binding {
            binding: plan.binding,
            class: register_class(plan.range_type),
            declared: plan.declared,
        })
        .chain(layout.roots.iter().map(|plan| root::Binding {
            binding: plan.binding,
            class: register_class(plan.range_type),
            // A root descriptor is exactly one resource; there is no unbounded
            // form of it.
            declared: 1,
        }))
        .collect();
    let assigned = root::assign_registers(&bindings, registers);

    let mut tables: Vec<D3D12_DESCRIPTOR_RANGE> = layout
        .views
        .iter()
        .chain(&layout.samplers)
        .zip(&assigned)
        .map(|(plan, register)| D3D12_DESCRIPTOR_RANGE {
            RangeType: plan.range_type,
            NumDescriptors: plan.declared,
            BaseShaderRegister: *register,
            RegisterSpace: 0,
            OffsetInDescriptorsFromTableStart: plan.offset,
        })
        .collect();
    let samplers = tables.split_off(layout.views.len());
    let views = tables;
    let roots = layout
        .roots
        .iter()
        .zip(assigned.iter().skip(views.len() + samplers.len()))
        .map(|(plan, register)| RootDescriptor {
            parameter_type: plan.parameter_type(),
            descriptor: D3D12_ROOT_DESCRIPTOR {
                ShaderRegister: *register,
                RegisterSpace: 0,
            },
            visibility: plan.visibility,
        })
        .collect();

    SetTables {
        views,
        samplers,
        roots,
        visibility: layout.visibility,
    }
}

/// Which HLSL register file a descriptor range takes its register from.
fn register_class(range: D3D12_DESCRIPTOR_RANGE_TYPE) -> dxil::RegisterClass {
    match range {
        D3D12_DESCRIPTOR_RANGE_TYPE_CBV => dxil::RegisterClass::Cbv,
        D3D12_DESCRIPTOR_RANGE_TYPE_SRV => dxil::RegisterClass::Srv,
        D3D12_DESCRIPTOR_RANGE_TYPE_UAV => dxil::RegisterClass::Uav,
        // The enum has four values and `descriptor_range_type` produces the
        // three above, so this is the sampler and cannot be anything else.
        _ => dxil::RegisterClass::Sampler,
    }
}

/// Allocates a bind group's blocks, and fills every view descriptor with a null
/// view.
///
/// The pre-fill is what makes [`BindingFlags::PARTIALLY_BOUND`] mean something
/// here: a heap slot is uninitialised memory until something writes it, and the
/// flag is a caller saying it will not write every slot — not a promise that
/// reading one is safe.
///
/// # Errors
///
/// [`HalError::OutOfDeviceMemory`] when a shader-visible heap is full. See the
/// module docs for why the heap does not grow.
pub(crate) fn allocate_group(
    device: &ID3D12Device,
    heaps: &mut VisibleHeaps,
    layout: &BindGroupLayoutRecord,
    desc: &BindGroupDesc<'_>,
) -> Result<(Option<Block>, Option<Block>), HalError> {
    let extra = variable_count(layout, desc);
    let in_views = layout.variable.is_none_or(|(_, in_views)| in_views);
    let view_count = layout.view_descriptors + if in_views { extra } else { 0 };
    let sampler_count = layout.sampler_descriptors + if in_views { 0 } else { extra };

    let views = match view_count {
        0 => None,
        count => Some(heaps.views.allocate(device, count)?),
    };
    let samplers = match sampler_count {
        0 => None,
        count => match heaps.samplers.allocate(device, count) {
            Ok(block) => Some(block),
            Err(error) => {
                // The view block was already taken, and returning here without
                // it would leak it for the device's lifetime.
                if let Some(block) = views {
                    heaps.views.free(block);
                }
                return Err(error);
            }
        },
    };

    if let Some(block) = views {
        for index in 0..block.count {
            null_view(device, heaps.views.cpu(block, index));
        }
    }
    Ok((views, samplers))
}

/// Returns a group's blocks to their heaps.
pub(crate) fn free_group(heaps: &mut VisibleHeaps, record: &BindGroupRecord) {
    if let Some(block) = record.views {
        heaps.views.free(block);
    }
    if let Some(block) = record.samplers {
        heaps.samplers.free(block);
    }
}

impl BindGroupLayoutRecord {
    /// The plan for a binding number, and whether it lives in the view heap.
    fn plan(&self, binding: u32) -> Option<(&RangePlan, bool)> {
        self.views
            .iter()
            .map(|plan| (plan, true))
            .chain(self.samplers.iter().map(|plan| (plan, false)))
            .find(|(plan, _)| plan.binding == binding)
    }

    /// Where a dynamic binding sits in [`Self::roots`], and so in a group's
    /// addresses and in the layout's root parameter indices.
    fn root(&self, binding: u32) -> Option<(usize, &RootPlan)> {
        self.roots
            .iter()
            .enumerate()
            .find(|(_, plan)| plan.binding == binding)
    }
}

/// How many descriptors the layout's unbounded binding gets in this group.
fn variable_count(layout: &BindGroupLayoutRecord, desc: &BindGroupDesc<'_>) -> u32 {
    let Some((binding, _)) = layout.variable else {
        return 0;
    };
    if let Some(count) = desc.variable_count {
        return count;
    }
    // "Infer from entries", which the seam documents as the meaning of `None`:
    // one past the highest array index written into the variable binding.
    desc.entries
        .iter()
        .filter(|entry| entry.binding == binding)
        .map(|entry| entry.array_index + 1)
        .max()
        .unwrap_or(0)
}

/// Writes one entry's descriptor into a group's block, or resolves it to the
/// address a root descriptor is bound at.
///
/// A dynamic binding has no descriptor to write: it is a root CBV/SRV/UAV, and
/// what the group keeps for it is a GPU virtual address. Returning it rather
/// than storing it is what lets one function serve both `create_bind_group`,
/// which holds the record by value, and `update_bind_group`, which holds it
/// through the pool.
///
/// # Errors
///
/// [`HalError::InvalidDescriptor`] when the entry names a binding the layout
/// does not declare, a slot past the block the group holds, or a resource of the
/// wrong shape for the binding's kind.
pub(crate) fn write_entry(
    device: &ID3D12Device,
    heaps: &VisibleHeaps,
    layout: &BindGroupLayoutRecord,
    group: &BindGroupRecord,
    entry: &BindGroupEntry,
    resource: &Resolved,
) -> Result<Option<(usize, BoundBuffer)>, HalError> {
    if let Some((index, plan)) = layout.root(entry.binding) {
        return write_root(plan, entry, resource).map(|bound| Some((index, bound)));
    }
    let Some((plan, in_views)) = layout.plan(entry.binding) else {
        return Err(HalError::InvalidDescriptor(format!(
            "binding {} is not declared by this bind group's layout",
            entry.binding
        )));
    };
    let (block, heap) = if in_views {
        (group.views, &heaps.views)
    } else {
        (group.samplers, &heaps.samplers)
    };
    let Some(block) = block else {
        return Err(HalError::InvalidDescriptor(format!(
            "binding {} names a heap this bind group allocated no block in",
            entry.binding
        )));
    };
    let Some(slot) = plan.offset.checked_add(entry.array_index) else {
        return Err(HalError::InvalidDescriptor(format!(
            "binding {} index {} overflows a descriptor index",
            entry.binding, entry.array_index
        )));
    };
    if slot >= block.count {
        return Err(HalError::InvalidDescriptor(format!(
            "binding {} index {} is past the {} descriptor(s) this bind group holds",
            entry.binding, entry.array_index, block.count
        )));
    }
    resource.write(
        device,
        plan.range_type,
        heap.cpu(block, slot),
        entry.binding,
    )?;
    Ok(None)
}

/// Resolves a dynamic binding's entry to the address its root descriptor takes.
///
/// # Errors
///
/// [`HalError::InvalidDescriptor`] when the entry names an array element a root
/// descriptor does not have, or a resource that is not a buffer — a root
/// CBV/SRV/UAV is an address into one, and there is no image or sampler form.
fn write_root(
    plan: &RootPlan,
    entry: &BindGroupEntry,
    resource: &Resolved,
) -> Result<BoundBuffer, HalError> {
    if entry.array_index != 0 {
        return Err(HalError::InvalidDescriptor(format!(
            "binding {} index {} was written, and a dynamic binding is one root descriptor with \
             no array to index",
            entry.binding, entry.array_index
        )));
    }
    let Resolved::Buffer {
        address,
        offset,
        size,
        capacity,
        location,
        ..
    } = resource
    else {
        return Err(HalError::InvalidDescriptor(format!(
            "binding {} takes a dynamic offset and was given a {}; a root {:?} is an address \
             into a buffer",
            entry.binding,
            resource.what(),
            plan.parameter_type(),
        )));
    };
    // A root unordered access view is subject to D3D12's heap rule exactly as a
    // descriptor-table one is: the address it takes has to be a resource created
    // for unordered access, which no host-visible heap can be.
    if plan.parameter_type() == D3D12_ROOT_PARAMETER_TYPE_UAV {
        buffer::check_unordered_access(*location, entry.binding)?;
    }
    Ok(BoundBuffer {
        // The buffer's **base**, not the bound range's start: `crate::root`
        // adds the entry's offset and the dynamic one together, because it is
        // their sum that has to satisfy D3D12's address alignment.
        address: Some(*address),
        offset: *offset,
        size: *size,
        capacity: *capacity,
    })
}

/// A [`BindingResource`](crcbl_hal::BindingResource) resolved against the
/// device's tables.
///
/// Built in `crcbl_dx12::device`, where the pools are, and consumed here — the
/// same split every other entry point in this crate makes, so this module never
/// takes the device lock.
#[derive(Debug)]
pub(crate) enum Resolved {
    /// A buffer range: the resource, its GPU address, and the byte range.
    Buffer {
        /// The resource, kept so the group can hold a reference to it.
        raw: ID3D12Resource,
        /// `GetGPUVirtualAddress`, which a constant buffer view takes directly.
        address: u64,
        /// Byte offset into the buffer.
        offset: u64,
        /// Byte length, already resolved from the seam's `WHOLE_BUFFER`.
        size: u64,
        /// The buffer's **own** size. Only a dynamic binding reads it, and only
        /// to bound the offset a bind adds — see [`BoundBuffer::capacity`].
        capacity: u64,
        /// The bytes the resource occupies, which a constant buffer view is the
        /// one descriptor allowed to read to the end of — see
        /// [`buffer::allocation_size`].
        allocation: u64,
        /// Which heap it lives on, because D3D12 admits an unordered access
        /// view only on the default one.
        location: MemoryLocation,
    },
    /// An image view's descriptor in the device's CPU-visible heap.
    View {
        /// The image, kept so the group can hold a reference to it.
        raw: ID3D12Resource,
        /// Where `create_image_view` wrote the descriptor.
        descriptor: D3D12_CPU_DESCRIPTOR_HANDLE,
    },
    /// A sampler's descriptor in the device's CPU-visible sampler heap.
    Sampler {
        /// Where `create_sampler` wrote the descriptor.
        descriptor: D3D12_CPU_DESCRIPTOR_HANDLE,
    },
}

impl Resolved {
    /// The resource a descriptor written from this points into, if any.
    pub(crate) fn resource(&self) -> Option<&ID3D12Resource> {
        match self {
            Self::Buffer { raw, .. } | Self::View { raw, .. } => Some(raw),
            Self::Sampler { .. } => None,
        }
    }

    fn write(
        &self,
        device: &ID3D12Device,
        range_type: D3D12_DESCRIPTOR_RANGE_TYPE,
        at: D3D12_CPU_DESCRIPTOR_HANDLE,
        binding: u32,
    ) -> Result<(), HalError> {
        match self {
            Self::Buffer {
                address,
                offset,
                size,
                allocation,
                ..
            } if range_type == D3D12_DESCRIPTOR_RANGE_TYPE_CBV => {
                // Rounded **up** to D3D12's block, which is legal only because
                // `create_buffer` padded the allocation to the same block —
                // `crate::buffer` owns both halves of that rule and checks this
                // one against the allocation rather than assuming it.
                let bytes = buffer::constant_view_size(*offset, *size, *allocation, binding)?;
                let desc = D3D12_CONSTANT_BUFFER_VIEW_DESC {
                    BufferLocation: address + offset,
                    SizeInBytes: bytes,
                };
                // SAFETY: `desc` is a live local borrowed for the call, and `at`
                // is a descriptor inside this device's own shader-visible
                // CBV/SRV/UAV heap, which is the heap type this call writes.
                unsafe { device.CreateConstantBufferView(Some(&raw const desc), at) };
                Ok(())
            }
            Self::Buffer {
                raw,
                offset,
                size,
                location,
                ..
            } if range_type == D3D12_DESCRIPTOR_RANGE_TYPE_SRV
                || range_type == D3D12_DESCRIPTOR_RANGE_TYPE_UAV =>
            {
                // A **raw** view rather than a structured one, because the seam
                // has no element stride to give — `BindingResource::Buffer` is a
                // byte range. A raw view's element is four bytes and the HLSL's
                // own `StructuredBuffer<T>` declaration supplies the stride,
                // which is what `_FLAG_RAW` means and why the format is
                // `R32_TYPELESS`. `crate::buffer` owns the arithmetic and the
                // alignment D3D12 requires of the start.
                let (first, elements) = buffer::raw_view_range(*offset, *size, binding)?;
                if range_type == D3D12_DESCRIPTOR_RANGE_TYPE_SRV {
                    let desc = D3D12_SHADER_RESOURCE_VIEW_DESC {
                        Format: DXGI_FORMAT_R32_TYPELESS,
                        ViewDimension: D3D12_SRV_DIMENSION_BUFFER,
                        Shader4ComponentMapping: D3D12_DEFAULT_SHADER_4_COMPONENT_MAPPING,
                        Anonymous: D3D12_SHADER_RESOURCE_VIEW_DESC_0 {
                            Buffer: D3D12_BUFFER_SRV {
                                FirstElement: first,
                                NumElements: elements,
                                StructureByteStride: 0,
                                Flags: D3D12_BUFFER_SRV_FLAG_RAW,
                            },
                        },
                    };
                    // SAFETY: `raw` is a live buffer resource this device owns a
                    // reference to, `desc` is a live local borrowed for the call
                    // whose `Buffer` union member is the one its `BUFFER`
                    // dimension names, and `at` is a descriptor in this device's
                    // own shader-visible CBV/SRV/UAV heap.
                    unsafe { device.CreateShaderResourceView(raw, Some(&raw const desc), at) };
                } else {
                    // D3D12 admits an unordered access view only on the default
                    // heap, and a host-visible buffer bound for writing is a
                    // combination the seam permits and D3D12 cannot express.
                    // Refused here, where the binding can be named, rather than
                    // left to `CreateUnorderedAccessView` — which returns `void`
                    // and takes the device down at the next call.
                    buffer::check_unordered_access(*location, binding)?;
                    let desc = D3D12_UNORDERED_ACCESS_VIEW_DESC {
                        Format: DXGI_FORMAT_R32_TYPELESS,
                        ViewDimension: D3D12_UAV_DIMENSION_BUFFER,
                        Anonymous: D3D12_UNORDERED_ACCESS_VIEW_DESC_0 {
                            Buffer: D3D12_BUFFER_UAV {
                                FirstElement: first,
                                NumElements: elements,
                                StructureByteStride: 0,
                                CounterOffsetInBytes: 0,
                                Flags: D3D12_BUFFER_UAV_FLAG_RAW,
                            },
                        },
                    };
                    // SAFETY: as the shader resource view above. The counter
                    // resource is `None`, which is what a buffer with no append
                    // counter takes.
                    unsafe {
                        device.CreateUnorderedAccessView(raw, None, Some(&raw const desc), at);
                    }
                }
                Ok(())
            }
            Self::View { descriptor, .. } if range_type != D3D12_DESCRIPTOR_RANGE_TYPE_SAMPLER => {
                // SAFETY: both handles are descriptors in heaps of this device's
                // `CBV_SRV_UAV` type — the source written by
                // `create_image_view`, the destination allocated above — and one
                // descriptor is copied.
                unsafe {
                    device.CopyDescriptorsSimple(
                        1,
                        at,
                        *descriptor,
                        D3D12_DESCRIPTOR_HEAP_TYPE_CBV_SRV_UAV,
                    );
                }
                Ok(())
            }
            Self::Sampler { descriptor } if range_type == D3D12_DESCRIPTOR_RANGE_TYPE_SAMPLER => {
                // SAFETY: as above, for the `SAMPLER` heap type — the source
                // written by `create_sampler`.
                unsafe {
                    device.CopyDescriptorsSimple(
                        1,
                        at,
                        *descriptor,
                        D3D12_DESCRIPTOR_HEAP_TYPE_SAMPLER,
                    );
                }
                Ok(())
            }
            _ => Err(HalError::InvalidDescriptor(format!(
                "binding {binding} was given a {} for a {range_type:?} range",
                self.what()
            ))),
        }
    }

    /// What a mismatch message calls this resource.
    const fn what(&self) -> &'static str {
        match self {
            Self::Buffer { .. } => "buffer",
            Self::View { .. } => "image view",
            Self::Sampler { .. } => "sampler",
        }
    }
}

/// Writes a null shader resource view, which reads as zero.
///
/// D3D12 defines a null view as `CreateShaderResourceView` with no resource and
/// a concrete descriptor. See [`allocate_group`] for why every slot gets one.
fn null_view(device: &ID3D12Device, at: D3D12_CPU_DESCRIPTOR_HANDLE) {
    let desc = D3D12_SHADER_RESOURCE_VIEW_DESC {
        Format: DXGI_FORMAT_R32_TYPELESS,
        ViewDimension: D3D12_SRV_DIMENSION_BUFFER,
        Shader4ComponentMapping: D3D12_DEFAULT_SHADER_4_COMPONENT_MAPPING,
        Anonymous: D3D12_SHADER_RESOURCE_VIEW_DESC_0 {
            Buffer: D3D12_BUFFER_SRV {
                FirstElement: 0,
                NumElements: 0,
                StructureByteStride: 0,
                Flags: D3D12_BUFFER_SRV_FLAG_RAW,
            },
        },
    };
    // SAFETY: a null resource with a non-null descriptor is exactly what D3D12
    // documents as a null view. `desc` is a live local borrowed for the call
    // whose `Buffer` member is the one its `BUFFER` dimension names, and `at` is
    // a descriptor in this device's own shader-visible CBV/SRV/UAV heap.
    unsafe { device.CreateShaderResourceView(None, Some(&raw const desc), at) };
}

#[cfg(test)]
mod tests {
    use super::*;
    use crcbl_core::Handle;
    use crcbl_hal::{BindingResource, ImageViewType, SampleType};

    /// A device with the resource-binding tier this backend targets: bindless,
    /// and no mesh stage — which is what `crcbl-dx12`'s adapter reports.
    fn caps() -> DeviceCaps {
        DeviceCaps {
            features: crcbl_hal::Features::GPU_DRIVEN,
            limits: crcbl_hal::Limits::desktop(),
        }
    }

    fn entry(binding: u32, kind: BindingKind) -> BindGroupLayoutEntry {
        BindGroupLayoutEntry {
            binding,
            visibility: ShaderStages::ALL,
            kind,
            count: 1,
            flags: BindingFlags::empty(),
        }
    }

    fn plan(entries: &[BindGroupLayoutEntry]) -> Result<BindGroupLayoutRecord, HalError> {
        plan_layout(
            &BindGroupLayoutDesc {
                label: None,
                entries,
            },
            &caps(),
            7,
        )
    }

    /// Samplers and views land in different tables, at offsets counted per
    /// table.
    ///
    /// The offsets are the property under test: one running counter across both
    /// halves would put the second view at offset 1 here and the sampler at 2,
    /// and every descriptor a shader read past the first would be the wrong one
    /// — with nothing to report it, because a descriptor table is addressed by
    /// arithmetic and never by name.
    #[test]
    fn a_layout_splits_into_a_view_table_and_a_sampler_table() {
        let entries = [
            entry(
                0,
                BindingKind::StorageBuffer {
                    read_only: true,
                    dynamic: false,
                },
            ),
            entry(1, BindingKind::Sampler { comparison: false }),
            entry(
                2,
                BindingKind::SampledImage {
                    view_type: ImageViewType::D2,
                    sample_type: SampleType::Float,
                },
            ),
        ];
        let record = plan(&entries).expect("three ordinary bindings");

        assert_eq!(record.views.len(), 2);
        assert_eq!(record.samplers.len(), 1);
        assert_eq!(record.views[0].binding, 0);
        assert_eq!(record.views[0].offset, 0);
        assert_eq!(record.views[0].range_type, D3D12_DESCRIPTOR_RANGE_TYPE_SRV);
        assert_eq!(record.views[1].binding, 2);
        assert_eq!(
            record.views[1].offset, 1,
            "the second view must follow the first in the *view* table"
        );
        assert_eq!(
            record.samplers[0].offset, 0,
            "the sampler table counts from zero, independently of the views"
        );
        assert_eq!(record.view_descriptors, 2);
        assert_eq!(record.sampler_descriptors, 1);
        assert_eq!(record.owner, 7);
        assert_eq!(record.visibility, ShaderStages::ALL);
    }

    /// A writable storage buffer is a UAV and a read-only one an SRV — the split
    /// that has to agree with `StructuredBuffer` versus `RWStructuredBuffer` in
    /// the artifact, or the root signature declares a range the shader does not
    /// have.
    #[test]
    fn read_only_and_writable_storage_buffers_take_different_range_types() {
        for (read_only, expected) in [
            (true, D3D12_DESCRIPTOR_RANGE_TYPE_SRV),
            (false, D3D12_DESCRIPTOR_RANGE_TYPE_UAV),
        ] {
            let entries = [entry(
                0,
                BindingKind::StorageBuffer {
                    read_only,
                    dynamic: false,
                },
            )];
            let record = plan(&entries).expect("one storage buffer");
            assert_eq!(
                record.views[0].range_type, expected,
                "read_only={read_only}"
            );
        }
    }

    /// An unbounded range is `NumDescriptors = u32::MAX` in the root signature
    /// and contributes nothing to the fixed size of a block.
    #[test]
    fn a_variable_count_binding_is_declared_unbounded_and_sized_by_the_group() {
        let entries = [
            entry(0, BindingKind::UniformBuffer { dynamic: false }),
            BindGroupLayoutEntry {
                flags: BindingFlags::VARIABLE_COUNT | BindingFlags::PARTIALLY_BOUND,
                count: 4096,
                ..entry(
                    1,
                    BindingKind::SampledImage {
                        view_type: ImageViewType::D2,
                        sample_type: SampleType::Float,
                    },
                )
            },
        ];
        let layout = plan(&entries).expect("a bindless layout");
        assert_eq!(layout.variable, Some((1, true)));
        assert_eq!(layout.views[1].declared, u32::MAX);
        assert_eq!(layout.views[1].count, 0);
        assert_eq!(
            layout.view_descriptors, 1,
            "only the fixed binding counts towards a block's base size"
        );

        let layout_handle = Handle::from_bits(1 << 32).expect("generation 1");
        let desc = BindGroupDesc {
            label: None,
            layout: layout_handle,
            entries: &[],
            variable_count: Some(64),
        };
        assert_eq!(variable_count(&layout, &desc), 64);

        // `None` infers from the entries actually written, which is what the
        // seam documents it as meaning.
        let written = [BindGroupEntry {
            binding: 1,
            array_index: 9,
            resource: BindingResource::Sampler(Handle::from_bits(1 << 32).expect("generation 1")),
        }];
        let inferred = BindGroupDesc {
            entries: &written,
            variable_count: None,
            ..desc
        };
        assert_eq!(
            variable_count(&layout, &inferred),
            10,
            "the inferred count is one past the highest array index written"
        );
    }

    /// A layout the **seam** forbids is refused here too, because
    /// [`plan_layout`] runs [`BindGroupLayoutDesc::check_entries`].
    ///
    /// This backend has no device to open on this machine, so a shared checker
    /// nothing calls would look identical to one every backend calls — the
    /// guard whose scope matches nothing. This is where that is told apart.
    ///
    /// **What turns it red.** Deleting the `check_entries` call from
    /// [`plan_layout`]: each of these layouts then plans cleanly, because
    /// nothing left in this module states these rules.
    #[test]
    fn the_seams_own_rules_arrive_through_plan_layout() {
        let image = BindingKind::SampledImage {
            view_type: ImageViewType::D2,
            sample_type: SampleType::Float,
        };
        let cases: Vec<(&str, Vec<BindGroupLayoutEntry>)> = vec![
            (
                "count 0",
                vec![BindGroupLayoutEntry {
                    count: 0,
                    ..entry(0, image)
                }],
            ),
            (
                "declared twice",
                vec![
                    entry(0, image),
                    entry(0, BindingKind::Sampler { comparison: false }),
                ],
            ),
            (
                "not the last entry",
                vec![
                    BindGroupLayoutEntry {
                        flags: BindingFlags::VARIABLE_COUNT,
                        ..entry(0, image)
                    },
                    entry(1, BindingKind::Sampler { comparison: false }),
                ],
            ),
            (
                "not the highest-numbered binding",
                vec![
                    entry(5, BindingKind::Sampler { comparison: false }),
                    BindGroupLayoutEntry {
                        flags: BindingFlags::VARIABLE_COUNT,
                        ..entry(1, image)
                    },
                ],
            ),
            (
                "max_bindless_descriptors",
                vec![BindGroupLayoutEntry {
                    count: caps().limits.max_bindless_descriptors + 1,
                    ..entry(0, image)
                }],
            ),
        ];
        assert!(!cases.is_empty(), "nothing to check");
        for (expected, entries) in cases {
            let error = plan(&entries).expect_err(expected);
            let HalError::InvalidDescriptor(text) = &error else {
                panic!("{expected}: a layout the seam forbids is not {error:?}");
            };
            assert!(text.contains(expected), "{expected}: {text}");
        }

        // A mesh-visible binding, which is `Unsupported` rather than
        // `InvalidDescriptor` — this backend reports no mesh stage.
        let mesh = [BindGroupLayoutEntry {
            visibility: ShaderStages::MESH,
            ..entry(0, image)
        }];
        let error = plan(&mesh).expect_err("this backend reports no MESH_SHADER");
        assert!(
            matches!(error, HalError::Unsupported { backend, .. } if backend == BackendKind::Dx12),
            "{error:?}"
        );
    }

    /// What only D3D12 cannot express is refused by name, and each for its own
    /// reason — a single "unsupported layout" would send a reader to the wrong
    /// half of the descriptor.
    ///
    /// The portable causes a layout can also fail for — a zero count, a
    /// duplicate binding number, a misplaced `VARIABLE_COUNT` — are
    /// `BindGroupLayoutDesc::check_entries`', tested where that lives; that
    /// this backend runs them is
    /// [`the_seams_own_rules_arrive_through_plan_layout`].
    ///
    /// **What turns it red.** Accepting a dynamic-offset array, which would
    /// take the seam's one offset per binding and apply it to element zero
    /// while every other element read the wrong address. Accepting bindless
    /// flags on a root descriptor, which is not in a descriptor heap for any of
    /// them to describe.
    #[test]
    fn a_layout_d3d12_cannot_express_is_refused_by_name() {
        let cases: Vec<(&str, Vec<BindGroupLayoutEntry>)> = vec![
            (
                "one offset per dynamic binding rather than per element",
                vec![BindGroupLayoutEntry {
                    count: 4,
                    ..entry(0, BindingKind::UniformBuffer { dynamic: true })
                }],
            ),
            (
                "a root descriptor is not in one",
                vec![BindGroupLayoutEntry {
                    flags: BindingFlags::PARTIALLY_BOUND,
                    ..entry(
                        0,
                        BindingKind::StorageBuffer {
                            read_only: true,
                            dynamic: true,
                        },
                    )
                }],
            ),
        ];
        assert!(!cases.is_empty(), "nothing to check");
        for (expected, entries) in cases {
            let error = plan(&entries).expect_err(expected);
            let HalError::InvalidDescriptor(text) = &error else {
                panic!("{expected}: a descriptor D3D12 cannot express is not {error:?}");
            };
            assert!(text.contains(expected), "{expected}: {text}");
        }

        // The same dynamic binding at count 1 with no flags is fine, so both
        // refusals are about what was added and not about dynamic offsets.
        plan(&[entry(0, BindingKind::UniformBuffer { dynamic: true })])
            .expect("one dynamic uniform buffer");
    }

    /// The seam's `u32::MAX` is a request to clamp, and a table offset is a sum
    /// of counts — so a range that kept the sentinel would make every later
    /// range in its table start past the end of the heap.
    ///
    /// **What turns it red.** Planning `entry.count` instead of
    /// `resolved_count`: the first assertion reads back 4 294 967 295, and the
    /// second overflows the `offset + count` in `next_offset`. A
    /// `VARIABLE_COUNT` binding is the case that must *not* change — D3D12 has
    /// its own unbounded spelling and the count it contributes is zero.
    #[test]
    fn the_count_sentinel_is_resolved_before_it_reaches_a_descriptor_range() {
        let limits = caps().limits;
        let image = BindingKind::SampledImage {
            view_type: ImageViewType::D2,
            sample_type: SampleType::Float,
        };

        let flat = [BindGroupLayoutEntry {
            count: u32::MAX,
            ..entry(0, image)
        }];
        let layout = plan(&flat).expect("the sentinel is a request to clamp");
        assert_eq!(layout.views[0].count, limits.max_bindless_descriptors);
        assert_eq!(layout.views[0].declared, limits.max_bindless_descriptors);

        // A second range after it, so the offset arithmetic is exercised rather
        // than assumed.
        let pair = [
            flat[0],
            BindGroupLayoutEntry {
                count: 2,
                ..entry(1, image)
            },
        ];
        let layout = plan(&pair).expect("two ranges");
        assert_eq!(layout.views[1].offset, limits.max_bindless_descriptors);
        assert_eq!(layout.view_descriptors, limits.max_bindless_descriptors + 2);

        // With `VARIABLE_COUNT` the sentinel stays D3D12's unbounded range.
        let unbounded = [BindGroupLayoutEntry {
            count: u32::MAX,
            flags: BindingFlags::VARIABLE_COUNT | BindingFlags::PARTIALLY_BOUND,
            ..entry(0, image)
        }];
        let layout = plan(&unbounded).expect("a bindless layout");
        assert_eq!(layout.views[0].declared, u32::MAX);
        assert_eq!(layout.views[0].count, 0);
    }

    /// **A binding's register is its position among the bindings of its own
    /// class, and every set is space 0.**
    ///
    /// This replaces an assertion that the register *was* the binding number and
    /// the space *was* the set index. That claim was checked against nothing but
    /// itself and is false of every artifact this workspace commits — see
    /// `crate::dxil`'s `registers_are_assigned_per_class_in_declaration_order`,
    /// which reads the register out of the container's own resource table. A
    /// root signature built the old way names `t1` and `u2` for a shader that
    /// reads `t0` and `u0`, which pipeline creation rejects.
    #[test]
    fn a_bindings_register_is_its_index_among_its_own_class() {
        let entries = [
            entry(0, BindingKind::UniformBuffer { dynamic: false }),
            entry(
                1,
                BindingKind::StorageBuffer {
                    read_only: true,
                    dynamic: false,
                },
            ),
            entry(2, BindingKind::Sampler { comparison: false }),
            entry(
                3,
                BindingKind::StorageBuffer {
                    read_only: false,
                    dynamic: false,
                },
            ),
            entry(
                4,
                BindingKind::SampledImage {
                    view_type: ImageViewType::D2,
                    sample_type: SampleType::Float,
                },
            ),
        ];
        let layout = plan(&entries).expect("five bindings");
        let mut registers = dxil::Registers::default();
        let tables = ranges(&layout, &mut registers);
        let (views, samplers) = (&tables.views, &tables.samplers);

        // In `views` order, which is declaration order: b0, t0, u0, t1.
        assert_eq!(views.len(), 4);
        assert_eq!(views[0].BaseShaderRegister, 0, "the only CBV is b0");
        assert_eq!(views[1].BaseShaderRegister, 0, "the first SRV is t0");
        assert_eq!(views[2].BaseShaderRegister, 0, "the only UAV is u0");
        assert_eq!(
            views[3].BaseShaderRegister, 1,
            "the second SRV is t1, and the UAV between them did not consume a t"
        );
        assert_eq!(samplers.len(), 1);
        assert_eq!(samplers[0].BaseShaderRegister, 0, "the only sampler is s0");
        for range in views.iter().chain(samplers) {
            assert_eq!(
                range.RegisterSpace, 0,
                "Slang's HLSL output puts every set in space 0"
            );
        }
        assert_eq!(views[0].NumDescriptors, 1);
        assert_eq!(views[0].OffsetInDescriptorsFromTableStart, 0);
        assert!(tables.roots.is_empty(), "no binding here is dynamic");

        // The counter carries across sets, so a second layout continues where
        // the first stopped — which is what makes a two-set pipeline layout
        // agree with a source `dxc` numbered end to end.
        let more = [entry(
            0,
            BindingKind::StorageBuffer {
                read_only: true,
                dynamic: false,
            },
        )];
        let second = plan(&more).expect("one binding");
        let tables = ranges(&second, &mut registers);
        assert_eq!(tables.views[0].BaseShaderRegister, 2, "the third SRV is t2");
    }

    /// **A dynamic binding leaves the descriptor table and becomes a root
    /// descriptor, and every binding after it keeps the offset it had.**
    ///
    /// The table offsets are the assertion that would otherwise go wrong
    /// silently: a dynamic binding occupies no descriptor, so leaving a gap for
    /// it would put every later binding one slot past the descriptor its group
    /// wrote — a shader reading the wrong resource, with nothing in D3D12 to say
    /// so. The registers are the other half: it is still a `ConstantBuffer` in
    /// the source, so it still takes `b1` and the CBV after it takes `b2`.
    #[test]
    fn a_dynamic_binding_leaves_the_table_and_becomes_a_root_descriptor() {
        let entries = [
            entry(0, BindingKind::UniformBuffer { dynamic: false }),
            entry(1, BindingKind::UniformBuffer { dynamic: true }),
            entry(2, BindingKind::UniformBuffer { dynamic: false }),
            entry(
                3,
                BindingKind::StorageBuffer {
                    read_only: false,
                    dynamic: true,
                },
            ),
            entry(
                4,
                BindingKind::SampledImage {
                    view_type: ImageViewType::D2,
                    sample_type: SampleType::Float,
                },
            ),
        ];
        let layout = plan(&entries).expect("two dynamic bindings among three table ones");

        assert_eq!(
            layout.views.len(),
            3,
            "the two dynamic bindings are not here"
        );
        assert_eq!(
            layout
                .views
                .iter()
                .map(|plan| plan.offset)
                .collect::<Vec<_>>(),
            vec![0, 1, 2],
            "the table packs, so nothing reserves a slot for a root descriptor"
        );
        assert_eq!(layout.view_descriptors, 3);
        assert_eq!(
            layout
                .roots
                .iter()
                .map(|plan| plan.binding)
                .collect::<Vec<_>>(),
            vec![1, 3],
            "ascending, which is the order dynamic offsets arrive in"
        );
        assert!(layout.roots[0].uniform(), "binding 1 is a root CBV");
        assert!(
            !layout.roots[1].uniform(),
            "binding 3 is a writable storage buffer, so a root UAV"
        );

        let mut registers = dxil::Registers::default();
        let tables = ranges(&layout, &mut registers);
        assert_eq!(
            tables
                .views
                .iter()
                .map(|range| range.BaseShaderRegister)
                .collect::<Vec<_>>(),
            vec![0, 2, 0],
            "b0 and b2 in the table, and t0 for the sampled image — b1 went to the root descriptor"
        );
        assert_eq!(tables.roots.len(), 2);
        assert_eq!(
            tables.roots[0].parameter_type,
            D3D12_ROOT_PARAMETER_TYPE_CBV
        );
        assert_eq!(tables.roots[0].descriptor.ShaderRegister, 1, "b1");
        assert_eq!(tables.roots[0].descriptor.RegisterSpace, 0);
        assert_eq!(
            tables.roots[1].parameter_type,
            D3D12_ROOT_PARAMETER_TYPE_UAV
        );
        assert_eq!(tables.roots[1].descriptor.ShaderRegister, 0, "u0");
    }

    /// A block's addresses are its start times the stride, in both spaces —
    /// which is the arithmetic every descriptor table depends on and which no
    /// D3D12 call would report as wrong.
    #[test]
    fn a_blocks_addresses_follow_its_start_and_the_heaps_stride() {
        let mut heap = VisibleHeap::new(D3D12_DESCRIPTOR_HEAP_TYPE_CBV_SRV_UAV, 16);
        heap.stride = 32;
        heap.cpu_base = 0x1000;
        heap.gpu_base = 0x2000;
        let block = Block { start: 3, count: 2 };
        assert_eq!(heap.cpu(block, 0).ptr, 0x1000 + 3 * 32);
        assert_eq!(heap.cpu(block, 1).ptr, 0x1000 + 4 * 32);
        assert_eq!(heap.gpu(block).ptr, 0x2000 + 3 * 32);
    }

    /// The bump allocator hands out disjoint blocks, recycles a freed one at its
    /// own size, and refuses rather than wrapping when the heap is full.
    ///
    /// Runs against [`VisibleHeap::take`], the real allocator, with no device —
    /// which is why that arithmetic is a function of its own.
    #[test]
    fn blocks_are_disjoint_recycled_by_size_and_bounded_by_the_capacity() {
        let mut heap = VisibleHeap::new(D3D12_DESCRIPTOR_HEAP_TYPE_CBV_SRV_UAV, 8);

        let first = heap.take(3).expect("three descriptors");
        let second = heap.take(2).expect("two more");
        assert_eq!(first, Block { start: 0, count: 3 });
        assert_eq!(
            second,
            Block { start: 3, count: 2 },
            "the second block must start past the first"
        );

        heap.free(first);
        assert_eq!(
            heap.take(3).expect("the recycled block"),
            first,
            "a freed block must come back at its own address"
        );
        assert_eq!(
            heap.take(2).expect("a fresh block"),
            Block { start: 5, count: 2 },
            "a size the free list does not hold must bump rather than reuse"
        );
        assert!(
            matches!(heap.take(2), Err(HalError::OutOfDeviceMemory)),
            "a full heap must refuse rather than hand out a block past its end"
        );
        assert_eq!(heap.take(1).expect("the last descriptor").start, 7);
    }
}
