//! ICO, written here: Windows icon files, the frame nearest a size decoded
//! to RGBA8.
//!
//! An icon file is a directory of frames, each a PNG or a headerless bitmap
//! with an AND mask (see [`crate::bmp`]). [`decode`] picks the frame to draw
//! at a size by [`resize::nearest_size`] — the
//! smallest at least that large, else the largest — and decodes only that
//! one. A cursor file (`.cur`) has the same layout and is read the same way.

use crate::{ImageError, Limits, Rgba8Image, bmp, png, resize};

/// The bytes of the directory header, before its entries.
const HEADER: usize = 6;

/// The bytes of one directory entry.
const ENTRY: usize = 16;

/// The PNG signature, which marks a frame stored as PNG.
const PNG_SIGNATURE: [u8; 8] = [0x89, b'P', b'N', b'G', 0x0d, 0x0a, 0x1a, 0x0a];

fn malformed(why: impl Into<String>) -> ImageError {
    ImageError::Malformed(why.into())
}

/// One frame's entry in the directory.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct Entry {
    width: u32,
    height: u32,
    offset: usize,
    size: usize,
}

/// The directory of frames.
fn entries(bytes: &[u8]) -> Result<Vec<Entry>, ImageError> {
    let header = bytes
        .get(..HEADER)
        .ok_or_else(|| malformed("an icon header is cut short"))?;
    let kind = u16::from_le_bytes([header[2], header[3]]);
    if header[..2] != [0, 0] || !matches!(kind, 1 | 2) {
        return Err(malformed("not an icon or cursor file"));
    }
    let count = usize::from(u16::from_le_bytes([header[4], header[5]]));
    if count == 0 {
        return Err(malformed("an icon file with no frames"));
    }
    let table = bytes
        .get(HEADER..HEADER + count * ENTRY)
        .ok_or_else(|| malformed("an icon directory is cut short"))?;
    Ok(table
        .as_chunks::<ENTRY>()
        .0
        .iter()
        .map(|entry| Entry {
            // A side of zero means 256, the largest a byte cannot hold.
            width: if entry[0] == 0 {
                256
            } else {
                u32::from(entry[0])
            },
            height: if entry[1] == 0 {
                256
            } else {
                u32::from(entry[1])
            },
            size: u32::from_le_bytes([entry[8], entry[9], entry[10], entry[11]]) as usize,
            offset: u32::from_le_bytes([entry[12], entry[13], entry[14], entry[15]]) as usize,
        })
        .collect())
}

/// The sizes of the frames an icon file holds, in its order.
///
/// # Errors
///
/// [`ImageError::Malformed`] if the bytes are not an icon or cursor file.
pub fn sizes(bytes: &[u8]) -> Result<Vec<(u32, u32)>, ImageError> {
    Ok(entries(bytes)?
        .iter()
        .map(|entry| (entry.width, entry.height))
        .collect())
}

/// Decodes the frame of an icon file to draw at `size` pixels.
///
/// # Errors
///
/// [`ImageError::Malformed`] if the bytes are not an icon file or the frame's
/// data is not where its entry says, and otherwise whatever decoding the
/// frame as a PNG or a bitmap gives.
pub fn decode(bytes: &[u8], size: u32, limits: &Limits) -> Result<Rgba8Image, ImageError> {
    let entries = entries(bytes)?;
    let sizes: Vec<(u32, u32)> = entries.iter().map(|e| (e.width, e.height)).collect();
    let chosen = resize::nearest_size(&sizes, size)
        .and_then(|index| entries.get(index))
        .ok_or_else(|| malformed("an icon file with no frames"))?;
    let data = chosen
        .offset
        .checked_add(chosen.size)
        .and_then(|end| bytes.get(chosen.offset..end))
        .ok_or_else(|| malformed("an icon frame is not where its entry says"))?;
    if data.starts_with(&PNG_SIGNATURE) {
        png::decode(data, limits)
    } else {
        bmp::decode_icon_dib(data, limits)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The 32 × 32 frame of an icon Pillow 12.3 wrote at 16, 32 and 48 pixels
    /// from seeded random pixels with varied alpha, as PNG frames and as
    /// bitmap frames, beside Pillow's decode of that frame.
    const FRAMES: [(&str, &[u8], &[u8]); 2] = [
        (
            "png",
            include_bytes!("../fixtures/bmp/frames-png.ico"),
            include_bytes!("../fixtures/bmp/frames-png.ico.rgba"),
        ),
        (
            "bmp",
            include_bytes!("../fixtures/bmp/frames-bmp.ico"),
            include_bytes!("../fixtures/bmp/frames-bmp.ico.rgba"),
        ),
    ];

    #[test]
    fn the_frame_nearest_the_size_is_the_one_decoded() {
        for (name, bytes, rgba) in FRAMES {
            assert_eq!(
                sizes(bytes),
                Ok(vec![(16, 16), (32, 32), (48, 48)]),
                "{name}"
            );
            for asked in [17, 32] {
                let image = decode(bytes, asked, &Limits::DEFAULT).expect("decodes");
                assert_eq!(
                    (image.width(), image.height()),
                    (32, 32),
                    "{name} at {asked}"
                );
                assert_eq!(image.pixels(), rgba, "{name} at {asked}");
            }
            let large = decode(bytes, 64, &Limits::DEFAULT).expect("decodes");
            assert_eq!(
                large.width(),
                48,
                "{name}: the largest when none is large enough"
            );
        }
    }

    #[test]
    fn a_damaged_directory_is_malformed() {
        let (_, bytes, _) = FRAMES[0];
        let mut moved = bytes.to_vec();
        // The 32-pixel frame's offset, pointed past the file.
        moved[HEADER + ENTRY + 12..HEADER + ENTRY + 16].copy_from_slice(&u32::MAX.to_le_bytes());
        for broken in [
            &b""[..],
            &[0, 0, 3, 0, 1, 0][..],
            &[0, 0, 1, 0, 0, 0][..],
            &moved[..],
        ] {
            assert!(
                matches!(
                    decode(broken, 32, &Limits::DEFAULT),
                    Err(ImageError::Malformed(_))
                ),
                "{} bytes",
                broken.len()
            );
        }
    }
}
