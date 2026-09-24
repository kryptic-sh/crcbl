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
//! **The menu is built on the element tree.** [`Menu::layout`] and
//! [`Menu::render`] build the same [`crate::tree`] of blocks and spans — a
//! screen, the scrim, the panel, the title and a row per item — lay it out on
//! Taffy and read the rectangles back or emit it, so the arithmetic that used to
//! place each label is flexbox now. The tree is a view: which row is selected,
//! held or hovered stays in this module's model, and each row is built in the
//! pseudo-class [`Menu::state`] derives, so the keyboard-first API below is
//! unchanged.
//!
//! **The skin is `default.css`'s.** The window frame and the button frames are
//! `border-image`s over the panel and the rows, which frame a row draws is its
//! `:hover` or `:active` rule, and every colour is a rule too. The pictures are
//! a [`MenuSkin`]'s registered images, which [`Menu::render`] binds under the
//! names the sheet's `url()`s use; the art itself is `crcbl-render`'s
//! (`crcbl_render::menu::menu_skin` registers it). The lengths stay
//! [`MenuStyle`]'s, set on the nodes inline, because they are a pixel-art scale
//! times a base metric and a stylesheet has no arithmetic to scale with; the
//! frame's insets reach the layout through them, and `crcbl-render`'s tests
//! read the insets back off the art so the layout and the picture cannot drift.
//!
//! # Why the whole menu is one type and not a `Vec<Button>`
//!
//! A menu has one property a pile of buttons does not: **it is centred as a
//! unit**. Its panel has to be measured before any button can be placed, because
//! the panel's width is the widest button's width and the buttons are centred in
//! the panel. Handing a caller three `Button`s and letting it do the arithmetic
//! is handing every sample the same arithmetic to get subtly differently wrong.
//!
//! # The keyboard is the primary input, and the pointer is optional
//!
//! The samples are played with the keyboard, so a menu that could only be
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
//!
//! # Three kinds of row
//!
//! A row fires, or it carries a value — see [`MenuItemKind`]. A **button**
//! reports its id from the commit key or a click. A **slider** carries a
//! position along a groove and reports nothing; the arrows and a drag move it.
//! A **cycler** carries one choice of a fixed list and reports nothing either:
//! the arrows walk the list and stop at its ends, the commit key and a click
//! walk it forward and round. The two value rows are read back by id —
//! [`Menu::slider`] and [`Menu::cycler`] — rather than fired, so no value ever
//! reaches a game's action table looking like a button press.
//!
//! # Captions
//!
//! [`Menu::subtitle`] is lines of text under the title — the controls, a
//! warning — drawn in the hint's colour. They are not rows: nothing selects,
//! hovers or fires them, so the selection, the pointer and every index into
//! [`Menu::items`] are exactly what they were without them.
//!
//! # Fitting a small window, then scrolling
//!
//! [`Menu::layout_with_font_fitted`] lays a menu out in a registered font at
//! the largest size that fits the window, in two stages:
//!
//! 1. **Shrink.** Every font size and gap shrinks by one factor, in steps of
//!    [`FIT_FONT_STEP`], down to [`MenuStyle::MIN_FONT_SIZE`]; the art stays at
//!    a whole-number scale of at least one, so the frames' corners stay on
//!    whole pixels.
//! 2. **Scroll.** If even the smallest size is too tall, the item list is
//!    capped at the height left and scrolls; the title and the captions stay
//!    put. Only a menu too wide at the smallest size — the list scrolls up and
//!    down only — or too short to show one row is refused, with a
//!    [`MenuFitError`] saying which.
//!
//! A scrolled list follows the keyboard: a selection moved by
//! [`Menu::select_next`] or [`Menu::select_previous`] is scrolled into view by
//! the next layout. [`Menu::scroll_wheel`] moves it by the pointer's wheel
//! instead, until the keyboard next moves the selection. A row scrolled wholly
//! out of view is neither drawn nor hit.

use std::ops::Range;

use glam::Vec2;

use crate::draw_list::{DrawCommand, DrawList};
use crate::font::Font;
use crate::image::{AtlasImage, NineSliceImage};
use crate::style::{Declaration, PseudoClasses, Sides};
use crate::text::FontAtlas;
#[cfg(test)]
use crate::text::LINE_HEIGHT;
use crate::tree::{
    AvailableSpace, FamilyName, Length, LengthAuto, LineHeight, NodeKey, Overflow, Ui,
};
use crate::widget::{
    ButtonSkin, ButtonState, NATURAL_FONT_SIZE, PointerInput, SkinInsets, UiState, WidgetId,
};

// ---------------------------------------------------------------------------
// Skin
// ---------------------------------------------------------------------------

/// The pictures a menu is drawn with: a window frame, a button frame per state
/// and the scrim behind them, all registered in one [`ImageAtlas`](crate::ImageAtlas).
///
/// Every sample's menus use the one `crcbl-render` ships; see
/// `crcbl_render::menu` for why the art lives there.
///
/// [`Menu::render`] binds the panel and the three button images under the
/// names `default.css`'s menu rules give them, and the sheet cuts each at its
/// own `border-image-slice`: the insets a [`NineSliceImage`] carries are the
/// art's, which [`PANEL_INSETS`] and [`BUTTON_INSETS`] restate for the layout
/// and the sheet's slices are held to.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct MenuSkin {
    /// The window frame, nine-sliced round the panel.
    pub panel: NineSliceImage,
    /// The button frames.
    pub buttons: ButtonSkin,
    /// Stretched over the whole framebuffer and tinted by the sheet's
    /// `.menu-scrim` colour, so the art decides its texture and the sheet how
    /// dark it gets.
    pub scrim: AtlasImage,
}

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
    /// The window frame's fixed corners, in pixels.
    pub panel: SkinInsets,
    /// A button skin's fixed corners, in pixels.
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
    /// The least width a [`Slider`] row's groove is given.
    ///
    /// A least, not a width: [`Menu::layout_with`] stretches the groove into
    /// whatever the panel's widest row left over, so a slider beside a long
    /// label is a long slider. This is what stops it collapsing when the slider
    /// row *is* the widest one.
    pub track_width: f32,
    /// How tall a slider's groove is drawn.
    pub track_height: f32,
    /// The handle's size. Its width is also the travel the groove loses at each
    /// end, so the handle stays inside the groove at both extremes.
    pub handle_size: Vec2,
    /// The title's colour.
    ///
    /// **This and the other colours restate `default.css`'s menu rules, which
    /// are what draw them**: a style with different colours lays out the same
    /// menu and draws it in the sheet's. They are here for a caller measuring
    /// what a menu drew — the scrim's alpha over a golden image, say — and
    /// `the_sheet_draws_the_colours_and_the_slices_the_style_restates` holds
    /// [`MenuStyle::pixel_art`]'s to the sheet's.
    pub title_color: [f32; 4],
    /// An item label's colour. See [`MenuStyle::title_color`].
    pub label_color: [f32; 4],
    /// A key hint's colour — dimmer than the label it belongs to. See
    /// [`MenuStyle::title_color`].
    pub hint_color: [f32; 4],
    /// A slider groove's colour, behind the part not filled in. See
    /// [`MenuStyle::title_color`].
    pub track_color: [f32; 4],
    /// The colour of the groove up to the handle. See
    /// [`MenuStyle::title_color`].
    pub fill_color: [f32; 4],
    /// The handle's colour. See [`MenuStyle::title_color`].
    pub handle_color: [f32; 4],
    /// What the frame behind the menu is dimmed with. See
    /// [`MenuStyle::title_color`].
    pub scrim_color: [f32; 4],
}

/// The insets of the window frame `crcbl-render` ships, in **texels**.
///
/// The source figure the pixel corners come from: [`MenuStyle::pixel_art`] is
/// the only place texels become pixels, and it multiplies this by the scale
/// into [`MenuStyle::panel`]. Kept in texels so a menu can be laid out — and
/// the layout tested — with no renderer and no device, and checked against the
/// art by `crcbl_render::menu`'s
/// `the_shipped_art_has_the_insets_the_layout_assumes`, which reads the same
/// figure back off the registered skin and compares.
pub const PANEL_INSETS: SkinInsets = SkinInsets::new(4.0, 4.0, 4.0, 4.0);

/// The insets of the button skin `crcbl-render` ships, in **texels**.
///
/// The source figure [`MenuStyle::button`] is pre-multiplied from —
/// [`MenuStyle::pixel_art`] is the only place texels become pixels.
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

    /// The smallest item font size [`Menu::layout_with_font_fitted`] shrinks
    /// to, in pixels per em.
    ///
    /// Eight: a typical face's x-height is about half an em, so this leaves
    /// lowercase four pixels tall — the fewest the hinter can snap `e`, `c`
    /// and `o` apart in. A window that needs smaller text gets a scrolled
    /// list, or a [`MenuFitError`], rather than a menu nobody can read.
    pub const MIN_FONT_SIZE: f32 = 8.0;

    /// The shipped look, at `scale` device pixels per texel.
    ///
    /// `scale` is clamped to at least one — a menu drawn at zero is not a
    /// smaller menu, it is an invisible one.
    #[must_use]
    pub fn pixel_art(scale: u32) -> Self {
        let scale = scale.max(1) as f32;
        Self {
            scale,
            panel: scaled(PANEL_INSETS, scale),
            button: scaled(BUTTON_INSETS, scale),
            panel_padding: Vec2::new(8.0, 7.0) * scale,
            button_padding: Vec2::new(6.0, 3.0) * scale,
            item_gap: 4.0 * scale,
            title_gap: 8.0 * scale,
            // Twice the atlas's natural height, so the title is a heading rather
            // than a bolder line of the same text.
            title_size: NATURAL_FONT_SIZE * 2.0 * scale,
            item_size: NATURAL_FONT_SIZE * scale,
            hint_gap: 10.0 * scale,
            track_width: 48.0 * scale,
            track_height: 4.0 * scale,
            handle_size: Vec2::new(6.0, 12.0) * scale,
            title_color: [1.0, 0.94, 0.55, 1.0],
            label_color: [0.94, 0.95, 1.0, 1.0],
            hint_color: [0.62, 0.64, 0.82, 1.0],
            // The groove is the darkest of the three and the handle the
            // brightest, so the three read as recess, level and grip without
            // any art behind them.
            track_color: [0.10, 0.11, 0.18, 1.0],
            fill_color: [0.42, 0.46, 0.72, 1.0],
            handle_color: [0.94, 0.95, 1.0, 1.0],
            // Two thirds, in straight alpha: enough that the panel's own dark
            // fill separates from the game behind it, not so much that the
            // player loses track of where the ball was.
            scrim_color: [0.0, 0.0, 0.0, 0.66],
        }
    }

    /// This style with its item font at `item_size`, and every other length
    /// shrunk by the same factor — what [`Menu::layout_with_font_fitted`]
    /// tries.
    ///
    /// The frames' corners are the art's texels at the whole-number scale the
    /// factor leaves, never below one, so the nine-slice stays on whole
    /// pixels. Every other length but the font sizes is rounded to a whole
    /// pixel, which with [`row_line`]'s whole line pitch makes every row a
    /// whole number of pixels tall — what lets a scrolled list build only the
    /// rows in view and still put them where the layout said.
    fn shrunk(&self, item_size: f32) -> Self {
        let factor = item_size / self.item_size;
        let whole = |length: f32| (length * factor).round();
        let art = (self.scale * factor).floor().max(1.0);
        let corners = |insets: SkinInsets| {
            let texels = |length: f32| (length / self.scale * art).round();
            SkinInsets::new(
                texels(insets.left),
                texels(insets.right),
                texels(insets.top),
                texels(insets.bottom),
            )
        };
        Self {
            scale: art,
            panel: corners(self.panel),
            button: corners(self.button),
            panel_padding: Vec2::new(whole(self.panel_padding.x), whole(self.panel_padding.y)),
            button_padding: Vec2::new(whole(self.button_padding.x), whole(self.button_padding.y)),
            item_gap: whole(self.item_gap),
            title_gap: whole(self.title_gap),
            title_size: self.title_size * factor,
            item_size,
            hint_gap: whole(self.hint_gap),
            track_width: whole(self.track_width),
            track_height: whole(self.track_height),
            handle_size: Vec2::new(whole(self.handle_size.x), whole(self.handle_size.y)),
            ..*self
        }
    }

    /// The panel's fixed corners, in device pixels.
    ///
    /// [`MenuStyle::panel`] itself: `pixel_art` pre-multiplies the texel
    /// source figure [`PANEL_INSETS`] by the scale, so the layout reads the
    /// pixel corners through this accessor the same way it always has.
    #[must_use]
    pub fn panel_corners(&self) -> SkinInsets {
        self.panel
    }

    /// A button's fixed corners, in device pixels.
    ///
    /// [`MenuStyle::button`] itself — see [`panel_corners`](Self::panel_corners).
    #[must_use]
    pub fn button_corners(&self) -> SkinInsets {
        self.button
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

/// A value between zero and one that the player drags along a groove.
///
/// **Unitless on purpose.** A slider that knew it was showing an exposure, a
/// volume or a mouse sensitivity would need to know that exposure is a ratio
/// and volume is decibels, and it would be wrong for the next one. The caller
/// owns the mapping in both directions: it writes a position in with
/// [`Menu::set_slider`] and reads one back with [`Menu::slider`].
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Slider {
    position: f32,
}

impl Slider {
    /// How far one press of a menu key moves the handle, as a share of the
    /// groove.
    ///
    /// Twenty presses end to end, which is the granularity a player expects of
    /// a volume: fine enough to land where they meant and coarse enough to
    /// cross the range without holding a key down. A pointer drag is not
    /// quantised by this at all — it lands wherever the cursor is.
    pub const KEY_STEP: f32 = 0.05;

    /// A slider whose handle sits `position` of the way along, clamped into
    /// `0.0..=1.0`.
    #[must_use]
    pub fn new(position: f32) -> Self {
        Self {
            position: clamp_unit(position),
        }
    }

    /// Where the handle sits: `0.0` at the left end of the groove, `1.0` at the
    /// right. Always in that range.
    #[must_use]
    pub const fn position(&self) -> f32 {
        self.position
    }

    /// Moves the handle, clamped into `0.0..=1.0`.
    pub fn set_position(&mut self, position: f32) {
        self.position = clamp_unit(position);
    }
}

/// Into `0.0..=1.0`, with `NaN` landing at zero.
///
/// `f32::clamp` propagates a `NaN` rather than replacing it, and a `NaN`
/// position is a handle drawn nowhere and a caller handed back a number it
/// cannot map. Zero is the end of the groove, which is a place.
fn clamp_unit(position: f32) -> f32 {
    if position.is_nan() {
        0.0
    } else {
        position.clamp(0.0, 1.0)
    }
}

/// A row that holds one choice of a fixed, ordered list — a display mode, a
/// frame cap, an on/off switch.
///
/// It knows how many choices there are and which is chosen, and **not what
/// they are**: the caller owns the list and its spellings, and writes the
/// chosen one's caption in as the row's hint, exactly as a [`Slider`] leaves
/// the mapping from a position to a value with the caller. What the widget
/// owns is the stepping, and it has two rules because a player's hands have
/// two: the arrows walk the list and **stop at its ends** — [`Menu::nudge_cycler`],
/// which draws a chevron only on the side a step would go — and the commit
/// key or a click walks it **forward and round** — [`Menu::activate`] and
/// [`Menu::point`] — so one key reaches every choice, which is what a row that
/// was a button did before this existed.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Cycler {
    chosen: usize,
    count: usize,
}

impl Cycler {
    /// `count` choices with `chosen` picked, clamped to the last of them.
    ///
    /// No choices at all is a cycler with nothing chosen: [`Cycler::chosen`]
    /// reads zero and every step is refused, rather than a wrap that would
    /// divide by the count.
    #[must_use]
    pub fn new(count: usize, chosen: usize) -> Self {
        let mut cycler = Self { chosen: 0, count };
        cycler.choose(chosen);
        cycler
    }

    /// How many choices there are.
    #[must_use]
    pub const fn count(&self) -> usize {
        self.count
    }

    /// Which choice is picked: an index into the caller's list, always below
    /// [`Cycler::count`] unless that is zero.
    #[must_use]
    pub const fn chosen(&self) -> usize {
        self.chosen
    }

