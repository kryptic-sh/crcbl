//! Index buffers, indexed draws, and the indirect path — the arithmetic half.
//!
//! # Not Windows-only, for the reason [`crate::present`] is not
//!
//! Nothing here holds a `windows` type, and nothing here holds a seam *handle*
//! either: an index-buffer view is an address, a byte count and a format, and
//! every rule below is about offsets, strides and the length of the buffer they
//! run through. So the functions take the numbers rather than the descriptors
//! they came out of, and off Windows this module exists in the test build alone
//! — which is what lets `cargo test` on any host check the one part of the draw
//! path that can be checked without a D3D12 device.
//!
//! That matters more here than the pattern suggests: **none of these rules can
//! be observed from a draw's output.** A misaligned offset is a GPU fault, and a
//! stride one word short draws a plausible picture out of the wrong words.
//!
//! # `ExecuteIndirect` is the whole indirect path, and the command signature
//! shapes it
//!
//! D3D12 has no `vkCmdDrawIndirect`, no `vkCmdDrawIndexedIndirectCount` and no
//! per-call stride. It has one entry point that reads *any* argument layout an
//! `ID3D12CommandSignature` describes, and the signature carries the
//! `ByteStride` — so the seam's per-call `stride` is not an argument of the call
//! here, it is part of the object the call reads through. That is why
//! `crate::device`'s signature cache is keyed on `(kind, stride)` rather than
//! holding one signature per kind: two callers striding differently over the
//! same argument layout need two objects, and a signature built for one and used
//! by the other reads every structure after the first from the wrong offset.
//!
//! **The count buffer is a parameter rather than part of the signature**, which
//! is why one signature serves both the CPU-count and the GPU-count call, and
//! why [`DRAW_INDIRECT_COUNT`](crcbl_hal::Features::DRAW_INDIRECT_COUNT) costs
//! this backend nothing beyond passing it: `ExecuteIndirect`'s `pCountBuffer`
//! *is* the feature. `crcbl-mtl` had to refuse the same flag because Metal has
//! no such parameter anywhere — see `crcbl_mtl::draw` — and that difference is
//! the whole of why D3D12 derives
//! [`IndirectCount`](crcbl_hal::GeometryPath::IndirectCount) and Metal does not.
//!
//! # The argument structures are the same bytes as every other backend's
//!
//! `D3D12_DRAW_ARGUMENTS` is `{ VertexCountPerInstance, InstanceCount,
//! StartVertexLocation, StartInstanceLocation }` and
//! `D3D12_DRAW_INDEXED_ARGUMENTS` is `{ IndexCountPerInstance, InstanceCount,
//! StartIndexLocation, BaseVertexLocation, StartInstanceLocation }` — field for
//! field the layouts Vulkan calls `VkDrawIndirectCommand` and
//! `VkDrawIndexedIndirectCommand`, Metal calls
//! `MTLDrawPrimitivesIndirectArguments` and its indexed twin, and
//! `crcbl_shaders::draw_gen::DrawIndexedArgs` writes. The seam never spells the
//! layout, because it is the backend's native one; these agreeing is what lets
//! one compute pass feed every backend.
//!
//! `StartInstanceLocation` is a field D3D12 *reads*, not one it requires to be
//! zero, which is what earns
//! [`INDIRECT_FIRST_INSTANCE`](crcbl_hal::Features::INDIRECT_FIRST_INSTANCE).
//! The engine's own draws still pass zero for it and for `BaseVertexLocation` —
//! `crates/crcbl-shaders/shaders/mesh.slang`'s header measured the four shader
//! targets disagreeing about what a non-zero base does to `SV_VertexID` and
//! `SV_InstanceID`, and zero is the value all four agree on. That is the
//! caller's rule rather than this backend's: both fields are passed through
//! exactly as they arrive.

use crcbl_hal::{HalError, IndexFormat};

