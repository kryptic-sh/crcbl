//! JPEG, written here: baseline and progressive Huffman-coded JPEG (ITU-T
//! T.81) decoded to RGBA8.
//!
//! # What it reads
//!
//! Eight-bit samples, Huffman coding, sequential (`SOF0`, `SOF1`) and
//! progressive (`SOF2`) frames with spectral selection and successive
//! approximation, restart intervals, any sampling factors that divide the
//! largest evenly, and one component (grey) or three (YCbCr, or RGB when an
//! Adobe marker or the component ids say so). Refused with
//! [`ImageError::Unsupported`]: arithmetic coding, lossless and hierarchical
//! frames, 12-bit samples, a height given only by a `DNL` marker, and four
//! components — CMYK and YCCK, whose conversion to RGB decoders do not agree
//! on, and which album art and icons do not use. EXIF orientation is not
//! applied.
//!
//! # The same pixels as libjpeg-turbo
//!
//! Three stages decide the pixels beyond what the standard fixes, and each is
//! libjpeg-turbo's (the decoder behind Pillow, browsers and most of the
//! desktop), transcribed from the IJG algorithms it inherits, so a file
//! decodes here to exactly what it decodes to there:
//!
//! * The inverse DCT is the IJG "islow" integer transform (Loeffler, Ligtenberg
//!   and Moschytz's factorisation, 13-bit constants, two extra bits of
//!   precision between the passes).
//! * Chroma is upsampled with IJG "fancy" upsampling for 2:1 horizontally and
//!   2:1 both ways: a triangle filter weighting the nearer sample three to one,
//!   with the image's first row repeated above it and its last below. Like
//!   libjpeg-turbo, a component at most two samples wide is replicated
//!   instead. Every other ratio is replicated; libjpeg-turbo filters 1:2
//!   vertically too, so a 4:4:0 file may differ there by a little.
//! * YCbCr becomes RGB through the IJG fixed-point tables, 16 fractional bits.
//!
//! The tests hold this to exact equality with libjpeg-turbo's output on
//! committed files.
//!
//! # Hostile input
//!
//! The frame's size passes the caller's [`Limits`] before anything is sized
//! from it, and the coefficient store is bounded by
//! [`COEFFICIENT_ALLOWANCE`] times the limit's bytes. Entropy-coded data that
//! runs into a marker early is read as zeros, as libjpeg does with a damaged
//! scan; a file that ends before its end-of-image marker is
//! [`ImageError::Malformed`].

use crate::{ImageError, Limits, Rgba8Image};

/// How many times the caller's [`Limits::max_bytes`] the decoded coefficients
/// may take. Two bytes per coefficient and three full-resolution components
/// is six bytes a pixel against RGBA's four; a little over that covers the
/// padding to whole blocks.
pub const COEFFICIENT_ALLOWANCE: usize = 2;

/// Where each zigzag position's coefficient sits in an 8×8 block, row by row.
const ZIGZAG: [usize; 64] = [
    0, 1, 8, 16, 9, 2, 3, 10, 17, 24, 32, 25, 18, 11, 4, 5, 12, 19, 26, 33, 40, 48, 41, 34, 27, 20,
    13, 6, 7, 14, 21, 28, 35, 42, 49, 56, 57, 50, 43, 36, 29, 22, 15, 23, 30, 37, 44, 51, 58, 59,
    52, 45, 38, 31, 39, 46, 53, 60, 61, 54, 47, 55, 62, 63,
];

/// The most components a frame may have: T.81's limit for a scan, and the
/// most any colour model here needs.
const MAX_COMPONENTS: usize = 4;

/// The most blocks one MCU of an interleaved scan may hold, by T.81.
const MAX_BLOCKS_PER_MCU: usize = 10;

fn malformed(why: impl Into<String>) -> ImageError {
    ImageError::Malformed(why.into())
}

fn unsupported(why: impl Into<String>) -> ImageError {
    ImageError::Unsupported(why.into())
}

/// A Huffman table, in T.81 Annex F's decoding form.
#[derive(Clone, Debug)]
struct Huffman {
    /// For each code length, the largest code of that length, or -1.
    maxcode: [i32; 17],
    /// For each code length, the smallest code of that length.
    mincode: [i32; 17],
    /// For each code length, where its values start in `values`.
    valptr: [usize; 17],
    values: Vec<u8>,
}

