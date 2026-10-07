//! Retained selection, expansion and click state for an outliner.

use std::collections::HashSet;
use std::time::Duration;

use glam::Vec2;

use super::{OutlinerBuilder, OutlinerId, OutlinerRow, SelectMode};

/// An outliner's expansion, selection and flattened rows: the application's to
/// keep, and the thing a saved outliner is saved from. See the module docs.
#[derive(Clone, Debug)]
pub struct OutlinerState {
    expanded: HashSet<OutlinerId>,
    selected: HashSet<OutlinerId>,
    rows: Vec<OutlinerRow>,
    /// Where a [`SelectMode::Range`] runs from.
    anchor: Option<OutlinerId>,
    /// Whether the rows must be flattened again before they are read.
    stale: bool,
    /// The last click on a row that did not complete a double-click: which
    /// row, when on the tree's text clock, and where.
    pub(super) last_click: Option<(OutlinerId, Duration, Vec2)>,
    /// The row this frame's click made a double-click of.
    pub(super) double_clicked: Option<OutlinerId>,
}

impl Default for OutlinerState {
    fn default() -> Self {
        Self::new()
    }
}

impl OutlinerState {
    /// Nothing expanded, nothing selected, and the rows still to be flattened.
    #[must_use]
    pub fn new() -> Self {
        Self {
            expanded: HashSet::new(),
            selected: HashSet::new(),
            rows: Vec::new(),
            anchor: None,
            stale: true,
            last_click: None,
            double_clicked: None,
        }
    }

    /// The row a click completed a double-click on during the last
    /// [`Ui::outliner`](crate::tree::Ui::outliner) call, if one did.
    #[must_use]
    pub const fn double_clicked(&self) -> Option<OutlinerId> {
        self.double_clicked
    }

    /// The flattened visible rows, in the order they are shown.
    #[must_use]
    pub fn rows(&self) -> &[OutlinerRow] {
        &self.rows
    }

    /// Whether the rows must be flattened again: set by every change to the
    /// expansion and by [`Self::invalidate`], cleared by [`Self::flatten`].
    #[must_use]
    pub const fn is_stale(&self) -> bool {
        self.stale
    }

    /// Says the tree the rows were flattened from has changed, so that the next
    /// [`Ui::outliner`](crate::tree::Ui::outliner) flattens it again.
    pub const fn invalidate(&mut self) {
        self.stale = true;
    }

    /// Forgets state for items the application's model no longer contains.
    ///
    /// `keep` must include existing items hidden under collapsed branches:
    /// the visible rows alone cannot distinguish hidden items from deleted
    /// ones. Expansion, selection, the range anchor and click history survive
    /// only for retained items. The cached rows are cleared and made stale,
    /// so the next outliner build flattens the updated model.
    pub fn retain(&mut self, keep: impl Fn(OutlinerId) -> bool) {
        self.expanded.retain(|&id| keep(id));
        self.selected.retain(|&id| keep(id));
        self.anchor = self.anchor.filter(|&id| keep(id));
        self.last_click = self.last_click.filter(|&(id, _, _)| keep(id));
        self.double_clicked = self.double_clicked.filter(|&id| keep(id));
        self.rows.clear();
        self.invalidate();
    }

    /// Flattens the rows again: `build` walks the caller's tree, pushing a row
    /// per visible item, and a branch's children are walked only while it is
    /// expanded. Clears [`Self::is_stale`].
    pub fn flatten(&mut self, build: impl FnOnce(&mut OutlinerBuilder<'_>)) {
        self.rows.clear();
        self.stale = false;
        let mut builder = OutlinerBuilder {
            rows: &mut self.rows,
            expanded: &self.expanded,
            depth: 0,
            parent: None,
        };
        build(&mut builder);
    }

    /// Whether `id`'s children are shown.
    #[must_use]
    pub fn is_expanded(&self, id: OutlinerId) -> bool {
        self.expanded.contains(&id)
    }

    /// Shows or hides `id`'s children. Returns whether that changed anything,
    /// and makes the rows stale when it did.
    pub fn set_expanded(&mut self, id: OutlinerId, expanded: bool) -> bool {
        let moved = if expanded {
            self.expanded.insert(id)
        } else {
            self.expanded.remove(&id)
        };
        self.stale |= moved;
        moved
    }

    /// [`Self::set_expanded`] to the opposite of what `id` is now; returns what
    /// it became.
    pub fn toggle_expanded(&mut self, id: OutlinerId) -> bool {
        let expanded = !self.is_expanded(id);
        self.set_expanded(id, expanded);
        expanded
    }

    /// Whether `id` is selected.
    #[must_use]
    pub fn is_selected(&self, id: OutlinerId) -> bool {
        self.selected.contains(&id)
    }

    /// How many items are selected.
    #[must_use]
    pub fn selected_len(&self) -> usize {
        self.selected.len()
    }

    /// The selected items, in the order the flattened rows show them; an item
    /// selected while a collapsed branch hides it is not among them.
    pub fn selected(&self) -> impl Iterator<Item = OutlinerId> + '_ {
        self.rows
            .iter()
            .map(|row| row.id)
            .filter(|id| self.selected.contains(id))
    }

