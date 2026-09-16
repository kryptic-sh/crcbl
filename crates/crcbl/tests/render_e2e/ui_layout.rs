//! [`Scene::UiLayout`]: `docs/plan/07-ui-debug.md` rung 8a's outliner, tabs and
//! dockable splitter layout, drawn, held to its golden and to relations read
//! off the frame itself.
//!
//! Every claim measures pixels against the colours and lengths the scene and
//! `default.css` are written from — never against a widget's own state or the
//! layout's output, which would only prove the tree agrees with itself.
//! `ui_layout_layout` is read to know *where to look*, and nowhere else.

use crcbl::screenshot::{
    Scene, UI_LAYOUT_ACCENT, UI_LAYOUT_DIVIDER_DRAG, UI_LAYOUT_LOG_FILL, UI_LAYOUT_PADDING,
    UI_LAYOUT_ROW_HEIGHT, UI_LAYOUT_SELECTED, UI_LAYOUT_SELECTED_ROW, UI_LAYOUT_SHOWN_TAB,
    UI_LAYOUT_STRIPE_EVEN, UI_LAYOUT_STRIPE_ODD, UI_LAYOUT_STRIPE_STEP, UI_LAYOUT_TAB_FILLS,
    UiLayoutLayout, ui_layout_indent, ui_layout_layout, ui_layout_rows, ui_layout_stripe_steps,
};
use crcbl_golden::Image;
use glam::Vec2;

use super::EXTENT;

/// The anti-vacuity colour count: the page, the outliner's well, its border
/// (which is the divider's fill too), its text, a closed row's toggle, the
/// accent, the selection fill, a tab's surface, both stripes, the showing
/// tab's pane fill and the dock's bottom pane's.
const MIN_COLORS_UI_LAYOUT: usize = 12;

/// How far a pixel may sit from the colour it is compared with, per channel.
const FLAT: u8 = 2;

/// `default.css`'s outliner border width.
const OUTLINER_BORDER: f32 = 1.0;

/// `default.css`'s divider thickness, on both axes.
const DIVIDER: f32 = 4.0;

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

/// **The outliner shows the rows its offset and its expansion state name, each
/// indented by its depth**: on every pixel row of its view, the stripe is the
/// colour and the width of the model row that the offset and the row height put
/// there, and it starts the model row's depth times the indent from the well's
/// edge — so a frame that opened nothing, scrolled by any other amount, or drew
/// its nesting flat cannot match.
fn the_outliner_shows_the_rows_its_offset_and_expansion_name(
    image: &Image,
    layout: &UiLayoutLayout,
) {
    let rows = ui_layout_rows();
    assert!(
        rows.len() > UI_LAYOUT_SELECTED_ROW,
        "the model is shorter than the row the pad walked to: the root never opened"
    );
    let (min, max) = layout.outliner;
    let (top, bottom) = (min.y + OUTLINER_BORDER, max.y - OUTLINER_BORDER);
    let offset = layout.outliner_offset;
    assert!(
        offset > bottom - top,
        "the outliner scrolled by {offset} px, less than its own view: a window built at any \
         offset would show the same rows"
    );

    let (mut seen, mut depths) = (Vec::new(), Vec::new());
    for y in top as u32..bottom as u32 {
        let index = ((y as f32 - top + offset) / UI_LAYOUT_ROW_HEIGHT).floor() as usize;
        let Some(&(id, depth)) = rows.get(index) else {
            continue;
        };
        let stripe = if id.is_multiple_of(2) {
            UI_LAYOUT_STRIPE_EVEN
        } else {
            UI_LAYOUT_STRIPE_ODD
        };
        let run: Vec<u32> = (min.x as u32..max.x as u32)
            .filter(|&x| is(at(image, x as f32, y as f32), stripe))
            .collect();
        let want = (ui_layout_stripe_steps(id) as f32 * UI_LAYOUT_STRIPE_STEP) as usize;
        assert_eq!(
            run.len(),
            want,
            "pixel row {y} shows {} px of item {id}'s stripe, not {want}",
            run.len()
        );
        // The row is indented by its depth, so the stripe after the toggle
        // starts that much further in than a root row's would.
        let start = *run.first().expect("the stripe is drawn") as f32;
        depths.push((index, depth, start));
        if seen.last() != Some(&index) {
            seen.push(index);
        }
    }
    assert!(seen.len() > 3, "the view showed rows {seen:?}");
    assert!(
        seen.contains(&UI_LAYOUT_SELECTED_ROW),
        "the row the pad walked to is not in view: {seen:?}"
    );

    let shallow = depths
        .iter()
        .map(|&(_, depth, _)| depth)
        .min()
        .expect("the view shows rows");
    let base = depths
        .iter()
        .find(|&&(_, depth, _)| depth == shallow)
        .map(|&(_, _, start)| start)
        .expect("a row at the shallowest depth");
    assert!(
        depths.iter().any(|&(_, depth, _)| depth > shallow),
        "every row in view is at depth {shallow}, so indentation proves nothing"
    );
    for &(index, depth, start) in &depths {
        let want = base + ui_layout_indent(depth) - ui_layout_indent(shallow);
        assert_eq!(
            start, want,
            "row {index} at depth {depth} starts its stripe at {start}, not {want}"
        );
    }
}

