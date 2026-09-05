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
//! [`resample`] is the one filter for a *colour* page, and it is also what
//! `crcbl-scene`'s glTF importer packs real textures onto the base-colour page
//! with: a box average in linear light, weighted by alpha. One filter for both
//! jobs, so a texture that was resampled onto the page and then mipped was
//! averaged the same way twice.
//!
//! [`normal_resample`] and [`normal_chain`] are the same pair for the **normal**
//! page, and they are a second filter rather than a flag on the first because a
//! normal texel is not a colour: no transfer curve, no alpha weighting, and a
//! renormalise after the average. `docs/plan/44-lighting.md`'s rung 2 is where
//! that is argued.
//!
//! [`linear_resample`] and [`linear_chain`] are the third pair, for the
//! **metallic-roughness-occlusion** page: a plain box mean of four independent
//! linear channels, with neither the colour filter's transfer curve and alpha
//! weight nor the normal filter's renormalise. A roughness is a number and the
//! channels beside it are different numbers, so nothing about them may be
//! coupled by the mipper.

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

/// Resample `pixels`, a tightly packed `width × height` RGBA8 **normal** image,
/// onto a square `extent` — in linear light, with no transfer curve and a
/// renormalise after the average.
///
/// [`resample`]'s box filter over a different kind of value, and every one of
/// the three differences is `docs/plan/44-lighting.md`'s rung 2:
///
/// * **No sRGB decode.** A normal texel is a direction stored as `n * 0.5 +
///   0.5`, not a colour, and pushing it through the IEC transfer function is
///   wrong by a gamma — which shows up as a downscaled map leaning further off
///   vertical than the one it was built from.
/// * **No alpha weighting.** A normal page has no meaningful alpha to weight by;
///   the channel is averaged plainly and read by nothing.
/// * **Renormalised.** The mean of unit vectors is shorter than one, so an
///   averaged texel re-encoded as-is is a normal the shader then stretches back
///   to unit length along whatever direction the shortening left. That plan's
///   rung 4 is what turns the *length* the average lost into roughness; until it
///   lands, the honest thing is to hand the device a unit vector rather than a
///   short one, and `docs/backlog.md` carries the missing half.
///
/// A cell whose decoded vectors cancel — opposing normals, which a real map can
/// hold at a crease — has no direction left to renormalise, and gets the neutral
/// `(0, 0, 1)` rather than a `NaN`.
///
/// **A cell covering exactly one source texel is copied**, not decoded and
/// re-encoded. Nothing was averaged, so there is no length to put back, and the
/// round trip would move every texel an eight-bit encoding could not make a unit
/// vector out of — which is most of them. So an image the page does not have to
/// resize arrives byte for byte, and an upscale is nearest-neighbour on
/// [`resample`]'s terms.
///
/// # Panics
///
/// [`resample`]'s conditions exactly.
#[must_use]
pub fn normal_resample(pixels: &[u8], width: u32, height: u32, extent: u32) -> Vec<u8> {
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
            let at = (y as usize * extent as usize + x as usize) * 4;
            if y1 - y0 == 1 && x1 - x0 == 1 {
                // **One source texel is copied, not renormalised.** Nothing has
                // been averaged, so there is no length to put back — and a
                // decode, renormalise and re-encode of a single texel moves it:
                // `(1, -1, -1)` is not a unit vector, and an authored map is
                // full of texels that are not, because eight bits cannot hold
                // one. An unresized layer therefore reaches the device byte for
                // byte, which is what makes a page the caller built exactly the
                // page the shader samples.
                let from = (y0 as usize * width as usize + x0 as usize) * 4;
                out[at..at + 4].copy_from_slice(&pixels[from..from + 4]);
                continue;
            }
            let mut sum = [0.0f32; 3];
            let mut alpha = 0.0f32;
            let mut texels = 0.0f32;
            for sy in y0..y1 {
                for sx in x0..x1 {
                    let from = (sy as usize * width as usize + sx as usize) * 4;
                    for (channel, axis) in sum.iter_mut().enumerate() {
                        *axis += f32::from(pixels[from + channel]) / 255.0 * 2.0 - 1.0;
                    }
                    alpha += f32::from(pixels[from + 3]) / 255.0;
                    texels += 1.0;
                }
            }
            let length = (sum[0] * sum[0] + sum[1] * sum[1] + sum[2] * sum[2]).sqrt();
            let unit = if length > 0.0 {
                [sum[0] / length, sum[1] / length, sum[2] / length]
            } else {
                [0.0, 0.0, 1.0]
            };
            for (channel, axis) in unit.iter().enumerate() {
                out[at + channel] = quantise(axis * 0.5 + 0.5);
            }
            out[at + 3] = quantise(alpha / texels);
        }
    }
    out
}

