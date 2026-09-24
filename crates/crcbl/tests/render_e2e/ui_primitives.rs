//! [`Scene::UiPrimitives`]: UI rung 1's draw-list
//! primitives, held to their golden and to one relation per primitive read off
//! the frame itself.
//!
//! A file of its own rather than more of `render_e2e.rs`, which is the largest
//! test in the crate; every helper it reads is that file's or
//! `crcbl::screenshot`'s, whose `ui_primitives_layout` places every sample
//! below.

use crcbl::screenshot::{
    Scene, UI_PRIMITIVES_CHECKER, UI_PRIMITIVES_NINE_CENTRE, UI_PRIMITIVES_NINE_CORNERS,
    UI_PRIMITIVES_NINE_EDGE, ui_primitives_layout,
};
use crcbl_golden::Image;

use super::{EXTENT, differ};

/// The anti-vacuity colour count: the clear, two fills, a border, two checker
/// colours, four corners, an edge band and a centre — twelve before a single
/// blended corner pixel is counted.
const MIN_COLORS_UI_PRIMITIVES: usize = 12;

/// How far a sampled flat colour may sit from the value it was authored as,
/// per channel: the sRGB decode, the blend and the encode round-trip one level.
const FLAT: u8 = 2;

fn at(image: &Image, point: glam::Vec2) -> [u8; 4] {
    image
        .pixel(point.x as u32, point.y as u32)
        .unwrap_or_else(|| panic!("{point:?} is outside the frame"))
}

/// Whether `pixel` is `expected` to within [`FLAT`] on every colour channel.
fn is(pixel: [u8; 4], expected: [u8; 3]) -> bool {
    pixel[..3]
        .iter()
        .zip(&expected)
        .all(|(left, right)| left.abs_diff(*right) <= FLAT)
}

fn rgb(pixel: [u8; 4]) -> [u8; 3] {
    [pixel[0], pixel[1], pixel[2]]
}

