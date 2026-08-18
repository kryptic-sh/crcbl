//! The workgroup size, push-constant block and output length
//! `push_constant_probe.slang` declares — and the slot each committed artifact
//! actually puts the block at.
//!
//! Same reason as [`crate::compute_probe`] for the first three: the shader
//! fixes a number and a byte layout, every producer of those has to agree with
//! it exactly, and keeping both in the crate that owns the source means one
//! place to change rather than one per consumer.
//!
//! # Where the block lands, per target
//!
//! This is the part no other compute module needs, and the reason this one
//! exists: `crcbl_hal::PipelineLayoutDesc::push_constants` is a single
//! `PushConstantRange`, and each backend has to turn it into something its API
//! names. Read out of the committed artifacts rather than assumed:
//!
//! * **SPIR-V** (`spirv/push_constant_probe.spv`) — `%constants` is an
//!   `OpVariable` in the `PushConstant` storage class, its block's single
//!   member decorated `Offset 0`. There is no descriptor set or binding number,
//!   which is the whole point of the storage class: `vkCmdPushConstants`
//!   addresses it by offset within the layout's range. `destination` is set 0,
//!   binding 0.
//! * **MSL** (`msl/push_constant_probe.metal`) — Metal has no push constants,
//!   so Slang lowers the block to an ordinary buffer argument: `constants` is
//!   `[[buffer(1)]]` and `destination` is `[[buffer(0)]]`. The order is a
//!   consequence of the source declaring the push constant **last**, which
//!   `crate::declaration_order` requires precisely so that the numbered
//!   bindings keep the indices `crcbl-mtl` computes for them. A backend feeds
//!   the block with `setBytes:length:atIndex:` at the index past the last
//!   binding.
//! * **DXIL** (`dxil/push_constant_probe.computeMain.dxil`) — HLSL has no push
//!   constants either; Slang emits a `cbuffer` and `dxc` binds it at **`cb0`**
//!   (register `b0`, space 0), sixteen bytes, with `destination` at `u0`. A
//!   D3D12 root signature therefore carries a root-constants entry of
//!   [`CONSTANTS_SIZE`](crate::push_constant_probe::CONSTANTS_SIZE) / 4 32-bit
//!   values at shader register `b0`.
//!
//! Both of the two backends that must translate the range put it in a slot
//! **numbered independently of the bind groups** — Metal's buffer table shares
//! its numbering with the bindings and DXIL's `b` space does not — which is
//! what makes this shader's answer worth recording rather than re-deriving.

/// Invocations per workgroup, matching `[numthreads(64, 1, 1)]` in
/// `shaders/push_constant_probe.slang`.
///
/// A caller dispatches exactly one group, `(1, 1, 1)`: the workgroup is wider
/// than the output on purpose, so the shader's bounds check is a branch the
/// intended dispatch really takes.
pub const WORKGROUP_SIZE: u32 = 64;

/// Words the shader writes, which is the length of [`Constants::values`] and
/// the number of `u32` elements the destination storage buffer needs.
pub const WORD_COUNT: usize = 4;

/// Bytes of the push-constant block — what
/// `crcbl_hal::PushConstantRange::size` takes.
///
/// The range starts at offset 0: the SPIR-V decorates the block's only member
/// `Offset 0`, and there is nothing in front of it on any target.
pub const CONSTANTS_SIZE: u32 = 16;

// One `uint4`, and the buffer it is copied into is the same length in bytes.
// Both consumers index by word, so a block that grew without the output growing
// with it would silently stop being fully observable.
const _: () = assert!(CONSTANTS_SIZE as usize == WORD_COUNT * size_of::<u32>());

/// The push-constant block, matching `struct ProbeConstants` in
/// `shaders/push_constant_probe.slang`.
///
/// `values[i]` is what the shader writes to element `i` of its destination
/// buffer, so a readback compared against these words is the observation that
/// the constants arrived — and arrived at the right offsets, which a checksum
/// of the block would not distinguish.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Constants {
    /// The words pushed, in order.
    pub values: [u32; WORD_COUNT],
}

impl Constants {
    /// The block as the bytes `crcbl_hal::CommandEncoder::push_constants`
    /// takes.
    ///
    /// Little-endian, tightly packed, no padding: a `uint4` is sixteen bytes on
    /// every target this shader is compiled for, which is why the source spells
    /// the block as a vector rather than an array.
    #[must_use]
    pub fn to_bytes(self) -> [u8; CONSTANTS_SIZE as usize] {
        let mut bytes = [0u8; CONSTANTS_SIZE as usize];
        for (word, chunk) in self.values.iter().zip(bytes.chunks_exact_mut(4)) {
            chunk.copy_from_slice(&word.to_le_bytes());
        }
        bytes
    }
}

