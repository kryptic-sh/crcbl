//! Asking before unsaved edits are lost: the unsaved bar's flow, the window's
//! close request held open while it asks, and a recovery copy where nothing
//! could be asked.
//!
//! # The bar (decided 2026-10-03)
//!
//! A new scene, an open and the window closing each go through
//! [`Editor::guard`]. A clean document goes straight on; a dirty one puts the
//! unsaved bar up (`crate::panel`'s `unsaved` module) saying what would be
//! lost, and the editor waits for its answer — Enter, D or Escape, or a click
//! on its buttons — with every other action held:
//!
//! ```text
//!     guard ──clean──▶ go on
//!           ──dirty──▶ the bar ──Discard──▶ go on, the edits dropped
//!                              ──Cancel───▶ nothing (a close is kept open)
//!                              ──Save─────▶ save ──▶ go on
//!                                           no directory ──▶ the save-as line
//!                                               ──saved──▶ go on
//!                                               ──cancelled──▶ nothing
//!                                           refused ──▶ the bar again
//! ```
//!
//! An open asks once the directory typed has been read as a scene, so a
//! directory that is not one is refused before anything is asked, and the
//! scene being edited is never at stake for a typing slip. A Save while the
//! scene plays — only a close can ask then, since a new scene and an open are
//! refused in play — stops play first, so it is the authored scene that is
//! saved.
//!
//! # Which backends hold a close, and which recover
//!
//! **Every backend this editor runs on holds a close request open** until the
//! application answers it ([`crcbl::shell::CloseReply`]): Win32 intercepts
//! `WM_CLOSE` rather than passing it to `DefWindowProc`, AppKit's
//! `windowShouldClose:` answers `NO`, X11's `WM_DELETE_WINDOW` and Wayland's
//! `xdg_toplevel.close` are requests the client acts on or not, and the
//! headless shell models the same. So a close request with unsaved edits is
//! left unanswered while the bar is up, accepted by Save or Discard, and
//! answered [`CloseReply::Keep`] by Cancel.
//! The browser's shell asks too (`__crcbl_web_close`), but the editor is a
//! native target with no web build, and a tab closing is no request at all.
//!
//! **What cannot be held is recovered.** A window taken away without a request
//! — a [`WindowDestroyed`](crcbl::shell::ShellEvent::WindowDestroyed) every
//! backend reports when the window system ends it from outside — and a run
//! whose frame fails leave nothing to ask, so a dirty scene is written to a
//! recovery copy ([`Document::write_recovery`]) under [`recovery_base`] and the
//! log says where. A run that ends on its frame budget or limit is a run that
//! was told when to stop, and writes none.

use std::path::PathBuf;
use std::time::{SystemTime, UNIX_EPOCH};

use crcbl::engine::{ExitReason, Flow, LoopError, accept_close};
use crcbl::shell::{CloseReply, Shell};

use super::{Editor, EditorError};
use crate::document::{Document, EditError, PlayState, RECOVERY_DIR};
use crate::keys::Unsaved;
use crate::panel::Tone;

/// What the status line says while the bar asks.
const ASK: &str = "Unsaved edits: Enter saves, D discards, Escape cancels";

/// What the status line says once the bar is cancelled.
const KEPT: &str = "Cancelled: the scene and its unsaved edits are as they were";

/// Something that would lose unsaved edits, held while the bar asks.
#[derive(Debug)]
pub(super) enum Guarded {
    /// A new, empty scene.
    New,
    /// The scene opened from a typed directory, read already, to put in place.
    Open(Box<Document>),
    /// The window closing.
    Close,
}

/// Where recovery copies are written: [`RECOVERY_DIR`] under the system's
/// temporary directory.
///
/// Not beside the scene or in the game's folder: a copy there would be one
/// more scene in a tree a person commits, and the asset browser would list
/// it. The log names the copy, so it is found where it is.
pub(super) fn recovery_base() -> PathBuf {
    std::env::temp_dir().join(RECOVERY_DIR)
}

impl<S: Shell + ?Sized> Editor<S> {
    /// Goes on with `guarded` if the document is clean, and otherwise puts
    /// the unsaved bar up to ask first — see the module docs. Whatever was
    /// already waiting on an answer, or on a save-as a Save asked for, is
    /// dropped: the newest request is the one asked about.
    pub(super) fn guard(&mut self, guarded: Guarded) -> Result<(), EditError> {
        self.after_save = None;
        if !self.document.is_dirty() {
            self.unsaved = None;
            self.panels.end_unsaved();
            return self.proceed(guarded);
        }
        // A drag of the scene's handles or of an asset has nothing to land
        // on while the scene is held.
        self.drag = None;
        self.dragged = None;
        let question = self.question(&guarded);
        self.panels.begin_unsaved(question);
        self.panels.set_status(ASK, Tone::Warning);
        self.unsaved = Some(guarded);
        Ok(())
    }

    /// What the bar says about `guarded`: whose edits, and what loses them.
    fn question(&self, guarded: &Guarded) -> String {
        let what = match guarded {
            Guarded::New => "to a new scene".to_owned(),
            Guarded::Open(opened) => match opened.origin() {
                Some(dir) => format!("by opening `{}`", dir.display()),
                None => format!("by opening `{}`", opened.name()),
            },
            Guarded::Close => "when the window closes".to_owned(),
        };
        format!(
            "Unsaved edits to `{}` would be lost {what}",
            self.document.name()
        )
    }

