//! Immediate-mode UI widgets: label, button, and helpers.
//!
//! Widgets are stateless on their own — each frame you call their `render`
//! method with the current state (hovered, clicked) and they push draw
//! commands into a [`DrawList`].

use crate::draw_list::DrawList;
use crate::text::FontAtlas;
use glam::Vec2;

// ---------------------------------------------------------------------------
// Style
// ---------------------------------------------------------------------------

/// Colour palette for a widget theme.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Style {
    /// Normal text colour.
    pub text: [f32; 4],
    /// Normal background colour.
    pub bg: [f32; 4],
    /// Background colour when hovered.
    pub bg_hover: [f32; 4],
    /// Background colour when pressed/active.
    pub bg_active: [f32; 4],
    /// Outline/border colour.
    pub border: [f32; 4],
}

impl Default for Style {
    fn default() -> Self {
        Self {
            text: [1.0, 1.0, 1.0, 1.0],        // white
            bg: [0.15, 0.15, 0.15, 0.9],       // dark grey
            bg_hover: [0.25, 0.25, 0.3, 0.9],  // slightly lighter
            bg_active: [0.35, 0.35, 0.4, 0.9], // lighter still
            border: [0.4, 0.4, 0.4, 1.0],      // grey
        }
    }
}

// ---------------------------------------------------------------------------
// Label
// ---------------------------------------------------------------------------

/// A static text label.
///
/// Renders a single line of text at the given position using the built-in
/// font atlas. Optionally draws a background rectangle behind the text.
#[derive(Debug, Clone)]
pub struct Label {
    /// Text content.
    pub text: String,
    /// Font size in pixels.
    pub font_size: f32,
    /// If true, draw a background rect behind the text.
    pub show_bg: bool,
}

impl Label {
    /// Create a new label.
    #[must_use]
    pub fn new(text: impl Into<String>) -> Self {
        Self {
            text: text.into(),
            font_size: 14.0,
            show_bg: false,
        }
    }

    /// Set the font size.
    #[must_use]
    pub fn with_size(mut self, size: f32) -> Self {
        self.font_size = size;
        self
    }

    /// Enable or disable the background rectangle.
    #[must_use]
    pub fn with_bg(mut self, show: bool) -> Self {
        self.show_bg = show;
        self
    }

    /// Render this label into a `DrawList`.
    pub fn render(&self, dl: &mut DrawList, pos: Vec2, atlas: &FontAtlas, style: &Style) {
        let scale = self.font_size / 14.0; // 14px is the "natural" size
        let w = atlas.text_width(&self.text, scale);

        if self.show_bg {
            let h = scale * crate::text::LINE_HEIGHT;
            dl.rect(pos, Vec2::new(pos.x + w + 4.0, pos.y + h), style.bg);
        }

        dl.text(pos, &self.text, style.text, self.font_size);
    }

    /// Measure the rendered width.
    #[must_use]
    pub fn width(&self, atlas: &FontAtlas) -> f32 {
        let scale = self.font_size / 14.0;
        atlas.text_width(&self.text, scale) + 4.0
    }

    /// Measure the rendered height.
    #[must_use]
    pub fn height(&self) -> f32 {
        let scale = self.font_size / 14.0;
        scale * crate::text::LINE_HEIGHT
    }
}

// ---------------------------------------------------------------------------
// Button
// ---------------------------------------------------------------------------

/// Button interaction state (computed externally from mouse input).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ButtonState {
    /// Default idle appearance.
    Idle,
    /// Mouse is hovering over the button.
    Hovered,
    /// Mouse button is pressed down on the button.
    Pressed,
}

/// A clickable button with text.
///
/// Button is stateless — you provide the [`ButtonState`] each frame based
/// on your input system (mouse position, click events). The button draws
/// itself and you inspect `was_clicked` to detect a press.
#[derive(Debug, Clone)]
pub struct Button {
    /// Button label.
    pub text: String,
    /// Font size.
    pub font_size: f32,
    /// Padding inside the button (horizontal, vertical).
    pub padding: Vec2,
}

impl Button {
    /// Create a new button with the given text.
    #[must_use]
    pub fn new(text: impl Into<String>) -> Self {
        Self {
            text: text.into(),
            font_size: 14.0,
            padding: Vec2::new(8.0, 4.0),
        }
    }

    /// The bounding rectangle for this button at the given position.
    #[must_use]
    pub fn rect(&self, pos: Vec2, atlas: &FontAtlas) -> (Vec2, Vec2) {
        let scale = self.font_size / 14.0;
        let text_w = atlas.text_width(&self.text, scale);
        let text_h = scale * crate::text::LINE_HEIGHT;
        let min = Vec2::new(pos.x, pos.y);
        let max = Vec2::new(
            pos.x + text_w + self.padding.x * 2.0,
            pos.y + text_h + self.padding.y * 2.0,
        );
        (min, max)
    }

    /// Whether the given mouse position is inside the button area.
    #[must_use]
    pub fn hit_test(&self, pos: Vec2, mouse: Vec2, atlas: &FontAtlas) -> bool {
        let (min, max) = self.rect(pos, atlas);
        mouse.x >= min.x && mouse.x <= max.x && mouse.y >= min.y && mouse.y <= max.y
    }

