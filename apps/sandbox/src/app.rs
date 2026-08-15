//! The sandbox as the engine hosts it: shell events in, fixed ticks and
//! presented frames out.
//!
//! `docs/plan/01-foundations.md` §1.4 asks for exactly this — "shell window,
//! event loop, raw surface handle plumbed to where the HAL surface will be
//! created" — and the shape is the one `crcbl-shell`'s crate docs fix:
//!
//! ```text
//! loop {                                  // the outer loop is *ours*
//!     shell.pump(&mut |event| …);         // drain what arrived
//!     clock.update(time.elapsed());
//!     while clock.consume_tick() { tick(dt); }
//!     render(clock.alpha());
//! }
//! ```
//!
//! **That loop is [`crcbl::engine::Loop`]'s now**, and this file is what a game
//! plugs into it: [`Sandbox`], with the pause menu's two settings rows, and the
//! two empty call sites [`tick`] and [`render`] that everything later grows
//! around. The sandbox is the honest measure of how much of a frame the engine
//! owns — a game with nothing in it still runs, pauses, opens a menu, goes
//! fullscreen and reports a summary.
//!
//! There is still no `Shell::run(closure)` and there never will be: on wasm the
//! outer loop is `requestAnimationFrame`, which calls the engine and cannot be
//! called by it. [`crcbl::engine::drive`] owns a native `loop {}`; the body it
//! wraps is one `Loop::frame`, which would be the `rAF` callback unchanged.
//!
//! # Determinism, and why the clock is injected
//!
//! `--headless` must produce the same tick count on every machine, or the CI
//! job asserting it is a coin toss. So the time source is a choice, not a
//! constant: a headless run advances a
//! [`ManualTime`] by a fixed step per frame and
//! a windowed run reads [`MonotonicTime`].
//! `crcbl-core` made that injectable at P0.2 precisely so this loop would not
//! have to grow a `#[cfg(test)]` branch, and nothing below this comment knows
//! which one it got.
//!
//! # What "no window system" means here
//!
//! On macOS and Windows no shell backend is compiled in
//! ([`Backend`] has the entries; the registry
//! has no registration until P14), so a windowed run cannot start.
//! [`open`] says so with
//! [`ShellError::NoBackend`], and [`run`]
//! turns that into a diagnostic naming `--headless` rather than a panic — see
//! [`SandboxError::NoWindowSystem`]. `--headless` itself works on every
//! platform, which is what keeps the cross-platform CI leg meaningful.

use std::time::Duration;

use crcbl::backend::GpuBackend;
// The loop's scaffolding — the clock, the event sink, the configure wait, the
// exit vocabulary and the four timing constants — lives in `crcbl::engine`,
// shared with `apps/breakout`. It was two copies of the same code, and only one
// of them carried the rationale.
//
// `WINDOWED_IDLE` is advisory: `Shell::wait_events` may return immediately, but
// on the backends that honour it this is the difference between an idle sandbox
// using 4% of a core and using all of one.
//
// `MAX_CONSECUTIVE_RECONFIGURES` is what makes `--frames N` terminate when the
// swapchain never becomes presentable — a budget of *presented* frames cannot.
use crcbl::engine::{
    Booted, Clock, ExitReason, FrameInfo, FrameLimit, GpuOptions, HostedGame, LoopConfig, Pacing,
    RunSummary, wait_for_configure,
};
use crcbl::prelude::*;
use crcbl::shell::{DisplayMode, LogicalSize, ShellBackend as Backend, open, open_backend};
use crcbl::ui::draw_list::DrawList;

use crate::gpu::Gpu;
use crate::menu::{self, Menus, SandboxAction};

/// Which projection the camera uses.
///
/// **Milestone 5, entire.** `docs/plan/02-vulkan-backend.md`'s rung 5 is
/// "orthographic camera mode proving the 2D story (z = z-index) is just a
/// projection matrix swap", and this enum is the proof's user-facing half:
/// [`CameraMode::projection`] is the only place the two differ, and nothing
/// downstream of it — not the pipeline, not the shader, not the render graph —
/// is told which one was chosen.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum CameraMode {
    /// Infinite-far reversed-Z perspective. The 3D default.
    #[default]
    Perspective,
    /// Reversed-Z orthographic. The 2D mode, where world z is a z-index and a
    /// larger one draws on top.
    Orthographic,
}

impl CameraMode {
    /// Parses `perspective` / `ortho` / `orthographic`.
    #[must_use]
    pub fn from_name(name: &str) -> Option<Self> {
        match name {
            "perspective" | "persp" => Some(Self::Perspective),
            "ortho" | "orthographic" => Some(Self::Orthographic),
            _ => None,
        }
    }

    /// The projection this mode means.
    ///
    /// The orthographic half-height is chosen so the unit cube fills a similar
    /// share of the frame as it does under the default perspective camera, which
    /// is what makes the two modes comparable at a glance rather than one of
    /// them looking broken.
    #[must_use]
    pub fn projection(self) -> crcbl::render::Projection {
        match self {
            Self::Perspective => crcbl::render::Projection::default(),
            Self::Orthographic => crcbl::render::Projection::Orthographic {
                half_height: 0.9,
                near: 0.1,
                far: 100.0,
            },
        }
    }
}

