//! Argument parsing for the breakout sample.
//!
//! ```text
//! breakout [--headless] [--frames N] [--tick-hz N]
//! ```

pub const USAGE: &str = "\
breakout — the first playable Crucible sample

USAGE:
    breakout [OPTIONS]

OPTIONS:
    --headless           Run without a window (for CI / determinism tests)
    --frames <N>         Stop after N presented frames
    --tick-hz <N>        Simulation rate in Hz (default 60). Sets the server's
                         clock, the ECS timestep and every integrator.
    --backend <B>        GPU backend: vk, vulkan, null, none or wgpu
    -h, --help           Print this help";

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Options {
    pub headless: bool,
    pub frames: Option<u64>,
    pub tick_hz: u32,
    pub backend: Option<crcbl::backend::GpuBackend>,
}

impl Default for Options {
    fn default() -> Self {
        Self {
            headless: false,
            frames: None,
            tick_hz: crate::game::DEFAULT_TICK_HZ,
            backend: None,
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
}

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
            // `GpuBackend::from_name`, not a hand-written match: the sandbox
            // and every CI harness script pass `--backend vk`, which a
            // `"vulkan" | "null"` match rejects.
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
    fn the_defaults_are_a_windowed_sixty_hertz_run() {
        let options = parsed(&[]);
        assert!(!options.headless);
        assert_eq!(options.tick_hz, crate::game::DEFAULT_TICK_HZ);
        assert_eq!(options.frames, None);
        assert_eq!(options.backend, None);
        assert_eq!(options.frame_budget(), None);
    }

    /// Every spelling the sandbox and the CI harness scripts use. `vk` in
    /// particular was rejected by a hand-written `"vulkan" | "null"` match
    /// while every `run-*-e2e.sh` passes exactly that.
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
    fn a_zero_tick_rate_and_a_zero_frame_count_are_rejected() {
        assert!(rejected(&["--tick-hz", "0"]).contains("tick rate"));
        assert!(rejected(&["--tick-hz", "-1"]).contains("tick rate"));
        assert!(rejected(&["--frames", "0"]).contains("frame count"));
        assert!(rejected(&["--nonsense"]).contains("nonsense"));
    }

    #[test]
    fn a_headless_run_gets_a_default_budget_and_a_windowed_one_does_not() {
        assert_eq!(parsed(&["--headless"]).frame_budget(), Some(120));
        assert_eq!(
            parsed(&["--headless", "--frames", "7"]).frame_budget(),
            Some(7)
        );
        assert_eq!(parsed(&["--frames", "7"]).frame_budget(), Some(7));
    }

    #[test]
    fn help_is_help() {
        assert!(matches!(
            parse(["-h".to_string()].into_iter()),
            Invocation::Help
        ));
        assert!(matches!(
            parse(["--help".to_string()].into_iter()),
            Invocation::Help
        ));
        assert!(USAGE.contains("--tick-hz"));
    }
}
