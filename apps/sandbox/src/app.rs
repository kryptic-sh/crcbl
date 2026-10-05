//! The sandbox as the engine hosts it: shell events in, fixed ticks and
//! presented frames out.
//!
//! The foundations stage asked for exactly this — "shell window, event loop,
//! raw surface handle plumbed to where the HAL surface will be created" — and
//! the shape is the one `crcbl-shell`'s crate docs fix:
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
//! plugs into it: [`Sandbox`], with the pause menu's two settings rows, the
//! scene its fixed tick advances ([`crate::scene`]), and the empty call site
//! [`render`] that everything later grows around. The sandbox is the honest
//! measure of how much of a frame the engine owns — a game with nothing in it
//! still runs, pauses, opens a menu, goes fullscreen and reports a summary.
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
// An idle windowed sandbox is kept off a whole core by its pacing, not by a
// fixed idle: the default present mode waits on the display, and without vsync
// `Loop::frame` hands `Shell::wait_events` the time until the frame limiter's
// next deadline (`Clock::idle`).
//
// `MAX_CONSECUTIVE_RECONFIGURES` is what makes `--frames N` terminate when the
// swapchain never becomes presentable — a budget of *presented* frames cannot.
use crcbl::engine::{
    Booted, Clock, FrameInfo, FrameLimit, GpuOptions, HostedGame, LoopConfig, Pacing, RunSummary,
    wait_for_configure,
};
use crcbl::prelude::*;

use crate::lan::{Lan, LanError, SimRoute};
use crate::steam::SteamLink;
use crcbl::render::RenderEffects;
use crcbl::shell::{DisplayMode, PhysicalSize, ShellBackend as Backend, open, open_backend};
use crcbl::ui::draw_list::DrawList;

use crate::gpu::Gpu;
use crate::menu::{self, MenuKind, Menus, SandboxAction};
use crate::scene::Scene;

/// Which projection the camera uses.
///
/// **Milestone 5, entire.** Stage 2's rung 5 is
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
    /// Requested window extent in physical pixels.
    ///
    /// The sandbox's 1280x720 default predates the shared 960x720 sample
    /// default, so this is a value rather than an `Option` that would fall back
    /// through [`crcbl::engine::requested_window_size`].
    pub size: PhysicalSize,
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
    /// Host, join or look for a LAN session — see [`crate::lan`]. Native
    /// builds only: web builds have no networking.
    #[cfg(not(target_arch = "wasm32"))]
    pub lan: crate::lan::LanMode,
    /// Record the hosted session to this new `.crpl` file — see
    /// `crcbl::replay_record`. Native builds only.
    #[cfg(not(target_arch = "wasm32"))]
    pub record: Option<std::path::PathBuf>,
    /// Open on the LAN lobby — see `crate::lobby`.
    ///
    /// [`crate::args::parse`] sets it for a command line that chose no
    /// session and is not a script (`--headless`, `--frames`), so every CI
    /// run and harness starts where it always did. **`false` by default**, so
    /// `Options` built in code opens on the cube. Native builds only: web
    /// builds have no networking.
    #[cfg(not(target_arch = "wasm32"))]
    pub lobby: bool,
}

impl Default for Options {
    fn default() -> Self {
        Self {
            headless: false,
            backend: None,
            frames: None,
            tick_hz: 60,
            title: "Crucible sandbox".to_string(),
            size: PhysicalSize::new(1280, 720),
            camera: CameraMode::default(),
            fullscreen: false,
            debug_overlay: None,
            pacing: Pacing::default(),
            limit: FrameLimit::default(),
            wait_unpresented: false,
            #[cfg(not(target_arch = "wasm32"))]
            lan: crate::lan::LanMode::Off,
            #[cfg(not(target_arch = "wasm32"))]
            record: None,
            #[cfg(not(target_arch = "wasm32"))]
            lobby: false,
        }
    }
}

impl Options {
    /// What the command line contributes to opening a GPU.
    ///
    /// The same value [`crcbl::args::Common::gpu`] gives every sample that
    /// takes the shared flags.
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
    /// The half of the report every sample shares.
    pub run: RunSummary,
    /// Which of topic 18's effects the frames were drawn through, **resolved**.
    ///
    /// Read back off the renderer rather than copied from the request the
    /// context handed it — see [`Gpu::effects`](crate::gpu::Gpu::effects). It is
    /// the only observable this sample has for its own
    /// `renderer.set_effect_request(ctx.effect_request())`: without it that line
    /// could be deleted and every test here would stay green.
    pub effects: RenderEffects,
}

/// Anything that can stop the sandbox before it starts.
///
/// An alias rather than an enum: [`crcbl::engine::LoopError`] owns these
/// variants for every sample. The sandbox has no simulation of its own to
/// fail; its `Game` variant is a LAN session that could not start
/// ([`LanError`]), which in a web build is uninhabited, and free.
pub type SandboxError = crcbl::engine::LoopError<LanError>;

