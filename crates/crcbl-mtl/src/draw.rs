//! Index buffers, indexed draws, and the indirect path.
//!
//! # An index buffer is a draw argument, so this backend carries the binding
//!
//! Metal has no `vkCmdBindIndexBuffer`. `drawIndexedPrimitives:` takes the
//! buffer, the offset and the index type as **arguments of the draw**, so
//! [`bind_index_buffer`](crcbl_hal::CommandEncoder::bind_index_buffer) has no
//! encoder call to make and the backend has to hold the binding until a draw
//! reads it. [`IndexBinding`] is that state.
//!
//! It is deliberately **not** cleared when a Metal encoder closes, which is the
//! opposite of what `crcbl_mtl::command`'s `bound_primitive` does. The reason
//! is the difference the two record: a pipeline state genuinely dies with its
//! render encoder, so keeping it would let a draw in the next pass proceed with
//! nothing set; an index binding has no Metal state at all, so keeping it makes
//! this backend behave the way Vulkan does, where the binding belongs to the
//! command buffer and survives a pass boundary.
//!
//! For the same reason [`bind_index_buffer`](crcbl_hal::CommandEncoder::bind_index_buffer)
//! is legal outside a render pass here. Metal is not being told anything, so
//! there is nothing to have nowhere to go.
//!
//! # The CPU-counted indirect path is a **loop**; the GPU-counted one is an ICB
//!
//! `docs/plan/09-backends-metal-dx12.md`'s mapping table offers "ICBs or
//! indirect command encoding — closest-fit chosen during implementation" for
//! draw-indirect-count. The answer is both, split by which seam call it is,
//! because the two are not equally hard.
//!
//! * [`draw_indirect`](crcbl_hal::CommandEncoder::draw_indirect) and
//!   [`draw_indexed_indirect`](crcbl_hal::CommandEncoder::draw_indexed_indirect)
//!   pass a **CPU-known** `draw_count`. Metal's
//!   `drawPrimitives:indirectBuffer:indirectBufferOffset:` emits one draw from
//!   GPU-written arguments, so N of them emit N draws from N argument
//!   structures — which is exactly what
//!   [`DrawIndirect`](crcbl_hal::DrawIndirect) means. The count is CPU-side by
//!   definition, so the loop is not an approximation of multi-draw-indirect; it
//!   *is* multi-draw-indirect, at the cost of N encoder calls instead of one.
//!   That is what earns
//!   [`MULTI_DRAW_INDIRECT`](crcbl_hal::Features::MULTI_DRAW_INDIRECT).
//! * [`draw_indirect_count`](crcbl_hal::CommandEncoder::draw_indirect_count)
//!   and its indexed sibling read the count from GPU memory, so no draw call
//!   Metal has can express them: `executeCommandsInBuffer:` over an
//!   `MTLIndirectCommandBuffer` is the one execution whose extent the GPU
//!   decides, and an ICB's commands must already exist. `crcbl_mtl::icb` is
//!   where they come from — a compute kernel that runs before the render
//!   encoder is opened, which is what `crcbl_mtl::command`'s deferred recording
//!   exists to make possible. [`plan_indirect_count`] is that path's half of
//!   this module: the same bounds as [`plan_indirect`], over the two buffers
//!   and the ceiling an ICB adds.
//!
//!   What is *not* done, and was rejected before the ICB path was written:
//!   issuing `max_draw_count` draws and hoping the unused argument structures
//!   are zeroed. Nothing in the seam promises they are, so that version is
//!   silently wrong rather than merely slow.
//!
//! # The argument structures are the same bytes on both APIs
//!
//! `MTLDrawPrimitivesIndirectArguments` is `{ vertexCount, instanceCount,
//! vertexStart, baseInstance }` and
//! `MTLDrawIndexedPrimitivesIndirectArguments` is `{ indexCount,
//! instanceCount, indexStart, baseVertex, baseInstance }` — field for field,
//! width for width, the layouts Vulkan calls `VkDrawIndirectCommand` and
//! `VkDrawIndexedIndirectCommand` and wgpu calls `DrawIndirectArgs` and
//! `DrawIndexedIndirectArgs`. The seam never writes the layout down, so what
//! each backend requires of an argument buffer is its own API's; these two
//! agreeing is what lets one compute pass feed every backend, and it is why
//! [`INDIRECT_FIRST_INSTANCE`](crcbl_hal::Features::INDIRECT_FIRST_INSTANCE) is
//! reported — `baseInstance` is a field Metal reads, not one it requires to be
//! zero.

