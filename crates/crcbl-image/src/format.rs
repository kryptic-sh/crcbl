//! Which decoder a file is for, named from its first bytes rather than from
//! whatever its name or a `data:` URL's media type claims.
//!
//! The patterns are the WHATWG MIME Sniffing Standard's image type pattern
//! matching algorithm, for the formats this crate decodes and that have one.
//! It defines none for XPM or SVG, so they are not sniffed. Each is text
//! whose marker need not be its first bytes: an XPM header (`/* XPM */` for
//! XPM 3, `! XPM2` for XPM 2) may have whitespace before and within it, as
//! the XPM decoder accepts, and an SVG may have an XML declaration, comments
//! or a doctype before its `<svg>`. A match says only which decoder to ask:
//! the file may still be damaged, and only that decoder can say so.

/// A file format [`sniff`] recognises.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
#[non_exhaustive]
pub enum Format {
    /// PNG, for `png::decode`.
    Png,
    /// JPEG, for `jpeg::decode`.
    Jpeg,
    /// WebP, for `webp::decode`.
    Webp,
    /// A Windows bitmap, for `bmp::decode`.
    Bmp,
    /// A Windows icon or cursor, for `ico::decode`.
    Ico,
    /// A GIF, for `gif::decode`.
    Gif,
}

/// The two GIF signatures.
const GIF87A: &[u8] = b"GIF87a";
const GIF89A: &[u8] = b"GIF89a";

/// The PNG signature.
const PNG: &[u8] = b"\x89PNG\r\n\x1a\n";

/// A JPEG's start-of-image marker and the first byte of the marker after it.
const JPEG: &[u8] = b"\xff\xd8\xff";

/// A BMP's file header signature.
const BMP: &[u8] = b"BM";

/// An icon directory's reserved word and its type: 1 for an icon, 2 for a
/// cursor.
const ICO: &[u8] = b"\0\0\x01\0";
const CUR: &[u8] = b"\0\0\x02\0";

/// The format whose signature `bytes` starts with, or `None` for one this
/// crate does not decode or cannot recognise by its first bytes.
///
/// Reads at most the first few bytes; feature-independent, so it names a
/// format whose decoder this build may not have.
#[must_use]
pub fn sniff(bytes: &[u8]) -> Option<Format> {
    if bytes.starts_with(PNG) {
        Some(Format::Png)
    } else if bytes.starts_with(JPEG) {
        Some(Format::Jpeg)
    } else if bytes.starts_with(b"RIFF") && bytes.get(8..14) == Some(b"WEBPVP") {
        // The RIFF size between is anything; `VP` begins the first chunk,
        // `VP8 `, `VP8L` or `VP8X`.
        Some(Format::Webp)
    } else if bytes.starts_with(BMP) {
        Some(Format::Bmp)
    } else if bytes.starts_with(ICO) || bytes.starts_with(CUR) {
        Some(Format::Ico)
    } else if bytes.starts_with(GIF87A) || bytes.starts_with(GIF89A) {
        Some(Format::Gif)
    } else {
        None
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn each_signature_names_its_format() {
        for (bytes, format) in [
            (&b"\x89PNG\r\n\x1a\n\0\0\0\x0dIHDR"[..], Format::Png),
            (b"\xff\xd8\xff\xe0\0\x10JFIF\0", Format::Jpeg),
            (b"RIFF\x24\0\0\0WEBPVP8 ", Format::Webp),
            (b"RIFF\0\0\0\0WEBPVP8L", Format::Webp),
            (b"RIFF\xff\xff\xff\xffWEBPVP8X", Format::Webp),
            (b"BM\x36\0\0\0", Format::Bmp),
            (b"\0\0\x01\0\x01\0", Format::Ico),
            (b"\0\0\x02\0\x01\0", Format::Ico),
            (b"GIF87a\x01\0\x01\0", Format::Gif),
            (b"GIF89a\x01\0\x01\0", Format::Gif),
        ] {
            assert_eq!(sniff(bytes), Some(format), "{bytes:?}");
        }
    }

    /// A committed file of each format, as the decoders' tests read them.
    #[test]
    fn each_committed_fixture_is_its_format() {
        for (bytes, format) in [
            (
                &include_bytes!("../fixtures/jpeg/baseline-444.jpg")[..],
                Format::Jpeg,
            ),
            (include_bytes!("../fixtures/webp/noise.webp"), Format::Webp),
            (include_bytes!("../fixtures/webp/lossy.webp"), Format::Webp),
            (
                include_bytes!("../fixtures/webp/animated.webp"),
                Format::Webp,
            ),
            (include_bytes!("../fixtures/bmp/rgb.bmp"), Format::Bmp),
            (
                include_bytes!("../fixtures/bmp/frames-png.ico"),
                Format::Ico,
            ),
            (include_bytes!("../fixtures/gif/global.gif"), Format::Gif),
            (include_bytes!("../fixtures/gif/animated.gif"), Format::Gif),
        ] {
            assert_eq!(sniff(bytes), Some(format), "{format:?}");
        }
    }

    #[test]
    fn garbage_short_and_undecoded_formats_are_none() {
        for bytes in [
            &b""[..],
            b"\x89PNG\r\n\x1a",
            b"\xff\xd8",
            b"RIFF\0\0\0\0WEBP",
            b"RIFF\0\0\0\0WAVEfmt ",
            b"B",
            b"\0\0\x01",
            b"\0\0\x03\0",
            b"GIF89",
            b"GIF88a",
            b"gif89a",
            b"/* XPM */",
            b"<svg xmlns=\"http://www.w3.org/2000/svg\"/>",
            b"not an image at all",
        ] {
            assert_eq!(sniff(bytes), None, "{bytes:?}");
        }
    }
}
