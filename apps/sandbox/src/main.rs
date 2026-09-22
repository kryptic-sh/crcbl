//! Development playground: the engine's first real loop.
//!
//! ```text
//! sandbox [--headless] [--wall-clock] [--scene NAME] [--frames N]
//! ```
//!
//! Opens a window through `crcbl-shell`, joins it to `crcbl-hal` through
//! [`SurfaceTarget`](crcbl::core::SurfaceTarget), and runs a fixed-timestep
//! loop that draws a selected fixed scene through `crcbl-render`'s graph — into
//! an HDR target, tonemapped to the swapchain, with every barrier computed
//! rather than written. `--headless` runs the *same* loop against
//! `HeadlessShell` for a fixed number of frames and exits, which is what makes
//! it a CI job rather than a demo.
//!
//! Exit codes match the `crcbl` CLI's, because a sample is something CI runs:
//! **0** ran, **1** it failed, **2** the arguments were wrong.

mod app;
mod args;
mod gpu;
mod menu;
mod stress;

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
                     {} (scene {}, effects {}, {:?})",
                    summary.run.frames,
                    summary.run.ticks,
                    summary.run.events,
                    summary.run.backend,
                    summary.run.extent.0,
                    summary.run.extent.1,
                    // What the window system actually did, not what
                    // `--fullscreen` asked for. It is free to refuse.
                    summary.run.mode,
                    summary.scene.name(),
                    // And which of topic 18's effects were in those frames,
                    // resolved — the observable for this sample's own
                    // `[engine.video]` wiring.
                    summary.effects.row(),
                    summary.run.exit,
                );
                if let Some(timings) = summary.simulation {
                    let millis =
                        |nanos| std::time::Duration::from_nanos(nanos).as_secs_f64() * 1_000.0;
                    println!(
                        "sandbox simulation (p50 / p95 over the last {} ticks): complete {:.3} / \
                         {:.3} ms, move+refit {:.3} / {:.3} ms, queries {:.3} / {:.3} ms",
                        timings.samples,
                        millis(timings.complete_ns.0),
                        millis(timings.complete_ns.1),
                        millis(timings.refit_ns.0),
                        millis(timings.refit_ns.1),
                        millis(timings.query_ns.0),
                        millis(timings.query_ns.1),
                    );
                }
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
