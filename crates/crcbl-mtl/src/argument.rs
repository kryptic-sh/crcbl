//! How many entries Metal's three per-stage argument tables have, and where a
//! push-constant block lands in the buffer one.
//!
//! Plain Rust with no Objective-C in it, which is why — like [`crate::quirk`]
//! and [`crate::present`] — it is compiled off macOS under `cfg(test)` and
//! `cargo test` on any host runs its assertions. That matters more here than in
//! either of those, because **nothing in Metal reports a wrong answer**:
//! `setBytes:length:atIndex:` at the wrong index feeds the block to whatever
//! argument the shader declared there, and a shader reading a plausible, wrong
//! buffer draws a picture rather than raising.
//!
//! # The block sits one past the last binding, and that is the artifact's fact
//!
//! Metal has no push constants. Slang lowers a `[[vk::push_constant]]` block to
//! an ordinary buffer argument and numbers the arguments in **declaration
//! order**, and `crcbl-shaders`' `declaration_order` lint requires every source
//! to declare its push constant **last**, behind every numbered binding. So the
//! index is not a number chosen here: it is the one left over once
//! [`crate::binding`]'s flattening has given every set's buffers theirs.
//!
//! `msl/push_constant_probe.metal` is the measurement. Its one binding is a
//! storage buffer and it reads
//! `uint device* destination_1 [[buffer(0)]], ProbeConstants_0 constant*
//! constants_1 [[buffer(1)]]` — the block **behind** the bound buffer, at the
//! index [`plan`] computes for a layout with one buffer binding.
//! `crcbl_shaders::push_constant_probe`'s own tests assert both halves of that
//! line out of the committed artifact, so a Slang release that renumbered it
//! fails there rather than here.
//!
//! **`wgpu-hal` puts its own at the other end, and that is not a disagreement
//! about Metal.** Its `metal` module docs say "We put immediates first (if any)
//! in the table, followed by bind group 0 resources, followed by other bind
//! groups", and its `create_pipeline_layout` can choose that because it
//! *generates* the MSL: it hands naga
//! `back::msl::EntryPointResources::immediates_buffer` and the shader is
//! emitted at whichever slot it picked. crcbl compiles no shader at run time —
//! `crcbl-shaders` commits the MSL — so the index here is read off the artifact
//! instead of assigned to it, and the two backends end up at opposite ends of
//! the same table for the same reason.
//!
//! # The whole block is re-sent on every write
//!
//! `setBytes:length:atIndex:` **replaces** the argument. There is no
//! `DestOffsetIn32BitValues` the way `SetGraphicsRoot32BitConstants` has, and
//! no way to leave the bytes in front of a range alone — a call passing only
//! the caller's `data` would bind a block that starts at the caller's `offset`,
//! so every member behind it would be read from the wrong bytes.
//!
//! So one seam write is not one Metal call: the encoder keeps a [`Shadow`] of
//! the block, splices the write into it at `offset`, and sends from byte zero
//! every time. That is what makes a partial write mean what the seam says it
//! means, and it is why [`Block::bytes`] is the range's **end** rather than its
//! size.

use crcbl_hal::{
    BackendKind, DeviceCaps, Features, HalError, PipelineLayoutHandle, PushConstantRange,
    ShaderStages,
};

/// Entries in Metal's per-stage **buffer** argument table.
///
/// Fixed by the API rather than by any device: Apple's Metal Feature Set Tables
/// give the same number for every GPU family, and it is what
/// `setVertexBuffer:offset:atIndex:` bounds its index against. Exceeding it
/// raises rather than returning an error, which is why
/// [`plan_set`](crate::binding::plan_set), [`plan_layout`](crate::binding::plan_layout)
/// and [`plan`] check it while it is still a descriptor bug.
///
/// This one lives here rather than beside its texture and sampler siblings in
/// [`crate::binding`] because it is the ceiling a **push-constant** block runs
/// into: the block is one more buffer argument, so a layout whose sets fill the
/// table has nowhere to put it — and that is the arithmetic this module exists
/// to have checked on a host with no Metal.
pub(crate) const BUFFER_TABLE_ENTRIES: u32 = 31;

