//! BMP, written here: Windows bitmaps, and the device-independent bitmaps
//! inside icons, decoded to RGBA8.
//!
//! # What it reads
//!
//! The core (OS/2 1.x) header and the `BITMAPINFOHEADER` family through V5;
//! 1, 4 and 8 bits a pixel through a palette, 16, 24 and 32 bits direct;
//! uncompressed, `BI_BITFIELDS` and `BI_ALPHABITFIELDS` masks, and `BI_RLE8`
//! and `BI_RLE4` run-length coding; rows bottom-up or, with a negative height,
//! top-down. JPEG- and PNG-compressed bitmaps, and the OS/2 2.x header, are
//! [`ImageError::Unsupported`].
//!
//! # Alpha
//!
//! A 32-bit bitmap with no masks has a fourth byte Windows calls unused, which
//! many writers fill with alpha. As Chrome does, it is alpha unless it is
//! zero everywhere, in which case the bitmap is opaque. (Pillow always drops
//! it.) A bitmap without an alpha channel is opaque. Pixels a run-length
//! bitmap skips are transparent.
//!
//! A mask's channel narrower than eight bits is widened by repeating its bits
//! (five bits `abcde` become `abcdeabc`), which maps full to full and empty to
//! empty, as libpng and Pillow widen 16-bit pixels.
//!
//! # Inside an icon
//!
//! An icon's bitmap has no file header, a height twice the image's, and a
//! one-bit AND mask after its pixels that says which show when the pixels
//! carry no alpha. The `ico` module reads that form through this one.

use crate::raw::{AllZeroAlpha, Alpha, ChannelOrder, RawPixels, RowOrder, convert};
use crate::{ImageError, Limits, Rgba8Image};

/// The size of `BITMAPFILEHEADER`, before the DIB header.
const FILE_HEADER: usize = 14;

const BI_RGB: u32 = 0;
const BI_RLE8: u32 = 1;
const BI_RLE4: u32 = 2;
const BI_BITFIELDS: u32 = 3;
const BI_ALPHABITFIELDS: u32 = 6;

fn malformed(why: impl Into<String>) -> ImageError {
    ImageError::Malformed(why.into())
}

fn unsupported(why: impl Into<String>) -> ImageError {
    ImageError::Unsupported(why.into())
}

/// Little-endian reads that fail rather than panic.
fn u16_at(bytes: &[u8], at: usize) -> Result<u16, ImageError> {
    bytes
        .get(at..at + 2)
        .map(|b| u16::from_le_bytes([b[0], b[1]]))
        .ok_or_else(|| malformed("a bitmap header is cut short"))
}

fn u32_at(bytes: &[u8], at: usize) -> Result<u32, ImageError> {
    bytes
        .get(at..at + 4)
        .map(|b| u32::from_le_bytes([b[0], b[1], b[2], b[3]]))
        .ok_or_else(|| malformed("a bitmap header is cut short"))
}

/// Decodes a `.bmp` file to straight RGBA8.
///
/// # Errors
///
/// [`ImageError::Empty`] or [`ImageError::TooLarge`] if its size breaks
/// `limits`, [`ImageError::Unsupported`] for a kind of bitmap the [module
/// docs](self) do not list, and [`ImageError::Malformed`] otherwise.
pub fn decode(bytes: &[u8], limits: &Limits) -> Result<Rgba8Image, ImageError> {
    if bytes.get(..2) != Some(b"BM") {
        return Err(malformed("no BM signature"));
    }
    let pixels_at = u32_at(bytes, 10)? as usize;
    let dib = bytes
        .get(FILE_HEADER..)
        .ok_or_else(|| malformed("a bitmap file header is cut short"))?;
    let (header, _) = Header::read(dib, false)?;
    let pixels = bytes
        .get(pixels_at..)
        .ok_or_else(|| malformed("the pixel offset is past the file"))?;
    header.decode(pixels, AllZeroAlpha::Opaque, limits)
}

