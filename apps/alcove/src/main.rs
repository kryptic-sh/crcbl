//! Alcove — the native front end.
//!
//! ```text
//! alcove [--camera fixed|free] [--force-geometry P] [--force-binding B]
//!        [--no-ao] [--ao-view] [--technique T] [--split [AT]] [--headless]
//! ```
//!
//! Argv in, exit code out, and nothing else: the fixture itself is the
//! `crcbl_alcove` library this binary links.
//!
//! Exit codes: 0 ran, 1 it failed, 2 bad arguments.

use std::process::ExitCode;

use crcbl_alcove::{USAGE, parse, run};

fn main() -> ExitCode {
    crcbl::args::run_front_end(
        "alcove",
        USAGE,
        parse(std::env::args().skip(1)),
        run,
        |summary| {
            format!(
                "alcove: {} frames, {} ticks on the {} shell at {}x{}, {} \
                 (camera {}, {:?} / {:?} / {:?}, effects {}, technique {} vs {}, seam {}, \
                 radius {:.3}, intensity {:.2}, cost {}, {:?})",
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
                // The occlusion state, which is what this fixture is for: the
                // near side's technique, the far side's, and where the seam
                // between them stood.
                summary.knobs.near_side(),
                summary.knobs.far_side(),
                summary.knobs.seam_row(),
                summary.knobs.radius,
                summary.knobs.intensity,
                // The charter's "cost per technique, per frame", in the headless
                // summary as well as on the panel.
                summary.occlusion_cost.row(),
                summary.exit,
            )
        },
    )
}
