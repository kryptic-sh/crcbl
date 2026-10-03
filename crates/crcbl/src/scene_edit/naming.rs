//! What a person calls an entity: the scene's names, read and changed through
//! [`EditCommand::Rename`].
//!
//! A name is the scene's (`crate::scene::scn::names`), keyed by the
//! [`SceneEntityId`] every command already names, so a rename is undone and
//! redone through the same log as every other edit and refused in play mode by
//! the same check.

use std::collections::BTreeMap;

use crate::scene::scn::{EntityName, SceneEntityId};

use super::{Document, EditError};
use crate::scene::edit::EditCommand;

impl Document {
    /// What `id` is called, if it is named.
    #[must_use]
    pub fn entity_name(&self, id: SceneEntityId) -> Option<&EntityName> {
        self.scene.entity_name(id)
    }

    /// Every named entity's name, in id order.
    #[must_use]
    pub const fn entity_names(&self) -> &BTreeMap<SceneEntityId, EntityName> {
        self.scene.entity_names()
    }

    /// A number that moves every time an entity's name changes — a rename,
    /// and either one undone or redone.
    ///
    /// What a view showing names re-reads them on: a spawn and a delete move
    /// [`membership`](Self::membership) instead, which such a view re-reads on
    /// already.
    #[must_use]
    pub const fn naming(&self) -> u64 {
        self.naming
    }

    /// Names `id` what `text` says, as an [`EditCommand::Rename`] — or takes
    /// its name away, for text that is empty once trimmed, which is what
    /// clearing a name field means. Returns whether anything changed: a rename
    /// to the name the entity already has records nothing.
    ///
    /// # Errors
    ///
    /// [`EditError::Playing`] in play mode, [`EditError::NoEntity`] for an id
    /// this document does not hold, and [`EditError::Name`] for text that is
    /// too long or holds a control character. Nothing is recorded when it
    /// refuses.
    pub fn rename(&mut self, id: SceneEntityId, text: &str) -> Result<bool, EditError> {
        self.refuse_in_play()?;
        if self.ids.entity(id).is_none() {
            return Err(EditError::NoEntity(id));
        }
        let name = if text.trim().is_empty() {
            None
        } else {
            Some(EntityName::new(text).map_err(EditError::Name)?)
        };
        if name.as_ref() == self.scene.entity_name(id) {
            return Ok(false);
        }
        self.apply(EditCommand::Rename { entity: id, name })?;
        Ok(true)
    }

    /// Gives `id` `name`, and hands back the rename that puts the name it had
    /// back — the body of [`EditCommand::Rename`].
    pub(super) fn set_name(
        &mut self,
        id: SceneEntityId,
        name: Option<EntityName>,
    ) -> Result<EditCommand, EditError> {
        if self.ids.entity(id).is_none() {
            return Err(EditError::NoEntity(id));
        }
        let had = self.scene.set_entity_name(id, name);
        self.naming += 1;
        Ok(EditCommand::Rename {
            entity: id,
            name: had,
        })
    }
}
