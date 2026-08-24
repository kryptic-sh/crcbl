//! orbit's start-up, its controls, and the [`HostedGame`] methods the engine's
//! loop calls.
//!
//! # There is no loop in this file
//!
//! ```text
//! Loop::frame()                     ← the engine's
//!   pump, input, menu, pause, resize
//!     ─────────────────────────────→ Orbit::key_event  (queued, not applied)
//!   run_ticks  ─────────────────────→ Orbit::tick      (controls, then a tick)
//!   draw_list.clear()
//!     ─────────────────────────────→ Orbit::draw       (the whole page)
//!     menu, debug overlay             ← the engine's
//!   gpu.frame()
//! ```
//!
//! What is left here is start-up, because a window's title is this sample's;
//! the action map, because a keyboard is not something [`crate::game`] should
//! know about; and the trait methods, because they are what a hosted game is.
//!
//! # The action map lives here, not in the simulation
//!
//! Every other sample with input keeps its [`ActionMap`] beside its `Game`.
//! Orbit's server takes [`Controls`] — seven booleans — and the map is what
//! turns a keyboard into them, so it sits on the presentation side of the seam
//! and the simulation stays something a replay or a test can drive by handing it
//! `Controls` directly.

use crcbl::core::input::KeyCode;
use crcbl::engine::{
    Booted, Clock, ExitReason, FrameInfo, HostedGame, RunSummary, wait_for_configure,
};
use crcbl::input::{ActionDecl, ActionKind, ActionMap, Binding};
use crcbl::prelude::*;
use crcbl::shell::{DisplayMode, ShellBackend as Backend, WindowId};

use crate::game::{Controls, FlightStats, Game, Phase, RenderState};
use crate::gpu::Gpu;
use crate::menu::{MenuKind, Menus};
use crate::page::PageStats;

pub use crate::args::Options;

// ---- the controls --------------------------------------------------------------

/// The throttle, opened and closed. Two keys each, because the modifier is what
/// a flight-sim player reaches for and the letter is what everyone else does.
const ACTION_THROTTLE_UP: &str = "throttle-up";
/// See [`ACTION_THROTTLE_UP`].
const ACTION_THROTTLE_DOWN: &str = "throttle-down";
/// Turning in the orbital plane, anticlockwise and clockwise.
const ACTION_PITCH_LEFT: &str = "pitch-left";
/// See [`ACTION_PITCH_LEFT`].
const ACTION_PITCH_RIGHT: &str = "pitch-right";
/// One step up or down the timewarp ladder, on the `.` and `,` keys — the
/// unshifted `>` and `<`, so the binding is the same on every layout the shell
/// reports physical positions for.
const ACTION_WARP_UP: &str = "warp-up";
/// See [`ACTION_WARP_UP`].
const ACTION_WARP_DOWN: &str = "warp-down";
/// The launch clamp, and a new flight after a landing or a crash.
const ACTION_LAUNCH: &str = "launch";

/// The keyboard this sample is flown with.
///
/// Declared in one place so the bindings and the read-out below cannot name
/// different actions: a typo in either is an action that resolves to nothing,
/// and `ActionMap` answers `false` for an action nobody declared rather than
/// complaining.
fn action_map() -> ActionMap {
    let mut map = ActionMap::new();
    for (name, keys) in [
        (ACTION_THROTTLE_UP, vec![KeyCode::KeyW, KeyCode::ShiftLeft]),
        (
            ACTION_THROTTLE_DOWN,
            vec![KeyCode::KeyS, KeyCode::ControlLeft],
        ),
        (ACTION_PITCH_LEFT, vec![KeyCode::KeyA]),
        (ACTION_PITCH_RIGHT, vec![KeyCode::KeyD]),
        (ACTION_WARP_UP, vec![KeyCode::Period]),
        (ACTION_WARP_DOWN, vec![KeyCode::Comma]),
        (ACTION_LAUNCH, vec![KeyCode::Space]),
    ] {
        map.declare(ActionDecl {
            name: name.into(),
            kind: ActionKind::Button,
            bindings: keys.into_iter().map(Binding::Key).collect(),
        });
    }
    map
}

