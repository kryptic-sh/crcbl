//! The lossless (`VP8L`) bitstream of RFC 9649, decoded to RGBA8.

use super::malformed;
use crate::{ImageError, Limits, Rgba8Image};

/// The byte every lossless bitstream starts with.
const VP8L_SIGNATURE: u8 = 0x2f;

/// The longest code a lossless prefix code may have.
const MAX_CODE_LENGTH: usize = 15;

/// The literal symbols of the green, red, blue and alpha codes.
const LITERALS: usize = 256;

/// The length prefix codes that share the green code's alphabet.
const LENGTH_CODES: usize = 24;

/// The distance prefix codes.
const DISTANCE_CODES: usize = 40;

/// The code-length code's symbols, in the order their lengths are stored.
const CODE_LENGTH_ORDER: [usize; 19] = [
    17, 18, 0, 1, 2, 3, 4, 5, 16, 6, 7, 8, 9, 10, 11, 12, 13, 14, 15,
];

/// The colour cache's hash multiplier, from the specification.
const COLOR_CACHE_MULTIPLIER: u32 = 0x1e35_a7bd;

/// The largest colour cache, as `color_cache_code_bits`.
const MAX_COLOR_CACHE_BITS: u32 = 11;

/// Distance codes up to this name a pixel near the current one through
/// [`DISTANCE_MAP`]; larger ones are a scan-line distance plus this.
const NEAR_DISTANCE_CODES: usize = 120;

/// RFC 9649's Figure 20: each near distance code's `(x, y)` offset, `y` rows
/// up and `x` columns left of the current pixel.
///
/// Laid out as the RFC prints it, seven to a row, so it can be checked
/// against Figure 20 by eye.
#[rustfmt::skip]
const DISTANCE_MAP: [(i8, i8); NEAR_DISTANCE_CODES] = [
    (0, 1), (1, 0), (1, 1), (-1, 1), (0, 2), (2, 0), (1, 2),
    (-1, 2), (2, 1), (-2, 1), (2, 2), (-2, 2), (0, 3), (3, 0),
    (1, 3), (-1, 3), (3, 1), (-3, 1), (2, 3), (-2, 3), (3, 2),
    (-3, 2), (0, 4), (4, 0), (1, 4), (-1, 4), (4, 1), (-4, 1),
    (3, 3), (-3, 3), (2, 4), (-2, 4), (4, 2), (-4, 2), (0, 5),
    (3, 4), (-3, 4), (4, 3), (-4, 3), (5, 0), (1, 5), (-1, 5),
    (5, 1), (-5, 1), (2, 5), (-2, 5), (5, 2), (-5, 2), (4, 4),
    (-4, 4), (3, 5), (-3, 5), (5, 3), (-5, 3), (0, 6), (6, 0),
    (1, 6), (-1, 6), (6, 1), (-6, 1), (2, 6), (-2, 6), (6, 2),
    (-6, 2), (4, 5), (-4, 5), (5, 4), (-5, 4), (3, 6), (-3, 6),
    (6, 3), (-6, 3), (0, 7), (7, 0), (1, 7), (-1, 7), (5, 5),
    (-5, 5), (7, 1), (-7, 1), (4, 6), (-4, 6), (6, 4), (-6, 4),
    (2, 7), (-2, 7), (7, 2), (-7, 2), (3, 7), (-3, 7), (7, 3),
    (-7, 3), (5, 6), (-5, 6), (6, 5), (-6, 5), (8, 0), (4, 7),
    (-4, 7), (7, 4), (-7, 4), (8, 1), (8, 2), (6, 6), (-6, 6),
    (8, 3), (5, 7), (-5, 7), (7, 5), (-7, 5), (8, 4), (6, 7),
    (-6, 7), (7, 6), (-7, 6), (8, 5), (7, 7), (-7, 7), (8, 6),
    (8, 7),
];

/// The lossless bitstream's bits: bytes in order, each least significant bit
/// first.
struct Bits<'a> {
    data: &'a [u8],
    at: usize,
    acc: u64,
    count: u32,
}

impl<'a> Bits<'a> {
    fn new(data: &'a [u8]) -> Self {
        Self {
            data,
            at: 0,
            acc: 0,
            count: 0,
        }
    }

    /// `n` bits, the first read the least significant.
    fn read(&mut self, n: u32) -> Result<u32, ImageError> {
        debug_assert!(n <= 32);
        while self.count < n {
            let byte = *self
                .data
                .get(self.at)
                .ok_or_else(|| malformed("the bitstream ends early"))?;
            self.acc |= u64::from(byte) << self.count;
            self.at += 1;
            self.count += 8;
        }
        let value = (self.acc & ((1u64 << n) - 1)) as u32;
        self.acc >>= n;
        self.count -= n;
        Ok(value)
    }

