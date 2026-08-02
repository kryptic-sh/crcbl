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
// SkinInsets
// ---------------------------------------------------------------------------

/// The fixed corner insets of a nine-slice button skin, in **screen pixels**.
///
/// # Why a `Button` carries the insets and not the art
///
/// A skinned button needs two things: art to draw, and the sizes that art
/// refuses to stretch. Only the second is a *layout* fact, and layout is all
/// this crate does.
///
/// It could not carry the art even if it wanted to. `crcbl-render` depends on
/// `crcbl-ui` — its UI pass takes a [`DrawList`] and its glyph atlas is this
/// crate's [`FontAtlas`] — so a `Button` naming a `SheetId`, a frame or a
/// `NineSlice` would be a dependency *cycle*, not a convenience. The insets are
/// four floats and belong to nobody.
///
/// So the split is: this crate knows how big the corners are and lays out
/// around them, and `crcbl_render::ButtonSkin` owns the sheet and the per-state
/// frames and expands them into sprites. `ButtonSkin::insets` hands back exactly
/// this type, read off the art itself, so the two cannot drift.
///
/// # Screen space, so `top` is the visually-upper edge
///
/// Every rectangle in this crate is Y-**down** with the origin at the
/// framebuffer's top-left, and these insets follow it: `top` is the inset at the
/// smaller Y. That happens to agree with `crcbl_sprite::NineSlice`, whose `top`
/// is the top of the sheet *image* — the flip between the two conventions
/// happens once, where a screen rect becomes a world-space sprite target, and
/// not here.
#[derive(Debug, Clone, Copy, PartialEq, Default)]
pub struct SkinInsets {
    /// Fixed width of the left column, in pixels.
    pub left: f32,
    /// Fixed width of the right column, in pixels.
    pub right: f32,
    /// Fixed height of the top row, in pixels.
    pub top: f32,
    /// Fixed height of the bottom row, in pixels.
    pub bottom: f32,
}

impl SkinInsets {
    /// No fixed bands at all — a skin that stretches everywhere.
    pub const NONE: Self = Self {
        left: 0.0,
        right: 0.0,
        top: 0.0,
        bottom: 0.0,
    };

    /// Insets given per side.
    #[must_use]
    pub const fn new(left: f32, right: f32, top: f32, bottom: f32) -> Self {
        Self {
            left,
            right,
            top,
            bottom,
        }
    }

    /// The same inset on all four sides.
    #[must_use]
    pub const fn uniform(inset: f32) -> Self {
        Self::new(inset, inset, inset, inset)
    }

    /// `(left + right, top + bottom)` — the smallest box that draws all four
    /// corners at their natural size.
    ///
    /// The float twin of `crcbl_sprite::NineSlice::minimum_size`, and the floor
    /// under [`Button::minimum_size`] before the label is even considered.
    #[must_use]
    pub fn minimum_size(&self) -> Vec2 {
        Vec2::new(self.left + self.right, self.top + self.bottom)
    }

    /// The top-left corner's size, as an offset from a rect's `min`.
    #[must_use]
    pub fn min_corner(&self) -> Vec2 {
        Vec2::new(self.left, self.top)
    }

