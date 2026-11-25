//! JPEG, written here: baseline and progressive Huffman-coded JPEG (ITU-T
//! T.81) decoded to RGBA8.
//!
//! # What it reads
//!
//! Eight-bit samples, Huffman coding, sequential (`SOF0`, `SOF1`) and
//! progressive (`SOF2`) frames with spectral selection and successive
//! approximation, restart intervals, any sampling factors that divide the
//! largest evenly, and one component (grey), three (YCbCr, or RGB when an
//! Adobe marker or the component ids say so) or four (CMYK or YCCK, below).
//! Refused with [`ImageError::Unsupported`]: arithmetic coding, lossless and
//! hierarchical frames, 12-bit samples, and a height given only by a `DNL`
//! marker.
//!
//! The orientation an `APP1` segment's EXIF block records is applied by
//! [`decode`] and left alone by [`decode_stored`], as the [crate
//! docs](crate) say. The first `APP1` segment that starts `Exif\0\0` before
//! the first scan is the one read: Pillow's walk of the markers, which
//! `ImageOps.exif_transpose` reads the tag through, stops at that scan too.
//! An XMP `APP1`'s `tiff:Orientation`, which Pillow falls back to, is not
//! read ([EXIF only](crate::orientation#exif-only)).
//!
//! # CMYK and YCCK: Pillow's answer
//!
//! Decoders do not agree on how four components become RGB, so this one
//! matches Pillow 13's `Image.open(…).convert("RGB")`, read from its source
//! (`JpegImagePlugin.py`, and `cmyk2rgb` in `libImaging/Convert.c`):
//!
//! * libjpeg's rule picks the colour space: CMYK with no Adobe marker or an
//!   Adobe transform of 0, and YCCK under any other transform. YCCK becomes
//!   CMYK as libjpeg's `ycck_cmyk_convert` does it: the YCbCr part through
//!   the same fixed-point tables as a three-component file, each result
//!   subtracted from 255, K passed through.
//! * Every sample is then inverted, the Adobe (Photoshop) convention, which
//!   Pillow assumes of every four-component JPEG whether or not an Adobe
//!   marker is there. A decoder that inverts only under the marker draws a
//!   file without one as the negative of what this one draws.
//! * Each channel becomes `nk - (((t >> 8) + t) >> 8)`, where `t` is
//!   `s · nk + 128`, `s` the inverted sample and `nk` 255 less the inverted
//!   K: `cmyk2rgb`, the naive `(255 - C)(255 - K) / 255` rounded in shifts.
//!
//! No ICC profile is applied, here or in Pillow's `convert`: a print
//! profile embedded in the file is ignored, so the colours are the naive
//! conversion's, not a colour-managed viewer's.
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
//! A four-component frame goes through the same transform and upsampling,
//! its fourth component like the others. The tests hold this to exact
//! equality with libjpeg-turbo's output on committed files.
//!
//! # The same pixels as stb_image
//!
//! [`decode_as_stb_image`] gives what stb_image 2.30 gives instead: the
//! decoder llama.cpp loads images with, as `stbi_load_from_memory(…, 3)`
//! returns them in llama.cpp's x86-64 builds. It is stb_image's decoder
//! transcribed from `stb_image.h`, not this one with other stages: stb_image
//! reads markers, damage and truncation its own way, so its marker walk, bit
//! buffer and block decoding are transcribed too, and only the frame header
//! goes through the code here, for the [`Limits`] and the coefficient store.
//!
//! * The inverse DCT is stb_image's SSE2 version (`stbi__idct_simd`), which
//!   x86-64 builds run: sums of input pairs in 16 bits that wrap, 32-bit
//!   products, and each pass's results saturated to 16 bits. Its plain C
//!   version (`stbi__idct_block`), which llama.cpp's arm64 builds run, since
//!   llama.cpp does not define `STBI_NEON`, keeps 32 bits throughout. The
//!   two give the same bytes unless a block's dequantised coefficients are
//!   large enough to overflow 16 bits in those sums or results; there,
//!   this matches x86-64 and not arm64.
//! * Chroma is upsampled by stb_image's choice of filter: a triangle filter
//!   for 2:1 horizontally (`stbi__resample_row_h_2`), vertically
//!   (`stbi__resample_row_v_2`) and both ways (`stbi__resample_row_hv_2`),
//!   each rounding its own way, and the nearest sample for every other
//!   ratio.
//! * YCbCr becomes RGB in stb_image's reduced-precision fixed point
//!   (`stbi__YCbCr_to_RGB_row`, 12-bit constants).
//! * Grey is copied to all three channels. Three components are RGB when
//!   their ids spell `RGB`, or an Adobe marker's transform is 0 and there is
//!   no JFIF marker; YCbCr otherwise. Four are CMYK when the Adobe
//!   transform is 0, each channel times K by `stbi__blinn_8x8` with no
//!   inversion; YCCK when it is 2, the YCbCr part's RGB inverted and then
//!   times K; and with no Adobe marker or any other transform, YCbCr with
//!   K ignored. None of this is Pillow's answer above.
//! * The colour markers count wherever they are in the file, and the
//!   quantisation tables are the ones in force when a sequential block is
//!   read, or at the end of the file for a progressive one.
//! * Damage reads as stb_image reads it: a table no marker defined decodes
//!   as zeros, a file cut short reads on as zero bytes, a run past the end
//!   of a block writes its last coefficient, bytes that are not a marker
//!   between two segments before the frame are skipped, and a `DNL` marker
//!   repeating the frame's height is read. What stb_image refuses is
//!   refused, as [`ImageError::Malformed`]: among it a DC that does not fit
//!   16 bits once dequantised, and a Huffman code the bits before a marker
//!   cut short.
//!
//! The tests hold this to exact equality with stb_image's output on
//! committed files: baseline, progressive, every chroma sampling stb_image
//! filters and some it does not, odd sizes, restart intervals, grey, RGB,
//! four components, coefficients that overflow 16 bits, and damage.
//!
//! stb_image allocates its sample and coefficient buffers without clearing
//! them, and clears a block only by decoding it: a sequential block whole,
//! a progressive one by its DC first pass. Where its result depends on a
//! block it never cleared, it is whatever memory held, and the file is
//! refused as [`ImageError::Unsupported`]:
//!
//! * At a marker after the frame that it cannot read (an unknown one, a
//!   second frame, a table it cannot parse, bytes that are not a marker),
//!   or at the end of a file with no end-of-image marker, stb_image stops
//!   decoding and returns its buffers as they are. A sequential file is
//!   decoded where every block it returns was decoded before the stop; a
//!   progressive one never is, since stb_image transforms its coefficients
//!   only at the end-of-image marker.
//! * A restart interval with no marker after it ends its scan there, and
//!   the blocks the scan did not reach keep what they held.
//! * An AC refinement reads the coefficients it refines, and which it finds
//!   nonzero decides how many bits it reads: on a block no DC first pass
//!   has cleared, whether the scan fails, stops or decodes depends on
//!   memory. Some such files decode to the same bytes however stb_image's
//!   buffers start, when a later DC first pass clears every block the
//!   refinement wrote, but nothing short of every start proves it, so they
//!   are refused too.
//!
//! # Hostile input
//!
//! The frame's size passes the caller's [`Limits`] before anything is sized
//! from it, and the coefficient store is bounded by
//! [`coefficient_allowance`], a share of the limit's bytes for each
//! component. Entropy-coded data that
//! runs into a marker early is read as zeros, as libjpeg does with a damaged
//! scan; a file that ends before its end-of-image marker is
//! [`ImageError::Malformed`].