    fn flag(&mut self) -> Result<bool, ImageError> {
        Ok(self.read(1)? == 1)
    }
}

/// A canonical prefix code, kept as the count of codes of each length and
/// its symbols in code order: only the symbols it has.
#[derive(Debug)]
struct Prefix {
    counts: [u16; MAX_CODE_LENGTH + 1],
    symbols: Vec<u16>,
}

impl Prefix {
    /// The code `lengths` describes, symbol by symbol; a tree that is not
    /// complete is refused, except the tree of one symbol.
    fn from_lengths(lengths: &[u8]) -> Result<Self, ImageError> {
        let mut counts = [0u16; MAX_CODE_LENGTH + 1];
        for &length in lengths {
            counts[usize::from(length)] += 1;
        }
        counts[0] = 0;
        let used = check_counts(&counts)?;
        let mut symbols = Vec::with_capacity(used);
        for length in 1..=MAX_CODE_LENGTH {
            for (symbol, &l) in lengths.iter().enumerate() {
                if usize::from(l) == length {
                    symbols.push(symbol as u16);
                }
            }
        }
        Ok(Self { counts, symbols })
    }

    /// The next symbol. A code of one symbol takes no bits.
    fn decode(&self, bits: &mut Bits<'_>) -> Result<u16, ImageError> {
        if self.symbols.len() == 1 {
            return Ok(self.symbols[0]);
        }
        // Canonical decoding, a bit at a time, as zlib's `puff` does it.
        let (mut code, mut first, mut index) = (0i32, 0i32, 0i32);
        for &count in &self.counts[1..] {
            code |= bits.read(1)? as i32;
            let count = i32::from(count);
            if code - first < count {
                return Ok(self.symbols[(index + code - first) as usize]);
            }
            index += count;
            first = (first + count) << 1;
            code <<= 1;
        }
        Err(malformed("a prefix code past its longest length"))
    }
}

/// How many symbols a code of `counts` codes of each length has, if that is
/// a code: a tree that is not complete is refused, except the tree of one
/// symbol. `counts[0]`, the unused symbols, is ignored.
fn check_counts(counts: &[u16; MAX_CODE_LENGTH + 1]) -> Result<usize, ImageError> {
    let used: usize = counts[1..].iter().map(|&count| usize::from(count)).sum();
    match used {
        0 => return Err(malformed("a prefix code with no symbols")),
        1 => return Ok(used),
        _ => {}
    }
    // The Kraft sum, in units of the longest code: a complete tree's is
    // exactly one.
    let mut space: i64 = 1;
    for &count in &counts[1..] {
        space = space * 2 - i64::from(count);
        if space < 0 {
            return Err(malformed("a prefix code with more codes than fit"));
        }
    }
    if space != 0 {
        return Err(malformed("an incomplete prefix code"));
    }
    Ok(used)
}

/// Reads one prefix code's lengths over an alphabet of `alphabet` symbols,
/// handing each run of symbols that share a length to `set` as `(first
/// symbol, run length, code length)`, every one inside the alphabet. Symbols
/// it names no length for have none. The runs of the normal form are
/// disjoint and ascending; the simple form's two symbols may be one.
fn read_lengths(
    bits: &mut Bits<'_>,
    alphabet: usize,
    mut set: impl FnMut(usize, usize, u8),
) -> Result<(), ImageError> {
    if bits.flag()? {
        // The simple form: one or two symbols of length one.
        let two = bits.flag()?;
        let first_bits = if bits.flag()? { 8 } else { 1 };
        let mut symbol_bits = first_bits;
        for _ in 0..=usize::from(two) {
            let symbol = bits.read(symbol_bits)? as usize;
            if symbol >= alphabet {
                return Err(malformed("a symbol past its alphabet"));
            }
            set(symbol, 1, 1);
            symbol_bits = 8;
        }
        return Ok(());
    }

    let mut code_length_lengths = [0u8; CODE_LENGTH_ORDER.len()];
    let stored = 4 + bits.read(4)? as usize;
    for &symbol in CODE_LENGTH_ORDER.iter().take(stored) {
        code_length_lengths[symbol] = bits.read(3)? as u8;
    }
    let code_lengths = Prefix::from_lengths(&code_length_lengths)?;
    let mut budget = if bits.flag()? {
        let length_bits = 2 + 2 * bits.read(3)?;
        let max_symbol = 2 + bits.read(length_bits)? as usize;
        if max_symbol > alphabet {
            return Err(malformed("more code lengths than the alphabet has"));
        }
        max_symbol
    } else {
        alphabet
    };
    let mut symbol = 0;
    let mut previous = 8u8;
    while symbol < alphabet && budget > 0 {
        budget -= 1;
        let code = code_lengths.decode(bits)?;
        let (value, repeat) = match code {
            0..=15 => {
                set(symbol, 1, code as u8);
                symbol += 1;
                if code != 0 {
                    previous = code as u8;
                }
                continue;
            }
            16 => (previous, 3 + bits.read(2)? as usize),
            17 => (0, 3 + bits.read(3)? as usize),
            _ => (0, 11 + bits.read(7)? as usize),
        };
        if symbol + repeat > alphabet {
            return Err(malformed("a repeated code length past the alphabet"));
        }
        set(symbol, repeat, value);
        symbol += repeat;
    }
    Ok(())
}

