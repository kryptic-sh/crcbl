//! Recovery copies offered back at start-up, the recovery directory kept
//! tidy, and the autosave that writes into it.
//!
//! # Offered, never forced (decided 2026-10-03)
//!
//! At start-up — whether or not a scene was named on the command line, since
//! a copy of that very scene is the likeliest one wanted — the recovery
//! directory is pruned and, if copies are left, the newest [`OFFERED`] are
//! listed on the recovery bar (`crate::panel`'s `recovery` module) with how
//! long ago each was written:
//!
//! * **Open copy** reads it with [`Document::open_recovery`] — no origin, so a
//!   save asks for a directory and a copy is never written back over itself,
//!   and dirty — and puts it in place through Open's own path: the unsaved bar
//!   asks first about edits to the scene being edited. Refused in play mode,
//!   as an open is. The bar goes away while the unsaved bar asks; once the
//!   copy is opened the other copies are offered again at the next start,
//!   and if the question ends with nothing opened — Cancel, or a Save whose
//!   save-as line was closed unsaved — the bar comes back as it was.
//! * **Delete** removes that copy's directory, by the path listed for it
//!   ([`remove_copy`] refuses anything that is not a copy directly under the
//!   recovery directory, and another editor's live autosave), and the bar
//!   lists the rest.
//! * **Later** puts the bar away for this run.
//!
//! O, Ctrl+Delete and L answer Open copy, Delete and Later for the newest
//! copy from the keyboard ([`crate::keys::recovery`]), while nothing is typed
//! into and the unsaved bar is not asking.
//!
//! **A recovered copy goes once its scene is saved elsewhere** (decided
//! 2026-10-03). The first save-as that lands for a document opened from a
//! copy removes that copy, by the path the document kept
//! ([`Document::take_recovered`]) and through [`remove_copy`] as Delete does:
//! the work it held now lives in the scene's own directory, and offering it
//! again would only invite a stale restore. A save-as that fails leaves it,
//! and a copy that would not go is logged and offered at the next start.
//!
//! # Pruning
//!
//! Before anything is listed, [`prune_copies`] removes each copy older than
//! [`MAX_AGE`](crate::document::MAX_AGE) and each past the newest
//! [`KEEP_NEWEST`](crate::document::KEEP_NEWEST), by its own path, and the log
//! names each one. A copy named on the command line is open in this run and
//! is never pruned. Nothing is pruned later in a run, so a copy opened from
//! the bar is never pruned in the run that opened it.
//!
//! # Autosave (decided 2026-10-03)
//!
//! While the document is dirty, every [`AUTOSAVE_KEY`] seconds of the
//! editor's clock — [`AUTOSAVE_SECONDS`] with nothing set — the authored scene
//! (never the played state: [`Document::write_recovery`] writes the authored
//! files) is written into the recovery directory as a copy like any other, so
//! a crash or a killed process loses at most that long. The interval restarts
//! whenever the document is clean, so the first autosave comes a whole
//! interval after the first unsaved edit; a scene unchanged since the last
//! autosave is not written again.
//!
//! **Each document session has one slot.** A new autosave is written into a
//! new copy first and the session's previous one removed after, so an
//! interrupted write never leaves the session with nothing; no other copy is
//! touched. The slot is removed once the document is clean — a save, or an
//! undo back to the saved state — and when the session ends: a discard, a new
//! scene, an open or the window closing. A run ending on its frame budget or
//! its limit leaves a dirty session's slot, which the next start offers. A
//! recovery copy written as the window is taken away supersedes the slot,
//! which is removed.
//!
//! **A live session's slot is marked in use** ([`mark_in_use`]), so another
//! editor sharing the directory neither offers, deletes nor prunes it; the
//! mark goes with the slot, and a crashed session's slot, whose mark the
//! operating system released, is an ordinary copy — `copies`' module docs in
//! `crate::document` say how. A slot that would not be marked is logged and
//! kept: it is still this session's autosave, only unguarded.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use crcbl::shell::Shell;
use crcbl::store::settings::SettingsStack;