/// The sandbox, as the engine's loop hosts it.
///
/// **A game with no game in it, and that is the point.** The sandbox exists to
/// show the engine's frame with nothing of its own in the way: no game rules, no
/// HUD, no score. What is left is the scene the debug panel inspects — a cube
/// and a light as entities ([`crate::scene`]) — and [`render`], empty and
/// load-bearing: the call site is what everything later grows around.
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
    /// The cube and the light as entities, and the debug panel's selection
    /// over them. See [`crate::scene`].
    scene: Scene,
    /// What the frames are being drawn with, re-read each
    /// [`HostedGame::draw`] off the renderer.
    ///
    /// Every frame rather than once at start-up, so the field is about the
    /// frames rather than about a value copied before any of them ran.
    effects: RenderEffects,
    /// Steam, when the `steam` feature is on and a windowed run started it;
    /// inert otherwise. See [`crate::steam`].
    steam: SteamLink,
    /// The LAN session `--host`, `--join` or `--browse` started, or the
    /// lobby did; inert otherwise. See [`crate::lan`].
    lan: Lan,
    /// The lobby, while it is on screen — see `crate::lobby`. Every key and
    /// every character is its own while it is. Native only.
    #[cfg(not(target_arch = "wasm32"))]
    lobby: Option<crate::lobby::Lobby>,
    /// The lobby a join was picked from, set aside while the session it
    /// started runs: when that session ends the player is back in it — see
    /// `Sandbox::follow_session`. Native only.
    #[cfg(not(target_arch = "wasm32"))]
    parked: Option<crate::lobby::Lobby>,
}

impl Sandbox {
    /// A sandbox starting from the command line's pacing, frame limit and
    /// `--wait-unpresented` probe, drawing `effects`, with its scene lit as
    /// `light`.
    ///
    /// `effects` is the resolved set the device and the player's settings left
    /// standing, read off the renderer by the caller — a run that stops before
    /// its first frame still reports what it would have drawn. `light` is the
    /// renderer's opening light, which the scene's sun starts as and then owns.
    #[must_use]
    pub fn new(
        pacing: Pacing,
        limit: FrameLimit,
        wait_unpresented: bool,
        effects: RenderEffects,
        light: crcbl::render::DirectionalLight,
    ) -> Self {
        Self {
            pacing,
            applied: pacing,
            limit,
            pending_limit: None,
            shown: None,
            wait_unpresented,
            unpresented: None,
            // Timed on the real clock always, not only while the panel shows:
            // two clock reads a system a tick, and the mean is warm when F3
            // opens it. This sample has no browser build (`web/demos` holds
            // none), so `MonotonicTime`'s `Instant` is a clock it has.
            scene: Scene::new(
                light,
                Some(Box::new(crcbl::core::time::MonotonicTime::new())),
            ),
            effects,
            steam: SteamLink::off(),
            lan: Lan::off(),
            #[cfg(not(target_arch = "wasm32"))]
            lobby: None,
            #[cfg(not(target_arch = "wasm32"))]
            parked: None,
        }
    }

