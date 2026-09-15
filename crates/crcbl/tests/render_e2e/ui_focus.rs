//! [`Scene::UiFocus`]: `docs/plan/07-ui-debug.md` rung 6's focus, drawn, held
//! to its golden and to relations read off the frame itself.
//!
//! Every claim measures pixels against the colours and lengths the scene's
//! stylesheet is written from — never against the focus state or the layout's
//! own output, which would only prove the tree agrees with itself.
//! `ui_focus_layout` is read to know *where to look*, and nowhere else.
//!
//! A colour claim compares a pixel with a stylesheet's sRGB bytes directly, to
//! within [`FLAT`]: the sheet's colour is decoded to linear light on the way in
//! and the swapchain encodes it back.

use crcbl::screenshot::{
    Scene, UI_FOCUS_LIST_PADDING, UI_FOCUS_RING, UI_FOCUS_RING_OFFSET, UI_FOCUS_RING_WIDTH,
    UI_FOCUS_ROW_HEIGHT, UI_FOCUS_TARGET, UiFocusLayout, ui_focus_layout,
};
use crcbl_golden::Image;
use glam::Vec2;

use super::EXTENT;

/// The anti-vacuity colour count: the page, the grid's buttons, the dialog,
/// the list, a row, the target row and the ring.
const MIN_COLORS_UI_FOCUS: usize = 7;

/// How far a pixel may sit from the colour the stylesheet names, per channel.
const FLAT: u8 = 2;

/// Whether a pixel is the sheet's colour, to within [`FLAT`].
fn is(pixel: [u8; 4], colour: [u8; 3]) -> bool {
    pixel[..3]
        .iter()
        .zip(colour)
        .all(|(got, want)| got.abs_diff(want) <= FLAT)
}

/// Every pixel of the frame that is `colour`.
fn pixels_of(image: &Image, colour: [u8; 3]) -> Vec<(u32, u32)> {
    (0..EXTENT.1)
        .flat_map(|y| (0..EXTENT.0).map(move |x| (x, y)))
        .filter(|&(x, y)| is(image.pixel(x, y).expect("inside"), colour))
        .collect()
}

/// Whether the pixel at `(x, y)` is inside `(min, max)`, half-open.
fn within((x, y): (u32, u32), (min, max): (Vec2, Vec2)) -> bool {
    let point = Vec2::new(x as f32, y as f32);
    point.cmpge(min).all() && point.cmplt(max).all()
}

/// `rect` grown by `by` on every side.
fn grown((min, max): (Vec2, Vec2), by: f32) -> (Vec2, Vec2) {
    (min - Vec2::splat(by), max + Vec2::splat(by))
}

/// The ring the stylesheet draws round `rect`: the band between its outline's
/// outer and inner edges.
fn ring_band(rect: (Vec2, Vec2)) -> ((Vec2, Vec2), (Vec2, Vec2)) {
    let outer = grown(rect, UI_FOCUS_RING_OFFSET + UI_FOCUS_RING_WIDTH);
    let inner = grown(rect, UI_FOCUS_RING_OFFSET);
    (outer, inner)
}

fn band_area((outer, inner): ((Vec2, Vec2), (Vec2, Vec2))) -> usize {
    let area = |(min, max): (Vec2, Vec2)| ((max.x - min.x) * (max.y - min.y)) as usize;
    area(outer) - area(inner)
}

/// **The focused row's ring is drawn, whole, and no other ring is**: every
/// pixel of the ring colour on the frame lies in the band the stylesheet's
/// outline puts round the target row, and that band is entirely the ring
/// colour.
fn only_the_focused_node_is_ringed(image: &Image, layout: &UiFocusLayout) {
    let band = ring_band(layout.target);
    let ring = pixels_of(image, UI_FOCUS_RING);
    let stray: Vec<_> = ring
        .iter()
        .copied()
        .filter(|&pixel| !within(pixel, band.0) || within(pixel, band.1))
        .collect();
    assert!(
        stray.is_empty(),
        "{} ring pixel(s) outside the focused row's ring, first at {:?}: another node shows focus",
        stray.len(),
        stray.first()
    );
    let want = band_area(band);
    assert_eq!(
        ring.len(),
        want,
        "the focused row's ring has {} of its {want} pixels",
        ring.len()
    );
}

