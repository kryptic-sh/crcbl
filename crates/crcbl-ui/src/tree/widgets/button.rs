//! The instant-activation widgets: a button and a checkbox.
//!
//! Both are [`Behavior::BUTTON`] nodes, so a click and accept arrive as the one
//! [`Response::clicked`] — the plan's "same event path as click, widgets can't
//! tell" — and neither has an engaged state.

use std::panic::Location;

use super::{Ui, typed};
use crate::style::PseudoClasses;
use crate::tree::{Behavior, Response};

impl Ui {
    /// A button: a `button` block holding its label, a `.button-label` span.
    /// [`Response::clicked`] is the frame a click or accept fired it.
    ///
    /// `selector` is the widget's `#id.class`; see the widgets module docs.
    #[track_caller]
    pub fn button(&mut self, selector: &str, label: &str) -> Response {
        let selector = typed("button", selector);
        self.block_with(&selector, &[], Behavior::BUTTON, |ui| {
            ui.span(".button-label", label, &[]);
        })
    }

    /// A checkbox editing `value`: a `checkbox` block holding a
    /// `.checkbox-box` block with a `.checkbox-mark` inside it, then a
    /// `.checkbox-label` span. A click or accept flips `value` in the frame it
    /// arrives, and [`Response::changed`] says so; the block is `:checked`
    /// while `value` is true, and the stylesheet decides how the mark shows.
    #[track_caller]
    pub fn checkbox(&mut self, selector: &str, label: &str, value: &mut bool) -> Response {
        let selector = typed("checkbox", selector);
        let parsed = self.node_selector(&selector);
        let key = self.widget_key(parsed, Location::caller());
        let flipped = self.interaction_of(key).clicked && !self.building_disabled();
        if flipped {
            *value = !*value;
        }
        let state = if *value {
            PseudoClasses::CHECKED
        } else {
            PseudoClasses::NONE
        };
        let mut response = self.open_block(key, parsed, &[], Behavior::BUTTON, state, |ui| {
            ui.block(".checkbox-box", &[], |ui| {
                ui.block(".checkbox-mark", &[], |_| {});
            });
            ui.span(".checkbox-label", label, &[]);
        });
        response.changed = flipped;
        response
    }
}