    /// Follows the session a lobby join started: once the host admits this
    /// player the lobby steps aside, set aside until the session ends; a join
    /// that ends first leaves the lobby saying why, and a session that ends
    /// later brings it back saying how. A session the command line started
    /// has no lobby to go back to, and `crcbl::lan` has logged how it ended.
    #[cfg(not(target_arch = "wasm32"))]
    fn follow_session(&mut self) {
        use crate::lan::Standing;

        match self.lan.standing() {
            None | Some(Standing::Joining) => {}
            Some(Standing::InSession) => {
                if let Some(lobby) = self.lobby.take() {
                    self.parked = Some(lobby);
                }
            }
            Some(Standing::Over(how)) => {
                if let Some(lobby) = &mut self.lobby {
                    lobby.model_mut().join_failed(&how);
                    self.lan = Lan::off();
                } else if let Some(mut lobby) = self.parked.take() {
                    lobby.model_mut().session_ended(&how);
                    self.lan = Lan::off();
                    self.lobby = Some(lobby);
                }
            }
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
        size: options.size.to_logical(1.0),
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

    // Read before the GPU moves into the loop, and off the renderer rather than
    // from the request: the device clamps last, so this is what the frames will
    // actually be drawn with.
    let effects = gpu.effects();

    let mut sandbox = Sandbox::new(
        options.pacing,
        options.limit,
        options.wait_unpresented,
        effects,
        gpu.light,
    );
    // Windowed runs only: a headless run is CI's, and must neither need nor
    // touch a developer's Steam client.
    if !options.headless {
        sandbox.steam = SteamLink::start();
    }
    // Headless too: a `--headless --host` run is a host with no window.
    #[cfg(not(target_arch = "wasm32"))]
    {
        sandbox.lan = Lan::start(
            options.lan,
            options.tick_hz,
            options.record.as_deref(),
            options.headless,
        )
        .map_err(SandboxError::Game)?;
        if options.lobby {
            sandbox.lobby = Some(crate::lobby::Lobby::on_the_lan(options.tick_hz));
        }
    }
    let steam_pads = sandbox.steam.pad_source();

    let mut engine = Loop::new(
        Booted {
            shell,
            window,
            gpu,
            clock_source,
            events,
        },
        sandbox,
        LoopConfig {
            tick_hz: options.tick_hz,
            frames: options.frame_budget(),
            debug_overlay: options.debug_overlay_visible(),
            windowed: !options.headless,
            limit: options.limit,
            // Sandbox parses its own flags and offers no `--exec`.
            exec: Vec::new(),
        },
    );
    if steam_pads.is_some() {
        engine.set_pad_source(steam_pads);
    }
    Ok(engine)
}

/// The sandbox's half of the frame, which is as little as a game can have.
///
/// Two of the seven do anything at all, and neither does much: [`render`] is the
/// empty call site P1's renderer grows into, and the scene's tick spins the
/// cube on the fixed timestep so a `--headless --frames N` run is a
/// bit-reproducible picture on every machine.
impl HostedGame for Sandbox {
    /// The sandbox has no simulation, so all it can fail at is starting a LAN
    /// session; in a web build, which has none, the type is uninhabited.
    type Error = LanError;
    type Gpu = Gpu;
    /// Paused, in the lobby, or neither.
    type MenuKind = MenuKind;
    /// The pause menu's two settings rows and the lobby's rows.
    type MenuAction = SandboxAction;
    type Summary = Summary;

    const NAME: &'static str = "sandbox";

    fn menus() -> Menus {
        menu::menus()
    }

    /// `sv_spin_rate`, the one simulation variable; see [`crate::spin`].
    fn console_table() -> crcbl::console::Table {
        crate::spin::console_table()
    }

    /// To the LAN session's host when there is a session, and otherwise to
    /// the scene, which applies it at the start of its next tick.
    fn submit_sim_set(&mut self, set: crcbl::console::SimSet) -> Result<(), crcbl::console::Fault> {
        match self.lan.route_sim_set(set)? {
            SimRoute::Offline(set) => self.scene.submit_sim_set(set),
            #[cfg(not(target_arch = "wasm32"))]
            SimRoute::Sent => {}
        }
        Ok(())
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
        // The cube spins on the **fixed** timestep, not on the frame rate. That
        // is what makes `--headless --frames N` render a bit-reproducible
        // picture on every machine, and therefore what makes a golden image of
        // it evidence rather than a coincidence. The scene owns the spin and
        // the light; the GPU is handed both.
        self.scene.tick(tick_dt);
        for reply in self.scene.take_replies() {
            crcbl::log::console::print(&reply.to_string());
        }
        // In a LAN session the cube drawn is the session's — the host's own,
        // or what it replicated — so a rate the host set is the rate seen.
        if let Some(seconds) = self.lan.cube_seconds() {
            self.scene.set_cube_seconds(seconds);
        }
        if let Some(seconds) = self.scene.cube_seconds() {
            gpu.set_elapsed(seconds);
        }
        if let Some(light) = self.scene.light() {
            gpu.light = light;
        }
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

    /// The debug panel's selection keys (see [`crate::scene`]), then the
    /// Steam lobby keys, when the `steam` feature is live; see
    /// [`crate::steam`]. While the LAN lobby is up, every key is its own.
    fn key_event(&mut self, key: crcbl::core::input::KeyCode, pressed: bool) {
        #[cfg(not(target_arch = "wasm32"))]
        if let Some(lobby) = &mut self.lobby {
            lobby.model_mut().key(key, pressed);
            return;
        }
        if self.scene.key(key, pressed) {
            return;
        }
        self.steam.key_event(key, pressed);
    }

    /// The lobby's connect address, typed. Nothing else here takes text.
    #[cfg(not(target_arch = "wasm32"))]
    fn text_event(&mut self, text: &str) {
        if let Some(lobby) = &mut self.lobby {
            lobby.model_mut().text(text);
        }
    }

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
            #[cfg(not(target_arch = "wasm32"))]
            SandboxAction::Offline => {
                self.lobby = None;
                self.lan = Lan::off();
            }
            #[cfg(not(target_arch = "wasm32"))]
            SandboxAction::Lobby(pick) => {
                let Some(lobby) = &mut self.lobby else {
                    return;
                };
                // Any pick replaces a join under way, whether or not it
                // starts anything, as the lobby forgets it too.
                self.lan = Lan::off();
                match lobby.pick(pick) {
                    Some(crate::lobby::Started::Hosting(lan)) => {
                        self.lan = lan;
                        self.lobby = None;
                    }
                    // The lobby stays up, saying where, until the host
                    // admits this player.
                    Some(crate::lobby::Started::Joining(lan)) => self.lan = lan,
                    // The lobby shows why on the next frame.
                    None => {}
                }
            }
        }
    }

    fn menu_kind(&mut self, menus: &mut Menus, paused: bool) -> MenuKind {
        if !paused {
            #[cfg(not(target_arch = "wasm32"))]
            if let Some(lobby) = &mut self.lobby {
                return show_lobby(lobby, menus);
            }
            return MenuKind::Running;
        }
        if self.shown != Some((self.pacing, self.limit)) {
            // A row's label changed (or this is the first pause): rebuild the
            // panel with the values in force, restoring the selection so a
            // press on a row does not throw the player back to the top.
            let selected = menus
                .current()
                .and_then(crcbl::ui::menu::Menu::selected_item)
                .map(|item| item.id);
            menus.replace(MenuKind::Paused, menu::pause_menu(self.pacing, self.limit));
            if let Some(id) = selected {
                menus
                    .current_mut()
                    .expect("the pause menu is in the set")
                    .select_id(id);
            }
            self.shown = Some((self.pacing, self.limit));
        }
        MenuKind::Paused
    }

    /// The "scene" section and the selected entity's systems' (see
    /// [`crate::scene`]), the "steam" section, when the `steam` feature is
    /// live, and the "lan" and "net" ones during a LAN session.
    fn debug_sections(&self, panel: &mut crcbl::ui::DebugPanel) {
        self.scene.debug_sections(panel);
        self.steam.debug_sections(panel);
        self.lan.debug_sections(panel);
    }

    fn take_pending_frame_limit(&mut self) -> Option<FrameLimit> {
        self.pending_limit.take()
    }

    /// Steam, lent to the loop to pump; see [`crate::steam`].
    #[cfg(all(
        feature = "steam",
        target_pointer_width = "64",
        any(target_os = "linux", target_os = "windows", target_os = "macos")
    ))]
    fn steam(&mut self) -> Option<&mut dyn crcbl::engine::SteamSource> {
        self.steam.source()
    }

    /// Every event the loop's pump decoded.
    #[cfg(all(
        feature = "steam",
        target_pointer_width = "64",
        any(target_os = "linux", target_os = "windows", target_os = "macos")
    ))]
    fn steam_event(&mut self, event: &crcbl::steam::SteamEvent) {
        self.steam.event(event);
    }

    fn draw(&mut self, gpu: &mut Gpu, _draw_list: &mut DrawList, frame: FrameInfo) {
        // `alpha` is read after the tick loop, never before: before, the
        // accumulator may still hold whole ticks. `FrameInfo` is handed over
        // after `run_ticks` for exactly that reason.
        render(frame.alpha);
        // Here because `draw` is the one hook that runs on every frame, paused
        // or not, after the loop's Steam pump: a call answered while paused is
        // still taken.
        self.steam.frame();
        // And the LAN session, on wall time for the same reason: a paused
        // host that stopped reading its peers would time every one of them
        // out.
        self.lan.frame(frame.render_dt);
        #[cfg(not(target_arch = "wasm32"))]
        self.follow_session();
        // Re-read rather than kept: the device clamps last, so what the summary
        // reports comes back off the renderer.
        self.effects = gpu.effects();
    }

    fn summary(&self, run: RunSummary) -> Summary {
        Summary {
            run,
            effects: self.effects,
        }
    }

    fn log_summary(summary: &Summary) {
        crcbl::log::info!(
            "sandbox: {} frames, {} ticks on the {} shell at {}x{}, effects {} ({:?})",
            summary.run.frames,
            summary.run.ticks,
            summary.run.backend,
            summary.run.extent.0,
            summary.run.extent.1,
            summary.effects.row(),
            summary.run.exit,
        );
    }
}

