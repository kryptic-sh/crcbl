//! Pixel buffers other programs hand an application, turned into an
//! [`Rgba8Image`].
//!
//! One description, [`RawPixels`], covers every layout the callers meet: the
//! channel order, the row stride, which way the rows run, and whether alpha
//! is straight, premultiplied, or missing altogether. The named helpers are
//! that description filled in for one source each:
//!
//! * [`from_argb32_be`]: a StatusNotifierItem's `IconPixmap`, ARGB32 in
//!   network byte order.
//! * [`from_image_data`]: a notification's `image-data` hint, GdkPixbuf's
//!   layout.
//!
//! A Windows DIB from `GetDIBits` or `IShellItemImageFactory::GetImage` is a
//! [`RawPixels`] with [`ChannelOrder::Bgra`], since whether its alpha is
//! premultiplied, which way its rows run and whether its alpha is all zero
//! depend on where it came from, and the caller knows that.

use crate::{ImageError, Limits, Rgba8Image};

/// The order a pixel's channels arrive in, by byte.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ChannelOrder {
    /// `r, g, b, a`.
    Rgba,
    /// `b, g, r, a`: a Windows DIB, little-endian `0xAARRGGBB`.
    Bgra,
    /// `a, r, g, b`: ARGB32 in network byte order, as StatusNotifierItem
    /// sends it.
    Argb,
    /// `r, g, b`, three bytes and no alpha: every pixel is opaque.
    Rgb,
}

impl ChannelOrder {
    /// The bytes one pixel takes.
    #[must_use]
    pub const fn bytes_per_pixel(self) -> usize {
        match self {
            Self::Rgba | Self::Bgra | Self::Argb => 4,
            Self::Rgb => 3,
        }
    }

    /// One pixel's `r, g, b, a`.
    const fn unpack(self, pixel: &[u8]) -> [u8; 4] {
        match self {
            Self::Rgba => [pixel[0], pixel[1], pixel[2], pixel[3]],
            Self::Bgra => [pixel[2], pixel[1], pixel[0], pixel[3]],
            Self::Argb => [pixel[1], pixel[2], pixel[3], pixel[0]],
            Self::Rgb => [pixel[0], pixel[1], pixel[2], u8::MAX],
        }
    }
}

/// Which way a buffer's rows run.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum RowOrder {
    /// The first row in the buffer is the image's top.
    TopDown,
    /// The first row in the buffer is the image's bottom: a DIB with a
    /// positive height.
    BottomUp,
}

/// What a buffer's alpha channel means.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Alpha {
    /// Colour and alpha independent: what [`Rgba8Image`] holds.
    Straight,
    /// Colour already multiplied by alpha, as a DIB from Windows' shell
    /// usually is; divided back out.
    Premultiplied,
}

/// What to do when every pixel's alpha is zero.
///
/// An icon from before alpha channels has a colour bitmap whose alpha bytes
/// are all zero and a separate monochrome mask saying which pixels show.
/// Taken at its word, such a buffer is entirely transparent, and an icon that
/// draws as nothing is the one outcome nobody wants. Only an alpha channel
/// that is zero everywhere triggers this: one transparent pixel among others
/// is an ordinary image.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum AllZeroAlpha<'a> {
    /// Leave it transparent: the alpha channel is real.
    Transparent,
    /// Treat every pixel as opaque.
    Opaque,
    /// Take visibility from a Windows AND mask: one bit per pixel, most
    /// significant first, a set bit transparent, each row `stride` bytes and
    /// running the same way as the colour rows.
    AndMask {
        /// The mask's bits.
        bits: &'a [u8],
        /// The bytes from one mask row to the next.
        stride: usize,
    },
}

/// A pixel buffer as another program laid it out.
#[derive(Clone, Copy, Debug)]
pub struct RawPixels<'a> {
    /// Its width in pixels.
    pub width: u32,
    /// Its height in pixels.
    pub height: u32,
    /// The bytes from the start of one row to the next: at least
    /// `width * order.bytes_per_pixel()`. The last row needs only its pixels.
    pub stride: usize,
    /// The byte order of each pixel.
    pub order: ChannelOrder,
    /// Which way the rows run.
    pub rows: RowOrder,
    /// What the alpha channel means. Ignored for [`ChannelOrder::Rgb`].
    pub alpha: Alpha,
    /// What to do when every pixel's alpha is zero.
    pub all_zero_alpha: AllZeroAlpha<'a>,
    /// The bytes.
    pub data: &'a [u8],
}

