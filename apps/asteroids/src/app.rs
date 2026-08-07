//! Asteroids' start-up, and the seven methods the engine's loop calls.
//!
//! # There is no loop in this file
//!
//! There was, and `docs/backlog.md` called it the third copy: the pump's key
//! branch, `lose_focus`, the F11 toggle, `Loop::paused` and the pointer's
//! press-capture bookkeeping were the same code in three files. All of it is
//! [`crcbl::engine::Loop`]'s now and this crate reaches it through
//! [`HostedGame`].
//!
//! ```text
//! Loop::frame()                     ← the engine's
//!   pump, input, menu, pause, resize
//!   run_ticks  ─────────────────────→ Asteroids::tick
//!   draw_list.clear()
//!     ─────────────────────────────→ Asteroids::draw  (field + HUD)
//!     menu, debug overlay             ← the engine's
//!   gpu.frame()
//! ```
//!
//! # What is this sample's own
//!
//! **The alpha.** Every other sample draws the last tick's state and is right to
//! — a pipe and a paddle are the same picture at 60 Hz and at 144. This one
//! turns things, so the frame is asked how far through a tick it sits and the
//! rotations are interpolated across it. [`FrameInfo::alpha`] is that number,
//! read after the accumulator has drained; [`render_alpha`] is what a paused
//! frame does with it, and `game::lerp_angle` is what the renderer does.
//!
//! **The simulation is still inside `run_ticks`'s `while`, not after it.**
//! Anything stepped once per frame has a speed proportional to the frame rate,
//! which a headless run — where a frame is pinned to exactly 1/60 s — cannot
//! see.

use crcbl::core::input::KeyCode;
use crcbl::engine::{
    Booted, Clock, ExitReason, FrameInfo, HostedGame, RunSummary, wait_for_configure,
};
use crcbl::math::Vec2;
use crcbl::prelude::*;
use crcbl::shell::{
    DisplayMode, LogicalSize, ShellBackend as Backend, WindowId, open, open_backend,
};
use crcbl::ui::draw_list::DrawList;

use crate::game::{self, Game, GameState, RenderState};
use crate::gpu::Gpu;
use crate::menu::{self, Fire, MenuKind, Menus};

pub use crate::args::Options;

// ---- summary ----------------------------------------------------------------

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Summary {
    pub backend: Backend,
    pub frames: u64,
    pub ticks: u64,
    pub events: u64,
    pub extent: (u32, u32),
    pub exit: ExitReason,
    pub score: u32,
    pub lives: u32,
    /// Zero-based, like the simulation's. `main.rs` prints `wave + 1`.
    pub wave: u32,
    pub state: GameState,
    /// Whether the simulation was stopped when the run ended. Beside `state`
    /// rather than inside it: pause is the loop declining to advance the
    /// simulation, not a state the simulation is in.
    pub paused: bool,
    /// The mode the window system actually had the window in, **not** the one
    /// the run last asked for.
    pub mode: DisplayMode,
}

// ---- errors -----------------------------------------------------------------

/// What can stop asteroids: the loop's own failures, plus this game's.
///
/// An alias rather than an enum. Every sample had the same five loop
/// variants written out with the same `Display` arms, so they live in
/// [`crcbl::engine::LoopError`] now and this names the game error that
/// goes in the sixth. Its docs say why a game error is wrapped by name —
/// `.map_err(AsteroidsError::Game)` — while the engine's three convert with `?`.
pub type AsteroidsError = crcbl::engine::LoopError<game::GameError>;

// ---- the game ---------------------------------------------------------------

/// The key a menu's `FLY` and `TRY AGAIN` buttons stand for.
///
/// Fired as a real key event rather than by calling into `Game`, because
/// starting and restarting a game is the simulation's business and the
/// simulation is driven by its action map.
const FIRE_KEY: KeyCode = KeyCode::Space;

/// Asteroids, as the engine's loop hosts it.
///
/// **The loop is not here any more.** The pump, the input routing, the
/// fixed-step accumulator, the menu, the debug panel, the budget and teardown
/// are [`crcbl::engine::Loop`]'s, and were the same in all five samples. What is
/// left is what was always this game's: the simulation, the state it renders
/// from, and its HUD.
#[derive(Debug)]
pub struct Asteroids {
    game: Game,
    /// Refilled from the simulation every frame, so a steady-state frame does
    /// not allocate a fresh rock list.
    render_state: RenderState,
    hud: HudStrings,
}

/// The loop asteroids runs in.
///
/// A type alias, because the loop is the engine's. `S` is the shell type: the
/// native and browser paths both build `Loop<dyn Shell>`, and the tests build
/// `Loop<HeadlessShell>` so they can inject the events a compositor would send.
pub type Loop<S = dyn Shell> = crcbl::engine::Loop<S, Asteroids>;

/// Runs the full loop.
///
/// # Errors
///
/// [`AsteroidsError`] if the shell, the GPU or the game failed. Teardown runs on
/// every path: a failing frame must still release the swapchain, the surface and
/// the window, or `crcbl-vk`'s device teardown logs objects still alive.
pub fn run(options: &Options) -> Result<Summary, AsteroidsError> {
    crcbl::engine::drive(start(options)?)
}