/// Decodes the bitmap inside an icon: no file header, the pixels straight
/// after the header and palette, then the AND mask.
#[cfg(feature = "ico")]
pub(crate) fn decode_icon_dib(dib: &[u8], limits: &Limits) -> Result<Rgba8Image, ImageError> {
    let (header, pixels_at) = Header::read(dib, true)?;
    let pixels = dib
        .get(pixels_at..)
        .ok_or_else(|| malformed("an icon's bitmap is cut short"))?;
    let row_bytes = header.row_bytes()?;
    let mask_at = row_bytes
        .checked_mul(header.height as usize)
        .ok_or_else(|| malformed("an icon's bitmap is too large"))?;
    let mask_stride = (header.width as usize).div_ceil(32) * 4;
    // A mask cut short or missing shows every pixel, as Windows draws it.
    let mask = pixels
        .get(mask_at..)
        .filter(|mask| mask.len() >= mask_stride * header.height as usize);
    let all_zero_alpha = match mask {
        Some(bits) => AllZeroAlpha::AndMask {
            bits,
            stride: mask_stride,
        },
        None => AllZeroAlpha::Opaque,
    };
    header.decode(pixels, all_zero_alpha, limits)
}

/// What a DIB header says.
#[derive(Debug)]
struct Header {
    width: u32,
    height: u32,
    top_down: bool,
    bits: u16,
    compression: u32,
    /// Red, green, blue and alpha masks for a direct-colour bitmap.
    masks: [u32; 4],
    palette: Vec<[u8; 4]>,
}

impl Header {
    /// Reads the DIB header at the start of `dib`, its masks and its palette,
    /// and says where in `dib` they end: where an icon's pixels start. An
    /// icon's header gives twice the image's height.
    fn read(dib: &[u8], icon: bool) -> Result<(Self, usize), ImageError> {
        let size = u32_at(dib, 0)? as usize;
        let (width, raw_height, planes, bits, compression, colours_used) = match size {
            12 => (
                i32::from(u16_at(dib, 4)?),
                i32::from(u16_at(dib, 6)?),
                u16_at(dib, 8)?,
                u16_at(dib, 10)?,
                BI_RGB,
                0,
            ),
            40 | 52 | 56 | 108 | 124 => (
                u32_at(dib, 4)? as i32,
                u32_at(dib, 8)? as i32,
                u16_at(dib, 12)?,
                u16_at(dib, 14)?,
                u32_at(dib, 16)?,
                u32_at(dib, 32)? as usize,
            ),
            _ => return Err(unsupported(format!("a {size}-byte bitmap header"))),
        };
        if planes != 1 {
            return Err(malformed("a bitmap of other than one plane"));
        }
        let top_down = raw_height < 0;
        let height = raw_height.unsigned_abs();
        let height = if icon { height / 2 } else { height };
        let width = u32::try_from(width).map_err(|_| malformed("a negative bitmap width"))?;
        if top_down && matches!(compression, BI_RLE8 | BI_RLE4) {
            return Err(malformed("a top-down run-length bitmap"));
        }
        match (bits, compression) {
            (1 | 4 | 8, BI_RGB) | (8, BI_RLE8) | (4, BI_RLE4) => {}
            (16 | 24 | 32, BI_RGB) | (16 | 32, BI_BITFIELDS | BI_ALPHABITFIELDS) => {}
            (_, 4 | 5) => return Err(unsupported("a JPEG- or PNG-compressed bitmap")),
            (bits, compression) => {
                return Err(unsupported(format!(
                    "a {bits}-bit bitmap with compression {compression}"
                )));
            }
        }

        let mut after = size;
        let masks = match (bits, compression) {
            (16, BI_RGB) => [0x7c00, 0x03e0, 0x001f, 0],
            (24 | 32, BI_RGB) => [0x00ff_0000, 0x0000_ff00, 0x0000_00ff, 0xff00_0000],
            (_, BI_BITFIELDS | BI_ALPHABITFIELDS) => {
                // A V2 or later header holds the masks; an INFO header is
                // followed by them.
                let base = if size >= 52 { 40 } else { size };
                let with_alpha = compression == BI_ALPHABITFIELDS || size >= 56;
                let masks = [
                    u32_at(dib, base)?,
                    u32_at(dib, base + 4)?,
                    u32_at(dib, base + 8)?,
                    if with_alpha {
                        u32_at(dib, base + 12)?
                    } else {
                        0
                    },
                ];
                if size < 52 {
                    after += if compression == BI_ALPHABITFIELDS {
                        16
                    } else {
                        12
                    };
                }
                masks
            }
            _ => [0; 4],
        };

        let entry = if size == 12 { 3 } else { 4 };
        let palette = if bits <= 8 {
            let count = match colours_used {
                0 => 1 << bits,
                used => used.min(1 << bits),
            };
            let table = dib
                .get(after..after + count * entry)
                .ok_or_else(|| malformed("a bitmap's palette is cut short"))?;
            after += count * entry;
            table
                .chunks_exact(entry)
                .map(|bgr| [bgr[2], bgr[1], bgr[0], u8::MAX])
                .collect()
        } else {
            Vec::new()
        };
        let header = Self {
            width,
            height,
            top_down,
            bits,
            compression,
            masks,
            palette,
        };
        Ok((header, after))
    }

