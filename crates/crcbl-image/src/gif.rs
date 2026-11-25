//! GIF, written here: the first image of a GIF87a or GIF89a file, decoded to
//! RGBA8 on its logical screen.
//!
//! Section numbers below are the GIF89a specification's (CompuServe, 1990),
//! which GIF87a files are a subset of.
//!
//! # What it reads
//!
//! The header (§17) and logical screen descriptor (§18), the global colour
//! table (§19), extensions walked block by block through their data
//! sub-blocks (§15, §16), the graphic control extension's transparent index
//! (§23), and the first image descriptor (§20), its local colour table
//! (§21) and its LZW-coded data (§22, appendix F), rows in order or
//! interlaced (appendix E). Everything after the first image is ignored:
//! further frames, their extensions and the trailer (§27). Delays, disposal,
//! the background colour and the pixel aspect ratio are not read.
//!
//! The image is the logical screen's size, as stb_image (llama.cpp's
//! loader, `llama.cpp/vendor/stb/stb_image.h`), Pillow and browsers all
//! return it. The first image is placed at its offset on that screen, which
//! it must fit inside (§20: "Each image must fit within the boundaries of
//! the Logical Screen"); one that does not is [`ImageError::Malformed`], as
//! stb_image refuses it, where Pillow grows the screen to hold it.
//!
//! # Transparency
//!
//! The screen outside the first image is transparent black, as browsers
//! draw a GIF's canvas, and as stb_image leaves it when the background
//! colour index is 0, the index every common encoder writes. stb_image
//! paints a nonzero background index's colour there instead, opaque and
//! with red and blue swapped; Pillow and ImageMagick paint colour 0 (or
//! the transparent index's colour, transparent). The background colour is
//! a hint, so this decoder ignores it.
//!
//! A pixel whose index is the graphic control extension's transparent
//! index, or past the end of its colour table, is transparent black, as
//! stb_image decodes both (Pillow keeps the transparent entry's colour, and
//! paints an index past the table opaque black). Only the last graphic
//! control extension before the first image counts, and a plain text
//! extension between them takes it (§23: "The scope of this Extension is
//! the graphic rendering block that follows it").
//!
//! # What it refuses
//!
//! A file with no colour table at all is [`ImageError::Unsupported`]: §11
//! leaves the decoder to pick a table of its own, Pillow picks greys, and
//! stb_image refuses it. An LZW minimum code size outside 2..=8 is
//! [`ImageError::Malformed`]: it is a colour index's bits, and appendix F
//! gives one-bit images a code size of 2, so 1 is refused (Pillow decodes
//! it), as is anything past 8 (stb_image takes up to 12). A code used
//! before the table defines it is [`ImageError::Malformed`], as stb_image
//! refuses it. So is a file that ends before its first image's LZW code
//! size byte.
//!
//! # Data that ends early
//!
//! The first image's data may end before its last pixel — at an end code
//! (one straight after a clear code included), a block terminator, or the
//! end of the file, inside a sub-block or between two. The pixels decoded
//! by then are kept and the rest of the image is transparent black, as
//! browsers draw what arrived. Against stb_image:
//!
//! - With the background colour index 0, stb_image leaves the missing
//!   pixels transparent black too, except where a sub-block's length
//!   promised bytes the file did not hold: it reads those as zeros, so
//!   their codes paint colour 0, opaque, where this decoder stays
//!   transparent.
//! - With the index above 0, stb_image paints every missing pixel that
//!   colour, opaque and with red and blue swapped; this decoder leaves them
//!   transparent black.
//!
//! So once llama.cpp drops alpha the two agree wherever the colour
//! stb_image paints is black. Pillow refuses the file unless told to load
//! truncated images, and then paints colour 0. Once the last pixel is
//! decoded nothing further is read, so a missing end code, block terminator
//! or trailer after it does no harm.
//!
//! # The code stream
//!
//! The data need not start with a clear code (appendix F: encoders
//! "should" write one; stb_image refuses a stream without it, Pillow does
//! not). After the code table's 4096 entries are full the codes go on at 12
//! bits and add nothing until a clear code, however long that takes (stb_image
//! refuses a stream more than 4096 codes past a full table).
//!
//! # Hostile input
//!
//! The headers are walked without allocating; the logical screen passes
//! the caller's [`Limits`] before any buffer is sized; the first image's
//! colour indices are decoded into a buffer of at most its pixel count,
//! never written past; and each code costs at most one table entry's
//! length, so the work is linear in the input and the output. Peak memory
//! is the screen's RGBA plus that index buffer, one byte a pixel of an image
//! no larger than the screen: at most a quarter more than the RGBA the
//! limits allow.

use crate::{ImageError, Limits, Rgba8Image};

/// The widest LZW code, and so the size of the code table.
const MAX_CODE_BITS: u32 = 12;
const TABLE_SIZE: usize = 1 << MAX_CODE_BITS;

