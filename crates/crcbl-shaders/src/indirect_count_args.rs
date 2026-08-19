//! The workgroup size and uniform block `indirect_count_args.slang` declares,
//! in the layout that shader declares.
//!
//! Same reason as [`crate::clear_counters`]: the shader fixes a number and a
//! byte layout, every producer of those has to agree with it exactly, and
//! keeping both in the crate that owns the source means there is one place to
//! change rather than one per consumer.
//!
//! What the shader does is pack the argument structures a *count-limited*
//! multi-draw will issue, zeroing the instance count of every structure at or
//! past the GPU-written draw count — so that the draws past the count render
//! nothing. It exists because Metal has no count-buffer draw at all; the
//! shader's own header carries the argument, and `crcbl_mtl::indirect_count` is
//! the backend that dispatches it.

/// Invocations per workgroup, matching `[numthreads(64, 1, 1)]` in
/// `shaders/indirect_count_args.slang`.
///
/// One invocation packs one whole argument structure, so a caller dispatches
/// `max_draw_count.div_ceil(WORKGROUP_SIZE)` groups.
pub const WORKGROUP_SIZE: u32 = 64;

/// Bytes of the uniform block: six `uint`s, tightly packed.
///
/// **Not rounded up to a multiple of 16**, unlike
/// [`crate::clear_counters::PARAMS_SIZE`], and the difference is the target
/// list. `std140` rounds the size of a block a *uniform buffer* holds, and this
/// block is never in one: the only artifact anybody loads is the MSL, where the
/// struct is tightly packed at 24 bytes and the backend sends it inline with
/// `setBytes:length:atIndex:` — a call that takes the length rather than
/// declaring a buffer's size. The member offsets are the same either way, which
/// `the_pack_params_block_matches_the_offsets_slangc_emits` pins against the
/// decorations `slangc` actually emitted.
pub const PARAMS_SIZE: usize = 24;

/// The uniform block, matching `struct PackParams` in
/// `shaders/indirect_count_args.slang`.
///
/// Every field is a **word** index or a word count, never a byte offset. That
/// is deliberate: it lets both buffers be bound at offset zero, so no caller
/// has to know what offset alignment a `device` buffer binding requires on the
/// device it happens to be running on.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Params {
    /// Index of the `u32` draw count within the count buffer.
    pub count_word: u32,
    /// Index of the first source structure's first word within the caller's
    /// argument buffer.
    pub first_word: u32,
    /// Words between consecutive source structures — the caller's stride, which
    /// may be wider than a structure. The read strides by it; the write does
    /// not, so the packed copy is always tight.
    pub source_stride_words: u32,
    /// Words in one argument structure, which is also the packed stride: four
    /// for a non-indexed draw and five for an indexed one.
    pub structure_words: u32,
    /// Index of the instance count within a structure. Zeroing it is what makes
    /// a draw past the count a no-op.
    pub instance_word: u32,
    /// Structures the pass will issue. The draw count is clamped to it, which
    /// is what `vkCmdDrawIndirectCount` does with the same two numbers.
    pub max_draw_count: u32,
}

impl Params {
    /// The block as the bytes the kernel reads, little-endian and in
    /// declaration order.
    #[must_use]
    pub fn to_bytes(&self) -> [u8; PARAMS_SIZE] {
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

#[cfg(test)]
mod tests {
    use super::*;

    /// The constant and the shader must name the same workgroup size.
    ///
    /// A dispatch sized against the wrong number packs a *prefix* of the
    /// structures and leaves the rest holding whatever the scratch buffer held
    /// before — which is a draw reading arguments nobody wrote.
    #[test]
    fn the_workgroup_size_matches_the_numthreads_indirect_count_args_slang_declares() {
        let source = include_str!("../shaders/indirect_count_args.slang");
        let declaration = format!("[numthreads({WORKGROUP_SIZE}, 1, 1)]");
        assert!(
            source.contains(&declaration),
            "indirect_count_args.slang does not declare `{declaration}`; WORKGROUP_SIZE has \
             drifted from the shader"
        );
    }

    /// The offsets `slangc` actually emitted for `PackParams`, read out of the
    /// disassembly.
    ///
    /// **What turns it red.** Reordering the fields of [`Params::to_bytes`] —
    /// every assertion below names a different value, so any transposition
    /// lands on at least two of them.
    #[test]
    fn the_pack_params_block_matches_the_offsets_slangc_emits() {
        // `OpMemberDecorate %PackParams_std140 n Offset …`: 0, 4, 8, 12, 16, 20.
        assert_eq!(PARAMS_SIZE, 24);
        let bytes = Params {
            count_word: 1,
            first_word: 8,
            source_stride_words: 4,
            structure_words: 5,
            instance_word: 1,
            max_draw_count: 2,
        }
        .to_bytes();
        let uint_at =
            |offset: usize| u32::from_le_bytes(bytes[offset..offset + 4].try_into().expect("4"));
        assert_eq!(uint_at(0), 1, "count_word at offset 0");
        assert_eq!(uint_at(4), 8, "first_word at offset 4");
        assert_eq!(uint_at(8), 4, "source_stride_words at offset 8");
        assert_eq!(uint_at(12), 5, "structure_words at offset 12");
        assert_eq!(uint_at(16), 1, "instance_word at offset 16");
        assert_eq!(uint_at(20), 2, "max_draw_count at offset 20");
    }
}
