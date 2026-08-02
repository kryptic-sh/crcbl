//! The asteroids engine loop: window, fixed-timestep accumulator, one draw.
//!
//! Shape matches `apps/flappy/src/app.rs` and `apps/breakout/src/app.rs` — same
//! clock, same event loop, same accumulator, same three menus, same pause,
//! fullscreen and focus handling. **This is the third copy of each of those**,
//! and `docs/backlog.md` says so: the pump's key branch, `lose_focus`, the F11
//! toggle, `Loop::paused` and the pointer's press-capture bookkeeping are the
//! same code in three files. So, now, are [`PendingLoop`] and
//! [`Loop::set_frame_step`] — the polled browser start-up below — and
//! `crate::web`, which is S1B finding 2 written out a third time. See the S2
//! findings note in `docs/plan/ROADMAP.md`.
//!
//! # What is this sample's own
//!
//! **The alpha.** Every other sample draws the last tick's state and is right to
//! — a pipe and a paddle are the same picture at 60 Hz and at 144. This one
//! turns things, so the frame is asked how far through a tick it sits and the
//! rotations are interpolated across it. See [`render_alpha`] and
//! `game::lerp_angle`.
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
use crcbl::ui::draw_list::DrawList;
use crcbl::ui::{DebugOverlay, PointerInput};
use glam::Vec2;

use crate::game::{self, Game, GameState, RenderState};
use crate::gpu::{Gpu, PendingGpu};
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

/// The keys a menu takes for itself while one is on screen.
///
/// The same three in every sample, for the reason F3, Escape and F11 are the
/// same in every sample. Two of them are free here; **ArrowUp is not** — it is
/// the second binding of `game.rs`'s thrust action, beside `KeyW`, and a panel
/// on screen shadows it. `crate::menu`'s header carries the argument; the short
/// version is that `KeyW` still thrusts and that a menu is only ever up on a
/// frame the game is not being flown on.
///
/// They are consumed **only while a menu is showing**.
pub const MENU_UP_KEY: KeyCode = KeyCode::ArrowUp;
/// See [`MENU_UP_KEY`].
pub const MENU_DOWN_KEY: KeyCode = KeyCode::ArrowDown;
/// See [`MENU_UP_KEY`].
pub const MENU_ACTIVATE_KEY: KeyCode = KeyCode::Enter;

/// The key a menu's `FLY` and `TRY AGAIN` buttons stand for.
///
/// Fired as a real key event rather than by calling into [`Game`], because
/// starting and restarting a game is the simulation's business and the
/// simulation is driven by its action map.
const FIRE_KEY: KeyCode = KeyCode::Space;