/// Polls `lobby` and puts its panel in `menus`, rebuilt first when what it
/// lists changed — carrying the selection across by id, onto the first row
/// if what it was on is gone — and on the connect row once text arrives.
#[cfg(not(target_arch = "wasm32"))]
fn show_lobby(lobby: &mut crate::lobby::Lobby, menus: &mut Menus) -> MenuKind {
    use crate::menu::CONNECT_ID;

    lobby.model_mut().poll();
    if lobby.model_mut().take_changed() {
        let selected = menus
            .get_mut(MenuKind::Lobby)
            .and_then(|menu| menu.selected_item().map(|item| item.id));
        let mut panel = lobby.menu();
        if let Some(id) = selected {
            panel.select_id(id);
        }
        menus.replace(MenuKind::Lobby, panel);
    }
    if let Some(panel) = menus.get_mut(MenuKind::Lobby) {
        panel.set_item_hint(CONNECT_ID, lobby.connect_hint());
        if lobby.model_mut().take_typed() {
            panel.select_id(CONNECT_ID);
        }
    }
    MenuKind::Lobby
}

/// One rendered frame's worth of interpolation.
///
/// Also empty, and also load-bearing: `alpha` is what a renderer lerps between
/// the last two simulated states with, and reading it here — after the tick
/// loop — is the ordering P1's renderer inherits.
fn render(alpha: f32) {
    let _ = alpha;
}

