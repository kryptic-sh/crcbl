//! A document whose edits go to a server: a client's copy of a served scene,
//! which applies only what the server's notices bring.
//!
//! # Held, not applied (decided 2026-10-05)
//!
//! While routed, the four ways an edit reaches the history —
//! [`Document::apply`], [`Document::apply_in`], [`Document::undo`] and
//! [`Document::redo`] — hold the operation for the caller to send
//! ([`Document::take_routed`]) and change nothing. Every edit the document
//! makes, a panel's field and a rename and a paste alike, ends in one of
//! them, so a caller written against an editing document routes unchanged.
//! The copy changes only when a notice applies — the
//! [`SceneFollower`](super::SceneFollower) applying it to an unrouted
//! document — so it is the server's scene and nothing else, and needs no
//! reconciling: an edit refused there was never shown here.
//!
//! The price is a round trip: an edit shows once its notice comes back, so
//! a drag trails the pointer by the link's latency. Applying first and
//! reconciling on the notice would hide that, at the cost of undoing local
//! edits whenever another client's notice landed between them, and of a
//! history that no longer folds exactly as the server's.
//!
//! **Nothing is validated here**: the server applies the operation through
//! its own document and refuses what that refuses, so a second check would
//! only be a second set of rules. Play mode is still refused here, as it is
//! by an editing document: a copy that plays is no copy of the server's.

use crate::scene::edit::{EditCommand, EditOp, Gesture};

use super::Document;

/// An operation a routed document held for its server instead of applying
/// it — see the module docs.
#[derive(Clone, Debug, PartialEq)]
pub struct RoutedEdit {
    /// What was asked: a command, an undo or a redo.
    pub op: EditOp,
    /// The gesture a command was asked in ([`Document::apply_in`]) — the
    /// document's own number, which the caller maps to the one it sends —
    /// or [`None`].
    pub gesture: Option<Gesture>,
}

impl Document {
    /// Routes this document's edits from now on: they are held for
    /// [`take_routed`](Self::take_routed) and not applied — see the module
    /// docs. Routing a routed document changes nothing.
    pub fn route_edits(&mut self) {
        self.routed.get_or_insert_with(Vec::new);
    }

    /// Stops routing, so edits apply here again, and hands back what was
    /// held and not yet taken.
    pub fn stop_routing(&mut self) -> Vec<RoutedEdit> {
        self.routed.take().unwrap_or_default()
    }

    /// Whether this document's edits are routed — see the module docs.
    #[must_use]
    pub const fn is_routed(&self) -> bool {
        self.routed.is_some()
    }

    /// The edits held since the last call, oldest first; none for a document
    /// that is not routed.
    pub fn take_routed(&mut self) -> Vec<RoutedEdit> {
        self.routed.as_mut().map(std::mem::take).unwrap_or_default()
    }

    /// Forgets the state the document was last saved at, so it reads unsaved
    /// until it is saved somewhere: what a copy of a served scene becomes
    /// once the server is gone and the copy is all this program holds of it.
    /// The history goes too, as a recovery copy's does: it was the server's,
    /// stepped by notices.
    pub fn forget_saved(&mut self) {
        self.log = crate::scene::edit::UndoLog::new();
    }

    /// Holds `command` for the server when routed, or hands it back to
    /// apply here.
    pub(super) fn route(
        &mut self,
        command: EditCommand,
        gesture: Option<Gesture>,
    ) -> Option<EditCommand> {
        let Some(held) = self.routed.as_mut() else {
            return Some(command);
        };
        held.push(RoutedEdit {
            op: EditOp::Apply(command),
            gesture,
        });
        None
    }

    /// Holds a step of the history — [`EditOp::Undo`] or [`EditOp::Redo`] —
    /// for the server when routed, handing back whether it did.
    pub(super) fn route_step(&mut self, step: EditOp) -> bool {
        let Some(held) = self.routed.as_mut() else {
            return false;
        };
        held.push(RoutedEdit {
            op: step,
            gesture: None,
        });
        true
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::reflect::Value;
    use crate::scene::scn::SceneEntityId;

    use super::super::tests::{BLOCKS, one_block};

    /// Block 0 moved along x to `x`.
    fn shift(x: f64) -> EditCommand {
        EditCommand::SetProperty {
            entity: SceneEntityId(0),
            system: BLOCKS.to_owned(),
            path: "position.0".to_owned(),
            value: Value::Float(x),
        }
    }

    /// **A routed document holds every edit and changes nothing**: a
    /// command, one in a gesture, a paste that ends in a command, an undo
    /// and a redo all come back from `take_routed` in order, the scene and
    /// its history as they were; and once routing stops, an edit applies
    /// again.
    #[test]
    fn a_routed_document_holds_every_edit_and_changes_nothing() {
        let mut document = one_block();
        let before = document.files().expect("saves");
        document.route_edits();
        assert!(document.is_routed());

        document.apply(shift(1.0)).expect("held");
        let gesture = document.begin_gesture();
        document.apply_in(shift(2.0), gesture).expect("held");
        assert!(document.undo().expect("held"), "an undo is asked");
        assert!(document.redo().expect("held"), "a redo is asked");
        document.delete(&[SceneEntityId(0)]).expect("held");

        assert_eq!(document.files().expect("saves"), before);
        assert!(document.log().is_empty(), "nothing was recorded");
        let held = document.take_routed();
        assert_eq!(
            held,
            vec![
                RoutedEdit {
                    op: EditOp::Apply(shift(1.0)),
                    gesture: None,
                },
                RoutedEdit {
                    op: EditOp::Apply(shift(2.0)),
                    gesture: Some(gesture),
                },
                RoutedEdit {
                    op: EditOp::Undo,
                    gesture: None,
                },
                RoutedEdit {
                    op: EditOp::Redo,
                    gesture: None,
                },
                RoutedEdit {
                    op: EditOp::Apply(EditCommand::Delete {
                        entity: SceneEntityId(0),
                    }),
                    gesture: None,
                },
            ]
        );
        assert!(document.take_routed().is_empty(), "taken once");

        document.apply(shift(3.0)).expect("held");
        assert_eq!(document.stop_routing().len(), 1, "handed back on stop");
        assert!(!document.is_routed());
        document.apply(shift(4.0)).expect("applies");
        assert_eq!(document.log().len(), 1);
        assert_ne!(document.files().expect("saves"), before);
    }

    /// **A copy whose server is gone reads unsaved**, so closing or opening
    /// over it asks first.
    #[test]
    fn a_document_that_forgot_its_save_reads_unsaved() {
        let mut document = one_block();
        assert!(!document.is_dirty());
        document.forget_saved();
        assert!(document.is_dirty());
    }
}
