//! Tumble — the native front end.
//!
//! ```text
//! tumble [--headless] [--frames N] [--size WxH] [--tick-hz N]
//! ```
//!
//! Argv in, exit code out, and nothing else: the sample itself is the
//! `crcbl_tumble` library this binary links, which is also what the browser's
//! wasm entry point drives.
//!
//! Exit codes: 0 ran, 1 it failed, 2 bad arguments.

use std::process::ExitCode;

use crcbl_tumble::{USAGE, parse, run};

fn main() -> ExitCode {
    crcbl::args::run_front_end(
        "tumble",
        USAGE,
        parse(std::env::args().skip(1)),
        run,
        |summary| {
            let r = &summary.reading;
            format!(
                "tumble: {} frames, {} ticks on the {} shell at {}x{}, {} \
                 ({} flips, momentum drift {:.1e}, box at {:.3} m, wall {} bodies {}+ {}- \
                 contacts, pit {} balls {} pairs, hash {:016x}, {:?})",
                summary.run.frames,
                summary.run.ticks,
                summary.run.backend,
                summary.run.extent.0,
                summary.run.extent.1,
                // What the window system actually did, not what `--fullscreen`
                // asked for. It is free to refuse.
                summary.run.mode,
                r.spin.flips,
                r.spin.momentum_drift,
                r.spin.box_height,
                r.wall.contacts.bodies,
                r.wall.contacts.begun,
                r.wall.contacts.ended,
                r.pit.balls,
                r.pit.contacts.pairs,
                r.hash,
                summary.run.exit,
            )
        },
    )
}