/// What one structure of an `ExecuteIndirect` argument buffer is.
///
/// One enum rather than three constants, because the three differ only in the
/// argument type they name and the width of the structure it reads — and every
/// rule in this module, and the signature cache in `crate::device`, is the same
/// for all of them.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum IndirectKind {
    /// `D3D12_DISPATCH_ARGUMENTS`, for
    /// [`dispatch_indirect`](crcbl_hal::CommandEncoder::dispatch_indirect).
    Dispatch,
    /// `D3D12_DRAW_ARGUMENTS`.
    Draw,
    /// `D3D12_DRAW_INDEXED_ARGUMENTS`.
    DrawIndexed,
}

impl IndirectKind {
    /// Bytes one argument structure occupies, fixed by D3D12's own struct: three
    /// `u32`s for a dispatch, four for a draw, five for an indexed draw.
    pub(crate) const fn arguments(self) -> u64 {
        match self {
            Self::Dispatch => 12,
            Self::Draw => 16,
            Self::DrawIndexed => 20,
        }
    }

    /// The seam call this layout belongs to, for an error a caller reads.
    pub(crate) const fn what(self) -> &'static str {
        match self {
            Self::Dispatch => "dispatch_indirect",
            Self::Draw => "an indirect draw",
            Self::DrawIndexed => "an indexed indirect draw",
        }
    }
}

/// What an indirect argument or count offset must be a multiple of: the width of
/// the `u32` words every one of these structures is made of, which is also what
/// D3D12 documents for both of `ExecuteIndirect`'s offsets.
pub(crate) const INDIRECT_ARGUMENT_ALIGNMENT: u64 = 4;

/// Bytes `ExecuteIndirect` reads from a count buffer: one `u32`.
pub(crate) const COUNT_BYTES: u64 = 4;

/// A validated indirect call: what `ExecuteIndirect` may read, and through a
/// signature of what stride.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct IndirectPlan {
    /// `MaxCommandCount`.
    pub(crate) count: u32,
    /// `ByteStride` of the command signature this must be executed through.
    pub(crate) stride: u32,
}

/// Checks an indirect call against D3D12's rules and the buffer it reads, or
/// says why it cannot be encoded.
///
/// `None` for a call of nothing, which is not an error: the seam's counts are
/// counts, and a culling pass that produced no buckets asks for zero draws.
///
/// For a call whose count comes out of GPU memory, `count` is the caller's
/// ceiling rather than the number that will be executed — which is the point of
/// that call, and why the ceiling is what has to fit: nothing on the CPU may
/// read the real count, so the largest span the call *could* read is the only
/// one it is safe to check.
///
/// `ExecuteIndirect` bounds-checks neither the offset nor the span, so a short
/// buffer is a GPU fault where this is an error a caller can catch — the same
/// trade `dispatch_indirect` already made, and `crcbl-mtl` makes for Metal.
///
/// # Errors
///
/// [`HalError::InvalidDescriptor`] when the offset is not
/// [`INDIRECT_ARGUMENT_ALIGNMENT`]-aligned, when a multi-command call's stride
/// is smaller than one argument structure or is not itself aligned, or when the
/// structures do not fit inside the buffer.
pub(crate) fn plan_indirect(
    kind: IndirectKind,
    offset: u64,
    count: u32,
    stride: u32,
    length: u64,
) -> Result<Option<IndirectPlan>, HalError> {
    if count == 0 {
        return Ok(None);
    }
    let arguments = kind.arguments();
    if !offset.is_multiple_of(INDIRECT_ARGUMENT_ALIGNMENT) {
        return Err(HalError::InvalidDescriptor(format!(
            "{} reads its arguments at offset {offset}, and D3D12 requires ExecuteIndirect's \
             ArgumentBufferOffset to be a multiple of {INDIRECT_ARGUMENT_ALIGNMENT}",
            kind.what()
        )));
    }
    // A call of one structure reads it at `offset` and never strides, so its
    // stride is not a number D3D12 is ever told — checking it would refuse the
    // tightly packed `stride: 0` a one-command caller may well pass. The
    // signature still needs a `ByteStride`, so it gets the structure's own
    // width, which also keeps every such caller on one cached signature.
    let stride = if count == 1 {
        arguments
    } else {
        let stride = u64::from(stride);
        if stride < arguments || !stride.is_multiple_of(INDIRECT_ARGUMENT_ALIGNMENT) {
            return Err(HalError::InvalidDescriptor(format!(
                "{} reads {count} argument structures at a stride of {stride}, and one structure \
                 is {arguments} bytes on a {INDIRECT_ARGUMENT_ALIGNMENT}-byte alignment",
                kind.what()
            )));
        }
        stride
    };
    let span = u64::from(count - 1)
        .checked_mul(stride)
        .and_then(|span| span.checked_add(arguments))
        .and_then(|span| offset.checked_add(span));
    if span.is_none_or(|span| span > length) {
        return Err(HalError::InvalidDescriptor(format!(
            "{} reads {count} argument structure(s) {stride} bytes apart from offset {offset}, \
             which runs past a {length}-byte buffer",
            kind.what()
        )));
    }
    let stride = u32::try_from(stride).map_err(|_| {
        HalError::InvalidDescriptor(format!(
            "{} has a stride of {stride}, and a command signature's ByteStride is a u32",
            kind.what()
        ))
    })?;
    Ok(Some(IndirectPlan { count, stride }))
}

