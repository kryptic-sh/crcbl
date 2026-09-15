//! The readout panel a 3D sample draws its numbers in.
//!
//! ```text
//!  ┌────────────────────┐
//!  │ POSITION  0.0 18.0 │   ← a label at the left margin, a reading
//!  │ HEALTH    100/100  │     right-aligned against the right one
//!  │ TORCHES   LIT      │
//!  └────────────────────┘
//!
//!         W/A/S/D walk   Q/E turn   SPACE strikes      ← the centred hint
//! ```
//!
//! # Geometry only, and the rows are the caller's
//!
//! A [`ReadoutPanel`] is an inset, a width, a row height, a padding and three
//! colours. It formats nothing and decides nothing: a sample builds its own
//! [`ReadoutRow`]s, in its own order, with its own colour per row, and this
//! draws them. It is the whole of what five samples were each writing out.
//!
//! # Laid out by the element tree
//!
//! The panel is a [`crate::tree`] block — a column with the border and padding
//! the fields name, one fixed-height row block per [`ReadoutRow`], a label span
//! in each and a reading span positioned against the row's right edge — and the
//! hint is a span centred along the bottom of a surface-sized block. The public
//! API is the one the samples always called, and every command lands where the
//! hand-written arithmetic put it; the tests below hold the two to bit-for-bit
//! equality.
//!
//! **One place it now differs.** Layout is rounded to whole pixels, so a hint
//! whose centred position fell on a half pixel — an odd surface width — lands
//! on the pixel to its right.
//!
//! # Laid out against the surface
//!
//! [`ReadoutPanel::hint`] takes the extent the swapchain was actually acquired
//! at rather than a size the page assumed, so a hint is centred in a resized
//! window and in a headless offscreen ring at whatever size was asked for. The
//! panel itself hangs off its own corner and needs no extent at all.

use glam::Vec2;

use crate::draw_list::DrawList;
use crate::text::FontAtlas;
use crate::tree::{
    Align, AvailableSpace, Edges, FlexDirection, Justify, Length, LengthAuto, NodeStyle, Position,
    Ui,
};
use crate::widget::{NATURAL_FONT_SIZE, PointerInput};

/// The scale [`FontAtlas::text_width`] is measured at, which is a multiplier on
/// the baked glyph size rather than a size in pixels.
///
/// A panel draws at the font's natural size, so it measures at the natural
/// scale. Public because a page's own tests measure the strings it emitted the
/// same way the layout did — measuring at a different scale would assert about
/// a panel nothing drew.
pub const NATURAL_SCALE: f32 = 1.0;

/// One row: a label at the panel's left margin and a reading right-aligned
/// against its right one.
///
/// The value is already formatted — the panel does no formatting of its own,
/// on [`crate::debug::DebugRow`]'s terms — and the colour is the row's rather
/// than the panel's, because the one piece of state each of these pages puts a
/// colour on is a reading and not a label.
#[derive(Clone, Debug, PartialEq)]
pub struct ReadoutRow {
    /// What the number is.
    pub label: String,
    /// The number, already formatted.
    pub value: String,
    /// What the reading is drawn in.
    pub colour: [f32; 4],
}

impl ReadoutRow {
    /// A row.
    #[must_use]
    pub fn new(label: impl Into<String>, value: impl Into<String>, colour: [f32; 4]) -> Self {
        Self {
            label: label.into(),
            value: value.into(),
            colour,
        }
    }
}

