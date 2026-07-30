//! HUD skeleton: a simple overlay that positions and renders widgets.
//!
//! The HUD is composed of panels (anchored regions of the screen) that
//! contain widgets. In this first slice it provides a top-anchored info
//! bar and a debug overlay region.

use crate::draw_list::DrawList;
use crate::text::FontAtlas;
use crate::widget::{Button, ButtonState, Label, Style};
use glam::Vec2;

// ---------------------------------------------------------------------------
// HudPanel
// ---------------------------------------------------------------------------

/// A screen region that holds HUD content.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Anchor {
    TopLeft,
    TopRight,
    BottomLeft,
    BottomRight,
    Center,
}

/// A positioned panel in the HUD.
#[derive(Debug, Clone)]
pub struct HudPanel {
    pub anchor: Anchor,
    pub offset: Vec2,
    pub labels: Vec<Label>,
    pub buttons: Vec<Button>,
    /// Whether to draw a semi-transparent background rect behind the panel.
    pub show_bg: bool,
}

impl HudPanel {
    /// Create a new panel at the given anchor with an offset.
    #[must_use]
    pub fn new(anchor: Anchor, offset: Vec2) -> Self {
        Self {
            anchor,
            offset,
            labels: Vec::new(),
            buttons: Vec::new(),
            show_bg: false,
        }
    }

    /// Draw a background rectangle behind this panel's content.
    #[must_use]
    pub fn with_bg(mut self) -> Self {
        self.show_bg = true;
        self
    }

    /// Add a label.
    pub fn add_label(&mut self, label: Label) {
        self.labels.push(label);
    }

    /// Add a button.
    pub fn add_button(&mut self, button: Button) {
        self.buttons.push(button);
    }

    /// Calculate the total content extent of this panel (width × height in
    /// pixels) given a font atlas. Used to size the background rectangle.
    #[must_use]
    pub fn content_size(&self, atlas: &FontAtlas) -> Vec2 {
        let mut w = 0.0f32;
        let mut h = 0.0f32;

        // Labels stack vertically.
        for label in &self.labels {
            let lw = label.width(atlas);
            w = w.max(lw);
            h += label.height() + 2.0;
        }
        // Buttons also stack vertically below labels.
        for btn in &self.buttons {
            let (_, max) = btn.rect(Vec2::ZERO, atlas);
            w = w.max(max.x);
            h += (max.y - 0.0) + 2.0;
        }
        // Remove trailing spacing.
        if !self.labels.is_empty() || !self.buttons.is_empty() {
            h -= 2.0;
        }
        Vec2::new(w, h)
    }

    /// Render all widgets in this panel into the draw list.
    ///
    /// `screen_size` is used to compute the anchor position.
    /// Returns the index of any button that was clicked, or `None`.
    #[allow(clippy::too_many_arguments)]
    pub fn render(
        &self,
        dl: &mut DrawList,
        screen_size: Vec2,
        atlas: &FontAtlas,
        style: &Style,
        mouse_pos: Vec2,
        mouse_released: bool,
        _button_clicked: impl Fn(usize, ButtonState) -> bool,
    ) {
        let anchor = self.anchor_pos(screen_size);

        // Optional panel background.
        if self.show_bg {
            let content = self.content_size(atlas);
            if content.x > 0.0 && content.y > 0.0 {
                let pad = 4.0;
                let min = Vec2::new(anchor.x - pad, anchor.y - pad);
                let max = Vec2::new(anchor.x + content.x + pad, anchor.y + content.y + pad);
                dl.rect(min, max, style.bg);
            }
        }

        let mut cursor = anchor;

        for label in &self.labels {
            label.render(dl, cursor, atlas, style);
            cursor.y += label.height() + 2.0;
        }

        for btn in &self.buttons {
            let state = if btn.hit_test(cursor, mouse_pos, atlas) {
                if mouse_released {
                    ButtonState::Hovered
                } else {
                    ButtonState::Idle
                }
            } else {
                ButtonState::Idle
            };
            let _ = btn.render(dl, cursor, atlas, style, state, mouse_released);
            let (_, max) = btn.rect(cursor, atlas);
            cursor.y = max.y + 2.0;
        }
    }

    /// Compute the top-left pixel position of this panel based on anchor.
    fn anchor_pos(&self, screen_size: Vec2) -> Vec2 {
        match self.anchor {
            Anchor::TopLeft => self.offset,
            Anchor::TopRight => Vec2::new(screen_size.x - self.offset.x, self.offset.y),
            Anchor::BottomLeft => Vec2::new(self.offset.x, screen_size.y - self.offset.y),
            Anchor::BottomRight => screen_size - self.offset,
            Anchor::Center => (screen_size - self.offset) * 0.5,
        }
    }
}

// ---------------------------------------------------------------------------
// Hud
// ---------------------------------------------------------------------------

/// Top-level HUD container.
///
/// Owns a collection of [`HudPanel`]s and renders them each frame.
/// The `Hud` is the public API for game code to add UI elements.
#[derive(Debug, Clone)]
pub struct Hud {
    pub panels: Vec<HudPanel>,
}

