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
//! # Orientation
//!
//! An `EXIF` chunk's orientation is applied by [`decode`] and left alone by
//! [`decode_stored`], as the [crate docs](crate) say, to the whole image: an
//! animation's canvas. The chunk counts only when the `VP8X` chunk's EXIF
//! flag says it is there, as libwebp's demuxer (and so Pillow) reads it.
//! An `XMP ` chunk's `tiff:Orientation`, which Pillow falls back to, is not
//! read ([EXIF only](crate::orientation#exif-only)).
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

use crate::{ImageError, Limits, Orientation, Rgba8Image};

/// The `VP8X` flag saying the file has an `EXIF` chunk.
const EXIF_FLAG: u8 = 0x08;

fn malformed(why: impl Into<String>) -> ImageError {
    ImageError::Malformed(why.into())
}

fn unsupported(why: impl Into<String>) -> ImageError {
    ImageError::Unsupported(why.into())
}

/// Decodes a WebP file to straight RGBA8, upright: [`decode_stored`]'s
/// pixels with [`orientation`]'s flip or quarter turn applied, the upright
/// size checked against `limits` too.
///
/// # Errors
///
/// As [`decode_stored`], and [`ImageError::TooLarge`] if the upright size
/// breaks `limits`.
pub fn decode(bytes: &[u8], limits: &Limits) -> Result<Rgba8Image, ImageError> {
    let image = decode_stored(bytes, limits)?;
    orientation(bytes).apply(image, limits)
}

/// Decodes a WebP file to straight RGBA8 as stored, any EXIF orientation not
/// applied.
///
/// # Errors
///
/// [`ImageError::Empty`] or [`ImageError::TooLarge`] if its size breaks
/// `limits`, [`ImageError::Unsupported`] for a reserved VP8 version or
/// alpha compression method, and [`ImageError::Malformed`] for anything that
/// is not a well-formed WebP.
pub fn decode_stored(bytes: &[u8], limits: &Limits) -> Result<Rgba8Image, ImageError> {
    match find_image(bytes)? {
        Image::Still {
            fourcc,
            data,
            alpha,
        } => decode_frame(&fourcc, data, alpha, limits),
        Image::Animated {
            canvas: (width, height),
            first,
        } => first_frame(first, width, height, limits),
    }
}

/// The width and height of the image [`decode_stored`] returns, read from
/// the headers without decoding a pixel: a still image's bitstream header,
/// or an animation's canvas. [`decode`] returns it too unless
/// [`orientation`] swaps its sides ([`Orientation::upright_size`]).
///
/// The container is walked as [`decode`] walks it, a still image's size is
/// held to its canvas's, and an animation's first frame is found and held
/// inside its canvas by its bitstream's header, as there. Every header is
/// refused as [`decode`] refuses it: the bitstream's, the alpha chunk's
/// (its compression method) and the first frame's. The bitstreams past
/// their headers are not read, so damage there is found only by decoding.
/// The size is not checked against any [`Limits`]: compare it with the
/// caller's, or ask [`Limits::rgba_bytes`].
///
/// # Errors
///
/// [`ImageError::Empty`] for a lossy frame of a zero side,
/// [`ImageError::Unsupported`] for a reserved VP8 version or alpha
/// compression method, and [`ImageError::Malformed`] if the bytes do not
/// reach well-formed image headers.
pub fn dimensions(bytes: &[u8]) -> Result<(u32, u32), ImageError> {
    match find_image(bytes)? {
        Image::Still {
            fourcc,
            data,
            alpha,
        } => frame_header(&fourcc, data, alpha),
        Image::Animated {
            canvas: (width, height),
            first,
        } => {
            first_frame_header(first, width, height)?;
            Ok((width, height))
        }
    }
}