mod libjpeg;
mod stb;

use crate::orientation::EXIF_IDENTIFIER;
use crate::{ImageError, Limits, Orientation, Rgba8Image};

/// The most bytes a frame of `components` components may decode its
/// coefficients into under `limits`: two thirds of [`Limits::max_bytes`]
/// for each component.
///
/// A coefficient is two bytes, so a full-resolution component takes two
/// bytes a pixel against RGBA's four, half of `max_bytes`; the third over
/// that covers the padding to whole blocks. It scales with the components so
/// a frame of fewer is held tighter: three get twice `max_bytes`, the bound
/// before four-component frames were read, one gets two thirds of it, and
/// four (CMYK) eight thirds.
#[must_use]
pub fn coefficient_allowance(limits: &Limits, components: usize) -> usize {
    limits
        .max_bytes
        .saturating_mul(components)
        .saturating_mul(2)
        / 3
}

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
                match self.data.get(self.pos) {
                    // Fill bytes may come before a stuffed zero as before a
                    // marker, and are skipped there: as stb_image reads
                    // them, and libjpeg-turbo's bit reader in its slow path
                    // (`jpeg_fill_bit_buffer`). Its fast path, which it takes
                    // where enough bytes remain, reads `ff ff` as a marker
                    // instead, so its own output on such a stream depends on
                    // where the bytes fall.
                    Some(0xff) => {
                        let after_fill = past_fill(self.data, self.pos);
                        if self.data.get(after_fill + 1) == Some(&0x00) {
                            self.pos = after_fill + 2;
                            0xff
                        } else {
                            self.at_marker = true;
                            0
                        }
                    }
                    None => {
                        self.at_marker = true;
                        0
                    }
                    Some(&byte) => {
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

    /// The position of the marker after the scan's data, past any bytes
    /// left before it, stuffed zeros (with fill bytes before them) among
    /// them.
    fn end(&self) -> usize {
        let mut pos = self.pos;
        while pos < self.data.len() {
            if self.data[pos] == 0xff {
                let after_fill = past_fill(self.data, pos);
                if self.data.get(after_fill + 1) != Some(&0x00) {
                    return pos;
                }
                pos = after_fill + 1;
            }
            pos += 1;
        }
        pos
    }
}

/// The last `0xff` of the run that starts at `pos`: where a marker or a
/// stuffed zero's `0xff` is, past the fill bytes before it.
fn past_fill(data: &[u8], mut pos: usize) -> usize {
    while data.get(pos + 1) == Some(&0xff) {
        pos += 1;
    }
    pos
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
    colour: ColourMarkers,
}

/// What the markers say about the colour space, which libjpeg settles when
/// the first scan starts: markers after it change nothing.
#[derive(Clone, Copy, Debug, Default)]
struct ColourMarkers {
    /// The last Adobe `APP14` marker's transform.
    adobe_transform: Option<u8>,
    /// Whether a JFIF `APP0` marker was seen.
    jfif: bool,
}

/// Decodes a JPEG to straight RGBA8, every pixel opaque, upright:
/// [`decode_stored`]'s pixels with [`orientation`]'s flip or quarter turn
/// applied, the upright size checked against `limits` too.
///
/// # Errors
///
/// As [`decode_stored`], and [`ImageError::TooLarge`] if the upright size
/// breaks `limits`.
pub fn decode(bytes: &[u8], limits: &Limits) -> Result<Rgba8Image, ImageError> {
    let image = decode_stored(bytes, limits)?;
    orientation(bytes).apply(image, limits)
}

/// Decodes a JPEG to straight RGBA8, every pixel opaque, as stored: any EXIF
/// orientation not applied.
///
/// # Errors
///
/// [`ImageError::Empty`] or [`ImageError::TooLarge`] if the frame's size
/// breaks `limits`, [`ImageError::Unsupported`] for a kind of JPEG listed in
/// the [module docs](self), and [`ImageError::Malformed`] for anything that is
/// not a well-formed JPEG.
pub fn decode_stored(bytes: &[u8], limits: &Limits) -> Result<Rgba8Image, ImageError> {
    let mut tables = Tables::default();
    let (marker, segment, mut pos) = read_to_frame(bytes, &mut tables)?;
    let mut frame = read_frame(segment, marker == 0xc2, limits)?;
    let mut eobrun = 0u32;
    let mut colour = None;
    loop {
        let (marker, segment) = next_marker(bytes, &mut pos)?;
        match marker {
            0xd9 => break,
            0xc0..=0xc2 => return Err(malformed("a second frame")),
            0xda => {
                colour.get_or_insert(tables.colour);
                pos = read_scan(bytes, pos, segment, &mut frame, &tables, &mut eobrun)?;
            }
            _ => read_segment(marker, segment, &mut tables)?,
        }
    }
    libjpeg::finish(&frame, colour.unwrap_or(tables.colour), limits)
}

/// Decodes a JPEG to straight RGBA8, every pixel opaque, as stb_image 2.30
/// decodes it: the RGB llama.cpp's `stbi_load_from_memory(…, 3)` returns,
/// byte for byte, with alpha 255. Any EXIF orientation is not applied,
/// since stb_image does not read it.
///
/// [`decode_stored`] matches libjpeg-turbo instead, and the two differ by a
/// few levels on typical files and by more on chroma-subsampled and
/// four-component ones. The [module docs](self#the-same-pixels-as-stb_image)
/// say where and why, and what this reads that stb_image does not and the
/// reverse.
///
/// # Errors
///
/// [`ImageError::Empty`] or [`ImageError::TooLarge`] if the frame's size
/// breaks `limits`, [`ImageError::Malformed`] for a file stb_image refuses,
/// and [`ImageError::Unsupported`] for one whose stb_image result depends on
/// memory it never wrote: one it stops decoding before every block it
/// returns was decoded (at a marker it cannot read, at the end of a file
/// with no end-of-image marker, or where a restart interval ends without
/// its marker, so no whole scan reached some block), a progressive one it
/// stops decoding at all, and one whose AC refinement reads a block no DC
/// first pass cleared. The [module docs](self#the-same-pixels-as-stb_image)
/// say more.
pub fn decode_as_stb_image(bytes: &[u8], limits: &Limits) -> Result<Rgba8Image, ImageError> {
    stb::decode(bytes, limits)
}

/// The width and height a JPEG's frame header gives, read without decoding
/// a pixel: the size [`decode_stored`] returns, and [`decode`] too unless
/// [`orientation`] swaps its sides ([`Orientation::upright_size`]).
///
/// The markers are walked to the first frame header as [`decode`] walks
/// them, reading the tables and settings before it, and that header is
/// checked as [`decode`] checks it. So a file [`decode`] refuses before its
/// first scan — a frame of a kind the [module docs](self) list as
/// [`ImageError::Unsupported`], or a damaged table, restart interval or
/// frame header — is refused here with the same error. The scans are not
/// read, so damage there is found only by decoding. Nothing is allocated
/// for the image; the Huffman tables before the frame are built as
/// [`decode`] builds them.
///
/// The size is not checked against any [`Limits`]. Comparing it with the
/// caller's, or asking [`Limits::rgba_bytes`], is not quite [`decode`]'s
/// whole check: [`decode`] also refuses as [`ImageError::TooLarge`] a frame
/// whose coefficients, every component padded to whole MCUs at two bytes a
/// coefficient, take more than [`coefficient_allowance`] — a small frame
/// with large sampling factors, whose padding outweighs its pixels.
///
/// # Errors
///
/// [`ImageError::Empty`] for a zero width, [`ImageError::Unsupported`] for a
/// frame [`decode`] does not read, and [`ImageError::Malformed`] if the
/// bytes do not reach a well-formed frame header.
pub fn dimensions(bytes: &[u8]) -> Result<(u32, u32), ImageError> {
    let (_, segment, _) = read_to_frame(bytes, &mut Tables::default())?;
    let (width, height) = frame_size(segment)?;
    let size = crate::non_empty(u32::from(width), u32::from(height))?;
    frame_components(segment)?;
    Ok(size)
}

/// The orientation a JPEG's EXIF `APP1` segment records, read from the
/// markers before the first scan without decoding a pixel.
///
/// Never an error: no segment, a damaged one, damage in the markers before
/// it, or a file that is not a JPEG is [`Orientation::Normal`], as the
/// [`orientation`](crate::orientation) module docs say.
#[must_use]
pub fn orientation(bytes: &[u8]) -> Orientation {
    if bytes.get(..2) != Some(&[0xff, 0xd8]) {
        return Orientation::Normal;
    }
    let mut pos = 2;
    while let Ok((marker, segment)) = next_marker(bytes, &mut pos) {
        match marker {
            0xe1 if segment.starts_with(EXIF_IDENTIFIER) => {
                return Orientation::read_exif(segment);
            }
            0xd9 | 0xda => break,
            _ => {}
        }
    }
    Orientation::Normal
}

/// Walks from the start-of-image marker to the first frame header, reading
/// every segment before it into `tables`, and returns that header's marker,
/// its segment and the position after it.
fn read_to_frame<'a>(
    bytes: &'a [u8],
    tables: &mut Tables,
) -> Result<(u8, &'a [u8], usize), ImageError> {
    if bytes.get(..2) != Some(&[0xff, 0xd8]) {
        return Err(malformed("no start-of-image marker"));
    }
    let mut pos = 2;
    loop {
        let (marker, segment) = next_marker(bytes, &mut pos)?;
        match marker {
            0xd9 => return Err(malformed("no frame")),
            0xc0..=0xc2 => return Ok((marker, segment, pos)),
            0xda => return Err(malformed("a scan before its frame")),
            _ => read_segment(marker, segment, tables)?,
        }
    }
}

