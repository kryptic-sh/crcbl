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
//! What is **not** honoured is a dynamic offset:
//! [`BindingKind::UniformBuffer`](crcbl_hal::BindingKind::UniformBuffer)'s
//! `dynamic` and its storage-buffer twin are refused at layout creation. A
//! descriptor table has no offset to apply — D3D12's answer is a *root*
//! CBV/SRV/UAV, a different root parameter type carrying a raw GPU address,
//! which changes how every binding in the set is reached rather than adding to
//! it.

use crcbl_hal::{
    BindGroupDesc, BindGroupEntry, BindGroupLayoutDesc, BindGroupLayoutEntry,
    BindGroupLayoutHandle, BindingFlags, BindingKind, HalError, ShaderStages,
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
    D3D12_MAX_SHADER_VISIBLE_SAMPLER_HEAP_SIZE, D3D12_SHADER_RESOURCE_VIEW_DESC,
    D3D12_SHADER_RESOURCE_VIEW_DESC_0, D3D12_SRV_DIMENSION_BUFFER, D3D12_UAV_DIMENSION_BUFFER,
    D3D12_UNORDERED_ACCESS_VIEW_DESC, D3D12_UNORDERED_ACCESS_VIEW_DESC_0, ID3D12DescriptorHeap,
    ID3D12Device, ID3D12Resource,
};
use windows::Win32::Graphics::Dxgi::Common::DXGI_FORMAT_R32_TYPELESS;

use crate::conv;
use crate::handle::Owned;

/// Descriptors in the one shader-visible CBV/SRV/UAV heap.
///
/// Not `D3D12_MAX_SHADER_VISIBLE_DESCRIPTOR_HEAP_SIZE_TIER_2`, which is a
/// million: the heap is allocated up front and in full, so the ceiling is also a
/// per-device cost every caller pays. This is large enough for a bindless page
/// of textures beside a frame's ordinary sets, and small enough to be
/// unremarkable. Raising it is a one-line change with a measurable cost, which
/// is the property that matters.
const VIEW_DESCRIPTORS: u32 = 4096;

/// Descriptors in the one shader-visible sampler heap.
///
/// D3D12's own hard ceiling, because a sampler descriptor is small and there is
/// no smaller number that is not arbitrary.
const SAMPLER_DESCRIPTORS: u32 = D3D12_MAX_SHADER_VISIBLE_SAMPLER_HEAP_SIZE;

/// A constant buffer view's size must be a multiple of this many bytes.
///
/// D3D12's rule, not a choice here: `SizeInBytes` is rounded **up** to it, so a
/// caller's legal seam-side size is accepted and the view reads padding the
/// caller's own buffer already owns.
const CONSTANT_BUFFER_ALIGNMENT: u32 = 256;

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

/// A block of descriptors inside one shader-visible heap.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct Block {
    start: u32,
    count: u32,
}

