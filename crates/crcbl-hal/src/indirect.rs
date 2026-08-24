//! Indirect draw arguments: the widths the APIs fix, and the rules for
//! stepping an array of them.
//!
//! [`draw_indirect`](crate::CommandEncoder::draw_indirect),
//! [`draw_indexed_indirect`](crate::CommandEncoder::draw_indexed_indirect) and
//! [`draw_mesh_tasks_indirect`](crate::CommandEncoder::draw_mesh_tasks_indirect)
//! each read an array of argument structures out of a buffer, and the seam
//! already states what that array has to look like: a four-byte-aligned
//! offset, a stride no smaller than one structure, and structures that fit
//! inside the buffer. None of those rules is any one API's — the structures are
//! the same words on Vulkan, D3D12 and Metal, which is what the constants below
//! record — so they live here, once, and a backend calls them rather than
//! keeping its own copy. That is the arrangement
//! [`ImageDesc::check`](crate::ImageDesc::check) already has for image
//! descriptors.
//!
//! # It reads no argument bytes and resolves no handle
//!
//! Everything here is arithmetic over a [`DrawIndirect`] — or a
//! [`DrawIndirectCount`], which adds the offset of the count word to the same
//! array — and the lengths of the buffers they name, so it is compiled and
//! unit-tested on every host, with no
//! device in the room. That matters because none of these mistakes is one the
//! APIs report usefully: an out-of-range indirect offset is undefined on
//! Vulkan, and on Metal it raises — which aborts the process rather than
//! returning something a caller could report.

use crate::{DrawIndirect, DrawIndirectCount, HalError};

/// Alignment an indirect draw's argument offset must have.
///
/// The argument structures are `uint32_t` fields throughout, and the APIs that
/// read them agree on the consequence: Vulkan requires `vkCmdDrawIndirect`'s
/// `offset` to be a multiple of 4, and Metal's headers document
/// `indirectBufferOffset` the same way. It is the rule
/// [`draw_mesh_tasks_indirect`](crate::CommandEncoder::draw_mesh_tasks_indirect)
/// already states for [`DrawIndirect::offset`].
pub const INDIRECT_OFFSET_ALIGNMENT: u64 = 4;

/// Bytes of one non-indexed draw's argument structure: four 32-bit fields.
///
/// Fixed by the APIs, and identical across them: Vulkan's
/// `VkDrawIndirectCommand` is `{ vertexCount, instanceCount, firstVertex,
/// firstInstance }` and Metal's `MTLDrawPrimitivesIndirectArguments` is the
/// same four words in the same order. The seam never writes the layout down —
/// what each backend requires of an argument buffer is its own API's — and the
/// two agreeing is what lets one compute pass feed either.
pub const DRAW_ARGS_BYTES: u64 = 16;

/// Bytes of one indexed draw's argument structure: five 32-bit fields, which
/// Vulkan calls `VkDrawIndexedIndirectCommand` and Metal calls
/// `MTLDrawIndexedPrimitivesIndirectArguments`. See [`DRAW_ARGS_BYTES`].
pub const DRAW_INDEXED_ARGS_BYTES: u64 = 20;

/// Bytes of one mesh draw's argument structure: three 32-bit fields, the x, y
/// and z workgroup counts.
///
/// Fixed by the APIs, and identical across them: Vulkan's
/// `VkDrawMeshTasksIndirectCommandEXT`, D3D12's `D3D12_DISPATCH_MESH_ARGUMENTS`
/// and Metal's `MTLDispatchThreadgroupsIndirectArguments` are the same three
/// words in the same order, which is why
/// [`draw_mesh_tasks_indirect`](crate::CommandEncoder::draw_mesh_tasks_indirect)
/// documents them.
pub const MESH_DISPATCH_ARGS_BYTES: u64 = 12;

/// Bytes of the draw count a count-limited draw reads out of GPU memory: one
/// `u32`.
///
/// Fixed by the APIs, and identical across them: Vulkan's
/// `vkCmdDrawIndirectCount` reads a `uint32_t` at `countBufferOffset` and
/// D3D12's `ExecuteIndirect` reads one at `CountBufferOffset`. It is what
/// bounds [`DrawIndirectCount::count_offset`] against the buffer
/// [`count_buffer`](DrawIndirectCount::count_buffer) names.
pub const DRAW_COUNT_BYTES: u64 = 4;

