//! Apply-on-confirm with a timed revert, for the keys that can leave a player
//! unable to see the screen.
//!
//! ```text
//! change(key, value) ── a key the catalogue does not mark `confirm` ──▶ apply: written, at once
//!        │
//!        └─ a confirm key ─▶ applied live, previous held ─▶ PendingChange
//!                                                             │
//!            keep ◀── the player keeps it ───────────────────┤
//!   written to the stack and the file                         │
//!                                                             ├── revert ◀── the player reverts, or
//!                                      the previous value re-applied, nothing written   REVERT_AFTER of frame time passes
//! ```
//!
//! # Why the stack is not written until the player keeps it
//!
//! A pending value is a trial, not a setting. Written straight away, a `save`
//! from the console or a settings screen in the seconds before the countdown
//! ran out would put a mode that blanks the screen into the file — and the
//! revert would have to undo a write it does not own. Held here instead, the
//! revert only re-applies what was live, and "writes nothing" is literally what
//! happens.
//!
//! # The clock is the frame's
//!
//! [`PendingChange::tick`] is handed the frame time the loop already measures,
//! never the wall clock: a test steps the countdown by stepping frames, and a
//! frame clock the loop holds back — a paused browser tab, a debugger — holds
//! the countdown with it, rather than reverting a change the player never got
//! the chance to look at.
//!
//! # What is kept is what landed
//!
//! A window system can refuse a borderless window and a swapchain that cannot
//! be rebuilt keeps its mode, so the value a player is asked to keep is the one
//! the live seam reports — [`PendingChange::landed`] — and that is the value
//! [`PendingChange::keep`] writes. A request the window system refused is kept
//! as the mode the window is really in, not as the one that was asked for.

use std::time::Duration;

use crcbl_console::{Fault, Value};
use crcbl_store::settings::SettingsStack;

use super::engine_display::{
    DISPLAY_MODE_KEY, PRESENT_MODE_KEY, display_mode_from_name, display_mode_name,
};
use super::key_catalogue::catalogued;
use super::stage::{Applied, Stage, Unsupported, apply};
use crate::engine::{Pacing, SettingsSource};

/// How long a confirm key waits for the player to keep it before it reverts.
///
/// Fifteen seconds, the common figure in shipped games' display menus: long
/// enough to read the prompt and find the button on a screen that came back,
/// short enough that a player looking at a blank one is not left there.
pub const REVERT_AFTER: Duration = Duration::from_secs(15);

/// What [`change`] did with a write.
#[derive(Clone, Debug, PartialEq)]
pub enum Change {
    /// Written, as [`apply`] writes every key: the key was not a confirm key,
    /// or the host has no live seam to hold one on — so nothing on screen
    /// moved that could need undoing.
    Applied(Applied),
    /// Applied live and held: the caller keeps it until the player keeps it,
    /// reverts it, or [`REVERT_AFTER`] runs out.
    Pending(PendingChange),
}

/// A confirm key applied live, waiting on the player.
///
/// It holds the dotted key, the value that was live before, the value the seam
/// reports now and the time left. **It never touches the settings stack until
/// [`keep`](Self::keep)** — see the [module docs](self).
#[derive(Clone, Debug, PartialEq)]
pub struct PendingChange {
    key: String,
    asked: Value,
    previous: Value,
    landed: Value,
    remaining: Duration,
}

impl PendingChange {
    /// The dotted key this change is to.
    #[must_use]
    pub fn key(&self) -> &str {
        &self.key
    }

    /// The value the player asked for.
    #[must_use]
    pub const fn asked(&self) -> &Value {
        &self.asked
    }

    /// The value that was live before the change, and that a revert puts back.
    #[must_use]
    pub const fn previous(&self) -> &Value {
        &self.previous
    }

    /// The value the live seam reports now — what the player is looking at,
    /// and what [`keep`](Self::keep) writes.
    #[must_use]
    pub const fn landed(&self) -> &Value {
        &self.landed
    }

