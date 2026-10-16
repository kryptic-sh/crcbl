//! VP8 key frames, RFC 6386, decoded to Y'CbCr 4:2:0 planes: what a lossy
//! WebP's `VP8 ` chunk holds.
//!
//! # What it reads
//!
//! A WebP image is one key frame, so this is the RFC's intra half: the frame
//! header, segmentation with its map and per-segment quantiser and filter
//! level (absolute or relative), the loop filter's type, level, sharpness
//! and per-mode deltas, one to eight token partitions, coefficient
//! probability updates and skipped macroblocks, the luma modes (four whole
//! block modes and ten subblock modes) and chroma modes, dequantisation,
//! the inverse WHT and DCT, and the normal and simple loop filters. An
//! interframe is [`ImageError::Malformed`]: WebP has none, and neither does
//! this decoder's support for motion. The header's colour space and
//! clamping bits are ignored, as libwebp ignores them (the one colour space
//! is BT.601, and clamping is always safe); its scaling bits are a display
//! hint, and the planes come out at the coded size, as libwebp's do.
//!
//! # Exactly the RFC's pixels
//!
//! VP8 decoding is bit-exact by definition: the transforms, prediction and
//! loop filter are the RFC's C (its reference decoder, `dixie`, where the
//! prose leaves out a detail), so these planes are libwebp's. One place
//! where the reference decoder and libwebp part: a segment's filter level
//! is clamped to 0..=63 before the mode deltas are added here, as `dixie`
//! and libvpx do, where libwebp clamps only the sum. They differ only for a
//! relative segment level that leaves the range and a delta that brings it
//! back, which libwebp's encoder never writes (its segment levels are
//! absolute and its deltas zero).
//!
//! # Hostile input
//!
//! The frame's size passes the caller's [`Limits`] before the planes are
//! sized from it; the planes are padded to whole macroblocks, at most
//! fifteen pixels more each way. A partition that runs out — a bool read
//! with the decoder's window past its last byte — is
//! [`ImageError::Malformed`], where libwebp also stops.

mod boolean;
mod filter;
mod predict;
mod tables;
mod transform;

use super::{malformed, unsupported};
use crate::{ImageError, Limits};
use boolean::BoolDecoder;
use filter::Strength;
use predict::{ABOVE_EDGE, LEFT_EDGE};
use tables::{
    AC_QUANT, BMODE_TREE, CATEGORY_BASE, CATEGORY_PROBS, COEFF_BANDS, COEFF_TREE,
    COEFF_UPDATE_PROBS, DC_QUANT, DEFAULT_COEFF_PROBS, KF_BMODE_PROB, KF_UV_MODE_PROB,
    KF_YMODE_PROB, KF_YMODE_TREE, SEGMENT_TREE, UV_MODE_TREE, ZIGZAG,
};

/// The luma and chroma prediction modes, RFC 6386 section 11.2, in the
/// RFC's order (chroma uses the first four).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[repr(u8)]
pub(super) enum Mode {
    Dc,
    V,
    H,
    Tm,
    /// Each luma subblock predicted on its own.
    B,
}

/// The tree leaves' modes, by leaf.
const MODES: [Mode; 5] = [Mode::Dc, Mode::V, Mode::H, Mode::Tm, Mode::B];

/// The subblock prediction modes, RFC 6386 section 11.2, in the RFC's order.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
#[repr(u8)]
pub(super) enum BMode {
    #[default]
    Dc,
    Tm,
    Ve,
    He,
    Ld,
    Rd,
    Vr,
    Vl,
    Hd,
    Hu,
}

/// The tree leaves' subblock modes, by leaf.
const BMODES: [BMode; 10] = [
    BMode::Dc,
    BMode::Tm,
    BMode::Ve,
    BMode::He,
    BMode::Ld,
    BMode::Rd,
    BMode::Vr,
    BMode::Vl,
    BMode::Hd,
    BMode::Hu,
];

/// The coefficient tokens, RFC 6386 section 13.2, in the RFC's order: a
/// value, a range of values (`dct_cat1` to `dct_cat6`) and end-of-block.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[repr(u8)]
pub(super) enum Token {
    Zero,
    One,
    Two,
    Three,
    Four,
    Cat1,
    Cat2,
    Cat3,
    Cat4,
    Cat5,
    Cat6,
    Eob,
}

