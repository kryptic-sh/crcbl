//! hud's start-up, and the [`HostedGame`] methods the engine's loop calls.
//!
//! # There is no loop in this file
//!
//! ```text
//! Loop::frame()                     ← the engine's
//!   pump, input, menu, pause, resize
//!   run_ticks  ─────────────────────→ Hud::tick      (one turn of the ticker)
//!   draw_list.clear()
//!     ─────────────────────────────→ Hud::draw      (the whole page)
//!     menu, debug overlay             ← the engine's
//!   gpu.frame()
//! ```
//!
//! What is left here is start-up, because a window's title is this sample's, and
//! the trait methods, because they are what a hosted game is. Both are shorter
//! than any other sample's: hud has no input to route and no animation on the
//! frame's clock, so [`Hud::draw`] does two things — snapshot the ticker, draw
//! the page — and nothing else.

use crcbl::core::input::KeyCode;
use crcbl::engine::{
    Booted, Clock, ExitReason, FrameInfo, HostedGame, RunSummary, wait_for_configure,
};
use crcbl::prelude::*;
use crcbl::shell::{DisplayMode, ShellBackend as Backend, WindowId, open, open_backend};

use crate::game::{Game, HudStats, RenderState};
use crate::gpu::Gpu;
use crate::menu::{MenuKind, Menus};
use crate::page::PageStats;

pub use crate::args::Options;

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
    /// Whether the ticker was stopped when the run ended.
    pub paused: bool,
    /// The mode the window system actually had the window in, **not** the one
    /// the run last asked for.
    pub mode: DisplayMode,
    /// Which wave the ticker reached.
    pub wave: u32,
    /// How many commands the last page drew. Zero would mean a run that
    /// presented frames with nothing on them, which is the one failure a
    /// headless smoke test could otherwise report as a pass.
    pub commands: usize,
}

// ---- errors ------------------------------------------------------------------

/// What can stop hud: the loop's own failures, plus this sample's.
pub type HudError = crcbl::engine::LoopError<crate::game::GameError>;

// ---- the hosted game ---------------------------------------------------------

/// hud, as the engine's loop hosts it.
#[derive(Debug)]
pub struct Hud {
    game: Game,
    /// Refilled from the ticker every frame, so a steady-state frame does not
    /// allocate a fresh damage vector.
    render_state: RenderState,
    /// The ticker's numbers, snapshotted in [`Hud::draw`].
    ///
    /// A snapshot rather than a read at panel time because
    /// [`HostedGame::debug_sections`] is handed `&self` while reading the ticker
    /// takes its lock. Flappy's `course` field is the same arrangement.
    stats: HudStats,
    /// What the last page drew, from the same snapshot.
    page: PageStats,
}

/// The loop hud runs in.
///
/// A type alias, because the loop is the engine's. `S` is the shell type: the
/// native path builds `Loop<dyn Shell>`, and the tests build
/// `Loop<HeadlessShell>` so they can inject the events a compositor would send.
pub type Loop<S = dyn Shell> = crcbl::engine::Loop<S, Hud>;

/// Runs the full loop.
///
/// # Errors
///
/// [`HudError`] if the shell, the GPU or the ticker failed. Teardown runs on
/// every path.
pub fn run(options: &Options) -> Result<Summary, HudError> {
    crcbl::engine::drive(start(options)?)
}

