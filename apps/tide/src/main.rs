//! Tide — the native front end.
//!
//! ```text
//! tide [--scene S] [--medium M] [--camera fixed|orbit|free]
//!      [--force-geometry P] [--force-binding B] [--headless]
//! ```
//!
//! Argv in, exit code out, and nothing else: the fixture itself is the
//! `crcbl_tide` library this binary links.
//!
//! Exit codes: 0 ran, 1 it failed, 2 bad arguments.

use std::process::ExitCode;

use crcbl_tide::{USAGE, parse, run};

fn main() -> ExitCode {
    crcbl::args::run_front_end(
        "tide",
        USAGE,
        parse(std::env::args().skip(1)),
        run,
        |summary| {
            format!(
                "tide: {} frames, {} ticks on the {} shell at {}x{}, {} \
             (camera {}, {:?} / {:?} / {:?}, effects {}, scene {}, medium {}, cost {}, {:?})",
                summary.run.frames,
                summary.run.ticks,
                summary.run.backend,
                summary.run.extent.0,
                summary.run.extent.1,
                // What the window system actually did, not what `--fullscreen` asked
                // for.
                summary.run.mode,
                summary.knobs.camera.label(),
                // Rule 12's headless half: the selectors this run drew through.
                summary.paths.geometry,
                summary.paths.binding,
                summary.paths.lighting,
                summary.paths.effects.row(),
                // What the last frame staged, which is what the frames were of.
                summary.staged.0.label(),
                summary.staged.1.label(),
                // The water passes' cost, in the summary as well as on the panel.
                summary.water_cost.row(),
                summary.run.exit,
            )
        },
    )
}
