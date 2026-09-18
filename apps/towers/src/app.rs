//! Towers' start-up, its controls, and the [`HostedGame`] methods the engine's
//! loop calls.
//!
//! # There is no loop in this file
//!
//! ```text
//! Loop::frame()                     ← the engine's
//!   pump, input, menu, pause, resize
//!     ─────────────────────────────→ Towers::key_event   (queued, not applied)
//!   run_ticks  ─────────────────────→ Towers::tick       (commands, then a tick)
//!   draw_list.clear()
//!     ─────────────────────────────→ Towers::draw        (field, readout)
//!     menu, debug overlay             ← the engine's
//!   gpu.frame()
//! ```
//!
//! What is left here is start-up, because a window's title is this sample's;
//! the action map, because a keyboard is not something [`crate::game`] should
//! know about; the build cursor, because which plot is highlighted is
//! presentation; and the trait methods, because they are what a hosted game is.
//!
//! # The cursor and the kind are the client's, and the build is the server's
//!
//! `LEFT` and `RIGHT` move [`Towers::selected`], `1`/`2`/`3` move
//! [`Towers::kind`], and neither crosses the wire on its own. `B` seals **that
//! plot number and that kind** into a command and `U` seals the plot to step up,
//! and `crate::game::Stage::place_tower` and `Stage::upgrade_tower` are what
//! decide whether anything happens. So the two highlighted rows are a local
//! convenience and both commands are authoritative, which is the split
//! `docs/plan/sample/07-towers.md` needs for co-op: four players each have their
//! own cursor and one server has the purse.
//!
//! # This sample is played with the keyboard
//!
//! No [`pointer_event`](HostedGame::pointer_event) and no
//! [`touch_event`](HostedGame::touch_event) override: the two lists are picked
//! through with keys rather than clicked on. **The browser demo does not change
//! that in the window**, which is a decision rather than an omission: what a tap
//! wants to land on is the build menu
//! `docs/plan/sample/07-towers.md`'s slice 3b brings with the `.crpix` art, and a
//! hit test written against the untextured lists [`crate::page`] draws today
//! would be thrown away with it.
//!
//! **What a phone gets instead is a row of buttons on the page**, outside the
//! canvas: `web/demos/towers/main.js` puts the plot cursor, the three kinds,
//! `B` and `U` under the field and **synthesises the key each stands for**. That
//! is the same input path a keyboard takes — the shell's own `keydown` listener
//! — rather than a second one, so there is nothing for the two to disagree
//! about, and it is thrown away with the hint text rather than built into the
//! engine. The pointer and the finger **inside the canvas** are still owed, and
//! that document records them.
//!
//! # `[HUD]` is logged here rather than in `crate::game`
//!
//! Every other line on it is the simulation's, and `apps/puppet` logs its
//! heartbeat from the tick for that reason. This one also names the three
//! selectors the frame is drawn through — rule 12 — and those are
//! [`crate::Paths`]', which the stage cannot see, plus the plot and the kind the
//! client has picked, which the stage cannot see either. Logging it here is what
//! puts all of them on one line at one cadence; `apps/breach` and `apps/quarry`
//! do the same, and for the same reason.

use crcbl::core::input::KeyCode;
use crcbl::engine::{Booted, Clock, FrameInfo, HostedGame, RunSummary, wait_for_configure};
use crcbl::input::{ActionDecl, ActionKind, ActionMap, Binding};
use crcbl::prelude::*;
use crcbl::shell::DisplayMode;

use crate::game::{Controls, Game, RenderState, Stats};
use crate::gpu::{Gpu, Paths};
use crate::map::PLOTS;
use crate::menu::{MenuAction, MenuKind, Menus};
use crate::page::PageStats;
use crate::tower;
use crate::wave::Outcome;

pub use crate::args::Options;

// ---- the controls --------------------------------------------------------------

/// Move the build cursor one plot down the list, and one up.
const ACTION_PREV: &str = "prev-plot";
/// See [`ACTION_PREV`].
const ACTION_NEXT: &str = "next-plot";
/// Build on the highlighted plot. Read as a press **edge**: one press is one
/// command, and a held key must not spend the purse sixty times a second.
const ACTION_BUILD: &str = "build";
/// Step the tower on the highlighted plot up a tier. An edge, for
/// [`ACTION_BUILD`]'s reason — and the more pressing one, because an upgrade is
/// the dearest thing a key can spend.
const ACTION_UPGRADE: &str = "upgrade";
/// Send the next wave now. An edge, for [`ACTION_BUILD`]'s reason.
const ACTION_WAVE: &str = "send-wave";
/// Throw the run away. An edge.
const ACTION_RESTART: &str = "restart";

