//! [`Scene::UiTree`]: UI rungs 2 and 3's element tree,
//! laid out by Taffy and drawn, held to its golden and to relations read off
//! the frame itself.
//!
//! Every claim below measures pixels against pixels and against the scene's
//! declared style constants — a gap, an offset — never against the layout's own
//! output, which would only prove the layout agrees with itself.
//! `ui_tree_layout` is read to know *where to look*, and nowhere else.

use crcbl::screenshot::{
    Scene, UI_TREE_CELL, UI_TREE_GAP, UI_TREE_OVERLAY_OFFSET, UiTreeLayout, ui_tree_layout,
};
use crcbl_golden::Image;
use glam::Vec2;

use super::{EXTENT, differ};

/// The anti-vacuity colour count: the clear, the panel, three cells, two rows,
/// the overlay, the border and the overflowing child.
const MIN_COLORS_UI_TREE: usize = 10;

/// How far a flat colour may sit from a sample of the same fill, per channel.
const FLAT: u8 = 2;

fn at(image: &Image, point: Vec2) -> [u8; 4] {
    image
        .pixel(point.x as u32, point.y as u32)
        .unwrap_or_else(|| panic!("{point:?} is outside the frame"))
}

/// Whether two pixels are the same flat colour, to within [`FLAT`].
fn same(pixel: [u8; 4], other: [u8; 4]) -> bool {
    pixel[..3]
        .iter()
        .zip(&other[..3])
        .all(|(left, right)| left.abs_diff(*right) <= FLAT)
}

/// The runs of one row of pixels from `from` to `to` (exclusive), each as the
/// colour it is and how many pixels long.
fn runs(image: &Image, y: u32, from: u32, to: u32) -> Vec<([u8; 4], u32)> {
    let mut runs: Vec<([u8; 4], u32)> = Vec::new();
    for x in from..to {
        let pixel = image.pixel(x, y).expect("inside");
        match runs.last_mut() {
            Some((colour, length)) if same(*colour, pixel) => *length += 1,
            _ => runs.push((pixel, 1)),
        }
    }
    runs
}

/// **Children do not overlap, and the gap between them is exactly the gap.**
///
/// One row through the middle of the gap row reads, left to right, the panel,
/// then three cells each exactly [`UI_TREE_CELL`] units wide with a run of the
/// panel exactly [`UI_TREE_GAP`] units wide between each pair — and no pixel of
/// any other colour, which a cell painted over its neighbour would leave.
fn the_cells_are_whole_and_a_gap_apart(image: &Image, layout: &UiTreeLayout) {
    let (row_min, row_max) = layout.gap_row;
    let y = ((row_min.y + row_max.y) * 0.5) as u32;
    let panel = at(image, row_min + Vec2::splat(4.0 * layout.unit));
    let cells = layout.cells.map(|(min, max)| at(image, (min + max) * 0.5));
    for (index, cell) in cells.iter().enumerate() {
        assert!(differ(*cell, panel), "cell {index} is the panel's colour");
    }

    let got = runs(image, y, row_min.x as u32, row_max.x as u32);
    let kinds: Vec<&str> = got
        .iter()
        .map(|(colour, _)| {
            if same(*colour, panel) {
                "panel"
            } else if let Some(index) = cells.iter().position(|cell| same(*colour, *cell)) {
                ["cell 0", "cell 1", "cell 2"][index]
            } else {
                "other"
            }
        })
        .collect();
    assert_eq!(
        kinds,
        [
            "panel", "cell 0", "panel", "cell 1", "panel", "cell 2", "panel"
        ],
        "the gap row at y={y} reads {got:?}"
    );

    let cell = (UI_TREE_CELL * layout.unit) as u32;
    let gap = (UI_TREE_GAP * layout.unit) as u32;
    for index in [1, 3, 5] {
        assert_eq!(
            got[index].1, cell,
            "{} is {} px wide, not {cell}: a neighbour painted over it or it moved — {got:?}",
            kinds[index], got[index].1
        );
    }
    for index in [2, 4] {
        assert_eq!(
            got[index].1,
            gap,
            "the gap after {} is {} px, not the {gap} px the style asks for — {got:?}",
            kinds[index - 1],
            got[index].1
        );
    }
}

