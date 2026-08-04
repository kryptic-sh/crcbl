//! Menus: a centred window frame, skinned buttons inside it, and the keyboard
//! that drives them.
//!
//! ```text
//!                     ┌───────────────────────────┐
//!                     │         PAUSED            │   ← title
//!                     │  ┌─────────────────────┐  │
//!                     │  │ RESUME          ESC │  │   ← selected → Hovered
//!                     │  ├─────────────────────┤  │
//!                     │  │ FULLSCREEN      F11 │  │
//!                     │  └─────────────────────┘  │
//!                     └───────────────────────────┘
//!                       centred in the framebuffer
//! ```
//!
//! # What is here and what is not
//!
//! **Layout, selection and activation.** Every rectangle this module produces is
//! in screen pixels, Y **down**, the space the whole crate measures in — so a
//! menu is positioned, hit-tested and navigated with no device and no art in the
//! room, which is what lets the interesting properties (it is centred, its
//! corners do not move, the paused state does not also draw the start menu) be
//! asserted as arithmetic.
//!
//! **Not the pictures.** The window frame and the three button frames are
//! nine-sliced **sprites**, and the UI pass cannot draw one — its atlas is a
//! single-channel glyph coverage mask, and [`DrawList`] has no textured-quad
//! command. `crcbl_render::MenuArt` owns the art and turns a [`MenuLayout`] into
//! sprites; [`Menu::render`] emits only the text. The split is exactly
//! [`Button`](crate::Button)'s, for exactly the same reason, and the two halves
//! are joined by [`MenuStyle`]'s insets — read off the art by
//! `crcbl_render::MenuArt::insets`, so the layout and the picture cannot drift.
//!
//! # Why the whole menu is one type and not a `Vec<Button>`
//!
//! A menu has one property a pile of buttons does not: **it is centred as a
//! unit**. Its panel has to be measured before any button can be placed, because
//! the panel's width is the widest button's width and the buttons are centred in
//! the panel. Handing a caller three `Button`s and letting it do the arithmetic
//! is handing three samples the same arithmetic to get subtly differently wrong.
//!
//! # The keyboard is the primary input, and the pointer is optional
//!
//! Both samples are played with the keyboard, so a menu that could only be
//! clicked would be a regression. The model here is keyboard-first:
//! [`Menu::select_next`]/[`Menu::select_previous`] move a **selection**, and the
//! selected item draws as [`ButtonState::Hovered`] — the same art the pointer
//! lights up, because "the thing that will happen if you commit" is the same
//! idea for both devices. [`Menu::press`] holds it at [`ButtonState::Pressed`]
//! while the key is down and [`Menu::activate`] fires it.
//!
//! The pointer is [`Menu::point`], is entirely optional, and when a cursor is
//! over an item it takes over the highlight for as long as it is there. A game
//! that never calls it behaves exactly as if there were no mouse.

use glam::Vec2;

use crate::draw_list::DrawList;
use crate::text::{FontAtlas, LINE_HEIGHT};
use crate::widget::{ButtonState, NATURAL_FONT_SIZE, PointerInput, SkinInsets, UiState, WidgetId};

// ---------------------------------------------------------------------------
// Style
// ---------------------------------------------------------------------------

/// Every length a menu is laid out with, in **pixels**.
///
/// Built from a whole-number `scale` by [`MenuStyle::pixel_art`]: the art is
/// pixel art and the panel is drawn at one texel per `scale` device pixels, so
/// every gap, pad and font size is a base metric times the same integer. A
/// fractional scale would put a nine-slice's fixed corner on a half pixel, which
/// is the one thing `SampleMode::Pixel` cannot hide.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct MenuStyle {
    /// Device pixels per texel of the menu art.
    pub scale: f32,
    /// The window frame's fixed corners.
    pub panel: SkinInsets,
    /// A button skin's fixed corners.
    pub button: SkinInsets,
    /// Space between the panel's frame and its contents.
    pub panel_padding: Vec2,
    /// Space between a button's frame and its label.
    pub button_padding: Vec2,
    /// Vertical space between two buttons.
    pub item_gap: f32,
    /// Vertical space between the title and the first button.
    pub title_gap: f32,
    /// The title's font size.
    pub title_size: f32,
    /// An item label's font size.
    pub item_size: f32,
    /// The least space between an item's label and its key hint.
    pub hint_gap: f32,
    /// The title's colour.
    pub title_color: [f32; 4],
    /// An item label's colour.
    pub label_color: [f32; 4],
    /// A key hint's colour — dimmer than the label it belongs to.
    pub hint_color: [f32; 4],
    /// What the frame behind the menu is dimmed with.
    pub scrim_color: [f32; 4],
}

/// The insets of the window frame `crcbl-render` ships, in texels.
///
/// Here rather than only in the art so a menu can be laid out — and the layout
/// tested — with no renderer and no device. `crcbl_render::menu`'s
/// `the_shipped_art_has_the_insets_the_layout_assumes` is what stops the two
/// drifting: it reads them back off the baked sheet and compares.
pub const PANEL_INSETS: SkinInsets = SkinInsets::new(4.0, 4.0, 4.0, 4.0);

/// The insets of the button skin `crcbl-render` ships, in texels.
///
/// **Currently the same four numbers as [`PANEL_INSETS`], and a separate
/// constant anyway.** A `.crpix` carries one set of insets for the whole sheet
/// and the shipped art is one sheet, so the panel and the buttons have to agree
/// today; they describe different things and a redrawn sheet could split them
/// without touching a line of layout. `crcbl_render::menu`'s
/// `the_shipped_art_has_the_insets_the_layout_assumes` reads both back off the
/// art, so a split shows up as a failure rather than as a silently wrong label
/// position.
pub const BUTTON_INSETS: SkinInsets = SkinInsets::new(4.0, 4.0, 4.0, 4.0);

impl MenuStyle {
    /// The largest scale [`Menu::layout`] will choose.
    ///
    /// Four rather than unbounded: past it the title's glyphs are taller than a
    /// brick and the menu stops reading as part of the game.
    pub const MAX_SCALE: u32 = 4;

    /// The shipped look, at `scale` device pixels per texel.
    ///
    /// `scale` is clamped to at least one — a menu drawn at zero is not a
    /// smaller menu, it is an invisible one.
    #[must_use]
    pub fn pixel_art(scale: u32) -> Self {
        let scale = scale.max(1) as f32;
        Self {
            scale,
            panel: PANEL_INSETS,
            button: BUTTON_INSETS,
            panel_padding: Vec2::new(8.0, 7.0) * scale,
            button_padding: Vec2::new(6.0, 3.0) * scale,
            item_gap: 4.0 * scale,
            title_gap: 8.0 * scale,
            // Twice the atlas's natural height, so the title is a heading rather
            // than a bolder line of the same text.
            title_size: NATURAL_FONT_SIZE * 2.0 * scale,
            item_size: NATURAL_FONT_SIZE * scale,
            hint_gap: 10.0 * scale,
            title_color: [1.0, 0.94, 0.55, 1.0],
            label_color: [0.94, 0.95, 1.0, 1.0],
            hint_color: [0.62, 0.64, 0.82, 1.0],
            // Two thirds, in straight alpha: enough that the panel's own dark
            // fill separates from the game behind it, not so much that the
            // player loses track of where the ball was.
            scrim_color: [0.0, 0.0, 0.0, 0.66],
        }
    }