/// How the sandbox was asked to run.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Options {
    /// Run against [`HeadlessShell`](crcbl::shell::HeadlessShell) with a
    /// hand-driven clock instead of opening a window.
    pub headless: bool,
    /// GPU backend to force, or `None` to let [`crcbl::backend`]'s own table
    /// choose.
    ///
    /// `None` deliberately does **not** mean "null": see that module's docs for
    /// why a silent fallback to a backend that renders nothing is the wrong
    /// default.
    pub backend: Option<GpuBackend>,
    /// Stop after this many presented frames. `None` means "until the window
    /// closes", which is never the case headless — see [`Options::frames`].
    pub frames: Option<u64>,
    /// Simulation rate.
    pub tick_hz: u32,
    /// Window title.
    pub title: String,
    /// Which projection the camera uses — milestone 5.
    pub camera: CameraMode,
    /// Whether to open the window borderless rather than windowed.
    ///
    /// A *request*. `F11` toggles from either starting point, and a window
    /// system is free to refuse both — see [`Options::display_mode`].
    pub fullscreen: bool,
    /// Whether the debug overlay starts visible, or `None` for the default.
    ///
    /// Three-valued because the default is not a constant:
    /// `docs/plan/sample/00-samples-overview.md` rule 4 is "on by default in dev
    /// builds", so `None` means [`Options::debug_overlay_visible`]'s
    /// `cfg!(debug_assertions)` and either flag overrides it.
    pub debug_overlay: Option<bool>,
    /// How presented frames are paced against the display.
    ///
    /// The same value [`crcbl::args::Common::pacing`] carries for the four
    /// games; the sandbox predates that shared parser and still has its own.
    pub pacing: Pacing,
    /// The most frames a second the loop will run.
    ///
    /// The same value [`crcbl::args::Common::limit`] carries. It is the only
    /// pacing there is under [`Pacing::Off`], and under vsync it rarely fires.
    pub limit: FrameLimit,
    /// After the first tick, wait once for a present id the swapchain was
    /// never given (`u64::MAX`) and log whether the device answered at once.
    ///
    /// The wayland e2e harness's probe of the id guard on
    /// `wait_until_presented` with a real swapchain. Off by default so an
    /// ordinary run never blocks.
    pub wait_unpresented: bool,
}

impl Default for Options {
    fn default() -> Self {
        Self {
            headless: false,
            backend: None,
            frames: None,
            tick_hz: 60,
            title: "Crucible sandbox".to_string(),
            camera: CameraMode::default(),
            fullscreen: false,
            debug_overlay: None,
            pacing: Pacing::default(),
            limit: FrameLimit::default(),
            wait_unpresented: false,
        }
    }
}

impl Options {
    /// What the command line contributes to opening a GPU.
    ///
    /// The same value [`crcbl::args::Common::gpu`] gives the four games.
    #[must_use]
    pub const fn gpu(&self) -> GpuOptions {
        GpuOptions {
            backend: self.backend,
            pacing: self.pacing,
        }
    }

    /// The frame budget actually used: a headless run always has one, because a
    /// headless window is never closed by a user and a CI job must terminate.
    #[must_use]
    pub fn frame_budget(&self) -> Option<u64> {
        match (self.frames, self.headless) {
            (Some(frames), _) => Some(frames),
            (None, true) => Some(120),
            (None, false) => None,
        }
    }

    /// Whether the debug overlay starts visible.
    #[must_use]
    pub fn debug_overlay_visible(&self) -> bool {
        self.debug_overlay.unwrap_or(cfg!(debug_assertions))
    }

    /// The mode to create the window in.
    ///
    /// The same answer [`crcbl::args::Common::display_mode`] gives the four
    /// games; the sandbox predates that shared parser and still has its own.
    #[must_use]
    pub const fn display_mode(&self) -> DisplayMode {
        if self.fullscreen {
            DisplayMode::Borderless { monitor: None }
        } else {
            DisplayMode::Windowed
        }
    }
}

/// What a completed run did. Printed by `main`, asserted by the tests.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Summary {
    /// Which shell backend ran.
    pub backend: Backend,
    /// Frames presented.
    pub frames: u64,
    /// Fixed simulation steps executed.
    pub ticks: u64,
    /// Shell events observed, of every kind.
    pub events: u64,
    /// The swapchain's size when the loop stopped.
    pub extent: (u32, u32),
    /// Why it stopped.
    pub exit: ExitReason,
    /// Whether the simulation was stopped when the loop ended.
    pub paused: bool,
    /// The mode the window system actually had the window in, **not** the one
    /// the run last asked for. A summary that reported the request would say
    /// "borderless" for every compositor that refused.
    pub mode: DisplayMode,
}

/// Anything that can stop the sandbox before it starts.
///
/// An alias rather than an enum: [`crcbl::engine::LoopError`] owns these
/// variants for every sample. The sandbox has no simulation of its own to
/// fail, so it takes the default type parameter and its `Game` variant is
/// [`Infallible`](core::convert::Infallible) — uninhabited, and free.
pub type SandboxError = crcbl::engine::LoopError;

/// The sandbox, as the engine's loop hosts it.
///
/// **A game with no game in it, and that is the point.** The sandbox exists to
/// show the engine's frame with nothing of its own in the way: no simulation, no
/// HUD, no score. What is left is [`tick`] and [`render`], both empty and both
/// load-bearing — the call sites are what everything later grows around.
///
/// Its only state is the pause menu's two settings rows: the pacing and frame
/// limit they show, and the copies that have reached the GPU and the clock.
#[derive(Debug)]
pub struct Sandbox {
    /// The pacing the menu's row shows, applied to the GPU on the first tick
    /// after it changes.
    pacing: Pacing,
    /// The pacing already applied to the GPU, so `tick` changes it once per
    /// press rather than querying the surface every tick.
    applied: Pacing,
    /// The frame limit the menu's row shows; a change is handed to the loop
    /// through [`take_pending_frame_limit`](HostedGame::take_pending_frame_limit).
    limit: FrameLimit,
    /// A limit the menu asked for since the loop last read it.
    pending_limit: Option<FrameLimit>,
    /// The values the pause panel was last built for — `None` until the
    /// first pause, so the panel is always rebuilt once with the real values.
    shown: Option<(Pacing, FrameLimit)>,
    /// Whether `--wait-unpresented` was asked for: the one-shot probe below
    /// runs on the first tick and records its outcome here.
    wait_unpresented: bool,
    /// The outcome of the probe, once it has run — `Ok` with the time the
    /// device took, `Err` with the formatted error.
    unpresented: Option<Result<Duration, String>>,
}

