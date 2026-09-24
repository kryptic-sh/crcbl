//! [`Scene::UiWidgets`]: UI rung 7's widget set, drawn,
//! held to its golden and to relations read off the frame itself.
//!
//! Every claim measures pixels against the colours and lengths the scene and
//! `default.css` are written from — never against a widget's value or the
//! layout's own output, which would only prove the tree agrees with itself.
//! `ui_widgets_layout` is read to know *where to look*, and nowhere else.

use crcbl::screenshot::{
    Scene, UI_WIDGETS_ACCENT, UI_WIDGETS_AFTER, UI_WIDGETS_BODY, UI_WIDGETS_COLUMN_GAP,
    UI_WIDGETS_LIST_TARGET, UI_WIDGETS_PAGE, UI_WIDGETS_RING, UI_WIDGETS_ROW_HEIGHT,
    UI_WIDGETS_SLIDER_START, UI_WIDGETS_STRIPE_EVEN, UI_WIDGETS_STRIPE_ODD, UI_WIDGETS_STRIPE_STEP,
    UI_WIDGETS_VOLUME, UiWidgetsLayout, ui_widgets_layout,
};
use crcbl_golden::Image;
use glam::Vec2;

use super::EXTENT;

/// The anti-vacuity colour count: the page, a widget's surface, border, well
/// and text, the accent, the engaged fill, the ring, the header, the body, the
/// block after the shut header, and both stripes.
const MIN_COLORS_UI_WIDGETS: usize = 13;

/// How far a pixel may sit from the colour it is compared with, per channel.
const FLAT: u8 = 2;

/// `default.css`'s slider inset on each side: a one-pixel border and two of
/// padding, between the border box and the track the fill spans.
const SLIDER_INSET: f32 = 3.0;

/// `default.css`'s list border width.
const LIST_BORDER: f32 = 1.0;

/// `default.css`'s focus ring: its width and how far outside the border box.
const RING_WIDTH: f32 = 2.0;
const RING_OFFSET: f32 = 1.0;

/// A slider's range, as the scene builds both.
const SLIDER_MAX: f32 = 10.0;

fn is(pixel: [u8; 4], colour: [u8; 3]) -> bool {
    pixel[..3]
        .iter()
        .zip(colour)
        .all(|(got, want)| got.abs_diff(want) <= FLAT)
}

fn at(image: &Image, x: f32, y: f32) -> [u8; 4] {
    image.pixel(x as u32, y as u32).expect("inside the frame")
}

/// Every pixel of the frame that is `colour`.
fn pixels_of(image: &Image, colour: [u8; 3]) -> Vec<(u32, u32)> {
    (0..EXTENT.1)
        .flat_map(|y| (0..EXTENT.0).map(move |x| (x, y)))
        .filter(|&(x, y)| is(image.pixel(x, y).expect("inside"), colour))
        .collect()
}

fn within((x, y): (u32, u32), (min, max): (Vec2, Vec2)) -> bool {
    let point = Vec2::new(x as f32, y as f32);
    point.cmpge(min).all() && point.cmplt(max).all()
}

/// **The engaged slider moved and the other did not**: along the middle of
/// each track, the accent run is the share of the track its value names —
/// `#pitch` still its start, `#volume` the value the script stepped it to.
fn the_engaged_slider_moved_and_the_other_did_not(image: &Image, layout: &UiWidgetsLayout) {
    let run = |(min, max): (Vec2, Vec2)| {
        // The track only: an engaged slider's border is the accent too.
        let y = ((min.y + max.y) * 0.5).floor();
        ((min.x + SLIDER_INSET) as u32..(max.x - SLIDER_INSET) as u32)
            .filter(|&x| is(at(image, x as f32, y), UI_WIDGETS_ACCENT))
            .count()
    };
    let expected = |(min, max): (Vec2, Vec2), value: f32| {
        ((max.x - min.x - 2.0 * SLIDER_INSET) * value / SLIDER_MAX).round() as usize
    };
    let (pitch, volume) = (run(layout.pitch), run(layout.volume));
    let (want_pitch, want_volume) = (
        expected(layout.pitch, UI_WIDGETS_SLIDER_START),
        expected(layout.volume, UI_WIDGETS_VOLUME),
    );
    assert!(
        pitch.abs_diff(want_pitch) <= 1,
        "#pitch fills {pitch} px, not {want_pitch}: the slider nothing engaged moved"
    );
    assert!(
        volume.abs_diff(want_volume) <= 1,
        "#volume fills {volume} px, not {want_volume}: the engaged slider did not move"
    );
    assert!(volume > pitch, "#volume does not fill more than #pitch");
}

