//! The on-screen keyboard the console draws for a device that has no keys.
//!
//! `docs/plan/52-debug-console.md` decision 6 recorded "no on-screen keyboard"
//! as a known gap: a phone can open the panel and press **Send**, and can type
//! nothing into it. This is that gap closed, and it is **drawn** rather than
//! borrowed from the platform.
//!
//! # Why a drawn keyboard and not the platform's
//!
//! The browser's answer to "raise the on-screen keyboard" is to focus an
//! editable DOM element, and that answer was weighed and declined for three
//! reasons, each of which is a thing this crate cannot do anything about:
//!
//! - **It is one platform's answer.** No native backend in this workspace
//!   reports a contact at all — `crcbl_shell::ShellCaps::TOUCH` is set by the
//!   web backend and by `HeadlessShell`, and `crates/crcbl-shell/src/caps.rs`
//!   asserts Wayland does not set it — so a DOM input would leave every other
//!   backend with the gap it started with. A keyboard drawn from a
//!   [`DrawList`] is the same keyboard everywhere, including in a headless
//!   test.
//! - **It would take the keys away from the panel.** `web/engine/shell.js`
//!   listens for `keydown` on the **canvas**, and focuses the canvas on every
//!   `pointerdown`. An editable element focused while the console is open is an
//!   element holding the keyboard focus the console's own `Tab`, arrows,
//!   `PageUp`, `Escape` and `Ctrl`+`V` handling is read from.
//! - **It could type what the console cannot draw.** The built-in
//!   [`FontAtlas`] covers printable ASCII only, so a system keyboard's accented
//!   or CJK output would reach the field and be drawn as the not-def glyph.
//!   Every key here types a character the atlas has.
//!
//! # The layers, and that every printable character is on one of them
//!
//! Three layers — lower-case letters, upper-case letters, and one holding the
//! digits and **all** of ASCII's punctuation. Between them they cover the
//! atlas's whole printable range, which
//! `layers_cover_every_printable_character` asserts rather than claims.
//!
//! [`Layer::Symbols`] carries one more row than the letter layers, so the
//! keyboard is laid out from its **bottom** edge: the control row a thumb rests
//! on stays where it is when the layer changes, and only the top edge moves.

use glam::Vec2;

use crate::draw_list::DrawList;
use crate::text::FontAtlas;
use crate::widget::{ButtonState, NATURAL_FONT_SIZE, PointerInput, UiState, WidgetId};

use super::{ConsoleStyle, panel::SEND_ID};

/// The share of the frame's height a **letter** layer covers, along the bottom
/// edge.
///
/// A third, which is where a phone's own keyboard sits and about what it takes.
/// The panel above it is [`CONSOLE_HEIGHT_FRACTION`](super::CONSOLE_HEIGHT_FRACTION),
/// and the two together deliberately leave a strip of the game visible between
/// them: a console that covered the frame would be a console you cannot see the
/// effect of a command through.
///
/// It sizes a **row**, not the keyboard: [`Layer::Symbols`] has one row more
/// than a letter layer and is that much taller, because the alternative is
/// shorter keys on the layer whose keys are already the narrowest.
pub const KEYBOARD_HEIGHT_FRACTION: f32 = 0.34;

/// How many [`WidgetId`]s the keyboard reserves below [`SEND_ID`].
///
/// Every key interacts under an id of its own, because [`UiState`] captures a
/// press by id and a keyboard whose keys shared one would let a press that
/// started on `q` commit whatever the finger slid onto. Held to the widest
/// layer by `every_layer_fits_the_reserved_ids`, so a row added to a layer
/// cannot silently start colliding with **Send**.
pub const KEY_ID_SPAN: WidgetId = 64;

/// The first [`WidgetId`] the keyboard's keys use.
///
/// [`SEND_ID`]'s reasoning, one block lower: the console is given its own
/// [`UiState`] and a game numbering its buttons from zero never reaches here.
pub const KEY_ID_BASE: WidgetId = SEND_ID - KEY_ID_SPAN;

/// The lower-case layer's character rows, widest first.
const LOWER_ROWS: [&str; 3] = ["qwertyuiop", "asdfghjkl", "zxcvbnm"];

/// The upper-case layer's, in the same order and the same shape.
const UPPER_ROWS: [&str; 3] = ["QWERTYUIOP", "ASDFGHJKL", "ZXCVBNM"];

