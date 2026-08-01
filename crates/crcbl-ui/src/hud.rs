//! HUD skeleton: a simple overlay that positions and renders widgets.
//!
//! The HUD is composed of panels (anchored regions of the screen) that
//! contain widgets. In this first slice it provides a top-anchored info
//! bar and a debug overlay region.

use crate::draw_list::DrawList;
use crate::text::FontAtlas;
use crate::widget::{Button, Label, PointerInput, Style, UiState, WidgetId};
use glam::Vec2;

// ---------------------------------------------------------------------------
// HudPanel
// ---------------------------------------------------------------------------

/// Which corner (or the centre) of the screen a panel is positioned against.
///
/// In every arm the panel's `offset` is an **inset from that anchor**, and the
/// panel's content grows away from it: a `TopRight` panel's right edge sits
/// `offset.x` in from the right of the screen. `Center` centres the panel and
/// then applies `offset` as a nudge.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Anchor {
    /// Inset from the top-left corner.
    TopLeft,
    /// Inset from the top-right corner.
    TopRight,
    /// Inset from the bottom-left corner.
    BottomLeft,
    /// Inset from the bottom-right corner.
    BottomRight,
    /// Centred, then nudged by `offset`.
    Center,
}

/// A positioned panel in the HUD.
#[derive(Debug, Clone)]
pub struct HudPanel {
    /// Which screen corner this panel is inset from.
    pub anchor: Anchor,
    /// Inset from the anchor, in pixels.
    pub offset: Vec2,
    /// Labels, stacked vertically from the panel's top-left.
    pub labels: Vec<Label>,
    /// Buttons, stacked vertically below the labels.
    pub buttons: Vec<Button>,
    /// Whether to draw a semi-transparent background rect behind the panel.
    pub show_bg: bool,
    /// Namespaces this panel's widget ids inside a [`UiState`], so two panels'
    /// buttons cannot collide. [`Hud::add_panel`] assigns the panel's index;
    /// a panel rendered on its own can keep the default.
    pub id: u64,
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
            id: 0,
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
            w = w.max(label.width(atlas));
            h += label.height_for(atlas) + 2.0;
        }
        // Buttons also stack vertically below labels.
        for btn in &self.buttons {
            let (min, max) = btn.rect(Vec2::ZERO, atlas);
            w = w.max(max.x - min.x);
            h += (max.y - min.y) + 2.0;
        }
        // Remove trailing spacing.
        if !self.labels.is_empty() || !self.buttons.is_empty() {
            h -= 2.0;
        }
        Vec2::new(w, h)
    }

    /// Render all widgets in this panel into the draw list.
    ///
    /// `screen_size` is used to compute the anchor position; widget ids are
    /// namespaced by [`HudPanel::id`].
    ///
    /// Returns the index of the button that was clicked this frame, or `None`.
    /// A click requires the press to have *started* on that button — see
    /// [`UiState`].
    pub fn render(
        &self,
        dl: &mut DrawList,
        screen_size: Vec2,
        atlas: &FontAtlas,
        style: &Style,
        ui: &mut UiState,
        pointer: PointerInput,
    ) -> Option<usize> {
        let content = self.content_size(atlas);
        let anchor = self.anchor_pos(screen_size, content);

        // Optional panel background.
        if self.show_bg && content.x > 0.0 && content.y > 0.0 {
            let pad = 4.0;
            let min = anchor - Vec2::splat(pad);
            let max = anchor + content + Vec2::splat(pad);
            dl.rect(min, max, style.bg);
        }

        let mut cursor = anchor;
        let mut clicked = None;

        for label in &self.labels {
            label.render(dl, cursor, atlas, style);
            cursor.y += label.height_for(atlas) + 2.0;
        }

        for (i, btn) in self.buttons.iter().enumerate() {
            let (state, was_clicked) =
                btn.interact(cursor, atlas, ui, widget_id(self.id, i), pointer);
            if was_clicked {
                clicked = Some(i);
            }
            btn.render(dl, cursor, atlas, style, state);
            let (_, max) = btn.rect(cursor, atlas);
            cursor.y = max.y + 2.0;
        }

        clicked
    }

    /// Compute the top-left pixel position of this panel from its anchor and
    /// the extent of its content.
    ///
    /// The content extent is needed because `offset` is an *inset*: a
    /// right-anchored panel's right edge is `offset.x` in from the right of the
    /// screen, so its left edge — what this returns — depends on how wide it
    /// is. Returning `screen.x - offset.x` as the left edge, as this used to,
    /// runs a right-anchored panel straight off the screen.
    fn anchor_pos(&self, screen_size: Vec2, content: Vec2) -> Vec2 {
        let far = screen_size - self.offset - content;
        match self.anchor {
            Anchor::TopLeft => self.offset,
            Anchor::TopRight => Vec2::new(far.x, self.offset.y),
            Anchor::BottomLeft => Vec2::new(self.offset.x, far.y),
            Anchor::BottomRight => far,
            Anchor::Center => (screen_size - content) * 0.5 + self.offset,
        }
    }
}