/// A bind group layout: the ranges it declares, split by heap type.
#[derive(Debug)]
pub(crate) struct BindGroupLayoutRecord {
    pub(crate) owner: u64,
    /// CBV/SRV/UAV ranges, in declaration order.
    pub(crate) views: Vec<RangePlan>,
    /// Sampler ranges, in declaration order.
    pub(crate) samplers: Vec<RangePlan>,
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
/// # Errors
///
/// [`HalError::InvalidDescriptor`] for a descriptor D3D12 has no encoding for:
/// a duplicate binding number, a zero-length binding, a dynamic offset (see the
/// module docs), or a [`BindingFlags::VARIABLE_COUNT`] entry that is not both
/// last in the slice and highest-numbered — the rule
/// [`BindGroupLayoutDesc::entries`] states, and which every "the variable
/// binding is `entries.last()`" reading depends on.
pub(crate) fn plan_layout(
    desc: &BindGroupLayoutDesc<'_>,
    owner: u64,
) -> Result<BindGroupLayoutRecord, HalError> {
    let mut views: Vec<RangePlan> = Vec::new();
    let mut samplers: Vec<RangePlan> = Vec::new();
    let mut visibility = ShaderStages::empty();
    let mut variable = None;

    for (index, entry) in desc.entries.iter().enumerate() {
        check_entry(desc, index, entry)?;
        visibility |= entry.visibility;
        let unbounded = entry.flags.contains(BindingFlags::VARIABLE_COUNT);
        let range_type = conv::descriptor_range_type(entry.kind);
        let in_views = range_type.is_some();
        let table = if in_views { &mut views } else { &mut samplers };
        table.push(RangePlan {
            binding: entry.binding,
            range_type: range_type.unwrap_or(D3D12_DESCRIPTOR_RANGE_TYPE_SAMPLER),
            count: if unbounded { 0 } else { entry.count },
            offset: next_offset(table),
            declared: if unbounded { u32::MAX } else { entry.count },
        });
        if unbounded {
            variable = Some((entry.binding, in_views));
        }
    }

    Ok(BindGroupLayoutRecord {
        owner,
        view_descriptors: next_offset(&views),
        sampler_descriptors: next_offset(&samplers),
        views,
        samplers,
        variable,
        visibility,
    })
}

/// Where the next range starts in a table.
fn next_offset(ranges: &[RangePlan]) -> u32 {
    ranges.last().map_or(0, |range| range.offset + range.count)
}

/// Everything about one layout entry D3D12 cannot express, named.
fn check_entry(
    desc: &BindGroupLayoutDesc<'_>,
    index: usize,
    entry: &BindGroupLayoutEntry,
) -> Result<(), HalError> {
    if desc.entries[..index]
        .iter()
        .any(|earlier| earlier.binding == entry.binding)
    {
        return Err(HalError::InvalidDescriptor(format!(
            "binding {} is declared twice in one bind group layout",
            entry.binding
        )));
    }
    if entry.count == 0 {
        return Err(HalError::InvalidDescriptor(format!(
            "binding {} has a count of zero, which is a range no shader can index",
            entry.binding
        )));
    }
    if matches!(
        entry.kind,
        BindingKind::UniformBuffer { dynamic: true }
            | BindingKind::StorageBuffer { dynamic: true, .. }
    ) {
        return Err(HalError::InvalidDescriptor(format!(
            "binding {} takes a dynamic offset, and a D3D12 descriptor table has none to apply: \
             the equivalent is a root CBV/SRV/UAV, which is a different root parameter type for \
             every binding in the set (the DX12 dynamic-offset slice)",
            entry.binding
        )));
    }
    if entry.flags.contains(BindingFlags::VARIABLE_COUNT) {
        if index + 1 != desc.entries.len() {
            return Err(HalError::InvalidDescriptor(format!(
                "binding {} is VARIABLE_COUNT but is not the last entry of its layout",
                entry.binding
            )));
        }
        if desc
            .entries
            .iter()
            .any(|other| other.binding > entry.binding)
        {
            return Err(HalError::InvalidDescriptor(format!(
                "binding {} is VARIABLE_COUNT but is not the highest binding number of its layout",
                entry.binding
            )));
        }
    }
    Ok(())
}

/// The `D3D12_DESCRIPTOR_RANGE` array a root parameter points at.
///
/// `RegisterSpace` is the **set index**, which is how Slang's
/// `[[vk::binding(binding, set)]]` reaches HLSL: set 0 becomes `register(t0)` in
/// the default space, and set *n* becomes `space{n}`. So the seam's set index
/// and the artifact's register space are the same number by construction rather
/// than through a mapping either side could get wrong.
pub(crate) fn ranges(plans: &[RangePlan], set: u32) -> Vec<D3D12_DESCRIPTOR_RANGE> {
    plans
        .iter()
        .map(|plan| D3D12_DESCRIPTOR_RANGE {
            RangeType: plan.range_type,
            NumDescriptors: plan.declared,
            BaseShaderRegister: plan.binding,
            RegisterSpace: set,
            OffsetInDescriptorsFromTableStart: plan.offset,
        })
        .collect()
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

/// Writes one entry's descriptor into a group's block.
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
) -> Result<(), HalError> {
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
    )
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
                ..
            } if range_type == D3D12_DESCRIPTOR_RANGE_TYPE_CBV => {
                let Ok(bytes) = u32::try_from(*size) else {
                    return Err(HalError::InvalidDescriptor(format!(
                        "binding {binding} binds {size} bytes as a constant buffer, and D3D12's \
                         SizeInBytes is 32-bit"
                    )));
                };
                let desc = D3D12_CONSTANT_BUFFER_VIEW_DESC {
                    BufferLocation: address + offset,
                    SizeInBytes: bytes.next_multiple_of(CONSTANT_BUFFER_ALIGNMENT),
                };
                // SAFETY: `desc` is a live local borrowed for the call, and `at`
                // is a descriptor inside this device's own shader-visible
                // CBV/SRV/UAV heap, which is the heap type this call writes.
                unsafe { device.CreateConstantBufferView(Some(&raw const desc), at) };
                Ok(())
            }
            Self::Buffer {
                raw, offset, size, ..
            } if range_type == D3D12_DESCRIPTOR_RANGE_TYPE_SRV
                || range_type == D3D12_DESCRIPTOR_RANGE_TYPE_UAV =>
            {
                // A **raw** view rather than a structured one, because the seam
                // has no element stride to give — `BindingResource::Buffer` is a
                // byte range. A raw view's element is four bytes and the HLSL's
                // own `StructuredBuffer<T>` declaration supplies the stride,
                // which is what `_FLAG_RAW` means and why the format is
                // `R32_TYPELESS`.
                let first = offset / 4;
                let elements = u32::try_from(size / 4).unwrap_or(u32::MAX);
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
    use crcbl_hal::BindingResource;

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
            entry(1, BindingKind::Sampler),
            entry(2, BindingKind::SampledImage),
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
                ..entry(1, BindingKind::SampledImage)
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

    /// Everything D3D12 cannot express is refused by name, and each for its own
    /// reason — a single "unsupported layout" would send a reader to the wrong
    /// half of the descriptor.
    #[test]
    fn a_layout_d3d12_cannot_express_is_refused_by_name() {
        let cases: Vec<(&str, Vec<BindGroupLayoutEntry>)> = vec![
            (
                "twice",
                vec![
                    entry(0, BindingKind::SampledImage),
                    entry(0, BindingKind::Sampler),
                ],
            ),
            (
                "count of zero",
                vec![BindGroupLayoutEntry {
                    count: 0,
                    ..entry(0, BindingKind::SampledImage)
                }],
            ),
            (
                "dynamic offset",
                vec![entry(0, BindingKind::UniformBuffer { dynamic: true })],
            ),
            (
                "dynamic offset",
                vec![entry(
                    0,
                    BindingKind::StorageBuffer {
                        read_only: true,
                        dynamic: true,
                    },
                )],
            ),
            (
                "not the last entry",
                vec![
                    BindGroupLayoutEntry {
                        flags: BindingFlags::VARIABLE_COUNT,
                        ..entry(0, BindingKind::SampledImage)
                    },
                    entry(1, BindingKind::Sampler),
                ],
            ),
            (
                "highest binding number",
                vec![
                    entry(5, BindingKind::Sampler),
                    BindGroupLayoutEntry {
                        flags: BindingFlags::VARIABLE_COUNT,
                        ..entry(1, BindingKind::SampledImage)
                    },
                ],
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
    }

    /// The register space is the set index, which is the agreement that makes a
    /// Slang `[[vk::binding(b, s)]]` reach the register the artifact declares.
    #[test]
    fn ranges_put_the_set_index_in_the_register_space() {
        let entries = [entry(3, BindingKind::SampledImage)];
        let layout = plan(&entries).expect("one binding");
        let ranges = ranges(&layout.views, 2);
        assert_eq!(ranges.len(), 1);
        assert_eq!(ranges[0].BaseShaderRegister, 3);
        assert_eq!(ranges[0].RegisterSpace, 2);
        assert_eq!(ranges[0].NumDescriptors, 1);
        assert_eq!(ranges[0].OffsetInDescriptorsFromTableStart, 0);
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
