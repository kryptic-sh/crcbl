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
//! Apart from that, a file whose scans would do more work than their data
//! allows is refused as [`ImageError::TooMuchWork`], determined or not, as
//! the [work bound](#work-bounded-by-the-input) below says: a resource
//! limit, like the frame's [`Limits`].
//!
//! # Hostile input
//!
//! The frame's size passes the caller's [`Limits`] before anything is sized
//! from it, and the coefficient store is bounded by
//! [`coefficient_allowance`], a share of the limit's bytes for each
//! component. Entropy-coded data that runs into a marker early is read as
//! libjpeg-turbo reads it: zeros to the end of the MCU it runs out in, and
//! the rest of its restart interval left undecoded, a sequential block with
//! its coefficients zero (flat, every sample mid-level) and a progressive
//! one with what the scans before gave it. A marker from `0xc0` up that is
//! not a restart marker, where one should be, ends the scan there, and the
//! rest of it is left so too, but for the first MCU after it, which is read
//! from zeros when the interval before was whole; a marker below `0xc0`
//! there is refused.
//! libjpeg-turbo then smooths a progressive frame across its blocks while
//! any component's first nine AC coefficients are not coded to their last
//! bit (`decompress_smooth_data` in `jdcoefct.c`), which this decoder does
//! not do, so a progressive file cut short before its last scan can differ
//! from its output there. A file that ends before its end-of-image marker
//! is [`ImageError::Malformed`] here.
//!
//! ## Work bounded by the input
//!
//! A scan can cover every block of the frame in a few bytes, or in none:
//! stb_image reads zeros past the end of a scan's data, so a scan with no
//! data still decodes a whole frame there, and a progressive one can skip
//! any number of blocks with end-of-band runs in both decoders. Work done
//! from real data bounds itself, since each Huffman code, magnitude bit and
//! correction bit read is at least one bit of the file. What does not is
//! work done without bits, so both decoders count it as they go, block by
//! block, and refuse the file as [`ImageError::TooMuchWork`] the moment it
//! passes:
//!
//! * the blocks the scans walk, beyond one walk over the frame, against
//!   800 for each entropy-coded byte they brought. In its end-of-band
//!   tails a refinement visits only the coefficients it corrects, and
//!   elsewhere at most a run's zeros between reads, so a block costs about
//!   as much to walk in every kind of scan;
//! * the reads of bits made up past the end of the scans' data, over the
//!   whole file, against 128 for each block of the frame and 64 for each
//!   byte. A read is a Huffman code with the magnitude or run bits after
//!   it, or a lone sign, correction or refinement bit. The most one pass
//!   reads in a block is 127, in a refinement (a code and a sign for each
//!   new coefficient, a correction for each nonzero one, and an end of
//!   band), so the share for each block lets every block be read once
//!   from made-up bits, which is what a file cut short needs in stb_image's
//!   flavour (the default decoder reads only the rest of an MCU); the share
//!   for each byte is for a file damaged in more than one scan. In a scan
//!   charged at its end, reads are checked while the reader is past the
//!   data; a restart starts it on the next interval's data, so reads made
//!   up at the end of an interval may be refused only when the scan ends;
//! * the scans, against [`MAX_SCANS`].
//!
//! Each counts what is done, not what a scan could do, so a scan stb_image
//! stops early is charged for the blocks it walked. A file is refused
//! when its scans would do more work than its data allows, whether or not
//! its pixels are otherwise determined, and the refusal names the bound
//! ([`WorkBound`](crate::WorkBound)).
//!
//! A real file stays under all three. Each component's first DC pass, or
//! its sequential scan, codes every block it covers in at least one bit, so
//! the bytes are at least an eighth of the frame's blocks, and with at most
//! [`MAX_SCANS`] scans the walk past one frame is at most 99 frames: 792
//! blocks a byte at the most. The closest a real encoder comes is
//! jpegtran with a script of 100 scans, its own limit, one AC coefficient a
//! scan, over a flat 8192 × 8192 grey frame (one bit a DC block, end-of-band
//! runs for the rest): 742 blocks a byte. The same over flat colour comes
//! to 367 at 4:2:0 and 248 at 4:4:4 (96 scans), libjpeg-turbo's own
//! progressive script to 15 to 24, and baseline and restart-heavy files to
//! under 1. A complete file reads no made-up bits. A file cut short reads
//! what decoding the rest of its scan from zeros takes in stb_image's
//! flavour, at most a pass of reads for each block left, and in the default
//! decoder what the rest of the MCU its data runs out in takes, and the
//! first MCU of each restart interval after it that has no data. A file
//! damaged in more than one scan takes that for each damaged scan, and each
//! brought bytes; one whose damage would cost more than its bytes and one
//! frame allow is refused.
//!
//! Other decoders cap the count of scans, which bounds passes but not the
//! bytes behind them: Chrome's (Blink's `ProgressMonitor` in
//! `jpeg_image_decoder.cc` aborts at scan 100), zune-jpeg's `max_scans`
//! (100 by default), libjpeg-turbo's `djpeg -maxscans` and
//! `TJPARAM_SCANLIMIT` (off by default; its fuzz targets set 100 for lossy
//! images), and jpegtran's scan scripts (`MAX_SCANS`, 100). jpeg-decoder has
//! no such cap, and Pillow bounds pixels only (`MAX_IMAGE_PIXELS`).

mod libjpeg;
mod stb;
mod work;

pub use work::MAX_SCANS;

use crate::orientation::EXIF_IDENTIFIER;
use crate::{ImageError, Limits, Orientation, Rgba8Image};
use work::Work;