/// A widget id unique to `(panel, index)`.
///
/// The panel id occupies the high 32 bits, so panels stay distinct as long as a
/// single one has fewer than 2³² buttons.
fn widget_id(panel_id: u64, index: usize) -> WidgetId {
    (panel_id << 32) | (index as u64 & 0xffff_ffff)
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

    /// Add a panel, stamping it with an id unique within this HUD.
    pub fn add_panel(&mut self, mut panel: HudPanel) {
        panel.id = self.panels.len() as u64;
        self.panels.push(panel);
    }

    /// Render all panels into a draw list.
    ///
    /// Produces draw commands for every widget in every panel and returns
    /// `(panel index, button index)` for the button clicked this frame, if any.
    /// `ui` must be the same instance across frames — it holds the press
    /// capture that makes a click mean "pressed *and* released on this button".
    pub fn render(
        &self,
        dl: &mut DrawList,
        screen_size: Vec2,
        atlas: &FontAtlas,
        style: &Style,
        ui: &mut UiState,
        pointer: PointerInput,
    ) -> Option<(usize, usize)> {
        let mut clicked = None;
        for (p, panel) in self.panels.iter().enumerate() {
            if let Some(b) = panel.render(dl, screen_size, atlas, style, ui, pointer) {
                clicked = Some((p, b));
            }
        }
        clicked
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

    fn screen() -> Vec2 {
        Vec2::new(800.0, 600.0)
    }

    #[test]
    fn hud_renders_labels_and_buttons() {
        let hud = setup_hud();
        let mut dl = DrawList::new();
        let mut ui = UiState::new();
        hud.render(
            &mut dl,
            screen(),
            &FontAtlas::built_in(),
            &Style::default(),
            &mut ui,
            PointerInput::default(),
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
        let mut ui = UiState::new();
        let clicked = hud.render(
            &mut dl,
            screen(),
            &FontAtlas::built_in(),
            &Style::default(),
            &mut ui,
            PointerInput::default(),
        );
        assert!(dl.is_empty());
        assert!(clicked.is_none());
    }

    /// The bug: `offset` meant "inset" for `TopLeft` but "left edge" for
    /// `TopRight`, so a right-anchored panel ran off the screen, and `Center`
    /// halved the offset instead of applying it.
    #[test]
    fn every_anchor_insets_the_panel_by_offset() {
        let content = Vec2::new(100.0, 40.0);
        let offset = Vec2::splat(10.0);
        let size = screen();
        let at = |anchor| HudPanel::new(anchor, offset).anchor_pos(size, content);

        assert_eq!(at(Anchor::TopLeft), Vec2::new(10.0, 10.0));
        // Right edge sits 10px in from the right: 800 - 10 - 100 = 690.
        assert_eq!(at(Anchor::TopRight), Vec2::new(690.0, 10.0));
        // Bottom edge sits 10px up from the bottom: 600 - 10 - 40 = 550.
        assert_eq!(at(Anchor::BottomLeft), Vec2::new(10.0, 550.0));
        assert_eq!(at(Anchor::BottomRight), Vec2::new(690.0, 550.0));
        // Centred, then nudged: (800-100)/2 + 10, (600-40)/2 + 10.
        assert_eq!(at(Anchor::Center), Vec2::new(360.0, 290.0));
    }

    /// Every anchored panel must be fully on-screen for any offset that fits.
    #[test]
    fn anchored_panels_stay_on_screen() {
        let content = Vec2::new(120.0, 60.0);
        let size = screen();
        for anchor in [
            Anchor::TopLeft,
            Anchor::TopRight,
            Anchor::BottomLeft,
            Anchor::BottomRight,
        ] {
            let pos = HudPanel::new(anchor, Vec2::splat(8.0)).anchor_pos(size, content);
            assert!(pos.x >= 0.0 && pos.y >= 0.0, "{anchor:?} → {pos:?}");
            let far = pos + content;
            assert!(far.x <= size.x && far.y <= size.y, "{anchor:?} → {far:?}");
        }
    }

    /// The bug: `HudPanel::render` returned `()`, dropped `Button::render`'s
    /// `#[must_use]` bool with `let _ =`, and never called the closure it took;
    /// `Hud::render` hardcoded a cursor at the origin with no buttons down. A
    /// HUD button could not be clicked at all.
    #[test]
    fn a_hud_button_reports_its_click() {
        let atlas = FontAtlas::built_in();
        let hud = setup_hud();
        let mut ui = UiState::new();

        // Where the "Pause" button actually lands: below the one label.
        let panel = &hud.panels[0];
        let content = panel.content_size(&atlas);
        let anchor = panel.anchor_pos(screen(), content);
        let btn_pos = Vec2::new(
            anchor.x,
            anchor.y + panel.labels[0].height_for(&atlas) + 2.0,
        );
        let (min, max) = panel.buttons[0].rect(btn_pos, &atlas);
        let centre = (min + max) * 0.5;

        // Frame 1: press.
        let mut dl = DrawList::new();
        let clicked = hud.render(
            &mut dl,
            screen(),
            &atlas,
            &Style::default(),
            &mut ui,
            PointerInput {
                pos: centre,
                down: true,
                released: false,
            },
        );
        assert_eq!(clicked, None, "a press alone is not a click");
        assert!(ui.active().is_some(), "the press must capture the button");

        // Frame 2: release over the same button.
        let mut dl = DrawList::new();
        let clicked = hud.render(
            &mut dl,
            screen(),
            &atlas,
            &Style::default(),
            &mut ui,
            PointerInput {
                pos: centre,
                down: false,
                released: true,
            },
        );
        assert_eq!(clicked, Some((0, 0)), "panel 0's button 0 was clicked");
    }

    /// The bug: the state machine was `if hit { if released { Hovered } else
    /// { Idle } }`, so it produced `Hovered` only on a release frame and
    /// `Pressed` never — leaving `Style::bg_hover` and `bg_active` dead.
    #[test]
    fn hud_button_uses_hover_and_active_backgrounds() {
        let atlas = FontAtlas::built_in();
        let style = Style::default();
        let hud = setup_hud();
        let panel = &hud.panels[0];
        let anchor = panel.anchor_pos(screen(), panel.content_size(&atlas));
        let btn_pos = Vec2::new(
            anchor.x,
            anchor.y + panel.labels[0].height_for(&atlas) + 2.0,
        );
        let (min, max) = panel.buttons[0].rect(btn_pos, &atlas);
        let centre = (min + max) * 0.5;

        let bg_for = |pointer: PointerInput, ui: &mut UiState| {
            let mut dl = DrawList::new();
            hud.render(&mut dl, screen(), &atlas, &style, ui, pointer);
            // The button's own background is the first Rect after the label text.
            dl.commands()
                .iter()
                .filter_map(|c| match c {
                    DrawCommand::Rect { color, .. } => Some(*color),
                    _ => None,
                })
                .next_back()
                .expect("the button draws a background")
        };

        let mut ui = UiState::new();
        assert_eq!(
            bg_for(PointerInput::hovering(Vec2::new(-1.0, -1.0)), &mut ui),
            style.bg
        );
        assert_eq!(
            bg_for(PointerInput::hovering(centre), &mut ui),
            style.bg_hover
        );
        assert_eq!(
            bg_for(
                PointerInput {
                    pos: centre,
                    down: true,
                    released: false
                },
                &mut ui
            ),
            style.bg_active
        );
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
        let mut ui = UiState::new();
        panel.render(
            &mut dl,
            screen(),
            &FontAtlas::built_in(),
            &Style::default(),
            &mut ui,
            PointerInput::default(),
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
        let mut ui = UiState::new();
        hud.render(
            &mut dl,
            screen(),
            &FontAtlas::built_in(),
            &Style::default(),
            &mut ui,
            PointerInput::default(),
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