/// What the keyboard is asking for on the tick `actions` has just begun.
///
/// The throttle and the turn read the **held** state, because they are rates the
/// server applies for as long as the key is down. The other three read the
/// **edge**: a held `.` would run the whole warp ladder in four ticks, and a
/// held space would re-launch the moment a flight ended. That distinction is
/// this function's entire content, which is why it is separable from the tick
/// and tested on its own.
fn controls(actions: &ActionMap) -> Controls {
    Controls {
        throttle_up: actions.button_held(ACTION_THROTTLE_UP),
        throttle_down: actions.button_held(ACTION_THROTTLE_DOWN),
        pitch_left: actions.button_held(ACTION_PITCH_LEFT),
        pitch_right: actions.button_held(ACTION_PITCH_RIGHT),
        warp_up: actions.just_pressed(ACTION_WARP_UP),
        warp_down: actions.just_pressed(ACTION_WARP_DOWN),
        launch: actions.just_pressed(ACTION_LAUNCH),
    }
}

// ---- summary -----------------------------------------------------------------

/// What a finished run reports.
///
/// [`PartialEq`] but not [`Eq`], unlike the other samples': [`Summary::altitude`]
/// is a float, so two runs are compared by the numbers they produced and there
/// is no total order to claim.
#[derive(Clone, Debug, PartialEq)]
pub struct Summary {
    pub backend: Backend,
    pub frames: u64,
    pub ticks: u64,
    pub events: u64,
    pub extent: (u32, u32),
    pub exit: ExitReason,
    /// Whether the flight was stopped when the run ended.
    pub paused: bool,
    /// The mode the window system actually had the window in, **not** the one
    /// the run last asked for.
    pub mode: DisplayMode,
    /// Where the mission got to.
    pub phase: Phase,
    /// Height above the surface when the run ended, in metres. The other
    /// samples report a score here; this one is a flight, and the altitude is
    /// the number that says whether it flew.
    pub altitude: f64,
    /// How many commands the last page drew. Zero would mean a run that
    /// presented frames with nothing on them, which is the one failure a
    /// headless smoke test could otherwise report as a pass.
    pub commands: usize,
}

// ---- errors ------------------------------------------------------------------

/// What can stop orbit: the loop's own failures, plus this sample's.
pub type OrbitError = crcbl::engine::LoopError<crate::game::GameError>;

// ---- the hosted game ---------------------------------------------------------

/// orbit, as the engine's loop hosts it.
#[derive(Debug)]
pub struct Orbit {
    game: Game,
    /// The keyboard, resolved into [`Controls`] once per tick.
    actions: ActionMap,
    /// Key events from the shell pump, replayed after `ActionMap::begin_tick`.
    ///
    /// The pump runs once per **frame** and the map's edge flags are per
    /// **tick**, and `begin_tick` clears those flags — so an event fed before it
    /// has its press edge erased. Queueing here and replaying after is the order
    /// the map asks for, and it is what makes a frame that runs no ticks
    /// lossless. It matters most for `launch`: a press that arrived a tick early
    /// is a launch that never happened.
    pending_keys: Vec<(KeyCode, bool)>,
    /// Refilled from the flight every frame, so a steady-state frame does not
    /// allocate a fresh path vector.
    render_state: RenderState,
    /// The flight's numbers, snapshotted in [`Orbit::draw`].
    ///
    /// A snapshot rather than a read at panel time because
    /// [`HostedGame::debug_sections`] is handed `&self` while reading the flight
    /// takes its lock.
    stats: FlightStats,
    /// What the last page drew, from the same frame.
    page: PageStats,
}

