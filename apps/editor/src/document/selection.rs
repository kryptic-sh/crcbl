//! What is selected: an ordered set of entities, the last of them the
//! **primary**.
//!
//! `docs/plan/08-editor.md`'s decision of 2026-10-03. The set is the editor's
//! state, not the scene's: nothing here is recorded in the undo log, and a
//! save writes none of it. What the log does change is which entities exist,
//! so every way an entity leaves the document — a delete, an undo of a spawn,
//! the restore when play stops — drops it from the set, and a set can never
//! name a hole.
//!
//! # Order, and the primary
//!
//! The set keeps the order entities joined it, so the primary is the last one
//! a click added: what the inspector edits, F2 renames and a play action
//! naming a picked entity takes. A [`toggle`](Document::toggle_selected) that
//! takes the primary out hands the role to the one that joined before it.
//!
//! # The pivot
//!
//! [`selection_pivot`](Document::selection_pivot) is the **bounds centre**:
//! the centre of the smallest world-axis box holding every selected entity's
//! own box. The translate gizmo stands there, and a shared-pivot drag moves
//! it — not the primary's centre, which would put the handles at one edge of
//! a wide selection, and not the mean of the centres, which a cluster at one
//! end drags away from the middle of what is drawn.

use crcbl::math::DVec3;
use crcbl::scene::scn::SceneEntityId;

use super::Document;

impl Document {
    /// Every selected entity, in the order they joined the set: the primary
    /// last. Empty with nothing selected.
    #[must_use]
    pub fn selection(&self) -> &[SceneEntityId] {
        &self.selection
    }

    /// The primary: the selected entity the last click added, or [`None`]
    /// with nothing selected.
    #[must_use]
    pub fn primary(&self) -> Option<SceneEntityId> {
        self.selection.last().copied()
    }

    /// Whether `id` is one of the selected entities.
    #[must_use]
    pub fn is_selected(&self, id: SceneEntityId) -> bool {
        self.selection.contains(&id)
    }

    /// Selects `id` alone — what a plain click does — or clears the selection
    /// with [`None`].
    ///
    /// An id the document does not hold selects nothing, so a stale id from
    /// before a reload cannot leave a selection pointing at a hole.
    pub fn select(&mut self, id: Option<SceneEntityId>) {
        self.set_selection(id);
    }

    /// Takes `id` out of the selection if it is in it, or adds it as the
    /// primary if it is not — what Ctrl and a click do. An id the document
    /// does not hold changes nothing.
    pub fn toggle_selected(&mut self, id: SceneEntityId) {
        if self.ids.entity(id).is_none() {
            return;
        }
        match self.selection.iter().position(|each| *each == id) {
            Some(at) => {
                self.selection.remove(at);
            }
            None => self.selection.push(id),
        }
    }

    /// Selects every id of `ids`, in that order, the last as the primary.
    ///
    /// An id the document does not hold is passed over, and a repeated one
    /// keeps its first place, so the set holds each live entity once.
    pub fn set_selection(&mut self, ids: impl IntoIterator<Item = SceneEntityId>) {
        self.selection.clear();
        for id in ids {
            if self.ids.entity(id).is_some() && !self.selection.contains(&id) {
                self.selection.push(id);
            }
        }
    }

    /// The bounds centre of the selection, in simulation space — see the
    /// module docs — or [`None`] when nothing selected is placed in space.
    ///
    /// An entity nothing places, a sun, adds nothing to the box and does not
    /// move with it.
    #[must_use]
    pub fn selection_pivot(&mut self) -> Option<DVec3> {
        let mut reach: Option<(DVec3, DVec3)> = None;
        for index in 0..self.selection.len() {
            let Some((min, max)) = self
                .placement(self.selection[index])
                .map(|placement| placement.bounds())
            else {
                continue;
            };
            reach = Some(match reach {
                Some((low, high)) => (low.min(min), high.max(max)),
                None => (min, max),
            });
        }
        reach.map(|(min, max)| (min + max) * 0.5)
    }

    /// Takes every id the document no longer holds out of the selection,
    /// keeping the order of the rest — after anything that can take an entity
    /// away.
    pub(super) fn prune_selection(&mut self) {
        let ids = &self.ids;
        self.selection.retain(|id| ids.entity(*id).is_some());
    }
}
