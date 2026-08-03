//! Crucible driven as a **library**: this file writes its own loop.
//!
//! # Why this sample exists
//!
//! Every other sample plugs a game module into a loop the engine owns, which is
//! the arrangement Unity, Unreal and Bevy all settled on and the one `crcbl new`
//! scaffolds. That is the right default, and it has a cost: an engine whose only
//! consumers are framework-shaped stops being usable any other way, and nobody
//! notices until someone tries.
//!
//! `bare` is the someone who tries. It opens a shell, a window and a device,
//! runs a fixed-step simulation, records a frame and presents it, using nothing
//! but `crcbl`'s public API and its own `loop {}`. **It must never be converted
//! to the engine-owned loop** — a guard that adopts the thing it guards against
//! is not a guard.
//!
//! Concretely, it is the answer to three questions that are easy to answer wrong
//! from inside the engine:
//!
//! * Is every type a hand-written loop needs actually **public**, and reachable
//!   through the umbrella rather than a workspace path?
//! * Can a caller drive the frame without [`crcbl::engine::drive`] — the
//!   swappable-runner property the browser already depends on?
//! * Does the bring-up sequence *read* like something a person would write?
//!
//! # What it deliberately does not do
//!
//! No game, no menus, no audio, no assets. Those are the other samples' job and
//! adding them here would bury the seam under content. The simulation is a tick
//! counter and the render is one clear pass whose colour is derived from it —
//! enough that a frame differs from the one before it, which is what makes
//! "the simulation drives the picture" observable rather than asserted.

use core::time::Duration;

use crcbl::args::{Common, Consumed};
use crcbl::engine::{
    Clock, ExitReason, Flow, FrameBudget, FrameOutcome, GpuContext, GpuContextDesc, GpuError,
    LoopError, ModeRequest, Pending, WINDOWED_IDLE, accept_close, open_window, run_ticks,
    wait_for_configure,
};
use crcbl::hal::{CommandEncoderDesc, Format, ResourceState};
use crcbl::prelude::*;
use crcbl::render::{ImportedImage, RenderGraph, TransientPool};
use crcbl::shell::{
    DisplayMode, LogicalSize, Shell, ShellBackend, WindowDesc, WindowId, open, open_backend,
};

/// What a completed run did.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Summary {
    /// Which shell backend ran.
    pub backend: ShellBackend,
    /// Frames presented.
    pub frames: u64,
    /// Fixed simulation steps executed.
    pub ticks: u64,
    /// Shell events observed, of every kind.
    pub events: u64,
    /// The swapchain's size when the loop stopped.
    pub extent: (u32, u32),
    /// The mode the window system actually had the window in.
    ///
    /// **Not the one `--fullscreen` asked for.** A library consumer has to read
    /// this back the same way the engine's own loop does, which is the point of
    /// carrying it here: `crcbl::engine::ModeRequest::mode` is public, and a
    /// hand-written loop that reported its request instead would say
    /// "borderless" for every window system that refused.
    pub mode: DisplayMode,
    /// Why it stopped.
    pub exit: ExitReason,
}

/// This sample has no simulation of its own to fail, so the engine's error type
/// is used at its default type parameter.
pub type BareError = LoopError;

/// Everything one turn of the hand-written loop needs.
///
/// Public, and stepped through [`Self::frame`], because that is the property
/// under test: a caller can hold the parts and drive them.
#[derive(Debug)]
pub struct Bare<S: Shell + ?Sized = dyn Shell> {
    shell: Box<S>,
    window: WindowId,
    gpu: GpuContext,
    pool: TransientPool,
    clock_source: Clock,
    frame_clock: FrameClock,
    budget: FrameBudget,
    /// The whole simulation: how many fixed steps have run.
    ticks: u64,
    events: u64,
    windowed: bool,
}

