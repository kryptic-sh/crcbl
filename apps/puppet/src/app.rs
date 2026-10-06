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
//! What is left here is start-up, because a window's title is this sample's;
//! the input queues, because the map's edges are the tick's; the camera,
//! because it is presentation; and the trait methods, because they are what a
//! hosted game is. The action map itself is [`crate::bindings`]', because a
//! keyboard or a pad is not something [`crate::game`] should know about.
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

use crcbl::client::ClientQueryWorld;
use crcbl::core::input::KeyCode;
use crcbl::engine::{Booted, Clock, FrameInfo, HostedGame, RunSummary, wait_for_configure};
use crcbl::input::{ActionMap, GamepadEvent};
use crcbl::math::Vec3;
use crcbl::prelude::*;
use crcbl::rebind::{Capture, Rebinder};
use crcbl::shell::{DisplayMode, WindowId};
use crcbl::store::profile::ProfileStore;

use crate::anim::Animator;
use crate::audio::Audio;
use crate::camera::Follow;
use crate::game::{Game, RenderState, Stats};
use crate::gpu::Gpu;
use crate::map::Map;
use crate::menu::{MenuKind, Menus, PuppetAction};
use crate::page::PageStats;

pub use crate::args::Options;

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
    /// How many footsteps the animation raised over the whole run.
    pub footsteps: u64,
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
    /// The keyboard and the pad, resolved into
    /// [`Controls`](crate::game::Controls) once per tick — inside the rebind
    /// flow that keeps the player's run and jump binds in their profile. See
    /// [`crate::bindings`].
    controls: Rebinder,
    /// Key events from the shell pump, replayed after `ActionMap::begin_tick`.
    ///
    /// The pump runs once per **frame** and the map's edge flags are per
    /// **tick**, and `begin_tick` clears those flags — so an event fed before it
    /// has its press edge erased. Queueing here and replaying after is the order
    /// the map asks for, and it is what makes a frame that runs no ticks
    /// lossless.
    pending_keys: Vec<(KeyCode, bool)>,
    /// Pad events from the loop's pad poll, replayed after the keys, for the
    /// reason [`Puppet::pending_keys`] queues a key.
    ///
    /// Every event, in order, rather than the last snapshot only: a press and
    /// its release inside one frame are two edges — a tap of jump — and a
    /// connection carries the pad's family, which the prompt names buttons
    /// after. After the keys because the loop polls the pads after the shell's
    /// events, so that is the order the player made them in.
    pending_pads: Vec<GamepadEvent>,
    /// The control prompt under the panel, for the device the player last
    /// used — [`crate::bindings::prompt`]. Rebuilt when the map's last device
    /// changes and when a rebind moves a label, not every frame.
    prompt: String,
    /// Which of puppet's panels the pause shows: the pause panel itself, or
    /// the controls overlay opened from it. The clash panel is not one of
    /// them: it is shown over the overlay while the rebind flow has a clash in
    /// it.
    panel: MenuKind,
    /// The third-person camera. **Presentation**: it never crosses the wire, and
    /// the only thing the simulation is told about it is its yaw.
    follow: Follow,
    /// What the camera's boom is swept against and the beacons are heard
    /// through: the map's colliders, built once from the same [`Map`] the
    /// server's controller walks. **The client's own copy**, because the
    /// stage's world is the simulation's.
    query: ClientQueryWorld,
    /// The beacons behind the mounds, or `None` on a headless run, which opens
    /// no device and plays nothing. See [`crate::audio`].
    audio: Option<Audio>,
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
    /// The character's rig, posed from the animation state the simulation
    /// left.
    ///
    /// **Presentation, like the camera**: it samples the state machine's state
    /// and never steps it, nothing in it crosses the wire, and the tick would
    /// run the same without it. The animation rules in
    /// `docs/notes/simulation.md` put pose evaluation on the client, and this
    /// is where puppet's client is.
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
    /// The `[POSE]` line: what the animation did with the state the server's
    /// state machine left.
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
    /// number nothing but the animation can move:
    ///
    /// * `blend` — how far the character is out of its idle stance, 0 in idle
    ///   and 1 running or jumping ([`crate::anim::Animator::blend`]). The state
    ///   machine leaves idle when `speed`, measured from the controller's own
    ///   displacement, passes the asset's threshold, and fades between them.
    /// * `mid` — how many frames the blend has spent **strictly between** the
    ///   two ends. The heartbeat is a second apart and a fade takes less than
    ///   that, so the counter is what says the weight swept rather than
    ///   snapped.
    /// * `dev` — how far the pose has carried a joint from the rest pose, in
    ///   metres. It sweeps while the character moves and holds still while it
    ///   stands, because [`crate::rig::idle`] is a stance.
    ///
    /// `state` is the state machine's current state, for a reader.
    fn report_pose(&mut self, render_dt: f32) {
        self.pose_report += render_dt;
        if self.pose_report < POSE_REPORT_S {
            return;
        }
        self.pose_report = 0.0;
        crcbl::log::info!(
            "[POSE] speed: {:.2}  blend: {:.2}  mid: {}  dev: {:.3}  state: {}",
            self.render_state.speed,
            self.anim.blend(),
            self.anim.partial(),
            self.anim.deviation(),
            self.anim.state_name(),
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
    let (statics, colliders) = options.map.world_with_ids();
    let audio = (!options.common.headless)
        .then(|| Audio::open(crate::audio::materials(options.map.surfaces(), &colliders)));
    // The profile follows the settings file's rule: the player's own natively
    // and in a browser, nowhere from a headless run.
    let controls =
        crate::bindings::open(ProfileStore::for_app(Puppet::NAME, options.common.headless));
    let prompt = crate::bindings::prompt(controls.actions());
    Ok(Loop::new(
        booted,
        Puppet {
            game,
            map: options.map.clone(),
            controls,
            pending_keys: Vec::new(),
            pending_pads: Vec::new(),
            prompt,
            panel: MenuKind::Paused,
            follow: Follow::default(),
            query: ClientQueryWorld::new(statics),
            audio,
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

    /// The control prompt the overlay draws, for this crate's own tests.
    pub fn prompt(&self) -> &str {
        &self.prompt
    }

    /// The rebind flow and the map inside it.
    pub const fn controls(&self) -> &Rebinder {
        &self.controls
    }

    /// Keeps this run's binds in `store` instead, read from it now — what a
    /// test uses, so a restart can be two runs over one temp directory.
    pub fn keep_binds_in(&mut self, store: ProfileStore) {
        self.controls = crate::bindings::open(store);
        self.prompt = crate::bindings::prompt(self.controls.actions());
    }

    /// The prompt for the device the map now names, after anything that may
    /// have moved a label.
    fn refresh_prompt(&mut self) {
        self.prompt = crate::bindings::prompt(self.controls.actions());
    }
}

/// Puppet's half of the frame, and nothing else.
impl HostedGame for Puppet {
    type Error = crate::game::GameError;
    type Gpu = Gpu;
    type MenuKind = MenuKind;
    /// The controls overlay's rows — see [`crate::menu`].
    type MenuAction = PuppetAction;
    type Summary = Summary;

    const NAME: &'static str = "puppet";

    fn menus() -> Menus {
        crate::menu::menus()
    }

    fn tick(&mut self, _gpu: &mut Gpu, tick_dt: f64) {
        let actions = self.controls.actions_mut();
        // `ActionMap` holds its timers in `f32`, which is the precision an
        // input edge is worth.
        #[allow(clippy::cast_possible_truncation)]
        actions.begin_tick(tick_dt as f32);
        for (key, pressed) in self.pending_keys.drain(..) {
            actions.key_event(key, pressed);
        }
        for event in self.pending_pads.drain(..) {
            actions.gamepad_event(&event);
        }
        // Read on the tick whose replay raised it: the next `begin_tick`
        // clears the edge, and a frame can run two ticks before it draws.
        let swapped = actions.last_device_changed();
        // The yaw goes with the buttons: what the player asked for is "forward",
        // and forward only means something beside the angle they were looking
        // along when they asked.
        self.game.set_controls(crate::bindings::controls(
            self.controls.actions(),
            self.follow.yaw(),
        ));
        self.game.tick();
        if swapped {
            self.refresh_prompt();
        }
    }

    fn key_event(&mut self, key: KeyCode, pressed: bool) {
        // A press heard while the overlay listens is the player's answer, not
        // play, so it is not queued — or resuming would replay a captured
        // Space as a jump. A release always is: one the map never saw held is
        // harmless, and one it did see is owed.
        let answer = pressed && self.controls.listening();
        self.controls.key(key, pressed);
        if !answer {
            // Queued rather than fed straight in: the map's edges belong to the
            // tick, not to the frame. See [`Puppet::pending_keys`].
            self.pending_keys.push((key, pressed));
        }
    }

    /// The pads, queued like the keys and shown to the rebind flow — every
    /// event, so a button already held when the overlay starts listening is
    /// known to be held.
    ///
    /// A snapshot heard while the overlay listens is not queued, for
    /// [`Puppet::key_event`]'s reason. Nothing is lost by it: a snapshot is
    /// the pad's whole state, so the next one the map hears carries every
    /// release the skipped one had.
    fn gamepad_event(&mut self, event: &GamepadEvent) {
        let answer = self.controls.listening() && matches!(event, GamepadEvent::State { .. });
        self.controls.pad(event);
        if !answer {
            self.pending_pads.push(*event);
        }
    }

    /// While the controls overlay listens for the input to bind.
    ///
    /// Keys and pad buttons only: puppet binds no pointer input and feeds its
    /// map none, so a mouse button bound here would be an action nothing could
    /// press. A click while listening reaches no panel and binds nothing.
    fn captures_input(&self) -> bool {
        self.controls.listening()
    }

    /// The map the console's `bind` and `unbind` rebind.
    ///
    /// The same map the queued inputs above are replayed into, so a rebind
    /// typed at the console moves the key this game actually plays on rather
    /// than a copy of it — and the profile follows, by
    /// [`Rebinder::persist`]'s rule.
    fn actions(&mut self) -> Option<&mut ActionMap> {
        Some(self.controls.actions_mut())
    }

    fn menu_action(id: crcbl::ui::WidgetId) -> Option<PuppetAction> {
        PuppetAction::of(id)
    }

    fn apply(&mut self, action: PuppetAction) {
        match action {
            PuppetAction::Controls => self.panel = MenuKind::Controls,
            PuppetAction::Back => {
                self.controls.cancel();
                self.panel = MenuKind::Paused;
            }
            PuppetAction::Rebind(index) => self.controls.listen(index),
            PuppetAction::ResetControls => self.controls.reset(),
            PuppetAction::Swap => self.controls.swap(),
            PuppetAction::Cancel => self.controls.cancel(),
        }
    }

    /// The profile written if the binds moved, the overlay's rows and both
    /// panels' captions refreshed, and the panel this frame shows.
    ///
    /// The write comes first, on every frame and not only a paused one, so a
    /// console `bind` typed mid-walk reaches the profile and the prompt too.
    /// Resuming leaves the overlay: the next pause opens on the pause panel,
    /// and a clash nobody answered is dropped as `CANCEL` would drop it.
    fn menu_kind(&mut self, menus: &mut Menus, paused: bool) -> MenuKind {
        if self.controls.persist() {
            self.refresh_prompt();
        }
        if !paused {
            self.controls.cancel();
            self.panel = MenuKind::Paused;
            return MenuKind::None;
        }
        if let Some(menu) = menus.get_mut(MenuKind::Controls) {
            self.controls.refresh_page(crate::bindings::IDS, menu);
        }
        if let Some(menu) = menus.get_mut(MenuKind::Conflict) {
            self.controls.refresh_conflict(menu);
        }
        match self.controls.capture() {
            Capture::Conflict { .. } => MenuKind::Conflict,
            Capture::Idle | Capture::Listening { .. } => self.panel,
        }
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
        let (yaw, pitch) =
            crate::bindings::camera_turn(self.controls.actions(), frame.render_dt.as_secs_f32());
        if yaw != 0.0 || pitch != 0.0 {
            self.follow.turn(yaw, pitch);
        }

        self.render_state = self.game.render_state();
        self.stats = self.game.stats();

        // The pose is sampled from the state machine's state as the last tick
        // left it — see [`crate::anim`] for why the client samples and never
        // steps. A paused loop runs no tick, so the pose holds where the
        // simulation left it rather than walking on the spot.
        self.anim.advance(&self.render_state.anim);
        gpu.set_palette(self.anim.palette());
        self.report_pose(frame.render_dt.as_secs_f32());

        gpu.place_character(self.render_state.position, self.render_state.facing);
        // The simulation is `f64` and the renderer's camera is `f32`; this is
        // the one place the two meet.
        #[allow(clippy::cast_possible_truncation)]
        let focus = Vec3::new(
            self.render_state.position.x as f32,
            self.render_state.feet as f32 + crate::camera::FOCUS_HEIGHT,
            self.render_state.position.z as f32,
        );
        let camera = self.follow.camera(focus, &mut self.query);
        // The ear rides the camera, through the same world its boom swept.
        if let Some(audio) = &mut self.audio {
            audio.hear_from(&camera, &mut self.query);
        }
        gpu.set_camera(camera);
        // The sun turns on the simulation's clock, so the shadows on the map
        // stop where they are while the loop is paused — see [`Map::sun`].
        gpu.set_sun(self.map.sun(self.render_state.elapsed));

        self.page = crate::page::draw(
            draw_list,
            gpu.atlas(),
            gpu.extent(),
            &self.render_state,
            self.anim.blend(),
            self.anim.state_name(),
            &self.prompt,
        );
    }

    /// **Puppet's one module, and no second.**
    ///
    /// No network section: this sample runs over `InMemoryTransport` and has no
    /// connection to report on. No audio section either: the beacons are two
    /// held voices with nothing to count, and a headless run, which is what
    /// this panel is tested on, has no audio at all. What it does have is the
    /// character, and every row in it is a number
    /// [`crcbl::phys::CharacterController`] produced.
    fn debug_sections(&self, panel: &mut crcbl::ui::DebugPanel) {
        panel.add(&self.stats);
    }

    /// The beacons' mixer, which the console's `[engine.audio]` keys move —
    /// refused on a headless run, which has none.
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
            feet: [
                self.stats.position.x,
                self.stats.feet,
                self.stats.position.z,
            ],
            climbed: self.stats.climbed,
            blocked: self.stats.blocked,
            footsteps: self.stats.footsteps,
            commands: self.page.commands,
        }
    }

    fn log_summary(summary: &Summary) {
        crcbl::log::info!(
            "puppet: {} frames, {} ticks, feet at {:.2} {:.2} {:.2}, {} step(s) climbed, \
             {} tick(s) blocked, {} footstep(s), {} overlay commands ({:?})",
            summary.run.frames,
            summary.run.ticks,
            summary.feet[0],
            summary.feet[1],
            summary.feet[2],
            summary.climbed,
            summary.blocked,
            summary.footsteps,
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
        assert!(
            summary.footsteps > 0,
            "two seconds of walking the circuit raised no footstep",
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

    /// **Shift and Space reach the state machine**: the run key carries the
    /// character into the run state, and a tap of the jump key — down and up
    /// again — puts it in the jump. The shell-to-server path for the two new
    /// actions, made where a failure names the step.
    #[test]
    fn the_run_and_jump_keys_reach_the_state_machine() {
        let mut engine = scripted(&headless(240));
        let window = engine.window();
        for key in [KeyCode::KeyW, KeyCode::ShiftLeft] {
            engine
                .shell_mut()
                .key_press(window, key)
                .expect("the window is live");
        }
        for _ in 0..60 {
            engine.frame().expect("a frame");
        }
        assert_eq!(engine.game().game().stats().anim, "run");

        engine
            .shell_mut()
            .key_press(window, KeyCode::Space)
            .expect("the window is live");
        engine
            .shell_mut()
            .key_release(window, KeyCode::Space)
            .expect("the window is live");
        let mut jumped = false;
        for _ in 0..10 {
            engine.frame().expect("a frame");
            jumped |= engine.game().game().stats().anim == "jump";
        }
        assert!(jumped, "a tap of Space never put the character in the jump");
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

    // ---- the pad, the prompt and the controls overlay ------------------------

    use crate::menu::PuppetAction;
    use crcbl::input::{Binding, GamepadId, GamepadSnapshot, PadAxis, PadButton, PadKind};
    use crcbl::rebind::ProfileWrite;
    use crcbl::store::profile::PROFILE_FILE;
    use crcbl::store::record::Backing;
    use crcbl_sample_test::ScriptedPads;

    /// The pad these tests hold: an Xbox one, so the prompt's words are known.
    const PAD: GamepadId = GamepadId(1);

    /// A scripted run with a scripted pad plugged in, and that pad's handle.
    fn with_pad(frames: u64) -> (Loop<HeadlessShell>, ScriptedPads) {
        let mut engine = scripted(&headless(frames));
        let pads = ScriptedPads::default();
        engine.set_pad_source(Some(Box::new(pads.clone())));
        pads.push([GamepadEvent::Connected {
            id: PAD,
            kind: PadKind::Xbox,
        }]);
        (engine, pads)
    }

    /// A snapshot of [`PAD`], edited from neutral.
    fn pad_state(edit: impl FnOnce(&mut GamepadSnapshot)) -> GamepadEvent {
        let mut snapshot = GamepadSnapshot::neutral(PadKind::Xbox);
        edit(&mut snapshot);
        GamepadEvent::State { id: PAD, snapshot }
    }

    fn frames(engine: &mut Loop<HeadlessShell>, count: usize) {
        for _ in 0..count {
            engine.frame().expect("a frame");
        }
    }

    fn tap(engine: &mut Loop<HeadlessShell>, key: KeyCode) {
        let window = engine.window();
        engine
            .shell_mut()
            .key_press(window, key)
            .expect("the window is live");
        engine
            .shell_mut()
            .key_release(window, key)
            .expect("the window is live");
    }

    /// **The pad drives the state machine the way the keys do**: the left
    /// stick pushed up with the right bumper held carries the character into
    /// the run, a tap of South — down and up inside one poll — puts it in the
    /// jump, and letting go brings it back to idle. The shell-to-server path
    /// of `the_run_and_jump_keys_reach_the_state_machine`, from the loop's pad
    /// poll instead.
    #[test]
    fn the_pad_drives_idle_run_and_jump_like_the_keys() {
        let (mut engine, pads) = with_pad(480);
        let running = pad_state(|pad| {
            pad.axes[PadAxis::LeftY as usize] = 1.0;
            pad.buttons.insert(crate::bindings::RUN_PAD_BUTTON);
        });
        pads.push([running]);
        frames(&mut engine, 60);
        assert_eq!(engine.game().game().stats().anim, "run");
        assert!(
            !engine.game().game().render_state().patrolling,
            "the stick did not take the controls from the circuit"
        );

        let jumping = pad_state(|pad| {
            pad.axes[PadAxis::LeftY as usize] = 1.0;
            pad.buttons.insert(crate::bindings::RUN_PAD_BUTTON);
            pad.buttons.insert(PadButton::South);
        });
        pads.push([jumping, running]);
        let mut jumped = false;
        for _ in 0..10 {
            engine.frame().expect("a frame");
            jumped |= engine.game().game().stats().anim == "jump";
        }
        assert!(jumped, "a tap of South never put the character in the jump");

        pads.push([pad_state(|_| {})]);
        let mut idle = false;
        for _ in 0..240 {
            engine.frame().expect("a frame");
            if engine.game().game().stats().anim == "idle" {
                idle = true;
                break;
            }
        }
        assert!(idle, "letting go of the pad never brought the idle back");
        engine.finish(ExitReason::FrameBudget).expect("teardown");
    }

    /// **The prompt follows the device the player last used, and a drifting
    /// stick does not take it.** A key keeps the keyboard's words; a press on
    /// the pad swaps the line the frame draws to the pad's; a key swaps it to
    /// the keyboard again; and a stick resting a little off centre swaps
    /// nothing back.
    #[test]
    fn the_prompt_follows_the_last_device_and_not_a_drifting_stick() {
        let (mut engine, pads) = with_pad(64);
        let keyboard = crate::bindings::prompt(&crate::bindings::action_map());
        let drawn = |engine: &Loop<HeadlessShell>| {
            let prompt = engine.game().prompt().to_owned();
            assert!(
                ui_text(engine.gpu().draw_list()).contains(&prompt),
                "the frame did not draw the prompt {prompt:?}"
            );
            prompt
        };

        tap(&mut engine, KeyCode::KeyD);
        frames(&mut engine, 2);
        assert_eq!(drawn(&engine), keyboard);

        pads.push([
            pad_state(|pad| pad.buttons.insert(PadButton::South)),
            pad_state(|_| {}),
        ]);
        frames(&mut engine, 2);
        let on_the_pad = drawn(&engine);
        assert!(
            on_the_pad.starts_with("Left stick walk   RB run   A jump"),
            "a pad press left the prompt at {on_the_pad:?}"
        );

        tap(&mut engine, KeyCode::KeyD);
        frames(&mut engine, 2);
        assert_eq!(drawn(&engine), keyboard, "a key did not take it back");

        // Inside the stick's own dead zone and well inside the activity
        // threshold: what a worn stick reads at rest.
        pads.push([pad_state(|pad| {
            pad.axes[PadAxis::LeftX as usize] = 0.15;
            pad.axes[PadAxis::LeftY as usize] = -0.1;
        })]);
        frames(&mut engine, 4);
        assert_eq!(drawn(&engine), keyboard, "a drifting stick took the prompt");
        engine.finish(ExitReason::FrameBudget).expect("teardown");
    }

    /// A directory of this test's own under the system temp directory, empty.
    fn scratch(name: &str) -> std::path::PathBuf {
        let dir = std::env::temp_dir().join(format!("puppet-{name}-{}", std::process::id()));
        if dir.exists() {
            std::fs::remove_dir_all(&dir).expect("a scratch dir of this test's own");
        }
        std::fs::create_dir_all(&dir).expect("the temp dir is writable");
        dir
    }

    /// A scripted run whose binds are kept in `dir`, standing in for the
    /// player's config directory — what a "restart" opens twice.
    fn run_over_profile(dir: &std::path::Path, frames: u64) -> Loop<HeadlessShell> {
        let mut engine = scripted(&headless(frames));
        engine.game_mut().keep_binds_in(ProfileStore::open(
            Backing::Native(dir.to_path_buf()),
            PROFILE_FILE,
        ));
        engine
    }

    /// The row of [`crate::bindings::ROWS`] for `name`.
    fn row(name: &str) -> usize {
        crate::bindings::ROWS
            .iter()
            .position(|row| row.name == name)
            .unwrap_or_else(|| panic!("no row {name}"))
    }

    /// Pauses, opens the controls overlay from the pause panel and starts
    /// listening for `name`, as ENTER on the two rows does.
    fn listen_for(engine: &mut Loop<HeadlessShell>, name: &str) {
        let window = engine.window();
        engine
            .shell_mut()
            .key_press(window, PAUSE_KEY)
            .expect("the window is live");
        frames(engine, 2);
        assert_eq!(engine.menu_kind(), MenuKind::Paused);
        let open = Puppet::menu_action(crate::bindings::CONTROLS_ID).expect("CONTROLS fires");
        engine.game_mut().apply(open);
        frames(engine, 1);
        assert_eq!(engine.menu_kind(), MenuKind::Controls);
        let listen = Puppet::menu_action(crate::bindings::IDS.action(row(name)))
            .expect("an action row fires");
        engine.game_mut().apply(listen);
        assert!(
            engine.game().captures_input(),
            "the row did not start listening"
        );
    }

    fn jump_bindings(engine: &Loop<HeadlessShell>) -> Vec<Binding> {
        engine
            .game()
            .controls()
            .actions()
            .bindings(crate::bindings::ACTION_JUMP)
            .expect("jump is declared")
            .to_vec()
    }

    /// **A rebind made in the overlay is there after a restart, and is what
    /// the character jumps on.** Listening on `JUMP`, a press of `J` replaces
    /// Space and keeps the pad's South; the profile is written; and a second
    /// run over the same directory opens with it, prints it in the prompt, and
    /// jumps on a tap of `J`.
    #[test]
    fn a_rebind_in_the_overlay_survives_a_restart() {
        let dir = scratch("rebind-restart");
        let rebound = vec![
            Binding::Key(KeyCode::KeyJ),
            Binding::PadButton(PadButton::South),
        ];

        let mut engine = run_over_profile(&dir, 64);
        frames(&mut engine, 2);
        listen_for(&mut engine, "jump");
        tap(&mut engine, KeyCode::KeyJ);
        frames(&mut engine, 2);
        assert!(
            !engine.game().captures_input(),
            "a captured key left it listening"
        );
        assert_eq!(engine.menu_kind(), MenuKind::Controls);
        assert_eq!(jump_bindings(&engine), rebound);
        assert_eq!(engine.game().controls().saved(), &ProfileWrite::Saved);
        engine.finish(ExitReason::FrameBudget).expect("teardown");

        let mut restarted = run_over_profile(&dir, 64);
        assert_eq!(jump_bindings(&restarted), rebound, "the restart lost it");
        assert!(
            restarted.game().prompt().contains("J jump"),
            "the prompt still names the old key: {:?}",
            restarted.game().prompt()
        );
        tap(&mut restarted, KeyCode::KeyJ);
        let mut jumped = false;
        for _ in 0..10 {
            restarted.frame().expect("a frame");
            jumped |= restarted.game().game().stats().anim == "jump";
        }
        assert!(jumped, "a tap of the rebound key never jumped");
        restarted.finish(ExitReason::FrameBudget).expect("teardown");
        std::fs::remove_dir_all(&dir).expect("a scratch dir of this test's own");
    }

    /// **The press the overlay captured is not replayed as play.** Rebinding
    /// jump to `J` with the loop paused, then resuming, must not jump: the
    /// press was the player's answer to the overlay, not a press of the new
    /// binding.
    #[test]
    fn a_captured_press_is_not_replayed_when_the_run_resumes() {
        let dir = scratch("captured-not-replayed");
        let mut engine = run_over_profile(&dir, 64);
        frames(&mut engine, 2);
        listen_for(&mut engine, "jump");
        tap(&mut engine, KeyCode::KeyJ);
        frames(&mut engine, 1);
        let window = engine.window();
        engine
            .shell_mut()
            .key_press(window, PAUSE_KEY)
            .expect("the window is live");
        frames(&mut engine, 1);
        assert!(!engine.is_paused(), "Escape did not resume");
        for _ in 0..10 {
            engine.frame().expect("a frame");
            assert_ne!(
                engine.game().game().stats().anim,
                "jump",
                "the captured J was replayed as a jump"
            );
        }
        engine.finish(ExitReason::FrameBudget).expect("teardown");
        std::fs::remove_dir_all(&dir).expect("a scratch dir of this test's own");
    }

    /// **A clash asks, by the rebind flow's policy**: Shift is run's, so a
    /// press of it for jump opens the clash panel naming run, `CANCEL` leaves
    /// both, and `SWAP` gives jump the Shift and run jump's old Space — the
    /// pad buttons stay where they were.
    #[test]
    fn a_clash_in_the_overlay_asks_and_swap_trades_the_keys() {
        let (mut engine, _pads) = with_pad(64);
        frames(&mut engine, 2);
        listen_for(&mut engine, "jump");
        tap(&mut engine, KeyCode::ShiftLeft);
        frames(&mut engine, 1);
        assert_eq!(engine.menu_kind(), MenuKind::Conflict);
        assert!(
            ui_text(engine.gpu().draw_list())
                .iter()
                .any(|line| line == "Shift IS ON RUN"),
            "the clash is not named on the panel"
        );

        let cancel = Puppet::menu_action(crate::bindings::IDS.cancel()).expect("CANCEL fires");
        engine.game_mut().apply(cancel);
        frames(&mut engine, 1);
        assert_eq!(engine.menu_kind(), MenuKind::Controls);
        assert_eq!(
            jump_bindings(&engine),
            [
                Binding::Key(KeyCode::Space),
                Binding::PadButton(PadButton::South)
            ]
        );

        let listen = Puppet::menu_action(crate::bindings::IDS.action(row("jump")))
            .expect("an action row fires");
        engine.game_mut().apply(listen);
        tap(&mut engine, KeyCode::ShiftLeft);
        frames(&mut engine, 1);
        assert_eq!(engine.menu_kind(), MenuKind::Conflict);
        engine.game_mut().apply(PuppetAction::Swap);
        frames(&mut engine, 1);
        assert_eq!(
            jump_bindings(&engine),
            [
                Binding::Key(KeyCode::ShiftLeft),
                Binding::PadButton(PadButton::South)
            ]
        );
        assert_eq!(
            engine
                .game()
                .controls()
                .actions()
                .bindings(crate::bindings::ACTION_RUN),
            Some(
                &[
                    Binding::Key(KeyCode::Space),
                    Binding::Key(KeyCode::ShiftRight),
                    Binding::PadButton(crate::bindings::RUN_PAD_BUTTON),
                ][..]
            )
        );
        engine.finish(ExitReason::FrameBudget).expect("teardown");
    }
}
