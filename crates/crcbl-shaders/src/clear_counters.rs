//! The workgroup size and uniform block `clear_counters.slang` declares, in the
//! layouts that shader declares.
//!
//! Same reason as [`crate::cull`]: the shader fixes a number and a byte layout,
//! every producer of those has to agree with it exactly, and keeping both in the
//! crate that owns the source means there is one place to change rather than one
//! per consumer.
//!
//! What the shader zeroes is the three counters [`crate::cull`]'s and
//! [`crate::draw_gen`]'s passes only ever *add* to. It is a dispatch rather than
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
/// of the three.
pub const WORKGROUP_SIZE: u32 = 64;

/// Bytes of the uniform block.
///
/// Two `uint` and the tail padding `std140` requires of a uniform block's size.
/// Checked against the `Offset` decorations `slangc` emits by this module's
/// `the_params_block_matches_the_offsets_slangc_emits`.
pub const PARAMS_SIZE: usize = 16;

/// The uniform block, matching `struct ClearParams` in
/// `shaders/clear_counters.slang`.
///
/// The survivor count is not here: it is one word by construction, and the
/// shader zeroes element 0 without being told.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Params {
    /// Words in the indirect-argument buffer —
    /// [`crate::draw_gen::DRAW_ARGS_WORDS`] per bucket. Computed by the caller
    /// from that constant so the clearing shader does not re-declare the
    /// argument layout.
    pub args_words: u32,
    /// Words in the per-bucket draw-count buffer, which is one per bucket.
    pub counts_words: u32,
}

impl Params {
    /// The block as the bytes a uniform buffer holds, in `std140` order.
    ///
    /// The tail padding is written rather than left alone, for the reason
    /// [`crate::compute_probe::Params::to_bytes`] gives: a buffer allocated for
    /// this block is [`PARAMS_SIZE`] bytes wide and a partial write leaves the
    /// rest undefined.
    #[must_use]
    pub fn to_bytes(&self) -> [u8; PARAMS_SIZE] {
        let mut bytes = [0u8; PARAMS_SIZE];
        for (slot, value) in [self.args_words, self.counts_words].into_iter().enumerate() {
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
    fn the_workgroup_size_matches_the_numthreads_the_shader_declares() {
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
    fn the_params_block_matches_the_offsets_slangc_emits() {
        // `OpMemberDecorate %ClearParams_std140 n Offset …`: 0, 4.
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
        }
        .to_bytes();
        let uint_at =
            |offset: usize| u32::from_le_bytes(bytes[offset..offset + 4].try_into().expect("4"));
        assert_eq!(uint_at(0), 10, "args_words at offset 0");
        assert_eq!(uint_at(4), 2, "counts_words at offset 4");
        assert!(
            bytes[8..].iter().all(|byte| *byte == 0),
            "the std140 tail padding is written, and it is zero: {:?}",
            &bytes[8..]
        );
    }
}
