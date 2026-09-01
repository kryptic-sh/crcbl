//! The uniform block and the constants `contact_shadows.slang` declares, in the
//! layouts that shader declares.
//!
//! Same reason as [`crate::ssao`]: the shader fixes a byte layout and a set of
//! values, every producer of those has to agree with it exactly, and keeping
//! both in the crate that owns the source means there is one place to change
//! rather than one per consumer.

/// Bytes of the uniform block: two `float4x4` and one `float4` row.
///
/// `std140` gives a `float4x4` four sixteen-byte columns and a `float4` one row,
/// and the total is already a multiple of sixteen, so there is no tail padding
/// to write. See [`ContactShadowParams::to_bytes`].
pub const PARAMS_SIZE: usize = 64 + 64 + 16;

/// The reversed-Z far plane, matching `static const float DEPTH_FAR` in
/// `shaders/contact_shadows.slang`.
///
/// [`crate::ssao::DEPTH_FAR`]'s value and its argument, restated on this
/// shader's own terms: the march leaves early at exactly this depth, because an
/// infinite reversed-Z projection takes `clip.w` to zero here and a pixel the
/// geometry never covered would reconstruct a view-space position by dividing by
/// nothing. `crcbl_render::forward` is where both are asserted equal to
/// `crcbl_hal::depth::CLEAR`.
pub const DEPTH_FAR: f32 = 0.0;

/// The channel value a fragment nothing shadows carries, matching
/// `static const float LIT` in `shaders/contact_shadows.slang`.
///
/// It is also the value `crcbl_render::forward`'s 1×1 placeholder holds, which
/// is what makes a frame that records no march multiply its sun by one.
pub const LIT: f32 = 1.0;

/// Depth texels the march may cross, matching `static const uint MAX_STEPS` in
/// `shaders/contact_shadows.slang`.
///
/// The march takes one depth texel per step, so this is the reach in pixels as
/// well as the step budget — the shader's `MAX_REACH` is one less, for the
/// partial pixel the ray starts inside. Mirrored here because it is what prices
/// the pass: the cost of this effect is this many `Load`s per covered pixel, and
/// a reader asking what the pass costs should not have to open a shader to find
/// the number.
pub const MAX_STEPS: u32 = 16;

/// How far the ray reaches, in world units, matching
/// `static const float RAY_LENGTH` in `shaders/contact_shadows.slang`.
pub const RAY_LENGTH: f32 = 0.25;

/// `contact_shadows.slang`'s uniform block.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct ContactShadowParams {
    /// Clip → view: the inverse of the camera's projection alone, **not** of its
    /// view-projection. The march is a view-space ray, and view space is where
    /// the depth prepass's texels unproject to.
    pub inv_proj: [f32; 16],
    /// View → clip, for projecting the ray's start and its direction into the
    /// screen the march walks.
    pub proj: [f32; 16],
    /// The unit **view-space** direction towards the sun.
    ///
    /// The same vector a `KIND_DIRECTIONAL` row of `mesh.slang`'s light list
    /// carries in its `direction`, rotated into view space by the caller: the
    /// march has no view matrix and no world-space anything, so the rotation
    /// happens once on the host rather than per fragment.
    pub to_light: [f32; 3],
}

impl ContactShadowParams {
    /// The block as the bytes a uniform buffer holds.
    ///
    /// Little-endian throughout, and the padding word after [`to_light`] is
    /// written rather than left alone for [`crate::ssao::SsaoParams::to_bytes`]'s
    /// reason: the buffer is [`PARAMS_SIZE`] wide and a partial write leaves the
    /// tail undefined.
    ///
    /// [`to_light`]: Self::to_light
    #[must_use]
    pub fn to_bytes(self) -> [u8; PARAMS_SIZE] {
        let mut bytes = [0u8; PARAMS_SIZE];
        let mut at = 0;
        for value in self
            .inv_proj
            .into_iter()
            .chain(self.proj)
            .chain(self.to_light)
        {
            bytes[at..at + 4].copy_from_slice(&value.to_le_bytes());
            at += 4;
        }
        debug_assert_eq!(at + 4, PARAMS_SIZE, "one padding word closes the row");
        bytes
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The constants and the shader must name the same values.
    ///
    /// [`crate::ssao`]'s `the_far_plane_matches_the_constant_ssao_slang_declares`
    /// check, for its reason: the shader compiles whatever these say, and a
    /// disagreement shows up only as a frame marched at a reach nobody chose.
    /// Reading the source is the check, and the source is hash-pinned by the
    /// manifest, so it is the same file the committed artifact was built from.
    #[test]
    fn the_constants_match_the_ones_contact_shadows_slang_declares() {
        let source = include_str!("../shaders/contact_shadows.slang");
        for declaration in [
            format!("static const float DEPTH_FAR = {DEPTH_FAR:.1};"),
            format!("static const float LIT = {LIT:.1};"),
            format!("static const uint MAX_STEPS = {MAX_STEPS}u;"),
            format!("static const float RAY_LENGTH = {RAY_LENGTH};"),
        ] {
            assert!(
                source.contains(&declaration),
                "contact_shadows.slang does not declare `{declaration}`; the mirror has drifted"
            );
        }
    }

    /// The block is written whole, and the padding word is written too.
    ///
    /// The observable is the tail: a `to_bytes` that stopped after `to_light`
    /// would leave four bytes of whatever the array was initialised with, which
    /// is zero here and undefined in the mapped buffer this is copied into.
    #[test]
    fn the_block_is_the_two_matrices_the_direction_and_a_padding_word() {
        let params = ContactShadowParams {
            inv_proj: [1.0; 16],
            proj: [2.0; 16],
            to_light: [3.0, 4.0, 5.0],
        };
        let bytes = params.to_bytes();
        assert_eq!(bytes.len(), PARAMS_SIZE);
        assert_eq!(&bytes[0..4], &1.0f32.to_le_bytes());
        assert_eq!(&bytes[64..68], &2.0f32.to_le_bytes());
        assert_eq!(&bytes[128..132], &3.0f32.to_le_bytes());
        assert_eq!(&bytes[132..136], &4.0f32.to_le_bytes());
        assert_eq!(&bytes[136..140], &5.0f32.to_le_bytes());
        assert_eq!(&bytes[140..144], &0.0f32.to_le_bytes());
    }
}