/// The narrowest and widest LZW minimum code size: a colour index's bits.
const MIN_CODE_SIZES: core::ops::RangeInclusive<u8> = 2..=8;

/// Block introducers (§20, §27, and the extension introducer of §23–§26).
const IMAGE: u8 = 0x2c;
const EXTENSION: u8 = 0x21;
const TRAILER: u8 = 0x3b;

/// Extension labels (§23, §25).
const GRAPHIC_CONTROL: u8 = 0xf9;
const PLAIN_TEXT: u8 = 0x01;

/// The packed fields' flags: a colour table follows (§18, §20), the image is
/// interlaced (§20), and the transparent index is set (§23).
const COLOUR_TABLE: u8 = 0x80;
const INTERLACED: u8 = 0x40;
const TRANSPARENT: u8 = 0x01;

/// Appendix E's interlace passes: each the first row and the step.
const INTERLACE_PASSES: [(usize, usize); 4] = [(0, 8), (4, 8), (2, 4), (1, 2)];

fn malformed(why: impl Into<String>) -> ImageError {
    ImageError::Malformed(why.into())
}

/// Decodes the first image of a GIF file to straight RGBA8, on its logical
/// screen as the [module docs](self) say.
///
/// # Errors
///
/// [`ImageError::Empty`] or [`ImageError::TooLarge`] if the logical
/// screen's size breaks `limits`, [`ImageError::Unsupported`] for a file
/// with no colour table, and [`ImageError::Malformed`] for anything that is
/// not a well-formed GIF up to its first image's data, or a code in that
/// data used before it is defined.
pub fn decode(bytes: &[u8], limits: &Limits) -> Result<Rgba8Image, ImageError> {
    let frame = first_frame(bytes)?;
    let (width, height) = frame.screen;
    let mut pixels = vec![0; limits.rgba_bytes(width, height)?];
    let indices = lzw(frame.data, frame.min_code_size, frame.width * frame.height)?;
    let colours = frame.colours();
    let rows: Vec<usize> = if frame.interlaced {
        INTERLACE_PASSES
            .iter()
            .flat_map(|&(first, step)| (first..frame.height).step_by(step))
            .collect()
    } else {
        (0..frame.height).collect()
    };
    if frame.width > 0 {
        let stride = width as usize * 4;
        // Data that ended early leaves the last rows short or missing.
        for (row_indices, row) in indices.chunks(frame.width).zip(rows) {
            let at = (frame.y + row) * stride + frame.x * 4;
            let (row_pixels, _) = pixels[at..at + frame.width * 4].as_chunks_mut::<4>();
            for (pixel, &index) in row_pixels.iter_mut().zip(row_indices) {
                *pixel = colours[usize::from(index)];
            }
        }
    }
    Ok(Rgba8Image::from_sized(width, height, pixels))
}

/// The width and height of a GIF's logical screen, the size [`decode`]
/// returns, read without decoding a pixel.
///
/// The blocks up to the first image's data are walked and checked as
/// [`decode`] walks them, so a file it refuses there is refused here the
/// same way: no colour table, the first image past the screen, a bad LZW
/// minimum code size, an unknown block, or a file cut short before the
/// data. The data itself is not read, so a code used before it is defined
/// is found only by decoding. The size is not checked against any [`Limits`]: compare it
/// with the caller's, or ask [`Limits::rgba_bytes`].
///
/// # Errors
///
/// [`ImageError::Empty`] for a zero side, [`ImageError::Unsupported`] for a
/// file with no colour table, and [`ImageError::Malformed`] otherwise.
pub fn dimensions(bytes: &[u8]) -> Result<(u32, u32), ImageError> {
    first_frame(bytes).map(|frame| frame.screen)
}

/// The first image, as the blocks before its data describe it.
struct Frame<'a> {
    /// The logical screen's width and height.
    screen: (u32, u32),
    /// The image's place and size on the screen, inside it.
    x: usize,
    y: usize,
    width: usize,
    height: usize,
    interlaced: bool,
    /// The colour table it uses, three bytes an entry.
    table: &'a [u8],
    transparent: Option<u8>,
    min_code_size: u8,
    /// From the first data sub-block's length to the end of the file.
    data: &'a [u8],
}

impl Frame<'_> {
    /// Each colour index's RGBA: the table's colours opaque, the transparent
    /// index and every index past the table transparent black.
    fn colours(&self) -> [[u8; 4]; 256] {
        let mut colours = [[0; 4]; 256];
        for (colour, rgb) in colours.iter_mut().zip(self.table.as_chunks::<3>().0) {
            *colour = [rgb[0], rgb[1], rgb[2], u8::MAX];
        }
        if let Some(index) = self.transparent {
            colours[usize::from(index)] = [0; 4];
        }
        colours
    }
}

/// Reads that fail rather than panic, every failure the file cut short.
struct Cursor<'a> {
    bytes: &'a [u8],
    at: usize,
}

