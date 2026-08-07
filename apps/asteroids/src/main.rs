//! Asteroids — the native front end.
//!
//! ```text
//! asteroids [--headless] [--frames N] [--seed N]
//! ```
//!
//! Argv in, exit code out, and nothing else: the game itself is the `asteroids`
//! library this binary links, which is also what the browser's wasm entry point
//! will drive once that sub-slice lands.
//!
//! Exit codes: 0 ran, 1 it failed, 2 bad arguments.

use std::process::ExitCode;

use crcbl_asteroids::{USAGE, parse, run};

fn main() -> ExitCode {
    crcbl::args::run_front_end(
        "asteroids",
        USAGE,
        parse(std::env::args().skip(1)),
        run,
        |summary| {
            format!(
                "asteroids: {} frames, {} ticks on the {} shell at {}x{}, {} \
                 (score {}, wave {}, lives {}, {:?}, {:?})",
                summary.frames,
                summary.ticks,
                summary.backend,
                summary.extent.0,
                summary.extent.1,
                // What the window system actually did, not what
                // `--fullscreen` asked for. It is free to refuse.
                summary.mode,
                summary.score,
                summary.wave + 1,
                summary.lives,
                summary.state,
                summary.exit,
            )
        },
    )
}
