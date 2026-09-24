//! Puppet's start-up, its controls, and the [`HostedGame`] methods the engine's
//! loop calls.
//!
//! # There is no loop in this file
//!
//! ```text
//! Loop::frame()                     ← the engine's
//!   pump, input, menu, pause, resize
//!     ─────────────────────────────→ Puppet::key_event  (queued, not applied)
//!   run_ticks  ─────────────────────→ Puppet::tick      (controls, then a tick)
//!   draw_list.clear()
//!     ─────────────────────────────→ Puppet::draw       (camera, character, overlay)
//!     menu, debug overlay             ← the engine's
//!   gpu.frame()
//! ```
//!
//! What is left here is start-up, because a window's title is this sample's; the
//! action map, because a keyboard is not something [`crate::game`] should know
//! about; the camera, because it is presentation; and the trait methods, because
//! they are what a hosted game is.
//!
//! # The camera turns on the frame's clock, the character walks on the tick's
//!
//! [`Puppet::tick`] sends the simulation what the player is holding down and
//! the yaw the view is at; [`Puppet::draw`] turns the view and points it at
//! wherever the tick left the character. That split is the seam
//! `docs/plan/30-player-kit.md` draws — movement is a server system, camera
//! follow is client presentation — and it is why a paused frame can still be
//! looked around from while the character does not move.
//!
//! The yaw crossing that seam is the whole of what the simulation knows about
//! the camera. [`crate::camera`] is where it becomes a direction, and
//! `crcbl-phys` never sees either.

use crcbl::core::input::KeyCode;
use crcbl::engine::{Booted, Clock, FrameInfo, HostedGame, RunSummary, wait_for_configure};
use crcbl::input::{ActionDecl, ActionKind, ActionMap, Binding};
use crcbl::math::Vec3;
use crcbl::prelude::*;
use crcbl::shell::{DisplayMode, WindowId};

use crate::anim::Animator;
use crate::camera::Follow;
use crate::game::{Controls, Game, RenderState, Stats};
use crate::gpu::Gpu;
use crate::map::Map;
use crate::menu::{MenuKind, Menus};
use crate::page::PageStats;

pub use crate::args::Options;

// ---- the controls --------------------------------------------------------------

/// Walk away from the camera. Two bindings each, so the demo is playable on the
/// arrow keys alone and on `WASD` alone.
const ACTION_FORWARD: &str = "forward";
/// See [`ACTION_FORWARD`].
const ACTION_BACK: &str = "back";
/// See [`ACTION_FORWARD`].
const ACTION_LEFT: &str = "left";
/// See [`ACTION_FORWARD`].
const ACTION_RIGHT: &str = "right";
/// Swing the camera about the character, anticlockwise and clockwise.
const ACTION_CAMERA_LEFT: &str = "camera-left";
/// See [`ACTION_CAMERA_LEFT`].
const ACTION_CAMERA_RIGHT: &str = "camera-right";
/// Raise and lower the camera's elevation.
const ACTION_CAMERA_UP: &str = "camera-up";
/// See [`ACTION_CAMERA_UP`].
const ACTION_CAMERA_DOWN: &str = "camera-down";

/// The keyboard this sample is walked with.
///
/// Declared in one place so the bindings and the read-out below cannot name
/// different actions: a typo in either is an action that resolves to nothing,
/// and [`ActionMap`] answers `false` for an action nobody declared rather than
/// complaining.
fn action_map() -> ActionMap {
    let mut map = ActionMap::new();
    for (name, keys) in [
        (ACTION_FORWARD, vec![KeyCode::KeyW, KeyCode::ArrowUp]),
        (ACTION_BACK, vec![KeyCode::KeyS, KeyCode::ArrowDown]),
        (ACTION_LEFT, vec![KeyCode::KeyA, KeyCode::ArrowLeft]),
        (ACTION_RIGHT, vec![KeyCode::KeyD, KeyCode::ArrowRight]),
        (ACTION_CAMERA_LEFT, vec![KeyCode::KeyQ]),
        (ACTION_CAMERA_RIGHT, vec![KeyCode::KeyE]),
        (ACTION_CAMERA_UP, vec![KeyCode::KeyR]),
        (ACTION_CAMERA_DOWN, vec![KeyCode::KeyF]),
    ] {
        map.declare(ActionDecl {
            name: name.into(),
            kind: ActionKind::Button,
            bindings: keys.into_iter().map(Binding::Key).collect(),
        });
    }
    map
}

