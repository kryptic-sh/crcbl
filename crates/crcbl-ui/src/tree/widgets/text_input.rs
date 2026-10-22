//! A single-line text input: UI rung 7's text input
//! with selection and clipboard, on [`crate::edit`]'s model.
//!
//! # Engaged, under the LOCKED rule
//!
//! Focus passes over a text input like any engage widget. Accept or a click
//! engages it; while engaged it takes the frame's [`TextInput`] — typed text
//! and editing keys — and every navigation step it captures does nothing, so
//! the arrows the keyboard sends as [`Edit`]s move the caret and never focus.
//! Accept, or a click outside it, commits; back cancels to the text it had
//! when it engaged. A drag inside it selects and, unlike a slider's, **ends
//! engaged**: selecting text is what a person does before typing over it or
//! copying it, and both need the input engaged.
//!
//! **Edits are taken only by an input engaged since an earlier frame.** The
//! press that engages — Space, which is `ui_accept`, or a click — is in the
//! same frame as whatever text that press committed, and a field that took it
//! would open with a space typed into it.
//!
//! # Where the caret starts
//!
//! **An engagement no pointer began selects the whole text**, so what is typed
//! first replaces it: accept on a focused input, and [`Ui::engage`] — which the
//! editor's rename begins with — as a browser's Tab into a field and a file
//! dialog's name box do. **A click places the caret where it lands instead**,
//! as on every platform, and a double-click selects the word under it, as
//! Windows, macOS and GTK do. An input built with
//! [`TextInputOptions::keep_caret`] keeps its caret through every engagement: a
//! prompt engaged again after each click elsewhere, whose half-typed line the
//! next key must not replace, or a line offering text to type after.
//!
//! # The frame's text input
//!
//! [`Ui::set_text_input`] hands the tree one frame's [`TextInput`] between
//! [`Ui::begin_frame_with`] and the build: its edits in the order they
//! arrived, the clipboard answers this frame delivers, and how long the frame
//! was — the clock the caret blinks on and a double-click is timed by, so
//! nothing here reads wall time. The tree reads no keyboard and no shell:
//! [`Edit::for_key`] turns a key into an edit, a `TextCommit` is an
//! [`Edit::Insert`], and the caller routes the rest.
//!
//! # The clipboard is a request and a later answer
//!
//! Copy and cut push a [`ClipboardRequest`] offering the selection; paste
//! pushes one asking for a read. [`Ui::take_clipboard_requests`] hands them to
//! the caller after the build, and a later frame's [`TextInput::clipboard`]
//! carries each [`ClipboardAnswer`] back to the input that asked. A read's text
//! replaces the selection the input has when it arrives, if the input is still
//! engaged. An answer of [`ClipboardReply::Refused`] — a backend with no
//! clipboard, or a read that failed — sets **`:refused`** on the input until
//! its next edit or until it is no longer engaged, which `default.css` draws as
//! a red border. A masked input offers nothing and cuts nothing.
//!
//! # What it builds
//!
//! A `text-input` block holding, in paint order: a `.text-input-selection`
//! block behind the selected glyphs while the input is engaged or pressed and
//! something is selected; the text as a `.text-input-text` span, or, while the
//! value is empty, the placeholder as a `.text-input-placeholder` span; and a
//! `.text-input-caret` block while it is engaged. The placeholder is a part
//! with a class of its own, as every other widget's parts are, rather than
//! CSS's `::placeholder`: the selector subset has no pseudo-elements, and one
//! part is not worth adding them for.
//!
//! The selection and the caret are placed after layout, by `Ui::layout`,
//! from the edges of every cluster in the span's own font — so the
//! highlight spans exactly the glyphs selected and the caret sits on the
//! boundary it names, in the bitmap font and a parsed one alike. In text
//! with right-to-left levels the caret before a char stands at its
//! cluster's leading edge — the right edge of a right-to-left cluster — and
//! the selection is one `.text-input-selection` block for each stretch of
//! one bidi level it reaches into, since a selection across a change of
//! direction is several pieces on the screen. The arrows still move the
//! caret in logical order, one cluster at a time. Their
//! `position: absolute` in `default.css` keeps them out of the span's flow.
//! The same pass scrolls the block: while it is engaged or pressed, by the
//! least that keeps the caret inside its content box, and never past the end
//! of the text; otherwise back to the start.
//!
//! The caret blinks on [`crate::console::caret_shown`]'s interval, and is shown
//! solid from any edit or caret move. A hidden caret is the same block with a
//! transparent background, so the blink changes no layout.

