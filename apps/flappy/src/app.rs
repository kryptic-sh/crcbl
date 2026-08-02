//! The flappy engine loop: window, scrolling orthographic camera, render graph.
//!
//! Shape matches `apps/breakout/src/app.rs` — same clock, same event loop, same
//! fixed-timestep accumulator. What differs is the camera, which moves, and the
//! draw, which places everything against where the camera is rather than against
//! the origin.
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

use core::time::Duration;

use crcbl::engine::{
    Clock, ConfigureError, ExitReason, Flow, FrameOutcome, GpuError, MAX_CONSECUTIVE_RECONFIGURES,
    Pending, WINDOWED_IDLE, accept_close, wait_for_configure,
};
use crcbl::prelude::*;
use crcbl::shell::{LogicalSize, ShellBackend as Backend, WindowId, open, open_backend};

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
pub enum FlappyError {
    NoWindowSystem(ShellError),
    Shell(ShellError),
    Configure(ConfigureError),
    NeverPresented,
    Gpu(GpuError),
    Game(game::GameError),
}

impl std::fmt::Display for FlappyError {
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

impl std::error::Error for FlappyError {}

impl From<ShellError> for FlappyError {
    fn from(error: ShellError) -> Self {
        Self::Shell(error)
    }
}

impl From<GpuError> for FlappyError {
    fn from(error: GpuError) -> Self {
        Self::Gpu(error)
    }
}

impl From<game::GameError> for FlappyError {
    fn from(error: game::GameError) -> Self {
        Self::Game(error)
    }
}

impl From<ConfigureError> for FlappyError {
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
    /// draw list or pipe vector.
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
/// [`FlappyError`] if the shell, the GPU or the game failed. Teardown runs on
/// every path: a failing frame must still release the swapchain, the surface and
/// the window, or `crcbl-vk`'s device teardown logs objects still alive.
pub fn run(options: &Options) -> Result<Summary, FlappyError> {
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
    /// [`FlappyError`] if any of them refused.
    pub fn start(options: &Options) -> Result<Self, FlappyError> {
        let shell = if options.headless {
            open_backend(Backend::Headless).map_err(FlappyError::Shell)?
        } else {
            open().map_err(FlappyError::NoWindowSystem)?
        };
        Self::with_shell(shell, options)
    }
}

impl<S: Shell + ?Sized> Loop<S> {
    /// Builds the loop on an already-open shell.
    ///
    /// # Errors
    ///
    /// [`FlappyError`] if the window never configured, the GPU would not open,
    /// or the game could not be built.
    pub fn with_shell(mut shell: Box<S>, options: &Options) -> Result<Self, FlappyError> {
        let clock_source = Clock::new(options.headless);
        let window = open_the_window(shell.as_mut(), &clock_source)?;

        let mut events = 0;
        let extent = wait_for_configure(shell.as_mut(), window, &mut events)?;
        log::info!("shell: first configure at {}x{}", extent.0, extent.1);

        let gpu = Gpu::open(shell.as_ref(), window, extent, options.backend)?;
        Self::assemble(shell, window, gpu, options, clock_source, events)
    }

