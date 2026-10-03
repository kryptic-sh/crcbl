//! The scene as files: a new one from empty, a scene directory opened, and
//! save-as into a directory — the last two typed on the path line. The
//! editor's half of [`Document::new_scene`], [`Document::open_dir`] and
//! [`Document::save_as`], whose docs say what each does to the document.
//!
//! **A new scene and an open ask before unsaved edits are lost**, through the
//! unsaved bar — `unsaved`'s module docs — as closing the window does.
//!
//! # Open
//!
//! Ctrl+O or the toolbar's Open put the path line up for a scene directory;
//! what is committed is checked ([`open_target`]) and read
//! ([`Document::open_dir`], with this build's vocabulary) **before** the bar
//! asks, so a directory that is not a scene is refused by name, the line
//! opening again holding what was typed, and nothing of the scene being
//! edited is at stake. The scene read is then put in place whole: its asset
//! root is [`crate::document::asset_root`] of it unless `--assets` named one,
//! the panels are built over it afresh — the browser lists its assets, the
//! selection is empty — the history is its own, and the renderer is rebuilt
//! from its assets on the next draw. Refused in play mode, as a new scene is.

use crcbl::assets::DirSource;
use crcbl::shell::Shell;

use super::unsaved::Guarded;
use super::{Editor, EditorError, scene_bounds};
use crate::document::{Document, EditError, PlayState, open_target, save_target};
use crate::panel::{Panels, Tone};

/// What the status line says once a new scene is in place.
pub(super) const NEW_SCENE: &str =
    "New scene: drag a mesh from the asset browser into the viewport to start it";

/// What the status line says while a directory is asked for.
const ASK_DIRECTORY: &str = "Save as: type a directory for the scene under the toolbar";

/// What the status line says while a scene directory to open is asked for.
const ASK_SCENE: &str = "Open: type a scene directory under the toolbar";

impl<S: Shell + ?Sized> Editor<S> {
    /// Puts a new, empty scene in place of the one being edited — asking
    /// first if it has unsaved edits. Refused in play mode.
    pub(super) fn ask_new_scene(&mut self) -> Result<(), EditError> {
        if self.document.play_state() != PlayState::Editing {
            return Err(EditError::Playing);
        }
        self.guard(Guarded::New)
    }

    /// Puts a new, empty scene in place of the one being edited, frames it,
    /// and says so — naming the scene whose unsaved edits went with it, if
    /// any did.
    pub(super) fn new_scene(&mut self) -> Result<(), EditError> {
        let dropped = self
            .document
            .is_dirty()
            .then(|| self.document.name().to_owned());
        self.document.new_scene()?;
        // A drag of the old scene's handles or of an asset held over from
        // the frame before has nothing left to land on.
        self.drag = None;
        self.dragged = None;
        self.frame_scene();
        match dropped {
            None => self.panels.set_status(NEW_SCENE, Tone::Info),
            Some(name) => {
                self.panels.set_status(
                    format!("{NEW_SCENE}. The unsaved edits to `{name}` were discarded"),
                    Tone::Warning,
                );
            }
        }
        Ok(())
    }

    /// Opens the path line for a save-as, holding the directory the document
    /// came from if it came from one — or, for a recovered scene, the one
    /// its copy recorded it living in. Into that directory it is refused
    /// like any other occupied one: the line only offers it.
    pub(super) fn begin_save_as(&mut self) -> Result<(), EditError> {
        let text = self.origin_text();
        self.panels.begin_save_as(&self.document, text)?;
        self.panels.set_status(ASK_DIRECTORY, Tone::Info);
        Ok(())
    }

    /// Opens the path line for an open, holding the directory the document's
    /// own directory is in, with a separator after it, if it came from one —
    /// where the next scene most likely is, so what is typed is its name.
    pub(super) fn begin_open(&mut self) -> Result<(), EditError> {
        let text = self
            .document
            .origin()
            .and_then(std::path::Path::parent)
            .map(|holding| format!("{}{}", holding.display(), std::path::MAIN_SEPARATOR))
            .unwrap_or_default();
        self.panels.begin_open(&self.document, text)?;
        self.panels.set_status(ASK_SCENE, Tone::Info);
        Ok(())
    }

    /// The directory the document came from, or else the one its recovery
    /// copy recorded, as the path line shows it — or nothing for a document
    /// with neither.
    fn origin_text(&self) -> String {
        self.document
            .origin()
            .or_else(|| self.document.recorded_origin())
            .map(|origin| origin.display().to_string())
            .unwrap_or_default()
    }

