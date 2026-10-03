//! Navigation inside an open pop-up beyond the arrows: Home, End, Page Up and
//! Page Down, and a list's typeahead.
//!
//! # Jumps
//!
//! A frame's [`Jump`] moves focus inside the **topmost open pop-up**, and does
//! nothing while none is open or a node is engaged:
//!
//! * [`Jump::First`] and [`Jump::Last`] go to the first and last node in it
//!   focus can rest on, in tree order — so past a disabled item, which focus
//!   never rests on.
//! * [`Jump::PageDown`] goes to the furthest node after the focused one whose
//!   bottom edge is no more than the pop-up's content height below the focused
//!   node's, and [`Jump::PageUp`] to the furthest before it whose top edge is no
//!   more than that above — a view's worth, or one node when the next is taller
//!   than the view.
//!
//! Focus scrolls the node it lands on into view as every move does
//! (`focus/mod.rs`). **Jumps are the pop-up's alone, not a panel's**: a
//! virtualized list or outliner builds only the rows in its window, so its
//! first and last built rows are not its first and last, and Home and End
//! there want the widget's own answer.
//!
//! # Typeahead
//!
//! A drop-down's list and every level of a context menu take the frame's typed
//! text ([`TextInput`](super::TextInput)'s inserts) while theirs is the topmost
//! pop-up and nothing is engaged. Each character is added to a prefix, which
//! starts again after [`TYPEAHEAD_TIMEOUT`] on the `Ui`'s text clock — the one
//! [`TextInput::dt`](super::TextInput::dt) advances, never wall time — or when
//! another list takes the text. Focus goes to the first item from the focused
//! one, wrapping, whose label starts with the prefix, ignoring case, skipping
//! disabled items; a prefix of one letter typed again and again starts after
//! the focused item instead, so it steps through the items starting with that
//! letter, as Windows' lists and menus do. White space and control characters
//! are not typed: Space is accept. The move is a [`Ui::set_focus`], so the
//! item is focused when the next frame begins and drawn in view in this one.
//!
//! **The keys arrive through the `list` context**: [`Ui::popup_list_open`] says
//! when a caller should push it, as [`Ui::text_editing`] says when to push
//! `text`. While it is pushed the letters, digits and punctuation are the
//! list's — so W is not `ui_move` and a game's binding on it hears nothing —
//! and Home, End, Page Up and Page Down are its jumps.

use std::time::Duration;

use glam::Vec2;

use super::popup::OpenPopup;
use super::store::NodeKey;
use super::{Jump, Ui};
use crate::edit::Edit;

/// How long a list waits after a typed character before the next one starts
/// a new prefix rather than extending it: long enough to type a word at a
/// hunt-and-peck pace, short enough that a pause to read the list starts
/// afresh.
pub const TYPEAHEAD_TIMEOUT: Duration = Duration::from_secs(1);

/// The prefix typed into the topmost list; see the module docs.
#[derive(Clone, Debug, Default)]
pub(super) struct Typeahead {
    /// The anchor of the list the prefix was typed into.
    list: Option<NodeKey>,
    /// What has been typed, as typed.
    prefix: String,
    /// The text clock when the last character was typed.
    typed_at: Duration,
    /// The anchor of the topmost pop-up a list widget built this frame.
    pub built: Option<NodeKey>,
}

/// One item of a list, for typeahead: its key, its label, and whether it can
/// be picked.
pub(super) struct ListItem<'a> {
    pub key: NodeKey,
    pub label: &'a str,
    pub enabled: bool,
}

impl Ui {
    /// Whether the topmost open pop-up is a list — a drop-down's or a context
    /// menu's, built this frame — with nothing engaged: while it is, the
    /// caller's typed characters, Home, End, Page Up and Page Down belong to
    /// the list and not to navigation or a game, and its `TextInput` should
    /// carry what is typed. See `popup_nav.rs`.
    #[must_use]
    pub fn popup_list_open(&self) -> bool {
        self.engaged().is_none()
            && self
                .popups
                .last()
                .is_some_and(|open| self.typeahead.built == Some(open.anchor))
    }

