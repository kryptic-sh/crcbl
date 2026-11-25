//! stb_image's stages from dequantised coefficients to pixels: its inverse
//! DCT as x86-64 builds run it, its chroma upsampling and its colour
//! conversion, and `load_jpeg_image`'s choices among them.
//!
//! Each function names the stb_image routine it transcribes. The arithmetic
//! is kept in stb_image's own order and widths, since the rounding and the
//! wrapping are what decide the bytes.

use super::Colour;
use crate::jpeg::{Component, Frame, malformed};
use crate::{ImageError, Limits, Rgba8Image};

/// The decoded frame as RGBA8, as stb_image's `load_jpeg_image` converts
/// it for three requested components, with alpha 255. Every component's
/// coefficients are dequantised already.
pub(super) fn finish(
    frame: &Frame,
    colour: Colour,
    limits: &Limits,
) -> Result<Rgba8Image, ImageError> {
    let bytes = limits.rgba_bytes(frame.width as u32, frame.height as u32)?;
    let planes = frame
        .components
        .iter()
        .map(|component| Ok(upsample(&plane(component)?, component, frame)))
        .collect::<Result<Vec<_>, ImageError>>()?;
    let ids: Vec<u8> = frame.components.iter().map(|c| c.id).collect();
    let transform = colour.app14_color_transform;
    let rgb = ids == b"RGB" || (transform == 0 && !colour.jfif);
    let mut pixels = Vec::with_capacity(bytes);
    for i in 0..frame.width * frame.height {
        let [r, g, b] = match planes.as_slice() {
            [grey] => [grey[i]; 3],
            [r, g, b] if rgb => [r[i], g[i], b[i]],
            [y, cb, cr] => ycbcr_to_rgb(y[i], cb[i], cr[i]),
            [c, m, y, k] if transform == 0 => [c[i], m[i], y[i]].map(|s| blinn_8x8(s, k[i])),
            [y, cb, cr, k] if transform == 2 => {
                ycbcr_to_rgb(y[i], cb[i], cr[i]).map(|s| blinn_8x8(u8::MAX - s, k[i]))
            }
            [y, cb, cr, _] => ycbcr_to_rgb(y[i], cb[i], cr[i]),
            _ => return Err(malformed("a frame of an unexpected component count")),
        };
        pixels.extend_from_slice(&[r, g, b, u8::MAX]);
    }
    Ok(Rgba8Image::from_sized(
        frame.width as u32,
        frame.height as u32,
        pixels,
    ))
}

/// A component's samples at its own resolution: every block it has samples
/// in through the inverse DCT, `blocks_wide * 8` to a row. The padding
/// blocks past them, which stb_image never reads, are left zero.
fn plane(component: &Component) -> Result<Vec<u8>, ImageError> {
    let stride = component.blocks_wide * 8;
    let mut samples = vec![0u8; stride * component.blocks_high * 8];
    for by in 0..component.height.div_ceil(8) {
        for bx in 0..component.width.div_ceil(8) {
            let offset = (by * component.blocks_wide + bx) * 64;
            let block: &[i16; 64] = component.coefficients[offset..offset + 64]
                .try_into()
                .map_err(|_| malformed("a block outside its component"))?;
            let start = by * 8 * stride + bx * 8;
            idct_block(block, &mut samples[start..], stride);
        }
    }
    Ok(samples)
}

/// `stbi__f2f`: a constant times 2^12, rounded, from the `float` literal
/// stb_image writes it as: `x`, read as a C `float`, times 4096, plus a
/// half in `double`, truncated.
const fn f2f(x: f64) -> i32 {
    (x as f32 as f64 * 4096.0 + 0.5) as i32
}

/// A pair of 16-bit constants `stbi__idct_simd` multiplies an interleaved
/// pair of rows by with `_mm_madd_epi16`: each its `dct_const(x, y)`, sums
/// of `stbi__f2f` constants as the source writes them.
type Rotation = (i32, i32);

