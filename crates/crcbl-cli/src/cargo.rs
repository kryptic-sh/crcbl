//! `crcbl run` and `crcbl build` — driving Cargo, and refusing wasm honestly.
//!
//! # Decision: shell out to Cargo rather than reimplement it
//!
//! A Crucible project *is* a Cargo project — `crcbl new` writes a manifest and
//! nothing more — so `crcbl run` is `cargo run` with the arguments a game
//! needs, plus this CLI's exit-code contract. What the wrapper buys is not
//! abstraction over Cargo; it is that `crcbl run --headless`, `crcbl build` and
//! (from P1) `crcbl screenshot` are one vocabulary, so a CI file and an agent
//! learn one command surface instead of one per phase.
//!
//! Cargo is invoked through `$CARGO` when it is set, which it always is when
//! this binary was itself started by Cargo, so a `cargo +nightly` or a rustup
//! shim does not get lost on the way down.
//!
//! # Decision: `--target wasm` fails, and names what to run instead
//!
//! P5 has shipped and the samples deploy to Pages, so the refusal is no longer
//! "not yet" — it is that **a browser bundle is not a `cargo build`**. Getting
//! one means `cargo build --target wasm32-unknown-unknown`, then the loader
//! beside the artifact, the shim, the shader artifacts and the site layout.
//! (It used to mean a `wasm-bindgen` whose version had to match the crate the
//! build resolved; the browser backend imports nothing, so the tool is gone.)
//! `web/build.sh` is that
//! pipeline and is what CI runs; a `crcbl build --target wasm` that shelled out
//! to Cargo alone would exit 0 having produced something no page can load,
//! which is the one outcome worse than refusing.
//!
//! The flag stays *recognized*, because "unknown target `wasm`" and "use
//! `web/build.sh`" send a reader to two different places — one to check the
//! spelling, the other to the tool that does the job. It exits 1 (the command
//! could not be carried out), not 2 (the invocation was fine), and `--json`
//! carries `"use": "web/build.sh"` so a script can branch on it without
//! matching prose.

use std::path::{Path, PathBuf};
use std::process::Command;

use crate::args::{BuildArgs, RunArgs, Target};
use crate::json::Json;
use crate::report::{Failure, Outcome};

/// Runs the project in the current directory.
///
/// # Errors
///
/// [`Failure`] if there is no Cargo project here, if Cargo could not be
/// started, or if it exited non-zero.
pub fn run(args: &RunArgs) -> Result<Outcome, Failure> {
    let manifest = locate_manifest()?;
    let mut command = cargo("run", &manifest);
    if let Some(package) = &args.package {
        command.args(["--package", package]);
    }
    if args.release {
        command.arg("--release");
    }
    // Everything from here belongs to the game. `--headless` first so an
    // explicit passthrough can still override it if a game wants that.
    command.arg("--");
    if args.headless {
        command.arg("--headless");
    }
    command.args(&args.passthrough);

    let described = describe(&command);
    execute(&mut command, &described)?;
    Ok(Outcome {
        human: format!("ran {}", manifest.display()),
        json: vec![
            ("manifest", Json::string(manifest.display().to_string())),
            ("headless", Json::Bool(args.headless)),
            ("invocation", Json::string(&described)),
            // Zero by construction: `execute` turns every other code into a
            // `Failure` that carries the real one. The field stays so the
            // success and failure objects have the same shape.
            ("cargo_exit_code", Json::Number(0)),
        ],
    })
}

/// Builds the project in the current directory.
///
/// # Errors
///
/// [`Failure`] if the target is not buildable yet, if there is no Cargo project
/// here, or if Cargo exited non-zero.
pub fn build(args: &BuildArgs) -> Result<Outcome, Failure> {
    if args.target == Target::Wasm {
        return Err(Failure::new(
            "a browser bundle is not a cargo build: run `web/build.sh`, which pairs the \
             wasm32 build with its loader, the shim and the site layout",
        )
        .with("target", Json::string("wasm"))
        .with("use", Json::string("web/build.sh")));
    }

    let manifest = locate_manifest()?;
    let mut command = cargo("build", &manifest);
    if let Some(package) = &args.package {
        command.args(["--package", package]);
    }
    if args.release {
        command.arg("--release");
    }

    let described = describe(&command);
    execute(&mut command, &described)?;
    Ok(Outcome {
        human: format!("built {}", manifest.display()),
        json: vec![
            ("manifest", Json::string(manifest.display().to_string())),
            ("target", Json::string("native")),
            (
                "profile",
                Json::string(if args.release { "release" } else { "debug" }),
            ),
            ("invocation", Json::string(&described)),
            // See `run`: zero by construction.
            ("cargo_exit_code", Json::Number(0)),
        ],
    })
}

/// A `cargo <subcommand> --manifest-path <manifest>` invocation.
///
/// `--manifest-path` rather than a working-directory change, so `crcbl run`
/// works from anywhere inside a project and the reported path is unambiguous.
/// It goes *after* the subcommand, which is the only place Cargo accepts it —
/// a global-looking flag that is in fact per-subcommand.
fn cargo(subcommand: &str, manifest: &Path) -> Command {
    let program = std::env::var_os("CARGO").unwrap_or_else(|| "cargo".into());
    let mut command = Command::new(program);
    command.arg(subcommand).arg("--manifest-path").arg(manifest);
    command
}

