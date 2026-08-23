//! The flags every sample has, and the pieces for the ones it does not.
//!
//! ```text
//!   argv ──▶ Common::consume ──┬─▶ Consumed::Yes    the engine took it
//!                              ├─▶ Consumed::Help   -h / --help
//!                              ├─▶ Consumed::Bad    it took it and it was wrong
//!                              └─▶ Consumed::No     the game's turn
//! ```
//!
//! # Offered, not imposed
//!
//! A game owns its parse loop and its `Options` struct. This offers each
//! argument to the shared set first and hands back anything it does not
//! recognise, so a game adds `--seed` or `--max-enemies` by matching on it
//! after — rather than by registering it with a framework that then owns the
//! error messages, the help text and the ordering.
//!
//! That matters more than it sounds: `apps/sandbox` takes `--camera` and
//! `--title` and no `--seed`, keeps its `Options` in `app.rs`, and is
//! deliberately **not** a consumer of this module. A design that could not
//! accommodate it would be a design that had guessed.
//!
//! # Why the engine owns any of it
//!
//! Because the four games' parsers were the same file. `apps/flappy` and
//! `apps/asteroids` differed in **eight lines**, of which six were the usage
//! prose and one was a test name. The flags are not a game's business either:
//! `--backend` names the GPU registry, `--headless` means "no shell",
//! `--tick-hz` sets the session's rate, and `--frames` bounds a CI run. Every
//! one of those is the engine's vocabulary, so every game spelling it out was
//! four chances to spell it differently — and they did. Three of the parsers
//! dropped breakout's assertion that the default backend stays `None`, and
//! stranded CI on a machine with no driver.

use std::fmt;
use std::process::ExitCode;

use crate::backend::GpuBackend;
use crate::engine::{FrameLimit, GpuOptions, LoopConfig, Pacing};

/// The shared `OPTIONS:` block, so the samples' help texts cannot drift.
///
/// A game's `USAGE` is its own prose — its name, its tagline, its own flags —
/// with this pasted in for the common set. Kept as one string rather than
/// assembled from the parser because help text is read, not parsed, and the
/// alignment is part of it.
pub const COMMON_OPTIONS_HELP: &str = "\
    --headless           Run without a window (for CI / determinism tests)
    --frames <N>         Stop after N presented frames
    --tick-hz <N>        Simulation rate in Hz (default 60). Sets the server's
                         clock, the ECS timestep and every integrator.
    --backend <B>        GPU backend: vk, vulkan, mtl, metal, dx12, d3d12,
                         null, none, wgpu or webgpu
    --fullscreen         Open borderless instead of windowed. F11 still toggles.
                         A window system may refuse; the summary reports what
                         it actually did, not what was asked for.
    --pacing <P>         How frames are paced against the display: auto, vsync,
                         adaptive or off. Default: auto, which is adaptive sync
                         where the display is running it and vsync where it is
                         not. 'adaptive' is the one to ask for on a VRR panel.
    --fps <N>            Frame limit, in frames a second. Default: 1000, high
                         enough to be a runaway guard rather than a cap. 0 is
                         unlimited. Under vsync the display paces the loop and
                         this rarely fires.
    --size <WxH>         Window size in pixels, WxH (default 960x720). The
                         headless offscreen ring renders at exactly this extent,
                         which is what makes a scale measurement reproducible.";

/// The `--screenshot` line, for the samples that have wired it up.
///
/// **Separate from [`COMMON_OPTIONS_HELP`] because the flag is separate**: it
/// only exists for a sample whose [`Common`] said `with_screenshot` — named
/// rather than linked because it is `cfg(not(target_arch = "wasm32"))` and a
/// browser build documents no such item — and a help text listing a flag
/// that same binary answers with exit 2 would be worse than not listing it.
/// Spliced in where a game's own flags go — between the two shared blocks — so
/// the ordering is the one every other flag already has.
pub const SCREENSHOT_HELP: &str = "\
    --screenshot <PATH>  Write the run's last presented frame to PATH as a PNG.
                         Turns --headless on: the frame is read back off the
                         offscreen ring, which is the only surface every backend
                         can copy a presented image out of.";

/// The tail of the shared block: the debug overlay pair and `--help`.
///
/// Separate from [`COMMON_OPTIONS_HELP`] so a game can list its own flags
/// *between* the two, which is where all four already had them.
pub const COMMON_TAIL_HELP: &str = "\
    --debug-overlay      Start with the debug panel visible (F3 toggles it)
    --no-debug-overlay   Start with it hidden. The default is 'visible in a
                         debug build, hidden in a release build'
    -h, --help           Print this help";

/// How many frames a headless run presents when `--frames` did not say.
///
/// Enough to boot, simulate and present repeatedly; short enough that a CI job
/// waiting on four of them is not waiting long.
pub const HEADLESS_FRAME_BUDGET: u64 = 120;

/// The highest tick rate a `FrameClock` can express: `1e9 / hz` is the tick
/// period in nanoseconds, and `hz > 1e9` truncates it to zero, which the
/// clock asserts against after the GPU is already open. `apps/sim` guards its
/// own flag with the same bound.
pub const MAX_TICK_RATE: u32 = 1_000_000_000;

/// What the command line asked for, wrapped around a game's own options.
///
/// Generic because each game's `Options` is its own — `Run` carries the game's
/// type, not this module's, so nothing here has to know what a seed is.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Invocation<T> {
    /// Run with these options.
    Run(T),
    /// Print the usage text.
    Help,
    /// The arguments do not make sense, for this reason.
    BadUsage(String),
}

