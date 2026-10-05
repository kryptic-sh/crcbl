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
//! ([`Document::open_with_history_or_fresh`], with this build's vocabulary)
//! **before** the bar asks, so a directory that is not a scene is refused by
//! name, the line opening again holding what was typed, and nothing of the
//! scene being edited is at stake. The scene read is then put in place whole: its asset
//! root is [`crate::document::asset_root`] of it unless `--assets` named one,
//! the panels are built over it afresh — the browser lists its assets, the
//! selection is empty — the history is its own, and the renderer is rebuilt
//! from its assets on the next draw. Refused in play mode, as a new scene is.
//!
//! **An address joins a served scene** (decided 2026-10-05): an `IP:PORT`
//! committed on the line is the scene `crcbl edit --serve` serves there,
//! joined as `--join` joins it, after the bar asks about unsaved edits as
//! for an open — `join`'s module docs. Read as an address before it is read
//! as a path: no scene directory is named like one, and Windows refuses the
//! colon in a name.
//!
//! **The history is the one beside the scene**, `.crcbl-history`, which the
//! `crcbl scene` CLI keeps: Ctrl+Z walks back an edit made from a terminal,
//! and Save writes the history back, so the CLI's `undo` walks back the
//! editor's. A history that is refused is said on the status line, opens as
//! an empty log, and is replaced at the next save — see
//! `crcbl::scene_edit::history`'s module docs for why each.
//!
//! # The scene is locked while it is open (decided 2026-10-04)
//!
//! A scene directory is opened locked ([`lock_scene`], then
//! [`Document::open_locked`]) and the document holds the lock until it lets
//! the scene go: a new scene, another scene or a recovery copy put in its
//! place, a save-as moving it — which then locks the new directory — or the
//! editor closing. So a `crcbl scene` edit run made while the scene is open is
//! refused rather than lost under the editor's next save.
//! `crcbl::scene_edit::lock`'s module docs hold the lock's own decisions.
//!
//! **A scene another program holds is refused, not opened read-only**: by
//! Ctrl+O with the status line naming the holder where it can be read and the
//! scene being edited kept, and by `editor <SCENE_DIR>`, which does not start.
//! A read-only view would need every edit path gated, and would go stale as
//! the holder saves, since the editor reads a scene only when it opens it —
//! a view showing another program's scene as it was is the confusion the
//! lock exists to prevent. Opening the scene this editor already holds reads
//! it again under the lock it has, which the scene read is handed when it is
//! put in place ([`Document::hand_lock_to`]).

use std::net::SocketAddr;

use crcbl::assets::DirSource;
use crcbl::shell::Shell;

