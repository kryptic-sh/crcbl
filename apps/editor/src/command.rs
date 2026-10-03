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
//! [`crate::Document::record_edits`] is where that is turned back into a
//! command. It is the crate's only field write, and it is a rewind: what the
//! panel did is undone so the command can do it, and so record an exact
//! inverse. That method's docs say why.
//!
//! # The shape of an edit
//!
//! A command names **the entity, the system, the dotted path and the new
//! value** — the system because one entity may hold a component in several,
//! and the rest exactly what [`crcbl::reflect::set_path`] takes, which is why
//! that crate's [`Value`] is the payload rather than a second one declared
//! here. Applying a property reads the leaf first and hands back the command
//! that puts it back: an inverse is produced, never derived later from a rule
//! that could be wrong.
//!
//! ```
//! use crcbl::reflect::{Reflect, Value};
//! use crcbl::scene::scn::SceneEntityId;
//! use crcbl_editor::command::{EditCommand, set_property};
//!
//! #[derive(Reflect)]
//! #[reflect(crate = "crcbl::reflect")]
//! struct Brick {
//!     position: [f64; 3],
//! }
//!
//! let mut brick = Brick { position: [1.0, 2.0, 3.0] };
//! let inverse = set_property(
//!     &mut brick,
//!     SceneEntityId(7),
//!     "bricks",
//!     "position.1",
//!     &Value::Float(9.0),
//! )
//! .expect("a brick has a y");
//! assert_eq!(brick.position, [1.0, 9.0, 3.0]);
//! let EditCommand::SetProperty { entity, system, path, value } = inverse else {
//!     unreachable!("a property's inverse is a property");
//! };
//! set_property(&mut brick, entity, &system, &path, &value).expect("and it still has one");
//! assert_eq!(brick.position, [1.0, 2.0, 3.0]);
//! ```
//!
//! # Switching an enum's variant
//!
//! [`SetVariant`](EditCommand::SetVariant) is the one edit a leaf's value
//! cannot carry: a switch replaces every field of the variant it leaves, so
//! its inverse has to hold that variant and everything under it. It carries
//! a [`Snapshot`] of the whole enum, and applying it reads the snapshot
//! there first and hands back the switch to that ([`set_variant`]) — so an
//! undo puts back the variant and its fields bit for bit, as a property's
//! puts back its leaf. A switch is never folded into a gesture's entry: it
//! is one click, and one entry.
//!
//! # Creating and removing entities
//!
//! [`Spawn`](EditCommand::Spawn) and [`Delete`](EditCommand::Delete) are each
//! other's inverse, and a spawn carries **the id and every system's row** —
//! each of the entity's components as one chunk row's RON text, which
//! [`crcbl::scene::scn::SystemChunk::row`] reads and `attach_row` rebuilds. So
//! undoing a delete brings the entity back under the id it had, in every system
//! it was in, and a later command in the history that names it still finds
//! it. A **duplicate** is not
//! a variant of its own: it is a spawn whose row was read off the original and
//! whose id is the next one the document would hand out
//! ([`crate::Document::duplicate`]). A **paste** is a
//! [`Batch`](EditCommand::Batch) of spawns, one per entity the clipboard holds
//! ([`crate::Document::paste`]), so one undo takes the whole paste back.
//!
//! # Attaching and detaching one system's data
//!
//! [`Attach`](EditCommand::Attach) gives an entity a component in one more
//! system, from a row; [`Detach`](EditCommand::Detach) takes one system's
//! component away and hands back the attach of the row it removed. Each is the
//! other's inverse. **Detaching an entity's last system is refused**, not
//! turned into a delete: a delete's inverse is a spawn carrying the name as well
//! as the rows, so a detach that sometimes deleted would sometimes have an
//! inverse of another shape, and "remove this component" quietly removing the
//! entity is not what the person asked for. [`Delete`](EditCommand::Delete) is
//! how an entity goes.
//!
//! # Listing a system in the manifest
//!
//! [`ListSystem`](EditCommand::ListSystem) adds a system the vocabulary knows
//! to the scene's manifest, so its chunk is saved;
//! [`UnlistSystem`](EditCommand::UnlistSystem) takes one holding no entity
//! back out. Each is the other's inverse, a listing carrying the place it
//! takes so that an unlisting's undo puts the system back where it stood. A
//! drop from the asset browser into a scene with no meshes yet is the case,
//! and an attach to a system the manifest does not list: a
//! [`Batch`](EditCommand::Batch) of the listing and the spawn or the attach,
//! so one undo takes the row and the manifest entry back and the files are
//! what they were. Unlisting a system still holding rows is refused,
//! since the save that followed would drop them.
//!
//! # Names
//!
//! [`Rename`](EditCommand::Rename) names an entity, or takes its name away,
//! and its inverse is the rename back to the name it had — the scene's
//! `names.ron` ([`crcbl::scene::scn::names`]) is what it writes. A spawn
//! carries a name too, so a deleted entity comes back under its name as well as
//! its id; a **duplicate** carries none, because a name says which entity this
//! is and the copy is another one ([`crate::Document::duplicate`]).
//!
//! # What task 4 does not have yet
//!
//! The plan's task 4 lists about ten commands for the MVP. One waits on
//! something outside this crate, and an arm nothing could apply would be an
//! inverse nothing could show was right: **scene load and save markers** — the
//! log's position against the position of the last save already is the dirty
//! marker, and a load replaces the document and its log wholesale, so there is
//! nothing in between for a marker entry to mean yet.
//!
//! A **transform** command is not a second variant either, and that is not an
//! omission: a brick's placement *is* `position`, a field its `#[derive(Reflect)]`
//! describes, so a nudge is a [`SetProperty`](EditCommand::SetProperty) on
//! `position.0`. A transform arm arrives when something carries a transform
//! that is not a reflected field — `crcbl::phys::Transform` on an entity with no
//! component holding it.
//!
//! [`Value`]: crcbl::reflect::Value