/// The bytes before the first partition: the frame tag, the start code and
/// the dimensions, RFC 6386 section 9.1.
const KEY_FRAME_HEADER_BYTES: usize = 10;

/// A key frame's start code, RFC 6386 section 9.1.
const START_CODE: [u8; 3] = [0x9d, 0x01, 0x2a];

/// The highest version the RFC defines; the rest are reserved.
const MAX_VERSION: u32 = 3;

/// The coefficient probability planes, RFC 6386 section 13.3.
const PLANE_Y_AFTER_Y2: usize = 0;
const PLANE_Y2: usize = 1;
const PLANE_CHROMA: usize = 2;
const PLANE_Y_WITH_DC: usize = 3;

/// The second-order block's index among a macroblock's 25 blocks, after the
/// 16 luma and 8 chroma ones.
const Y2_BLOCK: usize = 24;

/// Where a macroblock's U and V blocks start among its 25.
const U_BLOCKS: usize = 16;
const V_BLOCKS: usize = 20;

/// The width of the luma workspace: the column to the left, the sixteen
/// pixels of the macroblock and the four above and to its right that the
/// rightmost subblocks predict from.
const LUMA_STRIDE: usize = 1 + 16 + 4;

/// The width of a chroma workspace: the column to the left and eight pixels.
const CHROMA_STRIDE: usize = 1 + 8;

/// The Y2 and chroma dequantisation adjustments of the reference decoder's
/// `dequant_init` (RFC 6386 section 20.4, which section 14.1 points to): the
/// Y2 DC factor doubled, the Y2 AC factor scaled by 155/100 and at least 8,
/// and the chroma DC factor at most 132.
const Y2_DC_SCALE: i32 = 2;
const Y2_AC_SCALE_PERCENT: i32 = 155;
const MIN_Y2_AC: i32 = 8;
const MAX_CHROMA_DC: i32 = 132;

/// The segments a frame may have, RFC 6386 section 9.3.
const SEGMENTS: usize = 4;

/// A decoded key frame: its planes padded to whole macroblocks, luma rows
/// `y_stride` apart and chroma rows `uv_stride`.
pub(super) struct Picture {
    pub width: usize,
    pub height: usize,
    pub y: Vec<u8>,
    pub u: Vec<u8>,
    pub v: Vec<u8>,
    pub y_stride: usize,
    pub uv_stride: usize,
}

/// The segmentation header, RFC 6386 section 9.3, as a key frame leaves it.
#[derive(Debug, Default)]
struct Segmentation {
    update_map: bool,
    /// Whether the values below replace the frame's (rather than adjust it).
    absolute: bool,
    quantizer: [i32; SEGMENTS],
    filter_level: [i32; SEGMENTS],
    tree_probs: [u8; 3],
}

/// The loop filter header, RFC 6386 section 9.4.
#[derive(Debug)]
struct FilterHeader {
    kind: filter::Kind,
    level: i32,
    sharpness: i32,
    /// The reference-frame and mode deltas, when enabled; a key frame only
    /// ever uses the intra frame's and B_PRED's (the first of each).
    deltas: Option<([i32; 4], [i32; 4])>,
}

/// The quantiser indices, RFC 6386 section 9.6: the luma AC index and the
/// other five as deltas from it.
#[derive(Debug, Default)]
struct Quantizer {
    y_ac: i32,
    y_dc_delta: i32,
    y2_dc_delta: i32,
    y2_ac_delta: i32,
    uv_dc_delta: i32,
    uv_ac_delta: i32,
}

/// One segment's dequantisation factors, `[DC, AC]` for each block type.
#[derive(Clone, Copy, Debug, Default)]
struct Dequant {
    y1: [i32; 2],
    y2: [i32; 2],
    uv: [i32; 2],
}

/// Everything the first partition says before its macroblocks.
struct FrameHeader {
    segmentation: Option<Segmentation>,
    filter: FilterHeader,
    partitions: usize,
    quantizer: Quantizer,
    coeff_probs: [[[[u8; 11]; 3]; 8]; 4],
    skip_prob: Option<u8>,
}

/// What the macroblocks above and to the left left for the next one: which
/// of their edge blocks had coefficients (four luma, two U, two V, then Y2)
/// and the subblock modes along the edge.
#[derive(Clone, Copy, Debug, Default)]
struct Context {
    nonzero: [bool; 9],
    modes: [BMode; 4],
}

