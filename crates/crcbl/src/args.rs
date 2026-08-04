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

use crate::backend::GpuBackend;

/// The shared `OPTIONS:` block, so four help texts cannot drift.
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
    --backend <B>        GPU backend: vk, vulkan, null, none or wgpu
    --fullscreen         Open borderless instead of windowed. F11 still toggles.
                         A window system may refuse; the summary reports what
                         it actually did, not what was asked for.";

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
/// so adding a shared flag reaches four games without touching four structs.
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
            "--backend" => {
                let Some(name) = rest.next() else {
                    return Consumed::Bad("--backend needs a value".into());
                };
                match GpuBackend::from_name(&name) {
                    Some(backend) => self.backend = Some(backend),
                    None => {
                        return Consumed::Bad(format!(
                            "unknown backend '{name}' — try `vk`, `null` or `wgpu`"
                        ));
                    }
                }
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

#[cfg(test)]
mod tests {
    use super::*;

    /// Runs a whole argv through `consume`, the way a game's loop does, and
    /// reports the first thing that was not `Yes`.
    fn run(argv: &[&str]) -> (Common, Consumed) {
        let mut common = Common::new(60);
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
        for name in ["null", "none"] {
            assert_eq!(parsed(&["--backend", name]).backend, Some(GpuBackend::Null));
        }
        assert_eq!(
            parsed(&["--backend", "wgpu"]).backend,
            Some(GpuBackend::Wgpu)
        );
        assert!(rejected(&["--backend", "metal"]).contains("metal"));
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

    /// The value helpers are what stop four games spelling the same rejection
    /// four ways.
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

    /// The help blocks are pasted into four usage strings, so what they claim
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
            "--debug-overlay",
            "--no-debug-overlay",
            "-h",
            "--help",
        ] {
            assert!(help.contains(flag), "{flag} is not in the help");
            let mut rest = ["1".to_string()].into_iter();
            assert_ne!(
                Common::new(60).consume(flag, &mut rest),
                Consumed::No,
                "{flag} is documented and not implemented"
            );
        }
    }
}
