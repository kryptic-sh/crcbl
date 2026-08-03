//! Crucible driven as a library, with a loop this crate writes itself.
//!
//! ```text
//! bare [--headless] [--frames N] [--tick-hz N] [--backend B]
//! ```
//!
//! See the [library docs](bare) for why this sample exists and why it must stay
//! hand-written. `--headless` runs the same loop against `HeadlessShell` for a
//! fixed number of frames and exits, which is what makes it a CI job rather than
//! a demo.
//!
//! Exit codes match the `crcbl` CLI's, because a sample is something CI runs:
//! **0** ran, **1** it failed, **2** the arguments were wrong.

use std::process::ExitCode;

use bare::{Invocation, USAGE, parse, run};

fn main() -> ExitCode {
    // `CRCBL_LOG=debug` turns on the per-event lines; the default is warnings.
    crcbl::core::log::init_logging();

    match parse(std::env::args().skip(1)) {
        Invocation::Run(options) => match run(&options) {
            Ok(summary) => {
                println!(
                    "bare: {} frames, {} ticks, {} events on the {} shell at {}x{} ({:?})",
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
                eprintln!("bare: {error}");
                ExitCode::FAILURE
            }
        },
        Invocation::Help => {
            println!("{USAGE}");
            ExitCode::SUCCESS
        }
        Invocation::BadUsage(message) => {
            eprintln!("bare: {message}");
            eprintln!("{USAGE}");
            // 2 = bad invocation, the same contract `crcbl --help` states.
            ExitCode::from(2)
        }
    }
}
