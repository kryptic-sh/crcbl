//! Root parameters: which register a binding takes, which root parameter index
//! reaches it, and what the whole signature costs.
//!
//! Not Windows-only, for the reason [`crate::draw`] and [`crate::present`] are
//! not: it holds no `windows` type, so off Windows it exists in the test build
//! alone and `cargo test` on any host checks the arithmetic that decides where
//! every binding of every pipeline layout lands. That matters more here than in
//! either of those, because **nothing in D3D12 reports a wrong answer**: a root
//! parameter index that disagrees between layout creation and bind time sets a
//! table or an address on the wrong parameter, and the shader reads a
//! plausible, wrong resource.
//!
//! # A dynamic offset is a root descriptor, and that is D3D12's own answer
//!
//! [`BindingKind::UniformBuffer`](crcbl_hal::BindingKind::UniformBuffer)'s
//! `dynamic` and its storage-buffer twin ask for an offset applied at
//! [`bind_group`](crcbl_hal::CommandEncoder::bind_group) time. A descriptor
//! table has none to apply — the table is addressed by a GPU descriptor handle
//! and every view inside it was written when the group was created. A **root
//! descriptor** is the other shape D3D12 offers: `SetGraphicsRootConstantBufferView`
//! and its SRV/UAV siblings take a raw *GPU virtual address* rather than a
//! handle, so the dynamic offset is one addition on the way to the call and
//! costs nothing at bind time.
//!
//! What it costs is **root signature space**, which is why this module exists at
//! all rather than being three lines in [`crate::binding`]. D3D12 gives a
//! signature [`MAX_ROOT_COST`] DWORDs; a descriptor table spends
//! [`TABLE_COST`] and a root descriptor [`ROOT_DESCRIPTOR_COST`], because the
//! address it carries is 64 bits. A layout with enough dynamic bindings does not
//! fit, and [`place`] refuses it **at pipeline-layout creation** with the
//! arithmetic in the message — the alternative being
//! `D3D12SerializeRootSignature` failing with a sentence about parameter counts,
//! or worse, a draw that binds nothing.
//!
//! Two shapes were rejected before this one:
//!
//! * **Root constants for the offset**, with the table kept. The shader would
//!   have to add the offset itself, which means editing every `.slang` source
//!   that uses a dynamic binding and diverging the HLSL from the SPIR-V. The
//!   seam's dynamic offset is a *binding* mechanism on the other three backends
//!   and would become a *shader* mechanism here alone.
//! * **One descriptor per distinct offset**, re-written into the heap at bind
//!   time. That is a descriptor write inside a recording encoder — a write to
//!   memory a submitted command list may already be reading — which is the
//!   hazard [`BindingFlags::UPDATE_AFTER_BIND`](crcbl_hal::BindingFlags::UPDATE_AFTER_BIND)
//!   exists to make the caller's, and it would be taken here without the caller
//!   asking.
//!
//! # A push constant is a root-constants parameter, and it is in the same budget
//!
//! [`PushConstantRange`] is D3D12's `D3D12_ROOT_PARAMETER_TYPE_32BIT_CONSTANTS`:
//! a `D3D12_ROOT_CONSTANTS` naming a shader register and a count of 32-bit
//! values that live **inside** the signature rather than being pointed at.
//! Nothing is allocated, nothing is bound, and
//! `SetGraphicsRoot32BitConstants`/`SetComputeRoot32BitConstants` write the
//! values straight into the command list.
//!
//! What that costs is one DWORD per `u32` — [`ROOT_CONSTANT_COST`] — which
//! makes it the one parameter whose cost is not fixed, and is the reason
//! [`MAX_PUSH_CONSTANT_BYTES`] is `4 ×` [`MAX_ROOT_COST`]: a range using the
//! whole budget leaves room for no table and no root descriptor at all. So the
//! range is placed by [`place`] alongside everything else and refused by the
//! same arithmetic, rather than being checked against a limit on its own —
//! [`plan_push_constants`] bounds it against
//! [`Limits::max_push_constant_size`], and the *combination* is what [`place`]
//! answers.
//!
//! **The register is taken after every binding's**, and that is a fact about the
//! committed artifacts rather than a choice here. HLSL has no push constants:
//! Slang emits the block as an ordinary `cbuffer` and `dxc` numbers it in the
//! `b` file with the rest, in declaration order — and `crcbl-shaders`'
//! `declaration_order` lint requires every source to declare its push constant
//! **last**, behind every numbered binding. So the block's register is the next
//! free `b` once a pipeline layout's sets have taken theirs, which for
//! `push_constant_probe.slang` — whose one binding is a UAV — is `b0`, the
//! register that artifact was measured at.
//!
//! # The block starts at word zero, whatever the range's offset is
//!
//! A `cbuffer`'s first member is at byte 0 of the constant buffer and
//! `D3D12_ROOT_CONSTANTS::Num32BitValues` counts from there, so the seam's byte
//! offsets index the parameter directly: byte `4n` of the block is root constant
//! word `n`, and a write at `offset` is `DestOffsetIn32BitValues = offset / 4`.
//! A range that starts above zero therefore declares the words in front of it
//! too — they are part of the same `cbuffer` — and pays for them, which is why
//! [`plan_push_constants`] sizes the parameter from the range's **end** rather
//! than from its size.
//!
//! # The mapping lives here because two sides have to agree on it
//!
//! `crate::pipeline` builds the `D3D12_ROOT_PARAMETER` array and
//! `crate::device` sets arguments against it by index. If the two derive the
//! order separately they will one day derive it differently, and the register
//! numbering the compute slice got wrong is the same class of bug. So [`place`]
//! answers **both** questions from one pass: [`RootLayout::slots`] is the
//! parameter array's own order, and [`RootLayout::sets`] and
//! [`RootLayout::push_constants`] are what a bind and a
//! [`push_constants`](crcbl_hal::CommandEncoder::push_constants) read.
//! `crate::pipeline` iterates the first rather than rebuilding it, so the two
//! cannot drift.

use crcbl_hal::{
    BackendKind, DeviceCaps, Features, HalError, Limits, PushConstantRange, ShaderStages,
};

use crate::dxil::{RegisterClass, Registers};

/// DWORDs a D3D12 root signature may cost.
///
/// D3D12's `D3D12_MAX_ROOT_COST`, spelled out rather than imported because this
/// module compiles off Windows. `crate::pipeline` asserts the two are equal in
/// the build that has both.
pub(crate) const MAX_ROOT_COST: u32 = 64;