use std::panic::Location;
use std::time::Duration;

use crcbl_core::input::TextPurpose;
use glam::Vec2;

use super::super::store::Interaction;
use super::{Ui, WidgetKind, WidgetState, typed};
use crate::console::caret_shown;
use crate::edit::{ClipboardOp, Edit, LineEdit, run_at};
use crate::font::Font;
use crate::font::bidi::char_runs;
use crate::font::layout::{Carets, caret_stops};
use crate::style::{Declaration, NodeSelector, PseudoClasses};
use crate::text::FontAtlas;
use crate::tree::emit::content_width;
use crate::tree::{Behavior, Content, DRAG_THRESHOLD, Engagement, NodeKey, NodeStyle, Response};
use crate::widget::NATURAL_FONT_SIZE;

/// The longest gap between two presses that makes them a double-click: the
/// default of Windows' `GetDoubleClickTime`. A second press must also land
/// within [`DRAG_THRESHOLD`] of the first.
pub const DOUBLE_CLICK_TIME: Duration = Duration::from_millis(500);

/// What a masked input draws for every character of its value.
pub const MASK: char = '*';

/// One frame's text input for the tree; see the module docs.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct TextInput {
    /// How long this frame was: the clock the caret blinks on and a
    /// double-click is timed by.
    pub dt: Duration,
    /// Typed text and editing keys, in the order they arrived.
    pub edits: Vec<Edit>,
    /// The clipboard's answers that arrived for this frame.
    pub clipboard: Vec<ClipboardAnswer>,
}

/// What a text input asks of the clipboard; see the module docs.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ClipboardRequest {
    /// The input asking, which an answer names.
    pub from: NodeKey,
    /// The offer or the read.
    pub op: ClipboardOp,
}

/// The clipboard's answer to one [`ClipboardRequest`].
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ClipboardAnswer {
    /// The input that asked.
    pub to: NodeKey,
    /// What came back.
    pub reply: ClipboardReply,
}

/// What came back from the clipboard.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum ClipboardReply {
    /// A read's text.
    Text(String),
    /// A read found the clipboard empty, or holding no text.
    Empty,
    /// The offer or the read could not be made or answered.
    Refused,
}

/// How a text input behaves beyond its value.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct TextInputOptions<'a> {
    /// Drawn in the input while its value is empty.
    pub placeholder: &'a str,
    /// Draws every character as [`MASK`], and copies and cuts nothing: a
    /// password field.
    pub masked: bool,
    /// Leaves the caret where it was when an engagement no pointer began
    /// starts, rather than selecting the whole text: for a prompt engaged
    /// again after every click elsewhere, whose half-typed line the next key
    /// must not replace, or a line offering text to type after. See the
    /// module docs.
    pub keep_caret: bool,
}

/// What a text input keeps between frames, beside its store node.
#[derive(Clone, Debug, Default)]
pub(crate) struct EditState {
    line: LineEdit,
    /// Where each caret stop and cluster edge of what the span drew lies,
    /// from the span's content left edge, as last laid out.
    carets: Carets,
    /// A hash of what `carets` was measured from.
    measured: u64,
    /// The span's content left edge from the block's border box, unscrolled,
    /// as last laid out.
    origin: f32,
    /// How long since the caret last moved or the text last changed.
    blink: Duration,
    /// When and where the last single press began, for a double-click.
    last_press: Option<(Duration, Vec2)>,
    /// Whether the input held the press last frame.
    held: bool,
    /// Whether the press held now selected a word, so a drag keeps it.
    words: bool,
    /// Whether the clipboard refused this input since its last edit.
    refused: bool,
    /// Whether the input was engaged when it was last built, so the frame an
    /// engagement starts is told from the ones after it — [`Ui::engage`]'s
    /// first frame reports [`Engagement::Engaged`], not
    /// [`Engagement::Began`].
    engaged: bool,
    /// Whether it was built masked last, as a password field is.
    masked: bool,
}

impl EditState {
    /// A line holding `text`, all of it selected, so the first character typed
    /// replaces it. `held` says a press still down began it, which then keeps
    /// the selection rather than placing the caret where the press is, as a
    /// double-click's word is kept.
    pub(super) fn selecting_all(text: &str, held: bool) -> Self {
        let mut state = Self::default();
        state.select_all_of(text);
        state.held = held;
        state.words = held;
        state
    }

