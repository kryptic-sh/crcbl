//! Argument parsing for the asteroids sample.
//!
//! ```text
//! asteroids [--headless] [--frames N] [--tick-hz N] [--backend B] [--seed N]
//! ```
//!
//! # What is left here after the engine took the shared half
//!
//! [`crcbl::args::Common`] owns `--headless`, `--frames`, `--tick-hz`,
//! `--backend`, `--size` and the debug-overlay pair, because those are the *engine's*
//! vocabulary — a backend names the GPU registry and a tick rate sets the
//! session's clock — and four games spelling them out was four chances to spell
//! them differently. This file is asteroids' own: its usage prose, its `--seed`,
//! and the default seed that goes with it.

use crcbl::args::{Common, Consumed};

/// The `--help` text.
///
/// Written out rather than assembled from [`crcbl::args::COMMON_OPTIONS_HELP`]
/// and [`crcbl::args::COMMON_TAIL_HELP`], because `concat!` takes literals and
/// a `&'static str` const is not one — the alternative is building the help at
/// run time, which is an allocation for something read once.
///
/// It cannot drift from them silently:
/// `the_shared_half_of_the_usage_text_is_the_engines_verbatim` asserts
/// this string *contains* both blocks byte for byte, so editing one and not the
/// other reddens the build.
pub const USAGE: &str = "\
asteroids — the engine's third game, and its churn sample

USAGE:
    asteroids [OPTIONS]

OPTIONS:
    --headless           Run without a window (for CI / determinism tests)
    --frames <N>         Stop after N presented frames
    --tick-hz <N>        Simulation rate in Hz (default 60). Sets the server's
                         clock, the ECS timestep and every integrator.
    --backend <B>        GPU backend: vk, vulkan, mtl, metal, dx12, d3d12,
                         null, none or wgpu
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
    --seed <N>           Board seed. The same seed is the same rocks.
    --debug-overlay      Start with the debug panel visible (F3 toggles it)
    --no-debug-overlay   Start with it hidden. The default is 'visible in a
                         debug build, hidden in a release build'
    -h, --help           Print this help";

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Options {
    /// The flags every sample has.
    pub common: Common,
    /// The board seed. The same seed is the same rocks.
    pub seed: u64,
}

impl Default for Options {
    fn default() -> Self {
        Self {
            common: Common::new(crate::game::DEFAULT_TICK_HZ),
            seed: crate::game::DEFAULT_SEED,
        }
    }
}

/// What the command line asked for.
pub type Invocation = crcbl::args::Invocation<Options>;

/// Parses a flat `["--flag", "value", "--flag2"]` iterator.
///
/// Every argument is offered to the shared set first; what comes back as
/// [`Consumed::No`] is asteroids' to claim, and what asteroids does not claim either
/// is the unknown-argument rejection.
pub fn parse(args: impl Iterator<Item = String>) -> Invocation {
    let mut options = Options::default();
    let mut args = args.peekable();

    while let Some(arg) = args.next() {
        match options.common.consume(&arg, &mut args) {
            Consumed::Yes => continue,
            Consumed::Help => return Invocation::Help,
            Consumed::Bad(message) => return Invocation::BadUsage(message),
            Consumed::No => {}
        }

        match arg.as_str() {
            "--seed" => match crcbl::args::number("--seed", &mut args, "seed") {
                Ok(seed) => options.seed = seed,
                Err(message) => return Invocation::BadUsage(message),
            },
            other => return Invocation::BadUsage(format!("unknown argument: {other}")),
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

    /// The engine tests the shared flags; what is asteroids' to assert is that
    /// this game's defaults reach them — the tick rate and the seed, both of
    /// which come from `game.rs` and neither of which the engine can know.
    #[test]
    fn the_defaults_are_a_windowed_sixty_hertz_run_on_the_published_board() {
        let options = parsed(&[]);
        assert!(!options.common.headless);
        assert_eq!(options.common.tick_hz, crate::game::DEFAULT_TICK_HZ);
        assert_eq!(options.seed, crate::game::DEFAULT_SEED);
        assert_eq!(options.common.frame_budget(), None);
        assert_eq!(options.common.backend, None);
    }

    /// The shared flags still work *through this parser*, which is the join the
    /// engine's own tests cannot make: a game that forgot to call `consume`
    /// would pass every test in `crcbl::args` and reject `--headless`.
    #[test]
    fn the_shared_flags_reach_the_common_set_through_this_parser() {
        assert!(parsed(&["--headless"]).common.headless);
        assert_eq!(parsed(&["--frames", "7"]).common.frames, Some(7));
        assert_eq!(parsed(&["--tick-hz", "30"]).common.tick_hz, 30);
        assert_eq!(
            parsed(&["--backend", "null"]).common.backend,
            Some(crcbl::backend::GpuBackend::Null)
        );
        assert!(rejected(&["--tick-hz", "0"]).contains("tick rate"));
        assert!(matches!(
            parse(["--help".to_string()].into_iter()),
            Invocation::Help
        ));
    }

    #[test]
    fn a_seed_can_be_named_so_two_runs_can_be_compared() {
        assert_eq!(parsed(&["--seed", "17"]).seed, 17);
        assert_ne!(parsed(&["--seed", "17"]).seed, parsed(&[]).seed);
        assert!(rejected(&["--seed", "kittens"]).contains("seed"));
    }

    #[test]
    fn nonsense_is_refused_rather_than_ignored() {
        assert!(rejected(&["--nonsense"]).contains("nonsense"));
    }

    /// The shared flags are documented in two places — here and in
    /// `crcbl::args` — and this is what stops them disagreeing.
    ///
    /// A containment check on the whole block, not a flag-by-flag one: a
    /// reworded description or a changed indent is exactly the drift that would
    /// otherwise ship, and a test looking only for `"--headless"` would miss
    /// both.
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
        assert!(USAGE.contains("asteroids — the engine's third game, and its churn sample"));
        assert!(USAGE.contains("--seed"), "this game's own flag is missing");
    }
}
