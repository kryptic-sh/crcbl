//! `ISteamTimeline`: marks on the recording Steam keeps of a game session.
//!
//! When the player has Steam's game recording on, the game can describe what
//! is happening: a tooltip for the current state, the timeline's colour by
//! [`GameMode`], events — instantaneous, or ranges that start now and end
//! later ([`TimelineRange`]) — and game phases, the sections a player browses
//! recordings by. Steam may offer an event with a [`ClipPriority`] as a clip.
//!
//! Every limit the header states is checked before the call: priorities up to
//! [`MAX_TIMELINE_PRIORITY`], finite offsets, durations from zero to
//! [`MAX_TIMELINE_EVENT_SECONDS`], and phase ids up to
//! [`MAX_PHASE_ID_LENGTH`] bytes.

use std::{marker::PhantomData, sync::Arc, time::Duration};

use crate::{
    Steam, SteamCall, SteamError,
    call::{CallRow, private::Answer},
    callbacks::{Base, fixed_string, read},
    client::Client,
    error::c_string,
    ffi::structs,
};

/// The highest priority an event, tag or attribute may carry:
/// `k_unMaxTimelinePriority` (`isteamtimeline.h`).
pub const MAX_TIMELINE_PRIORITY: u32 = 1000;

/// The longest a range event may last, in seconds:
/// `k_flMaxTimelineEventDuration` (`isteamtimeline.h`). A `float` constant,
/// which the drift gate's limit table — integers only — does not read.
pub const MAX_TIMELINE_EVENT_SECONDS: f32 = 600.0;

/// The longest phase id, in bytes: `k_cchMaxPhaseIDLength`
/// (`isteamtimeline.h`) is the size of the `char` array the id comes back in,
/// NUL included.
pub const MAX_PHASE_ID_LENGTH: usize = 63;

/// What the game is doing, which colours the timeline (`ETimelineGameMode`).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum GameMode {
    /// Playing (`k_ETimelineGameMode_Playing`).
    Playing,
    /// Between rounds, choosing, getting ready (`k_ETimelineGameMode_Staging`).
    Staging,
    /// In menus (`k_ETimelineGameMode_Menus`).
    Menus,
    /// Loading (`k_ETimelineGameMode_LoadingScreen`).
    LoadingScreen,
}

/// Whether Steam may offer an event as a clip
/// (`ETimelineEventClipPriority`).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
pub enum ClipPriority {
    /// Not a clip (`k_ETimelineEventClipPriority_None`).
    #[default]
    None,
    /// A clip worth offering (`k_ETimelineEventClipPriority_Standard`).
    Standard,
    /// A clip offered before the standard ones
    /// (`k_ETimelineEventClipPriority_Featured`).
    Featured,
}

impl ClipPriority {
    const fn raw(self) -> i32 {
        match self {
            Self::None => 1,
            Self::Standard => 2,
            Self::Featured => 3,
        }
    }
}

/// An event on the timeline; see [`Timeline::instant_event`].
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct TimelineEvent<'a> {
    /// The title, in the language `Utils::ui_language` names.
    pub title: &'a str,
    /// The description, likewise.
    pub description: &'a str,
    /// An icon uploaded on the partner site, or one of Steam's `steam_…`
    /// icons.
    pub icon: &'a str,
    /// How prominently it shows, up to [`MAX_TIMELINE_PRIORITY`].
    pub priority: u32,
    /// When it happened relative to now, in seconds; negative is the past.
    pub offset: f32,
    /// Whether Steam may offer it as a clip.
    pub clip: ClipPriority,
}

/// An event on the timeline, good for this game process
/// (`TimelineEventHandle_t`).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct TimelineEventId(pub u64);

/// The event's strings as Steam takes them, and its checked numbers.
struct Checked {
    title: std::ffi::CString,
    description: std::ffi::CString,
    icon: std::ffi::CString,
}

/// Checks an event before any call: its strings, priority and offset.
fn check(event: &TimelineEvent<'_>) -> Result<Checked, SteamError> {
    check_priority(event.priority)?;
    check_offset(event.offset)?;
    Ok(Checked {
        title: c_string(event.title, "title", usize::MAX)?,
        description: c_string(event.description, "description", usize::MAX)?,
        icon: c_string(event.icon, "icon", usize::MAX)?,
    })
}