/// What the keyboard is asking the **simulation** for on the tick `actions` has
/// just begun, at the yaw the view is currently at.
///
/// Every one of these reads the **held** state: walking is a thing that happens
/// for as long as a key is down, and there is nothing in milestone 1 that
/// happens on a press. The camera actions are deliberately absent — they are
/// read in [`Puppet::draw`], on the frame's clock, because the camera is not
/// part of what the server owns.
fn controls(actions: &ActionMap, yaw: f32) -> Controls {
    Controls {
        forward: actions.button_held(ACTION_FORWARD),
        back: actions.button_held(ACTION_BACK),
        left: actions.button_held(ACTION_LEFT),
        right: actions.button_held(ACTION_RIGHT),
        yaw,
    }
}

/// How far the camera should turn this frame, given what is held down and how
/// long the frame was: `(yaw, pitch)` in radians.
fn camera_turn(actions: &ActionMap, seconds: f32) -> (f32, f32) {
    let axis = |positive: &str, negative: &str| {
        f32::from(i8::from(actions.button_held(positive)) - i8::from(actions.button_held(negative)))
    };
    (
        axis(ACTION_CAMERA_RIGHT, ACTION_CAMERA_LEFT) * crate::camera::TURN_RATE * seconds,
        axis(ACTION_CAMERA_UP, ACTION_CAMERA_DOWN) * crate::camera::TURN_RATE * seconds,
    )
}

// ---- summary -----------------------------------------------------------------

/// What a finished run reports.
///
/// [`PartialEq`] but not [`Eq`], unlike the 2D samples': the position is floats,
/// so two runs are compared by the numbers they produced and there is no total
/// order to claim.
#[derive(Clone, Debug, PartialEq)]
pub struct Summary {
    /// The half of the report every sample shares.
    pub run: RunSummary,
    /// Where the character's feet ended up, in metres. The other samples report
    /// a score here; this one is a walk, and this is where the walk got to.
    pub feet: [f64; 3],
    /// How many steps the controller climbed over the whole run.
    pub climbed: u64,
    /// How many ticks it was stopped by something too steep to stand on.
    pub blocked: u64,
    /// How many commands the last overlay drew. Zero would mean a run that
    /// presented frames with nothing on them, which is the one failure a
    /// headless smoke test could otherwise report as a pass.
    pub commands: usize,
}

// ---- errors ------------------------------------------------------------------

/// What can stop puppet: the loop's own failures, plus this sample's.
pub type PuppetError = crcbl::engine::LoopError<crate::game::GameError>;

// ---- the hosted game ---------------------------------------------------------

/// Puppet, as the engine's loop hosts it.
#[derive(Debug)]
pub struct Puppet {
    game: Game,
    /// The blockout this run opened on — the committed one, or whatever
    /// `--scene` pointed at.
    ///
    /// Kept because the sun is the map's: [`Puppet::draw`] asks it for the light
    /// at the simulation's own elapsed time, and a run reading the built-in
    /// map's sun while walking a loaded one would light the wrong world.
    map: Map,
    /// The keyboard, resolved into [`Controls`] once per tick.
    actions: ActionMap,
    /// Key events from the shell pump, replayed after `ActionMap::begin_tick`.
    ///
    /// The pump runs once per **frame** and the map's edge flags are per
    /// **tick**, and `begin_tick` clears those flags — so an event fed before it
    /// has its press edge erased. Queueing here and replaying after is the order
    /// the map asks for, and it is what makes a frame that runs no ticks
    /// lossless.
    pending_keys: Vec<(KeyCode, bool)>,
    /// The third-person camera. **Presentation**: it never crosses the wire, and
    /// the only thing the simulation is told about it is its yaw.
    follow: Follow,
    /// Refilled from the simulation every frame.
    render_state: RenderState,
    /// The simulation's numbers, snapshotted in [`Puppet::draw`].
    ///
    /// A snapshot rather than a read at panel time because
    /// [`HostedGame::debug_sections`] is handed `&self` while reading the stage
    /// takes its lock.
    stats: Stats,
    /// What the last overlay drew, from the same frame.
    page: PageStats,
    /// The character's rig, posed from the simulation's measured speed.
    ///
    /// **Presentation, like the camera**: it runs on the frame's clock, nothing
    /// in it crosses the wire, and the tick would draw the same picture without
    /// it. The animation rules in `docs/notes/simulation.md` put pose
    /// evaluation on the client, and this is where puppet's client is.
    anim: Animator,
    /// Seconds of frame time since the last `[POSE]` line.
    pose_report: f32,
}

