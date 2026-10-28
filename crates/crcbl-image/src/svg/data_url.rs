//! `data:` URLs, the one kind of reference an `<image>` may make: the bytes
//! are in the file, so following one reads nothing else.
//!
//! [`crate::data_url::parse`] reads the URL and refuses every reference that
//! is not a `data:` URL, by name; this module picks which of the formats this
//! renderer draws its media type names.

use super::unsupported;
use crate::ImageError;
use crate::data_url::shown;

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

/// The image an `<image>`'s `href` embeds.
///
/// # Errors
///
/// [`ImageError::Unsupported`] for a reference that is not a `data:` URL
/// (a path, a web address, a fragment), never followed, and for a media
/// type this renderer does not draw; [`ImageError::Malformed`] for a
/// `data:` URL with no comma or a payload that is not base64.
pub(super) fn parse(href: &str) -> Result<DataUrl, ImageError> {
    let url = crate::data_url::parse(href).map_err(|error| match error {
        ImageError::Unsupported(why) => unsupported(format_args!("<image> of {why}")),
        other => other,
    })?;
    let media_type = match url.media_type.as_str() {
        "image/png" => MediaType::Png,
        // `image/jpg` is not registered, but writers use it and resvg
        // reads it as JPEG.
        "image/jpeg" | "image/jpg" => MediaType::Jpeg,
        "image/webp" => MediaType::Webp,
        "image/svg+xml" => MediaType::Svg,
        other => {
            return Err(unsupported(format_args!(
                "<image> of media type {:?}",
                shown(other)
            )));
        }
    };
    Ok(DataUrl {
        media_type,
        bytes: url.bytes,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn parsed(href: &str) -> DataUrl {
        parse(href).unwrap_or_else(|error| panic!("{href}: {error}"))
    }

    /// **Only the four kinds drawn are accepted**, whatever the case or
    /// parameters; anything else is refused by name.
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
        assert_eq!(parsed("data:image/svg+xml,a%23b").bytes, b"a#b");
        for href in [
            "data:image/gif;base64,R0lGOD",
            "data:text/plain,hi",
            "data:,hi",
            "data:;base64,Zg==",
            "data:image/pngx;base64,",
        ] {
            let Err(ImageError::Unsupported(why)) = parse(href) else {
                panic!("{href} was not refused");
            };
            assert!(why.contains("<image> of media type"), "{why}");
        }
    }

    /// **A reference that is not a `data:` URL is refused as an `<image>`'s**,
    /// never followed, its error naming it.
    #[test]
    fn an_external_reference_is_refused_by_name() {
        for href in ["x.png", "file:///etc/passwd", "https://example.com/x.png"] {
            let Err(ImageError::Unsupported(why)) = parse(href) else {
                panic!("{href} was not refused");
            };
            assert!(why.starts_with("SVG <image> of "), "{why}");
            assert!(why.contains(&shown(href)), "{why}");
        }
        assert!(matches!(
            parse("data:image/png;base64,Zm9vY"),
            Err(ImageError::Malformed(_))
        ));
    }
}
