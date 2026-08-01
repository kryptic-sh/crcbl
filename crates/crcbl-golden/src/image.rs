//! An 8-bit RGBA image, and PNG on both sides of it.
//!
//! The one storage format is RGBA8. A readback arrives in whatever the
//! swapchain's format was — BGRA on most surfaces — so the conversion happens
//! once, at the boundary, rather than being a flag every comparison has to
//! respect. That also means a golden PNG is a *display-referred* file anyone can
//! open, which is what makes a CI artifact worth uploading.

use std::path::Path;

/// The largest image this crate will allocate for, in pixels.
///
/// `2^28` px is 16384×16384 — the `maxImageDimension2D` every Vulkan
/// implementation the engine targets guarantees, squared — and one gibibyte of
/// RGBA8. The bound exists because a PNG's *declared* size is attacker-chosen
/// and free to write: a hundred-byte file whose IHDR says 50000×50000 asks for
/// a ten-gigabyte buffer before a single IDAT byte has been inflated. Every
/// allocation in this module is checked against it.
pub const MAX_PIXELS: u64 = 1 << 28;

/// The budget handed to `png`'s own decoder for its internal buffers.
///
/// Pinned rather than left at `png::Limits::default()` so a version bump cannot
/// silently change it. It covers row and chunk buffers only — *not* the output
/// buffer, which is this module's job and is bounded by [`MAX_PIXELS`].
const MAX_DECODER_BYTES: usize = 64 * 1024 * 1024;

/// What can go wrong reading or writing an image.
#[derive(Debug)]
pub enum ImageError {
    /// The file could not be read or written.
    Io(std::io::Error),
    /// The PNG could not be decoded, or is a kind this crate does not model.
    Decode(String),
    /// The pixel buffer's length does not match its declared size.
    Size {
        /// Bytes expected, from `width * height * 4`.
        expected: usize,
        /// Bytes supplied.
        actual: usize,
    },
    /// The declared dimensions are larger than [`MAX_PIXELS`], or their byte
    /// count does not fit this machine's address space.
    ///
    /// Returned *before* anything is allocated, which is the entire point.
    TooLarge {
        /// Declared width, in pixels.
        width: u32,
        /// Declared height, in pixels.
        height: u32,
        /// The budget that was exceeded, in pixels. [`MAX_PIXELS`].
        max_pixels: u64,
    },
}

impl std::fmt::Display for ImageError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Io(error) => write!(f, "{error}"),
            Self::Decode(message) => write!(f, "{message}"),
            Self::Size { expected, actual } => write!(
                f,
                "a pixel buffer of {actual} bytes cannot be an image needing {expected}"
            ),
            Self::TooLarge {
                width,
                height,
                max_pixels,
            } => write!(
                f,
                "a {width}x{height} image is more than the {max_pixels} pixels this crate \
                 will allocate for"
            ),
        }
    }
}

/// `width * height * 4`, refusing anything past [`MAX_PIXELS`].
///
/// The multiplication is done in `u64`, where two `u32`s cannot overflow, and
/// the narrowing to `usize` is checked — `width as usize * height as usize * 4`
/// wraps on a 32-bit target and on a 64-bit one produces a length no allocator
/// can serve.
fn checked_byte_count(width: u32, height: u32) -> Result<usize, ImageError> {
    let too_large = || ImageError::TooLarge {
        width,
        height,
        max_pixels: MAX_PIXELS,
    };
    // Two `u32`s multiplied in `u64` cannot overflow, and `MAX_PIXELS * 4` is
    // `2^30`, so neither can the byte count.
    let pixels = u64::from(width) * u64::from(height);
    if pixels > MAX_PIXELS {
        return Err(too_large());
    }
    usize::try_from(pixels * 4).map_err(|_| too_large())
}

impl std::error::Error for ImageError {}

impl From<std::io::Error> for ImageError {
    fn from(error: std::io::Error) -> Self {
        Self::Io(error)
    }
}

/// The channel order a readback's bytes are in.
///
/// Named after the *memory* order, which is what a Vulkan format literally
/// describes and what a caller can read off `Format::Bgra8Unorm` without having
/// to reason about anything.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ChannelOrder {
    /// Red, green, blue, alpha.
    Rgba,
    /// Blue, green, red, alpha — what most surfaces actually hand back.
    Bgra,
}

