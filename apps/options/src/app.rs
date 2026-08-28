//! options' start-up, and the [`HostedGame`] methods the engine's loop calls.
//!
//! # There is no loop in this file
//!
//! ```text
//! Loop::frame()                     ← the engine's
//!   pump, input, menu, pause, resize
//!   run_ticks  ─────────────────────→ Screen::tick   (nothing: see below)
//!   draw_list.clear()
//!     ─────────────────────────────→ Screen::draw    (nothing: see below)
//!     menu, debug overlay             ← the engine's
//!   gpu.frame()
//! ```
//!
//! Two of those arrows do nothing here, and that is the sample rather than an
//! omission: the screen is a [`crcbl::ui::menu::Menu`], the engine's own menu
//! pass draws it, and there is no simulation behind it to step.
//! [`Screen::menu_kind`](HostedGame::menu_kind) is where this sample's whole
//! frame happens, because that is the callback handed the menu set.
//!
//! # What it edits, and what it does not
//!
//! The six `[engine.audio]` bus gains, which are the cheapest settings to make
//! real: they need no renderer change and no restart to write, and three of them
//! carry sound — see [`crate::audio`] — so a fader is audible while it moves.
//! The video half of `docs/plan/sample/20-options.md` — display mode,
//! resolution, present mode, frame cap, the quality tiers — is not here yet.
//! `docs/backlog.md` carries it.
//!
//! **Nothing in this process listens to these gains.** A settings file belongs
//! to the application that wrote it — natively, `~/.config/<label>/` — so the
//! music volume set here is this sample's own, and the round trip it proves is
//! its own restart: set a fader, quit, start again, and the fader is where it
//! was left. That is the claim the sample exists to make, and it is the one no
//! other application in the workspace makes, because no other one *writes* a
//! setting.

use crcbl::audio::mixer::Bus;
use crcbl::core::input::KeyCode;
use crcbl::engine::{
    Booted, Clock, ExitReason, FrameInfo, HostedGame, RunSummary, SettingsSource,
    wait_for_configure,
};
use crcbl::prelude::*;
use crcbl::shell::{DisplayMode, ShellBackend as Backend, WindowId};
use crcbl::store::settings::SettingsStack;

use crate::audio::Audio;
use crate::gpu::Gpu;
use crate::menu::{Action, MenuKind, Menus};

pub use crate::args::Options;

/// The name this sample's settings file is kept under, and the label its device
/// wears.
///
/// One constant for both because they are one fact — see
/// [`crcbl::engine::GpuContextDesc::label`].
pub const APP_NAME: &str = "options";

/// How often the screen logs its `[HUD]` line, in ticks.
///
/// A second of simulated time, which is the cadence every other sample's
/// heartbeat uses — the gates that read one are written against that spacing.
pub const HEARTBEAT_TICKS: u64 = 60;

// ---- summary -----------------------------------------------------------------

/// What a finished run reports.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Summary {
    pub backend: Backend,
    pub frames: u64,
    pub ticks: u64,
    pub events: u64,
    pub extent: (u32, u32),
    pub exit: ExitReason,
    /// The mode the window system actually had the window in, **not** the one
    /// the run last asked for.
    pub mode: DisplayMode,
    /// How many times a fader moved. Zero from a run nobody touched.
    pub edits: u64,
    /// What the last press of `SAVE` did.
    pub saved: SaveState,
}

// ---- errors ------------------------------------------------------------------

/// What can stop options: the loop's own failures, and no others.
///
/// [`core::convert::Infallible`] rather than an error enum with no variants
/// used, because this sample has no simulation that could fail — its work is
/// editing a settings stack, and the one fallible thing it does (writing the
/// file) is reported to the player on the screen rather than by ending the run.
pub type OptionsError = crcbl::engine::LoopError<core::convert::Infallible>;

// ---- the hosted game ---------------------------------------------------------

/// What the last `SAVE` did, which is what its row says.
#[derive(Clone, Debug, PartialEq, Eq, Default)]
pub enum SaveState {
    /// Nothing has been written yet, and nothing has been changed either.
    #[default]
    Untouched,
    /// A fader has moved since the last write.
    Unsaved,
    /// Written to the player's own settings file.
    Saved,
    /// This run has nowhere to write — a headless run, which must not touch
    /// whichever home directory it is executing in.
    Nowhere,
    /// The write was refused, with what the storage said.
    ///
    /// Kept and shown rather than logged and dropped: a player who pressed
    /// `SAVE` and was told nothing would go on believing their settings are on
    /// disk, which is the worst version of this bug.
    Failed(String),
}

