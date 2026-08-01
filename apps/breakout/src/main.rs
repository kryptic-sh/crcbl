//! Breakout — the native front end.
//!
//! ```text
//! breakout [--headless] [--frames N]
//! ```
//!
//! Argv in, exit code out, and nothing else: the game itself is the `breakout`
//! library this binary links, which is also what the browser's wasm entry point
//! drives. See that crate's docs for why the split exists.
//!
//! Exit codes: 0 ran, 1 it failed, 2 bad arguments.

use std::process::ExitCode;

use crcbl_breakout::{Invocation, USAGE, parse, run};

fn main() -> ExitCode {
    crcbl::core::log::init_logging();

    match parse(std::env::args().skip(1)) {
        Invocation::Run(options) => match run(&options) {
            Ok(summary) => {
                println!(
                    "breakout: {} frames, {} ticks on the {} shell at {}x{} \
                     (score {}, {:?}, {:?})",
                    summary.frames,
                    summary.ticks,
                    summary.backend,
                    summary.extent.0,
                    summary.extent.1,
                    summary.score,
                    summary.state,
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
            println!("{USAGE}");
            ExitCode::SUCCESS
        }
        Invocation::BadUsage(message) => {
            eprintln!("breakout: {message}");
            eprintln!("{USAGE}");
            ExitCode::from(2)
        }
    }
}