/// Pick which kind the next build is of: one action per [`crate::tower::Kind`],
/// in [`crate::tower::ALL`]'s order.
///
/// Named after the kinds rather than after the keys, so a rebind typed at the
/// console reads as the game does — `the_kind_actions_are_named_after_the_kinds`
/// holds the two lists together, which is what stops a reordered table silently
/// binding `1` to the splash tower.
const ACTION_KINDS: [&str; tower::KINDS] = ["kind-bolt", "kind-splash", "kind-slow"];

/// The keys those three actions are bound to, in the same order.
const KIND_KEYS: [KeyCode; tower::KINDS] = [KeyCode::Digit1, KeyCode::Digit2, KeyCode::Digit3];

/// The keyboard this sample is played with.
///
/// Declared in one place so the bindings and the read-out below cannot name
/// different actions: a typo in either is an action that resolves to nothing,
/// and [`ActionMap`] answers `false` for an action nobody declared rather than
/// complaining.
fn action_map() -> ActionMap {
    let mut map = ActionMap::new();
    let kinds = ACTION_KINDS
        .iter()
        .zip(KIND_KEYS)
        .map(|(name, key)| (*name, vec![Binding::Key(key)]));
    for (name, bindings) in [
        (ACTION_PREV, vec![Binding::Key(KeyCode::ArrowLeft)]),
        (ACTION_NEXT, vec![Binding::Key(KeyCode::ArrowRight)]),
        (ACTION_BUILD, vec![Binding::Key(KeyCode::KeyB)]),
        (ACTION_UPGRADE, vec![Binding::Key(KeyCode::KeyU)]),
        (ACTION_WAVE, vec![Binding::Key(KeyCode::KeyN)]),
        (ACTION_RESTART, vec![Binding::Key(KeyCode::KeyR)]),
    ]
    .into_iter()
    .chain(kinds)
    {
        map.declare(ActionDecl {
            name: name.into(),
            kind: ActionKind::Button,
            bindings,
        });
    }
    map
}

// ---- summary -----------------------------------------------------------------

/// What a finished run reports.
///
/// Every field is an integer or an enum, so this is [`Eq`] where the 3D
/// samples' summaries are only [`PartialEq`]: a tower defense's state is
/// counters, and two runs either agree exactly or do not.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Summary {
    /// The half of the report every sample shares.
    pub run: RunSummary,
    /// What the team finished with.
    pub gold: u32,
    pub lives: u32,
    /// How many waves had been started.
    pub wave: usize,
    pub kills: u64,
    pub leaks: u64,
    /// How many towers were built, how many of each kind, how many were stepped
    /// up a tier, and how many commands the server turned down. The last is the
    /// one that says validation happened.
    pub built: u64,
    pub built_by_kind: [u64; tower::KINDS],
    pub upgrades: u64,
    pub refused: u64,
    pub outcome: Outcome,
    /// How many runs the demo played, restarts included.
    pub runs: u64,
    /// Which selectors the frames were drawn through — rule 12's "says which it
    /// took", in the summary line as well as in the panel.
    pub paths: Paths,
    /// How many commands the last overlay drew. Zero would mean a run that
    /// presented frames with nothing on them, which is the one failure a
    /// headless smoke test could otherwise report as a pass.
    pub commands: usize,
}

// ---- errors ------------------------------------------------------------------

/// What can stop towers: the loop's own failures, plus this sample's.
pub type TowersError = crcbl::engine::LoopError<crate::game::GameError>;

// ---- the hosted game ---------------------------------------------------------

