//! Resizing to the size an image is drawn at, and choosing that size.
//!
//! # The filter
//!
//! A separable Lanczos filter with three lobes, `sinc(x) · sinc(x / 3)` for
//! `|x| < 3`: the usual choice for photographs and icons alike, sharp without
//! the blockiness of a box or the softness of a triangle. Shrinking widens
//! the kernel by the shrink factor so every source pixel is weighed (the
//! approach Pillow's resampler takes); growing keeps it at one source pixel
//! per unit. Each output pixel's weights are normalised to sum to one, so a
//! flat colour stays exactly that colour at the edges, where the kernel is
//! cut off by the image's border.
//!
//! # In premultiplied light
//!
//! Filtering straight alpha would let a transparent pixel's colour, which
//! nothing ever sees, bleed into its opaque neighbours: a dark fringe round
//! every icon. So colour is multiplied by alpha before filtering and divided
//! back out after, and a transparent pixel contributes nothing but its
//! transparency.
//!
//! The negative lobes can overshoot; results are clamped to the channel's
//! range, and colour to its pixel's alpha, before they are stored.
//!
//! # llama.cpp's Pillow resampler, to the byte
//!
//! [`resize_pillow`] is a second path, for a caller whose output must equal
//! llama.cpp's exactly: a vision model's preprocessing, where the model was
//! trained on Pillow's pixels. It is named for, and held by the tests to,
//! llama.cpp's `resize_pillow`, a transcription of Pillow's `Resample.c`. It
//! differs from [`resize`] in more than the kernel. Weights are computed in
//! `f64`, normalised, and quantised to fixed point; the horizontal pass always
//! runs first and is rounded and clipped to `u8` before the vertical one
//! reads it; and an axis whose size does not change is not filtered at all.
//!
//! That is Pillow's own result only when Pillow takes the passes in the same
//! order. Pillow 13 was seen running the vertical pass first for some sizes
//! (a width that grows while the height shrinks), and there the two differ by
//! several levels; this follows llama.cpp, not Pillow, wherever they part.
//!
//! Alpha goes the way Pillow's `Image.resize` takes it for RGBA: colour is
//! premultiplied in integers before filtering and divided back out after, all
//! four channels filtered alike. On opaque input both conversions change
//! nothing. llama.cpp never sees alpha, since its loader drops it, so only an
//! opaque image comes out as llama.cpp's RGB would.

use crate::{ImageError, Limits, Rgba8Image};

/// The filter's half-width in source pixels at a scale of one.
const LANCZOS_LOBES: f32 = 3.0;

/// `sin(πx) / (πx)`, with its limit at zero.
fn sinc(x: f32) -> f32 {
    if x == 0.0 {
        1.0
    } else {
        let px = core::f32::consts::PI * x;
        px.sin() / px
    }
}

/// The Lanczos kernel with [`LANCZOS_LOBES`] lobes.
fn lanczos(x: f32) -> f32 {
    if x.abs() < LANCZOS_LOBES {
        sinc(x) * sinc(x / LANCZOS_LOBES)
    } else {
        0.0
    }
}

/// One output pixel's taps along an axis: the first source index and the
/// normalised weight of it and each one after.
#[derive(Debug)]
struct Taps {
    first: usize,
    weights: Vec<f32>,
}

/// The taps for every output pixel when an axis of `source` pixels becomes
/// `target` pixels.
fn taps(source: u32, target: u32) -> Vec<Taps> {
    let scale = source as f32 / target as f32;
    let filter_scale = scale.max(1.0);
    let support = LANCZOS_LOBES * filter_scale;
    (0..target)
        .map(|i| {
            let centre = (i as f32 + 0.5) * scale;
            let first = (centre - support).floor().max(0.0) as usize;
            let end = ((centre + support).ceil() as usize).min(source as usize);
            let mut weights: Vec<f32> = (first..end)
                .map(|j| lanczos((j as f32 + 0.5 - centre) / filter_scale))
                .collect();
            let total: f32 = weights.iter().sum();
            if total != 0.0 {
                for weight in &mut weights {
                    *weight /= total;
                }
            }
            Taps { first, weights }
        })
        .collect()
}