    /// Carries out the bar's answer, if the bar is up — see the module docs.
    ///
    /// # Errors
    ///
    /// What the save or what was asked for refused, which the caller puts on
    /// the status line. A refused save puts the bar back up.
    pub(super) fn answer(&mut self, answer: Unsaved) -> Result<(), EditError> {
        let Some(guarded) = self.unsaved.take() else {
            return Ok(());
        };
        self.panels.end_unsaved();
        match answer {
            Unsaved::Cancel => {
                if matches!(guarded, Guarded::Close) {
                    self.keep_open();
                }
                self.panels.set_status(KEPT, Tone::Info);
                Ok(())
            }
            Unsaved::Discard => {
                crcbl::log::warn!(
                    "editor: the unsaved edits to `{}` were discarded",
                    self.document.name()
                );
                self.proceed(guarded)
            }
            Unsaved::Save => self.save_then(guarded),
        }
    }

    /// Saves the document and goes on with `guarded`; or, for a document with
    /// no directory, asks for one and goes on once the save-as lands; or puts
    /// the bar back up when the save is refused.
    fn save_then(&mut self, guarded: Guarded) -> Result<(), EditError> {
        if self.document.play_state() != PlayState::Editing {
            self.document.stop()?;
        }
        match self.document.save() {
            Ok(()) => {
                let reported = self.report_saved("Saved");
                self.proceed(guarded)?;
                reported
            }
            Err(EditError::NoOrigin) => {
                self.begin_save_as()?;
                self.after_save = Some(guarded);
                Ok(())
            }
            Err(error) => {
                let question = self.question(&guarded);
                self.panels.begin_unsaved(question);
                self.unsaved = Some(guarded);
                Err(error)
            }
        }
    }

    /// Goes on with what a save-as asked for by a Save on the bar was waiting
    /// on, now that it landed.
    pub(super) fn after_saved_as(&mut self) {
        if let Some(guarded) = self.after_save.take()
            && let Err(error) = self.proceed(guarded)
        {
            crcbl::log::warn!("editor: {error}");
            self.panels.set_status(error.to_string(), Tone::Warning);
        }
    }

    /// Drops what a Save on the bar was waiting on once the save-as line it
    /// asked for has closed without saving — the person cancelled it, or
    /// asked for something else — and says so.
    pub(super) fn follow_after_save(&mut self) {
        if self.panels.saving_as().is_some() {
            return;
        }
        if let Some(guarded) = self.after_save.take() {
            if matches!(guarded, Guarded::Close) {
                self.keep_open();
            }
            self.panels.set_status(KEPT, Tone::Info);
        }
    }

    /// Does what `guarded` asked for.
    fn proceed(&mut self, guarded: Guarded) -> Result<(), EditError> {
        match guarded {
            Guarded::New => self.new_scene(),
            Guarded::Open(document) => {
                self.replace_document(*document);
                Ok(())
            }
            Guarded::Close => {
                self.closing = true;
                Ok(())
            }
        }
    }

    /// Answers a close request: at once for a clean document, and otherwise
    /// by holding it open while the bar asks — see the module docs. Hands
    /// back the flow that ends the run once the window is closing.
    ///
    /// # Errors
    ///
    /// [`EditorError`] if the shell refused the close.
    pub(super) fn close_requested(&mut self) -> Result<Option<Flow>, EditorError> {
        if matches!(self.unsaved, Some(Guarded::Close)) {
            return Ok(None);
        }
        self.guard(Guarded::Close).map_err(LoopError::Game)?;
        self.close_if_asked()
    }

    /// Closes the window and hands back the flow that ends the run, once
    /// something asked for the window to close.
    ///
    /// # Errors
    ///
    /// [`EditorError`] if the shell refused the close.
    pub(super) fn close_if_asked(&mut self) -> Result<Option<Flow>, EditorError> {
        if !self.closing {
            return Ok(None);
        }
        accept_close(self.shell.as_mut(), self.window)?;
        Ok(Some(Flow::Stop(ExitReason::CloseRequested)))
    }

    /// Answers an outstanding close request with "keep it open", once the
    /// person said not to close.
    fn keep_open(&mut self) {
        let kept = match self.shell.window_state(self.window) {
            Ok(state) if state.close_pending => self
                .shell
                .reply_close_request(self.window, CloseReply::Keep),
            Ok(_) => Ok(()),
            Err(error) => Err(error),
        };
        if let Err(error) = kept {
            crcbl::log::warn!("editor: the close request would not be answered: {error}");
        }
    }

    /// Writes a dirty document to a recovery copy and says in the log where,
    /// for a run ending where nothing could be asked — see the module docs.
    /// Hands back the copy's directory, if one was written.
    pub(super) fn recover_unsaved(&mut self) -> Option<PathBuf> {
        if !self.document.is_dirty() {
            return None;
        }
        let name = self.document.name().to_owned();
        // A clock set before 1970 names the copy 0: the name is a label, and
        // `write_recovery` never reuses one that is taken.
        let stamp = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map_or(0, |since| since.as_millis());
        match self.document.write_recovery(&self.recovery, stamp) {
            Ok(dir) => {
                crcbl::log::warn!(
                    "editor: the editor is going without asking, so the unsaved edits to \
                     `{name}` were written to {}",
                    dir.display()
                );
                Some(dir)
            }
            Err(error) => {
                crcbl::log::error!(
                    "editor: the unsaved edits to `{name}` are lost: no recovery copy was \
                     written: {error}"
                );
                None
            }
        }
    }
}
