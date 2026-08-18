//! Bind group layouts, bind groups, and the Metal argument tables they occupy.
//!
//! # The decision: **flat argument-table slots, not argument buffers**
//!
//! `docs/plan/09-backends-metal-dx12.md`'s mapping table offers "argument
//! buffers (tier 2) — resource heaps + `useResource` residency management (the
//! real work)" for bindless, and MTL1 reported
//! [`DESCRIPTOR_INDEXING`](crcbl_hal::Features::DESCRIPTOR_INDEXING) off the
//! back of `argumentBuffersSupport`. This slice takes the *other* option —
//! `setVertexBuffer:offset:atIndex:` and its five siblings — and the reason is
//! not a preference. It is what the engine's own shaders are compiled to.
//!
//! **Every MSL artifact `crcbl-shaders` commits declares plain per-stage
//! arguments.** `msl/triangle.metal` reads
//! `Vertex_natural_0 device* vertices_1 [[buffer(0)]]`, `msl/sprite.metal`
//! reads a `constant*` at `[[buffer(0)]]`, a `device*` at `[[buffer(1)]]`, a
//! `texture2d` at `[[texture(0)]]` and a `sampler` at `[[sampler(0)]]`. Not one
//! of them declares an argument-buffer struct, because Slang's Metal target
//! lowers `ConstantBuffer`/`StructuredBuffer`/`Texture2D`/`SamplerState` to the
//! argument tables and nothing else.
//!
//! An `MTLArgumentEncoder` writes a *descriptor block* into a buffer. Binding
//! that block at `[[buffer(0)]]` of a shader that declared
//! `device Vertex*` there does not fail: the shader reads the descriptor words
//! as vertex data. So argument buffers are not the harder-but-better option
//! here — they are the option that silently draws garbage with the artifacts
//! this engine actually ships. They become the right answer on the day
//! `crcbl-shaders` emits `ParameterBlock`-shaped MSL, and not before.
//!
//! Two consequences follow, and both are stated rather than absorbed:
//!
//! * **Residency needs no `useResource`.** That call exists because Metal
//!   cannot see through an argument buffer or an `MTLHeap` to the resources
//!   behind it. A directly bound resource is one Metal knows the encoder
//!   references, so it is made resident *and* hazard-tracked automatically —
//!   which is also what keeps `crcbl_mtl::command`'s barrier-is-an-encoder-
//!   boundary argument intact, since that argument rests on every resource
//!   being tracked.
//! * **`DESCRIPTOR_INDEXING` comes off.** See [`plan_set`]: a
//!   [`BindingFlags`](crcbl_hal::BindingFlags) of any kind is refused here,
//!   and `crcbl_hal::pipeline` is explicit that a backend which refuses them
//!   must not report the feature. `crcbl_mtl::adapter` carries the full
//!   argument.
//!
//! # The index a binding lands on, and the rule that decides it
//!
//! Metal has no set/binding pair. Each stage has three flat tables — buffers,
//! textures, samplers — and a shader argument names an index in one of them. So
//! a backend has to *flatten* the seam's `(set, binding)` into a per-table
//! index, and the flattening has to be the one the shader was compiled with or
//! every draw reads the wrong resource.
//!
//! **The rule this backend adopts: ascending `(set, binding)`, counted
//! per table.** A binding's index is the number of same-table descriptors
//! declared before it — earlier sets first, then lower binding numbers within a
//! set — plus its [`array_index`](crcbl_hal::BindGroupEntry::array_index). The
//! per-set half is computed once in [`plan_set`] and the per-layout half once in
//! [`plan_layout`], so a bind is an addition rather than a search.
//!
//! Checked against the committed artifacts, which is the only evidence that
//! matters: `triangle.metal` (`vertices` at set 0 binding 0 → `buffer(0)`),
//! `mesh.metal` (`frame` 0/0 → `buffer(0)`, `vertices` 0/1 → `buffer(1)`),
//! `sprite.metal` (`constants` 0/0 → `buffer(0)`, `sprites` 0/1 →
//! `buffer(1)`, `sheet` 1/0 → `texture(0)`, `sheetSampler` 1/1 →
//! `sampler(0)`) and `tonemap.metal` (`scene` 0/0 → `texture(0)`,
//! `sceneSampler` 0/1 → `sampler(0)`) all agree with it exactly.
//!
//! **`ui.slang` did not, and the fix was its own.** It declared its constant
//! buffer *before* the storage buffer in source while numbering it *after* —
//! `constants` at binding 3, `vertices` at binding 2 — and Slang assigns Metal
//! indices in declaration order, so its MSL had `constants` at `buffer(0)` and
//! `vertices` at `buffer(1)`, the reverse of what this rule computes. What that
//! produced was not a diagnostic: the UI vertex stage read the viewport
//! constants as its vertex array, every quad landed nowhere, and macOS drew a
//! game with no text in it. Nothing below the seam can detect it either —
//! reflection would name the shader's own parameter names, and a
//! [`BindGroupLayoutEntry`] has no name to compare them with.
//!
//! It now declares its resources in the order it numbers them, so
//! `msl/ui.metal` has `vertices` at `buffer(0)` and `constants` at `buffer(1)`.
//! The rule holds across every committed artifact again, and `crcbl-shaders`'
//! `declaration_order` lint now checks it over every source rather than leaving
//! it to a comment.
//!
//! # A bind group holds the Metal objects, not the handles
//!
//! [`create_bind_group`](crcbl_hal::Device::create_bind_group) resolves every
//! [`BindingResource`] to its `MTLBuffer`/`MTLTexture`/`MTLSamplerState` and
//! retains it, exactly as Vulkan writes descriptors at creation time. Two
//! things follow: a handle destroyed after the group was built cannot dangle,
//! because the group holds a reference of its own; and
//! [`bind_group`](crcbl_hal::CommandEncoder::bind_group) does no pool lookup
//! per resource, which is what keeps the per-draw path an index computation.
//!
//! Two consequences of that shape are stated rather than left to be found:
//!
//! * **A slot the group never filled is not bound**, and Metal leaves whatever
//!   the previous bind put in that argument-table entry. This backend does not
//!   refuse a partially filled group, because
//!   [`update_bind_group`](crcbl_hal::Device::update_bind_group) exists
//!   precisely so a group can be created and filled later — so "every
//!   descriptor written" is not a property creation can check. It is the same
//!   hazard Vulkan has and leaves to its validation layer.
//! * **An update does not reach a command buffer that already bound the
//!   group.** The values were copied onto the encoder when `bind_group` ran, so
//!   a later write lands on the next bind. That is exactly what refusing
//!   [`UPDATE_AFTER_BIND`](crcbl_hal::BindingFlags::UPDATE_AFTER_BIND) means,
//!   and it is why refusing it is not cosmetic.

use crcbl_hal::{
    BindGroupDesc, BindGroupEntry, BindGroupHandle, BindGroupLayoutDesc, BindGroupLayoutEntry,
    BindGroupLayoutHandle, BindingKind, BindingResource, HalError, Limits, PipelineLayoutHandle,
    ShaderStages,
};
use objc2::rc::Retained;
use objc2::runtime::ProtocolObject;
use objc2_metal::{
    MTLBuffer, MTLComputeCommandEncoder, MTLRenderCommandEncoder, MTLSamplerState, MTLTexture,
};

use crate::argument::BUFFER_TABLE_ENTRIES;
use crate::device::{
    DeviceInner, DeviceState, MetalDevice, Owned, lookup, lookup_mut, owned, take_owned, to_ns,
};

/// Entries in Metal's per-stage **texture** argument table. See
/// [`BUFFER_TABLE_ENTRIES`], which carries the argument for all three and lives
/// in [`crate::argument`] because a push-constant block competes for it.
const TEXTURE_TABLE_ENTRIES: u32 = 128;

/// Entries in Metal's per-stage **sampler** argument table. See
/// [`TEXTURE_TABLE_ENTRIES`].
const SAMPLER_TABLE_ENTRIES: u32 = 16;

/// How many argument tables Metal gives a stage.
const TABLES: usize = 3;

/// Which of Metal's three per-stage argument tables a binding occupies.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Table {
    /// `setVertexBuffer:offset:atIndex:` and its fragment sibling.
    Buffer,
    /// `setVertexTexture:atIndex:` and its fragment sibling.
    Texture,
    /// `setVertexSamplerState:atIndex:` and its fragment sibling.
    Sampler,
}

impl Table {
    /// Every table, in the order [`TableCounts`] indexes them.
    const ALL: [Self; TABLES] = [Self::Buffer, Self::Texture, Self::Sampler];

    /// The table a binding kind lands in.
    ///
    /// Metal has one texture table for sampled and storage images alike — the
    /// read/write distinction is the texture's own `MTLTextureUsage`, set at
    /// creation, not a second argument table — and a uniform and a storage
    /// buffer are both `MTLBuffer` bound to the same slots. Both collapses are
    /// Metal's, not this backend's.
    ///
    /// [`BindingKind::SampledImage`]'s `view_type` and `sample_type` are both
    /// dropped here and nothing else in this backend reads either: an
    /// `MTLTexture` carries its own `textureType` and `pixelFormat`, set by
    /// `conv::view_texture_type` and `conv::pixel_format` when the view was
    /// created, and binding one into an argument table takes neither a dimension
    /// nor a format. [`BindingKind::StorageImage`]'s `view_type` and `format`
    /// are dropped in the same place and for the same reason — a
    /// `RWTexture2D<float4>` is bound with `setComputeTexture:atIndex:`, which
    /// takes a texture and a slot and nothing else, and the texture already
    /// knows both. [`BindingKind::Sampler`]'s `comparison` goes the same way: an
    /// `MTLSamplerState` decides whether it compares through its descriptor's
    /// `compareFunction`, which is where `SamplerDesc::compare` lands. Only
    /// WebGPU wants any of the five in the layout.
    const fn of(kind: BindingKind) -> Self {
        match kind {
            BindingKind::UniformBuffer { .. } | BindingKind::StorageBuffer { .. } => Self::Buffer,
            BindingKind::SampledImage { .. } | BindingKind::StorageImage { .. } => Self::Texture,
            BindingKind::Sampler { .. } => Self::Sampler,
        }
    }

