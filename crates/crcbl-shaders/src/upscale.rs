//! The uniform block `upscale.slang` reads, and the CPU mirror of the
//! Catmull-Rom kernel that shader filters with.
//!
//! Same reason as [`crate::fxaa`]: the shader fixes a byte layout, every
//! producer of those bytes has to agree with it exactly, and keeping the mirror
//! in the crate that owns the source means there is one place to change rather
//! than one per consumer.
//!
//! [`catmull_rom_weights`] is here for a second reason. Two multiplied-out
//! cubics are exactly the kind of arithmetic that compiles, reads plausibly and
//! is wrong in a way no picture makes obvious — a slipped sign softens the
//! frame rather than breaking it. The tests below pin the kernel against the
//! properties the Catmull-Rom family is *defined* by, not against numbers this
//! file produced.

/// Bytes of the uniform block: two `float2`s.
///
/// Sixteen bytes of value, which is already a whole constant-buffer row, so
/// nothing is rounded up here the way [`crate::fxaa::PARAMS_SIZE`] is.
pub const PARAMS_SIZE: usize = 16;

/// The uniform block, matching `struct UpscaleParams` in `shaders/upscale.slang`.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct UpscaleParams {
    /// The extent of the image being read, in texels — the **internal** render
    /// extent, not the pass's own. That those two differ is the whole of what
    /// this pass is for.
    pub source_extent: [f32; 2],
    /// One over [`source_extent`](Self::source_extent), so the tap loop
    /// multiplies rather than dividing sixteen times.
    pub inv_source: [f32; 2],
}

impl UpscaleParams {
    /// The block for a source of `extent` texels.
    ///
    /// The reciprocal is taken here rather than in the shader for
    /// [`crate::fxaa::FxaaParams::for_extent`]'s reason: one division per frame
    /// instead of two per pixel, and a zero extent is a thing a caller can be
    /// told about on the CPU rather than an infinity in a tap coordinate.
    #[must_use]
    pub fn for_extent(width: u32, height: u32) -> Self {
        let width = f64::from(width.max(1));
        let height = f64::from(height.max(1));
        Self {
            source_extent: [width as f32, height as f32],
            inv_source: [(1.0 / width) as f32, (1.0 / height) as f32],
        }
    }

    /// The block as the bytes a uniform buffer holds.
    ///
    /// Little-endian, and the whole [`PARAMS_SIZE`] is written for
    /// [`crate::tonemap::TonemapParams::to_bytes`]'s reason: a partial write
    /// leaves the tail of the buffer undefined.
    #[must_use]
    pub fn to_bytes(self) -> [u8; PARAMS_SIZE] {
        let mut bytes = [0u8; PARAMS_SIZE];
        bytes[0..4].copy_from_slice(&self.source_extent[0].to_le_bytes());
        bytes[4..8].copy_from_slice(&self.source_extent[1].to_le_bytes());
        bytes[8..12].copy_from_slice(&self.inv_source[0].to_le_bytes());
        bytes[12..16].copy_from_slice(&self.inv_source[1].to_le_bytes());
        bytes
    }
}

/// The four Catmull-Rom weights for a sample `f` of the way between the two
/// middle texels of a run of four, `f` in `[0, 1)`.
///
/// The mirror of `catmull_rom_weights` in `shaders/upscale.slang`, member for
/// member. The kernel is the Mitchell-Netravali cubic at `B = 0`, `C = 0.5`:
///
/// ```text
/// w(x) =  1.5|x|^3 - 2.5|x|^2 + 1              for |x| <= 1
/// w(x) = -0.5|x|^3 + 2.5|x|^2 - 4|x| + 2       for 1 < |x| < 2
/// ```
///
/// evaluated at the four distances `1 + f`, `f`, `1 - f` and `2 - f` and
/// multiplied out, so each lane is one polynomial with no branch in it.
#[must_use]
pub fn catmull_rom_weights(f: f32) -> [f32; 4] {
    let f2 = f * f;
    let f3 = f2 * f;
    [
        -0.5 * f3 + f2 - 0.5 * f,
        1.5 * f3 - 2.5 * f2 + 1.0,
        -1.5 * f3 + 2.0 * f2 + 0.5 * f,
        0.5 * f3 - 0.5 * f2,
    ]
}

#[cfg(test)]
mod tests {
    use super::*;

    /// How far a weight may sit from an exact expectation.
    ///
    /// The kernel is four multiply-adds, so the error is a handful of ulps and
    /// not an approximation of anything — this is tight on purpose. A slipped
    /// coefficient misses by a hundredth at least.
    const TOLERANCE: f32 = 1e-6;

    /// The block the shader declares, member for member.
    ///
    /// Nothing else can catch a rename or a reorder: the shader compiles either
    /// way and the buffer is bound either way, and a block whose two members
    /// swapped would read a texel size as an extent and collapse every tap onto
    /// one texel. Reading the source is the check, and the source is hash-pinned
    /// by the manifest, so it is the same file the committed artifact was built
    /// from.
    #[test]
    fn the_uniform_block_matches_the_struct_upscale_slang_declares() {
        let source = include_str!("../shaders/upscale.slang");
        for member in ["float2 source_extent;", "float2 inv_source;"] {
            assert!(
                source.contains(member),
                "upscale.slang does not declare `{member}`"
            );
        }
        assert!(
            source.contains("ConstantBuffer<UpscaleParams> params;"),
            "upscale.slang does not bind the block `to_bytes` writes"
        );
    }

