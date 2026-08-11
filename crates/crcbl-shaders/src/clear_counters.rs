//! The workgroup size and uniform block `clear_counters.slang` declares, in the
//! layouts that shader declares.
//!
//! Same reason as [`crate::cull`]: the shader fixes a number and a byte layout,
//! every producer of those has to agree with it exactly, and keeping both in the
//! crate that owns the source means there is one place to change rather than one
//! per consumer.
//!
//! What the shader zeroes is every counter [`crate::cull`]'s and
//! [`crate::draw_gen`]'s passes — and `mesh_cluster.slang`'s amplification
//! stage — only ever *add* to. It is a dispatch rather than
//! a [`CommandEncoder::fill_buffer`] because a fill is legal only outside a
//! pass, where a render-graph frame has no room, and because `crcbl-dx12`
//! refuses one outright — the shader's own header has the whole argument.
//!
//! [`CommandEncoder::fill_buffer`]: https://docs.rs/crcbl-hal

/// Invocations per workgroup, matching `[numthreads(64, 1, 1)]` in
/// `shaders/clear_counters.slang`.
///
/// One invocation covers the same index in every buffer the pass zeroes, so a
/// caller dispatches `args_words.div_ceil(WORKGROUP_SIZE)` groups — the longest
/// of them.
pub const WORKGROUP_SIZE: u32 = 64;

/// Bytes of the uniform block.
///
/// One `uint` per buffer the pass zeroes, which happens to be exactly the
/// multiple of 16 bytes `std140` requires of a uniform block's size — so there
/// is no tail padding left to write. Checked against the `Offset` decorations
/// `slangc` emits by this module's
/// `the_clear_counters_params_block_matches_the_offsets_slangc_emits`.
pub const PARAMS_SIZE: usize = 16;

/// The uniform block, matching `struct ClearParams` in
/// `shaders/clear_counters.slang`.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Params {
    /// Words in the indirect-argument buffer —
    /// [`crate::draw_gen::DRAW_ARGS_WORDS`] per bucket. Computed by the caller
    /// from that constant so the clearing shader does not re-declare the
    /// argument layout.
    pub args_words: u32,
    /// Words in the per-bucket draw-count buffer, which is one per bucket.
    pub counts_words: u32,
    /// Words in the culling-statistics buffer, which is
    /// [`crate::cull::STATS_WORDS`].
    ///
    /// Passed rather than assumed because two shaders own one word of that
    /// buffer each: a clearing pass that zeroed only the first would leave the
    /// second counter accumulating across frames, which reads as a plausible
    /// number rather than as a failure.
    pub stats_words: u32,
    /// Words in the mesh-dispatch argument buffer —
    /// [`crate::draw_gen::MESH_ARGS_WORDS`] per bucket.
    ///
    /// Zeroed for [`args_words`](Self::args_words)' reason: the y extent of a
    /// bucket's mesh dispatch is an atomic add, so one carrying the previous
    /// frame's total launches workgroups for instances this frame culled.
    pub mesh_args_words: u32,
}

impl Params {
    /// The block as the bytes a uniform buffer holds, in `std140` order.
    #[must_use]
    pub fn to_bytes(&self) -> [u8; PARAMS_SIZE] {
        let mut bytes = [0u8; PARAMS_SIZE];
        for (slot, value) in [
            self.args_words,
            self.counts_words,
            self.stats_words,
            self.mesh_args_words,
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
    /// A mismatch is the quietest failure this pass has: a dispatch sized
    /// against the wrong number zeroes a *prefix* of the arguments, so the first
    /// buckets look right and a later one carries the previous frame's instance
    /// count.
    #[test]
    fn the_workgroup_size_matches_the_numthreads_clear_counters_slang_declares() {
        let source = include_str!("../shaders/clear_counters.slang");
        let declaration = format!("[numthreads({WORKGROUP_SIZE}, 1, 1)]");
        assert!(
            source.contains(&declaration),
            "clear_counters.slang does not declare `{declaration}`; WORKGROUP_SIZE has drifted \
             from the shader"
        );
    }

    /// The offsets `slangc` actually emitted for `ClearParams`, read out of the
    /// disassembly.
    #[test]
    fn the_clear_counters_params_block_matches_the_offsets_slangc_emits() {
        // `OpMemberDecorate %ClearParams_std140 n Offset …`: 0, 4, 8, 12.
        assert_eq!(PARAMS_SIZE, 16);
        assert_eq!(
            PARAMS_SIZE % 16,
            0,
            "std140 rounds a uniform block's size up to a multiple of 16, so a block that is not \
             one already is a block the shader and the CPU disagree about the width of"
        );
        let bytes = Params {
            args_words: 10,
            counts_words: 2,
            stats_words: 5,
            mesh_args_words: 6,
        }
        .to_bytes();
        let uint_at =
            |offset: usize| u32::from_le_bytes(bytes[offset..offset + 4].try_into().expect("4"));
        assert_eq!(uint_at(0), 10, "args_words at offset 0");
        assert_eq!(uint_at(4), 2, "counts_words at offset 4");
        assert_eq!(uint_at(8), 5, "stats_words at offset 8");
        assert_eq!(uint_at(12), 6, "mesh_args_words at offset 12");
    }
}
