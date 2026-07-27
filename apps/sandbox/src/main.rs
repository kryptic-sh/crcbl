//! Development playground: the engine's first real loop.
//!
//! ```text
//! sandbox [--headless] [--frames N] [--tick-hz N] [--title T]
//! ```
//!
//! Opens a window through `crcbl-shell`, joins it to `crcbl-hal` through
//! [`SurfaceTarget`](crcbl::core::SurfaceTarget), and runs a fixed-timestep
//! loop that presents a frame through the null backend. `--headless` runs the
//! *same* loop against `HeadlessShell` for a fixed number of frames and exits,
//! which is what makes it a CI job rather than a demo.
//!
//! Exit codes match the `crcbl` CLI's, because a sample is something CI runs:
//! **0** ran, **1** it failed, **2** the arguments were wrong.

mod app;
mod args;
mod gpu;
mod triangle;

use std::process::ExitCode;

use crate::args::{Invocation, parse};

fn main() -> ExitCode {
    // `CRCBL_LOG=debug` turns on the per-event lines; the default is warnings.
    crcbl::core::log::init_logging();

    match parse(std::env::args().skip(1)) {
        Invocation::Run(options) => match app::run(&options) {
            Ok(summary) => {
                println!(
                    "sandbox: {} frames, {} ticks, {} events on the {} shell at {}x{} ({:?})",
                    summary.frames,
                    summary.ticks,
                    summary.events,
                    summary.backend,
                    summary.extent.0,
                    summary.extent.1,
                    summary.exit,
                );
                ExitCode::SUCCESS
            }
            Err(error) => {
                eprintln!("sandbox: {error}");
                ExitCode::FAILURE
            }
        },
        Invocation::Help => {
            println!("{}", args::USAGE);
            ExitCode::SUCCESS
        }
        Invocation::BadUsage(message) => {
            eprintln!("sandbox: {message}");
            eprintln!("{}", args::USAGE);
            // 2 = bad invocation, the same contract `crcbl --help` states.
            ExitCode::from(2)
        }
    }
}
