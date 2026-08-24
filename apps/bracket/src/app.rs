//! Bracket's start-up, and the [`HostedGame`] methods the engine's loop calls.
//!
//! # There is no loop in this file
//!
//! ```text
//! Loop::frame()                     ← the engine's
//!   pump, input, menu, pause, resize
//!   run_ticks  ─────────────────────→ Bracket::tick   (one matchmaking tick)
//!   draw_list.clear()
//!     ─────────────────────────────→ Bracket::draw    (the whole page)
//!     menu, debug overlay             ← the engine's
//!   gpu.frame()
//! ```
//!
//! What is left here is start-up, because a window's title is this sample's, and
//! the trait methods, because they are what a hosted game is. Both are short:
//! bracket has no input to route and no animation on the frame's clock, so
//! [`Bracket::tick`] steps the population and [`Bracket::draw`] draws it, and
//! neither does anything else.
//!
//! # It runs itself
//!
//! Nothing here reads a key, and that is the point rather than an omission. A
//! matchmaker is a claim about a **population over time**: players decide to
//! queue on their own, the queue widens its tolerance on its own, and the
//! convergence curve falls or it does not. A visitor who loads the published
//! page sees a ladder sorting itself out within a second of arriving, with no
//! instructions to read and nothing to press.

use crcbl::core::input::KeyCode;
use crcbl::engine::{
    Booted, Clock, ExitReason, FrameInfo, HostedGame, RunSummary, wait_for_configure,
};
use crcbl::prelude::*;
use crcbl::shell::{DisplayMode, ShellBackend as Backend, WindowId};
use crcbl::ui::{DebugModule, DebugSection};

use crate::gpu::Gpu;
use crate::menu::{MenuKind, Menus};
use crate::sim::Sim;

pub use crate::args::Options;

// ---- defaults ----------------------------------------------------------------

/// The published seed: the population the demo opens on when nothing said
/// otherwise.
///
/// The same value `bracket sim` defaults to, so the report a reader prints and
/// the ladder they watch are the same run.
pub const DEFAULT_SEED: u64 = 1;

/// How many synthetic players queue against each other by default.
///
/// Large enough that the queue has real choices to make — a matchmaker with a
/// handful of players pairs whoever is there and proves nothing — and small
/// enough that the ladder panel's rows are a meaningful slice of it.
pub const DEFAULT_PLAYERS: usize = 64;

/// How fast the population is stepped, in matchmaking ticks a second.
///
/// The engine's own default rate. A tick here is one round of "who wants to
/// queue, who can be paired, who played", not a physics step, and at this rate
/// the convergence curve crosses the plot in about the time it takes to read
/// the rest of the page.
pub const DEFAULT_TICK_HZ: u32 = 60;

/// How often [`Bracket::log_heartbeat`] logs, in ticks.
///
/// A second of simulated time at [`DEFAULT_TICK_HZ`], which is the cadence every
/// other sample's heartbeat is spaced at — `web/tools/browser-e2e.mjs` watches
/// for it to tell a paused demo from a running one, and its window is sized for
/// that spacing.
const HEARTBEAT_TICKS: u64 = 60;

// ---- summary -----------------------------------------------------------------

/// What a finished run reports.
///
/// [`PartialEq`] but not [`Eq`], unlike hud's: [`Summary::error`] is a float, so
/// two runs are compared by the numbers they produced and there is no total
/// order to claim.
#[derive(Clone, Debug, PartialEq)]
pub struct Summary {
    pub backend: Backend,
    pub frames: u64,
    pub ticks: u64,
    pub events: u64,
    pub extent: (u32, u32),
    pub exit: ExitReason,
    /// Whether the population was stopped when the run ended.
    pub paused: bool,
    /// The mode the window system actually had the window in, **not** the one
    /// the run last asked for.
    pub mode: DisplayMode,
    /// How many matches were played and rated.
    pub matches: u64,
    /// How far the ladder still is from the truth, in rating points. The other
    /// samples report a score here; this one is a measurement, and this is the
    /// number the whole sample exists to drive down.
    pub error: f64,
    /// How many commands the last page drew. Zero would mean a run that
    /// presented frames with nothing on them, which is the one failure a
    /// headless smoke test could otherwise report as a pass.
    pub commands: usize,
}

