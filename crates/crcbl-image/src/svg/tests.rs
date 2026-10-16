//! Whole files through [`decode`], each feature checked against pixels
//! worked out independently of the renderer: exact coverage of shapes on
//! the pixel grid, coverage of curved shapes by fine point sampling, and the
//! compositing and gradient formulas evaluated by hand.

use super::*;

mod filter;

const SVG: &str =
    r#"xmlns="http://www.w3.org/2000/svg" xmlns:xlink="http://www.w3.org/1999/xlink""#;

/// An `side`-square drawing of `body` in a viewBox of the same size.
fn svg(side: u32, body: &str) -> String {
    format!(r#"<svg {SVG} viewBox="0 0 {side} {side}">{body}</svg>"#)
}

fn render(text: &str, size: u32) -> Rgba8Image {
    decode(text.as_bytes(), size, 1, &Limits::DEFAULT)
        .unwrap_or_else(|error| panic!("{error}\n{text}"))
}

fn refusal(text: &str) -> ImageError {
    decode(text.as_bytes(), 8, 1, &Limits::DEFAULT).expect_err(text)
}

fn pixel(image: &Rgba8Image, x: u32, y: u32) -> [u8; 4] {
    let at = ((y * image.width() + x) * 4) as usize;
    image.pixels()[at..at + 4].try_into().unwrap_or([0; 4])
}

fn alpha(image: &Rgba8Image, x: u32, y: u32) -> u8 {
    pixel(image, x, y)[3]
}

/// `inside(x, y)` for every pixel: opaque `color` inside, transparent out.
fn assert_exact(image: &Rgba8Image, color: [u8; 4], inside: impl Fn(u32, u32) -> bool) {
    for y in 0..image.height() {
        for x in 0..image.width() {
            let want = if inside(x, y) { color } else { [0; 4] };
            assert_eq!(pixel(image, x, y), want, "pixel ({x}, {y})");
        }
    }
}

/// Each pixel's coverage by `inside`, from `samples²` point tests a pixel.
fn sampled(side: u32, samples: u32, inside: impl Fn(f64, f64) -> bool) -> Vec<f64> {
    let mut out = Vec::new();
    for y in 0..side {
        for x in 0..side {
            let mut hits = 0;
            for sy in 0..samples {
                for sx in 0..samples {
                    let px = f64::from(x) + (f64::from(sx) + 0.5) / f64::from(samples);
                    let py = f64::from(y) + (f64::from(sy) + 0.5) / f64::from(samples);
                    hits += u32::from(inside(px, py));
                }
            }
            out.push(f64::from(hits) / f64::from(samples * samples));
        }
    }
    out
}

const RED: [u8; 4] = [255, 0, 0, 255];
const BLUE: [u8; 4] = [0, 0, 255, 255];

/// **A rectangle fills exactly its pixels**, and one offset half a pixel
/// half-covers the pixels on its edges.
#[test]
fn a_rectangle_fills_exactly_its_pixels() {
    let image = render(
        &svg(8, r#"<rect x="2" y="1" width="4" height="3" fill="red"/>"#),
        8,
    );
    assert_exact(&image, RED, |x, y| {
        (2..6).contains(&x) && (1..4).contains(&y)
    });
    let half = render(
        &svg(
            4,
            r#"<rect x="0.5" y="0" width="2" height="4" fill="red"/>"#,
        ),
        4,
    );
    assert_eq!(pixel(&half, 0, 0), [255, 0, 0, 128]);
    assert_eq!(pixel(&half, 1, 2), RED);
    assert_eq!(pixel(&half, 2, 3), [255, 0, 0, 128]);
    assert_eq!(pixel(&half, 3, 0), [0; 4]);
}

/// **The viewport fits the square by its longer side and is centred, and
/// `preserveAspectRatio` places the viewBox in it.**
#[test]
fn the_viewport_fits_and_centres() {
    // A 4×2 drawing at 8 pixels is scaled by two and centred vertically.
    let wide = render(
        &format!(r#"<svg {SVG} viewBox="0 0 4 2"><rect width="4" height="2" fill="red"/></svg>"#),
        8,
    );
    assert_exact(&wide, RED, |_, y| (2..6).contains(&y));
    // A square viewBox in an 8×4 viewport meets it centred: 4 wide, from
    // x = 2; and the viewport is centred in the square, from y = 2.
    let met = render(
        &format!(
            r#"<svg {SVG} width="8" height="4" viewBox="0 0 4 4"><rect width="4" height="4" fill="red"/></svg>"#
        ),
        8,
    );
    assert_exact(&met, RED, |x, y| (2..6).contains(&x) && (2..6).contains(&y));
    // xMinYMid puts it at the left instead; slice fills the viewport and the
    // overflow stays hidden.
    let left = render(
        &format!(
            r#"<svg {SVG} width="8" height="4" viewBox="0 0 4 4" preserveAspectRatio="xMinYMid"><rect width="4" height="4" fill="red"/></svg>"#
        ),
        8,
    );
    assert_exact(&left, RED, |x, y| {
        (0..4).contains(&x) && (2..6).contains(&y)
    });
    let sliced = render(
        &format!(
            r#"<svg {SVG} width="8" height="4" viewBox="0 0 4 4" preserveAspectRatio="xMidYMid slice"><rect width="4" height="4" fill="red"/></svg>"#
        ),
        8,
    );
    assert_exact(&sliced, RED, |_, y| (2..6).contains(&y));
    // Drawing outside a viewport narrower than the square shows in the
    // margin, as where resvg draws an SVG as an image; only a sliced
    // viewBox's overflow is hidden.
    let overflowing = render(
        &format!(
            r#"<svg {SVG} viewBox="0 0 4 2"><rect y="-1" width="4" height="3" fill="red"/></svg>"#
        ),
        8,
    );
    assert_exact(&overflowing, RED, |_, y| y < 6);
    // Width and height alone, with no viewBox, are the user space.
    let sized = render(
        &format!(
            r#"<svg {SVG} width="4" height="4"><rect x="2" width="2" height="4" fill="red"/></svg>"#
        ),
        8,
    );
    assert_exact(&sized, RED, |x, _| x >= 4);
}

/// **A zero, negative or non-finite size is an error, not an image**, and
/// so is a square past the caller's limits, refused before parsing.
#[test]
fn unusable_sizes_are_errors() {
    for text in [
        format!(r#"<svg {SVG} viewBox="0 0 0 10"/>"#),
        format!(r#"<svg {SVG} width="10" height="10" viewBox="0 0 0 10"/>"#),
        format!(r#"<svg {SVG} width="10" height="10" viewBox="0 0 10 1e999"/>"#),
        format!(r#"<svg {SVG} viewBox="0 0 10 -1"/>"#),
        format!(r#"<svg {SVG} viewBox="0 0 1e999 10"/>"#),
        format!(r#"<svg {SVG} viewBox="0 0 10"/>"#),
        format!(r#"<svg {SVG} width="0" height="10"/>"#),
        format!(r#"<svg {SVG} width="10"/>"#),
        format!(r#"<svg {SVG}/>"#),
        r#"<svg viewBox="0 0 10 10"/>"#.to_owned(),
    ] {
        assert!(matches!(refusal(&text), ImageError::Malformed(_)), "{text}");
    }
    let good = svg(4, "");
    assert_eq!(
        decode(good.as_bytes(), 0, 1, &Limits::DEFAULT),
        Err(ImageError::Empty {
            width: 0,
            height: 0
        })
    );
    assert!(matches!(
        decode(good.as_bytes(), u32::MAX, 2, &Limits::DEFAULT),
        Err(ImageError::TooLarge { .. })
    ));
    let small = Limits {
        max_width: 4,
        max_height: 4,
        max_bytes: 64,
    };
    assert!(matches!(
        decode(b"not even XML", 8, 1, &small),
        Err(ImageError::TooLarge { .. })
    ));
    // A size times a scale is the square.
    let scaled = decode(good.as_bytes(), 3, 2, &Limits::DEFAULT).expect("renders");
    assert_eq!((scaled.width(), scaled.height()), (6, 6));
}

/// **Curved shapes cover each pixel by their exact area**, compared with
/// coverage sampled from their equations: a circle, an ellipse, a rounded
/// rectangle's corner and an arc.
#[test]
fn curved_shapes_cover_their_area() {
    const SAMPLES: u32 = 64;
    // The rasteriser's flattening tolerance, the sampling's resolution and
    // the byte's rounding.
    let within = 1.0 / 32.0 + 2.0 / f64::from(SAMPLES) + 1.0 / 255.0;
    let check = |body: &str, inside: &dyn Fn(f64, f64) -> bool| {
        let image = render(&svg(12, body), 12);
        let want = sampled(12, SAMPLES, inside);
        for (index, want) in want.iter().enumerate() {
            let got = f64::from(image.pixels()[index * 4 + 3]) / 255.0;
            assert!(
                (got - want).abs() <= within,
                "{body}: pixel {index} has {got}, want {want}"
            );
        }
    };
    check(r#"<circle cx="6" cy="5.5" r="4.3"/>"#, &|x, y| {
        (x - 6.0).powi(2) + (y - 5.5).powi(2) <= 4.3 * 4.3
    });
    check(r#"<ellipse cx="6" cy="6" rx="5" ry="2.5"/>"#, &|x, y| {
        ((x - 6.0) / 5.0).powi(2) + ((y - 6.0) / 2.5).powi(2) <= 1.0
    });
    check(
        r#"<rect x="1" y="1" width="10" height="10" rx="4"/>"#,
        &|x, y| {
            let cx = x.clamp(5.0, 7.0);
            let cy = y.clamp(5.0, 7.0);
            (1.0..=11.0).contains(&x)
                && (1.0..=11.0).contains(&y)
                && (x - cx).powi(2) + (y - cy).powi(2) <= 16.0
        },
    );
    // A half disc of radius 5 above the line y = 6, drawn as an arc.
    check(r#"<path d="M1 6 A5 5 0 0 1 11 6 Z"/>"#, &|x, y| {
        y <= 6.0 && (x - 6.0).powi(2) + (y - 6.0).powi(2) <= 25.0
    });
}

/// **Polygons, polylines and lines are the shapes their points make**: a
/// triangle halves the pixels its hypotenuse crosses.
#[test]
fn point_lists_draw_their_shapes() {
    let triangle = render(&svg(4, r#"<polygon points="0,0 4,0 0,4" fill="red"/>"#), 4);
    assert_eq!(pixel(&triangle, 0, 0), RED);
    assert_eq!(pixel(&triangle, 1, 2), [255, 0, 0, 128]);
    assert_eq!(pixel(&triangle, 3, 3), [0; 4]);
    // A polyline fills as if closed.
    let open = render(&svg(4, r#"<polyline points="0,0 4,0 0,4" fill="red"/>"#), 4);
    assert_eq!(open, triangle);
    let line = render(
        &svg(
            4,
            r#"<line x1="0" y1="2" x2="4" y2="2" stroke="red" stroke-width="2"/>"#,
        ),
        4,
    );
    assert_exact(&line, RED, |_, y| (1..3).contains(&y));
}

/// **`fill-rule` decides whether a contour inside another of the same
/// winding is a hole.**
#[test]
fn the_fill_rule_decides_holes() {
    let shape = |rule: &str| {
        render(
            &svg(
                6,
                &format!(
                    r#"<path fill-rule="{rule}" d="M0 0 H6 V6 H0 Z M2 2 H4 V4 H2 Z" fill="red"/>"#
                ),
            ),
            6,
        )
    };
    assert_exact(&shape("nonzero"), RED, |_, _| true);
    assert_exact(&shape("evenodd"), RED, |x, y| {
        !((2..4).contains(&x) && (2..4).contains(&y))
    });
}

/// **Transforms compose right to left and nest through groups.**
#[test]
fn transforms_compose_and_nest() {
    // The rotation takes the 2×2 square at the origin to x in -2..0; the
    // translation then moves it to 2..4 by 2..4.
    let image = render(
        &svg(
            8,
            r#"<g transform="translate(4 2)"><rect width="2" height="2" transform="rotate(90)" fill="red"/></g>"#,
        ),
        8,
    );
    assert_exact(&image, RED, |x, y| {
        (2..4).contains(&x) && (2..4).contains(&y)
    });
    let scaled = render(
        &svg(
            8,
            r#"<rect width="1" height="1" transform="matrix(4 0 0 2 1 1)" fill="red"/>"#,
        ),
        8,
    );
    assert_exact(&scaled, RED, |x, y| {
        (1..5).contains(&x) && (1..3).contains(&y)
    });
    // A transform that collapses the plane draws nothing.
    let flat = render(
        &svg(
            8,
            r#"<rect width="8" height="8" transform="scale(0 1)" fill="red"/>"#,
        ),
        8,
    );
    assert_exact(&flat, RED, |_, _| false);
}

/// **A stroke covers its outline, with caps, joins and dashes**, and under
/// a non-uniform scale it is scaled too.
#[test]
fn strokes_cover_their_outlines() {
    let butt = render(
        &svg(8, r#"<path d="M2 4 H6" stroke="red" stroke-width="2"/>"#),
        8,
    );
    assert_exact(&butt, RED, |x, y| {
        (2..6).contains(&x) && (3..5).contains(&y)
    });
    let square = render(
        &svg(
            8,
            r#"<path d="M2 4 H6" stroke="red" stroke-width="2" stroke-linecap="square"/>"#,
        ),
        8,
    );
    assert_exact(&square, RED, |x, y| {
        (1..7).contains(&x) && (3..5).contains(&y)
    });
    let dashed = render(
        &svg(
            8,
            r#"<path d="M0 4 H8" stroke="red" stroke-width="2" stroke-dasharray="2 1" stroke-dashoffset="1"/>"#,
        ),
        8,
    );
    // One into the pattern, the first dash has one left: on 0..1, 2..4 and
    // 5..7, with gaps between.
    let on = [true, false, true, true, false, true, true, false];
    assert_exact(&dashed, RED, |x, y| on[x as usize] && (3..5).contains(&y));
    // A stroke of width 1 scaled 2× across and 1× down is 2 wide on a
    // vertical line and 1 on a horizontal one.
    let scaled = render(
        &svg(
            8,
            r#"<path d="M1 1 V4 H3" transform="scale(2 1)" stroke="red" stroke-linejoin="miter" fill="none"/>"#,
        ),
        8,
    );
    assert_eq!(pixel(&scaled, 1, 2), RED);
    assert_eq!(pixel(&scaled, 2, 2), RED);
    assert_eq!(pixel(&scaled, 4, 3), [255, 0, 0, 128]);
    assert_eq!(pixel(&scaled, 4, 4), [255, 0, 0, 128]);
    // `paint-order` paints the stroke first, so the fill covers its inner
    // half.
    let ordered = render(
        &svg(
            8,
            r#"<rect x="2" y="2" width="4" height="4" fill="red" stroke="blue" stroke-width="2" paint-order="stroke"/>"#,
        ),
        8,
    );
    assert_eq!(pixel(&ordered, 2, 2), RED);
    assert_eq!(pixel(&ordered, 1, 1), BLUE);
    let normal = render(
        &svg(
            8,
            r#"<rect x="2" y="2" width="4" height="4" fill="red" stroke="blue" stroke-width="2"/>"#,
        ),
        8,
    );
    assert_eq!(pixel(&normal, 2, 2), BLUE);
}

/// **Opacities multiply, and a group's opacity applies to the group as a
/// whole**: two overlapping opaque squares in a half-opaque group are half
/// opaque where they overlap, where two half-opaque squares are ¾.
#[test]
fn opacity_applies_to_the_whole_group() {
    let fill = render(
        &svg(
            2,
            r#"<rect width="2" height="2" fill="red" fill-opacity="0.5"/>"#,
        ),
        2,
    );
    assert_eq!(pixel(&fill, 0, 0), [255, 0, 0, 128]);
    let group = render(
        &svg(
            4,
            r#"<g opacity="0.5"><rect width="3" height="4" fill="red"/><rect x="1" width="3" height="4" fill="red"/></g>"#,
        ),
        4,
    );
    assert_eq!(pixel(&group, 1, 0), [255, 0, 0, 128]);
    let each = render(
        &svg(
            4,
            r#"<rect width="3" height="4" fill="red" opacity="0.5"/><rect x="1" width="3" height="4" fill="red" opacity="0.5"/>"#,
        ),
        4,
    );
    // 1 - (1 - ½)² = ¾, through an eight-bit canvas: the first square
    // leaves round(127.5) = 128, and the second ½ + ½ · 128/255 of 255.
    let second = (255.0 * (0.5 + 0.5 * 128.0 / 255.0) + 0.5) as u8;
    assert_eq!(second, 192);
    assert_eq!(pixel(&each, 1, 0), [255, 0, 0, second]);
    // A shape's own opacity applies to its fill and stroke together.
    let both = render(
        &svg(
            4,
            r#"<rect x="1" y="1" width="2" height="2" fill="red" stroke="red" stroke-width="2" opacity="0.5"/>"#,
        ),
        4,
    );
    assert_eq!(pixel(&both, 1, 1), [255, 0, 0, 128]);
}

/// **Colours come from every notation, the cascade and `currentColor`**,
/// including breeze's sheet of `.ColorScheme-Text`.
#[test]
fn colours_resolve_through_the_cascade() {
    let image = render(
        &svg(
            8,
            r##"<style type="text/css">.ColorScheme-Text { color:#232629; } rect.b { fill: rgb(0, 128, 255) }</style>
<path class="ColorScheme-Text" d="M0 0 H2 V2 H0 Z" fill="currentColor"/>
<rect class="b" x="2" width="2" height="2" fill="red"/>
<rect x="4" width="2" height="2" fill="red" style="fill:#0f0"/>
<g color="#abcdef"><rect x="6" width="2" height="2" fill="currentColor"/></g>
<rect y="2" width="2" height="2" fill="SteelBlue"/>
<rect x="2" y="2" width="2" height="2"/>"##,
        ),
        8,
    );
    assert_eq!(pixel(&image, 0, 0), [0x23, 0x26, 0x29, 255]);
    assert_eq!(pixel(&image, 2, 0), [0, 128, 255, 255]);
    assert_eq!(pixel(&image, 4, 0), [0, 255, 0, 255]);
    assert_eq!(pixel(&image, 6, 0), [0xab, 0xcd, 0xef, 255]);
    assert_eq!(pixel(&image, 0, 2), [70, 130, 180, 255]);
    assert_eq!(
        pixel(&image, 2, 2),
        [0, 0, 0, 255],
        "fill defaults to black"
    );
}

/// **A linear gradient's pixels are its formula at their centres**, in user
/// space and bounding-box units, inheriting stops and attributes through
/// `href`, with each spread method.
#[test]
fn linear_gradients_follow_their_formula() {
    let ramp = |x: u32| (255.0 * (f64::from(x) + 0.5) / 8.0).round() as u8;
    let check = |defs: &str, id: &str, want: &dyn Fn(u32) -> u8| {
        let image = render(
            &svg(
                8,
                &format!(r#"<defs>{defs}</defs><rect width="8" height="8" fill="url(#{id})"/>"#),
            ),
            8,
        );
        for x in 0..8 {
            let got = pixel(&image, x, 3);
            assert!(
                got[0].abs_diff(want(x)) <= 1 && got[3] == 255,
                "{id} at {x}: {got:?} want {}",
                want(x)
            );
        }
    };
    let stops = r#"<stop offset="0" stop-color="black"/><stop offset="1" stop-color="white"/>"#;
    check(
        &format!(
            r#"<linearGradient id="u" gradientUnits="userSpaceOnUse" x1="0" x2="8">{stops}</linearGradient>"#
        ),
        "u",
        &ramp,
    );
    // The defaults run across the bounding box, which is the canvas.
    check(
        &format!(r#"<linearGradient id="b">{stops}</linearGradient>"#),
        "b",
        &ramp,
    );
    // In bounding-box units the ramp spans the shape, wherever it is: here
    // the right half, from x = 4.
    let image = render(
        &svg(
            8,
            &format!(
                r#"<linearGradient id="o">{stops}</linearGradient><rect x="4" width="4" height="8" fill="url(#o)"/>"#
            ),
        ),
        8,
    );
    for x in 4..8 {
        let want = (255.0 * (f64::from(x - 4) + 0.5) / 4.0).round() as u8;
        assert!(
            pixel(&image, x, 3)[0].abs_diff(want) <= 1,
            "x = {x}: {:?} want {want}",
            pixel(&image, x, 3)
        );
    }
    // Stops from the gradient referenced, coordinates from the referrer.
    check(
        &format!(
            r##"<linearGradient id="a">{stops}</linearGradient><linearGradient id="h" xlink:href="#a" gradientUnits="userSpaceOnUse" x1="0" x2="8"/>"##
        ),
        "h",
        &ramp,
    );
    // Reflected over half the width: up across 0..4, back down across 4..8.
    let reflected = |x: u32| {
        let t = (f64::from(x) + 0.5) / 4.0;
        let t = if t > 1.0 { 2.0 - t } else { t };
        (255.0 * t).round() as u8
    };
    check(
        &format!(
            r#"<linearGradient id="r" gradientUnits="userSpaceOnUse" x2="4" spreadMethod="reflect">{stops}</linearGradient>"#
        ),
        "r",
        &reflected,
    );
    // A gradientTransform scaling x by two stretches the ramp over 0..16.
    let stretched = |x: u32| (255.0 * (f64::from(x) + 0.5) / 16.0).round() as u8;
    check(
        &format!(
            r#"<linearGradient id="t" gradientUnits="userSpaceOnUse" x2="8" gradientTransform="scale(2 1)">{stops}</linearGradient>"#
        ),
        "t",
        &stretched,
    );
    // A single stop is a flat colour, and stop-opacity and currentColor
    // apply to stops.
    let flat = render(
        &svg(
            2,
            r#"<linearGradient id="f"><stop stop-color="currentColor" color="blue" stop-opacity="0.5"/></linearGradient><rect width="2" height="2" fill="url(#f)"/>"#,
        ),
        2,
    );
    assert_eq!(pixel(&flat, 0, 0), [0, 0, 255, 128]);
    // A reference to nothing paints its fallback.
    let fallback = render(
        &svg(2, r#"<rect width="2" height="2" fill="url(#none) blue"/>"#),
        2,
    );
    assert_eq!(pixel(&fallback, 0, 0), BLUE);
}

/// **A radial gradient's pixels are distance over radius at their centres**,
/// and its focus moves where offset 0 is.
#[test]
fn radial_gradients_follow_their_formula() {
    let image = render(
        &svg(
            8,
            r#"<radialGradient id="g" gradientUnits="userSpaceOnUse" cx="4" cy="4" r="4"><stop offset="0" stop-color="black"/><stop offset="1" stop-color="white"/></radialGradient><rect width="8" height="8" fill="url(#g)"/>"#,
        ),
        8,
    );
    for (x, y) in [(4, 4), (5, 2), (7, 7), (0, 3)] {
        let (dx, dy) = (f64::from(x) + 0.5 - 4.0, f64::from(y) + 0.5 - 4.0);
        let want = (255.0 * ((dx * dx + dy * dy).sqrt() / 4.0).min(1.0)).round() as u8;
        assert!(
            pixel(&image, x, y)[0].abs_diff(want) <= 1,
            "({x}, {y}): {:?} want {want}",
            pixel(&image, x, y)
        );
    }
    // With the focus on the circle, at its left edge's midpoint, the pixel
    // just right of it is near offset 0. For a focus on the circle the
    // quadratic loses its square term, so t = |p - f|² / (2 (p - f)·(c - f));
    // at pixel (7, 3), p - f = (0.9375, -0.0625) and c - f = (0.5, 0).
    let focal = render(
        &svg(
            8,
            r#"<radialGradient id="g" cx="0.5" cy="0.5" r="0.5" fx="0" fy="0.5"><stop offset="0" stop-color="black"/><stop offset="1" stop-color="white"/></radialGradient><rect width="8" height="8" fill="url(#g)"/>"#,
        ),
        8,
    );
    assert!(pixel(&focal, 0, 3)[0] < 40);
    let t: f64 = (0.9375f64.powi(2) + 0.0625f64.powi(2)) / (2.0 * 0.9375 * 0.5);
    assert!(
        pixel(&focal, 7, 3)[0].abs_diff((255.0 * t).round() as u8) <= 1,
        "{t}"
    );
}

/// **`use` draws its target moved by `x` and `y`, styled from the `use`**;
/// a reference to nothing draws nothing, and one to itself is refused.
#[test]
fn use_instances_its_target() {
    let image = render(
        &svg(
            8,
            r##"<defs><rect id="r" width="2" height="2"/></defs><use href="#r" x="4" fill="red"/><use xlink:href="#r" y="4" fill="blue"/><use href="#missing"/>"##,
        ),
        8,
    );
    for (x, y, want) in [
        (4, 0, RED),
        (5, 1, RED),
        (0, 4, BLUE),
        (1, 5, BLUE),
        (0, 0, [0; 4]),
        (6, 0, [0; 4]),
        (2, 4, [0; 4]),
    ] {
        assert_eq!(pixel(&image, x, y), want, "({x}, {y})");
    }
    let cycle = svg(8, r##"<g id="g"><use href="#g"/></g>"##);
    assert!(matches!(refusal(&cycle), ImageError::Malformed(_)));
    let mutual = svg(8, r##"<use id="a" href="#b"/><use id="b" href="#a"/>"##);
    assert!(matches!(refusal(&mutual), ImageError::Malformed(_)));
}

/// **A clip path keeps what is inside it**, in user space or the bounding
/// box, by its clip rule, and through a clip path of its own.
#[test]
fn clip_paths_keep_what_is_inside() {
    let left = render(
        &svg(
            8,
            r#"<clipPath id="c"><rect width="4" height="8"/></clipPath><rect width="8" height="8" fill="red" clip-path="url(#c)"/>"#,
        ),
        8,
    );
    assert_exact(&left, RED, |x, _| x < 4);
    let bbox = render(
        &svg(
            8,
            r#"<clipPath id="c" clipPathUnits="objectBoundingBox"><rect y="0.5" width="1" height="0.5"/></clipPath><rect x="2" y="2" width="4" height="4" fill="red" clip-path="url(#c)"/>"#,
        ),
        8,
    );
    assert_exact(&bbox, RED, |x, y| {
        (2..6).contains(&x) && (4..6).contains(&y)
    });
    let ring = render(
        &svg(
            6,
            r#"<clipPath id="c"><path clip-rule="evenodd" d="M0 0 H6 V6 H0 Z M2 2 H4 V4 H2 Z"/></clipPath><rect width="6" height="6" fill="red" clip-path="url(#c)"/>"#,
        ),
        6,
    );
    assert_exact(&ring, RED, |x, y| {
        !((2..4).contains(&x) && (2..4).contains(&y))
    });
    let nested = render(
        &svg(
            8,
            r#"<clipPath id="a"><rect width="8" height="4"/></clipPath><clipPath id="c" clip-path="url(#a)"><rect width="4" height="8"/></clipPath><rect width="8" height="8" fill="red" clip-path="url(#c)"/>"#,
        ),
        8,
    );
    assert_exact(&nested, RED, |x, y| x < 4 && y < 4);
    // A clip path or a mask that is missing is as if none were given.
    let missing = render(
        &svg(
            2,
            r#"<rect width="2" height="2" fill="red" clip-path="url(#none)" mask="url(#nor)"/>"#,
        ),
        2,
    );
    assert_exact(&missing, RED, |_, _| true);
}

/// **A mask scales by its content's luminance**, or its alpha with
/// `mask-type: alpha`, within its region.
#[test]
fn masks_scale_by_luminance() {
    let image = render(
        &svg(
            8,
            r##"<mask id="m" maskUnits="userSpaceOnUse" x="0" y="0" width="8" height="8"><rect width="4" height="8" fill="white"/><rect x="4" width="4" height="8" fill="#808080"/></mask><rect width="8" height="8" fill="red" mask="url(#m)"/>"##,
        ),
        8,
    );
    assert_eq!(pixel(&image, 1, 1), RED);
    // Grey 128 has luminance 128/255 whatever the coefficients, since they
    // sum to one.
    assert_eq!(pixel(&image, 5, 1), [255, 0, 0, 128]);
    // Pure green's luminance is its coefficient, 0.7154.
    let green = render(
        &svg(
            2,
            r#"<mask id="m"><rect width="2" height="2" fill="lime"/></mask><rect width="2" height="2" fill="red" mask="url(#m)"/>"#,
        ),
        2,
    );
    assert_eq!(alpha(&green, 0, 0), (0.7154f64 * 255.0).round() as u8);
    let by_alpha = render(
        &svg(
            2,
            r#"<mask id="m" style="mask-type: alpha"><rect width="2" height="2" fill="black" fill-opacity="0.5"/></mask><rect width="2" height="2" fill="red" mask="url(#m)"/>"#,
        ),
        2,
    );
    assert_eq!(pixel(&by_alpha, 0, 0), [255, 0, 0, 128]);
    // The default region is the bounding box grown by a tenth each way: a
    // mask of everything shows the element whole.
    let region = render(
        &svg(
            8,
            r#"<mask id="m"><rect x="-10" y="-10" width="30" height="30" fill="white"/></mask><rect x="2" y="2" width="4" height="4" fill="red" mask="url(#m)"/>"#,
        ),
        8,
    );
    assert_exact(&region, RED, |x, y| {
        (2..6).contains(&x) && (2..6).contains(&y)
    });
}

/// **`display: none` hides a subtree; `visibility: hidden` hides an element
/// whose children may show themselves again.**
#[test]
fn display_and_visibility_hide_differently() {
    let image = render(
        &svg(
            4,
            r#"<g display="none"><rect width="2" height="2" fill="red" visibility="visible"/></g><g visibility="hidden"><rect x="2" width="2" height="2" fill="red"/><rect y="2" width="2" height="2" fill="red" visibility="visible"/></g>"#,
        ),
        4,
    );
    assert_exact(&image, RED, |x, y| x < 2 && y >= 2);
}

/// **Editors' metadata changes nothing**: Inkscape and Sodipodi elements and
/// attributes, RDF, titles, comments and a DOCTYPE render as if absent.
#[test]
fn metadata_is_ignored_by_design() {
    let plain = render(&svg(4, r#"<rect width="2" height="2" fill="red"/>"#), 4);
    let decorated = render(
        r##"<?xml version="1.0" encoding="UTF-8" standalone="no"?>
<!-- Created with Inkscape -->
<!DOCTYPE svg PUBLIC "-//W3C//DTD SVG 1.1//EN" "http://www.w3.org/Graphics/SVG/1.1/DTD/svg11.dtd">
<svg xmlns="http://www.w3.org/2000/svg" xmlns:inkscape="http://www.inkscape.org/namespaces/inkscape"
     xmlns:sodipodi="http://sodipodi.sourceforge.net/DTD/sodipodi-0.dtd"
     xmlns:rdf="http://www.w3.org/1999/02/22-rdf-syntax-ns#" xmlns:cc="http://creativecommons.org/ns#"
     xmlns:dc="http://purl.org/dc/elements/1.1/" viewBox="0 0 4 4" version="1.1" sodipodi:docname="x.svg">
  <title>an icon</title>
  <sodipodi:namedview pagecolor="#ffffff" inkscape:zoom="8"><inkscape:grid type="xygrid"/></sodipodi:namedview>
  <metadata><rdf:RDF><cc:Work rdf:about=""><dc:format>image/svg+xml</dc:format></cc:Work></rdf:RDF></metadata>
  <g inkscape:label="Layer 1" inkscape:groupmode="layer">
    <rect width="2" height="2" fill="red" inkscape:connector-curvature="0" style="-inkscape-stroke:none;font-family:Sans"><desc>a square</desc></rect>
  </g>
</svg>"##,
        4,
    );
    assert_eq!(decorated, plain);
}

/// **Every unsupported element, attribute, property value and feature
/// refuses the file**, naming it, rather than drawing the rest.
#[test]
fn unsupported_features_refuse_the_file() {
    for body in [
        r#"<text>hi</text>"#,
        r#"<image href="x.png" width="2" height="2"/>"#,
        r#"<filter id="f"><feTurbulence baseFrequency="0.1"/></filter><rect width="2" height="2" filter="url(#f)"/>"#,
        r#"<rect width="2" height="2" style="filter: blur(1px)"/>"#,
        r#"<pattern id="p" width="1" height="1"/><rect width="2" height="2" fill="url(#p)"/>"#,
        r#"<marker id="m"/><path d="M0 0 H2" stroke="red" marker-end="url(#m)"/>"#,
        r##"<symbol id="s"><rect width="2" height="2"/></symbol><use href="#s"/>"##,
        r#"<svg width="2" height="2"/>"#,
        r#"<switch><rect width="2" height="2"/></switch>"#,
        r#"<rect width="2" height="2"><animate attributeName="x" to="4"/></rect>"#,
        r#"<foreignObject width="2" height="2"/>"#,
        r#"<rect width="2" height="2" frobnicate="1"/>"#,
        r#"<path d="M0 0 H2" pathLength="10" stroke="red"/>"#,
        r#"<rect width="2" height="2" systemLanguage="en"/>"#,
        r#"<style>g rect { fill: red }</style>"#,
        r#"<rect width="2" height="2" style="mix-blend-mode: multiply"/>"#,
        r#"<use href="other.svg#icon"/>"#,
        r#"<rect width="2em" height="2"/>"#,
        r#"<mask id="m"><rect width="2" height="2"/></mask><rect width="2" height="2" clip-path="url(#m)"/>"#,
        r#"<clipPath id="c"><text>x</text></clipPath><rect width="2" height="2" clip-path="url(#c)"/>"#,
        r#"<linearGradient id="g"><animate/></linearGradient><rect width="2" height="2" fill="url(#g)"/>"#,
    ] {
        assert!(
            matches!(refusal(&svg(8, body)), ImageError::Unsupported(_)),
            "{body}"
        );
    }
    // Unsupported content nothing uses is not drawn, so it refuses nothing.
    render(
        &svg(
            8,
            r#"<defs><filter id="f"/><text>unused</text><pattern id="p"/></defs>"#,
        ),
        8,
    );
}

/// **A file that multiplies itself through `use` is refused at the
/// instance cap**, before it can draw a billion squares.
#[test]
fn use_bombs_are_capped() {
    let mut body = String::from(r#"<rect id="l0" width="1" height="1"/>"#);
    for level in 1..10 {
        body.push_str(&format!(r#"<g id="l{level}">"#));
        for _ in 0..10 {
            body.push_str(&format!(r##"<use href="#l{}"/>"##, level - 1));
        }
        body.push_str("</g>");
    }
    let error = refusal(&svg(8, &format!("<defs>{body}</defs><use href=\"#l9\"/>")));
    assert!(
        matches!(&error, ImageError::Unsupported(why) if why.contains("elements drawn")),
        "{error:?}"
    );
}

/// **Two halves of a shape wound opposite ways meet without a seam**: the
/// area integral alone would read a pixel half in each as empty, and the
/// finer accumulation grid keeps them apart.
#[test]
fn opposite_windings_meet_without_a_seam() {
    // Left half anticlockwise on screen, right half clockwise, meeting on
    // x = 2.5 inside pixel 2.
    let image = render(
        &svg(
            5,
            r#"<path d="M2.5 0 H0 V5 H2.5 Z M2.5 0 H5 V5 H2.5 Z" fill="red"/>"#,
        ),
        5,
    );
    assert_exact(&image, RED, |_, _| true);
}

/// **A shape taller than one strip of the accumulation grid is drawn
/// without a seam between strips**, its partial rows exact.
#[test]
fn shapes_cross_strips_seamlessly() {
    let image = render(
        &svg(
            40,
            r#"<rect x="2" y="5.5" width="36" height="31.75" fill="red"/>"#,
        ),
        40,
    );
    for y in 0..40 {
        let want = match y {
            5 => 128,
            6..=36 => 255,
            37 => 64,
            _ => 0,
        };
        assert_eq!(alpha(&image, 20, y), want, "row {y}");
    }
}

/// **A repeated id names its last element**, as resvg reads it; a paint
/// reference to something that is not a paint server paints its fallback
/// or nothing.
#[test]
fn references_resolve_as_resvg_and_svg_2_say() {
    let repeated = render(
        &svg(
            2,
            r#"<linearGradient id="g"><stop stop-color="red"/></linearGradient><linearGradient id="g"><stop stop-color="blue"/></linearGradient><rect width="2" height="2" fill="url(#g)"/>"#,
        ),
        2,
    );
    assert_eq!(pixel(&repeated, 0, 0), BLUE);
    let fallback = render(
        &svg(
            2,
            r#"<rect id="r" width="2" height="2" fill="url(#r) blue"/>"#,
        ),
        2,
    );
    assert_eq!(pixel(&fallback, 0, 0), BLUE);
    let nothing = render(
        &svg(2, r#"<rect id="r" width="2" height="2" fill="url(#r)"/>"#),
        2,
    );
    assert_eq!(pixel(&nothing, 0, 0), [0; 4]);
}

/// **What means nothing where it is written changes nothing**: another
/// shape's geometry attributes and Inkscape's unprefixed filter properties;
/// and a miter limit below one bevels every corner, as one does.
#[test]
fn meaningless_attributes_change_nothing() {
    let plain = render(&svg(4, r#"<path d="M0 0 H3 V3 Z" fill="red"/>"#), 4);
    let littered = render(
        &svg(
            4,
            r#"<path d="M0 0 H3 V3 Z" fill="red" width="9" cx="1" r="5" style="filter-blend-mode:normal;filter-gaussianBlur-deviation:0"/>"#,
        ),
        4,
    );
    assert_eq!(littered, plain);
    let corner = |join: &str| {
        render(
            &svg(
                12,
                &format!(
                    r#"<path d="M2 2 H10 V10" fill="none" stroke="red" stroke-width="2" {join}/>"#
                ),
            ),
            12,
        )
    };
    assert_eq!(
        corner(r#"stroke-miterlimit="0""#),
        corner(r#"stroke-linejoin="bevel""#)
    );
    assert_ne!(corner(""), corner(r#"stroke-linejoin="bevel""#));
}

/// **A chain of `use` elements deeper than the walk's depth cap is
/// refused**, before it can exhaust the stack, and **group effects nested
/// past the layer cap are refused** before they allocate a canvas each.
#[test]
fn deep_chains_and_nested_layers_are_capped() {
    let chain: String = (0..300)
        .map(|link| format!(r##"<use id="u{link}" href="#u{}"/>"##, link + 1))
        .collect();
    let body =
        format!(r##"<defs>{chain}<rect id="u300" width="1" height="1"/></defs><use href="#u0"/>"##);
    let error = refusal(&svg(8, &body));
    assert!(
        matches!(&error, ImageError::Unsupported(why) if why.contains("deep")),
        "{error:?}"
    );

    let nested = |depth: usize| {
        let mut body = r#"<g opacity="0.5">"#.repeat(depth);
        body.push_str(r#"<rect width="1" height="1"/>"#);
        body.push_str(&"</g>".repeat(depth));
        svg(2, &body)
    };
    assert!(
        decode(
            nested(render::MAX_LAYERS).as_bytes(),
            2,
            1,
            &Limits::DEFAULT
        )
        .is_ok()
    );
    let error = refusal(&nested(render::MAX_LAYERS + 1));
    assert!(
        matches!(&error, ImageError::Unsupported(why) if why.contains("group effects")),
        "{error:?}"
    );

    // Clip paths clipped by clip paths hold a coverage buffer each, and count
    // against the same cap.
    let clips: String = (0..=render::MAX_LAYERS)
        .map(|link| {
            format!(r#"<clipPath id="c{link}" clip-path="url(#c{})"><rect width="2" height="2"/></clipPath>"#, link + 1)
        })
        .collect();
    let error = refusal(&svg(
        2,
        &format!(r#"{clips}<rect width="2" height="2" clip-path="url(#c0)"/>"#),
    ));
    assert!(
        matches!(&error, ImageError::Unsupported(why) if why.contains("group effects")),
        "{error:?}"
    );
}