    /// The bottom-right corner's size, as an offset back from a rect's `max`.
    #[must_use]
    pub fn max_corner(&self) -> Vec2 {
        Vec2::new(self.right, self.bottom)
    }
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
///
/// # Skinned and unskinned
///
/// With no [`skin`](Button::skin) the button paints itself: a filled rect and a
/// one-pixel outline in [`Style`]'s colours, then the label. That is the whole
/// widget, and [`Button::render`] emits all three commands.
///
/// With a skin, the background is a **nine-sliced sprite** drawn by the sprite
/// pass rather than by the UI pass, so pressing the button swaps art rather than
/// changing a tint, and stretching it leaves the corners alone. [`Button::render`]
/// then emits only the label — see its docs for what the caller owes.
#[derive(Debug, Clone)]
pub struct Button {
    /// Button label.
    pub text: String,
    /// Font size.
    pub font_size: f32,
    /// Padding inside the button (horizontal, vertical).
    ///
    /// Measured **inside the skin's corners**, not from the button's edge: a
    /// skinned button's content box is its rect inset by the corners and then by
    /// this. With no skin the corners are zero and it is the plain inset it has
    /// always been.
    pub padding: Vec2,
    /// The skin's fixed corner sizes, or `None` for the flat painted button.
    ///
    /// Set it from `crcbl_render::ButtonSkin::insets` rather than by hand, so the
    /// layout and the art it is laying out around come from one place.
    pub skin: Option<SkinInsets>,
    /// The size the caller wants the button drawn at, before
    /// [`Button::minimum_size`] is applied. `None` shrinks to fit the label.
    ///
    /// A *request*, not a guarantee — see [`Button::size`] for what happens when
    /// it is too small.
    pub fixed_size: Option<Vec2>,
}

impl Button {
    /// Create a new button with the given text.
    #[must_use]
    pub fn new(text: impl Into<String>) -> Self {
        Self {
            text: text.into(),
            font_size: 14.0,
            padding: Vec2::new(8.0, 4.0),
            skin: None,
            fixed_size: None,
        }
    }

    /// Set the font size.
    #[must_use]
    pub fn with_size(mut self, size: f32) -> Self {
        self.font_size = size;
        self
    }

    /// Lay this button out around a nine-slice skin with the given corners.
    #[must_use]
    pub fn with_skin(mut self, insets: SkinInsets) -> Self {
        self.skin = Some(insets);
        self
    }

    /// Ask for a particular size, in pixels.
    ///
    /// Clamped up to [`Button::minimum_size`] — a button is never drawn smaller
    /// than its own corners and label.
    #[must_use]
    pub fn with_fixed_size(mut self, size: Vec2) -> Self {
        self.fixed_size = Some(size);
        self
    }

    /// The layout scale this button's label is drawn at.
    fn scale(&self) -> f32 {
        self.font_size / NATURAL_FONT_SIZE
    }

    /// The skin's corners, or zeroes when there is no skin.
    fn insets(&self) -> SkinInsets {
        self.skin.unwrap_or(SkinInsets::NONE)
    }

    /// The label's own extent, with no padding and no corners around it.
    #[must_use]
    pub fn label_size(&self, atlas: &FontAtlas) -> Vec2 {
        let scale = self.scale();
        Vec2::new(
            atlas.text_width(&self.text, scale),
            atlas.line_count(&self.text) as f32 * LINE_HEIGHT * scale,
        )
    }

    /// The smallest size this button can be drawn at: **its corners plus its
    /// label**, with the padding between them.
    ///
    /// Both terms are load-bearing and neither is enough alone. The corners are
    /// the sizes the nine-slice refuses to stretch, so a box narrower than
    /// `left + right` has nowhere to put them; the label is the content, so a box
    /// that fits the corners and not the text draws the text over them.
    ///
    /// The padding is added rather than absorbed: it is the gap the caller asked
    /// for *between* the frame and the words, and a minimum that let the two
    /// touch would silently ignore it at exactly the size where it is most
    /// visible.
    ///
    /// With no skin the corners are zero and this is `label + 2 × padding` — the
    /// rect an unskinned button has always been.
    #[must_use]
    pub fn minimum_size(&self, atlas: &FontAtlas) -> Vec2 {
        self.insets().minimum_size() + self.label_size(atlas) + self.padding * 2.0
    }

