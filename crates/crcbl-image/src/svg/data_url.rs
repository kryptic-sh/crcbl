//! `data:` URLs, the one kind of reference an `<image>` may make: the bytes
//! are in the file, so following one reads nothing else.
//!
//! # Nothing outside the file
//!
//! An icon is untrusted, and an `<image>` that named a path or a web
//! address would let it read any file the caller can, or reach the
//! network, and draw what it found. So [`parse`] refuses every reference
//! that is not a `data:` URL, by name, and nothing here opens or fetches
//! anything.
//!
//! # The grammar
//!
//! As the Fetch standard's data URL processor reads them, which is what
//! browsers run: `data:`, a media type with any parameters, `;base64` if
//! the payload is base64, a comma, and the payload, percent-decoded and then,
//! for base64, decoded by the forgiving base64 decoder (white space anywhere,
//! padding optional). Tabs and line breaks are dropped anywhere, as the URL
//! parser drops them, so a payload an editor wrapped over lines reads whole;
//! and a `#` ends the payload, since the URL parser reads what follows it as
//! a fragment, so an SVG payload must spell a colour's `#` as `%23`, as it
//! must for a browser.

use super::{malformed, unsupported};
use crate::ImageError;

/// The most characters of a refused reference its error repeats: enough to
/// recognise a path or an address, never a whole embedded payload.
const SHOWN_CHARS: usize = 48;

/// What an embedded image is, from its media type.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum MediaType {
    Png,
    Jpeg,
    Webp,
    Svg,
}

/// The decoded payload of a `data:` URL and what it holds.
#[derive(Debug, PartialEq, Eq)]
pub(super) struct DataUrl {
    pub media_type: MediaType,
    pub bytes: Vec<u8>,
}

/// The start of `text`, at most [`SHOWN_CHARS`] of it.
fn shown(text: &str) -> String {
    let mut out: String = text.chars().take(SHOWN_CHARS).collect();
    if out.len() < text.len() {
        out.push('…');
    }
    out
}

/// The image an `<image>`'s `href` embeds.
///
/// # Errors
///
/// [`ImageError::Unsupported`] for a reference that is not a `data:` URL
/// (a path, a web address, a fragment), never followed, and for a media
/// type this renderer does not draw; [`ImageError::Malformed`] for a
/// `data:` URL with no comma or a payload that is not base64.
pub(super) fn parse(href: &str) -> Result<DataUrl, ImageError> {
    let text: String = href
        .trim_matches(|c: char| c <= ' ')
        .chars()
        .filter(|c| !matches!(c, '\t' | '\n' | '\r'))
        .collect();
    let rest = text
        .get(..5)
        .filter(|scheme| scheme.eq_ignore_ascii_case("data:"))
        .and_then(|_| text.get(5..));
    let Some(rest) = rest else {
        return Err(unsupported(format_args!(
            "<image> of {:?}, which is not a data: URL",
            shown(href)
        )));
    };
    let rest = rest.split('#').next().unwrap_or("");
    let Some((header, payload)) = rest.split_once(',') else {
        return Err(malformed("a data: URL with no comma"));
    };
    let mut header = header.trim_matches(|c: char| c.is_ascii_whitespace());
    let mut base64 = false;
    if let Some((before, last)) = header.rsplit_once(';')
        && last.trim_start_matches(' ').eq_ignore_ascii_case("base64")
    {
        base64 = true;
        header = before;
    }
    let essence = header
        .split(';')
        .next()
        .unwrap_or("")
        .trim_matches(|c: char| c.is_ascii_whitespace())
        .to_ascii_lowercase();
    let media_type = match essence.as_str() {
        "image/png" => MediaType::Png,
        // `image/jpg` is not registered, but writers use it and resvg
        // reads it as JPEG.
        "image/jpeg" | "image/jpg" => MediaType::Jpeg,
        "image/webp" => MediaType::Webp,
        "image/svg+xml" => MediaType::Svg,
        "" => return Err(unsupported("<image> of a data: URL with no media type")),
        other => {
            return Err(unsupported(format_args!(
                "<image> of media type {:?}",
                shown(other)
            )));
        }
    };
    let decoded = percent_decode(payload.as_bytes());
    let bytes = if base64 {
        base64_decode(&decoded).ok_or_else(|| malformed("a data: URL's base64 payload"))?
    } else {
        decoded
    };
    Ok(DataUrl { media_type, bytes })
}

