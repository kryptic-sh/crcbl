//! Every [`Rotation`] in the scene kept a rotation: refused when an edit would
//! leave one off unit, and reported when one is anyway.
//!
//! A rotation's four numbers are reflected leaves, so a
//! [`EditCommand::SetProperty`] can write one of them alone, and nothing in
//! [`crcbl::reflect`] refuses a leaf for what its neighbours hold. A file's
//! rotation is checked where it is read ([`Rotation`]'s `try_from`), so a
//! value left off unit saves and is then refused by the next load — the
//! failure this module moves to where the edit is made.
//!
//! # Refused at the edit, not renormalised
//!
//! [`Document::apply`] checks every component a command's property writes
//! touched, and an edit that leaves a rotation further than
//! [`ROTATION_TOLERANCE`](crcbl::registry::ROTATION_TOLERANCE) from unit is
//! put back and refused as [`EditError::Rotation`], naming the entity, the
//! system and the field — which the editor puts on its status line. Refused
//! rather than renormalised, for [`Rotation`]'s own reason: a file's value
//! off unit is refused, not repaired, because normalising writes numbers
//! nobody wrote, and the one path that writes a single leaf — pasting one of a
//! quaternion's numbers through [`Document::paste_field`] — is a person typing
//! a number into a value that only means something as four. Every editor edit
//! that turns a component (the rotate handle, the inspector's angles) writes
//! all four leaves in one command, and passes.
//!
//! # Reported at the save, for what did not come through an edit
//!
//! [`Document::component`] hands a panel the component to write before it
//! reports the write, so a value can stand in the world without a command —
//! the inspector's rewind puts it back, and another caller need not.
//! [`Document::problems`] therefore also reports every rotation off unit in
//! the components the scene would save, by entity and field, as it reports a
//! mesh whose asset will not load. A mesh's rotation off unit is reported
//! twice then: here, by entity and field, and by the meshes chunk check, by
//! file line and column.
//!
//! # Every component, found by type
//!
//! The walk ([`faults`]) looks for the [`Rotation`] type through each
//! component's reflected fields, nested ones included, rather than for a
//! field called `rotation` in components it names — so a game's component
//! that carries one is held by this without registering anything.

use crcbl::reflect::{Kind, Reflect};
use crcbl::registry::{Rotation, RotationError};
use crcbl::scene::scn::SceneEntityId;

use super::{Document, EditError};
use crate::command::EditCommand;

/// A rotation that is not one: where it is in a component, and why.
#[derive(Clone, Debug, PartialEq)]
pub(super) struct Fault {
    /// Its dotted path in the component — `rotation` for a placing
    /// component's own.
    pub(super) field: String,
    /// What a load would refuse it for.
    pub(super) error: RotationError,
}

/// Every [`Rotation`] under `value`, `value` itself included, that a file
/// could not hold — each with its path from `value`, written after `prefix`.
pub(super) fn faults(value: &dyn Reflect, prefix: &str) -> Vec<Fault> {
    let mut found = Vec::new();
    walk(value, prefix, &mut found);
    found
}

/// [`faults`]' body, adding to `found`.
fn walk(value: &dyn Reflect, path: &str, found: &mut Vec<Fault>) {
    if let Some(rotation) = value.as_any().downcast_ref::<Rotation>() {
        if let Err(error) = rotation.check() {
            found.push(Fault {
                field: path.to_owned(),
                error,
            });
        }
        return;
    }
    let child = |segment: &str| {
        if path.is_empty() {
            segment.to_owned()
        } else {
            format!("{path}.{segment}")
        }
    };
    match value.kind() {
        Kind::Leaf(_) => {}
        Kind::Struct | Kind::Enum => {
            for (index, field) in value.fields().iter().enumerate() {
                if let Some(inner) = value.field(index) {
                    walk(inner, &child(field.name), found);
                }
            }
        }
        Kind::List { len } => {
            for index in 0..len {
                if let Some(inner) = value.field(index) {
                    walk(inner, &child(&index.to_string()), found);
                }
            }
        }
    }
}

/// Every property write in `command`, batches opened: whose component, in
/// which system, at which path.
fn writes(command: &EditCommand) -> Vec<(SceneEntityId, &str, &str)> {
    match command {
        EditCommand::SetProperty {
            entity,
            system,
            path,
            ..
        } => vec![(*entity, system.as_str(), path.as_str())],
        EditCommand::Batch(commands) => commands.iter().flat_map(writes).collect(),
        _ => Vec::new(),
    }
}

impl Document {
    /// The first rotation `command`'s property writes left off unit, as the
    /// refusal [`apply`](Self::apply) gives — or `Ok` for a command that
    /// turned nothing out of true. Only a rotation a write landed in counts:
    /// the field is the written path or the path leads into it.
    pub(super) fn turned_true(&mut self, command: &EditCommand) -> Result<(), EditError> {
        for (entity, system, path) in writes(command) {
            let component = self.component_of(entity, system)?;
            let fault = faults(component, "").into_iter().find(|fault| {
                path == fault.field
                    || path
                        .strip_prefix(fault.field.as_str())
                        .is_some_and(|rest| rest.starts_with('.'))
            });
            if let Some(Fault { field, error }) = fault {
                return Err(EditError::Rotation {
                    entity,
                    system: system.to_owned(),
                    field,
                    error,
                });
            }
        }
        Ok(())
    }

    /// Every rotation off unit in a component the scene would save, each
    /// naming its entity, its system and its field — see the module docs of
    /// `document::rotations`.
    #[must_use]
    pub fn rotation_problems(&mut self) -> Vec<String> {
        let mut problems = Vec::new();
        for system in self.scene.systems().to_vec() {
            for entity in self.registry.entities(&mut self.world, &self.ids, &system) {
                let Some(id) = self.ids.id(entity) else {
                    continue;
                };
                let Some(component) = self.registry.component(&mut self.world, &system, entity)
                else {
                    continue;
                };
                for Fault { field, error } in faults(component, "") {
                    problems.push(self.about(id, &format!("`{system}`'s `{field}`: {error}")));
                }
            }
        }
        problems
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    use crcbl::reflect::{Value, set_path};

    /// A component carrying rotations at the top, nested, and in a list.
    #[derive(Default, Reflect)]
    #[reflect(crate = "crcbl::reflect")]
    struct Rig {
        rotation: Rotation,
        arm: Arm,
        joints: [Rotation; 2],
    }

    #[derive(Default, Reflect)]
    #[reflect(crate = "crcbl::reflect")]
    struct Arm {
        length: f64,
        rotation: Rotation,
    }

    /// **The walk finds a rotation off unit wherever the component holds it**,
    /// by its path, and nothing in a component whose rotations are all unit.
    #[test]
    fn the_walk_finds_every_rotation_off_unit_by_its_path() {
        let mut rig = Rig::default();
        assert!(faults(&rig, "").is_empty());
        for path in ["rotation.w", "arm.rotation.x", "joints.1.y"] {
            set_path(&mut rig, path, &Value::Float(2.0)).expect("a leaf");
        }
        let found: Vec<String> = faults(&rig, "")
            .into_iter()
            .map(|fault| fault.field)
            .collect();
        assert_eq!(found, ["rotation", "arm.rotation", "joints.1"]);
        assert_eq!(
            faults(&rig.arm, "arm")[0].error,
            RotationError::NotUnit {
                length: 5.0_f64.sqrt()
            },
        );
    }
}