/// Bytes of push constants this backend reports as
/// [`Limits::max_push_constant_size`](crcbl_hal::Limits::max_push_constant_size).
///
/// **The call's own ceiling, not a device's, and there is no query for it.**
/// Apple's Metal Feature Set Tables carry the row "Maximum length of inlined
/// buffer contents using setBytes" at 4 KB in every GPU-family column, with the
/// footnote "Inlined buffer contents populate through functions like setBytes,
/// and its variants for specific render stages. Noninlined buffer contents that
/// you access through MTLBuffer or its GPU virtual address are limited only by
/// the size of that buffer." `wgpu-hal` reports the same figure for the same
/// call as `max_immediate_size: 0x1000`.
///
/// Unlike `crcbl-dx12`'s root-constant budget this is a **promise rather than a
/// ceiling shared with something else**: a block is an argument-table entry
/// whose bytes the driver copies, so a range using all of it costs a bind group
/// nothing but the one buffer slot [`plan`] takes for it.
pub(crate) const MAX_PUSH_CONSTANT_BYTES: u32 = 4096;

/// Bytes a push-constant offset and size must be a multiple of. See [`plan`].
const ALIGNMENT: u32 = 4;

/// The push-constant block one [`PushConstantRange`] becomes.
///
/// Built by [`plan`] while the pipeline layout's per-table totals are still in
/// hand, because the buffer index this takes is the one after every binding's —
/// see the module docs.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct Block {
    /// The **absolute** buffer-table index, one past every binding of every set.
    pub(crate) index: u32,
    /// Bytes from the argument's byte zero to the end of the declared range.
    ///
    /// The range's **end**, not its size: the shader's argument is a pointer to
    /// byte zero of the block, so a range starting above zero still has the
    /// bytes in front of it inside the same argument and they are still sent.
    pub(crate) bytes: u32,
    /// The stages the range declared, which decide which of an encoder's
    /// per-stage tables a write reaches.
    ///
    /// The **layout's** stages are what a write uses, never the caller's — see
    /// `crate::command`'s `push_constants`.
    pub(crate) stages: ShaderStages,
}

/// Turns the seam's push-constant range into the block it becomes, taking its
/// buffer index from the tables the layout's sets have already filled.
///
/// **Call this after [`plan_layout`](crate::binding::plan_layout)**, and pass
/// the buffer total it computed: the index a block lands on is the one after
/// every binding, which is a property of the committed artifacts rather than a
/// choice here. See the module docs.
///
/// # Errors
///
/// [`HalError::Unsupported`] when the device reports no [`Features::PUSH_CONSTANTS`],
/// or when the range names a shader stage the device does not have — the seam
/// requires both at layout creation rather than at the write.
///
/// [`HalError::InvalidDescriptor`] for a range naming no stage, an empty one,
/// one whose offset or size is not a multiple of [`ALIGNMENT`] bytes, one
/// ending past
/// [`Limits::max_push_constant_size`](crcbl_hal::Limits::max_push_constant_size),
/// and one whose block has no buffer-table entry left to occupy.
pub(crate) fn plan(
    range: Option<PushConstantRange>,
    buffers: u32,
    caps: &DeviceCaps,
) -> Result<Option<Block>, HalError> {
    let Some(range) = range else {
        return Ok(None);
    };
    if !caps.features.contains(Features::PUSH_CONSTANTS) {
        // Loudly at layout creation, never by dropping the writes later — the
        // obligation `crcbl_hal::pipeline` states on the range itself.
        return Err(HalError::Unsupported {
            backend: BackendKind::Metal,
            what: "push constants on a device without PUSH_CONSTANTS",
        });
    }
    range
        .stages
        .check_supported(caps.features, BackendKind::Metal)?;
    if range.stages.is_empty() {
        return Err(HalError::InvalidDescriptor(
            "a push-constant range must name at least one stage: Metal's argument tables are \
             per-stage and setBytes:length:atIndex: has a separate selector for each, so a range \
             naming none would be a block nothing ever sends"
                .to_string(),
        ));
    }
    let end = range.offset.saturating_add(range.size);
    if range.size == 0
        || !range.offset.is_multiple_of(ALIGNMENT)
        || !range.size.is_multiple_of(ALIGNMENT)
    {
        return Err(HalError::InvalidDescriptor(format!(
            "push constant range {}..{end} must be a non-empty multiple of {ALIGNMENT} bytes at a \
             {ALIGNMENT}-byte offset; Metal would take any length, and the seam's other backends \
             would not — VkPushConstantRange and D3D12's 32-bit root constants both count words",
            range.offset
        )));
    }
    if end > caps.limits.max_push_constant_size {
        return Err(HalError::InvalidDescriptor(format!(
            "push constant range ends at {end} but max_push_constant_size is {}",
            caps.limits.max_push_constant_size
        )));
    }
    if buffers >= BUFFER_TABLE_ENTRIES {
        // The block competes for the same table the bindings do, because Metal
        // lowers it to an ordinary buffer argument. A layout whose sets fill the
        // table has nowhere to put one, and saying so here is the difference
        // between a refusal and `setBytes:` raising inside a frame.
        return Err(HalError::InvalidDescriptor(format!(
            "a push-constant block needs the buffer argument-table entry after the last binding, \
             and this pipeline layout's sets already fill all {BUFFER_TABLE_ENTRIES} of them"
        )));
    }
    Ok(Some(Block {
        index: buffers,
        bytes: end,
        stages: range.stages,
    }))
}

