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
//! # The keyboard, or a click on a plot
//!
//! Every command has a key, and a pointer reaches the same commands through
//! the build menu: a click or a tap on a plot opens [`crate::build_menu`] there,
//! and a pick in it is **the keys' own command** — it moves the cursor to the
//! plot, picks the kind and latches the build (or the upgrade) for the next
//! tick, where [`Towers::tick`] seals it exactly as it seals `B`. So there is no
//! second command path for the server to disagree with, and the keyboard is
//! untouched by the menu being open. [`pointer_event`](HostedGame::pointer_event)
//! is overridden for that and nothing else; there is no
//! [`touch_event`](HostedGame::touch_event) override, because a finger's primary
//! contact already arrives as the pointer and a tap is one finger.
//!
//! **The page still has its row of buttons**, outside the canvas:
//! `web/demos/towers/main.js` puts the plot cursor, the three kinds, `B` and `U`
//! under the field and synthesises the key each stands for. It is kept while the
//! browser gate's canvas click is unwritten — `docs/backlog.md` says why.
//!
//! # `C` is the dev camera's
//!
//! It goes from the overhead view to a fly camera, a walk camera and back —
//! [`crate::dev_camera`]. While the camera moves, a context of its own takes
//! WASD, Space, Shift and the arrows from the field, so `S` and the cursor's
//! arrows are the camera's until the overhead view is back. Nothing the camera
//! does reaches a [`Controls`] frame.
//!
//! # Saving is the player's key, the console, the wave's end and the close
//!
//! `S` saves the run, so does the debug console's `save`, each wave's end is
//! saved on its own, and so is the run as its window closes — all through the
//! one save path, [`HostedGame::save`], into this run's [`Vault`], which is
//! nowhere for a headless run — and `S` is refused, on the
//! page as a notice, while a wave is coming in. A joiner's `S` is refused too:
//! the run is its host's. A close mid-wave keeps the last save rather than
//! write one a save cannot hold; see `Towers::save_on_close`.
//! What is resumed is [`crate::save`]'s: the lobby's *Continue*, `--resume`,
//! or — in a browser, which has no lobby — the saved run, opened at boot as
//! `apps/shard` opens its character.
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
use crcbl::engine::{
    Booted, Clock, ExitReason, FrameInfo, HostedGame, RunSummary, wait_for_configure,
};
use crcbl::input::{ActionDecl, ActionKind, ActionMap, Binding, GLOBAL_CONTEXT};
use crcbl::prelude::*;
use crcbl::shell::DisplayMode;

use crate::audio::Audio;
use crate::build_menu::{BuildMenu, Pick};
use crate::cue::Watcher;
use crate::dev_camera::{DevCamera, Mode, Steer};
use crate::game::{Controls, Game, RenderState, Stats};
use crate::gpu::{Gpu, Paths};
use crate::menu::{MenuAction, MenuKind, Menus};
use crate::page::PageStats;
use crate::save::Vault;
use crate::tower;
use crate::wave::Outcome;
use crcbl::save::{SaveDesk, SaveFailure, SaveRequest, SaveTrigger, Saved};

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
/// Save the run, between waves. An edge: one press is one write.
const ACTION_SAVE: &str = "save";

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

/// Go to the dev camera's next mode — overhead, fly, walk, overhead. An edge,
/// in [`GLOBAL_CONTEXT`] so it works whatever else holds the keyboard; see
/// [`crate::dev_camera`].
const ACTION_CAMERA: &str = "camera";
/// The dev camera's walk and strafe, a WASD composite. In
/// [`crate::dev_camera::CONTEXT`], with the two below.
const ACTION_CAMERA_MOVE: &str = "camera-move";
/// Turn and tilt either moving camera, on the arrows.
const ACTION_CAMERA_LOOK: &str = "camera-look";
/// Fly up and down.
const ACTION_CAMERA_RISE: &str = "camera-rise";

/// The key [`ACTION_CAMERA`] is bound to.
const CAMERA_KEY: KeyCode = KeyCode::KeyC;

/// The dev camera's keys: the toggle, and what its movement context binds.
fn declare_dev_camera(map: &mut ActionMap) {
    map.declare_in(
        GLOBAL_CONTEXT,
        ActionDecl {
            name: ACTION_CAMERA.into(),
            kind: ActionKind::Button,
            bindings: vec![Binding::Key(CAMERA_KEY)],
        },
    );
    for (name, kind, binding) in [
        (
            ACTION_CAMERA_MOVE,
            ActionKind::Axis2,
            Binding::Wasd {
                up: KeyCode::KeyW,
                down: KeyCode::KeyS,
                left: KeyCode::KeyA,
                right: KeyCode::KeyD,
            },
        ),
        (
            ACTION_CAMERA_LOOK,
            ActionKind::Axis2,
            Binding::Wasd {
                up: KeyCode::ArrowUp,
                down: KeyCode::ArrowDown,
                left: KeyCode::ArrowLeft,
                right: KeyCode::ArrowRight,
            },
        ),
        (
            ACTION_CAMERA_RISE,
            ActionKind::Axis1,
            Binding::KeyAxis {
                negative: KeyCode::ShiftLeft,
                positive: KeyCode::Space,
            },
        ),
    ] {
        map.declare_in(
            crate::dev_camera::CONTEXT,
            ActionDecl {
                name: name.into(),
                kind,
                bindings: vec![binding],
            },
        );
    }
}

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
        (ACTION_SAVE, vec![Binding::Key(KeyCode::KeyS)]),
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
    declare_dev_camera(&mut map);
    map
}

/// Each kind's count as the `[HUD]` line spells it — `bolt: 2  splash: 0  slow:
/// 1`, in [`tower::ALL`]'s order — so a new kind is on the line, under the key
/// the browser gate matches it by, without the line being edited.
fn built_on_the_hud_line(built_by_kind: &[u64; tower::KINDS]) -> String {
    let counts: Vec<String> = tower::ALL
        .iter()
        .map(|kind| format!("{}: {}", kind.label(), built_by_kind[kind.index()]))
        .collect();
    counts.join("  ")
}

/// The same counts as the run's summary spells them: `2 bolt, 0 splash, 1
/// slow`.
fn built_in_the_summary(built_by_kind: &[u64; tower::KINDS]) -> String {
    let counts: Vec<String> = tower::ALL
        .iter()
        .map(|kind| format!("{} {}", built_by_kind[kind.index()], kind.label()))
        .collect();
    counts.join(", ")
}

/// How long a refused command's line — or a save's — stays on the page, on
/// the frame's clock: long enough to read, short enough that it is about the
/// key just pressed.
const NOTICE_FOR: std::time::Duration = std::time::Duration::from_secs(3);