/// DWORDs one descriptor table costs: a descriptor handle is an offset into the
/// bound heap.
const TABLE_COST: u64 = 1;

/// DWORDs one root descriptor costs: it carries a 64-bit GPU virtual address.
const ROOT_DESCRIPTOR_COST: u64 = 2;

/// DWORDs one 32-bit root constant costs: the value *is* the DWORD.
const ROOT_CONSTANT_COST: u64 = 1;

/// Bytes of push constants this backend reports as
/// [`Limits::max_push_constant_size`].
///
/// The whole root signature, spent on root constants and nothing else: a
/// `D3D12_ROOT_CONSTANTS` parameter costs [`ROOT_CONSTANT_COST`] DWORD per
/// 32-bit value, so [`MAX_ROOT_COST`] DWORDs are this many bytes. That makes it
/// a **ceiling and not a promise** — a layout asking for all of it and one
/// descriptor table besides does not fit, and [`place`] is what says so, with
/// the arithmetic in the message.
///
/// `crcbl_dx12::adapter` reports this figure and [`plan_push_constants`] checks
/// against the reported one, so the number a caller reads and the number this
/// module enforces are the same by construction.
pub(crate) const MAX_PUSH_CONSTANT_BYTES: u32 = MAX_ROOT_COST * 4;

/// Bytes per 32-bit root constant, which is what makes the seam's byte offsets
/// and D3D12's word counts convertible.
const BYTES_PER_WORD: u32 = 4;

/// One binding, reduced to what register assignment needs of it.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct Binding {
    /// The seam's binding number, which decides the order registers are taken
    /// in and is *not* the register.
    pub(crate) binding: u32,
    /// Which HLSL register file it takes from.
    pub(crate) class: RegisterClass,
    /// `NumDescriptors`: the declared count, or [`u32::MAX`] for an unbounded
    /// range.
    pub(crate) declared: u32,
}

/// What one set contributes to a root signature.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct SetShape {
    /// Whether the set declares a CBV/SRV/UAV descriptor table.
    pub(crate) views: bool,
    /// Whether the set declares a sampler descriptor table.
    pub(crate) samplers: bool,
    /// Dynamic-offset bindings, each becoming a root descriptor.
    pub(crate) roots: u32,
}

impl SetShape {
    /// What this set spends of the signature's budget.
    fn cost(self) -> u64 {
        u64::from(self.views) * TABLE_COST
            + u64::from(self.samplers) * TABLE_COST
            + u64::from(self.roots) * ROOT_DESCRIPTOR_COST
    }
}

/// The root-constants parameter one [`PushConstantRange`] becomes.
///
/// Built by [`plan_push_constants`] while the pipeline layout's register counter
/// is still in hand, because the `b` register this takes is the one after every
/// binding's — see the module docs.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct RootConstants {
    /// `D3D12_ROOT_CONSTANTS::ShaderRegister`, in space 0 like every other
    /// register this backend assigns.
    pub(crate) register: u32,
    /// `D3D12_ROOT_CONSTANTS::Num32BitValues`: the block from word zero to the
    /// end of the declared range, because a `cbuffer`'s first member is at byte
    /// zero whatever the range's offset is.
    pub(crate) words: u32,
    /// Stages the range named, which becomes the parameter's
    /// `ShaderVisibility`.
    pub(crate) stages: ShaderStages,
    /// The range as the seam declared it, carried through so
    /// `crate::pipeline` can hand [`Declared`] to the encoder without
    /// re-deriving it.
    pub(crate) range: PushConstantRange,
}

impl RootConstants {
    /// What this parameter spends of the signature's budget.
    fn cost(self) -> u64 {
        u64::from(self.words) * ROOT_CONSTANT_COST
    }
}

/// What one root parameter is: the set it belongs to, and the thing inside it.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum SlotKind {
    /// The set's CBV/SRV/UAV descriptor table.
    Views,
    /// The set's sampler descriptor table.
    Samplers,
    /// The set's `n`th dynamic binding, ascending by binding number.
    Root(usize),
}

/// One root parameter, named by where it came from.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Slot {
    /// A parameter one of the pipeline layout's sets contributed.
    Set {
        /// Index of the set in the pipeline layout.
        set: usize,
        /// The thing inside the set.
        kind: SlotKind,
    },
    /// The signature's single root-constants parameter, which belongs to no set.
    PushConstants,
}

/// Where one set's root parameters landed.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub(crate) struct SetPlacement {
    /// Root parameter index of the CBV/SRV/UAV table, if the set has one.
    pub(crate) views: Option<u32>,
    /// Root parameter index of the sampler table, if the set has one.
    pub(crate) samplers: Option<u32>,
    /// Root parameter index per dynamic binding, ascending by binding number —
    /// the order `bind_group`'s `dynamic_offsets` arrive in.
    pub(crate) roots: Vec<u32>,
}

/// A whole root signature's parameters, from both sides.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct RootLayout {
    /// One entry per root parameter, **in root parameter index order**. This is
    /// the order `crate::pipeline` builds the `D3D12_ROOT_PARAMETER` array in.
    pub(crate) slots: Vec<Slot>,
    /// One entry per set, in set order. This is what a bind reads.
    pub(crate) sets: Vec<SetPlacement>,
    /// Root parameter index of the root-constants entry, if the layout declares
    /// a push-constant range. This is what a `push_constants` call reads.
    pub(crate) push_constants: Option<u32>,
}

/// Assigns each binding its HLSL register.
///
/// **The register is not the binding number.** `[[vk::binding(binding, set)]]`
/// reaches SPIR-V and nothing else; Slang's HLSL output drops it and `dxc`
/// numbers each register class from zero in declaration order, in space 0. So a
/// binding's register is its position among the bindings of its own class, and
/// the count runs across a pipeline layout's sets rather than restarting at each
/// — which is why `registers` is threaded in rather than created here.
///
/// The order is ascending [`Binding::binding`], which is the same thing as
/// declaration order because `crcbl-shaders`' `declaration_order` lint requires
/// every source to declare its resources in ascending `(set, binding)`. Sorting
/// rather than trusting the caller's order means a layout that declared its
/// entries out of order still gets the registers the artifact has, rather than a
/// root signature that is wrong in a way only a Windows runner would report.
///
/// A **dynamic** binding is in this list beside the table ones: it becomes a
/// root descriptor rather than a table entry, but it is still a `ConstantBuffer`
/// or a `StructuredBuffer` in the source and still consumes a register in its
/// class. Leaving it out would shift every later binding of that class by one.
///
/// Returns one register per input, in the input's order.
pub(crate) fn assign_registers(bindings: &[Binding], registers: &mut Registers) -> Vec<u32> {
    let mut order: Vec<usize> = (0..bindings.len()).collect();
    order.sort_by_key(|index| bindings[*index].binding);
    let mut assigned = vec![0; bindings.len()];
    for index in order {
        let binding = bindings[index];
        assigned[index] = registers.take(binding.class, binding.declared);
    }
    assigned
}