/// The digits and every punctuation mark printable ASCII has.
///
/// Four rows rather than three because there are more of them than a letter
/// layer holds, and eleven to a row rather than ten so the fourth row is the
/// only short one.
const SYMBOL_ROWS: [&str; 4] = ["1234567890-", "=[]{}\\|;:'\"", ",.<>/?!@#$%", "^&*()_+~`"];

/// The control row's keys, and the share of the keyboard's width each takes.
///
/// Fractions of the whole width rather than cells of a layer's grid, so the
/// four of them land in exactly the same place on every layer — the space bar
/// is the most-pressed key here and a thumb should not have to look for it
/// again after a layer change.
const CONTROL_ROW: [(KeyCap, f32); 5] = [
    (KeyCap::Shift, 0.20),
    (KeyCap::Symbols, 0.20),
    (KeyCap::Type(' '), 0.28),
    (KeyCap::Backspace, 0.16),
    (KeyCap::Enter, 0.16),
];

/// Which set of characters the keyboard is offering.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum Layer {
    /// The lower-case letters, which is what a console line is mostly made of.
    #[default]
    Lower,
    /// The upper-case letters. Reached with [`KeyCap::Shift`].
    Upper,
    /// The digits and the punctuation. Reached with [`KeyCap::Symbols`].
    Symbols,
}

impl Layer {
    /// This layer's character rows.
    const fn rows(self) -> &'static [&'static str] {
        match self {
            Self::Lower => &LOWER_ROWS,
            Self::Upper => &UPPER_ROWS,
            Self::Symbols => &SYMBOL_ROWS,
        }
    }

    /// How many cells wide this layer's grid is: its longest row.
    fn columns(self) -> usize {
        self.rows()
            .iter()
            .map(|row| row.chars().count())
            .max()
            .unwrap_or(1)
            .max(1)
    }

    /// Where [`KeyCap::Shift`] goes from here.
    ///
    /// A plain toggle rather than a phone's one-shot latch: a latch that
    /// dropped back to lower case after one letter would make `KeyF` — the
    /// spelling `bind` takes — four taps of shift instead of two.
    const fn shifted(self) -> Self {
        match self {
            Self::Lower | Self::Symbols => Self::Upper,
            Self::Upper => Self::Lower,
        }
    }

    /// Where [`KeyCap::Symbols`] goes from here.
    const fn toggled_symbols(self) -> Self {
        match self {
            Self::Lower | Self::Upper => Self::Symbols,
            Self::Symbols => Self::Lower,
        }
    }
}

/// What one key does when it is pressed.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum KeyCap {
    /// Types this character into the field.
    Type(char),
    /// Deletes the character before the caret.
    Backspace,
    /// Submits the line — the return key every keyboard has.
    ///
    /// The panel's **Send** button does the same thing and stays: this is the
    /// key a thumb is already over, and that one is the button a mouse is. They
    /// are one behaviour through
    /// [`ConsolePanel::submit`](super::ConsolePanel::submit) and cannot come to
    /// mean different things.
    Enter,
    /// Swaps the letter case, and leaves [`Layer::Symbols`] for it.
    Shift,
    /// Swaps between the letters and the digits-and-punctuation layer.
    Symbols,
}

impl KeyCap {
    /// What is drawn on this key, given the layer it is drawn on.
    ///
    /// Only [`KeyCap::Shift`] and [`KeyCap::Symbols`] read the layer, and both
    /// do it for the same reason: a key that switches between two things has to
    /// say which of them it is about to switch **to**.
    #[must_use]
    pub fn label(self, layer: Layer) -> &'static str {
        match self {
            // The one character with no ink of its own. Named rather than drawn
            // blank, because a blank key in a row of blank keys is a gap.
            Self::Type(' ') => "SPACE",
            Self::Type(character) => printable_label(character),
            Self::Backspace => "BKSP",
            Self::Enter => "ENTER",
            Self::Shift => match layer {
                Layer::Upper => "abc",
                Layer::Lower | Layer::Symbols => "ABC",
            },
            Self::Symbols => match layer {
                Layer::Symbols => "abc",
                Layer::Lower | Layer::Upper => "?123",
            },
        }
    }

    /// Whether this key changes the layer rather than the field.
    const fn switches_layer(self) -> bool {
        matches!(self, Self::Shift | Self::Symbols)
    }
}

