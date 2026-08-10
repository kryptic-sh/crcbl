//! hud — the native front end.
//!
//! ```text
//! hud [--headless] [--frames N] [--seed N]
//! ```
//!
//! Argv in, exit code out, and nothing else: the sample itself is the `hud`
//! library this binary links.
//!
//! Exit codes: 0 ran, 1 it failed, 2 bad arguments.

use std::process::ExitCode;

use hud::{USAGE, parse, run};

fn main() -> ExitCode {
    crcbl::args::run_front_end(
        "hud",
        USAGE,
        parse(std::env::args().skip(1)),
        run,
        |summary| {
            format!(
                "hud: {} frames, {} ticks on the {} shell at {}x{}, {} \
                 (wave {}, {} page commands, {:?})",
                summary.frames,
                summary.ticks,
                summary.backend,
                summary.extent.0,
                summary.extent.1,
                // What the window system actually did, not what `--fullscreen`
                // asked for. It is free to refuse.
                summary.mode,
                summary.wave,
                summary.commands,
                summary.exit,
            )
        },
    )
}
