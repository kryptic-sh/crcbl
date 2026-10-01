//! Every property write held to its component's rule: refused when it would
//! leave a value the next load refuses, and reported at the save when one
//! stands anyway.
//!
//! A registered component's [`Validate`](crcbl::registry::Validate) rule —
//! a body's mass above zero, a mesh's asset key inside its root, every
//! rotation unit — is what its chunk is read through, so a value that breaks
//! it saves and is then refused by the next load. `#[reflect(min, max)]` is
//! advisory, and a [`EditCommand::SetProperty`] can write any leaf alone. This
//! module moves the failure to where the edit is made.
//!
//! # Refused at the edit, and put back
//!
//! [`Document::apply`] runs [`Registry::validate`](crcbl::registry::Registry::validate)
//! on the component of every property write a command makes, batches opened,
//! once all of them are applied — so the rotate handle's four leaves, written
//! as one batch, are judged together. A write that leaves the component
//! failing is put back — the whole command, every member of a batch — and
//! refused as [`EditError::Invalid`], naming the entity, the system and the
//! field, which the editor puts on its status line. Refused rather than
//! repaired, for the rule's own reason: a value a file may not hold is a
//! typo, and clamping it would save numbers nobody wrote.
//!
//! The rule is the component's, not the write's: a component that already
//! fails one — written through [`Document::component`] without a command —
//! refuses every write that leaves it failing, including one to another
//! field, until a write puts it right.
//!
//! An undo or a redo is not checked: it puts back a state the log already
//! held, and every state the log holds passed when it was made.
//!
//! **The picking collider is rebuilt after the check**, not as each write
//! lands ([`Document::sync_written`]): a refused value may be no box at all —
//! a block's half extent below zero is one a box collider refuses — and a
//! batch's first leaf alone is a state nobody asked for.
//!
//! # Reported at the save, for what did not come through an edit
//!
//! [`Document::component`] hands a panel the component to write before it
//! reports the write, so a value can stand in the world without a command —
//! the inspector's rewind puts it back, and another caller need not.
//! [`Document::problems`] reads the scene's saved text back through the
//! vocabulary's codecs ([`Registry::problems`](crcbl::registry::Registry::problems)),
//! so such a value is reported by file, line and column — exactly what the
//! next load would refuse.
//!
//! # Not the scene's rules
//!
//! A game's rule over the whole scene — towers' path, whose legs must meet at
//! right angles — is a [`SceneCheck`](crcbl::registry::SceneCheck), reported
//! at the save and never run per edit: authoring passes through layouts the
//! game would refuse, a corner placed before the leg it bends.

use crcbl::scene::scn::SceneEntityId;

use super::{Document, EditError, sync_colliders};
use crate::command::EditCommand;

/// Every property write in `command`, batches opened: whose component, in
/// which system.
fn writes(command: &EditCommand) -> Vec<(SceneEntityId, &str)> {
    match command {
        EditCommand::SetProperty { entity, system, .. } => vec![(*entity, system.as_str())],
        EditCommand::Batch(commands) => commands.iter().flat_map(writes).collect(),
        _ => Vec::new(),
    }
}

impl Document {
    /// The first component `command`'s property writes left failing its rule,
    /// as the refusal [`apply`](Self::apply) gives — or `Ok` for a command
    /// whose every written component passes.
    pub(super) fn validated(&mut self, command: &EditCommand) -> Result<(), EditError> {
        for (entity, system) in writes(command) {
            let held = self.ids.entity(entity).ok_or(EditError::NoEntity(entity))?;
            self.registry
                .validate(&mut self.world, system, held)
                .map_err(|error| EditError::Invalid {
                    entity,
                    system: system.to_owned(),
                    error,
                })?;
        }
        Ok(())
    }

    /// Rebuilds the picking collider of every entity `command`'s property
    /// writes touched — once the whole command stands and has passed
    /// [`validated`](Self::validated), since a value the rule refuses may be
    /// no box a collider can be (a block's half extent below zero).
    pub(super) fn sync_written(&mut self, command: &EditCommand) {
        let entities: Vec<_> = writes(command)
            .into_iter()
            .filter_map(|(entity, _)| self.ids.entity(entity))
            .collect();
        sync_colliders(&self.registry, &mut self.world, entities);
    }
}