/// The levels below a square `extent`-wide RGBA8 **normal** layer, on
/// [`chain`]'s terms and through [`normal_resample`].
///
/// # Panics
///
/// [`normal_resample`]'s conditions on `level0` and `extent`.
#[must_use]
pub fn normal_chain(level0: &[u8], extent: u32) -> Vec<Vec<u8>> {
    let mut levels: Vec<Vec<u8>> = Vec::new();
    let mut wide = extent;
    while wide > 1 {
        let below = level_extent(wide, 1);
        let level = {
            let above = levels.last().map_or(level0, Vec::as_slice);
            normal_resample(above, wide, wide, below)
        };
        levels.push(level);
        wide = below;
    }
    levels
}

/// Resample `pixels`, a tightly packed `width × height` RGBA8 image of
/// **independent linear channels**, onto a square `extent`.
///
/// [`resample`]'s box filter with both of its colour steps removed, because
/// neither applies to a channel that is a number rather than a colour: no sRGB
/// decode, since the bytes are already linear, and no alpha weighting, since
/// the four channels are unrelated quantities and one of them is not an
/// opacity. Every channel — the fourth included — is the plain mean of the
/// source texels its cell covers.
///
/// **This is the metallic-roughness-occlusion page's filter**, and averaging
/// glTF's packed occlusion, roughness and metallic is exactly what a box mip of
/// them means: a minified texel is the area's mean roughness, not its mean
/// through a transfer curve. [`normal_resample`] cannot serve, because it
/// renormalises the first three channels as one vector — which would couple a
/// surface's roughness to its metalness.
///
/// **A cell covering exactly one source texel is copied**, on
/// [`normal_resample`]'s terms and for a weaker version of its reason: the
/// average of one value is that value, and the copy makes an unresized layer
/// arrive byte for byte without depending on this module's own rounding.
///
/// # Panics
///
/// [`resample`]'s conditions exactly.
#[must_use]
pub fn linear_resample(pixels: &[u8], width: u32, height: u32, extent: u32) -> Vec<u8> {
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
            let at = (y as usize * extent as usize + x as usize) * 4;
            if y1 - y0 == 1 && x1 - x0 == 1 {
                let from = (y0 as usize * width as usize + x0 as usize) * 4;
                out[at..at + 4].copy_from_slice(&pixels[from..from + 4]);
                continue;
            }
            let mut sum = [0.0f32; 4];
            let mut texels = 0.0f32;
            for sy in y0..y1 {
                for sx in x0..x1 {
                    let from = (sy as usize * width as usize + sx as usize) * 4;
                    for (channel, total) in sum.iter_mut().enumerate() {
                        *total += f32::from(pixels[from + channel]) / 255.0;
                    }
                    texels += 1.0;
                }
            }
            for (channel, total) in sum.iter().enumerate() {
                out[at + channel] = quantise(total / texels);
            }
        }
    }
    out
}