/// `image` resized to `width` × `height`; the same image when the size is
/// already that.
///
/// # Errors
///
/// [`ImageError::Empty`] or [`ImageError::TooLarge`] if the new size, or
/// the intermediate `width` × the source's height the horizontal pass
/// writes, breaks `limits`.
pub fn resize(
    image: &Rgba8Image,
    width: u32,
    height: u32,
    limits: &Limits,
) -> Result<Rgba8Image, ImageError> {
    let out_bytes = limits.rgba_bytes(width, height)?;
    if (width, height) == (image.width(), image.height()) {
        return Ok(image.clone());
    }
    // The horizontal pass always runs, even for an unchanged width.
    limits.rgba_bytes(width, image.height())?;
    let (source_width, source_height) = (image.width() as usize, image.height() as usize);
    let premultiplied: Vec<f32> = image
        .pixels()
        .as_chunks::<4>()
        .0
        .iter()
        .flat_map(|p| {
            let a = f32::from(p[3]) / 255.0;
            [
                f32::from(p[0]) * a,
                f32::from(p[1]) * a,
                f32::from(p[2]) * a,
                f32::from(p[3]),
            ]
        })
        .collect();

    // Across first, into `source_height` rows of `width` pixels.
    let across = taps(image.width(), width);
    let mut wide = vec![0.0f32; source_height * width as usize * 4];
    for y in 0..source_height {
        let row = &premultiplied[y * source_width * 4..(y + 1) * source_width * 4];
        let out = &mut wide[y * width as usize * 4..(y + 1) * width as usize * 4];
        for (pixel, taps) in out.as_chunks_mut::<4>().0.iter_mut().zip(&across) {
            for (k, weight) in taps.weights.iter().enumerate() {
                let source = &row[(taps.first + k) * 4..(taps.first + k) * 4 + 4];
                for c in 0..4 {
                    pixel[c] += source[c] * weight;
                }
            }
        }
    }

    // Then down, straight into the result.
    let down = taps(image.height(), height);
    let mut pixels = Vec::with_capacity(out_bytes);
    let stride = width as usize * 4;
    for taps in &down {
        for x in 0..width as usize {
            let mut sum = [0.0f32; 4];
            for (k, weight) in taps.weights.iter().enumerate() {
                let source = &wide[(taps.first + k) * stride + x * 4..][..4];
                for c in 0..4 {
                    sum[c] += source[c] * weight;
                }
            }
            pixels.extend_from_slice(&unpremultiply(sum));
        }
    }
    Ok(Rgba8Image::from_sized(width, height, pixels))
}

/// A filtered premultiplied pixel as straight RGBA8: alpha clamped to its
/// range, colour to its alpha, and colour divided back out.
fn unpremultiply([r, g, b, a]: [f32; 4]) -> [u8; 4] {
    let alpha = a.clamp(0.0, 255.0);
    let stored = alpha.round() as u8;
    if stored == 0 {
        return [0; 4];
    }
    let coverage = alpha / 255.0;
    let channel = |c: f32| (c.clamp(0.0, alpha) / coverage).round().min(255.0) as u8;
    [channel(r), channel(g), channel(b), stored]
}

/// The fractional bits of [`resize_pillow`]'s fixed-point weights, Pillow's
/// `PRECISION_BITS`: what an `i32` holds above a `u8` sample, keeping two
/// bits for the sum to overshoot into.
const PILLOW_PRECISION_BITS: u32 = 32 - 8 - 2;

/// A filter for [`resize_pillow`], each as Pillow defines it.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum PillowFilter {
    /// The triangle `1 - |x|`, Pillow's `BILINEAR`.
    Bilinear,
    /// The cubic convolution kernel with `a = -0.5`, Pillow's `BICUBIC`
    /// (PyTorch and OpenCV use `a = -0.75`, and differ from it).
    Bicubic,
    /// `sinc(x) · sinc(x / 3)`, Pillow's `LANCZOS`.
    Lanczos,
}

impl PillowFilter {
    /// The kernel's half-width in source pixels at a scale of one.
    const fn support(self) -> f64 {
        match self {
            Self::Bilinear => 1.0,
            Self::Bicubic => 2.0,
            Self::Lanczos => 3.0,
        }
    }

