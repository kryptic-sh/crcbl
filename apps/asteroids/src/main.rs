//! Asteroids — the native front end.
//!
//! ```text
//! asteroids [--headless] [--frames N] [--seed N]
//! ```
//!
//! Argv in, exit code out, and nothing else: the game itself is the `asteroids`
//! library this binary links, which is also what the browser's wasm entry point
//! will drive once that sub-slice lands.
//!
//! Exit codes: 0 ran, 1 it failed, 2 bad arguments.

use std::process::ExitCode;

use crcbl_asteroids::{Invocation, USAGE, parse, run};

fn main() -> ExitCode {
    crcbl::core::log::init_logging();

    match parse(std::env::args().skip(1)) {
        Invocation::Run(options) => match run(&options) {
            Ok(summary) => {
                println!(
                    "asteroids: {} frames, {} ticks on the {} shell at {}x{} \
                     (score {}, wave {}, lives {}, {:?}, {:?})",
                    summary.frames,
                    summary.ticks,
                    summary.backend,
                    summary.extent.0,
                    summary.extent.1,
                    summary.score,
                    summary.wave + 1,
                    summary.lives,
                    summary.state,
                    summary.exit,
                );
                ExitCode::SUCCESS
            }
            Err(error) => {
                eprintln!("asteroids: {error}");
                ExitCode::FAILURE
            }
        },
        Invocation::Help => {
            println!("{USAGE}");
            ExitCode::SUCCESS
        }
        Invocation::BadUsage(message) => {
            eprintln!("asteroids: {message}");
            eprintln!("{USAGE}");
            ExitCode::from(2)
        }
    }
}
