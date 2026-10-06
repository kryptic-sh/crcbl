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
//! * **What is no change is not asked about.** A chunk file holding exactly
//!   what the document last read or wrote there is the document's own save
//!   seen by a watch, or a file nobody changed, and reloads nothing; one
//!   whose rows all match the document's, a change of layout alone, is taken
//!   in without asking, since taking it changes no row.
//! * **The scene lock does not stop it.** The lock binds programs that take
//!   it, and a change on disk while one is held came from one that did not —
//!   a text editor, a checkout. Seeing that change is the point.
//! * **After a reload the chunk file is what the document last read**, and
//!   no other file is (`scene_edit::lock`'s one digest per file): the save
//!   that follows does not refuse over the very change it just took in, and
//!   still asks over another file changed at the same moment.
//! * **Play mode refuses** ([`EditError::Playing`]), as every edit does: the
//!   played world is thrown away at stop. **A routed copy refuses**
//!   ([`EditError::Routed`]): its server's document is the one with a
//!   directory, and a reload routed as an edit would ask for fresh ids and
//!   lose the file's.
//!
//! # Revert is every chunk reloaded (decided 2026-10-06, for the long term)
//!
//! [`Document::revert`] — the editor's "read the scene back from disk" —
//! goes through the same difference and the same commands as a reload, for
//! every chunk the manifest lists, then the names and the environment, so a
//! revert and a reload cannot come to disagree about what the disk says.
//!
//! * **One entry of the history**, after which the document is clean: the
//!   edits it drops are beneath it, and one undo brings them back, as an
//!   undo walks back a reload.
//! * **The header is not reverted**: a manifest or a scene name changed on
//!   disk is [`EditError::HeaderChanged`], changing nothing, since listing
//!   and unlisting systems in place is the header reload the backlog defers;
//!   the caller reads the scene again whole instead.
//! * **The whole directory is what the document last read** afterwards,
//!   since a revert read every file of it.

use std::path::Path;

use crate::assets::{AssetSource, DirSource};
use crate::reflect::get_path;
use crate::scene::edit::{EditCommand, SystemRow};
use crate::scene::scn::{ChunkDiff, RowChange, Scene, chunk_text, diff_chunk};

