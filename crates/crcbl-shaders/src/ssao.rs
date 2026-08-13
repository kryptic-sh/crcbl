//! The uniform block and the far-plane constant `ssao.slang` declares, in the
//! layouts that shader declares.
//!
//! Same reason as [`crate::compute_probe`]: the shader fixes a byte layout and a
//! value, every producer of those has to agree with it exactly, and keeping both
//! in the crate that owns the source means there is one place to change rather
//! than one per consumer.
//!
//! `ssao_blur.slang` has no module of its own, because it has no block and no
//! constant a caller has to match — it reads one texture and writes one channel.

/// Bytes of the uniform block: two `float4x4` and one `float4` row.
///
/// `std140` gives a `float4x4` four sixteen-byte columns and a `float4` one row,
/// and the total is already a multiple of sixteen, so there is no tail padding
/// to write. See [`SsaoParams::to_bytes`].
pub const PARAMS_SIZE: usize = 64 + 64 + 16;

/// The reversed-Z far plane, matching `static const float DEPTH_FAR` in
/// `shaders/ssao.slang`.
///
/// The value `crcbl_hal::depth::CLEAR` holds, restated here because this crate
/// does not depend on the seam and the shader cannot include either of them.
/// `crcbl_render::forward` is where the two are asserted equal, which is the
/// link that makes this a mirror rather than a second opinion.
///
/// **The far plane has no surface.** `ssao.slang` leaves early at exactly this
/// depth: an infinite reversed-Z projection takes `clip.w` to zero here, so a
/// pixel the geometry never covered would otherwise reconstruct a view-space
/// position by dividing by nothing.
pub const DEPTH_FAR: f32 = 0.0;

/// The uniform block, matching `struct SsaoParams` in `shaders/ssao.slang`.
///
/// Both matrices are **column-major**, the order `glam::Mat4::to_cols_array`
/// produces and the order every other block in this crate is written in — see
/// [`crate::mesh::FrameUniforms`], whose `view_proj` this file's `proj` is a
/// sibling of.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct SsaoParams {
    /// Clip → view: the inverse of the camera's projection alone, **not** of its
    /// view-projection. The occlusion integral is a question about the
    /// neighbourhood of a surface, and view space is where that neighbourhood is
    /// isotropic and the eye is at the origin.
    pub inv_proj: [f32; 16],
    /// View → clip, for projecting a sample point back to the pixel whose depth
    /// answers for it.
    pub proj: [f32; 16],
    /// The sampling radius, in world units.
    pub radius: f32,
    /// The depth bias that keeps a surface from occluding itself, in view-space
    /// units.
    pub bias: f32,
}

impl SsaoParams {
    /// The block as the bytes a uniform buffer holds.
    ///
    /// Little-endian throughout, and the two padding words after [`bias`] are
    /// written rather than left alone for [`crate::compute_probe::Params`]'s
    /// reason: the buffer is [`PARAMS_SIZE`] wide and a partial write leaves the
    /// tail undefined.
    ///
    /// [`bias`]: Self::bias
    #[must_use]
    pub fn to_bytes(self) -> [u8; PARAMS_SIZE] {
        let mut bytes = [0u8; PARAMS_SIZE];
        let mut at = 0;
        for value in self.inv_proj.into_iter().chain(self.proj) {
            bytes[at..at + 4].copy_from_slice(&value.to_le_bytes());
            at += 4;
        }
        for value in [self.radius, self.bias] {
            bytes[at..at + 4].copy_from_slice(&value.to_le_bytes());
            at += 4;
        }
        debug_assert_eq!(at + 8, PARAMS_SIZE, "two padding words close the row");
        bytes
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The constant and the shader must name the same far plane.
    ///
    /// Nothing else can catch this: the shader compiles either way, and a
    /// mismatch shows up only as an unoccluded sky or a division by zero on
    /// whatever machine happens to look. Reading the source is the check, and the
    /// source is hash-pinned by the manifest, so it is the same file the
    /// committed artifact was built from.
    #[test]
    fn the_far_plane_matches_the_constant_ssao_slang_declares() {
        let source = include_str!("../shaders/ssao.slang");
        let declaration = format!("static const float DEPTH_FAR = {DEPTH_FAR:.1};");
        assert!(
            source.contains(&declaration),
            "ssao.slang does not declare `{declaration}`; DEPTH_FAR has drifted from the shader"
        );
    }

    /// The block the shader declares, member for member and in this order.
    ///
    /// The offsets below are what [`SsaoParams::to_bytes`] writes, so this is the
    /// check that the *shader* agrees about which matrix comes first — swapping
    /// them produces a frame that is occluded everywhere or nowhere, and both are
    /// pictures.
    #[test]
    fn the_uniform_block_matches_the_struct_ssao_slang_declares() {
        let source = include_str!("../shaders/ssao.slang");
        for declaration in ["float4x4 inv_proj;", "float4x4 proj;", "float4 params;"] {
            assert!(
                source.contains(declaration),
                "ssao.slang does not declare `{declaration}`"
            );
        }
        let inv_proj = source.find("float4x4 inv_proj;").expect("just checked");
        let proj = source.find("float4x4 proj;").expect("just checked");
        let params = source.find("float4 params;").expect("just checked");
        assert!(
            inv_proj < proj && proj < params,
            "ssao.slang declares the block in a different order than `to_bytes` writes it"
        );
    }

    /// The layout claim, checked rather than asserted in prose.
    #[test]
    fn the_block_is_two_matrices_and_a_padded_row() {
        let mut inv_proj = [0.0f32; 16];
        inv_proj[0] = 1.0;
        let mut proj = [0.0f32; 16];
        proj[15] = 2.0;
        let bytes = SsaoParams {
            inv_proj,
            proj,
            radius: 0.5,
            bias: 0.25,
        }
        .to_bytes();

        assert_eq!(bytes.len(), PARAMS_SIZE);
        assert_eq!(&bytes[0..4], &1.0f32.to_le_bytes());
        assert_eq!(&bytes[124..128], &2.0f32.to_le_bytes());
        assert_eq!(&bytes[128..132], &0.5f32.to_le_bytes());
        assert_eq!(&bytes[132..136], &0.25f32.to_le_bytes());
        assert!(bytes[136..].iter().all(|byte| *byte == 0), "{bytes:?}");
    }
}
