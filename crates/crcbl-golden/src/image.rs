//! An 8-bit RGBA image, and PNG on both sides of it.
//!
//! The one storage format is RGBA8. A readback arrives in whatever the
//! swapchain's format was — BGRA on most surfaces — so the conversion happens
//! once, at the boundary, rather than being a flag every comparison has to
//! respect. That also means a golden PNG is a *display-referred* file anyone can
//! open, which is what makes a CI artifact worth uploading.

use std::path::Path;

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
        }
    }
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
    /// otherwise show up as a comparison against garbage.
    pub fn from_rgba8(width: u32, height: u32, pixels: Vec<u8>) -> Result<Self, ImageError> {
        let expected = width as usize * height as usize * 4;
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
    #[must_use]
    pub fn filled(width: u32, height: u32, color: [u8; 4]) -> Self {
        let mut pixels = Vec::with_capacity(width as usize * height as usize * 4);
        for _ in 0..(width as usize * height as usize) {
            pixels.extend_from_slice(&color);
        }
        Self {
            width,
            height,
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

    /// Loads a PNG.
    ///
    /// # Errors
    ///
    /// [`ImageError::Io`] if the file is missing, or [`ImageError::Decode`] if
    /// it is not a PNG this crate can read.
    pub fn load_png(path: impl AsRef<Path>) -> Result<Self, ImageError> {
        let file = std::fs::File::open(path.as_ref())?;
        let decoder = png::Decoder::new(std::io::BufReader::new(file));
        let mut reader = decoder
            .read_info()
            .map_err(|error| ImageError::Decode(error.to_string()))?;
        let mut buffer = vec![0u8; reader.output_buffer_size().unwrap_or(0)];
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
        let mut image = Image::filled(3, 2, [0, 0, 0, 255]);
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
        let mut image = Image::filled(7, 5, [3, 250, 128, 255]);
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
}