/// Reads one prefix code over an alphabet of `alphabet` symbols.
fn read_prefix(bits: &mut Bits<'_>, alphabet: usize) -> Result<Prefix, ImageError> {
    let mut lengths = vec![0u8; alphabet];
    read_lengths(bits, alphabet, |first, run, length| {
        lengths[first..first + run].fill(length);
    })?;
    Prefix::from_lengths(&lengths)
}

/// Reads past one prefix code over an alphabet of `alphabet` symbols that
/// nothing will decode with, refusing it as [`read_prefix`] would but
/// building nothing: only its count of codes of each length. The simple
/// form's two symbols counted as two where they are one makes no
/// difference, as either is a code.
fn skip_prefix(bits: &mut Bits<'_>, alphabet: usize) -> Result<(), ImageError> {
    let mut counts = [0u16; MAX_CODE_LENGTH + 1];
    read_lengths(bits, alphabet, |_, run, length| {
        // A run is inside an alphabet of at most `LITERALS + LENGTH_CODES +
        // (1 << MAX_COLOR_CACHE_BITS)` symbols, so no count passes it.
        counts[usize::from(length)] += run as u16;
    })?;
    counts[0] = 0;
    check_counts(&counts).map(drop)
}

/// The five codes one block of an image is read with.
struct Group {
    green: Prefix,
    red: Prefix,
    blue: Prefix,
    alpha: Prefix,
    distance: Prefix,
}

/// The alphabet sizes of a group's five codes, in the order they are stored.
fn group_alphabets(cache_size: usize) -> [usize; 5] {
    [
        LITERALS + LENGTH_CODES + cache_size,
        LITERALS,
        LITERALS,
        LITERALS,
        DISTANCE_CODES,
    ]
}

/// Replaces each entropy-image pixel by the index its meta-code's group will
/// have among the groups the image uses, numbered in meta-code order, and
/// returns which of the meta-codes up to the largest named are used.
fn dense_groups(entropy: &mut [u32]) -> Vec<bool> {
    let meta = |pixel: u32| ((pixel >> 8) & 0xffff) as usize;
    let count = entropy
        .iter()
        .map(|&pixel| meta(pixel))
        .max()
        .map_or(0, |max| max + 1);
    let mut used = vec![false; count];
    for &pixel in entropy.iter() {
        used[meta(pixel)] = true;
    }
    let mut index = vec![0u32; count];
    let named = (0..count).filter(|&meta_code| used[meta_code]);
    for (dense, meta_code) in (0u32..).zip(named) {
        index[meta_code] = dense;
    }
    for pixel in entropy.iter_mut() {
        *pixel = index[meta(*pixel)];
    }
    used
}

/// Reads one group per entry of `used`, in order, building only those it
/// marks: RFC 9649 stores every group up to the largest meta-code, used or
/// not, and libwebp too reads the rest only to pass and check them.
fn read_groups(
    bits: &mut Bits<'_>,
    used: &[bool],
    cache_size: usize,
) -> Result<Vec<Group>, ImageError> {
    let alphabets = group_alphabets(cache_size);
    let mut groups = Vec::with_capacity(used.iter().filter(|&&used| used).count());
    for &used in used {
        if used {
            groups.push(Group {
                green: read_prefix(bits, alphabets[0])?,
                red: read_prefix(bits, alphabets[1])?,
                blue: read_prefix(bits, alphabets[2])?,
                alpha: read_prefix(bits, alphabets[3])?,
                distance: read_prefix(bits, alphabets[4])?,
            });
        } else {
            for alphabet in alphabets {
                skip_prefix(bits, alphabet)?;
            }
        }
    }
    Ok(groups)
}

/// Turns an LZ77 prefix code and its extra bits into a length or distance.
fn lz77_value(prefix: u16, bits: &mut Bits<'_>) -> Result<usize, ImageError> {
    let prefix = u32::from(prefix);
    if prefix < 4 {
        return Ok(prefix as usize + 1);
    }
    let extra = (prefix - 2) >> 1;
    let offset = (2 + (prefix & 1)) << extra;
    Ok((offset + bits.read(extra)? + 1) as usize)
}