/// **A closed header's body takes no layout**: every pixel of body fill on
/// the frame lies between the open header and the shut one, and the block
/// after the shut header starts one column gap below it, with only page
/// between.
fn a_closed_headers_body_takes_no_layout(image: &Image, layout: &UiWidgetsLayout) {
    let body = pixels_of(image, UI_WIDGETS_BODY);
    assert!(!body.is_empty(), "the open header's body is not drawn");
    let stray = body
        .iter()
        .filter(|&&(_, y)| (y as f32) < layout.open.1.y || (y as f32) >= layout.shut.0.y)
        .count();
    assert_eq!(stray, 0, "body fill outside the open header's body");

    let x = (layout.shut.0.x + layout.shut.1.x) * 0.5;
    let top = (0..EXTENT.1)
        .find(|&y| is(at(image, x, y as f32), UI_WIDGETS_AFTER))
        .expect("the block after the shut header is drawn");
    let want = layout.shut.1.y + UI_WIDGETS_COLUMN_GAP;
    assert_eq!(
        top as f32, want,
        "the block after the shut header starts at {top}, not one gap below it"
    );
    for y in layout.shut.1.y as u32..top {
        assert!(
            is(at(image, x, y as f32), UI_WIDGETS_PAGE),
            "row {y} between the shut header and the block after it is not page"
        );
    }
}

/// **The list shows the rows its offset names**: on every pixel row of its
/// view, the stripe is the colour and the width of the row index that the
/// offset and the row height put there — a width no other row has, so a view
/// scrolled by any other amount cannot match.
fn the_list_shows_the_rows_its_offset_names(image: &Image, layout: &UiWidgetsLayout) {
    let (min, max) = layout.items;
    let (top, bottom) = (min.y + LIST_BORDER, max.y - LIST_BORDER);
    // What the walk to the target row scrolled by: the least that shows it.
    let offset = (UI_WIDGETS_LIST_TARGET + 1) as f32 * UI_WIDGETS_ROW_HEIGHT - (bottom - top);
    assert!(
        offset > 0.0,
        "the list never scrolled, so the claim proves nothing"
    );
    let mut rows = Vec::new();
    for y in top as u32..bottom as u32 {
        let index = ((y as f32 - top + offset) / UI_WIDGETS_ROW_HEIGHT).floor() as usize;
        let stripe = if index.is_multiple_of(2) {
            UI_WIDGETS_STRIPE_EVEN
        } else {
            UI_WIDGETS_STRIPE_ODD
        };
        let width = (min.x as u32..max.x as u32)
            .filter(|&x| is(at(image, x as f32, y as f32), stripe))
            .count();
        let want = (index as f32 * UI_WIDGETS_STRIPE_STEP) as usize;
        assert_eq!(
            width, want,
            "pixel row {y} has {width} px of row {index}'s stripe, not {want}"
        );
        if rows.last() != Some(&index) {
            rows.push(index);
        }
    }
    assert!(rows.len() > 2, "the view showed rows {rows:?}");
    assert_eq!(
        rows.last(),
        Some(&UI_WIDGETS_LIST_TARGET),
        "the target row is not the last row in view"
    );
}

/// **The checked box shows its mark, and exactly the engaged slider is
/// ringed**: accent inside the checkbox, and every ring pixel on the frame in
/// the band round `#volume`, the band whole.
fn the_box_is_checked_and_only_the_engaged_slider_is_ringed(
    image: &Image,
    layout: &UiWidgetsLayout,
) {
    assert!(
        pixels_of(image, UI_WIDGETS_ACCENT)
            .iter()
            .any(|&pixel| within(pixel, layout.mute)),
        "the checked box shows no mark"
    );
    let grow = |(min, max): (Vec2, Vec2), by: f32| (min - Vec2::splat(by), max + Vec2::splat(by));
    let outer = grow(layout.volume, RING_OFFSET + RING_WIDTH);
    let inner = grow(layout.volume, RING_OFFSET);
    let ring = pixels_of(image, UI_WIDGETS_RING);
    let stray = ring
        .iter()
        .filter(|&&pixel| !within(pixel, outer) || within(pixel, inner))
        .count();
    assert_eq!(
        stray, 0,
        "ring pixels outside #volume's ring: another node shows focus"
    );
    let area = |(min, max): (Vec2, Vec2)| ((max.x - min.x) * (max.y - min.y)) as usize;
    assert_eq!(
        ring.len(),
        area(outer) - area(inner),
        "#volume's ring is not whole"
    );
}

fn every_widget_promise_is_on_the_frame(image: &Image) {
    let layout = ui_widgets_layout(EXTENT);
    the_engaged_slider_moved_and_the_other_did_not(image, &layout);
    a_closed_headers_body_takes_no_layout(image, &layout);
    the_list_shows_the_rows_its_offset_names(image, &layout);
    the_box_is_checked_and_only_the_engaged_slider_is_ringed(image, &layout);
}

/// [`Scene::UiWidgets`] drawn, against the reference in `tests/golden/`.
#[test]
#[ignore = "needs a real GPU and a backend pin; run tests/run-render-e2e.sh"]
fn the_ui_widgets_scene_keeps_every_widget_promise_and_matches_its_golden() {
    super::draw_scene_and_match_its_golden(
        Scene::UiWidgets,
        "ui_widgets",
        EXTENT,
        MIN_COLORS_UI_WIDGETS,
        every_widget_promise_is_on_the_frame,
    );
}