    /// The bytes one uncompressed row takes, padded to four.
    fn row_bytes(&self) -> Result<usize, ImageError> {
        (self.width as usize)
            .checked_mul(usize::from(self.bits))
            .map(|bits| bits.div_ceil(32) * 4)
            .ok_or_else(|| malformed("a bitmap row too long"))
    }

    /// The pixels, through `raw::convert`, after checking the size.
    fn decode(
        &self,
        pixels: &[u8],
        all_zero_alpha: AllZeroAlpha<'_>,
        limits: &Limits,
    ) -> Result<Rgba8Image, ImageError> {
        limits.rgba_bytes(self.width, self.height)?;
        let (bgra, written_alpha) = match self.compression {
            BI_RLE8 | BI_RLE4 => (self.run_length(pixels)?, true),
            _ => (self.uncompressed(pixels)?, false),
        };
        let rows = if self.top_down {
            RowOrder::TopDown
        } else {
            RowOrder::BottomUp
        };
        convert(
            &RawPixels {
                width: self.width,
                height: self.height,
                stride: self.width as usize * 4,
                order: ChannelOrder::Bgra,
                rows,
                alpha: Alpha::Straight,
                all_zero_alpha: if written_alpha {
                    AllZeroAlpha::Transparent
                } else {
                    all_zero_alpha
                },
                data: &bgra,
            },
            limits,
        )
    }

    /// An uncompressed bitmap as BGRA rows in the file's row order, alpha
    /// zero where the bitmap has none.
    fn uncompressed(&self, pixels: &[u8]) -> Result<Vec<u8>, ImageError> {
        let (width, height) = (self.width as usize, self.height as usize);
        let row_bytes = self.row_bytes()?;
        let needed = row_bytes
            .checked_mul(height)
            .ok_or_else(|| malformed("a bitmap too large"))?;
        if pixels.len() < needed {
            return Err(ImageError::BufferSize {
                needed,
                actual: pixels.len(),
            });
        }
        let mut out = Vec::with_capacity(width * height * 4);
        for row in pixels[..needed].chunks_exact(row_bytes) {
            for x in 0..width {
                let bgra = match self.bits {
                    1 | 4 | 8 => {
                        let bits = usize::from(self.bits);
                        let bit = x * bits;
                        let shift = 8 - bits - bit % 8;
                        let index = (row[bit / 8] >> shift) & ((1 << bits) - 1) as u8;
                        let [r, g, b, _] = self.colour(index)?;
                        [b, g, r, 0]
                    }
                    16 => self.masked(u32::from(u16::from_le_bytes([row[x * 2], row[x * 2 + 1]]))),
                    24 => [row[x * 3], row[x * 3 + 1], row[x * 3 + 2], 0],
                    _ => self.masked(u32::from_le_bytes([
                        row[x * 4],
                        row[x * 4 + 1],
                        row[x * 4 + 2],
                        row[x * 4 + 3],
                    ])),
                };
                out.extend_from_slice(&bgra);
            }
        }
        Ok(out)
    }

    /// A palette entry, or an error for an index past the palette.
    fn colour(&self, index: u8) -> Result<[u8; 4], ImageError> {
        self.palette
            .get(usize::from(index))
            .copied()
            .ok_or_else(|| malformed("a pixel indexes past its palette"))
    }

    /// A direct-colour pixel through the masks, as BGRA.
    fn masked(&self, value: u32) -> [u8; 4] {
        let [r, g, b, a] = self.masks.map(|mask| widen(value, mask));
        [b, g, r, a]
    }

