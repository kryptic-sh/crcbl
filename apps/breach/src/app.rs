//! Breach's start-up, its controls, and the [`HostedGame`] methods the engine's
//! loop calls.
//!
//! # There is no loop in this file
//!
//! ```text
//! Loop::frame()                     ← the engine's
//!   pump, input, menu, pause, resize
//!     ─────────────────────────────→ Breach::key_event      (queued, not applied)
//!     ─────────────────────────────→ Breach::pointer_event  (the mouse look)
//!   run_ticks  ─────────────────────→ Breach::tick          (controls, then a tick)
//!   draw_list.clear()
//!     ─────────────────────────────→ Breach::draw           (view, plates, overlay)
//!     menu, debug overlay             ← the engine's
//!   gpu.frame()
//! ```
//!
//! What is left here is start-up, because a window's title is this sample's;
//! the action map, because a keyboard is not something [`crate::game`] should
//! know about; the view, because it is presentation; and the trait methods,
//! because they are what a hosted game is.
//!
//! # The view turns on the frame's clock, the player walks on the tick's
//!
//! [`Breach::tick`] sends the simulation what the player is holding down, the
//! trigger edge, and the two angles the view is at; [`Breach::draw`] turns the
//! view and points it out of wherever the tick left the player's eye. That
//! split is the seam `docs/plan/30-player-kit.md` draws — movement is a server
//! system, the camera is client presentation — and it is why a paused frame can
//! still be looked around from while the player does not move.
//!
//! The two angles crossing that seam are the whole of what the simulation knows
//! about the camera. [`crate::camera`] is where they become a walk direction
//! and a ray, and `crcbl-phys` never sees any of it.
//!
//! # Mouselook is asked for and not assumed
//!
//! [`Breach::pointer_mode`] answers [`PointerMode::Locked`] while the range is
//! being shot, and both halves of the mouse — the look and the trigger — are
//! bound to `at.is_none()` rather than to that request. See
//! [`Breach::pointer_event`]: a request is not a grant, and the loop declines
//! the lock outright on a shell that clears
//! `ShellCaps::has_mouselook`. On a shell that grants it the mouse turns the
//! view and its primary button pulls the trigger; on one that does not, the
//! arrows are the look and `ACTION_FIRE`'s key is the trigger, and the sample
//! plays with the keyboard alone rather than half-working. The arrows are
//! therefore a real second binding and not an accessibility afterthought.
//!
//! # `[HUD]` is logged here rather than in `crate::game`
//!
//! Every other line on it is the simulation's, and `apps/puppet` logs its
//! heartbeat from the tick for that reason. This one also names the three
//! selectors the frame is drawn through — rule 12 — and those are
//! [`crate::Paths`]', which the stage cannot see. Logging it here is what
//! puts both on one line at one cadence; `apps/quarry` does the same, and for
//! the same reason.

use crcbl::core::input::KeyCode;
use crcbl::engine::{
    Booted, Clock, ExitReason, FrameInfo, HostedGame, PointerUpdate, RunSummary, wait_for_configure,
};
use crcbl::input::{ActionDecl, ActionKind, ActionMap, Binding};
use crcbl::math::Vec3;
use crcbl::prelude::*;
use crcbl::shell::{DisplayMode, PointerMode, ShellBackend as Backend, WindowId};

use crate::camera::Eye;
use crate::game::{ArenaStats, Controls, Game, RenderState, Scene, Stats};
use crate::gpu::{Gpu, Paths};
use crate::menu::{MenuKind, Menus};
use crate::page::PageStats;

pub use crate::args::Options;

// ---- the controls --------------------------------------------------------------

/// Walk, relative to where the player is looking.
const ACTION_FORWARD: &str = "forward";
/// See [`ACTION_FORWARD`].
const ACTION_BACK: &str = "back";
/// See [`ACTION_FORWARD`].
const ACTION_LEFT: &str = "left";
/// See [`ACTION_FORWARD`].
const ACTION_RIGHT: &str = "right";
/// Turn the view, anticlockwise and clockwise. The keyboard's half of the look
/// — see the module docs for why a browser has only this half.
const ACTION_LOOK_LEFT: &str = "look-left";
/// See [`ACTION_LOOK_LEFT`].
const ACTION_LOOK_RIGHT: &str = "look-right";
/// Tilt it up and down. See [`ACTION_LOOK_LEFT`].
const ACTION_LOOK_UP: &str = "look-up";
/// See [`ACTION_LOOK_LEFT`].
const ACTION_LOOK_DOWN: &str = "look-down";
/// The trigger's **keyboard** half. Read as a press edge, not as a held state:
/// one pull is one shot, and slice 1's pistol is not an automatic weapon.
///
/// **The mouse's half is not in the action map**, and that is the point. A
/// click means two different things depending on whether the pointer is
/// captured: under the lock it is a trigger pull at the crosshair, and with a
/// visible cursor it is a click at a place on the page — the button that grabs
/// the lock in the first place, or a press on whatever the page put under the
/// mouse. An [`ActionMap`] binding cannot tell those apart, because it is not
/// told where the pointer is. [`Breach::pointer_event`] can, and gates the
/// mouse trigger on the same `at.is_none()` the look is gated on, so the two
/// halves of the mouse are bound under one rule stated once.
const ACTION_FIRE: &str = "fire";

/// The keyboard and the mouse this sample is played with.
///
/// Declared in one place so the bindings and the read-out below cannot name
/// different actions: a typo in either is an action that resolves to nothing,
/// and [`ActionMap`] answers `false` for an action nobody declared rather than
/// complaining.
fn action_map() -> ActionMap {
    let mut map = ActionMap::new();
    for (name, bindings) in [
        (ACTION_FORWARD, vec![Binding::Key(KeyCode::KeyW)]),
        (ACTION_BACK, vec![Binding::Key(KeyCode::KeyS)]),
        (ACTION_LEFT, vec![Binding::Key(KeyCode::KeyA)]),
        (ACTION_RIGHT, vec![Binding::Key(KeyCode::KeyD)]),
        (ACTION_LOOK_LEFT, vec![Binding::Key(KeyCode::ArrowLeft)]),
        (ACTION_LOOK_RIGHT, vec![Binding::Key(KeyCode::ArrowRight)]),
        (ACTION_LOOK_UP, vec![Binding::Key(KeyCode::ArrowUp)]),
        (ACTION_LOOK_DOWN, vec![Binding::Key(KeyCode::ArrowDown)]),
        (ACTION_FIRE, vec![Binding::Key(KeyCode::Space)]),
    ] {
        map.declare(ActionDecl {
            name: name.into(),
            kind: ActionKind::Button,
            bindings,
        });
    }
    map
}