    /// Position in a [`TableCounts`].
    pub(crate) const fn slot(self) -> usize {
        match self {
            Self::Buffer => 0,
            Self::Texture => 1,
            Self::Sampler => 2,
        }
    }

    /// How many entries this table has.
    const fn capacity(self) -> u32 {
        match self {
            Self::Buffer => BUFFER_TABLE_ENTRIES,
            Self::Texture => TEXTURE_TABLE_ENTRIES,
            Self::Sampler => SAMPLER_TABLE_ENTRIES,
        }
    }

    /// What to call this table in an error message.
    const fn name(self) -> &'static str {
        match self {
            Self::Buffer => "buffer",
            Self::Texture => "texture",
            Self::Sampler => "sampler",
        }
    }
}

/// How many entries of each table something occupies, indexed by
/// [`Table::slot`].
pub(crate) type TableCounts = [u32; TABLES];

/// One binding of a set, placed in its table.
#[derive(Clone, Copy, Debug)]
pub(crate) struct Slot {
    pub(crate) binding: u32,
    pub(crate) kind: BindingKind,
    pub(crate) table: Table,
    /// Index of this binding's first descriptor **within its own set**.
    pub(crate) first: u32,
    pub(crate) count: u32,
    pub(crate) visibility: ShaderStages,
    /// Whether this binding takes one of
    /// [`bind_group`](crcbl_hal::CommandEncoder::bind_group)'s dynamic offsets.
    pub(crate) dynamic: bool,
}

/// A bind-group layout, placed: every binding's table and index within the set,
/// plus what the set occupies as a whole.
#[derive(Clone, Debug)]
pub(crate) struct SetPlan {
    /// Ascending by [`Slot::binding`], which is the order the flattening rule
    /// assigns indices in and the order dynamic offsets arrive in.
    pub(crate) slots: Vec<Slot>,
    pub(crate) totals: TableCounts,
}

impl SetPlan {
    /// The binding a [`Slot`] with this number describes, if the set has one.
    fn slot(&self, binding: u32) -> Option<&Slot> {
        self.slots.iter().find(|slot| slot.binding == binding)
    }

    /// The bindings that take a dynamic offset, ascending — which is the order
    /// [`bind_group`](crcbl_hal::CommandEncoder::bind_group) documents its
    /// `dynamic_offsets` in.
    fn dynamic_bindings(&self) -> impl Iterator<Item = u32> + '_ {
        self.slots
            .iter()
            .filter(|slot| slot.dynamic)
            .map(|slot| slot.binding)
    }
}

/// Places one set's bindings in the argument tables, or says why it cannot be
/// placed at all.
///
/// Pure, and separated from every pool and every driver call for the reason
/// `crcbl_mtl::conv` is: these are the *decisions* — the flattening rule and
/// which layouts this backend can honour — and they deserve a unit test rather
/// than a GPU.
///
/// # Errors
///
/// * [`HalError::InvalidDescriptor`] for a layout the seam already forbids: a
///   binding with `count` 0, or a binding number declared twice.
/// * [`HalError::InvalidDescriptor`] for a dynamic binding with `count` other
///   than 1. The seam gives one offset per dynamic binding, so an array of them
///   has no offset per element to be given.
/// * [`HalError::InvalidDescriptor`] when the set needs more entries of one
///   table than Metal has.
/// * [`HalError::Unsupported`] for any [`BindingFlags`](crcbl_hal::BindingFlags).
///   All three describe a bindless array, this backend binds flat argument-table
///   slots, and `crcbl_hal::pipeline` requires the refusal to be loud: "a
///   bindless array quietly downgraded to a fixed one reads garbage at index
///   4097".
pub(crate) fn plan_set(desc: &BindGroupLayoutDesc<'_>) -> Result<SetPlan, HalError> {
    let mut ordered: Vec<&BindGroupLayoutEntry> = desc.entries.iter().collect();
    ordered.sort_by_key(|entry| entry.binding);

    let mut slots: Vec<Slot> = Vec::with_capacity(ordered.len());
    let mut totals: TableCounts = [0; TABLES];
    let mut previous: Option<u32> = None;
    for entry in ordered {
        if entry.count == 0 {
            return Err(HalError::InvalidDescriptor(format!(
                "binding {} has count 0; a binding must hold at least one descriptor",
                entry.binding
            )));
        }
        // The list is sorted, so a repeat is adjacent.
        if previous == Some(entry.binding) {
            return Err(HalError::InvalidDescriptor(format!(
                "binding {} is declared twice",
                entry.binding
            )));
        }
        previous = Some(entry.binding);

        if !entry.flags.is_empty() {
            return Err(crate::MetalInstance::not_yet(
                "descriptor-indexing flags: this backend binds Metal's flat argument tables, \
                 which have no runtime-sized array (the Metal argument-buffer slice)",
            ));
        }
        let dynamic = matches!(
            entry.kind,
            BindingKind::UniformBuffer { dynamic: true }
                | BindingKind::StorageBuffer { dynamic: true, .. }
        );
        if dynamic && entry.count != 1 {
            return Err(HalError::InvalidDescriptor(format!(
                "binding {} is a dynamic-offset buffer with count {}; bind_group carries one \
                 offset per dynamic binding, so an array of them has no offset per element",
                entry.binding, entry.count
            )));
        }

        let table = Table::of(entry.kind);
        let used = &mut totals[table.slot()];
        let Some(next) = used.checked_add(entry.count) else {
            return Err(table_overflow(table, entry.binding, entry.count, *used));
        };
        if next > table.capacity() {
            return Err(table_overflow(table, entry.binding, entry.count, *used));
        }
        slots.push(Slot {
            binding: entry.binding,
            kind: entry.kind,
            table,
            first: *used,
            count: entry.count,
            visibility: entry.visibility,
            dynamic,
        });
        *used = next;
    }
    Ok(SetPlan { slots, totals })
}

/// The refusal for a layout that asks for more of one table than Metal has.
fn table_overflow(table: Table, binding: u32, count: u32, used: u32) -> HalError {
    HalError::InvalidDescriptor(format!(
        "binding {binding} asks for {count} more entries of Metal's {} argument table, which has \
         {} and already holds {used}",
        table.name(),
        table.capacity()
    ))
}

/// Where one set's tables start inside a pipeline layout's flattened space.
#[derive(Clone, Copy, Debug)]
pub(crate) struct SetPlacement {
    /// The layout this set was placed from. A bind group built against a
    /// *different* layout has its own within-set indices and cannot be added to
    /// this base; see [`DeviceInner::bind_group_raw`].
    pub(crate) layout: BindGroupLayoutHandle,
    /// The first index of each table this set's bindings are offset by.
    pub(crate) base: TableCounts,
}

/// Places every set of a pipeline layout, one after another.
///
/// This is the "earlier sets first" half of the flattening rule; [`plan_set`]
/// is the "lower binding numbers first" half.
///
/// Returns the placements **and** what the sets occupy in total, because the
/// running counts are what a push-constant block's index is: it lands one past
/// the last binding, so [`crate::argument::plan`] needs the buffer total this
/// walk ends on. Handing it back rather than letting the caller re-sum the
/// per-set totals keeps one arithmetic instead of two that can disagree.
///
/// # Errors
///
/// [`HalError::InvalidDescriptor`] when the sets together need more entries of
/// one table than Metal has — the same ceiling [`plan_set`] applies to one set,
/// applied to their sum, because the tables are per *stage* and every set
/// competes for them.
pub(crate) fn plan_layout(
    sets: &[(BindGroupLayoutHandle, TableCounts)],
) -> Result<(Vec<SetPlacement>, TableCounts), HalError> {
    let mut base: TableCounts = [0; TABLES];
    let mut out = Vec::with_capacity(sets.len());
    for (index, (layout, totals)) in sets.iter().enumerate() {
        out.push(SetPlacement {
            layout: *layout,
            base,
        });
        for table in Table::ALL {
            let slot = table.slot();
            let Some(next) = base[slot].checked_add(totals[slot]) else {
                return Err(layout_overflow(table, index, base[slot]));
            };
            if next > table.capacity() {
                return Err(layout_overflow(table, index, base[slot]));
            }
            base[slot] = next;
        }
    }
    Ok((out, base))
}

/// The refusal for a pipeline layout whose sets together overrun a table.
fn layout_overflow(table: Table, set: usize, used: u32) -> HalError {
    HalError::InvalidDescriptor(format!(
        "bind group layout {set} does not fit Metal's {} argument table, which has {} entries and \
         already holds {used} from the sets before it",
        table.name(),
        table.capacity()
    ))
}