/// The most bytes a frame of `components` components may decode its
/// coefficients into under `limits`: two thirds of [`Limits::max_bytes`]
/// for each component.
///
/// A coefficient is two bytes, so a full-resolution component takes two
/// bytes a pixel against RGBA's four, half of `max_bytes`; the third over
/// that covers the padding to whole blocks. It scales with the components so
/// a frame of fewer is held tighter: three get twice `max_bytes`, the bound
/// before four-component frames were read, one gets two thirds of it, and
/// four (CMYK) eight thirds. A progressive frame also keeps eight bytes a
/// block (a sixteenth more) for which of its coefficients are nonzero, and
/// they count against the same allowance.
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
    /// Whether `acc` holds a byte made up past the marker or the end.
    made_up_byte: bool,
    /// Whether a bit has been made up since the reader last started on real
    /// data: libjpeg-turbo's `insufficient_data`, which `jpeg_fill_bit_buffer`
    /// sets when it must give a bit from past a marker. The MCU it is set in
    /// is decoded to its end from zeros; [`read_scan`] leaves every MCU after
    /// it undecoded until a restart marker clears it.
    out_of_data: bool,
    /// The reads before it that used a made-up bit, for the [work
    /// bound](Work).
    padded_reads: u64,
}

impl<'a> Bits<'a> {
    fn new(data: &'a [u8], pos: usize) -> Self {
        Self {
            data,
            pos,
            acc: 0,
            count: 0,
            at_marker: false,
            made_up_byte: false,
            out_of_data: false,
            padded_reads: 0,
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
            self.made_up_byte = self.at_marker;
            self.out_of_data |= self.at_marker;
            self.acc = u32::from(byte);
            self.count = 8;
        }
        self.count -= 1;
        (self.acc >> self.count) & 1
    }

    /// Starts a read: a Huffman code, with the magnitude or run bits read
    /// after it, or a lone bit. The read before it is counted in
    /// `padded_reads` if any of its bits was made up: if its last bit was,
    /// since within a restart interval every real byte comes before every
    /// made-up one, and a read does not span a restart. A restart leaves
    /// `made_up_byte` as it was, so the read before it is counted rightly.
    fn begin_read(&mut self) {
        // False for every read of real data, so the branch is all but free.
        if self.made_up_byte {
            self.padded_reads += 1;
        }
    }

    /// The made-up reads so far, the one in progress (the last, between
    /// blocks) included.
    fn made_up_reads(&self) -> u64 {
        self.padded_reads + u64::from(self.made_up_byte)
    }

    /// A lone bit, as one read.
    fn read_bit(&mut self) -> u32 {
        self.begin_read();
        self.bit()
    }

    fn bits(&mut self, n: u32) -> u32 {
        (0..n).fold(0, |value, _| (value << 1) | self.bit())
    }

