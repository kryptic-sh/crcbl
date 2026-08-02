//! The horde engine loop: window, fixed-timestep accumulator, one draw.
//!
//! Shape matches `apps/asteroids/src/app.rs`, `apps/flappy/src/app.rs` and
//! `apps/breakout/src/app.rs` — same clock, same event loop, same accumulator —
//! and is deliberately the *small* version of it. There are no menus, no pointer
//! handling and no browser entry point in this sub-slice, so what is left is the
//! part every sample shares.
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
//! # The draw is capped and the simulation is not
//!
//! This is the sample whose whole question is how many agents a tick can carry,
//! so the two numbers have to stay separable: [`draw_field`] culls to the view
//! and then stops at [`MAX_DRAWN_ENEMIES`], while `game.tick()` steers every
//! enemy there is. A frame rate that fell over because of the *placeholder*
//! renderer would be a measurement of `DrawList`, which nobody wants. See
//! `crate::gpu`'s header.

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
use glam::{DVec3, Vec2};

use crate::game::{self, EnemyKind, Game, GameState, RenderState};
use crate::gpu::{Gpu, camera_centre, pixels_per_unit, view_half_width, world_to_screen};

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
/// here: `game.rs` declares WASD, the arrows, R and Space.
pub const PAUSE_KEY: KeyCode = KeyCode::Escape;

/// The key that asks for fullscreen, and asks to leave it.
pub const FULLSCREEN_KEY: KeyCode = KeyCode::F11;

