//! The push-constant block `push_constant_raster.slang` reads from both
//! graphics stages — and the slot each committed artifact puts it at.
//!
//! Same reason as [`crate::push_constant_probe`], whose compute twin this is:
//! the shader fixes a byte layout, every producer of those bytes has to agree
//! with it exactly, and keeping the layout in the crate that owns the source
//! means one place to change rather than one per consumer.
//!
//! # Where the block lands, per target
//!
//! Read out of the committed artifacts rather than assumed, and **every one of
//! them is the first slot of its table**, because this shader declares no
//! numbered binding at all:
//!
//! * **SPIR-V** (`spirv/push_constant_raster.spv`) — one `OpVariable` in the
//!   `PushConstant` storage class, shared by both `OpEntryPoint`s, its block's
//!   two members decorated `Offset 0` and `Offset 16`. There is no descriptor
//!   set or binding number, which is the point of the storage class:
//!   `vkCmdPushConstants` addresses it by offset within the layout's range, for
//!   the stages the range names.
//! * **MSL** (`msl/push_constant_raster.metal`) — Metal has no push constants,
//!   so Slang lowers the block to an ordinary buffer argument, and it is
//!   `[[buffer(0)]]` in **both** entry points. `crcbl-mtl` computes the index as
//!   the buffer-table entry after the last binding, which for a layout with no
//!   bind groups is zero; it then sends the bytes with `setVertexBytes:` and
//!   `setFragmentBytes:` rather than the compute call, because Metal's argument
//!   tables are per stage.
//! * **DXIL** (`dxil/push_constant_raster.vertexMain.dxil` and its
//!   `fragmentMain` twin) — HLSL has no push constants either; Slang emits a
//!   `cbuffer` and `dxc` binds it at **`cb0`** (register `b0`, space 0),
//!   [`CONSTANTS_SIZE`](crate::push_constant_raster::CONSTANTS_SIZE) bytes, in
//!   each container. A D3D12 root signature therefore carries a root-constants
//!   entry of that many bytes' worth of 32-bit values at `b0`, whose *shader
//!   visibility* is computed from the range's stages — the per-stage plumbing a
//!   compute-only range never exercises.
//!
//! # The two stages read different halves, and that is deliberate
//!
//! [`rect`](crate::push_constant_raster::Constants::rect) is read only by the
//! vertex stage and [`color`](crate::push_constant_raster::Constants::color)
//! only by the fragment stage, so a block delivered to one stage and not the
//! other is visible in the picture instead of being folded into a single
//! verdict. The shader's own comment says which half produces which failure.

/// Vertices one draw of this shader submits: three per triangle, two triangles
/// over the four corners of [`Constants::rect`].
///
/// The shader has no vertex buffer to bound this against — the corners come out
/// of the block and the index the rasteriser supplies — so the number is the
/// one `vertexMain`'s `switch` names, and a draw of any other length is a
/// different shape rather than a different amount of the same one.
pub const VERTEX_COUNT: u32 = 6;

/// Bytes of the push-constant block — what
/// `crcbl_hal::PushConstantRange::size` takes.
///
/// The range starts at offset 0: the SPIR-V decorates the block's first member
/// `Offset 0`, and there is nothing in front of it on any target.
pub const CONSTANTS_SIZE: u32 = 32;

/// The push-constant block, matching `struct RasterConstants` in
/// `shaders/push_constant_raster.slang`.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Constants {
    /// What the fragment stage writes to the colour target, linear RGBA.
    pub color: [f32; 4],
    /// The clip-space rectangle the vertex stage spans, as
    /// `(min x, min y, max x, max y)`.
    pub rect: [f32; 4],
}

// The two members are the whole block, tightly packed, and the buffer a caller
// pushes is the same length. A member added without the size growing with it
// would silently stop being delivered.
const _: () = assert!(CONSTANTS_SIZE as usize == 8 * size_of::<f32>());