impl Bare<dyn Shell> {
    /// Opens a shell, a window and a device.
    ///
    /// # Errors
    ///
    /// [`BareError`] if any of them refused.
    pub fn start(options: &Common) -> Result<Self, BareError> {
        let shell = if options.headless {
            open_backend(ShellBackend::Headless).map_err(LoopError::Shell)?
        } else {
            open().map_err(LoopError::NoWindowSystem)?
        };
        Self::with_shell(shell, options)
    }
}

impl<S: Shell + ?Sized> Bare<S> {
    /// Builds the loop on an already-open shell.
    ///
    /// The five calls a library consumer makes, in order, with nothing hidden:
    /// create the window, wait for it to have a size, open a device at that
    /// size, and pick a clock.
    ///
    /// # Errors
    ///
    /// [`BareError`] if the window never configured or the device would not
    /// open.
    pub fn with_shell(mut shell: Box<S>, options: &Common) -> Result<Self, BareError> {
        let clock_source = Clock::new(options.headless);
        let window = open_window(
            shell.as_mut(),
            &clock_source,
            &WindowDesc {
                title: "crcbl — bare",
                app_id: "sh.kryptic.crcbl.bare",
                size: LogicalSize::new(640.0, 480.0),
                // Asked for at creation rather than switched to afterwards, so
                // `--fullscreen` does not show a decorated window first.
                mode: options.display_mode(),
                ..WindowDesc::default()
            },
        )?;

        let mut events = 0;
        let extent = wait_for_configure(shell.as_mut(), window, &mut events)?;
        let gpu = GpuContext::open(
            shell.as_ref(),
            window,
            extent,
            &GpuContextDesc {
                label: "bare",
                backend: options.backend,
                ..GpuContextDesc::default()
            },
        )?;

        Ok(Self {
            windowed: !options.headless,
            shell,
            window,
            gpu,
            pool: TransientPool::new(),
            clock_source,
            frame_clock: FrameClock::new(options.tick_hz),
            budget: FrameBudget::new(options.frame_budget()),
            ticks: 0,
            events,
        })
    }

    /// The swapchain's current size.
    #[must_use]
    pub fn extent(&self) -> (u32, u32) {
        self.gpu.extent()
    }

    /// How many fixed steps have run.
    #[must_use]
    pub const fn ticks(&self) -> u64 {
        self.ticks
    }

    /// One turn: pump, tick, draw, present.
    ///
    /// # Errors
    ///
    /// [`BareError`] if the shell or the device refused something.
    pub fn frame(&mut self) -> Result<Flow, BareError> {
        if self.budget.is_spent() {
            return Ok(Flow::Stop(ExitReason::FrameBudget));
        }
        if self.windowed {
            self.shell.wait_events(Some(WINDOWED_IDLE));
        }

        // The engine folds the window's half of a batch; what comes back as
        // `Handled::Game` is this caller's to deal with, and here there is
        // nothing that wants it.
        let mut pending = Pending::default();
        self.shell.pump(&mut |event| {
            let _ = pending.observe(&event);
        });
        self.events += pending.count;

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

        self.frame_clock.update(self.clock_source.advance());
        let ticks = &mut self.ticks;
        run_ticks(&mut self.frame_clock, false, || *ticks += 1);

        let outcome = self.draw()?;
        self.budget.record(outcome)?;
        Ok(Flow::Continue)
    }