impl Huffman {
    /// T.81 F.2.2.3's tables from the counts of codes of each length and the
    /// values in code order.
    fn new(counts: &[u8; 16], values: Vec<u8>) -> Result<Self, ImageError> {
        let mut maxcode = [-1i32; 17];
        let mut mincode = [0i32; 17];
        let mut valptr = [0usize; 17];
        let mut code = 0i32;
        let mut k = 0usize;
        for length in 1..=16 {
            let count = usize::from(counts[length - 1]);
            if count > 0 {
                valptr[length] = k;
                mincode[length] = code;
                code += count as i32;
                k += count;
                maxcode[length] = code - 1;
                if code > 1 << length {
                    return Err(malformed("a Huffman table has more codes than fit"));
                }
            }
            code <<= 1;
        }
        if k != values.len() {
            return Err(malformed("a Huffman table's counts and values disagree"));
        }
        Ok(Self {
            maxcode,
            mincode,
            valptr,
            values,
        })
    }
}

/// Entropy-coded bits, with T.81's byte stuffing undone. A marker ends the
/// bits: past it, every bit is zero until the reader is reset at a restart.
#[derive(Debug)]
struct Bits<'a> {
    data: &'a [u8],
    pos: usize,
    acc: u32,
    count: u32,
    at_marker: bool,
}

impl<'a> Bits<'a> {
    fn new(data: &'a [u8], pos: usize) -> Self {
        Self {
            data,
            pos,
            acc: 0,
            count: 0,
            at_marker: false,
        }
    }

    fn bit(&mut self) -> u32 {
        if self.count == 0 {
            let byte = if self.at_marker {
                0
            } else {
                match (self.data.get(self.pos), self.data.get(self.pos + 1)) {
                    (Some(0xff), Some(0x00)) => {
                        self.pos += 2;
                        0xff
                    }
                    (Some(0xff), _) | (None, _) => {
                        self.at_marker = true;
                        0
                    }
                    (Some(&byte), _) => {
                        self.pos += 1;
                        byte
                    }
                }
            };
            self.acc = u32::from(byte);
            self.count = 8;
        }
        self.count -= 1;
        (self.acc >> self.count) & 1
    }

    fn bits(&mut self, n: u32) -> u32 {
        (0..n).fold(0, |value, _| (value << 1) | self.bit())
    }

    /// T.81 F.2.2.3's `DECODE`.
    fn decode(&mut self, table: &Huffman) -> Result<u8, ImageError> {
        let mut code = self.bit() as i32;
        for length in 1..=16 {
            if code <= table.maxcode[length] {
                let index = table.valptr[length] + (code - table.mincode[length]) as usize;
                return table
                    .values
                    .get(index)
                    .copied()
                    .ok_or_else(|| malformed("a Huffman code past its table"));
            }
            code = (code << 1) | self.bit() as i32;
        }
        Err(malformed("a Huffman code longer than sixteen bits"))
    }

    /// `n` bits as T.81's `RECEIVE` and `EXTEND`: a signed value.
    fn signed(&mut self, n: u32) -> i32 {
        if n == 0 {
            return 0;
        }
        let value = self.bits(n) as i32;
        if value < 1 << (n - 1) {
            value - (1 << n) + 1
        } else {
            value
        }
    }

    /// Skips to the restart marker the reader stopped at, past any fill
    /// bytes, and starts reading after it.
    fn restart(&mut self) -> Result<(), ImageError> {
        self.acc = 0;
        self.count = 0;
        self.at_marker = false;
        while self.data.get(self.pos) == Some(&0xff) && self.data.get(self.pos + 1) == Some(&0xff) {
            self.pos += 1;
        }
        match (self.data.get(self.pos), self.data.get(self.pos + 1)) {
            (Some(0xff), Some(0xd0..=0xd7)) => {
                self.pos += 2;
                Ok(())
            }
            _ => Err(malformed("a restart interval ends without its marker")),
        }
    }

    /// The position of the marker after the scan's data.
    fn end(&self) -> usize {
        let mut pos = self.pos;
        while pos < self.data.len() {
            if self.data[pos] == 0xff && self.data.get(pos + 1).is_some_and(|&b| b != 0) {
                return pos;
            }
            pos += 1;
        }
        pos
    }
}

/// One component of the frame, with its coefficients.
#[derive(Debug)]
struct Component {
    id: u8,
    h: usize,
    v: usize,
    quant_index: usize,
    /// The quantisation table, in natural order, latched at the component's
    /// first scan as libjpeg does.
    quant: Option<[u16; 64]>,
    /// Blocks across and down, padded to whole MCUs.
    blocks_wide: usize,
    blocks_high: usize,
    /// Its samples across and down, before upsampling.
    width: usize,
    height: usize,
    coefficients: Vec<i16>,
    dc_prediction: i32,
}

/// What the markers before the first scan said.
#[derive(Debug)]
struct Frame {
    width: usize,
    height: usize,
    progressive: bool,
    components: Vec<Component>,
    h_max: usize,
    v_max: usize,
    mcus_wide: usize,
    mcus_high: usize,
}

/// Every table and setting a scan reads, as the markers so far set them.
#[derive(Debug, Default)]
struct Tables {
    quant: [Option<[u16; 64]>; 4],
    dc: [Option<Huffman>; 4],
    ac: [Option<Huffman>; 4],
    restart_interval: usize,
    adobe_transform: Option<u8>,
    jfif: bool,
}

