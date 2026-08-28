//! The base-colour page's mip chain, built on the host at import.
//!
//! ```text
//! level 0 (extent²) ─box, linear light─▶ level 1 (extent/2)² ─▶ … ─▶ 1×1
//! ```
//!
//! `docs/plan/43-render-standards.md`'s filtering rung: every layer of a
//! [`PageDesc`](crate::scene::PageDesc) reaches the device with its whole chain,
//! and [`crate::texture`] uploads one copy per level. The chain is built here
//! rather than by a compute pass because the page's `Rgba8UnormSrgb` format is
//! what decodes its texels, WebGPU refuses to reinterpret an sRGB image as a
//! storage view, and a host chain is the same bytes on every backend — a
//! determinism a device filter would have to be argued back to.
//!
//! [`resample`] is the one filter, and it is also what `crcbl-scene`'s glTF
//! importer packs real textures onto the page with: a box average in linear
//! light, weighted by alpha. One filter for both jobs, so a texture that was
//! resampled onto the page and then mipped was averaged the same way twice.

/// Resample `pixels`, a tightly packed `width × height` RGBA8 sRGB image, onto
/// a square `extent`, alpha-weighted and in linear light.
///
/// A box filter: each destination texel averages the source texels its own cell
/// covers. Where the source is *smaller* than the destination each cell covers
/// exactly one texel, so an upscale is nearest-neighbour — the page's sampler
/// blends at the fetch, and blending here as well would blur twice.
///
/// Two things it does that a naive average does not, and both are visible when
/// they are missing. The stored bytes are sRGB-encoded, so they are decoded to
/// linear before averaging and re-encoded after; averaging the encodings instead
/// darkens every downscale. And the colours are weighted by alpha, so a
/// transparent texel does not drag the colour of its neighbours towards whatever
/// happens to be stored under it.
///
/// # Panics
///
/// If `pixels` is shorter than `width × height × 4`, or any dimension is zero.
#[must_use]
pub fn resample(pixels: &[u8], width: u32, height: u32, extent: u32) -> Vec<u8> {
    assert!(
        width > 0 && height > 0 && extent > 0,
        "a {width}×{height} image resampled onto {extent}² has no texels"
    );
    assert!(
        pixels.len() >= width as usize * height as usize * 4,
        "a {width}×{height} RGBA8 image is {} bytes, not {}",
        width as usize * height as usize * 4,
        pixels.len()
    );
    let mut out = vec![0u8; extent as usize * extent as usize * 4];
    for y in 0..extent {
        let (y0, y1) = source_span(y, extent, height);
        for x in 0..extent {
            let (x0, x1) = source_span(x, extent, width);
            let mut weighted = [0.0f32; 3];
            let mut plain = [0.0f32; 3];
            let mut alpha = 0.0f32;
            let mut texels = 0.0f32;
            for sy in y0..y1 {
                for sx in x0..x1 {
                    let at = (sy as usize * width as usize + sx as usize) * 4;
                    let a = f32::from(pixels[at + 3]) / 255.0;
                    for channel in 0..3 {
                        let linear = srgb_to_linear(pixels[at + channel]);
                        weighted[channel] += linear * a;
                        plain[channel] += linear;
                    }
                    alpha += a;
                    texels += 1.0;
                }
            }
            let at = (y as usize * extent as usize + x as usize) * 4;
            for channel in 0..3 {
                // A cell that is wholly transparent has no alpha to weight by,
                // and its colour is still the best guess for what is under it.
                let linear = if alpha > 0.0 {
                    weighted[channel] / alpha
                } else {
                    plain[channel] / texels
                };
                out[at + channel] = linear_to_srgb(linear);
            }
            out[at + 3] = quantise(alpha / texels);
        }
    }
    out
}

/// The side of mip `level` of a square image `extent` wide: halved per level
/// and never below one texel, which is how every backend defines it.
#[must_use]
pub const fn level_extent(extent: u32, level: u32) -> u32 {
    let halved = if level >= u32::BITS {
        0
    } else {
        extent >> level
    };
    if halved == 0 { 1 } else { halved }
}

/// The levels **below** `level0`, a square `extent`-wide RGBA8 sRGB layer, down
/// to a single texel: `[level 1, level 2, …]`.
///
/// Each level is [`resample`]d from the one above it rather than from level 0,
/// which is what every offline mipper does and what makes the chain's cost a
/// third of level 0 on top rather than a level-0 pass per level. The count is
/// [`Extent3d::full_mip_levels`](crcbl_hal::Extent3d::full_mip_levels) minus
/// the one the caller already has, so a one-texel layer gets an empty chain.
///
/// # Panics
///
/// [`resample`]'s conditions on `level0` and `extent`.
#[must_use]
pub fn chain(level0: &[u8], extent: u32) -> Vec<Vec<u8>> {
    let mut levels: Vec<Vec<u8>> = Vec::new();
    let mut wide = extent;
    while wide > 1 {
        let below = level_extent(wide, 1);
        let level = {
            let above = levels.last().map_or(level0, Vec::as_slice);
            resample(above, wide, wide, below)
        };
        levels.push(level);
        wide = below;
    }
    levels
}

