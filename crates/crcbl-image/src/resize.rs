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
/// [`ImageError::Empty`] or [`ImageError::TooLarge`] if the new size breaks
/// `limits`.
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