impl SaveState {
    /// What this state says on the `SAVE` row.
    #[must_use]
    pub fn hint(&self) -> String {
        match self {
            Self::Untouched => String::new(),
            Self::Unsaved => "UNSAVED".to_string(),
            Self::Saved => "SAVED".to_string(),
            Self::Nowhere => "NOWHERE TO SAVE".to_string(),
            Self::Failed(error) => format!("FAILED: {error}"),
        }
    }
}

impl core::fmt::Display for SaveState {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        match self {
            Self::Untouched => write!(f, "nothing changed"),
            Self::Unsaved => write!(f, "unsaved edits"),
            Self::Saved => write!(f, "saved"),
            Self::Nowhere => write!(f, "nowhere to save"),
            Self::Failed(error) => write!(f, "save failed: {error}"),
        }
    }
}

/// Where this run's settings came from, and where `SAVE` writes them.
///
/// Not a [`SettingsSource`], which borrows the storage it names and would put a
/// lifetime on every type below this one. The two arms a run can be in are the
/// two [`SettingsSource::for_run`] picks, and this is that rule held as data.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Store {
    /// The player's own settings file, where this platform keeps it.
    Platform,
    /// Nowhere: a headless run is a test or a golden run, and neither may read
    /// or write whichever home directory it happens to execute in.
    None,
}

impl Store {
    /// The source a run that may be headless should use — the rule
    /// [`SettingsSource::for_run`] states, in this sample's own vocabulary.
    #[must_use]
    pub const fn for_run(headless: bool) -> Self {
        if headless { Self::None } else { Self::Platform }
    }

    /// Whether the run this store belongs to is headless.
    ///
    /// The inverse of [`Store::for_run`], and the reason it is worth a method:
    /// the audio side needs the same flag — a headless run wants
    /// `AudioStream::open_null` — and deriving it here keeps one answer rather
    /// than passing the bool down a second path that could disagree.
    #[must_use]
    pub const fn headless(self) -> bool {
        matches!(self, Self::None)
    }

    /// This store as the engine spells it.
    #[must_use]
    pub const fn source(self) -> SettingsSource<'static> {
        match self {
            Self::Platform => SettingsSource::Platform,
            Self::None => SettingsSource::None,
        }
    }
}

/// options, as the engine's loop hosts it.
#[derive(Debug)]
pub struct Screen {
    /// The player's settings, as opened at start-up and edited since.
    stack: SettingsStack,
    /// Where those settings came from, and where `SAVE` puts them back.
    store: Store,
    /// What each fader was last seen holding, in [`Bus::ALL`]'s order.
    ///
    /// The widget's own number, so a difference from it is the pointer and
    /// nothing else — `apps/viewer` makes the same comparison for the same
    /// reason.
    handles: [f32; Bus::ALL.len()],
    /// The gains those faders name, in the same order.
    gains: [f32; Bus::ALL.len()],
    /// Whether the faders have been put where the player's file says yet.
    ///
    /// **The first frame is not a drag, and without this it looks exactly like
    /// one.** [`HostedGame::menus`] is a `fn()` with no receiver, so the set is
    /// born with every handle at the top of its travel; a screen opened over a
    /// file that says `music_volume = 0.25` therefore finds its music handle at
    /// 1.0 while holding 0.5, which is the same comparison a pointer drag
    /// produces. Read as a drag, frame one would write 100% over the player's
    /// quarter and call it an edit.
    placed: bool,
    edits: u64,
    saved: SaveState,
    /// Ticks run, which is what the heartbeat counts.
    ticks: u64,
    /// The edit count the last heartbeat reported, so a moved fader is logged
    /// when it moves rather than up to a second later.
    logged_edits: u64,
    /// The cues the faders are heard on. See [`crate::audio`].
    audio: Audio,
}

/// The loop options runs in.
///
/// A type alias, because the loop is the engine's. `S` is the shell type: the
/// native path builds `Loop<dyn Shell>`, and the tests build
/// `Loop<HeadlessShell>` so they can inject the events a compositor would send.
pub type Loop<S = dyn Shell> = crcbl::engine::Loop<S, Screen>;

/// Runs the full loop.
///
/// # Errors
///
/// [`OptionsError`] if the shell or the GPU refused. Teardown runs on every
/// path.
pub fn run(options: &Options) -> Result<Summary, OptionsError> {
    crcbl::engine::drive(start(options)?)
}

