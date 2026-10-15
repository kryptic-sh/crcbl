//! PNG, through the `png` crate: decoded to RGBA8 whatever it was stored as,
//! and RGBA8 encoded back.
//!
//! An animated PNG decodes to its default image, the first frame a viewer
//! that does not animate shows.

use crate::{ImageError, Limits, Rgba8Image};

/// The decoder's own working allowance beyond the image itself: the zlib
/// window, a row of filter state and the ancillary chunks a file carries
/// alongside its pixels.
const DECODER_OVERHEAD: usize = 1 << 20;

/// Decodes a PNG to straight RGBA8.
///
/// Palette, grayscale, RGB and every bit depth come out as RGBA8: the decoder
/// is asked to expand a palette and sub-byte samples, add an opaque alpha to
/// anything without one, and keep the high byte of a 16-bit sample, which is
/// what libpng's own normalisation does.
///
/// The header's size is checked against `limits` before a byte is allocated
/// for the pixels, and the decoder's own allocations are capped at what that
/// image needs plus a fixed allowance.
///
/// # Errors
///
/// [`ImageError::Empty`] or [`ImageError::TooLarge`] if the header's size
/// breaks `limits`, and [`ImageError::Malformed`] if the bytes are not a
/// readable PNG.
pub fn decode(bytes: &[u8], limits: &Limits) -> Result<Rgba8Image, ImageError> {
    let malformed = |error: ::png::DecodingError| ImageError::Malformed(error.to_string());
    let decoder_limits = ::png::Limits {
        bytes: limits.max_bytes.saturating_add(DECODER_OVERHEAD),
    };
    let mut decoder = ::png::Decoder::new_with_limits(std::io::Cursor::new(bytes), decoder_limits);
    decoder.set_transformations(
        ::png::Transformations::EXPAND
            | ::png::Transformations::ALPHA
            | ::png::Transformations::STRIP_16,
    );
    let mut reader = decoder.read_info().map_err(malformed)?;
    let (width, height) = reader.info().size();
    let rgba_bytes = limits.rgba_bytes(width, height)?;
    let capacity = reader
        .output_buffer_size()
        .filter(|&capacity| capacity <= rgba_bytes)
        .ok_or_else(|| {
            ImageError::Unsupported(format!(
                "a {width}x{height} PNG deeper than 8-bit RGBA after expansion"
            ))
        })?;
    let mut buffer = vec![0u8; capacity];
    let frame = reader.next_frame(&mut buffer).map_err(malformed)?;
    buffer.truncate(frame.buffer_size());

    let pixels = match (frame.color_type, frame.bit_depth) {
        (::png::ColorType::Rgba, ::png::BitDepth::Eight) => buffer,
        (::png::ColorType::GrayscaleAlpha, ::png::BitDepth::Eight) => buffer
            .as_chunks::<2>()
            .0
            .iter()
            .flat_map(|pair| [pair[0], pair[0], pair[0], pair[1]])
            .collect(),
        (color, depth) => {
            return Err(ImageError::Unsupported(format!(
                "{color:?} at {depth:?} after expansion"
            )));
        }
    };
    if (frame.width, frame.height) != (width, height) || pixels.len() != rgba_bytes {
        return Err(ImageError::Malformed(format!(
            "a {width}x{height} PNG decoded to a {}x{} frame of {} bytes",
            frame.width,
            frame.height,
            pixels.len()
        )));
    }
    Ok(Rgba8Image::from_sized(width, height, pixels))
}