use super::Editor;
use super::unsaved::Guarded;
use crate::document::{
    Document, EditError, InUse, PlayState, RecoveryCopy, list_copies, mark_in_use, prune_copies,
    remove_copy,
};
use crate::panel::{RecoveryAnswer, Tone};

/// How many copies the recovery bar lists, newest first: enough to reach
/// past a copy of the wrong scene, few enough to keep the viewport tall.
pub const OFFERED: usize = 3;

/// The settings key holding the autosave interval, in seconds — beside the
/// panel layout and the snap steps in the editor's `settings.toml`
/// ([`crate::layout::APP_NAME`]).
pub const AUTOSAVE_KEY: &str = "editor.autosave.interval";

/// The autosave interval with nothing set, in seconds: a minute is a
/// minute's work at most lost, and writing a scene that often costs nothing.
pub const AUTOSAVE_SECONDS: f64 = 60.0;

/// What the status line says once the offer is put away.
const LATER: &str = "Recovery copies kept: they are offered again at the next start";

/// The autosave's state between frames — see the module docs.
#[derive(Debug)]
pub(super) struct Autosave {
    /// How long the document may stay dirty between autosaves.
    pub(super) interval: Duration,
    /// When, on the editor's clock, the next autosave is due.
    due: Duration,
    /// This session's copy, once one is written.
    pub(super) slot: Option<PathBuf>,
    /// The mark that keeps other editors off [`slot`](Self::slot), dropped
    /// before the slot is removed.
    marked: Option<InUse>,
    /// The files that copy holds, so a scene unchanged since is not written
    /// again.
    written: Option<BTreeMap<String, String>>,
}

impl Autosave {
    /// An autosave every `interval`, the first due one interval from the
    /// clock's start.
    pub(super) const fn new(interval: Duration) -> Self {
        Self {
            interval,
            due: interval,
            slot: None,
            marked: None,
            written: None,
        }
    }

    /// The interval `stack` sets, or [`AUTOSAVE_SECONDS`] where it sets none.
    ///
    /// A value that is not a positive, finite number of seconds is logged and
    /// passed over, as the snap steps are: a settings file is a thing people
    /// edit, and an interval of zero would write every frame.
    pub(super) fn load(stack: &SettingsStack) -> Self {
        let seconds = if stack.contains(AUTOSAVE_KEY) {
            match stack.get::<f64>(AUTOSAVE_KEY) {
                Some(seconds) if seconds.is_finite() && seconds > 0.0 => seconds,
                _ => {
                    crcbl::log::warn!(
                        "editor: {AUTOSAVE_KEY} is not a positive number of seconds; \
                         autosaving every {AUTOSAVE_SECONDS} seconds"
                    );
                    AUTOSAVE_SECONDS
                }
            }
        } else {
            AUTOSAVE_SECONDS
        };
        Self::new(Duration::from_secs_f64(seconds))
    }
}

/// Now, in milliseconds since the Unix epoch: what a copy's name is stamped
/// with and its age measured against.
///
/// A clock set before 1970 reads 0: a copy's name is a label, and
/// [`Document::write_recovery`] never reuses one that is taken.
pub(super) fn now_millis() -> u128 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_or(0, |since| since.as_millis())
}

/// How long ago `stamp` was at `now`, both in milliseconds since the Unix
/// epoch, as the recovery bar says it — in whole minutes, hours or days,
/// rounded down, so no calendar is needed.
fn ago(stamp: u128, now: u128) -> String {
    const MINUTE_MS: u128 = 60 * 1000;
    const HOUR_MS: u128 = 60 * MINUTE_MS;
    const DAY_MS: u128 = 24 * HOUR_MS;
    let age = now.saturating_sub(stamp);
    match age {
        _ if age < MINUTE_MS => "under a minute ago".to_owned(),
        _ if age < HOUR_MS => format!("{} min ago", age / MINUTE_MS),
        _ if age < DAY_MS => format!("{} h ago", age / HOUR_MS),
        _ if age < 2 * DAY_MS => "1 day ago".to_owned(),
        _ => format!("{} days ago", age / DAY_MS),
    }
}

