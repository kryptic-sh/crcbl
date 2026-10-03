//! Typing a number into a drag-value: UI rung 7's click-to-type, a
//! composition of [`Ui::drag_value`] and [`Ui::text_input`]'s line.
//!
//! # The way in
//!
//! A **double-click** on a drag-value, or **accept** while it is focused —
//! Enter or Space on a keyboard, the pad's accept — opens it for typing, as
//! Dear ImGui's drag widgets and Blender's number fields open on a
//! double-click and ImGui's on Enter. A single click still engages it for
//! stepping, as before: a pointer edits a number by dragging it, and a field a
//! click opened for typing would take the keys an editor gives a clicked field
//! — Ctrl+V over it pastes a value through the editor's own clipboard, which
//! reads the text exactly.
//!
//! **Typing a digit on a focused drag-value does not open it.** The tree hears
//! typed text only while [`Ui::text_editing`] says a field is engaged, and that
//! is also what pushes the `text` context that takes the arrows from
//! navigation; a drag-value that heard digits while it was not being typed
//! into would need a third state for the caller to route keys by.
//!
//! # While it is typed into
//!
//! The node keeps its key and its `drag-value` type, and is built as a text
//! input's line on it — caret, selection, word selection, the clipboard,
//! `:refused` — held at the width it had, with its overflow hidden, so the row
//! it sits in does not move as the text grows. Its text is every digit of the
//! value ([`DragNumber`](super::DragNumber)'s exact text: the fewest digits
//! that read back to the same `f64`, and a whole number whole), **all of it
//! selected**, so what is typed first replaces it.
//!
//! **A step still steps.** Up and down move the value by `step`, as a number
//! field's arrows do; so do left and right when they arrive as navigation —
//! a pad's — though a keyboard's move the caret. A step starts from the typed
//! number when the text spells one and from the value otherwise, moves the
//! value that frame as a step always has, and puts the stepped value's text
//! in the line, selected.
//!
//! # The way out
//!
//! **Accept puts the number in**: the text, parsed straight to the field's own
//! kind and held inside its range, becomes the value — one change, in the
//! frame it is accepted. **Text that spells no number of the kind is refused**:
//! the value is left as it was, nothing is reported, and the field stays open
//! with `:refused` set until the next edit, so the mistake can be mended
//! rather than retyped. A whole number refuses a fraction, an exponent and a
//! number past its type's ends; a float refuses one that is not finite.
//!
//! **A click elsewhere puts the number in the same way**, as it commits every
//! engaged widget; text it would refuse is dropped there instead, because
//! focus has gone and there is no field left open to mend it in.
//!
//! **Tab and Shift+Tab put the number in and go on**: focus moves to the next
//! or the previous focusable node in tree order, and when that is another
//! drag-value it opens for typing with its text selected, as the focus
//! module's exception to the engaged rule says — so a vector row's `x`, `y`
//! and `z` are typed in turn, each put in as a change of its own. Text Tab
//! would refuse is dropped, as for a click elsewhere: focus has gone.
//!
//! **Back puts back the value the engagement began with**, bit for bit — the
//! value before any step taken while engaged, as back does for every engaged
//! widget, even across a refused accept.

use super::super::store::{Interaction, NodeKey};
use super::text_input::{EditState, Line};
use super::value::sealed::Sealed;
use super::{TextInputOptions, Ui, WidgetState};
use crate::style::{Declaration, NodeSelector};
use crate::tree::{Direction, Engagement, LengthAuto, NavStep, Overflow, Response};

/// The drag-value a number is typed into: its node, and what it is held to.
#[derive(Clone, Copy, Debug)]
pub(super) struct NumberEntry<'a, N> {
    /// The node's key, found before it is built.
    pub key: NodeKey,
    /// The node's selector.
    pub selector: NodeSelector<'a>,
    /// The interaction it began this frame with.
    pub interaction: Interaction,
    /// The range's start.
    pub min: N,
    /// The range's end.
    pub max: N,
    /// What one step moves it by.
    pub step: N,
}

/// Which way a step the field captured moves a number being typed into:
/// forward for up and right, back for down and left; none for any other step.
const fn typed_step(step: Option<NavStep>) -> Option<bool> {
    match step {
        Some(NavStep::Move(Direction::Up | Direction::Right)) => Some(true),
        Some(NavStep::Move(Direction::Down | Direction::Left)) => Some(false),
        _ => None,
    }
}

