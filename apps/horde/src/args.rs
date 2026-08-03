//! Argument parsing for the horde sample.
//!
//! ```text
//! horde [--headless] [--frames N] [--tick-hz N] [--backend B] [--seed N]
//!       [--max-enemies N] [--prefill N] [--wall-clock]
//! ```
//!
//! # What is left here after the engine took the shared half
//!
//! [`crcbl::args::Common`] owns `--headless`, `--frames`, `--tick-hz`,
//! `--backend` and the debug-overlay pair. This file is horde's own, and it is
//! the largest of the four remainders because horde is the sample with the most
//! to say: a seed, an enemy cap, a prefill for the scale measurement, and the
//! `--wall-clock` escape hatch that lets a headless run read the real clock.

use crcbl::args::{Common, Consumed};

/// The `--help` text.
///
/// Cannot drift from [`crcbl::args::COMMON_OPTIONS_HELP`] silently:
/// `the_shared_half_of_the_usage_text_is_the_engines_verbatim` asserts
/// this string contains both shared blocks byte for byte.
pub const USAGE: &str = "\
horde — the engine's fourth game, and its scale sample

USAGE:
    horde [OPTIONS]

OPTIONS:
    --headless           Run without a window (for CI / determinism tests)
    --frames <N>         Stop after N presented frames
    --tick-hz <N>        Simulation rate in Hz (default 60). Sets the server's
                         clock, the ECS timestep and every integrator.
    --backend <B>        GPU backend: vk, vulkan, null, none or wgpu
    --fullscreen         Open borderless instead of windowed. F11 still toggles.
                         A window system may refuse; the summary reports what
                         it actually did, not what was asked for.
    --seed <N>           Run seed. The same seed is the same horde.
    --max-enemies <N>    Ceiling on live enemies (default 1500). The plan's
                         target is 10000; raising it is what the scale
                         sub-slice measures.
    --prefill <N>        Stage N enemies over the whole arena before the first
                         frame, and raise --max-enemies to fit them. The scale
                         measurement's fixture: the spawner would take over ten
                         minutes to reach the plan's target and nothing lives
                         that long.
    --wall-clock         Drive a headless run from the real monotonic clock, so
                         the debug panel's frame timing measures the frame
                         rather than reporting the fixed step a headless clock
                         hands it. Windowed runs already do this.
    --debug-overlay      Start with the debug panel visible (F3 toggles it)
    --no-debug-overlay   Start with it hidden. The default is 'visible in a
                         debug build, hidden in a release build'
    -h, --help           Print this help";

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Options {
    /// The flags every sample has.
    pub common: Common,
    /// The run seed. The same seed is the same horde.
    pub seed: u64,
    /// The ceiling on live enemies. See [`crate::game::DEFAULT_MAX_ENEMIES`].
    pub max_enemies: usize,
    /// How many enemies to stage before the first frame. See
    /// [`crate::game::Game::stage_field`].
    pub prefill: usize,
    /// Whether a headless run reads the real clock. See [`USAGE`].
    pub wall_clock: bool,
}

impl Default for Options {
    fn default() -> Self {
        Self {
            common: Common::new(crate::game::DEFAULT_TICK_HZ),
            seed: crate::game::DEFAULT_SEED,
            max_enemies: crate::game::DEFAULT_MAX_ENEMIES,
            prefill: 0,
            wall_clock: false,
        }
    }
}

impl Options {
    /// Whether the loop reads the real monotonic clock.
    ///
    /// Windowed runs always do. A headless one does only when `--wall-clock`
    /// asked for it, because the fixed step is what makes a scripted headless
    /// run reproducible — and reproducibility is worth more than a frame
    /// timing to every headless run except the one taking a measurement.
    #[must_use]
    pub const fn real_clock(&self) -> bool {
        !self.common.headless || self.wall_clock
    }

    /// The simulation setup these options describe.
    ///
    /// **`--prefill` raises the cap to fit.** A prefill larger than
    /// `max_enemies` would otherwise be silently truncated by
    /// [`Game::stage_field`](crate::game::Game::stage_field) and the run would
    /// measure a field of the wrong size while reporting the flag it was given.
    #[must_use]
    pub fn setup(&self) -> crate::game::Setup {
        crate::game::Setup {
            headless: self.common.headless,
            tick_hz: self.common.tick_hz,
            seed: self.seed,
            max_enemies: self.max_enemies.max(self.prefill),
        }
    }
}