/// How often the `[POSE]` line is logged, in seconds of frame time.
///
/// The same cadence [`crate::game::HEARTBEAT_TICKS`] spaces the `[HUD]` line
/// at, measured on the other clock — this line is the client's and that one is
/// the simulation's.
const POSE_REPORT_S: f32 = 1.0;

impl Puppet {
    /// The `[POSE]` line: what the locomotion blend did with the speed the
    /// controller measured.
    ///
    /// **A second line rather than four more terms on the `[HUD]` one**, and
    /// the reason is which clock each is on. The `[HUD]` line is logged from
    /// [`Game::tick`](crate::game::Game::tick) on the fixed timestep and is the
    /// simulation's report; everything here is composed on the frame, after the
    /// tick has already gone. Folding them together would mean either posting
    /// the pose back to the stage or logging the simulation off the frame, and
    /// both put a number on a line whose cadence is not the one that produced
    /// it.
    ///
    /// `web/tools/browser-e2e.mjs` reads three claims out of it, and each is a
    /// number nothing but the blend can move:
    ///
    /// * `blend` — where the character sits across the locomotion set, 0 at the
    ///   idle stop and 1 at [`crate::anim::WALK_STOP_MPS`]. It follows `speed`,
    ///   which is measured from the controller's own displacement.
    /// * `mid` — how many frames the blend has spent **strictly between** the
    ///   two stops. The heartbeat is a second apart and a crossing takes less
    ///   than that, so the counter is what says the weight swept rather than
    ///   snapped.
    /// * `dev` — how far the pose has carried a joint from the rest pose, in
    ///   metres. It sweeps while the character walks and holds still while it
    ///   stands, because [`crate::rig::idle`] is a stance.
    fn report_pose(&mut self, render_dt: f32, speed: f32) {
        self.pose_report += render_dt;
        if self.pose_report < POSE_REPORT_S {
            return;
        }
        self.pose_report = 0.0;
        crcbl::log::info!(
            "[POSE] speed: {:.2}  blend: {:.2}  mid: {}  dev: {:.3}",
            speed,
            self.anim.blend(),
            self.anim.partial(),
            self.anim.deviation(),
        );
    }
}

/// The loop puppet runs in.
///
/// A type alias, because the loop is the engine's. `S` is the shell type: the
/// native path builds `Loop<dyn Shell>`, and the tests build
/// `Loop<HeadlessShell>` so they can inject the events a compositor would send.
pub type Loop<S = dyn Shell> = crcbl::engine::Loop<S, Puppet>;

/// Runs the full loop.
///
/// # Errors
///
/// [`PuppetError`] if the shell, the GPU or the simulation's server failed.
/// Teardown runs on every path.
pub fn run(options: &Options) -> Result<Summary, PuppetError> {
    crcbl::engine::drive(start(options)?)
}