/// Lays every set's root parameters out, in set order, and the root-constants
/// parameter after them.
///
/// Within a set the order is the view table, then the sampler table, then one
/// root descriptor per dynamic binding ascending by binding number. Which order
/// is arbitrary; that there is exactly *one* of it is not — see the module docs.
///
/// The push-constant parameter is **last**, which is not arbitrary: it means a
/// set's parameter indices are the same whether or not the layout declares a
/// range, so a signature growing one does not move every table a bind sets.
///
/// # Errors
///
/// [`HalError::InvalidDescriptor`] when the signature would cost more than
/// [`MAX_ROOT_COST`] DWORDs, with the arithmetic in the message. This is the
/// refusal a caller gets at `create_pipeline_layout`, rather than at the draw
/// that would have bound nothing.
pub(crate) fn place(
    sets: &[SetShape],
    push_constants: Option<RootConstants>,
) -> Result<RootLayout, HalError> {
    let constants = push_constants.map_or(0, RootConstants::cost);
    let cost: u64 = sets.iter().copied().map(SetShape::cost).sum::<u64>() + constants;
    if cost > u64::from(MAX_ROOT_COST) {
        let tables: u64 = sets
            .iter()
            .map(|set| u64::from(set.views) + u64::from(set.samplers))
            .sum();
        let roots: u64 = sets.iter().map(|set| u64::from(set.roots)).sum();
        let words = push_constants.map_or(0, |range| range.words);
        return Err(HalError::InvalidDescriptor(format!(
            "this pipeline layout costs {cost} root DWORD(s) and a D3D12 root signature holds \
             {MAX_ROOT_COST}: {tables} descriptor table(s) at {TABLE_COST} each, {roots} dynamic \
             binding(s) at {ROOT_DESCRIPTOR_COST} each, because a dynamic offset becomes a root \
             CBV/SRV/UAV carrying a 64-bit address, and {words} push-constant word(s) at \
             {ROOT_CONSTANT_COST} each, because a root constant is stored in the signature itself"
        )));
    }

    let mut slots = Vec::new();
    let mut placements = Vec::with_capacity(sets.len());
    for (index, shape) in sets.iter().enumerate() {
        let mut placement = SetPlacement::default();
        for (present, kind, slot) in [
            (shape.views, SlotKind::Views, &mut placement.views),
            (shape.samplers, SlotKind::Samplers, &mut placement.samplers),
        ] {
            if present {
                *slot = Some(next(&mut slots, Slot::Set { set: index, kind }));
            }
        }
        for root in 0..shape.roots as usize {
            let parameter = next(
                &mut slots,
                Slot::Set {
                    set: index,
                    kind: SlotKind::Root(root),
                },
            );
            placement.roots.push(parameter);
        }
        placements.push(placement);
    }
    let push_constants = push_constants.map(|_| next(&mut slots, Slot::PushConstants));
    Ok(RootLayout {
        slots,
        sets: placements,
        push_constants,
    })
}

/// Appends one root parameter and returns its index.
///
/// The index is the position in `slots` and comes from nowhere else, which is
/// what makes [`RootLayout`]'s halves agree by construction. The cast is sound
/// because [`place`] checked the budget first: every parameter costs at least
/// one DWORD, so there are at most [`MAX_ROOT_COST`] of them.
fn next(slots: &mut Vec<Slot>, slot: Slot) -> u32 {
    let parameter = u32::try_from(slots.len())
        .unwrap_or_else(|_| unreachable!("the root budget bounds the parameter count"));
    slots.push(slot);
    parameter
}

/// Turns the seam's push-constant range into the root-constants parameter it
/// becomes, taking its shader register from `registers`.
///
/// **Call this after every set has taken its registers**, because the `b`
/// register a push-constant block lands on is the one after them — see the
/// module docs on why that is a property of the committed artifacts rather than
/// a choice.
///
/// The word count is derived from the range's **end**, not its size: the
/// `cbuffer` `dxc` binds starts at byte zero, so a range at a non-zero offset
/// declares the words in front of it as well.
///
/// # Errors
///
/// [`HalError::Unsupported`] when the device reports no
/// [`Features::PUSH_CONSTANTS`](crcbl_hal::Features::PUSH_CONSTANTS), or when
/// the range names a shader stage the device does not have — the seam requires
/// both at layout creation rather than at the write.
///
/// [`HalError::InvalidDescriptor`] for a range naming no stage, one whose offset
/// or size is not a whole number of 32-bit values, and one ending past
/// [`Limits::max_push_constant_size`]. A range that *fits* the limit and does not
/// fit beside the layout's descriptor tables is [`place`]'s refusal rather than
/// this one, because that is a property of the whole signature.
pub(crate) fn plan_push_constants(
    range: Option<PushConstantRange>,
    registers: &mut Registers,
    caps: &DeviceCaps,
) -> Result<Option<RootConstants>, HalError> {
    let Some(range) = range else {
        return Ok(None);
    };
    if !caps.features.contains(Features::PUSH_CONSTANTS) {
        // Loudly at layout creation, never by dropping the writes later — the
        // obligation `crcbl_hal::pipeline` states on the range itself.
        return Err(HalError::Unsupported {
            backend: BackendKind::Dx12,
            what: "push constants on a device without PUSH_CONSTANTS",
        });
    }
    range
        .stages
        .check_supported(caps.features, BackendKind::Dx12)?;
    if range.stages.is_empty() {
        return Err(HalError::InvalidDescriptor(
            "a push-constant range must name at least one stage, because a D3D12 root parameter \
             carries a shader visibility and there is no value meaning none"
                .to_string(),
        ));
    }
    let end = range.offset.saturating_add(range.size);
    if end > caps.limits.max_push_constant_size {
        return Err(HalError::InvalidDescriptor(format!(
            "push constant range ends at {end} but max_push_constant_size is {}",
            caps.limits.max_push_constant_size
        )));
    }
    if range.size == 0
        || !range.size.is_multiple_of(BYTES_PER_WORD)
        || !range.offset.is_multiple_of(BYTES_PER_WORD)
    {
        return Err(HalError::InvalidDescriptor(format!(
            "push constant range {}..{end} must be a non-empty multiple of {BYTES_PER_WORD} bytes \
             at a {BYTES_PER_WORD}-byte offset, because D3D12 root constants are counted in \
             32-bit values",
            range.offset
        )));
    }
    Ok(Some(RootConstants {
        register: registers.take(RegisterClass::Cbv, 1),
        words: end / BYTES_PER_WORD,
        stages: range.stages,
        range,
    }))
}