/// What the input is asking the **simulation** for on the tick `actions` has
/// just begun, at the angles the view is currently at.
///
/// The four movement actions read the **held** state — walking is a thing that
/// happens for as long as a key is down — and the trigger reads the **edge**,
/// because a held trigger is one shot and not sixty a second. `clicked` is that
/// same edge arriving from the mouse instead, latched by
/// [`Breach::pointer_event`]; either source pulls the trigger, and a tick that
/// gets both still fires once because [`Controls::fire`] is one flag. The look actions
/// are deliberately absent: they are read in [`Breach::draw`], on the frame's
/// clock, because the view is not part of what the server owns.
fn controls(actions: &ActionMap, clicked: bool, yaw: f32, pitch: f32) -> Controls {
    Controls {
        forward: actions.button_held(ACTION_FORWARD),
        back: actions.button_held(ACTION_BACK),
        left: actions.button_held(ACTION_LEFT),
        right: actions.button_held(ACTION_RIGHT),
        fire: actions.just_pressed(ACTION_FIRE) || clicked,
        yaw,
        pitch,
    }
}

/// How far the view should turn this frame from held keys, given how long the
/// frame was: `(yaw, pitch)` in radians.
fn look_turn(actions: &ActionMap, seconds: f32) -> (f32, f32) {
    let axis = |positive: &str, negative: &str| {
        f32::from(i8::from(actions.button_held(positive)) - i8::from(actions.button_held(negative)))
    };
    (
        axis(ACTION_LOOK_RIGHT, ACTION_LOOK_LEFT) * crate::camera::TURN_RATE * seconds,
        axis(ACTION_LOOK_UP, ACTION_LOOK_DOWN) * crate::camera::TURN_RATE * seconds,
    )
}

/// The middle of the `[HUD]` line: whichever map's own fields.
///
/// Built as one string rather than branched into two `log::info!` calls, so the
/// line's shared half — the position, the score, the pilot and rule 12's three
/// selectors — has exactly one spelling and a browser reads the same fields off
/// both maps.
fn arena_fields(arena: &ArenaStats) -> String {
    match *arena {
        ArenaStats::Range {
            plates_down,
            mover_x,
        } => format!(
            "near: {}  mover: {mover_x:.2}",
            if plates_down[0] { "down" } else { "up" },
        ),
        ArenaStats::Practice {
            alive,
            seen,
            covered,
            health,
            downs,
            fired,
            taken,
            lead,
        } => format!(
            "bots: {alive}  seen: {seen}  covered: {covered}  hp: {health}  downs: {downs}  \
             fired: {fired}  taken: {taken}  botx: {:.2}  botz: {:.2}",
            lead.x, lead.z,
        ),
    }
}

// ---- summary -----------------------------------------------------------------

/// What a finished run reports.
///
/// [`PartialEq`] but not [`Eq`], unlike the 2D samples': the position is floats,
/// so two runs are compared by the numbers they produced and there is no total
/// order to claim.
#[derive(Clone, Debug, PartialEq)]
pub struct Summary {
    pub backend: Backend,
    pub frames: u64,
    pub ticks: u64,
    pub events: u64,
    pub extent: (u32, u32),
    pub exit: ExitReason,
    /// Whether the simulation was stopped when the run ended.
    pub paused: bool,
    /// The mode the window system actually had the window in, **not** the one
    /// the run last asked for.
    pub mode: DisplayMode,
    /// Which of the two maps the run was on.
    pub map: crate::map::MapChoice,
    /// Where the player's feet ended up, in metres.
    pub feet: [f64; 3],
    /// How many shots were fired over the run.
    pub shots: u64,
    /// How many of them hit a standing plate.
    pub hits: u64,
    /// Which selectors the frames were drawn through — rule 12's "says which it
    /// took", in the summary line as well as in the panel.
    pub paths: Paths,
    /// How many commands the last overlay drew. Zero would mean a run that
    /// presented frames with nothing on them, which is the one failure a
    /// headless smoke test could otherwise report as a pass.
    pub commands: usize,
}

// ---- errors ------------------------------------------------------------------

/// What can stop breach: the loop's own failures, plus this sample's.
pub type BreachError = crcbl::engine::LoopError<crate::game::GameError>;

// ---- the hosted game ---------------------------------------------------------

