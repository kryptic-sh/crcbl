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
    /// Whether the simulation was stopped when the run ended.
    ///
    /// Beside `state` rather than inside it: pause is the loop declining to
    /// advance the simulation, not a state the simulation is in. See
    /// [`Loop::is_paused`].
    pub paused: bool,
    /// The mode the window system actually had the window in, **not** the one
    /// the run last asked for. A summary that reported the request would say
    /// "borderless" for every compositor that refused.
    pub mode: DisplayMode,
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

/// The key that shows and hides the debug overlay.
///
/// F3, the same key breakout uses: "switching it on is one thing" is only true
/// if it is the *same* thing in every sample.
pub const DEBUG_OVERLAY_KEY: KeyCode = KeyCode::F3;

/// The key that pauses and resumes.
///
/// Escape, the same key breakout uses, and free in both: flappy's action map
/// declares Space, Up and R.
///
/// **In a browser it is also the key that leaves fullscreen**, which the
/// browser reserves and no page can decline — so a fullscreen demo's Escape
/// both drops out of fullscreen and pauses.
pub const PAUSE_KEY: KeyCode = KeyCode::Escape;

/// The key that asks for fullscreen, and asks to leave it.
///
/// F11, the desktop convention, and the key `web/engine/shell.js` binds on its
/// side: a browser grants fullscreen only from inside a user-gesture handler,
/// so the shim makes the call and this loop records the request and reads back
/// what happened.
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
    /// draw list or pipe vector.
    draw_list: crcbl::ui::draw_list::DrawList,
    render_state: RenderState,
    hud: HudStrings,
    /// The modular debug panel: frame timing always, GPU pass timings when the
    /// device has timestamp queries, and nothing else — flappy runs over
    /// `InMemoryTransport`, so it has no network module to add.
    debug: DebugOverlay,
    /// Whether the simulation is stopped.
    ///
    /// **The loop owns this, not [`GameState`].** `GameState` lives inside
    /// `GameLogic`, which the authoritative server's module mutates from inside
    /// a tick and which the client replicates; a `Paused` variant there would
    /// make the server's state depend on which window a player's compositor has
    /// focused, and would put a value in `Summary::state` that a seeded,
    /// scripted run could reach. Pause is not something the simulation does —
    /// it is the loop declining to advance it.
    paused: bool,
    /// Keys forwarded to the game as pressed and not yet released.
    ///
    /// [`ShellEvent::Focus`] documents the obligation this discharges: no
    /// platform delivers releases for keys held when focus leaves, so a
    /// consumer that keeps its own key state must clear it. A `Vec` because a
    /// hand holds three keys, not three hundred.
    held_keys: Vec<KeyCode>,
    /// Whether the window system was last seen honouring the display mode this
    /// loop asked for, so a refusal is logged when it happens rather than every
    /// frame afterwards.
    mode_honoured: bool,
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
            debug: DebugOverlay::with_visible(options.debug_overlay_visible()),
            paused: false,
            held_keys: Vec::new(),
            mode_honoured: true,
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
    /// [`FlappyError`] if the shell or the GPU failed.
    pub fn frame(&mut self) -> Result<Flow, FlappyError> {
        if self.budget.is_some_and(|budget| self.frames >= budget) {
            return Ok(Flow::Stop(ExitReason::FrameBudget));
        }

        if self.windowed {
            self.shell.wait_events(Some(WINDOWED_IDLE));
        }

        let mut pending = Pending::default();
        // The three keys the loop keeps for itself, and the one event that is
        // not a key at all.
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
                    // recorded into the tick's input would change what a
                    // seeded, scripted run replays. `!repeat` because holding
                    // F11 down would otherwise toggle the mode at the
                    // keyboard's repeat rate.
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
        // Escape resolves as "paused, then the player unpaused" rather than the
        // reverse.
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
        self.check_mode_request();

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
        // **A paused frame keeps the clock and throws the ticks away.** The
        // three candidates differ only after a long pause, and only one of them
        // resumes without a lurch:
        //
        // * *Stop calling `update`.* `FrameClock::update` measures `now -
        //   last_update`, so the first update after the pause covers the whole
        //   of it. `DEFAULT_MAX_CATCH_UP_TICKS` caps that at 8 ticks and
        //   discards the rest, so resuming spends one frame running 133 ms of
        //   simulation and the bird teleports through a pipe.
        // * *Update but do not drain.* The accumulator saturates at the same
        //   cap, so resuming runs the same 8 ticks in one frame. No better.
        // * *Update and drain.* The accumulator holds only the sub-tick
        //   remainder when the game resumes, so the first live frame runs the
        //   one tick it is owed. This one.
        //
        // Draining also keeps `render_dt` real while paused, which is what the
        // debug overlay above is recording.
        let mut ticks_this_frame = 0;
        if self.paused {
            while self.frame_clock.consume_tick() {}
        } else {
            while self.frame_clock.consume_tick() {
                self.ticks += 1;
                ticks_this_frame += 1;
                self.game.tick();
            }
        }

        self.game.render_state(&mut self.render_state);
        self.gpu.set_world(&self.render_state);
        // The bird's flap is on the simulation's clock, not the frame's — see
        // `crate::art::Scene::advance`. A frame that ran no ticks advances it by
        // nothing, which is what makes a paused game's bird hold still.
        self.gpu.advance_animation(ticks_this_frame);

        self.draw_list.clear();
        self.hud.refresh(&self.render_state, self.paused);
        draw_hud(&mut self.draw_list, &self.hud);
        if self.paused {
            draw_pause_menu(&mut self.draw_list, self.gpu.extent());
        }
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
                    return Err(FlappyError::NeverPresented);
                }
            }
        }
        Ok(Flow::Continue)
    }

    /// Whether the simulation is stopped.
    #[must_use]
    pub const fn is_paused(&self) -> bool {
        self.paused
    }

    /// The mode the window system actually has this window in.
    ///
    /// Read back rather than remembered. There is deliberately no
    /// `self.fullscreen` field to disagree with the compositor: a tiling window
    /// manager makes the question moot, a browser page whose shim never calls
    /// `requestFullscreen` never grants it, and both are cases where a
    /// remembered flag would have the sample telling the player it is
    /// fullscreen while it plainly is not.
    ///
    /// Falls back to the request while the window is unconfigured.
    #[must_use]
    pub fn display_mode(&self) -> DisplayMode {
        self.shell
            .window_state(self.window)
            .map_or(DisplayMode::Windowed, |state| {
                state.effective_mode().unwrap_or(state.requested_mode)
            })
    }

    /// Every key the game thinks is held comes up, and the game pauses.
    ///
    /// Both halves, because they answer different problems. The releases are
    /// [`ShellEvent::Focus`]'s documented obligation. The pause is the reported
    /// bug: an unfocused window still gets frames on every desktop, so the bird
    /// flies into a pipe while nobody is looking and the status still reads
    /// "Playing".
    ///
    /// Regaining focus deliberately does **not** resume.
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
    ///
    /// The target comes from [`display_mode`](Self::display_mode) — the mode in
    /// effect — rather than from what was last requested, so a fullscreen the
    /// player left by some other route (Escape in a browser, a compositor
    /// keybinding) leaves F11 meaning "go fullscreen" again.
    fn toggle_fullscreen(&mut self) -> Result<(), FlappyError> {
        let target = if self.display_mode().is_borderless() {
            DisplayMode::Windowed
        } else {
            DisplayMode::Borderless { monitor: None }
        };
        self.shell.set_mode(self.window, target)?;
        log::info!("shell: asked for {target}");
        Ok(())
    }

    /// Logs the moment the window system stops agreeing with the request.
    ///
    /// Once per transition, not once per frame: a backend that cannot do
    /// fullscreen at all would otherwise print a line every frame forever.
    fn check_mode_request(&mut self) {
        let Ok(state) = self.shell.window_state(self.window) else {
            return;
        };
        if !state.is_configured() {
            return;
        }
        let honoured = state.mode_request_honoured();
        if honoured == self.mode_honoured {
            return;
        }
        self.mode_honoured = honoured;
        if honoured {
            log::info!("shell: the window is {}", state.requested_mode);
        } else {
            log::warn!(
                "shell: asked for {} and got {}",
                state.requested_mode,
                self.display_mode(),
            );
        }
    }

    /// Gathers this frame's debug sections and draws the panel.
    ///
    /// **This is the whole of "switching it on".** Frame timing comes with the
    /// overlay; the only sample-specific line is the one that offers the GPU
    /// timings, and it is a `Some` check because a device without timestamp
    /// queries has no timers at all. Flappy adds nothing else — it runs over
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
    last: Option<HudKey>,
}

