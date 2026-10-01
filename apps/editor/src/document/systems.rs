//! Which systems an entity is in: read, and changed through
//! [`EditCommand::Attach`] and [`EditCommand::Detach`].
//!
//! A scene may hold one entity in several systems (`crcbl::scene::scn`'s
//! _One entity, several systems_), each holding one component of it. An
//! attach gives the entity a component in one more of the scene's systems and
//! a detach takes one away, each the other's inverse through the same log as
//! every other edit, and refused in play mode by the same check.
//!
//! # Only the manifest's systems
//!
//! [`attachable`](Document::attachable) offers the systems the scene's
//! manifest lists that do not hold the entity, not every system the
//! vocabulary registers: a save writes the manifest's chunks and no others, so
//! a component in an unlisted system would be dropped by the next one, and
//! adding a system to the manifest is a change to the scene's header that no
//! command makes yet.
//!
//! # A new component's value
//!
//! [`attach`](Document::attach) starts the component at its type's `Default`,
//! which [`crcbl::registry::Registry::register`] requires of every registered
//! component — see that method for why the game chooses it rather than this
//! tool.

use crcbl::scene::scn::{SceneEntityId, SystemChunk};

use super::{Document, EditError, sync_colliders};
use crate::command::EditCommand;

impl Document {
    /// The systems holding `id`, in the order the scene's manifest lists them —
    /// the order the inspector draws their sections in. Empty for an id this
    /// document does not hold.
    #[must_use]
    pub fn systems_of(&mut self, id: SceneEntityId) -> Vec<String> {
        let Some(entity) = self.ids.entity(id) else {
            return Vec::new();
        };
        let held = self.registry.systems_of(&mut self.world, entity);
        self.scene
            .systems()
            .iter()
            .filter(|system| held.contains(system))
            .cloned()
            .collect()
    }

    /// The system whose component places `id` — what a gizmo or an arrow key
    /// moves, so that what moves is what the picture and the pick read.
    /// [`crcbl::registry::Registry::placing_system`] says which one that is.
    ///
    /// [`None`] for an id this document does not hold, and for an entity none
    /// of whose components is a thing in space.
    #[must_use]
    pub fn placing_system(&mut self, id: SceneEntityId) -> Option<String> {
        let entity = self.ids.entity(id)?;
        self.registry.placing_system(&mut self.world, entity)
    }

    /// The systems `id` could be given a component in: the manifest's, in its
    /// order, less those already holding it. See the module docs for why the
    /// manifest's alone.
    #[must_use]
    pub fn attachable(&mut self, id: SceneEntityId) -> Vec<String> {
        if self.ids.entity(id).is_none() {
            return Vec::new();
        }
        let held = self.systems_of(id);
        self.scene
            .systems()
            .iter()
            .filter(|system| !held.contains(system))
            .cloned()
            .collect()
    }

    /// Gives `id` a component in `system`, at the component type's `Default`,
    /// as one [`EditCommand::Attach`].
    ///
    /// # Errors
    ///
    /// [`EditError::Playing`] in play mode, [`EditError::NoSystem`] for a
    /// system the manifest does not list, [`EditError::NoEntity`] for an id
    /// this document does not hold, [`EditError::Attached`] for a system that
    /// already holds it, and [`EditError::Scene`] if the default would not
    /// serialise. Nothing is attached or recorded when it refuses.
    pub fn attach(&mut self, id: SceneEntityId, system: &str) -> Result<(), EditError> {
        self.refuse_in_play()?;
        self.listed_codec(system)?;
        let row = self.registry.default_row(system)?;
        self.apply(EditCommand::Attach {
            entity: id,
            system: system.to_owned(),
            row,
        })
    }

    /// Takes `id`'s component out of `system`, as one [`EditCommand::Detach`]
    /// whose undo attaches the same row back.
    ///
    /// # Errors
    ///
    /// [`EditError::Playing`] in play mode, [`EditError::NoEntity`] for an id
    /// this document does not hold, [`EditError::NoSystem`] for a system the
    /// manifest does not list, [`EditError::NotAttached`] for one that does not
    /// hold it, and [`EditError::NoComponent`] for its last system —
    /// [`delete`](Self::delete) is how an entity goes. Nothing is detached or
    /// recorded when it refuses.
    pub fn detach(&mut self, id: SceneEntityId, system: &str) -> Result<(), EditError> {
        self.apply(EditCommand::Detach {
            entity: id,
            system: system.to_owned(),
        })
    }

    /// Attaches `row` to `id` in `system`, and hands back the detach that
    /// undoes it — the body of [`EditCommand::Attach`].
    pub(super) fn attach_row(
        &mut self,
        id: SceneEntityId,
        system: &str,
        row: &str,
    ) -> Result<EditCommand, EditError> {
        let entity = self.ids.entity(id).ok_or(EditError::NoEntity(id))?;
        let codec = self.listed_codec(system)?;
        if codec.row(&mut self.world, entity)?.is_some() {
            return Err(EditError::Attached {
                entity: id,
                system: system.to_owned(),
            });
        }
        codec.attach_row(&mut self.world, entity, row)?;
        self.joined(entity);
        Ok(EditCommand::Detach {
            entity: id,
            system: system.to_owned(),
        })
    }

    /// Takes `id`'s component out of `system`, and hands back the attach of the
    /// row it removed — the body of [`EditCommand::Detach`].
    pub(super) fn detach_row(
        &mut self,
        id: SceneEntityId,
        system: &str,
    ) -> Result<EditCommand, EditError> {
        let entity = self.ids.entity(id).ok_or(EditError::NoEntity(id))?;
        let codec = self.listed_codec(system)?;
        let held = self.systems_of(id);
        if !held.iter().any(|each| each == system) {
            return Err(EditError::NotAttached {
                entity: id,
                system: system.to_owned(),
            });
        }
        if held.len() == 1 {
            return Err(EditError::NoComponent(id));
        }
        let row =
            codec
                .detach_row(&mut self.world, entity)?
                .ok_or_else(|| EditError::NotAttached {
                    entity: id,
                    system: system.to_owned(),
                })?;
        self.joined(entity);
        Ok(EditCommand::Attach {
            entity: id,
            system: system.to_owned(),
            row,
        })
    }

    /// The codec of `system`, if the scene's manifest lists it and the
    /// vocabulary can read it — the one check a spawn, an attach and a detach
    /// share.
    pub(super) fn listed_codec(&self, system: &str) -> Result<Box<dyn SystemChunk>, EditError> {
        self.scene
            .systems()
            .iter()
            .any(|listed| listed == system)
            .then(|| self.registry.codec(system))
            .flatten()
            .ok_or_else(|| EditError::NoSystem(system.to_owned()))
    }

    /// What follows `entity` entering or leaving a system: its collider rebuilt
    /// from whatever places it now, and the membership moved so the outline and
    /// the picture are read again.
    fn joined(&mut self, entity: crcbl::ecs::Entity) {
        sync_colliders(&self.registry, &mut self.world, [entity]);
        self.membership += 1;
    }
}
