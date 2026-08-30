//! Breakout's start-up, and the methods the engine's loop calls.
//!
//! # There is no loop in this file
//!
//! There was, and it was four hundred lines: pump the shell, route the input,
//! run the ticks the clock owes, lay out the menu, draw the panel, present,
//! count the frame, tear it all down. Every one of those was the same in all
//! five samples, so it is [`crcbl::engine::Loop`]'s now and this crate reaches
//! it through [`HostedGame`].
//!
//! ```text
//! Loop::frame()                     ← the engine's
//!   pump, input, menu, pause, resize
//!   run_ticks  ─────────────────────→ Breakout::tick
//!   draw_list.clear()
//!     ─────────────────────────────→ Breakout::draw    (board + HUD)
//!     menu, debug overlay             ← the engine's
//!   gpu.frame()
//! ```
//!
//! **The simulation is still inside `run_ticks`'s `while`, not after it.**
//! Anything stepped once per frame instead has a speed proportional to the
//! frame rate, which is a bug a headless run — where a frame is pinned to
//! exactly 1/60 s — cannot see. That rule moved into the engine with the loop;
//! it did not stop applying.
//!
//! What is left here is start-up ([`start`], [`with_shell`], [`PendingLoop`]),
//! because a window's title and a game's constructor are this game's, and the
//! seven [`HostedGame`] methods, because they are what a game is.

use crcbl::core::input::KeyCode;
use crcbl::engine::{
    Booted, Clock, ExitReason, FrameInfo, HostedGame, PauseControl, PointerUpdate, RunSummary,
    TouchUpdate, wait_for_configure,
};
use crcbl::prelude::*;
use crcbl::shell::{CursorIcon, DisplayMode, PointerMode, ShellBackend as Backend, WindowId};

use crate::game::{self, Game, GameState, RenderState};
use crate::gpu::Gpu;
use crate::menu::{self, Launch, MenuKind, Menus};

pub use crate::args::Options;

// ---- summary ----------------------------------------------------------------

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Summary {
    pub backend: Backend,
    pub frames: u64,
    /// Times the loop called `Game::tick`.
    ///
    /// Distinct from [`Self::sim_ticks`], and the distinction is the point: this
    /// one counts calls and rises whether or not the call did anything.
    pub ticks: u64,
    /// Times the simulation actually advanced, from `Game::ticks_run`.
    pub sim_ticks: u64,
    pub events: u64,
    pub extent: (u32, u32),
    pub exit: ExitReason,
    pub score: u32,
    pub state: GameState,
    /// Whether the simulation was stopped when the run ended.
    ///
    /// Beside `state` rather than inside it: pause is the loop declining to
    /// advance the simulation, not a state the simulation is in. See
    /// [`crcbl::engine::Loop::is_paused`].
    pub paused: bool,
    /// The mode the window system actually had the window in, **not** the one
    /// the run last asked for. A summary that reported the request would say
    /// "borderless" for every compositor that refused.
    pub mode: DisplayMode,
}

// ---- errors -----------------------------------------------------------------

/// What can stop breakout: the loop's own failures, plus this game's.
///
/// An alias rather than an enum. Every sample had the same five loop
/// variants written out with the same `Display` arms, so they live in
/// [`crcbl::engine::LoopError`] now and this names the game error that
/// goes in the sixth. Its docs say why a game error is wrapped by name —
/// `.map_err(BreakoutError::Game)` — while the engine's three convert with `?`.
pub type BreakoutError = crcbl::engine::LoopError<game::GameError>;

// ---- the game ---------------------------------------------------------------

/// The key a menu's `PLAY` button stands for.
///
/// Fired as a real key event rather than by calling into `Game`, because serving
/// the ball is the simulation's business and the simulation is driven by its
/// action map — a button that reached past it would be a second way to start a
/// game, with its own bugs.
const LAUNCH_KEY: KeyCode = KeyCode::Space;

/// Breakout, as the engine's loop hosts it.
///
/// **The loop is not here any more.** The pump, the input routing, the
/// fixed-step accumulator, the menu, the debug panel, the budget and teardown
/// are [`crcbl::engine::Loop`]'s, and were the same in all five samples. What is
/// left is what was always breakout's: the simulation, the state it renders
/// from, and its HUD.
#[derive(Debug)]
pub struct Breakout {
    game: Game,
    /// Refilled from the simulation every frame, so a steady-state frame does
    /// not allocate a fresh brick vector.
    render_state: RenderState,
    hud: HudStrings,
    /// The board numbers the debug panel shows, snapshotted in [`Breakout::draw`].
    ///
    /// Horde's `scene` field is the same shape and there for the same reason:
    /// `debug_sections` takes `&self`, so the numbers have to be read on a path
    /// that has the game mutably and be waiting when the panel asks.
    board: game::BoardStats,
    /// The on-screen pause button, which is the engine's rather than this
    /// game's: [`crcbl::engine::PAUSE_KEY`] never reaches a game, so without it
    /// a phone can start a run and never stop it — and the pause menu is the
    /// only place fullscreen and the debug panel are tappable. It draws nothing
    /// until a finger arrives, so a keyboard-and-mouse run is unchanged and no
    /// golden frame moves.
    pause: PauseControl,
    /// Whether a panel is up, as [`Breakout::menu_kind`] last decided.
    ///
    /// Kept because [`Breakout::cursor`] is asked `&self` and the answer is the
    /// menu's rather than the simulation's; `apps/viewer` and `apps/breach`
    /// keep the same copy for the same reason.
    panel_up: bool,
}

/// The loop breakout runs in.
///
/// A type alias, because the loop is the engine's. `S` is the shell type: the
/// native and browser paths both build `Loop<dyn Shell>`, and the tests build
/// `Loop<HeadlessShell>` so they can inject the events a compositor would send.
pub type Loop<S = dyn Shell> = crcbl::engine::Loop<S, Breakout>;

