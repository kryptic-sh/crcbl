//! Puppet — the native front end.
//!
//! ```text
//! puppet [--headless] [--frames N] [--size WxH] [--tick-hz N] …
//! ```
//!
//! Argv in, exit code out, and nothing else: the sample itself is the
//! `crcbl_puppet` library this binary links, which is also what the browser's
//! wasm entry point drives.
//!
//! Exit codes: 0 ran, 1 it failed, 2 bad arguments.

use std::process::ExitCode;

use crcbl_puppet::{USAGE, parse, run};

fn main() -> ExitCode {
    crcbl::args::run_front_end(
        "puppet",
        USAGE,
        parse(std::env::args().skip(1)),
        run,
        |summary| {
            format!(
                "puppet: {} frames, {} ticks on the {} shell at {}x{}, {} \
                 (feet at {:.2} {:.2} {:.2}, {} step(s) climbed, {} tick(s) blocked, {:?})",
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
                summary.climbed,
                summary.blocked,
                summary.exit,
            )
        },
    )
}