    /// The kernel at `x`, in `f64` as Pillow computes it.
    fn weight(self, x: f64) -> f64 {
        match self {
            Self::Bilinear => {
                let x = x.abs();
                if x < 1.0 { 1.0 - x } else { 0.0 }
            }
            Self::Bicubic => {
                const A: f64 = -0.5;
                let x = x.abs();
                if x < 1.0 {
                    ((A + 2.0) * x - (A + 3.0)) * x * x + 1.0
                } else if x < 2.0 {
                    (((x - 5.0) * x + 8.0) * x - 4.0) * A
                } else {
                    0.0
                }
            }
            Self::Lanczos => {
                let sinc = |v: f64| {
                    if v == 0.0 {
                        1.0
                    } else {
                        let pv = core::f64::consts::PI * v;
                        pv.sin() / pv
                    }
                };
                // Pillow's window is half-open.
                if (-3.0..3.0).contains(&x) {
                    sinc(x) * sinc(x / 3.0)
                } else {
                    0.0
                }
            }
        }
    }

    /// The fixed-point taps for every output pixel when an axis of `source`
    /// pixels becomes `target` pixels: Pillow's `precompute_coeffs` and
    /// `normalize_coeffs_8bpc`, each step in the order and precision it
    /// takes them, since a weight a rounding away changes output bytes.
    fn fixed_taps(self, source: u32, target: u32) -> Vec<FixedTaps> {
        let scale = f64::from(source) / f64::from(target);
        let filter_scale = scale.max(1.0);
        let support = self.support() * filter_scale;
        let inverse_scale = 1.0 / filter_scale;
        let one = f64::from(1u32 << PILLOW_PRECISION_BITS);
        (0..target)
            .map(|i| {
                let centre = (f64::from(i) + 0.5) * scale;
                // C's truncating casts; a negative start saturates to the
                // zero it would be clamped to.
                let first = (centre - support + 0.5) as usize;
                let end = ((centre + support + 0.5) as usize).min(source as usize);
                let mut weights: Vec<f64> = (first..end)
                    .map(|j| self.weight((j as f64 - centre + 0.5) * inverse_scale))
                    .collect();
                let total: f64 = weights.iter().sum();
                if total != 0.0 {
                    for weight in &mut weights {
                        *weight /= total;
                    }
                }
                let weights = weights
                    .iter()
                    .map(|&w| (w * one + if w < 0.0 { -0.5 } else { 0.5 }) as i32)
                    .collect();
                FixedTaps { first, weights }
            })
            .collect()
    }
}

/// One output pixel's taps along an axis for [`resize_pillow`]: the first
/// source index and the fixed-point weight of it and each one after.
#[derive(Debug)]
struct FixedTaps {
    first: usize,
    weights: Vec<i32>,
}

/// A fixed-point sum with Pillow's rounding: half added, shifted down,
/// clipped to a channel.
fn pillow_channel(sum: i32) -> u8 {
    (sum >> PILLOW_PRECISION_BITS).clamp(0, 255) as u8
}

/// The rounding half every fixed-point sum starts from.
const PILLOW_HALF: i32 = 1 << (PILLOW_PRECISION_BITS - 1);

