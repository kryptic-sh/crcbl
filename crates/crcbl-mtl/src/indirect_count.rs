//! The GPU-side draw count, without an indirect command buffer.
//!
//! # The problem, and the three attempts that are not this
//!
//! [`draw_indirect_count`](crcbl_hal::CommandEncoder::draw_indirect_count)
//! takes its draw count from device memory.
//! `drawPrimitives:indirectBuffer:indirectBufferOffset:` — the call
//! `crcbl_mtl::draw`'s multi-draw loop is built from — emits exactly one draw
//! and reads no count at all, and Metal has no second form that does.
//!
//! Metal's only count-from-memory *execution* is
//! `executeCommandsInBuffer:indirectBuffer:indirectBufferOffset:` over an
//! `MTLIndirectCommandBuffer`, whose commands a compute kernel has to encode
//! first. That was written three times and hung the GPU in a frame every time,
//! while the isolated probes passed on the same device; `docs/backlog.md` has
//! the table and the evidence. **This module is not that**, and nothing here
//! creates an ICB, an argument buffer or a `gpuResourceID`.
//!
//! # What it is instead: zero the instance count, then draw unconditionally
//!
//! A draw of **zero instances renders nothing**. That is not a trick — it is
//! what the instance count means, in Metal's argument structure exactly as in
//! Vulkan's. So a GPU-side count needs no new draw call:
//!
//! 1. A compute kernel reads the count and packs the argument structures the
//!    pass will issue, giving every structure at or past the count **no
//!    instances**.
//! 2. The pass issues `max_draw_count` ordinary indirect draws,
//!    unconditionally, through the same two calls
//!    [`MULTI_DRAW_INDIRECT`](crcbl_hal::Features::MULTI_DRAW_INDIRECT) already
//!    rests on and this runner already passes.
//!
//! The kernel is `crcbl-shaders`' `indirect_count_args.slang`, and its header
//! carries the shader-side half of the argument. The dispatch is placed by
//! `crcbl_mtl::command`, which defers render-pass encoding precisely so that
//! something can run *before* the render encoder opens; the kernel must,
//! because the arguments it writes are the ones that pass will read.
//!
//! # It packs into a buffer this backend owns
//!
//! Zeroing the instance count **in the caller's own argument buffer** is one
//! word per structure and no allocation, and it is silently wrong the second
//! time that buffer is drawn from: the zeroes survive the frame, so a later
//! count that is *larger* finds structures an earlier, smaller count had
//! already emptied. Nothing observes that but the picture. Every other
//! backend's count-limited draw leaves the caller's arguments alone, and so
//! does this one — the kernel copies, and the draws read the copy.
//!
//! The copy is **tight**: structure `i` is read at the caller's stride and
//! written at [`CountPlan::structure_bytes`], so the scratch allocation is
//! `max_draw_count` structures however far apart the caller's are — at most
//! [`MAX_DRAWS`] of them, which is why it is a trivial allocation whatever the
//! caller's arguments look like.
//!
//! # This module holds no Objective-C
//!
//! Same arrangement as `crcbl_mtl::argument`, `::pass`, `::query` and
//! `::quirk`, and for the same reason: what is decided here is arithmetic, and
//! Metal reports none of it wrong — an out-of-range indirect offset raises,
//! which aborts the process rather than returning an error. So the module is
//! compiled in the test build on every host and `cargo test` runs its rules
//! without a Mac. The two Metal-facing halves live where their objects do:
//! `crcbl_mtl::device` compiles the kernel, and `crcbl_mtl::command` records
//! the dispatch and the draws.

use crcbl_hal::{DrawIndirectCount, HalError, INDIRECT_OFFSET_ALIGNMENT, structure_bytes};