/// Runs the native front end every sample shares: logging init, the parsed
/// [`Invocation`], the run, the summary line, and the exit code.
///
/// The exit-code contract is the one each sample's docs state and `crcbl`
/// itself uses: **0** ran, **1** it failed, **2** the arguments were wrong.
/// `--help` prints the usage and runs cleanly; a bad invocation prints the
/// reason and the usage, on stderr, so a wrapper script can read one without
/// the other.
///
/// Everything that is a game's own is passed in — its name (the log and error
/// prefix), its usage text, its already-parsed [`Invocation`], its `run`, and
/// the summary line — so the flow below is written once instead of once per
/// sample. `bare` and the `crcbl new` template are samples too; only
/// `apps/sandbox` stays out, because its parser and its `Invocation` are
/// deliberately its own.
pub fn run_front_end<O, S, E>(
    name: &str,
    usage: &str,
    invocation: Invocation<O>,
    run: impl FnOnce(&O) -> Result<S, E>,
    print_summary: impl FnOnce(&S) -> String,
) -> ExitCode
where
    E: fmt::Display,
{
    // `CRCBL_LOG=debug` turns on the per-event lines; the default is warnings.
    crate::core::log::init_logging();
    // `CRCBL_TRACE=1` turns the CPU spans on and with them the debug panel's
    // budget row. Beside the logger and after it, because turning the trace on
    // logs a line saying so and a line logged before the sink exists goes
    // nowhere. Off by default: see `crcbl_core::trace`.
    crate::core::trace::init_from_env();

    match invocation {
        Invocation::Run(options) => match run(&options) {
            Ok(summary) => {
                println!("{}", print_summary(&summary));
                ExitCode::SUCCESS
            }
            Err(error) => {
                eprintln!("{name}: {error}");
                ExitCode::FAILURE
            }
        },
        Invocation::Help => {
            println!("{usage}");
            ExitCode::SUCCESS
        }
        Invocation::BadUsage(message) => {
            eprintln!("{name}: {message}");
            eprintln!("{usage}");
            ExitCode::from(2)
        }
    }
}

/// What [`Common::consume`] did with an argument.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Consumed {
    /// Recognised and applied. Move to the next argument.
    Yes,
    /// It was `-h` or `--help`.
    Help,
    /// Recognised, and wrong — a missing or unparseable value.
    Bad(String),
    /// Not one of the shared flags. The game's turn.
    No,
}

/// The options every sample has.
///
/// Held as a field on the game's own `Options` rather than flattened into it,
/// so adding a shared flag reaches every sample without touching any of their
/// structs.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Common {
    /// Run without a window.
    pub headless: bool,
    /// Stop after this many presented frames.
    pub frames: Option<u64>,
    /// Simulation rate in Hz.
    pub tick_hz: u32,
    /// The GPU backend, or `None` to let the registry choose.
    ///
    /// **`None` is not "no backend"** — it is "you pick", and it must stay the
    /// default. A default of `Some(Vulkan)` strands every machine without a
    /// driver, which is what it did to CI once already.
    pub backend: Option<GpuBackend>,
    /// Whether to open the window borderless rather than windowed.
    ///
    /// A *request*. `F11` toggles from either starting point, and a window
    /// system is free to refuse both — see [`Common::display_mode`].
    pub fullscreen: bool,
    /// Whether the debug overlay starts visible, or `None` for the default.
    ///
    /// Three-valued because the default is not a constant:
    /// `docs/plan/sample/00-samples-overview.md` rule 4 is "on by default in
    /// dev builds", so `None` means [`Common::debug_overlay_visible`]'s
    /// `cfg!(debug_assertions)` and either flag overrides it.
    pub debug_overlay: Option<bool>,
    /// How presented frames are paced against the display.
    ///
    /// Two-valued rather than three — no `Option` — because unlike
    /// [`debug_overlay`](Self::debug_overlay) the default *is* a constant:
    /// [`Pacing::Auto`]. There is nothing for `None` to mean that
    /// [`Pacing::Auto`] does not already say, and a second spelling of "the
    /// engine decides" is a second thing to keep in step.
    ///
    /// A **request**: `Auto` settles against the display after the first
    /// present, and a surface that does not offer the mode a pacing prefers
    /// falls back — [`GpuContext::effective_pacing`](crate::engine::GpuContext::effective_pacing)
    /// is what a run actually got.
    pub pacing: Pacing,
    /// The most frames a second the loop will run.
    ///
    /// Resolved rather than optional for the same reason as
    /// [`pacing`](Self::pacing): the default is the constant
    /// [`FrameLimit::DEFAULT_FPS`]. `--fps 0` is
    /// [`FrameLimit::unlimited`], which is the spelling
    /// [`FrameLimit::fps`] already documents.
    ///
    /// A game whose own default is not a thousand writes it before parsing —
    /// `Common { limit: FrameLimit::fps(144), ..Common::new(60) }` — the way it
    /// would for any other field it has an opinion about. It is not a
    /// [`Common::new`] parameter because, unlike the tick rate, most games have
    /// no opinion at all.
    pub limit: FrameLimit,
    /// Whether this sample arms `--screenshot` on its GPU context.
    ///
    /// **`false` makes the flag absent, not ignored.** [`consume`](Self::consume)
    /// hands `--screenshot` back as [`Consumed::No`] here, which every sample's
    /// parser turns into "unknown argument" and exit 2 — so a sample that has
    /// not wired the arming up refuses the flag rather than accepting it and
    /// writing nothing. That is the whole reason this is not simply always on:
    /// the shared half of the flag is the parse, and the half that cannot be
    /// shared is a bundle handing its [`GpuContext`](crate::engine::GpuContext)
    /// over.
    ///
    /// Set with [`with_screenshot`](Self::with_screenshot). `apps/breakout` is
    /// the sample that does.
    #[cfg(not(target_arch = "wasm32"))]
    pub can_screenshot: bool,
    /// Where to write the run's last presented frame as a PNG, if anywhere.
    ///
    /// **Setting this forces [`headless`](Self::headless) on**, and that is the
    /// enforcement rather than a note: a presented *window* swapchain image is
    /// not something every backend and surface will copy back, so a windowed
    /// `--screenshot` would be a flag that produced nothing. Headless renders
    /// into an offscreen ring, where a present returns the image and the copy
    /// is the same on every backend — see
    /// [`GpuContext::set_screenshot`](crate::engine::GpuContext::set_screenshot).
    ///
    /// Native only, like the request it builds: there is no argv in a browser
    /// and no file to write.
    #[cfg(not(target_arch = "wasm32"))]
    pub screenshot: Option<std::path::PathBuf>,
    /// The window size the sample opens at, or `None` for the sample's default.
    ///
    /// Pixels, as a `WxH` value. The window *request* is
    /// [`crcbl_shell::LogicalSize`] at scale 1, which is what makes `--size
    /// 1920x1080` produce a 1920 × 1080 headless offscreen ring on every
    /// machine; on a HiDPI display the compositor scales the request.
    pub size: Option<crcbl_shell::PhysicalSize>,
}