    /// Records and submits one frame, and reports what the swapchain did.
    ///
    /// The clear colour is derived from the tick count, so consecutive frames
    /// differ while the simulation runs and stop differing when it does not.
    fn draw(&mut self) -> Result<FrameOutcome, BareError> {
        let Some(acquired) = self.gpu.acquire()? else {
            // Reconfigured under us; the budget counts that and gives up if it
            // keeps happening.
            return Ok(FrameOutcome::Reconfigured);
        };

        let mut graph = RenderGraph::new(self.gpu.queue());
        // **`Undefined` in, `Present` out.** An acquired image's contents are
        // undefined — loading one is never correct — and the window system may
        // only be handed one in `Present`. Declaring both here is what lets the
        // graph emit the barriers; a hand-written pass would have to do it by
        // hand, which is the mistake this import exists to prevent.
        let target = graph.import_image(
            "swapchain",
            ImportedImage {
                image: acquired.image,
                view: acquired.view,
                format: self.gpu.format(),
                extent: acquired.extent,
                initial: ResourceState::Undefined,
                final_state: ResourceState::Present,
            },
        );
        graph
            .add_render_pass("clear")
            .clear_color(target, self.color())
            .execute(|_ctx| {});

        let compiled = graph.compile(&self.pool).map_err(GpuError::Graph)?;
        let mut encoder = self
            .gpu
            .device()
            .create_command_encoder(&CommandEncoderDesc {
                label: Some("bare"),
                queue: self.gpu.queue(),
            });
        compiled
            .execute(self.gpu.device(), &mut self.pool, encoder.as_mut(), None)
            .map_err(GpuError::Graph)?;
        let command_buffer = encoder.finish().map_err(GpuError::Hal)?;
        Ok(self.gpu.submit_and_present(&acquired, command_buffer)?)
    }

    /// A colour that walks with the simulation, so one frame differs from the
    /// last.
    fn color(&self) -> [f32; 4] {
        #[allow(clippy::cast_precision_loss)]
        let t = (self.ticks % 240) as f32 / 240.0;
        [t * 0.6, 0.15, 1.0 - t * 0.6, 1.0]
    }