use crcbl::reflect::{
    PathError, Reflect, Snapshot, Value, get_path, restore_path, set_path, snapshot_path,
};
use crcbl::scene::scn::{EntityName, SceneEntityId};

/// One undoable edit, as a value that could be sent rather than performed.
///
/// [`SceneEntityId`] and not [`crcbl::ecs::Entity`]: a command outlives the
/// application of it, and an `Entity`'s bits are a function of spawn history —
/// the file's id is the one both ends of a transport could agree on, and the
/// one an undo entry can still name after a reload.
#[derive(Clone, Debug, PartialEq)]
pub enum EditCommand {
    /// Write `value` into the leaf `path` names inside `entity`'s component in
    /// `system`.
    SetProperty {
        /// Whose component: the id the scene file spells.
        entity: SceneEntityId,
        /// Which of its components: the scene system holding it.
        system: String,
        /// The dotted path, in [`crcbl::reflect`]'s grammar — `"position.1"`
        /// is a brick's `y`.
        path: String,
        /// What the leaf is being set to.
        value: Value,
    },

    /// Switch the enum `path` names inside `entity`'s component in `system`
    /// to the variant `value` holds, its fields as `value` holds them.
    SetVariant {
        /// Whose component: the id the scene file spells.
        entity: SceneEntityId,
        /// Which of its components: the scene system holding it.
        system: String,
        /// The dotted path to the enum, in [`crcbl::reflect`]'s grammar —
        /// `"kind"` is a body's.
        path: String,
        /// The whole enum it is being set to.
        value: Snapshot,
    },

    /// Create `entity` holding a component in each system `rows` names.
    Spawn {
        /// The id the entity is filed under: one no entity holds, which is
        /// [`crcbl::scene::scn::IdMap::next_id`] for a new one and the id it had
        /// for one a delete removed.
        entity: SceneEntityId,
        /// Its components, one per system and at least one — an entity in no
        /// system is in no chunk file, and a save would lose it.
        rows: Vec<SystemRow>,
        /// What it is called, or [`None`] for an unnamed entity.
        name: Option<EntityName>,
    },

