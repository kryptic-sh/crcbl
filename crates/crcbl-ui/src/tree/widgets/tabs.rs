//! A tab strip and the one pane it shows.
//!
//! The tab that is showing is kept in the store as the **hash of its title**,
//! not as an index, so it survives a rebuild that reorders or inserts tabs —
//! the identity rule the whole tree is built on, applied to a widget's own
//! state. A title the strip no longer holds falls back to the first tab.
//!
//! **Only the showing tab's pane is built at all**, as a closed
//! [`Ui::collapsing`] body is not: the other panes take no layout, draw nothing
//! and cost nothing, and whatever state their nodes kept is dropped with them.
//!
//! Each tab is a [`Behavior::BUTTON`], so focus walks the strip and accept
//! shows a tab — the WAI-ARIA Authoring Practices' **manual activation**, which
//! it recommends wherever showing a panel is expensive, and which here is also
//! the widget set's one rule: a click and accept arrive as the same
//! [`Response::clicked`].

use std::panic::Location;

use super::{Ui, WidgetState, typed};
use crate::style::{Declaration, PseudoClasses};
use crate::tree::{Behavior, FlexDirection, KeySource, NodeKey, Response, hash_of};

impl Ui {
    /// A tab strip and its pane: a `tabs` block holding a `.tab-strip` block of
    /// one `.tab` block per title — each holding a `.tab-label` span, and
    /// `:checked` while it is the one showing — and a `.tab-pane` block that
    /// `pane` fills with the showing tab's index.
    ///
    /// A click or accept on a tab shows it. The first tab shows until one is,
    /// and the tab that is showing is remembered by its title. Returns the
    /// block's [`Response`], with [`Response::changed`] set the frame the
    /// showing tab moved. With no titles nothing but the empty strip is built,
    /// and `pane` is not run.
    #[track_caller]
    pub fn tabs(
        &mut self,
        selector: &str,
        titles: &[&str],
        pane: impl FnOnce(&mut Self, usize),
    ) -> Response {
        let selector = typed("tabs", selector);
        let parsed = self.node_selector(&selector);
        let key = self.widget_key(parsed, Location::caller());

        // The tab the store remembers, by title; the first tab otherwise.
        let kept = match self.widget_state(key) {
            WidgetState::Tabs(title) => Some(title),
            _ => None,
        };
        let showing = kept
            .and_then(|title| titles.iter().position(|&each| hash_of(each) == title))
            .unwrap_or(0);

        let mut chosen = showing;
        let mut response = self.open_block(
            key,
            parsed,
            &[Declaration::FlexDirection(FlexDirection::Column)],
            Behavior::NONE,
            PseudoClasses::NONE,
            |ui| {
                ui.block(".tab-strip", &[], |ui| {
                    // Which tab a click landed on is resolved before any of
                    // them is built, so the strip and the pane agree in the
                    // frame the click arrives rather than a frame later.
                    let keys: Vec<NodeKey> = titles
                        .iter()
                        .map(|&title| ui.key(KeySource::Keyed(hash_of(title))))
                        .collect();
                    if !ui.building_disabled() {
                        for (index, &key) in keys.iter().enumerate() {
                            if ui.interaction_of(key).clicked {
                                chosen = index;
                            }
                        }
                    }
                    for (index, (&title, &key)) in titles.iter().zip(&keys).enumerate() {
                        let state = if index == chosen {
                            PseudoClasses::CHECKED
                        } else {
                            PseudoClasses::NONE
                        };
                        ui.tab(key, title, state);
                    }
                });
                if let Some(&title) = titles.get(chosen) {
                    ui.block_keyed(title, ".tab-pane", &[], |ui| pane(ui, chosen));
                }
            },
        );

        let state = titles.get(chosen).map_or(WidgetState::None, |&title| {
            WidgetState::Tabs(hash_of(title))
        });
        self.set_widget_state(key, state);
        response.changed = chosen != showing;
        response
    }

    /// One tab of a strip on `key`: a `.tab` block holding a `.tab-label` span.
    fn tab(&mut self, key: NodeKey, title: &str, state: PseudoClasses) {
        let parsed = self.node_selector(".tab");
        let key = self.unique(key);
        self.open_block(key, parsed, &[], Behavior::BUTTON, state, |ui| {
            ui.span(".tab-label", title, &[]);
        });
    }
}