/// Decodes a JPEG to straight RGBA8, every pixel opaque.
///
/// # Errors
///
/// [`ImageError::Empty`] or [`ImageError::TooLarge`] if the frame's size
/// breaks `limits`, [`ImageError::Unsupported`] for a kind of JPEG listed in
/// the [module docs](self), and [`ImageError::Malformed`] for anything that is
/// not a well-formed JPEG.
pub fn decode(bytes: &[u8], limits: &Limits) -> Result<Rgba8Image, ImageError> {
    if bytes.get(..2) != Some(&[0xff, 0xd8]) {
        return Err(malformed("no start-of-image marker"));
    }
    let mut pos = 2;
    let mut tables = Tables::default();
    let mut frame: Option<Frame> = None;
    let mut eobrun = 0u32;
    loop {
        // Fill bytes may come before a marker.
        while bytes.get(pos) == Some(&0xff) && bytes.get(pos + 1) == Some(&0xff) {
            pos += 1;
        }
        let marker = match (bytes.get(pos), bytes.get(pos + 1)) {
            (Some(0xff), Some(&marker)) => marker,
            _ => return Err(malformed("the file ends before its end-of-image marker")),
        };
        pos += 2;
        if marker == 0xd9 {
            break;
        }
        if (0xd0..=0xd7).contains(&marker) || marker == 0x01 {
            continue;
        }
        let length = match bytes.get(pos..pos + 2) {
            Some(&[high, low]) => usize::from(u16::from_be_bytes([high, low])),
            _ => return Err(malformed("a marker segment is cut short")),
        };
        let segment = bytes
            .get(pos + 2..pos + length.max(2))
            .filter(|_| length >= 2)
            .ok_or_else(|| malformed("a marker segment is cut short"))?;
        pos += length;
        match marker {
            0xdb => read_quant(segment, &mut tables)?,
            0xc4 => read_huffman(segment, &mut tables)?,
            0xdd => {
                let interval = segment
                    .get(..2)
                    .ok_or_else(|| malformed("a restart interval marker is cut short"))?;
                tables.restart_interval =
                    usize::from(u16::from_be_bytes([interval[0], interval[1]]));
            }
            0xe0 if segment.starts_with(b"JFIF\0") => tables.jfif = true,
            0xee if segment.starts_with(b"Adobe") => {
                tables.adobe_transform = segment.get(11).copied()
            }
            0xc0..=0xc2 => {
                if frame.is_some() {
                    return Err(malformed("a second frame"));
                }
                frame = Some(read_frame(segment, marker == 0xc2, limits)?);
            }
            0xc3 | 0xc5..=0xc7 | 0xc9..=0xcb | 0xcd..=0xcf => {
                return Err(unsupported(format!(
                    "a JPEG frame of type {marker:#04x} (lossless, hierarchical or arithmetic)"
                )));
            }
            0xdc => return Err(unsupported("a JPEG whose height comes in a DNL marker")),
            0xda => {
                let frame = frame
                    .as_mut()
                    .ok_or_else(|| malformed("a scan before its frame"))?;
                pos = read_scan(bytes, pos, segment, frame, &tables, &mut eobrun)?;
            }
            _ => {}
        }
    }
    let frame = frame.ok_or_else(|| malformed("no frame"))?;
    finish(&frame, &tables, limits)
}

/// A `DQT` segment: one or more tables, 8 or 16 bits a value, in zigzag
/// order.
fn read_quant(mut segment: &[u8], tables: &mut Tables) -> Result<(), ImageError> {
    while let Some((&header, rest)) = segment.split_first() {
        let index = usize::from(header & 0x0f);
        let wide = header >> 4 == 1;
        let size = if wide { 128 } else { 64 };
        let values = rest
            .get(..size)
            .filter(|_| index < 4 && header >> 4 <= 1)
            .ok_or_else(|| malformed("a quantisation table is cut short or misnumbered"))?;
        let mut table = [0u16; 64];
        for (k, &natural) in ZIGZAG.iter().enumerate() {
            table[natural] = if wide {
                u16::from_be_bytes([values[2 * k], values[2 * k + 1]])
            } else {
                u16::from(values[k])
            };
        }
        tables.quant[index] = Some(table);
        segment = &rest[size..];
    }
    Ok(())
}

