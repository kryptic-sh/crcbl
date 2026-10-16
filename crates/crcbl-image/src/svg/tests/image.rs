//! `<image>` through [`decode`]: placement by every `preserveAspectRatio`,
//! the resampling kernel, embedded SVG, each media type, refusals and the
//! caps, each checked against pixels worked out by hand or by the format's
//! own decoder.

use super::*;
use crate::svg::render::image::MAX_NESTING;

/// RFC 4648's base64 of `bytes`, padded.
fn base64(bytes: &[u8]) -> String {
    const ALPHABET: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
    let mut out = String::new();
    for chunk in bytes.chunks(3) {
        let mut quantum = [0u8; 3];
        quantum[..chunk.len()].copy_from_slice(chunk);
        let bits = u32::from(quantum[0]) << 16 | u32::from(quantum[1]) << 8 | u32::from(quantum[2]);
        for digit in 0..4 {
            if digit <= chunk.len() {
                out.push(char::from(
                    ALPHABET[(bits >> (18 - 6 * digit) & 63) as usize],
                ));
            } else {
                out.push('=');
            }
        }
    }
    out
}

/// A `data:` URL of `bytes` as `media_type`, base64.
fn data_url(media_type: &str, bytes: &[u8]) -> String {
    format!("data:{media_type};base64,{}", base64(bytes))
}

/// A `width` × `height` image of one straight-alpha `color`.
fn solid(width: u32, height: u32, color: [u8; 4]) -> Rgba8Image {
    Rgba8Image::new(width, height, color.repeat((width * height) as usize))
        .unwrap_or_else(|error| panic!("{error}"))
}

/// A PNG `data:` URL of `image`.
#[cfg(feature = "png")]
fn png_url(image: &Rgba8Image) -> String {
    data_url(
        "image/png",
        &crate::png::encode(image).unwrap_or_else(|error| panic!("{error}")),
    )
}

/// The test of the base64 the tests write, against RFC 4648's vectors.
#[test]
fn the_test_encoder_follows_the_rfc() {
    for (text, want) in [
        ("", ""),
        ("f", "Zg=="),
        ("fo", "Zm8="),
        ("foo", "Zm9v"),
        ("foobar", "Zm9vYmFy"),
    ] {
        assert_eq!(base64(text.as_bytes()), want);
    }
}

/// **Every alignment places the image where SVG says**: drawn at its own
/// size into a viewport one pixel larger than it each way it does not fill
/// (three pixels for a two-pixel side), `Min` puts it at the viewport's
/// start, `Mid` one pixel in and `Max` two, along the axis it does not
/// fill, and the other alignment changes nothing.
#[cfg(feature = "png")]
#[test]
fn every_alignment_places_the_image() {
    let wide = png_url(&solid(2, 1, RED));
    let tall = png_url(&solid(1, 2, BLUE));
    let offset = |part: &str| match part {
        "Min" => 0,
        "Mid" => 1,
        _ => 2,
    };
    for x in ["Min", "Mid", "Max"] {
        for y in ["Min", "Mid", "Max"] {
            for meet in ["", " meet"] {
                let aspect = format!("x{x}Y{y}{meet}");
                let image = render(
                    &svg(
                        8,
                        &format!(
                            r#"<image x="2" y="2" width="2" height="3" preserveAspectRatio="{aspect}" href="{wide}"/>"#
                        ),
                    ),
                    8,
                );
                let top = 2 + offset(y);
                assert_exact(&image, RED, |px, py| (2..4).contains(&px) && py == top);
                let image = render(
                    &svg(
                        8,
                        &format!(
                            r#"<image x="2" y="2" width="3" height="2" preserveAspectRatio="{aspect}" href="{tall}"/>"#
                        ),
                    ),
                    8,
                );
                let left = 2 + offset(x);
                assert_exact(&image, BLUE, |px, py| px == left && (2..4).contains(&py));
            }
        }
    }
}