    /// The size this button is actually drawn at.
    ///
    /// [`Button::fixed_size`] when it is at least [`Button::minimum_size`] on
    /// that axis, and the minimum otherwise — **each axis clamps on its own**, so
    /// a button asked for a generous width and a mean height keeps the width.
    ///
    /// # Why clamp rather than shrink
    ///
    /// `NineSliceSource::expand` handles a target below its minimum by squashing
    /// the corners in proportion, which is right for what it was written for: a
    /// pipe closing to nothing should close continuously rather than vanish or
    /// spill. A button is the other case. Its content is *text*, which does not
    /// shrink with the frame — a squashed button would draw a full-size label
    /// across smudged corners, which is both of the failures this slice exists to
    /// remove, at once. Growing to the minimum is visible in the layout, where a
    /// caller can see it and fix it, rather than in the pixels.
    #[must_use]
    pub fn size(&self, atlas: &FontAtlas) -> Vec2 {
        let minimum = self.minimum_size(atlas);
        self.fixed_size.map_or(minimum, |asked| asked.max(minimum))
    }

    /// The bounding rectangle for this button at the given position.
    #[must_use]
    pub fn rect(&self, pos: Vec2, atlas: &FontAtlas) -> (Vec2, Vec2) {
        (pos, pos + self.size(atlas))
    }

    /// The box the label is centred in: the button inset by its corners and then
    /// by its padding.
    ///
    /// Centring here rather than in the button's full rect is what keeps an
    /// **asymmetric** skin honest. A frame with a 4-pixel left cap and a 20-pixel
    /// right one has its content box off-centre, and a label centred in the outer
    /// rect would sit under the fat cap at the minimum size. With no skin the two
    /// boxes share a centre and this is the ordinary thing.
    #[must_use]
    pub fn content_rect(&self, pos: Vec2, atlas: &FontAtlas) -> (Vec2, Vec2) {
        let insets = self.insets();
        let (min, max) = self.rect(pos, atlas);
        (
            min + insets.min_corner() + self.padding,
            max - insets.max_corner() - self.padding,
        )
    }

