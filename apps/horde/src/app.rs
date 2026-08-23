//! Horde's start-up, and the methods the engine's loop calls.
//!
//! # There is no loop in this file
//!
//! There was, and `docs/backlog.md` called it the fourth copy: the pump's key
//! branch, `lose_focus`, the F11 toggle, `Loop::paused` and the pointer's
//! press-capture bookkeeping were the same code in four files. All of it is
//! [`crcbl::engine::Loop`]'s now and this crate reaches it through
//! [`HostedGame`].
//!
//! ```text
//! Loop::frame()                     ← the engine's
//!   pump, input, menu, pause, resize
//!   run_ticks  ─────────────────────→ Horde::tick
//!   draw_list.clear()
//!     ─────────────────────────────→ Horde::draw       (field + HUD)
//!     menu ───────────────────────→ Horde::menu_kind   (rebuilds the offer)
//!     debug overlay ──────────────→ Horde::debug_sections
//!   gpu.frame()
//! ```
//!
//! # What is this sample's own
//!
//! **A menu whose buttons are simulation state.** The other games build
//! every menu once, because a `RESUME` button says `RESUME` forever. This one's
//! level-up panel is three upgrades drawn from the run's seed, so
//! [`HostedGame::menu_kind`] — which the loop calls with its own `MenuSet` for
//! exactly this reason — rebuilds the panel when, and only when, the offer
//! changes. Firing one of its buttons presses a **real digit key** into the
//! game's action map rather than calling into `Game`, for the reason asteroids'
//! `FLY` button does: which upgrade a run took is state a seeded, scripted
//! replay has to reproduce.
//!
//! **The only per-game debug section any sample adds.** Asteroids' finding 8
//! said switching the panel on needs no per-sample plumbing; adding a section is
//! the other half of that claim, and it is [`HostedGame::debug_sections`] with
//! one line in it.
//!
//! # There is a start menu, and it arrived late
//!
//! This sample shipped without one — the argument, and the user's reversal of
//! it, are in `game::GameState`'s docs and `crate::menu`'s header. The four
//! menus are start, pause, level-up and death, and nothing in the loop holds the
//! game on the first of them: `run_tick` short-circuits on
//! `GameState::WaitingToStart` exactly as it does on `LevelUp`, so a start
//! screen that failed to hold would be a simulation bug and not a loop one.
//!
//! **The simulation is still inside `run_ticks`'s `while`, not after it.**
//! Anything stepped once per frame has a speed proportional to the frame rate,
//! which a headless run — where a frame is pinned to exactly 1/60 s — cannot
//! see.
//!
//! # The draw is culled and nothing else is capped
//!
//! This is the sample whose whole question is how many agents a tick can carry.
//! The placeholder renderer capped the number of quads it would emit because a
//! `DrawList` quad was six vertices uploaded per frame; an instanced sprite is
//! not, so [`crate::art`] culls to the view and draws everything that survives.

use crcbl::core::input::KeyCode;
use crcbl::engine::{
    Booted, Clock, ExitReason, FrameInfo, HostedGame, RunSummary, TouchUpdate, wait_for_configure,
};
use crcbl::math::Vec2;
use crcbl::prelude::*;
use crcbl::shell::{DisplayMode, ShellBackend as Backend, WindowId};
use crcbl::ui::draw_list::DrawList;

use crate::art::SceneStats;
use crate::controls::Controls;
use crate::game::{self, Game, GameState, RenderState, UPGRADE_CHOICES, Upgrade};
use crate::gpu::Gpu;
use crate::menu::{self, HordeAction, LevelUpOffer, MenuKind, Menus};

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
    /// What the sprite pass did with the run's last frame — see
    /// [`SceneStats`]. Reported because it is what P7's GPU culling is meant to
    /// change, and a number nobody prints is a number nobody notices moving.
    pub scene: SceneStats,
    /// Whether the simulation was stopped when the run ended. Beside `state`
    /// rather than inside it: pause is the loop declining to advance the
    /// simulation, not a state the simulation is in.
    pub paused: bool,
    /// The mode the window system actually had the window in, **not** the one
    /// the run last asked for.
    pub mode: DisplayMode,
}

// ---- errors -----------------------------------------------------------------

/// What can stop horde: the loop's own failures, plus this game's.
///
/// An alias rather than an enum. Every sample had the same five loop
/// variants written out with the same `Display` arms, so they live in
/// [`crcbl::engine::LoopError`] now and this names the game error that
/// goes in the sixth. Its docs say why a game error is wrapped by name —
/// `.map_err(HordeError::Game)` — while the engine's three convert with `?`.
pub type HordeError = crcbl::engine::LoopError<game::GameError>;

// ---- the game ---------------------------------------------------------------

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

/// Horde, as the engine's loop hosts it.
///
/// **The loop is not here any more.** The pump, the input routing, the
/// fixed-step accumulator, the menu, the debug panel, the budget and teardown
/// are [`crcbl::engine::Loop`]'s, and were the same in all five samples. What is
/// left is what was always horde's: the simulation, the state it renders from,
/// its HUD, and the level-up panel it rebuilds when the offer changes.
#[derive(Debug)]
pub struct Horde {
    game: Game,
    /// Refilled from the simulation every frame, so a steady-state frame does
    /// not allocate a fresh enemy list.
    render_state: RenderState,
    hud: HudStrings,
    /// Which offer the level-up panel was last built from — see
    /// [`LevelUpOffer`], and [`menu_kind`](HostedGame::menu_kind) for when it is
    /// consulted.
    offer: LevelUpOffer,
    /// The upgrade `--choose` auto-presses at every level-up, zero-based into
    /// [`UPGRADE_CHOICES`]; `None` when the flag was not given. See
    /// [`menu_kind`](HostedGame::menu_kind).
    choose: Option<usize>,
    /// The offer the auto-choose has already pressed for, so it fires once per
    /// distinct level-up rather than every frame the screen is up. Mirrors
    /// `LevelUpOffer::built_from`'s identity on `Horde` itself, because a
    /// press is an action on the game, not a menu rebuild.
    choose_taken: Option<(u32, [Upgrade; UPGRADE_CHOICES])>,
    /// What the sprite pass did with the last frame's field.
    ///
    /// Recorded rather than asked for at teardown, because the scene is rebuilt
    /// every frame and the run's last one is what the report is about.
    scene: SceneStats,
    /// The movement stick and the pause button a finger plays this game with.
    ///
    /// Invisible, and inert, until a contact actually arrives — see
    /// [`crate::controls`]. Nothing about a keyboard-and-mouse run changes.
    controls: Controls,
}