const ROT0_0: Rotation = (f2f(0.541_196_1), f2f(0.541_196_1) + f2f(-1.847_759_065));
const ROT0_1: Rotation = (f2f(0.541_196_1) + f2f(0.765_366_865), f2f(0.541_196_1));
const ROT1_0: Rotation = (f2f(1.175_875_602) + f2f(-0.899_976_223), f2f(1.175_875_602));
const ROT1_1: Rotation = (f2f(1.175_875_602), f2f(1.175_875_602) + f2f(-2.562_915_447));
const ROT2_0: Rotation = (
    f2f(-1.961_570_560) + f2f(0.298_631_336),
    f2f(-1.961_570_560),
);
const ROT2_1: Rotation = (
    f2f(-1.961_570_560),
    f2f(-1.961_570_560) + f2f(3.072_711_026),
);
const ROT3_0: Rotation = (
    f2f(-0.390_180_644) + f2f(2.053_119_869),
    f2f(-0.390_180_644),
);
const ROT3_1: Rotation = (
    f2f(-0.390_180_644),
    f2f(-0.390_180_644) + f2f(1.501_321_110),
);

/// The column pass's rounding bias and shift, `bias_0` and its `10`.
const COLUMN_PASS: (i32, u32) = (512, 10);
/// The row pass's: rounding for the 17 bits the passes scaled by, with the
/// level shift folded in (`bias_1`).
const ROW_PASS: (i32, u32) = (65536 + (128 << 17), 17);

/// `_mm_madd_epi16` in one lane: `x · a + y · b`, the products in 32 bits
/// and their sum wrapping.
fn madd(x: i16, y: i16, (a, b): Rotation) -> i32 {
    (i32::from(x) * a).wrapping_add(i32::from(y) * b)
}

/// `_mm_packs_epi32` in one lane: saturated to 16 bits.
fn packs(x: i32) -> i16 {
    x.clamp(i32::from(i16::MIN), i32::from(i16::MAX)) as i16
}

/// One lane of `stbi__idct_simd`'s `dct_pass`: eight 16-bit inputs through
/// the 1-D transform to eight 16-bit outputs. The sums of input pairs wrap
/// at 16 bits (`_mm_add_epi16`), the products and their sums are 32-bit,
/// and each output saturates to 16 bits: where stb_image's plain C
/// (`stbi__idct_block`) keeps 32 bits throughout, and so differs on large
/// coefficients.
fn dct_pass(s: [i16; 8], (bias, shift): (i32, u32)) -> [i16; 8] {
    // Even part.
    let t2e = madd(s[2], s[6], ROT0_0);
    let t3e = madd(s[2], s[6], ROT0_1);
    let t0e = i32::from(s[0].wrapping_add(s[4])) << 12;
    let t1e = i32::from(s[0].wrapping_sub(s[4])) << 12;
    let x0 = t0e.wrapping_add(t3e);
    let x3 = t0e.wrapping_sub(t3e);
    let x1 = t1e.wrapping_add(t2e);
    let x2 = t1e.wrapping_sub(t2e);
    // Odd part.
    let y0o = madd(s[7], s[3], ROT2_0);
    let y2o = madd(s[7], s[3], ROT2_1);
    let y1o = madd(s[5], s[1], ROT3_0);
    let y3o = madd(s[5], s[1], ROT3_1);
    let sum17 = s[1].wrapping_add(s[7]);
    let sum35 = s[3].wrapping_add(s[5]);
    let y4o = madd(sum17, sum35, ROT1_0);
    let y5o = madd(sum17, sum35, ROT1_1);
    let x4 = y0o.wrapping_add(y4o);
    let x5 = y1o.wrapping_add(y5o);
    let x6 = y2o.wrapping_add(y5o);
    let x7 = y3o.wrapping_add(y4o);
    // `dct_bfly32o`: bias, butterfly, shift, pack.
    let bfly = |a: i32, b: i32| {
        let a = a.wrapping_add(bias);
        (
            packs(a.wrapping_add(b) >> shift),
            packs(a.wrapping_sub(b) >> shift),
        )
    };
    let (o0, o7) = bfly(x0, x7);
    let (o1, o6) = bfly(x1, x6);
    let (o2, o5) = bfly(x2, x5);
    let (o3, o4) = bfly(x3, x4);
    [o0, o1, o2, o3, o4, o5, o6, o7]
}