    /// Where [`Button::render`] anchors the label — the top-left of its em box.
    ///
    /// Centred in [`Button::content_rect`] on both axes, so it stays centred as
    /// the button is resized.
    #[must_use]
    pub fn label_pos(&self, pos: Vec2, atlas: &FontAtlas) -> Vec2 {
        let (min, max) = self.content_rect(pos, atlas);
        min + (max - min - self.label_size(atlas)) * 0.5
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
    ///
    /// # A skinned button emits only its label
    ///
    /// The background of a skinned button is a nine-sliced **sprite**, and the
    /// UI pass cannot draw one: its atlas is `R8Unorm` — a glyph coverage mask —
    /// and `ui.slang` multiplies that single channel into alpha and takes RGB
    /// from the vertex colour. There is no textured-quad command in
    /// [`DrawList`], no second texture bound, and no UV on a `Rect`.
    ///
    /// So a skinned button is drawn by two passes. The caller expands
    /// `crcbl_render::ButtonSkin` into sprites for the same `state` and submits
    /// them to the sprite pass, then calls this for the label. **The sprite pass
    /// must be added to the render graph before the UI pass** — passes execute in
    /// the order they were declared — or the skin paints over its own text.
    pub fn render(
        &self,
        dl: &mut DrawList,
        pos: Vec2,
        atlas: &FontAtlas,
        style: &Style,
        state: ButtonState,
    ) {
        if self.skin.is_none() {
            let (min, max) = self.rect(pos, atlas);
            let bg = match state {
                ButtonState::Pressed => style.bg_active,
                ButtonState::Hovered => style.bg_hover,
                ButtonState::Idle => style.bg,
            };

            dl.rect(min, max, bg);
            dl.rect_outline(min, max, 1.0, style.border);
        }

        dl.text(
            self.label_pos(pos, atlas),
            &self.text,
            style.text,
            self.font_size,
        );
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

    // ── skinned buttons ───────────────────────────────────────────────────

    /// Deliberately **asymmetric on every side**, so a transposed or mirrored
    /// pair cannot compare equal by accident: a swapped left/right shifts the
    /// content box by 16, and a swapped top/bottom by 6.
    const INSETS: SkinInsets = SkinInsets::new(4.0, 20.0, 3.0, 9.0);

    /// The minimum is the corners **and** the label, with the padding between
    /// them — neither term alone.
    #[test]
    fn a_skinned_buttons_minimum_size_is_its_corners_plus_its_label() {
        let atlas = atlas();
        let plain = Button::new("Play");
        let skinned = plain.clone().with_skin(INSETS);

        let label = plain.label_size(&atlas);
        assert!(label.x > 0.0 && label.y > 0.0);

        // Unskinned: unchanged from what a button has always measured.
        assert_eq!(
            plain.minimum_size(&atlas),
            label + plain.padding * 2.0,
            "an unskinned button's minimum must still be its label plus padding"
        );
        assert_eq!(plain.rect(Vec2::ZERO, &atlas).1, plain.minimum_size(&atlas));

        // Skinned: exactly the corners more.
        assert_eq!(
            skinned.minimum_size(&atlas) - plain.minimum_size(&atlas),
            Vec2::new(INSETS.left + INSETS.right, INSETS.top + INSETS.bottom),
            "the skin's corners must be added to the label's own minimum"
        );

        // And the corners alone are a floor even with no label at all: a
        // minimum that only measured text would let the corners overlap.
        let empty = Button::new("").with_skin(INSETS);
        let corners = INSETS.minimum_size();
        assert!(
            empty.minimum_size(&atlas).x >= corners.x && empty.minimum_size(&atlas).y >= corners.y,
            "an empty label must still leave room for the four corners"
        );
    }

    /// A caller asking for less than the minimum gets the minimum, per axis.
    #[test]
    fn a_button_asked_for_less_than_its_minimum_grows_to_it() {
        let atlas = atlas();
        let button = Button::new("Play").with_skin(INSETS);
        let minimum = button.minimum_size(&atlas);

        // Far too small on both axes.
        let squashed = button.clone().with_fixed_size(Vec2::new(1.0, 1.0));
        assert_eq!(squashed.size(&atlas), minimum);
        assert_eq!(squashed.rect(Vec2::ZERO, &atlas).1, minimum);

        // Generous on both axes: honoured exactly.
        let roomy = minimum + Vec2::new(200.0, 40.0);
        assert_eq!(button.clone().with_fixed_size(roomy).size(&atlas), roomy);

        // Each axis clamps on its own — a mean height must not cost the width.
        let mixed = button.with_fixed_size(Vec2::new(minimum.x + 200.0, 1.0));
        assert_eq!(
            mixed.size(&atlas),
            Vec2::new(minimum.x + 200.0, minimum.y),
            "the width was fine and must survive the height being clamped"
        );

        // Clamping, not squashing: the content box never inverts, so the label
        // is never drawn across the corners.
        let (content_min, content_max) = mixed.content_rect(Vec2::ZERO, &atlas);
        assert!(
            content_max.x >= content_min.x && content_max.y >= content_min.y,
            "the content box inverted: {content_min:?}..{content_max:?}"
        );
    }

    /// The label is centred in the content box, and **stays** centred as the
    /// button is resized — the property a fixed top-left anchor would fail at
    /// every width but one.
    #[test]
    fn the_label_stays_centred_when_the_button_is_resized() {
        let atlas = atlas();
        let button = Button::new("Play").with_skin(INSETS);
        let pos = Vec2::new(37.0, 11.0);
        let label = button.label_size(&atlas);

        for width in [button.minimum_size(&atlas).x, 200.0, 640.0] {
            for height in [button.minimum_size(&atlas).y, 60.0, 180.0] {
                let sized = button.clone().with_fixed_size(Vec2::new(width, height));
                let (content_min, content_max) = sized.content_rect(pos, &atlas);
                let anchor = sized.label_pos(pos, &atlas);

                // Equal slack on the left and the right, and on top and bottom.
                let before = anchor - content_min;
                let after = content_max - (anchor + label);
                assert!(
                    (before.x - after.x).abs() < 1e-3 && (before.y - after.y).abs() < 1e-3,
                    "at {width}x{height} the label is off-centre: {before:?} before, \
                     {after:?} after"
                );

                // And it is inside the content box, so it never touches a corner.
                assert!(
                    before.x >= -1e-3 && before.y >= -1e-3,
                    "at {width}x{height} the label escapes its content box"
                );
            }
        }
    }

    /// Adding a skin must not change where an *unskinned* button puts its text —
    /// centring in the content box has to reduce to the old `pos + padding`, or
    /// every existing layout shifts.
    ///
    /// The tolerance is `1e-4` **pixels** and not zero, deliberately. Centring
    /// reaches the same point by a different route — `min + (max - min - label) /
    /// 2` where the old code wrote `pos + padding` — and at a shrink-to-fit size
    /// those two associate the same sum differently, which lands up to a few ulps
    /// apart (measured: `58.000004` against `58.0`). That is four millionths of a
    /// pixel and is not a layout change; asserting bit equality here would be
    /// asserting float associativity, which is not the claim.
    #[test]
    fn an_unskinned_buttons_label_is_still_anchored_at_pos_plus_padding() {
        let atlas = atlas();
        for text in ["A", "LongButtonLabel", ""] {
            for size in [10.0, 14.0, 28.0] {
                let button = Button::new(text).with_size(size);
                let pos = Vec2::new(50.0, 50.0);
                let moved = (button.label_pos(pos, &atlas) - (pos + button.padding)).abs();
                assert!(
                    moved.x < 1e-4 && moved.y < 1e-4,
                    "{text:?} at {size}: the label moved by {moved:?} px"
                );
            }
        }
    }

    /// A skinned button emits **only** its label: the background is a sprite the
    /// UI pass cannot draw, and a widget that also painted a flat rect would
    /// paint it over the skin.
    #[test]
    fn a_skinned_button_draws_its_label_and_no_background() {
        let atlas = atlas();
        let button = Button::new("Play").with_skin(INSETS);
        for state in [
            ButtonState::Idle,
            ButtonState::Hovered,
            ButtonState::Pressed,
        ] {
            let mut dl = DrawList::new();
            button.render(&mut dl, Vec2::ZERO, &atlas, &style(), state);
            assert_eq!(dl.len(), 1, "{state:?} emitted more than the label");
            assert!(
                matches!(dl.commands()[0], DrawCommand::Text { .. }),
                "{state:?} emitted something that is not the label"
            );
        }

        // The unskinned button is untouched: rect, outline, label.
        let mut dl = DrawList::new();
        Button::new("Play").render(&mut dl, Vec2::ZERO, &atlas, &style(), ButtonState::Idle);
        assert_eq!(dl.len(), 3);
    }

    /// Hit testing follows the drawn rect, so a resized or clamped button is
    /// clickable exactly where it is visible — including in the corners, which
    /// are part of the button and not decoration around it.
    #[test]
    fn a_resized_skinned_button_is_clickable_across_its_whole_rect() {
        let atlas = atlas();
        let button = Button::new("Play")
            .with_skin(INSETS)
            .with_fixed_size(Vec2::new(400.0, 90.0));
        let pos = Vec2::new(10.0, 20.0);
        let (min, max) = button.rect(pos, &atlas);
        assert_eq!(max - min, Vec2::new(400.0, 90.0));

        assert!(button.hit_test(pos, (min + max) * 0.5, &atlas));
        assert!(
            button.hit_test(pos, min + Vec2::new(1.0, 1.0), &atlas),
            "the top-left corner of the skin is part of the button"
        );
        assert!(
            button.hit_test(pos, max - Vec2::new(1.0, 1.0), &atlas),
            "the bottom-right corner of the skin is part of the button"
        );
        assert!(
            !button.hit_test(pos, max + Vec2::new(2.0, 2.0), &atlas),
            "past the skin is not the button"
        );
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
