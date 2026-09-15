//! [`Scene::UiStyle`](super::Scene::UiStyle)'s content: a themed panel styled
//! entirely by a stylesheet — `docs/plan/07-ui-debug.md` rung 4 — with one of
//! its buttons hovered by the scene's own pointer, so that what the cascade
//! promises can be read back off the frame.
//!
//! # The layout, and what each part of it is for
//!
//! ```text
//!   ┌─ #panel ──────────────┐   the panel: `var(--panel)` filled, a
//!   │ ┏━ #accept.button ━━┓ │   `var(--accent)` border
//!   │ ┃ ACCEPT     ▲      ┃ │   ← hovered: `.button:hover` fills it, and its
//!   │ ┗━━━━━━━━━━━━┃━━━━━━┛ │     id rule's border beats the later class rule
//!   │      gap     pointer  │
//!   │ ┏━ #cancel.button ━━┓ │   ← not hovered: the class rule's fill and
//!   │ ┃ CANCEL            ┃ │     border
//!   │ ┗━━━━━━━━━━━━━━━━━━━┛ │
//!   └───────────────────────┘
//! ```
//!
//! * **The panel's fill and border** come through custom properties declared
//!   on the theme block above it.
//! * **The two buttons** match the same class rules and differ only by the
//!   hover and by the id rule, which sits earlier in the sheet than the class
//!   rule it must beat.
//! * **The two labels** set no colour of their own: `ACCEPT` inherits the
//!   theme's, and `CANCEL`'s `.muted` rule names a variable nothing defines, so
//!   its fallback is what draws.
//! * **The gap and the button heights** are the sheet's, and the hovered button
//!   is exactly as tall as the other: the hover is paint.
//!
//! Every colour the frame is measured against is a constant here, and the
//! stylesheet is written from those constants, so the two cannot drift. The
//! sheet's lengths are not scaled with the frame: at any `--size` the panel is
//! drawn at these sizes from the top-left corner.

use glam::Vec2;

use crate::ui::draw_list::DrawList;
use crate::ui::style::StyleStats;
use crate::ui::text::FontAtlas;
use crate::ui::tree::{AvailableSpace, NodeKey, Ui};
use crate::ui::widget::PointerInput;

/// The panel's fill, as the stylesheet writes it: sRGB bytes.
pub const UI_STYLE_PANEL: [u8; 3] = [0x1f, 0x2a, 0x44];

/// The panel's border.
pub const UI_STYLE_ACCENT: [u8; 3] = [0xe0, 0xa0, 0x30];

/// A button's fill.
pub const UI_STYLE_BUTTON: [u8; 3] = [0x40, 0x50, 0x70];

/// A hovered button's fill.
pub const UI_STYLE_HOVER: [u8; 3] = [0x30, 0xc0, 0x60];

/// A button's border, from the class rule.
pub const UI_STYLE_BUTTON_BORDER: [u8; 3] = [0x00, 0xd0, 0xd0];

/// `#accept`'s border, from the id rule.
pub const UI_STYLE_ACCEPT_BORDER: [u8; 3] = [0xd0, 0x00, 0xd0];

/// The theme's text colour, which `ACCEPT` inherits.
pub const UI_STYLE_TEXT: [u8; 3] = [0xf0, 0xf0, 0xf0];

/// `.muted`'s fallback colour, which `CANCEL` draws in.
pub const UI_STYLE_MUTED: [u8; 3] = [0x90, 0x60, 0x40];

/// The gap between the two buttons, in pixels.
pub const UI_STYLE_GAP: f32 = 8.0;

/// A button's border-box height, in pixels.
pub const UI_STYLE_BUTTON_HEIGHT: f32 = 32.0;

/// A button's border width, in pixels.
pub const UI_STYLE_BUTTON_BORDER_WIDTH: f32 = 3.0;

/// The panel's border width, in pixels.
pub const UI_STYLE_PANEL_BORDER_WIDTH: f32 = 2.0;

