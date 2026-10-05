//! The one save path: what a save is asked for, what it answers, and the desk
//! every trigger goes through.
//!
//! The persistence rules in `docs/notes/simulation.md` make a save the
//! server's, triggered by a command, so that the console, a game's key or
//! button, the autosave timer and a dedicated server's console take **one
//! path**. This module is that path's engine half:
//!
//! ```text
//!  console `save [SLOT]` ─▶ EngineLink ─▶ Loop::drain_saves ─┐
//!  a game's key, button ──────────────────────────────────────┤
//!  the autosave's cadence ────────────────────────────────────┼─▶ HostedGame::save
//!  the window closing ────────────────────────────────────────┘        │
//!  a dedicated server's `save [SLOT]` ─▶ the server's own save ────────┤
//!                                                                      ▼
//!                          SaveDesk::take ─▶ the game's one writer ─▶ SaveWriter
//!                                │
//!                                └─▶ the last save, each file's size: F3's "storage"
//! ```
//!
//! **A game writes its save in exactly one place.** [`SaveDesk::take`] runs
//! the game's writer — the one function that builds the game's payload and
//! hands it to `crcbl_store`'s [`SaveWriter`](crcbl_store::save::SaveWriter) —
//! and records what came of it, so every trigger is the same write and the
//! same record. A windowed game routes through
//! [`HostedGame::save`](crate::engine::HostedGame::save): the loop calls it for
//! the console, and the game calls it for its own key, its autosave and its
//! close. A dedicated server has no loop, so its own save method calls the
//! same desk and the same writer.
//!
//! **A game's own triggers call it where they arise**, not through a queue the
//! loop drains: an autosave belongs to the tick that came due — `apps/shard`
//! writes the state its `[HUD]` heartbeat reports on the same tick, and a
//! page gate compares the two — so a request held until the next frame would
//! write a later state than the one it was asked for.
//!
//! **The words are each trigger's own.** [`Saved`] and [`SaveFailure`] carry
//! what happened; the console prints [`console_line`], and a game's notice, a
//! log line or a server's reply phrases the same result its own way.

use std::fmt;

use crcbl_core::TickId;
use crcbl_store::save::SaveBacking;

/// The longest slot name, in bytes: room for a name a person types, and a
/// bound on the file name it becomes.
pub const SLOT_NAME_MAX: usize = 64;

/// What asked for a save.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SaveTrigger {
    /// The debug console's `save`.
    Console,
    /// A key or button the game binds to saving.
    Input,
    /// The game's own autosave cadence.
    Autosave,
    /// A dedicated server's console.
    ServerConsole,
    /// The window closing, at the player's ask.
    Close,
}

impl SaveTrigger {
    /// The trigger, in the words the storage section shows.
    #[must_use]
    pub const fn label(self) -> &'static str {
        match self {
            Self::Console => "console",
            Self::Input => "key",
            Self::Autosave => "autosave",
            Self::ServerConsole => "server console",
            Self::Close => "close",
        }
    }
}

/// A save slot a person named: `save slot2`.
///
/// **A bare name, never a path** — ASCII letters, digits, `-` and `_`, at most
/// [`SLOT_NAME_MAX`] bytes — because it was typed, and it becomes part of a
/// file name in the game's save directory. The rule is the console's `config`
/// rule for the same reason.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Slot(String);

impl Slot {
    /// The slot `name` names.
    ///
    /// # Errors
    ///
    /// The line to print, for a name that is empty, too long or not bare.
    pub fn new(name: &str) -> Result<Self, String> {
        if name.len() > SLOT_NAME_MAX || !crate::console_config::is_bare_name(name) {
            return Err(format!(
                "`{name}` is not a slot name — a slot is ASCII letters, digits, `-` and `_`, \
                 at most {SLOT_NAME_MAX} bytes, and names one file in this game's save \
                 directory; it is not a path"
            ));
        }
        Ok(Self(name.to_owned()))
    }

    /// The name as typed.
    #[must_use]
    pub fn as_str(&self) -> &str {
        &self.0
    }

