//! Argument parsing for the bracket sample.
//!
//! ```text
//! bracket [--headless] [--frames N] [--size WxH] [--seed N] [--players N]
//! ```
//!
//! # What is left here after the engine took the shared half
//!
//! [`crcbl::args::Common`] owns `--headless`, `--frames`, `--tick-hz`,
//! `--backend`, `--size`, `--screenshot` and the debug-overlay pair, because
//! those are the *engine's* vocabulary. This file is bracket's own: its usage
//! prose, the two flags that shape the population, and the defaults that go
//! with them.
//!
//! # `sim` is not parsed here
//!
//! `src/main.rs` routes the `sim` subcommand to its own parser before this one
//! is reached, so a word this parser does not recognise is genuinely unknown
//! rather than a command it forgot about. The usage text names the subcommand
//! anyway — a reader who typed `bracket --help` has no other place to find it.

use crcbl::args::{Common, Consumed};

/// The `--help` text.
///
/// Written out rather than assembled from [`crcbl::args::COMMON_OPTIONS_HELP`]
/// and [`crcbl::args::COMMON_TAIL_HELP`], because `concat!` takes literals and a
/// `&'static str` const is not one. It cannot drift from them silently:
/// `the_shared_half_of_the_usage_text_is_the_engines_verbatim` asserts this
/// string *contains* both blocks byte for byte.
pub const USAGE: &str = "\
bracket — matchmaking, rating and ranked session flow

USAGE:
    bracket [OPTIONS]        Watch a population converge, windowed or headless
    bracket sim [OPTIONS]    Run one headless and print a report instead.
                             `bracket sim --help` describes its own flags.

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
    --seed <N>           Population seed. The same seed is the same ladder.
    --players <N>        How many synthetic players queue against each other
                         (default 64, and at least two — a matchmaker with one
                         player has nobody to pair them with).
    --screenshot <PATH>  Write the run's last presented frame to PATH as a PNG.
                         Turns --headless on: the frame is read back off the
                         offscreen ring, which is the only surface every backend
                         can copy a presented image out of.
    --debug-overlay      Start with the debug panel visible (F3 toggles it)
    --no-debug-overlay   Start with it hidden. The default is 'visible in a
                         debug build, hidden in a release build'
    -h, --help           Print this help";

/// What the command line asked bracket for.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Options {
    /// The flags every sample has.
    pub common: Common,
    /// The population seed. The same seed is the same ladder.
    pub seed: u64,
    /// How many synthetic players the population holds.
    pub players: usize,
}

impl Default for Options {
    fn default() -> Self {
        Self {
            // `with_screenshot` is what makes `--screenshot` a flag this binary
            // has rather than an unknown argument: `crate::app::assemble` arms
            // the request on the context, and a sample that had not done that
            // must refuse the flag instead of writing nothing.
            #[cfg(not(target_arch = "wasm32"))]
            common: Common::new(crate::app::DEFAULT_TICK_HZ).with_screenshot(),
            #[cfg(target_arch = "wasm32")]
            common: Common::new(crate::app::DEFAULT_TICK_HZ),
            seed: crate::app::DEFAULT_SEED,
            players: crate::app::DEFAULT_PLAYERS,
        }
    }
}

/// What the command line asked for.
pub type Invocation = crcbl::args::Invocation<Options>;

