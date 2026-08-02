//! The engine loop: shell events in, fixed ticks and presented frames out.
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
//! There is no `Shell::run(closure)` to hand this to and there never will be:
//! on wasm the outer loop is `requestAnimationFrame`, which calls the engine
//! and cannot be called by it. [`run`] owns a native `loop {}`; the body it
//! wraps is [`Loop::frame`], which is one turn and would be the `rAF` callback
//! unchanged.
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
use crcbl::core::input::KeyCode;
use crcbl::engine::{
    Clock, ConfigureError, ExitReason, Flow, MAX_CONSECUTIVE_RECONFIGURES, Pending, WINDOWED_IDLE,
    accept_close, wait_for_configure,
};
use crcbl::prelude::*;
use crcbl::shell::{LogicalSize, ShellBackend as Backend, WindowId, open, open_backend};
use crcbl::ui::DebugOverlay;
use crcbl::ui::draw_list::DrawList;

use crate::gpu::{FrameOutcome, Gpu, GpuError};

/// The key that shows and hides the debug overlay.
///
/// F3, the same key breakout and flappy use: "switching it on is one thing" is
/// only true if it is the *same* thing in every sample.
pub const DEBUG_OVERLAY_KEY: KeyCode = KeyCode::F3;

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
    /// GPU backend to force, or `None` to let
    /// [`crcbl::backend::open`] choose.
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
    /// Whether the debug overlay starts visible, or `None` for the default.
    ///
    /// Three-valued because the default is not a constant:
    /// `docs/plan/sample/00-samples-overview.md` rule 4 is "on by default in dev
    /// builds", so `None` means [`Options::debug_overlay_visible`]'s
    /// `cfg!(debug_assertions)` and either flag overrides it.
    pub debug_overlay: Option<bool>,
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
            debug_overlay: None,
        }
    }
}

impl Options {
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
}

/// Anything that can stop the sandbox before it starts.
#[derive(Debug)]
pub enum SandboxError {
    /// No shell backend could be opened. On macOS and Windows this is the
    /// expected outcome until P14 and the message says so.
    NoWindowSystem(ShellError),
    /// The window system refused something.
    Shell(ShellError),
    /// The window was never configured, so there was never a size to create a
    /// swapchain at.
    Configure(ConfigureError),
    /// The swapchain reconfigured over and over and never presented a frame.
    NeverPresented,
    /// The GPU seam failed.
    Gpu(GpuError),
}

impl std::fmt::Display for SandboxError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::NoWindowSystem(error) => write!(
                f,
                "no window system: {error}\n\
                 hint: either nothing is listening (no compositor, no \
                 DISPLAY), or this platform has no shell backend yet — Win32 \
                 and AppKit land at P14. `--headless` runs the same loop with \
                 no window and works everywhere."
            ),
            Self::Shell(error) => write!(f, "shell error: {error}"),
            Self::Configure(error) => write!(f, "{error}"),
            Self::NeverPresented => write!(
                f,
                "the swapchain reconfigured {MAX_CONSECUTIVE_RECONFIGURES} times in a row                  without presenting a frame"
            ),
            Self::Gpu(error) => write!(f, "gpu error: {error}"),
        }
    }
}

impl std::error::Error for SandboxError {}

impl From<ShellError> for SandboxError {
    fn from(error: ShellError) -> Self {
        Self::Shell(error)
    }
}

impl From<ConfigureError> for SandboxError {
    fn from(error: ConfigureError) -> Self {
        Self::Configure(error)
    }
}

impl From<GpuError> for SandboxError {
    fn from(error: GpuError) -> Self {
        Self::Gpu(error)
    }
}

/// Everything one turn of the loop needs.
///
/// Public so a test can drive [`Loop::frame`] directly rather than through the
/// `loop {}` in [`run`] — the same reason the browser will call it.
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
#[derive(Debug)]
pub struct Loop<S: Shell + ?Sized = dyn Shell> {
    shell: Box<S>,
    window: WindowId,
    gpu: Gpu,
    clock_source: Clock,
    frame_clock: FrameClock,
    /// Reused every frame rather than reallocated, like the samples' HUD lists.
    draw_list: DrawList,
    /// The modular debug panel. The sandbox has no HUD and no connection, so
    /// this is the whole of its UI: frame timing, plus GPU pass timings when
    /// the device has timestamp queries.
    debug: DebugOverlay,
    frames: u64,
    ticks: u64,
    events: u64,
    budget: Option<u64>,
    windowed: bool,
    reconfigures_in_a_row: u32,
}

/// Opens a window (or does not), runs the loop, and tears everything down.
///
/// # Errors
///
/// [`SandboxError`] if no shell backend opened, if the window was never
/// configured, or if the HAL seam failed.
/// Teardown runs on **every** path, including a failing frame. `run` used to
/// propagate `engine.frame()?`, which skipped `finish` — and therefore
/// `gpu.destroy()` and `shell.destroy_window()` — because neither `Loop` nor
/// `Gpu` has a `Drop` impl to fall back on. The `crcbl-vk` `Drop` impls do
/// reclaim the objects, but they log "N object(s) still alive at device
/// teardown" while doing it, which is the diagnostic for a real leak.
pub fn run(options: &Options) -> Result<Summary, SandboxError> {
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
            // The frame error is the one worth reporting; a teardown failure on
            // top of it is logged rather than allowed to replace it.
            if let Err(teardown) = engine.finish(ExitReason::Failed) {
                log::error!("teardown after a failed frame also failed: {teardown}");
            }
            Err(error)
        }
    }
}