impl Sandbox {
    /// A sandbox starting from the command line's pacing, frame limit and
    /// `--wait-unpresented` probe.
    #[must_use]
    pub fn new(pacing: Pacing, limit: FrameLimit, wait_unpresented: bool) -> Self {
        Self {
            pacing,
            applied: pacing,
            limit,
            pending_limit: None,
            shown: None,
            wait_unpresented,
            unpresented: None,
        }
    }
}

/// The loop the sandbox runs in.
///
/// A type alias, because the loop is the engine's.
///
/// # Why the shell is a type parameter that defaults to `dyn Shell`
///
/// `dyn` is the primary form and [`run`] uses it: the backend is a *runtime*
/// choice, so a windowed run and a headless run must be the same code. But
/// scripting a compositor — "now resize, now ask to close" — is
/// [`HeadlessShell`](crcbl::shell::HeadlessShell)'s own API and not part of the
/// seam, so a test that wants it needs the concrete type back. A default type
/// parameter gives both with no downcasting and no `Any` on the trait: `Loop`
/// still means `Loop<dyn Shell>` everywhere it is written unadorned.
pub type Loop<S = dyn Shell> = crcbl::engine::Loop<S, Sandbox>;

/// Opens a window (or does not), runs the loop, and tears everything down.
///
/// # Errors
///
/// [`SandboxError`] if no shell backend opened, if the window was never
/// configured, or if the HAL seam failed.
/// Teardown runs on **every** path, including a failing frame: neither `Loop`
/// nor `Gpu` has a `Drop` impl to fall back on, and while the `crcbl-vk` ones do
/// reclaim the objects, they log "N object(s) still alive at device teardown"
/// while doing it — which is the diagnostic for a real leak.
pub fn run(options: &Options) -> Result<Summary, SandboxError> {
    crcbl::engine::drive(start(options)?)
}

/// Opens the shell the environment asks for and builds the loop on it.
///
/// # Errors
///
/// [`SandboxError`] if the shell, the window or the HAL seam refused.
pub fn start(options: &Options) -> Result<Loop, SandboxError> {
    let shell = if options.headless {
        // By name, never by fallback: `crcbl-shell`'s registry deliberately
        // refuses to auto-select headless, because a game that silently ran
        // without a window would look like a hang.
        open_backend(Backend::Headless).map_err(SandboxError::Shell)?
    } else {
        open().map_err(SandboxError::NoWindowSystem)?
    };
    with_shell(shell, options)
}

/// The body of [`start`], once a shell exists.
///
/// # Errors
///
/// [`SandboxError`] if the window is never configured or the HAL seam fails.
pub fn with_shell<S: Shell + ?Sized>(
    mut shell: Box<S>,
    options: &Options,
) -> Result<Loop<S>, SandboxError> {
    let clock_source = Clock::new(options.headless);
    crcbl::log::info!(
        "shell: {} backend, caps {:?}",
        shell.backend(),
        shell.caps()
    );

    // Once, at startup, so input timestamps and frame timestamps share an
    // origin. Without it every `EventTime` is offset by however long the
    // process took to get here.
    shell.align_event_clock(clock_source.elapsed());

    let window = shell.create_window(&WindowDesc {
        title: &options.title,
        app_id: "sh.kryptic.crcbl.sandbox",
        size: LogicalSize::new(1280.0, 720.0),
        // Asked for at creation rather than switched to afterwards, so
        // `--fullscreen` does not show a decorated window first.
        mode: options.display_mode(),
        ..WindowDesc::default()
    })?;

    let mut events = 0;
    let extent = wait_for_configure(shell.as_mut(), window, &mut events)?;

    let gpu = Gpu::open(
        shell.as_ref(),
        window,
        extent,
        options.gpu(),
        options.camera.projection(),
    )?;

    Ok(Loop::new(
        Booted {
            shell,
            window,
            gpu,
            clock_source,
            events,
        },
        Sandbox::new(options.pacing, options.limit, options.wait_unpresented),
        LoopConfig {
            tick_hz: options.tick_hz,
            frames: options.frame_budget(),
            debug_overlay: options.debug_overlay_visible(),
            windowed: !options.headless,
            limit: options.limit,
        },
    ))
}

/// The sandbox's half of the frame, which is as little as a game can have.
///
/// Two of the seven do anything at all, and neither does much: [`tick`] and
/// [`render`] are the empty call sites P1's renderer grows into, and the cube's
/// spin is on the fixed timestep so a `--headless --frames N` run is a
/// bit-reproducible picture on every machine.
impl HostedGame for Sandbox {
    /// The sandbox has no simulation, so it has nothing of its own to fail at.
    /// [`LoopError`](crcbl::engine::LoopError)'s `Game` variant is uninhabited
    /// as a result, which is the type system agreeing.
    type Error = core::convert::Infallible;
    type Gpu = Gpu;
    /// Paused or not, which is the sandbox's whole state machine.
    type MenuKind = bool;
    /// The pause menu's two settings rows, which are the only actions of its
    /// own.
    type MenuAction = SandboxAction;
    type Summary = Summary;

