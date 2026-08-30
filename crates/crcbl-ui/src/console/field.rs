//! The crate's first editable field: content, a caret, and the edits the
//! cursor keys make.
//!
//! `docs/plan/52-debug-console.md` decision 6. The console is its first
//! consumer, so nothing here mentions one: a [`TextField`] holds a line and a
//! caret, and the keys that drive it are named for what they do to the text
//! rather than for the keycodes that will call them.

use glam::Vec2;

use crate::draw_list::DrawList;
use crate::text::{FontAtlas, LINE_HEIGHT};
use crate::widget::NATURAL_FONT_SIZE;

/// Everything a [`TextField`] needs to draw itself, in **pixels**.
///
/// A style of its own rather than the console's: the field is a general widget
/// and a screen that grows a name box gets it without taking the console's
/// palette with it. `ConsoleStyle::field_style` builds the console's own.
///
/// [`ConsoleStyle::field_style`]: super::ConsoleStyle::field_style
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct TextFieldStyle {
    /// The font size the content is drawn at.
    pub size: f32,
    /// The content's colour.
    pub text_color: [f32; 4],
    /// The caret's colour.
    pub caret_color: [f32; 4],
    /// How wide the caret bar is drawn.
    pub caret_width: f32,
}

/// One line of editable text and a caret in it.
///
/// The caret is a position **between characters**, counted in characters and
/// never in bytes: every edit here is expressed as "the character before the
/// caret" or "the character after it", and a byte index would name the middle
/// of a multi-byte one. Byte offsets exist only where the `String` is actually
/// cut, and are derived from the caret rather than stored.
///
/// No selection — decision 6 says so, and the follow-up that adds one is the
/// clipboard slice.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct TextField {
    text: String,
    /// How many characters are to the left of the caret.
    caret: usize,
}

impl TextField {
    /// An empty field with the caret at the start.
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// The line being edited.
    #[must_use]
    pub fn text(&self) -> &str {
        &self.text
    }

    /// How many characters are to the left of the caret.
    #[must_use]
    pub const fn caret(&self) -> usize {
        self.caret
    }

    /// How many characters the line holds.
    #[must_use]
    pub fn len(&self) -> usize {
        self.text.chars().count()
    }