/// The next pacing a press of the menu's row selects: a cycle through
/// [`Pacing`]'s variants, wrapping.
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
        DEBUG_OVERLAY_KEY, ExitReason, FULLSCREEN_KEY, Flow, MENU_ACTIVATE_KEY, MENU_DOWN_KEY,
        PAUSE_KEY,
    };
    use crcbl::shell::HeadlessShell;

    use super::*;
    use crcbl_sample_test::{row_value, ui_images, ui_text};

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
            size: PhysicalSize::new(1280, 720),
            camera: CameraMode::Perspective,
            fullscreen: false,
            debug_overlay: None,
            pacing: Pacing::default(),
            limit: FrameLimit::default(),
            wait_unpresented: false,
            #[cfg(not(target_arch = "wasm32"))]
            lan: crate::lan::LanMode::Off,
            #[cfg(not(target_arch = "wasm32"))]
            record: None,
            #[cfg(not(target_arch = "wasm32"))]
            lobby: false,
        }
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
            ui_text(engine.gpu().draw_list()).is_empty(),
            "the sandbox draws no UI at all while the panel is off",
        );
        let dump = engine.gpu().last_dump();
        assert!(
            !dump.contains("ui-composite") && !dump.contains("ui-overlay"),
            "and declares neither half of the UI pass either:\n{dump}",
        );

        engine
            .shell_mut()
            .key_press(window, DEBUG_OVERLAY_KEY)
            .expect("the window is live");
        engine.frame().expect("a frame");
        let drawn = ui_text(engine.gpu().draw_list());
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
        // frame's, plus the renderer's when the device has timestamp queries,
        // and the scene's. There is no connection here at all, so there is no
        // network module — the stronger version of breakout's and flappy's
        // in-memory one — and nothing is selected, so no system's section.
        let titles: Vec<&str> = engine
            .debug()
            .panel
            .sections()
            .iter()
            .map(crcbl::ui::DebugSection::title)
            .collect();
        let expected: &[&str] = if engine.gpu().timings().is_some() {
            &[
                "frame",
                "gpu",
                "counters",
                crate::scene::SCENE_SECTION,
                crate::scene::SYSTEMS_SECTION,
            ]
        } else {
            &[
                "frame",
                "counters",
                crate::scene::SCENE_SECTION,
                crate::scene::SYSTEMS_SECTION,
            ]
        };
        assert_eq!(titles, expected, "no module appears that no system offered");

        // Each system's row is timed on the real clock the sample hands its
        // scene, so it carries a time rather than `UNTIMED` or `PENDING`.
        for system in [crate::scene::SPIN, crate::scene::SUN] {
            let time = row_value(&drawn, system);
            assert!(
                time.ends_with(" ms") && time.contains(", avg "),
                "{system}: {time:?}"
            );
        }

        // **And it reaches the GPU, in the half that draws over a menu.**
        // `UiRenderer::add_passes` declares nothing for an empty half, so which
        // pass is in the frame's graph says both that the overlay was
        // composited onto the frame the player sees *and* which layer it landed
        // in. The sandbox draws no HUD at all, so the half below the menu is
        // empty and `ui-composite` must be absent — the debug panel is overlay
        // and nothing else is.
        let dump = engine.gpu().last_dump();
        assert!(
            dump.contains("ui-overlay"),
            "the overlay half of the UI pass must be in the frame:\n{dump}",
        );
        assert!(
            !dump.contains("ui-composite"),
            "the sandbox draws no HUD, so the half under the menu is empty:\n{dump}",
        );

        engine
            .shell_mut()
            .key_press(window, DEBUG_OVERLAY_KEY)
            .expect("the window is live");
        engine.frame().expect("a frame");
        assert!(
            ui_text(engine.gpu().draw_list()).is_empty(),
            "F3 again must take it away: {:?}",
            ui_text(engine.gpu().draw_list()),
        );
        engine.finish(ExitReason::FrameBudget).expect("teardown");
    }

    /// **Selecting an entity in the panel shows each owning system's fields**,
    /// through the loop: the key reaches the scene, the selection outlives the
    /// frame it was made on, and the rows are the live component's.
    #[test]
    fn a_selected_entity_shows_its_systems_fields_in_the_panel() {
        use crate::scene::{SELECT_NEXT_KEY, SPIN, SUN};

        let mut engine = scripted(&Options {
            debug_overlay: Some(true),
            ..headless(32)
        });
        let window = engine.window();
        // By section title, not by drawn text: the systems section has a row
        // labelled with each system's name too.
        let titles = |engine: &Loop<HeadlessShell>| -> Vec<String> {
            engine
                .debug()
                .panel
                .sections()
                .iter()
                .map(|section| section.title().to_owned())
                .collect()
        };
        run_frames(&mut engine, 2);
        let drawn = ui_text(engine.gpu().draw_list());
        assert_eq!(row_value(&drawn, "selected"), "none", "{drawn:?}");
        assert!(!titles(&engine).iter().any(|title| title == SPIN));

        engine
            .shell_mut()
            .key_press(window, SELECT_NEXT_KEY)
            .expect("the window is live");
        run_frames(&mut engine, 1);
        engine
            .shell_mut()
            .key_release(window, SELECT_NEXT_KEY)
            .expect("the window is live");
        run_frames(&mut engine, 3);

        let drawn = ui_text(engine.gpu().draw_list());
        assert!(
            titles(&engine).iter().any(|title| title == SPIN),
            "the cube's spin system has a section: {:?}",
            titles(&engine)
        );
        assert!(!titles(&engine).iter().any(|title| title == SUN));
        assert_eq!(
            row_value(&drawn, "seconds"),
            format!("{:.3}", engine.gpu().elapsed()),
            "the row is the seconds the cube was last drawn at",
        );
        assert_ne!(row_value(&drawn, "selected"), "none");
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
        assert_eq!(first.run.backend, Backend::Headless);
        assert_eq!(first.run.frames, 30);
        assert_eq!(first.run.exit, ExitReason::FrameBudget);
        // 30 frames at a 1/60 s step, with the first update only establishing
        // the baseline: 29 steps of 16.666 ms at a 16.666 ms tick.
        assert_eq!(first.run.ticks, 29);
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
        assert_eq!(sixty.run.frames, thirty.run.frames, "same number of frames");
        // 61 steps of 1/60 s: 61 ticks at 60 Hz, half as many at 30 Hz. The
        // frame count is one higher than the tick count because the clock's
        // first update only establishes a baseline.
        assert_eq!(sixty.run.ticks, 61);
        assert_eq!(thirty.run.ticks, 30, "half the rate, half the ticks");
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
        assert_eq!(summary.run.exit, ExitReason::CloseRequested);
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

    /// **Offline, an `sv_spin_rate` set reaches the scene and the drawn cube
    /// turns at it** — the route a typed set takes once the loop hands it to
    /// the game (the loop's half is `crcbl`'s own test).
    #[test]
    fn an_offline_spin_rate_set_turns_the_drawn_cube_at_the_new_rate() {
        let mut engine = scripted(&headless(400));
        run_frames(&mut engine, 10);
        let set = crate::spin::sim_registry()
            .sim_set("sv_spin_rate", "2")
            .expect("in range");
        HostedGame::submit_sim_set(engine.game_mut(), set).expect("offline takes it");
        // One frame lets the set's own tick boundary pass.
        run_frames(&mut engine, 1);
        let (ticks, spun) = (engine.ticks(), engine.gpu().elapsed());
        run_frames(&mut engine, 30);
        let ran = engine.ticks() - ticks;
        assert!(ran > 0, "the frames ran no tick");
        let per_tick = (engine.gpu().elapsed() - spun) / ran as f32;
        assert!(
            (per_tick - 2.0 / 60.0).abs() < 1e-5,
            "the cube turned {per_tick} a tick, not twice a tick's length"
        );
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
            !ui_text(engine.gpu().draw_list())
                .iter()
                .any(|t| t == "PAUSED"),
            "nothing is paused yet",
        );

        engine
            .shell_mut()
            .key_press(window, PAUSE_KEY)
            .expect("the window is live");
        engine.frame().expect("a frame");
        let drawn = ui_text(engine.gpu().draw_list());
        assert!(drawn.iter().any(|t| t == "PAUSED"), "{drawn:?}");
        assert!(drawn.iter().any(|t| t == "RESUME"), "{drawn:?}");
        assert!(drawn.iter().any(|t| t == "PACING: AUTO"), "{drawn:?}");
        assert!(drawn.iter().any(|t| t == "FPS: 1000"), "{drawn:?}");
        // And the picture is in the list too: the scrim, the nine-slice frame,
        // and nine quads for each of five buttons.
        let images = ui_images(engine.gpu().draw_list());
        assert_eq!(images.len(), 1 + 9 + 9 * 5, "{}", images.len());
        let extent = engine.gpu().extent();
        assert_eq!(
            images[0],
            [0.0, 0.0, extent.0 as f32, extent.1 as f32],
            "the scrim does not cover the framebuffer",
        );
        engine.finish(ExitReason::FrameBudget).expect("teardown");
    }

    /// **A running sandbox draws no menu at all**: no image quad reaches the
    /// draw list.
    #[test]
    fn a_running_sandbox_draws_no_menu() {
        let mut engine = scripted(&headless(60));
        run_frames(&mut engine, 4);
        let images = ui_images(engine.gpu().draw_list());
        assert!(
            images.is_empty(),
            "a running frame drew {} image quads",
            images.len(),
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
            ui_text(engine.gpu().draw_list())
                .iter()
                .any(|text| text == "PACING: VSYNC"),
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
            ui_text(engine.gpu().draw_list())
                .iter()
                .any(|text| text == "FPS: UNLIMITED"),
            "the row's label must show the new value",
        );
        assert_eq!(
            engine.clock_source().limit(),
            FrameLimit::unlimited(),
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
        assert_eq!(summary.run.mode, DisplayMode::Windowed);
        assert!(!summary.run.paused);
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
        assert_eq!(summary.run.mode, DisplayMode::Windowed);
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

    // ---- the lobby ------------------------------------------------------------

    /// A loop opened on `lobby`: `Options` built in code never opens one,
    /// and the lobby a parsed command line opens queries the broadcast
    /// address.
    #[cfg(not(target_arch = "wasm32"))]
    fn in_a_lobby(lobby: crate::lobby::Lobby) -> Loop<HeadlessShell> {
        let mut engine = scripted(&headless(4000));
        engine.game_mut().lobby = Some(lobby);
        engine
    }

    /// A lobby that is not browsing, hosting on loopback.
    #[cfg(not(target_arch = "wasm32"))]
    fn lobby_alone() -> crate::lobby::Lobby {
        use crate::lobby::tests::{TICK_HZ, on_loopback};

        crate::lobby::Lobby::new(
            crcbl::lan::lobby::Lobby::new(crate::lan::SANDBOX, Err("not looking".to_string())),
            Ok(crcbl::net::PlayerId::from_seed(1)),
            on_loopback(),
            TICK_HZ,
        )
    }

    /// Runs frames of `engine`, serving `host` between them, until `done`
    /// holds, failing past the lobby tests' frame bound.
    #[cfg(not(target_arch = "wasm32"))]
    fn serve_until(
        engine: &mut Loop<HeadlessShell>,
        host: &mut crcbl::lan::LanHost,
        what: &str,
        done: impl Fn(&Loop<HeadlessShell>) -> bool,
    ) {
        use crate::lobby::tests::{FRAME, MAX_FRAMES, PAUSE};

        let mut now = Duration::ZERO;
        for _ in 0..MAX_FRAMES {
            if done(engine) {
                return;
            }
            now += FRAME;
            host.frame(now);
            run_frames(engine, 1);
            std::thread::sleep(PAUSE);
        }
        panic!("no {what} within {MAX_FRAMES} frames");
    }

    /// The lobby's lines under its title, while it is the panel up.
    #[cfg(not(target_arch = "wasm32"))]
    fn lobby_lines(engine: &Loop<HeadlessShell>) -> Vec<crcbl::ui::menu::Caption> {
        engine
            .menus()
            .current()
            .filter(|menu| menu.title == crate::lobby::TITLE)
            .map(|menu| menu.subtitle.clone())
            .unwrap_or_default()
    }

    /// Presses and releases `key`, a frame each.
    #[cfg(not(target_arch = "wasm32"))]
    fn tap(engine: &mut Loop<HeadlessShell>, key: crcbl::core::input::KeyCode) {
        let window = engine.window();
        engine
            .shell_mut()
            .key_press(window, key)
            .expect("the window is live");
        run_frames(engine, 1);
        engine
            .shell_mut()
            .key_release(window, key)
            .expect("the window is live");
        run_frames(engine, 1);
    }

    /// Types `address` into the lobby, which moves onto the connect row, and
    /// presses Enter.
    #[cfg(not(target_arch = "wasm32"))]
    fn connect_to(engine: &mut Loop<HeadlessShell>, address: std::net::SocketAddr) {
        let window = engine.window();
        engine
            .shell_mut()
            .commit_text(window, &address.to_string())
            .expect("the window is live");
        run_frames(engine, 2);
        tap(engine, MENU_ACTIVATE_KEY);
    }

    /// **The lobby's keys pick a host it heard, and the sandbox joins it.**
    /// A sandbox host on loopback announces; its row appears under offline
    /// and host; Down twice and Enter start a join to it, with the lobby up
    /// saying where; and once the host admits this player the lobby steps
    /// aside for the session, with no menu left on the frame.
    #[cfg(not(target_arch = "wasm32"))]
    #[test]
    fn the_lobbys_keys_pick_a_host_it_heard_and_the_sandbox_joins_it() {
        use crate::lan::{SANDBOX, Standing};
        use crate::lobby::tests::{address, bare_host, browsing};
        use crate::menu::FIRST_LISTED_ID;

        let mut host = bare_host(SANDBOX.compatibility);
        let mut engine = in_a_lobby(browsing(&host));
        serve_until(&mut engine, &mut host, "listed host", |engine| {
            engine
                .menus()
                .current()
                .is_some_and(|menu| menu.items().len() == 4)
        });
        tap(&mut engine, MENU_DOWN_KEY);
        tap(&mut engine, MENU_DOWN_KEY);
        assert_eq!(
            engine
                .menus()
                .current()
                .and_then(crcbl::ui::menu::Menu::selected_item)
                .map(|item| item.id),
            Some(FIRST_LISTED_ID)
        );
        tap(&mut engine, MENU_ACTIVATE_KEY);
        assert_eq!(engine.game().lan.joined(), Some(address(&host)));
        let waiting = format!("JOINING {}", address(&host));
        assert!(
            lobby_lines(&engine).iter().any(|line| line.text == waiting),
            "{:?}",
            lobby_lines(&engine)
        );

        serve_until(&mut engine, &mut host, "the session", |engine| {
            engine.game().lobby.is_none()
        });
        assert_eq!(engine.game().lan.standing(), Some(Standing::InSession));
        assert!(engine.game().parked.is_some(), "the lobby was dropped");
        run_frames(&mut engine, 1);
        assert!(engine.menus().current().is_none(), "a menu is still up");
        engine.finish(ExitReason::FrameBudget).expect("teardown");
    }

    /// **A join the host refuses leaves the player in the lobby, saying
    /// why** — here a host of another build, reached by a typed address —
    /// with no session left behind.
    #[cfg(not(target_arch = "wasm32"))]
    #[test]
    fn a_refused_join_leaves_the_lobby_saying_why() {
        use crate::lan::SANDBOX;
        use crate::lobby::tests::{address, bare_host};

        let mut host = bare_host(crcbl::net::ProtocolCompatibility {
            engine_build_id: SANDBOX.compatibility.engine_build_id + 1,
            ..SANDBOX.compatibility
        });
        let mut engine = in_a_lobby(lobby_alone());
        run_frames(&mut engine, 1);
        connect_to(&mut engine, address(&host));
        assert_eq!(engine.game().lan.joined(), Some(address(&host)));
        serve_until(&mut engine, &mut host, "a failed join", |engine| {
            lobby_lines(engine)
                .iter()
                .any(|line| line.text.starts_with("JOIN FAILED: "))
        });
        let failed = lobby_lines(&engine);
        let line = failed
            .iter()
            .find(|line| line.text.starts_with("JOIN FAILED: "))
            .expect("the line");
        assert_eq!(line.tone, crcbl::ui::menu::CaptionTone::Warning);
        assert!(line.text.contains("refused the join"), "{}", line.text);
        assert_eq!(engine.game().lan.standing(), None, "the join was kept");
        assert!(engine.game().lobby.is_some());
        engine.finish(ExitReason::FrameBudget).expect("teardown");
    }

    /// **A joined session that ends brings the player back to the lobby,
    /// saying how**: the host leaves, and the lobby is up again with the
    /// session gone.
    #[cfg(not(target_arch = "wasm32"))]
    #[test]
    fn a_joined_session_that_ends_returns_to_the_lobby_saying_how() {
        use crate::lan::SANDBOX;
        use crate::lobby::tests::{address, bare_host};

        let mut host = bare_host(SANDBOX.compatibility);
        let mut engine = in_a_lobby(lobby_alone());
        run_frames(&mut engine, 1);
        connect_to(&mut engine, address(&host));
        serve_until(&mut engine, &mut host, "the session", |engine| {
            engine.game().lobby.is_none()
        });
        host.host_mut()
            .shutdown(crcbl::net::SessionEndReason::HOST_LEFT);
        serve_until(&mut engine, &mut host, "the lobby again", |engine| {
            engine.game().lobby.is_some()
        });
        run_frames(&mut engine, 1);
        assert!(
            lobby_lines(&engine)
                .iter()
                .any(|line| line.text == "SESSION ENDED: the host left"),
            "{:?}",
            lobby_lines(&engine)
        );
        assert_eq!(engine.game().lan.standing(), None);
        assert!(engine.game().parked.is_none());
        engine.finish(ExitReason::FrameBudget).expect("teardown");
    }

    /// **Backspace in the loop takes a character off the typed address**:
    /// the lobby has the keys while it is up.
    #[cfg(not(target_arch = "wasm32"))]
    #[test]
    fn backspace_in_the_lobby_edits_the_typed_address() {
        use crate::menu::CONNECT_ID;

        let mut engine = in_a_lobby(lobby_alone());
        let window = engine.window();
        run_frames(&mut engine, 1);
        engine
            .shell_mut()
            .commit_text(window, "10.0.0.9:50")
            .expect("the window is live");
        run_frames(&mut engine, 1);
        tap(&mut engine, crcbl::core::input::KeyCode::Backspace);
        let hint = engine
            .menus()
            .current()
            .and_then(|menu| menu.items().iter().find(|item| item.id == CONNECT_ID))
            .map(|item| item.hint.clone());
        assert_eq!(hint.as_deref(), Some("10.0.0.9:5"));
        engine.finish(ExitReason::FrameBudget).expect("teardown");
    }

    /// **Offline leaves the lobby with no session**, and the keys are the
    /// game's again.
    #[cfg(not(target_arch = "wasm32"))]
    #[test]
    fn offline_leaves_the_lobby_with_no_session() {
        let mut engine = in_a_lobby(lobby_alone());
        run_frames(&mut engine, 1);
        assert_eq!(
            engine.menus().current().map(|menu| menu.title.as_str()),
            Some(crate::lobby::TITLE)
        );
        tap(&mut engine, MENU_ACTIVATE_KEY);
        assert!(engine.game().lobby.is_none(), "offline kept the lobby");
        assert_eq!(engine.game().lan.standing(), None);
        run_frames(&mut engine, 1);
        assert!(engine.menus().current().is_none());
        engine.finish(ExitReason::FrameBudget).expect("teardown");
    }
}