    const NAME: &'static str = "sandbox";

    fn menus() -> Menus {
        menu::menus()
    }

    fn tick(&mut self, gpu: &mut Gpu, tick_dt: f64) {
        // The `--wait-unpresented` probe: one direct wait, on the first tick,
        // for a present id the swapchain was never given. The wayland e2e
        // harness asserts the success line on a driver with present feedback;
        // the outcome is kept for the unit test below.
        if self.wait_unpresented && self.unpresented.is_none() {
            self.unpresented = Some(match gpu.wait_unpresented() {
                Ok(elapsed) => {
                    crcbl::log::info!(
                        "sandbox: an unpresented present id was answered at once on a real swapchain (wait took {:.3}s)",
                        elapsed.as_secs_f64(),
                    );
                    Ok(elapsed)
                }
                Err(error) => {
                    crcbl::log::error!(
                        "sandbox: an unpresented present id was NOT answered at once ({error}); the id guard is gone"
                    );
                    Err(error.to_string())
                }
            });
        }
        tick(tick_dt);
        // The cube spins on the **fixed** timestep, not on the frame rate. That
        // is what makes `--headless --frames N` render a bit-reproducible
        // picture on every machine, and therefore what makes a golden image of
        // it evidence rather than a coincidence.
        #[allow(clippy::cast_possible_truncation)]
        gpu.advance(tick_dt as f32);
        // The menu's pacing row changes this only while paused; the change
        // lands on the first tick after resume, and only when it differs —
        // re-querying the surface every tick is what `set_pacing` costs.
        if self.pacing != self.applied {
            if let Err(error) = gpu.set_pacing(self.pacing) {
                crcbl::log::warn!("sandbox: pacing {:?} refused: {error}", self.pacing);
            }
            // One attempt per press either way: a failed rebuild keeps the old
            // swapchain, and retrying it every tick would be a warn per tick.
            self.applied = self.pacing;
        }
    }

    /// The sandbox binds no keys of its own: the three the loop reserves are
    /// the three it has.
    fn key_event(&mut self, _key: crcbl::core::input::KeyCode, _pressed: bool) {}

    /// The action a widget id of this game's names; the mapping lives in the
    /// menu module, which owns the ids.
    fn menu_action(id: crcbl::ui::WidgetId) -> Option<SandboxAction> {
        menu::action_for(id)
    }

    fn apply(&mut self, action: SandboxAction) {
        match action {
            SandboxAction::CyclePacing => self.pacing = next_pacing(self.pacing),
            SandboxAction::CycleLimit => {
                self.limit = next_limit(self.limit);
                self.pending_limit = Some(self.limit);
            }
        }
    }

    fn menu_kind(&mut self, menus: &mut Menus, paused: bool) -> bool {
        if paused && self.shown != Some((self.pacing, self.limit)) {
            // A row's label changed (or this is the first pause): rebuild the
            // panel with the values in force, restoring the selection so a
            // press on a row does not throw the player back to the top.
            let selected = menus
                .current()
                .and_then(crcbl::ui::menu::Menu::selected_item)
                .map(|item| item.id);
            menus.replace(true, menu::pause_menu(self.pacing, self.limit));
            if let Some(id) = selected {
                menus
                    .current_mut()
                    .expect("the pause menu is in the set")
                    .select_id(id);
            }
            self.shown = Some((self.pacing, self.limit));
        }
        paused
    }

    fn take_pending_frame_limit(&mut self) -> Option<FrameLimit> {
        self.pending_limit.take()
    }

    fn draw(&mut self, _gpu: &mut Gpu, _draw_list: &mut DrawList, frame: FrameInfo) {
        // `alpha` is read after the tick loop, never before: before, the
        // accumulator may still hold whole ticks. `FrameInfo` is handed over
        // after `run_ticks` for exactly that reason.
        render(frame.alpha);
    }

    fn summary(&self, run: RunSummary) -> Summary {
        Summary {
            backend: run.backend,
            frames: run.frames,
            ticks: run.ticks,
            events: run.events,
            extent: run.extent,
            exit: run.exit,
            paused: run.paused,
            mode: run.mode,
        }
    }

    fn log_summary(summary: &Summary) {
        crcbl::log::info!(
            "sandbox: {} frames, {} ticks on the {} shell at {}x{} ({:?})",
            summary.frames,
            summary.ticks,
            summary.backend,
            summary.extent.0,
            summary.extent.1,
            summary.exit,
        );
    }
}

/// One fixed simulation step.
///
/// Empty on purpose: `crcbl-ecs` and `crcbl-phys` are P2 and P3. What matters
/// at P0 is that the *call site* exists and is driven by the accumulator rather
/// than by the frame rate, so nothing later grows around a variable-dt loop.
fn tick(dt: f64) {
    let _ = dt;
}

/// One rendered frame's worth of interpolation.
///
/// Also empty, and also load-bearing: `alpha` is what a renderer lerps between
/// the last two simulated states with, and reading it here — after the tick
/// loop — is the ordering P1's renderer inherits.
fn render(alpha: f32) {
    let _ = alpha;
}

/// The next pacing a press of the menu's row selects: a cycle through the
/// four variants, wrapping.
#[must_use]
fn next_pacing(pacing: Pacing) -> Pacing {
    match pacing {
        Pacing::Auto => Pacing::Vsync,
        Pacing::Vsync => Pacing::Adaptive,
        Pacing::Adaptive => Pacing::Off,
        Pacing::Off => Pacing::Auto,
    }
}

