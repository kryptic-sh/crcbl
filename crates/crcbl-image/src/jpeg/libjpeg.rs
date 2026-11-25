use super::{ColourMarkers, Component, Frame, malformed};
use crate::{ImageError, Limits, Rgba8Image};

/// The IJG "islow" constants: each the named cosine product times 2^13,
/// rounded, as `jidctint.c` gives them.
const CONST_BITS: u32 = 13;
const PASS1_BITS: u32 = 2;
const FIX_0_298631336: i64 = 2446;
const FIX_0_390180644: i64 = 3196;
const FIX_0_541196100: i64 = 4433;
const FIX_0_765366865: i64 = 6270;
const FIX_0_899976223: i64 = 7373;
const FIX_1_175875602: i64 = 9633;
const FIX_1_501321110: i64 = 12299;
const FIX_1_847759065: i64 = 15137;
const FIX_1_961570560: i64 = 16069;
const FIX_2_053119869: i64 = 16819;
const FIX_2_562915447: i64 = 20995;
const FIX_3_072711026: i64 = 25172;

/// `x` divided by 2^n, rounding half up: the IJG `DESCALE`.
const fn descale(x: i64, n: u32) -> i64 {
    (x + (1 << (n - 1))) >> n
}

/// One 1-D pass of the islow transform over eight values: the even part from
/// 0, 2, 4, 6 and the odd part from 1, 3, 5, 7, the outputs in natural order
/// before descaling.
fn islow_pass(input: [i64; 8]) -> [i64; 8] {
    let z1 = (input[2] + input[6]) * FIX_0_541196100;
    let tmp2 = z1 - input[6] * FIX_1_847759065;
    let tmp3 = z1 + input[2] * FIX_0_765366865;
    let tmp0 = (input[0] + input[4]) << CONST_BITS;
    let tmp1 = (input[0] - input[4]) << CONST_BITS;
    let (tmp10, tmp13) = (tmp0 + tmp3, tmp0 - tmp3);
    let (tmp11, tmp12) = (tmp1 + tmp2, tmp1 - tmp2);

    let (t0, t1, t2, t3) = (input[7], input[5], input[3], input[1]);
    let (z1, z2, z3, z4) = (t0 + t3, t1 + t2, t0 + t2, t1 + t3);
    let z5 = (z3 + z4) * FIX_1_175875602;
    let (z1, z2) = (-z1 * FIX_0_899976223, -z2 * FIX_2_562915447);
    let z3 = -z3 * FIX_1_961570560 + z5;
    let z4 = -z4 * FIX_0_390180644 + z5;
    let t0 = t0 * FIX_0_298631336 + z1 + z3;
    let t1 = t1 * FIX_2_053119869 + z2 + z4;
    let t2 = t2 * FIX_3_072711026 + z2 + z3;
    let t3 = t3 * FIX_1_501321110 + z1 + z4;
    [
        tmp10 + t3,
        tmp11 + t2,
        tmp12 + t1,
        tmp13 + t0,
        tmp13 - t0,
        tmp12 - t1,
        tmp11 - t2,
        tmp10 - t3,
    ]
}

/// The islow inverse DCT of a dequantised block into eight rows of `out`,
/// `stride` apart, each sample level-shifted and clamped.
pub(super) fn idct(coefficients: &[i16; 64], quant: &[u16; 64], out: &mut [u8], stride: usize) {
    let mut workspace = [0i64; 64];
    for column in 0..8 {
        let input: [i64; 8] = core::array::from_fn(|row| {
            i64::from(coefficients[row * 8 + column]) * i64::from(quant[row * 8 + column])
        });
        let output = islow_pass(input);
        for row in 0..8 {
            workspace[row * 8 + column] = descale(output[row], CONST_BITS - PASS1_BITS);
        }
    }
    for row in 0..8 {
        let input: [i64; 8] = core::array::from_fn(|column| workspace[row * 8 + column]);
        let output = islow_pass(input);
        for column in 0..8 {
            let sample = descale(output[column], CONST_BITS + PASS1_BITS + 3) + 128;
            out[row * stride + column] = sample.clamp(0, 255) as u8;
        }
    }
}

/// A component's samples at its own resolution, in whole blocks.
#[derive(Debug)]
struct Plane {
    samples: Vec<u8>,
    stride: usize,
}

/// Every block of `component` through the inverse DCT.
fn plane(component: &Component) -> Result<Plane, ImageError> {
    let quant = component
        .quant
        .ok_or_else(|| malformed("a component no scan ever covered"))?;
    let stride = component.blocks_wide * 8;
    let mut samples = vec![0u8; stride * component.blocks_high * 8];
    for by in 0..component.blocks_high {
        for bx in 0..component.blocks_wide {
            let offset = (by * component.blocks_wide + bx) * 64;
            let block: &[i16; 64] = component.coefficients[offset..offset + 64]
                .try_into()
                .map_err(|_| malformed("a block outside its component"))?;
            let start = by * 8 * stride + bx * 8;
            idct(block, &quant, &mut samples[start..], stride);
        }
    }
    Ok(Plane { samples, stride })
}