/// Towers, as the engine's loop hosts it.
#[derive(Debug)]
pub struct Towers {
    game: Game,
    /// The keyboard, resolved into [`Controls`] once per tick.
    actions: ActionMap,
    /// Key events from the shell pump, replayed after `ActionMap::begin_tick`.
    ///
    /// The pump runs once per **frame** and the map's edge flags are per
    /// **tick**, and `begin_tick` clears those flags — so an event fed before
    /// it has its press edge erased. Every control in this sample is an edge,
    /// so a key fed at the wrong moment is a command that never happened.
    /// Queueing here and replaying after is the order the map asks for, and it
    /// is what makes a frame that runs no ticks lossless.
    pending_keys: Vec<(KeyCode, bool)>,
    /// A `RESTART` pressed on the pause menu, waiting for the next tick. The
    /// menu cannot reach the stage — see [`crate::menu`].
    pending_restart: bool,
    /// Which plot the build cursor is on. **Presentation**: it never crosses
    /// the wire, and what does is the plot number a build command names.
    selected: u8,
    /// Which kind the next build is of. Presentation in the same sense, with one
    /// difference: it travels on **every** command frame rather than only on the
    /// tick a build is asked for — see [`Controls::kind`].
    kind: tower::Kind,
    /// Refilled from the simulation every frame.
    render_state: RenderState,
    /// The simulation's numbers, snapshotted in [`Towers::tick`].
    stats: Stats,
    /// What the last frame's overlay drew, from the same frame.
    page: PageStats,
    /// Which selectors this device drew through, read off the GPU bundle.
    ///
    /// Kept here rather than reached through `gpu` because
    /// [`HostedGame::debug_sections`] and [`HostedGame::summary`] are handed
    /// `&self` and no GPU at all.
    paths: Paths,
}

impl Towers {
    /// The `[HUD]` line, on the cadence every other sample uses.
    ///
    /// Five of its fields are the game and nothing on the client can move them
    /// — `wave` and `creeps` advance on the table's own clock, `kills` and
    /// `leaks` are what the towers and the exit volume did, and `refused` is
    /// the server turning a command down. `gold`, `lives` and `towers` are what
    /// a player changes. Together they are enough for a page gate to tell a
    /// demo that is playing itself from one whose loop has stopped, which is
    /// what `web/tools/browser-e2e.mjs`'s `towers` row reads them for.
    ///
    /// **`next` is on the line for that gate specifically.** It is the seconds
    /// until the table sends the next wave by itself, and `--` while one is
    /// releasing or the table is spent — which is the only thing on the line
    /// that says whether [`crate::wave::Waves::start_now`] would be accepted
    /// right now. A gate pressing the wave key without it cannot tell a wave it
    /// brought forward from one that was arriving anyway, and cannot tell a
    /// refusal from bad timing of its own.
    ///
    /// **And the slice 3a content is on it for the same reason.** `kind` is which
    /// kind the client has picked, the one field beside `plot` that never crosses
    /// the wire — so a gate pressing `2` can see the key arrive before it asks
    /// what a build did with it. `bolt`, `splash` and `slow` are how many of each
    /// kind the *server* has built, which is what tells a splash tower that went
    /// up from a bolt tower that went up instead; `upgrades` is the same claim for
    /// `UpgradeTower`. Note that `bolt` and `bolts` are different readings — the
    /// first is towers built, the second bolts in the air — and a gate matching
    /// one has to match the colon.
    ///
    /// It also names the three selectors — see the module docs.
    fn log_heartbeat(&self) {
        if self.stats.ticks == 0
            || !crcbl::engine::heartbeat_due(self.stats.ticks, crate::game::HEARTBEAT_TICKS)
        {
            return;
        }
        let stats = &self.stats;
        let next = match stats.next_wave_in {
            Some(seconds) => format!("{seconds:.2}"),
            None => "--".to_string(),
        };
        crcbl::log::info!(
            "[HUD] tick: {}  gold: {}  lives: {}  wave: {}  next: {}  creeps: {}  towers: {}  \
             bolt: {}  splash: {}  slow: {}  upgrades: {}  bolts: {}  kills: {}  leaks: {}  \
             shots: {}  built: {}  refused: {}  outcome: {}  runs: {}  plot: {}  kind: {}  \
             geometry: {:?}  binding: {:?}  lighting: {:?}",
            stats.ticks,
            stats.gold,
            stats.lives,
            stats.wave,
            next,
            stats.creeps,
            stats.towers,
            stats.built_of(tower::Kind::Bolt),
            stats.built_of(tower::Kind::Splash),
            stats.built_of(tower::Kind::Slow),
            stats.upgrades,
            stats.bolts,
            stats.kills,
            stats.leaks,
            stats.shots,
            stats.built,
            stats.refused,
            stats.outcome.label(),
            stats.runs,
            PLOTS[usize::from(self.selected)].label,
            self.kind.label(),
            self.paths.geometry,
            self.paths.binding,
            self.paths.lighting,
        );
    }