/// One macroblock's prediction record, RFC 6386 section 11.
struct Macroblock {
    segment: usize,
    skip: bool,
    mode: Mode,
    /// The subblock modes, raster order: B_PRED's own, or the one a whole
    /// block mode stands for in its neighbours' contexts.
    sub: [BMode; 16],
    uv_mode: Mode,
}

/// Decodes a `VP8 ` chunk's key frame.
///
/// # Errors
///
/// [`ImageError::Empty`] or [`ImageError::TooLarge`] if its size breaks
/// `limits`, [`ImageError::Unsupported`] for a reserved version, and
/// [`ImageError::Malformed`] for an interframe or a frame that is not
/// well-formed.
pub(super) fn decode(data: &[u8], limits: &Limits) -> Result<Picture, ImageError> {
    let header = data
        .get(..KEY_FRAME_HEADER_BYTES)
        .ok_or_else(|| malformed("a VP8 frame header is cut short"))?;
    let tag = u32::from_le_bytes([header[0], header[1], header[2], 0]);
    if tag & 1 != 0 {
        return Err(malformed("a VP8 interframe, where WebP has one key frame"));
    }
    let version = (tag >> 1) & 7;
    if version > MAX_VERSION {
        return Err(unsupported(format!("VP8 version {version}")));
    }
    if (tag >> 4) & 1 == 0 {
        return Err(malformed("a VP8 frame not for display"));
    }
    let first_partition = (tag >> 5) as usize;
    if header[3..6] != START_CODE {
        return Err(malformed("no VP8 start code"));
    }
    // The top two bits of each are a scale for display, not the size.
    let width = u32::from(u16::from_le_bytes([header[6], header[7]]) & 0x3fff);
    let height = u32::from(u16::from_le_bytes([header[8], header[9]]) & 0x3fff);
    limits.rgba_bytes(width, height)?;

    let rest = &data[KEY_FRAME_HEADER_BYTES..];
    let (first, rest) = rest
        .split_at_checked(first_partition)
        .ok_or_else(|| malformed("the first VP8 partition runs past its chunk"))?;
    let mut modes = BoolDecoder::new(first);
    let header = read_frame_header(&mut modes)?;
    let mut partitions = token_partitions(rest, header.partitions)?;
    decode_macroblocks(
        &header,
        &mut modes,
        &mut partitions,
        width as usize,
        height as usize,
    )
}

/// Reads the frame header from the first partition, RFC 6386 section 9 in
/// the order of the reference decoder's `decode_frame`.
fn read_frame_header(bits: &mut BoolDecoder<'_>) -> Result<FrameHeader, ImageError> {
    // Colour space and clamping type, both ignored (see the module docs).
    bits.literal(2);

    let segmentation = bits.flag().then(|| {
        let mut segmentation = Segmentation {
            update_map: bits.flag(),
            ..Segmentation::default()
        };
        if bits.flag() {
            // RFC 6386 section 9.3 has the sense of this bit the wrong way
            // round; section 19.2, the reference decoder and libwebp agree
            // that one means absolute values.
            segmentation.absolute = bits.flag();
            for quantizer in &mut segmentation.quantizer {
                *quantizer = bits.maybe_signed(7);
            }
            for level in &mut segmentation.filter_level {
                *level = bits.maybe_signed(6);
            }
        }
        if segmentation.update_map {
            for probability in &mut segmentation.tree_probs {
                *probability = if bits.flag() {
                    bits.literal(8) as u8
                } else {
                    255
                };
            }
        }
        segmentation
    });

    let kind = if bits.flag() {
        filter::Kind::Simple
    } else {
        filter::Kind::Normal
    };
    let level = bits.literal(6) as i32;
    let sharpness = bits.literal(3) as i32;
    let deltas = bits.flag().then(|| {
        let (mut reference, mut mode) = ([0; 4], [0; 4]);
        if bits.flag() {
            for delta in reference.iter_mut().chain(&mut mode) {
                *delta = bits.maybe_signed(6);
            }
        }
        (reference, mode)
    });
    let filter = FilterHeader {
        kind,
        level,
        sharpness,
        deltas,
    };

    let partitions = 1 << bits.literal(2);
    let quantizer = Quantizer {
        y_ac: bits.literal(7) as i32,
        y_dc_delta: bits.maybe_signed(4),
        y2_dc_delta: bits.maybe_signed(4),
        y2_ac_delta: bits.maybe_signed(4),
        uv_dc_delta: bits.maybe_signed(4),
        uv_ac_delta: bits.maybe_signed(4),
    };
    // Whether to keep the probabilities for the next frame: there is none.
    bits.flag();

    let mut coeff_probs = DEFAULT_COEFF_PROBS;
    for (plane, updates) in coeff_probs.iter_mut().zip(&COEFF_UPDATE_PROBS) {
        for (band, updates) in plane.iter_mut().zip(updates) {
            for (context, updates) in band.iter_mut().zip(updates) {
                for (probability, &update) in context.iter_mut().zip(updates) {
                    if bits.read(update) {
                        *probability = bits.literal(8) as u8;
                    }
                }
            }
        }
    }
    let skip_prob = bits.flag().then(|| bits.literal(8) as u8);
    if bits.past_end() {
        return Err(malformed("the VP8 frame header runs past its partition"));
    }
    Ok(FrameHeader {
        segmentation,
        filter,
        partitions,
        quantizer,
        coeff_probs,
        skip_prob,
    })
}