/// A pipeline layout's push-constant block as it currently stands on one
/// encoder.
///
/// The state `setBytes:length:atIndex:` makes necessary: it replaces the whole
/// argument, so a seam write of part of the block has to be re-sent with the
/// rest of it. See the module docs.
///
/// The bytes are **zeroed** rather than left undefined, both at the start and
/// whenever the encoder moves to another layout's block. Vulkan leaves an
/// unwritten push constant undefined and this is strictly narrower than that, so
/// nothing a caller may rely on changes; what it buys is that the block a
/// dispatch reads is a function of the writes that reached it and of nothing
/// else, which is what makes a readback evidence.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct Shadow {
    layout: PipelineLayoutHandle,
    block: Block,
    bytes: Vec<u8>,
}

impl Shadow {
    /// A zeroed shadow of `layout`'s `block`.
    pub(crate) fn new(layout: PipelineLayoutHandle, block: Block) -> Self {
        Self {
            layout,
            block,
            bytes: vec![0; block.bytes as usize],
        }
    }

    /// Whether this shadow is the one `layout` writes through.
    ///
    /// Identity, not compatibility: the index and the length both come from the
    /// layout that was planned, so splicing a write for one layout into the
    /// bytes of another would send the right words at the wrong offsets.
    pub(crate) fn matches(&self, layout: PipelineLayoutHandle) -> bool {
        self.layout == layout
    }

    /// The whole block, from byte zero, as `setBytes:length:atIndex:` takes it.
    pub(crate) fn bytes(&self) -> &[u8] {
        &self.bytes
    }

