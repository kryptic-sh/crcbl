//! Sundial — the native front end.
//!
//! ```text
//! sundial [--camera fixed|counters|free] [--force-geometry P] [--force-binding B]
//!         [--no-shadows] [--filter F] [--split [AT]] [--sun-tick N]
//!         [--sun-paused] [--headless]
//! ```
//!
//! Argv in, exit code out, and nothing else: the fixture itself is the
//! `crcbl_sundial` library this binary links.
//!
//! Exit codes: 0 ran, 1 it failed, 2 bad arguments.

use std::process::ExitCode;

use crcbl_sundial::{USAGE, parse, run};

fn main() -> ExitCode {
    crcbl::args::run_front_end(
        "sundial",
        USAGE,
        parse(std::env::args().skip(1)),
        run,
        |summary| {
            format!(
                "sundial: {} frames, {} ticks on the {} shell at {}x{}, {} \
                 (camera {}, {:?} / {:?} / {:?}, effects {}, filter {} vs {}, seam {}, \
                 sun {}, cost {}, {:?})",
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
                summary.paths.effects_row(),
                // The shadow state, which is what this fixture is for: the near
                // side's filter, the far side's, and where the seam between them
                // stood.
                summary.knobs.near_side(),
                summary.knobs.far_side(),
                summary.knobs.seam_row(),
                // And where the sun stood, without which no frame of this sample
                // can be reproduced.
                summary.clock.sky().row(),
                // The charter's "cost per technique, per frame", in the headless
                // summary as well as on the panel.
                summary.shadow_cost.row(),
                summary.exit,
            )
        },
    )
}
