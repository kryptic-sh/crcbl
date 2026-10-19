//! One line of editable text: its content, a caret, a selection anchor, and
//! every edit the keys make.
//!
//! The model every editable field in the crate is built on —
//! [`Ui::text_input`](crate::tree::Ui::text_input), which the console's prompt
//! is one of — so there is one copy of the caret arithmetic. It names no
//! keycode and draws nothing: a
//! [`LineEdit`] takes [`Edit`]s, and [`Edit::for_key`] is the one place a key
//! becomes one.
//!
//! # Positions are between clusters, counted in `char`s
//!
//! The caret and the anchor are counted in **`char`s**, never bytes, and
//! stand only **between clusters** — [`crate::font::cluster`]'s, UAX #29's
//! extended grapheme clusters, which keep a base and its marks, an emoji ZWJ
//! sequence, a skin-tone modifier, a keycap and a flag's regional-indicator
//! pair together. So an `e` followed by U+0301 COMBINING ACUTE ACCENT is one
//! caret stop, and an emoji built with zero-width joiners takes one press. A
//! position that lands inside a cluster — a click, a word move, an outside
//! edit, text typed in front of a mark — moves to the cluster's edge: back to
//! its start, except that a move or a selection reaching rightwards, and the
//! caret after an insert, go on to its end. Nothing is ever cut inside a
//! scalar: every byte offset is derived from a `char` count at the moment the
//! `String` is cut.
//!
//! **Backspace and Delete take a whole cluster**, as macOS, GTK and the
//! browsers do for emoji and Latin marks: the `e` and its accent go
//! together. Some platforms let Backspace take a cluster's last code point
//! alone in scripts that type marks one key each — Indic vowel signs, Hebrew
//! points — and this model does not; deleting by cluster everywhere is the
//! one rule that never leaves half a flag or half a ZWJ sequence behind.
//!
//! # Selection
//!
//! The selection is the text between the anchor and the caret, in whichever
//! order they lie; with the two equal there is none. A move that does not
//! select collapses a selection first: Left and Right land on its near edge
//! and stop there, a word move starts from that edge, and Home and End go to
//! the ends of the line wherever the selection was. A move that selects moves
//! the caret and leaves the anchor where it is. Inserting replaces the
//! selection, and Backspace and Delete delete it instead of a character.
//!
//! # Words
//!
//! Words are UAX #29's word boundaries, merged into runs of one class: a run
//! of words with a letter, a digit or `_` in them, or a run of anything else
//! that is not whitespace. So `can't`, `3.14` and `foo_bar` are one word
//! each, a mark never ends its word, and `--` is one run. Whitespace
//! separates words and a word move passes over it. A move left ends at the
//! start of the word before the caret and a move right at the end of the word
//! after it — the macOS `Option`+arrow and GTK `Ctrl`+arrow convention,
//! rather than Windows', where a move right ends at the start of the next
//! word.
//!
//! # Newlines
//!
//! Control characters never enter the line, one by one rather than by refusing
//! the whole string: a `TextCommit` carries whatever the platform's layout
//! produced, a `Return` or a `Tab` arrives on some platforms as a character as
//! well as a key, and a paste may carry line breaks. HTML's value sanitization
//! algorithm for `<input type=text>` strips line breaks the same way.

#[cfg(test)]
mod tests;

use crcbl_core::input::{KeyCode, Modifiers};
use unicode_segmentation::UnicodeSegmentation;

use crate::font::cluster;

/// Where a move takes the caret.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Motion {
    /// One cluster left.
    Left,
    /// One cluster right.
    Right,
    /// To the start of the word before the caret.
    WordLeft,
    /// To the end of the word after the caret.
    WordRight,
    /// To the start of the line.
    Home,
    /// To the end of the line.
    End,
}

/// One edit to a [`LineEdit`], in the order it arrived.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Edit {
    /// Text typed or committed by an input method, replacing the selection.
    Insert(String),
    /// A caret move; with `select` the anchor stays and the selection grows or
    /// shrinks.
    Move {
        /// Where the caret goes.
        motion: Motion,
        /// Whether the move selects.
        select: bool,
    },
    /// Deletes the selection, or else the cluster before the caret.
    Backspace,
    /// Deletes the selection, or else the cluster after the caret.
    Delete,
    /// Selects the whole line, the caret at its end.
    SelectAll,
    /// Puts the selection on the clipboard.
    Copy,
    /// Puts the selection on the clipboard and deletes it.
    Cut,
    /// Asks for the clipboard's text, to replace the selection when it comes.
    Paste,
}

impl Edit {
    /// The modifiers a clipboard or select-all shortcut is read under: `Ctrl`,
    /// or `Super` for the `Command` key a Mac keyboard sends. The engine keeps
    /// no per-platform shortcut table, so both are accepted everywhere, as
    /// the console's paste key accepts them.
    pub const SHORTCUT: Modifiers = Modifiers::CTRL.union(Modifiers::SUPER);