/// Draws one [`draw_indirect_count`](crcbl_hal::CommandEncoder::draw_indirect_count)
/// on this backend may issue, and therefore this backend's
/// [`Limits::max_draw_indirect_count`](crcbl_hal::Limits::max_draw_indirect_count).
///
/// **The number is a choice, not a measurement**, because there is no
/// descriptor to create and nothing in Metal to ask. It is the count of draws
/// this design is willing to issue, and it is small on purpose: the pass emits
/// `max_draw_count` draws where a real count-limited draw emits `count`, so the
/// cost of a bound is paid on every frame whether or not the GPU count ever
/// reaches it.
///
/// Eight, specifically:
///
/// * It has to clear **two**, or the row closes while running nowhere. The
///   agnostic seam suite's `can_multi_draw` gates both indirect exercises on
///   `max_draw_indirect_count >= RASTER_INDIRECT_DRAWS`, which is 2 — the two
///   argument structures whose *difference* is the only observable either
///   exercise has.
/// * `crcbl_render::forward` asks for **one**. So the engine's own caller is
///   already served by the floor, and everything above it is headroom.
/// * Eight leaves a handful of buckets' worth of that headroom while keeping
///   the worst case — eight no-op draws and eight structures copied — beneath
///   noticing.
///
/// **What would make it wrong to raise.** A caller asking for hundreds pays
/// hundreds of encoder calls and hundreds of draws every frame, however few the
/// GPU actually wants, which is the exact cost `draw_indirect_count` exists to
/// avoid; at that point the answer is a real count-limited execution and not a
/// bigger number here. Nothing in this workspace asks, and
/// `a_count_limited_draw_is_refused_past_the_ceiling` is what a caller that
/// started asking would hit — a refusal, rather than a scratch buffer too small
/// for the draws issued out of it.
pub(crate) const MAX_DRAWS: u32 = 8;

/// **The ceiling has to clear two, and this is what says so at compile time.**
///
/// `crcbl/tests/hal_seam_e2e.rs`'s `can_multi_draw` declines *both* indirect
/// exercises on a device reporting `max_draw_indirect_count` below
/// `RASTER_INDIRECT_DRAWS`, which is 2 — the two argument structures whose
/// difference is the only observable either has. A smaller ceiling here would
/// therefore close [`Capability::DrawIndirectCount`](crcbl_hal::Capability::DrawIndirectCount)
/// *and* stop `IndirectArgumentPaddedStride` being exercised, on every Metal
/// device, with nothing failing to say so.
///
/// A `const` assertion rather than a test, because it is a fact about a
/// constant and a test is something a run can skip. The seam's own constant
/// cannot be imported — it lives in another crate's test binary — so the number
/// is restated here with the reason, which is the one thing that would have to
/// be re-checked if that suite changed it.
const _: () = assert!(
    MAX_DRAWS >= 2,
    "the seam suite's indirect exercises both need two argument structures out of one call"
);

/// Which word of an argument structure the instance count is.
///
/// **The second word of both layouts** —
/// `{ vertexCount, instanceCount, … }` and `{ indexCount, instanceCount, … }` —
/// which is why one kernel serves the indexed and non-indexed forms with
/// nothing but a different `structure_words`. It is passed to the shader rather
/// than written there, so the layout is stated once on the Rust side, beside
/// the structure widths [`crcbl_hal::indirect`] holds.
const INSTANCE_COUNT_WORD: u32 = 1;

/// Invocations per workgroup, matching `[numthreads(64, 1, 1)]` in
/// `shaders/indirect_count_args.slang`.
///
/// One invocation packs one whole argument structure, so the prologue
/// dispatches `max_draw_count.div_ceil(WORKGROUP_SIZE)` groups — which is one,
/// for every bound [`MAX_DRAWS`] permits. It is restated here rather than
/// imported for the reason [`PACK_MSL`] is copied, and
/// `the_uniform_block_is_the_one_the_shader_crate_declares` pins it to
/// `crcbl_shaders::indirect_count_args::WORKGROUP_SIZE`.
pub(crate) const WORKGROUP_SIZE: u32 = 64;

/// The entry point `indirect_count_args.slang` compiles to.
pub(crate) const PACK_ENTRY: &str = "computeMain";