    /// Remove `entity` from the scene, every component and its name.
    Delete {
        /// Whose.
        entity: SceneEntityId,
    },

    /// Give `entity`, which `system` does not hold, the component `row` spells
    /// there.
    Attach {
        /// Whose.
        entity: SceneEntityId,
        /// The scene system it joins.
        system: String,
        /// The component, as one chunk row's RON text.
        row: String,
    },

    /// Take `entity`'s component out of `system`, leaving every other one —
    /// refused for its last system (see the [module docs](self)).
    Detach {
        /// Whose.
        entity: SceneEntityId,
        /// The scene system it leaves.
        system: String,
    },

    /// Add `system`, which the vocabulary registers and the manifest does not
    /// list, to the manifest at position `at`.
    ListSystem {
        /// The scene system listed.
        system: String,
        /// Its place in the manifest: the manifest's length for the end, which
        /// is where an attach or a drop lists a system, and the place it held
        /// for the undo of an [`UnlistSystem`](Self::UnlistSystem) — the
        /// manifest's order is the order its chunks are saved in, so a system
        /// put back anywhere else would save a different `scene.ron`.
        at: usize,
    },

    /// Take `system`, which holds no entity, out of the manifest, wherever it
    /// stands.
    UnlistSystem {
        /// The scene system unlisted.
        system: String,
    },

    /// Name `entity` `name`, or take its name away with [`None`].
    ///
    /// Its inverse is the rename to the name the entity had, read as it is
    /// applied — which is how "old and new name" are both in the log.
    Rename {
        /// Whose.
        entity: SceneEntityId,
        /// The name it is given.
        name: Option<EntityName>,
    },

    /// Several commands applied in order as one entry of the log, so one undo
    /// walks all of them back — a paste of several entities is the case.
    ///
    /// Its inverse is a batch of the inverses in reverse order. A batch whose
    /// member is refused puts back the members before it and applies nothing
    /// (`crate::Document::apply`).
    Batch(Vec<EditCommand>),
}

impl EditCommand {
    /// `commands` as one entry: the command itself when there is one, and a
    /// [`Batch`](Self::Batch) of them otherwise — so an edit of one entity
    /// records the same entry whether it came from a selection of one or
    /// from a path that only ever names one.
    #[must_use]
    pub fn one_or_batch(mut commands: Vec<Self>) -> Self {
        if commands.len() == 1 {
            commands.remove(0)
        } else {
            Self::Batch(commands)
        }
    }
}

/// One system's component of an entity: the system, and the component as one
/// chunk row's RON text — what a spawn carries per system.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SystemRow {
    /// The scene system whose chunk file the row is written into.
    pub system: String,
    /// The component, as [`crcbl::scene::scn::SystemChunk::row`] spells it.
    pub row: String,
}

/// Writes `value` into the leaf `path` names inside `component` — `entity`'s
/// component in `system`, which the caller has already resolved — and hands
/// back the [`EditCommand::SetProperty`] that undoes it.
///
/// The inverse carries **the value that was replaced**, read out of the
/// component immediately before the write. So an undo restores the bits that
/// were there rather than a value computed from the command, which is what
/// makes a round trip byte-for-byte on floats.
///
/// # Errors
///
/// [`PathError`] if the path names nothing in this component, stops at
/// something that is not a leaf, or reaches a leaf that refuses the value.
/// Nothing is written when it does — [`get_path`] runs first, and
/// [`crcbl::reflect::Reflect::set`] leaves a refused leaf untouched.
pub fn set_property(
    component: &mut dyn Reflect,
    entity: SceneEntityId,
    system: &str,
    path: &str,
    value: &Value,
) -> Result<EditCommand, PathError> {
    let replaced = get_path(component, path)?;
    set_path(component, path, value)?;
    Ok(EditCommand::SetProperty {
        entity,
        system: system.to_owned(),
        path: path.to_owned(),
        value: replaced,
    })
}