/// The one-character label for a printable ASCII character.
///
/// A table rather than a formatted `String`, so a [`KeyCap`]'s label is
/// `&'static str` and a layout allocates nothing per key. The slice is the
/// atlas's own printable range, indexed by the character's offset into it.
fn printable_label(character: char) -> &'static str {
    /// Every printable ASCII character, each as its own one-byte string.
    const LABELS: [&str; crate::text::ASCII_GLYPH_COUNT] = [
        " ", "!", "\"", "#", "$", "%", "&", "'", "(", ")", "*", "+", ",", "-", ".", "/", "0", "1",
        "2", "3", "4", "5", "6", "7", "8", "9", ":", ";", "<", "=", ">", "?", "@", "A", "B", "C",
        "D", "E", "F", "G", "H", "I", "J", "K", "L", "M", "N", "O", "P", "Q", "R", "S", "T", "U",
        "V", "W", "X", "Y", "Z", "[", "\\", "]", "^", "_", "`", "a", "b", "c", "d", "e", "f", "g",
        "h", "i", "j", "k", "l", "m", "n", "o", "p", "q", "r", "s", "t", "u", "v", "w", "x", "y",
        "z", "{", "|", "}", "~",
    ];
    let index = (character as usize).wrapping_sub(crate::text::FIRST_CHAR as usize);
    // Every character in the tables above is printable ASCII, which
    // `every_key_types_a_character_the_atlas_can_draw` holds them to. A caller
    // that reached here with anything else gets the not-def spelling rather
    // than a panic, for `FontAtlas`'s own reason: a glyph nobody has is a box.
    LABELS.get(index).copied().unwrap_or("?")
}

/// One key's cap and the rectangle it occupies, in screen pixels.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct KeyBox {
    /// What the key does.
    pub cap: KeyCap,
    /// The key's top-left corner.
    pub min: Vec2,
    /// The key's bottom-right corner.
    pub max: Vec2,
}

impl KeyBox {
    /// Whether `point` is inside this key.
    #[must_use]
    pub fn contains(&self, point: Vec2) -> bool {
        point.x >= self.min.x
            && point.x <= self.max.x
            && point.y >= self.min.y
            && point.y <= self.max.y
    }
}

/// Where every key goes, for one frame at one size and one layer.
///
/// [`ConsoleLayout`](super::ConsoleLayout)'s reason for existing, applied to the
/// keyboard: the finger is hit-tested against the rectangles the frame was
/// drawn from, and a layer swapped between the two calls would move them.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct KeyboardLayout {
    keys: Vec<KeyBox>,
    area: (Vec2, Vec2),
    layer: Layer,
}

impl KeyboardLayout {
    /// The keys, in the order they were laid out: the character rows top-first,
    /// then the control row.
    #[must_use]
    pub fn keys(&self) -> &[KeyBox] {
        &self.keys
    }

    /// The whole keyboard's rectangle, which is what a caller tests a press
    /// against to know the keyboard claimed it.
    ///
    /// The **area**, not the union of the keys: a tap in the gap between two
    /// keys is still a tap on the keyboard and must not fall through to
    /// whatever is drawn under it.
    #[must_use]
    pub const fn area(&self) -> (Vec2, Vec2) {
        self.area
    }

    /// The layer these keys were laid out from.
    #[must_use]
    pub const fn layer(&self) -> Layer {
        self.layer
    }

    /// Whether `point` is anywhere on the keyboard.
    #[must_use]
    pub fn contains(&self, point: Vec2) -> bool {
        point.x >= self.area.0.x
            && point.x <= self.area.1.x
            && point.y >= self.area.0.y
            && point.y <= self.area.1.y
    }
}

/// The on-screen keyboard: which layer it is showing, and which key is held.
///
/// Laid out and drawn like everything else in this module — data in, draw list
/// out. It reads no capability, no clock and no keycode: whether it is on
/// screen at all is the engine's call, and `crcbl::debug_console::Console` is
/// where that is made.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct TouchKeyboard {
    layer: Layer,
    /// The index into the last layout's keys that a press is being held on, for
    /// the pressed colour. Cleared when the press ends.
    held: Option<usize>,
}

impl TouchKeyboard {
    /// A keyboard showing its letters, with nothing held.
    #[must_use]
    pub const fn new() -> Self {
        Self {
            layer: Layer::Lower,
            held: None,
        }
    }

    /// The layer it is showing.
    #[must_use]
    pub const fn layer(&self) -> Layer {
        self.layer
    }