impl Common {
    /// The defaults, at `tick_hz`.
    ///
    /// The rate is an argument because it is the one shared field whose default
    /// is the *game's* — a game that integrates at 120 Hz says so here rather
    /// than overwriting the field afterwards and hoping nothing read it first.
    #[must_use]
    pub const fn new(tick_hz: u32) -> Self {
        Self {
            headless: false,
            frames: None,
            tick_hz,
            backend: None,
            fullscreen: false,
            debug_overlay: None,
            pacing: Pacing::Auto,
            limit: FrameLimit::fps(FrameLimit::DEFAULT_FPS),
            #[cfg(not(target_arch = "wasm32"))]
            can_screenshot: false,
            #[cfg(not(target_arch = "wasm32"))]
            screenshot: None,
            size: None,
        }
    }

    /// Declares that this sample hands its context to
    /// [`GpuContext::set_screenshot`](crate::engine::GpuContext::set_screenshot),
    /// and so may be given `--screenshot`.
    ///
    /// Written into the game's `Options::default` —
    /// `Common::new(TICK_HZ).with_screenshot()` — beside the tick rate, which
    /// is the other fact about a sample that this struct cannot guess.
    #[cfg(not(target_arch = "wasm32"))]
    #[must_use]
    pub const fn with_screenshot(mut self) -> Self {
        self.can_screenshot = true;
        self
    }

    /// What `--screenshot` asked the GPU for, or `None` if it was not passed.
    ///
    /// The frame is [`frame_budget`](Self::frame_budget)'s — the **last** one
    /// the run will present. Every sample's opening frames are its start-up:
    /// the first tick has not run, the atlas may not have uploaded, and a
    /// picture of that is a picture of nothing anyone plays. The budget is
    /// always `Some` here because `--screenshot` forced `headless` on, and
    /// [`frame_budget`](Self::frame_budget) always answers for a headless run.
    #[cfg(not(target_arch = "wasm32"))]
    #[must_use]
    pub fn screenshot_request(&self) -> Option<crate::engine::ScreenshotRequest> {
        let path = self.screenshot.clone()?;
        let frame = self.frame_budget()?;
        Some(crate::engine::ScreenshotRequest { path, frame })
    }

    /// What the command line contributes to opening a GPU.
    ///
    /// The value a sample's `Gpu::open` takes, so that a run-level knob the
    /// device or the swapchain cares about is added here and in
    /// [`GpuContextDesc`](crate::engine::GpuContextDesc) rather than as another
    /// parameter on every bring-up path.
    #[must_use]
    pub const fn gpu(&self) -> GpuOptions {
        GpuOptions {
            backend: self.backend,
            pacing: self.pacing,
        }
    }

    /// Everything [`crcbl::engine::Loop`](crate::engine::Loop) takes from the
    /// command line.
    ///
    /// Every sample parser built this struct literal from the same expressions,
    /// which is one place per sample to forget a field: `--fps` would have been
    /// wired into some of them and silently ignored by the rest, and nothing
    /// would have failed to compile.
    #[must_use]
    pub fn loop_config(&self) -> LoopConfig {
        LoopConfig {
            tick_hz: self.tick_hz,
            frames: self.frame_budget(),
            debug_overlay: self.debug_overlay_visible(),
            // A headless run has no compositor to idle against and would
            // otherwise sleep its way through its frame budget.
            windowed: !self.headless,
            limit: self.limit,
        }
    }

    /// The mode to create the window in.
    ///
    /// Belongs to [`crcbl_shell::WindowDesc::mode`] rather than to a `set_mode`
    /// after start-up: asking at creation is what stops a fullscreen game from
    /// showing a decorated window for the frames it takes the request to land.
    ///
    /// **A request, and the summary reports the answer.** No window system owes
    /// it: an X11 session with no window manager sends
    /// `_NET_WM_STATE_FULLSCREEN` into the void, and `RunSummary::mode` is what
    /// says so afterwards.
    #[must_use]
    pub const fn display_mode(&self) -> crcbl_shell::DisplayMode {
        if self.fullscreen {
            crcbl_shell::DisplayMode::Borderless { monitor: None }
        } else {
            crcbl_shell::DisplayMode::Windowed
        }
    }

