//! The breakout engine loop: window, orthographic camera, render graph.
//!
//! Shape matches `apps/sandbox/src/app.rs` — same clock, same event loop, same
//! fixed-timestep accumulator — but the camera is locked to orthographic.
//!
//! Slices 2–5 will add ECS, physics, input, UI, and audio on top of this.
//!
//! # The loop
//!
//! ```text
//! loop {
//!     shell.pump(&mut |event| …);
//!     clock.update(time.elapsed());
//!     while clock.consume_tick() { tick(dt); }
//!     render(clock.alpha());
//! }
//! ```

use std::time::Duration;

use crcbl::core::time::{ManualTime, MonotonicTime};
use crcbl::prelude::*;
use crcbl::shell::{LogicalSize, ShellBackend as Backend, WindowId, open, open_backend};

use crate::game::{self, Game};
use crate::gpu::{FrameOutcome, Gpu, GpuError};

// ---- constants --------------------------------------------------------------

/// How long to wait for the window to configure before giving up.
const CONFIGURE_TIMEOUT: Duration = Duration::from_secs(10);

/// Advisory idle for windowed frames.
const WINDOWED_IDLE: Duration = Duration::from_millis(4);

/// The simulated step a headless frame advances by (60 Hz wall clock).
const HEADLESS_FRAME_STEP: Duration = Duration::from_nanos(1_000_000_000 / 60);

pub use crate::args::Options;

// ---- exit / summary ---------------------------------------------------------

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ExitReason {
    FrameBudget,
    CloseRequested,
    WindowDestroyed,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Summary {
    pub backend: Backend,
    pub frames: u64,
    pub ticks: u64,
    pub events: u64,
    pub extent: (u32, u32),
    pub exit: ExitReason,
}

// ---- errors -----------------------------------------------------------------

#[derive(Debug)]
pub enum BreakoutError {
    NoWindowSystem(ShellError),
    Shell(ShellError),
    NeverConfigured,
    Gpu(GpuError),
    Game(game::GameError),
}

impl std::fmt::Display for BreakoutError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::NoWindowSystem(error) => write!(
                f,
                "no window system: {error}\n\
                 hint: `--headless` runs the same loop with no window \
                 and works everywhere."
            ),
            Self::Shell(error) => write!(f, "shell error: {error}"),
            Self::NeverConfigured => write!(
                f,
                "the window was never configured within {}s",
                CONFIGURE_TIMEOUT.as_secs()
            ),
            Self::Gpu(error) => write!(f, "gpu error: {error}"),
            Self::Game(error) => write!(f, "game error: {error}"),
        }
    }
}

impl std::error::Error for BreakoutError {}

impl From<ShellError> for BreakoutError {
    fn from(error: ShellError) -> Self {
        Self::Shell(error)
    }
}

impl From<GpuError> for BreakoutError {
    fn from(error: GpuError) -> Self {
        Self::Gpu(error)
    }
}

impl From<game::GameError> for BreakoutError {
    fn from(error: game::GameError) -> Self {
        Self::Game(error)
    }
}

// ---- clock ------------------------------------------------------------------

#[derive(Debug)]
enum Clock {
    Real(MonotonicTime),
    Manual { time: ManualTime, step: Duration },
}

impl Clock {
    fn new(headless: bool) -> Self {
        if headless {
            Self::Manual {
                time: ManualTime::new(),
                step: HEADLESS_FRAME_STEP,
            }
        } else {
            Self::Real(MonotonicTime::new())
        }
    }

    fn elapsed(&self) -> Duration {
        match self {
            Self::Real(time) => time.elapsed(),
            Self::Manual { time, .. } => time.elapsed(),
        }
    }

    fn advance(&mut self) -> Duration {
        match self {
            Self::Real(time) => time.elapsed(),
            Self::Manual { time, step } => {
                time.advance(*step);
                time.elapsed()
            }
        }
    }
}

// ---- the loop ---------------------------------------------------------------

#[derive(Debug)]
pub struct Loop<S: Shell + ?Sized = dyn Shell> {
    shell: Box<S>,
    window: WindowId,
    gpu: Gpu,
    game: Game,
    clock_source: Clock,
    frame_clock: FrameClock,
    frames: u64,
    ticks: u64,
    events: u64,
    budget: Option<u64>,
    windowed: bool,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Flow {
    Continue,
    Stop(ExitReason),
}

/// Runs the full loop.
pub fn run(options: &Options) -> Result<Summary, BreakoutError> {
    let mut engine = Loop::start(options)?;
    let exit = loop {
        match engine.frame()? {
            Flow::Continue => {}
            Flow::Stop(reason) => break reason,
        }
    };
    engine.finish(exit)
}

impl Loop<dyn Shell> {
    pub fn start(options: &Options) -> Result<Self, BreakoutError> {
        let shell = if options.headless {
            open_backend(Backend::Headless).map_err(BreakoutError::Shell)?
        } else {
            open().map_err(BreakoutError::NoWindowSystem)?
        };
        Self::with_shell(shell, options)
    }
}

impl<S: Shell + ?Sized> Loop<S> {
    pub fn with_shell(mut shell: Box<S>, options: &Options) -> Result<Self, BreakoutError> {
        let clock_source = Clock::new(options.headless);
        log::info!(
            "shell: {} backend, caps {:?}",
            shell.backend(),
            shell.caps()
        );
        shell.align_event_clock(clock_source.elapsed());

        let window = shell.create_window(&WindowDesc {
            title: "Breakout",
            app_id: "sh.kryptic.crcbl.breakout",
            size: LogicalSize::new(960.0, 720.0),
            ..WindowDesc::default()
        })?;

        let mut events = 0;
        let extent = wait_for_configure(shell.as_mut(), window, &mut events)?;
        log::info!("shell: first configure at {}x{}", extent.0, extent.1);

        // Locked to orthographic: breakout is a pure 2D game.
        let gpu = Gpu::open(shell.as_ref(), window, extent, options.backend)?;
        let game = Game::new(options.headless)?;

        Ok(Self {
            windowed: !options.headless,
            shell,
            window,
            gpu,
            game,
            clock_source,
            frame_clock: FrameClock::new(options.tick_hz),
            frames: 0,
            ticks: 0,
            events,
            budget: options.frame_budget(),
        })
    }