/// Opens a shell, a window, a GPU and the game.
///
/// # Errors
///
/// [`AsteroidsError`] if any of them refused.
pub fn start(options: &Options) -> Result<Loop, AsteroidsError> {
    let shell = if options.common.headless {
        open_backend(Backend::Headless).map_err(AsteroidsError::Shell)?
    } else {
        open().map_err(AsteroidsError::NoWindowSystem)?
    };
    with_shell(shell, options)
}

/// Builds the loop on an already-open shell, blocking on both waits.
///
/// The browser cannot use this — a main thread may not sit in
/// [`wait_for_configure`] — and takes [`PendingLoop`] instead. What the two
/// share is everything after the waiting, which is [`assemble`].
///
/// # Errors
///
/// [`AsteroidsError`] if the window never configured, the GPU would not open, or
/// the game could not be built.
pub fn with_shell<S: Shell + ?Sized>(
    mut shell: Box<S>,
    options: &Options,
) -> Result<Loop<S>, AsteroidsError> {
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
/// [`Booted`] is what both bring-up paths hand over, so the game is built and
/// the loop assembled in one place rather than one per path — a second copy is
/// how the browser build would come to run a subtly different game.
///
/// # Errors
///
/// [`AsteroidsError`] if the game could not be built.
fn assemble<S: Shell + ?Sized>(
    booted: Booted<S, Gpu>,
    options: &Options,
) -> Result<Loop<S>, AsteroidsError> {
    let game = Game::with_seed(
        options.common.headless,
        options.common.tick_hz,
        options.seed,
    )
    .map_err(AsteroidsError::Game)?;
    Ok(Loop::new(
        booted,
        Asteroids {
            game,
            render_state: RenderState::default(),
            hud: HudStrings::default(),
        },
        options.common.loop_config(),
    ))
}

impl Asteroids {
    /// The simulation, for scripted tests and for an embedder that drives it.
    pub const fn game(&self) -> &Game {
        &self.game
    }

    /// The simulation, mutably. See [`Asteroids::game`].
    pub const fn game_mut(&mut self) -> &mut Game {
        &mut self.game
    }

    /// What the last [`draw`](HostedGame::draw) read out of the simulation.
    ///
    /// The frame's own copy, not a fresh one: a test that re-read the game
    /// would be asking a different question from the one the renderer was
    /// answered with.
    pub const fn render_state(&self) -> &RenderState {
        &self.render_state
    }
}

/// How far through a tick the frame should draw, given whether it is paused.
///
/// **A paused frame draws the last tick, not part way past one of them.** The
/// clock keeps running while the simulation does not, so the accumulator's
/// fraction goes on climbing and interpolating against it would have the rocks
/// creep on a stopped game. One is "all the way to the tick that did happen".
#[must_use]
pub fn render_alpha(paused: bool, clock_alpha: f32) -> f32 {
    if paused { 1.0 } else { clock_alpha }
}

/// Asteroids' half of the frame, and nothing else.
impl HostedGame for Asteroids {
    type Error = game::GameError;
    type Gpu = Gpu;
    type MenuKind = MenuKind;
    type MenuAction = Fire;
    type Summary = Summary;

    const NAME: &'static str = "asteroids";

    fn menus() -> Menus {
        menu::menus()
    }

    fn tick(&mut self, _gpu: &mut Gpu, _tick_dt: f64) {
        self.game.tick();
    }

    fn key_event(&mut self, key: KeyCode, pressed: bool) {
        // Forwarded to the game, which replays it at the start of the next
        // tick. A frame that runs no ticks loses nothing.
        self.game.key_event(key, pressed);
    }

    fn menu_action(id: crcbl::ui::WidgetId) -> Option<Fire> {
        menu::fire_from_id(id)
    }

    fn apply(&mut self, action: Fire) {
        match action {
            // A real key event rather than a call into `Game`: starting a game
            // is the simulation's business and the simulation is driven by its
            // action map. The release is queued straight after the press because
            // the trigger is an *edge* — a press with no release leaves the
            // action held, which in this game is a magazine emptied into
            // whatever the ship happens to be pointing at.
            Fire::Now => {
                self.game.key_event(FIRE_KEY, true);
                self.game.key_event(FIRE_KEY, false);
            }
        }
    }

    fn menu_kind(
        &mut self,
        _menus: &mut crcbl::ui::menu::MenuSet<MenuKind>,
        paused: bool,
    ) -> MenuKind {
        MenuKind::of(paused, &self.render_state)
    }

    fn draw(&mut self, gpu: &mut Gpu, draw_list: &mut DrawList, frame: FrameInfo) {
        self.game.render_state(&mut self.render_state);
        // **After the accumulator has been drained**, which is what
        // `FrameClock::alpha` asks for and what `FrameInfo::alpha` carries:
        // read before, it saturates just under one rather than reporting the
        // fraction of a tick that is left.
        gpu.set_world(&self.render_state, render_alpha(frame.paused, frame.alpha));
        self.hud.refresh(&self.render_state, frame.paused);
        draw_hud(draw_list, &self.hud);
    }

    fn summary(&self, run: RunSummary) -> Summary {
        Summary {
            backend: run.backend,
            frames: run.frames,
            ticks: run.ticks,
            events: run.events,
            extent: run.extent,
            exit: run.exit,
            score: self.game.score,
            lives: self.game.lives,
            wave: self.game.wave,
            state: self.game.state,
            paused: run.paused,
            mode: run.mode,
        }
    }

    fn log_summary(summary: &Summary) {
        crcbl::log::info!(
            "asteroids: {} frames, {} ticks, score {}, wave {} ({:?}, {:?})",
            summary.frames,
            summary.ticks,
            summary.score,
            summary.wave + 1,
            summary.state,
            summary.exit,
        );
    }
}

// ---- polled start-up --------------------------------------------------------

/// Creates the one window this game has: its title, its app id, its size.
///
/// Everything else is [`crcbl::engine::open_window`]'s.
fn open_the_window<S: Shell + ?Sized>(
    shell: &mut S,
    clock_source: &Clock,
    mode: DisplayMode,
    size: Option<crcbl::shell::PhysicalSize>,
) -> Result<WindowId, AsteroidsError> {
    Ok(crcbl::engine::open_window(
        shell,
        clock_source,
        &WindowDesc {
            title: "Asteroids",
            app_id: "sh.kryptic.crcbl.asteroids",
            // `--size` names pixels; the window request is logical at scale 1,
            // which is exactly the extent the headless offscreen ring renders at.
            size: size.map_or(LogicalSize::new(960.0, 720.0), |size| size.to_logical(1.0)),
            // Asked for at creation rather than switched to afterwards, so
            // `--fullscreen` does not show a decorated window first.
            mode,
            ..WindowDesc::default()
        },
    )?)
}

/// A [`Loop`] being started one poll at a time, for a caller that may not
/// block — which on a browser main thread is every caller.
///
/// The state machine, the pump and the resize-during-start-up race are
/// [`crcbl::engine::PolledBoot`]'s; all that is left here is this game's
/// `Options` and the `Loop::assemble` call the engine deliberately stops
/// short of.
#[derive(Debug)]
pub struct PendingLoop<S: Shell + ?Sized = dyn Shell> {
    boot: crcbl::engine::PolledBoot<S, Gpu>,
    options: Options,
}

impl<S: Shell + ?Sized> PendingLoop<S> {
    /// Creates the window and starts the wait, without blocking on either half.
    ///
    /// `clock_source` is the caller's because the browser's cannot be
    /// [`Clock::new`]'s — see [`Loop::set_frame_step`].
    ///
    /// # Errors
    ///
    /// [`AsteroidsError`] if the shell refused the window.
    pub fn request(
        mut shell: Box<S>,
        options: &Options,
        clock_source: Clock,
    ) -> Result<Self, AsteroidsError> {
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
    /// [`AsteroidsError`] if the window went away before it had a size, if the device
    /// request failed, or if the game could not be built.
    pub fn poll(&mut self) -> Result<Option<Loop<S>>, AsteroidsError> {
        let Some(booted) = self.boot.poll::<AsteroidsError>()? else {
            return Ok(None);
        };
        assemble(booted, &self.options).map(Some)
    }
}

// ---- drawing ----------------------------------------------------------------

/// The HUD's two lines, rebuilt only when the numbers behind them change.
///
/// `DrawList::text` needs an owned `String`, so the alternative is a `format!`
/// per line every frame at whatever rate the window runs.
#[derive(Debug, Default)]
struct HudStrings {
    score: String,
    state: String,
    last: Option<HudKey>,
}

type HudKey = (u32, u32, u32, u32, Option<GameState>, bool);

impl HudStrings {
    /// **`paused` wins over the simulation's state**, which is the bug flappy
    /// fixed: the status line used to read straight off the *server's* idea of
    /// what was happening, and the server is still playing while the window sits
    /// behind a browser.
    fn refresh(&mut self, render: &RenderState, paused: bool) {
        let key = (
            render.score,
            render.best,
            render.lives,
            render.wave,
            render.state,
            paused,
        );
        if self.last == Some(key) {
            return;
        }
        self.last = Some(key);

        use std::fmt::Write as _;
        self.score.clear();
        let _ = write!(
            self.score,
            "Score: {}  Best: {}  Lives: {}  Wave: {}",
            render.score,
            render.best,
            render.lives,
            render.wave + 1,
        );
        self.state.clear();
        self.state.push_str(if paused {
            "PAUSED - press ESC"
        } else {
            match render.state {
                Some(GameState::WaitingToStart) | None => "SPACE to fire, arrows to fly",
                Some(GameState::Playing) => "Playing",
                Some(GameState::GameOver) => "Game over - press SPACE",
            }
        });
    }
}

/// Draws the HUD, and nothing else.
fn draw_hud(dl: &mut DrawList, hud: &HudStrings) {
    // Wider than flappy's 340 and than this game's own previous 360: the score
    // line gained `Best: …` when the save file landed, and a panel narrower than
    // the text it is behind reads as a clipped HUD rather than as a backdrop.
    dl.rect(
        Vec2::new(4.0, 4.0),
        Vec2::new(430.0, 52.0),
        [0.1, 0.1, 0.15, 0.85],
    );
    dl.text(
        Vec2::new(10.0, 10.0),
        hud.score.as_str(),
        [1.0, 1.0, 0.3, 1.0],
        16.0,
    );
    dl.text(
        Vec2::new(10.0, 32.0),
        hud.state.as_str(),
        [0.7, 0.7, 1.0, 1.0],
        14.0,
    );
}

// ---- tests ------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use crcbl::args::Common;
    use crcbl::engine::{Flow, MENU_ACTIVATE_KEY, PAUSE_KEY};

    use super::*;
    use crcbl::core::input::PointerButton;
    use crcbl::shell::{ButtonState as PointerState, HeadlessShell, PhysicalPoint, ShellBackend};
    use crcbl::ui::draw_list::DrawCommand;

    /// Options every test in this module builds its loop from.
    ///
    /// `Null` is not a detail: `headless` only says "no window", and without a
    /// backend named here the loop picks the real one and fails to start on any
    /// machine with no Vulkan driver — which is every plain CI runner. Breakout
    /// and flappy pin it for the same reason.
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
    #[allow(dead_code)]
    fn headless_with(frames: u64, edit: impl FnOnce(&mut Common)) -> Options {
        let mut options = headless(frames);
        edit(&mut options.common);
        options
    }

    fn headless_loop() -> Loop<dyn Shell> {
        start(&headless(8)).expect("a headless loop always starts")
    }

    /// A loop on a shell the test can post events to.
    fn scripted(options: &Options) -> Loop<HeadlessShell> {
        with_shell(Box::new(HeadlessShell::new()), options).expect("headless always starts")
    }

    fn run_frames(engine: &mut Loop<HeadlessShell>, frames: u32) {
        for _ in 0..frames {
            assert_eq!(
                engine.frame().expect("a frame"),
                Flow::Continue,
                "the loop stopped early",
            );
        }
    }

    /// Every string the UI pass will draw this frame.
    fn ui_text(engine: &Loop<HeadlessShell>) -> Vec<String> {
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

    // -----------------------------------------------------------------------
    // The loop
    // -----------------------------------------------------------------------

    /// The loop runs, ticks the simulation and stops when its budget is spent.
    #[test]
    fn a_headless_run_presents_its_budget_and_stops() {
        let mut engine = headless_loop();
        let reason = loop {
            match engine.frame().expect("a headless frame never fails") {
                Flow::Continue => {}
                Flow::Stop(reason) => break reason,
            }
        };
        assert_eq!(reason, ExitReason::FrameBudget);
        let summary = engine.finish(reason).expect("teardown");
        assert_eq!(summary.frames, 8);
        assert_eq!(summary.backend, ShellBackend::Headless);
        assert!(summary.ticks > 0, "the simulation never ran");
        assert_eq!(summary.state, GameState::WaitingToStart);
        assert_eq!(summary.lives, game::STARTING_LIVES);
    }

    /// Escape stops the simulation without stopping the loop, and does not
    /// reach the game: a pause the simulation knew about would be a state a
    /// scripted, seeded run could reach.
    #[test]
    fn escape_pauses_the_simulation_and_never_reaches_the_game() {
        let mut engine = headless_loop();
        engine.frame().expect("a frame");
        engine.set_paused(true);
        let before = engine.ticks();
        for _ in 0..3 {
            engine.frame().expect("a frame");
        }
        assert_eq!(engine.ticks(), before, "a paused frame ran a tick");
        assert!(engine.is_paused());
    }

    /// The HUD reports the pause rather than the simulation's state.
    #[test]
    fn the_hud_says_paused_even_though_the_simulation_is_not() {
        let mut hud = HudStrings::default();
        let render = RenderState {
            state: Some(GameState::Playing),
            lives: 3,
            ..RenderState::default()
        };
        hud.refresh(&render, false);
        assert_eq!(hud.state, "Playing");
        hud.refresh(&render, true);
        assert!(hud.state.contains("PAUSED"), "{}", hud.state);
    }

    // -----------------------------------------------------------------------
    // The art, through the loop
    // -----------------------------------------------------------------------

    /// **The field reaches the sprite pass and the HUD reaches the UI pass**,
    /// which is what the placeholder `draw_field` used to do in one place and
    /// wrongly.
    ///
    /// The count is against the board rather than "more than zero": a scene that
    /// lost the rocks and kept the ship would pass the weaker version. A wave
    /// spawns its rocks **on** the border, so most of them straddle a seam and
    /// the wrap rule draws each at every wrapped offset — the expected count is
    /// computed through [`crate::art::wrapped_offsets`], the same rule the scene
    /// draws with, so the two cannot drift.
    #[test]
    fn the_frame_hands_the_field_to_the_sprite_pass_and_the_hud_to_the_ui_pass() {
        let mut engine = scripted(&headless(4));
        run_frames(&mut engine, 1);

        let render = engine.game().render_state();
        let rocks = render.rocks.len();
        assert_eq!(
            rocks,
            game::wave_rocks(0) as usize,
            "the first wave should be on the field",
        );
        assert!(render.ship_alive);
        let alpha = f64::from(engine.gpu().alpha_for_test());
        let copies: usize = render
            .rocks
            .iter()
            .map(|rock| {
                let centre = crate::art::drawn_centre(
                    rock.prev_position,
                    rock.position,
                    rock.teleported,
                    alpha,
                );
                crate::art::wrapped_offsets(centre, rock.size.radius()).count()
            })
            .sum();
        let sprites = engine.gpu_mut().scene_sprites();
        assert_eq!(
            sprites.len(),
            copies + 1,
            "every rock copy and the ship, and nothing else",
        );

        // Nothing the game draws is a UI rectangle any more: the HUD panel is,
        // and the menu's own geometry, and that is all.
        let outlines = engine
            .gpu()
            .draw_list()
            .commands()
            .iter()
            .filter(|c| matches!(c, DrawCommand::RectOutline { .. }))
            .count();
        assert_eq!(outlines, 0, "a rock is art now, not an outline");
        assert!(
            ui_text(&engine).iter().any(|t| t.starts_with("Score:")),
            "the HUD is missing: {:?}",
            ui_text(&engine),
        );
        engine.finish(ExitReason::FrameBudget).expect("teardown");
    }

    /// **A paused frame is drawn at the newest tick, not somewhere between two
    /// of them.** See [`render_alpha`]: the clock keeps running while the
    /// simulation does not, so a paused field would otherwise rock back and
    /// forth between the last two ticks it ran.
    #[test]
    fn a_paused_frame_is_drawn_at_the_state_it_stopped_at() {
        assert_eq!(render_alpha(true, 0.0), 1.0);
        assert_eq!(render_alpha(true, 0.37), 1.0);
        assert_eq!(render_alpha(false, 0.37), 0.37);
        assert_eq!(render_alpha(false, 0.0), 0.0);

        // And the loop really uses it: a paused loop hands the renderer 1.0
        // however far through a tick the frame clock happens to be.
        let mut engine = scripted(&headless(60));
        let window = engine.window();
        run_frames(&mut engine, 2);
        engine
            .shell_mut()
            .key_press(window, PAUSE_KEY)
            .expect("the window is live");
        run_frames(&mut engine, 2);
        assert!(engine.is_paused());
        assert_eq!(engine.gpu().alpha_for_test(), 1.0);
        engine.finish(ExitReason::FrameBudget).expect("teardown");
    }

    /// **The ship's drawn rotation follows the heading it is actually flying**,
    /// through the whole loop rather than through `art.rs` alone.
    ///
    /// The wiring nothing else would catch: `Gpu::set_world` is the only caller
    /// of `Scene::build`, and one that dropped the heading would leave every
    /// test in `art.rs` green and the ship pointing north forever.
    #[test]
    fn turning_the_ship_turns_the_sprite() {
        let mut engine = scripted(&headless(200));
        let window = engine.window();
        // Start the game, then hold left.
        engine
            .shell_mut()
            .key_press(window, KeyCode::Space)
            .expect("the window is live");
        run_frames(&mut engine, 2);
        engine
            .shell_mut()
            .key_release(window, KeyCode::Space)
            .expect("the window is live");
        run_frames(&mut engine, 2);
        assert_eq!(engine.game().game().state, GameState::Playing);

        let ship_rotation = |engine: &mut Loop<HeadlessShell>| -> f32 {
            let sprites = engine.gpu_mut().scene_sprites();
            sprites.last().expect("the ship is drawn last").rotation
        };
        let before = ship_rotation(&mut engine);

        engine
            .shell_mut()
            .key_press(window, KeyCode::ArrowLeft)
            .expect("the window is live");
        run_frames(&mut engine, 20);
        let after = ship_rotation(&mut engine);
        assert!(
            (after - before).abs() > 0.1,
            "twenty ticks of a held turn moved the sprite from {before} to {after}",
        );
        assert!(
            (f64::from(after) - engine.game().game().ship_heading).abs() < 0.2,
            "the sprite is at {after} and the ship is flying {}",
            engine.game().game().ship_heading,
        );
        engine.finish(ExitReason::FrameBudget).expect("teardown");
    }

    // -----------------------------------------------------------------------
    // The menus
    // -----------------------------------------------------------------------

    /// **The start menu is on screen before the first shot, it is centred, and
    /// it reaches both passes.** The text is in the draw list the UI pass
    /// uploads and the frame is in the sprite list the menu pass draws — a menu
    /// that only made it to one of the two is a panel with no words or words
    /// with no panel.
    #[test]
    fn the_start_menu_is_drawn_before_the_first_shot() {
        let mut engine = scripted(&headless(60));
        run_frames(&mut engine, 2);
        assert_eq!(engine.menu_kind(), MenuKind::Start);

        let drawn = ui_text(&engine);
        assert!(
            drawn.iter().any(|t| t == "ASTEROIDS") && drawn.iter().any(|t| t == "FLY"),
            "the start menu's text is not in the draw list: {drawn:?}",
        );

        let extent = engine.extent();
        let layout = engine.menu_layout().expect("a menu is showing").clone();

        // **Centred on the framebuffer it is drawn into.** Read off the layout
        // the frame actually used, so a `draw_menu` measuring against the wrong
        // extent fails here rather than looking right.
        let centre = layout.panel_centre();
        assert!(
            (centre.x - extent.0 as f32 / 2.0).abs() < 1.0
                && (centre.y - extent.1 as f32 / 2.0).abs() < 1.0,
            "the panel is centred at {centre:?} in a {extent:?} framebuffer",
        );

        let sprites = engine.gpu().menu_sprites();
        // The scrim, the window frame's nine quads, and nine per button.
        assert_eq!(sprites.len(), 1 + 9 + 9 * 3, "{}", sprites.len());

        // **Centred, measured on what the menu pass was actually handed** rather
        // than on a layout the test recomputes. `crcbl::render::menu_camera` puts
        // the origin at the middle of the framebuffer, so the window frame's
        // nine quads have to straddle it.
        let panel = &sprites[1..10];
        let min_x = panel.iter().map(|s| s.rect[0]).fold(f32::MAX, f32::min);
        let min_y = panel.iter().map(|s| s.rect[1]).fold(f32::MAX, f32::min);
        let max_x = panel
            .iter()
            .map(|s| s.rect[0] + s.rect[2])
            .fold(f32::MIN, f32::max);
        let max_y = panel
            .iter()
            .map(|s| s.rect[1] + s.rect[3])
            .fold(f32::MIN, f32::max);
        assert!(max_x > min_x && max_y > min_y, "the panel has no area");
        assert!(
            ((min_x + max_x) / 2.0).abs() < 0.5 && ((min_y + max_y) / 2.0).abs() < 0.5,
            "the panel spans {min_x}..{max_x} by {min_y}..{max_y}, which is not \
             centred on a {extent:?} framebuffer",
        );

        assert_eq!(
            sprites[0].rect,
            [
                -(extent.0 as f32) / 2.0,
                -(extent.1 as f32) / 2.0,
                extent.0 as f32,
                extent.1 as f32,
            ],
            "the scrim does not cover the framebuffer",
        );
        engine.finish(ExitReason::FrameBudget).expect("teardown");
    }

    /// **A game being played draws no menu at all**, and the menu pass is handed
    /// nothing — which is what makes it free rather than cheap.
    #[test]
    fn a_game_in_play_draws_no_menu() {
        let mut engine = scripted(&headless(60));
        let window = engine.window();
        engine
            .shell_mut()
            .key_press(window, KeyCode::Space)
            .expect("the window is live");
        run_frames(&mut engine, 10);
        assert_eq!(engine.game().game().state, GameState::Playing);
        assert_eq!(engine.menu_kind(), MenuKind::None);
        assert!(
            engine.gpu().menu_sprites().is_empty(),
            "a playing frame submitted {} menu sprites",
            engine.gpu().menu_sprites().len(),
        );
        engine.finish(ExitReason::FrameBudget).expect("teardown");
    }

    /// **One menu per state, and only the state's own**, through the real loop:
    /// the start menu gives way to none when the game starts, and to the pause
    /// menu the moment it is paused.
    #[test]
    fn each_state_draws_its_own_menu_and_no_other() {
        let mut engine = scripted(&headless(60));
        let window = engine.window();
        run_frames(&mut engine, 2);
        assert_eq!(engine.menu_kind(), MenuKind::Start);
        assert!(!ui_text(&engine).iter().any(|t| t == "PAUSED"));

        engine
            .shell_mut()
            .key_press(window, KeyCode::Space)
            .expect("the window is live");
        run_frames(&mut engine, 6);
        assert_eq!(engine.menu_kind(), MenuKind::None);
        let drawn = ui_text(&engine);
        assert!(
            !drawn.iter().any(|t| t == "ASTEROIDS") && !drawn.iter().any(|t| t == "PAUSED"),
            "a playing frame drew a menu: {drawn:?}",
        );

        engine
            .shell_mut()
            .key_press(window, PAUSE_KEY)
            .expect("the window is live");
        engine.frame().expect("a frame");
        assert_eq!(engine.menu_kind(), MenuKind::Paused);
        let drawn = ui_text(&engine);
        assert!(
            drawn.iter().any(|t| t == "PAUSED") && drawn.iter().any(|t| t == "RESUME"),
            "the pause menu is not drawn: {drawn:?}",
        );
        assert!(
            !drawn.iter().any(|t| t == "ASTEROIDS"),
            "two menus at once: {drawn:?}",
        );
        engine.finish(ExitReason::FrameBudget).expect("teardown");
    }

    /// **Keyboard activation works through the real loop.** Escape opens the
    /// pause menu, Enter fires `RESUME`, and the game is running again — with no
    /// pointer anywhere in the story.
    #[test]
    fn enter_on_the_pause_menu_resumes_the_game() {
        let mut engine = scripted(&headless(60));
        let window = engine.window();
        run_frames(&mut engine, 2);

        engine
            .shell_mut()
            .key_press(window, PAUSE_KEY)
            .expect("the window is live");
        engine.frame().expect("a frame");
        assert!(engine.is_paused());
        assert_eq!(engine.menu_kind(), MenuKind::Paused);

        // Press and release, because the commit fires on the *release* — the
        // pressed frame of the skin has to be on screen while the key is down.
        engine
            .shell_mut()
            .key_press(window, MENU_ACTIVATE_KEY)
            .expect("the window is live");
        engine.frame().expect("a frame");
        assert!(engine.is_paused(), "the press alone must not fire it");

        engine
            .shell_mut()
            .key_release(window, MENU_ACTIVATE_KEY)
            .expect("the window is live");
        engine.frame().expect("a frame");
        assert!(!engine.is_paused(), "Enter on RESUME did not resume");
        engine.finish(ExitReason::FrameBudget).expect("teardown");
    }

    /// **The pointer works too**, through the same actions — and a click that
    /// lands on nothing leaves the game paused, which is what the corner
    /// assertion is for.
    #[test]
    fn a_click_on_resume_resumes_and_a_click_off_every_button_does_not() {
        let mut engine = scripted(&headless(60));
        let window = engine.window();
        run_frames(&mut engine, 2);
        engine
            .shell_mut()
            .key_press(window, PAUSE_KEY)
            .expect("the window is live");
        engine.frame().expect("a frame");
        assert!(engine.is_paused());

        let corner = crcbl::math::Vec2::new(3.0, 3.0);
        let item = engine.menu_layout().expect("a menu is showing").items()[0];
        assert!(
            corner.x < item.min.x || corner.y < item.min.y,
            "the corner is inside a button, so the test below proves nothing",
        );
        let at = (item.min + item.max) * 0.5;

        let click = |engine: &mut Loop<HeadlessShell>, pos: crcbl::math::Vec2| {
            let point = PhysicalPoint::new(f64::from(pos.x), f64::from(pos.y));
            engine
                .shell_mut()
                .button(
                    window,
                    PointerButton::Left,
                    PointerState::Pressed,
                    Some(point),
                )
                .expect("the window is live");
            engine.frame().expect("a frame");
            engine
                .shell_mut()
                .button(
                    window,
                    PointerButton::Left,
                    PointerState::Released,
                    Some(point),
                )
                .expect("the window is live");
            engine.frame().expect("a frame");
        };

        click(&mut engine, corner);
        assert!(engine.is_paused(), "a click on nothing resumed the game");

        click(&mut engine, at);
        assert!(!engine.is_paused(), "a click on RESUME did not resume");
        engine.finish(ExitReason::FrameBudget).expect("teardown");
    }

    /// **The key printed on a button still does what it always did.** Space is
    /// the only fire binding and the menu never takes it, so it starts the game
    /// with the start menu on screen and no menu key involved.
    #[test]
    fn space_still_fires_with_the_start_menu_showing() {
        let mut engine = scripted(&headless(60));
        let window = engine.window();
        run_frames(&mut engine, 2);
        assert_eq!(engine.menu_kind(), MenuKind::Start);

        engine
            .shell_mut()
            .key_press(window, KeyCode::Space)
            .expect("the window is live");
        engine
            .shell_mut()
            .key_release(window, KeyCode::Space)
            .expect("the window is live");
        run_frames(&mut engine, 10);
        assert_eq!(
            engine.menu_kind(),
            MenuKind::None,
            "the game never started, so the start menu ate the key",
        );
        engine.finish(ExitReason::FrameBudget).expect("teardown");
    }

    /// And the `FLY` button does the same thing the key does — the action goes
    /// through `game.rs`'s action map rather than round it.
    #[test]
    fn the_fly_button_starts_the_game() {
        let mut engine = scripted(&headless(60));
        let window = engine.window();
        run_frames(&mut engine, 2);
        assert_eq!(engine.menu_kind(), MenuKind::Start);

        engine
            .shell_mut()
            .key_press(window, MENU_ACTIVATE_KEY)
            .expect("the window is live");
        engine
            .shell_mut()
            .key_release(window, MENU_ACTIVATE_KEY)
            .expect("the window is live");
        run_frames(&mut engine, 10);
        assert_eq!(
            engine.menu_kind(),
            MenuKind::None,
            "FLY did not start the game",
        );
        engine.finish(ExitReason::FrameBudget).expect("teardown");
    }

    // -----------------------------------------------------------------------
    // Focus
    // -----------------------------------------------------------------------

    /// Losing focus releases every key the game thinks is held.
    ///
    /// It matters more here than in either earlier sample: turn and thrust are
    /// *held* actions, so a lost release leaves the ship spinning for the rest
    /// of the session.
    #[test]
    fn losing_focus_releases_the_keys_the_game_still_thinks_are_down() {
        let mut engine = scripted(&headless(8));
        let window = engine.window();
        engine.frame().expect("a frame");

        // Through the shell, not by poking the list: the loop only knows a key
        // is held because it saw the press go by, and a test that filled the
        // list itself would pass with the pump's key branch deleted.
        engine
            .shell_mut()
            .key_press(window, KeyCode::ArrowLeft)
            .expect("the window is live");
        engine.frame().expect("a frame");
        assert_eq!(
            engine.held_keys(),
            [KeyCode::ArrowLeft],
            "the loop never noticed the key go down",
        );

        engine
            .shell_mut()
            .set_focus(window, false)
            .expect("the window is live");
        engine.frame().expect("a frame");
        assert!(engine.held_keys().is_empty(), "the held list survived");
        assert!(engine.is_paused(), "focus loss must pause");
    }

    /// **The inset the browser gate clicks to restore focus is over no button,
    /// and the centre is over `RESUME`.**
    ///
    /// Both halves are load-bearing for `web/tools/browser-e2e.mjs`, whose
    /// group E has to tell "focus came back" from "the player pressed RESUME".
    /// It clicks the corner precisely because the centre is a button; a menu
    /// that grew until it reached the corner would make that gate silently
    /// meaningless, and this fast test is what fails instead. The same test
    /// exists in `apps/breakout` and `apps/flappy` — a third copy, because the
    /// menu geometry is per-sample even though the constant is not.
    #[test]
    fn a_focusing_click_off_every_button_leaves_the_game_paused() {
        /// Matches `FOCUS_CLICK_INSET` in `web/tools/browser-e2e.mjs`.
        const INSET: f32 = 8.0;

        let mut engine = scripted(&headless(60));
        let window = engine.window();
        run_frames(&mut engine, 2);

        engine
            .shell_mut()
            .set_focus(window, false)
            .expect("the window is live");
        engine.frame().expect("a frame");
        assert!(engine.is_paused(), "a blurred window is paused");

        let layout = engine.menu_layout().expect("the pause menu is showing");
        let over = |point: Vec2| {
            layout
                .items()
                .iter()
                .find(|item| {
                    point.x >= item.min.x
                        && point.x <= item.max.x
                        && point.y >= item.min.y
                        && point.y <= item.max.y
                })
                .map(|item| item.id)
        };

        let corner = Vec2::splat(INSET);
        assert_eq!(
            over(corner),
            None,
            "the inset the browser gate clicks to restore focus is on a button, \
             so that gate can no longer tell focus from a menu press",
        );
        let middle = layout.screen() * 0.5;
        assert_eq!(
            over(middle),
            Some(layout.items()[0].id),
            "the framebuffer's centre is no longer over RESUME — the comments in \
             web/tools/browser-e2e.mjs explain that gate's failure with this fact \
             and need rewriting if it stops being true",
        );

        // Focus comes back the way a browser gives it back: a press and release
        // at a real position, which here is over nothing.
        engine
            .shell_mut()
            .set_focus(window, true)
            .expect("the window is live");
        let at = PhysicalPoint::new(f64::from(corner.x), f64::from(corner.y));
        for state in [PointerState::Pressed, PointerState::Released] {
            engine
                .shell_mut()
                .button(window, PointerButton::Left, state, Some(at))
                .expect("the window is live");
            engine.frame().expect("a frame");
        }
        assert!(
            engine.is_paused(),
            "a click that landed on no button resumed the game",
        );
        engine.finish(ExitReason::FrameBudget).expect("teardown");
    }

    /// **The ship does not keep turning after the window loses focus**, which is
    /// the failure this game has and flappy did not: a flap is an edge, but a
    /// turn is *held*, so a release that never arrives is a ship spinning for the
    /// rest of the session.
    ///
    /// Measured through the real shell and the real event pump — a lost release
    /// is a property of the pump's bookkeeping, and `lose_focus` called directly
    /// would pass with the pump's `held` tracking deleted.
    #[test]
    fn a_window_that_loses_focus_stops_the_ship_turning() {
        let mut engine = scripted(&headless(400));
        let window = engine.window();
        engine
            .shell_mut()
            .key_press(window, KeyCode::Space)
            .expect("the window is live");
        run_frames(&mut engine, 4);
        assert_eq!(engine.game().game().state, GameState::Playing);

        engine
            .shell_mut()
            .key_press(window, KeyCode::ArrowLeft)
            .expect("the window is live");
        run_frames(&mut engine, 10);
        assert!(
            engine.held_keys().contains(&KeyCode::ArrowLeft),
            "the pump did not record the held key, so the test below is vacuous",
        );
        let turning = engine.game().game().ship_heading;

        // Focus goes away, and no release for ArrowLeft ever arrives — which is
        // exactly what every platform does.
        engine
            .shell_mut()
            .set_focus(window, false)
            .expect("the window is live");
        engine.frame().expect("a frame");
        assert!(engine.is_paused(), "focus loss must pause");
        assert!(engine.held_keys().is_empty(), "the held list survived");

        // Un-pause without ever releasing the key, and the ship must be still.
        engine
            .shell_mut()
            .key_press(window, PAUSE_KEY)
            .expect("the window is live");
        run_frames(&mut engine, 30);
        assert!(!engine.is_paused());
        assert!(
            (engine.game().game().ship_heading - turning).abs() < 1e-9,
            "the ship kept turning after focus was lost: {turning} → {}",
            engine.game().game().ship_heading,
        );
        engine.finish(ExitReason::FrameBudget).expect("teardown");
    }
}