    /// The simulation, for scripted tests and for an embedder that drives it.
    pub const fn game(&self) -> &Game {
        &self.game
    }

    /// Which plot the build cursor is on, for this crate's own tests.
    pub const fn selected(&self) -> u8 {
        self.selected
    }

    /// Which kind the next build is of, for this crate's own tests.
    pub const fn kind(&self) -> tower::Kind {
        self.kind
    }

    /// What the last frame's overlay drew.
    pub const fn page(&self) -> &PageStats {
        &self.page
    }
}

/// The loop towers runs in.
///
/// A type alias, because the loop is the engine's. `S` is the shell type: the
/// native path builds `Loop<dyn Shell>`, and the tests build
/// `Loop<HeadlessShell>` so they can inject the events a compositor would send.
pub type Loop<S = dyn Shell> = crcbl::engine::Loop<S, Towers>;

/// Runs the full loop.
///
/// # Errors
///
/// [`TowersError`] if the shell, the GPU or the simulation's server failed.
/// Teardown runs on every path.
pub fn run(options: &Options) -> Result<Summary, TowersError> {
    crcbl::engine::drive(start(options)?)
}

/// Opens a shell, a window, a GPU and the simulation.
///
/// # Errors
///
/// [`TowersError`] if any of them refused.
pub fn start(options: &Options) -> Result<Loop, TowersError> {
    let shell = crcbl::engine::open_shell(options.common.headless)?;
    with_shell(shell, options)
}

/// Builds the loop on an already-open shell, blocking on both waits.
///
/// The browser cannot use this — a main thread may not sit in
/// [`wait_for_configure`] — and takes [`PendingLoop`] instead. What the two
/// share is everything after the waiting, which is `assemble` — private,
/// because a caller has no `Booted` to hand it.
///
/// # Errors
///
/// [`TowersError`] if the window never configured, the GPU would not open, or
/// the simulation's server could not be built.
pub fn with_shell<S: Shell + ?Sized>(
    mut shell: Box<S>,
    options: &Options,
) -> Result<Loop<S>, TowersError> {
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
    assemble(
        Booted {
            shell,
            window,
            gpu,
            clock_source,
            events,
        },
        options,
    )
}

/// The half of start-up that is the same however the GPU arrived.
///
/// # Errors
///
/// [`TowersError`] if the simulation's server could not be built.
fn assemble<S: Shell + ?Sized>(
    booted: Booted<S, Gpu>,
    options: &Options,
) -> Result<Loop<S>, TowersError> {
    let booted = crcbl::engine::arm_screenshot(booted, &options.common);
    let paths = booted.gpu.paths();
    let game = Game::new(options.common.tick_hz).map_err(TowersError::Game)?;
    Ok(Loop::new(
        booted,
        Towers {
            game,
            actions: action_map(),
            pending_keys: Vec::new(),
            pending_restart: false,
            selected: 0,
            kind: tower::Kind::default(),
            render_state: RenderState::default(),
            stats: Stats::default(),
            page: PageStats::default(),
            paths,
        },
        options.common.loop_config(),
    ))
}

/// Creates the one window this sample has: its title, its app id, its size.
fn open_the_window<S: Shell + ?Sized>(
    shell: &mut S,
    clock_source: &Clock,
    mode: DisplayMode,
    size: Option<crcbl::shell::PhysicalSize>,
) -> Result<WindowId, TowersError> {
    Ok(crcbl::engine::open_window(
        shell,
        clock_source,
        &WindowDesc {
            title: "Towers",
            app_id: "sh.kryptic.crcbl.towers",
            size: crcbl::engine::requested_window_size(size),
            mode,
            ..WindowDesc::default()
        },
    )?)
}

/// Towers' half of the frame, and nothing else.
impl HostedGame for Towers {
    type Error = crate::game::GameError;
    type Gpu = Gpu;
    type MenuKind = MenuKind;
    type MenuAction = MenuAction;
    type Summary = Summary;

    const NAME: &'static str = "towers";

    fn menus() -> Menus {
        crate::menu::menus()
    }

