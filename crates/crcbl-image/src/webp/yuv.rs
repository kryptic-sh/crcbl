//! A lossy frame's Y'CbCr 4:2:0 planes to RGB, as libwebp's default decode
//! makes it.
//!
//! RFC 9649 leaves the conversion open (BT.601 "SHOULD be used"), so these
//! are libwebp's, the decoder behind Pillow, browsers and most of the
//! desktop, so a file decodes here to exactly what it decodes to there:
//!
//! * Chroma is upsampled with libwebp's "fancy" upsampler (`UpsampleRgbaLinePair`
//!   in its `dsp/upsampling.c`), which interpolates between the four chroma
//!   samples around each luma pixel with weights of 9, 3, 3 and 1 sixteenths
//!   — the nearest sample, the one beside it, the one above or below, and
//!   the diagonal — and repeats the chroma plane's edge rows and columns. It
//!   computes this as two rounded steps on packed pairs; a single
//!   `(9a + 3b + 3c + d + 8) >> 4` is the same value, since `(⌊x / 8⌋ + n) / 2`
//!   and `(x + 8n) / 16` floor alike for any integer `n`, and at an edge, where
//!   it computes `(3a + b + 2) >> 2`, the weights fold to that exactly.
//! * The conversion is libwebp's fixed point (`VP8YUVToR` and friends in its
//!   `dsp/yuv.h`): BT.601's limited-range matrix with each product taken to
//!   eight fewer bits than its 14-bit constant, summed with a rounding offset
//!   folded into the constant, and six fractional bits left to drop.
//!
//! The tests hold this to exact equality with libwebp's output, through
//! Pillow, on committed files.

use super::vp8::Picture;

/// The luma scale, 255 / 219, in 14-bit fixed point.
const Y_SCALE: i32 = 19077;
/// Cr's weight in red, 1.596, in 14-bit fixed point.
const V_TO_R: i32 = 26149;
/// Cb's and Cr's weights in green, 0.392 and 0.813, in 14-bit fixed point.
const U_TO_G: i32 = 6419;
const V_TO_G: i32 = 13320;
/// Cb's weight in blue, 2.017, in 14-bit fixed point.
const U_TO_B: i32 = 33050;
/// Each channel's offset in the 6-bit fixed point of the sums: the luma
/// and chroma biases (16 and 128) through the matrix, with half of the last
/// bit for rounding.
const R_OFFSET: i32 = -14234;
const G_OFFSET: i32 = 8708;
const B_OFFSET: i32 = -17685;
/// The fractional bits of the sums.
const FRACTION_BITS: u32 = 6;

/// libwebp's `MultHi`: a product to eight fewer bits, as SIMD's high-half
/// multiply leaves it.
const fn mult_hi(value: i32, coefficient: i32) -> i32 {
    (value * coefficient) >> 8
}

/// libwebp's `VP8Clip8`: the integer part, clamped to a byte.
fn clip8(value: i32) -> u8 {
    (value >> FRACTION_BITS).clamp(0, 255) as u8
}

/// One pixel's RGB from its luma and upsampled chroma.
fn rgb(y: u8, u: u8, v: u8) -> [u8; 3] {
    let (y, u, v) = (i32::from(y), i32::from(u), i32::from(v));
    let luma = mult_hi(y, Y_SCALE);
    [
        clip8(luma + mult_hi(v, V_TO_R) + R_OFFSET),
        clip8(luma - mult_hi(u, U_TO_G) - mult_hi(v, V_TO_G) + G_OFFSET),
        clip8(luma + mult_hi(u, U_TO_B) + B_OFFSET),
    ]
}

/// The chroma sample nearest luma position `at` along one axis, and the
/// other one it interpolates with: the next for an odd position, the one
/// before for an even one, the edge repeated where there is none.
fn neighbours(at: usize, samples: usize) -> (usize, usize) {
    let near = at / 2;
    let far = if at % 2 == 1 {
        (near + 1).min(samples - 1)
    } else {
        near.saturating_sub(1)
    };
    (near, far)
}