// ---- errors ------------------------------------------------------------------

/// What can stop bracket.
///
/// An alias rather than an enum: [`crcbl::engine::LoopError`] owns these
/// variants for every sample, and a population built from a seed has nothing of
/// its own to fail at — [`Sim::new`] cannot refuse — so it takes the default
/// type parameter and its `Game` variant is uninhabited.
pub type BracketError = crcbl::engine::LoopError;

// ---- the debug panel ---------------------------------------------------------

/// Bracket's section of the debug panel: what the population is doing, and what
/// the page made of it.
///
/// A **borrow** rather than a snapshot copied each frame, which is where this
/// differs from hud: hud reads its ticker through a lock and
/// [`HostedGame::debug_sections`] is handed only `&self`, so it has to copy the
/// numbers out during `draw` and hold them. Nothing here is behind a lock — the
/// [`Sim`] is owned by [`Bracket`] — so the section reads the live thing and
/// there is no second copy to go stale.
#[derive(Debug)]
struct Stats<'a> {
    sim: &'a Sim,
    /// What the last page drew.
    commands: usize,
}

impl DebugModule for Stats<'_> {
    fn debug_section(&self, out: &mut DebugSection) {
        out.set_title("bracket");
        out.row("tick", format_args!("{}", self.sim.tick_count()));
        out.row("matches", format_args!("{}", self.sim.matches_played()));
        out.row("queued", format_args!("{}", self.sim.queue().len()));
        out.row("gap", format_args!("{:.0}", self.sim.mean_gap()));
        out.row("wait", format_args!("{:.2}", self.sim.mean_wait()));
        out.row("error", format_args!("{:.1}", self.sim.mean_rating_error()));
        // The dogfood row, and the one that separates "the page ran" from "the
        // page drew something": `page::draw` counts the commands it emitted.
        out.row("commands", format_args!("{}", self.commands));
    }
}

// ---- the hosted game ---------------------------------------------------------

/// Bracket, as the engine's loop hosts it.
#[derive(Debug)]
pub struct Bracket {
    sim: Sim,
    /// How many commands the last [`Bracket::draw`] emitted.
    commands: usize,
}

/// The loop bracket runs in.
///
/// A type alias, because the loop is the engine's. `S` is the shell type: the
/// native path builds `Loop<dyn Shell>`, and the tests build
/// `Loop<HeadlessShell>` so they can inject the events a compositor would send.
pub type Loop<S = dyn Shell> = crcbl::engine::Loop<S, Bracket>;

/// Runs the full loop.
///
/// # Errors
///
/// [`BracketError`] if the shell or the GPU failed. Teardown runs on every path.
pub fn run(options: &Options) -> Result<Summary, BracketError> {
    crcbl::engine::drive(start(options)?)
}