/// A `DHT` segment: one or more tables.
fn read_huffman(mut segment: &[u8], tables: &mut Tables) -> Result<(), ImageError> {
    while let Some((&header, rest)) = segment.split_first() {
        let index = usize::from(header & 0x0f);
        let class = header >> 4;
        let counts: [u8; 16] = rest
            .get(..16)
            .and_then(|counts| counts.try_into().ok())
            .filter(|_| index < 4 && class <= 1)
            .ok_or_else(|| malformed("a Huffman table is cut short or misnumbered"))?;
        let total: usize = counts.iter().map(|&count| usize::from(count)).sum();
        let values = rest
            .get(16..16 + total)
            .ok_or_else(|| malformed("a Huffman table's values are cut short"))?;
        let table = Huffman::new(&counts, values.to_vec())?;
        if class == 0 {
            tables.dc[index] = Some(table);
        } else {
            tables.ac[index] = Some(table);
        }
        segment = &rest[16 + total..];
    }
    Ok(())
}

/// A `SOF0`, `SOF1` or `SOF2` segment, checked against `limits` before
/// anything is sized from it.
fn read_frame(segment: &[u8], progressive: bool, limits: &Limits) -> Result<Frame, ImageError> {
    let &[precision, h0, h1, w0, w1, count, ref rest @ ..] = segment else {
        return Err(malformed("a frame header is cut short"));
    };
    if precision != 8 {
        return Err(unsupported(format!("{precision}-bit JPEG samples")));
    }
    let height = u16::from_be_bytes([h0, h1]);
    let width = u16::from_be_bytes([w0, w1]);
    if height == 0 {
        return Err(unsupported("a JPEG whose height comes in a DNL marker"));
    }
    limits.rgba_bytes(u32::from(width), u32::from(height))?;
    let count = usize::from(count);
    match count {
        1 | 3 => {}
        4 => return Err(unsupported("a four-component (CMYK or YCCK) JPEG")),
        _ => return Err(unsupported(format!("a JPEG of {count} components"))),
    }
    let specs = rest
        .get(..count * 3)
        .ok_or_else(|| malformed("a frame header's components are cut short"))?;
    let mut components = Vec::with_capacity(count);
    for spec in specs.as_chunks::<3>().0 {
        let (h, v) = (usize::from(spec[1] >> 4), usize::from(spec[1] & 0x0f));
        if !(1..=4).contains(&h) || !(1..=4).contains(&v) || spec[2] > 3 {
            return Err(malformed("a component's sampling or table is out of range"));
        }
        components.push((spec[0], h, v, usize::from(spec[2])));
    }
    let h_max = components.iter().map(|c| c.1).max().unwrap_or(1);
    let v_max = components.iter().map(|c| c.2).max().unwrap_or(1);
    if components
        .iter()
        .any(|&(_, h, v, _)| h_max % h != 0 || v_max % v != 0)
    {
        return Err(unsupported(
            "sampling factors that do not divide the largest",
        ));
    }
    let (width, height) = (usize::from(width), usize::from(height));
    let mcus_wide = width.div_ceil(8 * h_max);
    let mcus_high = height.div_ceil(8 * v_max);
    let blocks: usize = components
        .iter()
        .map(|&(_, h, v, _)| mcus_wide * h * mcus_high * v)
        .sum();
    if blocks * 64 * 2 > limits.max_bytes.saturating_mul(COEFFICIENT_ALLOWANCE) {
        return Err(ImageError::TooLarge {
            width: width as u32,
            height: height as u32,
            limits: *limits,
        });
    }
    let components = components
        .into_iter()
        .map(|(id, h, v, quant_index)| Component {
            id,
            h,
            v,
            quant_index,
            quant: None,
            blocks_wide: mcus_wide * h,
            blocks_high: mcus_high * v,
            width: (width * h).div_ceil(h_max),
            height: (height * v).div_ceil(v_max),
            coefficients: vec![0; mcus_wide * h * mcus_high * v * 64],
            dc_prediction: 0,
        })
        .collect();
    Ok(Frame {
        width,
        height,
        progressive,
        components,
        h_max,
        v_max,
        mcus_wide,
        mcus_high,
    })
}

/// What one scan reads, from its `SOS` header.
#[derive(Debug)]
struct Scan {
    /// For each component in the scan: its index in the frame, and its DC and
    /// AC table numbers.
    components: Vec<(usize, usize, usize)>,
    start: usize,
    end: usize,
    high: u32,
    low: u32,
}

