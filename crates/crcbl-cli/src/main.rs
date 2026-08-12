//! `crcbl` — the Crucible engine's headless control CLI.
//!
//! `docs/plan/11-cli-headless.md` makes this a first-class pillar rather than a
//! convenience: **everything the engine and the editor can do must be reachable
//! without a window**, from a terminal, a script, a CI job or a coding agent.
//! The invariant it protects is that no capability is implemented GUI-side, and
//! the way it is protected is that the CLI is the thing CI runs.
//!
//! P0 ships the first three subcommands the topic schedules — `new`, `run`,
//! `build` — plus the design rules that are not per-command and would be
//! expensive to retrofit:
//!
//! | Rule | Where |
//! | --- | --- |
//! | `--json` on every subcommand, human output otherwise | `report::emit` |
//! | Exit 0 ok / 1 failed / 2 bad invocation | `report`, and `main` below |
//! | No prompt unless a TTY, and never required | `new` |
//! | Stable JSON schemas | `json`, and each command's field list |
//!
//! `scene`, `import`, `screenshot`, `sim`, `phys` and `edit` land from P1
//! onwards; the argument parser is written so adding one is a `match` arm.
//!
//! # This binary depends on the engine for GPU subcommands
//!
//! `new`, `run`, and `build` are pure-Cargo subcommands with no engine deps.
//! `screenshot` links the GPU backend; the binary's Cargo.toml adds the `crcbl`
//! umbrella to serve it.

mod args;
mod cargo;
mod crpix_cmd;
mod json;
mod lod_cmd;
mod new;
mod replay_cmd;
mod report;
mod screenshot;

use std::process::ExitCode;

use crate::args::{Command, Invocation};
use crate::report::EXIT_USAGE;

fn main() -> ExitCode {
    // `args_os`, not `args`: the latter panics on an argument that is not valid
    // Unicode, and `--path`, `--engine`, `-o` and `replay`'s file all take
    // paths, which on Linux are bytes. See `args`'s module docs.
    match args::parse(std::env::args_os().skip(1)) {
        Invocation::Command(command) => {
            let json = command.json();
            let name = command.name();
            let result = match &command {
                Command::New(args) => new::run(args),
                Command::Run(args) => cargo::run(args),
                Command::Build(args) => cargo::build(args),
                Command::Screenshot(args) => screenshot::run(args),
                Command::Replay(args) => replay_cmd::run(args),
                Command::Crpix(args) => crpix_cmd::run(args),
                Command::Lod(args) => lod_cmd::run(args),
            };
            report::emit(name, json, result)
        }
        Invocation::Help(text) => {
            println!("{text}");
            ExitCode::SUCCESS
        }
        Invocation::Version => {
            println!("crcbl {}", env!("CARGO_PKG_VERSION"));
            ExitCode::SUCCESS
        }
        Invocation::BadUsage(message) => {
            // Bad invocations are the one thing with no `--json` rendering: the
            // flag itself may be what failed to parse, so there is no reliable
            // way to know it was asked for.
            eprintln!("crcbl: {message}");
            eprintln!("\n{}", args::USAGE);
            ExitCode::from(EXIT_USAGE)
        }
    }
}