    /// How much frame time is left before the change reverts.
    #[must_use]
    pub const fn remaining(&self) -> Duration {
        self.remaining
    }

    /// The whole seconds a countdown shows: rounded up, so the prompt reads
    /// one until the moment it reverts rather than zero for the last second.
    #[must_use]
    pub fn seconds_left(&self) -> u64 {
        let whole = self.remaining.as_secs();
        if self.remaining.subsec_nanos() > 0 {
            whole + 1
        } else {
            whole
        }
    }

    /// Spends `frame_time` of the countdown, answering whether it has run out.
    ///
    /// The caller reverts on `true`: this only counts, so a caller that drops
    /// the answer has a change that never reverts, which `#[must_use]` refuses.
    #[must_use]
    pub fn tick(&mut self, frame_time: Duration) -> bool {
        self.remaining = self.remaining.saturating_sub(frame_time);
        self.remaining.is_zero()
    }

    /// Reads back what the live seam is showing now.
    ///
    /// **Only the display mode moves here.** A window system answers a mode
    /// request on a later frame than the one that made it, so what landed on
    /// the frame of the request is the old mode and the answer arrives after.
    /// A swapchain rebuild lands — or rolls back — inside the call that asked
    /// for it, so a present mode's landed value is final from the start.
    pub fn observe(&mut self, stage: &dyn Stage) {
        if self.name() == DISPLAY_MODE_KEY
            && let Ok(mode) = stage.display_mode()
        {
            self.landed = Value::Enum(display_mode_name(mode));
        }
    }

    /// The player kept it: write what landed into `stack`, and into the file
    /// `source` names for `app_name` — answering whether there was a file to
    /// write, on [`SettingsSource::save`]'s terms.
    ///
    /// **The file is written with this key and no other.** It is read fresh,
    /// the key set, and saved, so a settings screen's edits the player has not
    /// saved stay unsaved — a confirm is the player's word about this key, not
    /// about the rest of the screen. `stack` is the run's own copy, which gets
    /// the key so every reader of the run agrees with the file.
    ///
    /// # Errors
    ///
    /// A [`Fault`] naming the key, where the write or the save was refused.
    pub fn keep(
        self,
        stack: &mut SettingsStack,
        source: SettingsSource<'_>,
        app_name: &str,
    ) -> Result<bool, Fault> {
        let mut file = source.open_editable(app_name);
        apply(&mut file, &self.key, &self.landed, &mut Nowhere)?;
        let saved = source
            .save(app_name, &file)
            .map_err(|error| Fault::new(format!("`{}`: {error}", self.key)))?;
        apply(stack, &self.key, &self.landed, &mut Nowhere)?;
        Ok(saved)
    }

    /// The player reverted it, or the countdown ran out: put the previous
    /// value back on `stage`, and write nothing anywhere.
    ///
    /// # Errors
    ///
    /// [`Unsupported`] where `stage` has no seam for the key — which is not a
    /// stage this change was applied through, since [`change`] only holds a
    /// change a seam took.
    pub fn revert(self, stage: &mut dyn Stage) -> Result<(), Unsupported> {
        match (self.name(), &self.previous) {
            (DISPLAY_MODE_KEY, Value::Enum(name)) => stage
                .set_display_mode(
                    display_mode_from_name(name).expect("a held display mode is one of its words"),
                )
                .map(drop),
            (PRESENT_MODE_KEY, Value::Enum(name)) => stage
                .set_present_mode(
                    Pacing::from_name(name).expect("a held pacing is one of its words"),
                )
                .map(drop),
            (name, previous) => {
                unreachable!("`{name}` held {previous}, which no confirm key's seam answers")
            }
        }
    }