/// Checks the `u32` an indirect-count call reads against the buffer holding it.
///
/// Separate from [`plan_indirect`] because it is a second buffer with its own
/// two rules, and because a call whose ceiling is zero reads neither buffer and
/// must not be refused over either.
///
/// # Errors
///
/// [`HalError::InvalidDescriptor`] when the offset is not
/// [`INDIRECT_ARGUMENT_ALIGNMENT`]-aligned — which D3D12 requires of
/// `CountBufferOffset` — or when the `u32` it names is not inside the buffer.
pub(crate) fn check_count(kind: IndirectKind, offset: u64, length: u64) -> Result<(), HalError> {
    if !offset.is_multiple_of(INDIRECT_ARGUMENT_ALIGNMENT) {
        return Err(HalError::InvalidDescriptor(format!(
            "{} reads its count at offset {offset}, and D3D12 requires ExecuteIndirect's \
             CountBufferOffset to be a multiple of {INDIRECT_ARGUMENT_ALIGNMENT}",
            kind.what()
        )));
    }
    if offset
        .checked_add(COUNT_BYTES)
        .is_none_or(|end| end > length)
    {
        return Err(HalError::InvalidDescriptor(format!(
            "{} reads a {COUNT_BYTES}-byte count at offset {offset} of a {length}-byte buffer",
            kind.what()
        )));
    }
    Ok(())
}