/// Where a readout panel sits, how wide it is, and what it is drawn in.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct ReadoutPanel {
    /// The panel's inset from the corner of the surface, in pixels — and the
    /// bottom margin [`ReadoutPanel::hint`] leaves.
    pub inset: f32,
    /// How wide the panel is, in pixels. Wide enough for the longest label and
    /// a right-aligned reading beside it, which is a fact about the rows a
    /// sample puts in it rather than one this crate can know.
    pub width: f32,
    /// The height of one row, in pixels.
    pub row_height: f32,
    /// The panel's padding inside its own border, in pixels.
    pub pad: f32,
    /// How thick the panel's border is, in pixels.
    pub border_width: f32,
    /// What the panel is filled with.
    pub background: [f32; 4],
    /// What its border is drawn in.
    pub border: [f32; 4],
    /// What a label — and a hint — is drawn in. A reading's colour is the
    /// row's; see [`ReadoutRow::colour`].
    pub label: [f32; 4],
}

impl ReadoutPanel {
    /// How tall a panel of `rows` rows stands, in pixels.
    ///
    /// What a page stacking a second panel under the first offsets it by.
    #[must_use]
    pub fn height(&self, rows: usize) -> f32 {
        2.0f32.mul_add(self.pad, rows as f32 * self.row_height)
    }

    /// Draws the panel in the top-left corner, [`ReadoutPanel::inset`] from
    /// both edges.
    pub fn draw(&self, list: &mut DrawList, atlas: &FontAtlas, rows: &[ReadoutRow]) {
        self.draw_at(list, atlas, Vec2::new(self.inset, self.inset), rows);
    }

    /// Draws it at `origin` instead, for a page that stacks two of them.
    ///
    /// `atlas` is only measured against — the glyphs themselves are the UI
    /// pass's business — and it is what right-aligns the readings against a
    /// proportional font rather than against a guess. Its measurements take a
    /// scale relative to the baked glyph size, not a pixel size, so everything
    /// here is drawn at [`NATURAL_FONT_SIZE`] and measured at
    /// [`NATURAL_SCALE`].
    pub fn draw_at(
        &self,
        list: &mut DrawList,
        atlas: &FontAtlas,
        origin: Vec2,
        rows: &[ReadoutRow],
    ) {
        // The border is inside the panel's `pad`, as it always was: content
        // starts `pad` in from the outer edge whatever the border's width.
        let panel = NodeStyle {
            width: LengthAuto::Px(self.width),
            flex_direction: FlexDirection::Column,
            border: Edges::all(self.border_width),
            padding: Edges::all(Length::Px((self.pad - self.border_width).max(0.0))),
            background: self.background,
            border_color: self.border,
            ..NodeStyle::DEFAULT
        };
        let row = NodeStyle {
            height: LengthAuto::Px(self.row_height),
            ..NodeStyle::DEFAULT
        };
        let label = NodeStyle {
            color: self.label,
            font_size: NATURAL_FONT_SIZE,
            ..NodeStyle::DEFAULT
        };
        // Out of the row's flow and against its right edge, so a reading ends
        // on the margin however long the label beside it is.
        let reading_at = NodeStyle {
            position: Position::Absolute,
            inset: Edges {
                top: LengthAuto::Px(0.0),
                right: LengthAuto::Px(0.0),
                ..Edges::all(LengthAuto::Auto)
            },
            ..label
        };

        let mut ui = Ui::new();
        ui.begin_frame(PointerInput::default());
        ui.block(None, &panel, |ui| {
            for (index, reading) in rows.iter().enumerate() {
                ui.block_keyed(index, &row, |ui| {
                    ui.span(reading.label.as_str(), &label);
                    ui.span(
                        reading.value.as_str(),
                        &NodeStyle {
                            color: reading.colour,
                            ..reading_at
                        },
                    );
                });
            }
        });
        ui.layout(origin, AvailableSpace::MAX_CONTENT, atlas);
        ui.emit(list);
    }

