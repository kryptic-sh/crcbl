//! The uniform block `fxaa.slang` reads, in the layout that shader declares.
//!
//! Same reason as [`crate::tonemap`]: the shader fixes a byte layout, every
//! producer of those bytes has to agree with it exactly, and keeping the mirror
//! in the crate that owns the source means there is one place to change rather
//! than one per consumer.

/// Bytes of the uniform block: a `float2` and three `float`s.
///
/// Twenty bytes of value in a block a constant buffer rounds up to two
/// sixteen-byte rows, because an element may not straddle a row boundary and
/// [`FxaaParams::subpixel`] is the fifth word. See [`FxaaParams::to_bytes`].
pub const PARAMS_SIZE: usize = 32;

/// Local luma range, as a fraction of the local maximum, below which a pixel is
/// left alone.
///
/// FXAA 3.11's own "high quality" edge threshold. Above it the filter is
/// noticeably reluctant on low-contrast edges — which are the ones a gradient
/// crawls along — and below it the pass starts filtering texture detail that was
/// never an edge.
pub const DEFAULT_EDGE_THRESHOLD: f32 = 0.166;

/// Absolute floor under [`DEFAULT_EDGE_THRESHOLD`].
///
/// FXAA 3.11's own. Without it a dark region reads as one edge after another,
/// because a range that is a sixth of a very small maximum is very small too.
pub const DEFAULT_EDGE_THRESHOLD_MIN: f32 = 0.083;

/// How much of the subpixel correction is applied.
///
/// FXAA 3.11's own default. This is the knob that trades the filter's
/// characteristic softening against the shimmer it exists to remove, so it is
/// the one a quality tier moves first.
pub const DEFAULT_SUBPIXEL: f32 = 0.75;

/// The uniform block, matching `struct FxaaParams` in `shaders/fxaa.slang`.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct FxaaParams {
    /// One over the extent of the image being read, which is the pass's own
    /// extent — the resolve is 1:1.
    pub inv_source: [f32; 2],
    /// See [`DEFAULT_EDGE_THRESHOLD`].
    pub edge_threshold: f32,
    /// See [`DEFAULT_EDGE_THRESHOLD_MIN`].
    pub edge_threshold_min: f32,
    /// See [`DEFAULT_SUBPIXEL`].
    pub subpixel: f32,
}

impl Default for FxaaParams {
    /// FXAA 3.11's own defaults, and an `inv_source` of zero.
    ///
    /// Zero is not a plausible texel size and is not meant to be: the extent is
    /// the one field only the renderer knows, so a block that still carries the
    /// default there is a block nobody finished filling in. The pass degrades to
    /// sampling one texel over and over rather than to something that looks
    /// nearly right, which is the failure worth having.
    fn default() -> Self {
        Self {
            inv_source: [0.0, 0.0],
            edge_threshold: DEFAULT_EDGE_THRESHOLD,
            edge_threshold_min: DEFAULT_EDGE_THRESHOLD_MIN,
            subpixel: DEFAULT_SUBPIXEL,
        }
    }
}

impl FxaaParams {
    /// The block for an image of `extent` texels, at the default quality.
    ///
    /// The reciprocal is taken here rather than in the shader because it is one
    /// division per frame instead of two per pixel, and because a zero extent is
    /// a thing a caller can be told about on the CPU.
    #[must_use]
    pub fn for_extent(width: u32, height: u32) -> Self {
        Self {
            inv_source: [1.0 / width.max(1) as f32, 1.0 / height.max(1) as f32],
            ..Self::default()
        }
    }

    /// The block as the bytes a uniform buffer holds.
    ///
    /// Little-endian, and the padding after [`subpixel`] is written rather than
    /// left alone for [`crate::tonemap::TonemapParams::to_bytes`]'s reason: the
    /// buffer is [`PARAMS_SIZE`] wide and a partial write leaves the tail
    /// undefined.
    ///
    /// [`subpixel`]: Self::subpixel
    #[must_use]
    pub fn to_bytes(self) -> [u8; PARAMS_SIZE] {
        let mut bytes = [0u8; PARAMS_SIZE];
        bytes[0..4].copy_from_slice(&self.inv_source[0].to_le_bytes());
        bytes[4..8].copy_from_slice(&self.inv_source[1].to_le_bytes());
        bytes[8..12].copy_from_slice(&self.edge_threshold.to_le_bytes());
        bytes[12..16].copy_from_slice(&self.edge_threshold_min.to_le_bytes());
        bytes[16..20].copy_from_slice(&self.subpixel.to_le_bytes());
        bytes
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The block the shader declares, member for member.
    ///
    /// Nothing else can catch a rename or a reorder: the shader compiles either
    /// way and the buffer is bound either way, and a block whose members moved
    /// would read the edge threshold as a texel size and blur the whole frame.
    /// Reading the source is the check, and the source is hash-pinned by the
    /// manifest, so it is the same file the committed artifact was built from.
    #[test]
    fn the_uniform_block_matches_the_struct_fxaa_slang_declares() {
        let source = include_str!("../shaders/fxaa.slang");
        for member in [
            "float2 inv_source;",
            "float edge_threshold;",
            "float edge_threshold_min;",
            "float subpixel;",
        ] {
            assert!(
                source.contains(member),
                "fxaa.slang does not declare `{member}`"
            );
        }
        assert!(
            source.contains("ConstantBuffer<FxaaParams> params;"),
            "fxaa.slang does not bind the block `to_bytes` writes"
        );
    }

    /// Each member lands in the word the shader will read it from, and the tail
    /// of the second row is zeroed.
    #[test]
    fn every_member_is_written_at_the_offset_the_block_declares() {
        let params = FxaaParams {
            inv_source: [0.5, 0.25],
            edge_threshold: 0.125,
            edge_threshold_min: 0.0625,
            subpixel: 0.5,
        };
        let bytes = params.to_bytes();
        assert_eq!(bytes.len(), PARAMS_SIZE);
        assert_eq!(&bytes[0..4], &0.5f32.to_le_bytes());
        assert_eq!(&bytes[4..8], &0.25f32.to_le_bytes());
        assert_eq!(&bytes[8..12], &0.125f32.to_le_bytes());
        assert_eq!(&bytes[12..16], &0.0625f32.to_le_bytes());
        assert_eq!(&bytes[16..20], &0.5f32.to_le_bytes());
        assert!(bytes[20..].iter().all(|byte| *byte == 0), "{bytes:?}");
    }

    /// The extent becomes a texel size, and a zero extent does not become an
    /// infinity.
    #[test]
    fn the_texel_size_is_the_reciprocal_of_the_extent() {
        let params = FxaaParams::for_extent(256, 192);
        assert!((params.inv_source[0] - 1.0 / 256.0).abs() < f32::EPSILON);
        assert!((params.inv_source[1] - 1.0 / 192.0).abs() < f32::EPSILON);

        let degenerate = FxaaParams::for_extent(0, 0);
        assert!(degenerate.inv_source[0].is_finite());
        assert!(degenerate.inv_source[1].is_finite());
    }

    /// A block nobody gave an extent to carries a texel size of zero, which is
    /// the tell rather than a plausible-looking wrong answer.
    #[test]
    fn the_default_block_has_no_extent_in_it() {
        assert_eq!(FxaaParams::default().inv_source, [0.0, 0.0]);
    }
}
