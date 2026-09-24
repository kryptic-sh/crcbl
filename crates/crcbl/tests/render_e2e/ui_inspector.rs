//! [`Scene::UiInspector`]: UI rung 8's
//! reflection-driven property inspector, drawn, held to its golden and to
//! relations read off the frame itself.
//!
//! Every claim measures pixels against the colours and lengths the scene and
//! `default.css` are written from — never against the widget's own state or the
//! layout's output, which would only prove the tree agrees with itself.
//! `ui_inspector_layout` is read to know *where to look*, and nowhere else.
//!
//! One promise of rung 8 is not a pixel claim and cannot be: that the ranged
//! field **shows** its clamped value, which is a string of glyphs. The gauge
//! below the panel is this scene's readable stand-in — its fill is what the
//! panel left in the component, as a share of twice the field's maximum — and
//! the text itself is asserted against the draw list by
//! `crcbl::screenshot`'s own
//! `the_ranged_field_stopped_at_its_maximum_and_is_shown_at_its_steps_decimals`.

use crcbl::screenshot::{
    Scene, UI_INSPECTOR_ENGAGED, UI_INSPECTOR_GAUGE_FILL, UI_INSPECTOR_GAUGE_TRACK,
    UI_INSPECTOR_GAUGE_WIDTH, UI_INSPECTOR_HEIGHT_MAX, UI_INSPECTOR_NESTED,
    UI_INSPECTOR_NESTED_ROWS, UI_INSPECTOR_ROW, UI_INSPECTOR_SHUT_ROWS, UI_INSPECTOR_TOP_ROWS,
    UiInspectorLayout, ui_inspector_gauge, ui_inspector_layout,
};
use crcbl_golden::Image;
use glam::Vec2;

use super::EXTENT;

/// The anti-vacuity colour count: the page, the panel's well, its border, its
/// text, a header's surface, the open header's toggle in the accent, a
/// drag-value's field, the engaged one's fill and border, a text input's
/// surface, a checkbox's box, the row fill, the nested fill, the gauge's track
/// and its fill.
const MIN_COLORS_UI_INSPECTOR: usize = 12;

/// How far a pixel may sit from the colour it is compared with, per channel.
const FLAT: u8 = 2;

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

/// How many separate horizontal bands of `colour` the frame carries: runs of
/// pixel rows that hold any of it, with a gap between them.
fn bands(image: &Image, colour: [u8; 3]) -> usize {
    let mut bands = 0;
    let mut inside = false;
    for y in 0..EXTENT.1 {
        let any = (0..EXTENT.0).any(|x| is(at(image, x as f32, y as f32), colour));
        if any && !inside {
            bands += 1;
        }
        inside = any;
    }
    bands
}

/// **One band per row, at the depth the component puts it**: the top-level row
/// fill is on the frame once per field that is a leaf or an override's row and
/// nowhere else, and the nested fill once per row of the **open** group — so a
/// panel that built the shut group's body too would carry
/// [`UI_INSPECTOR_SHUT_ROWS`] more bands of it.
fn every_row_is_drawn_at_its_own_depth(image: &Image, layout: &UiInspectorLayout) {
    let top = pixels_of(image, UI_INSPECTOR_ROW);
    assert!(!top.is_empty(), "no row is drawn in the row fill");
    let stray = top
        .iter()
        .filter(|&&pixel| !layout.rows.iter().any(|&row| within(pixel, row)))
        .count();
    assert_eq!(stray, 0, "row fill outside the rows the panel laid out");
    assert_eq!(
        bands(image, UI_INSPECTOR_ROW),
        UI_INSPECTOR_TOP_ROWS,
        "the panel is not one band per top-level field"
    );

    let nested = pixels_of(image, UI_INSPECTOR_NESTED);
    assert!(
        !nested.is_empty(),
        "the open group's rows are not on the frame"
    );
    let stray = nested
        .iter()
        .filter(|&&pixel| !layout.nested.iter().any(|&row| within(pixel, row)))
        .count();
    assert_eq!(stray, 0, "nested fill outside the open group's own rows");
    let drawn = bands(image, UI_INSPECTOR_NESTED);
    assert_eq!(
        drawn, UI_INSPECTOR_NESTED_ROWS,
        "{drawn} bands of nested fill, not {UI_INSPECTOR_NESTED_ROWS}: the shut group built its \
         {UI_INSPECTOR_SHUT_ROWS} rows too"
    );

    // Depth is on the picture: the nested band starts further in than every
    // top-level one, and it sits between the two headers.
    let left = |pixels: &[(u32, u32)]| pixels.iter().map(|&(x, _)| x).min().expect("some pixels");
    assert!(
        left(&nested) > left(&top),
        "the nested row is not indented past the top-level ones"
    );
    let above = nested.iter().map(|&(_, y)| y).min().expect("some pixels") as f32;
    let below = nested.iter().map(|&(_, y)| y).max().expect("some pixels") as f32;
    assert!(
        above >= layout.group.1.y && below < layout.shut.0.y,
        "the open group's body is not between its own header and the shut one"
    );
}