/// The loop orbit runs in.
///
/// A type alias, because the loop is the engine's. `S` is the shell type: the
/// native path builds `Loop<dyn Shell>`, and the tests build
/// `Loop<HeadlessShell>` so they can inject the events a compositor would send.
pub type Loop<S = dyn Shell> = crcbl::engine::Loop<S, Orbit>;

/// Runs the full loop.
///
/// # Errors
///
/// [`OrbitError`] if the shell, the GPU or the flight's server failed. Teardown
/// runs on every path.
pub fn run(options: &Options) -> Result<Summary, OrbitError> {
    crcbl::engine::drive(start(options)?)
}

/// Opens a shell, a window, a GPU and the flight.
///
/// # Errors
///
/// [`OrbitError`] if any of them refused.
pub fn start(options: &Options) -> Result<Loop, OrbitError> {
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
/// [`OrbitError`] if the window never configured, the GPU would not open, or the
/// flight's server could not be built.
pub fn with_shell<S: Shell + ?Sized>(
    mut shell: Box<S>,
    options: &Options,
) -> Result<Loop<S>, OrbitError> {
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
/// [`Booted`] is what both bring-up paths hand over, so the flight is built and
/// the loop assembled in one place rather than one per path — a second copy is
/// how the browser build would come to run a subtly different sample.
///
/// # Errors
///
/// [`OrbitError`] if the flight's server could not be built.
fn assemble<S: Shell + ?Sized>(
    booted: Booted<S, Gpu>,
    options: &Options,
) -> Result<Loop<S>, OrbitError> {
    // `--screenshot`, armed before the first frame because the frame it names is
    // counted from this point. The flag forces `--headless` on, so the context
    // behind this is always an offscreen ring — see
    // [`crcbl::args::Common::screenshot`].
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
    let game = Game::new(options.common.tick_hz).map_err(OrbitError::Game)?;
    Ok(Loop::new(
        booted,
        Orbit {
            game,
            actions: action_map(),
            pending_keys: Vec::new(),
            render_state: RenderState::default(),
            stats: FlightStats::default(),
            page: PageStats::default(),
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
) -> Result<WindowId, OrbitError> {
    Ok(crcbl::engine::open_window(
        shell,
        clock_source,
        &WindowDesc {
            title: "Orbit",
            app_id: "sh.kryptic.crcbl.orbit",
            size: crcbl::engine::requested_window_size(size),
            mode,
            ..WindowDesc::default()
        },
    )?)
}

impl Orbit {
    /// The flight, for scripted tests and for an embedder that drives it.
    pub const fn game(&self) -> &Game {
        &self.game
    }

    /// What the last frame's page drew.
    pub const fn page(&self) -> &PageStats {
        &self.page
    }
}

/// orbit's half of the frame, and nothing else.
impl HostedGame for Orbit {
    type Error = crate::game::GameError;
    type Gpu = Gpu;
    type MenuKind = MenuKind;
    /// orbit declares no menu action of its own — see [`crate::menu`].
    /// Uninhabited rather than a placeholder enum, so [`Orbit::apply`] is a
    /// match on nothing and the compiler agrees there is no case to handle.
    type MenuAction = core::convert::Infallible;
    type Summary = Summary;

    const NAME: &'static str = "orbit";

    fn menus() -> Menus {
        crate::menu::menus()
    }

    fn tick(&mut self, _gpu: &mut Gpu, tick_dt: f64) {
        self.actions.begin_tick(tick_dt as f32);
        for (key, pressed) in std::mem::take(&mut self.pending_keys) {
            self.actions.key_event(key, pressed);
        }
        self.game.set_controls(controls(&self.actions));
        self.game.tick();
    }

    fn key_event(&mut self, key: KeyCode, pressed: bool) {
        // Queued rather than fed straight in: the map's edges belong to the
        // tick, not to the frame. See [`Orbit::pending_keys`].
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
        _frame: FrameInfo,
    ) {
        self.game.render_state(&mut self.render_state);
        self.stats = self.game.stats();
        self.page = crate::page::draw(draw_list, gpu.atlas(), gpu.extent(), &self.render_state);
    }

    /// **orbit's one module, and no second.**
    ///
    /// No network section: this sample runs over `InMemoryTransport` and has no
    /// connection to report on. No audio section either — it plays nothing, and
    /// a section that said so would be a module with no system behind it. What
    /// it does have is the flight, and `06-orbit.md` asks for it by name:
    /// timewarp is a frame-budget question before it is a physics question, and
    /// the panel is where the two are read side by side.
    fn debug_sections(&self, panel: &mut crcbl::ui::DebugPanel) {
        panel.add(&self.stats);
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
            phase: self.stats.phase,
            altitude: self.stats.altitude,
            commands: self.page.commands,
        }
    }

    fn log_summary(summary: &Summary) {
        crcbl::log::info!(
            "orbit: {} frames, {} ticks, {} at {:.0} m, {} page commands ({:?})",
            summary.frames,
            summary.ticks,
            summary.phase.label(),
            summary.altitude,
            summary.commands,
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
    /// [`OrbitError`] if the shell refused the window.
    pub fn request(
        mut shell: Box<S>,
        options: &Options,
        clock_source: Clock,
    ) -> Result<Self, OrbitError> {
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
    /// [`OrbitError`] if the window went away before it had a size, if the
    /// device request failed, or if the flight could not be built.
    pub fn poll(&mut self) -> Result<Option<Loop<S>>, OrbitError> {
        let Some(booted) = self.boot.poll::<OrbitError>()? else {
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
    use crcbl::engine::{DEBUG_OVERLAY_KEY, Flow, PAUSE_KEY};
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

    /// [`headless`] with one shared field changed.
    ///
    /// Struct-update syntax cannot reach through `Options::common` — `..` fills
    /// whole fields, and `common` is one field — so an override is a closure
    /// rather than another literal.
    fn headless_with(frames: u64, edit: impl FnOnce(&mut Common)) -> Options {
        let mut options = headless(frames);
        edit(&mut options.common);
        options
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

    /// The value drawn immediately after the row labelled `label`.
    fn row_value(drawn: &[String], label: &str) -> String {
        let mut matches = drawn
            .iter()
            .enumerate()
            .filter(|(_, text)| *text == label)
            .map(|(at, _)| at);
        let at = matches
            .next()
            .unwrap_or_else(|| panic!("no {label} row in {drawn:?}"));
        // Row labels share one namespace across every section of the panel, and
        // two have collided already — `crcbl-render`'s frame timings draw a
        // `pending` row, and hud's first draft named one of its own the same. A
        // reader tells them apart by the heading above them; a search through
        // the flat draw list cannot, and would read whichever came first for
        // ever after.
        assert!(
            matches.next().is_none(),
            "more than one {label} row in {drawn:?}, so this reads whichever the panel \
             happened to draw first"
        );
        drawn
            .get(at + 1)
            .unwrap_or_else(|| panic!("no value after {label} in {drawn:?}"))
            .clone()
    }

    /// Presses and releases one key through the shell, a frame apart, so the
    /// action map sees a press edge and then a release.
    fn tap(engine: &mut Loop<HeadlessShell>, key: KeyCode) {
        let window = engine.window();
        engine
            .shell_mut()
            .key_press(window, key)
            .expect("the window is live");
        engine.frame().expect("a frame");
        engine
            .shell_mut()
            .key_release(window, key)
            .expect("the window is live");
        engine.frame().expect("a frame");
    }

    #[test]
    fn a_headless_run_is_deterministic() {
        let first = run(&headless(30)).expect("headless runs everywhere");
        let second = run(&headless(30)).expect("headless runs everywhere");
        assert_eq!(first, second, "two identical runs must agree exactly");
        assert_eq!(first.backend, Backend::Headless);
        assert_eq!(first.frames, 30);
        assert_eq!(first.exit, ExitReason::FrameBudget);
        assert!(
            first.commands > 0,
            "a run that drew nothing presented 30 blank frames"
        );
    }

    /// **The panel renders with no network module.** The sections orbit has are
    /// the frame's, the GPU's where the device has timestamp queries, and this
    /// sample's own one. Nothing else, and no configuration decided that.
    #[test]
    fn the_overlay_is_composed_of_exactly_the_modules_orbit_has() {
        let mut engine = scripted(&headless_with(8, |common| {
            common.debug_overlay = Some(true)
        }));
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
            &["frame", "gpu", "counters", "orbit"]
        } else {
            &["frame", "counters", "orbit"]
        };
        assert_eq!(titles, expected, "no module appears that no system offered");

        // **This sample's section reached the draw list with its own numbers in
        // it**, not just its heading. Two frames have run one loop tick between
        // them, and `Game::new` spends one more on the client's hello before the
        // loop ever starts — so the flight is two ticks old here, not one. The
        // autopilot holds the clamp for its first second either way, so the ship
        // is still on the pad.
        let drawn = ui_text(&engine);
        assert_eq!(row_value(&drawn, "tick"), "2");
        assert_eq!(row_value(&drawn, "phase"), Phase::Prelaunch.label());
        assert_eq!(row_value(&drawn, "warp"), "x1");
        assert_eq!(row_value(&drawn, "pilot"), "auto");
        engine.finish(ExitReason::FrameBudget).expect("teardown");
    }

    /// **Switching the panel on is one thing, and it works through the real
    /// loop.** F3 arrives as an ordinary shell key event and the very next
    /// frame's draw list gains the frame section; F3 again and it is gone. The
    /// page is untouched either way.
    #[test]
    fn f3_toggles_the_debug_overlay_in_the_frames_draw_list() {
        let mut engine = scripted(&headless_with(16, |common| {
            common.debug_overlay = Some(false)
        }));
        let window = engine.window();

        engine.frame().expect("a frame");
        engine.frame().expect("a frame");
        let hidden = ui_text(&engine);
        assert!(
            hidden.iter().any(|t| t == "ALT"),
            "the page is always drawn: {hidden:?}",
        );
        assert!(
            !hidden.iter().any(|t| t == "frame"),
            "the overlay starts hidden here: {hidden:?}",
        );

        // **And the page reaches the GPU.** `UiRenderer::add_pass` declares
        // nothing when the draw list is empty, so the pass's presence in the
        // frame's graph is what separates "the page was drawn" from "the page
        // was composited".
        assert!(
            engine.gpu().last_dump().contains("ui-composite"),
            "the page's UI pass must be in the frame:\n{}",
            engine.gpu().last_dump(),
        );

        engine
            .shell_mut()
            .key_press(window, DEBUG_OVERLAY_KEY)
            .expect("the window is live");
        engine.frame().expect("a frame");
        let shown = ui_text(&engine);
        assert!(
            shown.iter().any(|t| t == "frame") && shown.iter().any(|t| t == "orbit"),
            "F3 must show this sample's section: {shown:?}",
        );
        assert!(
            shown.iter().any(|t| t == "ALT"),
            "the page survives the overlay: {shown:?}",
        );

        engine
            .shell_mut()
            .key_press(window, DEBUG_OVERLAY_KEY)
            .expect("the window is live");
        engine.frame().expect("a frame");
        assert!(
            !ui_text(&engine).iter().any(|t| t == "frame"),
            "F3 hides it"
        );
        engine.finish(ExitReason::FrameBudget).expect("teardown");
    }

    /// Escape stops the flight and puts the one menu this sample has on screen;
    /// escape again starts it. The page keeps drawing behind it either way.
    #[test]
    fn escape_stops_the_flight_and_shows_the_pause_menu() {
        let mut engine = scripted(&headless(24));
        let window = engine.window();
        engine.frame().expect("a frame");
        engine.frame().expect("a frame");
        let running = engine.game().game().ticks_run();
        assert!(running > 0, "the flight never ran");
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
            ui_text(&engine).iter().any(|t| t == "ALT"),
            "the page is drawn behind the panel",
        );
        engine.finish(ExitReason::FrameBudget).expect("teardown");
    }

    /// Ticks are paced by the clock and not the frame rate, which is what makes
    /// the flight's rhythm a property of `--tick-hz` rather than of the display.
    #[test]
    fn ticks_are_paced_by_the_clock_not_the_frame_rate() {
        let sixty = run(&headless(62)).expect("headless runs everywhere");
        let thirty = run(&headless_with(62, |common| common.tick_hz = 30))
            .expect("headless runs everywhere");
        assert_eq!(sixty.frames, thirty.frames);
        // 62 frames, the first update establishing the baseline: 61 ticks at
        // 60 Hz.
        assert_eq!(sixty.ticks, 61);
        assert_eq!(thirty.ticks, 30, "half the rate, half the ticks");

        // The case that needs the accumulator to be a `while` rather than an
        // `if`: a headless frame is pinned to 1/60 s, so at 120 Hz every frame
        // owes the simulation two ticks.
        let fast = run(&headless_with(62, |common| common.tick_hz = 120))
            .expect("headless runs everywhere");
        assert_eq!(fast.ticks, 122, "a frame owing two ticks must run both");
    }

    /// **The script flies, and the summary reports where it got to.** The
    /// autopilot holds the clamp for its first second and then launches, so a
    /// run long enough to cross that leaves the pad — and the summary's phase
    /// and altitude are read off the flight rather than being the defaults they
    /// started at.
    #[test]
    fn the_autopilot_leaves_the_pad_and_the_summary_says_so() {
        let clamped = run(&headless(4)).expect("headless runs everywhere");
        assert_eq!(clamped.phase, Phase::Prelaunch);
        assert!(clamped.altitude < 1.0, "{} m off the pad", clamped.altitude);

        // Two seconds of flight: one on the clamp, one climbing.
        let flown = run(&headless(crate::game::AUTOPILOT_LAUNCH_TICK * 2))
            .expect("headless runs everywhere");
        assert_eq!(flown.phase, Phase::Flying);
        assert!(
            flown.altitude > clamped.altitude,
            "{} m is no higher than the pad",
            flown.altitude,
        );
        assert!(!flown.paused);
        // The instrument panel and the map, on every frame from the first.
        assert!(flown.commands >= 20, "{} commands", flown.commands);
    }

    /// Two runs of the same length fly the same flight, down to the state hash
    /// — which is a stronger claim than the summary's, because it folds in the
    /// position, the velocity and the attitude the summary never reports. There
    /// is no `--seed` to vary here: nothing in the flight draws a random number.
    #[test]
    fn the_same_run_twice_is_the_same_flight() {
        let hash = || {
            let mut engine = scripted(&headless(90));
            while let Ok(Flow::Continue) = engine.frame() {}
            let hash = engine.game().game().state_hash();
            engine.finish(ExitReason::FrameBudget).expect("teardown");
            hash
        };
        assert_eq!(hash(), hash());
    }

    /// **The keyboard reaches the flight through the whole input path** — shell
    /// event, action map, [`Controls`], the client's wire bytes, the server's
    /// module — and the first thing the player asks for ends the script for
    /// good. Space is the edge that does it here, and pressing it on the pad
    /// releases the clamp at the same time.
    #[test]
    fn the_first_key_takes_the_flight_off_the_autopilot() {
        let mut engine = scripted(&headless(30));
        engine.frame().expect("a frame");
        engine.frame().expect("a frame");
        assert!(
            engine.game().game().stats().autopilot,
            "a page that has just loaded flies itself",
        );

        tap(&mut engine, KeyCode::Space);
        let stats = engine.game().game().stats();
        assert!(
            !stats.autopilot,
            "the first key is the last one the script flies",
        );
        // The clamp is off, which is what the key was for. The phase does not
        // stay `Flying`: the player took the controls with the throttle still
        // closed, so the ship settles back onto the pad on the same tick it was
        // released and reads `Landed`. What this asserts is that the edge
        // reached the server, not that a rocket with no thrust flew.
        assert_ne!(
            stats.phase,
            Phase::Prelaunch,
            "space must release the launch clamp",
        );
        engine.finish(ExitReason::FrameBudget).expect("teardown");
    }

    /// One tick of the map, so a test can say what the seven actions resolved
    /// to after a given set of key edges.
    fn resolved(map: &mut ActionMap, edges: &[(KeyCode, bool)]) -> Controls {
        map.begin_tick(1.0 / f64::from(crate::game::DEFAULT_TICK_HZ) as f32);
        for &(key, pressed) in edges {
            map.key_event(key, pressed);
        }
        controls(map)
    }

    /// **Every action is bound to the keys the sample documents, and to no
    /// others.** A binding that reached the wrong field would fly the rocket
    /// sideways and nothing else in this file would notice: the flight is
    /// scripted until a key arrives, and every test above it presses at most
    /// one.
    #[test]
    fn each_key_reaches_its_own_control_and_no_other() {
        let mut map = action_map();
        for (key, field) in [
            (KeyCode::KeyW, "throttle_up"),
            (KeyCode::ShiftLeft, "throttle_up"),
            (KeyCode::KeyS, "throttle_down"),
            (KeyCode::ControlLeft, "throttle_down"),
            (KeyCode::KeyA, "pitch_left"),
            (KeyCode::KeyD, "pitch_right"),
            (KeyCode::Period, "warp_up"),
            (KeyCode::Comma, "warp_down"),
            (KeyCode::Space, "launch"),
        ] {
            let down = resolved(&mut map, &[(key, true)]);
            let named = [
                ("throttle_up", down.throttle_up),
                ("throttle_down", down.throttle_down),
                ("pitch_left", down.pitch_left),
                ("pitch_right", down.pitch_right),
                ("warp_up", down.warp_up),
                ("warp_down", down.warp_down),
                ("launch", down.launch),
            ];
            for (name, set) in named {
                assert_eq!(set, name == field, "{key} set {name}: {down:?}");
            }
            // Let go, or the next key in the list is pressed on top of this one.
            let up = resolved(&mut map, &[(key, false)]);
            assert_eq!(up, Controls::default(), "{key} stayed down: {up:?}");
        }
    }

    /// **A held key is a rate; a tapped one is a step.** The throttle and the
    /// turn stay set for as long as the key is down, so the server keeps
    /// applying their rate — while warp and launch report only the tick the key
    /// went down on, because a held `.` would run the whole warp ladder in four
    /// ticks and a held space would re-launch the instant a flight ended.
    ///
    /// The two halves are what [`controls`] is, and swapping either one is a
    /// change no other test in this file would fail on.
    #[test]
    fn a_held_key_keeps_asking_while_an_edge_asks_once() {
        let mut map = action_map();

        assert!(resolved(&mut map, &[(KeyCode::KeyW, true)]).throttle_up);
        assert!(
            resolved(&mut map, &[]).throttle_up,
            "the throttle is a rate: still open on the next tick with no new event",
        );
        assert!(!resolved(&mut map, &[(KeyCode::KeyW, false)]).throttle_up);

        assert!(resolved(&mut map, &[(KeyCode::Period, true)]).warp_up);
        assert!(
            !resolved(&mut map, &[]).warp_up,
            "warp is a step: a key still down is not a second one",
        );
        assert!(resolved(&mut map, &[(KeyCode::Space, true)]).launch);
        assert!(!resolved(&mut map, &[]).launch, "so is the launch clamp");
    }
}
