//! Argument parsing for the breakout sample.
//!
//! ```text
//! breakout [--headless] [--frames N] [--tick-hz N] [--backend B]
//! ```
//!
//! # What is left here after the engine took the shared half
//!
//! [`crcbl::args::Common`] owns every flag breakout has, because breakout has
//! none of its own — its board is fixed, so there is no `--seed` to take. That
//! makes this the smallest of the four parsers and the one that shows the seam
//! most plainly: `Options` wraps `Common`, and `parse` rejects anything the
//! engine did not claim.
//!
//! Still a wrapper rather than a bare alias for `Common`, because the *next*
//! flag breakout grows goes here without changing its callers' types.

use crcbl::args::{Common, Consumed};

/// The `--help` text.
///
/// Cannot drift from [`crcbl::args::COMMON_OPTIONS_HELP`] silently:
/// `the_shared_half_of_the_usage_text_is_the_engines_verbatim` asserts
/// this string contains both shared blocks byte for byte.
pub const USAGE: &str = "\
breakout — the first playable Crucible sample

USAGE:
    breakout [OPTIONS]

OPTIONS:
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
                         which is what makes a scale measurement reproducible.
    --debug-overlay      Start with the debug panel visible (F3 toggles it)
    --no-debug-overlay   Start with it hidden. The default is 'visible in a
                         debug build, hidden in a release build'
    -h, --help           Print this help";

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Options {
    /// The flags every sample has, which for breakout is all of them.
    pub common: Common,
}

impl Default for Options {
    fn default() -> Self {
        Self {
            common: Common::new(crate::game::DEFAULT_TICK_HZ),
        }
    }
}

/// What the command line asked for.
pub type Invocation = crcbl::args::Invocation<Options>;

/// Parses a flat `["--flag", "value", "--flag2"]` iterator.
pub fn parse(args: impl Iterator<Item = String>) -> Invocation {
    let mut options = Options::default();
    let mut args = args.peekable();

    while let Some(arg) = args.next() {
        match options.common.consume(&arg, &mut args) {
            Consumed::Yes => continue,
            Consumed::Help => return Invocation::Help,
            Consumed::Bad(message) => return Invocation::BadUsage(message),
            // Breakout claims nothing of its own, so anything the engine hands
            // back is an unknown argument.
            Consumed::No => return Invocation::BadUsage(format!("unknown argument: {arg}")),
        }
    }

    Invocation::Run(options)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn parsed(argv: &[&str]) -> Options {
        match parse(argv.iter().map(|s| (*s).to_string())) {
            Invocation::Run(options) => options,
            Invocation::Help => panic!("expected a run, got help"),
            Invocation::BadUsage(message) => panic!("expected a run, got: {message}"),
        }
    }

    fn rejected(argv: &[&str]) -> String {
        match parse(argv.iter().map(|s| (*s).to_string())) {
            Invocation::BadUsage(message) => message,
            _ => panic!("expected a rejection"),
        }
    }

    /// What is breakout's to assert is that this game's tick rate reaches the
    /// shared set; the flags themselves are tested in `crcbl::args`.
    #[test]
    fn the_defaults_are_a_windowed_sixty_hertz_run() {
        let options = parsed(&[]);
        assert!(!options.common.headless);
        assert_eq!(options.common.tick_hz, crate::game::DEFAULT_TICK_HZ);
        assert_eq!(options.common.frame_budget(), None);
        assert_eq!(options.common.backend, None);
    }

    /// The join the engine's own tests cannot make: a game that forgot to call
    /// `consume` would pass every test in `crcbl::args` and reject `--headless`
    /// here.
    #[test]
    fn the_shared_flags_reach_the_common_set_through_this_parser() {
        assert!(parsed(&["--headless"]).common.headless);
        assert_eq!(parsed(&["--frames", "7"]).common.frames, Some(7));
        assert_eq!(parsed(&["--tick-hz", "30"]).common.tick_hz, 30);
        assert_eq!(
            parsed(&["--backend", "vk"]).common.backend,
            Some(crcbl::backend::GpuBackend::Vulkan)
        );
        assert!(parsed(&["--debug-overlay"]).common.debug_overlay_visible());
        assert!(
            !parsed(&["--no-debug-overlay"])
                .common
                .debug_overlay_visible()
        );
        assert_eq!(
            parsed(&["--headless"]).common.frame_budget(),
            Some(crcbl::args::HEADLESS_FRAME_BUDGET)
        );
        assert!(rejected(&["--tick-hz", "0"]).contains("tick rate"));
        assert!(rejected(&["--frames", "0"]).contains("frame count"));
        assert!(matches!(
            parse(["--help".to_string()].into_iter()),
            Invocation::Help
        ));
    }

    /// Breakout claims no flags of its own, so **every** unknown argument is a
    /// rejection — including one another sample takes. A `--seed` silently
    /// ignored here would be a run the caller believed was seeded.
    #[test]
    fn an_argument_this_game_does_not_claim_is_refused_including_another_games() {
        assert!(rejected(&["--nonsense"]).contains("nonsense"));
        assert!(rejected(&["--seed", "17"]).contains("--seed"));
    }

    #[test]
    fn the_shared_half_of_the_usage_text_is_the_engines_verbatim() {
        assert!(
            USAGE.contains(crcbl::args::COMMON_OPTIONS_HELP),
            "the shared OPTIONS block has drifted from crcbl::args"
        );
        assert!(
            USAGE.contains(crcbl::args::COMMON_TAIL_HELP),
            "the shared tail has drifted from crcbl::args"
        );
        assert!(USAGE.contains("breakout — the first playable Crucible sample"));
    }
}
