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
//! And the `[engine.video]` keys that have a reader: `frame_limit`, which is the
//! cheapest of the video keys for the opposite reason — the loop reads it once,
//! at start-up, so the row can write it honestly without a way to re-mode a
//! live window — and `anisotropic_filtering`, `render_scale` and the effect
//! switches of `crcbl::settings::VIDEO_KEYS`, which a `ForwardRenderer` takes
//! and this sample has none to hand them to. The rest of
//! `docs/plan/sample/20-options.md`'s video half — display mode, resolution,
//! present mode, the quality tiers — is not here yet, and `docs/backlog.md`
//! says what each of them is waiting on.
//!
//! **The video rows do not apply as they move.** Everything else here reaches
//! its stage in the same call that writes its key; the ceiling reaches the loop
//! only when a loop is built, and the anisotropy, the scale and the switches
//! reach a renderer only where a scene is drawn. The mark `Screen`'s frame
//! writes on those rows is what stops any of them being a silent lie.
//!
//! **Nothing in this process listens to these gains.** A settings file belongs
//! to the application that wrote it — natively, `~/.config/<label>/` — so the
//! music volume set here is this sample's own, and the round trip it proves is
//! its own restart: set a fader, quit, start again, and the fader is where it
//! was left. That is the claim the sample exists to make, and it is the one no
//! other application in the workspace makes, because no other one *writes* a
//! setting.