    /// A run-length bitmap as BGRA rows bottom-up, alpha full where a pixel
    /// was written and zero where it was skipped.
    fn run_length(&self, data: &[u8]) -> Result<Vec<u8>, ImageError> {
        let (width, height) = (self.width as usize, self.height as usize);
        let mut out = vec![0u8; width * height * 4];
        let four = self.compression == BI_RLE4;
        let (mut x, mut y, mut at) = (0usize, 0usize, 0usize);
        let put = |out: &mut [u8], x: usize, y: usize, index: u8| -> Result<(), ImageError> {
            if x < width && y < height {
                let [r, g, b, _] = self.colour(index)?;
                let at = (y * width + x) * 4;
                out[at..at + 4].copy_from_slice(&[b, g, r, u8::MAX]);
            }
            Ok(())
        };
        // Data that stops without its end marker ends the bitmap there.
        while let (Some(&count), Some(&value)) = (data.get(at), data.get(at + 1)) {
            at += 2;
            if count > 0 {
                for i in 0..usize::from(count) {
                    let index = if four {
                        if i % 2 == 0 { value >> 4 } else { value & 0x0f }
                    } else {
                        value
                    };
                    put(&mut out, x, y, index)?;
                    x += 1;
                }
                continue;
            }
            match value {
                0 => {
                    x = 0;
                    y += 1;
                }
                1 => break,
                2 => {
                    let (Some(&dx), Some(&dy)) = (data.get(at), data.get(at + 1)) else {
                        return Err(malformed("a run-length delta is cut short"));
                    };
                    at += 2;
                    x += usize::from(dx);
                    y += usize::from(dy);
                }
                literal => {
                    let literal = usize::from(literal);
                    let bytes = if four { literal.div_ceil(2) } else { literal };
                    let run = data
                        .get(at..at + bytes)
                        .ok_or_else(|| malformed("a run-length literal is cut short"))?;
                    for i in 0..literal {
                        let index = if four {
                            let byte = run[i / 2];
                            if i % 2 == 0 { byte >> 4 } else { byte & 0x0f }
                        } else {
                            run[i]
                        };
                        put(&mut out, x, y, index)?;
                        x += 1;
                    }
                    // Each literal run is padded to a whole 16-bit word.
                    at += bytes + bytes % 2;
                }
            }
            if y >= height {
                break;
            }
        }
        Ok(out)
    }
}