    /// Whether the line is empty.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.text.is_empty()
    }

    /// Replaces the line and puts the caret at its end.
    ///
    /// What a history recall and a completion fill both do: the caret goes
    /// where the typing would continue, which is after the text that arrived.
    /// Control characters are dropped, as they are for [`insert`](Self::insert).
    pub fn set_text(&mut self, text: &str) {
        self.text.clear();
        self.caret = 0;
        self.insert(text);
    }

    /// Empties the line and puts the caret back at the start.
    pub fn clear(&mut self) {
        self.text.clear();
        self.caret = 0;
    }

    /// Inserts text at the caret and leaves the caret after it.
    ///
    /// **Control characters are dropped**, one by one rather than by refusing
    /// the whole string. A `TextCommit` carries whatever the platform's layout
    /// produced, and a `Return` or a `Tab` arrives on some of them as a
    /// character as well as a key — a single-line field that took them would
    /// hold a newline no caret arithmetic here can place.
    pub fn insert(&mut self, text: &str) {
        for c in text.chars().filter(|c| !c.is_control()) {
            let at = self.byte_at(self.caret);
            self.text.insert(at, c);
            self.caret += 1;
        }
    }

    /// Deletes the character before the caret. Reports whether one was there.
    pub fn backspace(&mut self) -> bool {
        if self.caret == 0 {
            return false;
        }
        self.caret -= 1;
        let at = self.byte_at(self.caret);
        self.text.remove(at);
        true
    }

    /// Deletes the character after the caret. Reports whether one was there.
    pub fn delete(&mut self) -> bool {
        let at = self.byte_at(self.caret);
        if at == self.text.len() {
            return false;
        }
        self.text.remove(at);
        true
    }

    /// Moves the caret one character left. Reports whether it moved.
    pub fn move_left(&mut self) -> bool {
        if self.caret == 0 {
            return false;
        }
        self.caret -= 1;
        true
    }

    /// Moves the caret one character right. Reports whether it moved.
    pub fn move_right(&mut self) -> bool {
        if self.caret >= self.len() {
            return false;
        }
        self.caret += 1;
        true
    }

    /// Moves the caret to the start of the line. Reports whether it moved.
    pub fn move_home(&mut self) -> bool {
        let moved = self.caret != 0;
        self.caret = 0;
        moved
    }

    /// Moves the caret to the end of the line. Reports whether it moved.
    pub fn move_end(&mut self) -> bool {
        let end = self.len();
        let moved = self.caret != end;
        self.caret = end;
        moved
    }

    /// The part of the line that fits in `columns`, and where the caret is in
    /// it.
    ///
    /// **The field has no horizontal scroll of its own and needs none.** The
    /// window is derived from the caret every time it is asked for: the caret is
    /// always in the last column it could be in, so typing past the right-hand
    /// edge walks the text left under a caret that stays put — a terminal's
    /// behaviour, and one that cannot drift out of step with the text the way a
    /// stored offset can.
    ///
    /// [`DrawList`] has no clip rectangle, so a caller that drew the whole line
    /// would draw it over whatever is beside the field.
    #[must_use]
    pub fn window(&self, columns: usize) -> (&str, usize) {
        if columns == 0 {
            return ("", 0);
        }
        let start = self.caret.saturating_sub(columns - 1);
        let end = start.saturating_add(columns).min(self.len());
        (
            &self.text[self.byte_at(start)..self.byte_at(end)],
            self.caret - start,
        )
    }

    /// The rectangle the caret is drawn as, for a field anchored at `pos` and
    /// `columns` wide.
    ///
    /// `pos` is the top-left of the content's em box, the anchor
    /// [`DrawList::text`] takes, and the rectangle is placed inside the same
    /// [`window`](Self::window) [`render`](Self::render) draws.
    ///
    /// # Why the caret is measured and not read off a glyph
    ///
    /// [`FontAtlas::layout_line`] returns a rectangle per *drawn* glyph and
    /// drops the ones with no ink, so its `n`-th entry is not the `n`-th
    /// character of a line holding a space — and a caret placed from it would
    /// slide left by one column for every space to its left. Measuring the text
    /// before the caret walks the same advances `layout_line` walks, spaces
    /// included, so the two agree on where column `n` is.
    #[must_use]
    pub fn caret_rect(
        &self,
        pos: Vec2,
        atlas: &FontAtlas,
        style: &TextFieldStyle,
        columns: usize,
    ) -> (Vec2, Vec2) {
        let scale = style.size / NATURAL_FONT_SIZE;
        let (visible, caret) = self.window(columns);
        let min = Vec2::new(
            pos.x + atlas.text_width(&visible[..byte_of(visible, caret)], scale),
            pos.y,
        );
        let max = min + Vec2::new(style.caret_width, LINE_HEIGHT * scale);
        (min, max)
    }

    /// Draws the `columns` of the line that fit, and the caret when
    /// `caret_visible`.
    ///
    /// `pos` is the top-left of the content's em box. An empty window emits no
    /// text command at all — a zero-glyph run is a command the UI pass expands
    /// into nothing — so a caret in an empty field is the only thing drawn.
    pub fn render(
        &self,
        dl: &mut DrawList,
        pos: Vec2,
        atlas: &FontAtlas,
        style: &TextFieldStyle,
        columns: usize,
        caret_visible: bool,
    ) {
        let (visible, _) = self.window(columns);
        if !visible.is_empty() {
            dl.text(pos, visible, style.text_color, style.size);
        }
        if caret_visible {
            let (min, max) = self.caret_rect(pos, atlas, style, columns);
            dl.rect(min, max, style.caret_color);
        }
    }

    /// The byte offset of character `caret`, or the line's length past its end.
    fn byte_at(&self, caret: usize) -> usize {
        byte_of(&self.text, caret)
    }
}