    /// Reads the scene directory `text` names and puts it in place of the
    /// scene being edited, asking first about unsaved edits — or says why not
    /// and asks again, holding what was typed. See the module docs.
    pub(super) fn open(&mut self, text: &str) {
        let opened = open_target(text).and_then(|dir| {
            if self.document.play_state() != PlayState::Editing {
                return Err(EditError::Playing);
            }
            Document::open_dir(dir, crate::scene::vocabulary())
        });
        let outcome = match opened {
            Ok(document) => self.guard(Guarded::Open(Box::new(document))),
            Err(error) => {
                // Play mode refuses the line as it refused the open; anything
                // else is a slip to correct rather than retype.
                if !matches!(error, EditError::Playing)
                    && let Err(refused) = self.panels.begin_open(&self.document, text.to_owned())
                {
                    crcbl::log::warn!("editor: {refused}");
                }
                Err(error)
            }
        };
        if let Err(error) = outcome {
            crcbl::log::warn!("editor: {error}");
            self.panels.set_status(error.to_string(), Tone::Warning);
        }
    }

    /// Puts `document` in place of the scene being edited — see the module
    /// docs — and says what was opened.
    pub(super) fn replace_document(&mut self, mut document: Document) {
        if let Some(root) = &self.assets {
            document.set_assets(Box::new(DirSource::at(root.clone())));
        }
        self.document = document;
        super::log_outline(&mut self.document);
        self.panels = Panels::new(
            &mut self.document,
            self.panels.layout().clone(),
            self.gpu.extent(),
        );
        // What was held over from the old scene has nothing left to land on:
        // a drag of its handles or of an asset, and a paste asked for it.
        self.drag = None;
        self.dragged = None;
        self.paste = crate::clipboard::Paste::default();
        self.grid_extent = super::grid_extent(&scene_bounds(&mut self.document));
        self.rebuild_due = true;
        self.frame_scene();
        // The panels are new, so the offer is put back up if it stands.
        self.show_offer();
        let notes = self.document.take_recovery_notes();
        let mut opened = match (self.document.origin(), self.document.recorded_origin()) {
            (Some(dir), _) => format!("Opened {}", dir.display()),
            // Only a recovery copy is opened with no directory.
            (None, None) => format!(
                "Opened a recovery copy of `{}`: saving asks for a directory",
                self.document.name()
            ),
            (None, Some(lived)) => format!(
                "Opened a recovery copy of `{}`: saving asks for a directory, starting from \
                 `{}`, where it lived",
                self.document.name(),
                lived.display()
            ),
        };
        for note in &notes {
            crcbl::log::warn!("editor: {note}");
            opened.push_str("; ");
            opened.push_str(note);
        }
        let tone = if notes.is_empty() {
            Tone::Info
        } else {
            Tone::Warning
        };
        self.panels.set_status(opened, tone);
    }

    /// Saves the document into the directory `text` names and makes it the
    /// document's own, saying so — or says why not and asks again, holding
    /// what was typed so a slip is corrected rather than retyped.
    ///
    /// When the asset source moved with the document, the browser lists the
    /// new one and the renderer is rebuilt from it: the same keys name other
    /// files there.
    ///
    /// # Errors
    ///
    /// [`EditorError`] only when the device refused the rebuilt renderer; a
    /// refused save is on the status line.
    pub(super) fn save_as(&mut self, text: &str) -> Result<(), EditorError> {
        let saved = save_target(text).and_then(|dir| {
            let moved = self.document.save_as(&dir)?;
            Ok((dir, moved))
        });
        let (dir, moved) = match saved {
            Ok(saved) => saved,
            Err(error) => {
                crcbl::log::warn!("editor: {error}");
                self.panels.set_status(error.to_string(), Tone::Warning);
                // Play mode refuses the line as it refused the save, and the
                // status line already says so; a typing slip made while play
                // began is refused the line the same way, and logged.
                if !matches!(error, EditError::Playing)
                    && let Err(refused) = self.panels.begin_save_as(&self.document, text.to_owned())
                {
                    crcbl::log::warn!("editor: {refused}");
                }
                return Ok(());
            }
        };
        // Before anything that could put another document in place, which
        // would take the copy's path with it.
        self.remove_recovered();
        if moved {
            self.panels.relist_assets(&self.document);
            let wanted = self.document.mesh_assets();
            self.rebuild(&wanted)?;
        }
        let saved = format!("Saved as {}", dir.display());
        if let Err(error) = self.report_saved(&saved) {
            crcbl::log::warn!("editor: {error}");
            self.panels.set_status(error.to_string(), Tone::Warning);
        }
        self.after_saved_as();
        Ok(())
    }
}