impl Constants {
    /// The block as the bytes `crcbl_hal::CommandEncoder::push_constants`
    /// takes.
    ///
    /// Little-endian IEEE-754 singles, tightly packed, `color` then `rect`: a
    /// `float4` is sixteen bytes at a sixteen-byte alignment on every target
    /// this shader is compiled for, so the pair has no padding between them and
    /// none at the end.
    #[must_use]
    pub fn to_bytes(self) -> [u8; CONSTANTS_SIZE as usize] {
        let mut bytes = [0u8; CONSTANTS_SIZE as usize];
        let values = self.color.into_iter().chain(self.rect);
        for (value, chunk) in values.zip(bytes.chunks_exact_mut(4)) {
            chunk.copy_from_slice(&value.to_le_bytes());
        }
        bytes
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The shader must read a push constant and not a bound block, or this
    /// whole module documents a shader that proves nothing — and it must not
    /// declare the WGSL target, which is the reason [`crate`]'s docs give.
    #[test]
    fn the_source_declares_a_push_constant_and_no_wgsl_target() {
        let source = include_str!("../shaders/push_constant_raster.slang");
        assert!(
            source.contains("[[vk::push_constant]]\nConstantBuffer<RasterConstants> constants;"),
            "push_constant_raster.slang no longer declares its block as a push constant"
        );
        assert!(
            source.contains("// crcbl-targets: spirv, msl, dxil\n"),
            "push_constant_raster.slang's target declaration has changed; WGSL cannot carry a push \
             constant and naga refuses the artifact Slang emits for one"
        );
    }

    /// The byte layout, checked rather than asserted in prose: eight
    /// little-endian singles, tightly packed, `color` before `rect`.
    #[test]
    fn the_block_is_two_le_float4s_with_no_padding() {
        let bytes = Constants {
            color: [1.0, 2.0, 3.0, 4.0],
            rect: [5.0, 6.0, 7.0, 8.0],
        }
        .to_bytes();
        assert_eq!(bytes.len(), CONSTANTS_SIZE as usize);
        let mut want = Vec::new();
        for value in [1.0f32, 2.0, 3.0, 4.0, 5.0, 6.0, 7.0, 8.0] {
            want.extend_from_slice(&value.to_le_bytes());
        }
        assert_eq!(bytes.as_slice(), want.as_slice());
    }

    /// The committed artifacts really do put the block where the module docs
    /// say — the fact every backend's implementation is written against.
    ///
    /// Read out of the artifacts rather than restated, because a Slang release
    /// that moved any of these would leave the prose above describing a layout
    /// nothing has any more. The DXIL register is the one bullet not asserted
    /// here, for [`crate::push_constant_probe`]'s reason: it lives in the
    /// container's `RDEF` chunk, and parsing DXBC to reach it is more machinery
    /// than this crate — which has no dependencies — should carry for one
    /// number. It was read with `dxc -dumpbin` over the committed containers,
    /// and the byte-for-byte recompile gate is what keeps those containers the
    /// ones this source produces.
    /// The two MSL assertions are matched per **stage** rather than by
    /// searching the whole file, because "both stages take the block" is half of
    /// what this shader exists to prove and a file-wide match is satisfied by
    /// either one of them alone.
    #[test]
    fn each_artifact_puts_the_block_where_the_docs_say() {
        let msl = crate::PUSH_CONSTANT_RASTER
            .msl()
            .expect("the shader declares the msl target");
        for attribute in ["[[vertex]]", "[[fragment]]"] {
            let after = msl
                .split_once(attribute)
                .unwrap_or_else(|| panic!("the MSL declares no {attribute} stage:\n{msl}"))
                .1;
            // Split on the body's brace rather than on the signature's closing
            // parenthesis: `[[buffer(0)]]` carries one of its own, so a match on
            // `)` truncates the argument list mid-attribute and the assertion
            // below could never see the index it is looking for.
            let parameters = after
                .split_once('{')
                .unwrap_or_else(|| panic!("the {attribute} signature is unterminated:\n{after}"))
                .0;
            assert!(
                parameters.contains("RasterConstants_0 constant* constants")
                    && parameters.contains("[[buffer(0)]]"),
                "the {attribute} stage no longer takes the push-constant block at buffer(0), which \
                 is the index crcbl-mtl computes for a layout with no bind groups:\n{parameters}"
            );
        }

        assert_eq!(
            crate::push_constant_probe::push_constant_variables(
                crate::PUSH_CONSTANT_RASTER.spirv()
            ),
            1,
            "the SPIR-V no longer declares exactly the one PushConstant variable this shader \
             exists to carry"
        );

        for entry in ["vertexMain", "fragmentMain"] {
            assert!(
                crate::PUSH_CONSTANT_RASTER.dxil(entry).is_some(),
                "the shader no longer ships the DXIL container the docs above describe for {entry}"
            );
        }
    }
}