/// The byte offset of character `count` in `text`, or its length past the end.
fn byte_of(text: &str, count: usize) -> usize {
    text.char_indices()
        .nth(count)
        .map_or(text.len(), |(at, _)| at)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::draw_list::DrawCommand;

    fn atlas() -> FontAtlas {
        FontAtlas::built_in()
    }

    fn style() -> TextFieldStyle {
        TextFieldStyle {
            size: NATURAL_FONT_SIZE,
            text_color: [1.0, 1.0, 1.0, 1.0],
            caret_color: [1.0, 0.5, 0.0, 1.0],
            caret_width: 2.0,
        }
    }

    /// A field with `text` typed into it and the caret left at the end.
    fn typed(text: &str) -> TextField {
        let mut field = TextField::new();
        field.insert(text);
        field
    }

    /// **Typing inserts at the caret**, which is the whole of the widget's job,
    /// and the caret follows what was typed rather than staying put.
    #[test]
    fn typing_inserts_at_the_caret() {
        let mut field = typed("antialiasing");
        assert_eq!(field.text(), "antialiasing");
        assert_eq!(field.caret(), 12);

        field.move_home();
        field.insert("r_");
        assert_eq!(field.text(), "r_antialiasing");
        assert_eq!(field.caret(), 2, "the caret did not follow the insert");
    }

    /// **The caret is counted in characters, not bytes**, so an edit after a
    /// multi-byte character cuts the string where the caret says and not in the
    /// middle of a codepoint. A byte-indexed caret panics on this input.
    #[test]
    fn a_multi_byte_character_does_not_split_under_the_caret() {
        let mut field = typed("é");
        assert_eq!(field.len(), 1, "one character, two bytes");
        field.insert("x");
        assert_eq!(field.text(), "éx");
        assert!(field.move_left());
        field.insert("y");
        assert_eq!(field.text(), "éyx");
        assert!(field.backspace());
        assert_eq!(field.text(), "éx");
        assert!(field.move_home());
        assert!(field.delete(), "the multi-byte character was not deleted");
        assert_eq!(field.text(), "x");
    }

    /// **Backspace and delete take the characters on either side of the caret**
    /// and report nothing to take at the ends, which is what stops a caret
    /// walking off the line.
    #[test]
    fn backspace_and_delete_take_the_character_on_their_own_side() {
        let mut field = typed("abc");
        assert!(field.move_left());
        assert!(field.backspace());
        assert_eq!((field.text(), field.caret()), ("ac", 1));
        assert!(field.delete());
        assert_eq!((field.text(), field.caret()), ("a", 1));
        assert!(!field.delete(), "deleted past the end of the line");
        assert!(field.backspace());
        assert!(!field.backspace(), "backspaced past the start of the line");
        assert!(field.is_empty());
    }

    /// The four cursor motions, and the ends they refuse to walk past.
    #[test]
    fn the_cursor_keys_walk_the_line_and_stop_at_its_ends() {
        let mut field = typed("help");
        assert!(!field.move_right(), "moved right from the end");
        assert!(field.move_left());
        assert_eq!(field.caret(), 3);
        assert!(field.move_home());
        assert_eq!(field.caret(), 0);
        assert!(!field.move_left(), "moved left from the start");
        assert!(!field.move_home(), "reported a move it did not make");
        assert!(field.move_end());
        assert_eq!(field.caret(), 4);
        assert!(!field.move_end(), "reported a move it did not make");
    }

    /// **A control character never enters the line.** `Return` and `Tab` arrive
    /// as text as well as as keys on some platforms, and a newline in a
    /// single-line field is a caret position no arithmetic here can draw.
    #[test]
    fn control_characters_are_dropped_and_the_rest_of_the_text_is_kept() {
        let mut field = TextField::new();
        field.insert("a\nb\tc\r");
        assert_eq!(field.text(), "abc");
        assert_eq!(field.caret(), 3, "the dropped characters moved the caret");

        field.set_text("save\n");
        assert_eq!(field.text(), "save");
        assert_eq!(field.caret(), 4, "a recall left the caret before the end");
    }

    /// **The caret sits at the column the caret index names**, spaces included.
    ///
    /// The failure this catches is specific: [`FontAtlas::layout_line`] drops
    /// the zero-ink glyphs, so a caret placed from its `n`-th rectangle drifts
    /// one column left for every space before it. Here the same line is
    /// measured with and without a space and the caret has to move by exactly
    /// one advance.
    #[test]
    fn the_caret_lands_on_the_column_it_names_across_a_space() {
        let atlas = atlas();
        let style = style();
        let advance = atlas.text_width("M", 1.0);
        let origin = Vec2::new(40.0, 20.0);

        let spaced = typed("a b");
        let tight = typed("ab");
        let (spaced_min, spaced_max) = spaced.caret_rect(origin, &atlas, &style, usize::MAX);
        let (tight_min, _) = tight.caret_rect(origin, &atlas, &style, usize::MAX);
        assert!(
            (spaced_min.x - tight_min.x - advance).abs() < 1e-3,
            "the space cost the caret {} pixels, not one {advance}-pixel advance",
            spaced_min.x - tight_min.x,
        );
        assert!((spaced_min.y - origin.y).abs() < 1e-3);
        assert!(
            (spaced_max.x - spaced_min.x - style.caret_width).abs() < 1e-3,
            "the caret is not the width the style asked for",
        );
        assert!(
            spaced_max.y > spaced_min.y,
            "the caret has no height to draw",
        );

        // And an empty field puts the caret exactly on the anchor.
        let (empty_min, _) = TextField::new().caret_rect(origin, &atlas, &style, usize::MAX);
        assert_eq!(empty_min, origin);
    }

    /// **A line longer than the box scrolls under a caret that stays put**, and
    /// what is drawn is never wider than the columns it was given — the field
    /// is drawn into a [`DrawList`] that has no clip rectangle, so text it drew
    /// past its own box would land on whatever is beside it.
    #[test]
    fn a_line_longer_than_the_box_scrolls_under_the_caret() {
        let atlas = atlas();
        let style = style();
        let columns = 8;
        let mut field = typed("abcdefghijkl");

        let (visible, caret) = field.window(columns);
        assert_eq!(
            visible, "fghijkl",
            "the tail of the line is not what is shown"
        );
        assert_eq!(caret, visible.chars().count(), "the caret left its column");

        // The caret walking left slides the window with it, one column at a
        // time, until the head of the line is back on screen.
        assert!(field.move_left());
        assert_eq!(field.window(columns), ("efghijkl", 7));
        assert!(field.move_left());
        assert_eq!(field.window(columns), ("defghijk", 7));
        assert!(field.move_home());
        assert_eq!(field.window(columns), ("abcdefgh", 0));

        // Whatever the caret is doing, neither the text nor the caret is drawn
        // wider than the box.
        let width = columns as f32 * atlas.text_width("M", 1.0);
        for caret in 0..=field.len() {
            field.move_home();
            for _ in 0..caret {
                field.move_right();
            }
            let mut dl = DrawList::new();
            field.render(&mut dl, Vec2::ZERO, &atlas, &style, columns, true);
            for command in dl.commands() {
                let right = match command {
                    DrawCommand::Text { pos, text, .. } => pos.x + atlas.text_width(text, 1.0),
                    DrawCommand::Rect { max, .. } => max.x,
                    other => panic!("a field drew {other:?}"),
                };
                assert!(
                    right <= width + 1e-3,
                    "with the caret at {caret} something reaches {right}, past the \
                     {width}-pixel box",
                );
            }
        }

        // A box with no columns in it draws nothing at all.
        let mut none = DrawList::new();
        field.render(&mut none, Vec2::ZERO, &atlas, &style, 0, true);
        assert_eq!(none.len(), 1, "no room, and yet text was drawn");
    }

    /// The field draws its text and, only while it is shown, its caret.
    #[test]
    fn render_draws_the_line_and_the_caret_when_it_is_shown() {
        let atlas = atlas();
        let style = style();
        let field = typed("echo hi");

        let mut shown = DrawList::new();
        field.render(&mut shown, Vec2::ZERO, &atlas, &style, usize::MAX, true);
        let texts: Vec<&str> = shown
            .commands()
            .iter()
            .filter_map(|command| match command {
                DrawCommand::Text { text, .. } => Some(text.as_str()),
                _ => None,
            })
            .collect();
        assert_eq!(texts, ["echo hi"]);
        assert_eq!(
            shown
                .commands()
                .iter()
                .filter(|command| matches!(command, DrawCommand::Rect { .. }))
                .count(),
            1,
            "the caret was not drawn while it was shown",
        );

        let mut hidden = DrawList::new();
        field.render(&mut hidden, Vec2::ZERO, &atlas, &style, usize::MAX, false);
        assert_eq!(hidden.len(), 1, "the blink left the caret on screen");

        // An empty field is a caret and nothing else — a text command with no
        // glyphs in it is work the UI pass expands into nothing.
        let mut empty = DrawList::new();
        TextField::new().render(&mut empty, Vec2::ZERO, &atlas, &style, usize::MAX, true);
        assert_eq!(empty.len(), 1);
        assert!(matches!(empty.commands()[0], DrawCommand::Rect { .. }));
    }
}