    /// This change, asked again for the same key while it was waiting.
    ///
    /// The next change keeps **this** one's previous value, so a revert goes
    /// back to where the player started rather than to the trial they moved
    /// off; and a next change that asks for that starting value is no change
    /// at all — the live seam is already back there — so it answers `None`,
    /// and nothing is left waiting. Pressing the fullscreen key a second time
    /// is that case.
    ///
    /// # Panics
    ///
    /// If `next` is to another key: a waiting change for one key is reverted
    /// before a change to another starts, and that is the caller's to do.
    #[must_use]
    pub fn superseded_by(self, mut next: Self) -> Option<Self> {
        assert_eq!(
            self.key, next.key,
            "a change to another key supersedes nothing"
        );
        if next.asked == self.previous {
            return None;
        }
        next.previous = self.previous;
        Some(next)
    }

    /// The key without its namespace — what the catalogue's arms match on.
    fn name(&self) -> &str {
        self.key.rsplit('.').next().unwrap_or(&self.key)
    }
}

/// A [`Stage`] with no seams, for a write that must reach the stack and
/// nothing on screen: the value [`PendingChange::keep`] writes is already
/// live.
struct Nowhere;

impl Stage for Nowhere {}

/// Write one catalogue key, holding it for the player to keep where the
/// catalogue says it can blank the screen.
///
/// [`apply`] for every key the catalogue does not mark
/// [`confirm`](super::CatalogueKey::confirm). A confirm key is applied live
/// through `stage` and **not written**; the answer is the
/// [`PendingChange`] the caller holds, ticks and resolves. A confirm key on a
/// host with no seam for it is [`apply`]'s too — written, and
/// [`Applied::NextStart`] — because nothing on screen moved that could need
/// undoing.
///
/// A confirm key asked for the value already live changes nothing on screen,
/// so it is written and answers [`Applied::Live`] rather than asking the
/// player to keep what they already had.
///
/// # Errors
///
/// [`apply`]'s refusals — a key the engine does not define, one nothing reads,
/// a value outside the key's kind — for a confirm key as for any other.
pub fn change(
    stack: &mut SettingsStack,
    key: &str,
    value: &Value,
    stage: &mut dyn Stage,
) -> Result<Change, Fault> {
    let entry = catalogued(key)
        .ok_or_else(|| Fault::new(format!("`{key}` is not a key the engine defines")))?;
    if !entry.confirm {
        return apply(stack, key, value, stage).map(Change::Applied);
    }
    entry.kind.check(key, value)?;
    let Value::Enum(word) = *value else {
        unreachable!("every confirm key is an enum kind, which `check` has held it to")
    };
    let landed = match entry.name {
        DISPLAY_MODE_KEY => stage
            .set_display_mode(
                display_mode_from_name(word)
                    .expect("`check` has already held the value to `DISPLAY_MODE_NAMES`"),
            )
            .map(|landed| landed.map(display_mode_name)),
        PRESENT_MODE_KEY => stage
            .set_present_mode(
                Pacing::from_name(word)
                    .expect("`check` has already held the value to `PRESENT_MODE_NAMES`"),
            )
            .map(|landed| landed.map(Pacing::name)),
        name => unreachable!("`{name}` is marked confirm and has no live seam here"),
    };
    let Ok(landed) = landed else {
        return apply(stack, key, value, stage).map(Change::Applied);
    };
    if landed.previous == word {
        return apply(stack, key, value, &mut Nowhere).map(|_| Change::Applied(Applied::Live));
    }
    Ok(Change::Pending(PendingChange {
        key: entry.key,
        asked: value.clone(),
        previous: Value::Enum(landed.previous),
        landed: Value::Enum(landed.landed),
        remaining: REVERT_AFTER,
    }))
}

#[cfg(test)]
mod tests {
    use super::*;

    use crcbl_audio::mixer::Bus;
    use crcbl_shell::DisplayMode;
    use crcbl_store::MemoryStorage;
    use crcbl_store::StorageSource;
    use crcbl_store::settings::SETTINGS_FILE;

    use super::super::engine_display::{display_mode, present_mode};
    use super::super::engine_video::VIDEO_NAMESPACE;
    use super::super::stage::Landed;
    use crate::settings::AUDIO_NAMESPACE;
    use crate::settings::tests::{Recorder, stack_from};