    /// Releases the device, then the window.
    ///
    /// # Errors
    ///
    /// [`BareError`] if either refused. Both are attempted regardless, because
    /// a leaked device is worse than a lost error.
    pub fn finish(mut self, exit: ExitReason) -> Result<Summary, BareError> {
        let summary = Summary {
            backend: self.shell.backend(),
            frames: self.budget.presented(),
            ticks: self.ticks,
            events: self.events,
            extent: self.gpu.extent(),
            mode: ModeRequest::mode(&*self.shell, self.window),
            exit,
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

/// Runs until something stops it.
///
/// **Deliberately not [`crcbl::engine::drive`].** That helper does exactly this,
/// and using it here would leave the "a caller can write their own loop"
/// property untested — which is the one thing this sample is for.
///
/// # Errors
///
/// [`BareError`] from the frame that failed, or from teardown.
pub fn run(options: &Common) -> Result<Summary, BareError> {
    let mut engine = Bare::start(options)?;
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

/// The `--help` text.
pub const USAGE: &str = "\
bare — Crucible driven as a library, with a hand-written loop

USAGE:
    bare [OPTIONS]

OPTIONS:
    --headless           Run without a window (for CI / determinism tests)
    --frames <N>         Stop after N presented frames
    --tick-hz <N>        Simulation rate in Hz (default 60). Sets the server's
                         clock, the ECS timestep and every integrator.
    --backend <B>        GPU backend: vk, vulkan, null, none or wgpu
    --fullscreen         Open borderless instead of windowed. F11 still toggles.
                         A window system may refuse; the summary reports what
                         it actually did, not what was asked for.
    --debug-overlay      Start with the debug panel visible (F3 toggles it)
    --no-debug-overlay   Start with it hidden. The default is 'visible in a
                         debug build, hidden in a release build'
    -h, --help           Print this help";

/// What the command line asked for.
pub type Invocation = crcbl::args::Invocation<Common>;

/// Parses a flat argument iterator.
///
/// This sample claims no flags of its own, so anything the shared set hands
/// back is an unknown argument.
pub fn parse(args: impl Iterator<Item = String>) -> Invocation {
    let mut common = Common::new(DEFAULT_TICK_HZ);
    let mut args = args.peekable();

    while let Some(arg) = args.next() {
        match common.consume(&arg, &mut args) {
            Consumed::Yes => continue,
            Consumed::Help => return Invocation::Help,
            Consumed::Bad(message) => return Invocation::BadUsage(message),
            Consumed::No => return Invocation::BadUsage(format!("unknown argument: {arg}")),
        }
    }

    Invocation::Run(common)
}

/// The simulation rate, and the rate the clear colour walks at.
pub const DEFAULT_TICK_HZ: u32 = 60;

/// The mode this sample runs in. It never asks for anything else, and says so
/// rather than leaving a reader to check.
pub const DISPLAY_MODE: DisplayMode = DisplayMode::Windowed;

/// The swapchain format, exposed so a test can assert the device gave a real
/// one rather than a default.
#[must_use]
pub fn format_of(engine: &Bare<impl Shell + ?Sized>) -> Format {
    engine.gpu.format()
}

/// One tick's duration at the default rate, for a caller pacing its own clock.
#[must_use]
pub const fn tick_period() -> Duration {
    Duration::from_nanos(1_000_000_000 / DEFAULT_TICK_HZ as u64)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn headless(frames: u64) -> Common {
        let mut common = Common::new(DEFAULT_TICK_HZ);
        common.headless = true;
        common.frames = Some(frames);
        common.backend = Some(crcbl::backend::GpuBackend::Null);
        common
    }

    /// **The whole hand-written loop runs, presents its budget and tears down.**
    ///
    /// The end-to-end claim this sample exists to make. If the public API ever
    /// stops being enough to write a loop with, this stops compiling — which is
    /// the failure mode worth having, because it happens at the seam rather than
    /// in a game.
    #[test]
    fn a_headless_run_presents_its_budget_and_stops() {
        let summary = run(&headless(8)).expect("the null backend runs everywhere");
        assert_eq!(summary.frames, 8);
        assert_eq!(summary.exit, ExitReason::FrameBudget);
        assert_eq!(summary.backend, ShellBackend::Headless);
        assert!(summary.extent.0 > 0 && summary.extent.1 > 0);
    }

    /// **The simulation drives the picture.**
    ///
    /// A clear colour derived from the tick count is the cheapest way to make
    /// "the frame changed because the simulation advanced" observable. A loop
    /// that presented without ticking would hold one colour forever, and every
    /// other assertion here would still pass.
    #[test]
    fn the_clear_colour_walks_with_the_simulation() {
        let mut engine = Bare::start(&headless(4)).expect("headless starts");
        let first = engine.color();
        while engine.ticks() == 0 {
            engine.frame().expect("a frame");
        }
        assert_ne!(
            engine.color(),
            first,
            "the colour did not move once the simulation did",
        );
        engine.finish(ExitReason::FrameBudget).expect("teardown");
    }

    /// **A caller can step the loop by hand**, which is the property that keeps
    /// the runner swappable — the browser depends on it and cannot use
    /// `crcbl::engine::drive`.
    #[test]
    fn the_loop_can_be_stepped_one_frame_at_a_time() {
        let mut engine = Bare::start(&headless(3)).expect("headless starts");
        for _ in 0..3 {
            assert_eq!(engine.frame().expect("a frame"), Flow::Continue);
        }
        assert_eq!(
            engine.frame().expect("a frame"),
            Flow::Stop(ExitReason::FrameBudget),
            "the budget must stop a hand-stepped loop too",
        );
        let summary = engine.finish(ExitReason::FrameBudget).expect("teardown");
        assert_eq!(summary.frames, 3);
    }

    /// Unknown arguments are refused rather than ignored; this sample claims no
    /// flags of its own.
    #[test]
    fn nonsense_is_refused() {
        let bad = parse(["--nonsense".to_string()].into_iter());
        assert!(matches!(bad, Invocation::BadUsage(message) if message.contains("nonsense")));
        assert!(matches!(
            parse(["--help".to_string()].into_iter()),
            Invocation::Help
        ));
    }

    /// The shared half of the usage text is the engine's, byte for byte.
    #[test]
    fn the_shared_half_of_the_usage_text_is_the_engines_verbatim() {
        assert!(USAGE.contains(crcbl::args::COMMON_OPTIONS_HELP));
        assert!(USAGE.contains(crcbl::args::COMMON_TAIL_HELP));
    }
}