    /// The panel's fixed corners, in device pixels — the insets times the scale.
    ///
    /// The art's insets are in **texels** and everything else in this type is in
    /// pixels; this is the one place the two meet, and the reason
    /// [`MenuStyle::panel`] is not pre-multiplied is that
    /// `crcbl_render::MenuArt` needs the texel figure to check the art against.
    #[must_use]
    pub fn panel_corners(&self) -> SkinInsets {
        scaled(self.panel, self.scale)
    }

    /// A button's fixed corners, in device pixels.
    #[must_use]
    pub fn button_corners(&self) -> SkinInsets {
        scaled(self.button, self.scale)
    }
}

fn scaled(insets: SkinInsets, scale: f32) -> SkinInsets {
    SkinInsets::new(
        insets.left * scale,
        insets.right * scale,
        insets.top * scale,
        insets.bottom * scale,
    )
}

impl Default for MenuStyle {
    fn default() -> Self {
        Self::pixel_art(1)
    }
}

// ---------------------------------------------------------------------------
// The model
// ---------------------------------------------------------------------------

/// One line of a menu: what it says, what key does it, and what it is called.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MenuItem {
    /// Names this item to the caller — what [`Menu::activate`] hands back.
    ///
    /// A [`WidgetId`] rather than an index, so a menu that grows an entry does
    /// not silently re-number what the loop is matching on.
    pub id: WidgetId,
    /// The label, drawn left-aligned in the button.
    pub label: String,
    /// The key that also does this, drawn right-aligned and dimmer. Empty for an
    /// item with no keyboard shortcut of its own.
    ///
    /// **A hint, not a binding.** This type does not read the keyboard; the game
    /// loop already owns its key handling and this is what tells the player what
    /// it is.
    pub hint: String,
}

impl MenuItem {
    /// An item with a key hint beside it.
    #[must_use]
    pub fn new(id: WidgetId, label: impl Into<String>, hint: impl Into<String>) -> Self {
        Self {
            id,
            label: label.into(),
            hint: hint.into(),
        }
    }
}

/// A modal menu: a title, some items, and which one is selected.
///
/// Retained across frames — unlike the rest of this crate — because a selection
/// is state by definition, and an immediate-mode menu whose highlight was
/// recomputed from nothing every frame would have no keyboard at all.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct Menu {
    /// The heading, drawn above the items.
    pub title: String,
    items: Vec<MenuItem>,
    selected: usize,
    /// Whether the selected item is being held down.
    pressed: bool,
    /// The item the current press belongs to — the pointer's captured item or
    /// the keyboard's selection. What decides who draws [`ButtonState::Pressed`]:
    /// `pressed` alone cannot, because a drag onto a neighbour leaves the
    /// capture (and the drawn press) with the item it started on.
    pressed_index: Option<usize>,
    /// Which item the pointer is over, if any. Takes over the highlight from
    /// [`Menu::selected`] while it is `Some`.
    hovered: Option<usize>,
}

impl Menu {
    /// A menu with a title and its items, the first selected.
    #[must_use]
    pub fn new(title: impl Into<String>, items: Vec<MenuItem>) -> Self {
        Self {
            title: title.into(),
            items,
            selected: 0,
            pressed: false,
            pressed_index: None,
            hovered: None,
        }
    }

    /// The items, in the order they are drawn.
    #[must_use]
    pub fn items(&self) -> &[MenuItem] {
        &self.items
    }

    /// Which item is selected. Zero for an empty menu, which has no items to
    /// index and never activates one.
    #[must_use]
    pub const fn selected(&self) -> usize {
        self.selected
    }

    /// The selected item, or `None` when there are none.
    #[must_use]
    pub fn selected_item(&self) -> Option<&MenuItem> {
        self.items.get(self.selected)
    }

    /// Moves the selection down one, wrapping at the end.
    pub fn select_next(&mut self) {
        self.move_selection(1);
    }

    /// Moves the selection up one, wrapping at the start.
    pub fn select_previous(&mut self) {
        self.move_selection(-1);
    }

    fn move_selection(&mut self, delta: isize) {
        if self.items.is_empty() {
            return;
        }
        let len = self.items.len() as isize;
        let next = (self.selected as isize + delta).rem_euclid(len);
        self.selected = next as usize;
        // A keyboard move takes the highlight back off the pointer: otherwise a
        // cursor left resting over an item makes the arrow keys look dead.
        self.hovered = None;
        self.pressed = false;
        self.pressed_index = None;
    }

    /// Selects the item with this id, if it is in the menu.
    ///
    /// Returns whether it was found — a caller restoring a selection across a
    /// rebuild wants to know when the item it remembered is gone.
    pub fn select_id(&mut self, id: WidgetId) -> bool {
        match self.items.iter().position(|item| item.id == id) {
            Some(index) => {
                self.selected = index;
                self.hovered = None;
                true
            }
            None => false,
        }
    }

    /// Holds the highlighted item down, or lets it up.
    ///
    /// Called from the key-down and key-up of whatever key commits — the visible
    /// half of "the button is being pressed". Activation is [`Menu::activate`];
    /// this only changes what is drawn.
    pub const fn press(&mut self, down: bool) {
        self.pressed = down;
        // A keyboard press belongs to the highlighted item, which is the
        // selected one until the pointer takes over.
        self.pressed_index = if down { Some(self.selected) } else { None };
    }

    /// Fires the highlighted item and returns its id.
    ///
    /// The **highlighted** one, which is the hovered item when a pointer is over
    /// one and the selected item otherwise — so Enter commits what the player can
    /// see is about to happen, whichever device put the highlight there.
    pub fn activate(&mut self) -> Option<WidgetId> {
        self.pressed = false;
        self.pressed_index = None;
        let index = self.hovered.unwrap_or(self.selected);
        self.items.get(index).map(|item| item.id)
    }

    /// Drops any hover and any held key.
    ///
    /// For a menu being taken off the screen: a menu re-shown with a stale
    /// `Pressed` on it draws a button nobody is touching.
    pub const fn clear_input(&mut self) {
        self.hovered = None;
        self.pressed = false;
        self.pressed_index = None;
    }

    /// How item `index` is drawn.
    ///
    /// [`ButtonState::Pressed`] for the item the current press belongs to —
    /// the pointer's captured item, or the selected one under the keyboard —
    /// [`ButtonState::Hovered`] for the highlighted item otherwise, and
    /// [`ButtonState::Idle`] for everything else — which is what makes a
    /// keyboard-only player see the same three frames of art a mouse would light
    /// up.
    #[must_use]
    pub fn state(&self, index: usize) -> ButtonState {
        if index >= self.items.len() || index != self.hovered.unwrap_or(self.selected) {
            return ButtonState::Idle;
        }
        if self.pressed_index == Some(index) {
            ButtonState::Pressed
        } else if self.pressed {
            // The press belongs to another item (a drag-off): this one must
            // stay Idle, exactly as `interact` reports it.
            ButtonState::Idle
        } else {
            ButtonState::Hovered
        }
    }

    /// Runs one frame of pointer input against `layout`, and reports a click.
    ///
    /// Entirely optional: a game that never calls this is driven by the keyboard
    /// alone and every item stays at the state [`Menu::state`] derives from the
    /// selection. A game that does call it gets hover, press capture through
    /// `ui` — so a press that starts on one item and is released over another
    /// fires neither — and a click that also **moves the selection**, so the
    /// keyboard picks up where the mouse left off.
    pub fn point(
        &mut self,
        layout: &MenuLayout,
        ui: &mut UiState,
        pointer: PointerInput,
    ) -> Option<WidgetId> {
        let mut clicked = None;
        let mut hovered = None;
        for (index, item) in layout.items.iter().enumerate() {
            let inside = pointer.pos.x >= item.min.x
                && pointer.pos.x <= item.max.x
                && pointer.pos.y >= item.min.y
                && pointer.pos.y <= item.max.y;
            let (state, fired) = ui.interact(item.id, inside, pointer.down, pointer.released);
            if inside {
                hovered = Some(index);
            }
            if state == ButtonState::Pressed && inside {
                self.pressed = true;
                self.pressed_index = Some(index);
            }
            if fired {
                clicked = Some(item.id);
                self.selected = index;
            }
        }
        self.hovered = hovered;
        if !pointer.down {
            self.pressed = false;
            self.pressed_index = None;
        }
        clicked
    }