    /// How many frames to present before stopping, if a limit applies.
    ///
    /// A headless run always gets one: it has no window to close, so without a
    /// budget it never terminates and a CI job hangs instead of failing.
    #[must_use]
    pub fn frame_budget(&self) -> Option<u64> {
        match (self.frames, self.headless) {
            (Some(frames), _) => Some(frames),
            (None, true) => Some(HEADLESS_FRAME_BUDGET),
            (None, false) => None,
        }
    }

    /// Whether the debug overlay starts visible.
    #[must_use]
    pub fn debug_overlay_visible(&self) -> bool {
        self.debug_overlay.unwrap_or(cfg!(debug_assertions))
    }

    /// Offers `arg` to the shared flag set, taking its value from `rest`.
    ///
    /// Returns [`Consumed::No`] for anything it does not recognise, including
    /// an unknown `--flag` — reporting that is the caller's job, because only
    /// the caller knows whether the game claims it.
    pub fn consume(&mut self, arg: &str, rest: &mut impl Iterator<Item = String>) -> Consumed {
        match arg {
            "--headless" => self.headless = true,
            "--fullscreen" => self.fullscreen = true,
            "--debug-overlay" => self.debug_overlay = Some(true),
            "--no-debug-overlay" => self.debug_overlay = Some(false),
            "-h" | "--help" => return Consumed::Help,
            "--frames" => match positive(arg, rest, "frame count") {
                Ok(frames) => self.frames = Some(frames),
                Err(message) => return Consumed::Bad(message),
            },
            "--tick-hz" => match positive(arg, rest, "tick rate") {
                Ok(hz) => match u32::try_from(hz) {
                    Ok(hz) if hz <= MAX_TICK_RATE => self.tick_hz = hz,
                    Ok(hz) => {
                        return Consumed::Bad(format!(
                            "tick rate {hz} is too large (max {MAX_TICK_RATE})"
                        ));
                    }
                    Err(_) => return Consumed::Bad(format!("not a positive tick rate: {hz}")),
                },
                Err(message) => return Consumed::Bad(message),
            },
            // Zero is a legal value — it is how `FrameLimit` spells "no limit" —
            // so `number` rather than `positive`.
            "--fps" => match number(arg, rest, "frame rate") {
                Ok(fps) => match u32::try_from(fps) {
                    Ok(fps) => self.limit = FrameLimit::fps(fps),
                    Err(_) => {
                        return Consumed::Bad(format!(
                            "frame rate {fps} is too large (max {})",
                            u32::MAX
                        ));
                    }
                },
                Err(message) => return Consumed::Bad(message),
            },
            "--pacing" => {
                let Some(name) = rest.next() else {
                    return Consumed::Bad("--pacing needs a value".into());
                };
                match Pacing::from_name(&name) {
                    Some(pacing) => self.pacing = pacing,
                    // Every name, not just a complaint: the word a player knows
                    // for the adaptive case is "VRR", which is not one of these,
                    // and a message that did not list them would leave them
                    // guessing at a four-word vocabulary.
                    None => {
                        return Consumed::Bad(format!(
                            "unknown pacing '{name}' — try `auto`, `vsync`, `adaptive` or `off`"
                        ));
                    }
                }
            }
            "--backend" => {
                let Some(name) = rest.next() else {
                    return Consumed::Bad("--backend needs a value".into());
                };
                match GpuBackend::from_name(&name) {
                    Some(backend) => self.backend = Some(backend),
                    None => {
                        return Consumed::Bad(format!(
                            "unknown backend '{name}' — try {}",
                            GpuBackend::name_list()
                        ));
                    }
                }
            }
            "--size" => match size(arg, rest) {
                Ok(size) => self.size = Some(size),
                Err(message) => return Consumed::Bad(message),
            },
            // `headless` is set here rather than checked after the parse,
            // because a check would have to run somewhere every game
            // remembered to put it and this cannot be forgotten. The two orders
            // `--screenshot x --headless` and `--headless --screenshot x` reach
            // the same `Common`, and there is no third state where a file was
            // asked for and no offscreen ring exists to read it off.
            #[cfg(not(target_arch = "wasm32"))]
            "--screenshot" if self.can_screenshot => {
                let Some(path) = rest.next() else {
                    return Consumed::Bad("--screenshot needs a path".into());
                };
                self.screenshot = Some(path.into());
                self.headless = true;
            }
            _ => return Consumed::No,
        }
        Consumed::Yes
    }
}

/// The value after `flag`, parsed as a number and required to be non-zero.
///
/// `noun` names the thing in the rejection, so `--frames 0` reads "not a
/// positive frame count: 0" rather than a message about a type.
///
/// # Errors
///
/// The message to hand back as `BadUsage`, when the value is missing, does not
/// parse, or is zero.
pub fn positive(
    flag: &str,
    rest: &mut impl Iterator<Item = String>,
    noun: &str,
) -> Result<u64, String> {
    let Some(value) = rest.next() else {
        return Err(format!("{flag} needs a number"));
    };
    match value.parse::<u64>() {
        // A *negative* value fails the parse rather than reaching this arm,
        // which is the same rejection by a different route.
        Ok(parsed) if parsed > 0 => Ok(parsed),
        _ => Err(format!("not a positive {noun}: {value}")),
    }
}