/// The token partitions after the first, RFC 6386 section 9.5: the sizes of
/// all but the last as three bytes each, then the partitions back to back.
fn token_partitions(data: &[u8], count: usize) -> Result<Vec<BoolDecoder<'_>>, ImageError> {
    let (sizes, mut rest) = data
        .split_at_checked(3 * (count - 1))
        .ok_or_else(|| malformed("the VP8 partition sizes are cut short"))?;
    let mut partitions = Vec::with_capacity(count);
    for size in sizes.as_chunks::<3>().0 {
        let size = usize::from(size[0]) | usize::from(size[1]) << 8 | usize::from(size[2]) << 16;
        let (partition, tail) = rest
            .split_at_checked(size)
            .ok_or_else(|| malformed("a VP8 token partition runs past its chunk"))?;
        partitions.push(BoolDecoder::new(partition));
        rest = tail;
    }
    partitions.push(BoolDecoder::new(rest));
    Ok(partitions)
}

/// Each segment's dequantisation factors, RFC 6386 section 14.1 as the
/// reference decoder's `dequant_init` computes them.
fn dequant_factors(header: &FrameHeader) -> [Dequant; SEGMENTS] {
    let q = &header.quantizer;
    let last = DC_QUANT.len() as i32 - 1;
    std::array::from_fn(|segment| {
        let index = match &header.segmentation {
            Some(segmentation) if segmentation.absolute => segmentation.quantizer[segment],
            Some(segmentation) => q.y_ac + segmentation.quantizer[segment],
            None => q.y_ac,
        };
        let dc = |delta: i32| DC_QUANT[(index + delta).clamp(0, last) as usize];
        let ac = |delta: i32| AC_QUANT[(index + delta).clamp(0, last) as usize];
        Dequant {
            y1: [dc(q.y_dc_delta), ac(0)],
            y2: [
                dc(q.y2_dc_delta) * Y2_DC_SCALE,
                (ac(q.y2_ac_delta) * Y2_AC_SCALE_PERCENT / 100).max(MIN_Y2_AC),
            ],
            uv: [dc(q.uv_dc_delta).min(MAX_CHROMA_DC), ac(q.uv_ac_delta)],
        }
    })
}

/// Each segment's loop filter strength for a whole-block macroblock and for
/// a B_PRED one, RFC 6386 sections 9.3, 9.4 and 15.4 as the reference
/// decoder's `calculate_filter_parameters` computes them for a key frame.
fn filter_strengths(header: &FrameHeader) -> [[Strength; 2]; SEGMENTS] {
    let filter = &header.filter;
    std::array::from_fn(|segment| {
        let base = match &header.segmentation {
            Some(segmentation) if segmentation.absolute => segmentation.filter_level[segment],
            Some(segmentation) => filter.level + segmentation.filter_level[segment],
            None => filter.level,
        }
        .clamp(0, filter::MAX_LEVEL);
        std::array::from_fn(|b_pred| {
            let mut level = base;
            if let Some((reference, mode)) = filter.deltas {
                // Every macroblock of a key frame is intra, the first
                // reference delta's; the first mode delta is B_PRED's.
                level += reference[0];
                if b_pred == 1 {
                    level += mode[0];
                }
            }
            Strength::new(level.clamp(0, filter::MAX_LEVEL), filter.sharpness)
        })
    })
}

