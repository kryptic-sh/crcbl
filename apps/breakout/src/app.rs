//! The breakout engine loop: window, orthographic camera, render graph.
//!
//! Shape matches `apps/sandbox/src/app.rs` — same clock, same event loop, same
//! fixed-timestep accumulator — but the camera is locked to orthographic and
//! the tick body is the game rather than a no-op.
//!
//! # The loop
//!
//! ```text
//! loop {
//!     shell.pump(&mut |event| …);
//!     clock.update(time.elapsed());
//!     while clock.consume_tick() { game.tick(); }
//!     render(clock.alpha());
//! }
//! ```
//!
//! **The simulation is in the `while`, not after it.** Anything stepped once
//! per frame instead has a speed proportional to the frame rate, which is a bug
//! a headless run — where a frame is pinned to exactly 1/60 s — cannot see.

use crcbl::engine::{
    Clock, ConfigureError, ExitReason, Flow, FrameOutcome, GpuError, MAX_CONSECUTIVE_RECONFIGURES,
    Pending, WINDOWED_IDLE, accept_close, wait_for_configure,
};
use crcbl::prelude::*;
use crcbl::shell::{LogicalSize, ShellBackend as Backend, WindowId, open, open_backend};

use crate::game::{self, Game, GameState, RenderState};
use crate::gpu::Gpu;

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
    pub state: GameState,
}

// ---- errors -----------------------------------------------------------------

#[derive(Debug)]
pub enum BreakoutError {
    NoWindowSystem(ShellError),
    Shell(ShellError),
    Configure(ConfigureError),
    NeverPresented,
    Gpu(GpuError),
    Game(game::GameError),
}

impl std::fmt::Display for BreakoutError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::NoWindowSystem(error) => write!(
                f,
                "no window system: {error}\n\
                 hint: `--headless` runs the same loop with no window \
                 and works everywhere."
            ),
            Self::Shell(error) => write!(f, "shell error: {error}"),
            Self::Configure(error) => write!(f, "{error}"),
            Self::NeverPresented => write!(
                f,
                "the swapchain reconfigured {MAX_CONSECUTIVE_RECONFIGURES} times \
                 in a row without presenting a frame"
            ),
            Self::Gpu(error) => write!(f, "gpu error: {error}"),
            Self::Game(error) => write!(f, "game error: {error}"),
        }
    }
}

impl std::error::Error for BreakoutError {}

impl From<ShellError> for BreakoutError {
    fn from(error: ShellError) -> Self {
        Self::Shell(error)
    }
}

impl From<GpuError> for BreakoutError {
    fn from(error: GpuError) -> Self {
        Self::Gpu(error)
    }
}

impl From<game::GameError> for BreakoutError {
    fn from(error: game::GameError) -> Self {
        Self::Game(error)
    }
}

impl From<ConfigureError> for BreakoutError {
    fn from(error: ConfigureError) -> Self {
        Self::Configure(error)
    }
}

// ---- the loop ---------------------------------------------------------------

#[derive(Debug)]
pub struct Loop<S: Shell + ?Sized = dyn Shell> {
    shell: Box<S>,
    window: WindowId,
    gpu: Gpu,
    game: Game,
    clock_source: Clock,
    frame_clock: FrameClock,
    /// Reused every frame, so a steady-state frame does not allocate a fresh
    /// draw list or brick vector.
    draw_list: crcbl::ui::draw_list::DrawList,
    render_state: RenderState,
    hud: HudStrings,
    frames: u64,
    ticks: u64,
    events: u64,
    budget: Option<u64>,
    windowed: bool,
    reconfigures_in_a_row: u32,
}

/// Runs the full loop.
///
/// # Errors
///
/// [`BreakoutError`] if the shell, the GPU or the game failed. Teardown runs on
/// every path: a failing frame must still release the swapchain, the surface
/// and the window, or `crcbl-vk`'s device teardown logs objects still alive.
pub fn run(options: &Options) -> Result<Summary, BreakoutError> {
    let mut engine = Loop::start(options)?;
    let outcome = loop {
        match engine.frame() {
            Ok(Flow::Continue) => {}
            Ok(Flow::Stop(reason)) => break Ok(reason),
            Err(error) => break Err(error),
        }
    };
    match outcome {
        Ok(reason) => engine.finish(reason),
        Err(error) => {
            // The frame error is the one worth reporting; a teardown failure on
            // top of it is logged rather than allowed to replace it.
            if let Err(teardown) = engine.finish(ExitReason::Failed) {
                log::error!("teardown after a failed frame also failed: {teardown}");
            }
            Err(error)
        }
    }
}