/// The compiled MSL of `crcbl-shaders`' `indirect_count_args.slang`, byte for
/// byte.
///
/// **A copy of a committed artifact, and it is checked against the original.**
/// `crcbl-shaders` is a *dev*-dependency of this crate — the crate docs and
/// `Cargo.toml` both hold the line that nothing this backend ships depends on
/// the engine's shaders — so there is no way to reach the artifact from
/// shipping code, and the alternative to a copy is an `include_str!` reaching
/// across a crate boundary into another package's directory.
///
/// The drift that arrangement invites is closed by
/// `the_embedded_kernel_is_the_committed_artifact` below, which compares this
/// constant with `crcbl_shaders::INDIRECT_COUNT_ARGS.msl()` and fails on any
/// difference — and which runs on **every** host, because this module is in the
/// test build everywhere. Regenerating the shader without updating this
/// constant is a red `cargo test` on Linux, not a wrong picture on a Mac.
///
/// The literal opens flush against the first `#include` so that the constant is
/// the artifact **byte for byte**, newline for newline, rather than the
/// artifact with a leading blank line — which is what makes the comparison an
/// equality instead of a trim.
pub(crate) const PACK_MSL: &str = r#"#include <metal_stdlib>
#include <metal_math>
#include <metal_texture>
using namespace metal;

#line 55 "shaders/indirect_count_args.slang"
struct PackParams_0
{
    uint count_word_0;
    uint first_word_0;
    uint source_stride_words_0;
    uint structure_words_0;
    uint instance_word_0;
    uint max_draw_count_0;
};


#line 126
struct KernelContext_0
{
    PackParams_0 constant* pack_0;
    uint device* count_0;
    uint device* packed_0;
    uint device* source_0;
};


#line 111
[[kernel]] void computeMain(uint3 thread_0 [[thread_position_in_grid]], PackParams_0 constant* pack_1 [[buffer(0)]], uint device* count_1 [[buffer(1)]], uint device* packed_1 [[buffer(3)]], uint device* source_1 [[buffer(2)]])
{

#line 111
    thread KernelContext_0 kernelContext_0;

#line 111
    (&kernelContext_0)->pack_0 = pack_1;

#line 111
    (&kernelContext_0)->count_0 = count_1;

#line 111
    (&kernelContext_0)->packed_0 = packed_1;

#line 111
    (&kernelContext_0)->source_0 = source_1;

    uint structure_0 = thread_0.x;
    if(structure_0 >= (pack_1->max_draw_count_0))
    {
        return;
    }



    uint _S1 = min((&kernelContext_0)->count_0[(&kernelContext_0)->pack_0->count_word_0], pack_1->max_draw_count_0);
    uint _S2 = (&kernelContext_0)->pack_0->first_word_0 + structure_0 * (&kernelContext_0)->pack_0->source_stride_words_0;
    uint to_0 = structure_0 * (&kernelContext_0)->pack_0->structure_words_0;

#line 123
    uint word_0 = 0U;
    for(;;)
    {

#line 124
        if(word_0 < ((&kernelContext_0)->pack_0->structure_words_0))
        {
        }
        else
        {

#line 124
            break;
        }
        *((&kernelContext_0)->packed_0+(to_0 + word_0)) = (&kernelContext_0)->source_0[_S2 + word_0];

#line 124
        word_0 = word_0 + 1U;

#line 124
    }

#line 130
    if(structure_0 >= _S1)
    {
        *((&kernelContext_0)->packed_0+(to_0 + (&kernelContext_0)->pack_0->instance_word_0)) = 0U;

#line 130
    }



    return;
}

"#;

/// Bytes of the kernel's uniform block: six `uint`s, tightly packed.
///
/// Mirrors `crcbl_shaders::indirect_count_args::PARAMS_SIZE`, which
/// `the_uniform_block_is_the_one_the_shader_crate_declares` pins it to. It is
/// restated here rather than imported for the reason [`PACK_MSL`] is copied:
/// the shader crate is a dev-dependency and shipping code cannot see it.
pub(crate) const PARAMS_SIZE: usize = 24;

/// The kernel's uniform block, matching `struct PackParams` in
/// `shaders/indirect_count_args.slang`.
///
/// Every field is a **word** index or a word count. That is what lets both
/// buffers be bound at offset zero: nothing here has to know what offset
/// alignment a `device` buffer binding requires on the device it is running on.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct Params {
    count_word: u32,
    first_word: u32,
    source_stride_words: u32,
    structure_words: u32,
    instance_word: u32,
    max_draw_count: u32,
}

impl Params {
    /// The block as the bytes `setBytes:length:atIndex:` sends, little-endian
    /// and in declaration order.
    fn to_bytes(self) -> [u8; PARAMS_SIZE] {
        let mut bytes = [0u8; PARAMS_SIZE];
        for (slot, value) in [
            self.count_word,
            self.first_word,
            self.source_stride_words,
            self.structure_words,
            self.instance_word,
            self.max_draw_count,
        ]
        .into_iter()
        .enumerate()
        {
            let at = slot * 4;
            bytes[at..at + 4].copy_from_slice(&value.to_le_bytes());
        }
        bytes
    }
}