use crcbl::audio::mixer::Bus;
use crcbl::console::Value;
use crcbl::core::input::KeyCode;
use crcbl::engine::{
    Booted, Clock, ExitReason, FrameInfo, FrameLimit, HostedGame, RunSummary, SettingsSource,
    wait_for_configure,
};
use crcbl::prelude::*;
use crcbl::render::{Antialiasing, DEFAULT_ANISOTROPY, RenderEffects};
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
    /// `[engine.video] frame_limit` as the screen currently holds it.
    cap: FrameLimit,
    /// The ceiling this run **started** on.
    ///
    /// The loop takes its limit once, when `Loop::new` holds the game's own
    /// `--fps` under the file's — so a cap changed on the screen is a cap the
    /// next start will use and this one will not. Keeping what the run opened
    /// with is the only way the row can say so; comparing against the stack
    /// would compare the setting with itself.
    opened_cap: FrameLimit,
    /// The ceiling the **game** asked for — its `--fps`, or the default it was
    /// built with — which the file's ceiling is held against in `Loop::new`.
    ///
    /// Kept so the row can show the rate the run resolves to beside the one
    /// the file names, when the two differ; see [`crate::menu::HELD_MARK`].
    asked: FrameLimit,
    /// `[engine.video] anisotropic_filtering` as the screen currently holds it.
    anisotropy: f32,
    /// The anisotropy this run **started** on, for `opened_cap`'s reason: a
    /// renderer takes the key when it opens, and this sample opens none, so the
    /// value the run came up with is the only one it can claim to be under.
    opened_anisotropy: f32,
    /// `[engine.video] render_scale` as the screen currently holds it.
    scale: f32,
    /// What the scale groove was last seen holding — `handles`' rule for the
    /// one groove that is not a fader.
    scale_handle: f32,
    /// The scale this run **started** on, for `opened_anisotropy`'s reason.
    opened_scale: f32,
    /// The effects `[engine.video]` allows, as the screen currently holds them.
    effects: RenderEffects,
    /// The effects this run **started** with, for `opened_anisotropy`'s reason.
    opened_effects: RenderEffects,
    /// `[engine.video] antialiasing` as the screen currently holds it.
    ///
    /// A tier rather than an `Option<Antialiasing>`: the row always sits on a
    /// rung, and the rung an absent key means is the game's own —
    /// `menu::DEFAULT_ANTIALIASING`. What the key holds is the screen's answer
    /// to "which filter", and "the player has not said" is not one of the
    /// answers a row can show.
    antialiasing: Antialiasing,
    /// The tier this run **started** on, for `opened_anisotropy`'s reason.
    opened_antialiasing: Antialiasing,
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
    let config = options.common.loop_config();
    Loop::new(
        booted,
        // The game's own limit goes to the screen as well as to the loop, so
        // the row can show what the file's ceiling resolves to against it.
        Screen::opened(Store::for_run(options.common.headless), config.limit),
        config,
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
    /// Reads `store` and holds what it said, beside `asked`, the frame limit
    /// the game itself is running under — see [`Screen::over`].
    ///
    /// A store with nothing in it — a headless run, a player who has never
    /// pressed `SAVE` — is every bus at unity, which is what an absent
    /// `[engine.audio]` key means.
    #[must_use]
    pub fn opened(store: Store, asked: FrameLimit) -> Self {
        let stack = store.source().open(APP_NAME).unwrap_or_default();
        Self::over(stack, store, asked)
    }

    /// The same screen over a stack the caller already has — what a test uses,
    /// and what a browser build will, since neither reads a config directory.
    ///
    /// `asked` is the game's own frame limit, the one `Loop::new` holds under
    /// the file's ceiling; the row shows the rate that resolves to when it is
    /// not the ceiling itself.
    #[must_use]
    pub fn over(stack: SettingsStack, store: Store, asked: FrameLimit) -> Self {
        let gains = crcbl::settings::audio_gains(&stack).map(|(_, gain)| gain);
        let cap = crcbl::settings::frame_limit(&stack);
        let anisotropy = crcbl::settings::anisotropic_filtering(&stack);
        let scale = crcbl::settings::render_scale(&stack);
        let effects = crcbl::settings::video_effects(&stack);
        let antialiasing =
            crcbl::settings::antialiasing(&stack).unwrap_or(crate::menu::DEFAULT_ANTIALIASING);
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
            cap,
            opened_cap: cap,
            asked,
            anisotropy,
            opened_anisotropy: anisotropy,
            scale,
            scale_handle: crate::menu::scale_handle_at(scale),
            opened_scale: scale,
            effects,
            opened_effects: effects,
            antialiasing,
            opened_antialiasing: antialiasing,
        }
    }

    /// `bus`'s gain as this screen currently holds it.
    #[must_use]
    pub fn gain(&self, bus: Bus) -> f32 {
        self.gains[bus.index()]
    }

    /// The frame ceiling as this screen currently holds it.
    #[must_use]
    pub const fn cap(&self) -> FrameLimit {
        self.cap
    }

    /// The page anisotropy as this screen currently holds it.
    #[must_use]
    pub const fn anisotropy(&self) -> f32 {
        self.anisotropy
    }

    /// The render scale as this screen currently holds it.
    #[must_use]
    pub const fn render_scale(&self) -> f32 {
        self.scale
    }

    /// The effects the screen currently allows.
    #[must_use]
    pub const fn effects(&self) -> RenderEffects {
        self.effects
    }

    /// The antialiasing tier as this screen currently holds it.
    #[must_use]
    pub const fn antialiasing(&self) -> Antialiasing {
        self.antialiasing
    }

    /// How many times a setting has been changed.
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
             ambience: {}  cap: {}  aniso: {}  scale: {}  aa: {}  \
             effects: {} of {}  edits: {}  file: {}",
            self.ticks,
            crate::menu::percent(self.gains[Bus::Master.index()]),
            crate::menu::percent(self.gains[Bus::Music.index()]),
            crate::menu::percent(self.gains[Bus::Sfx.index()]),
            crate::menu::percent(self.gains[Bus::Ui.index()]),
            crate::menu::percent(self.gains[Bus::Voice.index()]),
            crate::menu::percent(self.gains[Bus::Ambience.index()]),
            crate::menu::frame_cap_label(self.cap),
            crate::menu::anisotropy_label(self.anisotropy),
            crate::menu::percent(self.scale),
            crate::menu::antialiasing_label(self.antialiasing),
            // The switches that are on, not the bits that are set: the two
            // resolve bits are the `aa:` field's and are never in this table,
            // so counting bits would read as more effects than there are rows.
            crcbl::settings::VIDEO_KEYS
                .iter()
                .filter(|(_, effect)| self.effects.contains(*effect))
                .count(),
            crcbl::settings::VIDEO_KEYS.len(),
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
        self.edited();
        // `apply` writes the key **and** moves the gain stage, through
        // `impl Stage for Audio`. Moving the level is the whole claim the audio
        // half of this sample makes, and it is in the engine's one write-and-
        // apply body rather than here so that a console setting `music_volume`
        // moves the same fader through the same call.
        self.write(
            &format!(
                "{}.{}",
                crcbl::settings::AUDIO_NAMESPACE,
                bus.settings_key()
            ),
            &Value::Float(gain),
        );
    }

    /// Counts an edit and marks the file unsaved.
    ///
    /// Separate from [`Screen::write`] because a row can be **one** edit over
    /// several keys — the effect switches write every entry of `VIDEO_KEYS` —
    /// and a count that followed the keys would report a player who flipped one
    /// switch as having changed the whole table.
    fn edited(&mut self) {
        self.edits += 1;
        self.saved = SaveState::Unsaved;
    }

    /// Writes one catalogue key through [`crcbl::settings::apply`], which writes
    /// the stack and applies whatever this screen can apply.
    ///
    /// **The single place this screen writes a setting**, and the reason
    /// `docs/plan/52-debug-console.md` decision 3 moved the fan-out into the
    /// engine: the per-key spelling, the clamp and the live application were
    /// this file's, and a console would have had to copy all three.
    ///
    /// A refusal — a value outside the key's domain, or a stack with no user
    /// layer — lands on the row as [`SaveState::Failed`], which is where it
    /// landed before: `SAVE` would otherwise write a file this key is missing
    /// from and say nothing about it.
    fn write(&mut self, key: &str, value: &Value) {
        if let Err(fault) = crcbl::settings::apply(&mut self.stack, key, value, &mut self.audio) {
            self.saved = SaveState::Failed(fault.to_string());
        }
    }

    /// The dotted key of one `[engine.video]` row.
    fn video_key(name: &str) -> String {
        format!("{}.{name}", crcbl::settings::VIDEO_NAMESPACE)
    }

    /// Moves the frame ceiling, writing the key and marking the file unsaved.
    ///
    /// [`Screen::set`]'s rule for the other half of the screen: one place a
    /// value changes, so the key and the unsaved marker cannot come apart.
    /// Nothing is applied to the running loop — the loop took its limit when it
    /// was built — which is why this is the one setting here whose row has
    /// something to say beyond its value.
    fn set_cap(&mut self, cap: FrameLimit) {
        self.cap = cap;
        self.edited();
        self.write(
            &Self::video_key(crcbl::settings::FRAME_LIMIT_KEY),
            &Value::Int(i64::from(cap.rate())),
        );
    }

    /// What the row says: the ceiling; the rate the game's own limit holds it
    /// to, where that is lower; and whether this run is running under it.
    ///
    /// The held-to rate is computed against the ceiling the row *shows*, so a
    /// stepped ceiling says what the next start would resolve to — the game's
    /// own limit is a fact about the binary, not about this run.
    fn cap_hint(&self) -> String {
        let mut hint = crate::menu::frame_cap_label(self.cap);
        let resolved = self.asked.clamped_to(self.cap);
        if resolved != self.cap {
            hint = format!(
                "{hint}, {} {}",
                crate::menu::HELD_MARK,
                crate::menu::frame_cap_label(resolved)
            );
        }
        if self.cap != self.opened_cap {
            hint = format!("{hint} {}", crate::menu::NEXT_START_MARK);
        }
        hint
    }

    /// Moves the page anisotropy, writing the key and marking the file unsaved,
    /// on [`Screen::set_cap`]'s terms — and applied to nothing, since this
    /// sample draws no page. `apps/viewer` is where the key reaches
    /// `ForwardRenderer::set_anisotropy`.
    fn set_anisotropy(&mut self, anisotropy: f32) {
        self.anisotropy = anisotropy;
        self.edited();
        self.write(
            &Self::video_key(crcbl::settings::ANISOTROPIC_FILTERING_KEY),
            &Value::Float(anisotropy),
        );
    }

    /// What the row says: the anisotropy, and whether this run came up on it.
    ///
    /// Compared by bits rather than by `==`, so a `NaN` the file could never
    /// hold does not read as a change on every frame.
    fn anisotropy_hint(&self) -> String {
        let label = crate::menu::anisotropy_label(self.anisotropy);
        if self.anisotropy.to_bits() == self.opened_anisotropy.to_bits() {
            label
        } else {
            format!("{label} {}", crate::menu::NEXT_START_MARK)
        }
    }

    /// Moves the render scale, writing the key and marking the file unsaved,
    /// on [`Screen::set_cap`]'s terms — and applied to nothing, since this
    /// sample draws no scene. `apps/viewer` is where the key reaches
    /// `ForwardRenderer::set_render_scale`.
    ///
    /// Like [`Screen::set`] it does not touch `scale_handle`: the groove is
    /// the authority on where its handle is.
    fn set_scale(&mut self, scale: f32) {
        self.scale = scale;
        self.edited();
        self.write(
            &Self::video_key(crcbl::settings::RENDER_SCALE_KEY),
            &Value::Float(scale),
        );
    }

    /// What the groove says: the scale, and whether this run came up on it,
    /// compared by bits for [`Screen::anisotropy_hint`]'s reason.
    fn scale_hint(&self) -> String {
        let label = crate::menu::percent(self.scale);
        if self.scale.to_bits() == self.opened_scale.to_bits() {
            label
        } else {
            format!("{label} {}", crate::menu::NEXT_START_MARK)
        }
    }

    /// Moves the effect set, writing every effect key and marking the file
    /// unsaved, on [`Screen::set_cap`]'s terms — and applied to nothing, since
    /// this sample draws no scene.
    ///
    /// Every key rather than the one that flipped, because that is what
    /// `set_video_effects` writes and the file it leaves says what the player
    /// chose for each switch rather than only where they differed.
    fn set_effects(&mut self, effects: RenderEffects) {
        self.effects = effects;
        self.edited();
        for (key, effect) in crcbl::settings::VIDEO_KEYS {
            self.write(
                &Self::video_key(key),
                &Value::Bool(effects.contains(effect)),
            );
        }
    }

    /// Moves the antialiasing tier, writing the key and marking the file
    /// unsaved, on [`Screen::set_cap`]'s terms — and applied to nothing, since
    /// this sample draws no scene. `GpuContext::effect_request` is where the key
    /// reaches a renderer, one start later.
    fn set_antialiasing(&mut self, tier: Antialiasing) {
        self.antialiasing = tier;
        self.edited();
        self.write(
            &Self::video_key(crcbl::settings::ANTIALIASING_KEY),
            &Value::Enum(tier.name()),
        );
    }

    /// What the AA row says: the tier, and whether this run came up on it.
    fn antialiasing_hint(&self) -> String {
        let label = crate::menu::antialiasing_label(self.antialiasing);
        if self.antialiasing == self.opened_antialiasing {
            label.to_string()
        } else {
            format!("{label} {}", crate::menu::NEXT_START_MARK)
        }
    }

    /// What a switch says: on or off, and whether this run came up that way.
    fn effect_hint(&self, effect: RenderEffects) -> String {
        let on = self.effects.contains(effect);
        let label = crate::menu::switch_label(on);
        if on == self.opened_effects.contains(effect) {
            label.to_string()
        } else {
            format!("{label} {}", crate::menu::NEXT_START_MARK)
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

    /// Puts every setting back to what a file that says nothing means — every
    /// bus at unity, no frame ceiling, the engine's own anisotropy, the whole
    /// extent, every effect allowed and the game's own antialiasing tier —
    /// which is what a player asking for the defaults is asking for.
    ///
    /// **The tier is written rather than removed**, which is the one place this
    /// falls short of an absent key: the key has no word for "unpicked", so a
    /// reset names the rung an absent key would have meant. A later change to
    /// `RenderEffects::DEFAULT_STACK` would leave that file holding the old
    /// rung, which is the same bargain `set_video_effects` makes when it writes
    /// `true` for an effect the player kept.
    fn reset(&mut self) {
        for bus in Bus::ALL {
            self.set(bus, 1.0);
        }
        self.set_cap(FrameLimit::unlimited());
        self.set_anisotropy(DEFAULT_ANISOTROPY);
        self.set_scale(1.0);
        self.set_effects(RenderEffects::all());
        self.set_antialiasing(crate::menu::DEFAULT_ANTIALIASING);
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
            // A groove, a cycler or a switch. None of them fires — a value row
            // reports nothing from either device — so the `None` here is
            // unreachable through the loop; it is there because the ids exist
            // and a catch-all that swallowed a real button would be silent.
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

    /// **The whole of this sample's frame: every value row reconciled both
    /// ways.**
    ///
    /// A drag moves a handle and the gain has to follow; a `RESET` moves the
    /// gain and the handle has to follow. Both directions run here, in that
    /// order, and which one wins is decided by a comparison that cannot be
    /// fooled — `handles` is the number the widget itself last held, so a
    /// difference is the pointer and nothing else. The cyclers are the same
    /// shape with a rung for a handle: the widget's choice is compared with
    /// the rung the screen's value sits on, a difference is a key or a click,
    /// and the widget is put back on the value's rung afterwards — which is
    /// where [`crate::menu::stepped`]'s off-ladder rule needs it to be.
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
            // The scale groove, reconciled both ways on the faders' rule — and
            // silently, since the detent click is a bus's and this is no bus.
            let id = crate::menu::RENDER_SCALE_ID;
            match menu.slider(id) {
                Some(position) if self.placed && position != self.scale_handle => {
                    self.set_scale(crate::menu::scale_at(position));
                    self.scale_handle = position;
                }
                _ => {
                    menu.set_slider(id, crate::menu::scale_handle_at(self.scale));
                    if let Some(position) = menu.slider(id) {
                        self.scale_handle = position;
                    }
                }
            }
            menu.set_item_hint(id, self.scale_hint());

            // The cap's cycler. `placed` guards the first frame here as it
            // does the faders: the set is born on no ceiling and the file may
            // say otherwise, which is a placement rather than a step.
            let id = crate::menu::FRAME_CAP_ID;
            let rung = crate::menu::frame_cap_rung(self.cap);
            if let Some(chosen) = menu.cycler(id)
                && self.placed
                && chosen != rung
            {
                let forward = chosen == (rung + 1) % crate::menu::FRAME_CAPS.len();
                self.set_cap(crate::menu::frame_cap_stepped(self.cap, forward, chosen));
            }
            menu.set_cycler(id, crate::menu::frame_cap_rung(self.cap));
            menu.set_item_hint(id, self.cap_hint());

            // The anisotropy's, on the same terms.
            let id = crate::menu::ANISOTROPY_ID;
            let rung = crate::menu::anisotropy_rung(self.anisotropy);
            if let Some(chosen) = menu.cycler(id)
                && self.placed
                && chosen != rung
            {
                let forward = chosen == (rung + 1) % crate::menu::ANISOTROPIES.len();
                self.set_anisotropy(crate::menu::anisotropy_stepped(
                    self.anisotropy,
                    forward,
                    chosen,
                ));
            }
            menu.set_cycler(id, crate::menu::anisotropy_rung(self.anisotropy));
            menu.set_item_hint(id, self.anisotropy_hint());

            // The antialiasing ladder's, on the same terms — and simpler than
            // the two above, because every value the key reads is a rung: there
            // is no `stepped` to consult about where a value between rungs
            // lands, so the rung the widget chose *is* the answer.
            let id = crate::menu::ANTIALIASING_ID;
            let rung = crate::menu::antialiasing_rung(self.antialiasing);
            if let Some(chosen) = menu.cycler(id)
                && self.placed
                && chosen != rung
            {
                self.set_antialiasing(crate::menu::antialiasing_stepped(chosen));
            }
            menu.set_cycler(id, crate::menu::antialiasing_rung(self.antialiasing));
            menu.set_item_hint(id, self.antialiasing_hint());

            // The switches: two rungs each, so any move is a flip. Every key
            // is written once for however many flipped this frame.
            let mut effects = self.effects;
            for (index, (_, effect)) in crcbl::settings::VIDEO_KEYS.iter().enumerate() {
                let id = crate::menu::effect_id(index);
                if let Some(chosen) = menu.cycler(id)
                    && self.placed
                    && chosen != crate::menu::switch_rung(effects.contains(*effect))
                {
                    effects.toggle(*effect);
                }
            }
            if effects != self.effects {
                self.set_effects(effects);
            }
            for (index, (_, effect)) in crcbl::settings::VIDEO_KEYS.iter().enumerate() {
                let id = crate::menu::effect_id(index);
                menu.set_cycler(id, crate::menu::switch_rung(self.effects.contains(*effect)));
                menu.set_item_hint(id, self.effect_hint(*effect));
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

    /// The audio section, which is this sample's only live system, and the
    /// settings file as it stands.
    ///
    /// The engine's own sections carry the frame times and the GPU; what only
    /// this sample can report is whether its cues are reaching a mixer, which
    /// is the thing a fader is claiming to control — and what the file holds,
    /// every key of it, which is the thing `SAVE` is claiming to write. See
    /// [`crate::view`].
    fn debug_sections(&self, panel: &mut crcbl::ui::DebugPanel) {
        panel.add(&self.audio);
        panel.add(&crate::view::FileView(&self.stack));
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
            Screen::over(
                SettingsStack::from_storage(&storage),
                Store::None,
                FrameLimit::unlimited(),
            ),
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

    /// The choice a cycler row holds.
    fn rung(menus: &mut Menus, id: crcbl::ui::WidgetId) -> usize {
        menus
            .get_mut(MenuKind::Settings)
            .expect("the one menu")
            .cycler(id)
            .expect("the row is a cycler")
    }

    /// One arrow press on a cycler row, as the loop's menu pass delivers it:
    /// the row highlighted, then nudged. The screen reads it on the next
    /// [`reconcile`].
    fn step(menus: &mut Menus, id: crcbl::ui::WidgetId, forward: bool) -> bool {
        let menu = menus.get_mut(MenuKind::Settings).expect("the one menu");
        assert!(menu.select_id(id), "no row {id}");
        menu.nudge_cycler(forward)
    }

    /// `ENTER` on a cycler row: forward, and round the end.
    fn press(menus: &mut Menus, id: crcbl::ui::WidgetId) {
        let menu = menus.get_mut(MenuKind::Settings).expect("the one menu");
        assert!(menu.select_id(id), "no row {id}");
        assert_eq!(menu.activate(), None, "a cycler fired an id");
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
        let mut screen = Screen::over(
            SettingsStack::from_storage(&storage),
            Store::None,
            FrameLimit::unlimited(),
        );
        let mut menus = Screen::menus();
        reconcile(&mut screen, &mut menus);

        menus
            .get_mut(MenuKind::Settings)
            .expect("the one menu")
            .set_slider(crate::menu::fader_id(Bus::Ambience), 0.5);
        reconcile(&mut screen, &mut menus);
        screen.save_to(SettingsSource::Source(&storage));
        assert_eq!(screen.saved(), &SaveState::Saved);

        let reopened = Screen::over(
            SettingsStack::from_storage(&storage),
            Store::None,
            FrameLimit::unlimited(),
        );
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

    /// The buttons are the only ids that fire, and a fader never does.
    #[test]
    fn only_the_buttons_name_an_action() {
        assert_eq!(
            Screen::menu_action(crate::menu::SAVE_ID),
            Some(Action::Save)
        );
        assert_eq!(
            Screen::menu_action(crate::menu::RESET_ID),
            Some(Action::Reset)
        );
        assert_eq!(Screen::menu_action(crate::menu::FRAME_CAP_ID), None);
        assert_eq!(Screen::menu_action(crate::menu::ANISOTROPY_ID), None);
        assert_eq!(Screen::menu_action(crate::menu::ANTIALIASING_ID), None);
        for (index, (key, _)) in crcbl::settings::VIDEO_KEYS.iter().enumerate() {
            assert_eq!(
                Screen::menu_action(crate::menu::effect_id(index)),
                None,
                "{key}'s switch",
            );
        }
        for bus in Bus::ALL {
            assert_eq!(Screen::menu_action(crate::menu::fader_id(bus)), None);
        }
        assert_eq!(Screen::menu_action(crate::menu::RENDER_SCALE_ID), None);
    }

    /// **A switch round trips through the file, and flips only its own key.**
    ///
    /// An absent section allows everything; one press turns one effect off,
    /// writes every key so the file says what was chosen, and a screen opened
    /// over the same storage comes up with that effect off and the rest on.
    #[test]
    fn a_saved_switch_is_still_off_when_the_screen_opens_again() {
        let storage = crcbl::store::MemoryStorage::new();
        let mut screen = Screen::over(
            SettingsStack::from_storage(&storage),
            Store::None,
            FrameLimit::unlimited(),
        );
        let mut menus = Screen::menus();
        reconcile(&mut screen, &mut menus);
        assert_eq!(
            screen.effects(),
            RenderEffects::all(),
            "an absent section allows everything"
        );

        let shadows = crcbl::settings::VIDEO_KEYS
            .iter()
            .position(|(_, effect)| *effect == RenderEffects::SHADOWS)
            .expect("shadows has a key");
        // Back, from `on` to `off`: the arrow a player reaches for on a switch
        // that reads `< on`.
        assert!(step(&mut menus, crate::menu::effect_id(shadows), false));
        reconcile(&mut screen, &mut menus);
        assert_eq!(
            screen.effects(),
            RenderEffects::all().difference(RenderEffects::SHADOWS),
            "one press flipped more than its own switch",
        );
        assert_eq!(screen.edits(), 1);
        assert_eq!(screen.saved(), &SaveState::Unsaved);

        screen.save_to(SettingsSource::Source(&storage));
        assert_eq!(screen.saved(), &SaveState::Saved);

        let reopened = Screen::over(
            SettingsStack::from_storage(&storage),
            Store::None,
            FrameLimit::unlimited(),
        );
        assert_eq!(
            reopened.effects(),
            RenderEffects::all().difference(RenderEffects::SHADOWS),
            "the restart came up with a different set from the one saved",
        );
    }

    /// **A switch says when this run did not come up its way, and stops saying
    /// it when flipped back** — and a file that turned an effect off opens
    /// with that row reading `off` and nothing beside it.
    #[test]
    fn a_switch_this_run_is_not_under_says_so_and_stops_saying_it() {
        let (mut screen, mut menus) = screen("[engine.video]\nbloom = false\n");
        reconcile(&mut screen, &mut menus);
        let bloom = crcbl::settings::VIDEO_KEYS
            .iter()
            .position(|(_, effect)| *effect == RenderEffects::BLOOM)
            .expect("bloom has a key");
        let id = crate::menu::effect_id(bloom);
        assert_eq!(
            hint(&mut menus, id),
            crate::menu::switch_label(false),
            "a run opened with the effect off has nothing to add",
        );
        assert!(
            !hint(&mut menus, crate::menu::effect_id(0)).contains(crate::menu::NEXT_START_MARK),
            "an untouched switch wears the mark",
        );

        assert_eq!(rung(&mut menus, id), crate::menu::switch_rung(false));
        assert!(
            !step(&mut menus, id, false),
            "a switch already off stepped further off"
        );
        // `ENTER` goes round: off to on.
        press(&mut menus, id);
        reconcile(&mut screen, &mut menus);
        let marked = hint(&mut menus, id);
        assert!(
            marked.starts_with(crate::menu::switch_label(true))
                && marked.contains(crate::menu::NEXT_START_MARK),
            "the row reads {marked:?} for an effect this run did not come up with",
        );
        assert_eq!(rung(&mut menus, id), crate::menu::switch_rung(true));

        press(&mut menus, id);
        reconcile(&mut screen, &mut menus);
        assert_eq!(
            hint(&mut menus, id),
            crate::menu::switch_label(false),
            "the mark stayed on a switch back where the run came up",
        );
    }

    /// `RESET` allows every effect again, and writes the keys to say so.
    #[test]
    fn reset_allows_every_effect_again() {
        let (mut screen, mut menus) = screen("[engine.video]\nbloom = false\nshadows = false\n");
        assert_eq!(
            screen.effects(),
            RenderEffects::all().difference(RenderEffects::BLOOM | RenderEffects::SHADOWS),
        );
        reconcile(&mut screen, &mut menus);

        screen.apply(Action::Reset);
        assert_eq!(screen.effects(), RenderEffects::all());
        assert_eq!(
            crcbl::settings::video_effects(screen.stack()),
            RenderEffects::all(),
            "the screen reset a set it never wrote",
        );
    }

    /// **The frame cap round trips through the file, like a gain does.**
    ///
    /// Stepped on the screen, written to the key, and read back by a screen
    /// that opens over the same storage — the same claim
    /// `a_saved_gain_is_still_there_when_the_screen_opens_again` makes for
    /// `[engine.audio]`, for the first `[engine.video]` key to reach a screen.
    #[test]
    fn a_saved_frame_cap_is_still_there_when_the_screen_opens_again() {
        let storage = crcbl::store::MemoryStorage::new();
        let mut screen = Screen::over(
            SettingsStack::from_storage(&storage),
            Store::None,
            FrameLimit::unlimited(),
        );
        assert_eq!(
            screen.cap(),
            FrameLimit::unlimited(),
            "an absent key is no cap"
        );
        let mut menus = Screen::menus();
        reconcile(&mut screen, &mut menus);
        assert_eq!(screen.edits(), 0, "placing the row is not an edit");

        // Off the top rung, `ENTER` goes round to the bottom.
        press(&mut menus, crate::menu::FRAME_CAP_ID);
        reconcile(&mut screen, &mut menus);
        let stepped = screen.cap();
        assert_eq!(
            stepped,
            crate::menu::FRAME_CAPS[0],
            "the first step off unlimited is the lowest rung",
        );
        assert_eq!(screen.edits(), 1);
        assert_eq!(screen.saved(), &SaveState::Unsaved);

        screen.save_to(SettingsSource::Source(&storage));
        assert_eq!(screen.saved(), &SaveState::Saved);

        let reopened = Screen::over(
            SettingsStack::from_storage(&storage),
            Store::None,
            FrameLimit::unlimited(),
        );
        assert_eq!(
            reopened.cap(),
            stepped,
            "the restart came up on {:?} rather than the {stepped:?} that was saved",
            reopened.cap(),
        );
    }

    /// **The row says when the ceiling is not the one this run is under.**
    ///
    /// The loop takes its limit once, so a cap chosen on the screen is one the
    /// next start will use — and a screen that showed the new number with
    /// nothing beside it would be claiming an effect it has not had. Stepping
    /// all the way round the ladder brings the mark back off again, which is
    /// what tells a mark from a flag that is only ever set.
    #[test]
    fn a_cap_this_run_is_not_under_says_so_and_stops_saying_it() {
        let (mut screen, mut menus) = screen("");
        reconcile(&mut screen, &mut menus);
        let hint = |menus: &mut Menus| hint(menus, crate::menu::FRAME_CAP_ID);
        assert_eq!(
            hint(&mut menus),
            crate::menu::frame_cap_label(FrameLimit::unlimited()),
            "a run opened on its own ceiling has nothing to add",
        );

        press(&mut menus, crate::menu::FRAME_CAP_ID);
        reconcile(&mut screen, &mut menus);
        let marked = hint(&mut menus);
        assert!(
            marked.contains(crate::menu::NEXT_START_MARK),
            "the row reads {marked:?} for a ceiling this run is not under",
        );
        assert!(
            marked.contains(&crate::menu::frame_cap_label(screen.cap())),
            "the row reads {marked:?} rather than the ceiling that was chosen",
        );

        // All the way round: `ENTER` wraps, so a rung's worth of presses from
        // anywhere is back where it started — and one of them has already
        // been taken above.
        for _ in 1..crate::menu::FRAME_CAPS.len() {
            press(&mut menus, crate::menu::FRAME_CAP_ID);
            reconcile(&mut screen, &mut menus);
        }
        assert_eq!(
            screen.cap(),
            FrameLimit::unlimited(),
            "the ladder did not wrap"
        );
        assert_eq!(
            hint(&mut menus),
            crate::menu::frame_cap_label(FrameLimit::unlimited()),
            "the mark stayed on a ceiling the run is under",
        );
    }

    /// **The row shows the rate the game's own limit holds the ceiling to,
    /// and only where they differ.**
    ///
    /// A file saying 240 in a binary launched at 60 runs at 60, and a file
    /// saying nothing runs at 60 too — both rows say so. A file saying 30 is
    /// the ceiling that wins, and the row has nothing to add. Stepping the
    /// ceiling recomputes the held-to rate against the new ceiling and marks
    /// the row for the next start.
    #[test]
    fn the_row_shows_the_rate_the_games_own_limit_holds_the_ceiling_to() {
        let asked = FrameLimit::fps(60);
        let hint_of = |toml: &str| {
            let storage = settings_file(toml);
            let mut screen =
                Screen::over(SettingsStack::from_storage(&storage), Store::None, asked);
            let mut menus = Screen::menus();
            reconcile(&mut screen, &mut menus);
            (screen, menus)
        };
        let held = |cap: FrameLimit| {
            format!(
                "{}, {} {}",
                crate::menu::frame_cap_label(cap),
                crate::menu::HELD_MARK,
                crate::menu::frame_cap_label(asked),
            )
        };

        let (_, mut menus) = hint_of("[engine.video]\nframe_limit = 240\n");
        assert_eq!(
            hint(&mut menus, crate::menu::FRAME_CAP_ID),
            held(FrameLimit::fps(240)),
        );

        let (_, mut menus) = hint_of("");
        assert_eq!(
            hint(&mut menus, crate::menu::FRAME_CAP_ID),
            held(FrameLimit::unlimited()),
            "no ceiling still runs at the game's own limit",
        );

        let (mut screen, mut menus) = hint_of("[engine.video]\nframe_limit = 30\n");
        assert_eq!(
            hint(&mut menus, crate::menu::FRAME_CAP_ID),
            crate::menu::frame_cap_label(FrameLimit::fps(30)),
            "a ceiling under the game's limit is the rate, and says nothing more",
        );

        // Two steps up from 30 is 72, above the game's 60: held, and next start.
        for _ in 0..2 {
            assert!(step(&mut menus, crate::menu::FRAME_CAP_ID, true));
            reconcile(&mut screen, &mut menus);
        }
        assert_eq!(screen.cap(), FrameLimit::fps(72));
        assert_eq!(
            hint(&mut menus, crate::menu::FRAME_CAP_ID),
            format!(
                "{} {}",
                held(FrameLimit::fps(72)),
                crate::menu::NEXT_START_MARK
            ),
        );
    }

    /// `RESET` is the defaults of the whole screen, not only of the faders.
    #[test]
    fn reset_takes_the_frame_cap_back_to_no_ceiling_too() {
        let (mut screen, mut menus) = screen("[engine.video]\nframe_limit = 60\n");
        assert_eq!(screen.cap(), FrameLimit::fps(60));
        reconcile(&mut screen, &mut menus);

        screen.apply(Action::Reset);
        assert_eq!(screen.cap(), FrameLimit::unlimited());
        assert_eq!(
            crcbl::settings::frame_limit(screen.stack()),
            FrameLimit::unlimited(),
            "the screen reset a number it never wrote",
        );
    }

    /// **The anisotropy round trips through the file, like the cap does.**
    ///
    /// Stepped on the screen, written to the key, and read back by a screen
    /// that opens over the same storage. An absent key is the engine's default,
    /// and the first step off it is the rung above — not the bottom of the
    /// ladder, which is where a step from the *cap's* absent value lands, since
    /// no ceiling is that ladder's top and eight is this one's middle.
    #[test]
    fn a_saved_anisotropy_is_still_there_when_the_screen_opens_again() {
        let storage = crcbl::store::MemoryStorage::new();
        let mut screen = Screen::over(
            SettingsStack::from_storage(&storage),
            Store::None,
            FrameLimit::unlimited(),
        );
        assert_eq!(
            screen.anisotropy(),
            DEFAULT_ANISOTROPY,
            "an absent key is the engine's default"
        );
        let mut menus = Screen::menus();
        reconcile(&mut screen, &mut menus);
        assert_eq!(screen.edits(), 0, "placing the row is not an edit");

        assert!(step(&mut menus, crate::menu::ANISOTROPY_ID, true));
        reconcile(&mut screen, &mut menus);
        let stepped = screen.anisotropy();
        assert_eq!(
            stepped,
            crate::menu::ANISOTROPIES[crate::menu::anisotropy_rung(DEFAULT_ANISOTROPY) + 1],
            "the first step off the default is the rung above it",
        );
        assert_ne!(stepped, DEFAULT_ANISOTROPY);
        assert_eq!(screen.edits(), 1);
        assert_eq!(screen.saved(), &SaveState::Unsaved);

        screen.save_to(SettingsSource::Source(&storage));
        assert_eq!(screen.saved(), &SaveState::Saved);

        let reopened = Screen::over(
            SettingsStack::from_storage(&storage),
            Store::None,
            FrameLimit::unlimited(),
        );
        assert_eq!(
            reopened.anisotropy(),
            stepped,
            "the restart came up on {} rather than the {stepped} that was saved",
            reopened.anisotropy(),
        );
    }

    /// **The row says when the anisotropy is not the one this run came up on,
    /// and a file value between rungs steps to the rung on either side.**
    ///
    /// The same mark the cap wears, for a related reason: nothing in this
    /// process samples a page, so a value chosen here reaches a sampler only
    /// where the next renderer opens over the key. A hand-written `6` sits on
    /// `4`'s rung and reads `6x`; one step up is `8`, and — from `6` again —
    /// one step back is `4`, not the `2` the widget landed on, which is what
    /// putting the widget back on the value's rung after every step is for.
    #[test]
    fn an_anisotropy_this_run_is_not_on_says_so_and_a_value_between_rungs_steps_either_way() {
        let (mut screen, mut menus) = screen("[engine.video]\nanisotropic_filtering = 6\n");
        assert_eq!(
            screen.anisotropy(),
            6.0,
            "the reader accepts what is in range"
        );
        reconcile(&mut screen, &mut menus);
        let hint = |menus: &mut Menus| hint(menus, crate::menu::ANISOTROPY_ID);
        assert_eq!(
            hint(&mut menus),
            crate::menu::anisotropy_label(6.0),
            "a run opened on its own value has nothing to add",
        );
        assert_eq!(
            rung(&mut menus, crate::menu::ANISOTROPY_ID),
            crate::menu::anisotropy_rung(6.0),
            "the widget was not placed on the file's rung",
        );

        assert!(step(&mut menus, crate::menu::ANISOTROPY_ID, true));
        reconcile(&mut screen, &mut menus);
        assert_eq!(screen.anisotropy(), 8.0, "six steps up to eight");
        let marked = hint(&mut menus);
        assert!(
            marked.contains(crate::menu::NEXT_START_MARK),
            "the row reads {marked:?} for an anisotropy this run is not on",
        );
        assert!(
            marked.contains(&crate::menu::anisotropy_label(8.0)),
            "the row reads {marked:?} rather than the value that was chosen",
        );

        // Round the whole ladder from eight lands on eight again — but the run
        // came up on six, which is not a rung, so the mark stays until the
        // screen is reset back to the file's own value.
        for _ in 0..crate::menu::ANISOTROPIES.len() {
            press(&mut menus, crate::menu::ANISOTROPY_ID);
            reconcile(&mut screen, &mut menus);
        }
        assert_eq!(screen.anisotropy(), 8.0, "the ladder did not wrap");
        assert!(hint(&mut menus).contains(crate::menu::NEXT_START_MARK));

        // And the other way from six: back is four.
        let (mut screen, mut menus) = self::screen("[engine.video]\nanisotropic_filtering = 6\n");
        reconcile(&mut screen, &mut menus);
        assert!(step(&mut menus, crate::menu::ANISOTROPY_ID, false));
        reconcile(&mut screen, &mut menus);
        assert_eq!(screen.anisotropy(), 4.0, "six steps back to four");
        assert_eq!(
            rung(&mut menus, crate::menu::ANISOTROPY_ID),
            crate::menu::anisotropy_rung(4.0),
            "the widget was left where it landed rather than on the value's rung",
        );
        assert_eq!(screen.edits(), 1);
    }

    /// **A hand-written rate steps to the rung on either side of it**, and the
    /// arrows stop at the ladder's ends where `ENTER` goes round.
    #[test]
    fn a_rate_between_rungs_steps_either_way_and_the_arrows_stop_at_the_ends() {
        let id = crate::menu::FRAME_CAP_ID;
        let (mut screen, mut menus) = screen("[engine.video]\nframe_limit = 90\n");
        reconcile(&mut screen, &mut menus);
        assert_eq!(
            hint(&mut menus, id),
            crate::menu::frame_cap_label(FrameLimit::fps(90)),
            "the row reads the file's rate, not a rung",
        );
        assert!(step(&mut menus, id, true));
        reconcile(&mut screen, &mut menus);
        assert_eq!(screen.cap(), FrameLimit::fps(120), "90 steps up to 120");

        let (mut screen, mut menus) = self::screen("[engine.video]\nframe_limit = 90\n");
        reconcile(&mut screen, &mut menus);
        assert!(step(&mut menus, id, false));
        reconcile(&mut screen, &mut menus);
        assert_eq!(screen.cap(), FrameLimit::fps(72), "90 steps back to 72");
        assert_eq!(
            crcbl::settings::frame_limit(screen.stack()),
            FrameLimit::fps(72),
            "the step did not reach the key",
        );

        // No ceiling is the top rung: Right stays, ENTER goes round.
        let (mut screen, mut menus) = self::screen("");
        reconcile(&mut screen, &mut menus);
        assert!(
            !step(&mut menus, id, true),
            "the top rung stepped further up"
        );
        reconcile(&mut screen, &mut menus);
        assert_eq!(screen.cap(), FrameLimit::unlimited());
        assert_eq!(screen.edits(), 0, "a refused step was written as an edit");
        press(&mut menus, id);
        reconcile(&mut screen, &mut menus);
        assert_eq!(screen.cap(), crate::menu::FRAME_CAPS[0]);
        assert!(
            !step(&mut menus, id, false),
            "the bottom rung stepped further down"
        );
    }

    /// Round the ladder from a value that is a rung brings the mark back off,
    /// which is what tells a mark from a flag that is only ever set.
    #[test]
    fn stepping_round_the_anisotropy_ladder_brings_the_mark_off_again() {
        let (mut screen, mut menus) = screen("");
        reconcile(&mut screen, &mut menus);
        press(&mut menus, crate::menu::ANISOTROPY_ID);
        reconcile(&mut screen, &mut menus);
        assert!(
            hint(&mut menus, crate::menu::ANISOTROPY_ID).contains(crate::menu::NEXT_START_MARK)
        );

        for _ in 1..crate::menu::ANISOTROPIES.len() {
            press(&mut menus, crate::menu::ANISOTROPY_ID);
            reconcile(&mut screen, &mut menus);
        }
        assert_eq!(screen.anisotropy(), DEFAULT_ANISOTROPY);
        assert_eq!(
            hint(&mut menus, crate::menu::ANISOTROPY_ID),
            crate::menu::anisotropy_label(DEFAULT_ANISOTROPY),
            "the mark stayed on a value the run came up on",
        );
    }

    /// **The scale groove is placed from the file before it is read as a drag,
    /// a drag writes the key, and the row says the run is not under it.**
    ///
    /// The faders' first-frame rule, for the one groove that is not a fader: a
    /// screen opened over `render_scale = 0.5` finds the handle at the right
    /// end and has to walk it down without calling that an edit. Then a drag
    /// to the left end is the smallest frame the renderer draws, written to
    /// the key and marked, since nothing in this process draws at it.
    #[test]
    fn the_scale_groove_is_placed_from_the_file_and_a_drag_writes_the_key() {
        let (mut screen, mut menus) = screen("[engine.video]\nrender_scale = 0.5\n");
        assert_eq!(screen.render_scale(), 0.5);
        reconcile(&mut screen, &mut menus);
        let groove = |menus: &mut Menus| {
            menus
                .get_mut(MenuKind::Settings)
                .expect("the one menu")
                .slider(crate::menu::RENDER_SCALE_ID)
                .expect("the scale groove")
        };
        let expected = crate::menu::scale_handle_at(0.5);
        assert!(
            (groove(&mut menus) - expected).abs() < 1e-6,
            "the handle was not walked down to the file's half",
        );
        assert_eq!(screen.edits(), 0, "placing the handle is not an edit");
        assert_eq!(
            hint(&mut menus, crate::menu::RENDER_SCALE_ID),
            crate::menu::percent(0.5),
            "a run opened on its own scale has nothing to add",
        );

        menus
            .get_mut(MenuKind::Settings)
            .expect("the one menu")
            .set_slider(crate::menu::RENDER_SCALE_ID, 0.0);
        reconcile(&mut screen, &mut menus);
        assert_eq!(
            screen.render_scale(),
            crcbl::render::MIN_RENDER_SCALE,
            "the left end of the groove is the renderer's floor",
        );
        assert_eq!(
            crcbl::settings::render_scale(screen.stack()),
            crcbl::render::MIN_RENDER_SCALE,
            "the drag did not reach the key",
        );
        assert_eq!(screen.edits(), 1);
        assert_eq!(screen.saved(), &SaveState::Unsaved);
        let marked = hint(&mut menus, crate::menu::RENDER_SCALE_ID);
        assert!(
            marked.contains(crate::menu::NEXT_START_MARK)
                && marked.contains(&crate::menu::percent(crcbl::render::MIN_RENDER_SCALE)),
            "the row reads {marked:?}",
        );

        // Held still, the next frame reads no drag.
        reconcile(&mut screen, &mut menus);
        assert_eq!(screen.edits(), 1, "a handle at rest was read as a drag");
    }

    /// `RESET` takes the scale back to the whole extent, and the groove
    /// follows the number rather than the other way round.
    #[test]
    fn reset_takes_the_scale_back_to_the_whole_extent_and_the_groove_follows() {
        let (mut screen, mut menus) = screen("[engine.video]\nrender_scale = 0.5\n");
        reconcile(&mut screen, &mut menus);

        screen.apply(Action::Reset);
        assert_eq!(screen.render_scale(), 1.0);
        assert_eq!(
            crcbl::settings::render_scale(screen.stack()),
            1.0,
            "the screen reset a number it never wrote",
        );
        reconcile(&mut screen, &mut menus);
        let position = menus
            .get_mut(MenuKind::Settings)
            .expect("the one menu")
            .slider(crate::menu::RENDER_SCALE_ID)
            .expect("the scale groove");
        assert_eq!(position, 1.0, "the groove did not follow the reset");
        let edits = screen.edits();
        reconcile(&mut screen, &mut menus);
        assert_eq!(
            edits,
            screen.edits(),
            "following a reset was read as a drag"
        );
    }

    /// **The panel shows the file as it stands, keys no row owns included.**
    ///
    /// A hand-edited `[engine.window]` has no row on this screen and would
    /// otherwise be invisible to a player wondering what their file says; and
    /// the view is live — a fader moved after the panel was read shows the new
    /// gain, since the section is gathered again every frame it is visible.
    #[test]
    fn the_panel_shows_every_key_in_the_file_as_it_stands() {
        let (mut screen, mut menus) =
            screen("[engine.audio]\nmusic_volume = 0.25\n\n[engine.window]\nwidth = 640\n");
        reconcile(&mut screen, &mut menus);
        let file_rows = |screen: &Screen| {
            let mut panel = crcbl::ui::DebugPanel::new();
            panel.set_visible(true);
            panel.begin_frame();
            screen.debug_sections(&mut panel);
            let section = panel
                .sections()
                .iter()
                .find(|section| section.title() == crate::view::TITLE)
                .expect("the file has a section");
            section
                .rows()
                .iter()
                .map(|row| (row.label.clone(), row.value.clone()))
                .collect::<Vec<_>>()
        };
        let rows = file_rows(&screen);
        assert!(
            rows.contains(&("[engine.window]".to_string(), String::new()))
                && rows.contains(&("width".to_string(), "640".to_string())),
            "a key no row owns is missing from {rows:?}",
        );
        assert!(
            rows.contains(&("music_volume".to_string(), "0.25".to_string())),
            "the file's gain is missing from {rows:?}",
        );

        screen.set(Bus::Music, 0.5);
        let rows = file_rows(&screen);
        assert!(
            rows.contains(&("music_volume".to_string(), "0.5".to_string())),
            "the view is not live: {rows:?}",
        );

        let (screen, _) = self::screen("");
        assert_eq!(
            file_rows(&screen),
            [(
                crate::view::EMPTY_ROW.0.to_string(),
                crate::view::EMPTY_ROW.1.to_string()
            )],
            "an empty file says so",
        );
    }

    /// **The antialiasing tier round trips through the file, and the row is
    /// born on the game's rung rather than on the bottom of the ladder.**
    ///
    /// The cap's and the anisotropy's claim for the key that replaced the two
    /// effect switches. An absent key is the tier
    /// `RenderEffects::DEFAULT_STACK` carries, because that is the slot the
    /// view's own stack keeps; a step writes the rung the widget chose; and a
    /// screen opened over the same storage comes up on it.
    #[test]
    fn a_saved_antialiasing_tier_is_still_there_when_the_screen_opens_again() {
        let storage = crcbl::store::MemoryStorage::new();
        let mut screen = Screen::over(
            SettingsStack::from_storage(&storage),
            Store::None,
            FrameLimit::unlimited(),
        );
        assert_eq!(
            screen.antialiasing(),
            crate::menu::DEFAULT_ANTIALIASING,
            "an absent key is the tier the game's own stack carries",
        );
        let mut menus = Screen::menus();
        reconcile(&mut screen, &mut menus);
        assert_eq!(screen.edits(), 0, "placing the row is not an edit");
        assert_eq!(
            rung(&mut menus, crate::menu::ANTIALIASING_ID),
            crate::menu::antialiasing_rung(crate::menu::DEFAULT_ANTIALIASING),
        );

        assert!(step(&mut menus, crate::menu::ANTIALIASING_ID, true));
        reconcile(&mut screen, &mut menus);
        let stepped = screen.antialiasing();
        assert_eq!(
            stepped,
            Antialiasing::Smaa,
            "the first step up from the game's tier is the rung above it",
        );
        assert_eq!(screen.edits(), 1);
        assert_eq!(screen.saved(), &SaveState::Unsaved);
        assert_eq!(
            crcbl::settings::antialiasing(screen.stack()),
            Some(stepped),
            "the step did not reach the key",
        );

        screen.save_to(SettingsSource::Source(&storage));
        assert_eq!(screen.saved(), &SaveState::Saved);

        let reopened = Screen::over(
            SettingsStack::from_storage(&storage),
            Store::None,
            FrameLimit::unlimited(),
        );
        assert_eq!(
            reopened.antialiasing(),
            stepped,
            "the restart came up on {:?} rather than the {stepped:?} that was saved",
            reopened.antialiasing(),
        );
    }

    /// **The file's tier reaches the row before a frame is drawn, the row says
    /// when this run is not under it, and it stops saying so.**
    ///
    /// The set is born on the game's rung, so a screen opened over
    /// `antialiasing = "none"` finds the widget one rung above its value —
    /// which is the first-frame placement, not a step, and calling it a step
    /// would write over the player's choice on frame one. Then the ladder is
    /// walked all the way round, which is what tells a mark from a flag that is
    /// only ever set.
    #[test]
    fn the_antialiasing_row_is_placed_from_the_file_and_marks_a_tier_this_run_is_not_on() {
        let (mut screen, mut menus) = screen("[engine.video]\nantialiasing = \"none\"\n");
        assert_eq!(screen.antialiasing(), Antialiasing::None);
        assert_eq!(
            rung(&mut menus, crate::menu::ANTIALIASING_ID),
            crate::menu::antialiasing_rung(crate::menu::DEFAULT_ANTIALIASING),
            "the set is born on the game's rung",
        );

        reconcile(&mut screen, &mut menus);
        assert_eq!(screen.edits(), 0, "placing the row was read as a step");
        assert_eq!(
            rung(&mut menus, crate::menu::ANTIALIASING_ID),
            crate::menu::antialiasing_rung(Antialiasing::None),
            "the widget was not walked to the file's rung",
        );
        assert_eq!(
            hint(&mut menus, crate::menu::ANTIALIASING_ID),
            crate::menu::antialiasing_label(Antialiasing::None),
            "a run opened on its own tier has nothing to add",
        );

        // Round the whole ladder with `ENTER`, which wraps: the mark comes on
        // at the first press and off again at the last.
        press(&mut menus, crate::menu::ANTIALIASING_ID);
        reconcile(&mut screen, &mut menus);
        let marked = hint(&mut menus, crate::menu::ANTIALIASING_ID);
        assert!(
            marked.starts_with(crate::menu::antialiasing_label(screen.antialiasing()))
                && marked.contains(crate::menu::NEXT_START_MARK),
            "the row reads {marked:?} for a tier this run is not under",
        );

        for _ in 1..Antialiasing::ALL.len() {
            press(&mut menus, crate::menu::ANTIALIASING_ID);
            reconcile(&mut screen, &mut menus);
        }
        assert_eq!(
            screen.antialiasing(),
            Antialiasing::None,
            "the ladder did not wrap"
        );
        assert_eq!(
            hint(&mut menus, crate::menu::ANTIALIASING_ID),
            crate::menu::antialiasing_label(Antialiasing::None),
            "the mark stayed on a tier the run came up on",
        );
    }

    /// `RESET` puts the tier back on the game's own rung — what an absent key
    /// means — and writes the key to say so.
    #[test]
    fn reset_takes_the_antialiasing_tier_back_to_the_games_own_rung() {
        let (mut screen, mut menus) = screen("[engine.video]\nantialiasing = \"smaa\"\n");
        assert_eq!(screen.antialiasing(), Antialiasing::Smaa);
        reconcile(&mut screen, &mut menus);

        screen.apply(Action::Reset);
        assert_eq!(screen.antialiasing(), crate::menu::DEFAULT_ANTIALIASING);
        assert_eq!(
            crcbl::settings::antialiasing(screen.stack()),
            Some(crate::menu::DEFAULT_ANTIALIASING),
            "the screen reset a tier it never wrote",
        );
    }

    /// `RESET` takes the anisotropy back to the engine's default, which is what
    /// an absent key means — not to the bottom of the ladder.
    #[test]
    fn reset_takes_the_anisotropy_back_to_the_engines_default() {
        let (mut screen, mut menus) = screen("[engine.video]\nanisotropic_filtering = 16\n");
        assert_eq!(screen.anisotropy(), 16.0);
        reconcile(&mut screen, &mut menus);

        screen.apply(Action::Reset);
        assert_eq!(screen.anisotropy(), DEFAULT_ANISOTROPY);
        assert_eq!(
            crcbl::settings::anisotropic_filtering(screen.stack()),
            DEFAULT_ANISOTROPY,
            "the screen reset a number it never wrote",
        );
    }
}