impl Loop<dyn Shell> {
    /// Opens a shell, a window, a GPU and the game.
    ///
    /// # Errors
    ///
    /// [`BreakoutError`] if any of them refused.
    pub fn start(options: &Options) -> Result<Self, BreakoutError> {
        let shell = if options.headless {
            open_backend(Backend::Headless).map_err(BreakoutError::Shell)?
        } else {
            open().map_err(BreakoutError::NoWindowSystem)?
        };
        Self::with_shell(shell, options)
    }
}

impl<S: Shell + ?Sized> Loop<S> {
    /// Builds the loop on an already-open shell.
    ///
    /// # Errors
    ///
    /// [`BreakoutError`] if the window never configured, the GPU would not
    /// open, or the game could not be built.
    pub fn with_shell(mut shell: Box<S>, options: &Options) -> Result<Self, BreakoutError> {
        let clock_source = Clock::new(options.headless);
        log::info!(
            "shell: {} backend, caps {:?}",
            shell.backend(),
            shell.caps()
        );
        shell.align_event_clock(clock_source.elapsed());

        let window = shell.create_window(&WindowDesc {
            title: "Breakout",
            app_id: "sh.kryptic.crcbl.breakout",
            size: LogicalSize::new(960.0, 720.0),
            ..WindowDesc::default()
        })?;

        let mut events = 0;
        let extent = wait_for_configure(shell.as_mut(), window, &mut events)?;
        log::info!("shell: first configure at {}x{}", extent.0, extent.1);

        // Locked to orthographic: breakout is a pure 2D game.
        let gpu = Gpu::open(shell.as_ref(), window, extent, options.backend)?;
        let game = Game::new(options.headless, options.tick_hz)?;

        Ok(Self {
            windowed: !options.headless,
            shell,
            window,
            gpu,
            game,
            clock_source,
            frame_clock: FrameClock::new(options.tick_hz),
            draw_list: crcbl::ui::draw_list::DrawList::new(),
            render_state: RenderState::default(),
            hud: HudStrings::default(),
            frames: 0,
            ticks: 0,
            events,
            budget: options.frame_budget(),
            reconfigures_in_a_row: 0,
        })
    }

    /// The game, for scripted tests and for an embedder that wants to drive it.
    #[cfg(test)]
    pub fn game_mut(&mut self) -> &mut Game {
        &mut self.game
    }

    /// One frame: pump, tick the simulation to catch up with the clock, draw.
    ///
    /// # Errors
    ///
    /// [`BreakoutError`] if the shell or the GPU failed.
    pub fn frame(&mut self) -> Result<Flow, BreakoutError> {
        if self.budget.is_some_and(|budget| self.frames >= budget) {
            return Ok(Flow::Stop(ExitReason::FrameBudget));
        }

        if self.windowed {
            self.shell.wait_events(Some(WINDOWED_IDLE));
        }

        let mut pending = Pending::default();
        let game = &mut self.game;
        self.shell.pump(&mut |event| {
            pending.observe(&event);
            // Forward key events to the game, which replays them at the start
            // of the next tick. A frame that runs no ticks loses nothing.
            if let ShellEvent::Key {
                key_code: Some(code),
                state,
                ..
            } = event
            {
                game.key_event(code, matches!(state, crcbl::shell::ButtonState::Pressed));
            }
        });
        self.events += pending.count;

        if pending.destroyed {
            return Ok(Flow::Stop(ExitReason::WindowDestroyed));
        }
        if pending.close_requested {
            accept_close(self.shell.as_mut(), self.window)?;
            return Ok(Flow::Stop(ExitReason::CloseRequested));
        }
        if let Some(size) = pending.resized {
            self.gpu.resize((size.width, size.height))?;
        }

        let now = self.clock_source.advance();
        self.frame_clock.update(now);
        while self.frame_clock.consume_tick() {
            self.ticks += 1;
            self.game.tick();
        }

        self.game.render_state(&mut self.render_state);
        self.gpu.set_paddle_x(self.game.paddle_x());

        self.draw_list.clear();
        self.hud.refresh(&self.render_state);
        draw_field(
            &mut self.draw_list,
            &self.render_state,
            self.gpu.extent(),
            &self.hud,
        );
        self.gpu.take_draw_list(&mut self.draw_list);

        match self.gpu.frame()? {
            FrameOutcome::Presented => {
                self.frames += 1;
                self.reconfigures_in_a_row = 0;
            }
            FrameOutcome::Reconfigured => {
                self.reconfigures_in_a_row += 1;
                if self.reconfigures_in_a_row >= MAX_CONSECUTIVE_RECONFIGURES {
                    return Err(BreakoutError::NeverPresented);
                }
            }
        }
        Ok(Flow::Continue)
    }