    /// A window and a swapchain that do what they are told — or, where a test
    /// says so, land somewhere else: `refuse_borderless` is a tiling window
    /// manager, `refuse_rebuild` a swapchain that could not be rebuilt.
    #[derive(Debug, Default)]
    struct Screen {
        mode: DisplayMode,
        pacing: Pacing,
        refuse_borderless: bool,
        refuse_rebuild: bool,
        /// Every mode asked for, in order, so a revert can be seen to ask.
        modes_asked: Vec<DisplayMode>,
        pacings_asked: Vec<Pacing>,
    }

    impl Stage for Screen {
        fn display_mode(&self) -> Result<DisplayMode, Unsupported> {
            Ok(self.mode)
        }

        fn set_display_mode(
            &mut self,
            mode: DisplayMode,
        ) -> Result<Landed<DisplayMode>, Unsupported> {
            self.modes_asked.push(mode);
            let previous = self.mode;
            if !(self.refuse_borderless && mode.is_borderless()) {
                self.mode = mode;
            }
            Ok(Landed {
                previous,
                landed: self.mode,
            })
        }

        fn set_present_mode(&mut self, pacing: Pacing) -> Result<Landed<Pacing>, Unsupported> {
            self.pacings_asked.push(pacing);
            let previous = self.pacing;
            if !self.refuse_rebuild {
                self.pacing = pacing;
            }
            Ok(Landed {
                previous,
                landed: self.pacing,
            })
        }
    }

    fn display_key() -> String {
        format!("{VIDEO_NAMESPACE}.{DISPLAY_MODE_KEY}")
    }

    fn present_key() -> String {
        format!("{VIDEO_NAMESPACE}.{PRESENT_MODE_KEY}")
    }

    /// The change `change` held, or a panic naming what it did instead.
    fn pending(change: Result<Change, Fault>) -> PendingChange {
        match change {
            Ok(Change::Pending(pending)) => pending,
            other => panic!("the change was not held for a confirm: {other:?}"),
        }
    }

    /// **A confirm key is applied live, held, and starts the countdown** —
    /// and the stack does not hold it yet.
    #[test]
    fn a_risky_change_applies_live_and_starts_the_countdown() {
        let mut stack = stack_from("");
        let mut screen = Screen::default();
        let held = pending(change(
            &mut stack,
            &display_key(),
            &Value::Enum("borderless"),
            &mut screen,
        ));
        assert!(screen.mode.is_borderless(), "the window was not told");
        assert_eq!(held.previous(), &Value::Enum("windowed"));
        assert_eq!(held.landed(), &Value::Enum("borderless"));
        assert_eq!(held.remaining(), REVERT_AFTER);
        assert_eq!(held.seconds_left(), REVERT_AFTER.as_secs());
        assert!(
            !stack.contains(&display_key()),
            "a trial reached the stack before the player kept it",
        );

        let held = pending(change(
            &mut stack,
            &present_key(),
            &Value::Enum("off"),
            &mut screen,
        ));
        assert_eq!(screen.pacing, Pacing::Off, "the swapchain was not told");
        assert_eq!(held.previous(), &Value::Enum("auto"));
        assert!(!stack.contains(&present_key()));
    }

    /// **Keep writes what landed to the stack and to the file, and only that
    /// key to the file.**
    #[test]
    fn keeping_a_change_writes_it_to_the_stack_and_the_file() {
        let storage = MemoryStorage::new();
        let mut stack = SettingsStack::from_storage(&storage);
        // An edit the player has not saved, which a confirm must not save for
        // them.
        let music = format!("{AUDIO_NAMESPACE}.{}", Bus::Music.settings_key());
        apply(
            &mut stack,
            &music,
            &Value::Float(0.25),
            &mut Recorder::default(),
        )
        .expect("a gain in range");

        let mut screen = Screen::default();
        let held = pending(change(
            &mut stack,
            &display_key(),
            &Value::Enum("borderless"),
            &mut screen,
        ));
        assert_eq!(
            held.keep(&mut stack, SettingsSource::Source(&storage), "test"),
            Ok(true),
        );
        assert_eq!(
            display_mode(&stack),
            Some(DisplayMode::Borderless { monitor: None }),
            "the run's own stack does not hold the kept mode",
        );
        let reread = SettingsStack::from_storage(&storage);
        assert_eq!(
            display_mode(&reread),
            Some(DisplayMode::Borderless { monitor: None }),
            "the file does not hold the kept mode",
        );
        let written = String::from_utf8(
            storage
                .read(std::path::Path::new(SETTINGS_FILE))
                .expect("keep wrote the file"),
        )
        .expect("the writer emits UTF-8");
        assert!(
            !written.contains("music_volume"),
            "keeping one key saved the screen's other edits:\n{written}",
        );
    }