/// Everything the HUD's text depends on. A named tuple rather than an inline
/// one because with `paused` in it there are five fields, which is past the
/// point where the positions read.
type HudKey = (u32, u32, Option<GameState>, Option<game::Death>, bool);

impl HudStrings {
    /// **`paused` wins over the simulation's state, and that is the bug fix.**
    /// The status line used to read straight off `RenderState::state`, which is
    /// the *server's* idea of what is happening — and the server is still
    /// playing while the window sits behind a browser. A player alt-tabbed away
    /// saw "Playing" and came back dead.
    fn refresh(&mut self, render: &RenderState, paused: bool) {
        let key = (
            render.score,
            render.best,
            render.state,
            render.death,
            paused,
        );
        if self.last == Some(key) {
            return;
        }
        self.last = Some(key);

        use std::fmt::Write as _;
        self.score.clear();
        let _ = write!(self.score, "Score: {}  Best: {}", render.score, render.best);
        self.state.clear();
        self.state.push_str(if paused {
            "PAUSED - press ESC"
        } else {
            match (render.state, render.death) {
                (Some(GameState::WaitingToStart) | None, _) => "Press SPACE to fly",
                (Some(GameState::Playing), _) => "Playing",
                (Some(GameState::Dead), Some(game::Death::Ground)) => {
                    "You hit the ground - press SPACE"
                }
                (Some(GameState::Dead), _) => "You hit a pipe - press SPACE",
            }
        });
    }
}

