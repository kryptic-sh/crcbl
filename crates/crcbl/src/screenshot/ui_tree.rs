//! [`Scene::UiTree`](super::Scene::UiTree)'s content: a small panel built with
//! `docs/plan/07-ui-debug.md` rungs 2 and 3's element tree and laid out by
//! Taffy, so that each thing layout promises can be read back off the frame.
//!
//! # The layout, and what each part of it is for
//!
//! ```text
//!   ┌─────────────────────────────────────────┐
//!   │  ┌─────┐     ┌─────┐     ┌─────┐        │  the gap row: three cells
//!   │  │ red │ gap │green│ gap │blue │        │  of a fixed width, a column
//!   │  └─────┘     └─────┘     └─────┘        │  gap apart
//!   └─────────────────────────────────────────┘
//!   ┌──────────────────┐   ╔═══════════════╗
//!   │ ┌──────────────┐ │   ║ ┌─────────────╫──┐  the clipper: a bordered,
//!   │ │orange ┌────┐ │ │   ║ │ lime        ║  │  `overflow: hidden` block
//!   │ └───────│over│─┘ │   ║ │             ║  │  whose child is far bigger
//!   │ ┌───────│lay │─┐ │   ╚═╪═════════════╝  │  than it
//!   │ │teal   └────┘ │ │     └────────────────┘
//!   │ └──────────────┘ │  the column: two stretched rows and an
//!   └──────────────────┘  absolutely positioned overlay
//! ```
//!
//! * **The gap row's cells** are a known width and a known gap apart, so the
//!   run of panel colour between two of them is the gap and nothing else.
//! * **The overlay** is absolutely positioned at a known offset inside the
//!   column, so its first pixel sits that far from the column's own edge.
//! * **The clipper's child** is larger than the clipper on both axes and
//!   starts inside it, so any pixel of it outside the clipper's padding box is
//!   a clip that did not happen.
//!
//! Every length is a whole number of base pixels times a whole-number unit, so
//! every edge is on the pixel grid at every `--size` that fits
//! [`UI_TREE_BASE`].

use glam::Vec2;

use crate::ui::draw_list::DrawList;
use crate::ui::text::FontAtlas;
use crate::ui::tree::{
    AvailableSpace, Edges, FlexDirection, Length, LengthAuto, NodeKey, NodeStyle, Overflow,
    Position, Ui,
};
use crate::ui::widget::PointerInput;

/// The extent the layout is written at. A frame at a whole multiple of it on
/// both axes draws the same picture that many times larger.
pub const UI_TREE_BASE: (u32, u32) = (256, 192);

/// The gap row's column gap, in base pixels.
pub const UI_TREE_GAP: f32 = 8.0;

/// Each gap-row cell's width, in base pixels.
pub const UI_TREE_CELL: f32 = 40.0;

/// The overlay's `left` and `top` inside the column, in base pixels.
pub const UI_TREE_OVERLAY_OFFSET: Vec2 = Vec2::new(30.0, 14.0);

/// The clipper's border width, in base pixels.
pub const UI_TREE_CLIP_BORDER: f32 = 2.0;

/// The panels' fill, in linear light.
pub const UI_TREE_PANEL: [f32; 4] = [0.05, 0.06, 0.10, 1.0];

/// The gap row's three cells, in linear light.
pub const UI_TREE_CELLS: [[f32; 4]; 3] = [
    [0.80, 0.08, 0.06, 1.0],
    [0.08, 0.60, 0.10, 1.0],
    [0.06, 0.12, 0.85, 1.0],
];

/// The column's two rows, in linear light.
pub const UI_TREE_ROWS: [[f32; 4]; 2] = [[0.90, 0.40, 0.05, 1.0], [0.05, 0.45, 0.45, 1.0]];

/// The absolute overlay, in linear light.
pub const UI_TREE_OVERLAY: [f32; 4] = [0.75, 0.10, 0.70, 1.0];

/// The clipper's border, in linear light.
pub const UI_TREE_CLIP_BORDER_COLOR: [f32; 4] = [1.0, 0.85, 0.0, 1.0];

/// The clipper's oversized child, in linear light.
pub const UI_TREE_OVERFLOW: [f32; 4] = [0.45, 0.95, 0.20, 1.0];

