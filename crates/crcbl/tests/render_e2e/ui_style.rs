//! [`Scene::UiStyle`]: UI rung 4's stylesheets, drawn,
//! held to its golden and to relations read off the frame itself.
//!
//! Every claim below measures pixels against the colours and lengths the
//! scene's stylesheet is written from — never against the resolved style or
//! the layout's own output, which would only prove the cascade agrees with
//! itself. `ui_style_layout` is read to know *where to look*, and nowhere else.
//!
//! A colour claim compares a pixel with a stylesheet's sRGB bytes directly: the
//! sheet's colour is decoded to linear light on the way in and the swapchain
//! encodes it back, so the bytes the sheet wrote are the bytes a correct frame
//! holds, to within [`FLAT`].

use crcbl::screenshot::{
    Scene, UI_STYLE_ACCENT, UI_STYLE_ACCEPT_BORDER, UI_STYLE_BUTTON, UI_STYLE_BUTTON_BORDER,
    UI_STYLE_BUTTON_BORDER_WIDTH, UI_STYLE_BUTTON_HEIGHT, UI_STYLE_GAP, UI_STYLE_HOVER,
    UI_STYLE_MUTED, UI_STYLE_PANEL, UI_STYLE_PANEL_BORDER_WIDTH, UI_STYLE_TEXT, UiStyleLayout,
    ui_style_layout,
};
use crcbl_golden::Image;
use glam::Vec2;

use super::EXTENT;

/// The anti-vacuity colour count: the clear, the panel, its border, two
/// button fills, two button borders and two text colours.
const MIN_COLORS_UI_STYLE: usize = 9;

/// How far a pixel may sit from the colour the stylesheet names, per channel:
/// a round trip through linear light and back, and driver noise.
const FLAT: u8 = 2;

/// How many pixels of a label must be exactly its text colour. A glyph of the
/// built-in font covers whole pixels, and a six-letter word covers far more
/// than this.
const MIN_GLYPH_PIXELS: usize = 20;

fn at(image: &Image, point: Vec2) -> [u8; 4] {
    image
        .pixel(point.x as u32, point.y as u32)
        .unwrap_or_else(|| panic!("{point:?} is outside the frame"))
}

/// Whether a pixel is the sheet's colour, to within [`FLAT`].
fn is(pixel: [u8; 4], colour: [u8; 3]) -> bool {
    pixel[..3]
        .iter()
        .zip(colour)
        .all(|(got, want)| got.abs_diff(want) <= FLAT)
}

/// **The theme's custom properties reach the panel**: its fill is `--panel`
/// and its border `--accent`, both declared on the block above it.
fn the_panel_is_themed_by_its_ancestors_variables(image: &Image, layout: &UiStyleLayout) {
    let (min, max) = layout.panel;
    // Inside the border and the padding, beside the buttons.
    let fill = at(image, Vec2::new(min.x + 6.0, (min.y + max.y) * 0.5));
    assert!(
        is(fill, UI_STYLE_PANEL),
        "the panel's fill is {fill:?}, not the sheet's --panel {UI_STYLE_PANEL:?}"
    );
    let edge = UI_STYLE_PANEL_BORDER_WIDTH * 0.5;
    for (side, point) in [
        ("left", Vec2::new(min.x + edge, (min.y + max.y) * 0.5)),
        ("top", Vec2::new((min.x + max.x) * 0.5, min.y + edge)),
        ("bottom", Vec2::new((min.x + max.x) * 0.5, max.y - edge)),
    ] {
        let pixel = at(image, point);
        assert!(
            is(pixel, UI_STYLE_ACCENT),
            "the panel's {side} border is {pixel:?}, not the sheet's --accent {UI_STYLE_ACCENT:?}"
        );
    }
}

/// A point on a button's fill clear of its label: inside the border, in the
/// left padding.
fn fill_point((min, max): (Vec2, Vec2)) -> Vec2 {
    Vec2::new(
        min.x + UI_STYLE_BUTTON_BORDER_WIDTH + 3.0,
        max.y - UI_STYLE_BUTTON_BORDER_WIDTH - 3.0,
    )
}

/// **The pointer's hover selects `:hover`, on the hovered button only**: the
/// button under the pointer is filled from `.button:hover` and the other from
/// `.button`.
fn only_the_hovered_button_takes_the_hover_rule(image: &Image, layout: &UiStyleLayout) {
    let hovered = at(image, fill_point(layout.accept));
    assert!(
        is(hovered, UI_STYLE_HOVER),
        "the hovered button's fill is {hovered:?}, not `:hover`'s {UI_STYLE_HOVER:?}"
    );
    let other = at(image, fill_point(layout.cancel));
    assert!(
        is(other, UI_STYLE_BUTTON),
        "the unhovered button's fill is {other:?}, not `.button`'s {UI_STYLE_BUTTON:?}"
    );
}

