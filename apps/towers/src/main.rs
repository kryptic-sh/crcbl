//! Towers — the native front end.
//!
//! ```text
//! towers [--headless] [--frames N] [--size WxH] [--tick-hz N] …
//!        [--host [PORT] | --join IP:PORT | --browse]
//! ```
//!
//! Argv in, exit code out, and nothing else: the sample itself is the
//! `crcbl_towers` library this binary links, which is also what the browser's
//! wasm entry point will drive once the demo page lands.
//!
//! Exit codes: 0 ran, 1 it failed, 2 bad arguments.

use std::process::ExitCode;

use crcbl_towers::{USAGE, parse, run};

fn main() -> ExitCode {
    crcbl::args::run_front_end(
        "towers",
        USAGE,
        parse(std::env::args().skip(1)),
        run,
        |summary| {
            format!(
                "towers: {} frames, {} ticks on the {} shell at {}x{}, {} \
                 ({} gold, {} lives, wave {}, {} kills, {} leaks, {} built, {} refused, {}, \
                 {:?}/{:?}/{:?}, {:?})",
                summary.run.frames,
                summary.run.ticks,
                summary.run.backend,
                summary.run.extent.0,
                summary.run.extent.1,
                // What the window system actually did, not what `--fullscreen`
                // asked for. It is free to refuse.
                summary.run.mode,
                summary.gold,
                summary.lives,
                summary.wave,
                summary.kills,
                summary.leaks,
                summary.built,
                summary.refused,
                summary.outcome.label(),
                // Rule 12 in the summary line, which is where a headless CI run
                // reads it.
                summary.paths.geometry,
                summary.paths.binding,
                summary.paths.lighting,
                summary.run.exit,
            )
        },
    )
}