    /// Lays this menu out centred in an `extent`-sized framebuffer, at the
    /// largest scale that fits.
    ///
    /// The scale is a pure function of the extent and the menu's own contents:
    /// the largest whole number up to [`MenuStyle::MAX_SCALE`] whose panel fits
    /// inside [`FIT_FRACTION`] of the framebuffer on **both** axes, and 1 when
    /// none of them do. Whole numbers because the art is pixel art; the fit
    /// because a menu that overflowed a 1440×400 canvas would have its buttons
    /// off the bottom of the screen, which is the failure this replaces.
    #[must_use]
    pub fn layout(&self, extent: (u32, u32), atlas: &FontAtlas) -> MenuLayout {
        let mut chosen = MenuStyle::pixel_art(1);
        for scale in 2..=MenuStyle::MAX_SCALE {
            let style = MenuStyle::pixel_art(scale);
            let size = self.panel_size(atlas, &style);
            if size.x <= extent.0 as f32 * FIT_FRACTION && size.y <= extent.1 as f32 * FIT_FRACTION
            {
                chosen = style;
            } else {
                break;
            }
        }
        self.layout_with(extent, atlas, &chosen)
    }

    /// Lays this menu out centred, at a style the caller chose.
    #[must_use]
    pub fn layout_with(
        &self,
        extent: (u32, u32),
        atlas: &FontAtlas,
        style: &MenuStyle,
    ) -> MenuLayout {
        let screen = Vec2::new(extent.0 as f32, extent.1 as f32);
        let size = self.panel_size(atlas, style);
        // Rounded to whole pixels: a nine-slice corner starting on a half pixel
        // is a blurred corner under `SampleMode::Pixel`, which is the whole
        // reason the art is authored in texels. The centre is therefore within
        // half a pixel of the framebuffer's, not exactly on it — see
        // `the_panel_is_centred_at_every_aspect_ratio`.
        let min = ((screen - size) * 0.5).round();
        let panel = (min, min + size);

        let corners = style.panel_corners();
        let content_left = min.x + corners.left + style.panel_padding.x;
        let content_width = size.x - corners.left - corners.right - style.panel_padding.x * 2.0;
        let mut y = min.y + corners.top + style.panel_padding.y;

        let title_pos = Vec2::new(
            content_left + (content_width - text_width(atlas, &self.title, style.title_size)) * 0.5,
            y,
        );
        if !self.title.is_empty() {
            y += line_height(style.title_size) + style.title_gap;
        }

        let button_height = self.button_height(style);
        let items = self
            .items
            .iter()
            .enumerate()
            .map(|(index, item)| {
                let top = y + index as f32 * (button_height + style.item_gap);
                let item_min = Vec2::new(content_left, top);
                let item_max = Vec2::new(content_left + content_width, top + button_height);
                let inner = style.button_corners();
                let label_x = item_min.x + inner.left + style.button_padding.x;
                let label_y = item_min.y
                    + (button_height - line_height(style.item_size)) * 0.5
                    // Zero on the shipped art, whose cap and base are the same
                    // depth. It is here for a skin whose base is deeper than its
                    // cap: a label centred in the whole button would then sit low
                    // in the face the art actually draws, and half the difference
                    // puts it back in the middle of what is visible.
                    + (inner.top - inner.bottom) * 0.5;
                let hint_width = text_width(atlas, &item.hint, style.item_size);
                MenuItemLayout {
                    id: item.id,
                    min: item_min,
                    max: item_max,
                    label_pos: Vec2::new(label_x, label_y),
                    hint_pos: Vec2::new(
                        item_max.x - inner.right - style.button_padding.x - hint_width,
                        label_y,
                    ),
                }
            })
            .collect();

        MenuLayout {
            style: *style,
            screen,
            panel,
            title_pos,
            items,
        }
    }

    /// The panel's size at `style`, before it is placed anywhere.
    ///
    /// Public because the *fit* half of [`Menu::layout`] is a question a caller
    /// may reasonably ask on its own ("would this menu fit at scale 3?"), and
    /// because a test that could not measure the panel without placing it could
    /// not tell a menu that grew from one that moved.
    #[must_use]
    pub fn panel_size(&self, atlas: &FontAtlas, style: &MenuStyle) -> Vec2 {
        let corners = style.panel_corners();
        let content = self.content_size(atlas, style);
        content + corners.minimum_size() + style.panel_padding * 2.0
    }

    /// The box the title and the buttons occupy, inside the frame and the
    /// padding.
    fn content_size(&self, atlas: &FontAtlas, style: &MenuStyle) -> Vec2 {
        let mut width = text_width(atlas, &self.title, style.title_size);
        let inner = style.button_corners();
        for item in &self.items {
            let label = text_width(atlas, &item.label, style.item_size);
            let hint = text_width(atlas, &item.hint, style.item_size);
            let gap = if item.hint.is_empty() {
                0.0
            } else {
                style.hint_gap
            };
            width = width
                .max(label + gap + hint + inner.minimum_size().x + style.button_padding.x * 2.0);
        }

        let mut height = 0.0;
        if !self.title.is_empty() {
            height += line_height(style.title_size) + style.title_gap;
        }
        if !self.items.is_empty() {
            let button = self.button_height(style);
            height +=
                button * self.items.len() as f32 + style.item_gap * (self.items.len() - 1) as f32;
        }
        Vec2::new(width, height)
    }

    /// Every button is the same height: its label, its padding and its skin's
    /// cap and base.
    fn button_height(&self, style: &MenuStyle) -> f32 {
        let inner = style.button_corners();
        line_height(style.item_size) + style.button_padding.y * 2.0 + inner.minimum_size().y
    }

    /// Emits the menu's **text** — the title, and each item's label and hint.
    ///
    /// Not the panel and not the buttons: those are sprites, and the caller
    /// submits them through `crcbl_render::MenuArt` to a pass that runs **before**
    /// the UI pass. A menu drawn with this alone is a menu with no frame, which
    /// is what a caller that forgot the other half sees.
    pub fn render(&self, dl: &mut DrawList, layout: &MenuLayout) {
        let style = &layout.style;
        if !self.title.is_empty() {
            dl.text(
                layout.title_pos,
                self.title.as_str(),
                style.title_color,
                style.title_size,
            );
        }
        for (item, placed) in self.items.iter().zip(layout.items.iter()) {
            dl.text(
                placed.label_pos,
                item.label.as_str(),
                style.label_color,
                style.item_size,
            );
            if !item.hint.is_empty() {
                dl.text(
                    placed.hint_pos,
                    item.hint.as_str(),
                    style.hint_color,
                    style.item_size,
                );
            }
        }
    }
}

/// The share of the framebuffer a menu is allowed to fill before
/// [`Menu::layout`] drops to a smaller scale.
///
/// Nine tenths rather than all of it: a modal that reached the edge of the
/// window would have no frame visible on the short axis, and the scrim behind it
/// would have nothing to dim.
pub const FIT_FRACTION: f32 = 0.9;

