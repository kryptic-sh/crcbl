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

use core::time::Duration;

use crcbl::core::input::KeyCode;
use crcbl::engine::{
    Clock, ConfigureError, ExitReason, Flow, FrameOutcome, GpuError, MAX_CONSECUTIVE_RECONFIGURES,
    Pending, WINDOWED_IDLE, accept_close, wait_for_configure,
};
use crcbl::prelude::*;
use crcbl::shell::{LogicalSize, ShellBackend as Backend, WindowId, open, open_backend};
use crcbl::ui::DebugOverlay;

use crate::game::{self, Game, GameState, RenderState};
use crate::gpu::{Gpu, PendingGpu};

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

/// The key that shows and hides the debug overlay.
///
/// F3 because that is what a player already tries. Every sample uses the same
/// one for the same reason `--debug-overlay` is spelled the same way in each:
/// "switching it on is one thing" is only true if it is the *same* thing.
pub const DEBUG_OVERLAY_KEY: KeyCode = KeyCode::F3;

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
    /// The modular debug panel: frame timing always, GPU pass timings when the
    /// device has timestamp queries, and nothing else — breakout runs over
    /// `InMemoryTransport`, so it has no network module to add.
    debug: DebugOverlay,
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
        let window = open_the_window(shell.as_mut(), &clock_source)?;

        let mut events = 0;
        let extent = wait_for_configure(shell.as_mut(), window, &mut events)?;
        log::info!("shell: first configure at {}x{}", extent.0, extent.1);

        // Locked to orthographic: breakout is a pure 2D game.
        let gpu = Gpu::open(shell.as_ref(), window, extent, options.backend)?;
        Self::assemble(shell, window, gpu, options, clock_source, events)
    }

    /// The half of start-up that is the same however the GPU arrived.
    ///
    /// Shared with [`PendingLoop::poll`], which reaches this point several rAF
    /// frames later than [`with_shell`](Self::with_shell) does. A second copy
    /// of this struct literal is how the browser build would come to run a
    /// subtly different game from the native one.
    fn assemble(
        shell: Box<S>,
        window: WindowId,
        gpu: Gpu,
        options: &Options,
        clock_source: Clock,
        events: u64,
    ) -> Result<Self, BreakoutError> {
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
            debug: DebugOverlay::with_visible(options.debug_overlay_visible()),
            frames: 0,
            ticks: 0,
            events,
            budget: options.frame_budget(),
            reconfigures_in_a_row: 0,
        })
    }

    /// Sets how far one [`frame`](Self::frame) advances a manual clock.
    ///
    /// **The browser's clock is the browser's.** `Clock::Real` reads
    /// [`std::time::Instant`], which on `wasm32-unknown-unknown` has no
    /// implementation at all and panics on the first `now()`. The web entry
    /// point therefore builds the loop on `Clock::manual` and calls this once
    /// per `requestAnimationFrame` with the delta the browser reported, so the
    /// simulation is paced by real time without the engine ever reading a clock
    /// the platform does not have. A real clock ignores this — a loop that
    /// *can* read the time must not be steered by its caller.
    ///
    /// `dt` is clamped to [`MAX_FRAME_STEP`]: a backgrounded tab resumes with a
    /// multi-second gap, and feeding that to the fixed-timestep accumulator
    /// spends the next frame running thousands of ticks.
    pub fn set_frame_step(&mut self, dt: Duration) {
        if let Clock::Manual { step, .. } = &mut self.clock_source {
            *step = dt.min(MAX_FRAME_STEP);
        }
    }

    /// The game, for scripted tests and for an embedder that wants to drive it.
    #[cfg(test)]
    pub fn game_mut(&mut self) -> &mut Game {
        &mut self.game
    }

    /// The shell, at whatever type this loop was built with — so a test can
    /// inject the key events a compositor would deliver.
    #[cfg(test)]
    fn shell_mut(&mut self) -> &mut S {
        self.shell.as_mut()
    }

    /// The window this loop is driving.
    #[cfg(test)]
    const fn window(&self) -> WindowId {
        self.window
    }

    /// The swapchain's current extent, in pixels.
    #[must_use]
    pub const fn extent(&self) -> (u32, u32) {
        self.gpu.extent()
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
        let mut toggle_debug = false;
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
                let pressed = matches!(state, crcbl::shell::ButtonState::Pressed);
                // The overlay is not simulation, so its key never reaches the
                // game: a toggle recorded into the tick's input would change
                // what a scripted run replays.
                if code == DEBUG_OVERLAY_KEY {
                    toggle_debug |= pressed;
                    return;
                }
                game.key_event(code, pressed);
            }
        });
        self.events += pending.count;
        if toggle_debug {
            self.debug.toggle();
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
        // The frame interval the clock just measured, recorded whether or not
        // the panel is visible — a window that only fills while you are looking
        // at it shows two seconds of nothing every time you press F3.
        self.debug.record(self.frame_clock.render_dt());
        while self.frame_clock.consume_tick() {
            self.ticks += 1;
            self.game.tick();
        }

        self.game.render_state(&mut self.render_state);
        self.gpu.set_board(&self.render_state);

        self.draw_list.clear();
        self.hud.refresh(&self.render_state);
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
                    return Err(BreakoutError::NeverPresented);
                }
            }
        }
        Ok(Flow::Continue)
    }

    /// Gathers this frame's debug sections and draws the panel.
    ///
    /// **This is the whole of "switching it on".** Frame timing comes with the
    /// overlay; the only sample-specific line is the one that offers the GPU
    /// timings, and it is a `Some` check because a device without timestamp
    /// queries has no timers at all. Breakout adds nothing else — it runs over
    /// `InMemoryTransport`, so there is no network module, and the panel renders
    /// exactly the same way without one.
    fn draw_debug_overlay(&mut self) {
        self.debug.begin_frame();
        if let Some(timings) = self.gpu.timings() {
            self.debug.panel.add(timings);
        }
        let (width, height) = self.gpu.extent();
        self.debug.render(
            &mut self.draw_list,
            glam::Vec2::new(width as f32, height as f32),
            self.gpu.atlas(),
        );
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

// ---- polled start-up --------------------------------------------------------

/// The largest step [`Loop::set_frame_step`] will accept.
///
/// A tab that was backgrounded for a minute reports a one-minute
/// `requestAnimationFrame` delta on the frame it comes back. Handing that to a
/// fixed-timestep accumulator asks for 3600 ticks in one frame, which is a
/// freeze the user reads as a crash. Four frames' worth of catch-up is the
/// budget; anything beyond it is time the simulation simply did not experience.
pub const MAX_FRAME_STEP: Duration = Duration::from_millis(64);

/// Creates the one window, and puts the shell's event clock on the engine's.
///
/// The two lines both paths share before they diverge over how they wait.
fn open_the_window<S: Shell + ?Sized>(
    shell: &mut S,
    clock_source: &Clock,
) -> Result<WindowId, BreakoutError> {
    log::info!(
        "shell: {} backend, caps {:?}",
        shell.backend(),
        shell.caps()
    );
    shell.align_event_clock(clock_source.elapsed());
    Ok(shell.create_window(&WindowDesc {
        title: "Breakout",
        app_id: "sh.kryptic.crcbl.breakout",
        size: LogicalSize::new(960.0, 720.0),
        ..WindowDesc::default()
    })?)
}

/// How far [`PendingLoop`] has got.
#[derive(Debug)]
enum BootStage {
    /// The window has no size yet. On every platform this is the compositor's
    /// answer to `create_window`; in a browser it is the shim's first
    /// `__crcbl_web_resize`, from initial layout.
    Configure,
    /// A device has been requested and has not arrived.
    Device { pending: PendingGpu },
    /// The loop has been handed over, or a step failed.
    Done,
}

/// A [`Loop`] being started one poll at a time.
///
/// [`Loop::with_shell`] blocks twice — once in
/// [`wait_for_configure`](crcbl::engine::wait_for_configure) and once inside
/// `Gpu::open` — and a browser main thread may do neither: both of the things
/// being waited for are resolved by the very event loop the wait would be
/// sitting inside. This is the same start-up with the waits turned inside out,
/// so the outer loop can be `requestAnimationFrame`.
///
/// It is deliberately **not** the native path's implementation. `with_shell`
/// keeps its blocking waits because on a native platform they are the honest
/// shape and their timeouts are real diagnostics; what the two share is
/// `Loop::assemble`, which is everything after the waiting.
#[derive(Debug)]
pub struct PendingLoop<S: Shell + ?Sized = dyn Shell> {
    shell: Option<Box<S>>,
    window: WindowId,
    options: Options,
    clock_source: Option<Clock>,
    stage: BootStage,
    /// The most recent size the shell reported, which is not necessarily the
    /// one the swapchain was requested at: the canvas can be resized while the
    /// device request is still in flight.
    extent: Option<(u32, u32)>,
    events: u64,
}

impl<S: Shell + ?Sized> PendingLoop<S> {
    /// Creates the window and starts the wait, without blocking on either half.
    ///
    /// `clock_source` is the caller's because the browser's cannot be
    /// [`Clock::new`]'s — see [`Loop::set_frame_step`].
    ///
    /// # Errors
    ///
    /// [`BreakoutError`] if the shell refused the window.
    pub fn request(
        mut shell: Box<S>,
        options: &Options,
        clock_source: Clock,
    ) -> Result<Self, BreakoutError> {
        let window = open_the_window(shell.as_mut(), &clock_source)?;
        Ok(Self {
            shell: Some(shell),
            window,
            options: options.clone(),
            clock_source: Some(clock_source),
            stage: BootStage::Configure,
            extent: None,
            events: 0,
        })
    }

    /// Advances start-up. `Ok(None)` means "not yet, poll again next frame".
    ///
    /// Events are pumped on every poll, including while the device request is
    /// outstanding — a queue nobody drains is how a resize during start-up
    /// becomes a swapchain at the wrong size, and how the shim's own diagnostic
    /// ("the canvas never gets input") goes unexplained.
    ///
    /// # Errors
    ///
    /// [`BreakoutError`] if the window went away before it had a size, if the
    /// device request failed, or if the game could not be built. Polling after
    /// the loop was handed over is a caller bug and reports the same
    /// [`GpuError::Unusable`] the engine does.
    pub fn poll(&mut self) -> Result<Option<Loop<S>>, BreakoutError> {
        let Some(shell) = self.shell.as_mut() else {
            return Err(BreakoutError::Gpu(GpuError::Unusable(
                "this breakout loop was already started",
            )));
        };

        let mut pending = Pending::default();
        shell.pump(&mut |event| pending.observe(&event));
        self.events += pending.count;
        if pending.destroyed {
            return Err(BreakoutError::Shell(ShellError::invalid_window(
                self.window,
            )));
        }
        if let Some(size) = pending.resized {
            self.extent = Some((size.width, size.height));
        }

        match core::mem::replace(&mut self.stage, BootStage::Done) {
            BootStage::Configure => {
                let Some(extent) = self.extent else {
                    self.stage = BootStage::Configure;
                    return Ok(None);
                };
                log::info!("shell: first configure at {}x{}", extent.0, extent.1);
                // Left `Done` if this fails, so a failed start-up stays failed
                // rather than requesting a second device next frame.
                self.stage = BootStage::Device {
                    pending: Gpu::request_open(
                        shell.as_ref(),
                        self.window,
                        extent,
                        self.options.backend,
                    )?,
                };
                Ok(None)
            }
            BootStage::Device { mut pending } => {
                let Some(mut gpu) = pending.poll()? else {
                    self.stage = BootStage::Device { pending };
                    return Ok(None);
                };
                // The canvas may have been resized while the promise was in
                // flight; the swapchain was requested at the older size.
                if let Some(extent) = self.extent
                    && extent != gpu.extent()
                {
                    gpu.resize(extent)?;
                }
                let shell = self.shell.take().expect("checked at the top");
                let clock = self.clock_source.take().expect("taken with the shell");
                Loop::assemble(shell, self.window, gpu, &self.options, clock, self.events).map(Some)
            }
            BootStage::Done => Err(BreakoutError::Gpu(GpuError::Unusable(
                "this breakout loop was already started",
            ))),
        }
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

/// Draws the HUD, and nothing else.
///
/// # The board used to be in here
///
/// Until the sprite pass existed it had to be: `crcbl-render`'s
/// [`crcbl::render::ForwardRenderer`] draws **one** instance — `begin_frame`
/// takes a single `model: Mat4` which it writes into a per-frame uniform buffer,
/// and `add_passes` records exactly `draw_indexed(0..index_count, 0, 0..1)`. The
/// paddle was that instance, and the ball and the forty bricks went through the
/// UI pass as screen-space quads, re-triangulated on the CPU every frame.
/// Flappy hit the same wall with its pipes, independently, which is what made it
/// a finding rather than a quirk.
///
/// It is closed. The court, the grid, the paddle and the ball are sprites in
/// world coordinates now, and with them went `WorldToScreen`, the world→pixel
/// mapping this function used to build: there is one mapping, the camera's, and
/// nothing left here that could disagree with it. The HUD is measured in pixels
/// because a HUD is, which is what the UI pass has always been for.
fn draw_hud(dl: &mut crcbl::ui::draw_list::DrawList, hud: &HudStrings) {
    use glam::Vec2;

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
            debug_overlay: None,
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

    /// **The board reaches the frame, and the HUD is all the draw list has.**
    ///
    /// This replaces `the_frame_draws_every_live_brick_and_the_ball`, which
    /// counted the bricks as UI rectangles — the check that closed finding 4,
    /// when only `paddle_model(paddle_x)` was ever submitted and the ball and
    /// the forty bricks existed solely in a log line. They are sprites now, so
    /// counting rectangles would count the HUD panel forever and never notice
    /// the grid had stopped being drawn; what is checked instead is that the
    /// bricks reach [`Gpu::set_board`] and that nothing but the HUD is left in
    /// the draw list.
    #[test]
    fn the_frame_hands_the_board_to_the_sprite_pass_and_the_hud_to_the_ui_pass() {
        use crcbl::ui::draw_list::{DrawCommand, DrawList};

        let mut engine = scripted(&headless(4));
        engine.frame().expect("a frame");

        let mut render = RenderState::default();
        engine.game.render_state(&mut render);
        assert_eq!(render.bricks.len(), crate::game::BRICK_COUNT);
        assert_eq!(
            engine.gpu.bricks(),
            render.bricks.as_slice(),
            "the grid never reached the renderer"
        );

        let mut hud = HudStrings::default();
        hud.refresh(&render);
        let mut dl = DrawList::new();
        draw_hud(&mut dl, &hud);
        assert_eq!(
            dl.commands()
                .iter()
                .filter(|c| matches!(c, DrawCommand::Rect { .. }))
                .count(),
            1,
            "only the HUD panel is a UI rectangle now",
        );
        assert!(
            !dl.commands()
                .iter()
                .any(|c| matches!(c, DrawCommand::RectOutline { .. })),
            "the paddle is art now, not an outline",
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

    /// Every `Text` command the frame handed to the UI pass.
    fn ui_text(engine: &Loop<HeadlessShell>) -> Vec<String> {
        use crcbl::ui::draw_list::DrawCommand;
        engine
            .gpu
            .draw_list()
            .commands()
            .iter()
            .filter_map(|command| match command {
                DrawCommand::Text { text, .. } => Some(text.clone()),
                _ => None,
            })
            .collect()
    }

    /// **Switching the panel on is one thing, and it works through the real
    /// loop.** F3 arrives as an ordinary shell key event and the very next
    /// frame's draw list gains the frame section; F3 again and it is gone. The
    /// game's HUD is untouched either way.
    #[test]
    fn f3_toggles_the_debug_overlay_in_the_frames_draw_list() {
        let mut engine = scripted(&Options {
            debug_overlay: Some(false),
            ..headless(16)
        });
        let window = engine.window();

        // Two frames so the frame clock has a non-zero interval to report.
        engine.frame().expect("a frame");
        engine.frame().expect("a frame");
        let hidden = ui_text(&engine);
        assert!(
            hidden.iter().any(|t| t.starts_with("Score:")),
            "the game HUD is always drawn: {hidden:?}",
        );
        assert!(
            !hidden.iter().any(|t| t == "frame"),
            "the overlay starts hidden here: {hidden:?}",
        );

        engine
            .shell_mut()
            .key_press(window, DEBUG_OVERLAY_KEY)
            .expect("the window is live");
        engine.frame().expect("a frame");
        let shown = ui_text(&engine);
        assert!(
            shown.iter().any(|t| t == "frame") && shown.iter().any(|t| t == "fps"),
            "F3 must show the frame section: {shown:?}",
        );
        assert!(
            shown.iter().any(|t| t.starts_with("Score:")),
            "the game HUD survives the overlay: {shown:?}",
        );
        engine
            .shell_mut()
            .key_press(window, DEBUG_OVERLAY_KEY)
            .expect("the window is live");
        engine.frame().expect("a frame");
        let hidden_again = ui_text(&engine);
        assert!(
            !hidden_again.iter().any(|t| t == "frame"),
            "F3 again must hide it: {hidden_again:?}",
        );
        engine.finish(ExitReason::FrameBudget).expect("teardown");
    }

    /// **The panel renders with no network module.** Breakout is one half of the
    /// modularity check: it runs over `InMemoryTransport`, so the sections it
    /// has are the frame's, plus the GPU's when the device has timestamp
    /// queries. Nothing else, and no configuration decided that — the panel got
    /// what the sample's systems offered.
    #[test]
    fn the_overlay_is_composed_of_exactly_the_modules_breakout_has() {
        let mut engine = scripted(&Options {
            debug_overlay: Some(true),
            ..headless(8)
        });
        engine.frame().expect("a frame");
        engine.frame().expect("a frame");

        let titles: Vec<&str> = engine
            .debug
            .panel
            .sections()
            .iter()
            .map(crcbl::ui::DebugSection::title)
            .collect();
        let expected: &[&str] = if engine.gpu.timings().is_some() {
            &["frame", "gpu"]
        } else {
            &["frame"]
        };
        assert_eq!(titles, expected, "no module appears that no system offered");

        let drawn = ui_text(&engine);
        for row in ["frame", "fps", "avg", "worst", "window"] {
            assert!(drawn.iter().any(|t| t == row), "missing {row}: {drawn:?}");
        }

        // **The numbers come from the clock, not from nowhere.** A frame that
        // never fed the window would draw the same labels with 0.00 ms beside
        // them, which is the failure a "the rows are present" assertion misses.
        // The first frame's interval is the clock's zero-length sentinel and is
        // dropped, so two frames leave exactly one sample: the headless step.
        assert_eq!(engine.debug.frame.len(), 1, "one real interval so far");
        assert_eq!(
            engine.debug.frame.mean(),
            crcbl::engine::HEADLESS_FRAME_STEP,
            "the window holds the clock's own step",
        );
        assert_eq!(row_value(&drawn, "avg"), "16.67 ms");
        assert_eq!(row_value(&drawn, "window"), "1/120");
        assert_eq!(row_value(&drawn, "fps"), "60.0");
        engine.finish(ExitReason::FrameBudget).expect("teardown");
    }

    /// Helper: build a Loop<HeadlessShell> for scripting.
    fn scripted(options: &Options) -> Loop<HeadlessShell> {
        Loop::with_shell(Box::new(HeadlessShell::new()), options).expect("headless always starts")
    }

    /// Drives [`PendingLoop`] to completion on the headless shell.
    ///
    /// The browser has no test harness, so the polled start-up would otherwise
    /// be code that only CI's `cargo check --target wasm32-unknown-unknown`
    /// ever looks at — compiled, never run. The headless shell configures its
    /// window and the null backend answers its device request, which is exactly
    /// the two waits the browser turns into promises, so the state machine runs
    /// here end to end.
    fn poll_to_completion(options: &Options, clock: Clock) -> (Loop<HeadlessShell>, u32) {
        let mut pending = PendingLoop::request(Box::new(HeadlessShell::new()), options, clock)
            .expect("headless always creates a window");
        let mut polls = 0;
        loop {
            polls += 1;
            assert!(polls < 64, "headless + null must not poll forever");
            if let Some(engine) = pending.poll().expect("nothing here can fail") {
                return (engine, polls);
            }
        }
    }

    /// The polled start-up reaches the same loop the blocking one does.
    ///
    /// Same frame and tick counts from the same options: if the two ever
    /// diverge, the browser build is running a different game from the one CI
    /// tests, and nothing else in this file would notice.
    #[test]
    fn the_polled_start_up_reaches_the_same_loop_as_the_blocking_one() {
        let options = headless(30);
        let (mut polled, polls) = poll_to_completion(&options, Clock::new(true));
        assert!(
            polls >= 2,
            "the first poll cannot both learn the size and have a device, got {polls}",
        );
        while let Ok(Flow::Continue) = polled.frame() {}
        let polled = polled.finish(ExitReason::FrameBudget).expect("teardown");

        let mut blocking = scripted(&options);
        while let Ok(Flow::Continue) = blocking.frame() {}
        let blocking = blocking.finish(ExitReason::FrameBudget).expect("teardown");

        assert_eq!(polled.frames, blocking.frames);
        assert_eq!(polled.ticks, blocking.ticks);
        assert_eq!(polled.extent, blocking.extent);
    }

    /// A polled loop asks for the device only once it knows the canvas size.
    ///
    /// The browser's configure handshake: a `<canvas>` has no size until the
    /// document lays it out, and a swapchain cannot be created without one. A
    /// shell that never reports a size must leave start-up parked rather than
    /// request a swapchain at a guessed extent.
    #[test]
    fn start_up_parks_until_the_window_reports_a_size() {
        let options = headless(1);
        let mut pending =
            PendingLoop::request(Box::new(HeadlessShell::new()), &options, Clock::new(true))
                .expect("headless always creates a window");
        assert!(pending.extent.is_none(), "no size before the first pump");
        assert!(matches!(pending.stage, BootStage::Configure));

        // `HeadlessShell` delays the first configure by a pump or two, exactly
        // as a compositor does. Every poll before it arrives must stay parked
        // in `Configure` rather than guess an extent.
        let mut polls = 0;
        while matches!(pending.stage, BootStage::Configure) {
            polls += 1;
            assert!(polls < 64, "the configure never arrived");
            assert!(
                pending.poll().expect("no failure").is_none(),
                "a loop cannot exist before the window has a size",
            );
            assert_eq!(
                pending.extent.is_some(),
                matches!(pending.stage, BootStage::Device { .. }),
                "the device is requested on exactly the poll that learns the size",
            );
        }
    }

    /// The browser's clock drives the simulation, and a stalled tab does not
    /// stampede it.
    ///
    /// [`Loop::set_frame_step`] is the whole of the wasm timing story:
    /// `Instant::now` panics on that target, so the rAF delta is the only clock
    /// there is. Half the step must produce half the ticks, and a delta from a
    /// tab that was backgrounded for a minute must be clamped rather than
    /// spending the next frame catching up.
    #[test]
    fn the_frame_step_paces_the_simulation_and_is_clamped() {
        let options = Options {
            headless: true,
            backend: Some(GpuBackend::Null),
            frames: Some(60),
            tick_hz: 60,
            debug_overlay: None,
        };

        let mut fast = poll_to_completion(&options, Clock::manual(Duration::ZERO)).0;
        for _ in 0..60 {
            fast.set_frame_step(Duration::from_micros(16_667));
            let _ = fast.frame();
        }
        let fast = fast.finish(ExitReason::FrameBudget).expect("teardown");

        let mut slow = poll_to_completion(&options, Clock::manual(Duration::ZERO)).0;
        for _ in 0..60 {
            slow.set_frame_step(Duration::from_micros(8_333));
            let _ = slow.frame();
        }
        let slow = slow.finish(ExitReason::FrameBudget).expect("teardown");

        assert_eq!(
            fast.frames, slow.frames,
            "the frame count is the frame count"
        );
        assert!(
            fast.ticks >= slow.ticks * 2 - 2 && fast.ticks <= slow.ticks * 2 + 2,
            "half the step must be about half the ticks: {} vs {}",
            fast.ticks,
            slow.ticks,
        );

        // A minute-long delta from a backgrounded tab is one clamped frame, not
        // 3600 ticks in one go.
        let mut resumed = poll_to_completion(&options, Clock::manual(Duration::ZERO)).0;
        resumed.set_frame_step(Duration::from_secs(60));
        let before = resumed.ticks;
        let _ = resumed.frame();
        assert!(
            resumed.ticks - before <= 4,
            "a resumed tab ran {} ticks in one frame",
            resumed.ticks - before,
        );
        resumed.finish(ExitReason::FrameBudget).expect("teardown");
    }

    /// A real clock ignores the caller: a loop that can read the time must not
    /// be steered by whoever calls it.
    #[test]
    fn a_real_clock_is_not_steerable() {
        let (mut engine, _) = poll_to_completion(&headless(1), Clock::new(false));
        engine.set_frame_step(Duration::from_secs(1));
        assert!(matches!(engine.clock_source, Clock::Real(_)));
        engine.finish(ExitReason::FrameBudget).expect("teardown");
    }
}
