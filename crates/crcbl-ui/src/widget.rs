//! Immediate-mode UI widgets: label, button, and helpers.
//!
//! Widgets are stateless on their own — each frame you call their `render`
//! method with the current state (hovered, clicked) and they push draw
//! commands into a [`DrawList`].

use crate::draw_list::DrawList;
use crate::text::{FontAtlas, GLYPH_HEIGHT, LINE_HEIGHT};
use glam::Vec2;

/// The font size at which the built-in atlas renders 1:1.
///
/// Measurement and layout both divide by this. They used to disagree — widgets
/// divided by `14.0` while [`DrawList::to_triangles`] divided by
/// [`GLYPH_HEIGHT`] — which made every bound and hit rect ~7% smaller than the
/// glyphs it was supposed to contain.
///
/// [`DrawList::to_triangles`]: crate::draw_list::DrawList::to_triangles
pub const NATURAL_FONT_SIZE: f32 = GLYPH_HEIGHT as f32;

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

    /// The layout scale this label's glyphs are drawn at.
    fn scale(&self) -> f32 {
        self.font_size / NATURAL_FONT_SIZE
    }

    /// Render this label into a `DrawList`.
    ///
    /// `pos` is the top-left corner of the label, and is the same anchor the
    /// background rect and [`Label::width`]/[`Label::height`] describe.
    pub fn render(&self, dl: &mut DrawList, pos: Vec2, atlas: &FontAtlas, style: &Style) {
        if self.show_bg {
            let size = self.size(atlas);
            dl.rect(pos, pos + size, style.bg);
        }

        dl.text(pos, &self.text, style.text, self.font_size);
    }

    /// Measure the rendered width.
    #[must_use]
    pub fn width(&self, atlas: &FontAtlas) -> f32 {
        atlas.text_width(&self.text, self.scale()) + 4.0
    }

    /// Measure the rendered height, which grows with the number of lines.
    #[must_use]
    pub fn height_for(&self, atlas: &FontAtlas) -> f32 {
        atlas.line_count(&self.text) as f32 * LINE_HEIGHT * self.scale()
    }

    /// Measure the rendered height of a single line.
    #[must_use]
    pub fn height(&self) -> f32 {
        LINE_HEIGHT * self.scale()
    }

    /// The label's full extent, the rect [`Label::render`] draws its background
    /// as.
    #[must_use]
    pub fn size(&self, atlas: &FontAtlas) -> Vec2 {
        Vec2::new(self.width(atlas), self.height_for(atlas))
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

// ---------------------------------------------------------------------------
// PointerInput / UiState
// ---------------------------------------------------------------------------

/// The pointer state for one frame.
///
/// Bundled rather than passed as three loose arguments because every clickable
/// widget needs all three, and a bare `bool, bool` pair at a call site is the
/// kind of thing that gets swapped.
#[derive(Debug, Clone, Copy, Default, PartialEq)]
pub struct PointerInput {
    /// Cursor position in screen-space pixels.
    pub pos: Vec2,
    /// Whether the primary pointer button is currently held.
    pub down: bool,
    /// Whether the primary pointer button came up this frame.
    pub released: bool,
}

impl PointerInput {
    /// Pointer at `pos` with no button activity.
    #[must_use]
    pub fn hovering(pos: Vec2) -> Self {
        Self {
            pos,
            down: false,
            released: false,
        }
    }
}

/// Identifies a widget across frames.
///
/// Immediate-mode widgets are rebuilt every frame, so press capture needs
/// *something* stable to latch onto. Callers pick the number; the only
/// requirement is that it names the same widget next frame and no other widget
/// this frame.
pub type WidgetId = u64;

/// The one piece of retained state an immediate-mode UI cannot do without:
/// which widget captured the current press.
///
/// Without it a click is credited to whatever the cursor happens to be over on
/// release, so pressing button A, dragging onto B and releasing fires B. With
/// it, the press latches an `active` widget and only that widget can be
/// clicked — and only if the cursor is still over it when the button comes up.
///
/// Create one, keep it alive across frames, and drive every clickable widget
/// through [`UiState::interact`].
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct UiState {
    active: Option<WidgetId>,
}

impl UiState {
    /// Create state with nothing captured.
    #[must_use]
    pub fn new() -> Self {
        Self { active: None }
    }

    /// The widget that captured the current press, if any.
    #[must_use]
    pub fn active(&self) -> Option<WidgetId> {
        self.active
    }

    /// Drop any capture — for a UI that was torn down mid-press.
    pub fn clear(&mut self) {
        self.active = None;
    }

    /// Resolve one widget's visual state and whether it was clicked.
    ///
    /// - `hovered`: the cursor is inside the widget this frame.
    /// - `pointer_down`: the pointer button is currently held.
    /// - `pointer_released`: the pointer button came up this frame.
    ///
    /// Returns `(state, clicked)`. `clicked` is true only when *this* widget
    /// captured the press **and** still has the cursor at release.
    pub fn interact(
        &mut self,
        id: WidgetId,
        hovered: bool,
        pointer_down: bool,
        pointer_released: bool,
    ) -> (ButtonState, bool) {
        // Latch the capture on the frame the press starts over this widget.
        if pointer_down && hovered && self.active.is_none() {
            self.active = Some(id);
        }
        let captured = self.active == Some(id);
        let clicked = pointer_released && captured && hovered;
        if pointer_released && captured {
            self.active = None;
        }

        let state = if captured {
            ButtonState::Pressed
        } else if hovered && self.active.is_none() {
            ButtonState::Hovered
        } else {
            // Another widget owns the press — this one stays idle even under
            // the cursor, so a drag-off does not light up its neighbour.
            ButtonState::Idle
        };
        (state, clicked)
    }
}

/// A clickable button with text.
///
/// Button is stateless — you provide the [`ButtonState`] each frame based
/// on your input system, normally via [`UiState::interact`] or the
/// [`Button::interact`] wrapper, and the button draws itself.
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

    /// Set the font size.
    #[must_use]
    pub fn with_size(mut self, size: f32) -> Self {
        self.font_size = size;
        self
    }

    /// The layout scale this button's label is drawn at.
    fn scale(&self) -> f32 {
        self.font_size / NATURAL_FONT_SIZE
    }

    /// The bounding rectangle for this button at the given position.
    #[must_use]
    pub fn rect(&self, pos: Vec2, atlas: &FontAtlas) -> (Vec2, Vec2) {
        let scale = self.scale();
        let text_w = atlas.text_width(&self.text, scale);
        let text_h = atlas.line_count(&self.text) as f32 * LINE_HEIGHT * scale;
        let min = pos;
        let max = pos + Vec2::new(text_w, text_h) + self.padding * 2.0;
        (min, max)
    }

    /// Whether the given mouse position is inside the button area.
    #[must_use]
    pub fn hit_test(&self, pos: Vec2, mouse: Vec2, atlas: &FontAtlas) -> bool {
        let (min, max) = self.rect(pos, atlas);
        mouse.x >= min.x && mouse.x <= max.x && mouse.y >= min.y && mouse.y <= max.y
    }

    /// Hit-test this button and resolve its state through `ui`'s press capture.
    ///
    /// Returns `(state, clicked)` — feed `state` straight to
    /// [`Button::render`]. `clicked` is true only when the press *started* on
    /// this button and the cursor is still on it at release.
    pub fn interact(
        &self,
        pos: Vec2,
        atlas: &FontAtlas,
        ui: &mut UiState,
        id: WidgetId,
        pointer: PointerInput,
    ) -> (ButtonState, bool) {
        let hovered = self.hit_test(pos, pointer.pos, atlas);
        ui.interact(id, hovered, pointer.down, pointer.released)
    }

    /// Render the button into a `DrawList` at `state`'s appearance.
    ///
    /// Click detection is [`Button::interact`]'s job — a renderer that also
    /// decided whether it had been clicked could only do so from the state it
    /// was handed, which is exactly the "credited on release to whoever is
    /// under the cursor" bug.
    pub fn render(
        &self,
        dl: &mut DrawList,
        pos: Vec2,
        atlas: &FontAtlas,
        style: &Style,
        state: ButtonState,
    ) {
        let (min, max) = self.rect(pos, atlas);
        let bg = match state {
            ButtonState::Pressed => style.bg_active,
            ButtonState::Hovered => style.bg_hover,
            ButtonState::Idle => style.bg,
        };

        dl.rect(min, max, bg);
        dl.rect_outline(min, max, 1.0, style.border);

        let text_pos = pos + self.padding;
        dl.text(text_pos, &self.text, style.text, self.font_size);
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

    /// The bug: measurement divided by 14 while layout divided by
    /// `GLYPH_HEIGHT` (13), so a label's declared box was ~7% narrower than the
    /// glyphs drawn inside it.
    #[test]
    fn declared_bounds_contain_the_glyphs_they_describe() {
        let atlas = atlas();
        for size in [10.0, 14.0, 28.0] {
            let btn = Button::new("LongButtonLabel").with_size(size);
            let pos = Vec2::new(50.0, 50.0);
            let (min, max) = btn.rect(pos, &atlas);

            let mut dl = DrawList::new();
            btn.render(&mut dl, pos, &atlas, &style(), ButtonState::Idle);
            let (verts, _) = dl.to_triangles(Some(&atlas), 1.0);
            for v in &verts {
                assert!(
                    v.pos.x >= min.x - 0.001
                        && v.pos.x <= max.x + 0.001
                        && v.pos.y >= min.y - 0.001
                        && v.pos.y <= max.y + 0.001,
                    "size {size}: vertex {:?} escapes the declared rect {min:?}..{max:?}",
                    v.pos
                );
            }
        }
    }

    /// The `Text` anchor is a top-left corner, so a label's background must
    /// contain its own glyphs.
    #[test]
    fn label_background_contains_its_glyphs() {
        let atlas = atlas();
        let label = Label::new("Hi").with_bg(true);
        let pos = Vec2::new(10.0, 100.0);
        let mut dl = DrawList::new();
        label.render(&mut dl, pos, &atlas, &style());

        let DrawCommand::Rect { min, max, .. } = dl.commands()[0] else {
            panic!("expected a background Rect");
        };
        let (verts, _) = dl.to_triangles(Some(&atlas), 1.0);
        assert!(!verts.is_empty());
        for v in &verts {
            assert!(
                v.pos.y >= min.y - 0.001 && v.pos.y <= max.y + 0.001,
                "glyph vertex {:?} falls outside the background {min:?}..{max:?}",
                v.pos
            );
        }
    }

    // ── Button ────────────────────────────────────────────────────────────

    #[test]
    fn button_in_idle_state_renders_rect_and_text() {
        let btn = Button::new("Click");
        let mut dl = DrawList::new();
        btn.render(&mut dl, Vec2::ZERO, &atlas(), &style(), ButtonState::Idle);
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
        let atlas = atlas();
        let mut ui = UiState::new();
        let centre = {
            let (min, max) = btn.rect(Vec2::ZERO, &atlas);
            (min + max) * 0.5
        };

        // Press.
        let down = PointerInput {
            pos: centre,
            down: true,
            released: false,
        };
        let up = PointerInput {
            pos: centre,
            down: false,
            released: true,
        };
        let (state, clicked) = btn.interact(Vec2::ZERO, &atlas, &mut ui, 1, down);
        assert_eq!(state, ButtonState::Pressed);
        assert!(!clicked);
        // Release over the same button.
        let (_, clicked) = btn.interact(Vec2::ZERO, &atlas, &mut ui, 1, up);
        assert!(clicked);
        assert_eq!(ui.active(), None, "the capture must be released");
    }

    #[test]
    fn button_no_click_when_not_hovered() {
        let btn = Button::new("Fire");
        let atlas = atlas();
        let mut ui = UiState::new();
        let outside = PointerInput {
            pos: Vec2::new(-100.0, -100.0),
            down: false,
            released: true,
        };
        let (state, clicked) = btn.interact(Vec2::ZERO, &atlas, &mut ui, 1, outside);
        assert_eq!(state, ButtonState::Idle);
        assert!(!clicked);
    }

    // ── press capture ─────────────────────────────────────────────────────

    /// The bug: a click was credited to whatever the cursor was over on
    /// release, so press-A / drag-to-B / release fired B.
    #[test]
    fn press_on_a_and_release_on_b_clicks_neither() {
        let atlas = atlas();
        let a = Button::new("A");
        let b = Button::new("B");
        let a_pos = Vec2::new(0.0, 0.0);
        let b_pos = Vec2::new(200.0, 0.0);
        let inside = |btn: &Button, pos: Vec2| {
            let (min, max) = btn.rect(pos, &atlas);
            (min + max) * 0.5
        };
        let a_centre = inside(&a, a_pos);
        let b_centre = inside(&b, b_pos);
        let mut ui = UiState::new();

        let held = |pos| PointerInput {
            pos,
            down: true,
            released: false,
        };
        let up = |pos| PointerInput {
            pos,
            down: false,
            released: true,
        };

        // Frame 1: press over A.
        let (a_state, a_clicked) = a.interact(a_pos, &atlas, &mut ui, 1, held(a_centre));
        let (b_state, b_clicked) = b.interact(b_pos, &atlas, &mut ui, 2, held(a_centre));
        assert_eq!(a_state, ButtonState::Pressed);
        assert_eq!(b_state, ButtonState::Idle);
        assert!(!a_clicked && !b_clicked);

        // Frame 2: still held, cursor dragged onto B. B must not light up.
        let (a_state, _) = a.interact(a_pos, &atlas, &mut ui, 1, held(b_centre));
        let (b_state, _) = b.interact(b_pos, &atlas, &mut ui, 2, held(b_centre));
        assert_eq!(a_state, ButtonState::Pressed, "A still owns the press");
        assert_eq!(b_state, ButtonState::Idle, "B must not steal the capture");

        // Frame 3: release over B. Neither fires — A lost the cursor, B never
        // captured the press.
        let (_, a_clicked) = a.interact(a_pos, &atlas, &mut ui, 1, up(b_centre));
        let (_, b_clicked) = b.interact(b_pos, &atlas, &mut ui, 2, up(b_centre));
        assert!(!a_clicked, "A lost the cursor before release");
        assert!(!b_clicked, "B never captured the press");
        assert_eq!(ui.active(), None);
    }

    #[test]
    fn hover_needs_no_capture_but_a_captured_press_suppresses_it() {
        let mut ui = UiState::new();
        // Nothing captured: hover reads as Hovered.
        assert_eq!(ui.interact(1, true, false, false).0, ButtonState::Hovered);
        // Widget 1 captures the press…
        assert_eq!(ui.interact(1, true, true, false).0, ButtonState::Pressed);
        // …so widget 2 stays idle even under the cursor.
        assert_eq!(ui.interact(2, true, true, false).0, ButtonState::Idle);
        ui.clear();
        assert_eq!(ui.active(), None);
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
        btn.render(
            &mut dl_hover,
            Vec2::ZERO,
            &atlas(),
            &style(),
            ButtonState::Hovered,
        );

        let mut dl_idle = DrawList::new();
        btn.render(
            &mut dl_idle,
            Vec2::ZERO,
            &atlas(),
            &style(),
            ButtonState::Idle,
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