/// An `SOS` segment and the entropy-coded data after it; returns where the
/// next marker is.
fn read_scan(
    bytes: &[u8],
    data_start: usize,
    segment: &[u8],
    frame: &mut Frame,
    tables: &Tables,
    eobrun: &mut u32,
) -> Result<usize, ImageError> {
    let count = usize::from(
        *segment
            .first()
            .ok_or_else(|| malformed("a scan header is empty"))?,
    );
    if count == 0 || count > MAX_COMPONENTS {
        return Err(malformed("a scan of no or too many components"));
    }
    let specs = segment
        .get(1..1 + count * 2)
        .ok_or_else(|| malformed("a scan header is cut short"))?;
    let mut components = Vec::with_capacity(count);
    for spec in specs.as_chunks::<2>().0 {
        let index = frame
            .components
            .iter()
            .position(|c| c.id == spec[0])
            .ok_or_else(|| malformed("a scan names a component the frame has not"))?;
        let (dc, ac) = (usize::from(spec[1] >> 4), usize::from(spec[1] & 0x0f));
        if dc > 3 || ac > 3 {
            return Err(malformed("a scan's Huffman table is out of range"));
        }
        components.push((index, dc, ac));
    }
    let &[start, end, approximation] = segment
        .get(1 + count * 2..4 + count * 2)
        .ok_or_else(|| malformed("a scan header is cut short"))?
    else {
        return Err(malformed("a scan header is cut short"));
    };
    let scan = Scan {
        components,
        start: usize::from(start),
        end: usize::from(end),
        high: u32::from(approximation >> 4),
        low: u32::from(approximation & 0x0f),
    };
    if frame.progressive {
        let dc = scan.start == 0;
        if (dc && scan.end != 0)
            || (!dc && (scan.end < scan.start || scan.end > 63))
            || scan.low > 13
            || (!dc && count != 1)
        {
            return Err(malformed("a progressive scan's band is out of range"));
        }
    } else if scan.start != 0 || scan.end != 63 || scan.high != 0 || scan.low != 0 {
        return Err(malformed("a sequential scan that is not the whole band"));
    }
    let blocks_per_mcu: usize = scan
        .components
        .iter()
        .map(|&(index, ..)| frame.components[index].h * frame.components[index].v)
        .sum();
    if count > 1 && blocks_per_mcu > MAX_BLOCKS_PER_MCU {
        return Err(malformed("an MCU of too many blocks"));
    }
    for &(index, dc, ac) in &scan.components {
        let component = &mut frame.components[index];
        if component.quant.is_none() {
            component.quant = Some(
                tables.quant[component.quant_index]
                    .ok_or_else(|| malformed("a component whose quantisation table is missing"))?,
            );
        }
        let needs_dc = scan.start == 0 && scan.high == 0;
        if (needs_dc && tables.dc[dc].is_none()) || (scan.end > 0 && tables.ac[ac].is_none()) {
            return Err(malformed("a scan whose Huffman table is missing"));
        }
        component.dc_prediction = 0;
    }
    *eobrun = 0;

    let mut bits = Bits::new(bytes, data_start);
    // A scan of one component goes block by block over its own samples, not
    // over the padded MCU grid: each block is an MCU.
    let (first, ..) = scan.components[0];
    let own_blocks_wide = frame.components[first].width.div_ceil(8);
    let mcu_count = if count == 1 {
        own_blocks_wide * frame.components[first].height.div_ceil(8)
    } else {
        frame.mcus_wide * frame.mcus_high
    };
    let contexts: Vec<BlockContext<'_>> = scan
        .components
        .iter()
        .map(|&(index, dc, ac)| BlockContext {
            scan: &scan,
            index,
            dc: tables.dc[dc].as_ref(),
            ac: tables.ac[ac].as_ref(),
        })
        .collect();
    for mcu in 0..mcu_count {
        if tables.restart_interval > 0 && mcu > 0 && mcu % tables.restart_interval == 0 {
            bits.restart()?;
            for &(index, ..) in &scan.components {
                frame.components[index].dc_prediction = 0;
            }
            *eobrun = 0;
        }
        if count == 1 {
            let (x, y) = (mcu % own_blocks_wide, mcu / own_blocks_wide);
            decode_block(&mut bits, frame, &contexts[0], (x, y), eobrun)?;
        } else {
            let (mcu_x, mcu_y) = (mcu % frame.mcus_wide, mcu / frame.mcus_wide);
            for context in &contexts {
                let component = &frame.components[context.index];
                let (h, v) = (component.h, component.v);
                for by in 0..v {
                    for bx in 0..h {
                        let at = (mcu_x * h + bx, mcu_y * v + by);
                        decode_block(&mut bits, frame, context, at, eobrun)?;
                    }
                }
            }
        }
    }
    Ok(bits.end())
}

/// What decoding one component's blocks in a scan needs: the scan, the
/// component's index in the frame, and its Huffman tables.
#[derive(Debug)]
struct BlockContext<'a> {
    scan: &'a Scan,
    index: usize,
    dc: Option<&'a Huffman>,
    ac: Option<&'a Huffman>,
}