fn check_priority(priority: u32) -> Result<(), SteamError> {
    (priority <= MAX_TIMELINE_PRIORITY)
        .then_some(())
        .ok_or(SteamError::OutOfRange("priority"))
}

fn check_offset(offset: f32) -> Result<(), SteamError> {
    offset
        .is_finite()
        .then_some(())
        .ok_or(SteamError::OutOfRange("offset"))
}

fn phase_id(id: &str) -> Result<std::ffi::CString, SteamError> {
    c_string(id, "phase id", MAX_PHASE_ID_LENGTH)
}

/// `ISteamTimeline`, borrowed from a [`Steam`]; from [`Steam::timeline`].
#[derive(Debug)]
pub struct Timeline<'a> {
    steam: &'a mut Steam,
}

impl Steam {
    /// The game-recording timeline.
    pub fn timeline(&mut self) -> Timeline<'_> {
        Timeline { steam: self }
    }
}

impl Timeline<'_> {
    /// Describes the current state, `offset` seconds from now
    /// (`SetTimelineTooltip`), replacing any description before it.
    ///
    /// # Errors
    ///
    /// [`SteamError::InteriorNul`] or [`SteamError::OutOfRange`] (a
    /// non-finite offset), before the call.
    pub fn set_tooltip(&self, description: &str, offset: f32) -> Result<(), SteamError> {
        check_offset(offset)?;
        let description = c_string(description, "description", usize::MAX)?;
        let client = &self.steam.client;
        // SAFETY: `client.timeline` is the non-null interface init resolved;
        // `Timeline` borrows the `!Send` `Steam`, so this is the pump thread;
        // `description` is NUL-terminated for the call.
        unsafe {
            (client.lib.fns.timeline.set_timeline_tooltip)(
                client.timeline,
                description.as_ptr(),
                offset,
            );
        };
        Ok(())
    }

    /// Clears the tooltip, `offset` seconds from now
    /// (`ClearTimelineTooltip`).
    ///
    /// # Errors
    ///
    /// [`SteamError::OutOfRange`] for a non-finite offset.
    pub fn clear_tooltip(&self, offset: f32) -> Result<(), SteamError> {
        check_offset(offset)?;
        let client = &self.steam.client;
        // SAFETY: as in `set_tooltip`.
        unsafe { (client.lib.fns.timeline.clear_timeline_tooltip)(client.timeline, offset) };
        Ok(())
    }

    /// Sets what the game is doing, which colours the timeline
    /// (`SetTimelineGameMode`).
    pub fn set_game_mode(&self, mode: GameMode) {
        let raw = match mode {
            GameMode::Playing => 1,
            GameMode::Staging => 2,
            GameMode::Menus => 3,
            GameMode::LoadingScreen => 4,
        };
        let client = &self.steam.client;
        // SAFETY: as in `set_tooltip`.
        unsafe { (client.lib.fns.timeline.set_timeline_game_mode)(client.timeline, raw) };
    }

    /// Marks a moment (`AddInstantaneousTimelineEvent`).
    ///
    /// # Errors
    ///
    /// [`SteamError::InteriorNul`] or [`SteamError::OutOfRange`] (a priority
    /// past [`MAX_TIMELINE_PRIORITY`], a non-finite offset), before the call.
    pub fn instant_event(&self, event: &TimelineEvent<'_>) -> Result<TimelineEventId, SteamError> {
        let checked = check(event)?;
        let client = &self.steam.client;
        // SAFETY: as in `set_tooltip`; every string is NUL-terminated for the
        // call.
        let handle = unsafe {
            (client.lib.fns.timeline.add_instantaneous_timeline_event)(
                client.timeline,
                checked.title.as_ptr(),
                checked.description.as_ptr(),
                checked.icon.as_ptr(),
                event.priority,
                event.offset,
                event.clip.raw(),
            )
        };
        Ok(TimelineEventId(handle))
    }

    /// Marks a finished stretch, `seconds` long from the event's offset
    /// (`AddRangeTimelineEvent`).
    ///
    /// # Errors
    ///
    /// As [`instant_event`](Self::instant_event), and
    /// [`SteamError::OutOfRange`] for a duration outside
    /// `0..=`[`MAX_TIMELINE_EVENT_SECONDS`].
    pub fn range_event(
        &self,
        event: &TimelineEvent<'_>,
        seconds: f32,
    ) -> Result<TimelineEventId, SteamError> {
        let checked = check(event)?;
        if !(0.0..=MAX_TIMELINE_EVENT_SECONDS).contains(&seconds) {
            return Err(SteamError::OutOfRange("duration"));
        }
        let client = &self.steam.client;
        // SAFETY: as in `instant_event`.
        let handle = unsafe {
            (client.lib.fns.timeline.add_range_timeline_event)(
                client.timeline,
                checked.title.as_ptr(),
                checked.description.as_ptr(),
                checked.icon.as_ptr(),
                event.priority,
                event.offset,
                seconds,
                event.clip.raw(),
            )
        };
        Ok(TimelineEventId(handle))
    }

    /// Starts a stretch that ends when the [`TimelineRange`] does
    /// (`StartRangeTimelineEvent`). Steam discards one still open at exit.
    ///
    /// # Errors
    ///
    /// As [`instant_event`](Self::instant_event).
    pub fn range_start(&self, event: &TimelineEvent<'_>) -> Result<TimelineRange, SteamError> {
        let checked = check(event)?;
        let client = &self.steam.client;
        // SAFETY: as in `instant_event`.
        let handle = unsafe {
            (client.lib.fns.timeline.start_range_timeline_event)(
                client.timeline,
                checked.title.as_ptr(),
                checked.description.as_ptr(),
                checked.icon.as_ptr(),
                event.priority,
                event.offset,
                event.clip.raw(),
            )
        };
        Ok(TimelineRange {
            client: Arc::clone(client),
            event: TimelineEventId(handle),
            open: true,
            _not_send: PhantomData,
        })
    }

    /// Removes an event this process added (`RemoveTimelineEvent`).
    pub fn remove_event(&self, event: TimelineEventId) {
        let client = &self.steam.client;
        // SAFETY: as in `set_tooltip`.
        unsafe { (client.lib.fns.timeline.remove_timeline_event)(client.timeline, event.0) };
    }

    /// Asks whether Steam kept a recording of `event`
    /// (`DoesEventRecordingExist`).
    ///
    /// # Errors
    ///
    /// [`SteamError::Refused`] when Steam did not start the call.
    pub fn event_recording(
        &mut self,
        event: TimelineEventId,
    ) -> Result<SteamCall<EventRecording>, SteamError> {
        let client = &self.steam.client;
        // SAFETY: as in `set_tooltip`.
        let handle = unsafe {
            (client.lib.fns.timeline.does_event_recording_exist)(client.timeline, event.0)
        };
        self.steam
            .calls
            .register(handle)
            .ok_or(SteamError::Refused("DoesEventRecordingExist"))
    }

    /// Starts a game phase — a match, a chapter, a run — ending any before it
    /// (`StartGamePhase`).
    pub fn start_phase(&self) {
        let client = &self.steam.client;
        // SAFETY: as in `set_tooltip`.
        unsafe { (client.lib.fns.timeline.start_game_phase)(client.timeline) };
    }

    /// Ends the current game phase (`EndGamePhase`).
    pub fn end_phase(&self) {
        let client = &self.steam.client;
        // SAFETY: as in `set_tooltip`.
        unsafe { (client.lib.fns.timeline.end_game_phase)(client.timeline) };
    }

    /// Names the current phase, so the game can refer back to it
    /// (`SetGamePhaseID`).
    ///
    /// # Errors
    ///
    /// [`SteamError::InteriorNul`] or [`SteamError::TooLong`] (past
    /// [`MAX_PHASE_ID_LENGTH`]), before the call.
    pub fn set_phase_id(&self, id: &str) -> Result<(), SteamError> {
        let id = phase_id(id)?;
        let client = &self.steam.client;
        // SAFETY: as in `set_tooltip`; `id` is NUL-terminated for the call.
        unsafe { (client.lib.fns.timeline.set_game_phase_id)(client.timeline, id.as_ptr()) };
        Ok(())
    }

    /// Tags the current phase — a hero played, a boss defeated
    /// (`AddGamePhaseTag`).
    ///
    /// # Errors
    ///
    /// [`SteamError::InteriorNul`] or [`SteamError::OutOfRange`] (a
    /// priority), before the call.
    pub fn add_phase_tag(
        &self,
        name: &str,
        icon: &str,
        group: &str,
        priority: u32,
    ) -> Result<(), SteamError> {
        check_priority(priority)?;
        let name = c_string(name, "tag name", usize::MAX)?;
        let icon = c_string(icon, "tag icon", usize::MAX)?;
        let group = c_string(group, "tag group", usize::MAX)?;
        let client = &self.steam.client;
        // SAFETY: as in `set_tooltip`; every string is NUL-terminated for the
        // call.
        unsafe {
            (client.lib.fns.timeline.add_game_phase_tag)(
                client.timeline,
                name.as_ptr(),
                icon.as_ptr(),
                group.as_ptr(),
                priority,
            );
        };
        Ok(())
    }

    /// Sets a text attribute of the current phase — a score, a character's
    /// name — replacing its last value (`SetGamePhaseAttribute`).
    ///
    /// # Errors
    ///
    /// As [`add_phase_tag`](Self::add_phase_tag).
    pub fn set_phase_attribute(
        &self,
        group: &str,
        value: &str,
        priority: u32,
    ) -> Result<(), SteamError> {
        check_priority(priority)?;
        let group = c_string(group, "attribute group", usize::MAX)?;
        let value = c_string(value, "attribute value", usize::MAX)?;
        let client = &self.steam.client;
        // SAFETY: as in `add_phase_tag`.
        unsafe {
            (client.lib.fns.timeline.set_game_phase_attribute)(
                client.timeline,
                group.as_ptr(),
                value.as_ptr(),
                priority,
            );
        };
        Ok(())
    }

    /// Asks what Steam kept of the phase named `id`
    /// (`DoesGamePhaseRecordingExist`).
    ///
    /// # Errors
    ///
    /// As [`set_phase_id`](Self::set_phase_id), and [`SteamError::Refused`]
    /// when Steam did not start the call.
    pub fn phase_recording(&mut self, id: &str) -> Result<SteamCall<PhaseRecording>, SteamError> {
        let id = phase_id(id)?;
        let client = &self.steam.client;
        // SAFETY: as in `set_phase_id`.
        let handle = unsafe {
            (client.lib.fns.timeline.does_game_phase_recording_exist)(client.timeline, id.as_ptr())
        };
        self.steam
            .calls
            .register(handle)
            .ok_or(SteamError::Refused("DoesGamePhaseRecordingExist"))
    }

    /// Opens the overlay at the phase named `id` (`OpenOverlayToGamePhase`).
    ///
    /// # Errors
    ///
    /// As [`set_phase_id`](Self::set_phase_id).
    pub fn open_overlay_to_phase(&self, id: &str) -> Result<(), SteamError> {
        let id = phase_id(id)?;
        let client = &self.steam.client;
        // SAFETY: as in `set_phase_id`.
        unsafe {
            (client.lib.fns.timeline.open_overlay_to_game_phase)(client.timeline, id.as_ptr());
        };
        Ok(())
    }

    /// Opens the overlay at `event` (`OpenOverlayToTimelineEvent`).
    pub fn open_overlay_to_event(&self, event: TimelineEventId) {
        let client = &self.steam.client;
        // SAFETY: as in `set_tooltip`.
        unsafe {
            (client.lib.fns.timeline.open_overlay_to_timeline_event)(client.timeline, event.0);
        };
    }
}