    /// The file this slot is kept in, beside a game's own save `base`: the
    /// slot's name before the extension, so `towers-run.crb` in slot `slot2`
    /// is `towers-run-slot2.crb`, and a base with no extension takes the name
    /// at its end.
    #[must_use]
    pub fn file_name(&self, base: &str) -> String {
        match base.rsplit_once('.') {
            Some((stem, extension)) if !stem.is_empty() => {
                format!("{stem}-{}.{extension}", self.0)
            }
            _ => format!("{base}-{}", self.0),
        }
    }
}

/// One ask for a save: what asked, and the slot it named, if any.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SaveRequest {
    trigger: SaveTrigger,
    slot: Option<Slot>,
}

impl SaveRequest {
    /// A save into the game's own slot, asked for by `trigger`.
    #[must_use]
    pub const fn new(trigger: SaveTrigger) -> Self {
        Self {
            trigger,
            slot: None,
        }
    }

    /// This request, into `slot` instead.
    #[must_use]
    pub fn in_slot(self, slot: Slot) -> Self {
        Self {
            slot: Some(slot),
            ..self
        }
    }

    /// The request a typed `save` line makes — `args` are the words after
    /// `save`: none for the game's own slot, or one slot name. The debug
    /// console and a dedicated server's console read the line through this,
    /// so the two agree on what `save slot2` means.
    ///
    /// # Errors
    ///
    /// The line to print, for more than one word or a name [`Slot::new`]
    /// refuses.
    pub fn from_args(trigger: SaveTrigger, args: &[&str]) -> Result<Self, String> {
        match args {
            [] => Ok(Self::new(trigger)),
            [name] => Ok(Self::new(trigger).in_slot(Slot::new(name)?)),
            _ => Err("save takes one slot name, or nothing for the game's own slot".to_owned()),
        }
    }

    /// What asked.
    #[must_use]
    pub const fn trigger(&self) -> SaveTrigger {
        self.trigger
    }

    /// The slot named, or `None` for the game's own.
    #[must_use]
    pub const fn slot(&self) -> Option<&Slot> {
        self.slot.as_ref()
    }

    /// The file this request writes, for a game whose own save is `base`:
    /// `base` itself, or the named slot's file beside it.
    #[must_use]
    pub fn file_name(&self, base: &str) -> String {
        match &self.slot {
            None => base.to_owned(),
            Some(slot) => slot.file_name(base),
        }
    }
}

/// What a save that landed wrote.
#[derive(Clone, Debug, PartialEq)]
pub struct Saved {
    /// The file it went to, in the game's save directory.
    pub file: String,
    /// The tick the save was taken at — its header's.
    pub tick: TickId,
    /// The playtime its header carries, in seconds.
    pub playtime_secs: f64,
    /// How long the file is.
    pub bytes: usize,
    /// What the save kept, in the game's own words — `wave 3/10` — for a
    /// trigger's line to name.
    pub summary: String,
}

/// Why a save did not land.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum SaveFailure {
    /// This game takes no saves: its
    /// [`HostedGame::save`](crate::engine::HostedGame::save) is the default.
    Unsupported,
    /// This run keeps its saves nowhere — a headless run, or a platform that
    /// gave no place. A game's autosave says nothing about it, as a save that
    /// was never going anywhere.
    Nowhere,
    /// The game would not save now, and says why in its own words: a wave
    /// coming in, a joiner, nothing played yet.
    Refused(String),
    /// The write was attempted and failed.
    Failed(String),
}

impl fmt::Display for SaveFailure {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Unsupported => f.write_str(
                "this game takes no saves — `HostedGame::save` is the seam, and this game \
                 leaves it at the default",
            ),
            Self::Nowhere => f.write_str("this run keeps its saves nowhere"),
            Self::Refused(why) | Self::Failed(why) => f.write_str(why),
        }
    }
}

impl std::error::Error for SaveFailure {}

/// The line the debug console prints for a save's outcome.
#[must_use]
pub fn console_line(outcome: &Result<Saved, SaveFailure>) -> String {
    match outcome {
        Ok(saved) => format!(
            "saved {} at tick {}: {}",
            saved.file,
            saved.tick.get(),
            saved.summary
        ),
        Err(failure) => format!("not saved: {failure}"),
    }
}