    /// Draws `text` centred along the bottom of a surface of `extent`, one row
    /// above [`ReadoutPanel::inset`].
    ///
    /// The control hint, which on these pages is the whole of what a first-time
    /// visitor needs. Its text is the sample's; where it lands is the panel's,
    /// so a page and its readout keep one margin between them.
    pub fn hint(&self, list: &mut DrawList, atlas: &FontAtlas, extent: (u32, u32), text: &str) {
        let size = Vec2::new(extent.0 as f32, extent.1 as f32);
        let surface = NodeStyle {
            width: LengthAuto::Px(size.x),
            height: LengthAuto::Px(size.y),
            flex_direction: FlexDirection::Column,
            justify_content: Some(Justify::FlexEnd),
            align_items: Some(Align::Center),
            ..NodeStyle::DEFAULT
        };
        let hint = NodeStyle {
            height: LengthAuto::Px(self.row_height),
            // A margin rather than the surface's padding, so a surface shorter
            // than the inset still puts the hint the inset above its bottom edge.
            margin: Edges {
                bottom: LengthAuto::Px(self.inset),
                ..Edges::all(LengthAuto::Px(0.0))
            },
            // A hint taller than the room above it overflows past the top
            // rather than being squashed, as the arithmetic it replaced did.
            flex_shrink: 0.0,
            color: self.label,
            font_size: NATURAL_FONT_SIZE,
            ..NodeStyle::DEFAULT
        };

        let mut ui = Ui::new();
        ui.begin_frame(PointerInput::default());
        ui.block(None, &surface, |ui| {
            ui.span(text, &hint);
        });
        ui.layout(Vec2::ZERO, AvailableSpace::definite(size), atlas);
        ui.emit(list);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::draw_list::DrawCommand;

    const PANEL: ReadoutPanel = ReadoutPanel {
        inset: 16.0,
        width: 180.0,
        row_height: 18.0,
        pad: 8.0,
        border_width: 1.0,
        background: [0.06, 0.07, 0.11, 0.80],
        border: [0.34, 0.38, 0.48, 1.0],
        label: [0.66, 0.70, 0.80, 1.0],
    };

    const READING: [f32; 4] = [0.95, 0.96, 1.0, 1.0];

    fn rows() -> Vec<ReadoutRow> {
        vec![
            ReadoutRow::new("SHORT", "1", READING),
            ReadoutRow::new("A LONGER LABEL", "-12.75 m", READING),
        ]
    }

    /// **A reading is right-aligned against the panel's own inner edge**, which
    /// is the arithmetic every one of these pages was writing out for itself:
    /// the value's measured width comes off the right margin, so a wider
    /// reading starts further left and both end on the same column.
    #[test]
    fn a_reading_ends_at_the_panels_right_margin_whatever_it_says() {
        let atlas = FontAtlas::built_in();
        let mut list = DrawList::new();
        let rows = rows();
        PANEL.draw(&mut list, &atlas, &rows);

        let drawn: Vec<(Vec2, String)> = list
            .commands()
            .iter()
            .filter_map(|command| match command {
                DrawCommand::Text { pos, text, .. } => Some((*pos, text.clone())),
                _ => None,
            })
            .collect();
        assert_eq!(drawn.len(), rows.len() * 2, "a label and a reading a row");

        let right = PANEL.inset + PANEL.width - PANEL.pad;
        for row in &rows {
            let (pos, _) = drawn
                .iter()
                .find(|(_, text)| *text == row.value)
                .unwrap_or_else(|| panic!("{} was not drawn", row.value));
            let end = pos.x + atlas.text_width(&row.value, NATURAL_SCALE);
            assert!(
                (end - right).abs() < 1e-3,
                "{} ends at {end} rather than the panel's {right}",
                row.value,
            );
        }
    }

    /// **A label sits at the left margin and a row is one `row_height` below
    /// the last**, so a panel of any length keeps its own grid.
    #[test]
    fn every_row_stands_one_row_height_below_the_one_above_it() {
        let atlas = FontAtlas::built_in();
        let mut list = DrawList::new();
        let rows = rows();
        PANEL.draw(&mut list, &atlas, &rows);

        let labels: Vec<Vec2> = list
            .commands()
            .iter()
            .filter_map(|command| match command {
                DrawCommand::Text { pos, text, .. }
                    if rows.iter().any(|row| row.label == *text) =>
                {
                    Some(*pos)
                }
                _ => None,
            })
            .collect();
        assert_eq!(labels.len(), rows.len());
        for (index, pos) in labels.iter().enumerate() {
            assert!((pos.x - (PANEL.inset + PANEL.pad)).abs() < 1e-3, "{pos:?}");
            let want = PANEL.inset + PANEL.pad + index as f32 * PANEL.row_height;
            assert!((pos.y - want).abs() < 1e-3, "row {index} at {pos:?}");
        }
    }

    /// **The box is as tall as its rows say**, which is what a page stacking a
    /// second panel under the first offsets it by.
    #[test]
    fn the_box_is_as_tall_as_the_rows_it_holds() {
        let atlas = FontAtlas::built_in();
        let mut list = DrawList::new();
        let rows = rows();
        PANEL.draw(&mut list, &atlas, &rows);

        let (min, max) = list
            .commands()
            .iter()
            .find_map(|command| match command {
                DrawCommand::Rect { min, max, .. } => Some((*min, *max)),
                _ => None,
            })
            .expect("the panel drew no box");
        assert_eq!(min, Vec2::new(PANEL.inset, PANEL.inset));
        assert_eq!(max.x, PANEL.inset + PANEL.width);
        assert!((max.y - (PANEL.inset + PANEL.height(rows.len()))).abs() < 1e-3);
    }

    /// The arithmetic `draw_at` was before the element tree laid it out,
    /// verbatim: the oracle the tree's output is held to.
    fn arithmetic_draw_at(
        panel: &ReadoutPanel,
        list: &mut DrawList,
        atlas: &FontAtlas,
        origin: Vec2,
        rows: &[ReadoutRow],
    ) {
        let max = Vec2::new(origin.x + panel.width, origin.y + panel.height(rows.len()));
        list.rect(origin, max, panel.background);
        list.rect_outline(origin, max, panel.border_width, panel.border);
        for (index, row) in rows.iter().enumerate() {
            let y = origin.y + panel.pad + index as f32 * panel.row_height;
            list.text(
                Vec2::new(origin.x + panel.pad, y),
                row.label.clone(),
                panel.label,
                NATURAL_FONT_SIZE,
            );
            let reading = atlas.text_width(&row.value, NATURAL_SCALE);
            list.text(
                Vec2::new(max.x - panel.pad - reading, y),
                row.value.clone(),
                row.colour,
                NATURAL_FONT_SIZE,
            );
        }
    }

    /// The arithmetic `hint` was, verbatim.
    fn arithmetic_hint(
        panel: &ReadoutPanel,
        list: &mut DrawList,
        atlas: &FontAtlas,
        extent: (u32, u32),
        text: &str,
    ) {
        let width = extent.0 as f32;
        let height = extent.1 as f32;
        let measured = atlas.text_width(text, NATURAL_SCALE);
        list.text(
            Vec2::new(
                (width - measured) * 0.5,
                height - panel.inset - panel.row_height,
            ),
            text,
            panel.label,
            NATURAL_FONT_SIZE,
        );
    }

    /// Every command and the clip it was pushed under, with every float printed
    /// to round-trip precision — so two renderings are equal exactly when every
    /// float is the same value.
    fn rendering(list: &DrawList) -> Vec<String> {
        list.commands()
            .iter()
            .zip(list.clips())
            .map(|(command, clip)| format!("{command:?} under {clip:?}"))
            .collect()
    }

    /// The five samples' panels, as each page declares it.
    fn sample_panels() -> [(&'static str, ReadoutPanel); 6] {
        let widths = [
            ("shard", 190.0),
            ("towers readout", 176.0),
            ("towers build", 168.0),
            ("sparks", 196.0),
            ("puppet", 168.0),
            ("breach", 180.0),
        ];
        widths.map(|(name, width)| (name, ReadoutPanel { width, ..PANEL }))
    }

    /// **For every panel a sample declares, the tree draws exactly the commands
    /// the arithmetic did**, float for float, at the corner and at a stacked
    /// origin like towers' — which is what keeps every page that draws one
    /// looking the same.
    #[test]
    fn the_tree_draws_every_sample_panel_exactly_where_the_arithmetic_did() {
        let atlas = FontAtlas::built_in();
        let rows = [
            ReadoutRow::new("POSITION", "0.0 18.0", READING),
            ReadoutRow::new("HEALTH", "100/100", [0.95, 0.40, 0.36, 1.0]),
            ReadoutRow::new("TORCHES", "LIT", READING),
            ReadoutRow::new("", "", READING),
            ReadoutRow::new("W", "-12.75 m", READING),
        ];
        for (name, panel) in sample_panels() {
            for count in [0, 1, rows.len()] {
                for origin in [Vec2::splat(panel.inset), Vec2::new(16.0, 114.0)] {
                    let mut tree = DrawList::new();
                    panel.draw_at(&mut tree, &atlas, origin, &rows[..count]);
                    let mut arithmetic = DrawList::new();
                    arithmetic_draw_at(&panel, &mut arithmetic, &atlas, origin, &rows[..count]);
                    assert_eq!(
                        rendering(&tree),
                        rendering(&arithmetic),
                        "{name}'s panel of {count} rows at {origin:?}"
                    );
                }
            }
        }
    }

    /// **The hint lands exactly where the arithmetic put it on every surface
    /// whose centring is a whole pixel**, and on an odd width — where the
    /// arithmetic's half pixel is rounded — half a pixel right of it.
    #[test]
    fn the_hint_lands_where_the_arithmetic_did_and_rounds_a_half_pixel() {
        let atlas = FontAtlas::built_in();
        let hint = "W/A/S/D walk   Q/E turn   SPACE strikes";
        for extent in [(256, 192), (960, 720), (1920, 1080), (961, 721), (99, 10)] {
            let mut tree = DrawList::new();
            PANEL.hint(&mut tree, &atlas, extent, hint);
            let mut arithmetic = DrawList::new();
            arithmetic_hint(&PANEL, &mut arithmetic, &atlas, extent, hint);
            if extent.0 % 2 == 0 {
                assert_eq!(rendering(&tree), rendering(&arithmetic), "{extent:?}");
                continue;
            }
            let at = |list: &DrawList| match &list.commands()[0] {
                DrawCommand::Text { pos, .. } => *pos,
                other => panic!("{other:?}"),
            };
            assert_eq!(
                at(&tree),
                at(&arithmetic) + Vec2::new(0.5, 0.0),
                "{extent:?}"
            );
        }
    }

    /// **The hint is centred on the surface it was handed**, not on one the
    /// page assumed.
    #[test]
    fn the_hint_is_centred_on_whatever_surface_it_was_given() {
        let atlas = FontAtlas::built_in();
        for extent in [(960u32, 720u32), (1920, 1080)] {
            let mut list = DrawList::new();
            PANEL.hint(&mut list, &atlas, extent, "W/A/S/D walk");
            let pos = list
                .commands()
                .iter()
                .find_map(|command| match command {
                    DrawCommand::Text { pos, .. } => Some(*pos),
                    _ => None,
                })
                .expect("the hint drew nothing");
            let measured = atlas.text_width("W/A/S/D walk", NATURAL_SCALE);
            assert!(
                (pos.x + measured * 0.5 - extent.0 as f32 * 0.5).abs() < 1e-3,
                "the hint is off centre on a {extent:?} surface: {pos:?}"
            );
            assert!(
                (pos.y - (extent.1 as f32 - PANEL.inset - PANEL.row_height)).abs() < 1e-3,
                "the hint is not one row above the bottom margin: {pos:?}"
            );
        }
    }
}
