//! Tooltips: a line of text hung from a widget once the pointer has rested on
//! it, or navigation has held focus on it, for [`TOOLTIP_DELAY`].
//!
//! # Asking for one
//!
//! A widget's builder passes the [`Response`] it got back to [`Ui::tooltip`]
//! with the text, every frame, as it passes one to [`Ui::snapshot`]. The call
//! builds nothing until the tooltip is due, so it can sit after a widget
//! unconditionally; when it is due it builds a `tooltip` block holding the
//! text, styled by `default.css`'s `tooltip` rule, which an app's sheet
//! restyles by naming the same selector.
//!
//! # When it shows
//!
//! **The subject** is the widget the tooltip would be for, decided when a
//! frame begins from last frame's tree, by the mixed-input rule in
//! `focus/mod.rs`: in [`InputMode::Pointer`] the innermost hovered node that
//! asked for a tooltip last frame, in [`InputMode::Navigation`] the focused
//! node if it asked for one. Whichever device spoke, a tooltip shows once the
//! same subject has been held for the delay, counted from the first frame that
//! built its [`Ui::tooltip`] call after the subject changed — on the clock
//! [`TextInput::dt`](super::TextInput::dt) advances, the one the caret blinks
//! on, so a frame that hands the tree no text input does not move it.
//! [`Ui::set_tooltip_delay`] replaces the delay.
//!
//! **It hides** when the subject changes — the pointer leaves, focus moves or
//! blurs, the mode changes — and when the widget is not built: the call that
//! builds it is the widget's. **A press of either button hides it**, as accept
//! and `ui_menu` do — each fires or opens something the tooltip would sit
//! over — and so does a wheel ([`Ui::scroll_wheel`], or
//! [`Ui::dismiss_tooltip`] for a caller that scrolls its own way); a dismissed
//! tooltip stays hidden until the subject changes, so a click on a button does
//! not bring its tooltip back over the result.
//!
//! # Where it is drawn
//!
//! A tooltip is a pop-up of its own kind, on the same layer machinery
//! (`popup.rs`): a root of its own, keyed by its anchor ([`Ui::tooltip_key`]),
//! placed as a pop-up is — below its anchor, flipped above it at the bottom
//! of the viewport, shifted into the viewport — clipped to the viewport, and
//! drawn after every open pop-up. **It is inert**: never hit-tested, so the
//! pointer and every press pass through it to what lies beneath, and never
//! focused or a focus scope. A press anywhere is delivered as if it were not
//! there; it only hides it. It is not in the pop-up stack, so nothing closes
//! it and nothing it does closes a pop-up.

use std::time::Duration;

use super::popup::CLOSED_LAYER;
use super::store::NodeKey;
use super::{Behavior, InputMode, KeySource, NavInput, ROOT_KEY, Response, Ui};
use crate::widget::PointerInput;

/// How long a subject is held before its tooltip shows: the default of
/// Windows' `TTDT_INITIAL`, which is the double-click time
/// ([`DOUBLE_CLICK_TIME`](super::DOUBLE_CLICK_TIME)).
pub const TOOLTIP_DELAY: Duration = Duration::from_millis(500);

/// The layer a tooltip is drawn in: above every open pop-up's, whose layers
/// count up from 1, and never hit.
pub(super) const TOOLTIP_LAYER: usize = CLOSED_LAYER - 1;

/// A tree's tooltip, between frames and within one; see the module docs.
#[derive(Debug)]
pub(super) struct TooltipState {
    delay: Duration,
    /// The node whose tooltip is pending or shown.
    subject: Option<NodeKey>,
    /// The clock when the subject's call was first built, once it has been.
    since: Option<Duration>,
    /// A press, accept or wheel hid it for as long as the subject lasts.
    dismissed: bool,
    /// This frame's tooltip root and its anchor, as indices into the frame's
    /// nodes, once built.
    pub built: Option<(usize, usize)>,
}

impl Default for TooltipState {
    fn default() -> Self {
        Self {
            delay: TOOLTIP_DELAY,
            subject: None,
            since: None,
            dismissed: false,
            built: None,
        }
    }
}

impl Ui {
    /// A tooltip saying `text` for the widget `anchor` answers for, built — a
    /// `tooltip` block holding a span — once it is due and `None` until then;
    /// see the module docs. Call it every frame the widget is built, after it.
    pub fn tooltip(&mut self, anchor: &Response, text: &str) -> Option<Response> {
        let slot = self.store.find(anchor.key)?;
        self.store.get_mut(slot).tooltip = true;
        let state = &mut self.tooltip;
        if state.subject != Some(anchor.key) || state.dismissed || state.built.is_some() {
            return None;
        }
        let since = *state.since.get_or_insert(self.text_clock);
        if self.text_clock.saturating_sub(since) < state.delay {
            return None;
        }
        let anchor_index = self.nodes.iter().rposition(|node| node.key == anchor.key)?;
        let (index, response) = self.open_root(
            Self::tooltip_key(anchor.key),
            "tooltip",
            &[],
            Behavior::NONE,
            |ui| {
                ui.span("", text, &[]);
            },
        );
        self.tooltip.built = Some((index, anchor_index));
        Some(response)
    }

    /// The key of the root [`Ui::tooltip`] builds for `anchor`: what a caller
    /// or a test walks a tooltip's span from with [`Ui::child_keys`].
    #[must_use]
    pub fn tooltip_key(anchor: NodeKey) -> NodeKey {
        Self::key_under(ROOT_KEY, KeySource::Tooltip(anchor.0))
    }

    /// Replaces how long a subject is held before its tooltip shows, from
    /// [`TOOLTIP_DELAY`].
    pub fn set_tooltip_delay(&mut self, delay: Duration) {
        self.tooltip.delay = delay;
    }

    /// Hides the tooltip until its subject changes, as a press does: for a
    /// caller that scrolls a block itself rather than through
    /// [`Ui::scroll_wheel`]. Call it between [`Ui::begin_frame_with`] and the
    /// build to hide it from this frame on.
    pub fn dismiss_tooltip(&mut self) {
        self.tooltip.dismissed = true;
    }

    /// This frame's subject, from last frame's tree, and whether the frame's
    /// press or accept dismisses it; runs once pointer and focus are resolved.
    pub(super) fn resolve_tooltip(&mut self, pointer: PointerInput, nav: NavInput) {
        self.tooltip.built = None;
        let subject = match self.focus.mode {
            InputMode::Pointer => self
                .store
                .iter()
                .filter(|node| node.tooltip && node.interaction.hovered)
                .max_by_key(|node| node.stacking())
                .map(|node| node.key),
            InputMode::Navigation => self
                .focused()
                .filter(|&key| self.store.by_key(key).is_some_and(|node| node.tooltip)),
        };
        let state = &mut self.tooltip;
        if subject != state.subject {
            state.subject = subject;
            state.since = None;
            state.dismissed = false;
        }
        if pointer.down || pointer.secondary_pressed || nav.accept || nav.menu {
            state.dismissed = true;
        }
    }
}