/// Decodes one entropy-coded image of `width` × `height` pixels, as ARGB
/// words. Only the main image may carry meta prefix codes.
fn decode_image(
    bits: &mut Bits<'_>,
    width: usize,
    height: usize,
    main: bool,
) -> Result<Vec<u32>, ImageError> {
    let cache_bits = if bits.flag()? {
        let cache_bits = bits.read(4)?;
        if !(1..=MAX_COLOR_CACHE_BITS).contains(&cache_bits) {
            return Err(malformed("a colour cache size out of range"));
        }
        Some(cache_bits)
    } else {
        None
    };
    let cache_size = cache_bits.map_or(0, |cache_bits| 1usize << cache_bits);

    // The entropy image, if any, holds each block's index into `groups`.
    let (entropy, prefix_bits, used) = if main && bits.flag()? {
        let prefix_bits = bits.read(3)? + 2;
        let entropy_width = width.div_ceil(1 << prefix_bits);
        let entropy_height = height.div_ceil(1 << prefix_bits);
        let mut entropy = decode_image(bits, entropy_width, entropy_height, false)?;
        let used = dense_groups(&mut entropy);
        (Some((entropy, entropy_width)), prefix_bits, used)
    } else {
        (None, 0, vec![true])
    };
    let groups = read_groups(bits, &used, cache_size)?;

    let total = width * height;
    let mut pixels: Vec<u32> = Vec::with_capacity(total);
    let mut cache = vec![0u32; cache_size];
    let mut cached = 0;
    let insert = |cache: &mut [u32], pixels: &[u32], cached: &mut usize| {
        if let Some(cache_bits) = cache_bits {
            for &pixel in &pixels[*cached..] {
                let key = COLOR_CACHE_MULTIPLIER.wrapping_mul(pixel) >> (32 - cache_bits);
                cache[key as usize] = pixel;
            }
        }
        *cached = pixels.len();
    };
    while pixels.len() < total {
        let at = pixels.len();
        let group = match &entropy {
            Some((entropy, entropy_width)) => {
                let (x, y) = (at % width, at / width);
                let index = (y >> prefix_bits) * entropy_width + (x >> prefix_bits);
                &groups[entropy[index] as usize]
            }
            None => &groups[0],
        };
        let symbol = usize::from(group.green.decode(bits)?);
        if symbol < LITERALS {
            let red = u32::from(group.red.decode(bits)?);
            let blue = u32::from(group.blue.decode(bits)?);
            let alpha = u32::from(group.alpha.decode(bits)?);
            pixels.push((alpha << 24) | (red << 16) | ((symbol as u32) << 8) | blue);
        } else if symbol < LITERALS + LENGTH_CODES {
            let length = lz77_value((symbol - LITERALS) as u16, bits)?;
            let code = lz77_value(group.distance.decode(bits)?, bits)?;
            let distance = if code > NEAR_DISTANCE_CODES {
                code - NEAR_DISTANCE_CODES
            } else {
                let (dx, dy) = DISTANCE_MAP[code - 1];
                (i64::from(dx) + i64::from(dy) * width as i64).max(1) as usize
            };
            if distance > at || at + length > total {
                return Err(malformed("a backward reference outside the image"));
            }
            for i in 0..length {
                let pixel = pixels[at + i - distance];
                pixels.push(pixel);
            }
        } else {
            let index = symbol - LITERALS - LENGTH_CODES;
            insert(&mut cache, &pixels, &mut cached);
            let pixel = *cache
                .get(index)
                .ok_or_else(|| malformed("a colour cache index past the cache"))?;
            pixels.push(pixel);
        }
        insert(&mut cache, &pixels, &mut cached);
    }
    Ok(pixels)
}

/// One inverse transform, as read.
enum Transform {
    Predictor { bits: u32, modes: Vec<u32> },
    Color { bits: u32, elements: Vec<u32> },
    SubtractGreen,
    ColorIndexing { palette: Vec<u32>, width_bits: u32 },
}

/// Decodes a `VP8L` chunk's bitstream to RGBA8.
pub(super) fn decode_vp8l(data: &[u8], limits: &Limits) -> Result<Rgba8Image, ImageError> {
    if data.first() != Some(&VP8L_SIGNATURE) {
        return Err(malformed("no lossless signature"));
    }
    let mut bits = Bits::new(&data[1..]);
    let width = bits.read(14)? + 1;
    let height = bits.read(14)? + 1;
    let _alpha_is_used = bits.read(1)?;
    if bits.read(3)? != 0 {
        return Err(malformed("a lossless version other than 0"));
    }
    let bytes = limits.rgba_bytes(width, height)?;
    let pixels = decode_stream(&mut bits, width as usize, height as usize)?;
    let mut rgba = Vec::with_capacity(bytes);
    for pixel in pixels {
        rgba.extend_from_slice(&[
            (pixel >> 16) as u8,
            (pixel >> 8) as u8,
            pixel as u8,
            (pixel >> 24) as u8,
        ]);
    }
    Ok(Rgba8Image::from_sized(width, height, rgba))
}