/// **Exactly one widget on the frame is engaged, and it is the one the script
/// clicked**: `default.css` fills an engaged drag-value and nothing else with
/// [`UI_INSPECTOR_ENGAGED`], and every pixel of it lies in the ranged field's
/// own editor.
fn only_the_clicked_drag_value_is_engaged(image: &Image, layout: &UiInspectorLayout) {
    let engaged = pixels_of(image, UI_INSPECTOR_ENGAGED);
    assert!(!engaged.is_empty(), "no widget is drawn engaged");
    let stray = engaged
        .iter()
        .filter(|&&pixel| !within(pixel, layout.height))
        .count();
    assert_eq!(
        stray, 0,
        "engaged fill outside the drag-value the script clicked"
    );
    let (min, max) = layout.height;
    let area = ((max.x - min.x) * (max.y - min.y)) as usize;
    assert!(
        engaged.len() > area / 2,
        "the drag-value is {} px of {area} in the engaged fill, so it is not engaged",
        engaged.len()
    );
}

/// **The ranged field stopped inside its range**: the gauge — the scene's
/// readout of what the panel left in the component — fills the share of its
/// track that the field's **maximum** names, which is half of it. A drag that
/// ignored `Field::range` would have run the fill off the end of the track, and
/// one that ignored `Field::step` would have stopped short of half.
fn the_gauge_shows_the_field_held_at_its_maximum(image: &Image, layout: &UiInspectorLayout) {
    let want = ui_inspector_gauge(UI_INSPECTOR_HEIGHT_MAX);
    assert!(
        want > 1.0 && want < UI_INSPECTOR_GAUGE_WIDTH - 1.0,
        "the gauge's share is at one end of its track, so nothing distinguishes it"
    );

    let (min, max) = layout.gauge;
    let y = ((min.y + max.y) * 0.5).floor();
    let run = (min.x as u32..(min.x + UI_INSPECTOR_GAUGE_WIDTH) as u32)
        .filter(|&x| is(at(image, x as f32, y), UI_INSPECTOR_GAUGE_FILL))
        .count();
    assert!(
        (run as f32 - want).abs() <= 1.0,
        "the gauge fills {run} px, not the {want} the field's maximum names"
    );

    // The rest of the track is track, so the fill did not simply cover it.
    let rest = ((min.x + want) as u32..(min.x + UI_INSPECTOR_GAUGE_WIDTH) as u32)
        .filter(|&x| is(at(image, x as f32, y), UI_INSPECTOR_GAUGE_TRACK))
        .count();
    assert!(
        rest > 0,
        "nothing of the gauge's track is left, so the fill ran past the field's range"
    );
}

fn every_panel_promise_is_on_the_frame(image: &Image) {
    let layout = ui_inspector_layout(EXTENT);
    every_row_is_drawn_at_its_own_depth(image, &layout);
    only_the_clicked_drag_value_is_engaged(image, &layout);
    the_gauge_shows_the_field_held_at_its_maximum(image, &layout);
}

/// [`Scene::UiInspector`] drawn, against the reference in `tests/golden/`.
#[test]
#[ignore = "needs a real GPU and a backend pin; run tests/run-render-e2e.sh"]
fn the_ui_inspector_scene_keeps_every_panel_promise_and_matches_its_golden() {
    super::draw_scene_and_match_its_golden(
        Scene::UiInspector,
        "ui_inspector",
        EXTENT,
        MIN_COLORS_UI_INSPECTOR,
        every_panel_promise_is_on_the_frame,
    );
}
