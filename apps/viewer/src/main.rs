//! viewer — the native front end.
//!
//! ```text
//! viewer <MODEL> [OPTIONS]
//! ```
//!
//! Argv in, exit code out, and nothing else: the tool itself is the
//! `crcbl_viewer` library this binary links.
//!
//! Exit codes match every other sample's, because a sample is something CI
//! runs: **0** ran, **1** it failed — which here includes a file that would not
//! load — and **2** the arguments were wrong.

use std::process::ExitCode;

use crcbl_viewer::{USAGE, parse, run};

fn main() -> ExitCode {
    crcbl::args::run_front_end(
        "viewer",
        USAGE,
        parse(std::env::args().skip(1)),
        run,
        |summary| {
            format!(
                "viewer: {} frames, {} events on the {} shell at {}x{}, {} \
                 ({} instances, {} skipped, effects {}, {:?})",
                summary.frames,
                summary.events,
                summary.backend,
                summary.extent.0,
                summary.extent.1,
                // What the window system actually did, not what `--fullscreen`
                // asked for. It is free to refuse.
                summary.mode,
                summary.instances,
                summary.skipped,
                // And which of topic 18's effects were in those frames,
                // resolved — the observable for this sample's own
                // `[engine.video]` wiring.
                summary.effects.row(),
                summary.exit,
            )
        },
    )
}