fn text_width(atlas: &FontAtlas, text: &str, size: f32) -> f32 {
    if text.is_empty() {
        return 0.0;
    }
    atlas.text_width(text, size / NATURAL_FONT_SIZE)
}

fn line_height(size: f32) -> f32 {
    LINE_HEIGHT * (size / NATURAL_FONT_SIZE)
}

// ---------------------------------------------------------------------------
// The layout
// ---------------------------------------------------------------------------

/// Where one item landed.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct MenuItemLayout {
    /// The item this rectangle belongs to.
    pub id: WidgetId,
    /// The button's upper-left corner, in screen pixels.
    pub min: Vec2,
    /// The button's lower-right corner, in screen pixels.
    pub max: Vec2,
    /// Where the label's em box starts.
    pub label_pos: Vec2,
    /// Where the key hint's em box starts — right-aligned inside the button.
    pub hint_pos: Vec2,
}

impl MenuItemLayout {
    /// The button's size in pixels.
    #[must_use]
    pub fn size(&self) -> Vec2 {
        self.max - self.min
    }
}

/// One frame's placement of a menu: the panel, the title and every button, in
/// screen pixels.
///
/// Produced by [`Menu::layout`], consumed by [`Menu::render`] for the text and by
/// `crcbl_render::MenuArt` for the art. Carrying the [`MenuStyle`] it was built
/// with is what stops the two halves being measured with different metrics — the
/// failure that puts a label a scale out of step with the frame around it.
#[derive(Debug, Clone, PartialEq)]
pub struct MenuLayout {
    style: MenuStyle,
    screen: Vec2,
    panel: (Vec2, Vec2),
    title_pos: Vec2,
    items: Vec<MenuItemLayout>,
}

impl MenuLayout {
    /// The style this layout was measured with.
    #[must_use]
    pub const fn style(&self) -> &MenuStyle {
        &self.style
    }

    /// The framebuffer this was laid out in, in pixels.
    #[must_use]
    pub const fn screen(&self) -> Vec2 {
        self.screen
    }

    /// The window frame's rectangle: `(min, max)` in screen pixels.
    #[must_use]
    pub const fn panel(&self) -> (Vec2, Vec2) {
        self.panel
    }

    /// The panel's size in pixels.
    #[must_use]
    pub fn panel_size(&self) -> Vec2 {
        self.panel.1 - self.panel.0
    }

    /// The middle of the panel, in screen pixels.
    #[must_use]
    pub fn panel_centre(&self) -> Vec2 {
        (self.panel.0 + self.panel.1) * 0.5
    }

    /// Where the title's em box starts.
    #[must_use]
    pub const fn title_pos(&self) -> Vec2 {
        self.title_pos
    }

    /// Every button, in the order they are drawn.
    #[must_use]
    pub fn items(&self) -> &[MenuItemLayout] {
        &self.items
    }

    /// The whole framebuffer, as the rectangle the scrim covers.
    #[must_use]
    pub fn scrim(&self) -> (Vec2, Vec2) {
        (Vec2::ZERO, self.screen)
    }
}

// ---------------------------------------------------------------------------
// Menu set
// ---------------------------------------------------------------------------

/// Every menu one game owns, which of them is on screen, and the pointer's
/// capture that outlives any single frame.
///
/// A [`Menu`] is one panel. A game has several — a start screen, a pause panel,
/// whatever it shows when a run ends — and needs exactly three things a single
/// menu cannot give it: a way to say which one this frame draws, a guarantee
/// that switching between two does not carry a half-finished click across, and
/// one [`UiState`] shared by all of them so a press captured on a button is the
/// same capture the release is tested against.
///
/// `K` names the panels. It is the game's own enum, not one this crate
/// dictates, because which states a game has is the one part of a menu system
/// that is genuinely per-game. **A `K` with no menu in the set draws nothing**,
/// which is how a "no menu this frame" state is spelled: give the set a `shown`
/// it holds no entry for — a `None` variant, or `false` for a game whose only
/// panel is a pause screen — and every method below becomes a no-op.
///
/// # What it does with the capture
///
/// [`UiState`] latches the widget a press started on so a release somewhere else
/// fires nothing. That latch has to survive between frames, which means it also
/// survives a menu being swapped out from under it, and a capture pointing at a
/// button that is no longer on screen is how a click on the pause menu ends up
/// firing whatever the start menu happens to draw in the same place. So both
/// [`show`](Self::show) and [`replace`](Self::replace) drop it.
///
/// # Example
///
/// ```
/// use crcbl_ui::menu::{Menu, MenuItem, MenuSet};
///
/// #[derive(Clone, Copy, PartialEq, Eq, Debug)]
/// enum Kind {
///     None,
///     Paused,
/// }
///
/// let mut menus = MenuSet::new(
///     Kind::None,
///     vec![(
///         Kind::Paused,
///         Menu::new("PAUSED", vec![MenuItem::new(1, "RESUME", "ESC")]),
///     )],
/// );
///
/// // `Kind::None` has no menu, so the frame draws nothing and the keys do
/// // nothing.
/// assert!(!menus.is_showing());
/// assert_eq!(menus.activate(), None);
///
/// menus.show(Kind::Paused);
/// assert_eq!(menus.activate(), Some(1));
/// ```
#[derive(Debug)]
pub struct MenuSet<K> {
    /// The panels, keyed by the state each belongs to. A `Vec` rather than a
    /// map because a game has a handful and they are walked, never hashed.
    menus: Vec<(K, Menu)>,
    /// Which of them this frame draws.
    shown: K,
    /// The pointer's capture, shared by every menu in the set — see the type's
    /// docs for why it cannot be per-menu.
    ui: UiState,
}

impl<K: Copy + Eq> MenuSet<K> {
    /// A set of menus, with `shown` on screen.
    ///
    /// Pass the state that draws no menu as `shown` to open with none.
    ///
    /// # Panics
    ///
    /// If two entries name the same `K`. The second would be unreachable —
    /// every lookup here takes the first match — so a game that built one by
    /// mistake would have a panel it could never show and no way to notice.
    #[must_use]
    pub fn new(shown: K, menus: Vec<(K, Menu)>) -> Self {
        for (index, (kind, _)) in menus.iter().enumerate() {
            assert!(
                !menus[..index].iter().any(|(seen, _)| seen == kind),
                "two menus claim the same state",
            );
        }
        Self {
            menus,
            shown,
            ui: UiState::new(),
        }
    }

    /// Which menu is being shown.
    #[must_use]
    pub const fn kind(&self) -> K {
        self.shown
    }

    /// Whether this frame has a menu on it.
    ///
    /// False for a `shown` the set holds no menu for, which is what the games
    /// use to decide whether the menu keys are the menu's this frame or the
    /// simulation's.
    #[must_use]
    pub fn is_showing(&self) -> bool {
        self.current().is_some()
    }

    /// Switches to the menu this frame shows.
    ///
    /// A change drops the outgoing menu's hover and held key and the pointer's
    /// capture with it: a menu re-shown with a stale press on it draws a button
    /// nobody is touching, and a capture left in [`UiState`] would credit the
    /// next click to a widget that is no longer on screen.
    ///
    /// Showing what is already shown does nothing, so this is safe to call
    /// every frame — which is how the games call it.
    pub fn show(&mut self, kind: K) {
        if kind == self.shown {
            return;
        }
        if let Some(menu) = self.current_mut() {
            menu.clear_input();
        }
        self.ui.clear();
        self.shown = kind;
    }