    /// The line's text.
    pub(in crate::tree) fn text(&self) -> &str {
        self.line.text()
    }

    /// The line, its caret and its selection.
    #[cfg(feature = "accesskit")]
    pub(in crate::tree) const fn line(&self) -> &LineEdit {
        &self.line
    }

    /// Replaces the line with `text`, all of it selected, the caret shown.
    pub(super) fn select_all_of(&mut self, text: &str) {
        self.line.set_text(text);
        self.line.select(0..self.line.len());
        self.blink = Duration::ZERO;
    }

    /// Sets `:refused` until the next edit, as a refused clipboard does.
    pub(super) fn refuse(&mut self) {
        self.refused = true;
    }
}

/// What [`Ui::line_block`] builds on: the node and what it keeps.
#[derive(Clone, Copy, Debug)]
pub(super) struct Line<'a> {
    /// The block's key, found before it is built.
    pub key: NodeKey,
    /// The block's selector.
    pub selector: NodeSelector<'a>,
    /// The block's inline declarations.
    pub inline: &'a [Declaration],
    /// What the node keeps between frames: a [`WidgetState`] that
    /// [`types`](WidgetState::types).
    pub widget: WidgetState,
}

/// Where one text input's parts are this frame, for the pass after layout.
#[derive(Clone, Debug)]
pub(crate) struct TextFit {
    key: NodeKey,
    block: usize,
    text: usize,
    /// Each `.text-input-selection` block and the chars it covers: the
    /// selection's part in one stretch of one bidi level.
    selection: Vec<(usize, core::ops::Range<usize>)>,
    caret: Option<usize>,
    caret_at: usize,
    /// Engaged or pressed: the view follows the caret.
    active: bool,
    /// The span shows the placeholder, so every caret stop is its start.
    placeholder: bool,
}

/// The caret stops and cluster edges of `text` at `style`'s size in `font` —
/// the bitmap font when that is `None` — from its left edge: in the bitmap
/// font every advance, left to right, and in a parsed font [`caret_stops`]'s,
/// placed as [`crate::font::layout::TextLayout`] shapes and places the glyphs.
/// A char inside a cluster, where [`crate::edit::LineEdit`] never puts the
/// caret, shares its cluster's edges.
fn measure(
    atlas: &FontAtlas,
    font: Option<&Font>,
    text: &str,
    style: &NodeStyle,
    out: &mut Carets,
) {
    match font {
        None => {
            out.stops.clear();
            out.trailing.clear();
            out.stops.push(0.0);
            let scale = style.font_size / NATURAL_FONT_SIZE;
            let mut buffer = [0; 4];
            let mut pen = 0.0;
            for c in text.chars() {
                pen += atlas.text_width(c.encode_utf8(&mut buffer), scale);
                out.stops.push(pen);
                out.trailing.push(pen);
            }
            out.width = pen;
        }
        Some(font) => caret_stops(font, text, style.font_size, out),
    }
}

impl Ui {
    /// A single-line text input editing `value`, with no placeholder and not
    /// masked: [`Ui::text_input_with`] with the default options.
    #[track_caller]
    pub fn text_input(&mut self, selector: &str, value: &mut String) -> Response {
        self.text_input_with(selector, value, TextInputOptions::default())
    }

    /// A single-line text input editing `value`: a `text-input` block holding
    /// the parts the module docs name. [`Response::changed`] is the frame the
    /// value changed — by an edit, a paste, or a cancel.
    ///
    /// `value` is the caller's: a change made to it between frames is taken as
    /// it stands, the caret held inside it, except that its control characters
    /// — line breaks among them — are removed from it.
    #[track_caller]
    pub fn text_input_with(
        &mut self,
        selector: &str,
        value: &mut String,
        options: TextInputOptions<'_>,
    ) -> Response {
        let before = value.clone();
        // The line holds no control characters, as `LineEdit::insert` drops
        // them, and HTML strips a text input's line breaks the same way. The
        // caller's value is held to that here rather than in `LineEdit::sync`,
        // which would otherwise see it differ from the line every frame.
        value.retain(|c| !c.is_control());
        let selector = typed("text-input", selector);
        let parsed = self.node_selector(&selector);
        let key = self.widget_key(parsed, Location::caller());
        let interaction = self.interaction_of(key);
        self.snapshot_for(key, interaction.engagement, value);
        let line = Line {
            key,
            selector: parsed,
            inline: &[],
            widget: WidgetState::TextInput,
        };
        let masked = options.masked;
        let mut response = self.line_block(line, interaction, value, options);
        self.mark(key, WidgetKind::TextInput { masked });
        response.changed = *value != before;
        response
    }