/// One block's share of a scan, the block at `(x, y)` in its component.
fn decode_block(
    bits: &mut Bits<'_>,
    frame: &mut Frame,
    context: &BlockContext<'_>,
    (x, y): (usize, usize),
    eobrun: &mut u32,
) -> Result<(), ImageError> {
    let scan = context.scan;
    let component = &mut frame.components[context.index];
    let offset = (y * component.blocks_wide + x) * 64;
    let block: &mut [i16; 64] = (&mut component.coefficients[offset..offset + 64])
        .try_into()
        .map_err(|_| malformed("a block outside its component"))?;
    if scan.start == 0 {
        if scan.high == 0 {
            let table = context.dc.ok_or_else(|| malformed("no DC table"))?;
            let size = u32::from(bits.decode(table)?);
            if size > 11 {
                return Err(malformed("a DC difference too large"));
            }
            component.dc_prediction = component.dc_prediction.wrapping_add(bits.signed(size));
            block[0] = (component.dc_prediction << scan.low) as i16;
        } else if bits.bit() == 1 {
            block[0] |= 1 << scan.low;
        }
        if frame.progressive {
            return Ok(());
        }
    }
    let table = context.ac.ok_or_else(|| malformed("no AC table"))?;
    let start = scan.start.max(1);
    if scan.high == 0 {
        decode_ac_first(bits, table, block, start, scan.end, scan.low, eobrun)
    } else {
        decode_ac_refine(bits, table, block, start, scan.end, scan.low, eobrun)
    }
}

/// A band's first pass, or a sequential block's AC: T.81 G.1.2.2 (and F.2.2.2
/// with no end-of-band runs).
fn decode_ac_first(
    bits: &mut Bits<'_>,
    table: &Huffman,
    block: &mut [i16; 64],
    start: usize,
    end: usize,
    low: u32,
    eobrun: &mut u32,
) -> Result<(), ImageError> {
    if *eobrun > 0 {
        *eobrun -= 1;
        return Ok(());
    }
    let mut k = start;
    while k <= end {
        let symbol = bits.decode(table)?;
        let (run, size) = (usize::from(symbol >> 4), u32::from(symbol & 0x0f));
        if size == 0 {
            if run < 15 {
                *eobrun = (1 << run) - 1;
                if run > 0 {
                    *eobrun += bits.bits(run as u32);
                }
                break;
            }
            k += 16;
            continue;
        }
        k += run;
        if k > end {
            return Err(malformed("a coefficient past its band"));
        }
        block[ZIGZAG[k]] = (bits.signed(size) << low) as i16;
        k += 1;
    }
    Ok(())
}