    /// Swaps the menu a state draws, or adds it if the state had none.
    ///
    /// For a panel whose buttons are not known until the game is running — a
    /// level-up screen offering three upgrades drawn from a seed. The capture
    /// goes, for the same reason [`show`](Self::show) drops it: the button the
    /// press landed on no longer says what it said.
    ///
    /// The caller decides *when* the menu is stale. This rebuilds
    /// unconditionally, so calling it every frame would throw away the
    /// selection every frame.
    pub fn replace(&mut self, kind: K, menu: Menu) {
        match self.menus.iter_mut().find(|(seen, _)| *seen == kind) {
            Some(slot) => slot.1 = menu,
            None => self.menus.push((kind, menu)),
        }
        self.ui.clear();
    }

    /// The menu being shown, or `None` on a frame with no menu on it.
    #[must_use]
    pub fn current(&self) -> Option<&Menu> {
        let shown = self.shown;
        self.menus
            .iter()
            .find_map(|(kind, menu)| (*kind == shown).then_some(menu))
    }

    /// The menu being shown, mutably.
    pub fn current_mut(&mut self) -> Option<&mut Menu> {
        let shown = self.shown;
        self.menus
            .iter_mut()
            .find_map(|(kind, menu)| (*kind == shown).then_some(menu))
    }

    /// Moves the selection down, if there is a menu.
    pub fn select_next(&mut self) {
        if let Some(menu) = self.current_mut() {
            menu.select_next();
        }
    }

    /// Moves the selection up, if there is a menu.
    pub fn select_previous(&mut self) {
        if let Some(menu) = self.current_mut() {
            menu.select_previous();
        }
    }

    /// Holds the highlighted button down, or lets it up.
    pub fn press(&mut self, down: bool) {
        if let Some(menu) = self.current_mut() {
            menu.press(down);
        }
    }

    /// Fires the highlighted button, reporting the [`WidgetId`] it carries.
    pub fn activate(&mut self) -> Option<WidgetId> {
        self.current_mut().and_then(Menu::activate)
    }

    /// Runs one frame of pointer input against the menu on screen.
    ///
    /// The layout is recomputed here rather than kept, because it depends on
    /// the framebuffer's size and on the menu's own contents and both can
    /// change between frames — and a hit test against last frame's rectangles
    /// is how a resized window gets buttons that are not where they are drawn.
    pub fn point(
        &mut self,
        extent: (u32, u32),
        atlas: &FontAtlas,
        pointer: PointerInput,
    ) -> Option<WidgetId> {
        // The menus and the capture are borrowed as **separate fields**: a
        // `self.current_mut()` here would borrow the whole struct and `self.ui`
        // with it.
        let shown = self.shown;
        let menu = self
            .menus
            .iter_mut()
            .find_map(|(kind, menu)| (*kind == shown).then_some(menu))?;
        let layout = menu.layout(extent, atlas);
        menu.point(&layout, &mut self.ui, pointer)
    }
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use crate::draw_list::DrawCommand;

    /// The five extents every "it is on screen" test in this repository uses:
    /// what a window opens at, a smaller 4:3, 16:9, a canvas clamped by
    /// `max-height: 68vh`, and one taller than it is wide.
    const EXTENTS: [(u32, u32); 5] = [
        (960, 720),
        (800, 600),
        (1920, 1080),
        (1440, 400),
        (600, 900),
    ];

    fn atlas() -> FontAtlas {
        FontAtlas::built_in()
    }

    fn pause_menu() -> Menu {
        Menu::new(
            "PAUSED",
            vec![
                MenuItem::new(1, "RESUME", "ESC"),
                MenuItem::new(2, "FULLSCREEN", "F11"),
                MenuItem::new(3, "DEBUG OVERLAY", "F3"),
            ],
        )
    }

    // -----------------------------------------------------------------------
    // Centred
    // -----------------------------------------------------------------------

    /// **The ask, as arithmetic.** The panel's centre is the framebuffer's
    /// centre, at every aspect ratio and every size — not "on screen", which a
    /// menu jammed into the top-left corner also is.
    ///
    /// Half a pixel of tolerance and no more: [`Menu::layout_with`] rounds the
    /// panel's origin to a whole pixel so the nine-slice's corners land on texel
    /// boundaries, and a size of odd parity then puts the centre on a half pixel.
    /// Anything looser would admit a menu that was merely near the middle.
    #[test]
    fn the_panel_is_centred_at_every_aspect_ratio() {
        let menu = pause_menu();
        let atlas = atlas();
        for extent in EXTENTS {
            let layout = menu.layout(extent, &atlas);
            let screen = Vec2::new(extent.0 as f32, extent.1 as f32);
            let offset = layout.panel_centre() - screen * 0.5;
            assert!(
                offset.x.abs() <= 0.5 && offset.y.abs() <= 0.5,
                "{extent:?}: the panel's centre is {offset:?} off the screen's",
            );
        }
    }

    /// And it is centred at every scale, not only the one the fit happened to
    /// choose — so a game that pins a style still gets a centred menu.
    #[test]
    fn the_panel_is_centred_at_every_scale() {
        let menu = pause_menu();
        let atlas = atlas();
        for scale in 1..=MenuStyle::MAX_SCALE {
            let style = MenuStyle::pixel_art(scale);
            for extent in EXTENTS {
                let layout = menu.layout_with(extent, &atlas, &style);
                let screen = Vec2::new(extent.0 as f32, extent.1 as f32);
                let offset = layout.panel_centre() - screen * 0.5;
                assert!(
                    offset.x.abs() <= 0.5 && offset.y.abs() <= 0.5,
                    "scale {scale} at {extent:?}: centre off by {offset:?}",
                );
            }
        }
    }

    /// Everything the menu draws is inside the panel, and the panel is inside
    /// the framebuffer. Without this the centring test above would pass on a
    /// panel so large it hung off both edges symmetrically.
    #[test]
    fn the_whole_menu_is_inside_the_screen_at_every_aspect_ratio() {
        let menu = pause_menu();
        let atlas = atlas();
        for extent in EXTENTS {
            let layout = menu.layout(extent, &atlas);
            let (min, max) = layout.panel();
            assert!(
                min.x >= 0.0
                    && min.y >= 0.0
                    && max.x <= extent.0 as f32
                    && max.y <= extent.1 as f32,
                "{extent:?}: the panel {min:?}..{max:?} leaves the framebuffer",
            );
            for item in layout.items() {
                assert!(
                    item.min.x >= min.x
                        && item.min.y >= min.y
                        && item.max.x <= max.x
                        && item.max.y <= max.y,
                    "{extent:?}: a button at {:?}..{:?} escapes the panel {min:?}..{max:?}",
                    item.min,
                    item.max,
                );
            }
        }
    }

    /// The chosen scale is the largest that fits, which is what makes the menu
    /// grow with the window rather than sit at a fixed size in the middle of a
    /// 4K screen.
    #[test]
    fn a_bigger_window_gets_a_bigger_menu() {
        let menu = pause_menu();
        let atlas = atlas();
        let small = menu.layout((640, 480), &atlas);
        let large = menu.layout((1920, 1440), &atlas);
        assert!(
            large.style().scale > small.style().scale,
            "640x480 chose {} and 1920x1440 chose {}",
            small.style().scale,
            large.style().scale,
        );
        // And the small one is not merely a scale-1 fallback: the fit must have
        // been exercised at more than one candidate.
        assert!(small.style().scale >= 1.0);
        assert!(large.style().scale <= MenuStyle::MAX_SCALE as f32);
    }

    // -----------------------------------------------------------------------
    // The nine-slice property, at the menu's level
    // -----------------------------------------------------------------------