/// Opens a shell, a window, a GPU and the ticker.
///
/// # Errors
///
/// [`HudError`] if any of them refused.
pub fn start(options: &Options) -> Result<Loop, HudError> {
    let shell = if options.common.headless {
        open_backend(Backend::Headless).map_err(HudError::Shell)?
    } else {
        open().map_err(HudError::NoWindowSystem)?
    };
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
/// [`HudError`] if the window never configured, the GPU would not open, or the
/// ticker's server could not be built.
pub fn with_shell<S: Shell + ?Sized>(
    mut shell: Box<S>,
    options: &Options,
) -> Result<Loop<S>, HudError> {
    let clock_source = Clock::new(options.common.headless);
    let window = open_the_window(
        shell.as_mut(),
        &clock_source,
        options.common.display_mode(),
        options.common.size,
    )?;

    let mut events = 0;
    let extent = wait_for_configure(shell.as_mut(), window, &mut events)?;
    crcbl::log::info!("shell: first configure at {}x{}", extent.0, extent.1);

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
/// [`Booted`] is what both bring-up paths hand over, so the ticker is built and
/// the loop assembled in one place rather than one per path — a second copy is
/// how the browser build would come to run a subtly different sample.
///
/// # Errors
///
/// [`HudError`] if the ticker's server could not be built.
fn assemble<S: Shell + ?Sized>(
    booted: Booted<S, Gpu>,
    options: &Options,
) -> Result<Loop<S>, HudError> {
    let game = Game::new(options.common.tick_hz, options.seed).map_err(HudError::Game)?;
    Ok(Loop::new(
        booted,
        Hud {
            game,
            render_state: RenderState::default(),
            stats: HudStats::default(),
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
) -> Result<WindowId, HudError> {
    Ok(crcbl::engine::open_window(
        shell,
        clock_source,
        &WindowDesc {
            title: "HUD",
            app_id: "sh.kryptic.crcbl.hud",
            size: crcbl::engine::requested_window_size(size),
            mode,
            ..WindowDesc::default()
        },
    )?)
}

impl Hud {
    /// The ticker, for scripted tests and for an embedder that drives it.
    pub const fn game(&self) -> &Game {
        &self.game
    }

    /// What the last frame's page drew.
    pub const fn page(&self) -> &PageStats {
        &self.page
    }
}

/// hud's half of the frame, and nothing else.
impl HostedGame for Hud {
    type Error = crate::game::GameError;
    type Gpu = Gpu;
    type MenuKind = MenuKind;
    /// hud declares no menu action of its own — see [`crate::menu`]. Uninhabited
    /// rather than a placeholder enum, so [`Hud::apply`] is a match on nothing
    /// and the compiler agrees there is no case to handle.
    type MenuAction = core::convert::Infallible;
    type Summary = Summary;

    const NAME: &'static str = "hud";

    fn menus() -> Menus {
        crate::menu::menus()
    }

    fn tick(&mut self, _gpu: &mut Gpu, _tick_dt: f64) {
        self.game.tick();
    }

    /// hud reads no key of its own.
    ///
    /// Every key this sample answers to is the loop's — `ESC` pauses, `F3`
    /// toggles the panel, `F11` goes fullscreen — and the page is driven end to
    /// end by the server's ticker. There is nothing here to forward a key to,
    /// and a binding that did nothing would be worse than none.
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
        self.game.render_state(&mut self.render_state);
        self.stats = self.game.stats();
        let (width, height) = gpu.extent();
        self.page = crate::page::draw(
            draw_list,
            crcbl::math::Vec2::new(width as f32, height as f32),
            gpu.atlas(),
            &self.render_state,
        );
    }

    /// **hud's two modules, and no third.**
    ///
    /// No network section: this sample runs over `InMemoryTransport` and has no
    /// connection to report on. No audio section either — it plays nothing, and
    /// a section that said so would be a module with no system behind it. What
    /// it does have is the ticker that produced the page and the page that came
    /// out of it, and the second of those is the dogfood claim this sample
    /// exists to make: the panel is drawn out of the same primitives, into the
    /// same draw list, and reports how many of them the page used.
    fn debug_sections(&self, panel: &mut crcbl::ui::DebugPanel) {
        panel.add(&self.stats);
        panel.add(&self.page);
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
            wave: self.stats.wave,
            commands: self.page.total(),
        }
    }

    fn log_summary(summary: &Summary) {
        crcbl::log::info!(
            "hud: {} frames, {} ticks, wave {}, {} page commands ({:?})",
            summary.frames,
            summary.ticks,
            summary.wave,
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
    /// [`HudError`] if the shell refused the window.
    pub fn request(
        mut shell: Box<S>,
        options: &Options,
        clock_source: Clock,
    ) -> Result<Self, HudError> {
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
            ),
            options: options.clone(),
        })
    }

    /// Advances start-up. `Ok(None)` means "not yet, poll again next frame".
    ///
    /// # Errors
    ///
    /// [`HudError`] if the window went away before it had a size, if the device
    /// request failed, or if the ticker could not be built.
    pub fn poll(&mut self) -> Result<Option<Loop<S>>, HudError> {
        let Some(booted) = self.boot.poll::<HudError>()? else {
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
        let at = drawn
            .iter()
            .position(|text| text == label)
            .unwrap_or_else(|| panic!("no {label} row in {drawn:?}"));
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

    /// **The panel renders with no network module.** The sections hud has are
    /// the frame's, the GPU's where the device has timestamp queries, and this
    /// sample's own two. Nothing else, and no configuration decided that.
    #[test]
    fn the_overlay_is_composed_of_exactly_the_modules_hud_has() {
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
            &["frame", "gpu", "counters", "hud", "page"]
        } else {
            &["frame", "counters", "hud", "page"]
        };
        assert_eq!(titles, expected, "no module appears that no system offered");

        let drawn = ui_text(&engine);
        for row in ["frame", "fps", "avg", "worst", "window"] {
            assert!(drawn.iter().any(|t| t == row), "missing {row}: {drawn:?}");
        }

        // **Both of this sample's sections reached the draw list with its own
        // numbers in them**, not just their headings. Two frames have run one
        // tick between them, so the ticker has cast once and the page is the
        // opening one: the banner is up and every slot but the one that fired is
        // ready.
        assert_eq!(row_value(&drawn, "tick"), "1");
        assert_eq!(row_value(&drawn, "wave"), "1");
        assert_eq!(row_value(&drawn, "ready"), "3/4");
        assert_eq!(row_value(&drawn, "dmg"), "1");
        // The page section counts the commands the page actually emitted, and
        // they are all still in the list the panel was appended to.
        let page = engine.game().page();
        assert_eq!(row_value(&drawn, "rects"), page.rects.to_string());
        assert_eq!(row_value(&drawn, "outlines"), page.outlines.to_string());
        assert_eq!(row_value(&drawn, "text"), page.text.to_string());
        assert!(page.total() > 0, "the page drew nothing");

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
            hidden.iter().any(|t| t == "HEALTH"),
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
            shown.iter().any(|t| t == "frame") && shown.iter().any(|t| t == "hud"),
            "F3 must show this sample's sections: {shown:?}",
        );
        assert!(
            shown.iter().any(|t| t == "HEALTH"),
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

    /// Escape stops the ticker and puts the one menu this sample has on screen;
    /// escape again starts it. The page keeps drawing behind it either way.
    #[test]
    fn escape_stops_the_ticker_and_shows_the_pause_menu() {
        let mut engine = scripted(&headless(24));
        let window = engine.window();
        engine.frame().expect("a frame");
        engine.frame().expect("a frame");
        let running = engine.game().game().ticks_run();
        assert!(running > 0, "the ticker never ran");
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
            ui_text(&engine).iter().any(|t| t == "HEALTH"),
            "the page is drawn behind the panel",
        );
        engine.finish(ExitReason::FrameBudget).expect("teardown");
    }

    /// Ticks are paced by the clock and not the frame rate, which is what makes
    /// the ticker's rhythm a property of `--tick-hz` rather than of the display.
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

    /// `--seed` reaches the ticker through the whole start-up path, so the flag
    /// is not a number the parser stores and nothing reads.
    #[test]
    fn the_seed_flag_reaches_the_ticker() {
        let hash = |seed: u64| {
            let mut engine = scripted(&Options {
                seed,
                ..headless(40)
            });
            while let Ok(Flow::Continue) = engine.frame() {}
            let hash = engine.game().game().state_hash();
            engine.finish(ExitReason::FrameBudget).expect("teardown");
            hash
        };
        let published = hash(crate::game::DEFAULT_SEED);
        assert_ne!(published, hash(crate::game::DEFAULT_SEED + 1));
        assert_eq!(published, hash(crate::game::DEFAULT_SEED));
    }

    /// The whole start-up path — window, GPU, server, client, handshake — reaches
    /// a page with the opening wave on it.
    #[test]
    fn a_run_starts_on_the_first_wave_with_a_full_page() {
        let summary = run(&headless(5)).expect("headless runs everywhere");
        assert_eq!(summary.frames, 5);
        assert_eq!(summary.ticks, 4);
        assert!(summary.events >= 1, "at least a configure event");
        assert_eq!(summary.wave, 1);
        assert!(!summary.paused);
        // The opening page: the vitals panel with both bars, the wave banner and
        // the four ability slots, all of it drawn on the very first frame.
        assert!(summary.commands >= 20, "{} commands", summary.commands);
    }

    /// The summary reports the wave the ticker actually reached, not the one it
    /// started on — the field would otherwise read `1` forever and every short
    /// run in this file would agree with it.
    #[test]
    fn the_summary_reports_the_wave_the_ticker_actually_reached() {
        // One frame is one tick after the baseline, so a run long enough to
        // cross `WAVE_TICKS` needs a frame more than that.
        let frames = crate::game::WAVE_TICKS + 2;
        let summary = run(&headless(frames)).expect("headless runs everywhere");
        assert_eq!(summary.ticks, frames - 1);
        assert_eq!(
            summary.wave, 2,
            "{} ticks is past the first wave",
            summary.ticks
        );
    }
}
