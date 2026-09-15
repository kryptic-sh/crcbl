//! [`Scene::UiText`]: `docs/plan/07-ui-debug.md` rung 5's real fonts, drawn,
//! held to its golden and to relations read off the frame itself.
//!
//! Every claim measures pixels against the stylesheet's colours and lengths
//! and against the font's own kerning — never against the layout's output,
//! which would only prove the layout agrees with itself. `ui_text_layout` is
//! read to know *where to look*, and nowhere else.
//!
//! A pixel is **ink** when it differs from the fill behind it by more than
//! [`FLAT`] in any channel: a glyph's faintest antialiased edge counts, so a
//! claim that no ink is somewhere is a claim about every sliver of coverage.

use crcbl::screenshot::{
    Scene, UI_TEXT_CENTRED_FILL, UI_TEXT_LINE_HEIGHT, UI_TEXT_PAGE, UI_TEXT_PAIR,
    UI_TEXT_PAIR_SIZE, UI_TEXT_PARAGRAPH_FILL, UiTextLayout, ui_text_layout,
};
use crcbl::ui::font::Font;
use crcbl_golden::Image;
use glam::Vec2;

use super::EXTENT;

/// The anti-vacuity colour count: the three fills, the ink, and the
/// antialiased edges between them.
const MIN_COLORS_UI_TEXT: usize = 12;

/// How far a fill pixel may sit from the colour the stylesheet names.
const FLAT: u8 = 2;

/// How far, in pixels, the measured kerning may differ from the font's.
///
/// Each glyph's pen is rounded to the nearest quarter-pixel bin, which moves it
/// at most an eighth of a pixel, and the pair's two `o`s round independently;
/// an ink centroid read through quantised, sRGB-encoded coverage adds a little
/// on top.
const KERNING_SLACK: f64 = 0.35;

/// Whether `pixel` is ink over `fill`; see the module docs.
fn ink(pixel: [u8; 4], fill: [u8; 3]) -> bool {
    pixel[..3]
        .iter()
        .zip(fill)
        .any(|(got, want)| got.abs_diff(want) > FLAT)
}

/// How much ink `pixel` carries over `fill`: its channels' distance from it.
fn weight(pixel: [u8; 4], fill: [u8; 3]) -> f64 {
    pixel[..3]
        .iter()
        .zip(fill)
        .map(|(got, want)| f64::from(got.abs_diff(want)))
        .sum()
}

fn px(image: &Image, x: u32, y: u32) -> [u8; 4] {
    image
        .pixel(x, y)
        .unwrap_or_else(|| panic!("({x}, {y}) is outside the frame"))
}

fn span((min, max): (Vec2, Vec2)) -> (std::ops::Range<u32>, std::ops::Range<u32>) {
    (min.x as u32..max.x as u32, min.y as u32..max.y as u32)
}

/// The runs of rows in `rect` holding ink over `fill`, as `(top, bottom)` with
/// `bottom` exclusive.
fn ink_bands(image: &Image, rect: (Vec2, Vec2), fill: [u8; 3]) -> Vec<(u32, u32)> {
    let (columns, rows) = span(rect);
    let mut bands: Vec<(u32, u32)> = Vec::new();
    for y in rows {
        if !columns.clone().any(|x| ink(px(image, x, y), fill)) {
            continue;
        }
        match bands.last_mut() {
            Some((_, bottom)) if *bottom == y => *bottom += 1,
            _ => bands.push((y, y + 1)),
        }
    }
    bands
}

/// **The paragraph's lines fall on the stylesheet's pitch**: its ink comes in
/// at least three bands of rows, and each band's top and bottom are exactly
/// one `line-height` below the band above's.
fn paragraph_rows_fall_on_the_line_pitch(image: &Image, layout: &UiTextLayout) {
    let bands = ink_bands(image, layout.paragraph, UI_TEXT_PARAGRAPH_FILL);
    assert!(
        bands.len() >= 3,
        "the paragraph drew {bands:?}, not three lines"
    );
    let pitch = UI_TEXT_LINE_HEIGHT as u32;
    for pair in bands.windows(2) {
        assert_eq!(
            (pair[1].0 - pair[0].0, pair[1].1 - pair[0].1),
            (pitch, pitch),
            "consecutive lines' ink is not {pitch} rows apart: {bands:?}"
        );
    }
}

/// **No glyph pixel lies outside the paragraph block's width**: in the block's
/// rows, every pixel left and right of it is the page's fill exactly.
fn no_paragraph_ink_outside_its_block(image: &Image, layout: &UiTextLayout) {
    let (min, max) = layout.paragraph;
    let (_, rows) = span(layout.paragraph);
    let outside = (0..min.x as u32).chain(max.x as u32..image.width());
    let mut stray = Vec::new();
    for y in rows {
        for x in outside.clone() {
            if ink(px(image, x, y), UI_TEXT_PAGE) {
                stray.push((x, y));
            }
        }
    }
    assert!(
        stray.is_empty(),
        "{} pixel(s) of the paragraph's rows beside its block are not the page, first {:?}",
        stray.len(),
        stray.first()
    );
}

