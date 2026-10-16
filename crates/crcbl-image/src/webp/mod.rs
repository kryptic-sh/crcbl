//! WebP, written here: the RIFF container and the lossless (`VP8L`)
//! bitstream of RFC 9649, decoded to RGBA8.
//!
//! # What it reads
//!
//! A simple lossless file (`VP8L` alone), an extended one (`VP8X` with a
//! `VP8L` chunk), and the first frame of an animated one, placed where its
//! `ANMF` chunk puts it on a transparent canvas of the size the `VP8X` chunk
//! declares — the canvas's background colour is a hint the specification lets
//! a reader ignore, and libwebp's animation decoder ignores it too. Every part
//! of the lossless bitstream: the predictor, colour, subtract-green and
//! colour-indexing transforms (with pixel bundling), prefix codes in both
//! code-length forms, meta prefix codes through an entropy image, LZ77
//! backward references with the distance map, and the colour cache.
//!
//! Lossy WebP (`VP8 ` and its `ALPH` alpha chunk) is
//! [`ImageError::Unsupported`]: its decoder is RFC 6386's VP8, a codec of its
//! own.
//!
//! # Hostile input
//!
//! The image's size passes the caller's [`Limits`] before anything is sized
//! from it, a prefix code keeps only the symbols it has (so a stream cannot
//! claim tables larger than the bits it spent describing them), and reading
//! past the end of the stream is [`ImageError::Malformed`] rather than a run
//! of zeros.

mod lossless;

use crate::{ImageError, Limits, Rgba8Image};

fn malformed(why: impl Into<String>) -> ImageError {
    ImageError::Malformed(why.into())
}

fn unsupported(why: impl Into<String>) -> ImageError {
    ImageError::Unsupported(why.into())
}

/// Decodes a WebP file to straight RGBA8.
///
/// # Errors
///
/// [`ImageError::Empty`] or [`ImageError::TooLarge`] if its size breaks
/// `limits`, [`ImageError::Unsupported`] for lossy WebP, and
/// [`ImageError::Malformed`] for anything that is not a well-formed WebP.
pub fn decode(bytes: &[u8], limits: &Limits) -> Result<Rgba8Image, ImageError> {
    if bytes.get(..4) != Some(b"RIFF") || bytes.get(8..12) != Some(b"WEBP") {
        return Err(malformed("not a RIFF WebP file"));
    }
    let riff_size = u32_at(bytes, 4)? as usize;
    let body = bytes
        .get(12..(8 + riff_size).min(bytes.len()))
        .ok_or_else(|| malformed("a RIFF size past the file"))?;
    // A still image is a `VP8L` chunk, alone or after a `VP8X` chunk naming
    // its canvas; an animation's first `ANMF` chunk holds its first frame.
    let mut canvas = None;
    for chunk in Chunks::new(body) {
        let (fourcc, data) = chunk?;
        match &fourcc {
            b"VP8X" => {
                let width = u24_at(data, 4)? + 1;
                let height = u24_at(data, 7)? + 1;
                canvas = Some((width, height));
            }
            b"VP8L" => {
                let image = lossless::decode_vp8l(data, limits)?;
                if canvas.is_some_and(|size| size != (image.width(), image.height())) {
                    return Err(malformed("a still image the size its canvas is not"));
                }
                return Ok(image);
            }
            b"ANMF" => {
                let (width, height) =
                    canvas.ok_or_else(|| malformed("an animation frame without a VP8X chunk"))?;
                return first_frame(data, width, height, limits);
            }
            b"VP8 " | b"ALPH" => return Err(unsupported("lossy WebP")),
            _ => {}
        }
    }
    Err(malformed("no image data"))
}

/// The first frame of an animation, placed on a transparent canvas.
fn first_frame(
    anmf: &[u8],
    canvas_width: u32,
    canvas_height: u32,
    limits: &Limits,
) -> Result<Rgba8Image, ImageError> {
    let canvas_bytes = limits.rgba_bytes(canvas_width, canvas_height)?;
    let x = u24_at(anmf, 0)? * 2;
    let y = u24_at(anmf, 3)? * 2;
    let frame_data = anmf
        .get(16..)
        .ok_or_else(|| malformed("an animation frame is cut short"))?;
    let mut bitstream = None;
    for chunk in Chunks::new(frame_data) {
        let (fourcc, data) = chunk?;
        match &fourcc {
            b"VP8L" => {
                bitstream = Some(data);
                break;
            }
            b"VP8 " | b"ALPH" => return Err(unsupported("lossy WebP")),
            _ => {}
        }
    }
    let frame = lossless::decode_vp8l(
        bitstream.ok_or_else(|| malformed("an animation frame with no image"))?,
        limits,
    )?;
    let (frame_width, frame_height) = (frame.width(), frame.height());
    if u64::from(x) + u64::from(frame_width) > u64::from(canvas_width)
        || u64::from(y) + u64::from(frame_height) > u64::from(canvas_height)
    {
        return Err(malformed("an animation frame outside its canvas"));
    }
    let mut pixels = vec![0u8; canvas_bytes];
    let row = frame_width as usize * 4;
    for (row_index, source) in frame.pixels().chunks_exact(row).enumerate() {
        let start = ((y as usize + row_index) * canvas_width as usize + x as usize) * 4;
        pixels[start..start + row].copy_from_slice(source);
    }
    Ok(Rgba8Image::from_sized(canvas_width, canvas_height, pixels))
}

