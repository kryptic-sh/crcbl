//! Development playground: the engine's first real loop.
//!
//! ```text
//! sandbox [--headless] [--frames N] [--tick-hz N] [--title T] [--camera MODE]
//! ```
//!
//! Opens a window through `crcbl-shell`, joins it to `crcbl-hal` through
//! [`SurfaceTarget`](crcbl::core::SurfaceTarget), and runs a fixed-timestep
//! loop that draws a lit spinning cube through `crcbl-render`'s graph — into an
//! HDR target, tonemapped to the swapchain, with every barrier computed rather
//! than written. `--headless` runs the *same* loop against `HeadlessShell` for a
//! fixed number of frames and exits, which is what makes it a CI job rather than
//! a demo.
//!
//! Exit codes match the `crcbl` CLI's, because a sample is something CI runs:
//! **0** ran, **1** it failed, **2** the arguments were wrong.

mod app;
mod args;
mod gpu;
mod menu;

use std::process::ExitCode;

use crate::args::{Invocation, parse};

fn main() -> ExitCode {
    // `CRCBL_LOG=debug` turns on the per-event lines; the default is warnings.
    crcbl::core::log::init_logging();
    // And `CRCBL_TRACE=1` turns the CPU spans on. Spelled out here rather than
    // inherited because this sample's front end is deliberately its own — see
    // `crcbl::args::run_front_end`, which does the same two calls for every
    // sample that does share one.
    crcbl::core::trace::init_from_env();

    match parse(std::env::args().skip(1)) {
        Invocation::Run(options) => match app::run(&options) {
            Ok(summary) => {
                println!(
                    "sandbox: {} frames, {} ticks, {} events on the {} shell at {}x{}, \
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