/// A validated indirect draw: where the first argument structure is, how far
/// apart the rest are, and how many there are.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct IndirectPlan {
    /// Byte offset of the first argument structure.
    pub first: u64,
    /// Bytes between consecutive argument structures. Never zero: a one-draw
    /// caller's `stride: 0` is reported as the structure's own width, because
    /// that is the step a backend issuing one draw would take.
    pub stride: u64,
    /// Argument structures the draw reads.
    pub count: u32,
}

impl IndirectPlan {
    /// The byte offset of argument structure `index`.
    pub const fn offset(self, index: u32) -> u64 {
        self.first + self.stride * index as u64
    }
}

/// [`plan_structures`] for a **mesh** draw, whose argument structure is
/// [`MESH_DISPATCH_ARGS_BYTES`] rather than a draw's.
///
/// Every other rule is identical — same four-byte offset alignment, same
/// stride rule, same bound — because they are all rules about *stepping an
/// array of structures in a buffer* rather than about what the structures say.
/// A separate entry point rather than a third `bool`, because the two callers
/// name different draw calls and a boolean at the call site says nothing.
///
/// # Errors
///
/// As [`plan_structures`].
pub fn plan_mesh_indirect(
    draw: &DrawIndirect,
    length: u64,
) -> Result<Option<IndirectPlan>, HalError> {
    plan_structures(draw, MESH_DISPATCH_ARGS_BYTES, length)
}

/// Where the first argument structure starts and how far apart the rest are —
/// [`plan_structures`] without the part that needs a buffer length.
///
/// [`plan_structures`] is this plus the bound, and calls it, so a backend that
/// can resolve the argument buffer's size wants that one instead. This is for
/// the backend that cannot: `crcbl-webgpu`'s command encoder holds a channel
/// and a handle pool, has no way to reach a buffer's length, and can still be
/// held to the offset and stride rules — which are the rules an API reports
/// worst. The browser checks the range itself, so the bound is the one part
/// that backend can afford to leave to it.
///
/// **A draw of nothing is accepted**, which is the answer [`plan_structures`]
/// gives it too (`Ok(None)`): `draw_count: 0` reads no structure, so there is
/// no offset to step from and no stride to step by. The two entry points have
/// to agree there, or one backend would refuse the draw another runs.
///
/// # Errors
///
/// [`HalError::InvalidDescriptor`] when [`DrawIndirect::offset`] is not a
/// multiple of [`INDIRECT_OFFSET_ALIGNMENT`], or when a multi-draw's
/// [`stride`](DrawIndirect::stride) is smaller than one argument structure or
/// is not itself so aligned.
pub fn check_layout(draw: &DrawIndirect, args: u64) -> Result<(), HalError> {
    if draw.draw_count == 0 {
        return Ok(());
    }
    if !draw.offset.is_multiple_of(INDIRECT_OFFSET_ALIGNMENT) {
        return Err(HalError::InvalidDescriptor(format!(
            "an indirect draw's argument offset {} is not a multiple of {INDIRECT_OFFSET_ALIGNMENT}",
            draw.offset
        )));
    }
    // A single draw reads one structure at `offset` and never strides, so its
    // `stride` is not a value the API is ever told — checking it would refuse
    // the tightly-packed `stride: 0` a one-draw caller may well pass.
    if draw.draw_count > 1 {
        let stride = u64::from(draw.stride);
        if stride < args || !stride.is_multiple_of(INDIRECT_OFFSET_ALIGNMENT) {
            return Err(HalError::InvalidDescriptor(format!(
                "an indirect draw of {} structures has a stride of {stride}, and one argument \
                 structure is {args} bytes on a {INDIRECT_OFFSET_ALIGNMENT}-byte alignment",
                draw.draw_count
            )));
        }
    }
    Ok(())
}