/// The bytes a buffer of `rows` rows, `stride` apart and each `row_bytes`
/// long, needs: every row's stride but the last, which needs only its own
/// bytes. `None` if that overflows.
fn bytes_needed(rows: u32, stride: usize, row_bytes: usize) -> Option<usize> {
    let rows = usize::try_from(rows).ok()?;
    stride
        .checked_mul(rows.checked_sub(1)?)?
        .checked_add(row_bytes)
}

/// Converts `raw` into an [`Rgba8Image`].
///
/// # Errors
///
/// [`ImageError::Empty`] or [`ImageError::TooLarge`] if its size breaks
/// `limits`; [`ImageError::BufferSize`] if its stride is shorter than a row,
/// or its data or its AND mask is shorter than its size and stride need.
pub fn convert(raw: &RawPixels<'_>, limits: &Limits) -> Result<Rgba8Image, ImageError> {
    let out_bytes = limits.rgba_bytes(raw.width, raw.height)?;
    let width = raw.width as usize;
    let pixel_bytes = raw.order.bytes_per_pixel();
    // Both factors are bounded by the limits check above, which kept
    // `width * 4` inside `usize`.
    let row_bytes = width * pixel_bytes;
    if raw.stride < row_bytes {
        return Err(ImageError::BufferSize {
            needed: row_bytes,
            actual: raw.stride,
        });
    }
    let needed = bytes_needed(raw.height, raw.stride, row_bytes).ok_or(ImageError::TooLarge {
        width: raw.width,
        height: raw.height,
        limits: *limits,
    })?;
    if raw.data.len() < needed {
        return Err(ImageError::BufferSize {
            needed,
            actual: raw.data.len(),
        });
    }
    let mask = match raw.all_zero_alpha {
        AllZeroAlpha::AndMask { bits, stride } => {
            let mask_row = width.div_ceil(8);
            let needed = bytes_needed(raw.height, stride, mask_row)
                .filter(|_| stride >= mask_row)
                .ok_or(ImageError::BufferSize {
                    needed: mask_row,
                    actual: stride,
                })?;
            if bits.len() < needed {
                return Err(ImageError::BufferSize {
                    needed,
                    actual: bits.len(),
                });
            }
            Some((bits, stride))
        }
        AllZeroAlpha::Transparent | AllZeroAlpha::Opaque => None,
    };

    let height = raw.height as usize;
    // The source row that is the image's row `y`.
    let source_row = |y: usize| match raw.rows {
        RowOrder::TopDown => y,
        RowOrder::BottomUp => height - 1 - y,
    };
    let mut pixels = Vec::with_capacity(out_bytes);
    for y in 0..height {
        let start = source_row(y) * raw.stride;
        for pixel in raw.data[start..start + row_bytes].chunks_exact(pixel_bytes) {
            let [r, g, b, a] = raw.order.unpack(pixel);
            match raw.alpha {
                Alpha::Premultiplied if raw.order != ChannelOrder::Rgb => {
                    pixels.extend_from_slice(&unpremultiply([r, g, b, a]));
                }
                Alpha::Straight | Alpha::Premultiplied => pixels.extend_from_slice(&[r, g, b, a]),
            }
        }
    }

    let alpha_all_zero =
        raw.order != ChannelOrder::Rgb && pixels.as_chunks::<4>().0.iter().all(|p| p[3] == 0);
    if alpha_all_zero {
        if let Some((bits, stride)) = mask {
            for (y, row) in pixels.chunks_exact_mut(width * 4).enumerate() {
                let mask_row = &bits[source_row(y) * stride..];
                for (x, pixel) in row.as_chunks_mut::<4>().0.iter_mut().enumerate() {
                    let transparent = mask_row[x / 8] & (0x80 >> (x % 8)) != 0;
                    pixel[3] = if transparent { 0 } else { u8::MAX };
                }
            }
        } else if raw.all_zero_alpha == AllZeroAlpha::Opaque {
            for pixel in pixels.as_chunks_mut::<4>().0.iter_mut() {
                pixel[3] = u8::MAX;
            }
        }
    }
    Ok(Rgba8Image::from_sized(raw.width, raw.height, pixels))
}