    /// **A run with nowhere to save keeps the key and says there was nowhere
    /// to write it** — a headless run must not touch a home directory.
    #[test]
    fn keeping_on_a_run_with_no_file_writes_the_stack_and_no_file() {
        let mut stack = stack_from("");
        let held = pending(change(
            &mut stack,
            &present_key(),
            &Value::Enum("vsync"),
            &mut Screen::default(),
        ));
        assert_eq!(
            held.keep(&mut stack, SettingsSource::None, "test"),
            Ok(false)
        );
        assert_eq!(present_mode(&stack), Some(Pacing::Vsync));
    }

    /// **The countdown runs out on frame time, reverts, and writes nothing.**
    ///
    /// Stepped in frames: one frame short of [`REVERT_AFTER`] is still
    /// waiting, the next is out — so a countdown that read the wall clock, or
    /// ignored what it was handed, is red here.
    #[test]
    fn running_out_of_time_reverts_and_writes_nothing() {
        let storage = MemoryStorage::new();
        let mut stack = SettingsStack::from_storage(&storage);
        let mut screen = Screen::default();
        let mut held = pending(change(
            &mut stack,
            &display_key(),
            &Value::Enum("borderless"),
            &mut screen,
        ));

        let frame = Duration::from_millis(250);
        let frames = REVERT_AFTER.as_millis() / frame.as_millis();
        for step in 1..frames {
            assert!(!held.tick(frame), "out after {step} of {frames} frames");
        }
        assert_eq!(
            held.seconds_left(),
            1,
            "the last frame still shows a second"
        );
        assert!(held.tick(frame), "still waiting after the whole countdown");

        held.revert(&mut screen).expect("the window that took it");
        assert_eq!(
            screen.mode,
            DisplayMode::Windowed,
            "the old mode is not back"
        );
        assert!(!stack.contains(&display_key()), "a revert wrote the stack");
        assert!(
            storage.read(std::path::Path::new(SETTINGS_FILE)).is_err(),
            "a revert wrote a file",
        );
    }

    /// **Revert puts the previous pacing back**, through the seam that took
    /// the change.
    #[test]
    fn reverting_puts_the_previous_value_back() {
        let mut stack = stack_from("");
        let mut screen = Screen {
            pacing: Pacing::Vsync,
            ..Screen::default()
        };
        let held = pending(change(
            &mut stack,
            &present_key(),
            &Value::Enum("off"),
            &mut screen,
        ));
        held.revert(&mut screen)
            .expect("the swapchain that took it");
        assert_eq!(screen.pacing, Pacing::Vsync);
        assert_eq!(screen.pacings_asked, [Pacing::Off, Pacing::Vsync]);
        assert!(!stack.contains(&present_key()));
    }

    /// **A safe key is written at once and never held** — a volume is one.
    #[test]
    fn a_safe_key_applies_at_once_with_no_prompt() {
        let mut stack = stack_from("");
        let mut stage = Recorder::default();
        let music = format!("{AUDIO_NAMESPACE}.{}", Bus::Music.settings_key());
        assert_eq!(
            change(&mut stack, &music, &Value::Float(0.5), &mut stage),
            Ok(Change::Applied(Applied::Live)),
        );
        assert_eq!(stage.gains, [(Bus::Music, 0.5)]);
        assert!(stack.contains(&music), "a safe key was not written");
    }

