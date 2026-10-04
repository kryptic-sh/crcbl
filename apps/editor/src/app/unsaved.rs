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
//! # A scene changed on disk (decided 2026-10-04)
//!
//! A save that finds the scene's files changed since the editor read or
//! wrote them — by a program that took no lock, an older build, a checkout —
//! does not overwrite them: the document refuses
//! ([`EditError::ChangedOnDisk`]) and the bar asks, with its buttons named
//! for this question ([`Guarded::Reload`]):
//!
//! ```text
//!     a save ──changed on disk──▶ the bar ──Overwrite──▶ that save again,
//!                                                        over them
//!                                         ──Reload─────▶ read them back, the
//!                                                        edits here dropped
//!                                         ──Cancel─────▶ nothing
//! ```
//!
//! Asked rather than refused with a status line: a refusal alone would leave
//! the person no way to keep their own edits in place but a save-as
//! elsewhere and a copy back, and overwriting is often what they mean after
//! a checkout they know about. Asked whether or not the scene has unsaved
//! edits, since a clean scene's Save overwrites just the same.
//!
//! **Every save into the scene's own directory asks the same question**
//! (decided 2026-10-05), through one helper ([`Editor::refused_save`]) told
//! which save it was ([`Saving`]): Ctrl+S, a save-as committed onto the
//! directory the scene lives in, and the bar's own Save. Overwrite carries
//! on as that save would have: Ctrl+S says it saved, the save-as says it
//! saved as, and the bar's Save goes on with the close, open or new scene it
//! was asked for. Reload and Cancel differ only for the bar's Save, whose
//! question was asked first and is answered by the second:
//!
//! ```text
//!     close / open / new ──dirty──▶ the bar ──Save──▶ changed on disk ──▶ the bar
//!         ──Overwrite──▶ saved over them, then the close / open / new
//!         ──Reload─────▶ read back, the edits dropped, then the close / open / new
//!         ──Cancel─────▶ nothing: the edits kept, a close kept open
//! ```
//!
//! Reload goes on because the person asked for the close or open and then
//! chose the disk's scene over their edits: nothing is left to lose, and
//! stopping would make them ask twice. A reload refused — the changed files
//! no longer read as a scene — leaves the edits in place and goes on with
//! nothing, as Cancel does; so whichever branch ends the question, a close
//! is either carried out or answered "keep", and the window never holds a
//! request nobody will answer. Cancel never reopens the save-as line: the
//! person said stop.
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
//! was told when to stop, and writes none. The next start offers copies back
//! (`recovery`'s module docs), and an autosave on a timer covers what nothing
//! reports at all: a crash, or a process killed with its session.

use std::path::PathBuf;

use crcbl::engine::{ExitReason, Flow, LoopError, accept_close};
use crcbl::shell::{CloseReply, Shell};

use super::{Editor, EditorError};
use crate::args::Options;
use crate::document::{Document, EditError, PlayState, RECOVERY_DIR};
use crate::keys::Unsaved;
use crate::panel::Tone;

/// What the status line says while the bar asks.
const ASK: &str = "Unsaved edits: Enter saves, D discards, Escape cancels";

/// What the status line says while the bar asks about a scene changed on
/// disk.
const ASK_CHANGED: &str = "Changed on disk: Enter overwrites, D reloads, Escape cancels";

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
    /// The scene read again from its directory, whose files changed on disk
    /// since the editor read or wrote them — what the bar's Reload does,
    /// asked about by the save that found the change; its Overwrite makes
    /// that save again over them instead. See the module docs.
    Reload(Saving),
}

impl Guarded {
    /// Whether this ends with the window closing, so a question about it
    /// that ends otherwise answers the close request "keep".
    fn closes(&self) -> bool {
        match self {
            Self::Close => true,
            Self::Reload(Saving::Then(pending)) => pending.closes(),
            Self::New | Self::Open(_) | Self::Reload(_) => false,
        }
    }
}