    /// Tears the frame down and reports what the run did.
    ///
    /// # Errors
    ///
    /// [`BreakoutError`] if the GPU or the shell failed to release something.
    /// Both are attempted regardless: the window is destroyed even when the GPU
    /// teardown failed, because leaving it mapped is strictly worse.
    pub fn finish(mut self, exit: ExitReason) -> Result<Summary, BreakoutError> {
        if let Some(timings) = self.gpu.timings()
            && !timings.is_empty()
        {
            log::info!("{}", timings.report().trim_end());
        }
        let summary = Summary {
            backend: self.shell.backend(),
            frames: self.frames,
            ticks: self.ticks,
            events: self.events,
            extent: self.gpu.extent(),
            exit,
            score: self.game.score,
            state: self.game.state,
        };

        let gpu_result = self.gpu.destroy();
        let shell_result = if exit.window_survives() {
            self.shell.destroy_window(self.window)
        } else {
            Ok(())
        };
        gpu_result?;
        shell_result?;
        Ok(summary)
    }
}

// ---- drawing ----------------------------------------------------------------

/// The HUD's three lines, rebuilt only when the numbers behind them change.
///
/// `DrawList::text` needs an owned `String`, so the alternative is three
/// `format!`s every frame at whatever rate the window runs — which is what the
/// sandbox's "a steady-state frame allocates nothing" property rules out.
#[derive(Debug, Default)]
struct HudStrings {
    score: String,
    lives: String,
    state: String,
    last: Option<(u32, u32, u32, Option<GameState>)>,
}

impl HudStrings {
    fn refresh(&mut self, render: &RenderState) {
        let key = (render.score, render.high_score, render.lives, render.state);
        if self.last == Some(key) {
            return;
        }
        self.last = Some(key);

        use std::fmt::Write as _;
        self.score.clear();
        let _ = write!(
            self.score,
            "Score: {}  High: {}",
            render.score, render.high_score
        );
        self.lives.clear();
        let _ = write!(self.lives, "Lives: {}", render.lives);
        self.state.clear();
        self.state.push_str(match render.state {
            Some(GameState::WaitingForLaunch) | None => "Press SPACE to launch",
            Some(GameState::Playing) => "Playing",
            Some(GameState::Won) => "YOU WIN! Press SPACE",
            Some(GameState::Lost) => "GAME OVER - Press SPACE",
        });
    }
}

/// Maps the orthographic world onto the surface, in pixels.
///
/// The camera is `Projection::Orthographic { half_height: 9.0 }`, so the
/// visible world is `±9 * aspect` across and `±9` tall. Deriving the mapping
/// from the same numbers the camera uses is what makes the quads land where the
/// forward pass puts the paddle cube.
#[derive(Clone, Copy, Debug)]
struct WorldToScreen {
    half_width: f32,
    half_height: f32,
    width: f32,
    height: f32,
}

impl WorldToScreen {
    fn new(extent: (u32, u32)) -> Self {
        let width = extent.0.max(1) as f32;
        let height = extent.1.max(1) as f32;
        let half_height = crate::gpu::CAMERA_HALF_HEIGHT;
        Self {
            half_width: half_height * (width / height),
            half_height,
            width,
            height,
        }
    }

