//! Shard's start-up, its controls, and the [`HostedGame`] methods the engine's
//! loop calls.
//!
//! # There is no loop in this file
//!
//! ```text
//! Loop::frame()                     ← the engine's
//!   pump, input, menu, pause, resize
//!     ─────────────────────────────→ Shard::key_event   (queued, except the torch key)
//!   run_ticks  ─────────────────────→ Shard::tick       (controls, then a tick)
//!   draw_list.clear()
//!     ─────────────────────────────→ Shard::draw        (camera, figure, lights, overlay)
//!     menu, debug overlay             ← the engine's
//!   gpu.frame()
//! ```
//!
//! What is left here is start-up, because a window's title is this sample's; the
//! action map, because a keyboard is not something [`crate::game`] should know
//! about; the camera and the light switch, because both are presentation; and the
//! trait methods, because they are what a hosted game is.
//!
//! # The camera swings on the frame's clock, the character walks on the tick's
//!
//! [`Shard::tick`] sends the simulation what the player is holding down and the
//! bearing the camera is at; [`Shard::draw`] closes whatever gap is left between
//! the camera's bearing and the one it was last asked for, and points it at
//! wherever the tick left the character. That split is the seam
//! `docs/plan/30-player-kit.md` draws — movement is a server system, the camera
//! is client presentation — and it is why a paused frame is still drawn from a
//! camera that finishes its swing.
//!
//! The bearing crossing that seam is the whole of what the simulation knows about
//! the camera. [`crate::camera`] is where it becomes a walk direction, and
//! `crcbl-phys` never sees any of it.
//!
//! # The torch key is presentation, and it is handled where presentation is
//!
//! `L` never reaches [`crate::game`]. What it changes is the **light list the
//! renderer is handed** — [`crate::light::torches`] — and a light is not
//! something an authoritative server owns in a sample with no gameplay attached
//! to darkness. So it is read straight off [`Shard::key_event`] rather than
//! through the [`ActionMap`], which is also what makes it work on a paused frame:
//! a demo whose subject is its lighting should let a visitor switch the lighting
//! while they are looking at a still picture of it.
//!
//! **`web/tools/browser-e2e.mjs` is built on that.** It douses the torches with
//! nothing else held and asserts the canvas stops changing, which is the control
//! for the claim that the light is what was changing it.
//!
//! # `[HUD]` is logged here rather than in `crate::game`
//!
//! Most of the line is the simulation's, and `apps/puppet` logs its heartbeat
//! from the tick for that reason. This one also names the three selectors and the
//! resolved effect set the frame is drawn through — rule 12 — and those are
//! [`crate::Paths`]', which the stage cannot see. Logging it here is what puts
//! both on one line at one cadence; `apps/quarry` and `apps/breach` do the same,
//! and for the same reason.

use crcbl::core::input::KeyCode;
use crcbl::engine::{
    Booted, Clock, ExitReason, FrameInfo, HostedGame, RunSummary, wait_for_configure,
};
use crcbl::input::{ActionDecl, ActionKind, ActionMap, Binding};
use crcbl::math::Vec3;
use crcbl::prelude::*;
use crcbl::shell::{DisplayMode, ShellBackend as Backend, WindowId};

use crate::camera::Iso;
use crate::game::{Controls, Game, RenderState, Stats};
use crate::gpu::{Gpu, Paths};
use crate::menu::{MenuKind, Menus};
use crate::page::PageStats;

pub use crate::args::Options;

// ---- the controls --------------------------------------------------------------

/// Walk, relative to where the camera is looking.
const ACTION_FORWARD: &str = "forward";
/// See [`ACTION_FORWARD`].
const ACTION_BACK: &str = "back";
/// See [`ACTION_FORWARD`].
const ACTION_LEFT: &str = "left";
/// See [`ACTION_FORWARD`].
const ACTION_RIGHT: &str = "right";
/// Swing the camera a quarter turn, anticlockwise and clockwise about `+Y`.
///
/// Read as a **press edge**, not as a held state: this rig's yaw moves between
/// four bearings rather than freely, and a held key that kept turning would make
/// it an orbit camera the player flies — which is `apps/puppet`'s rig and not
/// this one. See [`crate::camera`].
const ACTION_TURN_LEFT: &str = "turn-left";
/// See [`ACTION_TURN_LEFT`].
const ACTION_TURN_RIGHT: &str = "turn-right";

