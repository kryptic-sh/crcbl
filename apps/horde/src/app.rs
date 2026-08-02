//! The horde engine loop: window, fixed-timestep accumulator, one draw.
//!
//! Shape matches `apps/asteroids/src/app.rs`, `apps/flappy/src/app.rs` and
//! `apps/breakout/src/app.rs` — same clock, same event loop, same accumulator,
//! same menus, same pause, fullscreen, focus and pointer handling. **This is the
//! fourth copy of that block**, and `docs/backlog.md` says so: the pump's key
//! branch, `lose_focus`, the F11 toggle, `Loop::paused` and the pointer's
//! press-capture bookkeeping are the same code in four files now. See the S2
//! findings note in `docs/plan/ROADMAP.md`.
//!
//! # What is this sample's own
//!
//! **A menu whose buttons are simulation state.** The other three samples build
//! every menu once, because a `RESUME` button says `RESUME` forever. This one's
//! level-up panel is three upgrades drawn from the run's seed, so the loop hands
//! `Menus::set_offer` whatever the render state carries and the panel is rebuilt
//! when — and only when — that changes. Firing one of its buttons presses a
//! **real digit key** into the game's action map rather than calling into
//! `Game`, for the reason asteroids' `FLY` button does: which upgrade a run took
//! is state a seeded, scripted replay has to reproduce.
//!
//! # There is a start menu, and it arrived late
//!
//! This sample shipped without one — the argument, and the user's reversal of
//! it, are in `game::GameState`'s docs and `crate::menu`'s header. The loop's
//! four menus are start, pause, level-up and death, and the loop itself does
//! **nothing** to hold the game on the first of them: `run_tick` short-circuits
//! on `GameState::WaitingToStart` exactly as it does on `LevelUp`, so a start
//! screen that failed to hold would be a simulation bug and not a loop one. A
//! loop that stopped calling `tick` instead would have nothing to feed the start
//! key into.
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
//!
//! # The draw is culled and nothing else is capped
//!
//! This is the sample whose whole question is how many agents a tick can carry.
//! The placeholder renderer capped the number of quads it would emit because a
//! `DrawList` quad was six vertices uploaded per frame; an instanced sprite is
//! not, so [`crate::art`] culls to the view and draws everything that survives.

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
use crcbl::ui::draw_list::DrawList;
use crcbl::ui::{DebugOverlay, PointerInput};
use glam::Vec2;

use crate::game::{self, Game, GameState, RenderState, UPGRADE_CHOICES};
use crate::gpu::{Gpu, PendingGpu};
use crate::menu::{MenuAction, MenuKind, Menus};

pub use crate::args::Options;

// ---- summary ----------------------------------------------------------------

