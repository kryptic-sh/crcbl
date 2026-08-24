//! orbit — the native front end.
//!
//! ```text
//! orbit [--headless] [--frames N]
//! ```
//!
//! Argv in, exit code out, and nothing else: the sample itself is the
//! `crcbl_orbit` library this binary links, which is also what the browser's
//! wasm entry point drives.
//!
//! Exit codes: 0 ran, 1 it failed, 2 bad arguments.

use std::process::ExitCode;

use crcbl_orbit::{USAGE, parse, run};

fn main() -> ExitCode {
    crcbl::args::run_front_end(
        "orbit",
        USAGE,
        parse(std::env::args().skip(1)),
        run,
        |summary| {
            format!(
                "orbit: {} frames, {} ticks on the {} shell at {}x{}, {} \
                 ({} at {:.0} m, {} page commands, {:?})",
                summary.frames,
                summary.ticks,
                summary.backend,
                summary.extent.0,
                summary.extent.1,
                // What the window system actually did, not what `--fullscreen`
                // asked for. It is free to refuse.
                summary.mode,
                summary.phase.label(),
                summary.altitude,
                summary.commands,
                summary.exit,
            )
        },
    )
}