/// Opens a shell, a window, a GPU and the simulation.
///
/// # Errors
///
/// [`PuppetError`] if any of them refused.
pub fn start(options: &Options) -> Result<Loop, PuppetError> {
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
/// [`PuppetError`] if the window never configured, the GPU would not open, or
/// the simulation's server could not be built.
pub fn with_shell<S: Shell + ?Sized>(
    mut shell: Box<S>,
    options: &Options,
) -> Result<Loop<S>, PuppetError> {
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
/// [`Booted`] is what both bring-up paths hand over, so the simulation is built
/// and the loop assembled in one place rather than one per path — a second copy
/// is how the browser build would come to run a subtly different sample.
///
/// # Errors
///
/// [`PuppetError`] if the simulation's server could not be built.
fn assemble<S: Shell + ?Sized>(
    booted: Booted<S, Gpu>,
    options: &Options,
) -> Result<Loop<S>, PuppetError> {
    let booted = crcbl::engine::arm_screenshot(booted, &options.common);
    let game = Game::new(options.common.tick_hz, &options.map).map_err(PuppetError::Game)?;
    Ok(Loop::new(
        booted,
        Puppet {
            game,
            map: options.map.clone(),
            actions: action_map(),
            pending_keys: Vec::new(),
            follow: Follow::default(),
            render_state: RenderState::default(),
            stats: Stats::default(),
            page: PageStats::default(),
            anim: Animator::new(),
            pose_report: 0.0,
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
) -> Result<WindowId, PuppetError> {
    Ok(crcbl::engine::open_window(
        shell,
        clock_source,
        &WindowDesc {
            title: "Puppet",
            app_id: "sh.kryptic.crcbl.puppet",
            size: crcbl::engine::requested_window_size(size),
            mode,
            ..WindowDesc::default()
        },
    )?)
}

impl Puppet {
    /// The simulation, for scripted tests and for an embedder that drives it.
    pub const fn game(&self) -> &Game {
        &self.game
    }

    /// Where the camera is, for this crate's own tests.
    pub const fn follow(&self) -> &Follow {
        &self.follow
    }

    /// What the last frame's overlay drew.
    pub const fn page(&self) -> &PageStats {
        &self.page
    }
}

/// Puppet's half of the frame, and nothing else.
impl HostedGame for Puppet {
    type Error = crate::game::GameError;
    type Gpu = Gpu;
    type MenuKind = MenuKind;
    /// Puppet declares no menu action of its own — see [`crate::menu`].
    /// Uninhabited rather than a placeholder enum, so [`Puppet::apply`] is a
    /// match on nothing and the compiler agrees there is no case to handle.
    type MenuAction = core::convert::Infallible;
    type Summary = Summary;

    const NAME: &'static str = "puppet";

    fn menus() -> Menus {
        crate::menu::menus()
    }

    fn tick(&mut self, _gpu: &mut Gpu, tick_dt: f64) {
        // `ActionMap` holds its timers in `f32`, which is the precision an
        // input edge is worth.
        #[allow(clippy::cast_possible_truncation)]
        self.actions.begin_tick(tick_dt as f32);
        for (key, pressed) in self.pending_keys.drain(..) {
            self.actions.key_event(key, pressed);
        }
        // The yaw goes with the buttons: what the player asked for is "forward",
        // and forward only means something beside the angle they were looking
        // along when they asked.
        self.game
            .set_controls(controls(&self.actions, self.follow.yaw()));
        self.game.tick();
    }

    fn key_event(&mut self, key: KeyCode, pressed: bool) {
        // Queued rather than fed straight in: the map's edges belong to the
        // tick, not to the frame. See [`Puppet::pending_keys`].
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
        // **The camera turns on the wall clock**, so a paused frame can still be
        // looked around from — and so the turn is smooth on a machine whose
        // frames do not line up with its ticks.
        let (yaw, pitch) = camera_turn(&self.actions, frame.render_dt.as_secs_f32());
        if yaw != 0.0 || pitch != 0.0 {
            self.follow.turn(yaw, pitch);
        }

        self.render_state = self.game.render_state();
        self.stats = self.game.stats();

        // The pose runs on the **frame** clock and off the simulation's
        // *measured* speed — see [`crate::anim`] for why measured and not
        // commanded. `render_dt` and not `dt`, so a paused loop holds the pose
        // where the simulation left it rather than walking on the spot.
        #[allow(clippy::cast_possible_truncation)]
        let speed = self.render_state.speed as f32;
        let render_dt = frame.render_dt.as_secs_f32();
        self.anim.advance(render_dt, speed);
        gpu.set_palette(self.anim.palette());
        self.report_pose(render_dt, speed);

        gpu.place_character(self.render_state.position, self.render_state.facing);
        // The simulation is `f64` and the renderer's camera is `f32`; this is
        // the one place the two meet.
        #[allow(clippy::cast_possible_truncation)]
        let focus = Vec3::new(
            self.render_state.position.x as f32,
            self.render_state.feet as f32 + crate::camera::FOCUS_HEIGHT,
            self.render_state.position.z as f32,
        );
        gpu.set_camera(self.follow.camera(focus));
        // The sun turns on the simulation's clock, so the shadows on the map
        // stop where they are while the loop is paused — see [`Map::sun`].
        gpu.set_sun(self.map.sun(self.render_state.elapsed));

        self.page = crate::page::draw(
            draw_list,
            gpu.atlas(),
            gpu.extent(),
            &self.render_state,
            self.anim.blend(),
        );
    }

    /// **Puppet's one module, and no second.**
    ///
    /// No network section: this sample runs over `InMemoryTransport` and has no
    /// connection to report on. No audio section either — milestone 1 plays
    /// nothing, and a section that said so would be a module with no system
    /// behind it. What it does have is the character, and every row in it is a
    /// number [`crcbl::phys::CharacterController`] produced.
    fn debug_sections(&self, panel: &mut crcbl::ui::DebugPanel) {
        panel.add(&self.stats);
    }

    fn summary(&self, run: RunSummary) -> Summary {
        Summary {
            run,
            feet: [
                self.stats.position.x,
                self.stats.feet,
                self.stats.position.z,
            ],
            climbed: self.stats.climbed,
            blocked: self.stats.blocked,
            commands: self.page.commands,
        }
    }

    fn log_summary(summary: &Summary) {
        crcbl::log::info!(
            "puppet: {} frames, {} ticks, feet at {:.2} {:.2} {:.2}, {} step(s) climbed, \
             {} tick(s) blocked, {} overlay commands ({:?})",
            summary.run.frames,
            summary.run.ticks,
            summary.feet[0],
            summary.feet[1],
            summary.feet[2],
            summary.climbed,
            summary.blocked,
            summary.commands,
            summary.run.exit,
        );
    }
}

// ---- polled start-up ---------------------------------------------------------

crcbl::impl_pending_loop!(
    running: Loop,
    gpu: Gpu,
    options: Options,
    error: PuppetError,
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
mod tests {
    use super::*;
    use crcbl::engine::{ExitReason, PAUSE_KEY};
    use crcbl::shell::{HeadlessShell, ShellBackend as Backend};
    use crcbl_sample_test::{headless_common, ui_text};

    fn scripted(options: &Options) -> Loop<HeadlessShell> {
        with_shell(Box::new(HeadlessShell::new()), options).expect("headless always starts")
    }

    fn headless(frames: u64) -> Options {
        Options {
            common: headless_common(crate::game::DEFAULT_TICK_HZ, frames),
            ..Options::default()
        }
    }

    /// **A headless run walks the circuit and draws it.** The one check that
    /// says the whole bundle — server, controller, renderer and overlay — came
    /// up and produced a frame with something on it.
    #[test]
    fn a_headless_run_walks_the_circuit_and_draws_it() {
        let summary = run(&headless(120)).expect("the null backend always runs");
        assert_eq!(summary.run.frames, 120);
        assert_eq!(summary.run.exit, ExitReason::FrameBudget);
        assert!(summary.run.ticks > 0, "no tick ran");
        assert!(
            summary.commands > 0,
            "the run presented frames with nothing on them",
        );
        let travelled =
            (summary.feet[0] - crate::map::SPAWN.x).hypot(summary.feet[2] - crate::map::SPAWN.z);
        assert!(
            travelled > 0.5,
            "the character stayed within {travelled:.2} m of the spawn",
        );
        assert!(
            summary.feet[1].abs() < 0.05,
            "the circuit left the flat, at {:.2} m",
            summary.feet[1],
        );
    }

    /// **A map read off disk reaches the simulation and the frame.**
    ///
    /// The end of the `--scene` path, and the only place it is a *run* rather
    /// than a parse: `crate::args` proves the directory reaches
    /// [`Options::map`], and this proves that field reaches the stage the
    /// character stands on. A binding that read `Map::built_in` on the way to
    /// [`Game::new`] would pass every test in `crate::map` and walk the
    /// committed blockout while the flag said otherwise.
    ///
    /// **The renderer's half is not what this reads.** The whole run goes
    /// through `Gpu::from_context`, so a map that could not be made resident is
    /// a failure here — but the picture is not, and nothing in this summary
    /// would change if the frame drew the committed blockout beside the slab the
    /// character walks. `docs/backlog.md` carries that gap.
    ///
    /// The one-slab scene puts the spawn where the committed one does not, so
    /// where the run *ends* is the answer: the circuit walks a couple of metres
    /// from wherever it started, and the two spawns are sixteen apart.
    #[test]
    fn a_scene_directory_is_the_map_the_run_actually_walks() {
        let dir = std::env::temp_dir().join(format!("puppet-run-{}.scn", std::process::id()));
        std::fs::create_dir_all(dir.join("sys")).expect("the temp dir is writable");
        std::fs::write(
            dir.join("scene.ron"),
            "Scene(format: 0, name: \"slab\", systems: [\"surfaces\", \"spawn\", \"sun\"])",
        )
        .expect("the temp dir is writable");
        std::fs::write(
            dir.join("env.ron"),
            "Env(camera: (position: (0.0, 2.0, 6.0), look_at: (0.0, 1.0, 0.0)), \
             ambient: (0.1, 0.11, 0.14))",
        )
        .expect("the temp dir is writable");
        std::fs::write(
            dir.join("sys").join("surfaces.ron"),
            "Chunk(system: \"surfaces\", entities: [(0, (label: \"slab\", \
             position: (0.0, -1.0, 0.0), shape: Platform(width: 40.0, depth: 40.0, \
             height: 1.0), tint: (0.3, 0.31, 0.33)))])",
        )
        .expect("the temp dir is writable");
        std::fs::write(
            dir.join("sys").join("spawn.ron"),
            "Chunk(system: \"spawn\", entities: [(1, (position: (-9.0, 0.0, -7.0), \
             facing: 0.0))])",
        )
        .expect("the temp dir is writable");
        std::fs::write(
            dir.join("sys").join("sun.ron"),
            "Chunk(system: \"sun\", entities: [(2, (elevation: 0.78, \
             color: (1.0, 0.97, 0.9), intensity: 2.2, period: 45.0))])",
        )
        .expect("the temp dir is writable");

        let map = crate::map::Map::read_dir(dir.to_str().expect("utf-8"))
            .expect("the slab scene is a puppet map");
        let options = Options {
            map,
            ..headless(60)
        };
        let summary = run(&options).expect("the null backend always runs");
        let from_slab = (summary.feet[0] - (-9.0_f64)).hypot(summary.feet[2] - (-7.0_f64));
        let from_committed =
            (summary.feet[0] - crate::map::SPAWN.x).hypot(summary.feet[2] - crate::map::SPAWN.z);
        assert!(
            from_slab < 4.0,
            "the run ended {from_slab:.2} m from the slab's spawn",
        );
        assert!(
            from_committed > 4.0,
            "the run walked the committed blockout, {from_committed:.2} m from its spawn",
        );
        assert!(
            summary.feet[1].abs() < 0.05,
            "the slab is flat and the character ended at {:.2} m",
            summary.feet[1],
        );
    }

    /// **Two identical runs agree exactly**, which is what a fixed timestep over
    /// a scripted circuit is for.
    #[test]
    fn a_headless_run_is_deterministic() {
        let first = run(&headless(60)).expect("headless runs everywhere");
        let second = run(&headless(60)).expect("headless runs everywhere");
        assert_eq!(first, second, "two identical runs must agree exactly");
        assert_eq!(first.run.backend, Backend::Headless);
    }

    /// **The camera keys turn the view and the walk keys do not.** They are read
    /// on the frame's clock rather than the tick's, so this is also the check
    /// that they are read at all: an action declared and never polled is silent.
    #[test]
    fn the_camera_keys_turn_the_view_and_the_walk_keys_leave_it_alone() {
        let mut engine = scripted(&headless(64));
        let window = engine.window();
        engine.frame().expect("a frame");
        let opened = engine.game().follow().yaw();

        engine
            .shell_mut()
            .key_press(window, KeyCode::KeyE)
            .expect("the window is live");
        for _ in 0..8 {
            engine.frame().expect("a frame");
        }
        let turned = engine.game().follow().yaw();
        assert!(turned > opened, "E left the yaw at {turned}");

        engine
            .shell_mut()
            .key_release(window, KeyCode::KeyE)
            .expect("the window is live");
        engine
            .shell_mut()
            .key_press(window, KeyCode::KeyW)
            .expect("the window is live");
        for _ in 0..8 {
            engine.frame().expect("a frame");
        }
        assert!(
            (engine.game().follow().yaw() - turned).abs() < 1e-6,
            "walking turned the camera",
        );
        engine.finish(ExitReason::FrameBudget).expect("teardown");
    }

    /// **A held walk key reaches the simulation and moves the character**, which
    /// is the whole path this sample exists to prove: shell event → action map →
    /// wire → module → `move_and_slide`. The same claim
    /// `web/tools/browser-e2e.mjs` makes in a browser, made here where a failure
    /// names the step.
    #[test]
    fn a_held_key_reaches_the_controller_and_moves_the_character() {
        let mut engine = scripted(&headless(240));
        let window = engine.window();
        // Let the circuit run, then take it over: the first movement key is
        // what ends it, and the check below is about the player's own walk.
        for _ in 0..8 {
            engine.frame().expect("a frame");
        }
        engine
            .shell_mut()
            .key_press(window, KeyCode::KeyW)
            .expect("the window is live");
        for _ in 0..60 {
            engine.frame().expect("a frame");
        }
        let walked = engine.game().game().render_state();
        assert!(!walked.patrolling, "the circuit survived a key press");

        engine
            .shell_mut()
            .key_release(window, KeyCode::KeyW)
            .expect("the window is live");
        for _ in 0..60 {
            engine.frame().expect("a frame");
        }
        let stopped = engine.game().game().render_state();
        assert!(
            (stopped.position - walked.position).length() < 0.01,
            "it kept moving after the key came up: {:?} then {:?}",
            walked.position,
            stopped.position,
        );
        engine.finish(ExitReason::FrameBudget).expect("teardown");
    }

    /// **The panel renders with no network module.** The sections puppet has are
    /// the frame's, the GPU's where the device has timestamp queries, and this
    /// sample's own one. Nothing else, and no configuration decided that.
    #[test]
    fn the_overlay_is_composed_of_exactly_the_modules_puppet_has() {
        let mut options = headless(8);
        options.common.debug_overlay = Some(true);
        let mut engine = scripted(&options);
        engine.frame().expect("a frame");
        engine.frame().expect("a frame");

        let titles: Vec<&str> = engine
            .debug()
            .panel
            .sections()
            .iter()
            .map(crcbl::ui::DebugSection::title)
            .collect();
        let expected: &[&str] = if engine.gpu().timings().is_some() {
            &["frame", "gpu", "counters", "puppet"]
        } else {
            &["frame", "counters", "puppet"]
        };
        assert_eq!(titles, expected, "no module appears that no system offered");

        let drawn = ui_text(engine.gpu().draw_list());
        for row in ["frame", "climbed", "blocked", "ground"] {
            assert!(drawn.iter().any(|t| t == row), "missing {row}: {drawn:?}");
        }
        assert!(
            drawn.iter().any(|t| t == "GROUND"),
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
        engine.frame().expect("a frame");
        engine.frame().expect("a frame");
        let running = engine.game().game().ticks_run();
        assert!(running > 0, "the simulation never ticked");
        assert_eq!(engine.menu_kind(), MenuKind::None);

        engine
            .shell_mut()
            .key_press(window, PAUSE_KEY)
            .expect("the window is live");
        engine.frame().expect("a frame");
        engine.frame().expect("a frame");
        assert!(engine.is_paused());
        assert_eq!(engine.menu_kind(), MenuKind::Paused);
        assert_eq!(
            engine.game().game().ticks_run(),
            running,
            "a paused loop runs no ticks",
        );
        assert!(
            ui_text(engine.gpu().draw_list())
                .iter()
                .any(|t| t == "GROUND"),
            "the overlay is drawn behind the panel",
        );
        engine.finish(ExitReason::FrameBudget).expect("teardown");
    }
}
