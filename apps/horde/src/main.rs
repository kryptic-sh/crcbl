//! Horde — the native front end.
//!
//! ```text
//! horde [--headless] [--frames N] [--seed N] [--max-enemies N]
//! ```
//!
//! Argv in, exit code out, and nothing else: the game itself is the `horde`
//! library this binary links, which is also what the browser's wasm entry point
//! will drive once that sub-slice lands.
//!
//! Exit codes: 0 ran, 1 it failed, 2 bad arguments.

use std::process::ExitCode;

use crcbl_horde::{USAGE, parse, run};

fn main() -> ExitCode {
    crcbl::args::run_front_end(
        "horde",
        USAGE,
        parse(std::env::args().skip(1)),
        run,
        |summary| {
            format!(
                "horde: {} frames, {} ticks on the {} shell at {}x{}, {} \
                 (survived {:.1}s, {} kills, level {}, {} enemies left, \
                 scene {:?}, {:?}, {:?})",
                summary.frames,
                summary.ticks,
                summary.backend,
                summary.extent.0,
                summary.extent.1,
                // What the window system actually did, not what
                // `--fullscreen` asked for. It is free to refuse.
                summary.mode,
                summary.elapsed,
                summary.kills,
                summary.level,
                summary.enemies,
                summary.scene,
                summary.state,
                summary.exit,
            )
        },
    )
}
