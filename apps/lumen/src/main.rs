//! Lumen — the native front end.
//!
//! ```text
//! lumen [--camera fixed|free] [--force-geometry P] [--force-binding B] [--headless]
//! ```
//!
//! Argv in, exit code out, and nothing else: the fixture itself is the
//! `crcbl_lumen` library this binary links.
//!
//! Exit codes: 0 ran, 1 it failed, 2 bad arguments.

use std::process::ExitCode;

use crcbl_lumen::{USAGE, parse, run};

fn main() -> ExitCode {
    crcbl::args::run_front_end(
        "lumen",
        USAGE,
        parse(std::env::args().skip(1)),
        run,
        |summary| {
            format!(
                "lumen: {} frames, {} ticks on the {} shell at {}x{}, {} \
                 (camera {}, {:?} / {:?} / {:?}, {:?})",
                summary.frames,
                summary.ticks,
                summary.backend,
                summary.extent.0,
                summary.extent.1,
                // What the window system actually did, not what `--fullscreen`
                // asked for. It is free to refuse.
                summary.mode,
                summary.camera.label(),
                // Rule 12's headless half: the three selectors this run's frames
                // were actually drawn through.
                summary.paths.geometry,
                summary.paths.binding,
                summary.paths.lighting,
                summary.exit,
            )
        },
    )
}