/// A stretch of the timeline begun with [`Timeline::range_start`], which
/// ends — `EndRangeTimelineEvent`, at the moment it ends — when this is
/// dropped or [`end`](Self::end)ed, exactly once.
///
/// Holds the session, as a `Lobby` does, so it needs no `&Steam`; `!Send`.
#[derive(Debug)]
pub struct TimelineRange {
    client: Arc<Client>,
    event: TimelineEventId,
    /// Until it is ended: `end` ends it and clears this, so `Drop` does not
    /// end it again.
    open: bool,
    _not_send: PhantomData<*const ()>,
}

impl TimelineRange {
    /// The event, for [`Timeline::remove_event`] or
    /// [`Timeline::open_overlay_to_event`].
    #[must_use]
    pub const fn event(&self) -> TimelineEventId {
        self.event
    }

    /// Changes what the event says while it is open
    /// (`UpdateRangeTimelineEvent`); its offset is ignored.
    ///
    /// # Errors
    ///
    /// As [`Timeline::instant_event`].
    pub fn update(&self, event: &TimelineEvent<'_>) -> Result<(), SteamError> {
        let checked = check(event)?;
        let client = &self.client;
        // SAFETY: `client.timeline` is live, `TimelineRange` is `!Send` so this
        // is the pump thread, and every string is NUL-terminated for the call.
        unsafe {
            (client.lib.fns.timeline.update_range_timeline_event)(
                client.timeline,
                self.event.0,
                checked.title.as_ptr(),
                checked.description.as_ptr(),
                checked.icon.as_ptr(),
                event.priority,
                event.clip.raw(),
            );
        };
        Ok(())
    }