impl<S: Shell + ?Sized> Editor<S> {
    /// Prunes the recovery directory and offers back what is left — see the
    /// module docs. `opened` is the scene named on the command line, which
    /// is never pruned.
    pub(super) fn offer_recovery(&mut self, opened: Option<&Path>) {
        let Some(base) = self.recovery.clone() else {
            return;
        };
        let now = now_millis();
        let keep: Vec<PathBuf> = opened.map(Path::to_path_buf).into_iter().collect();
        match prune_copies(&base, now, &keep) {
            Ok(pruned) => {
                for dir in &pruned.removed {
                    crcbl::log::info!("editor: removed the old recovery copy {}", dir.display());
                }
                for (dir, error) in &pruned.failed {
                    crcbl::log::warn!(
                        "editor: the old recovery copy {} was not removed: {error}",
                        dir.display()
                    );
                }
            }
            Err(error) => crcbl::log::warn!("editor: no recovery copy was pruned: {error}"),
        }
        match list_copies(&base) {
            Ok(mut copies) => {
                copies.truncate(OFFERED);
                self.offered = copies;
            }
            Err(error) => crcbl::log::warn!("editor: no recovery copy is offered: {error}"),
        }
        self.show_offer();
    }

    /// Puts the recovery bar up listing what is offered, or takes it down
    /// once nothing is.
    pub(super) fn show_offer(&mut self) {
        if self.offered.is_empty() {
            self.panels.end_recovery();
            return;
        }
        let now = now_millis();
        let heading = match self.offered.len() {
            1 => "An earlier run left a recovery copy of unsaved changes:".to_owned(),
            count => format!("An earlier run left {count} recovery copies of unsaved changes:"),
        };
        let rows = self
            .offered
            .iter()
            .map(|copy| format!("`{}`, {}", copy.name, ago(copy.stamp, now)))
            .collect();
        self.panels.begin_recovery(heading, rows);
    }

    /// Carries out a click on the recovery bar — see the module docs.
    ///
    /// # Errors
    ///
    /// Why the copy would not open or go, which the caller puts on the status
    /// line; the bar stays up.
    pub(super) fn answer_recovery(&mut self, answer: RecoveryAnswer) -> Result<(), EditError> {
        match answer {
            RecoveryAnswer::Open(index) => {
                let Some(copy) = self.offered.get(index).cloned() else {
                    return Ok(());
                };
                if self.document.play_state() != PlayState::Editing {
                    return Err(EditError::Playing);
                }
                let document = Document::open_recovery(&copy.dir, crate::scene::vocabulary())?;
                let offered = std::mem::take(&mut self.offered);
                self.panels.end_recovery();
                self.guard(Guarded::Open(Box::new(document)))?;
                // Still asking, so the offer is held until the answer.
                if self.unsaved.is_some() {
                    self.held_offer = offered;
                }
                Ok(())
            }
            RecoveryAnswer::Delete(index) => {
                let (Some(copy), Some(base)) = (self.offered.get(index), &self.recovery) else {
                    return Ok(());
                };
                remove_copy(base, &copy.dir)?;
                let RecoveryCopy { dir, .. } = self.offered.remove(index);
                crcbl::log::info!("editor: deleted the recovery copy {}", dir.display());
                self.panels.set_status(
                    format!("Deleted the recovery copy {}", dir.display()),
                    Tone::Info,
                );
                self.show_offer();
                Ok(())
            }
            RecoveryAnswer::Later => {
                self.offered.clear();
                self.panels.end_recovery();
                self.panels.set_status(LATER, Tone::Info);
                Ok(())
            }
        }
    }

    /// Puts back the offer Open copy took down, once the unsaved bar it
    /// asked ended with nothing opened — see the module docs.
    pub(super) fn restore_offer(&mut self) {
        if self.held_offer.is_empty() {
            return;
        }
        self.offered = std::mem::take(&mut self.held_offer);
        self.show_offer();
    }