/// The key that puts the torches out and lights them again.
///
/// **Not in the [`ActionMap`]**, and the module docs say why: it is presentation,
/// so it is read off the raw key event on the frame's side of the seam rather
/// than resolved into a tick's [`Controls`].
const TORCH_KEY: KeyCode = KeyCode::KeyL;

/// The keyboard this sample is played with.
///
/// Declared in one place so the bindings and the read-out below cannot name
/// different actions: a typo in either is an action that resolves to nothing, and
/// [`ActionMap`] answers `false` for an action nobody declared rather than
/// complaining.
fn action_map() -> ActionMap {
    let mut map = ActionMap::new();
    for (name, bindings) in [
        (ACTION_FORWARD, vec![Binding::Key(KeyCode::KeyW)]),
        (ACTION_BACK, vec![Binding::Key(KeyCode::KeyS)]),
        (ACTION_LEFT, vec![Binding::Key(KeyCode::KeyA)]),
        (ACTION_RIGHT, vec![Binding::Key(KeyCode::KeyD)]),
        (ACTION_TURN_LEFT, vec![Binding::Key(KeyCode::KeyQ)]),
        (ACTION_TURN_RIGHT, vec![Binding::Key(KeyCode::KeyE)]),
    ] {
        map.declare(ActionDecl {
            name: name.into(),
            kind: ActionKind::Button,
            bindings,
        });
    }
    map
}

/// What the input is asking the **simulation** for on the tick `actions` has just
/// begun, at the bearing the camera is currently at.
///
/// The four movement actions read the **held** state — walking is a thing that
/// happens for as long as a key is down. The rotate actions are deliberately
/// absent: they move the camera, which is not part of what the server owns, and
/// [`Shard::tick`] reads their edges separately.
fn controls(actions: &ActionMap, yaw: f32) -> Controls {
    Controls {
        forward: actions.button_held(ACTION_FORWARD),
        back: actions.button_held(ACTION_BACK),
        left: actions.button_held(ACTION_LEFT),
        right: actions.button_held(ACTION_RIGHT),
        yaw,
    }
}

/// How many quarter turns the rotate keys asked for on this tick.
///
/// Positive is anticlockwise about `+Y`, which is [`Iso::rotate`]'s measure. Both
/// keys pressed on one tick is no turn, which is what makes releasing one of them
/// do the obvious thing.
fn turn_steps(actions: &ActionMap) -> i32 {
    i32::from(actions.just_pressed(ACTION_TURN_LEFT))
        - i32::from(actions.just_pressed(ACTION_TURN_RIGHT))
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
    /// The mode the window system actually had the window in, **not** the one the
    /// run last asked for.
    pub mode: DisplayMode,
    /// Where the character's feet ended up, in metres.
    pub feet: [f64; 3],
    /// How many ticks the walk was refused by stone.
    pub blocked: u64,
    /// How many ticks it stepped up onto the dais.
    pub climbed: u64,
    /// Whether the torches were still lit when the run ended.
    pub torches_lit: bool,
    /// Which selectors and effects the frames were drawn through — rule 12's
    /// "says which it took", in the summary line as well as in the panel.
    pub paths: Paths,
    /// How many commands the last overlay drew. Zero would mean a run that
    /// presented frames with nothing on them, which is the one failure a headless
    /// smoke test could otherwise report as a pass.
    pub commands: usize,
}

// ---- errors ------------------------------------------------------------------

/// What can stop shard: the loop's own failures, plus this sample's.
pub type ShardError = crcbl::engine::LoopError<crate::game::GameError>;

// ---- the hosted game ---------------------------------------------------------