/// Runs `command` with inherited stdio and maps its status onto this CLI's
/// exit-code contract.
///
/// Inherited, not captured: a build is a stream of progress a developer wants
/// to watch, and buffering it to re-print at the end would break every
/// `--json`-less use for the sake of one. With `--json` the object is printed
/// *after* Cargo's own output, on stdout — the convention `cargo --message-
/// format=json` set.
///
/// Returns `()` rather than the exit code: success *is* code zero, every other
/// code is a [`Failure`] carrying it, and a `Result<i32, _>` whose `Ok` arm can
/// only ever hold `0` invited callers to report a number they had not checked.
fn execute(command: &mut Command, described: &str) -> Result<(), Failure> {
    let status = command.status().map_err(|error| {
        Failure::new(format!("could not start `{described}`: {error}"))
            .with("invocation", Json::string(described))
    })?;
    match status.code() {
        Some(0) => Ok(()),
        Some(code) => Err(
            Failure::new(format!("`{described}` failed with exit code {code}"))
                .with("invocation", Json::string(described))
                .with("cargo_exit_code", Json::Number(i64::from(code))),
        ),
        // Unix only: killed by a signal, which has no exit code of its own.
        None => Err(
            Failure::new(format!("`{described}` was killed by a signal"))
                .with("invocation", Json::string(described)),
        ),
    }
}

/// The nearest `Cargo.toml`, searching upwards from the working directory.
///
/// Upwards because `crcbl run` from `src/` is something people do, and because
/// a game with a `scenes/` directory invites being run from inside it.
///
/// `pub(crate)` for [`settings_cmd`](crate::settings_cmd), which derives the
/// game's name from the same project these two build and run. A second search
/// of its own would be a second answer to "which project is this", and the two
/// would drift.
pub(crate) fn locate_manifest() -> Result<PathBuf, Failure> {
    let cwd = std::env::current_dir()
        .map_err(|error| Failure::new(format!("cannot read the current directory: {error}")))?;
    let mut candidate: Option<&Path> = Some(cwd.as_path());
    while let Some(directory) = candidate {
        let manifest = directory.join("Cargo.toml");
        if manifest.is_file() {
            return Ok(manifest);
        }
        candidate = directory.parent();
    }
    Err(Failure::new(format!(
        "no Cargo.toml at or above {}\n\
         hint: run this inside a project, or create one with `crcbl new <NAME>`.",
        cwd.display()
    )))
}

/// The command line, for logs and for the `command` JSON field.
///
/// Not shell-quoted: it is a description, not something to paste back into a
/// shell, and pretending otherwise would need per-platform quoting rules.
fn describe(command: &Command) -> String {
    let mut described = command.get_program().to_string_lossy().into_owned();
    for argument in command.get_args() {
        described.push(' ');
        described.push_str(&argument.to_string_lossy());
    }
    described
}

#[cfg(test)]
mod tests {
    use super::*;

    fn run_args() -> RunArgs {
        RunArgs {
            headless: false,
            package: None,
            release: false,
            json: false,
            passthrough: Vec::new(),
        }
    }

    /// `--headless` has to reach the *game*, after `--`, or it is silently a
    /// Cargo flag that does not exist.
    #[test]
    fn headless_is_passed_through_to_the_game_not_to_cargo() {
        let args = RunArgs {
            headless: true,
            passthrough: vec!["--frames".into(), "3".into()],
            ..run_args()
        };
        let mut command = cargo("run", Path::new("/p/Cargo.toml"));
        command.arg("--");
        if args.headless {
            command.arg("--headless");
        }
        command.args(&args.passthrough);
        let described = describe(&command);
        assert!(
            described.contains("-- --headless --frames 3"),
            "{described}"
        );
    }

    /// The refusal has to name the tool that *does* the job, not a phase.
    ///
    /// It used to say "the wasm target lands at P5", which stopped being true
    /// when P5 shipped and six samples started deploying to Pages — a refusal
    /// whose stated reason has expired reads as a bug in the CLI rather than a
    /// signpost. The machine-readable half is asserted too, because a script
    /// branching on `--json` is the caller least able to notice prose drift.
    #[test]
    fn the_wasm_target_is_refused_and_points_at_the_bundle_script() {
        let failure = build(&BuildArgs {
            target: Target::Wasm,
            package: None,
            release: false,
            json: false,
        })
        .expect_err("a browser bundle is not a cargo build");
        assert!(
            failure.message.contains("web/build.sh"),
            "{}",
            failure.message
        );
        assert!(
            !failure.message.contains("P5"),
            "the refusal must not name a phase that has shipped: {}",
            failure.message
        );
        assert_eq!(failure.code, crate::report::EXIT_FAILED);
        assert!(
            failure
                .json
                .contains(&("use", Json::string("web/build.sh")))
        );
    }

    #[test]
    fn a_described_command_names_the_manifest() {
        let command = cargo("build", Path::new("/projects/mygame/Cargo.toml"));
        assert_eq!(
            describe(&command).split_whitespace().last(),
            Some("/projects/mygame/Cargo.toml")
        );
    }
}