/// The upsampled chroma at a luma pixel from the plane's four samples around
/// it: `(near row, near column)` weighs 9, the sample beside it and the one
/// above or below 3 each, the diagonal 1.
fn upsample(
    plane: &[u8],
    stride: usize,
    (row, far_row): (usize, usize),
    (column, far_column): (usize, usize),
) -> u8 {
    let at = |r: usize, c: usize| u32::from(plane[r * stride + c]);
    ((9 * at(row, column)
        + 3 * at(row, far_column)
        + 3 * at(far_row, column)
        + at(far_row, far_column)
        + 8)
        >> 4) as u8
}

/// Writes the picture's RGB into `rgba`, four bytes a pixel, leaving each
/// pixel's fourth byte as it is.
pub(super) fn to_rgba(picture: &Picture, rgba: &mut [u8]) {
    let chroma_width = picture.width.div_ceil(2);
    let chroma_height = picture.height.div_ceil(2);
    for (y, row) in rgba.chunks_exact_mut(picture.width * 4).enumerate() {
        let rows = neighbours(y, chroma_height);
        let luma = &picture.y[y * picture.y_stride..][..picture.width];
        for (x, (pixel, &luma)) in row.as_chunks_mut::<4>().0.iter_mut().zip(luma).enumerate() {
            let columns = neighbours(x, chroma_width);
            let u = upsample(&picture.u, picture.uv_stride, rows, columns);
            let v = upsample(&picture.v, picture.uv_stride, rows, columns);
            pixel[..3].copy_from_slice(&rgb(luma, u, v));
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The conversion worked from the constants: black (16, 128, 128) and
    /// white (235, 128, 128) are exact, and BT.601's red, its coordinates
    /// rounded to (81, 90, 240), comes out at 254: `(81 * 19077) >> 8 = 6036`,
    /// `(240 * 26149) >> 8 = 24514`, and `6036 + 24514 - 14234 = 16316`,
    /// which is 254 and 60 sixty-fourths.
    #[test]
    fn the_conversion_maps_bt601_limited_range() {
        assert_eq!(rgb(16, 128, 128), [0, 0, 0]);
        assert_eq!(rgb(235, 128, 128), [255, 255, 255]);
        assert_eq!(rgb(81, 90, 240), [254, 0, 0]);
        assert_eq!(rgb(145, 54, 34), [0, 255, 1]);
        assert_eq!(rgb(41, 240, 110), [0, 0, 255]);
        // A mid grey: (126 * 19077) >> 8 = 9389; 9389 + (128 * 26149 >> 8)
        // - 14234 = 8229, and 8229 >> 6 = 128.
        assert_eq!(rgb(126, 128, 128), [128, 128, 128]);
    }

    /// The upsampler's weights on a 2x2 chroma plane: the luma pixel at
    /// (1, 1) is nearest sample (0, 0), beside (1, 0), below (0, 1) and
    /// diagonal to (1, 1), so `(9 * 0 + 3 * 16 + 3 * 32 + 64 + 8) >> 4 = 13`;
    /// at the corner (0, 0) all four are the edge sample.
    #[test]
    fn upsampling_weighs_nine_three_three_one() {
        let plane = [0, 16, 32, 64];
        assert_eq!(neighbours(1, 2), (0, 1));
        assert_eq!(neighbours(2, 2), (1, 0));
        assert_eq!(neighbours(3, 2), (1, 1));
        assert_eq!(neighbours(0, 2), (0, 0));
        assert_eq!(upsample(&plane, 2, neighbours(1, 2), neighbours(1, 2)), 13);
        assert_eq!(upsample(&plane, 2, neighbours(0, 2), neighbours(0, 2)), 0);
        // (2, 0): nearest (1, 0) = 16, beside (0, 0) = 0, the row repeated:
        // (9 * 16 + 3 * 0 + 3 * 16 + 0 + 8) >> 4 = 12.
        assert_eq!(upsample(&plane, 2, neighbours(0, 2), neighbours(2, 2)), 12);
    }
}