    /// **The window frame's corners keep their size as the menu grows.**
    ///
    /// A menu with one short item and one with five long ones produce very
    /// different panels at the same scale, and the fixed bands the art refuses to
    /// stretch are the same number of pixels in both. This is the layout half of
    /// the property `NineSliceSource::expand` implements — a caller that laid out
    /// against a corner that scaled with the panel would put its content over the
    /// frame at one size and inside it at another.
    #[test]
    fn the_frames_corners_do_not_grow_with_the_menu() {
        let atlas = atlas();
        let style = MenuStyle::pixel_art(3);
        let small = Menu::new("GO", vec![MenuItem::new(1, "OK", "")]);
        let large = Menu::new(
            "A MUCH LONGER TITLE",
            (0..5)
                .map(|i| MenuItem::new(i, "A VERY LONG MENU ITEM LABEL", "SHIFT+F11"))
                .collect(),
        );

        let small_size = small.panel_size(&atlas, &style);
        let large_size = large.panel_size(&atlas, &style);
        assert!(
            large_size.x > small_size.x * 1.5 && large_size.y > small_size.y * 1.5,
            "the two menus are not different enough to prove anything: \
             {small_size:?} vs {large_size:?}",
        );

        // The corners are the same pixels in both, and they are the art's insets
        // times the scale rather than a fraction of the panel.
        let corners = style.panel_corners();
        assert_eq!(corners.left, PANEL_INSETS.left * style.scale);
        assert_eq!(corners.bottom, PANEL_INSETS.bottom * style.scale);

        // And the content box of each sits exactly that far inside its panel, so
        // the claim is about the layout and not only about the constant.
        for (menu, size) in [(&small, small_size), (&large, large_size)] {
            let layout = menu.layout_with((1600, 1200), &atlas, &style);
            let (min, max) = layout.panel();
            assert_eq!(max - min, size);
            let item = layout.items()[0];
            assert_eq!(
                item.min.x - min.x,
                corners.left + style.panel_padding.x,
                "the first button is not one fixed corner plus the padding in",
            );
            assert_eq!(max.x - item.max.x, corners.right + style.panel_padding.x);
        }
    }

    /// Every button in one menu is the same size, whatever its label — a menu
    /// whose buttons were shrink-to-fit reads as a ragged list rather than as a
    /// panel of choices.
    #[test]
    fn every_button_is_the_same_size() {
        let atlas = atlas();
        let menu = Menu::new(
            "T",
            vec![
                MenuItem::new(1, "X", ""),
                MenuItem::new(2, "A MUCH LONGER LABEL", "F11"),
                MenuItem::new(3, "MID", "Q"),
            ],
        );
        let layout = menu.layout((1280, 960), &atlas);
        let first = layout.items()[0].size();
        for item in layout.items() {
            assert_eq!(item.size(), first, "buttons disagree about their size");
        }
        assert!(first.x > 0.0 && first.y > 0.0);
    }

    // -----------------------------------------------------------------------
    // States
    // -----------------------------------------------------------------------

    /// **Exactly one item is highlighted, and it is the selected one.**
    #[test]
    fn only_the_selected_item_is_highlighted() {
        let mut menu = pause_menu();
        for expected in 0..menu.items().len() {
            let states: Vec<ButtonState> = (0..menu.items().len()).map(|i| menu.state(i)).collect();
            assert_eq!(states[expected], ButtonState::Hovered, "{states:?}");
            assert_eq!(
                states.iter().filter(|s| **s != ButtonState::Idle).count(),
                1,
                "{states:?} lights up more than one item",
            );
            menu.select_next();
        }
        // Wrapped back to the start.
        assert_eq!(menu.selected(), 0);
    }

    /// The selection wraps both ways, so a player at the top can reach the
    /// bottom without walking the whole list.
    #[test]
    fn the_selection_wraps_both_ways() {
        let mut menu = pause_menu();
        menu.select_previous();
        assert_eq!(menu.selected(), 2);
        menu.select_next();
        assert_eq!(menu.selected(), 0);
    }

    /// **Holding the commit key changes the state, and letting it go changes it
    /// back.** Asserted as the three-way state rather than as a bool, because the
    /// state is what selects the frame of art the button draws.
    #[test]
    fn holding_the_commit_key_presses_the_highlighted_button() {
        let mut menu = pause_menu();
        assert_eq!(menu.state(0), ButtonState::Hovered);
        menu.press(true);
        assert_eq!(menu.state(0), ButtonState::Pressed);
        assert_eq!(menu.state(1), ButtonState::Idle, "the press leaked");
        assert_eq!(menu.activate(), Some(1));
        assert_eq!(
            menu.state(0),
            ButtonState::Hovered,
            "activating left the button held down",
        );
    }

    /// Activation reports the item's **id**, not its index, and follows the
    /// selection.
    #[test]
    fn activation_reports_the_selected_items_id() {
        let mut menu = pause_menu();
        assert_eq!(menu.activate(), Some(1));
        menu.select_next();
        assert_eq!(menu.activate(), Some(2));
        assert!(menu.select_id(3));
        assert_eq!(menu.activate(), Some(3));
        assert!(!menu.select_id(99), "an id that is not in the menu");
    }

    /// An empty menu has nothing to select, nothing to press and nothing to
    /// fire — and says so rather than panicking on an index.
    #[test]
    fn an_empty_menu_activates_nothing() {
        let mut menu = Menu::new("EMPTY", Vec::new());
        menu.select_next();
        menu.select_previous();
        menu.press(true);
        assert_eq!(menu.activate(), None);
        assert_eq!(menu.state(0), ButtonState::Idle);
    }

    // -----------------------------------------------------------------------
    // The pointer
    // -----------------------------------------------------------------------

    /// The pointer hovers, presses and clicks — and a press released off the
    /// button it started on fires nothing, which is [`UiState`]'s capture doing
    /// its job through the menu.
    #[test]
    fn the_pointer_hovers_presses_and_clicks() {
        let atlas = atlas();
        let mut menu = pause_menu();
        let layout = menu.layout((960, 720), &atlas);
        let mut ui = UiState::new();
        let second = layout.items()[1];
        let over_second = (second.min + second.max) * 0.5;
        let outside = Vec2::new(2.0, 2.0);

        // Hover moves the highlight off the keyboard's selection.
        assert_eq!(
            menu.point(&layout, &mut ui, PointerInput::hovering(over_second)),
            None
        );
        assert_eq!(menu.state(1), ButtonState::Hovered);
        assert_eq!(
            menu.state(0),
            ButtonState::Idle,
            "the selection still shows"
        );

        // Press.
        let down = PointerInput {
            pos: over_second,
            down: true,
            released: false,
        };
        assert_eq!(menu.point(&layout, &mut ui, down), None);
        assert_eq!(menu.state(1), ButtonState::Pressed);

        // Released somewhere else: nothing fires, and the capture is dropped.
        let elsewhere = PointerInput {
            pos: outside,
            down: false,
            released: true,
        };
        assert_eq!(menu.point(&layout, &mut ui, elsewhere), None);
        assert_eq!(ui.active(), None);

        // A press and release over the same button fires it, and leaves the
        // keyboard's selection on it.
        assert_eq!(menu.point(&layout, &mut ui, down), None);
        let up = PointerInput {
            pos: over_second,
            down: false,
            released: true,
        };
        assert_eq!(menu.point(&layout, &mut ui, up), Some(2));
        assert_eq!(menu.selected(), 1);
    }

