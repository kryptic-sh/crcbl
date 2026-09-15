//! [`Scene::UiTextInput`]: `docs/plan/07-ui-debug.md` rung 7's single-line
//! text input, drawn, held to its golden and to relations read off the frame
//! itself.
//!
//! Every claim measures pixels against the colours and lengths the scene and
//! `default.css` are written from, and the bitmap font's fixed advance —
//! never against the input's caret, selection or scroll offset, which would
//! only prove the tree agrees with itself. `ui_text_input_layout` is read to
//! know *where to look*, and nowhere else.

use std::collections::BTreeSet;

use crcbl::screenshot::{
    Scene, UI_TEXT_INPUT_CARET, UI_TEXT_INPUT_CARET_WIDTH, UI_TEXT_INPUT_INSET_X,
    UI_TEXT_INPUT_INSET_Y, UI_TEXT_INPUT_LONG, UI_TEXT_INPUT_PICK, UI_TEXT_INPUT_PICKED,
    UI_TEXT_INPUT_PLACEHOLDER, UI_TEXT_INPUT_SECRET, UI_TEXT_INPUT_SELECTION, UI_TEXT_INPUT_TEXT,
    UiTextInputLayout, ui_text_input_layout,
};
use crcbl::ui::{GLYPH_ADVANCE, LINE_HEIGHT};
use crcbl_golden::Image;
use glam::Vec2;

use super::EXTENT;

/// The anti-vacuity colour count: the page, an input's well, its border, the
/// text, the selection, the caret and the placeholder.
const MIN_COLORS_UI_TEXT_INPUT: usize = 7;

/// How far a pixel may sit from the colour it is compared with, per channel.
const FLAT: u8 = 2;

fn is(pixel: [u8; 4], colour: [u8; 3]) -> bool {
    pixel[..3]
        .iter()
        .zip(colour)
        .all(|(got, want)| got.abs_diff(want) <= FLAT)
}

/// An input's text box: its border box less `default.css`'s insets, one line
/// tall — the pixels its text, selection and caret are drawn in.
fn text_box((min, max): (Vec2, Vec2)) -> (u32, u32, u32, u32) {
    let left = (min.x + UI_TEXT_INPUT_INSET_X) as u32;
    let right = (max.x - UI_TEXT_INPUT_INSET_X) as u32;
    let top = (min.y + UI_TEXT_INPUT_INSET_Y) as u32;
    (left, right, top, top + LINE_HEIGHT as u32)
}

/// Every pixel of `colour` in `rect`'s text box.
fn pixels_in(image: &Image, rect: (Vec2, Vec2), colour: [u8; 3]) -> Vec<(u32, u32)> {
    let (left, right, top, bottom) = text_box(rect);
    (top..bottom)
        .flat_map(|y| (left..right).map(move |x| (x, y)))
        .filter(|&(x, y)| is(image.pixel(x, y).expect("inside the frame"), colour))
        .collect()
}

/// **The selection highlight spans exactly the selected glyphs**: in
/// `#picked`'s text box, the selection colour runs from the left edge of the
/// first selected glyph's cell to the right edge of the last one's, one fixed
/// advance per character, with none of it outside — and the selected glyphs'
/// ink is drawn over it rather than hidden under it.
fn the_selection_spans_exactly_the_selected_glyphs(image: &Image, layout: &UiTextInputLayout) {
    let (left, ..) = text_box(layout.picked);
    let (start, end) = UI_TEXT_INPUT_PICK;
    assert!(
        end > start && end < UI_TEXT_INPUT_PICKED.chars().count(),
        "the pick leaves no glyph unselected on its right, so the claim cannot see an overrun"
    );
    let cell = |index: usize| left + (index as f32 * GLYPH_ADVANCE) as u32;
    let selection = pixels_in(image, layout.picked, UI_TEXT_INPUT_SELECTION);
    assert!(!selection.is_empty(), "no selection is drawn");
    let first = selection.iter().map(|&(x, _)| x).min().expect("some");
    let last = selection.iter().map(|&(x, _)| x).max().expect("some");
    assert_eq!(
        (first, last + 1),
        (cell(start), cell(end)),
        "the selection spans {first}..{} px, not the cells of glyphs {start}..{end}",
        last + 1
    );
    let ink_under = pixels_in(image, layout.picked, UI_TEXT_INPUT_TEXT)
        .iter()
        .filter(|&&(x, _)| x >= cell(start) && x < cell(end))
        .count();
    assert!(
        ink_under > 0,
        "the selected glyphs are hidden under the selection"
    );
}