impl Loop<dyn Shell> {
    /// Opens the shell the environment asks for, waits for the first configure,
    /// and joins the window to the HAL.
    ///
    /// # Errors
    ///
    /// [`SandboxError`] if any of those three steps fails.
    pub fn start(options: &Options) -> Result<Self, SandboxError> {
        let shell = if options.headless {
            // By name, never by fallback: `crcbl-shell`'s registry deliberately
            // refuses to auto-select headless, because a game that silently ran
            // without a window would look like a hang.
            open_backend(Backend::Headless).map_err(SandboxError::Shell)?
        } else {
            open().map_err(SandboxError::NoWindowSystem)?
        };
        Self::with_shell(shell, options)
    }
}

impl<S: Shell + ?Sized> Loop<S> {
    /// The body of [`start`](Loop::start), once a shell exists.
    ///
    /// # Errors
    ///
    /// [`SandboxError`] if the window is never configured or the HAL seam
    /// fails.
    pub fn with_shell(mut shell: Box<S>, options: &Options) -> Result<Self, SandboxError> {
        let clock_source = Clock::new(options.headless);
        log::info!(
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
            ..WindowDesc::default()
        })?;

        let mut events = 0;
        let extent = wait_for_configure(shell.as_mut(), window, &mut events)?;
        log::info!("shell: first configure at {}x{}", extent.0, extent.1);

        let gpu = Gpu::open(
            shell.as_ref(),
            window,
            extent,
            options.backend,
            options.camera.projection(),
        )?;

