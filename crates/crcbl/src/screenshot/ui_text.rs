//! [`Scene::UiText`](super::Scene::UiText)'s content: real text —
//! `docs/plan/07-ui-debug.md` rung 5 — drawn in the committed font through the
//! glyph atlas, laid out by the element tree and styled by a stylesheet, so
//! that what wrapping, kerning and alignment promise can be read back off the
//! frame.
//!
//! # The layout, and what each part of it is for
//!
//! ```text
//!   ┌─ #page ──────────────────────────────────────┐
//!   │ ┌─ #paragraph ──────┐                          │  a paragraph wrapped to a
//!   │ │ LEFT THE FILE FELT│                          │  fixed width, on a fixed
//!   │ │ THE TILE LIFT THE │                          │  line pitch
//!   │ │ HILT              │                          │
//!   │ └───────────────────┘                          │
//!   │ HILL  HILL                                     │  two sizes, one twice the
//!   │ To    To                                       │  other; a kerned pair, and
//!   │ ┌─ #centred ────────────────────────────────┐  │  the same glyphs unkerned
//!   │ │                   HIH                     │  │  centred text
//!   │ └───────────────────────────────────────────┘  │
//!   └──────────────────────────────────────────────┘
//! ```
//!
//! * **The paragraph** is words of flat-topped, flat-bottomed capitals only, so
//!   every line's ink starts at its cap height and ends at its baseline and the
//!   rows two lines apart are exactly the pitch apart.
//! * **The two sizes** are one word at [`UI_TEXT_SMALL`] and at twice that.
//! * **The kerned pair** `To` is a tree span; beside it the same two glyphs are
//!   drawn as a run placed by their advances alone, with no kerning.
//! * **The centred text** is a word whose first glyph's left bearing equals its
//!   last glyph's right bearing, so its ink is centred exactly when its advance
//!   box is.
//!
//! Every colour and length the frame is measured against is a constant here,
//! and the stylesheet is written from those constants.

use glam::Vec2;

use crate::ui::draw_list::{DrawCommand, DrawList};
use crate::ui::font::Font;
use crate::ui::font::layout::{PositionedGlyph, TextLayout};
use crate::ui::text::FontAtlas;
use crate::ui::tree::{AvailableSpace, NodeKey, Ui};
use crate::ui::widget::PointerInput;

/// The page's fill, as the stylesheet writes it: sRGB bytes.
pub const UI_TEXT_PAGE: [u8; 3] = [0x18, 0x1c, 0x24];

/// The paragraph block's fill.
pub const UI_TEXT_PARAGRAPH_FILL: [u8; 3] = [0x20, 0x30, 0x50];

/// The centred block's fill.
pub const UI_TEXT_CENTRED_FILL: [u8; 3] = [0x40, 0x20, 0x30];

/// Every glyph's colour.
pub const UI_TEXT_INK: [u8; 3] = [0xf8, 0xf4, 0xe0];

/// The paragraph block's width, in pixels.
pub const UI_TEXT_PARAGRAPH_WIDTH: f32 = 132.0;

/// The paragraph's font size, in pixels.
pub const UI_TEXT_PARAGRAPH_SIZE: f32 = 13.0;

/// The paragraph's line pitch, in pixels.
pub const UI_TEXT_LINE_HEIGHT: f32 = 16.0;

/// The paragraph: capitals with flat tops and flat bottoms only.
pub const UI_TEXT_PARAGRAPH: &str = "LEFT THE FILE FELT THE TILE LIFT THE HILT";

/// The word drawn at two sizes.
pub const UI_TEXT_SIZES_WORD: &str = "HILL";

/// The smaller of the two sizes; the larger is twice it.
pub const UI_TEXT_SMALL: f32 = 12.0;

/// The kerned pair.
pub const UI_TEXT_PAIR: &str = "To";

/// The pair's font size, in pixels.
pub const UI_TEXT_PAIR_SIZE: f32 = 32.0;

/// The pair's line pitch, in pixels.
pub const UI_TEXT_PAIR_LINE_HEIGHT: f32 = 36.0;

/// The centred word.
pub const UI_TEXT_CENTRED: &str = "HIH";

/// The centred block's width, in pixels.
pub const UI_TEXT_CENTRED_WIDTH: f32 = 200.0;

/// The centred word's font size, in pixels.
pub const UI_TEXT_CENTRED_SIZE: f32 = 20.0;

fn hex([r, g, b]: [u8; 3]) -> String {
    format!("#{r:02x}{g:02x}{b:02x}")
}