/// **Every primitive is where it was put and is what it claims to be**, each as
/// a relation between pixels of this one frame:
///
/// * the filled rectangle's corner is **rounded and smooth**: its outermost
///   pixel is the clear, its straight edge is the fill, and along the corner's
///   diagonal at least one pixel is strictly between the two — coverage, with
///   multisampling off;
/// * the bordered rectangle's corners have **their own radii**: at mirrored
///   positions, the large top-left radius leaves the clear where the small
///   top-right one has already reached its border, and its border is a ring
///   of its own colour round a different fill;
/// * **nothing is drawn outside the clip**: the checker image overhangs its clip
///   on every side and every overhang is the clear, while the clip's first
///   column inside is the checker and the column before it is not;
/// * the nine-slice's **corners are not stretched**: each corner block is its
///   own colour exactly `inset × scale` pixels across, and the next pixel in is
///   the edge band.
fn every_primitive_is_what_and_where_it_says(image: &Image) {
    let layout = ui_primitives_layout(EXTENT);
    let clear = image.pixel(1, 1).expect("inside");

    // --- the rounded corner ------------------------------------------------
    let (min, max) = layout.rounded;
    let fill = at(image, (min + max) * 0.5);
    assert!(
        differ(fill, clear),
        "the filled rectangle drew nothing: {fill:?}"
    );
    let corner = at(image, min);
    assert!(
        is(corner, rgb(clear)),
        "the filled rectangle's outermost corner pixel is {corner:?}, not the clear {clear:?} \
         — the corner is square"
    );
    let edge = at(image, glam::Vec2::new((min.x + max.x) * 0.5, min.y));
    assert!(
        is(edge, rgb(fill)),
        "the filled rectangle's top edge is {edge:?}, not its fill {fill:?} — a straight edge \
         must be as crisp as a rect's"
    );
    let radius = layout.rounded_radius as u32;
    let diagonal: Vec<[u8; 4]> = (0..radius)
        .map(|step| {
            image
                .pixel(min.x as u32 + step, min.y as u32 + step)
                .expect("inside")
        })
        .collect();
    let between = |pixel: [u8; 4]| {
        (0..3).all(|channel| {
            let (low, high) = if fill[channel] < clear[channel] {
                (fill[channel], clear[channel])
            } else {
                (clear[channel], fill[channel])
            };
            (low..=high).contains(&pixel[channel])
        }) && differ(pixel, fill)
            && differ(pixel, clear)
    };
    assert!(
        diagonal.iter().copied().any(between),
        "no pixel on the corner's diagonal {diagonal:?} is between the clear {clear:?} and the \
         fill {fill:?} — the corner is not antialiased"
    );
    assert!(
        is(*diagonal.last().expect("a radius"), rgb(fill)),
        "the diagonal ends at {:?}, not the fill, a radius in",
        diagonal.last()
    );

    // --- per-corner radii and the border -----------------------------------
    let (min, max) = layout.bordered;
    let border = at(image, glam::Vec2::new(min.x + 1.0, (min.y + max.y) * 0.5));
    let inside = at(image, (min + max) * 0.5);
    assert!(
        differ(border, inside) && differ(border, clear) && differ(inside, clear),
        "the bordered rectangle's border {border:?} and fill {inside:?} are not two colours over \
         the clear {clear:?}"
    );
    let reach = layout.border_width;
    let top_left = at(image, min + glam::Vec2::splat(reach));
    let top_right = at(image, glam::Vec2::new(max.x - 1.0 - reach, min.y + reach));
    assert!(
        is(top_left, rgb(clear)),
        "{} px in from the top-left corner is {top_left:?}: its {} px radius should leave the \
         clear there",
        reach,
        layout.bordered_radii.top_left
    );
    assert!(
        is(top_right, rgb(border)),
        "{} px in from the top-right corner is {top_right:?}: its {} px radius should have \
         reached the border {border:?} there",
        reach,
        layout.bordered_radii.top_right
    );

    // --- the clip ----------------------------------------------------------
    let (clip_min, clip_max) = layout.clip;
    let (image_min, image_max) = layout.checker;
    let middle = (clip_min + clip_max) * 0.5;
    for (side, point) in [
        (
            "left",
            glam::Vec2::new((image_min.x + clip_min.x) * 0.5, middle.y),
        ),
        (
            "right",
            glam::Vec2::new((clip_max.x + image_max.x) * 0.5, middle.y),
        ),
        (
            "top",
            glam::Vec2::new(middle.x, (image_min.y + clip_min.y) * 0.5),
        ),
        (
            "bottom",
            glam::Vec2::new(middle.x, (clip_max.y + image_max.y) * 0.5),
        ),
        ("just left", glam::Vec2::new(clip_min.x - 1.0, middle.y)),
    ] {
        let pixel = at(image, point);
        assert!(
            is(pixel, rgb(clear)),
            "the image's {side} overhang at {point:?} is {pixel:?}, not the clear — it was drawn \
             outside its clip"
        );
    }
    for point in [middle, glam::Vec2::new(clip_min.x, middle.y)] {
        let pixel = at(image, point);
        assert!(
            UI_PRIMITIVES_CHECKER
                .iter()
                .any(|colour| is(pixel, *colour)),
            "inside the clip at {point:?} is {pixel:?}, which is neither checker colour"
        );
    }

    // --- the nine-slice ----------------------------------------------------
    let (min, max) = layout.nine;
    let band = (crcbl::screenshot::UI_PRIMITIVES_NINE_INSET as f32 * layout.nine_scale) as u32;
    let (x0, y0, x1, y1) = (min.x as u32, min.y as u32, max.x as u32, max.y as u32);
    for (name, colour, left, top) in [
        ("top-left", UI_PRIMITIVES_NINE_CORNERS[0], x0, y0),
        ("top-right", UI_PRIMITIVES_NINE_CORNERS[1], x1 - band, y0),
        ("bottom-left", UI_PRIMITIVES_NINE_CORNERS[2], x0, y1 - band),
        (
            "bottom-right",
            UI_PRIMITIVES_NINE_CORNERS[3],
            x1 - band,
            y1 - band,
        ),
    ] {
        for y in top..top + band {
            for x in left..left + band {
                let pixel = image.pixel(x, y).expect("inside");
                assert!(
                    is(pixel, colour),
                    "the nine-slice's {name} corner at ({x}, {y}) is {pixel:?}, not {colour:?}"
                );
            }
        }
    }
    for (what, x, y) in [
        ("right of the top-left corner", x0 + band, y0),
        ("under the top-left corner", x0, y0 + band),
        ("left of the bottom-right corner", x1 - band - 1, y1 - 1),
    ] {
        let pixel = image.pixel(x, y).expect("inside");
        assert!(
            is(pixel, UI_PRIMITIVES_NINE_EDGE),
            "the pixel {what} at ({x}, {y}) is {pixel:?}, not the edge band — the corner is \
             not the {band} px across the art says"
        );
    }
    let centre = at(image, (min + max) * 0.5);
    assert!(
        is(centre, UI_PRIMITIVES_NINE_CENTRE),
        "the nine-slice's centre is {centre:?}, not {UI_PRIMITIVES_NINE_CENTRE:?}"
    );
}

/// [`Scene::UiPrimitives`] drawn, against the reference in `tests/golden/`.
#[test]
#[ignore = "needs a real GPU and a backend pin; run tests/run-render-e2e.sh"]
fn the_ui_primitives_scene_draws_every_primitive_and_matches_its_golden() {
    super::draw_scene_and_match_its_golden(
        Scene::UiPrimitives,
        "ui_primitives",
        EXTENT,
        MIN_COLORS_UI_PRIMITIVES,
        every_primitive_is_what_and_where_it_says,
    );
}