/// The chunks of a RIFF body, each its FourCC and its data, padding skipped.
struct Chunks<'a> {
    data: &'a [u8],
    at: usize,
}

impl<'a> Chunks<'a> {
    fn new(data: &'a [u8]) -> Self {
        Self { data, at: 0 }
    }
}

impl<'a> Iterator for Chunks<'a> {
    type Item = Result<([u8; 4], &'a [u8]), ImageError>;

    fn next(&mut self) -> Option<Self::Item> {
        if self.at + 8 > self.data.len() {
            return None;
        }
        let header = &self.data[self.at..self.at + 8];
        let fourcc = [header[0], header[1], header[2], header[3]];
        let size = u32::from_le_bytes([header[4], header[5], header[6], header[7]]) as usize;
        let start = self.at + 8;
        let Some(data) = start
            .checked_add(size)
            .and_then(|end| self.data.get(start..end))
        else {
            self.at = self.data.len();
            return Some(Err(malformed("a chunk runs past its file")));
        };
        // A chunk of odd size is followed by one padding byte.
        self.at = start + size + size % 2;
        Some(Ok((fourcc, data)))
    }
}

fn u32_at(bytes: &[u8], at: usize) -> Result<u32, ImageError> {
    bytes
        .get(at..at + 4)
        .map(|b| u32::from_le_bytes([b[0], b[1], b[2], b[3]]))
        .ok_or_else(|| malformed("a header is cut short"))
}

fn u24_at(bytes: &[u8], at: usize) -> Result<u32, ImageError> {
    bytes
        .get(at..at + 3)
        .map(|b| u32::from_le_bytes([b[0], b[1], b[2], 0]))
        .ok_or_else(|| malformed("a header is cut short"))
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Each fixture beside libwebp's decode of it, through Pillow 12.3, made
    /// by a script that saves seeded images with Pillow's lossless encoder:
    /// noise with varied alpha at its slowest and its fastest settings, an
    /// opaque one, a smooth gradient, images of 2, 4, 16 and 200 colours (each
    /// pixel bundling width and none), a single pixel, a two-frame animation
    /// whose first frame sits off the canvas's corner, and a near-grey image,
    /// whose channels move together, which the encoder codes with the
    /// subtract-green transform.
    macro_rules! fixture {
        ($name:literal) => {
            (
                $name,
                include_bytes!(concat!("../../fixtures/webp/", $name, ".webp")) as &[u8],
                include_bytes!(concat!("../../fixtures/webp/", $name, ".rgba")) as &[u8],
            )
        };
    }

    const FIXTURES: [(&str, &[u8], &[u8]); 11] = [
        fixture!("noise"),
        fixture!("noise-fast"),
        fixture!("opaque"),
        fixture!("gradient"),
        fixture!("two-colours"),
        fixture!("four-colours"),
        fixture!("sixteen-colours"),
        fixture!("many-colours"),
        fixture!("one-pixel"),
        fixture!("animated"),
        fixture!("grey"),
    ];

    #[test]
    fn every_fixture_decodes_to_exactly_libwebps_pixels() {
        for (name, webp, rgba) in FIXTURES {
            let image = decode(webp, &Limits::DEFAULT).unwrap_or_else(|e| panic!("{name}: {e}"));
            assert_eq!(image.pixels().len(), rgba.len(), "{name}: size");
            let first = image
                .pixels()
                .as_chunks::<4>()
                .0
                .iter()
                .zip(rgba.as_chunks::<4>().0)
                .position(|(ours, theirs)| ours != theirs);
            assert_eq!(first, None, "{name}: the first differing pixel");
        }
    }

    #[test]
    fn lossy_webp_is_unsupported_and_damage_is_malformed() {
        let mut lossy = b"RIFF\x0c\0\0\0WEBPVP8 \0\0\0\0".to_vec();
        lossy[4] = (lossy.len() - 8) as u8;
        assert!(matches!(
            decode(&lossy, &Limits::DEFAULT),
            Err(ImageError::Unsupported(_))
        ));
        let (_, webp, _) = FIXTURES[0];
        for bytes in [
            &b""[..],
            b"RIFF\0\0\0\0WEBP",
            &webp[..webp.len() / 2],
            &webp[..30],
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
        let small = Limits {
            max_width: 4,
            max_height: 4,
            max_bytes: 64,
        };
        assert!(matches!(
            decode(webp, &small),
            Err(ImageError::TooLarge { .. })
        ));
    }
}