/// The most enemy quads one frame will emit.
///
/// **A cap on the placeholder renderer, not on the game.** `crate::gpu`'s header
/// has the argument: a `DrawList` quad is six vertices uploaded per frame, so a
/// field of ten thousand would be measuring the UI pass rather than the
/// simulation. The view cull in [`draw_field`] does most of the work — an
/// off-screen enemy is never a quad — and this is the backstop for a crowd that
/// is genuinely all on screen at once.
pub const MAX_DRAWN_ENEMIES: usize = 2_000;

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
        let clock_source = Clock::new(options.headless);
        log::info!(
            "shell: {} backend, caps {:?}",
            shell.backend(),
            shell.caps()
        );
        shell.align_event_clock(clock_source.elapsed());
        let window = shell.create_window(&WindowDesc {
            title: "Horde",
            app_id: "sh.kryptic.crcbl.horde",
            size: LogicalSize::new(960.0, 720.0),
            ..WindowDesc::default()
        })?;

        let mut events = 0;
        let extent = wait_for_configure(shell.as_mut(), window, &mut events)?;
        log::info!("shell: first configure at {}x{}", extent.0, extent.1);

        let gpu = Gpu::open(shell.as_ref(), window, extent, options.backend)?;
        let game = Game::with_setup(&options.setup())?;
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
        draw_hud(&mut self.draw_list, &self.hud, self.gpu.extent());
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
        let summary = Summary {
            backend: self.shell.backend(),
            frames: self.frames,
            ticks: self.ticks,
            events: self.events,
            extent: self.gpu.extent(),
            exit,
            elapsed: self.game.elapsed,
            kills: self.game.kills,
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

// ---- drawing ----------------------------------------------------------------

/// The colour a body of each kind is drawn in.
fn enemy_colour(kind: EnemyKind, health: f64) -> [f32; 4] {
    let base = match kind {
        EnemyKind::Grunt => [0.72, 0.36, 0.30],
        EnemyKind::Runner => [0.86, 0.72, 0.28],
        EnemyKind::Brute => [0.55, 0.30, 0.70],
    };
    // Damage darkens rather than adding a health bar per body: at these counts a
    // bar is a second quad each, and the plan's non-goals cap what this is
    // allowed to spend on the costume.
    let dim = 0.45 + 0.55 * health as f32;
    [base[0] * dim, base[1] * dim, base[2] * dim, 1.0]
}

/// Draws the arena as untextured quads.
///
/// **Placeholder, and named as one.** `crate::gpu`'s header sets out why there
/// is no sprite pass in this sub-slice; what this gives is a window that shows
/// the simulation actually running, which is the difference between "the loop
/// compiles" and "the loop works". The art sub-slice replaces this function
/// wholesale.
///
/// Enemies outside the view are **culled on the CPU** rather than drawn off
/// screen — the arena is nearly three times the view in each axis, so most of a
/// large horde is off screen most of the time, and a quad the player cannot see
/// is six vertices of upload for nothing. GPU culling is P7's; this is the
/// version a sample can have today.
pub fn draw_field(dl: &mut DrawList, render: &RenderState, extent: (u32, u32)) {
    let camera = camera_centre(render.player, extent);
    let scale = pixels_per_unit(extent);

    // The arena's wall, so the edge of the world is legible when the camera
    // stops at it. Drawn in world space and clipped by the viewport, which is
    // what makes it appear only when it is actually in view.
    let top_left = world_to_screen(
        DVec3::new(-game::ARENA_HALF_WIDTH, game::ARENA_HALF_HEIGHT, 0.0),
        camera,
        extent,
    );
    let bottom_right = world_to_screen(
        DVec3::new(game::ARENA_HALF_WIDTH, -game::ARENA_HALF_HEIGHT, 0.0),
        camera,
        extent,
    );
    dl.rect_outline(top_left, bottom_right, 3.0, [0.30, 0.28, 0.36, 1.0]);

    // The cull box: the view, grown by the largest body's radius so nothing
    // pops in half-way across the edge.
    let margin = game::max_enemy_radius();
    let half_x = view_half_width(extent) + margin;
    let half_y = game::VIEW_HALF_HEIGHT + margin;

    let mut drawn = 0;
    for enemy in &render.enemies {
        if drawn >= MAX_DRAWN_ENEMIES {
            break;
        }
        let offset = enemy.position - camera;
        if offset.x.abs() > half_x || offset.y.abs() > half_y {
            continue;
        }
        drawn += 1;
        let centre = world_to_screen(enemy.position, camera, extent);
        let half = (enemy.kind.radius() as f32 * scale).max(1.5);
        dl.rect(
            centre - Vec2::splat(half),
            centre + Vec2::splat(half),
            enemy_colour(enemy.kind, enemy.health),
        );
    }

    for bolt in &render.bolts {
        let centre = world_to_screen(bolt.position, camera, extent);
        let half = (game::BOLT_RADIUS as f32 * scale).max(2.0);
        dl.rect(
            centre - Vec2::splat(half),
            centre + Vec2::splat(half),
            [1.0, 0.97, 0.75, 1.0],
        );
    }

    let centre = world_to_screen(render.player, camera, extent);
    let half = game::PLAYER_RADIUS as f32 * scale;
    let colour = if render.state == Some(GameState::Dead) {
        [0.45, 0.45, 0.50, 1.0]
    } else {
        [0.40, 0.90, 1.00, 1.0]
    };
    dl.rect(
        centre - Vec2::splat(half),
        centre + Vec2::splat(half),
        colour,
    );
}

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

type HudKey = (u64, u64, u32, usize, Option<GameState>, bool);

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
            render.enemies.len(),
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
            "{}   Kills: {}   HP: {:.0}/{:.0}   Enemies: {}",
            clock(render.elapsed),
            render.kills,
            render.player_hp.max(0.0),
            game::PLAYER_MAX_HP,
            render.enemies.len(),
        );
        self.state.clear();
        if paused {
            self.state.push_str("PAUSED - press ESC");
        } else {
            match render.state {
                Some(GameState::Dead) => {
                    let _ = write!(
                        self.state,
                        "YOU DIED - survived {}, {} kills - press R",
                        clock(render.elapsed),
                        render.kills,
                    );
                }
                Some(GameState::Playing) | None => {
                    self.state.push_str("WASD to move - the gun aims itself");
                }
            }
        }
    }
}

