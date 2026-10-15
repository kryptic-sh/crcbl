//! Image assets turned into the straight-alpha RGBA8 that
//! `crcbl_ui::image::ImageAtlas::register` takes: files decoded, raw buffers
//! from other programs converted, and either resized to the size it is drawn
//! at.
//!
//! # Every input is untrusted
//!
//! The pixels this crate sees come from wherever a desktop sends them: a
//! notification any program may raise, album art from any player, an icon a
//! user-writable `.desktop` file points at. So nothing here panics on its
//! input, and nothing allocates for an image before its size has passed the
//! caller's [`Limits`]: a file a few bytes long can claim to be gigapixels,
//! and the check comes before the buffer, not after.
//!
//! # Straight alpha out
//!
//! [`Rgba8Image`] is always straight (unassociated) alpha, the form the atlas
//! takes. Input that arrives premultiplied — a Windows DIB, usually — says so
//! through [`raw::Alpha`] and is divided back out. The resize works in
//! premultiplied light internally, so a transparent pixel's colour never
//! bleeds into its opaque neighbours, and hands back straight alpha again.
//!
//! # Formats
//!
//! Each file format is a cargo feature: `png`, `jpeg`, `bmp`, `ico` and
//! `webp` (lossless) today
//! (`ico` brings `bmp` and `png`, the two forms an icon's frames take). The raw conversions and
//! the resize need none. The `theme` feature adds the freedesktop.org icon-theme
//! lookup, which finds an icon's file by name; decoding it is the format's.

pub mod raw;
pub mod resize;

#[cfg(feature = "bmp")]
pub mod bmp;

#[cfg(feature = "ico")]
pub mod ico;

#[cfg(feature = "jpeg")]
pub mod jpeg;

#[cfg(feature = "png")]
pub mod png;

#[cfg(feature = "webp")]
pub mod webp;

#[cfg(feature = "theme")]
pub mod theme;

/// The most pixels across a side [`Limits::DEFAULT`] accepts: larger than any
/// icon, notification image or album art a desktop shows, so an image past it
/// is a mistake or an attack.
pub const DEFAULT_MAX_SIDE: u32 = 8192;

/// The most bytes of decoded RGBA8 [`Limits::DEFAULT`] lets one image take: a
/// full square of [`DEFAULT_MAX_SIDE`].
pub const DEFAULT_MAX_BYTES: usize = DEFAULT_MAX_SIDE as usize * DEFAULT_MAX_SIDE as usize * 4;

/// How large an image this crate will build, checked before any allocation
/// for it: against a decompression bomb, a header that lies, and an
/// application handed a buffer whose size fields are hostile.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Limits {
    /// The widest image accepted, in pixels.
    pub max_width: u32,
    /// The tallest image accepted, in pixels.
    pub max_height: u32,
    /// The most bytes one image's RGBA8 may take.
    pub max_bytes: usize,
}

impl Limits {
    /// [`DEFAULT_MAX_SIDE`] each way, at most [`DEFAULT_MAX_BYTES`].
    pub const DEFAULT: Self = Self {
        max_width: DEFAULT_MAX_SIDE,
        max_height: DEFAULT_MAX_SIDE,
        max_bytes: DEFAULT_MAX_BYTES,
    };

    /// The bytes a `width` × `height` RGBA8 image takes, once it has passed
    /// every limit.
    ///
    /// # Errors
    ///
    /// [`ImageError::Empty`] for a zero side, and [`ImageError::TooLarge`] for
    /// a side or a byte count past these limits or past the host's address
    /// space.
    pub fn rgba_bytes(&self, width: u32, height: u32) -> Result<usize, ImageError> {
        if width == 0 || height == 0 {
            return Err(ImageError::Empty { width, height });
        }
        let too_large = ImageError::TooLarge {
            width,
            height,
            limits: *self,
        };
        if width > self.max_width || height > self.max_height {
            return Err(too_large);
        }
        // Two u32s multiplied in u64 cannot overflow; four times that can.
        let bytes = (u64::from(width) * u64::from(height))
            .checked_mul(4)
            .and_then(|bytes| usize::try_from(bytes).ok());
        match bytes {
            Some(bytes) if bytes <= self.max_bytes => Ok(bytes),
            _ => Err(too_large),
        }
    }
}

impl Default for Limits {
    fn default() -> Self {
        Self::DEFAULT
    }
}