    /// A text input's block on `line.key`, editing `value` — everything
    /// [`Ui::text_input_with`] builds once it has its key and has restored a
    /// cancelled value, which a drag-value being typed into shares: see
    /// `number_entry.rs`. [`Response::changed`] is left for the caller, whose
    /// value is not always the text.
    pub(super) fn line_block(
        &mut self,
        line: Line<'_>,
        interaction: Interaction,
        value: &mut String,
        options: TextInputOptions<'_>,
    ) -> Response {
        let Line {
            key,
            selector: parsed,
            inline,
            widget,
        } = line;
        let disabled = self.building_disabled();
        let mut state = self.edits.remove(&key).unwrap_or_else(|| {
            let mut state = EditState::default();
            state.line.set_text(value);
            state
        });
        let mut moved = state.line.sync(value);
        state.masked = options.masked;
        if interaction.engagement == Engagement::Cancelled {
            moved |= state.line.place(usize::MAX, false);
        }

        if interaction.pressed && !disabled {
            moved |= self.press_text(key, &mut state);
        } else {
            state.words = false;
        }
        // A captured press still reports pressed on the frame it is released,
        // so only a button still down carries the press into the next frame.
        state.held = interaction.pressed && self.pointer.down && !disabled;

        let engaged = interaction.engagement.is_engaged();
        // Before this frame's edits, so a key typed in the frame
        // `Ui::engage` hands over replaces the text rather than joining it.
        let pointed = interaction.pressed || self.clicked_key().is_some();
        if engaged && !state.engaged && !pointed && !options.keep_caret {
            moved |= state.line.select(0..state.line.len());
        }
        if interaction.engagement == Engagement::Engaged && !disabled {
            for edit in std::mem::take(&mut self.text_frame.edits) {
                if options.masked && matches!(edit, Edit::Copy | Edit::Cut) {
                    continue;
                }
                let applied = state.line.apply(&edit);
                moved |= applied.caret || applied.text;
                if applied.text {
                    state.refused = false;
                }
                if let Some(op) = applied.clipboard {
                    self.clipboard_requests
                        .push(ClipboardRequest { from: key, op });
                }
            }
        }
        let mut answers = std::mem::take(&mut self.text_frame.clipboard);
        answers.retain(|answer| {
            if answer.to != key {
                return true;
            }
            match &answer.reply {
                ClipboardReply::Text(text) if engaged && !disabled => {
                    if state.line.insert(text) {
                        moved = true;
                        state.refused = false;
                    }
                }
                ClipboardReply::Refused => state.refused = engaged,
                ClipboardReply::Text(_) | ClipboardReply::Empty => {}
            }
            false
        });
        self.text_frame.clipboard = answers;
        if !engaged {
            state.refused = false;
        }
        if state.line.text() != value.as_str() {
            state.line.text().clone_into(value);
        }
        if moved || interaction.engagement == Engagement::Began {
            state.blink = Duration::ZERO;
        } else {
            state.blink += self.text_frame.dt;
        }

        let active = engaged || interaction.pressed;
        let placeholder = value.is_empty() && !options.placeholder.is_empty();
        let masked: String;
        let shown = if placeholder {
            options.placeholder
        } else if options.masked {
            masked = core::iter::repeat_n(MASK, state.line.len()).collect();
            &masked
        } else {
            state.line.text()
        };
        let range = state.line.selection();
        let pseudo = if state.refused {
            PseudoClasses::REFUSED
        } else {
            PseudoClasses::NONE
        };
        let hidden = [Declaration::Background([0.0; 4])];
        let caret_style: &[Declaration] = if caret_shown(state.blink) {
            &[]
        } else {
            &hidden
        };
        let mut fit = TextFit {
            key,
            block: self.nodes.len(),
            text: 0,
            selection: Vec::new(),
            caret: None,
            caret_at: state.line.caret(),
            active,
            placeholder,
        };
        let response = self.open_block(key, parsed, inline, Behavior::ENGAGE, pseudo, |ui| {
            if active && !placeholder && !range.is_empty() {
                for run in char_runs(shown) {
                    let chars = range.start.max(run.start)..range.end.min(run.end);
                    if !chars.is_empty() {
                        fit.selection.push((ui.nodes.len(), chars));
                        ui.block(".text-input-selection", &[], |_| {});
                    }
                }
            }
            fit.text = ui.nodes.len();
            let class = if placeholder {
                ".text-input-placeholder"
            } else {
                ".text-input-text"
            };
            ui.span(class, shown, &[]);
            if engaged {
                fit.caret = Some(ui.nodes.len());
                ui.block(".text-input-caret", caret_style, |_| {});
            }
        });
        state.engaged = engaged;
        self.edits.insert(key, state);
        self.set_widget_state(key, widget);
        self.fits.push(fit);
        response
    }