/// The scene's stylesheet, written from the constants above.
#[must_use]
pub fn ui_text_css() -> String {
    format!(
        "\
#page {{
  width: 100%;
  height: 100%;
  flex-direction: column;
  align-items: flex-start;
  gap: 6px;
  padding: 8px;
  background: {page};
  color: {ink};
  font-family: sans-serif;
}}
#paragraph {{
  flex-direction: column;
  width: {paragraph_width}px;
  background: {paragraph_fill};
  font-size: {paragraph_size}px;
  line-height: {line_height}px;
}}
#sizes, #pairs {{
  gap: 12px;
  align-items: flex-end;
}}
.small {{
  font-size: {small}px;
}}
.large {{
  font-size: {large}px;
}}
.pair {{
  font-size: {pair_size}px;
  line-height: {pair_line}px;
}}
#unkerned {{
  width: 40px;
  height: {pair_line}px;
}}
#centred {{
  flex-direction: column;
  width: {centred_width}px;
  background: {centred_fill};
  font-size: {centred_size}px;
  line-height: 24px;
  text-align: center;
}}
",
        page = hex(UI_TEXT_PAGE),
        ink = hex(UI_TEXT_INK),
        paragraph_width = UI_TEXT_PARAGRAPH_WIDTH,
        paragraph_fill = hex(UI_TEXT_PARAGRAPH_FILL),
        paragraph_size = UI_TEXT_PARAGRAPH_SIZE,
        line_height = UI_TEXT_LINE_HEIGHT,
        small = UI_TEXT_SMALL,
        large = UI_TEXT_SMALL * 2.0,
        pair_size = UI_TEXT_PAIR_SIZE,
        pair_line = UI_TEXT_PAIR_LINE_HEIGHT,
        centred_width = UI_TEXT_CENTRED_WIDTH,
        centred_fill = hex(UI_TEXT_CENTRED_FILL),
        centred_size = UI_TEXT_CENTRED_SIZE,
    )
}

/// Where every part of the scene was laid out, in screen pixels, each as the
/// `(min, max)` of its border box.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct UiTextLayout {
    /// The page, which fills the frame.
    pub page: (Vec2, Vec2),
    /// The paragraph block.
    pub paragraph: (Vec2, Vec2),
    /// The word at the small size.
    pub small: (Vec2, Vec2),
    /// The word at twice it.
    pub large: (Vec2, Vec2),
    /// The kerned pair's span.
    pub kerned: (Vec2, Vec2),
    /// The block the unkerned pair is drawn in, from its top-left.
    pub unkerned: (Vec2, Vec2),
    /// The centred block.
    pub centred: (Vec2, Vec2),
}

/// Lays the scene out and emits the tree into `list`.
fn build(extent: (u32, u32), list: &mut DrawList) -> UiTextLayout {
    let atlas = FontAtlas::built_in();
    let mut ui = Ui::new();
    ui.add_stylesheet("ui_text.css", &ui_text_css());
    ui.begin_frame(PointerInput::default());
    let mut keys: Vec<NodeKey> = Vec::new();
    let page = ui.block("#page", &[], |ui| {
        keys.push(
            ui.block("#paragraph", &[], |ui| {
                ui.span("", UI_TEXT_PARAGRAPH, &[]);
            })
            .key,
        );
        ui.block("#sizes", &[], |ui| {
            keys.push(ui.span(".small", UI_TEXT_SIZES_WORD, &[]).key);
            keys.push(ui.span(".large", UI_TEXT_SIZES_WORD, &[]).key);
        });
        ui.block("#pairs", &[], |ui| {
            keys.push(ui.span(".pair", UI_TEXT_PAIR, &[]).key);
            keys.push(ui.block("#unkerned", &[], |_| {}).key);
        });
        keys.push(
            ui.block("#centred", &[], |ui| {
                ui.span("", UI_TEXT_CENTRED, &[]);
            })
            .key,
        );
    });
    ui.layout(
        Vec2::ZERO,
        AvailableSpace::definite(Vec2::new(extent.0 as f32, extent.1 as f32)),
        &atlas,
    );
    ui.emit(list);

    let rect = |key| ui.rect(key).expect("every reported part was laid out");
    let layout = UiTextLayout {
        page: rect(page.key),
        paragraph: rect(keys[0]),
        small: rect(keys[1]),
        large: rect(keys[2]),
        kerned: rect(keys[3]),
        unkerned: rect(keys[4]),
        centred: rect(keys[5]),
    };
    // In the colour the sheet gave the kerned pair.
    let ink = list
        .commands()
        .iter()
        .find_map(|command| match command {
            DrawCommand::Glyphs { origin, color, .. } if *origin == layout.kerned.0 => Some(*color),
            _ => None,
        })
        .expect("the kerned pair drew a glyph run");
    let (origin, glyphs) = unkerned_pair(layout.unkerned.0);
    list.glyphs(origin, Font::sans(), UI_TEXT_PAIR_SIZE, ink, glyphs);
    layout
}