    /// The shader's kernel is spelled with the same four polynomials.
    ///
    /// The mirror below is only evidence about the shader if the two are the
    /// same arithmetic, and nothing compiles them together — so the coefficients
    /// are grepped out of the hash-pinned source, exactly as
    /// [`crate::tonemap`]'s ACES constants are.
    #[test]
    fn the_shader_spells_the_same_kernel() {
        let source = include_str!("../shaders/upscale.slang");
        for lane in [
            "-0.5 * f3 + f2 - 0.5 * f,",
            "1.5 * f3 - 2.5 * f2 + 1.0,",
            "-1.5 * f3 + 2.0 * f2 + 0.5 * f,",
            "0.5 * f3 - 0.5 * f2)",
        ] {
            assert!(
                source.contains(lane),
                "upscale.slang does not carry the lane `{lane}`"
            );
        }
    }

    /// Each member lands in the word the shader will read it from.
    #[test]
    fn every_member_is_written_at_the_offset_the_block_declares() {
        let params = UpscaleParams {
            source_extent: [256.0, 192.0],
            inv_source: [0.5, 0.25],
        };
        let bytes = params.to_bytes();
        assert_eq!(bytes.len(), PARAMS_SIZE);
        assert_eq!(&bytes[0..4], &256.0f32.to_le_bytes());
        assert_eq!(&bytes[4..8], &192.0f32.to_le_bytes());
        assert_eq!(&bytes[8..12], &0.5f32.to_le_bytes());
        assert_eq!(&bytes[12..16], &0.25f32.to_le_bytes());
    }

    /// The extent and its reciprocal describe the same image, and a zero extent
    /// does not become an infinity in a tap coordinate.
    #[test]
    fn the_texel_size_is_the_reciprocal_of_the_extent() {
        let params = UpscaleParams::for_extent(256, 192);
        assert_eq!(params.source_extent, [256.0, 192.0]);
        assert!((params.inv_source[0] - 1.0 / 256.0).abs() < f32::EPSILON);
        assert!((params.inv_source[1] - 1.0 / 192.0).abs() < f32::EPSILON);

        let degenerate = UpscaleParams::for_extent(0, 0);
        assert!(degenerate.inv_source[0].is_finite());
        assert!(degenerate.inv_source[1].is_finite());
    }

    /// **A partition of unity, everywhere.** The four weights sum to one at
    /// every position, which is what makes the filter preserve a flat region
    /// rather than darkening or brightening it.
    ///
    /// This is an exact algebraic identity — the `f`, `f^2` and `f^3` terms
    /// cancel across the four lanes and the constants leave `1` — so a slip in
    /// any single coefficient breaks it. It is the one property that checks all
    /// four polynomials at once.
    #[test]
    fn the_four_weights_sum_to_one_at_every_position() {
        for step in 0..=64 {
            let f = step as f32 / 64.0;
            let sum: f32 = catmull_rom_weights(f).iter().sum();
            assert!(
                (sum - 1.0).abs() < TOLERANCE,
                "the weights at f = {f} sum to {sum}"
            );
        }
    }

    /// **Interpolating, which is the reason this family and not a B-spline.**
    /// A sample landing exactly on a texel takes that texel and nothing else, so
    /// a 1:1 pass through this filter is the image it was handed.
    #[test]
    fn a_sample_on_a_texel_takes_that_texel_alone() {
        assert_eq!(catmull_rom_weights(0.0), [0.0, 1.0, 0.0, 0.0]);
    }

    /// **Symmetric at the half-way point**, where the two middle texels are
    /// equidistant and so are the two outer ones. An asymmetry here is a filter
    /// that shifts the image by a fraction of a texel, which reads as softness
    /// rather than as a shift.
    #[test]
    fn the_half_way_sample_is_symmetric() {
        let weights = catmull_rom_weights(0.5);
        assert!((weights[0] - weights[3]).abs() < TOLERANCE, "{weights:?}");
        assert!((weights[1] - weights[2]).abs() < TOLERANCE, "{weights:?}");
        // The published values of the kernel at its half-way point, which is the
        // one position a Catmull-Rom table is usually quoted at.
        assert!((weights[0] - -0.0625).abs() < TOLERANCE, "{weights:?}");
        assert!((weights[1] - 0.5625).abs() < TOLERANCE, "{weights:?}");
    }

    /// **The outer pair is negative between the texels**, which is where the
    /// filter's acutance comes from and is why `upscale.slang` clamps.
    ///
    /// A kernel whose outer lanes never went negative would be a smoothing
    /// filter wearing this one's name — it would pass every test above and
    /// produce a picture no sharper than a bilinear stretch.
    #[test]
    fn the_outer_lanes_go_negative_away_from_a_texel() {
        let weights = catmull_rom_weights(0.5);
        assert!(weights[0] < 0.0, "{weights:?}");
        assert!(weights[3] < 0.0, "{weights:?}");
    }
}