/// The loop horde runs in.
///
/// A type alias, because the loop is the engine's. `S` is the shell type: the
/// native and browser paths both build `Loop<dyn Shell>`, and the tests build
/// `Loop<HeadlessShell>` so they can inject the events a compositor would send.
pub type Loop<S = dyn Shell> = crcbl::engine::Loop<S, Horde>;

/// Runs the full loop.
///
/// # Errors
///
/// [`HordeError`] if the shell, the GPU or the game failed. Teardown runs on
/// every path: a failing frame must still release the swapchain, the surface and
/// the window, or `crcbl-vk`'s device teardown logs objects still alive.
pub fn run(options: &Options) -> Result<Summary, HordeError> {
    crcbl::engine::drive(start(options)?)
}

/// Opens a shell, a window, a GPU and the game.
///
/// # Errors
///
/// [`HordeError`] if any of them refused.
pub fn start(options: &Options) -> Result<Loop, HordeError> {
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
/// [`HordeError`] if the window never configured, the GPU would not open, or the
/// game could not be built.
pub fn with_shell<S: Shell + ?Sized>(
    mut shell: Box<S>,
    options: &Options,
) -> Result<Loop<S>, HordeError> {
    // **`--wall-clock` is why this is not `Clock::new(options.common.headless)`.**
    // A headless run's clock is a fake one stepping exactly 1/60 s, which is
    // what makes a scripted run reproducible and what makes the debug panel's
    // frame timing report the step rather than the frame. The scale measurement
    // needs the second of those to be a real number; every other headless run
    // needs the first. See `crate::args`.
    let clock_source = Clock::new(!options.real_clock());
    let window = open_the_window(
        shell.as_mut(),
        &clock_source,
        options.common.display_mode(),
        options.common.size,
    )?;

    let mut events = 0;
    let extent = wait_for_configure(shell.as_mut(), window, &mut events)?;

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
/// [`HordeError`] if the game could not be built.
fn assemble<S: Shell + ?Sized>(
    booted: Booted<S, Gpu>,
    options: &Options,
) -> Result<Loop<S>, HordeError> {
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
    let mut game = Game::with_setup(&options.setup()).map_err(HordeError::Game)?;
    if options.prefill > 0 {
        let staged = game.stage_field(options.prefill);
        if staged < options.prefill {
            crcbl::log::warn!(
                "prefill: asked for {} enemies and the cap left room for {staged}",
                options.prefill,
            );
        }
        // **A prefilled field starts itself.** `--prefill` is the scale
        // measurement's fixture and every number in
        // `docs/plan/sample/03-horde.md` was taken through it; left on the title
        // screen it would time a simulation that short-circuits on its first
        // line, and report it as ten thousand enemies a tick. The edge is
        // queued, not poked, so it goes through the same action map a player's
        // key does.
        game.key_event(RESTART_KEY, true);
        game.key_event(RESTART_KEY, false);
        crcbl::log::info!("prefill: started the run without waiting for the title screen");
    }
    // The extent the swapchain actually opened at, so the controls have a real
    // surface to lay out on before the first frame is drawn — a contact can
    // arrive in the same pump as the first configure.
    let controls = Controls::new(booted.gpu.extent());
    Ok(Loop::new(
        booted,
        Horde {
            game,
            render_state: RenderState::default(),
            hud: HudStrings::default(),
            offer: LevelUpOffer::default(),
            choose: options.choose,
            choose_taken: None,
            scene: SceneStats::default(),
            controls,
        },
        options.common.loop_config(),
    ))
}

impl Horde {
    /// The simulation, for scripted tests and for an embedder that drives it.
    pub const fn game(&self) -> &Game {
        &self.game
    }

    /// The simulation, mutably. See [`Horde::game`].
    pub const fn game_mut(&mut self) -> &mut Game {
        &mut self.game
    }

    /// What the last [`draw`](HostedGame::draw) read out of the simulation.
    pub const fn render_state(&self) -> &RenderState {
        &self.render_state
    }
}

/// Horde's half of the frame, and nothing else.
impl HostedGame for Horde {
    type Error = game::GameError;
    type Gpu = Gpu;
    type MenuKind = MenuKind;
    type MenuAction = HordeAction;
    type Summary = Summary;

    const NAME: &'static str = "horde";

    fn menus() -> Menus {
        menu::menus()
    }

    fn tick(&mut self, _gpu: &mut Gpu, _tick_dt: f64) {
        self.game.tick();
    }

    fn key_event(&mut self, key: KeyCode, pressed: bool) {
        // Forwarded to the game, which replays it at the start of the next
        // tick. A frame that runs no ticks loses nothing.
        self.game.key_event(key, pressed);
    }

    /// One finger, offered to the on-screen controls and then reported as a
    /// **device**.
    ///
    /// The stick's deflection is pushed after every contact event rather than
    /// once a frame, so a thumb that moves reaches the tick that runs in the
    /// same frame: the pump is before the ticks and the draw is after them, and
    /// a value handed over in the draw would be a frame of lag on every step the
    /// player takes.
    ///
    /// Pushed even when the value has not changed, which costs one `f32` pair
    /// and removes a class of bug: a lift and a cancel both centre the stick,
    /// and both go out through this same line.
    fn touch_event(&mut self, touch: TouchUpdate) {
        self.controls.touch(touch);
        let stick = self.controls.stick();
        self.game.stick_moved(stick.x, stick.y);
    }

    fn take_pending_pause(&mut self) -> bool {
        self.controls.take_pause()
    }

    fn menu_action(id: crcbl::ui::WidgetId) -> Option<HordeAction> {
        menu::action_from_id(id)
    }

    fn apply(&mut self, action: HordeAction) {
        match action {
            // Real key events rather than calls into `Game`: restarting a run
            // and taking an upgrade are the simulation's business and the
            // simulation is driven by its action map. The release is queued
            // straight after the press because both are *edges* — a press with
            // no release leaves the action held, which for the restart key is a
            // run that begins again sixty times a second.
            HordeAction::Restart => {
                self.game.key_event(RESTART_KEY, true);
                self.game.key_event(RESTART_KEY, false);
            }
            HordeAction::Choose(index) => {
                if let Some(&key) = CHOOSE_KEYS.get(index) {
                    self.game.key_event(key, true);
                    self.game.key_event(key, false);
                }
            }
        }
    }

    /// Rebuilds the level-up panel first, so the menu a level-up frame switches
    /// to is already the one this level's offer built.
    fn menu_kind(&mut self, menus: &mut Menus, paused: bool) -> MenuKind {
        self.offer
            .refresh(menus, self.render_state.level, self.render_state.offer);
        let kind = MenuKind::of(paused, &self.render_state);
        // `--choose <N>`: press the digit for the player once per distinct
        // level-up offer, so a headless run can reach past the screen without
        // a hand. The identity is the same pair `LevelUpOffer` rebuilds on —
        // the level and the offer — and the offer is `Some` exactly on
        // `LevelUp` frames, so `kind` is what makes the unwrap safe. Runs
        // every frame, so the `choose_taken` marker is what stops a second
        // press on the same offer from taking a second upgrade.
        if let (Some(index), MenuKind::LevelUp, Some(offer)) =
            (self.choose, kind, self.render_state.offer)
        {
            let identity = (self.render_state.level, offer);
            if Some(identity) != self.choose_taken {
                if let Some(&key) = CHOOSE_KEYS.get(index) {
                    self.game.key_event(key, true);
                    self.game.key_event(key, false);
                }
                self.choose_taken = Some(identity);
            }
        }
        // **After the auto-choose, and with the kind this frame settled on.**
        // A panel takes the on-screen controls away and centres a held stick —
        // see [`Controls::set_panel_up`] — and the contacts that arrive before
        // the next call are hit-tested against this answer, which is the same
        // "last frame's menu" rule the loop applies to its own pointer.
        self.controls.set_panel_up(kind != MenuKind::None);
        kind
    }

    fn draw(&mut self, gpu: &mut Gpu, draw_list: &mut DrawList, frame: FrameInfo) {
        self.game.render_state(&mut self.render_state);
        gpu.set_world(&self.render_state);
        self.scene = gpu.scene_stats();
        self.hud.refresh(&self.render_state, frame.paused);
        draw_hud(draw_list, &self.hud);
        // After the HUD and before the menu, which is appended to this same list
        // by the loop: a control belongs over the field it is steering something
        // across, and under the panel that has taken it away.
        //
        // **This frame's menu, not the one `menu_kind` last reported.** The loop
        // asks for the draw before it asks which menu the frame shows, and
        // `MenuKind::of` is a pure function of the two things this method has
        // already refreshed — so the controls go away on the same frame the
        // panel arrives rather than a frame late.
        let panel_up = MenuKind::of(frame.paused, &self.render_state) != MenuKind::None;
        self.controls.layout(gpu.extent(), gpu.atlas());
        self.controls.render(draw_list, gpu.atlas(), panel_up);
    }

    /// **This sample's own module, and the only one any sample adds.**
    ///
    /// Asteroids' finding 8 said switching the panel on needs no per-sample
    /// plumbing, and it does not; adding a per-sample *section* is the other
    /// half of the same claim and it is one line. The numbers are the ones this
    /// game's whole argument rests on — how much of the field survived the cull,
    /// and how many draw calls the survivors cost. The audio section is the
    /// silence explained: [`crate::audio::MAX_VOICES`] refuses the newest voice
    /// on a full mixer, and its refusal count is the only reason a cue that
    /// happened (the player's death among sixteen kills) was not heard.
    fn debug_sections(&self, panel: &mut crcbl::ui::DebugPanel) {
        panel.add(&self.scene);
        panel.add(&self.game.audio);
    }

    fn summary(&self, run: RunSummary) -> Summary {
        Summary {
            backend: run.backend,
            frames: run.frames,
            ticks: run.ticks,
            events: run.events,
            extent: run.extent,
            exit: run.exit,
            elapsed: self.game.elapsed,
            kills: self.game.kills,
            level: self.game.level,
            enemies: self.game.enemy_count(),
            state: self.game.state,
            scene: self.scene,
            paused: run.paused,
            mode: run.mode,
        }
    }

    fn log_summary(summary: &Summary) {
        crcbl::log::info!(
            "horde: {} frames, {} ticks, survived {:.1}s with {} kills at level {} \
             ({} enemies left, scene {:?}, {:?}, {:?})",
            summary.frames,
            summary.ticks,
            summary.elapsed,
            summary.kills,
            summary.level,
            summary.enemies,
            summary.scene,
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
) -> Result<WindowId, HordeError> {
    Ok(crcbl::engine::open_window(
        shell,
        clock_source,
        &WindowDesc {
            title: "Horde",
            app_id: "sh.kryptic.crcbl.horde",
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
    /// [`HordeError`] if the shell refused the window.
    pub fn request(
        mut shell: Box<S>,
        options: &Options,
        clock_source: Clock,
    ) -> Result<Self, HordeError> {
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
            ),
            options: options.clone(),
        })
    }

    /// Advances start-up. `Ok(None)` means "not yet, poll again next frame".
    ///
    /// # Errors
    ///
    /// [`HordeError`] if the window went away before it had a size, if the device
    /// request failed, or if the game could not be built.
    pub fn poll(&mut self) -> Result<Option<Loop<S>>, HordeError> {
        let Some(booted) = self.boot.poll::<HordeError>()? else {
            return Ok(None);
        };
        assemble(booted, &self.options).map(Some)
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
/// its death screen by hand; `crcbl::render::MenuRenderer` draws one behind every
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
///
/// `pub(crate)` for one reason: `crate::controls` puts the pause button in the
/// top-right corner and asserts it clears this, so the two layouts are held
/// apart by the number itself rather than by two people eyeballing a screenshot.
pub(crate) const HUD_PANEL_RIGHT: f32 = 820.0;
/// Where both lines of text start.
const HUD_TEXT_X: f32 = 10.0;
/// The stat line's font size, and the reason the panel is as wide as it is.
///
/// **The width is measured, not guessed** — `the_hud_fits_the_panel_it_is_drawn_on`
/// puts a stated worst-case run through the real
/// [`crcbl::ui::text::FontAtlas`] and requires it inside [`HUD_PANEL_RIGHT`].
/// The placeholder renderer's 430 became 560 became 690 by eye, and the browser
/// gate's capture caught the last of those with the text running off the end of
/// its own backdrop.
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
    use crcbl::args::Common;
    use crcbl::engine::{DEBUG_OVERLAY_KEY, Flow, PAUSE_KEY};

    use super::*;
    use crcbl::core::input::{ContactId, PointerButton, TouchPhase};
    use crcbl::math::DVec3;
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

    fn headless_loop() -> Loop<dyn Shell> {
        let mut engine = start(&headless(8)).expect("a headless loop always starts");
        start_the_run(engine.game_mut().game_mut());
        engine
    }

    /// A loop on a shell the test can post events to, with the run started.
    fn scripted(options: &Options) -> Loop<HeadlessShell> {
        let mut engine = at_the_title_screen(options);
        start_the_run(engine.game_mut().game_mut());
        engine
    }

    /// The same, left on the title screen — for the tests that are *about* it.
    fn at_the_title_screen(options: &Options) -> Loop<HeadlessShell> {
        with_shell(Box::new(HeadlessShell::new()), options).expect("headless always starts")
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

    /// **`--wall-clock` is a wiring fact, not a parser fact.** The parser test
    /// (`crate::args`) proves the flag is *read*; this proves the loop's clock
    /// obeys it. The wiring used to be `Clock::new(options.common.headless)`,
    /// which silently ignored the flag and left every headless measurement
    /// reporting the fixed step — the regression this test exists to catch.
    #[test]
    fn a_headless_run_reads_the_real_clock_when_wall_clock_asked_for_it() {
        let fixed = at_the_title_screen(&headless(8));
        assert!(
            matches!(fixed.clock_source(), Clock::Manual { .. }),
            "a headless run without --wall-clock must keep the fixed step"
        );

        let mut options = headless(8);
        options.wall_clock = true;
        let real = at_the_title_screen(&options);
        assert!(
            matches!(real.clock_source(), Clock::Real(_)),
            "--wall-clock must hand the loop the real clock"
        );
    }

    /// Every string the UI pass will draw this frame.
    fn ui_text(engine: &Loop<HeadlessShell>) -> Vec<String> {
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

    /// Presses and releases a key, and runs the frame that consumes it.
    fn tap(engine: &mut Loop<HeadlessShell>, code: KeyCode) {
        let window = engine.window();
        engine
            .shell_mut()
            .key_press(window, code)
            .expect("the window is live");
        engine
            .shell_mut()
            .key_release(window, code)
            .expect("the window is live");
        engine.frame().expect("a frame");
    }

    /// Clicks at `at`: press on one frame, release on the next, which is what
    /// the press-capture rule asks for.
    fn click(engine: &mut Loop<HeadlessShell>, at: Vec2) {
        let window = engine.window();
        let point = PhysicalPoint {
            x: f64::from(at.x),
            y: f64::from(at.y),
        };
        engine
            .shell_mut()
            .move_pointer(window, point, (0.0, 0.0))
            .expect("the window is live");
        for state in [PointerState::Pressed, PointerState::Released] {
            engine
                .shell_mut()
                .button(window, PointerButton::Left, state, Some(point))
                .expect("the window is live");
            engine.frame().expect("a frame");
        }
    }

    /// Posts one contact event, in framebuffer pixels, and runs the frame that
    /// folds it.
    fn finger(engine: &mut Loop<HeadlessShell>, contact: ContactId, phase: TouchPhase, at: Vec2) {
        let window = engine.window();
        engine
            .shell_mut()
            .touch(
                window,
                contact,
                phase,
                PhysicalPoint {
                    x: f64::from(at.x),
                    y: f64::from(at.y),
                },
            )
            .expect("the headless shell reports TOUCH");
        engine.frame().expect("a frame");
    }

    /// A loop with the run under way and no panel on screen, which is the state
    /// the on-screen controls are live in.
    fn playing(frames: u64) -> Loop<HeadlessShell> {
        let mut engine = scripted(&headless(frames));
        run_frames(&mut engine, 4);
        assert_eq!(
            engine.game().game().state,
            GameState::Playing,
            "the fixture never left the title screen, so no control is live",
        );
        engine
    }

    /// **A finger on the field walks the wizard, and letting go stops him.**
    ///
    /// The whole chain in one test: a contact the shell posted, through the
    /// loop's normalisation, into a `crcbl-ui` stick, out as a
    /// `Binding::Virtual` on the same `move` action `WASD` drives, and into the
    /// simulation's velocity. The *direction* is asserted rather than "it
    /// moved": a stick whose axes were swapped or whose sign was flipped moves
    /// the wizard just as far.
    #[test]
    fn a_finger_on_the_field_walks_the_wizard() {
        let mut engine = playing(240);
        let start = engine.game().game().player;

        // Deflected right, well past the dead zone: the pad is a fraction of
        // the surface, and 200 px is past the throw at this extent.
        let grab = Vec2::new(300.0, 500.0);
        finger(&mut engine, ContactId(1), TouchPhase::Began, grab);
        finger(
            &mut engine,
            ContactId(1),
            TouchPhase::Moved,
            grab + Vec2::new(200.0, 0.0),
        );
        run_frames(&mut engine, 30);

        let walked = engine.game().game().player;
        assert!(
            walked.x > start.x + 1.0,
            "a thumb pushed right walked the wizard from {start} to {walked}",
        );
        assert!(
            (walked.y - start.y).abs() < 0.5,
            "pushing due right also moved him up or down: {start} to {walked}",
        );

        // Straight up the screen is +Y in the world, which is the flip that
        // would send him south if it were missing.
        finger(
            &mut engine,
            ContactId(1),
            TouchPhase::Moved,
            grab - Vec2::new(0.0, 200.0),
        );
        run_frames(&mut engine, 30);
        let north = engine.game().game().player;
        assert!(
            north.y > walked.y + 1.0,
            "a thumb pushed up the screen walked him {} instead",
            north.y - walked.y,
        );

        // And the lift stops him: the stick centres, so the next frames move
        // him nowhere at all.
        finger(
            &mut engine,
            ContactId(1),
            TouchPhase::Ended,
            grab - Vec2::new(0.0, 200.0),
        );
        run_frames(&mut engine, 2);
        let released = engine.game().game().player;
        run_frames(&mut engine, 30);
        assert_eq!(
            engine.game().game().player,
            released,
            "a lifted finger left the wizard walking",
        );
    }

    /// **A cancelled gesture stops him too, and does not commit the direction
    /// he was last pushed in.**
    ///
    /// `TouchPhase::Cancelled` is "undo rather than commit", and for a stick
    /// that means the value goes to zero rather than latching the direction the
    /// thumb happened to be pushing. No finger is lifted anywhere in this test:
    /// the system takes the gesture, which is what an edge swipe or a palm
    /// rejection does.
    #[test]
    fn a_cancelled_gesture_stops_the_wizard() {
        let mut engine = playing(240);
        let grab = Vec2::new(300.0, 500.0);
        let pushed = grab + Vec2::new(200.0, 0.0);
        finger(&mut engine, ContactId(1), TouchPhase::Began, grab);
        finger(&mut engine, ContactId(1), TouchPhase::Moved, pushed);
        run_frames(&mut engine, 10);

        // The control: he really is walking, so "he stopped" below is a change
        // and not the state he was already in.
        let walking = engine.game().game().player;
        run_frames(&mut engine, 10);
        assert!(
            engine.game().game().player.x > walking.x,
            "the fixture was not actually walking, so this proves nothing",
        );

        finger(&mut engine, ContactId(1), TouchPhase::Cancelled, pushed);
        run_frames(&mut engine, 2);
        let cancelled = engine.game().game().player;
        run_frames(&mut engine, 30);
        assert_eq!(
            engine.game().game().player,
            cancelled,
            "the wizard kept walking after the system took the gesture away",
        );
    }

    /// **Two fingers, two controls, at the same time.**
    ///
    /// The claim this slice exists to make, and the one no earlier sample could
    /// make: the thumb on the stick is the *primary* contact, so the second
    /// finger raises no pointer event at all and pauses the game anyway. Both
    /// halves are asserted — the wizard was walking from the first contact when
    /// the second one landed, and the pause came from the second.
    #[test]
    fn a_second_finger_pauses_while_the_first_is_walking() {
        let mut engine = playing(240);
        let grab = Vec2::new(300.0, 500.0);
        finger(&mut engine, ContactId(1), TouchPhase::Began, grab);
        finger(
            &mut engine,
            ContactId(1),
            TouchPhase::Moved,
            grab + Vec2::new(200.0, 0.0),
        );
        run_frames(&mut engine, 20);
        let before = engine.game().game().player;
        run_frames(&mut engine, 10);
        assert!(
            engine.game().game().player.x > before.x,
            "the first finger is not walking him, so the pause below proves \
             nothing about two fingers",
        );
        assert!(!engine.is_paused());

        // The second finger, on the button, while the first has not moved.
        let button = crcbl::engine::PauseControl::centre(engine.gpu().extent());
        finger(&mut engine, ContactId(2), TouchPhase::Began, button);
        assert!(!engine.is_paused(), "a press paused it before the lift");
        finger(&mut engine, ContactId(2), TouchPhase::Ended, button);
        assert!(
            engine.is_paused(),
            "the second finger did not pause the run"
        );

        let paused_at = engine.game().game().player;
        run_frames(&mut engine, 20);
        assert_eq!(
            engine.game().game().player,
            paused_at,
            "a paused game kept walking",
        );
    }

    /// **The pause button is on the frame once a finger has arrived**, laid out
    /// where `crate::controls` says, and not there before.
    ///
    /// At the extent a browser canvas actually opens at rather than at the
    /// window's default: the button is placed from the *surface's* corner, and a
    /// short wide canvas is where a corner-relative layout goes wrong.
    #[test]
    fn the_pause_button_reaches_the_frame_once_a_finger_has_landed() {
        const CANVAS: (u32, u32) = (959, 463);
        let mut options = headless(240);
        options.common.size = Some(crcbl::shell::PhysicalSize {
            width: CANVAS.0,
            height: CANVAS.1,
        });
        let mut engine = at_the_title_screen(&options);
        start_the_run(engine.game_mut().game_mut());
        run_frames(&mut engine, 4);
        assert_eq!(engine.gpu().extent(), CANVAS);
        assert_eq!(engine.game().game().state, GameState::Playing);

        assert!(
            !ui_text(&engine).iter().any(|text| text == "PAUSE"),
            "a run nobody has touched drew an on-screen control",
        );

        finger(
            &mut engine,
            ContactId(1),
            TouchPhase::Began,
            Vec2::new(300.0, 300.0),
        );
        run_frames(&mut engine, 2);
        assert!(
            ui_text(&engine).iter().any(|text| text == "PAUSE"),
            "the button never reached the frame: {:?}",
            ui_text(&engine),
        );

        // And a contact at its centre really is on it — the same point the
        // browser gate taps, checked here where a failure names the rectangle.
        let centre = crcbl::engine::PauseControl::centre(CANVAS);
        finger(&mut engine, ContactId(2), TouchPhase::Began, centre);
        finger(&mut engine, ContactId(2), TouchPhase::Ended, centre);
        assert!(engine.is_paused(), "a tap on the button did not pause");
    }

    /// `--size` reaches the window: the headless offscreen ring opens at the
    /// extent named, not at the sample's 960 × 720 default. The default is the
    /// game's and stays the game's; the flag overrides it.
    #[test]
    fn a_size_flag_opens_the_window_at_the_extent_it_names() {
        let sized = at_the_title_screen(&headless_with(8, |common| {
            common.size = Some(crcbl::shell::PhysicalSize::new(320, 240));
        }));
        assert_eq!(sized.gpu().extent(), (320, 240));

        let default = at_the_title_screen(&headless(8));
        assert_eq!(default.gpu().extent(), (960, 720));
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
            !engine.gpu_mut().scene_sprites().is_empty(),
            "the loop handed the sprite pass nothing",
        );

        engine.game_mut().game_mut().freeze_spawns();
        engine.game_mut().game_mut().clear_enemies();
        engine.game_mut().game_mut().stage_player(DVec3::ZERO);
        engine
            .game_mut()
            .game_mut()
            .stage_enemy(game::EnemyKind::Grunt, DVec3::new(3.0, 0.0, 0.0));
        engine
            .game_mut()
            .game_mut()
            .stage_pickup(DVec3::new(-3.0, 0.0, 0.0), game::PickupKind::Xp(1));
        engine.frame().expect("a frame");

        // By position, not by count: the gun fires at the staged enemy, so the
        // frame may also carry a bolt, and a bare count would be satisfied by
        // three of the wrong things.
        let sprites = engine.gpu_mut().scene_sprites();
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

        let before = engine.ticks();
        run_frames(&mut engine, 3);
        assert_eq!(engine.ticks(), before, "a paused frame ran a tick");
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
                    position: crcbl::math::DVec3::ZERO,
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
        let mut engine = scripted(&headless(8));
        let window = engine.window();
        engine.frame().expect("a frame");

        // Through the shell, not by poking the list: the loop only knows a key
        // is held because it saw the press go by, and a test that filled the
        // list itself would pass with the pump's key branch deleted.
        engine
            .shell_mut()
            .key_press(window, KeyCode::KeyD)
            .expect("the window is live");
        engine.frame().expect("a frame");
        assert_eq!(
            engine.held_keys(),
            [KeyCode::KeyD],
            "the loop never noticed the key go down",
        );
        // Two ticks, not one: a tick writes the velocity the *next* integration
        // step consumes, so one tick moves nothing at all.
        engine.game_mut().game_mut().tick();
        engine.game_mut().game_mut().tick();
        let moved = engine.game_mut().game_mut().player;
        assert!(moved.x > 0.0, "the player never started moving: {moved:?}");

        engine
            .shell_mut()
            .set_focus(window, false)
            .expect("the window is live");
        engine.frame().expect("a frame");
        engine.game_mut().game_mut().tick();
        engine.game_mut().game_mut().tick();
        let after = engine.game_mut().game_mut().player;
        engine.game_mut().game_mut().tick();
        assert!(engine.held_keys().is_empty(), "the held list survived");
        assert!(engine.is_paused(), "focus loss must pause");
        assert_eq!(
            engine.game_mut().game_mut().player,
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
        let window = engine.window();
        engine.frame().expect("a frame");
        engine
            .shell_mut()
            .key_press(window, KeyCode::KeyD)
            .expect("the window is live");
        engine.frame().expect("a frame");
        assert_eq!(engine.held_keys(), vec![KeyCode::KeyD]);

        engine
            .shell_mut()
            .set_focus(window, false)
            .expect("the window is live");
        engine.frame().expect("a frame");
        assert!(engine.held_keys().is_empty(), "the held list survived");
        assert!(engine.is_paused(), "the focus loss did not pause");

        // **Resumed before the walk is measured**, which is what makes this able
        // to fail. A paused loop runs no ticks, so a player that stands still
        // behind the pause says nothing about whether the key came up — a
        // `lose_focus` that merely emptied its own list would pass. The failure
        // being guarded is a player who walks forever, and that only shows once
        // the simulation is running again.
        tap(&mut engine, PAUSE_KEY);
        assert!(!engine.is_paused(), "escape did not resume");
        let x = engine.game_mut().game_mut().player.x;
        run_frames(&mut engine, 6);
        assert_eq!(
            engine.game_mut().game_mut().player.x,
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
        assert_eq!(
            engine.game_mut().game_mut().state,
            game::GameState::WaitingToStart
        );

        let mut before = RenderState::default();
        engine.game_mut().game_mut().render_state(&mut before);
        let ticks = engine.ticks();
        run_frames(&mut engine, 64);

        let mut after = RenderState::default();
        engine.game_mut().game_mut().render_state(&mut after);
        assert!(engine.ticks() > ticks + 32, "the frames ran no ticks");
        assert_eq!(before, after, "the start screen played the game");
        assert_eq!(
            engine.game_mut().game_mut().enemies_spawned(),
            0,
            "the spawner ran"
        );
        assert_eq!(
            engine.game_mut().game_mut().elapsed,
            0.0,
            "the run clock ran"
        );
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
            !engine.gpu().menu_sprites().is_empty(),
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
    /// `SPACE` is what the other games print on their start screens and
    /// what the browser gate presses. `RESTART_KEY` is `R`, so a button that
    /// worked and a hint that lied would look identical without this.
    #[test]
    fn the_key_the_start_button_prints_starts_the_run() {
        let mut engine = at_the_title_screen(&headless(256));
        engine.frame().expect("a frame");
        assert_eq!(engine.menu_kind(), MenuKind::Start);

        tap(&mut engine, KeyCode::Space);
        run_frames(&mut engine, 2);
        assert_eq!(engine.game_mut().game_mut().state, game::GameState::Playing);
        assert_eq!(engine.menu_kind(), MenuKind::None);
        assert_eq!(
            engine.game_mut().game_mut().run,
            1,
            "the key restarted rather than started"
        );

        let elapsed = engine.game_mut().game_mut().elapsed;
        run_frames(&mut engine, 16);
        assert!(
            engine.game_mut().game_mut().elapsed > elapsed,
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
        assert_eq!(engine.game_mut().game_mut().state, game::GameState::Playing);
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

        let window = engine.window();
        engine
            .shell_mut()
            .set_focus(window, false)
            .expect("the window is live");
        engine.frame().expect("a frame");
        assert!(engine.is_paused(), "the focus loss did not pause");
        tap(&mut engine, PAUSE_KEY);
        run_frames(&mut engine, 8);
        assert_eq!(
            engine.game_mut().game_mut().state,
            game::GameState::WaitingToStart
        );
        assert_eq!(
            engine.game_mut().game_mut().elapsed,
            0.0,
            "the run clock ran"
        );
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
        assert_eq!(engine.game_mut().game_mut().state, game::GameState::Playing);
        assert!(
            engine.game_mut().game_mut().enemy_count() >= 64,
            "the field went away"
        );
        assert!(
            engine.game_mut().game_mut().elapsed > 0.0,
            "the clock never started"
        );
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
            !engine.gpu().menu_sprites().is_empty(),
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
        engine
            .game_mut()
            .game_mut()
            .bank_xp(game::xp_for_next_level(1));
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

        engine.game_mut().game_mut().set_player_hp(0.000_1);
        engine.game_mut().game_mut().stage_player(DVec3::ZERO);
        engine
            .game_mut()
            .game_mut()
            .stage_enemy(game::EnemyKind::Grunt, DVec3::ZERO);
        run_frames(&mut engine, 4);
        assert_eq!(engine.menu_kind(), MenuKind::GameOver);
    }

    /// **`--choose` presses the digit for the player, once per level-up offer**,
    /// so a headless run reaches past the screen without a hand — the frame the
    /// screen opens still reports [`MenuKind::LevelUp`], and the next couple of
    /// frames clear it with nothing injected.
    ///
    /// The negative control is the same script with the default `choose: None`:
    /// the screen opens and stays, which is what says the auto-advance came
    /// from the flag and not from something else about the loop.
    #[test]
    fn a_choose_flag_auto_advances_the_level_up_screen_and_the_default_parks_on_it() {
        let mut engine = scripted(&Options {
            choose: Some(0),
            ..headless(256)
        });
        engine.frame().expect("a frame");
        // The first frame is the title screen, whatever this test's helper
        // queued — see `each_state_gets_its_own_menu_and_a_playing_frame_gets_none`.
        assert_eq!(engine.menu_kind(), MenuKind::Start);
        run_frames(&mut engine, 1);
        assert_eq!(engine.menu_kind(), MenuKind::None);
        engine
            .game_mut()
            .game_mut()
            .bank_xp(game::xp_for_next_level(1));
        run_frames(&mut engine, 1);
        assert_eq!(
            engine.menu_kind(),
            MenuKind::LevelUp,
            "the screen did not open before the auto-choose could reach it",
        );
        run_frames(&mut engine, 2);
        assert_eq!(
            engine.menu_kind(),
            MenuKind::None,
            "the auto-choose never pressed a digit",
        );
        assert_eq!(
            engine.game_mut().game_mut().state,
            GameState::Playing,
            "the auto-choose closed the screen without taking the upgrade",
        );
        // The level crossed — the run really levelled up, rather than the
        // screen closing by some path that skipped it. (`level` increments on
        // entry to the screen, so the state check above is what proves the
        // upgrade was *taken*; `level` proves the screen was a real level-up.)
        assert_eq!(
            engine.game_mut().game_mut().level,
            2,
            "the auto-choose advanced but the level never moved",
        );

        // The same frames without the flag: parked on the screen, with the
        // same crossed level — which is what says the auto-advance came from
        // the flag and not from something else about the loop.
        let mut parked = scripted(&headless(256));
        parked.frame().expect("a frame");
        run_frames(&mut parked, 1);
        parked
            .game_mut()
            .game_mut()
            .bank_xp(game::xp_for_next_level(1));
        run_frames(&mut parked, 1);
        assert_eq!(parked.menu_kind(), MenuKind::LevelUp);
        run_frames(&mut parked, 2);
        assert_eq!(
            parked.menu_kind(),
            MenuKind::LevelUp,
            "without --choose the level-up screen still closed",
        );
        assert_eq!(parked.game_mut().game_mut().state, GameState::LevelUp);
        assert_eq!(parked.game_mut().game_mut().level, 2);
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
        engine
            .game_mut()
            .game_mut()
            .bank_xp(game::xp_for_next_level(1));
        run_frames(&mut engine, 2);
        assert_eq!(engine.menu_kind(), MenuKind::LevelUp);

        let before = engine.game_mut().game_mut().stats();
        let offer = engine
            .game_mut()
            .game_mut()
            .offer()
            .expect("a level-up has an offer");
        let layout = engine.menu_layout().expect("a level-up frame has a menu");
        let target = layout.items()[1];
        let over = (target.min + target.max) * 0.5;

        click(&mut engine, over);
        run_frames(&mut engine, 2);

        let after = engine.game_mut().game_mut().stats();
        assert_ne!(after, before, "the click changed nothing at all");
        assert_eq!(
            after,
            expected_after(before, offer[1]),
            "the click applied something other than {:?}",
            offer[1],
        );
        assert_eq!(engine.game_mut().game_mut().state, GameState::Playing);
    }

    /// **A movement key let go under the level-up menu stops the wizard.**
    ///
    /// `ArrowDown` is also the menu's own "select the next item", so while a
    /// menu is up the pump claims it — and the release used to go no further
    /// than the pump. The game was told about the press and never about the
    /// release, so picking an upgrade handed control back to a wizard still
    /// walking south with nothing pressed. The assertion is where he is after
    /// the menu closes, which is the thing the player sees.
    #[test]
    fn a_move_key_let_go_under_the_level_up_menu_stops_the_player() {
        let mut engine = scripted(&headless(256));
        engine.frame().expect("a frame");
        run_frames(&mut engine, 1);
        let window = engine.window();

        let before = engine.game().render_state().player;
        engine
            .shell_mut()
            .key_press(window, KeyCode::ArrowDown)
            .expect("the window is live");
        run_frames(&mut engine, 4);
        let walking = engine.game().render_state().player;
        assert_ne!(walking, before, "holding Down never moved the wizard");

        // Level up with the key still down, and let go under the menu.
        engine
            .game_mut()
            .game_mut()
            .bank_xp(game::xp_for_next_level(1));
        run_frames(&mut engine, 2);
        assert_eq!(engine.menu_kind(), MenuKind::LevelUp);
        engine
            .shell_mut()
            .key_release(window, KeyCode::ArrowDown)
            .expect("the window is live");
        run_frames(&mut engine, 1);

        tap(&mut engine, KeyCode::Digit1);
        run_frames(&mut engine, 2);
        assert_eq!(engine.menu_kind(), MenuKind::None);

        let settled = engine.game().render_state().player;
        run_frames(&mut engine, 8);
        assert_eq!(
            engine.game().render_state().player,
            settled,
            "the wizard walked on with nothing pressed",
        );
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
            engine.game_mut().game_mut().enemy_count(),
            2_000,
            "the prefill did not reach the simulation",
        );
        let stats = engine.gpu().scene_stats();
        assert_eq!(stats.field, 2_000, "the renderer was handed an empty field");
        assert!(
            stats.culled > 0 && stats.drawn > 1,
            "a field of 2000 over a 96x72 arena should be partly on screen: {stats:?}",
        );
        assert_eq!(
            stats.batches, 3,
            "the ground, the scenery, then one sheet of actors — no bolt is in \
             the air on the first frame",
        );
        assert!(stats.props > 0, "the arena was dealt no scenery");

        // A run with no prefill draws the handful of enemies a fifteenth of a
        // second produces, which is what says the assertion above is about the
        // flag and not about the game.
        let mut plain = scripted(&headless(4));
        plain.frame().expect("a frame");
        assert!(
            plain.gpu().scene_stats().field < 10,
            "an unprefilled run already had a crowd: {:?}",
            plain.gpu().scene_stats(),
        );
    }

    /// **The scene section reaches the panel**, so the numbers this sample's
    /// whole claim rests on are readable in the running game rather than only
    /// from a test.
    #[test]
    fn the_debug_panel_carries_this_samples_own_scene_section() {
        let mut engine = scripted(&headless_with(8, |common| {
            common.debug_overlay = Some(false)
        }));
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

    /// **The same rule on the title screen, where the centre button is `PLAY`.**
    ///
    /// A separate test from the paused one because the menu is a different
    /// layout with a different first item, and because the consequence is worse:
    /// `PLAY` is bound to [`RESTART_KEY`], the one edge that both starts a
    /// waiting run and restarts a live one. A harness that clicked the centre to
    /// hand over keyboard focus therefore *started* the run, and the `Space` it
    /// pressed next restarted it — which is what
    /// `web/tools/browser-e2e.mjs` did, and why the Pages gate failed on horde
    /// about two runs in three while the other three demos passed.
    ///
    /// This is the fast half of that fix. The gate now clicks a corner like its
    /// own group E always did; this goes red in seconds if the title menu ever
    /// grows into the corner and makes that corner a button again.
    #[test]
    fn a_focusing_click_off_every_button_leaves_the_title_screen_up() {
        // `at_the_title_screen`, not `scripted` — the latter queues the start
        // edge, so its first tick starts the run and a click that changed
        // nothing would still read as one that started it.
        let mut engine = at_the_title_screen(&headless(64));
        engine.frame().expect("a frame");
        assert_eq!(
            engine.game_mut().game_mut().state,
            GameState::WaitingToStart
        );

        let layout = engine.menu_layout().expect("the title menu").clone();
        assert!(
            !layout
                .items()
                .iter()
                .any(|item| item.min.x <= 3.0 && item.min.y <= 3.0),
            "the title menu reaches the corner of the screen",
        );
        click(&mut engine, Vec2::new(3.0, 3.0));
        assert_eq!(
            engine.game_mut().game_mut().state,
            GameState::WaitingToStart,
            "a click in the corner started the run",
        );

        // …and the centre of `PLAY` really does start it, or the check above
        // passes on a menu nothing can click.
        let target = layout.items()[0];
        click(&mut engine, (target.min + target.max) * 0.5);
        assert_eq!(
            engine.game_mut().game_mut().state,
            GameState::Playing,
            "PLAY did not start the run",
        );
    }
}