/// The pair's glyphs from `origin`, each placed by the advances before it and
/// nothing else, on the baseline the tree gives the kerned pair.
fn unkerned_pair(origin: Vec2) -> (Vec2, Vec<PositionedGlyph>) {
    let font = Font::sans();
    let kerned = TextLayout::new(
        font,
        UI_TEXT_PAIR,
        UI_TEXT_PAIR_SIZE,
        UI_TEXT_PAIR_LINE_HEIGHT,
        None,
    );
    let baseline = kerned.lines()[0].baseline;
    let scale = font.metrics().scale(UI_TEXT_PAIR_SIZE);
    let mut pen = 0.0;
    let glyphs = UI_TEXT_PAIR
        .chars()
        .map(|c| {
            let glyph = font.glyph_id(c);
            let placed = PositionedGlyph {
                glyph,
                offset: Vec2::new(pen, baseline),
            };
            pen += font.advance(glyph) * scale;
            placed
        })
        .collect();
    (origin, glyphs)
}

/// The layout of [`Scene::UiText`](super::Scene::UiText) in an `extent`-sized
/// frame.
#[must_use]
pub fn ui_text_layout(extent: (u32, u32)) -> UiTextLayout {
    build(extent, &mut DrawList::new())
}

/// The scene's draw list for an `extent`-sized frame.
#[must_use]
pub fn ui_text_draw_list(extent: (u32, u32)) -> DrawList {
    let mut list = DrawList::new();
    build(extent, &mut list);
    list
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The extent every UI golden is blessed at.
    const EXTENT: (u32, u32) = (256, 192);

    /// The scene is what its claims need before any pixel is read: the sheet
    /// parses cleanly, every part is on the frame, the paragraph wraps into at
    /// least three lines with no word wider than the block, the pair's kerning
    /// is at least two pixels, the centred word's outer bearings match, and
    /// every text node drew as a glyph run.
    #[test]
    fn the_scene_is_laid_out_as_its_claims_need() {
        let logs = crcbl_core::log::capture();
        let list = ui_text_draw_list(EXTENT);
        assert!(logs.records().is_empty(), "{:#?}", logs.records());
        let layout = ui_text_layout(EXTENT);
        assert_eq!(
            layout.page,
            (Vec2::ZERO, Vec2::new(EXTENT.0 as f32, EXTENT.1 as f32)),
            "the page does not fill the frame"
        );
        for (name, (min, max)) in [
            ("paragraph", layout.paragraph),
            ("small", layout.small),
            ("large", layout.large),
            ("kerned", layout.kerned),
            ("unkerned", layout.unkerned),
            ("centred", layout.centred),
        ] {
            assert!(
                min.cmpge(Vec2::ZERO).all() && max.x <= EXTENT.0 as f32 && max.y <= EXTENT.1 as f32,
                "{name} is off the frame: {min} {max}"
            );
        }

        let font = Font::sans();
        let paragraph = TextLayout::new(
            font,
            UI_TEXT_PARAGRAPH,
            UI_TEXT_PARAGRAPH_SIZE,
            UI_TEXT_LINE_HEIGHT,
            Some(UI_TEXT_PARAGRAPH_WIDTH),
        );
        assert!(paragraph.lines().len() >= 3, "{:?}", paragraph.lines());
        assert!(
            paragraph.width() <= UI_TEXT_PARAGRAPH_WIDTH,
            "a word overflows"
        );
        assert_eq!(
            layout.paragraph.1.y - layout.paragraph.0.y,
            paragraph.lines().len() as f32 * UI_TEXT_LINE_HEIGHT
        );

        let (t, o) = (font.glyph_id('T'), font.glyph_id('o'));
        let kerning = -font.kerning(t, o) * font.metrics().scale(UI_TEXT_PAIR_SIZE);
        assert!(kerning >= 2.0, "the pair kerns by only {kerning}px");

        // The outer bearings as the atlas rasterises them: the first glyph's
        // ink from its pen, and the last glyph's from its advance.
        let mut atlas = crate::ui::font::atlas::GlyphAtlas::default();
        atlas.begin_frame();
        let first = font.glyph_id(UI_TEXT_CENTRED.chars().next().expect("a word"));
        let last = font.glyph_id(UI_TEXT_CENTRED.chars().last().expect("a word"));
        let left = atlas
            .glyph(font, first, UI_TEXT_CENTRED_SIZE, 0)
            .expect("room")
            .left;
        let placed = atlas
            .glyph(font, last, UI_TEXT_CENTRED_SIZE, 0)
            .expect("room");
        let advance = font.advance(last) * font.metrics().scale(UI_TEXT_CENTRED_SIZE);
        let right = advance - (placed.left + placed.width as i32) as f32;
        assert!(
            (left as f32 - right).abs() < 1.0,
            "the centred word's bearings differ: {left} left, {right} right"
        );

        let runs = list
            .commands()
            .iter()
            .filter(|command| matches!(command, DrawCommand::Glyphs { .. }))
            .count();
        assert_eq!(runs, 6, "five text spans and the unkerned pair");
        assert!(
            !list
                .commands()
                .iter()
                .any(|command| matches!(command, DrawCommand::Text { .. })),
            "a span drew in the bitmap font"
        );
    }
}