/// A band's refinement pass: T.81 G.1.2.3, in the order libjpeg reads it.
fn decode_ac_refine(
    bits: &mut Bits<'_>,
    table: &Huffman,
    block: &mut [i16; 64],
    start: usize,
    end: usize,
    low: u32,
    eobrun: &mut u32,
) -> Result<(), ImageError> {
    let plus = 1i16 << low;
    let minus = -1i16 << low;
    // A coefficient already nonzero takes one correction bit wherever the
    // scan passes it.
    let refine = |bits: &mut Bits<'_>, coefficient: &mut i16| {
        if bits.bit() == 1 && *coefficient & plus == 0 {
            *coefficient = coefficient.wrapping_add(if *coefficient >= 0 { plus } else { minus });
        }
    };
    let mut k = start;
    if *eobrun == 0 {
        while k <= end {
            let symbol = bits.decode(table)?;
            let (mut run, size) = (i32::from(symbol >> 4), symbol & 0x0f);
            let mut value = 0i16;
            if size != 0 {
                if size != 1 {
                    return Err(malformed(
                        "a refinement coefficient that is not a single bit",
                    ));
                }
                value = if bits.bit() == 1 { plus } else { minus };
            } else if run != 15 {
                *eobrun = 1 << run;
                if run > 0 {
                    *eobrun += bits.bits(run as u32);
                }
                break;
            }
            // Pass `run` zero coefficients, refining the nonzero ones between,
            // and stop on the zero the new value goes in.
            while k <= end {
                let coefficient = &mut block[ZIGZAG[k]];
                if *coefficient != 0 {
                    refine(bits, coefficient);
                } else {
                    run -= 1;
                    if run < 0 {
                        break;
                    }
                }
                k += 1;
            }
            if value != 0 && k <= end {
                block[ZIGZAG[k]] = value;
            }
            k += 1;
        }
    }
    if *eobrun > 0 {
        while k <= end {
            let coefficient = &mut block[ZIGZAG[k]];
            if *coefficient != 0 {
                refine(bits, coefficient);
            }
            k += 1;
        }
        *eobrun -= 1;
    }
    Ok(())
}

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
fn idct(coefficients: &[i16; 64], quant: &[u16; 64], out: &mut [u8], stride: usize) {
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
/// (see the [module docs](self)), `width` × `height` samples.
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
fn ycc_to_rgb(y: u8, cb: u8, cr: u8) -> [u8; 3] {
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

/// The decoded frame as RGBA8.
fn finish(frame: &Frame, tables: &Tables, limits: &Limits) -> Result<Rgba8Image, ImageError> {
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
        && !tables.jfif
        && match tables.adobe_transform {
            Some(transform) => transform == 0,
            None => ids == b"RGB",
        };
    let mut pixels = Vec::with_capacity(bytes);
    for i in 0..frame.width * frame.height {
        let [r, g, b] = match planes.as_slice() {
            [grey] => [grey[i]; 3],
            [a, b, c] if rgb => [a[i], b[i], c[i]],
            [y, cb, cr] => ycc_to_rgb(y[i], cb[i], cr[i]),
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

#[cfg(test)]
mod tests {
    use super::*;

    /// Each fixture beside libjpeg-turbo's decode of it (through Pillow 12.3,
    /// `Image.open(…).convert("RGB")`), written by a script that draws a
    /// seeded gradient with noise at an odd size and saves it with Pillow's
    /// encoder under the options its name gives: chroma sampling, progressive
    /// scans with successive approximation, optimised Huffman tables, a
    /// restart marker after every MCU.
    macro_rules! fixture {
        ($name:literal) => {
            (
                $name,
                include_bytes!(concat!("../fixtures/jpeg/", $name, ".jpg")) as &[u8],
                include_bytes!(concat!("../fixtures/jpeg/", $name, ".rgb")) as &[u8],
            )
        };
    }

    const FIXTURES: [(&str, &[u8], &[u8]); 13] = [
        fixture!("baseline-444"),
        fixture!("baseline-422"),
        fixture!("baseline-420"),
        fixture!("baseline-420-mcu"),
        fixture!("grey"),
        fixture!("optimised-420"),
        fixture!("progressive-420"),
        fixture!("progressive-444"),
        fixture!("progressive-grey"),
        fixture!("restart-420"),
        fixture!("narrow-420"),
        fixture!("progressive-smooth"),
        fixture!("rgb"),
    ];

    /// The first pixel that differs, as `(x, y, ours, theirs)`.
    fn first_difference(image: &Rgba8Image, rgb: &[u8]) -> Option<(u32, u32, [u8; 3], [u8; 3])> {
        let ours = image.pixels().as_chunks::<4>().0.iter();
        let theirs = rgb.as_chunks::<3>().0.iter();
        ours.zip(theirs)
            .enumerate()
            .find_map(|(i, (ours, theirs))| {
                (ours[..3] != theirs[..] || ours[3] != u8::MAX).then(|| {
                    let i = i as u32;
                    (
                        i % image.width(),
                        i / image.width(),
                        [ours[0], ours[1], ours[2]],
                        *theirs,
                    )
                })
            })
    }

    #[test]
    fn every_fixture_decodes_to_exactly_libjpeg_turbos_pixels() {
        for (name, jpeg, rgb) in FIXTURES {
            let image =
                decode(jpeg, &Limits::DEFAULT).unwrap_or_else(|error| panic!("{name}: {error}"));
            assert_eq!(
                image.pixels().len() / 4,
                rgb.len() / 3,
                "{name}: {}x{}",
                image.width(),
                image.height()
            );
            assert_eq!(first_difference(&image, rgb), None, "{name}");
        }
    }

    #[test]
    fn the_islow_transform_of_a_lone_dc_is_flat() {
        // A DC of 8 (dequantised 8 · 1) is a block of 8 / 8 = 1 above the
        // level shift.
        let mut coefficients = [0i16; 64];
        coefficients[0] = 8;
        let mut out = [0u8; 64];
        idct(&coefficients, &[1; 64], &mut out, 8);
        assert_eq!(out, [129; 64]);
    }

    /// Each fixed-point constant is its coefficient times 2^16, rounded, as
    /// `jdcolor.c`'s `FIX` makes it: computed here from the decimals.
    #[test]
    fn the_colour_constants_are_the_ijg_fixed_point_coefficients() {
        let fix = |coefficient: f64| (coefficient * 65536.0 + 0.5) as i64;
        let (y, cb, cr) = (100u8, 200u8, 50u8);
        let (cbx, crx) = (i64::from(cb) - 128, i64::from(cr) - 128);
        let half = 1 << 15;
        let expected = [
            i64::from(y) + ((fix(1.402) * crx + half) >> 16),
            i64::from(y) + ((-fix(0.34414) * cbx + half - fix(0.71414) * crx) >> 16),
            i64::from(y) + ((fix(1.772) * cbx + half) >> 16),
        ]
        .map(|channel| channel.clamp(0, 255) as u8);
        assert_eq!(ycc_to_rgb(y, cb, cr), expected);
        // Every Cr through the red term, where a constant one off moves the
        // rounding somewhere.
        for cr in 0..=255u8 {
            let crx = i64::from(cr) - 128;
            let red = (128 + ((fix(1.402) * crx + half) >> 16)).clamp(0, 255) as u8;
            assert_eq!(ycc_to_rgb(128, 128, cr)[0], red, "Cr {cr}");
        }
        for cb in 0..=255u8 {
            let cbx = i64::from(cb) - 128;
            let blue = (128 + ((fix(1.772) * cbx + half) >> 16)).clamp(0, 255) as u8;
            let green = (128 + ((-fix(0.34414) * cbx + half) >> 16)).clamp(0, 255) as u8;
            assert_eq!(ycc_to_rgb(128, cb, 128)[2], blue, "Cb {cb}");
            assert_eq!(ycc_to_rgb(128, cb, 128)[1], green, "Cb {cb}");
        }
    }

    /// With no JFIF marker and no Adobe marker, libjpeg reads three
    /// components as YCbCr unless their ids spell `RGB`.
    #[test]
    fn three_components_without_a_marker_are_ycbcr_by_their_ids() {
        let (_, jpeg, rgb) = FIXTURES[0];
        let app0 = jpeg
            .windows(2)
            .position(|pair| pair == [0xff, 0xe0])
            .expect("a JFIF marker");
        let length = usize::from(u16::from_be_bytes([jpeg[app0 + 2], jpeg[app0 + 3]]));
        let mut bare = jpeg[..app0].to_vec();
        bare.extend_from_slice(&jpeg[app0 + 2 + length..]);
        let image = decode(&bare, &Limits::DEFAULT).expect("decodes");
        assert_eq!(first_difference(&image, rgb), None);
    }

    #[test]
    fn colour_converts_through_the_fixed_point_tables() {
        assert_eq!(ycc_to_rgb(128, 128, 128), [128, 128, 128]);
        // R = 76 + round(1.402 · 127), G and B likewise, each clamped.
        assert_eq!(ycc_to_rgb(76, 85, 255), [254, 0, 0]);
        assert_eq!(ycc_to_rgb(0, 0, 0), [0, 135, 0]);
    }

    #[test]
    fn a_frame_past_the_limits_is_refused_before_its_coefficients() {
        let (_, jpeg, _) = FIXTURES[0];
        let small = Limits {
            max_width: 8,
            max_height: 8,
            max_bytes: Limits::DEFAULT.max_bytes,
        };
        assert!(matches!(
            decode(jpeg, &small),
            Err(ImageError::TooLarge { .. })
        ));
        // A header claiming 65535 × 65535, nothing after it.
        let mut bomb = vec![0xff, 0xd8, 0xff, 0xc0, 0, 11, 8, 0xff, 0xff, 0xff, 0xff, 1];
        bomb.extend_from_slice(&[1, 0x11, 0]);
        assert!(matches!(
            decode(&bomb, &Limits::DEFAULT),
            Err(ImageError::TooLarge { .. })
        ));
    }

    #[test]
    fn what_it_does_not_read_is_unsupported_and_damage_is_malformed() {
        let (_, jpeg, _) = FIXTURES[0];
        let sof = jpeg
            .windows(2)
            .position(|pair| pair == [0xff, 0xc0])
            .expect("a baseline frame");
        let mut arithmetic = jpeg.to_vec();
        arithmetic[sof + 1] = 0xc9;
        let mut twelve_bit = jpeg.to_vec();
        twelve_bit[sof + 4] = 12;
        for (bytes, why) in [(arithmetic, "arithmetic"), (twelve_bit, "12-bit")] {
            assert!(
                matches!(
                    decode(&bytes, &Limits::DEFAULT),
                    Err(ImageError::Unsupported(_))
                ),
                "{why}"
            );
        }
        for bytes in [
            &b""[..],
            b"\xff\xd8",
            &jpeg[..jpeg.len() - 2],
            &jpeg[..jpeg.len() / 2],
        ] {
            assert!(
                matches!(
                    decode(bytes, &Limits::DEFAULT),
                    Err(ImageError::Malformed(_))
                ),
                "{} bytes",
                bytes.len()
            );
        }
    }

    /// A scan's table selectors are four bits each, but only tables 0 to 3
    /// exist: one naming 4 or above is damage, not a panic.
    #[test]
    fn a_scan_naming_a_huffman_table_past_three_is_malformed() {
        let (_, jpeg, _) = FIXTURES[0];
        let sos = jpeg
            .windows(2)
            .position(|pair| pair == [0xff, 0xda])
            .expect("a scan");
        // Marker, length and component count, then the first component's
        // id and its DC/AC selector byte.
        let selectors = sos + 6;
        for (byte, why) in [(0x40, "DC 4"), (0x0f, "AC 15"), (0xf0, "DC 15")] {
            let mut bytes = jpeg.to_vec();
            bytes[selectors] = byte;
            assert!(
                matches!(
                    decode(&bytes, &Limits::DEFAULT),
                    Err(ImageError::Malformed(_))
                ),
                "{why}"
            );
        }
    }
}