    /// A press held on the input `key`: the frame it lands, a caret placed
    /// where it landed — or, a second press soon enough and near enough after
    /// the first, the word under it selected; on later frames, the selection
    /// dragged to where the pointer is. Returns whether the caret moved.
    fn press_text(&self, key: NodeKey, state: &mut EditState) -> bool {
        let Some(node) = self.store.by_key(key) else {
            return false;
        };
        let x = self.pointer.pos.x - (node.rect.0.x - node.scroll_offset.x + state.origin);
        if state.held {
            return !state.words && state.line.place(state.carets.nearest(x), true);
        }
        let pos = self.pointer.pos;
        if self.second_press(state.last_press) {
            state.last_press = None;
            state.words = true;
            let chars: Vec<char> = state.line.text().chars().collect();
            state.line.select(run_at(&chars, state.carets.char_at(x)))
        } else {
            state.last_press = Some((self.text_clock, pos));
            state.words = false;
            state.line.place(state.carets.nearest(x), false)
        }
    }

    /// Whether a press landing now is the second of a double-click after a
    /// press that began at `last`: within [`DOUBLE_CLICK_TIME`] of it on the
    /// text clock, and within [`DRAG_THRESHOLD`] of where it landed.
    pub(super) fn second_press(&self, last: Option<(Duration, Vec2)>) -> bool {
        last.is_some_and(|(at, from)| {
            self.text_clock.saturating_sub(at) <= DOUBLE_CLICK_TIME
                && (self.pointer.pos - from).length_squared() <= DRAG_THRESHOLD * DRAG_THRESHOLD
        })
    }

    /// Hands the tree this frame's text input; see the module docs. Call it
    /// between [`Ui::begin_frame_with`] and the build: a frame that does not
    /// has no text input, and its clock does not advance.
    pub fn set_text_input(&mut self, input: TextInput) {
        self.text_clock += input.dt;
        self.text_frame = input;
    }

    /// The clipboard requests this frame's build made, oldest first, for the
    /// caller to carry to the platform. A frame's requests are dropped when
    /// the next frame begins.
    pub fn take_clipboard_requests(&mut self) -> Vec<ClipboardRequest> {
        std::mem::take(&mut self.clipboard_requests)
    }

    /// Whether the engaged node is a text input, or a drag-value being typed
    /// into: while it is, the caller's typing and editing keys belong to the
    /// tree and not to navigation or a game.
    #[must_use]
    pub fn text_editing(&self) -> bool {
        self.engaged()
            .and_then(|key| self.store.by_key(key))
            .is_some_and(|node| node.widget.types())
    }

    /// What the field being typed into is for, as the platform input method
    /// should be told with `crcbl_shell::Shell::set_text_input`: `None` while
    /// [`text_editing`](Self::text_editing) is false, a password for a masked
    /// text input, and ordinary text otherwise.
    #[must_use]
    pub fn text_editing_purpose(&self) -> Option<TextPurpose> {
        let key = self.engaged()?;
        let node = self.store.by_key(key)?;
        node.widget.types().then(|| {
            if self.edits.get(&key).is_some_and(|state| state.masked) {
                TextPurpose::Password
            } else {
                TextPurpose::Normal
            }
        })
    }

    /// The text input `key`'s caret and selection anchor, as `char` counts;
    /// `None` for a node that is not a text input.
    #[must_use]
    pub fn text_caret(&self, key: NodeKey) -> Option<(usize, usize)> {
        self.edits
            .get(&key)
            .map(|state| (state.line.caret(), state.line.anchor()))
    }