/// **`slice` fills the viewport and cuts the rest**, aligned as asked: a
/// four by two image, red then blue in halves, sliced into a two-pixel
/// square at its own size shows its left half for `xMin`, its right for
/// `xMax`, and the middle for `xMid`. **`none` stretches** it to the
/// viewport, and the default is `xMidYMid meet`.
#[cfg(feature = "png")]
#[test]
fn slice_and_none_fill_the_viewport() {
    let mut pixels = Vec::new();
    for _row in 0..2 {
        pixels.extend_from_slice(&[RED, RED, BLUE, BLUE].concat());
    }
    let halves = png_url(&Rgba8Image::new(4, 2, pixels).unwrap_or_else(|e| panic!("{e}")));
    for (align, left_colour, right_colour) in [
        ("xMinYMid", RED, RED),
        ("xMidYMid", RED, BLUE),
        ("xMaxYMid", BLUE, BLUE),
    ] {
        let image = render(
            &svg(
                8,
                &format!(
                    r#"<image x="2" y="2" width="2" height="2" preserveAspectRatio="{align} slice" href="{halves}"/>"#
                ),
            ),
            8,
        );
        for y in 0..8 {
            for x in 0..8 {
                let want = match (x, y) {
                    (2, 2..4) => left_colour,
                    (3, 2..4) => right_colour,
                    _ => [0; 4],
                };
                assert_eq!(pixel(&image, x, y), want, "{align} ({x}, {y})");
            }
        }
    }
    let red = png_url(&solid(2, 1, RED));
    let stretched = render(
        &svg(
            8,
            &format!(
                r#"<image x="2" y="2" width="4" height="4" preserveAspectRatio="none" href="{red}"/>"#
            ),
        ),
        8,
    );
    assert_exact(&stretched, RED, |x, y| {
        (2..6).contains(&x) && (2..6).contains(&y)
    });
    // Two by one into four by four, meet and centred: rows 3 and 4.
    let default = render(
        &svg(
            8,
            &format!(r#"<image x="2" y="2" width="4" height="4" href="{red}"/>"#),
        ),
        8,
    );
    assert_exact(&default, RED, |x, y| {
        (2..6).contains(&x) && (3..5).contains(&y)
    });
}

/// **A missing side takes the image's own size, or the other side's scaled
/// by its aspect**; `auto` is the same as missing, and a zero or negative
/// side draws nothing.
#[cfg(feature = "png")]
#[test]
fn missing_sides_size_themselves() {
    let red = png_url(&solid(2, 1, RED));
    let own = render(&svg(8, &format!(r#"<image x="1" y="1" href="{red}"/>"#)), 8);
    assert_exact(&own, RED, |x, y| (1..3).contains(&x) && y == 1);
    for sides in [r#"width="6""#, r#"width="6" height="auto""#] {
        let wide = render(&svg(8, &format!(r#"<image {sides} href="{red}"/>"#)), 8);
        assert_exact(&wide, RED, |x, y| x < 6 && y < 3);
    }
    let high = render(&svg(8, &format!(r#"<image height="4" href="{red}"/>"#)), 8);
    assert_exact(&high, RED, |_, y| y < 4);
    for sides in [r#"width="0""#, r#"height="0""#, r#"width="-2" height="2""#] {
        let none = render(&svg(8, &format!(r#"<image {sides} href="{red}"/>"#)), 8);
        assert_exact(&none, RED, |_, _| false);
    }
    // A negative side would mirror an SVG image back into view rather than
    // empty it, so it is not drawn at all.
    let inner =
        format!(r#"<svg {SVG} width="2" height="2"><rect width="2" height="2" fill="red"/></svg>"#);
    let svg_url = data_url("image/svg+xml", inner.as_bytes());
    let mirrored = render(
        &svg(
            8,
            &format!(r#"<image x="4" width="-2" height="2" href="{svg_url}"/>"#),
        ),
        8,
    );
    assert_exact(&mirrored, RED, |_, _| false);
    // No href at all draws nothing.
    let empty = render(&svg(8, r#"<image width="2" height="2"/>"#), 8);
    assert_exact(&empty, RED, |_, _| false);
}

/// Mitchell and Netravali's cubic with `B = C = 1/3`, from their paper's
/// piecewise definition.
fn mitchell(x: f64) -> f64 {
    let x = x.abs();
    if x < 1.0 {
        (7.0 * x.powi(3) - 12.0 * x * x + 16.0 / 3.0) / 6.0
    } else if x < 2.0 {
        (-7.0 / 3.0 * x.powi(3) + 12.0 * x * x - 20.0 * x + 32.0 / 3.0) / 6.0
    } else {
        0.0
    }
}

/// **An enlarged image is sampled by the Mitchell cubic at each pixel's
/// centre**, edge pixels repeated outwards: a black and a white pixel
/// stretched four times across give, at each column, the kernel's weights
/// summed over the white taps.
#[cfg(feature = "png")]
#[test]
fn an_enlarged_image_is_sampled_by_the_cubic() {
    let pixels = [[0, 0, 0, 255], [255, 255, 255, 255]].concat();
    let pair = png_url(&Rgba8Image::new(2, 1, pixels).unwrap_or_else(|e| panic!("{e}")));
    let image = render(
        &svg(
            8,
            &format!(r#"<image width="8" height="4" preserveAspectRatio="none" href="{pair}"/>"#),
        ),
        8,
    );
    for x in 0..8 {
        let u = (f64::from(x) + 0.5) / 4.0;
        let first = (u - 2.0).floor() as i64;
        let white: f64 = (first..=first + 4)
            .filter(|&tap| tap.clamp(0, 1) == 1)
            .map(|tap| mitchell(u - (tap as f64 + 0.5)))
            .sum();
        let want = (white.clamp(0.0, 1.0) * 255.0).round() as u8;
        for y in 0..4 {
            let got = pixel(&image, x, y);
            assert!(
                got[0].abs_diff(want) <= 1 && got[0] == got[1] && got[1] == got[2],
                "({x}, {y}): {got:?}, want {want}"
            );
            assert_eq!(got[3], 255);
        }
    }
}

/// **An image at its own size is copied pixel for pixel**, as nearest
/// neighbour samples it, through transforms that only translate.
#[cfg(feature = "png")]
#[test]
fn an_unscaled_image_is_copied() {
    let pixels: Vec<u8> = (0u8..16)
        .flat_map(|i| [i * 16, 255 - i * 16, i * 8, 255])
        .collect();
    let source = Rgba8Image::new(4, 4, pixels).unwrap_or_else(|e| panic!("{e}"));
    let url = png_url(&source);
    let image = render(
        &svg(
            8,
            &format!(r#"<g transform="translate(1 0)"><image x="2" y="3" href="{url}"/></g>"#),
        ),
        8,
    );
    for y in 0..8 {
        for x in 0..8 {
            let want = if (3..7).contains(&x) && (3..7).contains(&y) {
                pixel(&source, x - 3, y - 3)
            } else {
                [0; 4]
            };
            assert_eq!(pixel(&image, x, y), want, "({x}, {y})");
        }
    }
}

/// **An image takes transforms, opacity, clip paths and masks as any
/// element does.**
#[cfg(feature = "png")]
#[test]
fn an_image_takes_group_effects() {
    let red = png_url(&solid(1, 1, RED));
    let faded = render(
        &svg(
            8,
            &format!(
                r#"<image transform="translate(2 2)" width="4" height="4" opacity="0.5" href="{red}"/>"#
            ),
        ),
        8,
    );
    assert_exact(&faded, [255, 0, 0, 128], |x, y| {
        (2..6).contains(&x) && (2..6).contains(&y)
    });
    let clipped = render(
        &svg(
            8,
            &format!(
                r#"<clipPath id="c"><rect width="4" height="8"/></clipPath><mask id="m"><rect y="4" width="8" height="4" fill="white"/></mask><image width="8" height="8" clip-path="url(#c)" mask="url(#m)" href="{red}"/>"#
            ),
        ),
        8,
    );
    assert_exact(&clipped, RED, |x, y| x < 4 && y >= 4);
    // Hidden, it draws nothing.
    let hidden = render(
        &svg(
            8,
            &format!(r#"<image width="8" height="8" visibility="hidden" href="{red}"/>"#),
        ),
        8,
    );
    assert_exact(&hidden, RED, |_, _| false);
    // A mask in bounding-box units measures the image's viewport.
    let boxed = render(
        &svg(
            8,
            &format!(
                r#"<mask id="m" maskContentUnits="objectBoundingBox"><rect width="0.5" height="1" fill="white"/></mask><image x="2" y="2" width="4" height="4" mask="url(#m)" href="{red}"/>"#
            ),
        ),
        8,
    );
    assert_exact(&boxed, RED, |x, y| {
        (2..4).contains(&x) && (2..6).contains(&y)
    });
}

/// **An embedded SVG is drawn by this renderer as vectors**, placed like
/// any image: a two-unit SVG whose left half is red, in a four-pixel
/// viewport, is red over exactly four by two pixels, percent-encoded or
/// base64. Nothing of the outer document's style reaches it: a fill on the
/// `<image>` leaves the inner shape's default black.
#[test]
fn an_embedded_svg_is_drawn_as_vectors() {
    let inner =
        format!(r#"<svg {SVG} width="2" height="2"><rect width="1" height="2" fill="red"/></svg>"#);
    let percent = format!(
        "data:image/svg+xml,{}",
        inner
            .replace('%', "%25")
            .replace('<', "%3C")
            .replace('>', "%3E")
            .replace('"', "%22")
            .replace('#', "%23")
    );
    for href in [percent, data_url("image/svg+xml", inner.as_bytes())] {
        let image = render(
            &svg(
                8,
                &format!(r#"<image x="2" y="2" width="4" height="4" href="{href}"/>"#),
            ),
            8,
        );
        assert_exact(&image, RED, |x, y| {
            (2..4).contains(&x) && (2..6).contains(&y)
        });
    }
    let unstyled = format!(r#"<svg {SVG} width="2" height="2"><rect width="2" height="2"/></svg>"#);
    let image = render(
        &svg(
            8,
            &format!(
                r#"<style>rect {{ fill: blue }}</style><image width="8" height="8" fill="red" href="{}"/>"#,
                data_url("image/svg+xml", unstyled.as_bytes())
            ),
        ),
        8,
    );
    assert_exact(&image, [0, 0, 0, 255], |_, _| true);
    // Sliced, its overflow is cut at the image's viewport.
    let wide =
        format!(r#"<svg {SVG} width="4" height="2"><rect width="4" height="2" fill="red"/></svg>"#);
    let sliced = render(
        &svg(
            8,
            &format!(
                r#"<image x="2" y="2" width="4" height="4" preserveAspectRatio="xMidYMid slice" href="{}"/>"#,
                data_url("image/svg+xml", wide.as_bytes())
            ),
        ),
        8,
    );
    assert_exact(&sliced, RED, |x, y| {
        (2..6).contains(&x) && (2..6).contains(&y)
    });
}

/// A document holding `levels` SVG images, each inside the last, the
/// innermost a red square.
fn nested(levels: usize) -> String {
    let mut text =
        format!(r#"<svg {SVG} width="2" height="2"><rect width="2" height="2" fill="red"/></svg>"#);
    for _ in 0..levels {
        text = format!(
            r#"<svg {SVG} width="2" height="2"><image width="2" height="2" href="{}"/></svg>"#,
            data_url("image/svg+xml", text.as_bytes())
        );
    }
    text
}

/// **SVG images nest [`MAX_NESTING`] deep and no deeper.**
#[test]
fn svg_images_nest_to_the_cap() {
    let image = render(&nested(MAX_NESTING), 4);
    assert_exact(&image, RED, |_, _| true);
    let deeper = nested(MAX_NESTING + 1);
    assert!(matches!(
        decode(deeper.as_bytes(), 4, 1, &Limits::DEFAULT),
        Err(ImageError::Unsupported(_))
    ));
}

/// **The images of a document decode within one budget of the caller's
/// `max_bytes`**: two four-pixel-square PNGs take exactly 128 bytes, which
/// a budget of 128 allows and one of 127 refuses; a `use` drawing one image
/// again costs nothing more; and an SVG image's source is charged each time
/// it is drawn.
#[cfg(feature = "png")]
#[test]
fn images_decode_within_one_budget() {
    let limits = |max_bytes| Limits {
        max_width: 64,
        max_height: 64,
        max_bytes,
    };
    let red = png_url(&solid(4, 4, RED));
    let two = svg(4, &format!(r#"<image href="{red}"/><image href="{red}"/>"#));
    assert!(decode(two.as_bytes(), 4, 1, &limits(128)).is_ok());
    assert!(matches!(
        decode(two.as_bytes(), 4, 1, &limits(127)),
        Err(ImageError::TooLarge { .. })
    ));
    let reused = svg(
        4,
        &format!(r##"<image id="i" href="{red}"/><use href="#i"/><use href="#i"/>"##),
    );
    assert!(decode(reused.as_bytes(), 4, 1, &limits(64)).is_ok());
    let inner = format!(r#"<svg {SVG} width="4" height="4"/>"#);
    let twice = svg(
        4,
        &format!(
            r##"<image id="i" href="{}"/><use href="#i"/>"##,
            data_url("image/svg+xml", inner.as_bytes())
        ),
    );
    let both = 2 * inner.len();
    assert!(decode(twice.as_bytes(), 4, 1, &limits(both)).is_ok());
    assert!(matches!(
        decode(twice.as_bytes(), 4, 1, &limits(both - 1)),
        Err(ImageError::Unsupported(_))
    ));
    // An SVG image's own images count against the same budget, decoded
    // again each time it is drawn: twice its source and twice its PNG.
    let holder = format!(r#"<svg {SVG} width="4" height="4"><image href="{red}"/></svg>"#);
    let drawn_twice = svg(
        4,
        &format!(
            r##"<image id="i" href="{}"/><use href="#i"/>"##,
            data_url("image/svg+xml", holder.as_bytes())
        ),
    );
    let all = 2 * holder.len() + 2 * 64;
    assert!(decode(drawn_twice.as_bytes(), 4, 1, &limits(all)).is_ok());
    assert!(decode(drawn_twice.as_bytes(), 4, 1, &limits(all - 1)).is_err());
}

/// **Elements an SVG image draws count against the document's instance
/// cap**: an image of ten thousand squares drawn ten times passes it,
/// though no one drawing does.
#[test]
fn svg_images_share_the_instance_cap() {
    let mut body = String::from(r#"<rect id="l0" width="1" height="1"/>"#);
    for level in 1..5 {
        body.push_str(&format!(r#"<g id="l{level}">"#));
        for _ in 0..10 {
            body.push_str(&format!(r##"<use href="#l{}"/>"##, level - 1));
        }
        body.push_str("</g>");
    }
    let inner =
        format!(r##"<svg {SVG} width="4" height="4"><defs>{body}</defs><use href="#l4"/></svg>"##);
    let url = data_url("image/svg+xml", inner.as_bytes());
    // One drawing alone renders.
    render(&svg(4, &format!(r#"<image href="{url}"/>"#)), 4);
    let uses = r##"<use href="#i"/>"##.repeat(9);
    let error = refusal(&svg(4, &format!(r#"<image id="i" href="{url}"/>{uses}"#)));
    assert!(
        matches!(&error, ImageError::Unsupported(why) if why.contains("elements drawn")),
        "{error:?}"
    );
}

/// Draws `bytes` of `media_type` at its own size, decoded by its own
/// decoder as `decoded`, and checks every pixel against that decode:
/// colour within a step where alpha is partial (the canvas holds it
/// premultiplied), exact otherwise.
#[cfg(any(feature = "png", feature = "jpeg", feature = "webp"))]
fn assert_drawn_as_decoded(media_type: &str, bytes: &[u8], decoded: &Rgba8Image) {
    let side = decoded.width().max(decoded.height());
    let image = render(
        &svg(
            side,
            &format!(r#"<image href="{}"/>"#, data_url(media_type, bytes)),
        ),
        side,
    );
    for y in 0..side {
        for x in 0..side {
            let got = pixel(&image, x, y);
            let inside = x < decoded.width() && y < decoded.height();
            let want = if inside { pixel(decoded, x, y) } else { [0; 4] };
            let matches = match want[3] {
                0 => got[3] == 0,
                255 => got == want,
                _ => got[3] == want[3] && got.iter().zip(want).all(|(g, w)| g.abs_diff(w) <= 1),
            };
            assert!(matches, "{media_type} ({x}, {y}): {got:?} for {want:?}");
        }
    }
}

/// **Each media type goes to its own decoder**: a PNG, a JPEG and a WebP
/// drawn at their own size are their decoders' pixels.
#[cfg(all(feature = "png", feature = "jpeg", feature = "webp"))]
#[test]
fn each_media_type_goes_to_its_decoder() {
    let pixels: Vec<u8> = (0u8..12)
        .flat_map(|i| [i * 20, 255 - i * 20, 128, 255])
        .collect();
    let png = Rgba8Image::new(4, 3, pixels).unwrap_or_else(|e| panic!("{e}"));
    let png_bytes = crate::png::encode(&png).unwrap_or_else(|e| panic!("{e}"));
    assert_drawn_as_decoded("image/png", &png_bytes, &png);
    let jpeg_bytes = include_bytes!("../../../fixtures/jpeg/grey.jpg");
    let jpeg = crate::jpeg::decode(jpeg_bytes, &Limits::DEFAULT).unwrap_or_else(|e| panic!("{e}"));
    assert_drawn_as_decoded("image/jpeg", jpeg_bytes, &jpeg);
    let webp_bytes = include_bytes!("../../../fixtures/webp/four-colours.webp");
    let webp = crate::webp::decode(webp_bytes, &Limits::DEFAULT).unwrap_or_else(|e| panic!("{e}"));
    assert_drawn_as_decoded("image/webp", webp_bytes, &webp);
    // A PNG cut short is the decoder's to refuse.
    let cut = svg(
        4,
        &format!(
            r#"<image href="{}"/>"#,
            data_url("image/png", &png_bytes[..40])
        ),
    );
    assert!(matches!(
        decode(cut.as_bytes(), 4, 1, &Limits::DEFAULT),
        Err(ImageError::Malformed(_))
    ));
    // Bytes of one format labelled as another go to the labelled decoder,
    // which refuses them.
    for (media_type, bytes) in [
        ("image/png", &jpeg_bytes[..]),
        ("image/jpeg", &webp_bytes[..]),
        ("image/webp", &png_bytes[..]),
    ] {
        let text = svg(
            4,
            &format!(r#"<image href="{}"/>"#, data_url(media_type, bytes)),
        );
        assert!(
            decode(text.as_bytes(), 4, 1, &Limits::DEFAULT).is_err(),
            "{media_type}"
        );
    }
}

/// **A format whose feature is off is refused by name**, never drawn as
/// nothing.
#[cfg(not(all(feature = "png", feature = "jpeg", feature = "webp")))]
#[test]
fn a_format_not_built_is_refused_by_name() {
    for (media_type, feature) in [
        ("image/png", cfg!(feature = "png")),
        ("image/jpeg", cfg!(feature = "jpeg")),
        ("image/webp", cfg!(feature = "webp")),
    ] {
        if feature {
            continue;
        }
        let text = svg(
            4,
            &format!(r#"<image href="{}"/>"#, data_url(media_type, b"x")),
        );
        let Err(ImageError::Unsupported(why)) = decode(text.as_bytes(), 4, 1, &Limits::DEFAULT)
        else {
            panic!("{media_type} was not refused");
        };
        assert!(why.contains("feature is off"), "{why}");
    }
}

/// **A reference outside the file is refused, never read**, wherever the
/// `<image>` is, and so is a media type this renderer does not draw and a
/// truncated payload.
#[test]
fn external_and_unknown_images_are_refused() {
    for href in [
        "x.png",
        "/etc/passwd",
        "file:///etc/passwd",
        "https://example.com/icon.png",
        "data:image/gif;base64,R0lGODdh",
    ] {
        for attribute in ["href", "xlink:href"] {
            let text = svg(
                8,
                &format!(r#"<image width="2" height="2" {attribute}="{href}"/>"#),
            );
            assert!(
                matches!(refusal(&text), ImageError::Unsupported(_)),
                "{attribute}={href}"
            );
        }
    }
    let truncated = svg(
        8,
        r#"<image width="2" height="2" href="data:image/png;base64,iVBOR"/>"#,
    );
    assert!(matches!(refusal(&truncated), ImageError::Malformed(_)));
    // Unused, as in `<defs>`, it refuses nothing.
    render(
        &svg(
            8,
            r#"<defs><image href="/etc/passwd" width="2" height="2"/></defs>"#,
        ),
        8,
    );
}