fn hex([r, g, b]: [u8; 3]) -> String {
    format!("#{r:02x}{g:02x}{b:02x}")
}

/// The scene's stylesheet, written from the constants above.
#[must_use]
pub fn ui_style_css() -> String {
    format!(
        "\
.theme {{
  --panel: {panel};
  --accent: {accent};
  --hover: {hover};
  padding: 16px;
  color: {text};
}}
#panel {{
  flex-direction: column;
  width: 160px;
  padding: 12px;
  gap: {gap}px;
  background: var(--panel);
  border-width: {panel_border}px;
  border-color: var(--accent);
}}
#accept {{
  border-color: {accept_border};
}}
.button {{
  height: {button_height}px;
  padding: 8px 10px;
  background: {button};
  border-width: {button_border_width}px;
  border-color: {button_border};
}}
.button:hover {{
  background: var(--hover);
}}
.muted {{
  color: var(--undefined-in-this-theme, {muted});
}}
",
        panel = hex(UI_STYLE_PANEL),
        accent = hex(UI_STYLE_ACCENT),
        hover = hex(UI_STYLE_HOVER),
        text = hex(UI_STYLE_TEXT),
        gap = UI_STYLE_GAP,
        panel_border = UI_STYLE_PANEL_BORDER_WIDTH,
        accept_border = hex(UI_STYLE_ACCEPT_BORDER),
        button_height = UI_STYLE_BUTTON_HEIGHT,
        button = hex(UI_STYLE_BUTTON),
        button_border_width = UI_STYLE_BUTTON_BORDER_WIDTH,
        button_border = hex(UI_STYLE_BUTTON_BORDER),
        muted = hex(UI_STYLE_MUTED),
    )
}

/// Where every part of the scene was laid out, in screen pixels, each as the
/// `(min, max)` of its border box, and where the pointer was.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct UiStyleLayout {
    /// The themed panel.
    pub panel: (Vec2, Vec2),
    /// The hovered button.
    pub accept: (Vec2, Vec2),
    /// The other button.
    pub cancel: (Vec2, Vec2),
    /// The hovered button's label.
    pub accept_label: (Vec2, Vec2),
    /// The other button's label.
    pub cancel_label: (Vec2, Vec2),
    /// Where the scene's pointer hovers: the middle of `accept` as the first
    /// frame laid it out.
    pub pointer: Vec2,
}

struct Parts {
    panel: NodeKey,
    accept: NodeKey,
    cancel: NodeKey,
    accept_label: NodeKey,
    cancel_label: NodeKey,
}

fn frame(ui: &mut Ui, pointer: PointerInput, extent: (u32, u32), atlas: &FontAtlas) -> Parts {
    ui.begin_frame(pointer);
    let mut keys = Vec::new();
    ui.block(".theme", &[], |ui| {
        let panel = ui.block("#panel", &[], |ui| {
            let accept = ui.block("#accept.button", &[], |ui| {
                keys.push(ui.span(".label", "ACCEPT", &[]).key);
            });
            keys.push(accept.key);
            let cancel = ui.block("#cancel.button", &[], |ui| {
                keys.push(ui.span(".label.muted", "CANCEL", &[]).key);
            });
            keys.push(cancel.key);
        });
        keys.push(panel.key);
    });
    ui.layout(
        Vec2::ZERO,
        AvailableSpace::definite(Vec2::new(extent.0 as f32, extent.1 as f32)),
        atlas,
    );
    let [accept_label, accept, cancel_label, cancel, panel] = keys[..] else {
        unreachable!("the scene builds exactly five reported parts");
    };
    Parts {
        panel,
        accept,
        cancel,
        accept_label,
        cancel_label,
    }
}