    /// Ends the stretch `offset` seconds from now; dropping it ends it now.
    ///
    /// # Errors
    ///
    /// [`SteamError::OutOfRange`] for a non-finite offset, which then ends it
    /// now instead.
    pub fn end(mut self, offset: f32) -> Result<(), SteamError> {
        let checked = check_offset(offset);
        self.end_at(if checked.is_ok() { offset } else { 0.0 });
        checked
    }

    /// Ends the event at `offset`, if it is still open.
    fn end_at(&mut self, offset: f32) {
        if !std::mem::take(&mut self.open) {
            return;
        }
        let client = &self.client;
        // SAFETY: as in `update`; `open` makes this the event's one end.
        unsafe {
            (client.lib.fns.timeline.end_range_timeline_event)(
                client.timeline,
                self.event.0,
                offset,
            );
        };
    }
}

impl Drop for TimelineRange {
    fn drop(&mut self) {
        self.end_at(0.0);
    }
}

/// The answer to [`Timeline::event_recording`]
/// (`SteamTimelineEventRecordingExists_t`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct EventRecording {
    /// The event asked about.
    pub event: TimelineEventId,
    /// Whether Steam kept a recording of it.
    pub exists: bool,
}

impl Answer for EventRecording {
    const ROW: CallRow = CallRow {
        base: Base::Timeline,
        offset: 2,
        #[cfg(test)]
        name: "SteamTimelineEventRecordingExists_t",
        size: size_of::<structs::SteamTimelineEventRecordingExists>(),
    };