/// Where every part of the scene was laid out, in screen pixels, each as the
/// `(min, max)` of its border box.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct UiTreeLayout {
    /// Pixels per base pixel.
    pub unit: f32,
    /// The gap row.
    pub gap_row: (Vec2, Vec2),
    /// Its three cells, left to right.
    pub cells: [(Vec2, Vec2); 3],
    /// The column holding two rows and the overlay.
    pub column: (Vec2, Vec2),
    /// The absolutely positioned overlay.
    pub overlay: (Vec2, Vec2),
    /// The `overflow: hidden` block.
    pub clipper: (Vec2, Vec2),
    /// The clipper's child, as laid out — before the clip.
    pub overflow: (Vec2, Vec2),
}

/// The keys of the parts [`UiTreeLayout`] reports.
struct Parts {
    gap_row: NodeKey,
    cells: [NodeKey; 3],
    column: NodeKey,
    overlay: NodeKey,
    clipper: NodeKey,
    overflow: NodeKey,
}

/// Builds and lays out the scene's tree for an `extent`-sized frame.
fn build(extent: (u32, u32)) -> (Ui, Parts, f32) {
    let unit = (extent.0 / UI_TREE_BASE.0)
        .min(extent.1 / UI_TREE_BASE.1)
        .max(1) as f32;
    let px = |base: f32| LengthAuto::Px(base * unit);
    let pad = |base: f32| Edges::all(Length::Px(base * unit));
    let fill = |color: [f32; 4]| NodeStyle {
        background: color,
        ..NodeStyle::DEFAULT
    };

    let root = NodeStyle {
        width: px(UI_TREE_BASE.0 as f32),
        height: px(UI_TREE_BASE.1 as f32),
        flex_direction: FlexDirection::Column,
        padding: pad(12.0),
        row_gap: Length::Px(12.0 * unit),
        ..NodeStyle::DEFAULT
    };
    let gap_row = NodeStyle {
        height: px(48.0),
        padding: pad(8.0),
        column_gap: Length::Px(UI_TREE_GAP * unit),
        ..fill(UI_TREE_PANEL)
    };
    let cell = |color| NodeStyle {
        width: px(UI_TREE_CELL),
        flex_shrink: 0.0,
        ..fill(color)
    };
    let lower = NodeStyle {
        flex_grow: 1.0,
        column_gap: Length::Px(12.0 * unit),
        ..NodeStyle::DEFAULT
    };
    let column = NodeStyle {
        width: px(100.0),
        flex_direction: FlexDirection::Column,
        padding: pad(6.0),
        row_gap: Length::Px(6.0 * unit),
        ..fill(UI_TREE_PANEL)
    };
    let row = |color| NodeStyle {
        height: px(20.0),
        ..fill(color)
    };
    let overlay = NodeStyle {
        position: Position::Absolute,
        inset: Edges {
            left: px(UI_TREE_OVERLAY_OFFSET.x),
            top: px(UI_TREE_OVERLAY_OFFSET.y),
            ..Edges::all(LengthAuto::Auto)
        },
        width: px(24.0),
        height: px(24.0),
        ..fill(UI_TREE_OVERLAY)
    };
    let clipper = NodeStyle {
        width: px(80.0),
        height: px(60.0),
        overflow: Overflow::Hidden,
        border: Edges::all(UI_TREE_CLIP_BORDER * unit),
        border_color: UI_TREE_CLIP_BORDER_COLOR,
        ..fill(UI_TREE_PANEL)
    };
    let overflow = NodeStyle {
        width: px(140.0),
        height: px(100.0),
        margin: Edges {
            left: px(10.0),
            top: px(10.0),
            ..Edges::all(LengthAuto::Px(0.0))
        },
        flex_shrink: 0.0,
        ..fill(UI_TREE_OVERFLOW)
    };

    let mut ui = Ui::new();
    ui.begin_frame(PointerInput::default());
    let mut keys = Vec::new();
    ui.block(Some("#root"), &root, |ui| {
        let row_key = ui
            .block(Some("#gap-row"), &gap_row, |ui| {
                for (index, color) in UI_TREE_CELLS.into_iter().enumerate() {
                    keys.push(ui.block_keyed(index, &cell(color), |_| {}).key);
                }
            })
            .key;
        keys.push(row_key);
        ui.block(Some("#lower"), &lower, |ui| {
            let column_key = ui
                .block(Some("#column"), &column, |ui| {
                    ui.block(Some("#first"), &row(UI_TREE_ROWS[0]), |_| {});
                    ui.block(Some("#second"), &row(UI_TREE_ROWS[1]), |_| {});
                    keys.push(ui.block(Some("#overlay"), &overlay, |_| {}).key);
                })
                .key;
            keys.push(column_key);
            let clipper_key = ui
                .block(Some("#clipper"), &clipper, |ui| {
                    keys.push(ui.block(Some("#overflow"), &overflow, |_| {}).key);
                })
                .key;
            keys.push(clipper_key);
        });
    });
    ui.layout(
        Vec2::ZERO,
        AvailableSpace::definite(Vec2::new(extent.0 as f32, extent.1 as f32)),
        &FontAtlas::built_in(),
    );
    // Pushed in the order the closures above return: each cell before its
    // row, the overlay before its column, the child before its clipper.
    let [a, b, c, gap_row, overlay, column, overflow, clipper] = keys[..] else {
        unreachable!("the scene builds exactly eight reported parts");
    };
    let parts = Parts {
        gap_row,
        cells: [a, b, c],
        column,
        overlay,
        clipper,
        overflow,
    };
    (ui, parts, unit)
}

