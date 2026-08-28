//! options — the native front end.
//!
//! ```text
//! options [--headless] [--frames N]
//! ```
//!
//! Argv in, exit code out, and nothing else: the sample itself is the
//! `crcbl_options` library this binary links.
//!
//! Exit codes: 0 ran, 1 it failed, 2 bad arguments.

use std::process::ExitCode;

use crcbl_options::{USAGE, parse, run};

fn main() -> ExitCode {
    crcbl::args::run_front_end(
        "options",
        USAGE,
        parse(std::env::args().skip(1)),
        run,
        |summary| {
            format!(
                "options: {} frames, {} ticks on the {} shell at {}x{}, {} \
                 ({} edit(s), {}, {:?})",
                summary.frames,
                summary.ticks,
                summary.backend,
                summary.extent.0,
                summary.extent.1,
                // What the window system actually did, not what `--fullscreen`
                // asked for. It is free to refuse.
                summary.mode,
                summary.edits,
                summary.saved,
                summary.exit,
            )
        },
    )
}