/// Breach, as the engine's loop hosts it.
#[derive(Debug)]
pub struct Breach {
    game: Game,
    /// The keyboard and the mouse, resolved into [`Controls`] once per tick.
    actions: ActionMap,
    /// Key events from the shell pump, replayed after `ActionMap::begin_tick`.
    ///
    /// The pump runs once per **frame** and the map's edge flags are per
    /// **tick**, and `begin_tick` clears those flags — so an event fed before
    /// it has its press edge erased. That matters more here than it does in
    /// `apps/puppet`: the trigger is read as an edge, so a shot fed at the
    /// wrong moment is a shot that never happened. Queueing here and replaying
    /// after is the order the map asks for, and it is what makes a frame that
    /// runs no ticks lossless.
    pending_keys: Vec<(KeyCode, bool)>,
    /// Whether the pointer is really captured, as the last frame that could
    /// tell it said.
    ///
    /// **`at.is_none()` on its own does not mean captured**, which is the trap
    /// this field exists to step around. [`PointerUpdate::at`] is `Some` only
    /// on a frame the pointer actually **moved**, so a click with a visible
    /// cursor that has been sitting still reports no position either — and
    /// reading that as a capture is a shot the visitor never asked for, on the
    /// commonest click there is. A *motion* carrying no position is a shape
    /// only a held lock produces, so that is what sets this, and a motion
    /// carrying one clears it.
    captured: bool,
    /// A trigger pull from the mouse, waiting for the next tick.
    ///
    /// [`Breach::pending_keys`]' counterpart, and latched for the same reason:
    /// the pump runs once per frame and the trigger is an edge, so a click on a
    /// frame that runs no ticks would otherwise be a shot that never happened.
    /// Cleared by the tick that spends it, so one click stays one shot however
    /// many ticks the frame runs.
    pending_fire: bool,
    /// The first-person view. **Presentation**: it never crosses the wire, and
    /// the only thing the simulation is told about it is its two angles.
    eye: Eye,
    /// Refilled from the simulation every frame.
    render_state: RenderState,
    /// The simulation's numbers, snapshotted in [`Breach::tick`].
    stats: Stats,
    /// What the last frame's overlay drew, from the same frame.
    page: PageStats,
    /// Whether the loop is stopped, as [`Breach::menu_kind`] was last told.
    ///
    /// Kept because [`Breach::pointer_mode`] is asked `&self` and the pause is
    /// the loop's state rather than this game's. `apps/lantern` keeps the same
    /// copy for the same reason.
    paused: bool,
    /// Which selectors this device drew through, read off the GPU bundle.
    ///
    /// Kept here rather than reached through `gpu` because
    /// [`HostedGame::debug_sections`] and [`HostedGame::summary`] are handed
    /// `&self` and no GPU at all.
    paths: Paths,
}

impl Breach {
    /// The `[HUD]` line, on the cadence every other sample uses.
    ///
    /// `web/tools/browser-e2e.mjs` reads five claims out of it, and each one is
    /// a number nothing on the JS side can move:
    ///
    /// * `px`, `py`, `pz` — where the player is. The gate holds a key and
    ///   requires `pz` to advance, then releases it and requires `pz` to stop,
    ///   which is the pair a demo that merely drifts cannot pass.
    /// * `yaw` — where they are looking, as
    ///   [`crate::camera::Eye::yaw`] reached the tick. The gate holds a look
    ///   key and requires it to take new values, which is the only thing on the
    ///   page that says the view is being turned rather than the picture merely
    ///   changing.
    /// * `shots` and `hits` — [`crate::game`]'s counters. The gate fires once
    ///   with a plate in the crosshair and requires both to rise, then turns
    ///   away and fires again and requires only `shots` to. The second is the
    ///   control for the first: a build that scored on every trigger pull
    ///   passes the hit and fails the miss.
    /// * `aim` — what the crosshair is on, which is what makes the two shots
    ///   above deliberate rather than lucky.
    /// * `near` — the nearest lane's plate, up or down. The observable a hit
    ///   has on the *range* rather than on the score.
    /// * `mover` — where [`crate::map::MOVER_LANE`]'s travelling plate is, which
    ///   is the one number here **nothing a player does can move**. It is what
    ///   the gate's generic "the demo advances under its own steam" check
    ///   reads, and a page whose loop had stopped would leave it standing
    ///   still.
    ///
    /// **`map` says which room the rest of the line is about**, and the fields
    /// after it are that map's. `near` and `mover` above are the range's; the
    /// practice map's are these, and the gate reads three pairs out of them:
    ///
    /// * `bots` — how many are on their feet, and `botx`/`botz`, where the
    ///   first one's feet are. That pair is a **bot's** position and not the
    ///   player's, which is what makes "the patrol walks under its own steam" a
    ///   claim about `crate::bots` rather than about the input path.
    /// * `seen` and `covered` — how many bots have the player in sight, and how
    ///   many are near enough to and cannot because something is in the way.
    ///   The second is the control for the first: a build that noticed
    ///   unconditionally reports a rising `seen` and a `covered` that never
    ///   leaves zero.
    /// * `hp`, `downs`, `fired` and `taken` — what the player has left, how many
    ///   times they have been put down, and the bots' shots against the ones
    ///   that arrived. `fired` above `taken` is a round cover stopped, which is
    ///   the control for `hp` ever falling at all.
    ///
    /// It also names the three selectors — see the module docs.
    fn log_heartbeat(&self) {
        if self.stats.ticks == 0
            || !self
                .stats
                .ticks
                .is_multiple_of(crate::game::HEARTBEAT_TICKS)
        {
            return;
        }
        let stats = &self.stats;
        crcbl::log::info!(
            "[HUD] tick: {}  px: {:.2}  py: {:.2}  pz: {:.2}  yaw: {:.3}  pitch: {:.3}  \
             shots: {}  hits: {}  acc: {}  aim: {}  map: {}  {}  ground: {}  \
             pilot: {}  geometry: {:?}  binding: {:?}  lighting: {:?}",
            stats.ticks,
            stats.position.x,
            stats.feet,
            stats.position.z,
            stats.aim.0,
            stats.aim.1,
            stats.shots,
            stats.hits,
            match stats.accuracy() {
                Some(percent) => format!("{percent:.0}%"),
                None => "--".to_string(),
            },
            stats.crosshair.label(),
            stats.arena.map().name(),
            arena_fields(&stats.arena),
            if stats.grounded { "yes" } else { "no" },
            if stats.warming_up { "range" } else { "player" },
            self.paths.geometry,
            self.paths.binding,
            self.paths.lighting,
        );
    }

    /// The simulation, for scripted tests and for an embedder that drives it.
    pub const fn game(&self) -> &Game {
        &self.game
    }

    /// Where the view is pointing, for this crate's own tests.
    pub const fn eye(&self) -> &Eye {
        &self.eye
    }

    /// What the last frame's overlay drew.
    pub const fn page(&self) -> &PageStats {
        &self.page
    }
}

/// The loop breach runs in.
///
/// A type alias, because the loop is the engine's. `S` is the shell type: the
/// native path builds `Loop<dyn Shell>`, and the tests build
/// `Loop<HeadlessShell>` so they can inject the events a compositor would send.
pub type Loop<S = dyn Shell> = crcbl::engine::Loop<S, Breach>;