use crcbl_hal::{DrawIndirect, DrawIndirectCount, HalError, IndexFormat};
use objc2::rc::Retained;
use objc2::runtime::ProtocolObject;
use objc2_metal::{MTLBuffer, MTLIndexType};

/// Bytes of one `MTLDrawPrimitivesIndirectArguments`: four 32-bit fields.
///
/// Fixed by the API, and identical to Vulkan's `VkDrawIndirectCommand`; see the
/// module docs.
const DRAW_ARGS_BYTES: u64 = 16;

/// Bytes of one `MTLDrawIndexedPrimitivesIndirectArguments`: five 32-bit
/// fields. See [`DRAW_ARGS_BYTES`].
const DRAW_INDEXED_ARGS_BYTES: u64 = 20;

/// Alignment Metal requires of an indirect buffer offset.
///
/// The argument structures are `uint32_t` fields, and Metal's headers document
/// `indirectBufferOffset` as a multiple of four.
const INDIRECT_OFFSET_ALIGNMENT: u64 = 4;

/// The index buffer a later draw will read, held because Metal takes it at the
/// draw call. See the module docs.
#[derive(Debug)]
pub(crate) struct IndexBinding {
    pub(crate) raw: Retained<ProtocolObject<dyn MTLBuffer>>,
    /// Byte offset of index zero.
    pub(crate) offset: u64,
    pub(crate) format: IndexFormat,
    /// How many indices lie between [`offset`](Self::offset) and the end of the
    /// allocation. Kept so an indexed draw's range can be checked without
    /// asking the buffer its length again.
    pub(crate) capacity: u64,
}

impl IndexBinding {
    /// Metal's spelling of the seam's index width.
    pub(crate) const fn index_type(&self) -> MTLIndexType {
        match self.format {
            IndexFormat::Uint16 => MTLIndexType::UInt16,
            IndexFormat::Uint32 => MTLIndexType::UInt32,
        }
    }

    /// The byte offset a draw starting at index `first` reads from, or why the
    /// range does not fit. See [`index_draw_offset`].
    pub(crate) fn draw_offset(&self, first: u32, count: u32) -> Result<u64, HalError> {
        index_draw_offset(self.offset, self.format, self.capacity, first, count)
    }
}

/// The byte offset an indexed draw of `count` indices from `first` reads, or
/// why the range does not fit the bound one.
///
/// Metal bounds-checks neither the offset nor the count and raises on an
/// overrun, so both ends are checked against the allocation's own length — the
/// same treatment every copy in `crcbl_mtl::command` gets. Free-standing rather
/// than a method body so the arithmetic is testable without an `MTLBuffer` to
/// hold.
pub(crate) fn index_draw_offset(
    offset: u64,
    format: IndexFormat,
    capacity: u64,
    first: u32,
    count: u32,
) -> Result<u64, HalError> {
    let end = u64::from(first).saturating_add(u64::from(count));
    if end > capacity {
        return Err(HalError::InvalidDescriptor(format!(
            "draw_indexed reads indices {first}..{end} and the bound range holds {capacity}"
        )));
    }
    Ok(offset + u64::from(first) * format.size())
}

/// Where an index buffer binding starts and how much of it a draw may read, or
/// why the binding is not one Metal accepts.
///
/// # Errors
///
/// [`HalError::InvalidDescriptor`] when the offset is past the end of the
/// buffer, or is not a multiple of the index width — which Metal's own headers
/// require of `indexBufferOffset` and enforce by raising.
pub(crate) fn plan_index_binding(
    offset: u64,
    format: IndexFormat,
    length: u64,
) -> Result<u64, HalError> {
    if !offset.is_multiple_of(format.size()) {
        return Err(HalError::InvalidDescriptor(format!(
            "bind_index_buffer offset {offset} is not a multiple of the {:?} index width of {}; \
             Metal requires indexBufferOffset to be a multiple of the index size",
            format,
            format.size()
        )));
    }
    if offset >= length {
        return Err(HalError::InvalidDescriptor(format!(
            "bind_index_buffer offset {offset} is past the end of a {length}-byte buffer"
        )));
    }
    Ok((length - offset) / format.size())
}