/// The orientation a WebP's `EXIF` chunk records, read without decoding a
/// pixel: the chunk anywhere in the file, if the first chunk is a `VP8X`
/// whose EXIF flag is set.
///
/// Never an error: no chunk, a damaged one, damage in the chunks before it,
/// or a file that is not a WebP is [`Orientation::Normal`], as the
/// [`orientation`](crate::orientation) module docs say.
#[must_use]
pub fn orientation(bytes: &[u8]) -> Orientation {
    let Ok(body) = riff_body(bytes) else {
        return Orientation::Normal;
    };
    let mut chunks = Chunks::new(body);
    match chunks.next() {
        Some(Ok((fourcc, data)))
            if &fourcc == b"VP8X" && data.first().is_some_and(|flags| flags & EXIF_FLAG != 0) => {}
        _ => return Orientation::Normal,
    }
    for chunk in chunks {
        match chunk {
            Ok((fourcc, data)) if &fourcc == b"EXIF" => return Orientation::read_exif(data),
            Ok(_) => {}
            Err(_) => break,
        }
    }
    Orientation::Normal
}

/// The chunks of a RIFF WebP file, after its header, to the end its RIFF
/// size gives or the file's, whichever is first.
fn riff_body(bytes: &[u8]) -> Result<&[u8], ImageError> {
    if bytes.get(..4) != Some(b"RIFF") || bytes.get(8..12) != Some(b"WEBP") {
        return Err(malformed("not a RIFF WebP file"));
    }
    let riff_size = u32_at(bytes, 4)? as usize;
    bytes
        .get(12..riff_size.saturating_add(8).min(bytes.len()))
        .ok_or_else(|| malformed("a RIFF size past the file"))
}

/// Where a file's image is, as [`find_image`] finds it.
enum Image<'a> {
    /// A `VP8L` or `VP8 ` chunk's FourCC and data, and the `ALPH` chunk
    /// before it, if any.
    Still {
        fourcc: [u8; 4],
        data: &'a [u8],
        alpha: Option<&'a [u8]>,
    },
    /// The `VP8X` chunk's canvas size and the first `ANMF` chunk's data.
    Animated { canvas: (u32, u32), first: &'a [u8] },
}

/// Finds a file's image in its chunks: a still image is a `VP8L` or `VP8 `
/// chunk, alone or after a `VP8X` chunk naming its canvas (and, if lossy,
/// an `ALPH` chunk holding its alpha), whose size its header must match; an
/// animation's first `ANMF` chunk holds its first frame.
fn find_image(bytes: &[u8]) -> Result<Image<'_>, ImageError> {
    let body = riff_body(bytes)?;
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
                if canvas.is_some() && canvas != Some(frame_size(&fourcc, data)?) {
                    return Err(malformed("a still image the size its canvas is not"));
                }
                return Ok(Image::Still {
                    fourcc,
                    data,
                    alpha,
                });
            }
            b"ANMF" => {
                let canvas =
                    canvas.ok_or_else(|| malformed("an animation frame without a VP8X chunk"))?;
                return Ok(Image::Animated {
                    canvas,
                    first: data,
                });
            }
            _ => {}
        }
    }
    Err(malformed("no image data"))
}

/// The width and height a frame's bitstream header gives.
fn frame_size(fourcc: &[u8; 4], data: &[u8]) -> Result<(u32, u32), ImageError> {
    if fourcc == b"VP8L" {
        lossless::vp8l_size(data)
    } else {
        vp8::size(data)
    }
}

/// A frame's size from its headers, refused as [`decode_frame`] refuses
/// them: the bitstream's, and for a lossy frame the alpha chunk's.
fn frame_header(
    fourcc: &[u8; 4],
    data: &[u8],
    alpha: Option<&[u8]>,
) -> Result<(u32, u32), ImageError> {
    let (width, height) = frame_size(fourcc, data)?;
    let size = crate::non_empty(width, height)?;
    if fourcc == b"VP8 " {
        alpha.map(alpha::read_header).transpose()?;
    }
    Ok(size)
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
    let header = first_frame_header(anmf, canvas_width, canvas_height)?;
    let frame = decode_frame(&header.fourcc, header.data, header.alpha, limits)?;
    let (x, y) = (header.x as usize, header.y as usize);
    let mut pixels = vec![0u8; canvas_bytes];
    let row = frame.width() as usize * 4;
    for (row_index, source) in frame.pixels().chunks_exact(row).enumerate() {
        let start = ((y + row_index) * canvas_width as usize + x) * 4;
        pixels[start..start + row].copy_from_slice(source);
    }
    Ok(Rgba8Image::from_sized(canvas_width, canvas_height, pixels))
}