/// **An id rule beats a class rule written after it**: `#accept`'s border is the
/// id rule's colour and `#cancel`'s, which only the class rule reaches, is the
/// class rule's.
fn the_id_rule_beats_the_later_class_rule(image: &Image, layout: &UiStyleLayout) {
    let border = |(min, max): (Vec2, Vec2)| {
        at(
            image,
            Vec2::new(
                min.x + UI_STYLE_BUTTON_BORDER_WIDTH * 0.5,
                (min.y + max.y) * 0.5,
            ),
        )
    };
    let accept = border(layout.accept);
    assert!(
        is(accept, UI_STYLE_ACCEPT_BORDER),
        "#accept's border is {accept:?}, not its id rule's {UI_STYLE_ACCEPT_BORDER:?}"
    );
    let cancel = border(layout.cancel);
    assert!(
        is(cancel, UI_STYLE_BUTTON_BORDER),
        "#cancel's border is {cancel:?}, not the class rule's {UI_STYLE_BUTTON_BORDER:?}"
    );
}

/// How many of the pixels in `rect` are `colour`.
fn count(image: &Image, (min, max): (Vec2, Vec2), colour: [u8; 3]) -> usize {
    (min.y as u32..max.y as u32)
        .flat_map(|y| (min.x as u32..max.x as u32).map(move |x| (x, y)))
        .filter(|&(x, y)| is(image.pixel(x, y).expect("inside"), colour))
        .count()
}

/// **A label with no colour of its own draws in what it inherits, or in its
/// rule's `var()` fallback**: `ACCEPT` has glyph pixels of the theme's text
/// colour and none of the muted one, and `CANCEL` the other way round.
fn labels_draw_in_the_inherited_colour_or_the_fallback(image: &Image, layout: &UiStyleLayout) {
    for (name, rect, want, not) in [
        ("ACCEPT", layout.accept_label, UI_STYLE_TEXT, UI_STYLE_MUTED),
        ("CANCEL", layout.cancel_label, UI_STYLE_MUTED, UI_STYLE_TEXT),
    ] {
        let drawn = count(image, rect, want);
        assert!(
            drawn >= MIN_GLYPH_PIXELS,
            "{name} has {drawn} pixels of {want:?}, fewer than {MIN_GLYPH_PIXELS}: its colour was \
             not the one it inherits or falls back to"
        );
        let wrong = count(image, rect, not);
        assert_eq!(
            wrong, 0,
            "{name} has {wrong} pixels of the other label's {not:?}"
        );
    }
}

/// **The sheet's lengths are the frame's, hovered or not**: a column down the
/// buttons' left padding reads a border-to-border run exactly
/// [`UI_STYLE_BUTTON_HEIGHT`] tall for each button — the hovered one included,
/// whose fill changed colour and nothing else — with a run of the panel exactly
/// [`UI_STYLE_GAP`] between them.
fn the_gap_and_button_heights_are_the_sheets(image: &Image, layout: &UiStyleLayout) {
    let x = fill_point(layout.accept).x as u32;
    let (panel_min, panel_max) = layout.panel;
    let mut runs: Vec<(&str, u32)> = Vec::new();
    for y in panel_min.y as u32..panel_max.y as u32 {
        let pixel = image.pixel(x, y).expect("inside");
        let kind = if is(pixel, UI_STYLE_PANEL) {
            "panel"
        } else if is(pixel, UI_STYLE_ACCENT) {
            "panel border"
        } else {
            "button"
        };
        match runs.last_mut() {
            Some((last, length)) if *last == kind => *length += 1,
            _ => runs.push((kind, 1)),
        }
    }
    let kinds: Vec<&str> = runs.iter().map(|(kind, _)| *kind).collect();
    assert_eq!(
        kinds,
        [
            "panel border",
            "panel",
            "button",
            "panel",
            "button",
            "panel",
            "panel border"
        ],
        "the column at x={x} reads {runs:?}"
    );
    let height = UI_STYLE_BUTTON_HEIGHT as u32;
    assert_eq!(
        runs[2].1, height,
        "the hovered button is not the sheet's height: {runs:?}"
    );
    assert_eq!(
        runs[4].1, height,
        "the other button is not the sheet's height: {runs:?}"
    );
    assert_eq!(
        runs[3].1, UI_STYLE_GAP as u32,
        "the gap is not the sheet's: {runs:?}"
    );
}

fn every_style_promise_is_on_the_frame(image: &Image) {
    let layout = ui_style_layout(EXTENT);
    the_panel_is_themed_by_its_ancestors_variables(image, &layout);
    only_the_hovered_button_takes_the_hover_rule(image, &layout);
    the_id_rule_beats_the_later_class_rule(image, &layout);
    labels_draw_in_the_inherited_colour_or_the_fallback(image, &layout);
    the_gap_and_button_heights_are_the_sheets(image, &layout);
}

/// [`Scene::UiStyle`] drawn, against the reference in `tests/golden/`.
#[test]
#[ignore = "needs a real GPU and a backend pin; run tests/run-render-e2e.sh"]
fn the_ui_style_scene_keeps_every_cascade_promise_and_matches_its_golden() {
    super::draw_scene_and_match_its_golden(
        Scene::UiStyle,
        "ui_style",
        EXTENT,
        MIN_COLORS_UI_STYLE,
        every_style_promise_is_on_the_frame,
    );
}