/// An 8-bit RGBA image.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Image {
    width: u32,
    height: u32,
    /// `width * height * 4` bytes, row-major, top row first.
    pixels: Vec<u8>,
}

impl Image {
    /// Wraps a buffer that is already RGBA8.
    ///
    /// # Errors
    ///
    /// [`ImageError::Size`] if the buffer is not exactly `width * height * 4`
    /// bytes — the mistake a row-pitch bug produces, and one that would
    /// otherwise show up as a comparison against garbage — or
    /// [`ImageError::TooLarge`] if the dimensions exceed [`MAX_PIXELS`].
    pub fn from_rgba8(width: u32, height: u32, pixels: Vec<u8>) -> Result<Self, ImageError> {
        let expected = checked_byte_count(width, height)?;
        if pixels.len() != expected {
            return Err(ImageError::Size {
                expected,
                actual: pixels.len(),
            });
        }
        Ok(Self {
            width,
            height,
            pixels,
        })
    }

    /// Wraps a readback, converting from its memory channel order.
    ///
    /// # Errors
    ///
    /// As [`Image::from_rgba8`].
    pub fn from_readback(
        width: u32,
        height: u32,
        bytes: &[u8],
        order: ChannelOrder,
    ) -> Result<Self, ImageError> {
        let mut pixels = bytes.to_vec();
        if order == ChannelOrder::Bgra {
            for pixel in pixels.chunks_exact_mut(4) {
                pixel.swap(0, 2);
            }
        }
        Self::from_rgba8(width, height, pixels)
    }

    /// Width in pixels.
    #[must_use]
    pub const fn width(&self) -> u32 {
        self.width
    }

    /// Height in pixels.
    #[must_use]
    pub const fn height(&self) -> u32 {
        self.height
    }

    /// The RGBA8 bytes, row-major.
    #[must_use]
    pub fn pixels(&self) -> &[u8] {
        &self.pixels
    }

    /// The pixel at `(x, y)`, or `None` outside the image.
    #[must_use]
    pub fn pixel(&self, x: u32, y: u32) -> Option<[u8; 4]> {
        if x >= self.width || y >= self.height {
            return None;
        }
        let offset = (y as usize * self.width as usize + x as usize) * 4;
        Some([
            self.pixels[offset],
            self.pixels[offset + 1],
            self.pixels[offset + 2],
            self.pixels[offset + 3],
        ])
    }

    /// An image filled with one colour. For tests and for diff backgrounds.
    ///
    /// # Errors
    ///
    /// [`ImageError::TooLarge`] if the dimensions exceed [`MAX_PIXELS`]. It
    /// returns a `Result` rather than allocating on trust because the
    /// unchecked version wrapped its capacity on absurd dimensions and then
    /// looped `width * height` times pushing into it.
    pub fn filled(width: u32, height: u32, color: [u8; 4]) -> Result<Self, ImageError> {
        let byte_count = checked_byte_count(width, height)?;
        let mut pixels = vec![0u8; byte_count];
        for pixel in pixels.chunks_exact_mut(4) {
            pixel.copy_from_slice(&color);
        }
        Ok(Self {
            width,
            height,
            pixels,
        })
    }

    /// An image the same size as `model`, filled with one colour.
    ///
    /// Infallible where [`Image::filled`] is not: `model` is already a valid
    /// `Image`, so its own buffer length *is* the allocation needed and there
    /// is no arithmetic left to overflow.
    pub(crate) fn filled_like(model: &Self, color: [u8; 4]) -> Self {
        let mut pixels = vec![0u8; model.pixels.len()];
        for pixel in pixels.chunks_exact_mut(4) {
            pixel.copy_from_slice(&color);
        }
        Self {
            width: model.width,
            height: model.height,
            pixels,
        }
    }

    /// Writes an RGBA8 pixel, ignoring an out-of-range coordinate.
    pub fn set_pixel(&mut self, x: u32, y: u32, color: [u8; 4]) {
        if x >= self.width || y >= self.height {
            return;
        }
        let offset = (y as usize * self.width as usize + x as usize) * 4;
        self.pixels[offset..offset + 4].copy_from_slice(&color);
    }