/// `image` resized to `width` × `height` with `filter` by llama.cpp's
/// `resize_pillow`, whose name it keeps: equal byte for byte to llama.cpp on
/// an opaque image, and to Pillow's `Image.resize` only where Pillow runs
/// the horizontal pass first (see the module documentation); the same image
/// when the size is already that.
///
/// llama.cpp loads images as RGB, dropping alpha, and filters what was left.
/// This premultiplies instead, so a translucent pixel's colour counts for
/// less, and one with no alpha for nothing. A caller matching llama.cpp
/// makes the image opaque first, every alpha set to 255 and colour kept.
///
/// # Errors
///
/// [`ImageError::Empty`] or [`ImageError::TooLarge`] if the new size, or
/// the intermediate `width` × the source's height the horizontal pass
/// writes, breaks `limits`.
pub fn resize_pillow(
    image: &Rgba8Image,
    width: u32,
    height: u32,
    filter: PillowFilter,
    limits: &Limits,
) -> Result<Rgba8Image, ImageError> {
    limits.rgba_bytes(width, height)?;
    if (width, height) == (image.width(), image.height()) {
        return Ok(image.clone());
    }
    // Pillow's `RGBA` to `RGBa`: `MULDIV255`, colour times alpha over 255
    // rounded to nearest.
    let mut pixels: Vec<u8> = image
        .pixels()
        .as_chunks::<4>()
        .0
        .iter()
        .flat_map(|&[r, g, b, a]| {
            let times_alpha = |c: u8| {
                let t = u32::from(c) * u32::from(a) + 128;
                ((t >> 8) + t) >> 8
            };
            // At most 255 · 255 / 255.
            [
                times_alpha(r) as u8,
                times_alpha(g) as u8,
                times_alpha(b) as u8,
                a,
            ]
        })
        .collect();

    let source_height = image.height() as usize;
    if width != image.width() {
        limits.rgba_bytes(width, image.height())?;
        let source_stride = image.width() as usize * 4;
        let across = filter.fixed_taps(image.width(), width);
        let mut wide = Vec::with_capacity(source_height * width as usize * 4);
        for row in pixels.chunks_exact(source_stride) {
            for taps in &across {
                let mut sum = [PILLOW_HALF; 4];
                for (k, &weight) in taps.weights.iter().enumerate() {
                    let source = &row[(taps.first + k) * 4..][..4];
                    for c in 0..4 {
                        sum[c] += i32::from(source[c]) * weight;
                    }
                }
                wide.extend(sum.map(pillow_channel));
            }
        }
        pixels = wide;
    }
    if height != image.height() {
        let stride = width as usize * 4;
        let down = filter.fixed_taps(image.height(), height);
        let mut tall = Vec::with_capacity(height as usize * stride);
        let mut sum = vec![0i32; stride];
        for taps in &down {
            sum.fill(PILLOW_HALF);
            for (k, &weight) in taps.weights.iter().enumerate() {
                let row = &pixels[(taps.first + k) * stride..][..stride];
                for (total, &sample) in sum.iter_mut().zip(row) {
                    *total += i32::from(sample) * weight;
                }
            }
            tall.extend(sum.iter().map(|&total| pillow_channel(total)));
        }
        pixels = tall;
    }

    // And back, Pillow's `rgba2rgbA`: colour times 255 over alpha,
    // truncated and clipped, left as it is where alpha is full or none.
    for pixel in pixels.as_chunks_mut::<4>().0 {
        let alpha = u32::from(pixel[3]);
        if alpha != 0 && alpha != 255 {
            for c in &mut pixel[..3] {
                *c = (255 * u32::from(*c) / alpha).min(255) as u8;
            }
        }
    }
    Ok(Rgba8Image::from_sized(width, height, pixels))
}

/// The largest size with `size`'s aspect that fits inside `bounds`, growing
/// or shrinking: an icon drawn into a square, a picture into its slot. Each
/// side is at least one pixel, so an extreme aspect still has a size.
#[must_use]
pub fn fit_within(size: (u32, u32), bounds: (u32, u32)) -> (u32, u32) {
    let (width, height) = (u64::from(size.0.max(1)), u64::from(size.1.max(1)));
    let (max_width, max_height) = (u64::from(bounds.0.max(1)), u64::from(bounds.1.max(1)));
    // Whichever side meets its bound first decides, in exact integer arithmetic:
    // width / height against max_width / max_height, cross-multiplied.
    let (w, h) = if width * max_height >= height * max_width {
        (max_width, height * max_width / width)
    } else {
        (width * max_height / height, max_height)
    };
    // Each is at most its bound, which came from a u32.
    (w.max(1) as u32, h.max(1) as u32)
}

/// `size` unchanged if it already fits inside `bounds`, else
/// [`fit_within`]: a notification image shown at most so large, never
/// enlarged.
#[must_use]
pub fn shrink_to_fit(size: (u32, u32), bounds: (u32, u32)) -> (u32, u32) {
    if size.0 <= bounds.0 && size.1 <= bounds.1 && size.0 > 0 && size.1 > 0 {
        size
    } else {
        fit_within(size, bounds)
    }
}