/// The value after `flag`, parsed as a number. Zero is allowed.
///
/// For a seed, where every value is legal and there is nothing to be positive
/// about.
///
/// # Errors
///
/// The message to hand back as `BadUsage`, when the value is missing or does
/// not parse.
pub fn number(
    flag: &str,
    rest: &mut impl Iterator<Item = String>,
    noun: &str,
) -> Result<u64, String> {
    let Some(value) = rest.next() else {
        return Err(format!("{flag} needs a number"));
    };
    value
        .parse::<u64>()
        .map_err(|_| format!("not a {noun}: {value}"))
}

/// The value after `--size`, parsed as `WxH`.
///
/// `1` is a size a `--frames`-style positive-number parse would also accept,
/// and it is exactly the value this must *not* accept — "1" is not "1x1", and
/// accepting it would turn a typo into a tiny window.
///
/// # Errors
///
/// The message to hand back as `BadUsage`, when the value is missing, is not
/// `WxH`, or a dimension is zero.
pub fn size(
    flag: &str,
    rest: &mut impl Iterator<Item = String>,
) -> Result<crcbl_shell::PhysicalSize, String> {
    let Some(value) = rest.next() else {
        return Err(format!("{flag} needs a size"));
    };
    let (w, h) = match value.split_once('x') {
        Some(pair) => pair,
        None => return Err(format!("not a WxH size: {value}")),
    };
    let (width, height) = match (w.parse::<u32>(), h.parse::<u32>()) {
        (Ok(width), Ok(height)) => (width, height),
        _ => return Err(format!("not a WxH size: {value}")),
    };
    if width == 0 || height == 0 {
        return Err(format!("not a WxH size: {value}"));
    }
    Ok(crcbl_shell::PhysicalSize::new(width, height))
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Runs a whole argv through `consume`, the way a game's loop does, and
    /// reports the first thing that was not `Yes`.
    fn run(argv: &[&str]) -> (Common, Consumed) {
        run_from(Common::new(60), argv)
    }

    /// The same, starting from a `Common` the caller has already shaped — the
    /// only thing a sample does to it before parsing is declare what it
    /// supports.
    fn run_from(mut common: Common, argv: &[&str]) -> (Common, Consumed) {
        let mut rest = argv.iter().map(|s| (*s).to_string());
        while let Some(arg) = rest.next() {
            match common.consume(&arg, &mut rest) {
                Consumed::Yes => {}
                other => return (common, other),
            }
        }
        (common, Consumed::Yes)
    }

    fn parsed(argv: &[&str]) -> Common {
        match run(argv) {
            (common, Consumed::Yes) => common,
            (_, other) => panic!("expected a clean parse, got {other:?}"),
        }
    }

    fn rejected(argv: &[&str]) -> String {
        match run(argv) {
            (_, Consumed::Bad(message)) => message,
            (_, other) => panic!("expected a rejection, got {other:?}"),
        }
    }

    /// `--screenshot` is a flag a sample *has*, not one every sample accepts
    /// and half of them ignore. A `Common` that never said
    /// `with_screenshot` hands it straight back, which is what every sample's
    /// parser turns into "unknown argument" and exit 2.
    #[cfg(not(target_arch = "wasm32"))]
    #[test]
    fn a_sample_that_did_not_wire_the_capture_refuses_the_flag() {
        let (common, consumed) = run(&["--screenshot", "/tmp/frame.png"]);
        assert_eq!(consumed, Consumed::No);
        assert_eq!(common.screenshot, None);
        assert!(
            !common.headless,
            "and it did not quietly go headless either"
        );
    }

    /// The half that makes the flag mean something: it names the file *and* it
    /// turns the window off, in either order, because reading a presented
    /// image back off a real surface is not something every backend can do.
    #[cfg(not(target_arch = "wasm32"))]
    #[test]
    fn the_screenshot_flag_names_a_file_and_forces_the_offscreen_path() {
        for argv in [
            ["--screenshot", "/tmp/frame.png"].as_slice(),
            ["--screenshot", "/tmp/frame.png", "--frames", "8"].as_slice(),
        ] {
            let (common, consumed) = run_from(Common::new(60).with_screenshot(), argv);
            assert_eq!(consumed, Consumed::Yes);
            assert_eq!(
                common.screenshot.as_deref(),
                Some(std::path::Path::new("/tmp/frame.png"))
            );
            assert!(common.headless, "--screenshot has to imply --headless");
        }
    }

    /// The frame the request names is the run's **last**, so a picture is of a
    /// sample that has finished starting up — and a bare `--screenshot` still
    /// gets one, because forcing headless on is what gives the run a budget.
    #[cfg(not(target_arch = "wasm32"))]
    #[test]
    fn the_request_names_the_runs_last_frame() {
        let common = run_from(
            Common::new(60).with_screenshot(),
            &["--screenshot", "/tmp/frame.png", "--frames", "8"],
        )
        .0;
        let request = common.screenshot_request().expect("a request");
        assert_eq!(request.frame, 8);
        assert_eq!(request.path, std::path::Path::new("/tmp/frame.png"));

        let default = run_from(
            Common::new(60).with_screenshot(),
            &["--screenshot", "/tmp/frame.png"],
        )
        .0;
        assert_eq!(
            default.screenshot_request().expect("a request").frame,
            HEADLESS_FRAME_BUDGET
        );
    }

    /// A path is not optional, and a run without one must not start.
    #[cfg(not(target_arch = "wasm32"))]
    #[test]
    fn a_screenshot_with_no_path_is_refused() {
        let (_, consumed) = run_from(Common::new(60).with_screenshot(), &["--screenshot"]);
        assert_eq!(
            consumed,
            Consumed::Bad("--screenshot needs a path".to_string())
        );
    }

    /// Nothing was asked for, so nothing is requested — the flag is the only
    /// thing that arms a capture.
    #[cfg(not(target_arch = "wasm32"))]
    #[test]
    fn a_run_that_asked_for_no_screenshot_requests_none() {
        assert!(parsed(&["--headless"]).screenshot_request().is_none());
    }

    #[test]
    fn the_defaults_are_a_windowed_run_at_the_rate_it_was_built_with() {
        let common = parsed(&[]);
        assert!(!common.headless);
        assert_eq!(common.tick_hz, 60);
        assert_eq!(common.frames, None);
        assert_eq!(common.frame_budget(), None);
        // The assertion three of the four sample parsers dropped. `None` is
        // "let the registry choose", and a default that quietly became
        // `Some(Vulkan)` strands every machine without a driver.
        assert_eq!(common.backend, None);
        assert!(!common.fullscreen);
        assert_eq!(
            common.display_mode(),
            crcbl_shell::DisplayMode::Windowed,
            "a run nobody asked to be fullscreen opens windowed",
        );
        assert_eq!(Common::new(120).tick_hz, 120, "the rate is the game's");
        assert_eq!(
            common.pacing,
            Pacing::Auto,
            "follow the display unless told otherwise",
        );
        assert_eq!(
            common.limit,
            FrameLimit::fps(FrameLimit::DEFAULT_FPS),
            "a runaway guard, not a cap",
        );
        assert_eq!(common.limit.rate(), 1000);
        assert_eq!(
            common.size, None,
            "the window size is the sample's default until --size says otherwise",
        );
    }

    /// Both new flags reach the engine, through the two values a sample passes.
    ///
    /// The seam itself: `--pacing` is only real if it lands in
    /// [`GpuContextDesc::pacing`](crate::engine::GpuContextDesc::pacing), and
    /// `--fps` is only real if it lands in
    /// [`LoopConfig::limit`](crate::engine::LoopConfig::limit). A field parsed
    /// into `Common` and read by nothing is the failure this covers.
    #[test]
    fn the_pacing_and_the_frame_limit_reach_the_gpu_and_the_loop() {
        let common = parsed(&["--pacing", "adaptive", "--fps", "30"]);
        assert_eq!(common.pacing, Pacing::Adaptive);
        assert_eq!(common.limit, FrameLimit::fps(30));

        assert_eq!(common.gpu().pacing, Pacing::Adaptive);
        assert_eq!(common.gpu().backend, None);
        assert_eq!(
            crate::engine::GpuContextDesc::from(common.gpu()).pacing,
            Pacing::Adaptive,
            "and through the desc a sample's Gpu::open builds",
        );
        assert_eq!(common.loop_config().limit, FrameLimit::fps(30));

        // The rest of the config still says what it said, because a sample now
        // gets all five fields from this one call.
        let config = parsed(&["--headless", "--frames", "9", "--tick-hz", "120"]).loop_config();
        assert_eq!(config.tick_hz, 120);
        assert_eq!(config.frames, Some(9));
        assert!(!config.windowed, "a headless run must not idle");
        assert_eq!(config.limit, FrameLimit::default());
    }

    /// Every pacing the engine has is reachable by name, and the word a player
    /// would try first is refused with the word this engine uses.
    #[test]
    fn the_pacing_flag_accepts_every_name_and_says_so_when_it_does_not() {
        for (name, pacing) in [
            ("auto", Pacing::Auto),
            ("vsync", Pacing::Vsync),
            ("adaptive", Pacing::Adaptive),
            ("off", Pacing::Off),
        ] {
            assert_eq!(
                parsed(&["--pacing", name]).pacing,
                pacing,
                "--pacing {name}"
            );
        }

        let refused = rejected(&["--pacing", "vrr"]);
        assert!(refused.contains("vrr"), "{refused}");
        assert!(
            refused.contains("adaptive"),
            "a player who typed the hardware word has to be told this engine's: {refused}",
        );
        assert!(rejected(&["--pacing"]).contains("--pacing"));
    }

    /// Zero is unlimited, and a rate too large for the field is refused rather
    /// than truncated into a plausible one.
    #[test]
    fn the_fps_flag_takes_a_rate_and_zero_means_unlimited() {
        assert_eq!(parsed(&["--fps", "144"]).limit, FrameLimit::fps(144));
        assert_eq!(
            parsed(&["--fps", "0"]).limit,
            FrameLimit::unlimited(),
            "0 is how FrameLimit already spells 'no limit'",
        );
        assert_eq!(parsed(&["--fps", "0"]).limit.period(), None);

        assert!(rejected(&["--fps", "kittens"]).contains("frame rate"));
        // A negative rate fails the u64 parse, which is the same rejection by a
        // different route.
        assert!(rejected(&["--fps", "-1"]).contains("frame rate"));
        assert!(rejected(&["--fps"]).contains("--fps"));

        let too_big = (u64::from(u32::MAX) + 1).to_string();
        assert!(rejected(&["--fps", &too_big]).contains("too large"));
    }

    /// `--fullscreen` reaches [`crcbl_shell::WindowDesc::mode`], which is what
    /// makes it a creation-time request rather than a switch afterwards.
    #[test]
    fn the_fullscreen_flag_is_the_mode_the_window_is_created_in() {
        let common = parsed(&["--fullscreen"]);
        assert!(common.fullscreen);
        assert_eq!(
            common.display_mode(),
            crcbl_shell::DisplayMode::Borderless { monitor: None },
            "None is the only monitor a Wayland client can ask for",
        );
    }

    /// Every spelling the CI harness scripts use.
    #[test]
    fn the_backend_flag_accepts_every_name_the_registry_knows() {
        for name in ["vk", "vulkan"] {
            assert_eq!(
                parsed(&["--backend", name]).backend,
                Some(GpuBackend::Vulkan),
                "--backend {name}",
            );
        }
        for name in ["mtl", "metal"] {
            assert_eq!(
                parsed(&["--backend", name]).backend,
                Some(GpuBackend::Metal),
                "--backend {name}",
            );
        }
        for name in ["dx12", "d3d12"] {
            assert_eq!(
                parsed(&["--backend", name]).backend,
                Some(GpuBackend::Dx12),
                "--backend {name}",
            );
        }
        for name in ["null", "none"] {
            assert_eq!(parsed(&["--backend", name]).backend, Some(GpuBackend::Null));
        }
        // `wgpu` named `crcbl-wgpu`, deleted 2026-08-21. It must now be
        // *rejected* rather than quietly resolving to the browser backend
        // beside it — a stale `CRCBL_GPU=wgpu` in someone's shell should say so.
        assert!(rejected(&["--backend", "wgpu"]).contains("wgpu"));
        assert_eq!(
            parsed(&["--backend", "webgpu"]).backend,
            Some(GpuBackend::WebGpu)
        );
        assert!(rejected(&["--backend", "opengl"]).contains("opengl"));
        assert!(rejected(&["--backend"]).contains("--backend"));
    }

    #[test]
    fn nonsense_is_refused_rather_than_ignored() {
        assert!(rejected(&["--tick-hz", "0"]).contains("tick rate"));
        // A negative rate fails the `u64` parse, which is the same rejection by
        // a different route.
        assert!(rejected(&["--tick-hz", "-1"]).contains("tick rate"));
        // Above `MAX_TICK_RATE` the nanosecond period truncates to zero and the
        // clock asserts — refused here, where the exit code is 2, instead of
        // after the GPU is open.
        assert!(rejected(&["--tick-hz", "1000000001"]).contains("tick rate"));
        assert!(rejected(&["--frames", "0"]).contains("frame count"));
        assert!(rejected(&["--frames"]).contains("--frames"));
    }

    /// `MAX_TICK_RATE` is the largest rate the clock can express — `1e9 / 1e9`
    /// is a 1 ns period — and stays accepted.
    #[test]
    fn the_largest_expressible_tick_rate_is_still_accepted() {
        assert_eq!(parsed(&["--tick-hz", "1000000000"]).tick_hz, 1_000_000_000);
    }

    /// A tick rate above `u32::MAX` parses as a `u64` and has to be caught on
    /// the way down, or it truncates into a plausible-looking rate.
    #[test]
    fn a_tick_rate_too_large_for_the_field_is_refused_rather_than_truncated() {
        let too_big = (u64::from(u32::MAX) + 1).to_string();
        assert!(rejected(&["--tick-hz", &too_big]).contains("tick rate"));
    }

    #[test]
    fn a_headless_run_gets_a_default_budget_and_a_windowed_one_does_not() {
        assert_eq!(
            parsed(&["--headless"]).frame_budget(),
            Some(HEADLESS_FRAME_BUDGET)
        );
        assert_eq!(
            parsed(&["--headless", "--frames", "7"]).frame_budget(),
            Some(7)
        );
        assert_eq!(parsed(&["--frames", "7"]).frame_budget(), Some(7));
    }

    #[test]
    fn the_debug_overlay_flags_override_the_build_profile_default() {
        assert_eq!(parsed(&[]).debug_overlay, None);
        assert_eq!(
            parsed(&[]).debug_overlay_visible(),
            cfg!(debug_assertions),
            "the default is 'on in a dev build'",
        );
        assert!(parsed(&["--debug-overlay"]).debug_overlay_visible());
        assert!(!parsed(&["--no-debug-overlay"]).debug_overlay_visible());
        // Last flag wins, so a wrapper script can append an override.
        assert!(!parsed(&["--debug-overlay", "--no-debug-overlay"]).debug_overlay_visible());
    }

    #[test]
    fn both_spellings_of_help_are_help() {
        assert_eq!(run(&["-h"]).1, Consumed::Help);
        // A `--help` that fell through would reach the game's loop and exit 2
        // with a usage complaint instead of printing the usage.
        assert_eq!(run(&["--help"]).1, Consumed::Help);
    }

    /// A `WxH` value parses into the size the window will be opened at, and
    /// every shape that is not `WxH` is refused rather than guessed at.
    #[test]
    fn the_size_flag_takes_a_width_and_a_height() {
        assert_eq!(
            parsed(&["--size", "1920x1080"]).size,
            Some(crcbl_shell::PhysicalSize::new(1920, 1080)),
        );
        assert_eq!(
            parsed(&["--size", "640x360"]).size,
            Some(crcbl_shell::PhysicalSize::new(640, 360)),
        );
        assert_eq!(
            parsed(&["--size", "1x1"]).size,
            Some(crcbl_shell::PhysicalSize::new(1, 1))
        );

        for bad in [
            "kittens", // not WxH at all
            "1",       // a typo for "1x1", which the positive-number parse would take
            "1920x",   // half a size
            "x1080",   // the other half
            "0x1080",  // a zero dimension
            "1920x0",
            "1920X1080", // the separator is lowercase x
        ] {
            let refused = rejected(&["--size", bad]);
            assert!(refused.contains("WxH"), "--size {bad}: {refused}");
        }
        assert!(rejected(&["--size"]).contains("--size"));
        assert!(
            rejected(&["--size", "99999999999x1"]).contains("WxH"),
            "too wide for u32"
        );
    }

    /// The whole point of `No`: an argument the engine does not know is handed
    /// back rather than rejected, because only the game knows whether it claims
    /// it. A `Bad` here would make `--seed` impossible.
    #[test]
    fn an_unknown_flag_is_handed_back_and_not_rejected() {
        let mut common = Common::new(60);
        let mut rest = std::iter::empty();
        assert_eq!(common.consume("--seed", &mut rest), Consumed::No);
        assert_eq!(common.consume("--max-enemies", &mut rest), Consumed::No);
        assert_eq!(common.consume("nonsense", &mut rest), Consumed::No);
    }

    /// The value helpers are what stop each sample spelling the same rejection
    /// its own way.
    #[test]
    fn the_value_helpers_reject_what_they_should_and_pass_what_they_should() {
        let mut one = ["17".to_string()].into_iter();
        assert_eq!(number("--seed", &mut one, "seed"), Ok(17));

        let mut zero = ["0".to_string()].into_iter();
        assert_eq!(number("--seed", &mut zero, "seed"), Ok(0), "0 is a seed");

        let mut bad = ["kittens".to_string()].into_iter();
        assert!(
            number("--seed", &mut bad, "seed")
                .unwrap_err()
                .contains("seed")
        );

        let mut none = std::iter::empty();
        assert!(
            number("--seed", &mut none, "seed")
                .unwrap_err()
                .contains("--seed")
        );

        let mut zero = ["0".to_string()].into_iter();
        assert!(
            positive("--frames", &mut zero, "frame count")
                .unwrap_err()
                .contains("frame count"),
            "0 is not positive"
        );
    }

    /// The help blocks are pasted into every sample's usage string, so what they claim
    /// has to match what `consume` accepts — a flag documented and not
    /// implemented is worse than one that is neither.
    #[test]
    fn every_flag_the_help_names_is_a_flag_the_parser_takes() {
        let help = format!("{COMMON_OPTIONS_HELP}\n{COMMON_TAIL_HELP}");
        for flag in [
            "--headless",
            "--frames",
            "--tick-hz",
            "--backend",
            "--pacing",
            "--fps",
            "--size",
            "--debug-overlay",
            "--no-debug-overlay",
            "-h",
            "--help",
        ] {
            assert!(help.contains(flag), "{flag} is not in the help");
            // `1` is a legal value for every flag that takes one — a rate, a
            // count — except `--pacing`, whose values are words, and `--size`,
            // which wants a `WxH` pair.
            let value = match flag {
                "--pacing" => "auto",
                "--size" => "1x1",
                _ => "1",
            };
            let mut rest = [value.to_string()].into_iter();
            assert_ne!(
                Common::new(60).consume(flag, &mut rest),
                Consumed::No,
                "{flag} is documented and not implemented"
            );
        }
    }

    /// The same claim for the block that is not in every sample's help: the
    /// flag `SCREENSHOT_HELP` documents has to be one a `Common` that declared
    /// the capability actually takes.
    #[cfg(not(target_arch = "wasm32"))]
    #[test]
    fn the_screenshot_block_documents_a_flag_the_parser_takes() {
        assert!(SCREENSHOT_HELP.contains("--screenshot"));
        let mut rest = ["/tmp/frame.png".to_string()].into_iter();
        assert_ne!(
            Common::new(60)
                .with_screenshot()
                .consume("--screenshot", &mut rest),
            Consumed::No,
            "--screenshot is documented and not implemented"
        );
    }

    /// The front end's contract is "argv in, exit code out", and the four
    /// exit codes are what that contract pins: 0 ran, 1 it failed, 2 bad
    /// arguments, and `--help` runs cleanly. The printed lines are each
    /// game's own and are covered by the scaffold e2e, which runs a real
    /// generated binary and asserts its summary line.
    #[test]
    fn the_front_end_returns_the_contract_exit_codes() {
        assert_eq!(
            run_front_end::<(), u32, &str>(
                "sample",
                "usage",
                Invocation::Run(()),
                |_| Ok(7),
                |n| format!("ran {n}"),
            ),
            ExitCode::SUCCESS,
        );
        assert_eq!(
            run_front_end::<(), u32, &str>(
                "sample",
                "usage",
                Invocation::Run(()),
                |_| Err("boom"),
                |_| String::new(),
            ),
            ExitCode::FAILURE,
        );
        assert_eq!(
            run_front_end::<(), u32, &str>(
                "sample",
                "usage",
                Invocation::Help,
                |_| unreachable!("help never runs the game"),
                |_| String::new(),
            ),
            ExitCode::SUCCESS,
        );
        assert_eq!(
            run_front_end::<(), u32, &str>(
                "sample",
                "usage",
                Invocation::BadUsage("nonsense".into()),
                |_| unreachable!("a bad invocation never runs the game"),
                |_| String::new(),
            ),
            ExitCode::from(2),
        );
    }
}