    /// Where a [`SelectMode::Range`] runs from: the row last selected outright.
    #[must_use]
    pub const fn anchor(&self) -> Option<OutlinerId> {
        self.anchor
    }

    /// Selects nothing, and forgets the anchor.
    pub fn clear_selection(&mut self) {
        self.selected.clear();
        self.anchor = None;
    }

    /// Acts on `id` as `mode` says; see [`SelectMode`]. Returns whether the
    /// selection changed.
    ///
    /// A [`SelectMode::Range`] walks the flattened rows to find the anchor, so
    /// it costs the model's length; the other two are constant.
    pub fn select(&mut self, id: OutlinerId, mode: SelectMode) -> bool {
        match mode {
            SelectMode::Replace => {
                let already = self.selected.len() == 1 && self.selected.contains(&id);
                self.selected.clear();
                self.selected.insert(id);
                self.anchor = Some(id);
                !already
            }
            SelectMode::Toggle => {
                if !self.selected.remove(&id) {
                    self.selected.insert(id);
                }
                self.anchor = Some(id);
                true
            }
            SelectMode::Range => {
                let Some(anchor) = self.anchor else {
                    return self.select(id, SelectMode::Replace);
                };
                let from = self.rows.iter().position(|row| row.id == anchor);
                let to = self.rows.iter().position(|row| row.id == id);
                let (Some(from), Some(to)) = (from, to) else {
                    return self.select(id, SelectMode::Replace);
                };
                let (from, to) = (from.min(to), from.max(to));
                let wanted: HashSet<OutlinerId> =
                    self.rows[from..=to].iter().map(|row| row.id).collect();
                let moved = wanted != self.selected;
                self.selected = wanted;
                moved
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn retaining_the_model_forgets_deleted_items_but_keeps_hidden_ones() {
        let mut state = OutlinerState::new();
        state.set_expanded(OutlinerId(2), true);
        state.set_expanded(OutlinerId(9), true);
        state.select(OutlinerId(3), SelectMode::Toggle);
        state.select(OutlinerId(4), SelectMode::Toggle);
        state.last_click = Some((OutlinerId(9), Duration::ZERO, Vec2::ZERO));
        state.double_clicked = Some(OutlinerId(4));
        state.flatten(|out| {
            out.branch(OutlinerId(0), |_| panic!("the root is collapsed"));
        });

        state.retain(|id| matches!(id.0, 0..=3 | 5));

        assert!(state.is_expanded(OutlinerId(2)));
        assert!(!state.is_expanded(OutlinerId(9)));
        assert!(state.is_selected(OutlinerId(3)));
        assert_eq!(state.selected_len(), 1);
        assert_eq!(state.anchor(), None);
        assert_eq!(state.double_clicked(), None);
        assert_eq!(state.last_click, None);
        assert!(state.rows().is_empty());
        assert!(state.is_stale());

        state.set_expanded(OutlinerId(0), true);
        state.flatten(|out| {
            out.branch(OutlinerId(0), |out| {
                out.leaf(OutlinerId(1));
                out.branch(OutlinerId(2), |out| out.leaf(OutlinerId(3)));
                out.leaf(OutlinerId(5));
            });
        });
        assert_eq!(state.selected().collect::<Vec<_>>(), [OutlinerId(3)]);
        state.select(OutlinerId(5), SelectMode::Range);
        assert_eq!(state.selected().collect::<Vec<_>>(), [OutlinerId(5)]);
    }

    #[test]
    fn retaining_live_items_preserves_range_anchor_and_click_history() {
        let mut state = OutlinerState::new();
        let retained = OutlinerId(1);
        state.select(retained, SelectMode::Replace);
        let click = (retained, Duration::from_millis(50), Vec2::new(2.0, 3.0));
        state.last_click = Some(click);
        state.double_clicked = Some(retained);

        state.retain(|id| id == retained);

        assert!(state.is_selected(retained));
        assert_eq!(state.anchor(), Some(retained));
        assert_eq!(state.last_click, Some(click));
        assert_eq!(state.double_clicked(), Some(retained));
    }
}