/// Divides premultiplied colour back out of its alpha, rounding to nearest.
///
/// A colour channel brighter than its alpha cannot come from premultiplying,
/// so it is held at full. A fully transparent pixel has no colour to recover
/// and is transparent black.
#[must_use]
pub const fn unpremultiply([r, g, b, a]: [u8; 4]) -> [u8; 4] {
    if a == 0 {
        return [0; 4];
    }
    [
        unpremultiply_channel(r, a),
        unpremultiply_channel(g, a),
        unpremultiply_channel(b, a),
        a,
    ]
}

/// One colour channel of [`unpremultiply`], for a nonzero `alpha`.
const fn unpremultiply_channel(channel: u8, alpha: u8) -> u8 {
    let alpha = alpha as u16;
    let channel = if (channel as u16) > alpha {
        alpha
    } else {
        channel as u16
    };
    // At most 255, because the channel is at most the alpha.
    ((channel * 255 + alpha / 2) / alpha) as u8
}

/// A StatusNotifierItem `IconPixmap`: `width` × `height` pixels of ARGB32 in
/// network byte order, straight alpha, rows top to bottom with no padding.
///
/// # Errors
///
/// As [`convert`].
pub fn from_argb32_be(
    width: u32,
    height: u32,
    data: &[u8],
    limits: &Limits,
) -> Result<Rgba8Image, ImageError> {
    convert(
        &RawPixels {
            width,
            height,
            stride: (width as usize).saturating_mul(4),
            order: ChannelOrder::Argb,
            rows: RowOrder::TopDown,
            alpha: Alpha::Straight,
            all_zero_alpha: AllZeroAlpha::Transparent,
            data,
        },
        limits,
    )
}

/// A notification's `image-data` (or `icon_data`) hint, field for field: the
/// Desktop Notifications specification's `(iiibiiay)`, which is GdkPixbuf's
/// layout.
#[derive(Clone, Copy, Debug)]
pub struct ImageData<'a> {
    /// Its width in pixels.
    pub width: i32,
    /// Its height in pixels.
    pub height: i32,
    /// The bytes from one row to the next.
    pub rowstride: i32,
    /// Whether each pixel carries alpha.
    pub has_alpha: bool,
    /// Bits per channel; the specification allows only 8.
    pub bits_per_sample: i32,
    /// Channels per pixel: 4 with alpha, 3 without.
    pub channels: i32,
    /// The pixels, straight alpha, rows top to bottom.
    pub data: &'a [u8],
}

