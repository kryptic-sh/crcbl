//! Every edit the editor makes, as a value — and the log that walks them back.
//!
//! **This is the decision of 2026-09-16**, recorded in
//! `docs/plan/08-editor.md`: the command enum and the undo log exist from day
//! one, applied in-process, and are routed over the transport later. So no edit
//! in this crate is a component field written at the call site: a key press
//! builds an [`EditCommand`], hands it to [`crate::Document::apply`], and the
//! log gets the pair back. What the transport gains later is a carrier, not a
//! vocabulary.
//!
//! An inspector is the one thing that arrives the other way round — the widget
//! writes the field and *reports* what it wrote — and
//! [`crate::Document::record_edit`] is where that is turned back into a
//! command. It is the crate's only field write, and it is a rewind: what the
//! panel did is undone so the command can do it, and so record an exact
//! inverse. That method's docs say why.
//!
//! # The shape of an edit
//!
//! A command names **the entity, the dotted path and the new value** — exactly
//! what [`crcbl::reflect::set_path`] takes, which is why that crate's
//! [`Value`] is the payload rather than a second one declared here. Applying a
//! command reads the leaf first and hands back the command that puts it back:
//! an inverse is produced, never derived later from a rule that could be wrong.
//!
//! ```
//! use crcbl::reflect::{Reflect, Value};
//! use crcbl::scene::scn::SceneEntityId;
//! use crcbl_editor::command::EditCommand;
//!
//! #[derive(Reflect)]
//! #[reflect(crate = "crcbl::reflect")]
//! struct Brick {
//!     position: [f64; 3],
//! }
//!
//! let mut brick = Brick { position: [1.0, 2.0, 3.0] };
//! let set = EditCommand::SetProperty {
//!     entity: SceneEntityId(7),
//!     path: "position.1".to_owned(),
//!     value: Value::Float(9.0),
//! };
//!
//! let inverse = set.apply(&mut brick).expect("a brick has a y");
//! assert_eq!(brick.position, [1.0, 9.0, 3.0]);
//! inverse.apply(&mut brick).expect("and it still has one");
//! assert_eq!(brick.position, [1.0, 2.0, 3.0]);
//! ```
//!
//! # What slice 1 does not have
//!
//! One variant, because one variant is what this slice issues. The plan's task
//! 4 lists about ten for the MVP — spawn, delete, duplicate, rename,
//! attach/detach system data, scene load/save markers — and each of them needs
//! something the tree does not have yet: `IdMap` has no removal, the scene
//! format cannot hold one entity in two systems, and an entity has no name to
//! rename. Adding an arm that no key press produces would be an inverse nothing
//! could show was right, which is the failure mode the plan's own property test
//! exists to catch.
//!
//! A **transform** command is not a second variant either, and that is not an
//! omission: a brick's placement *is* `position`, a field its `#[derive(Reflect)]`
//! describes, so a nudge is a [`SetProperty`](EditCommand::SetProperty) on
//! `position.0`. A transform arm arrives when something carries a transform
//! that is not a reflected field — `crcbl::phys::Transform` on an entity with no
//! component holding it.
//!
//! [`Value`]: crcbl::reflect::Value

use crcbl::reflect::{PathError, Reflect, Value, get_path, set_path};
use crcbl::scene::scn::SceneEntityId;

/// One undoable edit, as a value that could be sent rather than performed.
///
/// [`SceneEntityId`] and not [`crcbl::ecs::Entity`]: a command outlives the
/// application of it, and an `Entity`'s bits are a function of spawn history —
/// the file's id is the one both ends of a transport could agree on, and the
/// one an undo entry can still name after a reload.
#[derive(Clone, Debug, PartialEq)]
pub enum EditCommand {
    /// Write `value` into the leaf `path` names inside `entity`'s component.
    SetProperty {
        /// Whose component: the id the scene file spells.
        entity: SceneEntityId,
        /// The dotted path, in [`crcbl::reflect`]'s grammar — `"position.1"`
        /// is a brick's `y`.
        path: String,
        /// What the leaf is being set to.
        value: Value,
    },
}

impl EditCommand {
    /// Which entity this command is about.
    #[must_use]
    pub const fn entity(&self) -> SceneEntityId {
        match self {
            Self::SetProperty { entity, .. } => *entity,
        }
    }

    /// Applies this command to `component` — the entity's component, which the
    /// caller has already resolved — and hands back the command that undoes it.
    ///
    /// The inverse carries **the value that was replaced**, read out of the
    /// component immediately before the write. So an undo restores the bits
    /// that were there rather than a value computed from the command, which is
    /// what makes a round trip byte-for-byte on floats.
    ///
    /// # Errors
    ///
    /// [`PathError`] if the path names nothing in this component, stops at
    /// something that is not a leaf, or reaches a leaf that refuses the value.
    /// Nothing is written when it does — [`get_path`] runs first, and
    /// [`crcbl::reflect::Reflect::set`] leaves a refused leaf untouched.
    pub fn apply(&self, component: &mut dyn Reflect) -> Result<Self, PathError> {
        match self {
            Self::SetProperty {
                entity,
                path,
                value,
            } => {
                let replaced = get_path(component, path)?;
                set_path(component, path, value)?;
                Ok(Self::SetProperty {
                    entity: *entity,
                    path: path.clone(),
                    value: replaced,
                })
            }
        }
    }
}

