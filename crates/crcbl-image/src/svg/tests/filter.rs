//! Filters through [`decode`]: regions, units, subregions, the result
//! graph, colour spaces and where SVG says an element is not rendered, each
//! checked against pixels worked out by hand.

use super::*;
use crate::svg::filter::MAX_PRIMITIVES;

/// A filter with `primitives` and the region attributes `region`, applied
/// to `target`.
fn filtered(side: u32, region: &str, primitives: &str, target: &str) -> String {
    svg(
        side,
        &format!(r#"<filter id="f" {region}>{primitives}</filter>{target}"#),
    )
}

/// **The default region is the bounding box grown by a tenth each way**,
/// and a region in user units takes percentages of the viewport.
#[test]
fn the_region_follows_its_units() {
    let image = render(
        &filtered(
            40,
            "",
            r#"<feFlood flood-color="red"/>"#,
            r#"<rect x="10" y="10" width="20" height="10" filter="url(#f)"/>"#,
        ),
        40,
    );
    // Across, 10 − 20/10 = 8 to 30 + 2 = 32; down, 10 − 1 = 9 to 21.
    assert_exact(&image, RED, |x, y| {
        (8..32).contains(&x) && (9..21).contains(&y)
    });
    let user = render(
        &filtered(
            40,
            r#"filterUnits="userSpaceOnUse" x="25%" y="0" width="50%" height="4""#,
            r#"<feFlood flood-color="red"/>"#,
            r#"<rect x="30" y="30" width="2" height="2" filter="url(#f)"/>"#,
        ),
        40,
    );
    assert_exact(&user, RED, |x, y| (10..30).contains(&x) && y < 4);
    // In bounding-box units, numbers are fractions of the box.
    let fractions = render(
        &filtered(
            40,
            r#"x="0.5" y="0" width="0.5" height="25%""#,
            r#"<feFlood flood-color="red"/>"#,
            r#"<rect x="8" y="8" width="16" height="16" filter="url(#f)"/>"#,
        ),
        40,
    );
    assert_exact(&fractions, RED, |x, y| {
        (16..24).contains(&x) && (8..12).contains(&y)
    });
}

/// **A region on fractional pixels covers every pixel it touches**, and
/// the filter clips what is drawn to it.
#[test]
fn the_region_rounds_outwards_and_clips() {
    let image = render(
        &filtered(
            8,
            r#"filterUnits="userSpaceOnUse" x="1.5" y="0" width="2" height="8""#,
            r#"<feOffset/>"#,
            r#"<rect width="8" height="8" fill="red" filter="url(#f)"/>"#,
        ),
        8,
    );
    assert_exact(&image, RED, |x, _| (1..4).contains(&x));
}

/// **Primitive subregions take user lengths or, with
/// `primitiveUnits="objectBoundingBox"`, fractions of the box**, each
/// absent one the default's; and lengths like `dx` scale with the box too.
#[test]
fn subregions_and_lengths_follow_the_primitive_units() {
    let user = render(
        &filtered(
            8,
            r#"filterUnits="userSpaceOnUse" x="0" y="0" width="8" height="8""#,
            r#"<feFlood x="2" y="3" width="4" height="1" flood-color="blue"/>"#,
            r#"<rect width="1" height="1" filter="url(#f)"/>"#,
        ),
        8,
    );
    assert_exact(&user, BLUE, |x, y| (2..6).contains(&x) && y == 3);
    // The box is 8..24; its region 6.4..25.6 covers rows 6..26.
    let boxed = render(
        &filtered(
            32,
            r#"primitiveUnits="objectBoundingBox""#,
            r#"<feFlood x="0.5" width="50%" flood-color="red"/>"#,
            r#"<rect x="8" y="8" width="16" height="16" filter="url(#f)"/>"#,
        ),
        32,
    );
    assert_exact(&boxed, RED, |x, y| {
        (16..24).contains(&x) && (6..26).contains(&y)
    });
    // A quarter of a 16-wide box is 4 pixels.
    let moved = render(
        &filtered(
            32,
            r#"filterUnits="userSpaceOnUse" x="0" y="0" width="32" height="32" primitiveUnits="objectBoundingBox""#,
            r#"<feOffset dx="0.25"/>"#,
            r#"<rect width="16" height="32" fill="red" filter="url(#f)"/>"#,
        ),
        32,
    );
    assert_exact(&moved, RED, |x, _| (4..20).contains(&x));
}

/// **Results chain by name, `in` defaults to the previous result (the
/// source for the first), a name nothing earlier has is read as no name,
/// `SourceAlpha` is the source in black, and `feMerge` stacks its nodes in
/// order**: a blue copy of a red bar's alpha, moved beside it, under it.
#[test]
fn results_chain_through_the_graph() {
    let image = render(
        &filtered(
            8,
            r#"filterUnits="userSpaceOnUse" x="0" y="0" width="8" height="8""#,
            r#"<feOffset in="SourceAlpha" dx="2" result="shadow"/>
<feFlood flood-color="blue" result="blue"/>
<feComposite in="blue" in2="shadow" operator="in" result="tinted"/>
<feMerge><feMergeNode in="tinted"/><feMergeNode in="SourceGraphic"/></feMerge>"#,
            r#"<rect width="4" height="8" fill="red" filter="url(#f)"/>"#,
        ),
        8,
    );
    // The bar on 0..4 over its blue copy on 2..6.
    for y in 0..8 {
        for x in 0..8 {
            let want = match x {
                0..4 => RED,
                4..6 => BLUE,
                _ => [0; 4],
            };
            assert_eq!(pixel(&image, x, y), want, "({x}, {y})");
        }
    }
    let defaults = render(
        &filtered(
            8,
            r#"filterUnits="userSpaceOnUse" x="0" y="0" width="8" height="8""#,
            r#"<feOffset dx="2"/><feOffset in="later" dx="2"/><feOffset dx="2" result="later"/>"#,
            r#"<rect width="2" height="8" fill="red" filter="url(#f)"/>"#,
        ),
        8,
    );
    assert_exact(&defaults, RED, |x, _| (6..8).contains(&x));
    // A name two results share means the later.
    let repeated = render(
        &filtered(
            8,
            r#"filterUnits="userSpaceOnUse" x="0" y="0" width="8" height="8""#,
            r#"<feOffset dx="1" result="a"/><feOffset in="SourceGraphic" dx="3" result="a"/><feFlood/><feOffset in="a"/>"#,
            r#"<rect width="2" height="8" fill="red" filter="url(#f)"/>"#,
        ),
        8,
    );
    assert_exact(&repeated, RED, |x, _| (3..5).contains(&x));
}

/// **A subregion defaults to the union of the subregions it reads**, so
/// an offset of a narrow flood is cut to the flood's columns unless its own
/// width says otherwise.
#[test]
fn a_subregion_defaults_to_its_inputs() {
    let cut = render(
        &filtered(
            8,
            r#"filterUnits="userSpaceOnUse" x="0" y="0" width="8" height="8""#,
            r#"<feFlood width="2" flood-color="red"/><feOffset dx="2"/>"#,
            r#"<rect width="1" height="1" filter="url(#f)"/>"#,
        ),
        8,
    );
    assert_exact(&cut, RED, |_, _| false);
    let widened = render(
        &filtered(
            8,
            r#"filterUnits="userSpaceOnUse" x="0" y="0" width="8" height="8""#,
            r#"<feFlood width="2" flood-color="red"/><feOffset dx="2" width="8"/>"#,
            r#"<rect width="1" height="1" filter="url(#f)"/>"#,
        ),
        8,
    );
    assert_exact(&widened, RED, |x, _| (2..4).contains(&x));
}

/// **A blur spreads by the specification's three boxes, its deviation in
/// user space scaled by the transform**: a one-pixel column blurred
/// across by 1.3 under a scale of two is three boxes of 5, whose centre
/// keeps 19/125 of it and whose edge, six pixels out, 1/125.
#[test]
fn blurs_scale_with_the_transform() {
    let image = render(
        &filtered(
            21,
            r#"filterUnits="userSpaceOnUse" x="0" y="0" width="21" height="21""#,
            r#"<feGaussianBlur stdDeviation="1.3 0"/>"#,
            r#"<rect x="5" width="0.5" height="21" filter="url(#f)"/>"#,
        ),
        42,
    );
    let byte = |fraction: f64| (fraction * 255.0 + 0.5) as u8;
    assert_eq!(alpha(&image, 10, 20), byte(19.0 / 125.0));
    assert_eq!(alpha(&image, 4, 20), byte(1.0 / 125.0));
    assert_eq!(alpha(&image, 16, 20), byte(1.0 / 125.0));
    assert_eq!(alpha(&image, 3, 20), 0);
    assert_eq!(alpha(&image, 17, 20), 0);
    // A quarter turn takes the blur across the column to down the row it
    // becomes, and the offset along it to down too.
    let turned = render(
        &filtered(
            21,
            r#"filterUnits="userSpaceOnUse" x="0" y="0" width="21" height="21""#,
            r#"<feGaussianBlur stdDeviation="2.6 0"/><feOffset dx="1"/>"#,
            r#"<g transform="rotate(90 10.5 10.5)"><rect x="10" width="1" height="21" filter="url(#f)"/></g>"#,
        ),
        21,
    );
    assert_eq!(alpha(&turned, 7, 11), byte(19.0 / 125.0));
    assert_eq!(alpha(&turned, 7, 5), byte(1.0 / 125.0));
    assert_eq!(alpha(&turned, 7, 4), 0);
    // A negative deviation passes the input through.
    let negative = render(
        &filtered(
            21,
            r#"filterUnits="userSpaceOnUse" x="0" y="0" width="21" height="21""#,
            r#"<feGaussianBlur stdDeviation="-1 2.6"/>"#,
            r#"<rect x="10" width="1" height="21" filter="url(#f)"/>"#,
        ),
        21,
    );
    assert_exact(&negative, [0, 0, 0, 255], |x, _| x == 10);
}

/// **Primitives work in linearRGB unless `color-interpolation-filters`
/// says sRGB**: halving red's channel gives 0.5 linear, 188 encoded, in the
/// first and 128 in the second.
#[test]
fn colour_interpolation_picks_the_space() {
    let halve_red = |space: &str| {
        render(
            &filtered(
                4,
                space,
                r#"<feColorMatrix values="0.5 0 0 0 0 0 1 0 0 0 0 0 1 0 0 0 0 0 1 0"/>"#,
                r#"<rect width="4" height="4" fill="red" filter="url(#f)"/>"#,
            ),
            4,
        )
    };
    // 1.055 · 0.5^(1/2.4) − 0.055 = 0.7354.
    assert_eq!(pixel(&halve_red(""), 1, 1), [188, 0, 0, 255]);
    assert_eq!(
        pixel(&halve_red(r#"color-interpolation-filters="sRGB""#), 1, 1),
        [128, 0, 0, 255]
    );
    assert_eq!(
        pixel(
            &halve_red(r#"style="color-interpolation-filters: sRGB""#),
            1,
            1
        ),
        [128, 0, 0, 255],
        "inherited from the filter element's style"
    );
    // A flood's colour is sRGB whatever space its primitive works in, and
    // its opacity scales it.
    let flood = render(
        &filtered(
            4,
            "",
            r##"<feFlood flood-color="#808080" flood-opacity="0.5"/>"##,
            r#"<rect width="4" height="4" filter="url(#f)"/>"#,
        ),
        4,
    );
    assert_eq!(pixel(&flood, 1, 1), [128, 128, 128, 128]);
}

/// **The filter comes before the element's opacity, clip path and mask**:
/// an opaque flood at half opacity is half opaque, and a clip path cuts the
/// flood rather than the shape under it.
#[test]
fn filters_apply_before_the_other_effects() {
    let image = render(
        &filtered(
            8,
            r#"filterUnits="userSpaceOnUse" x="0" y="0" width="8" height="8""#,
            r#"<feFlood flood-color="red"/>"#,
            r#"<clipPath id="c"><rect width="4" height="8"/></clipPath><rect x="6" width="1" height="1" opacity="0.5" clip-path="url(#c)" filter="url(#f)"/>"#,
        ),
        8,
    );
    assert_exact(&image, [255, 0, 0, 128], |x, _| x < 4);
    // A filter inside a mask draws there too, as Adwaita's icons use it.
    let masked = render(
        &svg(
            4,
            r#"<filter id="w" x="0" y="0" width="1" height="1"><feColorMatrix values="0 0 0 0 1 0 0 0 0 1 0 0 0 0 1 0 0 0 1 0"/></filter><mask id="m"><rect width="2" height="4" filter="url(#w)"/></mask><rect width="4" height="4" fill="red" mask="url(#m)"/>"#,
        ),
        4,
    );
    assert_exact(&masked, RED, |x, _| x < 2);
}

/// **Where SVG 1.1 says the element is not rendered, it is not**: a
/// reference to nothing, a filter with no primitives, an empty region, and
/// bounding-box units on an element with no box.
#[test]
fn unrenderable_filters_hide_the_element() {
    for (filter, target) in [
        (
            "",
            r#"<rect width="4" height="4" fill="red" filter="url(#none)"/>"#,
        ),
        (
            "<desc>empty</desc>",
            r#"<rect width="4" height="4" fill="red" filter="url(#f)"/>"#,
        ),
        (
            r#"<feOffset/>"#,
            r#"<path d="M0 2 H4" stroke="red" filter="url(#f)"/>"#,
        ),
    ] {
        let image = render(&filtered(4, "", filter, target), 4);
        assert_exact(&image, RED, |_, _| false);
    }
    let empty = render(
        &filtered(
            4,
            r#"width="0""#,
            r#"<feOffset/>"#,
            r#"<rect width="4" height="4" fill="red" filter="url(#f)"/>"#,
        ),
        4,
    );
    assert_exact(&empty, RED, |_, _| false);
    let no_box = render(
        &filtered(
            4,
            r#"filterUnits="userSpaceOnUse" x="0" y="0" width="4" height="4" primitiveUnits="objectBoundingBox""#,
            r#"<feOffset/>"#,
            r#"<path d="M0 2 H4" stroke="red" filter="url(#f)"/>"#,
        ),
        4,
    );
    assert_exact(&no_box, RED, |_, _| false);
    // Bounding-box units are the defaults that need a box; user units
    // throughout draw a line, which has none.
    let user = render(
        &filtered(
            4,
            r#"filterUnits="userSpaceOnUse" x="0" y="0" width="4" height="4""#,
            r#"<feOffset/>"#,
            r#"<path d="M0 2 H4" stroke="red" stroke-width="2" filter="url(#f)"/>"#,
        ),
        4,
    );
    assert_exact(&user, RED, |_, y| (1..3).contains(&y));
}

/// **What the filter subset does not draw is refused by name, and a value
/// that cannot be read is malformed**, never a partial picture.
#[test]
fn unsupported_filters_refuse_the_file() {
    let target = r#"<rect width="4" height="4" filter="url(#f)"/>"#;
    for (region, primitives) in [
        ("", r#"<feTurbulence baseFrequency="0.1"/>"#),
        ("", r#"<feBlend in2="SourceGraphic"/>"#),
        ("", r#"<feImage href="x.png"/>"#),
        ("", r#"<feDropShadow/>"#),
        ("", r#"<feOffset in="BackgroundImage"/>"#),
        ("", r#"<feComposite in2="FillPaint"/>"#),
        ("", r#"<feComposite operator="lighter"/>"#),
        ("", r#"<feColorMatrix type="hueShift"/>"#),
        ("", r#"<feGaussianBlur stdDeviation="1" edgeMode="wrap"/>"#),
        ("", r#"<feMerge><feOffset/></feMerge>"#),
        (
            "",
            r#"<feOffset><animate attributeName="dx" to="2"/></feOffset>"#,
        ),
        (r##"href="#g""##, r#"<feOffset/>"#),
        (r#"filterUnits="strokeBox""#, r#"<feOffset/>"#),
    ] {
        let text = filtered(8, region, primitives, target);
        assert!(
            matches!(refusal(&text), ImageError::Unsupported(_)),
            "{text}"
        );
    }
    let wrong_kind = svg(
        8,
        r##"<rect id="r" width="1" height="1"/><rect width="4" height="4" filter="url(#r)"/>"##,
    );
    assert!(matches!(refusal(&wrong_kind), ImageError::Unsupported(_)));
    for primitives in [
        r#"<feGaussianBlur stdDeviation="1 2 3"/>"#,
        r#"<feGaussianBlur stdDeviation="1e999"/>"#,
        r#"<feOffset dx="1 2"/>"#,
        r#"<feColorMatrix values="1 0 0"/>"#,
        r#"<feColorMatrix type="saturate" values="0.5 0.5"/>"#,
        r#"<feComposite operator="arithmetic" k1="x"/>"#,
        r#"<feFlood width="-1"/>"#,
    ] {
        let text = filtered(8, "", primitives, target);
        assert!(matches!(refusal(&text), ImageError::Malformed(_)), "{text}");
    }
}

/// **A filter of more primitives than the cap is refused**, merge nodes
/// counted, **and neither a huge region nor a huge deviation costs more
/// than the canvas**: the region is cut to the square, and the blur's boxes
/// sum from prefix sums.
#[test]
fn filters_are_capped() {
    let offsets = |count: usize| "<feOffset/>".repeat(count);
    let target = r#"<rect width="4" height="4" fill="red" filter="url(#f)"/>"#;
    render(&filtered(8, "", &offsets(MAX_PRIMITIVES), target), 8);
    let error = refusal(&filtered(8, "", &offsets(MAX_PRIMITIVES + 1), target));
    assert!(
        matches!(&error, ImageError::Unsupported(why) if why.contains("primitives")),
        "{error:?}"
    );
    let merge = format!(
        "{}<feMerge>{}</feMerge>",
        offsets(MAX_PRIMITIVES - 1),
        "<feMergeNode/>"
    );
    let error = refusal(&filtered(8, "", &merge, target));
    assert!(
        matches!(&error, ImageError::Unsupported(why) if why.contains("primitives")),
        "{error:?}"
    );
    let huge = render(
        &filtered(
            8,
            r#"filterUnits="userSpaceOnUse" x="-1e9" y="-1e9" width="2e9" height="2e9""#,
            r#"<feFlood flood-color="red"/>"#,
            target,
        ),
        8,
    );
    assert_exact(&huge, RED, |_, _| true);
    let wide = render(
        &filtered(8, "", r#"<feGaussianBlur stdDeviation="1e300"/>"#, target),
        8,
    );
    assert_exact(&wide, RED, |_, _| false);
}