/// An animation frame's place on its canvas and its bitstream, as
/// [`first_frame_header`] finds them.
struct FrameHeader<'a> {
    x: u32,
    y: u32,
    fourcc: [u8; 4],
    data: &'a [u8],
    alpha: Option<&'a [u8]>,
}

/// An `ANMF` chunk's offset and bitstream, its headers refused as
/// [`frame_header`] refuses them, and the frame they size held inside the
/// canvas.
fn first_frame_header(
    anmf: &[u8],
    canvas_width: u32,
    canvas_height: u32,
) -> Result<FrameHeader<'_>, ImageError> {
    let x = u24_at(anmf, 0)? * 2;
    let y = u24_at(anmf, 3)? * 2;
    let frame_data = anmf
        .get(16..)
        .ok_or_else(|| malformed("an animation frame is cut short"))?;
    let mut alpha = None;
    for chunk in Chunks::new(frame_data) {
        let (fourcc, data) = chunk?;
        match &fourcc {
            b"ALPH" => alpha = Some(data),
            b"VP8L" | b"VP8 " => {
                let (width, height) = frame_header(&fourcc, data, alpha)?;
                if u64::from(x) + u64::from(width) > u64::from(canvas_width)
                    || u64::from(y) + u64::from(height) > u64::from(canvas_height)
                {
                    return Err(malformed("an animation frame outside its canvas"));
                }
                return Ok(FrameHeader {
                    x,
                    y,
                    fourcc,
                    data,
                    alpha,
                });
            }
            _ => {}
        }
    }
    Err(malformed("an animation frame with no image"))
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
    use std::mem::discriminant;

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

    #[test]
    fn dimensions_are_the_size_decode_returns() {
        for (name, webp, _) in FIXTURES.into_iter().chain(LOSSY) {
            let image = decode(webp, &Limits::DEFAULT).unwrap_or_else(|e| panic!("{name}: {e}"));
            assert_eq!(
                dimensions(webp),
                Ok((image.width(), image.height())),
                "{name}"
            );
        }
    }

    #[test]
    fn dimensions_refuse_what_decode_refuses_in_the_headers() {
        let (_, lossy, _) = LOSSY[0];
        let frame = &lossy[20..];
        let mut reserved = frame.to_vec();
        reserved[0] |= 5 << 1;
        let reserved = simple_lossy(&reserved);
        assert!(matches!(
            dimensions(&reserved),
            Err(ImageError::Unsupported(_))
        ));
        assert!(matches!(
            decode(&reserved, &Limits::DEFAULT),
            Err(ImageError::Unsupported(_))
        ));

        // Every cut of a simple file, an extended still image and an
        // animation leaves a chunk running past the file, or none at all,
        // but for dropping the padding byte after an odd-sized last chunk:
        // dimensions and decode give the same verdict on each.
        let (_, lossless, _) = FIXTURES[0];
        let (_, animated, _) = FIXTURES[9];
        let (_, extended, _) = LOSSY[14];
        assert_eq!(extended.get(12..16), Some(&b"VP8X"[..]), "a VP8X file");
        let mut cuts_refused = 0;
        for webp in [lossless, lossy, animated, extended] {
            for end in 0..webp.len() {
                let cut = &webp[..end];
                let decoded = decode(cut, &Limits::DEFAULT);
                let decoded = decoded.map(|image| (image.width(), image.height()));
                assert_eq!(
                    dimensions(cut).map_err(|e| discriminant(&e)),
                    decoded.map_err(|e| discriminant(&e)),
                    "cut at {end}"
                );
                cuts_refused += usize::from(dimensions(cut).is_err());
            }
        }
        assert!(cuts_refused > 0, "no cut was refused");
        let mut malformed: Vec<(String, Vec<u8>)> = Vec::new();
        // The `VP8X` chunk's canvas width, a pixel wider than its image.
        let mut wider = extended.to_vec();
        wider[24] = wider[24].wrapping_add(1);
        malformed.push(("a canvas the image does not fill".into(), wider));
        let mut interframe = frame.to_vec();
        interframe[0] |= 1;
        malformed.push(("an interframe".into(), simple_lossy(&interframe)));
        let mut version = lossless.to_vec();
        // The lossless signature, then 14 bits of width, 14 of height and
        // one of alpha: the version's three bits are the top of byte 24.
        version[24] |= 1 << 5;
        malformed.push(("a lossless version 1".into(), version));
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

        // A lossy frame of no width, the scale bits above it kept.
        let mut empty = frame.to_vec();
        empty[6] = 0;
        empty[7] &= 0xc0;
        let empty = simple_lossy(&empty);
        let refused = |result: Result<(u32, u32), ImageError>| {
            matches!(result, Err(ImageError::Empty { width: 0, .. }))
        };
        assert!(refused(dimensions(&empty)));
        assert!(refused(
            decode(&empty, &Limits::DEFAULT).map(|image| (image.width(), image.height()))
        ));
    }

    /// A chunk of `fourcc` around `data`, padded to an even size.
    fn chunk(fourcc: &[u8; 4], data: &[u8]) -> Vec<u8> {
        let mut chunk = fourcc.to_vec();
        chunk.extend_from_slice(&(data.len() as u32).to_le_bytes());
        chunk.extend_from_slice(data);
        if data.len() % 2 == 1 {
            chunk.push(0);
        }
        chunk
    }

    /// A RIFF WebP file of `chunks`, its size made to fit.
    fn riff(chunks: &[Vec<u8>]) -> Vec<u8> {
        let body = chunks.concat();
        let mut file = b"RIFF".to_vec();
        file.extend_from_slice(&((body.len() + 4) as u32).to_le_bytes());
        file.extend_from_slice(b"WEBP");
        file.extend_from_slice(&body);
        file
    }

    /// A `VP8X` chunk declaring a `width` × `height` canvas under `flags`.
    fn vp8x(flags: u8, width: u32, height: u32) -> Vec<u8> {
        let mut data = vec![flags, 0, 0, 0];
        data.extend_from_slice(&(width - 1).to_le_bytes()[..3]);
        data.extend_from_slice(&(height - 1).to_le_bytes()[..3]);
        chunk(b"VP8X", &data)
    }

    /// An `ANMF` chunk at the canvas's corner around `frame`'s chunks.
    fn anmf(frame: &[Vec<u8>]) -> Vec<u8> {
        let mut data = vec![0; 16];
        data.extend_from_slice(&frame.concat());
        chunk(b"ANMF", &data)
    }

    /// Headers decode refuses before a pixel, in the alpha chunk and in an
    /// animation's first frame: dimensions refuses each with decode's own
    /// error.
    #[test]
    fn dimensions_read_the_alpha_and_first_frame_headers() {
        let (_, lossy, _) = LOSSY[0];
        let frame = &lossy[20..];
        let (width, height) = vp8::size(frame).expect("a VP8 header");
        let mut reserved = frame.to_vec();
        reserved[0] |= 5 << 1;
        let alpha_flag = 0x10;
        let animation_flag = 0x02;
        let with_alpha = |alph: &[u8]| {
            riff(&[
                vp8x(alpha_flag, width, height),
                chunk(b"ALPH", alph),
                chunk(b"VP8 ", frame),
            ])
        };
        let animated = |canvas: (u32, u32), first: &[Vec<u8>]| {
            riff(&[vp8x(animation_flag, canvas.0, canvas.1), anmf(first)])
        };
        // Raw and unfiltered: the header byte, then a value a pixel.
        let mut raw = vec![u8::MAX; 1 + (width * height) as usize];
        raw[0] = 0;
        let good_alpha = with_alpha(&raw);
        assert!(decode(&good_alpha, &Limits::DEFAULT).is_ok());
        assert_eq!(dimensions(&good_alpha), Ok((width, height)));
        let good_animation = animated((width, height), &[chunk(b"VP8 ", frame)]);
        assert!(decode(&good_animation, &Limits::DEFAULT).is_ok());
        assert_eq!(dimensions(&good_animation), Ok((width, height)));
        for (what, bytes) in [
            ("alpha compression method 3", with_alpha(&[0x03])),
            ("an empty alpha chunk", with_alpha(&[])),
            ("a frame with no image", animated((1, 1), &[])),
            (
                "a frame outside its canvas",
                animated((1, 1), &[chunk(b"VP8 ", frame)]),
            ),
            (
                "a frame of a reserved VP8 version",
                animated((width, height), &[chunk(b"VP8 ", &reserved)]),
            ),
        ] {
            let decoded =
                decode(&bytes, &Limits::DEFAULT).map(|image| (image.width(), image.height()));
            assert!(decoded.is_err(), "{what}: decode");
            assert_eq!(dimensions(&bytes), decoded, "{what}");
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

    /// Pillow 13 wrote one 5 × 3 picture losslessly with EXIF orientation 1
    /// to 8; each is held to Pillow's `ImageOps.exif_transpose` of the PNG
    /// written beside it, which its decode of the WebP matched, as did
    /// ImageMagick's `-auto-orient`.
    macro_rules! oriented {
        ($value:literal) => {
            (
                $value,
                include_bytes!(concat!("../../fixtures/orientation/", $value, ".webp")) as &[u8],
                include_bytes!(concat!("../../fixtures/orientation/", $value, ".rgba")) as &[u8],
            )
        };
    }

    const ORIENTED: [(u16, &[u8], &[u8]); 8] = [
        oriented!(1),
        oriented!(2),
        oriented!(3),
        oriented!(4),
        oriented!(5),
        oriented!(6),
        oriented!(7),
        oriented!(8),
    ];

    #[test]
    fn exif_orientation_is_applied_as_pillow_and_imagemagick_apply_it() {
        let (_, _, stored) = ORIENTED[0];
        for (value, webp, upright) in ORIENTED {
            assert_eq!(orientation(webp).exif_value(), value);
            let image = decode(webp, &Limits::DEFAULT).expect("decodes");
            let size = Orientation::from_exif(value).upright_size((5, 3));
            assert_eq!((image.width(), image.height()), size, "{value}");
            assert_eq!(image.pixels(), upright, "{value}");
            let as_stored = decode_stored(webp, &Limits::DEFAULT).expect("decodes");
            assert_eq!(as_stored.pixels(), stored, "{value}: stored");
            assert_eq!(dimensions(webp), Ok((5, 3)), "{value}");
        }
    }

    /// The `EXIF` chunk counts only under the `VP8X` flag, as libwebp's
    /// demuxer reads it: Pillow reports no EXIF for the file with the flag
    /// cleared. Damage is normal, and no cut panics.
    #[test]
    fn exif_counts_under_its_flag_and_damage_is_normal() {
        let (_, six, _) = ORIENTED[5];
        let (_, _, stored) = ORIENTED[0];
        // The file header, then the VP8X chunk's header, then its flags.
        let flags = 12 + 8;
        assert_eq!(six.get(12..16), Some(&b"VP8X"[..]));
        let mut unflagged = six.to_vec();
        unflagged[flags] &= !EXIF_FLAG;
        assert_eq!(orientation(&unflagged), Orientation::Normal);
        let image = decode(&unflagged, &Limits::DEFAULT).expect("decodes");
        assert_eq!(image.pixels(), stored);
        let exif = six
            .windows(4)
            .position(|window| window == b"EXIF")
            .expect("an EXIF chunk");
        let mut broken = six.to_vec();
        broken[exif + 8] = b'Q';
        assert_eq!(orientation(&broken), Orientation::Normal);
        for cut in 0..six.len() {
            assert!(matches!(
                orientation(&six[..cut]),
                Orientation::Normal | Orientation::Rotate90Clockwise
            ));
        }
        // A simple file has no VP8X chunk, so no EXIF.
        let (_, lossless, _) = FIXTURES[0];
        assert_eq!(orientation(lossless), Orientation::Normal);
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