/// The pause menu.
///
/// **This is the seam the art slice replaces, and it is deliberately one
/// function taking a draw list and an extent.** Everything in it goes through
/// the same `DrawList` rect-and-text calls the HUD uses, because that is what
/// exists today; nothing above it knows how a paused frame is drawn, so
/// swapping this body for a nine-slice panel and real buttons touches this
/// function and nothing else. The state machine — what pauses, what resumes,
/// what stops ticking — is settled and does not move with the art.
fn draw_pause_menu(dl: &mut crcbl::ui::draw_list::DrawList, extent: (u32, u32)) {
    use glam::Vec2;

    let (width, height) = (extent.0 as f32, extent.1 as f32);
    dl.rect(Vec2::ZERO, Vec2::new(width, height), [0.0, 0.0, 0.0, 0.6]);

    let panel = Vec2::new(360.0, 132.0);
    let origin = Vec2::new((width - panel.x) / 2.0, (height - panel.y) / 2.0);
    dl.rect(origin, panel, [0.08, 0.08, 0.12, 0.92]);
    dl.text(
        origin + Vec2::new(24.0, 22.0),
        "PAUSED",
        [1.0, 1.0, 0.3, 1.0],
        32.0,
    );
    for (row, line) in PAUSE_MENU_LINES.iter().enumerate() {
        dl.text(
            origin + Vec2::new(24.0, 68.0 + row as f32 * 20.0),
            *line,
            [0.8, 0.8, 0.9, 1.0],
            14.0,
        );
    }
}