/// The shared body of every indirect *draw* plan: `args` bytes per structure,
/// and nothing that reads them.
///
/// `length` is the size of the buffer [`DrawIndirect::args`] names, which the
/// backend has already resolved; `None` is a draw of nothing, which is not an
/// error because there is no structure to read and no draw to issue.
///
/// The offset and stride rules are [`check_layout`]'s and are checked by
/// calling it; what this adds is the bound, which is the one rule that needs
/// `length`.
///
/// # Errors
///
/// [`HalError::InvalidDescriptor`] when [`DrawIndirect::offset`] is not a
/// multiple of [`INDIRECT_OFFSET_ALIGNMENT`], when a multi-draw's
/// [`stride`](DrawIndirect::stride) is smaller than one argument structure or
/// is not itself so aligned, or when the structures do not fit inside a
/// `length`-byte buffer.
pub fn plan_structures(
    draw: &DrawIndirect,
    args: u64,
    length: u64,
) -> Result<Option<IndirectPlan>, HalError> {
    if draw.draw_count == 0 {
        return Ok(None);
    }
    check_layout(draw, args)?;
    // A single draw reads one structure at `offset` and never strides, so the
    // step it takes is the structure's own width — which is why `check_layout`
    // leaves a one-draw `stride` unchecked and why the plan does not report it.
    let stride = if draw.draw_count == 1 {
        args
    } else {
        u64::from(draw.stride)
    };
    let span = u64::from(draw.draw_count - 1)
        .checked_mul(stride)
        .and_then(|span| span.checked_add(args))
        .and_then(|span| draw.offset.checked_add(span));
    if span.is_none_or(|span| span > length) {
        return Err(HalError::InvalidDescriptor(format!(
            "an indirect draw of {} structures {stride} bytes apart from offset {} runs past a \
             {length}-byte buffer",
            draw.draw_count, draw.offset
        )));
    }
    Ok(Some(IndirectPlan {
        first: draw.offset,
        stride,
        count: draw.draw_count,
    }))
}

/// [`plan_structures`] for a **count-limited** draw, whose draw count is read
/// from a second buffer rather than named by the caller.
///
/// `args_length` is the size of the buffer [`DrawIndirectCount::args`] names
/// and `count_length` the size of the one
/// [`count_buffer`](DrawIndirectCount::count_buffer) names, both already
/// resolved by the backend; `None` is a draw of nothing, which is
/// [`plan_structures`]' answer to a `draw_count` of zero for the same reason —
/// there is no structure to read and no count to fetch.
///
/// **The plan covers every structure the draw *could* reach**, which is
/// `max_draw_count` of them and not what this frame's count buffer happens to
/// hold: the count is a value nobody on this side can see. That makes the args
/// half of this exactly [`plan_structures`] over the same draw, and it is
/// written as that call rather than as a second copy of the stride and span
/// rules.
///
/// # Errors
///
/// [`HalError::InvalidDescriptor`] when
/// [`count_offset`](DrawIndirectCount::count_offset) is not a multiple of
/// [`INDIRECT_OFFSET_ALIGNMENT`], when the [`DRAW_COUNT_BYTES`] count word does
/// not fit inside the count buffer, or for any of [`plan_structures`]' reasons
/// about the argument array.
pub fn plan_count_structures(
    draw: &DrawIndirectCount,
    args: u64,
    args_length: u64,
    count_length: u64,
) -> Result<Option<IndirectPlan>, HalError> {
    if draw.max_draw_count == 0 {
        return Ok(None);
    }
    if !draw.count_offset.is_multiple_of(INDIRECT_OFFSET_ALIGNMENT) {
        return Err(HalError::InvalidDescriptor(format!(
            "a count-limited draw's count offset {} is not a multiple of \
             {INDIRECT_OFFSET_ALIGNMENT}",
            draw.count_offset
        )));
    }
    // The count is one `u32` and a backend reads it by offset, so a count word
    // hanging off the end of its buffer is a read of somebody else's memory
    // rather than an API-side refusal.
    if draw
        .count_offset
        .checked_add(DRAW_COUNT_BYTES)
        .is_none_or(|end| end > count_length)
    {
        return Err(HalError::InvalidDescriptor(format!(
            "a count-limited draw reads its u32 count at offset {} of a {count_length}-byte \
             buffer",
            draw.count_offset
        )));
    }
    plan_structures(
        &DrawIndirect {
            args: draw.args,
            offset: draw.args_offset,
            draw_count: draw.max_draw_count,
            stride: draw.stride,
        },
        args,
        args_length,
    )
}