    /// How many distinct RGBA colours the image contains, counting no further
    /// than `ceiling`.
    ///
    /// This is the **anti-vacuity** measurement, and it is why a tolerance can
    /// be widened without the gate quietly becoming decorative: two blank frames
    /// agree perfectly, so "the images match" is only evidence when the images
    /// are pictures of something. A cleared frame scores 1, a cleared frame with
    /// a HUD scores a handful, and the engine's lit cube scores dozens.
    ///
    /// `ceiling` stops the count early — a caller asserting "at least 16
    /// colours" does not need the exact answer for a 4K frame, and the early
    /// exit keeps the set small.
    #[must_use]
    pub fn distinct_colors(&self, ceiling: usize) -> usize {
        let mut seen: Vec<[u8; 4]> = Vec::new();
        for pixel in self.pixels.chunks_exact(4) {
            let color = [pixel[0], pixel[1], pixel[2], pixel[3]];
            if !seen.contains(&color) {
                seen.push(color);
                if seen.len() >= ceiling {
                    break;
                }
            }
        }
        seen.len()
    }

    /// Loads a PNG.
    ///
    /// # Errors
    ///
    /// [`ImageError::Io`] if the file is missing, [`ImageError::Decode`] if it
    /// is not a PNG this crate can read, or [`ImageError::TooLarge`] if its
    /// IHDR declares more than [`MAX_PIXELS`] pixels.
    pub fn load_png(path: impl AsRef<Path>) -> Result<Self, ImageError> {
        let file = std::fs::File::open(path.as_ref())?;
        let mut decoder = png::Decoder::new(std::io::BufReader::new(file));
        decoder.set_limits(png::Limits {
            bytes: MAX_DECODER_BYTES,
        });
        let mut reader = decoder
            .read_info()
            .map_err(|error| ImageError::Decode(error.to_string()))?;

        // `output_buffer_size` is computed from IHDR's width and height alone,
        // capped only at `isize::MAX`, and `png`'s own `Limits` budget covers
        // the decoder's internal buffers rather than this one. So the size is
        // whatever the file *claims*, and a hundred-byte file can claim ten
        // gigabytes. Bound it before allocating a single byte.
        let (declared_width, declared_height) = reader.info().size();
        let rgba_bytes = checked_byte_count(declared_width, declared_height)?;
        let output_bytes = reader.output_buffer_size().ok_or(ImageError::TooLarge {
            width: declared_width,
            height: declared_height,
            max_pixels: MAX_PIXELS,
        })?;
        if output_bytes > rgba_bytes {
            // More than four bytes per pixel: a 16-bit-per-channel PNG, which
            // the colour-type match below refuses anyway. Refuse it *before*
            // allocating twice what an RGBA8 image of the same size would need.
            return Err(ImageError::Decode(format!(
                "a {declared_width}x{declared_height} PNG needing {output_bytes} bytes is deeper \
                 than 8-bit RGBA; re-bless the golden rather than re-saving it from an image \
                 editor"
            )));
        }

        let mut buffer = vec![0u8; output_bytes];
        let info = reader
            .next_frame(&mut buffer)
            .map_err(|error| ImageError::Decode(error.to_string()))?;
        buffer.truncate(info.buffer_size());

        // Everything this crate writes is 8-bit RGBA, and a golden that is not
        // is a golden someone re-saved from an editor — which is a real thing to
        // do and a real thing to be told about, because a re-encode can change
        // pixels.
        let pixels = match (info.color_type, info.bit_depth) {
            (png::ColorType::Rgba, png::BitDepth::Eight) => buffer,
            (png::ColorType::Rgb, png::BitDepth::Eight) => buffer
                .chunks_exact(3)
                .flat_map(|rgb| [rgb[0], rgb[1], rgb[2], 255])
                .collect(),
            (color_type, depth) => {
                return Err(ImageError::Decode(format!(
                    "{color_type:?}/{depth:?} is not 8-bit RGB or RGBA; re-bless the golden \
                     rather than re-saving it from an image editor"
                )));
            }
        };
        Self::from_rgba8(info.width, info.height, pixels)
    }

