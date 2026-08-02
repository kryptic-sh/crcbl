//! The asteroids engine loop: window, fixed-timestep accumulator, one draw.
//!
//! Shape matches `apps/flappy/src/app.rs` and `apps/breakout/src/app.rs` — same
//! clock, same event loop, same accumulator — and is deliberately the *small*
//! version of it. There are no menus, no pointer handling and no browser entry
//! point in this sub-slice, so what is left is the part every sample shares.
//!
//! # The loop
//!
//! ```text
//! loop {
//!     shell.pump(&mut |event| …);
//!     clock.update(time.elapsed());
//!     while clock.consume_tick() { game.tick(); }
//!     render();
//! }
//! ```
//!
//! **The simulation is in the `while`, not after it.** Anything stepped once per
//! frame has a speed proportional to the frame rate, which a headless run —
//! where a frame is pinned to exactly 1/60 s — cannot see.

use crcbl::core::input::KeyCode;
use crcbl::engine::{
    Clock, ConfigureError, ExitReason, Flow, FrameOutcome, GpuError, MAX_CONSECUTIVE_RECONFIGURES,
    Pending, WINDOWED_IDLE, accept_close, wait_for_configure,
};
use crcbl::prelude::*;
use crcbl::shell::{
    DisplayMode, LogicalSize, ShellBackend as Backend, WindowId, open, open_backend,
};
use crcbl::ui::DebugOverlay;
use crcbl::ui::draw_list::DrawList;
use glam::Vec2;

use crate::game::{self, Game, GameState, RenderState};
use crate::gpu::{Gpu, world_to_screen};

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

#[derive(Debug)]
pub enum AsteroidsError {
    NoWindowSystem(ShellError),
    Shell(ShellError),
    Configure(ConfigureError),
    NeverPresented,
    Gpu(GpuError),
    Game(game::GameError),
}