/// A segment read the same before the frame and after it: a table or
/// setting into `tables`, a frame this decoder does not read refused, and
/// anything else skipped.
fn read_segment(marker: u8, segment: &[u8], tables: &mut Tables) -> Result<(), ImageError> {
    match marker {
        0xdb => read_quant(segment, tables)?,
        0xc4 => read_huffman(segment, tables)?,
        0xdd => {
            let interval = segment
                .get(..2)
                .ok_or_else(|| malformed("a restart interval marker is cut short"))?;
            tables.restart_interval = usize::from(u16::from_be_bytes([interval[0], interval[1]]));
        }
        0xe0 if segment.starts_with(b"JFIF\0") => tables.colour.jfif = true,
        0xee if segment.starts_with(b"Adobe") => {
            tables.colour.adobe_transform = segment.get(11).copied();
        }
        0xc3 | 0xc5..=0xc7 | 0xc9..=0xcb | 0xcd..=0xcf => {
            return Err(unsupported(format!(
                "a JPEG frame of type {marker:#04x} (lossless, hierarchical or arithmetic)"
            )));
        }
        0xdc => return Err(dnl_height()),
        _ => {}
    }
    Ok(())
}

/// The marker at `pos`, past any fill bytes before it, and its segment
/// (empty for a marker that has none), with `pos` moved past both.
fn next_marker<'a>(bytes: &'a [u8], pos: &mut usize) -> Result<(u8, &'a [u8]), ImageError> {
    // Fill bytes may come before a marker.
    while bytes.get(*pos) == Some(&0xff) && bytes.get(*pos + 1) == Some(&0xff) {
        *pos += 1;
    }
    let marker = match (bytes.get(*pos), bytes.get(*pos + 1)) {
        (Some(0xff), Some(&marker)) => marker,
        _ => return Err(malformed("the file ends before its end-of-image marker")),
    };
    *pos += 2;
    if marker == 0xd9 || (0xd0..=0xd7).contains(&marker) || marker == 0x01 {
        return Ok((marker, &[]));
    }
    let length = match bytes.get(*pos..*pos + 2) {
        Some(&[high, low]) => usize::from(u16::from_be_bytes([high, low])),
        _ => return Err(malformed("a marker segment is cut short")),
    };
    let segment = bytes
        .get(*pos + 2..*pos + length.max(2))
        .filter(|_| length >= 2)
        .ok_or_else(|| malformed("a marker segment is cut short"))?;
    *pos += length;
    Ok((marker, segment))
}