/// The layout of [`Scene::UiTree`](super::Scene::UiTree) in an `extent`-sized
/// frame, as the tree laid it out.
#[must_use]
pub fn ui_tree_layout(extent: (u32, u32)) -> UiTreeLayout {
    let (ui, parts, unit) = build(extent);
    let rect = |key| ui.rect(key).expect("every reported part was laid out");
    UiTreeLayout {
        unit,
        gap_row: rect(parts.gap_row),
        cells: parts.cells.map(rect),
        column: rect(parts.column),
        overlay: rect(parts.overlay),
        clipper: rect(parts.clipper),
        overflow: rect(parts.overflow),
    }
}

/// The scene's draw list for an `extent`-sized frame.
#[must_use]
pub fn ui_tree_draw_list(extent: (u32, u32)) -> DrawList {
    let (ui, _, _) = build(extent);
    let mut list = DrawList::new();
    ui.emit(&mut list);
    list
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ui::draw_list::ClipRect;

    /// The scene's structure holds before any pixel is read: the cells are a
    /// gap apart, the overlay is at its offset, the clipper's child really
    /// overflows it on both axes, everything is inside the frame, and exactly
    /// the child is clipped — at the base extent and at twice it.
    #[test]
    fn the_scene_is_laid_out_as_its_claims_need() {
        for extent in [UI_TREE_BASE, (512, 384)] {
            let layout = ui_tree_layout(extent);
            let unit = layout.unit;
            for pair in layout.cells.windows(2) {
                assert_eq!(pair[1].0.x - pair[0].1.x, UI_TREE_GAP * unit, "{extent:?}");
            }
            assert_eq!(
                layout.overlay.0 - layout.column.0,
                UI_TREE_OVERLAY_OFFSET * unit,
                "{extent:?}"
            );
            assert!(
                layout.overflow.1.cmpgt(layout.clipper.1).all()
                    && layout.overflow.0.cmpgt(layout.clipper.0).all(),
                "{extent:?}: the child does not overflow the clipper"
            );
            let list = ui_tree_draw_list(extent);
            let frame = Vec2::new(extent.0 as f32, extent.1 as f32);
            let clipped = list
                .clips()
                .iter()
                .filter(|clip| **clip != ClipRect::NONE)
                .count();
            assert_eq!(clipped, 1, "{extent:?}: only the child is clipped");
            for command in list.commands() {
                if let crate::ui::draw_list::DrawCommand::Rect { min, max, .. } = command {
                    assert!(
                        min.cmpge(Vec2::ZERO).all() && max.cmple(frame).all()
                            || *min == layout.overflow.0,
                        "{extent:?}: {command:?} leaves the frame"
                    );
                }
            }
        }
    }
}