use super::unsaved::{Guarded, Saving};
use super::{Editor, EditorError, scene_bounds};
use crate::document::{Document, EditError, PlayState, lock_scene, open_target, save_target};
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
        if let Ok(addr) = text.trim().parse::<SocketAddr>() {
            if let Err(error) = self.ask_join(addr) {
                crcbl::log::warn!("editor: {error}");
                self.panels.set_status(error.to_string(), Tone::Warning);
            }
            return;
        }
        let opened = open_target(text).and_then(|dir| {
            if self.document.play_state() != PlayState::Editing {
                return Err(EditError::Playing);
            }
            if self.document.holds_lock_on(&dir) {
                // The scene this editor holds: read again under its own
                // lock, which a second lock would refuse, and handed it
                // when it is put in place.
                Document::open_with_history_or_fresh(dir, crate::scene::vocabulary())
            } else {
                Document::open_locked(lock_scene(dir)?, crate::scene::vocabulary())
            }
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

    /// Joins the scene served at `addr`, asking first about unsaved edits,
    /// as an open does — see `join`'s module docs. Refused in play mode.
    fn ask_join(&mut self, addr: SocketAddr) -> Result<(), EditError> {
        if self.document.play_state() != PlayState::Editing {
            return Err(EditError::Playing);
        }
        self.guard(Guarded::Join(addr))
    }

    /// Puts `document` in place of the scene being edited — see the module
    /// docs — and says what was opened.
    pub(super) fn replace_document(&mut self, mut document: Document) {
        if let Some(root) = &self.assets {
            document.set_assets(Box::new(DirSource::at(root.clone())));
        }
        self.document.hand_lock_to(&mut document);
        self.document = document;
        self.settle_document();
        let mut notes = self.document.take_recovery_notes();
        notes.extend(super::history_refusal(&mut self.document));
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

    /// Builds what shows the document afresh around the one now in place —
    /// an opened scene, or a joined scene's copy fetched afresh (`join`) —
    /// and logs what it holds: the panels over it, with nothing selected,
    /// the renderer rebuilt from its assets on the next draw, and the view
    /// framed on it.
    pub(super) fn settle_document(&mut self) {
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
    }

    /// Saves the document into the directory `text` names and makes it the
    /// document's own, saying so — or says why not and asks again, holding
    /// what was typed so a slip is corrected rather than retyped. Onto the
    /// scene's own directory, changed on disk since the editor read or wrote
    /// it, the unsaved bar asks whether to overwrite it, as Ctrl+S does
    /// (`unsaved`'s module docs).
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
        let Some(moved) = self.write_as(text) else {
            return Ok(());
        };
        if moved {
            self.panels.relist_assets(&self.document);
            let wanted = self.document.mesh_assets();
            self.rebuild(&wanted)?;
        }
        self.after_saved_as();
        Ok(())
    }

    /// The unsaved bar's Overwrite for a save-as committed onto the scene's
    /// own directory, which found its files changed on disk: the save-as
    /// made again as `text` was typed, over them, and carried on as
    /// [`save_as`](Self::save_as) carries on — see `unsaved`'s module docs.
    pub(super) fn overwrite_as(&mut self, text: &str) {
        let Some(moved) = self.write_as(text) else {
            return;
        };
        // Onto the scene's own directory nothing moves (`Document::save_as`);
        // one that did is rebuilt at the next draw, as an opened scene is,
        // since the bar's answer has no device refusal to hand back.
        if moved {
            self.panels.relist_assets(&self.document);
            self.rebuild_due = true;
        }
        self.after_saved_as();
    }

    /// A save-as's write: the document saved into the directory `text` names
    /// and made the document's own and locked, saying so — handing back
    /// whether the asset source moved with it — or, refused, what
    /// [`refused_save`](Self::refused_save) does, said on the status line,
    /// and [`None`].
    fn write_as(&mut self, text: &str) -> Option<bool> {
        let saved = save_target(text).and_then(|dir| match self.document.save_as(&dir) {
            Ok(moved) => Ok((dir, moved, None)),
            // Only a save-as into the scene's own directory writes the
            // history, and that moves nothing; the scene itself landed.
            Err(EditError::History(unwritten)) => Ok((dir, false, Some(unwritten))),
            Err(error) => Err(error),
        });
        let (dir, moved, unwritten) = match saved {
            Ok(saved) => saved,
            Err(error) => {
                if let Err(error) = self.refused_save(error, Saving::As(text.to_owned())) {
                    crcbl::log::warn!("editor: {error}");
                    self.panels.set_status(error.to_string(), Tone::Warning);
                }
                return None;
            }
        };
        // Before anything that could put another document in place, which
        // would take the copy's path with it.
        self.remove_recovered();
        let saved = format!("Saved as {}", dir.display());
        if let Err(error) = self.report_saved(&saved, unwritten.as_ref()) {
            crcbl::log::warn!("editor: {error}");
            self.panels.set_status(error.to_string(), Tone::Warning);
        }
        // After the save, which needs a scene there to lock: the directory
        // the document now lives in is the one it holds.
        if let Err(error) = self.document.lock_origin() {
            crcbl::log::warn!("editor: saved, but the scene is not locked: {error}");
            self.panels.set_status(
                format!("{saved}, but it is not locked against other programs: {error}"),
                Tone::Warning,
            );
        }
        Some(moved)
    }
}