    /// Lays the keyboard out along the bottom edge of a `screen`-sized
    /// framebuffer, one [`KEYBOARD_HEIGHT_FRACTION`] tall for a letter layer.
    ///
    /// Rows are laid from the bottom up, so [`Layer::Symbols`]' extra row grows
    /// the keyboard **upwards** and leaves the control row where the thumb left
    /// it. A framebuffer too small for one row lays out no keys rather than
    /// inside-out ones.
    #[must_use]
    pub fn layout(&self, screen: Vec2) -> KeyboardLayout {
        let rows = self.layer.rows();
        let row_count = rows.len() + 1;
        // Sized off a **letter** layer, whatever layer is showing, so the row
        // height — and with it the control row's top edge — is the same on all
        // three. The keyboard grows upwards for the layer that has more rows.
        let row_height =
            (screen.y * KEYBOARD_HEIGHT_FRACTION / (LOWER_ROWS.len() + 1) as f32).floor();
        let height = row_height * row_count as f32;
        let area = (Vec2::new(0.0, (screen.y - height).max(0.0)), screen);
        if row_height <= 0.0 || screen.x <= 0.0 {
            return KeyboardLayout {
                keys: Vec::new(),
                area,
                layer: self.layer,
            };
        }

        // From the bottom edge: the control row is the last one laid out and is
        // the one whose position must not move between layers.
        let bottom = screen.y;
        let mut keys = Vec::new();
        let columns = self.layer.columns();
        let cell = screen.x / columns as f32;
        for (index, row) in rows.iter().enumerate() {
            // `row_count - 1` rows of characters sit above the control row, and
            // this is the `index`-th of them counting down from the top.
            let top = bottom - (row_count - index) as f32 * row_height;
            let taken = row.chars().count();
            let inset = ((columns - taken) as f32 * cell * 0.5).round();
            for (column, character) in row.chars().enumerate() {
                let left = inset + column as f32 * cell;
                keys.push(KeyBox {
                    cap: KeyCap::Type(character),
                    min: Vec2::new(left.round(), top),
                    max: Vec2::new((left + cell).round(), top + row_height),
                });
            }
        }

        let control_top = bottom - row_height;
        let mut left = 0.0f32;
        for (cap, share) in CONTROL_ROW {
            let right = left + screen.x * share;
            keys.push(KeyBox {
                cap,
                min: Vec2::new(left.round(), control_top),
                max: Vec2::new(right.round(), bottom),
            });
            left = right;
        }

        KeyboardLayout {
            keys,
            area,
            layer: self.layer,
        }
    }

    /// Runs one frame of pointer input against `layout`, and reports the key
    /// that was pressed.
    ///
    /// A [`KeyCap::Shift`] or [`KeyCap::Symbols`] press is **swallowed**: it
    /// changes this keyboard's own layer and answers `None`, because the layer
    /// is the keyboard's state and not the field's. Everything else is handed
    /// back for the panel to apply.
    ///
    /// Press capture goes through `ui`, so a press that starts on one key and
    /// is released over its neighbour types nothing — the rule **Send** and
    /// every other clickable widget in this crate follow.
    pub fn point(
        &mut self,
        layout: &KeyboardLayout,
        ui: &mut UiState,
        pointer: PointerInput,
    ) -> Option<KeyCap> {
        let mut typed = None;
        self.held = None;
        for (index, key) in layout.keys().iter().enumerate() {
            let id = KEY_ID_BASE + index as WidgetId;
            let (state, clicked) = ui.interact(
                id,
                key.contains(pointer.pos),
                pointer.down,
                pointer.released,
            );
            if state == ButtonState::Pressed {
                self.held = Some(index);
            }
            if !clicked {
                continue;
            }
            if key.cap.switches_layer() {
                self.layer = match key.cap {
                    KeyCap::Shift => self.layer.shifted(),
                    _ => self.layer.toggled_symbols(),
                };
                // The layout this was hit-tested against is now stale, and so
                // is the index of the key being held in it — the caller draws
                // from a fresh layout and the next frame's press sets it again.
                self.held = None;
                return None;
            }
            typed = Some(key.cap);
        }
        typed
    }

    /// Draws the keyboard: its ground, then every key with its label centred.
    pub fn render(
        &self,
        dl: &mut DrawList,
        layout: &KeyboardLayout,
        atlas: &FontAtlas,
        style: &ConsoleStyle,
    ) {
        if layout.keys().is_empty() {
            return;
        }
        let (area_min, area_max) = layout.area();
        dl.rect(area_min, area_max, style.panel_color);

        let scale = style.text_size / NATURAL_FONT_SIZE;
        let row = style.row_height();
        for (index, key) in layout.keys().iter().enumerate() {
            let held = self.held == Some(index);
            let fill = if held {
                style.button.bg_active
            } else {
                style.button.bg
            };
            dl.rect(key.min, key.max, fill);
            dl.rect_outline(key.min, key.max, style.scale, style.button.border);

            let label = key.cap.label(layout.layer());
            let size = key.max - key.min;
            let text = Vec2::new(
                key.min.x + ((size.x - atlas.text_width(label, scale)) * 0.5).round(),
                key.min.y + ((size.y - row) * 0.5).round(),
            );
            dl.text(text, label, style.button.text, style.text_size);
        }
    }
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use crate::text::{FIRST_CHAR, LAST_CHAR};