    /// Places every text input's selection and caret on the boundaries of the
    /// glyphs they name, and scrolls each block to keep its caret in view:
    /// after Taffy's layout is rounded, before scroll offsets are clamped and
    /// rectangles placed. See the module docs.
    pub(in crate::tree) fn fit_text_inputs(&mut self, atlas: &FontAtlas) {
        let fits = std::mem::take(&mut self.fits);
        for fit in &fits {
            let Some(state) = self.edits.get_mut(&fit.key) else {
                continue;
            };
            let span = &self.nodes[fit.text];
            let Content::Text { start, end } = span.content else {
                continue;
            };
            let shown = if fit.placeholder {
                ""
            } else {
                &self.text[start..end]
            };
            let style = &span.style;
            let hash = super::super::hash_of((
                shown,
                style.font_size.to_bits(),
                style.font_family,
                span.font
                    .as_ref()
                    .map(|font| (font.id(), self.fallback_hash)),
            ));
            if state.carets.stops.is_empty() || state.measured != hash {
                measure(atlas, span.font.as_ref(), shown, style, &mut state.carets);
                state.measured = hash;
            }
            let carets = &state.carets;
            let last = carets.stops.len() - 1;
            let at = |index: usize, origin: f32| (origin + carets.stops[index.min(last)]).round();

            let span_layout = span.layout;
            let block = self.nodes[fit.block].layout;
            let inset = block.border.left + block.padding.left;
            let view = content_width(&block);
            let origin =
                span_layout.location.x + span_layout.border.left + span_layout.padding.left;
            state.origin = origin;
            let caret_x = at(fit.caret_at, origin);
            let caret_width = fit
                .caret
                .map_or(0.0, |index| self.nodes[index].layout.size.width);

            let stored = self.store.get_mut(self.nodes[fit.block].slot);
            let mut offset = if fit.active {
                stored.scroll_offset.x
            } else {
                0.0
            };
            if fit.active {
                if caret_x < inset + offset {
                    offset = caret_x - inset;
                } else if caret_x + caret_width > inset + offset + view {
                    offset = caret_x + caret_width - inset - view;
                }
            }
            let reach = ((origin + carets.width).round() + caret_width - inset - view).max(0.0);
            stored.scroll_offset = Vec2::new(offset.clamp(0.0, reach), 0.0);

            let top = span_layout.location.y;
            let height = span_layout.size.height;
            for (index, chars) in &fit.selection {
                let shown_chars = carets.trailing.len();
                let chars = chars.start.min(shown_chars)..chars.end.min(shown_chars);
                let (left, right) = carets.span(chars);
                let (left, right) = ((origin + left).round(), (origin + right).round());
                let layout = &mut self.nodes[*index].layout;
                layout.location = taffy::Point { x: left, y: top };
                layout.size = taffy::Size {
                    width: right - left,
                    height,
                };
            }
            if let Some(index) = fit.caret {
                let layout = &mut self.nodes[index].layout;
                layout.location = taffy::Point { x: caret_x, y: top };
                layout.size.height = height;
            }
        }
        self.fits = fits;
        self.fits.clear();
    }
}

#[cfg(all(test, feature = "parsed-font"))]
mod fallback_tests {
    use super::*;
    use crate::font::fixtures;
    use crate::font::layout::TextLayout;

    /// **Caret stops fall back as layout places glyphs**: the stop before
    /// each cluster is where layout draws it, an emoji in the fallback face
    /// takes that face's advance, and the presentation selector inside its
    /// cluster shares the emoji's stop.
    #[test]
    fn caret_stops_fall_back_as_layout_places_glyphs() {
        let font = Font::sans().with_fallback(&[fixtures::cbdt()]);
        let text = "A\u{1F600}\u{FE0F}V";
        let style = NodeStyle {
            font_size: 20.0,
            ..NodeStyle::default()
        };
        let mut carets = Carets::default();
        measure(
            &FontAtlas::built_in(),
            Some(&font),
            text,
            &style,
            &mut carets,
        );
        let layout = TextLayout::new(&font, text, style.font_size, 24.0, None);
        let x: Vec<f32> = layout.glyphs().iter().map(|glyph| glyph.offset.x).collect();
        assert_eq!(layout.faces(), [0, 1, 0]);
        assert_eq!(x[2] - x[1], style.font_size, "the emoji is an em wide");
        assert_eq!(carets.stops, [0.0, x[1], x[1], x[2], layout.width()]);
    }
}