/// Opens a shell, a window and a GPU, and reads the player's settings.
///
/// # Errors
///
/// [`OptionsError`] if any of them refused.
pub fn start(options: &Options) -> Result<Loop, OptionsError> {
    let shell = crcbl::engine::open_shell(options.common.headless)?;
    with_shell(shell, options)
}

/// Builds the loop on an already-open shell, blocking on both waits.
///
/// The browser cannot use this — a main thread may not sit in
/// [`wait_for_configure`] — and takes [`PendingLoop`] instead. What the two
/// share is everything after the waiting, which is `assemble`.
///
/// # Errors
///
/// [`OptionsError`] if the window never configured or the GPU would not open.
pub fn with_shell<S: Shell + ?Sized>(
    mut shell: Box<S>,
    options: &Options,
) -> Result<Loop<S>, OptionsError> {
    let clock_source = Clock::new(options.common.headless);
    let window = open_the_window(
        shell.as_mut(),
        &clock_source,
        options.common.display_mode(),
        options.common.size,
    )?;

    let mut events = 0;
    let extent = wait_for_configure(shell.as_mut(), window, &mut events)?;

    let gpu = Gpu::open(shell.as_ref(), window, extent, options.common.gpu())?;
    Ok(assemble(
        Booted {
            shell,
            window,
            gpu,
            clock_source,
            events,
        },
        options,
    ))
}

/// The half of start-up that is the same however the GPU arrived.
///
/// [`Booted`] is what both bring-up paths hand over, so the settings are read
/// and the loop assembled in one place rather than one per path — a second copy
/// is how the browser build would come to run a subtly different sample.
fn assemble<S: Shell + ?Sized>(booted: Booted<S, Gpu>, options: &Options) -> Loop<S> {
    // `--screenshot`, armed before the first frame because the frame it names
    // is counted from this point. The flag forces `--headless` on.
    //
    // The mutable binding lives inside the `cfg` rather than on the parameter:
    // a browser build arms nothing, so a `mut` in the signature would be one
    // the wasm32 target correctly reports as unused.
    #[cfg(not(target_arch = "wasm32"))]
    let booted = {
        let mut booted = booted;
        if let Some(request) = options.common.screenshot_request() {
            booted.gpu.context_mut().set_screenshot(request);
        }
        booted
    };
    Loop::new(
        booted,
        Screen::opened(Store::for_run(options.common.headless)),
        options.common.loop_config(),
    )
}

/// Creates the one window this sample has: its title, its app id, its size.
fn open_the_window<S: Shell + ?Sized>(
    shell: &mut S,
    clock_source: &Clock,
    mode: DisplayMode,
    size: Option<crcbl::shell::PhysicalSize>,
) -> Result<WindowId, OptionsError> {
    Ok(crcbl::engine::open_window(
        shell,
        clock_source,
        &WindowDesc {
            title: "Settings",
            app_id: "sh.kryptic.crcbl.options",
            size: crcbl::engine::requested_window_size(size),
            mode,
            ..WindowDesc::default()
        },
    )?)
}

impl Screen {
    /// Reads `store` and holds what it said.
    ///
    /// A store with nothing in it — a headless run, a player who has never
    /// pressed `SAVE` — is every bus at unity, which is what an absent
    /// `[engine.audio]` key means.
    #[must_use]
    pub fn opened(store: Store) -> Self {
        let stack = store.source().open(APP_NAME).unwrap_or_default();
        Self::over(stack, store)
    }

    /// The same screen over a stack the caller already has — what a test uses,
    /// and what a browser build will, since neither reads a config directory.
    #[must_use]
    pub fn over(stack: SettingsStack, store: Store) -> Self {
        let gains = crcbl::settings::audio_gains(&stack).map(|(_, gain)| gain);
        Self {
            stack,
            store,
            handles: gains.map(crate::menu::handle_at),
            // The mixer is opened on the gains this screen opened on, not on a
            // second read of the file: two readers of one file is how a screen
            // and the levels under it come to disagree about what the player
            // set.
            audio: Audio::new(store.headless(), &gains),
            gains,
            placed: false,
            edits: 0,
            saved: SaveState::default(),
            ticks: 0,
            logged_edits: 0,
        }
    }

    /// `bus`'s gain as this screen currently holds it.
    #[must_use]
    pub fn gain(&self, bus: Bus) -> f32 {
        self.gains[bus.index()]
    }