    /// The modifiers a word move is read under: `Ctrl`, or `Alt` for the
    /// `Option` key a Mac keyboard sends.
    pub const WORD: Modifiers = Modifiers::CTRL.union(Modifiers::ALT);

    /// The edit `key` makes when pressed with `modifiers` held, or `None` for
    /// a key that edits nothing.
    ///
    /// The arrows move by a character, by a word with [`Self::WORD`], and to
    /// the line's end with `Super` (a Mac's `Command`+arrow); Home and End go
    /// to the ends; `Shift` makes any move select. With [`Self::SHORTCUT`] and
    /// no `Alt`, `A` selects all, `C` copies, `X` cuts and `V` pastes — `Alt`
    /// excluded because Windows reports `AltGr`, which types characters on
    /// many layouts, as `Ctrl`+`Alt`. Every character a key types arrives as
    /// a `TextCommit` instead, with the layout applied, so no letter is an
    /// edit here.
    #[must_use]
    pub fn for_key(key: KeyCode, modifiers: Modifiers) -> Option<Self> {
        let select = modifiers.contains(Modifiers::SHIFT);
        let arrow = |by_char, by_word, to_end| {
            let motion = if modifiers.contains(Modifiers::SUPER) {
                to_end
            } else if modifiers.intersects(Self::WORD) {
                by_word
            } else {
                by_char
            };
            Some(Self::Move { motion, select })
        };
        let shortcut = modifiers.intersects(Self::SHORTCUT) && !modifiers.contains(Modifiers::ALT);
        match key {
            KeyCode::ArrowLeft => arrow(Motion::Left, Motion::WordLeft, Motion::Home),
            KeyCode::ArrowRight => arrow(Motion::Right, Motion::WordRight, Motion::End),
            KeyCode::Home => Some(Self::Move {
                motion: Motion::Home,
                select,
            }),
            KeyCode::End => Some(Self::Move {
                motion: Motion::End,
                select,
            }),
            KeyCode::Backspace => Some(Self::Backspace),
            KeyCode::Delete => Some(Self::Delete),
            KeyCode::KeyA if shortcut => Some(Self::SelectAll),
            KeyCode::KeyC if shortcut => Some(Self::Copy),
            KeyCode::KeyX if shortcut => Some(Self::Cut),
            KeyCode::KeyV if shortcut => Some(Self::Paste),
            _ => None,
        }
    }
}

/// What an edit asks of the clipboard.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum ClipboardOp {
    /// Put this text on it.
    Offer(String),
    /// Read its text, for [`LineEdit::insert`].
    Read,
}

/// What one [`LineEdit::apply`] did.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Applied {
    /// The line's text changed.
    pub text: bool,
    /// The caret or the anchor moved.
    pub caret: bool,
    /// What the edit asks of the clipboard.
    pub clipboard: Option<ClipboardOp>,
}

/// One line of text, a caret and a selection anchor. See the module docs.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct LineEdit {
    text: String,
    /// How many characters are to the left of the caret.
    caret: usize,
    /// How many characters are to the left of the selection's other end.
    anchor: usize,
}

impl LineEdit {
    /// An empty line with the caret at the start.
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// The line.
    #[must_use]
    pub fn text(&self) -> &str {
        &self.text
    }

    /// How many characters are to the left of the caret.
    #[must_use]
    pub const fn caret(&self) -> usize {
        self.caret
    }

    /// How many characters are to the left of the selection's anchor: the
    /// caret itself when nothing is selected.
    #[must_use]
    pub const fn anchor(&self) -> usize {
        self.anchor
    }

    /// The selected characters as a range of `char` positions, empty when
    /// nothing is selected.
    #[must_use]
    pub fn selection(&self) -> core::ops::Range<usize> {
        self.caret.min(self.anchor)..self.caret.max(self.anchor)
    }