/// `SizeInBytes` of the `D3D12_INDEX_BUFFER_VIEW` a binding becomes, or why the
/// binding is not one D3D12 accepts.
///
/// The view's `BufferLocation` is the resource's GPU virtual address plus
/// `offset`, and D3D12 requires an index buffer's address to be aligned to the
/// index width — so the offset must be, since the allocation's own base already
/// is. `SizeInBytes` then bounds every index read the view serves, which is what
/// makes an index past the end a defined read of zero rather than a fault, and
/// why this is the only bound an indexed draw needs.
///
/// # Errors
///
/// [`HalError::InvalidDescriptor`] when the offset is not a multiple of the
/// index width, or is at or past the end of the buffer — a view holding no
/// indices serves no draw, and is worth refusing while it is still a caller's
/// mistake rather than a frame of nothing.
pub(crate) fn plan_index_binding(
    offset: u64,
    format: IndexFormat,
    length: u64,
) -> Result<u32, HalError> {
    let width = format.size();
    if !offset.is_multiple_of(width) {
        return Err(HalError::InvalidDescriptor(format!(
            "bind_index_buffer offset {offset} is not a multiple of the {format:?} index width of \
             {width}; D3D12 requires an index buffer's address to be aligned to its index size"
        )));
    }
    if offset >= length {
        return Err(HalError::InvalidDescriptor(format!(
            "bind_index_buffer offset {offset} is at or past the end of a {length}-byte buffer"
        )));
    }
    // Whole indices only, and never more than `SizeInBytes` can express. Both
    // are ceilings below the truth rather than above it: D3D12 reads zero past
    // the view's size, so a view that stops short of a huge buffer's tail is
    // safe where one that claimed the tail and wrapped would not be.
    let bytes = (length - offset) / width * width;
    let ceiling = u64::from(u32::MAX) / width * width;
    u32::try_from(bytes.min(ceiling)).map_err(|_| {
        HalError::InvalidDescriptor(format!(
            "bind_index_buffer cannot express {bytes} bytes of indices in a u32 SizeInBytes"
        ))
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The three argument widths are D3D12's own, and nothing else may decide
    /// them.
    ///
    /// **What turns it red.** Any of the three moving. They are the `ByteStride`
    /// a command signature is created with and the span every bounds check below
    /// is computed from, so one wrong number is a signature that reads every
    /// structure after the first from the wrong offset — which draws a plausible
    /// picture out of the wrong words rather than failing.
    #[test]
    fn the_argument_structures_are_the_widths_d3d12_declares() {
        assert_eq!(IndirectKind::Dispatch.arguments(), 12);
        assert_eq!(IndirectKind::Draw.arguments(), 16);
        assert_eq!(IndirectKind::DrawIndexed.arguments(), 20);
        // The same five words `draw_gen.slang` writes and every other backend
        // reads, which is what lets one compute pass feed all of them.
        assert_eq!(
            IndirectKind::DrawIndexed.arguments(),
            crcbl_shaders::draw_gen::DRAW_ARGS_SIZE as u64
        );
    }

    /// A single indirect call ignores the stride it was given; a multi-command
    /// one does not.
    ///
    /// **What turns it red.** Requiring a stride of a one-command call — the
    /// first assertion passes `0`, which is what a caller with one tightly
    /// packed structure writes, and it must still come back as the structure's
    /// own width so the cached signature is the shared one. Accepting a stride
    /// below one argument structure, or an unaligned one — the two `expect_err`s.
    /// Getting the indexed structure's width wrong — the last pair, where 16 is
    /// legal for a plain draw and one word short of an indexed one.
    #[test]
    fn an_indirect_calls_stride_is_only_checked_when_it_is_used() {
        assert_eq!(
            plan_indirect(IndirectKind::Draw, 0, 1, 0, 16).expect("one structure fills the buffer"),
            Some(IndirectPlan {
                count: 1,
                stride: 16
            })
        );
        assert_eq!(
            plan_indirect(IndirectKind::Draw, 0, 0, 0, 0).expect("zero draws read nothing"),
            None,
            "a draw of nothing is not an error"
        );

        let error = plan_indirect(IndirectKind::Draw, 0, 2, 12, 1024)
            .expect_err("12 is below one 16-byte structure");
        assert!(matches!(error, HalError::InvalidDescriptor(_)), "{error:?}");
        let error = plan_indirect(IndirectKind::Draw, 0, 2, 18, 1024)
            .expect_err("18 is not four-byte aligned");
        assert!(matches!(error, HalError::InvalidDescriptor(_)), "{error:?}");

        plan_indirect(IndirectKind::Draw, 0, 2, 16, 1024).expect("16 is one plain structure");
        let error = plan_indirect(IndirectKind::DrawIndexed, 0, 2, 16, 1024)
            .expect_err("an indexed structure is 20 bytes");
        assert!(matches!(error, HalError::InvalidDescriptor(_)), "{error:?}");
        plan_indirect(IndirectKind::DrawIndexed, 0, 2, 20, 1024)
            .expect("20 is one indexed structure");
    }

    /// The span a multi-command call reads is bounded by the buffer, and the
    /// offset it starts at must be aligned.
    ///
    /// **What turns it red.** Computing the span as `count * stride` rather than
    /// `(count - 1) * stride + arguments` — the exact-fit assertion, which is the
    /// largest legal call and would be refused. Dropping the bounds check — the
    /// one-byte-short `expect_err`. Dropping the offset's alignment check — the
    /// last one.
    #[test]
    fn a_multi_command_span_is_bounded_by_the_buffer_it_reads() {
        // Three structures, 32 bytes apart, from offset 8: the last one ends at
        // 8 + 64 + 16 = 88, which is exactly the buffer.
        assert_eq!(
            plan_indirect(IndirectKind::Draw, 8, 3, 32, 88)
                .expect("the last structure ends exactly at the end"),
            Some(IndirectPlan {
                count: 3,
                stride: 32
            })
        );
        let error = plan_indirect(IndirectKind::Draw, 8, 3, 32, 87).expect_err("one byte short");
        assert!(matches!(error, HalError::InvalidDescriptor(_)), "{error:?}");

        let error = plan_indirect(IndirectKind::Draw, 2, 1, 0, 1024).expect_err("2 is not aligned");
        assert!(matches!(error, HalError::InvalidDescriptor(_)), "{error:?}");
    }

    /// A GPU-side count is bounded by its ceiling, and the count itself has to
    /// be inside its own buffer at an aligned offset.
    ///
    /// **What turns it red.** Sizing the argument span off anything but the
    /// ceiling — the first `expect_err` passes a buffer holding two structures
    /// and a ceiling of three, which no CPU-side read may excuse. Dropping the
    /// count buffer's bounds check — the second, where the `u32` ends one byte
    /// past a six-byte buffer. Dropping its alignment check — the third.
    #[test]
    fn a_gpu_side_count_is_bounded_by_the_ceiling_and_its_own_buffer() {
        let kind = IndirectKind::DrawIndexed;
        assert_eq!(
            plan_indirect(kind, 0, 3, 20, 60).expect("three 20-byte structures fill 60 bytes"),
            Some(IndirectPlan {
                count: 3,
                stride: 20
            })
        );
        let error = plan_indirect(kind, 0, 3, 20, 40)
            .expect_err("the ceiling is three structures and the buffer holds two");
        assert!(matches!(error, HalError::InvalidDescriptor(_)), "{error:?}");

        check_count(kind, 4, 8).expect("a u32 at offset 4 fits in eight bytes");
        let error = check_count(kind, 4, 6).expect_err("it does not fit in six");
        assert!(matches!(error, HalError::InvalidDescriptor(_)), "{error:?}");
        let error = check_count(kind, 2, 64).expect_err("2 is not four-byte aligned");
        assert!(matches!(error, HalError::InvalidDescriptor(_)), "{error:?}");
    }

    /// An index binding is aligned to its own width, bounded by the buffer, and
    /// measured in whole indices.
    ///
    /// **What turns it red.** Dropping the alignment check — the first
    /// assertion, which uses an offset D3D12 refuses for 32-bit indices and
    /// accepts for 16-bit ones, so the refusal is about the width rather than
    /// about the number. Dropping the bounds check — the second. Rounding the
    /// size up rather than down to whole indices — the third, where three bytes
    /// of tail must not become a fourth index.
    #[test]
    fn an_index_binding_is_aligned_bounded_and_measured_in_whole_indices() {
        let error = plan_index_binding(2, IndexFormat::Uint32, 64)
            .expect_err("2 is not a multiple of four");
        assert!(matches!(error, HalError::InvalidDescriptor(_)), "{error:?}");
        assert_eq!(
            plan_index_binding(2, IndexFormat::Uint16, 64).expect("2 is a multiple of two"),
            62
        );

        let error =
            plan_index_binding(64, IndexFormat::Uint32, 64).expect_err("64 is past the end");
        assert!(matches!(error, HalError::InvalidDescriptor(_)), "{error:?}");

        assert_eq!(
            plan_index_binding(4, IndexFormat::Uint32, 19).expect("15 bytes of tail"),
            12,
            "three trailing bytes are not a fourth index"
        );

        // A buffer wider than `SizeInBytes` can express is clamped to whole
        // indices below the ceiling rather than wrapping.
        let huge = plan_index_binding(0, IndexFormat::Uint32, 1 << 33).expect("an 8 GiB buffer");
        assert_eq!(huge, u32::MAX - 3);
        assert!(u64::from(huge).is_multiple_of(IndexFormat::Uint32.size()));
    }
}
