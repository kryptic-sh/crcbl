//! The uniform block `tonemap.slang` reads, in the layout that shader declares.
//!
//! Same reason as [`crate::ssao`]: the shader fixes a byte layout, every
//! producer of those bytes has to agree with it exactly, and keeping the mirror
//! in the crate that owns the source means there is one place to change rather
//! than one per consumer.

/// Bytes of the uniform block: one `float`, in a row of its own.
///
/// `std140` rounds a block up to a multiple of sixteen, so the one value is
/// followed by three padding words rather than sitting in a four-byte buffer.
/// See [`TonemapParams::to_bytes`].
pub const PARAMS_SIZE: usize = 16;

/// The exposure a renderer nobody has configured applies.
///
/// **`1.0`, which is what `static const float EXPOSURE` held while the value was
/// a compile-time constant** — so a caller that never touches it draws the frame
/// this engine drew before the block existed, and every golden image is
/// unchanged. It is also the value at which the operator is the identity on
/// `[0, 1]`, which is the argument `tonemap.slang`'s header makes for
/// exposure-and-clamp over a curve.
pub const DEFAULT_EXPOSURE: f32 = 1.0;

/// The uniform block, matching `struct TonemapParams` in `shaders/tonemap.slang`.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct TonemapParams {
    /// The multiplier applied to the scene colour before the clamp.
    pub exposure: f32,
}

impl Default for TonemapParams {
    /// [`DEFAULT_EXPOSURE`], so a block built without an opinion is the one the
    /// compile-time constant used to produce.
    fn default() -> Self {
        Self {
            exposure: DEFAULT_EXPOSURE,
        }
    }
}

impl TonemapParams {
    /// The block as the bytes a uniform buffer holds.
    ///
    /// Little-endian, and the three padding words after [`exposure`] are written
    /// rather than left alone for [`crate::ssao::SsaoParams::to_bytes`]'s reason:
    /// the buffer is [`PARAMS_SIZE`] wide and a partial write leaves the tail
    /// undefined.
    ///
    /// [`exposure`]: Self::exposure
    #[must_use]
    pub fn to_bytes(self) -> [u8; PARAMS_SIZE] {
        let mut bytes = [0u8; PARAMS_SIZE];
        bytes[..4].copy_from_slice(&self.exposure.to_le_bytes());
        bytes
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The block the shader declares, member for member.
    ///
    /// Nothing else can catch a rename or a reorder: the shader compiles either
    /// way and the buffer is bound either way, and a block whose one member moved
    /// would read a padding word as the exposure and draw a black frame. Reading
    /// the source is the check, and the source is hash-pinned by the manifest, so
    /// it is the same file the committed artifact was built from.
    #[test]
    fn the_uniform_block_matches_the_struct_tonemap_slang_declares() {
        let source = include_str!("../shaders/tonemap.slang");
        assert!(
            source.contains("float exposure;"),
            "tonemap.slang does not declare `float exposure;`"
        );
        assert!(
            source.contains("ConstantBuffer<TonemapParams> params;"),
            "tonemap.slang does not bind the block `to_bytes` writes"
        );
    }

    /// The exposure is the first word and the rest of the row is zeroed.
    #[test]
    fn the_block_is_one_value_and_a_padded_row() {
        let bytes = TonemapParams { exposure: 2.5 }.to_bytes();
        assert_eq!(bytes.len(), PARAMS_SIZE);
        assert_eq!(&bytes[0..4], &2.5f32.to_le_bytes());
        assert!(bytes[4..].iter().all(|byte| *byte == 0), "{bytes:?}");
    }

    /// The default is the value the constant held, which is what says the
    /// goldens cannot have moved.
    #[test]
    fn the_default_is_the_constant_the_shader_used_to_carry() {
        assert!((TonemapParams::default().exposure - 1.0).abs() < f32::EPSILON);
        assert_eq!(
            TonemapParams::default().to_bytes(),
            TonemapParams {
                exposure: DEFAULT_EXPOSURE,
            }
            .to_bytes(),
        );
    }
}