/// **The selected row is the only one marked**: every pixel of the selection
/// fill lies inside the selected row's box, and its middle line is that fill
/// from edge to edge but for the stripe it holds.
fn the_selected_row_is_the_only_one_marked(image: &Image, layout: &UiLayoutLayout) {
    let marked = pixels_of(image, UI_LAYOUT_SELECTED);
    assert!(!marked.is_empty(), "no row is drawn in the selection fill");
    let stray = marked
        .iter()
        .filter(|&&pixel| !within(pixel, layout.selected))
        .count();
    assert_eq!(
        stray, 0,
        "selection fill outside the selected row: more than one row is marked"
    );

    let (min, max) = layout.selected;
    let y = ((min.y + max.y) * 0.5).floor();
    let run = (min.x as u32..max.x as u32)
        .filter(|&x| is(at(image, x as f32, y), UI_LAYOUT_SELECTED))
        .count();
    let width = (max.x - min.x) as usize;
    assert!(
        run > width / 2,
        "the selected row's middle line is {run} px of {width} in the selection fill"
    );
}

/// **The showing tab's pane is the only pane built**: its fill is on the frame
/// and the other two tabs' fills are nowhere at all — not merely hidden — and
/// exactly one tab is drawn in the accent, inside the strip above that pane.
fn only_the_showing_tabs_pane_is_built(image: &Image, layout: &UiLayoutLayout) {
    for (index, fill) in UI_LAYOUT_TAB_FILLS.iter().enumerate() {
        let drawn = pixels_of(image, *fill);
        if index == UI_LAYOUT_SHOWN_TAB {
            assert!(!drawn.is_empty(), "the showing tab's pane is not drawn");
            let stray = drawn
                .iter()
                .filter(|&&pixel| !within(pixel, layout.tab_pane))
                .count();
            assert_eq!(stray, 0, "the showing pane's fill spills out of its pane");
        } else {
            assert!(
                drawn.is_empty(),
                "tab {index}'s pane is on the frame although it is not showing"
            );
        }
    }

    // The accent is also an open outliner row's toggle glyph, so this looks
    // only inside the pane the strip sits in.
    let accent = pixels_of(image, UI_LAYOUT_ACCENT);
    let showing = layout.tabs[UI_LAYOUT_SHOWN_TAB];
    let stray = accent
        .iter()
        .filter(|&&pixel| within(pixel, layout.views_pane) && !within(pixel, showing))
        .count();
    assert_eq!(
        stray, 0,
        "accent inside the tabs' pane but outside the showing tab: another tab is marked"
    );
    let covered = accent
        .iter()
        .filter(|&&pixel| within(pixel, showing))
        .count();
    let area = ((showing.1.x - showing.0.x) * (showing.1.y - showing.0.y)) as usize;
    assert!(
        covered > area / 2,
        "the showing tab is {covered} px of {area} in the accent, so it is not the marked one"
    );
    assert!(
        showing.1.y <= layout.tab_pane.0.y,
        "the showing tab is not above its pane"
    );
}

/// **The drag moved both of the dock's panes**: the outliner pane's width is
/// the even share plus what was dragged, the tab pane starts exactly one
/// divider past its right edge, the divider's own pixels are page-free, and the
/// pane under the tabs starts one divider below them.
fn the_dragged_divider_moved_both_panes(image: &Image, layout: &UiLayoutLayout) {
    let room = EXTENT.0 as f32 - 2.0 * UI_LAYOUT_PADDING;
    let even = (room - DIVIDER) / 2.0;
    let outline = layout.outline_pane.1.x - layout.outline_pane.0.x;
    assert_eq!(
        outline,
        even + UI_LAYOUT_DIVIDER_DRAG,
        "the outliner pane is not the even share plus the drag"
    );
    assert_eq!(
        layout.views_pane.1.x - layout.views_pane.0.x,
        even - UI_LAYOUT_DIVIDER_DRAG,
        "the other pane did not take what the drag gave it"
    );
    assert_eq!(
        layout.views_pane.0.x - layout.outline_pane.1.x,
        DIVIDER,
        "the two panes do not meet one divider apart"
    );
    assert_eq!(
        layout.log_pane.0.y - layout.views_pane.1.y,
        DIVIDER,
        "the nested panes do not meet one divider apart"
    );

    // Nothing of the page shows through the gap the divider fills.
    let y = ((layout.outline_pane.0.y + layout.outline_pane.1.y) * 0.5).floor();
    for x in layout.outline_pane.1.x as u32..layout.views_pane.0.x as u32 {
        let pixel = at(image, x as f32, y);
        assert!(
            !is(pixel, crcbl::screenshot::UI_LAYOUT_PAGE),
            "column {x} of the divider shows the page through it"
        );
    }
    assert!(
        !pixels_of(image, UI_LAYOUT_LOG_FILL).is_empty(),
        "the pane under the tabs is not drawn"
    );
}

fn every_surface_promise_is_on_the_frame(image: &Image) {
    let layout = ui_layout_layout(EXTENT);
    the_outliner_shows_the_rows_its_offset_and_expansion_name(image, &layout);
    the_selected_row_is_the_only_one_marked(image, &layout);
    only_the_showing_tabs_pane_is_built(image, &layout);
    the_dragged_divider_moved_both_panes(image, &layout);
}

/// [`Scene::UiLayout`] drawn, against the reference in `tests/golden/`.
#[test]
#[ignore = "needs a real GPU and a backend pin; run tests/run-render-e2e.sh"]
fn the_ui_layout_scene_keeps_every_surface_promise_and_matches_its_golden() {
    super::draw_scene_and_match_its_golden(
        Scene::UiLayout,
        "ui_layout",
        EXTENT,
        MIN_COLORS_UI_LAYOUT,
        every_surface_promise_is_on_the_frame,
    );
}