/// One dynamic binding of a group being bound: what its layout says, and what
/// the group holds.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub(crate) struct Dynamic {
    /// The seam's binding number, for the message.
    pub(crate) binding: u32,
    /// Root parameter index this binding's address is set at.
    pub(crate) parameter: u32,
    /// Whether the uniform-buffer alignment applies rather than the storage
    /// one — that is, whether this is a root CBV.
    pub(crate) uniform: bool,
    /// The buffer's GPU virtual address, or `None` when nothing has been written
    /// into this binding. A root descriptor carries an address and nothing
    /// validates it, so an unwritten binding must refuse rather than set zero.
    pub(crate) address: Option<u64>,
    /// Byte offset the group's entry bound the range at.
    pub(crate) offset: u64,
    /// Bytes the group's entry bound.
    pub(crate) size: u64,
    /// The buffer's own size, which is what the dynamic offset is bounded by.
    pub(crate) capacity: u64,
}

/// Applies one set's dynamic offsets, returning the address each root descriptor
/// is set to.
///
/// # The alignment is D3D12's, read off the device's own limits
///
/// A root CBV's address must be a multiple of
/// `D3D12_CONSTANT_BUFFER_DATA_PLACEMENT_ALIGNMENT`, which is what
/// `crcbl_dx12::adapter` reports as
/// [`min_uniform_buffer_offset_alignment`](crcbl_hal::Limits::min_uniform_buffer_offset_alignment);
/// a root SRV/UAV's takes `D3D12_RAW_UAV_SRV_BYTE_ALIGNMENT`, reported as the
/// storage twin. The check is on the **whole** offset — the group entry's plus
/// the dynamic one — because it is their sum that reaches the call, and a
/// buffer's own address is already far more aligned than either.
///
/// # Errors
///
/// [`HalError::InvalidDescriptor`] for the wrong number of offsets, an offset
/// that does not satisfy the alignment above, an offset that pushes the bound
/// range past the end of its buffer, or a dynamic binding no entry has written a
/// buffer into.
pub(crate) fn apply(
    set: u32,
    dynamic: &[Dynamic],
    offsets: &[u32],
    limits: &Limits,
) -> Result<Vec<(u32, u64)>, HalError> {
    if offsets.len() != dynamic.len() {
        return Err(HalError::InvalidDescriptor(format!(
            "bind_group was given {} dynamic offset(s) at set {set}, and its layout declares {} \
             dynamic binding(s)",
            offsets.len(),
            dynamic.len()
        )));
    }
    let mut bound = Vec::with_capacity(dynamic.len());
    for (slot, offset) in dynamic.iter().zip(offsets) {
        let Some(address) = slot.address else {
            return Err(HalError::InvalidDescriptor(format!(
                "binding {} at set {set} takes a dynamic offset and is a root descriptor, and no \
                 entry has written a buffer into it — a root descriptor is a bare address, so \
                 there is no null view to read as zero",
                slot.binding
            )));
        };
        let alignment = alignment(slot.uniform, limits);
        let start = slot.offset.checked_add(u64::from(*offset));
        let Some(start) = start.filter(|start| start.is_multiple_of(alignment)) else {
            return Err(HalError::InvalidDescriptor(format!(
                "binding {} at set {set} is bound at byte {} of its buffer — the entry's offset \
                 {} plus a dynamic offset of {offset} — and a D3D12 root descriptor's address \
                 must be a multiple of {alignment}",
                slot.binding,
                slot.offset.saturating_add(u64::from(*offset)),
                slot.offset,
            )));
        };
        let end = start.checked_add(slot.size);
        if end.is_none_or(|end| end > slot.capacity) {
            return Err(HalError::InvalidDescriptor(format!(
                "a dynamic offset of {offset} puts binding {}'s {}-byte range at byte {start} of \
                 a {}-byte buffer",
                slot.binding, slot.size, slot.capacity
            )));
        }
        bound.push((slot.parameter, address.saturating_add(start)));
    }
    Ok(bound)
}

/// The offset alignment a root descriptor of this class must satisfy.
///
/// Read off the device's [`Limits`] rather than written here, so the one number
/// a caller can see is the one the check applies. `crcbl_dx12::adapter` fills
/// both from D3D12's own constants. The `max(1)` is what keeps a backend that
/// reported zero from dividing by it.
fn alignment(uniform: bool, limits: &Limits) -> u64 {
    let alignment = if uniform {
        limits.min_uniform_buffer_offset_alignment
    } else {
        limits.min_storage_buffer_offset_alignment
    };
    alignment.max(1)
}

/// The push-constant range a pipeline layout declared, as a `push_constants`
/// call has to be checked against.
///
/// Kept on `crate::pipeline`'s layout entry rather than re-derived, because the
/// root parameter index is the half nothing in D3D12 would report wrong: writing
/// constants at a descriptor table's index sets four bytes over a table pointer
/// and the draw reads whatever that address now is.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct Declared {
    /// Root parameter index of the root-constants entry.
    pub(crate) parameter: u32,
    /// First byte of the block the range covers.
    pub(crate) offset: u32,
    /// Bytes the range covers, from [`offset`](Self::offset).
    pub(crate) size: u32,
}

/// What one `push_constants` call sets, in `SetGraphicsRoot32BitConstants`
/// terms.
///
/// The words are **copied** out of the caller's bytes rather than pointed at:
/// `SetGraphicsRoot32BitConstants` takes a `*const c_void` it reads
/// `Num32BitValues` 32-bit values through, and the seam's `data` is a `&[u8]`
/// that need not be four-byte aligned. The array is stack-sized at the root
/// budget, which [`write`] bounds the count against, so no call allocates.
#[derive(Clone, Copy, Debug)]
pub(crate) struct Write {
    /// Root parameter index to set the constants at.
    pub(crate) parameter: u32,
    /// `DestOffsetIn32BitValues`: where in the block the first word lands.
    pub(crate) dest_word: u32,
    /// The words, of which [`count`](Self::count) are the write's.
    words: [u32; MAX_ROOT_COST as usize],
    /// How many of `words` the caller supplied.
    count: usize,
}