/// Converts a notification's `image-data` hint.
///
/// # Errors
///
/// [`ImageError::Unsupported`] for a sample depth other than 8 or a channel
/// count that disagrees with `has_alpha`, and otherwise as [`convert`]. A
/// negative size or stride is [`ImageError::Malformed`].
pub fn from_image_data(image: &ImageData<'_>, limits: &Limits) -> Result<Rgba8Image, ImageError> {
    if image.bits_per_sample != 8 {
        return Err(ImageError::Unsupported(format!(
            "{} bits per sample in a notification image",
            image.bits_per_sample
        )));
    }
    let order = match (image.has_alpha, image.channels) {
        (true, 4) => ChannelOrder::Rgba,
        (false, 3) => ChannelOrder::Rgb,
        (has_alpha, channels) => {
            return Err(ImageError::Unsupported(format!(
                "{channels} channels with has_alpha {has_alpha} in a notification image"
            )));
        }
    };
    let field = |name: &str, value: i32| {
        u32::try_from(value)
            .map_err(|_| ImageError::Malformed(format!("a notification image's {name} is {value}")))
    };
    convert(
        &RawPixels {
            width: field("width", image.width)?,
            height: field("height", image.height)?,
            stride: field("rowstride", image.rowstride)? as usize,
            order,
            rows: RowOrder::TopDown,
            alpha: Alpha::Straight,
            all_zero_alpha: AllZeroAlpha::Transparent,
            data: image.data,
        },
        limits,
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    fn raw(order: ChannelOrder, data: &[u8]) -> RawPixels<'_> {
        RawPixels {
            width: 2,
            height: 2,
            stride: 2 * order.bytes_per_pixel(),
            order,
            rows: RowOrder::TopDown,
            alpha: Alpha::Straight,
            all_zero_alpha: AllZeroAlpha::Transparent,
            data,
        }
    }

    #[test]
    fn each_channel_order_lands_in_rgba() {
        let pixel = |order: ChannelOrder, bytes: [u8; 4]| {
            let data: Vec<u8> = bytes[..order.bytes_per_pixel()].repeat(4);
            convert(&raw(order, &data), &Limits::DEFAULT)
                .expect("converts")
                .pixels()[..4]
                .to_vec()
        };
        assert_eq!(pixel(ChannelOrder::Rgba, [1, 2, 3, 4]), [1, 2, 3, 4]);
        assert_eq!(pixel(ChannelOrder::Bgra, [3, 2, 1, 4]), [1, 2, 3, 4]);
        assert_eq!(pixel(ChannelOrder::Argb, [4, 1, 2, 3]), [1, 2, 3, 4]);
        assert_eq!(pixel(ChannelOrder::Rgb, [1, 2, 3, 0]), [1, 2, 3, 255]);
    }

    #[test]
    fn a_tray_pixmap_is_argb_in_network_order() {
        let data = [
            0x80, 0x10, 0x20, 0x30, 0xff, 0x01, 0x02, 0x03, //
            0x00, 0x00, 0x00, 0x00, 0x40, 0xaa, 0xbb, 0xcc,
        ];
        let image = from_argb32_be(2, 2, &data, &Limits::DEFAULT).expect("converts");
        assert_eq!(
            image.pixels(),
            [
                0x10, 0x20, 0x30, 0x80, 0x01, 0x02, 0x03, 0xff, //
                0x00, 0x00, 0x00, 0x00, 0xaa, 0xbb, 0xcc, 0x40,
            ]
        );
        assert_eq!(
            from_argb32_be(2, 2, &data[..15], &Limits::DEFAULT),
            Err(ImageError::BufferSize {
                needed: 16,
                actual: 15
            })
        );
    }

    #[test]
    fn padded_rows_are_skipped_and_bottom_up_rows_are_flipped() {
        // Two rows of two RGB pixels, each row padded to eight bytes.
        let data = [
            1, 1, 1, 2, 2, 2, 0xee, 0xee, //
            3, 3, 3, 4, 4, 4,
        ];
        let mut pixels = RawPixels {
            stride: 8,
            ..raw(ChannelOrder::Rgb, &data)
        };
        let top_down = convert(&pixels, &Limits::DEFAULT).expect("converts");
        assert_eq!(
            top_down.pixels(),
            [1, 1, 1, 255, 2, 2, 2, 255, 3, 3, 3, 255, 4, 4, 4, 255]
        );
        pixels.rows = RowOrder::BottomUp;
        let bottom_up = convert(&pixels, &Limits::DEFAULT).expect("converts");
        assert_eq!(
            bottom_up.pixels(),
            [3, 3, 3, 255, 4, 4, 4, 255, 1, 1, 1, 255, 2, 2, 2, 255]
        );
        pixels.stride = 5;
        assert_eq!(
            convert(&pixels, &Limits::DEFAULT),
            Err(ImageError::BufferSize {
                needed: 6,
                actual: 5
            })
        );
    }

    #[test]
    fn premultiplied_colour_is_divided_back_out() {
        assert_eq!(unpremultiply([16, 32, 64, 128]), [32, 64, 128, 128]);
        assert_eq!(unpremultiply([10, 20, 30, 255]), [10, 20, 30, 255]);
        assert_eq!(unpremultiply([200, 1, 1, 100]), [255, 3, 3, 100]);
        assert_eq!(unpremultiply([9, 9, 9, 0]), [0, 0, 0, 0]);
        let data = [64, 32, 16, 128].repeat(4);
        let pixels = RawPixels {
            alpha: Alpha::Premultiplied,
            ..raw(ChannelOrder::Bgra, &data)
        };
        assert_eq!(
            &convert(&pixels, &Limits::DEFAULT)
                .expect("converts")
                .pixels()[..4],
            [32, 64, 128, 128]
        );
    }

    #[test]
    fn an_all_zero_alpha_channel_is_read_as_the_caller_says() {
        let data = [9, 8, 7, 0].repeat(4);
        let mut pixels = raw(ChannelOrder::Bgra, &data);
        let alphas = |pixels: &RawPixels<'_>| -> Vec<u8> {
            convert(pixels, &Limits::DEFAULT)
                .expect("converts")
                .pixels()
                .as_chunks::<4>()
                .0
                .iter()
                .map(|p| p[3])
                .collect()
        };
        assert_eq!(alphas(&pixels), [0, 0, 0, 0]);
        pixels.all_zero_alpha = AllZeroAlpha::Opaque;
        assert_eq!(alphas(&pixels), [255, 255, 255, 255]);
        // Top row: the left pixel masked out; bottom row: the right one.
        let mask = [0b1000_0000, 0, 0, 0, 0b0100_0000, 0, 0, 0];
        pixels.all_zero_alpha = AllZeroAlpha::AndMask {
            bits: &mask,
            stride: 4,
        };
        assert_eq!(alphas(&pixels), [0, 255, 255, 0]);
        pixels.rows = RowOrder::BottomUp;
        assert_eq!(alphas(&pixels), [255, 0, 0, 255]);
        pixels.all_zero_alpha = AllZeroAlpha::AndMask {
            bits: &mask[..4],
            stride: 4,
        };
        assert_eq!(
            convert(&pixels, &Limits::DEFAULT),
            Err(ImageError::BufferSize {
                needed: 5,
                actual: 4
            })
        );

        // One visible pixel means the channel is real, whatever the setting.
        let mut data = data;
        data[3] = 1;
        let pixels = RawPixels {
            all_zero_alpha: AllZeroAlpha::Opaque,
            ..raw(ChannelOrder::Bgra, &data)
        };
        assert_eq!(alphas(&pixels), [1, 0, 0, 0]);
    }

    #[test]
    fn a_notification_image_is_checked_field_by_field() {
        let rgb = [10, 20, 30, 0, 40, 50, 60, 0];
        let image = ImageData {
            width: 1,
            height: 2,
            rowstride: 4,
            has_alpha: false,
            bits_per_sample: 8,
            channels: 3,
            data: &rgb,
        };
        assert_eq!(
            from_image_data(&image, &Limits::DEFAULT)
                .expect("converts")
                .pixels(),
            [10, 20, 30, 255, 40, 50, 60, 255]
        );
        for (broken, why) in [
            (
                ImageData {
                    bits_per_sample: 16,
                    ..image
                },
                "depth",
            ),
            (
                ImageData {
                    has_alpha: true,
                    ..image
                },
                "channels",
            ),
        ] {
            assert!(
                matches!(
                    from_image_data(&broken, &Limits::DEFAULT),
                    Err(ImageError::Unsupported(_))
                ),
                "{why}"
            );
        }
        assert!(matches!(
            from_image_data(
                &ImageData {
                    height: -1,
                    ..image
                },
                &Limits::DEFAULT
            ),
            Err(ImageError::Malformed(_))
        ));
        assert!(matches!(
            from_image_data(
                &ImageData {
                    width: 100_000,
                    ..image
                },
                &Limits::DEFAULT
            ),
            Err(ImageError::TooLarge { .. })
        ));
    }

    #[test]
    fn a_stride_too_large_to_multiply_is_refused_not_wrapped() {
        let data = [0u8; 16];
        let pixels = RawPixels {
            height: 3,
            stride: usize::MAX / 2 + 1,
            ..raw(ChannelOrder::Rgba, &data)
        };
        assert!(matches!(
            convert(&pixels, &Limits::DEFAULT),
            Err(ImageError::TooLarge { .. })
        ));
    }
}