/// Runs the full loop.
///
/// # Errors
///
/// [`BreachError`] if the shell, the GPU or the simulation's server failed.
/// Teardown runs on every path.
pub fn run(options: &Options) -> Result<Summary, BreachError> {
    crcbl::engine::drive(start(options)?)
}

/// Opens a shell, a window, a GPU and the simulation.
///
/// # Errors
///
/// [`BreachError`] if any of them refused.
pub fn start(options: &Options) -> Result<Loop, BreachError> {
    let shell = crcbl::engine::open_shell(options.common.headless)?;
    with_shell(shell, options)
}

/// Builds the loop on an already-open shell, blocking on both waits.
///
/// The browser cannot use this — a main thread may not sit in
/// [`wait_for_configure`] — and takes [`PendingLoop`] instead. What the two
/// share is everything after the waiting, which is `assemble` — private, because
/// a caller has no `Booted` to hand it.
///
/// # Errors
///
/// [`BreachError`] if the window never configured, the GPU would not open, or
/// the simulation's server could not be built.
pub fn with_shell<S: Shell + ?Sized>(
    mut shell: Box<S>,
    options: &Options,
) -> Result<Loop<S>, BreachError> {
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
        options.map,
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
/// [`Booted`] is what both bring-up paths hand over, so the simulation is built
/// and the loop assembled in one place rather than one per path — a second copy
/// is how the browser build would come to run a subtly different sample.
///
/// # Errors
///
/// [`BreachError`] if the simulation's server could not be built.
fn assemble<S: Shell + ?Sized>(
    booted: Booted<S, Gpu>,
    options: &Options,
) -> Result<Loop<S>, BreachError> {
    // `--screenshot`, armed before the first frame because the frame it names is
    // counted from this point. The flag forces `--headless` on, so the context
    // behind this is always an offscreen ring.
    //
    // The mutable binding lives inside the `cfg` rather than on the parameter: a
    // browser build arms nothing, so a `mut` in the signature would be one the
    // wasm32 target correctly reports as unused.
    #[cfg(not(target_arch = "wasm32"))]
    let booted = {
        let mut booted = booted;
        if let Some(request) = options.common.screenshot_request() {
            booted.gpu.context_mut().set_screenshot(request);
        }
        booted
    };
    let paths = booted.gpu.paths();
    let game = Game::new(options.common.tick_hz, options.map).map_err(BreachError::Game)?;
    Ok(Loop::new(
        booted,
        Breach {
            game,
            actions: action_map(),
            pending_keys: Vec::new(),
            captured: false,
            pending_fire: false,
            eye: Eye::default(),
            render_state: RenderState::default(),
            stats: Stats::default(),
            page: PageStats::default(),
            paused: false,
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
) -> Result<WindowId, BreachError> {
    Ok(crcbl::engine::open_window(
        shell,
        clock_source,
        &WindowDesc {
            title: "Breach",
            app_id: "sh.kryptic.crcbl.breach",
            size: crcbl::engine::requested_window_size(size),
            mode,
            ..WindowDesc::default()
        },
    )?)
}

/// Breach's half of the frame, and nothing else.
impl HostedGame for Breach {
    type Error = crate::game::GameError;
    type Gpu = Gpu;
    type MenuKind = MenuKind;
    /// Breach declares no menu action of its own — see [`crate::menu`].
    /// Uninhabited rather than a placeholder enum, so [`Breach::apply`] is a
    /// match on nothing and the compiler agrees there is no case to handle.
    type MenuAction = core::convert::Infallible;
    type Summary = Summary;

    const NAME: &'static str = "breach";

    fn menus() -> Menus {
        crate::menu::menus()
    }

    fn tick(&mut self, gpu: &mut Gpu, tick_dt: f64) {
        // `ActionMap` holds its timers in `f32`, which is the precision an
        // input edge is worth.
        #[allow(clippy::cast_possible_truncation)]
        self.actions.begin_tick(tick_dt as f32);
        for (key, pressed) in std::mem::take(&mut self.pending_keys) {
            self.actions.key_event(key, pressed);
        }
        // The angles go with the buttons: what the player asked for is
        // "forward" and "fire", and neither means anything beside the direction
        // they were looking when they asked.
        self.game.set_controls(controls(
            &self.actions,
            core::mem::take(&mut self.pending_fire),
            self.eye.yaw(),
            self.eye.pitch(),
        ));
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
        // tick, not to the frame. See [`Breach::pending_keys`].
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

    /// The whole of the mouse — the look and the trigger — and the one
    /// condition both are bound under.
    ///
    /// **`at.is_none()` is what says the pointer is really captured.**
    /// [`PointerUpdate::motion`] states that shape: under
    /// [`PointerMode::Locked`] there is no absolute position at all, so a
    /// locked frame carries a motion and no `at`, and an unlocked one that
    /// moved carries both. Binding to that rather than to the request
    /// [`pointer_mode`](HostedGame::pointer_mode) makes is the whole point — a
    /// request is not a grant, the loop declines the lock on a shell without
    /// `ShellCaps::has_mouselook`, and a view that turned anyway would swing
    /// while a visible cursor walked out of the window onto whatever is behind
    /// it. `apps/lantern` binds its free camera the same way.
    ///
    /// The trigger reads the same condition for the reason `ACTION_FIRE` gives:
    /// a click with a visible cursor is the click that asks a browser for the
    /// lock, or a press on whatever the page has under the mouse, and neither
    /// is a shot. So the frame that grabs the pointer does not also fire, and
    /// every click after it does.
    fn pointer_event(&mut self, pointer: PointerUpdate) {
        // Only a frame that carries a motion can say whether the pointer is
        // held, and it says it by whether a position came with it. A frame with
        // neither — a click from a mouse that has not moved — says nothing, and
        // leaves the answer as it was. See [`Breach::captured`].
        if pointer.motion.is_some() {
            self.captured = pointer.at.is_none();
        } else if pointer.at.is_some() {
            self.captured = false;
        }
        if !self.captured {
            return;
        }
        if let Some(motion) = pointer.motion {
            self.eye.look(motion);
        }
        // Latched, not fed in: the trigger is an edge and the tick has not run
        // yet. See [`Breach::pending_fire`].
        self.pending_fire |= pointer.pressed;
    }

    /// [`PointerMode::Locked`] while the range is being shot, free while the
    /// pause panel is up.
    ///
    /// A player who cannot reach their own cursor cannot leave, and the pause
    /// panel is the one place this demo has to be left from.
    fn pointer_mode(&self) -> PointerMode {
        if self.paused {
            PointerMode::Free
        } else {
            PointerMode::Locked
        }
    }

    fn menu_action(_id: crcbl::ui::WidgetId) -> Option<core::convert::Infallible> {
        None
    }

    fn apply(&mut self, action: core::convert::Infallible) {
        match action {}
    }

    fn menu_kind(&mut self, _menus: &mut Menus, paused: bool) -> MenuKind {
        // Recorded as well as answered: `pointer_mode` is asked immediately
        // after this and needs to know, and it is handed no argument.
        self.paused = paused;
        // The pause frees the pointer — `pointer_mode` below answers
        // `PointerMode::Free` — so whatever was held is not held any more, and
        // a click on the panel is a click at a place on it.
        self.captured &= !paused;
        MenuKind::of(paused)
    }

    fn draw(
        &mut self,
        gpu: &mut Gpu,
        draw_list: &mut crcbl::ui::draw_list::DrawList,
        frame: FrameInfo,
    ) {
        // **The view turns on the wall clock**, so a paused frame can still be
        // looked around from — and so the turn is smooth on a machine whose
        // frames do not line up with its ticks.
        let (yaw, pitch) = look_turn(&self.actions, frame.render_dt.as_secs_f32());
        if yaw != 0.0 || pitch != 0.0 {
            self.eye.turn(yaw, pitch);
        }

        let was_the_range_aiming = self.render_state.imposed_aim.is_some();
        self.render_state = self.game.render_state();

        // **And is taken away again while the range is running itself**, and
        // handed back squared up. The demonstration is aiming, and a
        // first-person camera that ignored that would be showing a different
        // room from the one being shot at; the frame the imposition ends is
        // the one that puts the shooter on the near lane, so a visitor's first
        // string starts from a known pose rather than from whatever bearing
        // the warm-up was swinging through. See [`RenderState::imposed_aim`],
        // which is where the argument lives — including why this edge is the
        // client's to notice and not a pose the simulation imposes for a tick.
        if let Some((yaw, pitch)) = self.render_state.imposed_aim {
            self.eye.point_at(yaw, pitch);
        } else if was_the_range_aiming {
            self.eye
                .point_at(crate::game::SQUARE_UP.0, crate::game::SQUARE_UP.1);
        }

        match &self.render_state.scene {
            Scene::Range {
                plates_x,
                plates_down,
            } => gpu.set_plates(*plates_x, *plates_down),
            Scene::Practice { bots, .. } => gpu.set_bots(bots),
        }
        // The simulation is `f64` and the renderer's camera is `f32`; this is
        // the one place the two meet.
        #[allow(clippy::cast_possible_truncation)]
        let eye = Vec3::new(
            self.render_state.eye.x as f32,
            self.render_state.eye.y as f32,
            self.render_state.eye.z as f32,
        );
        gpu.set_camera(self.eye.camera(eye));

        self.page = crate::page::draw(draw_list, gpu.atlas(), gpu.extent(), &self.render_state);
    }

    /// **Breach's two modules, and no third.**
    ///
    /// No network section: this sample runs over `InMemoryTransport` and has no
    /// connection to report on. No audio section either — slice 1 plays
    /// nothing, and a section that said so would be a module with no system
    /// behind it. What it does have is the range, whose every row is a number
    /// [`crcbl::phys`] produced, and the paths, which is rule 12.
    fn debug_sections(&self, panel: &mut crcbl::ui::DebugPanel) {
        panel.add(&self.stats);
        panel.add(&self.paths);
    }

    fn summary(&self, run: RunSummary) -> Summary {
        Summary {
            backend: run.backend,
            frames: run.frames,
            ticks: run.ticks,
            events: run.events,
            extent: run.extent,
            exit: run.exit,
            paused: run.paused,
            mode: run.mode,
            map: self.stats.arena.map(),
            feet: [
                self.stats.position.x,
                self.stats.feet,
                self.stats.position.z,
            ],
            shots: self.stats.shots,
            hits: self.stats.hits,
            paths: self.paths,
            commands: self.page.commands,
        }
    }

    fn log_summary(summary: &Summary) {
        crcbl::log::info!(
            "breach: {} frames, {} ticks on the {} map, feet at {:.2} {:.2} {:.2}, \
             {} shot(s) and {} hit(s), {} overlay commands, \
             geometry {:?}, binding {:?}, lighting {:?} ({:?})",
            summary.frames,
            summary.ticks,
            summary.map.name(),
            summary.feet[0],
            summary.feet[1],
            summary.feet[2],
            summary.shots,
            summary.hits,
            summary.commands,
            summary.paths.geometry,
            summary.paths.binding,
            summary.paths.lighting,
            summary.exit,
        );
    }
}

// ---- polled start-up ---------------------------------------------------------

/// A [`Loop`] being started one poll at a time, for a caller that may not block
/// — which on a browser main thread is every caller.
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
    /// [`BreachError`] if the shell refused the window.
    pub fn request(
        mut shell: Box<S>,
        options: &Options,
        clock_source: Clock,
    ) -> Result<Self, BreachError> {
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
                options.map,
            ),
            options: options.clone(),
        })
    }

    /// Advances start-up. `Ok(None)` means "not yet, poll again next frame".
    ///
    /// # Errors
    ///
    /// [`BreachError`] if the window went away before it had a size, if the
    /// device request failed, or if the simulation could not be built.
    pub fn poll(&mut self) -> Result<Option<Loop<S>>, BreachError> {
        let Some(booted) = self.boot.poll::<BreachError>()? else {
            return Ok(None);
        };
        assemble(booted, &self.options).map(Some)
    }
}

// ---- tests -------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use crcbl::args::Common;
    use crcbl::engine::{CONSOLE_KEY, PAUSE_KEY};
    use crcbl::shell::HeadlessShell;

    fn scripted(options: &Options) -> Loop<HeadlessShell> {
        with_shell(Box::new(HeadlessShell::new()), options).expect("headless always starts")
    }

    fn headless(frames: u64) -> Options {
        on(crate::map::MapChoice::Range, frames)
    }

    /// A headless run of `map`, for `frames` frames.
    fn on(map: crate::map::MapChoice, frames: u64) -> Options {
        Options {
            common: Common {
                headless: true,
                backend: Some(GpuBackend::Null),
                frames: Some(frames),
                ..Common::new(crate::game::DEFAULT_TICK_HZ)
            },
            map,
        }
    }

    /// Every `Text` command the frame handed to the UI pass.
    fn ui_text(engine: &Loop<HeadlessShell>) -> Vec<String> {
        use crcbl::ui::draw_list::DrawCommand;
        engine
            .gpu()
            .draw_list()
            .commands()
            .iter()
            .filter_map(|command| match command {
                DrawCommand::Text { text, .. } => Some(text.clone()),
                _ => None,
            })
            .collect()
    }

    /// Runs `count` frames.
    fn frames(engine: &mut Loop<HeadlessShell>, count: usize) {
        for _ in 0..count {
            engine.frame().expect("a frame");
        }
    }

    /// **A `bind` typed at the console moves the key the view turns on.**
    ///
    /// `docs/plan/52-debug-console.md` slice 8, over the *other* shape of
    /// `HostedGame::actions`: breach keeps its [`ActionMap`] on the hosted
    /// struct itself rather than inside a `Game` a module away, and this is that
    /// half proven. The observable is not the map — it is the **eye**, which
    /// `draw` turns from `look_turn(&self.actions, …)`: a rebind that reached
    /// some other map leaves the view exactly where it was.
    ///
    /// The practice map, not the range: the range imposes an aim on the camera
    /// during its warm-up (see `RenderState::imposed_aim`), and a yaw something
    /// else is writing cannot say whether a key turned it.
    #[test]
    fn a_console_rebind_moves_the_key_the_view_turns_on() {
        const NEW_KEY: KeyCode = KeyCode::KeyJ;
        let mut engine = scripted(&on(crate::map::MapChoice::Practice, 400));
        let window = engine.window();
        let tap = |engine: &mut Loop<HeadlessShell>, key: KeyCode| {
            let window = engine.window();
            engine
                .shell_mut()
                .key_press(window, key)
                .expect("the window is live");
            engine
                .shell_mut()
                .key_release(window, key)
                .expect("the window is live");
        };

        // One frame per step: the pump decides who a batch's keys belong to
        // from last frame's panel.
        tap(&mut engine, CONSOLE_KEY);
        frames(&mut engine, 1);
        engine
            .shell_mut()
            .commit_text(window, "bind look-left KeyJ")
            .expect("the window is live");
        tap(&mut engine, KeyCode::Enter);
        frames(&mut engine, 1);
        tap(&mut engine, CONSOLE_KEY);
        frames(&mut engine, 2);

        let before = engine.game().eye().yaw();

        // The key it was declared on turns nothing now. Exact equality rather
        // than a tolerance: on this map nothing else writes the yaw, so any
        // movement at all is this key still being bound.
        engine
            .shell_mut()
            .key_press(window, KeyCode::ArrowLeft)
            .expect("the window is live");
        frames(&mut engine, 10);
        let after_old = engine.game().eye().yaw();
        assert_eq!(
            after_old, before,
            "ten frames of the old key turned the view, so `look-left` is still bound to it"
        );
        engine
            .shell_mut()
            .key_release(window, KeyCode::ArrowLeft)
            .expect("the window is live");
        frames(&mut engine, 1);

        // And the key the console named does.
        engine
            .shell_mut()
            .key_press(window, NEW_KEY)
            .expect("the window is live");
        frames(&mut engine, 10);
        let after_new = engine.game().eye().yaw();
        assert_ne!(
            after_new, before,
            "ten frames of the key the console bound turned nothing, so the rebind \
             reached a map this game does not look with"
        );
    }

    /// **A headless run shoots the range and draws it.** The one check that says
    /// the whole bundle — server, controller, ray cast, renderer and overlay —
    /// came up and produced a frame with something on it.
    #[test]
    fn a_headless_run_shoots_the_range_and_draws_it() {
        let summary = run(&headless(240)).expect("the null backend always runs");
        assert_eq!(summary.frames, 240);
        assert_eq!(summary.exit, ExitReason::FrameBudget);
        assert!(summary.ticks > 0, "no tick ran");
        assert!(
            summary.commands > 0,
            "the run presented frames with nothing on them",
        );
        // The warm-up fires on its own, and it aims at a plate before it does.
        assert!(summary.shots > 0, "nothing was fired over the whole run");
        assert_eq!(
            summary.hits, summary.shots,
            "the range's own demonstration missed",
        );
        assert!(
            summary.feet[1].abs() < 0.05,
            "the player left the floor, at {:.2} m",
            summary.feet[1],
        );
    }

    /// **A headless run of the practice map walks its bots and gets shot at**,
    /// which is the one check that says the whole second bundle — that map's
    /// scene description, its instance pool, its bots and the shots they take —
    /// came up and produced a frame with something on it.
    ///
    /// Nothing presses a key: the practice map has no warm-up because three
    /// bots on patrol are already a picture that moves, and one of them is in
    /// the open in front of the spawn.
    #[test]
    fn a_headless_practice_run_walks_its_bots_and_shoots_back() {
        let summary =
            run(&on(crate::map::MapChoice::Practice, 300)).expect("the null backend always runs");
        assert_eq!(summary.map, crate::map::MapChoice::Practice);
        assert!(summary.ticks > 0, "no tick ran");
        assert!(
            summary.commands > 0,
            "the run presented frames with nothing on them",
        );
        assert_eq!(
            (summary.shots, summary.hits),
            (0, 0),
            "the practice map fired the player's pistol for them",
        );
        assert!(
            summary.feet[1].abs() < 0.05,
            "the player left the floor, at {:.2} m",
            summary.feet[1],
        );
    }

    /// **The mouse pulls the trigger only while the pointer is captured**, and
    /// neither a click with a visible cursor nor one from a mouse that has not
    /// moved counts as captured.
    ///
    /// Three cases, and the third is the one that is easy to get wrong. A
    /// build that fired on every click would shoot on the very click a browser
    /// is asked for its Pointer Lock with; one that fired on none would leave
    /// the mouse trigger silently unbound; and one that read `at.is_none()` as
    /// "captured" would fire on a still mouse with the cursor in plain sight,
    /// because [`PointerUpdate::at`] is `Some` only on a frame the pointer
    /// moved. See [`Breach::captured`].
    ///
    /// Driven on the practice map because it fires nothing by itself — the
    /// range warms up by shooting — so every shot on the counter is this
    /// test's.
    #[test]
    fn the_mouse_fires_only_while_the_pointer_is_captured() {
        use crcbl::math::Vec2;

        let click = |at: Option<Vec2>| PointerUpdate {
            at,
            motion: None,
            pressed: true,
            released: false,
        };
        let moved = |at: Option<Vec2>| PointerUpdate {
            at,
            motion: Some(Vec2::new(4.0, 0.0)),
            pressed: false,
            released: false,
        };
        let mut engine = scripted(&on(crate::map::MapChoice::Practice, 400));
        frames(&mut engine, 30);
        let shots = |engine: &Loop<HeadlessShell>| engine.game().stats.shots;
        assert_eq!(
            shots(&engine),
            0,
            "the practice map fired the player's pistol for them",
        );

        // A visible cursor means the click is the page's, not the trigger's.
        engine.game_mut().pointer_event(click(Some(Vec2::ZERO)));
        frames(&mut engine, 30);
        assert_eq!(
            shots(&engine),
            0,
            "a click with the cursor at a place on the surface pulled the trigger",
        );

        // **A still mouse reports no position either.** Nothing has captured
        // the pointer, so this must not fire — and it is the commonest click
        // there is.
        engine.game_mut().pointer_event(click(None));
        frames(&mut engine, 30);
        assert_eq!(
            shots(&engine),
            0,
            "a click from a mouse that had not moved was read as a captured pointer",
        );

        // A motion with no position at all is what a granted lock looks like,
        // and only that makes the click a shot.
        engine.game_mut().pointer_event(moved(None));
        engine.game_mut().pointer_event(click(None));
        frames(&mut engine, 30);
        assert_eq!(
            shots(&engine),
            1,
            "a click under the lock did not pull the trigger, or pulled it twice",
        );
    }

    /// **Two identical runs agree exactly**, which is what a fixed timestep over
    /// a scripted warm-up is for.
    #[test]
    fn a_headless_run_is_deterministic() {
        let first = run(&headless(120)).expect("headless runs everywhere");
        let second = run(&headless(120)).expect("headless runs everywhere");
        assert_eq!(first, second, "two identical runs must agree exactly");
        assert_eq!(first.backend, Backend::Headless);
    }

    /// **The look keys turn the view and the walk keys do not.** They are read
    /// on the frame's clock rather than the tick's, so this is also the check
    /// that they are read at all: an action declared and never polled is
    /// silent.
    ///
    /// Driven after the warm-up has been taken over, because until then the
    /// range owns the view and would overwrite any turn — which is the
    /// behaviour [`RenderState::imposed_aim`] describes and this test is the
    /// only thing that pins.
    #[test]
    fn the_look_keys_turn_the_view_and_the_walk_keys_leave_it_alone() {
        let mut engine = scripted(&headless(120));
        let window = engine.window();
        // Take the controls with one tap of a movement key, then let go.
        engine
            .shell_mut()
            .key_press(window, KeyCode::KeyD)
            .expect("the window is live");
        frames(&mut engine, 4);
        engine
            .shell_mut()
            .key_release(window, KeyCode::KeyD)
            .expect("the window is live");
        frames(&mut engine, 4);
        assert!(
            !engine.game().game().render_state().warming_up,
            "a movement key did not take the controls",
        );
        let opened = engine.game().eye().yaw();

        engine
            .shell_mut()
            .key_press(window, KeyCode::ArrowRight)
            .expect("the window is live");
        frames(&mut engine, 8);
        let turned = engine.game().eye().yaw();
        assert!(turned > opened, "the right arrow left the yaw at {turned}");

        engine
            .shell_mut()
            .key_release(window, KeyCode::ArrowRight)
            .expect("the window is live");
        engine
            .shell_mut()
            .key_press(window, KeyCode::KeyW)
            .expect("the window is live");
        frames(&mut engine, 8);
        assert!(
            (engine.game().eye().yaw() - turned).abs() < 1e-6,
            "walking turned the view",
        );
        engine.finish(ExitReason::FrameBudget).expect("teardown");
    }

    /// **The square-up survives a frame that is worth several ticks.** The
    /// range hands the view over by imposing a pose, and the client adopts it
    /// when it draws — so a pose offered on one tick and withdrawn on the next
    /// is one a slow frame never sees. This is the browser's case, not a
    /// hypothetical: the gate runs on a software rasteriser, and the first run
    /// of it walked off at the warm-up's last bearing and put its shot into a
    /// wall.
    ///
    /// Driven at six ticks a frame, which is what a browser at ten frames a
    /// second is doing. The bearing the warm-up is on when the key lands is
    /// asserted first, because a warm-up that happened to be squared up
    /// already would pass this whatever the handover did.
    #[test]
    fn the_range_squares_the_shooter_up_even_when_a_frame_is_many_ticks() {
        let slow = Options {
            common: Common {
                headless: true,
                backend: Some(GpuBackend::Null),
                frames: Some(600),
                ..Common::new(crate::game::DEFAULT_TICK_HZ * 6)
            },
            map: crate::map::MapChoice::Range,
        };
        let mut engine = scripted(&slow);
        let window = engine.window();
        // Far enough in for the warm-up to be off the near lane, which is the
        // only bearing that would flatter the handover.
        frames(&mut engine, 200);
        let swung = engine.game().eye().yaw();
        assert!(
            (swung - crate::game::SQUARE_UP.0).abs() > 0.05,
            "the warm-up was already squared up at yaw {swung}, so this proves nothing",
        );

        engine
            .shell_mut()
            .key_press(window, KeyCode::KeyW)
            .expect("the window is live");
        frames(&mut engine, 60);
        let squared = engine.game().eye().yaw();
        assert!(
            (squared - crate::game::SQUARE_UP.0).abs() < 1e-6,
            "the handover left the view at yaw {squared}, not squared up",
        );

        let walked = engine.game().game().render_state();
        assert!(
            walked.position.z < crate::map::SPAWN_Z - 0.5,
            "the walk did not start: z = {:.2}",
            walked.position.z,
        );
        assert!(
            (walked.position.x - crate::map::SPAWN.x).abs() < 0.05,
            "the walk drifted to x = {:.2}, so ticks ran at the old bearing",
            walked.position.x,
        );
        assert_eq!(
            walked.crosshair,
            crate::game::Aim::Plate(0),
            "a squared-up shooter is looking down the near lane",
        );
        engine.finish(ExitReason::FrameBudget).expect("teardown");
    }

    /// **A held walk key reaches the simulation and moves the player**, which is
    /// the path this sample exists to prove: shell event → action map → wire →
    /// module → `move_and_slide`, driven from a first-person camera. The same
    /// claim `web/tools/browser-e2e.mjs` makes in a browser, made here where a
    /// failure names the step.
    #[test]
    fn a_held_key_reaches_the_controller_and_moves_the_player() {
        let mut engine = scripted(&headless(300));
        let window = engine.window();
        frames(&mut engine, 8);
        engine
            .shell_mut()
            .key_press(window, KeyCode::KeyW)
            .expect("the window is live");
        frames(&mut engine, 60);
        let walked = engine.game().game().render_state();
        assert!(!walked.warming_up, "the warm-up survived a key press");
        assert!(
            walked.position.z < crate::map::SPAWN_Z - 0.5,
            "a second of walking got to z = {:.2} from {:.2}",
            walked.position.z,
            crate::map::SPAWN_Z,
        );

        engine
            .shell_mut()
            .key_release(window, KeyCode::KeyW)
            .expect("the window is live");
        frames(&mut engine, 60);
        let stopped = engine.game().game().render_state();
        assert!(
            (stopped.position - walked.position).length() < 0.01,
            "it kept moving after the key came up: {:?} then {:?}",
            walked.position,
            stopped.position,
        );
        engine.finish(ExitReason::FrameBudget).expect("teardown");
    }

    /// **The trigger is an edge, not a held state**, which is the difference
    /// between a pistol and a machine gun — and the one claim about this
    /// sample's input that nothing else here can make: `Controls::fire` is read
    /// with `just_pressed`, and a build that read it as held would fire on
    /// every tick the key was down.
    #[test]
    fn a_held_trigger_is_one_shot_and_not_sixty() {
        let mut engine = scripted(&headless(300));
        let window = engine.window();
        engine
            .shell_mut()
            .key_press(window, KeyCode::Space)
            .expect("the window is live");
        // Long enough that a held-state reading would have fired dozens of
        // times, and long enough for the knocked plate to come back up.
        frames(&mut engine, 200);
        let held = engine.game().game().render_state();
        assert_eq!(
            (held.shots, held.hits),
            (1, 1),
            "the trigger fired more than once while it was held down",
        );
        assert!(!held.warming_up, "a trigger pull did not take the controls",);

        engine
            .shell_mut()
            .key_release(window, KeyCode::Space)
            .expect("the window is live");
        engine
            .shell_mut()
            .key_press(window, KeyCode::Space)
            .expect("the window is live");
        frames(&mut engine, 30);
        assert_eq!(
            engine.game().game().render_state().shots,
            2,
            "a second press did not fire",
        );
        engine.finish(ExitReason::FrameBudget).expect("teardown");
    }

    /// **The panel renders with no network module, and it names the paths.**
    /// The sections breach has are the frame's, the GPU's where the device has
    /// timestamp queries, this sample's own, and rule 12's. Nothing else, and
    /// no configuration decided that.
    #[test]
    fn the_overlay_is_composed_of_exactly_the_modules_breach_has() {
        let mut options = headless(8);
        options.common.debug_overlay = Some(true);
        let mut engine = scripted(&options);
        frames(&mut engine, 2);

        let titles: Vec<&str> = engine
            .debug()
            .panel
            .sections()
            .iter()
            .map(crcbl::ui::DebugSection::title)
            .collect();
        let expected: &[&str] = if engine.gpu().timings().is_some() {
            &["frame", "gpu", "counters", "breach", "paths"]
        } else {
            &["frame", "counters", "breach", "paths"]
        };
        assert_eq!(titles, expected, "no module appears that no system offered");

        let drawn = ui_text(&engine);
        for row in ["frame", "shots", "hits", "geometry", "lighting"] {
            assert!(drawn.iter().any(|t| t == row), "missing {row}: {drawn:?}");
        }
        assert!(
            drawn.iter().any(|t| t == "ACCURACY"),
            "the overlay is drawn behind the panel: {drawn:?}",
        );
        engine.finish(ExitReason::FrameBudget).expect("teardown");
    }

    /// Escape stops the player and puts the one menu this sample has on screen;
    /// escape again starts it. The overlay keeps drawing either way.
    #[test]
    fn escape_stops_the_player_and_shows_the_pause_menu() {
        let mut engine = scripted(&headless(24));
        let window = engine.window();
        frames(&mut engine, 2);
        let running = engine.game().game().ticks_run();
        assert!(running > 0, "the simulation never ticked");
        assert_eq!(engine.menu_kind(), MenuKind::None);

        engine
            .shell_mut()
            .key_press(window, PAUSE_KEY)
            .expect("the window is live");
        frames(&mut engine, 2);
        assert!(engine.is_paused());
        assert_eq!(engine.menu_kind(), MenuKind::Paused);
        assert_eq!(
            engine.game().game().ticks_run(),
            running,
            "a paused loop runs no ticks",
        );
        assert!(
            ui_text(&engine).iter().any(|t| t == "ACCURACY"),
            "the overlay is drawn behind the panel",
        );
        engine.finish(ExitReason::FrameBudget).expect("teardown");
    }
}
