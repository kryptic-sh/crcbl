//! The Crucible scene editor.
//!
//! ```text
//! editor [--headless] [--frames N] [--backend B] [SCENE_DIR]
//! ```
//!
//! See the [library docs](crcbl_editor) for what the slice delivers.
//! `--headless` runs the same loop against the headless shell for a fixed
//! number of frames and exits, which is what makes it a CI job rather than a
//! demo.
//!
//! Exit codes match the `crcbl` CLI's, because a tool is something CI runs:
//! **0** ran, **1** it failed, **2** the arguments were wrong.

use std::process::ExitCode;

use crcbl_editor::{USAGE, parse, run};

fn main() -> ExitCode {
    crcbl::args::run_front_end(
        "editor",
        USAGE,
        parse(std::env::args().skip(1)),
        run,
        |summary| {
            format!(
                "editor: {} frames, {} ticks, {} events on the {} shell at {}x{}, \
                 {} ({:?}), {} entities, {} commands",
                summary.run.frames,
                summary.run.ticks,
                summary.run.events,
                summary.run.backend,
                summary.run.extent.0,
                summary.run.extent.1,
                // What the window system actually did, not what `--fullscreen`
                // asked for. It is free to refuse.
                summary.run.mode,
                summary.run.exit,
                summary.entities,
                summary.commands,
            )
        },
    )
}