/// Bytes of one argument structure for the indexed or non-indexed form.
pub const fn structure_bytes(indexed: bool) -> u64 {
    if indexed {
        DRAW_INDEXED_ARGS_BYTES
    } else {
        DRAW_ARGS_BYTES
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::BufferHandle;
    use crcbl_core::Handle;

    /// A handle no device issued — every function here is pure and never
    /// resolves it.
    fn buffer() -> BufferHandle {
        Handle::from_bits(1 << 32).expect("generation 1 is non-zero")
    }

    /// One indirect draw, as [`plan_structures`] is asked for it.
    fn indirect(offset: u64, draw_count: u32, stride: u32) -> DrawIndirect {
        DrawIndirect {
            args: buffer(),
            offset,
            draw_count,
            stride,
        }
    }

    /// One count-limited draw, as [`plan_count_structures`] is asked for it.
    fn counted(
        args_offset: u64,
        count_offset: u64,
        max_draw_count: u32,
        stride: u32,
    ) -> DrawIndirectCount {
        DrawIndirectCount {
            args: buffer(),
            args_offset,
            count_buffer: buffer(),
            count_offset,
            max_draw_count,
            stride,
        }
    }

    /// **The mesh argument structure is three words, and every offset a
    /// backend draws from is stepped by that width.**
    ///
    /// Table-driven, because everything `draw_mesh_tasks_indirect` does outside
    /// the encoder call is this arithmetic: a backend makes one launch per
    /// structure, and an offset past the buffer is the mistake the APIs report
    /// worst — on Metal it raises, which aborts the process rather than
    /// returning an error a caller could report.
    ///
    /// **What turns it red.** Giving `plan_mesh_indirect` a draw's
    /// [`DRAW_ARGS_BYTES`] instead of [`MESH_DISPATCH_ARGS_BYTES`]: the tight
    /// stride becomes 16, so the second offset is 16 rather than 12 and the
    /// four-structure bound stops fitting a 48-byte buffer. Dropping the
    /// alignment check, the stride check or the bound each turns one row of the
    /// refusal table green.
    #[test]
    fn a_mesh_indirect_draw_steps_three_words_per_structure() {
        assert_eq!(
            MESH_DISPATCH_ARGS_BYTES,
            3 * size_of::<u32>() as u64,
            "VkDrawMeshTasksIndirectCommandEXT, D3D12_DISPATCH_MESH_ARGUMENTS and \
             MTLDispatchThreadgroupsIndirectArguments are the same three words"
        );

        // A draw of nothing reads no structure and is not an error, which is
        // the answer a zero `draw_count` gets from every form.
        assert_eq!(
            plan_mesh_indirect(&indirect(0, 0, 12), 0).expect("a draw of nothing"),
            None
        );

        // `stride: 0` from a one-draw caller is legal and never told to the
        // API, so the plan reports the structure's own width.
        let one = plan_mesh_indirect(&indirect(4, 1, 0), 16)
            .expect("one structure at offset 4 of a 16-byte buffer")
            .expect("a draw of one");
        assert_eq!(one.first, 4);
        assert_eq!(one.stride, MESH_DISPATCH_ARGS_BYTES);
        assert_eq!(one.count, 1);
        assert_eq!(one.offset(0), 4);

        // Tightly packed, and every offset the loop will draw from.
        let tight = plan_mesh_indirect(&indirect(0, 4, 12), 48)
            .expect("four tight structures fill a 48-byte buffer exactly")
            .expect("a draw of four");
        let offsets: Vec<u64> = (0..tight.count).map(|index| tight.offset(index)).collect();
        assert_eq!(offsets, vec![0, 12, 24, 36]);

        // A padded stride is honoured rather than assumed away, which is what
        // `Capability::IndirectArgumentPaddedStride` means here.
        let padded = plan_mesh_indirect(&indirect(8, 3, 16), 8 + 16 * 2 + 12)
            .expect("three structures 16 bytes apart from offset 8")
            .expect("a draw of three");
        let offsets: Vec<u64> = (0..padded.count)
            .map(|index| padded.offset(index))
            .collect();
        assert_eq!(offsets, vec![8, 24, 40]);

        for (draw, length, what) in [
            (
                indirect(2, 1, 0),
                64,
                "an offset that is not a multiple of four",
            ),
            (indirect(0, 2, 8), 64, "a stride below one structure"),
            (
                indirect(0, 2, 14),
                64,
                "a stride that is not a multiple of four",
            ),
            (indirect(0, 4, 12), 47, "four tight structures in 47 bytes"),
            (
                indirect(40, 1, 0),
                48,
                "one structure that starts 8 bytes from the end",
            ),
        ] {
            let Err(error) = plan_mesh_indirect(&draw, length) else {
                panic!("{what} must be refused");
            };
            assert!(
                matches!(error, HalError::InvalidDescriptor(_)),
                "{what}: {error:?}"
            );
        }
    }
    /// **The offset and stride rules hold without a buffer length, and the
    /// bound is not smuggled in with them.**
    ///
    /// [`check_layout`] is what `crcbl-webgpu` calls: its encoder cannot reach
    /// a buffer's length, so a check that needed one would be no check at all
    /// there. Both halves matter — the rules it does enforce have to bite, and
    /// the rule it does not must be visibly absent, or a backend calling this
    /// would be credited with a bounds check nothing performs.
    ///
    /// **What turns it red.** Dropping the alignment check or either half of
    /// the stride check turns one of the refusals green; checking a one-draw
    /// `stride` refuses the `stride: 0` row; folding [`plan_structures`]'s
    /// bound in here refuses the last row, which this function has no `length`
    /// to judge.
    #[test]
    fn check_layout_judges_the_offset_and_the_stride_and_not_the_bound() {
        // A draw of nothing reads no structure, which is `plan_structures`'
        // `Ok(None)` and has to be this function's `Ok(())`.
        check_layout(&indirect(2, 0, 3), DRAW_ARGS_BYTES).expect("a draw of nothing");

        // `stride: 0` from a one-draw caller is legal: the API is never told
        // it, because there is nothing to step to.
        check_layout(&indirect(4, 1, 0), DRAW_ARGS_BYTES)
            .expect("one structure at offset 4 strides nowhere");

        // The rule this function exists for on the bounds-free backend: an
        // offset+stride that runs past any plausible buffer is still a legal
        // *layout*. `plan_structures` is what refuses it, and only because it
        // was given a length.
        let past_the_end = indirect(1 << 40, 4, 32);
        check_layout(&past_the_end, DRAW_ARGS_BYTES)
            .expect("a bound is not this function's to judge");
        assert!(
            matches!(
                plan_structures(&past_the_end, DRAW_ARGS_BYTES, 4096),
                Err(HalError::InvalidDescriptor(_))
            ),
            "the same draw must be refused where a length is available"
        );

        for (draw, args, what) in [
            (
                indirect(2, 1, 0),
                DRAW_ARGS_BYTES,
                "an offset that is not a multiple of four",
            ),
            (
                indirect(0, 2, 8),
                DRAW_ARGS_BYTES,
                "a stride below one 16-byte structure",
            ),
            (
                indirect(0, 2, 16),
                DRAW_INDEXED_ARGS_BYTES,
                "a stride below one 20-byte structure",
            ),
            (
                indirect(0, 2, 18),
                DRAW_ARGS_BYTES,
                "a stride that is not a multiple of four",
            ),
        ] {
            let Err(error) = check_layout(&draw, args) else {
                panic!("{what} must be refused");
            };
            assert!(
                matches!(error, HalError::InvalidDescriptor(_)),
                "{what}: {error:?}"
            );
        }
    }

    /// **A count-limited draw is held to the argument rules its plain form
    /// already has, plus the two the count word brings.**
    ///
    /// The count is read from GPU memory, so nothing here can see how many
    /// draws happen — which is why the plan covers `max_draw_count` structures
    /// and why the count word's own offset has to be judged separately. Neither
    /// mistake is one the APIs report: `vkCmdDrawIndirectCount` states
    /// `countBufferOffset`'s alignment and range as valid-usage conditions with
    /// no error code behind them, so an over-run offset means the driver
    /// fetches a draw count out of whatever is there.
    ///
    /// **The legal draws are half the test.** `crcbl-render`'s `EmitTail::Count`
    /// path records one of these per bucket every frame, so a check that
    /// refused everything would satisfy the refusals below and break the
    /// engine; the accepting side runs first, exact fits included.
    ///
    /// **What turns it red.** Dropping the count-offset alignment or the count
    /// bound turns one refusal green; writing either bound as `>=` refuses the
    /// exact fits above them; delegating the argument array to anything other
    /// than [`plan_structures`] over `max_draw_count` structures turns the
    /// stride and span refusals green, since those rules live only there.
    #[test]
    fn a_count_limited_draw_checks_the_count_word_and_the_argument_array() {
        // A draw of nothing reads neither buffer, which is `plan_structures`'
        // answer to a `draw_count` of zero and has to be this one's too.
        assert_eq!(
            plan_count_structures(&counted(2, 2, 0, 3), DRAW_ARGS_BYTES, 0, 0)
                .expect("a draw of nothing"),
            None
        );

        // Two structures at offset 32 of a 64-byte buffer and the count one
        // word into an 8-byte one: both spans end exactly at the end, which is
        // the layout `exercise_draw_indirect_count` in the seam's e2e suite
        // actually records.
        let tight = plan_count_structures(&counted(32, 4, 2, 16), DRAW_ARGS_BYTES, 64, 8)
            .expect("two tight structures and a count that both fit exactly")
            .expect("a draw of up to two");
        assert_eq!(tight.first, 32);
        assert_eq!(tight.stride, DRAW_ARGS_BYTES);
        assert_eq!(tight.count, 2);
        let offsets: Vec<u64> = (0..tight.count).map(|index| tight.offset(index)).collect();
        assert_eq!(offsets, vec![32, 48]);

        // A one-draw caller never strides, so `stride: 0` is legal here for the
        // reason it is legal in `check_layout`, and the plan reports the
        // structure's own width.
        let one = plan_count_structures(&counted(0, 0, 1, 0), DRAW_INDEXED_ARGS_BYTES, 20, 4)
            .expect("one indexed structure filling its buffer")
            .expect("a draw of up to one");
        assert_eq!(one.stride, DRAW_INDEXED_ARGS_BYTES);

        // A padded stride is honoured rather than tightened, as it is for the
        // plain form.
        let padded = plan_count_structures(&counted(8, 12, 3, 32), DRAW_ARGS_BYTES, 88, 16)
            .expect("three structures 32 bytes apart from offset 8")
            .expect("a draw of up to three");
        let offsets: Vec<u64> = (0..padded.count)
            .map(|index| padded.offset(index))
            .collect();
        assert_eq!(offsets, vec![8, 40, 72]);

        for (draw, args, args_length, count_length, what) in [
            (
                counted(0, 2, 1, 0),
                DRAW_ARGS_BYTES,
                64,
                16,
                "a count offset that is not a multiple of four",
            ),
            (
                counted(0, 4, 1, 0),
                DRAW_ARGS_BYTES,
                64,
                7,
                "a count word whose last byte is past the end",
            ),
            (
                counted(0, u64::MAX - 3, 1, 0),
                DRAW_ARGS_BYTES,
                64,
                8,
                "a count offset whose end overflows",
            ),
            (
                counted(2, 0, 1, 0),
                DRAW_ARGS_BYTES,
                64,
                8,
                "an argument offset that is not a multiple of four",
            ),
            (
                counted(0, 0, 2, 8),
                DRAW_ARGS_BYTES,
                64,
                8,
                "a stride below one 16-byte structure",
            ),
            (
                counted(0, 0, 2, 18),
                DRAW_ARGS_BYTES,
                64,
                8,
                "a stride that is not a multiple of four",
            ),
            (
                counted(32, 4, 2, 16),
                DRAW_ARGS_BYTES,
                63,
                8,
                "two tight structures one byte past the argument buffer",
            ),
        ] {
            let Err(error) = plan_count_structures(&draw, args, args_length, count_length) else {
                panic!("{what} must be refused");
            };
            assert!(
                matches!(error, HalError::InvalidDescriptor(_)),
                "{what}: {error:?}"
            );
        }
    }
}