/// Switches the enum `path` names inside `component` to the variant `value`
/// holds — `entity`'s component in `system`, which the caller has already
/// resolved — and hands back the [`EditCommand::SetVariant`] that undoes it.
///
/// The inverse carries **the whole enum that was replaced**, read out of the
/// component immediately before the write, as [`set_property`]'s carries the
/// leaf.
///
/// # Errors
///
/// [`PathError`] if the path names nothing in this component, or the enum
/// refuses the snapshot — a variant it does not have, or fields of another
/// shape — in which case it is put back as it was
/// ([`crcbl::reflect::Snapshot::restore`]).
pub fn set_variant(
    component: &mut dyn Reflect,
    entity: SceneEntityId,
    system: &str,
    path: &str,
    value: &Snapshot,
) -> Result<EditCommand, PathError> {
    let replaced = snapshot_path(component, path)?;
    restore_path(component, path, value)?;
    Ok(EditCommand::SetVariant {
        entity,
        system: system.to_owned(),
        path: path.to_owned(),
        value: replaced,
    })
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
///
/// # A gesture is one entry
///
/// A drag writes its leaf every frame it moves, and a log that kept each write
/// would take as many undos to walk back as the drag had frames.
/// [`record_in`](Self::record_in) folds a property set — or a batch of them —
/// into the entry on top when both came from the same [`Gesture`], **whatever
/// leaves each names** (decided 2026-10-03: one gesture is one undo entry).
/// The entry keeps, per leaf, the value from before the gesture first wrote it
/// — the earliest inverse — and the value its newest write left, so its undo
/// puts back every leaf the gesture touched and its redo applies where each
/// ended. A leaf that ends bit for bit where it began drops out, and an entry
/// left with no leaf goes, so a drag back to its start records nothing.
/// [`seal`](Self::seal) closes the entry on top to further folding, which a
/// save does so that a drag carried on past it is dirty again.
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
    /// The gesture this entry is still open to, or [`None`] once nothing more
    /// may fold into it.
    gesture: Option<Gesture>,
}

/// One continuous gesture — a drag, from press to release — whose writes the
/// log keeps as a single entry. See [`UndoLog`].
///
/// Handed out by [`crate::Document::begin_gesture`], each one distinct, so two
/// drags never fold into each other.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Gesture(pub u64);

impl UndoLog {
    /// An empty log, standing at position 0.
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// Records a command that has just been applied, and the inverse
    /// applying it handed back ([`crate::Document::apply`] is where both meet).
    ///
    /// Anything above the current position is dropped: it described a future
    /// that this command has replaced.
    pub fn record(&mut self, done: EditCommand, undo: EditCommand) {
        self.push(done, undo, None);
    }

    /// [`record`](Self::record), for a command that is part of `gesture` —
    /// folded into the entry on top when that entry is the same gesture's,
    /// open and at the top of the log, and both are property sets or batches
    /// of them. The leaves are merged (see [`UndoLog`]): a leaf the gesture
    /// writes for the first time part-way brings its inverse into the entry,
    /// so a drag whose frames report different leaves is still one undo that
    /// restores them all.
    pub fn record_in(&mut self, done: EditCommand, undo: EditCommand, gesture: Gesture) {
        if self.position == self.entries.len()
            && let Some(last) = self.entries.last_mut()
            && last.gesture == Some(gesture)
            && let Some(merged) = Leaves::merged(&last.done, &last.undo, &done, &undo)
        {
            match merged.into_commands() {
                Some((done, undo)) => {
                    last.done = done;
                    last.undo = undo;
                }
                None => {
                    self.entries.pop();
                    self.position = self.entries.len();
                }
            }
            return;
        }
        self.push(done, undo, Some(gesture));
    }

    /// Closes the entry the log stands on to further folding.
    pub fn seal(&mut self) {
        if let Some(last) = self
            .position
            .checked_sub(1)
            .and_then(|top| self.entries.get_mut(top))
        {
            last.gesture = None;
        }
    }