/// `OpVariable`s in the `PushConstant` storage class, counted by walking the
/// module's instruction stream.
///
/// A scan for the bare storage-class number would match any operand that
/// happened to equal it — which is a check that cannot fail — so the words are
/// stepped through by length instead: five header words, then each instruction
/// as `(word_count << 16) | opcode` followed by its operands.
///
/// Test-only and shared with [`crate::push_constant_raster`], which asks the
/// same question of the raster artifact. It lives here because this is the
/// module whose test also runs it over a shader that has *no* push constant,
/// which is what shows the walk is finding the storage class rather than a
/// coincidence.
#[cfg(test)]
pub(crate) fn push_constant_variables(words: &[u32]) -> usize {
    /// `OpVariable`, whose third operand word is the storage class.
    const OP_VARIABLE: u32 = 59;
    /// `StorageClass PushConstant`.
    const PUSH_CONSTANT: u32 = 9;
    /// Words of a SPIR-V module header, before the first instruction.
    const HEADER_WORDS: usize = 5;
    /// Where the storage class sits within an `OpVariable`, after the opcode
    /// word, the result type and the result id.
    const STORAGE_CLASS_WORD: usize = 3;

    let mut found = 0;
    let mut cursor = HEADER_WORDS;
    while cursor < words.len() {
        let length = (words[cursor] >> 16) as usize;
        assert!(length > 0, "a zero-length instruction never terminates");
        assert!(
            cursor + length <= words.len(),
            "instruction at word {cursor} runs past the end of the module"
        );
        if words[cursor] & 0xffff == OP_VARIABLE
            && length > STORAGE_CLASS_WORD
            && words[cursor + STORAGE_CLASS_WORD] == PUSH_CONSTANT
        {
            found += 1;
        }
        cursor += length;
    }
    found
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The constant and the shader must name the same workgroup size, for the
    /// reason [`crate::compute_probe`]'s twin of this test gives: a mismatch
    /// compiles, dispatches, and shows up only as a partially written buffer.
    #[test]
    fn the_workgroup_size_matches_the_numthreads_the_source_declares() {
        let source = include_str!("../shaders/push_constant_probe.slang");
        let declaration = format!("[numthreads({WORKGROUP_SIZE}, 1, 1)]");
        assert!(
            source.contains(&declaration),
            "push_constant_probe.slang does not declare `{declaration}`; WORKGROUP_SIZE has \
             drifted from the shader"
        );
    }

    /// The word count is the shader's own bound, and the shader writes past the
    /// buffer if the two disagree.
    #[test]
    fn the_word_count_matches_the_bound_the_source_declares() {
        let source = include_str!("../shaders/push_constant_probe.slang");
        let declaration = format!("static const uint WORD_COUNT = {WORD_COUNT};");
        assert!(
            source.contains(&declaration),
            "push_constant_probe.slang does not declare `{declaration}`; WORD_COUNT has drifted \
             from the shader"
        );
    }

    /// The shader must read a push constant and not a bound block, or this
    /// whole module documents a shader that proves nothing.
    ///
    /// The claim is checkable in one line and is the reason the source declares
    /// no `wgsl` target, so it is checked rather than asserted in prose.
    #[test]
    fn the_source_declares_a_push_constant_and_no_wgsl_target() {
        let source = include_str!("../shaders/push_constant_probe.slang");
        assert!(
            source.contains("[[vk::push_constant]]\nConstantBuffer<ProbeConstants> constants;"),
            "push_constant_probe.slang no longer declares its block as a push constant"
        );
        assert!(
            source.contains("// crcbl-targets: spirv, msl, dxil\n"),
            "push_constant_probe.slang's target declaration has changed; WGSL cannot carry a \
             push constant and naga refuses the artifact Slang emits for one"
        );
    }

    /// The byte layout, checked rather than asserted in prose: four
    /// little-endian words, tightly packed, in declaration order.
    #[test]
    fn the_block_is_four_le_u32s_with_no_padding() {
        let bytes = Constants {
            values: [0x0102_0304, 0x0506_0708, 0x090a_0b0c, 0x0d0e_0f10],
        }
        .to_bytes();
        assert_eq!(bytes.len(), CONSTANTS_SIZE as usize);
        assert_eq!(
            bytes,
            [
                0x04, 0x03, 0x02, 0x01, 0x08, 0x07, 0x06, 0x05, 0x0c, 0x0b, 0x0a, 0x09, 0x10, 0x0f,
                0x0e, 0x0d,
            ]
        );
    }

    /// The committed artifacts really do put the block where the module docs
    /// say — the fact every backend's implementation is written against.
    ///
    /// Read out of the artifacts rather than restated, because a Slang release
    /// that moved any of these would leave the prose above describing a layout
    /// nothing has any more. The DXIL register is the one bullet not asserted
    /// here: its resource table lives in the container's `RDEF` chunk, and
    /// parsing DXBC to reach it is more machinery than this crate — which has
    /// no dependencies — should carry for one number. It was read with
    /// `dxc -dumpbin` over the committed container, and the byte-for-byte
    /// recompile gate is what keeps that container the one this source
    /// produces.
    #[test]
    fn each_artifact_puts_the_block_where_the_docs_say() {
        let msl = crate::PUSH_CONSTANT_PROBE
            .msl()
            .expect("the shader declares the msl target");
        assert!(
            msl.contains("uint device* destination_1 [[buffer(0)]]"),
            "the MSL no longer binds `destination` at buffer(0):\n{msl}"
        );
        assert!(
            msl.contains("ProbeConstants_0 constant* constants_1 [[buffer(1)]]"),
            "the MSL no longer binds the push-constant block at buffer(1), which is the index \
             crcbl-mtl computes for the slot after the last binding:\n{msl}"
        );

        let variables = push_constant_variables(crate::PUSH_CONSTANT_PROBE.spirv());
        assert_eq!(
            variables, 1,
            "the SPIR-V declares {variables} PushConstant variables, not the one block this \
             shader exists to carry"
        );
        // The same walk over the compute shader that takes its parameters from
        // a bound uniform instead. A scan that counted every module's
        // variables, or that matched the storage-class number anywhere in the
        // stream, would report one here too and the assertion above would mean
        // nothing.
        assert_eq!(
            push_constant_variables(crate::COMPUTE_PROBE.spirv()),
            0,
            "compute_probe.slang takes a bound uniform, so its SPIR-V has no PushConstant \
             variable — a walk that finds one is matching something else"
        );

        assert!(
            crate::PUSH_CONSTANT_PROBE.dxil("computeMain").is_some(),
            "the shader no longer ships the DXIL container the docs above describe"
        );
    }
}