/// The channel `mask` selects from `value`, widened or narrowed to 8 bits:
/// a narrower channel's bits repeated, a wider one's top 8 kept, an absent
/// one zero.
fn widen(value: u32, mask: u32) -> u8 {
    if mask == 0 {
        return 0;
    }
    let shift = mask.trailing_zeros();
    let bits = (mask >> shift).trailing_ones();
    let channel = (value & mask) >> shift;
    if bits >= 8 {
        return (channel >> (bits - 8)) as u8;
    }
    let mut out = 0u32;
    let mut filled = 0;
    while filled < 8 {
        out = (out << bits) | channel;
        filled += bits;
    }
    (out >> (filled - 8)) as u8
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Each fixture beside its expected RGBA: colour from Pillow 12.3's decode
    /// of it, alpha from the image the script wrote (Pillow drops a 32-bit
    /// bitmap's fourth byte). Written by Pillow's encoder from seeded random
    /// pixels.
    macro_rules! fixture {
        ($name:literal) => {
            (
                $name,
                include_bytes!(concat!("../fixtures/bmp/", $name)) as &[u8],
                include_bytes!(concat!("../fixtures/bmp/", $name, ".rgba")) as &[u8],
            )
        };
    }

    #[test]
    fn every_fixture_decodes_to_its_expected_pixels() {
        for (name, bytes, rgba) in [
            fixture!("one-bit.bmp"),
            fixture!("grey.bmp"),
            fixture!("palette.bmp"),
            fixture!("rgb.bmp"),
            fixture!("rgba.bmp"),
        ] {
            let image = decode(bytes, &Limits::DEFAULT).unwrap_or_else(|e| panic!("{name}: {e}"));
            assert_eq!(image.pixels(), rgba, "{name}");
        }
    }

    /// A file of `dib`, its pixels at `pixels_at` within it.
    fn file(dib: &[u8], pixels_at: usize) -> Vec<u8> {
        let mut bytes = b"BM".to_vec();
        bytes.extend_from_slice(&((FILE_HEADER + dib.len()) as u32).to_le_bytes());
        bytes.extend_from_slice(&[0; 4]);
        bytes.extend_from_slice(&((FILE_HEADER + pixels_at) as u32).to_le_bytes());
        bytes.extend_from_slice(dib);
        bytes
    }

    /// A 40-byte header.
    fn info(width: i32, height: i32, bits: u16, compression: u32, colours: u32) -> Vec<u8> {
        let mut header = 40u32.to_le_bytes().to_vec();
        header.extend_from_slice(&width.to_le_bytes());
        header.extend_from_slice(&height.to_le_bytes());
        header.extend_from_slice(&1u16.to_le_bytes());
        header.extend_from_slice(&bits.to_le_bytes());
        header.extend_from_slice(&compression.to_le_bytes());
        header.extend_from_slice(&[0; 12]);
        header.extend_from_slice(&colours.to_le_bytes());
        header.extend_from_slice(&[0; 4]);
        header
    }

    /// Black, red, green and blue, as a palette's BGRx entries.
    const PALETTE: [u8; 16] = [0, 0, 0, 0, 0, 0, 255, 0, 0, 255, 0, 0, 255, 0, 0, 0];
    const BLACK: [u8; 4] = [0, 0, 0, 255];
    const RED: [u8; 4] = [255, 0, 0, 255];
    const GREEN: [u8; 4] = [0, 255, 0, 255];
    const BLUE: [u8; 4] = [0, 0, 255, 255];
    const CLEAR: [u8; 4] = [0, 0, 0, 0];

    fn pixels(image: &Rgba8Image) -> Vec<[u8; 4]> {
        image.pixels().as_chunks::<4>().0.to_vec()
    }

    #[test]
    fn four_bit_rows_read_high_nibble_first_from_the_bottom() {
        let mut dib = info(3, 2, 4, BI_RGB, 4);
        dib.extend_from_slice(&PALETTE);
        let at = dib.len();
        // Bottom row 1, 2, 3; top row 3, 0, 1; each padded to four bytes.
        dib.extend_from_slice(&[0x12, 0x30, 0, 0, 0x30, 0x10, 0, 0]);
        let image = decode(&file(&dib, at), &Limits::DEFAULT).expect("decodes");
        assert_eq!(pixels(&image), [BLUE, BLACK, RED, RED, GREEN, BLUE]);
    }

    #[test]
    fn run_length_bitmaps_follow_runs_literals_deltas_and_ends() {
        let mut dib = info(4, 3, 8, BI_RLE8, 4);
        dib.extend_from_slice(&PALETTE);
        let at = dib.len();
        dib.extend_from_slice(&[
            2, 1, // two reds on the bottom row
            0, 3, 2, 3, 1, 0, // a literal green, blue, red, padded
            0, 0, // next row
            0, 2, 2, 1, // a delta right two and up one
            1, 3, // one blue
            0, 1, // the end
        ]);
        let image = decode(&file(&dib, at), &Limits::DEFAULT).expect("decodes");
        assert_eq!(
            pixels(&image),
            [
                CLEAR, CLEAR, BLUE, CLEAR, //
                CLEAR, CLEAR, CLEAR, CLEAR, //
                RED, RED, GREEN, BLUE,
            ]
        );
        let mut dib = info(3, 1, 4, BI_RLE4, 4);
        dib.extend_from_slice(&PALETTE);
        let at = dib.len();
        dib.extend_from_slice(&[3, 0x12, 0, 1]);
        let image = decode(&file(&dib, at), &Limits::DEFAULT).expect("decodes");
        assert_eq!(pixels(&image), [RED, GREEN, RED]);
    }

    #[test]
    fn masks_choose_the_channels_and_a_negative_height_runs_top_down() {
        // 5-6-5, top-down: pure red over pure blue.
        let mut dib = info(1, -2, 16, BI_BITFIELDS, 0);
        for mask in [0xf800u32, 0x07e0, 0x001f] {
            dib.extend_from_slice(&mask.to_le_bytes());
        }
        let at = dib.len();
        dib.extend_from_slice(&[0x00, 0xf8, 0, 0, 0x1f, 0x00, 0, 0]);
        let image = decode(&file(&dib, at), &Limits::DEFAULT).expect("decodes");
        assert_eq!(pixels(&image), [RED, BLUE]);
    }

    #[test]
    fn sixteen_bits_without_masks_are_five_five_five() {
        let dib = info(2, 1, 16, BI_RGB, 0);
        let at = dib.len();
        let mut dib = dib;
        // Full red, then full blue; the top bit is unused.
        dib.extend_from_slice(&[0x00, 0x7c, 0x1f, 0x00]);
        let image = decode(&file(&dib, at), &Limits::DEFAULT).expect("decodes");
        assert_eq!(pixels(&image), [RED, BLUE]);
    }

    /// An icon's pixels follow its header directly, so the masks an INFO
    /// header is followed by have to be stepped over to find them.
    #[cfg(feature = "ico")]
    #[test]
    fn an_icon_bitmap_finds_its_pixels_past_its_masks() {
        let mut dib = info(1, 2, 16, BI_BITFIELDS, 0);
        for mask in [0xf800u32, 0x07e0, 0x001f] {
            dib.extend_from_slice(&mask.to_le_bytes());
        }
        dib.extend_from_slice(&[0xe0, 0x07, 0, 0]);
        dib.extend_from_slice(&[0, 0, 0, 0]);
        let image = decode_icon_dib(&dib, &Limits::DEFAULT).expect("decodes");
        assert_eq!(pixels(&image), [GREEN]);
    }

    #[test]
    fn a_core_header_has_three_byte_palette_entries() {
        let mut dib = 12u32.to_le_bytes().to_vec();
        for value in [2u16, 1, 1, 1] {
            dib.extend_from_slice(&value.to_le_bytes());
        }
        dib.extend_from_slice(&[0, 0, 0, 255, 255, 255]);
        let at = dib.len();
        dib.extend_from_slice(&[0b0100_0000, 0, 0, 0]);
        let image = decode(&file(&dib, at), &Limits::DEFAULT).expect("decodes");
        assert_eq!(pixels(&image), [BLACK, [255; 4]]);
    }

    #[cfg(feature = "ico")]
    #[test]
    fn an_icon_bitmap_without_alpha_takes_its_and_mask() {
        let mut dib = info(2, 4, 24, BI_RGB, 0);
        // Bottom row blue, red; top row green, blue; rows padded to eight.
        dib.extend_from_slice(&[255, 0, 0, 0, 0, 255, 0, 0, 0, 255, 0, 255, 0, 0, 0, 0]);
        // AND mask, bottom row first: the left pixel of the bottom row and the
        // right of the top are transparent.
        dib.extend_from_slice(&[0b1000_0000, 0, 0, 0, 0b0100_0000, 0, 0, 0]);
        let image = decode_icon_dib(&dib, &Limits::DEFAULT).expect("decodes");
        assert_eq!(pixels(&image), [GREEN, [0, 0, 255, 0], [0, 0, 255, 0], RED]);
    }

    #[test]
    fn damage_is_malformed_and_a_huge_claim_is_too_large() {
        let mut dib = info(2, 1, 8, BI_RGB, 2);
        dib.extend_from_slice(&PALETTE[..8]);
        let at = dib.len();
        dib.extend_from_slice(&[0, 3, 0, 0]);
        assert!(matches!(
            decode(&file(&dib, at), &Limits::DEFAULT),
            Err(ImageError::Malformed(_))
        ));
        let short_header = [b'B', b'M', 0, 0, 0, 0, 0, 0, 0, 0, 0x36, 0, 0, 0];
        for bytes in [&b""[..], b"BM", &short_header[..]] {
            assert!(matches!(
                decode(bytes, &Limits::DEFAULT),
                Err(ImageError::Malformed(_))
            ));
        }
        let huge = file(&info(100_000, 100_000, 24, BI_RGB, 0), 40);
        assert!(matches!(
            decode(&huge, &Limits::DEFAULT),
            Err(ImageError::TooLarge { .. })
        ));
    }

    #[test]
    fn a_narrow_channel_is_widened_by_repeating_its_bits() {
        assert_eq!(widen(0b11111, 0b11111), 255);
        assert_eq!(widen(0, 0b11111), 0);
        assert_eq!(widen(0b10000, 0b11111), 0b1000_0100);
        assert_eq!(widen(0b100000, 0b111111), 0b1000_0010);
        assert_eq!(widen(0b1, 0b1), 255);
        assert_eq!(widen(0x3ff << 2, 0x3ff << 2), 255);
        assert_eq!(widen(0x200 << 2, 0x3ff << 2), 0x80);
        assert_eq!(widen(0xab00, 0xff00), 0xab);
    }
}