impl<'a> Cursor<'a> {
    fn take(&mut self, count: usize) -> Result<&'a [u8], ImageError> {
        let taken = self
            .bytes
            .get(self.at..self.at + count)
            .ok_or_else(|| malformed("a GIF is cut short before its first image's data"))?;
        self.at += count;
        Ok(taken)
    }

    fn byte(&mut self) -> Result<u8, ImageError> {
        self.take(1).map(|byte| byte[0])
    }

    fn u16(&mut self) -> Result<u16, ImageError> {
        self.take(2).map(|b| u16::from_le_bytes([b[0], b[1]]))
    }

    /// A colour table, if `flags` says one follows: three bytes for each of
    /// `2 << (flags & 7)` entries.
    fn colour_table(&mut self, flags: u8) -> Result<Option<&'a [u8]>, ImageError> {
        if flags & COLOUR_TABLE == 0 {
            return Ok(None);
        }
        self.take(3 << ((flags & 7) + 1)).map(Some)
    }

    /// Steps over data sub-blocks through their block terminator.
    fn skip_sub_blocks(&mut self) -> Result<(), ImageError> {
        loop {
            match self.byte()? {
                0 => return Ok(()),
                length => {
                    self.take(usize::from(length))?;
                }
            }
        }
    }
}

/// Walks the blocks before the first image's data.
fn first_frame(bytes: &[u8]) -> Result<Frame<'_>, ImageError> {
    if !matches!(bytes.get(..6), Some(b"GIF87a" | b"GIF89a")) {
        return Err(malformed("no GIF87a or GIF89a signature"));
    }
    let mut cursor = Cursor { bytes, at: 6 };
    let screen = crate::non_empty(cursor.u16()?.into(), cursor.u16()?.into())?;
    let flags = cursor.byte()?;
    // The background colour index and the pixel aspect ratio.
    cursor.take(2)?;
    let global = cursor.colour_table(flags)?;
    let mut transparent = None;
    loop {
        match cursor.byte()? {
            EXTENSION => {
                let label = cursor.byte()?;
                if label == GRAPHIC_CONTROL {
                    let size = cursor.byte()?;
                    let fields = cursor.take(usize::from(size))?;
                    // Packed fields, delay, transparent index; a block of
                    // any other size says nothing this decoder can trust.
                    transparent = match fields {
                        &[packed, _, _, index] if packed & TRANSPARENT != 0 => Some(index),
                        _ => None,
                    };
                } else if label == PLAIN_TEXT {
                    transparent = None;
                }
                cursor.skip_sub_blocks()?;
            }
            IMAGE => break,
            TRAILER => return Err(malformed("a GIF with no image")),
            other => return Err(malformed(format!("an unknown GIF block 0x{other:02x}"))),
        }
    }
    let (x, y) = (cursor.u16()?, cursor.u16()?);
    let (width, height) = (cursor.u16()?, cursor.u16()?);
    if u32::from(x) + u32::from(width) > screen.0 || u32::from(y) + u32::from(height) > screen.1 {
        return Err(malformed("a GIF's first image is past its logical screen"));
    }
    let flags = cursor.byte()?;
    let local = cursor.colour_table(flags)?;
    let table = local
        .or(global)
        .ok_or_else(|| ImageError::Unsupported("a GIF with no colour table".into()))?;
    let min_code_size = cursor.byte()?;
    if !MIN_CODE_SIZES.contains(&min_code_size) {
        return Err(malformed(format!(
            "a GIF LZW minimum code size of {min_code_size}"
        )));
    }
    Ok(Frame {
        screen,
        x: x.into(),
        y: y.into(),
        width: width.into(),
        height: height.into(),
        interlaced: flags & INTERLACED != 0,
        table,
        transparent,
        min_code_size,
        data: &bytes[cursor.at..],
    })
}

/// The codes of LZW data in sub-blocks, least significant bit first.
struct Codes<'a> {
    data: &'a [u8],
    at: usize,
    /// Bytes left in the current sub-block.
    left: usize,
    bits: u32,
    count: u32,
}

impl Codes<'_> {
    /// The next `size`-bit code, or `None` once the data ends: at a block
    /// terminator or the end of the file.
    fn read(&mut self, size: u32) -> Option<u16> {
        while self.count < size {
            if self.left == 0 {
                self.left = usize::from(*self.data.get(self.at)?);
                self.at += 1;
                if self.left == 0 {
                    return None;
                }
            }
            self.bits |= u32::from(*self.data.get(self.at)?) << self.count;
            self.at += 1;
            self.left -= 1;
            self.count += 8;
        }
        let code = self.bits & ((1 << size) - 1);
        self.bits >>= size;
        self.count -= size;
        Some(code as u16)
    }
}