/// The document's history: what was done, what puts each one back, and where in
/// the list the world currently stands.
///
/// **One log, not one per client** — `docs/plan/08-editor.md`'s 2026-07-27
/// correction fixes that for the multi-client case, and starting with anything
/// else would be a shape to undo later. There is nothing to attribute yet
/// because there is one author; the author column arrives with the transport.
///
/// # Position, and why it is not a stack
///
/// [`position`](Self::position) is how many entries are applied. An undo moves
/// it down and a redo moves it up; nothing is dropped until a **new** command
/// arrives, which truncates the entries above it — the ordinary editor rule,
/// and the reason a redo survives an undo but not an edit.
///
/// It is also the whole of the dirty marker: [`crate::Document`] remembers the
/// position it last saved at, so undoing back to it is clean again. A flag
/// would say "dirty" forever.
#[derive(Debug, Default)]
pub struct UndoLog {
    entries: Vec<Entry>,
    position: usize,
}

/// One applied command and the command that puts it back.
#[derive(Debug)]
struct Entry {
    done: EditCommand,
    undo: EditCommand,
}

impl UndoLog {
    /// An empty log, standing at position 0.
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// Records a command that has just been applied, and the inverse
    /// [`EditCommand::apply`] handed back.
    ///
    /// Anything above the current position is dropped: it described a future
    /// that this command has replaced.
    pub fn record(&mut self, done: EditCommand, undo: EditCommand) {
        self.entries.truncate(self.position);
        self.entries.push(Entry { done, undo });
        self.position = self.entries.len();
    }

    /// The command that undoes the most recent applied entry, and steps back
    /// over it — or [`None`] at the bottom of the log.
    ///
    /// The caller applies what comes back and does **not** record it: the entry
    /// it came from already holds both halves.
    pub fn undo(&mut self) -> Option<EditCommand> {
        let entry = self.entries.get(self.position.checked_sub(1)?)?;
        let command = entry.undo.clone();
        self.position -= 1;
        Some(command)
    }

    /// The command that re-applies the entry just above the position, and steps
    /// over it — or [`None`] at the top.
    pub fn redo(&mut self) -> Option<EditCommand> {
        let entry = self.entries.get(self.position)?;
        let command = entry.done.clone();
        self.position += 1;
        Some(command)
    }

    /// How many entries are currently applied.
    #[must_use]
    pub const fn position(&self) -> usize {
        self.position
    }

    /// How many entries the log holds, applied or not.
    #[must_use]
    pub fn len(&self) -> usize {
        self.entries.len()
    }

    /// Whether the log holds nothing at all.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }

    /// The applied entries, oldest first — the order they were applied in, and
    /// the order a replay would take.
    pub fn applied(&self) -> impl Iterator<Item = &EditCommand> {
        self.entries[..self.position]
            .iter()
            .map(|entry| &entry.done)
    }
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    use crcbl::reflect::Reflect;

    #[derive(Debug, PartialEq, Reflect)]
    #[reflect(crate = "crcbl::reflect")]
    struct Brick {
        position: [f64; 3],
        half_extents: [f64; 3],
    }

    fn brick() -> Brick {
        Brick {
            position: [-11.8, 7.0, 0.0],
            half_extents: [1.2, 0.4, 0.5],
        }
    }

    fn set(path: &str, value: Value) -> EditCommand {
        EditCommand::SetProperty {
            entity: SceneEntityId(3),
            path: path.to_owned(),
            value,
        }
    }

    /// **A command moves exactly the field its path names**, and nothing else.
    ///
    /// The assertion is on the *whole* component rather than on the one field:
    /// a write that landed in a neighbouring leaf would satisfy "the y is 9"
    /// just as well if the y were read back through the same wrong path.
    #[test]
    fn a_command_moves_exactly_the_field_its_path_names() {
        let mut value = brick();
        set("position.1", Value::Float(9.0))
            .apply(&mut value)
            .expect("a brick has a y");
        assert_eq!(
            value,
            Brick {
                position: [-11.8, 9.0, 0.0],
                half_extents: [1.2, 0.4, 0.5],
            },
        );
    }

