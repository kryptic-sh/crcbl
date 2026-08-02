//! Argument parsing for the horde sample.
//!
//! ```text
//! horde [--headless] [--frames N] [--tick-hz N] [--backend B] [--seed N]
//!       [--max-enemies N]
//! ```

pub const USAGE: &str = "\
horde — the engine's fourth game, and its scale sample

USAGE:
    horde [OPTIONS]

OPTIONS:
    --headless           Run without a window (for CI / determinism tests)
    --frames <N>         Stop after N presented frames
    --tick-hz <N>        Simulation rate in Hz (default 60). Sets the server's
                         clock, the ECS timestep and the integrator.
    --backend <B>        GPU backend: vk, vulkan, null, none or wgpu
    --seed <N>           Run seed. The same seed is the same horde.
    --max-enemies <N>    Ceiling on live enemies (default 1500). The plan's
                         target is 10000; raising it is what the scale
                         sub-slice measures.
    --debug-overlay      Start with the debug panel visible (F3 toggles it)
    --no-debug-overlay   Start with it hidden. The default is 'visible in a
                         debug build, hidden in a release build'
    -h, --help           Print this help";

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Options {
    pub headless: bool,
    pub frames: Option<u64>,
    pub tick_hz: u32,
    pub backend: Option<crcbl::backend::GpuBackend>,
    pub seed: u64,
    /// The ceiling on live enemies. See [`crate::game::DEFAULT_MAX_ENEMIES`].
    pub max_enemies: usize,
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
            frames: None,
            tick_hz: crate::game::DEFAULT_TICK_HZ,
            backend: None,
            seed: crate::game::DEFAULT_SEED,
            max_enemies: crate::game::DEFAULT_MAX_ENEMIES,
            debug_overlay: None,
        }
    }
}

impl Options {
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

    /// The simulation setup these options describe.
    #[must_use]
    pub fn setup(&self) -> crate::game::Setup {
        crate::game::Setup {
            headless: self.headless,
            tick_hz: self.tick_hz,
            seed: self.seed,
            max_enemies: self.max_enemies,
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Invocation {
    Run(Options),
    Help,
    BadUsage(String),
}

/// Parses a flat `["--flag", "value", "--flag2"]` iterator.
pub fn parse(args: impl Iterator<Item = String>) -> Invocation {
    let mut options = Options::default();
    let mut args = args.peekable();

    while let Some(arg) = args.next() {
        match arg.as_str() {
            "--headless" => options.headless = true,
            "--debug-overlay" => options.debug_overlay = Some(true),
            "--no-debug-overlay" => options.debug_overlay = Some(false),
            "-h" | "--help" => return Invocation::Help,
            "--frames" => {
                let Some(val) = args.next() else {
                    return Invocation::BadUsage("--frames needs a number".into());
                };
                match val.parse::<u64>() {
                    Ok(n) if n > 0 => options.frames = Some(n),
                    _ => return Invocation::BadUsage(format!("not a positive frame count: {val}")),
                }
            }
            "--tick-hz" => {
                let Some(val) = args.next() else {
                    return Invocation::BadUsage("--tick-hz needs a number".into());
                };
                match val.parse::<u32>() {
                    Ok(n) if n > 0 => options.tick_hz = n,
                    _ => return Invocation::BadUsage(format!("not a positive tick rate: {val}")),
                }
            }
            "--max-enemies" => {
                let Some(val) = args.next() else {
                    return Invocation::BadUsage("--max-enemies needs a number".into());
                };
                match val.parse::<usize>() {
                    Ok(n) if n > 0 => options.max_enemies = n,
                    _ => return Invocation::BadUsage(format!("not a positive enemy cap: {val}")),
                }
            }
            "--seed" => {
                let Some(val) = args.next() else {
                    return Invocation::BadUsage("--seed needs a number".into());
                };
                match val.parse::<u64>() {
                    Ok(n) => options.seed = n,
                    Err(_) => return Invocation::BadUsage(format!("not a seed: {val}")),
                }
            }
            "--backend" => {
                let Some(val) = args.next() else {
                    return Invocation::BadUsage("--backend needs a value".into());
                };
                match crcbl::backend::GpuBackend::from_name(&val) {
                    Some(backend) => options.backend = Some(backend),
                    None => {
                        return Invocation::BadUsage(format!(
                            "unknown backend '{val}' — try `vk`, `null` or `wgpu`"
                        ));
                    }
                }
            }
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
        assert!(!options.headless);
        assert_eq!(options.tick_hz, crate::game::DEFAULT_TICK_HZ);
        assert_eq!(options.seed, crate::game::DEFAULT_SEED);
        assert_eq!(options.max_enemies, crate::game::DEFAULT_MAX_ENEMIES);
        assert_eq!(options.frames, None);
        assert_eq!(options.frame_budget(), None);
    }

    /// Every spelling the CI harness scripts use — the same list breakout's
    /// parser was fixed to accept after `--backend vk` was rejected by a
    /// hand-written match.
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

    #[test]
    fn a_seed_can_be_named_so_two_runs_can_be_compared() {
        assert_eq!(parsed(&["--seed", "17"]).seed, 17);
        assert_ne!(parsed(&["--seed", "17"]).seed, parsed(&[]).seed);
        assert_eq!(parsed(&["--seed", "17"]).setup().seed, 17);
    }

    #[test]
    fn a_headless_run_gets_a_default_budget_and_a_windowed_one_does_not() {
        assert_eq!(parsed(&["--headless"]).frame_budget(), Some(120));
        assert_eq!(
            parsed(&["--headless", "--frames", "7"]).frame_budget(),
            Some(7)
        );
        assert_eq!(parsed(&["--frames", "7"]).frame_budget(), Some(7));
        assert!(parsed(&["--headless"]).setup().headless);
    }

    /// Switching the debug overlay on is one flag, and the default follows the
    /// build profile rather than a constant.
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
        assert!(USAGE.contains("--debug-overlay"));
    }

    #[test]
    fn help_is_help() {
        assert!(matches!(
            parse(["-h".to_string()].into_iter()),
            Invocation::Help
        ));
        assert!(USAGE.contains("--seed"));
    }
}