/// Runs the full loop.
///
/// # Errors
///
/// [`BreakoutError`] if the shell, the GPU or the game failed. Teardown runs on
/// every path: a failing frame must still release the swapchain, the surface
/// and the window, or `crcbl-vk`'s device teardown logs objects still alive.
pub fn run(options: &Options) -> Result<Summary, BreakoutError> {
    crcbl::engine::drive(start(options)?)
}

/// Opens a shell, a window, a GPU and the game.
///
/// # Errors
///
/// [`BreakoutError`] if any of them refused.
pub fn start(options: &Options) -> Result<Loop, BreakoutError> {
    let shell = crcbl::engine::open_shell(options.common.headless)?;
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
/// [`BreakoutError`] if the window never configured, the GPU would not open, or
/// the game could not be built.
pub fn with_shell<S: Shell + ?Sized>(
    mut shell: Box<S>,
    options: &Options,
) -> Result<Loop<S>, BreakoutError> {
    let clock_source = Clock::new(options.common.headless);
    let window = open_the_window(
        shell.as_mut(),
        &clock_source,
        options.common.display_mode(),
        options.common.size,
    )?;

    let mut events = 0;
    let extent = wait_for_configure(shell.as_mut(), window, &mut events)?;

    // Locked to orthographic: breakout is a pure 2D game.
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
/// [`BreakoutError`] if the game could not be built.
fn assemble<S: Shell + ?Sized>(
    booted: Booted<S, Gpu>,
    options: &Options,
) -> Result<Loop<S>, BreakoutError> {
    // `--screenshot`, armed before the first frame because the frame it names
    // is counted from this point. The flag forces `--headless` on, so the
    // context behind this is always an offscreen ring — see
    // [`crcbl::args::Common::screenshot`].
    //
    // The mutable binding lives inside the `cfg` rather than on the parameter:
    // a browser build arms nothing, so a `mut` in the signature would be one
    // the wasm32 target correctly reports as unused.
    #[cfg(not(target_arch = "wasm32"))]
    let booted = {
        let mut booted = booted;
        if let Some(request) = options.common.screenshot_request() {
            booted.gpu.context_mut().set_screenshot(request);
        }
        booted
    };
    let game =
        Game::new(options.common.headless, options.common.tick_hz).map_err(BreakoutError::Game)?;
    Ok(Loop::new(
        booted,
        Breakout {
            game,
            render_state: RenderState::default(),
            hud: HudStrings::default(),
            board: game::BoardStats::default(),
            pause: PauseControl::new(),
            panel_up: false,
        },
        options.common.loop_config(),
    ))
}

impl Breakout {
    /// The simulation, for scripted tests and for an embedder that drives it.
    pub const fn game(&self) -> &Game {
        &self.game
    }

    /// The simulation, mutably. See [`Breakout::game`].
    pub const fn game_mut(&mut self) -> &mut Game {
        &mut self.game
    }
}

/// Breakout's half of the frame, and nothing else.
///
/// A method apiece where there was a 400-line `frame()`. Everything the loop does
/// around these — and it is most of what a frame does — is
/// [`crcbl::engine::Loop`]'s now.
impl HostedGame for Breakout {
    type Error = game::GameError;
    type Gpu = Gpu;
    type MenuKind = MenuKind;
    type MenuAction = Launch;
    type Summary = Summary;

    const NAME: &'static str = "breakout";

    fn menus() -> Menus {
        menu::menus()
    }

    fn tick(&mut self, gpu: &mut Gpu, _tick_dt: f64) {
        // Here rather than on resize, because this is where the camera's extent
        // is in hand and there is no cheaper place to be right: a tick that ran
        // with last size's mapping would put the paddle somewhere the finger is
        // not, on exactly the frame a phone rotates.
        self.game
            .set_view_half_width(f64::from(crate::gpu::camera_half_width(gpu.extent())));
        self.game.tick();
    }

    fn key_event(&mut self, key: KeyCode, pressed: bool) {
        // Forwarded to the game, which replays it at the start of the next
        // tick. A frame that runs no ticks loses nothing.
        self.game.key_event(key, pressed);
    }

    /// The map the console's `bind` and `unbind` rebind.
    ///
    /// The same map `key_event` above feeds, so a rebind typed at the console
    /// moves the key this game actually plays on rather than a copy of it.
    fn actions(&mut self) -> Option<&mut crcbl::input::ActionMap> {
        Some(self.game.action_map_mut())
    }

    /// One finger, offered to the pause button and to nothing else.
    ///
    /// Breakout's own input is a place and an edge, and both already arrive as
    /// the emulated pointer — see [`pointer_event`](Self::pointer_event). The
    /// contact stream is here for the one control the pointer cannot carry.
    fn touch_event(&mut self, touch: TouchUpdate) {
        self.pause.touch(touch);
    }

    fn take_pending_pause(&mut self) -> bool {
        self.pause.take_fired()
    }

    /// **The paddle follows the finger, and a tap serves.**
    ///
    /// Only the x coordinate: this game's one axis is the width of the court.
    /// The conversion from the surface's −1…1 to world units is the game's, and
    /// it happens in [`Game::tick`] against the half width [`Self::tick`] hands
    /// over — the engine has no idea where this camera is pointed and the sample
    /// has no business doing DPI arithmetic.
    ///
    /// **Unless the pause button has it.** The finger pressing that button is
    /// also the emulated pointer, so without the first line asking for the pause
    /// would jerk the paddle into the corner and serve the ball on the way.
    fn pointer_event(&mut self, pointer: PointerUpdate) {
        if self.pause.takes_pointer(pointer) {
            return;
        }
        if let Some(at) = pointer.at {
            self.game.pointer_moved(at.x);
        }
        if pointer.pressed {
            self.game.pointer_button(true);
        }
        if pointer.released {
            self.game.pointer_button(false);
        }
    }

    /// Confined to the board while the paddle is being driven, free under a
    /// panel.
    ///
    /// The other half of [`Breakout::cursor`], and the pair together is what
    /// GLFW calls `CAPTURED`: the pointer may not leave, and nothing is drawn
    /// where it is. `pointer_event` binds the pointer's `x` to the paddle by
    /// **absolute position**, so a hand that runs past the window's edge parks
    /// the paddle at the end of its travel and then has a dead zone to cross on
    /// the way back — the classic fault of every paddle game that did not
    /// confine.
    ///
    /// The panel frees it for the reason `apps/breach` frees its lock: a player
    /// who cannot reach their own cursor cannot leave, and the panels are the
    /// only place this game has to be left from. Focus loss pauses the loop, so
    /// the pointer is handed back on the frame the window stops being the one
    /// in front — no window this game owns can hold a pointer nobody is
    /// looking at.
    ///
    /// A browser has no confine primitive and declines this, which the loop
    /// logs once and turns into [`PointerMode::Free`]; the demo plays exactly
    /// as it did before. `docs/backlog.md` says why emulating it is not on the
    /// table.
    fn pointer_mode(&self) -> PointerMode {
        if self.panel_up {
            PointerMode::Free
        } else {
            PointerMode::Confined
        }
    }

    /// Hidden while the paddle is being driven, and the arrow under a panel.
    ///
    /// **The paddle is the cursor here.** `pointer_event` above binds the
    /// pointer's `x` straight to the paddle, so a visible arrow is a second
    /// pointer drawn a paddle's height above the first — which is why every
    /// game of this shape, from the original on, hides it. It is the honest use
    /// of the hook's `None`: the pointer stays
    /// [`Free`](crcbl::shell::PointerMode::Free), because nothing here wants
    /// relative motion or a pointer that cannot leave.
    ///
    /// A panel gets it back, and has to: the start, pause, won and lost menus
    /// are all clicked, and a menu is not something a paddle can point at.
    fn cursor(&self) -> Option<CursorIcon> {
        if self.panel_up {
            Some(CursorIcon::Default)
        } else {
            None
        }
    }

    fn menu_action(id: crcbl::ui::WidgetId) -> Option<Launch> {
        menu::launch_from_id(id)
    }

    fn apply(&mut self, action: Launch) {
        match action {
            // A real key event rather than a call into `Game`: serving the ball
            // is the simulation's business and the simulation is driven by its
            // action map, so a button that reached past it would be a second way
            // to start a game with its own bugs. The release is queued straight
            // after the press because launching is an *edge* — a press with no
            // release leaves the action held for the rest of the run.
            Launch::Ball => {
                self.game.key_event(LAUNCH_KEY, true);
                self.game.key_event(LAUNCH_KEY, false);
            }
        }
    }

    fn menu_kind(
        &mut self,
        _menus: &mut crcbl::ui::menu::MenuSet<MenuKind>,
        paused: bool,
    ) -> MenuKind {
        let kind = MenuKind::of(paused, &self.render_state);
        let panel_up = kind != MenuKind::None;
        // A panel takes the button away and a half-press with it, and the
        // contacts that arrive before the next call are hit-tested against this
        // answer — the same "last frame's menu" rule the loop applies to its own
        // pointer.
        self.pause.set_panel_up(panel_up);
        // Recorded as well as acted on: `cursor` is asked immediately after this
        // and is handed no argument.
        self.panel_up = panel_up;
        kind
    }

    fn draw(
        &mut self,
        gpu: &mut Gpu,
        draw_list: &mut crcbl::ui::draw_list::DrawList,
        frame: FrameInfo,
    ) {
        self.game.render_state(&mut self.render_state);
        gpu.set_board(&self.render_state);
        self.board = self.game.board_stats();
        self.hud.refresh(&self.render_state, frame.paused);
        draw_hud(draw_list, &self.hud);
        // After the HUD and before the menu, which the loop appends to this same
        // list. **This frame's menu, not the one `menu_kind` last reported**:
        // the loop asks for the draw before it asks which menu the frame shows,
        // and `MenuKind::of` is a pure function of what this method has already
        // refreshed, so the button goes away on the same frame the panel arrives
        // rather than a frame late.
        let panel_up = MenuKind::of(frame.paused, &self.render_state) != MenuKind::None;
        self.pause.layout(gpu.extent(), gpu.atlas());
        self.pause.render(draw_list, gpu.atlas(), panel_up);
    }

    /// **Breakout's own module, and it has exactly one.**
    ///
    /// No network section, because this game runs over `InMemoryTransport` and
    /// has no connection to report on; no audio section, because
    /// [`crate::audio::Audio`] keeps no counter — it banks two cues and plays
    /// them, and a row invented to fill the panel would be state added for the
    /// panel's benefit rather than the game's. What is left is the board, and
    /// the panel is the two numbers of it that are nowhere else.
    fn debug_sections(&self, panel: &mut crcbl::ui::DebugPanel) {
        panel.add(&self.board);
    }

    fn summary(&self, run: RunSummary) -> Summary {
        Summary {
            backend: run.backend,
            frames: run.frames,
            ticks: run.ticks,
            sim_ticks: self.game.ticks_run,
            events: run.events,
            extent: run.extent,
            exit: run.exit,
            score: self.game.score,
            state: self.game.state,
            paused: run.paused,
            mode: run.mode,
        }
    }

    fn log_summary(summary: &Summary) {
        crcbl::log::info!(
            "breakout: {} frames, {} ticks, score {} ({:?}, {:?})",
            summary.frames,
            summary.ticks,
            summary.score,
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
) -> Result<WindowId, BreakoutError> {
    Ok(crcbl::engine::open_window(
        shell,
        clock_source,
        &WindowDesc {
            title: "Breakout",
            app_id: "sh.kryptic.crcbl.breakout",
            size: crcbl::engine::requested_window_size(size),
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
    /// [`BreakoutError`] if the shell refused the window.
    pub fn request(
        mut shell: Box<S>,
        options: &Options,
        clock_source: Clock,
    ) -> Result<Self, BreakoutError> {
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
                (),
            ),
            options: options.clone(),
        })
    }

    /// Advances start-up. `Ok(None)` means "not yet, poll again next frame".
    ///
    /// # Errors
    ///
    /// [`BreakoutError`] if the window went away before it had a size, if the device
    /// request failed, or if the game could not be built.
    pub fn poll(&mut self) -> Result<Option<Loop<S>>, BreakoutError> {
        let Some(booted) = self.boot.poll::<BreakoutError>()? else {
            return Ok(None);
        };
        assemble(booted, &self.options).map(Some)
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
    last: Option<(u32, u32, u32, Option<GameState>, bool)>,
}

impl HudStrings {
    /// **`paused` wins over the simulation's state, and that is the bug fix.**
    /// The status line used to read straight off `RenderState::state`, which is
    /// the *server's* idea of what is happening — and the server is still
    /// playing while the window sits behind a browser. A player alt-tabbed away
    /// saw "Playing" and came back to a lost life.
    fn refresh(&mut self, render: &RenderState, paused: bool) {
        let key = (
            render.score,
            render.high_score,
            render.lives,
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
            "Score: {}  High: {}",
            render.score, render.high_score
        );
        self.lives.clear();
        let _ = write!(self.lives, "Lives: {}", render.lives);
        self.state.clear();
        self.state.push_str(if paused {
            "PAUSED - press ESC"
        } else {
            match render.state {
                Some(GameState::WaitingForLaunch) | None => "Press SPACE to launch",
                Some(GameState::Playing) => "Playing",
                Some(GameState::Won) => "YOU WIN! Press SPACE",
                Some(GameState::Lost) => "GAME OVER - Press SPACE",
            }
        });
    }
}

/// Draws the HUD, and nothing else.
///
/// # The board used to be in here
///
/// Until the sprite pass existed it had to be: `crcbl-render`'s
/// [`crcbl::render::ForwardRenderer`] draws **one** instance — `begin_frame`
/// takes a single `model: Mat4`, and `add_passes` records exactly
/// `draw_indexed(0..index_count, 0, 0..1)`. The
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
    use crcbl::math::Vec2;

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
    use crcbl::args::Common;
    use crcbl::engine::{
        DEBUG_OVERLAY_KEY, FULLSCREEN_KEY, MENU_ACTIVATE_KEY, MENU_DOWN_KEY, PAUSE_KEY,
    };

    use super::*;
    use core::time::Duration;

    use crcbl::core::input::KeyCode;
    use crcbl::engine::Flow;
    use crcbl::shell::HeadlessShell;

    fn headless(frames: u64) -> Options {
        Options {
            common: Common {
                headless: true,
                backend: Some(GpuBackend::Null),
                frames: Some(frames),
                ..Common::new(crate::game::DEFAULT_TICK_HZ)
            },
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
        engine.game_mut().game_mut().key_event(KeyCode::Space, true);
        engine
            .game_mut()
            .game_mut()
            .key_event(KeyCode::Space, false);
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
        engine.game().game().render_state(&mut render);
        assert_eq!(render.bricks.len(), crate::game::BRICK_COUNT);
        assert_eq!(
            engine.gpu().bricks(),
            render.bricks.as_slice(),
            "the grid never reached the renderer"
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
        let mut matches = drawn
            .iter()
            .enumerate()
            .filter(|(_, text)| *text == label)
            .map(|(at, _)| at);
        let at = matches
            .next()
            .unwrap_or_else(|| panic!("no {label} row in {drawn:?}"));
        // Row labels share one namespace across every section of the panel, and
        // two have collided already — `crcbl-render`'s frame timings draw a
        // `pending` row, and this sample's first draft named one of its own the
        // same. A reader tells them apart by the heading above them; a search
        // through the flat draw list cannot, and would read whichever came
        // first for ever after.
        assert!(
            matches.next().is_none(),
            "more than one {label} row in {drawn:?}, so this reads whichever the panel \
             happened to draw first"
        );
        drawn
            .get(at + 1)
            .unwrap_or_else(|| panic!("no value after {label} in {drawn:?}"))
            .clone()
    }

    /// Every `Text` command the frame handed to the UI pass.
    fn ui_text(engine: &Loop<HeadlessShell>) -> Vec<String> {
        use crcbl::ui::draw_list::DrawCommand;
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

        // **And the HUD reaches the GPU.** `UiRenderer::add_pass` declares
        // nothing when the draw list is empty, so the pass's presence in the
        // frame's graph is what separates "the HUD was drawn" from "the HUD was
        // composited". Unlike the sandbox — which draws no UI at all with the
        // overlay off — the HUD is on every frame, so the pass is present even
        // before F3; the claim is that the game's own UI reaches the graph,
        // not that the overlay is distinguishable in the dump.
        assert!(
            engine.gpu().last_dump().contains("ui-composite"),
            "the HUD's UI pass must be in the frame:\n{}",
            engine.gpu().last_dump(),
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
    /// has are the frame's, the GPU's when the device has timestamp queries, and
    /// this game's own board. No network section, and no audio one either —
    /// `crate::audio::Audio` counts nothing, so there is nothing for it to say.
    /// No configuration decided any of that; the panel got what the sample's
    /// systems offered.
    #[test]
    fn the_overlay_is_composed_of_exactly_the_modules_breakout_has() {
        let mut engine = scripted(&headless_with(8, |common| {
            common.debug_overlay = Some(true)
        }));
        engine.frame().expect("a frame");
        engine.frame().expect("a frame");

        let titles: Vec<&str> = engine
            .debug()
            .panel
            .sections()
            .iter()
            .map(crcbl::ui::DebugSection::title)
            .collect();
        let expected: &[&str] = if engine.gpu().timings().is_some() {
            &["frame", "gpu", "counters", "board"]
        } else {
            &["frame", "counters", "board"]
        };
        assert_eq!(titles, expected, "no module appears that no system offered");

        let drawn = ui_text(&engine);
        for row in ["frame", "fps", "avg", "worst", "window"] {
            assert!(drawn.iter().any(|t| t == row), "missing {row}: {drawn:?}");
        }

        // **The board section reached the draw list with the board's numbers in
        // it**, not just its heading: nothing has been played, so the grid is
        // whole and the ball is on its launch speed.
        assert_eq!(
            row_value(&drawn, "bricks"),
            format!("{0}/{0}", game::BRICK_COUNT),
        );
        assert_eq!(row_value(&drawn, "ball"), "11.00/s");

        // **The numbers come from the clock, not from nowhere.** A frame that
        // never fed the window would draw the same labels with 0.00 ms beside
        // them, which is the failure a "the rows are present" assertion misses.
        // The first frame's interval is the clock's zero-length sentinel and is
        // dropped, so two frames leave exactly one sample: the headless step.
        assert_eq!(engine.debug().frame.len(), 1, "one real interval so far");
        assert_eq!(
            engine.debug().frame.mean(),
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
        with_shell(Box::new(HeadlessShell::new()), options).expect("headless always starts")
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
        let options = headless(60);

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
        let before = resumed.ticks();
        let _ = resumed.frame();
        assert!(
            resumed.ticks() - before <= 4,
            "a resumed tab ran {} ticks in one frame",
            resumed.ticks() - before,
        );
        resumed.finish(ExitReason::FrameBudget).expect("teardown");
    }

    // ---- focus, pause and fullscreen ----------------------------------------

    /// Runs `frames` frames, and insists every one of them was a real frame.
    ///
    /// The `assert_eq!` is not decoration: `Loop::frame` answers a spent frame
    /// budget with `Ok(Flow::Stop)` **before** it pumps, so a test that let one
    /// through would go on injecting key events into a loop that had stopped
    /// reading them and would report whatever state it was in when it stopped.
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
    /// A window that loses focus must stop reading as playing *and* must let go
    /// of whatever was held — no platform sends the releases. Both halves are
    /// asserted against the input state and the simulation, not against a flag:
    /// the paddle is moving left when focus goes, and after the player resumes
    /// it must be standing still even though no `keyup` ever arrived.
    #[test]
    fn losing_focus_pauses_the_game_and_lets_go_of_every_held_key() {
        let mut engine = scripted(&headless(400));
        let window = engine.window();

        // Hold left. The paddle is a kinematic body the tick moves, so a few
        // frames of it is a position that visibly changed.
        engine
            .shell_mut()
            .key_press(window, KeyCode::ArrowLeft)
            .expect("the window is live");
        run_frames(&mut engine, 20);
        let moved_to = engine.game().game().paddle_x();
        assert!(
            moved_to < -0.5,
            "the held key has to actually move the paddle first, got {moved_to}",
        );
        assert!(engine.game().game().move_left_is_held(), "left is held");
        assert_eq!(engine.held_keys(), vec![KeyCode::ArrowLeft]);

        // Focus goes. The player is looking at something else now.
        engine
            .shell_mut()
            .set_focus(window, false)
            .expect("the window is live");
        engine.frame().expect("a frame");
        assert!(engine.is_paused(), "an unfocused window is not playing");
        assert!(
            engine.held_keys().is_empty(),
            "the loop still thinks a key is down",
        );

        // Resume, and feed nothing. The compositor never sent a release for
        // ArrowLeft and never will; if the release the focus loss synthesized
        // did not reach the action map, the paddle drives into the wall.
        engine
            .shell_mut()
            .key_press(window, PAUSE_KEY)
            .expect("the window is live");
        run_frames(&mut engine, 60);
        assert!(!engine.is_paused(), "Escape resumes");
        assert!(
            !engine.game().game().move_left_is_held(),
            "the action map still holds a key focus loss should have released",
        );
        assert_eq!(
            engine.game().game().paddle_x(),
            moved_to,
            "the paddle kept moving on a key nobody is pressing",
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
            "clicking back in must not drop the player into a live ball",
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

    /// **The pointer reaches the paddle, in the space the surface is in.**
    ///
    /// The routing test, and the one that pins the conversion the *engine*
    /// owns: framebuffer pixels to the −1…1 a binding is declared against. The
    /// three positions are chosen so that no camera arithmetic is needed to say
    /// what the right answer is — the middle of the surface is the middle of
    /// the field, and each edge is the wall on that side — so an axis
    /// normalised 0…1, one still in pixels, and an inverted one each land
    /// somewhere this rejects.
    ///
    /// It also pins that a menu on screen does **not** claim the pointer's
    /// position: this run is a fresh game, so the start panel is up the whole
    /// time, and a paddle that could not be lined up before serving would be a
    /// paddle a phone cannot aim.
    #[test]
    fn the_paddle_follows_the_pointer_across_the_surface() {
        use crcbl::shell::PhysicalPoint;

        let mut engine = scripted(&headless(60));
        let window = engine.window();
        run_frames(&mut engine, 2);
        let (width, height) = engine.extent();
        let middle_y = f64::from(height) / 2.0;
        let paddle_x = |engine: &Loop<HeadlessShell>| engine.game().game().paddle_x();

        let point_at = |engine: &mut Loop<HeadlessShell>, x: f64| {
            engine
                .shell_mut()
                .move_pointer(window, PhysicalPoint::new(x, middle_y), (0.0, 0.0))
                .expect("the window is live");
            run_frames(engine, 2);
        };

        point_at(&mut engine, f64::from(width) / 2.0);
        assert!(
            paddle_x(&engine).abs() < 1e-6,
            "the middle of the surface is the middle of the field, got {}",
            paddle_x(&engine),
        );

        point_at(&mut engine, f64::from(width));
        assert!(
            (paddle_x(&engine) - (crate::game::WORLD_RIGHT - crate::game::PADDLE_HALF_WIDTH)).abs()
                < 1e-6,
            "the right edge of the surface is the right wall, got {}",
            paddle_x(&engine),
        );

        point_at(&mut engine, 0.0);
        assert!(
            (paddle_x(&engine) - (crate::game::WORLD_LEFT + crate::game::PADDLE_HALF_WIDTH)).abs()
                < 1e-6,
            "the left edge of the surface is the left wall, got {}",
            paddle_x(&engine),
        );

        engine.finish(ExitReason::FrameBudget).expect("teardown");
    }

    /// **A menu on screen owns the button.**
    ///
    /// The start panel is up on a fresh game, and a tap that both fired the
    /// widget under it and served the ball would be one gesture doing two
    /// things. The corner is [`the browser gate's own inset`](self) — the point
    /// the sibling test proves is over no button — so this is a tap on the
    /// panel's backdrop, which is where a phone's misses land.
    #[test]
    fn a_tap_beside_a_menu_does_not_reach_the_game() {
        use crcbl::core::input::PointerButton;
        use crcbl::shell::{ButtonState as PointerState, PhysicalPoint};

        let mut engine = scripted(&headless(60));
        let window = engine.window();
        run_frames(&mut engine, 2);
        assert!(
            engine.menu_layout().is_some(),
            "a fresh game shows the start menu, which is what this is about",
        );

        let at = PhysicalPoint::new(8.0, 8.0);
        for state in [PointerState::Pressed, PointerState::Released] {
            engine
                .shell_mut()
                .button(window, PointerButton::Left, state, Some(at))
                .expect("the window is live");
        }
        run_frames(&mut engine, 4);

        assert_eq!(
            engine.game().game().state,
            GameState::WaitingForLaunch,
            "a tap on the menu's backdrop served the ball",
        );
        engine.finish(ExitReason::FrameBudget).expect("teardown");
    }

    /// The cursor the shell actually has on this loop's window.
    fn the_cursor_the_shell_is_drawing(engine: &mut Loop<HeadlessShell>) -> Option<CursorIcon> {
        let window = engine.window();
        engine
            .shell_mut()
            .cursor(window)
            .expect("the loop's window is live")
    }

    /// The pointer mode the shell actually has this loop's window in.
    fn the_mode_the_shell_is_in(engine: &mut Loop<HeadlessShell>) -> PointerMode {
        let window = engine.window();
        engine
            .shell_mut()
            .window_state(window)
            .expect("the loop's window is live")
            .pointer_mode
    }

    /// **The pointer is confined to the board while the paddle is being driven,
    /// and handed back whenever a panel is up — focus loss included.**
    ///
    /// Read off the window rather than off [`Breakout::pointer_mode`], so what
    /// this asserts is the whole path: this game's answer, the loop's poll and
    /// the shell call.
    ///
    /// The focus-loss half is the one that would strand a player, and it is not
    /// this game's code at all — `crcbl::engine::lose_focus` pauses, and the
    /// pause is what frees the pointer. Asserted here because that is the only
    /// place the two meet, and a loop that reconciled the pointer *before*
    /// applying the pause would leave a window nobody is looking at holding it.
    #[test]
    fn the_pointer_is_confined_to_the_board_while_the_paddle_is_driven() {
        let mut engine = scripted(&headless(120));
        let window = engine.window();
        engine.frame().expect("a frame");
        assert_eq!(
            the_mode_the_shell_is_in(&mut engine),
            PointerMode::Free,
            "the start menu is up and the pointer is already trapped",
        );

        engine
            .shell_mut()
            .key_press(window, LAUNCH_KEY)
            .expect("the window is live");
        engine
            .shell_mut()
            .key_release(window, LAUNCH_KEY)
            .expect("the window is live");
        run_frames(&mut engine, 8);
        assert_eq!(engine.game().game().state, GameState::Playing);
        assert_eq!(
            the_mode_the_shell_is_in(&mut engine),
            PointerMode::Confined,
            "the paddle is bound to a pointer that can walk off the board",
        );

        engine
            .shell_mut()
            .set_focus(window, false)
            .expect("the window is live");
        engine.frame().expect("a frame");
        assert!(engine.is_paused(), "focus loss did not pause the run");
        assert_eq!(
            the_mode_the_shell_is_in(&mut engine),
            PointerMode::Free,
            "the window lost focus and kept the pointer",
        );
        engine.finish(ExitReason::FrameBudget).expect("teardown");
    }

    /// **The cursor is hidden while the paddle is the pointer, and back under a
    /// panel.**
    ///
    /// Read off the window rather than off [`Breakout::cursor`], so what this
    /// asserts is the whole path: this game's answer, the loop's poll, and the
    /// shell call. A hook that returned the right value and was never polled
    /// leaves the window on the arrow it was created with for the whole run.
    ///
    /// The panel halves are the ones that would strand a player: every menu
    /// this game has is clicked, and a menu is not something a paddle can point
    /// at.
    #[test]
    fn the_cursor_is_hidden_while_the_paddle_is_the_pointer() {
        let mut engine = scripted(&headless(120));
        let window = engine.window();
        engine.frame().expect("a frame");
        assert!(engine.menu_layout().is_some(), "the start menu is up");
        assert_eq!(
            the_cursor_the_shell_is_drawing(&mut engine),
            Some(CursorIcon::Default),
            "the start menu is up and there is no cursor to click it with",
        );

        engine
            .shell_mut()
            .key_press(window, LAUNCH_KEY)
            .expect("the window is live");
        engine
            .shell_mut()
            .key_release(window, LAUNCH_KEY)
            .expect("the window is live");
        run_frames(&mut engine, 8);
        assert_eq!(engine.game().game().state, GameState::Playing);
        assert_eq!(
            the_cursor_the_shell_is_drawing(&mut engine),
            None,
            "the paddle is following the pointer under a visible arrow",
        );

        engine
            .shell_mut()
            .key_press(window, crcbl::engine::PAUSE_KEY)
            .expect("the window is live");
        engine
            .shell_mut()
            .key_release(window, crcbl::engine::PAUSE_KEY)
            .expect("the window is live");
        engine.frame().expect("a frame");
        assert!(engine.is_paused());
        assert_eq!(
            the_cursor_the_shell_is_drawing(&mut engine),
            Some(CursorIcon::Default),
            "the pause panel is up and the cursor is still hidden",
        );

        engine
            .shell_mut()
            .key_press(window, crcbl::engine::PAUSE_KEY)
            .expect("the window is live");
        engine
            .shell_mut()
            .key_release(window, crcbl::engine::PAUSE_KEY)
            .expect("the window is live");
        engine.frame().expect("a frame");
        assert!(!engine.is_paused());
        assert_eq!(
            the_cursor_the_shell_is_drawing(&mut engine),
            None,
            "resuming left the arrow over the board",
        );
        engine.finish(ExitReason::FrameBudget).expect("teardown");
    }

    /// **A finger on the pause button pauses the run, and does nothing else.**
    ///
    /// Both halves matter here. The pause is the point — a phone had no way to
    /// reach it, and the pause menu is the only route to fullscreen and the
    /// debug panel. And "nothing else": the finger pressing that button *is* the
    /// emulated pointer this game binds its paddle and its serve to, so a
    /// control that only took the contact would jerk the paddle into the corner
    /// on the way to pausing.
    ///
    /// The tap is one pump, press and release together, which is what a tap on a
    /// phone is.
    #[test]
    fn a_finger_on_the_pause_button_pauses_the_run_without_moving_the_paddle() {
        use crcbl::core::input::{ContactId, PointerButton, TouchPhase};
        use crcbl::shell::{ButtonState as PointerState, PhysicalPoint};

        let mut engine = scripted(&headless(120));
        let window = engine.window();
        // Launched with the keyboard, so the panel is down and the paddle is
        // the pointer's — the state the button has to survive.
        engine
            .shell_mut()
            .key_press(window, KeyCode::Space)
            .expect("the window is live");
        engine
            .shell_mut()
            .key_release(window, KeyCode::Space)
            .expect("the window is live");
        run_frames(&mut engine, 8);
        assert_eq!(engine.game().game().state, GameState::Playing);
        assert!(engine.menu_layout().is_none(), "no panel is up");
        assert!(
            !ui_text(&engine).iter().any(|text| text == "PAUSE"),
            "a run nobody has touched drew an on-screen control",
        );
        let parked = engine.game().game().paddle_x();

        let centre = crcbl::engine::PauseControl::centre(engine.gpu().extent());
        let at = PhysicalPoint {
            x: f64::from(centre.x),
            y: f64::from(centre.y),
        };
        for (phase, state) in [
            (TouchPhase::Began, PointerState::Pressed),
            (TouchPhase::Ended, PointerState::Released),
        ] {
            engine
                .shell_mut()
                .touch(window, ContactId(1), phase, at)
                .expect("the headless shell reports TOUCH");
            // The emulated pointer the platform owes for the primary contact:
            // without it this would be testing a phone that does not exist.
            engine
                .shell_mut()
                .button(window, PointerButton::Left, state, Some(at))
                .expect("the window is live");
        }
        run_frames(&mut engine, 1);

        assert!(engine.is_paused(), "the tap never reached the pause");

        // **Un-paused with the key before the paddle is looked at**, because a
        // paused frame runs no ticks: the pointer position the tap carried maps
        // to the paddle on the *tick*, so it would be sitting unapplied and
        // reading the paddle here would call that a tap that moved nothing. The
        // key, not a button, so nothing in this half is the pointer's.
        for state in [PointerState::Pressed, PointerState::Released] {
            engine
                .shell_mut()
                .key(window, PAUSE_KEY, state)
                .expect("the window is live");
        }
        run_frames(&mut engine, 3);
        assert!(!engine.is_paused(), "the key did not resume");
        assert!(
            (engine.game().game().paddle_x() - parked).abs() < 1e-6,
            "the pause tap dragged the paddle from {parked} to {}",
            engine.game().game().paddle_x(),
        );

        // And out through the panel the button opens — a finger on `RESUME`,
        // which is the only way back on a device with no keyboard — with the
        // button on the frame once the panel has gone.
        for state in [PointerState::Pressed, PointerState::Released] {
            engine
                .shell_mut()
                .key(window, PAUSE_KEY, state)
                .expect("the window is live");
        }
        run_frames(&mut engine, 1);
        assert!(engine.is_paused(), "the key did not pause");
        let resume = engine.menu_layout().expect("the pause menu").items()[0];
        let resume = (resume.min + resume.max) * 0.5;
        let resume = PhysicalPoint {
            x: f64::from(resume.x),
            y: f64::from(resume.y),
        };
        for (phase, state) in [
            (TouchPhase::Began, PointerState::Pressed),
            (TouchPhase::Ended, PointerState::Released),
        ] {
            engine
                .shell_mut()
                .touch(window, ContactId(2), phase, resume)
                .expect("the headless shell reports TOUCH");
            engine
                .shell_mut()
                .button(window, PointerButton::Left, state, Some(resume))
                .expect("the window is live");
        }
        run_frames(&mut engine, 2);
        assert!(!engine.is_paused(), "the panel could not be tapped shut");
        assert!(
            ui_text(&engine).iter().any(|text| text == "PAUSE"),
            "the button never reached the frame: {:?}",
            ui_text(&engine),
        );
        engine.finish(ExitReason::FrameBudget).expect("teardown");
    }

    /// **A paused game's world is byte-identical after any number of frames.**
    ///
    /// `RenderState` is the whole board — ball, paddle, every live brick, score
    /// and lives — and comparing it is a far stronger claim than "the state
    /// enum did not change": the ball is in flight here, so a single tick moves
    /// it and the comparison fails.
    #[test]
    fn a_paused_game_does_not_advance_its_simulation() {
        let mut engine = scripted(&headless(400));
        let window = engine.window();

        // Launch, and let the ball get moving.
        engine
            .shell_mut()
            .key_press(window, KeyCode::Space)
            .expect("the window is live");
        run_frames(&mut engine, 40);
        let mut before = RenderState::default();
        engine.game().game().render_state(&mut before);
        assert_ne!(before.ball.y, 0.0, "the ball is live");
        assert_eq!(before.state, Some(GameState::Playing));

        engine
            .shell_mut()
            .key_press(window, PAUSE_KEY)
            .expect("the window is live");
        engine.frame().expect("a frame");
        assert!(engine.is_paused());

        let ticks = engine.ticks();
        let mut paused = RenderState::default();
        engine.game().game().render_state(&mut paused);
        run_frames(&mut engine, 120);
        let mut after = RenderState::default();
        engine.game().game().render_state(&mut after);

        assert_eq!(after, paused, "120 paused frames moved the world");
        assert_eq!(engine.ticks(), ticks, "a paused frame ran a tick");

        // And it starts again, which is what makes the equality above evidence
        // rather than a game that had already stopped.
        engine
            .shell_mut()
            .key_press(window, PAUSE_KEY)
            .expect("the window is live");
        run_frames(&mut engine, 20);
        let mut resumed = RenderState::default();
        engine.game().game().render_state(&mut resumed);
        assert_ne!(resumed, after, "resuming did not restart the simulation");
        engine.finish(ExitReason::FrameBudget).expect("teardown");
    }

    /// **A long pause does not lurch on resume.** The accumulator question:
    /// five seconds paused is three hundred ticks of wall-clock time that the
    /// simulation did not experience, and a resume that tried to catch any of
    /// it up would fast-forward the ball across the board in one frame. See the
    /// comment on the tick loop for the two alternatives and why they lurch.
    #[test]
    fn resuming_after_a_long_pause_runs_one_tick_not_a_catch_up_burst() {
        const STEP: Duration = Duration::from_micros(16_667);
        // A budget past the 320 frames this runs: `frame` answers a spent
        // budget before it pumps, so a test that overran it would go on
        // pressing keys nothing was reading.
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

        // Five seconds of paused frames: the clock keeps running, the
        // simulation does not.
        let ticks_at_pause = engine.ticks();
        for _ in 0..300 {
            step_frame(&mut engine);
        }
        assert_eq!(engine.ticks(), ticks_at_pause, "a paused frame ran a tick");

        engine
            .shell_mut()
            .key_press(window, PAUSE_KEY)
            .expect("the window is live");
        let before = engine.ticks();
        step_frame(&mut engine);
        assert!(!engine.is_paused());
        let burst = engine.ticks() - before;
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
            "the launched game reads as playing: {:?}",
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

    // -----------------------------------------------------------------------
    // The menus
    // -----------------------------------------------------------------------

    /// **The start menu is on screen before the first serve, and it reaches both
    /// passes.** The text is in the draw list the UI pass uploads and the frame
    /// is in the sprite list the menu pass draws — a menu that only made it to
    /// one of the two is a panel with no words or words with no panel.
    #[test]
    fn the_start_menu_is_drawn_before_the_first_serve() {
        let mut engine = scripted(&headless(60));
        run_frames(&mut engine, 2);
        assert_eq!(engine.menu_kind(), MenuKind::Start);

        let drawn = ui_text(&engine);
        assert!(
            drawn.iter().any(|t| t == "BREAKOUT") && drawn.iter().any(|t| t == "PLAY"),
            "the start menu's text is not in the draw list: {drawn:?}",
        );

        let sprites = engine.gpu().menu_sprites();
        // The scrim, the nine-slice frame, and nine quads for each of three
        // buttons.
        assert_eq!(sprites.len(), 1 + 9 + 9 * 3, "{}", sprites.len());
        let extent = engine.extent();
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
    fn a_game_in_progress_draws_no_menu() {
        let mut engine = scripted(&headless(60));
        let window = engine.window();
        engine
            .shell_mut()
            .key_press(window, KeyCode::Space)
            .expect("the window is live");
        run_frames(&mut engine, 10);
        assert_eq!(engine.menu_kind(), MenuKind::None);
        assert!(
            engine.gpu().menu_sprites().is_empty(),
            "a playing frame submitted {} menu sprites",
            engine.gpu().menu_sprites().len(),
        );
        let drawn = ui_text(&engine);
        assert!(
            !drawn.iter().any(|t| t == "RESUME"),
            "a menu's buttons are on screen mid-game: {drawn:?}",
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

        // Down once: the second item, which is FULLSCREEN.
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

        // A press in the corner, released over RESUME: the capture is on nothing,
        // so nothing fires.
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

        // Press and release over RESUME.
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

    /// **The key printed on a button still does what it always did.** The menu
    /// documents the keyboard rather than replacing it, so Space launches the
    /// ball with the start menu on screen and no menu key involved.
    #[test]
    fn space_still_launches_with_the_start_menu_showing() {
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
            "the ball never launched, so the start menu ate the key",
        );
        engine.finish(ExitReason::FrameBudget).expect("teardown");
    }

    /// And the `PLAY` button does the same thing the key does — the action goes
    /// through `game.rs`'s action map rather than round it.
    #[test]
    fn the_play_button_serves_the_ball() {
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
            "PLAY did not launch the ball",
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
        assert_ne!(
            engine.extent(),
            windowed_extent,
            "a borderless window covers the monitor, so the swapchain moved",
        );

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
    /// never calls `requestFullscreen` does. The request stands; the loop must
    /// not read it back as fact.
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
        // The answer: same size, still windowed.
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
        assert!(!engine.mode_honoured(), "the refusal has to be noticed");

        let summary = engine.finish(ExitReason::FrameBudget).expect("teardown");
        assert_eq!(summary.mode, DisplayMode::Windowed);
    }

    /// Holding F11 down does not strobe the window between modes.
    ///
    /// A compositor sends auto-repeat presses for a held key, and the mode
    /// toggle is the one loop key where acting on them is visibly wrong.
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

    /// A real clock ignores the caller: a loop that can read the time must not
    /// be steered by whoever calls it.
    #[test]
    fn a_real_clock_is_not_steerable() {
        let (mut engine, _) = poll_to_completion(&headless(1), Clock::new(false));
        engine.set_frame_step(Duration::from_secs(1));
        assert!(matches!(engine.clock_source(), Clock::Real(_)));
        engine.finish(ExitReason::FrameBudget).expect("teardown");
    }
}