    fn push(&mut self, done: EditCommand, undo: EditCommand, gesture: Option<Gesture>) {
        self.entries.truncate(self.position);
        self.entries.push(Entry {
            done,
            undo,
            gesture,
        });
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

/// One leaf a property set writes: the entity, the system and the path.
type Leaf = (SceneEntityId, String, String);

/// A gesture's leaves, in the order it first wrote them, each with the value
/// from before its first write and the value its newest write left — what
/// [`UndoLog::record_in`] folds a gesture's writes into.
struct Leaves(Vec<(Leaf, Value, Value)>);

impl Leaves {
    /// The entry `done`/`undo` with the write `next`/`next_undo` folded in, or
    /// [`None`] when any of them is not a property set or a batch of them.
    ///
    /// A leaf already in the entry keeps its earliest value and takes
    /// `next`'s; a leaf new to it takes its earliest value from `next_undo` —
    /// what it held before this write, which no earlier write of the gesture
    /// had changed. Leaves ending bit for bit where they began are dropped.
    fn merged(
        done: &EditCommand,
        undo: &EditCommand,
        next: &EditCommand,
        next_undo: &EditCommand,
    ) -> Option<Self> {
        let mut leaves = Self(Vec::new());
        leaves.fold(sets(done)?, &sets(undo)?)?;
        leaves.fold(sets(next)?, &sets(next_undo)?)?;
        leaves
            .0
            .retain(|(_, before, after)| !before.identical(after));
        Some(leaves)
    }

    /// Folds in one write's `sets`: a leaf already held takes the newer
    /// value, and a new one is held with the value `befores` — that write's
    /// inverse — puts back. [`None`] if the inverse does not name the leaf.
    fn fold(&mut self, sets: Vec<(Leaf, Value)>, befores: &[(Leaf, Value)]) -> Option<()> {
        for (leaf, after) in sets {
            if let Some(held) = self.0.iter_mut().find(|(each, _, _)| *each == leaf) {
                held.2 = after;
            } else {
                let before = earliest(befores, &leaf)?;
                self.0.push((leaf, before, after));
            }
        }
        Some(())
    }

    /// The entry's command and its inverse — the inverse setting the leaves
    /// in reverse, as a batch's does — or [`None`] when no leaf is left.
    fn into_commands(self) -> Option<(EditCommand, EditCommand)> {
        if self.0.is_empty() {
            return None;
        }
        let set = |(entity, system, path): Leaf, value: Value| EditCommand::SetProperty {
            entity,
            system,
            path,
            value,
        };
        let mut done = Vec::with_capacity(self.0.len());
        let mut undo = Vec::with_capacity(self.0.len());
        for (leaf, before, after) in self.0 {
            undo.push(set(leaf.clone(), before));
            done.push(set(leaf, after));
        }
        undo.reverse();
        Some((
            EditCommand::one_or_batch(done),
            EditCommand::one_or_batch(undo),
        ))
    }
}

/// The value an inverse's `sets` puts back into `leaf`: the last one it
/// names, since an inverse batch runs in reverse and its last write is the
/// value from before the first.
fn earliest(sets: &[(Leaf, Value)], leaf: &Leaf) -> Option<Value> {
    sets.iter()
        .rev()
        .find(|(each, _)| each == leaf)
        .map(|(_, value)| value.clone())
}

/// Every leaf `command` sets and the value it sets there, in order — a
/// property set's one, a batch of them its members' — or [`None`] for a
/// command that is not made of property sets.
fn sets(command: &EditCommand) -> Option<Vec<(Leaf, Value)>> {
    match command {
        EditCommand::SetProperty {
            entity,
            system,
            path,
            value,
        } => Some(vec![(
            (*entity, system.clone(), path.clone()),
            value.clone(),
        )]),
        EditCommand::Batch(members) => {
            let mut all = Vec::with_capacity(members.len());
            for member in members {
                all.extend(sets(member)?);
            }
            Some(all)
        }
        _ => None,
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
            system: "bricks".to_owned(),
            path: path.to_owned(),
            value,
        }
    }

    /// A property command applied through [`set_property`], so the tests below
    /// read as "apply this command" — which is what the log's entries are.
    trait Apply {
        fn apply(&self, component: &mut dyn Reflect) -> Result<EditCommand, PathError>;
    }

    impl Apply for EditCommand {
        fn apply(&self, component: &mut dyn Reflect) -> Result<EditCommand, PathError> {
            // A batch's inverse is its members' in reverse, as the document's.
            if let EditCommand::Batch(commands) = self {
                let mut undo = commands
                    .iter()
                    .map(|command| command.apply(component))
                    .collect::<Result<Vec<_>, _>>()?;
                undo.reverse();
                return Ok(EditCommand::Batch(undo));
            }
            let EditCommand::SetProperty {
                entity,
                system,
                path,
                value,
            } = self
            else {
                panic!("this module's tests apply property commands and batches of them: {self:?}");
            };
            set_property(component, *entity, system, path, value)
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
                    other => panic!("only properties were recorded: {other:?}"),
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

    /// **A gesture's writes to one leaf are one entry**, whose undo restores the
    /// value from before the first write and whose redo applies the last.
    #[test]
    fn a_gestures_writes_to_one_leaf_are_one_entry() {
        let mut value = brick();
        let mut log = UndoLog::new();
        let drag = Gesture(1);
        for to in [1.0, 2.0, 3.0] {
            let command = set("position.0", Value::Float(to));
            let undo = command.apply(&mut value).expect("a brick has an x");
            log.record_in(command, undo, drag);
        }
        assert_eq!(log.len(), 1);
        log.undo()
            .expect("one entry")
            .apply(&mut value)
            .expect("a brick has an x");
        assert_eq!(
            value,
            brick(),
            "the undo did not reach the value before the drag"
        );
        log.redo()
            .expect("one entry above")
            .apply(&mut value)
            .expect("a brick has an x");
        assert_eq!(value.position[0], 3.0);
    }

    /// Another gesture or a sealed entry each start an entry of their own; a
    /// second leaf of the same gesture folds into its entry.
    ///
    /// **The rule changed 2026-10-03**: a write to another leaf used to start
    /// an entry of its own, because folding it kept only the first leaf's
    /// inverse. The fold now merges leaves, keeping each one's earliest
    /// inverse, so one gesture is one entry whatever leaves its frames name.
    #[test]
    fn a_new_gesture_or_seal_starts_a_new_entry_and_a_new_leaf_folds() {
        let mut value = brick();
        let mut log = UndoLog::new();
        let mut write = |log: &mut UndoLog, path: &str, gesture: u64| {
            let command = set(path, Value::Float(5.0));
            let undo = command.apply(&mut value).expect("a brick has that leaf");
            log.record_in(command, undo, Gesture(gesture));
        };
        write(&mut log, "position.0", 1);
        write(&mut log, "position.0", 2);
        assert_eq!(log.len(), 2, "a second gesture folded into the first");
        write(&mut log, "position.1", 2);
        assert_eq!(log.len(), 2, "a second leaf of one gesture did not fold");
        log.seal();
        write(&mut log, "position.2", 2);
        assert_eq!(log.len(), 3, "a sealed entry was folded into");
    }

    /// **A gesture that writes two leaves a frame is one entry too**, as one
    /// batch a frame — a plane drag's shape — and so is one whose batches
    /// name other leaves part-way: one undo puts back every leaf any frame
    /// wrote.
    #[test]
    fn a_gestures_batches_are_one_entry_whatever_leaves_they_name() {
        let mut value = brick();
        let mut log = UndoLog::new();
        let mut write = |log: &mut UndoLog, paths: [&str; 2], to: f64| {
            let command =
                EditCommand::Batch(paths.map(|path| set(path, Value::Float(to))).to_vec());
            let undo = command.apply(&mut value).expect("a brick has both leaves");
            log.record_in(command, undo, Gesture(1));
        };
        for to in [1.0, 2.0, 3.0] {
            write(&mut log, ["position.0", "position.1"], to);
        }
        assert_eq!(log.len(), 1, "a batch of the same leaves did not fold");
        write(&mut log, ["position.1", "position.2"], 4.0);
        assert_eq!(log.len(), 1, "a batch of other leaves did not fold");

        log.undo()
            .expect("the one entry")
            .apply(&mut value)
            .expect("a brick has every leaf");
        assert_eq!(
            value,
            brick(),
            "the undo did not reach the value before the drag"
        );
        log.redo()
            .expect("the one entry above")
            .apply(&mut value)
            .expect("a brick has every leaf");
        assert_eq!(
            value.position,
            [3.0, 4.0, 4.0],
            "the redo is not where it ended"
        );
    }

    /// **The regression for the 2026-10-03 decision: a drag whose second
    /// frame reports a different leaf set is one undo, restoring every
    /// leaf** — the one only the first frame wrote, the one only the second
    /// wrote, and the one both wrote, each to the bits from before the drag.
    #[test]
    fn a_drag_whose_frames_report_different_leaves_is_one_undo_restoring_all() {
        let mut value = brick();
        let before = value.position.map(f64::to_bits);
        let mut log = UndoLog::new();
        let frames: [&[(&str, f64)]; 2] = [
            &[("position.0", 1.0), ("position.1", 2.0)],
            &[("position.1", 3.0), ("position.2", 4.0)],
        ];
        for frame in frames {
            let command = EditCommand::one_or_batch(
                frame
                    .iter()
                    .map(|(path, to)| set(path, Value::Float(*to)))
                    .collect(),
            );
            let undo = command.apply(&mut value).expect("a brick has the leaves");
            log.record_in(command, undo, Gesture(7));
        }
        assert_eq!(log.len(), 1, "the drag is more than one entry");
        assert_eq!(value.position, [1.0, 3.0, 4.0]);

        log.undo()
            .expect("the one entry")
            .apply(&mut value)
            .expect("a brick has the leaves");
        assert_eq!(
            value.position.map(f64::to_bits),
            before,
            "the undo did not put every leaf back",
        );
        assert!(log.undo().is_none(), "the drag left a second entry");
    }

    /// **A leaf a gesture puts back where it began drops out of the entry**,
    /// and a gesture that ends exactly where it began records nothing — its
    /// entry goes, so the log stands where it did before the drag.
    #[test]
    fn a_gesture_back_to_its_start_drops_the_leaf_and_then_the_entry() {
        let mut value = brick();
        let mut log = UndoLog::new();
        let start = value.position[0];
        let mut write = |log: &mut UndoLog, path: &str, to: f64| {
            let command = set(path, Value::Float(to));
            let undo = command.apply(&mut value).expect("a brick has that leaf");
            log.record_in(command, undo, Gesture(3));
        };
        write(&mut log, "position.0", 1.0);
        write(&mut log, "position.1", 9.0);
        write(&mut log, "position.0", start);
        let Some(EditCommand::SetProperty { path, .. }) = log.applied().last() else {
            panic!("a gesture left with one leaf is one property set");
        };
        assert_eq!(path, "position.1", "the leaf back at its start stayed");
        write(&mut log, "position.1", brick().position[1]);
        assert!(log.is_empty(), "a gesture back at its start left an entry");
        assert_eq!(log.position(), 0);
    }

    /// **A gesture's writes to one path of two systems' components are two
    /// leaves**: one entity's block and its body each have a `position`, so
    /// the entry holds both inverses rather than taking one for the other.
    #[test]
    fn writes_to_one_path_in_two_systems_are_two_leaves() {
        let mut log = UndoLog::new();
        for (system, was) in [("bricks", 1.0), ("bodies", 2.0)] {
            let at = |value| EditCommand::SetProperty {
                entity: SceneEntityId(3),
                system: system.to_owned(),
                path: "position.0".to_owned(),
                value: Value::Float(value),
            };
            log.record_in(at(9.0), at(was), Gesture(1));
        }
        assert_eq!(log.len(), 1);
        let EditCommand::Batch(undo) = log.undo().expect("one entry") else {
            panic!("two leaves' inverse is a batch");
        };
        assert_eq!(undo.len(), 2, "one system's inverse was lost: {undo:?}");
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
