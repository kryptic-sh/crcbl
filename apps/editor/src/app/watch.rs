//! The scene's chunk files watched while it is edited: a chunk another
//! program changed — a text editor, a checkout, a `crcbl` run that took no
//! lock — is reloaded into the document, or asked about first.
//!
//! The watch is the document's (`crcbl::scene_edit::ChunkWatch`), polled
//! once a frame on the editor's own clock, and the reload is the document's
//! ([`Document::reload_chunk`]): only the changed rows of the changed chunk,
//! as one entry of the history. `crcbl::scene_edit`'s `reload` module docs
//! hold the rule against unsaved edits; this is the editor's half of it.
//!
//! # What the editor does with a changed chunk (decided 2026-10-06)
//!
//! * **A clean scene reloads at once and stays clean**, the status line
//!   saying which file came in: one entry, so Ctrl+Z puts the scene back as
//!   it was, and the scene is then dirty.
//! * **A scene with unsaved edits asks**, on the unsaved bar
//!   ([`Asking::ChunkChanged`](crate::panel::Asking::ChunkChanged)): **Keep
//!   mine** (Enter, or Escape) changes nothing — the edits stay, the file
//!   stays as the other program left it, and the next save into the scene
//!   asks whether to overwrite it, as it asks today over any file changed on
//!   disk (`unsaved`'s module docs); **Reload from disk** (D) takes the
//!   file's rows over the edits as one more entry on top
//!   ([`OverEdits::Reload`]), so the edits are still in the history beneath
//!   it and the scene stays dirty. Every chunk changed at once is asked
//!   about in one question.
//! * **A change is held, not dropped, while the editor cannot take it**:
//!   while the bar asks something else, while the scene plays — the played
//!   world is thrown away at stop — and while a gizmo drag is held. It is
//!   reloaded, or asked about, in the first frame that can; so a change
//!   made during play comes in once play stops, and the status line says
//!   so when it is seen.
//! * **The editor's own saves are no change**: a chunk file holding what the
//!   document last wrote there reloads nothing and asks nothing.
//! * **A joined scene watches nothing**: its copy has no directory, and its
//!   server watches the files (`crcbl edit --serve`).
//!
//! # A browser build watches nothing
//!
//! It opens no scene directory — [`crate::document::lock_scene`] and the
//! open read the filesystem, which `wasm32` has not — and the engine's watch
//! is absent there (`crcbl::assets::watch`'s _Native only_), so its
//! [`Disk`] offers nothing.

use std::time::Duration;

use crcbl::shell::Shell;

use super::unsaved::Guarded;
use super::{Drag, Editor};
use crate::document::{Document, EditError, OverEdits, PlayState};
use crate::panel::Tone;

#[cfg(not(target_arch = "wasm32"))]
use crate::document::ChunkWatch;

/// The document's chunk files, watched — see the module docs.
#[derive(Debug)]
pub(super) struct Disk {
    #[cfg(not(target_arch = "wasm32"))]
    watch: ChunkWatch,
}

#[cfg(not(target_arch = "wasm32"))]
impl Disk {
    /// Watches `document`'s chunk files as they stand at `now`.
    pub(super) fn new(document: &Document, now: Duration) -> Self {
        Self {
            watch: ChunkWatch::new(document, now),
        }
    }

    /// Looks at `document`'s chunk files at `now`, handing back the systems
    /// whose chunk changed in this look — see [`ChunkWatch::poll`].
    fn poll(&mut self, document: &Document, now: Duration) -> Vec<String> {
        self.watch.poll(document, now)
    }

    /// Every changed chunk not yet taken, forgotten — see
    /// [`ChunkWatch::take_due`].
    fn take_due(&mut self) -> Vec<String> {
        self.watch.take_due()
    }
}

/// A browser build's: nothing to watch — see the module docs.
#[cfg(target_arch = "wasm32")]
impl Disk {
    /// Watches nothing.
    pub(super) const fn new(_document: &Document, _now: Duration) -> Self {
        Self {}
    }