    /// The selected text.
    #[must_use]
    pub fn selected_text(&self) -> &str {
        let range = self.selection();
        &self.text[byte_of(&self.text, range.start)..byte_of(&self.text, range.end)]
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

    /// Replaces the line and puts the caret at its end, nothing selected.
    /// Control characters are dropped, as they are for
    /// [`insert`](Self::insert).
    pub fn set_text(&mut self, text: &str) {
        self.clear();
        self.insert(text);
    }

    /// Takes `text` as the line when it is not already the line — an edit
    /// made from outside — keeping the caret and the anchor where they were,
    /// held inside the new length and moved back to the start of a cluster
    /// they would land inside. Returns whether the line changed.
    pub fn sync(&mut self, text: &str) -> bool {
        if self.text == text {
            return false;
        }
        text.clone_into(&mut self.text);
        let stops = self.stops();
        self.caret = snap_back(&stops, self.caret);
        self.anchor = snap_back(&stops, self.anchor);
        true
    }

    /// Empties the line and puts the caret back at the start.
    pub fn clear(&mut self) {
        self.text.clear();
        self.caret = 0;
        self.anchor = 0;
    }

    /// Puts the caret at `at` — held inside the line, and back at the start
    /// of a cluster it would land inside — and the anchor with it unless
    /// `select`. Returns whether either moved.
    pub fn place(&mut self, at: usize, select: bool) -> bool {
        let at = snap_back(&self.stops(), at);
        let before = (self.caret, self.anchor);
        self.caret = at;
        if !select {
            self.anchor = at;
        }
        before != (self.caret, self.anchor)
    }

    /// Selects `range` of `char` positions, held inside the line and grown
    /// to the clusters it reaches into, with the caret at its end. Returns
    /// whether the caret or the anchor moved.
    pub fn select(&mut self, range: core::ops::Range<usize>) -> bool {
        let stops = self.stops();
        let before = (self.caret, self.anchor);
        self.anchor = snap_back(&stops, range.start);
        self.caret = snap_on(&stops, range.end);
        before != (self.caret, self.anchor)
    }

    /// Inserts `text` over the selection and leaves the caret after it — and
    /// after the rest of a cluster it joined — with control characters
    /// dropped. Returns whether the line changed — an insert with nothing left
    /// to insert leaves a selection alone.
    pub fn insert(&mut self, text: &str) -> bool {
        let mut kept = text.chars().filter(|c| !c.is_control()).peekable();
        if kept.peek().is_none() {
            return false;
        }
        self.delete_selection();
        let mut at = byte_of(&self.text, self.caret);
        for c in kept {
            self.text.insert(at, c);
            at += c.len_utf8();
            self.caret += 1;
        }
        self.caret = snap_on(&self.stops(), self.caret);
        self.anchor = self.caret;
        true
    }

    /// Deletes the selection, or else the cluster before the caret. Returns
    /// whether anything was deleted.
    pub fn backspace(&mut self) -> bool {
        if self.delete_selection() {
            return true;
        }
        if self.caret == 0 {
            return false;
        }
        let start = previous_stop(&self.stops(), self.caret);
        self.remove(start..self.caret);
        self.caret = start;
        self.anchor = start;
        true
    }

    /// Deletes the selection, or else the cluster after the caret. Returns
    /// whether anything was deleted.
    pub fn delete(&mut self) -> bool {
        if self.delete_selection() {
            return true;
        }
        if self.caret == self.len() {
            return false;
        }
        let end = next_stop(&self.stops(), self.caret);
        self.remove(self.caret..end);
        true
    }

    /// Moves the caret as `motion` says; see the module docs for what a move
    /// does to a selection. Returns whether the caret or the anchor moved.
    pub fn move_caret(&mut self, motion: Motion, select: bool) -> bool {
        let before = (self.caret, self.anchor);
        let range = self.selection();
        let collapsing = !select && !range.is_empty();
        let from = match motion {
            Motion::Left | Motion::WordLeft if collapsing => range.start,
            Motion::Right | Motion::WordRight if collapsing => range.end,
            _ => self.caret,
        };
        let chars: Vec<char> = self.text.chars().collect();
        let stops = self.stops();
        let to = match motion {
            Motion::Left if collapsing => from,
            Motion::Right if collapsing => from,
            Motion::Left => previous_stop(&stops, from),
            Motion::Right => next_stop(&stops, from),
            Motion::WordLeft => snap_back(&stops, word_left(&chars, from)),
            Motion::WordRight => snap_on(&stops, word_right(&chars, from)),
            Motion::Home => 0,
            Motion::End => chars.len(),
        };
        self.caret = to;
        if !select {
            self.anchor = to;
        }
        before != (self.caret, self.anchor)
    }

    /// Applies one edit. [`Edit::Copy`] and [`Edit::Cut`] with nothing
    /// selected do nothing and offer nothing; [`Edit::Paste`] changes nothing
    /// and asks for a read.
    pub fn apply(&mut self, edit: &Edit) -> Applied {
        let before = (self.caret, self.anchor);
        let mut applied = Applied::default();
        match edit {
            Edit::Insert(text) => applied.text = self.insert(text),
            Edit::Move { motion, select } => {
                self.move_caret(*motion, *select);
            }
            Edit::Backspace => applied.text = self.backspace(),
            Edit::Delete => applied.text = self.delete(),
            Edit::SelectAll => {
                self.select(0..self.len());
            }
            Edit::Copy | Edit::Cut => {
                if !self.selection().is_empty() {
                    applied.clipboard = Some(ClipboardOp::Offer(self.selected_text().to_owned()));
                    if *edit == Edit::Cut {
                        applied.text = self.delete_selection();
                    }
                }
            }
            Edit::Paste => applied.clipboard = Some(ClipboardOp::Read),
        }
        applied.caret = before != (self.caret, self.anchor);
        applied
    }

    /// Deletes the selected text and collapses the caret where it began.
    /// Returns whether anything was selected.
    fn delete_selection(&mut self) -> bool {
        let range = self.selection();
        if range.is_empty() {
            return false;
        }
        self.remove(range.clone());
        self.caret = range.start;
        self.anchor = range.start;
        true
    }

    /// Removes the chars in `range`, leaving the caret and the anchor alone.
    fn remove(&mut self, range: core::ops::Range<usize>) {
        let start = byte_of(&self.text, range.start);
        let end = byte_of(&self.text, range.end);
        self.text.replace_range(start..end, "");
    }

    /// Where the caret may stand: [`cluster::caret_stops`] of the line.
    fn stops(&self) -> Vec<usize> {
        cluster::caret_stops(&self.text)
    }
}

/// The last of `stops` at or before `at`: `at` held inside the line and moved
/// back to the start of a cluster it lands inside.
fn snap_back(stops: &[usize], at: usize) -> usize {
    stops[stops.partition_point(|&stop| stop <= at) - 1]
}

/// The first of `stops` at or after `at`, or the line's end past it: `at`
/// moved on to the end of a cluster it lands inside.
fn snap_on(stops: &[usize], at: usize) -> usize {
    let end = stops.len() - 1;
    stops[stops.partition_point(|&stop| stop < at).min(end)]
}

/// The last of `stops` before `at`, or the line's start.
fn previous_stop(stops: &[usize], at: usize) -> usize {
    stops[stops.partition_point(|&stop| stop < at).saturating_sub(1)]
}

/// The first of `stops` after `at`, or the line's end.
fn next_stop(stops: &[usize], at: usize) -> usize {
    let end = stops.len() - 1;
    stops[stops.partition_point(|&stop| stop <= at).min(end)]
}

/// The byte offset of character `count` in `text`, or its length past the end.
#[must_use]
pub fn byte_of(text: &str, count: usize) -> usize {
    text.char_indices()
        .nth(count)
        .map_or(text.len(), |(at, _)| at)
}

/// The class a character's word is made of; see the module docs.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Class {
    Space,
    Word,
    Other,
}

