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
//! Everything here is arithmetic over a [`DrawIndirect`] and the length of the
//! buffer it names, so it is compiled and unit-tested on every host, with no
//! device in the room. That matters because none of these mistakes is one the
//! APIs report usefully: an out-of-range indirect offset is undefined on
//! Vulkan, and on Metal it raises — which aborts the process rather than
//! returning something a caller could report.

use crate::{DrawIndirect, HalError};

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

/// The shared body of every indirect *draw* plan: `args` bytes per structure,
/// and nothing that reads them.
///
/// `length` is the size of the buffer [`DrawIndirect::args`] names, which the
/// backend has already resolved; `None` is a draw of nothing, which is not an
/// error because there is no structure to read and no draw to issue.
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
    if !draw.offset.is_multiple_of(INDIRECT_OFFSET_ALIGNMENT) {
        return Err(HalError::InvalidDescriptor(format!(
            "an indirect draw's argument offset {} is not a multiple of {INDIRECT_OFFSET_ALIGNMENT}",
            draw.offset
        )));
    }
    // A single draw reads one structure at `offset` and never strides, so its
    // `stride` is not a value the API is ever told — checking it would refuse
    // the tightly-packed `stride: 0` a one-draw caller may well pass.
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
}