/// Encodes `image` as an 8-bit RGBA PNG.
///
/// # Errors
///
/// [`ImageError::Malformed`] if the encoder refuses it, which an
/// [`Rgba8Image`] gives it no reason to.
pub fn encode(image: &Rgba8Image) -> Result<Vec<u8>, ImageError> {
    let refused = |error: ::png::EncodingError| ImageError::Malformed(error.to_string());
    let mut bytes = Vec::new();
    let mut encoder = ::png::Encoder::new(&mut bytes, image.width(), image.height());
    encoder.set_color(::png::ColorType::Rgba);
    encoder.set_depth(::png::BitDepth::Eight);
    let mut writer = encoder.write_header().map_err(refused)?;
    writer.write_image_data(image.pixels()).map_err(refused)?;
    writer.finish().map_err(refused)?;
    Ok(bytes)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A PNG of `pixels` in `color` at `depth`, written by the `png` crate.
    fn png_of(
        width: u32,
        height: u32,
        color: ::png::ColorType,
        depth: ::png::BitDepth,
        pixels: &[u8],
    ) -> Vec<u8> {
        let mut bytes = Vec::new();
        let mut encoder = ::png::Encoder::new(&mut bytes, width, height);
        encoder.set_color(color);
        encoder.set_depth(depth);
        let mut writer = encoder.write_header().expect("header");
        writer.write_image_data(pixels).expect("pixels");
        writer.finish().expect("finish");
        bytes
    }

    #[test]
    fn rgba_survives_an_encode_and_a_decode() {
        let image = Rgba8Image::new(
            2,
            2,
            vec![1, 2, 3, 4, 5, 6, 7, 8, 9, 10, 11, 12, 13, 14, 15, 16],
        )
        .expect("sized");
        let bytes = encode(&image).expect("encodes");
        assert_eq!(decode(&bytes, &Limits::DEFAULT), Ok(image));
    }

    #[test]
    fn every_storage_comes_out_as_rgba8() {
        use ::png::{BitDepth, ColorType};
        let grey = png_of(2, 1, ColorType::Grayscale, BitDepth::Eight, &[10, 200]);
        assert_eq!(
            decode(&grey, &Limits::DEFAULT).expect("decodes").pixels(),
            [10, 10, 10, 255, 200, 200, 200, 255]
        );
        let grey_alpha = png_of(1, 1, ColorType::GrayscaleAlpha, BitDepth::Eight, &[7, 9]);
        assert_eq!(
            decode(&grey_alpha, &Limits::DEFAULT)
                .expect("decodes")
                .pixels(),
            [7, 7, 7, 9]
        );
        let rgb16 = png_of(
            1,
            1,
            ColorType::Rgb,
            BitDepth::Sixteen,
            &[0x12, 0x34, 0x56, 0x78, 0x9a, 0xbc],
        );
        assert_eq!(
            decode(&rgb16, &Limits::DEFAULT).expect("decodes").pixels(),
            [0x12, 0x56, 0x9a, 255]
        );
        // Four 2-bit greys in one byte: 0, 1, 2, 3 of 3.
        let grey2 = png_of(4, 1, ColorType::Grayscale, BitDepth::Two, &[0b0001_1011]);
        assert_eq!(
            decode(&grey2, &Limits::DEFAULT)
                .expect("decodes")
                .pixels()
                .as_chunks::<4>()
                .0
                .iter()
                .map(|p| p[0])
                .collect::<Vec<_>>(),
            [0, 85, 170, 255]
        );
    }

    #[test]
    fn a_header_claiming_too_many_pixels_is_refused_before_the_pixels() {
        // The header and a token data chunk, far too short for the claim: a
        // decoder that allocated for the claim first would ask for gigabytes
        // before finding the data missing.
        let mut bytes = Vec::new();
        let encoder = ::png::Encoder::new(&mut bytes, 100_000, 100_000);
        let mut writer = encoder.write_header().expect("header");
        writer
            .write_chunk(::png::chunk::IDAT, &[0x78, 0x9c])
            .expect("chunk");
        drop(writer);
        assert!(matches!(
            decode(&bytes, &Limits::DEFAULT),
            Err(ImageError::TooLarge {
                width: 100_000,
                height: 100_000,
                ..
            })
        ));
        let small = Limits {
            max_width: 1,
            max_height: 1,
            max_bytes: 4,
        };
        let two = png_of(
            2,
            1,
            ::png::ColorType::Rgba,
            ::png::BitDepth::Eight,
            &[0; 8],
        );
        assert!(matches!(
            decode(&two, &small),
            Err(ImageError::TooLarge { .. })
        ));
    }

    #[test]
    fn bytes_that_are_not_a_png_are_malformed_not_a_panic() {
        let good = png_of(
            2,
            2,
            ::png::ColorType::Rgba,
            ::png::BitDepth::Eight,
            &[9; 16],
        );
        for bytes in [
            &b""[..],
            b"not a png",
            &good[..good.len() - 20],
            &good[..30],
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
    }
}