#[derive(Clone, Debug, PartialEq)]
pub struct Summary {
    pub backend: Backend,
    pub frames: u64,
    pub ticks: u64,
    pub events: u64,
    pub extent: (u32, u32),
    pub exit: ExitReason,
    /// How long the run lasted, in simulated seconds.
    pub elapsed: f64,
    pub kills: u64,
    pub level: u32,
    pub enemies: usize,
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
pub enum HordeError {
    NoWindowSystem(ShellError),
    Shell(ShellError),
    Configure(ConfigureError),
    NeverPresented,
    Gpu(GpuError),
    Game(game::GameError),
}

impl std::fmt::Display for HordeError {
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

impl std::error::Error for HordeError {}

impl From<ShellError> for HordeError {
    fn from(error: ShellError) -> Self {
        Self::Shell(error)
    }
}

impl From<GpuError> for HordeError {
    fn from(error: GpuError) -> Self {
        Self::Gpu(error)
    }
}

impl From<game::GameError> for HordeError {
    fn from(error: game::GameError) -> Self {
        Self::Game(error)
    }
}

impl From<ConfigureError> for HordeError {
    fn from(error: ConfigureError) -> Self {
        Self::Configure(error)
    }
}

// ---- the loop ---------------------------------------------------------------

/// The key that shows and hides the debug overlay.
///
/// F3, the key breakout, flappy and asteroids use: "switching it on is one
/// thing" is only true if it is the *same* thing in every sample.
pub const DEBUG_OVERLAY_KEY: KeyCode = KeyCode::F3;

/// The key that pauses and resumes. Escape, as in the other three, and free
/// here: `game.rs` declares WASD, the arrows, R, Space and the digits.
pub const PAUSE_KEY: KeyCode = KeyCode::Escape;

/// The key that asks for fullscreen, and asks to leave it.
pub const FULLSCREEN_KEY: KeyCode = KeyCode::F11;

/// The keys a menu takes for itself while one is on screen.
///
/// The same three in every sample, for the reason F3, Escape and F11 are the
/// same in every sample. Two of them are free here; **ArrowUp is not** — it is
/// the second binding of `game.rs`'s "up" movement action, beside `KeyW`, and a
/// panel on screen shadows it. `crate::menu`'s header carries the argument; the
/// short version is that `KeyW` still walks and that a menu is only ever up on a
/// frame the game is not being played on.
///
/// They are consumed **only while a menu is showing**.
pub const MENU_UP_KEY: KeyCode = KeyCode::ArrowUp;
/// See [`MENU_UP_KEY`].
pub const MENU_DOWN_KEY: KeyCode = KeyCode::ArrowDown;
/// See [`MENU_UP_KEY`].
pub const MENU_ACTIVATE_KEY: KeyCode = KeyCode::Enter;

/// The key the start menu's `PLAY` and the death menu's `TRY AGAIN` stand for.
///
/// Fired as a real key event rather than by calling into [`Game`], because
/// beginning a run is the simulation's business and the simulation is driven by
/// its action map.
///
/// `R` and not `Space`, even though the start button prints `SPACE`: `game.rs`
/// binds **both** to the one `restart` action, so the two are the same edge and
/// the button's hint is the key a player coming from another demo would reach
/// for. `the_key_the_start_button_prints_starts_the_run` holds the pair
/// together.
const RESTART_KEY: KeyCode = KeyCode::KeyR;

/// The keys the level-up menu's three buttons stand for, in offer order. The
/// same three `game.rs` binds to its `choose` actions.
const CHOOSE_KEYS: [KeyCode; UPGRADE_CHOICES] = [KeyCode::Digit1, KeyCode::Digit2, KeyCode::Digit3];

#[derive(Debug)]
pub struct Loop<S: Shell + ?Sized = dyn Shell> {
    shell: Box<S>,
    window: WindowId,
    gpu: Gpu,
    game: Game,
    clock_source: Clock,
    frame_clock: FrameClock,
    /// Reused every frame, so a steady-state frame does not allocate a fresh
    /// draw list or render state.
    draw_list: DrawList,
    render_state: RenderState,
    hud: HudStrings,
    /// The pause, level-up and death menus, and which is on screen.
    menus: Menus,
    /// Where [`Loop::draw_menu`] last laid the menu out, or `None` on a frame
    /// that showed none.
    ///
    /// Kept rather than recomputed because it is wanted **twice** in the same
    /// frame — the UI pass's text and the menu pass's sprites are two halves of
    /// one layout — and a second call is a second chance to measure it against a
    /// different extent.
    menu_layout: Option<crcbl::ui::menu::MenuLayout>,
    /// Where the pointer was last seen, in framebuffer pixels.
    ///
    /// Kept across frames because motion and buttons arrive as separate events
    /// and a click carries a position only on some backends. `None` until the
    /// pointer has been inside the window, so a menu does not open with a
    /// phantom cursor at the origin.
    pointer: Option<Vec2>,
    /// Whether the primary pointer button is down.
    ///
    /// Kept here rather than derived per frame because press capture spans
    /// frames: a press that starts on a button and is released two frames later
    /// must still be *held* on the frame in between, or the button flickers back
    /// to idle under the finger.
    pointer_held: bool,
    /// The modular debug panel: frame timing always, GPU pass timings when the
    /// device has them.
    debug: DebugOverlay,
    /// Whether the simulation is stopped. **The loop owns this, not
    /// [`GameState`]**: a `Paused` variant in the simulation would make the
    /// authoritative server's state depend on which window a compositor has
    /// focused. [`GameState::LevelUp`] is the other way round for the opposite
    /// reason — see `game.rs`.
    paused: bool,
    /// Keys forwarded to the game as pressed and not yet released.
    ///
    /// [`ShellEvent::Focus`] documents the obligation this discharges: no
    /// platform delivers releases for keys held when focus leaves. Every one of
    /// this game's movement keys is a *held* action, so a lost release leaves
    /// the player walking into a wall for the rest of the session.
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
/// [`HordeError`] if the shell, the GPU or the game failed. Teardown runs on
/// every path: a failing frame must still release the swapchain, the surface and
/// the window.
pub fn run(options: &Options) -> Result<Summary, HordeError> {
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
    /// [`HordeError`] if any of them refused.
    pub fn start(options: &Options) -> Result<Self, HordeError> {
        let shell = if options.headless {
            open_backend(Backend::Headless).map_err(HordeError::Shell)?
        } else {
            open().map_err(HordeError::NoWindowSystem)?
        };
        Self::with_shell(shell, options)
    }
}

impl<S: Shell + ?Sized> Loop<S> {
    /// Builds the loop on an already-open shell.
    ///
    /// # Errors
    ///
    /// [`HordeError`] if the window never configured, the GPU would not open, or
    /// the game could not be built.
    pub fn with_shell(mut shell: Box<S>, options: &Options) -> Result<Self, HordeError> {
        // **`--wall-clock` is why this is not `Clock::new(options.headless)`.**
        // A headless run's clock is a fake one stepping exactly 1/60 s, which is
        // what makes a scripted run reproducible and what makes the debug
        // panel's frame timing report the step rather than the frame. The scale
        // measurement needs the second of those to be a real number; every other
        // headless run needs the first. See `crate::args`.
        let clock_source = Clock::new(!options.real_clock());
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
    ) -> Result<Self, HordeError> {
        let mut game = Game::with_setup(&options.setup())?;
        if options.prefill > 0 {
            let staged = game.stage_field(options.prefill);
            if staged < options.prefill {
                log::warn!(
                    "prefill: asked for {} enemies and the cap left room for {staged}",
                    options.prefill,
                );
            }
            // **A prefilled field starts itself.** `--prefill` is the scale
            // measurement's fixture and every number in
            // `docs/plan/sample/03-horde.md` was taken through it; left on the
            // title screen it would time a simulation that short-circuits on its
            // first line, and report it as ten thousand enemies a tick. The edge
            // is queued, not poked, so it goes through the same action map a
            // player's key does.
            game.key_event(RESTART_KEY, true);
            game.key_event(RESTART_KEY, false);
            log::info!("prefill: started the run without waiting for the title screen");
        }
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
            menus: Menus::new(),
            menu_layout: None,
            pointer: None,
            pointer_held: false,
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

    /// Sets how far one [`frame`](Self::frame) advances a manual clock.
    ///
    /// **The browser's clock is the browser's.** `Clock::Real` reads
    /// [`std::time::Instant`], which on `wasm32-unknown-unknown` has no
    /// implementation at all and panics on the first `now()`. `dt` is clamped to
    /// [`MAX_FRAME_STEP`]: a backgrounded tab resumes with a multi-second gap,
    /// and feeding that to the accumulator spends the next frame running
    /// thousands of ticks — which in this game is a minute of spawns arriving at
    /// once.
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

    /// Whether the simulation is stopped.
    #[must_use]
    pub const fn is_paused(&self) -> bool {
        self.paused
    }

    /// One frame: pump, tick the simulation to catch up with the clock, draw.
    ///
    /// # Errors
    ///
    /// [`HordeError`] if the shell or the GPU failed.
    pub fn frame(&mut self) -> Result<Flow, HordeError> {
        if self.budget.is_some_and(|budget| self.frames >= budget) {
            return Ok(Flow::Stop(ExitReason::FrameBudget));
        }

        if self.windowed {
            self.shell.wait_events(Some(WINDOWED_IDLE));
        }

        let mut pending = Pending::default();
        let (mut toggle_debug, mut toggle_pause, mut toggle_fullscreen) = (false, false, false);
        let mut focus_lost = false;
        // The pointer's three facts for this frame. `pointer_pos` starts at what
        // the last frame left, because motion and buttons are separate events
        // and a click carries a position only on some backends.
        let mut pointer_pos = self.pointer;
        let (mut pointer_pressed, mut pointer_released) = (false, false);
        let mut keyboard_action: Option<MenuAction> = None;
        let game = &mut self.game;
        let held = &mut self.held_keys;
        let menus = &mut self.menus;
        // **Last frame's menu, deliberately.** The pump runs before this frame's
        // state is known, and the menu the player is pressing keys at is the one
        // that was on screen when they pressed them.
        let menu_showing = menus.kind() != MenuKind::None;
        self.shell.pump(&mut |event| {
            pending.observe(&event);
            match event {
                // Losing focus is not a key event and never will be: the
                // releases for whatever was held are exactly what no platform
                // sends. See `ShellEvent::Focus`.
                ShellEvent::Focus { focused: false, .. } => focus_lost = true,
                ShellEvent::PointerMotion {
                    abs: Some(point), ..
                } => pointer_pos = Some(Vec2::new(point.x as f32, point.y as f32)),
                // A pointer that left the window is not hovering anything, and
                // must not leave the last button it crossed lit up.
                ShellEvent::PointerFocus {
                    entered, position, ..
                } => {
                    pointer_pos = if entered {
                        position.map(|point| Vec2::new(point.x as f32, point.y as f32))
                    } else {
                        None
                    };
                }
                ShellEvent::Button {
                    button: crcbl::core::input::PointerButton::Left,
                    state,
                    position,
                    ..
                } => {
                    if let Some(point) = position {
                        pointer_pos = Some(Vec2::new(point.x as f32, point.y as f32));
                    }
                    if matches!(state, crcbl::shell::ButtonState::Pressed) {
                        pointer_pressed = true;
                    } else {
                        pointer_released = true;
                    }
                }
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
                    // The menu's three keys, taken only while one is on screen —
                    // see `MENU_UP_KEY`. Repeats move the selection, because
                    // holding Down to walk a list is what a player expects; the
                    // commit key fires on **release**, so the pressed frame of
                    // the skin is on screen for as long as the key is held.
                    if menu_showing {
                        match code {
                            MENU_UP_KEY => {
                                if pressed {
                                    menus.select_previous();
                                }
                                return;
                            }
                            MENU_DOWN_KEY => {
                                if pressed {
                                    menus.select_next();
                                }
                                return;
                            }
                            MENU_ACTIVATE_KEY => {
                                if pressed {
                                    menus.press(true);
                                } else {
                                    keyboard_action = menus.activate();
                                }
                                return;
                            }
                            _ => {}
                        }
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
        self.pointer = pointer_pos;
        if pointer_pressed {
            self.pointer_held = true;
        }
        // **`down` must be false on the frame the button came up**, or
        // `UiState::interact` latches the capture and fires it in the same call —
        // and a press that started in the corner of the screen would be credited
        // to whatever the cursor was over at release, which is the exact bug
        // press capture exists to prevent.
        //
        // Except when the press *also* arrived this frame: a click faster than a
        // frame is one event pair, and it must latch and fire together or a quick
        // tap does nothing.
        let pointer_down = pointer_pressed || (self.pointer_held && !pointer_released);
        // Hit-tested against **this** frame's layout, which is why the pointer is
        // resolved here and not inside the pump: the rectangles depend on the
        // framebuffer's size, and a click checked against last frame's would miss
        // on the frame a resize lands.
        let pointer_action = self.menus.point(
            self.gpu.extent(),
            self.gpu.atlas(),
            PointerInput {
                // A pointer that has never been in the window is nowhere, not at
                // the origin — which is a real pixel, inside the HUD.
                pos: self.pointer.unwrap_or(Vec2::splat(f32::NEG_INFINITY)),
                down: pointer_down,
                released: pointer_released,
            },
        );
        if pointer_released {
            self.pointer_held = false;
        }
        for action in [keyboard_action, pointer_action].into_iter().flatten() {
            self.apply(action)?;
        }

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
        //
        // A level-up screen is **not** handled here: the simulation keeps
        // ticking through it and freezes its own field, because the choice is
        // simulation state and a loop that stopped calling `tick` would have
        // nothing to feed the choice into.
        if self.paused {
            while self.frame_clock.consume_tick() {}
        } else {
            while self.frame_clock.consume_tick() {
                self.ticks += 1;
                self.game.tick();
            }
        }

        self.game.render_state(&mut self.render_state);
        self.gpu.set_world(&self.render_state);

        self.draw_list.clear();
        self.hud.refresh(&self.render_state, self.paused);
        draw_hud(&mut self.draw_list, &self.hud);
        self.draw_menu();
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
                    return Err(HordeError::NeverPresented);
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
    /// [`HordeError`] if the shell refused a display-mode change.
    fn apply(&mut self, action: MenuAction) -> Result<(), HordeError> {
        match action {
            MenuAction::Resume => {
                if self.paused {
                    self.paused = false;
                    log::info!("game resumed");
                }
            }
            // Real key events rather than calls into `Game`: restarting a run
            // and taking an upgrade are the simulation's business and the
            // simulation is driven by its action map. The release is queued
            // straight after the press because both are *edges* — a press with
            // no release leaves the action held, which for the restart key is a
            // run that begins again sixty times a second.
            MenuAction::Restart => {
                self.game.key_event(RESTART_KEY, true);
                self.game.key_event(RESTART_KEY, false);
            }
            MenuAction::Choose(index) => {
                if let Some(&key) = CHOOSE_KEYS.get(index) {
                    self.game.key_event(key, true);
                    self.game.key_event(key, false);
                }
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
    /// `gpu.rs` declares the menu pass between the game and the UI for exactly
    /// this reason.
    ///
    /// Called after the HUD and before the debug overlay: the scrim dims the
    /// game *including its HUD*, and the overlay is a developer tool that must
    /// stay legible on top of everything.
    fn draw_menu(&mut self) {
        // Before `show`, so the panel a level-up frame switches to is already
        // the one this level's offer built.
        self.menus
            .set_offer(self.render_state.level, self.render_state.offer);
        self.menus
            .show(MenuKind::of(self.paused, &self.render_state));
        self.menu_layout = self
            .menus
            .current()
            .map(|menu| menu.layout(self.gpu.extent(), self.gpu.atlas()));
        match (&self.menu_layout, self.menus.current()) {
            (Some(layout), Some(menu)) => {
                menu.render(&mut self.draw_list, layout);
                self.gpu.set_menu(Some((menu, layout)));
            }
            _ => self.gpu.set_menu(None),
        }
    }

    /// Which menu this frame is showing, for the loop's own tests.
    #[cfg(test)]
    const fn menu_kind(&self) -> MenuKind {
        self.menus.kind()
    }

    /// Where this frame's menu was laid out, for the loop's own tests — so a
    /// scripted click lands on the button the player would have seen.
    ///
    /// **The layout the frame actually used**, not a fresh one measured the same
    /// way: a test that recomputed it would agree with a `draw_menu` that
    /// measured against the wrong framebuffer, which is the one mistake there is
    /// to make here.
    #[cfg(test)]
    const fn menu_layout(&self) -> Option<&crcbl::ui::menu::MenuLayout> {
        self.menu_layout.as_ref()
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
    fn toggle_fullscreen(&mut self) -> Result<(), HordeError> {
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
    /// the first slice rather than at the end** — more so for this sample than
    /// for any other on the ladder, because its claim is a flat CPU cost at
    /// scale and the panel's frame-timing module is where that is read.
    fn draw_debug_overlay(&mut self) {
        self.debug.begin_frame();
        if let Some(timings) = self.gpu.timings() {
            self.debug.panel.add(timings);
        }
        // **This sample's own module, and the first one any sample has added.**
        // Asteroids' finding 8 said switching the panel on needs no per-sample
        // plumbing, and it does not; adding a per-sample *section* is the other
        // half of the same claim and it is four lines. The numbers are the ones
        // this game's whole argument rests on — how much of the field survived
        // the cull, and how many draw calls the survivors cost.
        self.debug.panel.add(&self.gpu.scene_stats());
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
    /// [`HordeError`] if the GPU or the shell failed to release something. Both
    /// are attempted regardless: the window is destroyed even when the GPU
    /// teardown failed, because leaving it mapped is strictly worse.
    pub fn finish(mut self, exit: ExitReason) -> Result<Summary, HordeError> {
        if let Some(timings) = self.gpu.timings()
            && !timings.is_empty()
        {
            log::info!("{}", timings.report().trim_end());
        }
        // **The CPU half of the same report, and the reason `--wall-clock`
        // exists.** `FrameTimings` is GPU timestamps; this is the monotonic
        // clock the loop was driven from, which on a headless run without
        // `--wall-clock` is the fixed step and therefore says nothing. Printed
        // either way, with the clock named, because a number whose conditions
        // are not stated is not a measurement.
        let frame = &self.debug.frame;
        if let (Some(best), Some(worst)) = (frame.best(), frame.worst()) {
            log::info!(
                "frame cpu ({} clock, last {} frames): mean {:.3} ms ({:.1} fps), \
                 best {:.3} ms, worst {:.3} ms; scene {:?}",
                if matches!(self.clock_source, Clock::Real(_)) {
                    "real"
                } else {
                    "fixed-step"
                },
                frame.len(),
                frame.mean().as_secs_f64() * 1e3,
                frame.fps(),
                best.as_secs_f64() * 1e3,
                worst.as_secs_f64() * 1e3,
                self.gpu.scene_stats(),
            );
        }
        let summary = Summary {
            backend: self.shell.backend(),
            frames: self.frames,
            ticks: self.ticks,
            events: self.events,
            extent: self.gpu.extent(),
            exit,
            elapsed: self.game.elapsed,
            kills: self.game.kills,
            level: self.game.level,
            enemies: self.game.enemy_count(),
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
/// crash — and in this game, as a minute of spawns landing at once on a player
/// who has already been eaten.
pub const MAX_FRAME_STEP: Duration = Duration::from_millis(64);

/// Creates the one window, and puts the shell's event clock on the engine's.
fn open_the_window<S: Shell + ?Sized>(
    shell: &mut S,
    clock_source: &Clock,
) -> Result<WindowId, HordeError> {
    log::info!(
        "shell: {} backend, caps {:?}",
        shell.backend(),
        shell.caps()
    );
    shell.align_event_clock(clock_source.elapsed());
    Ok(shell.create_window(&WindowDesc {
        title: "Horde",
        app_id: "sh.kryptic.crcbl.horde",
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
    /// [`HordeError`] if the shell refused the window.
    pub fn request(
        mut shell: Box<S>,
        options: &Options,
        clock_source: Clock,
    ) -> Result<Self, HordeError> {
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
    /// [`HordeError`] if the window went away before it had a size, if the
    /// device request failed, or if the game could not be built.
    pub fn poll(&mut self) -> Result<Option<Loop<S>>, HordeError> {
        let Some(shell) = self.shell.as_mut() else {
            return Err(HordeError::Gpu(GpuError::Unusable(
                "this horde loop was already started",
            )));
        };

        let mut pending = Pending::default();
        shell.pump(&mut |event| pending.observe(&event));
        self.events += pending.count;
        if pending.destroyed {
            return Err(HordeError::Shell(ShellError::invalid_window(self.window)));
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
            BootStage::Done => Err(HordeError::Gpu(GpuError::Unusable(
                "this horde loop was already started",
            ))),
        }
    }
}

// ---- drawing ----------------------------------------------------------------

/// The HUD's lines, rebuilt only when the numbers behind them change.
///
/// `DrawList::text` needs an owned `String`, so the alternative is a `format!`
/// per line every frame at whatever rate the window runs. The clock is keyed on
/// **tenths of a second** rather than on the raw `f64`, which is both what the
/// line shows and the only way an `f64` can be part of an `Eq` key.
#[derive(Debug, Default)]
struct HudStrings {
    stats: String,
    state: String,
    last: Option<HudKey>,
}

type HudKey = (u64, u64, u32, u32, u64, usize, u32, Option<GameState>, bool);

/// `seconds` as `m:ss`.
fn clock(seconds: f64) -> String {
    let whole = seconds.max(0.0) as u64;
    format!("{}:{:02}", whole / 60, whole % 60)
}

impl HudStrings {
    /// **`paused` wins over the simulation's state**, which is the bug flappy
    /// fixed: the status line used to read straight off the *server's* idea of
    /// what was happening, and the server is still playing while the window sits
    /// behind a browser.
    fn refresh(&mut self, render: &RenderState, paused: bool) {
        let key = (
            (render.elapsed * 10.0) as u64,
            render.kills,
            render.player_hp.max(0.0) as u32,
            render.level,
            render.xp,
            render.enemies.len(),
            render.best,
            render.state,
            paused,
        );
        if self.last == Some(key) {
            return;
        }
        self.last = Some(key);

        use std::fmt::Write as _;
        self.stats.clear();
        let _ = write!(
            self.stats,
            "{}   Kills: {}   HP: {:.0}/{:.0}   Lv {} ({}/{})   Enemies: {}",
            clock(render.elapsed),
            render.kills,
            render.player_hp.max(0.0),
            render.player_max_hp,
            render.level,
            render.xp,
            render.xp_needed,
            render.enemies.len(),
        );
        self.state.clear();
        // **The record lives on the second line, not the first**, and that is a
        // width decision rather than a taste one: the stat line already runs to
        // most of a 960-pixel window at the counts this game reaches, and the
        // second line is short in every state. `the_hud_fits_the_panel_it_is_drawn_on`
        // is what holds both of them.
        let _ = write!(self.state, "Best {}   ", clock(f64::from(render.best)));
        if paused {
            self.state.push_str("PAUSED - press ESC");
        } else {
            match render.state {
                Some(GameState::WaitingToStart) | None => {
                    self.state.push_str("PRESS SPACE TO PLAY - WASD to move");
                }
                Some(GameState::Dead) => {
                    let _ = write!(
                        self.state,
                        "YOU DIED - survived {}, {} kills - press R",
                        clock(render.elapsed),
                        render.kills,
                    );
                }
                Some(GameState::LevelUp) => {
                    self.state.push_str("LEVEL UP - press 1, 2 or 3");
                }
                Some(GameState::Playing) => {
                    self.state.push_str("WASD to move - the gun aims itself");
                }
            }
        }
    }
}

/// Draws the HUD, and nothing else.
///
/// **No scrim here any more.** The placeholder renderer dimmed the field behind
/// its death screen by hand; `crcbl_render::MenuRenderer` draws one behind every
/// menu, so a second would dim the field twice on exactly the frames a menu is
/// up.
fn draw_hud(dl: &mut DrawList, hud: &HudStrings) {
    dl.rect(
        HUD_ORIGIN,
        Vec2::new(HUD_PANEL_RIGHT, 52.0),
        [0.1, 0.1, 0.15, 0.85],
    );
    dl.text(
        Vec2::new(HUD_TEXT_X, 10.0),
        hud.stats.as_str(),
        [1.0, 1.0, 0.3, 1.0],
        HUD_STAT_SIZE,
    );
    dl.text(
        Vec2::new(HUD_TEXT_X, 32.0),
        hud.state.as_str(),
        [0.7, 0.7, 1.0, 1.0],
        HUD_STATE_SIZE,
    );
}

/// The HUD backdrop's top-left corner, in framebuffer pixels.
const HUD_ORIGIN: Vec2 = Vec2::new(4.0, 4.0);
/// Where the backdrop ends. See [`HUD_STAT_SIZE`].
const HUD_PANEL_RIGHT: f32 = 820.0;
/// Where both lines of text start.
const HUD_TEXT_X: f32 = 10.0;
/// The stat line's font size, and the reason the panel is as wide as it is.
///
/// **The width is measured, not guessed** — `the_hud_fits_the_panel_it_is_drawn_on`
/// puts a stated worst-case run through the real [`FontAtlas`] and requires it
/// inside [`HUD_PANEL_RIGHT`]. The placeholder renderer's 430 became 560 became
/// 690 by eye, and the browser gate's capture caught the last of those with the
/// text running off the end of its own backdrop.
///
/// What is bounded is a five-minute run at the shipped enemy cap. `--max-enemies
/// 10000` with a twenty-minute soak behind it can still outgrow this; that is
/// recorded in `docs/backlog.md` rather than solved by a panel two thirds of the
/// window wide.
const HUD_STAT_SIZE: f32 = 16.0;
/// The state line's, which is smaller because it is prose.
const HUD_STATE_SIZE: f32 = 14.0;

// ---- tests ------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use crcbl::core::input::PointerButton;
    use crcbl::shell::{ButtonState as PointerState, HeadlessShell, PhysicalPoint, ShellBackend};
    use crcbl::ui::draw_list::DrawCommand;
    use glam::DVec3;

    /// Options every test in this module builds its loop from.
    ///
    /// `Null` is not a detail: `headless` only says "no window", and without a
    /// backend named here the loop picks the real one and fails to start on any
    /// machine with no Vulkan driver — which is every plain CI runner. Breakout
    /// and flappy pin it for the same reason.
    fn headless(frames: u64) -> Options {
        Options {
            headless: true,
            backend: Some(GpuBackend::Null),
            frames: Some(frames),
            ..Options::default()
        }
    }

    fn headless_loop() -> Loop<dyn Shell> {
        let mut engine = Loop::start(&headless(8)).expect("a headless loop always starts");
        start_the_run(&mut engine.game);
        engine
    }

    /// A loop on a shell the test can post events to, with the run started.
    fn scripted(options: &Options) -> Loop<HeadlessShell> {
        let mut engine = at_the_title_screen(options);
        start_the_run(&mut engine.game);
        engine
    }

    /// The same, left on the title screen — for the tests that are *about* it.
    fn at_the_title_screen(options: &Options) -> Loop<HeadlessShell> {
        Loop::with_shell(Box::new(HeadlessShell::new()), options).expect("headless always starts")
    }

    /// Queues the start edge, so the loop's first frame ticks a run that is
    /// already playing and every test written before the start screen existed
    /// still measures what it measured.
    ///
    /// Through [`Game::key_event`] rather than by poking the state: the first
    /// tick replays it into the real action map, so a start path that stopped
    /// working takes the whole file down with it.
    fn start_the_run(game: &mut Game) {
        game.key_event(RESTART_KEY, true);
        game.key_event(RESTART_KEY, false);
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

    /// Presses and releases a key, and runs the frame that consumes it.
    fn tap(engine: &mut Loop<HeadlessShell>, code: KeyCode) {
        let window = engine.window;
        engine
            .shell
            .key_press(window, code)
            .expect("the window is live");
        engine
            .shell
            .key_release(window, code)
            .expect("the window is live");
        engine.frame().expect("a frame");
    }

    /// Clicks at `at`: press on one frame, release on the next, which is what
    /// the press-capture rule asks for.
    fn click(engine: &mut Loop<HeadlessShell>, at: Vec2) {
        let window = engine.window;
        let point = PhysicalPoint {
            x: f64::from(at.x),
            y: f64::from(at.y),
        };
        engine
            .shell
            .move_pointer(window, point, (0.0, 0.0))
            .expect("the window is live");
        for state in [PointerState::Pressed, PointerState::Released] {
            engine
                .shell
                .button(window, PointerButton::Left, state, Some(point))
                .expect("the window is live");
            engine.frame().expect("a frame");
        }
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
        assert_eq!(summary.state, GameState::Playing);
        assert_eq!(summary.level, 1);
        assert!(summary.elapsed > 0.0, "the clock never advanced");
    }

    /// **The field the loop built actually reaches the sprite pass.**
    ///
    /// `art::Scene` is tested directly, which says the sprite list is right and
    /// nothing at all about whether the loop hands the world over — and
    /// `set_world` is a copy, so a loop that forgot the call would draw the
    /// default `RenderState` forever.
    #[test]
    fn every_frame_hands_the_sprite_pass_the_field() {
        let mut engine = scripted(&headless(8));
        engine.frame().expect("a frame");
        // The player, at least: the field is empty for the first fraction of a
        // second of a run.
        assert!(
            !engine.gpu.scene_sprites().is_empty(),
            "the loop handed the sprite pass nothing",
        );

        engine.game_mut().freeze_spawns();
        engine.game_mut().clear_enemies();
        engine.game_mut().stage_player(DVec3::ZERO);
        engine
            .game_mut()
            .stage_enemy(game::EnemyKind::Grunt, DVec3::new(3.0, 0.0, 0.0));
        engine
            .game_mut()
            .stage_pickup(DVec3::new(-3.0, 0.0, 0.0), 1);
        engine.frame().expect("a frame");

        // By position, not by count: the gun fires at the staged enemy, so the
        // frame may also carry a bolt, and a bare count would be satisfied by
        // three of the wrong things.
        let sprites = engine.gpu.scene_sprites();
        let scale = f64::from(crate::art::TEXELS_PER_UNIT);
        for (what, x) in [("the gem", -3.0), ("the player", 0.0), ("the enemy", 3.0)] {
            let want = ((x - crate::art::ACTOR_HALF_EXTENT) * scale) as f32;
            assert!(
                sprites.iter().any(|s| (s.rect[0] - want).abs() < 1e-3),
                "{what} did not reach the sprite pass: {:?}",
                sprites.iter().map(|s| s.rect[0]).collect::<Vec<_>>(),
            );
        }
    }

    /// Escape stops the simulation without stopping the loop, and does not reach
    /// the game: a pause the simulation knew about would be a state a scripted,
    /// seeded run could reach.
    #[test]
    fn escape_pauses_the_simulation_and_never_reaches_the_game() {
        let mut engine = scripted(&headless(64));
        engine.frame().expect("a frame");
        tap(&mut engine, PAUSE_KEY);
        assert!(engine.is_paused(), "escape did not pause");

        let before = engine.ticks;
        run_frames(&mut engine, 3);
        assert_eq!(engine.ticks, before, "a paused frame ran a tick");
        assert_eq!(engine.menu_kind(), MenuKind::Paused);
    }

    /// The HUD reports the pause rather than the simulation's state, and the
    /// death line says how the run ended.
    #[test]
    fn the_hud_says_paused_even_though_the_simulation_is_not() {
        let mut hud = HudStrings::default();
        let render = RenderState {
            state: Some(GameState::Playing),
            player_hp: game::PLAYER_MAX_HP,
            player_max_hp: game::PLAYER_MAX_HP,
            elapsed: 74.0,
            kills: 12,
            level: 3,
            xp: 5,
            xp_needed: 16,
            ..RenderState::default()
        };
        hud.refresh(&render, false);
        assert!(hud.state.contains("WASD"), "{}", hud.state);
        assert!(hud.stats.contains("1:14"), "{}", hud.stats);
        assert!(hud.stats.contains("Kills: 12"), "{}", hud.stats);
        assert!(hud.stats.contains("Lv 3 (5/16)"), "{}", hud.stats);

        hud.refresh(&render, true);
        assert!(hud.state.contains("PAUSED"), "{}", hud.state);

        hud.refresh(
            &RenderState {
                state: Some(GameState::LevelUp),
                ..render.clone()
            },
            false,
        );
        assert!(hud.state.contains("LEVEL UP"), "{}", hud.state);

        hud.refresh(
            &RenderState {
                state: Some(GameState::Dead),
                elapsed: 133.0,
                kills: 208,
                ..render
            },
            false,
        );
        assert!(hud.state.contains("YOU DIED"), "{}", hud.state);
        assert!(hud.state.contains("2:13"), "{}", hud.state);
        assert!(hud.state.contains("208 kills"), "{}", hud.state);
    }

    /// **Both HUD lines fit the backdrop they are drawn on**, at a worst case
    /// this game can actually reach.
    ///
    /// Measured through the real [`crcbl::ui::text::FontAtlas`] — the same one
    /// the UI pass draws with — rather than by counting characters, and it is
    /// the check that was missing: the browser gate's canvas capture showed
    /// `Enemies: 9` sitting a hundred pixels past the end of its own panel, on a
    /// line three fields shorter than this one.
    #[test]
    fn the_hud_fits_the_panel_it_is_drawn_on() {
        use crcbl::ui::NATURAL_FONT_SIZE;
        let atlas = crcbl::ui::text::FontAtlas::built_in();

        // A five-minute run at the shipped cap, with every field at the widest
        // this game puts in it: nine hundred kills, six Vitality upgrades, a
        // level in double figures, and a full field.
        let render = RenderState {
            state: Some(GameState::Dead),
            player_hp: 0.0,
            player_max_hp: 250.0,
            elapsed: 300.0,
            kills: 2_048,
            level: 18,
            xp: 240,
            xp_needed: 1_024,
            best: 359,
            enemies: vec![
                game::EnemyView {
                    position: glam::DVec3::ZERO,
                    kind: game::EnemyKind::Grunt,
                    health: 1.0,
                };
                game::DEFAULT_MAX_ENEMIES
            ],
            ..RenderState::default()
        };
        let mut hud = HudStrings::default();
        hud.refresh(&render, false);

        for (line, size) in [(&hud.stats, HUD_STAT_SIZE), (&hud.state, HUD_STATE_SIZE)] {
            let right = HUD_TEXT_X + atlas.text_width(line, size / NATURAL_FONT_SIZE);
            assert!(
                right <= HUD_PANEL_RIGHT,
                "\"{line}\" ends at {right:.0} px, past the panel's {HUD_PANEL_RIGHT:.0}",
            );
        }
        // …and the panel is not simply enormous: it fits the window the game
        // opens at, which is what makes the assertion above a fit rather than a
        // licence.
        const { assert!(HUD_PANEL_RIGHT < 960.0) };
        // The record really is on the second line, or the width above is being
        // asserted about the wrong string.
        assert!(hud.state.starts_with("Best 5:59"), "{}", hud.state);
        assert!(!hud.stats.contains("Best"), "{}", hud.stats);
    }

    /// The clock is `m:ss`, including the cases a naive `{}:{}` gets wrong.
    #[test]
    fn the_clock_reads_as_minutes_and_seconds() {
        assert_eq!(clock(0.0), "0:00");
        assert_eq!(clock(9.9), "0:09");
        assert_eq!(clock(59.999), "0:59");
        assert_eq!(clock(60.0), "1:00");
        assert_eq!(clock(65.0), "1:05");
        assert_eq!(clock(605.0), "10:05");
        assert_eq!(clock(-1.0), "0:00", "a clock never runs backwards");
    }

    /// Losing focus releases every key the game thinks is held.
    ///
    /// It matters more here than in any earlier sample: every movement key is a
    /// *held* action, so a lost release walks the player into a wall for the
    /// rest of the session.
    #[test]
    fn losing_focus_releases_the_keys_the_game_still_thinks_are_down() {
        let mut engine = headless_loop();
        engine.frame().expect("a frame");
        engine.game_mut().key_event(KeyCode::KeyD, true);
        engine.held_keys.push(KeyCode::KeyD);
        // Two ticks, not one: a tick writes the velocity the *next* integration
        // step consumes, so one tick moves nothing at all.
        engine.game_mut().tick();
        engine.game_mut().tick();
        let moved = engine.game_mut().player;
        assert!(moved.x > 0.0, "the player never started moving: {moved:?}");

        engine.lose_focus();
        engine.game_mut().tick();
        engine.game_mut().tick();
        let after = engine.game_mut().player;
        engine.game_mut().tick();
        assert!(engine.held_keys.is_empty(), "the held list survived");
        assert!(engine.is_paused(), "focus loss must pause");
        assert_eq!(
            engine.game_mut().player,
            after,
            "the player kept walking after the key was released",
        );
    }

    /// **The whole focus path, through the shell**, rather than by calling
    /// `lose_focus` directly: a key held when the window goes away is released,
    /// and the game is paused.
    #[test]
    fn a_focus_loss_event_releases_the_held_keys_and_pauses() {
        let mut engine = scripted(&headless(64));
        let window = engine.window;
        engine.frame().expect("a frame");
        engine
            .shell
            .key_press(window, KeyCode::KeyD)
            .expect("the window is live");
        engine.frame().expect("a frame");
        assert_eq!(engine.held_keys, vec![KeyCode::KeyD]);

        engine
            .shell
            .set_focus(window, false)
            .expect("the window is live");
        engine.frame().expect("a frame");
        assert!(engine.held_keys.is_empty(), "the held list survived");
        assert!(engine.is_paused(), "the focus loss did not pause");

        // **Resumed before the walk is measured**, which is what makes this able
        // to fail. A paused loop runs no ticks, so a player that stands still
        // behind the pause says nothing about whether the key came up — a
        // `lose_focus` that merely emptied its own list would pass. The failure
        // being guarded is a player who walks forever, and that only shows once
        // the simulation is running again.
        tap(&mut engine, PAUSE_KEY);
        assert!(!engine.is_paused(), "escape did not resume");
        let x = engine.game_mut().player.x;
        run_frames(&mut engine, 6);
        assert_eq!(
            engine.game_mut().player.x,
            x,
            "the player kept walking after the window lost focus",
        );
    }

    // -----------------------------------------------------------------------
    // The start screen
    // -----------------------------------------------------------------------

    /// **The window opens on the start screen and the game does not play
    /// itself.**
    ///
    /// Sixty-four frames of it — a second of the loop's own clock, and twenty
    /// times the half-second the spawner's first enemy is owed at — with the
    /// whole [`RenderState`] compared before and after. Not the menu kind and
    /// not the state enum: a simulation that ran every line of its tick and
    /// mislabelled itself would satisfy both.
    ///
    /// The tick count is asserted to have moved, because "nothing changed" over
    /// a loop that ran no ticks is not a claim about anything.
    #[test]
    fn the_loop_opens_on_the_start_screen_and_nothing_moves() {
        let mut engine = at_the_title_screen(&headless(256));
        engine.frame().expect("a frame");
        assert_eq!(engine.menu_kind(), MenuKind::Start);
        assert_eq!(engine.game_mut().state, game::GameState::WaitingToStart);

        let mut before = RenderState::default();
        engine.game_mut().render_state(&mut before);
        let ticks = engine.ticks;
        run_frames(&mut engine, 64);

        let mut after = RenderState::default();
        engine.game_mut().render_state(&mut after);
        assert!(engine.ticks > ticks + 32, "the frames ran no ticks");
        assert_eq!(before, after, "the start screen played the game");
        assert_eq!(engine.game_mut().enemies_spawned(), 0, "the spawner ran");
        assert_eq!(engine.game_mut().elapsed, 0.0, "the run clock ran");
        assert_eq!(engine.menu_kind(), MenuKind::Start);

        // And it is a screen the player can see: titled, centred and drawn,
        // like the other three menus.
        let extent = engine.extent();
        let centre = engine
            .menu_layout()
            .expect("a waiting frame has a menu")
            .panel_centre();
        assert!(
            (centre.x - extent.0 as f32 / 2.0).abs() < 1.0
                && (centre.y - extent.1 as f32 / 2.0).abs() < 1.0,
            "the panel is at {centre:?} on a {extent:?} framebuffer",
        );
        assert!(
            !engine.gpu.menu_sprites().is_empty(),
            "the menu pass got nothing to draw",
        );
        let text = ui_text(&engine);
        assert!(
            text.iter().any(|line| line == "HORDE") && text.iter().any(|line| line == "PLAY"),
            "the start menu never reached the UI pass: {text:?}",
        );
    }

    /// **The key the button prints is the key that starts it**, through the
    /// shell, and the run really begins afterwards.
    ///
    /// `SPACE` is what the other three samples print on their start screens and
    /// what the browser gate presses. `RESTART_KEY` is `R`, so a button that
    /// worked and a hint that lied would look identical without this.
    #[test]
    fn the_key_the_start_button_prints_starts_the_run() {
        let mut engine = at_the_title_screen(&headless(256));
        engine.frame().expect("a frame");
        assert_eq!(engine.menu_kind(), MenuKind::Start);

        tap(&mut engine, KeyCode::Space);
        run_frames(&mut engine, 2);
        assert_eq!(engine.game_mut().state, game::GameState::Playing);
        assert_eq!(engine.menu_kind(), MenuKind::None);
        assert_eq!(
            engine.game_mut().run,
            1,
            "the key restarted rather than started"
        );

        let elapsed = engine.game_mut().elapsed;
        run_frames(&mut engine, 16);
        assert!(
            engine.game_mut().elapsed > elapsed,
            "the clock never started",
        );
    }

    /// **Clicking `PLAY` starts it too**, on the panel the player can see —
    /// through the layout the frame actually used, into the action map, into the
    /// simulation.
    #[test]
    fn clicking_play_starts_the_run() {
        let mut engine = at_the_title_screen(&headless(256));
        engine.frame().expect("a frame");
        let target = engine
            .menu_layout()
            .expect("a waiting frame has a menu")
            .items()[0];
        let over = (target.min + target.max) * 0.5;

        click(&mut engine, over);
        run_frames(&mut engine, 2);
        assert_eq!(engine.game_mut().state, game::GameState::Playing);
        assert_eq!(engine.menu_kind(), MenuKind::None);
    }

    /// **The start screen composes with the pause and with a focus loss.**
    ///
    /// Pause wins over it, as it wins over every other state; resuming goes back
    /// to the start screen rather than into a run; and a window that lost focus
    /// on the title screen has not started a game behind the player's back.
    #[test]
    fn the_start_screen_composes_with_pause_and_focus_loss() {
        let mut engine = at_the_title_screen(&headless(256));
        engine.frame().expect("a frame");
        assert_eq!(engine.menu_kind(), MenuKind::Start);

        tap(&mut engine, PAUSE_KEY);
        assert!(engine.is_paused(), "escape did not pause the title screen");
        assert_eq!(engine.menu_kind(), MenuKind::Paused);

        tap(&mut engine, PAUSE_KEY);
        assert!(!engine.is_paused(), "escape did not resume");
        run_frames(&mut engine, 2);
        assert_eq!(
            engine.menu_kind(),
            MenuKind::Start,
            "resuming from the title screen started the game",
        );

        let window = engine.window;
        engine
            .shell
            .set_focus(window, false)
            .expect("the window is live");
        engine.frame().expect("a frame");
        assert!(engine.is_paused(), "the focus loss did not pause");
        tap(&mut engine, PAUSE_KEY);
        run_frames(&mut engine, 8);
        assert_eq!(engine.game_mut().state, game::GameState::WaitingToStart);
        assert_eq!(engine.game_mut().elapsed, 0.0, "the run clock ran");
    }

    /// **`--prefill` starts its own run.**
    ///
    /// The scale fixture stages ten thousand enemies and every number in
    /// `docs/plan/sample/03-horde.md` was measured through it. Left on the title
    /// screen it would time a `run_tick` that returns on its second line and
    /// report the result as the cost of a full field.
    #[test]
    fn a_prefilled_run_does_not_wait_at_the_title_screen() {
        // `at_the_title_screen`, deliberately: `scripted` queues a start edge of
        // its own, and a test that started the run itself would pass on a
        // `--prefill` that had never learned to.
        let mut engine = at_the_title_screen(&Options {
            prefill: 64,
            ..headless(256)
        });
        run_frames(&mut engine, 4);
        assert_eq!(engine.game_mut().state, game::GameState::Playing);
        assert!(engine.game_mut().enemy_count() >= 64, "the field went away");
        assert!(engine.game_mut().elapsed > 0.0, "the clock never started");
    }

    // -----------------------------------------------------------------------
    // The menus
    // -----------------------------------------------------------------------

    /// **A menu is drawn, and it is centred**, measured through the layout the
    /// frame actually used.
    #[test]
    fn the_menu_a_frame_shows_is_centred_on_the_framebuffer() {
        let mut engine = scripted(&headless(64));
        engine.frame().expect("a frame");
        tap(&mut engine, PAUSE_KEY);

        let extent = engine.extent();
        let layout = engine.menu_layout().expect("a paused frame has a menu");
        let centre = layout.panel_centre();
        assert!(
            (centre.x - extent.0 as f32 / 2.0).abs() < 1.0
                && (centre.y - extent.1 as f32 / 2.0).abs() < 1.0,
            "the panel is at {centre:?} on a {extent:?} framebuffer",
        );
        assert!(
            !engine.gpu.menu_sprites().is_empty(),
            "the menu pass got nothing to draw",
        );
        assert!(
            ui_text(&engine).iter().any(|line| line == "PAUSED"),
            "the menu's title never reached the UI pass",
        );
    }

    /// **One menu per state**, driven through the loop rather than through
    /// `MenuKind::of` — which `crate::menu` already tests on its own.
    #[test]
    fn each_state_gets_its_own_menu_and_a_playing_frame_gets_none() {
        let mut engine = scripted(&headless(256));
        engine.frame().expect("a frame");
        // **The first frame is the title screen**, whatever this test's helper
        // queued: a loop's first frame establishes the clock's baseline and runs
        // no ticks, so nothing has consumed the start edge yet.
        assert_eq!(engine.menu_kind(), MenuKind::Start);
        run_frames(&mut engine, 1);
        assert_eq!(engine.menu_kind(), MenuKind::None);
        assert!(engine.menu_layout().is_none());

        // Exactly one threshold, so one button press closes the screen: 64
        // would cross four and the second panel would open behind the first.
        engine.game_mut().bank_xp(game::xp_for_next_level(1));
        run_frames(&mut engine, 2);
        assert_eq!(engine.menu_kind(), MenuKind::LevelUp);
        assert!(
            ui_text(&engine)
                .iter()
                .any(|line| line.starts_with("LEVEL ")),
            "{:?}",
            ui_text(&engine),
        );

        // Take one, and the menu goes away with the state.
        tap(&mut engine, KeyCode::Digit1);
        run_frames(&mut engine, 2);
        assert_eq!(engine.menu_kind(), MenuKind::None);

        engine.game_mut().set_player_hp(0.000_1);
        engine.game_mut().stage_player(DVec3::ZERO);
        engine
            .game_mut()
            .stage_enemy(game::EnemyKind::Grunt, DVec3::ZERO);
        run_frames(&mut engine, 4);
        assert_eq!(engine.menu_kind(), MenuKind::GameOver);
    }

    /// **The level-up menu's buttons apply the upgrade**, through the whole
    /// path: a click on the panel the player can see, into the action map, into
    /// the simulation's stats.
    ///
    /// The assertion is the *effect* — the stat the button names changed — not
    /// that a menu closed.
    #[test]
    fn clicking_a_level_up_button_applies_that_upgrade() {
        let mut engine = scripted(&headless(256));
        engine.frame().expect("a frame");
        engine.game_mut().bank_xp(game::xp_for_next_level(1));
        run_frames(&mut engine, 2);
        assert_eq!(engine.menu_kind(), MenuKind::LevelUp);

        let before = engine.game_mut().stats();
        let offer = engine.game_mut().offer().expect("a level-up has an offer");
        let layout = engine.menu_layout().expect("a level-up frame has a menu");
        let target = layout.items()[1];
        let over = (target.min + target.max) * 0.5;

        click(&mut engine, over);
        run_frames(&mut engine, 2);

        let after = engine.game_mut().stats();
        assert_ne!(after, before, "the click changed nothing at all");
        assert_eq!(
            after,
            expected_after(before, offer[1]),
            "the click applied something other than {:?}",
            offer[1],
        );
        assert_eq!(engine.game_mut().state, GameState::Playing);
    }

    /// `before`, with `upgrade` applied — the loop-side mirror of
    /// `game::apply_upgrade`, so the test above names an *effect* rather than
    /// re-reading the simulation's own arithmetic.
    fn expected_after(before: game::Stats, upgrade: game::Upgrade) -> game::Stats {
        let mut after = before;
        match upgrade {
            game::Upgrade::RapidFire => {
                after.fire_cooldown = (before.fire_cooldown * 0.85).max(game::FIRE_COOLDOWN_FLOOR);
            }
            game::Upgrade::HeavyBolts => after.bolt_damage += 2.0,
            game::Upgrade::SwiftBoots => after.player_speed += 0.6,
            game::Upgrade::LongBarrel => after.weapon_range += 2.0,
            game::Upgrade::Vitality => after.max_hp += 25.0,
            game::Upgrade::Magnet => after.pickup_radius += 1.0,
        }
        after
    }

    // -----------------------------------------------------------------------
    // The scale knobs
    // -----------------------------------------------------------------------

    /// **`--prefill` reaches the field, the cap and the first frame's sprites.**
    ///
    /// Every number in `docs/plan/sample/03-horde.md` was taken through this
    /// flag, so a flag that parsed and did nothing — or that staged the field
    /// but left the views empty, which is exactly what happens if `stage_field`
    /// forgets to refresh them — would make all of them measurements of an empty
    /// arena.
    #[test]
    fn a_prefilled_run_draws_a_crowd_on_its_very_first_frame() {
        let options = Options {
            prefill: 2_000,
            ..headless(4)
        };
        // The cap is raised to fit, or 1 500 of the 2 000 would be silently
        // dropped and the measurement would be of the wrong field.
        assert_eq!(options.setup().max_enemies, 2_000);

        let mut engine = scripted(&options);
        engine.frame().expect("a frame");
        assert_eq!(
            engine.game_mut().enemy_count(),
            2_000,
            "the prefill did not reach the simulation",
        );
        let stats = engine.gpu.scene_stats();
        assert_eq!(stats.field, 2_000, "the renderer was handed an empty field");
        assert!(
            stats.culled > 0 && stats.drawn > 1,
            "a field of 2000 over a 96x72 arena should be partly on screen: {stats:?}",
        );
        assert_eq!(stats.batches, 1, "one sheet, one batch");

        // A run with no prefill draws the handful of enemies a fifteenth of a
        // second produces, which is what says the assertion above is about the
        // flag and not about the game.
        let mut plain = scripted(&headless(4));
        plain.frame().expect("a frame");
        assert!(
            plain.gpu.scene_stats().field < 10,
            "an unprefilled run already had a crowd: {:?}",
            plain.gpu.scene_stats(),
        );
    }

    /// **The scene section reaches the panel**, so the numbers this sample's
    /// whole claim rests on are readable in the running game rather than only
    /// from a test.
    #[test]
    fn the_debug_panel_carries_this_samples_own_scene_section() {
        let mut engine = scripted(&Options {
            debug_overlay: Some(false),
            ..headless(8)
        });
        engine.frame().expect("a frame");
        assert!(
            !ui_text(&engine).iter().any(|line| line == "scene"),
            "a hidden panel gathered a section",
        );

        tap(&mut engine, DEBUG_OVERLAY_KEY);
        let text = ui_text(&engine);
        assert!(
            text.iter().any(|line| line == "scene"),
            "the scene section never reached the UI pass: {text:?}",
        );
        for row in ["field", "culled", "drawn", "batches"] {
            assert!(
                text.iter().any(|line| line == row),
                "the scene section has no {row} row: {text:?}",
            );
        }
        // The frame-timing module is still there beside it, or this replaced
        // the panel rather than adding to it.
        assert!(text.iter().any(|line| line == "frame"), "{text:?}");
    }

    /// **A click that focuses the window does not fire a button.**
    ///
    /// The corner of the screen is over no menu item, so the click that brings a
    /// paused window back leaves it paused — the same guard the other three
    /// samples carry, because the geometry is per-sample even though the rule is
    /// not.
    #[test]
    fn a_focusing_click_off_every_button_leaves_the_game_paused() {
        let mut engine = scripted(&headless(64));
        engine.frame().expect("a frame");
        tap(&mut engine, PAUSE_KEY);
        assert!(engine.is_paused());

        let layout = engine.menu_layout().expect("a menu").clone();
        assert!(
            !layout
                .items()
                .iter()
                .any(|item| item.min.x <= 3.0 && item.min.y <= 3.0),
            "the menu reaches the corner of the screen",
        );
        click(&mut engine, Vec2::new(3.0, 3.0));
        assert!(engine.is_paused(), "a click in the corner resumed the game");

        // …and the centre of the first item really does resume, or the check
        // above passes on a menu nothing can click.
        let target = layout.items()[0];
        click(&mut engine, (target.min + target.max) * 0.5);
        assert!(!engine.is_paused(), "RESUME did not resume");
    }
}