/// **The larger word is twice the smaller**: its ink is twice as many rows
/// tall, to within one.
fn the_large_word_is_twice_the_small_one(image: &Image, layout: &UiTextLayout) {
    let height = |rect| {
        let bands = ink_bands(image, rect, UI_TEXT_PAGE);
        assert_eq!(bands.len(), 1, "{bands:?}");
        bands[0].1 - bands[0].0
    };
    let (small, large) = (height(layout.small), height(layout.large));
    assert!(
        large.abs_diff(2 * small) <= 1,
        "the large word is {large} rows tall and the small one {small}"
    );
}

/// The ink-weighted centre, from `rect`'s left edge, of the second run of ink
/// columns in `rect`'s lower half: for `To`, the `o` beside `T`'s stem, below
/// `T`'s bar.
fn second_glyph_centre(image: &Image, rect: (Vec2, Vec2)) -> f64 {
    let (columns, _) = span(rect);
    let (min, max) = rect;
    let rows = (min.y + (max.y - min.y) * 0.55) as u32..max.y as u32;
    let inked: Vec<u32> = columns
        .clone()
        .filter(|&x| rows.clone().any(|y| ink(px(image, x, y), UI_TEXT_PAGE)))
        .collect();
    let mut runs: Vec<Vec<u32>> = Vec::new();
    for x in inked {
        match runs.last_mut() {
            Some(run) if run.last() == Some(&(x - 1)) => run.push(x),
            _ => runs.push(vec![x]),
        }
    }
    assert_eq!(
        runs.len(),
        2,
        "the pair's lower half is not a stem and a bowl: {runs:?}"
    );
    let (mut sum, mut mass) = (0.0, 0.0);
    for &x in &runs[1] {
        for y in rows.clone() {
            let w = weight(px(image, x, y), UI_TEXT_PAGE);
            sum += w * (f64::from(x) + 0.5 - f64::from(min.x));
            mass += w;
        }
    }
    sum / mass
}

/// **The kerned pair is closer than the same glyphs unkerned, by the font's
/// kerning**: the `o` sits nearer its `T` in the tree's span than in the run
/// placed by advances alone, by the pair adjustment the font gives, to within
/// [`KERNING_SLACK`].
fn the_kerned_pair_is_closer_by_the_fonts_kerning(image: &Image, layout: &UiTextLayout) {
    let font = Font::sans();
    let mut chars = UI_TEXT_PAIR.chars();
    let (left, right) = (
        font.glyph_id(chars.next().expect("a pair")),
        font.glyph_id(chars.next().expect("a pair")),
    );
    let expected = -f64::from(font.kerning(left, right) * font.metrics().scale(UI_TEXT_PAIR_SIZE));
    let kerned = second_glyph_centre(image, layout.kerned);
    let unkerned = second_glyph_centre(image, layout.unkerned);
    let closer = unkerned - kerned;
    assert!(
        closer > 0.0,
        "the kerned o is at {kerned} and the unkerned one at {unkerned}: not closer"
    );
    assert!(
        (closer - expected).abs() <= KERNING_SLACK,
        "kerning moved the o {closer}px, and the font kerns the pair by {expected}px"
    );
}

/// **Centred text's ink is symmetric about its block's centre**, to within a
/// pixel.
fn centred_ink_is_symmetric_about_the_block(image: &Image, layout: &UiTextLayout) {
    let (columns, rows) = span(layout.centred);
    let inked: Vec<u32> = columns
        .filter(|&x| {
            rows.clone()
                .any(|y| ink(px(image, x, y), UI_TEXT_CENTRED_FILL))
        })
        .collect();
    let (first, last) = (
        *inked.first().expect("the centred word drew ink"),
        *inked.last().expect("the centred word drew ink"),
    );
    let ink_centre = (f64::from(first) + f64::from(last) + 1.0) * 0.5;
    let (min, max) = layout.centred;
    let block_centre = f64::from(min.x + max.x) * 0.5;
    assert!(
        (ink_centre - block_centre).abs() <= 1.0,
        "the centred word's ink spans {first}..={last}, centred at {ink_centre}, and its block \
         is centred at {block_centre}"
    );
}

fn every_text_promise_is_on_the_frame(image: &Image) {
    let layout = ui_text_layout(EXTENT);
    paragraph_rows_fall_on_the_line_pitch(image, &layout);
    no_paragraph_ink_outside_its_block(image, &layout);
    the_large_word_is_twice_the_small_one(image, &layout);
    the_kerned_pair_is_closer_by_the_fonts_kerning(image, &layout);
    centred_ink_is_symmetric_about_the_block(image, &layout);
}

/// [`Scene::UiText`] drawn, against the reference in `tests/golden/`.
#[test]
#[ignore = "needs a real GPU and a backend pin; run tests/run-render-e2e.sh"]
fn the_ui_text_scene_keeps_every_text_promise_and_matches_its_golden() {
    super::draw_scene_and_match_its_golden(
        Scene::UiText,
        "ui_text",
        EXTENT,
        MIN_COLORS_UI_TEXT,
        every_text_promise_is_on_the_frame,
    );
}