    /// The half of start-up that is the same however the GPU arrived.
    ///
    /// Shared with [`PendingLoop::poll`], which reaches this point several rAF
    /// frames later. A second copy of this struct literal is how the browser
    /// build would come to run a subtly different game from the native one.
    fn assemble(
        shell: Box<S>,
        window: WindowId,
        gpu: Gpu,
        options: &Options,
        clock_source: Clock,
        events: u64,
    ) -> Result<Self, FlappyError> {
        let game = Game::with_seed(options.headless, options.tick_hz, options.seed)?;
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

    /// Sets how far one [`frame`](Self::frame) advances a manual clock.
    ///
    /// **The browser's clock is the browser's.** `Clock::Real` reads
    /// [`std::time::Instant`], which on `wasm32-unknown-unknown` has no
    /// implementation at all and panics on the first `now()`. `dt` is clamped to
    /// [`MAX_FRAME_STEP`]: a backgrounded tab resumes with a multi-second gap,
    /// and feeding that to the accumulator spends the next frame running
    /// thousands of ticks.
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

    /// The swapchain's current extent, in pixels.
    #[must_use]
    pub const fn extent(&self) -> (u32, u32) {
        self.gpu.extent()
    }

    /// One frame: pump, tick the simulation to catch up with the clock, draw.
    ///
    /// # Errors
    ///
    /// [`FlappyError`] if the shell or the GPU failed.
    pub fn frame(&mut self) -> Result<Flow, FlappyError> {
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
        let mut ticks_this_frame = 0;
        while self.frame_clock.consume_tick() {
            self.ticks += 1;
            ticks_this_frame += 1;
            self.game.tick();
        }

        self.game.render_state(&mut self.render_state);
        self.gpu.set_world(&self.render_state);
        // The bird's flap is on the simulation's clock, not the frame's — see
        // `crate::art::Scene::advance`. A frame that ran no ticks advances it by
        // nothing, which is what makes a paused game's bird hold still.
        self.gpu.advance_animation(ticks_this_frame);

        self.draw_list.clear();
        self.hud.refresh(&self.render_state);
        draw_hud(&mut self.draw_list, &self.hud);
        self.gpu.take_draw_list(&mut self.draw_list);

        match self.gpu.frame()? {
            FrameOutcome::Presented => {
                self.frames += 1;
                self.reconfigures_in_a_row = 0;
            }
            FrameOutcome::Reconfigured => {
                self.reconfigures_in_a_row += 1;
                if self.reconfigures_in_a_row >= MAX_CONSECUTIVE_RECONFIGURES {
                    return Err(FlappyError::NeverPresented);
                }
            }
        }
        Ok(Flow::Continue)
    }

    /// Tears the frame down and reports what the run did.
    ///
    /// # Errors
    ///
    /// [`FlappyError`] if the GPU or the shell failed to release something. Both
    /// are attempted regardless: the window is destroyed even when the GPU
    /// teardown failed, because leaving it mapped is strictly worse.
    pub fn finish(mut self, exit: ExitReason) -> Result<Summary, FlappyError> {
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
/// A tab backgrounded for a minute reports a one-minute `requestAnimationFrame`
/// delta on the frame it comes back. Handing that to a fixed-timestep
/// accumulator asks for 3600 ticks in one frame, which the user reads as a
/// crash.
pub const MAX_FRAME_STEP: Duration = Duration::from_millis(64);

/// Creates the one window, and puts the shell's event clock on the engine's.
fn open_the_window<S: Shell + ?Sized>(
    shell: &mut S,
    clock_source: &Clock,
) -> Result<WindowId, FlappyError> {
    log::info!(
        "shell: {} backend, caps {:?}",
        shell.backend(),
        shell.caps()
    );
    shell.align_event_clock(clock_source.elapsed());
    Ok(shell.create_window(&WindowDesc {
        title: "Flappy",
        app_id: "sh.kryptic.crcbl.flappy",
        size: LogicalSize::new(960.0, 720.0),
        ..WindowDesc::default()
    })?)
}

/// How far [`PendingLoop`] has got.
#[derive(Debug)]
enum BootStage {
    /// The window has no size yet.
    Configure,
    /// A device has been requested and has not arrived.
    Device { pending: PendingGpu },
    /// The loop has been handed over, or a step failed.
    Done,
}

/// A [`Loop`] being started one poll at a time.
///
/// [`Loop::with_shell`] blocks twice — once waiting for a configure and once
/// inside `Gpu::open` — and a browser main thread may do neither: both of the
/// things being waited for are resolved by the very event loop the wait would be
/// sitting inside.
#[derive(Debug)]
pub struct PendingLoop<S: Shell + ?Sized = dyn Shell> {
    shell: Option<Box<S>>,
    window: WindowId,
    options: Options,
    clock_source: Option<Clock>,
    stage: BootStage,
    extent: Option<(u32, u32)>,
    events: u64,
}

impl<S: Shell + ?Sized> PendingLoop<S> {
    /// Creates the window and starts the wait, without blocking on either half.
    ///
    /// # Errors
    ///
    /// [`FlappyError`] if the shell refused the window.
    pub fn request(
        mut shell: Box<S>,
        options: &Options,
        clock_source: Clock,
    ) -> Result<Self, FlappyError> {
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
    /// # Errors
    ///
    /// [`FlappyError`] if the window went away before it had a size, if the
    /// device request failed, or if the game could not be built.
    pub fn poll(&mut self) -> Result<Option<Loop<S>>, FlappyError> {
        let Some(shell) = self.shell.as_mut() else {
            return Err(FlappyError::Gpu(GpuError::Unusable(
                "this flappy loop was already started",
            )));
        };

        let mut pending = Pending::default();
        shell.pump(&mut |event| pending.observe(&event));
        self.events += pending.count;
        if pending.destroyed {
            return Err(FlappyError::Shell(ShellError::invalid_window(self.window)));
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
            BootStage::Done => Err(FlappyError::Gpu(GpuError::Unusable(
                "this flappy loop was already started",
            ))),
        }
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
    last: Option<(u32, u32, Option<GameState>, Option<game::Death>)>,
}

impl HudStrings {
    fn refresh(&mut self, render: &RenderState) {
        let key = (render.score, render.best, render.state, render.death);
        if self.last == Some(key) {
            return;
        }
        self.last = Some(key);

        use std::fmt::Write as _;
        self.score.clear();
        let _ = write!(self.score, "Score: {}  Best: {}", render.score, render.best);
        self.state.clear();
        self.state.push_str(match (render.state, render.death) {
            (Some(GameState::WaitingToStart) | None, _) => "Press SPACE to fly",
            (Some(GameState::Playing), _) => "Playing",
            (Some(GameState::Dead), Some(game::Death::Ground)) => {
                "You hit the ground - press SPACE"
            }
            (Some(GameState::Dead), _) => "You hit a pipe - press SPACE",
        });
    }
}

/// Draws the HUD, and nothing else.
///
/// # The world used to be in here
///
/// Until the sprite pass existed it had to be: `crcbl-render`'s
/// [`crcbl::render::ForwardRenderer`] draws **one** instance — `begin_frame`
/// takes a single `model: Mat4` — so the bird was that instance and every pipe
/// went through the UI pass as a screen-space quad, re-triangulated on the CPU
/// every frame. Breakout hit the same wall with its bricks, independently,
/// which is what made it a finding rather than a quirk.
///
/// It is closed. The course and the bird are sprites in world coordinates now,
/// and with them went `WorldToScreen`, the world→pixel mapping this function
/// used to build: there is one mapping, the camera's, and nothing left here
/// that could disagree with it. The HUD is measured in pixels because a HUD is,
/// which is what the UI pass has always been for.
fn draw_hud(dl: &mut crcbl::ui::draw_list::DrawList, hud: &HudStrings) {
    use glam::Vec2;

    // HUD panel.
    dl.rect(
        Vec2::new(4.0, 4.0),
        Vec2::new(340.0, 52.0),
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
    use crcbl::core::input::KeyCode;
    use crcbl::shell::HeadlessShell;

    fn scripted(options: &Options) -> Loop<HeadlessShell> {
        Loop::with_shell(Box::new(HeadlessShell::new()), options).expect("headless always starts")
    }

    /// Drives [`PendingLoop`] to completion on the headless shell.
    ///
    /// The browser has no test harness here, so the polled start-up would
    /// otherwise be code that only `cargo check --target wasm32-unknown-unknown`
    /// ever looks at — compiled, never run. The headless shell configures its
    /// window and the null backend answers its device request, which is exactly
    /// the two waits the browser turns into promises.
    fn poll_to_completion(options: &Options, clock: Clock) -> (Loop<HeadlessShell>, u32) {
        let mut pending = PendingLoop::request(Box::new(HeadlessShell::new()), options, clock)
            .expect("headless always creates a window");
        let mut polls = 0;
        loop {
            polls += 1;
            assert!(polls < 64, "the headless path must not poll forever");
            if let Some(engine) = pending.poll().expect("nothing here can fail") {
                break (engine, polls);
            }
        }
    }

    fn headless(frames: u64) -> Options {
        Options {
            headless: true,
            backend: Some(GpuBackend::Null),
            frames: Some(frames),
            ..Options::default()
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
        // 62 frames, the first update establishing the baseline: 61 ticks at
        // 60 Hz.
        assert_eq!(sixty.ticks, 61);
        assert_eq!(thirty.ticks, 30, "half the rate, half the ticks");

        // The case that needs the accumulator to be a `while` rather than an
        // `if`: a headless frame is pinned to 1/60 s, so at 120 Hz every frame
        // owes the simulation two ticks. A loop that ran one per frame would
        // report 61 here and look right at 60 Hz forever.
        let fast = run(&Options {
            tick_hz: 120,
            ..headless(62)
        })
        .expect("headless runs everywhere");
        assert_eq!(fast.ticks, 122, "a frame owing two ticks must run both");
    }

    /// The whole start-up path — window, GPU, ECS world, physics,
    /// server/client, handshake — reaches a game waiting for its first flap.
    #[test]
    fn a_run_starts_waiting_for_the_player() {
        let summary = run(&headless(5)).expect("headless runs everywhere");
        assert_eq!(summary.frames, 5);
        assert_eq!(summary.ticks, 4);
        assert!(summary.events >= 1, "at least a configure event");
        assert_eq!(summary.state, GameState::WaitingToStart);
        assert_eq!(summary.score, 0);
    }

    /// Driving the real loop with a flap starts a run, which is what makes the
    /// loop's tick placement observable end to end.
    #[test]
    fn the_loop_plays_the_game() {
        let mut engine = Loop::start(&headless(200)).expect("headless runs everywhere");
        engine.game_mut().key_event(KeyCode::Space, true);
        engine.game_mut().key_event(KeyCode::Space, false);
        while let Ok(Flow::Continue) = engine.frame() {}
        let summary = engine.finish(ExitReason::FrameBudget).expect("teardown");
        assert_ne!(
            summary.state,
            GameState::WaitingToStart,
            "the flap never reached the simulation"
        );
        assert!(summary.ticks > 0);
    }

    /// `--seed` reaches the course.
    ///
    /// Without this the flag is a number the parser stores and nothing reads:
    /// every other test here runs on the default seed, and two runs on
    /// different ones would compare equal because a summary carries no pipes.
    #[test]
    fn the_seed_flag_reaches_the_course() {
        let course = |seed: u64| {
            let engine = scripted(&Options {
                seed,
                ..headless(2)
            });
            let pipes = engine.game.pipes();
            engine.finish(ExitReason::FrameBudget).expect("teardown");
            pipes
        };
        let first = course(1);
        assert!(!first.is_empty(), "a run with no course proves nothing");
        assert_eq!(first, course(1), "one seed, one course");
        assert_ne!(first, course(2), "two seeds, two courses");
    }

    /// A key the shell reports reaches the game.
    ///
    /// `the_loop_plays_the_game` hands the game its flap directly, so on its own
    /// it would pass with the pump's key branch deleted — and that branch is the
    /// entire input path of a one-button game.
    #[test]
    fn a_key_the_shell_reports_reaches_the_simulation() {
        let mut engine = scripted(&headless(30));
        let window = engine.window;
        engine
            .shell
            .key_press(window, KeyCode::Space)
            .expect("the headless shell takes a key");

        // Two frames: the first pumps the key and establishes the clock's
        // baseline without running a tick, so the flap it queued is consumed by
        // the second. That is the queueing this loop exists to get right, not a
        // workaround for it.
        engine.frame().expect("a frame");
        engine.frame().expect("a second frame");
        assert_eq!(
            engine.game.state,
            GameState::Playing,
            "the flap never got from the shell to the game"
        );
        engine.finish(ExitReason::FrameBudget).expect("teardown");
    }

    /// The browser's start-up shape, on the headless shell: polling never
    /// blocks, and it produces the same loop the blocking path does.
    #[test]
    fn the_polled_start_up_produces_a_working_loop() {
        let options = headless(5);
        let (mut engine, polls) = poll_to_completion(&options, Clock::new(true));
        assert!(
            polls >= 2,
            "the state machine short-circuited: {polls} polls"
        );
        engine.frame().expect("a frame");
        let summary = engine.finish(ExitReason::FrameBudget).expect("teardown");
        assert_eq!(summary.frames, 1);
        assert_eq!(summary.state, GameState::WaitingToStart);
    }

    /// **The course reaches the frame, and the HUD is all the draw list has.**
    ///
    /// This replaces `the_frame_draws_every_visible_pipe`, which counted the
    /// pipes as UI rectangles. They are sprites now, so counting rectangles
    /// would count the HUD panel forever and never notice the course had
    /// stopped being drawn; what is checked instead is that the pipes reach
    /// [`Gpu::set_world`] and that nothing but the HUD is left in the draw list.
    #[test]
    fn the_frame_hands_the_course_to_the_sprite_pass_and_the_hud_to_the_ui_pass() {
        use crcbl::ui::draw_list::{DrawCommand, DrawList};

        let mut engine = Loop::start(&headless(4)).expect("headless runs everywhere");
        engine.frame().expect("a frame");

        let mut render = RenderState::default();
        engine.game.render_state(&mut render);
        assert!(!render.pipes.is_empty(), "the course is empty");
        assert_eq!(
            engine.gpu.pipes(),
            render.pipes.as_slice(),
            "the course never reached the renderer"
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
        assert_eq!(
            dl.commands()
                .iter()
                .filter(|c| matches!(c, DrawCommand::Text { .. }))
                .count(),
            2,
        );
        assert!(
            !dl.commands()
                .iter()
                .any(|c| matches!(c, DrawCommand::RectOutline { .. })),
            "the bird is art now, not an outline",
        );
        engine.finish(ExitReason::FrameBudget).expect("teardown");
    }

    /// The animation is on the simulation's clock: a frame that ran `n` ticks
    /// advances the flap by exactly `n`.
    ///
    /// **At 120 Hz, not at 60.** A headless frame is pinned to 1/60 s, so at the
    /// default rate every frame owes exactly one tick and a flap advanced once
    /// per *frame* would agree with one advanced once per tick — the same
    /// vacuity `ticks_are_paced_by_the_clock_not_the_frame_rate` exists to
    /// avoid. At 120 Hz each frame owes two, and the two answers differ by a
    /// factor of two.
    #[test]
    fn the_flap_advances_by_the_ticks_the_frame_ran_and_not_by_the_frames() {
        let mut engine = Loop::start(&Options {
            tick_hz: 120,
            ..headless(40)
        })
        .expect("headless runs everywhere");
        let mut frames = 0u64;
        while let Ok(Flow::Continue) = engine.frame() {
            frames += 1;
        }
        let ticks = engine.ticks;
        let elapsed = engine.gpu.animation_ticks();
        assert!(ticks > 0, "a run with no ticks proves nothing");
        assert!(
            ticks > frames,
            "at 120 Hz a 60 fps run owes two ticks a frame; {ticks} ticks over \
             {frames} frames cannot tell the two apart"
        );
        assert_eq!(
            elapsed, ticks,
            "the flap has seen {elapsed} ticks and the simulation has run {ticks}"
        );
        engine.finish(ExitReason::FrameBudget).expect("teardown");
    }
}