    /// The five extents every "it is on screen" test in this repository uses.
    const EXTENTS: [Vec2; 5] = [
        Vec2::new(960.0, 720.0),
        Vec2::new(800.0, 600.0),
        Vec2::new(1920.0, 1080.0),
        Vec2::new(1440.0, 400.0),
        Vec2::new(600.0, 900.0),
    ];

    const LAYERS: [Layer; 3] = [Layer::Lower, Layer::Upper, Layer::Symbols];

    fn press(keyboard: &mut TouchKeyboard, screen: Vec2, at: Vec2) -> Option<KeyCap> {
        let mut ui = UiState::new();
        let layout = keyboard.layout(screen);
        keyboard.point(
            &layout,
            &mut ui,
            PointerInput {
                pos: at,
                down: true,
                released: false,
            },
        );
        keyboard.point(
            &layout,
            &mut ui,
            PointerInput {
                pos: at,
                down: false,
                released: true,
            },
        )
    }

    /// The centre of the first key whose cap is `cap`, on the current layer.
    fn centre_of(keyboard: &TouchKeyboard, screen: Vec2, cap: KeyCap) -> Vec2 {
        let layout = keyboard.layout(screen);
        let key = layout
            .keys()
            .iter()
            .find(|key| key.cap == cap)
            .unwrap_or_else(|| panic!("no {cap:?} key on {:?}", layout.layer()));
        (key.min + key.max) * 0.5
    }

    /// **The claim the whole layer design rests on.** A keyboard that cannot
    /// reach a character is a keyboard a console line can be blocked on, and
    /// which character went missing is exactly what a spot check would not say.
    #[test]
    fn layers_cover_every_printable_character() {
        let mut reachable = std::collections::BTreeSet::new();
        for layer in LAYERS {
            for row in layer.rows() {
                reachable.extend(row.chars());
            }
        }
        for (cap, _) in CONTROL_ROW {
            if let KeyCap::Type(character) = cap {
                reachable.insert(character);
            }
        }
        let missing: Vec<char> = (FIRST_CHAR..=LAST_CHAR)
            .map(char::from)
            .filter(|character| !reachable.contains(character))
            .collect();
        assert!(
            missing.is_empty(),
            "no key types {missing:?}, so a console line needing one cannot be typed",
        );
    }

    /// Every label the keyboard draws has to be drawable by the atlas it is
    /// drawn with, which is the same range the check above walks.
    #[test]
    fn every_key_types_a_character_the_atlas_can_draw() {
        for layer in LAYERS {
            let keyboard = TouchKeyboard { layer, held: None };
            let layout = keyboard.layout(EXTENTS[0]);
            assert!(!layout.keys().is_empty(), "{layer:?} laid out no keys");
            for key in layout.keys() {
                let printable = |character: char| {
                    u8::try_from(character)
                        .is_ok_and(|byte| (FIRST_CHAR..=LAST_CHAR).contains(&byte))
                };
                if let KeyCap::Type(character) = key.cap {
                    assert!(
                        printable(character),
                        "{layer:?} types {character:?}, which the atlas has no glyph for",
                    );
                }
                for label in key.cap.label(layer).chars() {
                    assert!(
                        printable(label),
                        "{:?}'s label carries {label:?}, which the atlas has no glyph for",
                        key.cap,
                    );
                }
            }
        }
    }

    /// The ids are a fixed block below `SEND_ID`, so the block has to be big
    /// enough for the widest layer or two keys — or a key and **Send** — would
    /// share a capture.
    #[test]
    fn every_layer_fits_the_reserved_ids() {
        for layer in LAYERS {
            let keyboard = TouchKeyboard { layer, held: None };
            let count = keyboard.layout(EXTENTS[0]).keys().len();
            assert!(
                (count as WidgetId) <= KEY_ID_SPAN,
                "{layer:?} has {count} keys and only {KEY_ID_SPAN} ids are reserved",
            );
        }
    }