/// Decodes a lossless image stream with no header, `width` × `height`
/// pixels the caller already knows and has checked against its limits, as
/// ARGB words: how an `ALPH` chunk codes its alpha (RFC 9649 section
/// 2.7.1.2).
pub(super) fn decode_headerless(
    data: &[u8],
    width: usize,
    height: usize,
) -> Result<Vec<u32>, ImageError> {
    decode_stream(&mut Bits::new(data), width, height)
}

/// Decodes an image stream past its header: the transforms, then the
/// entropy-coded image they apply to, as ARGB words.
fn decode_stream(
    bits: &mut Bits<'_>,
    full_width: usize,
    height: usize,
) -> Result<Vec<u32>, ImageError> {
    let mut transforms = Vec::new();
    let mut seen = [false; 4];
    let mut coded_width = full_width;
    while bits.flag()? {
        let kind = bits.read(2)? as usize;
        if std::mem::replace(&mut seen[kind], true) {
            return Err(malformed("a transform used twice"));
        }
        transforms.push(match kind {
            0 | 1 => {
                let block_bits = bits.read(3)? + 2;
                let blocks_wide = coded_width.div_ceil(1 << block_bits);
                let blocks_high = height.div_ceil(1 << block_bits);
                let data = decode_image(bits, blocks_wide, blocks_high, false)?;
                if kind == 0 {
                    Transform::Predictor {
                        bits: block_bits,
                        modes: data,
                    }
                } else {
                    Transform::Color {
                        bits: block_bits,
                        elements: data,
                    }
                }
            }
            2 => Transform::SubtractGreen,
            _ => {
                let size = bits.read(8)? as usize + 1;
                let mut palette = decode_image(bits, size, 1, false)?;
                // The palette is coded as differences from the entry before.
                for i in 1..palette.len() {
                    palette[i] = add_pixels(palette[i], palette[i - 1]);
                }
                let width_bits = match size {
                    1..=2 => 3,
                    3..=4 => 2,
                    5..=16 => 1,
                    _ => 0,
                };
                coded_width = coded_width.div_ceil(1 << width_bits);
                Transform::ColorIndexing {
                    palette,
                    width_bits,
                }
            }
        });
    }

    let mut pixels = decode_image(bits, coded_width, height, true)?;
    let mut width = coded_width;
    for transform in transforms.iter().rev() {
        match transform {
            Transform::Predictor { bits, modes } => {
                predict(&mut pixels, width, height, *bits, modes);
            }
            Transform::Color { bits, elements } => {
                uncorrelate(&mut pixels, width, *bits, elements);
            }
            Transform::SubtractGreen => {
                for pixel in &mut pixels {
                    let green = (*pixel >> 8) & 0xff;
                    let red = (((*pixel >> 16) & 0xff) + green) & 0xff;
                    let blue = ((*pixel & 0xff) + green) & 0xff;
                    *pixel = (*pixel & 0xff00_ff00) | (red << 16) | blue;
                }
            }
            Transform::ColorIndexing {
                palette,
                width_bits,
            } => {
                // Colour indexing is the one transform that narrows the
                // image, and it can come only once, so it widens it back to
                // the header's width.
                pixels = unindex(&pixels, width, full_width, height, palette, *width_bits);
                width = full_width;
            }
        }
    }
    if width != full_width {
        return Err(malformed("a lossless image narrower than its header"));
    }
    Ok(pixels)
}

/// Each channel of `a` plus `b`'s, modulo 256.
const fn add_pixels(a: u32, b: u32) -> u32 {
    let alpha_green = (a & 0xff00_ff00).wrapping_add(b & 0xff00_ff00) & 0xff00_ff00;
    let red_blue = (a & 0x00ff_00ff).wrapping_add(b & 0x00ff_00ff) & 0x00ff_00ff;
    alpha_green | red_blue
}

/// One channel of a pixel, shifted down.
const fn channel(pixel: u32, shift: u32) -> i32 {
    ((pixel >> shift) & 0xff) as i32
}

/// The specification's `Average2`, channel by channel.
const fn average2(a: u32, b: u32) -> u32 {
    let mut out = 0;
    let mut shift = 0;
    while shift < 32 {
        out |= (((channel(a, shift) + channel(b, shift)) / 2) as u32) << shift;
        shift += 8;
    }
    out
}