/// Why an image could not be built.
#[derive(Clone, Debug, PartialEq, Eq, thiserror::Error)]
#[non_exhaustive]
pub enum ImageError {
    /// A side is zero: there is nothing to draw, and the atlas refuses it.
    #[error("a {width}x{height} image has no pixels")]
    Empty {
        /// The width given.
        width: u32,
        /// The height given.
        height: u32,
    },
    /// The image is larger than the caller's [`Limits`] allow.
    #[error(
        "a {width}x{height} image is past the limits of {}x{} and {} bytes",
        limits.max_width,
        limits.max_height,
        limits.max_bytes
    )]
    TooLarge {
        /// The width claimed.
        width: u32,
        /// The height claimed.
        height: u32,
        /// The limits it broke.
        limits: Limits,
    },
    /// A buffer is shorter than its own size fields say it must be, or a
    /// pixel buffer is not exactly the size its image needs.
    #[error("the buffer is {actual} bytes and needs {needed}")]
    BufferSize {
        /// The bytes the size fields call for.
        needed: usize,
        /// The bytes there are.
        actual: usize,
    },
    /// The input's layout is one this crate does not read, named.
    #[error("unsupported: {0}")]
    Unsupported(String),
    /// The input is not a well-formed file of its format, with the decoder's
    /// reason.
    #[error("malformed: {0}")]
    Malformed(String),
}

/// A straight-alpha RGBA8 image: rows top to bottom, each pixel four bytes
/// `r, g, b, a`, no padding.
///
/// Its size is checked when it is built, so `pixels().len()` is always
/// `width * height * 4` and neither side is zero.
#[derive(Clone, PartialEq, Eq)]
pub struct Rgba8Image {
    width: u32,
    height: u32,
    pixels: Vec<u8>,
}

impl Rgba8Image {
    /// An image of `pixels`, which must be exactly `width * height * 4` bytes.
    ///
    /// # Errors
    ///
    /// [`ImageError::Empty`] for a zero side, [`ImageError::TooLarge`] past
    /// [`Limits::DEFAULT`], and [`ImageError::BufferSize`] if `pixels` is not
    /// the right length.
    pub fn new(width: u32, height: u32, pixels: Vec<u8>) -> Result<Self, ImageError> {
        let needed = Limits::DEFAULT.rgba_bytes(width, height)?;
        if pixels.len() != needed {
            return Err(ImageError::BufferSize {
                needed,
                actual: pixels.len(),
            });
        }
        Ok(Self {
            width,
            height,
            pixels,
        })
    }

    /// Wraps pixels a caller in this crate has already sized.
    fn from_sized(width: u32, height: u32, pixels: Vec<u8>) -> Self {
        debug_assert_eq!(
            pixels.len() as u64,
            u64::from(width) * u64::from(height) * 4
        );
        Self {
            width,
            height,
            pixels,
        }
    }

    /// Its width in pixels.
    #[must_use]
    pub const fn width(&self) -> u32 {
        self.width
    }

    /// Its height in pixels.
    #[must_use]
    pub const fn height(&self) -> u32 {
        self.height
    }

    /// Its pixels, as described on the type.
    #[must_use]
    pub fn pixels(&self) -> &[u8] {
        &self.pixels
    }

    /// Its pixels, given up.
    #[must_use]
    pub fn into_pixels(self) -> Vec<u8> {
        self.pixels
    }
}

impl core::fmt::Debug for Rgba8Image {
    // The pixels would bury the size in a failure message.
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        f.debug_struct("Rgba8Image")
            .field("width", &self.width)
            .field("height", &self.height)
            .finish_non_exhaustive()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_size_past_any_limit_is_refused_before_it_is_built() {
        let limits = Limits {
            max_width: 4,
            max_height: 4,
            max_bytes: 4 * 3 * 4,
        };
        assert_eq!(limits.rgba_bytes(4, 3), Ok(48));
        for (width, height) in [(5, 1), (1, 5), (4, 4)] {
            assert_eq!(
                limits.rgba_bytes(width, height),
                Err(ImageError::TooLarge {
                    width,
                    height,
                    limits
                }),
                "{width}x{height}"
            );
        }
        assert_eq!(
            limits.rgba_bytes(0, 3),
            Err(ImageError::Empty {
                width: 0,
                height: 3
            })
        );
        let unbounded = Limits {
            max_width: u32::MAX,
            max_height: u32::MAX,
            max_bytes: usize::MAX,
        };
        // Four bytes a pixel of the largest square is past `u64`, never
        // mind `usize`.
        assert!(matches!(
            unbounded.rgba_bytes(u32::MAX, u32::MAX),
            Err(ImageError::TooLarge { .. })
        ));
    }

    #[test]
    fn an_image_is_exactly_as_many_bytes_as_its_pixels() {
        assert!(Rgba8Image::new(2, 1, vec![0; 8]).is_ok());
        assert_eq!(
            Rgba8Image::new(2, 1, vec![0; 7]),
            Err(ImageError::BufferSize {
                needed: 8,
                actual: 7
            })
        );
    }
}