use super::environment::Environment;
use super::field::texts_of;
use super::{Document, EditError, Performed, load};

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
    /// What the file held that the document did not, row by row — or
    /// [`None`] for a file holding what the document last read or wrote
    /// there, which is no change on disk and is not compared.
    pub diff: Option<ChunkDiff>,
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
    /// [`EditError::NoSystem`] for a system the manifest does not list or the
    /// vocabulary cannot read, [`EditError::Scene`] for a file that will not
    /// read or is not the system's chunk — a write caught half-finished among
    /// them — and [`EditError::Unsaved`] for a file that changes rows of a
    /// document with unsaved edits, under [`OverEdits::Refuse`]. **Every
    /// refusal changes nothing**, so the document keeps the last good state.
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
        let codec = self.listed_codec(system)?;
        // Rooted at the scene directory and read with an empty prefix, as
        // `Document::open_dir` reads it.
        let source = DirSource::at(origin);
        let (key, text) = chunk_text(&source, Path::new(""), system)?;
        if self.last_read(&key, text.as_bytes()) {
            return Ok(Reloaded {
                diff: None,
                command: None,
            });
        }
        let diff = diff_chunk(codec.as_ref(), &mut self.world, &self.ids, &key, &text)?;
        let clean = !self.is_dirty();
        if !clean && !diff.is_empty() && over_edits == OverEdits::Refuse {
            return Err(EditError::Unsaved(system.to_owned()));
        }
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
        self.record_file(&key, text.as_bytes());
        Ok(Reloaded {
            diff: Some(diff),
            command,
        })
    }

    /// Reads the whole scene back from the document's own directory and
    /// makes the document hold what it holds — every chunk through a
    /// reload's difference, then the names and the environment — as one
    /// entry of the history, after which the document is clean. Hands back
    /// whether anything changed. See the `scene_edit::reload` module docs.
    ///
    /// # Errors
    ///
    /// [`EditError::Playing`] in play mode, [`EditError::Routed`] for a
    /// routed copy, [`EditError::NoOrigin`] for a document with no directory,
    /// [`EditError::Scene`] for a scene on disk that will not read,
    /// [`EditError::HeaderChanged`] for a manifest or a name changed on disk,
    /// and a refusal of any command the revert came to — a row the
    /// component's rule refuses among them. **Every refusal changes
    /// nothing.**
    pub fn revert(&mut self) -> Result<bool, EditError> {
        self.refuse_in_play()?;
        if self.is_routed() {
            return Err(EditError::Routed);
        }
        let origin = self.origin.clone().ok_or(EditError::NoOrigin)?;
        let source = DirSource::at(origin.clone());
        // Read whole first, so a scene that will not read refuses before
        // anything changes.
        let (_, disk, _) = load(&source, Path::new(""), &self.registry)?;
        if disk.systems() != self.scene.systems() || disk.name() != self.scene.name() {
            return Err(EditError::HeaderChanged(origin));
        }
        let mut performed = Vec::new();
        if let Err(error) = self.revert_steps(&source, &disk, &mut performed) {
            for step in performed.iter().rev() {
                self.perform(&step.undo)
                    .expect("an inverse produced a moment ago applies");
                self.sync_written(&step.undo);
            }
            return Err(error);
        }
        let changed = !performed.is_empty();
        if changed {
            let (done, mut undo): (Vec<_>, Vec<_>) = performed
                .into_iter()
                .map(|step| (step.done, step.undo))
                .unzip();
            undo.reverse();
            self.resolve_meshes();
            self.log
                .record(EditCommand::Batch(done), EditCommand::Batch(undo));
        }
        self.log.mark_saved();
        self.record_disk();
        Ok(changed)
    }

    /// Performs what makes the document hold `disk`, read through `source`,
    /// pushing each step onto `performed` as it lands: each chunk's
    /// difference against what the steps before it left, then the names,
    /// then the environment — [`revert`](Self::revert)'s body.
    fn revert_steps(
        &mut self,
        source: &dyn AssetSource,
        disk: &Scene,
        performed: &mut Vec<Performed>,
    ) -> Result<(), EditError> {
        for system in self.scene.systems().to_vec() {
            let codec = self.listed_codec(&system)?;
            let (key, text) = chunk_text(source, Path::new(""), &system)?;
            let diff = diff_chunk(codec.as_ref(), &mut self.world, &self.ids, &key, &text)?;
            let commands = self.commands_of(&diff);
            if !commands.is_empty() {
                performed.push(self.perform_valid(&EditCommand::Batch(commands))?);
            }
        }
        let theirs = disk.entity_names();
        let ours = self.scene.entity_names();
        let renames: Vec<EditCommand> = theirs
            .iter()
            .filter(|(id, name)| ours.get(id) != Some(name))
            .map(|(id, name)| EditCommand::Rename {
                entity: *id,
                name: Some(name.clone()),
            })
            .chain(ours.keys().filter(|id| !theirs.contains_key(id)).map(|id| {
                EditCommand::Rename {
                    entity: *id,
                    name: None,
                }
            }))
            .collect();
        if !renames.is_empty() {
            performed.push(self.perform_valid(&EditCommand::Batch(renames))?);
        }
        let theirs = Environment::from(disk.env());
        let (mut their_texts, mut our_texts) = (Vec::new(), Vec::new());
        texts_of(&theirs, "", &mut their_texts);
        texts_of(&self.environment(), "", &mut our_texts);
        let mut writes = Vec::new();
        for ((path, text), (_, ours)) in their_texts.iter().zip(&our_texts) {
            if text != ours {
                writes.push(EditCommand::SetEnvironment {
                    path: path.clone(),
                    value: get_path(&theirs, path)?,
                });
            }
        }
        if !writes.is_empty() {
            performed.push(self.perform_valid(&EditCommand::Batch(writes))?);
        }
        Ok(())
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