/// **The absolute child sits at its offset**: its first pixel is exactly
/// [`UI_TREE_OVERLAY_OFFSET`] units right of and below the column's own first
/// pixel, each edge found by walking in from the clear.
fn the_overlay_sits_at_its_offset(image: &Image, layout: &UiTreeLayout) {
    let clear = image.pixel(1, 1).expect("inside");
    let (overlay_min, overlay_max) = layout.overlay;
    let middle = (overlay_min + overlay_max) * 0.5;
    let overlay = at(image, middle);
    assert!(differ(overlay, clear), "the overlay drew nothing");

    let (column_min, _) = layout.column;
    let first = |along_x: bool, matches: &dyn Fn([u8; 4]) -> bool| -> u32 {
        let start = if along_x {
            column_min.x as u32 - 4
        } else {
            column_min.y as u32 - 4
        };
        (start..start + 400)
            .find(|&step| {
                let pixel = if along_x {
                    image.pixel(step, middle.y as u32)
                } else {
                    image.pixel(middle.x as u32, step)
                };
                matches(pixel.expect("inside"))
            })
            .expect("the edge is inside the frame")
    };
    for (axis, along_x, offset) in [
        ("left", true, UI_TREE_OVERLAY_OFFSET.x),
        ("top", false, UI_TREE_OVERLAY_OFFSET.y),
    ] {
        let edge = first(along_x, &|pixel| !same(pixel, clear));
        let child = first(along_x, &|pixel| same(pixel, overlay));
        let want = (offset * layout.unit) as u32;
        assert_eq!(
            child - edge,
            want,
            "the overlay's {axis} edge is {} px inside the column's, not its {want} px offset",
            child - edge
        );
    }
}

/// **Nothing is drawn outside the clipped block**: the clipper's oversized child
/// fills its inside, stops at its padding box — so the clipper's border is still
/// the border — and not one of its pixels is right of or below the clipper,
/// where the child's own box reaches.
fn nothing_of_the_clipped_child_is_outside_its_clip(image: &Image, layout: &UiTreeLayout) {
    let (clip_min, clip_max) = layout.clipper;
    let (child_min, child_max) = layout.overflow;
    assert!(
        child_max.cmpgt(clip_max).all(),
        "the child does not overflow the clipper, so this proves nothing"
    );
    let child = at(image, child_min + Vec2::splat(4.0 * layout.unit));
    let clear = image.pixel(1, 1).expect("inside");
    assert!(
        differ(child, clear),
        "the clipped child drew nothing inside its clip"
    );

    let mid = (clip_min + clip_max) * 0.5;
    let (width, height) = EXTENT;
    let child_x_end = (child_max.x as u32).min(width);
    let child_y_end = (child_max.y as u32).min(height);
    for x in clip_max.x as u32..child_x_end {
        for y in [child_min.y as u32 + 1, mid.y as u32] {
            let pixel = image.pixel(x, y).expect("inside");
            assert!(
                !same(pixel, child),
                "({x}, {y}) right of the clipper is the child's {pixel:?} — drawn outside its clip"
            );
        }
    }
    for y in clip_max.y as u32..child_y_end {
        for x in [child_min.x as u32 + 1, mid.x as u32] {
            let pixel = image.pixel(x, y).expect("inside");
            assert!(
                !same(pixel, child),
                "({x}, {y}) below the clipper is the child's {pixel:?} — drawn outside its clip"
            );
        }
    }

    // And the clip is the padding box, not the border box: the border the
    // child's box runs under is still the border where the child crosses it.
    let border = at(image, Vec2::new(clip_min.x, mid.y));
    assert!(
        differ(border, child),
        "the clipper's left border is the child's colour"
    );
    for (edge, point) in [
        ("right", Vec2::new(clip_max.x - 1.0, child_min.y + 4.0)),
        ("bottom", Vec2::new(child_min.x + 4.0, clip_max.y - 1.0)),
    ] {
        let pixel = at(image, point);
        assert!(
            same(pixel, border),
            "the clipper's {edge} border where the child crosses it is {pixel:?}, not the border \
             {border:?} — the child was clipped to the border box"
        );
    }
}

fn every_layout_promise_is_on_the_frame(image: &Image) {
    let layout = ui_tree_layout(EXTENT);
    the_cells_are_whole_and_a_gap_apart(image, &layout);
    the_overlay_sits_at_its_offset(image, &layout);
    nothing_of_the_clipped_child_is_outside_its_clip(image, &layout);
}

/// [`Scene::UiTree`] drawn, against the reference in `tests/golden/`.
#[test]
#[ignore = "needs a real GPU and a backend pin; run tests/run-render-e2e.sh"]
fn the_ui_tree_scene_keeps_every_layout_promise_and_matches_its_golden() {
    super::draw_scene_and_match_its_golden(
        Scene::UiTree,
        "ui_tree",
        EXTENT,
        MIN_COLORS_UI_TREE,
        every_layout_promise_is_on_the_frame,
    );
}
