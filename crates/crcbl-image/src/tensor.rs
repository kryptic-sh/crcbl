//! Images as a vision model's input: made opaque, then normalised floats.
//!
//! # llama.cpp's preprocessing
//!
//! A caller whose input must equal what llama.cpp's `mtmd` feeds a vision
//! encoder takes four steps from the same decoded pixels:
//!
//! 1. decode the image to an [`Rgba8Image`];
//! 2. make it [`opaque`], since llama.cpp loads with
//!    `stbi_load_from_memory(..., 3)`, which drops alpha rather than
//!    compositing it;
//! 3. resize it with [`resize_pillow`] and the model's filter
//!    ([`PillowFilter::Bicubic`] for most);
//! 4. turn it into floats with [`normalised_rgb`] and the model's mean and
//!    standard deviation.
//!
//! The last step is `clip_image_f32::from_u8` followed by `normalize` from
//! `llama.cpp/tools/mtmd/clip-impl.h`, operation for operation in `f32`.
//!
//! This reproduces llama.cpp only from the same decoded pixels. llama.cpp
//! decodes with stb_image. This crate's PNG and BMP decoders have agreed
//! with it on every variant tested, but its JPEG decoder is
//! libjpeg-turbo's, not stb's: the two differ by a few levels on typical
//! files and by more on some chroma-subsampled ones.
//!
//! The steps a model adds are the caller's: how it picks the target size,
//! any padding, and its mean and deviation, each as that model's
//! preprocessing in `llama.cpp/tools/mtmd/mtmd-image.cpp` does it. The
//! output here is interleaved, like `clip_image_f32`; llama.cpp's encoder
//! input tensor is planar.
//!
//! [`resize_pillow`]: crate::resize::resize_pillow
//! [`PillowFilter::Bicubic`]: crate::resize::PillowFilter::Bicubic

use crate::Rgba8Image;

/// `image` with every alpha set to opaque and its colour kept, in its own
/// buffer.
///
/// This is what llama.cpp's loader gives by asking for three channels: the
/// alpha is dropped, not composited over a background, so a fully
/// transparent pixel keeps whatever colour it stored. Do it before
/// [`resize_pillow`], which premultiplies and would otherwise let a
/// translucent pixel's colour count for less.
///
/// [`resize_pillow`]: crate::resize::resize_pillow
#[must_use]
pub fn opaque(image: Rgba8Image) -> Rgba8Image {
    let (width, height) = (image.width(), image.height());
    let mut pixels = image.into_pixels();
    for pixel in pixels.as_chunks_mut::<4>().0 {
        pixel[3] = u8::MAX;
    }
    Rgba8Image::from_sized(width, height, pixels)
}