/// A resource a bind group has already resolved and retained.
#[derive(Clone, Debug)]
pub(crate) enum BoundResource {
    /// A buffer and the byte offset it is bound at. `length` is the
    /// allocation's own size, kept so a dynamic offset can be re-checked
    /// against it at bind time.
    Buffer {
        raw: Retained<ProtocolObject<dyn MTLBuffer>>,
        offset: u64,
        length: u64,
    },
    /// A texture. Metal has no separate view object, so this is the view's own
    /// `MTLTexture`.
    Texture(Retained<ProtocolObject<dyn MTLTexture>>),
    /// A sampler state.
    Sampler(Retained<ProtocolObject<dyn MTLSamplerState>>),
}

/// One filled slot of a bind group.
#[derive(Clone, Debug)]
pub(crate) struct Bound {
    pub(crate) binding: u32,
    pub(crate) array_index: u32,
    pub(crate) table: Table,
    /// Index within the set: the slot's `first` plus `array_index`.
    pub(crate) local: u32,
    pub(crate) visibility: ShaderStages,
    pub(crate) dynamic: bool,
    /// Whether this binding's offset alignment is the uniform one. Kept as a
    /// flag rather than the whole [`BindingKind`] because it is the only thing
    /// a bind still needs the kind for — the kind itself was checked against
    /// the resource when the group was created.
    pub(crate) uniform: bool,
    pub(crate) resource: BoundResource,
}

/// A bind group: its layout, and the resources written into it.
#[derive(Debug)]
pub(crate) struct BindGroupRecord {
    pub(crate) owner: u64,
    pub(crate) layout: BindGroupLayoutHandle,
    /// Sorted by `(binding, array_index)`, so a replay is deterministic and a
    /// duplicate write is adjacent.
    pub(crate) bindings: Vec<Bound>,
}

/// A bind-group layout.
#[derive(Debug)]
pub(crate) struct BindGroupLayoutRecord {
    pub(crate) owner: u64,
    pub(crate) plan: SetPlan,
}

owned!(BindGroupLayoutRecord, BindGroupRecord);

/// One argument-table write, with its **absolute** index and any dynamic offset
/// already folded in.
///
/// Resolved under the device lock and applied without it, exactly as
/// [`BoundPipeline`](crate::pipeline::BoundPipeline) is and for the same
/// reason.
///
/// There is no `table` field: which table a write goes to is exactly which
/// [`BoundResource`] variant it carries, and a second copy of that fact is a
/// second thing to keep in step.
#[derive(Debug)]
pub(crate) struct BoundBinding {
    pub(crate) index: u32,
    pub(crate) visibility: ShaderStages,
    pub(crate) resource: BoundResource,
}

impl DeviceInner {
    /// Everything `bind_group` has to tell the render encoder, with the
    /// per-table indices already absolute.
    ///
    /// # Errors
    ///
    /// [`HalError::ForeignObject`] or [`HalError::InvalidHandle`] for a handle
    /// that is not this device's live one, and [`HalError::InvalidDescriptor`]
    /// for a slot the layout does not declare, a bind group built against a
    /// different layout than the pipeline layout names at that slot, a wrong
    /// number of dynamic offsets, or a dynamic offset that pushes a binding
    /// past the end of its buffer.
    pub(crate) fn bind_group_raw(
        &self,
        slot: u32,
        group: BindGroupHandle,
        dynamic_offsets: &[u32],
        layout: PipelineLayoutHandle,
        limits: &Limits,
    ) -> Result<Vec<BoundBinding>, HalError> {
        let state = self.state();
        let pipeline_layout = lookup(&state.pipeline_layouts, "pipeline layout", layout, self)?;
        let Some(placement) = pipeline_layout.sets.get(slot as usize).copied() else {
            return Err(HalError::InvalidDescriptor(format!(
                "bind_group names slot {slot}, and the pipeline layout declares {} bind group \
                 layout(s)",
                pipeline_layout.sets.len()
            )));
        };
        let record = lookup(&state.bind_groups, "bind group", group, self)?;
        if record.layout != placement.layout {
            // Not ceremony: a bind group's indices are positions inside *its*
            // layout, and adding them to a base computed from a different one
            // would place every resource somewhere plausible and wrong. Vulkan
            // permits a merely-compatible layout here; Metal has no layout
            // object to compare, so identity is the only check available.
            return Err(HalError::InvalidDescriptor(format!(
                "the bind group at slot {slot} was created from a different bind group layout \
                 than the pipeline layout names there, and its argument-table indices are \
                 positions inside its own layout"
            )));
        }
        let plan = &lookup(
            &state.bind_group_layouts,
            "bind group layout",
            record.layout,
            self,
        )?
        .plan;
        let dynamic: Vec<u32> = plan.dynamic_bindings().collect();
        if dynamic_offsets.len() != dynamic.len() {
            return Err(HalError::InvalidDescriptor(format!(
                "bind_group was given {} dynamic offset(s) for a layout with {} dynamic binding(s)",
                dynamic_offsets.len(),
                dynamic.len()
            )));
        }

        let mut out = Vec::with_capacity(record.bindings.len());
        for bound in &record.bindings {
            let mut resource = bound.resource.clone();
            if bound.dynamic
                && let BoundResource::Buffer { offset, length, .. } = &mut resource
            {
                let index = dynamic
                    .iter()
                    .position(|binding| *binding == bound.binding)
                    .unwrap_or_else(|| unreachable!("a dynamic slot is in the dynamic list"));
                let extra = u64::from(dynamic_offsets[index]);
                let alignment = buffer_offset_alignment(bound.uniform, limits);
                if !extra.is_multiple_of(alignment) {
                    return Err(HalError::InvalidDescriptor(format!(
                        "dynamic offset {extra} for binding {} is not a multiple of this device's \
                         {alignment}-byte buffer offset alignment",
                        bound.binding
                    )));
                }
                let size = *length;
                let moved = offset.checked_add(extra);
                if moved.is_none_or(|moved| moved >= size) {
                    return Err(HalError::InvalidDescriptor(format!(
                        "dynamic offset {extra} pushes binding {} to byte {} of a {size}-byte \
                         buffer",
                        bound.binding,
                        offset.saturating_add(extra)
                    )));
                }
                *offset = offset.saturating_add(extra);
            }
            out.push(BoundBinding {
                index: placement.base[bound.table.slot()] + bound.local,
                visibility: bound.visibility,
                resource,
            });
        }
        Ok(out)
    }
}

/// The offset alignment a buffer binding of this class must satisfy.
///
/// Read off this device's [`Limits`] rather than hard-coded: Metal's own
/// requirement varies with the GPU family, and the seam's rule is that a limit
/// is a ceiling — or here a floor — the backend guarantees. `crcbl_mtl::adapter`
/// leaves both at the seam's minimum, which is at or above every real Metal
/// requirement, so a caller that satisfies it satisfies the hardware too.
fn buffer_offset_alignment(uniform: bool, limits: &Limits) -> u64 {
    let alignment = if uniform {
        limits.min_uniform_buffer_offset_alignment
    } else {
        limits.min_storage_buffer_offset_alignment
    };
    alignment.max(1)
}

/// Sets every binding of a resolved group on the open **compute** encoder.
///
/// Metal gives the compute stage one set of three argument tables rather than
/// the render encoder's two sets, so this is `setBuffer:offset:atIndex:` and
/// its two siblings with no per-stage fan-out — and the *index* is the one
/// [`plan_layout`] already computed, because the flattening rule counts
/// declarations per table and knows nothing about which stage reads them. A
/// binding not visible to [`ShaderStages::COMPUTE`] sets nothing, mirroring
/// what [`apply`] does with a compute-only one.
pub(crate) fn apply_compute(
    bindings: &[BoundBinding],
    encoder: &ProtocolObject<dyn MTLComputeCommandEncoder>,
) {
    for binding in bindings {
        if !binding.visibility.contains(ShaderStages::COMPUTE) {
            continue;
        }
        let index = to_ns(u64::from(binding.index));
        match &binding.resource {
            // SAFETY: as `apply` — the index was bounded by `Table::capacity`
            // when the pipeline layout was planned, the offset was checked
            // against the buffer's own length when the group was created and
            // again against the dynamic offset in `bind_group_raw`, and every
            // resource is kept alive by the `Retained` the group holds.
            BoundResource::Buffer { raw, offset, .. } => unsafe {
                encoder.setBuffer_offset_atIndex(Some(raw), to_ns(*offset), index);
            },
            // SAFETY: as above, minus the offset.
            BoundResource::Texture(raw) => unsafe {
                encoder.setTexture_atIndex(Some(raw), index);
            },
            // SAFETY: as above, against the sampler table's capacity.
            BoundResource::Sampler(raw) => unsafe {
                encoder.setSamplerState_atIndex(Some(raw), index);
            },
        }
    }
}