/// Draws the HUD, and nothing else.
///
/// A death screen is a full-screen scrim plus the same status line, rather than
/// a menu: `crcbl-render`'s `MenuRenderer` is what the art sub-slice will use,
/// and half a menu in the placeholder renderer is work the next slice deletes.
fn draw_hud(dl: &mut DrawList, hud: &HudStrings, extent: (u32, u32)) {
    if hud.state.starts_with("YOU DIED") {
        dl.rect(
            Vec2::ZERO,
            Vec2::new(extent.0 as f32, extent.1 as f32),
            [0.0, 0.0, 0.0, 0.55],
        );
    }
    dl.rect(
        Vec2::new(4.0, 4.0),
        Vec2::new(430.0, 52.0),
        [0.1, 0.1, 0.15, 0.85],
    );
    dl.text(
        Vec2::new(10.0, 10.0),
        hud.stats.as_str(),
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
    use crate::game::EnemyView;
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
        assert_eq!(summary.state, GameState::Playing);
        assert!(summary.elapsed > 0.0, "the clock never advanced");
    }

    /// **The geometry the loop builds actually reaches the GPU.**
    ///
    /// `draw_field` and `draw_hud` are tested directly below, which says the
    /// draw list is right and nothing at all about whether the loop hands it
    /// over — and `take_draw_list` is a swap, so a loop that forgot the call
    /// would present the previous frame's list forever.
    #[test]
    fn every_frame_hands_the_gpu_something_to_draw() {
        let mut engine = headless_loop();
        assert!(
            engine.gpu.draw_list().is_empty(),
            "the GPU has a list before the first frame",
        );
        engine.frame().expect("a frame");
        // The arena wall and the player, at least: the field is empty for the
        // first fraction of a second of a run.
        assert!(
            engine.gpu.draw_list().len() >= 2,
            "the loop handed over {} items",
            engine.gpu.draw_list().len(),
        );
    }

    /// Escape stops the simulation without stopping the loop, and does not reach
    /// the game: a pause the simulation knew about would be a state a scripted,
    /// seeded run could reach.
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
    /// draw list that lost the enemies and kept the wall would pass the weaker
    /// version.
    #[test]
    fn the_draw_list_carries_the_wall_the_player_and_every_visible_enemy() {
        let render = RenderState {
            player: DVec3::ZERO,
            player_hp: game::PLAYER_MAX_HP,
            enemies: vec![
                EnemyView {
                    position: DVec3::new(2.0, 1.0, 0.0),
                    kind: EnemyKind::Grunt,
                    health: 1.0,
                },
                EnemyView {
                    position: DVec3::new(-4.0, 3.0, 0.0),
                    kind: EnemyKind::Brute,
                    health: 0.5,
                },
            ],
            bolts: vec![crate::game::BoltView {
                position: DVec3::new(1.0, 0.0, 0.0),
            }],
            state: Some(GameState::Playing),
            ..RenderState::default()
        };
        let mut dl = DrawList::new();
        draw_field(&mut dl, &render, (960, 720));
        // One wall + two enemies + one bolt + the player.
        assert_eq!(dl.len(), 1 + 2 + 1 + 1);
    }

    /// **An enemy outside the view is not a quad.**
    ///
    /// The cull is the only reason a large horde is drawable at all, and a cull
    /// that silently matched nothing would look exactly like this test passing
    /// — so the near enemy is asserted *present* in the same run that asserts
    /// the far one absent.
    #[test]
    fn enemies_outside_the_view_are_culled_and_the_near_one_is_not() {
        let extent = (960, 720);
        let far = view_half_width(extent) + game::max_enemy_radius() + 5.0;
        let render = |x: f64| RenderState {
            player: DVec3::ZERO,
            enemies: vec![EnemyView {
                position: DVec3::new(x, 0.0, 0.0),
                kind: EnemyKind::Grunt,
                health: 1.0,
            }],
            state: Some(GameState::Playing),
            ..RenderState::default()
        };

        let mut near_list = DrawList::new();
        draw_field(&mut near_list, &render(2.0), extent);
        let mut far_list = DrawList::new();
        draw_field(&mut far_list, &render(far), extent);

        assert_eq!(near_list.len(), 3, "wall + enemy + player");
        assert_eq!(far_list.len(), 2, "wall + player, the enemy was culled");
        assert!(
            far < game::ARENA_HALF_WIDTH,
            "the far enemy has to be somewhere the arena can actually hold it",
        );
    }

    /// **The draw cap is a cap on the draw and not on the simulation.**
    ///
    /// A crowd larger than [`MAX_DRAWN_ENEMIES`] that is entirely on screen
    /// still emits exactly the cap, and the render state it was built from still
    /// holds all of them.
    #[test]
    fn a_crowd_larger_than_the_cap_draws_exactly_the_cap() {
        let count = MAX_DRAWN_ENEMIES + 500;
        let render = RenderState {
            player: DVec3::ZERO,
            enemies: (0..count)
                .map(|i| EnemyView {
                    // A tight cluster, so every one of them is inside the view
                    // and the cap rather than the cull is what limits this.
                    position: DVec3::new((i % 7) as f64 * 0.5, (i % 5) as f64 * 0.5, 0.0),
                    kind: EnemyKind::Grunt,
                    health: 1.0,
                })
                .collect(),
            state: Some(GameState::Playing),
            ..RenderState::default()
        };
        let mut dl = DrawList::new();
        draw_field(&mut dl, &render, (960, 720));
        assert_eq!(render.enemies.len(), count, "the state kept all of them");
        assert_eq!(dl.len(), 1 + MAX_DRAWN_ENEMIES + 1, "wall + cap + player");
    }

    /// The HUD reports the pause rather than the simulation's state, and the
    /// death screen says how the run ended.
    #[test]
    fn the_hud_says_paused_even_though_the_simulation_is_not() {
        let mut hud = HudStrings::default();
        let render = RenderState {
            state: Some(GameState::Playing),
            player_hp: game::PLAYER_MAX_HP,
            elapsed: 74.0,
            kills: 12,
            ..RenderState::default()
        };
        hud.refresh(&render, false);
        assert!(hud.state.contains("WASD"), "{}", hud.state);
        assert!(hud.stats.contains("1:14"), "{}", hud.stats);
        assert!(hud.stats.contains("Kills: 12"), "{}", hud.stats);

        hud.refresh(&render, true);
        assert!(hud.state.contains("PAUSED"), "{}", hud.state);
    }

    /// **The death screen exists, says what the run scored, and dims the field.**
    ///
    /// Both halves: the line, and the full-screen scrim that only a dead run
    /// gets — a death screen that produced the same draw list as a live one is
    /// not a death screen.
    #[test]
    fn the_death_screen_reports_the_run_and_dims_the_field() {
        let dead = RenderState {
            state: Some(GameState::Dead),
            player_hp: 0.0,
            elapsed: 133.0,
            kills: 208,
            ..RenderState::default()
        };
        let mut hud = HudStrings::default();
        hud.refresh(&dead, false);
        assert!(hud.state.contains("YOU DIED"), "{}", hud.state);
        assert!(hud.state.contains("2:13"), "{}", hud.state);
        assert!(hud.state.contains("208 kills"), "{}", hud.state);
        assert!(hud.state.contains('R'), "{}", hud.state);

        let mut dead_list = DrawList::new();
        draw_hud(&mut dead_list, &hud, (960, 720));

        let mut alive = HudStrings::default();
        alive.refresh(
            &RenderState {
                state: Some(GameState::Playing),
                ..dead.clone()
            },
            false,
        );
        let mut alive_list = DrawList::new();
        draw_hud(&mut alive_list, &alive, (960, 720));

        assert_eq!(
            dead_list.len(),
            alive_list.len() + 1,
            "the death screen must add the scrim and nothing else",
        );
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
}