/// `stbi__idct_simd`, the inverse DCT llama.cpp's x86-64 builds run: a
/// dequantised block into eight rows of `out`, `stride` apart. The column
/// pass's 16-bit results are the row pass's inputs (its transposes move
/// lanes, not values), and the row pass's results are saturated to bytes
/// (`_mm_packus_epi16`).
fn idct_block(data: &[i16; 64], out: &mut [u8], stride: usize) {
    let mut columns = [[0i16; 8]; 8];
    for (c, column) in columns.iter_mut().enumerate() {
        *column = dct_pass(core::array::from_fn(|r| data[r * 8 + c]), COLUMN_PASS);
    }
    for r in 0..8 {
        let row = dct_pass(core::array::from_fn(|c| columns[c][r]), ROW_PASS);
        for (c, &value) in row.iter().enumerate() {
            out[r * stride + c] = value.clamp(0, 255) as u8;
        }
    }
}

/// Which of stb_image's row resamplers `load_jpeg_image` picks for a
/// component, by its expansion each way.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Resampler {
    /// `resample_row_1`: the row as it is.
    Copy,
    /// `stbi__resample_row_v_2`.
    Vertical,
    /// `stbi__resample_row_h_2`.
    Horizontal,
    /// `stbi__resample_row_hv_2`, and its SSE2 version, which agrees.
    Both,
    /// `stbi__resample_row_generic`: the nearest sample.
    Nearest,
}

impl Resampler {
    fn for_expansion(hs: usize, vs: usize) -> Self {
        match (hs, vs) {
            (1, 1) => Self::Copy,
            (1, 2) => Self::Vertical,
            (2, 1) => Self::Horizontal,
            (2, 2) => Self::Both,
            _ => Self::Nearest,
        }
    }

    /// One output row into `out` from the nearer input row and the farther,
    /// `w` input samples wide, `hs` the horizontal expansion.
    fn row(self, out: &mut [u8], near_row: &[u8], far_row: &[u8], w: usize, hs: usize) {
        let div4 = |x: u32| (x >> 2) as u8;
        let div16 = |x: u32| (x >> 4) as u8;
        let near = |i: usize| u32::from(near_row[i]);
        let far = |i: usize| u32::from(far_row[i]);
        match self {
            Self::Copy => out[..w].copy_from_slice(&near_row[..w]),
            Self::Vertical => {
                for ((out, &near), &far) in out.iter_mut().zip(&near_row[..w]).zip(far_row) {
                    *out = div4(3 * u32::from(near) + u32::from(far) + 2);
                }
            }
            Self::Horizontal => {
                if w == 1 {
                    out[0] = near(0) as u8;
                    out[1] = near(0) as u8;
                    return;
                }
                out[0] = near(0) as u8;
                out[1] = div4(near(0) * 3 + near(1) + 2);
                for i in 1..w - 1 {
                    let n = 3 * near(i) + 2;
                    out[i * 2] = div4(n + near(i - 1));
                    out[i * 2 + 1] = div4(n + near(i + 1));
                }
                // stb_image weights the second-last sample here, not the
                // last.
                let i = w - 1;
                out[i * 2] = div4(near(w - 2) * 3 + near(w - 1) + 2);
                out[i * 2 + 1] = near(w - 1) as u8;
            }
            Self::Both => {
                if w == 1 {
                    let value = div4(3 * near(0) + far(0) + 2);
                    out[0] = value;
                    out[1] = value;
                    return;
                }
                let mut t1 = 3 * near(0) + far(0);
                out[0] = div4(t1 + 2);
                for i in 1..w {
                    let t0 = t1;
                    t1 = 3 * near(i) + far(i);
                    out[i * 2 - 1] = div16(3 * t0 + t1 + 8);
                    out[i * 2] = div16(3 * t1 + t0 + 8);
                }
                out[w * 2 - 1] = div4(t1 + 2);
            }
            Self::Nearest => {
                for i in 0..w {
                    for j in 0..hs {
                        out[i * hs + j] = near(i) as u8;
                    }
                }
            }
        }
    }
}