/// `plane`, upsampled to the frame's full resolution as libjpeg-turbo does
/// (see the [module docs](super)), `width` × `height` samples.
fn upsample(plane: &Plane, component: &Component, frame: &Frame) -> Vec<u8> {
    let (h_factor, v_factor) = (frame.h_max / component.h, frame.v_max / component.v);
    let (width, height) = (frame.width, frame.height);
    let at = |x: usize, y: usize| i32::from(plane.samples[y * plane.stride + x]);
    // libjpeg's context rows: the first row above the top, the last below the
    // bottom.
    let row_above = |y: usize| y.saturating_sub(1);
    let row_below = |y: usize| (y + 1).min(component.height - 1);
    let last = component.width - 1;
    let fancy = component.width > 2;
    let mut out = vec![0u8; width * height];
    match (h_factor, v_factor) {
        (1, 1) => {
            for y in 0..height {
                for x in 0..width {
                    out[y * width + x] = at(x, y) as u8;
                }
            }
        }
        (2, 1) if fancy => {
            for y in 0..height {
                for x in 0..width {
                    let source = x / 2;
                    let near = at(source, y) * 3;
                    let value = if x % 2 == 0 {
                        if source == 0 {
                            at(0, y)
                        } else {
                            (near + at(source - 1, y) + 1) >> 2
                        }
                    } else if source == last {
                        at(last, y)
                    } else {
                        (near + at(source + 1, y) + 2) >> 2
                    };
                    out[y * width + x] = value as u8;
                }
            }
        }
        (2, 2) if fancy => {
            for y in 0..height {
                let source_y = y / 2;
                let other = if y % 2 == 0 {
                    row_above(source_y)
                } else {
                    row_below(source_y)
                };
                let column = |x: usize| at(x, source_y) * 3 + at(x, other);
                for x in 0..width {
                    let source = x / 2;
                    let this = column(source);
                    let value = if x % 2 == 0 {
                        if source == 0 {
                            (this * 4 + 8) >> 4
                        } else {
                            (this * 3 + column(source - 1) + 8) >> 4
                        }
                    } else if source == last {
                        (this * 4 + 7) >> 4
                    } else {
                        (this * 3 + column(source + 1) + 7) >> 4
                    };
                    out[y * width + x] = value as u8;
                }
            }
        }
        _ => {
            for y in 0..height {
                for x in 0..width {
                    out[y * width + x] = at(x / h_factor, y / v_factor) as u8;
                }
            }
        }
    }
    out
}

/// The IJG fixed-point YCbCr-to-RGB conversion: 16 fractional bits, each
/// term rounded as `jdcolor.c` builds its tables.
pub(super) fn ycc_to_rgb(y: u8, cb: u8, cr: u8) -> [u8; 3] {
    const SCALE_BITS: u32 = 16;
    const ONE_HALF: i64 = 1 << (SCALE_BITS - 1);
    // FIX(x): x times 2^16, rounded.
    const CR_R: i64 = 91881; // 1.40200
    const CB_B: i64 = 116130; // 1.77200
    const CR_G: i64 = 46802; // 0.71414
    const CB_G: i64 = 22554; // 0.34414
    let (y, cb, cr) = (i64::from(y), i64::from(cb) - 128, i64::from(cr) - 128);
    let r = y + ((CR_R * cr + ONE_HALF) >> SCALE_BITS);
    let g = y + ((-CB_G * cb + ONE_HALF - CR_G * cr) >> SCALE_BITS);
    let b = y + ((CB_B * cb + ONE_HALF) >> SCALE_BITS);
    [r, g, b].map(|channel| channel.clamp(0, 255) as u8)
}

/// Four samples as libjpeg gives them, CMYK, turned into RGB as Pillow
/// turns them (see the [module docs](super)): inverted, then `cmyk2rgb`.
pub(super) fn cmyk_to_rgb(samples: [u8; 4]) -> [u8; 3] {
    let [c, m, y, k] = samples.map(|sample| i32::from(u8::MAX - sample));
    let nk = 255 - k;
    // Pillow's `MULDIV255`: `a · b / 255`, rounded, in shifts, exact for
    // both factors at most 255. So it is at most `nk`, and the difference
    // is in 0..=255: Pillow's `CLIP8` around it never clips.
    let muldiv255 = |a: i32| {
        let product = a * nk + 128;
        ((product >> 8) + product) >> 8
    };
    [c, m, y].map(|channel| (nk - muldiv255(channel)) as u8)
}

/// The decoded frame as RGBA8.
pub(super) fn finish(
    frame: &Frame,
    colour: ColourMarkers,
    limits: &Limits,
) -> Result<Rgba8Image, ImageError> {
    let bytes = limits.rgba_bytes(frame.width as u32, frame.height as u32)?;
    let planes = frame
        .components
        .iter()
        .map(|component| Ok(upsample(&plane(component)?, component, frame)))
        .collect::<Result<Vec<_>, ImageError>>()?;
    // libjpeg's rule for three components: YCbCr under JFIF, else as an Adobe
    // marker says, else RGB if the ids spell it, else YCbCr.
    let ids: Vec<u8> = frame.components.iter().map(|c| c.id).collect();
    let rgb = planes.len() == 3
        && !colour.jfif
        && match colour.adobe_transform {
            Some(transform) => transform == 0,
            None => ids == b"RGB",
        };
    // libjpeg's rule for four: CMYK unless an Adobe marker's transform says
    // otherwise, when it is YCCK.
    let ycck = colour
        .adobe_transform
        .is_some_and(|transform| transform != 0);
    let mut pixels = Vec::with_capacity(bytes);
    for i in 0..frame.width * frame.height {
        let [r, g, b] = match planes.as_slice() {
            [grey] => [grey[i]; 3],
            [a, b, c] if rgb => [a[i], b[i], c[i]],
            [y, cb, cr] => ycc_to_rgb(y[i], cb[i], cr[i]),
            [c, m, y, k] if !ycck => cmyk_to_rgb([c[i], m[i], y[i], k[i]]),
            [y, cb, cr, k] => {
                let [c, m, y] = ycc_to_rgb(y[i], cb[i], cr[i]).map(|channel| u8::MAX - channel);
                cmyk_to_rgb([c, m, y, k[i]])
            }
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