    /// How many times a fader has moved.
    #[must_use]
    pub const fn edits(&self) -> u64 {
        self.edits
    }

    /// What the last `SAVE` did.
    #[must_use]
    pub const fn saved(&self) -> &SaveState {
        &self.saved
    }

    /// The settings as they stand, for a test that wants to read the keys back
    /// without going near a config directory.
    #[must_use]
    pub const fn stack(&self) -> &SettingsStack {
        &self.stack
    }

    /// The `[HUD]` line, in the shape every other sample logs one.
    ///
    /// Every second of simulated time, plus the tick an edit lands on so a
    /// moved fader is not swallowed by the gap — the same rule `apps/hud` uses
    /// for a wave turning over, and for the same reason: the gate outside the
    /// process reads the change off this line and nothing else.
    ///
    /// It carries the gains rather than the handle positions. A handle is where
    /// a pointer left a widget; a gain is what the file will say, and the two
    /// differ by a square, so logging the wrong one would make a reader's
    /// arithmetic silently disagree with the key.
    fn log_heartbeat(&mut self) {
        let edited = self.edits != self.logged_edits;
        if !edited && !self.ticks.is_multiple_of(HEARTBEAT_TICKS) {
            return;
        }
        crcbl::log::info!(
            "[HUD] tick: {}  master: {}  music: {}  sfx: {}  ui: {}  voice: {}  \
             ambience: {}  edits: {}  file: {}",
            self.ticks,
            crate::menu::percent(self.gains[Bus::Master.index()]),
            crate::menu::percent(self.gains[Bus::Music.index()]),
            crate::menu::percent(self.gains[Bus::Sfx.index()]),
            crate::menu::percent(self.gains[Bus::Ui.index()]),
            crate::menu::percent(self.gains[Bus::Voice.index()]),
            crate::menu::percent(self.gains[Bus::Ambience.index()]),
            self.edits,
            self.saved,
        );
        self.logged_edits = self.edits;
    }

    /// Moves one bus, writing the key and marking the file unsaved.
    ///
    /// The single place a gain changes, so the key and the unsaved marker
    /// cannot come apart — a screen that moved a fader without writing the key
    /// is a setting the player watched themselves set and never got.
    ///
    /// **It does not touch `handles`**, which means "where the widget is" and
    /// not "where this screen would like it to be". Writing a wish into it is
    /// what makes the next frame's comparison read a change this screen made as
    /// a drag the player made.
    fn set(&mut self, bus: Bus, gain: f32) {
        self.gains[bus.index()] = gain;
        // The gain stage, moved with the number. This is the whole claim the
        // audio half of the sample exists to make, and putting it anywhere but
        // here is how a screen comes to show a level it is not applying.
        self.audio.set_bus_gain(bus, gain);
        self.edits += 1;
        self.saved = SaveState::Unsaved;
        if let Err(error) = crcbl::settings::set_audio_gain(&mut self.stack, bus, gain) {
            // The stack refused the value rather than the disk refusing the
            // file, which is a different failure and one the player can see on
            // the row: `SAVE` would write a file this key is missing from.
            self.saved = SaveState::Failed(error.to_string());
        }
    }

    /// Writes the edited settings back to wherever they were read from.
    fn save(&mut self) {
        let source = self.store.source();
        self.save_to(source);
    }

    /// Writes them to `source` instead.
    ///
    /// The seam [`SettingsSource::Source`] exists for — "a browser store, a
    /// dedicated server's configuration directory, a test's own storage" — and
    /// the only way to watch this screen write a file without it being whichever
    /// home directory the run happens to execute in. [`Store`] deliberately has
    /// no arm for it: a borrowed source would put a lifetime on the hosted game
    /// and therefore on the loop.
    pub fn save_to(&mut self, source: SettingsSource<'_>) {
        self.saved = match source.save(APP_NAME, &self.stack) {
            Ok(true) => SaveState::Saved,
            Ok(false) => SaveState::Nowhere,
            Err(error) => SaveState::Failed(error.to_string()),
        };
    }

    /// Puts every bus back to unity — which is what a file that says nothing
    /// means, and so what a player asking for the defaults is asking for.
    fn reset(&mut self) {
        for bus in Bus::ALL {
            self.set(bus, 1.0);
        }
    }
}

/// options' half of the frame, and nothing else.
impl HostedGame for Screen {
    /// Nothing this sample does can end a run. See [`OptionsError`].
    type Error = core::convert::Infallible;
    type Gpu = Gpu;
    type MenuKind = MenuKind;
    type MenuAction = Action;
    type Summary = Summary;