/// Which save into the scene's own directory was made, so one that found the
/// scene changed on disk is asked about once and carried on as itself — see
/// the module docs.
#[derive(Debug)]
pub(super) enum Saving {
    /// Ctrl+S, or the toolbar's Save.
    InPlace,
    /// A save-as committed on the path line, holding what was typed.
    As(String),
    /// The unsaved bar's Save, and what it goes on with once saved.
    Then(Box<Guarded>),
}

/// Where recovery copies are written: the directory `--recovery` named, or
/// [`RECOVERY_DIR`] under the system's temporary directory — or nowhere for a
/// headless run that named none.
///
/// Not beside the scene or in the game's folder: a copy there would be one
/// more scene in a tree a person commits, and the asset browser would list
/// it. The log names the copy, and the next start offers it back.
///
/// **A headless run keeps none unless told to**, as it keeps no settings
/// file (`SettingsSource::for_run`): it is a test or a CI job, and a start-up
/// that pruned the person's own recovery directory — or an autosave that
/// wrote into it — would be a test reaching into their work.
pub(super) fn recovery_base(options: &Options) -> Option<PathBuf> {
    match (&options.recovery, options.common.headless) {
        (Some(dir), _) => Some(dir.clone()),
        (None, true) => None,
        (None, false) => Some(std::env::temp_dir().join(RECOVERY_DIR)),
    }
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

    /// What follows a save `refused`, the one `saving` says was made: for
    /// files changed on disk, the bar asking whether to overwrite them or read
    /// them back — see the module docs — and `Ok`; for anything else, that
    /// save offered again and the refusal handed back for the status line. A
    /// save-as reopens its line holding what was typed, and the bar's Save
    /// puts the bar back up.
    ///
    /// # Errors
    ///
    /// `refused`, unless it was [`EditError::ChangedOnDisk`].
    pub(super) fn refused_save(
        &mut self,
        refused: EditError,
        saving: Saving,
    ) -> Result<(), EditError> {
        if matches!(refused, EditError::ChangedOnDisk(_)) {
            self.ask_changed_on_disk(saving);
            return Ok(());
        }
        match saving {
            Saving::InPlace => {}
            Saving::As(text) => {
                // Play mode refuses the line as it refused the save; a typing
                // slip made while play began is refused the line the same
                // way, and logged.
                if !matches!(refused, EditError::Playing)
                    && let Err(line) = self.panels.begin_save_as(&self.document, text)
                {
                    crcbl::log::warn!("editor: {line}");
                }
            }
            Saving::Then(guarded) => {
                let question = self.question(&guarded);
                self.panels.begin_unsaved(question);
                self.unsaved = Some(*guarded);
            }
        }
        Err(refused)
    }

    /// Puts the bar up asking whether to overwrite the scene's files, changed
    /// on disk since the editor read or wrote them, or to read them back,
    /// holding the save that found them changed — see the module docs.
    /// Whatever a Save on the bar was waiting on is dropped, as
    /// [`guard`](Self::guard) drops it.
    fn ask_changed_on_disk(&mut self, saving: Saving) {
        self.after_save = None;
        self.drag = None;
        self.dragged = None;
        let guarded = Guarded::Reload(saving);
        let question = self.question(&guarded);
        self.panels.begin_changed_on_disk(question);
        self.panels.set_status(ASK_CHANGED, Tone::Warning);
        self.unsaved = Some(guarded);
    }

    /// What the bar says about `guarded`: whose edits, and what loses them.
    fn question(&self, guarded: &Guarded) -> String {
        let what = match guarded {
            Guarded::Reload(_) => {
                let dir = self
                    .document
                    .origin()
                    .map_or_else(String::new, |dir| format!(" in `{}`", dir.display()));
                return format!(
                    "The files of `{}`{dir} changed on disk since this editor read or saved \
                     them: Overwrite writes this scene over them, Reload reads them back and \
                     drops the edits here",
                    self.document.name()
                );
            }
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
                self.kept(&guarded);
                Ok(())
            }
            Unsaved::Discard => {
                crcbl::log::warn!(
                    "editor: the unsaved edits to `{}` were discarded",
                    self.document.name()
                );
                self.proceed(guarded)
            }
            Unsaved::Save => match guarded {
                Guarded::Reload(saving) => self.overwrite(saving),
                guarded => self.save_then(guarded),
            },
        }
    }

    /// Makes `saving` again over the scene's files changed on disk, the bar's
    /// Overwrite — see the module docs.
    fn overwrite(&mut self, saving: Saving) -> Result<(), EditError> {
        self.document.accept_changes_on_disk();
        match saving {
            Saving::InPlace => self.save(),
            Saving::As(text) => {
                self.overwrite_as(&text);
                Ok(())
            }
            Saving::Then(pending) => self.save_then(*pending),
        }
    }

    /// Saves the document and goes on with `guarded`; or, for a document with
    /// no directory, asks for one and goes on once the save-as lands; or, for
    /// a refused save, what [`refused_save`](Self::refused_save) does.
    fn save_then(&mut self, guarded: Guarded) -> Result<(), EditError> {
        if self.document.play_state() != PlayState::Editing {
            self.document.stop()?;
        }
        match self.save_in_place() {
            Ok(unwritten) => {
                let reported = self.report_saved("Saved", unwritten.as_ref());
                self.proceed(guarded)?;
                reported
            }
            Err(EditError::NoOrigin) => {
                self.begin_save_as()?;
                self.after_save = Some(guarded);
                Ok(())
            }
            Err(error) => self.refused_save(error, Saving::Then(Box::new(guarded))),
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
            self.kept(&guarded);
        }
    }

    /// Leaves everything as it was once the bar's question ended with
    /// `guarded` not done: a close is kept open, an offer an Open copy took
    /// down comes back (see `recovery`), and the status line says so.
    fn kept(&mut self, guarded: &Guarded) {
        if guarded.closes() {
            self.keep_open();
        }
        self.restore_offer();
        self.panels.set_status(KEPT, Tone::Info);
    }

    /// Does what `guarded` asked for, which ends the document's session: its
    /// autosave goes — see `recovery`.
    fn proceed(&mut self, guarded: Guarded) -> Result<(), EditError> {
        self.end_autosave();
        // Whatever goes on, an offer Open copy took down stays down.
        self.held_offer.clear();
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
            Guarded::Reload(saving) => {
                let reloaded = self.reload();
                match saving {
                    Saving::Then(pending) => match reloaded {
                        Ok(()) => self.proceed(*pending),
                        Err(error) => {
                            self.kept(&pending);
                            Err(error)
                        }
                    },
                    Saving::InPlace | Saving::As(_) => reloaded,
                }
            }
        }
    }

    /// Reads the scene back from its own directory and puts it in place of
    /// the one being edited, under the lock the editor holds on it.
    fn reload(&mut self) -> Result<(), EditError> {
        let dir = self
            .document
            .origin()
            .ok_or(EditError::NoOrigin)?
            .to_path_buf();
        let document = Document::open_with_history_or_fresh(dir, crate::scene::vocabulary())?;
        self.replace_document(document);
        Ok(())
    }

    /// Answers a close request: at once for a clean document, and otherwise
    /// by holding it open while the bar asks — see the module docs. Hands
    /// back the flow that ends the run once the window is closing.
    ///
    /// # Errors
    ///
    /// [`EditorError`] if the shell refused the close.
    pub(super) fn close_requested(&mut self) -> Result<Option<Flow>, EditorError> {
        if self.unsaved.as_ref().is_some_and(Guarded::closes) {
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
    /// Hands back the copy's directory, if one was written. The copy
    /// supersedes the session's autosave, which goes.
    pub(super) fn recover_unsaved(&mut self) -> Option<PathBuf> {
        if !self.document.is_dirty() {
            return None;
        }
        let name = self.document.name().to_owned();
        let Some(base) = self.recovery.clone() else {
            crcbl::log::warn!(
                "editor: the unsaved edits to `{name}` are lost: this run keeps no recovery \
                 directory"
            );
            return None;
        };
        match self
            .document
            .write_recovery(&base, super::recovery::now_millis())
        {
            Ok(dir) => {
                crcbl::log::warn!(
                    "editor: the editor is going without asking, so the unsaved edits to \
                     `{name}` were written to {}",
                    dir.display()
                );
                self.end_autosave();
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