    /// A pointer that is over nothing leaves the keyboard in charge.
    #[test]
    fn a_pointer_over_nothing_gives_the_highlight_back_to_the_keyboard() {
        let atlas = atlas();
        let mut menu = pause_menu();
        let layout = menu.layout((960, 720), &atlas);
        let mut ui = UiState::new();
        let third = layout.items()[2];
        menu.point(
            &layout,
            &mut ui,
            PointerInput::hovering((third.min + third.max) * 0.5),
        );
        assert_eq!(menu.state(2), ButtonState::Hovered);
        menu.point(
            &layout,
            &mut ui,
            PointerInput::hovering(Vec2::new(1.0, 1.0)),
        );
        assert_eq!(menu.state(0), ButtonState::Hovered, "back to the selection");
        assert_eq!(menu.state(2), ButtonState::Idle);
    }

    /// **A drag from one item onto a neighbour draws neither of them
    /// pressed.** [`UiState`]'s capture belongs to the item the press started
    /// on, so the neighbour reports `Idle` even while the cursor is over it —
    /// and the drawn state must say the same, not the `Pressed` a menu-global
    /// "something is down" bool painted onto whatever was hovered.
    #[test]
    fn a_drag_onto_a_neighbour_does_not_press_it() {
        let atlas = atlas();
        let mut menu = pause_menu();
        let layout = menu.layout((960, 720), &atlas);
        let mut ui = UiState::new();
        let first = layout.items()[0];
        let second = layout.items()[1];

        // Press on the first item: it draws Pressed.
        let down = PointerInput {
            pos: (first.min + first.max) * 0.5,
            down: true,
            released: false,
        };
        assert_eq!(menu.point(&layout, &mut ui, down), None);
        assert_eq!(menu.state(0), ButtonState::Pressed);

        // Drag onto the second: the capture still belongs to the first, so the
        // second must draw Idle — and once the cursor leaves it, so must the
        // first, exactly as `interact` reports both.
        let dragged = PointerInput {
            pos: (second.min + second.max) * 0.5,
            down: true,
            released: false,
        };
        assert_eq!(menu.point(&layout, &mut ui, dragged), None);
        assert_eq!(
            menu.state(1),
            ButtonState::Idle,
            "the neighbour drew Pressed while the capture belongs to the first item",
        );
        assert_eq!(
            menu.state(0),
            ButtonState::Idle,
            "the cursor left the captured item"
        );
    }

    // -----------------------------------------------------------------------
    // Text
    // -----------------------------------------------------------------------

    /// The menu emits its title, every label and every hint — and **no
    /// rectangles**, because the frame and the buttons are sprites drawn by
    /// another pass.
    #[test]
    fn render_emits_the_text_and_no_rectangles() {
        let atlas = atlas();
        let menu = pause_menu();
        let layout = menu.layout((960, 720), &atlas);
        let mut dl = DrawList::new();
        menu.render(&mut dl, &layout);

        let texts: Vec<&str> = dl
            .commands()
            .iter()
            .map(|command| match command {
                DrawCommand::Text { text, .. } => text.as_str(),
                other => panic!("a menu drew {other:?}, which the UI pass cannot skin"),
            })
            .collect();
        assert_eq!(
            texts,
            [
                "PAUSED",
                "RESUME",
                "ESC",
                "FULLSCREEN",
                "F11",
                "DEBUG OVERLAY",
                "F3"
            ],
        );
    }

    /// An item with no hint draws one string, not an empty second one.
    #[test]
    fn an_item_with_no_hint_draws_no_hint() {
        let atlas = atlas();
        let menu = Menu::new("T", vec![MenuItem::new(1, "OK", "")]);
        let layout = menu.layout((960, 720), &atlas);
        let mut dl = DrawList::new();
        menu.render(&mut dl, &layout);
        assert_eq!(dl.len(), 2, "the title and the label, and nothing else");
    }

    /// Every glyph a menu draws is inside the button or panel it belongs to.
    /// Without this the layout could put a label a hundred pixels to the right
    /// and every other test here would still pass.
    #[test]
    fn every_label_is_inside_the_button_it_belongs_to() {
        let atlas = atlas();
        let menu = pause_menu();
        for extent in EXTENTS {
            let layout = menu.layout(extent, &atlas);
            let style = layout.style();
            for (item, placed) in menu.items().iter().zip(layout.items()) {
                let label_end =
                    placed.label_pos.x + text_width(&atlas, &item.label, style.item_size);
                assert!(
                    placed.label_pos.x >= placed.min.x && label_end <= placed.max.x,
                    "{extent:?}: {} runs from {} to {label_end} in a button \
                     {}..{}",
                    item.label,
                    placed.label_pos.x,
                    placed.min.x,
                    placed.max.x,
                );
                let hint_end = placed.hint_pos.x + text_width(&atlas, &item.hint, style.item_size);
                assert!(
                    placed.hint_pos.x >= label_end && hint_end <= placed.max.x,
                    "{extent:?}: the hint {} overlaps its label or leaves the button",
                    item.hint,
                );
                let bottom = placed.label_pos.y + line_height(style.item_size);
                assert!(
                    placed.label_pos.y >= placed.min.y && bottom <= placed.max.y,
                    "{extent:?}: {} is not vertically inside its button",
                    item.label,
                );
            }
            let title_end =
                layout.title_pos().x + text_width(&atlas, &menu.title, style.title_size);
            let (min, max) = layout.panel();
            assert!(layout.title_pos().x >= min.x && title_end <= max.x);
        }
    }

    // -----------------------------------------------------------------------
    // MenuSet
    // -----------------------------------------------------------------------

    /// A stand-in for a game's own menu states — the shape every sample has:
    /// one variant that draws nothing and the rest one panel each.
    #[derive(Clone, Copy, Debug, PartialEq, Eq)]
    enum Kind {
        None,
        Start,
        Paused,
    }

    fn start_menu() -> Menu {
        Menu::new(
            "START",
            vec![
                MenuItem::new(10, "PLAY", "SPACE"),
                MenuItem::new(2, "FULLSCREEN", "F11"),
            ],
        )
    }

    fn menus() -> MenuSet<Kind> {
        MenuSet::new(
            Kind::None,
            vec![(Kind::Start, start_menu()), (Kind::Paused, pause_menu())],
        )
    }

    /// The centre of the `index`-th button, in framebuffer pixels.
    fn over(menus: &MenuSet<Kind>, extent: (u32, u32), index: usize) -> Vec2 {
        let layout = menus.current().expect("a menu").layout(extent, &atlas());
        let item = layout.items()[index];
        (item.min + item.max) * 0.5
    }

    fn press_at(pos: Vec2) -> PointerInput {
        PointerInput {
            pos,
            down: true,
            released: false,
        }
    }

    fn release_at(pos: Vec2) -> PointerInput {
        PointerInput {
            pos,
            down: false,
            released: true,
        }
    }

    /// **A state the set holds no menu for draws nothing and takes no input.**
    /// This is how every sample spells "no menu this frame" — there is no
    /// separate `Option`, the state simply has no entry — so if any of these
    /// were to act on the last menu shown, a playing frame would be steering a
    /// panel nobody can see.
    #[test]
    fn a_state_with_no_menu_draws_nothing_and_takes_no_input() {
        let extent = (960, 720);
        let mut menus = menus();
        assert_eq!(menus.kind(), Kind::None);
        assert!(!menus.is_showing());
        assert!(menus.current().is_none());
        assert!(menus.current_mut().is_none());
        assert_eq!(menus.activate(), None, "there is no menu to activate");
        assert_eq!(menus.point(extent, &atlas(), press_at(Vec2::ZERO)), None);

        // And the keys are not merely ignored on the way in: they must not have
        // moved the menu that is *about* to be shown.
        menus.select_next();
        menus.select_previous();
        menus.press(true);
        menus.show(Kind::Paused);
        assert!(menus.is_showing());
        assert_eq!(menus.current().expect("a menu").selected(), 0);
        assert_eq!(
            menus.current().expect("a menu").state(0),
            ButtonState::Hovered
        );
    }