/// Shard, as the engine's loop hosts it.
#[derive(Debug)]
pub struct Shard {
    game: Game,
    /// The keyboard, resolved into [`Controls`] once per tick.
    actions: ActionMap,
    /// Key events from the shell pump, replayed after `ActionMap::begin_tick`.
    ///
    /// The pump runs once per **frame** and the map's edge flags are per **tick**,
    /// and `begin_tick` clears those flags — so an event fed before it has its
    /// press edge erased. That matters here because the rotate keys are read as
    /// edges: a `Q` fed at the wrong moment is a quarter turn that never
    /// happened. Queueing here and replaying after is the order the map asks for,
    /// and it is what makes a frame that runs no ticks lossless.
    pending_keys: Vec<(KeyCode, bool)>,
    /// The isometric camera. **Presentation**: it never crosses the wire, and the
    /// only thing the simulation is told about it is its bearing.
    iso: Iso,
    /// Whether the zone's torches are burning. Presentation too — see the module
    /// docs for why this is not a thing the server owns.
    torches_lit: bool,
    /// Refilled from the simulation every frame.
    render_state: RenderState,
    /// The simulation's numbers, snapshotted in [`Shard::tick`].
    stats: Stats,
    /// What the last frame's overlay drew, from the same frame.
    page: PageStats,
    /// Which selectors and effects this device drew through, read off the GPU
    /// bundle.
    ///
    /// Kept here rather than reached through `gpu` because
    /// [`HostedGame::debug_sections`] and [`HostedGame::summary`] are handed
    /// `&self` and no GPU at all.
    paths: Paths,
}

impl Shard {
    /// The `[HUD]` line, on the cadence every other sample uses.
    ///
    /// `web/tools/browser-e2e.mjs` reads five claims out of it, and each one is a
    /// number nothing on the JS side can move:
    ///
    /// * `px`, `py`, `pz` — where the character is. The gate holds a key and
    ///   requires `pz` to advance, then releases it and requires `pz` to stop,
    ///   which is the pair a demo that merely drifts cannot pass.
    /// * `flame` — how bright the first torch is, as
    ///   [`crate::light::flame`] put it into the light list. **The one number
    ///   here nothing a player's walk can move**, and a pure function of the
    ///   *simulated* seconds — so a page presenting frames without ticking leaves
    ///   it standing still. It is what the gate's generic "the demo advances
    ///   under its own steam" check reads. Dousing the torches pins it at zero,
    ///   which is also how the gate knows its key landed.
    /// * `torches` — the switch itself, so a canvas comparison either side of it
    ///   is anchored to a reading rather than to a keystroke the gate merely
    ///   dispatched.
    /// * `ground`, `blocked` and `climbed` —
    ///   [`MoveOutcome`](crcbl::phys::MoveOutcome), counted. The last is the dais
    ///   being walked onto, which is the zone's vertical variety being used.
    /// * `geometry`, `binding`, `lighting` and `effects` — rule 12, and the claim
    ///   `docs/plan/sample/15-shard.md` says matters here more than anywhere: a
    ///   browser has no mesh stage, no bindless and no ray query, so these are the
    ///   fallback arms **by construction**, and this line is the only place
    ///   anything checks that they are the ones the frames took.
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
            "[HUD] tick: {}  px: {:.2}  py: {:.2}  pz: {:.2}  bearing: {:.3}  \
             ground: {}  blocked: {}  climbed: {}  torches: {}  flame: {:.3}  \
             geometry: {:?}  binding: {:?}  lighting: {:?}  effects: {}",
            stats.ticks,
            stats.position.x,
            stats.feet,
            stats.position.z,
            stats.yaw,
            if stats.grounded { "yes" } else { "no" },
            stats.blocked,
            stats.climbed,
            if self.torches_lit { "lit" } else { "out" },
            self.flame(),
            self.paths.geometry,
            self.paths.binding,
            self.paths.lighting,
            self.paths.effects_row(),
        );
    }

    /// How bright the first torch was on the tick the heartbeat is reporting —
    /// **as the renderer got it**, so a doused zone reads zero rather than the
    /// brightness a torch would have had if it were burning.
    fn flame(&self) -> f64 {
        if self.torches_lit {
            crate::light::flame(0, self.stats.elapsed)
        } else {
            0.0
        }
    }

    /// The simulation, for scripted tests and for an embedder that drives it.
    pub const fn game(&self) -> &Game {
        &self.game
    }

    /// Where the camera is pointing, for this crate's own tests.
    pub const fn camera(&self) -> &Iso {
        &self.iso
    }

    /// Whether the torches are burning, for this crate's own tests.
    pub const fn torches_lit(&self) -> bool {
        self.torches_lit
    }

    /// What the last frame's overlay drew.
    pub const fn page(&self) -> &PageStats {
        &self.page
    }
}