    /// Removes the recovery copy the document was read back from, after a
    /// save-as landed — see the module docs.
    pub(super) fn remove_recovered(&mut self) {
        let (Some(copy), Some(base)) = (self.document.take_recovered(), &self.recovery) else {
            return;
        };
        match remove_copy(base, &copy) {
            Ok(()) => crcbl::log::info!(
                "editor: removed the recovery copy {}, saved elsewhere now",
                copy.display()
            ),
            Err(error) => crcbl::log::warn!(
                "editor: the recovery copy {} was not removed: {error}",
                copy.display()
            ),
        }
    }

    /// Writes the session's autosave once it is due and the document dirty,
    /// and removes the slot once the document is clean — see the module docs.
    pub(super) fn tick_autosave(&mut self) {
        let Some(base) = self.recovery.clone() else {
            return;
        };
        if !self.has_unsaved_edits() {
            self.autosave.due = self.elapsed + self.autosave.interval;
            self.end_autosave();
            return;
        }
        if self.elapsed < self.autosave.due {
            return;
        }
        self.autosave.due = self.elapsed + self.autosave.interval;
        let files = match self.document.authored_files() {
            Ok(files) => files,
            Err(error) => {
                crcbl::log::warn!("editor: no autosave was written: {error}");
                return;
            }
        };
        if self.autosave.written.as_ref() == Some(&files) {
            return;
        }
        let dir = match self.document.write_recovery(&base, now_millis()) {
            Ok(dir) => dir,
            Err(error) => {
                crcbl::log::warn!("editor: no autosave was written: {error}");
                return;
            }
        };
        crcbl::log::info!(
            "editor: the unsaved changes to `{}` were autosaved to {}",
            self.document.name(),
            dir.display()
        );
        let marked = match mark_in_use(&base, &dir) {
            Ok(marked) => Some(marked),
            Err(error) => {
                crcbl::log::warn!(
                    "editor: the autosave {} is not marked in use: {error}",
                    dir.display()
                );
                None
            }
        };
        self.autosave.written = Some(files);
        let previous = self.autosave.slot.replace(dir);
        // The old mark goes first: `remove_copy` refuses a slot in use, this
        // session's own included.
        self.autosave.marked = marked;
        if let Some(previous) = previous {
            remove_slot(&base, &previous);
        }
    }

    /// Removes the session's autosave, if one was written — see the module
    /// docs.
    pub(super) fn end_autosave(&mut self) {
        self.autosave.written = None;
        self.autosave.marked = None;
        if let (Some(slot), Some(base)) = (self.autosave.slot.take(), &self.recovery) {
            remove_slot(base, &slot);
        }
    }
}

/// Removes the autosave copy at `slot` under `base`, logging a refusal: the
/// copy left behind is pruned at a later start, so the run goes on.
fn remove_slot(base: &Path, slot: &Path) {
    if let Err(error) = remove_copy(base, slot) {
        crcbl::log::warn!(
            "editor: the autosave {} was not removed: {error}",
            slot.display()
        );
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// **An age is said in whole units, rounded down**, and a stamp after
    /// now is under a minute old.
    #[test]
    fn an_age_is_said_in_whole_units() {
        let minute = 60 * 1000;
        let now = 1_000 * 24 * 60 * minute;
        assert_eq!(ago(now - 59 * 1000, now), "under a minute ago");
        assert_eq!(ago(now + minute, now), "under a minute ago");
        assert_eq!(ago(now - 59 * minute, now), "59 min ago");
        assert_eq!(ago(now - 60 * minute, now), "1 h ago");
        assert_eq!(ago(now - 47 * 60 * minute, now), "1 day ago");
        assert_eq!(ago(now - 48 * 60 * minute, now), "2 days ago");
        assert_eq!(ago(now - 14 * 24 * 60 * minute, now), "14 days ago");
    }
}