    /// T.81 F.2.2.3's `DECODE`, as one read. Inlined into the block
    /// decoders, which call it for every symbol.
    #[inline(always)]
    fn decode(&mut self, table: &Huffman) -> Result<u8, ImageError> {
        self.begin_read();
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
    ///
    /// A marker there that [`ends_scan`] ends the scan's data, and is left
    /// where it is, as libjpeg-turbo's `jpeg_resync_to_restart` leaves any
    /// such marker (its third action): the reader stays at it,
    /// reading zeros, and `out_of_data` stays as it was, as libjpeg-turbo's
    /// `process_restart` leaves `insufficient_data` against a marker, so
    /// every interval left in the scan reads as empty. Its other recoveries,
    /// from bytes or an invalid marker before the restart marker, are not
    /// made here.
    fn restart(&mut self) -> Result<(), ImageError> {
        self.acc = 0;
        self.count = 0;
        while self.data.get(self.pos) == Some(&0xff) && self.data.get(self.pos + 1) == Some(&0xff) {
            self.pos += 1;
        }
        match (self.data.get(self.pos), self.data.get(self.pos + 1)) {
            (Some(0xff), Some(0xd0..=0xd7)) => {
                self.pos += 2;
                self.at_marker = false;
                self.out_of_data = false;
                Ok(())
            }
            (Some(0xff), Some(&marker)) if ends_scan(marker) => {
                self.at_marker = true;
                Ok(())
            }
            _ => Err(malformed("a restart interval ends without its marker")),
        }
    }

    /// Whether the reader has stopped at a marker that [`ends_scan`]: one
    /// [`restart`](Self::restart) leaves where it is, so no restart can
    /// start the reader again. At any other marker, or at the end of the
    /// data, the restart that comes next takes it or refuses it.
    fn at_scan_end(&self) -> bool {
        let marker = self.data.get(past_fill(self.data, self.pos) + 1);
        self.at_marker && marker.is_some_and(|&marker| ends_scan(marker))
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

/// Whether `marker` ends a scan's data where a restart marker should be:
/// a valid marker that is not a restart marker, which libjpeg-turbo's
/// `jpeg_resync_to_restart` leaves unread (its third action). A marker
/// below `0xc0` it reads past instead (its second), which is not done
/// here, so one there is refused.
fn ends_scan(marker: u8) -> bool {
    matches!(marker, 0xc0..=0xcf | 0xd8..=0xfe)
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
    /// For a progressive frame, a mask for each block of which AC
    /// coefficients are nonzero, bit `k` for zigzag position `k`: what lets
    /// a refinement pass straight to the coefficients it corrects, rather
    /// than walking every position of its band in every block. Empty for a
    /// sequential frame, which never refines.
    nonzero: Vec<u64>,
    dc_prediction: i32,
}

/// Records in a block's `nonzero` mask whether the coefficient at zigzag
/// position `k` is now nonzero.
fn mark(nonzero: &mut u64, k: usize, value: i16) {
    let bit = 1 << k;
    if value == 0 {
        *nonzero &= !bit;
    } else {
        *nonzero |= bit;
    }
}

/// The zigzag positions from `start` to `end` that a block's `nonzero` mask
/// has set, in order: the coefficients a refinement corrects there.
fn nonzero_between(nonzero: u64, start: usize, end: usize) -> impl Iterator<Item = usize> {
    let mut left = if start > end {
        0
    } else {
        nonzero & (u64::MAX >> (63 - end)) & (u64::MAX << start)
    };
    core::iter::from_fn(move || {
        (left != 0).then(|| {
            let k = left.trailing_zeros() as usize;
            left &= left - 1;
            k
        })
    })
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
/// breaks `limits`, [`ImageError::TooMuchWork`] for scans that would do
/// more work than their data allows (see [the module
/// docs](self#work-bounded-by-the-input)), [`ImageError::Unsupported`] for
/// a kind of JPEG listed in the [module docs](self), and
/// [`ImageError::Malformed`] for anything that is not a well-formed JPEG.
pub fn decode_stored(bytes: &[u8], limits: &Limits) -> Result<Rgba8Image, ImageError> {
    decode_stored_with(bytes, limits, &mut Work::default())
}

/// [`decode_stored`], its entropy decoding accounted in `work`.
fn decode_stored_with(
    bytes: &[u8],
    limits: &Limits,
    work: &mut Work,
) -> Result<Rgba8Image, ImageError> {
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
                pos = read_scan(bytes, pos, segment, &mut frame, &tables, &mut eobrun, work)?;
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
/// breaks `limits`, [`ImageError::TooMuchWork`] for scans that would do
/// more work than their data allows, determined or not, as for
/// [`decode_stored`],
/// [`ImageError::Malformed`] for a file stb_image refuses,
/// and [`ImageError::Unsupported`] for one whose stb_image result depends on
/// memory it never wrote: one it stops decoding before every block it
/// returns was decoded (at a marker it cannot read, at the end of a file
/// with no end-of-image marker, or where a restart interval ends without
/// its marker, so no whole scan reached some block), a progressive one it
/// stops decoding at all, and one whose AC refinement reads a block no DC
/// first pass cleared. The [module docs](self#the-same-pixels-as-stb_image)
/// say more.
pub fn decode_as_stb_image(bytes: &[u8], limits: &Limits) -> Result<Rgba8Image, ImageError> {
    stb::decode(bytes, limits, &mut Work::default())
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
/// coefficient (and eight bytes a block more for a progressive one), take
/// more than [`coefficient_allowance`] — a small frame
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
    // Two bytes a coefficient, and for a progressive frame a nonzero mask of
    // eight bytes a block.
    let block_bytes = if progressive { 64 * 2 + 8 } else { 64 * 2 };
    if blocks.saturating_mul(block_bytes) > coefficient_allowance(limits, components.len()) {
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
            nonzero: if progressive {
                vec![0; mcus_wide * h * mcus_high * v]
            } else {
                Vec::new()
            },
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
    work: &mut Work,
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
    let mcu_blocks = if count == 1 { 1 } else { blocks_per_mcu };
    let scan_blocks = (mcu_count * mcu_blocks) as u64;
    work.begin_scan(frame, (bytes, data_start), scan_blocks)?;
    let reads_before = work.padded_reads();
    let contexts: Vec<BlockContext<'_>> = scan
        .components
        .iter()
        .map(|&(index, dc, ac)| BlockContext {
            scan: &scan,
            index,
            dc: tables.dc[dc].as_ref(),
            ac: tables.ac[ac].as_ref(),
            first_pass: frame.progressive && scan.start > 0 && scan.high == 0,
        })
        .collect();
    let mut walked = scan_blocks;
    for mcu in 0..mcu_count {
        if tables.restart_interval > 0 && mcu > 0 && mcu % tables.restart_interval == 0 {
            bits.restart()?;
            for &(index, ..) in &scan.components {
                frame.components[index].dc_prediction = 0;
            }
            *eobrun = 0;
        }
        // Once a bit has been made up, libjpeg-turbo's MCU decoders
        // (`decode_mcu` in `jdhuff.c`, and each in `jdphuff.c`) leave the
        // rest of the interval as it was: a sequential block's coefficients
        // zero, a progressive one's what the scans before gave it. Its DC
        // refinement reads on, but zeros change nothing there. With no
        // restart marker ahead to start the reader again, that is the rest
        // of the scan, so the walk stops; otherwise the blocks to the
        // restart are walked, and charged, undecoded.
        let decoded = !bits.out_of_data;
        if !decoded && (tables.restart_interval == 0 || bits.at_scan_end()) {
            walked = (mcu * mcu_blocks) as u64;
            break;
        }
        if count == 1 {
            let (x, y) = (mcu % own_blocks_wide, mcu / own_blocks_wide);
            if work.needs_block(bits.at_marker) {
                work.block(bits.at_marker, reads_before + bits.made_up_reads())?;
            }
            if decoded {
                decode_block(&mut bits, frame, &contexts[0], (x, y), eobrun)?;
            }
        } else {
            let (mcu_x, mcu_y) = (mcu % frame.mcus_wide, mcu / frame.mcus_wide);
            for context in &contexts {
                let component = &frame.components[context.index];
                let (h, v) = (component.h, component.v);
                for by in 0..v {
                    for bx in 0..h {
                        let at = (mcu_x * h + bx, mcu_y * v + by);
                        if work.needs_block(bits.at_marker) {
                            work.block(bits.at_marker, reads_before + bits.made_up_reads())?;
                        }
                        if decoded {
                            decode_block(&mut bits, frame, context, at, eobrun)?;
                        }
                    }
                }
            }
        }
    }
    // The last block's reads count too.
    work.end_scan(walked, reads_before + bits.made_up_reads())?;
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
    /// Whether the scan is a progressive band's first pass, whose blocks
    /// inside an end-of-band run are passed untouched.
    first_pass: bool,
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
    // A band's first pass inside an end-of-band run touches nothing in the
    // block: the commonest block of a progressive file, so it is passed
    // before the block is found.
    if context.first_pass && *eobrun > 0 {
        *eobrun -= 1;
        return Ok(());
    }
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
        } else if bits.read_bit() == 1 {
            block[0] |= 1 << scan.low;
        }
        if frame.progressive {
            return Ok(());
        }
    }
    let table = context.ac.ok_or_else(|| malformed("no AC table"))?;
    let start = scan.start.max(1);
    // A sequential frame keeps no masks and never refines. A progressive
    // one always has one a block; only its AC passes touch it.
    let nonzero = if frame.progressive {
        Some(
            component
                .nonzero
                .get_mut(offset / 64)
                .ok_or_else(|| malformed("a progressive block without its nonzero mask"))?,
        )
    } else {
        None
    };
    if scan.high == 0 {
        let eobrun = frame.progressive.then_some(eobrun);
        decode_ac_first(
            bits,
            table,
            (block, nonzero),
            (start, scan.end),
            scan.low,
            eobrun,
        )
    } else {
        let nonzero = nonzero.ok_or_else(|| malformed("a refinement in a sequential frame"))?;
        decode_ac_refine(
            bits,
            table,
            (block, nonzero),
            (start, scan.end),
            scan.low,
            eobrun,
        )
    }
}

/// A band's first pass, or a sequential block's AC: T.81 G.1.2.2, and F.2.2.2
/// when `eobrun` is `None`. A sequential scan has no end-of-band runs, so
/// there a zero-size symbol other than sixteen zeros ends the block and
/// reads no run length, as libjpeg and stb_image read it.
fn decode_ac_first(
    bits: &mut Bits<'_>,
    table: &Huffman,
    (block, nonzero): (&mut [i16; 64], Option<&mut u64>),
    band: (usize, usize),
    low: u32,
    eobrun: Option<&mut u32>,
) -> Result<(), ImageError> {
    match nonzero {
        None => ac_first_pass::<false>(bits, table, block, band, low, eobrun).map(|_| ()),
        Some(nonzero) => {
            let (written, set) = ac_first_pass::<true>(bits, table, block, band, low, eobrun)?;
            *nonzero = (*nonzero & !written) | set;
            Ok(())
        }
    }
}

/// [`decode_ac_first`]'s band, for a progressive frame (keeping its mask)
/// or a sequential one (which has none) as `PROGRESSIVE` says, so the
/// sequential pass carries no mask work. Returns the zigzag positions it
/// wrote and those of them now nonzero, gathered in registers and put in
/// the block's mask once.
fn ac_first_pass<const PROGRESSIVE: bool>(
    bits: &mut Bits<'_>,
    table: &Huffman,
    block: &mut [i16; 64],
    (start, end): (usize, usize),
    low: u32,
    mut eobrun: Option<&mut u32>,
) -> Result<(u64, u64), ImageError> {
    let (mut written, mut set) = (0u64, 0u64);
    if let Some(eobrun) = eobrun.as_deref_mut()
        && *eobrun > 0
    {
        *eobrun -= 1;
        return Ok((written, set));
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
        let value = (bits.signed(size) << low) as i16;
        block[ZIGZAG[k]] = value;
        if PROGRESSIVE {
            written |= 1 << k;
            set |= u64::from(value != 0) << k;
        }
        k += 1;
    }
    Ok((written, set))
}

/// A band's refinement pass: T.81 G.1.2.3, in the order libjpeg reads it.
fn decode_ac_refine(
    bits: &mut Bits<'_>,
    table: &Huffman,
    (block, nonzero): (&mut [i16; 64], &mut u64),
    (start, end): (usize, usize),
    low: u32,
    eobrun: &mut u32,
) -> Result<(), ImageError> {
    let plus = 1i16 << low;
    let minus = -1i16 << low;
    // A coefficient already nonzero takes one correction bit wherever the
    // scan passes it.
    let refine = |bits: &mut Bits<'_>, coefficient: &mut i16| {
        if bits.read_bit() == 1 && *coefficient & plus == 0 {
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
                value = if bits.read_bit() == 1 { plus } else { minus };
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
            // A run that passes the band's end, which damaged data and
            // zeros read past it can give, puts the new value just after the
            // band, where libjpeg-turbo's `decode_mcu_AC_refine` puts it: at
            // `jpeg_natural_order[k]`, whose entries past the block's end are
            // its last position.
            if value != 0 {
                let k = k.min(ZIGZAG.len() - 1);
                block[ZIGZAG[k]] = value;
                mark(nonzero, k, value);
            }
            k += 1;
        }
    }
    if *eobrun > 0 {
        // The rest of the band's nonzero coefficients, from the mask: a
        // correction keeps a coefficient nonzero, so the mask holds.
        for k in nonzero_between(*nonzero, k, end) {
            refine(bits, &mut block[ZIGZAG[k]]);
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
    use crate::WorkBound;

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

    /// Damaged files beside `djpeg -ppm`'s pixels for them (libjpeg-turbo
    /// 2.1.5), all 61 × 45, a seeded gradient with noise that cjpeg 2.1.5
    /// wrote at quality 85, then cut short inside a scan's entropy-coded
    /// data with an end-of-image marker put back, or with bytes taken out:
    ///
    /// * `truncated-baseline-420`: a baseline scan cut short, the MCUs after
    ///   the one its data runs out in left grey;
    /// * `truncated-restart-420`: a restart marker after every MCU, cut
    ///   inside one, so the end-of-image marker stands where the next
    ///   restart marker should;
    /// * `truncated-at-restart-420`: the same file cut just before a restart
    ///   marker, so its interval is whole and the next MCU is read from
    ///   zeros alone;
    /// * `truncated-progressive-refine`: progressive with a restart marker
    ///   after every MCU, cut just before one in the last scan, a luma AC
    ///   refinement, whose next block's zeros decode as a run past the
    ///   band's end;
    /// * `shortened-restart-interval`: a restart marker after every three
    ///   MCUs, the second interval's last bytes taken out, so it runs out
    ///   early and the intervals after it decode in full.
    ///
    /// By the last scan of the progressive file every component's
    /// coefficients are coded down to their last bit, so libjpeg-turbo's
    /// block smoothing, which this decoder does not do, is off there.
    const DAMAGED: [(&str, &[u8], &[u8]); 5] = [
        fixture!("truncated-baseline-420"),
        fixture!("truncated-restart-420"),
        fixture!("truncated-at-restart-420"),
        fixture!("truncated-progressive-refine"),
        fixture!("shortened-restart-interval"),
    ];

    /// Data that runs out leaves the rest of its restart interval
    /// undecoded, and a marker standing where a restart marker should
    /// leaves the rest of the scan so, as libjpeg-turbo leaves them.
    #[test]
    fn a_scan_whose_data_runs_out_decodes_to_libjpeg_turbos_pixels() {
        for (name, jpeg, rgb) in DAMAGED {
            let image = decode_stored(jpeg, &Limits::DEFAULT)
                .unwrap_or_else(|error| panic!("{name}: {error}"));
            assert_eq!((image.width(), image.height()), (61, 45), "{name}");
            assert_eq!(image.pixels().len() / 4, rgb.len() / 3, "{name}");
            assert_eq!(first_difference(&image, rgb), None, "{name}");
        }
    }

    /// A sequential block past where the data runs out keeps the zero
    /// coefficients libjpeg-turbo leaves it, so it is flat grey whatever
    /// the DC prediction was: each sequential fixture of more than one MCU,
    /// cut a quarter of the way into its scan, ends in mid grey.
    #[test]
    fn sequential_blocks_past_the_data_are_grey() {
        let mut checked = 0;
        for (name, jpeg, _) in FIXTURES {
            let Some(sos) = jpeg.windows(2).position(|pair| pair == [0xff, 0xda]) else {
                continue;
            };
            let (width, height) = dimensions(jpeg).expect("a frame");
            if jpeg.windows(2).any(|pair| pair == [0xff, 0xc2]) || width.max(height) <= 16 {
                continue;
            }
            let header = usize::from(u16::from_be_bytes([jpeg[sos + 2], jpeg[sos + 3]]));
            let start = sos + 2 + header;
            let mut cut = jpeg[..start + (jpeg.len() - start) / 4].to_vec();
            cut.extend_from_slice(&[0xff, 0xd9]);
            let image = decode_stored(&cut, &Limits::DEFAULT)
                .unwrap_or_else(|error| panic!("{name}: {error}"));
            let last = image.pixels().len() - 4;
            assert_eq!(&image.pixels()[last..], [128, 128, 128, 255], "{name}");
            checked += 1;
        }
        assert!(checked > 5, "{checked} checked");
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
        // Sequential at 128 bytes a block, progressive at 136 with its
        // nonzero mask.
        for (progressive, components, smallest) in [
            (false, 1, 3072),
            (false, 3, 1152),
            (false, 4, 912),
            (true, 1, 3264),
            (true, 3, 1224),
            (true, 4, 969),
        ] {
            let blocks = 16 + (components - 1);
            let block_bytes = if progressive { 136 } else { 128 };
            assert_eq!(
                blocks * block_bytes,
                smallest * 2 * components / 3,
                "{components}"
            );
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
                read_frame(&segment, progressive, &limits(smallest)).is_ok(),
                "{components} just inside"
            );
            assert!(
                matches!(
                    read_frame(&segment, progressive, &limits(smallest - 1)),
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

    /// FNV-1a over `bytes`: how the oracle harness and the generator script
    /// fingerprint a file or its pixels.
    fn fnv(bytes: impl IntoIterator<Item = u8>) -> u64 {
        bytes.into_iter().fold(0xcbf2_9ce4_8422_2325, |h, byte| {
            (h ^ u64::from(byte)).wrapping_mul(0x0100_0000_01b3)
        })
    }

    /// The work-bound test files, built as the review's generators and the
    /// scratch `gen.py` that checked them against stb_image build them;
    /// each test pins its file's FNV-1a to that script's, so the oracle
    /// results quoted are for these bytes.
    mod work_files {
        use crate::jpeg::work::BLOCKS_PER_ENTROPY_BYTE;

        fn segment(marker: u8, body: &[u8]) -> Vec<u8> {
            let mut bytes = vec![0xff, marker];
            bytes.extend_from_slice(&(body.len() as u16 + 2).to_be_bytes());
            bytes.extend_from_slice(body);
            bytes
        }

        /// The start of a frame `side` square of `components` sampled
        /// 1 × 1, every quantiser 1: sequential or progressive by `marker`.
        fn head(side: u16, components: u8, marker: u8) -> Vec<u8> {
            let mut bytes = vec![0xff, 0xd8];
            bytes.extend(segment(0xdb, &[[0].as_slice(), &[1; 64]].concat()));
            let [high, low] = side.to_be_bytes();
            let mut frame = vec![8, high, low, high, low, components];
            for c in 1..=components {
                frame.extend_from_slice(&[c, 0x11, 0]);
            }
            bytes.extend(segment(marker, &frame));
            // DC table 0: the one-bit code 0 is a difference of size 0.
            bytes.extend(segment(
                0xc4,
                &[[0x00, 1].as_slice(), &[0; 15], &[0]].concat(),
            ));
            bytes
        }

        /// AC table 0 of one symbol, its code one bit, or sixteen to force
        /// the slow lookup.
        fn ac_table(symbol: u8, slow: bool) -> Vec<u8> {
            let mut counts = [0u8; 16];
            counts[if slow { 15 } else { 0 }] = 1;
            segment(0xc4, &[[0x10].as_slice(), &counts, &[symbol]].concat())
        }

        /// `scans` interleaved progressive DC scans of three components
        /// with no data.
        pub(super) fn empty_scans(side: u16, scans: usize) -> Vec<u8> {
            let mut bytes = head(side, 3, 0xc2);
            for _ in 0..scans {
                bytes.extend(segment(0xda, &[3, 1, 0, 2, 0, 3, 0, 0, 0, 0]));
            }
            bytes.extend_from_slice(&[0xff, 0xd9]);
            bytes
        }

        /// [`empty_scans`], but with a restart interval of `interval` MCUs
        /// and each scan's data its restart markers alone: every interval
        /// empty, so each is walked undecoded past its first MCU.
        pub(super) fn restart_scans(side: u16, scans: usize, interval: u16) -> Vec<u8> {
            let mcus = (usize::from(side) / 8).pow(2);
            let mut bytes = head(side, 3, 0xc2);
            bytes.extend(segment(0xdd, &interval.to_be_bytes()));
            for _ in 0..scans {
                bytes.extend(segment(0xda, &[3, 1, 0, 2, 0, 3, 0, 0, 0, 0]));
                for n in 1..mcus / usize::from(interval) {
                    bytes.extend_from_slice(&[0xff, 0xd0 + (n - 1) as u8 % 8]);
                }
            }
            bytes.extend_from_slice(&[0xff, 0xd9]);
            bytes
        }

        /// Grey: one DC scan of real data, a bit a block, then `scans` AC
        /// first scans of a byte for every [`BLOCKS_PER_ENTROPY_BYTE`]
        /// blocks and one more, whose zero bits past it decode as (run 0,
        /// size 1), 63 coefficients a block.
        pub(super) fn ac_scans(side: u16, scans: usize, slow: bool) -> Vec<u8> {
            let blocks = (usize::from(side) / 8).pow(2);
            let mut bytes = head(side, 1, 0xc2);
            bytes.extend(ac_table(0x01, slow));
            bytes.extend(segment(0xda, &[1, 1, 0, 0, 0, 0]));
            bytes.extend(vec![0; blocks.div_ceil(8)]);
            for _ in 0..scans {
                bytes.extend(segment(0xda, &[1, 1, 0, 1, 63, 0]));
                bytes.extend(vec![0; blocks / BLOCKS_PER_ENTROPY_BYTE as usize + 1]);
            }
            bytes.extend_from_slice(&[0xff, 0xd9]);
            bytes
        }

        /// Grey: a DC scan of one real byte, the rest read past it, then a
        /// restart interval of one MCU row and `scans` AC scans of one byte
        /// whose zero bits are an end of band. stb_image stops each AC scan
        /// at its first interval, which has no restart marker after it.
        pub(super) fn walked_not_whole(side: u16, scans: usize) -> Vec<u8> {
            let mut bytes = head(side, 1, 0xc2);
            bytes.extend(ac_table(0x00, false));
            bytes.extend(segment(0xda, &[1, 1, 0, 0, 0, 0]));
            bytes.push(0);
            bytes.extend(segment(0xdd, &(side / 8).to_be_bytes()));
            for _ in 0..scans {
                bytes.extend(segment(0xda, &[1, 1, 0, 1, 63, 0]));
                bytes.push(0);
            }
            bytes.extend_from_slice(&[0xff, 0xd9]);
            bytes
        }
    }

    /// The scans of the review's hostile files, at sizes a test can walk;
    /// the 8192 × 8192 versions were timed outside the tests. In stb_image
    /// a file of 100 empty scans walks its frame once from padding, then is
    /// refused at the first block of its second scan. The default decoder
    /// reads the first MCU of each scan from zeros, its three blocks a read
    /// each, and stops there, as libjpeg-turbo does: it decodes the file to
    /// the flat grey `djpeg` 2.1.5 gives.
    #[test]
    fn empty_scans_walk_the_frame_in_stb_image_and_an_mcu_each_here() {
        let bytes = work_files::empty_scans(1024, 100);
        assert_eq!(fnv(bytes.iter().copied()), 0xffac_78c9_3b56_cb07);
        let frame_blocks = 3 * 128 * 128;
        let [(_, stored, libjpeg), (_, stb_result, stb)] = decode_both(&bytes);
        assert!(
            matches!(
                stb_result,
                Err(ImageError::TooMuchWork {
                    bound: WorkBound::BlocksWalked { entropy_bytes: 0 },
                    ..
                })
            ),
            "{stb_result:?}"
        );
        assert_eq!((stb.scans, stb.walked), (2, frame_blocks + 1));
        let image = stored.expect("decodes");
        assert!(
            image
                .pixels()
                .chunks(4)
                .all(|pixel| pixel == [128, 128, 128, 255])
        );
        assert_eq!(
            (libjpeg.scans, libjpeg.walked, libjpeg.padded_reads),
            (100, 100 * 3, 100 * 3)
        );
    }

    /// Blocks walked undecoded to a restart marker are charged: a file of
    /// progressive DC scans whose data is nothing but their restart markers
    /// reads the first MCU of each interval from zeros and walks the rest of
    /// it, a whole frame a scan bar its last interval, against the bytes the
    /// markers bring. It is refused in its third scan.
    #[test]
    fn blocks_walked_undecoded_to_a_restart_are_charged() {
        let bytes = work_files::restart_scans(1024, 100, 1024);
        let mut work = Work::default();
        let result = decode_stored_with(&bytes, &Limits::DEFAULT, &mut work);
        assert!(
            matches!(
                result,
                Err(ImageError::TooMuchWork {
                    bound: WorkBound::BlocksWalked { entropy_bytes: 90 },
                    ..
                })
            ),
            "{result:?}"
        );
        assert_eq!(work.scans, 3);
    }

    /// A marker below `0xc0` where a restart marker should be is refused,
    /// whether the reader meets it early in an interval, with MCUs of the
    /// interval still to walk, or at its end: only a marker that
    /// [`ends_scan`] ends one early. The file has a restart marker after
    /// every three MCUs.
    #[test]
    fn a_marker_below_0xc0_in_a_restart_interval_is_refused() {
        let (_, jpeg, _) = DAMAGED
            .into_iter()
            .find(|(name, ..)| *name == "shortened-restart-interval")
            .expect("the fixture");
        let restarts: Vec<usize> = jpeg
            .windows(2)
            .enumerate()
            .filter_map(|(at, pair)| (pair[0] == 0xff && pair[1] & 0xf8 == 0xd0).then_some(at))
            .collect();
        // Two bytes into the second interval, so its first MCU runs into it.
        let early = restarts[0] + 4;
        let inside = [&jpeg[..early], &[0xff, 0x4a], &jpeg[early..]].concat();
        let mut instead = jpeg.to_vec();
        instead[restarts[1] + 1] = 0x4a;
        for (case, bytes) in [("inside", inside), ("instead", instead)] {
            assert_eq!(
                decode_stored(&bytes, &Limits::DEFAULT),
                Err(malformed("a restart interval ends without its marker")),
                "{case}"
            );
        }
    }

    /// The review's `ac2` and `slow2`: AC scans whose bits past their data,
    /// a byte for every [`work::BLOCKS_PER_ENTROPY_BYTE`] blocks and one
    /// more, decode as 63 coefficients a block, by a one-bit code and by a
    /// sixteen-bit one, each with a magnitude bit. stb_image reads every
    /// block of each scan so. The blocks walked stay within the bytes; the
    /// codes read from made-up bits do not, and refuse its third AC scan
    /// (the fourth scan): 63 codes a block each, against
    /// [`work::PADDED_READS_PER_BLOCK`] a block for the frame. The same for
    /// either code length.
    ///
    /// The default decoder reads only the first block of each AC scan, the
    /// one its data runs out in, and stops there, as libjpeg-turbo does: the
    /// one-bit codes take eight coefficients from the data and make up the
    /// other 55 reads, the sixteen-bit ones make up all 63. It decodes the
    /// file to the pixels `djpeg` 2.1.5 gives.
    #[test]
    fn coefficients_decoded_past_the_data_are_bounded() {
        for (slow, file_fnv, made_up) in [
            (false, 0xaf5f_ed8d_96ae_238d, 55),
            (true, 0x17f4_d805_c5f0_4d1f, 63),
        ] {
            let bytes = work_files::ac_scans(256, 99, slow);
            assert_eq!(fnv(bytes.iter().copied()), file_fnv, "slow {slow}");
            let [(_, stored, libjpeg), (_, stb_result, stb)] = decode_both(&bytes);
            assert!(
                matches!(
                    stb_result,
                    Err(ImageError::TooMuchWork {
                        bound: WorkBound::MadeUpReads { .. },
                        ..
                    })
                ),
                "slow {slow}: {stb_result:?}"
            );
            // Refused in the course of the fourth scan, not at its end: the
            // three before it are charged their blocks, it is not.
            assert_eq!((stb.scans, stb.walked), (4, 3 * 1024), "slow {slow}");
            assert!(stb.padded_reads > work::PADDED_READS_PER_BLOCK * 1024);
            let image = stored.unwrap_or_else(|error| panic!("slow {slow}: {error}"));
            let rgb = image.pixels().chunks(4).flat_map(|p| [p[0], p[1], p[2]]);
            assert_eq!(fnv(rgb), 0xe9f9_5dfb_8d63_9502, "slow {slow}");
            assert_eq!(
                (libjpeg.scans, libjpeg.walked, libjpeg.padded_reads),
                (100, 1024 + 99, 99 * made_up),
                "slow {slow}"
            );
        }
    }

    /// Blocks are charged as they are walked, not a whole scan up front:
    /// stb_image stops each AC scan of this file after one MCU row, so it
    /// walks 2624 blocks against an allowance of 1024 plus 800 for each of
    /// its 51 bytes, where whole scans would be 52224. The three builds of
    /// the oracle harness that fill stb_image's buffers with 0x00, 0x37 and
    /// 0xc8 all gave the pixels pinned here, so the file is determined. The
    /// default decoder gives the same pixels, as `djpeg` 2.1.5 does: each
    /// scan's data runs out in its ninth block, which it reads from zeros,
    /// and the marker where the first restart marker should be ends the
    /// scan there. With one scan more than [`MAX_SCANS`] stb_image refuses
    /// it for that.
    #[test]
    fn blocks_are_charged_as_they_are_walked() {
        for (scans, file_fnv) in [(50, 0x510a_5519_bb0c_f421), (99, 0x8b8d_9045_f50e_185a)] {
            let bytes = work_files::walked_not_whole(256, scans);
            assert_eq!(fnv(bytes.iter().copied()), file_fnv, "{scans}");
            let mut work = Work::default();
            let image = stb::decode(&bytes, &Limits::DEFAULT, &mut work).expect("decodes");
            let rgb = image.pixels().chunks(4).flat_map(|p| [p[0], p[1], p[2]]);
            assert_eq!(fnv(rgb), 0x141c_0b68_88de_2325, "{scans}");
            assert_eq!(work.walked, 1024 + 32 * scans as u64, "{scans}");
            let mut work = Work::default();
            let stored = decode_stored_with(&bytes, &Limits::DEFAULT, &mut work);
            assert_eq!(stored, Ok(image), "{scans}");
            assert_eq!(work.walked, 9 * (1 + scans as u64), "{scans}");
        }
        let bytes = work_files::walked_not_whole(256, 100);
        assert_eq!(fnv(bytes.iter().copied()), 0x70e6_a7f6_d6ad_8d4f);
        assert!(matches!(
            decode_as_stb_image(&bytes, &Limits::DEFAULT),
            Err(ImageError::TooMuchWork {
                bound: WorkBound::Scans { scans: 101 },
                ..
            })
        ));
    }

    /// A damaged file costs the default decoder no more than stb_image:
    /// each progressive fixture cut short in each of its scans, with an
    /// end-of-image marker put back, as Pillow's `LOAD_TRUNCATED_IMAGES`
    /// puts it back. Both count the same scans and bytes; stb_image reads
    /// the rest of a scan from zeros, where the default decoder reads only
    /// the rest of the MCU its data runs out in. Neither undercharges: the
    /// scans before the cut are charged whole in both, as the file cut at
    /// that scan's marker shows, and the cut scan at least its first block.
    #[test]
    fn a_damaged_file_costs_no_more_than_in_stb_image() {
        let mut compared = 0;
        let mut made_up = 0;
        let mut cheaper = 0;
        for (name, jpeg, _) in FIXTURES {
            if !name.starts_with("progressive") {
                continue;
            }
            let scans: Vec<usize> = jpeg
                .windows(2)
                .enumerate()
                .filter_map(|(at, pair)| (pair == [0xff, 0xda]).then_some(at))
                .collect();
            for &scan in &scans {
                let mut cut = jpeg[..scan + 20].to_vec();
                cut.extend_from_slice(&[0xff, 0xd9]);
                let [(_, stored, libjpeg), (_, stb_result, stb)] = decode_both(&cut);
                if stored.is_err() || stb_result.is_err() {
                    continue;
                }
                assert_eq!(
                    (libjpeg.scans, libjpeg.entropy_bytes),
                    (stb.scans, stb.entropy_bytes),
                    "{name} cut at {scan}"
                );
                assert!(
                    libjpeg.walked <= stb.walked && libjpeg.padded_reads <= stb.padded_reads,
                    "{name} cut at {scan}: {libjpeg:?} against {stb:?}"
                );
                let mut head = jpeg[..scan].to_vec();
                head.extend_from_slice(&[0xff, 0xd9]);
                let [(_, _, before), (_, _, stb_before)] = decode_both(&head);
                assert_eq!(before.walked, stb_before.walked, "{name} before {scan}");
                assert!(
                    libjpeg.walked > before.walked,
                    "{name} cut at {scan}: {libjpeg:?} after {before:?}"
                );
                compared += 1;
                made_up += u64::from(libjpeg.padded_reads > 0);
                cheaper += u64::from(libjpeg.padded_reads < stb.padded_reads);
            }
        }
        assert!(
            compared > 0 && made_up > 0 && cheaper > 0,
            "{compared} compared, {made_up} made up, {cheaper} cheaper"
        );
    }

    /// Both decoders, with their work.
    fn decode_both(bytes: &[u8]) -> [(&str, Result<Rgba8Image, ImageError>, Work); 2] {
        let mut libjpeg = Work::default();
        let stored = decode_stored_with(bytes, &Limits::DEFAULT, &mut libjpeg);
        let mut stb = Work::default();
        let stb_result = stb::decode(bytes, &Limits::DEFAULT, &mut stb);
        [("decode_stored", stored, libjpeg), ("stb", stb_result, stb)]
    }

    /// Both decoders charge real files alike: an interleaved 4:2:0
    /// baseline scan walks six blocks an MCU and brings its data from the
    /// header to the end-of-image marker, with nothing made up; a
    /// progressive file walks more than one pass, alike in both.
    #[test]
    fn both_decoders_charge_a_real_file_alike() {
        let (_, baseline, _) = FIXTURES[2];
        let (width, height) = dimensions(baseline).expect("a frame");
        let mcus = u64::from(width.div_ceil(16)) * u64::from(height.div_ceil(16));
        let sos = baseline
            .windows(2)
            .position(|pair| pair == [0xff, 0xda])
            .expect("a scan");
        let header = usize::from(u16::from_be_bytes([baseline[sos + 2], baseline[sos + 3]]));
        let data = (baseline.len() - 2 - (sos + 2 + header)) as u64;
        for (name, result, work) in decode_both(baseline) {
            assert!(result.is_ok(), "{name}");
            let counted = (
                work.scans,
                work.walked,
                work.entropy_bytes,
                work.padded_reads,
            );
            assert_eq!(counted, (1, mcus * 6, data, 0), "{name}");
        }
        let (_, progressive, _) = FIXTURES[6];
        let [(_, _, libjpeg), (_, _, stb)] = decode_both(progressive);
        let (width, height) = dimensions(progressive).expect("a frame");
        let one_pass = u64::from(width.div_ceil(16)) * u64::from(height.div_ceil(16)) * 6;
        assert!(libjpeg.walked > one_pass);
        assert_eq!(
            (
                libjpeg.scans,
                libjpeg.walked,
                libjpeg.entropy_bytes,
                libjpeg.padded_reads
            ),
            (stb.scans, stb.walked, stb.entropy_bytes, stb.padded_reads)
        );
    }
}
