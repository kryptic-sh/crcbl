//! Shard — the native front end.
//!
//! ```text
//! shard [--headless] [--frames N] [--size WxH] [--tick-hz N] …
//! ```
//!
//! Argv in, exit code out, and nothing else: the sample itself is the
//! `crcbl_shard` library this binary links, which is also what the browser's wasm
//! entry point drives.
//!
//! Exit codes: 0 ran, 1 it failed, 2 bad arguments.

use std::process::ExitCode;

use crcbl_shard::{USAGE, parse, run};

fn main() -> ExitCode {
    crcbl::args::run_front_end(
        "shard",
        USAGE,
        parse(std::env::args().skip(1)),
        run,
        |summary| {
            format!(
                "shard: {} frames, {} ticks on the {} shell at {}x{}, {} \
                 (feet at {:.2} {:.2} {:.2}, {} blocked, {} climbed, \
                 {} foes standing, {} health, {}/{} blows landed, torches {}, \
                 {} save(s) and {}, \
                 {:?}/{:?}/{:?}, effects {}, {:?})",
                summary.frames,
                summary.ticks,
                summary.backend,
                summary.extent.0,
                summary.extent.1,
                // What the window system actually did, not what `--fullscreen`
                // asked for. It is free to refuse.
                summary.mode,
                summary.feet[0],
                summary.feet[1],
                summary.feet[2],
                summary.blocked,
                summary.climbed,
                summary.foes_alive,
                summary.health,
                summary.hits,
                summary.swings,
                if summary.torches_lit { "lit" } else { "out" },
                summary.saves,
                if summary.resumed {
                    "resumed from one"
                } else {
                    "nothing to resume"
                },
                // Rule 12 in the summary line, which is where a headless CI run
                // reads it.
                summary.paths.geometry,
                summary.paths.binding,
                summary.paths.lighting,
                summary.paths.effects_row(),
                summary.exit,
            )
        },
    )
}