fn dnl_height() -> ImageError {
    unsupported("a JPEG whose height comes in a DNL marker")
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
    let (width, height) = frame_size(segment)?;
    limits.rgba_bytes(u32::from(width), u32::from(height))?;
    let (specs, h_max, v_max) = frame_components(segment)?;
    let components: Vec<_> = specs
        .iter()
        .map(|spec| {
            let (h, v) = sampling(spec);
            (spec[0], h, v, usize::from(spec[2]))
        })
        .collect();
    let (width, height) = (usize::from(width), usize::from(height));
    let mcus_wide = width.div_ceil(8 * h_max);
    let mcus_high = height.div_ceil(8 * v_max);
    let blocks: usize = components
        .iter()
        .map(|&(_, h, v, _)| mcus_wide * h * mcus_high * v)
        .sum();
    if blocks.saturating_mul(64 * 2) > coefficient_allowance(limits, components.len()) {
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

/// A frame header's width and height, refusing what [`read_frame`] does not
/// read before its size is known.
fn frame_size(segment: &[u8]) -> Result<(u16, u16), ImageError> {
    let &[precision, h0, h1, w0, w1, _count, ..] = segment else {
        return Err(malformed("a frame header is cut short"));
    };
    if precision != 8 {
        return Err(unsupported(format!("{precision}-bit JPEG samples")));
    }
    let height = u16::from_be_bytes([h0, h1]);
    if height == 0 {
        return Err(dnl_height());
    }
    Ok((u16::from_be_bytes([w0, w1]), height))
}

/// A frame header's component specifications, three bytes each (id,
/// sampling factors, quantisation table), and the largest horizontal and
/// vertical sampling factors among them, refusing a set [`read_frame`] does
/// not read.
fn frame_components(segment: &[u8]) -> Result<(&[[u8; 3]], usize, usize), ImageError> {
    let (&count, rest) = segment
        .get(5..)
        .and_then(<[u8]>::split_first)
        .ok_or_else(|| malformed("a frame header is cut short"))?;
    let count = usize::from(count);
    if !matches!(count, 1 | 3 | 4) {
        return Err(unsupported(format!("a JPEG of {count} components")));
    }
    let specs = rest
        .get(..count * 3)
        .ok_or_else(|| malformed("a frame header's components are cut short"))?
        .as_chunks::<3>()
        .0;
    for spec in specs {
        let (h, v) = sampling(spec);
        if !(1..=4).contains(&h) || !(1..=4).contains(&v) || spec[2] > 3 {
            return Err(malformed("a component's sampling or table is out of range"));
        }
    }
    let h_max = specs.iter().map(|spec| sampling(spec).0).max().unwrap_or(1);
    let v_max = specs.iter().map(|spec| sampling(spec).1).max().unwrap_or(1);
    if specs.iter().any(|spec| {
        let (h, v) = sampling(spec);
        h_max % h != 0 || v_max % v != 0
    }) {
        return Err(unsupported(
            "sampling factors that do not divide the largest",
        ));
    }
    Ok((specs, h_max, v_max))
}

/// A component specification's horizontal and vertical sampling factors.
fn sampling(spec: &[u8; 3]) -> (usize, usize) {
    (usize::from(spec[1] >> 4), usize::from(spec[1] & 0x0f))
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
        let eobrun = frame.progressive.then_some(eobrun);
        decode_ac_first(bits, table, block, start, scan.end, scan.low, eobrun)
    } else {
        decode_ac_refine(bits, table, block, start, scan.end, scan.low, eobrun)
    }
}

/// A band's first pass, or a sequential block's AC: T.81 G.1.2.2, and F.2.2.2
/// when `eobrun` is `None`. A sequential scan has no end-of-band runs, so
/// there a zero-size symbol other than sixteen zeros ends the block and
/// reads no run length, as libjpeg and stb_image read it.
fn decode_ac_first(
    bits: &mut Bits<'_>,
    table: &Huffman,
    block: &mut [i16; 64],
    start: usize,
    end: usize,
    low: u32,
    mut eobrun: Option<&mut u32>,
) -> Result<(), ImageError> {
    if let Some(eobrun) = eobrun.as_deref_mut()
        && *eobrun > 0
    {
        *eobrun -= 1;
        return Ok(());
    }
    let mut k = start;
    while k <= end {
        let symbol = bits.decode(table)?;
        let (run, size) = (usize::from(symbol >> 4), u32::from(symbol & 0x0f));
        if size == 0 {
            if run < 15 {
                if let Some(eobrun) = eobrun {
                    *eobrun = (1 << run) - 1;
                    if run > 0 {
                        *eobrun += bits.bits(run as u32);
                    }
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

#[cfg(test)]
mod tests {
    use super::libjpeg::{cmyk_to_rgb, idct, ycc_to_rgb};
    use super::stb::tests::stb_fixture;
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

    /// Four-component files beside Pillow 13's `convert("RGB")` of each (its
    /// bundled libjpeg-turbo 3.2.0, then `cmyk2rgb`), all 37 × 23, a seeded
    /// gradient with noise. Pillow wrote the CMYK ones with Adobe markers
    /// (transform 0), sequential and progressive; a copy of the first has
    /// its transform patched to 1, which libjpeg reads as YCCK. A small C
    /// program against the system's libjpeg-turbo 3.2.1 wrote the rest from
    /// CMYK samples: CMYK with no Adobe marker, and YCCK (transform 2)
    /// unsubsampled, with Y and K sampled 2 × 2 over Cb and Cr, and that
    /// progressive.
    const FOUR_COMPONENTS: [(&str, &[u8], &[u8]); 7] = [
        fixture!("cmyk-adobe"),
        fixture!("cmyk-progressive"),
        fixture!("cmyk-transform-1"),
        fixture!("cmyk-plain"),
        fixture!("ycck-444"),
        fixture!("ycck-420"),
        fixture!("ycck-progressive"),
    ];

    /// The first pixel that differs, as `(x, y, ours, theirs)`.
    pub(super) fn first_difference(
        image: &Rgba8Image,
        rgb: &[u8],
    ) -> Option<(u32, u32, [u8; 3], [u8; 3])> {
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
    fn every_four_component_fixture_decodes_to_exactly_pillows_rgb() {
        for (name, jpeg, rgb) in FOUR_COMPONENTS {
            let image =
                decode(jpeg, &Limits::DEFAULT).unwrap_or_else(|error| panic!("{name}: {error}"));
            assert_eq!((image.width(), image.height()), (37, 23), "{name}");
            assert_eq!(image.pixels().len() / 4, rgb.len() / 3, "{name}");
            assert_eq!(first_difference(&image, rgb), None, "{name}");
        }
    }

    /// Two edited streams read as stb_image reads them. A fill byte before a
    /// stuffed zero is skipped, so the file decodes as the one it was
    /// edited from: libjpeg-turbo's slow-path bit reader skips it too, but
    /// its fast path reads `ff ff` as a marker, so its own output on such a
    /// stream depends on where the bytes fall (in the review, `djpeg` 3.2.1's
    /// output moved on 37 of 436 such edits). A sequential AC symbol of size
    /// 0 ends its block without reading a run length, as both of
    /// libjpeg-turbo's paths read it, and `djpeg` gave these pixels.
    #[test]
    fn libjpeg_reads_fill_before_a_stuffed_zero_and_sequential_run_symbols_as_stb_image() {
        let (original, _) = stb_fixture("baseline-420-17x9");
        let (filled, _) = stb_fixture("fill-stuffed-420");
        assert_ne!(original, filled);
        assert_eq!(
            decode_stored(filled, &Limits::DEFAULT),
            decode_stored(original, &Limits::DEFAULT)
        );
        let (jpeg, rgb) = stb_fixture("sequential-run-symbol");
        let image = decode_stored(jpeg, &Limits::DEFAULT).expect("decodes");
        assert_eq!(first_difference(&image, rgb), None);
    }

    /// libjpeg's CMYK samples beside Pillow 13's RGB for them: each
    /// inverted into a one-pixel `Image.new("CMYK", …)`, then
    /// `convert("RGB")`.
    #[test]
    fn cmyk_converts_as_pillows_cmyk2rgb() {
        for (cmyk, rgb) in [
            ([255, 255, 255, 255], [255, 255, 255]),
            ([0, 0, 0, 0], [0, 0, 0]),
            ([0, 255, 255, 255], [0, 255, 255]),
            ([255, 255, 255, 0], [0, 0, 0]),
            ([127, 191, 223, 55], [27, 41, 48]),
            ([254, 253, 1, 128], [127, 127, 1]),
            ([134, 189, 66, 13], [7, 10, 3]),
            ([222, 249, 15, 123], [107, 120, 7]),
            ([136, 157, 15, 12], [6, 7, 1]),
            ([52, 178, 137, 178], [36, 124, 96]),
        ] {
            assert_eq!(cmyk_to_rgb(cmyk), rgb, "{cmyk:?}");
        }
    }

    /// Every inverted sample and K through `cmyk2rgb` is `s · k / 255`
    /// rounded, never past a byte: the shifts are exact and need no clip.
    #[test]
    fn cmyk_is_the_rounded_product_for_every_sample_and_k() {
        for sample in 0..=255u8 {
            for k in 0..=255u8 {
                let product = u32::from(sample) * u32::from(k);
                let rounded = ((product * 2 + 255) / 510) as u8;
                assert_eq!(
                    cmyk_to_rgb([sample, sample, sample, k]),
                    [rounded; 3],
                    "{sample} {k}"
                );
            }
        }
    }

    /// libjpeg settles the colour space when the first scan starts, so a
    /// marker after it changes nothing: an Adobe transform of 2 before a
    /// progressive CMYK file's second scan, and a JFIF marker before an
    /// RGB file's end. Pillow 13 decodes both edited files to the same
    /// pixels as the originals.
    #[test]
    fn colour_markers_after_the_first_scan_change_nothing() {
        let (_, cmyk, cmyk_rgb) = FOUR_COMPONENTS[1];
        let scans: Vec<usize> = cmyk
            .windows(2)
            .enumerate()
            .filter_map(|(at, pair)| (pair == [0xff, 0xda]).then_some(at))
            .collect();
        assert!(scans.len() > 1, "a progressive file");
        let mut late_adobe = cmyk[..scans[1]].to_vec();
        late_adobe.extend_from_slice(b"\xff\xee\x00\x0eAdobe\x00\x64\x00\x00\x00\x00\x02");
        late_adobe.extend_from_slice(&cmyk[scans[1]..]);
        let image = decode(&late_adobe, &Limits::DEFAULT).expect("decodes");
        assert_eq!(
            first_difference(&image, cmyk_rgb),
            None,
            "a late Adobe marker"
        );

        let (_, rgb_jpeg, rgb) = FIXTURES[12];
        let mut late_jfif = rgb_jpeg[..rgb_jpeg.len() - 2].to_vec();
        late_jfif
            .extend_from_slice(b"\xff\xe0\x00\x10JFIF\x00\x01\x01\x00\x00\x01\x00\x01\x00\x00");
        late_jfif.extend_from_slice(&rgb_jpeg[rgb_jpeg.len() - 2..]);
        let image = decode(&late_jfif, &Limits::DEFAULT).expect("decodes");
        assert_eq!(first_difference(&image, rgb), None, "a late JFIF marker");
    }

    /// The coefficient bound sits where [`coefficient_allowance`] puts it,
    /// for one, three and four components: a 1 × 1 frame whose first
    /// component is sampled 4 × 4 pads to one 32 × 32 MCU, 16 blocks for
    /// that component and one for each other, 128 bytes a block. Its RGBA
    /// is 4 bytes, so only the coefficients decide.
    #[test]
    fn the_coefficient_bound_is_two_thirds_of_the_bytes_a_component() {
        for (components, smallest) in [(1, 3072), (3, 1152), (4, 912)] {
            let blocks = 16 + (components - 1);
            assert_eq!(blocks * 128, smallest * 2 * components / 3, "{components}");
            let mut segment = vec![8, 0, 1, 0, 1, components as u8];
            for id in 0..components as u8 {
                segment.extend_from_slice(&[id, if id == 0 { 0x44 } else { 0x11 }, 0]);
            }
            let limits = |max_bytes| Limits {
                max_width: 1,
                max_height: 1,
                max_bytes,
            };
            assert!(
                read_frame(&segment, false, &limits(smallest)).is_ok(),
                "{components} just inside"
            );
            assert!(
                matches!(
                    read_frame(&segment, false, &limits(smallest - 1)),
                    Err(ImageError::TooLarge { .. })
                ),
                "{components} just outside"
            );
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

    #[test]
    fn dimensions_are_the_size_decode_returns() {
        for (name, jpeg, _) in FIXTURES.into_iter().chain(FOUR_COMPONENTS) {
            let image = decode(jpeg, &Limits::DEFAULT).expect(name);
            assert_eq!(
                dimensions(jpeg),
                Ok((image.width(), image.height())),
                "{name}"
            );
        }
        // Fill bytes before the frame's marker are skipped, as decode skips
        // them.
        let (_, jpeg, _) = FIXTURES[0];
        let sof = jpeg
            .windows(2)
            .position(|pair| pair == [0xff, 0xc0])
            .expect("a baseline frame");
        let mut filled = jpeg[..sof].to_vec();
        filled.extend_from_slice(&[0xff; 3]);
        filled.extend_from_slice(&jpeg[sof..]);
        let image = decode(&filled, &Limits::DEFAULT).expect("decodes");
        assert_eq!(dimensions(&filled), Ok((image.width(), image.height())));
        // Past any limit: the caller decides.
        let mut bomb = vec![0xff, 0xd8, 0xff, 0xc0, 0, 11, 8, 0xff, 0xfe, 0xff, 0xfd, 1];
        bomb.extend_from_slice(&[1, 0x11, 0]);
        assert_eq!(dimensions(&bomb), Ok((0xfffd, 0xfffe)));
    }

    #[test]
    fn dimensions_refuse_what_decode_refuses_in_the_header() {
        let (_, jpeg, _) = FIXTURES[0];
        let sof = jpeg
            .windows(2)
            .position(|pair| pair == [0xff, 0xc0])
            .expect("a baseline frame");
        let changed = |at: usize, byte: u8| {
            let mut bytes = jpeg.to_vec();
            bytes[at] = byte;
            bytes
        };
        // The marker, its length, then precision, height, width and the
        // component count; then each component's id, sampling and table.
        let mut dnl_marker = jpeg[..sof].to_vec();
        dnl_marker.extend_from_slice(&[0xff, 0xdc, 0, 4, 0, 1]);
        dnl_marker.extend_from_slice(&jpeg[sof..]);
        let mut odd_sampling = jpeg.to_vec();
        odd_sampling[sof + 11] = 0x31;
        odd_sampling[sof + 14] = 0x21;
        for (what, bytes) in [
            ("lossless", changed(sof + 1, 0xc3)),
            ("hierarchical", changed(sof + 1, 0xc5)),
            ("arithmetic", changed(sof + 1, 0xc9)),
            ("12-bit", changed(sof + 4, 12)),
            ("a DNL height", {
                let mut bytes = changed(sof + 5, 0);
                bytes[sof + 6] = 0;
                bytes
            }),
            ("a DNL marker", dnl_marker),
            ("two components", changed(sof + 9, 2)),
            ("five components", changed(sof + 9, 5)),
            ("sampling that does not divide", odd_sampling),
        ] {
            assert!(
                matches!(dimensions(&bytes), Err(ImageError::Unsupported(_))),
                "{what}: {:?}",
                dimensions(&bytes)
            );
            assert_eq!(
                dimensions(&bytes),
                decode(&bytes, &Limits::DEFAULT).map(|image| (image.width(), image.height())),
                "{what}: the same error as decode"
            );
        }
        let length = usize::from(u16::from_be_bytes([jpeg[sof + 2], jpeg[sof + 3]]));
        let mut malformed = (0..sof + 2 + length)
            .map(|end| (format!("cut at {end}"), jpeg[..end].to_vec()))
            .collect::<Vec<_>>();
        malformed.push(("a zero sampling factor".into(), changed(sof + 11, 0x01)));
        malformed.push((
            "four components, three specified".into(),
            changed(sof + 9, 4),
        ));
        malformed.push((
            "a quantisation table past three".into(),
            changed(sof + 12, 4),
        ));
        let sos = jpeg
            .windows(2)
            .position(|pair| pair == [0xff, 0xda])
            .expect("a scan");
        let mut scan_first = jpeg[..sof].to_vec();
        scan_first.extend_from_slice(&jpeg[sos..]);
        malformed.push(("a scan before its frame".into(), scan_first));
        malformed.push(("no frame".into(), vec![0xff, 0xd8, 0xff, 0xd9]));
        // Segments before the frame that decode reads, each put straight
        // after the start-of-image marker: a restart interval with no
        // interval, and a quantisation and a Huffman table misnumbered in
        // the byte after their lengths.
        let mut dht = vec![0xff, 0xc4, 0, 19, 0x25];
        dht.extend_from_slice(&[0; 16]);
        let mut dqt = vec![0xff, 0xdb, 0, 67, 0x05];
        dqt.extend_from_slice(&[1; 64]);
        for (what, segment) in [
            ("a restart interval cut short", vec![0xff, 0xdd, 0, 2]),
            ("a quantisation table numbered 5", dqt),
            ("a Huffman table of class 2", dht),
        ] {
            let mut bytes = jpeg[..2].to_vec();
            bytes.extend_from_slice(&segment);
            bytes.extend_from_slice(&jpeg[2..]);
            malformed.push((what.into(), bytes));
        }
        for (what, bytes) in malformed {
            assert!(
                matches!(dimensions(&bytes), Err(ImageError::Malformed(_))),
                "{what}: {:?}",
                dimensions(&bytes)
            );
            assert_eq!(
                dimensions(&bytes),
                decode(&bytes, &Limits::DEFAULT).map(|image| (image.width(), image.height())),
                "{what}: the same error as decode"
            );
        }
        let mut zero_width = changed(sof + 7, 0);
        zero_width[sof + 8] = 0;
        let empty = |result: Result<(u32, u32), ImageError>| {
            matches!(result, Err(ImageError::Empty { width: 0, .. }))
        };
        assert!(empty(dimensions(&zero_width)));
        assert!(empty(
            decode(&zero_width, &Limits::DEFAULT).map(|image| (image.width(), image.height()))
        ));
    }

    /// `rgba_bytes` is not decode's whole size check, as the docs on
    /// `dimensions` say: a 1×1 frame sampled 4×4 pads its coefficients to a
    /// whole 32×32 MCU.
    #[test]
    fn a_frame_within_rgba_bytes_can_still_have_too_many_coefficients() {
        let mut bytes = vec![0xff, 0xd8, 0xff, 0xc0, 0, 11, 8, 0, 1, 0, 1, 1];
        bytes.extend_from_slice(&[1, 0x44, 0, 0xff, 0xd9]);
        let limits = Limits {
            max_width: 1,
            max_height: 1,
            max_bytes: 4,
        };
        assert_eq!(dimensions(&bytes), Ok((1, 1)));
        assert_eq!(limits.rgba_bytes(1, 1), Ok(4));
        assert!(matches!(
            decode(&bytes, &limits),
            Err(ImageError::TooLarge {
                width: 1,
                height: 1,
                ..
            })
        ));
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

    /// Pillow 13 wrote one 5 × 3 picture at quality 95, 4:4:4, with EXIF
    /// orientation 1 to 8, and ImageMagick 7.1 rewrote the first with 6 in
    /// a big-endian block; each `.rgb` is Pillow's decode through
    /// `ImageOps.exif_transpose`, which ImageMagick's `-auto-orient`
    /// matched byte for byte.
    macro_rules! oriented {
        ($name:literal, $value:literal) => {
            (
                $name,
                $value,
                include_bytes!(concat!("../fixtures/orientation/", $name, ".jpg")) as &[u8],
                include_bytes!(concat!("../fixtures/orientation/", $name, ".jpg.rgb")) as &[u8],
            )
        };
    }

    pub(super) const ORIENTED: [(&str, u16, &[u8], &[u8]); 9] = [
        oriented!("1", 1),
        oriented!("2", 2),
        oriented!("3", 3),
        oriented!("4", 4),
        oriented!("5", 5),
        oriented!("6", 6),
        oriented!("7", 7),
        oriented!("8", 8),
        oriented!("magick-6", 6),
    ];

    #[test]
    fn exif_orientation_is_applied_as_pillow_and_imagemagick_apply_it() {
        for (name, value, jpeg, upright) in ORIENTED {
            assert_eq!(orientation(jpeg).exif_value(), value, "{name}");
            let image = decode(jpeg, &Limits::DEFAULT).expect(name);
            let size = Orientation::from_exif(value).upright_size((5, 3));
            assert_eq!((image.width(), image.height()), size, "{name}");
            assert_eq!(first_difference(&image, upright), None, "{name}");
            let stored = decode_stored(jpeg, &Limits::DEFAULT).expect(name);
            assert_eq!(dimensions(jpeg), Ok((stored.width(), stored.height())));
            assert_eq!(
                Orientation::from_exif(value).apply(stored, &Limits::DEFAULT),
                Ok(image),
                "{name}"
            );
        }
    }

    /// Only an `Exif` `APP1` before the first scan counts; anything else,
    /// damage on the way to it included, is normal, and no cut panics.
    #[test]
    fn exif_is_read_before_the_first_scan_and_damage_is_normal() {
        let (_, _, six, _) = ORIENTED[5];
        let app1 = six
            .windows(2)
            .position(|pair| pair == [0xff, 0xe1])
            .expect("an APP1");
        let length = usize::from(u16::from_be_bytes([six[app1 + 2], six[app1 + 3]]));
        let segment = &six[app1..app1 + 2 + length];
        let mut without = six[..app1].to_vec();
        without.extend_from_slice(&six[app1 + 2 + length..]);
        assert_eq!(orientation(&without), Orientation::Normal);
        // The segment moved to straight after the first scan's header, where
        // a walk that did not stop at the scan would read it next.
        let sos = without
            .windows(2)
            .position(|pair| pair == [0xff, 0xda])
            .expect("a scan");
        let sos_end =
            sos + 2 + usize::from(u16::from_be_bytes([without[sos + 2], without[sos + 3]]));
        let mut late = without[..sos_end].to_vec();
        late.extend_from_slice(segment);
        late.extend_from_slice(&without[sos_end..]);
        assert_eq!(orientation(&late), Orientation::Normal);
        // Another identifier, and an EXIF block that is not TIFF.
        let mut other = six.to_vec();
        other[app1 + 4] = b'X';
        assert_eq!(orientation(&other), Orientation::Normal);
        let mut broken = six.to_vec();
        broken[app1 + 10] = b'Q';
        assert_eq!(orientation(&broken), Orientation::Normal);
        assert_eq!(
            decode(&broken, &Limits::DEFAULT),
            decode_stored(six, &Limits::DEFAULT)
        );
        for cut in 0..six.len() {
            assert!(matches!(
                orientation(&six[..cut]),
                Orientation::Normal | Orientation::Rotate90Clockwise
            ));
        }
        assert_eq!(
            orientation(b"\xff\xd8\xff\xe1\xff\xff"),
            Orientation::Normal
        );
    }

    /// The upright size passes the limits: a 5 × 3 image turned is 3 × 5,
    /// past limits 5 wide and 3 high. The bytes are left room for the
    /// coefficients of the blocks the stored image pads to.
    #[test]
    fn a_quarter_turn_is_checked_against_the_limits() {
        let limits = Limits {
            max_width: 5,
            max_height: 3,
            max_bytes: 1024,
        };
        let (_, _, six, _) = ORIENTED[5];
        assert!(decode_stored(six, &limits).is_ok());
        assert_eq!(
            decode(six, &limits),
            Err(ImageError::TooLarge {
                width: 3,
                height: 5,
                limits
            })
        );
    }
}