    /// The frame's jump inside the topmost open pop-up; see the module docs.
    pub(super) fn jump_in_popup(&mut self, jump: Jump) {
        let Some(&OpenPopup { anchor, .. }) = self.popups.last() else {
            return;
        };
        let root = Self::popup_key(anchor);
        let order = self.focusable_in(Some(root));
        let at = self
            .focused()
            .and_then(|focused| order.iter().position(|&key| key == focused));
        // Every node `focusable_in` names is stored.
        let rects: Vec<(Vec2, Vec2)> = order
            .iter()
            .map(|&key| {
                self.store
                    .by_key(key)
                    .map_or((Vec2::ZERO, Vec2::ZERO), |node| node.rect)
            })
            .collect();
        let page = self.store.by_key(root).map_or(0.0, |node| {
            let (min, max) = node.content_box();
            max.y - min.y
        });
        let target = match (jump, at) {
            (Jump::First, _) | (Jump::PageDown, None) => (!order.is_empty()).then_some(0),
            (Jump::Last, _) | (Jump::PageUp, None) => order.len().checked_sub(1),
            (Jump::PageDown, Some(at)) => {
                let limit = rects[at].1.y + page;
                (at + 1..order.len())
                    .take_while(|&index| rects[index].1.y <= limit)
                    .last()
                    .or_else(|| (at + 1 < order.len()).then_some(at + 1))
            }
            (Jump::PageUp, Some(at)) => {
                let limit = rects[at].0.y - page;
                (0..at)
                    .rev()
                    .take_while(|&index| rects[index].0.y >= limit)
                    .last()
                    .or_else(|| at.checked_sub(1))
            }
        };
        if let Some(index) = target {
            self.move_focus(order[index]);
        }
    }

    /// Typeahead over `items`, the list a widget is building as the pop-up
    /// hanging from `anchor`: marks it as a list for
    /// [`Ui::popup_list_open`] when it is the topmost pop-up, and moves focus
    /// by the frame's typed text as the module docs say. Call it while
    /// building the list, every frame it is open.
    pub(super) fn typeahead(&mut self, anchor: NodeKey, items: &[ListItem<'_>]) {
        if self.popups.last().is_none_or(|open| open.anchor != anchor) {
            return;
        }
        self.typeahead.built = Some(anchor);
        if self.engaged().is_some() {
            return;
        }
        let typed: Vec<char> = self
            .text_frame
            .edits
            .iter()
            .filter_map(|edit| match edit {
                Edit::Insert(text) => Some(text.chars()),
                _ => None,
            })
            .flatten()
            .filter(|character| !character.is_whitespace() && !character.is_control())
            .collect();
        if typed.is_empty() {
            return;
        }
        let state = &mut self.typeahead;
        if state.list != Some(anchor)
            || self.text_clock.saturating_sub(state.typed_at) > TYPEAHEAD_TIMEOUT
        {
            state.prefix.clear();
        }
        state.list = Some(anchor);
        state.typed_at = self.text_clock;

        let focused = self.focused();
        let mut at = items.iter().position(|item| Some(item.key) == focused);
        let mut moved = None;
        for character in typed {
            self.typeahead.prefix.push(character);
            let prefix = self.typeahead.prefix.to_lowercase();
            let mut letters = prefix.chars();
            let first = letters.next();
            // One letter typed again and again steps through the items that
            // start with it, rather than looking for "aaa".
            let (needle, start) = if letters.all(|letter| Some(letter) == first) {
                let needle: String = first.into_iter().collect();
                (needle, at.map_or(0, |at| at + 1))
            } else {
                (prefix, at.unwrap_or(0))
            };
            let found = (0..items.len())
                .map(|offset| (start + offset) % items.len())
                .find(|&index| {
                    items[index].enabled && items[index].label.to_lowercase().starts_with(&needle)
                });
            if let Some(index) = found {
                at = Some(index);
                moved = Some(items[index].key);
            }
        }
        if let Some(key) = moved
            && Some(key) != focused
        {
            self.set_focus(key);
        }
    }
}