/// The levels below a square `extent`-wide RGBA8 layer of independent linear
/// channels, on [`chain`]'s terms and through [`linear_resample`].
///
/// # Panics
///
/// [`linear_resample`]'s conditions on `level0` and `extent`.
#[must_use]
pub fn linear_chain(level0: &[u8], extent: u32) -> Vec<Vec<u8>> {
    let mut levels: Vec<Vec<u8>> = Vec::new();
    let mut wide = extent;
    while wide > 1 {
        let below = level_extent(wide, 1);
        let level = {
            let above = levels.last().map_or(level0, Vec::as_slice);
            linear_resample(above, wide, wide, below)
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

    /// The neutral normal texel survives a downscale, which is the one property
    /// of this filter that every material naming no map depends on.
    ///
    /// **The chain keeps a flat map flat, and every level of a map that is not
    /// flat is still a unit vector.**
    ///
    /// The first half is the weaker claim and is here for the reason a constant
    /// image is worth checking at all: it is the one input whose right answer
    /// nobody can argue about. It does not discriminate between the two filters
    /// — a constant image survives any linear filter, `resample` included — so
    /// the second half is what holds `normal_chain` to *its* filter. A cell of
    /// two normals leaning opposite ways averages to something far short of
    /// unit length, and only the renormalise inside [`normal_resample`] puts the
    /// length back; a chain built through the colour filter emits the short
    /// vector, and a short shading normal is a surface that is quietly too dark.
    #[test]
    fn a_normal_chain_keeps_a_flat_map_flat_and_every_level_unit() {
        let neutral = [0x80u8, 0x80, 0xFF, 0xFF];
        let level0 = neutral.repeat(4 * 4);
        for level in normal_chain(&level0, 4) {
            for texel in level.chunks_exact(4) {
                assert_eq!(texel, neutral, "a flat map stopped being flat");
            }
        }

        // Columns leaning hard `+u` and hard `-u`, so every cell the chain
        // averages holds a pair that very nearly cancels.
        let leaning: Vec<u8> = (0..4 * 4)
            .flat_map(|texel| {
                if texel % 2 == 0 {
                    [0xFFu8, 0x80, 0x80, 0xFF]
                } else {
                    [0x00, 0x80, 0x80, 0xFF]
                }
            })
            .collect();
        for (index, level) in normal_chain(&leaning, 4).into_iter().enumerate() {
            for texel in level.chunks_exact(4) {
                let decoded = [
                    f32::from(texel[0]) / 255.0 * 2.0 - 1.0,
                    f32::from(texel[1]) / 255.0 * 2.0 - 1.0,
                    f32::from(texel[2]) / 255.0 * 2.0 - 1.0,
                ];
                let length =
                    (decoded[0] * decoded[0] + decoded[1] * decoded[1] + decoded[2] * decoded[2])
                        .sqrt();
                // An eight-bit encoding of a unit vector is not exactly unit;
                // half a level on each of three axes is what this allows.
                assert!(
                    (length - 1.0).abs() <= 3.0 / 255.0,
                    "level {index} holds a normal of length {length}, and a short shading \
                     normal is a surface that is quietly too dark"
                );
            }
        }
    }

    /// **A layer the page does not resize arrives byte for byte.**
    ///
    /// The decode is `t * 2 - 1` and eight bits cannot encode most unit
    /// vectors, so a decode-renormalise-re-encode round trip moves nearly every
    /// texel of a real map — and an authored level 0 that reached the device
    /// changed is a page the caller did not build. The single-texel copy in
    /// [`normal_resample`] is what makes this hold.
    #[test]
    fn a_normal_layer_the_page_does_not_resize_is_copied() {
        // `(1, -1, -1)` before normalising, which is the case that would move:
        // its length is `sqrt(3)`, so a round trip would land it on `0xC9`.
        let awkward = [0xFFu8, 0x00, 0x00, 0xFF];
        let source = awkward.repeat(2 * 2);
        assert_eq!(normal_resample(&source, 2, 2, 2), source);
        // And an upscale is nearest-neighbour, on `resample`'s terms: every
        // destination cell covers one source texel, so every one is a copy.
        assert_eq!(normal_resample(&source, 2, 2, 4), awkward.repeat(4 * 4));
    }

    /// Averaging two opposite normals renormalises rather than emitting a
    /// zero-length one, and a cell that cancels exactly gets the neutral.
    #[test]
    fn averaged_normals_come_out_unit_length() {
        // Two texels leaning hard `+u` and two leaning hard `+v`: the mean
        // leans between them, and the renormalise is what stops the encoded
        // vector being the short one the average produced.
        let leaning = [
            0xFF, 0x80, 0x80, 0xFF, //
            0xFF, 0x80, 0x80, 0xFF, //
            0x80, 0xFF, 0x80, 0xFF, //
            0x80, 0xFF, 0x80, 0xFF,
        ];
        let one = normal_resample(&leaning, 2, 2, 1);
        let decoded: Vec<f32> = one[..3]
            .iter()
            .map(|lane| f32::from(*lane) / 255.0 * 2.0 - 1.0)
            .collect();
        let length = decoded.iter().map(|axis| axis * axis).sum::<f32>().sqrt();
        assert!(
            (length - 1.0).abs() < 4.0 / 255.0,
            "the averaged normal decoded to a length of {length}, and the whole point of \
             the renormalise is that it is one"
        );
        assert!(
            decoded[0] > 0.5 && decoded[1] > 0.5,
            "the mean of a `+u` pair and a `+v` pair leans towards both: {decoded:?}"
        );

        // And a cell whose vectors cancel exactly has no direction to recover,
        // so it takes the neutral rather than a `NaN` that would poison every
        // level below it.
        let opposed = [
            0xFF, 0x80, 0x80, 0xFF, //
            0x00, 0x80, 0x80, 0xFF, //
            0xFF, 0x80, 0x80, 0xFF, //
            0x00, 0x80, 0x80, 0xFF,
        ];
        // `0xFF` and `0x00` decode to `+1` and `-1`, and `0x80` to `1 / 255`
        // on both of the other axes — so the sum is not exactly zero and this
        // is the near-cancellation a real crease produces rather than the
        // contrived exact one. What it must not be is a `NaN`.
        let one = normal_resample(&opposed, 2, 2, 1);
        let decoded: Vec<f32> = one[..3]
            .iter()
            .map(|lane| f32::from(*lane) / 255.0 * 2.0 - 1.0)
            .collect();
        assert!(
            decoded.iter().all(|axis| axis.is_finite()),
            "a cancelling cell produced {decoded:?}"
        );
        let length = decoded.iter().map(|axis| axis * axis).sum::<f32>().sqrt();
        assert!(
            (length - 1.0).abs() < 4.0 / 255.0,
            "and it is still a unit vector: {length}"
        );
    }

    /// The two filters really are different filters, which is what a page whose
    /// normal layers went through the colour one would silently lose.
    #[test]
    fn the_colour_filter_and_the_normal_filter_disagree() {
        // A cell of two whites and two blacks. The colour filter averages in
        // linear light and re-encodes through the sRGB curve, landing near mid
        // grey's *encoding*; the normal filter averages `(1, 1, 1)` and
        // `(-1, -1, -1)`, which nearly cancel, and renormalises whatever is
        // left. Neither answer is wrong for its own kind of value, and that is
        // the point.
        let checker = [
            0xFF, 0xFF, 0xFF, 0xFF, //
            0x00, 0x00, 0x00, 0xFF, //
            0x00, 0x00, 0x00, 0xFF, //
            0xFF, 0xFF, 0xFF, 0xFF,
        ];
        assert_ne!(
            resample(&checker, 2, 2, 1),
            normal_resample(&checker, 2, 2, 1),
            "a normal page mipped through the colour filter would be wrong by a gamma and \
             nothing in a frame would report it"
        );
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