/// `image`'s colour as interleaved `r, g, b` floats, rows top to bottom,
/// each channel `c` scaled to `0..=1` and then normalised as
/// `(v - mean[c]) / std[c]`: `width * height * 3` values, alpha ignored.
///
/// Equal bit for bit to llama.cpp's `clip_image_f32::from_u8` followed by
/// `normalize` built with default floating-point flags: each sample is
/// divided by 255 in `f32`, then has the mean subtracted and is divided by
/// the deviation, every step rounded to `f32` in the same order and nothing
/// fused. A build that relaxes float semantics does not keep that order:
/// with `-ffast-math`, or the `-ffp-model=fast` that llama.cpp's own arm64
/// toolchain files set, many values differ from this by about one ulp. Make the image [`opaque`] and resize
/// it with [`resize_pillow`] first, as the [module documentation](self)
/// sets out, for llama.cpp's input.
///
/// A zero in `std` gives infinities (or NaN where the mean is met exactly),
/// as llama.cpp's does; the deviations are the model's, and this does not
/// second-guess them.
///
/// No [`Limits`] are taken: the output is a fixed multiple of an image that
/// already passed its own, and llama.cpp's recipe resizes to the encoder's
/// size before this step.
///
/// [`resize_pillow`]: crate::resize::resize_pillow
/// [`Limits`]: crate::Limits
#[must_use]
pub fn normalised_rgb(image: &Rgba8Image, mean: [f32; 3], std: [f32; 3]) -> Vec<f32> {
    image
        .pixels()
        .as_chunks::<4>()
        .0
        .iter()
        .flat_map(|pixel| [0, 1, 2].map(|c| (f32::from(pixel[c]) / 255.0 - mean[c]) / std[c]))
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::Limits;
    use crate::resize::{PillowFilter, resize_pillow};

    /// SigLIP's mean and deviation, every channel alike.
    const SIGLIP: ([f32; 3], [f32; 3]) = ([0.5; 3], [0.5; 3]);

    /// Gemma 4's, which leaves its `2x - 1` scale to the graph.
    const GEMMA4: ([f32; 3], [f32; 3]) = ([0.0; 3], [1.0; 3]);

    /// CLIP's (OpenAI) per-channel mean and deviation.
    const CLIP: ([f32; 3], [f32; 3]) = (
        [0.481_454_66, 0.457_827_5, 0.408_210_73],
        [0.268_629_54, 0.261_302_6, 0.275_777_1],
    );

    /// The same formula, each operation done in `f64` and rounded to `f32`.
    /// For a sum or a quotient of two `f32`s that rounding is exact (`f64`
    /// carries more than twice `f32`'s precision), so this equals the `f32`
    /// arithmetic bit for bit without sharing its code.
    fn by_f64(sample: u8, mean: f32, std: f32) -> f32 {
        let v = (f64::from(sample) / 255.0) as f32;
        let centred = (f64::from(v) - f64::from(mean)) as f32;
        (f64::from(centred) / f64::from(std)) as f32
    }

    /// Every sample value in every channel, under many alphas.
    fn every_sample() -> Rgba8Image {
        let pixels = (0..=255u8)
            .flat_map(|v| {
                [
                    v,
                    v.wrapping_add(85),
                    v.wrapping_add(170),
                    v.wrapping_mul(7),
                ]
            })
            .collect();
        Rgba8Image::new(256, 1, pixels).expect("sized")
    }

    #[test]
    fn opaque_keeps_colour_and_sets_alpha() {
        let image = Rgba8Image::new(2, 1, vec![200, 10, 10, 0, 1, 2, 3, 128]).expect("sized");
        let made = opaque(image);
        assert_eq!((made.width(), made.height()), (2, 1));
        assert_eq!(made.pixels(), [200, 10, 10, 255, 1, 2, 3, 255]);
    }

    #[test]
    fn siglip_maps_black_and_white_to_minus_one_and_one() {
        let image = Rgba8Image::new(2, 1, vec![0, 0, 0, 255, 255, 255, 255, 0]).expect("sized");
        let (mean, std) = SIGLIP;
        assert_eq!(
            normalised_rgb(&image, mean, std),
            [-1.0, -1.0, -1.0, 1.0, 1.0, 1.0]
        );
    }

    /// White under CLIP, by hand: `(1 - mean) / std` per channel.
    #[test]
    fn clip_maps_white_by_its_own_channel() {
        let image = Rgba8Image::new(1, 1, vec![255; 4]).expect("sized");
        let (mean, std) = CLIP;
        let white = normalised_rgb(&image, mean, std);
        for (got, want) in white.iter().zip([1.930_336_3, 2.074_883_8, 2.145_897]) {
            assert!((got - want).abs() < 1e-6, "{got} against {want}");
        }
    }

    #[test]
    fn every_sample_is_the_formula_bit_for_bit_and_alpha_is_ignored() {
        let image = every_sample();
        for (mean, std) in [GEMMA4, SIGLIP, CLIP] {
            let got = normalised_rgb(&image, mean, std);
            assert_eq!(got.len(), image.pixels().len() / 4 * 3);
            for (pixel, floats) in image
                .pixels()
                .as_chunks::<4>()
                .0
                .iter()
                .zip(got.as_chunks::<3>().0)
            {
                for c in 0..3 {
                    assert_eq!(
                        floats[c].to_bits(),
                        by_f64(pixel[c], mean[c], std[c]).to_bits(),
                        "{pixel:?} channel {c}"
                    );
                }
            }
        }
    }

    /// The case the resize review raised: a transparent red pixel beside
    /// opaque green and blue, grown 3 to 5 wide. Dropping alpha as llama.cpp
    /// does lets the red overshoot past its own value; premultiplying would
    /// let it count for nothing.
    #[test]
    fn opaque_then_resize_pillow_gives_the_alpha_dropped_answer() {
        let image = Rgba8Image::new(
            3,
            1,
            vec![200, 10, 10, 0, 10, 200, 10, 255, 10, 10, 200, 255],
        )
        .expect("sized");
        let grown = |image: &Rgba8Image| {
            resize_pillow(image, 5, 1, PillowFilter::Bicubic, &Limits::DEFAULT).expect("resizes")
        };
        let dropped = grown(&opaque(image.clone()));
        assert_eq!(dropped.pixels()[..4], [214, 0, 10, 255]);
        assert_ne!(grown(&image).pixels()[..3], [214, 0, 10]);
    }
}
