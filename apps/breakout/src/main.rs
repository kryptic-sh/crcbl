//! Breakout — the first playable Crucible sample.
//!
//! ```text
//! breakout [--headless] [--frames N]
//! ```
//!
//! A 2D breakout game: paddle, ball, brick grid, score, lives. Runs
//! client+server over in-memory transport, orthographic camera, spatial audio
//! for bounces.
//!
//! Exit codes: 0 ran, 1 it failed, 2 bad arguments.

mod app;
mod args;
mod gpu;

use std::process::ExitCode;

use crate::args::{Invocation, parse};

fn main() -> ExitCode {
    crcbl::core::log::init_logging();

    match parse(std::env::args().skip(1)) {
        Invocation::Run(options) => match app::run(&options) {
            Ok(summary) => {
                println!(
                    "breakout: {} frames, {} ticks on the {} shell at {}x{} ({:?})",
                    summary.frames,
                    summary.ticks,
                    summary.backend,
                    summary.extent.0,
                    summary.extent.1,
                    summary.exit,
                );
                ExitCode::SUCCESS
            }
            Err(error) => {
                eprintln!("breakout: {error}");
                ExitCode::FAILURE
            }
        },
        Invocation::Help => {
            println!("{}", args::USAGE);
            ExitCode::SUCCESS
        }
        Invocation::BadUsage(message) => {
            eprintln!("breakout: {message}");
            eprintln!("{}", args::USAGE);
            ExitCode::from(2)
        }
    }
}