/// What the pause menu offers. Text today, buttons after the art slice.
const PAUSE_MENU_LINES: [&str; 3] = ["ESC   resume", "F11   fullscreen", "F3    debug overlay"];

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

    /// **The panel renders with no network module.** Flappy is the other half of
    /// the modularity check: it runs over `InMemoryTransport`, so the sections it
    /// has are the frame's, plus the GPU's when the device has timestamp
    /// queries. Nothing else, and no configuration decided that.
    #[test]
    fn the_overlay_is_composed_of_exactly_the_modules_flappy_has() {
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
        hud.refresh(&render, false);
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

    /// **A flap through the real loop restarts the wing beat.**
    ///
    /// `art::Scene::observe` is tested against a velocity directly; this is the
    /// wiring, which nothing else would catch — `Gpu::set_world` is the only
    /// caller, and a `set_world` that dropped the velocity on the floor would
    /// leave every test in `art.rs` green and the bird's wing free-running
    /// again. Measured as the clip's own clock going backwards while the
    /// simulation's goes forwards.
    #[test]
    fn a_flap_through_the_loop_restarts_the_wing_beat() {
        let mut engine = Loop::start(&headless(40)).expect("headless runs everywhere");

        // Let the clip run on. The bird is parked and not flapping, so the only
        // thing moving is the animation.
        for _ in 0..8 {
            engine.frame().expect("a frame");
        }
        let before = engine.gpu.animation_ticks();
        assert!(
            before > 0,
            "the clip has not advanced, so a restart would prove nothing"
        );
        assert_eq!(
            engine.gpu.animation_ticks(),
            before,
            "an idle frame flapped"
        );

        engine.game_mut().key_event(KeyCode::Space, true);
        engine.game_mut().key_event(KeyCode::Space, false);
        engine.frame().expect("a frame");
        let after = engine.gpu.animation_ticks();
        assert!(
            after < before,
            "the flap left the wing at {after} ticks, having been at {before} \
             — the beat did not start over"
        );
        assert!(
            engine.ticks > 0 && after <= engine.ticks,
            "the clip cannot be ahead of the simulation"
        );

        // And it stays restarted: the ticks after a flap advance it again
        // rather than restarting it every frame the bird is still climbing.
        engine.frame().expect("a frame");
        assert!(
            engine.gpu.animation_ticks() > after,
            "the beat restarts every frame while the bird climbs"
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
    // ---- focus, pause and fullscreen ----------------------------------------

    /// Runs `frames` frames, and insists every one of them was a real frame.
    ///
    /// The `assert_eq!` is not decoration: `Loop::frame` answers a spent frame
    /// budget with `Ok(Flow::Stop)` **before** it pumps, so a test that let one
    /// through would go on injecting key events into a loop that had stopped
    /// reading them.
    fn run_frames(engine: &mut Loop<HeadlessShell>, frames: u32) {
        for _ in 0..frames {
            assert_eq!(
                engine.frame().expect("a frame"),
                Flow::Continue,
                "the loop stopped early",
            );
        }
    }

    /// **The reported bug, and the obligation `ShellEvent::Focus` documents.**
    ///
    /// Flappy's flap is an *edge*, which makes the missing release worse than a
    /// stuck direction: `ActionMap` raises `just_pressed` only on the
    /// transition, so a Space it never saw released turns every later press
    /// into nothing and the bird can never flap again. The assertion is that
    /// the bird flaps after the round trip, which is the input state and not a
    /// flag.
    #[test]
    fn losing_focus_pauses_the_game_and_lets_go_of_every_held_key() {
        let mut engine = scripted(&headless(400));
        let window = engine.window();

        // Hold Space. The bird starts flying and never sees a release.
        engine
            .shell_mut()
            .key_press(window, KeyCode::Space)
            .expect("the window is live");
        run_frames(&mut engine, 20);
        assert_eq!(engine.game.state, GameState::Playing, "the bird is flying");
        assert!(engine.game.flap_is_down(), "Space is down");
        assert_eq!(engine.held_keys, vec![KeyCode::Space]);

        engine
            .shell_mut()
            .set_focus(window, false)
            .expect("the window is live");
        engine.frame().expect("a frame");
        assert!(engine.is_paused(), "an unfocused window is not playing");
        assert!(engine.held_keys.is_empty());

        // Resume, and let the bird fall for a while so a flap is unmistakable.
        engine
            .shell_mut()
            .key_press(window, PAUSE_KEY)
            .expect("the window is live");
        run_frames(&mut engine, 20);
        assert!(!engine.is_paused());
        assert!(
            !engine.game.flap_is_down(),
            "the action map still holds a key focus loss should have released",
        );
        let mut falling = RenderState::default();
        engine.game.render_state(&mut falling);
        assert!(
            falling.bird_velocity.y < 0.0,
            "the bird has to be falling for a flap to be visible, got {}",
            falling.bird_velocity.y,
        );

        // A fresh press, exactly as a keyboard sends it after the player let go
        // in some other window: a `keydown` with no `keyup` before it.
        engine
            .shell_mut()
            .key_press(window, KeyCode::Space)
            .expect("the window is live");
        run_frames(&mut engine, 2);
        let mut flapped = RenderState::default();
        engine.game.render_state(&mut flapped);
        assert!(
            flapped.bird_velocity.y > 0.0,
            "the bird never flapped again: velocity {} after a fresh Space",
            flapped.bird_velocity.y,
        );
        engine.finish(ExitReason::FrameBudget).expect("teardown");
    }

    /// Regaining focus does not resume. The pause menu is dismissed on purpose.
    #[test]
    fn focus_coming_back_leaves_the_game_paused() {
        let mut engine = scripted(&headless(60));
        let window = engine.window();
        run_frames(&mut engine, 2);

        engine
            .shell_mut()
            .set_focus(window, false)
            .expect("the window is live");
        engine.frame().expect("a frame");
        assert!(engine.is_paused());

        engine
            .shell_mut()
            .set_focus(window, true)
            .expect("the window is live");
        run_frames(&mut engine, 5);
        assert!(
            engine.is_paused(),
            "clicking back in must not drop the player into a live bird",
        );
        engine.finish(ExitReason::FrameBudget).expect("teardown");
    }

    /// **A paused game's world is byte-identical after any number of frames.**
    ///
    /// `RenderState` is the whole course — the bird, its velocity, and every
    /// pipe on the treadmill — which is a far stronger claim than "the state
    /// enum did not change": the pipes scroll every single tick, so one tick
    /// slipping through fails this.
    #[test]
    fn a_paused_game_does_not_advance_its_simulation() {
        let mut engine = scripted(&headless(400));
        let window = engine.window();

        engine
            .shell_mut()
            .key_press(window, KeyCode::Space)
            .expect("the window is live");
        engine
            .shell_mut()
            .key_release(window, KeyCode::Space)
            .expect("the window is live");
        run_frames(&mut engine, 30);
        assert_eq!(engine.game.state, GameState::Playing);

        engine
            .shell_mut()
            .key_press(window, PAUSE_KEY)
            .expect("the window is live");
        engine.frame().expect("a frame");
        assert!(engine.is_paused());

        let ticks = engine.ticks;
        let mut paused = RenderState::default();
        engine.game.render_state(&mut paused);
        run_frames(&mut engine, 120);
        let mut after = RenderState::default();
        engine.game.render_state(&mut after);

        assert_eq!(after, paused, "120 paused frames moved the world");
        assert_eq!(engine.ticks, ticks, "a paused frame ran a tick");
        assert_eq!(
            engine.gpu.animation_ticks(),
            engine.ticks,
            "the wing beat is on the simulation's clock, so a pause holds it too",
        );

        engine
            .shell_mut()
            .key_press(window, PAUSE_KEY)
            .expect("the window is live");
        run_frames(&mut engine, 20);
        let mut resumed = RenderState::default();
        engine.game.render_state(&mut resumed);
        assert_ne!(resumed, after, "resuming did not restart the simulation");
        engine.finish(ExitReason::FrameBudget).expect("teardown");
    }

    /// **A long pause does not lurch on resume.** Five seconds paused is three
    /// hundred ticks of wall-clock time the simulation did not experience, and
    /// a resume that caught any of it up would scroll the course through the
    /// bird in one frame. See the tick loop for the alternatives.
    #[test]
    fn resuming_after_a_long_pause_runs_one_tick_not_a_catch_up_burst() {
        const STEP: Duration = Duration::from_micros(16_667);
        // A budget past the 320 frames this runs: `frame` answers a spent
        // budget before it pumps.
        let options = headless(2_000);
        let (mut engine, _) = poll_to_completion(&options, Clock::manual(Duration::ZERO));
        let window = engine.window();

        let step_frame = |engine: &mut Loop<HeadlessShell>| {
            engine.set_frame_step(STEP);
            assert_eq!(
                engine.frame().expect("a frame"),
                Flow::Continue,
                "the loop stopped early",
            );
        };

        engine
            .shell_mut()
            .key_press(window, KeyCode::Space)
            .expect("the window is live");
        for _ in 0..10 {
            step_frame(&mut engine);
        }

        engine
            .shell_mut()
            .key_press(window, PAUSE_KEY)
            .expect("the window is live");
        step_frame(&mut engine);
        assert!(engine.is_paused());

        let ticks_at_pause = engine.ticks;
        for _ in 0..300 {
            step_frame(&mut engine);
        }
        assert_eq!(engine.ticks, ticks_at_pause, "a paused frame ran a tick");

        engine
            .shell_mut()
            .key_press(window, PAUSE_KEY)
            .expect("the window is live");
        let before = engine.ticks;
        step_frame(&mut engine);
        assert!(!engine.is_paused());
        let burst = engine.ticks - before;
        assert!(
            burst <= 1,
            "the first frame after a five-second pause ran {burst} ticks",
        );
        engine.finish(ExitReason::FrameBudget).expect("teardown");
    }

    /// The HUD says what the loop is doing, not what the server thinks.
    #[test]
    fn the_status_line_reads_paused_while_paused() {
        let mut engine = scripted(&headless(60));
        let window = engine.window();
        engine
            .shell_mut()
            .key_press(window, KeyCode::Space)
            .expect("the window is live");
        run_frames(&mut engine, 10);
        assert!(
            ui_text(&engine).iter().any(|t| t == "Playing"),
            "the flying game reads as playing: {:?}",
            ui_text(&engine),
        );

        engine
            .shell_mut()
            .set_focus(window, false)
            .expect("the window is live");
        engine.frame().expect("a frame");
        let drawn = ui_text(&engine);
        assert!(
            drawn.iter().any(|t| t.starts_with("PAUSED")),
            "an unfocused game still reads as playing: {drawn:?}",
        );
        assert!(
            !drawn.iter().any(|t| t == "Playing"),
            "both statuses at once: {drawn:?}",
        );
        assert!(
            drawn.iter().any(|t| *t == PAUSE_MENU_LINES[0]),
            "the pause menu is not drawn: {drawn:?}",
        );
        engine.finish(ExitReason::FrameBudget).expect("teardown");
    }

    /// **F11 twice is where it started, and the loop reports the mode the
    /// window system gave it rather than the one it asked for.**
    #[test]
    fn fullscreen_toggles_twice_back_to_windowed() {
        let mut engine = scripted(&headless(200));
        let window = engine.window();
        run_frames(&mut engine, 2);
        assert_eq!(engine.display_mode(), DisplayMode::Windowed);
        let windowed_extent = engine.extent();

        engine
            .shell_mut()
            .key_press(window, FULLSCREEN_KEY)
            .expect("the window is live");
        run_frames(&mut engine, 6);
        assert_eq!(
            engine.display_mode(),
            DisplayMode::Borderless { monitor: None },
            "the compositor answered and the loop did not notice",
        );
        assert!(
            engine
                .shell_mut()
                .window_state(window)
                .expect("state")
                .mode_request_honoured()
        );
        assert_ne!(engine.extent(), windowed_extent);

        engine
            .shell_mut()
            .key_press(window, FULLSCREEN_KEY)
            .expect("the window is live");
        run_frames(&mut engine, 6);
        assert_eq!(
            engine.display_mode(),
            DisplayMode::Windowed,
            "F11 twice must land back where it started",
        );
        let summary = engine.finish(ExitReason::FrameBudget).expect("teardown");
        assert_eq!(summary.mode, DisplayMode::Windowed);
    }

    /// **A backend that refuses reports the mode it really has.** The
    /// compositor answers a borderless request with a windowed configure, which
    /// is what a tiling window manager does and what a browser page whose shim
    /// never calls `requestFullscreen` does.
    #[test]
    fn a_refused_fullscreen_is_reported_as_the_mode_the_window_actually_has() {
        let mut engine = scripted(&headless(200));
        let window = engine.window();
        run_frames(&mut engine, 2);
        let windowed = engine.extent();

        engine
            .shell_mut()
            .key_press(window, FULLSCREEN_KEY)
            .expect("the window is live");
        engine.frame().expect("a frame");
        engine
            .shell_mut()
            .resize(
                window,
                crcbl::shell::PhysicalSize::new(windowed.0, windowed.1),
            )
            .expect("the window is live");
        run_frames(&mut engine, 4);

        let state = engine.shell_mut().window_state(window).expect("state");
        assert_eq!(
            state.requested_mode,
            DisplayMode::Borderless { monitor: None },
            "the request is a fact and stands",
        );
        assert!(!state.mode_request_honoured());
        assert_eq!(
            engine.display_mode(),
            DisplayMode::Windowed,
            "the loop must report what it got, not what it asked for",
        );
        assert!(!engine.mode_honoured, "the refusal has to be noticed");

        let summary = engine.finish(ExitReason::FrameBudget).expect("teardown");
        assert_eq!(summary.mode, DisplayMode::Windowed);
    }

    /// Holding F11 down does not strobe the window between modes.
    #[test]
    fn an_auto_repeat_does_not_toggle_anything() {
        let mut engine = scripted(&headless(60));
        let window = engine.window();
        run_frames(&mut engine, 2);

        for _ in 0..8 {
            engine
                .shell_mut()
                .key_repeat(window, FULLSCREEN_KEY)
                .expect("the window is live");
            engine
                .shell_mut()
                .key_repeat(window, PAUSE_KEY)
                .expect("the window is live");
        }
        run_frames(&mut engine, 6);
        assert_eq!(engine.display_mode(), DisplayMode::Windowed);
        assert!(!engine.is_paused());
        engine.finish(ExitReason::FrameBudget).expect("teardown");
    }
}