    /// Picks a choice, clamped to the last one.
    pub fn choose(&mut self, index: usize) {
        self.chosen = index.min(self.count.saturating_sub(1));
    }

    /// Steps one choice along, and reports whether the choice changed.
    ///
    /// Without `wrap` a step past either end stays at that end; with it the
    /// step goes round. `false` means the choice is where it was, which at an
    /// end without `wrap` is the ordinary way a held key arrives.
    pub fn step(&mut self, forward: bool, wrap: bool) -> bool {
        if self.count == 0 {
            return false;
        }
        let last = self.count - 1;
        let next = match (forward, wrap) {
            (true, _) if self.chosen < last => self.chosen + 1,
            (true, true) => 0,
            (false, _) if self.chosen > 0 => self.chosen - 1,
            (false, true) => last,
            _ => return false,
        };
        let moved = next != self.chosen;
        self.chosen = next;
        moved
    }

    /// Whether a step in that direction, without a wrap, would move.
    #[must_use]
    pub const fn can_step(&self, forward: bool) -> bool {
        if forward {
            self.chosen + 1 < self.count
        } else {
            self.chosen > 0
        }
    }
}

/// What a cycler's caption is drawn between: a chevron on each side a step
/// would go, and the same width of nothing on a side it would not — so the
/// caption stays put as the chevrons come and go, which needs every glyph to
/// advance alike and the built-in atlas's do.
const BACK_CHEVRON: &str = "< ";
const FORWARD_CHEVRON: &str = " >";
const NO_CHEVRON: &str = "  ";

/// What a menu row *is*: something that fires, or something that carries a
/// value.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum MenuItemKind {
    /// A button. [`Menu::activate`] and a click both report its id.
    Action,
    /// A slider. It never fires — see [`Menu::activate`] — and the caller reads
    /// its value with [`Menu::slider`] instead.
    Slider(Slider),
    /// A cycler. The commit key and a click step it rather than fire it, and
    /// the caller reads which choice is picked with [`Menu::cycler`].
    Cycler(Cycler),
}

/// One line of a menu: what it says, what key does it, and what it is called.
#[derive(Debug, Clone, PartialEq)]
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
    ///
    /// A [`MenuItemKind::Slider`] row has no key of its own to advertise, so
    /// this is where its **value** goes — the formatted number the caller
    /// refreshes as the handle moves. Right-aligned either way. A
    /// [`MenuItemKind::Cycler`] row's is the caption of the choice it holds,
    /// which the caller refreshes as the choice changes; it is drawn between
    /// chevrons — see [`MenuItem::caption`].
    pub hint: String,
    /// Whether this row fires or carries a value.
    pub kind: MenuItemKind,
}

impl MenuItem {
    /// An item with a key hint beside it.
    #[must_use]
    pub fn new(id: WidgetId, label: impl Into<String>, hint: impl Into<String>) -> Self {
        Self {
            id,
            label: label.into(),
            hint: hint.into(),
            kind: MenuItemKind::Action,
        }
    }

    /// A slider row: a label, a groove with its handle `position` of the way
    /// along, and `value` drawn where a key hint would be.
    #[must_use]
    pub fn slider(
        id: WidgetId,
        label: impl Into<String>,
        value: impl Into<String>,
        position: f32,
    ) -> Self {
        Self {
            id,
            label: label.into(),
            hint: value.into(),
            kind: MenuItemKind::Slider(Slider::new(position)),
        }
    }

    /// A cycler row: a label, one of `count` choices with `chosen` picked, and
    /// that choice's `caption` drawn where a key hint would be.
    #[must_use]
    pub fn cycler(
        id: WidgetId,
        label: impl Into<String>,
        caption: impl Into<String>,
        count: usize,
        chosen: usize,
    ) -> Self {
        Self {
            id,
            label: label.into(),
            hint: caption.into(),
            kind: MenuItemKind::Cycler(Cycler::new(count, chosen)),
        }
    }

    /// What this row draws where a key hint goes.
    ///
    /// The hint itself for a button or a slider. For a cycler, the hint between
    /// a `<` and a `>` — each present only on the side a step without a wrap
    /// would go, and replaced by blank of the same width on the side it would
    /// not, so the caption does not shift as they come and go.
    #[must_use]
    pub fn caption(&self) -> std::borrow::Cow<'_, str> {
        match self.kind {
            MenuItemKind::Cycler(cycler) => {
                let chevron = |forward: bool, chevron: &'static str| {
                    if cycler.can_step(forward) {
                        chevron
                    } else {
                        NO_CHEVRON
                    }
                };
                std::borrow::Cow::Owned(format!(
                    "{}{}{}",
                    chevron(false, BACK_CHEVRON),
                    self.hint,
                    chevron(true, FORWARD_CHEVRON)
                ))
            }
            MenuItemKind::Action | MenuItemKind::Slider(_) => {
                std::borrow::Cow::Borrowed(self.hint.as_str())
            }
        }
    }
}

/// A modal menu: a title, some items, and which one is selected.
///
/// Retained across frames — unlike the rest of this crate — because a selection
/// is state by definition, and an immediate-mode menu whose highlight was
/// recomputed from nothing every frame would have no keyboard at all.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct Menu {
    /// The heading, drawn above the items.
    pub title: String,
    /// Lines drawn under the title, centred and in the hint's colour, one
    /// line each — see the module's *Captions*. Empty by default, which draws
    /// exactly the menu there was before captions existed.
    pub subtitle: Vec<String>,
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
    /// The slider row the pointer is dragging, if it is dragging one.
    ///
    /// What makes [`Menu::set_slider`] leave a handle alone while the player
    /// has hold of it: a caller pushing the value it last read back in every
    /// frame would otherwise fight the drag it is reading from.
    dragging: Option<usize>,
    /// How far a scrolled item list is scrolled, in pixels: what the next
    /// [`Menu::layout_with_font_fitted`] starts from before it clamps it to
    /// the list's reach and, while `follow_selection` holds, scrolls the
    /// selection into view. Zero for a list that is not scrolled.
    scroll: f32,
    /// Whether the next fitted layout scrolls the selection into view. Set by
    /// every keyboard move and cleared by [`Menu::scroll_wheel`]: a wheel
    /// snapped back to the selection every frame could never move the list.
    follow_selection: bool,
}

