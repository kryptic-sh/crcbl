//! The square of pixels a drawing is composited into, and the layers group
//! effects are drawn on.
//!
//! # Premultiplied, in sRGB values
//!
//! Pixels are premultiplied RGBA8, the form source-over composites in
//! without a division, and blending runs on the sRGB-encoded values
//! themselves: SVG's `color-interpolation` defaults to `sRGB`, which is what
//! every renderer (resvg among them) composites in. The finished square is
//! divided back out to straight alpha, the form [`Rgba8Image`] holds.

use crate::Rgba8Image;

/// A straight-alpha colour in `0..=1`, as paint produces it.
pub(super) type Rgba = [f32; 4];

/// A square of premultiplied RGBA8 pixels.
#[derive(Clone, Debug)]
pub(super) struct Canvas {
    side: u32,
    pixels: Vec<u8>,
}

fn byte(value: f32) -> u8 {
    // `as` saturates and sends NaN to zero.
    (value * 255.0 + 0.5) as u8
}

impl Canvas {
    /// A transparent `side`-square; the caller has checked its size against
    /// the limits.
    pub fn new(side: u32) -> Self {
        Self {
            side,
            pixels: vec![0; side as usize * side as usize * 4],
        }
    }

    /// Composites `color` source-over at each pixel, scaled by that pixel's
    /// `coverage`. `color` is asked for only where coverage is not zero.
    pub fn paint(&mut self, coverage: &[f32], mut color: impl FnMut(usize) -> Option<Rgba>) {
        for (index, (&cover, pixel)) in coverage
            .iter()
            .zip(self.pixels.as_chunks_mut::<4>().0)
            .enumerate()
        {
            if cover <= 0.0 {
                continue;
            }
            let Some([r, g, b, a]) = color(index) else {
                continue;
            };
            let alpha = a * cover;
            if alpha <= 0.0 {
                continue;
            }
            over(pixel, [r * alpha, g * alpha, b * alpha, alpha]);
        }
    }

    /// Composites `layer` source-over, each of its pixels scaled by
    /// `factor(pixel)`: a group's opacity, clip and mask.
    pub fn composite(&mut self, layer: &Self, mut factor: impl FnMut(usize) -> f32) {
        for (index, (source, pixel)) in layer
            .pixels
            .as_chunks::<4>()
            .0
            .iter()
            .zip(self.pixels.as_chunks_mut::<4>().0)
            .enumerate()
        {
            if source[3] == 0 {
                continue;
            }
            let k = factor(index).clamp(0.0, 1.0) / 255.0;
            if k <= 0.0 {
                continue;
            }
            over(pixel, source.map(|channel| f32::from(channel) * k));
        }
    }

    /// Each pixel's alpha in `0..=1`.
    pub fn alpha(&self, index: usize) -> f32 {
        f32::from(self.pixels[index * 4 + 3]) / 255.0
    }

    /// Each pixel's luminance times its alpha in `0..=1`: the value a
    /// luminance mask takes, from the coefficients SVG's masking chapter
    /// gives, applied to the premultiplied channels so the alpha is already
    /// in.
    pub fn luminance(&self, index: usize) -> f32 {
        let pixel = &self.pixels[index * 4..index * 4 + 3];
        (0.2125 * f32::from(pixel[0]) + 0.7154 * f32::from(pixel[1]) + 0.0721 * f32::from(pixel[2]))
            / 255.0
    }

    /// The square as straight-alpha RGBA8.
    pub fn into_image(self) -> Rgba8Image {
        let side = self.side;
        let mut pixels = self.pixels;
        for pixel in pixels.as_chunks_mut::<4>().0 {
            let alpha = pixel[3];
            if alpha == 0 {
                *pixel = [0; 4];
            } else if alpha < 255 {
                for channel in &mut pixel[..3] {
                    let straight =
                        (u32::from(*channel) * 255 + u32::from(alpha) / 2) / u32::from(alpha);
                    *channel = straight.min(255) as u8;
                }
            }
        }
        Rgba8Image::from_sized(side, side, pixels)
    }
}

/// `source` (premultiplied, `0..=1`) over the premultiplied byte `pixel`.
fn over(pixel: &mut [u8; 4], source: [f32; 4]) {
    let keep = 1.0 - source[3];
    for (channel, value) in pixel.iter_mut().zip(source) {
        *channel = byte(value + f32::from(*channel) / 255.0 * keep);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// **Source-over composites by its formula in premultiplied values, and
    /// the result comes out straight**: half-transparent red over opaque blue
    /// is (½, 0, ½) opaque; half-covered opaque green alone is half-alpha
    /// green.
    #[test]
    fn source_over_follows_its_formula() {
        let mut canvas = Canvas::new(2);
        canvas.paint(&[1.0, 0.0, 0.0, 0.0], |_| Some([0.0, 0.0, 1.0, 1.0]));
        canvas.paint(&[1.0, 0.0, 0.0, 0.0], |_| Some([1.0, 0.0, 0.0, 0.5]));
        canvas.paint(&[0.0, 0.5, 0.0, 0.0], |_| Some([0.0, 1.0, 0.0, 1.0]));
        let image = canvas.into_image();
        assert_eq!(&image.pixels()[..4], &[128, 0, 128, 255]);
        assert_eq!(&image.pixels()[4..8], &[0, 255, 0, 128]);
        assert_eq!(&image.pixels()[8..], &[0; 8]);
    }

    /// **A layer composites scaled by its factor**: an opaque red layer at a
    /// quarter onto nothing is red at a quarter alpha.
    #[test]
    fn a_layer_composites_scaled() {
        let mut layer = Canvas::new(1);
        layer.paint(&[1.0], |_| Some([1.0, 0.0, 0.0, 1.0]));
        let mut canvas = Canvas::new(1);
        canvas.composite(&layer, |_| 0.25);
        assert_eq!(canvas.alpha(0), 64.0 / 255.0);
        assert_eq!(canvas.into_image().pixels(), &[255, 0, 0, 64]);
    }

    /// **Luminance weighs the channels by the masking chapter's
    /// coefficients**, alpha included.
    #[test]
    fn luminance_uses_the_masking_coefficients() {
        let mut canvas = Canvas::new(1);
        canvas.paint(&[1.0], |_| Some([0.0, 1.0, 0.0, 1.0]));
        assert!((canvas.luminance(0) - 0.7154).abs() < 1e-6);
        let mut half = Canvas::new(1);
        half.paint(&[1.0], |_| Some([1.0, 1.0, 1.0, 0.5]));
        assert!((half.luminance(0) - 128.0 / 255.0).abs() < 1e-3);
    }
}