        Ok(Self {
            windowed: !options.headless,
            shell,
            window,
            gpu,
            clock_source,
            frame_clock: FrameClock::new(options.tick_hz),
            draw_list: DrawList::new(),
            debug: DebugOverlay::with_visible(options.debug_overlay_visible()),
            frames: 0,
            ticks: 0,
            events,
            budget: options.frame_budget(),
            reconfigures_in_a_row: 0,
        })
    }

    /// One turn of the outer loop: drain events, advance the clock, run the
    /// ticks that fell due, present a frame.
    ///
    /// This is the whole engine loop. On wasm it is the `requestAnimationFrame`
    /// callback with nothing changed.
    ///
    /// # Errors
    ///
    /// [`SandboxError`] if the shell or the HAL failed. A swapchain that has
    /// merely gone out of date is not a failure.
    pub fn frame(&mut self) -> Result<Flow, SandboxError> {
        if self.budget.is_some_and(|budget| self.frames >= budget) {
            return Ok(Flow::Stop(ExitReason::FrameBudget));
        }

        // Advisory, and skipped headless so a `--headless` run never blocks.
        if self.windowed {
            self.shell.wait_events(Some(WINDOWED_IDLE));
        }

        let mut pending = Pending::default();
        let mut toggle_debug = false;
        // The sink borrows `pending` mutably while `self.shell` is borrowed
        // mutably too, which is exactly why replies (`reply_close_request`,
        // `resize`) are recorded here and acted on below rather than inline.
        self.shell.pump(&mut |event| {
            pending.observe(&event);
            if let ShellEvent::Key {
                key_code: Some(DEBUG_OVERLAY_KEY),
                state: crcbl::shell::ButtonState::Pressed,
                ..
            } = event
            {
                toggle_debug = true;
            }
        });
        self.events += pending.count;
        if toggle_debug {
            self.debug.toggle();
        }

        if pending.destroyed {
            return Ok(Flow::Stop(ExitReason::WindowDestroyed));
        }
        if pending.close_requested {
            // A close request is a question. A game would ask the player about
            // unsaved progress here; the sandbox has none, so it says yes.
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
        while self.frame_clock.consume_tick() {
            self.ticks += 1;
            tick(self.frame_clock.tick_dt_secs());
            // The cube spins on the **fixed** timestep, not on the frame rate.
            // That is what makes `--headless --frames N` render a
            // bit-reproducible picture on every machine, and therefore what
            // makes a golden image of it evidence rather than a coincidence.
            #[allow(clippy::cast_possible_truncation)]
            self.gpu.advance(self.frame_clock.tick_dt_secs() as f32);
        }

        // `alpha` is read after the tick loop, never before: before, the
        // accumulator may still hold whole ticks.
        render(self.frame_clock.alpha());
        self.draw_debug_overlay();
        match self.gpu.frame()? {
            FrameOutcome::Presented => {
                self.frames += 1;
                self.reconfigures_in_a_row = 0;
            }
            // The budget counts *presented* frames, so a swapchain that is
            // permanently suboptimal (or permanently out of date) would
            // reconfigure forever and never reach it — and `--frames N` would
            // never terminate. Four seconds of 60 Hz reconfiguring is far past
            // "a resize storm" and squarely in "this surface will never
            // present".
            FrameOutcome::Reconfigured => {
                self.reconfigures_in_a_row += 1;
                if self.reconfigures_in_a_row >= MAX_CONSECUTIVE_RECONFIGURES {
                    return Err(SandboxError::NeverPresented);
                }
            }
        }
        Ok(Flow::Continue)
    }

    /// Gathers this frame's debug sections and draws the panel.
    ///
    /// **This is the whole of "switching it on".** Frame timing comes with the
    /// overlay; the only sandbox-specific line is the one that offers the GPU
    /// timings, and it is a `Some` check because a device without timestamp
    /// queries has no timers at all. There is no network module here either —
    /// the sandbox has no connection at all, which is a stronger version of
    /// breakout's and flappy's in-memory one.
    fn draw_debug_overlay(&mut self) {
        self.draw_list.clear();
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
        self.gpu.take_draw_list(&mut self.draw_list);
    }

    /// Tears the window and the GPU down and reports what happened.
    ///
    /// # Errors
    ///
    /// [`SandboxError`] if teardown failed.
    pub fn finish(mut self, exit: ExitReason) -> Result<Summary, SandboxError> {
        // `docs/plan/02-vulkan-backend.md` §2.4: "GPU timestamp per pass,
        // exposed as a frame-timing report". At `info` so a normal run shows
        // what the frame cost without `CRCBL_LOG=debug`; empty on a device with
        // no timestamp queries, which says so rather than printing zeros.
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
        };
        // Surface and swapchain first: the seam is explicit that the HAL
        // surface must die before the window whose handles it holds. Both are
        // attempted even when the first failed — a mapped window left behind
        // because the GPU teardown errored is strictly worse than the error.
        let gpu_result = self.gpu.destroy();
        // A window destroyed in answer to a close request is already gone.
        let shell_result = if exit.window_survives() {
            self.shell.destroy_window(self.window)
        } else {
            Ok(())
        };
        gpu_result?;
        shell_result?;
        Ok(summary)
    }

    /// The swapchain's format, which the HAL chose from the surface's caps.
    ///
    /// Test-only, and `#[cfg(test)]` rather than merely undocumented: a `pub`
    /// accessor nobody calls is dead code in a binary crate, and CI builds with
    /// `-D warnings`.
    #[cfg(test)]
    fn format(&self) -> crcbl::hal::Format {
        self.gpu.format()
    }

    /// The shell, at whatever type this loop was built with — so a test that
    /// built one from a [`HeadlessShell`](crcbl::shell::HeadlessShell) can
    /// script what a compositor would do.
    #[cfg(test)]
    fn shell_mut(&mut self) -> &mut S {
        self.shell.as_mut()
    }

    /// The window this loop is driving.
    #[cfg(test)]
    fn window(&self) -> WindowId {
        self.window
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

#[cfg(test)]
mod tests {
    use crcbl::shell::HeadlessShell;

    use super::*;

    /// A loop over a *concrete* `HeadlessShell`, so the test can play
    /// compositor. `run` uses `dyn Shell`; both go through the same
    /// [`Loop::with_shell`].
    fn scripted(options: &Options) -> Loop<HeadlessShell> {
        Loop::with_shell(Box::new(HeadlessShell::new()), options).expect("headless always starts")
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
            debug_overlay: None,
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
            !engine.gpu.last_dump().contains("ui-composite"),
            "and declares no UI pass either:\n{}",
            engine.gpu.last_dump(),
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
        assert_eq!(engine.debug.frame.len(), 2, "two real intervals so far");
        assert_eq!(
            engine.debug.frame.mean(),
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

        // **And it reaches the GPU.** `UiRenderer::add_pass` declares nothing
        // when the draw list is empty, so the pass's presence in the frame's
        // graph is the difference between "the overlay was drawn" and "the
        // overlay was composited onto the frame the player sees".
        assert!(
            engine.gpu.last_dump().contains("ui-composite"),
            "the UI pass must be in the frame:\n{}",
            engine.gpu.last_dump(),
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
        assert!(engine.format().is_srgb(), "{:?}", engine.format());
        assert!(engine.events >= 1, "the first configure is an event");
        engine.finish(ExitReason::FrameBudget).expect("teardown");
    }

    /// Resize arrives as an event and reaches the swapchain. Scripted through
    /// `HeadlessShell`, which is what it is for.
    #[test]
    fn a_resize_reconfigures_the_swapchain() {
        let mut engine = scripted(&headless(10));
        assert_eq!(engine.gpu.extent(), (1280, 720));
        engine.frame().expect("a frame");

        let window = engine.window();
        engine
            .shell_mut()
            .resize(window, PhysicalSize::new(1920, 1080))
            .expect("the window is live");
        engine.frame().expect("a frame after the resize");
        assert_eq!(engine.gpu.extent(), (1920, 1080));

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
}