/// **The caret sits after the last typed glyph, and the long line scrolled to
/// it shows its end**: in `#long`'s text box the caret colour is exactly one
/// column one line tall, against the box's right edge — where an unscrolled
/// line's caret, a full line past the box, could not be — with the last
/// glyph's ink in the cell just left of it and no ink right of it.
fn the_caret_follows_the_last_glyph_at_the_end_of_the_line(
    image: &Image,
    layout: &UiTextInputLayout,
) {
    let (left, right, top, bottom) = text_box(layout.long);
    let line = UI_TEXT_INPUT_LONG.chars().count() as f32 * GLYPH_ADVANCE;
    assert!(
        line > (right - left) as f32,
        "the line fits the input, so nothing had to scroll"
    );
    let caret = pixels_in(image, layout.long, UI_TEXT_INPUT_CARET);
    let columns: BTreeSet<u32> = caret.iter().map(|&(x, _)| x).collect();
    let want = right - UI_TEXT_INPUT_CARET_WIDTH as u32;
    assert_eq!(
        columns.into_iter().collect::<Vec<_>>(),
        [want],
        "the caret is not one column at the right edge"
    );
    assert_eq!(
        caret.len() as u32,
        bottom - top,
        "the caret is not one line tall"
    );
    let ink = pixels_in(image, layout.long, UI_TEXT_INPUT_TEXT);
    let last_cell = (want - GLYPH_ADVANCE as u32)..want;
    assert!(
        ink.iter().any(|&(x, _)| last_cell.contains(&x)),
        "no glyph just left of the caret"
    );
    assert!(
        ink.iter().all(|&(x, _)| x < want),
        "ink right of the caret: the line does not end there"
    );
}

/// **An empty input shows its placeholder, and a masked one a mask per
/// character**: `#empty` has placeholder-coloured ink and no text-coloured
/// ink; `#secret`'s first cells, one per character of its value, each draw the
/// same non-empty glyph, and the cell after them draws nothing.
fn the_placeholder_and_the_mask_show(image: &Image, layout: &UiTextInputLayout) {
    assert!(
        !pixels_in(image, layout.empty, UI_TEXT_INPUT_PLACEHOLDER).is_empty(),
        "the placeholder is not drawn"
    );
    assert!(
        pixels_in(image, layout.empty, UI_TEXT_INPUT_TEXT).is_empty(),
        "an empty input drew text-coloured ink"
    );

    let (left, _, top, _) = text_box(layout.secret);
    let ink = pixels_in(image, layout.secret, UI_TEXT_INPUT_TEXT);
    let cell_of = |index: usize| -> Vec<(u32, u32)> {
        let from = left + (index as f32 * GLYPH_ADVANCE) as u32;
        let mut mask: Vec<(u32, u32)> = ink
            .iter()
            .filter(|&&(x, _)| x >= from && x < from + GLYPH_ADVANCE as u32)
            .map(|&(x, y)| (x - from, y - top))
            .collect();
        mask.sort_unstable();
        mask
    };
    let count = UI_TEXT_INPUT_SECRET.chars().count();
    let first = cell_of(0);
    assert!(!first.is_empty(), "the first mask draws nothing");
    for index in 1..count {
        assert_eq!(cell_of(index), first, "cell {index} is not the same mask");
    }
    assert!(cell_of(count).is_empty(), "a mask past the value's length");
}

fn every_text_input_promise_is_on_the_frame(image: &Image) {
    let layout = ui_text_input_layout(EXTENT);
    the_selection_spans_exactly_the_selected_glyphs(image, &layout);
    the_caret_follows_the_last_glyph_at_the_end_of_the_line(image, &layout);
    the_placeholder_and_the_mask_show(image, &layout);
}

/// [`Scene::UiTextInput`] drawn, against the reference in `tests/golden/`.
#[test]
#[ignore = "needs a real GPU and a backend pin; run tests/run-render-e2e.sh"]
fn the_ui_text_input_scene_keeps_every_input_promise_and_matches_its_golden() {
    super::draw_scene_and_match_its_golden(
        Scene::UiTextInput,
        "ui_text_input",
        EXTENT,
        MIN_COLORS_UI_TEXT_INPUT,
        every_text_input_promise_is_on_the_frame,
    );
}