    const NAME: &'static str = APP_NAME;

    /// The screen with every fader at unity.
    ///
    /// **Not where the player's file arrives.** This is a `fn()` with no
    /// receiver — the engine builds the set before it has a game — so the
    /// handles start at the top of their travel and
    /// [`menu_kind`](Self::menu_kind) walks them down to the gains this screen
    /// opened with. That runs in `draw_menu`, before the menu is laid out and
    /// handed to either pass, so no frame is ever drawn with a fader in the
    /// wrong place.
    fn menus() -> Menus {
        crate::menu::menus(&Bus::ALL.map(|bus| (bus, 1.0)))
    }

    /// Nothing to simulate: a metronome, a counter and one line to log.
    ///
    /// `docs/plan/sample/20-options.md` exempts this sample from rules 2 and 10
    /// by name — no game state, no `World`, no `GameModule`, because the
    /// settings are the content — so what the tick does is keep the two clocks
    /// a settings screen still needs.
    ///
    /// The **count** is the heartbeat, and the heartbeat is what tells a paused
    /// loop from a running one from outside the process: the windowed harness
    /// and the browser gate both read it, and neither can ask this sample
    /// anything else, since a settings screen at rest looks exactly like a
    /// settings screen whose loop has stopped.
    ///
    /// The **metronome** is the effects bus's content. It runs on the fixed
    /// tick rather than on the frame, so the repeating effect keeps its period
    /// whatever the display is doing — and a paused loop, which runs no ticks,
    /// stops it, which is what a player pressing `ESC` is asking for.
    fn tick(&mut self, _gpu: &mut Gpu, tick_dt: f64) {
        self.ticks += 1;
        self.audio.advance(tick_dt);
        self.log_heartbeat();
    }

    /// Every key this screen answers to is the loop's or the menu's.
    ///
    /// `ESC` pauses, `F3` toggles the panel, `F11` goes fullscreen, and the
    /// arrows and the commit key drive the menu. There is nothing left for a
    /// binding of this sample's own to do.
    fn key_event(&mut self, _key: KeyCode, _pressed: bool) {}

    fn menu_action(id: crcbl::ui::WidgetId) -> Option<Action> {
        match id {
            crate::menu::SAVE_ID => Some(Action::Save),
            crate::menu::RESET_ID => Some(Action::Reset),
            // A fader. Sliders fire nothing, so this arm is unreachable through
            // the loop — it is here because the ids exist and a `_ => None`
            // that swallowed a real button would be silent.
            _ => None,
        }
    }

    fn apply(&mut self, action: Action) {
        // One click for the press, before the action: `RESET` moves six gains
        // through `set`, and a click per gain would be six clicks for one
        // keypress.
        self.audio.click();
        match action {
            Action::Save => self.save(),
            Action::Reset => self.reset(),
        }
    }

    /// **The whole of this sample's frame: the faders reconciled both ways.**
    ///
    /// A drag moves a handle and the gain has to follow; a `RESET` moves the
    /// gain and the handle has to follow. Both directions run here, in that
    /// order, and which one wins is decided by a comparison that cannot be
    /// fooled — `handles` is the number the widget itself last held, so a
    /// difference is the pointer and nothing else.
    fn menu_kind(&mut self, menus: &mut Menus, _paused: bool) -> MenuKind {
        if let Some(menu) = menus.get_mut(MenuKind::Settings) {
            for bus in Bus::ALL {
                let id = crate::menu::fader_id(bus);
                match menu.slider(id) {
                    Some(position) if self.placed && position != self.handles[bus.index()] => {
                        self.set(bus, crate::menu::gain_at(position));
                        // The handle the widget holds, not the one `set`
                        // derived: a drag is the authority on where the handle
                        // is, and writing the derived one back would fight the
                        // pointer for the rest of the drag.
                        self.handles[bus.index()] = position;
                        self.audio.fader_moved(bus, position);
                    }
                    _ => {
                        // The other direction: the gain moved — `RESET`, or the
                        // first frame's walk down from unity — and the fader
                        // has to follow it.
                        menu.set_slider(id, crate::menu::handle_at(self.gains[bus.index()]));
                        // Read back rather than assumed: the widget clamps, and
                        // a handle this screen believes is somewhere the widget
                        // is not reads as a drag on the very next frame.
                        if let Some(position) = menu.slider(id) {
                            self.handles[bus.index()] = position;
                            // Silent, and it carries the click's mark with the
                            // handle — a `RESET` is not a drag, and leaving the
                            // mark behind would click on the next drag's very
                            // first frame.
                            self.audio.fader_placed(bus, position);
                        }
                    }
                }
                // Every frame and unconditionally, so the number beside the
                // groove is the gain that would be written if `SAVE` were
                // pressed now.
                menu.set_item_hint(
                    id,
                    crate::menu::fader_hint(self.gains[bus.index()], Audio::sounds(bus)),
                );
            }
            menu.set_item_hint(crate::menu::SAVE_ID, self.saved.hint());
            self.placed = true;
        }
        MenuKind::Settings
    }