/// How far through a tick this frame sits, for the renderer's interpolation.
///
/// **One is not the clock's answer while the game is paused, and that is the
/// whole of this function.** A paused frame drains the accumulator without
/// simulating, so the clock's alpha keeps moving while the two angles it would
/// interpolate between stand still — and every rock on a paused field would
/// rock back and forth between the last two ticks it ran. One means "the newest
/// state", which is what a stopped game should show.
#[must_use]
pub fn render_alpha(paused: bool, clock_alpha: f32) -> f32 {
    if paused { 1.0 } else { clock_alpha }
}

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
    /// The start, pause and game-over menus, and which is on screen.
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
    ) -> Result<Self, AsteroidsError> {
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
            menus: menu::menus(),
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
                                    keyboard_action =
                                        menus.activate().and_then(MenuAction::from_id);
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
        let pointer_action = self
            .menus
            .point(
                self.gpu.extent(),
                self.gpu.atlas(),
                PointerInput {
                    // A pointer that has never been in the window is nowhere, not at
                    // the origin — which is a real pixel, inside the HUD.
                    pos: self.pointer.unwrap_or(Vec2::splat(f32::NEG_INFINITY)),
                    down: pointer_down,
                    released: pointer_released,
                },
            )
            .and_then(MenuAction::from_id);
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
        if self.paused {
            while self.frame_clock.consume_tick() {}
        } else {
            while self.frame_clock.consume_tick() {
                self.ticks += 1;
                self.game.tick();
            }
        }

        self.game.render_state(&mut self.render_state);
        // **After the accumulator has been drained**, which is what
        // `FrameClock::alpha` asks for: called before, it saturates just under
        // one rather than reporting the fraction of a tick that is left.
        let alpha = render_alpha(self.paused, self.frame_clock.alpha());
        self.gpu.set_world(&self.render_state, alpha);

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
    /// [`AsteroidsError`] if the shell refused a display-mode change.
    fn apply(&mut self, action: MenuAction) -> Result<(), AsteroidsError> {
        match action {
            MenuAction::Resume => {
                if self.paused {
                    self.paused = false;
                    log::info!("game resumed");
                }
            }
            // A real key event rather than a call into `Game`: starting a game is
            // the simulation's business and the simulation is driven by its
            // action map. The release is queued straight after the press because
            // the trigger is an *edge* — a press with no release leaves the
            // action held, which in this game is a magazine emptied into whatever
            // the ship happens to be pointing at.
            MenuAction::Fire => {
                self.game.key_event(FIRE_KEY, true);
                self.game.key_event(FIRE_KEY, false);
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

// ---- polled start-up --------------------------------------------------------

/// The largest step [`Loop::set_frame_step`] will accept.
///
/// A tab backgrounded for a minute reports a one-minute `requestAnimationFrame`
/// delta on the frame it comes back. Handing that to a fixed-timestep
/// accumulator asks for 3600 ticks in one frame, which the user reads as a
/// crash — and in this game, as a magazine emptied and a field of rocks
/// teleported across the screen.
pub const MAX_FRAME_STEP: Duration = Duration::from_millis(64);

/// Creates the one window, and puts the shell's event clock on the engine's.
fn open_the_window<S: Shell + ?Sized>(
    shell: &mut S,
    clock_source: &Clock,
) -> Result<WindowId, AsteroidsError> {
    log::info!(
        "shell: {} backend, caps {:?}",
        shell.backend(),
        shell.caps()
    );
    shell.align_event_clock(clock_source.elapsed());
    Ok(shell.create_window(&WindowDesc {
        title: "Asteroids",
        app_id: "sh.kryptic.crcbl.asteroids",
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
    /// [`AsteroidsError`] if the shell refused the window.
    pub fn request(
        mut shell: Box<S>,
        options: &Options,
        clock_source: Clock,
    ) -> Result<Self, AsteroidsError> {
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
    /// [`AsteroidsError`] if the window went away before it had a size, if the
    /// device request failed, or if the game could not be built.
    pub fn poll(&mut self) -> Result<Option<Loop<S>>, AsteroidsError> {
        let Some(shell) = self.shell.as_mut() else {
            return Err(AsteroidsError::Gpu(GpuError::Unusable(
                "this asteroids loop was already started",
            )));
        };

        let mut pending = Pending::default();
        shell.pump(&mut |event| pending.observe(&event));
        self.events += pending.count;
        if pending.destroyed {
            return Err(AsteroidsError::Shell(ShellError::invalid_window(
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
            BootStage::Done => Err(AsteroidsError::Gpu(GpuError::Unusable(
                "this asteroids loop was already started",
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
            headless: true,
            backend: Some(GpuBackend::Null),
            frames: Some(frames),
            ..Options::default()
        }
    }

    fn headless_loop() -> Loop<dyn Shell> {
        Loop::start(&headless(8)).expect("a headless loop always starts")
    }

    /// A loop on a shell the test can post events to.
    fn scripted(options: &Options) -> Loop<HeadlessShell> {
        Loop::with_shell(Box::new(HeadlessShell::new()), options).expect("headless always starts")
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
        engine.paused = true;
        let before = engine.ticks;
        for _ in 0..3 {
            engine.frame().expect("a frame");
        }
        assert_eq!(engine.ticks, before, "a paused frame ran a tick");
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
    /// lost the rocks and kept the ship would pass the weaker version.
    #[test]
    fn the_frame_hands_the_field_to_the_sprite_pass_and_the_hud_to_the_ui_pass() {
        let mut engine = scripted(&headless(4));
        run_frames(&mut engine, 1);

        let rocks = engine.render_state.rocks.len();
        assert_eq!(
            rocks,
            game::wave_rocks(0) as usize,
            "the first wave should be on the field",
        );
        assert!(engine.render_state.ship_alive);
        let sprites = engine.gpu.scene_sprites();
        assert_eq!(
            sprites.len(),
            rocks + 1,
            "every rock and the ship, and nothing else",
        );

        // Nothing the game draws is a UI rectangle any more: the HUD panel is,
        // and the menu's own geometry, and that is all.
        let outlines = engine
            .gpu
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
        let window = engine.window;
        run_frames(&mut engine, 2);
        engine
            .shell
            .key_press(window, PAUSE_KEY)
            .expect("the window is live");
        run_frames(&mut engine, 2);
        assert!(engine.is_paused());
        assert_eq!(engine.gpu.alpha_for_test(), 1.0);
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
        let window = engine.window;
        // Start the game, then hold left.
        engine
            .shell
            .key_press(window, KeyCode::Space)
            .expect("the window is live");
        run_frames(&mut engine, 2);
        engine
            .shell
            .key_release(window, KeyCode::Space)
            .expect("the window is live");
        run_frames(&mut engine, 2);
        assert_eq!(engine.game.state, GameState::Playing);

        let ship_rotation = |engine: &mut Loop<HeadlessShell>| -> f32 {
            let sprites = engine.gpu.scene_sprites();
            sprites.last().expect("the ship is drawn last").rotation
        };
        let before = ship_rotation(&mut engine);

        engine
            .shell
            .key_press(window, KeyCode::ArrowLeft)
            .expect("the window is live");
        run_frames(&mut engine, 20);
        let after = ship_rotation(&mut engine);
        assert!(
            (after - before).abs() > 0.1,
            "twenty ticks of a held turn moved the sprite from {before} to {after}",
        );
        assert!(
            (f64::from(after) - engine.game.ship_heading).abs() < 0.2,
            "the sprite is at {after} and the ship is flying {}",
            engine.game.ship_heading,
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

        let sprites = engine.gpu.menu_sprites();
        // The scrim, the window frame's nine quads, and nine per button.
        assert_eq!(sprites.len(), 1 + 9 + 9 * 3, "{}", sprites.len());

        // **Centred, measured on what the menu pass was actually handed** rather
        // than on a layout the test recomputes. `crcbl_render::menu_camera` puts
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

        let scale = layout.style().scale;
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

    /// **A game being played draws no menu at all**, and the menu pass is handed
    /// nothing — which is what makes it free rather than cheap.
    #[test]
    fn a_game_in_play_draws_no_menu() {
        let mut engine = scripted(&headless(60));
        let window = engine.window;
        engine
            .shell
            .key_press(window, KeyCode::Space)
            .expect("the window is live");
        run_frames(&mut engine, 10);
        assert_eq!(engine.game.state, GameState::Playing);
        assert_eq!(engine.menu_kind(), MenuKind::None);
        assert!(
            engine.gpu.menu_sprites().is_empty(),
            "a playing frame submitted {} menu sprites",
            engine.gpu.menu_sprites().len(),
        );
        engine.finish(ExitReason::FrameBudget).expect("teardown");
    }

    /// **One menu per state, and only the state's own**, through the real loop:
    /// the start menu gives way to none when the game starts, and to the pause
    /// menu the moment it is paused.
    #[test]
    fn each_state_draws_its_own_menu_and_no_other() {
        let mut engine = scripted(&headless(60));
        let window = engine.window;
        run_frames(&mut engine, 2);
        assert_eq!(engine.menu_kind(), MenuKind::Start);
        assert!(!ui_text(&engine).iter().any(|t| t == "PAUSED"));

        engine
            .shell
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
            .shell
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
        let window = engine.window;
        run_frames(&mut engine, 2);

        engine
            .shell
            .key_press(window, PAUSE_KEY)
            .expect("the window is live");
        engine.frame().expect("a frame");
        assert!(engine.is_paused());
        assert_eq!(engine.menu_kind(), MenuKind::Paused);

        // Press and release, because the commit fires on the *release* — the
        // pressed frame of the skin has to be on screen while the key is down.
        engine
            .shell
            .key_press(window, MENU_ACTIVATE_KEY)
            .expect("the window is live");
        engine.frame().expect("a frame");
        assert!(engine.is_paused(), "the press alone must not fire it");

        engine
            .shell
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
        let window = engine.window;
        run_frames(&mut engine, 2);
        engine
            .shell
            .key_press(window, PAUSE_KEY)
            .expect("the window is live");
        engine.frame().expect("a frame");
        assert!(engine.is_paused());

        let corner = glam::Vec2::new(3.0, 3.0);
        let item = engine.menu_layout().expect("a menu is showing").items()[0];
        assert!(
            corner.x < item.min.x || corner.y < item.min.y,
            "the corner is inside a button, so the test below proves nothing",
        );
        let at = (item.min + item.max) * 0.5;

        let click = |engine: &mut Loop<HeadlessShell>, pos: glam::Vec2| {
            let point = PhysicalPoint::new(f64::from(pos.x), f64::from(pos.y));
            engine
                .shell
                .button(
                    window,
                    PointerButton::Left,
                    PointerState::Pressed,
                    Some(point),
                )
                .expect("the window is live");
            engine.frame().expect("a frame");
            engine
                .shell
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
        let window = engine.window;
        run_frames(&mut engine, 2);
        assert_eq!(engine.menu_kind(), MenuKind::Start);

        engine
            .shell
            .key_press(window, KeyCode::Space)
            .expect("the window is live");
        engine
            .shell
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
        let window = engine.window;
        run_frames(&mut engine, 2);
        assert_eq!(engine.menu_kind(), MenuKind::Start);

        engine
            .shell
            .key_press(window, MENU_ACTIVATE_KEY)
            .expect("the window is live");
        engine
            .shell
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
        let window = engine.window;
        run_frames(&mut engine, 2);

        engine
            .shell
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
            .shell
            .set_focus(window, true)
            .expect("the window is live");
        let at = PhysicalPoint::new(f64::from(corner.x), f64::from(corner.y));
        for state in [PointerState::Pressed, PointerState::Released] {
            engine
                .shell
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
        let window = engine.window;
        engine
            .shell
            .key_press(window, KeyCode::Space)
            .expect("the window is live");
        run_frames(&mut engine, 4);
        assert_eq!(engine.game.state, GameState::Playing);

        engine
            .shell
            .key_press(window, KeyCode::ArrowLeft)
            .expect("the window is live");
        run_frames(&mut engine, 10);
        assert!(
            engine.held_keys.contains(&KeyCode::ArrowLeft),
            "the pump did not record the held key, so the test below is vacuous",
        );
        let turning = engine.game.ship_heading;

        // Focus goes away, and no release for ArrowLeft ever arrives — which is
        // exactly what every platform does.
        engine
            .shell
            .set_focus(window, false)
            .expect("the window is live");
        engine.frame().expect("a frame");
        assert!(engine.is_paused(), "focus loss must pause");
        assert!(engine.held_keys.is_empty(), "the held list survived");

        // Un-pause without ever releasing the key, and the ship must be still.
        engine
            .shell
            .key_press(window, PAUSE_KEY)
            .expect("the window is live");
        run_frames(&mut engine, 30);
        assert!(!engine.is_paused());
        assert!(
            (engine.game.ship_heading - turning).abs() < 1e-9,
            "the ship kept turning after focus was lost: {turning} → {}",
            engine.game.ship_heading,
        );
        engine.finish(ExitReason::FrameBudget).expect("teardown");
    }
}
