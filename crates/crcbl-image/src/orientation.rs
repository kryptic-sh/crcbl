//! EXIF orientation: which way up a stored image is meant to be seen, and
//! the flip or quarter turn that brings it upright.
//!
//! A camera writes its pixels in the sensor's order and records in the
//! file's EXIF block how it was held, as TIFF tag 274 (`0x0112`,
//! `Orientation`) in the first image file directory. The PNG, JPEG and WebP
//! decoders read that tag and apply it by default: each format's `decode`
//! returns the image upright, its `decode_stored` returns the pixels as
//! stored, and its `orientation` reads the tag alone from the headers.
//!
//! # Reading the tag
//!
//! Only the first image file directory is read, one entry at a time, every
//! offset checked against the block: no other directory is followed, so the
//! work is bounded by the block's length. The tag counts only as the TIFF
//! 6.0 specification types it, one `SHORT`, in either byte order. A block
//! that is missing, damaged or cut short, a tag of another type or count,
//! and a value outside 1 to 8 all read as [`Orientation::Normal`], never as
//! an error: the picture is still there, as browsers draw it.
//!
//! # EXIF only
//!
//! The tag is read from the EXIF block alone. Pillow's `getexif`, which
//! `ImageOps.exif_transpose` reads it through, falls back to an XMP
//! packet's `tiff:Orientation` when the EXIF block has none, and reads a
//! PNG's `Raw profile type exif` text chunk as EXIF; this crate reads
//! neither, so a file oriented only that way decodes as stored here and
//! turned in Pillow.

use crate::{ImageError, Limits, Rgba8Image};

/// The eight orientations EXIF records, each named for the change that
/// brings the stored pixels upright.
///
/// The TIFF 6.0 specification defines the values by where the stored image's
/// first row and first column belong: value 6, say, is "the 0th row
/// represents the visual right-hand side of the image, and the 0th column
/// represents the visual top", which is a picture to be turned a quarter
/// clockwise. The last four swap the image's width and height.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub enum Orientation {
    /// Value 1: upright as stored.
    #[default]
    Normal,
    /// Value 2: mirrored left to right.
    FlipHorizontal,
    /// Value 3: turned half a turn.
    Rotate180,
    /// Value 4: mirrored top to bottom.
    FlipVertical,
    /// Value 5: mirrored about the diagonal from the top-left corner.
    Transpose,
    /// Value 6: turned a quarter turn clockwise.
    Rotate90Clockwise,
    /// Value 7: mirrored about the diagonal from the top-right corner.
    Transverse,
    /// Value 8: turned a quarter turn counter-clockwise.
    Rotate90CounterClockwise,
}

/// The `Exif\0\0` identifier a JPEG's `APP1` segment starts with, and some
/// writers put before the TIFF header in PNG and WebP too.
#[cfg(any(feature = "png", feature = "jpeg", feature = "webp"))]
pub(crate) const EXIF_IDENTIFIER: &[u8] = b"Exif\0\0";

/// The tag number of `Orientation`.
#[cfg(any(feature = "png", feature = "jpeg", feature = "webp"))]
const ORIENTATION_TAG: u16 = 0x0112;

/// TIFF's field type `SHORT`, a 16-bit unsigned integer.
#[cfg(any(feature = "png", feature = "jpeg", feature = "webp"))]
const SHORT: u16 = 3;

/// The bytes of a TIFF header: byte order, 42, and the first directory's
/// offset.
#[cfg(any(feature = "png", feature = "jpeg", feature = "webp"))]
const TIFF_HEADER: usize = 8;

/// The bytes of one directory entry: tag, type, count and value or offset.
#[cfg(any(feature = "png", feature = "jpeg", feature = "webp"))]
const ENTRY: usize = 12;

impl Orientation {
    /// The orientation an EXIF `Orientation` value names; anything outside 1
    /// to 8 is [`Orientation::Normal`].
    #[must_use]
    pub const fn from_exif(value: u16) -> Self {
        match value {
            2 => Self::FlipHorizontal,
            3 => Self::Rotate180,
            4 => Self::FlipVertical,
            5 => Self::Transpose,
            6 => Self::Rotate90Clockwise,
            7 => Self::Transverse,
            8 => Self::Rotate90CounterClockwise,
            _ => Self::Normal,
        }
    }