/// `bytes` with each `%` and two hex digits replaced by the byte they
/// spell; a `%` not followed by two hex digits stays as it is.
fn percent_decode(bytes: &[u8]) -> Vec<u8> {
    let hex = |byte: u8| char::from(byte).to_digit(16);
    let mut out = Vec::with_capacity(bytes.len());
    let mut index = 0;
    while index < bytes.len() {
        let byte = bytes[index];
        if byte == b'%'
            && let Some(&[high, low]) = bytes.get(index + 1..index + 3)
            && let (Some(high), Some(low)) = (hex(high), hex(low))
        {
            out.push((high * 16 + low) as u8);
            index += 3;
        } else {
            out.push(byte);
            index += 1;
        }
    }
    out
}

/// The forgiving base64 decode of the Infra standard: ASCII white space
/// removed, then one or two `=` of padding when the length is a multiple
/// of four; `None` for a length one more than a multiple of four (a
/// truncated quantum) or any other character outside the alphabet. Bits
/// left over past the last whole byte are dropped, as that standard says.
fn base64_decode(text: &[u8]) -> Option<Vec<u8>> {
    let mut digits: Vec<u8> = text
        .iter()
        .copied()
        .filter(|byte| !byte.is_ascii_whitespace())
        .collect();
    if digits.len().is_multiple_of(4) {
        for _ in 0..2 {
            if digits.last() == Some(&b'=') {
                digits.pop();
            }
        }
    }
    if digits.len() % 4 == 1 {
        return None;
    }
    let value = |byte: u8| match byte {
        b'A'..=b'Z' => Some(byte - b'A'),
        b'a'..=b'z' => Some(byte - b'a' + 26),
        b'0'..=b'9' => Some(byte - b'0' + 52),
        b'+' => Some(62),
        b'/' => Some(63),
        _ => None,
    };
    let mut out = Vec::with_capacity(digits.len() / 4 * 3 + 2);
    let (mut buffer, mut bits) = (0u32, 0u32);
    for byte in digits {
        buffer = buffer << 6 | u32::from(value(byte)?);
        bits += 6;
        if bits >= 8 {
            bits -= 8;
            out.push((buffer >> bits) as u8);
            buffer &= (1 << bits) - 1;
        }
    }
    Some(out)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn parsed(href: &str) -> DataUrl {
        parse(href).unwrap_or_else(|error| panic!("{href}: {error}"))
    }

    /// **Base64 decodes as RFC 4648's test vectors say**, padded or not.
    #[test]
    fn base64_follows_the_rfc_vectors() {
        for (text, want) in [
            ("", ""),
            ("Zg==", "f"),
            ("Zm8=", "fo"),
            ("Zm9v", "foo"),
            ("Zm9vYg==", "foob"),
            ("Zm9vYmE=", "fooba"),
            ("Zm9vYmFy", "foobar"),
            ("Zg", "f"),
            ("Zm8", "fo"),
        ] {
            assert_eq!(
                base64_decode(text.as_bytes()).as_deref(),
                Some(want.as_bytes()),
                "{text}"
            );
        }
        // The last two of the alphabet, and every bit of a byte.
        assert_eq!(base64_decode(b"+/8=").as_deref(), Some(&[0xfb, 0xff][..]));
    }

    /// **White space anywhere in a base64 payload is dropped**, as editors
    /// wrap a long one over lines.
    #[test]
    fn base64_with_white_space_reads_whole() {
        let url = parsed("data:image/png;base64,Zm9v\n  YmFy\r\n\tZm9v YmFy");
        assert_eq!(url.media_type, MediaType::Png);
        assert_eq!(url.bytes, b"foobarfoobar");
        // Padding separated by white space still counts as padding, and
        // bits past the last whole byte are dropped even when not zero.
        assert_eq!(parsed("data:image/png;base64,Zg= =").bytes, b"f");
        assert_eq!(parsed("data:image/png;base64,Zh==").bytes, b"f");
        // Tabs and line breaks go anywhere, as the URL parser drops them: in
        // the media type, and in a payload that is not base64.
        let wrapped = parsed("data:image/svg\n+xml,a\r\n%6\t2");
        assert_eq!(wrapped.media_type, MediaType::Svg);
        assert_eq!(wrapped.bytes, b"ab");
    }

    /// **A truncated or corrupt payload is malformed**: a quantum of one
    /// digit, a character outside the alphabet, padding in the middle or
    /// past two.
    #[test]
    fn a_truncated_payload_is_malformed() {
        for payload in ["Zm9vY", "Zm9v!mFy", "Zg==Zg==", "Zm9==="] {
            let href = format!("data:image/png;base64,{payload}");
            assert!(
                matches!(parse(&href), Err(ImageError::Malformed(_))),
                "{payload}"
            );
        }
        assert!(matches!(
            parse("data:image/png;base64"),
            Err(ImageError::Malformed(_))
        ));
    }

    /// **A payload without `;base64` is percent-decoded**, and a `%` that
    /// spells no byte stays as it is.
    #[test]
    fn a_plain_payload_is_percent_decoded() {
        let url = parsed("data:image/svg+xml,%3Csvg%20a='%2'/%3e%");
        assert_eq!(url.media_type, MediaType::Svg);
        assert_eq!(url.bytes, b"<svg a='%2'/>%");
        // Percent-encoded base64 digits decode before the base64 does.
        assert_eq!(parsed("data:image/png;base64,Zm9%76").bytes, b"foo");
        // A `#` starts the URL's fragment, which is no part of the payload.
        assert_eq!(parsed("data:image/svg+xml,a%23b#c").bytes, b"a#b");
    }

    /// **The media type is read without regard to case or parameters**, and
    /// only the four kinds drawn are accepted; anything else is refused by
    /// name.
    #[test]
    fn the_media_type_picks_the_decoder() {
        for (href, want) in [
            ("data:image/png;base64,", MediaType::Png),
            ("DATA:Image/PNG;BASE64,", MediaType::Png),
            ("data:image/jpeg;base64,", MediaType::Jpeg),
            ("data:image/jpg;base64,", MediaType::Jpeg),
            ("data:image/webp;base64,", MediaType::Webp),
            ("data:image/svg+xml;charset=utf-8,", MediaType::Svg),
            ("data:image/svg+xml;charset=utf-8; base64,", MediaType::Svg),
            (" data:image/png ; base64 ,", MediaType::Png),
        ] {
            assert_eq!(parsed(href).media_type, want, "{href}");
        }
        for href in [
            "data:image/gif;base64,R0lGOD",
            "data:text/plain,hi",
            "data:,hi",
            "data:;base64,Zg==",
            "data:image/pngx;base64,",
        ] {
            assert!(
                matches!(parse(href), Err(ImageError::Unsupported(_))),
                "{href}"
            );
        }
    }

    /// **A reference that is not a `data:` URL is refused, never followed**:
    /// paths, `file:` and web addresses, and fragments alike; its error names
    /// it, cut short.
    #[test]
    fn an_external_reference_is_refused_by_name() {
        for href in [
            "x.png",
            "/etc/passwd",
            "file:///etc/passwd",
            "http://example.com/x.png",
            "https://example.com/x.png",
            "#image",
            "dat:image/png;base64,",
            "",
        ] {
            let Err(ImageError::Unsupported(why)) = parse(href) else {
                panic!("{href} was not refused");
            };
            assert!(why.contains(&shown(href)), "{why}");
        }
        let long = format!("/{}", "a".repeat(200));
        let Err(ImageError::Unsupported(why)) = parse(&long) else {
            panic!("a long path was not refused");
        };
        assert!(why.len() < 120, "{why}");
    }
}
