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
    --tick-hz <N>        Simulation rate in Hz (default 60)
    --backend <B>        GPU backend: vulkan or null
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
            tick_hz: 60,
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
            "--backend" => {
                let Some(val) = args.next() else {
                    return Invocation::BadUsage("--backend needs `vulkan` or `null`".into());
                };
                match val.as_str() {
                    "vulkan" => options.backend = Some(crcbl::backend::GpuBackend::Vulkan),
                    "null" => options.backend = Some(crcbl::backend::GpuBackend::Null),
                    other => {
                        return Invocation::BadUsage(format!(
                            "unknown backend '{other}' — use `vulkan` or `null`"
                        ));
                    }
                }
            }
            other => return Invocation::BadUsage(format!("unknown argument: {other}")),
        }
    }

    Invocation::Run(options)
}