/// Of several sizes offered for one picture (a tray icon's pixmaps, an ICO's
/// frames), the index of the one to draw at `target` pixels: the smallest
/// whose longer side is at least `target`, since shrinking loses less than
/// growing, or else the largest. `None` only for no sizes.
#[must_use]
pub fn nearest_size(sizes: &[(u32, u32)], target: u32) -> Option<usize> {
    let side = |index: usize| sizes[index].0.max(sizes[index].1);
    let large_enough = (0..sizes.len())
        .filter(|&index| side(index) >= target)
        .min_by_key(|&index| side(index));
    large_enough.or_else(|| (0..sizes.len()).max_by_key(|&index| side(index)))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn grey_row(values: &[u8]) -> Rgba8Image {
        let pixels = values.iter().flat_map(|&v| [v, v, v, 255]).collect();
        Rgba8Image::new(values.len() as u32, 1, pixels).expect("sized")
    }

    fn greys(image: &Rgba8Image) -> Vec<u8> {
        image
            .pixels()
            .as_chunks::<4>()
            .0
            .iter()
            .map(|p| p[0])
            .collect()
    }

    /// Values computed from the kernel's definition, not from this code: the
    /// weights of `sinc(x)·sinc(x/3)` at each output centre, cut at the border
    /// and normalised.
    #[test]
    fn growing_a_bright_pixel_follows_the_lanczos_kernel() {
        let grown = resize(&grey_row(&[0, 255, 0]), 6, 1, &Limits::DEFAULT).expect("resizes");
        assert_eq!(greys(&grown), [0, 63, 221, 221, 63, 0]);
    }

    /// Computed from the kernel's definition as above, with every `x` divided
    /// by the shrink; a kernel left one source pixel wide gives other values.
    #[test]
    fn shrinking_widens_the_kernel_by_the_shrink() {
        let shrunk = resize(
            &grey_row(&[0, 255, 255, 0, 0, 255, 255, 0]),
            4,
            1,
            &Limits::DEFAULT,
        )
        .expect("resizes");
        assert_eq!(greys(&shrunk), [162, 121, 121, 162]);
    }

    #[test]
    fn a_flat_colour_stays_flat_at_any_size() {
        let flat = Rgba8Image::new(5, 3, [30, 60, 90, 200].repeat(15)).expect("sized");
        for (width, height) in [(1, 1), (2, 7), (10, 6), (13, 2)] {
            let resized = resize(&flat, width, height, &Limits::DEFAULT).expect("resizes");
            for pixel in resized.pixels().as_chunks::<4>().0.iter() {
                assert_eq!(*pixel, [30, 60, 90, 200], "{width}x{height}");
            }
        }
    }

    #[test]
    fn a_transparent_pixel_lends_no_colour_to_its_neighbour() {
        let pair = Rgba8Image::new(2, 1, vec![255, 0, 0, 0, 0, 0, 255, 255]).expect("sized");
        let one = resize(&pair, 1, 1, &Limits::DEFAULT).expect("resizes");
        assert_eq!(one.pixels(), [0, 0, 255, 128]);
    }

    #[test]
    fn the_same_size_is_the_same_image_and_a_zero_size_is_refused() {
        let image = grey_row(&[1, 2, 3]);
        assert_eq!(resize(&image, 3, 1, &Limits::DEFAULT), Ok(image.clone()));
        assert_eq!(
            resize(&image, 0, 1, &Limits::DEFAULT),
            Err(ImageError::Empty {
                width: 0,
                height: 1
            })
        );
    }

    /// A tall source made wide: the source and the target are 40 bytes of
    /// RGBA8 each, but the horizontal pass's intermediate is ten by ten.
    #[test]
    fn resize_refuses_an_intermediate_past_the_limits() {
        let tall = hashed(1, 10);
        let limits = |max_bytes| Limits {
            max_width: 10,
            max_height: 10,
            max_bytes,
        };
        assert_eq!(
            resize(&tall, 10, 1, &limits(10 * 10 * 4 - 1)),
            Err(ImageError::TooLarge {
                width: 10,
                height: 10,
                limits: limits(10 * 10 * 4 - 1)
            })
        );
        let wide = resize(&tall, 10, 1, &limits(10 * 10 * 4)).expect("resizes");
        assert_eq!((wide.width(), wide.height()), (10, 1));
    }

    /// Every case's source pixels: an opaque image whose RGB bytes, in
    /// order, are a multiplicative hash of their index, noisy enough that
    /// every filter overshoots and clips somewhere.
    fn hashed(width: u32, height: u32) -> Rgba8Image {
        let pixels = (0..width * height)
            .flat_map(|p| {
                let byte = |c: u32| ((p * 3 + c).wrapping_mul(0x9E37_79B1) >> 24) as u8;
                [byte(0), byte(1), byte(2), 255]
            })
            .collect();
        Rgba8Image::new(width, height, pixels).expect("sized")
    }

    /// `fixtures/resize/llama-cpp.rgb` is llama.cpp's `resize_pillow`
    /// (`llama.cpp/tools/mtmd/mtmd-image.cpp` at 57291f2, Pillow's `Resample.c`
    /// transcribed) copied verbatim into a C++ harness that resized
    /// [`hashed`] sources for each case below in order and wrote the RGB
    /// bytes back to back.
    #[test]
    fn resize_pillow_matches_llama_cpp_byte_for_byte() {
        const EXPECTED: &[u8] = include_bytes!("../fixtures/resize/llama-cpp.rgb");
        // Source size, target size, filter.
        let cases = [
            ((23, 17), (10, 7), PillowFilter::Bicubic),
            ((7, 5), (16, 13), PillowFilter::Bicubic),
            ((20, 9), (13, 9), PillowFilter::Bicubic),
            ((11, 14), (11, 6), PillowFilter::Bicubic),
            ((1, 9), (3, 4), PillowFilter::Bicubic),
            ((9, 7), (4, 11), PillowFilter::Bicubic),
            ((23, 17), (10, 7), PillowFilter::Lanczos),
            ((23, 17), (10, 7), PillowFilter::Bilinear),
            // Rounding a negative weight any way but half away from zero
            // (adding a half, or truncating) reddens some of these and
            // none of the ones above; the bilinear one has no negative
            // weights, and is here as an enlargement.
            ((22, 46), (99, 29), PillowFilter::Bicubic),
            ((44, 34), (62, 107), PillowFilter::Bicubic),
            ((16, 12), (64, 48), PillowFilter::Bicubic),
            ((3, 2), (9, 4), PillowFilter::Bicubic),
            ((4, 3), (20, 6), PillowFilter::Lanczos),
            ((8, 4), (16, 8), PillowFilter::Lanczos),
            ((7, 5), (16, 13), PillowFilter::Bilinear),
        ];
        let mut expected = EXPECTED;
        for ((sw, sh), (w, h), filter) in cases {
            let resized =
                resize_pillow(&hashed(sw, sh), w, h, filter, &Limits::DEFAULT).expect("resizes");
            let (want, rest) = expected.split_at((w * h * 3) as usize);
            expected = rest;
            let rgb: Vec<u8> = resized
                .pixels()
                .as_chunks::<4>()
                .0
                .iter()
                .flat_map(|p| {
                    assert_eq!(p[3], 255, "{sw}x{sh} to {w}x{h} {filter:?}");
                    [p[0], p[1], p[2]]
                })
                .collect();
            assert_eq!(rgb, want, "{sw}x{sh} to {w}x{h} {filter:?}");
        }
        assert!(expected.is_empty(), "the fixture has cases left over");
    }

    /// Weights `1/2` each, so straight alpha would give half the red; the
    /// premultiplied blue is `255 · 1/2` rounded up, divided back by alpha.
    #[test]
    fn resize_pillow_premultiplies_as_pillow_does() {
        let pair = Rgba8Image::new(2, 1, vec![255, 0, 0, 0, 0, 0, 255, 255]).expect("sized");
        let one =
            resize_pillow(&pair, 1, 1, PillowFilter::Bicubic, &Limits::DEFAULT).expect("resizes");
        assert_eq!(one.pixels(), [0, 0, 255, 128]);
    }

    #[test]
    fn resize_pillow_keeps_the_size_and_refuses_what_limits_refuse() {
        let image = hashed(3, 2);
        let same = resize_pillow(&image, 3, 2, PillowFilter::Bicubic, &Limits::DEFAULT);
        assert_eq!(same, Ok(image.clone()));
        assert_eq!(
            resize_pillow(&image, 3, 0, PillowFilter::Bicubic, &Limits::DEFAULT),
            Err(ImageError::Empty {
                width: 3,
                height: 0
            })
        );
    }

    #[test]
    fn a_size_fits_its_bounds_keeping_its_aspect() {
        assert_eq!(fit_within((400, 100), (200, 100)), (200, 50));
        assert_eq!(fit_within((100, 400), (200, 100)), (25, 100));
        assert_eq!(fit_within((16, 16), (32, 32)), (32, 32));
        assert_eq!(fit_within((10_000, 1), (32, 32)), (32, 1));
        assert_eq!(shrink_to_fit((150, 80), (200, 100)), (150, 80));
        assert_eq!(shrink_to_fit((400, 100), (200, 100)), (200, 50));
    }

    #[test]
    fn the_nearest_size_is_the_smallest_large_enough_else_the_largest() {
        let sizes = [(16, 16), (48, 48), (22, 22), (32, 32)];
        assert_eq!(nearest_size(&sizes, 18), Some(2));
        assert_eq!(nearest_size(&sizes, 32), Some(3));
        assert_eq!(nearest_size(&sizes, 64), Some(1));
        assert_eq!(nearest_size(&[], 16), None);
    }
}