impl std::fmt::Display for AsteroidsError {
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

impl std::error::Error for AsteroidsError {}

impl From<ShellError> for AsteroidsError {
    fn from(error: ShellError) -> Self {
        Self::Shell(error)
    }
}

impl From<GpuError> for AsteroidsError {
    fn from(error: GpuError) -> Self {
        Self::Gpu(error)
    }
}

impl From<game::GameError> for AsteroidsError {
    fn from(error: game::GameError) -> Self {
        Self::Game(error)
    }
}

impl From<ConfigureError> for AsteroidsError {
    fn from(error: ConfigureError) -> Self {
        Self::Configure(error)
    }
}

// ---- the loop ---------------------------------------------------------------

/// The key that shows and hides the debug overlay.
///
/// F3, the key breakout and flappy use: "switching it on is one thing" is only
/// true if it is the *same* thing in every sample.
pub const DEBUG_OVERLAY_KEY: KeyCode = KeyCode::F3;

/// The key that pauses and resumes. Escape, as in the other two, and free here:
/// `game.rs` declares the arrows, WASD, Space and R.
pub const PAUSE_KEY: KeyCode = KeyCode::Escape;

/// The key that asks for fullscreen, and asks to leave it.
pub const FULLSCREEN_KEY: KeyCode = KeyCode::F11;

#[derive(Debug)]
pub struct Loop<S: Shell + ?Sized = dyn Shell> {
    shell: Box<S>,
    window: WindowId,
    gpu: Gpu,
    game: Game,
    clock_source: Clock,
    frame_clock: FrameClock,
    /// Reused every frame, so a steady-state frame does not allocate a fresh
    /// draw list or entity vector.
    draw_list: DrawList,
    render_state: RenderState,
    hud: HudStrings,
    /// The modular debug panel: frame timing always, GPU pass timings when the
    /// device has them.
    debug: DebugOverlay,
    /// Whether the simulation is stopped. **The loop owns this, not
    /// [`GameState`]**: a `Paused` variant in the simulation would make the
    /// authoritative server's state depend on which window a compositor has
    /// focused.
    paused: bool,
    /// Keys forwarded to the game as pressed and not yet released.
    ///
    /// [`ShellEvent::Focus`] documents the obligation this discharges: no
    /// platform delivers releases for keys held when focus leaves. It matters
    /// more here than in either earlier sample, because this game's turn and
    /// thrust are *held* actions — a lost release leaves the ship spinning.
    held_keys: Vec<KeyCode>,
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
/// [`AsteroidsError`] if the shell, the GPU or the game failed. Teardown runs on
/// every path: a failing frame must still release the swapchain, the surface and
/// the window.
pub fn run(options: &Options) -> Result<Summary, AsteroidsError> {
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
    /// [`AsteroidsError`] if any of them refused.
    pub fn start(options: &Options) -> Result<Self, AsteroidsError> {
        let shell = if options.headless {
            open_backend(Backend::Headless).map_err(AsteroidsError::Shell)?
        } else {
            open().map_err(AsteroidsError::NoWindowSystem)?
        };
        Self::with_shell(shell, options)
    }
}

impl<S: Shell + ?Sized> Loop<S> {
    /// Builds the loop on an already-open shell.
    ///
    /// # Errors
    ///
    /// [`AsteroidsError`] if the window never configured, the GPU would not
    /// open, or the game could not be built.
    pub fn with_shell(mut shell: Box<S>, options: &Options) -> Result<Self, AsteroidsError> {
        let clock_source = Clock::new(options.headless);
        log::info!(
            "shell: {} backend, caps {:?}",
            shell.backend(),
            shell.caps()
        );
        shell.align_event_clock(clock_source.elapsed());
        let window = shell.create_window(&WindowDesc {
            title: "Asteroids",
            app_id: "sh.kryptic.crcbl.asteroids",
            size: LogicalSize::new(960.0, 720.0),
            ..WindowDesc::default()
        })?;

        let mut events = 0;
        let extent = wait_for_configure(shell.as_mut(), window, &mut events)?;
        log::info!("shell: first configure at {}x{}", extent.0, extent.1);

        let gpu = Gpu::open(shell.as_ref(), window, extent, options.backend)?;
        let game = Game::with_seed(options.headless, options.tick_hz, options.seed)?;
        Ok(Self {
            windowed: !options.headless,
            shell,
            window,
            gpu,
            game,
            clock_source,
            frame_clock: FrameClock::new(options.tick_hz),
            draw_list: DrawList::new(),
            render_state: RenderState::default(),
            hud: HudStrings::default(),
            debug: DebugOverlay::with_visible(options.debug_overlay_visible()),
            paused: false,
            held_keys: Vec::new(),
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

    /// The swapchain's current extent, in pixels.
    #[must_use]
    pub const fn extent(&self) -> (u32, u32) {
        self.gpu.extent()
    }

    /// Whether the simulation is stopped.
    #[must_use]
    pub const fn is_paused(&self) -> bool {
        self.paused
    }

    /// One frame: pump, tick the simulation to catch up with the clock, draw.
    ///
    /// # Errors
    ///
    /// [`AsteroidsError`] if the shell or the GPU failed.
    pub fn frame(&mut self) -> Result<Flow, AsteroidsError> {
        if self.budget.is_some_and(|budget| self.frames >= budget) {
            return Ok(Flow::Stop(ExitReason::FrameBudget));
        }

        if self.windowed {
            self.shell.wait_events(Some(WINDOWED_IDLE));
        }

        let mut pending = Pending::default();
        let (mut toggle_debug, mut toggle_pause, mut toggle_fullscreen) = (false, false, false);
        let mut focus_lost = false;
        let game = &mut self.game;
        let held = &mut self.held_keys;
        self.shell.pump(&mut |event| {
            pending.observe(&event);
            match event {
                // Losing focus is not a key event and never will be: the
                // releases for whatever was held are exactly what no platform
                // sends. See `ShellEvent::Focus`.
                ShellEvent::Focus { focused: false, .. } => focus_lost = true,
                ShellEvent::Key {
                    key_code: Some(code),
                    state,
                    repeat,
                    ..
                } => {
                    let pressed = matches!(state, crcbl::shell::ButtonState::Pressed);
                    // The loop's own keys never reach the game: a toggle
                    // recorded into the tick's input would change what a seeded,
                    // scripted run replays.
                    let edge = pressed && !repeat;
                    match code {
                        DEBUG_OVERLAY_KEY => {
                            toggle_debug |= edge;
                            return;
                        }
                        PAUSE_KEY => {
                            toggle_pause |= edge;
                            return;
                        }
                        FULLSCREEN_KEY => {
                            toggle_fullscreen |= edge;
                            return;
                        }
                        _ => {}
                    }
                    if pressed {
                        if !held.contains(&code) {
                            held.push(code);
                        }
                    } else {
                        held.retain(|key| *key != code);
                    }
                    game.key_event(code, pressed);
                }
                _ => {}
            }
        });
        self.events += pending.count;

        if toggle_debug {
            self.debug.toggle();
        }
        // Before the pause toggle, so a batch carrying both a focus loss and an
        // Escape resolves as "paused, then the player unpaused".
        if focus_lost {
            self.lose_focus();
        }
        if toggle_pause {
            self.paused = !self.paused;
            log::info!("game {}", if self.paused { "paused" } else { "resumed" });
        }
        if toggle_fullscreen {
            self.toggle_fullscreen()?;
        }

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
        self.debug.record(self.frame_clock.render_dt());
        // **A paused frame keeps the clock and throws the ticks away**, so
        // resuming runs the one tick it is owed rather than the eight the
        // accumulator saturated at. `apps/flappy/src/app.rs` carries the full
        // argument.
        if self.paused {
            while self.frame_clock.consume_tick() {}
        } else {
            while self.frame_clock.consume_tick() {
                self.ticks += 1;
                self.game.tick();
            }
        }

        self.game.render_state(&mut self.render_state);
        self.draw_list.clear();
        draw_field(&mut self.draw_list, &self.render_state, self.gpu.extent());
        self.hud.refresh(&self.render_state, self.paused);
        draw_hud(&mut self.draw_list, &self.hud);
        self.draw_debug_overlay();
        self.gpu.take_draw_list(&mut self.draw_list);

        match self.gpu.frame()? {
            FrameOutcome::Presented => {
                self.frames += 1;
                self.reconfigures_in_a_row = 0;
            }
            FrameOutcome::Reconfigured => {
                self.reconfigures_in_a_row += 1;
                if self.reconfigures_in_a_row >= MAX_CONSECUTIVE_RECONFIGURES {
                    return Err(AsteroidsError::NeverPresented);
                }
            }
        }
        Ok(Flow::Continue)
    }

    /// The mode the window system actually has this window in.
    ///
    /// Read back rather than remembered: there is deliberately no
    /// `self.fullscreen` field to disagree with the compositor.
    #[must_use]
    pub fn display_mode(&self) -> DisplayMode {
        self.shell
            .window_state(self.window)
            .map_or(DisplayMode::Windowed, |state| {
                state.effective_mode().unwrap_or(state.requested_mode)
            })
    }

    /// Every key the game thinks is held comes up, and the game pauses.
    fn lose_focus(&mut self) {
        for key in self.held_keys.drain(..) {
            self.game.key_event(key, false);
        }
        if !self.paused {
            self.paused = true;
            log::info!("game paused: the window lost focus");
        }
    }

    /// Asks for the mode the window is not in.
    fn toggle_fullscreen(&mut self) -> Result<(), AsteroidsError> {
        let target = if self.display_mode().is_borderless() {
            DisplayMode::Windowed
        } else {
            DisplayMode::Borderless { monitor: None }
        };
        self.shell.set_mode(self.window, target)?;
        log::info!("shell: asked for {target}");
        Ok(())
    }

    /// Gathers this frame's debug sections and draws the panel.
    ///
    /// **This is the whole of "switching it on", and the plan asked for it from
    /// the first slice rather than at the end.** Frame timing comes with the
    /// overlay; the only sample-specific line is the one that offers the GPU
    /// timings, and it is a `Some` check because a device without timestamp
    /// queries has no timers at all.
    fn draw_debug_overlay(&mut self) {
        self.debug.begin_frame();
        if let Some(timings) = self.gpu.timings() {
            self.debug.panel.add(timings);
        }
        let (width, height) = self.gpu.extent();
        self.debug.render(
            &mut self.draw_list,
            Vec2::new(width as f32, height as f32),
            self.gpu.atlas(),
        );
    }

    /// Tears the frame down and reports what the run did.
    ///
    /// # Errors
    ///
    /// [`AsteroidsError`] if the GPU or the shell failed to release something.
    /// Both are attempted regardless: the window is destroyed even when the GPU
    /// teardown failed, because leaving it mapped is strictly worse.
    pub fn finish(mut self, exit: ExitReason) -> Result<Summary, AsteroidsError> {
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
            lives: self.game.lives,
            wave: self.game.wave,
            state: self.game.state,
            paused: self.paused,
            mode: self.display_mode(),
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

/// Draws the playfield as untextured quads.
///
/// **Placeholder, and named as one.** `crate::gpu`'s header sets out why there
/// is no sprite pass in this sub-slice; what this gives is a window that shows
/// the simulation actually running, which is the difference between "the loop
/// compiles" and "the loop works". The art sub-slice replaces this function
/// wholesale.
fn draw_field(dl: &mut DrawList, render: &RenderState, extent: (u32, u32)) {
    let scale = crate::gpu::pixels_per_unit(extent);

    // The border, so the wrap is legible: a rock that vanishes off one side and
    // appears on the other needs an edge to have crossed.
    let top_left = world_to_screen(
        glam::DVec3::new(-game::WORLD_HALF_WIDTH, game::WORLD_HALF_HEIGHT, 0.0),
        extent,
    );
    let bottom_right = world_to_screen(
        glam::DVec3::new(game::WORLD_HALF_WIDTH, -game::WORLD_HALF_HEIGHT, 0.0),
        extent,
    );
    dl.rect_outline(top_left, bottom_right, 2.0, [0.25, 0.25, 0.4, 1.0]);

    for rock in &render.rocks {
        let centre = world_to_screen(rock.position, extent);
        let half = rock.size.radius() as f32 * scale;
        dl.rect_outline(
            centre - Vec2::splat(half),
            centre + Vec2::splat(half),
            2.0,
            [0.75, 0.72, 0.68, 1.0],
        );
    }
    for bullet in &render.bullets {
        let centre = world_to_screen(bullet.position, extent);
        let half = (game::BULLET_RADIUS as f32 * scale).max(2.0);
        dl.rect(
            centre - Vec2::splat(half),
            centre + Vec2::splat(half),
            [1.0, 0.95, 0.6, 1.0],
        );
    }
    if render.ship_alive {
        let centre = world_to_screen(render.ship, extent);
        let half = game::SHIP_RADIUS as f32 * scale;
        dl.rect(
            centre - Vec2::splat(half),
            centre + Vec2::splat(half),
            [0.4, 0.9, 1.0, 1.0],
        );
        // A stub along the heading, so the placeholder at least shows which way
        // the ship is pointing — the one thing a square cannot.
        let nose = world_to_screen(
            render.ship + game::heading_vector(render.ship_heading) * (game::SHIP_RADIUS * 2.2),
            extent,
        );
        dl.rect(
            nose - Vec2::splat(3.0),
            nose + Vec2::splat(3.0),
            [1.0, 1.0, 1.0, 1.0],
        );
    }
}

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

type HudKey = (u32, u32, u32, Option<GameState>, bool);

impl HudStrings {
    /// **`paused` wins over the simulation's state**, which is the bug flappy
    /// fixed: the status line used to read straight off the *server's* idea of
    /// what was happening, and the server is still playing while the window sits
    /// behind a browser.
    fn refresh(&mut self, render: &RenderState, paused: bool) {
        let key = (
            render.score,
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
            "Score: {}  Lives: {}  Wave: {}",
            render.score,
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
    dl.rect(
        Vec2::new(4.0, 4.0),
        Vec2::new(360.0, 52.0),
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
    use super::*;
    use crcbl::shell::ShellBackend;

    fn headless_loop() -> Loop<dyn Shell> {
        let options = Options {
            headless: true,
            frames: Some(8),
            ..Options::default()
        };
        Loop::start(&options).expect("a headless loop always starts")
    }

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
        engine.paused = true;
        let before = engine.ticks;
        for _ in 0..3 {
            engine.frame().expect("a frame");
        }
        assert_eq!(engine.ticks, before, "a paused frame ran a tick");
        assert!(engine.is_paused());
    }

    /// A field with something in it produces something to draw.
    ///
    /// The assertion is a **count against the board**, not "more than zero": a
    /// draw list that lost the rocks and kept the border would pass the weaker
    /// version.
    #[test]
    fn the_draw_list_carries_the_border_the_ship_and_every_rock() {
        let mut engine = headless_loop();
        engine.frame().expect("a frame");
        let render = &engine.render_state;
        assert_eq!(
            render.rocks.len(),
            game::wave_rocks(0) as usize,
            "the first wave should be on the field"
        );

        let mut dl = DrawList::new();
        draw_field(&mut dl, render, (960, 720));
        // One border + one outline per rock + two quads for the ship.
        assert_eq!(dl.len(), 1 + render.rocks.len() + 2);
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

    /// Losing focus releases every key the game thinks is held.
    ///
    /// It matters more here than in either earlier sample: turn and thrust are
    /// *held* actions, so a lost release leaves the ship spinning for the rest
    /// of the session.
    #[test]
    fn losing_focus_releases_the_keys_the_game_still_thinks_are_down() {
        let mut engine = headless_loop();
        engine.frame().expect("a frame");
        engine.game_mut().key_event(KeyCode::ArrowLeft, true);
        engine.held_keys.push(KeyCode::ArrowLeft);
        engine.game_mut().tick();

        engine.lose_focus();
        engine.game_mut().tick();
        assert!(engine.held_keys.is_empty(), "the held list survived");
        assert!(engine.is_paused(), "focus loss must pause");
    }
}