    fn tick(&mut self, gpu: &mut Gpu, tick_dt: f64) {
        // `ActionMap` holds its timers in `f32`, which is the precision an
        // input edge is worth.
        #[allow(clippy::cast_possible_truncation)]
        self.actions.begin_tick(tick_dt as f32);
        for (key, pressed) in self.pending_keys.drain(..) {
            self.actions.key_event(key, pressed);
        }

        // The cursor moves here rather than on the frame's clock, because it
        // is what a command names and a command belongs to a tick.
        let plots = PLOTS.len() as u8;
        if self.actions.just_pressed(ACTION_PREV) {
            self.selected = (self.selected + plots - 1) % plots;
        }
        if self.actions.just_pressed(ACTION_NEXT) {
            self.selected = (self.selected + 1) % plots;
        }

        // …and so does the kind, for the same reason: it is what a command names.
        for (at, kind) in tower::ALL.iter().enumerate() {
            if self.actions.just_pressed(ACTION_KINDS[at]) {
                self.kind = *kind;
            }
        }

        self.game.set_controls(Controls {
            place: self
                .actions
                .just_pressed(ACTION_BUILD)
                .then_some(self.selected),
            kind: self.kind,
            upgrade: self
                .actions
                .just_pressed(ACTION_UPGRADE)
                .then_some(self.selected),
            start_wave: self.actions.just_pressed(ACTION_WAVE),
            restart: self.actions.just_pressed(ACTION_RESTART)
                || core::mem::take(&mut self.pending_restart),
        });
        self.game.tick();
        // Read off the bundle rather than kept from start-up alone, so the
        // heartbeat below and the panel are reporting the device this frame
        // actually has.
        self.paths = gpu.paths();
        self.stats = self.game.stats();
        self.log_heartbeat();
    }

    fn key_event(&mut self, key: KeyCode, pressed: bool) {
        // Queued rather than fed straight in: the map's edges belong to the
        // tick, not to the frame. See [`Towers::pending_keys`].
        self.pending_keys.push((key, pressed));
    }

    /// The map the console's `bind` and `unbind` rebind.
    ///
    /// The same map the queued keys above are replayed into, so a rebind typed
    /// at the console moves the key this game actually plays on rather than a
    /// copy of it.
    fn actions(&mut self) -> Option<&mut ActionMap> {
        Some(&mut self.actions)
    }

    fn menu_action(id: crcbl::ui::WidgetId) -> Option<MenuAction> {
        (id == crate::menu::RESTART_ID).then_some(MenuAction::Restart)
    }

    fn apply(&mut self, action: MenuAction) {
        match action {
            // Latched rather than applied: the restart is a command the server
            // owns, and the next tick is what seals it. See [`crate::menu`].
            MenuAction::Restart => self.pending_restart = true,
        }
    }

    fn menu_kind(&mut self, _menus: &mut Menus, paused: bool) -> MenuKind {
        MenuKind::of(paused)
    }

    fn draw(
        &mut self,
        gpu: &mut Gpu,
        draw_list: &mut crcbl::ui::draw_list::DrawList,
        _frame: FrameInfo,
    ) {
        self.render_state = self.game.render_state();
        gpu.set_field(&self.render_state);
        self.page = crate::page::draw(
            draw_list,
            gpu.atlas(),
            gpu.extent(),
            &self.render_state,
            self.selected,
            self.kind,
        );
    }

    /// **Towers' two modules, and no third.**
    ///
    /// No network section: this sample runs over `InMemoryTransport` and has no
    /// connection to report on — which is the one thing
    /// `docs/plan/sample/07-towers.md` most wants from this sample and the one
    /// thing `crcbl-net` cannot yet give it. No audio section either: slice 1
    /// plays nothing, and a section that said so would be a module with no
    /// system behind it.
    fn debug_sections(&self, panel: &mut crcbl::ui::DebugPanel) {
        panel.add(&self.stats);
        panel.add(&self.paths);
    }

    fn summary(&self, run: RunSummary) -> Summary {
        Summary {
            run,
            gold: self.stats.gold,
            lives: self.stats.lives,
            wave: self.stats.wave,
            kills: self.stats.kills,
            leaks: self.stats.leaks,
            built: self.stats.built,
            built_by_kind: self.stats.built_by_kind,
            upgrades: self.stats.upgrades,
            refused: self.stats.refused,
            outcome: self.stats.outcome,
            runs: self.stats.runs,
            paths: self.paths,
            commands: self.page.commands,
        }
    }