/// Sets every binding of a resolved group on the open render encoder.
///
/// A binding visible only to [`ShaderStages::COMPUTE`] sets nothing here, and
/// that is correct rather than a gap: a render pass has no compute stage, and
/// the same declaration is what a compute pass would read.
pub(crate) fn apply(
    bindings: &[BoundBinding],
    encoder: &ProtocolObject<dyn MTLRenderCommandEncoder>,
) {
    for binding in bindings {
        let index = to_ns(u64::from(binding.index));
        let vertex = binding.visibility.contains(ShaderStages::VERTEX);
        let fragment = binding.visibility.contains(ShaderStages::FRAGMENT);
        match &binding.resource {
            BoundResource::Buffer { raw, offset, .. } => {
                let offset = to_ns(*offset);
                // SAFETY: `objc2` marks these unsafe because Metal
                // bounds-checks neither the argument-table index nor the
                // offset. The index was bounded by `Table::capacity` when the
                // pipeline layout was planned, the offset was checked against
                // this buffer's own length when the group was created and again
                // against the dynamic offset in `bind_group_raw`, and the
                // buffer is kept alive by the `Retained` the group holds.
                if vertex {
                    unsafe { encoder.setVertexBuffer_offset_atIndex(Some(raw), offset, index) };
                }
                if fragment {
                    unsafe { encoder.setFragmentBuffer_offset_atIndex(Some(raw), offset, index) };
                }
            }
            BoundResource::Texture(raw) => {
                // SAFETY: as above, minus the offset — the index was bounded by
                // the texture table's capacity at layout planning and the
                // texture is kept alive by the group.
                if vertex {
                    unsafe { encoder.setVertexTexture_atIndex(Some(raw), index) };
                }
                if fragment {
                    unsafe { encoder.setFragmentTexture_atIndex(Some(raw), index) };
                }
            }
            BoundResource::Sampler(raw) => {
                // SAFETY: as above, against the sampler table's capacity.
                if vertex {
                    unsafe { encoder.setVertexSamplerState_atIndex(Some(raw), index) };
                }
                if fragment {
                    unsafe { encoder.setFragmentSamplerState_atIndex(Some(raw), index) };
                }
            }
        }
    }
}

impl MetalDevice {
    /// Places a bind-group layout in the argument tables. See [`plan_set`].
    ///
    /// The seam's own rules come first, from
    /// [`BindGroupLayoutDesc::check_entries`] — including the mesh-stage
    /// visibility check, which is the one rule in this path that reads the
    /// *device* rather than the descriptor: this backend reports no
    /// `Features::MESH_SHADER`, so a layout naming the mesh stage is refused
    /// rather than becoming a set of argument-table slots no pipeline on this
    /// backend could ever read. [`plan_set`] then adds what only Metal refuses.
    ///
    /// This backend withdraws
    /// [`Features::DESCRIPTOR_INDEXING`](crcbl_hal::Features::DESCRIPTOR_INDEXING),
    /// so a layout carrying any [`BindingFlags`](crcbl_hal::BindingFlags) is
    /// refused by the seam's check before [`plan_set`]'s own refusal is
    /// reached. [`plan_set`] keeps it because it is a pure function with no
    /// caps to consult and its contract is tested without a device.
    pub(crate) fn create_bind_group_layout_impl(
        &self,
        desc: &BindGroupLayoutDesc<'_>,
    ) -> Result<BindGroupLayoutHandle, HalError> {
        desc.check_entries(&self.inner.caps, crcbl_hal::BackendKind::Metal)?;
        let plan = plan_set(desc)?;
        let handle = self
            .state()
            .bind_group_layouts
            .insert(BindGroupLayoutRecord {
                owner: self.inner.id,
                plan,
            });
        Ok(self.stamp(handle))
    }

    pub(crate) fn destroy_bind_group_layout_impl(&self, layout: BindGroupLayoutHandle) {
        let mut state = self.state();
        take_owned(&mut state.bind_group_layouts, layout, &*self.inner);
    }

    /// Resolves every entry to a retained Metal object. See the module docs for
    /// why the objects rather than the handles are kept.
    ///
    /// # Errors
    ///
    /// [`HalError::InvalidHandle`] for a layout no device issued,
    /// [`HalError::Unsupported`] for a
    /// [`BindGroupDesc::variable_count`](crcbl_hal::BindGroupDesc::variable_count)
    /// — the same answer [`plan_set`] gives the layout half of
    /// [`Capability::BindlessDescriptorArray`](crcbl_hal::Capability::BindlessDescriptorArray)
    /// — and [`HalError::InvalidDescriptor`] from [`resolve`] for an entry that
    /// does not fit the layout it names.
    pub(crate) fn create_bind_group_impl(
        &self,
        desc: &BindGroupDesc<'_>,
    ) -> Result<BindGroupHandle, HalError> {
        let limits = self.inner.caps.limits;
        let mut state = self.state();
        let plan = lookup(
            &state.bind_group_layouts,
            "bind group layout",
            desc.layout,
            &*self.inner,
        )?
        .plan
        .clone();
        if desc.variable_count.is_some() {
            // The field exists for a `VARIABLE_COUNT` binding, and `plan_set`
            // refuses every layout that declares one — so this is a caller
            // asking for a shape no layout on this backend can have, rather
            // than a value to ignore.
            //
            // **The same variant `plan_set` uses**, because it is the same
            // answer: `Capability::BindlessDescriptorArray` is what both halves
            // report, and the two ends of one capability refusing with two
            // different errors is what let a caller branch on `Unsupported` and
            // still be surprised here.
            return Err(crate::MetalInstance::unsupported(
                "BindGroupDesc::variable_count on a Metal bind group: this backend binds flat \
                 argument tables and refuses every VARIABLE_COUNT layout, so no group has a \
                 variable length to choose",
            ));
        }
        let mut bindings = Vec::with_capacity(desc.entries.len());
        for entry in desc.entries {
            bindings.push(resolve(&plan, entry, &state, &self.inner, &limits)?);
        }
        sort_bindings(&mut bindings)?;
        let handle = state.bind_groups.insert(BindGroupRecord {
            owner: self.inner.id,
            layout: desc.layout,
            bindings,
        });
        Ok(self.stamp(handle))
    }

    /// Rewrites some of a bind group's slots, leaving the rest alone.
    ///
    /// The seam's shape for streaming a bindless array, and still meaningful
    /// without one: this is how a frame's uniform buffer is repointed without
    /// rebuilding the group.
    pub(crate) fn update_bind_group_impl(
        &self,
        group: BindGroupHandle,
        entries: &[BindGroupEntry],
    ) -> Result<(), HalError> {
        let limits = self.inner.caps.limits;
        let mut state = self.state();
        let record = lookup(&state.bind_groups, "bind group", group, &*self.inner)?;
        let layout = record.layout;
        let plan = lookup(
            &state.bind_group_layouts,
            "bind group layout",
            layout,
            &*self.inner,
        )?
        .plan
        .clone();
        // Every entry is resolved before any is written, so a descriptor that
        // fails halfway leaves the group as it was rather than half-updated.
        let mut written = Vec::with_capacity(entries.len());
        for entry in entries {
            written.push(resolve(&plan, entry, &state, &self.inner, &limits)?);
        }
        let record = lookup_mut(&mut state.bind_groups, "bind group", group, &*self.inner)?;
        for bound in written {
            match record
                .bindings
                .iter_mut()
                .find(|old| old.binding == bound.binding && old.array_index == bound.array_index)
            {
                Some(old) => *old = bound,
                None => record.bindings.push(bound),
            }
        }
        sort_bindings(&mut record.bindings)?;
        Ok(())
    }

    pub(crate) fn destroy_bind_group_impl(&self, group: BindGroupHandle) {
        let mut state = self.state();
        take_owned(&mut state.bind_groups, group, &*self.inner);
    }
}

/// Puts a group's slots in replay order and rejects a slot written twice.
fn sort_bindings(bindings: &mut [Bound]) -> Result<(), HalError> {
    bindings.sort_by_key(|bound| (bound.binding, bound.array_index));
    for pair in bindings.windows(2) {
        if pair[0].binding == pair[1].binding && pair[0].array_index == pair[1].array_index {
            return Err(HalError::InvalidDescriptor(format!(
                "binding {} index {} is written twice by one bind group",
                pair[0].binding, pair[0].array_index
            )));
        }
    }
    Ok(())
}