/// Parses a flat `["--flag", "value", "--flag2"]` iterator.
///
/// Every argument is offered to the shared set first; what comes back as
/// [`Consumed::No`] is bracket's to claim, and what bracket does not claim
/// either is the unknown-argument rejection.
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
            // Two is the floor rather than one because the whole sample is
            // pairing: `Sim::new` clamps up to it, and a flag that silently
            // gave back more players than it was asked for would be worse than
            // a rejection.
            "--players" => match crcbl::args::positive("--players", &mut args, "player count") {
                // `usize::try_from` rather than a cast: on a 32-bit target a
                // `u64` player count does not fit, and a truncating cast would
                // turn an absurd number into a plausible one.
                Ok(asked) => match usize::try_from(asked) {
                    Ok(players) if players >= 2 => options.players = players,
                    _ => {
                        return Invocation::BadUsage(format!(
                            "not a player count a match can be made from: {asked}"
                        ));
                    }
                },
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

    /// The engine tests the shared flags; what is bracket's to assert is that
    /// this sample's defaults reach them — the tick rate, the seed and the
    /// population size, none of which the engine can know.
    #[test]
    fn the_defaults_are_a_windowed_run_on_the_published_population() {
        let options = parsed(&[]);
        assert!(!options.common.headless);
        assert_eq!(options.common.tick_hz, crate::app::DEFAULT_TICK_HZ);
        assert_eq!(options.seed, crate::app::DEFAULT_SEED);
        assert_eq!(options.players, crate::app::DEFAULT_PLAYERS);
        assert_eq!(options.common.frame_budget(), None);
        assert_eq!(options.common.backend, None);
    }

    /// The shared flags still work *through this parser*, which is the join the
    /// engine's own tests cannot make: a sample that forgot to call `consume`
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
        assert_eq!(
            parsed(&["--size", "640x480"]).common.size,
            Some(crcbl::shell::PhysicalSize::new(640, 480))
        );
        assert!(rejected(&["--tick-hz", "0"]).contains("tick rate"));
        assert!(matches!(
            parse(["--help".to_string()].into_iter()),
            Invocation::Help
        ));
    }

    /// `--screenshot` is a flag this sample answers to rather than one it
    /// refuses, which is `with_screenshot` on the default `Common` and nothing
    /// else — see [`crcbl::args::Common::can_screenshot`].
    #[cfg(not(target_arch = "wasm32"))]
    #[test]
    fn a_screenshot_path_is_accepted_and_forces_a_headless_run() {
        let options = parsed(&["--screenshot", "frame.png"]);
        assert_eq!(
            options.common.screenshot.as_deref(),
            Some(std::path::Path::new("frame.png"))
        );
        assert!(
            options.common.headless,
            "a windowed swapchain is not a surface every backend can copy back"
        );
    }

    #[test]
    fn a_seed_can_be_named_so_two_runs_can_be_compared() {
        assert_eq!(parsed(&["--seed", "17"]).seed, 17);
        assert_ne!(parsed(&["--seed", "17"]).seed, parsed(&[]).seed);
        assert!(rejected(&["--seed", "kittens"]).contains("seed"));
    }

    /// A population too small to pair is refused rather than quietly grown to
    /// the floor `Sim::new` clamps at.
    #[test]
    fn a_population_needs_two_players_to_be_a_population() {
        assert_eq!(parsed(&["--players", "512"]).players, 512);
        assert_eq!(parsed(&["--players", "2"]).players, 2);
        assert!(rejected(&["--players", "1"]).contains("player count"));
        assert!(rejected(&["--players", "0"]).contains("player count"));
        assert!(rejected(&["--players", "lots"]).contains("player count"));
    }

    #[test]
    fn nonsense_is_refused_rather_than_ignored() {
        assert!(rejected(&["--nonsense"]).contains("nonsense"));
    }

    /// The shared flags are documented in two places — here and in
    /// `crcbl::args` — and this is what stops them disagreeing.
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
        assert!(
            USAGE.contains(crcbl::args::SCREENSHOT_HELP),
            "the --screenshot block has drifted from crcbl::args"
        );
        assert!(USAGE.contains("bracket — matchmaking, rating and ranked session flow"));
        for flag in ["--seed", "--players"] {
            assert!(USAGE.contains(flag), "this sample's own {flag} is missing");
        }
        assert!(
            USAGE.contains("bracket sim"),
            "the headless report has no other place to be documented"
        );
    }
}
