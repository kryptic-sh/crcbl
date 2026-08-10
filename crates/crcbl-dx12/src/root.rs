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
//! # The mapping lives here because two sides have to agree on it
//!
//! `crate::pipeline` builds the `D3D12_ROOT_PARAMETER` array and
//! `crate::device` sets arguments against it by index. If the two derive the
//! order separately they will one day derive it differently, and the register
//! numbering the compute slice got wrong is the same class of bug. So [`place`]
//! answers **both** questions from one pass: [`RootLayout::slots`] is the
//! parameter array's own order, and [`RootLayout::sets`] is what a bind reads.
//! `crate::pipeline` iterates the first rather than rebuilding it, so the two
//! cannot drift.

use crcbl_hal::{HalError, Limits};

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
pub(crate) struct Slot {
    /// Index of the set in the pipeline layout.
    pub(crate) set: usize,
    pub(crate) kind: SlotKind,
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

/// Lays every set's root parameters out, in set order.
///
/// Within a set the order is the view table, then the sampler table, then one
/// root descriptor per dynamic binding ascending by binding number. Which order
/// is arbitrary; that there is exactly *one* of it is not — see the module docs.
///
/// # Errors
///
/// [`HalError::InvalidDescriptor`] when the signature would cost more than
/// [`MAX_ROOT_COST`] DWORDs, with the arithmetic in the message. This is the
/// refusal a caller gets at `create_pipeline_layout`, rather than at the draw
/// that would have bound nothing.
pub(crate) fn place(sets: &[SetShape]) -> Result<RootLayout, HalError> {
    let cost: u64 = sets.iter().copied().map(SetShape::cost).sum();
    if cost > u64::from(MAX_ROOT_COST) {
        let tables: u64 = sets
            .iter()
            .map(|set| u64::from(set.views) + u64::from(set.samplers))
            .sum();
        let roots: u64 = sets.iter().map(|set| u64::from(set.roots)).sum();
        return Err(HalError::InvalidDescriptor(format!(
            "this pipeline layout costs {cost} root DWORD(s) and a D3D12 root signature holds \
             {MAX_ROOT_COST}: {tables} descriptor table(s) at {TABLE_COST} each and {roots} \
             dynamic binding(s) at {ROOT_DESCRIPTOR_COST} each, because a dynamic offset becomes \
             a root CBV/SRV/UAV carrying a 64-bit address"
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
                *slot = Some(next(&mut slots, index, kind));
            }
        }
        for root in 0..shape.roots as usize {
            let parameter = next(&mut slots, index, SlotKind::Root(root));
            placement.roots.push(parameter);
        }
        placements.push(placement);
    }
    Ok(RootLayout {
        slots,
        sets: placements,
    })
}

/// Appends one root parameter and returns its index.
///
/// The index is the position in `slots` and comes from nowhere else, which is
/// what makes [`RootLayout`]'s two halves agree by construction. The cast is
/// sound because [`place`] checked the budget first: every parameter costs at
/// least one DWORD, so there are at most [`MAX_ROOT_COST`] of them.
fn next(slots: &mut Vec<Slot>, set: usize, kind: SlotKind) -> u32 {
    let parameter = u32::try_from(slots.len())
        .unwrap_or_else(|_| unreachable!("the root budget bounds the parameter count"));
    slots.push(Slot { set, kind });
    parameter
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
        let layout = place(&sets).expect("well inside the budget");
        assert_eq!(
            layout.slots.len(),
            2 + 2 + 1 + 1 + 1 + 3,
            "two tables and two roots, one table, one table and one root, three roots"
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
                    Some(&Slot { set: index, kind }),
                    "set {index}'s {kind:?} says root parameter {parameter}"
                );
                checked += 1;
            }
        }
        assert_eq!(
            checked,
            layout.slots.len(),
            "every root parameter must be named by exactly one placement"
        );
    }

    /// A set with nothing in it takes no root parameter, and the sets after it
    /// do not shift.
    #[test]
    fn an_empty_set_takes_no_root_parameter() {
        let layout =
            place(&[shape(false, false, 0), shape(true, false, 0)]).expect("one table in two sets");
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
        let exact = place(&[shape(false, false, 32)]).expect("32 root descriptors is exactly 64");
        assert_eq!(exact.slots.len(), 32);

        let error = place(&[shape(true, false, 32)])
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
        assert!(place(&many).is_err(), "66 tables is over the budget");
        assert!(
            place(&many[..32]).is_ok(),
            "64 tables is exactly the budget"
        );
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