    /// The screen is a menu, so there is nothing for this sample to put in the
    /// draw list. The engine's own overlay is what fills it.
    fn draw(
        &mut self,
        _gpu: &mut Gpu,
        _draw_list: &mut crcbl::ui::draw_list::DrawList,
        _frame: FrameInfo,
    ) {
    }

    /// The audio section, which is this sample's only live system.
    ///
    /// The engine's own sections carry the frame times and the GPU; what only
    /// this sample can report is whether its cues are reaching a mixer, which
    /// is the thing a fader is claiming to control.
    fn debug_sections(&self, panel: &mut crcbl::ui::DebugPanel) {
        panel.add(&self.audio);
    }

    fn summary(&self, run: RunSummary) -> Summary {
        Summary {
            backend: run.backend,
            frames: run.frames,
            ticks: run.ticks,
            events: run.events,
            extent: run.extent,
            exit: run.exit,
            mode: run.mode,
            edits: self.edits,
            saved: self.saved.clone(),
        }
    }

    fn log_summary(summary: &Summary) {
        crcbl::log::info!(
            "options: {} frames, {} edit(s), {} ({:?})",
            summary.frames,
            summary.edits,
            summary.saved,
            summary.exit,
        );
    }
}

// ---- polled start-up ---------------------------------------------------------

/// A [`Loop`] being started one poll at a time, for a caller that may not
/// block — which on a browser main thread is every caller.
///
/// The state machine, the pump and the resize-during-start-up race are
/// [`crcbl::engine::PolledBoot`]'s; all that is left here is this sample's
/// `Options` and the `assemble` call the engine deliberately stops short of.
#[derive(Debug)]
pub struct PendingLoop<S: Shell + ?Sized = dyn Shell> {
    boot: crcbl::engine::PolledBoot<S, Gpu>,
    options: Options,
}

impl<S: Shell + ?Sized> PendingLoop<S> {
    /// Creates the window and starts the wait, without blocking on either half.
    ///
    /// `clock_source` is the caller's because the browser's cannot be
    /// [`Clock::new`]'s: `std::time::Instant::now` panics on
    /// `wasm32-unknown-unknown`, so a page drives the loop from
    /// `performance.now()` instead.
    ///
    /// # Errors
    ///
    /// [`OptionsError`] if the shell refused the window.
    pub fn request(
        mut shell: Box<S>,
        options: &Options,
        clock_source: Clock,
    ) -> Result<Self, OptionsError> {
        let window = open_the_window(
            shell.as_mut(),
            &clock_source,
            options.common.display_mode(),
            options.common.size,
        )?;
        Ok(Self {
            boot: crcbl::engine::PolledBoot::request(
                shell,
                window,
                clock_source,
                options.common.gpu(),
                (),
            ),
            options: options.clone(),
        })
    }

    /// Advances start-up. `Ok(None)` means "not yet, poll again next frame".
    ///
    /// # Errors
    ///
    /// [`OptionsError`] if the window went away before it had a size, or if the
    /// device request failed.
    pub fn poll(&mut self) -> Result<Option<Loop<S>>, OptionsError> {
        let Some(booted) = self.boot.poll::<OptionsError>()? else {
            return Ok(None);
        };
        Ok(Some(assemble(booted, &self.options)))
    }
}