/// The half-open run of source texels destination texel `at` covers, on one
/// axis.
///
/// Never empty: when the destination is the larger of the two, the run is the
/// single texel the destination centre falls in.
fn source_span(at: u32, extent: u32, source: u32) -> (u32, u32) {
    let scale = |step: u32| (u64::from(step) * u64::from(source) / u64::from(extent)) as u32;
    let start = scale(at).min(source.saturating_sub(1));
    let end = scale(at + 1).clamp(start + 1, source);
    (start, end)
}

/// One sRGB-encoded byte as a linear value in `0..=1`.
///
/// The IEC 61966-2-1 transfer function, which is what `Rgba8UnormSrgb` decodes
/// with.
fn srgb_to_linear(value: u8) -> f32 {
    let encoded = f32::from(value) / 255.0;
    if encoded <= 0.040_45 {
        encoded / 12.92
    } else {
        ((encoded + 0.055) / 1.055).powf(2.4)
    }
}

/// The inverse of [`srgb_to_linear`], rounded to a byte.
fn linear_to_srgb(value: f32) -> u8 {
    let linear = value.clamp(0.0, 1.0);
    let encoded = if linear <= 0.003_130_8 {
        12.92 * linear
    } else {
        1.055 * linear.powf(1.0 / 2.4) - 0.055
    };
    quantise(encoded)
}

/// A `0..=1` fraction as the nearest byte.
fn quantise(value: f32) -> u8 {
    // `clamp` first so the cast cannot saturate on a NaN-free out-of-range
    // input, and `+ 0.5` so it rounds rather than truncates.
    (value.clamp(0.0, 1.0) * 255.0 + 0.5) as u8
}

#[cfg(test)]
mod tests {
    use super::*;
    use crcbl_hal::{Extent3d, ImageType};

    /// A 2×2 of red, green, blue and yellow, opaque.
    const QUAD: [u8; 16] = [
        0xFF, 0x00, 0x00, 0xFF, //
        0x00, 0xFF, 0x00, 0xFF, //
        0x00, 0x00, 0xFF, 0xFF, //
        0xFF, 0xFF, 0x00, 0xFF,
    ];

    #[test]
    fn resampling_onto_the_extent_an_image_already_has_changes_no_texel() {
        assert_eq!(resample(&QUAD, 2, 2, 2), QUAD);
    }

    #[test]
    fn a_downscale_averages_in_linear_light_rather_than_in_stored_bytes() {
        // Two black and two white texels. Their average is a half in *linear*
        // light, and sRGB encodes a half at 188 — the mid grey a person would
        // pick. Averaging the stored bytes instead lands on 128, which is a
        // linear 0.216: the same picture, visibly darker, on every downscale.
        let checker = [
            0x00, 0x00, 0x00, 0xFF, //
            0xFF, 0xFF, 0xFF, 0xFF, //
            0xFF, 0xFF, 0xFF, 0xFF, //
            0x00, 0x00, 0x00, 0xFF,
        ];
        let one = resample(&checker, 2, 2, 1);

        assert_eq!(one.len(), 4);
        assert_eq!(one[3], 0xFF, "opaque in, opaque out");
        for channel in &one[..3] {
            assert!(
                (i32::from(*channel) - 188).abs() <= 1,
                "a half in linear light encodes to 188 and this is {channel}"
            );
            assert!(
                (i32::from(*channel) - 128).abs() > 8,
                "{channel} is the byte average, which is the bug this asserts against"
            );
        }
    }

    #[test]
    fn an_upscale_repeats_texels_rather_than_inventing_them() {
        // The page's sampler magnifies bilinear, so an upscale that blended
        // here would blur the texels twice — once at import and once at the
        // fetch.
        let four = resample(&QUAD, 2, 2, 4);
        let texel = |x: usize, y: usize| &four[(y * 4 + x) * 4..][..4];

        assert_eq!(texel(0, 0), &QUAD[0..4], "red fills the top-left quarter");
        assert_eq!(texel(1, 1), &QUAD[0..4]);
        assert_eq!(texel(2, 0), &QUAD[4..8], "green the top-right");
        assert_eq!(texel(0, 2), &QUAD[8..12], "blue the bottom-left");
        assert_eq!(texel(3, 3), &QUAD[12..16], "yellow the bottom-right");
    }