/// Turns one [`BindGroupEntry`] into the slot it fills, or says why it does not
/// fill one.
fn resolve(
    plan: &SetPlan,
    entry: &BindGroupEntry,
    state: &DeviceState,
    owner: &DeviceInner,
    limits: &Limits,
) -> Result<Bound, HalError> {
    let Some(slot) = plan.slot(entry.binding) else {
        return Err(HalError::InvalidDescriptor(format!(
            "bind group entry names binding {}, which the layout does not declare",
            entry.binding
        )));
    };
    if entry.array_index >= slot.count {
        return Err(HalError::InvalidDescriptor(format!(
            "bind group entry writes index {} of binding {}, which holds {}",
            entry.array_index, entry.binding, slot.count
        )));
    }
    let uniform = matches!(slot.kind, BindingKind::UniformBuffer { .. });
    let resource = match (slot.kind, entry.resource) {
        (
            BindingKind::UniformBuffer { .. } | BindingKind::StorageBuffer { .. },
            BindingResource::Buffer {
                buffer,
                offset,
                size,
            },
        ) => {
            let (raw, length) = owner.buffer_raw_locked(state, buffer)?;
            let alignment = buffer_offset_alignment(uniform, limits);
            if !offset.is_multiple_of(alignment) {
                return Err(HalError::InvalidDescriptor(format!(
                    "binding {} is bound at byte {offset}, which is not a multiple of this \
                     device's {alignment}-byte buffer offset alignment",
                    entry.binding
                )));
            }
            // `size` has no Metal encoding — an argument-table buffer binding is
            // a base address and nothing more — so it is *checked* rather than
            // passed on: a caller that asked for a window running off the end
            // of the allocation has a descriptor bug either way, and Metal
            // would only find out when a shader read past it.
            let end = if size == BindingResource::WHOLE_BUFFER {
                Some(length)
            } else {
                offset.checked_add(size)
            };
            if offset >= length || end.is_none_or(|end| end > length) {
                return Err(HalError::InvalidDescriptor(format!(
                    "binding {} binds bytes {offset}..{} of a {length}-byte buffer",
                    entry.binding,
                    end.map_or_else(|| "overflow".to_string(), |end| end.to_string())
                )));
            }
            BoundResource::Buffer {
                raw,
                offset,
                length,
            }
        }
        (
            BindingKind::SampledImage { .. } | BindingKind::StorageImage { .. },
            BindingResource::ImageView(view),
        ) => BoundResource::Texture(owner.view_raw_locked(state, view)?),
        (BindingKind::Sampler { .. }, BindingResource::Sampler(sampler)) => {
            BoundResource::Sampler(owner.sampler_raw_locked(state, sampler)?)
        }
        (kind, resource) => {
            return Err(HalError::InvalidDescriptor(format!(
                "binding {} declares {kind:?} and was given {resource:?}",
                entry.binding
            )));
        }
    };
    Ok(Bound {
        binding: entry.binding,
        array_index: entry.array_index,
        table: slot.table,
        local: slot.first + entry.array_index,
        visibility: slot.visibility,
        dynamic: slot.dynamic,
        uniform,
        resource,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crcbl_core::Handle;
    use crcbl_hal::{
        BindingFlags, BufferDesc, BufferUsage, ClearValue, ColorAttachment, CommandEncoder,
        CommandEncoderDesc, Device, Extent3d, Format, ImageDesc, ImageType, ImageUsage,
        ImageViewDesc, ImageViewType, Instance, LoadOp, MemoryLocation, PipelineLayoutDesc,
        QueueKind, Rect2d, RenderPassDesc, SampleType, SamplerDesc, StoreOp,
    };

    use crate::MetalInstance;
    use crate::instance::tests::{desc as device_desc, open as open_instance};

    fn open_device() -> (MetalInstance, MetalDevice) {
        let instance = open_instance();
        let adapters = instance.adapters();
        assert!(!adapters.is_empty(), "a Mac has at least one adapter");
        let device = instance
            .open_device(&device_desc(adapters[0].id))
            .expect("a Metal device opens with no required features");
        (instance, device)
    }

    fn entry(binding: u32, kind: BindingKind, count: u32) -> BindGroupLayoutEntry {
        BindGroupLayoutEntry {
            binding,
            visibility: ShaderStages::GRAPHICS,
            kind,
            count,
            flags: BindingFlags::empty(),
        }
    }

    fn layout(entries: &[BindGroupLayoutEntry]) -> BindGroupLayoutDesc<'_> {
        BindGroupLayoutDesc {
            label: Some("crcbl-mtl test layout"),
            entries,
        }
    }

    const STORAGE: BindingKind = BindingKind::StorageBuffer {
        read_only: true,
        dynamic: false,
    };
    const UNIFORM: BindingKind = BindingKind::UniformBuffer { dynamic: false };
    const DYNAMIC_UNIFORM: BindingKind = BindingKind::UniformBuffer { dynamic: true };

    /// A layout the **seam** forbids is refused by
    /// `create_bind_group_layout`, because it runs
    /// `BindGroupLayoutDesc::check_entries`.
    ///
    /// [`plan_set`] is a pure function with no caps to consult, so it cannot
    /// state the rules that read the device — and the mesh-stage refusal is
    /// the one this backend has nothing else to make: it reports no
    /// `Features::MESH_SHADER`, and a layout naming the stage would otherwise
    /// become argument-table slots no pipeline here could ever read.
    ///
    /// **What turns it red.** Deleting the `check_entries` call from
    /// `create_bind_group_layout_impl`: the mesh case is then accepted, because
    /// nothing else in this backend looks at visibility at all.
    #[test]
    fn the_seams_own_rules_arrive_through_create_bind_group_layout() {
        let (_instance, device) = open_device();
        assert!(
            !device
                .inner
                .caps
                .features
                .contains(crcbl_hal::Features::MESH_SHADER),
            "this backend reports no mesh stage; the refusal below would prove nothing otherwise"
        );

        let mesh = [BindGroupLayoutEntry {
            visibility: ShaderStages::MESH,
            ..entry(0, STORAGE, 1)
        }];
        let error = device
            .create_bind_group_layout_impl(&layout(&mesh))
            .expect_err("a mesh-visible binding on a backend with no mesh stage");
        assert!(
            matches!(error, HalError::Unsupported { backend, .. }
                if backend == crcbl_hal::BackendKind::Metal),
            "{error:?}"
        );

        // The same binding visible to a stage this backend does report, so the
        // refusal is about the stage and not about the entry.
        device
            .create_bind_group_layout_impl(&layout(&[entry(0, STORAGE, 1)]))
            .expect("one read-only storage buffer, visible to the graphics stages");
    }

    /// The index of a binding in `plan`, by table and position.
    fn placed(plan: &SetPlan, binding: u32) -> (Table, u32) {
        let slot = plan
            .slots
            .iter()
            .find(|slot| slot.binding == binding)
            .unwrap_or_else(|| panic!("the plan has no binding {binding}"));
        (slot.table, slot.first)
    }

    /// **The flattening rule, stated as three separate claims.**
    ///
    /// Bindings are placed in ascending binding order whatever order the
    /// descriptor listed them in; each table counts independently; and an array
    /// binding consumes `count` consecutive entries.
    ///
    /// **What turns it red.** Placing in slice order rather than binding order
    /// — the entries below are deliberately out of order, so a plan that
    /// followed the slice would put binding 3 at index 0. Using one counter for
    /// all three tables — the texture and the sampler would not both be 0.
    /// Advancing by one per binding rather than by `count` — the last
    /// assertion, where a four-element texture array must push the next texture
    /// to index 4.
    #[test]
    fn a_set_is_flattened_in_binding_order_with_a_counter_per_table() {
        let entries = [
            entry(3, BindingKind::Sampler { comparison: false }, 1),
            entry(
                1,
                BindingKind::SampledImage {
                    view_type: ImageViewType::D2,
                    sample_type: SampleType::Float,
                },
                4,
            ),
            entry(0, UNIFORM, 1),
            entry(
                2,
                BindingKind::SampledImage {
                    view_type: ImageViewType::D2,
                    sample_type: SampleType::Float,
                },
                1,
            ),
            entry(4, STORAGE, 1),
        ];
        let plan = plan_set(&layout(&entries)).expect("a layout with no bindless flags");
        assert_eq!(plan.slots.len(), entries.len(), "a binding was dropped");
        assert_eq!(placed(&plan, 0), (Table::Buffer, 0));
        assert_eq!(placed(&plan, 4), (Table::Buffer, 1), "the second buffer");
        assert_eq!(
            placed(&plan, 1),
            (Table::Texture, 0),
            "textures count in their own table, so this is 0 and not 1"
        );
        assert_eq!(
            placed(&plan, 2),
            (Table::Texture, 4),
            "a four-element array occupies texture entries 0..4"
        );
        assert_eq!(placed(&plan, 3), (Table::Sampler, 0));
        assert_eq!(plan.totals, [2, 5, 1]);
    }

    /// **The engine's own `triangle.slang` needs its storage buffer at
    /// `[[buffer(0)]]`, and this is the check that the rule puts it there.**
    ///
    /// `msl/triangle.metal` declares
    /// `Vertex_natural_0 device* vertices [[buffer(0)]]` on *both* stages, and
    /// the `.slang` source declares it at `[[vk::binding(0, 0)]]`. So the one
    /// set, one binding case has exactly one right answer and this asserts it —
    /// including the visibility, because a layout that made the buffer
    /// vertex-only would leave the fragment stage's identical argument unbound.
    ///
    /// **What turns it red.** Any change to the flattening that stops a lone
    /// set-0 binding-0 buffer landing at index 0.
    #[test]
    fn the_engines_triangle_binding_lands_at_metal_buffer_zero() {
        let entries = [BindGroupLayoutEntry {
            binding: 0,
            visibility: ShaderStages::VERTEX | ShaderStages::FRAGMENT,
            kind: STORAGE,
            count: 1,
            flags: BindingFlags::empty(),
        }];
        let plan = plan_set(&layout(&entries)).expect("one read-only storage buffer");
        assert_eq!(placed(&plan, 0), (Table::Buffer, 0));
        let (placements, totals) =
            plan_layout(&[(unissued_layout(), plan.totals)]).expect("one set");
        assert_eq!(placements.len(), 1);
        assert_eq!(
            placements[0].base,
            [0, 0, 0],
            "the first set starts every table at zero"
        );
        assert_eq!(
            totals,
            [1, 0, 0],
            "the totals are what a push-constant block's index is taken from"
        );
    }

    /// A handle no device issued — the pure planners never resolve one.
    fn unissued_layout() -> BindGroupLayoutHandle {
        Handle::from_bits(1 << 32).expect("generation 1 is non-zero")
    }

    /// Later sets start where earlier ones stopped, per table.
    ///
    /// This is the shape `sprite.slang` has: set 0 holds two buffers, set 1
    /// holds a texture and a sampler, and `msl/sprite.metal` puts the sheet at
    /// `texture(0)` — which only comes out right if the texture counter is not
    /// advanced by set 0's buffers.
    ///
    /// **What turns it red.** Carrying one running index across all tables —
    /// the sheet would land at `texture(2)`. Restarting every set at zero — set
    /// 1's buffer base would be 0 and collide with set 0's.
    #[test]
    fn later_sets_start_where_the_earlier_ones_stopped() {
        let frame = plan_set(&layout(&[entry(0, UNIFORM, 1), entry(1, STORAGE, 1)]))
            .expect("two buffers in set 0");
        let sheet = plan_set(&layout(&[
            entry(
                0,
                BindingKind::SampledImage {
                    view_type: ImageViewType::D2,
                    sample_type: SampleType::Float,
                },
                1,
            ),
            entry(1, BindingKind::Sampler { comparison: false }, 1),
        ]))
        .expect("a texture and a sampler in set 1");
        let (placements, totals) = plan_layout(&[
            (unissued_layout(), frame.totals),
            (unissued_layout(), sheet.totals),
        ])
        .expect("two sets that fit");
        assert_eq!(placements.len(), 2);
        assert_eq!(placements[0].base, [0, 0, 0]);
        assert_eq!(
            placements[1].base,
            [2, 0, 0],
            "set 1's buffers start after set 0's two, and its texture still starts at zero"
        );
        assert_eq!(
            totals,
            [2, 1, 1],
            "the totals run across every set, which is what puts a push-constant block past all \
             of them"
        );
    }

    /// Every layout shape this backend cannot honour is refused, and the two
    /// causes are different errors.
    ///
    /// **What turns it red.** Accepting any [`BindingFlags`] bit — the loop,
    /// which walks all three individually rather than trusting one to stand for
    /// the others. Accepting `count: 0`, a duplicate binding number, or a
    /// dynamic-offset array — each has its own assertion, and each is a layout
    /// the seam already calls invalid.
    #[test]
    fn a_bindless_or_malformed_layout_is_refused_by_cause() {
        let flags = [
            BindingFlags::PARTIALLY_BOUND,
            BindingFlags::UPDATE_AFTER_BIND,
            BindingFlags::VARIABLE_COUNT,
        ];
        assert!(!flags.is_empty(), "nothing to check");
        for flag in flags {
            assert!(
                !flag.is_empty(),
                "{flag:?} is the empty set, so it proves nothing"
            );
            let entries = [BindGroupLayoutEntry {
                flags: flag,
                ..entry(
                    0,
                    BindingKind::SampledImage {
                        view_type: ImageViewType::D2,
                        sample_type: SampleType::Float,
                    },
                    8,
                )
            }];
            let error = plan_set(&layout(&entries))
                .expect_err("a flat argument table has no runtime-sized array");
            assert!(
                matches!(error, HalError::Unsupported { backend, .. }
                    if backend == crcbl_hal::BackendKind::Metal),
                "{flag:?}: {error:?}"
            );
        }

        for (what, entries) in [
            ("count 0", vec![entry(0, STORAGE, 0)]),
            (
                "a duplicate binding",
                vec![entry(1, STORAGE, 1), entry(1, UNIFORM, 1)],
            ),
            (
                "a dynamic array",
                vec![entry(0, DYNAMIC_UNIFORM, 1), entry(1, DYNAMIC_UNIFORM, 2)],
            ),
        ] {
            let error = plan_set(&layout(&entries)).expect_err(what);
            assert!(
                matches!(error, HalError::InvalidDescriptor(_)),
                "{what}: {error:?}"
            );
        }
        // The same dynamic binding at count 1 is fine, so the refusal above is
        // about the array and not about dynamic offsets.
        plan_set(&layout(&[entry(0, DYNAMIC_UNIFORM, 1)])).expect("one dynamic uniform buffer");
    }

    /// Metal's argument tables are finite, and both the per-set and the
    /// per-layout sum are bounded by them.
    ///
    /// **What turns it red.** Dropping either check — Metal raises on an
    /// out-of-range argument index, and a raise aborts the process, so the
    /// error has to happen here. Checking only the set and not the sum — the
    /// two-set half, where neither set alone overruns.
    #[test]
    fn the_argument_tables_bound_a_set_and_a_whole_layout() {
        let full = plan_set(&layout(&[entry(0, STORAGE, BUFFER_TABLE_ENTRIES)]))
            .expect("exactly the buffer table");
        assert_eq!(full.totals[Table::Buffer.slot()], BUFFER_TABLE_ENTRIES);
        let error = plan_set(&layout(&[entry(0, STORAGE, BUFFER_TABLE_ENTRIES + 1)]))
            .expect_err("one more than the table holds");
        assert!(matches!(error, HalError::InvalidDescriptor(_)), "{error:?}");

        // Two sets that each fit and together do not.
        let half = plan_set(&layout(&[entry(0, STORAGE, BUFFER_TABLE_ENTRIES - 1)]))
            .expect("one short of the table");
        plan_layout(&[(unissued_layout(), half.totals)]).expect("one set fits");
        let error = plan_layout(&[
            (unissued_layout(), half.totals),
            (unissued_layout(), half.totals),
        ])
        .expect_err("twice that does not");
        assert!(matches!(error, HalError::InvalidDescriptor(_)), "{error:?}");
    }

    /// A bind group's resource has to be the kind its binding declared, and an
    /// out-of-range write is refused rather than silently placed.
    ///
    /// **What turns it red.** Dropping the kind check — a buffer bound where
    /// the shader reads a texture is a Metal validation failure at draw time
    /// with nothing naming the binding. Dropping the array bound — the write
    /// would land on the *next* binding's entry, which is the failure the flat
    /// tables make possible and the seam's `array_index` makes checkable.
    #[test]
    #[ignore = "needs a real Metal device; run tests/run-mtl-e2e.sh"]
    fn a_bind_group_entry_must_match_the_kind_and_the_array_it_writes() {
        let (_instance, device) = open_device();
        let handle = device
            .create_bind_group_layout(&layout(&[
                entry(0, STORAGE, 1),
                entry(
                    1,
                    BindingKind::SampledImage {
                        view_type: ImageViewType::D2,
                        sample_type: SampleType::Float,
                    },
                    2,
                ),
                entry(2, BindingKind::Sampler { comparison: false }, 1),
            ]))
            .expect("a layout with one of each kind");
        let buffer = device
            .create_buffer(&BufferDesc {
                label: Some("crcbl-mtl bind group buffer"),
                size: 1024,
                usage: BufferUsage::STORAGE,
                memory: MemoryLocation::DeviceLocal,
            })
            .expect("a storage buffer");
        let sampler = device
            .create_sampler(&SamplerDesc::default())
            .expect("the seam's default sampler");

        // The happy path first, so every refusal below is about the thing it
        // names rather than about the layout being unusable.
        let group = device
            .create_bind_group(&BindGroupDesc {
                label: Some("crcbl-mtl bind group"),
                layout: handle,
                entries: &[
                    BindGroupEntry {
                        binding: 0,
                        array_index: 0,
                        resource: BindingResource::whole_buffer(buffer),
                    },
                    BindGroupEntry {
                        binding: 2,
                        array_index: 0,
                        resource: BindingResource::Sampler(sampler),
                    },
                ],
                variable_count: None,
            })
            .expect("a buffer and a sampler in their own bindings");
        device.destroy_bind_group(group);

        for (what, entry) in [
            (
                "a buffer where the layout declares a texture",
                BindGroupEntry {
                    binding: 1,
                    array_index: 0,
                    resource: BindingResource::whole_buffer(buffer),
                },
            ),
            (
                "a sampler where the layout declares a buffer",
                BindGroupEntry {
                    binding: 0,
                    array_index: 0,
                    resource: BindingResource::Sampler(sampler),
                },
            ),
            (
                "index 2 of a two-element array",
                BindGroupEntry {
                    binding: 1,
                    array_index: 2,
                    resource: BindingResource::ImageView(unissued_view()),
                },
            ),
            (
                "a binding the layout does not declare",
                BindGroupEntry {
                    binding: 9,
                    array_index: 0,
                    resource: BindingResource::whole_buffer(buffer),
                },
            ),
            (
                "bytes past the end of the buffer",
                BindGroupEntry {
                    binding: 0,
                    array_index: 0,
                    resource: BindingResource::Buffer {
                        buffer,
                        offset: 512,
                        size: 1024,
                    },
                },
            ),
            (
                "an unaligned offset",
                BindGroupEntry {
                    binding: 0,
                    array_index: 0,
                    resource: BindingResource::Buffer {
                        buffer,
                        offset: 4,
                        size: 16,
                    },
                },
            ),
        ] {
            let error = device
                .create_bind_group(&BindGroupDesc {
                    label: None,
                    layout: handle,
                    entries: &[entry],
                    variable_count: None,
                })
                .expect_err(what);
            assert!(
                matches!(error, HalError::InvalidDescriptor(_)),
                "{what}: {error:?}"
            );
        }

        device.destroy_sampler(sampler);
        device.destroy_buffer(buffer);
        device.destroy_bind_group_layout(handle);
    }

    /// A view handle nobody issued, for the entries above that must fail before
    /// the resource is ever resolved.
    fn unissued_view() -> crcbl_hal::ImageViewHandle {
        Handle::from_bits(1 << 32).expect("generation 1 is non-zero")
    }

    /// A group updated in place keeps the slots the update did not name.
    ///
    /// **What turns it red.** Replacing the whole binding list rather than the
    /// named slots — the untouched binding's offset would move or vanish.
    /// Appending instead of replacing — `sort_bindings` would find the slot
    /// written twice and fail the call.
    #[test]
    #[ignore = "needs a real Metal device; run tests/run-mtl-e2e.sh"]
    fn update_bind_group_rewrites_only_the_slots_it_names() {
        let (_instance, device) = open_device();
        let handle = device
            .create_bind_group_layout(&layout(&[entry(0, STORAGE, 1), entry(1, STORAGE, 1)]))
            .expect("two storage buffers");
        let buffer = device
            .create_buffer(&BufferDesc {
                label: Some("crcbl-mtl update"),
                size: 2048,
                usage: BufferUsage::STORAGE,
                memory: MemoryLocation::DeviceLocal,
            })
            .expect("a storage buffer");
        let at = |binding: u32, offset: u64| BindGroupEntry {
            binding,
            array_index: 0,
            resource: BindingResource::Buffer {
                buffer,
                offset,
                size: 256,
            },
        };
        let group = device
            .create_bind_group(&BindGroupDesc {
                label: None,
                layout: handle,
                entries: &[at(0, 0), at(1, 512)],
                variable_count: None,
            })
            .expect("both bindings filled");

        device
            .update_bind_group(group, &[at(0, 256)])
            .expect("binding 0 moves");
        let offsets = bound_offsets(&device, group);
        assert_eq!(
            offsets,
            vec![(0, 256), (1, 512)],
            "the update must move binding 0 and leave binding 1 where it was"
        );

        // An update naming a binding the layout does not declare fails, and the
        // group is unchanged — every entry is resolved before any is written.
        let error = device
            .update_bind_group(group, &[at(0, 1024), at(7, 0)])
            .expect_err("binding 7 is not in the layout");
        assert!(matches!(error, HalError::InvalidDescriptor(_)), "{error:?}");
        assert_eq!(
            bound_offsets(&device, group),
            offsets,
            "a failed update must not have applied its first entry"
        );

        device.destroy_bind_group(group);
        device.destroy_buffer(buffer);
        device.destroy_bind_group_layout(handle);
    }

    /// Every buffer binding of `group`, as `(binding, offset)` pairs.
    fn bound_offsets(device: &MetalDevice, group: BindGroupHandle) -> Vec<(u32, u64)> {
        let state = device.state();
        let record = lookup(&state.bind_groups, "bind group", group, &*device.inner)
            .expect("the group is live and this device's");
        record
            .bindings
            .iter()
            .filter_map(|bound| match &bound.resource {
                BoundResource::Buffer { offset, .. } => Some((bound.binding, *offset)),
                _ => None,
            })
            .collect()
    }

    /// Obligation 3 for the two tables this slice adds: a layout or a group
    /// from another device is *foreign*, not merely unresolvable.
    ///
    /// **What turns it red.** Dropping the handle tag — device B's own first
    /// layout occupies the slot A's handle names, so B would resolve A's handle
    /// to its own object, find the owner matching, and build a bind group
    /// against the wrong layout with no error anywhere.
    #[test]
    #[ignore = "needs a real Metal device; run tests/run-mtl-e2e.sh"]
    fn a_layout_or_group_from_another_device_is_foreign() {
        let instance = open_instance();
        let adapters = instance.adapters();
        assert!(!adapters.is_empty(), "nothing to check");
        let a = instance
            .open_device(&device_desc(adapters[0].id))
            .expect("device A");
        let b = instance
            .open_device(&device_desc(adapters[0].id))
            .expect("device B");

        let on_a = a
            .create_bind_group_layout(&layout(&[entry(0, STORAGE, 1)]))
            .expect("a layout on A");
        let on_b = b
            .create_bind_group_layout(&layout(&[entry(0, STORAGE, 1)]))
            .expect("a layout on B, occupying the slot A's handle would land in");
        assert_eq!(
            on_a.generation(),
            on_b.generation(),
            "both pools are fresh, so only the tag can tell these apart"
        );

        let error = b
            .create_bind_group(&BindGroupDesc {
                label: None,
                layout: on_a,
                entries: &[],
                variable_count: None,
            })
            .expect_err("A's layout is not B's to build against");
        assert!(
            matches!(error, HalError::ForeignObject { kind, .. } if kind == "bind group layout"),
            "{error:?}"
        );
        // And B's own layout still works, so the check is not refusing
        // everything.
        let group = b
            .create_bind_group(&BindGroupDesc {
                label: None,
                layout: on_b,
                entries: &[],
                variable_count: None,
            })
            .expect("B's own layout resolves");
        let error = a
            .update_bind_group(group, &[])
            .expect_err("B's group is not A's to update");
        assert!(
            matches!(error, HalError::ForeignObject { kind, .. } if kind == "bind group"),
            "{error:?}"
        );

        b.destroy_bind_group(group);
        b.destroy_bind_group_layout(on_b);
        a.destroy_bind_group_layout(on_a);
    }

    /// A pipeline layout carries the flattening, and the ceiling on how many
    /// sets it may name is the device's own.
    ///
    /// **What turns it red.** Not storing the placements — `bind_group` would
    /// have nothing to add its within-set indices to. Storing them without the
    /// running base — the second set's buffer base would be 0. Dropping the
    /// `max_bind_groups` check — the seam documents that limit as a hard
    /// ceiling.
    #[test]
    #[ignore = "needs a real Metal device; run tests/run-mtl-e2e.sh"]
    fn a_pipeline_layout_stores_where_each_of_its_sets_starts() {
        let (_instance, device) = open_device();
        let first = device
            .create_bind_group_layout(&layout(&[entry(0, UNIFORM, 1), entry(1, STORAGE, 1)]))
            .expect("two buffers");
        let second = device
            .create_bind_group_layout(&layout(&[
                entry(
                    0,
                    BindingKind::SampledImage {
                        view_type: ImageViewType::D2,
                        sample_type: SampleType::Float,
                    },
                    1,
                ),
                entry(1, BindingKind::Sampler { comparison: false }, 1),
            ]))
            .expect("a texture and a sampler");
        let pipeline_layout = device
            .create_pipeline_layout(&PipelineLayoutDesc {
                label: Some("crcbl-mtl two sets"),
                bind_group_layouts: &[first, second],
                push_constants: None,
            })
            .expect("two sets that fit the argument tables");
        {
            let state = device.state();
            let entry = lookup(
                &state.pipeline_layouts,
                "pipeline layout",
                pipeline_layout,
                &*device.inner,
            )
            .expect("the layout is live");
            assert_eq!(entry.sets.len(), 2);
            assert_eq!(entry.sets[0].base, [0, 0, 0]);
            assert_eq!(entry.sets[1].base, [2, 0, 0]);
            assert_eq!(entry.sets[1].layout, second);
        }

        let ceiling = device.caps().limits.max_bind_groups as usize;
        let too_many: Vec<BindGroupLayoutHandle> =
            core::iter::repeat_n(second, ceiling + 1).collect();
        let error = device
            .create_pipeline_layout(&PipelineLayoutDesc {
                label: None,
                bind_group_layouts: &too_many,
                push_constants: None,
            })
            .expect_err("one more set than the device admits");
        assert!(matches!(error, HalError::InvalidDescriptor(_)), "{error:?}");

        device.destroy_pipeline_layout(pipeline_layout);
        device.destroy_bind_group_layout(second);
        device.destroy_bind_group_layout(first);
    }

    /// A texture binding really does resolve an image view, which is the one
    /// resource kind whose Metal object comes from a different pool.
    ///
    /// **What turns it red.** Resolving a view through the image pool, or not
    /// resolving it at all — the group would hold the wrong texture or fail to
    /// build.
    #[test]
    #[ignore = "needs a real Metal device; run tests/run-mtl-e2e.sh"]
    fn a_texture_binding_resolves_the_views_own_texture() {
        let (_instance, device) = open_device();
        let image = device
            .create_image(&ImageDesc {
                label: Some("crcbl-mtl bound texture"),
                image_type: ImageType::D2,
                extent: Extent3d::d2(16, 16),
                format: Format::Rgba8Unorm,
                mip_levels: 1,
                samples: 1,
                usage: ImageUsage::SAMPLED,
            })
            .expect("a sampled image");
        let view = device
            .create_image_view(&ImageViewDesc {
                label: None,
                image,
                view_type: ImageViewType::D2,
                format: Format::Rgba8Unorm,
                range: crcbl_hal::ImageSubresourceRange::all(Format::Rgba8Unorm),
            })
            .expect("a whole-image view");
        let handle = device
            .create_bind_group_layout(&layout(&[entry(
                0,
                BindingKind::SampledImage {
                    view_type: ImageViewType::D2,
                    sample_type: SampleType::Float,
                },
                1,
            )]))
            .expect("one sampled image");
        let group = device
            .create_bind_group(&BindGroupDesc {
                label: None,
                layout: handle,
                entries: &[BindGroupEntry {
                    binding: 0,
                    array_index: 0,
                    resource: BindingResource::ImageView(view),
                }],
                variable_count: None,
            })
            .expect("a view in a texture binding");
        {
            let state = device.state();
            let record = lookup(&state.bind_groups, "bind group", group, &*device.inner)
                .expect("the group is live");
            assert_eq!(record.bindings.len(), 1, "the entry was dropped");
            let bound = &record.bindings[0];
            assert!(
                matches!(bound.resource, BoundResource::Texture(_)),
                "a view must resolve to a texture, got {:?}",
                bound.resource
            );
            assert_eq!((bound.table, bound.local), (Table::Texture, 0));
        }
        device.destroy_bind_group(group);
        device.destroy_bind_group_layout(handle);
        device.destroy_image_view(view);
        device.destroy_image(image);
    }

    // --- through the encoder ----------------------------------------------

    /// A small colour attachment, so a render pass can be opened at all.
    fn color_target(device: &MetalDevice) -> (crcbl_hal::ImageHandle, crcbl_hal::ImageViewHandle) {
        let image = device
            .create_image(&ImageDesc {
                label: Some("crcbl-mtl bind group target"),
                image_type: ImageType::D2,
                extent: Extent3d::d2(16, 16),
                format: Format::Rgba8Unorm,
                mip_levels: 1,
                samples: 1,
                usage: ImageUsage::COLOR_ATTACHMENT,
            })
            .expect("a colour attachment");
        let view = device
            .create_image_view(&ImageViewDesc {
                label: None,
                image,
                view_type: ImageViewType::D2,
                format: Format::Rgba8Unorm,
                range: crcbl_hal::ImageSubresourceRange::all(Format::Rgba8Unorm),
            })
            .expect("a whole-image view");
        (image, view)
    }

    /// Records `paint` inside a render pass and hands back what `finish` said.
    ///
    /// Every `bind_group` failure is a *recording* failure, and the seam gives
    /// a recording method nowhere to report — so `finish` is the only place the
    /// error can be observed, and this is that observation.
    fn record(
        device: &MetalDevice,
        paint: impl FnOnce(&mut dyn CommandEncoder),
    ) -> Result<(), HalError> {
        let (image, view) = color_target(device);
        let queue = device
            .queue(QueueKind::Graphics)
            .expect("the graphics queue exists");
        let mut encoder = device.create_command_encoder(&CommandEncoderDesc {
            label: Some("crcbl-mtl bind group"),
            queue,
        });
        encoder.begin_render_pass(&RenderPassDesc {
            label: Some("bind group"),
            color_attachments: &[ColorAttachment {
                view,
                resolve: None,
                load: LoadOp::Clear,
                store: StoreOp::Store,
                clear: ClearValue::color([0.0, 0.0, 0.0, 1.0]),
            }],
            depth_stencil_attachment: None,
            render_area: Rect2d::from_size(16, 16),
            timestamp_writes: None,
        });
        paint(encoder.as_mut());
        encoder.end_render_pass();
        let result = encoder.finish().map(|commands| {
            device.destroy_command_buffer(commands);
        });
        device.destroy_image_view(view);
        device.destroy_image(image);
        result
    }

    /// **The four ways a bind can be wrong that only the encoder can see**, all
    /// of them descriptor errors rather than silent mis-binds.
    ///
    /// The last is the one that matters most and the one no other backend has
    /// to check: a bind group's argument-table indices are positions inside
    /// *its own* layout, so adding them to a base computed from a different
    /// layout would place every resource somewhere plausible and wrong.
    ///
    /// **What turns it red.** Dropping the scope check — the first case
    /// succeeds and the following draw uses nothing. Dropping the slot bound —
    /// the second indexes past the placements and would panic or wrap.
    /// Dropping the dynamic-offset count check — the third binds with a stale
    /// offset. Dropping the layout-identity check — the fourth binds at the
    /// wrong index with no error anywhere.
    #[test]
    #[ignore = "needs a real Metal device; run tests/run-mtl-e2e.sh"]
    fn a_bind_is_refused_outside_a_pass_and_against_the_wrong_layout() {
        let (_instance, device) = open_device();
        let plain = device
            .create_bind_group_layout(&layout(&[entry(0, STORAGE, 1)]))
            .expect("one storage buffer");
        let dynamic = device
            .create_bind_group_layout(&layout(&[entry(0, DYNAMIC_UNIFORM, 1)]))
            .expect("one dynamic uniform buffer");
        let pipeline_layout = device
            .create_pipeline_layout(&PipelineLayoutDesc {
                label: None,
                bind_group_layouts: &[plain],
                push_constants: None,
            })
            .expect("one set");
        let buffer = device
            .create_buffer(&BufferDesc {
                label: Some("crcbl-mtl bound"),
                size: 1024,
                usage: BufferUsage::STORAGE | BufferUsage::UNIFORM,
                memory: MemoryLocation::DeviceLocal,
            })
            .expect("a buffer");
        let filled = |handle: BindGroupLayoutHandle| {
            device
                .create_bind_group(&BindGroupDesc {
                    label: None,
                    layout: handle,
                    entries: &[BindGroupEntry {
                        binding: 0,
                        array_index: 0,
                        resource: BindingResource::whole_buffer(buffer),
                    }],
                    variable_count: None,
                })
                .expect("binding 0 filled")
        };
        let group = filled(plain);
        let dynamic_group = filled(dynamic);

        // The happy path first, so every refusal below is about the thing it
        // names rather than about the setup being unusable.
        record(&device, |encoder| {
            encoder.bind_group(0, group, &[], pipeline_layout);
        })
        .expect("a group bound at the slot its own layout occupies");

        // Outside any pass: there is no encoder holding argument tables.
        let queue = device
            .queue(QueueKind::Graphics)
            .expect("the graphics queue exists");
        let mut encoder = device.create_command_encoder(&CommandEncoderDesc { label: None, queue });
        encoder.bind_group(0, group, &[], pipeline_layout);
        let error = encoder.finish().expect_err("no render encoder was open");
        assert!(matches!(error, HalError::InvalidDescriptor(_)), "{error:?}");

        for (what, slot, bound, offsets) in [
            (
                "a slot the pipeline layout does not declare",
                1,
                group,
                &[][..],
            ),
            (
                "an offset for a layout with no dynamic binding",
                0,
                group,
                &[0][..],
            ),
            (
                "a group built from a different bind group layout",
                0,
                dynamic_group,
                &[][..],
            ),
        ] {
            let error = record(&device, |encoder| {
                encoder.bind_group(slot, bound, offsets, pipeline_layout);
            })
            .expect_err(what);
            assert!(
                matches!(error, HalError::InvalidDescriptor(_)),
                "{what}: {error:?}"
            );
        }

        // And a hand-made pipeline layout handle is a stale handle rather than
        // a descriptor error, because nobody issued it.
        let error = record(&device, |encoder| {
            encoder.bind_group(
                0,
                group,
                &[],
                Handle::from_bits(1 << 32).expect("generation 1"),
            );
        })
        .expect_err("no device issued that pipeline layout");
        assert!(
            matches!(error, HalError::InvalidHandle { kind, .. } if kind == "pipeline layout"),
            "{error:?}"
        );

        device.destroy_bind_group(dynamic_group);
        device.destroy_bind_group(group);
        device.destroy_buffer(buffer);
        device.destroy_pipeline_layout(pipeline_layout);
        device.destroy_bind_group_layout(dynamic);
        device.destroy_bind_group_layout(plain);
    }

    /// A dynamic offset moves the binding, and one that runs off the end is
    /// refused rather than bound past the allocation.
    ///
    /// **What turns it red.** Ignoring the offset — the second case would
    /// succeed, since a binding that never moves can never leave the buffer.
    /// Dropping the alignment check — the third, whose offset is legal
    /// arithmetic and an illegal Metal buffer offset.
    #[test]
    #[ignore = "needs a real Metal device; run tests/run-mtl-e2e.sh"]
    fn a_dynamic_offset_is_aligned_and_bounded_by_its_buffer() {
        let (_instance, device) = open_device();
        let set = device
            .create_bind_group_layout(&layout(&[entry(0, DYNAMIC_UNIFORM, 1)]))
            .expect("one dynamic uniform buffer");
        let pipeline_layout = device
            .create_pipeline_layout(&PipelineLayoutDesc {
                label: None,
                bind_group_layouts: &[set],
                push_constants: None,
            })
            .expect("one set");
        let buffer = device
            .create_buffer(&BufferDesc {
                label: Some("crcbl-mtl dynamic"),
                size: 1024,
                usage: BufferUsage::UNIFORM,
                memory: MemoryLocation::DeviceLocal,
            })
            .expect("a uniform buffer");
        let group = device
            .create_bind_group(&BindGroupDesc {
                label: None,
                layout: set,
                entries: &[BindGroupEntry {
                    binding: 0,
                    array_index: 0,
                    resource: BindingResource::Buffer {
                        buffer,
                        offset: 0,
                        size: 256,
                    },
                }],
                variable_count: None,
            })
            .expect("the buffer at offset zero");

        let alignment = device.caps().limits.min_uniform_buffer_offset_alignment;
        assert!(alignment > 1, "an alignment of one cannot be violated");
        let inside = u32::try_from(alignment).expect("the seam's alignments are small");
        record(&device, |encoder| {
            encoder.bind_group(0, group, &[inside], pipeline_layout);
        })
        .expect("one aligned offset inside the buffer");

        for (what, offset) in [
            ("past the end of the buffer", 1024),
            ("an unaligned offset", inside + 1),
        ] {
            let error = record(&device, |encoder| {
                encoder.bind_group(0, group, &[offset], pipeline_layout);
            })
            .expect_err(what);
            assert!(
                matches!(error, HalError::InvalidDescriptor(_)),
                "{what}: {error:?}"
            );
        }

        device.destroy_bind_group(group);
        device.destroy_buffer(buffer);
        device.destroy_pipeline_layout(pipeline_layout);
        device.destroy_bind_group_layout(set);
    }
}