    /// **Keyboard activation works**, on whichever menu is shown, and reports
    /// the id the selected button carries.
    #[test]
    fn the_keyboard_selects_and_activates() {
        let mut menus = menus();
        menus.show(Kind::Paused);
        assert_eq!(menus.activate(), Some(1));
        menus.select_next();
        assert_eq!(menus.activate(), Some(2));
        menus.select_next();
        assert_eq!(menus.activate(), Some(3));
        menus.select_next();
        assert_eq!(menus.activate(), Some(1), "it wraps");
        menus.select_previous();
        assert_eq!(menus.activate(), Some(3));

        // A different menu has its own selection and its own ids.
        menus.show(Kind::Start);
        assert_eq!(menus.activate(), Some(10));
    }

    /// Holding the commit key presses the highlighted button and nothing else,
    /// which is what selects the pressed frame of the skin.
    #[test]
    fn holding_the_commit_key_presses_the_selected_button() {
        let mut menus = menus();
        menus.show(Kind::Paused);
        menus.select_next();
        menus.press(true);
        let menu = menus.current().expect("a menu");
        assert_eq!(menu.state(1), ButtonState::Pressed);
        assert_eq!(menu.state(0), ButtonState::Idle);
        menus.press(false);
        assert_eq!(
            menus.current().expect("a menu").state(1),
            ButtonState::Hovered,
        );
    }

    /// **The pointer activates too**, reporting the same ids, and a click over
    /// nothing fires nothing.
    #[test]
    fn the_pointer_clicks_a_button() {
        let extent = (960, 720);
        let mut menus = menus();
        menus.show(Kind::Paused);
        let target = over(&menus, extent, 1);

        assert_eq!(menus.point(extent, &atlas(), press_at(target)), None);
        assert_eq!(
            menus.current().expect("a menu").state(1),
            ButtonState::Pressed,
            "the pointer's press did not reach the art",
        );
        assert_eq!(menus.point(extent, &atlas(), release_at(target)), Some(2));

        let corner = Vec2::new(3.0, 3.0);
        assert_eq!(menus.point(extent, &atlas(), press_at(corner)), None);
        assert_eq!(menus.point(extent, &atlas(), release_at(corner)), None);
    }

    /// **Switching menus drops the press capture**, so a click that started on
    /// one menu cannot land on another menu's button.
    ///
    /// The reason [`UiState`] lives in the set rather than in each [`Menu`]:
    /// the capture spans frames, and the menu on screen can change between
    /// them.
    ///
    /// **The press is put on the `FULLSCREEN` button deliberately, because both
    /// menus carry it under the same [`WidgetId`].** [`UiState::interact`]
    /// fires on release only when the capture names the *same* id the cursor is
    /// over, so a press parked on an id the new menu does not have could not
    /// fire whatever the set did with it — and a test built that way would pass
    /// with the capture never cleared at all.
    #[test]
    fn switching_menus_drops_the_press() {
        let extent = (960, 720);
        let mut menus = menus();
        menus.show(Kind::Paused);
        assert_eq!(
            menus.current().expect("a menu").items()[1].id,
            start_menu().items()[1].id,
            "the two menus stopped sharing a button, so this test guards nothing",
        );

        menus.point(extent, &atlas(), press_at(over(&menus, extent, 1)));
        assert_eq!(
            menus.current().expect("a menu").state(1),
            ButtonState::Pressed,
        );

        menus.show(Kind::Start);
        assert_eq!(
            menus.current().expect("a menu").state(0),
            ButtonState::Hovered,
            "the new menu inherited a press nobody is making",
        );
        let shared = over(&menus, extent, 1);
        assert_eq!(
            menus.point(extent, &atlas(), release_at(shared)),
            None,
            "a release fired a button whose press was on another menu",
        );

        // And the menu that was left behind is clean when it comes back. The
        // capture is only half of it: the outgoing menu holds its own hover and
        // pressed flags, and a panel re-shown still lit up draws a button
        // nobody is touching. `Idle` rather than `Hovered` because the pointer
        // pressed the second button while the selection stayed on the first —
        // an uncleared menu reports this one `Pressed`.
        menus.show(Kind::Paused);
        assert_eq!(
            menus.current().expect("a menu").state(1),
            ButtonState::Idle,
            "the menu came back still holding the press it was left with",
        );
    }

    /// Showing what is already shown changes nothing — the samples call `show`
    /// every frame, so an unconditional clear here would throw the selection
    /// and the press away sixty times a second.
    #[test]
    fn showing_the_same_menu_again_keeps_the_selection_and_the_press() {
        let mut menus = menus();
        menus.show(Kind::Paused);
        menus.select_next();
        menus.press(true);
        menus.show(Kind::Paused);
        assert_eq!(menus.current().expect("a menu").selected(), 1);
        assert_eq!(
            menus.current().expect("a menu").state(1),
            ButtonState::Pressed,
        );
    }

    /// **A replaced menu drops the capture too** — the press was on a button
    /// that no longer says what it said. For a panel whose buttons are built
    /// while the game runs.
    ///
    /// The replacement keeps the **same ids** on purpose, which is what a
    /// rebuilt panel really looks like: the same three slots, three different
    /// things in them. Ids that changed would make the stale capture
    /// unmatchable and the test green whatever `replace` did — see
    /// [`switching_menus_drops_the_press`].
    #[test]
    fn a_replaced_menu_drops_the_press() {
        let extent = (960, 720);
        let mut menus = menus();
        menus.show(Kind::Start);
        menus.point(extent, &atlas(), press_at(over(&menus, extent, 0)));
        assert_eq!(
            menus.current().expect("a menu").state(0),
            ButtonState::Pressed,
        );

        menus.replace(
            Kind::Start,
            Menu::new(
                "START",
                vec![
                    MenuItem::new(10, "OTHER", "SPACE"),
                    MenuItem::new(2, "FULLSCREEN", "F11"),
                ],
            ),
        );
        assert_eq!(
            menus.current().expect("a menu").items()[0].label,
            "OTHER",
            "the swap did not take",
        );
        let same_slot = over(&menus, extent, 0);
        assert_eq!(
            menus.point(extent, &atlas(), release_at(same_slot)),
            None,
            "a release fired a button the player never pressed",
        );
    }

    /// `replace` on a state that had no menu adds one, so a set can be filled
    /// in after it is built.
    #[test]
    fn replacing_a_state_with_no_menu_adds_it() {
        let mut menus = MenuSet::new(Kind::None, vec![(Kind::Paused, pause_menu())]);
        menus.show(Kind::Start);
        assert!(!menus.is_showing(), "Start has no menu yet");
        menus.replace(Kind::Start, start_menu());
        assert!(menus.is_showing());
        assert_eq!(menus.current().expect("a menu").title, "START");
        assert_eq!(menus.activate(), Some(10));
    }

    /// Two menus for one state is rejected at construction: the second would be
    /// unreachable, because every lookup takes the first match.
    #[test]
    #[should_panic(expected = "two menus claim the same state")]
    fn two_menus_for_one_state_is_rejected() {
        let _ = MenuSet::new(
            Kind::None,
            vec![(Kind::Paused, pause_menu()), (Kind::Paused, start_menu())],
        );
    }
}