/// The next frame limit a press of the menu's row selects, up the ladder and
/// wrapping at "unlimited". A value the ladder does not contain (a `--fps
/// 144` start) climbs to the next rung and joins the cycle there.
#[must_use]
fn next_limit(limit: FrameLimit) -> FrameLimit {
    const LADDER: [u32; 5] = [30, 60, 120, 240, 1000];
    let rate = limit.rate();
    match LADDER.into_iter().find(|rung| *rung > rate) {
        Some(rung) => FrameLimit::fps(rung),
        None => FrameLimit::unlimited(),
    }
}

#[cfg(test)]
mod tests {
    // The six keys the loop reserves are `crcbl::engine`'s, and always were the
    // same in every sample: F3 opens the panel, Escape pauses, F11 asks for
    // fullscreen, and the menu's three navigate it. The sandbox used to declare
    // all six itself — "switching it on is one thing" is only true if it is the
    // *same* thing in every sample, and two declarations is how that stops being
    // true. Only the tests name them now, because the loop that reads them is
    // the engine's.
    use crcbl::engine::{
        DEBUG_OVERLAY_KEY, FULLSCREEN_KEY, Flow, MENU_ACTIVATE_KEY, MENU_DOWN_KEY, PAUSE_KEY,
    };
    use crcbl::shell::HeadlessShell;

    use super::*;

    /// A loop over a *concrete* `HeadlessShell`, so the test can play
    /// compositor. `run` uses `dyn Shell`; both go through the same
    /// [`Loop::with_shell`].
    fn scripted(options: &Options) -> Loop<HeadlessShell> {
        with_shell(Box::new(HeadlessShell::new()), options).expect("headless always starts")
    }