/// Where a game's saves go through: every trigger's request reaches the game's
/// one writer here, and what came of each is recorded for the debug panel's
/// "storage" section.
#[derive(Debug)]
pub struct SaveDesk {
    /// Where the saves go, as the section names it.
    location: String,
    /// The last save that landed, and what asked for it.
    last: Option<(SaveTrigger, Saved)>,
    /// Every file this run wrote, with its length at its latest write, in the
    /// order each was first written.
    files: Vec<(String, usize)>,
    /// The file the last autosave went to.
    autosave_file: Option<String>,
    /// How many saves did not land, and why the latest did not.
    failures: u64,
    last_failure: Option<String>,
}

impl SaveDesk {
    /// A desk for saves that go to `location`, as the section names it.
    #[must_use]
    pub fn new(location: impl Into<String>) -> Self {
        Self {
            location: location.into(),
            last: None,
            files: Vec::new(),
            autosave_file: None,
            failures: 0,
            last_failure: None,
        }
    }

    /// A desk for saves kept in `backing`: its label, and its directory where
    /// it has one.
    #[must_use]
    pub fn over(backing: &SaveBacking) -> Self {
        match backing.root() {
            Some(root) => Self::new(format!("{} {}", backing.label(), root.display())),
            None => Self::new(backing.label()),
        }
    }

    /// Runs `write` — the game's one save writer — for `request`, records what
    /// came of it, and answers it.
    ///
    /// # Errors
    ///
    /// Whatever `write` answered.
    pub fn take(
        &mut self,
        request: SaveRequest,
        write: impl FnOnce(&SaveRequest) -> Result<Saved, SaveFailure>,
    ) -> Result<Saved, SaveFailure> {
        let outcome = write(&request);
        match &outcome {
            Ok(saved) => {
                match self.files.iter_mut().find(|(file, _)| *file == saved.file) {
                    Some((_, bytes)) => *bytes = saved.bytes,
                    None => self.files.push((saved.file.clone(), saved.bytes)),
                }
                if request.trigger == SaveTrigger::Autosave {
                    self.autosave_file = Some(saved.file.clone());
                }
                self.last = Some((request.trigger, saved.clone()));
            }
            Err(failure) => {
                self.failures += 1;
                self.last_failure = Some(failure.to_string());
            }
        }
        outcome
    }

    /// The last save that landed.
    #[must_use]
    pub fn last(&self) -> Option<&Saved> {
        self.last.as_ref().map(|(_, saved)| saved)
    }

    /// What asked for the last save that landed.
    #[must_use]
    pub fn last_trigger(&self) -> Option<SaveTrigger> {
        self.last.as_ref().map(|(trigger, _)| *trigger)
    }
}

/// The debug panel's "storage" section: where saves go, the last one's tick
/// and playtime and what asked for it, each file this run wrote and its
/// size, where the autosave goes, and the saves that did not land.
///
/// **Sizes are the writes' own**, recorded as each landed, so the section
/// reads no file a frame and shows only what this run wrote.
impl crcbl_ui::DebugModule for SaveDesk {
    fn debug_section(&self, section: &mut crcbl_ui::DebugSection) {
        section.set_title("storage");
        section.row_str("where", &self.location);
        match &self.last {
            Some((trigger, saved)) => {
                section.row(
                    "last save",
                    format_args!(
                        "tick {}, {:.1} s played",
                        saved.tick.get(),
                        saved.playtime_secs
                    ),
                );
                section.row_str("by", trigger.label());
            }
            None => section.row_str("last save", "none this run"),
        }
        for (file, bytes) in &self.files {
            section.row(file, format_args!("{bytes} B"));
        }
        section.row_str(
            "autosave",
            self.autosave_file.as_deref().unwrap_or("none this run"),
        );
        if let Some(why) = &self.last_failure {
            section.row("not saved", format_args!("{}, last: {why}", self.failures));
        }
    }
}

#[cfg(test)]
mod tests;