    fn log_summary(summary: &Summary) {
        crcbl::log::info!(
            "towers: {} frames, {} ticks, run {} left {} gold and {} lives at wave {}, \
             {} kill(s) and {} leak(s), {} tower(s) built ({} bolt, {} splash, {} slow) with \
             {} upgrade(s) and {} command(s) refused, {}, \
             {} overlay commands, geometry {:?}, binding {:?}, lighting {:?} ({:?})",
            summary.run.frames,
            summary.run.ticks,
            summary.runs,
            summary.gold,
            summary.lives,
            summary.wave,
            summary.kills,
            summary.leaks,
            summary.built,
            summary.built_by_kind[tower::Kind::Bolt.index()],
            summary.built_by_kind[tower::Kind::Splash.index()],
            summary.built_by_kind[tower::Kind::Slow.index()],
            summary.upgrades,
            summary.refused,
            summary.outcome.label(),
            summary.commands,
            summary.paths.geometry,
            summary.paths.binding,
            summary.paths.lighting,
            summary.run.exit,
        );
    }
}

// ---- polled start-up ---------------------------------------------------------

crcbl::impl_pending_loop!(
    running: Loop,
    gpu: Gpu,
    options: Options,
    error: TowersError,
    window: |shell, clock, options| open_the_window(
        shell,
        clock,
        options.common.display_mode(),
        options.common.size,
    ),
    context: |_options| (),
    assemble: |booted, options| assemble(booted, options),
);