    /// Writes a PNG, creating parent directories.
    ///
    /// # Errors
    ///
    /// [`ImageError::Io`], or [`ImageError::Decode`] if the encoder refused.
    pub fn save_png(&self, path: impl AsRef<Path>) -> Result<(), ImageError> {
        let path = path.as_ref();
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent)?;
        }
        let file = std::fs::File::create(path)?;
        let mut encoder = png::Encoder::new(std::io::BufWriter::new(file), self.width, self.height);
        encoder.set_color(png::ColorType::Rgba);
        encoder.set_depth(png::BitDepth::Eight);
        let mut writer = encoder
            .write_header()
            .map_err(|error| ImageError::Decode(error.to_string()))?;
        writer
            .write_image_data(&self.pixels)
            .map_err(|error| ImageError::Decode(error.to_string()))?;
        writer
            .finish()
            .map_err(|error| ImageError::Decode(error.to_string()))?;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The anti-vacuity measurement, on the three cases that matter: a cleared
    /// frame, a frame with something in it, and the early exit.
    #[test]
    fn a_blank_frame_counts_one_colour_and_a_drawn_one_counts_more() {
        let blank = Image::filled(16, 16, [29, 34, 49, 255]).expect("a valid test image");
        assert_eq!(
            blank.distinct_colors(64),
            1,
            "a cleared frame is one colour"
        );

        let mut drawn = blank.clone();
        for i in 0..10u8 {
            drawn.set_pixel(u32::from(i), 4, [i * 20, 60, 30, 255]);
        }
        assert_eq!(drawn.distinct_colors(64), 11, "ten colours plus the clear");

        // The ceiling stops the count, so a caller asking "at least N" pays for
        // N and not for the frame.
        assert_eq!(drawn.distinct_colors(4), 4);
        // And a ceiling of zero is treated as one rather than looping forever
        // or returning something a comparison would read as "blank is fine".
        assert_eq!(drawn.distinct_colors(1), 1);
    }

    #[test]
    fn a_buffer_of_the_wrong_length_is_refused_rather_than_padded() {
        let error = Image::from_rgba8(2, 2, vec![0; 15]).expect_err("15 is not 2*2*4");
        assert!(matches!(
            error,
            ImageError::Size {
                expected: 16,
                actual: 15
            }
        ));
        Image::from_rgba8(2, 2, vec![0; 16]).expect("16 is");
    }

    /// The conversion that stops a golden image being a channel-swap bug: a
    /// BGRA readback and an RGBA one of the same picture must produce the same
    /// `Image`.
    #[test]
    fn bgra_readbacks_are_swizzled_and_rgba_ones_are_not() {
        let rgba = [10u8, 20, 30, 255];
        let bgra = [30u8, 20, 10, 255];
        let from_rgba = Image::from_readback(1, 1, &rgba, ChannelOrder::Rgba).expect("rgba");
        let from_bgra = Image::from_readback(1, 1, &bgra, ChannelOrder::Bgra).expect("bgra");
        assert_eq!(from_rgba, from_bgra);
        assert_eq!(from_rgba.pixel(0, 0), Some([10, 20, 30, 255]));
        // Alpha must not move.
        assert_eq!(from_bgra.pixel(0, 0).expect("a pixel")[3], 255);
    }

    #[test]
    fn pixels_are_addressed_row_major_from_the_top() {
        let mut image = Image::filled(3, 2, [0, 0, 0, 255]).expect("a valid test image");
        image.set_pixel(2, 1, [1, 2, 3, 4]);
        assert_eq!(image.pixel(2, 1), Some([1, 2, 3, 4]));
        assert_eq!(image.pixel(0, 0), Some([0, 0, 0, 255]));
        assert_eq!(image.pixel(3, 0), None, "out of range is None, not a wrap");
        assert_eq!(image.pixel(0, 2), None);
        // The last pixel really is the last four bytes.
        assert_eq!(&image.pixels()[20..24], &[1, 2, 3, 4]);
    }

    #[test]
    fn a_png_round_trips_byte_for_byte() {
        let mut image = Image::filled(7, 5, [3, 250, 128, 255]).expect("a valid test image");
        image.set_pixel(0, 0, [255, 0, 0, 0]);
        image.set_pixel(6, 4, [0, 0, 255, 17]);

        let dir = std::env::temp_dir().join(format!("crcbl-golden-{}", std::process::id()));
        let path = dir.join("round-trip.png");
        image.save_png(&path).expect("writes");
        let loaded = Image::load_png(&path).expect("reads");
        assert_eq!(loaded, image, "a PNG round trip must not touch a pixel");
        // Including alpha, which a careless encoder drops.
        assert_eq!(loaded.pixel(6, 4).expect("a pixel")[3], 17);
        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn a_missing_golden_is_an_io_error_a_caller_can_distinguish() {
        let error = Image::load_png("/nonexistent/crcbl/golden.png").expect_err("no such file");
        assert!(matches!(error, ImageError::Io(_)), "{error}");
    }

    /// `width as usize * height as usize * 4` wraps; the checked version does
    /// not, and refuses rather than handing back a length no allocator can
    /// serve.
    #[test]
    fn absurd_dimensions_are_refused_rather_than_multiplied() {
        for (width, height) in [(u32::MAX, u32::MAX), (4_000_000_000, 4_000_000_000)] {
            let error = Image::from_rgba8(width, height, Vec::new())
                .expect_err("{width}x{height} is not an image");
            assert!(
                matches!(error, ImageError::TooLarge { .. }),
                "{width}x{height}: {error}"
            );
            let error =
                Image::filled(width, height, [0; 4]).expect_err("nor is it one to allocate");
            assert!(matches!(error, ImageError::TooLarge { .. }), "{error}");
        }
        // The bound is exactly `MAX_PIXELS`, and one past it is refused.
        assert!(Image::from_rgba8(1 << 14, (1 << 14) + 1, Vec::new()).is_err());
    }

    /// A PNG's dimensions are *declared*, in twenty-four bytes an attacker
    /// writes, and `png`'s own byte budget does not cover the buffer this
    /// crate allocates from them. A hundred-byte file must not be able to ask
    /// for ten gigabytes.
    #[test]
    fn a_tiny_png_declaring_a_huge_size_errors_instead_of_allocating() {
        let dir = std::env::temp_dir().join(format!(
            "crcbl-golden-huge-{}-{:?}",
            std::process::id(),
            std::thread::current().id()
        ));
        std::fs::create_dir_all(&dir).expect("a scratch directory");
        let path = dir.join("liar.png");
        let bytes = png_declaring(50_000, 50_000);
        assert!(
            bytes.len() < 200,
            "the whole point is that the file is tiny: {} bytes",
            bytes.len()
        );
        std::fs::write(&path, &bytes).expect("writes");

        let error = Image::load_png(&path).expect_err("50000x50000 is 10 GB of RGBA8");
        assert!(
            matches!(
                error,
                ImageError::TooLarge {
                    width: 50_000,
                    height: 50_000,
                    ..
                }
            ),
            "{error}"
        );
        std::fs::remove_dir_all(&dir).ok();
    }

    /// A PNG with a valid header, a truthful-looking IDAT, and nothing behind
    /// it. Only IHDR is read before this crate's size check fires.
    fn png_declaring(width: u32, height: u32) -> Vec<u8> {
        let mut ihdr = Vec::with_capacity(13);
        ihdr.extend_from_slice(&width.to_be_bytes());
        ihdr.extend_from_slice(&height.to_be_bytes());
        ihdr.extend_from_slice(&[8, 6, 0, 0, 0]); // 8-bit RGBA, no interlace.

        let mut file = vec![0x89, b'P', b'N', b'G', 0x0d, 0x0a, 0x1a, 0x0a];
        push_chunk(&mut file, b"IHDR", &ihdr);
        // A zlib header and an empty final stored block: structurally a frame,
        // with no pixels behind it.
        push_chunk(
            &mut file,
            b"IDAT",
            &[0x78, 0x01, 0x01, 0x00, 0x00, 0xff, 0xff],
        );
        push_chunk(&mut file, b"IEND", &[]);
        file
    }

    fn push_chunk(file: &mut Vec<u8>, kind: &[u8; 4], data: &[u8]) {
        let length = u32::try_from(data.len()).expect("a short chunk");
        file.extend_from_slice(&length.to_be_bytes());
        file.extend_from_slice(kind);
        file.extend_from_slice(data);
        let crc = crc32(crc32(0, kind), data);
        file.extend_from_slice(&crc.to_be_bytes());
    }

    /// The PNG CRC-32, computed bitwise so there is no table to get wrong, and
    /// resumable so a chunk's type and its data are one run. `previous` is `0`
    /// to start.
    fn crc32(previous: u32, bytes: &[u8]) -> u32 {
        let mut crc = previous ^ 0xffff_ffff;
        for byte in bytes {
            crc ^= u32::from(*byte);
            for _ in 0..8 {
                crc = if crc & 1 == 1 {
                    (crc >> 1) ^ 0xedb8_8320
                } else {
                    crc >> 1
                };
            }
        }
        crc ^ 0xffff_ffff
    }
}