    /// A tap on a letter is that letter, and the panel is what puts it in the
    /// field.
    #[test]
    fn a_tap_on_a_key_types_that_key() {
        for screen in EXTENTS {
            let mut keyboard = TouchKeyboard::new();
            let at = centre_of(&keyboard, screen, KeyCap::Type('q'));
            assert_eq!(
                press(&mut keyboard, screen, at),
                Some(KeyCap::Type('q')),
                "a tap at {at} on a {screen} frame typed nothing",
            );
        }
    }

    /// Shift and the symbols key change the layer and type nothing — the layer
    /// is the keyboard's own state, and a `?123` that reached the field would
    /// put the label in it.
    #[test]
    fn the_layer_keys_switch_the_layer_and_type_nothing() {
        let screen = EXTENTS[0];
        let mut keyboard = TouchKeyboard::new();

        let shift = centre_of(&keyboard, screen, KeyCap::Shift);
        assert_eq!(press(&mut keyboard, screen, shift), None);
        assert_eq!(keyboard.layer(), Layer::Upper);
        let at = centre_of(&keyboard, screen, KeyCap::Type('Q'));
        assert_eq!(press(&mut keyboard, screen, at), Some(KeyCap::Type('Q')));

        let symbols = centre_of(&keyboard, screen, KeyCap::Symbols);
        assert_eq!(press(&mut keyboard, screen, symbols), None);
        assert_eq!(keyboard.layer(), Layer::Symbols);
        let at = centre_of(&keyboard, screen, KeyCap::Type('='));
        assert_eq!(press(&mut keyboard, screen, at), Some(KeyCap::Type('=')));

        // And back out of the symbols layer, which is the same key.
        let symbols = centre_of(&keyboard, screen, KeyCap::Symbols);
        assert_eq!(press(&mut keyboard, screen, symbols), None);
        assert_eq!(keyboard.layer(), Layer::Lower);
    }

    /// A press that starts on one key and is released over another types
    /// neither, which is `UiState`'s capture rule and the reason each key has an
    /// id of its own.
    #[test]
    fn a_press_dragged_off_its_key_types_nothing() {
        let screen = EXTENTS[0];
        let mut keyboard = TouchKeyboard::new();
        let layout = keyboard.layout(screen);
        let start = centre_of(&keyboard, screen, KeyCap::Type('q'));
        let end = centre_of(&keyboard, screen, KeyCap::Type('p'));
        let mut ui = UiState::new();
        keyboard.point(
            &layout,
            &mut ui,
            PointerInput {
                pos: start,
                down: true,
                released: false,
            },
        );
        let typed = keyboard.point(
            &layout,
            &mut ui,
            PointerInput {
                pos: end,
                down: false,
                released: true,
            },
        );
        assert_eq!(typed, None, "a drag from q to p typed {typed:?}");
    }

    /// The control row is in the same place on every layer, which is what makes
    /// the space bar findable without looking after a layer change.
    #[test]
    fn the_control_row_does_not_move_between_layers() {
        let screen = EXTENTS[4];
        let space = |layer| {
            let keyboard = TouchKeyboard { layer, held: None };
            let layout = keyboard.layout(screen);
            let key = layout
                .keys()
                .iter()
                .find(|key| key.cap == KeyCap::Type(' '))
                .expect("every layer draws a space bar");
            (key.min, key.max)
        };
        let first = space(Layer::Lower);
        for layer in LAYERS {
            assert_eq!(space(layer), first, "{layer:?} moved the space bar");
        }
    }

    /// Every key is inside the area the keyboard claims, or a tap the caller
    /// routed away from the game would land on nothing.
    #[test]
    fn every_key_is_inside_the_claimed_area() {
        for screen in EXTENTS {
            for layer in LAYERS {
                let keyboard = TouchKeyboard { layer, held: None };
                let layout = keyboard.layout(screen);
                for key in layout.keys() {
                    assert!(
                        layout.contains(key.min) && layout.contains(key.max),
                        "{:?} at {}..{} escapes the keyboard's area on a {screen} frame",
                        key.cap,
                        key.min,
                        key.max,
                    );
                }
            }
        }
    }

    /// A frame too small for a row draws no keys rather than inside-out ones.
    #[test]
    fn a_frame_with_no_room_lays_out_nothing() {
        let keyboard = TouchKeyboard::new();
        let layout = keyboard.layout(Vec2::new(320.0, 1.0));
        assert!(layout.keys().is_empty());
    }
}