    fn build(bytes: &[u8], _: &mut Steam) -> Option<Self> {
        let raw = read::<structs::SteamTimelineEventRecordingExists>(bytes)?;
        Some(Self {
            event: TimelineEventId(raw.event),
            exists: raw.exists != 0,
        })
    }

    fn abandon(_: &[u8], _: &Client) {}
}

/// The answer to [`Timeline::phase_recording`]
/// (`SteamTimelineGamePhaseRecordingExists_t`).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PhaseRecording {
    /// The phase asked about.
    pub phase: String,
    /// How much of it Steam recorded.
    pub recorded: Duration,
    /// The longest clip of it.
    pub longest_clip: Duration,
    /// How many clips of it there are.
    pub clips: u32,
    /// How many screenshots of it there are.
    pub screenshots: u32,
}

impl Answer for PhaseRecording {
    const ROW: CallRow = CallRow {
        base: Base::Timeline,
        offset: 1,
        #[cfg(test)]
        name: "SteamTimelineGamePhaseRecordingExists_t",
        size: size_of::<structs::SteamTimelineGamePhaseRecordingExists>(),
    };

    fn build(bytes: &[u8], steam: &mut Steam) -> Option<Self> {
        let raw = read::<structs::SteamTimelineGamePhaseRecordingExists>(bytes)?;
        let (phase, lossy) = fixed_string(&raw.phase);
        if lossy {
            steam.lossy_strings.set(steam.lossy_strings.get() + 1);
        }
        Some(Self {
            phase,
            recorded: Duration::from_millis(raw.recording_ms),
            longest_clip: Duration::from_millis(raw.longest_clip_ms),
            clips: raw.clips,
            screenshots: raw.screenshots,
        })
    }

    fn abandon(_: &[u8], _: &Client) {}
}

#[cfg(test)]
mod tests;