    pub fn frame(&mut self) -> Result<Flow, BreakoutError> {
        if self.budget.is_some_and(|budget| self.frames >= budget) {
            return Ok(Flow::Stop(ExitReason::FrameBudget));
        }

        if self.windowed {
            self.shell.wait_events(Some(WINDOWED_IDLE));
        }

        let mut pending = Pending::default();
        self.shell.pump(&mut |event| {
            pending.observe(&event);
            // Forward key events to the game's action map.
            if let ShellEvent::Key {
                key_code: Some(code),
                state,
                ..
            } = event
            {
                let pressed = matches!(state, crcbl::shell::ButtonState::Pressed);
                self.game.key_event(code, pressed);
            }
        });
        self.events += pending.count;

        if pending.destroyed {
            return Ok(Flow::Stop(ExitReason::WindowDestroyed));
        }
        if pending.close_requested {
            self.shell
                .reply_close_request(self.window, CloseReply::Close)?;
            return Ok(Flow::Stop(ExitReason::CloseRequested));
        }
        if let Some(size) = pending.resized {
            self.gpu.resize((size.width, size.height))?;
        }

        let now = self.clock_source.advance();
        self.frame_clock.update(now);
        while self.frame_clock.consume_tick() {
            self.ticks += 1;
            tick(self.frame_clock.tick_dt_secs());
        }

        // Advance the simulation and get the paddle position.
        let paddle_x = self.game.step(now);
        self.gpu.set_paddle_x(paddle_x);

        if self.gpu.frame()? == FrameOutcome::Presented {
            self.frames += 1;
        }
        Ok(Flow::Continue)
    }

    pub fn finish(mut self, exit: ExitReason) -> Result<Summary, BreakoutError> {
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
        self.gpu.destroy()?;
        if exit != ExitReason::CloseRequested && exit != ExitReason::WindowDestroyed {
            self.shell.destroy_window(self.window)?;
        }
        Ok(summary)
    }
}

// ---- event sink -------------------------------------------------------------

#[derive(Clone, Copy, Debug, Default)]
struct Pending {
    count: u64,
    resized: Option<crcbl::shell::PhysicalSize>,
    close_requested: bool,
    destroyed: bool,
}

impl Pending {
    fn observe(&mut self, event: &ShellEvent) {
        self.count += 1;
        match event {
            ShellEvent::Resized { size, .. } | ShellEvent::ScaleFactorChanged { size, .. } => {
                self.resized = Some(*size);
            }
            ShellEvent::CloseRequested { .. } => self.close_requested = true,
            ShellEvent::WindowDestroyed { .. } => self.destroyed = true,
            _ => {}
        }
        log::debug!("shell event: {event:?}");
    }
}

// ---- helpers ----------------------------------------------------------------

fn wait_for_configure<S: Shell + ?Sized>(
    shell: &mut S,
    window: WindowId,
    events: &mut u64,
) -> Result<(u32, u32), BreakoutError> {
    let started = MonotonicTime::new();
    loop {
        shell.pump(&mut |event| {
            *events += 1;
            log::debug!("shell event: {event:?}");
        });
        if let Some(size) = shell.window_state(window)?.size() {
            return Ok((size.width, size.height));
        }
        if started.elapsed() >= CONFIGURE_TIMEOUT {
            return Err(BreakoutError::NeverConfigured);
        }
        shell.wait_events(Some(Duration::from_millis(8)));
    }
}

/// One fixed simulation step — empty at Slice 1, gets physics/input in Slice 2+.
fn tick(dt: f64) {
    let _ = dt;
}

// ---- tests ------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use crcbl::shell::HeadlessShell;

    fn headless(frames: u64) -> Options {
        Options {
            headless: true,
            backend: Some(GpuBackend::Null),
            frames: Some(frames),
            tick_hz: 60,
        }
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
        // Drive a few frames to let the handshake settle.
        for _ in 0..5 {
            engine.frame().expect("a frame");
        }
        let summary = engine.finish(ExitReason::FrameBudget).expect("teardown");
        assert_eq!(summary.frames, 5);
        assert_eq!(summary.ticks, 4); // first update establishes baseline
        assert!(summary.events >= 1, "at least a configure event");
    }

    /// Helper: build a Loop<HeadlessShell> for scripting.
    fn scripted(options: &Options) -> Loop<HeadlessShell> {
        Loop::with_shell(Box::new(HeadlessShell::new()), options).expect("headless always starts")
    }
}