/// The specification's `Select`.
fn select(left: u32, top: u32, top_left: u32) -> u32 {
    let (mut to_left, mut to_top) = (0, 0);
    for shift in [24, 16, 8, 0] {
        let estimate = channel(left, shift) + channel(top, shift) - channel(top_left, shift);
        to_left += (estimate - channel(left, shift)).abs();
        to_top += (estimate - channel(top, shift)).abs();
    }
    if to_left < to_top { left } else { top }
}

/// The specification's `ClampAddSubtractFull`, channel by channel.
fn clamp_add_subtract_full(a: u32, b: u32, c: u32) -> u32 {
    [24, 16, 8, 0].iter().fold(0, |out, &shift| {
        let value = (channel(a, shift) + channel(b, shift) - channel(c, shift)).clamp(0, 255);
        out | ((value as u32) << shift)
    })
}

/// The specification's `ClampAddSubtractHalf`, channel by channel; its
/// division truncates toward zero, as C's does.
fn clamp_add_subtract_half(a: u32, b: u32) -> u32 {
    [24, 16, 8, 0].iter().fold(0, |out, &shift| {
        let (a, b) = (channel(a, shift), channel(b, shift));
        let value = (a + (a - b) / 2).clamp(0, 255);
        out | ((value as u32) << shift)
    })
}

/// The predicted value by `mode` from the neighbours.
fn predicted(mode: u32, left: u32, top: u32, top_right: u32, top_left: u32) -> u32 {
    match mode {
        1 => left,
        2 => top,
        3 => top_right,
        4 => top_left,
        5 => average2(average2(left, top_right), top),
        6 => average2(left, top_left),
        7 => average2(left, top),
        8 => average2(top_left, top),
        9 => average2(top, top_right),
        10 => average2(average2(left, top_left), average2(top, top_right)),
        11 => select(left, top, top_left),
        12 => clamp_add_subtract_full(left, top, top_left),
        13 => clamp_add_subtract_half(average2(left, top), top_left),
        // Mode 0, and the two values a 4-bit mode field leaves unnamed, which
        // libwebp also predicts as black.
        _ => 0xff00_0000,
    }
}

/// Undoes the predictor transform in place.
fn predict(pixels: &mut [u32], width: usize, height: usize, block_bits: u32, modes: &[u32]) {
    let blocks_wide = width.div_ceil(1 << block_bits);
    for y in 0..height {
        for x in 0..width {
            let at = y * width + x;
            let prediction = if y == 0 && x == 0 {
                0xff00_0000
            } else if y == 0 {
                pixels[at - 1]
            } else if x == 0 {
                pixels[at - width]
            } else {
                let mode = (modes[(y >> block_bits) * blocks_wide + (x >> block_bits)] >> 8) & 0xff;
                // The rightmost column's top-right is the row's own first
                // pixel: the next one in memory after its top.
                let top_right = pixels[at - width + 1];
                predicted(
                    mode,
                    pixels[at - 1],
                    pixels[at - width],
                    top_right,
                    pixels[at - width - 1],
                )
            };
            pixels[at] = add_pixels(pixels[at], prediction);
        }
    }
}

/// The specification's `ColorTransformDelta`: two signed bytes multiplied,
/// as a 3.5 fixed-point product.
const fn delta(t: u32, c: u32) -> u32 {
    (((t as u8 as i8 as i32) * (c as u8 as i8 as i32)) >> 5) as u32
}

/// Undoes the colour transform in place.
fn uncorrelate(pixels: &mut [u32], width: usize, block_bits: u32, elements: &[u32]) {
    let blocks_wide = width.div_ceil(1 << block_bits);
    for (at, pixel) in pixels.iter_mut().enumerate() {
        let (x, y) = (at % width, at / width);
        let element = elements[(y >> block_bits) * blocks_wide + (x >> block_bits)];
        let (red_to_blue, green_to_blue, green_to_red) = (element >> 16, element >> 8, element);
        let green = *pixel >> 8;
        let red = ((*pixel >> 16).wrapping_add(delta(green_to_red, green))) & 0xff;
        let blue = (*pixel)
            .wrapping_add(delta(green_to_blue, green))
            .wrapping_add(delta(red_to_blue, red))
            & 0xff;
        *pixel = (*pixel & 0xff00_ff00) | (red << 16) | blue;
    }
}