    /// The EXIF `Orientation` value, 1 to 8.
    #[must_use]
    pub const fn exif_value(self) -> u16 {
        match self {
            Self::Normal => 1,
            Self::FlipHorizontal => 2,
            Self::Rotate180 => 3,
            Self::FlipVertical => 4,
            Self::Transpose => 5,
            Self::Rotate90Clockwise => 6,
            Self::Transverse => 7,
            Self::Rotate90CounterClockwise => 8,
        }
    }

    /// Whether bringing the image upright swaps its width and height.
    #[must_use]
    pub const fn swaps_sides(self) -> bool {
        matches!(
            self,
            Self::Transpose
                | Self::Rotate90Clockwise
                | Self::Transverse
                | Self::Rotate90CounterClockwise
        )
    }

    /// The size an image stored at `(width, height)` is upright: the sides
    /// swapped if [`swaps_sides`](Self::swaps_sides).
    #[must_use]
    pub const fn upright_size(self, (width, height): (u32, u32)) -> (u32, u32) {
        if self.swaps_sides() {
            (height, width)
        } else {
            (width, height)
        }
    }

    /// `image`, stored in this orientation, brought upright.
    ///
    /// The upright size passes `limits` before anything is allocated: a
    /// quarter turn swaps the sides the caps are checked against. A flip or
    /// a half turn moves the pixels in place; a quarter turn of an image
    /// that is not square cannot be done in place without a cycle-following
    /// walk over every pixel, so it writes a second buffer, of the same
    /// size, under the same limits.
    ///
    /// # Errors
    ///
    /// [`ImageError::TooLarge`] if the upright size breaks `limits`.
    pub fn apply(self, image: Rgba8Image, limits: &Limits) -> Result<Rgba8Image, ImageError> {
        let (width, height) = (image.width, image.height);
        let (upright_width, upright_height) = self.upright_size((width, height));
        let bytes = limits.rgba_bytes(upright_width, upright_height)?;
        let mut pixels = image.pixels;
        let (w, h) = (width as usize, height as usize);
        // Where the upright pixel at (x, y) comes from, for the quarter
        // turns, whose upright image is `h` wide and `w` high.
        let source: fn(usize, usize, usize, usize) -> (usize, usize) = match self {
            Self::Normal => return Ok(Rgba8Image::from_sized(width, height, pixels)),
            Self::FlipHorizontal => {
                for row in pixels.chunks_exact_mut(w * 4) {
                    row.as_chunks_mut::<4>().0.reverse();
                }
                return Ok(Rgba8Image::from_sized(width, height, pixels));
            }
            Self::Rotate180 => {
                pixels.as_chunks_mut::<4>().0.reverse();
                return Ok(Rgba8Image::from_sized(width, height, pixels));
            }
            Self::FlipVertical => {
                let row = w * 4;
                for y in 0..h / 2 {
                    let (top, bottom) = pixels.split_at_mut((h - 1 - y) * row);
                    top[y * row..(y + 1) * row].swap_with_slice(&mut bottom[..row]);
                }
                return Ok(Rgba8Image::from_sized(width, height, pixels));
            }
            Self::Transpose => |x, y, _, _| (y, x),
            Self::Rotate90Clockwise => |x, y, _, h| (y, h - 1 - x),
            Self::Transverse => |x, y, w, h| (w - 1 - y, h - 1 - x),
            Self::Rotate90CounterClockwise => |x, y, w, _| (w - 1 - y, x),
        };
        let mut upright = Vec::with_capacity(bytes);
        for y in 0..w {
            for x in 0..h {
                let (sx, sy) = source(x, y, w, h);
                let at = (sy * w + sx) * 4;
                upright.extend_from_slice(&pixels[at..at + 4]);
            }
        }
        Ok(Rgba8Image::from_sized(
            upright_width,
            upright_height,
            upright,
        ))
    }

    /// The orientation an EXIF block records: a TIFF structure, optionally
    /// after the `Exif\0\0` identifier, read as the [module docs](self) say.
    #[cfg(any(feature = "png", feature = "jpeg", feature = "webp"))]
    pub(crate) fn read_exif(block: &[u8]) -> Self {
        let tiff = block.strip_prefix(EXIF_IDENTIFIER).unwrap_or(block);
        Self::read_tiff(tiff).map_or(Self::Normal, Self::from_exif)
    }