    fn point(self, x: f64, y: f64) -> glam::Vec2 {
        glam::Vec2::new(
            (x as f32 / self.half_width * 0.5 + 0.5) * self.width,
            (0.5 - y as f32 / self.half_height * 0.5) * self.height,
        )
    }

    /// A world-space axis-aligned box as a screen-space `(min, max)` pair.
    fn quad(self, cx: f64, cy: f64, half_w: f64, half_h: f64) -> (glam::Vec2, glam::Vec2) {
        let a = self.point(cx - half_w, cy + half_h);
        let b = self.point(cx + half_w, cy - half_h);
        (a, b)
    }
}

/// Draws the whole board — every live brick, the ball, the paddle — and the HUD
/// on top of it.
///
/// # Why the board is quads and not meshes
///
/// `crcbl-render`'s [`crcbl::render::ForwardRenderer`] draws
/// **one** instance: `begin_frame` takes a single `model: Mat4` which it writes
/// into a per-frame uniform buffer, and `add_passes` records exactly
/// `draw_indexed(0..index_count, 0, 0..1)`. There is no instance buffer, no
/// per-draw push constant and no second `model` slot, so a caller cannot submit
/// a ball and forty bricks through it — the seam genuinely cannot express more
/// than one transform. The paddle keeps the forward pass's lit cube; everything
/// else goes through the UI pass, which is the app's only multi-quad seam and
/// composites over the tonemapped target in the same graph.
fn draw_field(
    dl: &mut crcbl::ui::draw_list::DrawList,
    render: &RenderState,
    extent: (u32, u32),
    hud: &HudStrings,
) {
    use crate::game::{
        BALL_RADIUS, BRICK_HEIGHT, BRICK_WIDTH, PADDLE_HALF_HEIGHT, PADDLE_HALF_WIDTH, PADDLE_Y,
    };
    use glam::Vec2;

    let map = WorldToScreen::new(extent);

    // Bricks, coloured by row so the grid reads as a grid.
    for brick in &render.bricks {
        let (min, max) = map.quad(brick.x, brick.y, BRICK_WIDTH / 2.0, BRICK_HEIGHT / 2.0);
        let row = ((7.0 - brick.y) / 1.0).round().clamp(0.0, 3.0) as usize;
        let color = [
            [0.90, 0.30, 0.30, 1.0],
            [0.90, 0.60, 0.25, 1.0],
            [0.85, 0.85, 0.30, 1.0],
            [0.35, 0.80, 0.45, 1.0],
        ][row];
        dl.rect(min, max, color);
    }

    // The ball.
    let (min, max) = map.quad(render.ball.x, render.ball.y, BALL_RADIUS, BALL_RADIUS);
    dl.rect(min, max, [1.0, 1.0, 1.0, 1.0]);

    // The paddle, outlined rather than filled: the forward pass already draws
    // it as a lit cube, and the outline is what shows the collider agrees with
    // the mesh.
    let (min, max) = map.quad(
        render.paddle_x,
        PADDLE_Y,
        PADDLE_HALF_WIDTH,
        PADDLE_HALF_HEIGHT,
    );
    dl.rect_outline(min, max, 2.0, [0.4, 0.8, 1.0, 1.0]);

    // HUD panel.
    dl.rect(
        Vec2::new(4.0, 4.0),
        Vec2::new(380.0, 68.0),
        [0.1, 0.1, 0.15, 0.85],
    );
    dl.text(
        Vec2::new(10.0, 10.0),
        hud.score.as_str(),
        [1.0, 1.0, 0.3, 1.0],
        16.0,
    );
    dl.text(
        Vec2::new(10.0, 30.0),
        hud.lives.as_str(),
        [0.3, 1.0, 0.3, 1.0],
        16.0,
    );
    dl.text(
        Vec2::new(10.0, 50.0),
        hud.state.as_str(),
        [0.7, 0.7, 1.0, 1.0],
        14.0,
    );
}