// ---- tests -------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use crcbl::args::Common;
    use crcbl::engine::ExitReason;
    use crcbl::shell::{HeadlessShell, ShellBackend as Backend};
    use crcbl_sample_test::{headless_common, ui_text};

    fn scripted(options: &Options) -> Loop<HeadlessShell> {
        with_shell(Box::new(HeadlessShell::new()), options).expect("headless always starts")
    }

    /// A headless run of `frames` frames on the null backend.
    fn headless(frames: u64) -> Options {
        headless_with(frames, |_| {})
    }

    /// …with one knob turned.
    fn headless_with(frames: u64, tweak: impl FnOnce(&mut Common)) -> Options {
        let mut common = headless_common(crate::game::DEFAULT_TICK_HZ, frames);
        tweak(&mut common);
        Options { common }
    }

    /// Runs `count` frames.
    fn frames(engine: &mut Loop<HeadlessShell>, count: usize) {
        for _ in 0..count {
            engine.frame().expect("a frame");
        }
    }

    /// The digit that picks the slow tower — the third row of
    /// [`tower::ALL`], so the third of [`KIND_KEYS`].
    const KEY_SLOW: KeyCode = KIND_KEYS[2];

    /// Presses and releases one key, then runs a frame so the tick sees it.
    fn tap(engine: &mut Loop<HeadlessShell>, key: KeyCode) {
        let window = engine.window();
        engine
            .shell_mut()
            .key_press(window, key)
            .expect("the window is live");
        frames(engine, 1);
        engine
            .shell_mut()
            .key_release(window, key)
            .expect("the window is live");
        frames(engine, 1);
    }

    /// **A headless run plays the field and draws it.** The one check that says
    /// the whole bundle — server, wave table, physics world, renderer and
    /// overlay — came up and produced a frame with something on it.
    ///
    /// Nothing presses a key: waves arrive on their own, which is what makes
    /// this a claim about the demo rather than about the input path.
    #[test]
    fn a_headless_run_plays_the_field_and_draws_it() {
        // Long enough for the build phase and the first wave's release.
        let summary = run(&headless(400)).expect("the null backend always runs");
        assert_eq!(summary.run.frames, 400);
        assert_eq!(summary.run.exit, ExitReason::FrameBudget);
        assert!(summary.run.ticks > 0, "no tick ran");
        assert!(
            summary.commands > 0,
            "the run presented frames with nothing on them",
        );
        assert!(summary.wave > 0, "no wave ever started");
        assert_eq!(summary.built, 0, "a run that pressed nothing built a tower");
        assert_eq!(summary.gold, crate::wave::STARTING_GOLD);
        assert_eq!(summary.runs, 1);
    }

    /// **Two identical runs agree exactly**, which is what a fixed timestep
    /// over a table with no randomness in it is for.
    #[test]
    fn a_headless_run_is_deterministic() {
        let first = run(&headless(300)).expect("headless runs everywhere");
        let second = run(&headless(300)).expect("headless runs everywhere");
        assert_eq!(first, second, "two identical runs must agree exactly");
        assert_eq!(first.run.backend, Backend::Headless);
    }

    /// **The kind actions are named after the kinds**, in the same order, and
    /// each is bound to the digit that picks it.
    ///
    /// Three lists have to agree — [`tower::ALL`], [`ACTION_KINDS`] and
    /// [`KIND_KEYS`] — and nothing but this says so. A reordered tower table
    /// would silently bind `1` to the splash tower, and every test that presses
    /// a digit and reads a kind back would go on passing.
    #[test]
    fn the_kind_actions_are_named_after_the_kinds() {
        for (at, kind) in tower::ALL.iter().enumerate() {
            assert!(
                ACTION_KINDS[at].ends_with(kind.label()),
                "action {} is not the {} kind's",
                ACTION_KINDS[at],
                kind.label(),
            );
        }
        let map = action_map();
        for (at, name) in ACTION_KINDS.iter().enumerate() {
            assert_eq!(
                map.bindings(name),
                Some([Binding::Key(KIND_KEYS[at])].as_slice()),
                "{name} is not bound to {:?}",
                KIND_KEYS[at],
            );
        }
    }

    /// **The arrows move the build cursor, the digits move the kind, and `B`
    /// builds that kind on that plot**, which is the path this sample's commands
    /// take: shell event → action map → wire → module → validation.
    ///
    /// The gold is the observable rather than the tower count alone: a build
    /// that never reached the server leaves the purse full, and one that
    /// reached it twice leaves it short. And the gold is **the kind's**, which is
    /// what says the digit reached the server rather than only the client — a
    /// build that ignored the kind byte would spend a bolt tower's price here.
    #[test]
    fn the_arrows_move_the_cursor_and_b_builds_the_picked_kind_on_it() {
        let mut engine = scripted(&headless(400));
        frames(&mut engine, 4);
        assert_eq!(engine.game().selected(), 0);
        assert_eq!(
            engine.game().kind(),
            tower::Kind::Bolt,
            "the page opens on a kind nobody picked"
        );

        tap(&mut engine, KeyCode::ArrowRight);
        tap(&mut engine, KeyCode::ArrowRight);
        assert_eq!(
            engine.game().selected(),
            2,
            "two rights moved the cursor once"
        );

        // The slow tower, which is the third row and therefore `3`.
        tap(&mut engine, KEY_SLOW);
        assert_eq!(
            engine.game().kind(),
            tower::Kind::Slow,
            "the kind key did not reach the client",
        );

        tap(&mut engine, KeyCode::KeyB);
        frames(&mut engine, 2);
        let stats = engine.game().game().stats();
        assert_eq!(stats.towers, 1, "the build never reached the server");
        assert_eq!(stats.built, 1);
        assert_eq!(
            stats.built_of(tower::Kind::Slow),
            1,
            "the server built something other than the picked kind",
        );
        assert_eq!(stats.built_of(tower::Kind::Bolt), 0);
        assert_eq!(stats.refused, 0, "the build was refused");
        assert_eq!(
            stats.gold,
            crate::wave::STARTING_GOLD - tower::Kind::Slow.spec(tower::Tier::Base).cost,
            "the purse does not match one slow tower",
        );

        // The cursor wraps, which is the whole of what makes five plots
        // reachable with two keys.
        tap(&mut engine, KeyCode::ArrowLeft);
        tap(&mut engine, KeyCode::ArrowLeft);
        tap(&mut engine, KeyCode::ArrowLeft);
        assert_eq!(
            engine.game().selected(),
            (PLOTS.len() - 1) as u8,
            "the cursor did not wrap round the end of the list",
        );
    }

    /// **`U` steps the tower under the cursor up a tier, and the purse pays the
    /// kind's upgrade price.**
    ///
    /// The refusal beside it is the control: pressed on a plot with nothing on
    /// it, the same key reaches the same server and is turned down — so a build
    /// that upgraded on the client's word alone fails here rather than in a
    /// browser.
    #[test]
    fn u_steps_the_tower_under_the_cursor_up_and_is_refused_on_an_empty_plot() {
        let mut engine = scripted(&headless(400));
        frames(&mut engine, 4);

        // An empty plot first, so the refusal cannot be mistaken for a second
        // upgrade on a tower that already has one.
        tap(&mut engine, KeyCode::KeyU);
        frames(&mut engine, 2);
        let refused = engine.game().game().stats();
        assert_eq!(refused.upgrades, 0, "an empty plot was upgraded");
        assert_eq!(refused.refused, 1, "the server never saw the command");
        assert_eq!(
            refused.gold,
            crate::wave::STARTING_GOLD,
            "it took the gold anyway"
        );

        tap(&mut engine, KeyCode::KeyB);
        frames(&mut engine, 2);
        let purse = engine.game().game().stats().gold;

        tap(&mut engine, KeyCode::KeyU);
        frames(&mut engine, 2);
        let stats = engine.game().game().stats();
        assert_eq!(stats.upgrades, 1, "the upgrade never reached the server");
        assert_eq!(stats.refused, 1, "the upgrade was refused");
        assert_eq!(
            stats.gold,
            purse - tower::Kind::Bolt.spec(tower::Tier::Upgraded).cost,
            "the purse does not match one bolt tower's upgrade",
        );
        engine.finish(ExitReason::FrameBudget).expect("teardown");
    }

    /// **A scripted run builds towers, sends the wave and holds it**, which is
    /// the whole loop end to end through the real front end: two plots built by
    /// key, the wave sent by key, and every creep in it killed rather than let
    /// through.
    #[test]
    fn a_scripted_run_builds_towers_and_holds_the_first_wave() {
        let mut engine = scripted(&headless(900));
        frames(&mut engine, 4);

        // The first two plots, which are the two that cover the opening leg.
        tap(&mut engine, KeyCode::KeyB);
        tap(&mut engine, KeyCode::ArrowRight);
        tap(&mut engine, KeyCode::KeyB);
        assert_eq!(engine.game().game().stats().towers, 2);

        // …and send the wave rather than waiting the build phase out.
        tap(&mut engine, KeyCode::KeyN);
        let sent = engine.game().game().stats();
        assert_eq!(sent.wave, 1, "the wave command did not start a wave");
        assert_eq!(sent.refused, 0, "a command was refused");

        // Long enough for the whole first wave to be released and walked.
        frames(&mut engine, 600);
        let held = engine.game().game().stats();
        let first = crate::wave::WAVES[0];
        assert!(
            held.kills >= u64::from(first.creeps()),
            "the towers killed {} of the first wave's {}",
            held.kills,
            first.creeps(),
        );
        assert_eq!(held.leaks, 0, "the towers let {} through", held.leaks);
        assert_eq!(held.lives, crate::wave::STARTING_LIVES);
        let spent = 2 * tower::Kind::Bolt.spec(tower::Tier::Base).cost;
        assert!(
            held.gold > crate::wave::STARTING_GOLD - spent,
            "the kills paid nothing",
        );
        engine.finish(ExitReason::FrameBudget).expect("teardown");
    }

    /// **The overlay is composed of exactly the modules towers has**, and the
    /// stage's own rows reached the draw list with numbers in them.
    ///
    /// No network module and no audio one: this sample has neither system, and
    /// a panel that showed a row for either would be the overlay inventing
    /// state rather than reporting it.
    #[test]
    fn the_overlay_is_composed_of_exactly_the_modules_towers_has() {
        let mut engine = scripted(&headless_with(8, |common| {
            common.debug_overlay = Some(true);
        }));
        frames(&mut engine, 2);

        let titles: Vec<&str> = engine
            .debug()
            .panel
            .sections()
            .iter()
            .map(crcbl::ui::DebugSection::title)
            .collect();
        let expected: &[&str] = if engine.gpu().timings().is_some() {
            &["frame", "gpu", "counters", "towers", "paths"]
        } else {
            &["frame", "counters", "towers", "paths"]
        };
        assert_eq!(titles, expected, "no module appears that no system offered");

        let drawn = ui_text(engine.gpu().draw_list());
        for row in [
            "gold", "lives", "wave", "creeps", "towers", "upgrades", "refused",
        ] {
            assert!(drawn.iter().any(|text| text == row), "missing {row}");
        }
        // …and one row per tower kind, which is how the panel says a kind key
        // reached the server rather than only that a tower went up.
        for kind in tower::ALL {
            assert!(
                drawn.iter().any(|text| text == kind.label()),
                "missing the {} row",
                kind.label(),
            );
        }
        // The numbers are the stage's rather than a default: nothing has been
        // built and nothing has leaked, so the purse is whole.
        assert!(
            drawn.contains(&format!("{}", crate::wave::STARTING_GOLD)),
            "the gold row does not carry the opening purse: {drawn:?}",
        );
        engine.finish(ExitReason::FrameBudget).expect("teardown");
    }
}