    /// Always `--backend null`. These tests run on the macOS and Windows CI
    /// legs, where there is no Vulkan loader at all, and they are about the
    /// *loop* — determinism, tick pacing, resize plumbing — not about a driver.
    /// The Vulkan path is covered by `crcbl-vk`'s own e2e suite and by the
    /// sandbox runs in the three harness scripts.
    fn headless(frames: u64) -> Options {
        Options {
            headless: true,
            backend: Some(GpuBackend::Null),
            frames: Some(frames),
            tick_hz: 60,
            title: "test".to_string(),
            camera: CameraMode::Perspective,
            fullscreen: false,
            debug_overlay: None,
            pacing: Pacing::default(),
            limit: FrameLimit::default(),
            wait_unpresented: false,
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

    /// **The sandbox can turn the panel on too, and F3 is all it takes.**
    ///
    /// Rule 4 applies to the sandbox as much as to a game, and the sandbox had
    /// no UI pass at all before this — which is exactly the finding the rule
    /// exists to surface. It has no HUD and never will; the overlay is the whole
    /// of its UI.
    #[test]
    fn f3_toggles_the_debug_overlay_in_the_sandbox() {
        let mut engine = scripted(&Options {
            debug_overlay: Some(false),
            ..headless(16)
        });
        let window = engine.window();

        engine.frame().expect("a frame");
        engine.frame().expect("a frame");
        assert!(
            ui_text(&engine).is_empty(),
            "the sandbox draws no UI at all while the panel is off",
        );
        assert!(
            !engine.gpu().last_dump().contains("ui-composite"),
            "and declares no UI pass either:\n{}",
            engine.gpu().last_dump(),
        );

        engine
            .shell_mut()
            .key_press(window, DEBUG_OVERLAY_KEY)
            .expect("the window is live");
        engine.frame().expect("a frame");
        let drawn = ui_text(&engine);
        for row in ["frame", "fps", "avg", "worst", "window"] {
            assert!(drawn.iter().any(|t| t == row), "missing {row}: {drawn:?}");
        }

        // **The numbers come from the clock, not from nowhere.** A frame that
        // never fed the window would draw the same labels with 0.00 ms beside
        // them, which is the failure a "the rows are present" assertion misses.
        // The first frame's interval is the clock's zero-length sentinel and is
        // dropped, so three frames leave two samples of the headless step.
        assert_eq!(engine.debug().frame.len(), 2, "two real intervals so far");
        assert_eq!(
            engine.debug().frame.mean(),
            crcbl::engine::HEADLESS_FRAME_STEP,
            "the window holds the clock's own step",
        );
        assert_eq!(row_value(&drawn, "avg"), "16.67 ms");
        assert_eq!(row_value(&drawn, "window"), "2/120");
        assert_eq!(row_value(&drawn, "fps"), "60.0");

        // And the panel is composed of exactly the modules the sandbox has: the
        // frame's, plus the renderer's when the device has timestamp queries.
        // There is no connection here at all, so there is no network module —
        // the stronger version of breakout's and flappy's in-memory one.
        let titles: Vec<&str> = engine
            .debug()
            .panel
            .sections()
            .iter()
            .map(crcbl::ui::DebugSection::title)
            .collect();
        let expected: &[&str] = if engine.gpu().timings().is_some() {
            &["frame", "gpu", "counters"]
        } else {
            &["frame", "counters"]
        };
        assert_eq!(titles, expected, "no module appears that no system offered");

        // **And it reaches the GPU.** `UiRenderer::add_pass` declares nothing
        // when the draw list is empty, so the pass's presence in the frame's
        // graph is the difference between "the overlay was drawn" and "the
        // overlay was composited onto the frame the player sees".
        assert!(
            engine.gpu().last_dump().contains("ui-composite"),
            "the UI pass must be in the frame:\n{}",
            engine.gpu().last_dump(),
        );

        engine
            .shell_mut()
            .key_press(window, DEBUG_OVERLAY_KEY)
            .expect("the window is live");
        engine.frame().expect("a frame");
        assert!(
            ui_text(&engine).is_empty(),
            "F3 again must take it away: {:?}",
            ui_text(&engine),
        );
        engine.finish(ExitReason::FrameBudget).expect("teardown");
    }

    /// The `--wait-unpresented` probe runs once, on the first tick, and every
    /// backend answers it at once — a null device has no present feedback at
    /// all, so this pins the *plumbing* (flag → Sandbox → Gpu → Device); the
    /// guard's half is the wayland e2e's, on a real swapchain.
    #[test]
    fn the_unpresented_id_probe_answers_at_once() {
        let mut engine = scripted(&Options {
            wait_unpresented: true,
            ..headless(8)
        });
        // The first frame only establishes the clock's baseline — the probe
        // runs on the first *tick*, which is the second frame.
        engine.frame().expect("a frame");
        engine.frame().expect("a frame");
        match engine.game().unpresented.as_ref() {
            Some(Ok(elapsed)) => assert!(
                *elapsed < Duration::from_secs(5),
                "the probe must not block: {elapsed:?}",
            ),
            other => panic!("the probe must have run and answered Ok: {other:?}"),
        }
    }

    /// The CI-visible promise: a headless run terminates, and terminates with
    /// the *same* numbers every time. If this ever flakes, the loop has grown a
    /// dependency on wall-clock time.
    #[test]
    fn a_headless_run_is_deterministic() {
        let first = run(&headless(30)).expect("headless runs everywhere");
        let second = run(&headless(30)).expect("headless runs everywhere");
        assert_eq!(first, second, "two identical runs must agree exactly");
        assert_eq!(first.backend, Backend::Headless);
        assert_eq!(first.frames, 30);
        assert_eq!(first.exit, ExitReason::FrameBudget);
        // 30 frames at a 1/60 s step, with the first update only establishing
        // the baseline: 29 steps of 16.666 ms at a 16.666 ms tick.
        assert_eq!(first.ticks, 29);
    }

    /// The tick count follows the *clock*, not the frame count — the whole
    /// reason for a fixed-timestep accumulator.
    #[test]
    fn ticks_are_paced_by_the_clock_not_the_frame_rate() {
        let thirty = run(&Options {
            tick_hz: 30,
            ..headless(62)
        })
        .expect("headless runs everywhere");
        let sixty = run(&headless(62)).expect("headless runs everywhere");
        assert_eq!(sixty.frames, thirty.frames, "same number of frames");
        // 61 steps of 1/60 s: 61 ticks at 60 Hz, half as many at 30 Hz. The
        // frame count is one higher than the tick count because the clock's
        // first update only establishes a baseline.
        assert_eq!(sixty.ticks, 61);
        assert_eq!(thirty.ticks, 30, "half the rate, half the ticks");
    }

    /// The ordering constraint the shell seam exists to enforce: no size until
    /// the window system configures the window, and no swapchain before that.
    #[test]
    fn the_loop_waits_for_the_first_configure_before_touching_the_hal() {
        let engine = scripted(&headless(1));
        // The HAL picked an sRGB format from the surface's caps, which it could
        // only do after a surface existed, which needed the window.
        assert!(
            engine.gpu().format().is_srgb(),
            "{:?}",
            engine.gpu().format()
        );
        assert!(engine.events() >= 1, "the first configure is an event");
        engine.finish(ExitReason::FrameBudget).expect("teardown");
    }

    /// Resize arrives as an event and reaches the swapchain. Scripted through
    /// `HeadlessShell`, which is what it is for.
    #[test]
    fn a_resize_reconfigures_the_swapchain() {
        let mut engine = scripted(&headless(10));
        assert_eq!(engine.gpu().extent(), (1280, 720));
        engine.frame().expect("a frame");

        let window = engine.window();
        engine
            .shell_mut()
            .resize(window, PhysicalSize::new(1920, 1080))
            .expect("the window is live");
        engine.frame().expect("a frame after the resize");
        assert_eq!(engine.gpu().extent(), (1920, 1080));

        engine.finish(ExitReason::FrameBudget).expect("teardown");
    }

    /// A close request stops the loop, and is *answered* — the window stays
    /// open until it is.
    #[test]
    fn a_close_request_stops_the_loop() {
        let mut engine = scripted(&headless(1000));
        let window = engine.window();
        engine
            .shell_mut()
            .request_close(window)
            .expect("the window is live");
        assert_eq!(
            engine.frame().expect("a frame"),
            Flow::Stop(ExitReason::CloseRequested)
        );
        let summary = engine
            .finish(ExitReason::CloseRequested)
            .expect("teardown after a close");
        assert_eq!(summary.exit, ExitReason::CloseRequested);
    }

    /// The frame budget defaults exist so a headless CI job cannot hang, and so
    /// a windowed run is not silently capped.
    #[test]
    fn only_a_headless_run_gets_a_default_frame_budget() {
        assert_eq!(Options::default().frame_budget(), None);
        assert_eq!(
            Options {
                headless: true,
                ..Options::default()
            }
            .frame_budget(),
            Some(120)
        );
        assert_eq!(
            Options {
                frames: Some(7),
                ..Options::default()
            }
            .frame_budget(),
            Some(7)
        );
    }
    // ---- focus, pause and fullscreen ----------------------------------------

    /// Runs `frames` frames, and insists every one of them was a real frame.
    ///
    /// `Loop::frame` answers a spent frame budget with `Ok(Flow::Stop)`
    /// **before** it pumps, so a test that let one through would go on
    /// injecting key events into a loop that had stopped reading them.
    fn run_frames(engine: &mut Loop<HeadlessShell>, frames: u32) {
        for _ in 0..frames {
            assert_eq!(
                engine.frame().expect("a frame"),
                Flow::Continue,
                "the loop stopped early",
            );
        }
    }

    /// **Losing focus stops the simulation, and the cube stops with it.**
    ///
    /// `Gpu::elapsed` rather than the tick counter: the counter is the thing
    /// the pause branch skips incrementing, so asserting on it alone would pass
    /// with the tick still running. The cube's angle is what a player sees.
    #[test]
    fn losing_focus_pauses_and_the_cube_stops_where_it_was() {
        let mut engine = scripted(&headless(400));
        let window = engine.window();
        run_frames(&mut engine, 20);
        let spun = engine.gpu().elapsed();
        assert!(spun > 0.0, "the cube has to be spinning first");

        engine
            .shell_mut()
            .set_focus(window, false)
            .expect("the window is live");
        engine.frame().expect("a frame");
        assert!(engine.is_paused(), "an unfocused window is not simulating");

        let ticks = engine.ticks();
        run_frames(&mut engine, 120);
        assert_eq!(engine.ticks(), ticks, "a paused frame ran a tick");
        assert_eq!(
            engine.gpu().elapsed(),
            spun,
            "120 paused frames turned the cube",
        );

        // Regaining focus does not resume; Escape does.
        engine
            .shell_mut()
            .set_focus(window, true)
            .expect("the window is live");
        run_frames(&mut engine, 5);
        assert!(engine.is_paused(), "focus coming back must not resume");

        engine
            .shell_mut()
            .key_press(window, PAUSE_KEY)
            .expect("the window is live");
        run_frames(&mut engine, 10);
        assert!(!engine.is_paused());
        assert!(
            engine.gpu().elapsed() > spun,
            "resuming did not restart the simulation",
        );
        engine.finish(ExitReason::FrameBudget).expect("teardown");
    }

    /// **A long pause does not lurch on resume.** A headless frame advances the
    /// clock by exactly `HEADLESS_FRAME_STEP`, so 300 paused frames are five
    /// seconds of wall time the simulation did not experience. See the tick
    /// loop for the two alternatives and why they both spend the first resumed
    /// frame running eight ticks.
    #[test]
    fn resuming_after_a_long_pause_runs_one_tick_not_a_catch_up_burst() {
        let mut engine = scripted(&headless(2_000));
        let window = engine.window();
        run_frames(&mut engine, 10);

        engine
            .shell_mut()
            .key_press(window, PAUSE_KEY)
            .expect("the window is live");
        engine.frame().expect("a frame");
        assert!(engine.is_paused());

        let paused_at = engine.ticks();
        run_frames(&mut engine, 300);
        assert_eq!(engine.ticks(), paused_at, "a paused frame ran a tick");

        engine
            .shell_mut()
            .key_press(window, PAUSE_KEY)
            .expect("the window is live");
        let before = engine.ticks();
        engine.frame().expect("a frame");
        assert!(!engine.is_paused());
        let burst = engine.ticks() - before;
        assert!(
            burst <= 1,
            "the first frame after a five-second pause ran {burst} ticks",
        );
        engine.finish(ExitReason::FrameBudget).expect("teardown");
    }

    /// The pause menu is drawn, and it is drawn through the UI pass the debug
    /// overlay already uses.
    #[test]
    fn a_paused_frame_draws_the_pause_menu() {
        let mut engine = scripted(&headless(60));
        let window = engine.window();
        run_frames(&mut engine, 2);
        assert!(
            !ui_text(&engine).iter().any(|t| t == "PAUSED"),
            "nothing is paused yet",
        );

        engine
            .shell_mut()
            .key_press(window, PAUSE_KEY)
            .expect("the window is live");
        engine.frame().expect("a frame");
        let drawn = ui_text(&engine);
        assert!(drawn.iter().any(|t| t == "PAUSED"), "{drawn:?}");
        assert!(drawn.iter().any(|t| t == "RESUME"), "{drawn:?}");
        assert!(drawn.iter().any(|t| t == "PACING: AUTO"), "{drawn:?}");
        assert!(drawn.iter().any(|t| t == "FPS: 1000"), "{drawn:?}");
        // And the picture reached the *other* pass: the scrim, the nine-slice
        // frame, and nine quads for each of five buttons.
        let sprites = engine.gpu().menu_sprites();
        assert_eq!(sprites.len(), 1 + 9 + 9 * 5, "{}", sprites.len());
        let extent = engine.gpu().extent();
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

    /// **A running sandbox submits no menu sprites at all**, which is what makes
    /// the menu pass free rather than cheap.
    #[test]
    fn a_running_sandbox_draws_no_menu() {
        let mut engine = scripted(&headless(60));
        run_frames(&mut engine, 4);
        assert!(
            engine.gpu().menu_sprites().is_empty(),
            "a running frame submitted {} menu sprites",
            engine.gpu().menu_sprites().len(),
        );
        engine.finish(ExitReason::FrameBudget).expect("teardown");
    }

    /// **Keyboard activation works through the real loop.** Escape opens the
    /// pause menu, Enter fires `RESUME`, and the sandbox is running again — with
    /// no pointer anywhere in the story.
    #[test]
    fn enter_on_the_pause_menu_resumes() {
        let mut engine = scripted(&headless(60));
        let window = engine.window();
        run_frames(&mut engine, 2);
        engine
            .shell_mut()
            .key_press(window, PAUSE_KEY)
            .expect("the window is live");
        engine.frame().expect("a frame");
        assert!(engine.is_paused());

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

    /// The pause menu's PACING row cycles the pacing, and the change reaches
    /// the GPU on the first tick after resume.
    #[test]
    fn the_pacing_row_cycles_and_reaches_the_gpu_on_resume() {
        let mut engine = scripted(&headless(200));
        let window = engine.window();
        run_frames(&mut engine, 2);
        assert_eq!(engine.gpu().pacing(), Pacing::default());

        engine
            .shell_mut()
            .key_press(window, PAUSE_KEY)
            .expect("the window is live");
        engine.frame().expect("a frame");
        assert!(engine.is_paused());

        // Down three items to the PACING row; Enter cycles Auto → Vsync.
        for _ in 0..3 {
            engine
                .shell_mut()
                .key_press(window, MENU_DOWN_KEY)
                .expect("the window is live");
        }
        engine.frame().expect("a frame");
        engine
            .shell_mut()
            .key_press(window, MENU_ACTIVATE_KEY)
            .expect("the window is live");
        engine.frame().expect("a frame");
        engine
            .shell_mut()
            .key_release(window, MENU_ACTIVATE_KEY)
            .expect("the window is live");
        engine.frame().expect("a frame");

        assert_eq!(engine.game().pacing, Pacing::Vsync);
        assert!(
            ui_text(&engine).iter().any(|text| text == "PACING: VSYNC"),
            "the row's label must show the new value",
        );
        // Still paused, no tick has run yet — the change lands on resume.
        assert_eq!(engine.gpu().pacing(), Pacing::default());

        engine
            .shell_mut()
            .key_press(window, PAUSE_KEY)
            .expect("the window is live");
        run_frames(&mut engine, 3);
        assert!(!engine.is_paused());
        assert_eq!(
            engine.gpu().pacing(),
            Pacing::Vsync,
            "the first tick after resume must apply the new pacing",
        );
        engine.finish(ExitReason::FrameBudget).expect("teardown");
    }

    /// The pause menu's FPS row changes the loop's frame limit mid-run, on a
    /// *real* clock — the limiter lives on that one, so a headless run's
    /// manual clock is swapped for a real one the way the engine's own test
    /// does.
    #[test]
    fn the_fps_row_changes_the_loops_frame_limit_mid_run() {
        let mut engine = scripted(&headless(200));
        // The limiter lives on Clock::Real by construction; a headless run
        // gets a manual clock that takes no limit, so swap a real one in to
        // make the row's effect observable the way it is on a desktop.
        *engine.clock_source_mut() = Clock::new(false);
        let window = engine.window();
        run_frames(&mut engine, 2);

        engine
            .shell_mut()
            .key_press(window, PAUSE_KEY)
            .expect("the window is live");
        engine.frame().expect("a frame");
        assert!(engine.is_paused());

        // Down four items to the FPS row; Enter cycles 1000 → unlimited.
        for _ in 0..4 {
            engine
                .shell_mut()
                .key_press(window, MENU_DOWN_KEY)
                .expect("the window is live");
        }
        engine.frame().expect("a frame");
        engine
            .shell_mut()
            .key_press(window, MENU_ACTIVATE_KEY)
            .expect("the window is live");
        engine.frame().expect("a frame");
        engine
            .shell_mut()
            .key_release(window, MENU_ACTIVATE_KEY)
            .expect("the window is live");
        engine.frame().expect("a frame");

        assert_eq!(engine.game().limit, FrameLimit::unlimited());
        assert!(
            ui_text(&engine).iter().any(|text| text == "FPS: UNLIMITED"),
            "the row's label must show the new value",
        );
        assert_eq!(
            engine.clock_source().limit(),
            Some(FrameLimit::unlimited()),
            "the loop must apply the row's change to its clock",
        );
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
            "the second button resumed the sandbox, so the selection never moved",
        );
        engine.finish(ExitReason::FrameBudget).expect("teardown");
    }

    /// **A click fires the button under it**, through the same action path the
    /// keyboard uses — and a click that started somewhere else fires nothing.
    #[test]
    fn a_click_on_resume_resumes() {
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

    /// **F11 twice is where it started, and the loop reports the mode the
    /// window system gave it rather than the one it asked for.**
    #[test]
    fn fullscreen_toggles_twice_back_to_windowed() {
        let mut engine = scripted(&headless(200));
        let window = engine.window();
        run_frames(&mut engine, 2);
        assert_eq!(engine.display_mode(), DisplayMode::Windowed);
        let windowed_extent = engine.gpu().extent();

        engine
            .shell_mut()
            .key_press(window, FULLSCREEN_KEY)
            .expect("the window is live");
        run_frames(&mut engine, 6);
        assert_eq!(
            engine.display_mode(),
            DisplayMode::Borderless { monitor: None },
        );
        assert!(
            engine
                .shell_mut()
                .window_state(window)
                .expect("state")
                .mode_request_honoured()
        );
        assert_ne!(engine.gpu().extent(), windowed_extent);

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
        assert!(!summary.paused);
    }

    /// **A backend that refuses reports the mode it really has.**
    #[test]
    fn a_refused_fullscreen_is_reported_as_the_mode_the_window_actually_has() {
        let mut engine = scripted(&headless(200));
        let window = engine.window();
        run_frames(&mut engine, 2);
        let windowed = engine.gpu().extent();

        engine
            .shell_mut()
            .key_press(window, FULLSCREEN_KEY)
            .expect("the window is live");
        engine.frame().expect("a frame");
        // The compositor answers with a windowed configure instead.
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
