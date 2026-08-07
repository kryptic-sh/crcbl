//! Flappy — the native front end.
//!
//! ```text
//! flappy [--headless] [--frames N] [--seed N]
//! ```
//!
//! Argv in, exit code out, and nothing else: the game itself is the `flappy`
//! library this binary links, which is also what the browser's wasm entry point
//! drives.
//!
//! Exit codes: 0 ran, 1 it failed, 2 bad arguments.

use std::process::ExitCode;

use crcbl_flappy::{USAGE, parse, run};

fn main() -> ExitCode {
    crcbl::args::run_front_end(
        "flappy",
        USAGE,
        parse(std::env::args().skip(1)),
        run,
        |summary| {
            format!(
                "flappy: {} frames, {} ticks on the {} shell at {}x{}, {} \
                 (score {}, {:?}, {:?})",
                summary.frames,
                summary.ticks,
                summary.backend,
                summary.extent.0,
                summary.extent.1,
                // What the window system actually did, not what
                // `--fullscreen` asked for. It is free to refuse.
                summary.mode,
                summary.score,
                summary.state,
                summary.exit,
            )
        },
    )
}