/// A validated indirect draw: where the first argument structure is, how far
/// apart the rest are, and how many there are.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct IndirectPlan {
    pub(crate) first: u64,
    pub(crate) stride: u64,
    pub(crate) count: u32,
}

impl IndirectPlan {
    /// The byte offset of argument structure `index`.
    pub(crate) const fn offset(self, index: u32) -> u64 {
        self.first + self.stride * index as u64
    }
}

/// Checks an indirect draw against Metal's rules and the buffer it reads, or
/// says why it cannot be encoded.
///
/// `None` for a draw of nothing, which is not an error: the seam's counts are
/// counts, and a culling pass that produced no buckets asks for zero draws.
///
/// Pure, so the rules below are unit-testable without a GPU — which matters
/// because none of them can be observed from a draw's *output*, only from a
/// raise or a wrong picture.
///
/// # Errors
///
/// [`HalError::InvalidDescriptor`] when the offset is not four-byte aligned,
/// when a multi-draw's stride is smaller than one argument structure or is not
/// itself four-byte aligned, or when the structures do not fit inside the
/// buffer.
pub(crate) fn plan_indirect(
    draw: &DrawIndirect,
    indexed: bool,
    length: u64,
) -> Result<Option<IndirectPlan>, HalError> {
    if draw.draw_count == 0 {
        return Ok(None);
    }
    let args = if indexed {
        DRAW_INDEXED_ARGS_BYTES
    } else {
        DRAW_ARGS_BYTES
    };
    if !draw.offset.is_multiple_of(INDIRECT_OFFSET_ALIGNMENT) {
        return Err(HalError::InvalidDescriptor(format!(
            "an indirect draw's argument offset {} is not a multiple of {INDIRECT_OFFSET_ALIGNMENT}",
            draw.offset
        )));
    }
    // A single draw reads one structure at `offset` and never strides, so its
    // `stride` is not a value Metal is ever told — checking it would refuse the
    // tightly-packed `stride: 0` a one-draw caller may well pass.
    let stride = if draw.draw_count == 1 {
        args
    } else {
        let stride = u64::from(draw.stride);
        if stride < args || !stride.is_multiple_of(INDIRECT_OFFSET_ALIGNMENT) {
            return Err(HalError::InvalidDescriptor(format!(
                "an indirect draw of {} structures has a stride of {stride}, and one argument \
                 structure is {args} bytes on a {INDIRECT_OFFSET_ALIGNMENT}-byte alignment",
                draw.draw_count
            )));
        }
        stride
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

/// Bytes of the `uint32_t` draw count an indirect-count draw reads.
const COUNT_BYTES: u64 = 4;

/// A validated indirect-count draw: where each buffer is read from, how far
/// apart the argument structures are, and how many commands the indirect
/// command buffer needs.
///
/// The offsets are byte offsets the compute encoder *binds* at rather than ones
/// the kernel adds, which is why they are separated from the stride: MSL sees
/// `args[0]` and `counts[0]`, and where those point is Metal's business.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct IndirectCountPlan {
    pub(crate) args_offset: u64,
    pub(crate) count_offset: u64,
    /// Bytes between consecutive argument structures. Always a multiple of
    /// four, because `crcbl_mtl::icb`'s kernel steps a `uint` pointer by
    /// `stride / 4`.
    pub(crate) stride: u32,
    /// Commands the ICB is created with, threads the kernel is dispatched with,
    /// and the ceiling the GPU-written count is clamped to.
    pub(crate) max_draws: u32,
}

/// Checks an indirect-count draw against Metal's rules, the two buffers it
/// reads and the ICB ceiling this backend promises — or says why it cannot be
/// encoded.
///
/// `None` for a draw bounded at nothing, which is not an error for the reason
/// [`plan_indirect`] gives: a `max_draw_count` of zero is a caller saying this
/// bucket has no room, and there is nothing to allocate or dispatch for it.
///
/// `ceiling` is [`Limits::max_draw_indirect_count`](crcbl_hal::Limits::max_draw_indirect_count),
/// which on this backend is what an ICB was actually created with at
/// enumeration — see `crcbl_mtl::icb`'s `MAX_DRAW_INDIRECT_COUNT`. Checked here
/// rather than left to Metal because `newIndirectCommandBufferWithDescriptor:`
/// answers a request it cannot meet with nil and no reason.
///
/// Pure, so every rule below is unit-testable without a GPU — which matters
/// because none of them can be observed from a draw's *output*, only from a
/// raise, a nil allocation or a wrong picture.
///
/// # Errors
///
/// [`HalError::InvalidDescriptor`] when either offset is not four-byte aligned,
/// when `max_draw_count` exceeds `ceiling`, when the count word does not fit
/// the count buffer, when a multi-draw's stride is smaller than one argument
/// structure or is not itself four-byte aligned, or when the structures do not
/// fit inside the argument buffer.
pub(crate) fn plan_indirect_count(
    draw: &DrawIndirectCount,
    indexed: bool,
    args_length: u64,
    count_length: u64,
    ceiling: u32,
) -> Result<Option<IndirectCountPlan>, HalError> {
    if draw.max_draw_count == 0 {
        return Ok(None);
    }
    if draw.max_draw_count > ceiling {
        return Err(HalError::InvalidDescriptor(format!(
            "an indirect-count draw is bounded at {} structures and this device's \
             max_draw_indirect_count is {ceiling}",
            draw.max_draw_count
        )));
    }
    let args = if indexed {
        DRAW_INDEXED_ARGS_BYTES
    } else {
        DRAW_ARGS_BYTES
    };
    for (what, offset) in [("argument", draw.args_offset), ("count", draw.count_offset)] {
        if !offset.is_multiple_of(INDIRECT_OFFSET_ALIGNMENT) {
            return Err(HalError::InvalidDescriptor(format!(
                "an indirect-count draw's {what} offset {offset} is not a multiple of \
                 {INDIRECT_OFFSET_ALIGNMENT}"
            )));
        }
    }
    if draw
        .count_offset
        .checked_add(COUNT_BYTES)
        .is_none_or(|end| end > count_length)
    {
        return Err(HalError::InvalidDescriptor(format!(
            "an indirect-count draw reads a {COUNT_BYTES}-byte count at offset {} of a \
             {count_length}-byte buffer",
            draw.count_offset
        )));
    }
    // As `plan_indirect`: a single structure is never strided past, so the
    // tightly-packed `stride: 0` a one-draw caller may write is not a refusal.
    let stride = if draw.max_draw_count == 1 {
        args
    } else {
        let stride = u64::from(draw.stride);
        if stride < args || !stride.is_multiple_of(INDIRECT_OFFSET_ALIGNMENT) {
            return Err(HalError::InvalidDescriptor(format!(
                "an indirect-count draw of up to {} structures has a stride of {stride}, and one \
                 argument structure is {args} bytes on a {INDIRECT_OFFSET_ALIGNMENT}-byte \
                 alignment",
                draw.max_draw_count
            )));
        }
        stride
    };
    let span = u64::from(draw.max_draw_count - 1)
        .checked_mul(stride)
        .and_then(|span| span.checked_add(args))
        .and_then(|span| draw.args_offset.checked_add(span));
    if span.is_none_or(|span| span > args_length) {
        return Err(HalError::InvalidDescriptor(format!(
            "an indirect-count draw of up to {} structures {stride} bytes apart from offset {} \
             runs past a {args_length}-byte buffer",
            draw.max_draw_count, draw.args_offset
        )));
    }
    Ok(Some(IndirectCountPlan {
        args_offset: draw.args_offset,
        count_offset: draw.count_offset,
        // Fits because it is either `args` or a `u32` the caller passed.
        stride: stride as u32,
        max_draws: draw.max_draw_count,
    }))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crcbl_core::Handle;

    /// A handle no device issued — every function here is pure and never
    /// resolves it.
    fn args() -> crcbl_hal::BufferHandle {
        Handle::from_bits(1 << 32).expect("generation 1 is non-zero")
    }

    fn draw(offset: u64, draw_count: u32, stride: u32) -> DrawIndirect {
        DrawIndirect {
            args: args(),
            offset,
            draw_count,
            stride,
        }
    }

    /// The index binding's two refusals and the offset it computes.
    ///
    /// **What turns it red.** Dropping the alignment check — the first
    /// assertion, which uses an offset Metal's own headers forbid. Dropping the
    /// bounds check — the second. Forgetting to scale the first index by the
    /// index width — the third, where `Uint32` and `Uint16` must disagree.
    #[test]
    fn an_index_binding_is_aligned_bounded_and_scaled_by_its_width() {
        let error = plan_index_binding(2, IndexFormat::Uint32, 64)
            .expect_err("2 is not a multiple of four");
        assert!(matches!(error, HalError::InvalidDescriptor(_)), "{error:?}");
        // The same offset is legal for the narrower width, so the refusal is
        // about the width rather than about the number.
        assert_eq!(
            plan_index_binding(2, IndexFormat::Uint16, 64).expect("2 is a multiple of two"),
            31
        );

        let error =
            plan_index_binding(64, IndexFormat::Uint32, 64).expect_err("64 is past the end");
        assert!(matches!(error, HalError::InvalidDescriptor(_)), "{error:?}");

        let capacity = plan_index_binding(8, IndexFormat::Uint32, 64).expect("8..64 is 14 indices");
        assert_eq!(capacity, 14);
        assert_eq!(
            index_draw_offset(8, IndexFormat::Uint32, capacity, 3, 4).expect("3..7 of 14"),
            8 + 3 * 4
        );
        assert_eq!(
            index_draw_offset(8, IndexFormat::Uint16, capacity, 3, 4).expect("3..7 of 14"),
            8 + 3 * 2,
            "the first index is scaled by the index width, not by four"
        );
        let error = index_draw_offset(8, IndexFormat::Uint32, capacity, 12, 4)
            .expect_err("12..16 does not fit in 14");
        assert!(matches!(error, HalError::InvalidDescriptor(_)), "{error:?}");
    }

    /// A single indirect draw ignores the stride; a multi-draw does not.
    ///
    /// **What turns it red.** Requiring a stride of a one-draw call — the first
    /// assertion passes `0`, which is what a caller with one tightly packed
    /// structure writes. Accepting a stride below one argument structure, or an
    /// unaligned one — the two `expect_err`s. Getting the indexed structure's
    /// size wrong — the fourth assertion, where 16 is legal for a plain draw
    /// and too small for an indexed one.
    #[test]
    fn a_metal_indirect_draws_stride_is_only_checked_when_it_is_used() {
        assert_eq!(
            plan_indirect(&draw(0, 1, 0), false, 16).expect("one structure fills the buffer"),
            Some(IndirectPlan {
                first: 0,
                stride: DRAW_ARGS_BYTES,
                count: 1
            })
        );
        assert_eq!(
            plan_indirect(&draw(0, 0, 0), false, 0).expect("zero draws read nothing"),
            None,
            "a draw of nothing is not an error"
        );

        let error = plan_indirect(&draw(0, 2, 12), false, 1024).expect_err("12 < 16");
        assert!(matches!(error, HalError::InvalidDescriptor(_)), "{error:?}");
        let error = plan_indirect(&draw(0, 2, 18), false, 1024).expect_err("18 is not aligned");
        assert!(matches!(error, HalError::InvalidDescriptor(_)), "{error:?}");

        plan_indirect(&draw(0, 2, 16), false, 1024).expect("16 is one plain structure");
        let error = plan_indirect(&draw(0, 2, 16), true, 1024)
            .expect_err("an indexed structure is 20 bytes");
        assert!(matches!(error, HalError::InvalidDescriptor(_)), "{error:?}");
        plan_indirect(&draw(0, 2, 20), true, 1024).expect("20 is one indexed structure");
    }

    /// The span a multi-draw reads is bounded by the buffer, and the offsets it
    /// produces really do stride.
    ///
    /// **What turns it red.** Computing the span as `count * stride` rather
    /// than `(count - 1) * stride + args` — the exact-fit assertion, which is
    /// the largest legal call and would be refused. Dropping the bounds check —
    /// the one-byte-short `expect_err`. Dropping the alignment check on the
    /// offset — the last one.
    #[test]
    fn a_metal_multi_draws_span_is_bounded_by_the_buffer_it_reads() {
        // Three structures, 32 bytes apart, from offset 8: the last one ends at
        // 8 + 64 + 16 = 88, which is exactly the buffer.
        let plan = plan_indirect(&draw(8, 3, 32), false, 88)
            .expect("the last structure ends exactly at the end")
            .expect("three draws");
        assert_eq!(plan.count, 3);
        assert_eq!(plan.offset(0), 8);
        assert_eq!(plan.offset(1), 40);
        assert_eq!(plan.offset(2), 72);

        let error = plan_indirect(&draw(8, 3, 32), false, 87).expect_err("one byte short");
        assert!(matches!(error, HalError::InvalidDescriptor(_)), "{error:?}");

        let error = plan_indirect(&draw(2, 1, 0), false, 1024).expect_err("2 is not aligned");
        assert!(matches!(error, HalError::InvalidDescriptor(_)), "{error:?}");
    }

    fn count_draw(
        args_offset: u64,
        count_offset: u64,
        max_draw_count: u32,
        stride: u32,
    ) -> DrawIndirectCount {
        DrawIndirectCount {
            args: args(),
            args_offset,
            count_buffer: args(),
            count_offset,
            max_draw_count,
            stride,
        }
    }

    /// What the indirect-count plan adds over the plain one: two offsets, a
    /// count word that has to fit, and the ICB ceiling.
    ///
    /// **What turns it red.** Dropping the ceiling check — the first
    /// `expect_err`, which asks for one structure more than the ceiling
    /// allows while every other bound is satisfied. Checking only the argument
    /// offset's alignment — the second, whose *count* offset is the unaligned
    /// one. Forgetting that the count is four bytes rather than a position —
    /// the third, where offset 4 of a 4-byte buffer is in range and the word
    /// it names is not. Bounding nothing at zero — the `None`, which must not
    /// be an error and must not allocate an ICB of nothing.
    #[test]
    fn an_indirect_count_draw_is_bounded_by_both_buffers_and_by_the_icb_ceiling() {
        assert_eq!(
            plan_indirect_count(&count_draw(0, 0, 0, 0), false, 0, 0, 4096)
                .expect("a ceiling of zero structures reads nothing"),
            None,
            "a draw bounded at nothing is not an error"
        );

        let plan = plan_indirect_count(&count_draw(32, 4, 2, 16), false, 1024, 8, 4096)
            .expect("two tightly packed structures at 32 fit 1024 bytes")
            .expect("two draws");
        assert_eq!(
            plan,
            IndirectCountPlan {
                args_offset: 32,
                count_offset: 4,
                stride: 16,
                max_draws: 2,
            }
        );

        let error = plan_indirect_count(&count_draw(0, 0, 5, 16), false, 1024, 8, 4)
            .expect_err("five structures against a ceiling of four");
        assert!(matches!(error, HalError::InvalidDescriptor(_)), "{error:?}");

        let error = plan_indirect_count(&count_draw(0, 2, 1, 0), false, 1024, 8, 4096)
            .expect_err("the count offset 2 is not aligned");
        assert!(matches!(error, HalError::InvalidDescriptor(_)), "{error:?}");

        let error = plan_indirect_count(&count_draw(0, 4, 1, 0), false, 1024, 4, 4096)
            .expect_err("a four-byte count at offset 4 of a four-byte buffer");
        assert!(matches!(error, HalError::InvalidDescriptor(_)), "{error:?}");
    }

    /// The stride and span rules are [`plan_indirect`]'s, over the indexed
    /// structure's larger size.
    ///
    /// **What turns it red.** Requiring a stride of a one-draw call — the
    /// first assertion, which passes `0`. Using the plain structure's 16 bytes
    /// for an indexed draw — the second, where 16 is too small. Computing the
    /// span as `count * stride` rather than `(count - 1) * stride + args` —
    /// the exact-fit assertion, the largest legal call. Dropping the bound
    /// entirely — the one-byte-short `expect_err`.
    #[test]
    fn an_indirect_count_draws_stride_matches_the_structure_its_call_reads() {
        let plan = plan_indirect_count(&count_draw(0, 0, 1, 0), true, 20, 4, 4096)
            .expect("one indexed structure fills the buffer")
            .expect("one draw");
        assert_eq!(plan.stride, DRAW_INDEXED_ARGS_BYTES as u32);

        let error = plan_indirect_count(&count_draw(0, 0, 2, 16), true, 1024, 4, 4096)
            .expect_err("an indexed structure is 20 bytes");
        assert!(matches!(error, HalError::InvalidDescriptor(_)), "{error:?}");

        // Three structures 32 bytes apart from offset 8: the last ends at
        // 8 + 64 + 20 = 92, exactly the buffer.
        plan_indirect_count(&count_draw(8, 0, 3, 32), true, 92, 4, 4096)
            .expect("the last structure ends exactly at the end");
        let error = plan_indirect_count(&count_draw(8, 0, 3, 32), true, 91, 4, 4096)
            .expect_err("one byte short");
        assert!(matches!(error, HalError::InvalidDescriptor(_)), "{error:?}");
    }
}