    /// Render the button into a `DrawList`.
    ///
    /// Returns `true` if the button was just clicked (transitioned from pressed
    /// to released while hovered).
    #[must_use]
    pub fn render(
        &self,
        dl: &mut DrawList,
        pos: Vec2,
        atlas: &FontAtlas,
        style: &Style,
        state: ButtonState,
        was_released: bool,
    ) -> bool {
        let (min, max) = self.rect(pos, atlas);
        let bg = match state {
            ButtonState::Pressed => style.bg_active,
            ButtonState::Hovered => style.bg_hover,
            ButtonState::Idle => style.bg,
        };

        dl.rect(min, max, bg);
        dl.rect_outline(min, max, 1.0, style.border);

        let scale = self.font_size / 14.0;
        let text_pos = Vec2::new(pos.x + self.padding.x, pos.y + self.padding.y + scale * 1.0);
        dl.text(text_pos, &self.text, style.text, self.font_size);

        was_released && state == ButtonState::Hovered
    }
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use crate::draw_list::DrawCommand;

    fn atlas() -> FontAtlas {
        FontAtlas::built_in()
    }

    fn style() -> Style {
        Style::default()
    }

    // ── Label ─────────────────────────────────────────────────────────────

    #[test]
    fn label_renders_text_command() {
        let label = Label::new("Hello");
        let mut dl = DrawList::new();
        label.render(&mut dl, Vec2::ZERO, &atlas(), &style());
        assert!(!dl.is_empty());
        assert!(matches!(dl.commands()[0], DrawCommand::Text { .. }));
    }

    #[test]
    fn label_with_bg_adds_rect() {
        let label = Label::new("Hi").with_bg(true);
        let mut dl = DrawList::new();
        label.render(&mut dl, Vec2::ZERO, &atlas(), &style());
        assert_eq!(dl.len(), 2);
        assert!(matches!(dl.commands()[0], DrawCommand::Rect { .. }));
        assert!(matches!(dl.commands()[1], DrawCommand::Text { .. }));
    }

    #[test]
    fn label_width_and_height_are_positive() {
        let label = Label::new("Hello");
        assert!(label.width(&atlas()) > 0.0);
        assert!(label.height() > 0.0);
    }

    // ── Button ────────────────────────────────────────────────────────────

    #[test]
    fn button_in_idle_state_renders_rect_and_text() {
        let btn = Button::new("Click");
        let mut dl = DrawList::new();
        let _ = btn.render(
            &mut dl,
            Vec2::ZERO,
            &atlas(),
            &style(),
            ButtonState::Idle,
            false,
        );
        assert_eq!(dl.len(), 3);
    }

    #[test]
    fn button_hit_test_inside() {
        let btn = Button::new("Go");
        let pos = Vec2::new(10.0, 10.0);
        let (min, max) = btn.rect(pos, &atlas());
        let centre = (min + max) * 0.5;
        assert!(btn.hit_test(pos, centre, &atlas()));
    }

    #[test]
    fn button_hit_test_outside() {
        let btn = Button::new("Go");
        let pos = Vec2::new(10.0, 10.0);
        assert!(!btn.hit_test(pos, Vec2::new(0.0, 0.0), &atlas()));
    }

    #[test]
    fn button_click_detected() {
        let btn = Button::new("Fire");
        let mut dl = DrawList::new();
        let clicked = btn.render(
            &mut dl,
            Vec2::ZERO,
            &atlas(),
            &style(),
            ButtonState::Hovered,
            true, // was_released
        );
        assert!(clicked);
    }

    #[test]
    fn button_no_click_when_not_hovered() {
        let btn = Button::new("Fire");
        let mut dl = DrawList::new();
        let clicked = btn.render(
            &mut dl,
            Vec2::ZERO,
            &atlas(),
            &style(),
            ButtonState::Idle,
            true, // was_released
        );
        assert!(!clicked);
    }

    #[test]
    fn button_rect_grows_with_text() {
        let short = Button::new("A");
        let long = Button::new("LongButton");
        let (_, short_max) = short.rect(Vec2::ZERO, &atlas());
        let (_, long_max) = long.rect(Vec2::ZERO, &atlas());
        assert!(long_max.x > short_max.x);
    }

    #[test]
    fn button_different_styles_affect_bg_color() {
        let btn = Button::new("Test");
        let mut dl_hover = DrawList::new();
        let _hc = btn.render(
            &mut dl_hover,
            Vec2::ZERO,
            &atlas(),
            &style(),
            ButtonState::Hovered,
            false,
        );

        let mut dl_idle = DrawList::new();
        let _ic = btn.render(
            &mut dl_idle,
            Vec2::ZERO,
            &atlas(),
            &style(),
            ButtonState::Idle,
            false,
        );

        // Both should have a Rect command; colors differ.
        if let DrawCommand::Rect { color: c1, .. } = &dl_hover.commands()[0] {
            if let DrawCommand::Rect { color: c2, .. } = &dl_idle.commands()[0] {
                assert_ne!(
                    c1, c2,
                    "hover and idle should have different background colors"
                );
            } else {
                panic!("second command not Rect");
            }
        } else {
            panic!("first command not Rect");
        }
    }
}