// ---- tests -------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use crcbl::store::StorageSource as _;
    use crcbl::store::settings::SETTINGS_FILE;

    /// A settings file holding `toml`, with nothing behind it but memory.
    fn settings_file(toml: &str) -> crcbl::store::MemoryStorage {
        let storage = crcbl::store::MemoryStorage::new();
        storage
            .write(std::path::Path::new(SETTINGS_FILE), toml.as_bytes())
            .expect("memory storage accepts every write");
        storage
    }

    /// A screen over `toml`, with the menu set the loop would have built.
    fn screen(toml: &str) -> (Screen, Menus) {
        let storage = settings_file(toml);
        (
            Screen::over(SettingsStack::from_storage(&storage), Store::None),
            Screen::menus(),
        )
    }

    /// One turn of the only callback this sample does anything in.
    fn reconcile(screen: &mut Screen, menus: &mut Menus) {
        assert_eq!(
            screen.menu_kind(menus, false),
            MenuKind::Settings,
            "the screen is shown on every frame, paused or not",
        );
    }

    fn handle(menus: &mut Menus, bus: Bus) -> f32 {
        menus
            .get_mut(MenuKind::Settings)
            .expect("the one menu")
            .slider(crate::menu::fader_id(bus))
            .expect("every bus has a fader")
    }

    fn hint(menus: &mut Menus, id: crcbl::ui::WidgetId) -> String {
        menus
            .get_mut(MenuKind::Settings)
            .expect("the one menu")
            .items()
            .iter()
            .find(|item| item.id == id)
            .expect("the row")
            .hint
            .clone()
    }

    /// **The player's file reaches the faders before a frame is drawn.**
    ///
    /// `HostedGame::menus` is a `fn()` and cannot see a settings stack, so the
    /// set is born at unity; this is the walk down to what the file said, and
    /// it happens in `draw_menu` before the menu is laid out.
    #[test]
    fn a_screen_opens_with_every_fader_where_the_file_left_it() {
        let (mut screen, mut menus) = screen("[engine.audio]\nmusic_volume = 0.25\n");
        assert_eq!(screen.gain(Bus::Music), 0.25);
        assert_eq!(screen.gain(Bus::Sfx), 1.0, "an absent key is unity");

        assert_eq!(
            handle(&mut menus, Bus::Music),
            1.0,
            "the set is born at unity"
        );
        reconcile(&mut screen, &mut menus);
        assert!(
            (handle(&mut menus, Bus::Music) - 0.5).abs() < 1e-5,
            "a quarter of the amplitude is half the travel",
        );
        assert_eq!(hint(&mut menus, crate::menu::fader_id(Bus::Music)), "25%");
        assert_eq!(screen.edits(), 0, "placing a handle is not an edit");
    }

    /// A drag writes the key, and the key is the one a start-up reads.
    #[test]
    fn moving_a_fader_writes_the_key_that_start_up_reads_back() {
        let (mut screen, mut menus) = screen("");
        reconcile(&mut screen, &mut menus);

        menus
            .get_mut(MenuKind::Settings)
            .expect("the one menu")
            .set_slider(crate::menu::fader_id(Bus::Music), 0.5);
        reconcile(&mut screen, &mut menus);

        assert!((screen.gain(Bus::Music) - 0.25).abs() < 1e-6);
        assert_eq!(screen.edits(), 1);
        assert_eq!(screen.saved(), &SaveState::Unsaved);
        let read_back = crcbl::settings::audio_gains(screen.stack());
        assert!(
            (read_back[Bus::Music.index()].1 - 0.25).abs() < 1e-6,
            "the screen moved a fader without writing the key it is for",
        );
        assert_eq!(
            read_back[Bus::Sfx.index()].1,
            1.0,
            "one fader moved one bus",
        );
    }

    /// **The round trip this sample exists for**, with a store standing in for
    /// the player's config directory: set it, write it, open it again, and the
    /// fader is where it was left.
    #[test]
    fn a_saved_gain_is_still_there_when_the_screen_opens_again() {
        let storage = crcbl::store::MemoryStorage::new();
        let mut screen = Screen::over(SettingsStack::from_storage(&storage), Store::None);
        let mut menus = Screen::menus();
        reconcile(&mut screen, &mut menus);

        menus
            .get_mut(MenuKind::Settings)
            .expect("the one menu")
            .set_slider(crate::menu::fader_id(Bus::Ambience), 0.5);
        reconcile(&mut screen, &mut menus);
        screen.save_to(SettingsSource::Source(&storage));
        assert_eq!(screen.saved(), &SaveState::Saved);

        let reopened = Screen::over(SettingsStack::from_storage(&storage), Store::None);
        assert!(
            (reopened.gain(Bus::Ambience) - 0.25).abs() < 1e-6,
            "the restart read back {} rather than the quarter that was saved",
            reopened.gain(Bus::Ambience),
        );
        assert_eq!(reopened.gain(Bus::Music), 1.0, "nothing else moved");
    }

    /// **The other half of the round trip: the key reaches a gain stage.**
    ///
    /// Every other test here reads the number back off the stack, which a
    /// screen that showed a level it never applied would pass exactly as well.
    /// This one reads it off the mixer the voices are multiplied by.
    #[test]
    fn moving_a_fader_moves_the_gain_stage_and_not_only_the_key() {
        let (mut screen, mut menus) = screen("");
        reconcile(&mut screen, &mut menus);
        assert_eq!(
            screen.audio.bus_gain(Bus::Music),
            1.0,
            "an empty file is unity"
        );

        menus
            .get_mut(MenuKind::Settings)
            .expect("the one menu")
            .set_slider(crate::menu::fader_id(Bus::Music), 0.5);
        reconcile(&mut screen, &mut menus);

        assert!(
            (screen.audio.bus_gain(Bus::Music) - 0.25).abs() < 1e-6,
            "the mixer is at {} while the screen shows {}",
            screen.audio.bus_gain(Bus::Music),
            screen.gain(Bus::Music),
        );
        assert_eq!(
            screen.audio.bus_gain(Bus::Sfx),
            1.0,
            "one fader moved one stage",
        );
    }

    /// A screen opened over a file has the mixer on that file's gains before
    /// anything is reconciled — the faders walk down to the file, the levels do
    /// not have to.
    #[test]
    fn a_screen_opens_with_the_mixer_already_on_the_file_s_gains() {
        let (screen, _) = screen("[engine.audio]\nmusic_volume = 0.25\n");
        assert!((screen.audio.bus_gain(Bus::Music) - 0.25).abs() < 1e-6);
    }

    /// **A fader that moves nothing audible says so.**
    ///
    /// `docs/plan/sample/20-options.md`'s exit criteria want a control with no
    /// implementation labelled as such, and two of the six buses have no cue —
    /// see [`crate::audio`]. Without the mark they are indistinguishable from
    /// audio that is broken.
    #[test]
    fn a_bus_with_nothing_on_it_says_so_beside_its_gain() {
        let (mut screen, mut menus) = screen("");
        reconcile(&mut screen, &mut menus);

        for bus in Bus::ALL {
            let hint = hint(&mut menus, crate::menu::fader_id(bus));
            assert_eq!(
                hint.contains(crate::menu::SILENT_MARK),
                !Audio::sounds(bus),
                "{bus:?}'s row reads {hint:?}",
            );
        }
    }

    /// A headless run must not write into whichever home directory it is
    /// executing in — and a player who pressed `SAVE` has to be told which
    /// happened. The row says so rather than the log.
    #[test]
    fn a_run_with_nowhere_to_save_says_so_on_the_button() {
        let (mut screen, mut menus) = screen("");
        assert_eq!(Store::for_run(true), Store::None);
        assert_eq!(Store::for_run(false), Store::Platform);

        screen.apply(Action::Save);
        assert_eq!(screen.saved(), &SaveState::Nowhere);
        reconcile(&mut screen, &mut menus);
        assert_eq!(hint(&mut menus, crate::menu::SAVE_ID), "NOWHERE TO SAVE");
    }

    /// `RESET` is every bus back to unity — which is what an absent key means,
    /// so it is the defaults rather than a set of zeroes.
    #[test]
    fn reset_puts_every_bus_back_to_unity_and_the_faders_follow() {
        let (mut screen, mut menus) =
            screen("[engine.audio]\nmusic_volume = 0.25\nvoice_volume = 0.0\n");
        reconcile(&mut screen, &mut menus);

        screen.apply(Action::Reset);
        reconcile(&mut screen, &mut menus);

        for bus in Bus::ALL {
            assert_eq!(screen.gain(bus), 1.0, "{bus:?} did not go back to unity");
            assert_eq!(
                handle(&mut menus, bus),
                1.0,
                "{bus:?}'s fader did not follow"
            );
        }
        assert_eq!(screen.saved(), &SaveState::Unsaved);
    }

    /// The two buttons are the only ids that fire, and a fader never does.
    #[test]
    fn only_the_two_buttons_name_an_action() {
        assert_eq!(
            Screen::menu_action(crate::menu::SAVE_ID),
            Some(Action::Save)
        );
        assert_eq!(
            Screen::menu_action(crate::menu::RESET_ID),
            Some(Action::Reset)
        );
        for bus in Bus::ALL {
            assert_eq!(Screen::menu_action(crate::menu::fader_id(bus)), None);
        }
    }
}
