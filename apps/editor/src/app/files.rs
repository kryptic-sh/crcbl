//! The scene as files: a new one from empty, and save-as into a directory
//! typed on the save-as line — the editor's half of
//! [`Document::new_scene`](crate::document::Document::new_scene) and
//! [`Document::save_as`](crate::document::Document::save_as), whose module
//! docs say what each does to the document.
//!
//! **Unsaved edits are dropped by a new scene and the status line says so**,
//! as closing the window drops them: the editor has no prompt for either, and
//! a second, different rule for one of them would be the surprise.

use crcbl::shell::Shell;

use super::{Editor, EditorError};
use crate::document::{EditError, save_target};
use crate::panel::Tone;

/// What the status line says once a new scene is in place.
pub(super) const NEW_SCENE: &str =
    "New scene: drag a mesh from the asset browser into the viewport to start it";

/// What the status line says while a directory is asked for.
const ASK_DIRECTORY: &str = "Save as: type a directory for the scene under the toolbar";

impl<S: Shell + ?Sized> Editor<S> {
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
                crcbl::log::warn!("editor: a new scene dropped the unsaved edits to `{name}`");
                self.panels.set_status(
                    format!("{NEW_SCENE}. The unsaved edits to `{name}` were dropped"),
                    Tone::Warning,
                );
            }
        }
        Ok(())
    }

    /// Opens the save-as line, holding the directory the document came from
    /// if it came from one.
    pub(super) fn begin_save_as(&mut self) -> Result<(), EditError> {
        let text = self
            .document
            .origin()
            .map(|origin| origin.display().to_string())
            .unwrap_or_default();
        self.panels.begin_save_as(&self.document, text)?;
        self.panels.set_status(ASK_DIRECTORY, Tone::Info);
        Ok(())
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
        Ok(())
    }
}