impl Hud {
    /// Create an empty HUD.
    #[must_use]
    pub fn new() -> Self {
        Self { panels: Vec::new() }
    }

    /// Add a panel.
    pub fn add_panel(&mut self, panel: HudPanel) {
        self.panels.push(panel);
    }

    /// Render all panels into a draw list.
    ///
    /// Produces draw commands for every widget in every panel.
    pub fn render(&self, dl: &mut DrawList, screen_size: Vec2, atlas: &FontAtlas, style: &Style) {
        for panel in &self.panels {
            panel.render(
                dl,
                screen_size,
                atlas,
                style,
                Vec2::ZERO,   // mouse_pos — no input integration yet
                false,        // mouse_released
                |_, _| false, // button_clicked
            );
        }
    }
}

impl Default for Hud {
    fn default() -> Self {
        Self::new()
    }
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use crate::draw_list::DrawCommand;

    fn setup_hud() -> Hud {
        let mut hud = Hud::new();
        let mut panel = HudPanel::new(Anchor::TopLeft, Vec2::new(10.0, 10.0));
        panel.add_label(Label::new("Score: 0").with_size(14.0));
        panel.add_button(Button::new("Pause"));
        hud.add_panel(panel);
        hud
    }

    #[test]
    fn hud_renders_labels_and_buttons() {
        let hud = setup_hud();
        let mut dl = DrawList::new();
        hud.render(
            &mut dl,
            Vec2::new(800.0, 600.0),
            &FontAtlas::built_in(),
            &Style::default(),
        );
        // Should have at least 2 commands (label text + button rect + button text)
        assert!(
            dl.len() >= 2,
            "expected at least 2 commands, got {}",
            dl.len()
        );
    }

    #[test]
    fn empty_hud_produces_no_commands() {
        let hud = Hud::new();
        let mut dl = DrawList::new();
        hud.render(
            &mut dl,
            Vec2::new(800.0, 600.0),
            &FontAtlas::built_in(),
            &Style::default(),
        );
        assert!(dl.is_empty());
    }

    #[test]
    fn anchor_positions_are_distinct() {
        let panel_tl = HudPanel::new(Anchor::TopLeft, Vec2::splat(10.0));
        let panel_tr = HudPanel::new(Anchor::TopRight, Vec2::splat(10.0));
        let panel_br = HudPanel::new(Anchor::BottomRight, Vec2::splat(10.0));
        let size = Vec2::new(800.0, 600.0);
        let tl = panel_tl.anchor_pos(size);
        let tr = panel_tr.anchor_pos(size);
        let br = panel_br.anchor_pos(size);
        assert_eq!(tl, Vec2::new(10.0, 10.0));
        assert_eq!(tr, Vec2::new(790.0, 10.0));
        assert_eq!(br, Vec2::new(790.0, 590.0));
    }

    #[test]
    fn debug_format() {
        let hud = setup_hud();
        let s = format!("{hud:?}");
        assert!(s.contains("panels"));
    }

    #[test]
    fn panel_with_bg_draws_background_rect() {
        let mut panel = HudPanel::new(Anchor::TopLeft, Vec2::new(10.0, 10.0)).with_bg();
        panel.add_label(Label::new("FPS: 60"));
        let mut dl = DrawList::new();
        panel.render(
            &mut dl,
            Vec2::new(800.0, 600.0),
            &FontAtlas::built_in(),
            &Style::default(),
            Vec2::ZERO,
            false,
            |_, _| false,
        );
        // First command is the panel background rect, then the label text.
        assert!(
            dl.len() >= 2,
            "expected bg rect + label text, got {}",
            dl.len()
        );
        assert!(matches!(dl.commands()[0], DrawCommand::Rect { .. }));
    }

    #[test]
    fn panel_content_size_measures_labels() {
        let mut panel = HudPanel::new(Anchor::TopLeft, Vec2::ZERO);
        panel.add_label(Label::new("Hello World"));
        panel.add_label(Label::new("Score: 9999"));
        let atlas = FontAtlas::built_in();
        let size = panel.content_size(&atlas);
        assert!(size.x > 0.0, "width should be positive");
        assert!(size.y > 0.0, "height should be positive");
    }

    #[test]
    fn hud_full_cycle_to_triangles() {
        let hud = setup_hud();
        let mut dl = DrawList::new();
        hud.render(
            &mut dl,
            Vec2::new(800.0, 600.0),
            &FontAtlas::built_in(),
            &Style::default(),
        );
        let (verts, indices) = dl.to_triangles(None, 1.0);
        // A label ("Score: 0") → text only (no rect since show_bg=false) → skipped
        // A button ("Pause") → 1 rect + 1 outline + text → 1 + 4 = 5 quads → 20 verts
        assert!(!verts.is_empty(), "button rect should triangulate");
        // Label text is skipped (no atlas → None) so only button geometry.
        assert_eq!(verts.len(), 20);
        assert_eq!(indices.len(), 30);
    }
}
