//! Each rigged asset's ragdoll rig edits, undone and redone **apart from the
//! scene's history**.
//!
//! The rig is the asset's file and the history Ctrl+Z walks is the scene's —
//! the served scene's, in a joined editor — so one key stepping whichever of
//! the two changed last would undo a local file one press and a shared scene
//! the next. A rig steps through the Undo and Redo of its own inspector
//! section instead (`crate::panel::RigEdit`).
//!
//! A step is a rig as it was written. The rig before an edit is held from the
//! edit's first frame until the file is written — a drag is one step however
//! many frames it moved, and an edit the rig refused is no step — and only
//! then is it pushed. Stepping back or forward puts that rig in place to be
//! resolved and written, and an unwritten edit is dropped by it: the step
//! taken from it is the rig last written, never one that did not resolve.
//! The history lasts as long as the editor: it is not the file's, and an
//! edit made to the file outside the editor is stepped over, not recorded.

use std::collections::HashMap;

use crcbl::ragdoll::RagdollDesc;

/// Every rigged asset's steps — see the [module docs](self).
#[derive(Debug, Default)]
pub(super) struct RigHistory {
    steps: HashMap<String, Steps>,
    /// The asset whose rig an edit is changing, and the rig as it was last
    /// written, from the edit's first frame until its file is written.
    open: Option<(String, RagdollDesc)>,
}

/// One asset's rigs before and after the present one.
#[derive(Debug, Default)]
struct Steps {
    undo: Vec<RagdollDesc>,
    redo: Vec<RagdollDesc>,
}

impl RigHistory {
    /// `asset`'s rig was edited this frame, and was `before` going into it:
    /// held as the step back if no edit of `asset` already holds one.
    pub(super) fn edited(&mut self, asset: &str, before: &RagdollDesc) {
        if self.open.as_ref().is_some_and(|(open, _)| open == asset) {
            return;
        }
        self.open = Some((asset.to_owned(), before.clone()));
    }

    /// `asset`'s edited rig was written: the rig before the edit becomes the
    /// step back, and what was undone can no longer be redone.
    pub(super) fn written(&mut self, asset: &str) {
        let Some((open, before)) = self.open.take() else {
            return;
        };
        if open != asset {
            self.open = Some((open, before));
            return;
        }
        let steps = self.steps.entry(open).or_default();
        steps.undo.push(before);
        steps.redo.clear();
    }

    /// The rig to put in place of `asset`'s `present` one a step back, if
    /// there is a step; `present` becomes the step forward.
    pub(super) fn undo(&mut self, asset: &str, present: &RagdollDesc) -> Option<RagdollDesc> {
        let present = self.last_written(asset, present);
        let steps = self.steps.get_mut(asset)?;
        let back = steps.undo.pop()?;
        steps.redo.push(present);
        Some(back)
    }

    /// The rig to put in place of `asset`'s `present` one a step forward, if
    /// one was undone; `present` becomes the step back.
    pub(super) fn redo(&mut self, asset: &str, present: &RagdollDesc) -> Option<RagdollDesc> {
        let present = self.last_written(asset, present);
        let steps = self.steps.get_mut(asset)?;
        let forward = steps.redo.pop()?;
        steps.undo.push(present);
        Some(forward)
    }

    /// Whether `asset` has a step back.
    pub(super) fn can_undo(&self, asset: &str) -> bool {
        self.steps
            .get(asset)
            .is_some_and(|steps| !steps.undo.is_empty())
    }

    /// Whether `asset` has a step forward.
    pub(super) fn can_redo(&self, asset: &str) -> bool {
        self.steps
            .get(asset)
            .is_some_and(|steps| !steps.redo.is_empty())
    }

    /// `asset`'s rig as last written: the one an unwritten edit started from,
    /// which a step drops, or else `present`.
    fn last_written(&mut self, asset: &str, present: &RagdollDesc) -> RagdollDesc {
        match self.open.take() {
            Some((open, before)) if open == asset => before,
            other => {
                self.open = other;
                present.clone()
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const ASSET: &str = "props/rig.glb";

    /// A rig told apart from the others by its substeps alone.
    fn rig(substeps: u32) -> RagdollDesc {
        RagdollDesc {
            bodies: Vec::new(),
            joints: Vec::new(),
            ignore: Vec::new(),
            ignore_rest_overlaps: true,
            substeps,
        }
    }

    /// **An edit is one step however many frames it moved**: the rig before
    /// its first frame is the step back, and undoing then redoing walks back
    /// to it and forward again.
    #[test]
    fn a_written_edit_is_one_step() {
        let mut history = RigHistory::default();
        assert!(!history.can_undo(ASSET));
        history.edited(ASSET, &rig(0));
        history.edited(ASSET, &rig(1));
        history.written(ASSET);
        assert!(history.can_undo(ASSET));
        assert!(!history.can_redo(ASSET));

        assert_eq!(history.undo(ASSET, &rig(2)), Some(rig(0)));
        assert!(!history.can_undo(ASSET));
        assert_eq!(history.redo(ASSET, &rig(0)), Some(rig(2)));
        assert_eq!(history.undo(ASSET, &rig(2)), Some(rig(0)));
    }

    /// **An edit never written is no step, and a step drops it**: the step
    /// forward is the rig last written, not the one that did not resolve.
    #[test]
    fn an_unwritten_edit_is_dropped_by_a_step() {
        let mut history = RigHistory::default();
        history.edited(ASSET, &rig(0));
        history.written(ASSET);
        history.edited(ASSET, &rig(1));
        assert_eq!(history.undo(ASSET, &rig(9)), Some(rig(0)));
        assert_eq!(history.redo(ASSET, &rig(0)), Some(rig(1)));
    }

    /// **A new edit written after an undo ends what could be redone**, and
    /// one asset's steps are not another's.
    #[test]
    fn a_new_edit_ends_the_redo_and_assets_keep_their_own() {
        let mut history = RigHistory::default();
        history.edited(ASSET, &rig(0));
        history.written(ASSET);
        assert_eq!(history.undo(ASSET, &rig(1)), Some(rig(0)));
        assert!(history.can_redo(ASSET));
        history.edited(ASSET, &rig(0));
        history.written(ASSET);
        assert!(!history.can_redo(ASSET));
        assert!(!history.can_undo("props/other.glb"));
        assert_eq!(history.undo("props/other.glb", &rig(0)), None);
    }
}