/// The class of each of `chars`, from the UAX #29 word it is in: a word with
/// a letter, a digit or `_` in it, whitespace, or anything else.
fn classes(chars: &[char]) -> Vec<Class> {
    let text: String = chars.iter().collect();
    let mut classes = Vec::with_capacity(chars.len());
    for word in text.split_word_bounds() {
        let class = if word.chars().any(|c| c.is_alphanumeric() || c == '_') {
            Class::Word
        } else if word.chars().all(char::is_whitespace) {
            Class::Space
        } else {
            Class::Other
        };
        classes.extend(core::iter::repeat_n(class, word.chars().count()));
    }
    classes
}

/// The start of the word before position `from` in `chars`.
#[must_use]
pub fn word_left(chars: &[char], from: usize) -> usize {
    let classes = classes(chars);
    let mut at = from.min(chars.len());
    while at > 0 && classes[at - 1] == Class::Space {
        at -= 1;
    }
    if let Some(&run) = at.checked_sub(1).and_then(|before| classes.get(before)) {
        while at > 0 && classes[at - 1] == run {
            at -= 1;
        }
    }
    at
}

/// The end of the word after position `from` in `chars`.
#[must_use]
pub fn word_right(chars: &[char], from: usize) -> usize {
    let classes = classes(chars);
    let mut at = from.min(chars.len());
    while at < chars.len() && classes[at] == Class::Space {
        at += 1;
    }
    if let Some(&run) = classes.get(at) {
        while at < chars.len() && classes[at] == run {
            at += 1;
        }
    }
    at
}

/// The run of one class — a word, the punctuation between two words, or the
/// whitespace between them — holding character `index` of `chars`: what a
/// double-click selects. An index past the end is the last character; an
/// empty line has only the empty range.
#[must_use]
pub fn run_at(chars: &[char], index: usize) -> core::ops::Range<usize> {
    let Some(last) = chars.len().checked_sub(1) else {
        return 0..0;
    };
    let classes = classes(chars);
    let index = index.min(last);
    let run = classes[index];
    let mut start = index;
    while start > 0 && classes[start - 1] == run {
        start -= 1;
    }
    let mut end = index + 1;
    while end < chars.len() && classes[end] == run {
        end += 1;
    }
    start..end
}