/// The loop shard runs in.
///
/// A type alias, because the loop is the engine's. `S` is the shell type: the
/// native path builds `Loop<dyn Shell>`, and the tests build
/// `Loop<HeadlessShell>` so they can inject the events a compositor would send.
pub type Loop<S = dyn Shell> = crcbl::engine::Loop<S, Shard>;

/// Runs the full loop.
///
/// # Errors
///
/// [`ShardError`] if the shell, the GPU or the simulation's server failed.
/// Teardown runs on every path.
pub fn run(options: &Options) -> Result<Summary, ShardError> {
    crcbl::engine::drive(start(options)?)
}

/// Opens a shell, a window, a GPU and the simulation.
///
/// # Errors
///
/// [`ShardError`] if any of them refused.
pub fn start(options: &Options) -> Result<Loop, ShardError> {
    let shell = crcbl::engine::open_shell(options.common.headless)?;
    with_shell(shell, options)
}

/// Builds the loop on an already-open shell, blocking on both waits.
///
/// The browser cannot use this — a main thread may not sit in
/// [`wait_for_configure`] — and takes [`PendingLoop`] instead. What the two share
/// is everything after the waiting, which is `assemble` — private, because a
/// caller has no `Booted` to hand it.
///
/// # Errors
///
/// [`ShardError`] if the window never configured, the GPU would not open, or the
/// simulation's server could not be built.
pub fn with_shell<S: Shell + ?Sized>(
    mut shell: Box<S>,
    options: &Options,
) -> Result<Loop<S>, ShardError> {
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
/// [`Booted`] is what both bring-up paths hand over, so the simulation is built
/// and the loop assembled in one place rather than one per path — a second copy
/// is how the browser build would come to run a subtly different sample.
///
/// # Errors
///
/// [`ShardError`] if the simulation's server could not be built.
fn assemble<S: Shell + ?Sized>(
    booted: Booted<S, Gpu>,
    options: &Options,
) -> Result<Loop<S>, ShardError> {
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
    let game = Game::new(options.common.tick_hz).map_err(ShardError::Game)?;
    Ok(Loop::new(
        booted,
        Shard {
            game,
            actions: action_map(),
            pending_keys: Vec::new(),
            iso: Iso::default(),
            // A zone whose torches were out on arrival is a zone a visitor reads
            // as broken, and the whole subject here is what they light.
            torches_lit: true,
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
) -> Result<WindowId, ShardError> {
    Ok(crcbl::engine::open_window(
        shell,
        clock_source,
        &WindowDesc {
            title: "Shard",
            app_id: "sh.kryptic.crcbl.shard",
            size: crcbl::engine::requested_window_size(size),
            mode,
            ..WindowDesc::default()
        },
    )?)
}

/// Shard's half of the frame, and nothing else.
impl HostedGame for Shard {
    type Error = crate::game::GameError;
    type Gpu = Gpu;
    type MenuKind = MenuKind;
    /// Shard declares no menu action of its own — see [`crate::menu`].
    /// Uninhabited rather than a placeholder enum, so [`Shard::apply`] is a match
    /// on nothing and the compiler agrees there is no case to handle.
    type MenuAction = core::convert::Infallible;
    type Summary = Summary;

    const NAME: &'static str = "shard";

    fn menus() -> Menus {
        crate::menu::menus()
    }

    fn tick(&mut self, gpu: &mut Gpu, tick_dt: f64) {
        // `ActionMap` holds its timers in `f32`, which is the precision an input
        // edge is worth.
        #[allow(clippy::cast_possible_truncation)]
        self.actions.begin_tick(tick_dt as f32);
        for (key, pressed) in std::mem::take(&mut self.pending_keys) {
            self.actions.key_event(key, pressed);
        }
        // The camera's bearing is asked for here and closed in `draw`: a quarter
        // turn is a discrete request and the swing that serves it is a frame's
        // business. See [`crate::camera`].
        let steps = turn_steps(&self.actions);
        if steps != 0 {
            self.iso.rotate(steps);
        }
        // The bearing goes with the buttons: what the player asked for is
        // "forward", and that means nothing beside the direction they were
        // looking when they asked.
        self.game
            .set_controls(controls(&self.actions, self.iso.yaw()));
        self.game.tick();
        // Read off the bundle rather than kept from start-up alone, so the
        // heartbeat below and the panel are reporting the device this frame
        // actually has.
        self.paths = gpu.paths();
        self.stats = self.game.stats();
        self.log_heartbeat();
    }

    /// The keyboard, with the torch key taken out of it.
    ///
    /// Everything else is queued for the tick, in `Shard::pending_keys`. `L`
    /// is not: it is presentation, it changes the light list and nothing the
    /// server owns, and it works on a paused frame because a lighting demo should
    /// let its lighting be switched while a visitor is looking at a still picture.
    /// The module docs carry the argument, and
    /// `the_torch_key_douses_the_zone_on_a_paused_frame` is what pins it.
    fn key_event(&mut self, key: KeyCode, pressed: bool) {
        if key == TORCH_KEY {
            if pressed {
                self.torches_lit = !self.torches_lit;
            }
            return;
        }
        self.pending_keys.push((key, pressed));
    }

    fn menu_action(_id: crcbl::ui::WidgetId) -> Option<core::convert::Infallible> {
        None
    }

    fn apply(&mut self, action: core::convert::Infallible) {
        match action {}
    }

    fn menu_kind(&mut self, _menus: &mut Menus, paused: bool) -> MenuKind {
        MenuKind::of(paused)
    }

    fn draw(
        &mut self,
        gpu: &mut Gpu,
        draw_list: &mut crcbl::ui::draw_list::DrawList,
        frame: FrameInfo,
    ) {
        // **The camera swings on the wall clock**, so a paused frame finishes the
        // turn it was in the middle of, and so the swing is smooth on a machine
        // whose frames do not line up with its ticks.
        self.iso.advance(frame.render_dt.as_secs_f32());

        self.render_state = self.game.render_state();
        gpu.set_figure(self.render_state.feet);
        // **The whole of the zone's lighting, decided by two numbers.** The
        // simulated clock is what makes the flames a function of the tick rather
        // than of the frame, and the switch is what the browser gate's still-frame
        // control turns on. See [`crate::light::torches`].
        gpu.set_lighting(self.render_state.elapsed, self.torches_lit);

        // The simulation is `f64` and the renderer's camera is `f32`; this is the
        // one place the two meet.
        #[allow(clippy::cast_possible_truncation)]
        let feet = Vec3::new(
            self.render_state.feet.x as f32,
            self.render_state.feet.y as f32,
            self.render_state.feet.z as f32,
        );
        gpu.set_camera(self.iso.camera(feet));

        self.page = crate::page::draw(
            draw_list,
            gpu.atlas(),
            gpu.extent(),
            &self.render_state,
            self.torches_lit,
        );
    }

    /// **Shard's two modules, and no third.**
    ///
    /// No network section: this sample runs over `InMemoryTransport` and has no
    /// connection to report on — milestone 1 ships no networking at all, which
    /// `docs/plan/sample/15-shard.md` says in as many words. No audio section
    /// either; slice 1 plays nothing, and a section that said so would be a module
    /// with no system behind it. What it does have is the stage, whose every row
    /// is a number [`crcbl::phys`] produced, and the paths, which is rule 12.
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
            feet: [
                self.stats.position.x,
                self.stats.feet,
                self.stats.position.z,
            ],
            blocked: self.stats.blocked,
            climbed: self.stats.climbed,
            torches_lit: self.torches_lit,
            paths: self.paths,
            commands: self.page.commands,
        }
    }

    fn log_summary(summary: &Summary) {
        crcbl::log::info!(
            "shard: {} frames, {} ticks, feet at {:.2} {:.2} {:.2}, \
             {} blocked and {} climbed, torches {}, {} overlay commands, \
             geometry {:?}, binding {:?}, lighting {:?}, effects {} ({:?})",
            summary.frames,
            summary.ticks,
            summary.feet[0],
            summary.feet[1],
            summary.feet[2],
            summary.blocked,
            summary.climbed,
            if summary.torches_lit { "lit" } else { "out" },
            summary.commands,
            summary.paths.geometry,
            summary.paths.binding,
            summary.paths.lighting,
            summary.paths.effects_row(),
            summary.exit,
        );
    }
}

// ---- polled start-up ---------------------------------------------------------

/// A [`Loop`] being started one poll at a time, for a caller that may not block —
/// which on a browser main thread is every caller.
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
    /// [`ShardError`] if the shell refused the window.
    pub fn request(
        mut shell: Box<S>,
        options: &Options,
        clock_source: Clock,
    ) -> Result<Self, ShardError> {
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
    /// [`ShardError`] if the window went away before it had a size, if the device
    /// request failed, or if the simulation could not be built.
    pub fn poll(&mut self) -> Result<Option<Loop<S>>, ShardError> {
        let Some(booted) = self.boot.poll::<ShardError>()? else {
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
    use crcbl::engine::PAUSE_KEY;
    use crcbl::shell::HeadlessShell;

    fn scripted(options: &Options) -> Loop<HeadlessShell> {
        with_shell(Box::new(HeadlessShell::new()), options).expect("headless always starts")
    }

    fn headless(frames: u64) -> Options {
        Options {
            common: Common {
                headless: true,
                backend: Some(GpuBackend::Null),
                frames: Some(frames),
                ..Common::new(crate::game::DEFAULT_TICK_HZ)
            },
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

    /// **A headless run walks the zone and draws it.** The one check that says the
    /// whole bundle — server, controller, renderer and overlay — came up and
    /// produced a frame with something on it.
    #[test]
    fn a_headless_run_stands_in_the_zone_and_draws_it() {
        let summary = run(&headless(180)).expect("the null backend always runs");
        assert_eq!(summary.frames, 180);
        assert_eq!(summary.exit, ExitReason::FrameBudget);
        assert!(summary.ticks > 0, "no tick ran");
        assert!(
            summary.commands > 0,
            "the run presented frames with nothing on them",
        );
        assert!(
            summary.feet[1].abs() < 0.05,
            "the character left the floor, at {:.2} m",
            summary.feet[1],
        );
        assert!(summary.torches_lit, "the zone opened with its torches out");
    }

    /// **Two identical runs agree exactly**, which is what a fixed timestep with
    /// no input is for.
    #[test]
    fn a_headless_run_is_deterministic() {
        let first = run(&headless(90)).expect("headless runs everywhere");
        let second = run(&headless(90)).expect("headless runs everywhere");
        assert_eq!(first, second, "two identical runs must agree exactly");
        assert_eq!(first.backend, Backend::Headless);
    }

    /// **A held walk key reaches the simulation and moves the character**, which
    /// is the path this sample exists to prove: shell event → action map → wire →
    /// module → `move_and_slide`, driven from a third rig. The same claim
    /// `web/tools/browser-e2e.mjs` makes in a browser, made here where a failure
    /// names the step.
    #[test]
    fn a_held_key_reaches_the_controller_and_moves_the_character() {
        let mut engine = scripted(&headless(300));
        let window = engine.window();
        frames(&mut engine, 8);
        let start = engine.game().game().render_state().position;
        engine
            .shell_mut()
            .key_press(window, KeyCode::KeyW)
            .expect("the window is live");
        frames(&mut engine, 60);
        let walked = engine.game().game().render_state().position;
        assert!(
            walked.z < start.z - 1.0,
            "a second of walking got to z = {:.2} from {:.2}",
            walked.z,
            start.z,
        );

        engine
            .shell_mut()
            .key_release(window, KeyCode::KeyW)
            .expect("the window is live");
        frames(&mut engine, 60);
        let stopped = engine.game().game().render_state().position;
        assert!(
            (stopped - walked).length() < 0.01,
            "it kept moving after the key came up: {walked:?} then {stopped:?}",
        );
        engine.finish(ExitReason::FrameBudget).expect("teardown");
    }

    /// **A rotate key swings the camera one quarter turn and holds there**, and a
    /// walk key does not touch it.
    ///
    /// The walk is the control: a rig whose bearing followed anything the
    /// keyboard did would pass "the bearing changed" and fail this.
    #[test]
    fn a_rotate_key_swings_the_camera_and_the_walk_keys_leave_it_alone() {
        let mut engine = scripted(&headless(300));
        let window = engine.window();
        frames(&mut engine, 4);
        let opened = engine.game().camera().yaw();
        assert!(engine.game().camera().settled());

        engine
            .shell_mut()
            .key_press(window, KeyCode::KeyE)
            .expect("the window is live");
        frames(&mut engine, 4);
        engine
            .shell_mut()
            .key_release(window, KeyCode::KeyE)
            .expect("the window is live");
        // Long enough for the swing to finish, and then some — the point is that
        // it *stops* rather than that it moves.
        frames(&mut engine, 120);
        let turned = engine.game().camera().yaw();
        assert!(
            (turned - (opened - crate::camera::YAW_STEP)).abs() < 1e-5,
            "one tap of E left the bearing at {turned}, not one quarter turn from {opened}",
        );

        engine
            .shell_mut()
            .key_press(window, KeyCode::KeyW)
            .expect("the window is live");
        frames(&mut engine, 60);
        assert!(
            (engine.game().camera().yaw() - turned).abs() < 1e-6,
            "walking turned the camera",
        );
        engine.finish(ExitReason::FrameBudget).expect("teardown");
    }

    /// **The torch key douses the zone, and it does it on a paused frame** —
    /// which is what says the switch is on the presentation side of the seam
    /// rather than something the tick has to run to see.
    ///
    /// The paused half is the sharper claim, and it is the one the browser gate
    /// depends on being true of a key that never reaches [`crate::game`].
    #[test]
    fn the_torch_key_douses_the_zone_on_a_paused_frame() {
        let mut engine = scripted(&headless(120));
        let window = engine.window();
        frames(&mut engine, 4);
        assert!(engine.game().torches_lit(), "the zone opened dark");

        engine
            .shell_mut()
            .key_press(window, PAUSE_KEY)
            .expect("the window is live");
        frames(&mut engine, 2);
        assert!(engine.is_paused());
        let stalled = engine.game().game().ticks_run();

        engine
            .shell_mut()
            .key_press(window, KeyCode::KeyL)
            .expect("the window is live");
        frames(&mut engine, 2);
        assert!(
            !engine.game().torches_lit(),
            "the torch key did not reach a paused frame",
        );
        assert_eq!(
            engine.game().game().ticks_run(),
            stalled,
            "the torch key ran a tick, so it is not presentation",
        );
        assert!(
            ui_text(&engine).iter().any(|word| word == "OUT"),
            "the panel still says the torches are lit: {:?}",
            ui_text(&engine),
        );

        // …and a second press lights them again, which is what makes it a switch
        // rather than a one-way trip.
        engine
            .shell_mut()
            .key_release(window, KeyCode::KeyL)
            .expect("the window is live");
        engine
            .shell_mut()
            .key_press(window, KeyCode::KeyL)
            .expect("the window is live");
        frames(&mut engine, 2);
        assert!(engine.game().torches_lit(), "it would not light again");
        engine.finish(ExitReason::FrameBudget).expect("teardown");
    }

    /// **The panel renders with no network module, and it names the paths.** The
    /// sections shard has are the frame's, the GPU's where the device has
    /// timestamp queries, this sample's own, and rule 12's. Nothing else, and no
    /// configuration decided that.
    #[test]
    fn the_overlay_is_composed_of_exactly_the_modules_shard_has() {
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
            &["frame", "gpu", "counters", "shard", "paths"]
        } else {
            &["frame", "counters", "shard", "paths"]
        };
        assert_eq!(titles, expected, "no module appears that no system offered");

        let drawn = ui_text(&engine);
        for row in ["tick", "climbed", "geometry", "lighting", "effects"] {
            assert!(drawn.iter().any(|t| t == row), "missing {row}: {drawn:?}");
        }
        assert!(
            drawn.iter().any(|t| t == "TORCHES"),
            "the overlay is drawn behind the panel: {drawn:?}",
        );
        engine.finish(ExitReason::FrameBudget).expect("teardown");
    }

    /// Escape stops the character and puts the one menu this sample has on
    /// screen; escape again starts it. The overlay keeps drawing either way.
    #[test]
    fn escape_stops_the_character_and_shows_the_pause_menu() {
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
            ui_text(&engine).iter().any(|t| t == "TORCHES"),
            "the overlay is drawn behind the panel",
        );
        engine.finish(ExitReason::FrameBudget).expect("teardown");
    }
}