// ---- tests ------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use crcbl::core::input::KeyCode;
    use crcbl::shell::HeadlessShell;

    fn headless(frames: u64) -> Options {
        Options {
            headless: true,
            backend: Some(GpuBackend::Null),
            frames: Some(frames),
            tick_hz: 60,
        }
    }

    #[test]
    fn a_headless_run_is_deterministic() {
        let first = run(&headless(30)).expect("headless runs everywhere");
        let second = run(&headless(30)).expect("headless runs everywhere");
        assert_eq!(first, second, "two identical runs must agree exactly");
        assert_eq!(first.backend, Backend::Headless);
        assert_eq!(first.frames, 30);
        assert_eq!(first.exit, ExitReason::FrameBudget);
    }

    #[test]
    fn ticks_are_paced_by_the_clock_not_the_frame_rate() {
        let sixty = run(&headless(62)).expect("headless runs everywhere");
        let thirty = run(&Options {
            tick_hz: 30,
            ..headless(62)
        })
        .expect("headless runs everywhere");
        assert_eq!(sixty.frames, thirty.frames);
        // 62 frames, first update baseline: 61 ticks at 60 Hz.
        assert_eq!(sixty.ticks, 61);
        assert_eq!(thirty.ticks, 30, "half the rate, half the ticks");
    }

    /// A headless breakout run starts in WaitingForLaunch and produces the
    /// expected frame/event count.  Proves the full init path: window,
    /// GPU (null), ECS world, physics, server/client, audio (null), and
    /// handshake.
    #[test]
    fn breakout_starts_in_waiting_state() {
        let mut engine = scripted(&headless(5));
        for _ in 0..5 {
            engine.frame().expect("a frame");
        }
        let summary = engine.finish(ExitReason::FrameBudget).expect("teardown");
        assert_eq!(summary.frames, 5);
        assert_eq!(summary.ticks, 4); // first update establishes baseline
        assert!(summary.events >= 1, "at least a configure event");
        assert_eq!(summary.state, GameState::WaitingForLaunch);
        assert_eq!(summary.score, 0);
    }

    /// Driving the real loop with a launch produces a run that actually scores,
    /// which is what makes the loop's tick placement observable end to end.
    #[test]
    fn the_loop_plays_the_game() {
        let mut engine = scripted(&headless(600));
        engine.game_mut().key_event(KeyCode::Space, true);
        engine.game_mut().key_event(KeyCode::Space, false);
        while let Ok(Flow::Continue) = engine.frame() {}
        let summary = engine.finish(ExitReason::FrameBudget).expect("teardown");
        assert!(summary.score > 0, "the loop never broke a brick");
        assert!(summary.ticks > 0);
    }

    /// The draw list carries the whole board, not just the paddle.
    ///
    /// Finding 4: only `paddle_model(paddle_x)` was ever submitted, so the ball
    /// and the forty bricks existed solely in a log line.
    #[test]
    fn the_frame_draws_every_live_brick_and_the_ball() {
        use crcbl::ui::draw_list::{DrawCommand, DrawList};

        let mut engine = scripted(&headless(4));
        engine.frame().expect("a frame");

        let mut render = RenderState::default();
        engine.game.render_state(&mut render);
        assert_eq!(render.bricks.len(), crate::game::BRICK_COUNT);

        let mut hud = HudStrings::default();
        hud.refresh(&render);
        let mut dl = DrawList::new();
        draw_field(&mut dl, &render, engine.gpu.extent(), &hud);

        // 40 bricks + the ball + the HUD panel.
        let rects = dl
            .commands()
            .iter()
            .filter(|c| matches!(c, DrawCommand::Rect { .. }))
            .count();
        assert_eq!(rects, crate::game::BRICK_COUNT + 2, "{rects} rects");
        assert_eq!(
            dl.commands()
                .iter()
                .filter(|c| matches!(c, DrawCommand::RectOutline { .. }))
                .count(),
            1,
            "the paddle outline is missing",
        );
        assert_eq!(
            dl.commands()
                .iter()
                .filter(|c| matches!(c, DrawCommand::Text { .. }))
                .count(),
            3,
        );
        engine.finish(ExitReason::FrameBudget).expect("teardown");
    }

    /// Helper: build a Loop<HeadlessShell> for scripting.
    fn scripted(options: &Options) -> Loop<HeadlessShell> {
        Loop::with_shell(Box::new(HeadlessShell::new()), options).expect("headless always starts")
    }
}
