//! A chunk file of the document's own directory, read again from disk and
//! applied as an edit: stage 6's per-chunk reload, on the side where the
//! scene is authoritative.
//!
//! [`crate::scene::scn::reload`] says what a reload changes — row by row,
//! only the reloaded system's, ids kept where the file keeps them. Here the
//! difference becomes **edit commands**, applied through
//! [`Document::apply`]'s path: so a reload is one entry of the history, a
//! served document's clients hear of it as a server-side edit
//! ([`EditServer::reload_chunk`](super::EditServer::reload_chunk)), and
//! nothing about it is a second set of rules.
//!
//! * A row the file adds is an [`EditCommand::Attach`] to an entity another
//!   system holds, or an [`EditCommand::Spawn`] of a new one under the file's
//!   id.
//! * A row it changes is a detach and an attach of the new row — or, for an
//!   entity this system alone holds, a delete and a spawn under the same id
//!   and name, since a detach of an entity's last row is refused.
//! * A row it removes is a detach, or a delete for an entity this system
//!   alone holds.
//!
//! # The rule against the editor (decided 2026-10-06, for the long term)
//!
//! * **A document with no unsaved edits reloads, and stays clean.** The
//!   reload is an entry of its history, so an undo walks it back — and the
//!   document then differs from the disk, which is what dirty means.
//! * **A document with unsaved edits asks first**
//!   ([`OverEdits::Refuse`] answers [`EditError::Unsaved`], changing nothing),
//!   as a save asks when the disk changed under it
//!   ([`EditError::ChangedOnDisk`]): two people's work meeting is a choice for
//!   the person, not for the watch. [`OverEdits::Reload`] is the answer
//!   "take the disk's": the reload lands on top of the edits as one more
//!   entry, so they are still in the history beneath it and nothing is lost
//!   to the reload itself; the document stays dirty.
//! * **The scene lock does not stop it.** The lock binds programs that take
//!   it, and a change on disk while one is held came from one that did not —
//!   a text editor, a checkout. Seeing that change is the point.
//! * **After a reload the directory is what the document last read**
//!   ([`Document::accept_changes_on_disk`]), so the save that follows does not
//!   refuse over the very change it just took in.
//! * **Play mode refuses** ([`EditError::Playing`]), as every edit does: the
//!   played world is thrown away at stop. **A routed copy refuses**
//!   ([`EditError::Routed`]): its server's document is the one with a
//!   directory, and a reload routed as an edit would ask for fresh ids and
//!   lose the file's.

use std::path::Path;

use crate::assets::DirSource;
use crate::scene::edit::{EditCommand, SystemRow};
use crate::scene::scn::{ChunkDiff, RowChange, chunk_text, diff_chunk};

use super::{Document, EditError};

/// What a reload does when the document has edits it has not saved — see
/// the `scene_edit::reload` module docs.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum OverEdits {
    /// Refuse with [`EditError::Unsaved`], changing nothing, so the caller
    /// can ask.
    Refuse,
    /// Reload anyway, as one more entry on top of the edits.
    Reload,
}

/// What a reload changed.
#[derive(Clone, Debug, PartialEq)]
pub struct Reloaded {
    /// What the file held that the document did not, row by row.
    pub diff: ChunkDiff,
    /// The command the reload applied, as it applied — what the history
    /// records and what a server tells its clients — or [`None`] for a file
    /// that changed nothing.
    pub command: Option<EditCommand>,
}

impl Document {
    /// Reads `sys/<system>.ron` from the document's own directory again and
    /// applies what it changes as one entry of the history — see the
    /// `scene_edit::reload` module docs.
    ///
    /// # Errors
    ///
    /// [`EditError::Playing`] in play mode, [`EditError::Routed`] for a
    /// routed copy, [`EditError::NoOrigin`] for a document with no directory,
    /// [`EditError::Unsaved`] for unsaved edits under [`OverEdits::Refuse`],
    /// [`EditError::NoSystem`] for a system the manifest does not list or the
    /// vocabulary cannot read, and [`EditError::Scene`] for a file that will
    /// not read or is not the system's chunk — a write caught half-finished
    /// among them. **Every refusal changes nothing**, so the document keeps
    /// the last good state.
    pub fn reload_chunk(
        &mut self,
        system: &str,
        over_edits: OverEdits,
    ) -> Result<Reloaded, EditError> {
        self.refuse_in_play()?;
        if self.is_routed() {
            return Err(EditError::Routed);
        }
        let origin = self.origin.clone().ok_or(EditError::NoOrigin)?;
        let clean = !self.is_dirty();
        if !clean && over_edits == OverEdits::Refuse {
            return Err(EditError::Unsaved(system.to_owned()));
        }
        let codec = self.listed_codec(system)?;
        // Rooted at the scene directory and read with an empty prefix, as
        // `Document::open_dir` reads it.
        let source = DirSource::at(origin);
        let (key, text) = chunk_text(&source, Path::new(""), system)?;
        let diff = diff_chunk(codec.as_ref(), &mut self.world, &self.ids, &key, &text)?;
        let commands = self.commands_of(&diff);
        let command = if commands.is_empty() {
            None
        } else {
            self.apply_resolved(EditCommand::Batch(commands), None)?
        };
        if clean {
            self.log.mark_saved();
        }
        // Whatever the reload changed, the file it read is the disk's now: a
        // change of layout alone changes no row and is still a change.
        self.accept_changes_on_disk();
        Ok(Reloaded { diff, command })
    }

    /// The commands that make `diff`'s system hold what its file spells —
    /// see the [module docs](self).
    fn commands_of(&mut self, diff: &ChunkDiff) -> Vec<EditCommand> {
        let system = diff.system();
        let mut commands = Vec::new();
        for change in diff.changes() {
            let entity = change.id();
            // Whether another system holds the entity too, which decides
            // between a row-level edit and one of the whole entity.
            let shared = self.systems_of(entity).iter().any(|held| held != system);
            let row_of = |row: &str| SystemRow {
                system: system.to_owned(),
                row: row.to_owned(),
            };
            let detach = EditCommand::Detach {
                entity,
                system: system.to_owned(),
            };
            match change {
                RowChange::Added { row, .. } if self.ids.entity(entity).is_some() => {
                    commands.push(EditCommand::Attach {
                        entity,
                        system: system.to_owned(),
                        row: row.clone(),
                    });
                }
                RowChange::Added { row, .. } => commands.push(EditCommand::Spawn {
                    entity,
                    rows: vec![row_of(row)],
                    name: None,
                }),
                RowChange::Changed { row, .. } if shared => {
                    commands.push(detach);
                    commands.push(EditCommand::Attach {
                        entity,
                        system: system.to_owned(),
                        row: row.clone(),
                    });
                }
                RowChange::Changed { row, .. } => {
                    commands.push(EditCommand::Delete { entity });
                    commands.push(EditCommand::Spawn {
                        entity,
                        rows: vec![row_of(row)],
                        name: self.scene.entity_name(entity).cloned(),
                    });
                }
                RowChange::Removed { .. } if shared => commands.push(detach),
                RowChange::Removed { .. } => commands.push(EditCommand::Delete { entity }),
            }
        }
        commands
    }
}