    /// **What landed is what is held, shown and kept** — a refused borderless
    /// request is held as the windowed mode the window is really in, and the
    /// answer that arrives on a later frame replaces it.
    #[test]
    fn what_landed_is_what_is_held_and_kept() {
        let mut stack = stack_from("");
        let mut screen = Screen {
            refuse_borderless: true,
            ..Screen::default()
        };
        let mut held = pending(change(
            &mut stack,
            &display_key(),
            &Value::Enum("borderless"),
            &mut screen,
        ));
        assert_eq!(held.asked(), &Value::Enum("borderless"));
        assert_eq!(
            held.landed(),
            &Value::Enum("windowed"),
            "the request was held rather than what the window system did",
        );

        // The window system's answer, a frame later.
        screen.mode = DisplayMode::Borderless {
            monitor: Some(crcbl_shell::MonitorId(1)),
        };
        held.observe(&screen);
        assert_eq!(held.landed(), &Value::Enum("borderless"));

        // And it moving back again is what is kept.
        screen.mode = DisplayMode::Windowed;
        held.observe(&screen);
        held.keep(&mut stack, SettingsSource::None, "test")
            .expect("memory takes every write");
        assert_eq!(display_mode(&stack), Some(DisplayMode::Windowed));

        // A swapchain that could not be rebuilt lands on the old pacing.
        let mut screen = Screen {
            refuse_rebuild: true,
            ..Screen::default()
        };
        let held = pending(change(
            &mut stack,
            &present_key(),
            &Value::Enum("off"),
            &mut screen,
        ));
        assert_eq!(held.landed(), &Value::Enum("auto"));
    }

    /// **Asking again for the same key keeps the first previous value, and
    /// asking for that value is no change at all.**
    #[test]
    fn a_second_ask_for_the_same_key_keeps_where_the_player_started() {
        let mut stack = stack_from("");
        let mut screen = Screen::default();
        let first = pending(change(
            &mut stack,
            &present_key(),
            &Value::Enum("off"),
            &mut screen,
        ));
        let second = pending(change(
            &mut stack,
            &present_key(),
            &Value::Enum("adaptive"),
            &mut screen,
        ));
        assert_eq!(second.previous(), &Value::Enum("off"));
        let held = first
            .superseded_by(second)
            .expect("a third value is still a trial");
        assert_eq!(held.previous(), &Value::Enum("auto"), "the start was lost");

        let back = pending(change(
            &mut stack,
            &present_key(),
            &Value::Enum("vsync"),
            &mut screen,
        ));
        assert!(held.clone().superseded_by(back).is_some());
        let mut to_start = held.clone();
        to_start.asked = Value::Enum("auto");
        assert_eq!(
            held.superseded_by(to_start),
            None,
            "asking for where the player started left a change waiting",
        );
    }

    /// **A value already live is no change**: written, and nothing held.
    #[test]
    fn asking_for_what_is_already_live_holds_nothing() {
        let mut stack = stack_from("");
        assert_eq!(
            change(
                &mut stack,
                &display_key(),
                &Value::Enum("windowed"),
                &mut Screen::default(),
            ),
            Ok(Change::Applied(Applied::Live)),
        );
        assert_eq!(display_mode(&stack), Some(DisplayMode::Windowed));
    }

    /// **A host with no live seam writes the key for the next start** and
    /// holds nothing, since nothing on screen moved.
    #[test]
    fn a_host_with_no_live_seam_writes_the_key_for_the_next_start() {
        let mut stack = stack_from("");
        assert_eq!(
            change(
                &mut stack,
                &display_key(),
                &Value::Enum("borderless"),
                &mut Nowhere,
            ),
            Ok(Change::Applied(Applied::NextStart)),
        );
        assert!(stack.contains(&display_key()));
        let fault = change(
            &mut stack,
            &display_key(),
            &Value::Enum("exclusive"),
            &mut Nowhere,
        )
        .expect_err("a word outside the domain");
        assert!(
            fault.message().contains("display_mode"),
            "{}",
            fault.message()
        );
    }
}
