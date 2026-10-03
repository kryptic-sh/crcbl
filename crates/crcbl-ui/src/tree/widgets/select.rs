//! A drop-down: a button showing the chosen option, which opens the list of
//! options in a pop-up.
//!
//! The button and every option are [`Behavior::BUTTON`] nodes, so a click and
//! accept arrive as the same [`Response::clicked`], as everywhere in the set.
//! The list is a pop-up hanging from the button (`popup.rs`), so it draws over
//! whatever is below the button, clipped by nothing but the viewport, and
//! traps focus while it is open: focus lands on the chosen option, the frame's
//! navigation moves between options as it moves anywhere, accept or a click
//! picks one, and back or a press outside the list closes it with nothing
//! picked. Every way the list closes gives focus back to the button.
//!
//! **Arrowing over a closed drop-down moves past it**, as the LOCKED focus
//! rule in `focus/mod.rs` wants: only accept or a click opens the list, and
//! the list, not the button, is what navigation then drives.
//!
//! **The arrows stop at the list's ends**, as a Windows list box's do, where a
//! context menu's wrap. A list taller than the viewport scrolls, opening
//! scrolled to the chosen option (`popup.rs`); Home, End, Page Up, Page Down
//! and typeahead move through it as `popup_nav.rs` says.

use std::panic::Location;

use super::{Ui, typed};
use crate::style::{Declaration, PseudoClasses};
use crate::tree::popup_nav::ListItem;
use crate::tree::{Behavior, KeySource, LengthAuto, NodeKey, Response, hash_of};

/// What a drop-down's `.select-caret` shows: a letter rather than an arrow,
/// because the built-in bitmap font covers printable ASCII and nothing else.
pub const SELECT_CARET: &str = "v";

impl Ui {
    /// A drop-down choosing one of `options`, the one at `chosen`: a `select`
    /// block holding a `.select-label` span showing the chosen option and a
    /// `.select-caret` span showing [`SELECT_CARET`], `:open` while its list
    /// is. A click or accept
    /// opens the list — a `popup.select-list` pop-up at least as wide as the
    /// button, holding one `.select-option` block per option, each holding a
    /// `.select-option-label` span and `:checked` while it is the chosen one.
    ///
    /// Picking an option writes its index to `chosen` in the frame the pick
    /// arrives, closes the list, and sets [`Response::changed`] on the
    /// button's response — unless it was the chosen option already, which only
    /// closes the list. An index past the end of `options` shows no option as
    /// chosen and an empty label.
    ///
    /// Options are keyed by their text, as a tab strip's tabs are, so two
    /// options of one text are a duplicate key. Disabled, the drop-down never
    /// opens, and its list closes if it was open.
    #[track_caller]
    pub fn select(&mut self, selector: &str, options: &[&str], chosen: &mut usize) -> Response {
        let selector = typed("select", selector);
        let parsed = self.node_selector(&selector);
        let key = self.widget_key(parsed, Location::caller());
        let enabled = !self.building_disabled();

        // Which option a click landed on is resolved before anything is
        // built, so the button's label and the list agree in the frame the
        // click arrives, and a picked list is not drawn that frame at all.
        let list = Self::popup_key(key);
        let keys: Vec<NodeKey> = options
            .iter()
            .map(|&option| Self::key_under(list, KeySource::Keyed(hash_of(option))))
            .collect();
        let mut changed = false;
        if !enabled {
            self.close_popup(key);
        } else if self.is_popup_open(key)
            && let Some(picked) = keys
                .iter()
                .position(|&option| self.interaction_of(option).clicked)
        {
            changed = picked != *chosen;
            *chosen = picked;
            self.close_popup(key);
        }
        // The click was resolved against last frame's node, which may have
        // been enabled, so `enabled` is this frame's word, as a checkbox's.
        let opening = enabled && !self.is_popup_open(key) && self.interaction_of(key).clicked;
        if opening {
            self.open_popup(key);
        }

        let label = options.get(*chosen).copied().unwrap_or("");
        let state = if self.is_popup_open(key) {
            PseudoClasses::OPEN
        } else {
            PseudoClasses::NONE
        };
        let mut response = self.open_block(key, parsed, &[], Behavior::BUTTON, state, |ui| {
            ui.span(".select-label", label, &[]);
            ui.span(".select-caret", SELECT_CARET, &[]);
        });
        response.changed = changed;

        // As wide as the button at least, as last frame laid the button out.
        let inline: Vec<Declaration> = self
            .rect(key)
            .map(|(min, max)| Declaration::MinWidth(LengthAuto::Px(max.x - min.x)))
            .into_iter()
            .collect();
        let chosen = *chosen;
        self.popup(key, ".select-list", &inline, |ui| {
            let parsed = ui.node_selector(".select-option");
            for (index, (&option, &option_key)) in options.iter().zip(&keys).enumerate() {
                let state = if index == chosen {
                    PseudoClasses::CHECKED
                } else {
                    PseudoClasses::NONE
                };
                let built = ui.open_block(option_key, parsed, &[], Behavior::BUTTON, state, |ui| {
                    ui.span(".select-option-label", option, &[]);
                });
                // Focus moves into the list on the chosen option, rather than
                // wherever the landing rule would put it, and the list opens
                // scrolled to it.
                if opening && index == chosen {
                    ui.set_focus(built.key);
                }
            }
        });
        if self.is_popup_open(key) {
            let items: Vec<ListItem<'_>> = options
                .iter()
                .zip(&keys)
                .map(|(&label, &key)| ListItem {
                    key,
                    label,
                    enabled: true,
                })
                .collect();
            self.typeahead(key, &items);
        }
        response
    }
}