/// The LZW code table: each entry a string, held as its last index, the
/// entry before it, its first index and its length.
struct Table {
    prefix: [u16; TABLE_SIZE],
    suffix: [u8; TABLE_SIZE],
    first: [u8; TABLE_SIZE],
    length: [u16; TABLE_SIZE],
}

impl Table {
    /// Appends entry `code`'s string to `out`, cut off at `count` indices.
    fn emit(&self, code: u16, out: &mut Vec<u8>, count: usize) {
        let length = usize::from(self.length[usize::from(code)]);
        let start = out.len();
        let kept = length.min(count - start);
        out.resize(start + kept, 0);
        // The string is linked last index first, so it is written backwards.
        let mut code = usize::from(code);
        for i in (0..length).rev() {
            if i < kept {
                out[start + i] = self.suffix[code];
            }
            code = usize::from(self.prefix[code]);
        }
    }
}

/// The first `count` colour indices of an image's LZW data (appendix F),
/// starting at its first sub-block's length, or fewer if the data ends
/// before them.
fn lzw(data: &[u8], min_code_size: u8, count: usize) -> Result<Vec<u8>, ImageError> {
    let mut out = Vec::with_capacity(count);
    let clear = 1u16 << min_code_size;
    let end = clear + 1;
    let initial_size = u32::from(min_code_size) + 1;
    let mut table = Table {
        prefix: [0; TABLE_SIZE],
        suffix: [0; TABLE_SIZE],
        first: [0; TABLE_SIZE],
        length: [1; TABLE_SIZE],
    };
    for root in 0..usize::from(clear) {
        table.suffix[root] = root as u8;
        table.first[root] = root as u8;
    }
    let mut codes = Codes {
        data,
        at: 0,
        left: 0,
        bits: 0,
        count: 0,
    };
    let mut size = initial_size;
    let mut next = end + 1;
    let mut previous: Option<u16> = None;
    while out.len() < count {
        let Some(code) = codes.read(size) else {
            break;
        };
        if code == clear {
            size = initial_size;
            next = end + 1;
            previous = None;
            continue;
        }
        if code == end {
            break;
        }
        let undefined = match previous {
            None => code > clear,
            Some(_) => code > next,
        };
        if undefined {
            return Err(malformed("a GIF LZW code used before it is defined"));
        }
        // A full table takes no more entries until a clear code.
        if let Some(previous) = previous
            && usize::from(next) < TABLE_SIZE
        {
            // `code == next` is the entry being made: the previous string
            // and its own first index.
            let last = if code == next {
                table.first[usize::from(previous)]
            } else {
                table.first[usize::from(code)]
            };
            let at = usize::from(next);
            table.prefix[at] = previous;
            table.suffix[at] = last;
            table.first[at] = table.first[usize::from(previous)];
            table.length[at] = table.length[usize::from(previous)] + 1;
            next += 1;
            if u32::from(next) == 1 << size && size < MAX_CODE_BITS {
                size += 1;
            }
        }
        table.emit(code, &mut out, count);
        previous = Some(code);
    }
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Each fixture beside its expected RGBA.
    ///
    /// Provenance, since the generator is not committed: every file was
    /// written by a hand-rolled GIF89a writer (Python, every block and
    /// field set by hand, its own LZW encoder) from palettes and pixels
    /// drawn from a seeded random generator, alongside the RGBA each was
    /// meant to decode to. Each `.rgba` is stb_image's decode of the file
    /// (as vendored in llama.cpp at 57291f2, `stbi_load_from_memory` with
    /// four channels), which equalled the intended pixels on every file.
    /// Pillow 13 and ImageMagick 7.1.2 decoded each the same, except where
    /// the module docs say they differ: `offset.gif`'s screen outside the
    /// image (colour 0, opaque) and `transparent.gif`'s transparent pixels
    /// (the entry's colour under zero alpha).
    ///
    /// `global.gif` has an 8-bit code size and a 256-entry table;
    /// `local.gif` a local table over a global one, after a comment;
    /// `transparent.gif` a transparent index; `interlaced.gif` 13
    /// interlaced rows on a local table alone; `offset.gif` a 7×5 image at
    /// (6, 4) on a 20×16 screen; `animated.gif` two frames;
    /// `two-colour.gif` a code size of 2; and `full-table.gif` noise that
    /// fills the code table and goes on for thousands of codes at 12 bits
    /// before its end code, with no clear.
    macro_rules! fixture {
        ($name:literal) => {
            (
                $name,
                include_bytes!(concat!("../fixtures/gif/", $name)) as &[u8],
                include_bytes!(concat!("../fixtures/gif/", $name, ".rgba")) as &[u8],
            )
        };
    }

    const FIXTURES: [(&str, &[u8], &[u8]); 8] = [
        fixture!("global.gif"),
        fixture!("local.gif"),
        fixture!("transparent.gif"),
        fixture!("interlaced.gif"),
        fixture!("offset.gif"),
        fixture!("animated.gif"),
        fixture!("two-colour.gif"),
        fixture!("full-table.gif"),
    ];

    #[test]
    fn every_fixture_decodes_to_its_expected_pixels() {
        for (name, bytes, rgba) in FIXTURES {
            let image = decode(bytes, &Limits::DEFAULT).unwrap_or_else(|e| panic!("{name}: {e}"));
            assert_eq!(image.pixels(), rgba, "{name}");
        }
    }

    /// The offset fixture: transparent black around the image, which is
    /// opaque.
    #[test]
    fn the_screen_outside_the_first_image_is_transparent() {
        let image = decode(FIXTURES[4].1, &Limits::DEFAULT).expect("decodes");
        assert_eq!((image.width(), image.height()), (20, 16));
        let (pixels, _) = image.pixels().as_chunks::<4>();
        for (at, pixel) in pixels.iter().enumerate() {
            let (x, y) = (at % 20, at / 20);
            let inside = (6..13).contains(&x) && (4..9).contains(&y);
            assert_eq!(pixel[3], if inside { 255 } else { 0 }, "({x}, {y})");
            if !inside {
                assert_eq!(*pixel, [0; 4], "({x}, {y})");
            }
        }
    }

    /// Black, red, green and blue.
    const PALETTE: [[u8; 3]; 4] = [[0, 0, 0], [255, 0, 0], [0, 255, 0], [0, 0, 255]];
    const BLACK: [u8; 4] = [0, 0, 0, 255];
    const RED: [u8; 4] = [255, 0, 0, 255];
    const GREEN: [u8; 4] = [0, 255, 0, 255];
    const BLUE: [u8; 4] = [0, 0, 255, 255];
    const CLEAR: [u8; 4] = [0; 4];

    /// A header and logical screen descriptor, with `table` as the global
    /// colour table (its length a power of two from 2 to 256).
    fn screen(width: u16, height: u16, table: Option<&[[u8; 3]]>, background: u8) -> Vec<u8> {
        let mut bytes = b"GIF89a".to_vec();
        bytes.extend_from_slice(&width.to_le_bytes());
        bytes.extend_from_slice(&height.to_le_bytes());
        let flags = table.map_or(0, |table| {
            COLOUR_TABLE | (table.len().trailing_zeros() - 1) as u8
        });
        bytes.extend_from_slice(&[flags, background, 0]);
        bytes.extend(table.into_iter().flatten().flatten());
        bytes
    }

    /// An image descriptor and its local colour table.
    fn descriptor(rect: [u16; 4], interlaced: bool, local: Option<&[[u8; 3]]>) -> Vec<u8> {
        let mut bytes = vec![IMAGE];
        for value in rect {
            bytes.extend_from_slice(&value.to_le_bytes());
        }
        let mut flags = if interlaced { INTERLACED } else { 0 };
        if let Some(local) = local {
            flags |= COLOUR_TABLE | (local.len().trailing_zeros() - 1) as u8;
        }
        bytes.push(flags);
        bytes.extend(local.into_iter().flatten().flatten());
        bytes
    }

    /// A graphic control extension, its transparent index set if given.
    fn graphic_control(transparent: Option<u8>) -> Vec<u8> {
        let flags = u8::from(transparent.is_some());
        vec![
            EXTENSION,
            GRAPHIC_CONTROL,
            4,
            flags,
            0,
            0,
            transparent.unwrap_or(0),
            0,
        ]
    }

    /// The minimum code size and `codes` packed into sub-blocks, each code
    /// as wide as a decoder reads it: wider once the entries a run of
    /// codes adds reach its next power of two, narrow again after a clear.
    fn data(min_code_size: u8, codes: &[u16]) -> Vec<u8> {
        let clear = 1u16 << min_code_size;
        let initial = u32::from(min_code_size) + 1;
        let (mut size, mut next, mut fresh) = (initial, clear + 2, true);
        let (mut packed, mut bits, mut count) = (Vec::new(), 0u32, 0u32);
        for &code in codes {
            bits |= u32::from(code) << count;
            count += size;
            while count >= 8 {
                packed.push(bits as u8);
                bits >>= 8;
                count -= 8;
            }
            if code == clear {
                (size, next, fresh) = (initial, clear + 2, true);
            } else if fresh {
                fresh = false;
            } else if usize::from(next) < TABLE_SIZE {
                next += 1;
                if u32::from(next) == 1 << size && size < MAX_CODE_BITS {
                    size += 1;
                }
            }
        }
        if count > 0 {
            packed.push(bits as u8);
        }
        let mut bytes = vec![min_code_size];
        for block in packed.chunks(255) {
            bytes.push(block.len() as u8);
            bytes.extend_from_slice(block);
        }
        bytes.push(0);
        bytes
    }

    /// A file of a 4×1 screen with the palette, `before` the image, which
    /// fills it with `codes` at code size 2.
    fn four_wide(before: &[u8], codes: &[u16]) -> Vec<u8> {
        let mut bytes = screen(4, 1, Some(&PALETTE), 0);
        bytes.extend_from_slice(before);
        bytes.extend(descriptor([0, 0, 4, 1], false, None));
        bytes.extend(data(2, codes));
        bytes.push(TRAILER);
        bytes
    }

    fn pixels(bytes: &[u8]) -> Result<Vec<[u8; 4]>, ImageError> {
        decode(bytes, &Limits::DEFAULT).map(|image| image.pixels().as_chunks::<4>().0.to_vec())
    }

    #[test]
    fn codes_build_strings_including_the_one_being_defined() {
        // Clear, 1, 2, then 6 (= "1 2", the entry 1 then 2 made), end.
        assert_eq!(
            pixels(&four_wide(&[], &[4, 1, 2, 6, 5])),
            Ok(vec![RED, GREEN, RED, GREEN])
        );
        // Clear, 3, then 6 before it exists: "3" and its own first index.
        assert_eq!(
            pixels(&four_wide(&[], &[4, 3, 6, 1, 5])),
            Ok(vec![BLUE, BLUE, BLUE, RED])
        );
        // A string running past the last pixel is cut there, in the indices
        // themselves and not only in what is drawn: 6 is "1 2".
        assert_eq!(
            pixels(&four_wide(&[], &[4, 1, 2, 1, 6, 5])),
            Ok(vec![RED, GREEN, RED, RED])
        );
        assert_eq!(
            lzw(&data(2, &[4, 1, 2, 1, 6, 5])[1..], 2, 4),
            Ok(vec![1, 2, 1, 1])
        );
        // No leading clear code.
        assert_eq!(
            pixels(&four_wide(&[], &[0, 1, 2, 3, 5])),
            Ok(vec![BLACK, RED, GREEN, BLUE])
        );
    }

    #[test]
    fn a_transparent_index_and_an_index_past_the_table_are_transparent() {
        let mut bytes = screen(4, 1, Some(&PALETTE[..2]), 0);
        bytes.extend(graphic_control(Some(1)));
        bytes.extend(descriptor([0, 0, 4, 1], false, None));
        bytes.extend(data(2, &[4, 0, 1, 2, 3, 5]));
        bytes.push(TRAILER);
        assert_eq!(pixels(&bytes), Ok(vec![BLACK, CLEAR, CLEAR, CLEAR]));
        // On a local table, and the last of two extensions counts.
        let mut bytes = screen(4, 1, Some(&PALETTE[..2]), 0);
        bytes.extend(graphic_control(Some(0)));
        bytes.extend(graphic_control(Some(3)));
        bytes.extend(descriptor([0, 0, 4, 1], false, Some(&PALETTE)));
        bytes.extend(data(2, &[4, 0, 1, 2, 3, 5]));
        assert_eq!(pixels(&bytes), Ok(vec![BLACK, RED, GREEN, CLEAR]));
    }

    #[test]
    fn a_plain_text_extension_takes_the_graphic_control_before_it() {
        let mut before = graphic_control(Some(1));
        before.extend_from_slice(&[EXTENSION, PLAIN_TEXT, 12]);
        before.extend_from_slice(&[0; 12]);
        before.extend_from_slice(&[2, b'h', b'i', 0]);
        // A comment and an application extension are only stepped over.
        before.extend_from_slice(&[EXTENSION, 0xfe, 3, b'a', b'b', b'c', 0]);
        before.extend_from_slice(&[EXTENSION, 0xff, 11]);
        before.extend_from_slice(b"NETSCAPE2.0");
        before.extend_from_slice(&[3, 1, 0, 0, 0]);
        assert_eq!(
            pixels(&four_wide(&before, &[4, 0, 1, 2, 3, 5])),
            Ok(vec![BLACK, RED, GREEN, BLUE])
        );
    }

    /// stb_image paints a nonzero background index's colour, with red and
    /// blue swapped, around the image; this decoder keeps it transparent.
    #[test]
    fn a_background_colour_is_not_painted() {
        let mut bytes = screen(3, 2, Some(&PALETTE), 1);
        bytes.extend(descriptor([1, 1, 2, 1], false, None));
        bytes.extend(data(2, &[4, 2, 3, 5]));
        bytes.push(TRAILER);
        assert_eq!(
            pixels(&bytes),
            Ok(vec![CLEAR, CLEAR, CLEAR, CLEAR, GREEN, BLUE])
        );
    }

    #[test]
    fn interlaced_rows_land_in_appendix_e_order() {
        // Nine rows one pixel wide; row r's colour is the pass it is in.
        let pass = [0u16, 3, 2, 3, 1, 3, 2, 3, 0];
        let order = [0, 8, 4, 2, 6, 1, 3, 5, 7];
        let mut codes = vec![4];
        codes.extend(order.iter().map(|&row| pass[row]));
        codes.push(5);
        let mut bytes = screen(1, 9, Some(&PALETTE), 0);
        bytes.extend(descriptor([0, 0, 1, 9], true, None));
        bytes.extend(data(2, &codes));
        let colours = [BLACK, RED, GREEN, BLUE];
        assert_eq!(
            pixels(&bytes),
            Ok(pass.iter().map(|&p| colours[usize::from(p)]).collect())
        );
    }

    /// Past a full table every code is 12 bits and adds nothing, for longer
    /// than the 4096 codes stb_image allows: this stream decodes in Pillow
    /// 13 to the same roots, and stb_image refuses it ("too many codes").
    #[test]
    fn a_full_table_waits_for_its_clear_however_long() {
        let (width, height) = (100u16, 91u16);
        let roots: Vec<u16> = (0..usize::from(width) * usize::from(height))
            .map(|i| ((i * 7 + i / 3) % 4) as u16)
            .collect();
        // 4091 codes fill the table: the first adds nothing.
        assert!(roots.len() > 4091 + 4096);
        let mut codes = vec![4];
        codes.extend_from_slice(&roots);
        codes.push(5);
        let mut bytes = screen(width, height, Some(&PALETTE), 0);
        bytes.extend(descriptor([0, 0, width, height], false, None));
        bytes.extend(data(2, &codes));
        let colours = [BLACK, RED, GREEN, BLUE];
        let want: Vec<[u8; 4]> = roots.iter().map(|&r| colours[usize::from(r)]).collect();
        assert_eq!(pixels(&bytes), Ok(want));
    }

    /// A 4×2 screen filled by one image of `codes`, its background index 1:
    /// with the block terminator and trailer, or with the file ending at
    /// the last data byte.
    fn four_by_two(codes: &[u16], terminated: bool) -> Vec<u8> {
        let mut bytes = screen(4, 2, Some(&PALETTE), 1);
        bytes.extend(descriptor([0, 0, 4, 2], false, None));
        let data = data(2, codes);
        if terminated {
            bytes.extend(data);
            bytes.push(TRAILER);
        } else {
            bytes.extend_from_slice(&data[..data.len() - 1]);
        }
        bytes
    }

    /// Each case beside stb_image's decode of it (as the fixtures' docs
    /// say), whose colour this matches wherever stb_image's alpha is full
    /// and whose alpha it matches when the background index is 0: with it
    /// 1, as here, stb_image paints the missing pixels that colour with red
    /// and blue swapped, opaque blue.
    #[test]
    fn data_ending_before_the_last_pixel_leaves_the_rest_transparent() {
        let full = four_by_two(&[4, 0, 1, 2, 3, 0, 1, 2, 3, 5], true);
        let data_at = full.len() - data(2, &[4, 0, 1, 2, 3, 0, 1, 2, 3, 5]).len() - 1;
        let first_row = vec![BLACK, RED, GREEN, BLUE, CLEAR, CLEAR, CLEAR, CLEAR];
        let three = vec![BLACK, RED, GREEN, CLEAR, CLEAR, CLEAR, CLEAR, CLEAR];
        for (what, bytes, want) in [
            // Cut two bytes into a five-byte sub-block: 16 bits, a clear
            // and four pixels. stb_image reads the missing bytes as zeros,
            // so its four more pixels are colour 0, opaque black.
            ("cut mid-block", full[..data_at + 4].to_vec(), first_row),
            (
                "an end code three pixels in",
                four_by_two(&[4, 0, 1, 2, 5], true),
                three.clone(),
            ),
            (
                "an end code right after a clear",
                four_by_two(&[4, 0, 1, 2, 4, 5], true),
                three.clone(),
            ),
            // The clear fills the last byte, so no padding bits are left
            // to read as a code.
            (
                "no block terminator",
                four_by_two(&[4, 0, 1, 2, 4], false),
                three,
            ),
        ] {
            assert_eq!(pixels(&bytes), Ok(want), "{what}");
            assert_eq!(dimensions(&bytes), Ok((4, 2)), "{what}");
        }
        // Every cut inside the data keeps the pixels decoded before it.
        let whole = pixels(&full).expect("decodes");
        for end in data_at + 1..full.len() {
            let cut = pixels(&full[..end]).expect("a cut decodes");
            let kept = cut.iter().take_while(|&&pixel| pixel != CLEAR).count();
            assert_eq!(cut[..kept], whole[..kept], "cut at {end}");
            assert!(
                cut[kept..].iter().all(|&pixel| pixel == CLEAR),
                "cut at {end}"
            );
        }
        // Once the last pixel is in, nothing after it is read: no end code,
        // no terminator, no trailer, garbage past it.
        let codes = data(2, &[4, 0, 1, 2, 3]);
        let mut bytes = screen(4, 1, Some(&PALETTE), 0);
        bytes.extend(descriptor([0, 0, 4, 1], false, None));
        bytes.extend_from_slice(&codes[..codes.len() - 1]);
        assert_eq!(pixels(&bytes), Ok(vec![BLACK, RED, GREEN, BLUE]));
        bytes.extend_from_slice(&[0xff; 9]);
        assert_eq!(pixels(&bytes), Ok(vec![BLACK, RED, GREEN, BLUE]));
    }

    #[test]
    fn a_code_used_before_it_is_defined_is_malformed() {
        // Each followed by enough pixels to fill the image, so a decoder
        // that let the bad code through would succeed.
        for codes in [
            // The first code after a clear is the entry being made.
            &[4, 6, 0, 1, 2, 3, 5][..],
            // Past the entry being made.
            &[4, 0, 7, 1, 2, 3, 5],
            // The end code's neighbour, first.
            &[4, 7, 0, 1, 2, 3, 5],
        ] {
            assert!(
                matches!(
                    pixels(&four_wide(&[], codes)),
                    Err(ImageError::Malformed(why)) if why.contains("before it is defined")
                ),
                "{codes:?}: {:?}",
                pixels(&four_wide(&[], codes))
            );
        }
    }

    #[test]
    fn the_screen_passes_the_limits_before_anything_is_built() {
        let mut bytes = screen(u16::MAX, u16::MAX, Some(&PALETTE), 0);
        bytes.extend(descriptor([0, 0, 1, 1], false, None));
        bytes.extend(data(2, &[4, 0, 5]));
        assert_eq!(dimensions(&bytes), Ok((65_535, 65_535)));
        assert!(matches!(
            decode(&bytes, &Limits::DEFAULT),
            Err(ImageError::TooLarge {
                width: 65_535,
                height: 65_535,
                ..
            })
        ));
    }

    #[test]
    fn dimensions_are_the_size_decode_returns() {
        for (name, bytes, _) in FIXTURES {
            let image = decode(bytes, &Limits::DEFAULT).expect("decodes");
            assert_eq!(
                dimensions(bytes),
                Ok((image.width(), image.height())),
                "{name}"
            );
        }
        assert_eq!(dimensions(FIXTURES[4].1), Ok((20, 16)));
    }

    #[test]
    fn dimensions_refuse_what_decode_refuses_before_the_data() {
        let good = four_wide(&graphic_control(Some(1)), &[4, 0, 1, 2, 3, 5]);
        let data_at = good.len() - data(2, &[4, 0, 1, 2, 3, 5]).len() - 1;
        assert_eq!(dimensions(&good), Ok((4, 1)));
        let mut refused: Vec<(String, Vec<u8>)> = (0..=data_at)
            .map(|end| (format!("cut at {end}"), good[..end].to_vec()))
            .collect();
        let mut version = good.clone();
        version[4] = b'8';
        refused.push(("GIF88a".into(), version));
        let mut past = screen(4, 1, Some(&PALETTE), 0);
        past.extend(descriptor([1, 0, 4, 1], false, None));
        past.extend(data(2, &[4, 0, 1, 2, 3, 5]));
        refused.push(("an image past the screen".into(), past));
        for min_code_size in [0, 1, 9, 12] {
            let mut bytes = screen(4, 1, Some(&PALETTE), 0);
            bytes.extend(descriptor([0, 0, 4, 1], false, None));
            bytes.extend([min_code_size, 1, 0, 0]);
            refused.push((format!("code size {min_code_size}"), bytes));
        }
        let mut no_image = screen(4, 1, Some(&PALETTE), 0);
        no_image.push(TRAILER);
        refused.push(("a trailer first".into(), no_image));
        let mut unknown = screen(4, 1, Some(&PALETTE), 0);
        unknown.push(0x00);
        refused.push(("an unknown block".into(), unknown));
        let mut no_table = screen(4, 1, None, 0);
        no_table.extend(descriptor([0, 0, 4, 1], false, None));
        no_table.extend(data(2, &[4, 0, 1, 2, 3, 5]));
        refused.push(("no colour table".into(), no_table.clone()));
        refused.push(("an empty screen".into(), {
            let mut bytes = screen(0, 1, Some(&PALETTE), 0);
            bytes.extend(descriptor([0, 0, 0, 0], false, None));
            bytes.extend(data(2, &[4, 5]));
            bytes
        }));
        for (what, bytes) in &refused {
            let error = dimensions(bytes).expect_err(what);
            assert_eq!(
                decode(bytes, &Limits::DEFAULT).map(|image| (image.width(), image.height())),
                Err(error.clone()),
                "{what}: the same error as decode"
            );
            let want_kind = match what.as_str() {
                "no colour table" => matches!(error, ImageError::Unsupported(_)),
                "an empty screen" => {
                    error
                        == ImageError::Empty {
                            width: 0,
                            height: 1,
                        }
                }
                _ => matches!(error, ImageError::Malformed(_)),
            };
            assert!(want_kind, "{what}: {error:?}");
        }
    }
}