impl Write {
    /// The words to set, in the order they were written.
    pub(crate) fn words(&self) -> &[u32] {
        &self.words[..self.count]
    }
}

/// Turns one `push_constants` call into the root constants it sets.
///
/// The seam's `offset` is a byte offset **into the push-constant block**, which
/// is the same space the range's own offset is in, so the destination word is
/// `offset / 4` and needs no adjustment by the range's start — see the module
/// docs on why the block always begins at word zero.
///
/// # Errors
///
/// [`HalError::InvalidDescriptor`] for a layout that declares no range — the
/// seam is explicit that this must fail rather than be dropped — for an offset
/// or length that is not a whole number of 32-bit values, and for a write that
/// falls outside the declared range. The last is the one D3D12 would not report:
/// `SetGraphicsRoot32BitConstants` writing past the parameter's
/// `Num32BitValues` is a debug-layer message on a machine that has the layer on,
/// and nothing anywhere else.
pub(crate) fn write(
    declared: Option<Declared>,
    offset: u32,
    data: &[u8],
) -> Result<Write, HalError> {
    let Some(declared) = declared else {
        return Err(HalError::InvalidDescriptor(format!(
            "push_constants writes {} byte(s) at offset {offset} through a pipeline layout that \
             declares no push-constant range",
            data.len()
        )));
    };
    let length = u32::try_from(data.len()).unwrap_or(u32::MAX);
    if !offset.is_multiple_of(BYTES_PER_WORD) || !length.is_multiple_of(BYTES_PER_WORD) {
        return Err(HalError::InvalidDescriptor(format!(
            "push_constants writes {length} byte(s) at offset {offset}, and D3D12 root constants \
             are 32-bit values: both must be multiples of {BYTES_PER_WORD}"
        )));
    }
    let end = offset.saturating_add(length);
    let declared_end = declared.offset.saturating_add(declared.size);
    if offset < declared.offset || end > declared_end {
        return Err(HalError::InvalidDescriptor(format!(
            "push_constants writes {offset}..{end}, and the pipeline layout declares the range \
             {}..{declared_end}",
            declared.offset
        )));
    }
    let count = data.len() / BYTES_PER_WORD as usize;
    let mut words = [0u32; MAX_ROOT_COST as usize];
    let Some(slots) = words.get_mut(..count) else {
        // Unreachable through a layout this module planned — `place` bounds the
        // whole signature at `MAX_ROOT_COST` DWORDs and one word costs one — but
        // the array index is what would be out of bounds if it were not, so it is
        // a refusal rather than an assumption.
        return Err(HalError::InvalidDescriptor(format!(
            "push_constants writes {count} 32-bit value(s), and a D3D12 root signature holds \
             {MAX_ROOT_COST}"
        )));
    };
    for (word, chunk) in slots
        .iter_mut()
        .zip(data.chunks_exact(BYTES_PER_WORD as usize))
    {
        // Native, not little-endian: the runtime copies these words into the
        // constant buffer byte for byte, and the caller's bytes are already the
        // block's own layout. Reinterpreting them natively is what puts the same
        // bytes back; reading them as a little-endian *value* would byte-swap
        // them on a big-endian host.
        *word = u32::from_ne_bytes([chunk[0], chunk[1], chunk[2], chunk[3]]);
    }
    Ok(Write {
        parameter: declared.parameter,
        dest_word: offset / BYTES_PER_WORD,
        words,
        count,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn binding(binding: u32, class: RegisterClass) -> Binding {
        Binding {
            binding,
            class,
            declared: 1,
        }
    }

    fn shape(views: bool, samplers: bool, roots: u32) -> SetShape {
        SetShape {
            views,
            samplers,
            roots,
        }
    }

    /// D3D12's own limits, as `crcbl_dx12::adapter` reports them: 256 for a
    /// constant buffer view and 16 for a raw UAV/SRV.
    fn limits() -> Limits {
        Limits {
            min_uniform_buffer_offset_alignment: 256,
            min_storage_buffer_offset_alignment: 16,
            ..Limits::minimum()
        }
    }

    /// What `crcbl_dx12::adapter` reports of a D3D12 device, in the two fields
    /// [`plan_push_constants`] reads: root constants are core D3D12, and the
    /// budget is the whole root signature.
    fn caps() -> DeviceCaps {
        DeviceCaps {
            features: Features::PUSH_CONSTANTS,
            limits: Limits {
                max_push_constant_size: MAX_PUSH_CONSTANT_BYTES,
                ..limits()
            },
        }
    }

    /// A range of `size` bytes at offset zero, visible to the compute stage —
    /// the shape `push_constant_probe.slang` is driven with.
    fn range(size: u32) -> PushConstantRange {
        PushConstantRange {
            stages: ShaderStages::COMPUTE,
            offset: 0,
            size,
        }
    }

    /// The root-constants parameter a range plans to, or a panic naming the
    /// refusal.
    fn planned(range: PushConstantRange) -> RootConstants {
        planned_with(range, &mut Registers::default())
    }

    /// As [`planned`], against a register counter some bindings have already
    /// taken from.
    fn planned_with(range: PushConstantRange, registers: &mut Registers) -> RootConstants {
        plan_push_constants(Some(range), registers, &caps())
            .expect("a range this device can express")
            .expect("a range was asked for")
    }

    /// **A binding's register is its index among the bindings of its own class,
    /// and a dynamic binding is counted with the rest.**
    ///
    /// The dynamic binding is the assertion that matters. It becomes a root
    /// descriptor rather than a table entry, so it is easy to leave out of the
    /// numbering — and leaving it out shifts every later binding of its class by
    /// one, which is a root signature naming registers the shader does not read.
    /// `CreateGraphicsPipelineState` rejects that, on Windows, at the end of a
    /// CI round trip.
    #[test]
    fn a_dynamic_binding_still_takes_its_register_in_declaration_order() {
        let mut registers = Registers::default();
        // b0, t0, u0, b1 (dynamic), t1, s0 — in ascending binding order.
        let bindings = [
            binding(0, RegisterClass::Cbv),
            binding(1, RegisterClass::Srv),
            binding(2, RegisterClass::Uav),
            binding(3, RegisterClass::Cbv),
            binding(4, RegisterClass::Srv),
            binding(5, RegisterClass::Sampler),
        ];
        assert_eq!(
            assign_registers(&bindings, &mut registers),
            vec![0, 0, 0, 1, 1, 0],
            "each class counts from zero, and the dynamic b1 sits between b0 and the next CBV"
        );

        // The counter carries across sets, which is what makes a two-set
        // pipeline layout agree with a source `dxc` numbered end to end.
        let second = [
            binding(0, RegisterClass::Cbv),
            binding(1, RegisterClass::Srv),
        ];
        assert_eq!(assign_registers(&second, &mut registers), vec![2, 2]);
    }

    /// Registers are taken in ascending *binding* order however the entries were
    /// listed, so a caller that declared them out of order still gets the
    /// artifact's numbering.
    #[test]
    fn registers_follow_the_binding_number_and_not_the_slices_order() {
        let mut registers = Registers::default();
        let bindings = [
            binding(9, RegisterClass::Srv),
            binding(2, RegisterClass::Srv),
            binding(5, RegisterClass::Srv),
        ];
        assert_eq!(
            assign_registers(&bindings, &mut registers),
            vec![2, 0, 1],
            "binding 2 is t0, binding 5 is t1 and binding 9 is t2, whatever order they arrived in"
        );
    }

    /// An unbounded range consumes the rest of its class, so a binding declared
    /// after one lands somewhere legal rather than back on top of it.
    #[test]
    fn an_unbounded_range_consumes_the_rest_of_its_class() {
        let mut registers = Registers::default();
        let bindings = [
            binding(0, RegisterClass::Srv),
            Binding {
                declared: u32::MAX,
                ..binding(1, RegisterClass::Srv)
            },
        ];
        assert_eq!(assign_registers(&bindings, &mut registers), vec![0, 1]);
        assert_eq!(
            assign_registers(&[binding(2, RegisterClass::Srv)], &mut registers),
            vec![u32::MAX],
            "the saturating count is what keeps a later SRV off the unbounded range's registers"
        );
    }

    /// **The two halves of a [`RootLayout`] describe the same parameter array.**
    ///
    /// This is the check the whole module exists for: `crate::pipeline` builds
    /// the `D3D12_ROOT_PARAMETER` array from `slots` and `crate::device` sets
    /// arguments from `sets`, and nothing in D3D12 reports a disagreement — a
    /// table set on a root descriptor's index is a shader reading a plausible,
    /// wrong resource.
    #[test]
    fn every_placement_indexes_the_slot_it_describes() {
        let sets = [
            shape(true, true, 2),
            shape(true, false, 0),
            shape(false, true, 1),
            shape(false, false, 3),
        ];
        let layout = place(&sets, Some(planned(range(16)))).expect("well inside the budget");
        assert_eq!(
            layout.slots.len(),
            2 + 2 + 1 + 1 + 1 + 3 + 1,
            "two tables and two roots, one table, one table and one root, three roots, and the \
             root constants"
        );
        assert_eq!(layout.sets.len(), sets.len());

        let mut checked = 0;
        for (index, placement) in layout.sets.iter().enumerate() {
            for (parameter, kind) in placement
                .views
                .map(|parameter| (parameter, SlotKind::Views))
                .into_iter()
                .chain(
                    placement
                        .samplers
                        .map(|parameter| (parameter, SlotKind::Samplers)),
                )
                .chain(
                    placement
                        .roots
                        .iter()
                        .enumerate()
                        .map(|(root, parameter)| (*parameter, SlotKind::Root(root))),
                )
            {
                assert_eq!(
                    layout.slots.get(parameter as usize),
                    Some(&Slot::Set { set: index, kind }),
                    "set {index}'s {kind:?} says root parameter {parameter}"
                );
                checked += 1;
            }
        }
        let constants = layout.push_constants.expect("a range was placed") as usize;
        assert_eq!(
            layout.slots.get(constants),
            Some(&Slot::PushConstants),
            "the layout says the root constants are at parameter {constants}"
        );
        checked += 1;
        assert_eq!(
            checked,
            layout.slots.len(),
            "every root parameter must be named by exactly one placement"
        );
    }

    /// **A range is placed after every set, so adding one moves no table.**
    ///
    /// The property the ordering rule exists for: `bind_group` and
    /// `push_constants` are separate calls against one signature, and a layout
    /// that shifted its tables when a range appeared would bind them at indices
    /// the pipeline it was built for does not declare — which D3D12 reads as
    /// arithmetic and never reports.
    #[test]
    fn a_push_constant_range_is_placed_after_every_set() {
        let sets = [shape(true, true, 1), shape(true, false, 0)];
        let bare = place(&sets, None).expect("two sets");
        let with = place(&sets, Some(planned(range(8)))).expect("two sets and a range");
        assert_eq!(bare.push_constants, None);
        assert_eq!(
            with.sets, bare.sets,
            "a set's parameters must not move when a range is added"
        );
        assert_eq!(
            with.push_constants,
            Some(u32::try_from(bare.slots.len()).expect("four parameters")),
            "the root constants take the parameter after the last set's"
        );
        assert_eq!(with.slots.len(), bare.slots.len() + 1);
    }

    /// A set with nothing in it takes no root parameter, and the sets after it
    /// do not shift.
    #[test]
    fn an_empty_set_takes_no_root_parameter() {
        let layout = place(&[shape(false, false, 0), shape(true, false, 0)], None)
            .expect("one table in two sets");
        assert_eq!(layout.sets[0], SetPlacement::default());
        assert_eq!(layout.sets[1].views, Some(0));
        assert_eq!(layout.slots.len(), 1);
    }

    /// **A layout that does not fit the root signature is refused by name, at
    /// layout creation.**
    ///
    /// The boundary is the assertion: 32 root descriptors are exactly the 64
    /// DWORDs D3D12 gives, and one descriptor table beside them is one too many.
    #[test]
    fn a_layout_over_the_root_budget_is_refused_by_name() {
        let exact =
            place(&[shape(false, false, 32)], None).expect("32 root descriptors is exactly 64");
        assert_eq!(exact.slots.len(), 32);

        let error = place(&[shape(true, false, 32)], None)
            .expect_err("one more DWORD than a root signature holds");
        let HalError::InvalidDescriptor(text) = &error else {
            panic!("a signature that does not fit is not {error:?}");
        };
        assert!(text.contains("65 root DWORD(s)"), "{text}");
        assert!(text.contains("holds 64"), "{text}");
        assert!(text.contains("1 descriptor table(s)"), "{text}");
        assert!(text.contains("32 dynamic binding(s)"), "{text}");

        // Tables alone can overflow it too, which is the arm that has nothing to
        // do with this slice's own feature and still has to hold.
        let many = vec![shape(true, true, 0); 33];
        assert!(place(&many, None).is_err(), "66 tables is over the budget");
        assert!(
            place(&many[..32], None).is_ok(),
            "64 tables is exactly the budget"
        );
    }

    /// **A range that fits the limit on its own does not fit beside a table, and
    /// the refusal says so in DWORDs.**
    ///
    /// This is the arm [`MAX_PUSH_CONSTANT_BYTES`] makes necessary: the reported
    /// limit is the whole signature, so a caller asking for all of it has asked
    /// for something no layout with a bind group can have. The alternative to
    /// refusing here is `D3D12SerializeRootSignature` failing with a sentence
    /// about parameter counts — or a smaller reported limit, which would refuse
    /// a range D3D12 accepts.
    #[test]
    fn root_constants_are_spent_from_the_same_budget_as_the_tables() {
        let whole = planned(range(MAX_PUSH_CONSTANT_BYTES));
        assert_eq!(whole.words, MAX_ROOT_COST, "one DWORD per 32-bit value");
        let alone = place(&[], Some(whole)).expect("the whole signature, spent on constants");
        assert_eq!(alone.push_constants, Some(0));

        let error = place(&[shape(true, false, 0)], Some(whole))
            .expect_err("one descriptor table more than the signature holds");
        let HalError::InvalidDescriptor(text) = &error else {
            panic!("a signature that does not fit is not {error:?}");
        };
        assert!(text.contains("65 root DWORD(s)"), "{text}");
        assert!(text.contains("64 push-constant word(s)"), "{text}");
        assert!(text.contains("1 descriptor table(s)"), "{text}");

        // And the boundary from the other side: 63 words leave exactly the one
        // DWORD the table costs.
        place(
            &[shape(true, false, 0)],
            Some(planned(range(MAX_PUSH_CONSTANT_BYTES - 4))),
        )
        .expect("63 words and one table are exactly 64 DWORDs");
    }

    /// **The block's register is the one after every binding's, because that is
    /// where `dxc` puts it.**
    ///
    /// The assertion that matters is the second: a layout whose sets declare
    /// constant buffers pushes the block along, and a root signature naming `b0`
    /// there would collide with a binding the shader also reads at `b0`. The
    /// first is `push_constant_probe.slang`'s own measured answer — one UAV
    /// binding, so the block is `b0`.
    #[test]
    fn the_block_takes_the_b_register_after_every_bound_constant_buffer() {
        let mut registers = Registers::default();
        assert_eq!(
            assign_registers(&[binding(0, RegisterClass::Uav)], &mut registers),
            vec![0],
            "the probe's one binding is a UAV and takes no b register"
        );
        assert_eq!(
            planned_with(range(16), &mut registers).register,
            0,
            "cb0, which is where dxc -dumpbin found the probe's block"
        );

        let mut registers = Registers::default();
        assign_registers(
            &[
                binding(0, RegisterClass::Cbv),
                binding(1, RegisterClass::Cbv),
            ],
            &mut registers,
        );
        assert_eq!(
            planned_with(range(16), &mut registers).register,
            2,
            "two bound constant buffers take b0 and b1, so the block is b2"
        );
    }

    /// **A range at a non-zero offset declares the words in front of it.**
    ///
    /// The `cbuffer` starts at byte zero whatever the seam's range says, so the
    /// parameter has to be sized from the range's end — a parameter sized from
    /// `size` alone would be too short by the offset, and every write above it
    /// would land past `Num32BitValues`.
    #[test]
    fn the_parameter_is_sized_from_the_ranges_end_and_not_its_size() {
        assert_eq!(planned(range(16)).words, 4);
        assert_eq!(
            planned(PushConstantRange {
                offset: 16,
                ..range(16)
            })
            .words,
            8,
            "bytes 16..32 of a block are words 4..8 of a parameter that starts at word 0"
        );
    }

    /// Every range D3D12 has no root-constants parameter for is refused by name,
    /// at layout creation.
    #[test]
    fn a_range_d3d12_cannot_express_is_refused_by_name() {
        let cases = [
            ("multiple of 4", range(6)),
            ("multiple of 4", range(0)),
            (
                "multiple of 4",
                PushConstantRange {
                    offset: 2,
                    ..range(4)
                },
            ),
            ("max_push_constant_size", range(MAX_PUSH_CONSTANT_BYTES + 4)),
            ("at least one stage", {
                PushConstantRange {
                    stages: ShaderStages::empty(),
                    ..range(4)
                }
            }),
        ];
        assert!(!cases.is_empty(), "nothing to check");
        for (expected, range) in cases {
            let error = plan_push_constants(Some(range), &mut Registers::default(), &caps())
                .expect_err(expected);
            let HalError::InvalidDescriptor(text) = &error else {
                panic!("{expected}: {error:?}");
            };
            assert!(text.contains(expected), "{expected}: {text}");
        }

        // A device without the feature refuses at the same point and with the
        // *other* variant, because that is the one a caller branches on to reach
        // the seam's dynamic-offset substitute.
        let error = plan_push_constants(
            Some(range(16)),
            &mut Registers::default(),
            &DeviceCaps {
                features: Features::empty(),
                limits: limits(),
            },
        )
        .expect_err("a device reporting no PUSH_CONSTANTS");
        assert!(
            matches!(error, HalError::Unsupported { backend, .. } if backend == BackendKind::Dx12),
            "{error:?}"
        );

        // And no range at all is not a refusal, which is every pipeline layout
        // this backend built before this slice.
        assert_eq!(
            plan_push_constants(None, &mut Registers::default(), &caps()).expect("nothing to plan"),
            None
        );
    }

    /// **A `push_constants` call lands at the parameter, the word and the bytes
    /// the caller asked for.**
    ///
    /// The words are the assertion that matters: they reach D3D12 as raw memory,
    /// so a byte swap or a shifted destination is a shader reading plausible,
    /// wrong constants — which is exactly what the seam suite's per-word pattern
    /// exists to catch, and this is the half of it that runs off Windows.
    #[test]
    fn a_write_lands_at_the_parameter_and_word_the_range_puts_it_at() {
        let declared = Declared {
            parameter: 7,
            offset: 0,
            size: 16,
        };
        let first = write(Some(declared), 0, &0x1111_0001u32.to_ne_bytes())
            .expect("one word inside the range");
        assert_eq!(first.parameter, 7);
        assert_eq!(first.dest_word, 0);
        assert_eq!(first.words(), [0x1111_0001]);

        // A write into the middle of the range starts at its own word, and only
        // the words it supplied are set.
        let mut bytes = [0u8; 8];
        bytes[..4].copy_from_slice(&0xAAAA_0003u32.to_ne_bytes());
        bytes[4..].copy_from_slice(&0xBBBB_0004u32.to_ne_bytes());
        let middle = write(Some(declared), 8, &bytes).expect("the last two words");
        assert_eq!(middle.dest_word, 2, "byte 8 of the block is word 2");
        assert_eq!(middle.words(), [0xAAAA_0003, 0xBBBB_0004]);

        // A range starting above zero indexes the same space: the destination
        // word is the seam's offset, not the offset within the range.
        let above = write(
            Some(Declared {
                offset: 16,
                size: 16,
                ..declared
            }),
            16,
            &0x1234_5678u32.to_ne_bytes(),
        )
        .expect("the first word of a range at byte 16");
        assert_eq!(above.dest_word, 4);
    }

    /// A write the root signature has no room for is refused by name, rather
    /// than setting DWORDs the parameter does not declare.
    #[test]
    fn a_write_outside_the_declared_range_is_refused_by_name() {
        let declared = Declared {
            parameter: 0,
            offset: 8,
            size: 8,
        };
        let cases: Vec<(&str, Option<Declared>, u32, Vec<u8>)> = vec![
            ("declares no push-constant range", None, 0, vec![0; 4]),
            (
                "multiples of 4",
                Some(declared),
                8,
                // Six bytes is not a whole number of 32-bit values.
                vec![0; 6],
            ),
            ("multiples of 4", Some(declared), 10, vec![0; 4]),
            // Below the range, and past its end: both are writes to a word the
            // parameter does not declare.
            ("the range 8..16", Some(declared), 4, vec![0; 4]),
            ("the range 8..16", Some(declared), 12, vec![0; 8]),
        ];
        assert!(!cases.is_empty(), "nothing to check");
        for (expected, declared, offset, data) in cases {
            let error = write(declared, offset, &data).expect_err(expected);
            let HalError::InvalidDescriptor(text) = &error else {
                panic!("{expected}: {error:?}");
            };
            assert!(text.contains(expected), "{expected}: {text}");
        }

        // A write filling the range exactly is accepted, so the bounds check
        // above is on the ends rather than a blanket refusal.
        let whole = write(Some(declared), 8, &[0u8; 8]).expect("8..16 is exactly the range");
        assert_eq!(whole.words().len(), 2);

        // An empty write sets nothing and is not an error — but it is still
        // checked against the layout, which is what the first case above says.
        let empty = write(Some(declared), 8, &[]).expect("nothing to set");
        assert!(empty.words().is_empty());
    }

    /// A dynamic offset moves the address it is given, and the alignment the
    /// check applies is the one for that binding's class.
    #[test]
    fn a_dynamic_offset_moves_the_address_and_takes_its_own_alignment() {
        let uniform = Dynamic {
            binding: 3,
            parameter: 7,
            uniform: true,
            address: Some(0x1_0000),
            offset: 0,
            size: 16,
            capacity: 512,
        };
        let storage = Dynamic {
            binding: 4,
            parameter: 8,
            uniform: false,
            address: Some(0x2_0000),
            offset: 32,
            size: 16,
            capacity: 512,
        };
        assert_eq!(
            apply(0, &[uniform, storage], &[256, 16], &limits()).expect("both are aligned"),
            vec![(7, 0x1_0000 + 256), (8, 0x2_0000 + 48)],
            "the address is the buffer's plus the entry's offset plus the dynamic one"
        );

        // 16 satisfies the storage alignment and not the uniform one, which is
        // the whole point of reading the limit per class.
        let error =
            apply(0, &[uniform], &[16], &limits()).expect_err("16 is not a multiple of 256");
        let HalError::InvalidDescriptor(text) = &error else {
            panic!("{error:?}");
        };
        assert!(text.contains("multiple of 256"), "{text}");
        assert!(
            apply(0, &[storage], &[8], &limits()).is_err(),
            "8 is not a multiple of 16 either"
        );
    }

    /// The count, the bounds and an unwritten binding each refuse by name — and
    /// each is a different mistake, which is why each has its own message.
    #[test]
    fn an_offset_that_cannot_be_applied_is_refused_by_name() {
        let slot = Dynamic {
            binding: 3,
            parameter: 0,
            uniform: true,
            address: Some(0x1_0000),
            offset: 0,
            size: 16,
            capacity: 512,
        };
        let cases: Vec<(&str, Vec<Dynamic>, Vec<u32>)> = vec![
            ("declares 1 dynamic binding(s)", vec![slot], vec![]),
            ("declares 0 dynamic binding(s)", vec![], vec![0]),
            (
                "of a 512-byte buffer",
                vec![slot],
                // 512 + 16 is past the end, and 512 is a multiple of 256, so
                // this is the bounds check rather than the alignment one.
                vec![512],
            ),
            (
                "no entry has written a buffer into it",
                vec![Dynamic {
                    address: None,
                    ..slot
                }],
                vec![0],
            ),
        ];
        assert!(!cases.is_empty(), "nothing to check");
        for (expected, dynamic, offsets) in cases {
            let error = apply(1, &dynamic, &offsets, &limits()).expect_err(expected);
            let HalError::InvalidDescriptor(text) = &error else {
                panic!("{expected}: {error:?}");
            };
            assert!(text.contains(expected), "{expected}: {text}");
        }

        // And the offset that lands the range exactly at the end of the buffer
        // is accepted, so the bounds check above is `>` rather than `>=`.
        apply(
            1,
            &[Dynamic {
                capacity: 272,
                ..slot
            }],
            &[256],
            &limits(),
        )
        .expect("256 + 16 is exactly 272");
    }

    /// No dynamic bindings and no offsets is not an error, which is every set
    /// this backend bound before this slice.
    #[test]
    fn a_set_with_no_dynamic_bindings_takes_no_offsets() {
        assert_eq!(
            apply(0, &[], &[], &limits()).expect("nothing to apply"),
            vec![]
        );
    }
}
