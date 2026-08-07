//! Crucible driven as a library, with a loop this crate writes itself.
//!
//! ```text
//! bare [--headless] [--frames N] [--tick-hz N] [--backend B]
//! ```
//!
//! See the [library docs](bare) for why this sample exists and why it must stay
//! hand-written. `--headless` runs the same loop against `HeadlessShell` for a
//! fixed number of frames and exits, which is what makes it a CI job rather than
//! a demo.
//!
//! Exit codes match the `crcbl` CLI's, because a sample is something CI runs:
//! **0** ran, **1** it failed, **2** the arguments were wrong.

use std::process::ExitCode;

use bare::{USAGE, parse, run};

fn main() -> ExitCode {
    crcbl::args::run_front_end(
        "bare",
        USAGE,
        parse(std::env::args().skip(1)),
        run,
        |summary| {
            format!(
                "bare: {} frames, {} ticks, {} events on the {} shell at {}x{}, \
                 {} ({:?})",
                summary.frames,
                summary.ticks,
                summary.events,
                summary.backend,
                summary.extent.0,
                summary.extent.1,
                // What the window system actually did, not what
                // `--fullscreen` asked for. It is free to refuse.
                summary.mode,
                summary.exit,
            )
        },
    )
}