    /// Offers nothing.
    fn poll(&mut self, _document: &Document, _now: Duration) -> Vec<String> {
        Vec::new()
    }

    /// Holds nothing.
    fn take_due(&mut self) -> Vec<String> {
        Vec::new()
    }
}

/// What the status line says while the bar asks about changed chunks.
const ASK_CHUNK: &str = "Changed on disk: Enter or Escape keeps your edits, D reloads the file";

impl<S: Shell + ?Sized> Editor<S> {
    /// Polls the scene's chunk files on the editor's clock, and reloads each
    /// changed one — or asks about them, or holds them — see the module
    /// docs.
    pub(super) fn follow_disk(&mut self) {
        let settled = self.disk.poll(&self.document, self.elapsed);
        let playing = self.document.play_state() != PlayState::Editing;
        if playing && !settled.is_empty() {
            self.panels.set_status(
                format!(
                    "{} changed on disk: it reloads when play stops",
                    files(&settled)
                ),
                Tone::Info,
            );
        }
        let dragging = matches!(self.drag, Some(Drag::Gizmo(..)));
        if playing || dragging || self.unsaved.is_some() {
            return;
        }
        let mut asked = Vec::new();
        for system in self.disk.take_due() {
            match self.document.reload_chunk(&system, OverEdits::Refuse) {
                Ok(reloaded) => {
                    if reloaded.command.is_some() {
                        self.reloaded(&system);
                    }
                }
                Err(EditError::Unsaved(system)) => asked.push(system),
                Err(error) => self.not_reloaded(&system, &error),
            }
        }
        if !asked.is_empty() {
            self.ask_chunks_changed(asked);
        }
    }

    /// Puts the bar up asking whether to keep the edits or reload `systems`'
    /// chunks over them — see the module docs.
    fn ask_chunks_changed(&mut self, systems: Vec<String>) {
        self.drag = None;
        self.dragged = None;
        let guarded = Guarded::Chunks(systems);
        let question = self.question(&guarded);
        self.panels.begin_chunk_changed(question);
        self.panels.set_status(ASK_CHUNK, Tone::Warning);
        self.unsaved = Some(guarded);
    }

    /// The bar's Reload from disk: each of `systems`' chunks reloaded over
    /// the edits, as one entry each — see the module docs.
    pub(super) fn reload_over_edits(&mut self, systems: &[String]) {
        for system in systems {
            match self.document.reload_chunk(system, OverEdits::Reload) {
                Ok(reloaded) => {
                    if reloaded.command.is_some() {
                        self.reloaded(system);
                    }
                }
                Err(error) => self.not_reloaded(system, &error),
            }
        }
    }

    /// The bar's Keep mine: nothing changes, and the status line says what
    /// the next save will ask.
    pub(super) fn keep_over_disk(&mut self, systems: &[String]) {
        self.panels.set_status(
            format!(
                "Kept your edits: {} stays as it is on disk, and the next save asks before \
                 writing over it",
                files(systems)
            ),
            Tone::Info,
        );
    }

    /// Says on the status line that `system`'s chunk came in from disk.
    fn reloaded(&mut self, system: &str) {
        crcbl::log::info!("editor: reloaded `sys/{system}.ron` from disk");
        self.panels.set_status(
            format!("Reloaded `sys/{system}.ron` from disk: Ctrl+Z puts the scene back"),
            Tone::Info,
        );
    }

    /// Says on the status line and in the log why `system`'s changed chunk
    /// was not reloaded; the scene keeps what it had.
    fn not_reloaded(&mut self, system: &str, error: &EditError) {
        crcbl::log::warn!("editor: `sys/{system}.ron` changed on disk, not reloaded: {error}");
        self.panels.set_status(
            format!("`sys/{system}.ron` changed on disk and was not reloaded: {error}"),
            Tone::Warning,
        );
    }
}

/// `systems`' chunk files, named for a sentence.
pub(super) fn files(systems: &[String]) -> String {
    systems
        .iter()
        .map(|system| format!("`sys/{system}.ron`"))
        .collect::<Vec<_>>()
        .join(", ")
}