    /// The first directory's `Orientation` value, or `None` wherever the
    /// structure is not as TIFF lays it out.
    #[cfg(any(feature = "png", feature = "jpeg", feature = "webp"))]
    fn read_tiff(tiff: &[u8]) -> Option<u16> {
        let big_endian = match tiff.get(..2)? {
            b"II" => false,
            b"MM" => true,
            _ => return None,
        };
        let u16_at = |at: usize| {
            let bytes: [u8; 2] = tiff.get(at..at.checked_add(2)?)?.try_into().ok()?;
            Some(if big_endian {
                u16::from_be_bytes(bytes)
            } else {
                u16::from_le_bytes(bytes)
            })
        };
        let u32_at = |at: usize| {
            let bytes: [u8; 4] = tiff.get(at..at.checked_add(4)?)?.try_into().ok()?;
            Some(if big_endian {
                u32::from_be_bytes(bytes)
            } else {
                u32::from_le_bytes(bytes)
            })
        };
        if u16_at(2)? != 42 {
            return None;
        }
        let directory = usize::try_from(u32_at(4)?).ok()?;
        if directory < TIFF_HEADER {
            return None;
        }
        let count = usize::from(u16_at(directory)?);
        for index in 0..count {
            let entry = directory + 2 + index * ENTRY;
            if entry.checked_add(ENTRY)? > tiff.len() {
                return None;
            }
            if u16_at(entry)? == ORIENTATION_TAG {
                // A value of four bytes or fewer sits in the entry itself,
                // left-justified: a lone SHORT is its first two bytes.
                return (u16_at(entry + 2)? == SHORT && u32_at(entry + 4)? == 1)
                    .then(|| u16_at(entry + 8))
                    .flatten();
            }
        }
        None
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A 3 × 2 image whose pixels are numbered 0 to 5 in their red channel,
    /// row by row.
    fn numbered() -> Rgba8Image {
        let pixels = (0..6u8).flat_map(|n| [n, 0, 0, 255]).collect();
        Rgba8Image::new(3, 2, pixels).expect("sized")
    }

    fn reds(image: &Rgba8Image) -> Vec<u8> {
        image
            .pixels()
            .as_chunks::<4>()
            .0
            .iter()
            .map(|p| p[0])
            .collect()
    }

    /// Each orientation of the numbered image, worked from the TIFF 6.0
    /// wording: where the stored first row (0 1 2) and first column (0 3)
    /// end up.
    #[test]
    fn each_orientation_moves_the_first_row_and_column_where_tiff_says() {
        for (value, size, upright) in [
            // Row 0 at the top, column 0 at the left.
            (1, (3, 2), [0, 1, 2, 3, 4, 5]),
            // Row 0 at the top, column 0 at the right.
            (2, (3, 2), [2, 1, 0, 5, 4, 3]),
            // Row 0 at the bottom, column 0 at the right.
            (3, (3, 2), [5, 4, 3, 2, 1, 0]),
            // Row 0 at the bottom, column 0 at the left.
            (4, (3, 2), [3, 4, 5, 0, 1, 2]),
            // Row 0 at the left, column 0 at the top.
            (5, (2, 3), [0, 3, 1, 4, 2, 5]),
            // Row 0 at the right, column 0 at the top.
            (6, (2, 3), [3, 0, 4, 1, 5, 2]),
            // Row 0 at the right, column 0 at the bottom.
            (7, (2, 3), [5, 2, 4, 1, 3, 0]),
            // Row 0 at the left, column 0 at the bottom.
            (8, (2, 3), [2, 5, 1, 4, 0, 3]),
        ] {
            let orientation = Orientation::from_exif(value);
            assert_eq!(orientation.exif_value(), value);
            let image = orientation
                .apply(numbered(), &Limits::DEFAULT)
                .expect("applies");
            assert_eq!((image.width(), image.height()), size, "{value}");
            assert_eq!(orientation.upright_size((3, 2)), size, "{value}");
            assert_eq!(reds(&image), upright, "{value}");
        }
        for value in [0, 9, u16::MAX] {
            assert_eq!(Orientation::from_exif(value), Orientation::Normal);
        }
    }

    /// A tall image within limits that are wider than tall is refused once
    /// it is turned, before the second buffer.
    #[test]
    fn the_upright_size_passes_the_limits() {
        let limits = Limits {
            max_width: 3,
            max_height: 2,
            max_bytes: 24,
        };
        for value in 1..=8 {
            let orientation = Orientation::from_exif(value);
            let result = orientation.apply(numbered(), &limits);
            if orientation.swaps_sides() {
                assert_eq!(
                    result,
                    Err(ImageError::TooLarge {
                        width: 2,
                        height: 3,
                        limits
                    }),
                    "{value}"
                );
            } else {
                assert!(result.is_ok(), "{value}");
            }
        }
    }

    /// A TIFF block whose first directory holds one entry, `Orientation`
    /// of type `kind`, `count` and `value`, in either byte order.
    #[cfg(any(feature = "png", feature = "jpeg", feature = "webp"))]
    fn tiff(big_endian: bool, kind: u16, count: u32, value: u16) -> Vec<u8> {
        let u16b = |v: u16| {
            if big_endian {
                v.to_be_bytes()
            } else {
                v.to_le_bytes()
            }
        };
        let u32b = |v: u32| {
            if big_endian {
                v.to_be_bytes()
            } else {
                v.to_le_bytes()
            }
        };
        let mut block = if big_endian {
            b"MM".to_vec()
        } else {
            b"II".to_vec()
        };
        block.extend_from_slice(&u16b(42));
        block.extend_from_slice(&u32b(8));
        block.extend_from_slice(&u16b(1));
        block.extend_from_slice(&u16b(ORIENTATION_TAG));
        block.extend_from_slice(&u16b(kind));
        block.extend_from_slice(&u32b(count));
        block.extend_from_slice(&u16b(value));
        block.extend_from_slice(&[0, 0]);
        block.extend_from_slice(&u32b(0));
        block
    }

    #[cfg(any(feature = "png", feature = "jpeg", feature = "webp"))]
    #[test]
    fn the_tag_reads_in_either_byte_order_with_or_without_the_identifier() {
        for big_endian in [false, true] {
            for value in 1..=8 {
                let block = tiff(big_endian, SHORT, 1, value);
                assert_eq!(Orientation::read_exif(&block).exif_value(), value);
                let mut prefixed = EXIF_IDENTIFIER.to_vec();
                prefixed.extend_from_slice(&block);
                assert_eq!(Orientation::read_exif(&prefixed).exif_value(), value);
            }
        }
    }

    /// Damage of every kind reads as normal, and nothing panics: every cut
    /// of a good block, a wrong type or count, an out-of-range value, a
    /// directory past the block or inside the header, and a directory
    /// claiming more entries than there are.
    #[cfg(any(feature = "png", feature = "jpeg", feature = "webp"))]
    #[test]
    fn a_damaged_block_is_normal_not_an_error() {
        let good = tiff(false, SHORT, 1, 6);
        assert_eq!(
            Orientation::read_exif(&good),
            Orientation::Rotate90Clockwise
        );
        let mut damaged: Vec<Vec<u8>> = (0..good.len() - 4)
            .map(|end| good[..end].to_vec())
            .collect();
        damaged.push(tiff(false, 4, 1, 6));
        damaged.push(tiff(false, SHORT, 2, 6));
        damaged.push(tiff(true, SHORT, 1, 0));
        damaged.push(tiff(true, SHORT, 1, 9));
        let mut far = good.clone();
        far[4..8].copy_from_slice(&u32::MAX.to_le_bytes());
        damaged.push(far);
        let mut inside = good.clone();
        inside[4..8].copy_from_slice(&2u32.to_le_bytes());
        damaged.push(inside);
        let mut many = good.clone();
        // A directory of 65535 entries, the first not the tag, in a block
        // of one: the walk stops at the block's end.
        many[8..10].copy_from_slice(&u16::MAX.to_le_bytes());
        many[10..12].copy_from_slice(&0x0100u16.to_le_bytes());
        damaged.push(many);
        let mut wrong_magic = good.clone();
        wrong_magic[2] = 43;
        damaged.push(wrong_magic);
        damaged.push(b"XX*\0\x08\0\0\0".to_vec());
        for block in damaged {
            assert_eq!(
                Orientation::read_exif(&block),
                Orientation::Normal,
                "{block:?}"
            );
        }
    }
}