/// Decodes and reconstructs every macroblock, then runs the loop filter over
/// the frame.
fn decode_macroblocks(
    header: &FrameHeader,
    modes: &mut BoolDecoder<'_>,
    partitions: &mut [BoolDecoder<'_>],
    width: usize,
    height: usize,
) -> Result<Picture, ImageError> {
    let (mb_width, mb_height) = (width.div_ceil(16), height.div_ceil(16));
    let (y_stride, uv_stride) = (mb_width * 16, mb_width * 8);
    let mut picture = Picture {
        width,
        height,
        y: vec![0; y_stride * mb_height * 16],
        u: vec![0; uv_stride * mb_height * 8],
        v: vec![0; uv_stride * mb_height * 8],
        y_stride,
        uv_stride,
    };
    let dequant = dequant_factors(header);
    let strengths = filter_strengths(header);
    let mut above = vec![Context::default(); mb_width];
    // Each macroblock's filter strength, and whether its subblock edges are
    // filtered.
    let mut filtering = Vec::with_capacity(mb_width * mb_height);
    for mb_y in 0..mb_height {
        let mut left = Context::default();
        // Rows of macroblocks take turns at the partitions.
        let tokens = &mut partitions[mb_y % partitions.len()];
        for (mb_x, above) in above.iter_mut().enumerate() {
            let macroblock = read_modes(modes, header, &mut above.modes, &mut left.modes);
            let mut coeffs = [[0i16; 16]; 25];
            let has_coeffs = if macroblock.skip {
                clear_skipped(macroblock.mode, &mut above.nonzero);
                clear_skipped(macroblock.mode, &mut left.nonzero);
                false
            } else {
                read_coefficients(
                    tokens,
                    &header.coeff_probs,
                    &dequant[macroblock.segment],
                    macroblock.mode,
                    &mut above.nonzero,
                    &mut left.nonzero,
                    &mut coeffs,
                )
            };
            if modes.past_end() || tokens.past_end() {
                return Err(malformed("a VP8 partition ends before its macroblocks"));
            }
            reconstruct(&mut picture, mb_x, mb_y, &macroblock, &mut coeffs);
            let b_pred = macroblock.mode == Mode::B;
            filtering.push((
                strengths[macroblock.segment][usize::from(b_pred)],
                has_coeffs || b_pred,
            ));
        }
    }
    // A frame level of zero turns the filter off whatever the segments say.
    if header.filter.level != 0 {
        for (i, &(strength, inner)) in filtering.iter().enumerate() {
            let planes = [
                filter::Plane {
                    pixels: &mut picture.y,
                    stride: y_stride,
                },
                filter::Plane {
                    pixels: &mut picture.u,
                    stride: uv_stride,
                },
                filter::Plane {
                    pixels: &mut picture.v,
                    stride: uv_stride,
                },
            ];
            filter::macroblock(
                header.filter.kind,
                planes,
                i % mb_width,
                i / mb_width,
                strength,
                inner,
            );
        }
    }
    Ok(picture)
}

/// Reads one macroblock's prediction record from the first partition, RFC
/// 6386 sections 10 and 11, updating the subblock mode contexts above and
/// to the left.
fn read_modes(
    bits: &mut BoolDecoder<'_>,
    header: &FrameHeader,
    above: &mut [BMode; 4],
    left: &mut [BMode; 4],
) -> Macroblock {
    let segment = match &header.segmentation {
        Some(segmentation) if segmentation.update_map => {
            bits.read_tree(&SEGMENT_TREE, &segmentation.tree_probs)
        }
        _ => 0,
    };
    let skip = header
        .skip_prob
        .is_some_and(|probability| bits.read(probability));
    let mode = MODES[bits.read_tree(&KF_YMODE_TREE, &KF_YMODE_PROB)];
    let sub = if mode == Mode::B {
        let mut sub = [BMode::Dc; 16];
        for i in 0..16 {
            let a = if i < 4 { above[i] } else { sub[i - 4] };
            let l = if i % 4 == 0 { left[i / 4] } else { sub[i - 1] };
            let probabilities = &KF_BMODE_PROB[a as usize][l as usize];
            sub[i] = BMODES[bits.read_tree(&BMODE_TREE, probabilities)];
        }
        sub
    } else {
        [implied_subblock_mode(mode); 16]
    };
    *above = [sub[12], sub[13], sub[14], sub[15]];
    *left = [sub[3], sub[7], sub[11], sub[15]];
    let uv_mode = MODES[bits.read_tree(&UV_MODE_TREE, &KF_UV_MODE_PROB)];
    Macroblock {
        segment,
        skip,
        mode,
        sub,
        uv_mode,
    }
}

/// The subblock mode a whole-block mode stands for in its neighbours'
/// contexts, RFC 6386 section 11.3.
fn implied_subblock_mode(mode: Mode) -> BMode {
    match mode {
        Mode::V => BMode::Ve,
        Mode::H => BMode::He,
        Mode::Tm => BMode::Tm,
        Mode::Dc | Mode::B => BMode::Dc,
    }
}

/// Marks a skipped macroblock's blocks empty in one direction's
/// coefficient context, as the reference decoder's `reset_mb_context` does:
/// a B_PRED macroblock has no Y2 block, so it leaves Y2's context (the last)
/// as the macroblocks before it set it.
fn clear_skipped(mode: Mode, context: &mut [bool; 9]) {
    let cleared = if mode == Mode::B { 8 } else { 9 };
    context[..cleared].fill(false);
}

/// Reads a macroblock's coefficients, dequantised, RFC 6386 section 13 as
/// the reference decoder's `decode_mb_tokens` does: the Y2 block unless the
/// macroblock is B_PRED, the sixteen luma blocks, then four U and four V.
/// Returns whether any block had a token other than an immediate end of
/// block.
fn read_coefficients(
    bits: &mut BoolDecoder<'_>,
    probs: &[[[[u8; 11]; 3]; 8]; 4],
    dequant: &Dequant,
    mode: Mode,
    above: &mut [bool; 9],
    left: &mut [bool; 9],
    coeffs: &mut [[i16; 16]; 25],
) -> bool {
    let mut any = false;
    let mut block =
        |plane: usize, first: usize, a: usize, l: usize, factors: [i32; 2], out: &mut [i16; 16]| {
            let context = usize::from(above[a]) + usize::from(left[l]);
            let nonzero = read_block(bits, &probs[plane], first, context, factors, out);
            above[a] = nonzero;
            left[l] = nonzero;
            any |= nonzero;
        };
    let (luma_plane, first) = if mode == Mode::B {
        (PLANE_Y_WITH_DC, 0)
    } else {
        block(PLANE_Y2, 0, 8, 8, dequant.y2, &mut coeffs[Y2_BLOCK]);
        (PLANE_Y_AFTER_Y2, 1)
    };
    for (i, out) in coeffs[..16].iter_mut().enumerate() {
        block(luma_plane, first, i % 4, i / 4, dequant.y1, out);
    }
    for (context, start) in [(4, U_BLOCKS), (6, V_BLOCKS)] {
        for (i, out) in coeffs[start..start + 4].iter_mut().enumerate() {
            block(
                PLANE_CHROMA,
                0,
                context + i % 2,
                context + i / 2,
                dequant.uv,
                out,
            );
        }
    }
    any
}

/// Reads one block's tokens from position `first`, RFC 6386 section 13.2
/// and 13.3, storing each coefficient dequantised in its zigzag place.
/// Returns whether there was any token before the end of the block.
fn read_block(
    bits: &mut BoolDecoder<'_>,
    probs: &[[[u8; 11]; 3]; 8],
    first: usize,
    mut context: usize,
    factors: [i32; 2],
    out: &mut [i16; 16],
) -> bool {
    let mut position = first;
    let mut after_zero = false;
    while position < 16 {
        let probabilities = &probs[COEFF_BANDS[position]][context];
        // An end of block cannot follow a zero, so its branch is skipped.
        let start = if after_zero { 2 } else { 0 };
        let token = bits.read_tree_from(&COEFF_TREE, probabilities, start);
        if token == Token::Eob as usize {
            break;
        }
        let magnitude = if token <= Token::Four as usize {
            token as i32
        } else {
            let category = token - Token::Cat1 as usize;
            let extra = CATEGORY_PROBS[category]
                .iter()
                .fold(0, |value, &probability| {
                    (value << 1) | i32::from(bits.read(probability))
                });
            CATEGORY_BASE[category] + extra
        };
        after_zero = magnitude == 0;
        context = match magnitude {
            0 => 0,
            1 => 1,
            _ => 2,
        };
        if magnitude != 0 {
            let value = if bits.flag() { -magnitude } else { magnitude };
            // Stored in 16 bits, as the reference decoder stores it.
            out[ZIGZAG[position]] = (value * factors[usize::from(position > 0)]) as i16;
        }
        position += 1;
    }
    position > first
}

/// Adds the inverse DCT of `coeffs` to the predicted 4x4 block at `at`,
/// clamping, RFC 6386 section 14.5.
fn add_residue(buffer: &mut [u8], stride: usize, at: usize, coeffs: &[i16; 16]) {
    if coeffs.iter().all(|&c| c == 0) {
        return;
    }
    let residue = transform::idct(coeffs);
    for (row, residue) in residue.as_chunks::<4>().0.iter().enumerate() {
        let pixels = &mut buffer[at + row * stride..][..4];
        for (pixel, &r) in pixels.iter_mut().zip(residue) {
            *pixel = (i32::from(*pixel) + i32::from(r)).clamp(0, 255) as u8;
        }
    }
}

/// Fills a workspace's edge — the corner, the `size` pixels above and the
/// column to the left — for the `size`-pixel block at macroblock `(mb_x,
/// mb_y)` of `plane`, with the RFC's 127 above the frame and 129 left of it.
fn fill_edges(
    workspace: &mut [u8],
    stride: usize,
    plane: &[u8],
    plane_stride: usize,
    (mb_x, mb_y): (usize, usize),
    size: usize,
) {
    let (x, y) = (mb_x * size, mb_y * size);
    workspace[0] = match (mb_x, mb_y) {
        (_, 0) => ABOVE_EDGE,
        (0, _) => LEFT_EDGE,
        _ => plane[(y - 1) * plane_stride + x - 1],
    };
    if mb_y == 0 {
        workspace[1..=size].fill(ABOVE_EDGE);
    } else {
        let above = (y - 1) * plane_stride + x;
        workspace[1..=size].copy_from_slice(&plane[above..above + size]);
    }
    for row in 0..size {
        workspace[(row + 1) * stride] = if mb_x == 0 {
            LEFT_EDGE
        } else {
            plane[(y + row) * plane_stride + x - 1]
        };
    }
}

/// The four pixels above and to the right of the luma macroblock at
/// `(mb_x, mb_y)`, RFC 6386 section 12.3: the bottom row of the macroblock
/// above and to the right; on the right edge, which has none, the last pixel
/// above repeated; on the top row, 127.
fn above_right(plane: &[u8], stride: usize, mb_x: usize, mb_y: usize) -> [u8; 4] {
    if mb_y == 0 {
        return [ABOVE_EDGE; 4];
    }
    let row = (mb_y * 16 - 1) * stride;
    let x = mb_x * 16 + 16;
    if x < stride {
        std::array::from_fn(|i| plane[row + x + i])
    } else {
        [plane[row + x - 1]; 4]
    }
}

/// Predicts a macroblock and adds its residue, RFC 6386 sections 12 and 14,
/// writing it into the picture's planes.
fn reconstruct(
    picture: &mut Picture,
    mb_x: usize,
    mb_y: usize,
    macroblock: &Macroblock,
    coeffs: &mut [[i16; 16]; 25],
) {
    let (above, left) = (mb_y > 0, mb_x > 0);

    let mut luma = [0u8; LUMA_STRIDE * 17];
    let y_stride = picture.y_stride;
    fill_edges(
        &mut luma,
        LUMA_STRIDE,
        &picture.y,
        y_stride,
        (mb_x, mb_y),
        16,
    );
    luma[17..21].copy_from_slice(&above_right(&picture.y, y_stride, mb_x, mb_y));
    let at = LUMA_STRIDE + 1;
    let subblock_at = |i: usize| at + (i / 4) * 4 * LUMA_STRIDE + (i % 4) * 4;
    if macroblock.mode == Mode::B {
        // The right column's lower subblocks have no reconstructed pixels
        // above and to their right; they use the top-right subblock's.
        for row in [4, 8, 12] {
            luma.copy_within(17..21, row * LUMA_STRIDE + 17);
        }
        for (i, (&mode, coeffs)) in macroblock.sub.iter().zip(&coeffs[..16]).enumerate() {
            predict::subblock(&mut luma, LUMA_STRIDE, subblock_at(i), mode);
            add_residue(&mut luma, LUMA_STRIDE, subblock_at(i), coeffs);
        }
    } else {
        predict::whole_block(&mut luma, LUMA_STRIDE, at, 16, macroblock.mode, above, left);
        let dc = transform::iwht(&coeffs[Y2_BLOCK]);
        for (i, (block, dc)) in coeffs[..16].iter_mut().zip(dc).enumerate() {
            block[0] = dc;
            add_residue(&mut luma, LUMA_STRIDE, subblock_at(i), block);
        }
    }
    for row in 0..16 {
        let start = (mb_y * 16 + row) * y_stride + mb_x * 16;
        picture.y[start..start + 16].copy_from_slice(&luma[at + row * LUMA_STRIDE..][..16]);
    }

    let uv_stride = picture.uv_stride;
    for (plane, blocks) in [(&mut picture.u, U_BLOCKS), (&mut picture.v, V_BLOCKS)] {
        let mut chroma = [0u8; CHROMA_STRIDE * 9];
        fill_edges(
            &mut chroma,
            CHROMA_STRIDE,
            plane,
            uv_stride,
            (mb_x, mb_y),
            8,
        );
        let at = CHROMA_STRIDE + 1;
        predict::whole_block(
            &mut chroma,
            CHROMA_STRIDE,
            at,
            8,
            macroblock.uv_mode,
            above,
            left,
        );
        for (i, coeffs) in coeffs[blocks..blocks + 4].iter().enumerate() {
            let block_at = at + (i / 2) * 4 * CHROMA_STRIDE + (i % 2) * 4;
            add_residue(&mut chroma, CHROMA_STRIDE, block_at, coeffs);
        }
        for row in 0..8 {
            let start = (mb_y * 8 + row) * uv_stride + mb_x * 8;
            plane[start..start + 8].copy_from_slice(&chroma[at + row * CHROMA_STRIDE..][..8]);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// RFC 6386 section 11.3, item 4: DC_PRED stands for B_DC_PRED, V_PRED
    /// for B_VE_PRED, H_PRED for B_HE_PRED and TM_PRED for B_TM_PRED.
    #[test]
    fn whole_block_modes_stand_for_their_subblock_modes() {
        let implied: Vec<BMode> = [Mode::Dc, Mode::V, Mode::H, Mode::Tm]
            .into_iter()
            .map(implied_subblock_mode)
            .collect();
        assert_eq!(implied, [BMode::Dc, BMode::Ve, BMode::He, BMode::Tm]);
    }

    /// A skipped macroblock empties every context it has a block for, so
    /// B_PRED, which has no Y2 block, keeps the Y2 context it was given.
    #[test]
    fn a_skipped_macroblock_clears_the_contexts_of_the_blocks_it_has() {
        let mut whole = [true; 9];
        clear_skipped(Mode::Tm, &mut whole);
        assert_eq!(whole, [false; 9]);
        let mut subblocks = [true; 9];
        clear_skipped(Mode::B, &mut subblocks);
        assert_eq!(
            subblocks,
            [false, false, false, false, false, false, false, false, true]
        );
    }

    /// RFC 6386 section 12.3's pixels above and to the right, on a frame two
    /// macroblocks wide whose sixteenth row holds each pixel's column: the
    /// next macroblock's first four, the last pixel repeated on the right
    /// edge, and 127 on the top row.
    #[test]
    fn the_pixels_above_and_right_follow_section_12_3() {
        let stride = 32;
        let mut plane = vec![0u8; stride * 32];
        for (x, pixel) in plane[15 * stride..16 * stride].iter_mut().enumerate() {
            *pixel = x as u8;
        }
        assert_eq!(above_right(&plane, stride, 0, 1), [16, 17, 18, 19]);
        assert_eq!(above_right(&plane, stride, 1, 1), [31; 4]);
        assert_eq!(above_right(&plane, stride, 0, 0), [ABOVE_EDGE; 4]);
    }
}