/// Builds the scene's two frames — one to lay the panel out, one with the
/// pointer on `#accept` — and returns the tree after the second, what style
/// resolution did in it, and the layout.
fn build(extent: (u32, u32)) -> (Ui, StyleStats, UiStyleLayout) {
    let atlas = FontAtlas::built_in();
    let mut ui = Ui::new();
    ui.add_stylesheet("ui_style.css", &ui_style_css());
    let first = frame(
        &mut ui,
        PointerInput::hovering(Vec2::splat(-1.0)),
        extent,
        &atlas,
    );
    let (min, max) = ui.rect(first.accept).expect("laid out");
    let pointer = (min + max) * 0.5;
    let parts = frame(&mut ui, PointerInput::hovering(pointer), extent, &atlas);
    let stats = ui.style_stats();
    let rect = |key| ui.rect(key).expect("every reported part was laid out");
    let layout = UiStyleLayout {
        panel: rect(parts.panel),
        accept: rect(parts.accept),
        cancel: rect(parts.cancel),
        accept_label: rect(parts.accept_label),
        cancel_label: rect(parts.cancel_label),
        pointer,
    };
    (ui, stats, layout)
}

/// The layout of [`Scene::UiStyle`](super::Scene::UiStyle) in an
/// `extent`-sized frame, as its hovered frame laid it out.
#[must_use]
pub fn ui_style_layout(extent: (u32, u32)) -> UiStyleLayout {
    build(extent).2
}

/// The scene's draw list for an `extent`-sized frame: the hovered frame.
#[must_use]
pub fn ui_style_draw_list(extent: (u32, u32)) -> DrawList {
    let (ui, _, _) = build(extent);
    let mut list = DrawList::new();
    ui.emit(&mut list);
    list
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ui::draw_list::DrawCommand;

    /// The extent every UI golden is blessed at.
    const EXTENT: (u32, u32) = (256, 192);

    /// The scene is what its claims need before any pixel is read: the pointer
    /// is on `#accept` and off `#cancel`, the hover moved no layout, it
    /// re-resolved the one node whose rules test it, the stylesheet parsed
    /// cleanly, and the hovered fill is the only difference between the two
    /// buttons' fills.
    #[test]
    fn the_scene_is_laid_out_and_styled_as_its_claims_need() {
        let logs = crcbl_core::log::capture();
        let (_, stats, layout) = build(EXTENT);
        assert!(
            logs.records().is_empty(),
            "the scene's stylesheet reported {:#?}",
            logs.records()
        );
        let inside = |(min, max): (Vec2, Vec2), point: Vec2| {
            point.cmpge(min).all() && point.cmplt(max).all()
        };
        assert!(inside(layout.accept, layout.pointer));
        assert!(!inside(layout.cancel, layout.pointer));
        assert_eq!(stats.resolves, 1, "the hover re-resolved more than #accept");

        let atlas = FontAtlas::built_in();
        let mut unhovered = Ui::new();
        unhovered.add_stylesheet("ui_style.css", &ui_style_css());
        let parts = frame(
            &mut unhovered,
            PointerInput::hovering(Vec2::splat(-1.0)),
            EXTENT,
            &atlas,
        );
        assert_eq!(
            unhovered.rect(parts.accept),
            Some(layout.accept),
            "the hover moved #accept"
        );
        assert_eq!(
            unhovered.rect(parts.cancel),
            Some(layout.cancel),
            "the hover moved #cancel"
        );

        let gap = layout.cancel.0.y - layout.accept.1.y;
        assert_eq!(gap, UI_STYLE_GAP);
        assert_eq!(
            layout.accept.1.y - layout.accept.0.y,
            UI_STYLE_BUTTON_HEIGHT
        );

        let fills: Vec<[f32; 4]> = ui_style_draw_list(EXTENT)
            .commands()
            .iter()
            .filter_map(|command| match command {
                DrawCommand::Rect { min, color, .. }
                    if *min == layout.accept.0 || *min == layout.cancel.0 =>
                {
                    Some(*color)
                }
                _ => None,
            })
            .collect();
        assert_eq!(fills.len(), 2, "each button draws one fill: {fills:?}");
        assert_ne!(fills[0], fills[1], "the hovered fill is the unhovered one");
    }
}