/// A validated count-limited draw: what the kernel is told, how big the packed
/// buffer is, and how many draws come out of it.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct CountPlan {
    /// The uniform block, already packed, for `setBytes:length:atIndex:`.
    pub(crate) params: [u8; PARAMS_SIZE],
    /// Bytes the packed buffer must hold: one structure per issued draw, with
    /// nothing between them.
    pub(crate) packed_bytes: u64,
    /// Bytes of one argument structure, which is also the packed stride.
    pub(crate) structure_bytes: u64,
    /// Draws the pass issues, which is `max_draw_count` whatever the count
    /// buffer ends up holding — the ones past the count draw no instances.
    pub(crate) draws: u32,
    /// Threadgroups the prologue dispatches.
    pub(crate) groups: u32,
}

impl CountPlan {
    /// The byte offset of packed structure `index`.
    pub(crate) const fn offset(self, index: u32) -> u64 {
        self.structure_bytes * index as u64
    }
}

/// Checks a count-limited draw against Metal's rules, this backend's ceiling
/// and the two buffers it reads — or says why it cannot be encoded.
///
/// `None` for a draw of nothing, which is not an error and is the same answer
/// [`plan_indirect`](crate::draw::plan_indirect) gives a `draw_count` of zero:
/// there is no structure to pack and no draw to issue, so the prologue is not
/// recorded either.
///
/// Pure, so every rule below has a unit test on any host — which matters
/// because none of them can be observed from a draw's *output*. An
/// out-of-range indirect offset raises, and a raise from Objective-C aborts the
/// process rather than returning something a caller could report.
///
/// # Errors
///
/// [`HalError::InvalidDescriptor`] when `max_draw_count` is past
/// [`MAX_DRAWS`], when either offset is not four-byte aligned, when the count
/// does not fit inside the count buffer, when a multi-draw's stride is smaller
/// than one argument structure or is not itself four-byte aligned, or when the
/// structures do not fit inside the argument buffer.
pub(crate) fn plan_indirect_count(
    draw: &DrawIndirectCount,
    indexed: bool,
    args_length: u64,
    count_length: u64,
) -> Result<Option<CountPlan>, HalError> {
    if draw.max_draw_count == 0 {
        return Ok(None);
    }
    if draw.max_draw_count > MAX_DRAWS {
        return Err(HalError::InvalidDescriptor(format!(
            "a count-limited draw of up to {} structures exceeds this backend's \
             max_draw_indirect_count of {MAX_DRAWS}; the pass issues max_draw_count draws and \
             packs that many argument structures, so the bound is what it is willing to spend \
             rather than a ceiling it can stretch",
            draw.max_draw_count
        )));
    }
    let structure = structure_bytes(indexed);
    for (what, offset) in [("argument", draw.args_offset), ("count", draw.count_offset)] {
        if !offset.is_multiple_of(INDIRECT_OFFSET_ALIGNMENT) {
            return Err(HalError::InvalidDescriptor(format!(
                "a count-limited draw's {what} offset {offset} is not a multiple of \
                 {INDIRECT_OFFSET_ALIGNMENT}"
            )));
        }
    }
    // The count is one `u32`, and the kernel reads it by word index — so a
    // count sitting past the end of its buffer is a read of somebody else's
    // memory rather than a Metal-side refusal.
    if draw
        .count_offset
        .checked_add(4)
        .is_none_or(|end| end > count_length)
    {
        return Err(HalError::InvalidDescriptor(format!(
            "a count-limited draw reads its u32 count at offset {} of a {count_length}-byte \
             buffer",
            draw.count_offset
        )));
    }
    // As `plan_indirect`: a single structure is never strided over, so a
    // one-draw caller's `stride: 0` is not a value to refuse.
    let stride = if draw.max_draw_count == 1 {
        structure
    } else {
        let stride = u64::from(draw.stride);
        if stride < structure || !stride.is_multiple_of(INDIRECT_OFFSET_ALIGNMENT) {
            return Err(HalError::InvalidDescriptor(format!(
                "a count-limited draw of up to {} structures has a stride of {stride}, and one \
                 argument structure is {structure} bytes on a \
                 {INDIRECT_OFFSET_ALIGNMENT}-byte alignment",
                draw.max_draw_count
            )));
        }
        stride
    };
    // Every structure the *kernel* reads, which is every structure the pass
    // could issue — not only the ones this frame's count will reach, because
    // the count is not a value anybody here can see.
    let span = u64::from(draw.max_draw_count - 1)
        .checked_mul(stride)
        .and_then(|span| span.checked_add(structure))
        .and_then(|span| draw.args_offset.checked_add(span));
    if span.is_none_or(|span| span > args_length) {
        return Err(HalError::InvalidDescriptor(format!(
            "a count-limited draw of up to {} structures {stride} bytes apart from offset {} runs \
             past a {args_length}-byte buffer",
            draw.max_draw_count, draw.args_offset
        )));
    }
    let params = Params {
        count_word: (draw.count_offset / 4) as u32,
        first_word: (draw.args_offset / 4) as u32,
        source_stride_words: (stride / 4) as u32,
        structure_words: (structure / 4) as u32,
        instance_word: INSTANCE_COUNT_WORD,
        max_draw_count: draw.max_draw_count,
    };
    Ok(Some(CountPlan {
        params: params.to_bytes(),
        packed_bytes: structure * u64::from(draw.max_draw_count),
        structure_bytes: structure,
        draws: draw.max_draw_count,
        groups: draw.max_draw_count.div_ceil(WORKGROUP_SIZE),
    }))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crcbl_core::Handle;

    /// A handle no device issued — every function here is pure and never
    /// resolves it.
    fn buffer() -> crcbl_hal::BufferHandle {
        Handle::from_bits(1 << 32).expect("generation 1 is non-zero")
    }

    fn draw(
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

    /// Reads one `u32` field of a packed uniform block.
    fn word(params: &[u8; PARAMS_SIZE], slot: usize) -> u32 {
        u32::from_le_bytes(params[slot * 4..slot * 4 + 4].try_into().expect("4 bytes"))
    }

    /// **The kernel gets word indices, and they are the caller's byte offsets
    /// divided by four.**
    ///
    /// This is the whole translation the module performs, and the shape it is
    /// wrong in is silent: a byte offset passed where a word index belongs
    /// reads four times too far into the argument buffer and packs whatever is
    /// there, which draws a wrong picture with a clean log.
    ///
    /// **What turns it red.** Dropping any `/ 4`; passing the *packed* stride
    /// as the source stride, which the padded-stride case below separates;
    /// hard-coding the instance word to zero, which would zero a vertex count
    /// and leave the instance count alone.
    #[test]
    fn a_count_limited_draw_is_planned_as_word_indices_into_both_buffers() {
        // The agnostic seam suite's own numbers: two structures at offset 32,
        // a tight stride, and the count one word into its buffer.
        let plan = plan_indirect_count(&draw(32, 4, 2, 16), false, 64, 8)
            .expect("in range")
            .expect("two draws");
        assert_eq!(word(&plan.params, 0), 1, "count_word is count_offset / 4");
        assert_eq!(word(&plan.params, 1), 8, "first_word is args_offset / 4");
        assert_eq!(
            word(&plan.params, 2),
            4,
            "source_stride_words is stride / 4"
        );
        assert_eq!(word(&plan.params, 3), 4, "a draw structure is four words");
        assert_eq!(word(&plan.params, 4), 1, "instanceCount is the second word");
        assert_eq!(word(&plan.params, 5), 2, "max_draw_count travels verbatim");
        assert_eq!(plan.draws, 2);
        assert_eq!(plan.groups, 1);
        assert_eq!(plan.structure_bytes, 16);
        assert_eq!(plan.packed_bytes, 32, "two structures, packed tight");
        assert_eq!(plan.offset(0), 0);
        assert_eq!(plan.offset(1), 16, "the packed copy strides by a structure");
    }

    /// An indexed draw's structure is five words, and the instance count is
    /// still the second of them.
    ///
    /// **What turns it red.** Using one structure size for both forms, which
    /// packs four words of a five-word structure and leaves the fifth —
    /// `baseInstance` — holding whatever the scratch buffer had.
    #[test]
    fn an_indexed_count_limited_draw_packs_five_word_structures() {
        let plan = plan_indirect_count(&draw(0, 0, 2, 20), true, 40, 4)
            .expect("in range")
            .expect("two draws");
        assert_eq!(word(&plan.params, 3), 5);
        assert_eq!(word(&plan.params, 4), 1);
        assert_eq!(plan.structure_bytes, 20);
        assert_eq!(plan.packed_bytes, 40);
        assert_eq!(plan.offset(1), 20);
    }

    /// **A padded stride is honoured on the read and dropped on the write.**
    ///
    /// That asymmetry is the point of packing: the caller's structures may be
    /// any multiple of four bytes apart, and the scratch allocation is the
    /// number of structures rather than the span they cover.
    #[test]
    fn a_padded_stride_widens_the_read_and_not_the_packed_copy() {
        let plan = plan_indirect_count(&draw(0, 0, 2, 32), false, 48, 4)
            .expect("in range")
            .expect("two draws");
        assert_eq!(word(&plan.params, 2), 8, "the read strides by 32 bytes");
        assert_eq!(plan.structure_bytes, 16, "the write strides by a structure");
        assert_eq!(plan.packed_bytes, 32);
    }

    /// A draw of nothing is not an error, and records no prologue either.
    #[test]
    fn a_count_limited_draw_of_no_structures_is_a_no_op() {
        assert_eq!(
            plan_indirect_count(&draw(0, 0, 0, 16), false, 0, 0).expect("not an error"),
            None
        );
    }

    /// **Past the ceiling is a refusal, not a smaller scratch buffer.**
    ///
    /// [`MAX_DRAWS`] is a promise the seam makes through
    /// `Limits::max_draw_indirect_count`, and the buffer the draws read is
    /// sized from it — so a caller that ignored the limit would otherwise get
    /// draws reading past the end of the packed copy, which Metal does not
    /// bounds-check.
    ///
    /// **What turns it red.** Deleting the ceiling check; the assertion below
    /// that `MAX_DRAWS` itself is accepted is what stops the check being
    /// widened into one that refuses everything.
    #[test]
    fn a_count_limited_draw_is_refused_past_the_ceiling() {
        let bytes = 16 * u64::from(MAX_DRAWS) + 16;
        let error = plan_indirect_count(&draw(0, 0, MAX_DRAWS + 1, 16), false, bytes, 4)
            .expect_err("one past the ceiling");
        assert!(matches!(error, HalError::InvalidDescriptor(_)), "{error:?}");
        let plan = plan_indirect_count(&draw(0, 0, MAX_DRAWS, 16), false, bytes, 4)
            .expect("the ceiling itself is in range")
            .expect("draws");
        assert_eq!(plan.draws, MAX_DRAWS);
        assert_eq!(plan.groups, 1, "one workgroup covers every bound in range");
    }

    /// Both offsets are checked for Metal's four-byte alignment, and the count
    /// is checked against its own buffer.
    ///
    /// **What turns it red.** Dropping either alignment check — the first two
    /// assertions use offsets Metal's headers forbid and which would become
    /// word indices with the remainder silently lost. Dropping the count bound
    /// — the third reads a `u32` whose last byte is past the end.
    #[test]
    fn a_count_limited_draw_checks_both_offsets_and_the_count_buffer() {
        for (what, unaligned) in [
            ("argument", draw(2, 0, 1, 16)),
            ("count", draw(0, 2, 1, 16)),
        ] {
            let error = plan_indirect_count(&unaligned, false, 64, 8)
                .expect_err("an offset Metal's headers forbid");
            assert!(
                matches!(error, HalError::InvalidDescriptor(_)),
                "{what}: {error:?}"
            );
        }
        // The last byte of the count is one past the end of a four-byte
        // buffer, and the kernel reads it by word index — so nothing under this
        // would refuse it.
        let error = plan_indirect_count(&draw(0, 4, 1, 16), false, 64, 4)
            .expect_err("the count runs past its buffer");
        assert!(matches!(error, HalError::InvalidDescriptor(_)), "{error:?}");
        assert!(
            plan_indirect_count(&draw(0, 4, 1, 16), false, 64, 8)
                .expect("a count that fits")
                .is_some()
        );
    }

    /// A stride narrower than a structure, and a stride that is not aligned,
    /// are both refused — while a one-draw caller's `stride: 0` is not.
    ///
    /// **What turns it red.** Dropping the stride check makes the first two
    /// assertions plans; refusing `stride: 0` outright makes the third an
    /// error, which would break every caller that passes one structure and no
    /// stride at all.
    #[test]
    fn a_count_limited_draw_checks_the_stride_only_when_it_strides() {
        for stride in [12, 18] {
            let error = plan_indirect_count(&draw(0, 0, 2, stride), false, 256, 4)
                .expect_err("not a legal stride for a 16-byte structure");
            assert!(matches!(error, HalError::InvalidDescriptor(_)), "{error:?}");
        }
        let plan = plan_indirect_count(&draw(0, 0, 1, 0), false, 16, 4)
            .expect("a single structure is never strided over")
            .expect("one draw");
        assert_eq!(
            word(&plan.params, 2),
            4,
            "the stride falls back to a structure"
        );
    }

    /// The structures the kernel reads must fit inside the argument buffer.
    ///
    /// **What turns it red.** Dropping the span check; the accepted case beside
    /// it is what stops the check being an off-by-one that refuses an exact
    /// fit.
    #[test]
    fn a_count_limited_draw_must_fit_the_argument_buffer_it_reads() {
        let error = plan_indirect_count(&draw(0, 0, 2, 16), false, 31, 4)
            .expect_err("two 16-byte structures do not fit 31 bytes");
        assert!(matches!(error, HalError::InvalidDescriptor(_)), "{error:?}");
        assert!(
            plan_indirect_count(&draw(0, 0, 2, 16), false, 32, 4)
                .expect("an exact fit")
                .is_some()
        );
    }

    /// **The embedded kernel is the committed artifact, byte for byte.**
    ///
    /// [`PACK_MSL`] is a copy, so this is the only thing standing between a
    /// regenerated shader and a backend still compiling the old one. It runs on
    /// every host — this module is in the test build everywhere — so the drift
    /// is caught by `cargo test` on Linux rather than by a picture on a Mac.
    ///
    /// **What turns it red.** Editing `shaders/indirect_count_args.slang` and
    /// regenerating without pasting the new MSL in here, which is exactly the
    /// mistake it exists for.
    #[test]
    fn the_embedded_kernel_is_the_committed_artifact() {
        let artifact = crcbl_shaders::INDIRECT_COUNT_ARGS
            .msl()
            .expect("indirect_count_args.slang declares the msl target");
        assert_eq!(
            PACK_MSL, artifact,
            "the MSL embedded in crcbl-mtl is not the committed artifact; paste \
             crates/crcbl-shaders/msl/indirect_count_args.metal into PACK_MSL"
        );
        assert!(
            artifact.contains(&format!("void {PACK_ENTRY}(")),
            "the kernel does not declare `{PACK_ENTRY}`, which is the name \
             newFunctionWithName: asks Metal for"
        );
    }

    /// **The uniform block this module packs is the one the shader crate
    /// declares.**
    ///
    /// Two crates describe the same six words — this one because shipping code
    /// cannot see `crcbl-shaders`, and that one because it owns the source. A
    /// disagreement is a kernel reading the wrong field out of a block that is
    /// the right length, which produces a plausible picture and no error.
    ///
    /// **What turns it red.** Reordering either `to_bytes`, changing either
    /// `PARAMS_SIZE`, or changing one workgroup size without the other.
    #[test]
    fn the_uniform_block_is_the_one_the_shader_crate_declares() {
        assert_eq!(PARAMS_SIZE, crcbl_shaders::indirect_count_args::PARAMS_SIZE);
        assert_eq!(
            WORKGROUP_SIZE,
            crcbl_shaders::indirect_count_args::WORKGROUP_SIZE
        );
        let mine = Params {
            count_word: 1,
            first_word: 8,
            source_stride_words: 4,
            structure_words: 5,
            instance_word: 1,
            max_draw_count: 2,
        };
        let theirs = crcbl_shaders::indirect_count_args::Params {
            count_word: 1,
            first_word: 8,
            source_stride_words: 4,
            structure_words: 5,
            instance_word: 1,
            max_draw_count: 2,
        };
        assert_eq!(mine.to_bytes().as_slice(), theirs.to_bytes().as_slice());
    }
}