/// Undoes the colour-indexing transform: each green value, or each of the
/// several bundled into one, replaced by its palette colour; an index past
/// the palette is transparent black.
fn unindex(
    pixels: &[u32],
    coded_width: usize,
    width: usize,
    height: usize,
    palette: &[u32],
    width_bits: u32,
) -> Vec<u32> {
    let per_pixel = 8 >> width_bits;
    let mask = (1u32 << per_pixel) - 1;
    let mut out = Vec::with_capacity(width * height);
    for y in 0..height {
        for x in 0..width {
            let packed = (pixels[y * coded_width + (x >> width_bits)] >> 8) & 0xff;
            let shift = (x & ((1 << width_bits) - 1)) as u32 * per_pixel;
            let index = (packed >> shift) & mask;
            out.push(palette.get(index as usize).copied().unwrap_or(0));
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    /// `Select`, worked by hand from the specification: the estimate is
    /// `L + T - TL`, and the nearer of `L` and `T` to it by Manhattan distance
    /// wins, `T` on a tie.
    #[test]
    fn select_takes_the_neighbour_nearer_the_estimate_and_top_on_a_tie() {
        let pixel = |a: u32, r: u32, g: u32, b: u32| (a << 24) | (r << 16) | (g << 8) | b;
        let (left, top) = (pixel(255, 10, 10, 10), pixel(255, 20, 20, 20));
        // TL equal to L: the estimate is T, which is nearer.
        assert_eq!(select(left, top, left), top);
        // TL equal to T: the estimate is L.
        assert_eq!(select(left, top, top), left);
        // TL half-way: the estimate is half-way too, so a tie, and T wins.
        assert_eq!(select(left, top, pixel(255, 15, 15, 15)), top);
    }

    /// `ClampAddSubtractHalf(a, b)` is `a + (a - b) / 2` with C's division,
    /// which truncates toward zero: 10 and 13 make 10 + (-3 / 2) = 9, where a
    /// floor would make 8.
    #[test]
    fn the_half_predictor_truncates_toward_zero() {
        let grey = |v: u32| (255 << 24) | (v << 16) | (v << 8) | v;
        assert_eq!(clamp_add_subtract_half(grey(10), grey(13)) & 0xff, 9);
        assert_eq!(clamp_add_subtract_half(grey(13), grey(10)) & 0xff, 14);
        assert_eq!(clamp_add_subtract_half(grey(250), grey(0)) & 0xff, 255);
    }

    /// The rightmost column's top-right neighbour is the first pixel of its
    /// own row, by the specification's border rule.
    #[test]
    fn the_rightmost_columns_top_right_is_its_rows_first_pixel() {
        // Three wide, two high, every block predicted from its top-right
        // (mode 3). The first row holds 1, 2 and 3 as residuals over its
        // left neighbours (the first over black, whose full alpha it takes);
        // the second row is all zero residuals.
        let mode = 3 << 8;
        let mut pixels = vec![1, 1, 1, 0, 0, 0];
        predict(&mut pixels, 3, 2, 2, &[mode]);
        assert_eq!(&pixels[..3], &[0xff00_0001, 0xff00_0002, 0xff00_0003]);
        // (0, 1) takes its top; (1, 1) its top-right, 3; (2, 1) the row's own
        // first pixel, 1.
        assert_eq!(&pixels[3..], &[0xff00_0001, 0xff00_0003, 0xff00_0001]);
    }

    /// A lossless bitstream under construction, least significant bit first.
    #[derive(Default)]
    struct Writer {
        bytes: Vec<u8>,
        acc: u64,
        count: u32,
    }

    impl Writer {
        fn put(&mut self, value: u32, n: u32) {
            self.acc |= u64::from(value) << self.count;
            self.count += n;
            while self.count >= 8 {
                self.bytes.push(self.acc as u8);
                self.acc >>= 8;
                self.count -= 8;
            }
        }

        /// A simple prefix code of the one symbol `symbol`, which takes no
        /// bits to decode.
        fn simple(&mut self, symbol: u8) {
            self.put(1, 1);
            self.put(0, 1);
            if symbol < 2 {
                self.put(0, 1);
                self.put(u32::from(symbol), 1);
            } else {
                self.put(1, 1);
                self.put(u32::from(symbol), 8);
            }
        }

        /// A group of five one-symbol codes: a literal pixel of `green`,
        /// `red`, `blue` and `alpha`, and a distance code nothing reads.
        fn group(&mut self, [green, red, blue, alpha]: [u8; 4]) {
            for symbol in [green, red, blue, alpha, 0] {
                self.simple(symbol);
            }
        }

        fn finish(mut self) -> Vec<u8> {
            if self.count > 0 {
                self.bytes.push(self.acc as u8);
            }
            self.bytes
        }
    }

    /// The groups after a 1x1 main image's entropy image: every meta-code up
    /// to 65 535, which is the only one the entropy image names. Each skipped
    /// group is one-symbol codes of zero; the last is the pixel's colour.
    fn last_of_every_group(w: &mut Writer) {
        for _ in 0..u16::MAX {
            w.group([0; 4]);
        }
        w.group([0x80, 0x40, 0x20, 0xff]);
    }

    /// A 1x1 lossless image with an 11-bit colour cache (the largest green
    /// alphabet) whose entropy image names meta-code 65 535, so it carries
    /// 65 536 groups and uses one.
    fn sparse_groups_file() -> Vec<u8> {
        let mut w = Writer::default();
        w.put(u32::from(VP8L_SIGNATURE), 8);
        w.put(0, 14); // width - 1
        w.put(0, 14); // height - 1
        w.put(1, 1); // alpha is used
        w.put(0, 3); // version
        w.put(0, 1); // no transform
        w.put(1, 1); // a colour cache...
        w.put(MAX_COLOR_CACHE_BITS, 4); // ...of the largest size
        w.put(1, 1); // meta prefix codes...
        w.put(0, 3); // ...in blocks of four pixels
        // The entropy image: no cache, one group, one pixel whose red and
        // green name meta-code 0xffff.
        w.put(0, 1);
        w.group([0xff, 0xff, 0, 0]);
        last_of_every_group(&mut w);
        w.finish()
    }

    /// **Only the groups the entropy image names are built**: of 65 536
    /// groups, the one meta-code 65 535 names, and every other is read past
    /// to the end of the stream.
    #[test]
    fn only_the_groups_in_use_are_built() {
        let mut entropy = vec![0xffff << 8, 0x00ff_ff00, 7 << 8];
        let used = dense_groups(&mut entropy);
        assert_eq!(used.len(), 0x1_0000);
        let named: Vec<usize> = (0..used.len()).filter(|&i| used[i]).collect();
        assert_eq!(named, [7, 0xffff]);
        assert_eq!(entropy, [1, 1, 0]);

        let mut w = Writer::default();
        last_of_every_group(&mut w);
        let data = w.finish();
        let mut used = vec![false; 0x1_0000];
        used[0xffff] = true;
        let mut bits = Bits::new(&data);
        let groups =
            read_groups(&mut bits, &used, 1 << MAX_COLOR_CACHE_BITS).expect("valid groups");
        assert_eq!(groups.len(), 1);
        assert_eq!(groups[0].green.symbols, [0x80]);
        assert_eq!(groups[0].alpha.symbols, [0xff]);
        assert_eq!(bits.at, data.len());
    }

    /// **A skipped group is checked as a built one is**: a code with no symbols
    /// in a group nothing uses is refused, as libwebp refuses it.
    #[test]
    fn an_unused_group_with_an_empty_code_is_refused() {
        // A normal-form code whose code-length code is the one symbol 17,
        // at length one, so every length read is a run of 3 to 10 zeros:
        // twenty-five runs of ten and one of six zero the whole alphabet,
        // leaving a code with no symbols.
        let mut w = Writer::default();
        w.put(0, 1); // the normal form
        w.put(0, 4); // four code-length lengths stored: 17, 18, 0, 1
        w.put(1, 3); // 17 has length one
        w.put(0, 3);
        w.put(0, 3);
        w.put(0, 3);
        w.put(0, 1); // no max_symbol
        for _ in 0..25 {
            w.put(7, 3); // ten zeros
        }
        w.put(3, 3); // six zeros
        let data = w.finish();
        let built = read_prefix(&mut Bits::new(&data), LITERALS).map(drop);
        let skipped = skip_prefix(&mut Bits::new(&data), LITERALS);
        assert_eq!(built, Err(malformed("a prefix code with no symbols")));
        assert_eq!(skipped, built);
    }

    /// **An entropy image naming one high meta-code decodes**: RFC 9649 lets
    /// a meta-code skip the groups below it, which are read but unused.
    #[test]
    fn an_image_using_one_of_many_groups_decodes() {
        let image = decode_vp8l(&sparse_groups_file(), &Limits::DEFAULT).expect("a valid image");
        assert_eq!((image.width(), image.height()), (1, 1));
        assert_eq!(image.pixels(), [0x40, 0x80, 0x20, 0xff]);
    }

    #[test]
    fn an_lz77_value_is_its_prefix_and_extra_bits() {
        // Prefix codes below four are the value less one; prefix 4 is 5 or 6
        // by one extra bit; prefix 6 is 9 to 12 by two.
        let mut bits = Bits::new(&[0b0000_0111]);
        assert_eq!(lz77_value(3, &mut bits), Ok(4));
        assert_eq!(lz77_value(4, &mut bits), Ok(6));
        assert_eq!(lz77_value(6, &mut bits), Ok(12));
    }
}