/// `plane`, upsampled to the frame's full resolution as `load_jpeg_image`
/// walks it, `width` × `height` samples.
///
/// Output row `y` reads input row `y / vs` as the nearer, and for a
/// vertical 2:1 the row above it (on an even row) or below it (on an odd
/// one) as the farther, the first and last rows standing in past the edges:
/// what its `ystep`, `line0` and `line1` bookkeeping comes to.
fn upsample(plane: &[u8], component: &Component, frame: &Frame) -> Vec<u8> {
    let (hs, vs) = (frame.h_max / component.h, frame.v_max / component.v);
    let resampler = Resampler::for_expansion(hs, vs);
    let (width, height) = (frame.width, frame.height);
    let stride = component.blocks_wide * 8;
    // `w_lores`: the component's own samples across, which stb_image
    // computes as the frame's width over the expansion, rounded up.
    let w = width.div_ceil(hs);
    let rows = component.height;
    let mut line = vec![0u8; w * hs];
    let mut out = vec![0u8; width * height];
    for y in 0..height {
        let near = y / vs;
        let far = match (vs, y % 2) {
            (2, 0) => near.saturating_sub(1),
            (2, _) => (near + 1).min(rows - 1),
            _ => near,
        };
        let near = &plane[near * stride..][..w];
        let far = &plane[far * stride..][..w];
        resampler.row(&mut line, near, far, w, hs);
        out[y * width..][..width].copy_from_slice(&line[..width]);
    }
    out
}

/// `stbi__YCbCr_to_RGB_row`: stb_image's reduced-precision conversion,
/// 12-bit constants shifted to 20 fractional bits, with the blue-difference
/// part of green truncated to 16 bits first. Its SSE2 version converts only
/// four-byte pixels, so for three components this is the one that runs.
fn ycbcr_to_rgb(y: u8, cb: u8, cr: u8) -> [u8; 3] {
    let y_fixed = (i32::from(y) << 20) + (1 << 19);
    let (cr, cb) = (i32::from(cr) - 128, i32::from(cb) - 128);
    let r = y_fixed + cr * float2fixed(1.402);
    let g = y_fixed
        + cr * -float2fixed(0.714_14)
        + ((cb * -float2fixed(0.344_14)) as u32 & 0xffff_0000) as i32;
    let b = y_fixed + cb * float2fixed(1.772);
    [r, g, b].map(|channel| (channel >> 20).clamp(0, 255) as u8)
}

/// `stbi__float2fixed`: a constant, read as a C `float` literal, times
/// 2^12 in `float`, rounded, then shifted up 8 bits.
const fn float2fixed(x: f64) -> i32 {
    ((x as f32 * 4096.0 + 0.5) as i32) << 8
}

/// `stbi__blinn_8x8`: `x · y / 255`, rounded, in shifts.
fn blinn_8x8(x: u8, y: u8) -> u8 {
    let t = u32::from(x) * u32::from(y) + 128;
    ((t + (t >> 8)) >> 8) as u8
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The constants stb_image's `float` literals round to, as a C program
    /// printed them from its `stbi__f2f` and `stbi__float2fixed` macros
    /// built by gcc.
    #[test]
    fn the_constants_are_stb_images() {
        let idct = [
            (0.541_196_1, 2217),
            (-1.847_759_065, -7567),
            (0.765_366_865, 3135),
            (1.175_875_602, 4816),
            (0.298_631_336, 1223),
            (2.053_119_869, 8410),
            (3.072_711_026, 12586),
            (1.501_321_110, 6149),
            (-0.899_976_223, -3685),
            (-2.562_915_447, -10497),
            (-1.961_570_560, -8034),
            (-0.390_180_644, -1597),
        ];
        for (x, expected) in idct {
            assert_eq!(f2f(x), expected, "{x}");
        }
        for (x, expected) in [
            (1.402, 5743),
            (0.714_14, 2925),
            (0.344_14, 1410),
            (1.772, 7258),
        ] {
            assert_eq!(float2fixed(x), expected << 8, "{x}");
        }
    }

    #[test]
    fn a_lone_dc_is_flat() {
        let mut data = [0i16; 64];
        data[0] = 8;
        let mut out = [0u8; 64];
        idct_block(&data, &mut out, 8);
        assert_eq!(out, [129; 64]);
    }

    #[test]
    fn blinn_is_the_rounded_product() {
        for x in 0..=255u8 {
            for y in 0..=255u8 {
                let product = u32::from(x) * u32::from(y);
                assert_eq!(
                    u32::from(blinn_8x8(x, y)),
                    (product * 2 + 255) / 510,
                    "{x} {y}"
                );
            }
        }
    }
}