/// What the command line asked for.
pub type Invocation = crcbl::args::Invocation<Options>;

/// Parses a flat `["--flag", "value", "--flag2"]` iterator.
///
/// Every argument is offered to the shared set first; what comes back as
/// [`Consumed::No`] is horde's to claim, and what horde does not claim either
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
            "--wall-clock" => options.wall_clock = true,
            "--seed" => match crcbl::args::number("--seed", &mut args, "seed") {
                Ok(seed) => options.seed = seed,
                Err(message) => return Invocation::BadUsage(message),
            },
            "--max-enemies" => {
                match crcbl::args::positive("--max-enemies", &mut args, "enemy cap") {
                    Ok(cap) => match usize::try_from(cap) {
                        Ok(cap) => options.max_enemies = cap,
                        Err(_) => {
                            return Invocation::BadUsage(format!(
                                "not a positive enemy cap: {cap}"
                            ));
                        }
                    },
                    Err(message) => return Invocation::BadUsage(message),
                }
            }
            "--prefill" => match crcbl::args::number("--prefill", &mut args, "enemy count") {
                Ok(count) => match usize::try_from(count) {
                    Ok(count) => options.prefill = count,
                    Err(_) => {
                        return Invocation::BadUsage(format!("not an enemy count: {count}"));
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
    use crcbl::backend::GpuBackend;

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

    #[test]
    fn the_defaults_are_a_windowed_sixty_hertz_run_on_the_published_seed() {
        let options = parsed(&[]);
        assert!(!options.common.headless);
        assert_eq!(options.common.tick_hz, crate::game::DEFAULT_TICK_HZ);
        assert_eq!(options.seed, crate::game::DEFAULT_SEED);
        assert_eq!(options.max_enemies, crate::game::DEFAULT_MAX_ENEMIES);
        assert_eq!(options.common.frames, None);
        assert_eq!(options.common.frame_budget(), None);
        // Breakout has asserted this since it was the only sample and the three
        // parsers copied from it dropped it. `None` is "let the registry
        // choose", and a default that quietly became `Some(Vk)` would strand
        // every machine without a driver — which is what it did to CI.
        assert_eq!(options.common.backend, None);
    }

    /// The join the engine's own tests cannot make: a game that forgot to call
    /// `consume` would pass every test in `crcbl::args` and reject `--headless`
    /// here. The flag *set* is tested there; that it is reached is tested here.
    #[test]
    fn the_shared_flags_reach_the_common_set_through_this_parser() {
        assert!(parsed(&["--headless"]).common.headless);
        assert_eq!(parsed(&["--frames", "7"]).common.frames, Some(7));
        assert_eq!(parsed(&["--tick-hz", "30"]).common.tick_hz, 30);
        assert_eq!(
            parsed(&["--backend", "vk"]).common.backend,
            Some(GpuBackend::Vulkan)
        );
        assert!(rejected(&["--backend", "metal"]).contains("metal"));
        assert!(matches!(
            parse(["--help".to_string()].into_iter()),
            Invocation::Help
        ));
    }

    /// The shared flags are documented in two places — here and in
    /// `crcbl::args` — and this is what stops them disagreeing.
    ///
    /// A containment check on the whole block, not a flag-by-flag one: a
    /// reworded description or a changed indent is exactly the drift that would
    /// otherwise ship, and a test looking only for `"--headless"` would miss
    /// both. This module's header promised this test before it existed, which
    /// is the defect it now closes.
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
        assert!(USAGE.contains("horde — the engine's fourth game"));
    }

    #[test]
    fn nonsense_is_refused_rather_than_ignored() {
        assert!(rejected(&["--tick-hz", "0"]).contains("tick rate"));
        // A *negative* rate parses as `u32` and fails, which is the same
        // rejection by a different route — breakout tests it and the three
        // copies of this parser stopped.
        assert!(rejected(&["--tick-hz", "-1"]).contains("tick rate"));
        assert!(rejected(&["--frames", "0"]).contains("frame count"));
        assert!(rejected(&["--max-enemies", "0"]).contains("enemy cap"));
        assert!(rejected(&["--seed", "kittens"]).contains("seed"));
        assert!(rejected(&["--nonsense"]).contains("nonsense"));
    }

    /// The enemy cap is the knob the scale sub-slice drives, so it has to reach
    /// the simulation rather than only the parser.
    #[test]
    fn the_enemy_cap_is_a_knob_and_it_reaches_the_setup() {
        let options = parsed(&["--max-enemies", "9000"]);
        assert_eq!(options.max_enemies, 9_000);
        assert_eq!(options.setup().max_enemies, 9_000);
        assert_eq!(
            parsed(&[]).setup().max_enemies,
            crate::game::DEFAULT_MAX_ENEMIES,
        );
        assert!(USAGE.contains("--max-enemies"));
    }

    /// **A prefill raises the cap to fit itself.**
    ///
    /// `--prefill 10000` against the default cap of 1500 would otherwise stage
    /// 1500 and report success, which is the flag doing a sixth of what it was
    /// asked and saying nothing.
    #[test]
    fn a_prefill_larger_than_the_cap_raises_the_cap() {
        let options = parsed(&["--prefill", "10000"]);
        assert_eq!(options.prefill, 10_000);
        assert_eq!(options.max_enemies, crate::game::DEFAULT_MAX_ENEMIES);
        assert_eq!(
            options.setup().max_enemies,
            10_000,
            "the prefill did not reach the simulation's cap",
        );
        // An explicit cap above the prefill is left where the caller put it.
        assert_eq!(
            parsed(&["--prefill", "1000", "--max-enemies", "9000"])
                .setup()
                .max_enemies,
            9_000,
        );
        assert_eq!(parsed(&[]).prefill, 0);
        assert!(rejected(&["--prefill", "lots"]).contains("enemy count"));
        assert!(rejected(&["--prefill"]).contains("--prefill"));
        assert!(USAGE.contains("--prefill"));
    }

    /// The clock a run reads: real when there is a window, real when the
    /// measurement asked for it, fixed otherwise.
    #[test]
    fn only_a_headless_run_without_wall_clock_gets_the_fixed_step() {
        assert!(parsed(&[]).real_clock(), "a windowed run reads the clock");
        assert!(!parsed(&["--headless"]).real_clock());
        assert!(parsed(&["--headless", "--wall-clock"]).real_clock());
        assert!(parsed(&["--wall-clock"]).real_clock());
        assert!(!parsed(&[]).wall_clock);
        assert!(USAGE.contains("--wall-clock"));
    }

    #[test]
    fn a_seed_can_be_named_so_two_runs_can_be_compared() {
        assert_eq!(parsed(&["--seed", "17"]).seed, 17);
        assert_ne!(parsed(&["--seed", "17"]).seed, parsed(&[]).seed);
        assert_eq!(parsed(&["--seed", "17"]).setup().seed, 17);
    }

    #[test]
    fn a_headless_run_gets_a_default_budget_and_a_windowed_one_does_not() {
        assert_eq!(parsed(&["--headless"]).common.frame_budget(), Some(120));
        assert_eq!(
            parsed(&["--headless", "--frames", "7"])
                .common
                .frame_budget(),
            Some(7)
        );
        assert_eq!(parsed(&["--frames", "7"]).common.frame_budget(), Some(7));
        assert!(parsed(&["--headless"]).setup().headless);
    }

    /// Switching the debug overlay on is one flag, and the default follows the
    /// build profile rather than a constant.
    #[test]
    fn the_debug_overlay_flags_override_the_build_profile_default() {
        assert_eq!(parsed(&[]).common.debug_overlay, None);
        assert_eq!(
            parsed(&[]).common.debug_overlay_visible(),
            cfg!(debug_assertions),
            "the default is 'on in a dev build'",
        );
        assert!(parsed(&["--debug-overlay"]).common.debug_overlay_visible());
        assert!(
            !parsed(&["--no-debug-overlay"])
                .common
                .debug_overlay_visible()
        );
        // Last flag wins, so a wrapper script can append an override.
        assert!(
            !parsed(&["--debug-overlay", "--no-debug-overlay"])
                .common
                .debug_overlay_visible()
        );
        assert!(USAGE.contains("--debug-overlay"));
    }

    #[test]
    fn help_is_help() {
        assert!(matches!(
            parse(["-h".to_string()].into_iter()),
            Invocation::Help
        ));
        // Both spellings. Only `-h` was checked here; a `--help` that fell
        // through to "unknown argument" would exit 2 with a usage complaint
        // instead of printing the usage.
        assert!(matches!(
            parse(["--help".to_string()].into_iter()),
            Invocation::Help
        ));
        assert!(USAGE.contains("--seed"));
    }
}
