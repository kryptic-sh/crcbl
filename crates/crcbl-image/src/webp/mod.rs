//! WebP, written here: the RIFF container of RFC 9649, its lossless
//! (`VP8L`) bitstream, and lossy images — RFC 6386's VP8 key frames with
//! the container's `ALPH` alpha chunk — decoded to RGBA8.
//!
//! # What it reads
//!
//! A simple file (`VP8L` or `VP8 ` alone), an extended one (`VP8X` with a
//! `VP8L` chunk, or a `VP8 ` chunk after an optional `ALPH`), and the first
//! frame of an animated one, lossless or lossy, placed where its `ANMF`
//! chunk puts it on a transparent canvas of the size the `VP8X` chunk
//! declares — the canvas's background colour is a hint the specification
//! lets a reader ignore, and libwebp's animation decoder ignores it too.
//!
//! Every part of the lossless bitstream: the predictor, colour,
//! subtract-green and colour-indexing transforms (with pixel bundling),
//! prefix codes in both code-length forms, meta prefix codes through an
//! entropy image, LZ77 backward references with the distance map, and the
//! colour cache.
//!
//! Every part of a lossy image: the VP8 key frame (`vp8`'s docs list what
//! that takes), its Y'CbCr turned to RGB as libwebp does it (`yuv`'s docs
//! say how, and why it matches libwebp exactly), and its alpha raw or
//! losslessly coded, with any of the four filters (`alpha`'s docs).
//!
//! # Hostile input
//!
//! The image's size passes the caller's [`Limits`] before anything is sized
//! from it, a prefix code keeps only the symbols it has (so a stream cannot
//! claim tables larger than the bits it spent describing them), and reading
//! past the end of a lossless stream or a lossy partition is
//! [`ImageError::Malformed`] rather than a run of zeros.