    /// Splices one seam write into the block at `offset`.
    ///
    /// # Errors
    ///
    /// [`HalError::InvalidDescriptor`] when `offset` or `data` is not a whole
    /// number of [`ALIGNMENT`]-byte words, or when the write runs past the end
    /// of the range the pipeline layout declared.
    pub(crate) fn splice(&mut self, offset: u32, data: &[u8]) -> Result<(), HalError> {
        let length = u32::try_from(data.len()).unwrap_or(u32::MAX);
        if !offset.is_multiple_of(ALIGNMENT) || !length.is_multiple_of(ALIGNMENT) {
            return Err(HalError::InvalidDescriptor(format!(
                "push_constants writes {length} byte(s) at offset {offset}; both must be multiples \
                 of {ALIGNMENT}"
            )));
        }
        let end = offset.saturating_add(length);
        if end > self.block.bytes {
            return Err(HalError::InvalidDescriptor(format!(
                "push_constants writes {offset}..{end}, and the pipeline layout declares a block \
                 of {} byte(s)",
                self.block.bytes
            )));
        }
        self.bytes[offset as usize..end as usize].copy_from_slice(data);
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crcbl_core::Handle;
    use crcbl_hal::Limits;

    /// Caps a device with push constants would report, so the tests below turn
    /// on the range rather than on the device.
    fn caps() -> DeviceCaps {
        DeviceCaps {
            features: Features::PUSH_CONSTANTS | Features::COMPUTE,
            limits: Limits {
                max_push_constant_size: MAX_PUSH_CONSTANT_BYTES,
                ..Limits::minimum()
            },
        }
    }

    /// A range naming the compute stage, `offset..offset + size`.
    fn range(offset: u32, size: u32) -> PushConstantRange {
        PushConstantRange {
            stages: ShaderStages::COMPUTE,
            offset,
            size,
        }
    }

    /// A pipeline-layout handle no pool issued, which is all a [`Shadow`] needs:
    /// it compares handles and never looks one up.
    fn layout_handle(index: u32) -> PipelineLayoutHandle {
        Handle::from_bits((1 << 32) | u64::from(index)).expect("generation 1 is non-zero")
    }

    /// The whole reason this module exists: the block goes **behind** the
    /// bindings, at the index the committed MSL was measured at.
    ///
    /// `push_constant_probe.slang` declares one storage buffer and its push
    /// constant last, and `msl/push_constant_probe.metal` has `destination` at
    /// `buffer(0)` and the block at `buffer(1)`. So a layout whose sets took one
    /// buffer entry must plan index 1, and a layout with no bindings at all must
    /// plan index 0 — the second half being what stops this passing on a
    /// hard-coded 1.
    #[test]
    fn the_block_takes_the_buffer_index_after_every_binding() {
        let caps = caps();
        for (buffers, expected) in [(0, 0), (1, 1), (2, 2), (BUFFER_TABLE_ENTRIES - 1, 30)] {
            let block = plan(Some(range(0, 16)), buffers, &caps)
                .expect("a 16-byte compute range fits")
                .expect("a range was declared");
            assert_eq!(
                block.index, expected,
                "{buffers} buffer binding(s) put the block at {}, and the MSL numbers it after \
                 every one of them",
                block.index
            );
            assert_eq!(block.bytes, 16);
            assert_eq!(block.stages, ShaderStages::COMPUTE);
        }
    }

    /// No range is no block, and that is what keeps every layout the engine's
    /// own shaders use unchanged.
    #[test]
    fn a_layout_without_a_range_plans_nothing() {
        assert_eq!(
            plan(None, 3, &caps()).expect("no range is not an error"),
            None
        );
    }

    /// The block is sized from the range's **end**, because the shader's pointer
    /// is to byte zero whatever the offset is.
    ///
    /// The failure this catches is sizing it from `size`: a range at 16..32
    /// would send 16 bytes and the shader would read the caller's second word
    /// where its first member is.
    #[test]
    fn a_range_above_zero_still_declares_the_bytes_in_front_of_it() {
        let block = plan(Some(range(16, 16)), 0, &caps())
            .expect("a range at a non-zero offset is legal")
            .expect("a range was declared");
        assert_eq!(
            block.bytes, 32,
            "the block was sized from the range's size rather than its end"
        );
    }

    /// Every refusal `plan` makes, each with the accepted neighbour that stops
    /// it passing against a function that refused everything.
    #[test]
    fn plan_refuses_a_range_metal_could_not_serve() {
        let caps = caps();
        let limit = caps.limits.max_push_constant_size;

        // A device without the flag, which is the seam's own obligation and the
        // one refusal that is `Unsupported` rather than a descriptor bug.
        let mut lesser = caps;
        lesser.features = Features::COMPUTE;
        let error = plan(Some(range(0, 16)), 0, &lesser).expect_err("no PUSH_CONSTANTS");
        assert!(
            matches!(error, HalError::Unsupported { backend, .. } if backend == BackendKind::Metal),
            "{error:?}"
        );

        // A stage the device has not got, which the seam also pins to layout
        // creation.
        let error = plan(
            Some(PushConstantRange {
                stages: ShaderStages::MESH,
                offset: 0,
                size: 16,
            }),
            0,
            &caps,
        )
        .expect_err("this device reports no MESH_SHADER");
        assert!(matches!(error, HalError::Unsupported { .. }), "{error:?}");

        // No stage at all: nothing would ever be sent.
        let error = plan(
            Some(PushConstantRange {
                stages: ShaderStages::empty(),
                offset: 0,
                size: 16,
            }),
            0,
            &caps,
        )
        .expect_err("a range nothing reads");
        let HalError::InvalidDescriptor(text) = &error else {
            panic!("a stageless range is not {error:?}")
        };
        assert!(text.contains("at least one stage"), "{text}");

        // Empty, and unaligned in each of the two ways.
        for bad in [range(0, 0), range(0, 6), range(2, 16)] {
            let error = plan(Some(bad), 0, &caps).expect_err("not a non-empty whole word");
            let HalError::InvalidDescriptor(text) = &error else {
                panic!("{bad:?} is not {error:?}")
            };
            assert!(text.contains("multiple of"), "{text}");
        }

        // Exactly the reported budget is accepted, and four bytes more is not —
        // the pairing that makes the limit a number rather than a direction.
        plan(Some(range(0, limit)), 0, &caps).expect("max_push_constant_size must be acceptable");
        let error =
            plan(Some(range(0, limit + 4)), 0, &caps).expect_err("four bytes past the budget");
        let HalError::InvalidDescriptor(text) = &error else {
            panic!("a range past the limit is not {error:?}")
        };
        assert!(text.contains("max_push_constant_size"), "{text}");

        // And the table the block competes for: the last free entry takes one,
        // a full table does not.
        plan(Some(range(0, 16)), BUFFER_TABLE_ENTRIES - 1, &caps)
            .expect("the last buffer entry is the block's");
        let error = plan(Some(range(0, 16)), BUFFER_TABLE_ENTRIES, &caps)
            .expect_err("the sets filled the buffer table");
        let HalError::InvalidDescriptor(text) = &error else {
            panic!("a full argument table is not {error:?}")
        };
        assert!(text.contains("argument-table entry"), "{text}");
    }

    /// The shadow is what makes a partial write mean what the seam says: the
    /// words in front of it are still sent, at the offsets they were written at.
    ///
    /// Two writes that together cover the block, delivered out of order, must
    /// come back as one contiguous image — which is exactly what a backend that
    /// passed the caller's `data` straight to `setBytes:` would get wrong, since
    /// its second call would bind an eight-byte block starting at the caller's
    /// second word.
    #[test]
    fn two_partial_writes_land_at_their_own_offsets() {
        let handle = layout_handle(0);
        let block = Block {
            index: 1,
            bytes: 16,
            stages: ShaderStages::COMPUTE,
        };
        let mut shadow = Shadow::new(handle, block);
        assert_eq!(shadow.bytes(), &[0u8; 16], "a new shadow is zeroed");
        assert!(shadow.matches(handle));

        shadow
            .splice(8, &[9u8, 10, 11, 12, 13, 14, 15, 16])
            .expect("the back half");
        shadow.splice(0, &[1u8, 2, 3, 4]).expect("the first word");
        assert_eq!(
            shadow.bytes(),
            &[1u8, 2, 3, 4, 0, 0, 0, 0, 9, 10, 11, 12, 13, 14, 15, 16],
            "the block is not the two writes at the offsets they named"
        );

        // A whole-block write replaces every word of it, which is the shape the
        // seam's own exercise uses.
        shadow.splice(0, &[7u8; 16]).expect("the whole block");
        assert_eq!(shadow.bytes(), &[7u8; 16]);
    }

    /// A write the declared block cannot hold is refused rather than truncated,
    /// and an aligned one that fits is not.
    #[test]
    fn a_write_past_the_declared_block_is_refused() {
        let mut shadow = Shadow::new(
            layout_handle(0),
            Block {
                index: 0,
                bytes: 16,
                stages: ShaderStages::COMPUTE,
            },
        );
        shadow
            .splice(12, &[1u8, 2, 3, 4])
            .expect("the last word fits");
        shadow
            .splice(16, &[])
            .expect("an empty write at the end touches nothing");

        let error = shadow
            .splice(12, &[1u8, 2, 3, 4, 5, 6, 7, 8])
            .expect_err("four bytes past the block");
        let HalError::InvalidDescriptor(text) = &error else {
            panic!("an overrun is not {error:?}")
        };
        assert!(text.contains("12..20"), "{text}");

        for (offset, data) in [(2u32, &[1u8, 2, 3, 4][..]), (0, &[1, 2, 3][..])] {
            let error = shadow
                .splice(offset, data)
                .expect_err("neither offset nor length is a whole word");
            assert!(
                matches!(&error, HalError::InvalidDescriptor(text) if text.contains("multiples")),
                "{error:?}"
            );
        }
    }

    /// A shadow belongs to the layout it was made for, which is what stops one
    /// layout's write reaching another's block.
    #[test]
    fn a_shadow_belongs_to_one_pipeline_layout() {
        let block = Block {
            index: 0,
            bytes: 8,
            stages: ShaderStages::VERTEX | ShaderStages::FRAGMENT,
        };
        let mine = layout_handle(0);
        let shadow = Shadow::new(mine, block);
        assert!(shadow.matches(mine));
        assert!(
            !shadow.matches(layout_handle(1)),
            "a shadow matched a layout it was not planned from"
        );
    }
}