    #[test]
    fn a_wholly_transparent_cell_keeps_its_colour_instead_of_dividing_by_zero() {
        let clear = [
            0xFF, 0x00, 0x00, 0x00, //
            0xFF, 0x00, 0x00, 0x00, //
            0xFF, 0x00, 0x00, 0x00, //
            0xFF, 0x00, 0x00, 0x00,
        ];
        assert_eq!(resample(&clear, 2, 2, 1), [0xFF, 0x00, 0x00, 0x00]);
    }

    #[test]
    fn alpha_weighting_keeps_a_transparent_neighbour_from_tinting_an_opaque_one() {
        // One opaque red texel beside three fully transparent black ones. The
        // colour under a transparent texel is not colour anybody authored, so
        // the average must be red — an unweighted mean would drag it to a
        // quarter-strength red that no texel of the source holds.
        let fringe = [
            0xFF, 0x00, 0x00, 0xFF, //
            0x00, 0x00, 0x00, 0x00, //
            0x00, 0x00, 0x00, 0x00, //
            0x00, 0x00, 0x00, 0x00,
        ];
        let one = resample(&fringe, 2, 2, 1);
        assert_eq!(
            &one[..3],
            &[0xFF, 0x00, 0x00],
            "the colour is the opaque texel's"
        );
        assert_eq!(
            one[3], 64,
            "the coverage is a quarter, which is what alpha carries"
        );
    }

    /// A non-square source lands on the square extent without a panic or a
    /// stride mistake: a 4×2 image averaged onto 2² takes 2×1 cells.
    #[test]
    fn a_wide_source_is_read_at_its_own_stride() {
        let wide = [
            0xFF, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF, 0x00, 0x00, 0x00, 0xFF, 0x00, 0x00,
            0x00, 0xFF, //
            0xFF, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF, 0x00, 0x00, 0x00, 0xFF, 0x00, 0x00,
            0x00, 0xFF,
        ];
        let two = resample(&wide, 4, 2, 2);
        assert_eq!(&two[0..4], &[0xFF, 0xFF, 0xFF, 0xFF], "the left is white");
        assert_eq!(&two[4..8], &[0x00, 0x00, 0x00, 0xFF], "the right is black");
        assert_eq!(&two[8..12], &[0xFF, 0xFF, 0xFF, 0xFF]);
        assert_eq!(&two[12..16], &[0x00, 0x00, 0x00, 0xFF]);
    }

    /// The chain's length and each level's side agree with the seam's own
    /// count, including for an extent that is not a power of two.
    #[test]
    fn a_chain_halves_down_to_one_texel_and_agrees_with_the_seam() {
        for extent in [1u32, 2, 3, 4, 5, 8, 100] {
            let level0 = vec![0x80u8; extent as usize * extent as usize * 4];
            let below = chain(&level0, extent);
            let full = Extent3d::d2(extent, extent).full_mip_levels(ImageType::D2);
            assert_eq!(
                below.len() as u32 + 1,
                full,
                "an extent of {extent} has {full} levels including its own"
            );
            for (index, level) in below.iter().enumerate() {
                let side = level_extent(extent, index as u32 + 1);
                assert_eq!(
                    level.len(),
                    side as usize * side as usize * 4,
                    "level {} of a {extent}² layer is {side}²",
                    index + 1
                );
            }
            assert_eq!(below.last().map(Vec::len), (extent > 1).then_some(4));
        }
    }

    /// Each level is the box of the one above it: a 4×4 checker's first level
    /// is four mid greys, and its last is one.
    #[test]
    fn each_level_is_the_linear_average_of_the_one_above() {
        let mut checker = Vec::with_capacity(4 * 4 * 4);
        for y in 0..4u8 {
            for x in 0..4u8 {
                let white = (x + y) % 2 == 0;
                let value = if white { 0xFF } else { 0x00 };
                checker.extend_from_slice(&[value, value, value, 0xFF]);
            }
        }
        let below = chain(&checker, 4);
        assert_eq!(below.len(), 2);
        for (index, level) in below.iter().enumerate() {
            for texel in level.chunks_exact(4) {
                assert_eq!(texel[3], 0xFF);
                for channel in &texel[..3] {
                    assert!(
                        (i32::from(*channel) - 188).abs() <= 1,
                        "level {} holds {channel}, not the linear mid grey",
                        index + 1
                    );
                }
            }
        }
    }

    #[test]
    fn level_extent_never_reaches_zero() {
        assert_eq!(level_extent(5, 0), 5);
        assert_eq!(level_extent(5, 1), 2);
        assert_eq!(level_extent(5, 2), 1);
        assert_eq!(level_extent(5, 3), 1);
        assert_eq!(level_extent(1, 40), 1);
        assert_eq!(level_extent(u32::MAX, 31), 1);
    }
}