/// Opens a shell, a window, a GPU and the population.
///
/// # Errors
///
/// [`BracketError`] if any of them refused.
pub fn start(options: &Options) -> Result<Loop, BracketError> {
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
/// [`BracketError`] if the window never configured or the GPU would not open.
pub fn with_shell<S: Shell + ?Sized>(
    mut shell: Box<S>,
    options: &Options,
) -> Result<Loop<S>, BracketError> {
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
/// [`Booted`] is what both bring-up paths hand over, so the population is built
/// and the loop assembled in one place rather than one per path — a second copy
/// is how the browser build would come to run a subtly different sample.
fn assemble<S: Shell + ?Sized>(booted: Booted<S, Gpu>, options: &Options) -> Loop<S> {
    // `--screenshot`, armed before the first frame because the frame it names
    // is counted from this point. The flag forces `--headless` on, so the
    // context behind this is always an offscreen ring — see
    // [`crcbl::args::Common::screenshot`].
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
        Bracket {
            sim: Sim::new(options.seed, options.players),
            commands: 0,
        },
        options.common.loop_config(),
    )
}

/// Creates the one window this sample has: its title, its app id, its size.
fn open_the_window<S: Shell + ?Sized>(
    shell: &mut S,
    clock_source: &Clock,
    mode: DisplayMode,
    size: Option<crcbl::shell::PhysicalSize>,
) -> Result<WindowId, BracketError> {
    Ok(crcbl::engine::open_window(
        shell,
        clock_source,
        &WindowDesc {
            title: "Bracket",
            app_id: "sh.kryptic.crcbl.bracket",
            size: crcbl::engine::requested_window_size(size),
            mode,
            ..WindowDesc::default()
        },
    )?)
}

impl Bracket {
    /// The population, for scripted tests and for an embedder that drives it.
    pub const fn sim(&self) -> &Sim {
        &self.sim
    }

    /// How many commands the last frame's page drew.
    pub const fn commands(&self) -> usize {
        self.commands
    }

    /// The `[HUD]` line, on the cadence every other sample uses: every
    /// [`HEARTBEAT_TICKS`] steps, which is a second of simulated time at the
    /// default rate.
    ///
    /// It is the only thing this sample logs from inside the tick, and
    /// `web/tools/browser-e2e.mjs` reads two claims out of it. One is the
    /// heartbeat itself — it exists while the demo runs and stops while it is
    /// paused, which is how a browser tells a paused loop from a running one.
    ///
    /// The other is that the population is **moving**, and bracket takes no
    /// input, so nothing external can be shown to have reached it: it has to be
    /// read off numbers the simulation itself moves. `matches` is one — it only
    /// climbs when the queue actually paired somebody — and `error` is the
    /// other, and the better of the two, because it is the sample's whole claim:
    /// the distance between what the ladder believes and what the players are
    /// really worth, falling. A run that ticked without matchmaking would leave
    /// the first standing still; one that matched without learning would leave
    /// the second.
    fn log_heartbeat(&self) {
        if !self.sim.tick_count().is_multiple_of(HEARTBEAT_TICKS) {
            return;
        }
        crcbl::log::info!(
            "[HUD] tick: {}  matches: {}  queued: {}  gap: {:.0}  wait: {:.2}  error: {:.1}",
            self.sim.tick_count(),
            self.sim.matches_played(),
            self.sim.queue().len(),
            self.sim.mean_gap(),
            self.sim.mean_wait(),
            self.sim.mean_rating_error(),
        );
    }
}

/// Bracket's half of the frame, and nothing else.
impl HostedGame for Bracket {
    /// A population built from a seed has nothing of its own to fail at.
    type Error = core::convert::Infallible;
    type Gpu = Gpu;
    type MenuKind = MenuKind;
    /// Bracket declares no menu action of its own — see [`crate::menu`].
    /// Uninhabited rather than a placeholder enum, so [`Bracket::apply`] is a
    /// match on nothing and the compiler agrees there is no case to handle.
    type MenuAction = core::convert::Infallible;
    type Summary = Summary;

    const NAME: &'static str = "bracket";

    fn menus() -> Menus {
        crate::menu::menus()
    }

    fn tick(&mut self, _gpu: &mut Gpu, _tick_dt: f64) {
        self.sim.step();
        self.log_heartbeat();
    }

    /// Bracket reads no key of its own.
    ///
    /// Every key this sample answers to is the loop's — `ESC` pauses, `F3`
    /// toggles the panel, `F11` goes fullscreen — and the population runs itself
    /// from the seed. There is nothing here to forward a key to, and a binding
    /// that did nothing would be worse than none.
    fn key_event(&mut self, _key: KeyCode, _pressed: bool) {}

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
        let (width, height) = gpu.extent();
        self.commands = crate::page::draw(
            draw_list,
            crcbl::math::Vec2::new(width as f32, height as f32),
            gpu.atlas(),
            &self.sim,
        );
    }

    /// **One section, and no second one.**
    ///
    /// No network section: this sample has no connection to report on. No audio
    /// section either — it plays nothing, and a section that said so would be a
    /// module with no system behind it. What it does have is the population that
    /// produced the page and the page that came out of it, and both are in the
    /// one section because both are read off the same borrow at the same
    /// instant.
    fn debug_sections(&self, panel: &mut crcbl::ui::DebugPanel) {
        panel.add(&Stats {
            sim: &self.sim,
            commands: self.commands,
        });
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
            matches: self.sim.matches_played(),
            error: self.sim.mean_rating_error(),
            commands: self.commands,
        }
    }

    fn log_summary(summary: &Summary) {
        crcbl::log::info!(
            "bracket: {} frames, {} ticks, {} matches, {:.1} rating error, {} page commands ({:?})",
            summary.frames,
            summary.ticks,
            summary.matches,
            summary.error,
            summary.commands,
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
    /// [`BracketError`] if the shell refused the window.
    pub fn request(
        mut shell: Box<S>,
        options: &Options,
        clock_source: Clock,
    ) -> Result<Self, BracketError> {
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
    /// [`BracketError`] if the window went away before it had a size, or if the
    /// device request failed.
    pub fn poll(&mut self) -> Result<Option<Loop<S>>, BracketError> {
        let Some(booted) = self.boot.poll::<BracketError>()? else {
            return Ok(None);
        };
        Ok(Some(assemble(booted, &self.options)))
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
                ..Common::new(DEFAULT_TICK_HZ)
            },
            ..Options::default()
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
        // a collision would leave this reading whichever section drew first for
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

    /// **The panel renders with no network module.** The sections bracket has
    /// are the frame's, the GPU's where the device has timestamp queries, and
    /// this sample's own one. Nothing else, and no configuration decided that.
    #[test]
    fn the_overlay_is_composed_of_exactly_the_modules_bracket_has() {
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
            &["frame", "gpu", "counters", "bracket"]
        } else {
            &["frame", "counters", "bracket"]
        };
        assert_eq!(titles, expected, "no module appears that no system offered");

        let drawn = ui_text(&engine);
        for row in ["frame", "fps", "avg", "worst", "window"] {
            assert!(drawn.iter().any(|t| t == row), "missing {row}: {drawn:?}");
        }

        // **This sample's section reached the draw list with its own numbers in
        // it**, not just its heading. Two frames have run one tick between them,
        // so exactly one matchmaking tick has happened.
        let bracket = engine.game();
        assert_eq!(row_value(&drawn, "tick"), "1");
        assert_eq!(
            row_value(&drawn, "tick"),
            bracket.sim().tick_count().to_string()
        );
        assert_eq!(
            row_value(&drawn, "matches"),
            bracket.sim().matches_played().to_string()
        );
        assert_eq!(
            row_value(&drawn, "queued"),
            bracket.sim().queue().len().to_string()
        );
        // The commands row counts what the page actually emitted, and they are
        // all still in the list the panel was appended to.
        assert_eq!(
            row_value(&drawn, "commands"),
            bracket.commands().to_string()
        );
        assert!(bracket.commands() > 0, "the page drew nothing");

        // **The numbers come from the clock, not from nowhere.** The first
        // frame's interval is the clock's zero-length sentinel and is dropped,
        // so two frames leave exactly one sample: the headless step.
        assert_eq!(engine.debug().frame.len(), 1, "one real interval so far");
        assert_eq!(
            engine.debug().frame.mean(),
            crcbl::engine::HEADLESS_FRAME_STEP,
            "the window holds the clock's own step",
        );
        assert_eq!(row_value(&drawn, "avg"), "16.67 ms");
        assert_eq!(row_value(&drawn, "window"), "1/120");
        assert_eq!(row_value(&drawn, "fps"), "60.0");
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
            hidden.iter().any(|t| t == "LADDER"),
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
            shown.iter().any(|t| t == "frame") && shown.iter().any(|t| t == "bracket"),
            "F3 must show this sample's section: {shown:?}",
        );
        assert!(
            shown.iter().any(|t| t == "LADDER"),
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

    /// Escape stops the population and puts the one menu this sample has on
    /// screen; escape again starts it. The page keeps drawing behind it either
    /// way.
    #[test]
    fn escape_stops_the_population_and_shows_the_pause_menu() {
        let mut engine = scripted(&headless(24));
        let window = engine.window();
        engine.frame().expect("a frame");
        engine.frame().expect("a frame");
        let running = engine.game().sim().tick_count();
        assert!(running > 0, "the population never stepped");
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
            engine.game().sim().tick_count(),
            running,
            "a paused loop runs no ticks",
        );
        assert!(
            ui_text(&engine).iter().any(|t| t == "LADDER"),
            "the page is drawn behind the panel",
        );
        engine.finish(ExitReason::FrameBudget).expect("teardown");
    }

    /// Ticks are paced by the clock and not the frame rate, which is what makes
    /// the matchmaker's rhythm a property of `--tick-hz` rather than of the
    /// display.
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

    /// **The demo runs itself.** Nothing sends a key, and the population still
    /// queues, pairs and re-rates — which is what a visitor who loads the page
    /// and touches nothing has to see. Both halves are asserted because either
    /// alone would pass on a broken sample: ticks with no matches is a
    /// matchmaker that never paired, and a page that stopped changing is one
    /// nobody would watch.
    #[test]
    fn the_population_runs_with_nothing_driving_it() {
        let summary = run(&headless(240)).expect("headless runs everywhere");
        assert_eq!(summary.ticks, 239);
        assert!(summary.matches > 0, "nobody was ever paired");
        assert!(summary.commands > 0, "the page drew nothing");
    }

    /// The ladder learns, which is the claim the whole sample is for: the
    /// distance between the ratings and the true skills falls as matches are
    /// played.
    #[test]
    fn the_ladder_converges_on_the_true_skills_as_it_runs() {
        let short = run(&headless(120)).expect("headless runs everywhere");
        let long = run(&headless(2_400)).expect("headless runs everywhere");
        assert!(
            long.matches > short.matches,
            "the longer run played no more"
        );
        assert!(
            long.error < short.error,
            "{:.1} points after {} matches is no better than {:.1} after {}",
            long.error,
            long.matches,
            short.error,
            short.matches,
        );
    }

    /// `--seed` reaches the population through the whole start-up path, so the
    /// flag is not a number the parser stores and nothing reads.
    #[test]
    fn the_seed_flag_reaches_the_population() {
        let ladder = |seed: u64| {
            let mut engine = scripted(&Options {
                seed,
                ..headless(120)
            });
            while let Ok(Flow::Continue) = engine.frame() {}
            let ladder = engine.game().sim().ladder();
            engine.finish(ExitReason::FrameBudget).expect("teardown");
            ladder
        };
        let published = ladder(DEFAULT_SEED);
        assert_ne!(published, ladder(DEFAULT_SEED + 1));
        assert_eq!(published, ladder(DEFAULT_SEED));
    }

    /// `--players` does too, and it is the flag a seed check cannot stand in
    /// for: a population size the parser stored and start-up ignored would
    /// leave every run the default size.
    #[test]
    fn the_players_flag_reaches_the_population() {
        let mut engine = scripted(&Options {
            players: 9,
            ..headless(4)
        });
        engine.frame().expect("a frame");
        assert_eq!(engine.game().sim().players().len(), 9);
        assert_eq!(engine.game().sim().ladder().len(), 9);
        engine.finish(ExitReason::FrameBudget).expect("teardown");
    }
}