/// Why a save is refused while the lobby or a joining panel is up, in the
/// words [`crate::game::NotSaved`] uses for the stage's own refusals.
const NOTHING_PLAYED: &str = "NO RUN IS BEING PLAYED";

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
    /// A build, and an upgrade, picked in the build menu and waiting for the
    /// next tick — where they stand in for `B` and `U`, on the plot the pick
    /// moved the cursor to. See [`Towers::apply_pick`].
    pending_build: bool,
    pending_upgrade: bool,
    /// The menu a click or a tap on a plot opens — see [`crate::build_menu`].
    build_menu: BuildMenu,
    /// Which plot the build cursor is on. **Presentation**: it never crosses
    /// the wire, and what does is the plot number a build command names.
    selected: u8,
    /// Which kind the next build is of. Presentation in the same sense, with one
    /// difference: it travels on **every** command frame rather than only on the
    /// tick a build is asked for — see [`Controls::kind`].
    kind: tower::Kind,
    /// Which camera the frame is drawn from, and the fly camera and walker
    /// behind the two that move. **Presentation** in the same sense as the
    /// cursor, and further: nothing in it reaches a command frame.
    dev_camera: DevCamera,
    /// Refilled from the simulation every frame.
    render_state: RenderState,
    /// The simulation's numbers, snapshotted in [`Towers::tick`].
    stats: Stats,
    /// What the last frame's overlay drew, from the same frame.
    page: PageStats,
    /// The line the latest refusal of this player's commands — or the latest
    /// save — left on the page, and how much longer it stays — see
    /// [`NOTICE_FOR`].
    notice: Option<(String, std::time::Duration)>,
    /// Where this run's saves go: nowhere for a headless run, which is what
    /// keeps the test suite and CI out of a real data directory.
    vault: Vault,
    /// What every save this run took went through — see
    /// [`HostedGame::save`] — and the F3 panel's "storage" section.
    desk: SaveDesk,
    /// The field's sounds and the device they play on — `None` on a headless
    /// run, which opens no device and plays nothing. See [`crate::audio`].
    audio: Option<Audio>,
    /// What reads the field's events off the replicated field for `audio` —
    /// see [`crate::cue`].
    watcher: Watcher,
    /// The simulation's rate, which is what the watcher measures a gap
    /// between two snapshots against.
    tick_hz: u32,
    /// Which selectors this device drew through, read off the GPU bundle.
    ///
    /// Kept here rather than reached through `gpu` because
    /// [`HostedGame::debug_sections`] and [`HostedGame::summary`] are handed
    /// `&self` and no GPU at all.
    paths: Paths,
    /// The lobby, while it is open — see [`crate::lobby`]. The solo game
    /// under it is built and does not tick, so the field it draws under the
    /// panel is a run that has not started, and the one a player picking
    /// solo gets. Every key and every character is the lobby's while it is
    /// open. Native only.
    #[cfg(not(target_arch = "wasm32"))]
    lobby: Option<crate::lobby::Lobby>,
    /// A join waiting for the host's map — picked in the lobby, or asked for
    /// by `--join` or `--browse`. Until the map comes there is no joined game
    /// at all (`crate::lan`), so the game drawn is still the one under the
    /// lobby, or, from the command line, an idle solo run under the joining
    /// panel; neither ticks. Native only.
    #[cfg(not(target_arch = "wasm32"))]
    joining: Option<crate::lan::Joining>,
    /// The lobby a join was picked from and the idle solo game that was
    /// under it, set aside while the joined game runs: when its session ends
    /// the player goes back to both — see `Towers::drive_session_end`. Native
    /// only.
    #[cfg(not(target_arch = "wasm32"))]
    parked: Option<(crate::lobby::Lobby, Game)>,
    /// Why a join the command line asked for ended without a game, or how
    /// its session ended — the warning the joining panel shows for as long
    /// as the window stays open. A join picked in the lobby says either in
    /// the lobby instead. Native only.
    #[cfg(not(target_arch = "wasm32"))]
    panel_warning: Option<String>,
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
             {}  upgrades: {}  bolts: {}  kills: {}  leaks: {}  \
             shots: {}  built: {}  refused: {}  outcome: {}  runs: {}  plot: {}  kind: {}  \
             geometry: {:?}  binding: {:?}  lighting: {:?}",
            stats.ticks,
            stats.gold,
            stats.lives,
            stats.wave,
            next,
            stats.creeps,
            stats.towers,
            built_on_the_hud_line(&stats.built_by_kind),
            stats.upgrades,
            stats.bolts,
            stats.kills,
            stats.leaks,
            stats.shots,
            stats.built,
            stats.refused,
            stats.outcome.label(),
            stats.runs,
            self.game.map().plots()[usize::from(self.selected)].label,
            self.kind.label(),
            self.paths.geometry,
            self.paths.binding,
            self.paths.lighting,
        );
    }

    /// Ages the refusal line by `render_dt`, and puts the newest refusal of
    /// this player's commands in its place, if the server made one — the
    /// same line solo, hosting or joined, since [`Game::take_refusals`]
    /// hides which server said no.
    ///
    /// Every refusal is heard, each once, at the plot the cursor is on —
    /// the line only has room for the newest.
    fn update_notice(&mut self, render_dt: std::time::Duration) {
        self.notice = self.notice.take().and_then(|(line, left)| {
            left.checked_sub(render_dt)
                .filter(|left| !left.is_zero())
                .map(|left| (line, left))
        });
        let refusals = self.game.take_refusals();
        if let Some(audio) = &mut self.audio {
            for _ in &refusals {
                audio.play(crate::cue::refused_at(self.game.map(), self.selected));
            }
        }
        if let Some(refusal) = refusals.last() {
            crcbl::log::info!("towers: refused: {}", refusal.label());
            self.notice = Some((format!("REFUSED: {}", refusal.label()), NOTICE_FOR));
        }
    }

    /// Saves the run now, as `S` asks, and says on the page what became of
    /// it: saved, or why not — a wave coming in, a finished run, a joiner,
    /// nowhere to keep it.
    fn save_now(&mut self) {
        let line = match self.save(SaveRequest::new(SaveTrigger::Input)) {
            Ok(saved) => format!("SAVED: {}", saved.summary.to_uppercase()),
            Err(failure) => format!("NOT SAVED: {failure}"),
        };
        crcbl::log::info!("towers: {line}");
        self.notice = Some((line, NOTICE_FOR));
    }

    /// Writes the run at a wave's end, the first frame of the build phase
    /// after it. Quiet on the page — the wave's end is the player's moment,
    /// not a notice's — and logged, so a run says which waves it kept.
    fn autosave(&mut self) {
        if self.game.wave_end().is_none() {
            return;
        }
        match self.save(SaveRequest::new(SaveTrigger::Autosave)) {
            Ok(saved) => crcbl::log::info!(
                "towers: autosaved the end of {} ({})",
                saved.summary,
                self.vault.where_it_goes()
            ),
            // A run that keeps nothing saves in name only, as shard's does.
            Err(SaveFailure::Nowhere) => {}
            Err(failure) => crcbl::log::warn!("towers: the autosave failed: {failure}"),
        }
    }

    /// Writes the run as its window closes, when a save can hold it — the
    /// build phase of a run being played, solo or hosting — and otherwise
    /// keeps the last save and logs why: a wave coming in, a finished run, a
    /// joiner. Only for a stop the player asked for — the window closed or
    /// gone, or the debug console's `quit`; a frame budget was told when to
    /// stop, and a failed frame may not have left the run it was in. Nothing is
    /// written while the lobby or a joining panel is up, because the game
    /// under it never ticked and would write a fresh run over the saved one.
    /// A failed write is logged and the close goes on: the loop has already
    /// accepted it.
    fn save_on_close(&mut self, exit: ExitReason) {
        let asked = matches!(
            exit,
            ExitReason::CloseRequested | ExitReason::WindowDestroyed | ExitReason::Quit
        );
        if !asked || self.in_front() {
            return;
        }
        match self.save(SaveRequest::new(SaveTrigger::Close)) {
            Err(SaveFailure::Refused(why)) => {
                crcbl::log::info!("towers: closed without saving ({why}); the last save stands");
            }
            Ok(saved) => crcbl::log::info!(
                "towers: saved {} on close ({})",
                saved.summary,
                self.vault.where_it_goes()
            ),
            // A run that keeps nothing saves in name only, as the autosave.
            Err(SaveFailure::Nowhere) => {}
            Err(failure) => crcbl::log::warn!("towers: the save on close failed: {failure}"),
        }
    }

    /// What the console's storage section and this crate's tests read: the
    /// desk every save went through.
    pub const fn desk(&self) -> &SaveDesk {
        &self.desk
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

    /// The build menu, for this crate's own tests.
    pub const fn build_menu(&self) -> &BuildMenu {
        &self.build_menu
    }

    /// Does what a pick in the build menu asks, the way the keys would: the
    /// cursor goes to its plot, a build's kind becomes the picked kind, and the
    /// build or the upgrade is latched for the next tick — which seals it as
    /// it seals `B` or `U`.
    fn apply_pick(&mut self, pick: Pick) {
        match pick {
            Pick::Build { plot, kind } => {
                self.selected = plot;
                self.kind = kind;
                self.pending_build = true;
            }
            Pick::Upgrade { plot } => {
                self.selected = plot;
                self.pending_upgrade = true;
            }
        }
    }

    /// The dev camera, for this crate's own tests.
    pub const fn dev_camera(&self) -> &DevCamera {
        &self.dev_camera
    }

    /// The field's sounds, when this run plays any — never on a headless
    /// run. See [`crate::audio`].
    pub const fn audio(&self) -> Option<&Audio> {
        self.audio.as_ref()
    }

    /// Plays whatever the field did since the last frame that heard it —
    /// read off the replicated field, so solo, a host and a joiner hear alike
    /// (see [`crate::cue`]). Nothing at all without [`Towers::audio`].
    fn listen(&mut self) {
        let Some(audio) = &mut self.audio else {
            return;
        };
        let field = self.game.replicated();
        for cue in self.watcher.hear(&field, self.game.map(), self.tick_hz) {
            audio.play(cue);
        }
    }

    /// Goes to the dev camera's next mode, and pushes its keys' context on
    /// leaving the overhead camera or pops it on coming back.
    fn cycle_camera(&mut self) {
        let left = self.dev_camera.mode();
        let arrived = self.dev_camera.cycle();
        if left == Mode::Overhead {
            self.actions
                .push_context(crate::dev_camera::CONTEXT)
                .expect("the action map declares the dev camera's context, off the stack");
        } else if arrived == Mode::Overhead {
            self.actions
                .pop_context(crate::dev_camera::CONTEXT)
                .expect("nothing is pushed over the dev camera's context");
        }
    }

    /// What the dev camera's keys ask for this tick.
    fn steer(&self) -> Steer {
        let (strafe, ahead) = self.actions.axis2(ACTION_CAMERA_MOVE);
        let (turn, tilt) = self.actions.axis2(ACTION_CAMERA_LOOK);
        Steer {
            ahead,
            strafe,
            rise: self.actions.axis1(ACTION_CAMERA_RISE),
            turn,
            tilt,
        }
    }

    /// Whether the lobby is open.
    #[must_use]
    pub const fn in_the_lobby(&self) -> bool {
        #[cfg(not(target_arch = "wasm32"))]
        {
            self.lobby.is_some()
        }
        #[cfg(target_arch = "wasm32")]
        {
            false
        }
    }

    /// Whether nothing is being played: the lobby is open, a join is waiting
    /// for the host's map, or a command-line join failed or its session
    /// ended. The game underneath does not tick.
    #[must_use]
    pub const fn in_front(&self) -> bool {
        #[cfg(not(target_arch = "wasm32"))]
        {
            self.lobby.is_some() || self.joining.is_some() || self.panel_warning.is_some()
        }
        #[cfg(target_arch = "wasm32")]
        {
            false
        }
    }

    /// Whether a join is waiting for the host's map.
    #[cfg(not(target_arch = "wasm32"))]
    #[must_use]
    pub const fn is_joining(&self) -> bool {
        self.joining.is_some()
    }

    /// Runs the join under way for a frame covering `render_dt`: the host's
    /// map makes the GPU's field and then the game, and a join that ends
    /// without one says why — in the lobby when it was picked there, on the
    /// joining panel and in the log when the command line asked for it.
    #[cfg(not(target_arch = "wasm32"))]
    fn drive_join(&mut self, gpu: &mut Gpu, render_dt: std::time::Duration) {
        use crate::lan::Progress;

        let Some(joining) = self.joining.take() else {
            return;
        };
        match joining.step(render_dt) {
            Progress::Waiting(joining) => self.joining = Some(joining),
            // The field first: a game on the host's map is never drawn over
            // the field of another.
            Progress::Joined(game) => match gpu.set_map(game.map()) {
                Ok(()) => {
                    // The lobby the join was picked from waits, with the solo
                    // game that was under it, for the session to end.
                    let lobby = self.lobby.take();
                    let solo = self.start(game);
                    self.parked = lobby.map(|lobby| (lobby, solo));
                }
                Err(error) => self.join_failed(&format!("cannot draw the host's map: {error}")),
            },
            Progress::Failed(failure) => self.join_failed(&failure.to_string()),
        }
    }

    /// A join ended without a game: the lobby says why when there is one, and
    /// the joining panel and the log do when the command line asked for the
    /// join.
    #[cfg(not(target_arch = "wasm32"))]
    fn join_failed(&mut self, why: &str) {
        match &mut self.lobby {
            Some(lobby) => lobby.join_failed(why),
            None => {
                crcbl::log::error!("lan: the join failed: {why}");
                self.panel_warning = Some(format!("JOIN FAILED: {why}"));
            }
        }
    }

    /// Notices that the joined game's session ended — the host left or shut
    /// down, removed this player, or the link died — and takes the player
    /// back to where the join started: the lobby it was picked from, saying
    /// how the session ended, over the idle solo game that was under it and
    /// the GPU's field for this process's own map again. A join the command
    /// line asked for has no lobby to go back to, so its panel says it, as a
    /// failed join's does, and the stopped field stays under it.
    #[cfg(not(target_arch = "wasm32"))]
    fn drive_session_end(&mut self, gpu: &mut Gpu) {
        if self.panel_warning.is_some() {
            return;
        }
        let Some(how) = self.game.session_end() else {
            return;
        };
        crcbl::log::warn!("lan: the session ended: {how}");
        let Some((mut lobby, solo)) = self.parked.take() else {
            self.panel_warning = Some(format!("SESSION ENDED: {how}"));
            return;
        };
        // The field first, as a join's is: the solo game is never drawn over
        // the host's field.
        if let Err(error) = gpu.set_map(solo.map()) {
            crcbl::log::error!("lan: cannot draw this machine's map again: {error}");
            self.panel_warning = Some(format!(
                "SESSION ENDED: {how}; cannot draw this machine's map: {error}"
            ));
            return;
        }
        lobby.session_ended(&how);
        drop(self.start(solo));
        self.lobby = Some(lobby);
    }

    /// The joining panel: where the command line's join is going, or why it
    /// failed, or how its session ended.
    #[cfg(not(target_arch = "wasm32"))]
    fn joining_panel(&self) -> crcbl::ui::menu::Menu {
        use crcbl::ui::menu::{Caption, Menu};

        let mut menu = Menu::new(crate::lobby::JOINING_TITLE, Vec::new());
        if let Some(warning) = &self.panel_warning {
            menu.subtitle.push(Caption::warning(warning.clone()));
        } else if let Some(joining) = &self.joining {
            menu.subtitle.push(
                match joining.lan().host() {
                    Some(host) => format!("JOINING {host}"),
                    None => "LOOKING FOR HOSTS ON THE LAN".to_string(),
                }
                .into(),
            );
        }
        menu
    }

    /// Starts `game` in place of the one being drawn, from a clean cursor —
    /// the lobby's pick, the host's map arriving, or the way back from an
    /// ended session, and the only way a game is replaced. Answers the game
    /// it replaced.
    #[cfg(not(target_arch = "wasm32"))]
    fn start(&mut self, game: Game) -> Game {
        self.lobby = None;
        self.joining = None;
        self.panel_warning = None;
        self.selected = 0;
        self.kind = tower::Kind::default();
        // Back to the overhead camera over the new game's field, which may be
        // another map: the walker's world is built from it.
        if self.dev_camera.mode() != Mode::Overhead {
            self.actions
                .pop_context(crate::dev_camera::CONTEXT)
                .expect("nothing is pushed over the dev camera's context");
        }
        self.dev_camera = DevCamera::new(game.map());
        self.pending_keys.clear();
        self.pending_restart = false;
        self.pending_build = false;
        self.pending_upgrade = false;
        self.build_menu.close();
        self.stats = Stats::default();
        self.notice = None;
        // Another game's field: nothing in it was heard happening.
        self.watcher.forget();
        std::mem::replace(&mut self.game, game)
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
/// A native `--serve` is no loop and has no window to summarise, so it is
/// `serve`'s, and asked of this it is refused by name.
///
/// # Errors
///
/// [`TowersError`] if the shell, the GPU or the simulation's server failed.
/// Teardown runs on every path.
pub fn run(options: &Options) -> Result<Summary, TowersError> {
    #[cfg(not(target_arch = "wasm32"))]
    if options.serve.is_some() {
        return Err(TowersError::Game(crate::game::GameError::Server(
            "--serve is run by crcbl_towers::serve, not run".into(),
        )));
    }
    crcbl::engine::drive(start(options)?)
}

/// Runs `--serve`'s dedicated server — no window, no loop, a console on
/// stdin (see `crate::lan::serve`) — until `quit` is typed at it, and
/// answers its last status line, which is what it has instead of a summary.
///
/// # Errors
///
/// [`TowersError`] if the listener would not bind, or `options` asked for
/// no server.
#[cfg(not(target_arch = "wasm32"))]
pub fn serve(options: &Options) -> Result<String, TowersError> {
    let Some(port) = options.serve else {
        return Err(TowersError::Game(crate::game::GameError::Server(
            "no --serve was asked for".into(),
        )));
    };
    crate::lan::serve::serve(
        port,
        &options.map,
        options.common.tick_hz,
        options.record.as_deref(),
        options.resume,
    )
    .map_err(TowersError::Game)
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

    let gpu = Gpu::open(
        shell.as_ref(),
        window,
        extent,
        options.common.gpu(),
        &options.map,
    )?;
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
    let mut booted = crcbl::engine::arm_screenshot(booted, &options.common);
    let paths = booted.gpu.paths();
    let build_menu = BuildMenu::new(booted.gpu.images_mut())
        .map_err(|error| TowersError::Game(crate::game::GameError::Art(error)))?;
    let vault = Vault::player(options.common.headless);
    #[cfg_attr(target_arch = "wasm32", allow(unused_variables))]
    let (game, joining) = open_game(options, &vault).map_err(TowersError::Game)?;
    let dev_camera = DevCamera::new(game.map());
    Ok(Loop::new(
        booted,
        Towers {
            game,
            actions: action_map(),
            pending_keys: Vec::new(),
            pending_restart: false,
            pending_build: false,
            pending_upgrade: false,
            build_menu,
            selected: 0,
            kind: tower::Kind::default(),
            dev_camera,
            render_state: RenderState::default(),
            stats: Stats::default(),
            page: PageStats::default(),
            notice: None,
            paths,
            // Headless plays nothing: no device, and no cue to play on one.
            audio: (!options.common.headless).then(Audio::open),
            watcher: Watcher::default(),
            tick_hz: options.common.tick_hz,
            #[cfg(not(target_arch = "wasm32"))]
            lobby: options.lobby.then(|| {
                crate::lobby::Lobby::on_the_lan(crate::lan::SESSION, options.common.tick_hz)
                    .offering(vault.load(&options.map))
            }),
            desk: vault.desk(),
            vault,
            #[cfg(not(target_arch = "wasm32"))]
            joining,
            #[cfg(not(target_arch = "wasm32"))]
            parked: None,
            #[cfg(not(target_arch = "wasm32"))]
            panel_warning: None,
        },
        options.common.loop_config(),
    ))
}

/// What a join the command line asked for waits in, natively; nothing, in a
/// browser, which has no networking.
#[cfg(not(target_arch = "wasm32"))]
type CommandLineJoin = Option<crate::lan::Joining>;
#[cfg(target_arch = "wasm32")]
type CommandLineJoin = ();

/// The simulation the command line asked for: solo, or — natively — hosting,
/// or joining or looking for a co-op session. See `crate::lan`. Solo and a
/// host open on the run saved in `vault` when `--resume` asks for it; a
/// browser does whenever there is one, having no lobby to offer it from.
///
/// A join has no game until the host's map arrives, so for `--join` and
/// `--browse` this answers the join, and an idle solo run on this process's
/// own map for the frame to hold until then — never ticked, and replaced by
/// the joined game the moment it exists.
///
/// # Errors
///
/// [`crate::game::GameError`] if the server could not be built, the LAN
/// session could not start, or `--resume` found no run it would resume.
fn open_game(
    options: &Options,
    vault: &Vault,
) -> Result<(Game, CommandLineJoin), crate::game::GameError> {
    let tick_hz = options.common.tick_hz;
    #[cfg(not(target_arch = "wasm32"))]
    {
        use crate::game::GameError;
        use crate::lan::{JOIN_TIMEOUT, Joining, SESSION};
        use crcbl::lan::{LanBind, LanClient, LanMode};

        let client = match options.lan {
            LanMode::Off => {
                let mut game = Game::new(tick_hz, &options.map)?;
                if options.resume {
                    resume(&mut game, vault)?;
                }
                return Ok((game, None));
            }
            LanMode::Host { port } => {
                let mut game = Game::host(
                    tick_hz,
                    &options.map,
                    LanBind::on_the_lan(port),
                    options.record.as_deref(),
                    SESSION
                        .player_id(options.common.headless)
                        .map_err(GameError::Lan)?,
                )?;
                if options.resume {
                    resume(&mut game, vault)?;
                }
                return Ok((game, None));
            }
            LanMode::Join(addr) => SESSION
                .player_id(options.common.headless)
                .and_then(|player| LanClient::join(SESSION, player, addr, tick_hz)),
            LanMode::Browse => SESSION
                .player_id(options.common.headless)
                .and_then(|player| LanClient::browse_the_lan(SESSION, player, tick_hz)),
        }
        .map_err(GameError::Lan)?;
        let joining = Joining::new(client, tick_hz, JOIN_TIMEOUT);
        Ok((Game::new(tick_hz, &options.map)?, Some(joining)))
    }
    #[cfg(target_arch = "wasm32")]
    {
        let mut game = Game::new(tick_hz, &options.map)?;
        match resume(&mut game, vault) {
            Ok(()) | Err(crate::game::GameError::Resume(crate::save::SaveError::NoSave)) => {}
            Err(error) => crcbl::log::warn!("towers: opening on a fresh run: {error}"),
        }
        Ok((game, ()))
    }
}

/// Puts the run saved in `vault` in place of `game`'s fresh one.
///
/// # Errors
///
/// [`crate::game::GameError::Resume`] naming why not: no save, or one refused
/// — another map, another version, a corrupt file.
fn resume(game: &mut Game, vault: &Vault) -> Result<(), crate::game::GameError> {
    use crate::game::GameError;
    use crate::save::SaveError;

    let checkpoint = vault
        .load(game.map())
        .map_err(GameError::Resume)?
        .ok_or(GameError::Resume(SaveError::NoSave))?;
    game.restore(&checkpoint).map_err(GameError::Resume)?;
    crcbl::log::info!(
        "towers: resumed at the end of wave {}/{} with {} lives and {} gold",
        checkpoint.wave(),
        crate::wave::WAVES.len(),
        checkpoint.lives(),
        checkpoint.gold(),
    );
    Ok(())
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
        // Nothing runs under the lobby or a join: the run starts when one is
        // picked, or when the host's map is in.
        if self.in_front() {
            return;
        }
        // `ActionMap` holds its timers in `f32`, which is the precision an
        // input edge is worth.
        #[allow(clippy::cast_possible_truncation)]
        self.actions.begin_tick(tick_dt as f32);
        for (key, pressed) in self.pending_keys.drain(..) {
            self.actions.key_event(key, pressed);
        }

        // The camera moves on the tick, as `Flyer` does, so a walk covers the
        // same ground on every machine. Its towers are the last frame's,
        // which is what is on screen.
        if self.actions.just_pressed(ACTION_CAMERA) {
            self.cycle_camera();
        }
        self.dev_camera
            .step(self.steer(), tick_dt, &self.render_state.towers);

        // The cursor moves here rather than on the frame's clock, because it
        // is what a command names and a command belongs to a tick.
        let plots = self.game.map().plots().len() as u8;
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

        // A pick in the build menu is the key it stands in for, and so is the
        // pause menu's `RESTART`. Taken before the frame is built, so a pick
        // and a key on one tick are one command and no latch outlives the tick.
        let picked_build = core::mem::take(&mut self.pending_build);
        let picked_upgrade = core::mem::take(&mut self.pending_upgrade);
        let picked_restart = core::mem::take(&mut self.pending_restart);
        self.game.set_controls(Controls {
            place: (self.actions.just_pressed(ACTION_BUILD) || picked_build)
                .then_some(self.selected),
            kind: self.kind,
            upgrade: (self.actions.just_pressed(ACTION_UPGRADE) || picked_upgrade)
                .then_some(self.selected),
            start_wave: self.actions.just_pressed(ACTION_WAVE),
            restart: self.actions.just_pressed(ACTION_RESTART) || picked_restart,
        });
        self.game.tick();
        // After the tick, so what is saved is the stage the player sees.
        if self.actions.just_pressed(ACTION_SAVE) {
            self.save_now();
        }
        // Read off the bundle rather than kept from start-up alone, so the
        // heartbeat below and the panel are reporting the device this frame
        // actually has.
        self.paths = gpu.paths();
        self.stats = self.game.stats();
        self.log_heartbeat();
    }

    fn key_event(&mut self, key: KeyCode, pressed: bool) {
        #[cfg(not(target_arch = "wasm32"))]
        if let Some(lobby) = &mut self.lobby {
            lobby.key(key, pressed);
            return;
        }
        // Queued rather than fed straight in: the map's edges belong to the
        // tick, not to the frame. See [`Towers::pending_keys`].
        self.pending_keys.push((key, pressed));
    }

    /// The pointer, for the build menu — and nothing else, while the lobby is
    /// open: the lobby's panel is the loop's, and it has had the press.
    fn pointer_event(&mut self, pointer: crcbl::engine::PointerUpdate) {
        if self.in_the_lobby() {
            return;
        }
        self.build_menu.pointer(pointer);
    }

    /// The lobby's connect address, typed. Nothing else here takes text.
    #[cfg(not(target_arch = "wasm32"))]
    fn text_event(&mut self, text: &str) {
        if let Some(lobby) = &mut self.lobby {
            lobby.text(text);
        }
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
        MenuAction::from_id(id)
    }

    fn apply(&mut self, action: MenuAction) {
        match action {
            // Latched rather than applied: the restart is a command the server
            // owns, and the next tick is what seals it. See [`crate::menu`].
            MenuAction::Restart => self.pending_restart = true,
            #[cfg(not(target_arch = "wasm32"))]
            MenuAction::Lobby(pick) => {
                let Some(lobby) = &mut self.lobby else {
                    return;
                };
                // Any pick replaces a join under way, whether or not it starts
                // anything, as the lobby forgets it too.
                self.joining = None;
                match lobby.pick(pick, self.game.map()) {
                    Some(crate::lobby::Picked::Solo) => self.lobby = None,
                    // The solo run under the lobby has not ticked, so the
                    // saved one takes its place before it does.
                    Some(crate::lobby::Picked::Continue(checkpoint)) => {
                        match self.game.restore(&checkpoint) {
                            Ok(()) => {
                                self.lobby = None;
                                // A run that was saved, not one that played.
                                self.watcher.forget();
                            }
                            Err(error) => lobby.continue_failed(&error),
                        }
                    }
                    Some(crate::lobby::Picked::Session(game)) => drop(self.start(game)),
                    // The lobby stays up, saying where, until the map is in.
                    Some(crate::lobby::Picked::Joining(joining)) => self.joining = Some(joining),
                    // The lobby shows why on the next frame.
                    None => {}
                }
            }
        }
    }

    /// Pause, or none — or, while the lobby is open, the lobby, rebuilt
    /// first when what it lists changed.
    #[cfg_attr(target_arch = "wasm32", allow(unused_variables))]
    fn menu_kind(&mut self, menus: &mut Menus, paused: bool) -> MenuKind {
        #[cfg(not(target_arch = "wasm32"))]
        if let Some(lobby) = &mut self.lobby {
            use crate::menu::CONNECT_ID;

            lobby.poll();
            if lobby.take_changed() {
                // A rebuild resets the selection, so it is carried across by
                // id — onto the first row if what it was on is gone.
                let selected = menus
                    .get_mut(MenuKind::Lobby)
                    .and_then(|menu| menu.selected_item().map(|item| item.id));
                let mut menu = lobby.menu();
                if let Some(id) = selected {
                    menu.select_id(id);
                }
                menus.replace(MenuKind::Lobby, menu);
            }
            if let Some(menu) = menus.get_mut(MenuKind::Lobby) {
                menu.set_item_hint(CONNECT_ID, lobby.connect_hint());
                // Typing is for the connect row, so the selection goes there
                // and Enter joins what was typed.
                if lobby.take_typed() {
                    menu.select_id(CONNECT_ID);
                }
            }
            return MenuKind::in_the_lobby(paused);
        }
        #[cfg(not(target_arch = "wasm32"))]
        if self.joining.is_some() || self.panel_warning.is_some() {
            let panel = self.joining_panel();
            let stale = menus
                .get_mut(MenuKind::Joining)
                .is_none_or(|menu| menu.subtitle != panel.subtitle);
            if stale {
                menus.replace(MenuKind::Joining, panel);
            }
            return MenuKind::joining(paused);
        }
        MenuKind::of(paused)
    }

    fn draw(
        &mut self,
        gpu: &mut Gpu,
        draw_list: &mut crcbl::ui::draw_list::DrawList,
        frame: FrameInfo,
    ) {
        // Here because `draw` is the one hook that runs on every frame, paused
        // or not: a LAN session is served on wall time, so a paused host goes
        // on serving the others, and a join goes on waiting for its map. See
        // [`Game::frame`].
        #[cfg(not(target_arch = "wasm32"))]
        self.drive_join(gpu, frame.render_dt);
        self.game.frame(frame.render_dt);
        if !self.in_front() {
            self.autosave();
        }
        #[cfg(not(target_arch = "wasm32"))]
        self.drive_session_end(gpu);
        let camera = self.dev_camera.camera();
        // The ear goes where the eye is before anything this frame is heard.
        if let Some(audio) = &self.audio {
            audio.hear_from(&camera);
        }
        self.update_notice(frame.render_dt);
        self.listen();
        self.render_state = self.game.render_state();
        gpu.set_field(&self.render_state);
        gpu.set_camera(camera);
        // Under the page, so a panel is never covered by a creep's bar —
        // and not in the walk; see `crate::bars`.
        if self.dev_camera.mode() != Mode::Walk {
            let bars = crate::bars::bars(&self.render_state, &camera, gpu.extent());
            crate::bars::draw(draw_list, &bars);
        }
        let menu_frame = crate::build_menu::Frame {
            camera: &camera,
            extent: gpu.extent(),
            state: &self.render_state,
            plots: self.game.map().plots(),
            live: !frame.paused && !self.in_front(),
        };
        // The hover outline under the page too, for the bars' reason.
        self.build_menu.draw_highlight(draw_list, &menu_frame);
        if let Some((line, _)) = &self.notice {
            crate::page::draw_notice(draw_list, gpu.atlas(), gpu.extent(), line);
        }
        self.page = crate::page::draw(
            draw_list,
            gpu.atlas(),
            gpu.extent(),
            &self.render_state,
            self.game.map().plots(),
            self.selected,
            self.kind,
        );
        // Over the page: the menu opens where the plot is, which may be under
        // a panel's corner.
        if let Some(pick) = self.build_menu.frame(draw_list, gpu.atlas(), &menu_frame) {
            self.apply_pick(pick);
        }
    }

    /// **Towers' three modules, and a fourth during a LAN session.**
    ///
    /// The stage's numbers, the selectors the frame took, and the "camera"
    /// section — which camera the frame is drawn from, and while walking what
    /// the walker stands on and what its last move met; see
    /// [`crate::dev_camera`].
    ///
    /// The "lan" section is `crcbl::lan`'s: the port, the players and the
    /// snapshot's size against a datagram on a host, the session and the last
    /// applied tick on a joiner. Beside it the "net" section, the netgraph
    /// `docs/plan/sample/07-towers.md` wants, is `crcbl::lan::netgraph`'s: a
    /// row per peer on a host, the one link on a joiner — round trip,
    /// jitter, loss, resends, bytes each way and the snapshot's size — with
    /// a graph of the round trip and the snapshot. Solo has no connection to
    /// report on, so it has neither. The "audio" section is there when the
    /// run plays sound — never headless, where there is no [`Audio`] for it
    /// to report on.
    fn debug_sections(&self, panel: &mut crcbl::ui::DebugPanel) {
        panel.add(&self.stats);
        panel.add(&self.paths);
        panel.add(&self.desk);
        panel.add(&self.dev_camera);
        if let Some(audio) = &self.audio {
            panel.add(audio);
        }
        #[cfg(not(target_arch = "wasm32"))]
        if let Some(joining) = &self.joining {
            panel.add(joining.lan());
            panel.add(joining.lan().netgraph());
        } else {
            if let Some(lan) = self.game.lan_section() {
                panel.add(lan);
            }
            if let Some(netgraph) = self.game.netgraph() {
                panel.add(netgraph);
            }
        }
    }

    fn exiting(&mut self, exit: ExitReason) {
        self.save_on_close(exit);
    }

    /// **Towers' one save path**: the run, between waves, into this run's
    /// vault through [`crate::save::write_run`] — for `S`, the console's
    /// `save`, the autosave at a wave's end and the close alike. Refused
    /// while the lobby or a joining panel is up, because the game under it
    /// never ticked and would write a fresh run over the saved one.
    fn save(&mut self, request: SaveRequest) -> Result<Saved, SaveFailure> {
        if self.in_front() {
            return Err(SaveFailure::Refused(NOTHING_PLAYED.to_owned()));
        }
        let checkpoint = self.game.checkpoint();
        self.desk.take(request, |request| {
            crate::save::write_run(&self.vault, checkpoint, request)
        })
    }

    /// The mixer the console's `[engine.audio]` keys move — refused, as the
    /// default refuses, on a headless run, which has none.
    fn set_bus_gain(
        &mut self,
        bus: crcbl::audio::mixer::Bus,
        gain: f32,
    ) -> Result<(), crcbl::settings::Unsupported> {
        let audio = self.audio.as_ref().ok_or(crcbl::settings::Unsupported)?;
        audio.set_bus_gain(bus, gain);
        Ok(())
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
             {} kill(s) and {} leak(s), {} tower(s) built ({}) with \
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
            built_in_the_summary(&summary.built_by_kind),
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
    context: |options| options.map.clone(),
    assemble: |booted, options| assemble(booted, options),
);

// ---- tests -------------------------------------------------------------------

#[cfg(test)]
mod audio_tests;
#[cfg(test)]
mod build_menu_tests;
#[cfg(test)]
mod dev_camera_tests;
#[cfg(test)]
mod kind_tests;
#[cfg(test)]
mod save_tests;

#[cfg(test)]
mod tests {
    use super::*;
    use crcbl::args::Common;
    use crcbl::engine::ExitReason;
    use crcbl::shell::{HeadlessShell, ShellBackend as Backend};
    use crcbl_sample_test::{headless_common, ui_text};

    pub(super) fn scripted(options: &Options) -> Loop<HeadlessShell> {
        with_shell(Box::new(HeadlessShell::new()), options).expect("headless always starts")
    }

    /// A headless run of `frames` frames on the null backend.
    pub(super) fn headless(frames: u64) -> Options {
        headless_with(frames, |_| {})
    }

    /// …with one knob turned.
    pub(super) fn headless_with(frames: u64, tweak: impl FnOnce(&mut Common)) -> Options {
        let mut common = headless_common(crate::game::DEFAULT_TICK_HZ, frames);
        tweak(&mut common);
        Options {
            common,
            ..Options::default()
        }
    }

    /// Runs `count` frames.
    pub(super) fn frames(engine: &mut Loop<HeadlessShell>, count: usize) {
        for _ in 0..count {
            engine.frame().expect("a frame");
        }
    }

    /// The digit that picks the slow tower — the third row of
    /// [`tower::ALL`], so the third of [`KIND_KEYS`].
    const KEY_SLOW: KeyCode = KIND_KEYS[2];

    /// Presses and releases one key, then runs a frame so the tick sees it.
    pub(super) fn tap(engine: &mut Loop<HeadlessShell>, key: KeyCode) {
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

    /// **A map other than the committed one reaches the stage, the cursor and
    /// the renderer.**
    ///
    /// The end of the `--scene` path, and the only place it is a *run* rather
    /// than a parse: `crate::args` proves a directory reaches [`Options::map`],
    /// and this proves that field is what the server counts plots against, what
    /// the cursor wraps at, and what `Gpu::from_context` makes resident. The
    /// renderer's half is read off the pools it placed: a two-plot field has
    /// two tower, bolt and burst slots where the committed one has five, so a
    /// renderer still built from the committed field is red here — as is a
    /// stage still on it, which would count five plots.
    ///
    /// **The picture is not what this reads.** Nothing here would change if the
    /// right pools were placed through the wrong mesh slots; the mesh slots are
    /// `crate::map`'s own tests.
    #[test]
    fn a_map_other_than_the_committed_one_is_the_one_the_run_plays() {
        use crate::map::Map;
        use crate::scene::Plot;

        let plot = |label: &str, z: f64| Plot {
            label: label.to_string(),
            position: [0.0, 0.0, z],
        };
        let map = Map::new(
            vec![
                crcbl::math::DVec3::new(-10.0, 0.0, 0.0),
                crcbl::math::DVec3::new(10.0, 0.0, 0.0),
            ],
            vec![plot("north", -3.0), plot("south", 3.0)],
        )
        .expect("a straight lane with a plot either side is a map");
        let mut engine = scripted(&Options {
            map,
            ..headless(400)
        });
        frames(&mut engine, 4);

        // Build on the second plot, which is also the last: the cursor wraps
        // at the map's own length, so one step left from the first lands there.
        tap(&mut engine, KeyCode::ArrowLeft);
        assert_eq!(
            engine.game().selected(),
            1,
            "the cursor did not wrap at two"
        );
        tap(&mut engine, KeyCode::KeyB);
        frames(&mut engine, 2);

        let stats = engine.game().game().stats();
        assert_eq!(stats.plots, 2, "the stage is not on the two-plot field");
        assert_eq!(stats.towers, 1, "the build on the second plot was refused");
        assert_eq!(stats.refused, 0);
        assert!(
            engine.game().page().commands > 0,
            "the run presented frames with nothing on them",
        );

        let field = engine.gpu().field();
        assert_eq!(
            (field.plots(), field.bolt_slots(), field.burst_slots()),
            (2, 2, 2),
            "the renderer did not make the two-plot field resident",
        );
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
            (engine.game().game().map().plots().len() - 1) as u8,
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

    /// **A refused command is shown to the player who pressed it, for a
    /// while.** `U` on an empty plot is refused by the server, and the page
    /// names why; [`NOTICE_FOR`] later the line is gone.
    #[test]
    fn a_refused_command_is_shown_to_the_player_for_a_while() {
        let mut engine = scripted(&headless(1000));
        frames(&mut engine, 4);
        let line = format!("REFUSED: {}", crate::game::Refusal::NoTower.label());
        assert!(!ui_text(engine.gpu().draw_list()).contains(&line));

        tap(&mut engine, KeyCode::KeyU);
        assert_eq!(engine.game().game().stats().refused, 1);
        assert!(
            ui_text(engine.gpu().draw_list()).contains(&line),
            "the refusal is not on the page: {:?}",
            ui_text(engine.gpu().draw_list())
        );

        let past = NOTICE_FOR.as_nanos() / crcbl::engine::HEADLESS_FRAME_STEP.as_nanos() + 2;
        frames(
            &mut engine,
            usize::try_from(past).expect("a few hundred frames"),
        );
        assert!(
            !ui_text(engine.gpu().draw_list()).contains(&line),
            "the refusal outstayed its time"
        );
    }

    /// **The pause menu's `RESTART` and an `R` on one tick are one restart.**
    ///
    /// Both reach the stage through [`Controls::restart`], and the menu's is
    /// latched for the next tick. A latch read only when the key was not
    /// pressed outlived that tick and threw the fresh run away again on the
    /// one after — so the run counter is the observable: two here, not three.
    /// The tower built first says the one restart that did happen happened.
    #[test]
    fn a_menu_restart_and_an_r_on_one_tick_restart_the_run_once() {
        let mut engine = scripted(&headless(400));
        frames(&mut engine, 4);
        tap(&mut engine, KeyCode::KeyB);
        assert_eq!(engine.game().game().stats().towers, 1);

        let window = engine.window();
        engine
            .shell_mut()
            .key_press(window, KeyCode::KeyR)
            .expect("the window is live");
        engine.game_mut().apply(MenuAction::Restart);
        frames(&mut engine, 1);
        engine
            .shell_mut()
            .key_release(window, KeyCode::KeyR)
            .expect("the window is live");
        frames(&mut engine, 8);

        let stats = engine.game().game().stats();
        assert_eq!(stats.runs, 2, "one tick's restart ran {} runs", stats.runs);
        assert_eq!(stats.towers, 0, "the tower survived the restart");
        assert_eq!(stats.gold, crate::wave::STARTING_GOLD);
        assert_eq!(stats.outcome, Outcome::Playing);
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

    /// A loop opened on a lobby that browses `announcer` — or nothing — and
    /// hosts on loopback: `Options` built in code never opens one, and the
    /// lobby a parsed command line opens queries the broadcast address.
    #[cfg(not(target_arch = "wasm32"))]
    pub(super) fn in_a_lobby(announcer: Option<std::net::SocketAddr>) -> Loop<HeadlessShell> {
        in_a_lobby_timing_out(announcer, crate::lan::JOIN_TIMEOUT)
    }

    /// …whose joins give a host `timeout` to send its map.
    #[cfg(not(target_arch = "wasm32"))]
    fn in_a_lobby_timing_out(
        announcer: Option<std::net::SocketAddr>,
        timeout: std::time::Duration,
    ) -> Loop<HeadlessShell> {
        use crate::lan::tests::{loopback_browser, on_loopback};

        let mut engine = scripted(&headless(4000));
        engine.game_mut().lobby = Some(
            crate::lobby::Lobby::new(
                crate::lan::SESSION,
                Ok(crate::lan::tests::next_player()),
                announcer
                    .map(loopback_browser)
                    .ok_or_else(|| "NOT LOOKING".to_string()),
                on_loopback(),
                crate::game::DEFAULT_TICK_HZ,
            )
            .with_join_timeout(timeout),
        );
        engine
    }

    /// The lobby's lines under its title, while it is the panel up.
    #[cfg(not(target_arch = "wasm32"))]
    fn lobby_lines(engine: &Loop<HeadlessShell>) -> Vec<crcbl::ui::menu::Caption> {
        engine
            .menus()
            .current()
            .filter(|menu| menu.title == crate::lobby::TITLE)
            .map(|menu| menu.subtitle.clone())
            .unwrap_or_default()
    }

    /// The id of the lobby row the keyboard is on.
    #[cfg(not(target_arch = "wasm32"))]
    pub(super) fn lobby_row(engine: &Loop<HeadlessShell>) -> Option<crcbl::ui::WidgetId> {
        let menu = engine.menus().current()?;
        if menu.title != crate::lobby::TITLE {
            return None;
        }
        menu.selected_item().map(|item| item.id)
    }

    /// A LAN host on loopback with a player of its own, and where a joiner
    /// reaches it.
    #[cfg(not(target_arch = "wasm32"))]
    pub(super) fn loopback_host() -> (Game, std::net::SocketAddr) {
        loopback_host_on(&crate::map::Map::built_in())
    }

    /// …playing `map`.
    #[cfg(not(target_arch = "wasm32"))]
    fn loopback_host_on(map: &crate::map::Map) -> (Game, std::net::SocketAddr) {
        let host = Game::host(
            crate::game::DEFAULT_TICK_HZ,
            map,
            crate::lan::tests::on_loopback(),
            None,
            crate::lan::tests::next_player(),
        )
        .expect("loopback UDP must be available to these tests");
        let port = host.lan_host().expect("a host").game_port();
        (host, (std::net::Ipv4Addr::LOCALHOST, port).into())
    }

    /// Where `engine`'s game is a client of, if it is one.
    #[cfg(not(target_arch = "wasm32"))]
    pub(super) fn joined(engine: &Loop<HeadlessShell>) -> Option<std::net::SocketAddr> {
        engine
            .game()
            .game()
            .lan_client()
            .and_then(crcbl::lan::LanClient::host)
    }

    /// Where `engine`'s join under way is going, while one is.
    #[cfg(not(target_arch = "wasm32"))]
    fn joining(engine: &Loop<HeadlessShell>) -> Option<std::net::SocketAddr> {
        engine
            .game()
            .joining
            .as_ref()
            .and_then(|joining| joining.lan().host())
    }

    /// Types `address` into the lobby's connect row and presses Enter.
    #[cfg(not(target_arch = "wasm32"))]
    fn connect_to(engine: &mut Loop<HeadlessShell>, address: std::net::SocketAddr) {
        let window = engine.window();
        engine
            .shell_mut()
            .commit_text(window, &address.to_string())
            .expect("the window is live");
        frames(engine, 2);
        tap(engine, KeyCode::Enter);
    }

    /// **The lobby's keys pick a host it heard, and the game joins it — on
    /// the host's map, never on its own.** A host on loopback, on
    /// [`another_map`](crate::lan::tests::another_map)'s two plots where
    /// this process's own field has five, announces; its row appears under
    /// solo and host; Down twice and Enter start a join to it, and the lobby
    /// stays up saying so until the host's map is in. From the first frame
    /// the joined game is up the GPU's field is the host's — two plots — and
    /// the game's map is the host's; `LEFT` wraps the cursor at the host's
    /// plot count and `B` builds on the host's second plot, which the host
    /// builds and the joiner draws. Until the join, the solo game under the
    /// lobby never ticked, while the loop did.
    #[cfg(not(target_arch = "wasm32"))]
    #[test]
    fn the_lobbys_keys_pick_a_host_it_heard_and_the_game_joins_it() {
        use crate::lan::tests::{FRAME, MAX_FRAMES, PAUSE, another_map};
        use crate::menu::FIRST_LISTED_ID;

        let map = another_map();
        let (mut host, address) = loopback_host_on(&map);
        let announcer = host
            .lan_host()
            .and_then(crcbl::lan::LanHost::announcer_addr)
            .expect("the host announces");
        let mut engine = in_a_lobby(Some(announcer));

        let listed = |engine: &Loop<HeadlessShell>| {
            engine
                .menus()
                .current()
                .is_some_and(|menu| menu.items().iter().any(|item| item.id == FIRST_LISTED_ID))
        };
        for _ in 0..MAX_FRAMES {
            if listed(&engine) {
                break;
            }
            host.tick();
            host.frame(FRAME);
            frames(&mut engine, 1);
            std::thread::sleep(PAUSE);
        }
        assert!(listed(&engine), "the lobby never listed the host");
        assert!(engine.ticks() > 0, "the loop ran no tick");
        assert_eq!(
            engine.game().game().ticks_run(),
            0,
            "the game under the lobby ticked"
        );

        tap(&mut engine, KeyCode::ArrowDown);
        tap(&mut engine, KeyCode::ArrowDown);
        assert_eq!(lobby_row(&engine), Some(FIRST_LISTED_ID));
        tap(&mut engine, KeyCode::Enter);
        assert_eq!(
            joining(&engine),
            Some(address),
            "no join to the host picked"
        );
        assert!(
            engine.game().in_the_lobby(),
            "the lobby went before the map came"
        );
        let waiting = format!("JOINING {address}");
        assert!(
            lobby_lines(&engine).iter().any(|line| line.text == waiting),
            "{:?}",
            lobby_lines(&engine)
        );

        let plots = map.plots().len();
        let mut frames_on_the_hosts_map = 0;
        for _ in 0..MAX_FRAMES {
            if joined(&engine).is_some() && engine.game().game().stats().ticks > 0 {
                break;
            }
            host.tick();
            host.frame(FRAME);
            frames(&mut engine, 1);
            if joined(&engine).is_some() {
                // Every frame drawn of the joined game is on the host's map.
                assert_eq!(engine.game().game().map(), &map);
                assert_eq!(
                    engine.gpu().field().plots(),
                    plots,
                    "a frame on a stale map"
                );
                frames_on_the_hosts_map += 1;
            }
            std::thread::sleep(PAUSE);
        }
        assert!(
            engine.game().game().stats().ticks > 0,
            "the joiner never had the host's field"
        );
        assert!(frames_on_the_hosts_map > 0);
        assert_eq!(joined(&engine), Some(address));
        assert!(
            !engine.game().in_the_lobby(),
            "the map came and the lobby stayed"
        );
        assert!(!engine.menus().is_showing(), "a panel is still up");

        tap(&mut engine, KeyCode::ArrowLeft);
        assert_eq!(
            usize::from(engine.game().selected()),
            plots - 1,
            "the cursor did not wrap at the host's plot count"
        );
        tap(&mut engine, KeyCode::KeyB);
        for _ in 0..MAX_FRAMES {
            if engine.game().game().render_state().towers[plots - 1].is_some() {
                break;
            }
            host.tick();
            host.frame(FRAME);
            frames(&mut engine, 1);
            std::thread::sleep(PAUSE);
        }
        assert!(
            host.render_state().towers[plots - 1].is_some(),
            "the host did not build on its own last plot"
        );
        assert!(
            engine.game().game().render_state().towers[plots - 1].is_some(),
            "the joiner did not draw the tower it asked for"
        );
        assert_eq!(host.stats().refused, 0);
    }

    /// **A join the host refuses returns the player to the lobby, saying
    /// why** — here a host of the protocol before the map crossed the wire,
    /// reached by address. The lobby never went: it says the join failed and
    /// names the refusal, nothing is joining any more, and the solo game under
    /// it has still not ticked.
    #[cfg(not(target_arch = "wasm32"))]
    #[test]
    fn a_refused_join_returns_to_the_lobby_saying_why() {
        use crate::lan::tests::{FRAME, MAX_FRAMES, PAUSE, on_loopback};

        let mut old = crcbl::lan::LanHost::open(
            crcbl::lan::LanGame {
                compatibility: crcbl::net::ProtocolCompatibility {
                    protocol_version: crate::lan::SESSION.compatibility.protocol_version - 1,
                    ..crate::lan::SESSION.compatibility
                },
                ..crate::lan::SESSION
            },
            on_loopback(),
            crcbl::ecs::World::new(),
            crate::game::DEFAULT_TICK_HZ,
        )
        .expect("loopback UDP must be available to these tests");
        let address = (std::net::Ipv4Addr::LOCALHOST, old.game_port()).into();
        let mut engine = in_a_lobby(None);
        frames(&mut engine, 2);
        connect_to(&mut engine, address);
        assert_eq!(joining(&engine), Some(address));

        let failed = |engine: &Loop<HeadlessShell>| {
            lobby_lines(engine).into_iter().find(|line| {
                line.tone == crcbl::ui::menu::CaptionTone::Warning
                    && line.text.starts_with("JOIN FAILED:")
            })
        };
        let mut now = std::time::Duration::ZERO;
        for _ in 0..MAX_FRAMES {
            if failed(&engine).is_some() {
                break;
            }
            now += FRAME;
            old.frame(now);
            frames(&mut engine, 1);
            std::thread::sleep(PAUSE);
        }
        let line = failed(&engine).expect("the lobby never said the join failed");
        assert!(line.text.contains("refused the join"), "{}", line.text);
        assert!(
            line.text.contains("protocol version mismatch"),
            "{}",
            line.text
        );
        assert!(engine.game().in_the_lobby());
        assert!(!engine.game().is_joining());
        assert_eq!(engine.game().game().ticks_run(), 0);
        assert!(engine.game().game().lan_client().is_none());
    }

    /// **A join nobody answers returns the player to the lobby, saying why**,
    /// once the join's timeout has passed — shortened here to a handful of
    /// frames, on the loop's own clock. The address is a socket that takes
    /// the hello and never answers.
    #[cfg(not(target_arch = "wasm32"))]
    #[test]
    fn a_join_nobody_answers_returns_to_the_lobby_saying_why() {
        const TIMEOUT_FRAMES: u32 = 6;

        let silent =
            std::net::UdpSocket::bind(crate::lan::tests::loopback()).expect("loopback UDP");
        let address = silent.local_addr().expect("bound");
        let mut engine =
            in_a_lobby_timing_out(None, crcbl::engine::HEADLESS_FRAME_STEP * TIMEOUT_FRAMES);
        frames(&mut engine, 2);
        connect_to(&mut engine, address);
        assert_eq!(joining(&engine), Some(address));
        frames(&mut engine, 4 * TIMEOUT_FRAMES as usize);

        let expected = format!("JOIN FAILED: no answer from {address}");
        assert!(
            lobby_lines(&engine)
                .iter()
                .any(|line| line.text.starts_with(&expected)),
            "{:?}",
            lobby_lines(&engine)
        );
        assert!(engine.game().in_the_lobby());
        assert!(!engine.game().is_joining());
        assert_eq!(engine.game().game().ticks_run(), 0);
    }

    /// **A join the command line asked for waits under the joining panel and
    /// then plays the host's map.** `--join` to a host on another map: the
    /// panel says where, the idle game under it does not tick, and once the
    /// map is in the panel is gone and the game and the GPU's field are the
    /// host's.
    #[cfg(not(target_arch = "wasm32"))]
    #[test]
    fn a_command_line_join_waits_under_its_panel_and_plays_the_hosts_map() {
        use crate::lan::tests::{FRAME, MAX_FRAMES, PAUSE, another_map};

        let map = another_map();
        let (mut host, address) = loopback_host_on(&map);
        let mut engine = scripted(&Options {
            lan: crcbl::lan::LanMode::Join(address),
            ..headless(4000)
        });
        frames(&mut engine, 1);
        let panel = engine.menus().current().expect("the joining panel");
        assert_eq!(panel.title, crate::lobby::JOINING_TITLE);
        assert_eq!(panel.subtitle[0].text, format!("JOINING {address}"));
        assert_eq!(engine.game().game().ticks_run(), 0);

        for _ in 0..MAX_FRAMES {
            if joined(&engine).is_some() {
                break;
            }
            host.tick();
            host.frame(FRAME);
            frames(&mut engine, 1);
            std::thread::sleep(PAUSE);
        }
        assert_eq!(
            joined(&engine),
            Some(address),
            "the command line's join never played"
        );
        assert_eq!(engine.game().game().map(), &map);
        assert_eq!(engine.gpu().field().plots(), map.plots().len());
        assert!(
            !engine.menus().is_showing(),
            "the joining panel is still up"
        );
    }

    /// **A joiner's panel carries the netgraph beside the "lan" section, and
    /// the host's shows a row per peer.** A `--join` to a host on loopback,
    /// with the overlay on: once the joiner plays, its panel's sections end
    /// "lan", "net", and the "net" section's one link is to the host with a
    /// round trip measured. The host's netgraph lists its own player — an
    /// in-memory link, which measures nothing — and the joiner, measured.
    #[cfg(not(target_arch = "wasm32"))]
    #[test]
    fn a_joiner_and_its_host_show_the_netgraph() {
        use crate::lan::tests::{FRAME, MAX_FRAMES, PAUSE};

        let (mut host, address) = loopback_host();
        let mut engine = scripted(&Options {
            lan: crcbl::lan::LanMode::Join(address),
            ..headless_with(4000, |common| common.debug_overlay = Some(true))
        });
        let measured = |netgraph: &crcbl::lan::netgraph::Netgraph| {
            netgraph
                .links()
                .iter()
                .filter(|link| link.reading.stats.is_some_and(|stats| stats.rtt.is_some()))
                .count()
        };
        for _ in 0..MAX_FRAMES {
            let joiner = engine.game().game().netgraph().map_or(0, measured);
            let hosted = host.netgraph().map_or(0, measured);
            if joined(&engine).is_some() && joiner == 1 && hosted == 1 {
                break;
            }
            host.tick();
            host.frame(FRAME);
            frames(&mut engine, 1);
            std::thread::sleep(PAUSE);
        }
        assert_eq!(joined(&engine), Some(address), "the join never played");
        frames(&mut engine, 1);

        let titles: Vec<&str> = engine
            .debug()
            .panel
            .sections()
            .iter()
            .map(crcbl::ui::DebugSection::title)
            .collect();
        assert!(titles.ends_with(&["lan", "net"]), "{titles:?}");
        let net = engine.debug().panel.sections().last().expect("a section");
        let links: Vec<&str> = net
            .rows()
            .iter()
            .map(|row| row.label.as_str())
            .filter(|label| *label == "host" || label.starts_with("peer "))
            .collect();
        assert_eq!(links, ["host"], "a joiner shows its one link");
        assert_eq!(
            engine.game().game().netgraph().map(measured),
            Some(1),
            "the link's round trip is measured"
        );

        let hosted = host.netgraph().expect("a host's netgraph");
        assert_eq!(hosted.links().len(), 2, "its own player and the joiner");
        assert_eq!(measured(hosted), 1, "only the joiner's link is measured");
    }

    /// A dedicated server on loopback playing `map`, and where a joiner
    /// reaches it: a host whose sessions a test can end.
    #[cfg(not(target_arch = "wasm32"))]
    fn loopback_server(map: &crate::map::Map) -> (crate::lan::serve::Server, std::net::SocketAddr) {
        let server = crate::lan::serve::Server::open(
            crate::lan::tests::on_loopback(),
            map,
            crate::game::DEFAULT_TICK_HZ,
            None,
            crate::save::Vault::nowhere(),
        )
        .expect("loopback UDP must be available to these tests");
        let port = server.lan().game_port();
        (server, (std::net::Ipv4Addr::LOCALHOST, port).into())
    }

    /// Serves `server` and runs `engine` a frame at a time, on `now`, until
    /// `done` holds — failing, naming `what`, if it never does.
    #[cfg(not(target_arch = "wasm32"))]
    fn serve_until(
        server: &mut crate::lan::serve::Server,
        engine: &mut Loop<HeadlessShell>,
        now: &mut std::time::Duration,
        what: &str,
        done: impl Fn(&Loop<HeadlessShell>) -> bool,
    ) {
        use crate::lan::tests::{FRAME, MAX_FRAMES, PAUSE};

        for _ in 0..MAX_FRAMES {
            if done(engine) {
                return;
            }
            *now += FRAME;
            server.frame(*now);
            frames(engine, 1);
            std::thread::sleep(PAUSE);
        }
        panic!("no {what} within {MAX_FRAMES} frames");
    }

    /// **A joined session that ends returns the player to the lobby, saying
    /// how.** A join picked in the lobby plays the server's two-plot map;
    /// then the host leaves. The player is back in the lobby, which names
    /// the end as a warning, over the idle solo game that was under it —
    /// this process's own five-plot field, in the game and on the GPU, never
    /// ticked — and the lobby's browser still hears the server.
    #[cfg(not(target_arch = "wasm32"))]
    #[test]
    fn a_joined_session_that_ends_returns_to_the_lobby_saying_how() {
        use crate::lan::tests::another_map;
        use crate::menu::FIRST_LISTED_ID;

        let (mut server, address) = loopback_server(&another_map());
        let announcer = server.lan().announcer_addr().expect("the server announces");
        let mut engine = in_a_lobby(Some(announcer));
        let mut now = std::time::Duration::ZERO;
        frames(&mut engine, 2);
        connect_to(&mut engine, address);
        serve_until(
            &mut server,
            &mut engine,
            &mut now,
            "the joined game",
            |engine| joined(engine).is_some() && engine.game().game().stats().ticks > 0,
        );
        assert_eq!(engine.gpu().field().plots(), another_map().plots().len());

        server
            .lan_mut()
            .host_mut()
            .shutdown(crcbl::net::SessionEndReason::HOST_LEFT);
        serve_until(
            &mut server,
            &mut engine,
            &mut now,
            "the way back",
            |engine| engine.game().in_the_lobby(),
        );
        frames(&mut engine, 1);
        let ended = lobby_lines(&engine)
            .into_iter()
            .find(|line| line.text.starts_with("SESSION ENDED:"))
            .expect("the lobby does not say the session ended");
        assert_eq!(ended.text, "SESSION ENDED: the host left");
        assert_eq!(ended.tone, crcbl::ui::menu::CaptionTone::Warning);
        let game = engine.game().game();
        assert!(game.lan_client().is_none(), "still the joined game");
        assert_eq!(game.map(), &crate::map::Map::built_in());
        assert_eq!(
            game.ticks_run(),
            0,
            "not the idle solo game from under the lobby"
        );
        assert_eq!(
            engine.gpu().field().plots(),
            crate::map::Map::built_in().plots().len(),
            "the GPU still draws the host's field"
        );
        assert!(!engine.game().is_joining());
        serve_until(
            &mut server,
            &mut engine,
            &mut now,
            "the server's row",
            |engine| {
                engine
                    .menus()
                    .current()
                    .is_some_and(|menu| menu.items().iter().any(|item| item.id == FIRST_LISTED_ID))
            },
        );
    }

    /// **A command-line join whose session ends says so on its panel**, as a
    /// failed join does: `--join`, then the server shuts down, and the
    /// joining panel names the end while the stopped game under it ticks no
    /// more.
    #[cfg(not(target_arch = "wasm32"))]
    #[test]
    fn a_command_line_session_that_ends_says_so_on_its_panel() {
        let (mut server, address) = loopback_server(&crate::map::Map::built_in());
        let mut engine = scripted(&Options {
            lan: crcbl::lan::LanMode::Join(address),
            ..headless(4000)
        });
        let mut now = std::time::Duration::ZERO;
        serve_until(
            &mut server,
            &mut engine,
            &mut now,
            "the joined game",
            |engine| joined(engine).is_some() && engine.game().game().stats().ticks > 0,
        );

        server
            .lan_mut()
            .host_mut()
            .shutdown(crcbl::net::SessionEndReason::SHUTTING_DOWN);
        serve_until(&mut server, &mut engine, &mut now, "the panel", |engine| {
            engine.menus().is_showing()
        });
        let panel = engine.menus().current().expect("the joining panel");
        assert_eq!(panel.title, crate::lobby::JOINING_TITLE);
        assert_eq!(
            panel.subtitle[0].text,
            "SESSION ENDED: the server shut down"
        );
        assert_eq!(
            panel.subtitle[0].tone,
            crcbl::ui::menu::CaptionTone::Warning
        );
        let ticks = engine.game().game().ticks_run();
        frames(&mut engine, 4);
        assert_eq!(
            engine.game().game().ticks_run(),
            ticks,
            "the ended game ticked"
        );
    }

    /// **Typing in the lobby fills the connect row and Enter joins it** — or,
    /// for text that is not an `IP:PORT`, says so under the title and starts
    /// nothing. The text arrives as the window system's commits, through the
    /// loop, and Backspace takes it off a character a press.
    #[cfg(not(target_arch = "wasm32"))]
    #[test]
    fn typing_in_the_lobby_fills_the_connect_row_and_enter_joins_it() {
        use crate::menu::CONNECT_ID;

        let mut engine = in_a_lobby(None);
        frames(&mut engine, 2);
        let window = engine.window();
        let bad = "nowhere";
        engine
            .shell_mut()
            .commit_text(window, bad)
            .expect("the window is live");
        frames(&mut engine, 2);
        assert_eq!(lobby_row(&engine), Some(CONNECT_ID), "typing moved nothing");
        tap(&mut engine, KeyCode::Enter);
        assert!(engine.game().in_the_lobby(), "a bad address left the lobby");
        let warned = engine
            .menus()
            .current()
            .expect("the lobby")
            .subtitle
            .clone();
        let refusal = format!("NOT AN IP:PORT: {bad:?}");
        assert!(warned.iter().any(|line| line.text == refusal), "{warned:?}");

        for _ in 0..bad.len() {
            tap(&mut engine, KeyCode::Backspace);
        }
        let (_host, address) = loopback_host();
        engine
            .shell_mut()
            .commit_text(window, &address.to_string())
            .expect("the window is live");
        frames(&mut engine, 2);
        let row = engine
            .menus()
            .current()
            .and_then(|menu| menu.items().iter().find(|item| item.id == CONNECT_ID))
            .map(|item| item.hint.clone());
        assert_eq!(
            row,
            Some(address.to_string()),
            "the row shows exactly what was typed"
        );
        tap(&mut engine, KeyCode::Enter);
        assert_eq!(joining(&engine), Some(address));
    }

    /// **Solo from the lobby starts the run that was under it**, and a run
    /// the command line chose has no lobby at all — the control, on the same
    /// field.
    #[cfg(not(target_arch = "wasm32"))]
    #[test]
    fn solo_from_the_lobby_starts_the_run_under_it_and_a_chosen_run_has_no_lobby() {
        let mut engine = in_a_lobby(None);
        frames(&mut engine, 4);
        assert_eq!(lobby_row(&engine), Some(crate::menu::SOLO_ID));
        assert_eq!(engine.game().game().ticks_run(), 0);
        tap(&mut engine, KeyCode::Enter);
        assert!(!engine.game().in_the_lobby());
        frames(&mut engine, 4);
        assert!(engine.game().game().ticks_run() > 0, "solo did not start");
        assert!(engine.game().game().lan_client().is_none());
        assert!(engine.game().game().lan_host().is_none());

        let mut chosen = scripted(&headless(400));
        frames(&mut chosen, 4);
        assert!(!chosen.game().in_the_lobby());
        assert!(!chosen.menus().is_showing());
        assert!(chosen.game().game().ticks_run() > 0);
    }

    /// **The overlay is composed of exactly the modules towers has**, and the
    /// stage's own rows reached the draw list with numbers in them.
    ///
    /// No network module and no audio one: this sample has neither system, and
    /// a panel that showed a row for either would be the overlay inventing
    /// state rather than reporting it. The camera's is there in every mode,
    /// saying which camera the frame is drawn from, and so is the save path's
    /// storage section.
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
            &[
                "frame", "gpu", "counters", "towers", "paths", "storage", "camera",
            ]
        } else {
            &["frame", "counters", "towers", "paths", "storage", "camera"]
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