    /// **The inverse restores the value byte for byte.**
    ///
    /// `to_bits`, not `==`: the point of carrying the replaced [`Value`] rather
    /// than recomputing one is that a float comes back as the same float, and
    /// `-11.8 + 0.1 - 0.1` is a different one that compares equal to nothing
    /// useful.
    #[test]
    fn the_inverse_restores_the_replaced_value_byte_for_byte() {
        let mut value = brick();
        let before = value.position;
        let mut undo = Vec::new();
        for (path, to) in [
            ("position.0", -11.7),
            ("position.1", 7.25),
            ("position.2", 0.5),
        ] {
            undo.push(
                set(path, Value::Float(to))
                    .apply(&mut value)
                    .expect("a brick has that axis"),
            );
        }
        assert_ne!(value.position.map(f64::to_bits), before.map(f64::to_bits));
        for command in undo.iter().rev() {
            command.apply(&mut value).expect("and it still has it");
        }
        assert_eq!(
            value.position.map(f64::to_bits),
            before.map(f64::to_bits),
            "the inverses did not restore the bits that were there",
        );
    }

    /// A path that names nothing writes nothing and says which segment it is
    /// about.
    #[test]
    fn a_path_that_names_nothing_is_refused_and_writes_nothing() {
        let mut value = brick();
        let error = set("rotation.0", Value::Float(1.0))
            .apply(&mut value)
            .expect_err("a brick has no rotation");
        assert!(
            matches!(&error, PathError::NoField { segment, .. } if segment == "rotation"),
            "{error}",
        );
        assert_eq!(value, brick(), "a refused command must leave the component");
    }

    /// A value of the wrong kind is refused by the leaf, and the component is
    /// left alone.
    #[test]
    fn a_value_of_the_wrong_kind_is_refused_and_writes_nothing() {
        let mut value = brick();
        let error = set("position.1", Value::Text("up a bit".to_owned()))
            .apply(&mut value)
            .expect_err("a float leaf takes no text");
        assert!(matches!(error, PathError::Set(_)), "{error}");
        assert_eq!(value, brick());
    }

    /// **The log replays in order**, which is the claim an undo walk depends
    /// on: entry *n*'s inverse is only correct if entries *n+1..* have already
    /// been undone.
    #[test]
    fn the_log_replays_in_the_order_the_commands_were_applied() {
        let mut value = brick();
        let mut log = UndoLog::new();
        for to in [1.0, 2.0, 3.0] {
            let command = set("position.0", Value::Float(to));
            let undo = command.apply(&mut value).expect("a brick has an x");
            log.record(command, undo);
        }
        assert_eq!(
            log.applied()
                .map(|command| match command {
                    EditCommand::SetProperty { value, .. } => value.clone(),
                })
                .collect::<Vec<_>>(),
            vec![Value::Float(1.0), Value::Float(2.0), Value::Float(3.0)],
        );
        // Walking the whole log back lands on the value the first command
        // replaced, which is only true if the inverses ran newest-first.
        while let Some(command) = log.undo() {
            command.apply(&mut value).expect("a brick has an x");
        }
        assert_eq!(value, brick());
        assert_eq!(log.position(), 0);
        assert_eq!(log.len(), 3, "an undo keeps the entry, for the redo");
    }

    /// Undo and redo walk the same entries in both directions, and the
    /// position is what says where the world stands.
    #[test]
    fn redo_walks_back_up_the_entries_undo_walked_down() {
        let mut value = brick();
        let mut log = UndoLog::new();
        for to in [1.0, 2.0] {
            let command = set("position.0", Value::Float(to));
            let undo = command.apply(&mut value).expect("a brick has an x");
            log.record(command, undo);
        }
        let command = log.undo().expect("two entries");
        command.apply(&mut value).expect("a brick has an x");
        assert_eq!(value.position[0], 1.0);
        assert_eq!(log.position(), 1);

        let command = log.redo().expect("one entry above the position");
        command.apply(&mut value).expect("a brick has an x");
        assert_eq!(value.position[0], 2.0);
        assert_eq!(log.position(), 2);
        assert!(log.redo().is_none(), "there is nothing above the top");
    }

    /// A new command after an undo drops the future that undo had exposed.
    #[test]
    fn recording_after_an_undo_drops_the_entries_above_the_position() {
        let mut value = brick();
        let mut log = UndoLog::new();
        for to in [1.0, 2.0] {
            let command = set("position.0", Value::Float(to));
            let undo = command.apply(&mut value).expect("a brick has an x");
            log.record(command, undo);
        }
        log.undo()
            .expect("two entries")
            .apply(&mut value)
            .expect("a brick has an x");

        let command = set("position.0", Value::Float(5.0));
        let undo = command.apply(&mut value).expect("a brick has an x");
        log.record(command, undo);

        assert_eq!(log.len(), 2, "the 2.0 entry is gone");
        assert_eq!(log.position(), 2);
        assert!(log.redo().is_none());
    }

    /// An empty log has nothing to walk in either direction.
    #[test]
    fn an_empty_log_undoes_and_redoes_nothing() {
        let mut log = UndoLog::new();
        assert!(log.is_empty());
        assert!(log.undo().is_none());
        assert!(log.redo().is_none());
        assert_eq!(log.position(), 0);
    }
}