impl Ui {
    /// Opens the drag-value `entry` names for typing, with `value`'s every
    /// digit selected, and builds it. `held` says a press still down opened
    /// it — a double-click's second — which keeps the selection rather than
    /// placing the caret.
    pub(super) fn begin_typing<N: Sealed>(
        &mut self,
        entry: NumberEntry<'_, N>,
        value: &mut N,
        held: bool,
    ) -> Response {
        let mut interaction = entry.interaction;
        if !interaction.engagement.is_engaged() {
            // A double-click whose first click left it unengaged. It is under
            // the press, so it is a drawn, focusable engage node, which
            // `Ui::engage` takes.
            self.engage(entry.key);
            interaction.engagement = Engagement::Engaged;
        }
        let original = self.snapshot_of::<N>(entry.key).unwrap_or(*value);
        self.edits.insert(
            entry.key,
            EditState::selecting_all(&value.exact_text(), held),
        );
        self.typing_block(&entry, interaction, original)
    }

    /// A frame of the drag-value `entry` names while it is typed into, which
    /// engaged with `original`; see the module docs. Returns its response, or
    /// `None` when typing ended this frame — the number put in, refused on a
    /// click elsewhere, or cancelled — for the plain drag-value to be built
    /// instead.
    pub(super) fn typed_number<N: Sealed>(
        &mut self,
        entry: NumberEntry<'_, N>,
        value: &mut N,
        original: N,
    ) -> Option<Response> {
        let key = entry.key;
        let mut interaction = entry.interaction;
        match interaction.engagement {
            Engagement::Cancelled => {
                *value = original;
                self.edits.remove(&key);
                return None;
            }
            // Engagement taken away without an end — the node stopped being
            // focusable — leaves the value as it is, the typing unput.
            Engagement::Idle => {
                self.edits.remove(&key);
                return None;
            }
            Engagement::Committed => {
                let typed = self
                    .edits
                    .get(&key)
                    .and_then(|state| N::parse_text(state.text()));
                if let Some(typed) = typed {
                    *value = typed.clamped(entry.min, entry.max);
                    self.edits.remove(&key);
                    return None;
                }
                if !self.committed_by_accept(key) {
                    self.edits.remove(&key);
                    return None;
                }
                // Refused: open again for the frames that follow, and built
                // now as the engaged frame it hands over to.
                self.engage(key);
                if let Some(state) = self.edits.get_mut(&key) {
                    state.refuse();
                }
                interaction.engagement = Engagement::Engaged;
            }
            Engagement::Began | Engagement::Engaged => {}
        }
        if let Some(forward) = typed_step(interaction.captured)
            && let Some(state) = self.edits.get_mut(&key)
        {
            let from = N::parse_text(state.text()).unwrap_or(*value);
            *value = from
                .stepped(entry.step, forward)
                .clamped(entry.min, entry.max);
            state.select_all_of(&value.exact_text());
        }
        Some(self.typing_block(&entry, interaction, original))
    }

    /// Whether this frame's commit of `key` was accept on it — nothing
    /// clicked, `key` still focused and nothing else engaged — rather than
    /// focus going elsewhere.
    fn committed_by_accept(&self, key: NodeKey) -> bool {
        self.clicked_key().is_none() && self.focused() == Some(key) && self.engaged().is_none()
    }

    /// The drag-value `entry` names built as a text input's line over the text
    /// it keeps, at the width it was last laid out at.
    fn typing_block<N: Sealed>(
        &mut self,
        entry: &NumberEntry<'_, N>,
        interaction: Interaction,
        original: N,
    ) -> Response {
        let mut text = self
            .edits
            .get(&entry.key)
            .map(|state| state.text().to_owned())
            .unwrap_or_default();
        let width = self
            .store
            .by_key(entry.key)
            .map(|node| node.rect.1.x - node.rect.0.x);
        let hidden = [Declaration::Overflow(Overflow::Hidden)];
        let sized: [Declaration; 2];
        let inline: &[Declaration] = match width {
            Some(width) => {
                sized = [hidden[0], Declaration::Width(LengthAuto::Px(width))];
                &sized
            }
            None => &hidden,
        };
        let line = Line {
            key: entry.key,
            selector: entry.selector,
            inline,
            widget: WidgetState::TypedNumber(original.anchor()),
        };
        self.line_block(line, interaction, &mut text, TextInputOptions::default())
    }
}