impl Menu {
    /// A menu with a title and its items, the first selected.
    #[must_use]
    pub fn new(title: impl Into<String>, items: Vec<MenuItem>) -> Self {
        Self {
            title: title.into(),
            subtitle: Vec::new(),
            items,
            selected: 0,
            pressed: false,
            pressed_index: None,
            hovered: None,
            dragging: None,
            scroll: 0.0,
            follow_selection: true,
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
    ///
    /// A scrolled list — see [`Menu::layout_with_font_fitted`] — is scrolled
    /// to the new selection by the next layout, by as little as brings it
    /// wholly into view. "As little" is measured from the scroll the menu
    /// holds, which [`Menu::point`] and [`Menu::scroll_wheel`] keep up to
    /// date: a caller that calls neither still always sees the selection, but
    /// held at the edge it scrolled past rather than moving inside a still
    /// list.
    pub fn select_next(&mut self) {
        self.move_selection(1);
    }

    /// Moves the selection up one, wrapping at the start. A scrolled list
    /// follows it, as [`Menu::select_next`] says.
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
        self.follow_selection = true;
        // A keyboard move takes the highlight back off the pointer: otherwise a
        // cursor left resting over an item makes the arrow keys look dead.
        self.hovered = None;
        self.pressed = false;
        self.pressed_index = None;
        self.dragging = None;
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
                self.follow_selection = true;
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
    /// A [`MenuItemKind::Slider`] row reports **nothing**: there is no press of
    /// a commit key that means anything to a value, and an id fired from one
    /// would reach the game's action table as if a button had been clicked. A
    /// [`MenuItemKind::Cycler`] row reports nothing for the same reason, and
    /// the press **steps it forward, round the end** — the one-key walk of the
    /// whole list the caller reads back with [`Menu::cycler`].
    pub fn activate(&mut self) -> Option<WidgetId> {
        self.pressed = false;
        self.pressed_index = None;
        let index = self.hovered.unwrap_or(self.selected);
        match self.items.get_mut(index) {
            Some(item) => match &mut item.kind {
                MenuItemKind::Action => Some(item.id),
                MenuItemKind::Cycler(cycler) => {
                    cycler.step(true, true);
                    None
                }
                MenuItemKind::Slider(_) => None,
            },
            None => None,
        }
    }

    /// Drops any hover and any held key.
    ///
    /// For a menu being taken off the screen: a menu re-shown with a stale
    /// `Pressed` on it draws a button nobody is touching.
    pub const fn clear_input(&mut self) {
        self.hovered = None;
        self.pressed = false;
        self.pressed_index = None;
        self.dragging = None;
    }

    /// Where the handle of the slider row named `id` sits, in `0.0..=1.0`.
    ///
    /// `None` for an id this menu has no row for, and for a row that is a
    /// button rather than a slider.
    #[must_use]
    pub fn slider(&self, id: WidgetId) -> Option<f32> {
        self.items.iter().find_map(|item| match item.kind {
            MenuItemKind::Slider(slider) if item.id == id => Some(slider.position()),
            _ => None,
        })
    }

    /// Moves the handle of the slider row named `id`, and reports whether it
    /// moved.
    ///
    /// **Refused while the player is dragging that row**, which is what lets a
    /// caller push the value back in unconditionally every frame: the state the
    /// caller is mirroring came from this handle in the first place, so the
    /// drag is the newer of the two and writing over it would pin the handle
    /// under the cursor.
    ///
    /// So `false` means one of three things — no such row, not a slider, or the
    /// player has hold of it — and a caller that needs to tell them apart asks
    /// [`Menu::slider`] first.
    pub fn set_slider(&mut self, id: WidgetId, position: f32) -> bool {
        let dragging = self.dragging;
        for (index, item) in self.items.iter_mut().enumerate() {
            if item.id != id {
                continue;
            }
            let MenuItemKind::Slider(slider) = &mut item.kind else {
                return false;
            };
            if dragging == Some(index) {
                return false;
            }
            slider.set_position(position);
            return true;
        }
        false
    }

    /// Moves the highlighted slider one [`Slider::KEY_STEP`], and reports
    /// whether a handle moved.
    ///
    /// **The keyboard's half of a slider**, and until it existed there was no
    /// such half: [`Menu::activate`] reports nothing for a slider row by
    /// design, so a player with no pointer could select a volume and then had
    /// no way to change it. This module's own opening says the keyboard is the
    /// primary input; a row that only a mouse could work was that claim being
    /// false in the one place it is most visible.
    ///
    /// The **highlighted** row, on [`Menu::activate`]'s terms — the hovered one
    /// when a pointer is over one, the selected one otherwise — so the key moves
    /// what the player can see is about to move.
    ///
    /// `false` means nothing moved: the row is a button, there is no row, the
    /// handle is already at that end of the groove, or the pointer has hold of
    /// it. The last is [`Menu::set_slider`]'s rule and it is here for the same
    /// reason: a drag is the newer of the two inputs.
    pub fn nudge_slider(&mut self, forward: bool) -> bool {
        let index = self.hovered.unwrap_or(self.selected);
        if self.dragging == Some(index) {
            return false;
        }
        let Some(item) = self.items.get_mut(index) else {
            return false;
        };
        let MenuItemKind::Slider(slider) = &mut item.kind else {
            return false;
        };
        let before = slider.position();
        slider.set_position(if forward {
            before + Slider::KEY_STEP
        } else {
            before - Slider::KEY_STEP
        });
        slider.position() != before
    }

    /// Whether the highlighted row is a slider.
    ///
    /// What a caller deciding whether a key belongs to the menu has to ask
    /// **before** offering it: [`nudge_slider`](Self::nudge_slider) reports
    /// whether a handle moved, which is also `false` at the end of the groove,
    /// and a key claimed on that would be handed back to the game halfway
    /// through a player holding it down.
    #[must_use]
    pub fn slider_highlighted(&self) -> bool {
        matches!(
            self.items.get(self.hovered.unwrap_or(self.selected)),
            Some(item) if matches!(item.kind, MenuItemKind::Slider(_))
        )
    }

    /// Which choice the cycler row named `id` holds: an index into the
    /// caller's list.
    ///
    /// `None` for an id this menu has no row for, and for a row that is not a
    /// cycler.
    #[must_use]
    pub fn cycler(&self, id: WidgetId) -> Option<usize> {
        self.items.iter().find_map(|item| match item.kind {
            MenuItemKind::Cycler(cycler) if item.id == id => Some(cycler.chosen()),
            _ => None,
        })
    }

    /// Picks the choice of the cycler row named `id`, clamped to its last, and
    /// reports whether there was such a row.
    ///
    /// Nothing refuses it: a cycler is never dragged, so the caller's write is
    /// always the newer of the two inputs and mirroring a value back in every
    /// frame is safe — the row it mirrors into is the row it read.
    pub fn set_cycler(&mut self, id: WidgetId, chosen: usize) -> bool {
        self.items
            .iter_mut()
            .find(|item| item.id == id)
            .is_some_and(|item| match &mut item.kind {
                MenuItemKind::Cycler(cycler) => {
                    cycler.choose(chosen);
                    true
                }
                _ => false,
            })
    }

    /// Steps the highlighted cycler one choice, **stopping at the ends**, and
    /// reports whether the choice changed.
    ///
    /// The arrows' half of a cycler, beside [`Menu::nudge_slider`]: the same
    /// keys, the same highlighted row, and the same `false` at the end of the
    /// list that a slider gives at the end of its groove — a key held against
    /// the last choice does not come round to the first, because a player
    /// holding Right to reach the top of a list would overshoot it. The commit
    /// key is the one that goes round; see [`Menu::activate`].
    pub fn nudge_cycler(&mut self, forward: bool) -> bool {
        let index = self.hovered.unwrap_or(self.selected);
        match self.items.get_mut(index).map(|item| &mut item.kind) {
            Some(MenuItemKind::Cycler(cycler)) => cycler.step(forward, false),
            _ => false,
        }
    }

    /// Whether the highlighted row is a cycler — [`slider_highlighted`]'s
    /// question for the other row the arrows drive, and asked for the same
    /// reason.
    ///
    /// [`slider_highlighted`]: Self::slider_highlighted
    #[must_use]
    pub fn cycler_highlighted(&self) -> bool {
        matches!(
            self.items.get(self.hovered.unwrap_or(self.selected)),
            Some(item) if matches!(item.kind, MenuItemKind::Cycler(_))
        )
    }

    /// The value text a slider row draws, replaced.
    ///
    /// Separate from [`Menu::set_slider`] because the two move for different
    /// reasons: the handle is refused mid-drag and the caption never is — a
    /// player dragging the exposure is precisely who needs to read the number
    /// it has reached.
    pub fn set_item_hint(&mut self, id: WidgetId, hint: impl Into<String>) -> bool {
        match self.items.iter_mut().find(|item| item.id == id) {
            Some(item) => {
                item.hint = hint.into();
                true
            }
            None => false,
        }
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
    ///
    /// In a scrolled list only the part of a row inside the list's
    /// [`viewport`](MenuLayout::viewport) is hit, and a row scrolled wholly
    /// out of it is not hit at all. While the list follows the keyboard, the
    /// scroll `layout` settled on is kept as the one the next keyboard move
    /// scrolls from — see [`Menu::select_next`].
    pub fn point(
        &mut self,
        layout: &MenuLayout,
        ui: &mut UiState,
        pointer: PointerInput,
    ) -> Option<WidgetId> {
        if self.follow_selection {
            self.scroll = layout.scroll();
        }
        let in_view = layout
            .viewport()
            .is_none_or(|viewport| contains(viewport, pointer.pos));
        let mut clicked = None;
        let mut hovered = None;
        for (index, item) in layout.items.iter().enumerate() {
            let inside =
                in_view && layout.shows(index) && contains((item.min, item.max), pointer.pos);
            let (state, fired) = ui.interact(item.id, inside, pointer.down, pointer.released);
            if inside {
                hovered = Some(index);
            }
            let is_slider = matches!(
                self.items.get(index).map(|item| item.kind),
                Some(MenuItemKind::Slider(_))
            );
            if state == ButtonState::Pressed && is_slider {
                // Tracked whether or not the cursor is still **inside** the
                // row, unlike a button's press: a slider dragged off the end of
                // its groove — or above it, which is what a hand does while
                // watching the frame behind the panel — keeps the handle at the
                // end rather than letting go of it.
                if let Some(track) = item.track
                    && let Some(MenuItemKind::Slider(slider)) =
                        self.items.get_mut(index).map(|item| &mut item.kind)
                {
                    slider.set_position(position_along(
                        track,
                        layout.style.handle_size.x,
                        pointer.pos.x,
                    ));
                }
                self.dragging = Some(index);
                self.pressed = true;
                self.pressed_index = Some(index);
                self.selected = index;
            } else if state == ButtonState::Pressed && inside {
                self.pressed = true;
                self.pressed_index = Some(index);
            }
            // A release over a slider ends a drag; it is not a click, and an id
            // reported from one would reach the game's action table as though a
            // button had been pressed. A release over a cycler is its step
            // forward and round, `activate`'s rule, and reports nothing either.
            if fired {
                match self.items.get_mut(index).map(|item| &mut item.kind) {
                    Some(MenuItemKind::Action) => {
                        clicked = Some(item.id);
                        self.selected = index;
                    }
                    Some(MenuItemKind::Cycler(cycler)) => {
                        cycler.step(true, true);
                        self.selected = index;
                    }
                    _ => {}
                }
            }
        }
        self.hovered = hovered;
        if !pointer.down {
            self.pressed = false;
            self.pressed_index = None;
            self.dragging = None;
        }
        clicked
    }

    /// Scrolls a scrolled item list by a wheel's `delta`, in pixels, and says
    /// whether it moved: positive `y` moves toward the end of the list, as
    /// [`Ui::scroll_wheel`] reads a wheel. `x` is ignored — the list scrolls
    /// up and down only.
    ///
    /// Only while `pointer` is over the panel, since the list is the one thing
    /// in it that moves. The list stops following the keyboard's selection
    /// until the keyboard next moves it, so the selection may be scrolled out
    /// of view — and the scroll is kept to the fraction of a pixel, so a
    /// trackpad's small steps add up, while each layout draws it at the whole
    /// pixel nearest. `false` for a list that does not scroll, so the caller
    /// can give the wheel to something else.
    ///
    /// A separate call rather than a [`PointerInput`] field, as
    /// [`Ui::scroll_wheel`] is, because every caller builds that by literal.
    pub fn scroll_wheel(&mut self, layout: &MenuLayout, pointer: Vec2, delta: Vec2) -> bool {
        let Some(list) = &layout.setup.list else {
            return false;
        };
        if !contains(layout.panel, pointer) {
            return false;
        }
        let from = if self.follow_selection {
            list.scroll
        } else {
            self.scroll.clamp(0.0, list.reach)
        };
        let to = (from + delta.y).clamp(0.0, list.reach);
        self.follow_selection = false;
        self.scroll = to;
        to != from
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
    ///
    /// **The panel is measured once, at scale 1**, and each candidate is that
    /// size times the scale: every length [`MenuStyle::pixel_art`] makes and
    /// every glyph advance is a whole number times the scale, so the tree lays
    /// the panel out exactly proportionally —
    /// `the_panel_grows_exactly_with_the_scale` holds that — and one layout
    /// answers every candidate.
    #[must_use]
    pub fn layout(&self, extent: (u32, u32), atlas: &FontAtlas) -> MenuLayout {
        let base = self.panel_size(atlas, &MenuStyle::pixel_art(1));
        let mut chosen = 1;
        for scale in 2..=MenuStyle::MAX_SCALE {
            let size = base * scale as f32;
            if size.x <= extent.0 as f32 * FIT_FRACTION && size.y <= extent.1 as f32 * FIT_FRACTION
            {
                chosen = scale;
            } else {
                break;
            }
        }
        self.layout_with(extent, atlas, &MenuStyle::pixel_art(chosen))
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
        let setup = Setup::default();
        self.laid_out(Some(screen), style, None, &setup, atlas, |ui, built| {
            Self::read_layout(ui, built, &self.items, screen, style, &setup)
        })
    }

    /// Lays this menu out centred at `style`, with every line of its text
    /// measured in `font` rather than the built-in [`FontAtlas`] — and drawn in
    /// it too, since [`Menu::render`] draws in the font the layout it is given
    /// was measured in.
    ///
    /// The text is a registered font's in the menu's own tree, so it is
    /// measured by the tree's one text path, as any span in a registered font
    /// is: `font-size` is [`MenuStyle`]'s, and the line pitch is the font's
    /// `line-height: normal`. [`Menu::layout`]'s scale fit has no counterpart
    /// here — it counts on the bitmap font's advances growing exactly with the
    /// scale, which a parsed font's do not — so `style` is the caller's; see
    /// [`Menu::layout_with_font_fitted`] for a fit that measures instead. A
    /// cycler's chevrons keep its caption still only in a font whose `<`, `>`
    /// and space advance alike.
    #[must_use]
    pub fn layout_with_font(
        &self,
        extent: (u32, u32),
        style: &MenuStyle,
        font: &'static Font,
    ) -> MenuLayout {
        let setup = Setup {
            font: Some(font),
            ..Setup::default()
        };
        self.placed(extent_size(extent), style, &setup)
    }

    /// Lays this menu out centred in `font`, as [`Menu::layout_with_font`]
    /// does, at the largest size up to `style` that fits the framebuffer —
    /// shrinking it first and scrolling its item list after, as the module's
    /// *Fitting a small window, then scrolling* describes.
    ///
    /// **Fits** is [`Menu::layout`]'s word: inside [`FIT_FRACTION`] of the
    /// framebuffer on both axes. `style` is the largest the menu is drawn at,
    /// and is what it is drawn at when that fits. Otherwise the item font
    /// steps down by [`FIT_FONT_STEP`] to [`MenuStyle::MIN_FONT_SIZE`] — or
    /// stays at `style`'s own, if that is smaller — and every other length
    /// shrinks with it by the same factor, the art to the whole-number scale
    /// the factor leaves and never below one. The step is found by bisection
    /// over the steps, which is the largest that fits so long as a smaller
    /// step never lays out a bigger panel — every length shrinks with the
    /// font, so only a rounded pixel could. If even the smallest step is
    /// too tall, the item list at that step is capped at the height the rest
    /// of the panel leaves, and scrolls: see [`MenuLayout::viewport`].
    ///
    /// Every length but the font sizes is rounded to a whole pixel, and each
    /// row's line pitch is the font's `line-height: normal` rounded up to one,
    /// so every row is a whole number of pixels tall — which is what lets a
    /// scrolled list draw only the rows in view where this layout put them. A
    /// `style` that fits is therefore drawn at exactly its own lengths only
    /// when they are whole, as [`MenuStyle::pixel_art`]'s are.
    ///
    /// A pure function of the menu — its contents, its selection and its
    /// scroll — and its arguments: the same inputs lay out the same menu.
    ///
    /// # Errors
    ///
    /// [`MenuFitError::TooWide`] when the panel is wider than the room even at
    /// the smallest step, which scrolling up and down cannot help, and
    /// [`MenuFitError::TooShort`] when the frame, the title and the captions
    /// leave less height than the list's tallest row — or no list at all.
    pub fn layout_with_font_fitted(
        &self,
        extent: (u32, u32),
        style: &MenuStyle,
        font: &'static Font,
    ) -> Result<MenuLayout, MenuFitError> {
        let screen = extent_size(extent);
        let room = screen * FIT_FRACTION;
        let sizes = fit_sizes(style.item_size);
        let measure = |item_size: f32| {
            let style = style.shrunk(item_size);
            let setup = Setup {
                font: Some(font),
                line: Some(row_line(font, &style)),
                list: None,
            };
            let measured = self.measure(&style, &setup);
            Candidate {
                style,
                setup,
                measured,
            }
        };
        let fits = |candidate: &Candidate| {
            let panel = candidate.measured.panel;
            panel.x <= room.x && panel.y <= room.y
        };

        let mut fitting = measure(sizes[0]);
        if !fits(&fitting) {
            let last = sizes.len() - 1;
            let smallest = if last == 0 {
                fitting
            } else {
                measure(sizes[last])
            };
            if !fits(&smallest) {
                return self.scrolled(screen, room, smallest);
            }
            // `sizes[lo]` is too big and `sizes[hi]`, which `fitting` holds,
            // fits.
            fitting = smallest;
            let (mut lo, mut hi) = (0, last);
            while hi - lo > 1 {
                let mid = (lo + hi) / 2;
                let candidate = measure(sizes[mid]);
                if fits(&candidate) {
                    hi = mid;
                    fitting = candidate;
                } else {
                    lo = mid;
                }
            }
        }
        Ok(self.placed(screen, &fitting.style, &fitting.setup))
    }

    /// The fit's second stage: `candidate`, too tall for `room` at the
    /// smallest size the fit may use, with its item list capped at the height
    /// the rest of the panel leaves, and scrolled.
    fn scrolled(
        &self,
        screen: Vec2,
        room: Vec2,
        candidate: Candidate,
    ) -> Result<MenuLayout, MenuFitError> {
        let Candidate {
            style,
            setup,
            measured,
        } = candidate;
        let font_size = style.item_size;
        if measured.panel.x > room.x {
            return Err(MenuFitError::TooWide {
                font_size,
                width: measured.panel.x,
                room: room.x,
            });
        }
        let chrome = measured.panel.y - measured.list.y;
        let mut height = (room.y - chrome).floor();
        loop {
            if self.items.is_empty() || height < measured.tallest {
                return Err(MenuFitError::TooShort {
                    font_size,
                    height: chrome + measured.tallest,
                    room: room.y,
                });
            }
            // Every row, unscrolled: the geometry `scroll_list` scrolls.
            let list = ListView {
                size: Vec2::new(measured.list.x, height),
                viewport: (Vec2::ZERO, Vec2::ZERO),
                scroll: 0.0,
                reach: 0.0,
                shown: 0..self.items.len(),
                shown_top: 0.0,
            };
            let setup = Setup {
                list: Some(list),
                ..setup.clone()
            };
            let layout = self.placed(screen, &style, &setup);
            // The panel is its measured chrome plus the list's height, less
            // whatever Taffy's rounding moved; a pixel over is a pixel off
            // the list.
            let over = layout.panel_size().y - room.y;
            if over <= 0.0 {
                return Ok(self.scroll_list(layout));
            }
            height -= over.ceil();
        }
    }

    /// Scrolls `layout` — placed with every row of its capped list built and
    /// unscrolled — to this menu's scroll, clamped to the list's reach and,
    /// while the list follows the keyboard, moved by as little as brings the
    /// selected row wholly into view. Every row's rectangles move with it, and
    /// the rows left overlapping the viewport are the ones drawn and hit.
    fn scroll_list(&self, mut layout: MenuLayout) -> MenuLayout {
        let list = layout
            .setup
            .list
            .as_mut()
            .expect("a scrolled layout has a list");
        let (top, bottom) = (list.viewport.0.y, list.viewport.1.y);
        let height = bottom - top;
        let end = layout.items.last().map_or(top, |row| row.max.y);
        list.reach = (end - top - height).max(0.0);

        let mut scroll = self.scroll.round().clamp(0.0, list.reach);
        if self.follow_selection
            && let Some(row) = layout.items.get(self.selected)
        {
            let (row_top, row_bottom) = (row.min.y - top, row.max.y - top);
            if row_top < scroll {
                scroll = row_top;
            } else if row_bottom > scroll + height {
                scroll = row_bottom - height;
            }
        }
        let scroll = scroll.clamp(0.0, list.reach);
        list.scroll = scroll;

        let up = Vec2::new(0.0, scroll);
        for row in &mut layout.items {
            row.min -= up;
            row.max -= up;
            row.label_pos -= up;
            row.hint_pos -= up;
            if let Some(track) = &mut row.track {
                track.0 -= up;
                track.1 -= up;
            }
        }
        let rows = &layout.items;
        let first = rows
            .iter()
            .position(|row| row.max.y > top)
            .unwrap_or(rows.len());
        let past = rows
            .iter()
            .rposition(|row| row.min.y < bottom)
            .map_or(first, |last| (last + 1).max(first));
        list.shown = first..past;
        list.shown_top = rows.get(first).map_or(0.0, |row| row.min.y + scroll - top);
        layout
    }

    /// Lays this menu out centred in `screen` at `style`, built as `setup`
    /// says, and reads the layout back.
    fn placed(&self, screen: Vec2, style: &MenuStyle, setup: &Setup) -> MenuLayout {
        self.laid_out(
            Some(screen),
            style,
            None,
            setup,
            &FontAtlas::built_in(),
            |ui, built| Self::read_layout(ui, built, &self.items, screen, style, setup),
        )
    }

    /// The panel, the item list and the list's tallest row at `style`, built
    /// as `setup` says, before any of them is placed.
    fn measure(&self, style: &MenuStyle, setup: &Setup) -> Measured {
        self.laid_out(
            None,
            style,
            None,
            setup,
            &FontAtlas::built_in(),
            |ui, built| {
                let size = |key| {
                    let (min, max) = ui.rect(key).expect("laid out this frame");
                    max - min
                };
                Measured {
                    panel: size(built.panel),
                    list: size(built.items),
                    tallest: built
                        .rows
                        .iter()
                        .map(|row| size(row.row).y)
                        .fold(0.0, f32::max),
                }
            },
        )
    }

    /// Reads a [`MenuLayout`] back out of `ui`, which [`Menu::laid_out`] built
    /// for `items` at `style` in `screen`, as `setup` says.
    fn read_layout(
        ui: &Ui,
        built: &BuiltMenu,
        items: &[MenuItem],
        screen: Vec2,
        style: &MenuStyle,
        setup: &Setup,
    ) -> MenuLayout {
        let rect = |key| ui.rect(key).expect("laid out this frame");
        let panel = rect(built.panel);
        let corners = style.panel_corners();
        let content_min = panel.0 + corners.min_corner() + style.panel_padding;
        let content_max = panel.1 - Vec2::new(corners.right, corners.bottom) - style.panel_padding;
        // An empty title builds no span, and stands where a zero-width one
        // would: centred on the content's top edge.
        let title_pos = built.title.map_or(
            Vec2::new((content_min.x + content_max.x) * 0.5, content_min.y),
            |key| rect(key).0,
        );
        let items = items
            .iter()
            .zip(&built.rows)
            .map(|(item, row)| {
                let (min, max) = rect(row.row);
                let label_pos = rect(row.label).0;
                let inner = style.button_corners();
                let hint_pos = row.hint.map_or(
                    Vec2::new(max.x - inner.right - style.button_padding.x, label_pos.y),
                    |key| rect(key).0,
                );
                MenuItemLayout {
                    id: item.id,
                    min,
                    max,
                    label_pos,
                    hint_pos,
                    track: row.groove.map(rect),
                }
            })
            .collect();

        let mut setup = setup.clone();
        if let Some(list) = &mut setup.list {
            list.viewport = rect(built.items);
        }
        MenuLayout {
            style: *style,
            setup,
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
        self.laid_out(None, style, None, &Setup::default(), atlas, |ui, built| {
            let (min, max) = ui.rect(built.panel).expect("laid out this frame");
            max - min
        })
    }

    /// Builds this menu with [`Menu::build`] and lays it out, in the calling
    /// thread's one menu tree, then hands the tree to `read`. Its text is in
    /// `setup`'s font, registered in that tree, or in the bitmap font for
    /// none.
    ///
    /// **One tree per thread, rebuilt by every call** — two, one to measure
    /// in and one to place in — rather than a fresh
    /// [`Ui`] each time: a menu is laid out several times a frame — for its
    /// fit, its hit test and its drawing — with the same selectors and the same
    /// inline lengths, so a kept tree resolves each node's style from its own
    /// last resolve and lays out from Taffy's cache instead of from nothing.
    /// It holds no state a call can see: the pointer it begins each frame with
    /// is off every rectangle, so no rule's `:hover` applies, every build sets
    /// the item list's scroll, and whatever the last call built is replaced
    /// and pruned. `read` must not lay out a menu itself, which would borrow
    /// the tree twice.
    fn laid_out<R>(
        &self,
        screen: Option<Vec2>,
        style: &MenuStyle,
        skin: Option<&MenuSkin>,
        setup: &Setup,
        atlas: &FontAtlas,
        read: impl FnOnce(&Ui, &BuiltMenu) -> R,
    ) -> R {
        let tree = if screen.is_some() {
            &PLACED_TREE
        } else {
            &MEASURED_TREE
        };
        tree.with(|tree| {
            let mut ui = tree.borrow_mut();
            ui.begin_frame(PointerInput::hovering(OFF_SCREEN));
            if let Some(font) = setup.font {
                ui.register_font(MENU_FONT, font)
                    .expect("the menu's family name is not reserved");
            }
            let built = self.build(&mut ui, screen, style, skin, setup);
            let available = screen.map_or(AvailableSpace::MAX_CONTENT, AvailableSpace::definite);
            ui.layout(Vec2::ZERO, available, atlas);
            read(&ui, &built)
        })
    }

    /// Builds this menu into `ui` at `style`: the tree [`Menu::layout_with`]
    /// measures and [`Menu::render`] draws, one for both.
    ///
    /// `screen` is the framebuffer the panel is centred in, or `None` for a
    /// root that is only as big as the panel. `skin` is `Some` only when the
    /// tree is to be drawn: it binds the frames `default.css` names and adds
    /// the scrim, which is positioned out of the flow and moves nothing.
    /// A `setup` with a font names [`MENU_FONT`] on the root, which every span
    /// inherits; otherwise the text is `default.css`'s bitmap font. Its line
    /// pitch, if any, is set on the item list for every row to inherit, and
    /// its list, if any, caps the item list at its size, clips it and builds
    /// only the rows it shows, scrolled to where the layout put them.
    ///
    /// **What comes from where.** Every length is `style`'s, set inline, because
    /// it is the pixel-art scale times a base metric and a stylesheet has no
    /// arithmetic to scale with. The frames, their slices, which frame each
    /// state draws and every colour are `default.css`'s menu rules. Which state
    /// a row is in is [`Menu::state`]'s: the tree is this menu's view, and the
    /// selection, the press and the pointer's hover stay in the model.
    fn build(
        &self,
        ui: &mut Ui,
        screen: Option<Vec2>,
        style: &MenuStyle,
        skin: Option<&MenuSkin>,
        setup: &Setup,
    ) -> BuiltMenu {
        use Declaration as D;
        if let Some(skin) = skin {
            ui.set_image(PANEL_IMAGE, skin.panel.image);
            ui.set_image(IDLE_IMAGE, skin.buttons.idle.image);
            ui.set_image(HOVERED_IMAGE, skin.buttons.hovered.image);
            ui.set_image(PRESSED_IMAGE, skin.buttons.pressed.image);
        }
        let px = LengthAuto::Px;
        let mut root = screen.map_or_else(Vec::new, |screen| {
            vec![D::Width(px(screen.x)), D::Height(px(screen.y))]
        });
        if setup.font.is_some() {
            root.push(D::FamilyName(Some(FamilyName::new(MENU_FONT))));
        }
        let panel_corners = style.panel_corners();
        let panel = [
            D::BorderWidth(Sides::Top, panel_corners.top),
            D::BorderWidth(Sides::Right, panel_corners.right),
            D::BorderWidth(Sides::Bottom, panel_corners.bottom),
            D::BorderWidth(Sides::Left, panel_corners.left),
            D::Padding(Sides::Top, Length::Px(style.panel_padding.y)),
            D::Padding(Sides::Bottom, Length::Px(style.panel_padding.y)),
            D::Padding(Sides::Left, Length::Px(style.panel_padding.x)),
            D::Padding(Sides::Right, Length::Px(style.panel_padding.x)),
            D::RowGap(Length::Px(style.title_gap)),
        ];
        let button_corners = style.button_corners();
        let row = [
            D::BorderWidth(Sides::Top, button_corners.top),
            D::BorderWidth(Sides::Right, button_corners.right),
            D::BorderWidth(Sides::Bottom, button_corners.bottom),
            D::BorderWidth(Sides::Left, button_corners.left),
            D::Padding(Sides::Top, Length::Px(style.button_padding.y)),
            D::Padding(Sides::Bottom, Length::Px(style.button_padding.y)),
            D::Padding(Sides::Left, Length::Px(style.button_padding.x)),
            D::Padding(Sides::Right, Length::Px(style.button_padding.x)),
        ];
        let rows_style = RowStyle {
            row,
            label: [D::FontSize(style.item_size)],
            hint: [
                D::FontSize(style.item_size),
                D::Margin(Sides::Left, px(style.hint_gap)),
            ],
            groove: [
                D::Margin(Sides::Left, px(style.hint_gap)),
                D::MinWidth(px(style.track_width)),
                D::Height(px(style.track_height)),
            ],
            thumb: [
                D::Width(px(style.handle_size.x)),
                D::Height(px(style.handle_size.y)),
            ],
        };
        let caption = [D::FontSize(style.item_size)];
        let mut list = vec![D::RowGap(Length::Px(style.item_gap))];
        if let Some(line) = setup.line {
            list.push(D::LineHeight(LineHeight::Px(line)));
        }
        if let Some(view) = &setup.list {
            list.extend([
                D::Overflow(Overflow::Hidden),
                D::FlexShrink(0.0),
                D::Width(px(view.size.x)),
                D::Height(px(view.size.y)),
            ]);
        }
        let (shown, offset) = setup.list.as_ref().map_or(
            (0..self.items.len(), 0.0),
            // The first row built is laid out at the list's top, so the
            // offset is the scroll less how far down that row starts.
            |view| (view.shown.clone(), view.scroll - view.shown_top),
        );

        let mut title = None;
        let mut rows = Vec::with_capacity(shown.len());
        let mut panel_key = None;
        let mut items_key = None;
        ui.block("menu-screen", &root, |ui| {
            if let Some(skin) = skin {
                ui.span(".menu-scrim", skin.scrim, &[]);
            }
            panel_key = Some(
                ui.block("menu", &panel, |ui| {
                    if !self.title.is_empty() {
                        title = Some(
                            ui.span(
                                ".menu-title",
                                self.title.as_str(),
                                &[D::FontSize(style.title_size)],
                            )
                            .key,
                        );
                    }
                    if !self.subtitle.is_empty() {
                        ui.block(".menu-captions", &[], |ui| {
                            for line in &self.subtitle {
                                ui.span(".menu-caption", line.as_str(), &caption);
                            }
                        });
                    }
                    items_key = Some(
                        ui.block(".menu-items", &list, |ui| {
                            // Set every build, not only a scrolled one's: the
                            // tree is shared, and an offset a scrolled menu
                            // left on this node would move the next menu's
                            // rows.
                            ui.set_scroll_offset(Vec2::new(0.0, offset));
                            for (index, item) in self.items.iter().enumerate() {
                                if shown.contains(&index) {
                                    rows.push(self.build_row(ui, index, item, &rows_style));
                                }
                            }
                        })
                        .key,
                    );
                })
                .key,
            );
        });
        BuiltMenu {
            panel: panel_key.expect("the screen builds its panel"),
            title,
            items: items_key.expect("the panel builds its item list"),
            rows,
        }
    }

    /// Builds item `index`: a row in the pseudo-class [`Menu::state`] gives it,
    /// holding its label, then what the row carries — a groove and its handle
    /// for a slider, a spacer for anything with a caption to push right — and
    /// then the caption itself.
    fn build_row(&self, ui: &mut Ui, index: usize, item: &MenuItem, style: &RowStyle) -> BuiltRow {
        let state = match self.state(index) {
            ButtonState::Idle => PseudoClasses::NONE,
            ButtonState::Hovered => PseudoClasses::HOVER,
            ButtonState::Pressed => PseudoClasses::ACTIVE,
        };
        let mut label = None;
        let mut groove = None;
        let mut hint = None;
        let row = ui
            .block_keyed_in_state(index, "menu-item", &style.row, state, |ui| {
                label = Some(
                    ui.span(".menu-label", item.label.as_str(), &style.label)
                        .key,
                );
                let caption = item.caption();
                match item.kind {
                    MenuItemKind::Slider(slider) => {
                        groove = Some(build_groove(ui, slider, style));
                    }
                    MenuItemKind::Action | MenuItemKind::Cycler(_) if !caption.is_empty() => {
                        ui.block(".menu-spacer", &[], |_| {});
                    }
                    MenuItemKind::Action | MenuItemKind::Cycler(_) => {}
                }
                if !caption.is_empty() {
                    hint = Some(ui.span(".menu-hint", caption.as_ref(), &style.hint).key);
                }
            })
            .key;
        BuiltRow {
            row,
            label: label.expect("every row builds its label"),
            hint,
            groove,
        }
    }

    /// Emits the menu's **pictures** alone: the scrim over the framebuffer, the
    /// window frame, and each button's frame for its state.
    ///
    /// The half of [`render`](Self::render) that is art: the same tree, with
    /// every command that is not a picture left out — the text, and a
    /// slider's groove and handle. The frames are nine-sliced at
    /// [`MenuStyle::scale`] pixels per texel, which is what keeps the art's
    /// corners the layout's corners.
    pub fn render_art(&self, dl: &mut DrawList, layout: &MenuLayout, skin: &MenuSkin) {
        let mut whole = DrawList::new();
        self.render(&mut whole, layout, skin);
        // Each under the clip it was drawn under, which a scrolled list's
        // frames need: a row half out of view is cut at the list's edge.
        for (command, clip) in whole.commands().iter().zip(whole.clips()) {
            if matches!(command, DrawCommand::Image { .. }) {
                dl.push_clip(clip.min, clip.max);
                dl.push_command(command.clone());
                dl.pop_clip()
                    .expect("the clip pushed above is still on the stack");
            }
        }
    }

    /// Emits the whole menu into `dl`: the tree [`Menu::layout_with`] measured
    /// at `layout`'s style and framebuffer, drawn with `skin`. Its text is
    /// measured in the font `layout` was — the built-in [`FontAtlas`], or the
    /// font [`Menu::layout_with_font`] was given — so it lands where `layout`
    /// says.
    ///
    /// **The order is the tree's paint order**: the scrim first, then the
    /// window frame and the title, then each row — its frame, then what sits
    /// on it. The draw list is drawn in the order commands went in, so the
    /// scrim dims what was pushed before this call, the frame covers the
    /// scrim, and every label is drawn over its own row's frame; rows do not
    /// overlap, so no frame covers another row's text.
    pub fn render(&self, dl: &mut DrawList, layout: &MenuLayout, skin: &MenuSkin) {
        self.laid_out(
            Some(layout.screen),
            &layout.style,
            Some(skin),
            &layout.setup,
            &FontAtlas::built_in(),
            |ui, _| ui.emit(dl),
        );
    }
}

thread_local! {
    /// The tree [`Menu::laid_out`] places and draws menus in; see there.
    static PLACED_TREE: std::cell::RefCell<Ui> = std::cell::RefCell::new(Ui::new());
    /// The tree [`Menu::panel_size`] measures in: apart from
    /// [`PLACED_TREE`], because [`Menu::layout`] measures at scale 1 and
    /// places at the scale it chose, and one tree alternating between the two
    /// would resolve and lay out every node from nothing on every call.
    static MEASURED_TREE: std::cell::RefCell<Ui> = std::cell::RefCell::new(Ui::new());
}

/// Where [`Menu::laid_out`]'s pointer is: above and left of every rectangle a
/// menu lays out, which all start at the origin.
const OFF_SCREEN: Vec2 = Vec2::splat(-1.0);

/// The family a font [`Menu::layout_with_font`] is given is registered under
/// in the menu trees, which are this module's own, so no app's name collides.
const MENU_FONT: &str = "crcbl-menu";

/// The names `default.css`'s menu rules give the [`MenuSkin`]'s frames, which
/// [`Menu::render`] binds with [`Ui::set_image`].
const PANEL_IMAGE: &str = "menu-panel";
/// See [`PANEL_IMAGE`].
const IDLE_IMAGE: &str = "menu-button-idle";
/// See [`PANEL_IMAGE`].
const HOVERED_IMAGE: &str = "menu-button-hovered";
/// See [`PANEL_IMAGE`].
const PRESSED_IMAGE: &str = "menu-button-pressed";

/// The nodes [`Menu::build`] made, for reading a layout back.
struct BuiltMenu {
    panel: NodeKey,
    title: Option<NodeKey>,
    /// The block the rows are in.
    items: NodeKey,
    /// The rows built, which a scrolled list's render limits to those in view.
    rows: Vec<BuiltRow>,
}

/// A slider row's groove: the tree's own slider drawing the well and the fill,
/// and a handle over it in a rail whose two spaces share out the free width by
/// the value — so the handle travels from flush left to flush right and hangs
/// over neither end. Returns the groove's key, which is the rectangle a
/// pointer's drag is measured against.
fn build_groove(ui: &mut Ui, slider: Slider, style: &RowStyle) -> NodeKey {
    use Declaration as D;
    let share = slider.position();
    ui.block(".menu-groove", &style.groove, |ui| {
        // The tree's slider reads its value and draws it; the menu's model is
        // what moves it, so the copy it is handed goes nowhere.
        let mut position = share;
        ui.slider(".menu-slider", &mut position, 0.0..=1.0, Slider::KEY_STEP);
        ui.block(".menu-thumb-rail", &[], |ui| {
            ui.block(".menu-thumb-space", &[D::FlexGrow(share)], |_| {});
            ui.block(".menu-thumb", &style.thumb, |_| {});
            ui.block(".menu-thumb-space", &[D::FlexGrow(1.0 - share)], |_| {});
        });
    })
    .key
}

/// The inline declarations every row of one menu is built with, at its style.
struct RowStyle {
    row: [Declaration; 8],
    label: [Declaration; 1],
    hint: [Declaration; 2],
    groove: [Declaration; 3],
    thumb: [Declaration; 2],
}

/// One row's nodes.
struct BuiltRow {
    row: NodeKey,
    label: NodeKey,
    hint: Option<NodeKey>,
    groove: Option<NodeKey>,
}

/// The share of the framebuffer a menu is allowed to fill before
/// [`Menu::layout`] drops to a smaller scale.
///
/// Nine tenths rather than all of it: a modal that reached the edge of the
/// window would have no frame visible on the short axis, and the scrim behind it
/// would have nothing to dim.
pub const FIT_FRACTION: f32 = 0.9;

/// How far apart the item font sizes [`Menu::layout_with_font_fitted`] tries
/// are, in pixels.
///
/// A quarter pixel rather than any size at all: a window dragged through
/// every size in between then draws at a bounded set of sizes, so the glyph
/// atlas is not handed a fresh rasterisation of every glyph for each pixel of
/// the drag, and the fit is a bisection over a known list.
pub const FIT_FONT_STEP: f32 = 0.25;

/// The item font sizes [`Menu::layout_with_font_fitted`] tries, largest first:
/// `largest` itself, then every [`FIT_FONT_STEP`] below it down to
/// [`MenuStyle::MIN_FONT_SIZE`]. Only `largest` when it is no bigger than
/// that.
fn fit_sizes(largest: f32) -> Vec<f32> {
    let mut sizes = vec![largest];
    let spare = largest - MenuStyle::MIN_FONT_SIZE;
    if spare > 0.0 {
        let steps = (spare / FIT_FONT_STEP).floor() as u32;
        sizes.extend(
            (0..=steps)
                .rev()
                .map(|step| MenuStyle::MIN_FONT_SIZE + step as f32 * FIT_FONT_STEP)
                .filter(|&size| size < largest),
        );
    }
    sizes
}

/// A fitted row's line pitch in `font` at `style`: its `line-height: normal`
/// rounded up to a whole pixel — see [`MenuStyle::shrunk`].
fn row_line(font: &Font, style: &MenuStyle) -> f32 {
    font.metrics().normal_line_height(style.item_size).ceil()
}

/// `extent` as a size in pixels.
fn extent_size(extent: (u32, u32)) -> Vec2 {
    Vec2::new(extent.0 as f32, extent.1 as f32)
}

/// Whether `point` is inside `rect`, edges included.
fn contains(rect: (Vec2, Vec2), point: Vec2) -> bool {
    point.x >= rect.0.x && point.x <= rect.1.x && point.y >= rect.0.y && point.y <= rect.1.y
}

/// How a menu's tree is built beyond its style: the font its text is in, and
/// what [`Menu::layout_with_font_fitted`] decided. The default is the bitmap
/// font and every row in the flow — what [`Menu::layout`] builds.
#[derive(Debug, Clone, PartialEq, Default)]
struct Setup {
    /// The font the text is measured and drawn in; `None` for the bitmap font.
    font: Option<&'static Font>,
    /// Every row's line pitch, in whole pixels; `None` for the font's own.
    line: Option<f32>,
    /// The item list capped and scrolled; `None` for a list as tall as its
    /// rows.
    list: Option<ListView>,
}

/// A scrolled item list: see [`Menu::layout_with_font_fitted`].
#[derive(Debug, Clone, PartialEq)]
struct ListView {
    /// The list's width and its capped height, in whole pixels.
    size: Vec2,
    /// Where the list shows its rows: `(min, max)` in screen pixels.
    viewport: (Vec2, Vec2),
    /// How far the rows are scrolled, in whole pixels.
    scroll: f32,
    /// The furthest they can be.
    reach: f32,
    /// The rows at least partly in view: the ones built, drawn and hit.
    shown: Range<usize>,
    /// How far below the list's top the first shown row starts, unscrolled.
    shown_top: f32,
}

/// What [`Menu::measure`] reads off a menu.
struct Measured {
    panel: Vec2,
    list: Vec2,
    tallest: f32,
}

/// One size [`Menu::layout_with_font_fitted`] tries: the style and setup it
/// builds with, and what that measured.
struct Candidate {
    style: MenuStyle,
    setup: Setup,
    measured: Measured,
}

/// Why [`Menu::layout_with_font_fitted`] could not fit a menu, even at the
/// smallest size it may shrink to with its item list scrolled. Every length is
/// in pixels, and `room` is [`FIT_FRACTION`] of the framebuffer.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum MenuFitError {
    /// The panel is wider than the room, and a list that scrolls up and down
    /// cannot make it narrower.
    TooWide {
        /// The item font size it was measured at.
        font_size: f32,
        /// The panel's width there.
        width: f32,
        /// The width it had to fit.
        room: f32,
    },
    /// The frame, the title and the captions leave less height than the
    /// list's tallest row — or the menu has no rows to scroll.
    TooShort {
        /// The item font size it was measured at.
        font_size: f32,
        /// The panel's height showing one row, its tallest.
        height: f32,
        /// The height it had to fit.
        room: f32,
    },
}

impl std::fmt::Display for MenuFitError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match *self {
            Self::TooWide {
                font_size,
                width,
                room,
            } => write!(
                f,
                "the menu is {width}px wide at a {font_size}px font, the smallest                  it may shrink to, and the window has room for {room}px"
            ),
            Self::TooShort {
                font_size,
                height,
                room,
            } => write!(
                f,
                "the menu needs {height}px to show one row at a {font_size}px                  font, the smallest it may shrink to, and the window has room                  for {room}px"
            ),
        }
    }
}

impl std::error::Error for MenuFitError {}

#[cfg(test)]
fn text_width(atlas: &FontAtlas, text: &str, size: f32) -> f32 {
    if text.is_empty() {
        return 0.0;
    }
    atlas.text_width(text, size / NATURAL_FONT_SIZE)
}

#[cfg(test)]
fn line_height(size: f32) -> f32 {
    LINE_HEIGHT * (size / NATURAL_FONT_SIZE)
}

/// Where the **centre** of a handle `handle_width` wide may travel inside
/// `track`, as `(leftmost, rightmost)`.
///
/// Half a handle in from each end, so position `0.0` and position `1.0` both
/// draw a handle wholly inside the groove — a handle centred on the end would
/// hang half of itself out over the button's face and read as a different
/// value at each extreme. A groove narrower than one handle collapses the
/// travel to its middle rather than inverting it.
fn handle_travel(track: (Vec2, Vec2), handle_width: f32) -> (f32, f32) {
    let half = (handle_width * 0.5)
        .min((track.1.x - track.0.x) * 0.5)
        .max(0.0);
    (track.0.x + half, track.1.x - half)
}

/// The position a pointer at `x` names on `track`, before clamping.
fn position_along(track: (Vec2, Vec2), handle_width: f32, x: f32) -> f32 {
    let (start, end) = handle_travel(track, handle_width);
    if end <= start {
        return 0.0;
    }
    (x - start) / (end - start)
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
    /// A [`MenuItemKind::Slider`] row's groove: `(min, max)` in screen pixels,
    /// between the label and the value. `None` for a button row.
    pub track: Option<(Vec2, Vec2)>,
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
/// Produced by [`Menu::layout`] and consumed by [`Menu::render`], for the art
/// and the text alike. Carrying the [`MenuStyle`] it was built with is what
/// stops the two being measured with different metrics — the failure that puts
/// a label a scale out of step with the frame around it.
#[derive(Debug, Clone, PartialEq)]
pub struct MenuLayout {
    style: MenuStyle,
    /// How the tree was built: the font the text was measured in and, for a
    /// fitted layout, the rows' line pitch and a scrolled list.
    setup: Setup,
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
    ///
    /// In a scrolled list every row is here, where the scroll put it: a row
    /// scrolled out of view lies outside the [`viewport`](Self::viewport), and
    /// outside the panel too, and is neither drawn nor hit — see
    /// [`shows`](Self::shows).
    #[must_use]
    pub fn items(&self) -> &[MenuItemLayout] {
        &self.items
    }

    /// Where a scrolled item list shows its rows: `(min, max)` in screen
    /// pixels, inside the panel. `None` when the list is not scrolled and
    /// every row is in view.
    ///
    /// A row is drawn cut at this rectangle's edges and hit only inside it.
    #[must_use]
    pub fn viewport(&self) -> Option<(Vec2, Vec2)> {
        self.setup.list.as_ref().map(|list| list.viewport)
    }

    /// How far the item list is scrolled, in pixels: zero unless it scrolls.
    #[must_use]
    pub fn scroll(&self) -> f32 {
        self.setup.list.as_ref().map_or(0.0, |list| list.scroll)
    }

    /// Whether row `index` is at least partly in view, and so drawn and hit.
    /// Every row is, unless the list scrolls; no row past the last is.
    #[must_use]
    pub fn shows(&self, index: usize) -> bool {
        index < self.items.len()
            && self
                .setup
                .list
                .as_ref()
                .is_none_or(|list| list.shown.contains(&index))
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
        self.get_mut(self.shown)
    }

    /// One menu of the set by name, mutably, whether or not it is on screen.
    ///
    /// What a game refreshing a live value on a panel needs and
    /// [`current_mut`](Self::current_mut) cannot give it: `crcbl`'s loop asks
    /// the game which menu to show *after* the game has had its say, so during
    /// that call `current_mut` is still last frame's panel. Naming the menu is
    /// exact where "the current one" is a frame out of step.
    ///
    /// Unlike [`replace`](Self::replace) this keeps the panel's selection and
    /// the pointer's capture, which is what makes it safe to call every frame.
    pub fn get_mut(&mut self, kind: K) -> Option<&mut Menu> {
        self.menus
            .iter_mut()
            .find_map(|(seen, menu)| (*seen == kind).then_some(menu))
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

    /// Moves the highlighted slider one step, if there is a menu.
    pub fn nudge_slider(&mut self, forward: bool) -> bool {
        self.current_mut()
            .is_some_and(|menu| menu.nudge_slider(forward))
    }

    /// Whether the showing menu's highlighted row is a slider.
    ///
    /// `false` when nothing is showing, so a caller asking "is this key the
    /// menu's?" gets the answer for the panel the player can see.
    #[must_use]
    pub fn slider_highlighted(&self) -> bool {
        self.current().is_some_and(Menu::slider_highlighted)
    }

    /// Steps the highlighted cycler one choice, stopping at the ends, if there
    /// is a menu.
    pub fn nudge_cycler(&mut self, forward: bool) -> bool {
        self.current_mut()
            .is_some_and(|menu| menu.nudge_cycler(forward))
    }

    /// Whether the showing menu's highlighted row is a cycler. `false` when
    /// nothing is showing, as [`slider_highlighted`](Self::slider_highlighted)
    /// is.
    #[must_use]
    pub fn cycler_highlighted(&self) -> bool {
        self.current().is_some_and(Menu::cycler_highlighted)
    }

    /// Whether a press is latched onto one of this set's buttons.
    ///
    /// The question a caller driving the menu from **something other than the
    /// pointer** has to ask: a contact is offered to [`point`](Self::point) once
    /// and then has to be remembered or forgotten, and this is what says which —
    /// a press that latched a button is one whose later events matter, and a
    /// press that landed on the panel's background is not the menu's at all.
    #[must_use]
    pub fn press_captured(&self) -> bool {
        self.ui.active().is_some()
    }

    /// Drops a press in progress **without firing it**.
    ///
    /// The [`Cancelled`](crcbl_core::input::TouchPhase::Cancelled) path, and the
    /// distinction is the same one [`crate::touch::TouchButton`] draws: a lift
    /// commits, a gesture the system took away does not. Without it a cancelled
    /// contact would leave [`UiState`]'s capture latched onto a button nobody is
    /// touching, and the next press could latch nothing at all.
    ///
    /// A press the pointer made is dropped by this too — there is one capture,
    /// and a caller that cancels one cancels the press that is live.
    pub fn cancel_press(&mut self) {
        self.ui.clear();
        if let Some(menu) = self.current_mut() {
            menu.clear_input();
        }
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

    /// A skin of five flat 16x16 images, each a different shade, with the
    /// shipped art's insets — enough for `render` to draw, and different
    /// enough that a frame drawn from the wrong image samples a different UV.
    fn skin() -> MenuSkin {
        let mut images = crate::image::ImageAtlas::new();
        let mut register = |shade: u8| {
            images
                .register(16, 16, &[shade; 16 * 16 * 4])
                .expect("five 16x16 images fit an empty page")
        };
        let mut sliced = |shade, insets| NineSliceImage {
            image: register(shade),
            insets,
        };
        let panel = sliced(10, PANEL_INSETS);
        let buttons = ButtonSkin {
            idle: sliced(20, BUTTON_INSETS),
            hovered: sliced(30, BUTTON_INSETS),
            pressed: sliced(40, BUTTON_INSETS),
        };
        MenuSkin {
            panel,
            buttons,
            scrim: register(255),
        }
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
    // In a font
    // -----------------------------------------------------------------------

    /// **A menu laid out in a font measures its text in it and draws it in
    /// it**: a row is its label's advances in the font plus the button's frame
    /// and padding, and one line of the font tall — where the same style in
    /// the atlas measures the atlas's width — and [`Menu::render`] draws that
    /// layout's label as a glyph run of the font. [`Menu::layout`] is what it
    /// was before, the font registered in the shared trees notwithstanding.
    #[test]
    fn a_menu_laid_out_in_a_font_measures_and_draws_its_text_in_it() {
        use crate::font::FontMetrics;

        let font = Font::fixed_pitch(
            FontMetrics {
                units_per_em: 1000,
                ascent: 800.0,
                descent: -200.0,
                line_gap: 0.0,
            },
            500.0,
        );
        let menu = Menu::new("", vec![MenuItem::new(1, "RESUME", "")]);
        let style = MenuStyle::pixel_art(1);
        let extent = (960, 720);
        let before = menu.layout(extent, &atlas());

        let in_font = menu.layout_with_font(extent, &style, font);
        let in_atlas = menu.layout_with(extent, &atlas(), &style);
        let corners = style.button_corners();
        let chrome = Vec2::new(
            corners.left + corners.right + 2.0 * style.button_padding.x,
            corners.top + corners.bottom + 2.0 * style.button_padding.y,
        );
        let label = |layout: &MenuLayout| layout.items()[0].size() - chrome;
        assert_eq!(
            label(&in_font),
            Vec2::new(6.0 * 0.5 * style.item_size, style.item_size),
            "the row was not measured in the font"
        );
        assert_eq!(
            label(&in_atlas),
            Vec2::new(
                text_width(&atlas(), "RESUME", style.item_size),
                line_height(style.item_size)
            )
        );
        assert_ne!(label(&in_font), label(&in_atlas));

        let drawn = |layout: &MenuLayout| {
            let mut list = DrawList::new();
            menu.render(&mut list, layout, &skin());
            let runs: Vec<_> = list
                .commands()
                .iter()
                .filter_map(|command| match command {
                    DrawCommand::Glyphs { font, .. } => Some(font.id()),
                    _ => None,
                })
                .collect();
            let texts = list
                .commands()
                .iter()
                .filter(|command| matches!(command, DrawCommand::Text { .. }))
                .count();
            (runs, texts)
        };
        assert_eq!(drawn(&in_font), (vec![font.id()], 0));
        assert_eq!(drawn(&in_atlas), (Vec::new(), 1));

        assert_eq!(menu.layout(extent, &atlas()), before);
    }

    // -----------------------------------------------------------------------
    // Captions
    // -----------------------------------------------------------------------

    /// [`pause_menu`] with two lines under its title.
    fn captioned_menu() -> Menu {
        let mut menu = pause_menu();
        menu.subtitle = vec!["ARROWS MOVE".to_owned(), "ENTER PICKS".to_owned()];
        menu
    }

    /// The `(text, pos, colour)` of every bitmap string `dl` draws.
    fn texts(dl: &DrawList) -> Vec<(String, Vec2, [f32; 4])> {
        dl.commands()
            .iter()
            .filter_map(|command| match command {
                DrawCommand::Text {
                    text, pos, color, ..
                } => Some((text.clone(), *pos, *color)),
                _ => None,
            })
            .collect()
    }

    /// **The lines under the title are drawn there, in the hint's colour, and
    /// inside the panel** — between the title and the first row, in the
    /// order given — and the rows are laid out below them rather than over
    /// them.
    #[test]
    fn captions_are_drawn_under_the_title_in_the_hint_colour() {
        let atlas = atlas();
        let menu = captioned_menu();
        let layout = menu.layout((960, 720), &atlas);
        let mut dl = DrawList::new();
        menu.render(&mut dl, &layout, &skin());
        let drawn = texts(&dl);
        let line = |wanted: &str| {
            drawn
                .iter()
                .find(|(text, ..)| text == wanted)
                .unwrap_or_else(|| panic!("{wanted} is not drawn: {drawn:?}"))
                .clone()
        };
        let (_, first, first_colour) = line("ARROWS MOVE");
        let (_, second, second_colour) = line("ENTER PICKS");
        assert_eq!(first_colour, layout.style().hint_color);
        assert_eq!(second_colour, layout.style().hint_color);

        let title_bottom = layout.title_pos().y + line_height(layout.style().title_size);
        let caption_bottom = second.y + line_height(layout.style().item_size);
        let (panel_min, panel_max) = layout.panel();
        assert!(
            title_bottom <= first.y && first.y < second.y,
            "the captions are not under the title, in order: {first:?} {second:?}"
        );
        assert!(
            caption_bottom <= layout.items()[0].min.y,
            "the first row starts at {} over a caption ending at {caption_bottom}",
            layout.items()[0].min.y
        );
        for pos in [first, second] {
            assert!(pos.x >= panel_min.x && pos.x <= panel_max.x, "{pos:?}");
        }
    }

    /// **A caption takes no input from either device.** The keyboard walks
    /// the three rows and wraps as it did without captions, the commit key
    /// fires the rows' ids, and a pointer over a caption hovers and clicks
    /// nothing — the highlight stays with the keyboard.
    #[test]
    fn captions_are_skipped_by_the_keyboard_and_the_pointer() {
        let atlas = atlas();
        let mut menu = captioned_menu();
        let mut fired = Vec::new();
        for _ in 0..=menu.items().len() {
            fired.push(menu.activate());
            menu.select_next();
        }
        assert_eq!(fired, [Some(1), Some(2), Some(3), Some(1)]);
        assert!(menu.select_id(1));
        menu.select_previous();
        assert_eq!(menu.activate(), Some(3), "the wrap upward went astray");
        assert!(menu.select_id(1));

        let layout = menu.layout((960, 720), &atlas);
        let mut dl = DrawList::new();
        menu.render(&mut dl, &layout, &skin());
        let (_, caption, _) = texts(&dl)
            .into_iter()
            .find(|(text, ..)| text == "ENTER PICKS")
            .expect("the caption is drawn");
        let on_caption = caption + Vec2::new(2.0, 2.0);
        let mut ui = UiState::new();
        assert_eq!(menu.point(&layout, &mut ui, press_at(on_caption)), None);
        assert_eq!(menu.point(&layout, &mut ui, release_at(on_caption)), None);
        assert_eq!(
            menu.state(0),
            ButtonState::Hovered,
            "the caption took the hover"
        );
        assert_eq!(menu.selected(), 0);
    }

    /// A menu of captions and no rows has nothing to select or fire, and lays
    /// out, fitted or not, without an index to panic on.
    #[test]
    fn a_menu_of_only_captions_selects_nothing() {
        let mut menu = Menu::new("NOTICE", Vec::new());
        menu.subtitle = vec!["NOTHING TO CHOOSE".to_owned()];
        menu.select_next();
        menu.select_previous();
        assert_eq!(menu.activate(), None);
        assert_eq!(menu.selected_item(), None);
        let layout = menu.layout((960, 720), &atlas());
        assert!(layout.items().is_empty());
        let fitted = menu
            .layout_with_font_fitted((480, 360), &MenuStyle::pixel_art(1), font_like_roboto())
            .expect("a notice fits");
        assert!(fitted.items().is_empty() && fitted.viewport().is_none());
    }

    // -----------------------------------------------------------------------
    // Fitted in a font, and scrolled
    // -----------------------------------------------------------------------

    /// Every glyph's advance in [`font_like_roboto`], in ems.
    const ADVANCE_EM: f32 = 0.55;

    /// A stand-in for a parsed UI font such as Roboto: every glyph
    /// [`ADVANCE_EM`] wide, on a line 1.172 em tall, as Roboto's own metrics
    /// give it. One parse per process, as an app's registered font is, so two
    /// layouts in it compare equal.
    fn font_like_roboto() -> &'static Font {
        use crate::font::FontMetrics;
        static FONT: std::sync::OnceLock<&'static Font> = std::sync::OnceLock::new();
        FONT.get_or_init(|| {
            Font::fixed_pitch(
                FontMetrics {
                    units_per_em: 1000,
                    ascent: 928.0,
                    descent: -244.0,
                    line_gap: 0.0,
                },
                ADVANCE_EM * 1000.0,
            )
        })
    }

    /// The row count of [`scene_menu`].
    const SCENES: usize = 17;

    /// A stand-in for EW's scene menu: [`SCENES`] rows, each a label and a
    /// one-line description in the hint slot — the longest seventy
    /// characters — under a title and two captions.
    fn scene_menu() -> Menu {
        let items = (0..SCENES)
            .map(|index| {
                let description = format!("{:02} {}", index, "a".repeat(40 + index * 2))
                    .chars()
                    .take(70)
                    .collect::<String>();
                MenuItem::new(
                    100 + index as WidgetId,
                    format!("SCENE {index:02} NAME"),
                    description,
                )
            })
            .collect();
        let mut menu = Menu::new("SCENES", items);
        menu.subtitle = vec![
            "UP/DOWN MOVES - ENTER LOADS - ESC CLOSES".to_owned(),
            "LOADING A SCENE DISCARDS UNSAVED WORK".to_owned(),
        ];
        menu
    }

    /// The smallest window EW supports.
    const SMALL: (u32, u32) = (480, 360);

    /// The fitted layout of `menu` at `extent`, from EW's style.
    fn fitted(menu: &Menu, extent: (u32, u32)) -> MenuLayout {
        menu.layout_with_font_fitted(extent, &MenuStyle::pixel_art(2), font_like_roboto())
            .unwrap_or_else(|error| panic!("{extent:?}: {error}"))
    }

    /// Whether `inner` lies wholly inside `outer`.
    fn within(inner: (Vec2, Vec2), outer: (Vec2, Vec2)) -> bool {
        inner.0.x >= outer.0.x
            && inner.0.y >= outer.0.y
            && inner.1.x <= outer.1.x
            && inner.1.y <= outer.1.y
    }

    /// **Seventeen rows with descriptions and two captions fit a 480x360
    /// window.** The panel is inside the fit's share of the framebuffer; the
    /// font is no smaller than the minimum; the art is at a whole-number
    /// scale with the art's corners; seventeen rows do not fit that height
    /// at any legible size, so the list scrolls, and its viewport is inside
    /// the panel, every row drawn is inside the panel's width and reaches
    /// into the viewport, and the widest description ends inside its row.
    /// The same inputs lay out the same menu.
    #[test]
    fn a_seventeen_row_menu_fits_a_480_by_360_window() {
        let menu = scene_menu();
        let layout = fitted(&menu, SMALL);
        let style = layout.style();
        let screen = extent_size(SMALL);
        let panel = layout.panel();
        assert!(
            within(panel, (Vec2::ZERO, screen)),
            "the panel {panel:?} leaves the window"
        );
        assert!(layout.panel_size().cmple(screen * FIT_FRACTION).all());
        assert!(
            style.item_size >= MenuStyle::MIN_FONT_SIZE,
            "{}",
            style.item_size
        );
        assert!(
            style.scale >= 1.0 && style.scale.fract() == 0.0,
            "{}",
            style.scale
        );
        assert_eq!(style.panel_corners().left, PANEL_INSETS.left * style.scale);

        let viewport = layout
            .viewport()
            .expect("seventeen rows scroll at 360 tall");
        assert!(within(viewport, panel), "{viewport:?} leaves {panel:?}");
        let shown: Vec<usize> = (0..SCENES).filter(|&index| layout.shows(index)).collect();
        assert!(shown.len() > 1 && shown.len() < SCENES, "{shown:?}");
        for &index in &shown {
            let row = layout.items()[index];
            assert!(
                row.min.x >= panel.0.x && row.max.x <= panel.1.x,
                "row {index} {row:?} is wider than the panel"
            );
            assert!(
                row.max.y > viewport.0.y && row.min.y < viewport.1.y,
                "row {index} is drawn but nowhere in view"
            );
            let description = &menu.items()[index].hint;
            let end =
                row.hint_pos.x + description.chars().count() as f32 * ADVANCE_EM * style.item_size;
            assert!(
                end <= row.max.x + 0.5,
                "row {index}'s description ends at {end}"
            );
        }
        assert!(within(
            (layout.items()[0].min, layout.items()[0].max),
            viewport
        ));
        assert_eq!(fitted(&menu, SMALL), layout, "the fit is not deterministic");
    }

    /// **The fit is the largest step that fits.** At a window big enough to
    /// fit without scrolling but too small for the ceiling, the chosen size
    /// fits and the next step up does not; and a window big enough for the
    /// ceiling gets the ceiling, unscrolled.
    #[test]
    fn the_fit_takes_the_largest_step_that_fits() {
        let menu = scene_menu();
        let ceiling = MenuStyle::pixel_art(2);
        let font = font_like_roboto();
        let extent = (1100, 900);
        let layout = fitted(&menu, extent);
        let chosen = layout.style().item_size;
        assert!(layout.viewport().is_none(), "{extent:?} scrolled");
        assert!(
            chosen < ceiling.item_size && chosen > MenuStyle::MIN_FONT_SIZE,
            "{extent:?} does not exercise the shrink: {chosen}"
        );
        let panel_at = |size: f32| {
            let style = ceiling.shrunk(size);
            let setup = Setup {
                font: Some(font),
                line: Some(row_line(font, &style)),
                list: None,
            };
            menu.measure(&style, &setup).panel
        };
        let room = extent_size(extent) * FIT_FRACTION;
        assert!(panel_at(chosen).cmple(room).all());
        assert!(
            !panel_at(chosen + FIT_FONT_STEP).cmple(room).all(),
            "a step larger than {chosen} fits too"
        );

        let roomy = fitted(&menu, (3840, 2160));
        assert_eq!(roomy.style().item_size, ceiling.item_size);
        assert_eq!(roomy.style().scale, ceiling.scale);
        assert!(roomy.viewport().is_none());
    }

    /// **Below the minimum the fit refuses, and says why.** A window too
    /// narrow for the descriptions at [`MenuStyle::MIN_FONT_SIZE`] is too
    /// wide a menu, which scrolling cannot help; a window too short for the
    /// title, the captions and one row is too short. Neither is laid out at
    /// a smaller size than the minimum.
    #[test]
    fn the_fit_refuses_honestly_below_the_minimum() {
        let menu = scene_menu();
        let style = MenuStyle::pixel_art(2);
        let font = font_like_roboto();
        match menu.layout_with_font_fitted((300, 360), &style, font) {
            Err(MenuFitError::TooWide {
                font_size,
                width,
                room,
            }) => {
                assert_eq!(font_size, MenuStyle::MIN_FONT_SIZE);
                assert!(
                    width > room && room == 300.0 * FIT_FRACTION,
                    "{width} {room}"
                );
            }
            other => panic!("a 300px window gave {other:?}"),
        }
        match menu.layout_with_font_fitted((480, 60), &style, font) {
            Err(MenuFitError::TooShort {
                font_size,
                height,
                room,
            }) => {
                assert_eq!(font_size, MenuStyle::MIN_FONT_SIZE);
                assert!(height > room, "{height} {room}");
            }
            other => panic!("a 60px window gave {other:?}"),
        }
    }

    /// **The keyboard keeps its selection in view, all the way down and all
    /// the way back.** Every step of a walk to the last row and back to the
    /// first leaves the selected row wholly inside the viewport, both for a
    /// caller that only lays out and draws and for one that also runs the
    /// pointer — which keeps the list still while the selection moves inside
    /// it, as a list does.
    #[test]
    fn scrolling_keeps_the_selected_row_in_view_down_and_up() {
        for with_pointer in [false, true] {
            let mut menu = scene_menu();
            let mut ui = UiState::new();
            let mut scrolls = Vec::new();
            let mut step = |menu: &mut Menu, moved: &str| {
                let layout = fitted(menu, SMALL);
                let viewport = layout.viewport().expect("the list scrolls");
                let selected = layout.items()[menu.selected()];
                assert!(
                    layout.shows(menu.selected()) && within((selected.min, selected.max), viewport),
                    "after {moved} to row {}, {selected:?} is not wholly in {viewport:?}",
                    menu.selected(),
                );
                if with_pointer {
                    menu.point(&layout, &mut ui, PointerInput::hovering(OFF_SCREEN));
                }
                scrolls.push(layout.scroll());
            };
            step(&mut menu, "opening");
            for _ in 1..SCENES {
                menu.select_next();
                step(&mut menu, "down");
            }
            assert_eq!(menu.selected(), SCENES - 1);
            for _ in 1..SCENES {
                menu.select_previous();
                step(&mut menu, "up");
            }
            assert_eq!(menu.selected(), 0);
            assert!(scrolls[SCENES - 1] > 0.0, "the walk down never scrolled");
            assert_eq!(*scrolls.last().expect("walked"), 0.0);
            if with_pointer {
                // One step up from the bottom stays inside the view, so the
                // list does not move.
                assert_eq!(scrolls[SCENES], scrolls[SCENES - 1], "{scrolls:?}");
            }
        }
    }

    /// **A row scrolled out of view is neither drawn nor hit.** With the last
    /// row selected the first is scrolled off the top: a press where it was
    /// placed hovers and fires nothing, a row in view still answers, and the
    /// draw list holds the title, the captions and the shown rows' text and
    /// frames and nothing of the others.
    #[test]
    fn rows_scrolled_out_of_view_are_neither_drawn_nor_hit() {
        let mut menu = scene_menu();
        menu.select_previous();
        let last = SCENES - 1;
        assert_eq!(menu.selected(), last);
        let layout = fitted(&menu, SMALL);
        let viewport = layout.viewport().expect("the list scrolls");
        assert!(!layout.shows(0), "the first row is still in view");
        assert!(layout.shows(last));

        let mut ui = UiState::new();
        let hidden = layout.items()[0];
        let where_it_was = (hidden.min + hidden.max) * 0.5;
        assert_eq!(menu.point(&layout, &mut ui, press_at(where_it_was)), None);
        assert_eq!(menu.point(&layout, &mut ui, release_at(where_it_was)), None);
        assert_eq!(
            menu.state(0),
            ButtonState::Idle,
            "a hidden row took the hover"
        );
        let visible = layout.items()[last];
        let on_it = (visible.min + visible.max) * 0.5;
        menu.point(&layout, &mut ui, press_at(on_it));
        assert_eq!(
            menu.point(&layout, &mut ui, release_at(on_it)),
            Some(100 + last as WidgetId)
        );

        let shown = (0..SCENES).filter(|&index| layout.shows(index)).count();
        let mut dl = DrawList::new();
        menu.render(&mut dl, &layout, &skin());
        let runs: Vec<Vec2> = dl
            .commands()
            .iter()
            .filter_map(|command| match command {
                DrawCommand::Glyphs { origin, .. } => Some(*origin),
                _ => None,
            })
            .collect();
        assert_eq!(runs.len(), 1 + 2 + 2 * shown, "title, captions, shown rows");
        let frames = pictures(&dl).len();
        assert_eq!(
            frames,
            1 + 9 + 9 * shown,
            "scrim, panel, shown rows' frames"
        );
        // Every row's text starts inside the viewport's reach: none is drawn
        // at a hidden row's place.
        for origin in &runs[3..] {
            assert!(
                origin.y + layout.style().item_size > viewport.0.y && origin.y < viewport.1.y,
                "a row's text at {origin:?} is out of {viewport:?}"
            );
        }
        // And the art alone keeps the list's clip.
        let mut art = DrawList::new();
        menu.render_art(&mut art, &layout, &skin());
        assert_eq!(art.len(), frames);
        assert!(
            art.clips()[10..]
                .iter()
                .all(|clip| clip.min == viewport.0 && clip.max == viewport.1),
            "a row's frame lost the list's clip"
        );
    }

    /// **The wheel scrolls the list, and the keyboard takes it back.** A
    /// wheel over the panel moves the list and reports it — even off the
    /// selection, which stays where it was — a wheel off the panel, or past
    /// the end, reports nothing, and the next keyboard move scrolls its row
    /// back into view.
    #[test]
    fn the_wheel_scrolls_the_list_and_the_keyboard_takes_it_back() {
        let mut menu = scene_menu();
        let layout = fitted(&menu, SMALL);
        let (panel_min, panel_max) = layout.panel();
        let over = (panel_min + panel_max) * 0.5;
        assert_eq!(layout.scroll(), 0.0);
        assert!(!menu.scroll_wheel(&layout, Vec2::new(1.0, 1.0), Vec2::new(0.0, 60.0)));
        assert!(menu.scroll_wheel(&layout, over, Vec2::new(0.0, 60.0)));
        let wheeled = fitted(&menu, SMALL);
        assert_eq!(wheeled.scroll(), 60.0, "the wheel did not move the list");
        assert_eq!(menu.selected(), 0);
        assert!(!wheeled.shows(0), "60px should scroll the first row away");

        assert!(menu.scroll_wheel(&wheeled, over, Vec2::new(0.0, 10_000.0)));
        let bottom = fitted(&menu, SMALL);
        assert!(!menu.scroll_wheel(&bottom, over, Vec2::new(0.0, 10.0)));
        assert!(bottom.shows(SCENES - 1));

        menu.select_next();
        let back = fitted(&menu, SMALL);
        let row = back.items()[1];
        assert!(within(
            (row.min, row.max),
            back.viewport().expect("scrolls")
        ));

        // A menu that fits unscrolled has no list for the wheel to move.
        let small = pause_menu();
        let unscrolled = small.layout((960, 720), &atlas());
        assert!(!menu.clone().scroll_wheel(
            &unscrolled,
            unscrolled.panel_centre(),
            Vec2::new(0.0, 10.0)
        ));
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
    /// the property [`DrawList::nine_slice`] implements — a caller that laid out
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

    /// **The menu draws back to front: the scrim, the frame and the title on
    /// it, then each button's frame followed by the strings on that button** —
    /// every picture before the text that sits on it, so no frame paints over
    /// a label — and the text is the title, every label and every hint.
    #[test]
    fn render_draws_each_frame_before_the_text_on_it() {
        let atlas = atlas();
        let menu = pause_menu();
        let layout = menu.layout((960, 720), &atlas);
        let mut dl = DrawList::new();
        menu.render(&mut dl, &layout, &skin());

        // `I` for a picture, the string itself for text: one scrim, nine panel
        // quads (every band non-empty at this size), the title, then nine
        // quads and two strings per button.
        let shape: Vec<String> = dl
            .commands()
            .iter()
            .map(|command| match command {
                DrawCommand::Image { .. } => "I".to_owned(),
                DrawCommand::Text { text, .. } => text.clone(),
                other => panic!("a menu of buttons drew {other:?}"),
            })
            .collect();
        let quads = |count: usize| std::iter::repeat_n("I".to_owned(), count);
        let mut expected: Vec<String> = quads(1 + 9).collect();
        expected.push("PAUSED".to_owned());
        for (label, hint) in [
            ("RESUME", "ESC"),
            ("FULLSCREEN", "F11"),
            ("DEBUG OVERLAY", "F3"),
        ] {
            expected.extend(quads(9));
            expected.extend([label.to_owned(), hint.to_owned()]);
        }
        assert_eq!(shape, expected);
    }

    /// An item with no hint draws one string, not an empty second one.
    #[test]
    fn an_item_with_no_hint_draws_no_hint() {
        let atlas = atlas();
        let menu = Menu::new("T", vec![MenuItem::new(1, "OK", "")]);
        let layout = menu.layout((960, 720), &atlas);
        let mut dl = DrawList::new();
        menu.render(&mut dl, &layout, &skin());
        assert_eq!(
            dl.len(),
            1 + 9 + 9 + 2,
            "the scrim, the frame, one button, the title and the label, and nothing else"
        );
    }

    /// The `Image` rectangles of `commands`, with their UV origin.
    fn pictures(dl: &DrawList) -> Vec<(Vec2, Vec2, Vec2)> {
        dl.commands()
            .iter()
            .filter_map(|command| match command {
                DrawCommand::Image {
                    min, max, uv_min, ..
                } => Some((*min, *max, *uv_min)),
                _ => None,
            })
            .collect()
    }

    /// **The scrim covers the framebuffer, tinted by the style, and the frame's
    /// nine quads tile exactly the panel the layout placed** — so the art is
    /// centred wherever the layout is, and the layout's centring tests speak for
    /// the picture.
    #[test]
    fn the_scrim_covers_the_screen_and_the_frame_tiles_the_panel() {
        let atlas = atlas();
        let menu = pause_menu();
        let skin = skin();
        for extent in EXTENTS {
            let layout = menu.layout(extent, &atlas);
            let mut dl = DrawList::new();
            menu.render(&mut dl, &layout, &skin);

            let DrawCommand::Image { min, max, tint, .. } = &dl.commands()[0] else {
                panic!("{extent:?}: the first command is not the scrim");
            };
            assert_eq!((*min, *max), layout.scrim(), "{extent:?}");
            assert_eq!(*tint, layout.style().scrim_color);

            let frame = &pictures(&dl)[1..10];
            let area: f32 = frame
                .iter()
                .map(|(min, max, _)| (max.x - min.x) * (max.y - min.y))
                .sum();
            let (panel_min, panel_max) = layout.panel();
            let low = frame
                .iter()
                .fold(Vec2::splat(f32::MAX), |acc, q| acc.min(q.0));
            let high = frame
                .iter()
                .fold(Vec2::splat(f32::MIN), |acc, q| acc.max(q.1));
            assert_eq!((low, high), (panel_min, panel_max), "{extent:?}");
            assert_eq!(area, layout.panel_size().x * layout.panel_size().y);
        }
    }

    /// **A button's state changes the picture it samples, not the rectangle it
    /// covers.** A `ButtonState` that never reached the art is a menu whose
    /// buttons never light up and whose every layout test still passes.
    #[test]
    fn a_state_change_moves_the_uvs_and_leaves_the_rectangle_alone() {
        let atlas = atlas();
        let skin = skin();
        let mut menu = pause_menu();
        let layout = menu.layout((960, 720), &atlas);
        let button = |menu: &Menu, index: usize| {
            let mut dl = DrawList::new();
            menu.render(&mut dl, &layout, &skin);
            pictures(&dl)[10 + index * 9..10 + (index + 1) * 9].to_vec()
        };

        // Item 0 is selected, so it draws `Hovered` and the others `Idle`.
        let idle = button(&menu, 1);
        let hovered = button(&menu, 0);
        menu.press(true);
        let pressed = button(&menu, 0);
        assert_eq!(menu.state(0), ButtonState::Pressed);

        for quad in 0..9 {
            assert_eq!(
                (hovered[quad].0, hovered[quad].1),
                (pressed[quad].0, pressed[quad].1)
            );
            assert_ne!(hovered[quad].2, pressed[quad].2, "quad {quad}");
            assert_ne!(idle[quad].2, hovered[quad].2, "quad {quad}");
            assert_ne!(idle[quad].2, pressed[quad].2, "quad {quad}");
        }
    }

    /// **The frame's corners are the art's insets at the menu's scale** — four
    /// texels at scale three is twelve pixels — whatever the panel's size.
    #[test]
    fn the_frames_corner_quads_are_the_insets_at_the_menus_scale() {
        let atlas = atlas();
        let skin = skin();
        let style = MenuStyle::pixel_art(3);
        let small = Menu::new("GO", vec![MenuItem::new(1, "OK", "")]);
        let large = Menu::new(
            "A MUCH LONGER TITLE",
            (0..5)
                .map(|i| MenuItem::new(i, "A VERY LONG MENU ITEM LABEL", "SHIFT+F11"))
                .collect(),
        );
        let corners = |menu: &Menu| {
            let layout = menu.layout_with((1600, 1200), &atlas, &style);
            let mut dl = DrawList::new();
            menu.render(&mut dl, &layout, &skin);
            let frame = pictures(&dl)[1..10].to_vec();
            [0usize, 2, 6, 8].map(|index| (frame[index].1 - frame[index].0, frame[index].2))
        };
        let (a, b) = (corners(&small), corners(&large));
        assert_eq!(a, b, "a corner changed size or picture with the menu");
        for (size, _) in a {
            assert_eq!(size, Vec2::splat(PANEL_INSETS.left * style.scale));
        }
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

    /// **A cancelled press fires nothing and leaves nothing latched**, which is
    /// what lets the next press latch at all.
    ///
    /// The second half is the one that matters: a capture left behind by a
    /// gesture the system took away is not visible — the panel looks idle — and
    /// the symptom is every later press doing nothing, on touch only.
    #[test]
    fn a_cancelled_press_fires_nothing_and_frees_the_capture() {
        let extent = (960, 720);
        let mut menus = menus();
        menus.show(Kind::Paused);
        let target = over(&menus, extent, 1);

        assert!(!menus.press_captured(), "nothing is pressed yet");
        menus.point(extent, &atlas(), press_at(target));
        assert!(menus.press_captured(), "the press latched nothing");

        menus.cancel_press();
        assert!(
            !menus.press_captured(),
            "the cancel left the capture behind"
        );
        assert_eq!(
            menus.current().expect("a menu").state(1),
            ButtonState::Idle,
            "the button stayed pressed under a finger that is gone",
        );
        assert_eq!(
            menus.point(extent, &atlas(), release_at(target)),
            None,
            "the cancelled press fired on the lift anyway",
        );

        // And the button still works afterwards, which is what a capture left
        // latched would have made impossible.
        menus.point(extent, &atlas(), press_at(target));
        assert_eq!(menus.point(extent, &atlas(), release_at(target)), Some(2));
    }

    /// **A press on the panel's background captures nothing**, so a caller
    /// driving the menu with contacts can tell the press it must remember from
    /// the one it must let go of.
    #[test]
    fn a_press_that_lands_on_no_button_captures_nothing() {
        let extent = (960, 720);
        let mut menus = menus();
        menus.show(Kind::Paused);

        menus.point(extent, &atlas(), press_at(Vec2::new(3.0, 3.0)));
        assert!(
            !menus.press_captured(),
            "a press in the corner of the screen latched a button",
        );
        menus.point(extent, &atlas(), press_at(over(&menus, extent, 0)));
        assert!(menus.press_captured());
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
    // -----------------------------------------------------------------------
    // Sliders
    // -----------------------------------------------------------------------

    /// The id of the slider row in [`slider_menu`].
    const DIAL: WidgetId = 20;

    /// A panel whose second row is a slider, so every test below has a button
    /// above it to prove the two kinds do not behave alike.
    fn slider_menu() -> Menu {
        Menu::new(
            "OPTIONS",
            vec![
                MenuItem::new(10, "BACK", "ESC"),
                MenuItem::slider(DIAL, "EXPOSURE", "1.00", 0.5),
            ],
        )
    }

    /// The slider row's groove and the travel its handle's centre has inside
    /// it, at `extent`.
    ///
    /// Both read off the layout the menu actually chose. A test that measured
    /// the travel at [`MenuStyle::pixel_art`]`(1)` would be a quarter of a
    /// handle out at the scale a 960x720 window picks, and would report the
    /// clamp as a rounding error.
    fn groove(menu: &Menu, extent: (u32, u32)) -> ((Vec2, Vec2), (f32, f32)) {
        let layout = menu.layout(extent, &atlas());
        let track = layout.items()[1]
            .track
            .expect("the slider row has a groove");
        (track, handle_travel(track, layout.style().handle_size.x))
    }

    /// **A slider is dragged, not clicked.** The handle follows the pointer's
    /// `x` while the press is held, which is the whole of the control.
    #[test]
    fn dragging_a_slider_moves_the_handle_to_the_pointer() {
        let extent = (960, 720);
        let mut menu = slider_menu();
        let mut ui = UiState::new();
        let (track, (start, end)) = groove(&menu, extent);

        for (fraction, expected) in [(0.0, 0.0), (0.25, 0.25), (1.0, 1.0)] {
            let layout = menu.layout(extent, &atlas());
            let x = start + (end - start) * fraction;
            let y = (track.0.y + track.1.y) * 0.5;
            menu.point(&layout, &mut ui, press_at(Vec2::new(x, y)));
            let at = menu.slider(DIAL).expect("the row is a slider");
            assert!(
                (at - expected).abs() <= 1e-3,
                "a press {fraction} of the way along put the handle at {at}",
            );
        }
    }

    /// **Past either end is the end, not a wrap and not a `NaN`.** A pointer
    /// dragged off the panel entirely is the ordinary way a player reaches the
    /// minimum, so the clamp is the feature.
    #[test]
    fn a_drag_past_either_end_of_the_groove_stops_there() {
        let extent = (960, 720);
        let mut menu = slider_menu();
        let mut ui = UiState::new();
        let (track, _) = groove(&menu, extent);
        let y = (track.0.y + track.1.y) * 0.5;
        let layout = menu.layout(extent, &atlas());

        menu.point(&layout, &mut ui, press_at(Vec2::new(track.0.x, y)));
        menu.point(
            &layout,
            &mut ui,
            press_at(Vec2::new(track.0.x - 10_000.0, y)),
        );
        assert_eq!(menu.slider(DIAL), Some(0.0));

        menu.point(
            &layout,
            &mut ui,
            press_at(Vec2::new(track.1.x + 10_000.0, y)),
        );
        assert_eq!(menu.slider(DIAL), Some(1.0));
    }

    /// **A drag that leaves the row keeps the handle.** Unlike a button, whose
    /// press is abandoned when the cursor wanders off it: a player watching the
    /// frame behind the panel drags upward out of the row as a matter of course,
    /// and a slider that let go there would be unusable.
    #[test]
    fn a_drag_that_leaves_the_row_still_moves_the_handle() {
        let extent = (960, 720);
        let mut menu = slider_menu();
        let mut ui = UiState::new();
        let (track, (start, end)) = groove(&menu, extent);
        let layout = menu.layout(extent, &atlas());

        let inside = Vec2::new(start, (track.0.y + track.1.y) * 0.5);
        menu.point(&layout, &mut ui, press_at(inside));
        assert_eq!(menu.slider(DIAL), Some(0.0));

        // Three quarters along, and a long way above the panel.
        let outside = Vec2::new(start + (end - start) * 0.75, 0.0);
        menu.point(&layout, &mut ui, press_at(outside));
        let at = menu.slider(DIAL).expect("the row is a slider");
        assert!(
            (at - 0.75).abs() <= 1e-3,
            "the drag left the handle at {at}"
        );
    }

    /// **A slider never reports an id**, from the commit key or from a release
    /// over it. Either would arrive at the game's action table looking exactly
    /// like a button press, and the game would run whatever that id means.
    #[test]
    fn a_slider_fires_nothing_from_either_device() {
        let extent = (960, 720);
        let mut menu = slider_menu();
        let mut ui = UiState::new();

        menu.select_next();
        assert_eq!(menu.selected(), 1, "the slider is the second row");
        menu.press(true);
        assert_eq!(menu.activate(), None, "the commit key fired the slider");

        let (track, _) = groove(&menu, extent);
        let at = Vec2::new((track.0.x + track.1.x) * 0.5, (track.0.y + track.1.y) * 0.5);
        let layout = menu.layout(extent, &atlas());
        menu.point(&layout, &mut ui, press_at(at));
        assert_eq!(
            menu.point(&layout, &mut ui, release_at(at)),
            None,
            "letting go of a slider was reported as a click",
        );

        // And the button above it still does, so the suppression is the
        // slider's and not a menu that stopped firing altogether.
        menu.select_previous();
        assert_eq!(menu.activate(), Some(10));
    }

    /// **The caller's write loses to the player's drag.** A game mirroring the
    /// value it read back into the handle every frame is the normal shape; if
    /// that write won, the handle would be pinned wherever the game last
    /// committed and the drag would go nowhere.
    #[test]
    fn a_write_is_refused_while_the_handle_is_held_and_taken_after() {
        let extent = (960, 720);
        let mut menu = slider_menu();
        let mut ui = UiState::new();
        let (track, _) = groove(&menu, extent);
        let y = (track.0.y + track.1.y) * 0.5;
        let layout = menu.layout(extent, &atlas());

        assert!(menu.set_slider(DIAL, 0.25), "an untouched handle takes it");
        assert_eq!(menu.slider(DIAL), Some(0.25));

        menu.point(&layout, &mut ui, press_at(Vec2::new(track.1.x, y)));
        assert_eq!(menu.slider(DIAL), Some(1.0));
        assert!(
            !menu.set_slider(DIAL, 0.25),
            "the write went through mid-drag",
        );
        assert_eq!(menu.slider(DIAL), Some(1.0));

        menu.point(&layout, &mut ui, release_at(Vec2::new(track.1.x, y)));
        assert!(menu.set_slider(DIAL, 0.25), "the drag was never let go of");
        assert_eq!(menu.slider(DIAL), Some(0.25));
    }

    /// A value neither the caller nor the pointer can name lands at the start of
    /// the groove, because `f32::clamp` would hand a `NaN` straight back.
    #[test]
    fn a_nan_position_lands_at_the_start() {
        let mut menu = slider_menu();
        assert!(menu.set_slider(DIAL, f32::NAN));
        assert_eq!(menu.slider(DIAL), Some(0.0));
        assert_eq!(Slider::new(f32::NAN).position(), 0.0);
        assert_eq!(Slider::new(f32::INFINITY).position(), 1.0);
    }

    /// Asking a button row for a value, or a row that is not there, is `None`
    /// rather than a number the caller would act on.
    #[test]
    fn only_a_slider_row_has_a_value() {
        let mut menu = slider_menu();
        assert_eq!(menu.slider(10), None, "the button row reported a value");
        assert_eq!(menu.slider(999), None, "an absent row reported a value");
        assert!(!menu.set_slider(10, 0.5), "a button row took a value");
    }

    /// **The keyboard moves the row the player can see is highlighted**, in
    /// both directions and by the step the constant names.
    #[test]
    fn a_key_moves_the_highlighted_slider_and_nothing_else() {
        let mut menu = slider_menu();
        // The button row is selected first, so a nudge has no slider to move.
        assert!(!menu.nudge_slider(true), "a button row took a nudge");
        assert_eq!(menu.slider(DIAL), Some(0.5), "the far row moved anyway");

        menu.select_next();
        assert!(menu.nudge_slider(true), "the highlighted slider refused");
        assert_eq!(menu.slider(DIAL), Some(0.5 + Slider::KEY_STEP));
        assert!(menu.nudge_slider(false), "the slider refused to come back");
        assert_eq!(menu.slider(DIAL), Some(0.5));
    }

    /// **The end of the groove is a place, and a key held against it stays
    /// there.** The return value is what tells the two apart: nothing moved.
    #[test]
    fn a_nudge_at_the_end_of_the_groove_moves_nothing() {
        let mut menu = slider_menu();
        menu.select_next();
        // Twenty steps is the whole groove from the middle twice over, so this
        // arrives at the end whatever `KEY_STEP` is set to.
        for _ in 0..(1.0 / Slider::KEY_STEP) as u32 + 1 {
            menu.nudge_slider(true);
        }
        assert_eq!(menu.slider(DIAL), Some(1.0), "the handle left the groove");
        assert!(
            !menu.nudge_slider(true),
            "a handle at the end reported that it moved"
        );
    }

    /// **A drag is the newer input**, which is [`Menu::set_slider`]'s rule and
    /// applies to a key for the same reason: a keyboard repeat arriving while
    /// the player has the handle would fight the cursor for it.
    #[test]
    fn a_key_does_not_move_a_handle_the_pointer_is_holding() {
        let extent = (960, 720);
        let mut menu = slider_menu();
        let mut ui = UiState::new();
        let (track, _) = groove(&menu, extent);
        let middle = Vec2::new((track.0.x + track.1.x) * 0.5, (track.0.y + track.1.y) * 0.5);
        let layout = menu.layout(extent, &atlas());

        menu.point(&layout, &mut ui, press_at(middle));
        let held = menu.slider(DIAL).expect("the row is a slider");
        assert!(!menu.nudge_slider(true), "a held handle took a key");
        assert_eq!(menu.slider(DIAL), Some(held), "the held handle moved");

        // And it takes one again once the pointer lets go, from the position
        // the drag left it — not from the one the key would have written.
        menu.point(&layout, &mut ui, release_at(middle));
        assert!(
            menu.nudge_slider(false),
            "the released handle refused a key"
        );
        assert!(
            menu.slider(DIAL).expect("still a slider") < held,
            "the key moved it the wrong way, or not at all, from {held}"
        );
    }

    /// **The groove is reserved, not squeezed in.** The panel grows by at least
    /// a groove and its gap over the same menu with a button in that row — a
    /// slider row that fitted in a button's width would have laid its groove
    /// over the label.
    #[test]
    fn a_slider_row_widens_the_panel_by_its_groove() {
        let atlas = atlas();
        let style = MenuStyle::pixel_art(1);
        let with_button = Menu::new(
            "OPTIONS",
            vec![
                MenuItem::new(10, "BACK", "ESC"),
                MenuItem::new(DIAL, "EXPOSURE", "1.00"),
            ],
        );
        let grew =
            slider_menu().panel_size(&atlas, &style).x - with_button.panel_size(&atlas, &style).x;
        assert!(
            grew >= style.track_width + style.hint_gap - 1e-3,
            "a slider row only widened the panel by {grew}",
        );
    }

    /// **The groove sits between the label and the value, and the handle inside
    /// the groove — at both ends.** A handle centred on the groove's end would
    /// hang half of itself over the button's face, so nought and one would look
    /// like different amounts of overhang rather than like the two extremes.
    #[test]
    fn the_handle_stays_inside_the_groove_at_both_ends() {
        let extent = (960, 720);
        let atlas = atlas();
        for position in [0.0, 0.5, 1.0] {
            let mut menu = slider_menu();
            assert!(menu.set_slider(DIAL, position));
            let layout = menu.layout(extent, &atlas);
            let placed = layout.items()[1];
            let track = placed.track.expect("a groove");

            let label_end = placed.label_pos.x + atlas.text_width("EXPOSURE", 1.0);
            assert!(
                track.0.x >= label_end && track.1.x <= placed.hint_pos.x,
                "at {position} the groove {track:?} overlaps the row's text",
            );

            let mut dl = DrawList::new();
            menu.render(&mut dl, &layout, &skin());
            let handle = dl
                .commands()
                .iter()
                .filter_map(|command| match command {
                    DrawCommand::Rect { min, max, color }
                        if *color == layout.style().handle_color =>
                    {
                        Some((*min, *max))
                    }
                    _ => None,
                })
                .next()
                .expect("the handle is drawn");
            assert!(
                handle.0.x >= track.0.x - 1e-3 && handle.1.x <= track.1.x + 1e-3,
                "at {position} the handle {handle:?} left the groove {track:?}",
            );
        }
    }

    /// The filled part of the groove is what says how far along the handle is
    /// without reading the number, so it has to actually track the value.
    #[test]
    fn the_fill_grows_with_the_value() {
        let extent = (960, 720);
        let atlas = atlas();
        let mut widths = Vec::new();
        for position in [0.0, 0.5, 1.0] {
            let mut menu = slider_menu();
            assert!(menu.set_slider(DIAL, position));
            let layout = menu.layout(extent, &atlas);
            let mut dl = DrawList::new();
            menu.render(&mut dl, &layout, &skin());
            let fill = dl
                .commands()
                .iter()
                .find_map(|command| match command {
                    DrawCommand::Rect { min, max, color }
                        if *color == layout.style().fill_color =>
                    {
                        Some(max.x - min.x)
                    }
                    _ => None,
                })
                .expect("the fill is drawn");
            widths.push(fill);
        }
        assert!(
            widths[0] < widths[1] && widths[1] < widths[2],
            "the fill widths were {widths:?}",
        );
    }
    // -----------------------------------------------------------------------
    // Cyclers
    // -----------------------------------------------------------------------

    /// The id of the cycler row in [`cycler_menu`].
    const MODE: WidgetId = 30;

    /// The captions the cycler row in [`cycler_menu`] walks, in order — the
    /// caller's list, which the widget never sees.
    const MODES: [&str; 3] = ["windowed", "borderless", "exclusive"];

    /// A panel whose second row is a cycler holding the middle of three
    /// choices, with a button above it for the same reason [`slider_menu`]
    /// has one.
    fn cycler_menu() -> Menu {
        Menu::new(
            "OPTIONS",
            vec![
                MenuItem::new(10, "BACK", "ESC"),
                MenuItem::cycler(MODE, "DISPLAY", MODES[1], MODES.len(), 1),
            ],
        )
    }

    /// **The arrows walk the list and stop at its ends.** The return value is
    /// what tells a step from a key held against the end, exactly as
    /// [`Menu::nudge_slider`] reports the end of a groove — and a button row
    /// under the highlight takes no step at all.
    #[test]
    fn a_key_steps_the_highlighted_cycler_and_stops_at_its_ends() {
        let mut menu = cycler_menu();
        assert!(
            !menu.cycler_highlighted(),
            "the button row read as a cycler"
        );
        assert!(!menu.nudge_cycler(true), "a button row took a step");
        assert_eq!(menu.cycler(MODE), Some(1), "the far row stepped anyway");

        menu.select_next();
        assert!(menu.cycler_highlighted());
        assert!(menu.nudge_cycler(true), "the highlighted cycler refused");
        assert_eq!(menu.cycler(MODE), Some(2));
        assert!(
            !menu.nudge_cycler(true),
            "the last choice reported a step forward"
        );
        assert_eq!(menu.cycler(MODE), Some(2), "a key at the end went round");

        assert!(menu.nudge_cycler(false));
        assert!(menu.nudge_cycler(false));
        assert_eq!(menu.cycler(MODE), Some(0));
        assert!(
            !menu.nudge_cycler(false),
            "the first choice reported a step back"
        );
        assert_eq!(menu.cycler(MODE), Some(0), "a key at the start went round");
    }

    /// **The commit key and a click step a cycler forward and round, and
    /// neither reports an id.** One key reaches every choice, which is what
    /// the row did when it was a button; and nothing from it arrives at the
    /// game's action table, which is the slider's rule for the same reason.
    #[test]
    fn the_commit_key_and_a_click_step_a_cycler_round_and_report_nothing() {
        let extent = (960, 720);
        let mut menu = cycler_menu();
        let mut ui = UiState::new();

        menu.select_next();
        menu.press(true);
        assert_eq!(menu.activate(), None, "the commit key fired the cycler");
        assert_eq!(menu.cycler(MODE), Some(2));
        assert_eq!(menu.activate(), None);
        assert_eq!(
            menu.cycler(MODE),
            Some(0),
            "the commit key stopped at the end"
        );

        let layout = menu.layout(extent, &atlas());
        let row = layout.items()[1];
        let at = (row.min + row.max) * 0.5;
        menu.point(&layout, &mut ui, press_at(at));
        assert_eq!(
            menu.point(&layout, &mut ui, release_at(at)),
            None,
            "a click on a cycler was reported as a click",
        );
        assert_eq!(menu.cycler(MODE), Some(1), "the click did not step it");

        // And the button above it still fires, so the suppression is the
        // cycler's and not a menu that stopped firing altogether.
        menu.select_previous();
        assert_eq!(menu.activate(), Some(10));
    }

    /// Asking a button row for a choice, or a row that is not there, is `None`
    /// rather than an index the caller would look up; a write past the end
    /// lands on the last choice, and a write to a button row is refused.
    #[test]
    fn only_a_cycler_row_has_a_choice() {
        let mut menu = cycler_menu();
        assert_eq!(menu.cycler(10), None, "the button row reported a choice");
        assert_eq!(menu.cycler(999), None, "an absent row reported a choice");
        assert!(!menu.set_cycler(10, 1), "a button row took a choice");
        assert!(menu.set_cycler(MODE, 99));
        assert_eq!(menu.cycler(MODE), Some(MODES.len() - 1));
        assert!(menu.set_cycler(MODE, 0));
        assert_eq!(menu.cycler(MODE), Some(0));
    }

    /// **A chevron is drawn only on the side a step would go, and the caption
    /// does not move as they come and go.** The blank that stands in for a
    /// missing chevron is the same width, which the built-in atlas's uniform
    /// advance guarantees and this checks rather than assumes — with one
    /// caption for every choice, so the only thing that changes between the
    /// three layouts is the chevrons.
    #[test]
    fn a_cycler_draws_a_chevron_only_where_a_step_would_go() {
        let atlas = atlas();
        let extent = (960, 720);
        let mut menu = cycler_menu();
        assert!(menu.set_item_hint(MODE, "same"));
        let mut seen = Vec::new();
        for (chosen, expected) in [(1, "< same >"), (0, "  same >"), (2, "< same  ")] {
            assert!(menu.set_cycler(MODE, chosen));
            let layout = menu.layout(extent, &atlas);
            let row = layout.items()[1];
            assert!(row.track.is_none(), "a cycler row was given a groove");
            let mut dl = DrawList::new();
            menu.render(&mut dl, &layout, &skin());
            let texts: Vec<&str> = dl
                .commands()
                .iter()
                .filter_map(|command| match command {
                    DrawCommand::Text { text, .. } => Some(text.as_str()),
                    _ => None,
                })
                .collect();
            assert!(
                texts.contains(&expected),
                "choice {chosen} drew {texts:?}, not {expected:?}"
            );
            seen.push((row.hint_pos, layout.panel_size()));
        }
        let (anchor, panel) = seen[0];
        for (pos, size) in &seen[1..] {
            assert_eq!(*pos, anchor, "the caption moved as the chevrons changed");
            assert_eq!(*size, panel, "the panel resized as the chevrons changed");
        }
    }

    /// A cycler with no choices has nothing to step to and says so from both
    /// keys, rather than wrapping through a division by nothing; and a choice
    /// past the end lands on the last one.
    #[test]
    fn a_cycler_with_no_choices_refuses_every_step() {
        let mut empty = Cycler::new(0, 3);
        assert_eq!(empty.chosen(), 0);
        for (forward, wrap) in [(true, true), (true, false), (false, true), (false, false)] {
            assert!(!empty.step(forward, wrap), "an empty cycler stepped");
        }
        assert_eq!(Cycler::new(3, 9).chosen(), 2);

        let mut round = Cycler::new(3, 2);
        assert!(round.step(true, true), "a wrap from the end did not move");
        assert_eq!(round.chosen(), 0);
        assert!(round.step(false, true));
        assert_eq!(round.chosen(), 2);
        assert!(!round.step(true, false));
        assert_eq!(round.chosen(), 2);
    }

    // -----------------------------------------------------------------------
    // The tree and the sheet
    // -----------------------------------------------------------------------

    /// **The panel's size is the scale-1 panel times the scale, exactly**, for
    /// every kind of row — the property [`Menu::layout`]'s one-measurement fit
    /// rests on. A length that stopped being a whole number times the scale,
    /// or a rounding that stopped being exact, fails here rather than as a
    /// menu that silently picks the wrong scale.
    #[test]
    fn the_panel_grows_exactly_with_the_scale() {
        let atlas = atlas();
        let mut mixed = slider_menu();
        mixed
            .items
            .push(MenuItem::cycler(MODE, "DISPLAY", "windowed", 3, 1));
        mixed.items.push(MenuItem::new(40, "NO HINT", ""));
        for menu in [pause_menu(), mixed, Menu::new("", Vec::new())] {
            let base = menu.panel_size(&atlas, &MenuStyle::pixel_art(1));
            assert!(base.x > 0.0 && base.y > 0.0, "{base:?}");
            for scale in 2..=MenuStyle::MAX_SCALE {
                assert_eq!(
                    menu.panel_size(&atlas, &MenuStyle::pixel_art(scale)),
                    base * scale as f32,
                    "{:?} at scale {scale}",
                    menu.title,
                );
            }
        }
    }

    /// **`default.css` draws the colours [`MenuStyle::pixel_art`] restates,
    /// and cuts the frames at the insets the layout assumes** — the two
    /// places a sheet and the style could drift apart without a layout test
    /// noticing.
    #[test]
    fn the_sheet_draws_the_colours_and_the_slices_the_style_restates() {
        let atlas = atlas();
        let skin = skin();
        let mut menu = slider_menu();
        menu.items.push(MenuItem::new(11, "QUIT", "Q"));
        let layout = menu.layout((960, 720), &atlas);
        let style = layout.style();
        let mut dl = DrawList::new();
        menu.render(&mut dl, &layout, &skin);

        let text_colour = |wanted: &str| {
            dl.commands()
                .iter()
                .find_map(|command| match command {
                    DrawCommand::Text { text, color, .. } if text == wanted => Some(*color),
                    _ => None,
                })
                .unwrap_or_else(|| panic!("{wanted} is not drawn"))
        };
        assert_eq!(text_colour("OPTIONS"), style.title_color);
        assert_eq!(text_colour("BACK"), style.label_color);
        assert_eq!(text_colour("ESC"), style.hint_color);
        let rect_colours: Vec<[f32; 4]> = dl
            .commands()
            .iter()
            .filter_map(|command| match command {
                DrawCommand::Rect { color, .. } => Some(*color),
                _ => None,
            })
            .collect();
        assert_eq!(
            rect_colours,
            [style.track_color, style.fill_color, style.handle_color],
            "the groove, its fill and the handle, in that order"
        );
        let DrawCommand::Image { tint, .. } = dl.commands()[0] else {
            panic!("the scrim is not first");
        };
        assert_eq!(tint, style.scrim_color);

        let corner_uvs = |min: usize| {
            let DrawCommand::Image { uv_min, uv_max, .. } = pictures_with_uv_max(&dl)[min] else {
                unreachable!("filtered to images");
            };
            (uv_min, uv_max)
        };
        assert_eq!(
            corner_uvs(1),
            (
                skin.panel.image.uv(Vec2::ZERO),
                skin.panel
                    .image
                    .uv(Vec2::new(PANEL_INSETS.left, PANEL_INSETS.top))
            ),
            "the panel is not cut at PANEL_INSETS"
        );
        // Row 0 is selected, so it draws the hovered frame; row 1 is idle.
        assert_eq!(
            corner_uvs(10),
            (
                skin.buttons.hovered.image.uv(Vec2::ZERO),
                skin.buttons
                    .hovered
                    .image
                    .uv(Vec2::new(BUTTON_INSETS.left, BUTTON_INSETS.top))
            ),
            "the buttons are not cut at BUTTON_INSETS"
        );
        assert_eq!(
            corner_uvs(19).0,
            skin.buttons.idle.image.uv(Vec2::ZERO),
            "an idle row does not draw the idle frame"
        );
    }

    /// Every `Image` command of `dl`, whole.
    fn pictures_with_uv_max(dl: &DrawList) -> Vec<DrawCommand> {
        dl.commands()
            .iter()
            .filter(|command| matches!(command, DrawCommand::Image { .. }))
            .cloned()
            .collect()
    }
}
