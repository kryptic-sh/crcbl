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
    Clock, ExitReason, Flow, FrameBudget, Handled, MAX_FRAME_STEP, MenuPump, ModeRequest,
    PointerCapture, WINDOWED_IDLE, accept_close, lose_focus, run_ticks, wait_for_configure,
};
use crcbl::prelude::*;
use crcbl::shell::{
    DisplayMode, LogicalSize, ShellBackend as Backend, WindowId, open, open_backend,
};
use crcbl::ui::DebugOverlay;

use crate::game::{self, Game, GameState, RenderState};
use crate::gpu::Gpu;
use crate::menu::{self, MenuAction, MenuKind, Menus};

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

/// What can stop flappy: the loop's own failures, plus this game's.
///
/// An alias rather than an enum. Every sample had the same five loop
/// variants written out with the same `Display` arms, so they live in
/// [`crcbl::engine::LoopError`] now and this names the game error that
/// goes in the sixth. Its docs say why a game error is wrapped by name —
/// `.map_err(FlappyError::Game)` — while the engine's three convert with `?`.
pub type FlappyError = crcbl::engine::LoopError<game::GameError>;

// ---- the loop ---------------------------------------------------------------

/// The key a menu's `FLY` and `TRY AGAIN` buttons stand for.
///
/// Fired as a real key event rather than by calling into `Game`, because
/// starting and restarting a run is the simulation's business and the simulation
/// is driven by its action map.
const FLAP_KEY: KeyCode = KeyCode::Space;

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
    /// The start, pause and game-over menus, and which is on screen.
    menus: Menus,
    /// Where the pointer was last seen and whether its button is down.
    ///
    /// Both are needed across frames — see [`PointerCapture`].
    pointer: PointerCapture,
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
    /// The fullscreen request, and whether the window system agreed — see
    /// [`ModeRequest`].
    mode: ModeRequest,
    /// Presented frames, the budget they count against, and the guard that
    /// makes the budget reachable — see [`FrameBudget`].
    budget: FrameBudget,
    ticks: u64,
    events: u64,
    windowed: bool,
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
                crcbl::log::error!("teardown after a failed frame also failed: {teardown}");
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
        let shell = if options.common.headless {
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
        let clock_source = Clock::new(options.common.headless);
        let window = open_the_window(shell.as_mut(), &clock_source)?;

        let mut events = 0;
        let extent = wait_for_configure(shell.as_mut(), window, &mut events)?;
        crcbl::log::info!("shell: first configure at {}x{}", extent.0, extent.1);

        let gpu = Gpu::open(shell.as_ref(), window, extent, options.common.backend)?;
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
        let game = Game::with_seed(
            options.common.headless,
            options.common.tick_hz,
            options.seed,
        )
        .map_err(FlappyError::Game)?;
        Ok(Self {
            windowed: !options.common.headless,
            shell,
            window,
            gpu,
            game,
            clock_source,
            frame_clock: FrameClock::new(options.common.tick_hz),
            draw_list: crcbl::ui::draw_list::DrawList::new(),
            render_state: RenderState::default(),
            hud: HudStrings::default(),
            menus: menu::menus(),
            pointer: PointerCapture::new(),
            debug: DebugOverlay::with_visible(options.common.debug_overlay_visible()),
            paused: false,
            held_keys: Vec::new(),
            mode: ModeRequest::new(),
            ticks: 0,
            events,
            budget: FrameBudget::new(options.common.frame_budget()),
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
        if self.budget.is_spent() {
            return Ok(Flow::Stop(ExitReason::FrameBudget));
        }

        if self.windowed {
            self.shell.wait_events(Some(WINDOWED_IDLE));
        }

        // **Carrying the pointer, not defaulting it.** A batch with no pointer
        // event in it has not moved the cursor, and a menu whose hover state
        // reset every still frame would flicker.
        let mut pending = self.pointer.pending();
        let game = &mut self.game;
        // **Last frame's menu, deliberately.** The pump runs before this
        // frame's state is known, and the menu the player is pressing keys at
        // is the one that was on screen when they pressed them.
        let showing = self.menus.kind() != MenuKind::None;
        let mut menu = MenuPump::new(&mut self.menus, &mut self.held_keys, showing);
        self.shell.pump(&mut |event| {
            // The window's business, the pointer, focus loss and the loop's
            // three reserved keys are all folded by `Pending::observe`; the
            // menu's three and the held-key list are `MenuPump`'s. What comes
            // back from that is the key the *game* should see.
            if pending.observe(&event) == Handled::Loop {
                return;
            }
            // Forwarded to the game, which replays it at the start of the next
            // tick. A frame that runs no ticks loses nothing.
            if let Some((code, pressed)) = menu.observe(&event) {
                game.key_event(code, pressed);
            }
        });
        let keyboard_action = menu.activated.and_then(MenuAction::from_id);
        self.events += pending.count;
        // Hit-tested against **this** frame's layout, which is why the pointer
        // is resolved here and not inside the pump: the rectangles depend on the
        // framebuffer's size, and a click checked against last frame's would miss
        // on the frame a resize lands. The press-capture rule under `resolve` is
        // the engine's.
        let pointer_input = self.pointer.resolve(&pending);
        let pointer_action = self
            .menus
            .point(self.gpu.extent(), self.gpu.atlas(), pointer_input)
            .and_then(MenuAction::from_id);
        for action in [keyboard_action, pointer_action].into_iter().flatten() {
            self.apply(action)?;
        }
        if pending.toggle_debug_overlay {
            self.debug.toggle();
        }
        // Before the pause toggle, so a batch carrying both a focus loss and an
        // Escape resolves as "paused, then the player unpaused" rather than the
        // reverse.
        if pending.focus_lost {
            self.lose_focus();
        }
        if pending.toggle_pause {
            self.paused = !self.paused;
            crcbl::log::info!("game {}", if self.paused { "paused" } else { "resumed" });
        }
        if pending.toggle_fullscreen {
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
        let game = &mut self.game;
        let ticks_this_frame = run_ticks(&mut self.frame_clock, self.paused, || game.tick());
        self.ticks += ticks_this_frame;

        self.game.render_state(&mut self.render_state);
        self.gpu.set_world(&self.render_state);
        // The bird's flap is on the simulation's clock, not the frame's — see
        // `crate::art::Scene::advance`. A frame that ran no ticks advances it by
        // nothing, which is what makes a paused game's bird hold still.
        self.gpu.advance_animation(ticks_this_frame);

        self.draw_list.clear();
        self.hud.refresh(&self.render_state, self.paused);
        draw_hud(&mut self.draw_list, &self.hud);
        self.draw_menu();
        self.draw_debug_overlay();
        self.gpu.take_draw_list(&mut self.draw_list);

        self.budget.record(self.gpu.frame()?)?;
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
        ModeRequest::mode(self.shell.as_ref(), self.window)
    }

    /// What a fired menu button does.
    ///
    /// The one place a button becomes an effect, so a menu that grows a fourth
    /// item cannot quietly do a fifth thing. Both input devices arrive here:
    /// [`MENU_ACTIVATE_KEY`] and a click produce the same [`MenuAction`] and this
    /// cannot tell them apart, which is what makes "the keyboard still works" and
    /// "the mouse works too" the same sentence.
    ///
    /// # Errors
    ///
    /// [`FlappyError`] if the shell refused a display-mode change.
    fn apply(&mut self, action: MenuAction) -> Result<(), FlappyError> {
        match action {
            MenuAction::Resume => {
                if self.paused {
                    self.paused = false;
                    crcbl::log::info!("game resumed");
                }
            }
            // A real key event rather than a call into `Game`: starting a run is
            // the simulation's business and the simulation is driven by its
            // action map, so a button that reached past it would be a second way
            // to start a game with its own bugs. The release is queued straight
            // after the press because a flap is an *edge* — a press with no
            // release leaves the action held for the rest of the run.
            MenuAction::Flap => {
                self.game.key_event(FLAP_KEY, true);
                self.game.key_event(FLAP_KEY, false);
            }
            MenuAction::Fullscreen => self.toggle_fullscreen()?,
            MenuAction::DebugOverlay => self.debug.toggle(),
        }
        Ok(())
    }

    /// Picks this frame's menu, lays it out, and emits both halves of it.
    ///
    /// **Two halves, two passes.** The window frame and the buttons are
    /// nine-sliced sprites and go to [`crcbl::render::MenuRenderer`]; the title
    /// and the labels are text and go to the UI pass through the draw list.
    /// `gpu.rs` declares the menu pass between the course and the UI for exactly
    /// this reason.
    ///
    /// Called after the HUD and before the debug overlay: the scrim dims the
    /// game *including its HUD*, and the overlay is a developer tool that must
    /// stay legible on top of everything.
    fn draw_menu(&mut self) {
        self.menus
            .show(MenuKind::of(self.paused, &self.render_state));
        let layout = self
            .menus
            .current()
            .map(|menu| menu.layout(self.gpu.extent(), self.gpu.atlas()));
        match &layout {
            Some(layout) => {
                let menu = self.menus.current().expect("a layout implies a menu");
                menu.render(&mut self.draw_list, layout);
                self.gpu.set_menu(Some((menu, layout)));
            }
            None => self.gpu.set_menu(None),
        }
    }

    /// Which menu this frame is showing, for the loop's own tests.
    #[cfg(test)]
    const fn menu_kind(&self) -> MenuKind {
        self.menus.kind()
    }

    /// Where this frame's menu was laid out, for the loop's own tests — so a
    /// scripted click lands on the button the player would have seen.
    #[cfg(test)]
    fn menu_layout(&self) -> Option<crcbl::ui::menu::MenuLayout> {
        self.menus
            .current()
            .map(|menu| menu.layout(self.gpu.extent(), self.gpu.atlas()))
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
        let game = &mut self.game;
        lose_focus(&mut self.held_keys, &mut self.paused, |key| {
            game.key_event(key, false);
        });
    }

    /// Asks for the mode the window is not in.
    ///
    /// The target comes from [`display_mode`](Self::display_mode) — the mode in
    /// effect — rather than from what was last requested, so a fullscreen the
    /// player left by some other route (Escape in a browser, a compositor
    /// keybinding) leaves F11 meaning "go fullscreen" again.
    fn toggle_fullscreen(&mut self) -> Result<(), FlappyError> {
        Ok(ModeRequest::toggle(self.shell.as_mut(), self.window)?)
    }

    /// Logs the moment the window system stops agreeing with the request.
    ///
    /// Once per transition, not once per frame: a backend that cannot do
    /// fullscreen at all would otherwise print a line every frame forever.
    fn check_mode_request(&mut self) {
        self.mode.check(self.shell.as_ref(), self.window);
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
            crcbl::math::Vec2::new(width as f32, height as f32),
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
            crcbl::log::info!("{}", timings.report().trim_end());
        }
        let summary = Summary {
            backend: self.shell.backend(),
            frames: self.budget.presented(),
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

/// Creates the one window this game has: its title, its app id, its size.
///
/// Everything else is [`crcbl::engine::open_window`]'s.
fn open_the_window<S: Shell + ?Sized>(
    shell: &mut S,
    clock_source: &Clock,
) -> Result<WindowId, FlappyError> {
    Ok(crcbl::engine::open_window(
        shell,
        clock_source,
        &WindowDesc {
            title: "Flappy",
            app_id: "sh.kryptic.crcbl.flappy",
            size: LogicalSize::new(960.0, 720.0),
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
    /// [`FlappyError`] if the shell refused the window.
    pub fn request(
        mut shell: Box<S>,
        options: &Options,
        clock_source: Clock,
    ) -> Result<Self, FlappyError> {
        let window = open_the_window(shell.as_mut(), &clock_source)?;
        Ok(Self {
            boot: crcbl::engine::PolledBoot::request(
                shell,
                window,
                clock_source,
                options.common.backend,
            ),
            options: options.clone(),
        })
    }

    /// Advances start-up. `Ok(None)` means "not yet, poll again next frame".
    ///
    /// # Errors
    ///
    /// [`FlappyError`] if the window went away before it had a size, if the device
    /// request failed, or if the game could not be built.
    pub fn poll(&mut self) -> Result<Option<Loop<S>>, FlappyError> {
        let Some(booted) = self.boot.poll::<FlappyError>()? else {
            return Ok(None);
        };
        Loop::assemble(
            booted.shell,
            booted.window,
            booted.gpu,
            &self.options,
            booted.clock_source,
            booted.events,
        )
        .map(Some)
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
    use crcbl::math::Vec2;

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
    use crcbl::args::Common;
    use crcbl::engine::{
        DEBUG_OVERLAY_KEY, FULLSCREEN_KEY, MENU_ACTIVATE_KEY, MENU_DOWN_KEY, PAUSE_KEY,
    };

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
        let mut engine = scripted(&headless_with(16, |common| {
            common.debug_overlay = Some(false)
        }));
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
        let mut engine = scripted(&headless_with(8, |common| {
            common.debug_overlay = Some(true)
        }));
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
        let thirty = run(&headless_with(62, |common| common.tick_hz = 30))
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
        let fast = run(&headless_with(62, |common| common.tick_hz = 120))
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
        let mut engine = Loop::start(&headless_with(40, |common| common.tick_hz = 120))
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

    /// **A canvas is handed the keyboard by a click *in the game*, and that
    /// click still must not resume.**
    ///
    /// The sibling above moves focus with nothing else attached, which is what a
    /// window manager does. A browser has no such gesture: `web/engine/shell.js`
    /// calls `canvas.focus()` from its own `pointerdown` listener, so the event
    /// that restores focus is also a press at a real position, and whether the
    /// game resumes depends on what is under it. Both outcomes are correct and
    /// the browser gate confused them — it clicked the canvas's centre, the pause
    /// menu is centred, `RESUME` is the item the centre lands in, and a check
    /// named "focus coming back does not resume on its own" went red because the
    /// game did what [`a_click_on_resume_resumes_the_game`] requires.
    ///
    /// So: the corner is asserted to be over nothing, because
    /// `web/tools/browser-e2e.mjs` clicks there and a menu that grew until it
    /// reached the corner would quietly make that gate meaningless again; and the
    /// centre is asserted to be over `RESUME`, because that is the fact both
    /// files' comments explain the bug with.
    #[test]
    fn a_focusing_click_off_every_button_leaves_the_game_paused() {
        use crcbl::core::input::PointerButton;
        use crcbl::shell::{ButtonState as PointerState, PhysicalPoint};

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
        let over = |point: crcbl::math::Vec2| {
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

        let corner = crcbl::math::Vec2::splat(INSET);
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
             this file and in web/tools/browser-e2e.mjs explain the browser gate's \
             failure with that fact and need rewriting if it stops being true",
        );

        // Focus comes back the way a browser gives it back: a press and release
        // at a real position, which here is over nothing.
        engine
            .shell_mut()
            .set_focus(window, true)
            .expect("the window is live");
        let at = PhysicalPoint::new(f64::from(corner.x), f64::from(corner.y));
        engine
            .shell_mut()
            .button(window, PointerButton::Left, PointerState::Pressed, Some(at))
            .expect("the window is live");
        engine.frame().expect("a frame");
        engine
            .shell_mut()
            .button(
                window,
                PointerButton::Left,
                PointerState::Released,
                Some(at),
            )
            .expect("the window is live");
        run_frames(&mut engine, 5);
        assert!(
            engine.is_paused(),
            "the click that handed the keyboard back also resumed the game",
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

    // -----------------------------------------------------------------------
    // The menus
    // -----------------------------------------------------------------------

    /// **The start menu is on screen before the first flap, and it reaches both
    /// passes.** The text is in the draw list the UI pass uploads and the frame
    /// is in the sprite list the menu pass draws — a menu that only made it to
    /// one of the two is a panel with no words or words with no panel.
    #[test]
    fn the_start_menu_is_drawn_before_the_first_flap() {
        let mut engine = scripted(&headless(60));
        run_frames(&mut engine, 2);
        assert_eq!(engine.menu_kind(), MenuKind::Start);

        let drawn = ui_text(&engine);
        assert!(
            drawn.iter().any(|t| t == "FLAPPY") && drawn.iter().any(|t| t == "FLY"),
            "the start menu's text is not in the draw list: {drawn:?}",
        );

        let sprites = engine.gpu.menu_sprites();
        assert_eq!(sprites.len(), 1 + 9 + 9 * 3, "{}", sprites.len());
        let extent = engine.extent();
        let scale = engine
            .menu_layout()
            .expect("a menu is showing")
            .style()
            .scale;
        assert_eq!(
            sprites[0].rect,
            [
                -(extent.0 as f32) / (2.0 * scale),
                -(extent.1 as f32) / (2.0 * scale),
                extent.0 as f32 / scale,
                extent.1 as f32 / scale,
            ],
            "the scrim does not cover the framebuffer",
        );
        engine.finish(ExitReason::FrameBudget).expect("teardown");
    }

    /// **A run in the air draws no menu at all**, and the menu pass is handed
    /// nothing — which is what makes it free rather than cheap.
    #[test]
    fn a_run_in_the_air_draws_no_menu() {
        let mut engine = scripted(&headless(60));
        let window = engine.window();
        engine
            .shell_mut()
            .key_press(window, KeyCode::Space)
            .expect("the window is live");
        run_frames(&mut engine, 10);
        assert_eq!(engine.menu_kind(), MenuKind::None);
        assert!(
            engine.gpu.menu_sprites().is_empty(),
            "a flying frame submitted {} menu sprites",
            engine.gpu.menu_sprites().len(),
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

    /// The arrows move the selection, and Enter fires **what is selected** —
    /// asserted on an effect the window system reports, not on an index.
    #[test]
    fn the_arrows_choose_which_button_enter_fires() {
        let mut engine = scripted(&headless(200));
        let window = engine.window();
        run_frames(&mut engine, 2);
        assert_eq!(engine.display_mode(), DisplayMode::Windowed);

        engine
            .shell_mut()
            .key_press(window, PAUSE_KEY)
            .expect("the window is live");
        engine.frame().expect("a frame");

        engine
            .shell_mut()
            .key_press(window, MENU_DOWN_KEY)
            .expect("the window is live");
        engine
            .shell_mut()
            .key_press(window, MENU_ACTIVATE_KEY)
            .expect("the window is live");
        engine
            .shell_mut()
            .key_release(window, MENU_ACTIVATE_KEY)
            .expect("the window is live");
        run_frames(&mut engine, 6);

        assert_eq!(
            engine.display_mode(),
            DisplayMode::Borderless { monitor: None },
            "Enter fired the wrong button, or the arrows did not move",
        );
        assert!(
            engine.is_paused(),
            "the second button resumed the game, so the selection never moved",
        );
        engine.finish(ExitReason::FrameBudget).expect("teardown");
    }

    /// **A click fires the button under it**, through the same action path the
    /// keyboard uses — and a click that started somewhere else fires nothing.
    #[test]
    fn a_click_on_resume_resumes_the_game() {
        use crcbl::core::input::PointerButton;
        use crcbl::shell::{ButtonState as PointerState, PhysicalPoint};

        let mut engine = scripted(&headless(60));
        let window = engine.window();
        run_frames(&mut engine, 2);
        engine
            .shell_mut()
            .key_press(window, PAUSE_KEY)
            .expect("the window is live");
        engine.frame().expect("a frame");
        assert!(engine.is_paused());

        let layout = engine.menu_layout().expect("the pause menu is showing");
        let resume = layout.items()[0];
        let centre = (resume.min + resume.max) * 0.5;
        let at = PhysicalPoint::new(f64::from(centre.x), f64::from(centre.y));

        engine
            .shell_mut()
            .button(
                window,
                PointerButton::Left,
                PointerState::Pressed,
                Some(PhysicalPoint::new(3.0, 3.0)),
            )
            .expect("the window is live");
        engine.frame().expect("a frame");
        engine
            .shell_mut()
            .button(
                window,
                PointerButton::Left,
                PointerState::Released,
                Some(at),
            )
            .expect("the window is live");
        engine.frame().expect("a frame");
        assert!(
            engine.is_paused(),
            "a press that started off the button still fired it",
        );

        engine
            .shell_mut()
            .button(window, PointerButton::Left, PointerState::Pressed, Some(at))
            .expect("the window is live");
        engine.frame().expect("a frame");
        engine
            .shell_mut()
            .button(
                window,
                PointerButton::Left,
                PointerState::Released,
                Some(at),
            )
            .expect("the window is live");
        engine.frame().expect("a frame");
        assert!(!engine.is_paused(), "a click on RESUME did not resume");
        engine.finish(ExitReason::FrameBudget).expect("teardown");
    }

    /// **The key printed on a button still does what it always did.** Space is
    /// flappy's primary flap binding and the menu never takes it, so it starts
    /// the run with the start menu on screen and no menu key involved.
    #[test]
    fn space_still_flies_with_the_start_menu_showing() {
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
            "the run never started, so the start menu ate the key",
        );
        engine.finish(ExitReason::FrameBudget).expect("teardown");
    }

    /// And the `FLY` button does the same thing the key does — the action goes
    /// through `game.rs`'s action map rather than round it.
    #[test]
    fn the_fly_button_starts_the_run() {
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
            "FLY did not start the run",
        );
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
            drawn.iter().any(|t| t == "PAUSED") && drawn.iter().any(|t| t == "RESUME"),
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
        assert!(!engine.mode.honoured(), "the refusal has to be noticed");

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