mod alpha;
mod lossless;
mod vp8;
mod yuv;

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
/// `limits`, [`ImageError::Unsupported`] for a reserved VP8 version or
/// alpha compression method, and [`ImageError::Malformed`] for anything that
/// is not a well-formed WebP.
pub fn decode(bytes: &[u8], limits: &Limits) -> Result<Rgba8Image, ImageError> {
    if bytes.get(..4) != Some(b"RIFF") || bytes.get(8..12) != Some(b"WEBP") {
        return Err(malformed("not a RIFF WebP file"));
    }
    let riff_size = u32_at(bytes, 4)? as usize;
    let body = bytes
        .get(12..riff_size.saturating_add(8).min(bytes.len()))
        .ok_or_else(|| malformed("a RIFF size past the file"))?;
    // A still image is a `VP8L` or `VP8 ` chunk, alone or after a `VP8X`
    // chunk naming its canvas (and, if lossy, an `ALPH` chunk holding its
    // alpha); an animation's first `ANMF` chunk holds its first frame.
    let mut canvas = None;
    let mut alpha = None;
    for chunk in Chunks::new(body) {
        let (fourcc, data) = chunk?;
        match &fourcc {
            b"VP8X" => {
                let width = u24_at(data, 4)? + 1;
                let height = u24_at(data, 7)? + 1;
                canvas = Some((width, height));
            }
            b"ALPH" => alpha = Some(data),
            b"VP8L" | b"VP8 " => {
                let image = decode_frame(&fourcc, data, alpha, limits)?;
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
            _ => {}
        }
    }
    Err(malformed("no image data"))
}

/// Decodes one frame's bitstream chunk: lossless, or lossy with the alpha
/// chunk that came before it. An alpha chunk beside a lossless bitstream is
/// ignored: the RFC says it should not be there, and the lossless image
/// carries its own alpha.
fn decode_frame(
    fourcc: &[u8; 4],
    data: &[u8],
    alpha: Option<&[u8]>,
    limits: &Limits,
) -> Result<Rgba8Image, ImageError> {
    if fourcc == b"VP8L" {
        return lossless::decode_vp8l(data, limits);
    }
    let picture = vp8::decode(data, limits)?;
    let (width, height) = (picture.width, picture.height);
    let alpha = alpha
        .map(|chunk| alpha::decode(chunk, width, height))
        .transpose()?;
    let mut rgba = vec![u8::MAX; limits.rgba_bytes(width as u32, height as u32)?];
    yuv::to_rgba(&picture, &mut rgba);
    if let Some(alpha) = alpha {
        for (pixel, alpha) in rgba.as_chunks_mut::<4>().0.iter_mut().zip(alpha) {
            pixel[3] = alpha;
        }
    }
    Ok(Rgba8Image::from_sized(width as u32, height as u32, rgba))
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
    let mut alpha = None;
    let mut frame = None;
    for chunk in Chunks::new(frame_data) {
        let (fourcc, data) = chunk?;
        match &fourcc {
            b"ALPH" => alpha = Some(data),
            b"VP8L" | b"VP8 " => {
                frame = Some(decode_frame(&fourcc, data, alpha, limits)?);
                break;
            }
            _ => {}
        }
    }
    let frame = frame.ok_or_else(|| malformed("an animation frame with no image"))?;
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
        for (name, webp, rgba) in FIXTURES.into_iter().chain(LOSSY) {
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

    /// The lossy fixtures beside libwebp's decode of each, through Pillow
    /// 12.3 (`convert("RGBA")`), made by a script that draws a seeded scene —
    /// a gradient, shapes and a noisy band, so the encoder predicts whole
    /// blocks and subblocks and segments the frame — at odd sizes so the
    /// macroblock padding is cropped, and saves it with Pillow's lossy
    /// encoder: at qualities 75, 100 (no loop filter), 0 (the highest
    /// filter level), 30 and 80, one pixel, 96x64; alpha coded losslessly,
    /// level-reduced (`alpha_quality` 20) and raw; and a two-frame animation
    /// whose first frame, with alpha, sits off the canvas's corner. Between
    /// them they use every luma, subblock and chroma mode, every token, and
    /// coefficient probability updates.
    ///
    /// What libwebp's encoder never writes, the script makes by rewriting its
    /// output with RFC 6386's boolean encoder, replaying every macroblock
    /// decision at the probability it was read at under a changed header:
    /// the simple filter at sharpness 2, a frame filter level of zero over
    /// segments with levels of their own, sharpness 6 with filter deltas,
    /// relative segment values, a skipped macroblock, a skipped B_PRED
    /// macroblock (whose coefficients the rewrite drops, which changes the
    /// picture), and four token partitions. The alpha chunk of one file is likewise rewritten raw
    /// under each of the three filters. libwebp decodes every rewritten file
    /// for its reference, so each is held to libwebp, not to the script.
    const LOSSY: [(&str, &[u8], &[u8]); 21] = [
        fixture!("lossy"),
        fixture!("lossy-best"),
        fixture!("lossy-worst"),
        fixture!("lossy-q30"),
        fixture!("lossy-small"),
        fixture!("lossy-pixel"),
        fixture!("lossy-large"),
        fixture!("lossy-simple"),
        fixture!("lossy-level-zero"),
        fixture!("lossy-empty-subblocks"),
        fixture!("lossy-sharp"),
        fixture!("lossy-relative"),
        fixture!("lossy-skip"),
        fixture!("lossy-partitions"),
        fixture!("lossy-alpha"),
        fixture!("lossy-alpha-levels"),
        fixture!("lossy-alpha-raw"),
        fixture!("lossy-alpha-horizontal"),
        fixture!("lossy-alpha-vertical"),
        fixture!("lossy-alpha-gradient"),
        fixture!("lossy-animated"),
    ];

    #[test]
    fn damage_is_malformed_and_a_size_past_the_limits_is_refused() {
        let small = Limits {
            max_width: 4,
            max_height: 4,
            max_bytes: 64,
        };
        for (_, webp, _) in [FIXTURES[0], LOSSY[0]] {
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
            assert!(matches!(
                decode(webp, &small),
                Err(ImageError::TooLarge { .. })
            ));
        }
    }

    /// The largest RIFF size a header can hold reads to the file's end, as a
    /// size past the end always does, rather than overflowing where `usize`
    /// is 32 bits.
    #[test]
    fn the_largest_riff_size_reads_to_the_end_of_the_file() {
        for (name, webp, rgba) in [FIXTURES[0], LOSSY[0]] {
            let mut file = webp.to_vec();
            file[4..8].copy_from_slice(&u32::MAX.to_le_bytes());
            let image = decode(&file, &Limits::DEFAULT).unwrap_or_else(|e| panic!("{name}: {e}"));
            assert_eq!(image.pixels(), rgba, "{name}");
        }
    }

    /// A simple lossy file around `vp8`, its sizes made to fit.
    fn simple_lossy(vp8: &[u8]) -> Vec<u8> {
        let mut file = b"RIFF\0\0\0\0WEBPVP8 ".to_vec();
        file.extend_from_slice(&(vp8.len() as u32).to_le_bytes());
        file.extend_from_slice(vp8);
        if vp8.len() % 2 == 1 {
            file.push(0);
        }
        let riff = (file.len() - 8) as u32;
        file[4..8].copy_from_slice(&riff.to_le_bytes());
        file
    }

    /// Damage inside a well-framed `VP8 ` chunk: a partition that runs out,
    /// a header cut short, an interframe, a first partition longer than its
    /// chunk, and a reserved version, which is unsupported rather than
    /// malformed.
    #[test]
    fn a_damaged_vp8_frame_is_refused() {
        let (_, webp, _) = LOSSY[0];
        // A simple file: the frame is the one chunk, after the file header and
        // its own.
        let frame = &webp[20..];
        assert!(decode(&simple_lossy(frame), &Limits::DEFAULT).is_ok());
        let mut interframe = frame.to_vec();
        interframe[0] |= 1;
        let mut long_first = frame.to_vec();
        long_first[1] = 0xff;
        long_first[2] = 0xff;
        for (what, vp8) in [
            ("tokens cut short", &frame[..frame.len() * 3 / 4]),
            ("header cut short", &frame[..9]),
            ("an interframe", &interframe[..]),
            ("a first partition past its chunk", &long_first[..]),
        ] {
            assert!(
                matches!(
                    decode(&simple_lossy(vp8), &Limits::DEFAULT),
                    Err(ImageError::Malformed(_))
                ),
                "{what}"
            );
        }
        let mut reserved = frame.to_vec();
        reserved[0] |= 5 << 1;
        assert!(matches!(
            decode(&simple_lossy(&reserved), &Limits::DEFAULT),
            Err(ImageError::Unsupported(_))
        ));
    }
}