/// **The modal holds focus**: after the script's last step, toward the grid
/// behind the dialog, the ring lies wholly inside the dialog, and the grid
/// button that opened it — focused before — has no ring pixel round it.
fn the_modal_holds_focus(image: &Image, layout: &UiFocusLayout) {
    let ring = pixels_of(image, UI_FOCUS_RING);
    assert!(!ring.is_empty(), "no ring is drawn at all");
    let outside: Vec<_> = ring
        .iter()
        .copied()
        .filter(|&pixel| !within(pixel, layout.dialog))
        .collect();
    assert!(
        outside.is_empty(),
        "{} ring pixel(s) outside the dialog, first at {:?}: focus left the modal",
        outside.len(),
        outside.first()
    );
    let opener = grown(layout.opener, UI_FOCUS_RING_OFFSET + UI_FOCUS_RING_WIDTH);
    let round_opener = ring.iter().filter(|&&pixel| within(pixel, opener)).count();
    assert_eq!(round_opener, 0, "the opener still shows a ring");
}

/// **The scrolled list shows the focused row**: the target row's fill — a
/// colour nothing else has, on a row that starts below the list's view — is on
/// the frame in full, inside the list's padding box, with its ring's four sides
/// all drawn rather than clipped away.
fn the_scrolled_list_shows_the_focused_row(image: &Image, layout: &UiFocusLayout) {
    let (min, max) = layout.target;
    let filled = pixels_of(image, UI_FOCUS_TARGET);
    let area = ((max.x - min.x) * UI_FOCUS_ROW_HEIGHT) as usize;
    assert_eq!(
        filled.len(),
        area,
        "the target row shows {} of its {area} pixels: the list did not scroll it into view",
        filled.len()
    );
    let view = grown(layout.list, -UI_FOCUS_LIST_PADDING);
    let (view_min, view_max) = (view.0, view.1);
    assert!(
        filled.iter().all(|&pixel| within(pixel, view)),
        "the target row's fill spills outside the list's view {view_min}..{view_max}"
    );
    let (outer, _) = ring_band(layout.target);
    let ring = UI_FOCUS_RING;
    let at = |x: f32, y: f32| is(image.pixel(x as u32, y as u32).expect("inside"), ring);
    let centre = (outer.0 + outer.1) * 0.5;
    for (side, drawn) in [
        ("top", at(centre.x, outer.0.y)),
        ("bottom", at(centre.x, outer.1.y - 1.0)),
        ("left", at(outer.0.x, centre.y)),
        ("right", at(outer.1.x - 1.0, centre.y)),
    ] {
        assert!(drawn, "the ring's {side} side was clipped away");
    }
}

fn every_focus_promise_is_on_the_frame(image: &Image) {
    let layout = ui_focus_layout(EXTENT);
    only_the_focused_node_is_ringed(image, &layout);
    the_modal_holds_focus(image, &layout);
    the_scrolled_list_shows_the_focused_row(image, &layout);
}

/// [`Scene::UiFocus`] drawn, against the reference in `tests/golden/`.
#[test]
#[ignore = "needs a real GPU and a backend pin; run tests/run-render-e2e.sh"]
fn the_ui_focus_scene_keeps_every_focus_promise_and_matches_its_golden() {
    super::draw_scene_and_match_its_golden(
        Scene::UiFocus,
        "ui_focus",
        EXTENT,
        MIN_COLORS_UI_FOCUS,
        every_focus_promise_is_on_the_frame,
    );
}
